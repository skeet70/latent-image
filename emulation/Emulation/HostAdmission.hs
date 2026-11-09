-- | Host resource classes and admitted Bazel commands.
--
-- 'Build' admits one Bazel command at a time host-wide: a POSIX record lock on
-- @~/.cache/xm1-host-build/build.lock@, held for the whole command, which runs
-- as @bazel --batch@ in a systemd user scope capped at 'classMemoryMax'.
-- 'Emulator' is the slot pool of "Emulation.EmulatorSlot". The worst case on the
-- 30.4 GiB host, one 6 GiB build and two 8 GiB emulator runs, leaves 8.4 GiB for
-- the OS, interactive work and Nix daemon builders, which run in
-- @nix-daemon.service@ outside every class and are not capped.
module Emulation.HostAdmission
  ( ResourceClass (..)
  , MemoryMax (..)
  , classConcurrency
  , classMemoryMax
  , parseMemoryMax
  , memoryMaxBytes
  , userScopeAvailable
  , hostBuildMain
  ) where

import Control.Concurrent (forkIO, threadDelay)
import Control.Concurrent.MVar (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, evaluate, try)
import Control.Monad (unless, void, when)
import Data.Char (isDigit, isSpace)
import Data.List (dropWhileEnd, isPrefixOf, stripPrefix)
import Data.Maybe (isNothing)
import GHC.Clock (getMonotonicTime)
import Numeric (showFFloat)
import System.Directory (createDirectoryIfMissing, findExecutable, getHomeDirectory, removeFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, takeDirectory, takeFileName, (</>))
import System.IO (SeekMode (AbsoluteSeek), hFlush, hGetContents, hGetLine, hPutStrLn, stderr)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.IO (FdOption (CloseOnExec), LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, createPipe, defaultFileFlags, fdToHandle, openFd, setFdOption, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Signals (Handler (Catch), installHandler, sigINT)
import System.Posix.Types (Fd (..))
import System.Process (CreateProcess (..), createProcess, getProcessExitCode, proc, readProcessWithExitCode, terminateProcess, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

data ResourceClass = Build | Emulator

-- | A systemd @MemoryMax=@ value: decimal bytes with an optional K, M, G or T suffix.
newtype MemoryMax = MemoryMax String

-- | Simultaneous admissions host-wide.
classConcurrency :: ResourceClass -> Int
classConcurrency Build = 1
classConcurrency Emulator = 2

-- | The memory cap of one admission; an emulator run may ask for less.
classMemoryMax :: ResourceClass -> MemoryMax
classMemoryMax Build = MemoryMax "6G"
classMemoryMax Emulator = MemoryMax "8G"

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
  lock <- acquireBuildLock directory
  waited <- subtract waitStart <$> getMonotonicTime
  checkConfiguredAccounting
  scoped <- userScopeAvailable
  let MemoryMax memory = classMemoryMax Build
  pid <- getProcessID
  let unit = "xm1-host-build-" ++ show pid ++ ".scope"
  if scoped
    then stopPreviousScope directory unit
    else hPutStrLn stderr ("No systemd user session: host build runs without the " ++ memory ++ " memory cap") >> hFlush stderr
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

-- | Parallelism within the build scope; Bazel's resource estimate excludes its
-- own 2 GiB heap.
admittedFlags :: String -> [String]
admittedFlags command
  | command `elem` ["build", "test", "run", "coverage"] = ["--jobs=4", "--local_resources=memory=3072"]
  | otherwise = []

acquireBuildLock :: FilePath -> IO Fd
acquireBuildLock directory = do
  fd <- openFd (directory </> "build.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
  setFdOption fd CloseOnExec True
  let attempt announced = do
        locked <- try (setLock fd (WriteLock, AbsoluteSeek, 0, 0)) :: IO (Either IOException ())
        case locked of
          Right () -> pure fd
          Left _ -> do
            unless announced $ do
              hPutStrLn stderr ("Waiting for the host build lock in " ++ directory)
              hFlush stderr
            threadDelay 1_000_000
            attempt True
  attempt False

-- | Stops the scope of this lock's previous holder when its processes outlived
-- the holder, then records @unit@ as the current holder's scope.
stopPreviousScope :: FilePath -> String -> IO ()
stopPreviousScope directory unit = do
  let record = directory </> "scope"
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
  (_, _, _, child) <- createProcess (proc program arguments)
  released <- newEmptyMVar
  _ <- forkIO $ do
    _ <- try (hGetContents releaseHandle >>= evaluate . length) :: IO (Either IOException Int)
    status <- getProcessExitCode child
    when (isNothing status) $
      if unit == "-"
        then terminateProcess child
        else void (readProcessWithExitCode "systemctl" ["--user", "stop", "--no-block", unit] "")
    putMVar released ()
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
