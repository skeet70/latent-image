{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Exception (IOException, displayException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (parseJSON), Result (..), Value (..), eitherDecodeStrict', fromJSON, object, toJSON, withObject, withText, (.:), (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.Char (isDigit)
import Data.List (isInfixOf, isPrefixOf, isSuffixOf, nub, sort, stripPrefix, tails)
import Data.Maybe (fromMaybe, listToMaybe)
import Emulation.EmulatorSlot (SlotCount (..), SlotPolicy (..), insideSlot, reexecInSlot, slotPolicyFromEnvironment)
import qualified Emulation.HostAdmission as Admission
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import FujiAF.Host (failH, get, num, str)
import FujiAF.JSON (encodeCanonical)
import FujiAF.StorageBudget (checkStorageBudget)
import System.Directory
  ( createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getFileSize
  , listDirectory, removeFile, removePathForcibly, renameDirectory, renameFile, setCurrentDirectory )
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>), takeDirectory)
import System.IO (IOMode (ReadMode, WriteMode), SeekMode (AbsoluteSeek), hSeek, stderr, withBinaryFile, withFile)
import System.Posix.Process (getProcessID)
import System.Process
  ( CreateProcess (..), StdStream (NoStream, UseHandle), createProcess, proc, rawSystem
  , readProcess, waitForProcess )
import Text.Read (readMaybe)

newtype CaseId = CaseId String deriving (Eq, Ord, Show)
newtype CommitId = CommitId String deriving (Eq, Show)
newtype Predicate = Predicate String deriving (Eq, Show)

data Role = Stock | Owned deriving (Eq, Show)

data QualificationScope = HeadroomOnly | FullMatrix

data MatrixVerdict = MatrixQualified | MatrixUnqualified

matrixVerdictText :: MatrixVerdict -> String
matrixVerdictText MatrixQualified = "qualified"
matrixVerdictText MatrixUnqualified = "unqualified"

scopeText :: QualificationScope -> String
scopeText HeadroomOnly = "headroom-only"
scopeText FullMatrix = "full-matrix"

slottedArgument :: QualificationScope -> String
slottedArgument HeadroomOnly = "headroom-slotted"
slottedArgument FullMatrix = "execute-slotted"

data HostBuildVerdict = BuildPassed | BuildFailed | BuildInterrupted | BuildResourceFailure | BuildUnknown

instance FromJSON HostBuildVerdict where
  parseJSON = withText "host-build verdict" $ \text -> case text of
    "passed" -> pure BuildPassed
    "failed" -> pure BuildFailed
    "interrupted" -> pure BuildInterrupted
    "resource-failure" -> pure BuildResourceFailure
    "unknown" -> pure BuildUnknown
    _ -> fail "unrecognized host-build verdict"

newtype HostBuildReport = HostBuildReport HostBuildVerdict

instance FromJSON HostBuildReport where
  parseJSON = withObject "host-build report" $ \value -> HostBuildReport <$> value .: "verdict"

newtype VerdictLine = VerdictLine B.ByteString

data BuildPhase = CompileTargets | CheckTargets

buildCommand :: BuildPhase -> String
buildCommand CompileTargets = "build"
buildCommand CheckTargets = "test"

data SourceProvenance = SourceProvenance
  { sourceHead :: CommitId
  , sourceBranch :: String
  , sourceDirty :: Bool
  } deriving (Eq, Show)

data InputIdentity = InputIdentity
  { inputRunDocument, inputTrajectories, inputFirmware, inputAdjustment :: Sha256
  , inputArchive, inputRetired, inputRom, inputTables, inputPlan, inputClock :: Sha256
  , inputPairs :: Int
  } deriving (Eq)

data ToolIdentity = ToolIdentity
  { toolViewer, toolPairCheck, toolSettingsCompare, toolRenderCompare :: Sha256
  , toolSdkDriver, toolSdkRamPush :: Sha256
  , toolSdkStore :: FilePath
  } deriving (Eq)

data RoleIdentity = RoleIdentity
  { roleSource :: CommitId
  , roleCase :: CaseId
  , roleRole :: Role
  , roleTrajectory :: Sha256
  , roleInputs :: InputIdentity
  , roleTools :: ToolIdentity
  } deriving (Eq)

data Completion = Completion
  { completionIdentity :: Sha256
  , completionResult :: Sha256
  , completionExit :: Int
  , completionOomKills :: Integer
  } deriving (Eq)

data Failure = Failure Predicate String FilePath

data StorageRefusal = StorageRefusal
  { refusalDirectory :: FilePath
  , refusalAvailableBytes :: Integer
  , refusalRequiredBytes :: Integer
  } deriving (Eq, Show)

-- | Recognizes the diagnostic rendered for a storage reserve refusal.
parseStorageRefusal :: String -> Maybe StorageRefusal
parseStorageRefusal input = do
  line <- listToMaybe [candidate | candidate <- lines input, "Storage admission refused at " `isInfixOf` candidate]
  rest <- after "Storage admission refused at " line
  let (directory, afterDirectory) = breakOn ": " rest
  afterColon <- stripPrefix ": " afterDirectory
  (available, afterAvailable) <- decimal afterColon
  afterUnits <- stripPrefix " bytes available, " afterAvailable
  (required, afterRequired) <- decimal afterUnits
  _ <- stripPrefix " bytes required by XM1_STORAGE_MIN_FREE." afterRequired
  pure (StorageRefusal directory available required)
 where
  after needle haystack = case breakOn needle haystack of
    (_, rest) | Just suffix <- stripPrefix needle rest -> Just suffix
    _ -> Nothing
  breakOn needle haystack =
    case [index | (index, rest) <- zip [0 ..] (tails haystack), needle `isPrefixOf` rest] of
      index : _ -> splitAt index haystack
      [] -> (haystack, "")
  decimal text = case span isDigit text of
    (digits@(_ : _), rest) | Just value <- readMaybe digits -> Just (value, rest)
    _ -> Nothing

newtype MemoryBytes = MemoryBytes Integer deriving (Eq, Ord, Show)
newtype OomCount = OomCount Integer deriving (Eq, Show)

data SlotMemory = SlotMemory
  { slotPeak :: MemoryBytes
  , slotOomKills :: OomCount
  , slotOomEvents :: OomCount
  }

parseSlotMemory :: Value -> Either String SlotMemory
parseSlotMemory value = do
  unless (get value "memory_scope_kind" == String "emulator-slot")
    (Left "completion has no emulator-slot memory evidence")
  limit <- strictNum value "memory_max_bytes"
  swap <- strictNum value "swap_max_bytes"
  peak <- strictNum value "scope_peak_after"
  kills <- strictNum value "oom_kill_delta"
  events <- strictNum value "oom_event_delta"
  unless (limit == memoryMaxBytes && swap == 0)
    (Left "completion memory or swap limit differs")
  unless (peak > 0 && peak < limit)
    (Left "completion has no measured memory headroom")
  unless (kills == 0 && events == 0)
    (Left "completion records an OOM event")
  pure (SlotMemory (MemoryBytes peak) (OomCount kills) (OomCount events))

slotMemoryJson :: SlotMemory -> Value
slotMemoryJson memory =
  let MemoryBytes peak = slotPeak memory
      OomCount kills = slotOomKills memory
      OomCount events = slotOomEvents memory
  in object
    [ "scope_kind" .= ("emulator-slot" :: String)
    , "memory_max_bytes" .= memoryMaxBytes
    , "swap_max_bytes" .= (0 :: Integer)
    , "scope_peak_bytes" .= peak
    , "headroom_bytes" .= (memoryMaxBytes - peak)
    , "oom_kill_delta" .= kills
    , "oom_event_delta" .= events
    ]

