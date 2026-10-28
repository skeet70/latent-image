{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Value (..), eitherDecode, encode, object, toJSON, withObject, withText, (.:), (.=))
import qualified Data.ByteString.Lazy as BL
import System.Directory (createDirectory, createDirectoryIfMissing, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
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
  | SegmentedQualified | SegmentedWithControl | SegmentedFailed | SegmentedMalformed | SegmentedDuplicateName
  | SegmentedPathName | CounterpartControl | CounterpartFailedCommand
  | PassedWithMessage | FailedWithoutMessage | MissingMessage | NegativeSeconds | OverflowSeconds
  | ExtraKey | UnknownResult deriving (Show)
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
      temporary <- getTemporaryDirectory
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

qualificationWord :: Qualification -> Maybe String
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
      seed directory fixtureParity side = do
        createDirectoryIfMissing True directory
        case route of
          RamPushVolatile -> do
            let value = case fixtureParity of EqualReceipts -> 1 :: Int; DifferentReceipts -> 2
                writeOutcome path word = jsonFile path (object
                  [ "menu_receipts" .= [object ["menu_id" .= (1 :: Int)]]
                  , "outcome" .= String word])
                segmentRecord name result message = object
                  [ "segment" .= String name
                  , "seconds" .= (0 :: Double)
                  , "result" .= String result
                  , "message" .= message ]
                segments names = do
                  jsonFile (directory </> "e2e-segments.json")
                    (toJSON (map (\name -> segmentRecord name "passed" Null) names))
                  mapM_ (\name -> do
                    let segment = directory </> ("segment-" ++ name)
                    createDirectoryIfMissing True segment
                    writeOutcome (segment </> "e2e-result.json") "candidate-qualified") names
            jsonFile (directory </> "e2e-observation-0.json")
              (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= value]]])
            case qualification of
              MissingOutcome -> pure ()
              ControlReceipt -> jsonFile (directory </> "e2e-control.json")
                (object ["outcome" .= String "control-refused"])
              SegmentedQualified -> segments ["service-field-list"]
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
              CounterpartControl
                | side == CounterpartSide -> jsonFile (directory </> "e2e-control.json")
                    (object ["outcome" .= String "control-refused"])
                | otherwise -> writeOutcome (directory </> "e2e-result.json") "candidate-qualified"
              CounterpartFailedCommand -> writeOutcome (directory </> "e2e-result.json") "candidate-qualified"
              _ -> case qualificationWord qualification of
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
  seed (receipt counterpart) EqualReceipts CounterpartSide
  seed (receipt candidate) parity CandidateSide
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
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts SegmentedWithControl Failed
  expect UsbToggle MissingExit EarlierRun EqualReceipts qualified Passed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun EqualReceipts qualified Passed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun DifferentReceipts qualified Failed
