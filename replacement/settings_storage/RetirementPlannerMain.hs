{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (finally)
import Control.Monad (foldM, forM, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Emulation.CachedNativeBatch (CacheBinding, cacheBindingEntry, cacheBindingManifestSha256, CacheInspection (..), defaultNativeCacheRoot, inspectCachedBatch, readCachedPayload)
import Emulation.NativeCase (CaseId (..), caseIdText)
import Emulation.PrimaryInitializerSource (ByteCount (..))
import Emulation.OwnedCase (CaseSpec (..), writeBatch)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex, sha256File)
import FujiAF.Host (ensure, get, jsonRead, keys, list, num, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import FujiAF.StorageBudget (checkStorageBudget)
import Native
import qualified Replacement.PropertyService.Artifact as Artifact
import qualified Replacement.PropertyService.Oracle as Property
import Replacement.PropertyService.Classify (Class (..), Property (..))
import RetirementPlanner
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, doesPathExist, getFileSize, listDirectory, makeAbsolute, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, (</>))
import Text.Read (readMaybe)

main :: IO ()
main = do
  args <- getArgs
  dat <- required args "--dat" >>= readRegular 0x4000000
  image4 <- required args "--image4" >>= readRegular 0x300000
  loaderA <- required args "--loader-a" >>= readRegular 0x20000
  loaderB <- required args "--loader-b" >>= readRegular 0x20000
  image5 <- required args "--image5" >>= readRegular 0x800000
  plain <- required args "--plain" >>= readRegular 0x800000
  image2 <- required args "--image2" >>= readRegular 0x800000
  oldRoot <- required args "--old-root" >>= canonicalizePath
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional args "--cache"
  createDirectoryIfMissing True cacheRoot
  runner <- required args "--runner"
  output <- required args "--out" >>= makeAbsolute
  sourceCommit <- required args "--source-commit"
  protectedPath <- required args "--protected-file"
  protected <- readProtected protectedPath
  maxCases <- case optional args "--max-cases" of
    Nothing -> pure 4096
    Just value -> case readMaybe value of
      Just count | count > 0 && count <= 4096 -> pure count
      _ -> ioError (userError "--max-cases must be a positive integer at most 4096")
  offset <- case optional args "--offset" of
    Nothing -> pure (CaseOffset 0)
    Just value -> case readMaybe value of
      Just count | count >= 0 -> pure (CaseOffset count)
      _ -> ioError (userError "--offset must be a non-negative integer")
  let staging = output </> "staging"
      maxStaging = 512 * 1024 * 1024 :: Int
  exists <- doesDirectoryExist output
  ensure (not exists) "retirement output must be a fresh directory"
  checkStorageBudget output
  source <- either (fail . show) pure (Artifact.canonicalSource dat image4 loaderA loaderB image5 plain image2)
  let classified = Artifact.sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
      arms = [owned | Property _ _ _ (Owned owned) <- classified]
      fixture = Property.withConfigVariants arms (Property.fixture image4)
      matrix = storageMatrix (prepareNativeImage image4) properties fixture Property.caseGrid
  selected <- either (ioError . userError) pure (selectCaseWindow offset maxCases (matrixCases matrix))
  runnerDigest <- sha256File runner
  createDirectoryIfMissing True staging
  (candidates, preserved, witnesses) <- foldM
    (inspectOne oldRoot cacheRoot runner runnerDigest staging maxStaging protected)
    ([], [], S.empty)
    selected
  let report = object
        [ "schema_version" .= (1 :: Int)
        , "kind" .= ("native-result-retirement-plan" :: String)
        , "source_commit" .= sourceCommit
        , "old_root" .= oldRoot
        , "offset" .= unCaseOffset offset
        , "max_cases" .= maxCases
        , "runner" .= object ["path" .= runner, "sha256" .= sha256Hex runnerDigest]
        , "selected_current_cases" .= length selected
        , "candidates" .= reverse candidates
        , "preserved" .= reverse preserved
        , "witnesses" .= [show failure | failure <- S.toList witnesses]
        , "native_executions" .= (0 :: Int)
        , "deletions" .= (0 :: Int)
        ]
      bytes = encodeCanonical report
  ensure (B.length bytes <= maxStaging) "retirement plan exceeds the persistent output cap"
  createDirectoryIfMissing True output
  B.writeFile (output </> "retirement-plan.json") bytes

inspectOne :: FilePath -> FilePath -> FilePath -> Sha256 -> FilePath -> Int -> [FilePath] -> ([Value], [Value], S.Set LegacyFailure) -> (Int, CaseSpec) -> IO ([Value], [Value], S.Set LegacyFailure)
inspectOne oldRoot cacheRoot runner runnerDigest staging maxStaging protected (candidates, preserved, witnesses) (index, spec) = do
  let caseName = caseId spec
      oldPathRaw = oldRoot </> show index </> caseName ++ ".native-result.json"
  oldExists <- doesFileExist oldPathRaw
  if not oldExists
    then pure (candidates, object ["path" .= oldPathRaw, "reason" .= ("missing-old-report" :: String)] : preserved, witnesses)
    else do
      oldPath <- canonicalizePath oldPathRaw
      oldBytes <- B.readFile oldPath
      old <- jsonRead oldBytes
      let oldEvidence = OldEvidence
            { oldPath = oldPath
            , oldBytes = ByteCount (fromIntegral (B.length oldBytes))
            , oldDigest = sha256 oldBytes
            , oldCaseId = CaseId (str (get old "case_id"))
            , oldWireSha = parseDigest (str (get old "wire_sha256"))
            , oldRunnerSha = parseDigest (str (get old "runner_sha256"))
            , oldRunnerExit = exitCode (num (get old "runner_exit"))
            , oldRaw = get old "runner"
            , oldFailureClass = parseLegacyFailure [str item | item <- list (get old "mismatches")]
            , oldRawCache = if "raw_cache" `elem` keys old then Just (get old "raw_cache") else Nothing
            }
          classKey = oldFailureClass oldEvidence
      if classKey `S.member` witnesses
        then inspectCurrent oldEvidence
        else do
          let witness = object ["path" .= oldPath, "failure_class" .= show classKey]
          pure (candidates, witness : preserved, S.insert classKey witnesses)
  where
    inspectCurrent oldEvidence = do
      let stage = staging </> "case"
          requestedName = caseId spec
          oldCaseName = caseIdText (oldCaseId oldEvidence)
          oldCasePath = takeDirectory (oldPath oldEvidence) </> oldCaseName ++ ".case.json"
          currentCasePath = stage </> requestedName ++ ".case.json"
      cleanStage stage
      (stagedBytes, result, currentCase) <- (do
        batchPath <- writeBatch stage [spec]
        bytes <- directoryBytes stage
        ensure (bytes <= maxStaging) "retirement staging exceeds the persistent cap"
        currentCase <- readCaseJson currentCasePath
        result <- inspectCachedBatch cacheRoot runner batchPath
        pure (bytes, result, currentCase)) `finally` cleanStage stage
      case result of
        InspectionAbsent _ -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("missing-binding" :: String), "staged_bytes" .= stagedBytes] : preserved, witnesses)
        InspectionInvalid _ reason -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("invalid-binding" :: String), "detail" .= reason, "staged_bytes" .= stagedBytes] : preserved, witnesses)
        InspectionIneligible _ -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("ineligible-runner" :: String), "staged_bytes" .= stagedBytes] : preserved, witnesses)
        InspectionRawHit _ cacheBinding verdicts -> do
          let currentVerdict = case [verdict | (ident, verdict) <- verdicts, caseIdText ident == requestedName] of
                [verdict] -> Just verdict
                _ -> Nothing
          case currentVerdict of
            Nothing -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("missing-current-verdict" :: String)] : preserved, witnesses)
            Just verdict -> do
              manifest <- readManifest cacheBinding
              oldCase <- readCaseJson oldCasePath
              case (manifest, oldCase, currentCase) of
                (Just manifestEvidence, Just oldCaseValue, Just currentCaseValue) -> do
                  let recordedName = caseIdText (manifestRecordedCaseId manifestEvidence)
                  currentRaw <- readCachedPayload cacheBinding (CaseId recordedName)
                  let currentEvidence = CurrentEvidence
                        { currentCaseId = CaseId requestedName
                        , currentVerdict = verdict
                        , currentRunnerSha = runnerDigest
                        , currentRawBytes = currentRaw
                        , currentExpectationMatches = expectationsMatch oldCaseValue currentCaseValue
                        }
                  case classifyRetirement protected S.empty oldEvidence (Just manifestEvidence) currentEvidence of
                    RetirementCandidate candidate -> pure (candidateValue candidate : candidates, preserved, witnesses)
                    RetirementPreserved reason -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= show reason] : preserved, witnesses)
                (Nothing, _, _) -> do
                  let entry = cacheBindingEntry cacheBinding
                      manifestPath = entry </> "manifest.json"
                  manifestExists <- doesFileExist manifestPath
                  pure (candidates, object
                    [ "path" .= oldPath oldEvidence
                    , "reason" .= (if manifestExists then "missing-manifest-case" else "missing-manifest-file" :: String)
                    , "cache_entry" .= entry
                    ] : preserved, witnesses)
                (_, Nothing, _) -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("missing-old-case" :: String)] : preserved, witnesses)
                (_, _, Nothing) -> pure (candidates, object ["path" .= oldPath oldEvidence, "reason" .= ("missing-current-case" :: String)] : preserved, witnesses)

