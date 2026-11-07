{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Shared stock/candidate native execution for the record-store oracle and the
-- counterexample loop. The comparison values are typed here and serialized only at the
-- JSON boundary.
module Replacement.RecordStore.Execution
  ( -- * Stock inputs
    StockPaths (..)
  , defaultStockPaths
  , StockInputs (..)
  , StockInputError (..)
  , loadStockInputs
  , pinned
  , readFunctions
    -- * Case construction
  , patchStubs
  , candidateSpec
  , symbolFunctions
    -- * Typed comparison
  , RegionDifference (..)
  , ModelDifference (..)
  , RoleDifference (..)
  , firstDifference
  , boundaryBytes
  , modelDifferences
  , compareRoles
  , modelDifferenceJson
  , roleDifferenceJson
  , roleBoundary
  , boundaryLabel
    -- * Verdicts
  , CaseVerdict (..)
  , evaluateCase
  , replayVerdict
  , trialFromVerdict
    -- * Native pair execution
  , PairOutcome (..)
  , runCasePair
  , TrialRunner (..)
  , mkTrial
    -- * Observation cache
  , cachedCaseInputs
  , cachedObserved
  , executionInput
  ) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString as B
import Data.Char (isUpper, toLower)
import Data.IORef (IORef, atomicModifyIORef', modifyIORef', readIORef)
import Data.List (isPrefixOf, nub)
import qualified Data.Map.Strict as M
import Data.Word (Word8)
import Emulation.NativeCase (BatchEntry (..), Region (..), loadCase, readBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Replacement.RecordStore.CachedObservation
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Generate (ValidCase, oracleCase)
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Harness
import Replacement.RecordStore.Minimize
import Replacement.RecordStore.Model
import System.Directory
  ( createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , listDirectory
  , removeDirectoryRecursive
  , renameDirectory
  )
import System.FilePath (takeDirectory, (</>))
import Text.Read (readMaybe)

-- | Canonical stock inputs and the decompiled function index.
data StockPaths = StockPaths
  { stockImagePath :: FilePath
  , stockLoaderPath :: FilePath
  , stockIndexPath :: FilePath
  }
  deriving (Eq, Show)

defaultStockPaths :: StockPaths
defaultStockPaths =
  StockPaths
    { stockImagePath = "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"
    , stockLoaderPath = "private-data/analysis/x-m1-1.50/decode/loader-40000.bin"
    , stockIndexPath = "private-data/scratch/xm1-decomp/index/55436fc85c0560dd252b0410af6f2427fe198cd3-31e27c0e12fe.tsv"
    }

data StockInputs = StockInputs
  { stockImage :: B.ByteString
  , stockLoader :: B.ByteString
  , stockFunctions :: [Function]
  }

data StockInputError
  = StockInputMissing FilePath
  | StockInputDigestMismatch FilePath
  deriving (Eq, Show)

loadStockInputs :: StockPaths -> IO (Either StockInputError StockInputs)
loadStockInputs paths = do
  image <- pinnedEither (stockImagePath paths) imageDigest
  loader <- pinnedEither (stockLoaderPath paths) loaderDigest
  case (image, loader) of
    (Left problem, _) -> pure (Left problem)
    (_, Left problem) -> pure (Left problem)
    (Right imageBytes, Right loaderBytes) -> do
      indexExists <- doesFileExist (stockIndexPath paths)
      if not indexExists
        then pure (Left (StockInputMissing (stockIndexPath paths)))
        else do
          functions <- readFunctions (stockIndexPath paths)
          pure (Right (StockInputs imageBytes loaderBytes functions))
  where
    imageDigest = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
    loaderDigest = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"

pinnedEither :: FilePath -> String -> IO (Either StockInputError B.ByteString)
pinnedEither path expected = do
  present <- doesFileExist path
  if not present
    then pure (Left (StockInputMissing path))
    else do
      bytes <- B.readFile path
      pure (if sha256Hex (sha256 bytes) == expected then Right bytes else Left (StockInputDigestMismatch path))

pinned :: FilePath -> String -> IO B.ByteString
pinned path expected = do
  bytes <- B.readFile path
  unless (sha256Hex (sha256 bytes) == expected) (error (path ++ ": SHA-256 mismatch"))
  pure bytes

readFunctions :: FilePath -> IO [Function]
readFunctions path = do
  rows <- lines <$> readFile path
  let parsed =
        [ Function address size
        | row <- rows
        , ("range" : _ : _ : start : lengthBytes : _) <- [words row]
        , Just address <- [readMaybe start]
        , Just size <- [readMaybe lengthBytes]
        , address >= 0x61c400
        , address < 0x89ec28
        ]
      outside (Function address _) = address < 0x89aa90 || address >= 0x89abe0
  pure (Function 0x89aa90 0x128 : Function 0x89abb8 0x20 : filter outside parsed)

patchStubs :: FilePath -> [CaseSpec] -> IO ()
patchStubs directory specs =
  mapM_
    ( \spec -> do
        let path = directory </> caseId spec ++ ".case.json"
        bytes <- B.readFile path
        value <- either error pure (eitherDecodeStrict' bytes)
        let stubs =
              [ object
                  [ "address" .= hexWord32 address
                  , "inputs" .= object []
                  , "outputs" .= object ["r0" .= hexWord32 0]
                  , "side_effects" .= ("registers-only" :: String)
                  ]
              | address <- [0x724b68, 0x724c00, 0x42634, 0x4263c]
              ]
        case value of
          Object fields ->
            B.writeFile
              path
              ( encodeCanonical
                  ( Object
                      ( K.insert
                          "limits"
                          (object ["instructions" .= (4000000 :: Int), "wall_ms" .= (10000 :: Int)])
                          (K.insert "stubs" (toJSON stubs) fields)
                      )
                  )
              )
          _ -> error "case object"
    )
    specs

data RegionDifference = RegionDifference
  { differenceOffset :: Int
  , differenceExpected :: Maybe Word8
  , differenceActual :: Maybe Word8
  , differenceExpectedSize :: Int
  , differenceActualSize :: Int
  }
  deriving (Eq, Show)

firstDifference :: B.ByteString -> B.ByteString -> Maybe RegionDifference
firstDifference expected actual
  | expected == actual = Nothing
  | otherwise =
      Just
        RegionDifference
          { differenceOffset = offset
          , differenceExpected = byte expected
          , differenceActual = byte actual
          , differenceExpectedSize = B.length expected
          , differenceActualSize = B.length actual
          }
  where
    offset = length (takeWhile id (B.zipWith (==) expected actual))
    byte bytes = if offset < B.length bytes then Just (B.index bytes offset) else Nothing

regionDifferenceJson :: RegionDifference -> Value
regionDifferenceJson difference =
  object
    [ "offset" .= differenceOffset difference
    , "expected" .= differenceExpected difference
    , "actual" .= differenceActual difference
    , "expected_size" .= differenceExpectedSize difference
    , "actual_size" .= differenceActualSize difference
    ]

boundaryBytes :: NativeObservation -> RegionId -> B.ByteString
boundaryBytes observation ident =
  let bytes = regionBytesOf observation ident
   in if ident == RegionId "results" then B.take resultSize bytes else bytes

data ModelDifference
  = ModelStatus RunnerStatus RunnerStatus Word32
  | ModelRegion RegionId RegionDifference
  | ModelRegister Register Word32 Word32
  deriving (Eq, Show)

data RoleDifference
  = RoleStatus RunnerStatus RunnerStatus
  | RoleRegion RegionId RegionDifference
  | RoleRegister Register
  deriving (Eq, Show)

modelDifferences :: OracleCase -> NativeObservation -> [ModelDifference]
modelDifferences c observed =
  [ModelStatus expectedRunStatus (nativeStatus observed) (registerValue observed Pc) | nativeStatus observed /= expectedRunStatus]
    ++ [ModelRegion region difference | (region, bytes) <- expectedRegions, Just difference <- [firstDifference bytes (boundaryBytes observed region)]]
    ++ [ModelRegister register value (registerValue observed register) | not interrupted, (register, value) <- preserved, registerValue observed register /= value]
  where
    expected = model hardware c
    state = expectedStore expected
    p = persistence state
    interrupted = stopped p
    expectedRunStatus = if interrupted then MemoryGuard else Returned
    requestBytes = B.concat (map encodeRequest (requests p))
    cbBytes = B.concat (map encodeCallback (callbackEvents state))
    expectedRegions =
      [ (RegionId "results", expectedResults expected)
      , (RegionId "caller", expectedCaller expected)
      , (RegionId "medium", expectedMedium c expected)
      , (RegionId "source", sourceBytes (geometry (kind c)))
      , (RegionId "requests", requestBytes <> B.replicate (requestSize - B.length requestBytes) 0)
      , (RegionId "callbacks", cbBytes <> B.replicate (callbackSize - B.length cbBytes) 0)
      ]
    preserved = (Sp, stackTop) : zip [R4, R5, R6, R7, R8, R9, R10, R11] [0x44440004 .. 0x4444000b]

compareRoles :: NativeObservation -> NativeObservation -> [RoleDifference]
compareRoles stock candidate =
  [RoleStatus (nativeStatus stock) (nativeStatus candidate) | nativeStatus stock /= nativeStatus candidate]
    ++ [ RoleRegion ident difference
       | ident <- map RegionId ["results", "caller", "medium", "source", "requests", "callbacks"]
       , Just difference <- [firstDifference (boundaryBytes stock ident) (boundaryBytes candidate ident)]
       ]
    ++ [ RoleRegister register
       | nativeStatus stock == Returned
       , register <- [R4, R5, R6, R7, R8, R9, R10, R11, Sp]
       , registerValue stock register /= registerValue candidate register
       ]

modelDifferenceJson :: ModelDifference -> Value
modelDifferenceJson (ModelStatus expected actual pc) =
  object
    [ "observation" .= ("status" :: String)
    , "expected" .= runnerStatusLabel expected
    , "actual" .= runnerStatusLabel actual
    , "pc" .= hexWord32 pc
    ]
modelDifferenceJson (ModelRegion region difference) =
  object ["observation" .= regionName region, "difference" .= regionDifferenceJson difference]
modelDifferenceJson (ModelRegister register expected actual) =
  object
    [ "observation" .= registerLabel register
    , "expected" .= hexWord32 expected
    , "actual" .= hexWord32 actual
    ]

roleDifferenceJson :: RoleDifference -> Value
roleDifferenceJson (RoleStatus stock candidate) =
  object
    [ "observation" .= ("status" :: String)
    , "stock" .= runnerStatusLabel stock
    , "candidate" .= runnerStatusLabel candidate
    ]
roleDifferenceJson (RoleRegion ident difference) =
  object ["observation" .= regionName ident, "difference" .= regionDifferenceJson difference]
roleDifferenceJson (RoleRegister register) =
  object ["observation" .= registerLabel register]

roleBoundary :: RoleDifference -> Boundary
roleBoundary (RoleStatus _ _) = Outcome
roleBoundary (RoleRegion ident _) = regionBoundary ident
roleBoundary (RoleRegister register) = Preserved register

regionBoundary :: RegionId -> Boundary
regionBoundary (RegionId name) = case name of
  "results" -> Results
  "caller" -> Caller
  "medium" -> Medium
  "source" -> Source
  "requests" -> Requests
  "callbacks" -> Callbacks
  other -> error ("unexpected comparison region " ++ other)

boundaryLabel :: Boundary -> String
boundaryLabel Outcome = "outcome"
boundaryLabel Results = "results"
boundaryLabel Caller = "caller"
boundaryLabel Medium = "medium"
boundaryLabel Source = "source"
boundaryLabel Requests = "requests"
boundaryLabel Callbacks = "callbacks"
boundaryLabel (Preserved register) = "preserved:" ++ registerLabel register

-- | The candidate comparison policy: the stock role must match the independent model and
-- return; a candidate memory or fetch guard against a returning stock is a behavioural
-- outcome failure; any other non-returning role is unsupported rather than a difference.
data CaseVerdict
  = CaseMatches
  | CaseDiffers [RoleDifference]
  | CaseStockModelMismatch [ModelDifference]
  | CaseUnsupported String
  deriving (Eq, Show)

evaluateCase :: OracleCase -> NativeObservation -> NativeObservation -> CaseVerdict
evaluateCase c stock candidate
  | not (null modelProblems) = CaseStockModelMismatch modelProblems
  | nativeStatus stock /= Returned = CaseUnsupported ("stock status " ++ runnerStatusLabel (nativeStatus stock))
  | nativeStatus candidate == MemoryGuard || nativeStatus candidate == FetchGuard =
      CaseDiffers [RoleStatus (nativeStatus stock) (nativeStatus candidate)]
  | nativeStatus candidate /= Returned = CaseUnsupported ("candidate status " ++ runnerStatusLabel (nativeStatus candidate))
  | null differences = CaseMatches
  | otherwise = CaseDiffers differences
  where
    modelProblems = modelDifferences c stock
    differences = compareRoles stock candidate

-- | Replay uses the same validator, model and comparator as generation.
replayVerdict :: ValidCase -> NativeObservation -> NativeObservation -> CaseVerdict
replayVerdict valid = evaluateCase (oracleCase valid)

trialFromVerdict :: CaseVerdict -> Trial
trialFromVerdict CaseMatches = Matches
trialFromVerdict (CaseDiffers differences) = Differs (nub (map roleBoundary differences))
trialFromVerdict (CaseStockModelMismatch _) = Inconclusive (StockModelDisagreement "stock differs from the independent model")
trialFromVerdict (CaseUnsupported reason) = Inconclusive (UnsupportedNativeStatus reason)

data PairOutcome
  = PairObserved NativeObservation NativeObservation
  | PairFailed String
  deriving (Eq, Show)

-- | Runs one stock case and its candidate under the same batch and returns both
-- observations. The case ids are fixed so the pair can share one runner process.
runCasePair :: FilePath -> FilePath -> CaseSpec -> CaseSpec -> IO PairOutcome
runCasePair runner directory stockSpec candidateSpec0 = do
  let stockCase = stockSpec {caseId = "stock"}
      candidateCase = candidateSpec0 {caseId = "candidate"}
      specs = [stockCase, candidateCase]
  outcome <-
    try
      ( do
          createDirectoryIfMissing True directory
          batchPath <- writeBatch directory specs
          patchStubs directory specs
          observed <- loadObservedBatch runner batchPath
          let table = M.fromList [(caseIdText ident, observation) | (ident, observation) <- observed]
          case (M.lookup "stock" table, M.lookup "candidate" table) of
            (Just stock, Just candidate) -> pure (PairObserved stock candidate)
            _ -> pure (PairFailed "native batch did not return both roles")
      )
      :: IO (Either SomeException PairOutcome)
  case outcome of
    Left exception -> pure (PairFailed (displayException exception))
    Right result -> pure result

data TrialRunner = TrialRunner
  { trialRunnerPath :: FilePath
  , trialStockInputs :: StockInputs
  , trialCandidateBytes :: B.ByteString
  , trialCandidateFunctions :: [Function]
  , trialRoot :: FilePath
  , trialInconclusiveRoot :: FilePath
  , trialRetained :: IORef [String]
  , trialCounter :: IORef Int
  }

-- | The reducer's oracle: each trial writes a fresh pair and runs it once. A proven
-- difference is transient; the first inconclusive example of each class is kept.
mkTrial :: TrialRunner -> ValidCase -> IO Trial
mkTrial config valid = do
  counter <- atomicModifyIORef' (trialCounter config) (\n -> (n + 1, n))
  let directory = trialRoot config </> show counter
      c = oracleCase valid
      spec = caseSpec (stockImage (trialStockInputs config)) (stockLoader (trialStockInputs config)) (stockFunctions (trialStockInputs config)) c
      candidate = candidateSpec (trialCandidateBytes config) (trialCandidateFunctions config) spec
  outcome <- runCasePair (trialRunnerPath config) directory spec candidate
  let verdict = case outcome of
        PairFailed message -> Inconclusive (InfrastructureFailure message)
        PairObserved stock candidateObservation -> trialFromVerdict (evaluateCase c stock candidateObservation)
      caseLabel = caseName (name c)
  case verdict of
    Inconclusive block -> retainInconclusive config directory caseLabel block
    _ -> removeDirectoryIfPresent directory
  pure verdict

retainInconclusive :: TrialRunner -> FilePath -> String -> TrialBlock -> IO ()
retainInconclusive config directory caseLabel block = do
  retained <- readIORef (trialRetained config)
  let label = trialBlockClass block
      target = trialInconclusiveRoot config </> label
  if label `elem` retained
    then removeDirectoryIfPresent directory
    else do
      createDirectoryIfMissing True (trialInconclusiveRoot config)
      targetExists <- doesDirectoryExist target
      if targetExists
        then removeDirectoryIfPresent directory
        else do
          sourceExists <- doesDirectoryExist directory
          if sourceExists
            then renameDirectory directory target
            else createDirectoryIfMissing True target
          writeFile (target </> "case-name.txt") caseLabel
          writeFile (target </> "diagnostic.txt") (show block)
          modifyIORef' (trialRetained config) (label :)

trialBlockClass :: TrialBlock -> String
trialBlockClass block = kebab (takeWhile (/= ' ') (show block))
  where
    kebab [] = []
    kebab (first : rest) = toLower first : go rest
    go [] = []
    go (character : rest)
      | isUpper character = '-' : toLower character : go rest
      | otherwise = character : go rest

removeDirectoryIfPresent :: FilePath -> IO ()
removeDirectoryIfPresent path = do
  present <- doesDirectoryExist path
  if present then removeDirectoryRecursive path else pure ()

candidateSpec :: B.ByteString -> [Function] -> CaseSpec -> CaseSpec
candidateSpec code functions spec =
  spec
    { caseRegions = map patch (caseRegions spec)
    , caseFunctions =
        [ (address, size)
        | (address, size) <- caseFunctions spec
        , guestAddress address < 0x6ac2b4 || guestAddress address >= 0x6ae114
        ]
          ++ [(GuestAddress (fromIntegral address), size) | Function address size <- functions]
    }
  where
    patch region
      | regionName (regionIdOf (caseRegion region)) == "record-code" =
          let old = caseRegion region
              bytes = replace (0x6ac2b4 - 0x6a6b74) code (regionBytes old)
           in mkRegion (RegionId "record-code") (GuestAddress 0x6a6b74) (B.length bytes) ReadExecute bytes
      | otherwise = region

symbolFunctions :: FilePath -> IO [Function]
symbolFunctions path = do
  rows <- lines <$> readFile path
  pure
    ( nub
        [ Function (parseAddress "symbol address" ("0x" ++ address)) (parseAddress "symbol size" ("0x" ++ size))
        | row <- rows
        , [address, size, kind', _] <- [words row]
        , kind' `elem` ["T", "t"]
        , size /= "00000000"
        , size /= "0"
        ]
    )

cachedCaseInputs :: String -> Maybe FilePath -> IO (M.Map CaseId FilePath)
cachedCaseInputs _ Nothing = pure M.empty
cachedCaseInputs role (Just root) = do
  directories <- filter (isPrefixOf (role ++ "-")) <$> listDirectory root
  entries <- concat <$> mapM (fmap snd . readBatch . (\directory -> root </> directory </> "batch.json")) directories
  let inputs = M.fromList [(entryId entry, entryPath entry) | entry <- entries]
  unless (M.size inputs == length entries) (error ("duplicate cached " ++ role ++ " case"))
  pure inputs

cachedObserved :: String -> Maybe Sha256 -> M.Map CaseId FilePath -> FilePath -> [CaseSpec] -> IO [(CaseId, NativeObservation)]
cachedObserved role runnerDigest inputs directory specs = mapM observe specs
  where
    observe spec = do
      let stem = caseId spec
          originalCase = M.findWithDefault (error ("cached " ++ role ++ " case missing: " ++ stem)) (CaseId stem) inputs
          generatedCase = directory </> stem ++ ".case.json"
      original <- B.readFile originalCase >>= either error pure . executionInput
      generated <- B.readFile generatedCase >>= either error pure . executionInput
      unless (original == generated) (error ("cached " ++ role ++ " input differs: " ++ stem))
      (_, regions, _, _) <- loadCase generatedCase
      report <- B.readFile (takeDirectory originalCase </> stem ++ ".native-result.json") >>= either error pure . eitherDecodeStrict'
      digest <- maybe (error ("cached " ++ role ++ " requires --runner")) pure runnerDigest
      observation <- either (error . ((stem ++ ": ") ++) . show) pure (observeCachedStock digest regions report)
      pure (CaseId stem, observation)

executionInput :: B.ByteString -> Either String Value
executionInput bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object fields -> Right (Object (K.delete "expected" fields))
    _ -> Left "case must be an object"