data Gate = Gate
  { gateCaseId :: CaseId
  , gateFinished :: Bool
  , gateOomKills :: Integer
  } deriving (Eq, Show)

parseGate :: Value -> Either String Gate
parseGate value = Right (Gate
  { gateCaseId = CaseId (str (get value "case"))
  , gateFinished = get value "complete" == Bool True
  , gateOomKills = num (get value "oom_kill_delta")
  })

gateJson :: Gate -> Value
gateJson gate = object
  [ "case" .= caseText (gateCaseId gate)
  , "complete" .= gateFinished gate
  , "oom_kill_delta" .= gateOomKills gate
  ]

roleText :: Role -> String
roleText Stock = "stock"
roleText Owned = "owned"

caseText :: CaseId -> String
caseText (CaseId value) = value

predicateText :: Predicate -> String
predicateText (Predicate value) = value

gateCase :: CaseId
gateCase = CaseId "oracle-framed-push"

gateRole :: Role
gateRole = Stock

pairs :: [CaseId]
pairs =
  [ CaseId "oracle-close", CaseId "oracle-close-property", CaseId "oracle-fixed-wrap", CaseId "oracle-fn", CaseId "oracle-fn-auto-open"
  , CaseId "oracle-fn-down", CaseId "oracle-frame-fixed-open", CaseId "oracle-frame-pop-down", CaseId "oracle-frame-pop-up"
  , CaseId "oracle-frame-push-probe", CaseId "oracle-frame-push-up-probe", CaseId "oracle-frame-seek-down"
  , CaseId "oracle-frame-seek-step", CaseId "oracle-frame-seek-up", CaseId "oracle-framed-open"
  , CaseId "oracle-framed-open-restored", CaseId "oracle-framed-pop", CaseId "oracle-framed-pop-back"
  , CaseId "oracle-framed-pop-group41", CaseId "oracle-framed-push", CaseId "oracle-group-down", CaseId "oracle-group-down-wrap"
  , CaseId "oracle-group-left", CaseId "oracle-group-left-wrap", CaseId "oracle-group-right", CaseId "oracle-group-right-wrap"
  , CaseId "oracle-group-up", CaseId "oracle-group-up-wrap", CaseId "oracle-nowrap", CaseId "oracle-nowrap-edge"
  , CaseId "oracle-open", CaseId "oracle-open-seek", CaseId "oracle-pop", CaseId "oracle-pop-deep", CaseId "oracle-pop-remember"
  , CaseId "oracle-pop-remember-off", CaseId "oracle-push", CaseId "oracle-push-classifier", CaseId "oracle-push-remember-off"
  , CaseId "oracle-push-restored", CaseId "oracle-push-same-parent", CaseId "oracle-reopen", CaseId "oracle-scroll"
  , CaseId "oracle-scroll-down", CaseId "oracle-scroll-up", CaseId "oracle-step-rows", CaseId "oracle-step-rows-wrap", CaseId "oracle-wrap"
  , CaseId "picker-15", CaseId "picker-163", CaseId "picker-211", CaseId "picker-214", CaseId "picker-239", CaseId "picker-250", CaseId "picker-26"
  , CaseId "picker-260", CaseId "picker-270", CaseId "picker-271", CaseId "picker-272", CaseId "picker-273", CaseId "picker-297", CaseId "picker-312"
  , CaseId "picker-314", CaseId "picker-320", CaseId "picker-36", CaseId "picker-450", CaseId "picker-488"
  ]

sha :: String -> Sha256
sha = either error id . parseSha256Hex

expectedRunDocument, expectedTrajectories, expectedFirmware, expectedAdjustment :: Sha256
expectedRunDocument = sha "2184474fa40c2f8ad86e963ceb0898c5776ec597332805ee1365748de4df04e8"
expectedTrajectories = sha "fd9cd0f07092e2af7468c20ae83af4d08b7cc58b75654281d944f36c8d7a19d7"
expectedFirmware = sha "acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8"
expectedAdjustment = sha "baf475358f1b8ec5b619fd4891cd32a1c8225c0d9341e60318031106a4354aa0"

expectedArchive, expectedRetired, expectedTables, expectedPlan, expectedClock, expectedRom :: Sha256
expectedArchive = sha "b0ce0d00be44695b4b162ea6da0cb0a419618ab5524b5db99f63ba0e9a045d88"
expectedRetired = sha "5b289e1796ff1653abd1d9e50169c71d4e63bb61d65677d36345b442993172f6"
expectedTables = sha "7fa536b6f4ab72205be57f407ce7224fa0bbc397aca2061ad2fac3e14ba2021d"
expectedPlan = sha "fcfe3ec56b17f293acede09257ac967cd7f98d32d100ce540e4b17e85c735ceb"
expectedClock = sha "f9ee8573fe6d1c16d872369290d48735172174541215d2dda5ea5d17233012a1"
expectedRom = sha "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"

memoryMaxBytes :: Integer
memoryMaxBytes = 2147483648

qualRoot, binRoot :: FilePath
qualRoot = "private-data/scratch/ControllerStrategy/engine225/bounded-qual"
binRoot = qualRoot </> "bin"

trajectoryRoot, runDocument, inputsRoot, tablesPath, planPath, clockPath :: FilePath
trajectoryRoot = "private-data/scratch/ControllerStrategy/engine225/controlled-rtc/trajectories"
runDocument = "private-data/scratch/ControllerStrategy/engine225/controlled-rtc/xm1-150.json"
inputsRoot = "private-data/scratch/ControllerStrategy/engine225/final-2db1e64a/inputs"
tablesPath = "private-data/scratch/ControllerStrategy/menu-tables.json"
planPath = "private-data/scratch/ControllerStrategy/menu-engine/bank-plan.json"
clockPath = "emulation/menu-engine-pair-trajectories/clock-exclusions.json"

main :: IO ()
main = do
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  atRoot <- doesFileExist "emulation/BUILD.bazel"
  unless atRoot (failH "run from the repository root")
  arguments <- getArgs
  case arguments of
    ["plan"] -> planMode FullMatrix
    ["plan", "headroom"] -> planMode HeadroomOnly
    ["execute"] -> executeMode FullMatrix
    ["headroom"] -> executeMode HeadroomOnly
    ["execute-slotted"] -> slottedMode FullMatrix
    ["headroom-slotted"] -> slottedMode HeadroomOnly
    _ -> failH "usage: menu-engine-qualify-bounded plan [headroom]|execute|headroom"

planMode :: QualificationScope -> IO ()
planMode scope = do
  requireCleanSource
  identity <- verifySource
  verifyInputs
  printJson $ object
    [ "schema" .= (1 :: Int)
    , "status" .= ("planned-not-executed" :: String)
    , "qualification_scope" .= scopeText scope
    , "source" .= sourceJson identity
    , "inputs" .= inputJson inputIdentity
    , "resources" .= resourcesValue
    , "gate" .= (caseText gateCase ++ "." ++ roleText gateRole)
    , "reuse" .= object
        [ "old_matrix_eligible" .= False
        , "reason" .= ("the prior matrix binds run document efa32ff8 and trajectory inventory 5efb00f" :: String) ]
    , "command" .= ("menu-engine-qualify-bounded " ++ case scope of
        HeadroomOnly -> "headroom"
        FullMatrix -> "execute")
    , "execution" .= ("host-build queues builds automatically; execution takes an emulator slot after the build" :: String)
    ]

