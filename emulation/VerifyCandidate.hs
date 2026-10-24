{-# LANGUAGE OverloadedStrings #-}
-- | @verify-candidate@ runs every recorded emulator route and native case
-- suite through its existing command, compares each route's observations
-- with its baseline under @private-data/verify-candidate/baselines/@ and
-- writes one report. Emulator routes run at most one per emulator slot,
-- longest first; the native suites run beside them. No camera or USB I/O.
module Main (main) where

import Control.Concurrent (forkIO)
import Control.Concurrent.MVar (MVar, modifyMVar, modifyMVar_, newEmptyMVar, newMVar, putMVar, readMVar, takeMVar)
import Control.Exception (SomeException, finally, try)
import Control.Monad (forM, forM_, replicateM, unless, when)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate, isPrefixOf)
import qualified Data.Map.Strict as Map
import Data.Maybe (fromMaybe, isJust)
import qualified Data.Text as T
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Emulation.CandidateBaseline
import Emulation.EmulatorSlot (SlotCount (..), SlotPolicy (..), slotPolicyFromEnvironment)
import GHC.Clock (getMonotonicTime)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, pathIsSymbolicLink, removeDirectoryRecursive)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitSuccess)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), IOMode (WriteMode), hFlush, hPutStrLn, hSetBuffering, stderr, stdout, withFile)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), StdStream (..), createProcess, proc, readProcessWithExitCode, waitForProcess)
import Text.Read (readMaybe)

data Route
  = FlashBootCheck
  | RamPushVolatile
  | RamPushClassicChrome
  | RamPushClassicChromeIcon
  | FlashCandidateIcon
  | FlashCandidateQTile
  | UsbToggleE8
  | UpdaterRc0Toggle
  | UpdaterCcToggle
  | NativeVolatile
  | NativeSd
  | FujiModFlashBootCheck
  deriving (Eq, Ord, Enum, Bounded, Show)

routeName :: Route -> String
routeName route = case route of
  FlashBootCheck -> "flash-boot-check"
  RamPushVolatile -> "ram-push-volatile"
  RamPushClassicChrome -> "ram-push-cc"
  RamPushClassicChromeIcon -> "ram-push-cc-icon"
  FlashCandidateIcon -> "flash-candidate-icon-b"
  FlashCandidateQTile -> "flash-candidate-q-tile-b"
  UsbToggleE8 -> "usb-toggle-e8"
  UpdaterRc0Toggle -> "updater-f7-rc0-toggle"
  UpdaterCcToggle -> "updater-f7-cc-toggle"
  NativeVolatile -> "native-volatile"
  NativeSd -> "native-sd"
  FujiModFlashBootCheck -> "fujimod-flash-boot-check"

-- | A non-gating route is reported but does not decide the run's result.
routeGating :: Route -> Bool
routeGating route = route /= FujiModFlashBootCheck

parseRoute :: String -> Maybe Route
parseRoute name = lookup name [(routeName route, route) | route <- [minBound .. maxBound]]

data Host = EmulatorSlot | NativeSuite deriving (Eq)

routeHost :: Route -> Host
routeHost route
  | route `elem` [NativeVolatile, NativeSd] = NativeSuite
  | otherwise = EmulatorSlot

-- | Emulator routes in dispatch order, longest recorded run first.
emulatorOrder :: [Route]
emulatorOrder =
  [ RamPushVolatile, RamPushClassicChromeIcon, RamPushClassicChrome, UsbToggleE8
  , FlashCandidateIcon, FlashCandidateQTile, UpdaterRc0Toggle, UpdaterCcToggle, FlashBootCheck, FujiModFlashBootCheck ]

newtype Workspace = Workspace FilePath
newtype OutputBase = OutputBase FilePath
newtype RunDirectory = RunDirectory FilePath

data Context = Context
  { contextWorkspace :: Workspace
  , contextOutputBase :: OutputBase
  , contextRunfiles :: FilePath
  , contextRun :: RunDirectory
  , contextNativeJobs :: Int
  , contextBazelOptions :: [String]
  }

data Invocation = Invocation
  { invocationProgram :: FilePath
  , invocationArguments :: [String]
  , invocationEnvironment :: [(String, String)]
  }

