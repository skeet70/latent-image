-- | Host-wide class limits and a shared budget of physical RAM less a host reserve.
module Emulation.HostAdmission
  ( ResourceClass (..)
  , MemoryMax (..)
  , MemorySwapMax (..)
  , classMemorySwapMax
  , parseMemorySwapMax
  , memorySwapMaxBytes
  , configuredMemory
  , classConcurrency
  , classMemoryMax
  , parseMemoryMax
  , memoryMaxBytes
  , userScopeAvailable
  , hostBuildMain
  , ScopeName
  , parseScopeName
  , MemoryKey
  , memoryKey
  , Reservation
  , reservationBytes
  , expectedReservation
  , reserveMemory
  , startPeakRecorder
  , claimScopeRecord
  , lockOwnerText
  ) where

import Control.Concurrent (forkIO, threadDelay)
import Control.Concurrent.MVar (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, bracket, catch, evaluate, onException, try)
import Control.Monad (forM, forM_, unless, void, when)
import qualified Data.ByteString as ByteString
import qualified Data.ByteString.Char8 as ByteString8
import Data.Char (isDigit, isHexDigit, isPrint, isSpace)
import Data.Either (isRight)
import Data.Function (on)
import Data.List (dropWhileEnd, isPrefixOf, isSuffixOf, nub, nubBy, sort, sortOn, stripPrefix)
import Data.Maybe (fromMaybe, isJust, isNothing)
import Data.Ord (Down (..))
import GHC.Clock (getMonotonicTime)
import FujiAF.StorageBudget (StorageFailure, checkStorageBudget)
import System.Info (os)
import Numeric (showFFloat)
import System.Directory (createDirectoryIfMissing, doesFileExist, findExecutable, getHomeDirectory, getModificationTime, listDirectory, removeFile, renameFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (IOMode (ReadMode), SeekMode (AbsoluteSeek), hClose, hFlush, hGetContents, hGetLine, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (fileID, getFdStatus, readSymbolicLink)
import System.Posix.IO (FdOption (CloseOnExec), FileLock, LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadOnly, ReadWrite), closeFd, createPipe, defaultFileFlags, fdToHandle, getLock, openFd, setFdOption, setLock, waitToSetLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Signals (Handler (Catch), installHandler, sigINT)
import System.Posix.Types (Fd (..), FileID, FileOffset, ProcessID)
import System.Process (CreateProcess (..), StdStream (NoStream, UseHandle), createProcess, proc, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

data ResourceClass = Build | Emulator | NativeVerification

-- | A systemd @MemoryMax=@ value: decimal bytes with an optional K, M, G or T suffix.
newtype MemoryMax = MemoryMax String

newtype MemorySwapMax = MemorySwapMax String

-- | Simultaneous admissions host-wide.
classConcurrency :: ResourceClass -> Int
classConcurrency Build = 2
classConcurrency Emulator = 4
classConcurrency NativeVerification = 8

-- | The memory cap of one admission; an emulator run may ask for less.
classMemoryMax :: ResourceClass -> MemoryMax
classMemoryMax Build = MemoryMax "12G"
classMemoryMax Emulator = MemoryMax "8G"
classMemoryMax NativeVerification = MemoryMax "12G"

classMemorySwapMax :: ResourceClass -> MemorySwapMax
classMemorySwapMax _ = MemorySwapMax "8G"

configuredMemory :: ResourceClass -> IO (MemoryMax, MemorySwapMax)
configuredMemory resource = do
  let prefix = case resource of
        Build -> "XM1_HOST_BUILD"
        Emulator -> "XM1_EMULATOR"
        NativeVerification -> "XM1_NATIVE"
      memoryName = prefix ++ "_MEMORY_MAX"
      swapName = prefix ++ "_SWAP_MAX"
      limit = memoryMaxBytes (MemoryMax "32G")
  rawMemory <- lookupEnv memoryName
  memory <- case rawMemory of
    Nothing -> pure (classMemoryMax resource)
    Just text | Just value <- parseMemoryMax text, memoryMaxBytes value <= limit -> pure value
    _ -> ioError (userError (memoryName ++ " must be positive and at most 32G"))
  rawSwap <- lookupEnv swapName
  swap <- case rawSwap of
    Nothing -> pure (classMemorySwapMax resource)
    Just text | Just value <- parseMemorySwapMax text, memorySwapMaxBytes value <= limit -> pure value
    _ -> ioError (userError (swapName ++ " must be between 0 and 32G"))
  budget <- memoryBudget
  refuseOverBudget budget memory
  pure (memory, swap)

-- | Bytes per lock offset in @memory.lock@.
tokenBytes :: Integer
tokenBytes = 256 * 1024 * 1024

-- | Offsets 0..255 of @memory.lock@, which launchers that reserved RAM plus swap
-- from a fixed 64 GiB budget may still hold.
legacyTokenRange :: Int
legacyTokenRange = 256

budgetVariable :: String
budgetVariable = "XM1_HOST_MEMORY_BUDGET"

-- | RAM left outside the token budget for the desktop, browser, sessions and page cache.
hostReserveBytes :: Integer
hostReserveBytes = 8 * 1024 * 1024 * 1024

-- | @MemAvailable@ an admission leaves free beyond its reservation.
liveHeadroomBytes :: Integer
liveHeadroomBytes = 3 * 1024 * 1024 * 1024

-- | Admissible tokens: physical RAM less 'hostReserveBytes' (at least one
-- token), or a lower @XM1_HOST_MEMORY_BUDGET@. Offsets below 'censusTokens'
-- are counted when held and are the only ones allocated.
data MemoryBudget = MemoryBudget
  { budgetTokens :: Int
  , censusTokens :: Int
  }

memoryBudget :: IO MemoryBudget
memoryBudget = do
  physical <- physicalMemoryBytes
  let defaultBudget = max 1 ((physical - hostReserveBytes) `div` tokenBytes) * tokenBytes
  requested <- lookupEnv budgetVariable
  admissible <- case requested of
    Nothing -> pure defaultBudget
    Just text | Just value <- parseMemoryMax text
      , memoryMaxBytes value >= tokenBytes, memoryMaxBytes value <= defaultBudget -> pure (memoryMaxBytes value)
    _ -> ioError (userError (budgetVariable ++ " must be between 256M and physical RAM less the 8 GiB host reserve ("
      ++ show defaultBudget ++ " bytes)"))
  pure (MemoryBudget (fromInteger (admissible `div` tokenBytes)) (max legacyTokenRange (fromInteger (physical `div` tokenBytes))))

-- | @/proc/meminfo@, or the file named by @XM1_HOST_MEMINFO@.
meminfoBytes :: String -> IO Integer
meminfoBytes field = do
  path <- fromMaybe "/proc/meminfo" <$> lookupEnv "XM1_HOST_MEMINFO"
  text <- readStrict path
  case [kib | [name, value, "kB"] <- map words (lines text), name == field ++ ":", Just kib <- [readMaybe value]] of
    [kib] | kib >= 0 -> pure (kib * 1024)
    _ -> ioError (userError ("cannot read " ++ field ++ " from " ++ path))

-- | Linux @MemTotal@ or macOS @hw.memsize@.
physicalMemoryBytes :: IO Integer
physicalMemoryBytes = case os of
  "linux" -> do
    total <- meminfoBytes "MemTotal"
    if total > 0 then pure total else ioError (userError "MemTotal is zero")
  "darwin" -> do
    (code, out, _) <- readProcessWithExitCode "/usr/sbin/sysctl" ["-n", "hw.memsize"] ""
    case (code, readMaybe (trim out)) of
      (ExitSuccess, Just bytes) | bytes > 0 -> pure bytes
      _ -> ioError (userError "cannot read hw.memsize")
  other -> ioError (userError ("no physical memory probe for " ++ other))

-- | Linux @MemAvailable@. Reservations happen only inside systemd scopes.
availableMemoryBytes :: IO Integer
availableMemoryBytes = case os of
  "linux" -> meminfoBytes "MemAvailable"
  other -> ioError (userError ("no available memory probe for " ++ other))

reservationTokens :: MemoryMax -> Int
reservationTokens memory = fromInteger ((memoryMaxBytes memory + tokenBytes - 1) `div` tokenBytes)

refuseOverBudget :: MemoryBudget -> MemoryMax -> IO ()
refuseOverBudget budget memory@(MemoryMax text) =
  unless (reservationTokens memory <= budgetTokens budget) $
    ioError (userError ("MemoryMax " ++ text ++ " exceeds the host RAM budget of "
      ++ show (toInteger (budgetTokens budget) * tokenBytes) ++ " bytes (physical RAM less the 8 GiB host reserve, or "
      ++ budgetVariable ++ ")"))

newtype ScopeName = ScopeName String
  deriving (Eq)

parseScopeName :: String -> Maybe ScopeName
parseScopeName text
  | not (null text), all (\c -> c `elem` (['a'..'z'] ++ ['A'..'Z'] ++ ['0'..'9'] ++ "-_.")) text
  , reverse (take 6 (reverse text)) == ".scope" = Just (ScopeName text)
  | otherwise = Nothing

-- | A history key: a slot run's label, or a host build's command and targets.
-- Its first word names its class: @full-@, @native-@ or @host-build@.
newtype MemoryKey = MemoryKey String

memoryKey :: String -> MemoryKey
memoryKey = MemoryKey . take 1024 . map (\character -> if isPrint character && character /= '\t' then character else '?')

newtype KeyClass = KeyClass String
  deriving (Eq)

keyClass :: String -> KeyClass
keyClass = KeyClass . takeWhile (/= ' ')

-- | Whole tokens of RAM a scope holds against the budget; also its @MemoryHigh@.
newtype Reservation = Reservation Int

reservationBytes :: Reservation -> Integer
reservationBytes (Reservation tokens) = toInteger tokens * tokenBytes

memoryDirectory :: IO FilePath
memoryDirectory = do
  directory <- (</> ".cache" </> "xm1-host-memory") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  pure directory

-- | Peak records not yet folded into @history@: a peak line, then a key line.
pendingPeakDirectory :: FilePath -> FilePath
pendingPeakDirectory directory = directory </> "peaks"

historyLimit :: Int
historyLimit = 1024

-- | 1.25 times the recorded peak of KEY's most recent completed scope or, for
-- a key without a record, the 90th-percentile recorded peak of its class,
-- rounded up to tokens and at most MEMORY; MEMORY when the class has no record.
expectedReservation :: MemoryKey -> MemoryMax -> IO Reservation
expectedReservation (MemoryKey key) memory = do
  directory <- memoryDirectory
  history <- bracket (openFd (directory </> "history.lock") ReadWrite defaultFileFlags {creat = Just 0o600}) closeFd $ \fd -> do
    setFdOption fd CloseOnExec True
    waitToSetLock fd (WriteLock, AbsoluteSeek, 0, 0)
    foldPendingPeaks directory
  let cap = reservationTokens memory
      classPeaks = sort [peak | (peak, entry) <- history, keyClass entry == keyClass key]
      classEstimate
        | null classPeaks = Nothing
        | otherwise = Just (classPeaks !! ((9 * length classPeaks + 9) `div` 10 - 1))
      expected = maybe classEstimate Just (lookup key [(entry, peak) | (peak, entry) <- history])
  pure $ Reservation $ case expected of
    Nothing -> cap
    Just peak -> max 1 (min cap (fromInteger ((peak * 5 + 4 * tokenBytes - 1) `div` (4 * tokenBytes))))

-- | Most recent first, one entry per key. Caller holds @history.lock@.
foldPendingPeaks :: FilePath -> IO [(Integer, String)]
foldPendingPeaks directory = do
  let historyPath = directory </> "history"
      pending = pendingPeakDirectory directory
  stored <- (Just <$> readStrict historyPath)
    `catchIOError` \problem -> if isDoesNotExistError problem then pure Nothing else ioError problem
  let history = [(peak, drop 1 key) | line <- maybe [] lines stored, let (rawPeak, key) = break (== '\t') line, Just peak <- [readMaybe rawPeak]]
  names <- listDirectory pending
    `catchIOError` \problem -> if isDoesNotExistError problem then pure [] else ioError problem
  records <- fmap concat . forM [name | name <- names, not (".partial" `isSuffixOf` name)] $ \name -> do
    let path = pending </> name
    stamp <- getModificationTime path
    text <- readStrict path
    removeFile path
    pure [(stamp, (peak, key)) | [rawPeak, key] <- [lines text], Just peak <- [readMaybe rawPeak]]
  if null records then pure history else do
    let merged = take historyLimit (nubBy ((==) `on` snd) (map snd (sortOn (Down . fst) records) ++ history))
    pid <- getProcessID
    let temporary = historyPath ++ "." ++ show pid
    writeFile temporary (unlines [show peak ++ "\t" ++ key | (peak, key) <- merged])
    renameFile temporary historyPath
    pure merged

writePendingPeak :: MemoryKey -> String -> Integer -> IO ()
writePendingPeak (MemoryKey key) name peak = do
  pending <- pendingPeakDirectory <$> memoryDirectory
  createDirectoryIfMissing True pending
  let final = pending </> name
      temporary = final ++ ".partial"
  writeFile temporary (unlines [show peak, key])
  renameFile temporary final

-- | Records this scope's @memory.peak@ under KEY once this process, after any
-- exec, and every descendant that inherits the pipe have exited. A scope
-- stopped by systemd leaves no record.
startPeakRecorder :: MemoryKey -> ScopeName -> IO ()
startPeakRecorder (MemoryKey key) (ScopeName unit) = do
  groups <- readStrict "/proc/self/cgroup"
  group <- case [drop 3 line | line <- lines groups, take 3 line == "0::"] of
    [found] -> pure found
    _ -> ioError (userError "cannot establish this process's cgroup")
  pending <- pendingPeakDirectory <$> memoryDirectory
  createDirectoryIfMissing True pending
  (watchRead, watchWrite) <- createPipe
  setFdOption watchRead CloseOnExec True
  setFdOption watchWrite CloseOnExec False
  watch <- fdToHandle watchRead
  let final = pending </> unit
      script = "while read -r _; do :; done; read -r peak < \"$1\" || exit 0; printf '%s\\n%s\\n' \"$peak\" \"$4\" > \"$2\" && mv -f \"$2\" \"$3\""
  void $ createProcess (proc "sh" ["-c", script, "sh", "/sys/fs/cgroup" ++ group ++ "/memory.peak", final ++ ".partial", final, key])
    { std_in = UseHandle watch, std_out = NoStream, std_err = NoStream, close_fds = True }

-- | Reserves the scope's @MemoryHigh@, or its @MemoryMax@ when that is lower
-- or @MemoryHigh@ is unset, inside the already-started scope. Swap caps are
-- not reserved.
reserveMemory :: ScopeName -> IO (Fd, Reservation)
reserveMemory scope@(ScopeName unit) = do
  (_, out, _) <- readProcessWithExitCode "systemctl" ["--user", "show", unit, "-p", "MemoryMax", "-p", "MemoryHigh"] ""
  let properties = [(key, drop 1 value) | line <- lines out, let (key, value) = break (== '=') line]
      cap = lookup "MemoryMax" properties >>= parseMemoryMax
  reservation <- case (cap, lookup "MemoryHigh" properties) of
    (Just limit, Just "infinity") -> pure (Reservation (reservationTokens limit))
    (Just limit, Just rawHigh) | Just high <- parseMemoryMax rawHigh ->
      pure (Reservation (reservationTokens (if memoryMaxBytes high < memoryMaxBytes limit then high else limit)))
    _ -> ioError (userError ("cannot read the memory limits of " ++ unit))
  fd <- reserveMemoryWithLegacy Nothing reservation scope
  pure (fd, reservation)

newtype InvocationId = InvocationId String
  deriving (Eq)

newtype LegacyReservation = LegacyReservation ScopeIdentity
data ScopeCgroup = ScopeCgroup FilePath FileID
  deriving (Eq)
data ScopeIdentity = ScopeIdentity ScopeName InvocationId ScopeCgroup
  deriving (Eq)
data ScopeIdentityState = ScopeIdentityActive InvocationId MemoryMax | ScopeIdentityInactive

-- | Offset stride in @waiting.lock@: a waiter locks @tokens * stride + pid@,
-- so every lock below its own belongs to a smaller or earlier-numbered waiter.
waitingStride :: FileOffset
waitingStride = 4194304

-- | Polls a waiter that fits spends deferring to smaller registered waiters.
-- A smaller waiter that fits takes its tokens within one poll; one that stays
-- registered longer does not fit under its own budget and is passed.
smallerWaiterPolls :: Int
smallerWaiterPolls = 3

-- | A new admission takes its tokens when they fit the budget, when
-- @MemAvailable@ less 'liveHeadroomBytes' covers them, and after deferring to
-- smaller waiters for 'smallerWaiterPolls'. A legacy scope already holds its
-- memory and skips the last two conditions.
reserveMemoryWithLegacy :: Maybe LegacyReservation -> Reservation -> ScopeName -> IO Fd
reserveMemoryWithLegacy legacy reservation@(Reservation tokens) scope = do
  budget <- memoryBudget
  unless (tokens <= budgetTokens budget) $
    ioError (userError ("a reservation of " ++ show (reservationBytes reservation) ++ " bytes exceeds the host RAM budget of "
      ++ show (toInteger (budgetTokens budget) * tokenBytes) ++ " bytes"))
  directory <- memoryDirectory
  pid <- getProcessID
  queue <- openFd (directory </> "waiting.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
  setFdOption queue CloseOnExec True
  let place = fromIntegral tokens * waitingStride + fromIntegral pid
      admitting = isNothing legacy
      attempt registered deferred = do
        gate <- openFd (directory </> "gate.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
        entered <- try (setLock gate (WriteLock, AbsoluteSeek, 0, 1)) :: IO (Either IOException ())
        case entered of
          Left _ -> closeFd gate >> threadDelay 100_000 >> attempt registered deferred
          Right () -> do
            protected <- protectedLegacyTokens directory budget
            case legacy of
              Nothing -> pure ()
              Just owner -> do
                recorded <- doesFileExist (legacyReservationPath directory owner)
                when recorded (ioError (userError "this legacy invocation already has a persistent reservation"))
            fd <- openFd (directory </> "memory.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
            setFdOption fd CloseOnExec False
            holders <- heldTokens fd budget
            spare <- if admitting then subtract liveHeadroomBytes <$> availableMemoryBytes else pure (reservationBytes reservation)
            smaller <- if admitting then isJust <$> getLock queue (WriteLock, AbsoluteSeek, 0, place) else pure False
            let occupied = nub (map fst holders ++ protected)
                free = filter (`notElem` occupied) (map fromIntegral [0 .. censusTokens budget - 1])
                fits = length occupied + tokens <= budgetTokens budget && spare >= reservationBytes reservation
                deferring = fits && smaller && deferred < smallerWaiterPolls
            chosen <- if fits && not deferring
              then collect fd (take tokens free)
              else pure Nothing
            case chosen of
              Nothing -> do
                closeFd fd
                closeFd gate
                when (admitting && not registered) (setLock queue (WriteLock, AbsoluteSeek, place, 1))
                hPutStrLn stderr ("host admission: waiting for host RAM (" ++ show tokens ++ " x 256MiB; "
                  ++ show (length occupied) ++ " of " ++ show (budgetTokens budget) ++ " held; "
                  ++ show (max 0 spare) ++ " bytes available above the live headroom"
                  ++ (if deferring then "; deferring to a smaller reservation" else "") ++ "); owners="
                  ++ show (nub (map snd holders)))
                hFlush stderr
                threadDelay 500_000
                attempt (registered || admitting) (if deferring then deferred + 1 else 0)
              Just indices -> do
                let records = [directory </> ("scope-" ++ show index) | index <- indices]
                case legacy of
                  Nothing -> claimScopeRecords records scope
                  Just owner -> do
                    current <- currentScopeIdentity scope
                    reclaimScopeRecords records current
                    writeLegacyReservation directory owner indices
                closeFd gate
                closeFd queue
                pure fd
      collect fd indices = do
        results <- mapM (\index -> try (setLock fd (WriteLock, AbsoluteSeek, index, 1)) :: IO (Either IOException ())) indices
        pure (if all isRight results then Just indices else Nothing)
  attempt False 0

-- | Offsets below 'censusTokens' locked by other processes, with their owners.
heldTokens :: Fd -> MemoryBudget -> IO [(FileOffset, ProcessID)]
heldTokens fd budget = do
  owners <- mapM (\index -> (,) index <$> getLock fd (WriteLock, AbsoluteSeek, index, 1))
    (map fromIntegral [0 .. censusTokens budget - 1])
  pure [(index, pid) | (index, Just (pid, _)) <- owners]

claimScopeRecord :: FilePath -> ScopeName -> IO ()
claimScopeRecord record = claimScopeRecords [record]

claimScopeRecords :: [FilePath] -> ScopeName -> IO ()
claimScopeRecords records scope = do
  current <- currentScopeIdentity scope
  reclaimScopeRecords records current
  pid <- getProcessID
  forM_ records $ \record -> do
    let temporary = record ++ "." ++ show pid
    writeFile temporary (unlines (scopeIdentityLines current))
    renameFile temporary record

currentScopeIdentity :: ScopeName -> IO ScopeIdentity
currentScopeIdentity scope@(ScopeName unit) = do
  state <- scopeIdentityState scope
  invocation <- case state of
    ScopeIdentityActive identity _ -> pure identity
    ScopeIdentityInactive -> ioError (userError "cannot claim an inactive admission scope")
  (_, out, _) <- readProcessWithExitCode "systemctl" ["--user", "show", unit, "-p", "ControlGroup", "--value"] ""
  let group = trim out
  unless (validCgroupPath unit group) (ioError (userError ("invalid scope cgroup: " ++ unit)))
  bracket (openFd ("/sys/fs/cgroup" ++ group) ReadOnly defaultFileFlags) closeFd $ \fd -> do
    setFdOption fd CloseOnExec True
    inode <- fileID <$> getFdStatus fd
    confirmed <- scopeIdentityState scope
    case confirmed of
      ScopeIdentityActive current _ | current == invocation -> pure (ScopeIdentity scope invocation (ScopeCgroup group inode))
      _ -> ioError (userError "scope invocation changed during admission")

scopeIdentityLines :: ScopeIdentity -> [String]
scopeIdentityLines (ScopeIdentity (ScopeName unit) (InvocationId invocation) (ScopeCgroup group inode)) =
  [unit, invocation, group, show inode]

parseScopeIdentity :: [String] -> Maybe ScopeIdentity
parseScopeIdentity [unit, rawInvocation, group, rawInode]
  | Just scope <- parseScopeName unit
  , Just invocation <- parseInvocationId rawInvocation
  , validCgroupPath unit group
  , Just inode <- readMaybe rawInode
  , inode > 0 = Just (ScopeIdentity scope invocation (ScopeCgroup group inode))
parseScopeIdentity _ = Nothing

validCgroupPath :: String -> FilePath -> Bool
validCgroupPath unit group =
  isAbsolute group && takeFileName group == unit && ".." `notElem` splitDirectories group

readScopeRecord :: FilePath -> IO (Maybe ScopeIdentity)
readScopeRecord record = do
  previous <- (Just <$> (readFile record >>= \text -> evaluate (length text) >> pure text))
    `catchIOError` \problem -> if isDoesNotExistError problem then pure Nothing else ioError problem
  case previous of
    Nothing -> pure Nothing
    Just text | ["expired-build-scope", unit] <- lines text
      , "xm1-host-build-" `isPrefixOf` unit
      , Just _ <- parseScopeName unit -> pure Nothing
    Just text -> case parseScopeIdentity (lines text) of
      Just identity -> pure (Just identity)
      Nothing -> ioError (userError ("scope record lacks original cgroup identity; migrate before admission: " ++ record))

reclaimScopeRecords :: [FilePath] -> ScopeIdentity -> IO ()
reclaimScopeRecords records current = do
  previous <- mapM readScopeRecord records
  mapM_ reapScope (nub [prior | Just prior <- previous, prior /= current])

withOriginalCgroup :: ScopeCgroup -> IO a -> (Fd -> IO a) -> IO a
withOriginalCgroup (ScopeCgroup group expectedInode) absent action =
  bracket open (mapM_ closeFd) (maybe absent action)
  where
    open = do
      descriptor <- (Just <$> openFd ("/sys/fs/cgroup" ++ group) ReadOnly defaultFileFlags)
        `catchIOError` \problem -> if isDoesNotExistError problem then pure Nothing else ioError problem
      case descriptor of
        Nothing -> pure Nothing
        Just fd -> do
          inode <- (fileID <$> getFdStatus fd) `onException` closeFd fd
          if inode /= expectedInode
            then closeFd fd >> pure Nothing
            else setFdOption fd CloseOnExec True >> pure (Just fd)

emptyCgroup :: Fd -> IO Bool
emptyCgroup fd = (do
  contents <- readFile ("/proc/self/fd" </> fdText fd </> "cgroup.events")
  _ <- evaluate (length contents)
  case [value | ["populated", value] <- map words (lines contents)] of
    ["0"] -> pure True
    ["1"] -> pure False
    _ -> ioError (userError "cannot establish original scope cgroup population"))
  `catchIOError` \problem -> if isDoesNotExistError problem then pure True else ioError problem

sameInvocationActive :: ScopeIdentity -> IO Bool
sameInvocationActive (ScopeIdentity scope expected cgroup) = do
  ended <- withOriginalCgroup cgroup (pure True) emptyCgroup
  if ended then pure False else do
    state <- scopeIdentityState scope
    case state of
      ScopeIdentityActive invocation _ | invocation == expected -> pure True
      _ -> ioError (userError "populated original cgroup no longer has its recorded scope invocation")

reapScope :: ScopeIdentity -> IO ()
reapScope owner@(ScopeIdentity (ScopeName unit) _ cgroup) =
  withOriginalCgroup cgroup (pure ()) $ \fd -> do
    confirmed <- sameInvocationActive owner
    when confirmed $ do
      let settle 0 = ioError (userError ("scope still has work: " ++ unit))
          settle remaining = do
            ended <- emptyCgroup fd
            unless ended (threadDelay 50_000 >> settle (remaining - 1))
      killed <- try (writeFile ("/proc/self/fd" </> fdText fd </> "cgroup.kill") "1\n") :: IO (Either IOException ())
      case killed of
        Left problem -> emptyCgroup fd >>= \ended -> unless ended (ioError problem)
        Right () -> pure ()
      settle (200 :: Int)

parseMemoryMax :: String -> Maybe MemoryMax
parseMemoryMax text = case span isDigit text of
  (digits@(_ : _), suffix) | suffix `elem` ["", "K", "M", "G", "T"], read digits > (0 :: Integer) ->
    Just (MemoryMax (digits ++ suffix))
  _ -> Nothing

parseMemorySwapMax :: String -> Maybe MemorySwapMax
parseMemorySwapMax text = case span isDigit text of
  (digits@(_ : _), suffix) | suffix `elem` ["", "K", "M", "G", "T"] ->
    Just (MemorySwapMax (digits ++ suffix))
  _ -> Nothing

memorySwapMaxBytes :: MemorySwapMax -> Integer
memorySwapMaxBytes (MemorySwapMax text) = memoryMaxBytes (MemoryMax text)

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
  ["--migrate-build-records"] -> migrateBuildRecords
  ["--reserve-legacy", name] | Just scope <- parseScopeName name -> reserveLegacy scope
  "--scope-child" : report : release : unit : program : rest
    | Just reportFd <- readMaybe report, Just releaseFd <- readMaybe release ->
        scopeChild (Fd reportFd) (Fd releaseFd) unit program rest
  option : command : rest
    | Just base <- stripPrefix "--output_base=" option, not (null base), not ("-" `isPrefixOf` command) ->
        admit (Just base) command rest
  command : rest | not ("-" `isPrefixOf` command) -> admit Nothing command rest
  _ -> do
    hPutStrLn stderr "usage: host-build [--output_base=PATH] COMMAND [ARGUMENT...]\n       host-build --reserve-legacy UNIT.scope\n       host-build --migrate-build-records"
    exitWith (ExitFailure 2)

-- | The scope reserves its memory before this process takes a build lane, so a
-- build waiting for memory holds no lane.
admit :: Maybe FilePath -> String -> [String] -> IO ()
admit requestedBase command rest = do
  refuseNested
  (memory, swap) <- configuredMemory Build
  outputBase <- maybe worktreeOutputBase (pure . Just) requestedBase
  directory <- (</> ".cache" </> "xm1-host-build") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  waitStart <- getMonotonicTime
  scoped <- userScopeAvailable
  pid <- getProcessID
  let unit = "xm1-host-build-" ++ show pid ++ ".scope"
      key = buildMemoryKey command rest
  unless scoped $
    if os == "linux"
      then ioError (userError "host-build requires a systemd user scope to enforce its memory cap")
      else let MemoryMax cap = memory
           in hPutStrLn stderr ("No systemd user session: host build runs without the " ++ cap ++ " memory cap") >> hFlush stderr
  reservation <- expectedReservation key memory
  let script = directory </> ("run-" ++ show pid ++ ".sh")
      running = command == "run"
      bazelArguments =
        ["--batch"]
          ++ maybe [] (\base -> ["--output_base=" ++ base]) outputBase
          ++ [command]
          ++ admittedFlags command
          ++ ["--script_path=" ++ script | running]
          ++ rest
      takeLane = do
        (lock, lane) <- acquireBuildLock directory
        when scoped (stopPreviousScope directory lane)
        checkStorageBudget directory
          `catch` (\problem -> refuseAccounting (show (problem :: StorageFailure)))
          `catch` (\problem -> refuseAccounting (show (problem :: IOException)))
        checkConfiguredAccounting
        waited <- subtract waitStart <$> getMonotonicTime
        pure ((lock, waited), buildScopeRecord directory lane)
  when running (removeIfPresent script)
  setEnv admittedVariable "1"
  (admitted, outcome) <- runContained memory swap reservation scoped unit takeLane bazelArguments
  case (admitted, unitMemoryPeak (outcomeState outcome)) of
    (Just _, Just peak) -> writePendingPeak key unit peak
    _ -> pure ()
  let verdict = judge outcome
      waited = maybe (outcomeWall outcome) snd admitted
  hPutStrLn stderr ("host-build verdict " ++ renderVerdict memory swap reservation command waited verdict outcome)
  hFlush stderr
  case (verdict, running, admitted) of
    (Passed, True, Just (lock, _)) -> do
      launcher <- readFile script
      _ <- evaluate (length launcher)
      removeFile script
      closeFd lock
      unsetEnv admittedVariable
      executeFile "bash" True ["-c", launcher, "bash"] Nothing
    _ -> do
      when running (removeIfPresent script)
      exitWith (verdictExit verdict outcome)

-- | The command and its targets: arguments before @--@ that are not flags.
buildMemoryKey :: String -> [String] -> MemoryKey
buildMemoryKey command rest =
  memoryKey (unwords ("host-build" : command : filter (not . isPrefixOf "-") (takeWhile (/= "--") rest)))

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
  current <- currentScopeIdentity scope
  let legacyOwner = LegacyReservation current
  state <- scopeIdentityState scope
  memory <- case state of
    ScopeIdentityActive _ cap -> pure cap
    ScopeIdentityInactive -> ioError (userError "legacy reservation requires an active scope")
  active <- legacyReservationActive legacyOwner
  unless active (ioError (userError "legacy reservation requires a populated capped scope"))
  _ <- reserveMemoryWithLegacy (Just legacyOwner) (Reservation (reservationTokens memory)) scope
  hPutStrLn stderr ("legacy reservation ready: " ++ unit ++ " RAM bytes=" ++ show (memoryMaxBytes memory))
  hFlush stderr
  let wait = do
        occupied <- legacyReservationActive legacyOwner
        when occupied (threadDelay 1_000_000 >> wait)
  wait

scopeIdentityState :: ScopeName -> IO ScopeIdentityState
scopeIdentityState (ScopeName unit) = do
  (_, out, _) <- readProcessWithExitCode "systemctl"
    ["--user", "show", unit, "-p", "ActiveState", "-p", "MemoryMax", "-p", "InvocationID"] ""
  let properties = [(key, drop 1 value) | line <- lines out, let (key, value) = break (== '=') line]
  case lookup "ActiveState" properties of
    Just "inactive" -> pure ScopeIdentityInactive
    Just state | state `elem` ["active", "activating", "deactivating", "failed"]
      , Just rawIdentity <- lookup "InvocationID" properties
      , Just identity <- parseInvocationId rawIdentity
      , Just rawCap <- lookup "MemoryMax" properties
      , Just cap <- parseMemoryMax rawCap -> pure (ScopeIdentityActive identity cap)
    _ -> ioError (userError ("cannot validate scope identity and cap: " ++ unit))

parseInvocationId :: String -> Maybe InvocationId
parseInvocationId text
  | length text == 32 && all isHexDigit text = Just (InvocationId text)
  | otherwise = Nothing

legacyReservationActive :: LegacyReservation -> IO Bool
legacyReservationActive (LegacyReservation owner@(ScopeIdentity scope expected _)) = do
  occupied <- sameInvocationActive owner
  if not occupied then pure False else do
    state <- scopeIdentityState scope
    case state of
      ScopeIdentityActive identity _ | identity == expected -> pure True
      _ -> ioError (userError "cannot validate populated legacy scope identity")

-- | Offsets recorded for live legacy scopes whose helper may have exited. A
-- record covers at least the scope's MemoryMax; records written before the
-- RAM-only budget also cover its swap cap.
protectedLegacyTokens :: FilePath -> MemoryBudget -> IO [FileOffset]
protectedLegacyTokens directory budget = do
  let recordsDirectory = directory </> "legacy-reservations"
  names <- listDirectory recordsDirectory
    `catchIOError` \problem -> if isDoesNotExistError problem then pure [] else ioError problem
  protected <- concat <$> mapM (readReservation recordsDirectory) names
  unless (length protected == length (nub protected)) $
    ioError (userError "persistent legacy memory reservations overlap")
  pure protected
  where
    readReservation recordRoot name = do
      text <- readFile (recordRoot </> name)
      _ <- evaluate (length text)
      case lines text of
        [unit, rawIdentity, group, rawInode, rawIndices]
          | name == rawIdentity
          , Just owner <- parseScopeIdentity [unit, rawIdentity, group, rawInode]
          , Just indices <- readMaybe rawIndices
          , not (null indices) && length (nub indices) == length indices
            && all (\index -> index >= 0 && index < fromIntegral (censusTokens budget)) indices -> do
              active <- legacyReservationActive (LegacyReservation owner)
              when active $ do
                state <- scopeIdentityState (case owner of ScopeIdentity scope _ _ -> scope)
                case state of
                  ScopeIdentityActive _ cap -> unless
                    (toInteger (length indices) * tokenBytes >= memoryMaxBytes cap)
                    (ioError (userError "legacy scope RAM cap exceeds its persistent reservation"))
                  ScopeIdentityInactive -> pure ()
                hPutStrLn stderr ("host admission: legacy memory owner=" ++ unit ++ " invocation=" ++ rawIdentity ++ " reserved tokens=" ++ show (length indices))
              pure (if active then indices else [])
        _ -> ioError (userError ("invalid persistent legacy memory reservation: " ++ name))

legacyReservationPath :: FilePath -> LegacyReservation -> FilePath
legacyReservationPath directory (LegacyReservation (ScopeIdentity _ (InvocationId invocation) _)) =
  directory </> "legacy-reservations" </> invocation

writeLegacyReservation :: FilePath -> LegacyReservation -> [FileOffset] -> IO ()
writeLegacyReservation directory reservation@(LegacyReservation owner) indices = do
  active <- legacyReservationActive reservation
  unless active (ioError (userError "legacy scope ended before reservation was recorded"))
  pid <- getProcessID
  let destination = legacyReservationPath directory reservation
      temporary = directory </> ("legacy-record-" ++ takeFileName destination ++ "." ++ show pid)
  createDirectoryIfMissing True (takeDirectory destination)
  writeFile temporary (unlines (scopeIdentityLines owner ++ [show indices]))
  renameFile temporary destination

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

lockOwnerText :: Fd -> IO String
lockOwnerText fd = do
  owner <- queryKernelOwner fd
  pure (holderText (sampleOf owner Nothing))

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

buildScopeRecord :: FilePath -> BuildLane -> FilePath
buildScopeRecord directory (BuildLane lane) =
  directory </> (if lane == 0 then "scope" else "scope-" ++ show lane)

-- Name-only build records belong to systemd-run's default app.slice.
migrateBuildRecords :: IO ()
migrateBuildRecords = do
  refuseNested
  directory <- (</> ".cache" </> "xm1-host-build") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  bracket (openFd (directory </> "build.lock") ReadWrite defaultFileFlags {creat = Just 0o600}) closeFd $ \fd -> do
    setFdOption fd CloseOnExec True
    forM_ [0 .. classConcurrency Build - 1] $ \lane -> do
      locked <- try (setLock fd (WriteLock, AbsoluteSeek, fromIntegral lane, 1)) :: IO (Either IOException ())
      case locked of
        Right () -> pure ()
        Left _ -> ioError (userError ("build lane " ++ show lane ++ " is held; finish existing builds before metadata migration"))
    (_, out, _) <- readProcessWithExitCode "systemctl" ["--user", "show", "app.slice", "-p", "ControlGroup", "--value"] ""
    let slice = trim out
    unless (validCgroupPath "app.slice" slice) $
      ioError (userError "cannot establish the legacy build scope slice")
    forM_ [0 .. classConcurrency Build - 1] $ \lane -> do
      let record = buildScopeRecord directory (BuildLane lane)
      exists <- doesFileExist record
      when exists $ do
        text <- readFile record
        _ <- evaluate (length text)
        case lines text of
          [unit] | "xm1-host-build-" `isPrefixOf` unit, Just scope <- parseScopeName unit ->
            migrateExpiredBuildRecord record slice scope
          _ -> void (readScopeRecord record)

migrateExpiredBuildRecord :: FilePath -> FilePath -> ScopeName -> IO ()
migrateExpiredBuildRecord record slice (ScopeName unit) = do
  (code, out, err) <- readProcessWithExitCode "systemctl"
    ["--user", "show", unit, "-p", "LoadState", "-p", "ActiveState", "-p", "ControlGroup"] ""
  let properties = [(key, drop 1 value) | line <- lines out, let (key, value) = break (== '=') line]
      group = slice </> unit
      observedGroup = maybe "" id (lookup "ControlGroup" properties)
  unless (lookup "LoadState" properties `elem` map Just ["loaded", "not-found"]
      && lookup "ActiveState" properties `elem` map Just ["inactive", "failed"]
      && (null observedGroup || observedGroup == group)) $
    ioError (userError ("legacy build scope has not completed: " ++ unit ++ " " ++ show (code, out, err)))
  empty <- bracket
    ((Just <$> openFd ("/sys/fs/cgroup" ++ group) ReadOnly defaultFileFlags)
      `catchIOError` \problem -> if isDoesNotExistError problem then pure Nothing else ioError problem)
    (mapM_ closeFd)
    (maybe (pure True) emptyCgroup)
  unless empty (ioError (userError ("legacy build cgroup remains populated: " ++ group)))
  pid <- getProcessID
  let archive = record ++ ".legacy-" ++ show pid
      temporary = record ++ ".migrated-" ++ show pid
  archived <- doesFileExist archive
  when archived (ioError (userError ("migration archive already exists: " ++ archive)))
  writeFile temporary (unlines ["expired-build-scope", unit])
  renameFile record archive
  renameFile temporary record
  hPutStrLn stderr ("host-build migration: verified empty " ++ group ++ "; archived=" ++ archive)
  hFlush stderr

stopPreviousScope :: FilePath -> BuildLane -> IO ()
stopPreviousScope directory lane = do
  previous <- readScopeRecord (buildScopeRecord directory lane)
  mapM_ reapScope previous

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

-- | Runs Bazel through a scope child. The child reports @reserved@ once it
-- holds the scope's memory; ADMISSION then runs and names the scope record
-- the child claims before starting Bazel. The child reports Bazel's exit
-- status and stays alive until the unit's result has been read; systemd
-- collects a successful scope once its last process exits.
runContained :: MemoryMax -> MemorySwapMax -> Reservation -> Bool -> String -> IO (a, FilePath) -> [String] -> IO (Maybe a, Outcome)
runContained memoryCap swapCap reservation scoped unit admission bazelArguments = do
  (reportRead, reportWrite) <- createPipe
  (releaseRead, releaseWrite) <- createPipe
  setFdOption reportRead CloseOnExec True
  setFdOption releaseWrite CloseOnExec True
  self <- getExecutablePath
  let MemoryMax memory = memoryCap
      MemorySwapMax swap = swapCap
      child = ["--scope-child", fdText reportWrite, fdText releaseRead, if scoped then unit else "-"] ++ ("bazel" : bazelArguments)
      process
        | scoped = proc "systemd-run" $
            [ "--user", "--scope", "--quiet", "--unit=" ++ unit
            , "--property=MemoryMax=" ++ memory
            , "--property=MemoryHigh=" ++ show (reservationBytes reservation)
            , "--property=MemorySwapMax=" ++ swap
            , "--property=OOMPolicy=stop"
            , "--", self
            ] ++ child
        | otherwise = proc self child
  _ <- installHandler sigINT (Catch (pure ())) Nothing
  (_, _, _, handle) <- createProcess process {close_fds = False}
  closeFd reportWrite
  closeFd releaseRead
  reportHandle <- fdToHandle reportRead
  releaseHandle <- fdToHandle releaseWrite
  ready <- try (hGetLine reportHandle) :: IO (Either IOException String)
  admitted <- case ready of
    Right line | trim line == "reserved" -> do
      (value, record) <- admission
      sent <- try (hPutStrLn releaseHandle record >> hFlush releaseHandle) :: IO (Either IOException ())
      pure (either (const Nothing) (const (Just value)) sent)
    _ -> pure Nothing
  before <- daemonCounters
  start <- getMonotonicTime
  reported <- case admitted of
    Just _ -> try (hGetLine reportHandle) :: IO (Either IOException String)
    Nothing -> pure (Left (userError "the scope child ended before admission"))
  end <- getMonotonicTime
  let exitStatus = either (const Nothing) (readMaybe . trim) reported
  state <-
    if scoped
      then unitState unit (isNothing exitStatus)
      else pure (UnitState "uncapped" Nothing Nothing)
  after <- daemonCounters
  _ <- try (hClose releaseHandle) :: IO (Either IOException ())
  _ <- waitForProcess handle
  when scoped (void (readProcessWithExitCode "systemctl" ["--user", "reset-failed", unit] ""))
  pure (admitted, Outcome
    { outcomeExit = exitStatus
    , outcomeUnit = if scoped then Just unit else Nothing
    , outcomeState = state
    , outcomeWall = end - start
    , outcomeDaemonOom = (-) <$> daemonOomKills after <*> daemonOomKills before
    , outcomeDaemonCpu = (-) <$> daemonCpuMicros after <*> daemonCpuMicros before
    })

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

-- | Runs inside the scope. Reserves the scope's memory, reports @reserved@ on
-- @report@, reads the scope record path from @release@, then runs PROGRAM and
-- reports its exit status. Exits once @release@ reaches end of file; an end
-- of file before then means the admitting process died, and the scope is
-- stopped.
scopeChild :: Fd -> Fd -> String -> String -> [String] -> IO ()
scopeChild report release unit program arguments = do
  _ <- installHandler sigINT (Catch (pure ())) Nothing
  setFdOption report CloseOnExec True
  setFdOption release CloseOnExec True
  reportHandle <- fdToHandle report
  releaseHandle <- fdToHandle release
  scope <- maybe (ioError (userError "scope child requires a valid scope name")) pure (parseScopeName unit)
  recordPath <- newEmptyMVar
  released <- newEmptyMVar
  _ <- forkIO $ do
    line <- try (hGetLine releaseHandle) :: IO (Either IOException String)
    putMVar recordPath (either (const Nothing) Just line)
    _ <- try (hGetContents releaseHandle >>= evaluate . length) :: IO (Either IOException Int)
    void (readProcessWithExitCode "systemctl" ["--user", "stop", "--no-block", unit] "")
    putMVar released ()
  _ <- reserveMemory scope
  _ <- try (hPutStrLn reportHandle "reserved" >> hFlush reportHandle) :: IO (Either IOException ())
  record <- takeMVar recordPath
  case record of
    Nothing -> takeMVar released >> exitWith (ExitFailure 1)
    Just path -> do
      claimScopeRecord path scope
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

renderVerdict :: MemoryMax -> MemorySwapMax -> Reservation -> String -> Double -> Verdict -> Outcome -> String
renderVerdict (MemoryMax memory) (MemorySwapMax swap) reservation command waited verdict outcome =
  "{" ++ commaSeparated
    [ field "verdict" (quoted verdictName)
    , field "resource_failure" resource
    , field "command" (quoted (filter safe command))
    , field "exit_code" (maybe "null" show (outcomeExit outcome))
    , field "unit" (maybe "null" quoted (outcomeUnit outcome))
    , field "unit_result" (quoted (filter safe (unitResult state)))
    , field "memory_max" (quoted memory)
    , field "memory_swap_max" (quoted swap)
    , field "reserved_memory_bytes" (show (reservationBytes reservation))
    , field "lock_wait_seconds" (seconds waited)
    , field "wall_seconds" (seconds (outcomeWall outcome))
    , field "scope_cpu_seconds" (maybe "null" (\nanos -> seconds (fromIntegral nanos / 1e9)) (unitCpuNanos state))
    , field "scope_memory_peak_bytes" (maybe "null" show (unitMemoryPeak state))
    , field "nix_daemon_cpu_seconds" (maybe "null" (\micros -> seconds (fromIntegral micros / 1e6)) (outcomeDaemonCpu outcome))
    , field "nix_daemon_oom_kills" (maybe "null" show (outcomeDaemonOom outcome))
    ] ++ "}"
  where
    state = outcomeState outcome
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

readStrict :: FilePath -> IO String
readStrict path = do
  text <- readFile path
  _ <- evaluate (length text)
  pure text

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = removeFile path `catchIOError` \problem ->
  unless (isDoesNotExistError problem) (ioError problem)
