-- | @thermal-guard -- COMMAND [ARGUMENT...]@
--
-- Always starts one owned transient service whose MainPID is the internal
-- worker. Canonical callers reach this entry through the pinned guard.
module Main (main) where

import Control.Exception (IOException, SomeException, catch, onException, try)
import Data.Char (isAsciiLower, isAsciiUpper, isDigit)
import Data.List (stripPrefix)
import Data.Unique (hashUnique, newUnique)
import Emulation.ThermalGuard (callerCpuProperties, callerMemoryProperties)
import System.Directory (canonicalizePath, doesFileExist, getCurrentDirectory)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitWith)
import System.FilePath ((</>), takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)
import System.Posix.Process (getProcessID)
import System.Process (createProcess, proc, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "--" : command : rest -> startService command rest
    _ -> hPutStrLn stderr "usage: thermal-guard -- COMMAND [ARGUMENT...]" >> exitFailure

startService :: FilePath -> [String] -> IO ()
startService command rest = do
  self <- canonicalizePath =<< getExecutablePath
  let name = takeFileName self
      worker = case stripPrefix "guard-" name of
        Just suffix ->
          let (digits, _) = span isDigit suffix
          in takeDirectory self </> ("guard-worker-" ++ digits)
        Nothing -> takeDirectory self </> "thermal-guard-worker"
  present <- doesFileExist worker
  if not present then failClosed ("internal worker is missing: " ++ worker) else do
    memory <- callerMemoryProperties
    cpus <- callerCpuProperties `catch` \failure -> failClosed (show (failure :: IOException))
    environment <- getEnvironment
    working <- getCurrentDirectory
    pid <- getProcessID
    slot <- lookupEnv "XM1_EMULATOR_SLOT"
    unit <- case slot of
      Just value -> pure ("thermal-guard-slot-" ++ value ++ ".service")
      Nothing -> do
        unique <- hashUnique <$> newUnique
        pure ("thermal-guard-" ++ show pid ++ "-" ++ show unique ++ ".service")
    let environmentArgs =
          [ "--setenv=" ++ variable
          | (key, _) <- environment
          , Just (EnvironmentName variable) <- [parseEnvironmentName key]
          ]
        arguments =
          [ "--user", "--wait", "--pipe", "--collect"
          , "--unit=" ++ unit
          , "--property=KillMode=control-group"
          , "--property=TimeoutStopSec=10s"
          , "--property=WorkingDirectory=" ++ working
          ] ++ memory ++ cpus
          ++ environmentArgs
          ++ ["--", worker, "--", command] ++ rest
        cleanup = do
          _ <- timeout 10_000_000 (readProcessWithExitCode "systemctl"
            ["--user", "stop", unit] "")
          pure ()
    outcome <- try (do
      (_, _, _, process) <- createProcess (proc "systemd-run" arguments)
      waitForProcess process `onException` cleanup)
      :: IO (Either SomeException ExitCode)
    case outcome of
      Left failure -> failClosed (show failure)
      Right code -> exitWith code

failClosed :: String -> IO a
failClosed reason = do
  hPutStrLn stderr ("thermal-guard: " ++ reason)
  exitWith (ExitFailure 75)

-- | A name in systemd's environment-name grammar: ASCII letters, digits and
-- @_@, not starting with a digit. systemd-run rejects any other @--setenv@ name.
newtype EnvironmentName = EnvironmentName String

parseEnvironmentName :: String -> Maybe EnvironmentName
parseEnvironmentName name = case name of
  first : _ | not (isDigit first) && all allowed name -> Just (EnvironmentName name)
  _ -> Nothing
  where
    allowed character = isAsciiUpper character || isAsciiLower character || isDigit character || character == '_'
