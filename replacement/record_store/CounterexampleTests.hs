{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeStrict')
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.NativeObservation
import FujiAF.Host (OutputCapBytes (..), ProcessTimeoutMicros (..), runBoundedProcess, withTemporary)
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Generate (validCase)
import Replacement.RecordStore.Harness (Function (..))
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Minimize
import Replacement.RecordStore.Model hiding (Medium)
import System.Directory
  ( createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , doesPathExist
  , listDirectory
  , makeAbsolute
  )
import System.Environment (getArgs, getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Posix.Files (createSymbolicLink, setFileMode)
import System.Process (CreateProcess (..), proc)

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

baseCase :: OracleCase
baseCase =
  OracleCase
    (CaseName "counterexample-test")
    Kind0
    (mediumOf (geometry Kind0) [(10, False)] [])
    [Open, Register Normal, Latest]
    MarkerCallback
    NoFault

modelObservation :: OracleCase -> NativeObservation
modelObservation c =
  NativeObservation
    { nativeStatus = if stopped (persistence state) then MemoryGuard else Returned
    , nativeRegisters =
        M.fromList
          ( [(register, 0) | register <- registerLabels]
              ++ [(Sp, stackTop)]
              ++ zip [R4, R5, R6, R7, R8, R9, R10, R11] [0x44440004 .. 0x4444000b]
          )
    , nativeMemory =
        [ (RegionId "results", expectedResults expected)
        , (RegionId "caller", expectedCaller expected)
        , (RegionId "medium", expectedMedium c expected)
        , (RegionId "source", sourceBytes (geometry (kind c)))
        , (RegionId "requests", requestBytes <> B.replicate (requestSize - B.length requestBytes) 0)
        , (RegionId "callbacks", callbackBytes <> B.replicate (callbackSize - B.length callbackBytes) 0)
        ]
    , nativeProvenance = Complete (ProvenanceTotal 0) []
    , nativeInstructions = InstructionCount 1
    , nativeEngineError = EngineError 0
    , nativeKernel = Nothing
    }
  where
    expected = model hardware c
    state = expectedStore expected
    requestBytes = B.concat (map encodeRequest (requests (persistence state)))
    callbackBytes = B.concat (map encodeCallback (callbackEvents state))

withRegion :: RegionId -> B.ByteString -> NativeObservation -> NativeObservation
withRegion ident bytes observation =
  observation
    { nativeMemory =
        [(name, if name == ident then bytes else current) | (name, current) <- nativeMemory observation]
    }

mutateRegion :: RegionId -> NativeObservation -> NativeObservation
mutateRegion ident observation =
  let bytes = fromMaybe (error "missing observation region") (lookup ident (nativeMemory observation))
   in withRegion ident (B.cons (B.index bytes 0 `xor` 0xff) (B.tail bytes)) observation

withRegister :: Register -> Word32 -> NativeObservation -> NativeObservation
withRegister register value observation =
  observation {nativeRegisters = M.insert register value (nativeRegisters observation)}

withStatus :: RunnerStatus -> NativeObservation -> NativeObservation
withStatus status observation = observation {nativeStatus = status}

sampleDifference :: RegionDifference
sampleDifference = RegionDifference 0 (Just 1) (Just 2) 1 1

unitTests :: IO ()
unitTests = do
  let valid = either (error . show) id (validCase baseCase)
      observation = modelObservation baseCase
  assert "model-matching stock and candidate match" (evaluateCase baseCase observation observation == CaseMatches)
  assert "replay uses the same comparator for a matching case" (replayVerdict valid observation observation == CaseMatches)
  assert
    "stock model mismatch is a prerequisite, not candidate evidence"
    ( case evaluateCase baseCase (mutateRegion (RegionId "results") observation) observation of
        CaseStockModelMismatch problems -> not (null problems)
        _ -> False
    )
  assert
    "non-returning stock is unsupported before the model comparison"
    ( case evaluateCase baseCase (withStatus MemoryGuard observation) observation of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "stock instruction budget is unsupported, not model semantics"
    ( case evaluateCase baseCase (withStatus InstructionBudget observation) observation of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "candidate result difference reports the results boundary"
    ( case evaluateCase baseCase observation (mutateRegion (RegionId "results") observation) of
        CaseDiffers differences -> map roleBoundary differences == [Results]
        _ -> False
    )
  assert
    "candidate register difference reports the preserved register"
    ( case evaluateCase baseCase observation (withRegister R4 0xdeadbeef observation) of
        CaseDiffers differences -> map roleBoundary differences == [Preserved R4]
        _ -> False
    )
  let twoBoundaries = mutateRegion (RegionId "medium") (mutateRegion (RegionId "results") observation)
  assert
    "verdict exposes every differing boundary in comparison order"
    ( case evaluateCase baseCase observation twoBoundaries of
        CaseDiffers differences -> nub (map roleBoundary differences) == [Results, Medium]
        _ -> False
    )
  assert
    "replay exposes every differing boundary"
    ( case replayVerdict valid observation twoBoundaries of
        CaseDiffers differences -> nub (map roleBoundary differences) == [Results, Medium]
        _ -> False
    )
  assert
    "candidate memory guard against returning stock is an outcome failure"
    ( case evaluateCase baseCase observation (withStatus MemoryGuard observation) of
        CaseDiffers differences -> map roleBoundary differences == [Outcome]
        _ -> False
    )
  assert
    "candidate fetch guard against returning stock is an outcome failure"
    ( case evaluateCase baseCase observation (withStatus FetchGuard observation) of
        CaseDiffers differences -> map roleBoundary differences == [Outcome]
        _ -> False
    )
  assert
    "candidate instruction budget is unsupported, not a difference"
    ( case evaluateCase baseCase observation (withStatus InstructionBudget observation) of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "role comparison reports status before regions"
    ( case compareRoles observation (withStatus InstructionBudget observation) of
        RoleStatus Returned InstructionBudget : _ -> True
        _ -> False
    )
  assert
    "role comparison reports result and medium differences in order"
    ( case compareRoles observation twoBoundaries of
        [RoleRegion results _, RoleRegion medium _] -> results == RegionId "results" && medium == RegionId "medium"
        _ -> False
    )
  assert
    "differing verdict maps to typed boundaries"
    ( case trialFromVerdict (CaseDiffers [RoleRegion (RegionId "results") sampleDifference, RoleRegion (RegionId "medium") sampleDifference]) of
        Differs boundaries -> boundaries == [Results, Medium]
        _ -> False
    )
  assert
    "unsupported verdict maps to an inconclusive native status"
    ( case trialFromVerdict (CaseUnsupported "candidate status instruction_budget") of
        Inconclusive (UnsupportedNativeStatus _) -> True
        _ -> False
    )
  assert
    "stock model verdict maps to an inconclusive stock/model disagreement"
    ( case trialFromVerdict (CaseStockModelMismatch [ModelStatus Returned MemoryGuard 0]) of
        Inconclusive (StockModelDisagreement _) -> True
        _ -> False
    )
  assert
    "first difference reports the first differing byte"
    ( case firstDifference (B.pack [1, 2, 3]) (B.pack [1, 2, 4]) of
        Just difference ->
          differenceOffset difference == 2
            && differenceExpected difference == Just 3
            && differenceActual difference == Just 4
            && differenceExpectedSize difference == 3
            && differenceActualSize difference == 3
        Nothing -> False
    )
  assert
    "first difference reports a missing trailing byte"
    ( case firstDifference (B.pack [1]) (B.pack [1, 2]) of
        Just difference ->
          differenceOffset difference == 1
            && differenceExpected difference == Nothing
            && differenceActual difference == Just 2
        Nothing -> False
    )
  assert "equal bytes have no first difference" (firstDifference (B.pack [1, 2]) (B.pack [1, 2]) == Nothing)
  let rows =
        [ "range\tunit\tsection\t0x0061c400\t" ++ show (0x700000 :: Word32) ++ "\t32\t" ++ replicate 64 'a'
        , "range\tunit\tsection\t0x00000000\t" ++ show (0x1000 :: Word32) ++ "\t8\t" ++ replicate 64 'b'
        , "range\tunit\tsection\t0x0089aa90\t" ++ show (0x89aa90 :: Word32) ++ "\t16\t" ++ replicate 64 'c'
        ]
  assert
    "row input reads the decimal start and size after the hex column"
    ( readFunctionsBytes (BC.pack (unlines rows))
        == [Function 0x89aa90 0x128, Function 0x89abb8 0x20, Function 0x700000 32]
    )
  assert
    "trial blocks map to distinct typed evidence classes"
    ( map trialBlockClass [StockModelDisagreement "", UnsupportedNativeStatus "", DomainRefusal "", InfrastructureFailure ""]
        == [StockModelDisagreementClass, UnsupportedNativeStatusClass, DomainRefusalClass, InfrastructureFailureClass]
    )
  assert
    "inconclusive class labels round-trip"
    ( map inconclusiveClassFromLabel (map inconclusiveClassLabel [minBound .. maxBound])
        == map Just [minBound .. maxBound :: InconclusiveClass]
    )

main :: IO ()
main = do
  unitTests
  arguments <- getArgs
  binary <- case arguments of
    path : _ -> resolveArgument path
    [] -> error "counterexample-tests requires the counterexample-loop binary path"
  cliTests binary
  putStrLn "record-store counterexample tests passed"

data CliRun = CliRun
  { cliExit :: Int
  , cliValue :: Value
  }

data Fixtures = Fixtures
  { fixtureCandidate :: FilePath
  , fixtureSymbols :: FilePath
  , fixtureRunner :: FilePath
  , fixtureImage :: FilePath
  , fixtureLoader :: FilePath
  , fixtureIndex :: FilePath
  }

candidateRelative :: FilePath
candidateRelative = "replacement/record_store_carrier/record-store-image.bin"

cliTests :: FilePath -> IO ()
cliTests binary = withTemporary "record-counterexample-cli" $ \workspace -> do
  createDirectoryIfMissing True (workspace </> "private-data" </> "scratch")
  createDirectoryIfMissing True (workspace </> "private-data" </> "verify-candidate" </> "runs")
  fixtures <- makeFixtures workspace
  workspaceBinary <- copyBinary workspace binary
  runfiles <- findRunfiles
  let private = workspace </> "private-data"

  beforeUnknown <- snapshot private
  unknown <- runCli binary workspace Nothing Nothing ["--out", "private-data/scratch/unknown-flag", "--not-a-flag"]
  assertRun "unknown flag" unknown 2 "invalid"
  afterUnknown <- snapshot private
  assert "unknown flag writes nothing" (beforeUnknown == afterUnknown)

  mapM_ (assertNumericRefusal binary workspace fixtures private) numericRefusals

  let acceptedOut = "private-data/scratch/number-accepted"
  accepted <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures acceptedOut Nothing ++ ["--seed", "0", "--cases", "1", "--operations", "3", "--budget", "1"])
  assertRun "numeric in-range" accepted 2 "prerequisite"
  acceptedCreated <- doesPathExist (workspace </> "private-data" </> "scratch" </> "number-accepted")
  assert "numeric in-range reaches the stock prerequisite without an output" (not acceptedCreated)

  let outsideOutput = workspace </> "outside-output"
  beforeOutput <- snapshot private
  refusedOutput <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures outsideOutput Nothing)
  assertRun "output refusal" refusedOutput 2 "invalid"
  outputExists <- doesPathExist outsideOutput
  assert "output refusal creates no directory" (not outputExists)
  afterOutput <- snapshot private
  assert "output refusal writes nothing" (beforeOutput == afterOutput)

  let outsideCache = workspace </> "outside-cache"
  beforeCache <- snapshot private
  refusedCache <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures "private-data/scratch/cache-refusal" (Just outsideCache))
  assertRun "cache refusal" refusedCache 2 "invalid"
  cacheOutputExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "cache-refusal")
  outsideCacheExists <- doesPathExist outsideCache
  assert "cache refusal creates no output or cache directory" (not cacheOutputExists && not outsideCacheExists)
  afterCache <- snapshot private
  assert "cache refusal writes nothing" (beforeCache == afterCache)

  let sharedCacheOut = "private-data/scratch/shared-cache"
  beforeShared <- snapshot private
  sharedCache <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures sharedCacheOut (Just "private-data/cache/native"))
  assertRun "shared cache admission" sharedCache 2 "prerequisite"
  afterShared <- snapshot private
  assert "shared cache admission writes nothing before the stock prerequisite" (beforeShared == afterShared)

  beforeTraversal <- snapshot private
  traversal <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures "private-data/scratch/traversal" (Just "private-data/scratch/../escape-cache"))
  assertRun "cache traversal" traversal 2 "invalid"
  traversalOutputExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "traversal")
  escapedCacheExists <- doesPathExist (workspace </> "private-data" </> "escape-cache")
  assert "cache traversal creates no output or escaped cache" (not traversalOutputExists && not escapedCacheExists)
  afterTraversal <- snapshot private
  assert "cache traversal writes nothing" (beforeTraversal == afterTraversal)

  let link = workspace </> "private-data" </> "scratch" </> "escape-link"
      linkTarget = workspace </> "outside-cache-link"
  createSymbolicLink linkTarget link
  beforeLink <- snapshot private
  linked <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures "private-data/scratch/link-refusal" (Just "private-data/scratch/escape-link/cache"))
  assertRun "cache symlink escape" linked 2 "invalid"
  linkedOutputExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "link-refusal")
  linkedTargetExists <- doesPathExist (linkTarget </> "cache")
  assert "cache symlink escape creates no output or target cache" (not linkedOutputExists && not linkedTargetExists)
  afterLink <- snapshot private
  assert "cache symlink escape writes nothing" (beforeLink == afterLink)

  let fakeStockOut = "private-data/scratch/fake-stock"
  beforeStock <- snapshot private
  fakeStock <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures fakeStockOut Nothing)
  assertRun "fake stock" fakeStock 2 "prerequisite"
  fakeStockExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "fake-stock")
  assert "fake stock rejection creates no output" (not fakeStockExists)
  afterStock <- snapshot private
  assert "fake stock rejection writes nothing" (beforeStock == afterStock)

  let workspaceDefaultOut = "private-data/scratch/workspace-default"
  defaultRun <- runCli workspaceBinary workspace Nothing Nothing (stockArguments fixtures workspaceDefaultOut)
  assertRun "workspace default without runfiles" defaultRun 2 "invalid"
  defaultOutExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "workspace-default")
  assert "workspace default creates no output" (not defaultOutExists)

  let workspaceOverrideOut = "private-data/scratch/workspace-override"
  overrideRun <- runCli workspaceBinary workspace Nothing Nothing (fixtureArguments fixtures workspaceOverrideOut Nothing)
  assertRun "workspace explicit override" overrideRun 2 "prerequisite"
  overrideOutExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "workspace-override")
  assert "workspace override creates no output before stock rejection" (not overrideOutExists)

  let runfilesDefaultOut = "private-data/scratch/runfiles-default"
  runfilesDefault <- runCli binary runfiles (Just workspace) (Just runfiles) (stockArguments fixtures runfilesDefaultOut)
  assertRun "runfiles anchored default" runfilesDefault 2 "prerequisite"
  runfilesDefaultExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "runfiles-default")
  assert "runfiles default creates no output before stock rejection" (not runfilesDefaultExists)

  let runfilesOverrideOut = "private-data/scratch/runfiles-override"
  runfilesOverride <- runCli binary runfiles (Just workspace) (Just runfiles) (fixtureArguments fixtures runfilesOverrideOut Nothing)
  assertRun "runfiles explicit override" runfilesOverride 2 "prerequisite"
  runfilesOverrideExists <- doesPathExist (workspace </> "private-data" </> "scratch" </> "runfiles-override")
  assert "runfiles override creates no output before stock rejection" (not runfilesOverrideExists)

