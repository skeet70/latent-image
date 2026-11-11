{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.PropertyService.Compact
  ( PairPlan(..), PairRequirement(..), RetentionSelection(..), FamilyName(..)
  , CompactContract(..), CompactConfig(..), AllocationGeometry(..), StoragePhase(..), StorageCheckpoint(..)
  , ByteCount(..), CompactStats(..), CompactArtifact(..), CompactResult(..)
  , executeCompactPairs, compactResultValue, publishCompactEvidence, reserveFile
  ) where

import Codec.Compression.GZip (compress)
import Control.Exception (bracket, onException)
import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (newIORef, readIORef, modifyIORef')
import Data.List (nub)
import qualified Data.Set as S
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeEvidence as E
import qualified Emulation.NativeObservation as N
import Emulation.OwnedCase (CaseSpec(..), CaseRegion(..), Permission(ReadWrite), writeBatchWithInputPool)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex)
import FujiAF.Host (get, list, str)
import FujiAF.JSON (encodeCanonical)
import GHC.Clock (getMonotonicTimeNSec)
import Replacement.PropertyService.CompactReceipt
import Replacement.PropertyService.Oracle (Expectation, Run, Difference(..), WriteEvent(..), expectationDifferences, differenceKind, differenceDetail)
import System.Directory (createDirectory, createDirectoryIfMissing, copyFile, doesDirectoryExist, doesFileExist, getFileSize, listDirectory, removeDirectoryRecursive, renameFile)
import System.FilePath ((</>), takeDirectory, takeFileName, splitDirectories)
import System.Posix.Temp (mkdtemp)
import System.Posix.IO (OpenMode(ReadOnly), openFd, closeFd, defaultFileFlags)
import System.Posix.Unistd (fileSynchronise)

newtype FamilyName = FamilyName String deriving (Eq, Ord, Show)
newtype ByteCount = ByteCount Integer deriving (Eq, Ord, Show)
data RetentionSelection = CompactOnly | RepresentativeFull | ExactDiagnosticFull deriving (Eq, Show)
data PairRequirement = MustAgree | MustDiffer [Difference] deriving (Eq, Show)
data PairPlan = PairPlan
  { planStock :: CaseSpec
  , planOwned :: Run -> CaseSpec
  , planModel :: Maybe Expectation
  , planRetention :: RetentionSelection
  , planFamily :: FamilyName
  , planRequirement :: PairRequirement
  }
data CompactContract = CompactContract
  { contractSource :: Sha256
  , contractInput :: Sha256
  , contractRunner :: Sha256
  }
data StoragePhase = BeforeChunk | BeforeWrite | AfterWrite | AfterChunk deriving (Eq, Show)
data StorageCheckpoint = StorageCheckpoint
  { checkpointPhase :: StoragePhase
  , checkpointPendingBytes :: ByteCount
  } deriving (Eq, Show)
data AllocationGeometry = Btrfs4096 deriving (Eq, Show)
data CompactConfig = CompactConfig
  { compactOutput :: FilePath
  , compactRunner :: FilePath
  , compactContract :: CompactContract
  , compactChunkPairs :: Int
  , compactAllocationGeometry :: AllocationGeometry
  , compactCheck :: StorageCheckpoint -> IO ()
  }
data CompactStats = CompactStats
  { compactSessions :: Int
  , compactObservations :: Int
  , compactSDKFailedObservations :: Int
  , compactUnsupportedSourceBoundsObservations :: Int
  , compactFullFailureReports :: Int
  , compactReceiptBytes :: ByteCount
  , compactRawBytes :: ByteCount
  , compactFullEvidenceBytes :: ByteCount
  , compactStagingPeakBytes :: ByteCount
  , compactElapsedNanoseconds :: Integer
  , compactPeakResidentBytes :: Maybe ByteCount
  } deriving (Eq, Show)
data CompactArtifact = CompactArtifact
  { compactArtifactPath :: FilePath
  , compactArtifactBytes :: ByteCount
  , compactArtifactSHA :: Sha256
  } deriving Eq
data CompactResult = CompactResult
  { compactConsumerAgreement :: Bool
  , compactStats :: CompactStats
  , compactArtifacts :: [CompactArtifact]
  , compactSessionDirectory :: FilePath
  } deriving Eq

newtype RawReport = RawReport B.ByteString
data Captured = Captured CaseSpec FilePath RawReport ObservationReceipt

executeCompactPairs :: CompactConfig -> [PairPlan] -> IO CompactResult
executeCompactPairs config plans = do
  unless (compactChunkPairs config > 0 && compactChunkPairs config <= 64) (fail "compact chunk must contain 1..64 pairs")
  check config BeforeChunk 0
  actualRunner <- sha256File (compactRunner config)
  unless (actualRunner == contractRunner (compactContract config)) (fail "compact runner identity differs")
  check config BeforeWrite (directoryReservation config (length (splitDirectories (compactOutput config)) + 1))
  createDirectoryIfMissing True (compactOutput config)
  root <- mkdtemp (compactOutput config </> "compact-session-")
  started <- getMonotonicTimeNSec
  families <- newIORef S.empty
  let initial = CompactStats 0 0 0 0 0 (ByteCount 0) (ByteCount 0) (ByteCount 0) (ByteCount 0) 0 Nothing
      loop _ [] stats artifacts passed = do
        ended <- getMonotonicTimeNSec
        pure (CompactResult passed stats {compactElapsedNanoseconds = toInteger (ended-started)} (reverse artifacts) root)
      loop index remaining stats artifacts passed = do
        let (part,rest) = splitAt (compactChunkPairs config) remaining
            stage = root </> ("chunk-" ++ show index)
        check config BeforeChunk (maximum (0 : map (failureReservation config . planStock) part) + 2 * sum (map (stagingReservation config . planStock) part))
        createDirectory stage
        (next,artifact,ok) <- E.withInputPool stage $ \pool -> do
          stock <- runSpecs config pool (stage </> "stock") (map planStock part) (map stockAccepts part) 0
          let ownedSpecs = zipWith (\plan (Captured _ _ _ receipt) -> planOwned plan (observationRun receipt)) part stock
              ownedChecks = zipWith3 (\plan (Captured _ _ _ reference) spec subject ->
                observationPassed subject
                && (case planRequirement plan of
                  MustAgree -> diagnosticAllowed spec subject
                  MustDiffer expected -> controlDiagnosticsAllowed spec expected subject)
                && requirementPassed plan (pairReceipt (planModel plan) reference subject)) part stock ownedSpecs
          owned <- runSpecs config pool (stage </> "owned") ownedSpecs ownedChecks (capturedBytes config stock) `onException` emergencyCaptures stock
          let pairs = zipWith3 (\plan (Captured _ _ _ a) (Captured _ _ _ b) -> pairReceipt (planModel plan) a b) part stock owned
              ok = and (zipWith requirementPassed part pairs)
              captures = zip3 part stock owned
              failedSDK = [receipt | Captured _ _ _ receipt <- stock ++ owned, observationSDKStatus receipt == SDKFailed]
              unsupported = [receipt | Captured spec _ _ receipt <- stock ++ owned, observationSDKStatus receipt == SDKFailed, diagnosticAllowed spec receipt]
          let publicationConfig = reserveConfig config (capturedBytes config stock + capturedBytes config owned)
              retain capture = retainCapture publicationConfig capture >> pure ()
          (do
            forM_ (zip captures pairs) $ \((plan,a@(Captured _ _ _ ar),b@(Captured _ _ _ br)),receipt) -> do
              known <- readIORef families
              let key = (planFamily plan, observationDiagnosticClass ar, observationDiagnosticClass br)
                  selected = planRetention plan /= CompactOnly || S.notMember key known
                  required = not (requirementPassed plan receipt) || observationRequiresFull ar || observationRequiresFull br
              when (selected || required) (retain a >> retain b)
              modifyIORef' families (S.insert key)
            let body = B.concat [encodePlannedPair plan receipt <> "\n" | (plan,receipt) <- zip part pairs]
                contract = compactContract config
                header = encodeCanonical (object
                  [ "schema_version" .= (1::Int), "source_sha256" .= sha256Hex (contractSource contract)
                  , "input_sha256" .= sha256Hex (contractInput contract), "runner_sha256" .= sha256Hex (contractRunner contract)
                  , "pairs" .= length part, "chunk" .= index ]) <> "\n"
                compressed = BL.toStrict (compress (BL.fromStrict (header <> body)))
                destination = root </> ("receipts-" ++ show index ++ ".jsonl.gz")
            publishCompactEvidence publicationConfig destination compressed
            staging <- directoryBytes stage
            fullExists <- doesDirectoryExist (root </> "full")
            fullBytes <- if fullExists then directoryBytes (root </> "full") else pure 0
            raw <- if fullExists then rawReportBytes (root </> "full") else pure 0
            check publicationConfig AfterChunk 0
            let ByteCount oldReceipt = compactReceiptBytes stats
                ByteCount oldPeak = compactStagingPeakBytes stats
                next = stats { compactSessions = compactSessions stats+2*length part
                  , compactObservations = compactObservations stats+2*length part
                  , compactSDKFailedObservations = compactSDKFailedObservations stats + length failedSDK
                  , compactUnsupportedSourceBoundsObservations = compactUnsupportedSourceBoundsObservations stats + length unsupported
                  , compactFullFailureReports = compactFullFailureReports stats + length failedSDK
                  , compactReceiptBytes = ByteCount (oldReceipt+toInteger (B.length compressed))
                  , compactRawBytes = ByteCount raw
                  , compactFullEvidenceBytes = ByteCount fullBytes
                  , compactStagingPeakBytes = ByteCount (max oldPeak staging) }
            pure (next, CompactArtifact destination (ByteCount (toInteger (B.length compressed))) (sha256 compressed), ok))
            `onException` (emergencyCaptures stock >> emergencyCaptures owned)
        removeUnretained stage
        loop (index+1) rest next (artifact:artifacts) (passed && ok)
  loop (0::Int) plans initial [] True

check :: CompactConfig -> StoragePhase -> Integer -> IO ()
check config phase bytes = compactCheck config (StorageCheckpoint phase (ByteCount bytes))

publishCompactEvidence :: CompactConfig -> FilePath -> B.ByteString -> IO ()
publishCompactEvidence config path bytes = do
  check config BeforeWrite (fileReservation config (toInteger (B.length bytes)))
  durableWrite path bytes
  check config AfterWrite 0

runSpecs :: CompactConfig -> E.InputPool -> FilePath -> [CaseSpec] -> [ObservationReceipt -> Bool] -> Integer -> IO [Captured]
runSpecs config pool directory specs accepts heldBytes = do
  unless (length (nub (map caseId specs)) == length specs) (fail "duplicate compact case identifier")
  unless (length specs == length accepts) (fail "compact validator count differs")
  let step previous (spec,accept) = do
        current <- runSingle config pool directory [spec] [accept] (heldBytes + capturedBytes config previous)
          `onException` emergencyCaptures previous
        pure (current ++ previous)
  reverse <$> foldM step [] (zip specs accepts)

runSingle :: CompactConfig -> E.InputPool -> FilePath -> [CaseSpec] -> [ObservationReceipt -> Bool] -> Integer -> IO [Captured]
runSingle config pool directory specs accepts heldBytes = do
  unless (length specs == 1 && length accepts == 1) (fail "compact execution requires a singleton native batch")
  let stagingConfig = reserveConfig config heldBytes
  check stagingConfig BeforeWrite (sum (map (stagingReservation config) specs))
  batch <- writeBatchWithInputPool pool directory specs
  syncTree directory
  syncAncestors (takeDirectory directory)
  check stagingConfig AfterWrite 0
  captured <- newIORef []
  count <- newIORef (0::Int)
  rawFrames <- newIORef []
  let preserve = readIORef rawFrames >>= mapM_ (uncurry durableWrite)
      sink report = do
        i <- readIORef count
        let raw = C.reportValue report
            rawPath = directory </> C.caseIdText (C.reportCase report) ++ ".native-result.json"
            rawBytes = encodeCanonical raw
            saveFailure = preserve
        modifyIORef' rawFrames ((rawPath,rawBytes):)
        (do
          spec <- case drop i specs of
            s:_ | C.reportCase report == C.CaseId (caseId s) -> pure s
            _ -> fail "compact callback order or identifier differs"
          (_,_,_,identity) <- C.loadCase (directory </> caseId spec ++ ".case.json")
          regions <- maybe (fail "compact callback has no admitted regions") pure (C.reportRegions report)
          exit <- either (fail . N.renderObservationError) pure (N.reportRunnerExit raw)
          observed <- either (fail . N.renderObservationError) pure (N.observeNative exit regions (get raw "runner"))
          receipt <- either fail pure (parseObservationReceipt (N.R3 `elem` map fst (caseExpectedRegisters spec)) (contractRunner (compactContract config)) (C.reportCase report) identity observed raw)
          modifyIORef' captured (Captured spec directory (RawReport rawBytes) receipt :)
          case drop i accepts of
            accept:_ | accept receipt -> pure ()
            _ -> fail "compact observation failed consumer, model, diagnostic or provenance judgment"
          current <- readIORef captured
          let retentionConfig = reserveConfig config (heldBytes + capturedBytes config current)
          when (observationRequiresFull receipt) (retainCapture retentionConfig (Captured spec directory (RawReport rawBytes) receipt) >> pure ())
          modifyIORef' count (+1)
          previous <- readIORef captured
          let next = drop (i+1) specs
          when (not (null next)) (check config BeforeChunk (heldBytes + capturedBytes config previous + maximum (map (failureReservation config) next)))) `onException` saveFailure
  check config BeforeChunk (heldBytes + maximum (0 : map (failureReservation config) specs))
  summary <- C.runBatch (compactRunner config) batch sink `onException` preserve
  rows <- reverse <$> readIORef captured
  let expected = map (C.CaseId . caseId) specs
      actual = map C.batchCaseId (C.summaryCases summary)
      complete = C.summaryStatus summary == C.BatchComplete && actual == expected && length rows == length specs
      runnerMatches = C.summaryRunnerDigest summary == Just (C.DigestHex (sha256Hex (contractRunner (compactContract config))))
  unless (complete && runnerMatches) $ do
    preserve
    publishCompactEvidence (reserveConfig config heldBytes) (directory </> "batch-failure.json") (encodeCanonical (C.summaryJson summary))
    fail "compact native session incomplete or identity differs"
  pure rows


retainCapture :: CompactConfig -> Captured -> IO Integer
retainCapture config (Captured spec directory (RawReport raw) _) = do
  let stage = takeDirectory directory
      target = takeDirectory stage </> "full" </> takeFileName stage </> takeFileName directory </> caseId spec
      descriptor = caseId spec ++ ".case.json"
  (value,_,_,_) <- C.loadCase (directory </> descriptor)
  check config BeforeWrite (directoryReservation config 4)
  createDirectoryIfMissing True target
  let names = descriptor : [str (get artifact "path") | artifact <- list (get value "artifacts")]
  forM_ names $ \name -> do
    let destination = target </> name
    exists <- doesFileExist destination
    unless exists $ do
      size <- getFileSize (directory </> name)
      check config BeforeWrite (fileReservation config size)
      copyFile (directory </> name) destination
      syncPath destination
      syncAncestors target
      check config AfterWrite 0
  let report = target </> "runner-report.json"
  exists <- doesFileExist report
  unless exists (publishCompactEvidence config report raw)
  pure (if exists then 0 else toInteger (B.length raw))

removeUnretained :: FilePath -> IO ()
removeUnretained = removeDirectoryRecursive

directoryBytes :: FilePath -> IO Integer
directoryBytes directory = do
  names <- listDirectory directory
  sizes <- forM names $ \name -> do
    let path = directory </> name
    directorySize path
  pure (sum sizes)
  where
    directorySize path = do
      isDirectory <- doesDirectoryExist path
      if isDirectory then directoryBytes path else getFileSize path

failureReservation :: CompactConfig -> CaseSpec -> Integer
failureReservation config spec =
  fileReservation config (6 * (toInteger (C.profileOutputBytes C.DefaultMemory) + toInteger C.batchStreamBytes)
    + toInteger (C.profileJsonBytes C.DefaultMemory)) + stagingReservation config spec

stagingReservation :: CompactConfig -> CaseSpec -> Integer
stagingReservation config spec =
  fileReservation config (65536 + 6 * toInteger (length (caseId spec) + length (caseProvenance spec) + length (caseExclusion spec))
    + 512 * toInteger (length (caseRegions spec)) + 256 * toInteger (length (caseFunctions spec))
    + 2 * sum [toInteger (B.length bytes) | (_,bytes) <- caseExpectedMemory spec])
    + fileReservation config (4096 + 6 * toInteger (length (caseId spec)))
    + 3 * sum [fileReservation config (C.regionSize (caseRegion region)) | region <- caseRegions spec]
    + directoryReservation config 4

stockAccepts :: PairPlan -> ObservationReceipt -> Bool
stockAccepts plan receipt = observationPassed receipt && diagnosticAllowed (planStock plan) receipt
  && (planRetention plan /= ExactDiagnosticFull || case planRequirement plan of MustDiffer _ -> True; MustAgree -> False)
  && maybe True (\expected -> null (expectationDifferences expected (observationRun receipt))) (planModel plan)

requirementPassed :: PairPlan -> PairReceipt -> Bool
requirementPassed plan receipt = case planRequirement plan of
  MustAgree -> pairPassed receipt
  MustDiffer expected -> planRetention plan == ExactDiagnosticFull
    && not (null expected) && all publicDifference expected
    && maybe False (const True) (pairModel receipt)
    && observationPassed (pairStock receipt) && observationPassed (pairOwned receipt)
    && null (pairStockModelDifferences receipt)
    && pairDifferences receipt == expected
  where
    publicDifference (RegisterDiffers _ _ _) = True
    publicDifference (MemoryDiffers _) = True
    publicDifference WritesDiffer = True
    publicDifference _ = False

encodePlannedPair :: PairPlan -> PairReceipt -> B.ByteString
encodePlannedPair plan receipt =
  let requirement = case planRequirement plan of
        MustAgree -> object ["kind" .= ("must-agree" :: String)]
        MustDiffer expected -> object
          ["kind" .= ("must-differ" :: String), "expected_differences" .=
            [object ["kind" .= differenceKind difference, "detail" .= differenceDetail difference] | difference <- expected]]
      prefix = encodeCanonical (object ["requirement" .= requirement, "requirement_passed" .= requirementPassed plan receipt])
  in B.init prefix <> ",\"pair_receipt\":" <> encodePairReceipt receipt <> "}"

compactResultValue :: CompactResult -> Value
compactResultValue result =
  let stats = compactStats result
      count (ByteCount bytes) = bytes
  in object
    [ "measurement_completed" .= True
    , "public_consumer_agreement" .= compactConsumerAgreement result
    , "qualification_passed" .= False
    , "sdk_failed_observations" .= compactSDKFailedObservations stats
    , "unsupported_source_bounds_observations" .= compactUnsupportedSourceBoundsObservations stats
    , "full_failure_reports" .= compactFullFailureReports stats
    , "session_directory" .= compactSessionDirectory result
    , "sdk_sessions" .= compactSessions stats
    , "observations" .= compactObservations stats
    , "pairs" .= (compactObservations stats `div` 2)
    , "receipt_bytes" .= count (compactReceiptBytes stats)
    , "full_raw_report_bytes" .= count (compactRawBytes stats)
    , "full_reprocase_artifacts_and_reports_bytes" .= count (compactFullEvidenceBytes stats)
    , "staging_peak_bytes" .= count (compactStagingPeakBytes stats)
    , "elapsed_nanoseconds" .= compactElapsedNanoseconds stats
    , "peak_resident_bytes" .= fmap count (compactPeakResidentBytes stats)
    , "receipts" .= [object
        ["path" .= compactArtifactPath artifact, "size" .= count (compactArtifactBytes artifact)
        , "sha256" .= sha256Hex (compactArtifactSHA artifact)] | artifact <- compactArtifacts result]
    ]

rawReportBytes :: FilePath -> IO Integer
rawReportBytes directory = do
  names <- listDirectory directory
  sizes <- forM names $ \name -> do
    let path = directory </> name
    nested <- doesDirectoryExist path
    if nested then rawReportBytes path else
      if name == "runner-report.json" then getFileSize path else pure 0
  pure (sum sizes)

capturedBytes :: CompactConfig -> [Captured] -> Integer
capturedBytes config = sum . map (\(Captured _ _ (RawReport raw) _) -> fileReservation config (toInteger (B.length raw)))

emergencyCaptures :: [Captured] -> IO ()
emergencyCaptures = mapM_ $ \(Captured spec directory (RawReport raw) _) ->
  durableWrite (directory </> caseId spec ++ ".native-result.json") raw

durableWrite :: FilePath -> B.ByteString -> IO ()
durableWrite path bytes = do
  exists <- doesFileExist path
  if exists then do
    previous <- B.readFile path
    unless (previous == bytes) (fail "compact publication already exists with different bytes")
    syncPath path
    syncAncestors (takeDirectory path)
  else do
    let pending = path ++ ".pending"
    B.writeFile pending bytes
    syncPath pending
    renameFile pending path
    syncAncestors (takeDirectory path)

syncPath :: FilePath -> IO ()
syncPath path = bracket (openFd path ReadOnly defaultFileFlags) closeFd fileSynchronise

syncAncestors :: FilePath -> IO ()
syncAncestors directory = do
  syncPath directory
  let parent = takeDirectory directory
  when (parent /= directory) (syncAncestors parent)

syncTree :: FilePath -> IO ()
syncTree directory = do
  names <- listDirectory directory
  forM_ names $ \name -> do
    let path = directory </> name
    nested <- doesDirectoryExist path
    if nested then syncTree path else syncPath path
  syncPath directory

reserveConfig :: CompactConfig -> Integer -> CompactConfig
reserveConfig config outstanding = config
  {compactCheck = \checkpoint ->
    let ByteCount pending = checkpointPendingBytes checkpoint
    in compactCheck config checkpoint {checkpointPendingBytes = ByteCount (pending + outstanding)}}

diagnosticAllowed :: CaseSpec -> ObservationReceipt -> Bool
diagnosticAllowed spec receipt
  | observationSDKStatus receipt == SDKPassed = True
  | sdkExactMismatches (observationSDK receipt) /= [MismatchDiagnostic "memory stack: differs from exact allowed final bytes"] = False
  | otherwise = privateStackBounds spec receipt

privateStackBounds :: CaseSpec -> ObservationReceipt -> Bool
privateStackBounds spec receipt
  | not (null (caseIgnoredMemory spec)) = False
  | otherwise = case (lookup N.Sp (caseInitial spec),
      [region | CaseRegion region ReadWrite <- caseRegions spec, N.regionIdOf region == N.RegionId "stack"]) of
      (Just sp,[stack]) -> C.baseAddress stack < toInteger sp
        && C.baseAddress stack + C.regionSize stack == toInteger sp
        && N.lookupRegister N.Sp (observationNative receipt) == Just sp
        && all (containedWrite (C.baseAddress stack) (toInteger sp)) (observationPhysicalWrites receipt)
        && maybe False ((== C.regionSize stack) . toInteger . B.length)
          (lookup (N.RegionId "stack") (N.nativeMemory (observationNative receipt)))
      _ -> False
  where
    containedWrite low high event =
      let start = toInteger (eventAddress event)
          width = toInteger (eventWidth event)
          end = start + width
      in width `elem` [1,2,4,8] && end <= 0x100000000
        && (end <= low || start >= high || (low <= start && end <= high))

allocationUnit :: AllocationGeometry -> Integer
allocationUnit Btrfs4096 = 4096

reserveFile :: AllocationGeometry -> ByteCount -> ByteCount
reserveFile geometry (ByteCount bytes) =
  let unit = allocationUnit geometry
  in ByteCount (((bytes + unit - 1) `div` unit + 2) * unit)

fileReservation :: CompactConfig -> Integer -> Integer
fileReservation config bytes =
  let ByteCount reserved = reserveFile (compactAllocationGeometry config) (ByteCount bytes)
  in reserved

directoryReservation :: CompactConfig -> Int -> Integer
directoryReservation config count = toInteger count * allocationUnit (compactAllocationGeometry config)

controlDiagnosticsAllowed :: CaseSpec -> [Difference] -> ObservationReceipt -> Bool
controlDiagnosticsAllowed spec expected receipt =
  let sdk = observationSDK receipt
      mismatches = sdkMismatches sdk
  in (observationSDKStatus receipt == SDKPassed || not (null mismatches))
    && all explained mismatches
  where
    explained (SDKRegisterMismatch register) = any (registerDifference register) expected
    explained (SDKMemoryMismatch region)
      | region == N.RegionId "stack" = privateStackBounds spec receipt
      | otherwise = any (memoryDifference region) expected
    explained _ = False
    registerDifference register (RegisterDiffers compared _ _) = register == compared
    registerDifference _ _ = False
    memoryDifference region (MemoryDiffers compared) = region == compared
    memoryDifference _ _ = False
