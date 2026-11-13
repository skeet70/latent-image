{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), eitherDecode, encode, object, toJSON, withObject, withText, (.:), (.:?), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (isInfixOf, sortOn)
import qualified Data.Text as T
import Emulation.CandidateBaseline (Baseline (..), Field (..), FieldName (..), Segment (..), decodeBaseline, encodeBaseline)
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import qualified Data.Vector as V
import System.Directory (canonicalizePath, copyFile, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), makeRelative)
import System.IO (hClose, openTempFile)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

data Route = RamPushVolatile | UsbToggle deriving (Show)

routeNames :: Route -> (String, String)
routeNames route = case route of
  RamPushVolatile -> ("fujimod-ram-push-volatile", "ram-push-volatile")
  UsbToggle -> ("fujimod-usb-toggle-emu", "usb-toggle-emu")

data RecordedExit = MissingExit | NullExit | RecordedExit ExitCode deriving (Show)
data Qualification = QualifiedOutcome | MissingOutcome | ControlReceipt | UnknownOutcome | RefusedOutcome
  | SegmentedQualified | SegmentedMenuDiffers | SegmentedObservationDiffers | SegmentedWithControl | SegmentedFailed | SegmentedMalformed | SegmentedDuplicateName
  | SegmentedPathName | CounterpartControl | CounterpartFailedCommand
  | PassedWithMessage | FailedWithoutMessage | MissingMessage | NegativeSeconds | OverflowSeconds
  | ExtraKey | UnknownResult | SegmentedMissingChild | EmptySegments | FlatPlusSegmented deriving (Show)
data Side = CandidateSide | CounterpartSide deriving (Eq, Show)
data ReceiptSource = EarlierRun | ReceiptOverride deriving (Show)
data ReceiptParity = EqualReceipts | DifferentReceipts deriving (Show)
data Result = Passed | Failed deriving (Eq, Show)

instance FromJSON Result where
  parseJSON = withText "route result" $ \value -> case value of
    "pass" -> pure Passed
    "fail" -> pure Failed
    _ -> fail "unexpected route result"

newtype RouteOutcome = RouteOutcome Result
instance FromJSON RouteOutcome where
  parseJSON = withObject "route outcome" $ \value -> RouteOutcome <$> value .: "result"

newtype Report = Report [RouteOutcome]
instance FromJSON Report where
  parseJSON = withObject "verifier report" $ \value -> Report <$> value .: "routes"

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

withWorkspace :: (FilePath -> IO a) -> IO a
withWorkspace = bracket setup removeDirectoryRecursive
  where
    setup = do
      -- The verifier reports canonical paths; Darwin's /tmp and /var are symlinks into /private.
      temporary <- getTemporaryDirectory >>= canonicalizePath
      (root, handle) <- openTempFile temporary "verify-candidate-test"
      hClose handle
      removeFile root
      createDirectory root
      setFileMode root 0o700
      createDirectory (root </> "private-data")
      setFileMode (root </> "private-data") 0o700
      pure root

jsonFile :: FilePath -> Value -> IO ()
jsonFile path = BL.writeFile path . encode

-- | The observation comparison sources the verifier binds to a source manifest.
writeObservationSources :: FilePath -> IO ()
writeObservationSources root = do
  writeFile (root </> "emulation/E2EObservationCompare.hs") "observation compare\n"
  writeFile (root </> "emulation/Emulation/ObservationCompare.hs") "observation comparison\n"
  writeFile (root </> "emulation/Emulation/ObservationStream.hs") "observation stream\n"

qualificationWord :: Qualification -> Maybe T.Text
qualificationWord qualification = case qualification of
  QualifiedOutcome -> Just "candidate-qualified"
  RefusedOutcome -> Just "control-refused"
  UnknownOutcome -> Just "mystery"
  _ -> Nothing

verify :: FilePath -> Route -> RecordedExit -> ReceiptSource -> ReceiptParity -> Qualification -> IO Result
verify tool route evidence source parity qualification = withWorkspace $ \root -> do
  let (candidate, counterpart) = routeNames route
      earlier = root </> "private-data/earlier"
      receipt name = earlier </> "receipts" </> name
      output = root </> "private-data/result"
      exitFields = case evidence of
        MissingExit -> []
        NullExit -> ["exit_code" .= (Nothing :: Maybe Int)]
        RecordedExit code -> ["exit_code" .= case code of ExitSuccess -> 0 :: Int; ExitFailure n -> n]
      seed directory fixtureParity fixtureQualification = do
        createDirectoryIfMissing True directory
        case route of
          RamPushVolatile -> do
            let parityValue = case fixtureParity of EqualReceipts -> 1 :: Int; DifferentReceipts -> 2
                value = parityValue
                menuValue = case (fixtureQualification, fixtureParity) of
                  (SegmentedMenuDiffers, DifferentReceipts) -> 2 :: Int
                  _ -> 1
                observationValue = case (fixtureQualification, fixtureParity) of
                  (SegmentedObservationDiffers, DifferentReceipts) -> 2 :: Int
                  _ -> 1
                writeOutcome path word = jsonFile path (object
                  [ "menu_receipts" .= [object ["menu_id" .= menuValue]]
                  , "outcome" .= String word])
                segmentRecord name result message = object
                  [ "segment" .= String name
                  , "seconds" .= (0 :: Double)
                  , "result" .= String result
                  , "message" .= message ]
                segments :: [String] -> IO ()
                segments names = do
                  jsonFile (directory </> "e2e-segments.json")
                    (toJSON (map (\name -> segmentRecord (T.pack name) "passed" Null) names))
                  mapM_ (\name -> do
                    let segment = directory </> ("segment-" ++ name)
                    createDirectoryIfMissing True segment
                    jsonFile (segment </> "e2e-result.json")
                      (object [ "menu_receipts" .= [object ["menu_id" .= menuValue]]
                              , "outcome" .= String "candidate-qualified"])
                    jsonFile (segment </> "e2e-observation-0.json")
                      (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= observationValue]], "trace_hits" .= ([] :: [Value])])) names
            jsonFile (directory </> "e2e-observation-0.json")
              (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= value]], "trace_hits" .= ([] :: [Value])])
            case fixtureQualification of
              MissingOutcome -> pure ()
              ControlReceipt -> jsonFile (directory </> "e2e-control.json")
                (object ["outcome" .= String "control-refused"])
              SegmentedQualified -> segments ["service-field-list"]
              SegmentedMenuDiffers -> segments ["service-field-list"]
              SegmentedObservationDiffers -> segments ["service-field-list"]
              SegmentedWithControl -> do
                segments ["service-field-list", "service-save-current"]
                jsonFile (directory </> "segment-service-save-current/e2e-control.json")
                  (object ["outcome" .= String "control-refused"])
              SegmentedFailed -> do
                segments ["service-field-list"]
                jsonFile (directory </> "e2e-segments.json")
                  (toJSON [ segmentRecord "service-field-list" "failed" (String "segment failed") ])
              SegmentedMalformed -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [object ["segment" .= String "service-field-list", "result" .= String "passed"]])
              SegmentedDuplicateName -> do
                segments ["service-field-list"]
                jsonFile (directory </> "e2e-segments.json")
                  (toJSON [ segmentRecord "service-field-list" "passed" Null
                          , segmentRecord "service-field-list" "passed" Null ])
              SegmentedPathName -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ segmentRecord "../escape" "passed" Null ])
              PassedWithMessage -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ segmentRecord "service-field-list" "passed" (String "unexpected") ])
              FailedWithoutMessage -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ segmentRecord "service-field-list" "failed" Null ])
              MissingMessage -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ object ["segment" .= String "service-field-list", "seconds" .= (0 :: Double), "result" .= String "passed" ] ])
              NegativeSeconds -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ object ["segment" .= String "service-field-list", "seconds" .= (-1 :: Double), "result" .= String "passed", "message" .= Null] ])
              OverflowSeconds -> BL.writeFile (directory </> "e2e-segments.json")
                "[{\"segment\":\"service-field-list\",\"seconds\":1e400,\"result\":\"passed\",\"message\":null}]"
              ExtraKey -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ object ["segment" .= String "service-field-list", "seconds" .= (0 :: Double), "result" .= String "passed", "message" .= Null, "extra" .= True] ])
              UnknownResult -> jsonFile (directory </> "e2e-segments.json")
                (toJSON [ segmentRecord "service-field-list" "mystery" Null ])
              SegmentedMissingChild -> do
                jsonFile (directory </> "e2e-segments.json")
                  (toJSON [ segmentRecord "service-field-list" "passed" Null
                          , segmentRecord "service-save-current" "passed" Null ])
                createDirectoryIfMissing True (directory </> "segment-service-field-list")
                writeOutcome (directory </> "segment-service-field-list/e2e-result.json") "candidate-qualified"
              EmptySegments -> jsonFile (directory </> "e2e-segments.json") (toJSON ([] :: [Value]))
              FlatPlusSegmented -> do
                segments ["service-field-list"]
                writeOutcome (directory </> "e2e-result.json") "candidate-qualified"
              _ -> case qualificationWord fixtureQualification of
                Just word -> writeOutcome (directory </> "e2e-result.json") word
                Nothing -> pure ()
          UsbToggle -> do
            let (scenarioResult, boundary) = case fixtureParity of
                  EqualReceipts -> (String "blocked", object ["message" .= String "synthetic block", "boundary" .= object []])
                  DifferentReceipts -> (String "pass", Null)
                scenario = object ["scenario" .= String "synthetic", "result" .= scenarioResult
                  , "receipt" .= (directory </> "synthetic/receipt.json"), "failed_checks" .= ([] :: [String])]
            createDirectory (directory </> "synthetic")
            jsonFile (directory </> "summary.json") (object ["schema" .= (1 :: Int), "scenarios" .= [scenario]])
            jsonFile (directory </> "synthetic/receipt.json")
              (object ["schema" .= (1 :: Int), "scenario" .= String "synthetic", "result" .= scenarioResult
                , "boundary" .= boundary, "checks" .= ([] :: [Value])])
  let effective side = case qualification of
        CounterpartControl -> if side == CounterpartSide then ControlReceipt else QualifiedOutcome
        CounterpartFailedCommand -> QualifiedOutcome
        _ -> if side == CounterpartSide
          then case qualification of SegmentedQualified -> SegmentedQualified; SegmentedMenuDiffers -> SegmentedQualified; SegmentedObservationDiffers -> SegmentedQualified; _ -> QualifiedOutcome
          else qualification
  seed (receipt counterpart) EqualReceipts (effective CounterpartSide)
  seed (receipt candidate) parity (effective CandidateSide)
  let counterpartExit = case qualification of
        CounterpartFailedCommand -> ["exit_code" .= (1 :: Int)]
        _ -> ["exit_code" .= (0 :: Int)]
  jsonFile (earlier </> "report.json") (object ["routes" .=
    [ object (["route" .= candidate, "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")] ++ exitFields)
    , object (["route" .= counterpart, "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")] ++ counterpartExit) ]])
  inherited <- getEnvironment
  let overrides = case source of
        EarlierRun -> []
        ReceiptOverride -> ["--receipt", candidate ++ "=" ++ receipt candidate]
      command = (proc tool (["--routes", candidate, "--from-run", earlier, "--output", output] ++ overrides))
        { cwd = Just root
        , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
        }
  (code, out, err) <- readCreateProcessWithExitCode command ""
  check ("receipt-only verifier completed: " ++ out ++ err) (code == ExitSuccess)
  bytes <- BL.readFile (output </> "report.json")
  Report outcomes <- either (ioError . userError) pure (eitherDecode bytes)
  case outcomes of
    [RouteOutcome result] -> pure result
    _ -> ioError (userError "expected one reported route")

