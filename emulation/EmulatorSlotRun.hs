-- | @emulator-slot COMMAND [ARGUMENT...]@ runs an emulator command that does
-- not go through @xm1-viewer@, such as the preserved Python suites, inside an
-- emulator slot. Under @bazel run@ the command starts in the caller's directory.
module Main where

import Control.Exception (IOException, catch, displayException)
import Emulation.EmulatorSlot (attachGuardCommand, reexecInSlot, slotPolicyFromEnvironment)
import System.Directory (setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)
import System.Posix.Process (executeFile)

main :: IO ()
main = run `catch` \failure -> hPutStrLn stderr (displayException (failure :: IOException)) >> exitFailure
  where
    run = do
      arguments <- getArgs
      case arguments of
        [] -> hPutStrLn stderr "usage: emulator-slot COMMAND [ARGUMENT...]" >> exitFailure
        command : rest -> do
          lookupEnv "BUILD_WORKING_DIRECTORY" >>= mapM_ setCurrentDirectory
          policy <- slotPolicyFromEnvironment
          reexecInSlot policy arguments
          attachGuardCommand arguments
          executeFile command True rest Nothing