executeMode :: QualificationScope -> IO ()
executeMode scope = do
  occupied <- insideSlot
  when occupied (failH "qualification preparation must start outside an emulator slot")
  requireCleanSource
  verifyInputs
  policy <- slotPolicyFromEnvironment
  let SlotCount slots = slotCount policy
  unless (slots == 1 && Admission.memoryMaxBytes (memoryMax policy) == memoryMaxBytes)
    (failH "qualification requires XM1_EMULATOR_SLOTS=1 and a 2G emulator cap")
  checkStorageBudget qualRoot
  buildAndTest
  reexecInSlot policy [slottedArgument scope]
  failH "qualification did not enter a fresh emulator slot"

slottedMode :: QualificationScope -> IO ()
slottedMode scope = do
  occupied <- insideSlot
  unless occupied (failH "slotted qualification requires an emulator slot")
  readBoundedScope
  requireCleanSource
  verifyInputs
  source <- verifySource
  outcome <- runGate source inputIdentity
  case scope of
    HeadroomOnly -> publishHeadroom outcome source inputIdentity
    FullMatrix -> do
      matrix <- runMatrix source inputIdentity
      publish matrix source inputIdentity

verifySource :: IO SourceProvenance
verifySource = do
  dirtyText <- readProcess "git" ["status", "--porcelain"] ""
  branch <- trim <$> readProcess "git" ["rev-parse", "--abbrev-ref", "HEAD"] ""
  commit <- trim <$> readProcess "git" ["rev-parse", "HEAD"] ""
  pure SourceProvenance
    { sourceHead = CommitId commit
    , sourceBranch = branch
    , sourceDirty = not (null dirtyText)
    }

requireCleanSource :: IO ()
requireCleanSource = do
  dirty <- readProcess "git" ["status", "--porcelain"] ""
  unless (null (trim dirty)) (failH "source tree is dirty; commit the frozen source before qualification")

inputIdentity :: InputIdentity
inputIdentity = InputIdentity
  { inputRunDocument = expectedRunDocument
  , inputTrajectories = expectedTrajectories
  , inputFirmware = expectedFirmware
  , inputAdjustment = expectedAdjustment
  , inputArchive = expectedArchive
  , inputRetired = expectedRetired
  , inputRom = expectedRom
  , inputTables = expectedTables
  , inputPlan = expectedPlan
  , inputClock = expectedClock
  , inputPairs = length pairs
  }

sourceJson :: SourceProvenance -> Value
sourceJson source = object
  [ "head" .= commitText (sourceHead source)
  , "short" .= take 8 (commitText (sourceHead source))
  , "branch" .= sourceBranch source
  , "dirty" .= sourceDirty source
  ]

inputJson :: InputIdentity -> Value
inputJson inputs = object
  [ "run_document" .= sha256Hex (inputRunDocument inputs)
  , "trajectory_inventory" .= sha256Hex (inputTrajectories inputs)
  , "firmware" .= sha256Hex (inputFirmware inputs)
  , "adjustment_nand" .= sha256Hex (inputAdjustment inputs)
  , "owned_archive" .= sha256Hex (inputArchive inputs)
  , "retired_picker" .= sha256Hex (inputRetired inputs)
  , "boot_rom" .= sha256Hex (inputRom inputs)
  , "menu_tables" .= sha256Hex (inputTables inputs)
  , "bank_plan" .= sha256Hex (inputPlan inputs)
  , "clock_exclusions" .= sha256Hex (inputClock inputs)
  , "pairs" .= inputPairs inputs
  , "roles" .= (2 * inputPairs inputs)
  ]

resourcesValue :: Value
resourcesValue = object
  [ "emulator_memory_max_bytes" .= memoryMaxBytes
  , "emulator_swap_max_bytes" .= (0 :: Integer)
  , "build_memory_max_bytes" .= Admission.memoryMaxBytes (Admission.classMemoryMax Admission.Build)
  , "emulator_slots" .= (1 :: Int)
  , "bazel_jobs" .= (2 :: Int)
  , "native_batches" .= (0 :: Int)
  ]

verifyInputs :: IO ()
verifyInputs = do
  forM_
    [ ("run document", runDocument, expectedRunDocument)
    , ("firmware", inputsRoot </> "firmware.DAT", expectedFirmware)
    , ("adjustment NAND", inputsRoot </> "adjustment.nand", expectedAdjustment)
    , ("owned archive", inputsRoot </> "engine.a", expectedArchive)
    , ("retired picker", inputsRoot </> "retired.json", expectedRetired)
    , ("menu tables", tablesPath, expectedTables)
    , ("bank plan", planPath, expectedPlan)
    , ("clock exclusions", clockPath, expectedClock)
    , ("boot ROM", "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin", expectedRom)
    ] $ \(label, path, expected) -> requireDigest label path expected
  files <- sort . filter (".json" `isSuffixOf`) <$> listDirectory trajectoryRoot
  unless (length files == 134) (failH ("expected 134 trajectories, found " ++ show (length files)))
  let names = sort . nub $ map pairName files
  unless (names == sort pairs) (failH "trajectory pair names differ from the required 67")
  forM_ pairs $ \name -> forM_ [Stock, Owned] $ \role ->
    unless (trajectoryPath name role `elem` map (trajectoryRoot </>) files)
      (failH ("pair " ++ caseText name ++ " lacks its " ++ roleText role ++ " trajectory"))
  rows <- forM files $ \file -> do
    digest <- sha256File (trajectoryRoot </> file)
    pure (object ["file" .= file, "sha256" .= sha256Hex digest])
  let manifest = sha256 (encodeCanonical (toJSON rows))
  unless (manifest == expectedTrajectories)
    (failH ("trajectory inventory " ++ sha256Hex manifest ++ " differs"))
 where
  pairName file =
    let stem = fromMaybe file (dropSuffix ".json" file)
    in CaseId (fromMaybe stem (dropSuffix ".stock" stem `orElse` dropSuffix ".owned" stem))
  orElse (Just value) _ = Just value
  orElse Nothing other = other
  dropSuffix suffix value
    | suffix `isSuffixOf` value = Just (take (length value - length suffix) value)
    | otherwise = Nothing

requireDigest :: String -> FilePath -> Sha256 -> IO ()
requireDigest label path expected = do
  present <- doesFileExist path
  unless present (failH (label ++ " missing: " ++ path))
  actual <- sha256File path
  unless (actual == expected)
    (failH (label ++ " digest " ++ sha256Hex actual ++ " differs from " ++ sha256Hex expected))