routeOf :: Value -> Maybe T.Text
routeOf (Object value) = case KM.lookup "route" value of
  Just (String route) -> Just route
  _ -> Nothing
routeOf _ = Nothing

reportResult :: Maybe Value -> Maybe T.Text
reportResult (Just (Object value)) = case KM.lookup "result" value of
  Just (String result) -> Just result
  _ -> Nothing
reportResult _ = Nothing

recordBaseline :: FilePath -> RecordedExit -> Qualification -> IO (ExitCode, Maybe Value)
recordBaseline tool evidence qualification = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baseline = root </> "private-data/baselines/ram-push-volatile.json"
      output = root </> "private-data/result"
      exitFields = case evidence of
        MissingExit -> []
        NullExit -> ["exit_code" .= (Nothing :: Maybe Int)]
        RecordedExit code -> ["exit_code" .= case code of ExitSuccess -> 0 :: Int; ExitFailure n -> n]
  seedRecordReceipt receipt qualification
  jsonFile (earlier </> "report.json") (object ["routes" .= [object
    (["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")] ++ exitFields)]])
  inherited <- getEnvironment
  let command = (proc tool ["--routes", "ram-push-volatile", "--from-run", earlier
        , "--record-baseline", "--baselines", root </> "private-data/baselines", "--output", output])
        { cwd = Just root
        , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited) }
  (code, _, _) <- readCreateProcessWithExitCode command ""
  present <- doesFileExist baseline
  value <- if present then eitherDecode <$> BL.readFile baseline else pure (Left "absent")
  pure (code, either (const Nothing) Just value)

recordReplacement :: FilePath -> IO (ExitCode, Maybe BL.ByteString, Maybe BL.ByteString, Maybe Value)
recordReplacement tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baseline = root </> "private-data/baselines/ram-push-volatile.json"
      firstOutput = root </> "private-data/result-1"
      secondOutput = root </> "private-data/result-2"
  seedRecordReceipt receipt QualifiedOutcome
  jsonFile (earlier </> "report.json") (object ["routes" .= [object
    ["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log"), "exit_code" .= (0 :: Int)]]])
  inherited <- getEnvironment
  let runRecord output = readCreateProcessWithExitCode ((proc tool ["--routes", "ram-push-volatile", "--from-run", earlier
        , "--record-baseline", "--baselines", root </> "private-data/baselines", "--output", output])
        { cwd = Just root
        , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited) }) ""
  (firstCode, _, _) <- runRecord firstOutput
  before <- if firstCode == ExitSuccess then Just <$> BL.readFile baseline else pure Nothing
  seedRecordReceipt receipt ControlReceipt
  (secondCode, _, _) <- runRecord secondOutput
  present <- doesFileExist baseline
  after <- if present then Just <$> BL.readFile baseline else pure Nothing
  report <- either (const Nothing) Just <$> (eitherDecode <$> BL.readFile (secondOutput </> "report.json"))
  pure (secondCode, before, after, report)

seedRecordReceipt :: FilePath -> Qualification -> IO ()
seedRecordReceipt directory qualification = do
  createDirectoryIfMissing True directory
  jsonFile (directory </> "e2e-observation-0.json")
    (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= (1 :: Int)]]])
  case qualification of
    ControlReceipt -> jsonFile (directory </> "e2e-control.json") (object ["outcome" .= String "control-refused"])
    UnknownOutcome -> jsonFile (directory </> "e2e-result.json") (object ["outcome" .= String "mystery"])
    _ -> jsonFile (directory </> "e2e-result.json")
      (object ["menu_receipts" .= [object ["menu_id" .= (1 :: Int)]], "outcome" .= String "candidate-qualified"])

seedRecordSegments :: FilePath -> Int -> Int -> Qualification -> IO ()
seedRecordSegments directory menuValue observationValue qualification = do
  stale <- doesDirectoryExist directory
  when stale (removeDirectoryRecursive directory)
  createDirectoryIfMissing True directory
  let names = ["service-field-list"]
      outcome = case qualification of
        UnknownOutcome -> "mystery"
        _ -> "candidate-qualified"
  jsonFile (directory </> "e2e-segments.json")
    (toJSON (map (\name -> object [ "segment" .= String (T.pack name), "seconds" .= (0 :: Double)
                                   , "result" .= String "passed", "message" .= Null ]) names))
  forM_ names $ \name -> do
    let segment = directory </> ("segment-" ++ name)
    createDirectoryIfMissing True segment
    jsonFile (segment </> "e2e-result.json")
      (object [ "menu_receipts" .= [object ["menu_id" .= menuValue]], "outcome" .= String outcome ])
    jsonFile (segment </> "e2e-observation-0.json")
      (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= observationValue]]])
    case qualification of
      ControlReceipt -> jsonFile (segment </> "e2e-control.json") (object ["outcome" .= String "control-refused"])
      _ -> pure ()

segmentedBaselineChecks :: FilePath -> IO ()
segmentedBaselineChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baseline = root </> "private-data/baselines/ram-push-volatile.json"
      output name = root </> ("private-data/" ++ name)
      recordedField file pointer expected recordedValue = any (\(Field (FieldName actualName actualPointer) actualValue) ->
        actualName == file && actualPointer == pointer && actualValue == expected) (baselineFields recordedValue)
  inherited <- getEnvironment
  let environment = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
      report :: Int -> IO ()
      report exitCode = jsonFile (earlier </> "report.json") (object ["routes" .= [object
        ["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")
        , "exit_code" .= exitCode]]])
      run :: String -> Bool -> Int -> IO (ExitCode, String, String)
      run name record exitCode = do
        report exitCode
        readCreateProcessWithExitCode ((proc tool (["--routes", "ram-push-volatile", "--from-run", earlier,
          "--baselines", root </> "private-data/baselines", "--output", output name]
          ++ if record then ["--record-baseline"] else []))
          { cwd = Just root, env = environment }) ""
      readBaseline = BL.readFile baseline
  seedRecordSegments receipt 1 1 QualifiedOutcome
  (recordedExit, _, _) <- run "record" True 0
  first <- readBaseline
  firstStrict <- B.readFile baseline
  let decoded = decodeBaseline firstStrict
      childReceipt = "segment-service-field-list/e2e-result.json"
      childObservation = "segment-service-field-list/e2e-observation-0.json"
  check "a segmented receipt records the declared child menu receipt"
    (recordedExit == ExitSuccess && case decoded of
      Right value -> recordedField childReceipt (Just [Key "menu_receipts", Index 0, Key "menu_id"]) (Number 1) value
      Left _ -> False)
  check "a segmented receipt records the declared child observation and manifest"
    (case decoded of
      Right value -> recordedField childObservation (Just [Key "reads", Index 0, Key "value"]) (Number 1) value
        && recordedField "e2e-segments.json" (Just [Index 0, Key "segment"]) (String "service-field-list") value
      Left _ -> False)
  seedRecordSegments receipt 2 1 QualifiedOutcome
  (menuExit, _, _) <- run "menu-mutated" False 0
  menuReport <- either (const Nothing) Just <$> (eitherDecode <$> BL.readFile (output "menu-mutated" </> "report.json"))
  check "a changed child menu receipt fails against its recorded baseline"
    (menuExit == ExitFailure 1 && reportResult menuReport == Just (T.pack "fail"))
  seedRecordSegments receipt 1 2 QualifiedOutcome
  (observationExit, _, _) <- run "observation-mutated" False 0
  observationReport <- either (const Nothing) Just <$> (eitherDecode <$> BL.readFile (output "observation-mutated" </> "report.json"))
  check "a changed child observation fails against its recorded baseline"
    (observationExit == ExitFailure 1 && reportResult observationReport == Just (T.pack "fail"))
  second <- readBaseline
  seedRecordSegments receipt 1 1 ControlReceipt
  (controlExit, _, _) <- run "control" True 0
  third <- readBaseline
  check "a segmented control receipt leaves the baseline unchanged"
    (controlExit == ExitFailure 1 && first == third)
  seedRecordSegments receipt 1 1 UnknownOutcome
  (unknownExit, _, _) <- run "unknown" True 0
  fourth <- readBaseline
  check "a segmented unknown outcome leaves the baseline unchanged"
    (unknownExit == ExitFailure 1 && first == fourth)
  seedRecordSegments receipt 1 1 QualifiedOutcome
  (failedExit, _, _) <- run "failed-command" True 1
  fifth <- readBaseline
  check "a failed command leaves the segmented baseline unchanged"
    (failedExit == ExitFailure 1 && first == fifth && second == fifth)

