{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Runs the stock getter 0x61e110 and setter 0x624694 from the canonical image4 and from the staged
-- candidate image4 for every property id, checks the stock runs of owned ids against the owned
-- model, compares the candidate with stock case by case and runs the guest controls.
module Main (main) where

import qualified A32 as A
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing, listToMaybe)
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.NativeObservation (Register (..), RunnerStatus (..), runnerStatusLabel)
import Emulation.OwnedCase (CaseId (..), CaseSpec, loadObservedBatch, required, writeBatch)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.Host (ensure, failH, get, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Admission (FunctionExtent (..))
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.ClassRecord (reasonLabel)
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding (EncodedEntry (..), encodeClass)
import Replacement.PropertyService.Oracle
import qualified Replacement.PropertyService.Placement as P
import ServiceInventory (Image (..))
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory, takeFileName, (</>))

canonicalImage4Sha256 :: String
canonicalImage4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

batchLimit :: Int
batchLimit = 4096

-- | The property whose getter-exit adjustment at 0x6245a0 reads only mapped bytes and selects on r12.
exitAdjustedProbe :: PropertyId
exitAdjustedProbe = PropertyId 0xe9

data BatchRecord = BatchRecord
  { recordName :: String
  , recordCases :: Int
  , recordBatchSha :: String
  , recordSummarySha :: String
  }

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