stockArguments :: Fixtures -> FilePath -> [String]
stockArguments fixtures out =
  [ "--runner", fixtureRunner fixtures
  , "--image", fixtureImage fixtures
  , "--loader", fixtureLoader fixtures
  , "--index", fixtureIndex fixtures
  , "--out", out
  ]

fixtureArguments :: Fixtures -> FilePath -> Maybe FilePath -> [String]
fixtureArguments fixtures out cache =
  [ "--candidate", fixtureCandidate fixtures
  , "--symbols", fixtureSymbols fixtures
  ]
    ++ stockArguments fixtures out
    ++ maybe [] (\path -> ["--cache", path]) cache

-- | Out-of-range, fixed-width-wrapping and malformed numeric arguments.
numericRefusals :: [(String, String, String)]
numericRefusals =
  [ ("seed-negative", "--seed", "-1")
  , ("seed-overflow", "--seed", "18446744073709551616")
  , ("cases-negative", "--cases", "-1")
  , ("cases-zero", "--cases", "0")
  , ("cases-overflow", "--cases", "18446744073709551617")
  , ("cases-huge", "--cases", "99999999999999999999999999999999999999")
  , ("cases-malformed", "--cases", "eight")
  , ("operations-low", "--operations", "2")
  , ("operations-overflow", "--operations", "18446744073709551619")
  , ("budget-negative", "--budget", "-1")
  , ("budget-high", "--budget", "257")
  ]