-- | Where a route stages its emulator runtime. Attach checkpoints restore only
-- into the runtime path that saved them, so each route keeps one path.
routeRuntime :: Workspace -> Route -> FilePath
routeRuntime (Workspace root) route = root </> "private-data" </> "emulator-runtimes" </> "verify-candidate" </> routeName route

-- | Routes run by @xm1-viewer --ram-push-e2e@; their runtime is cleared before each run.
viewerRoutes :: [Route]
viewerRoutes = [RamPushVolatile, RamPushClassicChrome, RamPushClassicChromeIcon, FlashCandidateIcon, FlashCandidateQTile, UsbToggleE8]

prefixDat, stockDat, iconFlashDat, rc0ToggleDat, storedF7One, storedF7Zero, classicChromeCard :: FilePath
prefixDat = "private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT"
stockDat = "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
iconFlashDat = "private-data/analysis/classic-chrome-icon/build-b-1-pinned/FWUP0002.DAT"
rc0ToggleDat = "private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT"
storedF7One = "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
storedF7Zero = "private-data/analysis/usb-toggle/emu/inputs/f7-0.nand"
classicChromeCard = "private-data/analysis/classic-chrome-patch/ram-push/inputs/card.img"

routeInvocation :: Context -> Route -> FilePath -> Invocation
routeInvocation context route receipt = case route of
  FlashBootCheck -> bazelRun "//emulation:flash-boot-check" ["--firmware", stockDat, "--output", receipt]
  FujiModFlashBootCheck -> bazelRun "//emulation:fujimod-flash-boot-check" ["--firmware", stockDat, "--output", receipt]
  RamPushVolatile -> viewer
    ["--cardless", "--firmware", prefixDat, "--installed-nand", storedF7One
    , "--trajectories", "private-data/analysis/x-m1-1.50/ram-push/VolatileBank/scenario.json"]
  RamPushClassicChrome -> viewer (["--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  RamPushClassicChromeIcon -> viewer (["--classic-chrome-icon", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  FlashCandidateIcon -> viewer (["--flash-candidate", "--firmware", iconFlashDat, "--installed-nand", storedF7One] ++ card
    ++ ["--trajectories", "emulation/classic-chrome-icon-trajectories.json"])
  FlashCandidateQTile -> viewer (["--flash-candidate", "--firmware", iconFlashDat, "--installed-nand", storedF7One] ++ card
    ++ ["--trajectories", "emulation/classic-chrome-q-tile-trajectories.json"])
  UsbToggleE8 -> viewer (["--classic-chrome", "--attach-hold-q", "--firmware", rc0ToggleDat, "--installed-nand", storedF7Zero] ++ card)
  UpdaterRc0Toggle -> updater "restore-rc0-toggle"
  UpdaterCcToggle -> updater "restore-cc-toggle"
  NativeVolatile -> native "volatile"
  NativeSd -> native "sd"
  where
    Workspace root = contextWorkspace context
    OutputBase base = contextOutputBase context
    RunDirectory run = contextRun context
    card = ["--sd-card-image", classicChromeCard]
    bazelRun target arguments = Invocation "bazel"
      (["--output_base=" ++ base, "run"] ++ contextBazelOptions context ++ [target, "--"] ++ arguments) []
    viewer arguments = bazelRun "//emulation:xm1-viewer"
      (["--ram-push-e2e"] ++ arguments ++ ["--output", routeRuntime (contextWorkspace context) route, "--receipt-output", receipt])
    updater restore =
      let card' = "private-data/analysis/usb-toggle/emu-updater" </> restore
       in bazelRun "//emulation:updater-f7"
            [ "--recovery-ram", card' </> "terminal-valid-ram.bin", "--sd-image", card' </> "firmware-ram-recovery-sd.img"
            , "--installed-nand", storedF7One, "--receipt-output", receipt ]
    native mode = Invocation "bash" [root </> "emulation" </> "build-recipe-bank-cases.sh"]
      [ ("RB_MODE", mode), ("JOBS", show (contextNativeJobs context))
      , ("BUILD_DIR", run </> "builds" </> routeName route), ("OUT_DIR", receipt)
      , ("RUNNER", tool "native-runner"), ("GEN", tool "recipe-bank-cases"), ("INTEROP", tool "recipe-bank-interop") ]
    tool name = contextRunfiles context </> "_main" </> "emulation" </> name

exclude :: String -> T.Text -> ExclusionReason -> Exclusion
exclude file pointer = Exclusion (filePattern file) (pointerPattern pointer)

-- | The checkpoint record every RAM-push and flash-candidate receipt carries.
emulatorStart :: String -> T.Text -> [Exclusion]
emulatorStart file prefix =
  [ exclude file (prefix <> "/boot_seconds") Timing
  , exclude file (prefix <> "/key_seconds") Timing
  , exclude file (prefix <> "/save_seconds") Timing
  , exclude file (prefix <> "/restore_seconds") Timing
  , exclude file (prefix <> "/directory") CheckpointState
  , exclude file (prefix <> "/key") CheckpointState
  , exclude file (prefix <> "/saved") CheckpointState
  , exclude file (prefix <> "/start") CheckpointState
  , exclude file (prefix <> "/inputs/emulator_source") CandidateIdentity ]

-- | Fields of an @e2e-result.json@ that every RAM-push and flash-candidate run writes.
e2eResult :: String -> [Exclusion]
e2eResult file = emulatorStart file "/emulator_start" ++
  [ exclude file "/phases/*/seconds" Timing
  , exclude file "/observations/*/steps/*/seconds" Timing
  , exclude file "/menu_receipts/*/frame_number" Scheduling
  , exclude file "/menu_receipts/*/path" RunLocation ]

ramPushSpec :: ObservationSpec
ramPushSpec = ObservationSpec
  (map filePattern
    [ "e2e-result.json", "e2e-fetch-watch.json", "cold-restart/e2e-cold-restart.json"
    , "relinked/runtime.bin", "relinked/ram-push-manifest.json", "relinked/bank-allowlist.json", "relinked/kernel.syms"
    , "alloc/ram-alloc-state.json", "push/ram-push-state.json", "unload/ram-unload-state.json", "screenshots/*.png" ])
  (e2eResult "e2e-result.json" ++ emulatorStart "cold-restart/e2e-cold-restart.json" "/emulator_start" ++
    [ exclude "e2e-result.json" "/states" RunLocation
    , exclude "e2e-result.json" "/runtime/manifest" RunLocation
    , exclude "e2e-result.json" "/card/image" RunLocation ])

flashCandidateSpec :: ObservationSpec
flashCandidateSpec = ObservationSpec
  (map filePattern ["e2e-segments.json", "segment-*/e2e-result.json", "segment-*/screenshots/*.png"])
  (exclude "e2e-segments.json" "/*/seconds" Timing : e2eResult "segment-*/e2e-result.json")

updaterSpec :: ObservationSpec
updaterSpec = ObservationSpec
  [filePattern "receipt.json"]
  [ exclude "receipt.json" "/exported_nand/path" RunLocation
  , exclude "receipt.json" "/inputs/*/path" RunLocation ]

flashBootSpec :: ObservationSpec
flashBootSpec = ObservationSpec
  [filePattern "flash-boot-receipt.json"]
  [ exclude "flash-boot-receipt.json" "/emulator_source" CandidateIdentity
  , exclude "flash-boot-receipt.json" "/firmware/path" RunLocation
  , exclude "flash-boot-receipt.json" "/frame_png" RunLocation ]

fujimodFlashBootSpec :: ObservationSpec
fujimodFlashBootSpec = ObservationSpec
  [filePattern "fujimod-flash-boot-receipt.json"]
  [ exclude "fujimod-flash-boot-receipt.json" "/drive_wall_seconds" Timing
  , exclude "fujimod-flash-boot-receipt.json" "/emulator_source" CandidateIdentity
  , exclude "fujimod-flash-boot-receipt.json" "/firmware/path" RunLocation
  , exclude "fujimod-flash-boot-receipt.json" "/frame_png" RunLocation ]

nativeSpec :: ObservationSpec
nativeSpec = ObservationSpec (map filePattern ["results.json", "findings.txt"]) []

routeSpec :: Route -> ObservationSpec
routeSpec route = case route of
  FlashBootCheck -> flashBootSpec
  FujiModFlashBootCheck -> fujimodFlashBootSpec
  RamPushVolatile -> ramPushSpec
  RamPushClassicChrome -> ramPushSpec
  RamPushClassicChromeIcon -> ramPushSpec
  UsbToggleE8 -> ramPushSpec
  FlashCandidateIcon -> flashCandidateSpec
  FlashCandidateQTile -> flashCandidateSpec
  UpdaterRc0Toggle -> updaterSpec
  UpdaterCcToggle -> updaterSpec
  NativeVolatile -> nativeSpec
  NativeSd -> nativeSpec

data Options = Options
  { optionRoutes :: Maybe [Route]
  , optionOutput :: Maybe FilePath
  , optionBaselines :: Maybe FilePath
  , optionFromRun :: Maybe FilePath
  , optionReceipts :: [(Route, FilePath)]
  , optionRecord :: Bool
  , optionNativeJobs :: Int
  , optionBazel :: [String]
  }

usage :: String
usage = unlines
  [ "verify-candidate [--routes NAME,...] [--output DIR] [--baselines DIR] [--native-jobs N]"
  , "                 [--from-run DIR] [--receipt NAME=DIR]... [--record-baseline] [--bazel-option OPTION]..."
  , ""
  , "Runs each selected route through its existing command, compares its observations with"
  , "BASELINES/NAME.json and writes OUTPUT/report.json. Emulator routes hold emulator slots."
  , ""
  , "  --routes NAME,...    routes to verify (default: all)"
  , "  --output DIR         fresh run directory beneath private-data/"
  , "                       (default: private-data/verify-candidate/runs/UTC-STAMP)"
  , "  --baselines DIR      baseline directory (default: private-data/verify-candidate/baselines)"
  , "  --from-run DIR       reuse DIR/receipts/NAME of an earlier run instead of running routes"
  , "  --receipt NAME=DIR   reuse DIR as route NAME's receipt instead of running it"
  , "  --record-baseline    write BASELINES/NAME.json from the receipts instead of comparing"
  , "  --native-jobs N      JOBS for the native case suites (default: 4)"
  , "  --bazel-option OPT   option passed to each route's bazel run"
  , ""
  , "Routes: " ++ intercalate ", " (map routeName [minBound .. maxBound :: Route])
  ]

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing [] False 4 [])
  where
    go options [] = pure options
    go options ("--routes" : value : rest) = do
      routes <- mapM route (splitOn ',' value)
      go options {optionRoutes = Just routes} rest
    go options ("--output" : value : rest) = go options {optionOutput = Just value} rest
    go options ("--baselines" : value : rest) = go options {optionBaselines = Just value} rest
    go options ("--from-run" : value : rest) = go options {optionFromRun = Just value} rest
    go options ("--receipt" : value : rest) = case break (== '=') value of
      (name, '=' : directory@(_ : _)) -> do
        selected <- route name
        go options {optionReceipts = optionReceipts options ++ [(selected, directory)]} rest
      _ -> failure ("--receipt takes NAME=DIR, not " ++ value)
    go options ("--record-baseline" : rest) = go options {optionRecord = True} rest
    go options ("--native-jobs" : value : rest) = case readMaybe value of
      Just jobs | jobs >= 1 && jobs <= 64 -> go options {optionNativeJobs = jobs} rest
      _ -> failure "--native-jobs takes an integer 1..64"
    go options ("--bazel-option" : value : rest) = go options {optionBazel = optionBazel options ++ [value]} rest
    go _ (argument : _) = failure ("unknown option or missing value: " ++ argument)
    route name = maybe (failure ("unknown route: " ++ name)) pure (parseRoute name)
    failure message = ioError (userError (message ++ "\n" ++ usage))

