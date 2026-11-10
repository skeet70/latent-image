-- | Probe for @//emulation:emulator-slot-tests@: takes an emulator slot
-- through the library, reports reaching the in-slot run and holds the slot
-- until its stdin closes.
module Main (main) where

import Control.Exception (IOException, catch, displayException, evaluate, try)
import Data.Maybe (fromMaybe)
import Emulation.EmulatorSlot (ExecutionClass (..), reexecInSlot, slotPolicyForClass)
import System.Environment (getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitWith)
import System.Posix.Process (getProcessID)
import System.Process (createProcess, getPid, proc, readProcessWithExitCode)
import Text.Read (readMaybe)
import System.IO (hFlush, hPutStrLn, stdout, stderr)

main :: IO ()
main = run `catch` \failure -> do
  hPutStrLn stderr ("probe-failed " ++ displayException (failure :: IOException))
  exitFailure
  where
    run = do
      putStrLn "probe-starting"
      hFlush stdout
      selected <- lookupEnv "XM1_PROBE_CLASS"
      legacy <- lookupEnv "XM1_PROBE_LEGACY"
      policy <- slotPolicyForClass (if selected == Just "native" then NativeVerification else FullSystem)
      case legacy of
        Just "1" -> pure ()
        _ -> reexecInSlot policy []
      slot <- lookupEnv "XM1_EMULATOR_SLOT"
      groups <- readFile "/proc/self/cgroup"
      case [drop 3 line | line <- lines groups, take 3 line == "0::"] of
        [group] -> do
          putStrLn ("probe-unit " ++ reverse (takeWhile (/= '/') (reverse group)))
          limit <- try (readFile ("/sys/fs/cgroup" ++ group ++ "/memory.max") >>= \text -> evaluate (length text) >> pure text)
            :: IO (Either IOException String)
          case limit of
            Right value -> putStrLn ("probe-memory-max " ++ unwords (words value))
            Left _ -> pure ()
        _ -> pure ()
      pid <- getProcessID
      putStrLn ("probe-pid " ++ show pid)
      survivor <- lookupEnv "XM1_PROBE_SURVIVOR"
      case survivor of
        Just "1" -> do
          (_, _, _, child) <- createProcess (proc "sleep" ["600"])
          childPid <- getPid child
          putStrLn ("probe-survivor " ++ maybe "unknown" show childPid)
        _ -> pure ()
      nested <- lookupEnv "XM1_PROBE_NESTED"
      case nested of
        Just "1" -> do
          self <- getExecutablePath
          (code, _, _) <- readProcessWithExitCode self [] ""
          putStrLn ("probe-nested-exit " ++ show code)
        _ -> pure ()
      putStrLn ("probe-in-slot " ++ fromMaybe "unassigned" slot)
      hFlush stdout
      _ <- getContents >>= evaluate . length
      status <- lookupEnv "XM1_PROBE_EXIT"
      exitWith (maybe ExitSuccess ExitFailure (status >>= readMaybe))