buildAndTest :: IO ()
buildAndTest = do
  createDirectoryIfMissing True (qualRoot </> "logs")
  configured <- lookupEnv "MENU_BOUNDED_QUAL_OUTPUT_BASE"
  let baseArguments = ["--output_base=" ++ path | Just path <- [configured]]
      buildTargets =
        [ "//emulation:xm1-viewer", "//emulation:cardless-ram-push-bridge"
        , "//emulation:fujimod-paths-module"
        , "//emulation:modal-settings-compare", "//emulation:render-compare"
        , "//replacement/modal_runtime:menu-engine-pair-check"
        , "//replacement/modal_runtime:menu-engine-tree" ]
      testTargets =
        [ "//emulation:modal-watch-stream-tests", "//emulation:modal-observation-tests"
        , "//emulation:xm1-viewer-tests", "//emulation:ui-trajectory-tests"
        , "//emulation:render-comparison-tests"
        , "//emulation:menu-crawl-tests", "//emulation:observation-compare-tests"
        , "//replacement/modal_runtime:render-pair-tests"
        , "//replacement/modal_runtime:menu-engine-pair-tests"
        , "//replacement/modal_runtime:menu-engine-generator-tests" ]
  buildVerdict <- runHostBuild baseArguments CompileTargets buildTargets (qualRoot </> "logs" </> "build.log")
  testVerdict <- runHostBuild baseArguments CheckTargets testTargets (qualRoot </> "logs" </> "host-tests.log")
  source <- verifySource
  writeJson (qualRoot </> "host-build-verdicts.json") $ object
    [ "source" .= sourceJson source
    , "verdict_lines" .= [C.unpack line | VerdictLine line <- [buildVerdict, testVerdict]]
    ]
  checkStorageBudget qualRoot
  freezeTools

runHostBuild :: [String] -> BuildPhase -> [String] -> FilePath -> IO VerdictLine
runHostBuild baseArguments phase targets logPath = do
  let arguments = ["shell", "--", "host-build"] ++ baseArguments
        ++ [buildCommand phase, "--jobs=2", "--local_test_jobs=1"]
        ++ ["--test_output=errors" | CheckTargets <- [phase]]
        ++ targets
      blocked predicate detail code = do
        writeFailurePacket "blocked" (Failure (Predicate predicate) detail logPath)
        exitWith (ExitFailure code)
  status <- spawn "devenv" arguments logPath []
  size <- getFileSize logPath
  bytes <- withBinaryFile logPath ReadMode $ \handle -> do
    hSeek handle AbsoluteSeek (max 0 (size - 16384))
    B.hGet handle 16384
  let verdictLines = [line | line <- reverse (C.lines bytes), "host-build verdict " `B.isPrefixOf` line]
  forM_ (take 1 verdictLines) (C.hPutStrLn stderr)
  when (status == ExitFailure 75)
    (blocked "host-resource-limit" "host-build reached a host memory limit" 75)
  report <- case verdictLines of
    line : _ -> case eitherDecodeStrict' (B.drop (B.length "host-build verdict ") line) of
      Left problem -> blocked "host-build-verdict" problem 1
      Right decoded -> pure (line, decoded)
    [] -> blocked "host-build-verdict" "host-build produced no final verdict" 1
  case (status, report) of
    (ExitSuccess, (line, HostBuildReport BuildPassed)) -> pure (VerdictLine line)
    (_, (_, HostBuildReport BuildResourceFailure)) -> blocked "host-resource-limit" "host-build reached a host memory limit" 75
    (_, (_, HostBuildReport BuildUnknown)) -> blocked "host-unit-status" "host-build unit status could not be read" 1
    (_, (_, HostBuildReport BuildInterrupted)) -> blocked "host-build-interrupted" "host-build was interrupted" 8
    _ -> blocked "host-build-failed" "host-build did not pass" (max 1 (exitCodeValue status))

freezeTools :: IO ()
freezeTools = do
  exists <- doesDirectoryExist binRoot
  when exists $ do
    _ <- rawSystem "chmod" ["-R", "u+w", binRoot]
    removePathForcibly binRoot
  createDirectoryIfMissing True binRoot
  forM_
    [ "bazel-bin/emulation/xm1-viewer"
    , "bazel-bin/emulation/xm1-viewer.runfiles"
    , "bazel-bin/emulation/modal-settings-compare"
    , "bazel-bin/emulation/render-compare"
    , "bazel-bin/replacement/modal_runtime/menu-engine-pair-check"
    ] $ \source -> copyPath source binRoot
  moduleText <- readFile "bazel-bin/emulation/FujiModPaths.hs"
  driver <- sdkPath "fujimodDriver" moduleText
  ramPush <- sdkPath "fujimodRamPushBridge" moduleText
  driverDigest <- sha256File driver
  ramPushDigest <- sha256File ramPush
  writeJson (binRoot </> "sdk.json") $ object
    [ "store" .= takeDirectory (takeDirectory driver)
    , "driver" .= driver
    , "ram_push" .= ramPush
    , "driver_sha256" .= sha256Hex driverDigest
    , "ram_push_sha256" .= sha256Hex ramPushDigest
    ]

sdkPath :: String -> String -> IO FilePath
sdkPath name text =
  case [path | line <- lines text, Just rest <- [stripPrefix (name ++ " = \"") (dropWhile (== ' ') line)]
             , let path = takeWhile (/= '"') rest, not (null path)] of
    path : _ -> pure path
    [] -> failH ("generated FujiModPaths.hs lacks " ++ name)

readBoundedScope :: IO ()
readBoundedScope = do
  directory <- cgroupDirectory
  limit <- readInteger (directory </> "memory.max")
  swap <- readInteger (directory </> "memory.swap.max")
  unless (limit == memoryMaxBytes) (failH ("memory.max is " ++ show limit))
  unless (swap == 0) (failH ("memory.swap.max is " ++ show swap))
  createDirectoryIfMissing True qualRoot
  peak <- memPeak directory
  kills <- memOomKills directory
  events <- memOomEvents directory
  writeJson (qualRoot </> "resources-baseline.json") $ object
    [ "memory_max" .= limit
    , "swap_max" .= swap
    , "emulator_slots" .= (1 :: Int)
    , "bazel_jobs" .= (2 :: Int)
    , "baseline_peak" .= peak
    , "baseline_oom_kills" .= kills
    , "baseline_oom_events" .= events
    , "cgroup" .= directory
    ]

cgroupDirectory :: IO FilePath
cgroupDirectory = do
  text <- readFile "/proc/self/cgroup"
  case lines text of
    line : _ -> pure ("/sys/fs/cgroup" ++ dropWhile (/= '/') line)
    [] -> failH "cgroup membership is unavailable"

readInteger :: FilePath -> IO Integer
readInteger path = read <$> trim <$> readFile path

memPeak :: FilePath -> IO Integer
memPeak directory = readInteger (directory </> "memory.peak")

memOomKills, memOomEvents :: FilePath -> IO Integer
memOomKills = memoryEvent "oom_kill"
memOomEvents = memoryEvent "oom"

memoryEvent :: String -> FilePath -> IO Integer
memoryEvent name directory = do
  text <- readFile (directory </> "memory.events")
  pure $ sum [read value | line <- lines text, [key, value] <- [words line], key == name]

