{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Monad (filterM, forM_)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (isInfixOf, isPrefixOf)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase
  ( CaseId (..)
  , LoadedCase (..)
  , loadCaseWith
  , readArtifact
  )
import qualified Emulation.NativeObservation as N
import qualified Emulation.OwnedCase as O
import FujiAF.Host (ensure, get, jsonRead, list, str, withTemporary)
import FujiModPaths (fujimodNativeCase)
import System.Directory
  ( Permissions (..)
  , canonicalizePath
  , copyFile
  , createDirectoryIfMissing
  , doesDirectoryExist
  , getPermissions
  , listDirectory
  , removeFile
  , setPermissions
  )
import System.Posix.Files (createSymbolicLink)
import System.FilePath ((</>))

main :: IO ()
main = do
  canonicalRunner <- canonicalizePath fujimodNativeCase
  withTemporary "cached-native-batch-tests" $ \root -> do
    reuseTest (root </> "reuse") canonicalRunner
    expectedRejudgeTest (root </> "expected") canonicalRunner
    wireChangeTest (root </> "wire") canonicalRunner
    labelChangeTest (root </> "label") canonicalRunner
    runnerIdentityTest (root </> "runner") canonicalRunner
    corruptionTest (root </> "corruption") canonicalRunner
    interruptedTest (root </> "interrupted") canonicalRunner
    faultyTest (root </> "faulty") canonicalRunner

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

reuseTest :: FilePath -> FilePath -> IO ()
reuseTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  before <- B.readFile batchPath
  (firstReuse, first) <- cachedObservedBatch cache runner batchPath
  after <- B.readFile batchPath
  ensure (firstReuse == BatchExecuted) ("first call did not execute: " ++ show firstReuse)
  ensure (before == after) "cachedObservedBatch modified batch.json"
  ensure (map fst first == caseIds cases) ("observation order differs: " ++ show (map (caseIdText . fst) first))
  ensure (all ((== N.Returned) . N.nativeStatus . snd) first) ("unexpected status: " ++ show first)
  forM_ (zip cases first) $ \(caseSpec, (_, observation)) ->
    checkResultFile batchDirectory caseSpec observation "passed"
  (secondReuse, second) <- cachedObservedBatch cache runner batchPath
  ensure (secondReuse == BatchReused) ("second call did not reuse: " ++ show secondReuse)
  ensure (second == first) "cached observation differs from the executed observation"

expectedRejudgeTest :: FilePath -> FilePath -> IO ()
expectedRejudgeTest root runner = do
  let cache = root </> "cache"
      batchDirectory = root </> "batch"
      cases = specs 0xdeadbeef 0xdeadbeef 0x11223344
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  let changed = specs 0xdeadbeef 0xdeadbeee 0x11223344
  _ <- O.writeBatch batchDirectory changed
  (reused, rejudged) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) ("changed expectation did not reuse raw output: " ++ show reused)
  ensure (rejudged == baseline) "changed expectation changed the raw observation"
  checkResultFiles batchDirectory changed rejudged ["failed", "passed"]
  _ <- O.writeBatch batchDirectory cases
  (again, restored) <- cachedObservedBatch cache runner batchPath
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
  batchPath <- O.writeBatch batchDirectory cases
  (executed, baseline) <- cachedObservedBatch cache runner batchPath
  ensure (executed == BatchExecuted) "baseline did not execute"
  (reused, _) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) "unchanged labels did not reuse"
  _ <- O.writeBatch batchDirectory renamed
  (renamedReuse, renamedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (renamedReuse == BatchReused) ("renamed cases did not reuse: " ++ show renamedReuse)
  ensure (map fst renamedObservations == caseIds renamed) "renamed observation ids differ"
  ensure (map snd renamedObservations == map snd baseline) "renamed cases changed the raw observations"
  forM_ (zip renamed renamedObservations) $ \(caseSpec, (_, observation)) ->
    checkResultFile batchDirectory caseSpec observation "passed"
  entries <- cacheEntries cache
  ensure (length entries == 1) ("renaming cases created extra cache entries: " ++ show entries)
  _ <- O.writeBatch batchDirectory cases
  (restoredReuse, restoredObservations) <- cachedObservedBatch cache runner batchPath
  ensure (restoredReuse == BatchReused) "restored labels did not reuse"
  ensure (restoredObservations == baseline) "restored labels changed the raw observations"

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
  B.writeFile (entry </> "cases" </> "writer" </> "runner.json") (BC.pack "{}")
  (rawCorrupt, rawObservations) <- cachedObservedBatch cache runner batchPath
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
  readerRaw <- B.readFile (substitutedEntry </> "cases" </> "reader" </> "runner.json")
  B.writeFile (substitutedEntry </> "cases" </> "writer" </> "runner.json") readerRaw
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
  failed <- B.readFile (batchDirectory </> "faulty.native-result.json") >>= jsonRead
  ensure
    (any ("register r0" `isInfixOf`) (map str (list (get failed "mismatches"))))
    ("faulty result lacks actionable mismatch: " ++ show failed)
  (reused, reusedObservations) <- cachedObservedBatch cache runner batchPath
  ensure (reused == BatchReused) ("faulty batch was not reusable: " ++ show reused)
  ensure (reusedObservations == observations) "faulty batch reuse changed the observation"
  checkResultFiles batchDirectory cases reusedObservations ["failed"]

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
