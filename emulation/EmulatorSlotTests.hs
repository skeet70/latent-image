-- | Slot admission through the library: two probes limited to one slot each
-- hold both host slots at once, and a third waits, survives replacement of its
-- executable on disk during the wait, and re-executes the original program
-- into the slot the first releases.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless)
import Data.IORef (modifyIORef, newIORef, readIORef)
import Data.List (isInfixOf, isPrefixOf, sort, stripPrefix)
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
  second <- spawnProbe source environment
  secondAdmission <- admission second
  bothOk <- expect "two probes limited to one slot each hold both host slots at once"
    (sort (map admittedSlot [firstAdmission, secondAdmission]) == [Just "1", Just "2"])
  copyFile source probe
  setFileMode probe 0o700
  (third, thirdErr) <- spawnWaitingProbe probe environment
  beginning <- try (hGetLine (probeOut third)) :: IO (Either IOException String)
  waiting <- timeout 10_000_000 (untilWaiting thirdErr)
  startedOk <- expect "a third probe starts while both slots are held"
    (beginning == Right "probe-starting" && waiting == Just True)
  removeFile probe
  writeFile probe replacementScript
  setFileMode probe 0o700
  pending <- try (hWaitForInput (probeOut third) 3000) :: IO (Either IOException Bool)
  finished <- getProcessExitCode (probeProcess third)
  waitedOk <- expect "the third probe still waits after its executable is replaced"
    (pending == Right False && finished == Nothing)
  release first
  thirdAdmission <- admission third
  takenOk <- expect "the third probe takes the slot the first releases"
    (admittedSlot thirdAdmission == admittedSlot firstAdmission)
  release second
  release third
  codes <- mapM (waitForProcess . probeProcess) [first, second, third]
  codeOk <- expect "every probe exits successfully" (all (== ExitSuccess) codes)
  let Admission thirdLines _ = thirdAdmission
  originalOk <- expect "the re-exec runs the original program, not the replacement"
    (not (any ("replacement-ran" `isInfixOf`) thirdLines))
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
  let outcomes = [bothOk, startedOk, waitedOk, takenOk, codeOk, originalOk, sessionlessOk, storageOk]
      shown (Admission seen slot) = show seen ++ " -> " ++ show slot
  unless (and outcomes) $ putStrLn ("probe admissions: " ++ unwords (map shown [firstAdmission, secondAdmission, thirdAdmission, aloneAdmission]))
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

-- | A probe whose stderr is a pipe, for 'untilWaiting'.
spawnWaitingProbe :: FilePath -> [(String, String)] -> IO (Probe, Handle)
spawnWaitingProbe probe environment = do
  started <- createProcess (proc probe []) {std_in = CreatePipe, std_out = CreatePipe, std_err = CreatePipe, env = Just environment}
  case started of
    (Just input, Just out, Just err, process) -> pure (Probe input out process, err)
    _ -> ioError (userError "the probe's stdin, stdout or stderr did not open")

-- | Reads stderr through the announcement that the probe waits for a slot.
untilWaiting :: Handle -> IO Bool
untilWaiting err = do
  line <- try (hGetLine err) :: IO (Either IOException String)
  case line of
    Left _ -> pure False
    Right text | "Waiting for one of the host's" `isPrefixOf` text -> pure True
               | otherwise -> untilWaiting err

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
    : filter ((`notElem` ["XM1_EMULATOR_SLOT", "XM1_STORAGE_MIN_FREE"]) . fst)
      (keep "XDG_RUNTIME_DIR" runtime (keep "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus") inherited)))
