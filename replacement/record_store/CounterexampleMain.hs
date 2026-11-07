{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Deterministic generated-corpus -> stock/model prerequisite -> candidate comparison ->
-- boundary-preserving minimization. One terminal JSON; exit 0 pass, 1 behavioural
-- counterexample, 2 invalid/prerequisite/infrastructure/unsupported.
module Main (main) where

import Control.Exception (SomeException, displayException, fromException, throwIO, try)
import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Char (isUpper, toLower)
import Data.IORef (newIORef)
import Data.List (isPrefixOf, nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.Word (Word64)
import Emulation.NativeObservation (NativeObservation (..), runnerStatusLabel)
import Emulation.OwnedCase (CaseId, CaseSpec, loadObservedBatch, writeBatch)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import FujiModPaths (fujimodNativeCase)
import Replacement.RecordStore.CaseJson (decodeCase, encodeCase)
import Replacement.RecordStore.Cases (caseName, name)
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Generate (GenerationSeed (..), ValidCase, generateCases, mkGenerationConfig, oracleCase)
import Replacement.RecordStore.Harness (caseSpec)
import Replacement.RecordStore.Minimize
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
import System.IO (hPutStrLn, stderr)
import Text.Read (readMaybe)

data Options = Options
  { optionCandidate :: FilePath
  , optionSymbols :: FilePath
  , optionRunner :: FilePath
  , optionSeed :: Word64
  , optionCaseCount :: Int
  , optionOperationBound :: Int
  , optionBudget :: Int
  , optionOut :: FilePath
  , optionReplay :: Maybe FilePath
  , optionStockPaths :: StockPaths
  , optionStockCache :: Maybe FilePath
  , optionCandidateCache :: Maybe FilePath
  }

data RunContext = RunContext
  { contextOut :: FilePath
  , contextStock :: StockInputs
  , contextCandidatePath :: FilePath
  , contextCandidateBytes :: B.ByteString
  , contextCandidateDigest :: Sha256
  , contextSymbolsPath :: FilePath
  , contextSymbolsDigest :: Sha256
  , contextFunctions :: [Function]
  , contextRunner :: FilePath
  , contextRunnerDigest :: Sha256
  , contextIndexPath :: FilePath
  , contextIndexDigest :: Sha256
  , contextStockCache :: Maybe FilePath
  , contextCandidateCache :: Maybe FilePath
  }

data CorpusResult
  = CorpusPassed Int [String]
  | CorpusFound Int [String] ValidCase [RoleDifference] NativeObservation NativeObservation
  | CorpusStockMismatch ValidCase [ModelDifference]
  | CorpusAborted String

main :: IO ()
main = do
  args <- getArgs
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  options <- either (invalid . ("argument error: " ++)) pure (parseOptions args)
  run options

invalid :: String -> IO a
invalid message = do
  hPutStrLn stderr message
  exitWith (ExitFailure 2)

lastOption :: [String] -> String -> Either String (Maybe String)
lastOption arguments key = go Nothing arguments
  where
    go found [] = Right found
    go _ (argument : rest)
      | argument == key = case rest of
          [] -> Left (key ++ " needs a value")
          value : more -> go (Just value) more
      | otherwise = go found rest

requiredOption :: [String] -> String -> Either String FilePath
requiredOption arguments key = maybe (Left ("missing " ++ key)) Right =<< lastOption arguments key

parseWord64 :: String -> Either String Word64
parseWord64 text = maybe (Left ("invalid unsigned integer: " ++ text)) Right (readMaybe text)

parseBounded :: String -> Int -> Int -> String -> Either String Int
parseBounded label low high text = case readMaybe text :: Maybe Int of
  Just value
    | value >= low && value <= high -> Right value
  _ -> Left (label ++ " must be " ++ show low ++ ".." ++ show high)

parseOptions :: [String] -> Either String Options
parseOptions arguments = do
  candidate <- requiredOption arguments "--candidate"
  symbols <- requiredOption arguments "--symbols"
  out <- requiredOption arguments "--out"
  replay <- lastOption arguments "--replay"
  runner <- maybe (Right fujimodNativeCase) Right =<< lastOption arguments "--runner"
  seed <- maybe (Right 0) parseWord64 =<< lastOption arguments "--seed"
  caseCount <- maybe (Right 8) (parseBounded "--cases" 1 64) =<< lastOption arguments "--cases"
  operationBound <- maybe (Right 6) (parseBounded "--operations" 3 12) =<< lastOption arguments "--operations"
  budget <- maybe (Right 32) (parseBounded "--budget" 1 256) =<< lastOption arguments "--budget"
  image <- maybe (Right (stockImagePath defaultStockPaths)) Right =<< lastOption arguments "--image"
  loader <- maybe (Right (stockLoaderPath defaultStockPaths)) Right =<< lastOption arguments "--loader"
  index <- maybe (Right (stockIndexPath defaultStockPaths)) Right =<< lastOption arguments "--index"
  stockCache <- lastOption arguments "--stock-output"
  candidateCache <- lastOption arguments "--candidate-output"
  pure
    Options
      { optionCandidate = candidate
      , optionSymbols = symbols
      , optionRunner = runner
      , optionSeed = seed
      , optionCaseCount = caseCount
      , optionOperationBound = operationBound
      , optionBudget = budget
      , optionOut = out
      , optionReplay = replay
      , optionStockPaths = StockPaths image loader index
      , optionStockCache = stockCache
      , optionCandidateCache = candidateCache
      }

run :: Options -> IO ()
run options = do
  setup <- try (buildContext options) :: IO (Either SomeException RunContext)
  context <- either (invalid . displayException) pure setup
  replayFile <- traverse makeAbsolute (optionReplay options)
  case replayFile of
    Just file -> guardExit (runReplay options context file)
    Nothing -> guardExit (runLoop options context)

-- | Deliberate 'exitWith' codes pass through; any other exception is infrastructure.
guardExit :: IO () -> IO ()
guardExit action = do
  outcome <- try action :: IO (Either SomeException ())
  case outcome of
    Right () -> pure ()
    Left exception -> case (fromException exception :: Maybe ExitCode) of
      Just exitCode -> throwIO exitCode
      Nothing -> invalid (displayException exception)

buildContext :: Options -> IO RunContext
buildContext options = do
  candidate <- makeAbsolute (optionCandidate options)
  symbols <- makeAbsolute (optionSymbols options)
  runner <- makeAbsolute (optionRunner options)
  out <- makeAbsolute (optionOut options)
  stockCache <- traverse makeAbsolute (optionStockCache options)
  candidateCache <- traverse makeAbsolute (optionCandidateCache options)
  imagePath <- makeAbsolute (stockImagePath (optionStockPaths options))
  loaderPath <- makeAbsolute (stockLoaderPath (optionStockPaths options))
  indexPath <- makeAbsolute (stockIndexPath (optionStockPaths options))
  let stockPaths = StockPaths imagePath loaderPath indexPath
  exists <- doesPathExist out
  when exists (error (out ++ " already exists"))
  createDirectoryIfMissing True out
  outputRoot <- canonicalizePath out
  forM_ [root | Just root <- [stockCache, candidateCache]] $ \root -> do
    cachedRoot <- canonicalizePath root
    when
      ( splitDirectories outputRoot `isPrefixOf` splitDirectories cachedRoot
          || splitDirectories cachedRoot `isPrefixOf` splitDirectories outputRoot
      )
      (error "output directory overlaps cached observations")
  stock <- case loadStockInputs stockPaths of
    Left problem -> error (show problem)
    Right value -> pure value
  candidateBytes <- B.readFile candidate
  unless (B.length candidateBytes == 0x1e60) (error ("candidate span size " ++ show (B.length candidateBytes)))
  functions <- symbolFunctions symbols
  runnerDigest <- sha256File runner
  candidateDigest <- sha256File candidate
  symbolsDigest <- sha256File symbols
  indexDigest <- sha256File indexPath
  pure
    RunContext
      { contextOut = out
      , contextStock = stock
      , contextCandidatePath = candidate
      , contextCandidateBytes = candidateBytes
      , contextCandidateDigest = candidateDigest
      , contextSymbolsPath = symbols
      , contextSymbolsDigest = symbolsDigest
      , contextFunctions = functions
      , contextRunner = runner
      , contextRunnerDigest = runnerDigest
      , contextIndexPath = indexPath
      , contextIndexDigest = indexDigest
      , contextStockCache = stockCache
      , contextCandidateCache = candidateCache
      }

identityObject :: Options -> RunContext -> Value
identityObject options context =
  object
    [ "image4_sha256" .= sha256Hex (sha256 (stockImage (contextStock context)))
    , "loader_sha256" .= sha256Hex (sha256 (stockLoader (contextStock context)))
    , "function_index" .= contextIndexPath context
    , "function_index_sha256" .= sha256Hex (contextIndexDigest context)
    , "candidate" .= contextCandidatePath context
    , "candidate_sha256" .= sha256Hex (contextCandidateDigest context)
    , "symbols" .= contextSymbolsPath context
    , "symbols_sha256" .= sha256Hex (contextSymbolsDigest context)
    , "runner" .= contextRunner context
    , "runner_sha256" .= sha256Hex (contextRunnerDigest context)
    , "stock_role" .= ("stock" :: String)
    , "candidate_role" .= ("candidate" :: String)
    , "seed" .= optionSeed options
    , "cases" .= optionCaseCount options
    , "operations" .= optionOperationBound options
    , "shrink_budget" .= optionBudget options
    ]

countsValue :: Int -> Int -> Int -> Int -> Int -> Value
countsValue generated compared matched inconclusive differed =
  object
    [ "generated" .= generated
    , "compared" .= compared
    , "matched" .= matched
    , "inconclusive" .= inconclusive
    , "differed" .= differed
    ]

terminalValue :: String -> Int -> Int -> Int -> Int -> Int -> [String] -> Maybe FilePath -> Maybe FilePath -> FilePath -> Value
terminalValue verdict generated compared matched inconclusive differed boundaries originalCase minimizedCase reportPath =
  object
    [ "verdict" .= verdict
    , "generated" .= generated
    , "compared" .= compared
    , "matched" .= matched
    , "inconclusive" .= inconclusive
    , "differed" .= differed
    , "boundaries" .= boundaries
    , "original_case" .= originalCase
    , "minimized_case" .= minimizedCase
    , "report" .= reportPath
    ]

observationValue :: NativeObservation -> NativeObservation -> [RoleDifference] -> Value
observationValue stock candidate differences =
  object
    [ "stock_status" .= runnerStatusLabel (nativeStatus stock)
    , "candidate_status" .= runnerStatusLabel (nativeStatus candidate)
    , "boundaries" .= map boundaryLabel (nub (map roleBoundary differences))
    , "differences" .= map roleDifferenceJson differences
    ]

reductionStatusLabel :: ReductionStatus -> String
reductionStatusLabel = kebab . show
  where
    kebab [] = []
    kebab (first : rest) = toLower first : go rest
    go [] = []
    go (character : rest)
      | isUpper character = '-' : toLower character : go rest
      | otherwise = character : go rest

-- | FixedPoint is the only status that establishes local minimality.
minimalityLabel :: ReductionStatus -> String
minimalityLabel status = case status of
  FixedPoint -> "established"
  _ -> "incomplete"

listInconclusive :: FilePath -> IO [Value]
listInconclusive root = do
  present <- doesDirectoryExist root
  if not present
    then pure []
    else do
      names <- sort <$> listDirectory root
      pure [object ["class" .= name, "path" .= (root </> name)] | name <- names]

finish :: RunContext -> Value -> Value -> Int -> IO ()
finish context report terminal code = do
  B.writeFile (contextOut context </> "report.json") (encodeCanonical report)
  B.putStrLn (encodeCanonical terminal)
  exitWith (if code == 0 then ExitSuccess else ExitFailure code)

runPairAt :: RunContext -> String -> ValidCase -> IO PairOutcome
runPairAt context name valid = do
  let c = oracleCase valid
      spec = caseSpec (stockImage (contextStock context)) (stockLoader (contextStock context)) (stockFunctions (contextStock context)) c
      candidate = candidateSpec (contextCandidateBytes context) (contextFunctions context) spec
  runCasePair (contextRunner context) (contextOut context </> name) spec candidate

runLoop :: Options -> RunContext -> IO ()
runLoop options context = do
  config <-
    either
      (invalid . ("generation config: " ++) . show)
      pure
      (mkGenerationConfig (GenerationSeed (optionSeed options)) (optionCaseCount options) (optionOperationBound options))
  let validCases = generateCases config
  cachedStock <- cachedCaseInputs "stock" (contextStockCache context)
  cachedCandidate <- cachedCaseInputs "candidate" (contextCandidateCache context)
  corpus <- runCorpus context cachedStock cachedCandidate validCases
  case corpus of
    CorpusAborted message -> do
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("infrastructure" :: String)
              , "reason" .= message
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue "infrastructure" (length validCases) 0 0 0 0 [] Nothing Nothing (contextOut context </> "report.json")
      finish context report terminal 2
    CorpusStockMismatch valid problems -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("prerequisite" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "model_differences" .= map modelDifferenceJson problems
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue "prerequisite" (length validCases) 0 0 0 0 [] Nothing Nothing (contextOut context </> "report.json")
      finish context report terminal 2
    CorpusPassed matched inconclusive -> do
      let reasons = inconclusive
          verdict = if null reasons then "pass" else "inconclusive"
          compared = matched + length reasons
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdict
              , "counts" .= countsValue (length validCases) compared matched (length reasons) 0
              , "inconclusive_reasons" .= reasons
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue verdict (length validCases) compared matched (length reasons) 0 [] Nothing Nothing (contextOut context </> "report.json")
      finish context report terminal (if null reasons then 0 else 2)
    CorpusFound matched inconclusive valid differences _ _ -> do
      budget <- either (invalid . ("shrink budget: " ++)) pure (mkShrinkBudget (optionBudget options))
      counter <- newIORef 0
      retained <- newIORef []
      let trialRunner =
            TrialRunner
              { trialRunnerPath = contextRunner context
              , trialStockInputs = contextStock context
              , trialCandidateBytes = contextCandidateBytes context
              , trialCandidateFunctions = contextFunctions context
              , trialRoot = contextOut context </> "trials"
              , trialInconclusiveRoot = contextOut context </> "inconclusive"
              , trialRetained = retained
              , trialCounter = counter
              }
          boundaries = nub (map roleBoundary differences)
      primary <- case listToMaybe boundaries of
        Just boundary -> pure boundary
        Nothing -> invalid "candidate difference has no comparison boundary"
      shrunk <- try (shrinkCase budget primary (mkTrial trialRunner) valid) :: IO (Either SomeException ShrinkResult)
      result <- case shrunk of
        Left exception -> invalid ("minimization failed: " ++ displayException exception)
        Right value -> pure value
      removeDirectoryIfPresent (contextOut context </> "trials")
      inconclusiveExamples <- listInconclusive (contextOut context </> "inconclusive")
      originalOutcome <- runPairAt context "original" valid
      (originalStock', originalCandidate') <- case originalOutcome of
        PairFailed message -> invalid ("original replay failed: " ++ message)
        PairObserved stockObservation candidateObservation -> pure (stockObservation, candidateObservation)
      originalDifferences <- case replayVerdict valid originalStock' originalCandidate' of
        CaseDiffers observed | primary `elem` map roleBoundary observed -> pure observed
        other -> invalid ("original case no longer reproduces the boundary: " ++ show other)
      B.writeFile (contextOut context </> "original.case.json") (encodeCase valid)
      case reductionStatus result of
        OriginalDoesNotReproduce -> do
          let reportPath = contextOut context </> "report.json"
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= ("inconclusive" :: String)
                  , "reason" .= ("reduction baseline did not reproduce" :: String)
                  , "counts" .= countsValue (length validCases) (matched + length inconclusive + 1) matched (length inconclusive) 1
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                        , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                        ]
                  , "reduction" .=
                      object
                        [ "status" .= reductionStatusLabel (reductionStatus result)
                        , "trials" .= trialCount result
                        , "minimality" .= minimalityLabel (reductionStatus result)
                        , "inconclusive_examples" .= inconclusiveExamples
                        ]
                  , "paths" .=
                      object
                        [ "original_case" .= (contextOut context </> "original.case.json")
                        , "original_native" .= (contextOut context </> "original")
                        , "report" .= reportPath
                        ]
                  , "identity" .= identityObject options context
                  ]
              terminal =
                terminalValue
                  "inconclusive"
                  (length validCases)
                  (matched + length inconclusive + 1)
                  matched
                  (length inconclusive)
                  1
                  (map boundaryLabel boundaries)
                  (Just (contextOut context </> "original.case.json"))
                  Nothing
                  reportPath
          finish context report terminal 2
        _ -> pure ()
      let minimized = minimizedCase result
      minimizedOutcome <- runPairAt context "minimized" minimized
      (minimizedStock, minimizedCandidate) <- case minimizedOutcome of
        PairFailed message -> invalid ("minimized replay failed: " ++ message)
        PairObserved stockObservation candidateObservation -> pure (stockObservation, candidateObservation)
      minimizedDifferences <- case replayVerdict minimized minimizedStock minimizedCandidate of
        CaseDiffers observed | primary `elem` map roleBoundary observed -> pure observed
        other -> invalid ("minimized case does not reproduce the original boundary: " ++ show other)
      B.writeFile (contextOut context </> "minimized.case.json") (encodeCase minimized)
      let reportPath = contextOut context </> "report.json"
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("counterexample" :: String)
              , "counts" .= countsValue (length validCases) (matched + length inconclusive + 1) matched (length inconclusive) 1
              , "inconclusive_reasons" .= inconclusive
              , "failure" .=
                  object
                    [ "boundary" .= boundaryLabel primary
                    , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                    , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                    , "minimized" .= observationValue minimizedStock minimizedCandidate minimizedDifferences
                    ]
              , "reduction" .=
                  object
                    [ "status" .= reductionStatusLabel (reductionStatus result)
                    , "trials" .= trialCount result
                    , "minimality" .= minimalityLabel (reductionStatus result)
                    , "inconclusive_examples" .= inconclusiveExamples
                    ]
              , "paths" .=
                  object
                    [ "original_case" .= (contextOut context </> "original.case.json")
                    , "minimized_case" .= (contextOut context </> "minimized.case.json")
                    , "original_native" .= (contextOut context </> "original")
                    , "minimized_native" .= (contextOut context </> "minimized")
                    , "report" .= reportPath
                    ]
              , "identity" .= identityObject options context
              ]
          terminal =
            terminalValue
              "counterexample"
              (length validCases)
              (matched + length inconclusive + 1)
              matched
              (length inconclusive)
              1
              (map boundaryLabel boundaries)
              (Just (contextOut context </> "original.case.json"))
              (Just (contextOut context </> "minimized.case.json"))
              reportPath
      finish context report terminal 1

