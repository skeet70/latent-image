{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, displayException, try)
import Control.Monad (foldM, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, withObject, withText, (.:), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Text.Encoding (decodeUtf8')
import Data.Word (Word32)
import Emulation.ObservationStream
  ( ObservationRef, VerifiedObservation (..), decodeObservationRef
  , foldTraceHitRows, loadVerifiedObservation )
import Numeric (readHex, showHex)
import Numeric.Natural (Natural)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), takeDirectory)
import System.IO (hPutStrLn, stderr)
import System.Process (readProcessWithExitCode)

data Option = Jq | Checker | Demo | NoExecutors | NoPenderKernel | CounterDown | Objects deriving (Eq, Ord)
newtype ReceiptDirectory = ReceiptDirectory FilePath
newtype ProgramPath = ProgramPath FilePath
newtype CheckerPath = CheckerPath FilePath
newtype ObjectDigestsPath = ObjectDigestsPath FilePath
newtype TaskId = TaskId Word32 deriving (Eq, Ord)
newtype WakeCount = WakeCount Natural
newtype WakeCounts = WakeCounts (M.Map TaskId WakeCount)
newtype ReceiptDocument = ReceiptDocument Value
newtype ObservationMetadata = ObservationMetadata Value

data Options = Options
  { jqProgram :: ProgramPath
  , checkerProgram :: CheckerPath
  , demoDirectory :: ReceiptDirectory
  , noExecutorsDirectory :: ReceiptDirectory
  , noPenderDirectory :: ReceiptDirectory
  , counterDownDirectory :: ReceiptDirectory
  , objectDigestsPath :: ObjectDigestsPath
  }
data ObservationInput = Embedded Value | Referenced ObservationRef
data WakeEvent = KernelWake TaskId | OtherEvent

data DemoEvidence = DemoEvidence
  { evidenceReceipt :: !ReceiptDocument
  , evidenceObservations :: ![ObservationMetadata]
  , evidenceWakeCounts :: !WakeCounts
  }

main :: IO ()
main = do
  result <- try (getArgs >>= options >>= run) :: IO (Either IOException ())
  case result of
    Left problem -> hPutStrLn stderr (displayException problem) >> exitFailure
    Right () -> pure ()

options :: [String] -> IO Options
options arguments = do
  paths <- either refuse pure (parseArguments M.empty arguments)
  let required key = maybe (refuse "missing required checker argument") pure (M.lookup key paths)
      file key = do
        path <- makeAbsolute =<< required key
        present <- doesFileExist path
        unless present (refuse ("input file is missing: " ++ path))
        pure path
      directory key = do
        path <- canonicalizePath =<< required key
        present <- doesDirectoryExist path
        unless present (refuse ("receipt directory is missing: " ++ path))
        pure (ReceiptDirectory path)
      runtimeFile key = do
        executable <- getExecutablePath
        relative <- required key
        let path = (executable ++ ".runfiles") </> relative
        present <- doesFileExist path
        unless present (refuse ("checker runfile is missing: " ++ path))
        pure path
  Options <$> (ProgramPath <$> runtimeFile Jq) <*> (CheckerPath <$> runtimeFile Checker)
    <*> directory Demo <*> directory NoExecutors <*> directory NoPenderKernel <*> directory CounterDown
    <*> (ObjectDigestsPath <$> file Objects)

parseArguments :: M.Map Option FilePath -> [String] -> Either String (M.Map Option FilePath)
parseArguments found [] = Right found
parseArguments found (flag : path : rest) = do
  key <- case flag of
    "--jq" -> Right Jq
    "--checker" -> Right Checker
    "--demo" -> Right Demo
    "--no-executors" -> Right NoExecutors
    "--no-pender-kernel" -> Right NoPenderKernel
    "--counter-down" -> Right CounterDown
    "--objects" -> Right Objects
    _ -> Left ("unknown checker option: " ++ flag)
  if M.member key found then Left ("duplicate checker option: " ++ flag)
    else parseArguments (M.insert key path found) rest
parseArguments _ [_] = Left "checker option needs a path"