splitOn :: Char -> String -> [String]
splitOn separator text = case break (== separator) text of
  (front, []) -> [front]
  (front, _ : rest) -> front : splitOn separator rest

require :: Bool -> String -> IO ()
require condition message = unless condition (ioError (userError message))

workspaceRoot :: IO Workspace
workspaceRoot = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  Workspace <$> canonicalizePath root

resolve :: Workspace -> FilePath -> FilePath
resolve (Workspace root) path = if "/" `isPrefixOf` path then path else root </> path

beneathPrivate :: Workspace -> FilePath -> String -> IO FilePath
beneathPrivate root path name = do
  private <- canonicalizePath (resolve root "private-data")
  let target = resolve root path
  createDirectoryIfMissing True (takeDirectory target)
  parent <- canonicalizePath (takeDirectory target)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) (name ++ " must be beneath workspace/private-data/")
  require (takeFileName target `notElem` ["", ".", ".."]) (name ++ " must name a directory")
  pure (parent </> takeFileName target)

-- | The output base of the @bazel run@ that started this process.
outputBase :: FilePath -> IO OutputBase
outputBase executable = do
  real <- canonicalizePath executable
  let parts = splitDirectories real
      base = takeWhile (/= "execroot") parts
  require (length base < length parts) ("verify-candidate must run under bazel run; executable is " ++ real)
  pure (OutputBase (foldr1 (</>) base))