-- | One runner session over at most 'batchLimit' cases in @directory@, its runs keyed by case label.
runBatch :: FilePath -> FilePath -> [CaseSpec] -> IO (BatchRecord, M.Map CaseLabel Run)
runBatch runner directory specs = do
  ensure (length specs <= batchLimit) "batch over the case limit"
  batchPath <- writeBatch directory specs
  observed <- loadObservedBatch runner batchPath
  ensure (length observed == length specs) (batchPath ++ ": observation count differs from the case count")
  rows <- forM observed $ \(CaseId label, observation) -> do
    let reportPath = directory </> label ++ ".native-result.json"
    reportBytes <- B.readFile reportPath
    report <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (eitherDecodeStrict' reportBytes :: Either String Value)
    writes <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (reportWrites report)
    run <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (runOf observation writes)
    forced <- evaluate (forceRun run)
    pure (CaseLabel label, forced, sha256Hex (sha256 reportBytes))
  batchBytes <- B.readFile batchPath
  let batchSha = sha256Hex (sha256 batchBytes)
      summary = encodeCanonical $ object
        [ "batch_sha256" .= batchSha
        , "cases" .= [object ["id" .= label, "status" .= runnerStatusLabel (runStatus run), "result_sha256" .= digest] | (CaseLabel label, run, digest) <- rows]
        ]
  B.writeFile (directory </> "summary.json") summary
  pure (BatchRecord (takeFileName directory) (length specs) batchSha (sha256Hex (sha256 summary)), M.fromList [(label, run) | (label, run, _) <- rows])

runBatches :: FilePath -> FilePath -> String -> [CaseSpec] -> IO ([BatchRecord], M.Map CaseLabel Run)
runBatches runner out prefix specs = do
  results <- forM (zip [0 :: Int ..] (chunks batchLimit specs)) $ \(index, chunk) ->
    runBatch runner (out </> prefix ++ "-" ++ (if index < 10 then "0" else "") ++ show index) chunk
  pure (map fst results, M.unions (map snd results))

lookupRun :: M.Map CaseLabel Run -> CaseLabel -> IO Run
lookupRun runs label = maybe (failH ("no run for " ++ caseLabelText label)) pure (M.lookup label runs)

prefixed :: String -> OracleCase -> CaseLabel
prefixed prefix oracleCase = CaseLabel (prefix ++ "-" ++ caseLabelText (caseLabel oracleCase))

classLabel :: Standing -> String
classLabel standing = case standing of
  Classified (Owned (OwnedField _)) -> "owned-field"
  Classified (Owned OwnedUnassigned) -> "owned-unassigned"
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

stockSpec :: B.ByteString -> M.Map PropertyId Property -> CaseLabel -> [FunctionSpan] -> OracleCase -> CaseSpec
stockSpec image properties label functions oracleCase =
  caseSpec image functions fixture label standing oracleCase (maybe placeholderExpectation (\owned -> modelExpectation fixture owned (oracleOperation oracleCase)) (modelled standing))
  where
    standing = standingOf properties (oracleProperty oracleCase)

-- | Decodes each control word with the independent A32 decoder before it is used.
checkEncodings :: Word32 -> Word32 -> Word32 -> Word32 -> IO ()
checkEncodings getterStub setterStub setterExitAddress armTarget = do
  let always = A.Cond 14
      branchSite = setterStub + 0x14
  ensure (A.decode (getterStub + 0xc) movR12FromR4 == A.DataProcessing always A.MOV False (A.Reg 12) (A.Reg 0) (A.ShiftedImmediate (A.Reg 4) A.LSL 0)) "mov r12, r4 encoding"
  ensure (A.decode (getterStub + 0xc) movR12Zero == A.DataProcessing always A.MOV False (A.Reg 12) (A.Reg 0) (A.Immediate 0)) "mov r12, #0 encoding"
  ensure (A.decode branchSite (branchWord branchSite setterExitAddress) == A.Branch always False setterExitAddress) "setter exit branch encoding"
  ensure (A.decode branchSite (branchWord branchSite armTarget) == A.Branch always False armTarget) "arm-1 branch encoding"
  ensure (A.decode getterStub movR4FromR12 == A.DataProcessing always A.MOV False (A.Reg 4) (A.Reg 0) (A.ShiftedImmediate (A.Reg 12) A.LSL 0)) "mov r4, r12 encoding"
  ensure (A.decode getterStub movR6FromR12 == A.DataProcessing always A.MOV False (A.Reg 6) (A.Reg 0) (A.ShiftedImmediate (A.Reg 12) A.LSL 0)) "mov r6, r12 encoding"
  ensure (A.decode (getterStub + 0xc) movR12FromR6 == A.DataProcessing always A.MOV False (A.Reg 12) (A.Reg 0) (A.ShiftedImmediate (A.Reg 6) A.LSL 0)) "mov r12, r6 encoding"

-- | Whether a control's patched image must fail a case or pass every case.
data ControlKind = MustFail | MustPass deriving (Eq)

data Control = Control
  { controlName :: String
  , controlKind :: ControlKind
  , controlPatches :: [Patch]
  , controlCases :: [OracleCase]
  , controlNote :: String
  }

controlHolds :: Control -> Int -> Bool
controlHolds control failed = case controlKind control of
  MustFail -> failed > 0
  MustPass -> failed == 0

main :: IO ()
main = do
  arguments <- getArgs
  runner <- required arguments "--runner" >>= makeAbsolute
  bundled <- if "--smoke" `elem` arguments then pure Nothing else Just <$> mapM (\name -> required arguments name >>= makeAbsolute) ["--linked", "--symbols"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  case bundled of
    Nothing -> smoke arguments runner
    Just [linkedPath, symbolsPath] -> oracle arguments runner linkedPath symbolsPath
    Just _ -> failH "unreachable argument shape"

freshOutput :: FilePath -> IO FilePath
freshOutput path = do
  exists <- doesPathExist path
  when exists (failH (path ++ " already exists"))
  absolute <- makeAbsolute path
  createDirectoryIfMissing True (takeDirectory absolute)
  createDirectory absolute
  pure absolute

oracle :: [String] -> FilePath -> FilePath -> FilePath -> IO ()
oracle arguments runner linkedPath symbolsPath = do
  let input name = required arguments name
  image4Path <- input "--image4"
  candidatePath <- input "--candidate-image4"
  contractPath <- input "--contract"
  sourcePaths <- mapM input ["--dat", "--loader-a", "--loader-b", "--image5", "--plain", "--image2"]
  outPath <- input "--out"
  scratch <- makeAbsolute "private-data/scratch/PropertyService" >>= canonicalizePath
  absoluteOut <- makeAbsolute outPath >>= canonicalizePath
  ensure ((scratch ++ "/") `isPrefixOf` absoluteOut) "--out must be a fresh directory under private-data/scratch/PropertyService"
  image4 <- readRegular 0x300000 image4Path
  candidate <- readRegular 0x300000 candidatePath
  ensure (sha256Hex (sha256 image4) == canonicalImage4Sha256) "--image4 is not the canonical X-M1 1.50 image4"
  [dat, loaderA, loaderB, image5, plain, image2] <- forM (zip [64 * 1024 * 1024, 0x20000, 0x20000, 0x800000, 0x800000, 0x800000] sourcePaths) (uncurry readRegular)
  linked <- readRegular 0x2000 linkedPath
  symbols <- readRegular 0x10000 symbolsPath
  contractBytes <- readRegular 0x1000000 contractPath
  contract <- either (\problem -> failH (contractPath ++ ": " ++ problem)) pure (eitherDecodeStrict' contractBytes :: Either String Value)
  let candidateSha = sha256Hex (sha256 candidate)
      contractSha = str (get contract "staged_image4_sha256")
  ensure (candidateSha == contractSha) ("candidate image4 sha256 " ++ candidateSha ++ " differs from the contract's staged_image4_sha256 " ++ contractSha)
  source <- orFail "canonical sources" (canonicalSource dat image4 loaderA loaderB image5 plain image2)
  let classified = sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
  artifact <- orFail "linked artifact" (deriveArtifact classified linked symbols)
  rederived <- orFail "staging" (stagedImage4Bytes source artifact)
  ensure (rederived == candidate) "candidate image4 differs from the image4 re-derived from the canonical sources and the linked artifact"
  out <- freshOutput outPath
  let P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) = artifactCarrier artifact
      P.StubAddresses (P.GuestAddress getterStub) (P.GuestAddress setterStub) = artifactStubs artifact
      P.GuestAddress tableAddress = artifactTable artifact
      P.GuestAddress setterExitAddress = P.setterExit
      P.GuestAddress getterTable = P.getterTableBase
      P.GuestAddress setterTable = P.setterTableBase
      ownedSpans = [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions artifact]
      candidateFunctions = splitFunctions (carrierStart, carrierEnd) ownedSpans
      standing identifier = standingOf properties identifier
  armOne <- maybe (failH "property 1 missing") (\(Property _ _ (GuestAddress arm) _) -> pure arm) (M.lookup (PropertyId 1) properties)
  checkEncodings getterStub setterStub setterExitAddress armOne

  -- Pass 1: stock.
  (stockBatches, stockRuns) <- runBatches runner out "stock" [stockSpec image4 properties (caseLabel c) stockFunctions c | c <- caseGrid]
  let stockRun c = lookupRun stockRuns (caseLabel c)
  modelRows <- fmap concat . forM caseGrid $ \c -> case modelled (standing (oracleProperty c)) of
    Nothing -> pure []
    Just owned -> do
      run <- stockRun c
      pure [(c, expectationDifferences (modelExpectation fixture owned (oracleOperation c)) run)]

  -- Pass 2: candidate against stock.
  candidateSpecs <- forM caseGrid $ \c -> do
    run <- stockRun c
    pure (caseSpec candidate candidateFunctions fixture (caseLabel c) (standing (oracleProperty c)) c (observedExpectation run))
  (candidateBatches, candidateRuns) <- runBatches runner out "candidate" candidateSpecs
  verdicts <- forM caseGrid $ \c -> do
    reference <- stockRun c
    subject <- lookupRun candidateRuns (caseLabel c)
    pure (c, compareRuns reference subject)

  -- Guest controls.
  let fieldOf ec = exposingField fixture ec classified
      tableWord (PropertyId n) = tableAddress + 4 * (n - 1)
      entryControl name ec = case fieldOf ec of
        Nothing -> Left (name ++ ": no owned field exposes the mutation")
        Just (Property identifier _ _ verdict, mutated) ->
          let EncodedEntry before = encodeClass verdict
              EncodedEntry after = encodeClass (Owned (OwnedField mutated))
           in Right (Control name MustFail [Patch (tableWord identifier) before after] [OracleCase identifier o | o <- entryControlOperations ec] ("PROPERTIES entry of " ++ propertyHex identifier))
      firstField = listToMaybe [p | p@(Property _ _ _ (Owned (OwnedField _))) <- classified]
  delegatedChoice <- fmap listToMaybe . fmap concat . forM classified $ \property -> case propertyClass property of
    Delegated _ | propertyId property /= exitAdjustedProbe -> do
      runs <- mapM (\o -> stockRun (OracleCase (propertyId property) o)) operations
      pure [property | any (\run -> runStatus run == Returned && lookup R0 (runRegisters run) /= Just 0) runs]
    _ -> pure []
  let delegatedControl = case delegatedChoice of
        Nothing -> Left "c-delegated: no delegated id returns a nonzero r0"
        Just (Property identifier@(PropertyId n) (GuestAddress getterArm) (GuestAddress setterArm) _) -> Right $ Control "c-delegated" MustFail
          [Patch (getterTable + 4 * (n - 1)) getterArm getterStub, Patch (setterTable + 4 * (n - 1)) setterArm setterStub]
          [OracleCase identifier o | o <- operations] ("getter and setter words of delegated " ++ propertyHex identifier ++ " repointed at the stubs")
      probeRepoint = case M.lookup exitAdjustedProbe properties of
        Just (Property (PropertyId n) (GuestAddress getterArm) _ (Delegated _)) -> Right (Patch (getterTable + 4 * (n - 1)) getterArm getterStub)
        _ -> Left ("c-getter-r12: property " ++ propertyHex exitAdjustedProbe ++ " is not delegated")
      probeCases = [OracleCase exitAdjustedProbe o | o@(Get {}) <- operations]
      getterR12 = (\repoint -> Control "c-getter-r12" MustFail [Patch (getterStub + 0xc) movR12FromR4 movR12Zero, repoint] probeCases ("getter stub mov r12, r4 -> mov r12, #0, with " ++ propertyHex exitAdjustedProbe ++ "'s getter word at the stub")) <$> probeRepoint
      getterR12Reference = (\repoint -> Control "r-getter-r12" MustPass [repoint] probeCases ("reference: " ++ propertyHex exitAdjustedProbe ++ "'s getter word at the stub, stub unchanged")) <$> probeRepoint
      getterR6 = case firstField of
        Nothing -> Left "c-getter-r6: no owned field"
        Just (Property identifier _ _ _) -> Right $ Control "c-getter-r6" MustFail
          [Patch getterStub movR4FromR12 movR6FromR12, Patch (getterStub + 0xc) movR12FromR4 movR12FromR6]
          [OracleCase identifier o | o@(Get {}) <- operations] "getter stub holds the id in r6, which the stock getter epilogue does not restore"
      setterBranch = case firstField of
        Nothing -> Left "c-setter-branch: no owned field"
        Just (Property identifier _ _ _) -> Right $ Control "c-setter-branch" MustFail
          [Patch (setterStub + 0x14) (branchWord (setterStub + 0x14) setterExitAddress) (branchWord (setterStub + 0x14) armOne)]
          [OracleCase identifier o | o@(Set {}) <- operations] ("setter stub b " ++ hex setterExitAddress ++ " -> b " ++ hex armOne ++ " (property 1's setter arm)")
      controls =
        [ entryControl "c-offset" OffsetMoved
        , entryControl "c-save" SaveCleared
        , entryControl "c-default" DefaultFlipped
        , delegatedControl
        , getterR12
        , getterR6
        , setterBranch
        ]
      planned = [control | Right control <- controls] ++ [reference | Right reference <- [getterR12Reference]]
      unavailable = [problem | Left problem <- controls ++ [getterR12Reference]]
  controlImages <- forM planned $ \control -> do
    image <- either (failH . ((controlName control ++ ": ") ++)) pure (applyPatches (controlPatches control) candidate)
    pure (control, image)
  controlSpecs <- forM (concat [[(control, image, c) | c <- controlCases control] | (control, image) <- controlImages]) $ \(control, image, c) -> do
    run <- stockRun c
    pure (caseSpec image candidateFunctions fixture (prefixed (controlName control) c) (standing (oracleProperty c)) c (observedExpectation run))
  (controlBatches, controlRuns) <- runBatches runner out "controls" controlSpecs
  controlOutcomes <- forM controlImages $ \(control, _) -> do
    failures <- fmap concat . forM (controlCases control) $ \c -> do
      reference <- stockRun c
      subject <- lookupRun controlRuns (prefixed (controlName control) c)
      pure [(prefixed (controlName control) c, d) | d <- compareRuns reference subject]
    let failedCases = M.size (M.fromListWith (++) [(label, [d]) | (label, d) <- failures])
    pure (control, failedCases, failures)
  let controlsHold = null unavailable && and [controlHolds control failed | (control, failed, _) <- controlOutcomes]

  -- Results.
  runnerSha <- sha256Hex <$> (canonicalizePath runner >>= sha256File)
  inputs <- forM
    ( [("image4", image4Path), ("candidate_image4", candidatePath), ("contract", contractPath), ("linked", linkedPath), ("symbols", symbolsPath)]
        ++ zip ["dat", "loader_a", "loader_b", "image5", "plain", "image2"] sourcePaths
    ) $ \(name, path) -> do
    digest <- sha256Hex <$> sha256File path
    pure (fromString name .= object ["path" .= path, "sha256" .= digest])
  let modelFailures = [(c, ds) | (c, ds) <- modelRows, not (null ds)]
      verdictFailures = [(c, ds) | (c, ds) <- verdicts, not (null ds)]
      modelledIds = M.keys (M.fromList [(oracleProperty c, ()) | (c, _) <- modelRows])
      disagreeingIds = M.keys (M.fromList [(oracleProperty c, ()) | (c, _) <- modelFailures])
      perId = M.fromListWith (+) [(oracleProperty c, 1 :: Int) | (c, _) <- verdictFailures]
      modelPerId = M.fromListWith (+) [(oracleProperty c, 1 :: Int) | (c, _) <- modelFailures]
      classOf identifier = classLabel (standing identifier)
      byClass = M.fromListWith (\(a, b, c) (d, e, f) -> (a + d, b + e, c + f))
        [(classOf (oracleProperty c), (1 :: Int, if null ds then 1 else 0 :: Int, if null ds then 0 else 1 :: Int)) | (c, ds) <- verdicts]
      stockAll = M.elems stockRuns
      candidateAll = M.elems candidateRuns
      passed = null modelFailures && null verdictFailures && controlsHold
      batchValue record = object ["name" .= recordName record, "cases" .= recordCases record, "batch_sha256" .= recordBatchSha record, "summary_sha256" .= recordSummarySha record]
      results = object
        [ "schema_version" .= (1 :: Int)
        , "inputs" .= object inputs
        , "contract_staged_image4_sha256" .= contractSha
        , "runner" .= object ["path" .= runner, "sha256" .= runnerSha]
        , "batches" .= map batchValue (stockBatches ++ candidateBatches ++ controlBatches)
        , "case_shape" .= object
            [ "ids" .= ("0..646" :: String), "cases_per_id" .= length operations, "cases_per_role" .= length caseGrid
            , "carrier" .= object ["start" .= hex carrierStart, "end" .= hex carrierEnd]
            , "stubs" .= object ["getter" .= hex getterStub, "setter" .= hex setterStub, "properties" .= hex tableAddress]
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
                | identifier <- propertyIds
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
                  , "patches" .= [object ["address" .= hex a, "before" .= hex b, "after" .= hex c] | Patch a b c <- controlPatches control]
                  , "cases" .= length (controlCases control)
                  , "failed_cases" .= failed
                  , "holds" .= controlHolds control failed
                  , "first_differences" .= [object ["case" .= caseLabelText label, "kind" .= differenceKind d, "detail" .= differenceDetail d] | (label, d) <- take 20 failures]
                  ]
              | (control, failed, failures) <- controlOutcomes
              ]
                ++ [object ["name" .= problem, "holds" .= False] | problem <- unavailable]
            )
        , "differences" .= differenceSamples
            ( [("model", caseLabel c, d) | (c, ds) <- modelFailures, d <- ds]
                ++ [("pass2", caseLabel c, d) | (c, ds) <- verdictFailures, d <- ds]
            )
        , "passed" .= passed
        ]
  B.writeFile (out </> "results.json") (encodeCanonical results)
  putStrLn ("model: " ++ show (length modelFailures) ++ " disagreeing of " ++ show (length modelRows) ++ " owned-id stock cases")
  putStrLn ("pass 2: " ++ show (length verdictFailures) ++ " failing of " ++ show (length verdicts))
  forM_ controlOutcomes $ \(control, failed, _) -> putStrLn (controlName control ++ ": " ++ show failed ++ " failing of " ++ show (length (controlCases control)))
  forM_ unavailable putStrLn
  putStrLn ("results: " ++ out </> "results.json")
  unless passed exitFailure

