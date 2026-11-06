{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Exception (bracket_)
import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import FujiAF.Host (get, runBounded, str, withTemporary)
import Replacement.PropertyService.Qualification
import System.Directory (createDirectoryIfMissing, createDirectoryLink, doesFileExist, doesPathExist, makeAbsolute, withCurrentDirectory)
import System.Environment (getArgs, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..))
import System.FilePath (takeFileName, (</>))

check :: String -> Bool -> IO ()
check label holds = do
  putStrLn ((if holds then "ok   " else "FAIL ") ++ label)
  unless holds (ioError (userError ("assertion failed: " ++ label)))

dropField :: String -> Value -> Value
dropField key (Object objectValue) = Object (KM.delete (K.fromString key) objectValue)
dropField _ value = value

replaceField :: String -> Value -> Value -> Value
replaceField key replacement (Object objectValue) = Object (KM.insert (K.fromString key) replacement objectValue)
replaceField _ _ value = value

sampleDifference :: Value
sampleDifference = object ["case" .= ("case-1" :: String), "kind" .= ("status" :: String), "detail" .= ("differs" :: String)]

sampleControl :: Value
sampleControl =
  object
    [ "name" .= ("c-stock-swapped" :: String)
    , "kind" .= ("must-fail" :: String)
    , "note" .= ("stock default swapped" :: String)
    , "cases" .= (3 :: Int)
    , "failed_cases" .= (1 :: Int)
    , "holds" .= False
    , "first_differences" .= [sampleDifference]
    ]

sampleShape :: Value
sampleShape = object ["subset" .= True, "ids" .= ("0x1 0x2" :: String), "cases_per_role" .= (160 :: Int), "grid_cases" .= (80 :: Int)]

sampleExecution :: Value
sampleExecution =
  object
    [ "cache" .= ("/cache" :: String)
    , "batches_executed" .= (1 :: Int)
    , "batches_reused" .= (2 :: Int)
    , "cases_executed" .= (10 :: Int)
    , "cases_reused" .= (20 :: Int)
    ]

validSummary :: Value
validSummary =
  object
    [ "schema_version" .= (1 :: Int)
    , "passed" .= True
    , "case_shape" .= sampleShape
    , "execution" .= sampleExecution
    , "controls" .= [sampleControl]
    , "differences" .= object ["model:status" .= [sampleDifference]]
    , "inputs" .= object ["image4" .= object ["path" .= ("/image4" :: String), "sha256" .= ("aa" :: String)]]
    , "runner" .= object ["path" .= ("/runner" :: String), "sha256" .= ("bb" :: String)]
    , "contract_staged_image4_sha256" .= ("cc" :: String)
    ]

parseTests :: IO ()
parseTests = do
  case parseOracleSummary validSummary of
    Left problem -> check ("complete oracle summary parses: " ++ problem) False
    Right summary -> do
      check "parsed pass and subset scope" (summaryPassed summary && summarySubset summary)
      check "parsed subset ids" (summaryIds summary == ["0x1", "0x2"])
      check "parsed reuse counts" (summaryReusedBatches summary == 2 && summaryReusedCases summary == 20)
      check "parsed control failure" (case summaryControls summary of [control] -> not (controlHolds control) && controlFailedCases control == 1 && length (controlFirstDifferences control) == 1; _ -> False)
      check "parsed differences" (case summaryDifferences summary of [("model:status", [sample])] -> sampleDetail sample == "differs"; _ -> False)
      check "parsed input identity" (case M.lookup "image4" (summaryInputs summary) of Just (_, digest) -> digest == "aa"; Nothing -> False)
      check "parsed runner identity" (summaryRunnerPath summary == "/runner" && summaryRunnerSha256 summary == "bb")
  check "a summary without case_shape is refused" (isLeft (parseOracleSummary (dropField "case_shape" validSummary)))
  check "a summary without subset is refused" (isLeft (parseOracleSummary (replaceField "case_shape" (dropField "subset" sampleShape) validSummary)))
  check "a summary without cases_reused is refused" (isLeft (parseOracleSummary (replaceField "execution" (dropField "cases_reused" sampleExecution) validSummary)))
  check "a summary without controls is refused" (isLeft (parseOracleSummary (dropField "controls" validSummary)))
  check "a summary without runner sha256 is refused" (isLeft (parseOracleSummary (replaceField "runner" (object ["path" .= ("/runner" :: String)]) validSummary)))
  check "a string passed field is refused" (isLeft (parseOracleSummary (replaceField "passed" (String "true") validSummary)))
  check "a zero-batch summary is refused" (isLeft (parseOracleSummary (replaceField "execution" (object ["cache" .= ("/cache" :: String), "batches_executed" .= (0 :: Int), "batches_reused" .= (0 :: Int), "cases_executed" .= (0 :: Int), "cases_reused" .= (0 :: Int)]) validSummary)))

optionsTest :: IO ()
optionsTest =
  case parseOptions provided of
    Left problem -> check ("options parse: " ++ problem) False
    Right options -> check "a repeated --defect name keeps the later binary" (optDefects options == [("drop_provisional", "second.bin", "second.symbols")])
  where
    provided =
      required
        ++ ["--defect", "drop_provisional", "first.bin", "first.symbols", "--defect", "drop_provisional", "second.bin", "second.symbols"]
    required =
      [ "--dat", "dat", "--image4", "image4", "--loader-a", "loader-a", "--loader-b", "loader-b"
      , "--image5", "image5", "--plain", "plain", "--image2", "image2", "--linked", "linked"
      , "--symbols", "symbols", "--out", "out", "--cache", "cache", "--runner", "runner", "--emulator-slot", "slot"
      , "--oracle", "oracle", "--stage", "stage", "--stage-encoder", "encoder"
      , "--stage-rebuild", "rebuild", "--stage-decoder", "decoder"
      ]