runGate :: SourceProvenance -> InputIdentity -> IO RunOutcome
runGate source inputs = do
  result <- runRole gateCase gateRole
  outcome <- case result of
    Left failure -> do
      writeFailurePacket "blocked" failure
      failH "gate role failed"
    Right completed -> pure completed
  let directory = roleDirectory gateCase gateRole
      completionPath = directory </> "completion.json"
      watch = directory </> "receipt" </> "e2e-modal-watch-final.json"
  completionBytes <- B.readFile completionPath
  memory <- case eitherDecodeStrict' completionBytes >>= parseSlotMemory of
    Left problem -> do
      writeFailurePacket "blocked" (Failure (Predicate "memory-headroom") problem completionPath)
      failH problem
    Right observed -> pure observed
  present <- doesFileExist watch
  unless present $ do
    writeFailurePacket "blocked" (Failure (Predicate "known-worst-watch") "gate role wrote no terminal watch" directory)
    failH "gate role did not produce its terminal watch"
  size <- getFileSize watch
  digest <- sha256Hex <$> sha256File watch
  rows <- gateWatchRows
  let OomCount kills = slotOomKills memory
  writeJson (qualRoot </> "gate.json") $ object
    [ "case" .= caseText gateCase
    , "role" .= roleText gateRole
    , "complete" .= True
    , "source" .= sourceJson source
    , "inputs" .= inputJson inputs
    , "oom_kill_delta" .= kills
    , "memory" .= slotMemoryJson memory
    , "completion_sha256" .= sha256Hex (sha256 completionBytes)
    , "final_watch_bytes" .= size
    , "final_watch_rows" .= rows
    , "final_watch_sha256" .= digest
    , "directory" .= directory
    ]
  pure outcome

publishHeadroom :: RunOutcome -> SourceProvenance -> InputIdentity -> IO ()
publishHeadroom outcome source inputs = do
  gateValue <- decodeFile (qualRoot </> "gate.json") >>= either failH pure
  tools <- toolIdentity
  let directory = qualRoot </> "headroom"
      terminalPath = directory </> "terminal.json"
      reviewPath = directory </> "review-packet.json"
      executed = case outcome of Executed -> 1 :: Int; Reused -> 0
      reused = case outcome of Executed -> 0 :: Int; Reused -> 1
      terminal = object
        [ "schema" .= (1 :: Int)
        , "passed" .= True
        , "qualification" .= scopeText HeadroomOnly
        , "source" .= sourceJson source
        , "inputs" .= inputJson inputs
        , "tools" .= toolJson tools
        , "resources" .= resourcesValue
        , "gate" .= gateValue
        , "execution" .= object
            [ "executed_roles" .= executed
            , "reused_roles" .= reused
            , "matrix_roles_started" .= (0 :: Int)
            , "native_batches" .= (0 :: Int)
            ]
        , "first_failure" .= Null
        , "review_packet" .= reviewPath
        , "remaining_scope" .= object
            [ "full67" .= ("not run" :: String)
            , "subsystem_cutover" .= ("not qualified" :: String)
            , "hardware" .= ("not run" :: String)
            ]
        ]
  writeJson terminalPath terminal
  terminalDigest <- sha256File terminalPath
  writeJson reviewPath $ object
    [ "terminal" .= terminalPath
    , "terminal_sha256" .= sha256Hex terminalDigest
    , "review" .= object
        [ "checks_run_by_reviewer" .= False
        , "cutover_approved" .= False
        ]
    ]
  printJson $ object
    [ "passed" .= True
    , "qualification" .= scopeText HeadroomOnly
    , "executed_roles" .= executed
    , "reused_roles" .= reused
    , "review_packet" .= reviewPath
    ]

gateWatchRows :: IO Integer
gateWatchRows = do
  let path = roleDirectory gateCase gateRole </> "receipt" </> "e2e-modal-observation.json"
  present <- doesFileExist path
  unless present (failH "gate role wrote no observation receipt")
  value <- decodeFile path
  case value of
    Left problem -> failH ("gate role observation receipt is unreadable: " ++ problem)
    Right decoded -> do
      let rows = num (get (get decoded "watch_final_summary") "record_count")
      unless (rows > 0) (failH "gate role recorded no final watch rows")
      pure rows

runMatrix :: SourceProvenance -> InputIdentity -> IO Matrix
runMatrix _source _inputs = do
  executed <- newIORef (0 :: Int)
  reused <- newIORef (0 :: Int)
  comparisons <- newIORef ([] :: [(CaseId, FilePath)])
  forM_ pairs $ \name -> do
    forM_ [Stock, Owned] $ \role -> do
      outcome <- runRole name role
      case matrixGate outcome of
        Stop failure -> do
          writeFailurePacket "blocked" failure
          failH ("role failed: " ++ caseText name ++ "." ++ roleText role)
        Continue Reused -> modifyIORef' reused (+ 1)
        Continue Executed -> modifyIORef' executed (+ 1)
    complete <- roleComplete name Stock
    completeOwned <- roleComplete name Owned
    unless (complete && completeOwned) $ do
      writeFailurePacket "blocked" (Failure (Predicate "role-completion")
        ("role incomplete after execution: " ++ caseText name) (roleDirectory name Stock))
      failH ("role incomplete: " ++ caseText name)
    outcome <- runPairCheck name
    case outcome of
      Left failure -> do
        writeFailurePacket "blocked" failure
        failH ("pair comparison failed: " ++ caseText name)
      Right directory -> modifyIORef' comparisons ((name, directory) :)
  compared <- reverse <$> readIORef comparisons
  Matrix compared <$> readIORef executed <*> readIORef reused

data Matrix = Matrix
  { matrixPairs :: [(CaseId, FilePath)]
  , matrixExecuted :: Int
  , matrixReused :: Int
  }

data Step = Stop Failure | Continue RunOutcome

matrixGate :: Either Failure RunOutcome -> Step
matrixGate (Left failure) = Stop failure
matrixGate (Right outcome) = Continue outcome

data RunOutcome = Executed | Reused deriving (Eq, Show)

runRole :: CaseId -> Role -> IO (Either Failure RunOutcome)
runRole name role = do
  let directory = roleDirectory name role
  complete <- roleComplete name role
  if complete then pure (Right Reused) else do
    recorded <- doesFileExist (directory </> "completion.json")
    if recorded
      then pure (Left (Failure (Predicate "completed-observation-admission")
        "recorded completion failed current admission; automatic replay is refused"
        (directory </> "completion.json")))
      else do
        terminal <- roleTerminalFailure name role
        case terminal of
          Just failure -> pure (Left failure)
          Nothing -> executeRole name role directory

completionHolds :: Value -> Value -> Value -> Sha256 -> Sha256 -> Either String Value -> Bool
completionHolds current stored completionValue inputDigest resultDigest receiptValue =
  executionJson stored == executionJson current
  && either (const False) (\parsed ->
       completionIdentity parsed == inputDigest
       && completionResult parsed == resultDigest
       && completionExit parsed == 0
       && completionOomKills parsed == 0) (parseCompletion completionValue)
  && receiptInputsMatch receiptValue

roleComplete :: CaseId -> Role -> IO Bool
roleComplete name role = do
  let directory = roleDirectory name role
      inputPath = directory </> "input.json"
      completionPath = directory </> "completion.json"
      receipt = directory </> "receipt" </> "e2e-result.json"
  present <- andM [doesFileExist inputPath, doesFileExist completionPath, doesFileExist receipt]
  if not present then pure False else do
    current <- roleIdentity name role
    stored <- decodeFile inputPath
    completionValue <- decodeFile completionPath
    inputDigest <- sha256File inputPath
    resultDigest <- sha256File receipt
    receiptValue <- decodeFile receipt
    pure $ case (stored, completionValue) of
      (Right storedValue, Right completion) ->
        completionHolds (roleJson current) storedValue completion inputDigest resultDigest receiptValue
      _ -> False

strictNum :: Value -> String -> Either String Integer
strictNum value key = case fromJSON (get value key) of
  Success integer -> Right integer
  Error _ -> Left (key ++ " is missing or not an integer")

