{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Exception (SomeException, bracket, try)
import Codec.Compression.GZip (decompress)
import Data.Aeson (Value(..), object, toJSON, (.=))
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.String (fromString)
import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef (modifyIORef', newIORef, readIORef)
import Emulation.NativeObservation (GuestAddress(..), Register(..), RegionId(..), RunnerStatus(..))
import Emulation.OwnedCase (CaseSpec(..), CaseRegion(..), Permission(..), mkRegion, word32le)
import Emulation.Sha256 (sha256, sha256File)
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeObservation as N
import FujiAF.Host (get, list, set)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import qualified Replacement.PropertyService.Oracle as O
import Replacement.PropertyService.Compact
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory, removeDirectoryRecursive, removeFile)
import System.FilePath ((</>), takeExtension)
import System.Posix.Files (setFileMode)
import System.Posix.Temp (mkdtemp)
import System.Exit (ExitCode(ExitSuccess))
import System.Process (readProcessWithExitCode)
import Text.Read (readMaybe)

assert :: Bool -> String -> IO ()
assert condition message = unless condition (fail message)

main :: IO ()
main = bracket (mkdtemp "/tmp/property-compact-tests-") removeDirectoryRecursive $ \root -> do
  let runner = root </> "runner"
      marker = root </> "runner-invoked"
  B.writeFile runner ("#!/bin/sh\ncat >/dev/null\nprintf invoked > '" <> bytes marker <> "'\nprintf 'FUJI_NATIVE_BATCH_INVALID fixture refusal\\n'\nexit 1\n")
  setFileMode runner 0o700
  runnerHash <- sha256File runner
  checks <- newIORef []
  let contract = CompactContract (sha256 "source") (sha256 "input") runnerHash
      config = CompactConfig (root </> "output") runner contract 1 Btrfs4096 (\checkpoint -> modifyIORef' checks (checkpoint:))
      plan = PairPlan fixture (const (fixture {caseId="owned"})) Nothing CompactOnly (FamilyName "fixture") MustAgree
  empty <- executeCompactPairs config []
  assert (compactConsumerAgreement empty && compactSessions (compactStats empty) == 0 && compactObservations (compactStats empty) == 0) "empty plan invented observations"
  assert (compactReceiptBytes (compactStats empty) == ByteCount 0 && null (compactArtifacts empty)) "empty plan published receipts"
  assertM (not <$> doesFileExist marker) "empty plan executed runner"
  multiple <- try (executeCompactPairs config {compactChunkPairs=65} [plan]) :: IO (Either SomeException CompactResult)
  assertFailure multiple "out-of-bound chunk was admitted"
  assertM (not <$> doesFileExist marker) "out-of-bound chunk executed runner"
  denied <- try (executeCompactPairs config {compactCheck = \_ -> fail "storage denied"} [plan]) :: IO (Either SomeException CompactResult)
  assertFailure denied "storage rejection produced a result"
  assertM (not <$> doesFileExist marker) "storage rejection executed runner"
  mismatched <- try (executeCompactPairs config {compactContract=contract {contractRunner=sha256 "wrong runner"}} [plan]) :: IO (Either SomeException CompactResult)
  assertFailure mismatched "runner identity mismatch produced a result"
  assertM (not <$> doesFileExist marker) "runner mismatch executed runner"
  refused <- try (executeCompactPairs config [plan]) :: IO (Either SomeException CompactResult)
  assertFailure refused "incomplete native session produced a result"
  assertM (doesFileExist marker) ("refusal fixture was not exercised: " ++ either show (const "unexpected completed result") refused)
  preserved <- retainedJSON (root </> "output")
  assert (any (\value -> get value "status" == toJSON ("invalid_batch"::String)) preserved) "incomplete native diagnostics were discarded"
  assert (any (\value -> get value "id" == toJSON ("stock"::String) && get value "entry" /= Null) preserved) "incomplete native reprocase was discarded"
  checkpoints <- readIORef checks
  assert (any (\checkpoint -> checkpointPhase checkpoint == BeforeChunk && checkpointPendingBytes checkpoint > ByteCount 0) checkpoints) "execution had no failure retention reservation"
  successTests root config
  putStrLn "compact producer tests passed"
  where
    bytes = B.pack . map (fromIntegral . fromEnum)
    assertM action message = action >>= \condition -> assert condition message
    assertFailure (Left _) _ = pure ()
    assertFailure (Right _) message = fail message

fixture :: CaseSpec
fixture = CaseSpec
  { caseId="stock", caseProvenance="compact runner refusal fixture", caseExclusion="bounded fixture"
  , caseEntry=GuestAddress 0x1000, caseExit=GuestAddress 0x1004
  , caseRegions=
      [mkRegion (RegionId "code") (GuestAddress 0x1000) 8 ReadExecute (word32le 0xe12fff1e <> word32le 0xe1a00000)
      ,mkRegion (RegionId "stack") (GuestAddress 0x2000) 8 ReadWrite (B.replicate 8 0)]
  , caseFunctions=[(GuestAddress 0x1000,4)]
  , caseInitial=[(register, case register of Sp -> 0x2008; Lr -> 0x1004; Cpsr -> 0x10; _ -> 0) | register <- N.registerLabels, register /= Pc]
  , caseExpectedStatus=Returned, caseExpectedRegisters=[(R0,0)]
  , caseExpectedMemory=[], caseIgnoredMemory=[], caseCp14Identity=Nothing, caseKernel=Nothing
  }

successTests :: FilePath -> CompactConfig -> IO ()
successTests root config = do
  let specimen = fixture {caseRegions = caseRegions fixture ++
        [mkRegion ident (GuestAddress (fromIntegral base)) size ReadWrite (B.replicate size 0) | (ident,base,size) <- O.comparedRegions]}
      regions = map caseRegion (caseRegions specimen)
      observation = object
        ["wire_version" .= (1::Int), "status" .= ("returned"::String), "unicorn_error" .= (0::Int)
        , "instructions" .= ("1"::String), "memory_provenance_total" .= ("0"::String), "memory_provenance" .= ([]::[Value])
        , "elapsed_ns" .= ("1"::String), "trace_total" .= ("0"::String), "trace" .= ([]::[Value])
        , "access_total" .= ("0"::String), "accesses" .= ([]::[Value])
        , "events" .= ([]::[Value])
        , "registers" .= object [fromString (N.registerLabel register) .= N.hexWord32 (fixtureRegisterValue register) | register <- N.registerLabels]
        , "memory" .= [object ["address" .= N.hexWord32 (fromInteger (C.baseAddress region)), "bytes" .= C.hexBytes (C.regionBytes region)] | region <- regions]]
      fixtureRegisterValue register = case register of Sp -> 0x2008; Lr -> 0x1004; Pc -> 0x1004; Cpsr -> 0x10; _ -> 0
      runner = root </> "success-runner"
      fixturePath = root </> "observation.json"
      invoked = root </> "success-invoked"
  B.writeFile fixturePath (encodeCanonical observation <> "\n")
  BC.writeFile runner (BC.pack ("#!/bin/sh\ncat >/dev/null\nprintf invoked > '" ++ invoked ++ "'\nprintf 'FUJI_NATIVE_BATCH_CASE 0 0\\n'\ncat '" ++ fixturePath ++ "'\nprintf 'FUJI_NATIVE_BATCH_END 1\\n'\n"))
  setFileMode runner 0o700
  digest <- sha256File runner
  admissions <- newIORef []
  let plan spec owned = PairPlan spec (const (spec {caseId=owned})) Nothing CompactOnly (FamilyName "success") MustAgree
      configured = config {compactRunner=runner, compactContract=(compactContract config) {contractRunner=digest}, compactChunkPairs=2
        , compactCheck = \checkpoint -> modifyIORef' admissions (checkpoint:) >> compactCheck config checkpoint}
  result <- executeCompactPairs configured [plan specimen "owned", plan (specimen {caseId="second"}) "owned-second"]
  assert (compactConsumerAgreement result && compactSessions (compactStats result) == 4 && compactObservations (compactStats result) == 4) "successful paired runner results changed"
  baselineChecks <- readIORef admissions
  unless (length (compactArtifacts result) == 1) (fail "successful paired chunk did not publish its aggregate")
  encoded <- fmap B.concat $ forM (compactArtifacts result) $ \artifact ->
    BL.toStrict . decompress . BL.fromStrict <$> B.readFile (compactArtifactPath artifact)
  rows <- mapM (either fail pure . decodeStrictIntegers) (BC.lines encoded)
  case rows of
    [_header,first,second] -> do
      assert (get first "requirement_passed" == toJSON True && get second "requirement_passed" == toJSON True) "aggregate lost pair verdicts"
      let stock = get (get first "pair_receipt") "stock"
      assert (get stock "physical_writes" == toJSON ([]::[Value])) "receipt invented physical writes"
    _ -> fail "aggregate lost behavioral records"
  retained <- retainedJSON (compactSessionDirectory result)
  let rawReports = [value | value <- retained, get value "runner" /= Null]
      reprocaseIds = [get value "id" | value <- retained, get value "entry" /= Null]
  assert (length rawReports == 2 && all (\value -> get value "status" == toJSON ("passed"::String)) rawReports) "full diagnostic representatives were missing or repeated successes were persisted"
  assert (toJSON ("stock"::String) `elem` reprocaseIds && toJSON ("owned"::String) `elem` reprocaseIds) "representative reprocase was discarded"
  assert (toJSON ("second"::String) `notElem` reprocaseIds && toJSON ("owned-second"::String) `notElem` reprocaseIds) "successful ephemeral staging survived publication"
  let changed = set "memory" (toJSON
        [if get row "address" == toJSON ("0x00002000"::String)
          then set "bytes" (toJSON ("0100000000000000"::String)) row else row
        | row <- list (get observation "memory")]) observation
  B.writeFile fixturePath (encodeCanonical changed <> "\n")
  unknown <- executeCompactPairs configured [plan specimen "owned", plan (specimen {caseId="second"}) "owned-second"]
  assert (compactConsumerAgreement unknown && compactObservations (compactStats unknown) == 4) "fully judged SDK-only mismatch prevented paired execution"
  assert (compactSDKFailedObservations (compactStats unknown) == 4 && compactFullFailureReports (compactStats unknown) == 4 && compactUnsupportedSourceBoundsObservations (compactStats unknown) == 4) "SDK failure or unsupported-bounds counts changed"
  let measurement = compactResultValue unknown
  assert (get measurement "measurement_completed" == toJSON True && get measurement "qualification_passed" == toJSON False && get measurement "passed" == Null) "measurement status became qualification acceptance"
  unknownRetained <- retainedJSON (compactSessionDirectory unknown)
  let unknownRaw = [value | value <- unknownRetained, get value "runner" /= Null]
  assert (length unknownRaw == 4) "unknown stack diagnostics were compacted without source-frame proof"
  assert (all (\value -> get value "status" == toJSON ("failed"::String)
      && get value "mismatches" == toJSON (["memory stack: differs from exact allowed final bytes"]::[String])) unknownRaw) "full SDK mismatch diagnostics changed"
  let plans = [plan specimen "owned", plan (specimen {caseId="second"}) "owned-second"]
  B.writeFile fixturePath (encodeCanonical observation <> "\n")
  let bound = maximum [bytes | StorageCheckpoint BeforeChunk (ByteCount bytes) <- baselineChecks]
      cap maximumBytes checkpoint = case checkpoint of
        StorageCheckpoint BeforeChunk (ByteCount pending) -> assert (pending <= maximumBytes) "test cap exceeded"
        _ -> pure ()
  assert (bound >= 6 * (toInteger (C.profileOutputBytes C.DefaultMemory) + toInteger C.batchStreamBytes)) "admission omitted escaped stdout/stderr worst case"
  exact <- executeCompactPairs configured {compactCheck=cap bound} plans
  assert (compactConsumerAgreement exact) "exact admission bound was rejected"
  below <- try (executeCompactPairs configured {compactCheck=cap (bound-1)} plans) :: IO (Either SomeException CompactResult)
  assert (case below of Left _ -> True; Right _ -> False) "below-bound execution returned a passed result"
  removeFile invoked
  B.writeFile fixturePath (encodeCanonical (set "elapsed_ns" Null observation) <> "\n")
  let failureOutput = root </> "callback-failure"
      rejectAfterRunner checkpoint = do
        ran <- doesFileExist invoked
        when (ran && checkpointPhase checkpoint == BeforeWrite) (fail "publication callback denied")
  callbackFailure <- try (executeCompactPairs configured {compactOutput=failureOutput,compactCheck=rejectAfterRunner} plans) :: IO (Either SomeException CompactResult)
  assert (case callbackFailure of Left _ -> True; Right _ -> False) "callback failure returned a passed result"
  emergency <- retainedJSON failureOutput
  assert (any (\value -> get value "runner" /= Null) emergency) "throwing callback discarded current unparsable raw report"
  assert (any (\value -> get value "entry" /= Null) emergency) "throwing callback discarded durable reprocase"
  let binaryRunner = root </> "nul-runner"
      binaryOutput = root </> "nul-output"
  BC.writeFile binaryRunner "#!/bin/sh\ncat >/dev/null\nprintf '\\000\\000' >&2\nprintf 'FUJI_NATIVE_BATCH_CASE 0 1\\n\\000'\nexit 1\n"
  setFileMode binaryRunner 0o700
  binaryHash <- sha256File binaryRunner
  binaryFailure <- try (executeCompactPairs configured
    {compactOutput=binaryOutput,compactRunner=binaryRunner,compactContract=(compactContract configured) {contractRunner=binaryHash}}
    plans) :: IO (Either SomeException CompactResult)
  assert (case binaryFailure of Left _ -> True; Right _ -> False) "binary stderr failure returned a passed result"
  binaryRetained <- retainedJSON binaryOutput
  assert (any (\value -> get value "stderr" == toJSON ("\0\0"::String)) binaryRetained) "NUL-expanding stderr was not retained exactly"
  B.writeFile fixturePath (encodeCanonical observation <> "\n")
  published <- newIORef False
  publicationCeiling <- newIORef Nothing
  let publicationOutput = root </> "publication-failure"
      rejectAfterPublication checkpoint = when (checkpointPhase checkpoint == AfterWrite) $ do
        receipts <- pathsWithExtension ".gz" publicationOutput
        when (not (null receipts)) $ do
          modifyIORef' published (const True)
          used <- retainedBytes publicationOutput
          let ByteCount pending = checkpointPendingBytes checkpoint
          modifyIORef' publicationCeiling (const (Just (used+pending)))
          fail "post-publication callback denied"
  publicationFailure <- try (executeCompactPairs configured
    {compactOutput=publicationOutput,compactCheck=rejectAfterPublication} plans) :: IO (Either SomeException CompactResult)
  assert (case publicationFailure of Left _ -> True; Right _ -> False) "post-publication failure returned a passed result"
  observedPublication <- readIORef published
  assert observedPublication "publication callback did not observe the published aggregate"
  publicationEvidence <- retainedJSON publicationOutput
  assert (any (\value -> get value "id" == toJSON ("second"::String) && get value "entry" /= Null) publicationEvidence) "staging was cleaned before publication completed"
  publicationLimit <- readIORef publicationCeiling
  publicationUsed <- retainedBytes publicationOutput
  assert (maybe False (publicationUsed <=) publicationLimit) "AfterWrite emergency retention exceeded its admitted capacity"
  chunkCeiling <- newIORef Nothing
  let chunkOutput = root </> "after-chunk-failure"
      rejectAfterChunk checkpoint = when (checkpointPhase checkpoint == AfterChunk) $ do
        used <- retainedBytes chunkOutput
        let ByteCount pending = checkpointPendingBytes checkpoint
        modifyIORef' chunkCeiling (const (Just (used+pending)))
        fail "after-chunk callback denied"
  chunkFailure <- try (executeCompactPairs configured
    {compactOutput=chunkOutput,compactCheck=rejectAfterChunk} plans) :: IO (Either SomeException CompactResult)
  assert (case chunkFailure of Left _ -> True; Right _ -> False) "AfterChunk failure returned a result"
  chunkLimit <- readIORef chunkCeiling
  chunkUsed <- retainedBytes chunkOutput
  assert (maybe False (chunkUsed <=) chunkLimit) "AfterChunk emergency retention exceeded its admitted capacity"
  controlTests root configured specimen observation
  crossingTests root configured specimen observation

retainedJSON :: FilePath -> IO [Value]
retainedJSON directory = do
  names <- listDirectory directory
  rows <- forM names $ \name -> do
    let path = directory </> name
    nested <- doesDirectoryExist path
    if nested then retainedJSON path else
      if takeExtension path == ".json" then do
        raw <- B.readFile path
        pure (either (const []) (:[]) (decodeStrictIntegers raw))
      else pure []
  pure (concat rows)

pathsWithExtension :: String -> FilePath -> IO [FilePath]
pathsWithExtension extension directory = do
  names <- listDirectory directory
  paths <- forM names $ \name -> do
    let path = directory </> name
    nested <- doesDirectoryExist path
    if nested then pathsWithExtension extension path
      else pure [path | takeExtension path == extension]
  pure (concat paths)

retainedBytes :: FilePath -> IO Integer
retainedBytes directory = do
  exists <- doesDirectoryExist directory
  if not exists then pure 0 else do
    (code,out,errors) <- readProcessWithExitCode "du" ["-s","-B1","--",directory] ""
    assert (code == ExitSuccess) ("allocated-byte measurement failed: " ++ errors)
    case words out of
      count:_ | Just bytes <- readMaybe count, bytes >= (0::Integer) -> pure bytes
      _ -> fail "allocated-byte measurement returned no valid byte count"

controlTests :: FilePath -> CompactConfig -> CaseSpec -> Value -> IO ()
controlTests root config specimen reference = do
  let runner = root </> "control-runner"
      marker = root </> "control-selected"
      stockPath = root </> "control-stock.json"
      ownedPath = root </> "control-owned.json"
      subject = set "registers" (set "r0" (toJSON ("0x00000001"::String)) (get reference "registers")) reference
      expectation = O.Expectation Returned [(R0,0)] []
      plan = PairPlan specimen (const (specimen {caseId="controlled-owned"})) (Just expectation)
        ExactDiagnosticFull (FamilyName "register-control") (MustDiffer [O.RegisterDiffers R0 0 1])
  B.writeFile stockPath (encodeCanonical reference <> "\n")
  B.writeFile ownedPath (encodeCanonical subject <> "\n")
  BC.writeFile runner (BC.pack ("#!/bin/sh\ncat >/dev/null\nprintf 'FUJI_NATIVE_BATCH_CASE 0 0\\n'\nif test -f '" ++ marker ++ "'; then\ncat '" ++ ownedPath ++ "'\nelse\nprintf selected > '" ++ marker ++ "'\ncat '" ++ stockPath ++ "'\nfi\nprintf 'FUJI_NATIVE_BATCH_END 1\\n'\n"))
  setFileMode runner 0o700
  digest <- sha256File runner
  let configured output = config
        {compactOutput=output,compactRunner=runner,compactContract=(compactContract config) {contractRunner=digest}}
      legitimateOutput = root </> "matching-control"
  legitimate <- executeCompactPairs (configured legitimateOutput) [plan]
  assert (compactConsumerAgreement legitimate && compactFullFailureReports (compactStats legitimate) == 1) "matching declared control was rejected"
  assert (get (compactResultValue legitimate) "qualification_passed" == toJSON False) "control became qualification acceptance"
  full <- retainedJSON legitimateOutput
  assert (any (\value -> get value "mismatches" == toJSON (["register r0: differs"]::[String])) full) "matching control lost its full SDK diagnostic"
  removeFile marker
  let unexplained = set "events" (toJSON [object
        ["address" .= ("0x00001000"::String), "inputs" .= object [], "outputs" .= object []]]) subject
      unknownOutput = root </> "unexplained-control"
  B.writeFile ownedPath (encodeCanonical unexplained <> "\n")
  rejected <- try (executeCompactPairs (configured unknownOutput) [plan]) :: IO (Either SomeException CompactResult)
  assert (case rejected of Left _ -> True; Right _ -> False) "expected public difference admitted an unexplained SDK diagnostic"
  unknownFull <- retainedJSON unknownOutput
  assert (any (\value -> get value "mismatches" == toJSON (["register r0: differs","stub events differ"]::[String])) unknownFull) "unexplained control did not retain the entire full SDK mismatch list"

data StackCrossing = CrossBelowStack | CrossEntrySP | CrossAddressSpace
  deriving (Enum, Bounded)

crossingTests :: FilePath -> CompactConfig -> CaseSpec -> Value -> IO ()
crossingTests root config specimen reference =
  forM_ [minBound .. maxBound] $ \crossing -> do
    let label = case crossing of CrossBelowStack -> "cross-low"; CrossEntrySP -> "cross-sp"; CrossAddressSpace -> "cross-address-space"
        start = case crossing of CrossBelowStack -> 0x1ffe; CrossEntrySP -> 0x2006; CrossAddressSpace -> 0xfffffffe
        written = case crossing of CrossBelowStack -> 0x00010000; _ -> 1
        offset = case crossing of CrossEntrySP -> 6; _ -> 0
        stack = B.replicate offset 0 <> B.singleton 1 <> B.replicate (7-offset) 0
        guards =
          [mkRegion (RegionId "stack-low-guard") (GuestAddress 0x1ff8) 8 ReadWrite (B.replicate 8 0)
          ,mkRegion (RegionId "stack-high-guard") (GuestAddress 0x2008) 8 ReadWrite (B.replicate 8 0)]
        spec = specimen {caseId=label,caseRegions=caseRegions specimen ++ guards}
        event = object
          ["kind" .= ("write"::String), "address" .= N.hexWord32 start, "size" .= (4::Int)
          , "value" .= N.hexWord32 written, "pc" .= ("0x00001000"::String), "instruction" .= ("0xe5800000"::String)]
        memory =
          [if get row "address" == toJSON ("0x00002000"::String) then set "bytes" (toJSON (C.hexBytes stack)) row else row
          | row <- list (get reference "memory")]
          ++ [object ["address" .= N.hexWord32 (fromInteger (C.baseAddress region)), "bytes" .= C.hexBytes (C.regionBytes region)]
             | guard <- guards, let region = caseRegion guard]
        observation = set "memory" (toJSON memory) (set "memory_provenance_total" (toJSON (1::Int))
          (set "memory_provenance" (toJSON [event]) reference))
        expectation = O.Expectation Returned [(R0,0),(Sp,0x2008)]
          [(ident,B.replicate size 0) | (ident,_,size) <- O.comparedRegions]
        plan = PairPlan spec (const (spec {caseId="owned-" ++ label})) (Just expectation)
          CompactOnly (FamilyName "crossing-stack") MustAgree
        runner = root </> label ++ "-runner"
        fixturePath = root </> label ++ ".json"
        output = root </> label ++ "-output"
    B.writeFile fixturePath (encodeCanonical observation <> "\n")
    BC.writeFile runner (BC.pack ("#!/bin/sh\ncat >/dev/null\nprintf 'FUJI_NATIVE_BATCH_CASE 0 0\\n'\ncat '" ++ fixturePath ++ "'\nprintf 'FUJI_NATIVE_BATCH_END 1\\n'\n"))
    setFileMode runner 0o700
    digest <- sha256File runner
    result <- try (executeCompactPairs config
      {compactOutput=output,compactRunner=runner,compactContract=(compactContract config) {contractRunner=digest}}
      [plan]) :: IO (Either SomeException CompactResult)
    assert (case result of Left _ -> True; Right _ -> False) "crossing physical stack write completed measurement"
    retained <- retainedJSON output
    let completeRaw value = get value "mismatches" == toJSON (["memory stack: differs from exact allowed final bytes"]::[String])
          && get (get value "runner") "status" == toJSON ("returned"::String)
          && get (get value "runner") "memory_provenance_total" == toJSON (1::Int)
          && get (get value "runner") "memory_provenance" == toJSON [event]
    assert (any completeRaw retained) "crossing physical write lost full returned provenance and SDK diagnostic"