runReplay :: Options -> RunContext -> FilePath -> IO ()
runReplay options context file = do
  bytes <- B.readFile file
  valid <- either (invalid . ("case: " ++) . show) pure (decodeCase bytes)
  outcome <- runPairAt context "replay" valid
  (stockObservation, candidateObservation) <- case outcome of
    PairFailed message -> invalid ("replay failed: " ++ message)
    PairObserved stock candidate -> pure (stock, candidate)
  let reportPath = contextOut context </> "report.json"
  case replayVerdict valid stockObservation candidateObservation of
    CaseMatches -> do
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("matched" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "counts" .= countsValue 1 1 1 0 0
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue "matched" 1 1 1 0 0 [] Nothing Nothing reportPath
      finish context report terminal 0
    CaseDiffers differences -> do
      let boundaries = nub (map roleBoundary differences)
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("counterexample" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "counts" .= countsValue 1 1 0 0 1
              , "failure" .=
                  object
                    [ "boundaries" .= map boundaryLabel boundaries
                    , "observed" .= observationValue stockObservation candidateObservation differences
                    ]
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue "counterexample" 1 1 0 0 1 (map boundaryLabel boundaries) Nothing Nothing reportPath
      finish context report terminal 1
    CaseStockModelMismatch problems -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("prerequisite" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "model_differences" .= map modelDifferenceJson problems
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
      finish context report (terminalValue "prerequisite" 1 1 0 0 0 [] Nothing Nothing reportPath) 2
    CaseUnsupported reason -> do
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("unsupported" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "reason" .= reason
              , "identity" .= identityObject options context
              ]
      finish context report (terminalValue "unsupported" 1 1 0 0 0 [] Nothing Nothing reportPath) 2

runCorpus :: RunContext -> M.Map CaseId FilePath -> M.Map CaseId FilePath -> [ValidCase] -> IO CorpusResult
runCorpus context cachedStock cachedCandidate validCases =
  go 0 [] (zip [0 :: Int ..] (chunks 4 validCases))
  where
    out = contextOut context
    go :: Int -> [String] -> [(Int, [ValidCase])] -> IO CorpusResult
    go matched inconclusive [] = pure (CorpusPassed matched (reverse inconclusive))
    go matched inconclusive ((chunkIndex, chunk) : rest) = do
      let specs =
            [ caseSpec (stockImage (contextStock context)) (stockLoader (contextStock context)) (stockFunctions (contextStock context)) (oracleCase valid)
            | valid <- chunk
            ]
          stockDirectory = out </> "stock-" ++ show chunkIndex
          candidateDirectory = out </> "candidate-" ++ show chunkIndex
      stockObserved <- observeBatch "stock" context cachedStock stockDirectory specs (contextStockCache context)
      case stockObserved of
        Left problem -> pure (CorpusAborted problem)
        Right stockObservations -> do
          let stockValues = map snd stockObservations
              rows = zip3 chunk specs stockValues
              mismatches =
                [ (valid, spec, problems)
                | (valid, spec, observation) <- rows
                , let problems = modelDifferences (oracleCase valid) observation
                , not (null problems)
                ]
          case mismatches of
            ((valid, spec, problems) : _) -> do
              preserveFailureCase out spec
              cleanChunk out chunkIndex
              pure (CorpusStockMismatch valid problems)
            [] -> do
              let candidateSpecs = [candidateSpec (contextCandidateBytes context) (contextFunctions context) spec | spec <- specs]
              candidateObserved <- observeBatch "candidate" context cachedCandidate candidateDirectory candidateSpecs (contextCandidateCache context)
              case candidateObserved of
                Left problem -> pure (CorpusAborted problem)
                Right candidateObservations -> do
                  let evaluated =
                        [ (valid, stockObservation, candidateObservation, evaluateCase (oracleCase valid) stockObservation candidateObservation)
                        | (valid, stockObservation, candidateObservation) <- zip3 chunk stockValues (map snd candidateObservations)
                        ]
                  walked <- walk matched inconclusive evaluated
                  case walked of
                    Left result -> do
                      cleanChunk out chunkIndex
                      pure result
                    Right (matched', inconclusive') -> do
                      cleanChunk out chunkIndex
                      go matched' inconclusive' (chunkIndex + 1) rest
    walk :: Int -> [String] -> [(ValidCase, NativeObservation, NativeObservation, CaseVerdict)] -> IO (Either CorpusResult (Int, [String]))
    walk matched inconclusive [] = pure (Right (matched, inconclusive))
    walk matched inconclusive ((valid, stockObservation, candidateObservation, verdict) : more) =
      case verdict of
        CaseMatches -> walk (matched + 1) inconclusive more
        CaseUnsupported reason -> walk matched (reason : inconclusive) more
        CaseStockModelMismatch problems -> pure (Left (CorpusStockMismatch valid problems))
        CaseDiffers differences -> pure (Left (CorpusFound matched (reverse inconclusive) valid differences stockObservation candidateObservation))

observeBatch :: String -> RunContext -> M.Map CaseId FilePath -> FilePath -> [CaseSpec] -> Maybe FilePath -> IO (Either String [(CaseId, NativeObservation)])
observeBatch role context cached directory specs cache = do
  outcome <-
    try
      ( do
          batchPath <- writeBatch directory specs
          patchStubs directory specs
          case cache of
            Nothing -> loadObservedBatch (contextRunner context) batchPath
            Just _ -> cachedObserved role (Just (contextRunnerDigest context)) cached directory specs
      )
      :: IO (Either SomeException [(CaseId, NativeObservation)])
  pure (either (Left . displayException) Right outcome)

preserveFailureCase :: FilePath -> CaseSpec -> IO ()
preserveFailureCase out spec = do
  let directory = out </> "failure"
  _ <- writeBatch directory [spec]
  patchStubs directory [spec]

cleanChunk :: FilePath -> Int -> IO ()
cleanChunk out index = do
  removeDirectoryIfPresent (out </> "stock-" ++ show index)
  removeDirectoryIfPresent (out </> "candidate-" ++ show index)

removeDirectoryIfPresent :: FilePath -> IO ()
removeDirectoryIfPresent path = do
  present <- doesDirectoryExist path
  when present (removeDirectoryRecursive path)

chunks :: Int -> [a] -> [[a]]
chunks _ [] = []
chunks size items = let (first, rest) = splitAt size items in first : chunks size rest