readCaseJson :: FilePath -> IO (Maybe Value)
readCaseJson path = do
  exists <- doesFileExist path
  if exists then Just <$> (jsonRead =<< B.readFile path) else pure Nothing

readManifest :: CacheBinding -> IO (Maybe ManifestEvidence)
readManifest binding = do
  let entry = cacheBindingEntry binding
      path = entry </> "manifest.json"
  exists <- doesFileExist path
  if not exists then pure Nothing else do
    bytes <- B.readFile path
    ensure (sha256 bytes == cacheBindingManifestSha256 binding) "cache manifest changed"
    manifest <- jsonRead bytes
    case list (get manifest "cases") of
      [row] -> do
        let runner = get manifest "runner"
        pure (Just ManifestEvidence
          { manifestEntry = entry
          , manifestSha = cacheBindingManifestSha256 binding
          , manifestRecordedCaseId = CaseId (str (get row "id"))
          , manifestWireSha = parseDigest (str (get row "wire_sha256"))
          , manifestRunnerSha = parseDigest (str (get runner "sha256"))
          , manifestRunnerExit = exitCode (num (get row "runner_exit"))
          , manifestRawSha = parseDigest (str (get row "raw_sha256"))
          })
      _ -> pure Nothing

candidateValue :: Candidate -> Value
candidateValue candidate = object
  [ "path" .= candidatePath candidate
  , "bytes" .= unByteCount (candidateBytes candidate)
  , "wrapper_sha256" .= sha256Hex (candidateDigest candidate)
  , "case_id" .= caseIdText (candidateCaseId candidate)
  , "old_wire_sha256" .= sha256Hex (candidateOldWireSha candidate)
  , "manifest_wire_sha256" .= sha256Hex (candidateManifestWireSha candidate)
  , "old_runner_sha256" .= sha256Hex (candidateOldRunnerSha candidate)
  , "current_runner_sha256" .= sha256Hex (candidateCurrentRunnerSha candidate)
  , "raw_sha256" .= sha256Hex (candidateRawSha candidate)
  , "manifest_sha256" .= sha256Hex (candidateManifestSha candidate)
  , "current_verdict" .= ("passed" :: String)
  ]

