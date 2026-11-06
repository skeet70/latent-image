{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (isInfixOf)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, num, runBounded, str, withTemporary)
import FujiAF.JSON (encodeCanonical)
import Replacement.PropertyService.Qualification
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))

check :: String -> Bool -> IO Bool
check label holds = do
  putStrLn ((if holds then "ok   " else "FAIL ") ++ label)
  pure holds

refused :: Either String a -> Bool
refused result = case result of
  Left _ -> True
  Right _ -> False

boolOf :: Value -> Bool
boolOf (Bool value) = value
boolOf _ = False

writeStageFixture :: FilePath -> B.ByteString -> IO (Value, StageFiles)
writeStageFixture directory candidateBytes = do
  createDirectoryIfMissing True directory
  B.writeFile (directory </> "candidate.dat") candidateBytes
  B.writeFile (directory </> "contract.json") "{\"schema\":1}"
  B.writeFile (directory </> "image4.bin") (B.replicate 16 7)
  let identity = object ["schema_version" .= (1 :: Int), "probe" .= ("stage-fixture" :: String)]
  writeStageManifest
    directory
    identity
    [ ("candidate_dat", directory </> "candidate.dat")
    , ("contract", directory </> "contract.json")
    , ("image4", directory </> "image4.bin")
    ]
  pure (identity, StageFiles (directory </> "candidate.dat") (directory </> "contract.json") (directory </> "image4.bin") (sha256Hex (sha256 candidateBytes)) "" "")

stageAcceptance :: FilePath -> IO Bool
stageAcceptance root = do
  let directory = root </> "accept"
  (identity, _) <- writeStageFixture directory "candidate-bytes"
  verified <- verifyStage directory identity
  case verified of
    Left problem -> check ("completed stage is reused: " ++ problem) False
    Right files ->
      check
        "a completed stage reports the recorded candidate digest"
        (stageCandidateDatSha256 files == sha256Hex (sha256 "candidate-bytes"))

stageCorruption :: FilePath -> IO Bool
stageCorruption root = do
  let directory = root </> "corruption"
  (identity, _) <- writeStageFixture directory "candidate-bytes"
  B.writeFile (directory </> "candidate.dat") "tampered"
  tampered <- verifyStage directory identity
  B.writeFile (directory </> "candidate.dat") "candidate-bytes"
  B.writeFile (directory </> "contract.json") "{\"schema\":1}truncated"
  truncated <- verifyStage directory identity
  B.writeFile (directory </> "contract.json") "{\"schema\":1}"
  writeStageManifest directory (object ["schema_version" .= (1 :: Int), "probe" .= ("other" :: String)]) [("candidate_dat", directory </> "candidate.dat"), ("contract", directory </> "contract.json"), ("image4", directory </> "image4.bin")]
  replaced <- verifyStage directory identity
  check "tampered candidate bytes are refused" (refused tampered)
  check "truncated contract bytes are refused" (refused truncated)
  check "a stage manifest of another identity is refused" (refused replaced)

stageResume :: FilePath -> IO Bool
stageResume root = do
  let partial = root </> "partial"
      final = root </> "final"
  createDirectoryIfMissing True partial
  B.writeFile (partial </> "candidate.dat") "unfinished"
  (identity, _) <- writeStageFixture (root </> "complete") "complete-bytes"
  interrupted <- verifyStage partial identity
  committed <- commitStage partial final
  installed <- doesFileExist (final </> "candidate.dat")
  installedCheck <- verifyStage final identity
  let temporary = root </> "second"
  _ <- writeStageFixture temporary "different-bytes"
  second <- commitStage temporary final
  bytes <- B.readFile (final </> "candidate.dat")
  check "an interrupted stage has no completed manifest" (refused interrupted)
  check "an interrupted stage is never verified as complete" (committed == Right True && installed && refused installedCheck)
  check "an existing completed stage is not overwritten" (second == Right False && bytes == "unfinished")

stageCommit :: FilePath -> IO Bool
stageCommit root = do
  let temporary = root </> "commit-source"
      final = root </> "commit-final"
  (identity, _) <- writeStageFixture temporary "committed-bytes"
  committed <- commitStage temporary final
  verified <- verifyStage final identity
  check "a completed stage is published atomically" (committed == Right True && not (refused verified))

