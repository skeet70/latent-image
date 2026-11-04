-- | Probe for @//emulation:emulator-slot-tests@: takes an emulator slot
-- through the library, reports reaching the in-slot run and holds the slot
-- until its stdin closes.
module Main (main) where

import Control.Exception (IOException, catch, displayException, evaluate)
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
      putStrLn ("probe-in-slot " ++ fromMaybe "unassigned" slot)
      hFlush stdout
      _ <- getContents >>= evaluate . length
      pure ()
