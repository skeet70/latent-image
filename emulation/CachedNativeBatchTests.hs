{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import qualified Codec.Compression.GZip as GZip
import Control.Exception (IOException, SomeException, bracket_, try)
import Control.Monad (filterM, forM, forM_)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf, isSuffixOf, sort)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import qualified Emulation.NativeEvidence as E
import Emulation.CachedNativeBatch
  ( BatchReuse (..)
  , CacheEscape
  , CacheInspection (..)
  , CacheBinding
  , MaintenanceInterval (..)
  , readCachedPayload
  , cachedObservedBatch
  , cachedObservedBatchWith
  , cachedObservedBatchWithSince
  , cachedObservedBatchWithBinding
  , inspectCachedBatch
  , cacheOnlyRunner
  , cachedObservedBatchOnly
  , ResultPolicy (..)
  )
import StorageRun (openNativeCache, runSpecs, runCachedSpecs, cacheExecutionValue)
import Emulation.ParallelCases (mapNativeBounded, readNativeJobs)
import Emulation.NativeCase
  ( CaseId (..)
  , LoadedCase (..)
  , Region (..)
  , checkResult
  , hexBytes
  , loadCaseWith
  , readArtifact
  )
import qualified Emulation.NativeObservation as N
import qualified Emulation.OwnedCase as O
import Emulation.Sha256 (sha256File)
import FujiAF.Host (ensure, get, jsonRead, list, str, withTemporary, withTemporaryAt)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory
  ( Permissions (..)
  , canonicalizePath
  , copyFile
  , createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , getFileSize
  , getPermissions
  , getTemporaryDirectory
  , listDirectory
  , removeDirectoryRecursive
  , removeFile
  , setPermissions
  )
import System.Environment (lookupEnv, setEnv, unsetEnv)
import System.Posix.Files (createSymbolicLink, getFileStatus, getSymbolicLinkStatus, isRegularFile, isSymbolicLink)
import System.FilePath ((</>), takeFileName)

main :: IO ()
main = do
  previousReserve <- lookupEnv "XM1_STORAGE_MIN_FREE"
  previousBudget <- lookupEnv "XM1_NATIVE_CACHE_MAX"
  let restoreReserve = maybe (unsetEnv "XM1_STORAGE_MIN_FREE") (setEnv "XM1_STORAGE_MIN_FREE") previousReserve
      restoreBudget = maybe (unsetEnv "XM1_NATIVE_CACHE_MAX") (setEnv "XM1_NATIVE_CACHE_MAX") previousBudget
  bracket_
    (setEnv "XM1_STORAGE_MIN_FREE" "1" >> setEnv "XM1_NATIVE_CACHE_MAX" "20G")
    (restoreReserve >> restoreBudget)
    runTests

runTests :: IO ()
runTests = do
  canonicalRunner <- canonicalizePath fujimodNativeCase
  withTemporary "cached-native-batch-tests" $ \root -> do
    reuseTest (root </> "reuse") canonicalRunner
    concurrentReuseTest (root </> "concurrent") canonicalRunner
    qualificationParallelTest (root </> "qualification-parallel") canonicalRunner
    expectedRejudgeTest (root </> "expected") canonicalRunner
    wireChangeTest (root </> "wire") canonicalRunner
    labelChangeTest (root </> "label") canonicalRunner
    runnerIdentityTest (root </> "runner") canonicalRunner
    corruptionTest (root </> "corruption") canonicalRunner
    interruptedTest (root </> "interrupted") canonicalRunner
    faultyTest (root </> "faulty") canonicalRunner
    incompleteOutputTest (root </> "incomplete")
    rootAliasTest (root </> "root-alias") canonicalRunner
    escapeV1Test (root </> "escape-v1") canonicalRunner
    escapeTmpTest (root </> "escape-tmp") canonicalRunner
    escapeKeyTest (root </> "escape-key") canonicalRunner
    escapeCaseTest (root </> "escape-case") canonicalRunner
    retentionTest (root </> "retention") canonicalRunner
    defaultResultPolicyTest (root </> "default-policy") canonicalRunner
    cacheEvictionTest (root </> "eviction") canonicalRunner
    cacheRecencyTest (root </> "recency") canonicalRunner
    assetWireTest (root </> "assets") canonicalRunner
    consumerFrameTest (root </> "consumer-frame") canonicalRunner
    deadPrivateFrameTest (root </> "dead-private-frame") canonicalRunner
    genericFrameTest (root </> "generic-frame") canonicalRunner
    ignoredBoundaryTest (root </> "ignored-boundary")
    inspectionSafetyTest (root </> "inspection-safety") canonicalRunner
    inspectionEligibilityTest (root </> "inspection-eligibility")
    interruptedRunSpecsTest (root </> "interrupted-runspecs") canonicalRunner
    resumeOrdinaryTest (root </> "resume-ordinary") canonicalRunner
    compressedPayloadTest (root </> "compressed") canonicalRunner
    cacheOnlyTest (root </> "cache-only") canonicalRunner
  base <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  withTemporaryAt base "cached-native-batch-storage-budget" $ \root ->
    storageBudgetTransitionTest root canonicalRunner

spec :: String -> [Word32] -> [(N.Register, Word32)] -> [(N.Register, Word32)] -> [(N.RegionId, B.ByteString)] -> O.CaseSpec
spec ident code initial expected memory =
  O.CaseSpec
    { O.caseId = ident
    , O.caseProvenance = "CachedNativeBatchTests.hs: synthetic ARM cache case"
    , O.caseExclusion = "cached batch behavioral test"
    , O.caseEntry = N.GuestAddress 0x8000
    , O.caseExit = N.GuestAddress 0xa000
    , O.caseRegions =
        [ O.mkRegion (N.RegionId "code") (N.GuestAddress 0x8000) (4 * length code) O.ReadExecute (O.assemble code)
        , O.mkRegion (N.RegionId "data") (N.GuestAddress 0x9000) 4 O.ReadWrite (O.word32le 0x11223344)
        , O.mkRegion (N.RegionId "exit") (N.GuestAddress 0xa000) 4 O.ReadExecute (B.replicate 4 0)
        , O.mkRegion (N.RegionId "stack") (N.GuestAddress 0xb000) 32 O.ReadWrite (B.replicate 32 0)
        ]
    , O.caseFunctions = [(N.GuestAddress 0x8000, 4 * fromIntegral (length code))]
    , O.caseInitial =
        [(register, fromMaybe 0 (lookup register initial)) | register <- [N.R0 .. N.R12]]
          ++ [(N.Sp, 0xb020), (N.Lr, 0xa000), (N.Cpsr, 0x10)]
    , O.caseExpectedStatus = N.Returned
    , O.caseExpectedRegisters = expected
    , O.caseExpectedMemory = memory
    , O.caseIgnoredMemory = []
    , O.caseCp14Identity = Nothing
    , O.caseKernel = Nothing
    }

writerSpec :: Word32 -> Word32 -> O.CaseSpec
writerSpec value expected =
  spec
    "writer"
    [0xe5820000, 0xe12fff1e]
    [(N.R0, value), (N.R2, 0x9000)]
    [(N.R0, expected)]
    [(N.RegionId "data", O.word32le value)]

readerSpec :: Word32 -> O.CaseSpec
readerSpec expected =
  spec
    "reader"
    [0xe5921000, 0xe12fff1e]
    [(N.R2, 0x9000)]
    [(N.R1, expected)]
    []

specs :: Word32 -> Word32 -> Word32 -> [O.CaseSpec]
specs writerValue writerExpected readerExpected =
  [writerSpec writerValue writerExpected, readerSpec readerExpected]