assertNumericRefusal :: FilePath -> FilePath -> Fixtures -> FilePath -> (String, String, String) -> IO ()
assertNumericRefusal binary workspace fixtures private (label, flag, text) = do
  let out = "private-data/scratch/number-" ++ label
  before <- snapshot private
  refused <- runCli binary workspace Nothing Nothing (fixtureArguments fixtures out Nothing ++ [flag, text])
  assertRun ("numeric " ++ label) refused 2 "invalid"
  created <- doesPathExist (workspace </> "private-data" </> "scratch" </> ("number-" ++ label))
  assert ("numeric " ++ label ++ " creates no output") (not created)
  after <- snapshot private
  assert ("numeric " ++ label ++ " writes nothing") (before == after)

makeFixtures :: FilePath -> IO Fixtures
makeFixtures workspace = do
  let directory = workspace </> "fixtures"
  createDirectoryIfMissing True directory
  let write name bytes = do
        let path = directory </> name
        B.writeFile path bytes
        pure path
  candidate <- write "candidate.bin" (B.replicate 0x1e60 0)
  symbols <- write "symbols.txt" B.empty
  runner <- write "runner" (BC.pack "runner")
  image <- write "image.bin" (BC.pack "not-the-stock-image")
  loader <- write "loader.bin" (BC.pack "loader")
  index <- write "index.tsv" B.empty
  pure
    Fixtures
      { fixtureCandidate = candidate
      , fixtureSymbols = symbols
      , fixtureRunner = runner
      , fixtureImage = image
      , fixtureLoader = loader
      , fixtureIndex = index
      }

