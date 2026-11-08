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
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, toJSON, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Pair, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate, isInfixOf, isPrefixOf, isSuffixOf, nub, sort, sortOn)
import Data.Ratio (denominator, numerator)
import qualified Data.Set as Set
import qualified Data.Vector as V

import qualified Data.Map.Strict as Map
import Data.Maybe (catMaybes, fromMaybe, isJust)
import qualified Data.Text as T
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Emulation.CandidateRoute
import Emulation.ExecutionJournal
import Emulation.SegmentReceipt (SegmentReceipt (..), SegmentResult (..), decodeDeclaredReceipts, segmentNameText)
import Emulation.WatchObservation (WatchJudgement (..))
import Emulation.CandidateBaseline
import Emulation.EmulatorSlot (SlotCount (..), SlotPolicy (..), insideSlot, slotPolicyFromEnvironment)
import Emulation.Sha256 (parseSha256Hex, sha256, sha256Hex)
import Emulation.KeyReleaseObservation (KeyReleaseObservation, readReleaseObservationLog, readReleaseObservationReceipt, writeReleaseObservationReceipt)
import GHC.Clock (getMonotonicTime)
import FujiAF.StorageBudget (checkStorageBudget)
import System.Directory (canonicalizePath, copyFile, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, removeDirectoryRecursive)
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitSuccess)
import System.FilePath (dropExtension, splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), IOMode (WriteMode), hFlush, hPutStrLn, hSetBuffering, stderr, stdout, withFile)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), StdStream (..), createProcess, proc, readProcessWithExitCode, waitForProcess)
import Text.Read (readMaybe)

-- | The recorded routes of E-VERIFY-CANDIDATE-150 decide the run's
-- result; the FujiMod routes and their retained counterparts are reported only.
routeGating :: Route -> Bool
routeGating route = route `elem`
  [ FlashBootCheck, RamPushVolatile, RamPushClassicChrome, RamPushClassicChromeIcon, FlashCandidateIcon
  , FlashCandidateQTile, FlashCandidateRefresh, UsbToggleE8, UpdaterRc0Toggle, UpdaterCcToggle, NativeVolatile, NativeSd ]

data ReceiptRequirement = NoE2EQualification | RequireE2ECandidateQualification deriving (Eq, Show)

routeReceiptRequirement :: Route -> ReceiptRequirement
routeReceiptRequirement route = case route of
  RamPushVolatile -> RequireE2ECandidateQualification
  RamPushClassicChrome -> RequireE2ECandidateQualification
  RamPushClassicChromeIcon -> RequireE2ECandidateQualification
  FlashCandidateIcon -> RequireE2ECandidateQualification
  FlashCandidateQTile -> RequireE2ECandidateQualification
  FlashCandidateRefresh -> RequireE2ECandidateQualification
  RamPushCcCardless -> RequireE2ECandidateQualification
  RamPushCcIconCardless -> RequireE2ECandidateQualification
  FujiModRamPushVolatile -> RequireE2ECandidateQualification
  FujiModRamPushCcCardless -> RequireE2ECandidateQualification
  FujiModRamPushCcIconCardless -> RequireE2ECandidateQualification
  FujiModRamPushCc -> RequireE2ECandidateQualification
  FlashBootCheck -> NoE2EQualification
  UsbToggleE8 -> NoE2EQualification
  UpdaterRc0Toggle -> NoE2EQualification
  UpdaterCcToggle -> NoE2EQualification
  NativeVolatile -> NoE2EQualification
  NativeSd -> NoE2EQualification
  FujiModFlashBootCheck -> NoE2EQualification
  UsbToggleEmu -> NoE2EQualification
  FujiModUsbToggleE8 -> NoE2EQualification
  FujiModUsbToggleEmu -> NoE2EQualification

candidateQualification :: FilePath -> IO (Maybe String)
candidateQualification receipt = do
  rootControl <- doesFileExist (receipt </> "e2e-control.json")
  if rootControl then pure (Just "control receipt cannot qualify a candidate") else do
    split <- doesFileExist (receipt </> "e2e-segments.json")
    flat <- doesFileExist (receipt </> "e2e-result.json")
    if split then
      if flat then pure (Just "flat and segmented receipts cannot both qualify a candidate")
      else do
        parsed <- decodeDeclaredReceipts <$> B.readFile (receipt </> "e2e-segments.json")
        case parsed of
          Left problem -> pure (Just ("unreadable e2e-segments.json: " ++ problem))
          Right receipts -> case [(entry, segmentReceiptName entry) | entry <- receipts, SegmentFailed _ <- [segmentReceiptResult entry]] of
            (entry, name) : _ -> case segmentReceiptResult entry of
              SegmentFailed text -> pure (Just ("failed segment cannot qualify a candidate: "
                ++ T.unpack (segmentNameText name) ++ ": " ++ T.unpack text))
              SegmentPassed -> pure (Just "failed segment cannot qualify a candidate")
            [] -> do
              problems <- mapM segment (map segmentReceiptName receipts)
              pure (firstProblem problems)
    else if not flat then pure (Just "no e2e qualification record") else single (receipt </> "e2e-result.json")
  where
    segment name = do
      let directory = receipt </> ("segment-" ++ T.unpack (segmentNameText name))
      control <- doesFileExist (directory </> "e2e-control.json")
      if control then pure (Just ("control receipt in segment-" ++ T.unpack (segmentNameText name) ++ " cannot qualify a candidate")) else do
        present <- doesFileExist (directory </> "e2e-result.json")
        if not present then pure (Just ("segment-" ++ T.unpack (segmentNameText name) ++ " has no e2e result")) else single (directory </> "e2e-result.json")
    single path = do
      parsed <- eitherDecodeStrict' <$> B.readFile path
      pure $ case parsed of
        Left problem -> Just ("unreadable e2e result: " ++ problem)
        Right value -> case parseEither (withObject "e2e result" (.: "outcome")) value of
          Left problem -> Just problem
          Right CandidateQualified -> Nothing
          Right ControlRefusedProtectedWrite -> Just "control-refused outcome cannot qualify a candidate"

firstProblem :: [Maybe String] -> Maybe String
firstProblem problems = case [problem | Just problem <- problems] of
  [] -> Nothing
  problem : _ -> Just problem

commandSucceeded :: ExecutionEvidence -> Bool
commandSucceeded execution = case execution of
  ExecutionKnown result -> executionExit result == ExitSuccess
  ExecutionUnknown _ -> False

-- | How a route's outcome is judged.
data Check
  = AgainstBaseline ObservationSpec
  -- | @e2e-observation-compare@ of the counterpart's receipt and this one,
  -- passing with no difference; the flag requires this route to exit 0.
  | AgainstCounterpart Route Bool
  -- | Run only as a counterpart's reference; not judged.
  | Reference

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

-- | Admitted @host-build run@ of an emulator launcher on this output base, at
-- niceness 10 so the native suites' wall-clock case deadlines keep CPU priority.
bazelRun :: Context -> String -> [String] -> Invocation
bazelRun context target arguments = Invocation "nice"
  (["-n", "10", "host-build", "--output_base=" ++ base, "run"] ++ contextBazelOptions context ++ [target, "--"] ++ arguments) []
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
  UsbToggleEmu -> launcher "//emulation:usb-toggle-emu" ["--runs", "all", "--receipt-output", receipt]
  FujiModRamPushVolatile -> fujimod volatileScenario ["--cardless", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCcCardless -> fujimod classicChromeTrajectories
    ["--cardless", "--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCcIconCardless -> fujimod classicChromeIconTrajectories
    ["--cardless", "--classic-chrome-icon", "--firmware", prefixDat, "--installed-nand", storedF7One]
  FujiModRamPushCc -> fujimod classicChromeTrajectories (["--classic-chrome", "--firmware", prefixDat, "--installed-nand", storedF7One] ++ card)
  FujiModUsbToggleE8 -> fujimod classicChromeTrajectories
    (["--classic-chrome", "--attach-hold-q", "--firmware", rc0ToggleDat, "--installed-nand", storedF7Zero] ++ card)
  FujiModUsbToggleEmu -> launcher "//emulation:usb-toggle-emu" ["--fujimod", "--runs", "all", "--receipt-output", receipt]
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
      , ("RUNNER", tool "native-runner"), ("GEN", tool "recipe-bank-cases"), ("INTEROP", tool "recipe-bank-interop")
      , ("RB_RUST_ARCHIVE", contextRunfiles context </> "_main" </> "replacement" </> "bank" </> "bank.a") ]
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
e2eResult file = emulatorStart file "/emulator_start" ++ instructionTimingExclusions (filePattern file) ++
  [ exclude file "/phases/*/seconds" Timing
  , exclude file "/observations/*/steps/*/seconds" Timing
  , exclude file "/rendezvous_exits/*/steps/*/seconds" Timing
  , exclude file "/menu_receipts/*/frame_number" Scheduling
  , exclude file "/menu_receipts/*/path" RunLocation ]

-- | Recurring live-view develop markers differed between runs of one tree on the same restored checkpoint.
iconRunLength :: [Exclusion]
iconRunLength =
  [ exclude "e2e-result.json" "/observations/*/classic_chrome_site_entries" Scheduling
  , exclude "segment-*/e2e-result.json" "/observations/*/classic_chrome_site_entries" Scheduling ]

ramPushSpec :: [Exclusion] -> ObservationSpec
ramPushSpec extra = ObservationSpec
  (map filePattern
    [ "e2e-result.json", "e2e-fetch-watch.json", "cold-restart/e2e-cold-restart.json"
    , "relinked/runtime.bin", "relinked/ram-push-manifest.json", "relinked/bank-allowlist.json", "relinked/kernel.syms"
    , "alloc/ram-alloc-state.json", "push/ram-push-state.json", "unload/ram-unload-state.json", "screenshots/*.png"
    , "e2e-segments.json" ])
  (e2eResult "e2e-result.json" ++ e2eResult "segment-*/e2e-result.json"
    ++ emulatorStart "cold-restart/e2e-cold-restart.json" "/emulator_start"
    ++ emulatorStart "segment-*/cold-restart/e2e-cold-restart.json" "/emulator_start" ++ extra ++
    ramPushCardImageExclusions ++
    [ exclude "e2e-result.json" "/states" RunLocation
    , exclude "e2e-result.json" "/runtime/manifest" RunLocation
    , exclude "segment-*/e2e-result.json" "/states" RunLocation
    , exclude "segment-*/e2e-result.json" "/runtime/manifest" RunLocation
    , exclude "e2e-segments.json" "/*/seconds" Timing
    , exclude "segment-*/e2e-observation-*.json" "/steps/*/seconds" Timing
    , exclude "segment-*/e2e-observation-*.json" "/trace_hits/*/instructions" Timing ])
  (map filePattern
    [ "e2e-result.json", "e2e-fetch-watch.json", "e2e-observation-*.json", "cold-restart/e2e-cold-restart.json"
    , "relinked/runtime.bin", "relinked/ram-push-manifest.json", "relinked/bank-allowlist.json", "relinked/kernel.syms"
    , "alloc/ram-alloc-state.json", "push/ram-push-state.json", "unload/ram-unload-state.json", "screenshots/*.png" ])

flashCandidateSpec :: ObservationSpec
flashCandidateSpec = ObservationSpec
  [filePattern "e2e-segments.json"]
  (exclude "e2e-segments.json" "/*/seconds" Timing : e2eResult "segment-*/e2e-result.json")
  (map filePattern ["e2e-result.json", "screenshots/*.png"])

updaterSpec :: ObservationSpec
updaterSpec = ObservationSpec
  [filePattern "receipt.json"]
  [ exclude "receipt.json" "/exported_nand/path" RunLocation
  , exclude "receipt.json" "/inputs/*/path" RunLocation ]
  []


fujimodFlashBootSpec :: ObservationSpec
fujimodFlashBootSpec = ObservationSpec
  [filePattern "fujimod-flash-boot-receipt.json"]
  [ exclude "fujimod-flash-boot-receipt.json" "/drive_wall_seconds" Timing
  , exclude "fujimod-flash-boot-receipt.json" "/emulator_source" CandidateIdentity
  , exclude "fujimod-flash-boot-receipt.json" "/firmware/path" RunLocation
  , exclude "fujimod-flash-boot-receipt.json" "/frame_png" RunLocation ]
  []

nativeSpec :: ObservationSpec
nativeSpec = ObservationSpec (map filePattern ["results.json", "findings.txt"]) [] []

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
  , optionResume :: Bool
  , optionLedger :: Maybe FilePath
  , optionSourceManifest :: Maybe FilePath
  , optionRouteInputManifest :: Maybe FilePath
  , optionBaselineManifest :: Maybe FilePath
  , optionSdk :: Maybe FilePath
  , optionEngine :: Maybe String
  , optionMatrix :: Bool
  , optionNativeTransition :: Maybe FilePath
  , optionCompositionManifest :: Maybe FilePath
  }