pathTests :: FilePath -> IO ()
pathTests root = do
  let base = root </> "boundary"
      scratch = base </> "private-data" </> "scratch"
      inside = scratch </> "jobs" </> "job"
      outside = base </> "outside"
      link = scratch </> "link"
  createDirectoryIfMissing True inside
  createDirectoryIfMissing True outside
  createDirectoryLink outside link
  insideResult <- resolveWithin scratch inside
  outsideResult <- resolveWithin scratch outside
  traversalResult <- resolveWithin scratch (scratch </> "jobs" </> ".." </> ".." </> "outside")
  linkResult <- resolveWithin scratch link
  check "a path inside the resolved root is accepted" (either (const False) (const True) insideResult)
  check "a sibling path is refused" (isLeft outsideResult)
  check "a dot-dot path that leaves the root is refused" (isLeft traversalResult)
  check "a symlink that resolves outside the root is refused" (isLeft linkResult)

driverRefusal :: FilePath -> FilePath -> IO ()
driverRefusal binary root = do
  let directory = root </> "refusal"
      out = root </> "private-data" </> "scratch" </> "refusal"
  createDirectoryIfMissing True directory
  let names = ["dat", "image4", "loader-a", "loader-b", "image5", "plain", "image2", "linked", "symbols", "runner", "slot", "oracle", "stage", "encoder", "rebuild", "decoder"]
      flags = ["--dat", "--image4", "--loader-a", "--loader-b", "--image5", "--plain", "--image2", "--linked", "--symbols", "--runner", "--emulator-slot", "--oracle", "--stage", "--stage-encoder", "--stage-rebuild", "--stage-decoder"]
  paths <- mapM (\name -> do
    let path = directory </> name
    B.writeFile path B.empty
    pure path) names
  previousWorkspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let runtimeFlags = ["--linked", "--symbols", "--runner", "--emulator-slot", "--oracle", "--stage", "--stage-encoder", "--stage-rebuild", "--stage-decoder"]
      arguments = concat [[flag, if flag `elem` runtimeFlags then takeFileName path else path] | (flag, path) <- zip flags paths]
        ++ ["--out", out, "--cache", directory </> "cache"]
      restoreWorkspace = maybe (unsetEnv "BUILD_WORKSPACE_DIRECTORY") (setEnv "BUILD_WORKSPACE_DIRECTORY") previousWorkspace
      runDriver supplied = bracket_ (setEnv "BUILD_WORKSPACE_DIRECTORY" root) restoreWorkspace $
        withCurrentDirectory directory $ runBounded 60000000 (4 * 1024 * 1024) binary supplied B.empty
  createDirectoryIfMissing True (root </> "private-data" </> "scratch")
  (timedOut, exit, stdoutBytes, stderrBytes) <- runDriver arguments
  verdict <- case eitherDecodeStrict' stdoutBytes :: Either String Value of
    Left problem -> ioError (userError ("driver stdout is not JSON: " ++ problem ++ " stderr " ++ BC.unpack (B.take 200 stderrBytes)))
    Right value -> pure value
  check "the driver exits 2 for a noncanonical image4" (not timedOut && exit == ExitFailure 2)
  check "the terminal verdict reports a driver error" (str (get verdict "verdict") == "error" && str (get (get verdict "failure") "predicate") == "driver")
  pointerExists <- doesFileExist (out </> "current")
  current <- if pointerExists then B.readFile (out </> "current") else pure B.empty
  resultDirectory <- case eitherDecodeStrict' current :: Either String Value of
    Left problem -> ioError (userError ("current pointer is not JSON: " ++ problem))
    Right pointer -> pure (str (get pointer "result"))
  resultExists <- doesFileExist (resultDirectory </> "verdict.json")
  reviewExists <- doesFileExist (resultDirectory </> "review-packet.json")
  written <- if resultExists then B.readFile (resultDirectory </> "verdict.json") else pure B.empty
  writtenVerdict <- case eitherDecodeStrict' written :: Either String Value of
    Left problem -> ioError (userError ("published verdict is not JSON: " ++ problem))
    Right value -> pure value
  check "the current pointer names a result with both verdict and review" (pointerExists && resultExists && reviewExists)
  check "the published verdict is the current error" (str (get writtenVerdict "verdict") == "error")
  let outside = root </> "outside-refusal"
      outsideArguments = arguments ++ ["--out", outside]
  (_, outsideExit, _, _) <- runDriver outsideArguments
  outsideExists <- doesPathExist outside
  check "refusal cannot publish outside private scratch" (outsideExit == ExitFailure 2 && not outsideExists)

main :: IO ()
main = do
  arguments <- getArgs
  binary <- case arguments of
    path : _ -> makeAbsolute path
    [] -> ioError (userError "usage: qualify-tests QUALIFY_BINARY")
  withTemporary "qualify-tests" $ \root -> do
    parseTests
    optionsTest
    pathTests root
    driverRefusal binary root