caseIds :: [O.CaseSpec] -> [CaseId]
caseIds = map (CaseId . O.caseId)

cacheOnlyTest :: FilePath -> FilePath -> IO ()
cacheOnlyTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batch <- O.writeBatch batchDirectory cases
  (_, baseline) <- cachedObservedBatch cache runner batch
  digest <- sha256File runner
  frozen <- cacheOnlyRunner runner digest
  jobs <- readNativeJobs Nothing
  nativeCache <- openNativeCache jobs (root </> "consumer-cache")
  seededRows <- runSpecs nativeCache runner (root </> "consumer-seed") cases
  ensure (all (null . snd) seededRows) "cache-only consumer seed failed"
  consumerRows <- runCachedSpecs nativeCache frozen (root </> "consumer") cases
  ensure (all (null . snd) consumerRows && map fst consumerRows == map O.caseId cases) "cache-only consumer refused current cases"
  let scratch = (writerSpec 0xdeadbeef 0xdeadbeef) {O.caseId = "scratch-writer", O.caseExpectedMemory = []}
      scratchDirectory = root </> "scratch-consumer"
  scratchRows <- runSpecs nativeCache runner scratchDirectory [scratch]
  ensure (scratchRows == [("scratch-writer",[])]) "scratch-only change failed the consumer"
  duplicate <- doesFileExist (scratchDirectory </> "0" </> "scratch-writer.native-result.json")
  summary <- jsonRead =<< B.readFile (scratchDirectory </> "0" </> "summary.json")
  ensure (not duplicate && get summary "raw_cache" /= Null) "successful consumer retained duplicate raw output or lost its binding"
  _ <- O.writeBatch batchDirectory (specs 0xdeadbeef 0xdeadbeee 0x11223344)
  (reuse, observed, _) <- cachedObservedBatchOnly WriteAllResults cache frozen batch
  ensure (reuse == BatchReused && observed == baseline) "cache-only observation differs"
  checkResultFiles batchDirectory (specs 0xdeadbeef 0xdeadbeee 0x11223344) observed ["failed","passed"]
  let refuse action = do
        result <- try action :: IO (Either SomeException (BatchReuse,[(CaseId,N.NativeObservation)],Maybe CacheBinding))
        case result of
          Left _ -> pure ()
          Right _ -> fail "cache-only refusal executed or accepted an observation"
  missing <- cacheOnlyRunner "/nix/store/00000000000000000000000000000000-missing-runner/bin/native-case" digest
  refuse (cachedObservedBatchOnly WriteAllResults cache missing batch)
  _ <- O.writeBatch batchDirectory (specs 0xcafebabe 0xcafebabe 0x11223344)
  refuse (cachedObservedBatchOnly WriteAllResults cache frozen batch)
  entries <- cacheEntries cache
  ensure (length entries == 1) "cache-only miss published an execution"
  _ <- O.writeBatch batchDirectory cases
  entry <- singleEntry cache
  let payload = entry </> "cases" </> "writer" </> "runner.json.gz"
  B.writeFile payload "truncated"
  refuse (cachedObservedBatchOnly WriteAllResults cache frozen batch)
  retained <- doesDirectoryExist entry
  ensure retained "cache-only corruption rewrote cache history"

compressedPayloadTest :: FilePath -> FilePath -> IO ()
compressedPayloadTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
  batchPath <- O.writeBatch batchDirectory (specs 0xdeadbeef 0xdeadbeef 0x11223344)
  (_, baseline) <- cachedObservedBatch cache runner batchPath
  entry <- singleEntry cache
  let payload = entry </> "cases" </> "writer" </> "runner.json.gz"
      rawPath = entry </> "cases" </> "writer" </> "runner.json"
  original <- B.readFile payload
  let raw = BL.toStrict (GZip.decompress (BL.fromStrict original))
      invalid bytes = do
        B.writeFile payload bytes
        result <- inspectCachedBatch cache runner batchPath
        case result of
          InspectionInvalid _ _ -> pure ()
          _ -> fail "invalid compressed payload accepted"
  invalid (B.take (B.length original - 1) original)
  invalid (B.init original <> B.singleton (if B.last original == 0 then 1 else 0))
  invalid (original <> "trailing")
  invalid (BL.toStrict (GZip.compress (BL.fromStrict (raw <> "overflow"))))
  B.writeFile payload original
  inspection <- inspectCachedBatch cache runner batchPath
  binding <- case inspection of
    InspectionRawHit _ found _ -> pure found
    _ -> fail "compressed payload inspection failed"
  decoded <- readCachedPayload binding (CaseId "writer")
  ensure (decoded == raw) "compressed consumer payload differs"
  (reuse, observed) <- cachedObservedBatch cache runner batchPath
  ensure (reuse == BatchReused && observed == baseline) "compressed payload changed observations"
  forM_ ["writer", "reader"] $ \ident -> do
    let directory = entry </> "cases" </> ident
    encoded <- BL.readFile (directory </> "runner.json.gz")
    B.writeFile (directory </> "runner.json") (BL.toStrict (GZip.decompress encoded))
    removeFile (directory </> "runner.json.gz")
  legacyBefore <- B.readFile rawPath
  (legacy, legacyObserved) <- cachedObservedBatch cache runner batchPath
  ensure (legacy == BatchReused && legacyObserved == baseline) "legacy payload changed observations"
  legacyDecoded <- readCachedPayload binding (CaseId "writer")
  ensure (legacyDecoded == raw) "legacy consumer payload differs"
  legacyAfter <- B.readFile rawPath
  compressedExists <- doesFileExist payload
  ensure (legacyBefore == legacyAfter && not compressedExists) "legacy cache was rewritten"

reuseTest :: FilePath -> FilePath -> IO ()
reuseTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  before <- B.readFile batchPath
  (firstReuse, first) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  after <- B.readFile batchPath
  ensure (firstReuse == BatchExecuted) ("first call did not execute: " ++ show firstReuse)
  ensure (before == after) "cachedObservedBatch modified batch.json"
  ensure (map fst first == caseIds cases) ("observation order differs: " ++ show (map (caseIdText . fst) first))
  ensure (all ((== N.Returned) . N.nativeStatus . snd) first) ("unexpected status: " ++ show first)
  forM_ (zip cases first) $ \(caseSpec, (_, observation)) ->
    checkResultFile batchDirectory caseSpec observation "passed"
  (secondReuse, second) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (secondReuse == BatchReused) ("second call did not reuse: " ++ show secondReuse)
  ensure (second == first) "cached observation differs from the executed observation"

expectedRejudgeTest :: FilePath -> FilePath -> IO ()
expectedRejudgeTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  let changed = specs 0xdeadbeef 0xdeadbeee 0x11223344
  _ <- O.writeBatch batchDirectory changed
  (reused, rejudged) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (reused == BatchReused) ("changed expectation did not reuse raw output: " ++ show reused)
  ensure (rejudged == baseline) "changed expectation changed the raw observation"
  checkResultFiles batchDirectory changed rejudged ["failed", "passed"]
  _ <- O.writeBatch batchDirectory cases
  (again, restored) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (again == BatchReused) "restoring the expectation did not reuse raw output"
  ensure (restored == baseline) "restoring the expectation changed the raw observation"
  checkResultFiles batchDirectory cases restored ["passed", "passed"]
  entries <- cacheEntries cache
  ensure (length entries == 1) ("expectation changes created extra cache entries: " ++ show entries)

