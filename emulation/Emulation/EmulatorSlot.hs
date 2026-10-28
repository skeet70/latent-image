-- | Host admission for full-system emulator runs. A run holds one of N slot
-- locks under @~/.cache/xm1-emulator-slots/@, shared by every checkout on the
-- host, and executes inside a systemd user scope with a memory cap and no swap.
module Emulation.EmulatorSlot
  ( SlotCount (..)
  , MemoryMax
  , SlotPolicy (..)
  , slotPolicyFromEnvironment
  , reexecInSlot
  , attachGuard
  , attachGuardCommand
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isDigit, isSpace)
import Data.List (isPrefixOf, isSuffixOf, stripPrefix)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getHomeDirectory, listDirectory, removeFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv)
import System.Exit (ExitCode (ExitSuccess))
import System.FilePath ((</>), takeDirectory, takeFileName)
import System.IO (Handle, IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (FileStatus, createSymbolicLink, fileMode, fileOwner, getFileStatus, isRegularFile, ownerExecuteMode, readSymbolicLink, setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Types (ProcessID)
import System.Posix.User (getEffectiveUserID)
import System.Process (readProcessWithExitCode)
import System.Timeout (timeout)
import Text.Read (readMaybe)

newtype SlotCount = SlotCount Int

-- | A systemd @MemoryMax=@ value: decimal bytes with an optional K, M, G or T suffix.
newtype MemoryMax = MemoryMax String

data SlotPolicy = SlotPolicy
  { slotCount :: SlotCount
  , memoryMax :: MemoryMax
  }

newtype Slot = Slot Int

slotVariable, slotsVariable, memoryVariable :: String
slotVariable = "XM1_EMULATOR_SLOT"
slotsVariable = "XM1_EMULATOR_SLOTS"
memoryVariable = "XM1_EMULATOR_MEMORY_MAX"

-- | Two slots of 8G by default; @XM1_EMULATOR_SLOTS@ (1..16) and
-- @XM1_EMULATOR_MEMORY_MAX@ override them.
slotPolicyFromEnvironment :: IO SlotPolicy
slotPolicyFromEnvironment = do
  slots <- lookupEnv slotsVariable >>= maybe (pure (SlotCount 2)) parseSlots
  memory <- lookupEnv memoryVariable >>= maybe (pure (MemoryMax "8G")) parseMemory
  pure (SlotPolicy slots memory)
  where
    parseSlots text = case readMaybe text of
      Just count | count >= 1 && count <= 16 -> pure (SlotCount count)
      _ -> ioError (userError (slotsVariable ++ " must be an integer 1..16"))
    parseMemory text = case span isDigit text of
      (digits@(_ : _), suffix) | suffix `elem` ["", "K", "M", "G", "T"], read digits > (0 :: Integer) ->
        pure (MemoryMax (digits ++ suffix))
      _ -> ioError (userError (memoryVariable ++ " must be a positive byte count with an optional K, M, G or T suffix"))

-- | Returns inside a slot. Outside one, waits for a free slot, then replaces
-- the process with itself under @systemd-run --user --scope@. The slot is a
-- POSIX record lock, which execve preserves, so it is held until the scope's
-- main process exits.
--
-- The re-exec runs a pinned copy of the running executable, never its original
-- path: a concurrent build relinks that path while the wait runs, and
-- systemd-run must not start the replacement or a deleted path.
reexecInSlot :: SlotPolicy -> [String] -> IO ()
reexecInSlot policy arguments = do
  inside <- lookupEnv slotVariable
  case inside of
    Just _ -> pinInherited arguments
    Nothing -> do
      directory <- slotDirectory
      Slot slot <- acquire directory policy
      setEnv slotVariable (show slot)
      original <- getExecutablePath
      _ <- pinGuard directory original
      executable <- pinExecutable directory
      let MemoryMax memory = memoryMax policy
      executeFile "systemd-run" True
        ([ "--user", "--scope", "--quiet", "--collect"
         , "--property=MemoryMax=" ++ memory
         , "--property=MemorySwapMax=0"
         , "--"
         , executable
         ] ++ arguments) Nothing

pinInherited :: [String] -> IO ()
pinInherited arguments = do
  executable <- getExecutablePath
  directory <- slotDirectory
  pid <- getProcessID
  case pinIdentity (takeFileName (onDiskPath executable)) of
    Just _ -> pure ()
    Nothing -> do
      parents <- ancestors pid 8
      guard <- verifiedGuardAncestor parents
      case guard of
        Nothing -> ioError (userError "inherited slot has no verified thermal guard ancestor")
        Just guardPath -> case guardNumber (takeFileName guardPath) of
          Nothing -> ioError (userError ("verified guard has no guard number: " ++ guardPath))
          Just guardPid -> do
            pinned <- pinExecutableAs directory (InheritedPin guardPid pid)
            executeFile pinned True arguments Nothing

-- | Replaces the current launcher with the same program under the pinned
-- guard. Used by a program that is itself the emulation process.
attachGuard :: [String] -> IO ()
attachGuard arguments = do
  executable <- getExecutablePath
  attachGuardCommand (executable : arguments)

-- | Replaces the current launcher with the pinned guard running exactly this
-- command. The command is never recovered from an environment variable.
attachGuardCommand :: [String] -> IO ()
attachGuardCommand command = do
  executable <- getExecutablePath
  pid <- getProcessID
  let guard = case stripPrefix "pin-" (takeFileName executable) of
        Just rest ->
          let (digits, _) = span isDigit rest
          in takeDirectory executable </> ("guard-" ++ digits)
        Nothing -> takeDirectory executable </> ("missing-guard-" ++ show pid)
  valid <- validGuard guard
  unless valid $ ioError (userError ("thermal guard is missing or invalid: " ++ guard))
  parents <- ancestors pid 8
  matches <- mapM (isGuardProcess guard) parents
  unless (or matches) $ executeFile guard True (["--"] ++ command) Nothing

validGuard :: FilePath -> IO Bool
validGuard path = do
  present <- doesFileExist path
  if not present then pure False else do
    result <- try (getFileStatus path) :: IO (Either IOException FileStatus)
    case result of
      Left _ -> pure False
      Right status -> do
        owner <- getEffectiveUserID
        pure (isRegularFile status
          && fileOwner status == owner
          && fileMode status .&. ownerExecuteMode /= 0)

ancestors :: ProcessID -> Int -> IO [ProcessID]
ancestors _ 0 = pure []
ancestors pid remaining = do
  parent <- try (parentOf pid) :: IO (Either IOException (Maybe ProcessID))
  case parent of
    Left _ -> pure []
    Right Nothing -> pure []
    Right (Just value)
      | value == 0 || value == pid -> pure []
      | otherwise -> (value :) <$> ancestors value (remaining - 1)

parentOf :: ProcessID -> IO (Maybe ProcessID)
parentOf pid = do
  bytes <- B.readFile ("/proc" </> show pid </> "status")
  pure $ case [line | line <- lines (C.unpack bytes), "PPid:" `isPrefixOf` line] of
    (line : _) -> readMaybe (filter isDigit (drop 5 line))
    [] -> Nothing

isGuardProcess :: FilePath -> ProcessID -> IO Bool
isGuardProcess guard pid = do
  comm <- try (readFile ("/proc" </> show pid </> "comm")) :: IO (Either IOException String)
  exe <- try (readSymbolicLink ("/proc" </> show pid </> "exe")) :: IO (Either IOException FilePath)
  case (comm, exe) of
    (Right name, Right path) -> do
      canonical <- try (canonicalizePath guard) :: IO (Either IOException FilePath)
      case canonical of
        Left _ -> pure False
        Right guardPath -> do
          actual <- try (canonicalizePath path) :: IO (Either IOException FilePath)
          let workerPath = guardWorkerPath guardPath
          pure ((takeWhile (/= '\n') name == "thermal-guard"
              || "guard-" `isPrefixOf` takeWhile (/= '\n') name)
            && either (const False) (\actualPath -> actualPath == guardPath || actualPath == workerPath) actual)
    _ -> pure False

guardWorkerPath :: FilePath -> FilePath
guardWorkerPath guard = case stripPrefix "guard-" (takeFileName guard) of
  Just suffix ->
    let (digits, _) = span isDigit suffix
    in takeDirectory guard </> ("guard-worker-" ++ digits)
  Nothing -> takeDirectory guard </> "thermal-guard-worker"

-- | @pin-<owner>@ or @pin-<guard>-<owner>@; cleanup uses the owner, guard attachment the guard.
data PinIdentity
  = SlotPin ProcessID
  | InheritedPin ProcessID ProcessID
  deriving (Eq, Show)

pinName :: PinIdentity -> String
pinName (SlotPin owner) = "pin-" ++ show owner
pinName (InheritedPin guard owner) = "pin-" ++ show guard ++ "-" ++ show owner

pinIdentity :: String -> Maybe PinIdentity
pinIdentity entry = do
  let base = maybe entry id (stripNameSuffix ".runfiles" entry)
  rest <- stripPrefix "pin-" base
  let (guardDigits, suffix) = span isDigit rest
  guardPid <- positivePid guardDigits
  case suffix of
    "" -> Just (SlotPin guardPid)
    '-' : ownerRest -> do
      let (ownerDigits, trailing) = span isDigit ownerRest
      ownerPid <- positivePid ownerDigits
      if null trailing then Just (InheritedPin guardPid ownerPid) else Nothing
    _ -> Nothing

pinOwner :: PinIdentity -> ProcessID
pinOwner (SlotPin owner) = owner
pinOwner (InheritedPin _ owner) = owner

positivePid :: String -> Maybe ProcessID
positivePid text
  | null text || not (all isDigit text) = Nothing
  | otherwise = case readMaybe text :: Maybe Integer of
      Just value | value > 0 && value <= toInteger (maxBound :: ProcessID) -> Just (fromInteger value)
      _ -> Nothing

stripNameSuffix :: String -> String -> Maybe String
stripNameSuffix suffix text
  | suffix `isSuffixOf` text = Just (take (length text - length suffix) text)
  | otherwise = Nothing

guardNumber :: FilePath -> Maybe ProcessID
guardNumber name = do
  rest <- stripPrefix "guard-" name
  positivePid rest

verifiedGuardAncestorFor :: [ProcessID] -> FilePath -> IO Bool
verifiedGuardAncestorFor parents guard = do
  valid <- validGuard guard
  worker <- validGuard (guardWorkerPath guard)
  if not (valid && worker) then pure False else do
    matches <- mapM (isGuardProcess guard) parents
    pure (or matches)

verifiedGuardAncestor :: [ProcessID] -> IO (Maybe FilePath)
verifiedGuardAncestor parents = do
  directory <- slotDirectory
  entries <- listDirectory directory
  let guards = [directory </> entry | entry <- entries, Just _ <- [guardNumber entry]]
  matches <- concat <$> forM guards (\guard -> do
    verified <- verifiedGuardAncestorFor parents guard
    pure [guard | verified])
  pure $ case matches of
    [guard] -> Just guard
    _ -> Nothing

-- | Copies the canonical sibling guard and worker into the slot directory. The
-- sources are the launcher's siblings, never environment-selected paths.
pinGuard :: FilePath -> FilePath -> IO FilePath
pinGuard directory original = do
  let source = takeDirectory original </> "thermal-guard"
  present <- doesFileExist source
  unless present $ ioError (userError ("thermal guard is missing: " ++ source))
  status <- getFileStatus source
  owner <- getEffectiveUserID
  unless (isRegularFile status
    && fileOwner status == owner
    && fileMode status .&. ownerExecuteMode /= 0)
    (ioError (userError ("thermal guard is not an owned executable: " ++ source)))
  pid <- getProcessID
  let target = directory </> ("guard-" ++ show pid)
      workerSource = takeDirectory original </> "thermal-guard-worker"
      workerTarget = directory </> ("guard-worker-" ++ show pid)
  removeIfPresent target
  removeIfPresent workerTarget
  withBinaryFile source ReadMode $ \input ->
    withBinaryFile target WriteMode $ \output ->
      copyChunks input output
  setFileMode target 0o700
  presentWorker <- doesFileExist workerSource
  unless presentWorker $ ioError (userError ("thermal guard worker is missing: " ++ workerSource))
  workerStatus <- getFileStatus workerSource
  unless (isRegularFile workerStatus
    && fileOwner workerStatus == owner
    && fileMode workerStatus .&. ownerExecuteMode /= 0)
    (ioError (userError ("thermal guard worker is not an owned executable: " ++ workerSource)))
  withBinaryFile workerSource ReadMode $ \input ->
    withBinaryFile workerTarget WriteMode $ \output ->
      copyChunks input output
  setFileMode workerTarget 0o700
  pure target

slotDirectory :: IO FilePath
slotDirectory = do
  directory <- (</> ".cache" </> "xm1-emulator-slots") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  pure directory

acquire :: FilePath -> SlotPolicy -> IO Slot
acquire directory policy = do
  let SlotCount count = slotCount policy
      attempt announced = do
        free <- firstFree directory [1 .. count]
        case free of
          Just slot -> pure slot
          Nothing -> do
            unless announced $ do
              hPutStrLn stderr ("Waiting for one of " ++ show count ++ " emulator slots in " ++ directory)
              hFlush stderr
            threadDelay 2000000
            attempt True
  attempt False

firstFree :: FilePath -> [Int] -> IO (Maybe Slot)
firstFree _ [] = pure Nothing
firstFree directory (slot : rest) = do
  fd <- openFd (directory </> ("slot-" ++ show slot ++ ".lock")) ReadWrite defaultFileFlags {creat = Just 0o600}
  locked <- try (setLock fd (WriteLock, AbsoluteSeek, 0, 0)) :: IO (Either IOException ())
  case locked of
    Right () -> pure (Just (Slot slot))
    Left _ -> closeFd fd >> firstFree directory rest

-- | A private copy of the running executable beneath the slot directory, named
-- for the PID execve preserves. systemd-run starts the scope by path, and the
-- original path is not stable across the slot wait. The bytes come from the
-- running inode through @/proc/self/exe@, so no relink of the on-disk file
-- changes them.
pinExecutable :: FilePath -> IO FilePath
pinExecutable directory = do
  pid <- getProcessID
  pinExecutableAs directory (SlotPin pid)

pinExecutableAs :: FilePath -> PinIdentity -> IO FilePath
pinExecutableAs directory identity = do
  sweepPinned directory
  executable <- getExecutablePath
  let pinned = directory </> pinName identity
  -- A crashed run with a reused PID can leave either path behind.
  removeIfPresent pinned
  removeIfPresent (pinned ++ ".runfiles")
  withBinaryFile "/proc/self/exe" ReadMode $ \running ->
    withBinaryFile pinned WriteMode $ \copy -> copyChunks running copy
  setFileMode pinned 0o700
  linkRunfiles executable pinned
  pure pinned

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = removeFile path `catchIOError` \problem ->
  unless (isDoesNotExistError problem) (ioError problem)

copyChunks :: Handle -> Handle -> IO ()
copyChunks source target = do
  chunk <- B.hGet source copyChunkBytes
  unless (B.null chunk) (B.hPut target chunk >> copyChunks source target)

copyChunkBytes :: Int
copyChunkBytes = 1024 * 1024

-- | Bazel tools locate their data at @getExecutablePath ++ ".runfiles"@; the
-- pinned copy keeps that adjacency through a link to the original tree.
linkRunfiles :: FilePath -> FilePath -> IO ()
linkRunfiles executable pinned = do
  let runfiles = onDiskPath executable ++ ".runfiles"
  present <- doesDirectoryExist runfiles
  when present (createSymbolicLink runfiles (pinned ++ ".runfiles"))

-- | @readlink(2)@ of @/proc/self/exe@ reports the kernel's last path for the
-- inode, with @ (deleted)@ appended once a relink has replaced it.
onDiskPath :: FilePath -> FilePath
onDiskPath path
  | deletedSuffix `isSuffixOf` path = take (length path - length deletedSuffix) path
  | otherwise = path

deletedSuffix :: String
deletedSuffix = " (deleted)"

-- | Removes dead pinned copies only while no thermal-guard service is live.
-- Pruning is suspended while a service is active, activating or deactivating,
-- or the manager response is unknown.
sweepPinned :: FilePath -> IO ()
sweepPinned directory = do
  mayBeLive <- guardServiceMayBeLive
  unless mayBeLive $ do
    entries <- listDirectory directory
    forM_ entries $ \entry -> case pinnedPid entry of
      Nothing -> pure ()
      Just pid -> do
        alive <- doesDirectoryExist ("/proc" </> show pid)
        unless alive $ do
          _ <- try (removeFile (directory </> entry)) :: IO (Either IOException ())
          pure ()

-- | Manager query. True means pruning must stop: a guard service is live,
-- systemctl failed or timed out, or its state is unknown.
guardServiceMayBeLive :: IO Bool
guardServiceMayBeLive = do
  result <- timeout 5_000_000 (try (readProcessWithExitCode "systemctl"
    [ "--user", "list-units", "--type=service"
    , "--state=active", "--state=activating", "--state=deactivating"
    , "--no-legend", "--plain", "--no-pager"
    , "thermal-guard-*"
    ] "")) :: IO (Maybe (Either IOException (ExitCode, String, String)))
  pure $ case result of
    Nothing -> True
    Just (Left _) -> True
    Just (Right (ExitSuccess, stdout, errors)) ->
      not (null (filter (not . isSpace) errors))
        || not (null (filter (not . isSpace) stdout))
    Just (Right (_, _, _)) -> True

pinnedPid :: String -> Maybe ProcessID
pinnedPid entry = case pinIdentity entry of
  Just identity -> Just (pinOwner identity)
  Nothing -> case stripPrefix "guard-worker-" entry of
    Just rest -> parseGuard rest
    Nothing -> maybe Nothing parseGuard (stripPrefix "guard-" entry)
  where
    parseGuard text = positivePid (maybe text id (stripNameSuffix ".runfiles" text))