data Source = Execute | Existing deriving (Eq)

data Execution = Execution
  { executionExit :: ExitCode
  , executionSeconds :: Double
  , executionLog :: FilePath
  }

stamp :: IO String
stamp = formatTime defaultTimeLocale "%Y%m%dT%H%M%SZ" <$> getCurrentTime

say :: MVar () -> String -> IO ()
say lock message = modifyMVar_ lock $ \() -> do
  now <- formatTime defaultTimeLocale "%H:%M:%S" <$> getCurrentTime
  putStrLn (now ++ " " ++ message)
  hFlush stdout

execute :: Context -> MVar () -> Route -> FilePath -> IO Execution
execute context lock route receipt = do
  let RunDirectory run = contextRun context
      Workspace root = contextWorkspace context
      logPath = run </> "logs" </> (routeName route ++ ".log")
      invocation = routeInvocation context route receipt
  when (route `elem` viewerRoutes) $ do
    let runtime = routeRuntime (contextWorkspace context) route
    present <- doesDirectoryExist runtime
    when present $ do
      linked <- pathIsSymbolicLink runtime
      require (not linked) ("the route runtime must not be a symlink: " ++ runtime)
      removeDirectoryRecursive runtime
    createDirectoryIfMissing True (takeDirectory runtime)
  inherited <- getEnvironment
  let scrubbed = filter ((`notElem` ["BUILD_WORKSPACE_DIRECTORY", "BUILD_WORKING_DIRECTORY", "RUNFILES_DIR", "RUNFILES_MANIFEST_FILE", "JAVA_RUNFILES"]) . fst) inherited
      environment = invocationEnvironment invocation ++ filter ((`notElem` map fst (invocationEnvironment invocation)) . fst) scrubbed
  say lock ("start " ++ routeName route)
  started <- getMonotonicTime
  outcome <- try $ withFile logPath WriteMode $ \handle -> do
    hPutStrLn handle (unwords (invocationProgram invocation : invocationArguments invocation))
    hFlush handle
    (_, _, _, process) <- createProcess (proc (invocationProgram invocation) (invocationArguments invocation))
      { cwd = Just root, env = Just environment, std_in = NoStream, std_out = UseHandle handle, std_err = UseHandle handle }
    waitForProcess process
  ended <- getMonotonicTime
  let code = either (\problem -> ExitFailure (const 127 (problem :: SomeException))) id outcome
  say lock ("end " ++ routeName route ++ ": " ++ showExit code ++ " after " ++ showSeconds (ended - started))
  pure (Execution code (ended - started) logPath)

