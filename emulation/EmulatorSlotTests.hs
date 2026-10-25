-- | Slot re-exec across a relink: a probe takes the only emulator slot through
-- the library, its executable is replaced on disk while it waits, and the
-- re-exec still runs the original program into the slot.
module Main (main) where

import Control.Monad (unless)
import Data.List (isInfixOf)
import Data.Maybe (fromMaybe)
import System.Directory (copyFile, createDirectoryIfMissing, getTemporaryDirectory, removeFile)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (Handle, hGetContents, hGetLine, SeekMode (AbsoluteSeek))
import System.Posix.Files (setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (getProcessID)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe), createProcess, getProcessExitCode, proc, waitForProcess)

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

run :: FilePath -> IO Bool
run source = do
  temporary <- getTemporaryDirectory
  base <- fromMaybe temporary <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  let root = base </> ("emulator-slot-tests-" ++ show pid)
      home = root </> "home"
      slots = home </> ".cache" </> "xm1-emulator-slots"
      probe = root </> "probe"
  createDirectoryIfMissing True slots
  copyFile source probe
  setFileMode probe 0o700
  held <- openFd (slots </> "slot-1.lock") ReadWrite defaultFileFlags {creat = Just 0o600}
  () <- setLock held (WriteLock, AbsoluteSeek, 0, 0)
  environment <- probeEnvironment home
  (out, process) <- spawnProbe probe environment
  beginning <- hGetLine out
  startedOk <- expect "the probe reports starting before the slot is free" (beginning == "probe-starting")
  removeFile probe
  writeFile probe replacementScript
  setFileMode probe 0o700
  finished <- getProcessExitCode process
  waitedOk <- expect "the probe still waits after its executable is replaced" (finished == Nothing)
  closeFd held
  code <- waitForProcess process
  rest <- hGetContents out
  let output = beginning ++ rest
  codeOk <- expect "the re-exec exits successfully" (code == ExitSuccess)
  slotOk <- expect "the re-exec runs the probe's in-slot code" ("probe-in-slot" `isInfixOf` output)
  originalOk <- expect "the re-exec runs the original program, not the replacement" (not ("replacement-ran" `isInfixOf` output))
  let outcomes = [startedOk, waitedOk, codeOk, slotOk, originalOk]
  unless (and outcomes) (putStrLn ("probe output: " ++ show output))
  pure (and outcomes)

spawnProbe :: FilePath -> [(String, String)] -> IO (Handle, ProcessHandle)
spawnProbe probe environment = do
  started <- createProcess (proc probe []) {std_out = CreatePipe, env = Just environment}
  case started of
    (_, Just out, _, process) -> pure (out, process)
    _ -> ioError (userError "the probe's stdout did not open")

-- | A private HOME so the probe's slot locks are its own, one slot so it must
-- wait, and no inherited slot claim. The user bus settings keep systemd-run
-- reachable when the test runner has pruned the environment.
probeEnvironment :: FilePath -> IO [(String, String)]
probeEnvironment home = do
  inherited <- getEnvironment
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
      keep name value environment =
        if any ((== name) . fst) environment then environment else (name, value) : environment
  pure (("HOME", home) : ("XM1_EMULATOR_SLOTS", "1")
    : filter ((/= "XM1_EMULATOR_SLOT") . fst)
      (keep "XDG_RUNTIME_DIR" runtime (keep "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus") inherited)))
