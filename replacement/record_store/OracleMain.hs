{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.DeepSeq (force)
import Control.Exception (evaluate)
import Control.Monad
import Data.Aeson (Value(..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString as B
import Data.List (isInfixOf, isPrefixOf, nub)
import qualified Data.Map.Strict as M
import Emulation.NativeCase (Region(..), BatchEntry(..), loadCase, readBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase hiding (caseId)
import qualified Emulation.OwnedCase as O
import Emulation.Sha256
import FujiAF.Host (get)
import FujiAF.JSON (encodeCanonical)
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Hardware
import Replacement.RecordStore.Harness
import Replacement.RecordStore.Model
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
import Text.Read (readMaybe)

pinned :: FilePath -> String -> IO B.ByteString
pinned path expected = do
  bytes <- B.readFile path
  unless (sha256Hex (sha256 bytes) == expected) (error (path ++ ": SHA-256 mismatch"))
  pure bytes

indexPath :: FilePath
indexPath = "private-data/scratch/xm1-decomp/index/55436fc85c0560dd252b0410af6f2427fe198cd3-31e27c0e12fe.tsv"
readFunctions :: IO [Function]
readFunctions = do
  rows <- lines <$> readFile indexPath
  let parsed = [Function address size | row <- rows, ("range":_:_:_:start:lengthBytes:_) <- [words row], Just address <- [readMaybe start], Just size <- [readMaybe lengthBytes], address >= 0x61c400, address < 0x89ec28]
      outside (Function address _) = address < 0x89aa90 || address >= 0x89abe0
  pure (Function 0x89aa90 0x128 : Function 0x89abb8 0x20 : filter outside parsed)

chunks :: Int -> [a] -> [[a]]
chunks _ [] = []
chunks size items = let (first,rest) = splitAt size items in first : chunks size rest

patchStubs :: FilePath -> [CaseSpec] -> IO ()
patchStubs directory specs = forM_ specs $ \spec -> do
  let path = directory </> O.caseId spec ++ ".case.json"
  bytes <- B.readFile path
  value <- either error pure (eitherDecodeStrict' bytes)
  let stubs = [object ["address" .= hexWord32 address, "inputs" .= object [], "outputs" .= object ["r0" .= hexWord32 0], "side_effects" .= ("registers-only" :: String)] | address <- [0x724b68,0x724c00,0x42634,0x4263c]]
  case value of
    Object fields -> B.writeFile path (encodeCanonical (Object (K.insert "limits" (object ["instructions" .= (4000000::Int), "wall_ms" .= (10000::Int)]) (K.insert "stubs" (toJSON stubs) fields))))
    _ -> error "case object"

firstDifference :: B.ByteString -> B.ByteString -> Maybe Value
firstDifference expected actual
  | expected == actual = Nothing
  | otherwise = Just $ object ["offset" .= offset, "expected" .= byte expected, "actual" .= byte actual, "expected_size" .= B.length expected, "actual_size" .= B.length actual]
  where
    offset = length (takeWhile id (B.zipWith (==) expected actual))
    byte bytes = if offset < B.length bytes then Just (B.index bytes offset) else Nothing

boundaryBytes :: NativeObservation -> RegionId -> B.ByteString
boundaryBytes observation ident =
  let bytes = regionBytesOf observation ident
  in if ident == RegionId "results" then B.take resultSize bytes else bytes

modelDifferences :: OracleCase -> NativeObservation -> [Value]
modelDifferences c observed =
  [object ["observation" .= ("status" :: String), "expected" .= runnerStatusLabel expectedRunStatus, "actual" .= runnerStatusLabel (nativeStatus observed), "pc" .= hexWord32 (registerValue observed Pc)] | nativeStatus observed /= expectedRunStatus]
  ++ [object ["observation" .= regionName region, "difference" .= difference] | (region,bytes) <- expectedRegions, Just difference <- [firstDifference bytes (boundaryBytes observed region)]]
  ++ [object ["observation" .= registerLabel register, "expected" .= hexWord32 value, "actual" .= hexWord32 (registerValue observed register)] | not interrupted, (register,value) <- preserved, registerValue observed register /= value]
  where
    expected = model hardware c
    state = expectedStore expected
    p = persistence state
    interrupted = stopped p
    expectedRunStatus = if interrupted then MemoryGuard else Returned
    requestBytes = B.concat (map encodeRequest (requests p))
    cbBytes = B.concat (map encodeCallback (callbackEvents state))
    expectedRegions = [(RegionId "results",expectedResults expected),(RegionId "caller",expectedCaller expected),(RegionId "medium",expectedMedium c expected),(RegionId "source",sourceBytes (geometry (kind c))),(RegionId "requests",requestBytes <> B.replicate (requestSize-B.length requestBytes) 0),(RegionId "callbacks",cbBytes <> B.replicate (callbackSize-B.length cbBytes) 0)]
    preserved = (Sp,stackTop) : zip [R4,R5,R6,R7,R8,R9,R10,R11] [0x44440004..0x4444000b]

compareRoles :: NativeObservation -> NativeObservation -> [Value]
compareRoles stock candidate =
  [object ["observation" .= ("status" :: String), "stock" .= runnerStatusLabel (nativeStatus stock), "candidate" .= runnerStatusLabel (nativeStatus candidate)] | nativeStatus stock /= nativeStatus candidate]
  ++ [object ["observation" .= name', "difference" .= difference] | name' <- ["results","caller","medium","source","requests","callbacks"], Just difference <- [firstDifference (boundaryBytes stock (RegionId name')) (boundaryBytes candidate (RegionId name'))]]
  ++ [object ["observation" .= registerLabel register] | nativeStatus stock == Returned, register <- [R4,R5,R6,R7,R8,R9,R10,R11,Sp], registerValue stock register /= registerValue candidate register]

candidateSpec :: B.ByteString -> [Function] -> CaseSpec -> CaseSpec
candidateSpec code functions spec = spec { caseRegions = map patch (caseRegions spec), caseFunctions = [(address,size) | (address,size) <- caseFunctions spec, guestAddress address < 0x6ac2b4 || guestAddress address >= 0x6ae114] ++ [(GuestAddress (fromIntegral address),size) | Function address size <- functions] }
  where
    patch region | regionName (regionIdOf (caseRegion region)) == "record-code" =
      let old = caseRegion region
          bytes = replace (0x6ac2b4-0x6a6b74) code (regionBytes old)
      in mkRegion (RegionId "record-code") (GuestAddress 0x6a6b74) (B.length bytes) ReadExecute bytes
                 | otherwise = region

symbolFunctions :: FilePath -> IO [Function]
symbolFunctions path = do
  rows <- lines <$> readFile path
  pure (nub [Function (parseAddress "symbol address" ("0x"++address)) (parseAddress "symbol size" ("0x"++size)) | row <- rows, [address,size,kind',_] <- [words row], kind' `elem` ["T","t"], size /= "00000000", size /= "0"])

main :: IO ()
main = do
  args <- getArgs
  runner <- traverse makeAbsolute (optional args "--runner")
  outputPath <- required args "--out" >>= makeAbsolute
  candidate <- traverse makeAbsolute (optional args "--candidate")
  candidateSymbols <- traverse makeAbsolute (optional args "--symbols")
  cachedStock <- traverse makeAbsolute (optional args "--stock-output")
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  createDirectoryIfMissing True outputPath
  image <- pinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  loader <- pinned "private-data/analysis/x-m1-1.50/decode/loader-40000.bin" "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  area2a <- pinned "private-data/sessions/20261009T030736Z-read-media/bank-0.bin" "1bedd3273d3fe7ec5a3b17d732b9f63e2420db61074290bf636a5b91bfa0c072"
  area2b <- pinned "private-data/sessions/20261009T030736Z-read-media/bank-1.bin" "924ed517da238a81c5e61b2ecd17ecb76250b67236921be7bdc711a4509635c7"
  area3 <- pinned "private-data/analysis/x-m1-1.50/native-case/area3-validator/record-accept.bin" "87b1e44d39a7ad591ccae07e953f6c8e11b2eb3e0e118eabac44bc2c0976f34b"
  ranges <- readFunctions
  let recover c = c {name = CaseName (caseName (name c) ++ "-recover"), initialMedium = medium (persistence (expectedStore (model hardware c))), operations = [Open,Register Normal,Latest], injection = NoFault}
      recoveryCases = [recover c | c <- caseGrid, StopAfter _ <- [injection c]]
      allCases = caseGrid ++ recoveryCases ++ hardwareCases area2a area2b area3
      selected = maybe allCases (\pattern' -> filter (isInfixOf pattern' . caseName . name) allCases) (optional args "--filter")
      specs = map (caseSpec image loader ranges) selected
  when (null selected) (error "no cases selected")
  candidateInputs <- case (candidate,candidateSymbols) of
    (Nothing,Nothing) -> pure Nothing
    (Just bin,Just symbols) -> do bytes <- B.readFile bin; functions <- symbolFunctions symbols; unless (B.length bytes == 0x1e60) (error "candidate span size"); pure (Just (bytes,functions))
    _ -> error "--candidate and --symbols are required together"
  when ("--control" `elem` args && candidateInputs == Nothing) (error "--control requires a candidate")
  cachedInputs <- case cachedStock of
    Nothing -> pure M.empty
    Just root -> do
      directories <- filter (isPrefixOf "stock-") <$> listDirectory root
      entries <- concat <$> mapM (fmap snd . readBatch . (\directory -> root </> directory </> "batch.json")) directories
      let inputs = M.fromList [(entryId entry,entryPath entry) | entry <- entries]
      unless (M.size inputs == length entries) (error "duplicate cached stock case")
      pure inputs
  batches <- forM (zip [0::Int ..] (chunks 16 (zip selected specs))) $ \(batchIndex,rows) -> do
    let directory = outputPath </> "stock-" ++ show batchIndex
    batch <- writeBatch directory (map snd rows)
    patchStubs directory (map snd rows)
    candidateBatch <- forM candidateInputs $ \(bytes,functions) -> do
      let candidateSpecs = map (candidateSpec bytes functions . snd) rows
          candidateDirectory = outputPath </> "candidate-" ++ show batchIndex
      path <- writeBatch candidateDirectory candidateSpecs
      patchStubs candidateDirectory candidateSpecs
      pure (path,candidateSpecs)
    case runner of
      Nothing -> do
        let inputs = (directory,map snd rows) : [(takeDirectory path,candidateSpecs) | (path,candidateSpecs) <- maybe [] pure candidateBatch]
        forM_ inputs $ \(inputDirectory,inputSpecs) -> forM_ inputSpecs $ \spec -> void (loadCase (inputDirectory </> O.caseId spec ++ ".case.json"))
        pure []
      Just executable -> do
        observed <- case cachedStock of
          Nothing -> loadObservedBatch executable batch
          Just _ -> forM (map snd rows) $ \spec -> do
            let stem = O.caseId spec
                originalCase = M.findWithDefault (error ("cached stock case missing: " ++ stem)) (CaseId stem) cachedInputs
                originalDirectory = takeDirectory originalCase
                generatedCase = directory </> stem ++ ".case.json"
            original <- B.readFile originalCase >>= either error pure . executionInput
            generated <- B.readFile generatedCase >>= either error pure . executionInput
            unless (original == generated) (error ("cached stock input differs: " ++ stem))
            (_,regions,_,_) <- loadCase generatedCase
            report <- B.readFile (originalDirectory </> stem ++ ".native-result.json") >>= either error pure . eitherDecodeStrict'
            exit <- either (error . renderObservationError) pure (reportRunnerExit report)
            observation <- either (error . renderObservationError) pure (observeNative exit regions (get report "runner"))
            pure (CaseId stem,observation)
        candidateObserved <- case candidateBatch of
          Nothing -> pure M.empty
          Just (path,_) -> M.fromList <$> loadObservedBatch executable path
        compared <- forM (zip rows observed) $ \((c,_),(ident,observation)) -> do
          let differences = modelDifferences c observation
              candidateDifferences = maybe [] (compareRoles observation) (M.lookup ident candidateObserved)
          evaluate (force (object ["case" .= caseName (name c), "stock_status" .= runnerStatusLabel (nativeStatus observation), "model_differences" .= differences, "candidate_differences" .= candidateDifferences], not (null differences), not (null candidateDifferences)))
        B.writeFile (outputPath </> "batch-" ++ show batchIndex ++ ".json") (encodeCanonical (object ["results" .= [row | (row,_,_) <- compared]]))
        pure compared
  let rows = concat batches
      modelFailures = length [() | (_,failed,_) <- rows, failed]
      candidateFailures = length [() | (_,_,failed) <- rows, failed]
      controlMode = "--control" `elem` args
      failures = modelFailures + if controlMode then (if candidateFailures == 0 then 1 else 0) else candidateFailures
      summary = object ["schema_version" .= (1::Int),"profile" .= ("native-cortex-r5f"::String),"image4_sha256" .= sha256Hex (sha256 image),"loader_sha256" .= sha256Hex (sha256 loader),"candidate_sha256" .= fmap (sha256Hex . sha256 . fst) candidateInputs,"cases" .= length selected,"executed" .= length rows,"failed" .= failures,"model_failed" .= modelFailures,"candidate_failed" .= candidateFailures,"control" .= controlMode,"control_detected" .= (controlMode && candidateFailures > 0),"candidate" .= maybe False (const True) candidateInputs,"results" .= [row | (row,_,_) <- rows]]
  B.writeFile (outputPath </> "summary.json") (encodeCanonical summary)
  putStrLn ("cases=" ++ show (length selected) ++ " executed=" ++ show (length rows) ++ " failed=" ++ show failures)
  when (failures > 0) exitFailure
  where
    optional arguments key = case dropWhile (/=key) arguments of _:value:_ -> Just value; _ -> Nothing

executionInput :: B.ByteString -> Either String Value
executionInput bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object fields -> Right (Object (K.delete "expected" fields))
    _ -> Left "case must be an object"
