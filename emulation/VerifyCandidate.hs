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
import Control.Monad (filterM, forM, forM_, replicateM, unless, when)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, withObject, (.:), (.:?), (.=))
import Data.Aeson.Types (Pair, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate, isPrefixOf)
import qualified Data.Map.Strict as Map
import Data.Maybe (catMaybes, fromMaybe, isJust)
import qualified Data.Text as T
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Emulation.CandidateBaseline
import Emulation.EmulatorSlot (SlotCount (..), SlotPolicy (..), slotPolicyFromEnvironment)
import GHC.Clock (getMonotonicTime)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, listDirectory, removeDirectoryRecursive)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitSuccess)
import System.FilePath (dropExtension, splitDirectories, takeDirectory, takeFileName, (</>))
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
  | FlashCandidateRefresh
  | UsbToggleE8
  | UpdaterRc0Toggle
  | UpdaterCcToggle
  | NativeVolatile
  | NativeSd
  | FujiModFlashBootCheck
  | RamPushCcCardless
  | RamPushCcIconCardless
  | UsbToggleEmu
  | FujiModRamPushVolatile
  | FujiModRamPushCcCardless
  | FujiModRamPushCcIconCardless
  | FujiModRamPushCc
  | FujiModUsbToggleE8
  | FujiModUsbToggleEmu
  deriving (Eq, Ord, Enum, Bounded, Show)

routeName :: Route -> String
routeName route = case route of
  FlashBootCheck -> "flash-boot-check"
  RamPushVolatile -> "ram-push-volatile"
  RamPushClassicChrome -> "ram-push-cc"
  RamPushClassicChromeIcon -> "ram-push-cc-icon"
  FlashCandidateIcon -> "flash-candidate-icon-b"
  FlashCandidateQTile -> "flash-candidate-q-tile-b"
  FlashCandidateRefresh -> "flash-candidate-refresh"
  UsbToggleE8 -> "usb-toggle-e8"
  UpdaterRc0Toggle -> "updater-f7-rc0-toggle"
  UpdaterCcToggle -> "updater-f7-cc-toggle"
  NativeVolatile -> "native-volatile"
  NativeSd -> "native-sd"
  FujiModFlashBootCheck -> "fujimod-flash-boot-check"
  RamPushCcCardless -> "ram-push-cc-cardless"
  RamPushCcIconCardless -> "ram-push-cc-icon-cardless"
  UsbToggleEmu -> "usb-toggle-emu"
  FujiModRamPushVolatile -> "fujimod-ram-push-volatile"
  FujiModRamPushCcCardless -> "fujimod-ram-push-cc-cardless"
  FujiModRamPushCcIconCardless -> "fujimod-ram-push-cc-icon-cardless"
  FujiModRamPushCc -> "fujimod-ram-push-cc"
  FujiModUsbToggleE8 -> "fujimod-usb-toggle-e8"
  FujiModUsbToggleEmu -> "fujimod-usb-toggle-emu"

-- | The recorded routes of E-MERGE-EMULATOR-REVERIFY-150 decide the run's
-- result; the FujiMod routes and their retained counterparts are reported only.
routeGating :: Route -> Bool
routeGating route = route `elem`
  [ FlashBootCheck, RamPushVolatile, RamPushClassicChrome, RamPushClassicChromeIcon, FlashCandidateIcon
  , FlashCandidateQTile, FlashCandidateRefresh, UsbToggleE8, UpdaterRc0Toggle, UpdaterCcToggle, NativeVolatile, NativeSd ]

