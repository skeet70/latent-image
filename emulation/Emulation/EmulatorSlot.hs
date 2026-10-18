-- | Host admission for full-system emulator runs. A run holds one of N slot
-- locks under @~/.cache/xm1-emulator-slots/@, shared by every checkout on the
-- host, and executes inside a systemd user scope with a memory cap and no swap.
module Emulation.EmulatorSlot
  ( SlotCount
  , MemoryMax
  , SlotPolicy (..)
  , slotPolicyFromEnvironment
  , reexecInSlot
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (unless)
import Data.Char (isDigit)
import System.Directory (createDirectoryIfMissing, getHomeDirectory)
import System.Environment (getExecutablePath, lookupEnv, setEnv)
import System.FilePath ((</>))
import System.IO (SeekMode (AbsoluteSeek), hFlush, hPutStrLn, stderr)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (executeFile)
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
reexecInSlot :: SlotPolicy -> [String] -> IO ()
reexecInSlot policy arguments = do
  inside <- lookupEnv slotVariable
  case inside of
    Just _ -> pure ()
    Nothing -> do
      Slot slot <- acquire policy
      setEnv slotVariable (show slot)
      executable <- getExecutablePath
      let MemoryMax memory = memoryMax policy
      executeFile "systemd-run" True
        ([ "--user", "--scope", "--quiet", "--collect"
         , "--property=MemoryMax=" ++ memory
         , "--property=MemorySwapMax=0"
         , "--"
         , executable
         ] ++ arguments) Nothing

acquire :: SlotPolicy -> IO Slot
acquire policy = do
  directory <- (</> ".cache" </> "xm1-emulator-slots") <$> getHomeDirectory
  createDirectoryIfMissing True directory
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