testView :: FilePath -> String -> JobView
testView root stageStatus =
  JobView
    { jobOptions =
        defaultOptions
          { optOut = root </> "out"
          , optCache = root </> "cache"
          , optIds = Just "0x1"
          }
    , jobKey = "job-key"
    , jobStageKey = "stage-key"
    , jobIdentity =
        object
          [ "sources" .= object ["dat" .= ("aa" :: String)]
          , "candidate" .= object ["linked" .= ("bb" :: String), "symbols" .= ("cc" :: String)]
          , "runner" .= object ["path" .= ("runner" :: String), "sha256" .= ("dd" :: String)]
          , "oracle" .= object ["path" .= ("oracle" :: String), "sha256" .= ("ee" :: String)]
          ]
    , jobStageStatus = stageStatus
    , jobStageFiles = StageFiles "/stage/candidate.dat" "/stage/contract.json" "/stage/image4.bin" "11" "22" "33"
    , jobStageLogs = []
    , jobNotes = []
    }

attemptWith :: FilePath -> Int -> Maybe Value -> Attempt
attemptWith directory exit results =
  Attempt
    { attemptExit = exit
    , attemptTimedOut = False
    , attemptResults = results
    , attemptResultsError = ""
    , attemptDir = directory
    , attemptStdoutLog = directory </> "stdout.log"
    , attemptStderrLog = directory </> "stderr.log"
    }

failingResults :: Value
failingResults =
  object
    [ "passed" .= False
    , "case_shape" .= object ["subset" .= True, "ids" .= ("0x1 0x2" :: String), "cases_per_role" .= (160 :: Int), "grid_cases" .= (80 :: Int)]
    , "execution" .= object ["batches_executed" .= (1 :: Int), "batches_reused" .= (2 :: Int), "cases_executed" .= (10 :: Int), "cases_reused" .= (20 :: Int)]
    , "controls" .=
        [ object
            [ "name" .= ("c-stock-swapped" :: String)
            , "kind" .= ("must-fail" :: String)
            , "note" .= ("stock default swapped" :: String)
            , "cases" .= (3 :: Int)
            , "failed_cases" .= (1 :: Int)
            , "holds" .= False
            , "first_differences" .= [object ["case" .= ("c-stock-swapped-0x34-set" :: String), "kind" .= ("status" :: String), "detail" .= ("differs" :: String)]]
            ]
        ]
    , "differences" .= object []
    ]

passingResults :: Value
passingResults =
  object
    [ "passed" .= True
    , "case_shape" .= object ["subset" .= False, "ids" .= ("0..646" :: String), "cases_per_role" .= (160 :: Int), "grid_cases" .= (80 :: Int)]
    , "execution" .= object ["batches_executed" .= (0 :: Int), "batches_reused" .= (3 :: Int), "cases_executed" .= (0 :: Int), "cases_reused" .= (30 :: Int)]
    , "controls" .= ([] :: [Value])
    , "differences" .= object []
    ]

verdictFailure :: FilePath -> IO Bool
verdictFailure root = do
  let view = testView root "staged"
      (code, verdict) = judgeAttempt view (attemptWith (root </> "attempt-fail") 0 (Just failingResults))
  check
    "a failing control yields a nonzero verdict with its case"
    ( code == 1
        && str (get verdict "verdict") == "failed"
        && str (get (get verdict "failure") "predicate") == "control:c-stock-swapped"
        && str (get (get verdict "failure") "case") == "c-stock-swapped-0x34-set"
    )
  check
    "reuse counts and native-only subset scope are reported"
    ( num (get (get verdict "execution") "batches_reused") == 2
        && num (get (get verdict "execution") "cases_reused") == 20
        && boolOf (get verdict "native_only")
        && boolOf (get (get verdict "scope") "subset")
        && encodeCanonical (get (get verdict "scope") "ids") == "[\"0x1\",\"0x2\"]"
    )

verdictPass :: FilePath -> IO Bool
verdictPass root = do
  let view = testView root "reused"
      (code, verdict) = judgeAttempt view (attemptWith (root </> "attempt-pass") 0 (Just passingResults))
  check
    "a passing full run reports full native scope and zero exit"
    ( code == 0
        && str (get verdict "verdict") == "passed"
        && boolOf (get (get verdict "scope") "full_native")
        && encodeCanonical (get (get verdict "scope") "ids") == "[]"
        && num (get (get verdict "execution") "batches_reused") == 3
    )

