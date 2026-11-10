-- | Host-wide class limits and a shared 20 GiB managed-memory budget.
module Emulation.HostAdmission
  ( ResourceClass (..)
  , MemoryMax (..)
  , classConcurrency
  , classMemoryMax
  , parseMemoryMax
  , memoryMaxBytes
  , userScopeAvailable
  , hostBuildMain
  , ScopeName
  , parseScopeName
  , reserveMemory
  , claimScopeRecord
  ) where

import Control.Concurrent (forkIO, threadDelay)
import Control.Concurrent.MVar (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, catch, evaluate, try)
import Control.Monad (forM_, unless, void, when)
import qualified Data.ByteString as ByteString
import qualified Data.ByteString.Char8 as ByteString8
import Data.Char (isDigit, isPrint, isSpace)
import Data.List (dropWhileEnd, isPrefixOf, nub, stripPrefix)
import Data.Maybe (isNothing)
import GHC.Clock (getMonotonicTime)
import FujiAF.StorageBudget (StorageFailure, checkStorageBudget)
import Numeric (showFFloat)
import System.Directory (createDirectoryIfMissing, findExecutable, getHomeDirectory, removeFile, renameFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, takeDirectory, takeFileName, (</>))
import System.IO (IOMode (ReadMode), SeekMode (AbsoluteSeek), hFlush, hGetContents, hGetLine, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (readSymbolicLink)
import System.Posix.IO (FdOption (CloseOnExec), FileLock, LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, createPipe, defaultFileFlags, fdToHandle, getLock, openFd, setFdOption, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Signals (Handler (Catch), installHandler, sigINT)
import System.Posix.Types (Fd (..), ProcessID)
import System.Process (CreateProcess (..), createProcess, proc, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

data ResourceClass = Build | Emulator | NativeVerification

-- | A systemd @MemoryMax=@ value: decimal bytes with an optional K, M, G or T suffix.
newtype MemoryMax = MemoryMax String

-- | Simultaneous admissions host-wide.
classConcurrency :: ResourceClass -> Int
classConcurrency Build = 2
classConcurrency Emulator = 4
classConcurrency NativeVerification = 8

-- | The memory cap of one admission; an emulator run may ask for less.
classMemoryMax :: ResourceClass -> MemoryMax
classMemoryMax Build = MemoryMax "6G"
classMemoryMax Emulator = MemoryMax "4G"
classMemoryMax NativeVerification = MemoryMax "2G"

newtype ScopeName = ScopeName String
  deriving (Eq)

parseScopeName :: String -> Maybe ScopeName
parseScopeName text
  | not (null text), all (\c -> c `elem` (['a'..'z'] ++ ['A'..'Z'] ++ ['0'..'9'] ++ "-_.")) text
  , reverse (take 6 (reverse text)) == ".scope" = Just (ScopeName text)
  | otherwise = Nothing

-- | Reservations are acquired inside an already-started scope.
reserveMemory :: MemoryMax -> ScopeName -> IO Fd
reserveMemory memory scope = do
  let quantum = 256 * 1024 * 1024
      tokens = fromInteger ((memoryMaxBytes memory + quantum - 1) `div` quantum)
  unless (tokens > 0 && tokens <= 80) $
    ioError (userError "managed memory reservation must fit the 20 GiB host budget")
  directory <- (</> ".cache" </> "xm1-host-memory") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  let attempt = do
        gate <- openFd (directory </> "gate.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
        entered <- try (setLock gate (WriteLock, AbsoluteSeek, 0, 1)) :: IO (Either IOException ())
        case entered of
          Left _ -> closeFd gate >> threadDelay 100_000 >> attempt
          Right () -> do
            fd <- openFd (directory </> "memory.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
            setFdOption fd CloseOnExec False
            chosen <- collect fd tokens [0 .. 79] []
            case chosen of
              Nothing -> do
                closeFd fd
                closeFd gate
                announceMemory directory tokens
                threadDelay 500_000
                attempt
              Just indices -> do
                claimScopeRecords [directory </> ("scope-" ++ show index) | index <- indices] scope
                closeFd gate
                pure fd
      collect _ 0 _ selected = pure (Just selected)
      collect _ _ [] _ = pure Nothing
      collect fd count (index : rest) selected = do
        result <- try (setLock fd (WriteLock, AbsoluteSeek, index, 1)) :: IO (Either IOException ())
        case result of
          Right () -> collect fd (count - 1) rest (index : selected)
          Left _ -> collect fd count rest selected
  attempt

announceMemory :: FilePath -> Int -> IO ()
announceMemory directory tokens = do
  fd <- openFd (directory </> "memory.lock") ReadWrite defaultFileFlags
  owners <- mapM (\index -> getLock fd (WriteLock, AbsoluteSeek, index, 1)) [0 .. 79]
  closeFd fd
  hPutStrLn stderr ("host admission: waiting for managed memory (" ++ show tokens ++ " x 256MiB of 20GiB); owners=" ++ show (nub [pid | Just (pid, _) <- owners]))
  hFlush stderr

claimScopeRecord :: FilePath -> ScopeName -> IO ()
claimScopeRecord record = claimScopeRecords [record]

claimScopeRecords :: [FilePath] -> ScopeName -> IO ()
claimScopeRecords records scope@(ScopeName unit) = do
  previous <- mapM readRecord records
  mapM_ reapScope (nub [prior | Just prior <- previous, prior /= scope])
  pid <- getProcessID
  forM_ records $ \record -> do
    let temporary = record ++ "." ++ show pid
    writeFile temporary (unit ++ "\n")
    renameFile temporary record
  where
    readRecord record = do
      previous <- (Just <$> (readFile record >>= \text -> evaluate (length text) >> pure (trim text)))
        `catchIOError` \problem -> if isDoesNotExistError problem then pure Nothing else ioError problem
      case previous of
        Just name -> case parseScopeName name of
          Just prior -> pure (Just prior)
          Nothing -> ioError (userError ("invalid admission scope record: " ++ record))
        Nothing -> pure Nothing

scopeActive :: ScopeName -> IO Bool
scopeActive (ScopeName unit) = do
  (_, out, _) <- readProcessWithExitCode "systemctl" ["--user", "show", unit, "-p", "ActiveState", "--value"] ""
  case trim out of
    state | state `elem` ["active", "activating", "deactivating"] -> pure True
    state | state `elem` ["inactive", "failed"] -> pure False
    _ -> ioError (userError ("cannot establish scope state: " ++ unit))

reapScope :: ScopeName -> IO ()
reapScope scope@(ScopeName unit) = do
  active <- scopeActive scope
  when active $ do
    (code, _, err) <- readProcessWithExitCode "systemctl" ["--user", "stop", unit] ""
    unless (code == ExitSuccess) (ioError (userError ("cannot reap scope " ++ unit ++ ": " ++ err)))
    remaining <- scopeActive scope
    when remaining (ioError (userError ("scope still has work: " ++ unit)))

parseMemoryMax :: String -> Maybe MemoryMax
parseMemoryMax text = case span isDigit text of
  (digits@(_ : _), suffix) | suffix `elem` ["", "K", "M", "G", "T"], read digits > (0 :: Integer) ->
    Just (MemoryMax (digits ++ suffix))
  _ -> Nothing

memoryMaxBytes :: MemoryMax -> Integer
memoryMaxBytes (MemoryMax text) = read digits * scale suffix
  where
    (digits, suffix) = span isDigit text
    scale "K" = 1024
    scale "M" = 1024 ^ (2 :: Int)
    scale "G" = 1024 ^ (3 :: Int)
    scale "T" = 1024 ^ (4 :: Int)
    scale _ = 1

-- | Whether @systemd-run --user@ can start a scope: the binary exists and the
-- user manager answers within five seconds.
userScopeAvailable :: IO Bool
userScopeAvailable = do
  runner <- findExecutable "systemd-run"
  case runner of
    Nothing -> pure False
    Just _ -> do
      result <- timeout 5_000_000 (try (readProcessWithExitCode "systemctl" ["--user", "show-environment"] ""))
        :: IO (Maybe (Either IOException (ExitCode, String, String)))
      pure $ case result of
        Just (Right (ExitSuccess, _, _)) -> True
        _ -> False

-- | Set in every admitted command's environment.
admittedVariable :: String
admittedVariable = "XM1_HOST_BUILD"

resourceFailureExit :: Int
resourceFailureExit = 75

newtype AccountingExecutable = AccountingExecutable FilePath
newtype AccountingConfiguration = AccountingConfiguration FilePath
newtype AccountingStateDirectory = AccountingStateDirectory FilePath
data AccountingGuard = AccountingGuard AccountingExecutable AccountingConfiguration AccountingStateDirectory

configuredAccountingGuard :: IO (Maybe AccountingGuard)
configuredAccountingGuard = do
  config <- lookupEnv "XM1_STORAGE_ACCOUNTING_CONFIG"
  state <- lookupEnv "XM1_STORAGE_ACCOUNTING_STATE"
  program <- lookupEnv "XM1_STORAGE_ACCOUNTING_TOOL"
  case (config, state, program) of
    (Nothing, Nothing, Nothing) -> pure Nothing
    (Just configuration, Just directory, executable) -> do
      fallback <- (</> ".local/libexec/latent-image/storage-accounting") <$> getHomeDirectory
      let tool = maybe fallback id executable
      unless (all isAbsolute [configuration, directory, tool]) $
        refuseAccounting "accounting config, state and executable must be absolute paths"
      pure (Just (AccountingGuard (AccountingExecutable tool) (AccountingConfiguration configuration) (AccountingStateDirectory directory)))
    _ -> refuseAccounting "set both XM1_STORAGE_ACCOUNTING_CONFIG and XM1_STORAGE_ACCOUNTING_STATE to opt into aggregate admission"

refuseAccounting :: String -> IO a
refuseAccounting problem = do
  hPutStrLn stderr ("host-build storage admission: " ++ take 4096 problem)
  exitWith (ExitFailure resourceFailureExit)

checkConfiguredAccounting :: IO ()
checkConfiguredAccounting = do
  guard <- configuredAccountingGuard
  case guard of
    Nothing -> pure ()
    Just (AccountingGuard (AccountingExecutable program) (AccountingConfiguration config) (AccountingStateDirectory state)) -> do
      result <- try (readProcessWithExitCode program ["check", "--config", config, "--state", state, "+RTS", "-M512M", "-RTS"] "")
        :: IO (Either IOException (ExitCode, String, String))
      case result of
        Right (ExitSuccess, _, _) -> pure ()
        Right (_, out, err) -> refuseAccounting (err ++ out)
        Left problem -> refuseAccounting (show problem)

-- | @host-build [--output_base=PATH] COMMAND [ARGUMENT...]@, or the internal
-- scope child.
hostBuildMain :: [String] -> IO ()
hostBuildMain arguments = case arguments of
  ["--reserve-legacy", name] | Just scope <- parseScopeName name -> reserveLegacy scope
  "--scope-child" : report : release : unit : program : rest
    | Just reportFd <- readMaybe report, Just releaseFd <- readMaybe release ->
        scopeChild (Fd reportFd) (Fd releaseFd) unit program rest
  option : command : rest
    | Just base <- stripPrefix "--output_base=" option, not (null base), not ("-" `isPrefixOf` command) ->
        admit (Just base) command rest
  command : rest | not ("-" `isPrefixOf` command) -> admit Nothing command rest
  _ -> do
    hPutStrLn stderr "usage: host-build [--output_base=PATH] COMMAND [ARGUMENT...]  (a Bazel command such as build, test or run)"
    exitWith (ExitFailure 2)

admit :: Maybe FilePath -> String -> [String] -> IO ()
admit requestedBase command rest = do
  refuseNested
  outputBase <- maybe worktreeOutputBase (pure . Just) requestedBase
  directory <- (</> ".cache" </> "xm1-host-build") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  waitStart <- getMonotonicTime
  (lock, lane) <- acquireBuildLock directory
  waited <- subtract waitStart <$> getMonotonicTime
  scoped <- userScopeAvailable
  pid <- getProcessID
  let unit = "xm1-host-build-" ++ show pid ++ ".scope"
  if scoped
    then stopPreviousScope directory lane unit
    else ioError (userError "host-build requires a systemd user scope to enforce its memory cap")
  checkStorageBudget directory
    `catch` (\problem -> refuseAccounting (show (problem :: StorageFailure)))
    `catch` (\problem -> refuseAccounting (show (problem :: IOException)))
  checkConfiguredAccounting
  let script = directory </> ("run-" ++ show pid ++ ".sh")
      running = command == "run"
      bazelArguments =
        ["--batch"]
          ++ maybe [] (\base -> ["--output_base=" ++ base]) outputBase
          ++ [command]
          ++ admittedFlags command
          ++ ["--script_path=" ++ script | running]
          ++ rest
  when running (removeIfPresent script)
  setEnv admittedVariable "1"
  outcome <- runContained scoped unit bazelArguments
  let verdict = judge outcome
  hPutStrLn stderr ("host-build verdict " ++ renderVerdict command waited verdict outcome)
  hFlush stderr
  case (verdict, running) of
    (Passed, True) -> do
      launcher <- readFile script
      _ <- evaluate (length launcher)
      removeFile script
      closeFd lock
      unsetEnv admittedVariable
      executeFile "bash" True ["-c", launcher, "bash"] Nothing
    _ -> do
      when running (removeIfPresent script)
      exitWith (verdictExit verdict outcome)

-- | Waiting for the build lock while holding an emulator slot, or from inside an
-- admitted command, can deadlock.
refuseNested :: IO ()
refuseNested = do
  slot <- lookupEnv "XM1_EMULATOR_SLOT"
  nested <- lookupEnv admittedVariable
  let refuse reason = hPutStrLn stderr ("host-build: " ++ reason) >> exitWith (ExitFailure 2)
  case (slot, nested) of
    (Just _, _) -> refuse "refusing to wait for the build lock while holding an emulator slot; build first, then take the slot"
    (_, Just _) -> refuse "refusing a nested admission inside an admitted host build"
    _ -> pure ()

-- | @~/.cache/bazel/latent-image-NAME@ for a worktree at
-- @latent-image-worktrees/NAME@, as @agent-worktree.sh@ prints; Bazel's
-- default output base otherwise.
worktreeOutputBase :: IO (Maybe FilePath)
worktreeOutputBase = do
  result <- try (readProcessWithExitCode "git" ["rev-parse", "--show-toplevel"] "") :: IO (Either IOException (ExitCode, String, String))
  home <- getHomeDirectory
  pure $ case result of
    Right (ExitSuccess, out, _)
      | top <- trim out, takeFileName (takeDirectory top) == "latent-image-worktrees" ->
          Just (home </> ".cache" </> "bazel" </> ("latent-image-" ++ takeFileName top))
    _ -> Nothing

reserveLegacy :: ScopeName -> IO ()
reserveLegacy scope@(ScopeName unit) = do
  refuseNested
  (code, out, _) <- readProcessWithExitCode "systemctl"
    ["--user", "show", unit, "-p", "ActiveState", "-p", "MemoryMax"] ""
  unless (code == ExitSuccess && "ActiveState=active" `elem` lines out && "MemoryMax=4294967296" `elem` lines out) $
    ioError (userError "legacy reservation requires an active scope capped at exactly 4 GiB")
  _ <- reserveMemory (MemoryMax "4G") scope
  hPutStrLn stderr ("legacy reservation ready: " ++ unit ++ " 4GiB")
  hFlush stderr
  let wait = do
        (_, state, _) <- readProcessWithExitCode "systemctl" ["--user", "show", unit, "-p", "ActiveState", "--value"] ""
        when (trim state `elem` ["active", "activating", "deactivating"]) (threadDelay 1_000_000 >> wait)
  wait

-- | Parallelism within the build scope; Bazel's resource estimate excludes its
-- own 2 GiB heap.
admittedFlags :: String -> [String]
admittedFlags command
  | command `elem` ["build", "test", "run", "coverage"] = ["--jobs=4", "--local_resources=memory=3072"]
  | otherwise = []

-- | F_GETLK reports one conflicting lock at query time.
data KernelOwner = KernelOwner ProcessID | KernelUnlocked | KernelUnknown
  deriving (Eq)

data HolderDetails = HolderDetails
  { holderCommand :: Maybe String
  , holderDirectory :: Maybe FilePath
  , holderOutputBase :: OutputBase
  }

data OutputBase = NoOutputBase | OutputBaseUnavailable | OutputBaseObserved FilePath

data OwnerSample = OwnerNone | OwnerUnknown | OwnerReported ProcessID (Maybe HolderDetails)

newtype BuildLane = BuildLane Int

acquireBuildLock :: FilePath -> IO (Fd, BuildLane)
acquireBuildLock directory = do
  fd <- openFd (directory </> "build.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
  setFdOption fd CloseOnExec True
  let firstFree [] = pure Nothing
      firstFree (lane : rest) = do
        locked <- try (setLock fd (WriteLock, AbsoluteSeek, fromIntegral lane, 1)) :: IO (Either IOException ())
        case locked of
          Right () -> pure (Just (BuildLane lane))
          Left _ -> firstFree rest
      attempt announced = do
        available <- firstFree [0 .. classConcurrency Build - 1]
        case available of
          Just lane -> pure (fd, lane)
          Nothing -> do
            announced' <- announceContention directory fd announced
            threadDelay 1_000_000
            attempt announced'
  attempt Nothing

-- | Samples metadata only on first contention and observed owner changes.
announceContention :: FilePath -> Fd -> Maybe KernelOwner -> IO (Maybe KernelOwner)
announceContention directory fd previous = do
  current <- queryKernelOwner fd
  if previous == Just current
    then pure previous
    else case current of
      KernelOwner pid -> do
        details <- holderDetails pid
        confirmed <- queryKernelOwner fd
        announce confirmed (if confirmed == current then details else Nothing)
      _ -> announce current Nothing
  where
    announce owner details
      | previous == Just owner = pure previous
      | otherwise = do
          (hPutStrLn stderr (renderContention directory (sampleOf owner details)) >> hFlush stderr)
            `catchIOError` \_ -> pure ()
          pure (Just owner)

sampleOf :: KernelOwner -> Maybe HolderDetails -> OwnerSample
sampleOf (KernelOwner pid) details = OwnerReported pid details
sampleOf KernelUnlocked _ = OwnerNone
sampleOf KernelUnknown _ = OwnerUnknown

queryKernelOwner :: Fd -> IO KernelOwner
queryKernelOwner fd = do
  result <- try (getLock fd (WriteLock, AbsoluteSeek, 0, 1))
    :: IO (Either IOException (Maybe (ProcessID, FileLock)))
  pure $ case result of
    Right (Just (pid, _)) | pid > 0 -> KernelOwner pid
    Right (Just _) -> KernelUnknown
    Right Nothing -> KernelUnlocked
    Left _ -> KernelUnknown

-- | Bounded metadata for a sampled owner.
holderDetails :: ProcessID -> IO (Maybe HolderDetails)
holderDetails pid = do
  command <- processName pid
  directory <- try (readSymbolicLink ("/proc" </> show pid </> "cwd")) :: IO (Either IOException FilePath)
  outputBase <- processOutputBase pid
  let details = HolderDetails
        command
        (either (const Nothing) (Just . sanitizeText 512) directory)
        outputBase
  pure $ case details of
    HolderDetails Nothing Nothing NoOutputBase -> Nothing
    _ -> Just details

processName :: ProcessID -> IO (Maybe String)
processName pid = do
  contents <- boundedBytes 64 ("/proc" </> show pid </> "comm")
  pure (sanitizeText 64 . trim . ByteString8.unpack <$> contents)

outputBaseFlag :: ByteString.ByteString
outputBaseFlag = ByteString8.pack "--output_base="

processOutputBase :: ProcessID -> IO OutputBase
processOutputBase pid = do
  contents <- boundedBytes 4096 ("/proc" </> show pid </> "cmdline")
  pure $ case contents of
    Nothing -> OutputBaseUnavailable
    Just bytes ->
      let values =
            [ value
            | argument <- completeArguments bytes
            , Just value <- [ByteString8.stripPrefix outputBaseFlag argument]
            , not (ByteString.null value)
            ]
          truncated = outputBaseFlag `ByteString.isPrefixOf` incompleteTail bytes
      in case reverse values of
        value : _ | ByteString.length value > 512 -> OutputBaseUnavailable
                  | truncated -> OutputBaseUnavailable
                  | not (ByteString.all (\byte -> byte >= 32 && byte <= 126) value) -> OutputBaseUnavailable
                  | otherwise -> OutputBaseObserved (ByteString8.unpack value)
        [] | truncated -> OutputBaseUnavailable
           | otherwise -> NoOutputBase

-- | Only NUL-terminated argv tokens are complete.
completeArguments :: ByteString.ByteString -> [ByteString.ByteString]
completeArguments bytes = filter (not . ByteString.null) (dropLast (ByteString.split 0 bytes))
  where
    dropLast [] = []
    dropLast [_] = []
    dropLast (argument : rest) = argument : dropLast rest

incompleteTail :: ByteString.ByteString -> ByteString.ByteString
incompleteTail = snd . ByteString.breakEnd (== 0)

boundedBytes :: Int -> FilePath -> IO (Maybe ByteString.ByteString)
boundedBytes limit path = do
  result <- try (withBinaryFile path ReadMode (\handle -> ByteString.hGet handle (limit + 1)))
    :: IO (Either IOException ByteString.ByteString)
  pure (either (const Nothing) Just result)

sanitizeText :: Int -> String -> String
sanitizeText limit = take limit . map (\character -> if isPrint character then character else '?')

renderContention :: FilePath -> OwnerSample -> String
renderContention directory sample =
  "host-build: waiting for build lock in " ++ sanitizeText 512 directory ++ "; " ++ holderText sample

holderText :: OwnerSample -> String
holderText OwnerNone = "holder=none"
holderText OwnerUnknown = "holder=unknown"
holderText (OwnerReported pid Nothing) = "holder pid=" ++ show pid ++ " details=unavailable"
holderText (OwnerReported pid (Just details)) =
  "holder pid=" ++ show pid
    ++ maybe "" (\name -> " command=" ++ name) (holderCommand details)
    ++ maybe "" (\path -> " cwd=" ++ path) (holderDirectory details)
    ++ outputBaseText (holderOutputBase details)

outputBaseText :: OutputBase -> String
outputBaseText NoOutputBase = ""
outputBaseText OutputBaseUnavailable = " output_base=unavailable"
outputBaseText (OutputBaseObserved base) =
  " output_base=" ++ base ++ " command_log=" ++ (base </> "command.log")

-- | Stops the scope of this lock's previous holder when its processes outlived
-- the holder, then records @unit@ as the current holder's scope.
stopPreviousScope :: FilePath -> BuildLane -> String -> IO ()
stopPreviousScope directory (BuildLane lane) unit = do
  let record = directory </> (if lane == 0 then "scope" else "scope-" ++ show lane)
  previous <- try (readFile record >>= \text -> evaluate (length text) >> pure (trim text)) :: IO (Either IOException String)
  case previous of
    Right name | "xm1-host-build-" `isPrefixOf` name -> do
      (_, out, _) <- readProcessWithExitCode "systemctl" ["--user", "show", name, "-p", "ActiveState"] ""
      when (trim out `elem` ["ActiveState=active", "ActiveState=activating", "ActiveState=deactivating"]) $ do
        hPutStrLn stderr ("host-build: stopping " ++ name ++ ", left running by the lock's previous holder")
        void (readProcessWithExitCode "systemctl" ["--user", "stop", name] "")
      void (readProcessWithExitCode "systemctl" ["--user", "reset-failed", name] "")
    _ -> pure ()
  writeFile record (unit ++ "\n")

data UnitState = UnitState
  { unitResult :: String
  , unitCpuNanos :: Maybe Integer
  , unitMemoryPeak :: Maybe Integer
  }

data DaemonCounters = DaemonCounters
  { daemonOomKills :: Maybe Integer
  , daemonCpuMicros :: Maybe Integer
  }

data Outcome = Outcome
  { outcomeExit :: Maybe Int
  , outcomeUnit :: Maybe String
  , outcomeState :: UnitState
  , outcomeWall :: Double
  , outcomeDaemonOom :: Maybe Integer
  , outcomeDaemonCpu :: Maybe Integer
  }

-- | Runs Bazel through a scope child that reports Bazel's exit status and
-- stays alive until the unit's result has been read; systemd collects a
-- successful scope once its last process exits.
runContained :: Bool -> String -> [String] -> IO Outcome
runContained scoped unit bazelArguments = do
  before <- daemonCounters
  start <- getMonotonicTime
  (reportRead, reportWrite) <- createPipe
  (releaseRead, releaseWrite) <- createPipe
  setFdOption reportRead CloseOnExec True
  setFdOption releaseWrite CloseOnExec True
  self <- getExecutablePath
  let MemoryMax memory = classMemoryMax Build
      child = ["--scope-child", fdText reportWrite, fdText releaseRead, if scoped then unit else "-"] ++ ("bazel" : bazelArguments)
      process
        | scoped = proc "systemd-run" $
            [ "--user", "--scope", "--quiet", "--unit=" ++ unit
            , "--property=MemoryMax=" ++ memory
            , "--property=MemorySwapMax=0"
            , "--property=OOMPolicy=stop"
            , "--", self
            ] ++ child
        | otherwise = proc self child
  _ <- installHandler sigINT (Catch (pure ())) Nothing
  (_, _, _, handle) <- createProcess process {close_fds = False}
  closeFd reportWrite
  closeFd releaseRead
  reportHandle <- fdToHandle reportRead
  reported <- try (hGetLine reportHandle) :: IO (Either IOException String)
  end <- getMonotonicTime
  let exitStatus = either (const Nothing) (readMaybe . trim) reported
  state <-
    if scoped
      then unitState unit (isNothing exitStatus)
      else pure (UnitState "uncapped" Nothing Nothing)
  after <- daemonCounters
  closeFd releaseWrite
  _ <- waitForProcess handle
  when scoped (void (readProcessWithExitCode "systemctl" ["--user", "reset-failed", unit] ""))
  pure Outcome
    { outcomeExit = exitStatus
    , outcomeUnit = if scoped then Just unit else Nothing
    , outcomeState = state
    , outcomeWall = end - start
    , outcomeDaemonOom = (-) <$> daemonOomKills after <*> daemonOomKills before
    , outcomeDaemonCpu = (-) <$> daemonCpuMicros after <*> daemonCpuMicros before
    }

fdText :: Fd -> String
fdText (Fd fd) = show fd

-- | The scope's result. A unit whose processes all died is waited for until it
-- settles; a collected unit's result is unknown.
unitState :: String -> Bool -> IO UnitState
unitState unit died = do
  when died (settle (80 :: Int))
  properties <- showUnit
  let field name = lookup name properties
      number name = field name >>= readMaybe
  pure $ case field "LoadState" of
    Just "loaded" -> UnitState (maybe "unknown" id (field "Result")) (number "CPUUsageNSec") (number "MemoryPeak")
    _ -> UnitState "unknown" Nothing Nothing
  where
    showUnit = do
      (_, out, _) <- readProcessWithExitCode "systemctl"
        ["--user", "show", unit, "-p", "LoadState", "-p", "ActiveState", "-p", "Result", "-p", "CPUUsageNSec", "-p", "MemoryPeak"] ""
      pure [(key, drop 1 value) | line <- lines out, let (key, value) = break (== '=') line]
    settle 0 = pure ()
    settle remaining = do
      properties <- showUnit
      when (lookup "ActiveState" properties `elem` map Just ["active", "activating", "deactivating"]) $ do
        threadDelay 250_000
        settle (remaining - 1)

daemonCounters :: IO DaemonCounters
daemonCounters = do
  let cgroup = "/sys/fs/cgroup/system.slice/nix-daemon.service"
  events <- readCounter (cgroup </> "memory.events") "oom_kill"
  cpu <- readCounter (cgroup </> "cpu.stat") "usage_usec"
  pure (DaemonCounters events cpu)
  where
    readCounter path key = do
      contents <- try (readFile path >>= \text -> evaluate (length text) >> pure text) :: IO (Either IOException String)
      pure $ case contents of
        Right text -> lookup key [(name, value) | [name, raw] <- map words (lines text), Just value <- [readMaybe raw]]
        Left _ -> Nothing

-- | Runs inside the scope. Reports Bazel's exit status on @report@ and exits
-- once @release@ reaches end of file; an end of file while Bazel runs means
-- the admitting process died, and the scope is stopped.
scopeChild :: Fd -> Fd -> String -> String -> [String] -> IO ()
scopeChild report release unit program arguments = do
  _ <- installHandler sigINT (Catch (pure ())) Nothing
  setFdOption report CloseOnExec True
  setFdOption release CloseOnExec True
  reportHandle <- fdToHandle report
  releaseHandle <- fdToHandle release
  scope <- maybe (ioError (userError "scope child requires a valid scope name")) pure (parseScopeName unit)
  released <- newEmptyMVar
  _ <- forkIO $ do
    _ <- try (hGetContents releaseHandle >>= evaluate . length) :: IO (Either IOException Int)
    void (readProcessWithExitCode "systemctl" ["--user", "stop", "--no-block", unit] "")
    putMVar released ()
  _ <- reserveMemory (classMemoryMax Build) scope
  (_, _, _, child) <- createProcess (proc program arguments)
  code <- waitForProcess child
  _ <- try (hPutStrLn reportHandle (show (exitNumber code)) >> hFlush reportHandle) :: IO (Either IOException ())
  takeMVar released
  exitWith code

exitNumber :: ExitCode -> Int
exitNumber ExitSuccess = 0
exitNumber (ExitFailure code) = code

data Verdict = Passed | Failed | Interrupted | ResourceFailure String | Unknown

judge :: Outcome -> Verdict
judge outcome = case (outcomeExit outcome, unitResult (outcomeState outcome)) of
  (_, "oom-kill") -> ResourceFailure "oom-kill"
  (exit, _) | exit /= Just 0, maybe False (> 0) (outcomeDaemonOom outcome) -> ResourceFailure "nix-daemon-oom-kill"
  (Just 0, _) -> Passed
  (Nothing, _) -> Unknown
  (_, "unknown") -> Unknown
  (Just 8, _) -> Interrupted
  _ -> Failed

verdictExit :: Verdict -> Outcome -> ExitCode
verdictExit verdict outcome = case verdict of
  Passed -> ExitSuccess
  ResourceFailure _ -> ExitFailure resourceFailureExit
  _ -> case outcomeExit outcome of
    Just code | code > 0 && code < 256 -> ExitFailure code
    Just code | code < 0 -> ExitFailure (128 - code)
    _ -> ExitFailure 1

renderVerdict :: String -> Double -> Verdict -> Outcome -> String
renderVerdict command waited verdict outcome =
  "{" ++ commaSeparated
    [ field "verdict" (quoted verdictName)
    , field "resource_failure" resource
    , field "command" (quoted (filter safe command))
    , field "exit_code" (maybe "null" show (outcomeExit outcome))
    , field "unit" (maybe "null" quoted (outcomeUnit outcome))
    , field "unit_result" (quoted (filter safe (unitResult state)))
    , field "memory_max" (quoted memory)
    , field "lock_wait_seconds" (seconds waited)
    , field "wall_seconds" (seconds (outcomeWall outcome))
    , field "scope_cpu_seconds" (maybe "null" (\nanos -> seconds (fromIntegral nanos / 1e9)) (unitCpuNanos state))
    , field "scope_memory_peak_bytes" (maybe "null" show (unitMemoryPeak state))
    , field "nix_daemon_cpu_seconds" (maybe "null" (\micros -> seconds (fromIntegral micros / 1e6)) (outcomeDaemonCpu outcome))
    , field "nix_daemon_oom_kills" (maybe "null" show (outcomeDaemonOom outcome))
    ] ++ "}"
  where
    state = outcomeState outcome
    MemoryMax memory = classMemoryMax Build
    (verdictName, resource) = case verdict of
      Passed -> ("passed", "null")
      Failed -> ("failed", "null")
      Interrupted -> ("interrupted", "null")
      ResourceFailure kind -> ("resource-failure", quoted kind)
      Unknown -> ("unknown", quoted "unknown")
    field name value = quoted name ++ ":" ++ value
    quoted text = "\"" ++ text ++ "\""
    safe character = character /= '"' && character /= '\\' && not (isSpace character)
    seconds value = showFFloat (Just 3) (value :: Double) ""
    commaSeparated = foldr1 (\left right -> left ++ "," ++ right)

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = removeFile path `catchIOError` \problem ->
  unless (isDoesNotExistError problem) (ioError problem)