resolveArgument :: FilePath -> IO FilePath
resolveArgument path = do
  runfiles <- lookupEnv "RUNFILES_DIR"
  testSource <- lookupEnv "TEST_SRCDIR"
  let roots =
        [""]
          ++ maybe [] (: []) runfiles
          ++ maybe [] (: []) testSource
          ++ maybe [] (\root -> [root </> "_main", root </> "fuji-af"]) runfiles
      candidates = nub [root </> path | root <- roots]
  existing <- firstExisting candidates
  maybe (error ("counterexample-loop binary not found: " ++ path)) makeAbsolute existing
  where
    firstExisting [] = pure Nothing
    firstExisting (candidate : rest) = do
      present <- doesFileExist candidate
      if present then pure (Just candidate) else firstExisting rest

copyBinary :: FilePath -> FilePath -> IO FilePath
copyBinary workspace binary = do
  let directory = workspace </> "bin"
      destination = directory </> "counterexample-loop"
  createDirectoryIfMissing True directory
  bytes <- B.readFile binary
  B.writeFile destination bytes
  setFileMode destination 0o700
  pure destination

findRunfiles :: IO FilePath
findRunfiles = do
  environment <- lookupEnv "RUNFILES_DIR"
  testSource <- lookupEnv "TEST_SRCDIR"
  executable <- getExecutablePath
  let candidates = maybe [] (: []) environment ++ maybe [] (: []) testSource ++ [executable ++ ".runfiles"]
  search candidates
  where
    search [] = error "runfiles tree with the bundled record-store carrier not found"
    search (root : rest) = do
      nested <- anyM doesFileExist [root </> workspaceName </> candidateRelative | workspaceName <- ["_main", "fuji-af"]]
      direct <- doesFileExist (root </> candidateRelative)
      if nested || direct then pure root else search rest

    anyM _ [] = pure False
    anyM predicate (value : values) = do
      matched <- predicate value
      if matched then pure True else anyM predicate values