nativeBaselineChecks :: FilePath -> IO ()
nativeBaselineChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      original = earlier </> "receipts/native-sd"
      current = earlier </> "receipts/current"
      baselines = root </> "private-data/baselines"
      output name = root </> ("private-data/" ++ name)
      seed directory findings = do
        createDirectoryIfMissing True directory
        jsonFile (directory </> "results.json") (object
          ["cases" .= [object ["id" .= String "same", "status" .= String "passed"]]])
        writeFile (directory </> "findings.txt") findings
  seed original "header\nsame: passed\n"
  seed current "header\nnew explanation\nsame: passed\n"
  jsonFile (earlier </> "report.json") (object ["routes" .= [object
    ["route" .= String "native-sd", "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")
    , "exit_code" .= (0 :: Int)]]])
  inherited <- getEnvironment
  let run name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "native-sd", "--from-run", earlier, "--baselines", baselines, "--output", output name] ++ extra))
        { cwd = Just root
        , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited) }) ""
  (recordedExit, _, _) <- run "record" ["--record-baseline"]
  check "a synthetic native receipt records" (recordedExit == ExitSuccess)
  (changedExit, _, _) <- run "compare" ["--receipt", "native-sd=" ++ current]
  check "changed findings fail native comparison" (changedExit == ExitFailure 1)
  diff <- readFile (output "compare" </> "differences/native-sd.findings.diff")
  check "native findings report the added line"
    ("+new explanation" `elem` lines diff && " same: passed" `elem` lines diff)

nativeRunnerTransitionChecks :: FilePath -> IO ()
nativeRunnerTransitionChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/native-sd"
      baselineReceipt = earlier </> "receipts/baseline"
      baselines = root </> "private-data/baselines"
      baselinePath = baselines </> "native-sd.json"
      output name = root </> ("private-data/" ++ name)
      ledger = root </> "private-data/ledger.json"
      runnerBytes = BC.pack "synthetic-native-runner"
      runnerPath = root </> "private-data/runner.bin"
      candidateRunner = T.pack (sha256Hex (sha256 runnerBytes))
      baselineRunner = T.replicate 64 "b"
      ids = [T.pack ("svc-" ++ show n) | n <- [0 .. 25 :: Int]]
      otherId = T.pack "other"
      defaultIds = ids ++ [otherId]
      firstId = case ids of (cid : _) -> cid; [] -> error "composition fixture has no override ids"
      wire = T.replicate 64 "c"
      artifacts = [object ["id" .= ("kernel" :: T.Text), "sha256" .= T.replicate 64 "a", "size" .= (1 :: Int)]]
      findings = "header\n" ++ concat ["svc-" ++ show n ++ ": passed\n" | n <- [0 .. 25 :: Int]] ++ "other: passed\n"
      rich :: T.Text -> T.Text -> T.Text -> T.Text -> Value
      rich cid runner statusValue wireValue = object
        [ "id" .= cid
        , "status" .= statusValue
        , "runner_status" .= ("returned" :: T.Text)
        , "wire_sha256" .= wireValue
        , "runner_sha256" .= runner
        , "provenance_complete" .= True
        , "artifacts" .= artifacts
        , "observed_r0" .= (0 :: Int)
        , "expected_r0" .= (0 :: Int)
        , "instructions" .= (100 :: Int)
        , "provenance_total" .= (10 :: Int)
        , "mismatches" .= ([] :: [Value])
        ]
      reduced :: T.Text -> Value
      reduced cid = object
        [ "id" .= cid, "status" .= ("passed" :: T.Text), "runner_status" .= ("returned" :: T.Text)
        , "mismatches" .= ([] :: [Value]) ]
      other = reduced otherId
      defaultDir = earlier </> "default"
      defaultReportPath = defaultDir </> "results.json"
      defaultFindingsPath = defaultDir </> "findings.txt"
      overrideDir = earlier </> "override"
      overrideReportPath = overrideDir </> "results.json"
      compositionPath = root </> "private-data/composition.json"
      transitionPath = root </> "private-data/transition.json"
      field key (Object members) = KM.lookup (K.fromText key) members
      field _ _ = Nothing
      firstRoute value = case field "routes" value of
        Just (Array routes) -> case V.toList routes of (route : _) -> Just route; [] -> Nothing
        _ -> Nothing
      textOf key value = case field key value of Just (String text) -> Just text; _ -> Nothing
      writeResults directory cases = do
        createDirectoryIfMissing True directory
        jsonFile (directory </> "results.json") (object ["bank_mode" .= ("sd" :: T.Text), "cases" .= cases])
        writeFile (directory </> "findings.txt") findings
      writeDefaultFiles cases = do
        writeResults defaultDir cases
        forM_ cases $ \caseValue -> do
          cid <- maybe (ioError (userError "default case id")) pure (textOf "id" caseValue)
          let directory = defaultDir </> T.unpack cid
          createDirectoryIfMissing True directory
          jsonFile (directory </> "behavioral-result.json") caseValue
      writeOverride cases = writeResults overrideDir cases
      writeBaselineReceipt cases = writeResults baselineReceipt cases
      writeComposed cases = writeResults receipt cases
      caseFileManifest = do
        entries <- forM defaultIds $ \cid -> do
          bytes <- B.readFile (defaultDir </> T.unpack cid </> "behavioral-result.json")
          pure (cid, T.pack (sha256Hex (sha256 bytes)))
        pure (T.pack (sha256Hex (sha256 (BC.pack (concat [T.unpack cid ++ "\0" ++ T.unpack digest ++ "\n" | (cid, digest) <- sortOn fst entries])))))
      writeManifests overrideIds = do
        composedResults <- B.readFile (receipt </> "results.json")
        composedFindings <- B.readFile (receipt </> "findings.txt")
        defaultReport <- B.readFile defaultReportPath
        defaultFindings <- B.readFile defaultFindingsPath
        overrideReport <- B.readFile overrideReportPath
        baselineBytes <- B.readFile baselinePath
        perCase <- caseFileManifest
        jsonFile compositionPath (object
          [ "schema" .= (1 :: Int)
          , "kind" .= ("native-baseline-input-casewise-composition" :: T.Text)
          , "composed" .= object
              [ "results_sha256" .= T.pack (sha256Hex (sha256 composedResults))
              , "findings_sha256" .= T.pack (sha256Hex (sha256 composedFindings))
              , "case_count" .= (length defaultIds :: Int)
              ]
          , "baseline" .= object ["sha256" .= T.pack (sha256Hex (sha256 baselineBytes))]
          , "default_source" .= object
              [ "report" .= defaultReportPath
              , "sha256" .= T.pack (sha256Hex (sha256 defaultReport))
              , "findings" .= defaultFindingsPath
              , "findings_sha256" .= T.pack (sha256Hex (sha256 defaultFindings))
              , "case_file_manifest_sha256" .= perCase
              , "runner_sha256" .= candidateRunner
              , "case_count" .= (length defaultIds :: Int)
              ]
          , "override_source" .= object
              [ "report" .= overrideReportPath
              , "sha256" .= T.pack (sha256Hex (sha256 overrideReport))
              , "runner_sha256" .= candidateRunner
              , "ids" .= overrideIds
              , "case_count" .= (length overrideIds :: Int)
              ]
          ])
        compositionBytes <- B.readFile compositionPath
        let compositionDigest = T.pack (sha256Hex (sha256 compositionBytes))
        jsonFile transitionPath (object
          [ "baseline_runner_sha256" .= baselineRunner
          , "candidate_runner_sha256" .= candidateRunner
          , "runner_path" .= runnerPath
          , "binding" .= compositionDigest
          ])
        pure compositionDigest
      validDefault = map reduced defaultIds
      validOverride = map (\cid -> rich cid candidateRunner "passed" wire) ids
      validComposed = map (\cid -> rich cid candidateRunner "passed" wire) ids ++ [other]
      validBaseline = map (\cid -> rich cid baselineRunner "passed" wire) ids ++ [other]
  createDirectoryIfMissing True (root </> "private-data")
  BC.writeFile runnerPath runnerBytes
  writeBaselineReceipt validBaseline
  writeLedger ledger [object
    [ "route" .= ("native-sd" :: T.Text)
    , "evidence" .= object
        [ "kind" .= ("known" :: T.Text)
        , "exit_code" .= (0 :: Int)
        , "seconds" .= (1.0 :: Double)
        , "provenance" .= object
            [ "kind" .= ("verifier" :: T.Text)
            , "run" .= earlier
            , "log" .= (earlier </> "synthetic.log")
            ]
        ]
    ]]
  inherited <- getEnvironment
  let environment = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
      run name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "native-sd", "--receipt", "native-sd=" ++ receipt
         , "--baselines", baselines, "--output", output name] ++ extra))
        { cwd = Just (root </> "private-data"), env = environment }) ""
      runRecord name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "native-sd", "--receipt", "native-sd=" ++ baselineReceipt
         , "--execution-ledger", ledger, "--baselines", baselines, "--output", output name] ++ extra))
        { cwd = Just root, env = environment }) ""
      readReport name = eitherDecode <$> BL.readFile (output name </> "report.json")
      admittedArguments = ["--native-runner-transition", makeRelative root transitionPath, "--composition-manifest", makeRelative root compositionPath]
  (recordedExit, _, recordErr) <- runRecord "record" ["--record-baseline"]
  check ("a valid synthetic native baseline records: " ++ recordErr) (recordedExit == ExitSuccess)
  writeDefaultFiles validDefault
  writeOverride validOverride
  writeComposed validComposed
  compositionDigest <- writeManifests ids
  (strictExit, _, _) <- run "strict" []
  strictReport <- readReport "strict"
  check "strict native comparison still detects the runner changes" (strictExit == ExitFailure 1)
  check "strict native comparison reports 26 case differences"
    (case strictReport of
       Right value -> field "result" value == Just (String "fail")
         && (firstRoute value >>= field "differences") == Just (Number 26)
       _ -> False)
  (admittedExit, admittedOut, admittedErr) <- run "admitted" admittedArguments
  admittedReport <- readReport "admitted"
  check ("a valid composition passes without a fabricated ledger: " ++ admittedOut ++ admittedErr) (admittedExit == ExitSuccess)
  check "the admitted report passes with 26 provenance changes and no differences"
    (case admittedReport of
       Right value ->
         field "result" value == Just (String "pass")
         && (firstRoute value >>= field "differences") == Just (Number 0)
         && (firstRoute value >>= field "provenance_change_count") == Just (Number 26)
         && (firstRoute value >>= field "route") == Just (String "native-sd")
         && (firstRoute value >>= field "gating") == Just (Bool True)
         && (firstRoute value >>= field "execution") == Just (String "unknown")
         && (firstRoute value >>= field "evidence_kind") == Just (String "composition")
         && (firstRoute value >>= field "exit_code") == Nothing
         && (firstRoute value >>= field "seconds") == Nothing
       _ -> False)
  check "the admitted report carries the composition source provenance"
    (case admittedReport of
       Right value -> case firstRoute value >>= field "composition" of
         Just composition ->
           field "manifest" composition == Just (String (T.pack compositionPath))
           && field "sha256" composition == Just (String compositionDigest)
           && (field "document" composition >>= field "default_source" >>= field "report") == Just (String (T.pack defaultReportPath))
           && (field "document" composition >>= field "override_source" >>= field "report") == Just (String (T.pack overrideReportPath))
         _ -> False
       _ -> False)
  let changed firstCase replacement = map (\cid -> if cid == firstCase then replacement else rich cid candidateRunner "passed" wire) ids
  writeOverride (changed (firstId) (rich (firstId) (T.replicate 64 "d") "passed" wire))
  writeComposed (changed (firstId) (rich (firstId) (T.replicate 64 "d") "passed" wire) ++ [other])
  _ <- writeManifests ids
  (wrongExit, wrongOut, wrongErr) <- run "wrong-runner" admittedArguments
  check ("a third runner digest is refused: " ++ wrongOut ++ wrongErr)
    (wrongExit == ExitFailure 1 && "unexpected runner identity" `isInfixOf` (wrongOut ++ wrongErr))
  writeOverride (changed (firstId) (rich (firstId) candidateRunner "passed" (T.replicate 64 "e")))
  writeComposed (changed (firstId) (rich (firstId) candidateRunner "passed" (T.replicate 64 "e")) ++ [other])
  _ <- writeManifests ids
  (wireExit, _, _) <- run "wire" admittedArguments
  wireReport <- readReport "wire"
  check "a wire change still fails under an admitted transition"
    (wireExit == ExitFailure 1 && case wireReport of
       Right value -> (firstRoute value >>= field "differences") == Just (Number 1)
       _ -> False)
  writeOverride (changed (firstId) (rich (firstId) candidateRunner "failed" wire))
  writeComposed (changed (firstId) (rich (firstId) candidateRunner "failed" wire) ++ [other])
  _ <- writeManifests ids
  (behaviorExit, _, _) <- run "behavior" admittedArguments
  behaviorReport <- readReport "behavior"
  check "a behavior change still fails under an admitted transition"
    (behaviorExit == ExitFailure 1 && case behaviorReport of
       Right value -> (firstRoute value >>= field "differences") == Just (Number 1)
       _ -> False)
  writeOverride validOverride
  writeComposed (drop 1 validComposed)
  _ <- writeManifests ids
  (missingExit, missingOut, missingErr) <- run "missing" admittedArguments
  check ("a missing composed case is refused: " ++ missingOut ++ missingErr)
    (missingExit == ExitFailure 1 && "composed case order or set" `isInfixOf` (missingOut ++ missingErr))
  writeComposed validComposed
  _ <- writeManifests (init ids)
  (mismatchExit, mismatchOut, mismatchErr) <- run "mismatched-ids" admittedArguments
  check ("mismatched override ids are refused: " ++ mismatchOut ++ mismatchErr)
    (mismatchExit == ExitFailure 1 && "override ids do not match" `isInfixOf` (mismatchOut ++ mismatchErr))
  _ <- writeManifests (ids ++ [firstId])
  (duplicateExit, duplicateOut, duplicateErr) <- run "duplicate-ids" admittedArguments
  check ("duplicate override ids are refused: " ++ duplicateOut ++ duplicateErr)
    (duplicateExit == ExitFailure 1 && "repeats an id" `isInfixOf` (duplicateOut ++ duplicateErr))
  _ <- writeManifests ids
  appendFile defaultReportPath "\n"
  (staleExit, staleOut, staleErr) <- run "stale-source" admittedArguments
  check ("a stale source digest is refused: " ++ staleOut ++ staleErr)
    (staleExit == ExitFailure 1 && "default source digest does not match" `isInfixOf` (staleOut ++ staleErr))
  writeDefaultFiles validDefault
  writeOverride (changed (firstId) (rich (firstId) baselineRunner "passed" wire))
  writeComposed (changed (firstId) (rich (firstId) baselineRunner "passed" wire) ++ [other])
  _ <- writeManifests ids
  (provenanceExit, provenanceOut, provenanceErr) <- run "provenance-ids" admittedArguments
  check ("a provenance id mismatch is refused: " ++ provenanceOut ++ provenanceErr)
    (provenanceExit == ExitFailure 1 && "provenance case ids do not match" `isInfixOf` (provenanceOut ++ provenanceErr))