wireChangeTest :: FilePath -> FilePath -> IO ()
wireChangeTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  (reused, _) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) "unchanged wire did not reuse"
  let changed = specs 0xcafebabe 0xcafebabe 0x11223344
  _ <- O.writeBatch batchDirectory changed
  (changedReuse, changedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (changedReuse == BatchExecuted) ("changed wire reused: " ++ show changedReuse)
  ensure (changedObservations /= baseline) "changed wire produced the old observation"
  ensure
    ((lookup (CaseId "writer") changedObservations >>= N.lookupRegister N.R0) == Just 0xcafebabe)
    ("changed wire observation: " ++ show changedObservations)
  (again, againObservations) <- cachedObservedBatch cache runner batchPath
  ensure (again == BatchReused) "changed wire did not become reusable"
  ensure (againObservations == changedObservations) "changed wire reuse differs"

labelChangeTest :: FilePath -> FilePath -> IO ()
labelChangeTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      renamed = zipWith (\ident caseSpec -> caseSpec {O.caseId = ident}) ["writer-renamed", "reader-renamed"] cases
      swapped = zipWith (\ident caseSpec -> caseSpec {O.caseId = ident}) ["reader", "writer"] cases
      checkPayload binding ident register wanted = do
        raw <- readCachedPayload binding (CaseId ident) >>= jsonRead
        ensure (N.parseWord32 (str (get (get raw "registers") (N.registerLabel register))) == Just wanted)
          "cached payload belongs to a different execution"
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  (reused, _) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) "unchanged labels did not reuse"
  _ <- O.writeBatch batchDirectory renamed
  (renamedReuse, renamedObservations) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (renamedReuse == BatchReused) ("renamed cases did not reuse: " ++ show renamedReuse)
  ensure (map fst renamedObservations == caseIds renamed) "renamed observation ids differ"
  ensure (map snd renamedObservations == map snd baseline) "renamed cases changed the raw observations"
  forM_ (zip renamed renamedObservations) $ \(caseSpec, (_, observation)) ->
    checkResultFile batchDirectory caseSpec observation "passed"
  renamedBinding <- inspectCachedBatch cache runner batchPath >>= \inspection -> case inspection of
    InspectionRawHit _ binding _ -> pure binding
    _ -> fail "renamed payload inspection missed"
  checkPayload renamedBinding "writer-renamed" N.R0 0xdeadbeef
  checkPayload renamedBinding "reader-renamed" N.R1 0x11223344
  entries <- cacheEntries cache
  ensure (length entries == 1) ("renaming cases created extra cache entries: " ++ show entries)
  _ <- O.writeBatch batchDirectory cases
  (restoredReuse, restoredObservations) <- cachedObservedBatch cache runner batchPath
  ensure (restoredReuse == BatchReused) "restored labels did not reuse"
  ensure (restoredObservations == baseline) "restored labels changed the raw observations"
  _ <- O.writeBatch batchDirectory swapped
  (swappedReuse, _, swappedBinding) <- cachedObservedBatchWithBinding WriteAllResults cache runner batchPath
  ensure (swappedReuse == BatchReused) "swapped labels did not reuse"
  case swappedBinding of
    Just binding -> do
      checkPayload binding "reader" N.R0 0xdeadbeef
      checkPayload binding "writer" N.R1 0x11223344
    Nothing -> fail "swapped labels lost the immutable payload binding"

runnerIdentityTest :: FilePath -> FilePath -> IO ()
runnerIdentityTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  (reused, _) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) "unchanged runner did not reuse"
  let alias = root </> "runner-alias"
  createSymbolicLink runner alias
  (aliasReuse, aliasObservations) <- cachedObservedBatch cache alias batchPath
  ensure (aliasReuse == BatchReused) ("runner alias did not reuse: " ++ show aliasReuse)
  ensure (aliasObservations == baseline) "runner alias changed the observation"
  let copy = root </> "runner-copy"
  copyFile runner copy
  permissions <- getPermissions copy
  setPermissions copy (permissions {executable = True})
  (copyReuse, copyObservations) <- cachedObservedBatch cache copy batchPath
  ensure (copyReuse == BatchExecuted) ("mutable runner copy reused: " ++ show copyReuse)
  ensure (copyObservations == baseline) "runner copy changed the observation"
  (copyAgain, _) <- cachedObservedBatch cache copy batchPath
  ensure (copyAgain == BatchExecuted) ("mutable runner copy was cached: " ++ show copyAgain)