parseCompletion :: Value -> Either String Completion
parseCompletion value = do
  identity <- parseSha256Hex (str (get value "identity_sha256"))
  result <- parseSha256Hex (str (get value "result_sha256"))
  exit <- strictNum value "exit"
  oom <- strictNum value "oom_kill_delta"
  pure Completion
    { completionIdentity = identity
    , completionResult = result
    , completionExit = fromIntegral exit
    , completionOomKills = oom
    }

parseFailure :: Value -> Either String (Sha256, Failure)
parseFailure value = do
  identity <- parseSha256Hex (str (get value "identity_sha256"))
  pure ( identity
       , Failure (Predicate (str (get value "predicate")))
                 (str (get value "reason"))
                 (str (get value "viewer_log")))

receiptInputsMatch :: Either String Value -> Bool
receiptInputsMatch (Right value) =
  let inputs = get (get value "emulator_start") "inputs"
      parsed field = parseSha256Hex (str (get inputs field))
  in num (get inputs "schema") == 2
     && parsed "run_document" == Right expectedRunDocument
     && parsed "installed_dat" == Right expectedFirmware
     && parsed "adjustment_nand" == Right expectedAdjustment
     && parsed "boot_rom" == Right expectedRom
     && get value "trajectory_failure" == Null
receiptInputsMatch _ = False

terminalFailureFrom :: Value -> Maybe Value -> Maybe Failure
terminalFailureFrom current storedFailure = case storedFailure of
  Just value
    | Right (_, parsed) <- parseFailure value
    , get value "execution" == executionJson current -> Just parsed
  _ -> Nothing

roleTerminalFailure :: CaseId -> Role -> IO (Maybe Failure)
roleTerminalFailure name role = do
  let failurePath = roleDirectory name role </> "failure.json"
  present <- doesFileExist failurePath
  if not present then pure Nothing else do
    failure <- decodeFile failurePath
    current <- roleIdentity name role
    case failure of
      Right value -> do
        let terminal = terminalFailureFrom (roleJson current) (Just value)
        case terminal of
          Nothing -> pure Nothing
          Just _ -> do
            refusal <- storageRefusalAt (str (get value "viewer_log"))
            pure $ case refusal of
              Just _ -> Nothing
              Nothing -> terminal
      Left _ -> pure Nothing

executeRole :: CaseId -> Role -> FilePath -> IO (Either Failure RunOutcome)
executeRole name role directory = do
  archiveStale directory (caseText name ++ "-" ++ roleText role)
  createDirectoryIfMissing True directory
  identity <- roleIdentity name role
  writeJson (directory </> "input.json") (roleJson identity)
  cgroup <- cgroupDirectory
  beforeKills <- memOomKills cgroup
  beforeEvents <- memOomEvents cgroup
  beforePeak <- memPeak cgroup
  status <- runViewer name role (directory </> "viewer.log")
  afterKills <- memOomKills cgroup
  afterEvents <- memOomEvents cgroup
  let killDelta = afterKills - beforeKills
      eventDelta = afterEvents - beforeEvents
      receipt = directory </> "receipt" </> "e2e-result.json"
  receiptValue <- decodeFile receipt
  let receiptOk = receiptInputsMatch receiptValue
      receiptProblem = either Just (const Nothing) receiptValue
  if status == ExitSuccess && receiptOk && killDelta == 0 && eventDelta == 0 then do
    resultDigest <- sha256Hex <$> sha256File receipt
    inputDigest <- sha256Hex <$> sha256File (directory </> "input.json")
    completedAt <- show <$> getProcessID
    afterPeak <- memPeak cgroup
    writeJson (directory </> "completion.json") $ object
      [ "identity_sha256" .= inputDigest
      , "result_sha256" .= resultDigest
      , "exit" .= (0 :: Int)
      , "oom_kill_delta" .= killDelta
      , "oom_event_delta" .= eventDelta
      , "memory_scope_kind" .= ("emulator-slot" :: String)
      , "memory_scope" .= cgroup
      , "memory_max_bytes" .= memoryMaxBytes
      , "swap_max_bytes" .= (0 :: Integer)
      , "scope_peak_before" .= beforePeak
      , "scope_peak_after" .= afterPeak
      , "completed_at" .= completedAt
      ]
    failurePresent <- doesFileExist (directory </> "failure.json")
    when failurePresent (removeFile (directory </> "failure.json"))
    pure (Right Executed)
  else do
    let pressure = killDelta > 0 || eventDelta > 0
        interrupted = not pressure && status `elem` [ExitFailure 124, ExitFailure 143]
        abnormal = not pressure && status == ExitFailure 137
    if interrupted
      then pure (Left (Failure (Predicate "role-completion") "interrupted" (directory </> "viewer.log")))
      else do
        refusal <- if pressure || abnormal then pure Nothing else storageRefusalAt (directory </> "viewer.log")
        case refusal of
          Just refused -> do
            inputDigest <- sha256Hex <$> sha256File (directory </> "input.json")
            writeJson (directory </> "storage-refusal.json") $ object
              [ "identity_sha256" .= inputDigest
              , "exit" .= exitCodeValue status
              , "directory" .= refusalDirectory refused
              , "available_bytes" .= refusalAvailableBytes refused
              , "required_bytes" .= refusalRequiredBytes refused
              , "execution" .= executionJson (roleJson identity)
              , "viewer_log" .= (directory </> "viewer.log")
              ]
            pure (Left (Failure (Predicate "storage-admission")
              ("storage admission refused: " ++ show (refusalAvailableBytes refused)
                ++ " bytes available, " ++ show (refusalRequiredBytes refused) ++ " bytes required")
              (directory </> "viewer.log")))
          Nothing -> do
            let predicate = Predicate
                  (if pressure then "bounded-memory" else if abnormal then "abnormal-exit" else "role-completion")
                reason
                  | killDelta > 0 = "memcg-oom-kill"
                  | eventDelta > 0 = "memcg-oom-event"
                  | abnormal = "sigkill-without-oom-kill"
                  | otherwise = "viewer-failed"
            inputDigest <- sha256Hex <$> sha256File (directory </> "input.json")
            failurePeak <- memPeak cgroup
            writeJson (directory </> "failure.json") $ object
              [ "identity_sha256" .= inputDigest
              , "exit" .= exitCodeValue status
              , "oom_kill_delta" .= killDelta
              , "oom_event_delta" .= eventDelta
              , "reason" .= reason
              , "predicate" .= predicateText predicate
              , "execution" .= executionJson (roleJson identity)
              , "receipt_error" .= receiptProblem
              , "viewer_log" .= (directory </> "viewer.log")
              , "receipt" .= (directory </> "receipt")
              , "scope_peak" .= failurePeak
              , "complete" .= True
              ]
            pure (Left (Failure predicate reason (directory </> "viewer.log")))

runViewer :: CaseId -> Role -> FilePath -> IO ExitCode
runViewer name role logPath = do
  let directory = roleDirectory name role
      progress = runDocument
      extra = case role of
        Stock -> ["--modal-lifetime-guard"]
        Owned -> [ "--ram-push-modal-scoped"
                 , "--modal75-owned-archive", inputsRoot </> "engine.a"
                 , "--modal75-retired", inputsRoot </> "retired.json"
                 , "--menu-engine-owned" ]
      common = [ "--workspace", "."
               , "--ram-push-e2e", "--cardless", "--fujimod", "--unreviewed-relink"
               , "--fujimod-run-document", progress
               , "--firmware", inputsRoot </> "firmware.DAT"
               , "--installed-nand", inputsRoot </> "adjustment.nand"
               , "--trajectories", trajectoryPath name role
               , "--output", directory </> "runtime"
               , "--receipt-output", directory </> "receipt" ]
  spawn "devenv" (["shell", "--", binRoot </> "xm1-viewer"] ++ common ++ extra) logPath
    [("XM1_EMULATOR_SLOTS", "1"), ("XM1_EMULATOR_MEMORY_MAX", "2G")]