usage :: String
usage = unlines
  [ "verify-candidate [--routes NAME,...] [--output DIR] [--baselines DIR] [--native-jobs N]"
  , "                 [--from-run DIR] [--receipt NAME=DIR]... [--record-baseline] [--bazel-option OPTION]..."
  , "                 [--resume] [--execution-ledger FILE] [--matrix]"
  , "                 [--source-manifest FILE] [--route-input-manifest FILE] [--baseline-manifest FILE]"
  , "                 [--sdk DIR] [--engine COMMIT]"
  , "                 [--native-runner-transition FILE] [--composition-manifest FILE]"
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
  , "  --bazel-option OPT   option passed to each route's host-build run"
  , "  --resume             continue OUTPUT/checkpoint.json under its recorded identity"
  , "  --execution-ledger F preserved exits for routes supplied by --receipt or --from-run"
  , "  --matrix             require every route and report a whole-matrix verdict"
  , "  --source-manifest F  recorded source manifest bound into the checkpoint identity"
  , "  --route-input-manifest F  recorded route input manifest bound into the identity"
  , "  --baseline-manifest F     recorded baseline manifest bound into the identity"
  , "  --sdk DIR            recorded SDK store path bound into the identity"
  , "  --engine COMMIT      recorded engine source commit bound into the identity"
  , "  --native-runner-transition FILE  admitted SDK-pin runner transition manifest"
  , "  --composition-manifest FILE      two-batch composition manifest bound to the transition"
  , ""
  , "Routes: " ++ intercalate ", " (map routeName [minBound .. maxBound :: Route])
  ]

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing [] False 4 [] False Nothing Nothing Nothing Nothing Nothing Nothing False Nothing Nothing)
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
    go options ("--resume" : rest) = go options {optionResume = True} rest
    go options ("--execution-ledger" : value : rest) = go options {optionLedger = Just value} rest
    go options ("--source-manifest" : value : rest) = go options {optionSourceManifest = Just value} rest
    go options ("--route-input-manifest" : value : rest) = go options {optionRouteInputManifest = Just value} rest
    go options ("--baseline-manifest" : value : rest) = go options {optionBaselineManifest = Just value} rest
    go options ("--sdk" : value : rest) = go options {optionSdk = Just value} rest
    go options ("--engine" : value : rest) = go options {optionEngine = Just value} rest
    go options ("--matrix" : rest) = go options {optionMatrix = True} rest
    go options ("--native-runner-transition" : value : rest) = go options {optionNativeTransition = Just value} rest
    go options ("--composition-manifest" : value : rest) = go options {optionCompositionManifest = Just value} rest
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

data Execution = Execution
  { executionExit :: ExitCode
  , executionSeconds :: Maybe Seconds
  , executionLog :: Maybe FilePath
  , executionProvenance :: Maybe ExitProvenance
  }

data UnknownExecution = UnknownExecution
  { unknownReason :: String
  , unknownProvenance :: Maybe ExitProvenance
  }

data ExecutionEvidence = ExecutionUnknown UnknownExecution | ExecutionKnown Execution

executionExitCode :: Execution -> Int
executionExitCode execution = case executionExit execution of
  ExitSuccess -> 0
  ExitFailure code -> code

-- | Viewer routes write @key-releases/NAME.jsonl@; the launcher and native
-- routes do not.
routeObservesReleases :: Route -> Bool
routeObservesReleases route = route `elem`
  [ RamPushVolatile, RamPushClassicChrome, RamPushClassicChromeIcon, FlashCandidateIcon, FlashCandidateQTile
  , FlashCandidateRefresh, UsbToggleE8, RamPushCcCardless, RamPushCcIconCardless
  , FujiModRamPushVolatile, FujiModRamPushCcCardless, FujiModRamPushCcIconCardless, FujiModRamPushCc, FujiModUsbToggleE8 ]

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
    attempted <- try $ do
      checkStorageBudget (run </> "receipts" </> name)
      (_, _, _, process) <- createProcess (proc (invocationProgram invocation) (invocationArguments invocation))
        { cwd = Just root, env = Just environment, std_in = NoStream, std_out = UseHandle handle, std_err = UseHandle handle }
      waitForProcess process
    case attempted of
      Left problem -> hPutStrLn handle (show (problem :: SomeException)) >> pure (ExitFailure 127)
      Right code -> pure code
  ended <- getMonotonicTime
  let code = either (\problem -> ExitFailure (const 127 (problem :: SomeException))) id outcome
  say lock ("end " ++ name ++ ": " ++ showExit code ++ " after " ++ showSeconds (ended - started))
  pure (Execution code (Just (Seconds (ended - started))) (Just logPath)
    (Just (VerifierProvenance (VerifierReceipt run logPath Nothing))))

showExit :: ExitCode -> String
showExit ExitSuccess = "exit 0"
showExit (ExitFailure code) = "exit " ++ show code

showSeconds :: Double -> String
showSeconds value = show (fromIntegral (round (value * 10) :: Integer) / 10 :: Double) ++ " s"

releaseLogPath :: RunDirectory -> Route -> FilePath
releaseLogPath (RunDirectory run) route = run </> "key-releases" </> (routeName route ++ ".jsonl")

-- | Emulator routes on @slots@ workers in 'emulatorOrder' and native suites
-- on one worker, all from the start.
schedule :: Context -> Int -> [(Route, FilePath)] -> (Route -> Execution -> IO ()) -> IO (Map.Map Route Execution)
schedule context slots jobs record = do
  lock <- newMVar ()
  results <- newMVar Map.empty
  let emulator = [(route, receipt) | route <- emulatorOrder, Just receipt <- [lookup route jobs]]
      native = [(route, receipt) | (route, receipt) <- jobs, routeHost route == NativeSuite]
  queue <- newMVar emulator
  let take' = modifyMVar queue (\pending -> pure (drop 1 pending, take 1 pending))
      runRoute (route, receipt) = do
        let invocation = routeInvocation context route receipt
            observed = invocation {invocationEnvironment =
              [ ("XM1_KEY_RELEASE_ROUTE", routeName route)
              , ("XM1_KEY_RELEASE_LOG", releaseLogPath (contextRun context) route)
              ] ++ invocationEnvironment invocation}
        execution <- execute context lock (routeName route) observed
        modifyMVar_ results (pure . Map.insert route execution)
        record route execution
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

data ReleaseEvidence
  = ReleasesObserved [KeyReleaseObservation]
  | ReleasesMissing String
  | ReleasesNotApplicable

data Outcome = Outcome
  { outcomeRoute :: Route
  , outcomeReceipt :: FilePath
  , outcomeExecution :: ExecutionEvidence
  , outcomeResult :: T.Text
  , outcomeProblem :: Maybe String
  , outcomeDifferences :: [Difference]
  , outcomeBaseline :: Maybe FilePath
  , outcomeCounterpart :: Maybe (Route, FilePath)
  , outcomeReleases :: ReleaseEvidence
  , outcomeGeneration :: [GenerationAcceptance]
  , outcomeProvenance :: [NativeProvenanceChange]
  , outcomeComposition :: Maybe CompositionAnnotation
  }

