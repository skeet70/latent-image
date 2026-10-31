-- | Behavioral tests through the actual transient-service path. Each case runs
-- a worker as the service MainPID; the worker starts the fixture directly.
module Main (main) where

import Control.Concurrent (forkIO, threadDelay)
import Control.Concurrent.MVar (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, catch, evaluate)
import Control.Monad (unless)
import Data.List (intercalate, stripPrefix)
import Data.Maybe (fromMaybe)
import Data.Unique (hashUnique, newUnique)
import System.Directory
  ( canonicalizePath
  , createDirectoryIfMissing
  , copyFile
  , doesDirectoryExist
  , doesFileExist
  , getTemporaryDirectory
  , listDirectory
  , removeDirectoryRecursive
  , removeFile
  )
import System.Environment (getArgs, getEnvironment, lookupEnv, setEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (ReadMode), hGetContents, withFile)
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (Signal, signalProcess, sigKILL, sigTERM)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), createProcess, proc, readProcessWithExitCode, waitForProcess)
import Text.Read (readMaybe)
import Emulation.ThermalGuard
  ( SysfsRoot (..)
  , discoverThermalSensor
  )

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [worker, fixture, guard, productionWorker] ->
      run worker fixture guard productionWorker >>= \passed -> unless passed exitFailure
    _ -> ioError (userError "usage: thermal-guard-tests WORKER FIXTURE GUARD PRODUCTION_WORKER")

run :: FilePath -> FilePath -> FilePath -> FilePath -> IO Bool
run worker fixture guard productionWorker = do
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
  setEnv "XDG_RUNTIME_DIR" runtime
  setEnv "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus")
  temporary <- getTemporaryDirectory
  base <- fromMaybe temporary <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  workerPath <- canonicalizePath worker
  fixturePath <- canonicalizePath fixture
  let root = base </> ("thermal-guard-tests-" ++ show pid)
      sysfs = root </> "sys"
      hwmon = sysfs </> "class" </> "hwmon" </> "hwmon0"
      sensorFile = hwmon </> "temp1_input"
      pidFile = root </> "fixture.pids"
      limitsFile = root </> "child-limits"
  ignoreMissing (removeDirectoryRecursive root)
  createDirectoryIfMissing True hwmon
  writeFile (hwmon </> "name") "k10temp\n"
  writeFile (hwmon </> "temp1_label") "Tctl\n"
  writeFile sensorFile "50000\n"
  discovery <- discoverThermalSensor (SysfsRoot sysfs)
  discoveryOk <- expect "discovers k10temp Tctl" (either (const False) (const True) discovery)
  changedName <- discoveryFails sysfs (hwmon </> "name") "other\n"
  changedLabel <- discoveryFails sysfs (hwmon </> "temp1_label") "Tctl2\n"
  writeFile (hwmon </> "name") "k10temp\n"
  writeFile (hwmon </> "temp1_label") "Tctl\n"
  missingInput <- do
    removeFile sensorFile
    result <- discoverThermalSensor (SysfsRoot sysfs)
    writeFile sensorFile "50000\n"
    pure (either (const True) (const False) result)
  let service mode file = runService workerPath fixturePath sysfs mode file
  cool <- service "exit7" ""
  coolOk <- expect "cool service child propagates exit 7" (cool == ExitFailure 7)
  writeFile sensorFile "90000\n"
  hot <- service "exit0" ""
  hotOk <- expect "initial hot sensor fails closed" (hot == ExitFailure 75)
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  hotAfter <- startThenChange workerPath fixturePath sysfs "descendant" pidFile (writeFile sensorFile "90000\n")
  hotAfterOk <- expect "later hot service returns 75" (hotAfter == ExitFailure 75)
  hotCleanup <- expect "later hot service cleans parent and other-group child" =<< noProcesses pidFile
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  missingAfter <- startThenChange workerPath fixturePath sysfs "descendant" pidFile (removeFile sensorFile `catchIO` const (pure ()))
  missingAfterOk <- expect "later missing service returns 75" (missingAfter == ExitFailure 75)
  missingCleanup <- expect "later missing service cleans parent and other-group child" =<< noProcesses pidFile
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  normal <- service "orphan" pidFile
  normalOk <- expect "normal orphan service exits 0" (normal == ExitSuccess)
  normalCleanup <- expect "normal orphan service leaves no other-group child" =<< noProcesses pidFile
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  identity <- startThenChange workerPath fixturePath sysfs "descendant" pidFile (writeFile (hwmon </> "name") "other\n")
  writeFile (hwmon </> "name") "k10temp\n"
  identityOk <- expect "changed identity service fails closed" (identity == ExitFailure 75)
  identityCleanup <- expect "changed identity service cleans other-group child" =<< noProcesses pidFile
  writeFile sensorFile "85000\n"
  boundaryHot <- service "exit0" ""
  boundaryHotOk <- expect "85000 exactly fails closed" (boundaryHot == ExitFailure 75)
  writeFile sensorFile "84999\n"
  boundaryCool <- service "exit0" ""
  boundaryCoolOk <- expect "84999 is cool" (boundaryCool == ExitSuccess)
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile limitsFile)
  limits <- service "limits" limitsFile
  limitsOk <- expect "service child exits normally for limits" (limits == ExitSuccess)
  limitsText <- readFile limitsFile
  limitsMatch <- expect "service child sees MemoryMax 8G and swap 0" (limitsText == "8589934592\n0\n")
  ignoreMissing (removeFile limitsFile)
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  interrupted <- interruptThenCheck workerPath fixturePath sysfs pidFile sigTERM
  interruptedOk <- expect "interrupt cleans other-group child and leaves caller alive" interrupted
  writeFile sensorFile "50000\n"
  ignoreMissing (removeFile pidFile)
  killed <- interruptThenCheck workerPath fixturePath sysfs pidFile sigKILL
  killedOk <- expect "guardian SIGKILL cleans other-group child and leaves caller alive" killed
  launcherOk <- publicLauncher root guard productionWorker fixturePath
  ignoreMissing (removeDirectoryRecursive root)
  pure (and [discoveryOk, changedName, changedLabel, missingInput, coolOk, hotOk, hotAfterOk, hotCleanup, missingAfterOk, missingCleanup, normalOk, normalCleanup, identityOk, identityCleanup, boundaryHotOk, boundaryCoolOk, limitsOk, limitsMatch, interruptedOk, killedOk, launcherOk])

