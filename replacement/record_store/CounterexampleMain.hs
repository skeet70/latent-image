{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Deterministic generated-corpus -> stock/model prerequisite -> candidate comparison ->
-- boundary-preserving minimization. One terminal JSON; exit 0 pass, 1 behavioural
-- counterexample, 2 invalid/prerequisite/infrastructure/unsupported.
module Main (main) where

import Control.Exception (Exception, displayException, fromException, throwIO)
import Control.Monad (filterM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Char (isUpper, toLower)
import Data.IORef (newIORef)
import Data.List (intercalate, isPrefixOf, nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.Word (Word64)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.NativeObservation (NativeObservation (..), RunnerStatus (..), runnerStatusLabel)
import Emulation.OwnedCase (CaseId, CaseSpec, loadObservedBatch, writeBatch)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import FujiModPaths (fujimodNativeCase)
import Replacement.RecordStore.CaseJson (decodeCase, encodeCase)
import Replacement.RecordStore.Cases (caseName, name)
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Generate (GenerationSeed (..), ValidCase, generateCases, mkGenerationConfig, oracleCase)
import Replacement.RecordStore.Harness (Function, caseSpec)
import Replacement.RecordStore.Minimize
import System.Directory
import System.Environment (getArgs, getExecutablePath, lookupEnv, setCurrentDirectory, setEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, splitDirectories, takeDirectory, takeFileName, (</>))
import System.Posix.Files (setFileMode)
import Text.Read (readMaybe)

candidateRelative, symbolsRelative :: FilePath
candidateRelative = "replacement/record_store_carrier/record-store-image.bin"
symbolsRelative = "replacement/record_store_carrier/record-store-image.symbols"

data Anchor = Anchor
  { anchorInvocation :: FilePath
  , anchorRunfiles :: Maybe FilePath
  }

data RawOptions = RawOptions
  { rawCandidate :: Maybe FilePath
  , rawSymbols :: Maybe FilePath
  , rawRunner :: Maybe FilePath
  , rawSeed :: Word64
  , rawCaseCount :: Int
  , rawOperationBound :: Int
  , rawBudget :: Int
  , rawOut :: FilePath
  , rawReplay :: Maybe FilePath
  , rawImage :: FilePath
  , rawLoader :: FilePath
  , rawIndex :: FilePath
  , rawCache :: Maybe FilePath
  }

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
  , optionCache :: Maybe FilePath
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
  , contextRunnerImmutable :: Bool
  , contextIndexPath :: FilePath
  , contextIndexDigest :: Sha256
  , contextCache :: Maybe FilePath
  }

data CorpusResult
  = CorpusPassed Int [String]
  | CorpusFound Int [String] ValidCase [RoleDifference] NativeObservation NativeObservation
  | CorpusStockMismatch ValidCase [ModelDifference]
  | CorpusAborted String

data ToolFailure = ToolFailure String String

instance Show ToolFailure where
  show (ToolFailure verdict reason) = verdict ++ ": " ++ reason

instance Exception ToolFailure

main :: IO ()
main = do
  args <- getArgs
  anchorResult <- trySync captureAnchor
  anchor <- case anchorResult of
    Left exception -> terminalFailure Nothing "infrastructure" (displayException exception)
    Right value -> pure value
  workspaceResult <- trySync (lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory)
  case workspaceResult of
    Left exception -> terminalFailure Nothing "infrastructure" (displayException exception)
    Right () -> pure ()
  raw <- case parseOptions args of
    Left message -> terminalFailure Nothing "invalid" message
    Right value -> pure value
  resolved <- trySync (resolveOptions anchor raw)
  options <- case resolved of
    Left exception -> terminalFailure Nothing "invalid" (displayException exception)
    Right (Left message) -> terminalFailure Nothing "invalid" message
    Right (Right value) -> pure value
  setEnv "XM1_STORAGE_PATH" (takeDirectory (optionOut options))
  admitted <- trySync (slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy (reexecArguments options))
  case admitted of
    Left exception -> terminalFailure Nothing "infrastructure" (displayException exception)
    Right () -> run options

invalid :: String -> IO a
invalid message = throwIO (ToolFailure "invalid" message)

captureAnchor :: IO Anchor
captureAnchor = do
  invocation <- getCurrentDirectory
  executable <- getExecutablePath
  environment <- lookupEnv "RUNFILES_DIR"
  environmentExists <- maybe (pure False) doesDirectoryExist environment
  adjacentExists <- doesDirectoryExist (executable ++ ".runfiles")
  let runfiles
        | environmentExists = environment
        | adjacentExists = Just (executable ++ ".runfiles")
        | otherwise = Nothing
  pure (Anchor invocation runfiles)

resolveOptions :: Anchor -> RawOptions -> IO (Either String Options)
resolveOptions anchor raw = do
  candidate <- resolveInput anchor (rawCandidate raw) candidateRelative
  symbols <- resolveInput anchor (rawSymbols raw) symbolsRelative
  runner <- resolveRunner anchor (rawRunner raw)
  image <- makeAbsolute (rawImage raw)
  loader <- makeAbsolute (rawLoader raw)
  index <- makeAbsolute (rawIndex raw)
  replay <- traverse makeAbsolute (rawReplay raw)
  cache <- traverse makeAbsolute (rawCache raw)
  admittedOut <- outputAdmission (rawOut raw)
  case (candidate, symbols, runner, admittedOut) of
    (Right candidatePath, Right symbolsPath, Right runnerPath, Right outPath) -> do
      let files =
            [ ("candidate", candidatePath)
            , ("symbols", symbolsPath)
            , ("runner", runnerPath)
            , ("image", image)
            , ("loader", loader)
            , ("index", index)
            ]
              ++ maybe [] (\path -> [("replay", path)]) replay
      missing <- filterM (fmap not . doesFileExist . snd) files
      case missing of
        (label, path) : _ -> pure (Left ("missing " ++ label ++ ": " ++ path))
        [] ->
          pure
            ( Right
                Options
                  { optionCandidate = candidatePath
                  , optionSymbols = symbolsPath
                  , optionRunner = runnerPath
                  , optionSeed = rawSeed raw
                  , optionCaseCount = rawCaseCount raw
                  , optionOperationBound = rawOperationBound raw
                  , optionBudget = rawBudget raw
                  , optionOut = outPath
                  , optionReplay = replay
                  , optionStockPaths = StockPaths image loader index
                  , optionCache = cache
                  }
            )
    _ -> pure (Left (firstLeft [candidate, symbols, runner, admittedOut]))

firstLeft :: [Either String a] -> String
firstLeft values = case [message | Left message <- values] of
  message : _ -> message
  [] -> "invalid options"

resolveInput :: Anchor -> Maybe FilePath -> FilePath -> IO (Either String FilePath)
resolveInput anchor supplied relative = case supplied of
  Nothing -> resolveBundled anchor relative
  Just path -> do
    absolute <- makeAbsolute path
    exists <- doesFileExist absolute
    if exists
      then pure (Right absolute)
      else
        if isAbsolute path
          then pure (Left ("missing input: " ++ path))
          else resolveBundled anchor path

resolveRunner :: Anchor -> Maybe FilePath -> IO (Either String FilePath)
resolveRunner anchor supplied = case supplied of
  Just path -> do
    absolute <- makeAbsolute path
    exists <- doesFileExist absolute
    pure (if exists then Right absolute else Left ("missing runner: " ++ absolute))
  Nothing
    | isAbsolute fujimodNativeCase -> pure (Right fujimodNativeCase)
    | otherwise -> resolveBundled anchor fujimodNativeCase

resolveBundled :: Anchor -> FilePath -> IO (Either String FilePath)
resolveBundled anchor relative = do
  workspace <- getCurrentDirectory
  let variants = nub (relative : [rest | prefix <- ["_main/", "fuji-af/"], Just rest <- [stripPrefix prefix relative]])
      runfilesRoots = case anchorRunfiles anchor of
        Nothing -> []
        Just root -> [root, root </> "_main", root </> "fuji-af"]
      candidates =
        [root </> variant | root <- runfilesRoots, variant <- variants]
          ++ [anchorInvocation anchor </> relative, workspace </> relative]
  existing <- filterM doesFileExist candidates
  case existing of
    path : _ -> Right <$> makeAbsolute path
    [] -> pure (Left ("bundled input not found: " ++ relative ++ " (looked in " ++ intercalate ", " candidates ++ ")"))

outputAdmission :: FilePath -> IO (Either String FilePath)
outputAdmission path = do
  absolute <- makeAbsolute path
  workspace <- getCurrentDirectory >>= canonicalizePath
  private <- canonicalizePath (workspace </> "private-data")
  destination <- canonicalExisting absolute
  let components = splitDirectories destination
      admitted =
        splitDirectories (private </> "scratch") `isPrefixOf` components
          || splitDirectories (private </> "verify-candidate" </> "runs") `isPrefixOf` components
  if not admitted
    then pure (Left (destination ++ " must be under private-data/scratch or private-data/verify-candidate/runs"))
    else do
      exists <- doesPathExist destination
      pure (if exists then Left (destination ++ " already exists") else Right destination)

canonicalExisting :: FilePath -> IO FilePath
canonicalExisting path = do
  exists <- doesPathExist path
  if exists
    then canonicalizePath path
    else do
      let parent = takeDirectory path
      if parent == path
        then pure path
        else do
          resolved <- canonicalExisting parent
          pure (resolved </> takeFileName path)

freshOutput :: FilePath -> IO FilePath
freshOutput path = do
  admitted <- outputAdmission path
  destination <- either (throwIO . ToolFailure "invalid") pure admitted
  createDirectoryIfMissing True (takeDirectory destination)
  createDirectory destination
  setFileMode destination 0o700
  pure destination

reexecArguments :: Options -> [String]
reexecArguments options =
  concat
    [ ["--candidate", optionCandidate options]
    , ["--symbols", optionSymbols options]
    , ["--runner", optionRunner options]
    , ["--out", optionOut options]
    , ["--image", stockImagePath (optionStockPaths options)]
    , ["--loader", stockLoaderPath (optionStockPaths options)]
    , ["--index", stockIndexPath (optionStockPaths options)]
    , ["--seed", show (optionSeed options)]
    , ["--cases", show (optionCaseCount options)]
    , ["--operations", show (optionOperationBound options)]
    , ["--budget", show (optionBudget options)]
    ]
      ++ maybe [] (\path -> ["--replay", path]) (optionReplay options)
      ++ maybe [] (\path -> ["--cache", path]) (optionCache options)

valuedFlags :: [String]
valuedFlags =
  [ "--candidate"
  , "--symbols"
  , "--runner"
  , "--seed"
  , "--cases"
  , "--operations"
  , "--budget"
  , "--out"
  , "--replay"
  , "--image"
  , "--loader"
  , "--index"
  , "--cache"
  ]

collectOptions :: [String] -> Either String (M.Map String String)
collectOptions = go M.empty
  where
    go seen [] = Right seen
    go seen (argument : rest)
      | argument `elem` valuedFlags =
          case rest of
            [] -> Left (argument ++ " needs a value")
            value : more
              | value `elem` valuedFlags || "--" `isPrefixOf` value -> Left (argument ++ " needs a value")
              | M.member argument seen -> Left ("duplicate " ++ argument)
              | otherwise -> go (M.insert argument value seen) more
      | "--" `isPrefixOf` argument -> Left ("unknown argument " ++ argument)
      | otherwise -> Left ("unexpected argument " ++ argument)

parseWord64 :: String -> Either String Word64
parseWord64 text = maybe (Left ("invalid unsigned integer: " ++ text)) Right (readMaybe text)

parseBounded :: String -> Int -> Int -> String -> Either String Int
parseBounded label low high text = case readMaybe text :: Maybe Int of
  Just value
    | value >= low && value <= high -> Right value
  _ -> Left (label ++ " must be " ++ show low ++ ".." ++ show high)

parseOptions :: [String] -> Either String RawOptions
parseOptions arguments = do
  values <- collectOptions arguments
  let required key = maybe (Left ("missing " ++ key)) Right (M.lookup key values)
      optional key = Right (M.lookup key values)
  out <- required "--out"
  candidate <- optional "--candidate"
  symbols <- optional "--symbols"
  runner <- optional "--runner"
  replay <- optional "--replay"
  seed <- maybe (Right 0) parseWord64 =<< optional "--seed"
  caseCount <- maybe (Right 8) (parseBounded "--cases" 1 64) =<< optional "--cases"
  operationBound <- maybe (Right 6) (parseBounded "--operations" 3 12) =<< optional "--operations"
  budget <- maybe (Right 32) (parseBounded "--budget" 1 256) =<< optional "--budget"
  image <- maybe (Right (stockImagePath defaultStockPaths)) Right =<< optional "--image"
  loader <- maybe (Right (stockLoaderPath defaultStockPaths)) Right =<< optional "--loader"
  index <- maybe (Right (stockIndexPath defaultStockPaths)) Right =<< optional "--index"
  cache <- optional "--cache"
  pure
    RawOptions
      { rawCandidate = candidate
      , rawSymbols = symbols
      , rawRunner = runner
      , rawSeed = seed
      , rawCaseCount = caseCount
      , rawOperationBound = operationBound
      , rawBudget = budget
      , rawOut = out
      , rawReplay = replay
      , rawImage = image
      , rawLoader = loader
      , rawIndex = index
      , rawCache = cache
      }

run :: Options -> IO ()
run options = do
  setup <- trySync (buildContext options)
  context <- case setup of
    Right value -> pure value
    Left exception -> case fromException exception :: Maybe ToolFailure of
      Just (ToolFailure verdict reason) -> terminalFailure Nothing verdict reason
      Nothing -> terminalFailure Nothing "invalid" (displayException exception)
  guardExit options context $ case optionReplay options of
    Just file -> runReplay options context file
    Nothing -> runLoop options context

guardExit :: Options -> RunContext -> IO () -> IO ()
guardExit options context action = do
  outcome <- trySync action
  case outcome of
    Right () -> pure ()
    Left exception -> case fromException exception :: Maybe ToolFailure of
      Just (ToolFailure verdict reason) -> terminalFailure (Just (options, context)) verdict reason
      Nothing -> case fromException exception :: Maybe ExitCode of
        Just exitCode -> throwIO exitCode
        Nothing -> terminalFailure (Just (options, context)) "infrastructure" (displayException exception)

buildContext :: Options -> IO RunContext
buildContext options = do
  stockResult <- loadStockInputs (optionStockPaths options)
  stock <- either (throwIO . ToolFailure "prerequisite" . show) pure stockResult
  candidateBytes <- B.readFile (optionCandidate options)
  unless (B.length candidateBytes == 0x1e60) $
    throwIO (ToolFailure "invalid" ("candidate span size " ++ show (B.length candidateBytes)))
  symbolsBytes <- B.readFile (optionSymbols options)
  runner <- canonicalizePath (optionRunner options)
  runnerDigest <- sha256File runner
  forM_ (optionCache options) $ \root -> do
    cachedRoot <- canonicalExisting root
    outputRoot <- canonicalExisting (optionOut options)
    when
      ( splitDirectories outputRoot `isPrefixOf` splitDirectories cachedRoot
          || splitDirectories cachedRoot `isPrefixOf` splitDirectories outputRoot
      )
      (throwIO (ToolFailure "invalid" "output directory overlaps cache"))
  out <- freshOutput (optionOut options)
  let indexDigest = sha256 (stockIndexBytes stock)
  pure
    RunContext
      { contextOut = out
      , contextStock = stock
      , contextCandidatePath = optionCandidate options
      , contextCandidateBytes = candidateBytes
      , contextCandidateDigest = sha256 candidateBytes
      , contextSymbolsPath = optionSymbols options
      , contextSymbolsDigest = sha256 symbolsBytes
      , contextFunctions = parseSymbolFunctions symbolsBytes
      , contextRunner = runner
      , contextRunnerDigest = runnerDigest
      , contextRunnerImmutable = "/nix/store/" `isPrefixOf` runner
      , contextIndexPath = stockIndexPath (optionStockPaths options)
      , contextIndexDigest = indexDigest
      , contextCache = optionCache options
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
    , "runner_immutable" .= contextRunnerImmutable context
    , "cache" .= contextCache context
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

terminalValue :: String -> Int -> Int -> Int -> Int -> Int -> [String] -> [Value] -> Maybe FilePath -> Maybe FilePath -> FilePath -> Value
terminalValue verdict generated compared matched inconclusive differed boundaries evidence originalCase minimizedCase reportPath =
  object
    [ "verdict" .= verdict
    , "generated" .= generated
    , "compared" .= compared
    , "matched" .= matched
    , "inconclusive" .= inconclusive
    , "differed" .= differed
    , "boundaries" .= boundaries
    , "inconclusive_evidence" .= evidence
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

minimalityLabel :: ReductionStatus -> String
minimalityLabel status = case status of
  FixedPoint -> "local"
  _ -> "not-established"

reductionValue :: ShrinkResult -> Value
reductionValue result =
  object
    [ "status" .= reductionStatusLabel (reductionStatus result)
    , "trials" .= trialCount result
    , "minimality" .= minimalityLabel (reductionStatus result)
    ]

listInconclusive :: FilePath -> IO [Value]
listInconclusive root = do
  present <- doesDirectoryExist root
  if not present
    then pure []
    else do
      names <- sort <$> listDirectory root
      pure
        [ object
            [ "class" .= name
            , "case" .= (root </> name </> "case.json")
            , "diagnostic" .= (root </> name </> "diagnostic.json")
            , "native" .= (root </> name)
            ]
        | name <- names
        ]

terminalFailure :: Maybe (Options, RunContext) -> String -> String -> IO a
terminalFailure scope verdict reason = do
  let bounded = boundedText 2048 reason
  case scope of
    Nothing -> do
      B.putStrLn
        ( encodeCanonical
            ( object
                [ "schema_version" .= (1 :: Int)
                , "tool" .= ("record-store-counterexample" :: String)
                , "verdict" .= verdict
                , "reason" .= bounded
                , "exit_code" .= (2 :: Int)
                ]
            )
        )
      exitWith (ExitFailure 2)
    Just (options, context) -> do
      let reportPath = contextOut context </> "report.json"
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdict
              , "reason" .= bounded
              , "identity" .= identityObject options context
              ]
          terminal =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdict
              , "reason" .= bounded
              , "report" .= reportPath
              , "exit_code" .= (2 :: Int)
              ]
      B.writeFile reportPath (encodeCanonical report)
      B.putStrLn (encodeCanonical terminal)
      exitWith (ExitFailure 2)

boundedText :: Int -> String -> String
boundedText limit text
  | length text <= limit = text
  | otherwise = take limit text ++ "..."

finish :: Options -> RunContext -> Value -> Value -> Int -> IO ()
finish options context report terminal code = do
  drift <- runnerDrift context
  case drift of
    Just reason -> terminalFailure (Just (options, context)) "infrastructure" reason
    Nothing -> do
      B.writeFile (contextOut context </> "report.json") (encodeCanonical report)
      B.putStrLn (encodeCanonical terminal)
      exitWith (if code == 0 then ExitSuccess else ExitFailure code)

runnerDrift :: RunContext -> IO (Maybe String)
runnerDrift context
  | contextRunnerImmutable context = pure Nothing
  | otherwise = do
      digest <- sha256File (contextRunner context)
      pure (if digest == contextRunnerDigest context then Nothing else Just "runner changed during the run")

runPairAt :: RunContext -> String -> ValidCase -> IO PairOutcome
runPairAt context name valid = do
  let c = oracleCase valid
      spec = caseSpec (stockImage (contextStock context)) (stockLoader (contextStock context)) (stockFunctions (contextStock context)) c
      candidate = candidateSpec (contextCandidateBytes context) (contextFunctions context) spec
  runCasePair (contextRunner context) (contextCache context) (contextOut context </> name) spec candidate

runLoop :: Options -> RunContext -> IO ()
runLoop options context = do
  config <-
    either
      (invalid . ("generation config: " ++) . show)
      pure
      (mkGenerationConfig (GenerationSeed (optionSeed options)) (optionCaseCount options) (optionOperationBound options))
  let validCases = generateCases config
  corpus <- runCorpus context validCases
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
          terminal = terminalValue "infrastructure" (length validCases) 0 0 0 0 [] [] Nothing Nothing (contextOut context </> "report.json")
      finish options context report terminal 2
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
          terminal = terminalValue "prerequisite" (length validCases) 0 0 0 0 [] [] Nothing Nothing (contextOut context </> "report.json")
      finish options context report terminal 2
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
          terminal = terminalValue verdict (length validCases) compared matched (length reasons) 0 [] [] Nothing Nothing (contextOut context </> "report.json")
      finish options context report terminal (if null reasons then 0 else 2)
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
              , trialCache = contextCache context
              , trialCounter = counter
              }
          boundaries = nub (map roleBoundary differences)
          countsFor differed = countsValue (length validCases) (matched + length inconclusive + 1) matched (length inconclusive) differed
      primary <- case listToMaybe boundaries of
        Just boundary -> pure boundary
        Nothing -> invalid "candidate difference has no comparison boundary"
      shrunk <- trySync (shrinkCase budget primary (mkTrial trialRunner) valid)
      result <- case shrunk of
        Left exception -> throwIO (ToolFailure "infrastructure" ("minimization failed: " ++ displayException exception))
        Right value -> pure value
      removeDirectoryIfPresent (contextOut context </> "trials")
      inconclusiveExamples <- listInconclusive (contextOut context </> "inconclusive")
      let originalCasePath = contextOut context </> "original.case.json"
      B.writeFile originalCasePath (encodeCase valid)
      originalOutcome <- runPairAt context "original" valid
      (originalStock', originalCandidate') <- case originalOutcome of
        PairFailed message -> throwIO (ToolFailure "infrastructure" ("original replay failed: " ++ message))
        PairObserved stockObservation candidateObservation -> pure (stockObservation, candidateObservation)
      let originalVerdict = replayVerdict valid originalStock' originalCandidate'
      originalDifferences <- case originalVerdict of
        CaseDiffers observed | primary `elem` map roleBoundary observed -> pure observed
        _ -> do
          let reason = case originalVerdict of
                CaseMatches -> "original case no longer differs"
                CaseDiffers _ -> "original case no longer reproduces the boundary"
                CaseStockModelMismatch _ -> "original case stock differs from the model"
                CaseUnsupported text -> "original case is unsupported: " ++ text
              verdict = case originalVerdict of
                CaseStockModelMismatch _ -> "prerequisite"
                CaseUnsupported _ -> "unsupported"
                _ -> "inconclusive"
              observed = case originalVerdict of
                CaseDiffers observedDifferences -> observedDifferences
                _ -> []
              modelProblems = case originalVerdict of
                CaseStockModelMismatch problems -> problems
                _ -> []
              differed = case originalVerdict of
                CaseDiffers _ -> 1
                _ -> 0
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= verdict
                  , "reason" .= reason
                  , "counts" .= countsFor differed
                  , "case" .= caseName (name (oracleCase valid))
                  , "model_differences" .= map modelDifferenceJson modelProblems
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel boundaries
                        , "original" .= observationValue originalStock' originalCandidate' observed
                        ]
                  , "reduction" .= reductionValue result
                  , "inconclusive_evidence" .= inconclusiveExamples
                  , "saved_case" .= originalCasePath
                  , "paths" .=
                      object
                        [ "original_case" .= originalCasePath
                        , "original_native" .= (contextOut context </> "original")
                        , "report" .= (contextOut context </> "report.json")
                        ]
                  , "identity" .= identityObject options context
                  ]
              terminal =
                terminalValue
                  verdict
                  (length validCases)
                  (matched + length inconclusive + 1)
                  matched
                  (length inconclusive)
                  differed
                  (map boundaryLabel boundaries)
                  inconclusiveExamples
                  (Just originalCasePath)
                  Nothing
                  (contextOut context </> "report.json")
          finish options context report terminal 2
      case reductionStatus result of
        OriginalDoesNotReproduce -> do
          let reportPath = contextOut context </> "report.json"
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= ("inconclusive" :: String)
                  , "reason" .= ("reduction baseline did not reproduce" :: String)
                  , "counts" .= countsFor 1
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                        , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                        ]
                  , "reduction" .= reductionValue result
                  , "inconclusive_evidence" .= inconclusiveExamples
                  , "paths" .=
                      object
                        [ "original_case" .= originalCasePath
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
                  inconclusiveExamples
                  (Just originalCasePath)
                  Nothing
                  reportPath
          finish options context report terminal 2
        _ -> pure ()
      let minimized = minimizedCase result
          minimizedCasePath = contextOut context </> "minimized.case.json"
      B.writeFile minimizedCasePath (encodeCase minimized)
      minimizedOutcome <- runPairAt context "minimized" minimized
      (minimizedStock, minimizedCandidate) <- case minimizedOutcome of
        PairFailed message -> throwIO (ToolFailure "infrastructure" ("minimized replay failed: " ++ message))
        PairObserved stockObservation candidateObservation -> pure (stockObservation, candidateObservation)
      let minimizedVerdict = replayVerdict minimized minimizedStock minimizedCandidate
      minimizedDifferences <- case minimizedVerdict of
        CaseDiffers observed | primary `elem` map roleBoundary observed -> pure observed
        _ -> do
          let reason = case minimizedVerdict of
                CaseMatches -> "minimized case no longer differs"
                CaseDiffers _ -> "minimized case does not reproduce the original boundary"
                CaseStockModelMismatch _ -> "minimized case stock differs from the model"
                CaseUnsupported text -> "minimized case is unsupported: " ++ text
              verdict = case minimizedVerdict of
                CaseStockModelMismatch _ -> "prerequisite"
                CaseUnsupported _ -> "unsupported"
                _ -> "inconclusive"
              observed = case minimizedVerdict of
                CaseDiffers observedDifferences -> observedDifferences
                _ -> []
              modelProblems = case minimizedVerdict of
                CaseStockModelMismatch problems -> problems
                _ -> []
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= verdict
                  , "reason" .= reason
                  , "counts" .= countsFor 1
                  , "case" .= caseName (name (oracleCase minimized))
                  , "model_differences" .= map modelDifferenceJson modelProblems
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel boundaries
                        , "minimized" .= observationValue minimizedStock minimizedCandidate observed
                        ]
                  , "reduction" .= reductionValue result
                  , "inconclusive_evidence" .= inconclusiveExamples
                  , "saved_case" .= minimizedCasePath
                  , "paths" .=
                      object
                        [ "original_case" .= originalCasePath
                        , "minimized_case" .= minimizedCasePath
                        , "minimized_native" .= (contextOut context </> "minimized")
                        , "report" .= (contextOut context </> "report.json")
                        ]
                  , "identity" .= identityObject options context
                  ]
              terminal =
                terminalValue
                  verdict
                  (length validCases)
                  (matched + length inconclusive + 1)
                  matched
                  (length inconclusive)
                  1
                  (map boundaryLabel boundaries)
                  inconclusiveExamples
                  (Just originalCasePath)
                  (Just minimizedCasePath)
                  (contextOut context </> "report.json")
          finish options context report terminal 2
      let reportPath = contextOut context </> "report.json"
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("counterexample" :: String)
              , "counts" .= countsFor 1
              , "inconclusive_reasons" .= inconclusive
              , "failure" .=
                  object
                    [ "boundary" .= boundaryLabel primary
                    , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                    , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                    , "minimized" .= observationValue minimizedStock minimizedCandidate minimizedDifferences
                    ]
              , "reduction" .= reductionValue result
              , "inconclusive_evidence" .= inconclusiveExamples
              , "paths" .=
                  object
                    [ "original_case" .= originalCasePath
                    , "minimized_case" .= minimizedCasePath
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
              inconclusiveExamples
              (Just originalCasePath)
              (Just minimizedCasePath)
              reportPath
      finish options context report terminal 1

runReplay :: Options -> RunContext -> FilePath -> IO ()
runReplay options context file = do
  bytes <- B.readFile file
  valid <- either (invalid . ("case: " ++) . show) pure (decodeCase bytes)
  outcome <- runPairAt context "replay" valid
  (stockObservation, candidateObservation) <- case outcome of
    PairFailed message -> throwIO (ToolFailure "infrastructure" ("replay failed: " ++ message))
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
          terminal = terminalValue "matched" 1 1 1 0 0 [] [] Nothing Nothing reportPath
      finish options context report terminal 0
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
          terminal = terminalValue "counterexample" 1 1 0 0 1 (map boundaryLabel boundaries) [] Nothing Nothing reportPath
      finish options context report terminal 1
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
      finish options context report (terminalValue "prerequisite" 1 1 0 0 0 [] [] Nothing Nothing reportPath) 2
    CaseUnsupported reason -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= ("unsupported" :: String)
              , "case" .= caseName (name (oracleCase valid))
              , "reason" .= reason
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
      finish options context report (terminalValue "unsupported" 1 1 0 1 0 [] [] Nothing Nothing reportPath) 2

runCorpus :: RunContext -> [ValidCase] -> IO CorpusResult
runCorpus context validCases =
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
      stockObserved <- observeBatch context stockDirectory specs
      case stockObserved of
        Left problem -> pure (CorpusAborted problem)
        Right stockObservations -> do
          let stockValues = map snd stockObservations
              rows = zip3 chunk specs stockValues
              mismatches =
                [ (valid, spec, problems)
                | (valid, spec, observation) <- rows
                , nativeStatus observation == Returned
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
              candidateObserved <- observeBatch context candidateDirectory candidateSpecs
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

observeBatch :: RunContext -> FilePath -> [CaseSpec] -> IO (Either String [(CaseId, NativeObservation)])
observeBatch context directory specs = do
  outcome <-
    trySync
      ( do
          batchPath <- writeBatch directory specs
          patchStubs directory specs
          case contextCache context of
            Nothing -> loadObservedBatch (contextRunner context) batchPath
            Just root -> snd <$> cachedObservedBatch root (contextRunner context) batchPath
      )
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