runCli :: FilePath -> FilePath -> Maybe FilePath -> Maybe FilePath -> [String] -> IO CliRun
runCli binary cwd buildWorkspace runfiles arguments = do
  environment <- getEnvironment
  let overrides =
        [ ("XM1_EMULATOR_SLOT", "1")
        , ("XM1_ADMISSION_CLASS", "NativeVerification")
        , ("XM1_STORAGE_MIN_FREE", "1K")
        ]
          ++ maybe [] (\root -> [("BUILD_WORKSPACE_DIRECTORY", root)]) buildWorkspace
          ++ maybe [] (\root -> [("RUNFILES_DIR", root)]) runfiles
      filtered =
        [ (key, value)
        | (key, value) <- environment
        , key `notElem` ["BUILD_WORKSPACE_DIRECTORY", "RUNFILES_DIR", "XM1_EMULATOR_SLOT", "XM1_ADMISSION_CLASS", "XM1_ADMISSION_OWNER", "XM1_STORAGE_MIN_FREE"]
        ]
      -- Admission accepts only the admitted process's own PID, which exec preserves.
      admittedExec = "export XM1_ADMISSION_OWNER=$$; exec \"$0\" \"$@\""
  (timed, code, out, err) <-
    runBoundedProcess
      (ProcessTimeoutMicros 60000000)
      (OutputCapBytes (4 * 1024 * 1024))
      (proc "/bin/sh" (["-c", admittedExec, binary] ++ arguments)) {cwd = Just cwd, env = Just (filtered ++ overrides)}
      B.empty
  unless (not timed) (error ("counterexample-loop timed out: " ++ BC.unpack (B.take 400 err)))
  value <-
    either
      (error . (++ BC.unpack (B.take 400 err)) . ("counterexample-loop stdout is not JSON: " ++))
      pure
      (eitherDecodeStrict' out)
  pure (CliRun (exitNumber code) value)

exitNumber :: ExitCode -> Int
exitNumber ExitSuccess = 0
exitNumber (ExitFailure code) = code

assertRun :: String -> CliRun -> Int -> String -> IO ()
assertRun label run expectedExit expectedVerdict = do
  assert (label ++ " exit code") (cliExit run == expectedExit)
  assert (label ++ " verdict") (textField "verdict" (cliValue run) == Just expectedVerdict)

textField :: String -> Value -> Maybe String
textField key (Object fields) =
  case KM.lookup (K.fromString key) fields of
    Just (String text) -> Just (T.unpack text)
    _ -> Nothing
textField _ _ = Nothing

snapshot :: FilePath -> IO [FilePath]
snapshot root = do
  present <- doesDirectoryExist root
  if not present then pure [] else go root ""
  where
    go directory relative = do
      entries <- sort <$> listDirectory directory
      fmap concat $ forM entries $ \entry -> do
        let path = directory </> entry
            nested = if null relative then entry else relative </> entry
        directoryEntry <- doesDirectoryExist path
        if directoryEntry then (nested :) <$> go path nested else pure [nested]