parseDigest :: String -> Sha256
parseDigest text = either (error . ("invalid digest " ++)) id (parseSha256Hex text)

exitCode :: Integer -> ExitCode
exitCode 0 = ExitSuccess
exitCode code = ExitFailure (fromIntegral code)

unByteCount :: ByteCount -> Int
unByteCount (ByteCount count) = fromIntegral count

cleanStage :: FilePath -> IO ()
cleanStage stage = do
  exists <- doesDirectoryExist stage
  when exists (removeDirectoryRecursive stage)

directoryBytes :: FilePath -> IO Int
directoryBytes directory = do
  names <- listDirectory directory
  sizes <- forM names $ \name -> do
    let path = directory </> name
    isDirectory <- doesDirectoryExist path
    if isDirectory then directoryBytes path else fromIntegral <$> getFileSize path
  pure (sum sizes)

readProtected :: FilePath -> IO [FilePath]
readProtected path = do
  rows <- lines <$> readFile path
  forM rows $ \row -> do
    exists <- doesPathExist row
    if exists then canonicalizePath row else makeAbsolute row

required :: [String] -> String -> IO String
required args flag = maybe (ioError (userError ("missing " ++ flag))) pure (optional args flag)

optional :: [String] -> String -> Maybe String
optional args flag = case dropWhile (/= flag) args of
  _ : value : _ -> Just value
  _ -> Nothing
