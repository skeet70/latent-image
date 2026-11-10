module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (replicateM, unless)
import Data.IORef (modifyIORef, newIORef, readIORef)
import Data.List (isInfixOf, sort, stripPrefix)
import Data.Maybe (fromMaybe)
import System.Directory (copyFile, createDirectoryIfMissing, getTemporaryDirectory, removeFile)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (Handle, hClose, hGetLine, hWaitForInput)
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe), createProcess, getProcessExitCode, proc, readCreateProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

replacementScript :: String
replacementScript = unlines
  [ "#!/bin/sh"
  , "echo replacement-ran"
  , "exit 42"
  ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [source] -> run source >>= \passed -> unless passed exitFailure
    _ -> ioError (userError "usage: emulator-slot-tests PROBE")

data Probe = Probe
  { probeIn :: Handle
  , probeOut :: Handle
  , probeProcess :: ProcessHandle
  }

-- | The lines a probe printed up to its in-slot report, and the slot it reports.
data Admission = Admission [String] (Maybe String)

admittedSlot :: Admission -> Maybe String
admittedSlot (Admission _ slot) = slot

run :: FilePath -> IO Bool
run source = do
  temporary <- getTemporaryDirectory
  base <- fromMaybe temporary <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  let root = base </> ("emulator-slot-tests-" ++ show pid)
      home = root </> "home"
      probe = root </> "probe"
  createDirectoryIfMissing True (home </> ".cache" </> "xm1-emulator-slots")
  environment <- probeEnvironment home
  first <- spawnProbe source environment
  firstAdmission <- admission first
  others <- replicateM 5 $ do
    other <- spawnProbe source environment
    observed <- admission other
    pure (other, observed)
  let admissions = firstAdmission : map snd others
  bothOk <- expect "six independent probes hold all host slots concurrently"
    (sort (map admittedSlot admissions) == map (Just . show) [1 .. 6 :: Int])
  memoryOk <- expect "all admitted scopes enforce the 4 GiB verification cap"
    (all (\(Admission seen _) -> "probe-memory-max 4294967296" `elem` seen) admissions)
  copyFile source probe
  setFileMode probe 0o700
  waiting <- spawnProbe probe environment
  beginning <- try (hGetLine (probeOut waiting)) :: IO (Either IOException String)
  startedOk <- expect "a seventh probe starts while all slots are held" (beginning == Right "probe-starting")
  removeFile probe
  writeFile probe replacementScript
  setFileMode probe 0o700
  pending <- try (hWaitForInput (probeOut waiting) 3000) :: IO (Either IOException Bool)
  finished <- getProcessExitCode (probeProcess waiting)
  waitedOk <- expect "the waiting probe survives replacement of its executable"
    (pending == Right False && finished == Nothing)
  release first
  waitingAdmission <- admission waiting
  takenOk <- expect "the waiting probe takes the released slot"
    (admittedSlot waitingAdmission == admittedSlot firstAdmission)
  mapM_ (release . fst) others
  release waiting
  codes <- mapM (waitForProcess . probeProcess) (first : map fst others ++ [waiting])
  codeOk <- expect "every probe exits successfully" (all (== ExitSuccess) codes)
  let Admission waitingLines _ = waitingAdmission
  originalOk <- expect "the re-exec runs the original program, not the replacement"
    (not (any ("replacement-ran" `isInfixOf`) waitingLines))
  let sessionless = root </> "probe-sessionless"
  copyFile source sessionless
  setFileMode sessionless 0o700
  alone <- spawnProbe sessionless (sessionlessEnvironment root environment)
  aloneAdmission <- admission alone
  release alone
  aloneCode <- waitForProcess (probeProcess alone)
  sessionlessOk <- expect "without a user session the probe still runs in its slot"
    (aloneCode == ExitSuccess && admittedSlot aloneAdmission /= Nothing)
  let lowStorageEnvironment = ("XM1_STORAGE_MIN_FREE", "999999999999999999999T")
        : filter ((/= "XM1_STORAGE_MIN_FREE") . fst) environment
  (lowStorageCode, lowStorageOutput, _) <- readCreateProcessWithExitCode
    (proc source []) {env = Just lowStorageEnvironment} ""
  storageOk <- expect "insufficient disk reserve prevents an emulator-slot admission"
    (lowStorageCode /= ExitSuccess && not (any (isInfixOf "probe-in-slot ") (lines lowStorageOutput)))
  let overBudgetEnvironment = ("XM1_EMULATOR_MEMORY_MAX", "8G")
        : filter ((/= "XM1_EMULATOR_MEMORY_MAX") . fst) environment
  (overBudgetCode, overBudgetOutput, _) <- readCreateProcessWithExitCode
    (proc source []) {env = Just overBudgetEnvironment} ""
  capOk <- expect "a scope above the 4 GiB class budget is refused"
    (overBudgetCode /= ExitSuccess && not (any (isInfixOf "probe-in-slot ") (lines overBudgetOutput)))
  let outcomes = [bothOk, memoryOk, startedOk, waitedOk, takenOk, codeOk, originalOk, sessionlessOk, storageOk, capOk]
      shown (Admission seen slot) = show seen ++ " -> " ++ show slot
  unless (and outcomes) $ putStrLn ("probe admissions: " ++ unwords (map shown (admissions ++ [waitingAdmission, aloneAdmission])))
  pure (and outcomes)

-- | Reads a probe's stdout through its in-slot report, to end of file, or for
-- at most 10 seconds.
admission :: Probe -> IO Admission
admission probe = do
  seen <- newIORef []
  slot <- timeout 10_000_000 (go seen)
  Admission <$> (reverse <$> readIORef seen) <*> pure (fromMaybe Nothing slot)
  where
    go seen = do
      line <- try (hGetLine (probeOut probe)) :: IO (Either IOException String)
      case line of
        Left _ -> pure Nothing
        Right text -> do
          modifyIORef seen (text :)
          maybe (go seen) (pure . Just) (stripPrefix "probe-in-slot " text)

-- | Closes the probe's stdin, which ends its in-slot run.
release :: Probe -> IO ()
release = hClose . probeIn

-- | The probe environment with the user bus pointed at a directory that does
-- not exist, so @systemctl --user@ cannot reach a manager.
sessionlessEnvironment :: FilePath -> [(String, String)] -> [(String, String)]
sessionlessEnvironment root environment =
  ("XDG_RUNTIME_DIR", missing) : ("DBUS_SESSION_BUS_ADDRESS", "unix:path=" ++ missing ++ "/bus")
    : filter ((`notElem` ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"]) . fst) environment
  where
    missing = root </> "no-session"

spawnProbe :: FilePath -> [(String, String)] -> IO Probe
spawnProbe probe environment = do
  started <- createProcess (proc probe []) {std_in = CreatePipe, std_out = CreatePipe, env = Just environment}
  case started of
    (Just input, Just out, _, process) -> pure (Probe input out process)
    _ -> ioError (userError "the probe's stdin or stdout did not open")

-- | A private HOME so the probe's slot locks are its own, one slot per
-- invocation, and no inherited slot claim. The user bus settings keep
-- systemd-run reachable when the test runner has pruned the environment.
probeEnvironment :: FilePath -> IO [(String, String)]
probeEnvironment home = do
  inherited <- getEnvironment
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
      keep name value environment =
        if any ((== name) . fst) environment then environment else (name, value) : environment
  pure (("HOME", home) : ("XM1_EMULATOR_SLOTS", "1") : ("XM1_STORAGE_MIN_FREE", "1")
    : filter ((`notElem` ["HOME", "XM1_EMULATOR_SLOTS", "XM1_EMULATOR_SLOT", "XM1_EMULATOR_MEMORY_MAX", "XM1_STORAGE_MIN_FREE"]) . fst)
      (keep "XDG_RUNTIME_DIR" runtime (keep "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus") inherited)))