data RouteDetail = RouteDetail
  { detailResult :: T.Text
  , detailExit :: Maybe Int
  , detailProblem :: Maybe T.Text
  , detailRelease :: Maybe T.Text
  , detailIncomplete :: Maybe [Value]
  }

instance FromJSON RouteDetail where
  parseJSON = withObject "route detail" $ \value ->
    RouteDetail <$> value .: "result" <*> value .:? "exit_code" <*> value .:? "problem"
      <*> value .:? "release_evidence" <*> value .:? "incomplete_releases"

newtype DetailReport = DetailReport [RouteDetail]

instance FromJSON DetailReport where
  parseJSON = withObject "detail report" $ \value -> DetailReport <$> value .: "routes"

runVerifier :: FilePath -> FilePath -> [String] -> IO (ExitCode, String, String)
runVerifier tool root arguments = do
  inherited <- getEnvironment
  readCreateProcessWithExitCode ((proc tool arguments)
    { cwd = Just root
    , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited) }) ""

readDetails :: FilePath -> IO [RouteDetail]
readDetails output = do
  bytes <- BL.readFile (output </> "report.json")
  DetailReport routes <- either (ioError . userError) pure (eitherDecode bytes)
  pure routes

seedQualifiedReceipt :: FilePath -> IO ()
seedQualifiedReceipt directory = do
  createDirectoryIfMissing True directory
  jsonFile (directory </> "e2e-result.json") (object ["outcome" .= String "candidate-qualified"])

knownEntry :: String -> Int -> Value
knownEntry route exit = object
  [ "route" .= route
  , "evidence" .= object
      [ "kind" .= String "known"
      , "exit_code" .= exit
      , "provenance" .= object
          [ "kind" .= String "verifier"
          , "run" .= ("/synthetic" :: String)
          , "log" .= ("/synthetic/" ++ route ++ ".log" :: String) ] ]
  ]

unknownEntry :: String -> String -> Value
unknownEntry route reason = object
  [ "route" .= route
  , "evidence" .= object ["kind" .= String "unknown", "reason" .= reason] ]

writeLedger :: FilePath -> [Value] -> IO ()
writeLedger path entries = jsonFile path (object
  [ "schema" .= (1 :: Int)
  , "kind" .= String "verify-candidate-execution-ledger"
  , "routes" .= entries ])

