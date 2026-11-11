-- | Whole-process-tree admission for native suites and full-system emulators.
module Emulation.EmulatorSlot
  ( SlotCount (..)
  , MemoryMax
  , SlotPolicy (..)
  , ExecutionClass (..)
  , slotPolicyForClass
  , slotPolicyFromEnvironment
  , reexecInSlot
  , pinExecutable
  , insideSlot
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.List (isSuffixOf, stripPrefix)
import qualified Emulation.HostAdmission as Host
import Emulation.HostAdmission (MemoryMax (..), MemorySwapMax (..), ResourceClass (Emulator), ScopeName, claimScopeRecord, classConcurrency, configuredMemory, lockOwnerText, memoryMaxBytes, memorySwapMaxBytes, parseScopeName, reserveMemory, userScopeAvailable)
import qualified FujiAF.RunOutput as Run
import FujiAF.StorageBudget (checkStorageBudget)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, getCurrentDirectory, getHomeDirectory, listDirectory, removeFile)
import System.Environment (getArgs, getExecutablePath, getProgName, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>))
import System.IO (Handle, IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError, isPermissionError)
import System.Info (os)
import System.Posix.Files (createSymbolicLink, setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Signals (nullSignal, signalProcess)
import System.Posix.Types (ProcessID)
import Text.Read (readMaybe)

newtype SlotCount = SlotCount Int

data ExecutionClass = NativeVerification | FullSystem
  deriving (Eq, Read, Show)

data SlotPolicy = SlotPolicy
  { slotCount :: SlotCount
  , memoryMax :: MemoryMax
  , memorySwapMax :: MemorySwapMax
  , executionClass :: ExecutionClass
  }

newtype Slot = Slot Int

slotVariable, slotsVariable, runProgramVariable :: String
slotVariable = "XM1_EMULATOR_SLOT"
slotsVariable = "XM1_EMULATOR_SLOTS"
runProgramVariable = "XM1_RUN_PROGRAM"


resourceClass :: ExecutionClass -> Host.ResourceClass
resourceClass FullSystem = Emulator
resourceClass NativeVerification = Host.NativeVerification

-- | Whether this process or an ancestor holds a slot.
insideSlot :: IO Bool
insideSlot = maybe False (const True) <$> lookupEnv slotVariable

-- | Memory caps cover the suite and all of its children.
slotPolicyFromEnvironment :: IO SlotPolicy
slotPolicyFromEnvironment = slotPolicyForClass FullSystem

slotPolicyForClass :: ExecutionClass -> IO SlotPolicy
slotPolicyForClass execution = do
  let resource = resourceClass execution
      maximumSlots = classConcurrency resource
      slotsName = if execution == FullSystem then slotsVariable else "XM1_NATIVE_SLOTS"
  (memory, swap) <- configuredMemory resource
  slotsText <- lookupEnv slotsName
  slots <- case slotsText >>= readMaybe of
    Just count | count >= 1 && count <= maximumSlots -> pure (SlotCount count)
    Nothing | slotsText == Nothing -> pure (SlotCount maximumSlots)
    _ -> ioError (userError (slotsName ++ " must be an integer 1.." ++ show maximumSlots))
  pure (SlotPolicy slots memory swap execution)

-- | Re-execs a pinned inode into a capped scope before taking resource locks.
reexecInSlot :: SlotPolicy -> [String] -> IO ()
reexecInSlot policy arguments = do
  let resource = resourceClass (executionClass policy)
      SlotCount requestedSlots = slotCount policy
  unless (requestedSlots >= 1 && requestedSlots <= classConcurrency resource
      && memoryMaxBytes (memoryMax policy) > 0
      && memoryMaxBytes (memoryMax policy) <= 32 * 1024 ^ (3 :: Int)
      && memorySwapMaxBytes (memorySwapMax policy) <= 32 * 1024 ^ (3 :: Int)
      && memoryMaxBytes (memoryMax policy) + memorySwapMaxBytes (memorySwapMax policy) <= 64 * 1024 ^ (3 :: Int)) $
    ioError (userError "invalid verification class policy")
  storage <- lookupEnv "XM1_STORAGE_PATH" >>= maybe defaultStoragePath pure
  checkStorageBudget storage
  inside <- lookupEnv slotVariable
  build <- lookupEnv "XM1_HOST_BUILD"
  when (build /= Nothing) (ioError (userError "refusing slot admission inside a host build"))
  case inside of
    Just _ -> do
      owner <- lookupEnv "XM1_ADMISSION_OWNER"
      admittedClass <- lookupEnv "XM1_ADMISSION_CLASS"
      unless ((admittedClass >>= readMaybe) == Just (executionClass policy)) $
        ioError (userError "verification class differs from the active admission")
      pid <- getProcessID
      unless (owner == Just (show pid)) $
        ioError (userError "refusing nested verification admission")
    Nothing -> do
      directory <- slotDirectory
      executable <- getExecutablePath
      pending <- lookupEnv "XM1_ADMISSION_SCOPE"
      case pending of
        Just unit -> do
          groups <- readFile "/proc/self/cgroup"
          unless (any (\line -> reverse (take (length unit) (reverse line)) == unit) (lines groups)) $
            ioError (userError "admission scope does not contain this process")
          scope <- maybe (ioError (userError "invalid admission scope")) pure (parseScopeName unit)
          (Slot slot, pinned) <- withBinaryFile (runningImage executable) ReadMode $ \running -> do
            acquired <- acquire directory (executionClass policy) (Just scope)
            _ <- reserveMemory (memoryMax policy) (memorySwapMax policy) scope
            pinned <- pinExecutable directory executable running
            pure (acquired, pinned)
          hPutStrLn stderr ("verification admission: class=" ++ show (executionClass policy)
            ++ " memory_max_bytes=" ++ show (memoryMaxBytes (memoryMax policy))
            ++ " memory_swap_max_bytes=" ++ show (memorySwapMaxBytes (memorySwapMax policy))
            ++ " reserved_memory_bytes=" ++ show (memoryMaxBytes (memoryMax policy) + memorySwapMaxBytes (memorySwapMax policy)))
          hFlush stderr
          checkStorageBudget storage
          unsetEnv "XM1_ADMISSION_SCOPE"
          setEnv slotVariable (show slot)
          openSlotRun
          pid <- getProcessID
          setEnv "XM1_ADMISSION_OWNER" (show pid)
          setEnv "XM1_ADMISSION_CLASS" (show (executionClass policy))
          executeFile pinned False arguments Nothing
        Nothing -> do
          scoped <- userScopeAvailable
          if scoped
            then do
              pinned <- withBinaryFile (runningImage executable) ReadMode (pinExecutable directory executable)
              pid <- getProcessID
              let unit = "xm1-verification-" ++ show pid ++ ".scope"
                  MemoryMax memory = memoryMax policy
                  MemorySwapMax swap = memorySwapMax policy
              program <- getProgName
              setEnv runProgramVariable program
              setEnv "XM1_ADMISSION_SCOPE" unit
              executeFile "systemd-run" True
                ([ "--user", "--scope", "--quiet", "--collect", "--unit=" ++ unit
                 , "--property=MemoryMax=" ++ memory
                 , "--property=MemorySwapMax=" ++ swap, "--property=OOMPolicy=stop"
                 ] ++ ["--property=CPUWeight=1000" | executionClass policy == NativeVerification]
                   ++ ["--", pinned] ++ arguments) Nothing
            else do
              when (os == "linux") (ioError (userError "verification admission requires a systemd user scope"))
              (Slot slot, pinned) <- withBinaryFile (runningImage executable) ReadMode $ \running -> do
                acquired <- acquire directory (executionClass policy) Nothing
                pinned <- pinExecutable directory executable running
                pure (acquired, pinned)
              checkStorageBudget storage
              program <- getProgName
              setEnv runProgramVariable program
              setEnv slotVariable (show slot)
              openSlotRun
              pid <- getProcessID
              setEnv "XM1_ADMISSION_OWNER" (show pid)
              setEnv "XM1_ADMISSION_CLASS" (show (executionClass policy))
              let MemoryMax memory = memoryMax policy
              hPutStrLn stderr ("No systemd user session: " ++ show (executionClass policy) ++ " slot " ++ show slot ++ " runs without the " ++ memory ++ " memory cap")
              hFlush stderr
              executeFile pinned False arguments Nothing

defaultStoragePath :: IO FilePath
defaultStoragePath = do
  root <- getCurrentDirectory
  let private = root </> "private-data"
  exists <- doesDirectoryExist private
  pure (if exists then private else root)
slotDirectory :: IO FilePath
slotDirectory = do
  directory <- (</> ".cache" </> "xm1-emulator-slots") <$> getHomeDirectory
  createDirectoryIfMissing True directory
  pure directory

-- | The run lock must survive the exec into the pinned executable, and the next opener marks the
-- run unjudged.
openSlotRun :: IO ()
openSlotRun = do
  existing <- lookupEnv "XM1_RUN_DIR"
  case existing of
    Just _ -> pure ()
    Nothing -> do
      resolved <- Run.resolveTask
      case resolved of
        Nothing -> pure ()
        Just task -> do
          configured <- lookupEnv "XM1_RUN_LABEL"
          exported <- lookupEnv runProgramVariable
          program <- getProgName
          arguments <- getArgs
          budget <- Run.resolveWorkBudget
          let programName = case exported of
                Just name -> name
                Nothing -> program
              label = Run.labelFromText (maybe programName id configured)
          opened <- Run.openRun
            Run.OpenRequest
              { Run.openTask = task
              , Run.openLabel = label
              , Run.openOpener = Run.Slot
              , Run.openCommand = programName : arguments
              , Run.openWorkBudget = budget
              }
          case opened of
            Run.RunOpened handle -> do
              Run.makeRunLockInheritable handle
              let paths = Run.runPaths handle
              setEnv "XM1_RUN_DIR" (Run.runDirectory paths)
              setEnv "XM1_RUN_WORK" (Run.runWork paths)
              setEnv "XM1_RUN_RESULT" (Run.runResult paths)
            Run.RunRefused refusal -> do
              hPutStrLn stderr (Run.describeRefusal refusal)
              hFlush stderr
              exitWith (ExitFailure 75)

acquire :: FilePath -> ExecutionClass -> Maybe ScopeName -> IO Slot
acquire directory execution scope = do
  let maximumSlots = classConcurrency (resourceClass execution)
      prefix = if execution == FullSystem then "full-" else "native-"
      attempt = do
        free <- firstFree prefix [1 .. maximumSlots]
        case free of
          Just slot -> pure slot
          Nothing -> do
            hPutStrLn stderr ("host admission: waiting for " ++ show execution ++ " class slot")
            hFlush stderr
            threadDelay 500_000
            attempt
      firstFree _ [] = pure Nothing
      firstFree slotPrefix (slot : rest) = do
        fd <- openFd (directory </> (slotPrefix ++ show slot ++ ".lock")) ReadWrite defaultFileFlags {creat = Just 0o600}
        locked <- try (setLock fd (WriteLock, AbsoluteSeek, 0, 0)) :: IO (Either IOException ())
        case locked of
          Right () -> do
            forM_ scope (claimScopeRecord (directory </> (slotPrefix ++ show slot ++ ".scope")))
            pure (Just (Slot slot))
          Left _ -> do
            owner <- lockOwnerText fd
            hPutStrLn stderr ("host admission: " ++ slotPrefix ++ show slot ++ " " ++ owner)
            closeFd fd
            firstFree slotPrefix rest
  attempt

-- | A private copy of the running executable beneath the slot directory, named
-- @pin-<pid>@ for the PID execve preserves, copied from RUNNING, a handle on
-- 'runningImage'.
pinExecutable :: FilePath -> FilePath -> Handle -> IO FilePath
pinExecutable directory executable running = do
  sweepPinned directory
  pid <- getProcessID
  let pinned = directory </> ("pin-" ++ show pid)
  -- A process admitted under a user scope pins twice at one PID: the second pin
  -- runs from the first, so its runfiles tree is the link this call replaces.
  runfiles <- linkedRunfiles executable
  -- A crashed run with a reused PID can leave either path behind.
  removeIfPresent pinned
  removeIfPresent (pinned ++ ".runfiles")
  withBinaryFile pinned WriteMode $ \copy -> copyChunks running copy
  setFileMode pinned 0o700
  forM_ runfiles $ \tree -> createSymbolicLink tree (pinned ++ ".runfiles")
  pure pinned

-- | On Linux the running inode through @/proc/self/exe@; elsewhere the
-- executable's path, opened before the slot wait, so an unlink and relink of
-- that path during the wait leaves the open image unchanged.
runningImage :: FilePath -> FilePath
runningImage executable
  | os == "linux" = "/proc/self/exe"
  | otherwise = executable

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = removeFile path `catchIOError` \problem ->
  unless (isDoesNotExistError problem) (ioError problem)

copyChunks :: Handle -> Handle -> IO ()
copyChunks source target = do
  chunk <- B.hGet source copyChunkBytes
  unless (B.null chunk) (B.hPut target chunk >> copyChunks source target)

copyChunkBytes :: Int
copyChunkBytes = 1024 * 1024

-- | Bazel tools locate their data at @getExecutablePath ++ ".runfiles"@.
linkedRunfiles :: FilePath -> IO (Maybe FilePath)
linkedRunfiles executable = do
  let runfiles = onDiskPath executable ++ ".runfiles"
  present <- doesDirectoryExist runfiles
  if present then Just <$> canonicalizePath runfiles else pure Nothing

-- | @readlink(2)@ of @/proc/self/exe@ reports the kernel's last path for the
-- inode, with @ (deleted)@ appended once a relink has replaced it.
onDiskPath :: FilePath -> FilePath
onDiskPath path
  | deletedSuffix `isSuffixOf` path = take (length path - length deletedSuffix) path
  | otherwise = path

deletedSuffix :: String
deletedSuffix = " (deleted)"

-- | Removes the pinned copies whose owning process has exited.
sweepPinned :: FilePath -> IO ()
sweepPinned directory = do
  entries <- listDirectory directory
  forM_ entries $ \entry -> case pinnedPid entry of
    Nothing -> pure ()
    Just pid -> do
      alive <- processExists pid
      unless alive $ do
        _ <- try (removeFile (directory </> entry)) :: IO (Either IOException ())
        pure ()

-- | Signal 0 checks existence only; a permission refusal means the process exists.
processExists :: ProcessID -> IO Bool
processExists pid = (signalProcess nullSignal pid >> pure True) `catchIOError` (pure . isPermissionError)

pinnedPid :: String -> Maybe ProcessID
pinnedPid entry = do
  rest <- stripPrefix "pin-" entry
  let digits = maybe rest id (stripSuffix ".runfiles" rest)
  if not (null digits) && all isDigit digits
    then case readMaybe digits :: Maybe Integer of
      Just value | value > 0 && value <= toInteger (maxBound :: ProcessID) -> Just (fromInteger value)
      _ -> Nothing
    else Nothing
  where
    stripSuffix suffix text
      | suffix `isSuffixOf` text = Just (take (length text - length suffix) text)
      | otherwise = Nothing
