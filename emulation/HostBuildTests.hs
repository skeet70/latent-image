-- | Host build admission through the real @host-build@ binary and a fake
-- @bazel@ on PATH: two competing commands serialize, killing the holder admits
-- the waiter and terminates the holder's Bazel, failures keep Bazel's status,
-- and a caller holding an emulator slot is refused.
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (unless, void)
import Data.Char (isDigit)
import Data.List (elemIndex, isInfixOf, isPrefixOf)
import Data.Maybe (fromMaybe, listToMaybe, mapMaybe)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesFileExist, getTemporaryDirectory, makeAbsolute)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeDirectory, (</>))
import System.IO (IOMode (WriteMode), SeekMode (AbsoluteSeek), hClose, hGetContents, openFile)
import System.Posix.Files (setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (signalProcess, sigKILL)
import System.Posix.Types (FileOffset)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe, NoStream, UseHandle), createProcess, getPid, proc, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

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
    "--hold-range" : range -> holdRange range
    [binary] -> makeAbsolute binary >>= run >>= \passed -> unless passed exitFailure
    _ -> ioError (userError "usage: host-build-tests HOST_BUILD | --hold-range LOCK OFFSET READY RELEASE")

-- | A controlled external holder: one byte-range write lock on the shared lock
-- file until @RELEASE@ appears, then the descriptor closes and the lock drops.
holdRange :: [String] -> IO ()
holdRange arguments = case arguments of
  [lockPath, offsetText, readyPath, releasePath] ->
    case (readMaybe offsetText :: Maybe FileOffset) of
      Just offset -> do
        createDirectoryIfMissing True (takeDirectory lockPath)
        fd <- openFd lockPath ReadWrite defaultFileFlags {creat = Just 0o600}
        setLock fd (WriteLock, AbsoluteSeek, offset, 1)
        getProcessID >>= \pid -> writeFile readyPath (show pid)
        waitRelease releasePath (600 :: Int)
        closeFd fd
      Nothing -> ioError (userError "hold-range: OFFSET is not a file offset")
  _ -> ioError (userError "usage: host-build-tests --hold-range LOCK OFFSET READY RELEASE")