ledgerChecks :: FilePath -> IO ()
ledgerChecks tool = do
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/unknown-exit"
    seedQualifiedReceipt receipt
    writeLedger ledger [unknownEntry "ram-push-volatile" "kernel receipt unavailable"]
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--execution-ledger", ledger, "--output", output]
    details <- readDetails output
    check ("an unknown exit fails closed: " ++ err) (case details of
      [detail] -> code == ExitFailure 1 && detailResult detail == "fail" && detailExit detail == Nothing
        && maybe False (T.isInfixOf "unknown") (detailProblem detail)
      _ -> False)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
    seedQualifiedReceipt receipt
    writeLedger ledger [knownEntry "ram-push-volatile" 0, knownEntry "ram-push-volatile" 1]
    (duplicateExit, _, duplicateErr) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--execution-ledger", ledger, "--output", root </> "private-data/duplicate"]
    check ("a repeated ledger route is refused: " ++ duplicateErr) (duplicateExit /= ExitSuccess)
    writeLedger ledger [object
      [ "route" .= ("ram-push-volatile" :: String)
      , "evidence" .= object
          [ "kind" .= String "known", "exit_code" .= (0 :: Int)
          , "provenance" .= object
              [ "kind" .= String "journal"
              , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
              , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
              , "cursor" .= String "cursor"
              , "boot_id" .= String "98f88ad1cae54a648487ae021fba3b67"
              , "timestamp" .= String "1793954731456505"
              , "pid" .= (702514 :: Int)
              , "message" .= String "end ram-push-volatile: exit 1 after 1.0 s"
              , "raw" .= object
                  [ "MESSAGE" .= String "end ram-push-volatile: exit 1 after 1.0 s"
                  , "_SYSTEMD_INVOCATION_ID" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
                  , "_BOOT_ID" .= String "98f88ad1cae54a648487ae021fba3b67"
                  , "__CURSOR" .= String "cursor"
                  , "_PID" .= String "702514"
                  , "__REALTIME_TIMESTAMP" .= String "1793954731456505"
                  , "_SYSTEMD_CGROUP" .= String "/user.slice/user-1000.slice/user@1000.service/app.slice/latent-image-pin-backing-qualification-ee153f43.service"
                  ] ] ] ]]
    (conflictExit, _, conflictErr) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--execution-ledger", ledger, "--output", root </> "private-data/conflict"]
    check ("a journal line for a different exit is refused: " ++ conflictErr) (conflictExit /= ExitSuccess)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        output = root </> "private-data/partial-matrix"
    seedQualifiedReceipt receipt
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--matrix", "--output", output]
    report <- doesFileExist (output </> "report.json")
    check ("a partial whole-matrix claim is refused: " ++ err) (code /= ExitSuccess && not report)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/resume"
        firstManifest = root </> "private-data/source-a.sha256"
        secondManifest = root </> "private-data/source-b.sha256"
        arguments manifest = ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
          , "--execution-ledger", ledger, "--source-manifest", manifest, "--output", output]
    seedQualifiedReceipt receipt
    writeLedger ledger [knownEntry "ram-push-volatile" 0]
    createDirectoryIfMissing True (root </> "emulation/Emulation")
    writeFile (root </> "emulation/VerifyCandidate.hs") "coordinator\n"
    writeFile (root </> "emulation/Emulation/ExecutionJournal.hs") "journal\n"
    writeFile (root </> "emulation/Emulation/CandidateRoute.hs") "routes\n"
    writeFile (root </> "emulation/Emulation/CandidateBaseline.hs") "comparator\n"
    writeObservationSources root
    writeFile firstManifest "0000000000000000000000000000000000000000000000000000000000000000  emulation/VerifyCandidate.hs\n"
    writeFile secondManifest "1111111111111111111111111111111111111111111111111111111111111111  emulation/VerifyCandidate.hs\n"
    (_, _, _) <- runVerifier tool root (arguments firstManifest)
    checkpoint <- doesFileExist (output </> "checkpoint.json")
    (code, _, err) <- runVerifier tool root (arguments secondManifest ++ ["--resume"])
    check ("a checkpoint identity conflict is refused: " ++ err)
      (checkpoint && code /= ExitSuccess && "checkpoint identity differs" `isInfixOf` err)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/comparator-drift"
        sourceManifest = root </> "private-data/source.sha256"
        coordinator = root </> "emulation/VerifyCandidate.hs"
        comparator = root </> "emulation/Emulation/CandidateBaseline.hs"
        arguments = ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
          , "--execution-ledger", ledger, "--source-manifest", sourceManifest, "--output", output]
    seedQualifiedReceipt receipt
    writeLedger ledger [knownEntry "ram-push-volatile" 0]
    createDirectoryIfMissing True (root </> "emulation/Emulation")
    writeFile coordinator "coordinator\n"
    writeFile (root </> "emulation/Emulation/ExecutionJournal.hs") "journal\n"
    writeFile (root </> "emulation/Emulation/CandidateRoute.hs") "routes\n"
    writeFile comparator "comparator-a\n"
    writeObservationSources root
    coordinatorBytes <- B.readFile coordinator
    writeFile sourceManifest (sha256Hex (sha256 coordinatorBytes) ++ "  emulation/VerifyCandidate.hs\n")
    (_, _, firstErr) <- runVerifier tool root arguments
    checkpoint <- doesFileExist (output </> "checkpoint.json")
    check ("the comparator-drift checkpoint is created: " ++ firstErr) checkpoint
    writeFile comparator "comparator-b\n"
    (resumeExit, _, resumeErr) <- runVerifier tool root (arguments ++ ["--resume"])
    check ("a changed native comparator refuses resume: " ++ resumeErr)
      (resumeExit /= ExitSuccess && "checkpoint identity differs" `isInfixOf` resumeErr)
  withWorkspace $ \root -> do
    let earlier = root </> "private-data/earlier"
        receipt = earlier </> "receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
        missingOutput = root </> "private-data/release-missing"
        observedOutput = root </> "private-data/release-observed"
        arguments output = ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
          , "--execution-ledger", ledger, "--output", output]
    seedQualifiedReceipt receipt
    writeLedger ledger [knownEntry "ram-push-volatile" 0]
    (_, _, _) <- runVerifier tool root (arguments missingOutput)
    missing <- readDetails missingOutput
    check "missing raw key releases are not zero failures" (case missing of
      [detail] -> detailRelease detail == Just "missing" && detailIncomplete detail == Nothing
      _ -> False)
    createDirectoryIfMissing True (earlier </> "key-releases")
    writeFile (earlier </> "key-releases/ram-push-volatile.jsonl")
      "{\"route\":\"ram-push-volatile\",\"segment\":null,\"key\":\"ok\",\"row\":1,\"cap\":100,\"instructions_used\":5,\"instructions\":7}\n"
    (_, _, _) <- runVerifier tool root (arguments observedOutput)
    observed <- readDetails observedOutput
    check "preserved raw key releases are reconstructed" (case observed of
      [detail] -> detailRelease detail == Just "observed" && maybe False ((== 1) . length) (detailIncomplete detail)
      _ -> False)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        output = root </> "private-data/sdk-mismatch"
        notSdk = root </> "not-a-store"
    seedQualifiedReceipt receipt
    createDirectoryIfMissing True notSdk
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--sdk", notSdk, "--output", output]
    checkpoint <- doesFileExist (output </> "checkpoint.json")
    check ("a changed SDK is refused before checkpoint creation: " ++ err)
      (code /= ExitSuccess && not checkpoint)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        output = root </> "private-data/route-input-drift"
        target = root </> "private-data/input.bin"
        manifest = root </> "private-data/route-inputs.sha256"
    seedQualifiedReceipt receipt
    writeFile target "input\n"
    writeFile manifest ("0000000000000000000000000000000000000000000000000000000000000000  " ++ target ++ "\n")
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--route-input-manifest", manifest, "--output", output]
    checkpoint <- doesFileExist (output </> "checkpoint.json")
    check ("route input drift is refused before checkpoint creation: " ++ err)
      (code /= ExitSuccess && not checkpoint)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        output = root </> "private-data/baseline-drift"
        target = root </> "private-data/baseline.json"
        manifest = root </> "private-data/baselines.sha256"
    seedQualifiedReceipt receipt
    writeFile target "baseline\n"
    writeFile manifest ("0000000000000000000000000000000000000000000000000000000000000000  " ++ target ++ "\n")
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--baseline-manifest", manifest, "--output", output]
    checkpoint <- doesFileExist (output </> "checkpoint.json")
    check ("baseline drift is refused before checkpoint creation: " ++ err)
      (code /= ExitSuccess && not checkpoint)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/ram-push-volatile"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/journal-raw-conflict"
    seedQualifiedReceipt receipt
    writeLedger ledger [object
      [ "route" .= ("ram-push-volatile" :: String)
      , "evidence" .= object
          [ "kind" .= String "known", "exit_code" .= (0 :: Int)
          , "provenance" .= object
              [ "kind" .= String "journal"
              , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
              , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
              , "cursor" .= String "cursor"
              , "boot_id" .= String "98f88ad1cae54a648487ae021fba3b67"
              , "timestamp" .= String "1793954731456505"
              , "pid" .= (702514 :: Int)
              , "message" .= String "end ram-push-volatile: exit 0 after 1.0 s"
              , "raw" .= object
                  [ "MESSAGE" .= String "end ram-push-volatile: exit 1 after 1.0 s"
                  , "_SYSTEMD_INVOCATION_ID" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
                  , "_BOOT_ID" .= String "98f88ad1cae54a648487ae021fba3b67"
                  , "__CURSOR" .= String "cursor"
                  , "_PID" .= String "702514"
                  , "__REALTIME_TIMESTAMP" .= String "1793954731456505"
                  , "_SYSTEMD_CGROUP" .= String "/user.slice/user-1000.slice/user@1000.service/app.slice/latent-image-pin-backing-qualification-ee153f43.service"
                  ] ] ] ]]
    (code, _, err) <- runVerifier tool root
      ["--routes", "ram-push-volatile", "--receipt", "ram-push-volatile=" ++ receipt
      , "--execution-ledger", ledger, "--output", output]
    check ("a journal raw message conflict is refused: " ++ err) (code /= ExitSuccess)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/updater-f7-cc-toggle"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/kernel-raw-conflict"
        rawStat = "1 (x) Z 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
    seedQualifiedReceipt receipt
    writeLedger ledger [object
      [ "route" .= ("updater-f7-cc-toggle" :: String)
      , "evidence" .= object
          [ "kind" .= String "known", "exit_code" .= (0 :: Int)
          , "provenance" .= object
              [ "kind" .= String "kernel"
              , "boot_id" .= String "98f88ad1-cae5-4a64-8487-ae021fba3b67"
              , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
              , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
              , "pid" .= (868889 :: Integer)
              , "parent_pid" .= (702514 :: Integer)
              , "start_ticks" .= (10034048 :: Integer)
              , "raw_wait_status" .= (0 :: Int)
              , "raw_stat" .= String rawStat
              , "raw" .= object
                  [ "route" .= ("ram-push-volatile" :: String)
                  , "pid" .= (868889 :: Integer)
                  , "parent_pid" .= (702514 :: Integer)
                  , "start_ticks" .= (10034048 :: Integer)
                  , "boot_id" .= String "98f88ad1-cae5-4a64-8487-ae021fba3b67"
                  , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
                  , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
                  , "raw_wait_status" .= (0 :: Int)
                  , "exit_code" .= (0 :: Int)
                  , "process_state" .= String "Z"
                  , "raw_stat" .= String rawStat
                  ] ] ] ]]
    (code, _, err) <- runVerifier tool root
      ["--routes", "updater-f7-cc-toggle", "--receipt", "updater-f7-cc-toggle=" ++ receipt
      , "--execution-ledger", ledger, "--output", output]
    check ("a kernel raw route conflict is refused: " ++ err) (code /= ExitSuccess)