terminalOverwritesOldPass :: FilePath -> IO Bool
terminalOverwritesOldPass root = do
  let output = root </> "terminal"
      view = testView root "staged"
      passedVerdict = object ["schema_version" .= (1 :: Int), "verdict" .= ("passed" :: String)]
      (failedCode, failedVerdict) = judgeAttempt view (attemptWith (root </> "attempt-old") 0 (Just failingResults))
  createDirectoryIfMissing True output
  writeTerminal output passedVerdict (object [])
  writeTerminal output failedVerdict (object [])
  bytes <- B.readFile (output </> "verdict.json")
  case eitherDecodeStrict' bytes :: Either String Value of
    Left problem -> check ("the terminal verdict is readable: " ++ problem) False
    Right written ->
      check
        "an earlier passed verdict is replaced by the current failure"
        (failedCode == 1 && str (get written "verdict") == "failed")

defectOverride :: IO Bool
defectOverride =
  case parseOptions provided of
    Left problem -> check ("options parse: " ++ problem) False
    Right options ->
      check
        "a repeated --defect name keeps the later binary"
        (optDefects options == [("drop_provisional", "second.bin", "second.symbols")])
  where
    provided =
      required
        ++ ["--defect", "drop_provisional", "first.bin", "first.symbols", "--defect", "drop_provisional", "second.bin", "second.symbols"]
    required =
      [ "--dat", "dat", "--image4", "image4", "--loader-a", "loader-a", "--loader-b", "loader-b"
      , "--image5", "image5", "--plain", "plain", "--image2", "image2", "--linked", "linked"
      , "--symbols", "symbols", "--out", "out", "--cache", "cache", "--runner", "runner"
      , "--oracle", "oracle", "--stage", "stage", "--stage-encoder", "encoder"
      , "--stage-rebuild", "rebuild", "--stage-decoder", "decoder"
      ]

driverRefusal :: FilePath -> FilePath -> IO Bool
driverRefusal binary root = do
  let directory = root </> "refusal"
      out = directory </> "out"
  createDirectoryIfMissing True directory
  let names = ["dat", "image4", "loader-a", "loader-b", "image5", "plain", "image2", "linked", "symbols", "runner", "oracle", "stage", "encoder", "rebuild", "decoder"]
      flags = ["--dat", "--image4", "--loader-a", "--loader-b", "--image5", "--plain", "--image2", "--linked", "--symbols", "--runner", "--oracle", "--stage", "--stage-encoder", "--stage-rebuild", "--stage-decoder"]
  paths <- mapM (\name -> do let path = directory </> name; B.writeFile path B.empty; pure path) names
  let arguments =
        concat [[flag, path] | (flag, path) <- zip flags paths]
          ++ ["--out", out, "--cache", directory </> "cache"]
  (timedOut, exit, stdoutBytes, stderrBytes) <- runBounded 60000000 (4 * 1024 * 1024) binary arguments B.empty
  case eitherDecodeStrict' stdoutBytes :: Either String Value of
    Left problem ->
      check
        ("the driver refused a noncanonical image4: " ++ problem ++ " stderr " ++ BC.unpack (B.take 200 stderrBytes))
        False
    Right verdict -> do
      written <- doesFileExist (out </> "verdict.json")
      check
        "the driver refuses a noncanonical image4 with one error verdict and exit 2"
        ( not timedOut
            && exit == ExitFailure 2
            && str (get verdict "verdict") == "error"
            && "canonical" `isInfixOf` str (get (get verdict "failure") "detail")
            && written
        )

main :: IO ()
main = do
  arguments <- getArgs
  binary <- case arguments of
    path : _ -> pure path
    [] -> fail "usage: qualify-tests QUALIFY_BINARY"
  withTemporary "qualify-tests" $ \root -> do
    outcomes <-
      sequence
        [ stageAcceptance root
        , stageCorruption root
        , stageResume root
        , stageCommit root
        , verdictFailure root
        , verdictPass root
        , terminalOverwritesOldPass root
        , defectOverride
        , driverRefusal binary root
        ]
    unless (and outcomes) exitFailure
