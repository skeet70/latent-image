-- | Host build admission through the real @host-build@ binary.
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM, unless)
import Data.Char (isDigit)
import Data.List (elemIndex, isInfixOf, isPrefixOf)
import Data.Maybe (fromMaybe, listToMaybe, mapMaybe)
import GHC.IO.Encoding (setForeignEncoding)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesFileExist, getTemporaryDirectory, makeAbsolute)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeDirectory, (</>))
import System.IO (BufferMode (LineBuffering), IOMode (WriteMode), SeekMode (AbsoluteSeek), hClose, hGetContents, hSetBuffering, openFile, stdout, utf8)
import System.Posix.Files (setFileMode)
import System.Posix.IO (LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setLock)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (signalProcess, sigCONT, sigKILL, sigSTOP)
import System.Posix.Types (FileOffset)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe, NoStream, UseHandle), createProcess, getPid, proc, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

fakeBazel :: String
fakeBazel = unlines
  [ "#!/bin/sh"
  , "echo \"start $FAKE_NAME $$ $*\" >> \"$FAKE_LOG\""
  , "while IFS=: read -r hierarchy controllers group; do"
  , "  if [ \"$hierarchy\" = 0 ]; then"
  , "    read -r memory < \"/sys/fs/cgroup$group/memory.max\""
  , "    read -r swap < \"/sys/fs/cgroup$group/memory.swap.max\""
  , "    echo \"memory $FAKE_NAME $memory $swap\" >> \"$FAKE_LOG\""
  , "  fi"
  , "done < /proc/self/cgroup"
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
  hSetBuffering stdout LineBuffering
  setForeignEncoding utf8
  arguments <- getArgs
  case arguments of
    "--hold-range" : range -> holdRange 1 range
    "--hold-all" : range -> holdRange 0 range
    [binary] -> makeAbsolute binary >>= run >>= \passed -> unless passed exitFailure
    _ -> ioError (userError "usage: host-build-tests HOST_BUILD | --hold-range LOCK OFFSET READY RELEASE")

-- | A controlled external holder: one byte-range write lock on the shared lock
-- file until @RELEASE@ appears, then the descriptor closes and the lock drops.
holdRange :: FileOffset -> [String] -> IO ()
holdRange size arguments = case arguments of
  [lockPath, offsetText, readyPath, releasePath] ->
    case (readMaybe offsetText :: Maybe FileOffset) of
      Just offset -> do
        createDirectoryIfMissing True (takeDirectory lockPath)
        fd <- openFd lockPath ReadWrite defaultFileFlags {creat = Just 0o600}
        setLock fd (WriteLock, AbsoluteSeek, offset, size)
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

  writeFile (release "low-storage") ""
  lowStorage <- start "low-storage" [("XM1_STORAGE_MIN_FREE", "999999999999999999999T")]
  lowStorageCode <- waitForProcess (invocationProcess lowStorage)
  lowStorageRan <- started "low-storage"
  storageRefused <- expect "insufficient free space refuses a new build before bazel runs"
    (lowStorageCode == ExitFailure 75 && not lowStorageRan)
  writeFile (release "invalid-reserve") ""
  invalidReserve <- start "invalid-reserve" [("XM1_STORAGE_MIN_FREE", "0")]
  invalidReserveCode <- waitForProcess (invocationProcess invalidReserve)
  invalidReserveRan <- started "invalid-reserve"
  invalidReserveRefused <- expect "an invalid storage reserve cannot authorize a build"
    (invalidReserveCode == ExitFailure 75 && not invalidReserveRan)

  memoryCases <- forM
    [ ("default-memory", [], "12884901888 8589934592")
    , ("override-memory", [("XM1_HOST_BUILD_MEMORY_MAX", "24G"), ("XM1_HOST_BUILD_SWAP_MAX", "4G")], "25769803776 4294967296")
    , ("zero-swap", [("XM1_HOST_BUILD_MEMORY_MAX", "8G"), ("XM1_HOST_BUILD_SWAP_MAX", "0")], "8589934592 0")
    , ("maximum-memory", [("XM1_HOST_BUILD_MEMORY_MAX", "32G"), ("XM1_HOST_BUILD_SWAP_MAX", "32G")], "34359738368 34359738368")
    ] $ \(name, overrides, limits) -> do
      writeFile (release name) ""
      invocation <- start name overrides
      code <- waitForProcess (invocationProcess invocation)
      _ <- invocationStderr invocation
      entries <- lines <$> readFile' logFile
      expect (name ++ " enforces RAM and swap for the whole build scope")
        (code == ExitSuccess && ("memory " ++ name ++ " " ++ limits) `elem` entries)
  invalidMemoryCases <- forM
    [ ("invalid-ram", [("XM1_HOST_BUILD_MEMORY_MAX", "33G")])
    , ("zero-ram", [("XM1_HOST_BUILD_MEMORY_MAX", "0")])
    , ("invalid-swap", [("XM1_HOST_BUILD_SWAP_MAX", "33G")])
    , ("negative-swap", [("XM1_HOST_BUILD_SWAP_MAX", "-1")])
    ] $ \(name, overrides) -> do
      writeFile (release name) ""
      invocation <- start name overrides
      code <- waitForProcess (invocationProcess invocation)
      _ <- invocationStderr invocation
      ran <- started name
      expect (name ++ " refuses a build before bazel runs") (code /= ExitSuccess && not ran)

  first <- start "first" []
  firstStarted <- waitFor (started "first")
  second <- start "second" []
  secondStarted <- waitFor (started "second")
  third <- start "third" []
  threadDelay 1_500_000
  thirdEarly <- started "third"
  writeFile (release "first") ""
  firstCode <- waitForProcess (invocationProcess first)
  firstErr <- invocationStderr first
  thirdStarted <- waitFor (started "third")
  secondStillRunning <- maybe (pure False) running =<< startedPid "second"
  writeFile (release "second") ""
  writeFile (release "third") ""
  secondCode <- waitForProcess (invocationProcess second)
  secondErr <- invocationStderr second
  thirdCode <- waitForProcess (invocationProcess third)
  _ <- invocationStderr third
  overlapped <- expect "two builds overlap and a third waits until a lane is released"
    (firstStarted && secondStarted && not thirdEarly && thirdStarted && secondStillRunning)
  passed <- expect "all three builds pass with passed verdicts"
    (firstCode == ExitSuccess && secondCode == ExitSuccess && thirdCode == ExitSuccess
      && "\"verdict\":\"passed\"" `isInfixOf` firstErr && "\"verdict\":\"passed\"" `isInfixOf` secondErr)

  self <- getExecutablePath
  let lockPath = home </> ".cache" </> "xm1-host-build" </> "build.lock"
      laneReady = root </> "lane-ready"
      laneRelease = root </> "lane-release"
  peer <- start "peer" []
  peerStarted <- waitFor (started "peer")

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
  peerAlive <- maybe (pure False) running =<< startedPid "peer"
  released <- expect "killing one holder admits its waiter without stopping the other build"
    (peerStarted && peerAlive && holderStarted && waiterCode == Just ExitSuccess && holderGone && lockKept)
  unless released (putStrLn (show (waiterCode, holderGone, lockKept) ++ "\n" ++ waiterErr))
  writeFile (release "peer") ""
  peerCode <- waitForProcess (invocationProcess peer)
  _ <- invocationStderr peer
  reservedLane <- spawnHolder self root lockPath 1 laneReady laneRelease
  lanePid <- waitForPid laneReady
  laneHeld <- expect "the surviving build exits normally and a lane can be reserved"
    (peerCode == ExitSuccess && lanePid /= Nothing)

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

  let readyA = root </> "holder-a-ready"
      releaseA = root </> "holder-a-release"
      readyB = root </> "holder-b-ready"
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
  diagnosticEarly <- started "diagnostic"
  diagnosticPid <- getPid (invocationProcess diagnostic)
  maybe (pure ()) (signalProcess sigSTOP) diagnosticPid
  diagnosticStopped <- waitFor $ case diagnosticPid of
    Nothing -> pure False
    Just waiterPid -> do
      status <- readFile' ("/proc/" ++ show waiterPid ++ "/stat")
      pure $ case words (drop 1 (dropWhile (/= ')') status)) of
        state : _ -> state `elem` ["T", "t"]
        _ -> False
  unless diagnosticStopped $ do
    maybe (pure ()) (signalProcess sigCONT) diagnosticPid
    ioError (userError "diagnostic waiter did not stop before the holder handoff")
  writeFile releaseA ""
  _ <- waitForProcess (holderProcess holderA)
  holderB <- spawnHolder self root lockPath 0 readyB releaseB
  pidB <- waitForPid readyB
  maybe (pure ()) (signalProcess sigCONT) diagnosticPid
  changedSeen <- case pidB of
    Just owner -> waitFor (fmap (("pid=" ++ owner) `isInfixOf`) (readFile' diagnosticLog))
    Nothing -> pure False
  stillWaiting <- fmap not (started "diagnostic")
  writeFile releaseB ""
  diagnosticCode <- timeout 20_000_000 (waitForProcess (invocationProcess diagnostic))
  diagnosticErr <- invocationStderr diagnostic
  _ <- waitForProcess (holderProcess holderB)
  canonicalRoot <- canonicalizePath root
  let sampled = mapMaybe reportedPid (lines diagnosticErr)
      context = ("cwd=" ++ canonicalRoot) `isInfixOf` diagnosticErr
      reportedOnce = maybe False (\owner -> length (filter (== owner) sampled) == 1) pidA
      changedInOrder = case (pidA, pidB) of
        (Just previousOwner, Just nextOwner) ->
          maybe False id ((<) <$> elemIndex previousOwner sampled <*> elemIndex nextOwner sampled)
            && previousOwner `notElem` drop 1 (dropWhile (/= nextOwner) sampled)
        _ -> False
  holders <- expect "saturated admission reports an owner handoff without stale holder details"
    (firstSeen && reportedOnce && changedSeen && changedInOrder && stillWaiting
      && not diagnosticEarly && diagnosticCode == Just ExitSuccess && context)
  unless holders (putStrLn diagnosticErr)

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
  baseReported <- waitFor $ do
    text <- readFile' baseLog
    pure (("output_base=" ++ holderBase) `isInfixOf` text
      && ("command_log=" ++ (holderBase </> "command.log")) `isInfixOf` text)
  writeFile (release "base-holder") ""
  baseHolderCode <- waitForProcess (invocationProcess baseHolder)
  baseWaiterCode <- waitForProcess (invocationProcess baseWaiter)
  baseErr <- invocationStderr baseWaiter
  baseObserved <- expect "an explicit holder output base is named with its existing command.log"
    (baseHeld && baseReported && baseHolderCode == ExitSuccess && baseWaiterCode == ExitSuccess
      && "\"verdict\":\"passed\"" `isInfixOf` baseErr)

  let unavailableBase testCase = do
        let (name, argumentBase) = case testCase of
              OverBoundBase -> ("over", replicate 600 'x')
              CutBaseArgument -> ("cut", replicate 5000 'y')
              ControlByteBase -> ("control", "/tmp/diagnostic\nbase")
              NonAsciiBase -> ("unicode", "/tmp/diagnostic-\x00e9")
            holderName = name ++ "-holder"
            waiterName = name ++ "-waiter"
            diagnosticPath = root </> (name ++ "-stderr")
        externalHolder <- spawnArgs binary root
          (("FAKE_NAME", holderName) : ("FAKE_RELEASE", release holderName) : environment)
          ["--output_base=" ++ argumentBase, "build", "//fake:target"]
        held <- waitFor (started holderName)
        writeFile (release waiterName) ""
        externalWaiter <- spawnLoggedArgs binary root
          (("FAKE_NAME", waiterName) : ("FAKE_RELEASE", release waiterName) : environment)
          ["build", "//fake:target"] diagnosticPath
        unavailable <- waitFor (fmap ("output_base=unavailable" `isInfixOf`) (readFile' diagnosticPath))
        writeFile (release holderName) ""
        holderCode <- waitForProcess (invocationProcess externalHolder)
        externalWaiterCode <- waitForProcess (invocationProcess externalWaiter)
        diagnosticText <- invocationStderr externalWaiter
        expect (name ++ " output base stays unavailable without changing admission")
          (held && unavailable && holderCode == ExitSuccess && externalWaiterCode == ExitSuccess
            && not ("command_log=" `isInfixOf` diagnosticText))
  boundaries <- mapM unavailableBase [OverBoundBase, CutBaseArgument, ControlByteBase, NonAsciiBase]
  writeFile laneRelease ""
  _ <- waitForProcess (holderProcess reservedLane)
  let legacyReady = root </> "legacy-ready"
      legacyRelease = root </> "legacy-release"
  (_, _, _, legacy) <- createProcess
    (proc self ["--hold-all", lockPath, "0", legacyReady, legacyRelease])
    { cwd = Just root, std_out = NoStream, std_err = NoStream }
  legacyPid <- waitForPid legacyReady
  writeFile (release "legacy-waiter") ""
  legacyWaiter <- start "legacy-waiter" []
  threadDelay 1_500_000
  legacyEarly <- started "legacy-waiter"
  writeFile legacyRelease ""
  legacyCode <- waitForProcess (invocationProcess legacyWaiter)
  _ <- invocationStderr legacyWaiter
  _ <- waitForProcess legacy
  legacyCompatible <- expect "a legacy whole-file lock blocks both build lanes"
    (legacyPid /= Nothing && not legacyEarly && legacyCode == ExitSuccess)
  pure (and ([storageRefused, invalidReserveRefused, overlapped, passed, laneHeld, released, failed, refused, holders, baseObserved, legacyCompatible] ++ boundaries ++ memoryCases ++ invalidMemoryCases))

data UnavailableOutputBase = OverBoundBase | CutBaseArgument | ControlByteBase | NonAsciiBase

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
      dropped = ["HOME", "PATH", "XM1_EMULATOR_SLOT", "XM1_HOST_BUILD", "XM1_STORAGE_MIN_FREE",
        "XM1_HOST_BUILD_MEMORY_MAX", "XM1_HOST_BUILD_SWAP_MAX", "XM1_EMULATOR_MEMORY_MAX", "XM1_EMULATOR_SWAP_MAX",
        "XM1_NATIVE_MEMORY_MAX", "XM1_NATIVE_SWAP_MAX", "XM1_STORAGE_ACCOUNTING_CONFIG", "XM1_STORAGE_ACCOUNTING_STATE", "XM1_STORAGE_ACCOUNTING_TOOL"]
  pure $ ("HOME", home) : ("PATH", path) : ("FAKE_LOG", logFile) : ("XM1_STORAGE_MIN_FREE", "1")
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