runPairCheck :: CaseId -> IO (Either Failure FilePath)
runPairCheck name = do
  let stock = roleDirectory name Stock </> "receipt"
      owned = roleDirectory name Owned </> "receipt"
  check <- freshDirectory (qualRoot </> "checks") (caseText name)
  pairStatus <- spawn (binRoot </> "menu-engine-pair-check")
    ([ "--retained-render", "--tables", tablesPath, "--package-plan", planPath
     , "--stock", stock, "--owned", owned ]) (check </> "pair-check.log") []
  settingsStatus <- spawn (binRoot </> "modal-settings-compare")
    [stock, owned, check </> "settings.json"] (check </> "settings.log") []
  let exclusion
        | caseText name `elem` ["oracle-frame-fixed-open", "oracle-fixed-wrap"] = ["--clock-exclusions", clockPath]
        | otherwise = []
  renderStatus <- spawn (binRoot </> "render-compare")
    ([ stock </> "e2e-result-render.json"
     , owned </> "e2e-result-render.json"
     , check </> "render.json" ] ++ exclusion) (check </> "render.log") []
  settingsEqual <- jsonEqual (check </> "settings.json")
  renderEqual <- jsonEqual (check </> "render.json")
  let pass = pairStatus == ExitSuccess && settingsStatus == ExitSuccess
             && renderStatus == ExitSuccess && settingsEqual && renderEqual
  writeJson (check </> "pair.json") $ object
    [ "name" .= caseText name
    , "pair_status" .= exitCodeValue pairStatus
    , "settings_status" .= exitCodeValue settingsStatus
    , "render_status" .= exitCodeValue renderStatus
    , "settings_equal" .= settingsEqual
    , "render_equal" .= renderEqual
    , "directory" .= check
    , "pass" .= pass
    ]
  writeFile (qualRoot </> "checks" </> ("latest-" ++ caseText name)) check
  pure $ if pass
    then Right check
    else Left (Failure (Predicate "pair-comparison")
      ("pair comparison failed: " ++ caseText name) (check </> "pair.json"))

publish :: Matrix -> SourceProvenance -> InputIdentity -> IO ()
publish matrix source inputs = do
  roles <- fmap concat $ forM pairs $ \name -> forM [Stock, Owned] $ \role -> do
    complete <- roleComplete name role
    failed <- roleTerminalFailure name role
    let outcome
          | complete = "complete" :: String
          | Just _ <- failed = "failed"
          | otherwise = "partial"
    pure $ object
      [ "name" .= caseText name, "role" .= roleText role
      , "outcome" .= outcome, "directory" .= roleDirectory name role ]
  checks <- forM (matrixPairs matrix) $ \(name, directory) -> do
    value <- decodeFile (directory </> "pair.json")
    pure $ either
      (const (object ["name" .= caseText name, "pass" .= False, "reason" .= ("unreadable comparison" :: String)]))
      id value
  let completed = length [() | value <- roles, str (get value "outcome") == "complete"]
      failedRoles = length [() | value <- roles, str (get value "outcome") == "failed"]
      partialRoles = length [() | value <- roles, str (get value "outcome") == "partial"]
      passedPairs = length [() | value <- checks, get value "pass" == Bool True]
      failedPairs = length checks - passedPairs
      requiredPairs = length pairs
      allCompared = length checks == requiredPairs
  gateValue <- decodeFile (qualRoot </> "gate.json")
  tools <- toolIdentity
  let gateParsed = case gateValue of
        Right value -> parseGate value
        Left problem -> Left problem
      gateComplete = either (const False) (\gate -> gateFinished gate && gateOomKills gate == 0) gateParsed
      predicates :: [(String, Bool)]
      predicates =
        [ ("bounded-known-worst-gate", gateComplete)
        , ("all-required-roles-complete", completed == 2 * requiredPairs)
        , ("all-required-pairs-compared", allCompared)
        , ("all-required-pairs-pass", allCompared && failedPairs == 0)
        , ("no-role-failures", failedRoles == 0)
        , ("no-partial-roles", partialRoles == 0)
        ]
      qualified = all snd predicates
      status = if qualified then MatrixQualified else MatrixUnqualified
  terminal <- pure $ object
    [ "schema" .= (1 :: Int)
    , "status" .= matrixVerdictText status
    , "source" .= sourceJson source
    , "inputs" .= inputJson inputs
    , "tools" .= toolJson tools
    , "resources" .= resourcesValue
    , "gate" .= either (const Null) gateJson gateParsed
    , "execution" .= object
        [ "required_pairs" .= requiredPairs
        , "required_roles" .= (2 * requiredPairs)
        , "completed_roles" .= completed
        , "failed_roles" .= failedRoles
        , "partial_roles" .= partialRoles
        , "reused_roles" .= matrixReused matrix
        , "executed_roles" .= matrixExecuted matrix
        , "native_batches" .= (0 :: Int)
        , "reuse" .= ("only a role with equal execution identity, input and result is reused; comparisons rerun each invocation" :: String)
        ]
    , "predicates" .= [object ["name" .= name, "pass" .= holds] | (name, holds) <- predicates]
    , "pairs" .= checks
    , "roles" .= roles
    , "qualification_levels" .= object
        [ "host_checks" .= ("recorded build and host test logs" :: String)
        , "system_comparison" .= (if qualified then "complete final-input matrix" else "incomplete or failing final-input matrix" :: String)
        , "technical_review" .= ("prior source clearance only" :: String)
        , "hardware" .= ("none" :: String)
        ]
    , "failed_case" .= failedCase roles checks
    , "remaining_scope" .= object
        [ "image_size" .= ("responsive wrapping and the RestoreL cancel choice remain screen-specific" :: String)
        , "full67" .= (if qualified then "claimed by this matrix" else "not claimed" :: String)
        , "hardware" .= ("not claimed" :: String)
        ]
    ]
  writeJson (qualRoot </> "terminal.json") terminal
  writeJson (qualRoot </> "review-packet.json") $ object
    [ "terminal" .= terminal
    , "review" .= object
        [ "checks_run_by_reviewer" .= False
        , "cutover_approved" .= False
        ]
    ]
  printJson $ object
    [ "status" .= matrixVerdictText status
    , "passed_pairs" .= passedPairs
    , "required_pairs" .= requiredPairs
    , "failed_case" .= failedCase roles checks
    , "review_packet" .= (qualRoot </> "review-packet.json")
    ]
  unless qualified (exitWith (ExitFailure 1))

failedCase :: [Value] -> [Value] -> Value
failedCase roles checks =
  case [value | value <- roles, str (get value "outcome") == "failed"] of
    value : _ -> value
    [] -> case [value | value <- checks, get value "pass" == Bool False] of
      value : _ -> value
      [] -> Null

