{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Deterministic generated-corpus -> stock/model prerequisite -> candidate comparison ->
-- boundary-preserving minimization. One terminal JSON; exit 0 pass, 1 behavioural
-- counterexample, 2 invalid/prerequisite/infrastructure/unsupported.
module Main (main) where

import Control.Exception (Exception, IOException, displayException, fromException, throwIO, try)
import Control.Monad (filterM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.IORef (newIORef)
import Data.List (intercalate, isPrefixOf, nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.Word (Word64)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.NativeObservation (NativeObservation (..), RunnerStatus (..), runnerStatusLabel)
import Emulation.OwnedCase (CaseId, CaseSpec, caseId, loadObservedBatch, writeBatch)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModPaths (fujimodNativeCase)
import Replacement.RecordStore.CaseJson (decodeCase, encodeCase)
import Replacement.RecordStore.Cases (caseName, name)
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Generate (GenerationSeed (..), ValidCase, generateCases, mkGenerationConfig, oracleCase)
import Replacement.RecordStore.Harness (Function, caseSpec)
import Replacement.RecordStore.Minimize hiding (Inconclusive)
import System.Directory hiding (isSymbolicLink)
import System.Environment (getArgs, getExecutablePath, lookupEnv, setEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, normalise, splitDirectories, takeDirectory, (</>))
import System.Posix.Files (FileStatus, getSymbolicLinkStatus, isDirectory, isSymbolicLink, readSymbolicLink, setFileMode)
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

data CorpusInconclusive = CorpusInconclusive
  { inconclusiveCase :: ValidCase
  , inconclusiveReason :: String
  , inconclusiveClass :: InconclusiveClass
  }

data CorpusResult
  = CorpusPassed Int [CorpusInconclusive]
  | CorpusFound Int [CorpusInconclusive] ValidCase [RoleDifference] NativeObservation NativeObservation
  | CorpusStockMismatch Int [CorpusInconclusive] ValidCase [ModelDifference]
  | CorpusAborted Int [CorpusInconclusive] String

data Verdict
  = Pass
  | Matched
  | Counterexample
  | Prerequisite
  | Unsupported
  | Inconclusive
  | Invalid
  | Infrastructure
  deriving (Eq, Show)

verdictLabel :: Verdict -> String
verdictLabel Pass = "pass"
verdictLabel Matched = "matched"
verdictLabel Counterexample = "counterexample"
verdictLabel Prerequisite = "prerequisite"
verdictLabel Unsupported = "unsupported"
verdictLabel Inconclusive = "inconclusive"
verdictLabel Invalid = "invalid"
verdictLabel Infrastructure = "infrastructure"

verdictExitCode :: Verdict -> Int
verdictExitCode Pass = 0
verdictExitCode Matched = 0
verdictExitCode Counterexample = 1
verdictExitCode _ = 2

data ToolFailureKind
  = FailureInvalid
  | FailurePrerequisite
  | FailureUnsupported
  | FailureInfrastructure
  deriving (Eq, Show)

toolFailureVerdict :: ToolFailureKind -> Verdict
toolFailureVerdict FailureInvalid = Invalid
toolFailureVerdict FailurePrerequisite = Prerequisite
toolFailureVerdict FailureUnsupported = Unsupported
toolFailureVerdict FailureInfrastructure = Infrastructure

data ToolFailure = ToolFailure ToolFailureKind String

instance Show ToolFailure where
  show (ToolFailure kind reason) = verdictLabel (toolFailureVerdict kind) ++ ": " ++ reason

instance Exception ToolFailure

main :: IO ()
main = do
  args <- getArgs
  anchorResult <- trySync captureAnchor
  anchor <- case anchorResult of
    Left exception -> terminalFailure Nothing FailureInfrastructure (displayException exception)
    Right value -> pure value
  workspaceResult <- trySync (lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory)
  case workspaceResult of
    Left exception -> terminalFailure Nothing FailureInfrastructure (displayException exception)
    Right () -> pure ()
  raw <- case parseOptions args of
    Left message -> terminalFailure Nothing FailureInvalid message
    Right value -> pure value
  resolved <- trySync (resolveOptions anchor raw)
  options <- case resolved of
    Left exception -> terminalFailure Nothing FailureInvalid (displayException exception)
    Right (Left message) -> terminalFailure Nothing FailureInvalid message
    Right (Right value) -> pure value
  setEnv "XM1_STORAGE_PATH" (takeDirectory (optionOut options))
  admitted <- trySync (slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy (reexecArguments options))
  case admitted of
    Left exception -> terminalFailure Nothing FailureInfrastructure (displayException exception)
    Right () -> run options

invalid :: String -> IO a
invalid message = throwIO (ToolFailure FailureInvalid message)

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
  roots <- privateRoots
  candidate <- resolveInput anchor (rawCandidate raw) candidateRelative
  symbols <- resolveInput anchor (rawSymbols raw) symbolsRelative
  runner <- resolveRunner anchor (rawRunner raw)
  image <- makeAbsolute (rawImage raw)
  loader <- makeAbsolute (rawLoader raw)
  index <- makeAbsolute (rawIndex raw)
  replay <- traverse makeAbsolute (rawReplay raw)
  cache <- case rawCache raw of
    Nothing -> pure (Right Nothing)
    Just path -> fmap (fmap Just) (admitCache roots path)
  admittedOut <- admitOutput roots (rawOut raw)
  case (candidate, symbols, runner) of
    (Right candidatePath, Right symbolsPath, Right runnerPath) ->
      case admittedOut of
        Left message -> pure (Left message)
        Right outPath ->
          case cache of
            Left message -> pure (Left message)
            Right cachePath -> do
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
                          , optionCache = cachePath
                          }
                    )
    _ -> pure (Left (firstLeft [candidate, symbols, runner]))

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

data PrivateRoots = PrivateRoots
  { scratchRoot :: FilePath
  , runRoot :: FilePath
  }

privateRoots :: IO PrivateRoots
privateRoots = do
  workspace <- getCurrentDirectory >>= canonicalizePath
  private <- canonicalExisting (workspace </> "private-data")
  scratch <- canonicalExisting (private </> "scratch")
  runs <- canonicalExisting (private </> "verify-candidate" </> "runs")
  let inside root = splitDirectories private `isPrefixOf` splitDirectories root
  unless (inside scratch && inside runs) $
    throwIO (ToolFailure FailureInvalid "scratch and run roots resolve outside private-data")
  pure (PrivateRoots scratch runs)

canonicalExisting :: FilePath -> IO FilePath
canonicalExisting path = do
  absolute <- makeAbsolute path
  resolve 0 (splitDirectories absolute) "/"
  where
    resolve :: Int -> [FilePath] -> FilePath -> IO FilePath
    resolve depth _ _ | depth > 64 = ioError (userError "symlink depth exceeded")
    resolve _ [] current = pure current
    resolve depth (component : rest) current = do
      let candidate = current </> component
      status <- try (getSymbolicLinkStatus candidate) :: IO (Either IOException FileStatus)
      case status of
        Left _ -> pure (foldl (</>) candidate rest)
        Right entry
          | isSymbolicLink entry -> do
              target <- readSymbolicLink candidate
              let resolved = if isAbsolute target then target else current </> target
              resolve (depth + 1) (splitDirectories (normalise resolved) ++ rest) "/"
          | isDirectory entry -> resolve depth rest candidate
          | otherwise -> pure (foldl (</>) candidate rest)

-- | Canonicalize a path that must stay beneath a named private root. Traversal
-- components are refused before resolution, and a symlink in the existing
-- prefix resolves before the prefix check, so a suffix that does not exist yet
-- cannot carry the destination outside the root.
admitPrivatePath :: PrivateRoots -> String -> Bool -> FilePath -> IO (Either String FilePath)
admitPrivatePath roots label allowRoot path = do
  absolute <- makeAbsolute path
  if ".." `elem` splitDirectories absolute
    then pure (Left (label ++ " must not contain '..' components: " ++ absolute))
    else do
      destination <- canonicalExisting absolute
      let components = splitDirectories destination
          rootComponents = map splitDirectories [scratchRoot roots, runRoot roots]
          strict = any (\root -> root `isPrefixOf` components && length components > length root) rootComponents
          equal = any (== components) rootComponents
      pure
        ( if strict || (allowRoot && equal)
            then Right destination
            else Left (label ++ " must resolve under private-data/scratch or private-data/verify-candidate/runs: " ++ destination)
        )

admitOutput :: PrivateRoots -> FilePath -> IO (Either String FilePath)
admitOutput roots path = do
  admitted <- admitPrivatePath roots "output" False path
  case admitted of
    Left message -> pure (Left message)
    Right destination -> do
      exists <- doesPathExist destination
      pure (if exists then Left (destination ++ " already exists") else Right destination)

admitCache :: PrivateRoots -> FilePath -> IO (Either String FilePath)
admitCache roots path = admitPrivatePath roots "cache" True path

overlaps :: FilePath -> FilePath -> Bool
overlaps left right =
  splitDirectories left `isPrefixOf` splitDirectories right
    || splitDirectories right `isPrefixOf` splitDirectories left

revalidatePrivate :: PrivateRoots -> String -> Bool -> FilePath -> IO FilePath
revalidatePrivate roots label allowRoot path = do
  resolved <- canonicalizePath path
  admitted <- admitPrivatePath roots label allowRoot resolved
  either (throwIO . ToolFailure FailureInvalid) pure admitted

freshOutput :: PrivateRoots -> FilePath -> IO FilePath
freshOutput roots path = do
  destination <- admitOutput roots path >>= either (throwIO . ToolFailure FailureInvalid) pure
  checkStorageBudget destination
  createDirectoryIfMissing True (takeDirectory destination)
  createDirectory destination
  setFileMode destination 0o700
  revalidatePrivate roots "output" False destination

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
      | "--" `isPrefixOf` argument = Left ("unknown argument " ++ argument)
      | otherwise = Left ("unexpected argument " ++ argument)

-- | Numeric arguments are read as 'Integer' and range-checked before conversion, so a
-- literal outside 'Int' or 'Word64' is refused instead of being converted to a different
-- value.
parseInteger :: String -> Either String Integer
parseInteger text = maybe (Left ("invalid integer: " ++ text)) Right (readMaybe text)

parseBounded :: String -> Integer -> Integer -> String -> Either String Int
parseBounded label low high text = do
  value <- parseInteger text
  if value < low || value > high
    then Left (label ++ " must be " ++ show low ++ ".." ++ show high)
    else Right (fromInteger value)

parseSeed :: String -> Either String Word64
parseSeed text = do
  value <- parseInteger text
  if value < 0 || value > toInteger (maxBound :: Word64)
    then Left ("--seed must be 0.." ++ show (maxBound :: Word64))
    else Right (fromInteger value)

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
  seed <- maybe (Right 0) parseSeed =<< optional "--seed"
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
      Just (ToolFailure kind reason) -> terminalFailure Nothing kind reason
      Nothing -> terminalFailure Nothing FailureInfrastructure (displayException exception)
  guardExit options context $ case optionReplay options of
    Just file -> runReplay options context file
    Nothing -> runLoop options context

guardExit :: Options -> RunContext -> IO () -> IO ()
guardExit options context action = do
  outcome <- trySync action
  case outcome of
    Right () -> pure ()
    Left exception -> case fromException exception :: Maybe ToolFailure of
      Just (ToolFailure kind reason) -> terminalFailure (Just (options, context)) kind reason
      Nothing -> case fromException exception :: Maybe ExitCode of
        Just exitCode -> throwIO exitCode
        Nothing -> terminalFailure (Just (options, context)) FailureInfrastructure (displayException exception)

buildContext :: Options -> IO RunContext
buildContext options = do
  roots <- privateRoots
  stockLoad <- trySync (loadStockInputs (optionStockPaths options))
  stock <- case stockLoad of
    Left exception -> throwIO (ToolFailure FailurePrerequisite ("stock: " ++ displayException exception))
    Right (Left problem) -> throwIO (ToolFailure FailurePrerequisite (show problem))
    Right (Right value) -> pure value
  candidateRead <- trySync (readBoundedInput candidateLimit (optionCandidate options))
  candidateBytes <- case candidateRead of
    Left exception -> throwIO (ToolFailure FailureInvalid ("candidate: " ++ displayException exception))
    Right bytes -> pure bytes
  unless (B.length candidateBytes == candidateLimit) $
    throwIO (ToolFailure FailureInvalid ("candidate span size " ++ show (B.length candidateBytes)))
  symbolsRead <- trySync (readBoundedInput symbolsLimit (optionSymbols options))
  symbolsBytes <- case symbolsRead of
    Left exception -> throwIO (ToolFailure FailureInvalid ("symbols: " ++ displayException exception))
    Right value -> pure value
  runner <- canonicalizePath (optionRunner options)
  runnerDigest <- sha256File runner
  outAdmission <- trySync (admitOutput roots (optionOut options))
  outDestination <- case outAdmission of
    Left exception -> throwIO (ToolFailure FailureInvalid ("output: " ++ displayException exception))
    Right (Left message) -> throwIO (ToolFailure FailureInvalid message)
    Right (Right value) -> pure value
  cacheAdmission <- case optionCache options of
    Nothing -> pure (Right (Right Nothing))
    Just configured -> do
      admitted <- trySync (admitCache roots configured)
      pure (fmap (fmap Just) admitted)
  cacheDestination <- case cacheAdmission of
    Left exception -> throwIO (ToolFailure FailureInvalid ("cache: " ++ displayException exception))
    Right (Left message) -> throwIO (ToolFailure FailureInvalid message)
    Right (Right value) -> pure value
  forM_ cacheDestination $ \cachedRoot ->
    when (overlaps outDestination cachedRoot) (throwIO (ToolFailure FailureInvalid "output directory overlaps cache"))
  cache <- case cacheDestination of
    Nothing -> pure Nothing
    Just destination -> do
      checkStorageBudget destination
      existed <- doesDirectoryExist destination
      createDirectoryIfMissing True destination
      unless existed (setFileMode destination 0o700)
      Just <$> revalidatePrivate roots "cache" True destination
  out <- freshOutput roots outDestination
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
      , contextCache = cache
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

terminalValue :: Verdict -> Int -> Int -> Int -> Int -> Int -> [String] -> [Value] -> Maybe FilePath -> Maybe FilePath -> FilePath -> Value
terminalValue verdict generated compared matched inconclusive differed boundaries evidence originalCase minimizedCase reportPath =
  object
    [ "verdict" .= verdictLabel verdict
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

-- | Every status has a fixed label; the payload of a blocked baseline never reaches the
-- label.
reductionStatusLabel :: ReductionStatus -> String
reductionStatusLabel FixedPoint = "fixed-point"
reductionStatusLabel UnresolvedTrials = "unresolved-trials"
reductionStatusLabel BudgetExhausted = "budget-exhausted"
reductionStatusLabel OriginalDoesNotReproduce = "original-does-not-reproduce"
reductionStatusLabel (BaselineInconclusive _) = "baseline-inconclusive"

minimalityLabel :: ReductionStatus -> String
minimalityLabel FixedPoint = "local"
minimalityLabel UnresolvedTrials = "not-established"
minimalityLabel BudgetExhausted = "not-established"
minimalityLabel OriginalDoesNotReproduce = "not-established"
minimalityLabel (BaselineInconclusive _) = "not-established"

reductionValue :: ShrinkResult -> Value
reductionValue result =
  object $
    [ "status" .= reductionStatusLabel (reductionStatus result)
    , "trials" .= trialCount result
    , "minimality" .= minimalityLabel (reductionStatus result)
    ]
      ++ [ "blocking" .= blockingValue block
         | BaselineInconclusive block <- [reductionStatus result]
         ]

blockingValue :: TrialBlock -> Value
blockingValue block =
  object
    [ "class" .= inconclusiveClassLabel (trialBlockClass block)
    , "reason" .= trialBlockText block
    ]

listInconclusive :: FilePath -> IO [(InconclusiveClass, FilePath)]
listInconclusive root = do
  present <- doesDirectoryExist root
  if not present
    then pure []
    else do
      names <- sort <$> listDirectory root
      pure [(cls, root </> name) | name <- names, Just cls <- [inconclusiveClassFromLabel name]]

inconclusiveEvidence :: (InconclusiveClass, FilePath) -> Value
inconclusiveEvidence (cls, path) =
  object
    [ "class" .= inconclusiveClassLabel cls
    , "case" .= (path </> "case.json")
    , "diagnostic" .= (path </> "diagnostic.json")
    , "native" .= path
    ]

-- | One retained example per inconclusive corpus class, with the number of cases that
-- landed in that class.
corpusEvidence :: FilePath -> [CorpusInconclusive] -> [Value]
corpusEvidence out records =
  [ object
      [ "class" .= inconclusiveClassLabel cls
      , "case" .= (path </> "case.json")
      , "case_name" .= caseName (name (oracleCase (inconclusiveCase first)))
      , "count" .= length matching
      , "diagnostic" .= (path </> "diagnostic.json")
      , "native" .= path
      , "stock" .= (path </> "stock")
      , "candidate" .= (path </> "candidate")
      ]
  | cls <- nub (map inconclusiveClass records)
  , let matching = filter ((== cls) . inconclusiveClass) records
  , first : _ <- [matching]
  , let path = out </> "inconclusive" </> inconclusiveClassLabel cls
  ]

corpusDiagnostic :: InconclusiveClass -> String -> ValidCase -> CaseSpec -> NativeObservation -> NativeObservation -> Value
corpusDiagnostic cls reason valid spec stockObservation candidateObservation =
  object
    [ "class" .= inconclusiveClassLabel cls
    , "diagnostic" .= reason
    , "case" .= caseName (name (oracleCase valid))
    , "native_case" .= caseId spec
    , "stock_status" .= runnerStatusLabel (nativeStatus stockObservation)
    , "candidate_status" .= runnerStatusLabel (nativeStatus candidateObservation)
    , "stock_input" .= ("stock" </> (caseId spec ++ ".case.json"))
    , "candidate_input" .= ("candidate" </> (caseId spec ++ ".case.json"))
    , "stock_observation" .= ("stock" </> (caseId spec ++ ".native-result.json"))
    , "candidate_observation" .= ("candidate" </> (caseId spec ++ ".native-result.json"))
    ]

terminalFailure :: Maybe (Options, RunContext) -> ToolFailureKind -> String -> IO a
terminalFailure scope kind reason = do
  let bounded = boundedText 2048 reason
      verdict = verdictLabel (toolFailureVerdict kind)
  case scope of
    Nothing -> do
      BC.putStrLn
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
      BC.putStrLn (encodeCanonical terminal)
      exitWith (ExitFailure 2)

boundedText :: Int -> String -> String
boundedText limit text
  | length text <= limit = text
  | otherwise = take limit text ++ "..."

finish :: Options -> RunContext -> Value -> Value -> Verdict -> IO a
finish options context report terminal verdict = do
  drift <- runnerDrift context
  case drift of
    Just reason -> terminalFailure (Just (options, context)) FailureInfrastructure reason
    Nothing -> do
      B.writeFile (contextOut context </> "report.json") (encodeCanonical report)
      BC.putStrLn (encodeCanonical terminal)
      exitWith (if verdictExitCode verdict == 0 then ExitSuccess else ExitFailure (verdictExitCode verdict))

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
    CorpusAborted matched inconclusive message -> do
      let evidence = corpusEvidence (contextOut context) inconclusive
          compared = matched + length inconclusive
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Infrastructure
              , "reason" .= message
              , "counts" .= countsValue (length validCases) compared matched (length inconclusive) 0
              , "inconclusive_evidence" .= evidence
              , "native_output" .= contextOut context
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue Infrastructure (length validCases) compared matched (length inconclusive) 0 [] evidence Nothing Nothing (contextOut context </> "report.json")
      finish options context report terminal Infrastructure
    CorpusStockMismatch matched inconclusive valid problems -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let evidence = corpusEvidence (contextOut context) inconclusive
          compared = matched + length inconclusive + 1
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Prerequisite
              , "case" .= caseName (name (oracleCase valid))
              , "model_differences" .= map modelDifferenceJson problems
              , "counts" .= countsValue (length validCases) compared matched (length inconclusive + 1) 0
              , "inconclusive_evidence" .= evidence
              , "corpus_native" .= (contextOut context </> "corpus-failure")
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue Prerequisite (length validCases) compared matched (length inconclusive + 1) 0 [] evidence (Just (contextOut context </> "failure.case.json")) Nothing (contextOut context </> "report.json")
      finish options context report terminal Prerequisite
    CorpusPassed matched inconclusive -> do
      let reasons = map inconclusiveReason inconclusive
          evidence = corpusEvidence (contextOut context) inconclusive
          verdict = if null reasons then Pass else Inconclusive
          compared = matched + length reasons
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel verdict
              , "counts" .= countsValue (length validCases) compared matched (length reasons) 0
              , "inconclusive_reasons" .= reasons
              , "inconclusive_evidence" .= evidence
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue verdict (length validCases) compared matched (length reasons) 0 [] evidence Nothing Nothing (contextOut context </> "report.json")
      finish options context report terminal verdict
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
        Left exception -> throwIO (ToolFailure FailureInfrastructure ("minimization failed: " ++ displayException exception))
        Right value -> pure value
      removeDirectoryIfPresent (contextOut context </> "trials")
      listed <- listInconclusive (contextOut context </> "inconclusive")
      let corpusClasses = map inconclusiveClass inconclusive
          reducerOnly = [entry | entry@(cls, _) <- listed, cls `notElem` corpusClasses]
          inconclusiveExamples = corpusEvidence (contextOut context) inconclusive ++ map inconclusiveEvidence reducerOnly
      let originalCasePath = contextOut context </> "original.case.json"
      B.writeFile originalCasePath (encodeCase valid)
      originalOutcome <- runPairAt context "original" valid
      (originalStock', originalCandidate') <- case originalOutcome of
        PairFailed message -> throwIO (ToolFailure FailureInfrastructure ("original replay failed: " ++ message))
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
                CaseStockModelMismatch _ -> Prerequisite
                CaseUnsupported _ -> Unsupported
                _ -> Inconclusive
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
                  , "verdict" .= verdictLabel verdict
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
                  , "corpus_native" .= (contextOut context </> "corpus-failure")
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
          finish options context report terminal verdict
      case reductionStatus result of
        BaselineInconclusive _ -> do
          let reportPath = contextOut context </> "report.json"
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= verdictLabel Inconclusive
                  , "reason" .= ("reduction baseline inconclusive" :: String)
                  , "counts" .= countsFor 1
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                        , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                        ]
                  , "reduction" .= reductionValue result
                  , "corpus_native" .= (contextOut context </> "corpus-failure")
                  , "inconclusive_evidence" .= inconclusiveExamples
                  , "saved_case" .= originalCasePath
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
                  Inconclusive
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
          finish options context report terminal Inconclusive
        OriginalDoesNotReproduce -> do
          let reportPath = contextOut context </> "report.json"
              report =
                object
                  [ "schema_version" .= (1 :: Int)
                  , "tool" .= ("record-store-counterexample" :: String)
                  , "verdict" .= verdictLabel Inconclusive
                  , "reason" .= ("reduction baseline did not reproduce" :: String)
                  , "counts" .= countsFor 1
                  , "failure" .=
                      object
                        [ "boundary" .= boundaryLabel primary
                        , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                        , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                        ]
                  , "reduction" .= reductionValue result
                  , "corpus_native" .= (contextOut context </> "corpus-failure")
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
                  Inconclusive
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
          finish options context report terminal Inconclusive
        _ -> pure ()
      let minimized = minimizedCase result
          minimizedCasePath = contextOut context </> "minimized.case.json"
      B.writeFile minimizedCasePath (encodeCase minimized)
      minimizedOutcome <- runPairAt context "minimized" minimized
      (minimizedStock, minimizedCandidate) <- case minimizedOutcome of
        PairFailed message -> throwIO (ToolFailure FailureInfrastructure ("minimized replay failed: " ++ message))
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
                CaseStockModelMismatch _ -> Prerequisite
                CaseUnsupported _ -> Unsupported
                _ -> Inconclusive
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
                  , "verdict" .= verdictLabel verdict
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
                  , "corpus_native" .= (contextOut context </> "corpus-failure")
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
          finish options context report terminal verdict
      let reportPath = contextOut context </> "report.json"
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Counterexample
              , "counts" .= countsFor 1
              , "inconclusive_reasons" .= map inconclusiveReason inconclusive
              , "failure" .=
                  object
                    [ "boundary" .= boundaryLabel primary
                    , "boundaries" .= map boundaryLabel (nub (map roleBoundary originalDifferences))
                    , "original" .= observationValue originalStock' originalCandidate' originalDifferences
                    , "minimized" .= observationValue minimizedStock minimizedCandidate minimizedDifferences
                    ]
              , "reduction" .= reductionValue result
              , "corpus_native" .= (contextOut context </> "corpus-failure")
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
              Counterexample
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
      finish options context report terminal Counterexample

runReplay :: Options -> RunContext -> FilePath -> IO ()
runReplay options context file = do
  replayRead <- trySync (readBoundedInput caseFileLimit file)
  bytes <- case replayRead of
    Left exception -> invalid ("case: " ++ displayException exception)
    Right value -> pure value
  valid <- either (invalid . ("case: " ++) . show) pure (decodeCase bytes)
  outcome <- runPairAt context "replay" valid
  (stockObservation, candidateObservation) <- case outcome of
    PairFailed message -> throwIO (ToolFailure FailureInfrastructure ("replay failed: " ++ message))
    PairObserved stock candidate -> pure (stock, candidate)
  let reportPath = contextOut context </> "report.json"
  case replayVerdict valid stockObservation candidateObservation of
    CaseMatches -> do
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Matched
              , "case" .= caseName (name (oracleCase valid))
              , "counts" .= countsValue 1 1 1 0 0
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue Matched 1 1 1 0 0 [] [] Nothing Nothing reportPath
      finish options context report terminal Matched
    CaseDiffers differences -> do
      let boundaries = nub (map roleBoundary differences)
          report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Counterexample
              , "case" .= caseName (name (oracleCase valid))
              , "counts" .= countsValue 1 1 0 0 1
              , "failure" .=
                  object
                    [ "boundaries" .= map boundaryLabel boundaries
                    , "observed" .= observationValue stockObservation candidateObservation differences
                    ]
              , "identity" .= identityObject options context
              ]
          terminal = terminalValue Counterexample 1 1 0 0 1 (map boundaryLabel boundaries) [] Nothing Nothing reportPath
      finish options context report terminal Counterexample
    CaseStockModelMismatch problems -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Prerequisite
              , "case" .= caseName (name (oracleCase valid))
              , "model_differences" .= map modelDifferenceJson problems
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
      finish options context report (terminalValue Prerequisite 1 1 0 0 0 [] [] Nothing Nothing reportPath) Prerequisite
    CaseUnsupported reason -> do
      B.writeFile (contextOut context </> "failure.case.json") (encodeCase valid)
      let report =
            object
              [ "schema_version" .= (1 :: Int)
              , "tool" .= ("record-store-counterexample" :: String)
              , "verdict" .= verdictLabel Unsupported
              , "case" .= caseName (name (oracleCase valid))
              , "reason" .= reason
              , "saved_case" .= (contextOut context </> "failure.case.json")
              , "identity" .= identityObject options context
              ]
      finish options context report (terminalValue Unsupported 1 1 0 1 0 [] [] Nothing Nothing reportPath) Unsupported

runCorpus :: RunContext -> [ValidCase] -> IO CorpusResult
runCorpus context validCases =
  go 0 [] (zip [0 :: Int ..] (chunks 4 validCases))
  where
    out = contextOut context
    go :: Int -> [CorpusInconclusive] -> [(Int, [ValidCase])] -> IO CorpusResult
    go matched inconclusive [] = pure (CorpusPassed matched inconclusive)
    go matched inconclusive ((chunkIndex, chunk) : rest) = do
      let specs =
            [ caseSpec (stockImage (contextStock context)) (stockLoader (contextStock context)) (stockFunctions (contextStock context)) (oracleCase valid)
            | valid <- chunk
            ]
          stockDirectory = out </> "stock-" ++ show chunkIndex
          candidateDirectory = out </> "candidate-" ++ show chunkIndex
      stockObserved <- observeBatch context stockDirectory specs
      case stockObserved of
        Left problem -> pure (CorpusAborted matched inconclusive problem)
        Right stockObservations -> do
          let stockValues = map snd stockObservations
              rows = zip3 chunk specs stockValues
              mismatches =
                [ (valid, problems)
                | (valid, _, observation) <- rows
                , nativeStatus observation == Returned
                , let problems = modelDifferences (oracleCase valid) observation
                , not (null problems)
                ]
          case mismatches of
            ((valid, problems) : _) -> do
              preserveFailureChunk out stockDirectory candidateDirectory
              pure (CorpusStockMismatch matched inconclusive valid problems)
            [] -> do
              let candidateSpecs = [candidateSpec (contextCandidateBytes context) (contextFunctions context) spec | spec <- specs]
              candidateObserved <- observeBatch context candidateDirectory candidateSpecs
              case candidateObserved of
                Left problem -> pure (CorpusAborted matched inconclusive problem)
                Right candidateObservations -> do
                  let evaluated =
                        [ (valid, spec, stockObservation, candidateObservation, evaluateCase (oracleCase valid) stockObservation candidateObservation)
                        | ((valid, spec, stockObservation), candidateObservation) <- zip rows (map snd candidateObservations)
                        ]
                  walked <- walkCorpus context stockDirectory candidateDirectory matched inconclusive evaluated
                  case walked of
                    Left result -> do
                      preserveFailureChunk out stockDirectory candidateDirectory
                      pure result
                    Right (matched', inconclusive') -> do
                      cleanChunk out chunkIndex
                      go matched' inconclusive' rest

walkCorpus :: RunContext -> FilePath -> FilePath -> Int -> [CorpusInconclusive] -> [(ValidCase, CaseSpec, NativeObservation, NativeObservation, CaseVerdict)] -> IO (Either CorpusResult (Int, [CorpusInconclusive]))
walkCorpus _ _ _ matched inconclusive [] = pure (Right (matched, inconclusive))
walkCorpus context stockDirectory candidateDirectory matched inconclusive ((valid, spec, stockObservation, candidateObservation, verdict) : more) =
  case verdict of
    CaseMatches -> walkCorpus context stockDirectory candidateDirectory (matched + 1) inconclusive more
    CaseUnsupported reason -> do
      let cls = UnsupportedNativeStatusClass
      unless (any ((== cls) . inconclusiveClass) inconclusive) $
        preserveCorpusInconclusive context stockDirectory candidateDirectory cls reason valid spec stockObservation candidateObservation
      walkCorpus context stockDirectory candidateDirectory matched (inconclusive ++ [CorpusInconclusive valid reason cls]) more
    CaseStockModelMismatch problems -> pure (Left (CorpusStockMismatch matched inconclusive valid problems))
    CaseDiffers differences -> pure (Left (CorpusFound matched inconclusive valid differences stockObservation candidateObservation))

-- | The first corpus case of an inconclusive class keeps the executed native input and
-- observation of both roles plus the canonical replay input and a diagnostic.
preserveCorpusInconclusive :: RunContext -> FilePath -> FilePath -> InconclusiveClass -> String -> ValidCase -> CaseSpec -> NativeObservation -> NativeObservation -> IO ()
preserveCorpusInconclusive context stockDirectory candidateDirectory cls reason valid spec stockObservation candidateObservation = do
  let target = contextOut context </> "inconclusive" </> inconclusiveClassLabel cls
      candidate = candidateSpec (contextCandidateBytes context) (contextFunctions context) spec
  exists <- doesDirectoryExist target
  unless exists $ do
    createDirectoryIfMissing True target
    _ <- writeBatch (target </> "stock") [spec]
    patchStubs (target </> "stock") [spec]
    _ <- writeBatch (target </> "candidate") [candidate]
    patchStubs (target </> "candidate") [candidate]
    B.writeFile (target </> "case.json") (encodeCase valid)
    B.writeFile (target </> "diagnostic.json") (encodeCanonical (corpusDiagnostic cls reason valid spec stockObservation candidateObservation))
    copyNativeResult stockDirectory (target </> "stock") (caseId spec)
    copyNativeResult candidateDirectory (target </> "candidate") (caseId spec)

copyNativeResult :: FilePath -> FilePath -> String -> IO ()
copyNativeResult source target ident = do
  bytes <- B.readFile (source </> (ident ++ ".native-result.json"))
  B.writeFile (target </> (ident ++ ".native-result.json")) bytes

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

preserveFailureChunk :: FilePath -> FilePath -> FilePath -> IO ()
preserveFailureChunk out stockDirectory candidateDirectory = do
  let directory = out </> "corpus-failure"
  createDirectory directory
  renameDirectory stockDirectory (directory </> "stock")
  candidateExists <- doesDirectoryExist candidateDirectory
  when candidateExists (renameDirectory candidateDirectory (directory </> "candidate"))

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
