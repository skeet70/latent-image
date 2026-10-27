-- | Service-main worker used by thermal-guard process tests. It validates its
-- actual service MainPID through runGuarded and starts the fixture directly.
module Main (main) where

import Control.Exception (SomeException, try)
import Emulation.ThermalGuard
  ( MilliCelsius (..)
  , PollMicroseconds (..)
  , SysfsRoot (..)
  , ThermalPolicy (..)
  , discoverThermalSensor
  , runGuarded
  , thermalFailureExit
  )
import System.Environment (getArgs)
import System.Exit (ExitCode, exitWith)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [sysfs, fixture, mode, file] -> run sysfs fixture mode file
    _ -> hPutStrLn stderr "usage: thermal-guard-test-worker SYSFS FIXTURE MODE FILE" >> exitWith thermalFailureExit

run :: FilePath -> FilePath -> String -> FilePath -> IO ()
run sysfs fixture mode file = do
  sensor <- discoverThermalSensor (SysfsRoot sysfs)
  case sensor of
    Left reason -> hPutStrLn stderr ("worker: " ++ reason) >> exitWith thermalFailureExit
    Right found -> do
      let policy = ThermalPolicy (MilliCelsius 85000) (PollMicroseconds 50_000)
          fixtureArguments = case mode of
            "exit7" -> ["exit", "7"]
            "exit0" -> ["exit", "0"]
            "descendant" -> ["descendant", file]
            "orphan" -> ["orphan", file]
            "limits" -> ["limits", file]
            _ -> ["exit", "2"]
      outcome <- try (runGuarded (SysfsRoot sysfs) found policy fixture fixtureArguments)
        :: IO (Either SomeException ExitCode)
      case outcome of
        Left failure -> hPutStrLn stderr ("worker: " ++ show failure) >> exitWith thermalFailureExit
        Right code -> exitWith code

