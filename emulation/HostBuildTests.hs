-- | Host build admission through the real @host-build@ binary and a fake
-- @bazel@ on PATH: two competing commands serialize, killing the holder admits
-- the waiter and terminates the holder's Bazel, failures keep Bazel's status,
-- and a caller holding an emulator slot is refused.
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (unless)
import Data.List (isInfixOf, isPrefixOf)
import Data.Maybe (fromMaybe, listToMaybe)
import System.Directory (createDirectoryIfMissing, doesFileExist, getTemporaryDirectory, makeAbsolute)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hGetContents)
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (signalProcess, sigKILL)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe), createProcess, getPid, proc, waitForProcess)
import System.Timeout (timeout)

fakeBazel :: String
fakeBazel = unlines
  [ "#!/bin/sh"
  , "echo \"start $FAKE_NAME $$ $*\" >> \"$FAKE_LOG\""
  , "while [ ! -e \"$FAKE_RELEASE\" ]; do sleep 0.1; done"
  , "echo \"end $FAKE_NAME\" >> \"$FAKE_LOG\""
  , "exit \"${FAKE_EXIT:-0}\""
  ]

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [binary] -> makeAbsolute binary >>= run >>= \passed -> unless passed exitFailure
    _ -> ioError (userError "usage: host-build-tests HOST_BUILD")

data Invocation = Invocation
  { invocationProcess :: ProcessHandle
  , invocationStderr :: IO String
  }

run :: FilePath -> IO Bool
run binary = do
  temporary <- getTemporaryDirectory
  base <- fromMaybe temporary <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  let root = base </> ("host-build-tests-" ++ show pid)
      home = root </> "home"
      bin = root </> "bin"
      logFile = root </> "bazel.log"
  createDirectoryIfMissing True home
  createDirectoryIfMissing True bin
  writeFile (bin </> "bazel") fakeBazel
  setFileMode (bin </> "bazel") 0o755
  writeFile logFile ""
  environment <- testEnvironment home bin logFile
  let release name = root </> ("release-" ++ name)
      start name extra = spawn binary root (("FAKE_NAME", name) : ("FAKE_RELEASE", release name) : extra ++ environment)
      started name = any (("start " ++ name ++ " ") `isPrefixOf`) . lines <$> readFile' logFile
      startedPid name = do
        entries <- lines <$> readFile' logFile
        pure (listToMaybe [found | line <- entries, "start" : entry : found : _ <- [words line], entry == name])

  first <- start "first" []
  firstStarted <- waitFor (started "first")
  writeFile (release "second") ""
  second <- start "second" []
  threadDelay 1_500_000
  secondEarly <- started "second"
  writeFile (release "first") ""
  firstCode <- waitForProcess (invocationProcess first)
  firstErr <- invocationStderr first
  secondCode <- waitForProcess (invocationProcess second)
  secondErr <- invocationStderr second
  order <- lines <$> readFile' logFile
  let position line = lookup line (zip order [0 :: Int ..])
      startLine name = fromMaybe "" (listToMaybe [line | line <- order, ("start " ++ name ++ " ") `isPrefixOf` line])
  serialized <- expect "a second host build waits for the first to finish"
    (firstStarted && not secondEarly && "Waiting for the host build lock" `isInfixOf` secondErr
      && maybe False id ((<) <$> position "end first" <*> position (startLine "second")))
  passed <- expect "both builds pass with a passed verdict and run bazel --batch"
    (firstCode == ExitSuccess && secondCode == ExitSuccess
      && "\"verdict\":\"passed\"" `isInfixOf` firstErr && "\"verdict\":\"passed\"" `isInfixOf` secondErr
      && " --batch build " `isInfixOf` startLine "first")

  holder <- start "holder" []
  holderStarted <- waitFor (started "holder")
  writeFile (release "waiter") ""
  waiter <- start "waiter" []
  threadDelay 1_000_000
  getPid (invocationProcess holder) >>= maybe (pure ()) (signalProcess sigKILL)
  _ <- waitForProcess (invocationProcess holder)
  waiterCode <- timeout 20_000_000 (waitForProcess (invocationProcess waiter))
  holderPid <- startedPid "holder"
  holderGone <- waitFor (maybe (pure False) (fmap not . running) holderPid)
  lockKept <- doesFileExist (home </> ".cache" </> "xm1-host-build" </> "build.lock")
  waiterErr <- invocationStderr waiter
  released <- expect "killing the lock holder admits the waiter and terminates the holder's bazel"
    (holderStarted && waiterCode == Just ExitSuccess && holderGone && lockKept)
  unless released (putStrLn (show (waiterCode, holderGone, lockKept) ++ "\n" ++ waiterErr))

  writeFile (release "failing") ""
  failing <- start "failing" [("FAKE_EXIT", "3")]
  failingCode <- waitForProcess (invocationProcess failing)
  failingErr <- invocationStderr failing
  failed <- expect "a failing build keeps bazel's exit status in its failed verdict"
    (failingCode == ExitFailure 3 && "\"verdict\":\"failed\"" `isInfixOf` failingErr && "\"exit_code\":3" `isInfixOf` failingErr)

  writeFile (release "slotted") ""
  slotted <- start "slotted" [("XM1_EMULATOR_SLOT", "1")]
  slottedCode <- waitForProcess (invocationProcess slotted)
  slottedRan <- started "slotted"
  refused <- expect "a caller holding an emulator slot is refused without running bazel"
    (slottedCode == ExitFailure 2 && not slottedRan)

  pure (and [serialized, passed, released, failed, refused])

spawn :: FilePath -> FilePath -> [(String, String)] -> IO Invocation
spawn binary root environment = do
  (_, _, Just err, handle) <- createProcess (proc binary ["build", "//fake:target"])
    { env = Just environment, cwd = Just root, std_err = CreatePipe }
  contents <- hGetContents err
  pure (Invocation handle (length contents `seq` pure contents))

waitFor :: IO Bool -> IO Bool
waitFor condition = go (100 :: Int)
  where
    go 0 = condition
    go remaining = do
      satisfied <- condition
      if satisfied then pure True else threadDelay 100_000 >> go (remaining - 1)

readFile' :: FilePath -> IO String
readFile' path = do
  text <- readFile path
  length text `seq` pure text

-- | Fake bazel first on PATH, a private HOME for the lock, and the user
-- manager's bus so systemd-run is reachable from the pruned test environment.
testEnvironment :: FilePath -> FilePath -> FilePath -> IO [(String, String)]
testEnvironment home bin logFile = do
  inherited <- getEnvironment
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
      path = maybe bin ((bin ++ ":") ++) (lookup "PATH" inherited)
      keep name value environment =
        if any ((== name) . fst) environment then environment else (name, value) : environment
      dropped = ["HOME", "PATH", "XM1_EMULATOR_SLOT", "XM1_HOST_BUILD"]
  pure $ ("HOME", home) : ("PATH", path) : ("FAKE_LOG", logFile)
    : filter ((`notElem` dropped) . fst)
      (keep "XDG_RUNTIME_DIR" runtime (keep "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus") inherited))

-- | Whether a process exists and is not a zombie.
running :: String -> IO Bool
running pid = do
  status <- try (readFile' ("/proc" </> pid </> "stat")) :: IO (Either IOException String)
  pure $ case status of
    Right text | state : _ <- words (drop 1 (dropWhile (/= ')') text)) -> state /= "Z"
    Right _ -> True
    Left _ -> False
