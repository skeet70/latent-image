-- | Whole-process-tree admission for native suites and full-system emulators.
module Emulation.EmulatorSlot
  ( SlotCount (..)
  , MemoryMax
  , SlotPolicy (..)
  , ExecutionClass (..)
  , slotPolicyForClass
  , slotPolicyFromEnvironment
  , reexecInSlot
  , insideSlot
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.List (isSuffixOf, stripPrefix)
import qualified Emulation.HostAdmission as Host
import Emulation.HostAdmission (MemoryMax (..), ResourceClass (Emulator), ScopeName, claimScopeRecord, classConcurrency, classMemoryMax, memoryMaxBytes, parseMemoryMax, parseScopeName, reserveMemory, userScopeAvailable)
import FujiAF.StorageBudget (checkStorageBudget)
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, getCurrentDirectory, getHomeDirectory, listDirectory, removeFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.FilePath ((</>))
import System.IO (Handle, IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (createSymbolicLink, setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, getLock, openFd, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Types (ProcessID)
import Text.Read (readMaybe)

newtype SlotCount = SlotCount Int

data ExecutionClass = NativeVerification | FullSystem
  deriving (Eq, Read, Show)

data SlotPolicy = SlotPolicy
  { slotCount :: SlotCount
  , memoryMax :: MemoryMax
  , executionClass :: ExecutionClass
  }

newtype Slot = Slot Int

slotVariable, slotsVariable, memoryVariable :: String
slotVariable = "XM1_EMULATOR_SLOT"
slotsVariable = "XM1_EMULATOR_SLOTS"
memoryVariable = "XM1_EMULATOR_MEMORY_MAX"


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
      memoryName = if execution == FullSystem then memoryVariable else "XM1_NATIVE_MEMORY_MAX"
      MemoryMax cap = classMemoryMax resource
  slotsText <- lookupEnv slotsName
  slots <- case slotsText >>= readMaybe of
    Just count | count >= 1 && count <= maximumSlots -> pure (SlotCount count)
    Nothing | slotsText == Nothing -> pure (SlotCount maximumSlots)
    _ -> ioError (userError (slotsName ++ " must be an integer 1.." ++ show maximumSlots))
  memoryText <- lookupEnv memoryName
  memory <- case memoryText of
    Nothing -> pure (if execution == NativeVerification then MemoryMax "2G" else classMemoryMax resource)
    Just text -> case parseMemoryMax text of
      Just value | memoryMaxBytes value <= memoryMaxBytes (MemoryMax cap) -> pure value
      _ -> ioError (userError (memoryName ++ " must be positive and at most " ++ cap))
  pure (SlotPolicy slots memory execution)

-- | Re-execs a pinned inode into a capped scope before taking resource locks.
reexecInSlot :: SlotPolicy -> [String] -> IO ()
reexecInSlot policy arguments = do
  let resource = resourceClass (executionClass policy)
      SlotCount requestedSlots = slotCount policy
  unless (requestedSlots >= 1 && requestedSlots <= classConcurrency resource
      && memoryMaxBytes (memoryMax policy) <= memoryMaxBytes (classMemoryMax resource)) $
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
      pending <- lookupEnv "XM1_ADMISSION_SCOPE"
      case pending of
        Just unit -> do
          groups <- readFile "/proc/self/cgroup"
          unless (any (\line -> reverse (take (length unit) (reverse line)) == unit) (lines groups)) $
            ioError (userError "admission scope does not contain this process")
          scope <- maybe (ioError (userError "invalid admission scope")) pure (parseScopeName unit)
          Slot slot <- acquire directory (executionClass policy) scope
          _ <- reserveMemory (memoryMax policy) scope
          checkStorageBudget storage
          unsetEnv "XM1_ADMISSION_SCOPE"
          setEnv slotVariable (show slot)
          pid <- getProcessID
          setEnv "XM1_ADMISSION_OWNER" (show pid)
          setEnv "XM1_ADMISSION_CLASS" (show (executionClass policy))
          executable <- pinExecutable directory
          executeFile executable False arguments Nothing
        Nothing -> do
          scoped <- userScopeAvailable
          unless scoped (ioError (userError "verification admission requires a systemd user scope"))
          executable <- pinExecutable directory
          pid <- getProcessID
          let unit = "xm1-verification-" ++ show pid ++ ".scope"
              MemoryMax memory = memoryMax policy
          setEnv "XM1_ADMISSION_SCOPE" unit
          executeFile "systemd-run" True
            ([ "--user", "--scope", "--quiet", "--collect", "--unit=" ++ unit
             , "--property=MemoryMax=" ++ memory
             , "--property=MemorySwapMax=0", "--property=OOMPolicy=stop"
             , "--", executable
             ] ++ arguments) Nothing

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

acquire :: FilePath -> ExecutionClass -> ScopeName -> IO Slot
acquire directory execution scope = do
  let maximumSlots = classConcurrency (resourceClass execution)
      prefix = if execution == FullSystem then "slot-" else "native-"
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
      firstFree prefix (slot : rest) = do
        fd <- openFd (directory </> (prefix ++ show slot ++ ".lock")) ReadWrite defaultFileFlags {creat = Just 0o600}
        locked <- try (setLock fd (WriteLock, AbsoluteSeek, 0, 0)) :: IO (Either IOException ())
        case locked of
          Right () -> do
            claimScopeRecord (directory </> (prefix ++ show slot ++ ".scope")) scope
            pure (Just (Slot slot))
          Left _ -> do
            owner <- getLock fd (WriteLock, AbsoluteSeek, 0, 0)
            hPutStrLn stderr ("host admission: " ++ prefix ++ show slot ++ " owner=" ++ show owner)
            closeFd fd
            firstFree prefix rest
  attempt

-- | A private copy of the running executable beneath the slot directory, named
-- @pin-<pid>@ for the PID execve preserves. The bytes come from the running
-- inode through @/proc/self/exe@, so no relink of the on-disk file changes them.
pinExecutable :: FilePath -> IO FilePath
pinExecutable directory = do
  sweepPinned directory
  pid <- getProcessID
  executable <- getExecutablePath
  let pinned = directory </> ("pin-" ++ show pid)
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

-- | Removes the pinned copies whose owning process has exited.
sweepPinned :: FilePath -> IO ()
sweepPinned directory = do
  entries <- listDirectory directory
  forM_ entries $ \entry -> case pinnedPid entry of
    Nothing -> pure ()
    Just pid -> do
      alive <- doesDirectoryExist ("/proc" </> show pid)
      unless alive $ do
        _ <- try (removeFile (directory </> entry)) :: IO (Either IOException ())
        pure ()

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