differenceJson :: Difference -> Value
differenceJson difference =
  let (expected, actual) = differenceValue difference
      kind = case difference of
        Changed {} -> "changed" :: T.Text
        AbsentFromRun {} -> "absent-from-run"
        AbsentFromBaseline {} -> "absent-from-baseline"
      caseKind = case differenceField difference of
        FieldName "results.json" (Just [Key "cases", Key _]) -> case difference of
          Changed {} -> Just ("changed" :: T.Text)
          AbsentFromRun {} -> Just "removed"
          AbsentFromBaseline {} -> Just "added"
        _ -> Nothing
   in object (["field" .= renderFieldName (differenceField difference), "kind" .= kind, "baseline" .= expected, "run" .= actual]
     ++ maybe [] (\value -> ["case_change" .= value]) caseKind)

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
   , "generation" .= generationJson (outcomeGeneration outcome)
   , "evidence_kind" .= (if isJust (outcomeComposition outcome) then ("composition" :: T.Text) else "execution")
   , "provenance_changes" .= map provenanceJson (outcomeProvenance outcome)
   , "provenance_change_count" .= length (outcomeProvenance outcome)
   , "composition" .= compositionJson (outcomeComposition outcome)
   ] ++ releaseFields (outcomeReleases outcome) ++ executionFields (outcomeExecution outcome))

releaseFields :: ReleaseEvidence -> [Pair]
releaseFields releases = case releases of
  ReleasesObserved observations ->
    [ "release_evidence" .= ("observed" :: T.Text), "incomplete_releases" .= observations ]
  ReleasesMissing reason ->
    [ "release_evidence" .= ("missing" :: T.Text)
    , "incomplete_releases" .= (Nothing :: Maybe [KeyReleaseObservation])
    , "release_problem" .= reason ]
  ReleasesNotApplicable ->
    [ "release_evidence" .= ("not-applicable" :: T.Text)
    , "incomplete_releases" .= (Nothing :: Maybe [KeyReleaseObservation]) ]

executionFields :: ExecutionEvidence -> [Pair]
executionFields evidence = case evidence of
  ExecutionKnown execution ->
    [ "execution" .= ("known" :: T.Text)
    , "exit_code" .= executionExitCode execution ]
      ++ maybe [] (\value -> ["seconds" .= value]) (executionSeconds execution)
      ++ maybe [] (\value -> ["log" .= value]) (executionLog execution)
      ++ maybe [] (\value -> ["provenance" .= value]) (executionProvenance execution)
  ExecutionUnknown unknown ->
    [ "execution" .= ("unknown" :: T.Text)
    , "execution_reason" .= unknownReason unknown ]
      ++ maybe [] (\value -> ["provenance" .= value]) (unknownProvenance unknown)

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
          seconds <- value .:? "seconds"
          logPath <- value .:? "log"
          provenance <- value .:? "provenance"
          let resolved = fromMaybe
                (VerifierProvenance (VerifierReceipt run
                  (fromMaybe (run </> "logs" </> (routeName route ++ ".log")) logPath) Nothing))
                provenance
          pure (Just (route, Execution (if exitCode == (0 :: Int) then ExitSuccess else ExitFailure exitCode)
            seconds logPath (Just resolved)))
        _ -> pure Nothing

data NativeTransitionDocument = NativeTransitionDocument
  { transitionDocBaseline :: T.Text
  , transitionDocCandidate :: T.Text
  , transitionDocRunner :: FilePath
  , transitionDocBinding :: T.Text
  }

parseNativeTransitionDocument :: FilePath -> IO (Either String NativeTransitionDocument)
parseNativeTransitionDocument path = do
  present <- doesFileExist path
  if not present then pure (Left ("native runner transition manifest is not a file: " ++ path)) else do
    bytes <- B.readFile path
    pure $ eitherDecodeStrict' bytes >>= parseEither (withObject "native runner transition" $ \document ->
      NativeTransitionDocument
        <$> document .: "baseline_runner_sha256"
        <*> document .: "candidate_runner_sha256"
        <*> document .: "runner_path"
        <*> document .: "binding")

data CompositionAnnotation = CompositionAnnotation
  { compositionPath :: FilePath
  , compositionSha256 :: T.Text
  , compositionValue :: Value
  , compositionOverrideIds :: [T.Text]
  }

compositionJson :: Maybe CompositionAnnotation -> Value
compositionJson Nothing = Null
compositionJson (Just annotation) = object
  [ "manifest" .= compositionPath annotation
  , "sha256" .= compositionSha256 annotation
  , "override_ids" .= compositionOverrideIds annotation
  , "document" .= compositionValue annotation
  ]

data CompositionData = CompositionData
  { dataKind :: T.Text
  , dataComposed :: Value
  , dataBaselineDoc :: Value
  , dataDefaultSource :: Value
  , dataOverrideSource :: Value
  , dataBaselineBytes :: B.ByteString
  , dataReceiptResults :: B.ByteString
  , dataReceiptFindings :: B.ByteString
  , dataDefaultReport :: B.ByteString
  , dataDefaultFindings :: B.ByteString
  , dataDefaultValue :: Value
  , dataDefaultCases :: [Value]
  , dataDefaultIds :: [T.Text]
  , dataDefaultById :: Map.Map T.Text Value
  , dataOverrideReport :: B.ByteString
  , dataOverrideValue :: Value
  , dataOverrideCases :: [Value]
  , dataOverrideIds :: [T.Text]
  , dataOverrideById :: Map.Map T.Text Value
  , dataPerCase :: [(T.Text, B.ByteString)]
  , dataCandidate :: T.Text
  }

manifestFields :: Value -> Either String (T.Text, Value, Value, Value, Value)
manifestFields value =
  either (\problem -> Left ("composition manifest: " ++ problem)) Right
    (parseEither (withObject "composition" $ \document ->
      (,,,,) <$> document .: "kind" <*> document .: "composed" <*> document .: "baseline"
             <*> document .: "default_source" <*> document .: "override_source") value)

readRequired :: FilePath -> IO (Either String B.ByteString)
readRequired path = do
  present <- doesFileExist path
  if present then Right <$> B.readFile path else pure (Left ("composition source file is missing: " ++ path))

casesOf :: Value -> Either String [Value]
casesOf (Object members) = case KM.lookup "cases" members of
  Just (Array items) -> Right (V.toList items)
  _ -> Left "source report has no cases array"
casesOf _ = Left "source report is not an object"

caseIdOf :: Value -> Either String T.Text
caseIdOf value = case lookupKey "id" value of
  Just (String text) -> Right text
  _ -> Left "source case has no string id"

lookupKey :: T.Text -> Value -> Maybe Value
lookupKey key (Object members) = KM.lookup (K.fromText key) members
lookupKey _ _ = Nothing

topOf :: Value -> Value
topOf (Object members) = Object (KM.delete "cases" members)
topOf _ = Null

baselineCaseIds :: Observation -> [T.Text]
baselineCaseIds fields =
  [ text | Field (FieldName "results.json" (Just [Key "cases", Index _, Key "id"])) (String text) <- fields ]

