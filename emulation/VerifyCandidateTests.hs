{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Value (..), eitherDecode, encode, object, withObject, withText, (.:), (.=))
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

verify :: FilePath -> Route -> RecordedExit -> ReceiptSource -> ReceiptParity -> IO Result
verify tool route evidence source parity = withWorkspace $ \root -> do
  let (candidate, counterpart) = routeNames route
      earlier = root </> "private-data/earlier"
      receipt name = earlier </> "receipts" </> name
      output = root </> "private-data/result"
      exitFields = case evidence of
        MissingExit -> []
        NullExit -> ["exit_code" .= (Nothing :: Maybe Int)]
        RecordedExit code -> ["exit_code" .= case code of ExitSuccess -> 0 :: Int; ExitFailure n -> n]
      seed directory fixtureParity = do
        createDirectoryIfMissing True directory
        case route of
          RamPushVolatile -> do
            let value = case fixtureParity of EqualReceipts -> 1 :: Int; DifferentReceipts -> 2
            jsonFile (directory </> "e2e-observation-0.json")
              (object ["schema" .= (1 :: Int), "reads" .= [object ["value" .= value]]])
            jsonFile (directory </> "e2e-result.json")
              (object ["menu_receipts" .= [object ["menu_id" .= (1 :: Int)]]])
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
  seed (receipt counterpart) EqualReceipts
  seed (receipt candidate) parity
  jsonFile (earlier </> "report.json") (object ["routes" .= [object
    (["route" .= candidate, "seconds" .= (0 :: Double), "log" .= (earlier </> "synthetic.log")] ++ exitFields)]])
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
  let expect route evidence source parity expected = do
        result <- verify tool route evidence source parity
        check (show (route, evidence, source, parity) ++ " reports " ++ show expected) (result == expected)
  expect RamPushVolatile MissingExit EarlierRun EqualReceipts Failed
  expect RamPushVolatile NullExit EarlierRun EqualReceipts Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) ReceiptOverride EqualReceipts Failed
  expect RamPushVolatile (RecordedExit (ExitFailure 1)) EarlierRun EqualReceipts Failed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun EqualReceipts Passed
  expect RamPushVolatile (RecordedExit ExitSuccess) EarlierRun DifferentReceipts Failed
  expect UsbToggle MissingExit EarlierRun EqualReceipts Passed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun EqualReceipts Passed
  expect UsbToggle (RecordedExit (ExitFailure 1)) EarlierRun DifferentReceipts Failed