roleIdentity :: CaseId -> Role -> IO RoleIdentity
roleIdentity name role = do
  source <- verifySource
  trajectoryDigest <- sha256File (trajectoryPath name role)
  tools <- toolIdentity
  pure RoleIdentity
    { roleSource = sourceHead source
    , roleCase = name
    , roleRole = role
    , roleTrajectory = trajectoryDigest
    , roleInputs = inputIdentity
    , roleTools = tools
    }

roleJson :: RoleIdentity -> Value
roleJson identity = object
  [ "schema" .= (1 :: Int)
  , "name" .= caseText (roleCase identity)
  , "role" .= roleText (roleRole identity)
  , "source_head" .= commitText (roleSource identity)
  , "trajectory" .= object
      [ "path" .= trajectoryPath (roleCase identity) (roleRole identity)
      , "sha256" .= sha256Hex (roleTrajectory identity)
      ]
  , "inputs" .= inputJson (roleInputs identity)
  , "tools" .= toolJson (roleTools identity)
  ]

-- | The fields that determine a role's observations: the producing viewer and SDK, the
-- trajectory and the frozen inputs. Checker tools and source bookkeeping stay out, so a
-- checker-only change reuses completed observations and reruns the comparisons.
executionJson :: Value -> Value
executionJson value = object
  [ "viewer" .= get tools "viewer"
  , "sdk_store" .= get tools "sdk_store"
  , "sdk_driver" .= get tools "sdk_driver"
  , "sdk_ram_push" .= get tools "sdk_ram_push"
  , "inputs" .= get value "inputs"
  , "trajectory" .= get (get value "trajectory") "sha256"
  ]
 where tools = get value "tools"

commitText :: CommitId -> String
commitText (CommitId value) = value

toolIdentity :: IO ToolIdentity
toolIdentity = do
  viewer <- sha256File (binRoot </> "xm1-viewer")
  pairCheck <- sha256File (binRoot </> "menu-engine-pair-check")
  settingsCompare <- sha256File (binRoot </> "modal-settings-compare")
  renderCompare <- sha256File (binRoot </> "render-compare")
  sdk <- decodeFile (binRoot </> "sdk.json")
  case sdk of
    Left problem -> failH ("frozen SDK identity is unreadable: " ++ problem)
    Right value -> do
      driver <- either failH pure (parseSha256Hex (str (get value "driver_sha256")))
      ramPush <- either failH pure (parseSha256Hex (str (get value "ram_push_sha256")))
      pure ToolIdentity
        { toolViewer = viewer
        , toolPairCheck = pairCheck
        , toolSettingsCompare = settingsCompare
        , toolRenderCompare = renderCompare
        , toolSdkDriver = driver
        , toolSdkRamPush = ramPush
        , toolSdkStore = str (get value "store")
        }

toolJson :: ToolIdentity -> Value
toolJson tools = object
  [ "viewer" .= sha256Hex (toolViewer tools)
  , "pair_check" .= sha256Hex (toolPairCheck tools)
  , "settings_compare" .= sha256Hex (toolSettingsCompare tools)
  , "render_compare" .= sha256Hex (toolRenderCompare tools)
  , "sdk_store" .= toolSdkStore tools
  , "sdk_driver" .= sha256Hex (toolSdkDriver tools)
  , "sdk_ram_push" .= sha256Hex (toolSdkRamPush tools)
  ]

roleDirectory :: CaseId -> Role -> FilePath
roleDirectory name role = qualRoot </> "runs" </> caseText name </> roleText role

trajectoryPath :: CaseId -> Role -> FilePath
trajectoryPath name role = trajectoryRoot </> (caseText name ++ "." ++ roleText role ++ ".json")

archiveStale :: FilePath -> String -> IO ()
archiveStale directory label = do
  present <- doesDirectoryExist directory
  when present $ do
    createDirectoryIfMissing True (qualRoot </> "stale")
    processId <- getProcessID
    renameDirectory directory (qualRoot </> "stale" </> (label ++ "-" ++ show processId))

freshDirectory :: FilePath -> String -> IO FilePath
freshDirectory parent label = do
  createDirectoryIfMissing True parent
  processId <- getProcessID
  let directory = parent </> (label ++ "." ++ show processId)
  createDirectoryIfMissing True directory
  pure directory

writeFailurePacket :: String -> Failure -> IO ()
writeFailurePacket status (Failure predicate detail artifact) = do
  createDirectoryIfMissing True qualRoot
  source <- verifySource
  let value = object
        [ "schema" .= (1 :: Int)
        , "status" .= status
        , "qualification" .= ("not-qualified" :: String)
        , "source" .= sourceJson source
        , "inputs" .= inputJson inputIdentity
        , "resources" .= resourcesValue
        , "failed_case" .= object ["predicate" .= predicateText predicate, "detail" .= detail, "artifact" .= artifact]
        , "predicates" .= [object ["name" .= predicateText predicate, "pass" .= False, "detail" .= detail]]
        , "remaining_scope" .= object ["full67" .= ("not claimed" :: String)]
        ]
  writeJson (qualRoot </> "terminal.json") value
  writeJson (qualRoot </> "review-packet.json") $ object
    [ "terminal" .= value
    , "review" .= object ["checks_run_by_reviewer" .= False, "cutover_approved" .= False]
    ]

spawn :: FilePath -> [String] -> FilePath -> [(String, String)] -> IO ExitCode
spawn executable arguments logPath extraEnvironment = do
  environment <- getEnvironment
  createDirectoryIfMissing True (takeDirectory logPath)
  withFile logPath WriteMode $ \handle -> do
    (_, _, _, processHandle) <- createProcess ((proc executable arguments)
      { env = Just (extraEnvironment ++ environment)
      , std_in = NoStream
      , std_out = UseHandle handle
      , std_err = UseHandle handle
      })
    waitForProcess processHandle

jsonEqual :: FilePath -> IO Bool
jsonEqual path = do
  present <- doesFileExist path
  if not present then pure False else do
    value <- decodeFile path
    pure (either (const False) (\decoded -> get decoded "equal" == Bool True) value)

storageRefusalAt :: FilePath -> IO (Maybe StorageRefusal)
storageRefusalAt path = do
  bytes <- try (B.readFile path) :: IO (Either IOException B.ByteString)
  pure (either (const Nothing) (parseStorageRefusal . C.unpack) bytes)

decodeFile :: FilePath -> IO (Either String Value)
decodeFile path = do
  bytes <- try (B.readFile path) :: IO (Either IOException B.ByteString)
  pure (either (Left . displayException) eitherDecodeStrict' bytes)

writeJson :: FilePath -> Value -> IO ()
writeJson path value = do
  createDirectoryIfMissing True (takeDirectory path)
  B.writeFile (path ++ ".tmp") (encodeCanonical value <> "\n")
  renameFile (path ++ ".tmp") path

printJson :: Value -> IO ()
printJson value = B.putStr (encodeCanonical value <> "\n")

expectZero :: String -> ExitCode -> IO ()
expectZero label status = unless (status == ExitSuccess) (failH (label ++ " failed with " ++ show status))

andM :: [IO Bool] -> IO Bool
andM actions = and <$> sequence actions

exitCodeValue :: ExitCode -> Int
exitCodeValue ExitSuccess = 0
exitCodeValue (ExitFailure value) = value

trim :: String -> String
trim = reverse . dropWhile (`elem` (" \n\r\t" :: String)) . reverse

copyPath :: FilePath -> FilePath -> IO ()
copyPath source destination = do
  status <- rawSystem "cp" ["-a", source, destination]
  expectZero ("copy " ++ source) status
