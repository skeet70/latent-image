-- | Host admission for full-system emulator runs. A run holds one of N slot
-- locks under @~/.cache/xm1-emulator-slots/@, shared by every checkout on the
-- host, and executes inside a systemd user scope with a memory cap and no swap.
module Emulation.EmulatorSlot
  ( SlotCount (..)
  , MemoryMax
  , SlotPolicy (..)
  , slotPolicyFromEnvironment
  , reexecInSlot
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.List (isSuffixOf, stripPrefix)
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, getHomeDirectory, listDirectory, removeFile)
import System.Environment (getExecutablePath, lookupEnv, setEnv)
import System.FilePath ((</>))
import System.IO (Handle, IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (catchIOError, isDoesNotExistError)
import System.Posix.Files (createSymbolicLink, setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (executeFile, getProcessID)
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
    Just _ -> pure ()
    Nothing -> do
      directory <- slotDirectory
      Slot slot <- acquire directory policy
      setEnv slotVariable (show slot)
      executable <- pinExecutable directory
      let MemoryMax memory = memoryMax policy
      executeFile "systemd-run" True
        ([ "--user", "--scope", "--quiet", "--collect"
         , "--property=MemoryMax=" ++ memory
         , "--property=MemorySwapMax=0"
         , "--"
         , executable
         ] ++ arguments) Nothing

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

-- | Removes pinned copies whose PID is gone. The scope's parent process holds
-- the PID until the run ends, and a run reads its own pinned path throughout,
-- so only that absence makes a copy dead.
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

pinnedPid :: String -> Maybe Int
pinnedPid entry = do
  rest <- stripPrefix "pin-" entry
  let (digits, suffix) = span isDigit rest
  if not (null digits) && suffix `elem` ["", ".runfiles"] then readMaybe digits else Nothing