-- | How a route's outcome is judged.
data Check
  = AgainstBaseline ObservationSpec
  -- | @e2e-observation-compare@ of the counterpart's receipt and this one,
  -- passing with no difference; the flag requires this route to exit 0.
  | AgainstCounterpart Route Bool
  -- | Run only as a counterpart's reference; not judged.
  | Reference

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
  [ UsbToggleEmu, RamPushVolatile, RamPushClassicChromeIcon, RamPushClassicChrome, UsbToggleE8
  , FlashCandidateIcon, FlashCandidateQTile, FlashCandidateRefresh, UpdaterRc0Toggle, UpdaterCcToggle, RamPushCcIconCardless, RamPushCcCardless
  , FujiModUsbToggleEmu, FlashBootCheck, FujiModRamPushVolatile, FujiModRamPushCcIconCardless, FujiModRamPushCcCardless
  , FujiModRamPushCc, FujiModUsbToggleE8, FujiModFlashBootCheck ]

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
  -- | The @NAME.fujimod.json@ trajectory variants present in the workspace.
  , contextVariants :: [FilePath]
  }

data Invocation = Invocation
  { invocationProgram :: FilePath
  , invocationArguments :: [String]
  , invocationEnvironment :: [(String, String)]
  }

prefixDat, stockDat, iconFlashDat, refreshFlashDat, rc0ToggleDat, storedF7One, storedF7Zero, classicChromeCard, refreshCard :: FilePath
prefixDat = "private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT"
stockDat = "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
iconFlashDat = "private-data/analysis/classic-chrome-icon/build-b-1-pinned/FWUP0002.DAT"
refreshFlashDat = "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-20261024b/recipe-complete.DAT"
rc0ToggleDat = "private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT"
storedF7One = "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
storedF7Zero = "private-data/analysis/usb-toggle/emu/inputs/f7-0.nand"
classicChromeCard = "private-data/analysis/classic-chrome-patch/ram-push/inputs/card.img"
refreshCard = "private-data/analysis/x-m1-1.50/recipes/bank-selection-stage4/fast-ui/card-sel.img"

volatileScenario, classicChromeTrajectories, classicChromeIconTrajectories :: FilePath
volatileScenario = "private-data/analysis/x-m1-1.50/ram-push/VolatileBank/scenario.json"
classicChromeTrajectories = "emulation/ram-push-cc-e2e-trajectories.json"
classicChromeIconTrajectories = "emulation/ram-push-cc-icon-e2e-trajectories.json"

-- | @NAME.fujimod.json@ beside a retained @NAME.json@.
fujimodVariant :: FilePath -> FilePath
fujimodVariant path = dropExtension path ++ ".fujimod.json"

-- | @bazel run@ of an emulator launcher on this output base, at niceness 10 so
-- the native suites' wall-clock case deadlines keep CPU priority.
bazelRun :: Context -> String -> [String] -> Invocation
bazelRun context target arguments = Invocation "nice"
  (["-n", "10", "bazel", "--output_base=" ++ base, "run"] ++ contextBazelOptions context ++ [target, "--"] ++ arguments) []
  where OutputBase base = contextOutputBase context