newtype Cpu = Cpu Int
  deriving (Eq)

newtype ObservedPid = ObservedPid Int
  deriving (Eq)

newtype ObservedUnit = ObservedUnit String
  deriving (Eq)

publicLauncher :: FilePath -> FilePath -> FilePath -> FilePath -> IO Bool
publicLauncher root guardSource workerSource fixture = do
  let directory = root </> "public"
      guard = directory </> "thermal-guard"
      worker = directory </> "thermal-guard-worker"
  createDirectoryIfMissing True directory
  copyFile guardSource guard
  copyFile workerSource worker
  setFileMode guard 0o700
  setFileMode worker 0o700
  environmentOk <- publicEnvironment directory guard fixture
  available <- statusCpuList "/proc/self/status"
  affinityOk <- case available of
    first : _second : third : fourth : _ ->
      and <$> mapM (publicAffinityCase directory guard fixture)
        [take 4 available, [first], [first, third, fourth]]
    _ -> ioError (userError "public affinity tests require four available caller CPUs")
  pure (environmentOk && affinityOk)

publicEnvironment :: FilePath -> FilePath -> FilePath -> IO Bool
publicEnvironment directory guard fixture = do
  inherited <- getEnvironment
  let output = directory </> "environment"
      supplied =
        [ ("THERMAL_GUARD_KEPT", "kept value")
        , ("BASH_FUNC_thermal_guard_probe%%", "() {  :\n}")
        , ("1THERMAL_GUARD", "digit first")
        ]
      names = map fst supplied
      arguments =
        [ "--user", "--scope", "--quiet"
        , "--", guard, "--", fixture, "environment", output
        ] ++ names
  (_, _, _, process) <- createProcess (proc "systemd-run" arguments)
    { env = Just (supplied ++ filter ((`notElem` names) . fst) inherited) }
  code <- waitForProcess process
  written <- doesFileExist output
  observed <- if written then readFile output else pure ""
  expect "public guard passes representable variables and omits unrepresentable names"
    (code == ExitSuccess && observed == unlines
      [ "THERMAL_GUARD_KEPT=kept value"
      , "BASH_FUNC_thermal_guard_probe%% unset"
      , "1THERMAL_GUARD unset"
      ])