showExit :: ExitCode -> String
showExit ExitSuccess = "exit 0"
showExit (ExitFailure code) = "exit " ++ show code

showSeconds :: Double -> String
showSeconds value = show (fromIntegral (round (value * 10) :: Integer) / 10 :: Double) ++ " s"

-- | Emulator routes on @slots@ workers in 'emulatorOrder'; native suites on one worker.
schedule :: Context -> Int -> [(Route, FilePath)] -> IO (Map.Map Route Execution)
schedule context slots jobs = do
  lock <- newMVar ()
  results <- newMVar Map.empty
  let emulator = [(route, receipt) | route <- emulatorOrder, Just receipt <- [lookup route jobs]]
      native = [(route, receipt) | (route, receipt) <- jobs, routeHost route == NativeSuite]
  queue <- newMVar emulator
  let take' = modifyMVar queue (\pending -> pure (drop 1 pending, take 1 pending))
      record route execution = modifyMVar_ results (pure . Map.insert route execution)
      emulatorWorker = take' >>= \next -> case next of
        [] -> pure ()
        (route, receipt) : _ -> execute context lock route receipt >>= record route >> emulatorWorker
      nativeWorker = forM_ native $ \(route, receipt) -> execute context lock route receipt >>= record route
      fork action = do
        done <- newEmptyMVar
        _ <- forkIO (action `finally` putMVar done ())
        pure done
  workers <- (:) <$> fork nativeWorker <*> replicateM (max 1 (min slots (length emulator))) (fork emulatorWorker)
  mapM_ takeMVar workers
  readMVar results

