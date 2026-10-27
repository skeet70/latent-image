-- | Internal worker entry. It runs only as the MainPID of the transient
-- service created by the public thermal-guard launcher.
module Main (main) where

import Control.Exception (SomeException, try)
import Emulation.ThermalGuard
  ( SysfsRoot (..)
  , discoverThermalSensor
  , productionThermalPolicy
  , runGuarded
  , thermalFailureExit
  )
import System.Environment (getArgs)
import System.Exit (ExitCode, exitFailure, exitWith)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "--" : command : rest -> do
      sensor <- discoverThermalSensor (SysfsRoot "/sys")
      case sensor of
        Left reason -> failClosed reason
        Right found -> do
          outcome <- try (runGuarded (SysfsRoot "/sys") found productionThermalPolicy command rest)
            :: IO (Either SomeException ExitCode)
          case outcome of
            Left failure -> failClosed (show failure)
            Right code -> exitWith code
    _ -> hPutStrLn stderr "usage: thermal-guard-worker -- COMMAND [ARGUMENT...]" >> exitFailure

failClosed :: String -> IO a
failClosed reason = do
  hPutStrLn stderr ("thermal-guard-worker: " ++ reason)
  exitWith thermalFailureExit
