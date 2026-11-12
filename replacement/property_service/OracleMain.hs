{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Compares canonical public entries with the current linked body.
-- Native-only observations do not admit DAT retirement.
module OracleMain (main, ExecutionMode (..), FrozenRun, FrozenBatchName (..), BatchExecution (..), executionMode, loadFrozenRun, observeBatch, consumeReport, finishFrozenRun, ResearchMode (..), researchMode, checkCompactBudget, parseCompactAllocationGeometry) where

import qualified ArmElf as ELF
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON, Value (..), eitherDecodeStrict', object, withObject, (.=))
import Data.Aeson.Types (parseEither, (.:))
import qualified Data.Aeson.Key as K
import qualified Emulation.NativeCase as NC
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.Containers.ListUtils (nubOrd)
import Data.IORef (IORef, atomicModifyIORef', newIORef, readIORef)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, listToMaybe)
import qualified Data.Set as S
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse (..), ResultPolicy (WriteAllResults), cachedObservedBatchWith, defaultNativeCacheRoot)
import Emulation.Judge (StockEvidence (..), evidenceAt)
import Emulation.NativeObservation (RunnerStatus (Returned), runnerStatusLabel)
import qualified Emulation.NativeObservation as N
import Emulation.OwnedCase (CaseId (..), CaseSpec (..), CaseRegion (..), loadObservedBatch, readWordAt, required, word32le, writeBatch)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import qualified EngineStack
import FujiAF.Host (ensure, failH, get, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Admission (FunctionExtent (..))
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.ClassRecord (ownedLabel, reasonLabel, stockLabel)
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding (EncodedEntry (..), EncodedSlot (..), EncodedSpecial (..), SlotIndex (..), SpecialArm (..), TableEntry (..), encodeEntry, encodeSlot, encodeSpecial)
import Replacement.PropertyService.Frame (FrameDepth (..), caseFrameDepth, withPrivateFrame)
import Replacement.PropertyService.Oracle
import qualified Replacement.PropertyService.Compact as Compact
import qualified Replacement.PropertyService.Placement as P
import Replacement.PropertyService.Qualification (CheckpointPhase (..), StorageAdmission (..), StorageLimitBytes (..), StorageBytes (..), StorageSnapshot (..), measureStorage, BatchCheckpoint (..), parseStorageLimit, parseBatchCheckpoint, checkStoragePaths, storageCheckpoint, resolveRuntimePath)
import qualified StockStack as Stack
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, setCurrentDirectory)
import System.Environment (getArgs, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (ExitSuccess), exitFailure, exitSuccess)
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.Process (readProcessWithExitCode)

canonicalImage4Sha256 :: String
canonicalImage4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

batchLimit :: Int
batchLimit = 512

movR0R0Word :: Word32
movR0R0Word = 0xe1a00000

fixtureImage :: [(DefaultImage,P.GuestAddress)] -> Fixture -> B.ByteString -> B.ByteString
fixtureImage windows fx image = foldl inject image windows
  where
    inject bytes (kind,P.GuestAddress address) =
      let offset = fromIntegral (address-image4Base)
          value = case kind of
            SlotDefaultBytes -> fixtureSlotDefaults fx
            TailDefaultBytes -> B.take 40 (fixtureTailDefaults fx)
      in B.take offset bytes <> value <> B.drop (offset+B.length value) bytes

data BatchRecord = BatchRecord
  { recordName :: !String
  , recordCases :: !Int
  , recordBatchSha :: !Sha256
  , recordSummarySha :: !Sha256
  , recordReuse :: !BatchReuse
  }

reuseLabel :: BatchReuse -> String
reuseLabel BatchExecuted = "executed"
reuseLabel BatchReused = "reused"

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

propertyHex :: PropertyId -> String
propertyHex (PropertyId n) = hex n

chunks :: Int -> [a] -> [[a]]
chunks size items = case splitAt size items of
  (chunk, []) -> [chunk | not (null chunk)]
  (chunk, rest) -> chunk : chunks size rest

orFail :: Show e => String -> Either e a -> IO a
orFail context = either (\problem -> failH (context ++ ": " ++ show problem)) pure

data CandidateMode = NativeOnly | ActualStagedCandidate FilePath FilePath

data CandidateEvidence = NativeEvidence NativeImage4View | StagedEvidence FilePath FilePath Sha256

candidateMode :: [String] -> IO CandidateMode
candidateMode arguments
  | "--native-only" `elem` arguments = do
      ensure (not (any (`elem` arguments) ["--candidate-image4", "--contract"])) "--native-only cannot accept a staged candidate or DAT contract"
      pure NativeOnly
  | otherwise = ActualStagedCandidate <$> required arguments "--candidate-image4" <*> required arguments "--contract"


data ExecutionMode = ExecuteNative | RejudgeRunRoot FilePath Sha256 deriving Eq

data FrozenRun = FrozenRun
  { frozenRoot :: !FilePath
  , frozenResultsSha :: !Sha256
  , frozenResults :: !Value
  , frozenBatches :: ![Value]
  , frozenConsumed :: !(IORef [String])
  , frozenEvidence :: !(M.Map String FrozenBatchEvidence)
  }

newtype FrozenBatchName = FrozenBatchName String deriving (Eq, Ord)

data FrozenBatchEvidence = FrozenBatchEvidence
  { sourceBatchValue :: !Value
  , sourceSummaryValue :: !Value
  }

data BatchExecution = ExecuteBatch FilePath (Maybe FilePath) | RejudgeBatch FrozenRun

executionMode :: [String] -> IO ExecutionMode
executionMode arguments = case dropWhile (/= "--rejudge-run-root") arguments of
  [] -> do
    ensure (not ("--rejudge-results-sha256" `elem` arguments)) "--rejudge-results-sha256 requires --rejudge-run-root"
    pure ExecuteNative
  [_] -> failH "--rejudge-run-root needs an existing root"
  _ : root : _ -> do
    ensure (length (filter (== "--rejudge-run-root") arguments) == 1) "duplicate --rejudge-run-root"
    ensure (not (any (`elem` arguments) ["--cache", "--batch-checkpoint"])) "offline rejudgment cannot use cache or batch checkpoints"
    ensure (length (filter (== "--rejudge-results-sha256") arguments) == 1) "offline rejudgment requires one --rejudge-results-sha256"
    anchor <- required arguments "--rejudge-results-sha256" >>= either failH pure . parseSha256Hex
    RejudgeRunRoot <$> canonicalizePath root <*> pure anchor

field :: FromJSON a => Value -> String -> IO a
field value name = either failH pure (parseEither (withObject name (.: K.fromString name)) value)

readEvidence :: FilePath -> IO (B.ByteString, Value)
readEvidence path = do
  bytes <- readRegular (64 * 1024 * 1024) path
  value <- either (failH . ((path ++ ": ") ++)) pure (eitherDecodeStrict' bytes)
  pure (bytes, value)

inputDigests :: M.Map String Value -> IO (M.Map String Sha256)
inputDigests inputs = do
  ensure (not (M.null inputs)) "missing source input inventory"
  forM inputs $ \identity -> do
    path <- field identity "path" :: IO FilePath
    digest <- field identity "sha256" >>= either failH pure . parseSha256Hex
    ensure (not (null path)) "malformed input path"
    pure digest

loadFrozenRun :: FilePath -> FilePath -> Value -> Sha256 -> IO FrozenRun
loadFrozenRun root runner inputs anchor = do
  canonicalRoot <- canonicalizePath root
  (bytes, results) <- readEvidence (canonicalRoot </> "results.json")
  ensure (sha256 bytes == anchor) "source results SHA differs from the required external trust anchor"
  version <- field results "schema_version" :: IO Int
  ensure (version == 1) "unsupported source results schema"
  _ <- field results "passed" :: IO Bool
  retirement <- field results "dat_retirement_admitted"
  forM_ ["qualification_scope", "qualification_status", "native_image4_view", "retired_fetch_guard", "coverage", "wrappers", "entry_adapters", "case_shape", "totals", "model_agreement", "pass2", "status_distribution", "controls", "differences"] $ \name -> do
    value <- field results name :: IO Value
    ensure (value /= Null) ("source results missing complete " ++ name)
  ensure (not retirement) "source observations admitted retirement"
  sourceInputs <- field results "inputs" >>= inputDigests
  currentInputs <- field (object ["inputs" .= inputs]) "inputs" >>= inputDigests
  ensure (sourceInputs == currentInputs) "source input identities differ from current canonical inputs"
  sourceRunner <- field results "runner" :: IO Value
  sourcePath <- field sourceRunner "path"
  actualPath <- canonicalizePath runner
  ensure (sourcePath == actualPath) "source runner path differs from immutable actual runner"
  sourceSha <- field sourceRunner "sha256"
  actualSha <- sha256Hex <$> sha256File actualPath
  ensure (sourceSha == actualSha) "source runner SHA differs from immutable actual runner"
  batches <- field results "batches" :: IO [Value]
  names <- mapM (`field` "name") batches :: IO [String]
  ensure (not (null names) && length names == S.size (S.fromList names)) "source batch inventory is empty or duplicated"
  evidence <- forM batches $ \record -> do
    name <- field record "name"
    ensure (takeFileName name == name && name `notElem` [".", ".."]) "unsafe source batch name"
    count <- field record "cases"
    ensure (count > (0 :: Int)) "empty source batch"
    (_, batch) <- readAuthenticatedEvidence record "batch_sha256" (canonicalRoot </> name </> "batch.json")
    (_, summary) <- readAuthenticatedEvidence record "summary_sha256" (canonicalRoot </> name </> "summary.json")
    pure (name, count, FrozenBatchEvidence batch summary)
  execution <- field results "execution" :: IO Value
  executed <- field execution "cases_executed"
  reused <- field execution "cases_reused"
  executedBatches <- field execution "batches_executed"
  reusedBatches <- field execution "batches_reused"
  let counts = [count | (_, count, _) <- evidence]
  ensure (executed + reused == sum counts && executedBatches + reusedBatches == length batches) "source execution inventory is incomplete"
  consumed <- newIORef []
  pure (FrozenRun canonicalRoot (sha256 bytes) results batches consumed (M.fromList [(name, batch) | (name, _, batch) <- evidence]))

verifyBytes :: Value -> String -> FilePath -> B.ByteString -> IO ()
verifyBytes identity name path bytes = do
  expected <- field identity name
  ensure (expected == sha256Hex (sha256 bytes)) (path ++ ": " ++ name ++ " mismatch")

readAuthenticatedEvidence :: Value -> String -> FilePath -> IO (B.ByteString, Value)
readAuthenticatedEvidence identity name path = do
  evidence@(bytes, _) <- readEvidence path
  verifyBytes identity name path bytes
  pure evidence

frozenObservation :: FrozenRun -> FilePath -> String -> [Value] -> String -> IO N.NativeObservation
frozenObservation frozen freshDirectory batchName rows label = do
  let oldDirectory = frozenRoot frozen </> batchName
      caseFile = label ++ ".case.json"
      reportFile = label ++ ".native-result.json"
  (_, regions, _, currentIdentity) <- NC.loadCase (freshDirectory </> caseFile)
  (_, _, _, oldIdentity) <- NC.loadCase (oldDirectory </> caseFile)
  ensure (oldIdentity == currentIdentity) (label ++ ": source case/wire/artifact identities differ")
  row <- reportIdentity rows (CaseId label)
  (reportBytes, report) <- readAuthenticatedEvidence row "result_sha256" (oldDirectory </> reportFile)
  forM_ currentIdentity $ \(name, identity) -> do
    actual <- field report name
    ensure (actual == identity) (label ++ ": report " ++ name ++ " differs from current case")
  runnerIdentity <- field (frozenResults frozen) "runner" :: IO Value
  runnerSha <- field runnerIdentity "sha256" :: IO String
  reportSha <- field report "runner_sha256"
  ensure (reportSha == runnerSha) (label ++ ": report runner identity differs")
  exit <- orFail (label ++ ": runner exit") (N.reportRunnerExit report)
  observation <- orFail (label ++ ": native provenance") (N.observeNative exit regions (get report "runner"))
  ensure (isJust (N.provenanceWrites (N.nativeProvenance observation))) (label ++ ": incomplete physical write provenance")
  reportId <- field report "case_id"
  ensure (reportId == label) (label ++ ": physical report case id differs")
  status <- field row "status"
  ensure (status == runnerStatusLabel (N.nativeStatus observation)) (label ++ ": summary status differs")
  B.writeFile (freshDirectory </> reportFile) reportBytes
  pure observation

reportIdentity :: [Value] -> CaseId -> IO Value
reportIdentity rows (CaseId label) = do
  matches <- fmap concat $ forM rows $ \row -> do
    actual <- field row "id"
    pure [row | actual == label]
  case matches of
    [one] -> pure one
    _ -> failH (label ++ ": missing or duplicated physical report identity")

data ConsumedReport = ConsumedReport
  { consumedValue :: !Value
  , consumedDigest :: !Sha256
  }

consumeReport :: BatchExecution -> FilePath -> CaseId -> IO ConsumedReport
consumeReport execution directory identifier@(CaseId label) = do
  let reportPath = directory </> label ++ ".native-result.json"
  (bytes, value) <- readEvidence reportPath
  case execution of
    ExecuteBatch _ _ -> pure ()
    RejudgeBatch source -> do
      evidence <- maybe (failH "missing authenticated source batch") pure (M.lookup (takeFileName directory) (frozenEvidence source))
      rows <- field (sourceSummaryValue evidence) "cases"
      identity <- reportIdentity rows identifier
      verifyBytes identity "result_sha256" reportPath bytes
  pure (ConsumedReport value (sha256 bytes))

finishFrozenRun :: FrozenRun -> [FrozenBatchName] -> IO ()
finishFrozenRun frozen currentNames = do
  names <- map FrozenBatchName <$> mapM (`field` "name") (frozenBatches frozen)
  ensure (names == currentNames) "source batch order or inventory differs from current cases"
  consumed <- reverse <$> readIORef (frozenConsumed frozen)
  ensure (S.fromList (map FrozenBatchName consumed) == S.fromList names && length consumed == length names) "source observations were not consumed exactly once"
data BatchGate = BatchGate StorageAdmission [FilePath] (Maybe BatchCheckpoint) (IORef Int) (Maybe FrozenRun)

batchGate :: [String] -> Maybe FilePath -> FilePath -> IO BatchGate
batchGate arguments cache out = do
  root <- required arguments "--storage-root" >>= makeAbsolute >>= canonicalizePath
  limit <- required arguments "--storage-limit-bytes" >>= either failH pure . parseStorageLimit
  let admission = StorageAdmission root limit
  checkStoragePaths admission (out : maybe [] pure cache)
  mapM_ (createDirectoryIfMissing True) cache
  checkpoint <- case dropWhile (/= "--batch-checkpoint") arguments of
    [] -> pure Nothing
    [_] -> failH "--batch-checkpoint needs a positive count"
    _ : value : _ -> Just <$> either failH pure (parseBatchCheckpoint value)
  counter <- newIORef 0
  pure (BatchGate admission (out : maybe [] pure cache) checkpoint counter Nothing)

frozenBatch :: FrozenRun -> FilePath -> [CaseId] -> IO [(CaseId, N.NativeObservation)]
frozenBatch source directory identifiers = do
  let name = takeFileName directory
      batchPath = directory </> "batch.json"
      labels = [label | CaseId label <- identifiers]
  records <- fmap concat $ forM (frozenBatches source) $ \record -> do
    actual <- field record "name"
    pure [record | actual == name]
  record <- case records of
    [one] -> pure one
    _ -> failH (name ++ ": missing source batch")
  (currentBatchBytes, _) <- readEvidence batchPath
  verifyBytes record "batch_sha256" batchPath currentBatchBytes
  evidence <- maybe (failH (name ++ ": missing authenticated source batch")) pure (M.lookup name (frozenEvidence source))
  let oldBatch = sourceBatchValue evidence
      summary = sourceSummaryValue evidence
  verifyBytes summary "batch_sha256" batchPath currentBatchBytes
  oldCases <- field oldBatch "cases" :: IO [Value]
  oldIds <- mapM (`field` "id") oldCases
  summaryCases <- field summary "cases" :: IO [Value]
  summaryIds <- mapM (`field` "id") summaryCases
  count <- field record "cases"
  ensure (oldIds == labels && summaryIds == oldIds && count == length identifiers) (name ++ ": source case order/count differs")
  observations <- forM identifiers $ \ident@(CaseId label) -> do
    observation <- frozenObservation source directory name summaryCases label
    pure (ident, observation)
  atomicModifyIORef' (frozenConsumed source) (\names -> (name : names, ()))
  pure observations

observeBatch :: BatchExecution -> FilePath -> [CaseId] -> IO (BatchReuse, [(CaseId, N.NativeObservation)])
observeBatch execution directory identifiers = case execution of
  RejudgeBatch source -> do
    observations <- frozenBatch source directory identifiers
    pure (BatchExecuted, observations)
  ExecuteBatch runner cache -> case cache of
    Nothing -> do
      observations <- loadObservedBatch runner (directory </> "batch.json")
      pure (BatchExecuted, observations)
    Just cacheRoot -> cachedObservedBatchWith WriteAllResults cacheRoot runner (directory </> "batch.json")

-- | One runner session over at most 'batchLimit' cases in @directory@, its runs keyed by case label.
runBatch :: BatchGate -> Maybe FilePath -> FilePath -> FilePath -> [CaseSpec] -> IO (BatchRecord, M.Map CaseLabel Run)
runBatch (BatchGate admission measuredPaths checkpoint counter frozen) cache runner directory specs = do
  storageCheckpoint BeforeBatch admission measuredPaths
  ensure (length specs <= batchLimit) "batch over the case limit"
  batchPath <- writeBatch directory specs
  let execution = maybe (ExecuteBatch runner cache) RejudgeBatch frozen
  (reuse, observed) <- observeBatch execution directory (map (CaseId . caseId) specs)
  ensure (length observed == length specs) (batchPath ++ ": observation count differs from the case count")
  rows <- forM (zip specs observed) $ \(spec, (CaseId label, observation)) -> do
    consumed <- consumeReport execution directory (CaseId label)
    let report = consumedValue consumed
        reportPath = directory </> label ++ ".native-result.json"
    writes <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (reportWrites report)
    run <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (runOf observation writes)
    let abiRun = if any ((== N.R3) . fst) (caseExpectedRegisters spec) then run else run {runRegisters = filter ((/= N.R3) . fst) (runRegisters run)}
    forced <- evaluate (forceRun abiRun)
    pure (CaseLabel label, forced, sha256Hex (consumedDigest consumed))
  batchBytes <- B.readFile batchPath
  let batchDigest = sha256 batchBytes
      batchSha = sha256Hex batchDigest
      summary = encodeCanonical $ object
        [ "batch_sha256" .= batchSha
        , "cases" .= [object ["id" .= label, "status" .= runnerStatusLabel (runStatus run), "result_sha256" .= digest] | (CaseLabel label, run, digest) <- rows]
        ]
  B.writeFile (directory </> "summary.json") summary
  record <- evaluate (BatchRecord (takeFileName directory) (length specs) batchDigest (sha256 summary) reuse)
  runs <- evaluate (M.fromList [(label, run) | (label, run, _) <- rows])
  storageCheckpoint AfterBatch admission measuredPaths
  completed <- atomicModifyIORef' counter (\count -> let next = count + 1 in (next, next))
  when (maybe False (\(BatchCheckpoint limit) -> completed >= limit) checkpoint)
    (failH ("batch checkpoint reached after " ++ show completed ++ " complete chunks; passed=false qualification_status=partial"))
  pure (record, runs)

runBatches :: BatchGate -> Maybe FilePath -> FilePath -> FilePath -> String -> [CaseSpec] -> IO ([BatchRecord], M.Map CaseLabel Run)
runBatches gate cache runner out prefix specs = do
  results <- forM (zip [0 :: Int ..] (chunks batchLimit specs)) $ \(index, chunk) ->
    runBatch gate cache runner (out </> prefix ++ "-" ++ (if index < 10 then "0" else "") ++ show index) chunk
  pure (map fst results, M.unions (map snd results))

lookupRun :: M.Map CaseLabel Run -> CaseLabel -> IO Run
lookupRun runs label = maybe (failH ("no run for " ++ caseLabelText label)) pure (M.lookup label runs)


-- | A case under one fixture variant.
data RunCase = RunCase { runVariant :: FixtureVariant, runCase :: OracleCase } deriving (Eq, Ord)

canonical :: OracleCase -> RunCase
canonical = RunCase CanonicalFixture

runLabel :: RunCase -> CaseLabel
runLabel (RunCase variant c) = case variant of
  CanonicalFixture -> caseLabel c
  _ -> prefixedCaseLabel (CaseLabel (variantTag variant)) (caseLabel c)

runPrefixed :: String -> RunCase -> CaseLabel
runPrefixed prefix rc = prefixedCaseLabel (CaseLabel prefix) (runLabel rc)

runProperty :: RunCase -> PropertyId
runProperty = oracleProperty . runCase

classLabel :: Standing -> String
classLabel standing = case standing of
  Classified (Owned owned) -> ownedLabel owned
  Classified (Delegated _) -> "delegated"
  OutOfRange -> "out-of-range"

statusCounts :: [Run] -> Value
statusCounts runs = object [fromString status .= count | (status, count) <- M.toList (M.fromListWith (+) [(runnerStatusLabel (runStatus run), 1 :: Int) | run <- runs])]

-- | The first 20 differences of each kind, in case order.
differenceSamples :: [(String, CaseLabel, Difference)] -> Value
differenceSamples rows = object
  [ fromString kind .= [object ["case" .= caseLabelText label, "detail" .= differenceDetail difference] | (label, difference) <- take 20 (reverse samples)]
  | (kind, samples) <- M.toList (M.fromListWith (++) [(phase ++ ":" ++ differenceKind difference, [(label, difference)]) | (phase, label, difference) <- rows])
  ]

stockSpec :: Fixture -> B.ByteString -> M.Map PropertyId Property -> CaseLabel -> [FunctionSpan] -> OracleCase -> CaseSpec
stockSpec fx image properties label functions oracleCase =
  caseSpec image functions fx label standing oracleCase (fromMaybe placeholderExpectation (modelled standing >>= \owned -> modelExpectation fx owned (oracleOperation oracleCase)))
  where
    standing = standingOf properties (oracleProperty oracleCase)

-- | The case with the private frame its entry path derives, or unchanged when that path has no
-- bound: an unbounded case keeps exact stack bytes, so its observation keeps the stack diagnostic.
withDerivedFrame :: Stack.Supplied -> [(Word32, Word32)] -> CaseSpec -> CaseSpec
withDerivedFrame supplied ownedRanges spec =
  case caseFrameDepth ownedRanges supplied spec of
    Left _ -> spec
    Right depth -> either (\problem -> error (caseId spec ++ ": " ++ problem)) id (withPrivateFrame depth spec)

data ResearchMode = OrdinaryResearch | CompactMeasurement deriving (Eq, Show)

researchMode :: [String] -> IO ResearchMode
researchMode arguments
  | "--compact-measurement" `notElem` arguments = pure OrdinaryResearch
  | otherwise = do
      ensure (length (filter (== "--compact-measurement") arguments) == 1) "duplicate --compact-measurement"
      ensure ("--native-only" `elem` arguments) "--compact-measurement requires --native-only"
      ensure (not (any (`elem` arguments) ["--smoke", "--ids", "--cache", "--rejudge-run-root", "--rejudge-results-sha256", "--batch-checkpoint", "--candidate-image4", "--contract"])) "compact measurement cannot narrow cases, reuse evidence or admit retirement"
      pure CompactMeasurement

checkCompactBudget :: StorageLimitBytes -> StorageSnapshot -> StorageSnapshot -> Compact.ByteCount -> Either String ()
checkCompactBudget (StorageLimitBytes limit)
    (StorageSnapshot (StorageBytes rootUsed) (StorageBytes free) _)
    (StorageSnapshot (StorageBytes outputUsed) _ _)
    (Compact.ByteCount pending)
  | pending < 0 = Left "negative compact write reservation"
  | rootUsed + pending + 2147483648 > limit = Left "compact root lacks its 2GiB checkpoint margin"
  | rootUsed - 72711983104 + pending > 1073741824 = Left "compact cumulative new proof exceeds 1GiB"
  | outputUsed + pending > 201326592 = Left "compact new retention exceeds 192MiB"
  | free - pending < 135115554816 = Left "compact physical free space crosses the fixed phase floor"
  | otherwise = Right ()

parseCompactAllocationGeometry :: String -> Either String Compact.AllocationGeometry
parseCompactAllocationGeometry measured = case lines measured of
  [row] | words row == ["btrfs", "4096", "4096"] -> Right Compact.Btrfs4096
  _ -> Left "compact physical reservation requires the measured btrfs 4096-byte allocation geometry"

data CompactStorageScope = CompactStorageScope
  { checkCompactStorage :: Compact.StorageCheckpoint -> IO ()
  , finishCompactStorage :: IO ()
  }

compactStorageScope :: StorageAdmission -> FilePath -> IO CompactStorageScope
compactStorageScope admission@(StorageAdmission _ limit) out = do
  initialRoot <- measureStorage admission Nothing
  initialOutput <- measureStorage (StorageAdmission out limit) Nothing
  let StorageBytes rootBytes = storageAllocated initialRoot
      StorageBytes outputBytes = storageAllocated initialOutput
      outsideOutput = rootBytes - outputBytes
      check checkpoint = do
        measured <- measureStorage (StorageAdmission out limit) Nothing
        let StorageBytes currentOutput = storageAllocated measured
            accountedRoot = measured {storageAllocated = StorageBytes (outsideOutput + currentOutput)}
        either failH pure (checkCompactBudget limit accountedRoot measured (Compact.checkpointPendingBytes checkpoint))
        putStrLn ("compact storage " ++ show (Compact.checkpointPhase checkpoint) ++ ": accounted_root=" ++ show (storageAllocated accountedRoot) ++ " output=" ++ show (storageAllocated measured) ++ " available=" ++ show (storageAvailable measured) ++ " pending=" ++ show (Compact.checkpointPendingBytes checkpoint))
      finish = do
        measuredRoot <- measureStorage admission Nothing
        measuredOutput <- measureStorage (StorageAdmission out limit) Nothing
        let StorageBytes finalRoot = storageAllocated measuredRoot
            StorageBytes finalOutput = storageAllocated measuredOutput
        ensure (finalRoot - finalOutput == outsideOutput) "evidence outside the exclusive compact output changed"
        either failH pure (checkCompactBudget limit measuredRoot measuredOutput (Compact.ByteCount 0))
  -- This scope owns the output; the rest of its admitted evidence root is immutable.
  ensure (outsideOutput >= 0) "compact output allocation exceeds its containing root"
  pure (CompactStorageScope check finish)

noEffectExpectation :: Operation -> CaseSpec -> Expectation
noEffectExpectation operation spec = Expectation Returned registers memory
  where
    registers = (N.R0, 0) :
      [(register, value) | (register, value) <- caseInitial spec, register /= N.R0,
        register `elem` comparedRegisters || (register == N.R3 && case operation of Set {} -> True; Get {} -> False)]
    memory =
      [(ident, NC.regionBytes region) | CaseRegion region _ <- caseRegions spec,
        let ident = N.regionIdOf region, ident `elem` [name | (name, _, _) <- comparedRegions]]


-- | Whether a control's patched image must fail a case or pass every case.
data ControlKind = MustFail | MustPass deriving (Eq)

-- | A control's image: the candidate with words replaced, or a defect build staged like the candidate.
data ControlImage = PatchedCandidate [Patch] | DefectBuild DefectName

-- | One `property_defect_*` cfg of the Rust crate, as `--defect` names it.
newtype DefectName = DefectName String deriving (Eq, Ord)

data Control = Control
  { controlName :: String
  , controlKind :: ControlKind
  , controlImage :: ControlImage
  , controlCases :: [RunCase]
  , controlNote :: String
  }

controlHolds :: Control -> Int -> Bool
controlHolds control failed = case controlKind control of
  MustFail -> failed > 0
  MustPass -> failed == 0

main :: IO ()
main = do
  arguments <- getArgs
  case dropWhile (/= "--emit-case-table") arguments of
    [] -> pure ()
    [_] -> failH "--emit-case-table needs a path"
    _ : path : _ -> do
      B.writeFile path (encodeCanonical caseTableValue)
      exitSuccess
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    [_] -> failH "--runner needs a value"
    _ : value : _ -> makeAbsolute value
  bundled <- mapM (\name -> required arguments name >>= resolveRuntimePath) ["--linked", "--symbols", "--elf"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  case bundled of
    [linkedPath, symbolsPath, elfPath] -> oracle arguments runner linkedPath symbolsPath elfPath
    _ -> failH "unreachable argument shape"

freshOutput :: FilePath -> IO FilePath
freshOutput path = do
  exists <- doesPathExist path
  when exists (failH (path ++ " already exists"))
  absolute <- makeAbsolute path
  createDirectoryIfMissing True (takeDirectory absolute)
  createDirectory absolute
  pure absolute

-- | Each defect binds its binary, symbols and ELF.
defectArguments :: [String] -> [(String, FilePath, FilePath, FilePath)]
defectArguments arguments = case arguments of
  "--defect" : name : binPath : symbolsPath : elfPath : rest -> (name, binPath, symbolsPath, elfPath) : defectArguments rest
  _ : rest -> defectArguments rest
  [] -> []

-- | Wrong variants of the owned image: a name and the one word the mutants
-- tool changed, applied to the same candidate bytes the base run uses.
variantArguments :: [String] -> IO [(String, Word32, Word32)]
variantArguments arguments = case arguments of
  "--variant" : name : address : mutated : rest -> do
    site <- hexWord address
    word <- hexWord mutated
    ((name, site, word) :) <$> variantArguments rest
  _ : rest -> variantArguments rest
  [] -> pure []
  where
    hexWord text = case readHex (if take 2 text == "0x" then drop 2 text else text) :: [(Integer, String)] of
      [(value, "")] -> pure (fromIntegral value :: Word32)
      _ -> failH ("--variant needs hex addresses, not " ++ text)

-- | The entry shape the runner validates: the canonical branches or the owned public wrappers.
entryShape :: [String] -> PublicEntryShape
entryShape arguments = if "--public-entries" `elem` arguments then PublicWrapperEntry else CanonicalEntry

deriveFor :: PublicEntryShape -> ArtifactPurpose -> [Property] -> P.Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveFor shape = case shape of
  CanonicalEntry -> deriveArtifact
  PublicWrapperEntry -> derivePublicArtifact

stageFor :: PublicEntryShape -> CanonicalSource -> LinkedArtifact -> Either ArtifactError B.ByteString
stageFor shape = case shape of
  CanonicalEntry -> stagedImage4Bytes
  PublicWrapperEntry -> \source artifact -> stagedBytes <$> stageNativeImage4 source artifact

-- | The ids `--ids` lists as comma-separated hex, or Nothing for every id.
subsetIds :: [String] -> IO (Maybe (S.Set PropertyId))
subsetIds arguments = case dropWhile (/= "--ids") arguments of
  _ : text : _ -> Just . S.fromList <$> mapM parse (splitOn text)
  [_] -> failH "--ids needs a value"
  [] -> pure Nothing
  where
    splitOn text = case break (== ',') text of
      (item, []) -> [item]
      (item, _ : rest) -> item : splitOn rest
    parse item = case item of
      '0' : 'x' : digits | [(n, "")] <- readHex digits, n <= 646 -> pure (PropertyId n)
      _ -> failH ("--ids: " ++ item ++ " is not a hex id from 0x0 to 0x286")

-- | The native batch cache root. Absent selects the shared cache.
cacheOption :: [String] -> IO (Maybe FilePath)
cacheOption arguments = case dropWhile (/= "--cache") arguments of
  _ : value : _ -> Just <$> makeAbsolute value
  [_] -> failH "--cache needs a value"
  [] -> Just <$> makeAbsolute defaultNativeCacheRoot

oracle :: [String] -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
oracle arguments runner linkedPath symbolsPath elfPath = do
  let shape = entryShape arguments
  research <- researchMode arguments
  execution <- executionMode arguments
  cache <- cacheOption arguments
  let input name = required arguments name
  image4Path <- input "--image4"
  mode <- candidateMode arguments
  when ("--smoke" `elem` arguments) $ case mode of
    NativeOnly -> ensure (not ("--ids" `elem` arguments)) "--smoke uses a fixed bounded case set, not --ids"
    ActualStagedCandidate _ _ -> failH "--smoke requires --native-only"
  sourcePaths <- mapM input ["--dat", "--loader-a", "--loader-b", "--image5", "--plain", "--image2"]
  outPath <- input "--out"
  subset <- subsetIds arguments
  initialGate <- batchGate arguments cache outPath
  image4 <- readRegular 0x300000 image4Path
  ensure (sha256Hex (sha256 image4) == canonicalImage4Sha256) "--image4 is not the canonical X-M1 1.50 image4"
  [dat, loaderA, loaderB, image5, plain, image2] <- forM (zip [64 * 1024 * 1024, 0x20000, 0x20000, 0x800000, 0x800000, 0x800000] sourcePaths) (uncurry readRegular)
  linked <- readRegular (P.carrierBytes P.stage4Carrier) linkedPath
  symbols <- readRegular 0x10000 symbolsPath
  elf <- readRegular (16 * 1024 * 1024) elfPath
  source <- orFail "canonical sources" (canonicalSource dat image4 loaderA loaderB image5 plain image2)
  let chosenCarrier = P.stage4Carrier
      classified = sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
  artifact <- orFail "linked artifact" (deriveFor shape ProductionArtifact classified chosenCarrier (ArtifactInput linked symbols elf))
  linkedElf <- orFail "linked ELF" (ELF.parseElf elf)
  let carrier = artifactCarrier artifact
      P.EntryAddresses (P.GuestAddress linkedGetterEntry) (P.GuestAddress linkedSetterEntry) = artifactEntries artifact
      inRuntime address = P.inCarrier carrier (P.GuestAddress address)
      ownedEntries =
        [ (name, address, EngineStack.stackDepth linkedElf inRuntime name)
        | (name, address) <- zip entryNames [linkedGetterEntry, linkedSetterEntry] ]
      entryNames = case shape of
        CanonicalEntry -> ["xm1_property_get_entry", "xm1_property_set_entry"]
        PublicWrapperEntry -> ["xm1_public_get", "xm1_public_set"]
      ownedSupplied = Stack.Supplied (M.fromList [(address, Stack.StackDepth depth) | (_, address, Right depth) <- ownedEntries])
      stockSupplied = Stack.Supplied M.empty
      ownedRanges = [(start, end) | P.Carrier (P.GuestAddress start) (P.GuestAddress end) <- [carrier]]
  (candidate, evidence) <- case mode of
    NativeOnly -> do
      view <- orFail "native image4 view" (nativeImage4View source artifact)
      orFail "native image4 readback" (validateNativeImage4View view (nativeViewBytes view))
      pure (nativeViewBytes view, NativeEvidence view)
    ActualStagedCandidate candidatePath contractPath -> do
      candidate <- readRegular 0x300000 candidatePath
      contractBytes <- readRegular 0x1000000 contractPath
      contract <- either (\problem -> failH (contractPath ++ ": " ++ problem)) pure (eitherDecodeStrict' contractBytes :: Either String Value)
      let digest = sha256 candidate
      ensure (sha256Hex digest == str (get contract "staged_image4_sha256")) "candidate image4 differs from the contract's staged_image4_sha256"
      ensure (str (get contract "elf_sha256") == sha256Hex (sha256 elf)) "linked ELF differs from the contract's elf_sha256"
      rederived <- orFail "staging" (stageFor shape source artifact)
      ensure (rederived == candidate) "candidate image4 differs from the re-derived staged image4"
      pure (candidate, StagedEvidence candidatePath contractPath digest)
  defectInputs <- forM (defectArguments arguments) $ \(name, binPath, defectSymbolsPath, defectElfPath) -> do
    resolvedBin <- resolveRuntimePath binPath
    resolvedSymbols <- resolveRuntimePath defectSymbolsPath
    resolvedElf <- resolveRuntimePath defectElfPath
    pure (name, resolvedBin, resolvedSymbols, resolvedElf)
  defects <- fmap M.fromList . forM defectInputs $ \(name, binPath, defectSymbolsPath, defectElfPath) -> do
    defectLinked <- readRegular (P.carrierBytes P.stage4Carrier) binPath
    defectSymbols <- readRegular 0x10000 defectSymbolsPath
    defectElf <- readRegular (16 * 1024 * 1024) defectElfPath
    defectArtifact <- orFail ("defect " ++ name) (deriveFor shape ProductionArtifact classified chosenCarrier (ArtifactInput defectLinked defectSymbols defectElf))
    defectImage <- case mode of
      NativeOnly -> do
        view <- orFail ("defect " ++ name ++ " native view") (nativeImage4View source defectArtifact)
        orFail ("defect " ++ name ++ " native readback") (validateNativeImage4View view (nativeViewBytes view))
        pure (nativeViewBytes view)
      ActualStagedCandidate _ _ -> orFail ("defect " ++ name ++ " staging") (stageFor shape source defectArtifact)
    ensure (defectImage /= candidate) ("defect " ++ name ++ " stages the candidate image4")
    let spans = ownedFunctions [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions defectArtifact]
    pure (DefectName name, (defectImage, spans, sha256Hex (sha256 defectLinked),artifactDefaultWindows defectArtifact))
  inputs <- forM
    ( [("image4", image4Path), ("linked", linkedPath), ("symbols", symbolsPath), ("elf", elfPath)]
        ++ (case evidence of NativeEvidence _ -> []; StagedEvidence candidatePath contractPath _ -> [("candidate_image4", candidatePath), ("contract", contractPath)])
        ++ zip ["dat", "loader_a", "loader_b", "image5", "plain", "image2"] sourcePaths
        ++ concat [[("defect_" ++ name, binPath), ("defect_" ++ name ++ "_symbols", defectSymbolsPath), ("defect_" ++ name ++ "_elf", defectElfPath)] | (name, binPath, defectSymbolsPath, defectElfPath) <- defectInputs]
    ) $ \(name, path) -> do
    digest <- sha256Hex <$> sha256File path
    pure (fromString name .= object ["path" .= path, "sha256" .= digest])
  frozen <- case execution of
    ExecuteNative -> pure Nothing
    RejudgeRunRoot root anchor -> do
      sourceRun <- loadFrozenRun root runner (object inputs) anchor
      scope <- field (frozenResults sourceRun) "qualification_scope"
      ensure (scope == (case mode of NativeOnly -> "native-only"; ActualStagedCandidate _ _ -> "staged-candidate" :: String)) "source qualification scope differs"
      view <- field (frozenResults sourceRun) "native_image4_view"
      ensure (view == (case evidence of NativeEvidence native -> nativeViewIdentityValue native; StagedEvidence _ _ _ -> object [])) "source native image identity differs"
      contract <- field (frozenResults sourceRun) "contract_staged_image4_sha256" :: IO Value
      let currentContract = case evidence of NativeEvidence _ -> Null; StagedEvidence _ _ digest -> String (fromString (sha256Hex digest))
      ensure (contract == currentContract) "source staged contract identity differs"
      pure (Just sourceRun)
  let BatchGate admission measuredPaths checkpoint counter _ = initialGate
      gate = BatchGate admission measuredPaths checkpoint counter frozen
  out <- freshOutput outPath
  let P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) = artifactCarrier artifact
      P.EntryAddresses (P.GuestAddress getterStub) (P.GuestAddress setterEntry) = artifactEntries artifact
      P.GuestAddress tableAddress = artifactTable artifact
      P.GuestAddress slotTableAddress = artifactSlotTable artifact
      P.GuestAddress specialTableAddress = artifactSpecialTable artifact
      ownedArms = [owned | Property _ _ _ (Owned owned) <- classified]
      fx = withConfigVariants ownedArms (fixture image4)
      ownedSpans = [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions artifact]
      candidateFunctions = ownedFunctions ownedSpans
      standing identifier = standingOf properties identifier
      tableWord (PropertyId n) = tableAddress + 4 * (n - 1)
      slotIndexOf identifier = listToMaybe [SlotIndex index | (index, Property other _ _ _) <- zip [0 ..] [p | p@(Property _ _ _ (Owned (OwnedSlotField _))) <- classified], other == identifier]
      specialArms = [(identifier, special) | Property identifier _ _ (Owned owned) <- classified, Just special <- [specialOf owned]]
      specialOf owned = case owned of
        OwnedField (FieldArm _ _ _ (FieldDefaultOnly _) _ NoHook) -> Nothing
        OwnedField arm -> Just (HookedField arm)
        OwnedReadOnlyField arm -> Just (ReadOnlyField arm)
        OwnedWriteOnlyPair arm -> Just (WriteOnlyPair arm)
        _ -> Nothing
      specialAddressOf identifier = listToMaybe [specialTableAddress + 8 * index | (index, (other, _)) <- zip [0 ..] specialArms, other == identifier]
      specialPatches name identifier before after = do
        address <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no SPECIALS entry")) Right (specialAddressOf identifier)
        EncodedSpecial before0 before1 <- either (Left . ((name ++ ": ") ++) . show) Right (encodeSpecial before)
        EncodedSpecial after0 after1 <- either (Left . ((name ++ ": ") ++) . show) Right (encodeSpecial after)
        Right ([Patch address before0 after0 | before0 /= after0] ++ [Patch (address + 4) before1 after1 | before1 /= after1])
      slotPatch name identifier before after = do
        SlotIndex slotIndex <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
        let EncodedSlot wordBefore = encodeSlot before
            EncodedSlot wordAfter = encodeSlot after
        Right [Patch (slotTableAddress + 4 * slotIndex) wordBefore wordAfter | wordBefore /= wordAfter]
      allVariants = CanonicalFixture : configVariantsOf
      acrossVariants identifier chosen = [RunCase variant (OracleCase identifier o) | variant <- allVariants, o <- chosen]
      armsWhere predicate = [(identifier, owned) | Property identifier _ _ (Owned owned) <- classified, predicate owned]
      stockDefaulted owned = isJust (stockOf owned)
      configDefaulted owned = case owned of
        OwnedField (FieldArm _ _ _ (FieldConfig _ _) _ _) -> True
        _ -> False
      gated owned = case owned of
        OwnedField (FieldArm _ _ _ _ _ (ConfigGatedResave _)) -> True
        _ -> False
      defaultRequests bank = [o | o@(Set b _ SetDefaultRequest) <- operations, maybe True (== b) bank]
      defectControl name defect cases note = case cases of
        [] -> Left (name ++ ": no property has the class the defect changes")
        _ -> Right (Control name MustFail (DefectBuild (DefectName defect)) (nubOrd cases) note)
      stockSwapControl = case exposingStockSwap fx classified of
        Nothing -> Left "c-stock-swapped: no stock default exposes a swap"
        Just (Property identifier _ _ _, owned, swapped) -> do
          patches <- case (owned, swapped) of
            (OwnedField before, OwnedField after) -> specialPatches "c-stock-swapped" identifier (HookedField before) (HookedField after)
            (OwnedSlotField before, OwnedSlotField after) -> slotPatch "c-stock-swapped" identifier before after
            _ -> Left "c-stock-swapped: the swap changes the class"
          Right (Control "c-stock-swapped" MustFail (PatchedCandidate patches) (acrossVariants identifier defaultOperations)
            ("stock default of " ++ propertyHex identifier ++ " " ++ maybe "" stockLabel (stockOf owned) ++ " -> " ++ maybe "" stockLabel (stockOf swapped)))
      selectorControl = case [(identifier, arm) | (identifier, OwnedSlotField arm@(SlotArm _ _ _ (SlotStock (Stock61d65c Profile2)) _)) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          patches <- slotPatch "c-selector-3" identifier arm arm {slotDefault = SlotStock (Stock61d65c Profile3)}
          Right (Control "c-selector-3" MustFail (PatchedCandidate patches) (acrossVariants identifier defaultOperations) ("0x61d65c selector 2 -> 3 for " ++ propertyHex identifier))
        [] -> Left "c-selector-3: no slot field calls 0x61d65c with selector 2"
      gateByteControl = case [(identifier, arm, gateOffset) | (identifier, OwnedField arm@(FieldArm _ _ _ _ _ (ConfigGatedResave (ConfigOffset gateOffset)))) <- armsWhere gated] of
        (identifier, arm, gateOffset) : _ -> do
          patches <- specialPatches "c-gate-byte" identifier (HookedField arm) (HookedField arm {fieldHook = ConfigGatedResave (ConfigOffset (gateOffset + 1))})
          Right (Control "c-gate-byte" MustFail (PatchedCandidate patches) (acrossVariants identifier [o | o@(Set {}) <- operations])
            ("gate byte " ++ hex (configBase + fromIntegral gateOffset) ++ " -> " ++ hex (configBase + fromIntegral gateOffset + 1) ++ " for " ++ propertyHex identifier))
        [] -> Left "c-gate-byte: no gated field"
      mirrorControl name chosen note = case [(identifier, arm) | (identifier, OwnedSlotField arm@(SlotArm _ _ _ _ MirrorRecord0)) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          index <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
          let EncodedEntry before = encodeEntry (SlotEntry (bankArm arm) MirrorRecord0 index)
              EncodedEntry after = encodeEntry (SlotEntry (bankArm arm) NoMirror index)
          Right (Control name MustFail (PatchedCandidate [Patch (tableWord identifier) before after]) [canonical (OracleCase identifier o) | o <- chosen] (note ++ " for " ++ propertyHex identifier))
        [] -> Left (name ++ ": no record-0 mirrored slot field")
      readOnlyControl name change chosen note = case [(identifier, arm) | (identifier, OwnedReadOnlyField arm) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          patches <- specialPatches name identifier (ReadOnlyField arm) (change arm)
          Right (Control name MustFail (PatchedCandidate patches) [canonical (OracleCase identifier o) | o <- chosen] (note ++ " for " ++ propertyHex identifier))
        [] -> Left (name ++ ": no read-only field")
      bumpedDefault arm = case fieldSource arm of
        FieldDefaultOnly (DefaultConstant value) -> ReadOnlyField arm {fieldSource = FieldDefaultOnly (DefaultConstant (value + 1))}
        _ -> ReadOnlyField arm
      candidateWord address = readWordAt candidate (fromIntegral (address - image4Base))
      bracketedIds chosen = [identifier | (identifier, _) <- armsWhere (\owned -> bracketedOwned owned && chosen owned)]
      pairArm owned = case owned of
        OwnedWriteOnlyPair _ -> True
        _ -> False
      setsOn bank identifiers = [canonical (OracleCase identifier o) | identifier <- identifiers, o@(Set b _ _) <- operations, b == bank]
      readBeforeOpen name chosen = case exposingOpenAfterSet fx chosen classified of
        Just (Property identifier _ _ _) -> defectControl name "storage_read_before_open" (setsOn Bank0 [identifier])
          ("defect build: reads bank 0 before opening, for " ++ propertyHex identifier)
        Nothing -> Left (name ++ ": no bracketed property exposes the open order")
      pairSecondControl = case [(identifier, arm) | (identifier, OwnedWriteOnlyPair arm) <- armsWhere (const True)] of
        (identifier, arm@(PairArm _ (BankOffset second) _ _)) : _ -> do
          patches <- specialPatches "c-pair-second-byte" identifier (WriteOnlyPair arm) (WriteOnlyPair arm {pairSecond = BankOffset (second + 1)})
          Right (Control "c-pair-second-byte" MustFail (PatchedCandidate patches) [canonical (OracleCase identifier o) | o@(Set {}) <- operations]
            ("SPECIALS second byte " ++ hex (fromIntegral second) ++ " -> " ++ hex (fromIntegral second + 1) ++ " for " ++ propertyHex identifier))
        [] -> Left "c-pair-second-byte: no write-only pair"
      bracketControls =
        [ defectControl "c-open-removed" "storage_open_removed" (setsOn Bank0 (bracketedIds (const True)))
            "defect build: bank-0 header open omitted"
        , defectControl "c-close-before-set" "storage_close_before_set" (setsOn Bank0 (bracketedIds (const True)))
            "defect build: header closed before the field store"
        , defectControl "c-bracket-bank1" "storage_bracket_bank1" (setsOn Bank1 (bracketedIds (const True)))
            "defect build: bank-1 stores open a header session"
        , pairSecondControl
        , readBeforeOpen "c-read-before-open-field" (not . pairArm)
        , readBeforeOpen "c-read-before-open-pair" pairArm
        , defectControl "c-pair-before-close" "storage_pair_before_close" (setsOn Bank0 [identifier | (identifier, _) <- armsWhere pairArm])
            "defect build: second pair store precedes header close"
        ]
      stage3Controls =
        [ stockSwapControl
        , selectorControl
        , defectControl "c-drop-provisional" "drop_provisional" [canonical (OracleCase identifier o) | (identifier, _) <- armsWhere stockDefaulted, o <- defaultRequests Nothing]
            "defect build: a stock default request skips the provisional save and store"
        , defectControl "c-drop-second-save" "drop_second_save" [canonical (OracleCase identifier o) | (identifier, _) <- armsWhere stockDefaulted, o <- defaultRequests (Just Bank0)]
            "defect build: a stock default request stores the leaf's result without the second save"
        , defectControl "c-config-masked" "mask_config_default" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere configDefaulted, variant <- allVariants, o@(Get _ DefaultRead _) <- operations]
            "defect build: the getter's configuration default keeps only the field's bits"
        , defectControl "c-config-save" "config_save" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere configDefaulted, variant <- allVariants, o <- defaultRequests (Just Bank0)]
            "defect build: the configuration default's store saves first"
        , defectControl "c-gate-inverted" "invert_gate" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere gated, variant <- allVariants, o@(Set {}) <- operations]
            "defect build: the gated resave runs when the gate byte is not 0"
        , gateByteControl
        , mirrorControl "c-mirror-getter" [o | o@(Get {}) <- operations] "PROPERTIES record-0 mirror bit cleared; the getter reads the requested slot"
        , mirrorControl "c-mirror-write" [o | o@(Set {}) <- operations] "PROPERTIES record-0 mirror bit cleared; the set omits the record-0 write"
        , readOnlyControl "c-read-only-stores" (\arm -> HookedField arm) [o | o@(Set {}) <- operations] "SPECIALS subkind read-only -> hooked field; the set stores the field"
        , readOnlyControl "c-read-only-default" bumpedDefault [o | o@(Get _ DefaultRead _) <- operations] "SPECIALS default + 1"
        ]
          ++ bracketControls
      slotControl name control = case exposingSlot fx control classified of
        Nothing -> Left (name ++ ": no slot field exposes the mutation")
        Just (Property identifier _ _ _, arm, mutation) -> do
          index@(SlotIndex slotIndex) <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
          let cases = [canonical (OracleCase identifier o) | o <- slotControlOperations control]
              changed before after address = [Patch address before after | before /= after]
          case (mutation, control) of
            (MutatedArm mutated, _) -> do
              let EncodedEntry entryBefore = encodeEntry (SlotEntry (bankArm arm) (slotMirror arm) index)
                  EncodedEntry entryAfter = encodeEntry (SlotEntry (bankArm mutated) (slotMirror mutated) index)
                  EncodedSlot slotBefore = encodeSlot arm
                  EncodedSlot slotAfter = encodeSlot mutated
              Right (Control name MustFail (PatchedCandidate (changed entryBefore entryAfter (tableWord identifier) ++ changed slotBefore slotAfter (slotTableAddress + 4 * slotIndex))) cases ("PROPERTIES and SLOTS words of " ++ propertyHex identifier))
            (MutatedLayout _, RecordSavedAtBankOffset) -> defectControl name "storage_record_save_at_bank" cases "defect build: previous record stored at bank offset"
            (MutatedLayout _, SlotDefaultsMoved) -> defectControl name "storage_slot_defaults_moved" cases "defect build: slot defaults base moved"
            (MutatedLayout _, TailDefaultsMoved) -> defectControl name "storage_tail_defaults_moved" cases "defect build: tail defaults base moved"
            (MutatedLayout _, _) -> Left (name ++ ": the control changes no storage layout")
      slotControls =
        [ slotControl "c-record-offset" RecordOffsetMoved
        , slotControl "c-branches-swapped" BranchesSwapped
        , slotControl "c-slot1-default-record0" Slot1DefaultFromRecord0
        , slotControl "c-record-save-at-bank" RecordSavedAtBankOffset
        , slotControl "c-slot-defaults-base" SlotDefaultsMoved
        , slotControl "c-tail-base" TailDefaultsMoved
        ]
      entryControl name ec = case exposingField fx ec classified of
        Nothing -> Left (name ++ ": no owned field exposes the mutation")
        Just (Property identifier _ _ (Owned (OwnedField arm)), mutated) ->
          let EncodedEntry before = encodeEntry (FieldEntry arm)
              EncodedEntry after = encodeEntry (FieldEntry mutated)
           in Right (Control name MustFail (PatchedCandidate [Patch (tableWord identifier) before after]) [canonical (OracleCase identifier o) | o <- entryControlOperations ec] ("PROPERTIES entry of " ++ propertyHex identifier))
        Just (Property identifier _ _ _, _) -> Left (name ++ ": " ++ propertyHex identifier ++ " is not an owned field")
      halfTail = listToMaybe [p | p@(Property _ _ _ (Owned (OwnedField (FieldArm _ _ _ (FieldDefaultOnly (DefaultTail _ Half)) _ _)))) <- classified]
      getterPopControl = Right (Control "getter-adapter-wrong-target" MustFail
        (PatchedCandidate [Patch 0x61e110 (candidateWord 0x61e110) (branchWord 0x61e110 setterEntry)])
        [canonical (OracleCase (PropertyId 1) (Get Bank0 LiveRead (SlotArgument 0)))] "getter adapter branches to setter")
      tailHalfControl name defect note = case halfTail of
        Nothing -> Left (name ++ ": no field has a halfword tail default")
        Just (Property identifier _ _ _) -> defectControl name defect
          [RunCase variant (OracleCase identifier o) | variant <- [CanonicalFixture, TailWindow], o <- defaultOperations]
          (note ++ " for " ++ propertyHex identifier)
      tailHalfBase = tailHalfControl "c-tail16-base" "storage_tail16_moved" "defect build: halfword tail defaults base moved by 4"
      tailHalfByte = tailHalfControl "c-tail16-byte" "storage_tail16_byte" "defect build: halfword tail default read as a byte"
      stage4Defects =
        [ defectControl name defect [canonical (entryOperation c) | c <- entryCases, relevant c] ("stage4 " ++ name)
        | (name, defect, relevant) <-
            [ ("index-without-subtract", "index_without_subtract", \c -> oracleProperty (entryOperation c) `elem` map PropertyId [1, 645])
            , ("signed-bound", "signed_bound", \c -> oracleProperty (entryOperation c) `elem` map PropertyId [0, 0x80000000, 0xffffffff])
            , ("upper-bound-plus-one", "upper_bound_plus_one", \c -> oracleProperty (entryOperation c) == PropertyId 646)
            ]
        ]
      patched name patches cases = Right (Control name MustFail (PatchedCandidate patches) (map canonical cases) name)
      stage4Patches =
        [ patched "setter-adapter-wrong-target" [Patch 0x624694 (candidateWord 0x624694) (branchWord 0x624694 getterStub)]
            [OracleCase (PropertyId 1) (Set Bank0 (SlotArgument 0) SetOne)]
        , defectControl "bank-slot-swap" "storage_bank_slot_swap"
            [canonical (OracleCase (PropertyId 0xe9) (Set Bank1 (SlotArgument 2) SetOne))]
            "defect build: bank and slot arguments exchanged"
        ]
      linkedRefusal name patches =
        let local = [Patch (image4Base + address - carrierStart) before after | Patch address before after <- patches]
        in (name, case applyPatches local linked of
          Left _ -> False
          Right mutated -> isLeft (deriveFor shape ProductionArtifact classified chosenCarrier (ArtifactInput mutated symbols elf)))
      imageRefusal name patches = (name, case applyPatches patches candidate of
        Left _ -> False
        Right mutated -> case evidence of
          NativeEvidence view -> isLeft (validateNativeImage4View view mutated)
          StagedEvidence _ _ _ -> isLeft (case shape of
            CanonicalEntry -> validateStagedImage4 (sourceHull source) artifact image4 mutated
            PublicWrapperEntry -> validateLocalImage4 artifact image4 mutated))
      at address after = Patch address (candidateWord address) after
      admissionControls :: [(String, Bool)]
      admissionControls =
        [ imageRefusal "adapter-bl-not-b" [at 0x61e110 (branchLinkWord 0x61e110 getterStub)]
        , linkedRefusal "setter-public-push" [at setterEntry movR0R0Word]
        , linkedRefusal "getter-public-push" [at getterStub 0xe12fff1e]
        , linkedRefusal "getter-public-retired-call" [at (getterStub + 4) (branchLinkWord (getterStub + 4) 0x624400)]
        , linkedRefusal "setter-public-retired-call" [at (setterEntry + 4) (branchLinkWord (setterEntry + 4) 0x630200)]
        , imageRefusal "restore-table-dispatch" [at 0x61e110 (readWordAt image4 (fromIntegral (0x61e110 - image4Base)))]
        , linkedRefusal "retired-arm-branch" [at getterStub (branchWord getterStub 0x61e184)]
        , imageRefusal "overwrite-retained-wrapper" [at 0x6245f8 movR0R0Word]
        ]
      controls =
        [ entryControl "c-offset" OffsetMoved
        , entryControl "c-save" SaveCleared
        , entryControl "c-default" DefaultFlipped
        , entryControl "c-tail-offset" TailOffsetMoved
        , getterPopControl
        , tailHalfBase
        , tailHalfByte
        ]
          ++ slotControls
          ++ stage3Controls
          ++ stage4Defects ++ stage4Patches
      smokeRun = "--smoke" `elem` arguments
      smokeStore = canonical (OracleCase (PropertyId 1) (Set Bank0 (SlotArgument 0xffffffff) SetZero))
      smokeControls =
        [Right (Control name MustFail (DefectBuild (DefectName defect)) [smokeStore] "native public-entry smoke bracket defect")
        | (name, defect) <- [("c-open-removed", "storage_open_removed"), ("c-close-before-set", "storage_close_before_set")]]
      selectedControls = if smokeRun then smokeControls else controls
      planned = [control | Right control <- selectedControls]
      unavailable = [problem | Left problem <- selectedControls]
      inSubset identifier = maybe True (S.member identifier) subset
      variantCases =
        [RunCase TailWindow (OracleCase identifier o) | Just (Property identifier _ _ _) <- [halfTail], inSubset identifier, o <- defaultOperations]
          ++ [RunCase variant (OracleCase identifier o) | Property identifier _ _ (Owned owned) <- classified, inSubset identifier, variant <- configVariantsOf, o <- variantOperations owned]
      gridCases = if smokeRun
        then [canonical (OracleCase (PropertyId identifier) operation) | identifier <- [0, 646], operation <- [Get Bank0 LiveRead (SlotArgument 0), Set Bank0 (SlotArgument 0xffffffff) SetOne]]
          ++ [canonical (OracleCase (PropertyId 1) operation) | operation <- [Get Bank0 LiveRead (SlotArgument 0), Set Bank0 (SlotArgument 0xffffffff) SetZero, Set Bank1 (SlotArgument 0xffffffff) SetZero]]
        else [canonical c | c <- caseGrid, inSubset (oracleProperty c)] ++ variantCases
      runCases = nubOrd (gridCases ++ concatMap controlCases planned)
      -- Every grid case already enters through the adapters; call-context cases run for ids 1 and 645, the
      -- lowest id of each class, ids 0 and 646, and, with 646, the raw ids above it.
      entryRepresentatives = S.fromList (PropertyId 1 : PropertyId 645 : M.elems (M.fromListWith min [(classLabel (standing (PropertyId n)), PropertyId n) | n <- [1 .. 645]]))
      allAdapterCases = [c | c <- entryCases, let identifier@(PropertyId raw) = oracleProperty (entryOperation c), if raw > 646 then inSubset (PropertyId 646) else inSubset identifier && (raw == 0 || raw == 646 || S.member identifier entryRepresentatives)]
      adapterCases = if smokeRun
        then take 1 [c | c <- entryCases, oracleProperty (entryOperation c) == PropertyId 1, Get {} <- [oracleOperation (entryOperation c)]]
          ++ take 1 [c | c <- entryCases, oracleProperty (entryOperation c) == PropertyId 1, entrySlot c == SlotArgument 0xffffffff, Set {} <- [oracleOperation (entryOperation c)]]
        else allAdapterCases
      fxOf rc = variantFixture fx (runVariant rc)
      entryControlCases name = (if smokeRun then take 1 else id) [c | c <- entryCases, entryCallMode c == DirectCall, not (entryReturnVariant c),
        oracleProperty (entryOperation c) == PropertyId (if name `elem` ["id-from-r12", "clobber-r4-or-r11"] then 1 else 3),
        not smokeRun || case name of
          "nonzero-means-default" -> entrySelector c == 2
          "sentinel-plus-one" -> oracleOperation (entryOperation c) == Set Bank0 (SlotArgument 0) (SetRaw 0x1b000000)
          _ -> entrySelector c == 0 && case oracleOperation (entryOperation c) of Get Bank1 LiveRead _ -> True; _ -> False]
  when smokeRun $ do
    ensure (length runCases == 7 && length adapterCases == 2) "bounded smoke case plan differs"
    ensure (maybe False bracketedOwned (modelled (standing (PropertyId 1)))) "bounded smoke requires bracketed property 1"
    putStrLn "bounded native-only smoke: 28 observations in 8 chunks; not full qualification; all raw evidence retained"

  when (research == CompactMeasurement) $ do
    native <- case evidence of
      NativeEvidence view -> pure view
      StagedEvidence {} -> failH "compact measurement requires a native-only view"
    let boundaryOperations = [Get Bank0 LiveRead (SlotArgument 0), Get Bank1 DefaultRead (SlotArgument 0xffffffff),
          Set Bank0 (SlotArgument 0xffffffff) SetDefaultRequest, Set Bank1 (SlotArgument 3) SetAboveDefaultRequest]
        boundarySamples = [OracleCase (PropertyId identifier) operation | identifier <- [0, 646, 0x80000000, 0xffffffff], operation <- boundaryOperations]
        publicOperations = [Get Bank0 LiveRead (SlotArgument 0), Set Bank0 (SlotArgument 0xffffffff) SetZero,
          Get Bank1 DefaultRead (SlotArgument 2), Set Bank1 (SlotArgument 3) SetDefaultRequest,
          Get Bank0 LiveReadTwo (SlotArgument 1), Set Bank0 (SlotArgument 0) SetAboveDefaultRequest]
        familySamples = take 28 [OracleCase identifier operation | (identifier, operation) <- zip (cycle (PropertyId 1 : S.toList entryRepresentatives)) (cycle publicOperations)]
        bracketSamples = [OracleCase (PropertyId 1) operation | bank <- [Bank0, Bank1],
          operation <- [Set bank (SlotArgument 0xffffffff) SetZero, Set bank (SlotArgument 0) SetDefaultRequest,
            Set bank (SlotArgument 3) SetOne, Get bank DefaultRead (SlotArgument 0xffffffff)]]
        callerSamples =
          [EntryCase (OracleCase (PropertyId 1) operation) call selector bank slot alternate
          | (call, alternate) <- [(DirectCall, False), (RegisterCall, False), (TailCall, True), (DirectCall, True)]
          , (selector, bank, slot, operation) <-
              [(2, 1, SlotArgument 0, Get Bank1 LiveRead (SlotArgument 0)),
               (0, 0, SlotArgument 0xffffffff, Set Bank0 (SlotArgument 0xffffffff) SetZero)]]
        wrapperSamples = [(wrapper, OracleCase (PropertyId identifier) (Get Bank0 LiveRead (SlotArgument 0)))
          | wrapper <- [DefaultWrapper, LiveWrapper], identifier <- [0, 646]]
        expectation operation spec = case modelled (standing (oracleProperty operation)) of
          Just owned -> maybe (failH "compact model reaches outside the fixture") pure (modelExpectation fx owned (oracleOperation operation))
          Nothing -> case standing (oracleProperty operation) of
            OutOfRange -> pure (noEffectExpectation (oracleOperation operation) spec)
            Classified {} -> failH "compact measurement cannot omit an independent property model"
        plan family reference owned expected = Compact.PairPlan
          { Compact.planStock = reference, Compact.planOwned = owned, Compact.planModel = Just expected,
            Compact.planRetention = Compact.CompactOnly, Compact.planFamily = Compact.FamilyName family,
            Compact.planRequirement = Compact.MustAgree }
    publicPlans <- forM (boundarySamples ++ familySamples ++ bracketSamples) $ \operation -> do
      let label = caseLabel operation
          reference = withDerivedFrame stockSupplied [] (stockSpec fx image4 properties label stockFunctions operation)
          owned run = withDerivedFrame ownedSupplied ownedRanges (caseSpec (fixtureImage (artifactDefaultWindows artifact) fx candidate) candidateFunctions fx label (standing (oracleProperty operation)) operation (observedExpectation run))
          family = classLabel (standing (oracleProperty operation)) ++ case oracleOperation operation of Get {} -> "-getter"; Set {} -> "-setter"
      expected <- expectation operation reference
      pure (plan family reference owned expected)
    callerPlans <- forM callerSamples $ \entry -> do
      let operation = entryOperation entry
          label = entryLabel entry
      expected <- expectation operation (stockSpec fx image4 properties label stockFunctions operation)
      let reference = withDerivedFrame stockSupplied [] (entryCaseSpec image4 stockFunctions fx label (standing (oracleProperty operation)) entry expected)
          owned run = withDerivedFrame ownedSupplied ownedRanges (entryCaseSpec candidate candidateFunctions fx label (standing (oracleProperty operation)) entry (observedExpectation run))
      pure (plan ("caller-" ++ show (fromEnum (entryCallMode entry))) reference owned expected)
    wrapperPlans <- forM wrapperSamples $ \(wrapper, operation) -> do
      let label = prefixedCaseLabel (CaseLabel ("wrapper-" ++ show (fromEnum wrapper))) (caseLabel operation)
          reference = withDerivedFrame stockSupplied [] (wrapperCaseSpec wrapper image4 stockFunctions fx label (standing (oracleProperty operation)) operation)
          owned _ = withDerivedFrame ownedSupplied ownedRanges (wrapperCaseSpec wrapper candidate candidateFunctions fx label (standing (oracleProperty operation)) operation)
      pure (plan ("wrapper-" ++ show (fromEnum wrapper)) reference owned (noEffectExpectation (oracleOperation operation) reference))
    let plans = publicPlans ++ callerPlans ++ wrapperPlans
    ensure (length plans == 64) "compact measurement requires exactly 128 fresh role observations"
    executable <- getExecutablePath >>= canonicalizePath
    executableHash <- sha256File executable
    sourceIdentities <- forM ["OracleMain.hs", "Replacement/PropertyService/Compact.hs", "Replacement/PropertyService/CompactReceipt.hs",
        "Replacement/PropertyService/Frame.hs", "Replacement/PropertyService/Oracle.hs", "Replacement/PropertyService/Qualification.hs"] $ \name -> do
      path <- resolveRuntimePath ("replacement/property_service/" ++ name)
      digest <- sha256File path
      pure (path, digest)
    let sourceIdentity = object ["executable" .= object ["path" .= executable, "sha256" .= sha256Hex executableHash],
          "source_files" .= [object ["path" .= path, "sha256" .= sha256Hex digest] | (path, digest) <- sourceIdentities]]
        inputIdentity = object inputs
    actualRunnerHash <- sha256File runner
    let StorageAdmission evidenceRoot _ = admission
    (geometryExit, geometryText, geometryError) <- readProcessWithExitCode "stat" ["-f", "-c", "%T %S %s", "--", evidenceRoot] ""
    ensure (geometryExit == ExitSuccess) ("compact filesystem measurement failed: " ++ geometryError)
    allocationGeometry <- either failH pure (parseCompactAllocationGeometry geometryText)
    storageScope <- compactStorageScope admission out
    let config = Compact.CompactConfig
          { Compact.compactOutput = out </> "compact-native", Compact.compactRunner = runner,
            Compact.compactContract = Compact.CompactContract (sha256 (encodeCanonical sourceIdentity)) (sha256 (encodeCanonical inputIdentity)) actualRunnerHash,
            Compact.compactChunkPairs = 4, Compact.compactCheck = checkCompactStorage storageScope,
            Compact.compactAllocationGeometry = allocationGeometry }
        fullPlanCounts =
          [("public_grid_and_variants", 2 * length runCases),
           ("caller_private_abi", 2 * length allAdapterCases),
           ("retained_wrappers", 2 * length propertyIds * length ([minBound .. maxBound] :: [Wrapper])),
           ("guest_controls", sum (map (length . controlCases) planned)),
           ("entry_controls", sum [length (entryControlCases name) | name <- (["nonzero-means-default", "sentinel-plus-one", "id-from-r12", "clobber-r4-or-r11"] :: [String])])]
        fullPlan = object ["observations" .= sum (map snd fullPlanCounts),
          "families" .= object [fromString name .= count | (name, count) <- fullPlanCounts],
          "ids" .= length propertyIds, "operations_per_id" .= length operations, "execution_granted" .= False]
        frameOutcome result = either (\problem -> ["refused" .= show problem]) (\depth -> ["depth_bytes" .= depth]) result
        privateFrames = object
          [ "owned" .= [object (["symbol" .= name, "entry" .= hex address] ++ frameOutcome result) | (name, address, result) <- ownedEntries]
          , "stock" .=
              [ object (["case" .= caseId spec, "entry" .= hex address] ++ frameOutcome outcome)
              | spec <- map Compact.planStock plans
              , let address = fromIntegral (N.guestAddress (caseEntry spec)) :: Word32
              , let outcome = case caseFrameDepth [] stockSupplied spec of
                      Left refusal -> Left (show refusal)
                      Right (FrameDepth depth) -> Right depth ]
          ]
        manifest = encodeCanonical (object ["schema_version" .= (1 :: Int), "source" .= sourceIdentity, "inputs" .= inputIdentity,
          "runner" .= object ["path" .= runner, "sha256" .= sha256Hex actualRunnerHash], "native_image4_view" .= nativeViewIdentityValue native,
          "coverage" .= ("128-observation-measurement-not-full-qualification" :: String), "dat_retirement_admitted" .= False,
          "private_frames" .= privateFrames, "full_required_plan" .= fullPlan])
    Compact.publishCompactEvidence config (out </> "measurement-inputs.json") manifest
    result <- Compact.executeCompactPairs config plans
    ensure (Compact.compactObservations (Compact.compactStats result) == 128) "compact measurement did not execute its complete 128 observations"
    forM_ sourceIdentities $ \(path, digest) -> do
      currentHash <- sha256File path
      ensure (currentHash == digest) "compact source changed during execution"
    inputRows <- field (object ["inputs" .= inputIdentity]) "inputs" :: IO (M.Map String Value)
    forM_ (M.elems inputRows) $ \identity -> do
      path <- field identity "path"
      expectedHash <- field identity "sha256"
      currentHash <- sha256Hex <$> sha256File path
      ensure (currentHash == expectedHash) "compact input changed during execution"
    currentExecutableHash <- sha256File executable
    ensure (currentExecutableHash == executableHash) "compact executable changed during execution"
    finishCompactStorage storageScope
    let output = encodeCanonical (object ["schema_version" .= (1 :: Int), "measurement" .= Compact.compactResultValue result,
          "source" .= sourceIdentity, "inputs" .= inputIdentity, "native_image4_view" .= nativeViewIdentityValue native,
          "coverage" .= ("128-observation-measurement-not-full-qualification" :: String), "dat_retirement_admitted" .= False,
          "measurement_completed" .= True, "public_consumer_agreement" .= Compact.compactConsumerAgreement result,
          "qualification_passed" .= False])
    Compact.publishCompactEvidence config (out </> "measurement-results.json") output
    putStrLn ("compact measurement results: " ++ out </> "measurement-results.json")
    if Compact.compactConsumerAgreement result then exitSuccess else exitFailure

  -- Pass 1: stock.
  (stockBatches, stockRuns) <- runBatches gate cache runner out "stock" [withDerivedFrame stockSupplied [] (stockSpec (fxOf rc) image4 properties (runLabel rc) stockFunctions (runCase rc)) | rc <- runCases]
  let stockRun rc = lookupRun stockRuns (runLabel rc)
  modelRows <- fmap concat . forM runCases $ \rc -> case modelled (standing (runProperty rc)) of
    Nothing -> pure []
    Just owned -> do
      run <- stockRun rc
      pure [(rc, modelDifferences (fxOf rc) owned (oracleOperation (runCase rc)) run)]

  -- Pass 2: candidate against stock.
  candidateSpecs <- forM runCases $ \c -> do
    run <- stockRun c
    pure (withDerivedFrame ownedSupplied ownedRanges (caseSpec (fixtureImage (artifactDefaultWindows artifact) (fxOf c) candidate) candidateFunctions (fxOf c) (runLabel c) (standing (runProperty c)) (runCase c) (observedExpectation run)))
  (candidateBatches, candidateRuns) <- runBatches gate cache runner out "candidate" candidateSpecs
  verdicts <- forM runCases $ \c -> do
    reference <- stockRun c
    subject <- lookupRun candidateRuns (runLabel c)
    pure (c, compareRuns reference subject)
  -- Wrong variants: each applies the mutants tool's one word to the owned image
  -- and runs the same case plan, so a control row names the variant's site.
  variantInputs <- variantArguments arguments
  ensure (not (isJust frozen) || null variantInputs) "--variant cannot rejudge a frozen run"
  (variantBatches, variantOutcomes) <- fmap unzip . forM variantInputs $ \(name, address, mutated) -> do
    ensure (address >= carrierStart && address + 4 <= carrierEnd) ("variant " ++ name ++ " is outside the owned carrier")
    let offset = fromIntegral (address - image4Base)
    ensure (offset + 4 <= B.length candidate) ("variant " ++ name ++ " is outside the owned image")
    let image = B.take offset candidate <> word32le mutated <> B.drop (offset + 4) candidate
    specs <- forM runCases $ \c -> do
      run <- stockRun c
      pure (withDerivedFrame ownedSupplied ownedRanges (caseSpec (fixtureImage (artifactDefaultWindows artifact) (fxOf c) image) candidateFunctions (fxOf c) (runLabel c) (standing (runProperty c)) (runCase c) (observedExpectation run)))
    (batches, runs) <- runBatches gate cache runner out ("variant-" ++ name) specs
    outcomes <- forM runCases $ \c -> do
      reference <- stockRun c
      subject <- lookupRun runs (runLabel c)
      pure (runLabel c, compareRuns reference subject)
    pure (batches, (name, outcomes))
  let entryStanding c = standing (oracleProperty (entryOperation c))
      entryExpected c = fromMaybe placeholderExpectation (modelled (entryStanding c) >>= \owned -> modelExpectation fx owned (oracleOperation (entryOperation c)))
  (entryStockBatches, entryStockRuns) <- runBatches gate cache runner out "entry-stock"
    [withDerivedFrame stockSupplied [] (entryCaseSpec image4 stockFunctions fx (entryLabel c) (entryStanding c) c (entryExpected c)) | c <- adapterCases]
  (entryCandidateBatches, entryCandidateRuns) <- runBatches gate cache runner out "entry-candidate"
    [withDerivedFrame ownedSupplied ownedRanges (entryCaseSpec candidate candidateFunctions fx (entryLabel c) (entryStanding c) c (entryExpected c)) | c <- adapterCases]
  entryVerdicts <- forM adapterCases $ \c -> do
    reference <- lookupRun entryStockRuns (entryLabel c)
    subject <- lookupRun entryCandidateRuns (entryLabel c)
    pure (c, compareRuns reference subject ++ case modelled (entryStanding c) of
      Nothing -> []
      Just owned -> modelDifferences fx owned (oracleOperation (entryOperation c)) subject)
  let wrappers = (if smokeRun then take 2 else id) [(wrapper, OracleCase identifier (Get Bank0 LiveRead (SlotArgument 0))) | wrapper <- [minBound .. maxBound], identifier <- propertyIds, inSubset identifier]
      wrapperLabel wrapper c = prefixedCaseLabel (CaseLabel ("wrapper-" ++ show (fromEnum wrapper))) (caseLabel c)
      wrapperSpecs image functions = [wrapperCaseSpec wrapper image functions fx (wrapperLabel wrapper c) (standing (oracleProperty c)) c | (wrapper, c) <- wrappers]
  (wrapperStockBatches, wrapperStockRuns) <- runBatches gate cache runner out "wrapper-stock" (map (withDerivedFrame stockSupplied []) (wrapperSpecs image4 stockFunctions))
  (wrapperCandidateBatches, wrapperCandidateRuns) <- runBatches gate cache runner out "wrapper-candidate" (map (withDerivedFrame ownedSupplied ownedRanges) (wrapperSpecs candidate candidateFunctions))
  wrapperVerdicts <- forM wrappers $ \(wrapper, c) -> do
    let label = wrapperLabel wrapper c
    reference <- lookupRun wrapperStockRuns label
    subject <- lookupRun wrapperCandidateRuns label
    pure (label, compareRuns reference subject)
  entryControlImages <- forM
    [("nonzero-means-default", "nonzero_means_default"), ("sentinel-plus-one", "sentinel_plus_one")] $ \(name, defect) ->
      case M.lookup (DefectName defect) defects of
        Nothing -> failH ("no --defect " ++ defect)
        Just (image, spans, _, windows) -> pure (name, fixtureImage windows fx image, spans)
  veneerImages <- forM
    [("id-from-r12", [at getterStub 0xe1a0000c, at (getterStub + 4) (candidateWord (getterStub + 4) - 0x01000000)])
    ,("clobber-r4-or-r11", [at getterStub 0xe92d4430])] $ \(name, patches) -> do
      image <- orFail name (applyPatches patches candidate)
      pure (name, image, candidateFunctions)
  let controlEntryLabel name c = prefixedCaseLabel (CaseLabel ("ec-" ++ name)) (entryLabel c)
  (entryControlBatches, entryControlRuns) <- runBatches gate cache runner out "entry-controls"
    [entryCaseSpec image spans fx (controlEntryLabel name c) (entryStanding c) c (entryExpected c)
    | (name, image, spans) <- entryControlImages ++ veneerImages, c <- entryControlCases name]
  entryControlOutcomes <- forM (entryControlImages ++ veneerImages) $ \(name, _, _) -> do
    rows <- forM (entryControlCases name) $ \c -> do
      subject <- lookupRun entryControlRuns (controlEntryLabel name c)
      let differences = expectationDifferences (entryExpected c) subject
      pure (subject, differences)
    let failed = length [() | (_, differences) <- rows, not (null differences)]
        statuses = map (runnerStatusLabel . runStatus . fst) rows
        holds = failed > 0 && all ((== Returned) . runStatus . fst) rows
    pure (object ["name" .= name, "holds" .= holds, "failed_cases" .= failed, "statuses" .= statuses], holds)

  -- Guest controls.
  controlImages <- forM planned $ \control -> case controlImage control of
    PatchedCandidate patches -> do
      image <- either (failH . ((controlName control ++ ": ") ++)) pure (applyPatches patches candidate)
      pure (control, image, candidateFunctions,artifactDefaultWindows artifact)
    DefectBuild defect@(DefectName name) -> case M.lookup defect defects of
      Just (image, spans, _, windows) -> pure (control, image, spans,windows)
      Nothing -> failH (controlName control ++ ": no --defect " ++ name)
  controlSpecs <- forM (concat [[(control, image, spans, windows, c) | c <- controlCases control] | (control, image, spans, windows) <- controlImages]) $ \(control, image, spans, windows, c) -> do
    run <- stockRun c
    pure (caseSpec (fixtureImage windows (fxOf c) image) spans (fxOf c) (runPrefixed (controlName control) c) (standing (runProperty c)) (runCase c) (observedExpectation run))
  (controlBatches, controlRuns) <- runBatches gate cache runner out "controls" controlSpecs
  controlOutcomes <- forM controlImages $ \(control, _, _, _) -> do
    failures <- fmap concat . forM (controlCases control) $ \c -> do
      reference <- stockRun c
      subject <- lookupRun controlRuns (runPrefixed (controlName control) c)
      pure [(runPrefixed (controlName control) c, d) | d <- compareRuns reference subject]
    let failedCases = M.size (M.fromListWith (++) [(label, [d]) | (label, d) <- failures])
    pure (control, failedCases, failures)
  let controlsHold = null unavailable && and [controlHolds control failed | (control, failed, _) <- controlOutcomes]
        && and (map snd admissionControls) && and (map snd entryControlOutcomes)
        && all ((== Returned) . runStatus) (M.elems controlRuns)

  -- Per-case stock evidence for the shared judge: the entry each case entered and
  -- the instructions the runner reports inside that entry's extent.
  let caseBatch index = "stock-" ++ (if index < 10 then "0" else "") ++ show index
      caseOrdinals = M.fromList (zip (map runLabel runCases) [0 :: Int ..])
      entryExtents = [(spanStart span', spanEnd span' - spanStart span') | span' <- stockFunctions]
      extentOf entry = fromMaybe 4 (lookup entry entryExtents)
  caseRows <- forM (zip runCases verdicts) $ \(rc, (_, differences)) -> do
    let label = runLabel rc
        spec = stockSpec (fxOf rc) image4 properties label stockFunctions (runCase rc)
        N.GuestAddress entry = caseEntry spec
        entryAddress = fromIntegral entry :: Word32
        extent = extentOf entryAddress
        ordinal = fromMaybe 0 (M.lookup label caseOrdinals)
        path = out </> caseBatch (ordinal `div` batchLimit) </> (caseLabelText label ++ ".native-result.json")
    stockEvidence <- evidenceAt path entryAddress extent
    pure (object
      [ "id" .= caseLabelText label
      , "entry" .= hex entryAddress
      , "extent" .= extent
      , "stock_trace_instructions" .= evidenceTrace stockEvidence
      , "stock_provenance_instructions" .= evidenceProvenance stockEvidence
      , "stock_instructions_inside" .= (evidenceTrace stockEvidence + evidenceProvenance stockEvidence)
      , "stock_instructions" .= evidenceInstructions stockEvidence
      , "stock_status" .= evidenceStatus stockEvidence
      , "differences" .= [differenceKind difference ++ ": " ++ differenceDetail difference | difference <- differences]
      ])

  -- Results.
  runnerSha <- sha256Hex <$> (canonicalizePath runner >>= sha256File)
  let modelFailures = [(c, ds) | (c, ds) <- modelRows, not (null ds)]
      verdictFailures = [(c, ds) | (c, ds) <- verdicts, not (null ds)]
      modelledIds = M.keys (M.fromList [(runProperty c, ()) | (c, _) <- modelRows])
      disagreeingIds = M.keys (M.fromList [(runProperty c, ()) | (c, _) <- modelFailures])
      perId = M.fromListWith (+) [(runProperty c, 1 :: Int) | (c, _) <- verdictFailures]
      modelPerId = M.fromListWith (+) [(runProperty c, 1 :: Int) | (c, _) <- modelFailures]
      classOf identifier = classLabel (standing identifier)
      byClass = M.fromListWith (\(a, b, c) (d, e, f) -> (a + d, b + e, c + f))
        [(classOf (runProperty c), (1 :: Int, if null ds then 1 else 0 :: Int, if null ds then 0 else 1 :: Int)) | (c, ds) <- verdicts]
      runIds = M.keys (M.fromList [(runProperty c, ()) | c <- runCases])
      stockAll = M.elems stockRuns
      candidateAll = M.elems candidateRuns
      passed = null modelFailures && null verdictFailures && all (null . snd) entryVerdicts && all (null . snd) wrapperVerdicts && controlsHold
      allBatches = stockBatches ++ candidateBatches ++ controlBatches ++ concat variantBatches ++ entryStockBatches ++ entryCandidateBatches ++ entryControlBatches ++ wrapperStockBatches ++ wrapperCandidateBatches
      executedBatches = length [() | record <- allBatches, not (isJust frozen), recordReuse record == BatchExecuted]
      reusedBatches = length [() | record <- allBatches, not (isJust frozen), recordReuse record == BatchReused]
      executedCases = sum [recordCases record | record <- allBatches, not (isJust frozen), recordReuse record == BatchExecuted]
      reusedCases = sum [recordCases record | record <- allBatches, not (isJust frozen), recordReuse record == BatchReused]
      batchValue record = object ["name" .= recordName record, "cases" .= recordCases record, "batch_sha256" .= sha256Hex (recordBatchSha record), "summary_sha256" .= sha256Hex (recordSummarySha record), "reuse" .= (if isJust frozen then "offline-rejudged" else reuseLabel (recordReuse record))]
      results = object
        [ "schema_version" .= (1 :: Int)
        , "suite" .= ("property-service" :: String)
        , "image_path" .= image4Path
        , "image_sha256" .= sha256Hex (sha256 image4)
        , "owned_path" .= linkedPath
        , "owned_sha256" .= sha256Hex (sha256 linked)
        , "cases" .= caseRows
        , "inputs" .= object inputs
        , "qualification_scope" .= (case mode of NativeOnly -> "native-only"; ActualStagedCandidate _ _ -> "staged-candidate" :: String)
        , "dat_retirement_admitted" .= False
        , "qualification_status" .= ("current-joined-body-unqualified-by-frozen-cohort" :: String)
        , "native_image4_view" .= (case evidence of NativeEvidence view -> nativeViewIdentityValue view; StagedEvidence _ _ _ -> object [])
        , "contract_staged_image4_sha256" .= (case evidence of NativeEvidence _ -> Nothing; StagedEvidence _ _ digest -> Just (sha256Hex digest))
        , "retired_fetch_guard" .= True
        , "coverage" .= (if smokeRun then "bounded-smoke-not-full-qualification" else "current-source-matrix" :: String)
        , "runner" .= object ["path" .= runner, "sha256" .= runnerSha]
        , "execution" .= object
            [ "cache" .= cache
            , "batches_executed" .= executedBatches
            , "batches_reused" .= reusedBatches
            , "cases_executed" .= executedCases
            , "cases_reused" .= reusedCases
            , "mode" .= (if isJust frozen then "offline-rejudgment" else "native-execution" :: String)
            , "sdk_executions" .= executedBatches
            , "observations_rejudged" .= (if isJust frozen then sum (map recordCases allBatches) else 0)
            , "source" .= fmap (\frozenSource -> object ["root" .= frozenRoot frozenSource, "results_sha256" .= sha256Hex (frozenResultsSha frozenSource), "batches" .= frozenBatches frozenSource]) frozen
            , "consumer_scope" .= ("current-source ordered little-endian byte effects only in approved ordinary RAM spans; physical reports unchanged; no interrupt, concurrent, atomic-width equivalence or DAT retirement" :: String)
            ]
        , "batches" .= map batchValue allBatches
        , "wrappers" .= object ["cases" .= length wrappers, "failed" .= length [() | (_, ds) <- wrapperVerdicts, not (null ds)]]
        , "entry_adapters" .= object ["cases" .= length adapterCases, "failed" .= length [() | (_, ds) <- entryVerdicts, not (null ds)]]
        , "case_shape" .= object
            [ "ids" .= unwords (map propertyHex runIds)
            , "subset" .= isJust subset
            , "fixture_variants" .=
                [ object
                    [ "variant" .= variantTag variant
                    , "note" .= variantNote variant
                    , "cases" .= length [() | rc <- runCases, runVariant rc == variant]
                    ]
                | variant <- [minBound .. maxBound]
                ]
            , "cases_per_id" .= length operations, "grid_cases" .= length gridCases, "cases_per_role" .= length runCases
            , "carrier" .= object ["start" .= hex carrierStart, "end" .= hex carrierEnd]
            , "entries" .= object ["getter" .= hex getterStub, "setter" .= hex setterEntry, "properties" .= hex tableAddress]
            , "candidate_functions" .= [object ["start" .= hex s, "end" .= hex e] | FunctionSpan s e <- candidateFunctions]
            ]
        , "totals" .= object
            [ "classes" .= object [fromString name .= object ["cases" .= n, "pass2_passed" .= p, "pass2_failed" .= f] | (name, (n, p, f)) <- M.toList byClass]
            , "ids" .=
                [ object
                    [ "property" .= propertyHex identifier
                    , "class" .= classOf identifier
                    , "reason" .= case standing identifier of
                        Classified (Delegated reason) -> Just (reasonLabel reason)
                        _ -> Nothing
                    , "model_disagreeing_cases" .= (if isJust (modelled (standing identifier)) then Just (M.findWithDefault 0 identifier modelPerId) else Nothing)
                    , "pass2_failed_cases" .= M.findWithDefault 0 identifier perId
                    ]
                | identifier <- runIds
                ]
            ]
        , "model_agreement" .= object
            [ "modelled_ids" .= length modelledIds, "disagreeing_ids" .= map propertyHex disagreeingIds
            , "modelled_cases" .= length modelRows, "disagreeing_cases" .= length modelFailures
            ]
        , "pass2" .= object ["passed" .= (length verdicts - length verdictFailures), "failed" .= length verdictFailures]
        , "status_distribution" .= object ["stock" .= statusCounts stockAll, "candidate" .= statusCounts candidateAll]
        , "controls" .=
            ( [ object
                  [ "name" .= controlName control
                  , "kind" .= (case controlKind control of MustFail -> "must-fail"; MustPass -> "must-pass" :: String)
                  , "note" .= controlNote control
                  , "image" .= case controlImage control of
                      PatchedCandidate patches -> object ["patches" .= [object ["address" .= hex a, "before" .= hex b, "after" .= hex c] | Patch a b c <- patches]]
                      DefectBuild defect@(DefectName name) -> object ["defect" .= name, "linked_sha256" .= fmap (\(_, _, digest, _) -> digest) (M.lookup defect defects)]
                  , "cases" .= length (controlCases control)
                  , "failed_cases" .= failed
                  , "holds" .= controlHolds control failed
                  , "first_differences" .= [object ["case" .= caseLabelText label, "kind" .= differenceKind d, "detail" .= differenceDetail d] | (label, d) <- take 20 failures]
                  ]
              | (control, failed, failures) <- controlOutcomes
              ]
                ++ [object ["name" .= problem, "holds" .= False] | problem <- unavailable]
                ++ [object ["name" .= name, "kind" .= ("admission-refusal" :: String), "holds" .= holds] | (name, holds) <- admissionControls]
                ++ map fst entryControlOutcomes
                ++ [ object
                       [ "name" .= name
                       , "kind" .= ("wrong-variant" :: String)
                       , "reported" .= not (null differing)
                       , "cases" .= length differing
                       , "failed_cases" .= length failures
                       , "holds" .= not (null differing)
                       , "first_differences" .= [object ["case" .= caseLabelText label, "kind" .= differenceKind d, "detail" .= differenceDetail d] | (label, d) <- take 20 failures]
                       ]
                   | (name, outcomes) <- variantOutcomes
                   , let failures = [(label, d) | (label, ds) <- outcomes, d <- ds]
                         differing = nubOrd [label | (label, _) <- failures]
                   ]
            )
        , "differences" .= differenceSamples
            ( [("model", runLabel c, d) | (c, ds) <- modelFailures, d <- ds]
                ++ [("pass2", runLabel c, d) | (c, ds) <- verdictFailures, d <- ds]
                ++ [("entry", entryLabel c, d) | (c, ds) <- entryVerdicts, d <- ds]
            )
        , "passed" .= passed
        ]
  mapM_ (\frozenSource -> finishFrozenRun frozenSource (map (FrozenBatchName . recordName) allBatches)) frozen
  B.writeFile (out </> "results.json") (encodeCanonical results)
  putStrLn ("model: " ++ show (length modelFailures) ++ " disagreeing of " ++ show (length modelRows) ++ " owned-id stock cases")
  putStrLn ("pass 2: " ++ show (length verdictFailures) ++ " failing of " ++ show (length verdicts))
  forM_ controlOutcomes $ \(control, failed, _) -> putStrLn (controlName control ++ ": " ++ show failed ++ " failing of " ++ show (length (controlCases control)))
  forM_ unavailable putStrLn
  putStrLn ("results: " ++ out </> "results.json")
  unless passed exitFailure

