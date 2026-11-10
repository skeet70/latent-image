{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module StorageRun (NativeCache, NativeBatchFailure(..), openNativeCache, cacheJobs, cachedBatch, cacheExecutionValue, batchReuseLabel, runSpecs, runCachedSpecs, removeStaged, readResidueProof) where

import Control.DeepSeq (force)
import Control.Exception (AsyncException, Exception, SomeException, evaluate, fromException, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.Aeson (FromJSON(..), ToJSON, Value(..), eitherDecodeStrict', withObject, (.:), object, (.=))
import Data.List (isSuffixOf)
import Emulation.NativeObservation
import qualified Emulation.NativeEvidence as E
import Emulation.CachedNativeBatch (BatchReuse(..), CacheBinding, cacheBindingEntry, cacheBindingManifestSha256, CacheOnlyRunner, ResultPolicy(..), cachedObservedBatchWithBinding, cachedObservedBatchOnly)
import Data.IORef (IORef, newIORef, readIORef, atomicModifyIORef')
import Emulation.ParallelCases (NativeJobs, mapNativeBounded)
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure)
import FujiAF.JSON (encodeCanonical)
import FujiAF.StorageBudget (checkStorageBudget)
import Native (callerFramePreserved)
import System.Directory (createDirectoryIfMissing, doesFileExist, listDirectory, removeFile)
import System.FilePath ((</>))

failures :: CaseSpec -> NativeObservation -> [String]
failures spec observation =
  ["status " ++ show (nativeStatus observation) | nativeStatus observation /= caseExpectedStatus spec]
  ++ ["register " ++ show register | (register,value) <- caseExpectedRegisters spec,lookupRegister register observation /= Just value]
  ++ ["memory " ++ show ident | (ident,bytes) <- caseExpectedMemory spec,fmap (B.take (B.length bytes)) (lookup ident (nativeMemory observation)) /= Just bytes]
  ++ ["caller frame" | not (callerFramePreserved spec observation)]

data NativeCache = NativeCache NativeJobs FilePath (IORef CacheExecution)

cacheJobs :: NativeCache -> NativeJobs
cacheJobs (NativeCache jobs _ _) = jobs
newtype BatchCount = BatchCount Int deriving (Num, ToJSON)
newtype NativeCaseCount = NativeCaseCount Int deriving (Num, ToJSON)
data CacheExecution = CacheExecution
  { batchesExecuted :: !BatchCount
  , batchesReused :: !BatchCount
  , casesExecuted :: !NativeCaseCount
  , casesReused :: !NativeCaseCount
  }

data NativeBatchFailure = NativeBatchFailure CaseId FilePath SomeException deriving Show
instance Exception NativeBatchFailure

openNativeCache :: NativeJobs -> FilePath -> IO NativeCache
openNativeCache jobs root = NativeCache jobs root <$> newIORef (CacheExecution 0 0 0 0)

cachedBatch :: NativeCache -> FilePath -> FilePath -> CaseId -> IO (BatchReuse,[(CaseId,NativeObservation)],Maybe CacheBinding)
cachedBatch cache@(NativeCache _ root _) runner batch identifier =
  recordBatch cache batch identifier (cachedObservedBatchWithBinding WriteFailedResults root runner batch)

recordBatch :: NativeCache -> FilePath -> CaseId -> IO (BatchReuse,[(CaseId,NativeObservation)],Maybe CacheBinding) -> IO (BatchReuse,[(CaseId,NativeObservation)],Maybe CacheBinding)
recordBatch (NativeCache _ _ counts) batch identifier action = do
  attempt <- try action
  result@(reuse,observations,_binding) <- case attempt of
    Right observed -> pure observed
    Left exception
      | Just asynchronous <- (fromException exception :: Maybe AsyncException) -> throwIO asynchronous
      | otherwise -> throwIO (NativeBatchFailure identifier batch exception)
  let count=NativeCaseCount (length observations)
  atomicModifyIORef' counts $ \current -> (,()) $ case reuse of
    BatchExecuted -> current {batchesExecuted=batchesExecuted current+1,casesExecuted=casesExecuted current+count}
    BatchReused -> current {batchesReused=batchesReused current+1,casesReused=casesReused current+count}
  pure result

batchReuseLabel :: BatchReuse -> String
batchReuseLabel BatchExecuted = "executed"
batchReuseLabel BatchReused = "reused"

cacheExecutionValue :: NativeCache -> IO Value
cacheExecutionValue (NativeCache _ _ counts) = do
  current <- readIORef counts
  pure (object
    ["batches_executed" .= batchesExecuted current,"batches_reused" .= batchesReused current
    ,"cases_executed" .= casesExecuted current,"cases_reused" .= casesReused current])

runSpecs :: NativeCache -> FilePath -> FilePath -> [CaseSpec] -> IO [(String,[String])]
runSpecs cache runner = runSpecsUsing (cacheJobs cache) (cachedBatch cache runner)

runCachedSpecs :: NativeCache -> CacheOnlyRunner -> FilePath -> [CaseSpec] -> IO [(String,[String])]
runCachedSpecs cache@(NativeCache jobs root _) runner =
  runSpecsUsing jobs (\batch identifier -> recordBatch cache batch identifier
    (cachedObservedBatchOnly WriteFailedResults root runner batch))

runSpecsUsing :: NativeJobs -> (FilePath -> CaseId -> IO (BatchReuse,[(CaseId,NativeObservation)],Maybe CacheBinding)) -> FilePath -> [CaseSpec] -> IO [(String,[String])]
runSpecsUsing jobs observe directory specs = do
  createDirectoryIfMissing True directory
  E.withInputPool directory $ \pool -> fmap concat $ mapNativeBounded jobs (\(index,spec) -> do
    checkStorageBudget directory
    let batchDirectory = directory </> show index
        part = [spec]
    batch <- writeBatchWithInputPool pool batchDirectory part
    (reuse,observed,binding) <- observe batch (CaseId (caseId spec))
    ensure (length part == length observed) "native observation count differs"
    rows <- forM (zip part observed) $ \(observedSpec,(CaseId label,observation)) -> do
      ensure (label == caseId observedSpec) "native observation order differs"
      pure (label,failures observedSpec observation)
    compact <- evaluate (force rows)
    let failureRows = [(label,problems) | (label,problems) <- compact, not (null problems)]
        rawCache = case binding of
          Just cached -> object ["entry" .= cacheBindingEntry cached,"manifest_sha256" .= sha256Hex (cacheBindingManifestSha256 cached)]
          Nothing -> Null
        firstReports = [batchDirectory </> label ++ ".native-result.json" | (label,_) <- take 1 failureRows]
        consumerReports = [batchDirectory </> label ++ ".consumer-failure.json" | (label,_) <- failureRows]
    forM_ failureRows $ \(label,problems) -> do
      let resultPath = batchDirectory </> label ++ ".native-result.json"
          failureReport = encodeCanonical (object
            ["schema_version" .= (1::Int),"case" .= label,"status" .= ("failed"::String)
            ,"differences" .= problems,"raw_cache" .= rawCache])
      present <- doesFileExist resultPath
      unless present (B.writeFile resultPath failureReport)
      B.writeFile (batchDirectory </> label ++ ".consumer-failure.json") failureReport
    batchBytes <- B.readFile batch
    B.writeFile (batchDirectory </> "summary.json") (encodeCanonical (object
      ["batch_sha256" .= sha256Hex (sha256 batchBytes),"execution" .= batchReuseLabel reuse,"raw_cache" .= rawCache
      ,"cases" .= [object ["id" .= label,"status" .= runnerStatusLabel (nativeStatus observation),"differences" .= problems] | ((label,problems),(_,observation)) <- zip compact observed]
      ,"first_failure_reports" .= firstReports,"consumer_failure_reports" .= consumerReports]))
    when (null failureRows) $ case binding of
      Just _ -> removeStaged batchDirectory
      Nothing -> fail "successful case has no immutable raw-cache binding"
    pure compact) (zip [0::Int ..] specs)

-- The raw cache entry and summary retain successful evidence; only failed or incomplete cases
-- keep their staged wire and result artifacts. Removal errors are surfaced to the caller.
removeStaged :: FilePath -> IO ()
removeStaged directory = do
  names <- listDirectory directory
  forM_ names $ \name ->
    when (name == "batch.json" || ".case.json" `isSuffixOf` name || ".bin" `isSuffixOf` name || ".native-result.json" `isSuffixOf` name) $
      removeFile (directory </> name)

data ResidueProof = ResidueProof

instance FromJSON ResidueProof where
  parseJSON = withObject "full-export control receipt" $ \receipt -> do
    control :: String <- receipt .: "control"
    test :: String <- receipt .: "test"
    correct :: Int <- receipt .: "correct_exit"
    mutant :: Int <- receipt .: "mutant_exit"
    unless (control == "full-export-tail-zero-invented" && not (null test) && correct == 0 && mutant /= 0) (fail "invalid full-export control evidence")
    pure ResidueProof

readResidueProof :: [String] -> IO (Maybe String)
readResidueProof arguments = case dropWhile (/= "--full-export-control-result") arguments of
  [] -> pure Nothing
  (_:path:_) -> do
    bytes <- B.readFile path
    ResidueProof <- either fail pure (eitherDecodeStrict' bytes)
    pure (Just (sha256Hex (sha256 bytes)))
  _ -> fail "missing full-export control receipt path"
