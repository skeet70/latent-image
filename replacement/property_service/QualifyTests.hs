{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Exception (bracket_)
import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
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
    , "qualification_scope" .= ("staged-candidate" :: String)
    , "coverage" .= ("current-source-matrix" :: String)
    , "dat_retirement_admitted" .= False
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
      check "parsed control failure" (case summaryControls summary of
        [ControlSummary _ False (GuestControl MustFail _ 3 1 [_])] -> True
        _ -> False)
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
  check "native-only evidence cannot qualify a staged candidate" (isLeft (parseOracleSummary (replaceField "qualification_scope" (String "native-only") validSummary)))
  check "bounded smoke cannot qualify the full matrix" (isLeft (parseOracleSummary (replaceField "coverage" (String "bounded-smoke-not-full-qualification") validSummary)))
  check "native observations cannot admit retirement" (isLeft (parseOracleSummary (replaceField "dat_retirement_admitted" (Bool True) validSummary)))
  let admission = object ["name" .= ("getter-public-push" :: String), "kind" .= ("admission-refusal" :: String), "holds" .= True]
      entry = object ["name" .= ("id-from-r12" :: String), "holds" .= True, "failed_cases" .= (1 :: Int), "statuses" .= ["returned" :: String]]
      unavailable = object ["name" .= ("missing class" :: String), "holds" .= False]
      mixed = replaceField "controls" (toJSON [sampleControl, admission, entry, unavailable]) validSummary
  case parseOracleSummary mixed of
    Left problem -> check ("actual emitted control shapes parse: " ++ problem) False
    Right summary -> check "control evidence retains each emitted shape" (case map controlEvidence (summaryControls summary) of [GuestControl MustFail _ 3 1 [_], AdmissionRefusal, EntryControl 1 [_], UnavailableControl] -> True; _ -> False)
  check "entry control without failed count is refused" (isLeft (parseOracleSummary (replaceField "controls" (toJSON [dropField "failed_cases" entry]) validSummary)))
  check "guest control without note is refused" (isLeft (parseOracleSummary (replaceField "controls" (toJSON [dropField "note" sampleControl]) validSummary)))
  check "unavailable control cannot claim success" (isLeft (parseOracleSummary (replaceField "controls" (toJSON [replaceField "holds" (Bool True) unavailable]) validSummary)))

optionsTest :: IO ()
optionsTest = do
  case parseOptions provided of
    Left problem -> check ("options parse: " ++ problem) False
    Right options -> do
      check "a repeated --defect name keeps the later complete artifact" (optDefects options == [("drop_provisional", "second.bin", "second.symbols", "second.elf")])
      check "the canonical ELF is retained" (optElf options == "elf")
      check "the storage limit is typed" (optStorageLimit options == Just (StorageLimitBytes 75161927680))
      check "the cache root defaults to the shared cache" (optCache options == defaultNativeCacheRoot)
  check "a missing canonical ELF is refused" (isLeft (parseOptions (removeOption "--elf" required)))
  check "a defect without ELF is refused" (isLeft (parseOptions (required ++ ["--defect", "broken", "bin", "symbols"])))
  check "a missing storage root is refused" (isLeft (parseOptions (removeOption "--storage-root" required)))
  check "a missing storage limit is refused" (isLeft (parseOptions (removeOption "--storage-limit-bytes" required)))
  check "a nonpositive storage limit is refused" (isLeft (parseStorageLimit "0"))
  check "a fractional storage limit is refused" (isLeft (parseStorageLimit "1.5"))
  check "a zero checkpoint is refused" (isLeft (parseBatchCheckpoint "0"))
  check "a positive checkpoint is retained" (parseBatchCheckpoint "8" == Right (BatchCheckpoint 8))
  where
    removeOption flag arguments = case arguments of
      name : _ : rest | name == flag -> rest
      name : value : rest -> name : value : removeOption flag rest
      rest -> rest
    provided =
      required
        ++ ["--defect", "drop_provisional", "first.bin", "first.symbols", "first.elf", "--defect", "drop_provisional", "second.bin", "second.symbols", "second.elf"]
    required =
      [ "--dat", "dat", "--image4", "image4", "--loader-a", "loader-a", "--loader-b", "loader-b"
      , "--image5", "image5", "--plain", "plain", "--image2", "image2", "--linked", "linked"
      , "--symbols", "symbols", "--out", "out", "--runner", "runner", "--emulator-slot", "slot"
      , "--elf", "elf", "--storage-root", "storage", "--storage-limit-bytes", "75161927680"
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
  let cache = base </> "private-data" </> "cache"
  createDirectoryIfMissing True cache
  cacheResult <- resolveCacheOutput "--cache" scratch cache (cache </> "native")
  check "a cache path under private-data/cache/native is accepted" (either (const False) (const True) cacheResult)

driverRefusal :: FilePath -> FilePath -> IO ()
driverRefusal binary root = do
  let directory = root </> "refusal"
      out = root </> "private-data" </> "scratch" </> "refusal"
  createDirectoryIfMissing True directory
  let names = ["dat", "image4", "loader-a", "loader-b", "image5", "plain", "image2", "linked", "symbols", "elf", "runner", "slot", "oracle", "stage", "encoder", "rebuild", "decoder"]
      flags = ["--dat", "--image4", "--loader-a", "--loader-b", "--image5", "--plain", "--image2", "--linked", "--symbols", "--elf", "--runner", "--emulator-slot", "--oracle", "--stage", "--stage-encoder", "--stage-rebuild", "--stage-decoder"]
  paths <- mapM (\name -> do
    let path = directory </> name
    B.writeFile path B.empty
    pure path) names
  previousWorkspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let runtimeFlags = ["--linked", "--symbols", "--elf", "--runner", "--emulator-slot", "--oracle", "--stage", "--stage-encoder", "--stage-rebuild", "--stage-decoder"]
      arguments = concat [[flag, if flag `elem` runtimeFlags then takeFileName path else path] | (flag, path) <- zip flags paths]
        ++ ["--out", out, "--cache", root </> "private-data" </> "scratch" </> "refusal-cache", "--storage-root", root, "--storage-limit-bytes", "75161927680"]
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
  let admittedRoot = root </> "private-data" </> "scratch" </> "admitted"
      rejectedOut = admittedRoot </> "new-output"
      rejectedCache = root </> "private-data" </> "scratch" </> "rejected-cache"
  createDirectoryIfMissing True admittedRoot
  (_, cacheExit, _, _) <- runDriver (arguments ++ ["--storage-root", admittedRoot, "--out", rejectedOut, "--cache", rejectedCache])
  rejectedOutExists <- doesPathExist rejectedOut
  rejectedCacheExists <- doesPathExist rejectedCache
  check "a rejected cache cannot create output or publish a current pointer" (cacheExit == ExitFailure 2 && not rejectedOutExists && not rejectedCacheExists)
  let rejectedSibling = root </> "private-data" </> "scratch" </> "rejected-output"
  (_, rootExit, _, _) <- runDriver (arguments ++ ["--storage-root", admittedRoot, "--out", rejectedSibling, "--cache", admittedRoot </> "cache"])
  rejectedSiblingExists <- doesPathExist rejectedSibling
  check "a private-scratch sibling outside the admitted root cannot be published" (rootExit == ExitFailure 2 && not rejectedSiblingExists)
  let cacheLink = admittedRoot </> "cache-link"
      escapedCache = root </> "escaped-cache"
  createDirectoryLink root cacheLink
  (_, linkExit, _, _) <- runDriver (arguments ++ ["--storage-root", admittedRoot, "--out", rejectedOut, "--cache", cacheLink </> "escaped-cache"])
  linkedOutputExists <- doesPathExist rejectedOut
  escapedCacheExists <- doesPathExist escapedCache
  check "a cache symlink escape creates neither output nor escaped cache" (linkExit == ExitFailure 2 && not linkedOutputExists && not escapedCacheExists)
  let missingOutput = admittedRoot </> "missing-runtime-output"
  (missingTimedOut, missingExit, missingBytes, _) <- runDriver (arguments ++ ["--elf", root </> "absent.elf", "--storage-root", admittedRoot, "--out", missingOutput, "--cache", admittedRoot </> "cache"])
  missingVerdict <- case eitherDecodeStrict' missingBytes :: Either String Value of
    Left problem -> ioError (userError ("missing-runtime verdict is not JSON: " ++ problem))
    Right value -> pure value
  missingOutputExists <- doesPathExist missingOutput
  check "a missing runtime artifact returns an unpublished structured driver error" (not missingTimedOut && missingExit == ExitFailure 2 && str (get missingVerdict "verdict") == "error" && str (get (get missingVerdict "failure") "predicate") == "driver" && not missingOutputExists)

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