kernelStateChecks :: FilePath -> IO ()
kernelStateChecks tool = do
  let rawStat state = "868889 (updater) " ++ unwords
        (state : ["702514", "868889", "868889", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "10034048"] ++ replicate 30 "0")
      kernelLedger state = object
        [ "route" .= ("updater-f7-cc-toggle" :: String)
        , "evidence" .= object
            [ "kind" .= String "known", "exit_code" .= (0 :: Int)
            , "provenance" .= object
                [ "kind" .= String "kernel"
                , "boot_id" .= String "98f88ad1-cae5-4a64-8487-ae021fba3b67"
                , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
                , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
                , "pid" .= (868889 :: Integer)
                , "parent_pid" .= (702514 :: Integer)
                , "start_ticks" .= (10034048 :: Integer)
                , "raw_wait_status" .= (0 :: Int)
                , "raw_stat" .= String (T.pack (rawStat state))
                , "raw" .= object
                    [ "route" .= ("updater-f7-cc-toggle" :: String)
                    , "pid" .= (868889 :: Integer)
                    , "parent_pid" .= (702514 :: Integer)
                    , "start_ticks" .= (10034048 :: Integer)
                    , "boot_id" .= String "98f88ad1-cae5-4a64-8487-ae021fba3b67"
                    , "unit" .= String "latent-image-pin-backing-qualification-ee153f43.service"
                    , "invocation" .= String "c6f763f40d014f9b87bbd1d1e29fbcf2"
                    , "raw_wait_status" .= (0 :: Int)
                    , "exit_code" .= (0 :: Int)
                    , "process_state" .= String (T.pack state)
                    , "raw_stat" .= String (T.pack (rawStat state))
                    ]
                ]
            ]
        ]
  forM_ ["R", "S"] $ \state -> withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/updater-f7-cc-toggle"
        ledger = root </> "private-data/ledger.json"
        output = root </> ("private-data/kernel-" ++ state)
    seedQualifiedReceipt receipt
    writeLedger ledger [kernelLedger state]
    (code, _, err) <- runVerifier tool root
      ["--routes", "updater-f7-cc-toggle", "--receipt", "updater-f7-cc-toggle=" ++ receipt
      , "--execution-ledger", ledger, "--output", output]
    check ("a consistent nonterminal kernel state is refused: " ++ state ++ ": " ++ err)
      (code /= ExitSuccess && "not terminal" `isInfixOf` err)
  withWorkspace $ \root -> do
    let receipt = root </> "private-data/earlier/receipts/updater-f7-cc-toggle"
        ledger = root </> "private-data/ledger.json"
        output = root </> "private-data/kernel-Z"
    seedQualifiedReceipt receipt
    writeLedger ledger [kernelLedger "Z"]
    (_, _, err) <- runVerifier tool root
      ["--routes", "updater-f7-cc-toggle", "--receipt", "updater-f7-cc-toggle=" ++ receipt
      , "--execution-ledger", ledger, "--output", output]
    check ("a terminal kernel state passes evidence validation: " ++ err) (not ("not terminal" `isInfixOf` err))

data GenerationFixture = GenerationFixture
  { fixtureOutcome :: T.Text
  , fixtureDeclared :: [Value]
  , fixtureSymbols :: Value
  , fixtureHitSite :: T.Text
  , fixtureRendezvousMatched :: Bool
  , fixtureMenuSha :: T.Text
  , fixtureSeconds :: Double
  , fixtureObservationHit :: Bool
  , fixtureOk :: Maybe Value
  , fixtureExtra :: Maybe Value
  }

seedGenerationReceipt :: FilePath -> GenerationFixture -> IO ()
seedGenerationReceipt directory fixture = do
  createDirectoryIfMissing True directory
  let sibling = directory </> "e2e-observation-0.json"
  present <- doesFileExist sibling
  when present (removeFile sibling)
  jsonFile (directory </> "e2e-result.json") (object
    [ "menu_receipts" .= [object (["label" .= String "menu"
        , "menu" .= object ["menu_id" .= (1 :: Int), "setting_id" .= (2 :: Int), "cursor" .= (3 :: Int)]
        , "sha256" .= String (fixtureMenuSha fixture)]
        ++ maybe [] (\value -> ["ok" .= value]) (fixtureOk fixture)
        ++ maybe [] (\value -> ["extra" .= value]) (fixtureExtra fixture))]
    , "observations" .= [object
        [ "phase" .= String "menu"
        , "steps" .= [object ["key" .= String "menu", "expected" .= ([1, 2, 3] :: [Int])
                             , "actual" .= ([1, 2, 3] :: [Int]), "matched" .= True
                             , "seconds" .= fixtureSeconds fixture]]
        , "trace_hits" .= (if fixtureObservationHit fixture
            then [object ["site" .= String (fixtureHitSite fixture), "core" .= (0 :: Int)
                         , "instructions" .= (10 :: Int), "r0" .= String "0x00000000"]]
            else [])]]
    , "rendezvous_exits" .= [object
        [ "key" .= String "menu"
        , "steps" .= [object ["key" .= String "menu", "expected" .= ([1, 2, 3] :: [Int])
                              , "actual" .= ([1, 2, 3] :: [Int])
                              , "matched" .= fixtureRendezvousMatched fixture
                              , "seconds" .= fixtureSeconds fixture]]]]
    , "outcome" .= String (fixtureOutcome fixture)
    , "declared_trace_sites" .= fixtureDeclared fixture
    , "runtime" .= object ["symbols_sha256" .= fixtureSymbols fixture]
    ])

-- | The recorded fields a baseline written before the current receipt
-- generation carries.
legacyGeneration :: Baseline -> Baseline
legacyGeneration baseline = baseline
  { baselineFields = okField : filter (not . generated) (baselineFields baseline) }
  where
    okField = Field (FieldName "e2e-result.json" (Just [Key "menu_receipts", Index 0, Key "ok"])) (Bool True)
    generated (Field (FieldName _ pointer) _) = case pointer of
      Just [Key "outcome"] -> True
      Just (Key "declared_trace_sites" : _) -> True
      Just [Key "runtime", Key "symbols_sha256"] -> True
      _ -> False

rewriteBaseline :: FilePath -> (Baseline -> Baseline) -> IO ()
rewriteBaseline path change = do
  parsed <- decodeBaseline <$> B.readFile path
  case parsed of
    Left problem -> ioError (userError problem)
    Right baseline -> B.writeFile path (encodeBaseline (change baseline))

withBaselineOk :: Value -> Baseline -> Baseline
withBaselineOk value baseline = baseline { baselineFields = map set (baselineFields baseline) }
  where
    set field@(Field (FieldName "e2e-result.json" (Just [Key "menu_receipts", Index 0, Key "ok"])) _) =
      Field (fieldName field) value
    set field = field

withoutBaselineOk :: Baseline -> Baseline
withoutBaselineOk baseline = baseline { baselineFields = filter keep (baselineFields baseline) }
  where
    keep (Field (FieldName "e2e-result.json" (Just [Key "menu_receipts", Index 0, Key "ok"])) _) = False
    keep _ = True

data GenerationDetail = GenerationDetail T.Text [T.Text] (Maybe T.Text) Int deriving (Show)

instance FromJSON GenerationDetail where
  parseJSON = withObject "generation route" $ \value -> do
    entries <- value .: "generation"
    kinds <- mapM (withObject "generation entry" (\entry -> entry .: "kind")) entries
    GenerationDetail <$> value .: "result" <*> pure kinds <*> value .:? "problem" <*> value .: "differences"

newtype GenerationReport = GenerationReport [GenerationDetail]

instance FromJSON GenerationReport where
  parseJSON = withObject "generation report" $ \value -> GenerationReport <$> value .: "routes"