publicAffinityCase :: FilePath -> FilePath -> FilePath -> [Cpu] -> IO Bool
publicAffinityCase directory guard fixture cpus = do
  let mask = intercalate "," [show index | Cpu index <- cpus]
      pidFile = directory </> ("pids-" ++ mask)
      releaseFile = directory </> ("release-" ++ mask)
      arguments =
        [ "--user", "--scope", "--quiet"
        , "--property=MemoryMax=8589934592", "--property=MemorySwapMax=0"
        , "--", "taskset", "-c", mask, guard, "--", fixture
        , "affinity", pidFile, releaseFile
        ]
  (_, _, _, process) <- createProcess (proc "systemd-run" arguments)
  pids@(payload : descendant : _) <- waitForPids pidFile
  worker <- parentPid payload
  descendantParent <- parentPid descendant
  unit@(ObservedUnit name) <- processUnit payload
  mainPid <- serviceMainPid name
  units <- mapM processUnit (worker : pids)
  ancestryOk <- expect ("public guard ancestry " ++ mask)
    (descendantParent == payload && all (== unit) units
      && case worker of ObservedPid value -> mainPid == Just value)
  masks <- mapM threadCpuLists (worker : pids)
  masksOk <- expect ("public guard all-thread affinity " ++ mask)
    (all (not . null) masks && all (== cpus) (concat masks))
  memory <- cgroupFile payload "memory.max"
  swap <- cgroupFile payload "memory.swap.max"
  (modeCode, mode, _) <- readProcessWithExitCode "systemctl"
    ["--user", "show", "-p", "KillMode", "--value", name] ""
  limitsOk <- expect ("public guard active limits " ++ mask)
    (memory == "8589934592\n" && swap == "0\n"
      && modeCode == ExitSuccess && mode == "control-group\n")
  putStrLn ("observed public guard " ++ mask ++ ": " ++ name
    ++ " worker=" ++ showObservedPid worker ++ " payload/descendant="
    ++ intercalate "," (map showObservedPid pids)
    ++ " thread-masks=" ++ show [[renderCpus values | values <- entries] | entries <- masks])
  writeFile releaseFile ""
  code <- waitForProcess process
  exitOk <- expect ("public guard propagates child exit 7 " ++ mask) (code == ExitFailure 7)
  released <- waitForRelease unit (worker : pids) (60 :: Int)
  releaseOk <- expect ("public guard releases service and descendants " ++ mask) released
  pure (and [ancestryOk, masksOk, limitsOk, exitOk, releaseOk])

renderCpus :: [Cpu] -> String
renderCpus cpus = intercalate "," [show index | Cpu index <- cpus]

showObservedPid :: ObservedPid -> String
showObservedPid (ObservedPid pid) = show pid

waitForPids :: FilePath -> IO [ObservedPid]
waitForPids path = go (60 :: Int)
  where
    go 0 = ioError (userError "public affinity fixture did not publish two PIDs")
    go attempts = do
      present <- doesFileExist path
      values <- if present then mapM readMaybe . lines <$> readFile path else pure Nothing
      case values of
        Just [payload, descendant] -> pure [ObservedPid payload, ObservedPid descendant]
        _ -> threadDelay 50_000 >> go (attempts - 1)

parentPid :: ObservedPid -> IO ObservedPid
parentPid pid = do
  text <- readFile ("/proc" </> showObservedPid pid </> "status")
  case [value | line <- lines text, Just raw <- [stripPrefix "PPid:" line], Just value <- [readMaybe raw]] of
    [value] -> pure (ObservedPid value)
    _ -> ioError (userError "fixture parent PID is unavailable")

processUnit :: ObservedPid -> IO ObservedUnit
processUnit pid = do
  text <- readFile ("/proc" </> showObservedPid pid </> "cgroup")
  case [path | line <- lines text, Just path <- [stripPrefix "0::" line]] of
    [path] -> pure (ObservedUnit (reverse (takeWhile (/= '/') (reverse path))))
    _ -> ioError (userError "fixture cgroup is unavailable")

cgroupFile :: ObservedPid -> FilePath -> IO String
cgroupFile pid file = do
  text <- readFile ("/proc" </> showObservedPid pid </> "cgroup")
  case [path | line <- lines text, Just path <- [stripPrefix "0::" line]] of
    [path] -> readFile (("/sys/fs/cgroup" ++ path) </> file)
    _ -> ioError (userError "fixture cgroup is unavailable")

threadCpuLists :: ObservedPid -> IO [[Cpu]]
threadCpuLists pid = do
  let directory = "/proc" </> showObservedPid pid </> "task"
  threads <- listDirectory directory
  mapM (\thread -> statusCpuList (directory </> thread </> "status")) threads

statusCpuList :: FilePath -> IO [Cpu]
statusCpuList path = do
  text <- readFile path
  case [words value | line <- lines text, Just value <- [stripPrefix "Cpus_allowed_list:" line]] of
    [[value]] -> either (ioError . userError) pure (parse value)
    _ -> ioError (userError "observed CPU affinity is unavailable")
  where
    parse value = case break (== ',') value of
      (part, []) -> range part
      (part, _ : rest) -> (++) <$> range part <*> parse rest
    range value = case break (== '-') value of
      (lower, []) -> case readMaybe lower of
        Just index -> Right [Cpu index]
        Nothing -> Left "invalid observed CPU index"
      (lower, _ : upper) -> case (readMaybe lower, readMaybe upper) of
        (Just start, Just end) | start <= end -> Right (map Cpu [start .. end])
        _ -> Left "invalid observed CPU range"