routeInvocation :: Context -> Route -> FilePath -> Invocation
routeInvocation context route receipt = case route of
  FlashBootCheck -> launcher "//emulation:flash-boot-check" ["--firmware", stockDat, "--output", receipt]
  FujiModFlashBootCheck -> launcher "//emulation:fujimod-flash-boot-check" ["--firmware", stockDat, "--output", receipt]
  RamPushVolatile -> ramPush
    ["--cardless", "--firmware", prefixDat, "--installed-nand", storedF7One, "--trajectories", volatileScenario]
  RamPushClassicChrome -> ramPush (["--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  RamPushClassicChromeIcon -> ramPush (["--classic-chrome-icon", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  FlashCandidateIcon -> ramPush (["--flash-candidate", "--firmware", iconFlashDat, "--installed-nand", storedF7One] ++ card
    ++ ["--trajectories", "emulation/classic-chrome-icon-trajectories.json"])
  FlashCandidateQTile -> ramPush (["--flash-candidate", "--firmware", iconFlashDat, "--installed-nand", storedF7One] ++ card
    ++ ["--trajectories", "emulation/classic-chrome-q-tile-trajectories.json"])
  FlashCandidateRefresh -> ramPush (["--flash-candidate", "--firmware", refreshFlashDat, "--installed-nand", storedF7One
    , "--sd-card-image", refreshCard, "--trajectories", "emulation/ui-trajectories.json", "--segments", "dial-refresh"])
  UsbToggleE8 -> ramPush (["--classic-chrome", "--attach-hold-q", "--firmware", rc0ToggleDat, "--installed-nand", storedF7Zero] ++ card)
  UpdaterRc0Toggle -> updater "restore-rc0-toggle"
  UpdaterCcToggle -> updater "restore-cc-toggle"
  NativeVolatile -> native "volatile"
  NativeSd -> native "sd"
  -- Unpinned: against a pinned run the icon e2e's develop passes differed from FujiMod's.
  RamPushCcCardless -> viewer ["--cardless", "--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One]
  RamPushCcIconCardless -> viewer ["--cardless", "--classic-chrome-icon", "--firmware", prefixDat, "--installed-nand", storedF7One]
  UsbToggleEmu -> launcher "//emulation:usb-toggle-emu" ["--fresh-boot", "--runs", "all", "--receipt-output", receipt]
  FujiModRamPushVolatile -> fujimod volatileScenario ["--cardless", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCcCardless -> fujimod classicChromeTrajectories
    ["--cardless", "--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCcIconCardless -> fujimod classicChromeIconTrajectories
    ["--cardless", "--classic-chrome-icon", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCc -> fujimod classicChromeTrajectories (["--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  FujiModUsbToggleE8 -> fujimod classicChromeTrajectories
    (["--classic-chrome", "--attach-hold-q", "--firmware", rc0ToggleDat, "--installed-nand", storedF7Zero] ++ card)
  FujiModUsbToggleEmu -> launcher "//emulation:usb-toggle-emu" ["--fujimod", "--fresh-boot", "--runs", "all", "--receipt-output", receipt]
  where
    Workspace root = contextWorkspace context
    RunDirectory run = contextRun context
    card = ["--sd-card-image", classicChromeCard]
    launcher = bazelRun context
    viewer arguments = launcher "//emulation:xm1-viewer"
      (["--ram-push-e2e"] ++ arguments ++ ["--output", run </> "runtimes" </> routeName route, "--receipt-output", receipt])
    -- The RAM push's host-paced transfer moves the animated source's phase
    -- between runs; frame 7 is the fixture of the fixed-scene icon e2e record.
    ramPush arguments = viewer (arguments ++ ["--fixed-scene-frame", "7"])
    -- FujiMod refuses --fixed-scene-frame; a route runs its retained file's
    -- FujiMod variant when one exists.
    fujimod trajectories arguments = viewer (["--fujimod"] ++ arguments ++
      ["--trajectories", if fujimodVariant trajectories `elem` contextVariants context then fujimodVariant trajectories else trajectories])
    updater restore =
      let card' = "private-data/analysis/usb-toggle/emu-updater" </> restore
       in launcher "//emulation:updater-f7"
            [ "--recovery-ram", card' </> "terminal-valid-ram.bin", "--sd-image", card' </> "firmware-ram-recovery-sd.img"
            , "--installed-nand", storedF7One, "--receipt-output", receipt ]
    -- Each emulator route runs in its own user scope at the default CPUWeight
    -- of 100, where niceness does not reach; the suites get ten times that.
    native mode = Invocation "systemd-run"
      [ "--user", "--scope", "--quiet", "--collect", "--property=CPUWeight=1000", "--"
      , "bash", root </> "emulation" </> "build-recipe-bank-cases.sh" ]
      [ ("RB_MODE", mode), ("JOBS", show (contextNativeJobs context))
      , ("BUILD_DIR", run </> "builds" </> routeName route), ("OUT_DIR", receipt)
      , ("RUNNER", tool "native-runner"), ("GEN", tool "recipe-bank-cases"), ("INTEROP", tool "recipe-bank-interop") ]
    tool name = contextRunfiles context </> "_main" </> "emulation" </> name

exclude :: String -> T.Text -> ExclusionReason -> Exclusion
exclude file pointer = Exclusion (filePattern file) (pointerPattern pointer)

-- | The checkpoint record every RAM-push and flash-candidate receipt carries.
-- @inputs@ is the checkpoint key document.
emulatorStart :: String -> T.Text -> [Exclusion]
emulatorStart file prefix =
  [ exclude file (prefix <> "/boot_seconds") Timing
  , exclude file (prefix <> "/key_seconds") Timing
  , exclude file (prefix <> "/save_seconds") Timing
  , exclude file (prefix <> "/restore_seconds") Timing
  , exclude file (prefix <> "/directory") CheckpointState
  , exclude file (prefix <> "/key") CheckpointState
  , exclude file (prefix <> "/inputs") CheckpointState
  , exclude file (prefix <> "/saved") CheckpointState
  , exclude file (prefix <> "/start") CheckpointState ]

-- | Fields of an @e2e-result.json@ that every RAM-push and flash-candidate run writes.
e2eResult :: String -> [Exclusion]
e2eResult file = emulatorStart file "/emulator_start" ++
  [ exclude file "/phases/*/seconds" Timing
  , exclude file "/observations/*/steps/*/seconds" Timing
  , exclude file "/menu_receipts/*/frame_number" Scheduling
  , exclude file "/menu_receipts/*/path" RunLocation ]

-- | The Classic Chrome icon trajectory's per-observation trace-hit instruction
-- counters and recurring live-view develop markers. Both differed between two
-- runs of one tree on the same restored checkpoint.
iconRunLength :: [Exclusion]
iconRunLength =
  [ exclude "e2e-result.json" "/observations/*/trace_hits/*/instructions" Scheduling
  , exclude "e2e-result.json" "/observations/*/classic_chrome_site_entries" Scheduling ]

ramPushSpec :: [Exclusion] -> ObservationSpec
ramPushSpec extra = ObservationSpec
  (map filePattern
    [ "e2e-result.json", "e2e-fetch-watch.json", "cold-restart/e2e-cold-restart.json"
    , "relinked/runtime.bin", "relinked/ram-push-manifest.json", "relinked/bank-allowlist.json", "relinked/kernel.syms"
    , "alloc/ram-alloc-state.json", "push/ram-push-state.json", "unload/ram-unload-state.json", "screenshots/*.png" ])
  (e2eResult "e2e-result.json" ++ emulatorStart "cold-restart/e2e-cold-restart.json" "/emulator_start" ++ extra ++
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

routeCheck :: Route -> Check
routeCheck route = case route of
  FlashBootCheck -> AgainstBaseline flashBootSpec
  FujiModFlashBootCheck -> AgainstBaseline fujimodFlashBootSpec
  RamPushVolatile -> AgainstBaseline (ramPushSpec [])
  RamPushClassicChrome -> AgainstBaseline (ramPushSpec [])
  RamPushClassicChromeIcon -> AgainstBaseline (ramPushSpec iconRunLength)
  UsbToggleE8 -> AgainstBaseline (ramPushSpec [])
  FlashCandidateIcon -> AgainstBaseline flashCandidateSpec
  FlashCandidateQTile -> AgainstBaseline flashCandidateSpec
  FlashCandidateRefresh -> AgainstBaseline flashCandidateSpec
  UpdaterRc0Toggle -> AgainstBaseline updaterSpec
  UpdaterCcToggle -> AgainstBaseline updaterSpec
  NativeVolatile -> AgainstBaseline nativeSpec
  NativeSd -> AgainstBaseline nativeSpec
  RamPushCcCardless -> Reference
  RamPushCcIconCardless -> Reference
  UsbToggleEmu -> Reference
  FujiModRamPushVolatile -> AgainstCounterpart RamPushVolatile True
  FujiModRamPushCcCardless -> AgainstCounterpart RamPushCcCardless True
  FujiModRamPushCcIconCardless -> AgainstCounterpart RamPushCcIconCardless True
  FujiModRamPushCc -> AgainstCounterpart RamPushClassicChrome True
  FujiModUsbToggleE8 -> AgainstCounterpart UsbToggleE8 True
  -- usb-toggle-emu exits 1 while any scenario blocks; the summary's results are compared.
  FujiModUsbToggleEmu -> AgainstCounterpart UsbToggleEmu False

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

execute :: Context -> MVar () -> String -> Invocation -> IO Execution
execute context lock name invocation = do
  let RunDirectory run = contextRun context
      Workspace root = contextWorkspace context
      logPath = run </> "logs" </> (name ++ ".log")
  inherited <- getEnvironment
  let scrubbed = filter ((`notElem` ["BUILD_WORKSPACE_DIRECTORY", "BUILD_WORKING_DIRECTORY", "RUNFILES_DIR", "RUNFILES_MANIFEST_FILE", "JAVA_RUNFILES"]) . fst) inherited
      environment = invocationEnvironment invocation ++ filter ((`notElem` map fst (invocationEnvironment invocation)) . fst) scrubbed
  say lock ("start " ++ name)
  started <- getMonotonicTime
  outcome <- try $ withFile logPath WriteMode $ \handle -> do
    hPutStrLn handle (unwords (invocationProgram invocation : invocationArguments invocation))
    hFlush handle
    (_, _, _, process) <- createProcess (proc (invocationProgram invocation) (invocationArguments invocation))
      { cwd = Just root, env = Just environment, std_in = NoStream, std_out = UseHandle handle, std_err = UseHandle handle }
    waitForProcess process
  ended <- getMonotonicTime
  let code = either (\problem -> ExitFailure (const 127 (problem :: SomeException))) id outcome
  say lock ("end " ++ name ++ ": " ++ showExit code ++ " after " ++ showSeconds (ended - started))
  pure (Execution code (ended - started) logPath)

showExit :: ExitCode -> String
showExit ExitSuccess = "exit 0"
showExit (ExitFailure code) = "exit " ++ show code

showSeconds :: Double -> String
showSeconds value = show (fromIntegral (round (value * 10) :: Integer) / 10 :: Double) ++ " s"

-- | Emulator routes on @slots@ workers in 'emulatorOrder' and native suites
-- on one worker, all from the start.
schedule :: Context -> Int -> [(Route, FilePath)] -> IO (Map.Map Route Execution)
schedule context slots jobs = do
  lock <- newMVar ()
  results <- newMVar Map.empty
  let emulator = [(route, receipt) | route <- emulatorOrder, Just receipt <- [lookup route jobs]]
      native = [(route, receipt) | (route, receipt) <- jobs, routeHost route == NativeSuite]
  queue <- newMVar emulator
  let take' = modifyMVar queue (\pending -> pure (drop 1 pending, take 1 pending))
      runRoute (route, receipt) = execute context lock (routeName route) (routeInvocation context route receipt)
        >>= \execution -> modifyMVar_ results (pure . Map.insert route execution)
      emulatorWorker = take' >>= \next -> case next of
        [] -> pure ()
        job : _ -> runRoute job >> emulatorWorker
      fork action = do
        done <- newEmptyMVar
        _ <- forkIO (action `finally` putMVar done ())
        pure done
  workers <- (:) <$> fork (mapM_ runRoute native) <*> replicateM (max 1 (min slots (length emulator))) (fork emulatorWorker)
  mapM_ takeMVar workers
  readMVar results

data Outcome = Outcome
  { outcomeRoute :: Route
  , outcomeReceipt :: FilePath
  , outcomeExecution :: Maybe Execution
  , outcomeResult :: T.Text
  , outcomeProblem :: Maybe String
  , outcomeDifferences :: [Difference]
  , outcomeBaseline :: Maybe FilePath
  , outcomeCounterpart :: Maybe (Route, FilePath)
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
   , "counterpart" .= fmap (\(route, path) -> object ["route" .= routeName route, "receipt" .= path]) (outcomeCounterpart outcome)
   , "problem" .= outcomeProblem outcome
   , "differences" .= length (outcomeDifferences outcome)
   , "first_difference" .= fmap differenceJson (case outcomeDifferences outcome of first : _ -> Just first; [] -> Nothing)
   ] ++ maybe [] executionFields (outcomeExecution outcome))

executionFields :: Execution -> [Pair]
executionFields execution =
  [ "exit_code" .= (case executionExit execution of ExitSuccess -> 0; ExitFailure code -> code)
  , "seconds" .= executionSeconds execution
  , "log" .= executionLog execution ]

-- | The executions an earlier verifier run reported, so its receipts are
-- judged and recorded under their routes' exit codes.
earlierExecutions :: FilePath -> IO (Map.Map Route Execution)
earlierExecutions run = do
  let path = run </> "report.json"
  present <- doesFileExist path
  if not present then pure Map.empty else do
    bytes <- B.readFile path
    either (\problem -> ioError (userError ("unreadable earlier report " ++ path ++ ": " ++ problem))) (pure . Map.fromList)
      (eitherDecodeStrict' bytes >>= parseEither (withObject "report" (\report -> report .: "routes" >>= fmap catMaybes . mapM entry)))
  where
    entry = withObject "route" $ \value -> do
      name <- value .: "route"
      code <- value .:? "exit_code"
      case (parseRoute name, code) of
        (Just route, Just exitCode) -> do
          seconds <- value .: "seconds"
          logPath <- value .: "log"
          pure (Just (route, Execution (if exitCode == (0 :: Int) then ExitSuccess else ExitFailure exitCode) seconds logPath))
        _ -> pure Nothing

-- | What judging needs beyond the route: whether to record, where baselines
-- live, the comparison tool, each route's receipt and where comparison output goes.
data Judging = Judging
  { judgingRecord :: Bool
  , judgingBaselines :: FilePath
  , judgingCompare :: FilePath
  , judgingReceipt :: Route -> FilePath
  , judgingOutput :: FilePath
  }

judge :: Judging -> Route -> Maybe Execution -> IO Outcome
judge judging route execution = do
  let receipt = judgingReceipt judging route
      failedRun = case executionExit <$> execution of
        Just (ExitFailure code) -> Just ("route exited " ++ show code)
        _ -> Nothing
  present <- doesDirectoryExist receipt
  case routeCheck route of
    _ | not present -> pure (Outcome route receipt execution "fail" (Just "receipt directory is missing") [] Nothing Nothing)
    Reference -> pure (Outcome route receipt execution "reference" failedRun [] Nothing Nothing)
    AgainstCounterpart counterpart requireExit -> do
      let reference = judgingReceipt judging counterpart
          outcome result problem = Outcome route receipt execution result problem [] Nothing (Just (counterpart, reference))
      referencePresent <- doesDirectoryExist reference
      if not referencePresent then pure (outcome "fail" (Just ("counterpart " ++ routeName counterpart ++ " has no receipt"))) else do
        (code, out, err) <- readProcessWithExitCode (judgingCompare judging) [reference, receipt] ""
        createDirectoryIfMissing True (judgingOutput judging </> "differences")
        writeFile (judgingOutput judging </> "differences" </> (routeName route ++ ".compare.txt")) (out ++ err)
        let reported = [drop (length prefix) line | line <- lines out, prefix `isPrefixOf` line]
            prefix = "e2e-observation-compare: "
            exitProblem = if requireExit then failedRun else Nothing
        pure $ case (code, exitProblem) of
          (ExitSuccess, Nothing) -> outcome "pass" Nothing
          (ExitSuccess, Just problem) -> outcome "fail" (Just problem)
          (ExitFailure _, _) -> outcome "fail" (Just (intercalate "; " (maybe [] pure exitProblem
            ++ [case reported of first : _ : _ -> last reported ++ ", first " ++ first; _ -> unwords (lines (out ++ err))])))
    AgainstBaseline spec -> do
      let baselinePath = judgingBaselines judging </> (routeName route ++ ".json")
          outcome result problem differences = Outcome route receipt execution result problem differences (Just baselinePath) Nothing
      observed <- observe spec (ReceiptDirectory receipt)
      if judgingRecord judging
        then case failedRun of
          Just problem -> pure (outcome "fail" (Just (problem ++ "; baseline not recorded")) [])
          Nothing | null observed -> pure (outcome "fail" (Just "no observed files; baseline not recorded") [])
                  | otherwise -> do
            now <- formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" <$> getCurrentTime
            let baselines = judgingBaselines judging
            createDirectoryIfMissing True baselines
            setFileMode baselines 0o700
            B.writeFile baselinePath (encodeBaseline (Baseline (T.pack (routeName route)) (T.pack now) receipt observed))
            setFileMode baselinePath 0o600
            pure (outcome "recorded" Nothing [])
        else do
          exists <- doesFileExist baselinePath
          if not exists then pure (outcome "fail" (Just "no baseline") []) else do
            parsed <- decodeBaseline <$> B.readFile baselinePath
            case parsed of
              Left problem -> pure (outcome "fail" (Just ("unreadable baseline: " ++ problem)) [])
              Right baseline -> do
                let differences = compareObservations (withoutExcluded spec (baselineFields baseline)) observed
                    passed = null differences && not (isJust failedRun)
                pure (outcome (if passed then "pass" else "fail") failedRun differences)

-- | Removes what a passing route staged beside its observed files: its viewer
-- runtime, native build directory, native case directories and updater
-- runtime. A full run's native case directories alone take about 10 GB.
prune :: RunDirectory -> Outcome -> IO ()
prune (RunDirectory run) outcome = do
  let name = routeName (outcomeRoute outcome)
      receipt = outcomeReceipt outcome
  caseDirectories <- if routeHost (outcomeRoute outcome) == NativeSuite
    then do
      present <- doesDirectoryExist receipt
      entries <- if present then listDirectory receipt else pure []
      pure (map (receipt </>) entries)
    else pure []
  forM_ ([run </> "runtimes" </> name, run </> "builds" </> name, receipt </> "runtime"] ++ caseDirectories) $ \path -> do
    directory <- doesDirectoryExist path
    when directory (removeDirectoryRecursive path)

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
        (Nothing, Just earlier) -> (Existing, Just (earlier </> "receipts" </> routeName route))
        (Nothing, Nothing) -> (Execute, Nothing)
  earlier <- maybe (pure Map.empty) earlierExecutions fromRun
  started <- stamp
  run <- beneathPrivate root (fromMaybe ("private-data/verify-candidate/runs" </> started) (optionOutput options)) "--output"
  createDirectory run
  setFileMode run 0o700
  baselines <- beneathPrivate root (fromMaybe "private-data/verify-candidate/baselines" (optionBaselines options)) "--baselines"
  let toExecute = [route | route <- selected, fst (sourceOf route) == Execute]
      receiptOf route = fromMaybe (run </> "receipts" </> routeName route) (snd (sourceOf route))
  commit <- readProcessWithExitCode "git" ["-C", let Workspace path = root in path, "rev-parse", "HEAD"] ""
  executable <- getExecutablePath
  let runfiles = executable ++ ".runfiles"
  variants <- filterM (doesFileExist . resolve root)
    (map fujimodVariant [volatileScenario, classicChromeTrajectories, classicChromeIconTrajectories])
  clockStart <- getMonotonicTime
  executions <- if null toExecute then pure Map.empty else do
    forM_ ["receipts", "logs", "builds", "runtimes"] $ \directory -> createDirectory (run </> directory)
    base <- outputBase executable
    policy <- slotPolicyFromEnvironment
    let SlotCount slots = slotCount policy
        context = Context root base runfiles (RunDirectory run) (optionNativeJobs options) (optionBazel options) variants
    putStrLn ("verify-candidate run: " ++ run)
    schedule context slots [(route, receiptOf route) | route <- toExecute]
  clockEnd <- getMonotonicTime
  let executionOf route
        | isJust (lookup route receipts) = Nothing
        | otherwise = maybe (Map.lookup route earlier) Just (Map.lookup route executions)
      judging = Judging (optionRecord options) baselines (runfiles </> "_main" </> "emulation" </> "e2e-observation-compare") receiptOf run
  outcomes <- forM selected $ \route -> judge judging route (executionOf route)
  forM_ outcomes $ \outcome -> when (outcomeRoute outcome `elem` toExecute && outcomeResult outcome `elem` ["pass", "recorded", "reference"]) $
    prune (RunDirectory run) outcome
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