data Outcome = Outcome
  { outcomeRoute :: Route
  , outcomeReceipt :: FilePath
  , outcomeExecution :: Maybe Execution
  , outcomeResult :: T.Text
  , outcomeProblem :: Maybe String
  , outcomeDifferences :: [Difference]
  , outcomeBaseline :: FilePath
  }

differenceJson :: Difference -> Value
differenceJson difference =
  let (expected, actual) = differenceValue difference
      kind = case difference of
        Changed {} -> "changed" :: T.Text
        AbsentFromRun {} -> "absent-from-run"
        AbsentFromBaseline {} -> "absent-from-baseline"
   in object ["field" .= renderFieldName (differenceField difference), "kind" .= kind, "baseline" .= expected, "run" .= actual]

outcomeJson :: Outcome -> Value
outcomeJson outcome = object
  ([ "route" .= routeName (outcomeRoute outcome)
   , "result" .= outcomeResult outcome
   , "gating" .= routeGating (outcomeRoute outcome)
   , "receipt" .= outcomeReceipt outcome
   , "baseline" .= outcomeBaseline outcome
   , "problem" .= outcomeProblem outcome
   , "differences" .= length (outcomeDifferences outcome)
   , "first_difference" .= fmap differenceJson (case outcomeDifferences outcome of first : _ -> Just first; [] -> Nothing)
   ] ++ maybe [] (\execution ->
     [ "exit_code" .= (case executionExit execution of ExitSuccess -> 0; ExitFailure code -> code)
     , "seconds" .= executionSeconds execution
     , "log" .= executionLog execution ]) (outcomeExecution outcome))