generationChecks :: FilePath -> IO ()
generationChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baselines = root </> "private-data/baselines"
      baselinePath = baselines </> "ram-push-volatile.json"
      output name = root </> ("private-data/" ++ name)
      kernelBytes = BC.pack "synthetic kernel symbols\n"
      symbolsDigest = T.pack (sha256Hex (sha256 kernelBytes))
      menuSha = T.replicate 64 "a"
      site = object ["label" .= String "frames-film-class-entry", "address" .= String "00e42414"]
      fixture seconds = GenerationFixture
        { fixtureOutcome = "candidate-qualified"
        , fixtureDeclared = [site]
        , fixtureSymbols = String symbolsDigest
        , fixtureHitSite = "frames-film-class-entry"
        , fixtureRendezvousMatched = True
        , fixtureMenuSha = menuSha
        , fixtureSeconds = seconds
        , fixtureObservationHit = True
        , fixtureOk = Nothing
        , fixtureExtra = Nothing }
      detail name = do
        parsed <- eitherDecode <$> BL.readFile (output name </> "report.json") :: IO (Either String GenerationReport)
        pure $ case parsed of
          Left problem -> Left problem
          Right (GenerationReport (entry : _)) -> Right entry
          Right (GenerationReport []) -> Left "the report holds no route"
  createDirectoryIfMissing True (receipt </> "relinked")
  B.writeFile (receipt </> "relinked/kernel.syms") kernelBytes
  jsonFile (earlier </> "report.json") (object ["routes" .= [object
    ["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double)
    , "log" .= String "/synthetic.log", "exit_code" .= (0 :: Int)]]])
  inherited <- getEnvironment
  let environment = Just (("BUILD_WORKSPACE_DIRECTORY", root)
        : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
      run name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "ram-push-volatile", "--from-run", earlier, "--baselines", baselines
         , "--output", output name] ++ extra))
        { cwd = Just root, env = environment }) ""
      expectPass name kinds = do
        (code, out, err) <- run name []
        parsed <- detail name
        check ("accepted generation change passes: " ++ name ++ ": " ++ out ++ err ++ " " ++ show parsed)
          (code == ExitSuccess && case parsed of
             Right (GenerationDetail result found _ differences) ->
               result == "pass" && differences == 0 && all (`elem` found) kinds
             Left _ -> False)
      expectDiff name = do
        (code, _, err) <- run name []
        parsed <- detail name
        check ("a generation change outside the projection fails as a difference: " ++ name ++ ": " ++ err ++ " " ++ show parsed)
          (code == ExitFailure 1 && case parsed of
             Right (GenerationDetail result _ _ differences) -> result == "fail" && differences > 0
             Left _ -> False)
      expectProblem name = do
        (code, _, err) <- run name []
        parsed <- detail name
        check ("a generation change without its check fails as a check: " ++ name ++ ": " ++ err ++ " " ++ show parsed)
          (code == ExitFailure 1 && case parsed of
             Right (GenerationDetail result _ problem _) ->
               result == "fail" && maybe False (not . T.null) problem
             Left _ -> False)
  seedGenerationReceipt receipt (fixture 1.0)
  (recorded, _, recordErr) <- run "generation-record" ["--record-baseline"]
  check ("a generation fixture records: " ++ recordErr) (recorded == ExitSuccess)
  checkpointReceipt <- eitherDecode <$> BL.readFile (receipt </> "e2e-result.json")
  case checkpointReceipt of
    Right (Object fields) -> jsonFile (receipt </> "e2e-result.json") (Object (KM.insert "emulator_start"
      (object ["snapshot_manifest_sha256" .= T.replicate 64 "a", "snapshot_sha256" .= T.replicate 64 "b"]) fields))
    _ -> fail "generation fixture is not an object"
  expectPass "checkpoint-digest-receipt" []
  rewriteBaseline baselinePath legacyGeneration
  seedGenerationReceipt receipt (fixture 2.0)
  expectPass "generation-accepted"
    ["legacy-transport-success", "declared-trace-sites", "qualified-outcome", "runtime-symbols-sha256"]
  rewriteBaseline baselinePath (withBaselineOk (Bool False))
  seedGenerationReceipt receipt (fixture 2.0)
  expectDiff "generation-transport-false"
  rewriteBaseline baselinePath (withBaselineOk (String "true"))
  seedGenerationReceipt receipt (fixture 2.0)
  expectDiff "generation-transport-malformed"
  rewriteBaseline baselinePath legacyGeneration
  seedGenerationReceipt receipt (fixture 2.0) { fixtureMenuSha = T.replicate 64 "c" }
  expectDiff "generation-menu-drift"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureRendezvousMatched = False }
  expectDiff "generation-state-drift"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureExtra = Just (Bool True) }
  expectDiff "generation-unrelated-field"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureOk = Just (Bool True) }
  rewriteBaseline baselinePath (withoutBaselineOk . legacyGeneration)
  expectDiff "generation-run-transport-flag"
  rewriteBaseline baselinePath legacyGeneration
  seedGenerationReceipt receipt (fixture 2.0) { fixtureOutcome = "mystery" }
  expectProblem "generation-outcome-drift"
  seedGenerationReceipt receipt (fixture 2.0)
    { fixtureDeclared = [object ["label" .= String "frames-film-class-entry", "address" .= String "00e4241"]] }
  expectProblem "generation-trace-address"
  seedGenerationReceipt receipt (fixture 2.0)
    { fixtureDeclared = [ site
      , object ["label" .= String "frames-film-class-result", "address" .= String "00e42414"] ] }
  expectProblem "generation-trace-duplicate"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureHitSite = "frames-film-class-result" }
  expectProblem "generation-trace-hit"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureSymbols = String "not-a-digest" }
  expectProblem "generation-symbols-malformed"
  seedGenerationReceipt receipt (fixture 2.0) { fixtureSymbols = String (T.replicate 64 "b") }
  expectProblem "generation-symbols-provenance"
  rewriteBaseline baselinePath legacyGeneration
  seedGenerationReceipt receipt (fixture 2.0) { fixtureDeclared = [], fixtureObservationHit = True }
  expectProblem "generation-empty-declaration"

mixedGenerationChecks :: FilePath -> IO ()
mixedGenerationChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baselines = root </> "private-data/baselines"
      baselinePath = baselines </> "ram-push-volatile.json"
      output name = root </> ("private-data/" ++ name)
      kernelBytes = BC.pack "synthetic kernel symbols\n"
      symbolsDigest = T.pack (sha256Hex (sha256 kernelBytes))
      site = object ["label" .= String "frames-film-class-entry", "address" .= String "00e42414"]
      hit = object ["site" .= String "frames-film-class-entry", "core" .= (0 :: Int)
                   , "instructions" .= (10 :: Int), "r0" .= String "0x00000000"]
      segmentValue :: [Value] -> [Value] -> Value
      segmentValue declared hits = object
        [ "menu_receipts" .= [object ["label" .= String "menu"
            , "menu" .= object ["menu_id" .= (1 :: Int), "setting_id" .= (2 :: Int), "cursor" .= (3 :: Int)]
            , "sha256" .= String (T.replicate 64 "a")]]
        , "observations" .= [object ["phase" .= String "menu"
            , "steps" .= [object ["key" .= String "menu", "expected" .= ([1, 2, 3] :: [Int])
                                 , "actual" .= ([1, 2, 3] :: [Int]), "matched" .= True
                                 , "seconds" .= (1.0 :: Double)]]
            , "trace_hits" .= hits]]
        , "outcome" .= String "candidate-qualified"
        , "declared_trace_sites" .= declared
        , "runtime" .= object ["symbols_sha256" .= String symbolsDigest] ]
      seedSegment :: String -> [Value] -> [Value] -> [Value] -> IO ()
      seedSegment name declared hits siblingHits = do
        let directory = receipt </> ("segment-" ++ name)
        createDirectoryIfMissing True directory
        jsonFile (directory </> "e2e-result.json") (segmentValue declared hits)
        jsonFile (directory </> "e2e-observation-0.json")
          (object ["phase" .= String "menu", "trace_hits" .= siblingHits])
      seed :: [Value] -> IO ()
      seed siblingHits = do
        createDirectoryIfMissing True (receipt </> "relinked")
        B.writeFile (receipt </> "relinked/kernel.syms") kernelBytes
        seedSegment "a" [site] [hit] [hit]
        seedSegment "b" [] [] siblingHits
        jsonFile (receipt </> "e2e-segments.json") (toJSON
          [ object ["segment" .= String "a", "seconds" .= (0 :: Double), "result" .= String "passed", "message" .= Null]
          , object ["segment" .= String "b", "seconds" .= (0 :: Double), "result" .= String "passed", "message" .= Null] ])
        jsonFile (earlier </> "report.json") (object ["routes" .= [object
          ["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double)
          , "log" .= String "/synthetic.log", "exit_code" .= (0 :: Int)]]])
      detail name = do
        parsed <- eitherDecode <$> BL.readFile (output name </> "report.json") :: IO (Either String GenerationReport)
        pure $ case parsed of
          Left problem -> Left problem
          Right (GenerationReport (entry : _)) -> Right entry
          Right (GenerationReport []) -> Left "the report holds no route"
      segmentBGeneration (Field (FieldName file pointer) _) =
        file == "segment-b/e2e-result.json" && case pointer of
          Just [Key "outcome"] -> True
          Just (Key "declared_trace_sites" : _) -> True
          Just [Key "runtime", Key "symbols_sha256"] -> True
          _ -> False
  inherited <- getEnvironment
  let environment = Just (("BUILD_WORKSPACE_DIRECTORY", root)
        : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
      run name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "ram-push-volatile", "--from-run", earlier, "--baselines", baselines
         , "--output", output name] ++ extra))
        { cwd = Just root, env = environment }) ""
  seed []
  (recorded, _, recordErr) <- run "mixed-record" ["--record-baseline"]
  check ("a mixed-generation segmented fixture records: " ++ recordErr) (recorded == ExitSuccess)
  rewriteBaseline baselinePath $ \baseline ->
    baseline { baselineFields = filter (not . segmentBGeneration) (baselineFields baseline) }
  (passCode, _, passErr) <- run "mixed-pass" []
  passDetail <- detail "mixed-pass"
  check ("a mixed-generation segmented receipt projects each segment: " ++ passErr ++ " " ++ show passDetail)
    (passCode == ExitSuccess && case passDetail of
       Right (GenerationDetail result kinds _ differences) ->
         result == "pass" && differences == 0
           && all (`elem` kinds) ["declared-trace-sites", "qualified-outcome", "runtime-symbols-sha256"]
       Left _ -> False)
  seed [hit]
  (failCode, _, failErr) <- run "mixed-empty-declaration" []
  failDetail <- detail "mixed-empty-declaration"
  check ("an undeclared trace hit in a sibling observation fails: " ++ failErr ++ " " ++ show failDetail)
    (failCode == ExitFailure 1 && case failDetail of
       Right (GenerationDetail result _ problem _) ->
         result == "fail" && maybe False (not . T.null) problem
       Left _ -> False)