waitRelease :: FilePath -> Int -> IO ()
waitRelease path remaining = do
  released <- doesFileExist path
  if released || remaining <= 0
    then pure ()
    else threadDelay 100_000 >> waitRelease path (remaining - 1)

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
    (firstStarted && not secondEarly
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

  self <- getExecutablePath
  let lockPath = home </> ".cache" </> "xm1-host-build" </> "build.lock"
      readyA = root </> "holder-a-ready"
      readyB = root </> "holder-b-ready"
      releaseA = root </> "holder-a-release"
      releaseB = root </> "holder-b-release"
      diagnosticLog = root </> "diagnostic-stderr"
  holderA <- spawnHolder self root lockPath 0 readyA releaseA
  pidA <- waitForPid readyA
  writeFile (release "diagnostic") ""
  diagnostic <- spawnLogged binary root
    (("FAKE_NAME", "diagnostic") : ("FAKE_RELEASE", release "diagnostic") : environment) diagnosticLog
  firstSeen <- case pidA of
    Just owner -> waitFor (fmap (("pid=" ++ owner) `isInfixOf`) (readFile' diagnosticLog))
    Nothing -> pure False
  threadDelay 3_500_000
  firstReportedOnce <- case pidA of
    Just owner -> do
      text <- readFile' diagnosticLog
      pure (length (filter (== owner) (mapMaybe reportedPid (lines text))) == 1)
    Nothing -> pure False
  holderB <- spawnHolder self root lockPath 1 readyB releaseB
  pidB <- waitForPid readyB
  writeFile releaseA ""
  changedSeen <- case pidB of
    Just owner -> waitFor (fmap (("pid=" ++ owner) `isInfixOf`) (readFile' diagnosticLog))
    Nothing -> pure False
  early <- started "diagnostic"
  writeFile releaseB ""
  diagnosticCode <- timeout 20_000_000 (waitForProcess (invocationProcess diagnostic))
  case diagnosticCode of
    Just _ -> pure ()
    Nothing -> do
      getPid (invocationProcess diagnostic) >>= maybe (pure ()) (signalProcess sigKILL)
      void (waitForProcess (invocationProcess diagnostic))
  diagnosticErr <- invocationStderr diagnostic
  _ <- waitForProcess (holderProcess holderA)
  _ <- waitForProcess (holderProcess holderB)
  canonicalRoot <- canonicalizePath root
  let sampled = mapMaybe reportedPid (lines diagnosticErr)
      context = ("cwd=" ++ canonicalRoot) `isInfixOf` diagnosticErr
      count owner = length (filter (== owner) sampled)
      before earlier later = case (elemIndex earlier sampled, elemIndex later sampled) of
        (Just earlierAt, Just laterAt) -> earlierAt < laterAt
        _ -> False
      sequenceOk = case (pidA, pidB) of
        (Just firstPid, Just secondPid) ->
          count firstPid == 1
            && count secondPid >= 1
            && before firstPid secondPid
            && firstPid `notElem` drop 1 (dropWhile (/= secondPid) sampled)
        _ -> False
  holders <- expect "contention names the first and changed kernel owner while admission waits"
    (firstSeen && firstReportedOnce && changedSeen && not early
      && diagnosticCode == Just ExitSuccess && sequenceOk && context)
  unless holders (putStrLn (show (pidA, pidB, diagnosticCode, sampled, context) ++ "\n" ++ diagnosticErr))

  let holderBase = root </> "hb"
      waiterBase = root </> "wb"
      baseLog = root </> "base-stderr"
  baseHolder <- spawnArgs binary root
    (("FAKE_NAME", "base-holder") : ("FAKE_RELEASE", release "base-holder") : environment)
    ["--output_base=" ++ holderBase, "build", "//fake:target"]
  baseHeld <- waitFor (started "base-holder")
  writeFile (release "base-waiter") ""
  baseWaiter <- spawnLoggedArgs binary root
    (("FAKE_NAME", "base-waiter") : ("FAKE_RELEASE", release "base-waiter") : environment)
    ["--output_base=" ++ waiterBase, "build", "//fake:target"] baseLog
  let expectedBase = "output_base=" ++ holderBase ++ " command_log=" ++ (holderBase </> "command.log")
  baseReported <- waitFor (fmap (expectedBase `isInfixOf`) (readFile' baseLog))
  writeFile (release "base-holder") ""
  baseHolderCode <- waitForProcess (invocationProcess baseHolder)
  baseWaiterCode <- waitForProcess (invocationProcess baseWaiter)
  baseErr <- invocationStderr baseWaiter
  baseObserved <- expect "an explicit holder output base is named with its existing command.log"
    (baseHeld && baseReported && baseHolderCode == ExitSuccess && baseWaiterCode == ExitSuccess
      && "\"verdict\":\"passed\"" `isInfixOf` baseErr)

  let overBase = "--output_base=" ++ replicate 600 'x'
      overLog = root </> "over-stderr"
  overHolder <- spawnArgs binary root
    (("FAKE_NAME", "over-holder") : ("FAKE_RELEASE", release "over-holder") : environment)
    [overBase, "build", "//fake:target"]
  overHeld <- waitFor (started "over-holder")
  writeFile (release "over-waiter") ""
  overWaiter <- spawnLoggedArgs binary root
    (("FAKE_NAME", "over-waiter") : ("FAKE_RELEASE", release "over-waiter") : environment)
    ["build", "//fake:target"] overLog
  overReported <- waitFor (fmap ("output_base=unavailable" `isInfixOf`) (readFile' overLog))
  writeFile (release "over-holder") ""
  overHolderCode <- waitForProcess (invocationProcess overHolder)
  overWaiterCode <- waitForProcess (invocationProcess overWaiter)
  overErr <- invocationStderr overWaiter
  overOk <- expect "an over-bound output base is unavailable, not a truncated path"
    (overHeld && overReported && overHolderCode == ExitSuccess && overWaiterCode == ExitSuccess
      && not ("command_log=" `isInfixOf` overErr))

  let cutBase = "--output_base=" ++ replicate 5000 'y'
      cutLog = root </> "cut-stderr"
  cutHolder <- spawnArgs binary root
    (("FAKE_NAME", "cut-holder") : ("FAKE_RELEASE", release "cut-holder") : environment)
    [cutBase, "build", "//fake:target"]
  cutHeld <- waitFor (started "cut-holder")
  writeFile (release "cut-waiter") ""
  cutWaiter <- spawnLoggedArgs binary root
    (("FAKE_NAME", "cut-waiter") : ("FAKE_RELEASE", release "cut-waiter") : environment)
    ["build", "//fake:target"] cutLog
  cutReported <- waitFor (fmap ("output_base=unavailable" `isInfixOf`) (readFile' cutLog))
  writeFile (release "cut-holder") ""
  cutHolderCode <- waitForProcess (invocationProcess cutHolder)
  cutWaiterCode <- waitForProcess (invocationProcess cutWaiter)
  cutErr <- invocationStderr cutWaiter
  cutOk <- expect "a cmdline cut inside output_base is unavailable, not a fabricated path"
    (cutHeld && cutReported && cutHolderCode == ExitSuccess && cutWaiterCode == ExitSuccess
      && not ("command_log=" `isInfixOf` cutErr))

  pure (and [serialized, passed, released, failed, refused, holders, baseObserved, overOk, cutOk])

spawn :: FilePath -> FilePath -> [(String, String)] -> IO Invocation
spawn binary root environment = spawnWith CreatePipe binary root environment ["build", "//fake:target"]

spawnArgs :: FilePath -> FilePath -> [(String, String)] -> [String] -> IO Invocation
spawnArgs binary root environment arguments = spawnWith CreatePipe binary root environment arguments

spawnLogged :: FilePath -> FilePath -> [(String, String)] -> FilePath -> IO Invocation
spawnLogged binary root environment = spawnLoggedArgs binary root environment ["build", "//fake:target"]

spawnLoggedArgs :: FilePath -> FilePath -> [(String, String)] -> [String] -> FilePath -> IO Invocation
spawnLoggedArgs binary root environment arguments logPath = do
  handle <- openFile logPath WriteMode
  invocation <- spawnWith (UseHandle handle) binary root environment arguments
  pure invocation { invocationStderr = hClose handle >> readFile' logPath }

spawnWith :: StdStream -> FilePath -> FilePath -> [(String, String)] -> [String] -> IO Invocation
spawnWith stderrStream binary root environment arguments = do
  (_, _, err, handle) <- createProcess (proc binary arguments)
    { env = Just environment, cwd = Just root, std_err = stderrStream }
  case err of
    Just pipe -> do
      contents <- hGetContents pipe
      pure (Invocation handle (length contents `seq` pure contents))
    Nothing -> pure (Invocation handle (pure ""))

data Holder = Holder
  { holderProcess :: ProcessHandle
  }

spawnHolder :: FilePath -> FilePath -> FilePath -> FileOffset -> FilePath -> FilePath -> IO Holder
spawnHolder self root lockPath offset readyPath releasePath = do
  (_, _, _, handle) <- createProcess
    (proc self ["--hold-range", lockPath, show offset, readyPath, releasePath])
    { cwd = Just root, std_out = NoStream, std_err = NoStream }
  pure (Holder handle)

waitForPid :: FilePath -> IO (Maybe String)
waitForPid path = go (100 :: Int)
  where
    go 0 = readyPid path
    go remaining = do
      found <- readyPid path
      case found of
        Just pid -> pure (Just pid)
        Nothing -> threadDelay 100_000 >> go (remaining - 1)

readyPid :: FilePath -> IO (Maybe String)
readyPid path = do
  exists <- doesFileExist path
  if not exists
    then pure Nothing
    else do
      text <- readFile' path
      let digits = takeWhile isDigit text
      pure (if null digits then Nothing else Just digits)

reportedPid :: String -> Maybe String
reportedPid line = do
  rest <- after "holder pid=" line
  let digits = takeWhile isDigit rest
  if null digits then Nothing else Just digits

after :: String -> String -> Maybe String
after needle text
  | needle `isPrefixOf` text = Just (drop (length needle) text)
  | null text = Nothing
  | otherwise = after needle (drop 1 text)

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
