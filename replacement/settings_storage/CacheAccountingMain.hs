{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE BangPatterns #-}
module Main (main) where

import Control.Exception (finally)
import Control.Monad (foldM, forM, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (nub)
import Data.Maybe (fromMaybe)
import Data.String (fromString)
import Emulation.CachedNativeBatch (CacheInspection(..), CacheVerdict(..), defaultNativeCacheRoot, inspectCachedBatch)
import Emulation.NativeCase (CaseStatus(..))
import Emulation.OwnedCase (CaseSpec, Symbol, parseSymbols, writeBatch)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (ensure, get, jsonRead, list, num, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import Native
import CustomAdmission (bankEntries)
import qualified Replacement.PropertyService.Artifact as Artifact
import qualified Replacement.PropertyService.Oracle as Property
import Replacement.PropertyService.Classify (Class(..), Property(..))
import qualified Replacement.PropertyService.Placement as Placement
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, getFileSize, listDirectory, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath (takeDirectory, (</>))
import System.Process (readProcessWithExitCode)
import Text.Read (readMaybe)

data Counts = Counts
  { rawHits :: !Int
  , abiPasses :: !Int
  , abiFailures :: !Int
  , misses :: !Int
  , invalidEntries :: !Int
  , ineligibleRunners :: !Int
  , maxStagedBytes :: !Int
  }

emptyCounts :: Counts
emptyCounts = Counts 0 0 0 0 0 0 0

addInspection :: Int -> CacheInspection -> Counts -> Counts
addInspection staged result counts = case result of
  InspectionRawHit _ _ verdicts ->
    counts
      { rawHits = rawHits counts + 1
      , abiPasses = abiPasses counts + length [() | (_, CacheVerdict CasePassed _) <- verdicts]
      , abiFailures = abiFailures counts + length [() | (_, CacheVerdict CaseFailed _) <- verdicts]
      , maxStagedBytes = max (maxStagedBytes counts) staged
      }
  InspectionAbsent _ -> counts {misses = misses counts + 1, maxStagedBytes = max (maxStagedBytes counts) staged}
  InspectionInvalid _ _ -> counts {invalidEntries = invalidEntries counts + 1, maxStagedBytes = max (maxStagedBytes counts) staged}
  InspectionIneligible _ -> counts {ineligibleRunners = ineligibleRunners counts + 1, maxStagedBytes = max (maxStagedBytes counts) staged}

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
  propertyLinked <- required args "--property-linked" >>= readRegular (Placement.carrierBytes Placement.stage4Carrier)
  propertySymbols <- required args "--property-symbols" >>= readRegular 0x10000
  propertyElf <- required args "--property-elf" >>= readRegular 0x400000
  propertyImage4 <- required args "--property-image4" >>= readRegular 0x300000
  binary <- required args "--linked" >>= readRegular 0x200000
  symbols <- required args "--symbols" >>= readSymbolsFile
  planPath <- required args "--plan"
  cacheRoot <- maybe defaultNativeCacheRoot id (argument args "--cache")
  createDirectoryIfMissing True cacheRoot
  runner <- required args "--runner"
  output <- required args "--out"
  sourceCommit <- required args "--source-commit"
  let maxCases = fromMaybe 4096 (argument args "--max-cases" >>= readMaybe) :: Int
      maxStaging = fromMaybe (512 * 1024 * 1024) (argument args "--max-staging-bytes" >>= readMaybe) :: Int
      perCaseBytes = fromMaybe (2 * 1024 * 1024) (argument args "--per-case-bytes" >>= readMaybe) :: Int
      reserveBytes = 100 * 1024 * 1024 * 1024 :: Integer
  source <- either (fail . show) pure (Artifact.canonicalSource dat image4 loaderA loaderB image5 plain image2)
  carrier <- maybe (fail "canonical property hull has no carrier") pure (Placement.hullCarrier (Artifact.sourceHull source))
  artifact <- either (fail . show) pure (Artifact.deriveArtifact Artifact.ProductionArtifact (Artifact.sourceProperties source) carrier (Artifact.ArtifactInput propertyLinked propertySymbols propertyElf))
  let classified = Artifact.sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
      arms = [owned | Property _ _ _ (Owned owned) <- classified]
      fixture = Property.withConfigVariants arms (Property.fixture image4)
      matrix = storageMatrix (prepareNativeImage image4) properties fixture Property.caseGrid
      stockSpecs = matrixCases matrix
      entries = [(address, "settings_bank_" ++ role ++ "_entry") | (address, role) <- bankEntries]
      prepared = prepareComposedImage propertyImage4 (propertyMetadata artifact) binary symbols entries (matrixFixtures matrix)
      ownedSpecs = map (ownedCase prepared) stockSpecs
      staging = output </> "staging"
  createDirectoryIfMissing True staging
  stock <- inspectCohort cacheRoot runner staging maxCases maxStaging stockSpecs
  owned <- inspectCohort cacheRoot runner staging maxCases maxStaging ownedSpecs
  plan <- jsonRead =<< B.readFile planPath
  let planCohorts = list (get plan "cohorts")
      visited = ["bank-stock", "bank-owned"]
      unvisited :: [(String, Int)]
      unvisited = [(str (get cohort "name"), fromIntegral (num (get cohort "cases"))) | cohort <- planCohorts, str (get cohort "name") `notElem` visited]
      stockUnselected = max 0 (length stockSpecs - selectedCount stock)
      ownedUnselected = max 0 (length ownedSpecs - selectedCount owned)
      unvisitedCases = sum [count | (_, count) <- unvisited] + stockUnselected + ownedUnselected :: Int
      selectedProblems = selectedProblemCount stock + selectedProblemCount owned
      requiredAdditionalCases = selectedProblems + unvisitedCases :: Int
      requiredAdditionalBytes = toInteger requiredAdditionalCases * toInteger perCaseBytes :: Integer
      rawHitCases = rawHits stock + rawHits owned :: Int
      rawHitProjection = toInteger rawHitCases * toInteger perCaseBytes :: Integer
      measuredStaged = max (maxStagedBytes stock) (maxStagedBytes owned) :: Int
  available <- availableBytes (takeDirectory output)
  let headroom = max 0 (available - reserveBytes)
      shortfall = max 0 (requiredAdditionalBytes - headroom)
      rawHitShortfall = max 0 (rawHitProjection - headroom)
  inputPaths <-
    forM
      [ ("dat", argumentValue args "--dat")
      , ("image4", argumentValue args "--image4")
      , ("loader-a", argumentValue args "--loader-a")
      , ("loader-b", argumentValue args "--loader-b")
      , ("image5", argumentValue args "--image5")
      , ("plain", argumentValue args "--plain")
      , ("image2", argumentValue args "--image2")
      , ("property-linked", argumentValue args "--property-linked")
      , ("property-symbols", argumentValue args "--property-symbols")
      , ("property-elf", argumentValue args "--property-elf")
      , ("property-image4", argumentValue args "--property-image4")
      , ("linked", argumentValue args "--linked")
      , ("symbols", argumentValue args "--symbols")
      ]
      $ \(label, path) -> do
        digest <- sha256File path
        pure (label, object ["path" .= path, "sha256" .= sha256Hex digest])
  runnerDigest <- sha256File runner
  let report = object
        [ "schema_version" .= (1 :: Int)
        , "kind" .= ("cache-accounting" :: String)
        , "source_commit" .= sourceCommit
        , "runner" .= object ["path" .= runner, "sha256" .= sha256Hex runnerDigest]
        , "inputs" .= object [fromString label .= value | (label, value) <- inputPaths]
        , "cohorts" .= object
            [ "bank-stock" .= cohortValue "bank-stock" (length stockSpecs) stock
            , "bank-owned" .= cohortValue "bank-owned" (length ownedSpecs) owned
            ]
        , "unvisited" .= object
            [ "cohorts" .= [object ["name" .= name, "cases" .= count, "treated" .= ("misses-unproven" :: String)] | (name, count) <- unvisited]
            , "current_unselected_cases" .= (stockUnselected + ownedUnselected)
            , "cases" .= unvisitedCases
            , "full_plan_reuse_claim" .= False
            ]
        , "measured" .= object
            [ "available_bytes" .= available
            , "reserve_bytes" .= reserveBytes
            , "headroom_bytes" .= headroom
            , "max_staged_case_bytes" .= measuredStaged
            ]
        , "assumed_projection" .= object
            [ "kind" .= ("assumed_per_case" :: String)
            , "per_case_bytes" .= perCaseBytes
            , "per_case_bytes_demonstrated" .= False
            , "required_additional_cases" .= requiredAdditionalCases
            , "required_additional_bytes" .= requiredAdditionalBytes
            , "required_additional_shortfall_bytes" .= shortfall
            ]
        , "raw_cache_growth" .= object
            [ "raw_hit_cases" .= rawHitCases
            , "projection_bytes" .= rawHitProjection
            , "projection_kind" .= ("assumed_per_case" :: String)
            , "includes_current_result_staging_overhead" .= True
            , "whole_job_disk_clearance" .= False
            , "shortfall_bytes" .= rawHitShortfall
            ]
        , "native_executions" .= (0 :: Int)
        , "inspector" .= ("read-only inspectCachedBatch; no executeValidated/runBatch/quarantine" :: String)
        ]
      reportBytes = encodeCanonical report
  ensure (B.length reportBytes <= maxStaging) "accounting report exceeds the persistent output cap"
  createDirectoryIfMissing True output
  B.writeFile (output </> "accounting.json") reportBytes

inspectCohort :: FilePath -> FilePath -> FilePath -> Int -> Int -> [CaseSpec] -> IO Counts
inspectCohort cacheRoot runner staging maxCases maxStaging specs = do
  let selected = if maxCases <= 0 then specs else take maxCases specs
  foldM inspect emptyCounts selected
  where
    inspect counts spec = do
      let stage = staging </> "case"
      cleanStage stage
      (bytes, result) <- (do
        batchPath <- writeBatch stage [spec]
        bytes <- directoryBytes stage
        ensure (bytes <= maxStaging) "accounting staging exceeds the persistent cap"
        result <- inspectCachedBatch cacheRoot runner batchPath
        pure (bytes, result)) `finally` cleanStage stage
      let !next = addInspection bytes result counts
      pure next

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

readSymbolsFile :: FilePath -> IO (M.Map String Symbol)
readSymbolsFile path = do
  rows <- map words . lines <$> readFile path
  let names = [name | [_, _, kind, name] <- rows, kind `elem` ["T", "t", "A", "a"] || name `elem` ["SLOT_BYTES", "TAIL_BYTES"]]
  ensure (not (null names) && length names == length (nub names)) "missing or duplicate custom symbols"
  parseSymbols names path

availableBytes :: FilePath -> IO Integer
availableBytes path = do
  directory <- existingDirectory path
  (code, out, err) <- readProcessWithExitCode "df" ["-B1", "--output=avail", "--", directory] ""
  unless (code == ExitSuccess) (ioError (userError ("cannot measure available storage: " ++ err)))
  case lines out of
    [_header, value] | Just count <- readMaybe value, count >= (0 :: Integer) -> pure count
    _ -> ioError (userError "df did not return one available-byte count")

existingDirectory :: FilePath -> IO FilePath
existingDirectory path = do
  exists <- doesDirectoryExist path
  if exists
    then pure path
    else do
      let parent = takeDirectory path
      if parent == path
        then ioError (userError ("no existing ancestor for " ++ path))
        else existingDirectory parent

selectedCount :: Counts -> Int
selectedCount counts = rawHits counts + misses counts + invalidEntries counts + ineligibleRunners counts

selectedProblemCount :: Counts -> Int
selectedProblemCount counts = misses counts + invalidEntries counts + ineligibleRunners counts

cohortValue :: String -> Int -> Counts -> Value
cohortValue name total counts = object
  [ "cohort" .= name
  , "cohort_cases" .= total
  , "selected_cases" .= selectedCount counts
  , "unselected_cases" .= max 0 (total - selectedCount counts)
  , "selected_problem_cases" .= selectedProblemCount counts
  , "max_staged_case_bytes" .= maxStagedBytes counts
  , "raw_hits" .= rawHits counts
  , "current_abi_passes" .= abiPasses counts
  , "current_abi_failures" .= abiFailures counts
  , "misses" .= misses counts
  , "invalid_entries" .= invalidEntries counts
  , "ineligible_runners" .= ineligibleRunners counts
  ]

required :: [String] -> String -> IO String
required args flag = maybe (ioError (userError ("missing " ++ flag))) pure (argument args flag)

argument :: [String] -> String -> Maybe String
argument args flag = case dropWhile (/= flag) args of
  _ : value : _ -> Just value
  _ -> Nothing

argumentValue :: [String] -> String -> FilePath
argumentValue args flag = fromMaybe (error ("missing " ++ flag)) (argument args flag)