loadSource :: FilePath -> IO (Either String (B.ByteString, Value, [Value], [T.Text], Map.Map T.Text Value))
loadSource path = do
  bytes <- readRequired path
  pure $ do
    raw <- bytes
    value <- either (\problem -> Left (path ++ ": " ++ problem)) Right (eitherDecodeStrict' raw)
    cases <- casesOf value
    ids <- traverse caseIdOf cases
    unless (nub ids == ids) (Left (path ++ ": duplicate case id"))
    let byId = Map.fromList (zip ids cases)
    pure (raw, value, cases, ids, byId)

loadCompositionAnnotation :: FilePath -> FilePath -> FilePath -> T.Text -> IO (Either String CompositionAnnotation)
loadCompositionAnnotation manifestPath receipt baselinePath candidateSha = do
  manifestPresent <- doesFileExist manifestPath
  if not manifestPresent then pure (Left ("composition manifest is not a file: " ++ manifestPath)) else do
    manifestBytes <- B.readFile manifestPath
    receiptResults <- B.readFile (receipt </> "results.json")
    receiptFindings <- B.readFile (receipt </> "findings.txt")
    baselineBytes <- B.readFile baselinePath
    case eitherDecodeStrict' manifestBytes of
      Left problem -> pure (Left ("unreadable composition manifest: " ++ problem))
      Right value -> case manifestFields value of
        Left problem -> pure (Left problem)
        Right (kind, composed, baselineDoc, defaultSource, overrideSource) ->
          case (textField "report" defaultSource, textField "findings" defaultSource, textField "report" overrideSource) of
            (Right defaultReportPath, Right defaultFindingsPath, Right overrideReportPath) -> do
              defaultFindings <- readRequired (T.unpack defaultFindingsPath)
              defaultLoaded <- loadSource (T.unpack defaultReportPath)
              overrideLoaded <- loadSource (T.unpack overrideReportPath)
              case (defaultFindings, defaultLoaded, overrideLoaded) of
                (Right defaultFindingsBytes, Right (defaultReportBytes, defaultValue, defaultCases, defaultIds, defaultById), Right (overrideReportBytes, overrideValue, overrideCases, overrideIds, overrideById)) -> do
                  perCase <- forM defaultIds $ \cid -> do
                    let path = takeDirectory (T.unpack defaultReportPath) </> T.unpack cid </> "behavioral-result.json"
                    loaded <- readRequired path
                    pure (fmap (\bytes -> (cid, bytes)) loaded)
                  case sequence perCase of
                    Left problem -> pure (Left problem)
                    Right perCaseBytes -> pure $ do
                      ids <- validateComposition CompositionData
                        { dataKind = kind
                        , dataComposed = composed
                        , dataBaselineDoc = baselineDoc
                        , dataDefaultSource = defaultSource
                        , dataOverrideSource = overrideSource
                        , dataBaselineBytes = baselineBytes
                        , dataReceiptResults = receiptResults
                        , dataReceiptFindings = receiptFindings
                        , dataDefaultReport = defaultReportBytes
                        , dataDefaultFindings = defaultFindingsBytes
                        , dataDefaultValue = defaultValue
                        , dataDefaultCases = defaultCases
                        , dataDefaultIds = defaultIds
                        , dataDefaultById = defaultById
                        , dataOverrideReport = overrideReportBytes
                        , dataOverrideValue = overrideValue
                        , dataOverrideCases = overrideCases
                        , dataOverrideIds = overrideIds
                        , dataOverrideById = overrideById
                        , dataPerCase = perCaseBytes
                        , dataCandidate = candidateSha
                        }
                      pure (CompositionAnnotation manifestPath (T.pack (sha256Hex (sha256 manifestBytes))) value ids)
                (Left problem, _, _) -> pure (Left problem)
                (_, Left problem, _) -> pure (Left problem)
                (_, _, Left problem) -> pure (Left problem)
            (Left problem, _, _) -> pure (Left problem)
            (_, Left problem, _) -> pure (Left problem)
            (_, _, Left problem) -> pure (Left problem)

validateComposition :: CompositionData -> Either String [T.Text]
validateComposition dataValue = do
  let digestOf = T.pack . sha256Hex . sha256
  unless (dataKind dataValue == ("native-baseline-input-casewise-composition" :: T.Text))
    (Left "composition kind is not native-baseline-input-casewise-composition")
  composedResults <- textField "results_sha256" (dataComposed dataValue)
  composedFindings <- textField "findings_sha256" (dataComposed dataValue)
  unless (composedResults == digestOf (dataReceiptResults dataValue)) (Left "composition results digest does not match the receipt")
  unless (composedFindings == digestOf (dataReceiptFindings dataValue)) (Left "composition findings digest does not match the receipt")
  baselineSha <- textField "sha256" (dataBaselineDoc dataValue)
  unless (baselineSha == digestOf (dataBaselineBytes dataValue)) (Left "composition baseline digest does not match the baseline file")
  baseline <- either (\problem -> Left ("composition baseline: " ++ problem)) Right (decodeBaseline (dataBaselineBytes dataValue))
  let baselineIds = baselineCaseIds (baselineFields baseline)
  unless (nub baselineIds == baselineIds) (Left "composition baseline repeats a case id")
  unless (sort baselineIds == sort (dataDefaultIds dataValue)) (Left "composition baseline case set differs from the default source")
  unless (length (dataDefaultCases dataValue) == length (dataDefaultIds dataValue)) (Left "composition default source repeats a case id")
  defaultSha <- textField "sha256" (dataDefaultSource dataValue)
  unless (defaultSha == digestOf (dataDefaultReport dataValue)) (Left "composition default source digest does not match its report")
  defaultFindingsSha <- textField "findings_sha256" (dataDefaultSource dataValue)
  unless (defaultFindingsSha == digestOf (dataDefaultFindings dataValue)) (Left "composition default findings digest does not match its file")
  unless (digestOf (dataReceiptFindings dataValue) == digestOf (dataDefaultFindings dataValue)) (Left "composition composed findings differ from the default source")
  defaultRunner <- textField "runner_sha256" (dataDefaultSource dataValue)
  unless (defaultRunner == dataCandidate dataValue) (Left "composition default source runner is not the candidate runner")
  overrideSha <- textField "sha256" (dataOverrideSource dataValue)
  unless (overrideSha == digestOf (dataOverrideReport dataValue)) (Left "composition override source digest does not match its report")
  overrideRunner <- textField "runner_sha256" (dataOverrideSource dataValue)
  unless (overrideRunner == dataCandidate dataValue) (Left "composition override source runner is not the candidate runner")
  listedIds <- arrayText "ids" (dataOverrideSource dataValue)
  unless (nub listedIds == listedIds) (Left "composition override source repeats an id")
  unless (sort listedIds == sort (dataOverrideIds dataValue)) (Left "composition override ids do not match the override source")
  unless (length (dataOverrideCases dataValue) == length (dataOverrideIds dataValue)) (Left "composition override source repeats a case id")
  unless (all (`elem` dataDefaultIds dataValue) listedIds) (Left "composition override id is absent from the default source")
  let perCaseDigests = sortOn fst [(cid, T.pack (sha256Hex (sha256 bytes))) | (cid, bytes) <- dataPerCase dataValue]
      caseManifest = T.pack (sha256Hex (sha256 (BC.pack (concat
        [ T.unpack cid ++ "\0" ++ T.unpack fileDigest ++ "\n"
        | (cid, fileDigest) <- perCaseDigests ]))))
  defaultCaseManifestSha <- textField "case_file_manifest_sha256" (dataDefaultSource dataValue)
  unless (defaultCaseManifestSha == caseManifest) (Left "composition default case file manifest differs")
  forM_ (dataPerCase dataValue) $ \(cid, bytes) -> do
    perCaseValue <- either (\problem -> Left ("composition default case " ++ T.unpack cid ++ ": " ++ problem)) Right (eitherDecodeStrict' bytes)
    unless (Map.lookup cid (dataDefaultById dataValue) == Just perCaseValue) (Left ("composition default case file differs from the default report: " ++ T.unpack cid))
  composedValue <- either (\problem -> Left ("composition composed report: " ++ problem)) Right (eitherDecodeStrict' (dataReceiptResults dataValue))
  composedCases <- casesOf composedValue
  composedIds <- traverse caseIdOf composedCases
  unless (composedIds == dataDefaultIds dataValue) (Left "composition composed case order or set differs from the default source")
  forM_ (zip composedIds composedCases) $ \(cid, caseValue) ->
    if cid `elem` listedIds
      then unless (Map.lookup cid (dataOverrideById dataValue) == Just caseValue) (Left ("composition override case differs: " ++ T.unpack cid))
      else unless (Map.lookup cid (dataDefaultById dataValue) == Just caseValue) (Left ("composition default case differs: " ++ T.unpack cid))
  unless (topOf composedValue == topOf (dataDefaultValue dataValue)) (Left "composition composed top-level metadata differs from the default source")
  pure listedIds

textField :: T.Text -> Value -> Either String T.Text
textField key (Object members) = case KM.lookup (K.fromText key) members of
  Just (String text) -> Right text
  _ -> Left ("composition field " ++ T.unpack key ++ " is not a string")
textField key _ = Left ("composition field " ++ T.unpack key ++ " has no object")

arrayText :: T.Text -> Value -> Either String [T.Text]
arrayText key (Object members) = case KM.lookup (K.fromText key) members of
  Just (Array items) -> traverse one (V.toList items)
  _ -> Left ("composition field " ++ T.unpack key ++ " is not an array")
  where
    one (String text) = Right text
    one _ = Left ("composition field " ++ T.unpack key ++ " has a non-string id")
arrayText key _ = Left ("composition field " ++ T.unpack key ++ " has no object")

provenanceJson :: NativeProvenanceChange -> Value
provenanceJson change = object
  [ "case" .= provenanceCase change
  , "field" .= renderFieldName (provenanceField change)
  , "kind" .= ("execution-provenance" :: T.Text)
  , "baseline" .= provenanceBaseline change
  , "candidate" .= provenanceCandidate change
  ]

-- | What judging needs beyond the route: whether to record, where baselines
-- live, the comparison tool, each route's receipt and where comparison output goes.
data Judging = Judging
  { judgingRecord :: Bool
  , judgingBaselines :: FilePath
  , judgingCompare :: FilePath
  , judgingReceipt :: Route -> FilePath
  , judgingOutput :: FilePath
  , judgingExecution :: Route -> ExecutionEvidence
  , judgingNativeTransition :: Maybe NativeRunnerTransition
  , judgingComposition :: Maybe CompositionAnnotation
  }

-- | A receipt field whose presence follows from the generation that wrote the
-- receipt rather than from the route's observations.
data GenerationKind
  = LegacyTransportSuccess
  | QualifiedOutcomeField
  | DeclaredTraceSitesField
  | RuntimeSymbolsField
  deriving (Eq, Ord, Show)

generationKindName :: GenerationKind -> T.Text
generationKindName kind = case kind of
  LegacyTransportSuccess -> "legacy-transport-success"
  QualifiedOutcomeField -> "qualified-outcome"
  DeclaredTraceSitesField -> "declared-trace-sites"
  RuntimeSymbolsField -> "runtime-symbols-sha256"

-- | Fields accepted as a generation change, one row per file and kind.
data GenerationAcceptance = GenerationAcceptance
  { acceptanceFile :: FilePath
  , acceptanceKind :: GenerationKind
  , acceptanceFields :: Int
  } deriving (Eq, Show)

generationJson :: [GenerationAcceptance] -> Value
generationJson accepted = toJSON
  [ object ["file" .= acceptanceFile entry, "kind" .= generationKindName (acceptanceKind entry)
           , "fields" .= acceptanceFields entry]
  | entry <- accepted ]

-- | The fields each receipt generation places differently.
generationKindOf :: FieldName -> Maybe GenerationKind
generationKindOf (FieldName file pointer)
  | takeFileName file /= "e2e-result.json" = Nothing
  | otherwise = case pointer of
      Just [Key "menu_receipts", Index _, Key "ok"] -> Just LegacyTransportSuccess
      Just [Key "outcome"] -> Just QualifiedOutcomeField
      Just (Key "declared_trace_sites" : _) -> Just DeclaredTraceSitesField
      Just [Key "runtime", Key "symbols_sha256"] -> Just RuntimeSymbolsField
      _ -> Nothing

-- | Applies the receipt-generation projection. The baseline's legacy transport
-- flag is accepted only when it is exactly true and the run does not carry it.
-- A run field the baseline's generation did not write is accepted only after
-- its declaration checks pass.
projectGeneration :: Observation -> Observation -> Either String (Observation, Observation, [GenerationAcceptance])
projectGeneration baseline observed = do
  let observedIndex = Map.fromList [(fieldName field, field) | field <- observed]
      fieldsByFile = Map.fromListWith (++) [(fieldFile (fieldName field), [field]) | field <- observed]
      hitsByDirectory = Map.fromListWith (++)
        [ (takeDirectory name, hits)
        | (name, siblings) <- Map.toList fieldsByFile
        , let hits = [text | field <- siblings, Just text <- [traceHit (fieldName field) (fieldValue field)]]
        , takeFileName name == "e2e-result.json"
          || ("e2e-observation-" `isPrefixOf` takeFileName name && ".json" `isSuffixOf` takeFileName name) ]
      baselineKinds = Map.fromListWith (++)
        [ ((fieldFile (fieldName field), kind), [field])
        | field <- baseline, Just kind <- [generationKindOf (fieldName field)] ]
      observedKinds = Map.fromListWith (++)
        [ ((fieldFile (fieldName field), kind), [field])
        | field <- observed, Just kind <- [generationKindOf (fieldName field)] ]
      legacy = [ field | field <- baseline
               , Just LegacyTransportSuccess <- [generationKindOf (fieldName field)]
               , fieldValue field == Bool True
               , Map.notMember (fieldName field) observedIndex ]
      legacyNames = Set.fromList (map fieldName legacy)
      expected = filter (\field -> Set.notMember (fieldName field) legacyNames) baseline
      additions = [ (file, kind, fields)
                  | ((file, kind), fields) <- Map.toList observedKinds
                  , kind /= LegacyTransportSuccess
                  , Map.notMember (file, kind) baselineKinds ]
      candidates = concatMap (\(_, _, fields) -> fields) additions
      candidateNames = Set.fromList (map fieldName candidates)
  forM_ additions $ \(file, kind, _) -> validateGeneration fieldsByFile hitsByDirectory file kind
  validateSymbols observed
  let projected = filter (\field -> Set.notMember (fieldName field) candidateNames) observed
      accepted = sortOn (\(GenerationAcceptance file kind _) -> (file, kind))
        (summarize LegacyTransportSuccess legacy
          ++ [GenerationAcceptance file kind (length fields) | (file, kind, fields) <- additions])
  pure (expected, projected, accepted)

summarize :: GenerationKind -> [Field] -> [GenerationAcceptance]
summarize kind fields =
  [ GenerationAcceptance file kind (length group)
  | (file, group) <- Map.toList
      (Map.fromListWith (++) [(fieldFile (fieldName field), [field]) | field <- fields]) ]

validateGeneration :: Map.Map FilePath [Field] -> Map.Map FilePath [T.Text] -> FilePath -> GenerationKind -> Either String ()
validateGeneration fieldsByFile hitsByDirectory file kind = case kind of
  QualifiedOutcomeField -> forM_ values $ \value ->
    unless (value == String "candidate-qualified")
      (Left ("outcome in " ++ file ++ " is not candidate-qualified: " ++ show value))
  DeclaredTraceSitesField -> validateDeclarations fieldsByFile hitsByDirectory file
  RuntimeSymbolsField -> forM_ values $ \value -> case value of
    Null -> pure ()
    String digest | isDigest digest -> pure ()
    _ -> Left ("runtime symbols digest in " ++ file ++ " is not a SHA-256 or null")
  LegacyTransportSuccess -> pure ()
  where
    values = [fieldValue field | field <- Map.findWithDefault [] file fieldsByFile
             , generationKindOf (fieldName field) == Just kind]

-- | Checks a run's declared trace sites: distinct, word-aligned eight-digit
-- addresses, nonempty distinct labels, well-formed read ranges, and a
-- declaration for every recorded trace hit in the file's observation scope.
validateDeclarations :: Map.Map FilePath [Field] -> Map.Map FilePath [T.Text] -> FilePath -> Either String ()
validateDeclarations fieldsByFile hitsByDirectory file = do
  let fields = Map.findWithDefault [] file fieldsByFile
      declared = [field | field <- fields, isDeclared (fieldName field)]
      root = [fieldValue field | field <- declared
             , fieldName field == FieldName file (Just [Key "declared_trace_sites"])]
      hits = Map.findWithDefault [] (takeDirectory file) hitsByDirectory
  case root of
    [Array _] -> case hits of
      first : _ -> Left ("trace hit " ++ T.unpack first ++ " has no declared site in " ++ file)
      [] -> pure ()
    [] -> do
      forM_ declared $ \field -> case fieldName field of
        FieldName _ (Just [Key "declared_trace_sites", Index _, Key "label"]) -> pure ()
        FieldName _ (Just [Key "declared_trace_sites", Index _, Key "address"]) -> pure ()
        FieldName _ (Just [Key "declared_trace_sites", Index _, Key "reads", Index _, Key "address"]) -> pure ()
        FieldName _ (Just [Key "declared_trace_sites", Index _, Key "reads", Index _, Key "length"]) -> pure ()
        name -> Left ("unexpected declared trace site field " ++ T.unpack (renderFieldName name))
      let indices = nub [index | field <- declared, Just index <- [siteIndex (fieldName field)]]
      sites <- forM indices $ \index -> do
        let at key = [fieldValue field | field <- declared
                     , fieldName field == FieldName file (Just [Key "declared_trace_sites", Index index, Key key])]
        label <- case at "label" of
          [String text] | not (T.null text) -> pure text
          _ -> Left ("declared trace site " ++ show index ++ " in " ++ file ++ " has no nonempty label")
        address <- case at "address" of
          [value] | Just parsed <- parseAddress value -> pure parsed
          _ -> Left ("declared trace site " ++ show index ++ " in " ++ file ++ " has no eight-digit address")
        unless (address `mod` 4 == 0)
          (Left ("declared trace site " ++ T.unpack label ++ " in " ++ file ++ " is not word aligned"))
        let readIndices = nub [readIndex | field <- declared
                             , Just (index', readIndex) <- [siteReadIndex (fieldName field)]
                             , index' == index]
        forM_ readIndices $ \read' -> do
          let readAt key = [fieldValue field | field <- declared
                           , fieldName field == FieldName file
                               (Just [Key "declared_trace_sites", Index index, Key "reads", Index read', Key key])]
          case readAt "address" of
            [value] | Just start <- parseAddress value, start `mod` 4 == 0 -> pure ()
            _ -> Left ("declared trace site " ++ T.unpack label ++ " in " ++ file ++ " has a malformed read address")
          unless (any positiveInteger (readAt "length"))
            (Left ("declared trace site " ++ T.unpack label ++ " in " ++ file ++ " has a malformed read length"))
        pure (label, address)
      unless (length (nub (map snd sites)) == length sites)
        (Left ("declared trace sites in " ++ file ++ " repeat an address"))
      unless (length (nub (map fst sites)) == length sites)
        (Left ("declared trace sites in " ++ file ++ " repeat a label"))
      forM_ hits $ \hit ->
        unless (hit `elem` map fst sites)
          (Left ("trace hit " ++ T.unpack hit ++ " is not a declared site in " ++ file))
    _ -> Left ("declared trace sites in " ++ file ++ " are not one array")
  where
    isDeclared (FieldName name pointer) = name == file && case pointer of
      Just (Key "declared_trace_sites" : _) -> True
      _ -> False

siteIndex :: FieldName -> Maybe Int
siteIndex (FieldName _ (Just [Key "declared_trace_sites", Index index, _])) = Just index
siteIndex _ = Nothing

siteReadIndex :: FieldName -> Maybe (Int, Int)
siteReadIndex (FieldName _ (Just [Key "declared_trace_sites", Index index, Key "reads", Index read', Key _])) =
  Just (index, read')
siteReadIndex _ = Nothing

traceHit :: FieldName -> Value -> Maybe T.Text
traceHit (FieldName _ pointer) (String label) = case pointer of
  Just [Key "observations", Index _, Key "trace_hits", Index _, Key "site"] -> Just label
  Just [Key "trace_hits", Index _, Key "site"] -> Just label
  _ -> Nothing
traceHit _ _ = Nothing

positiveInteger :: Value -> Bool
positiveInteger (Number number) =
  let value = toRational number
   in denominator value == 1 && numerator value > 0
positiveInteger _ = False

isDigest :: T.Text -> Bool
isDigest text = either (const False) (const True) (parseSha256Hex (T.unpack text))

parseAddress :: Value -> Maybe Integer
parseAddress (String text)
  | T.length text == 8 && T.all hexadecimal text =
      Just (T.foldl' (\value digit -> value * 16 + hexDigit digit) 0 text)
  where
    hexadecimal digit = digit >= '0' && digit <= '9' || digit >= 'a' && digit <= 'f'
    hexDigit digit = fromIntegral
      (if digit <= '9' then fromEnum digit - fromEnum '0' else fromEnum digit - fromEnum 'a' + 10)
parseAddress _ = Nothing

-- | A recorded runtime symbol digest must be the one the declaring receipt's
-- relinked/kernel.syms carries. A segment uses its own recorded file; the root
-- file is an explicit fallback for the producer's shared-root contract.
validateSymbols :: Observation -> Either String ()
validateSymbols fields = do
  forM_ digests $ \(file, value) -> case value of
    Null -> pure ()
    String digest
      | not (isDigest digest) -> Left ("runtime symbols digest in " ++ file ++ " is not a SHA-256")
      | otherwise -> do
          let scope = takeDirectory file
              scoped = if scope == "." then "relinked/kernel.syms" else scope </> "relinked" </> "kernel.syms"
              candidates = scoped : [ "relinked/kernel.syms" | scope /= "." ]
          case firstSymbolPath candidates of
            Nothing -> pure ()
            Just path -> case Map.lookup path symbolFiles of
              Just recorded
                | recorded /= "sha256:" <> digest ->
                    Left ("runtime symbols digest in " ++ file ++ " differs from " ++ path)
                | otherwise -> pure ()
              Nothing -> pure ()
    _ -> Left ("runtime symbols digest in " ++ file ++ " is not a SHA-256 or null")
  where
    digests = [(fieldFile (fieldName field), fieldValue field) | field <- fields
              , generationKindOf (fieldName field) == Just RuntimeSymbolsField]
    symbolFiles = Map.fromList
      [ (file, value)
      | Field (FieldName file Nothing) (String value) <- fields
      , takeFileName file == "kernel.syms" ]
    firstSymbolPath [] = Nothing
    firstSymbolPath (path : rest) = if Map.member path symbolFiles then Just path else firstSymbolPath rest

judge :: Judging -> Route -> ExecutionEvidence -> IO Outcome
judge judging route execution = do
  outcome <- judgeKnown judging route receipt execution
  pure $ case execution of
    ExecutionKnown _ -> outcome
    ExecutionUnknown unknown
      | routeHost route == NativeSuite
      , isJust (judgingComposition judging)
      , outcomeResult outcome == "pass" -> outcome
      | otherwise ->
          let problem = "route exit status is unknown: " ++ unknownReason unknown
           in outcome
                { outcomeResult = "fail"
                , outcomeProblem = Just (maybe problem (\existing -> existing ++ "; " ++ problem) (outcomeProblem outcome))
                }
  where receipt = judgingReceipt judging route

judgeKnown :: Judging -> Route -> FilePath -> ExecutionEvidence -> IO Outcome
judgeKnown judging route receipt execution = do
  let failedRun = case execution of
        ExecutionKnown result | ExitFailure code <- executionExit result -> Just ("route exited " ++ show code)
        _ -> Nothing
  present <- doesDirectoryExist receipt
  qualified <- case routeReceiptRequirement route of
    NoE2EQualification -> pure Nothing
    RequireE2ECandidateQualification
      | not (commandSucceeded execution) -> pure (Just "route command did not succeed; no candidate qualification")
      | not present -> pure (Just "receipt directory is missing")
      | otherwise -> candidateQualification receipt
  case qualified of
    Just problem -> pure (Outcome route receipt execution "fail" (Just problem) [] Nothing Nothing ReleasesNotApplicable [] [] Nothing)
    Nothing ->     case routeCheck route of
        _ | not present -> pure (Outcome route receipt execution "fail" (Just "receipt directory is missing") [] Nothing Nothing ReleasesNotApplicable [] [] Nothing)
        Reference -> pure (Outcome route receipt execution "reference" failedRun [] Nothing Nothing ReleasesNotApplicable [] [] Nothing)
        AgainstCounterpart counterpart requireExit -> do
          let reference = judgingReceipt judging counterpart
              outcome result problem = Outcome route receipt execution result problem [] Nothing (Just (counterpart, reference)) ReleasesNotApplicable [] [] Nothing
          referencePresent <- doesDirectoryExist reference
          referenceQualification <- case routeReceiptRequirement counterpart of
            RequireE2ECandidateQualification
              | referencePresent, not (commandSucceeded (judgingExecution judging counterpart)) ->
                  pure (Just "counterpart command did not succeed")
              | referencePresent -> candidateQualification reference
            _ -> pure Nothing
          if not referencePresent then pure (outcome "fail" (Just ("counterpart " ++ routeName counterpart ++ " has no receipt")))
          else case referenceQualification of
            Just problem -> pure (outcome "fail" (Just ("counterpart " ++ routeName counterpart ++ ": " ++ problem)))
            Nothing -> do
              (code, out, err) <- readProcessWithExitCode (judgingCompare judging) [reference, receipt] ""
              createDirectoryIfMissing True (judgingOutput judging </> "differences")
              writeFile (judgingOutput judging </> "differences" </> (routeName route ++ ".compare.txt")) (out ++ err)
              let reported = [drop (length prefix) line | line <- lines out, prefix `isPrefixOf` line]
                  prefix = "e2e-observation-compare: "
                  exitProblem
                    | not requireExit = Nothing
                    | otherwise = failedRun
              pure $ case (code, exitProblem) of
                (ExitSuccess, Nothing) -> outcome "pass" Nothing
                (ExitSuccess, Just problem) -> outcome "fail" (Just problem)
                (ExitFailure _, _) -> outcome "fail" (Just (intercalate "; " (maybe [] pure exitProblem
                  ++ [case reported of first : _ : _ -> last reported ++ ", first " ++ first; _ -> unwords (lines (out ++ err))])))
        AgainstBaseline spec -> do
          let baselinePath = judgingBaselines judging </> (routeName route ++ ".json")
              outcome result problem differences generation provenance =
                Outcome route receipt execution result problem differences (Just baselinePath) Nothing ReleasesNotApplicable generation provenance (judgingComposition judging)
          observed <- observe spec (ReceiptDirectory receipt)
          if judgingRecord judging
            then case failedRun of
              Just problem -> pure (outcome "fail" (Just (problem ++ "; baseline not recorded")) [] [] [])
              Nothing | null observed -> pure (outcome "fail" (Just "no observed files; baseline not recorded") [] [] [])
                      | otherwise -> do
                now <- formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" <$> getCurrentTime
                let baselines = judgingBaselines judging
                createDirectoryIfMissing True baselines
                setFileMode baselines 0o700
                B.writeFile baselinePath (encodeBaseline (Baseline (T.pack (routeName route)) (T.pack now) receipt observed))
                setFileMode baselinePath 0o600
                pure (outcome "recorded" Nothing [] [] [])
            else do
              exists <- doesFileExist baselinePath
              if not exists then pure (outcome "fail" (Just "no baseline") [] [] []) else do
                parsed <- decodeBaseline <$> B.readFile baselinePath
                case parsed of
                  Left problem -> pure (outcome "fail" (Just ("unreadable baseline: " ++ problem)) [] [] [])
                  Right baseline -> case projectGeneration (withoutExcluded spec (baselineFields baseline)) observed of
                    Left problem -> pure (outcome "fail" (Just problem) [] [] [])
                    Right (expected, run, generation) -> do
                      let comparison = if routeHost route == NativeSuite
                            then case compareNativeObservations (judgingNativeTransition judging) expected run of
                              Left problem -> Left problem
                              Right result -> Right (nativeDifferences result, nativeProvenanceChanges result)
                            else Right (compareObservations expected run, [])
                      case comparison of
                        Left problem -> pure (outcome "fail" (Just problem) [] generation [])
                        Right (differences, provenance) -> do
                          when (routeHost route == NativeSuite && any ((== FieldName "findings.txt" Nothing) . differenceField) differences) $
                            writeFindingsDiff judging route baseline receipt
                          let compositionProblem = case judgingComposition judging of
                                Just annotation
                                  | sort (map provenanceCase provenance) /= sort (compositionOverrideIds annotation) ->
                                      Just "composition provenance case ids do not match the admitted override ids"
                                _ -> Nothing
                              passed = null differences && not (isJust failedRun) && not (isJust compositionProblem)
                              problem = case (failedRun, compositionProblem) of
                                (Just runProblem, Just compositionProblem') -> Just (runProblem ++ "; " ++ compositionProblem')
                                (Just runProblem, Nothing) -> Just runProblem
                                (Nothing, Just compositionProblem') -> Just compositionProblem'
                                (Nothing, Nothing) -> Nothing
                          pure (outcome (if passed then "pass" else "fail") problem differences generation provenance)

writeFindingsDiff :: Judging -> Route -> Baseline -> FilePath -> IO ()
writeFindingsDiff judging route baseline receipt = do
  let spec = ObservationSpec [filePattern "findings.txt"] [] []
      source = baselineRecordedFrom baseline
      expected = [field | field <- baselineFields baseline, fieldName field == FieldName "findings.txt" Nothing]
      directory = judgingOutput judging </> "differences"
  original <- observe spec (ReceiptDirectory source)
  report <- if null expected || original /= expected
    then pure ("Line diff unavailable: recorded findings.txt is missing or no longer matches the baseline at " ++ source ++ "\n")
    else do
      (code, out, err) <- readProcessWithExitCode "diff"
        ["-u", "--label", "baseline/findings.txt", "--label", "run/findings.txt", "--",
         source </> "findings.txt", receipt </> "findings.txt"] ""
      pure $ case code of
        ExitSuccess -> out
        ExitFailure 1 -> out
        _ -> "Line diff unavailable: " ++ err
  createDirectoryIfMissing True directory
  writeFile (directory </> (routeName route ++ ".findings.diff")) report

-- | Removes per-route build and runtime staging. Native evidence is retained
-- by the producer's compact/full policy.
prune :: RunDirectory -> Outcome -> IO ()
prune (RunDirectory run) outcome = do
  let name = routeName (outcomeRoute outcome)
      receipt = outcomeReceipt outcome
  forM_ [run </> "runtimes" </> name, run </> "builds" </> name, receipt </> "runtime"] $ \path -> do
    directory <- doesDirectoryExist path
    when directory (removeDirectoryRecursive path)

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  when (arguments == ["--help"]) (putStr usage >> exitSuccess)
  options <- parseOptions arguments
  root <- workspaceRoot
  let Workspace rootPath = root
  let selected = fromMaybe allRoutes (optionRoutes options)
  require (not (optionMatrix options) || sort (map routeName selected) == sort (map routeName allRoutes))
    "--matrix requires every recorded route"
  when (optionMatrix options) $ do
    require (isJust (optionSdk options)) "--matrix requires --sdk"
    require (isJust (optionSourceManifest options)) "--matrix requires --source-manifest"
    require (isJust (optionRouteInputManifest options)) "--matrix requires --route-input-manifest"
    require (isJust (optionBaselineManifest options)) "--matrix requires --baseline-manifest"
    require (isJust (optionEngine options)) "--matrix requires --engine"
  fromRun <- traverse (canonicalizePath . resolve root) (optionFromRun options)
  receipts <- forM (optionReceipts options) $ \(route, directory) -> (,) route <$> canonicalizePath (resolve root directory)
  let sourceOf route = case (lookup route receipts, fromRun) of
        (Just directory, _) -> (ExistingSelection, Just directory)
        (Nothing, Just earlier) -> (ExistingSelection, Just (earlier </> "receipts" </> routeName route))
        (Nothing, Nothing) -> (ExecuteSelection, Nothing)
      sourceRoutes = [(route, fst (sourceOf route), snd (sourceOf route)) | route <- selected]
  earlier <- maybe (pure Map.empty) earlierExecutions fromRun
  started <- stamp
  run <- beneathPrivate root (fromMaybe ("private-data/verify-candidate/runs" </> started) (optionOutput options)) "--output"
  baselines <- beneathPrivate root (fromMaybe "private-data/verify-candidate/baselines" (optionBaselines options)) "--baselines"
  let receiptOf route = fromMaybe (run </> "receipts" </> routeName route) (snd (sourceOf route))
      checkpointPath = run </> "checkpoint.json"

  (nativeTransition, composition) <- case (optionNativeTransition options, optionCompositionManifest options) of
    (Nothing, Nothing) -> pure (Nothing, Nothing)
    (Just transitionArgument, Just compositionArgument) -> do
      require (selected == [NativeSd]) "--native-runner-transition requires --routes native-sd"
      require (isJust (lookup NativeSd (optionReceipts options)))
        "--composition-manifest requires --receipt native-sd=DIR"
      require (not (optionRecord options)) "--composition-manifest is comparison-only and does not record a baseline"
      require (not (isJust (optionLedger options))) "--composition-manifest does not accept --execution-ledger"
      require (not (isJust (optionFromRun options))) "--composition-manifest does not accept --from-run"
      require (not (optionResume options)) "--composition-manifest does not accept --resume"
      transitionPath <- canonicalizePath (resolve root transitionArgument)
      compositionPath <- canonicalizePath (resolve root compositionArgument)
      document <- parseNativeTransitionDocument transitionPath >>= either (ioError . userError) pure
      compositionBytes <- B.readFile compositionPath
      let compositionDigest = T.pack (sha256Hex (sha256 compositionBytes))
      require (transitionDocBinding document == compositionDigest)
        "native runner transition binding does not match the composition manifest digest"
      admitted <- admitNativeRunnerTransition (transitionDocRunner document)
        (transitionDocBaseline document) (transitionDocCandidate document) (transitionDocBinding document)
      transition <- either (ioError . userError) pure admitted
      let nativeReceipt = receiptOf NativeSd
          nativeBaseline = baselines </> "native-sd.json"
      annotation <- loadCompositionAnnotation compositionPath nativeReceipt nativeBaseline (transitionCandidate transition)
        >>= either (ioError . userError) pure
      pure (Just transition, Just annotation)
    _ -> ioError (userError "--native-runner-transition and --composition-manifest must be given together")

  executable <- getExecutablePath
  runfiles <- fromMaybe (executable ++ ".runfiles") <$> lookupEnv "RUNFILES_DIR"
  (actualSdk, sdkSourceDigest) <- readGeneratedSdk runfiles
  let StorePath actualSdkPath = actualSdk
  sdkOption <- traverse (canonicalizePath . resolve root) (optionSdk options)
  let allExisting = all ((== ExistingSelection) . fst . sourceOf) selected
  selectedSdk <- traverse (\path -> do
    parsed <- either (ioError . userError) pure (storePath path)
    require (parsed == actualSdk || allExisting)
      ("--sdk " ++ path ++ " does not match the generated launcher SDK " ++ actualSdkPath)
    pure parsed) sdkOption

  ledgerPath <- traverse (canonicalizePath . resolve root) (optionLedger options)
  ledger <- traverse readLedger ledgerPath
  ledgerMap <- Map.fromList <$> forM (maybe [] ledgerEntries ledger) (\entry -> pure (entryRoute entry, entryEvidence entry))
  forM_ (Map.keys ledgerMap) $ \route ->
    when (fst (sourceOf route) == ExecuteSelection) $
      ioError (userError ("execution ledger supplies " ++ routeName route ++ " but the route is selected to execute"))
  forM_ (Map.toList ledgerMap) $ \(route, _) -> case (ledger >>= ledgerRun, snd (sourceOf route)) of
    (Just ledgerDirectory, Just receiptDirectory) -> do
      ledgerReal <- canonicalizePath ledgerDirectory
      receiptReal <- canonicalizePath (receiptRunRoot receiptDirectory)
      require (ledgerReal == receiptReal)
        ("execution ledger run " ++ ledgerReal ++ " does not hold receipt " ++ receiptReal ++ " for " ++ routeName route)
    _ -> pure ()

  let existingEvidence route = case Map.lookup route ledgerMap of
        Just evidence -> evidence
        Nothing -> case (lookup route receipts, Map.lookup route earlier) of
          (Just _, _) -> unknownEvidence "a receipt override does not supply an exit code"
          (Nothing, Just execution) -> fromExecution execution
          (Nothing, Nothing) -> unknownEvidence "no execution ledger entry for existing route"

  sourceManifest <- traverse (canonicalizePath . resolve root) (optionSourceManifest options)
  routeInputManifest <- traverse (canonicalizePath . resolve root) (optionRouteInputManifest options)
  baselineManifest <- traverse (canonicalizePath . resolve root) (optionBaselineManifest options)
  sourceDigest <- traverse manifestDigest sourceManifest
  routeInputDigest <- traverse manifestDigest routeInputManifest
  baselineDigest <- traverse manifestDigest baselineManifest
  ledgerDigest <- traverse manifestDigest ledgerPath
  sourceDrift <- maybe (pure []) (manifestDrift rootPath) sourceManifest
  routeInputDrift <- maybe (pure []) (manifestDrift rootPath) routeInputManifest
  baselineDrift <- maybe (pure []) (manifestDrift rootPath) baselineManifest
  forM_ sourceDrift $ \drift -> do
    require (driftPath drift `elem` ["emulation/VerifyCandidate.hs", "emulation/Emulation/CandidateBaseline.hs"])
      ("source drift outside the reviewed host verifier: " ++ driftPath drift)
    require (isJust (driftCurrent drift))
      ("the reviewed coordinator source is missing: " ++ driftPath drift)
  require (null routeInputDrift) ("route input drift: " ++ renderDrift routeInputDrift)
  require (null baselineDrift) ("baseline drift: " ++ renderDrift baselineDrift)
  hostVerifierSources <- case sourceManifest of
    Nothing -> pure []
    Just _ -> forM ["emulation/VerifyCandidate.hs", "emulation/Emulation/ExecutionJournal.hs", "emulation/Emulation/CandidateRoute.hs", "emulation/Emulation/CandidateBaseline.hs"] $ \path -> do
      present <- doesFileExist (rootPath </> path)
      require present ("reviewed host verifier source is missing: " ++ path)
      manifestDigest (rootPath </> path)
  earlierReport <- case fromRun of
    Just directory -> do
      let path = directory </> "report.json"
      present <- doesFileExist path
      if present then Just <$> manifestDigest path else pure Nothing
    Nothing -> pure Nothing
  commit <- readProcessWithExitCode "git" ["-C", rootPath, "rev-parse", "HEAD"] ""
  let headText = case commit of
        (ExitSuccess, out, _) -> Just (takeWhile (/= '\n') out)
        _ -> Nothing
      headCommit = headText >>= either (const Nothing) Just . commitId
  engineCommit <- traverse (\text -> either (ioError . userError) pure (commitId text)) (optionEngine options)
  let identityRecord = CheckpointIdentity
        { identityWorkspace = rootPath
        , identityRun = run
        , identityHead = headCommit
        , identitySdk = fromMaybe actualSdk selectedSdk
        , identitySdkSource = sdkSourceDigest
        , identityEngine = engineCommit
        , identityMatrix = optionMatrix options
        , identityBaselines = baselines
        , identityRoutes = [IdentityRoute route source receipt | (route, source, receipt) <- sourceRoutes]
        , identitySourceManifest = sourceDigest
        , identitySourceDrift = sourceDrift
        , identityRouteInputManifest = routeInputDigest
        , identityRouteInputDrift = routeInputDrift
        , identityBaselineManifest = baselineDigest
        , identityBaselineDrift = baselineDrift
        , identityExecutionLedger = ledgerDigest
        , identityEarlierReport = earlierReport
        , identityHostVerifierSources = hostVerifierSources
        }

  resumed <- if optionResume options
    then do
      require (isJust (optionOutput options)) "--resume requires --output naming the checkpoint's run"
      checkpoint <- readCheckpoint checkpointPath
      require (checkpointIdentity checkpoint == identityRecord)
        ("checkpoint identity differs from the current inputs: " ++ checkpointPath)
      let entries = [(journalRoute entry, entry) | entry <- checkpointRoutes checkpoint]
      forM_ entries $ \(route, _) ->
        require (route `elem` selected)
          ("checkpoint records " ++ routeName route ++ ", which --routes does not select")
      pure (Just (Map.fromList entries))
    else do
      present <- doesFileExist checkpointPath
      when present (ioError (userError ("checkpoint exists; pass --resume to continue " ++ checkpointPath)))
      pure Nothing

  let checkpointMap = fromMaybe Map.empty resumed
      reusable route = case Map.lookup route checkpointMap of
        Just entry -> case journalRouteEvidence entry of
          EvidenceKnown {} -> True
          EvidenceUnknown {} -> False
        Nothing -> False
      blocked route = case Map.lookup route checkpointMap of
        Just entry -> case journalRouteEvidence entry of
          EvidenceKnown {} -> False
          EvidenceUnknown {} -> True
        Nothing -> False
      toExecute = [route | (route, source, _) <- sourceRoutes, source == ExecuteSelection, not (reusable route), not (blocked route)]

  inSlot <- insideSlot
  require (not (inSlot && not (null toExecute)))
    "verify-candidate runs each emulator route in a slot of its own; start it outside //emulation:emulator-slot"

  forM_ (Map.toList checkpointMap) $ \(route, entry) -> case fst (sourceOf route) of
    ExistingSelection ->
      require (journalRouteSource entry == ExistingEvidence && journalRouteEvidence entry == existingEvidence route)
        ("checkpoint conflicts with execution ledger for " ++ routeName route)
    ExecuteSelection -> require (journalRouteSource entry == ExecutedEvidence)
      ("checkpoint records " ++ routeName route ++ " as an existing route but the route is selected to execute")

  let initialEvidence :: Map.Map Route JournalEvidence
      initialEvidence = Map.fromList
        [ (route, evidence)
        | (route, source, _) <- sourceRoutes
        , Just evidence <- [case source of
            ExistingSelection -> Just (existingEvidence route)
            ExecuteSelection -> journalRouteEvidence <$> Map.lookup route checkpointMap]
        ]
      evidenceSource route = case fst (sourceOf route) of
        ExistingSelection -> ExistingEvidence
        ExecuteSelection -> ExecutedEvidence
      checkpointOf evidence = Checkpoint identityRecord
        [ JournalRoute route (evidenceSource route) (receiptOf route) value
        | (route, value) <- Map.toList evidence ]
  unless (isJust resumed) $ do
    unless (null toExecute) (checkStorageBudget run)
    createDirectory run
    setFileMode run 0o700
  writeCheckpoint checkpointPath (checkpointOf initialEvidence)

  variants <- filterM (doesFileExist . resolve root)
    (map fujimodVariant [volatileScenario, classicChromeTrajectories, classicChromeIconTrajectories])
  clockStart <- getMonotonicTime
  evidenceLock <- newMVar initialEvidence
  executions <- if null toExecute then pure Map.empty else do
    forM_ ["receipts", "logs", "builds", "runtimes", "key-releases"] $ \directory ->
      createDirectoryIfMissing True (run </> directory)
    when (any ((== NativeSuite) . routeHost) toExecute) $
      copyFile (runfiles </> "_main" </> "replacement" </> "bank" </> "bank.a") (run </> "builds" </> "bank.a")
    base <- outputBase executable
    policy <- slotPolicyFromEnvironment
    let SlotCount slots = slotCount policy
        context = Context root base runfiles (RunDirectory run) (optionNativeJobs options) (optionBazel options) variants
        record route execution = modifyMVar_ evidenceLock $ \current -> do
          let updated = Map.insert route (fromExecution execution) current
          writeCheckpoint checkpointPath (checkpointOf updated)
          pure updated
    putStrLn ("verify-candidate run: " ++ run)
    schedule context slots [(route, receiptOf route) | route <- toExecute] record
  clockEnd <- getMonotonicTime
  evidence <- modifyMVar evidenceLock $ \current -> do
    let updated = foldl (\acc (route, execution) -> Map.insert route (fromExecution execution) acc)
          current (Map.toList executions)
    writeCheckpoint checkpointPath (checkpointOf updated)
    pure (updated, updated)
  let executionOf route = case Map.lookup route evidence of
        Just value -> toExecutionEvidence value
        Nothing
          | fst (sourceOf route) == ExistingSelection -> toExecutionEvidence (existingEvidence route)
          | otherwise -> ExecutionUnknown (UnknownExecution "no execution evidence" Nothing)
      judging = Judging (optionRecord options) baselines
        (runfiles </> "_main" </> "emulation" </> "e2e-observation-compare") receiptOf run executionOf
        nativeTransition composition
  outcomes <- forM selected $ \route -> do
    outcome <- judge judging route (executionOf route)
    releases <- releaseEvidence run route (receiptOf route) (route `elem` toExecute)
    pure outcome {outcomeReleases = releases}
  forM_ outcomes $ \outcome -> when (outcomeRoute outcome `elem` toExecute) $
    prune (RunDirectory run) outcome
  forM_ outcomes $ \outcome -> unless (null (outcomeDifferences outcome)) $ do
    createDirectoryIfMissing True (run </> "differences")
    BL.writeFile (run </> "differences" </> (routeName (outcomeRoute outcome) ++ ".json"))
      (encode (map differenceJson (outcomeDifferences outcome)) <> "\n")
  let reportPath = run </> "report.json"
      matrixComplete = optionMatrix options && sort (map routeName selected) == sort (map routeName allRoutes)
      gatingPassed = all ((`elem` ["pass", "recorded"]) . outcomeResult) (filter (routeGating . outcomeRoute) outcomes)
      passed = gatingPassed && (not (optionMatrix options) || matrixComplete)
  BL.writeFile reportPath (encode (object
    [ "schema" .= (2 :: Int)
    , "scope" .= ("host emulation and native case suites; no camera or USB device" :: T.Text)
    , "resource_scope" .= ("fresh emulator routes use the launcher default 2G/Swap0; completed routes retain the scopes recorded for their run" :: T.Text)
    , "workspace" .= rootPath
    , "run" .= run
    , "checkpoint" .= checkpointPath
    , "execution_ledger" .= ledgerPath
    , "commit" .= headCommit
    , "started_at" .= started
    , "mode" .= (if optionRecord options then "record" else "compare" :: T.Text)
    , "matrix" .= optionMatrix options
    , "matrix_complete" .= matrixComplete
    , "wall_seconds" .= (clockEnd - clockStart)
    , "result" .= (if passed then "pass" else "fail" :: T.Text)
    , "policy" .= case nativeTransition of
        Nothing -> Null
        Just transition -> object
          [ "kind" .= ("native-sdk-pin-qualification" :: T.Text)
          , "native_runner_transition" .= object
              [ "baseline_runner_sha256" .= transitionBaseline transition
              , "candidate_runner_sha256" .= transitionCandidate transition
              , "runner_path" .= transitionRunnerPath transition
              , "binding" .= transitionBinding transition
              ]
          , "composition" .= compositionJson composition
          ]
    , "routes" .= map outcomeJson outcomes ]) <> "\n")
  forM_ outcomes $ \outcome -> putStrLn (routeName (outcomeRoute outcome) ++ ": " ++ T.unpack (outcomeResult outcome)
    ++ (if routeGating (outcomeRoute outcome) then "" else " [non-gating]")
    ++ maybe "" (\problem -> " (" ++ problem ++ ")") (outcomeProblem outcome)
    ++ case outcomeDifferences outcome of
         [] -> ""
         first : _ -> "; " ++ show (length (outcomeDifferences outcome)) ++ " differences, first "
           ++ T.unpack (renderFieldName (differenceField first))
    ++ case outcomeProvenance outcome of
         [] -> ""
         changes -> "; " ++ show (length changes) ++ " execution-provenance changes")
  putStrLn ("wall time: " ++ showSeconds (clockEnd - clockStart))
  putStrLn ("report: " ++ reportPath)
  unless passed (hPutStrLn stderr "verification failed" >> exitFailure)

unknownEvidence :: String -> JournalEvidence
unknownEvidence reason = EvidenceUnknown (UnknownReason reason)

fromExecution :: Execution -> JournalEvidence
fromExecution execution = EvidenceKnown (ExitStatus (executionExit execution)) (executionSeconds execution)
  (executionLog execution) (executionProvenance execution)

toExecutionEvidence :: JournalEvidence -> ExecutionEvidence
toExecutionEvidence evidence = case evidence of
  EvidenceKnown (ExitStatus exit) seconds logPath provenance ->
    ExecutionKnown (Execution exit seconds logPath provenance)
  EvidenceUnknown (UnknownReason reason) -> ExecutionUnknown (UnknownExecution reason Nothing)

receiptRunRoot :: FilePath -> FilePath
receiptRunRoot receipt
  | takeFileName (takeDirectory receipt) == "receipts" = takeDirectory (takeDirectory receipt)
  | otherwise = takeDirectory receipt

releaseEvidence :: FilePath -> Route -> FilePath -> Bool -> IO ReleaseEvidence
releaseEvidence run route receipt executed
  | executed = do
      let logPath = run </> "key-releases" </> (routeName route ++ ".jsonl")
      records <- readReleaseObservationLog logPath
      writeReleaseObservationReceipt (receipt </> "key-release-observations.json") records
      pure (ReleasesObserved records)
  | otherwise = do
      present <- doesDirectoryExist receipt
      if not present then pure (ReleasesMissing ("receipt directory is missing: " ++ receipt)) else do
        let receiptFile = receipt </> "key-release-observations.json"
        receiptPresent <- doesFileExist receiptFile
        if receiptPresent
          then ReleasesObserved <$> readReleaseObservationReceipt receiptFile
          else do
            let raw = receiptRunRoot receipt </> "key-releases" </> (routeName route ++ ".jsonl")
            rawPresent <- doesFileExist raw
            if rawPresent
              then do
                records <- readReleaseObservationLog raw
                writeReleaseObservationReceipt receiptFile records
                pure (ReleasesObserved records)
              else if routeObservesReleases route
                then pure (ReleasesMissing ("no key-release observations at " ++ receiptFile ++ " or " ++ raw))
                else pure ReleasesNotApplicable

-- | The SDK named by the generated module the route launchers are built from.
readGeneratedSdk :: FilePath -> IO (StorePath, FileDigest)
readGeneratedSdk runfiles = do
  let path = runfiles </> "_main" </> "emulation" </> "FujiModPaths.hs"
  present <- doesFileExist path
  require present ("generated FujiModPaths.hs is missing: " ++ path)
  text <- readFile path
  let values = [value | line <- lines text, let stripped = dropWhile (== ' ') line
                      , "fujimod" `isPrefixOf` stripped, "= \"" `isInfixOf` stripped
                      , let value = takeWhile (/= '"') (drop 1 (dropWhile (/= '"') stripped))
                      , not (null value)]
  case values of
    driver : _ -> do
      sdk <- either (ioError . userError) pure (storePath (takeDirectory (takeDirectory driver)))
      let StorePath sdkPath = sdk
      forM_ values $ \value ->
        require ((sdkPath ++ "/") `isPrefixOf` value)
          ("generated FujiMod path is outside the SDK: " ++ value)
      digest <- manifestDigest path
      pure (sdk, digest)
    [] -> ioError (userError ("generated FujiModPaths.hs names no FujiMod paths: " ++ path))

renderDrift :: [SourceDrift] -> String
renderDrift = intercalate ", " . map render
  where
    render drift = driftPath drift ++ " expected " ++ sha256Hex (driftExpected drift)
      ++ " current " ++ maybe "missing" sha256Hex (driftCurrent drift)