segmentedSymbolChecks :: FilePath -> IO ()
segmentedSymbolChecks tool = withWorkspace $ \root -> do
  let earlier = root </> "private-data/earlier"
      receipt = earlier </> "receipts/ram-push-volatile"
      baselines = root </> "private-data/baselines"
      baselinePath = baselines </> "ram-push-volatile.json"
      output name = root </> ("private-data/" ++ name)
      kernelBytes = BC.pack "segment-a kernel symbols\n"
      symbolsDigest = T.pack (sha256Hex (sha256 kernelBytes))
      wrongDigest = T.replicate 64 "b"
      receiptValue symbols = object
        [ "outcome" .= String "candidate-qualified"
        , "runtime" .= object ["symbols_sha256" .= String symbols]
        ]
      seed symbols = do
        createDirectoryIfMissing True (receipt </> "segment-a/relinked")
        B.writeFile (receipt </> "segment-a/relinked/kernel.syms") kernelBytes
        jsonFile (receipt </> "segment-a/e2e-result.json") (receiptValue symbols)
        jsonFile (receipt </> "e2e-segments.json") (toJSON
          [ object ["segment" .= String "a", "seconds" .= (0 :: Double), "result" .= String "passed", "message" .= Null] ])
        jsonFile (earlier </> "report.json") (object ["routes" .= [object
          ["route" .= String "ram-push-volatile", "seconds" .= (0 :: Double)
          , "log" .= String "/synthetic.log", "exit_code" .= (0 :: Int)]]])
      detail name = do
        parsed <- eitherDecode <$> BL.readFile (output name </> "report.json") :: IO (Either String GenerationReport)
        pure $ case parsed of
          Left problem -> Left problem
          Right (GenerationReport (entry : _)) -> Right entry
          Right (GenerationReport []) -> Left "the report holds no route"
  inherited <- getEnvironment
  let environment = Just (("BUILD_WORKSPACE_DIRECTORY", root)
        : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited)
      run name extra = readCreateProcessWithExitCode ((proc tool
        (["--routes", "ram-push-volatile", "--from-run", earlier, "--baselines", baselines
         , "--output", output name] ++ extra))
        { cwd = Just root, env = environment }) ""
      isRuntime (Field (FieldName "segment-a/e2e-result.json" (Just [Key "runtime", Key "symbols_sha256"])) _) = True
      isRuntime _ = False
  seed symbolsDigest
  (recorded, _, recordErr) <- run "symbol-record" ["--record-baseline"]
  check ("a no-root segmented symbol baseline records: " ++ recordErr) (recorded == ExitSuccess)
  baselineBytes <- B.readFile baselinePath
  baseline <- either (\problem -> ioError (userError problem)) pure (decodeBaseline baselineBytes)
  B.writeFile baselinePath (encodeBaseline baseline { baselineFields = filter (not . isRuntime) (baselineFields baseline) })
  seed wrongDigest
  (code, _, err) <- run "symbol-wrong" []
  parsed <- detail "symbol-wrong"
  check ("a no-root segmented symbol digest must match the segment file: " ++ err ++ " " ++ show parsed)
    (code == ExitFailure 1 && case parsed of
       Right (GenerationDetail result _ problem _) ->
         result == "fail" && maybe False (T.isInfixOf "segment-a/relinked/kernel.syms") problem
       _ -> False)

main :: IO ()
main = do
  [toolArgument] <- getArgs
  tool <- makeAbsolute toolArgument
  let expect route evidence source parity qualification expected = do
        result <- verify tool route evidence source parity qualification
        check (show (route, evidence, source, parity, qualification) ++ " reports " ++ show expected) (result == expected)
      qualified = QualifiedOutcome
  expect RamPushVolatile MissingExit EarlierRun EqualReceipts qualified Failed
  expect RamPushVolatile NullExit EarlierRun EqualReceipts qualified Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) ReceiptOverride EqualReceipts qualified Failed
  expect RamPushVolatile (RecordedExit (ExitFailure 1)) EarlierRun EqualReceipts qualified Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts qualified Passed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun DifferentReceipts qualified Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts MissingOutcome Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts ControlReceipt Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts UnknownOutcome Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts RefusedOutcome Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedQualified Passed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun DifferentReceipts SegmentedMenuDiffers Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun DifferentReceipts SegmentedObservationDiffers Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts CounterpartControl Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts CounterpartFailedCommand Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedFailed Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedMalformed Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedDuplicateName Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedPathName Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts PassedWithMessage Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts FailedWithoutMessage Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts MissingMessage Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts NegativeSeconds Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts OverflowSeconds Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts ExtraKey Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts UnknownResult Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedMissingChild Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts EmptySegments Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts FlatPlusSegmented Failed
  recorded <- recordBaseline tool (RecordedExit ExitSuccess) QualifiedOutcome
  check ("a known-success receipt records the baseline: " ++ show recorded)
    (case recorded of (ExitSuccess, Just value) -> routeOf value == Just (T.pack "ram-push-volatile"); _ -> False)
  controlled <- recordBaseline tool (RecordedExit ExitSuccess) ControlReceipt
  check ("a control receipt does not record a baseline: " ++ show controlled)
    (case controlled of (ExitFailure _, Nothing) -> True; _ -> False)
  unknown <- recordBaseline tool (RecordedExit ExitSuccess) UnknownOutcome
  check ("an unknown outcome does not record a baseline: " ++ show unknown)
    (case unknown of (ExitFailure _, Nothing) -> True; _ -> False)
  failedRecord <- recordBaseline tool (RecordedExit (ExitFailure 1)) QualifiedOutcome
  check ("a failed command does not record a baseline: " ++ show failedRecord)
    (case failedRecord of (ExitFailure _, Nothing) -> True; _ -> False)
  unknownExit <- recordBaseline tool MissingExit QualifiedOutcome
  check ("an unknown command exit does not record a baseline: " ++ show unknownExit)
    (case unknownExit of (ExitFailure _, Nothing) -> True; _ -> False)
  nullExit <- recordBaseline tool NullExit QualifiedOutcome
  check ("a null command exit does not record a baseline: " ++ show nullExit)
    (case nullExit of (ExitFailure _, Nothing) -> True; _ -> False)
  segmentedBaselineChecks tool
  nativeBaselineChecks tool
  kernelStateChecks tool
  segmentedSymbolChecks tool
  nativeRunnerTransitionChecks tool
  generationChecks tool
  mixedGenerationChecks tool
  ledgerChecks tool
  (replaceExit, before, after, secondReport) <- recordReplacement tool
  check ("a failed record reports failure and does not replace an existing baseline: " ++ show replaceExit)
    (replaceExit /= ExitSuccess && before == after && reportResult secondReport == Just "fail")
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedWithControl Failed
  expect UsbToggle MissingExit EarlierRun EqualReceipts qualified Failed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun EqualReceipts qualified Passed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun DifferentReceipts qualified Failed
  slotChecks tool
  storageChecks tool

slotChecks :: FilePath -> IO ()
slotChecks tool = bracket (setEnv "XM1_EMULATOR_SLOT" "1") (const (unsetEnv "XM1_EMULATOR_SLOT")) $ \() -> do
  withWorkspace $ \root -> do
    inherited <- getEnvironment
    let output = root </> "private-data/in-slot"
    (code, _, _) <- readCreateProcessWithExitCode ((proc tool ["--routes", "ram-push-volatile", "--output", output])
      { cwd = Just root
      , env = Just (("BUILD_WORKSPACE_DIRECTORY", root) : filter ((/= "BUILD_WORKSPACE_DIRECTORY") . fst) inherited) }) ""
    created <- doesDirectoryExist output
    check "a run that would execute a route inside a slot is refused" (code /= ExitSuccess && not created)
  judged <- verify tool RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts QualifiedOutcome
  check "an earlier run's receipts are still judged inside a slot" (judged == Passed)

storageChecks :: FilePath -> IO ()
storageChecks tool = bracket (lookupEnv "XM1_STORAGE_MIN_FREE")
  (maybe (unsetEnv "XM1_STORAGE_MIN_FREE") (setEnv "XM1_STORAGE_MIN_FREE")) $ \_ -> do
    setEnv "XM1_STORAGE_MIN_FREE" "999999999999999999999T"
    withWorkspace $ \root -> do
      let detached = root </> "detached-verifier"
          output = root </> "private-data/refused"
      copyFile tool detached
      setFileMode detached 0o700
      inherited <- getEnvironment
      let environment = ("BUILD_WORKSPACE_DIRECTORY", root)
            : filter ((`notElem` ["BUILD_WORKSPACE_DIRECTORY", "XM1_EMULATOR_SLOT"]) . fst) inherited
      (code, _, _) <- readCreateProcessWithExitCode
        (proc detached ["--routes", "native-sd", "--output", output]) {cwd = Just root, env = Just environment} ""
      created <- doesDirectoryExist output
      check "insufficient disk reserve refuses a new run before output creation" (code /= ExitSuccess && not created)
      (helpCode, _, _) <- readCreateProcessWithExitCode (proc detached ["--help"]) {env = Just environment} ""
      check "help remains available below the disk reserve" (helpCode == ExitSuccess)
    judged <- verify tool RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts QualifiedOutcome
    check "offline receipt judging remains available below the disk reserve" (judged == Passed)