run :: Options -> IO ()
run given = do
  demo <- loadEvidence (demoDirectory given)
  noExecutors <- loadEvidence (noExecutorsDirectory given)
  noPender <- loadEvidence (noPenderDirectory given)
  counterDown <- loadEvidence (counterDownDirectory given)
  let ObjectDigestsPath digestsPath = objectDigestsPath given
  objects <- either (refuse . show) pure . decodeUtf8' =<< B.readFile digestsPath
  let input = object
        [ "demo" .= [evidenceValue demo], "no_executors" .= [evidenceValue noExecutors]
        , "no_pender_kernel" .= [evidenceValue noPender], "counter_down" .= [evidenceValue counterDown]
        , "objects" .= objects ]
      ProgramPath jq = jqProgram given
      CheckerPath checker = checkerProgram given
  (status, output, problem) <- readProcessWithExitCode jq ["-L", takeDirectory checker, "-f", checker] (BL.unpack (encode input))
  unless (status == ExitSuccess) (refuse problem)
  verdict <- either refuse pure (eitherDecodeStrict' (BL.toStrict (BL.pack output)))
  accepted <- either refuse pure (parseEither (withObject "demo judgement" (.: "verdict")) verdict)
  putStr output
  unless accepted exitFailure

loadEvidence :: ReceiptDirectory -> IO DemoEvidence
loadEvidence (ReceiptDirectory directory) = do
  bytes <- B.readFile (directory </> "e2e-result.json")
  value <- either refuse pure (eitherDecodeStrict' bytes)
  (receipt, inputs) <- either refuse pure (parseEither parseReceipt value)
  (metadata, counts) <- foldM consume ([], WakeCounts M.empty) inputs
  pure (DemoEvidence receipt (reverse metadata) counts)
 where
  consume (metadata, counts) input = do
    (sourceDirectory, document) <- case input of
      Embedded document -> pure (directory, document)
      Referenced ref -> do
        verified <- either refuse pure =<< loadVerifiedObservation directory ref
        pure (takeDirectory (verifiedObservationPath verified), verifiedObservationValue verified)
    updated <- either refuse pure =<< foldTraceHitRows sourceDirectory document countWake counts
    header <- either refuse pure (parseEither (withObject "observation" (pure . ObservationMetadata . Object . KM.delete "trace_hits")) document)
    pure (header : metadata, updated)

parseReceipt :: Value -> Parser (ReceiptDocument, [ObservationInput])
parseReceipt = withObject "e2e receipt" $ \fields -> do
  observations <- fields .: "observations"
  inputs <- mapM parseInput observations
  pure (ReceiptDocument (Object (KM.delete "observations" fields)), inputs)
 where
  parseInput value@(Object fields)
    | KM.member "path" fields = Referenced <$> either fail pure (decodeObservationRef value)
    | otherwise = pure (Embedded value)
  parseInput _ = fail "observation must be an object"

countWake :: WakeCounts -> Value -> Either String WakeCounts
countWake counts@(WakeCounts values) value = do
  event <- parseEither parseEvent value
  pure $ case event of
    OtherEvent -> counts
    KernelWake task -> WakeCounts (M.insertWith add task (WakeCount 1) values)
 where
  add (WakeCount left) (WakeCount right) = WakeCount (left + right)
  parseEvent = withObject "trace hit" $ \fields -> do
    site <- fields .: "site"
    if T.isPrefixOf "wakeup-" site
      then KernelWake <$> (fields .: "r0" >>= parseTaskId)
      else pure OtherEvent

parseTaskId :: Value -> Parser TaskId
parseTaskId = withText "task id register" $ \text -> case T.stripPrefix "0x" text of
  Nothing -> fail "task id register requires hexadecimal"
  Just digits -> case readHex (T.unpack digits) of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (TaskId (fromInteger value))
    _ -> fail "invalid task id register"

evidenceValue :: DemoEvidence -> Value
evidenceValue evidence = object
  [ "receipt" .= receipt
  , "observations" .= [value | ObservationMetadata value <- observations]
  , "wake_counts" .= Object (KM.fromList [(K.fromText (taskText task), toCount count) | (task, count) <- M.toList counts])
  ]
 where
  ReceiptDocument receipt = evidenceReceipt evidence
  observations = evidenceObservations evidence
  WakeCounts counts = evidenceWakeCounts evidence
  taskText (TaskId value) = let digits = showHex value "" in T.pack ("0x" ++ replicate (8 - length digits) '0' ++ digits)
  toCount (WakeCount value) = Number (fromIntegral value)

refuse :: String -> IO a
refuse = ioError . userError
