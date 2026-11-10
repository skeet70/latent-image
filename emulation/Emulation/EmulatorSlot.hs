-- | Admission for full-system emulator runs, the 'Emulator' class of
-- "Emulation.HostAdmission". A run holds one of the host's 'hostSlots' slot
-- locks under @~/.cache/xm1-emulator-slots/@, whichever is free, shared by every
-- checkout and agent on the host, and runs inside a systemd user scope with a
-- memory cap and no swap when the host has a systemd user session.
-- @XM1_EMULATOR_SLOTS@ is how many slots a caller such as verify-candidate may hold at once.
module Emulation.EmulatorSlot
  ( SlotCount (..)
  , MemoryMax
  , SlotPolicy (..)
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
import Emulation.HostAdmission (MemoryMax (..), ResourceClass (Emulator), classConcurrency, classMemoryMax, memoryMaxBytes, parseMemoryMax, userScopeAvailable)
import FujiAF.StorageBudget (checkStorageBudget)
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, getCurrentDirectory, getHomeDirectory, listDirectory, removeFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv)
import System.FilePath ((</>))
import System.IO (Handle, IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (createSymbolicLink, setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (executeFile, getProcessID)
import System.Posix.Types (ProcessID)
import Text.Read (readMaybe)

newtype SlotCount = SlotCount Int

data SlotPolicy = SlotPolicy
  { slotCount :: SlotCount
  , memoryMax :: MemoryMax
  }

newtype Slot = Slot Int

slotVariable, slotsVariable, memoryVariable :: String
slotVariable = "XM1_EMULATOR_SLOT"
slotsVariable = "XM1_EMULATOR_SLOTS"
memoryVariable = "XM1_EMULATOR_MEMORY_MAX"

hostSlots :: Int
hostSlots = classConcurrency Emulator

-- | Whether this process or an ancestor holds a slot.
insideSlot :: IO Bool
insideSlot = maybe False (const True) <$> lookupEnv slotVariable

-- | All host slots at 4G by default; @XM1_EMULATOR_SLOTS@ (1..'hostSlots') and
-- @XM1_EMULATOR_MEMORY_MAX@ (at most the class cap) override them.
slotPolicyFromEnvironment :: IO SlotPolicy
slotPolicyFromEnvironment = do
  slots <- lookupEnv slotsVariable >>= maybe (pure (SlotCount hostSlots)) parseSlots
  memory <- lookupEnv memoryVariable >>= maybe (pure (classMemoryMax Emulator)) parseMemory
  pure (SlotPolicy slots memory)
  where
    parseSlots text = case readMaybe text of
      Just count | count >= 1 && count <= hostSlots -> pure (SlotCount count)
      _ -> ioError (userError (slotsVariable ++ " must be an integer 1.." ++ show hostSlots ++ ", the host's slots"))
    MemoryMax cap = classMemoryMax Emulator
    parseMemory text = case parseMemoryMax text of
      Just memory | memoryMaxBytes memory <= memoryMaxBytes (MemoryMax cap) -> pure memory
      _ -> ioError (userError (memoryVariable ++ " must be a positive byte count with an optional K, M, G or T suffix, at most " ++ cap))

-- | Returns inside a slot. Outside one, waits for a free slot, then replaces
-- the process with itself, under @systemd-run --user --scope@ when a user
-- session answers. The slot is a POSIX record lock, which execve preserves, so
-- it is held until the run exits.
--
-- The re-exec runs a pinned copy of the running executable, never its original
-- path: a concurrent build relinks that path while the wait runs.
reexecInSlot :: SlotPolicy -> [String] -> IO ()
reexecInSlot policy arguments = do
  storage <- lookupEnv "XM1_STORAGE_PATH" >>= maybe defaultStoragePath pure
  checkStorageBudget storage
  inside <- lookupEnv slotVariable
  case inside of
    Just _ -> pure ()
    Nothing -> do
      directory <- slotDirectory
      Slot slot <- acquire directory
      checkStorageBudget storage
      setEnv slotVariable (show slot)
      executable <- pinExecutable directory
      scoped <- userScopeAvailable
      let MemoryMax memory = memoryMax policy
      if scoped
        then executeFile "systemd-run" True
          ([ "--user", "--scope", "--quiet", "--collect"
           , "--property=MemoryMax=" ++ memory
           , "--property=MemorySwapMax=0"
           , "--"
           , executable
           ] ++ arguments) Nothing
        else do
          hPutStrLn stderr ("No systemd user session: emulator slot " ++ show slot ++ " runs without the " ++ memory ++ " memory cap")
          hFlush stderr
          executeFile executable False arguments Nothing

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

acquire :: FilePath -> IO Slot
acquire directory = do
  let attempt announced = do
        free <- firstFree directory [1 .. hostSlots]
        case free of
          Just slot -> pure slot
          Nothing -> do
            unless announced $ do
              hPutStrLn stderr ("Waiting for one of the host's " ++ show hostSlots ++ " emulator slots in " ++ directory)
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