waitForRelease :: ObservedUnit -> [ObservedPid] -> Int -> IO Bool
waitForRelease (ObservedUnit unit) pids = go
  where
    go 0 = pure False
    go attempts = do
      dead <- and <$> mapM (\pid -> not <$> doesDirectoryExist ("/proc" </> showObservedPid pid)) pids
      (code, state, _) <- readProcessWithExitCode "systemctl"
        ["--user", "show", "-p", "LoadState", "--value", unit] ""
      if dead && code == ExitSuccess && state == "not-found\n"
        then pure True
        else threadDelay 50_000 >> go (attempts - 1)

serviceUnit :: IO String
serviceUnit = do
  pid <- getProcessID
  unique <- hashUnique <$> newUnique
  pure ("thermal-guard-test-" ++ show pid ++ "-" ++ show unique ++ ".service")

runService :: FilePath -> FilePath -> FilePath -> String -> FilePath -> IO ExitCode
runService worker fixture sysfs mode file = do
  unit <- serviceUnit
  runUnit unit worker fixture sysfs mode file

runUnit :: String -> FilePath -> FilePath -> FilePath -> String -> FilePath -> IO ExitCode
runUnit unit worker fixture sysfs mode file = do
  let arguments =
        [ "--user", "--wait", "--pipe", "--collect"
        , "--unit=" ++ unit
        , "--property=KillMode=control-group"
        , "--property=TimeoutStopSec=10s"
        , "--property=MemoryMax=8589934592"
        , "--property=MemorySwapMax=0"
        , "--"
        , worker
        , sysfs
        , fixture
        , mode
        , file
        ]
  (_, _, _, process) <- createProcess (proc "systemd-run" arguments)
  waitForProcess process

startThenChange :: FilePath -> FilePath -> FilePath -> String -> FilePath -> IO () -> IO ExitCode
startThenChange worker fixture sysfs mode pidFile change = do
  unit <- serviceUnit
  result <- newEmptyMVar
  _ <- forkIO $ runUnit unit worker fixture sysfs mode pidFile >>= putMVar result
  started <- waitForFile pidFile
  unless started (ioError (userError "the fixture did not start"))
  change
  takeMVar result

interruptThenCheck :: FilePath -> FilePath -> FilePath -> FilePath -> Signal -> IO Bool
interruptThenCheck worker fixture sysfs pidFile signal = do
  unit <- serviceUnit
  result <- newEmptyMVar
  _ <- forkIO $ runUnit unit worker fixture sysfs "descendant" pidFile >>= putMVar result
  started <- waitForFile pidFile
  unless started (ioError (userError "the fixture did not start"))
  mainPid <- serviceMainPid unit
  case mainPid of
    Just pid -> signalProcess signal (fromIntegral pid)
    Nothing -> ioError (userError "service MainPID unavailable")
  _ <- takeMVar result
  noProcesses pidFile

serviceMainPid :: String -> IO (Maybe Int)
serviceMainPid unit = do
  result <- readProcessWithExitCode "systemctl"
    ["--user", "show", "-p", "MainPID", "--value", unit] ""
  pure $ case result of
    (ExitSuccess, value, _) -> readMaybe (takeWhile (/= '\n') value)
    _ -> Nothing

waitForFile :: FilePath -> IO Bool
waitForFile path = go (60 :: Int)
  where
    go 0 = pure False
    go attempts = do
      present <- doesFileExist path
      if present then pure True else threadDelay 50_000 >> go (attempts - 1)

noProcesses :: FilePath -> IO Bool
noProcesses path = do
  present <- doesFileExist path
  if not present then pure False else do
    text <- readFile path
    case mapM (readMaybe . filter isDigit) (lines text) of
      Just pids -> and <$> mapM alive (pids :: [Int])
      Nothing -> pure False
  where
    alive pid = not <$> doesDirectoryExist ("/proc" </> show pid)

isDigit :: Char -> Bool
isDigit character = character >= '0' && character <= '9'

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

discoveryFails :: FilePath -> FilePath -> String -> IO Bool
discoveryFails sysfs path contents = do
  original <- withFile path ReadMode $ \handle -> do
    text <- hGetContents handle
    _ <- evaluate (length text)
    pure text
  writeFile path contents
  result <- discoverThermalSensor (SysfsRoot sysfs)
  writeFile path original
  pure (either (const True) (const False) result)

catchIO :: IO a -> (IOException -> IO a) -> IO a
catchIO = catch

ignoreMissing :: IO () -> IO ()
ignoreMissing action = action `catchIO` const (pure ())