judge :: Bool -> String -> Route -> FilePath -> Maybe Execution -> IO Outcome
judge record baselines route receipt execution = do
  let baselinePath = baselines </> (routeName route ++ ".json")
      outcome = Outcome route receipt execution
      failedRun = case executionExit <$> execution of
        Just (ExitFailure code) -> Just ("route exited " ++ show code)
        _ -> Nothing
  present <- doesDirectoryExist receipt
  if not present then pure (outcome "fail" (Just "receipt directory is missing") [] baselinePath) else do
    observed <- observe (routeSpec route) (ReceiptDirectory receipt)
    if record
      then case failedRun of
        Just problem -> pure (outcome "fail" (Just (problem ++ "; baseline not recorded")) [] baselinePath)
        Nothing | null observed -> pure (outcome "fail" (Just "no observed files; baseline not recorded") [] baselinePath)
                | otherwise -> do
          now <- formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" <$> getCurrentTime
          createDirectoryIfMissing True baselines
          setFileMode baselines 0o700
          B.writeFile baselinePath (encodeBaseline (Baseline (T.pack (routeName route)) (T.pack now) receipt observed))
          setFileMode baselinePath 0o600
          pure (outcome "recorded" Nothing [] baselinePath)
      else do
        exists <- doesFileExist baselinePath
        if not exists then pure (outcome "fail" (Just "no baseline") [] baselinePath) else do
          parsed <- decodeBaseline <$> B.readFile baselinePath
          case parsed of
            Left problem -> pure (outcome "fail" (Just ("unreadable baseline: " ++ problem)) [] baselinePath)
            Right baseline -> do
              let differences = compareObservations (baselineFields baseline) observed
                  passed = null differences && not (isJust failedRun)
              pure (outcome (if passed then "pass" else "fail") failedRun differences baselinePath)

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  when (arguments == ["--help"]) (putStr usage >> exitSuccess)
  options <- parseOptions arguments
  root <- workspaceRoot
  let selected = fromMaybe [minBound .. maxBound] (optionRoutes options)
  fromRun <- traverse (canonicalizePath . resolve root) (optionFromRun options)
  receipts <- forM (optionReceipts options) $ \(route, directory) -> (,) route <$> canonicalizePath (resolve root directory)
  let sourceOf route = case (lookup route receipts, fromRun) of
        (Just directory, _) -> (Existing, Just directory)
        (Nothing, Just run) -> (Existing, Just (run </> "receipts" </> routeName route))
        (Nothing, Nothing) -> (Execute, Nothing)
  started <- stamp
  run <- beneathPrivate root (fromMaybe ("private-data/verify-candidate/runs" </> started) (optionOutput options)) "--output"
  createDirectory run
  setFileMode run 0o700
  baselines <- beneathPrivate root (fromMaybe "private-data/verify-candidate/baselines" (optionBaselines options)) "--baselines"
  let toExecute = [route | route <- selected, fst (sourceOf route) == Execute]
      receiptOf route = fromMaybe (run </> "receipts" </> routeName route) (snd (sourceOf route))
  clockStart <- getMonotonicTime
  executions <- if null toExecute then pure Map.empty else do
    forM_ ["receipts", "logs", "builds"] $ \directory -> createDirectory (run </> directory)
    executable <- getExecutablePath
    base <- outputBase executable
    policy <- slotPolicyFromEnvironment
    let SlotCount slots = slotCount policy
        context = Context root base (executable ++ ".runfiles") (RunDirectory run) (optionNativeJobs options) (optionBazel options)
    putStrLn ("verify-candidate run: " ++ run)
    schedule context slots [(route, receiptOf route) | route <- toExecute]
  clockEnd <- getMonotonicTime
  outcomes <- forM selected $ \route -> judge (optionRecord options) baselines route (receiptOf route) (Map.lookup route executions)
  commit <- readProcessWithExitCode "git" ["-C", let Workspace path = root in path, "rev-parse", "HEAD"] ""
  forM_ outcomes $ \outcome -> unless (null (outcomeDifferences outcome)) $ do
    createDirectoryIfMissing True (run </> "differences")
    BL.writeFile (run </> "differences" </> (routeName (outcomeRoute outcome) ++ ".json"))
      (encode (map differenceJson (outcomeDifferences outcome)) <> "\n")
  let reportPath = run </> "report.json"
      passed = all ((`elem` ["pass", "recorded"]) . outcomeResult) (filter (routeGating . outcomeRoute) outcomes)
      Workspace rootPath = root
  BL.writeFile reportPath (encode (object
    [ "schema" .= (1 :: Int)
    , "scope" .= ("host emulation and native case suites; no camera or USB device" :: T.Text)
    , "workspace" .= rootPath
    , "commit" .= (case commit of (ExitSuccess, out, _) -> Just (takeWhile (/= '\n') out); _ -> Nothing)
    , "started_at" .= started
    , "mode" .= (if optionRecord options then "record" else "compare" :: T.Text)
    , "wall_seconds" .= (clockEnd - clockStart)
    , "result" .= (if passed then "pass" else "fail" :: T.Text)
    , "routes" .= map outcomeJson outcomes ]) <> "\n")
  forM_ outcomes $ \outcome -> putStrLn (routeName (outcomeRoute outcome) ++ ": " ++ T.unpack (outcomeResult outcome)
    ++ (if routeGating (outcomeRoute outcome) then "" else " [non-gating]")
    ++ maybe "" (\problem -> " (" ++ problem ++ ")") (outcomeProblem outcome)
    ++ case outcomeDifferences outcome of
         [] -> ""
         first : _ -> "; " ++ show (length (outcomeDifferences outcome)) ++ " differences, first "
           ++ T.unpack (renderFieldName (differenceField first)))
  putStrLn ("wall time: " ++ showSeconds (clockEnd - clockStart))
  putStrLn ("report: " ++ reportPath)
  unless passed (hPutStrLn stderr "verification failed" >> exitFailure)