corruptionTest :: FilePath -> FilePath -> IO ()
corruptionTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  (reused, _) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) "baseline did not reuse"
  entry <- singleEntry cache
  B.writeFile (entry </> "cases" </> "writer" </> "runner.json.gz") (BC.pack "{}")
  (rawCorrupt, rawObservations) <- cachedObservedBatchWith WriteAllResults cache runner batchPath
  ensure (rawCorrupt == BatchExecuted) ("corrupt raw output was reused: " ++ show rawCorrupt)
  ensure (rawObservations == baseline) "corrupt raw rerun produced a different observation"
  checkResultFiles batchDirectory cases rawObservations ["passed", "passed"]
  uncommittedEntry <- singleEntry cache
  removeFile (uncommittedEntry </> "manifest.json")
  (noManifest, noManifestObservations) <- cachedObservedBatch cache runner batchPath
  ensure (noManifest == BatchExecuted) ("uncommitted manifest was reused: " ++ show noManifest)
  ensure (noManifestObservations == baseline) "uncommitted rerun produced a different observation"
  missingExitEntry <- singleEntry cache
  removeFile (missingExitEntry </> "cases" </> "reader" </> "exit")
  (noExit, _) <- cachedObservedBatch cache runner batchPath
  ensure (noExit == BatchExecuted) ("missing recorded exit was reused: " ++ show noExit)
  provenanceEntry <- singleEntry cache
  B.writeFile (provenanceEntry </> "cases" </> "writer" </> "provenance.json") (BC.pack "{}")
  (badProvenance, _) <- cachedObservedBatch cache runner batchPath
  ensure (badProvenance == BatchExecuted) ("corrupt provenance was reused: " ++ show badProvenance)
  substitutedEntry <- singleEntry cache
  readerRaw <- B.readFile (substitutedEntry </> "cases" </> "reader" </> "runner.json.gz")
  B.writeFile (substitutedEntry </> "cases" </> "writer" </> "runner.json.gz") readerRaw
  (substituted, substitutedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (substituted == BatchExecuted) ("substituted raw output was reused: " ++ show substituted)
  ensure (substitutedObservations == baseline) "substituted raw rerun produced a different observation"

interruptedTest :: FilePath -> FilePath -> IO ()
interruptedTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  entry <- singleEntry cache
  removeFile (entry </> "manifest.json")
  (uncommitted, uncommittedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (uncommitted == BatchExecuted) ("interrupted entry was reused: " ++ show uncommitted)
  ensure (uncommittedObservations == baseline) "interrupted rerun produced a different observation"
  let partial = cache </> "v1" </> "tmp" </> "interrupted-attempt"
  createDirectoryIfMissing True partial
  B.writeFile (partial </> "runner.json") (BC.pack "partial")
  (committed, committedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (committed == BatchReused) ("leftover partial attempt interrupted a committed entry: " ++ show committed)
  ensure (committedObservations == baseline) "committed reuse differs after a partial attempt"

faultyTest :: FilePath -> FilePath -> IO ()
faultyTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      faulty = spec "faulty" [0xe3a00000, 0xe5820000, 0xe12fff1e]
        [(N.R0, 0xdeadbeef), (N.R2, 0x9000)] [(N.R0, 0xdeadbeef)]
        [(N.RegionId "data", O.word32le 0xdeadbeef)]
      cases = [faulty]
  batchPath <- O.writeBatch batchDirectory cases
  (executed, observations) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "faulty batch did not execute"
  ensure (all ((== N.Returned) . N.nativeStatus . snd) observations) "faulty batch lost its raw observation"
  checkResultFiles batchDirectory cases observations ["failed"]
  (reused, reusedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) ("faulty batch was not reusable: " ++ show reused)
  ensure (reusedObservations == observations) "faulty batch reuse changed the observation"
  checkResultFiles batchDirectory cases reusedObservations ["failed"]

incompleteOutputTest :: FilePath -> IO ()
incompleteOutputTest root = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  let fakeRunner = root </> "fake-runner"
  B.writeFile fakeRunner (BC.pack "#!/bin/sh\nprintf 'FUJI_NATIVE_BATCH_CASE 0 1\\nnot json\\n'\nexit 1\n")
  permissions <- getPermissions fakeRunner
  setPermissions fakeRunner (permissions {executable = True})
  outcome <-
    try (cachedObservedBatch cache fakeRunner batchPath)
      :: IO (Either IOException (BatchReuse, [(CaseId, N.NativeObservation)]))
  case outcome of
    Right _ -> fail "incomplete native output was accepted"
    Left _ -> pure ()
  case cases of
    [] -> fail "incomplete output fixture has no case"
    (caseSpec : _) -> do
      let resultPath = batchDirectory </> (O.caseId caseSpec ++ ".native-result.json")
      present <- doesFileExist resultPath
      ensure present "incomplete native output did not retain its failure report"
      failed <- B.readFile resultPath >>= jsonRead
      ensure (str (get failed "status") == "runner_failure") ("unexpected failure status: " ++ show failed)
      ensure
        (str (get failed "runner_stdout") == "not json\n")
        ("failure report lacks raw stdout: " ++ show failed)

rootAliasTest :: FilePath -> FilePath -> IO ()
rootAliasTest root runner = do
  let cache = root </> "cache"
      alias = root </> "cache-alias"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "alias baseline did not execute"
  createSymbolicLink cache alias
  (reused, observations) <- cachedObservedBatch alias runner batchPath
  ensure (reused == BatchReused) ("cache root alias did not reuse: " ++ show reused)
  ensure (observations == baseline) "cache root alias changed the observation"
  entries <- cacheEntries cache
  ensure (length entries == 1) ("cache root alias created extra entries: " ++ show entries)

escapeV1Test :: FilePath -> FilePath -> IO ()
escapeV1Test root runner = do
  let cache = root </> "cache"
      outside = root </> "outside"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  createDirectoryIfMissing True cache
  seedOutside outside
  before <- snapshotTree outside
  createSymbolicLink outside (cache </> "v1")
  refused cache runner batchPath
  after <- snapshotTree outside
  ensure (before == after) "escaping v1 symlink changed the outside tree"
  link <- getSymbolicLinkStatus (cache </> "v1")
  ensure (isSymbolicLink link) "escaping v1 symlink was removed"

escapeTmpTest :: FilePath -> FilePath -> IO ()
escapeTmpTest root runner = do
  let cache = root </> "cache"
      outside = root </> "outside"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  createDirectoryIfMissing True (cache </> "v1")
  seedOutside outside
  before <- snapshotTree outside
  createSymbolicLink outside (cache </> "v1" </> "tmp")
  refused cache runner batchPath
  after <- snapshotTree outside
  ensure (before == after) "escaping tmp symlink changed the outside tree"
  link <- getSymbolicLinkStatus (cache </> "v1" </> "tmp")
  ensure (isSymbolicLink link) "escaping tmp symlink was removed"

escapeKeyTest :: FilePath -> FilePath -> IO ()
escapeKeyTest root runner = do
  let cache = root </> "cache"
      outside = root </> "outside"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, _) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "key baseline did not execute"
  entry <- singleEntry cache
  removeDirectoryRecursive entry
  seedOutside outside
  before <- snapshotTree outside
  createSymbolicLink outside entry
  refused cache runner batchPath
  after <- snapshotTree outside
  ensure (before == after) "escaping entry symlink changed the outside tree"
  link <- getSymbolicLinkStatus entry
  ensure (isSymbolicLink link) "escaping entry symlink was removed"

escapeCaseTest :: FilePath -> FilePath -> IO ()
escapeCaseTest root runner = do
  let cache = root </> "cache"
      outside = root </> "outside"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, _) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "case baseline did not execute"
  entry <- singleEntry cache
  let caseDirectory = entry </> "cases" </> "writer"
  removeDirectoryRecursive caseDirectory
  seedOutside outside
  before <- snapshotTree outside
  createSymbolicLink outside caseDirectory
  refused cache runner batchPath
  after <- snapshotTree outside
  ensure (before == after) "escaping case symlink changed the outside tree"
  link <- getSymbolicLinkStatus caseDirectory
  ensure (isSymbolicLink link) "escaping case symlink was removed"
  still <- singleEntry cache
  ensure (still == entry) "refusing a case symlink moved the committed entry"

refused :: FilePath -> FilePath -> FilePath -> IO ()
refused cache runner batchPath = do
  result <-
    try (cachedObservedBatch cache runner batchPath)
      :: IO (Either CacheEscape (BatchReuse, [(CaseId, N.NativeObservation)]))
  case result of
    Left _ -> pure ()
    Right (reuse, _) -> fail ("escaping cache path was followed: " ++ show reuse)

seedOutside :: FilePath -> IO ()
seedOutside outside = do
  createDirectoryIfMissing True (outside </> "nested")
  B.writeFile (outside </> "sentinel") (BC.pack "outside")
  B.writeFile (outside </> "nested" </> "payload") (BC.pack "untouched")

snapshotTree :: FilePath -> IO [(FilePath, B.ByteString)]
snapshotTree directory = do
  names <- sort <$> listDirectory directory
  fmap concat . forM names $ \name -> do
    let path = directory </> name
    isDirectory <- doesDirectoryExist path
    if isDirectory
      then map (\(relative, bytes) -> (name </> relative, bytes)) <$> snapshotTree path
      else do
        bytes <- B.readFile path
        pure [(name, bytes)]

checkResultFiles :: FilePath -> [O.CaseSpec] -> [(CaseId, N.NativeObservation)] -> [String] -> IO ()
checkResultFiles directory cases observations statuses = do
  ensure (map fst observations == caseIds cases && length cases == length statuses) "result cohort differs"
  forM_ (zip3 cases observations statuses) $ \(caseSpec, (_, observation), status) ->
    checkResultFile directory caseSpec observation status

checkResultFile :: FilePath -> O.CaseSpec -> N.NativeObservation -> String -> IO ()
checkResultFile batchDirectory caseSpec observation wantedStatus = do
  let ident = O.caseId caseSpec
      resultPath = batchDirectory </> (ident ++ ".native-result.json")
      casePath = batchDirectory </> (ident ++ ".case.json")
  result <- B.readFile resultPath >>= jsonRead
  ensure
    (str (get result "status") == wantedStatus)
    (ident ++ ": result status " ++ str (get result "status") ++ ", wanted " ++ wantedStatus)
  ensure (get (get result "runner") "memory_provenance" /= Null) (ident ++ ": result has no raw runner observation")
  exit <- either (fail . N.renderObservationError) pure (N.reportRunnerExit result)
  loaded <- loadCaseWith readArtifact casePath
  let regions = map fst (loadedRegions loaded)
  observed <- either (fail . N.renderObservationError) pure (N.observeNative exit regions (get result "runner"))
  ensure (observed == observation) (ident ++ ": result file observation differs from the returned observation")

singleEntry :: FilePath -> IO FilePath
singleEntry cache = do
  entries <- cacheEntries cache
  case entries of
    [entry] -> pure entry
    _ -> fail ("expected one committed cache entry, found " ++ show entries)

cacheEntries :: FilePath -> IO [FilePath]
cacheEntries cache = do
  let versionDirectory = cache </> "v1"
  names <- listDirectory versionDirectory
  directories <- filterM (doesDirectoryExist . (versionDirectory </>)) names
  pure
    [ versionDirectory </> name
    | name <- directories
    , name /= "tmp"
    , name /= "quarantine"
    , not ("." `isPrefixOf` name)
    ]

retentionTest :: FilePath -> FilePath -> IO ()
retentionTest root runner = do
  let cache = root </> "cache"
      passing = root </> "passing"
      failing = root </> "failing"
  createDirectoryIfMissing True root
  jobs <- readNativeJobs Nothing
  cacheHandle <- openNativeCache jobs cache
  passed <- runSpecs cacheHandle runner passing [writerSpec 0xdeadbeef 0xdeadbeef]
  ensure (passed == [("writer",[])]) ("passing retention rows differ: " ++ show passed)
  passingSummary <- B.readFile (passing </> "0" </> "summary.json") >>= jsonRead
  ensure (get passingSummary "raw_cache" /= Null) "passing summary has no raw cache binding"
  let entry = str (get (get passingSummary "raw_cache") "entry")
  present <- doesDirectoryExist entry
  ensure present ("passing summary raw cache entry is absent: " ++ entry)
  retainedResult <- doesFileExist (passing </> "0" </> "writer.native-result.json")
  retainedBatch <- doesFileExist (passing </> "0" </> "batch.json")
  ensure (not retainedResult) "passing case retained a judged result"
  ensure (not retainedBatch) "passing case retained staged batch"
  failed <- runSpecs cacheHandle runner failing [writerSpec 0xdeadbeef 0xdeadbeee]
  ensure (not (null (concatMap snd failed))) "failing retention case did not fail"
  retainedFailedBatch <- doesFileExist (failing </> "0" </> "batch.json")
  retainedGenericFailure <- doesFileExist (failing </> "0" </> "writer.native-result.json")
  retainedConsumerFailure <- doesFileExist (failing </> "0" </> "writer.consumer-failure.json")
  ensure retainedFailedBatch "failing case lost staged batch"
  ensure retainedGenericFailure "failing case lost generic result"
  ensure retainedConsumerFailure "failing case lost consumer report"
  failingNames <- listDirectory (failing </> "0")
  ensure (any (".bin" `isSuffixOf`) failingNames) "failing case lost staged artifacts"
  failingSummary <- B.readFile (failing </> "0" </> "summary.json") >>= jsonRead
  let reports = list (get failingSummary "first_failure_reports")
  ensure (not (null reports)) "failing summary has no first failure report"
  forM_ reports $ \report -> do
    exists <- doesFileExist (str report)
    ensure exists ("first failure report is missing: " ++ str report)

defaultResultPolicyTest :: FilePath -> FilePath -> IO ()
defaultResultPolicyTest root runner = do
  let passing = root </> "passing"
      failing = root </> "failing"
      passingCases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      failingCases = [writerSpec 0xdeadbeef 0xdeadbeee]
  createDirectoryIfMissing True root
  passingPath <- O.writeBatch passing passingCases
  (executed, defaultObservations) <- cachedObservedBatch (root </> "cache-default") runner passingPath
  ensure (executed == BatchExecuted) "default policy baseline did not execute"
  defaultResult <- doesFileExist (passing </> "writer.native-result.json")
  ensure (not defaultResult) "default policy wrote a passing case result"
  (reused, hitObservations) <- cachedObservedBatch (root </> "cache-default") runner passingPath
  ensure (reused == BatchReused) "default policy did not reuse"
  ensure (hitObservations == defaultObservations) "default policy hit changed the observations"
  hitResult <- doesFileExist (passing </> "writer.native-result.json")
  ensure (not hitResult) "default policy hit wrote a passing case result"
  (_, allObservations) <- cachedObservedBatchWith WriteAllResults (root </> "cache-all") runner passingPath
  ensure (allObservations == defaultObservations) "default policy changed the observations"
  forM_ (zip passingCases allObservations) $ \(caseSpec, (_, observation)) ->
    checkResultFile passing caseSpec observation "passed"
  failingPath <- O.writeBatch failing failingCases
  (failedExecuted, failedObservations) <- cachedObservedBatch (root </> "cache-failed") runner failingPath
  ensure (failedExecuted == BatchExecuted) "failing default policy did not execute"
  checkResultFiles failing failingCases failedObservations ["failed"]

cacheEvictionTest :: FilePath -> FilePath -> IO ()
cacheEvictionTest root runner = do
  let cache = root </> "cache"
      first = root </> "first"
      second = root </> "second"
      firstCases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      secondCases = specs 0xcafebabe 0xcafebabe 0x11223344
  createDirectoryIfMissing True root
  firstPath <- O.writeBatch first firstCases
  secondPath <- O.writeBatch second secondCases
  (firstReuse, _) <- withCacheBudget "20G" $
    cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner firstPath
  ensure (firstReuse == BatchExecuted) "eviction baseline did not execute"
  firstEntry <- singleEntry cache
  firstBytes <- treeBytes firstEntry
  withCacheBudget (show (firstBytes + 1)) $ do
    (secondReuse, _) <- cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner secondPath
    ensure (secondReuse == BatchExecuted) "second publish did not execute"
    firstPresent <- doesDirectoryExist firstEntry
    entries <- cacheEntries cache
    ensure (not firstPresent) "publishing past the budget kept the oldest entry"
    ensure
      (length entries == 1 && takeFileName (head entries) /= takeFileName firstEntry)
      ("publishing past the budget did not keep only the newest entry: " ++ show entries)
    (again, _) <- cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner firstPath
    ensure (again == BatchExecuted) "an evicted batch did not execute again"
    republished <- doesDirectoryExist firstEntry
    ensure republished "an evicted batch was not cached again"
    (reused, _) <- cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner firstPath
    ensure (reused == BatchReused) "a republished batch did not reuse"

cacheRecencyTest :: FilePath -> FilePath -> IO ()
cacheRecencyTest root runner = do
  let cache = root </> "cache"
      first = root </> "first"
      second = root </> "second"
      third = root </> "third"
      firstCases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      secondCases = specs 0xcafebabe 0xcafebabe 0x11223344
      thirdCases = specs 0x0badf00d 0x0badf00d 0x11223344
  createDirectoryIfMissing True root
  firstPath <- O.writeBatch first firstCases
  secondPath <- O.writeBatch second secondCases
  thirdPath <- O.writeBatch third thirdCases
  _ <- withCacheBudget "20G" $
    cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner thirdPath
  thirdEntry <- singleEntry cache
  thirdBytes <- treeBytes thirdEntry
  removeDirectoryRecursive thirdEntry
  (_, _) <- withCacheBudget "20G" $
    cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner firstPath
  firstEntry <- singleEntry cache
  firstBytes <- treeBytes firstEntry
  _ <- withCacheBudget "20G" $
    cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner secondPath
  entries <- cacheEntries cache
  secondEntry <- case [entry | entry <- entries, takeFileName entry /= takeFileName firstEntry] of
    [entry] -> pure entry
    _ -> fail ("second publish did not add one entry: " ++ show entries)
  (hit, _) <- withCacheBudget "20G" $
    cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner firstPath
  ensure (hit == BatchReused) "recency baseline did not reuse"
  withCacheBudget (show (firstBytes + thirdBytes + 1)) $ do
    _ <- cachedObservedBatchWithSince (MaintenanceInterval 0) WriteFailedResults cache runner thirdPath
    firstPresent <- doesDirectoryExist firstEntry
    secondPresent <- doesDirectoryExist secondEntry
    thirdPresent <- doesDirectoryExist thirdEntry
    ensure firstPresent "a recently reused entry was evicted"
    ensure (not secondPresent) "the older entry survived the byte budget"
    ensure thirdPresent "the newly published entry was evicted"

withCacheBudget :: String -> IO a -> IO a
withCacheBudget value action = do
  previous <- lookupEnv "XM1_NATIVE_CACHE_MAX"
  let restore = maybe (unsetEnv "XM1_NATIVE_CACHE_MAX") (setEnv "XM1_NATIVE_CACHE_MAX") previous
  bracket_ (setEnv "XM1_NATIVE_CACHE_MAX" value) restore action

treeBytes :: FilePath -> IO Integer
treeBytes path = do
  directory <- doesDirectoryExist path
  if directory
    then do
      names <- listDirectory path
      sum <$> mapM (treeBytes . (path </>)) names
    else do
      size <- getFileSize path
      pure (fromIntegral size)

assetWireTest :: FilePath -> FilePath -> IO ()
assetWireTest root runner = do
  let direct = root </> "direct"
      shared = root </> "shared"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  createDirectoryIfMissing True root
  directPath <- O.writeBatch direct cases
  sharedPath <- E.withInputPool root $ \pool -> O.writeBatchWithInputPool pool shared cases
  directBytes <- B.readFile directPath
  sharedBytes <- B.readFile sharedPath
  ensure (directBytes == sharedBytes) "shared-asset batch wire differs from direct batch wire"
  forM_ cases $ \caseSpec -> do
    let name = O.caseId caseSpec ++ ".case.json"
    a <- B.readFile (direct </> name)
    b <- B.readFile (shared </> name)
    ensure (a == b) ("shared-asset case wire differs: " ++ name)
  (_, directObs) <- cachedObservedBatch (root </> "cache-direct") runner directPath
  names <- listDirectory shared
  let bins = [name | name <- names, ".bin" `isSuffixOf` name]
  ensure (not (null bins)) "shared batch has no staged artifact"
  forM_ bins $ \name -> do
    status <- getFileStatus (shared </> name)
    ensure (isRegularFile status) ("staged artifact is not regular after pool removal: " ++ name)
  (_, sharedObs) <- cachedObservedBatch (root </> "cache-shared") runner sharedPath
  ensure (directObs == sharedObs) "cache consumer observations differ after pool removal"
  directEntries <- cacheEntries (root </> "cache-direct")
  sharedEntries <- cacheEntries (root </> "cache-shared")
  ensure (map takeFileName directEntries == map takeFileName sharedEntries) "cache identity changed under shared staging"

consumerFrameTest :: FilePath -> FilePath -> IO ()
consumerFrameTest root runner = do
  let cache = root </> "cache"
      directory = root </> "cohort"
      changedStack = B.replicate 20 0 <> O.word32le 1 <> B.replicate 8 0
      base = spec "frame" [0xe3a00001, 0xe58d0004, 0xe12fff1e] [(N.R0, 0)] [(N.R0, 1)] [(N.RegionId "stack", changedStack)]
      frameCase = base { O.caseInitial = (N.Sp, 0xb010) : filter ((/= N.Sp) . fst) (O.caseInitial base) }
  genericPath <- O.writeBatch (root </> "generic") [frameCase]
  _ <- cachedObservedBatchWith WriteAllResults cache runner genericPath
  genericReport <- B.readFile (root </> "generic" </> "frame.native-result.json") >>= jsonRead
  ensure (get genericReport "status" == String "passed") "frame fixture does not pass generic judgement"
  jobs <- readNativeJobs Nothing
  cacheHandle <- openNativeCache jobs cache
  _ <- runSpecs cacheHandle runner directory [frameCase]
  summary <- B.readFile (directory </> "0" </> "summary.json") >>= jsonRead
  case list (get summary "first_failure_reports") of
    [reportPath] -> do
      report <- B.readFile (str reportPath) >>= jsonRead
      ensure (get report "status" == String "failed") "caller-frame defect was accepted"
      ensure (get report "raw_cache" /= Null) "consumer frame report has no raw cache binding"
    reports -> fail ("consumer frame first failure reports differ: " ++ show reports)

deadPrivateFrameTest :: FilePath -> FilePath -> IO ()
deadPrivateFrameTest root runner = do
  let cache = root </> "cache"
      directory = root </> "cohort"
      base = spec "dead-frame" [0xe50d0004, 0xe12fff1e] [(N.R0, 1)] [(N.R0, 1)] []
      frameCase = base { O.caseInitial = (N.Sp, 0xb010) : filter ((/= N.Sp) . fst) (O.caseInitial base) }
      explicit = frameCase { O.caseIgnoredMemory = [O.IgnoredMemory (N.RegionId "stack") 0 0x10 O.PrivateFrame] }
  genericPath <- O.writeBatch (root </> "generic") [frameCase]
  (firstReuse, _) <- cachedObservedBatch cache runner genericPath
  ensure (firstReuse == BatchExecuted) "conservative dead-frame batch did not execute"
  genericReport <- B.readFile (root </> "generic" </> "dead-frame.native-result.json") >>= jsonRead
  ensure (get genericReport "status" == String "failed") "conservative judgement accepted an undeclared dead frame"
  _ <- O.writeBatch (root </> "generic") [explicit]
  (secondReuse, _) <- cachedObservedBatch cache runner genericPath
  ensure (secondReuse == BatchReused) "explicit dead-frame expectation did not reuse the raw observation"
  jobs <- readNativeJobs Nothing
  cacheHandle <- openNativeCache jobs cache
  results <- runSpecs cacheHandle runner directory [explicit]
  ensure (all (null . snd) results) "dead private frame was refused by runSpecs"
  summary <- B.readFile (directory </> "0" </> "summary.json") >>= jsonRead
  ensure (get summary "execution" == String "reused") "dead-frame consumer did not reuse execution"
  ensure (get summary "raw_cache" /= Null) "dead-frame consumer has no immutable raw binding"
  ensure (null (list (get summary "first_failure_reports"))) "dead private frame retained a failure report"

genericFrameTest :: FilePath -> FilePath -> IO ()
genericFrameTest root runner = do
  let cache = root </> "cache"
      base = spec "generic-frame" [0xe50d0004, 0xe12fff1e] [(N.R0, 1)] [(N.R0, 1)] []
      frameCase = base { O.caseInitial = (N.Sp, 0xb010) : filter ((/= N.Sp) . fst) (O.caseInitial base) }
  genericPath <- O.writeBatch (root </> "generic") [frameCase]
  _ <- cachedObservedBatch cache runner genericPath
  genericReport <- B.readFile (root </> "generic" </> "generic-frame.native-result.json") >>= jsonRead
  ensure (get genericReport "status" == String "failed") "generic judgement accepted an undeclared dead frame"

ignoredBoundaryTest :: FilePath -> IO ()
ignoredBoundaryTest root = do
  createDirectoryIfMissing True root
  let base = spec "ignored" [0xe3a00001, 0xe12fff1e] [(N.R0, 1)] [(N.R0, 1)] []
      frameCase ignored status = base
        { O.caseInitial = (N.Sp, 0xb010) : filter ((/= N.Sp) . fst) (O.caseInitial base)
        , O.caseExpectedStatus = status
        , O.caseIgnoredMemory = ignored
        }
      stackRange = O.IgnoredMemory (N.RegionId "stack") 0 0x10 O.PrivateFrame
      regionsOf spec' = [O.caseRegion r | r <- O.caseRegions spec']
      regionName r = str (get (description r) "id")
      resultFor spec' stackBytes =
        let regions = regionsOf spec'
        in object
          [ "status" .= N.runnerStatusLabel (O.caseExpectedStatus spec')
          , "registers" .= object ["r0" .= ("0x00000001" :: String)]
          , "memory" .=
              [ object ["address" .= get (description r) "address"
                       , "bytes" .= hexBytes (if regionName r == "stack" then stackBytes else regionBytes r)]
              | r <- regions
              ]
          , "events" .= ([] :: [Value])
          ]
      judge label spec' stackBytes = do
        path <- O.writeCase (root </> label) spec'
        c <- B.readFile path >>= jsonRead
        checkResult c (regionsOf spec') (resultFor spec' stackBytes)
      fullStack spec' = case [regionBytes r | r <- regionsOf spec', regionName r == "stack"] of
        [bytes] -> bytes
        _ -> error "missing stack"
      expectPass label spec' stackBytes = do
        outcome <- try (judge label spec' stackBytes) :: IO (Either SomeException [String])
        case outcome of
          Right problems -> ensure (null problems) "explicit dead frame was refused"
          Left _ -> fail "explicit dead frame raised an input refusal"
      expectRefusal label spec' stackBytes = do
        outcome <- try (judge label spec' stackBytes) :: IO (Either SomeException [String])
        case outcome of
          Left _ -> pure ()
          Right _ -> fail "invalid ignored range was not refused"
      expectMismatch label spec' stackBytes = do
        outcome <- try (judge label spec' stackBytes) :: IO (Either SomeException [String])
        case outcome of
          Right problems -> ensure (not (null problems)) "truncated memory was accepted"
          Left _ -> fail "truncated memory raised an input refusal"
      outputBase = (spec "caller-output" [0xe5801000,0xe12fff1e] [(N.R0,0x9000),(N.R1,0x11223344)] [(N.R0,0)] [])
        { O.caseRegions =
            [ O.mkRegion (N.RegionId "code") (N.GuestAddress 0x8000) 8 O.ReadExecute (O.assemble [0xe5801000,0xe12fff1e])
            , O.mkRegion (N.RegionId "data") (N.GuestAddress 0x9000) 16 O.ReadWrite (B.replicate 16 0x5a)
            , O.mkRegion (N.RegionId "exit") (N.GuestAddress 0xa000) 4 O.ReadExecute (B.replicate 4 0)
            , O.mkRegion (N.RegionId "stack") (N.GuestAddress 0xb000) 32 O.ReadWrite (B.replicate 32 0)
            ]
        , O.caseFunctions = [(N.GuestAddress 0x8000,8)]
        , O.caseInitial = [(N.R0,0x9000),(N.R1,0x11223344),(N.Sp,0xb020),(N.Lr,0xa000),(N.Cpsr,0x10)]
        }
      outputCase ignored expected = outputBase { O.caseIgnoredMemory = ignored, O.caseExpectedMemory = expected }
      outputResult spec' =
        let regions = regionsOf spec'
        in object
          [ "status" .= N.runnerStatusLabel (O.caseExpectedStatus spec')
          , "registers" .= object ["r0" .= ("0x00000000" :: String)]
          , "memory" .=
              [ object ["address" .= get (description r) "address"
                       , "bytes" .= hexBytes (if regionName r == "data" then O.word32le 0x11223344 <> B.replicate 12 0x5a else regionBytes r)]
              | r <- regions
              ]
          , "events" .= ([] :: [Value])
          ]
      judgeOutput label spec' = do
        path <- O.writeCase (root </> label) spec'
        c <- B.readFile path >>= jsonRead
        checkResult c (regionsOf spec') (outputResult spec')
      expectOutputPass label spec' = do
        outcome <- try (judgeOutput label spec') :: IO (Either SomeException [String])
        case outcome of
          Right problems -> ensure (null problems) "caller-owned output prefix was refused"
          Left _ -> fail "caller-owned output prefix raised an input refusal"
      expectOutputRefusal label spec' = do
        outcome <- try (judgeOutput label spec') :: IO (Either SomeException [String])
        case outcome of
          Left _ -> pure ()
          Right _ -> fail "invalid caller-owned output range was not refused"
      dataPrefix = [(N.RegionId "data", O.word32le 0x11223344)]
      callerOutput = O.IgnoredMemory (N.RegionId "data") 4 12 O.CallerOutput
  expectOutputPass "caller-output" (outputCase [callerOutput] dataPrefix)
  expectOutputRefusal "caller-output-out-of-bounds" (outputCase [O.IgnoredMemory (N.RegionId "data") 4 16 O.CallerOutput] dataPrefix)
  expectOutputRefusal "caller-output-overlap" (outputCase [O.IgnoredMemory (N.RegionId "data") 0 8 O.CallerOutput] dataPrefix)
  expectOutputRefusal "caller-output-read-only" (outputCase [O.IgnoredMemory (N.RegionId "code") 0 4 O.CallerOutput] dataPrefix)
  expectOutputRefusal "caller-output-no-prefix" (outputCase [callerOutput] [])
  expectPass "explicit-dead" (frameCase [stackRange] N.Returned) (fullStack (frameCase [stackRange] N.Returned))
  let assertedFrame = (frameCase [stackRange] N.Returned)
        { O.caseExpectedMemory = [(N.RegionId "stack", O.word32le 1)] }
  expectRefusal "asserted-private-output" assertedFrame (fullStack assertedFrame)
  expectRefusal "overlap" (frameCase [stackRange, O.IgnoredMemory (N.RegionId "stack") 4 8 O.PrivateFrame] N.Returned) (fullStack (frameCase [stackRange] N.Returned))
  expectRefusal "live" (frameCase [O.IgnoredMemory (N.RegionId "stack") 0x10 8 O.PrivateFrame] N.Returned) (fullStack (frameCase [stackRange] N.Returned))
  expectRefusal "stopped" (frameCase [stackRange] N.StopAddress) (fullStack (frameCase [stackRange] N.Returned))
  expectMismatch "truncated" (frameCase [stackRange] N.Returned) (B.take 8 (fullStack (frameCase [stackRange] N.Returned)))

inspectionSafetyTest :: FilePath -> FilePath -> IO ()
inspectionSafetyTest root runner = do
  createDirectoryIfMissing True root
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      caseSpec = spec "inspect" [0xe12fff1e] [] [(N.R0, 0)] []
  batchPath <- O.writeBatch batchDirectory [caseSpec]
  first <- inspectCachedBatch cache runner batchPath
  entry <- case first of
    InspectionAbsent path -> pure path
    _ -> fail "missing cache entry did not report absent"
  createDirectoryIfMissing True entry
  B.writeFile (entry </> "manifest.json") "{}"
  second <- inspectCachedBatch cache runner batchPath
  case second of
    InspectionInvalid path _ -> ensure (path == entry) "invalid cache entry path differs"
    _ -> fail "invalid cache entry did not report invalid"
  retained <- doesDirectoryExist entry
  ensure retained "invalid lookup removed the existing entry"
  quarantined <- doesDirectoryExist (cache </> "v1" </> "quarantine")
  ensure (not quarantined) "invalid lookup quarantined the existing entry"

inspectionEligibilityTest :: FilePath -> IO ()
inspectionEligibilityTest root = do
  createDirectoryIfMissing True root
  let cache = root </> "cache"
      runner = root </> "runner.sh"
      marker = root </> "runner-invoked"
      batchDirectory = root </> "batch"
      caseSpec = spec "inspect" [0xe12fff1e] [] [(N.R0, 0)] []
  BC.writeFile runner (BC.pack ("#!/bin/sh\ntouch '" ++ marker ++ "'\n"))
  permissions <- getPermissions runner
  setPermissions runner (permissions {executable = True})
  batchPath <- O.writeBatch batchDirectory [caseSpec]
  result <- inspectCachedBatch cache runner batchPath
  case result of
    InspectionIneligible _ -> pure ()
    _ -> fail "mutable runner was inspected as reusable"
  invoked <- doesFileExist marker
  ensure (not invoked) "ineligible inspection invoked the runner"

interruptedRunSpecsTest :: FilePath -> FilePath -> IO ()
interruptedRunSpecsTest root _runner = do
  let cache = root </> "cache"
      directory = root </> "cohort"
      faulty = root </> "faulty-runner"
  createDirectoryIfMissing True root
  BC.writeFile faulty (BC.pack "#!/bin/sh\nexit 1\n")
  permissions <- getPermissions faulty
  setPermissions faulty (permissions { executable = True })
  jobs <- readNativeJobs Nothing
  cacheHandle <- openNativeCache jobs cache
  outcome <- try (runSpecs cacheHandle faulty directory [writerSpec 0xdeadbeef 0xdeadbeef]) :: IO (Either SomeException [(String, [String])])
  case outcome of
    Right _ -> fail "interrupted runSpecs returned a result"
    Left _ -> pure ()
  retainedBatch <- doesFileExist (directory </> "0" </> "batch.json")
  retainedCase <- doesFileExist (directory </> "0" </> "writer.case.json")
  names <- listDirectory (directory </> "0")
  ensure retainedBatch "interrupted run lost the staged batch"
  ensure retainedCase "interrupted run lost the staged case"
  ensure (any (".bin" `isSuffixOf`) names) "interrupted run lost staged artifacts"
  completed <- doesFileExist (directory </> "0" </> "summary.json")
  ensure (not completed) "interrupted run wrote a completed summary"

resumeOrdinaryTest :: FilePath -> FilePath -> IO ()
resumeOrdinaryTest root runner = do
  let batchDir = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      cited = batchDir </> "writer.native-result.json"
  createDirectoryIfMissing True root
  directPath <- O.writeBatch batchDir cases
  directBytes <- B.readFile directPath
  (_, directObs) <- cachedObservedBatch (root </> "cache-direct") runner directPath
  B.writeFile cited (BC.pack "cited failure")
  resumedPath <- E.withInputPool root $ \pool -> O.writeBatchWithInputPool pool batchDir cases
  resumedBytes <- B.readFile resumedPath
  ensure (directBytes == resumedBytes) "resume changed the batch wire"
  citedBytes <- B.readFile cited
  ensure (citedBytes == BC.pack "cited failure") "resume changed the cited failure artifact"
  (_, resumedObs) <- cachedObservedBatch (root </> "cache-resumed") runner resumedPath
  ensure (directObs == resumedObs) "resume changed the cache consumer observations"
  directEntries <- cacheEntries (root </> "cache-direct")
  resumedEntries <- cacheEntries (root </> "cache-resumed")
  ensure (map takeFileName directEntries == map takeFileName resumedEntries) "resume changed the cache identity"

storageBudgetTransitionTest :: FilePath -> FilePath -> IO ()
storageBudgetTransitionTest root runner = do
  let cache = root </> "cache"
      sevenPath = root </> "seven" </> "batch.json"
      ninePath = root </> "nine" </> "batch.json"
  _ <- O.writeBatch (root </> "seven") [writerSpec 7 7]
  (sevenExecuted, sevenObs) <- cachedObservedBatch cache runner sevenPath
  ensure (sevenExecuted == BatchExecuted) "storage baseline did not execute"
  ensure
    ([(N.nativeStatus observation, N.lookupRegister N.R0 observation, lookup (N.RegionId "data") (N.nativeMemory observation)) | (_, observation) <- sevenObs]
      == [(N.Returned, Just 7, Just (O.word32le 7))])
    "storage baseline returned the wrong value or memory"
  original <- lookupEnv "XM1_STORAGE_MIN_FREE"
  let restore = maybe (unsetEnv "XM1_STORAGE_MIN_FREE") (setEnv "XM1_STORAGE_MIN_FREE") original
  bracket_ (setEnv "XM1_STORAGE_MIN_FREE" "100T") restore $ do
    (sevenReused, sevenReusedObs) <- cachedObservedBatch cache runner sevenPath
    ensure (sevenReused == BatchReused) "raised reserve blocked a cache hit"
    ensure (sevenReusedObs == sevenObs) "raised reserve changed the cached observation"
    _ <- O.writeBatch (root </> "nine") [writerSpec 9 9]
    beforeEntries <- cacheEntries cache
    missResult <- try (cachedObservedBatch cache runner ninePath) :: IO (Either SomeException (BatchReuse, [(CaseId, N.NativeObservation)]))
    case missResult of
      Right _ -> fail "raised reserve did not refuse a distinct cache miss"
      Left _ -> pure ()
    afterEntries <- cacheEntries cache
    ensure (sort afterEntries == sort beforeEntries) "refused cache miss changed published entries"
  (nineExecuted, nineObs) <- cachedObservedBatch cache runner ninePath
  ensure (nineExecuted == BatchExecuted) "restored reserve did not execute the distinct wire"
  ensure
    ([(N.nativeStatus observation, N.lookupRegister N.R0 observation, lookup (N.RegionId "data") (N.nativeMemory observation)) | (_, observation) <- nineObs]
      == [(N.Returned, Just 9, Just (O.word32le 9))])
    "restored execution returned the wrong value or memory"

concurrentReuseTest :: FilePath -> FilePath -> IO ()
concurrentReuseTest root runner = do
  jobs <- readNativeJobs (Just "4")
  let cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
      cache = root </> "cache"
  results <- mapNativeBounded jobs (\index -> do
    let directory = root </> show index
    batch <- O.writeBatch directory cases
    result@(_, observations) <- cachedObservedBatchWith WriteAllResults cache runner batch
    ensure (map fst observations == caseIds cases) "parallel cache result identities differ"
    forM_ (zip cases observations) $ \(caseSpec, (_, observation)) ->
      checkResultFile directory caseSpec observation "passed"
    pure result) ([0..7] :: [Int])
  ensure (length [() | (BatchExecuted, _) <- results] == 1) "concurrent cache misses executed duplicate work"
  ensure (length [() | (BatchReused, _) <- results] == 7) "concurrent cache results did not reuse the single execution"
  let observations = map (map snd . snd) results
  case observations of
    first : rest -> ensure (all (== first) rest) "concurrent cache observations differ"
    [] -> fail "parallel cache returned no observations"

qualificationParallelTest :: FilePath -> FilePath -> IO ()
qualificationParallelTest root runner = do
  jobs <- readNativeJobs (Just "4")
  cache <- openNativeCache jobs (root </> "cache")
  let directory = root </> "consumer"
      writer = writerSpec 0xdeadbeef 0xdeadbeef
  rows <- runSpecs cache runner directory [writer, readerSpec 0xdeadbeef, writer]
  case rows of
    [("writer", []), ("reader", differences), ("writer", [])] ->
      ensure (not (null differences)) "parallel consumer discarded a failed expectation"
    _ -> fail ("parallel consumer results differ in order or judgement: " ++ show rows)
  forM_ ([0..2] :: [Int]) $ \index -> do
    present <- doesFileExist (directory </> show index </> "summary.json")
    ensure present "parallel consumer discarded a case summary"
  failure <- doesFileExist (directory </> "1" </> "reader.consumer-failure.json")
  ensure failure "parallel consumer discarded failed evidence"
  counts <- cacheExecutionValue cache
  ensure (get counts "batches_executed" == Number 2 && get counts "batches_reused" == Number 1)
    "parallel cache accounting or duplicate-wire execution differs"