-- | One batch of at most 64 cases of the canonical image4: 8 getter and 8 setter operations of the first owned
-- field and of the first delegated id, once with the stock functions and once with the setter split around the carrier.
smoke :: [String] -> FilePath -> IO ()
smoke arguments runner = do
  image4Path <- required arguments "--image4"
  outPath <- required arguments "--out"
  image4 <- readRegular 0x300000 image4Path
  ensure (sha256Hex (sha256 image4) == canonicalImage4Sha256) "--image4 is not the canonical X-M1 1.50 image4"
  classified <- either (\problem -> failH ("classification: " ++ problem)) pure (classify (Image "image4" "" 0x61c400 image4))
  P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) <- orFail "carrier" (P.deriveCarrier classified)
  let properties = M.fromList [(propertyId property, property) | property <- classified]
      chosen = take 1 [propertyId p | p@(Property _ _ _ (Owned (OwnedField _))) <- classified] ++ take 1 [propertyId p | p@(Property _ _ _ (Delegated _)) <- classified]
      cases = [OracleCase identifier o | identifier <- chosen, o <- take 8 [g | g@(Get {}) <- operations] ++ take 8 [s | s@(Set {}) <- operations]]
      split = splitFunctions (carrierStart, carrierEnd) [FunctionSpan carrierStart carrierEnd]
      specs = [stockSpec image4 properties (prefixed "stock" c) stockFunctions c | c <- cases] ++ [stockSpec image4 properties (prefixed "split" c) split c | c <- cases]
  out <- freshOutput outPath
  (record, runs) <- runBatch runner (out </> "smoke") specs
  rows <- forM cases $ \c -> do
    stockRun <- lookupRun runs (prefixed "stock" c)
    splitRun <- lookupRun runs (prefixed "split" c)
    let model = (\owned -> expectationDifferences (modelExpectation fixture owned (oracleOperation c)) stockRun) <$> modelled (standingOf properties (oracleProperty c))
        verdict = compareRuns stockRun splitRun
    putStrLn (caseLabelText (caseLabel c) ++ " stock " ++ runnerStatusLabel (runStatus stockRun) ++ " split " ++ runnerStatusLabel (runStatus splitRun)
      ++ maybe "" (\ds -> " model " ++ (if null ds then "agrees" else unwords (map differenceDetail ds))) model
      ++ " verdict " ++ (if null verdict then "equal" else unwords (map differenceDetail verdict)))
    pure (c, model, verdict)
  let results = object
        [ "batch" .= object ["cases" .= recordCases record, "batch_sha256" .= recordBatchSha record, "summary_sha256" .= recordSummarySha record]
        , "cases" .=
            [ object ["case" .= caseLabelText (caseLabel c), "model_differences" .= fmap (map differenceDetail) model, "split_differences" .= map differenceDetail verdict]
            | (c, model, verdict) <- rows
            ]
        ]
  B.writeFile (out </> "results.json") (encodeCanonical results)
  let failures = length [() | (_, model, verdict) <- rows, maybe False (not . null) model || not (null verdict)]
  putStrLn (show failures ++ " of " ++ show (length rows) ++ " smoke cases differ")
  when (failures > 0 || isNothing (listToMaybe rows)) exitFailure
