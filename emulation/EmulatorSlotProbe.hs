-- | Probe for @//emulation:emulator-slot-tests@: takes an emulator slot
-- through the library, reports reaching the in-slot run and holds the slot
-- until its stdin closes.
module Main (main) where

import Control.Exception (IOException, catch, displayException, evaluate, try)
import Data.Maybe (fromMaybe)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import System.Environment (lookupEnv)
import System.Exit (exitFailure)
import System.IO (hFlush, hPutStrLn, stdout, stderr)

main :: IO ()
main = run `catch` \failure -> do
  hPutStrLn stderr ("probe-failed " ++ displayException (failure :: IOException))
  exitFailure
  where
    run = do
      putStrLn "probe-starting"
      hFlush stdout
      policy <- slotPolicyFromEnvironment
      reexecInSlot policy []
      slot <- lookupEnv "XM1_EMULATOR_SLOT"
      groups <- readFile "/proc/self/cgroup"
      case [drop 3 line | line <- lines groups, take 3 line == "0::"] of
        [group] -> do
          limit <- try (readFile ("/sys/fs/cgroup" ++ group ++ "/memory.max") >>= \text -> evaluate (length text) >> pure text)
            :: IO (Either IOException String)
          case limit of
            Right value -> putStrLn ("probe-memory-max " ++ unwords (words value))
            Left _ -> pure ()
        _ -> pure ()
      putStrLn ("probe-in-slot " ++ fromMaybe "unassigned" slot)
      hFlush stdout
      _ <- getContents >>= evaluate . length
      pure ()
