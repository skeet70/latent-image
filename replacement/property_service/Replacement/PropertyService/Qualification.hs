{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

-- | The bounded qualification driver of the owned property service: stage the current candidate from
-- the canonical sources through the existing stager under a content-keyed, integrity-checked
-- manifest, run the existing oracle on one immutable attempt directory with the native batch cache,
-- and publish the verdict and review packet there before atomically replacing the job's current
-- result pointer. The command exits zero only for a complete current attempt whose effective
-- inputs, tools and result identities still match at publication.
module Replacement.PropertyService.Qualification
  ( Options (..)
  , defaultOptions
  , parseOptions
  , runQualification
  , OracleSummary (..)
  , DifferenceSample (..)
  , ControlSummary (..)
  , parseOracleSummary
  , resolveWithin
  , resolveCacheOutput
  ) where

import Control.Exception (IOException, SomeException, bracketOnError, displayException, finally, mask, try)
import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Foldable (toList)
import Data.List (intercalate, isPrefixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import Data.Scientific (floatingOrInteger)
import Data.String (fromString)
import qualified Data.Text as T
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256, sha256Hex, sha256File)
import FujiAF.Host (failH, readRegular, runBounded)
import FujiAF.JSON (encodeCanonical)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import System.Directory
  ( canonicalizePath
  , createDirectory
  , createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , doesPathExist
  , getCurrentDirectory
  , getFileSize
  , makeAbsolute
  , renameDirectory
  , renameFile
  , setCurrentDirectory
  )
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (SeekMode (AbsoluteSeek), hClose, openTempFile)
import System.Posix.IO (FdOption (CloseOnExec), LockRequest (WriteLock), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setFdOption, setLock)

canonicalImage4 :: String
canonicalImage4 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

stageTimeoutMicros, oracleTimeoutMicros, stageOutputCap, oracleOutputCap, detailLimit :: Int
stageTimeoutMicros = 1800 * 1000 * 1000
oracleTimeoutMicros = 6 * 3600 * 1000 * 1000
stageOutputCap = 4 * 1024 * 1024
oracleOutputCap = 8 * 1024 * 1024
detailLimit = 2000

-- Options ---------------------------------------------------------------------

newtype PropertySelection = PropertySelection [PropertyId] deriving (Eq, Show)

data Options = Options
  { optDat :: FilePath
  , optImage4 :: FilePath
  , optLoaderA :: FilePath
  , optLoaderB :: FilePath
  , optImage5 :: FilePath
  , optPlain :: FilePath
  , optImage2 :: FilePath
  , optLinked :: FilePath
  , optSymbols :: FilePath
  , optOut :: FilePath
  , optCache :: FilePath
  , optIds :: Maybe PropertySelection
  , optRunner :: FilePath
  , optSlot :: FilePath
  , optOracle :: FilePath
  , optStage :: FilePath
  , optStageEncoder :: FilePath
  , optStageRebuild :: FilePath
  , optStageDecoder :: FilePath
  , optDefects :: [(String, FilePath, FilePath)]
  }
  deriving (Eq, Show)

defaultOptions :: Options
defaultOptions =
  Options
    { optDat = ""
    , optImage4 = ""
    , optLoaderA = ""
    , optLoaderB = ""
    , optImage5 = ""
    , optPlain = ""
    , optImage2 = ""
    , optLinked = ""
    , optSymbols = ""
    , optOut = ""
    , optCache = defaultNativeCacheRoot
    , optIds = Nothing
    , optRunner = fujimodNativeCase
    , optSlot = ""
    , optOracle = ""
    , optStage = ""
    , optStageEncoder = ""
    , optStageRebuild = ""
    , optStageDecoder = ""
    , optDefects = []
    }

-- | Each value flag replaces the earlier value. `--defect NAME BIN SYMBOLS` may repeat; a repeated
-- name replaces the earlier binary and symbol list.
parseOptions :: [String] -> Either String Options
parseOptions = go defaultOptions
  where
    go options ("--defect" : name : bin : symbols : rest) =
      go options {optDefects = optDefects options ++ [(name, bin, symbols)]} rest
    go _ ("--defect" : _) = Left "--defect needs NAME BIN SYMBOLS"
    go options ("--ids" : value : rest) = do
      selection <- parseSelection value
      go options {optIds = Just selection} rest
    go options (flag : rest) = case lookup flag valueFlags of
      Just set -> case rest of
        value : more -> go (set options value) more
        [] -> Left (flag ++ " needs a value")
      Nothing -> Left ("unknown argument: " ++ flag)
    go options [] = check options

    check options =
      let required =
            [ ("--dat", optDat options), ("--image4", optImage4 options), ("--loader-a", optLoaderA options)
            , ("--loader-b", optLoaderB options), ("--image5", optImage5 options), ("--plain", optPlain options)
            , ("--image2", optImage2 options), ("--linked", optLinked options), ("--symbols", optSymbols options)
            , ("--out", optOut options), ("--runner", optRunner options)
            , ("--emulator-slot", optSlot options)
            , ("--oracle", optOracle options), ("--stage", optStage options)
            , ("--stage-encoder", optStageEncoder options), ("--stage-rebuild", optStageRebuild options)
            , ("--stage-decoder", optStageDecoder options)
            ]
          missing = [flag | (flag, value) <- required, null value]
       in if null missing
            then Right options {optDefects = dedupeDefects (optDefects options)}
            else Left ("missing required flags: " ++ unwords missing)

    valueFlags =
      [ ("--dat", \options value -> options {optDat = value})
      , ("--image4", \options value -> options {optImage4 = value})
      , ("--loader-a", \options value -> options {optLoaderA = value})
      , ("--loader-b", \options value -> options {optLoaderB = value})
      , ("--image5", \options value -> options {optImage5 = value})
      , ("--plain", \options value -> options {optPlain = value})
      , ("--image2", \options value -> options {optImage2 = value})
      , ("--linked", \options value -> options {optLinked = value})
      , ("--symbols", \options value -> options {optSymbols = value})
      , ("--out", \options value -> options {optOut = value})
      , ("--cache", \options value -> options {optCache = value})
      , ("--runner", \options value -> options {optRunner = value})
      , ("--emulator-slot", \options value -> options {optSlot = value})
      , ("--oracle", \options value -> options {optOracle = value})
      , ("--stage", \options value -> options {optStage = value})
      , ("--stage-encoder", \options value -> options {optStageEncoder = value})
      , ("--stage-rebuild", \options value -> options {optStageRebuild = value})
      , ("--stage-decoder", \options value -> options {optStageDecoder = value})
      ]

dedupeDefects :: [(String, FilePath, FilePath)] -> [(String, FilePath, FilePath)]
dedupeDefects defects =
  [(name, bin, symbols) | (name, (bin, symbols)) <- M.toList (M.fromList [(name, (bin, symbols)) | (name, bin, symbols) <- defects])]

-- Output boundary --------------------------------------------------------------

-- | Canonicalizes both paths and requires the candidate's directory components to extend the
-- root's, so a symlink or @..@ segment that leaves the root is refused after resolution.
resolveWithin :: FilePath -> FilePath -> IO (Either String FilePath)
resolveWithin root candidate = do
  rootCanonical <- canonicalizePath root
  attempted <- try (canonicalizePath candidate)
  pure $ case attempted of
    Left (exception :: SomeException) -> Left (candidate ++ ": " ++ displayException exception)
    Right canonical ->
      let rootComponents = splitDirectories rootCanonical
          pathComponents = splitDirectories canonical
       in if rootComponents `isPrefixOf` pathComponents && length pathComponents > length rootComponents
            then Right canonical
            else Left (candidate ++ " resolves to " ++ canonical ++ ", outside " ++ rootCanonical)

privateScratchRoot :: IO FilePath
privateScratchRoot = do
  createDirectoryIfMissing True "private-data/scratch"
  canonicalizePath "private-data/scratch"

privateCacheRoot :: IO FilePath
privateCacheRoot = do
  createDirectoryIfMissing True "private-data/cache"
  canonicalizePath "private-data/cache"

canonicalExisting :: FilePath -> IO FilePath
canonicalExisting candidate = do
  exists <- doesDirectoryExist candidate
  if exists then canonicalizePath candidate else canonicalExisting (takeDirectory candidate)

resolveOutput :: String -> FilePath -> FilePath -> IO (Either String FilePath)
resolveOutput flag scratch path = do
  absolute <- makeAbsolute path
  parent <- canonicalExisting (takeDirectory absolute)
  let parentInside = splitDirectories scratch `isPrefixOf` splitDirectories parent
  if not parentInside
    then pure (Left (flag ++ " must resolve under private-data/scratch; use private-data/scratch/QualificationAutomation/jobs or .../cache"))
    else do
      createDirectoryIfMissing True absolute
      resolved <- resolveWithin scratch absolute
      pure $ case resolved of
        Left problem ->
          Left (flag ++ ": " ++ problem ++ "; use private-data/scratch/QualificationAutomation/jobs or .../cache")
        Right canonical -> Right canonical

-- | A cache root below private-data/scratch or private-data/cache.
resolveCacheOutput :: String -> FilePath -> FilePath -> FilePath -> IO (Either String FilePath)
resolveCacheOutput flag scratch cache path = do
  absolute <- makeAbsolute path
  parent <- canonicalExisting (takeDirectory absolute)
  let roots = [scratch, cache]
      parentInside = any (\root -> splitDirectories root `isPrefixOf` splitDirectories parent) roots
  if not parentInside
    then pure (Left (flag ++ " must resolve under private-data/scratch or private-data/cache; use private-data/cache/native"))
    else do
      createDirectoryIfMissing True absolute
      resolved <- firstRoot absolute roots
      pure $ case resolved of
        Nothing -> Left (flag ++ ": " ++ absolute ++ " resolves outside private-data/scratch and private-data/cache")
        Just canonical -> Right canonical
  where
    firstRoot _ [] = pure Nothing
    firstRoot target (root : rest) = do
      outcome <- resolveWithin root target
      case outcome of
        Right canonical -> pure (Just canonical)
        Left _ -> firstRoot target rest

-- Typed oracle results ---------------------------------------------------------

data DifferenceSample = DifferenceSample
  { sampleCase :: String
  , sampleKind :: String
  , sampleDetail :: String
  }
  deriving (Eq, Show)

data ControlSummary = ControlSummary
  { controlName :: String
  , controlKind :: String
  , controlNote :: String
  , controlCases :: Int
  , controlFailedCases :: Int
  , controlHolds :: Bool
  , controlFirstDifferences :: [DifferenceSample]
  }
  deriving (Eq, Show)

-- | The required fields of @results.json@ the verdict consumes. Missing, mistyped or absent fields
-- are refused; no field defaults.
data OracleSummary = OracleSummary
  { summaryPassed :: Bool
  , summarySubset :: Bool
  , summaryIds :: [String]
  , summaryCasesPerRole :: Int
  , summaryGridCases :: Int
  , summaryCacheRoot :: FilePath
  , summaryExecutedBatches :: Int
  , summaryReusedBatches :: Int
  , summaryExecutedCases :: Int
  , summaryReusedCases :: Int
  , summaryControls :: [ControlSummary]
  , summaryDifferences :: [(String, [DifferenceSample])]
  , summaryInputs :: M.Map String (FilePath, String)
  , summaryRunnerPath :: FilePath
  , summaryRunnerSha256 :: String
  , summaryContractImage4Sha256 :: String
  }
  deriving (Eq, Show)

parseOracleSummary :: Value -> Either String OracleSummary
parseOracleSummary root = do
  schema <- intField "schema_version" root
  unless (schema == 1) (Left "schema_version: 1 required")
  passed <- boolField "passed" root
  shape <- field "case_shape" root
  subset <- boolField "subset" shape
  idsText <- stringField "ids" shape
  casesPerRole <- intField "cases_per_role" shape
  gridCases <- intField "grid_cases" shape
  execution <- field "execution" root
  cache <- stringField "cache" execution
  executedBatches <- intField "batches_executed" execution
  reusedBatches <- intField "batches_reused" execution
  executedCases <- intField "cases_executed" execution
  reusedCases <- intField "cases_reused" execution
  controlsValue <- arrayField "controls" root
  controls <- mapM parseControl controlsValue
  differencesValue <- objectField "differences" root
  differences <- sortOn fst <$> mapM parseDifferenceGroup (KM.toList differencesValue)
  inputsValue <- objectField "inputs" root
  inputs <- M.fromList <$> mapM parseInput (KM.toList inputsValue)
  runner <- field "runner" root
  runnerPath <- stringField "path" runner
  runnerSha <- stringField "sha256" runner
  contractSha <- stringField "contract_staged_image4_sha256" root
  unless (executedBatches + reusedBatches > 0) (Left "execution: no batches recorded")
  unless (executedCases + reusedCases > 0) (Left "execution: no cases recorded")
  pure
    OracleSummary
      { summaryPassed = passed
      , summarySubset = subset
      , summaryIds = words idsText
      , summaryCasesPerRole = casesPerRole
      , summaryGridCases = gridCases
      , summaryCacheRoot = cache
      , summaryExecutedBatches = executedBatches
      , summaryReusedBatches = reusedBatches
      , summaryExecutedCases = executedCases
      , summaryReusedCases = reusedCases
      , summaryControls = controls
      , summaryDifferences = differences
      , summaryInputs = inputs
      , summaryRunnerPath = runnerPath
      , summaryRunnerSha256 = runnerSha
      , summaryContractImage4Sha256 = contractSha
      }
  where
    parseControl value = do
      name <- stringField "name" value
      kind <- stringField "kind" value
      note <- stringField "note" value
      cases <- intField "cases" value
      failed <- intField "failed_cases" value
      holds <- boolField "holds" value
      samples <- arrayField "first_differences" value >>= mapM parseControlSample
      pure (ControlSummary name kind note cases failed holds samples)
    parseControlSample value = do
      caseId <- stringField "case" value
      kind <- stringField "kind" value
      detail <- stringField "detail" value
      pure (DifferenceSample caseId kind detail)
    parseDifferenceGroup (key, value) = do
      samples <- asArray (K.toString key) value >>= mapM parseDifferenceSample
      pure (K.toString key, samples)
    parseDifferenceSample value = do
      caseId <- stringField "case" value
      detail <- stringField "detail" value
      pure (DifferenceSample caseId "" detail)
    parseInput (key, value) = do
      path <- stringField "path" value
      digest <- stringField "sha256" value
      pure (K.toString key, (path, digest))

field :: String -> Value -> Either String Value
field key value = case value of
  Object objectValue -> maybe (Left ("missing field " ++ key)) Right (KM.lookup (K.fromString key) objectValue)
  _ -> Left ("object required for field " ++ key)

stringField :: String -> Value -> Either String String
stringField key value = field key value >>= asString key

asString :: String -> Value -> Either String String
asString _ (String text) = Right (T.unpack text)
asString key _ = Left (key ++ ": string required")

boolField :: String -> Value -> Either String Bool
boolField key value = field key value >>= asBool key

asBool :: String -> Value -> Either String Bool
asBool _ (Bool boolean) = Right boolean
asBool key _ = Left (key ++ ": boolean required")

intField :: String -> Value -> Either String Int
intField key value = field key value >>= asInt key

asInt :: String -> Value -> Either String Int
asInt key (Number number) = case floatingOrInteger number :: Either Double Integer of
  Right integer
    | integer >= 0 && integer <= toInteger (maxBound :: Int) -> Right (fromInteger integer)
  _ -> Left (key ++ ": nonnegative integer required")
asInt key _ = Left (key ++ ": number required")

arrayField :: String -> Value -> Either String [Value]
arrayField key value = field key value >>= asArray key

asArray :: String -> Value -> Either String [Value]
asArray _ (Array values) = Right (toList values)
asArray key _ = Left (key ++ ": array required")

objectField :: String -> Value -> Either String (KM.KeyMap Value)
objectField key value = field key value >>= asObject key

asObject :: String -> Value -> Either String (KM.KeyMap Value)
asObject _ (Object objectValue) = Right objectValue
asObject key _ = Left (key ++ ": object required")

-- Stage manifest ---------------------------------------------------------------

data StageFiles = StageFiles
  { stageCandidateDatPath :: FilePath
  , stageContractPath :: FilePath
  , stageImage4Path :: FilePath
  , stageCandidateDatSha256 :: String
  , stageContractSha256 :: String
  , stageImage4Sha256 :: String
  }
  deriving (Eq, Show)

stageOutputNames :: [(String, String)]
stageOutputNames = [("candidate_dat", "candidate.dat"), ("contract", "contract.json"), ("image4", "image4.bin")]

writeStageManifest :: FilePath -> Value -> [(String, FilePath)] -> IO ()
writeStageManifest directory identity files = do
  outputs <- forM files $ \(role, path) -> do
    digest <- fileDigest path
    size <- getFileSize path
    pure (object ["role" .= role, "file" .= takeFileName path, "sha256" .= digest, "size" .= size])
  let manifest =
        object
          [ "schema_version" .= (1 :: Int)
          , "stage_key" .= identityHash identity
          , "identity" .= identity
          , "outputs" .= outputs
          ]
  B.writeFile (directory </> "manifest.json") (encodeCanonical manifest)

verifyStage :: FilePath -> Value -> IO (Either String StageFiles)
verifyStage directory expected = do
  attempted <- try (readStageManifest directory >>= checkStage directory expected)
  pure $ case attempted of
    Left (exception :: SomeException) -> Left (directory ++ ": " ++ displayException exception)
    Right result -> result

readStageManifest :: FilePath -> IO Value
readStageManifest directory = do
  let path = directory </> "manifest.json"
  exists <- doesFileExist path
  unless exists (failH (path ++ ": no completed stage manifest"))
  bytes <- readRegular (1024 * 1024) path
  either (\problem -> failH (path ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes :: Either String Value)

checkStage :: FilePath -> Value -> Value -> IO (Either String StageFiles)
checkStage directory expected manifest
  | fieldInt "schema_version" manifest /= Just 1 = pure (Left "stage manifest schema differs")
  | fieldText "stage_key" manifest /= Just (identityHash expected) = pure (Left "stage key differs from the current identity")
  | fieldValue "identity" manifest /= Just expected = pure (Left "stage identity differs from the current identity")
  | otherwise = do
      checked <- forM stageOutputNames $ \(role, file) -> checkOutput directory manifest role file
      pure $ case checked of
        [Right (_, datPath, datSha), Right (_, contractPath, contractSha), Right (_, image4Path, image4Sha)] ->
          Right
            StageFiles
              { stageCandidateDatPath = datPath
              , stageContractPath = contractPath
              , stageImage4Path = image4Path
              , stageCandidateDatSha256 = datSha
              , stageContractSha256 = contractSha
              , stageImage4Sha256 = image4Sha
              }
        _ -> Left "stage manifest does not have the complete candidate, contract and image4 outputs"
  where
    fieldValue key value = case value of
      Object objectValue -> KM.lookup (K.fromString key) objectValue
      _ -> Nothing
    fieldText key value = case fieldValue key value of
      Just (String text) -> Just (T.unpack text)
      _ -> Nothing
    fieldInt key value = case fieldValue key value of
      Just (Number number) -> case floatingOrInteger number :: Either Double Integer of
        Right integer -> Just integer
        Left _ -> Nothing
      _ -> Nothing

checkOutput :: FilePath -> Value -> String -> String -> IO (Either String (String, FilePath, String))
checkOutput directory manifest role file = case matches of
  [record] -> do
    let path = directory </> file
    exists <- doesFileExist path
    if not exists
      then pure (Left (path ++ ": missing from the completed stage"))
      else do
        size <- getFileSize path
        digest <- fileDigest path
        pure $
          if size /= recordedInt "size" record
            then Left (path ++ ": size differs from the completed stage")
            else
              if digest /= recordedText "sha256" record
                then Left (path ++ ": sha256 differs from the completed stage")
                else Right (role, path, digest)
  _ -> pure (Left (directory ++ ": manifest has no single " ++ role ++ " output"))
  where
    matches =
      [ record
      | record <- outputs
      , recordedText "role" record == role
      , recordedText "file" record == file
      ]
    outputs = case manifest of
      Object objectValue -> case KM.lookup (K.fromString "outputs") objectValue of
        Just (Array values) -> toList values
        _ -> []
      _ -> []
    recordedText key record = case record of
      Object objectValue -> case KM.lookup (K.fromString key) objectValue of
        Just (String text) -> T.unpack text
        _ -> ""
      _ -> ""
    recordedInt key record = case record of
      Object objectValue -> case KM.lookup (K.fromString key) objectValue of
        Just (Number number) -> case floatingOrInteger number :: Either Double Integer of
          Right integer -> integer
          Left _ -> -1
        _ -> -1
      _ -> -1

commitStage :: FilePath -> FilePath -> IO (Either String Bool)
commitStage partial final = do
  exists <- doesPathExist final
  if exists
    then pure (Right False)
    else do
      attempted <- try (renameDirectory partial final)
      case attempted of
        Right () -> pure (Right True)
        Left (exception :: SomeException) -> do
          appeared <- doesPathExist final
          pure $ if appeared then Right False else Left (displayException exception)

freePath :: FilePath -> String -> IO FilePath
freePath base prefix = go (1 :: Int)
  where
    go index = do
      let candidate = base </> (prefix ++ show index)
      exists <- doesPathExist candidate
      if exists then go (index + 1) else pure candidate

reserveDirectory :: FilePath -> String -> IO (Either String FilePath)
reserveDirectory base prefix = go (1 :: Int)
  where
    go index = do
      let candidate = base </> (prefix ++ show index)
      created <- try (createDirectory candidate)
      case created of
        Right () -> pure (Right candidate)
        Left (exception :: SomeException) -> do
          exists <- doesDirectoryExist candidate
          if exists then go (index + 1) else pure (Left (displayException exception))

quarantine :: FilePath -> FilePath -> IO (Either String FilePath)
quarantine path rejectedBase = do
  attempted <- try $ do
    createDirectoryIfMissing True rejectedBase
    target <- freePath rejectedBase (takeFileName path ++ "-")
    renameDirectory path target
    pure target
  pure $ case attempted of
    Right target -> Right target
    Left (exception :: SomeException) -> Left (displayException exception)

identityHash :: Value -> String
identityHash = sha256Hex . sha256 . encodeCanonical

fileDigest :: FilePath -> IO String
fileDigest path = sha256Hex <$> sha256File path

-- Effective identity -----------------------------------------------------------

data StageDigests = StageDigests
  { digestsValue :: Value
  , digestsSources :: M.Map String String
  , digestsLinked :: String
  , digestsSymbols :: String
  }

data JobDigests = JobDigests
  { jobValue :: Value
  , jobRunnerSha256 :: String
  , jobDefects :: [(String, FilePath, FilePath, String, String)]
  }

sourceRoles :: Options -> [(String, FilePath)]
sourceRoles options =
  [ ("dat", optDat options), ("image4", optImage4 options), ("loader_a", optLoaderA options)
  , ("loader_b", optLoaderB options), ("image5", optImage5 options), ("plain", optPlain options)
  , ("image2", optImage2 options)
  ]

digestsOf :: [(String, FilePath)] -> IO [(String, String)]
digestsOf roles = forM roles $ \(role, path) -> do
  digest <- fileDigest path
  pure (role, digest)

stageIdentityFor :: Options -> IO StageDigests
stageIdentityFor options = do
  sources <- digestsOf (sourceRoles options)
  linked <- fileDigest (optLinked options)
  symbols <- fileDigest (optSymbols options)
  tools <-
    digestsOf
      [ ("stage", optStage options)
      , ("encoder", optStageEncoder options)
      , ("rebuild", optStageRebuild options)
      , ("decoder", optStageDecoder options)
      ]
  let value =
        object
          [ "schema_version" .= (1 :: Int)
          , "sources" .= object [fromString role .= digest | (role, digest) <- sources]
          , "candidate" .= object ["linked" .= linked, "symbols" .= symbols]
          , "tools" .= object [fromString role .= digest | (role, digest) <- tools]
          ]
  pure
    StageDigests
      { digestsValue = value
      , digestsSources = M.fromList sources
      , digestsLinked = linked
      , digestsSymbols = symbols
      }

jobIdentityFor :: Options -> StageDigests -> String -> IO JobDigests
jobIdentityFor options stage stageKey = do
  oracleDigest <- fileDigest (optOracle options)
  runnerDigest <- fileDigest (optRunner options)
  slotDigest <- fileDigest (optSlot options)
  defects <- forM (optDefects options) $ \(name, bin, symbolPath) -> do
    binDigest <- fileDigest bin
    symbolsDigest <- fileDigest symbolPath
    pure (name, bin, symbolPath, binDigest, symbolsDigest)
  let value =
        object
          [ "schema_version" .= (1 :: Int)
          , "stage_key" .= stageKey
          , "sources" .= object [fromString role .= digest | (role, digest) <- M.toList (digestsSources stage)]
          , "candidate" .= object ["linked" .= digestsLinked stage, "symbols" .= digestsSymbols stage]
          , "comparison" .= object
              [ "ids" .= fmap selectionIds (optIds options)
              , "full_native" .= isNothing (optIds options)
              , "defects" .=
                  [ object ["name" .= name, "bin_sha256" .= binDigest, "symbols_sha256" .= symbolsDigest]
                  | (name, _, _, binDigest, symbolsDigest) <- defects
                  ]
              ]
          , "oracle" .= object ["path" .= optOracle options, "sha256" .= oracleDigest]
          , "runner" .= object ["path" .= optRunner options, "sha256" .= runnerDigest]
          , "emulator_slot" .= object ["path" .= optSlot options, "sha256" .= slotDigest]
          ]
  pure JobDigests {jobValue = value, jobRunnerSha256 = runnerDigest, jobDefects = defects}

expectedInputsFor :: Options -> StageDigests -> StageFiles -> JobDigests -> M.Map String (FilePath, String)
expectedInputsFor options stage files jobs =
  M.fromList $
    [ ("image4", (optImage4 options, M.findWithDefault "" "image4" (digestsSources stage)))
    , ("candidate_image4", (stageImage4Path files, stageImage4Sha256 files))
    , ("contract", (stageContractPath files, stageContractSha256 files))
    , ("linked", (optLinked options, digestsLinked stage))
    , ("symbols", (optSymbols options, digestsSymbols stage))
    ]
      ++ [(role, (path, digest)) | (role, path) <- sourceRoles options, Just digest <- [M.lookup role (digestsSources stage)]]
      ++ [("defect_" ++ name, (bin, binDigest)) | (name, bin, _, binDigest, _) <- jobDefects jobs]
      ++ [("defect_" ++ name ++ "_symbols", (symbols, symbolsDigest)) | (name, _, symbols, _, symbolsDigest) <- jobDefects jobs]

-- Driver -----------------------------------------------------------------------

data Setup = Setup
  { setupOptions :: Options
  , setupOut :: FilePath
  , setupStage :: StageDigests
  , setupStageKey :: String
  , setupJobs :: JobDigests
  , setupJobKey :: String
  }

data JobView = JobView
  { jobOptions :: Options
  , jobKey :: String
  , jobStageKey :: String
  , jobIdentity :: Value
  , jobStageStatus :: String
  , jobStageFiles :: StageFiles
  , jobStageLogs :: [FilePath]
  , jobNotes :: [String]
  , jobExpectedInputs :: M.Map String (FilePath, String)
  , jobRunnerPath :: FilePath
  , viewRunnerSha256 :: String
  }

data Attempt = Attempt
  { attemptExit :: Int
  , attemptTimedOut :: Bool
  , attemptSummary :: Either String OracleSummary
  , attemptDir :: FilePath
  , attemptParent :: FilePath
  , attemptStdoutLog :: FilePath
  , attemptStderrLog :: FilePath
  }

runQualification :: Options -> IO (Int, Value)
runQualification supplied = do
  invocation <- getCurrentDirectory
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe (pure invocation) makeAbsolute
  options <- anchorRuntimePaths invocation workspace supplied
  setCurrentDirectory workspace
  prepared <- try (setupJob options) :: IO (Either SomeException (Either String Setup))
  case prepared of
    Left exception -> publishSetupError options (displayException exception)
    Right (Left message) -> publishSetupError options message
    Right (Right setup) -> do
      locked <- withOutLock (setupOut setup) (runLocked setup)
      case locked of
        Left message ->
          pure (2, preJobVerdict (setupOptions setup) "" ("--out is busy: " ++ message))
        Right outcome -> pure outcome

anchorRuntimePaths :: FilePath -> FilePath -> Options -> IO Options
anchorRuntimePaths invocation workspace options = do
  runner <- anchor (optRunner options)
  slot <- anchor (optSlot options)
  oracle <- anchor (optOracle options)
  stage <- anchor (optStage options)
  encoder <- anchor (optStageEncoder options)
  rebuild <- anchor (optStageRebuild options)
  decoder <- anchor (optStageDecoder options)
  linked <- anchor (optLinked options)
  symbols <- anchor (optSymbols options)
  defects <- forM (optDefects options) $ \(name, image, syms) -> do
    anchoredImage <- anchor image
    anchoredSymbols <- anchor syms
    pure (name, anchoredImage, anchoredSymbols)
  pure options
    { optRunner = runner, optSlot = slot, optOracle = oracle, optStage = stage
    , optStageEncoder = encoder, optStageRebuild = rebuild, optStageDecoder = decoder
    , optLinked = linked, optSymbols = symbols, optDefects = defects
    }
  where
    anchor path = do
      present <- doesFileExist (invocation </> path)
      makeAbsolute ((if present then invocation else workspace) </> path)

setupJob :: Options -> IO (Either String Setup)
setupJob options = do
  resolved <- resolveInputs options
  digest <- fileDigest (optImage4 resolved)
  if digest /= canonicalImage4
    then pure (Left "--image4 is not the canonical X-M1 1.50 image4")
    else do
      scratch <- privateScratchRoot
      cacheRoot <- privateCacheRoot
      outResult <- resolveOutput "--out" scratch (optOut resolved)
      cacheResult <- resolveCacheOutput "--cache" scratch cacheRoot (optCache resolved)
      case (outResult, cacheResult) of
        (Left problem, _) -> pure (Left problem)
        (_, Left problem) -> pure (Left problem)
        (Right out, Right cache) -> do
          let withPaths = resolved {optOut = out, optCache = cache}
          stage <- stageIdentityFor withPaths
          let stageKey = identityHash (digestsValue stage)
          jobs <- jobIdentityFor withPaths stage stageKey
          pure
            ( Right
                Setup
                  { setupOptions = withPaths
                  , setupOut = out
                  , setupStage = stage
                  , setupStageKey = stageKey
                  , setupJobs = jobs
                  , setupJobKey = identityHash (jobValue jobs)
                  }
            )

resolveInputs :: Options -> IO Options
resolveInputs options = do
  dat <- resolve (optDat options) "--dat"
  image4 <- resolve (optImage4 options) "--image4"
  loaderA <- resolve (optLoaderA options) "--loader-a"
  loaderB <- resolve (optLoaderB options) "--loader-b"
  image5 <- resolve (optImage5 options) "--image5"
  plain <- resolve (optPlain options) "--plain"
  image2 <- resolve (optImage2 options) "--image2"
  linked <- resolve (optLinked options) "--linked"
  symbols <- resolve (optSymbols options) "--symbols"
  runner <- resolve (optRunner options) "--runner"
  slot <- resolve (optSlot options) "--emulator-slot"
  oracle <- resolve (optOracle options) "--oracle"
  stage <- resolve (optStage options) "--stage"
  encoder <- resolve (optStageEncoder options) "--stage-encoder"
  rebuild <- resolve (optStageRebuild options) "--stage-rebuild"
  decoder <- resolve (optStageDecoder options) "--stage-decoder"
  defects <- forM (optDefects options) $ \(name, bin, symbolPath) -> do
    resolvedBin <- resolve bin ("--defect " ++ name ++ " binary")
    resolvedSymbols <- resolve symbolPath ("--defect " ++ name ++ " symbols")
    pure (name, resolvedBin, resolvedSymbols)
  pure
    options
      { optDat = dat
      , optImage4 = image4
      , optLoaderA = loaderA
      , optLoaderB = loaderB
      , optImage5 = image5
      , optPlain = plain
      , optImage2 = image2
      , optLinked = linked
      , optSymbols = symbols
      , optRunner = runner
      , optSlot = slot
      , optOracle = oracle
      , optStage = stage
      , optStageEncoder = encoder
      , optStageRebuild = rebuild
      , optStageDecoder = decoder
      , optDefects = defects
      }
  where
    resolve path label = do
      exists <- doesFileExist path
      unless exists (failH (label ++ ": file not found: " ++ path))
      canonicalizePath path

withOutLock :: FilePath -> IO (Either String a) -> IO (Either String a)
withOutLock out action = mask $ \restore -> do
  acquired <- try $ bracketOnError
    (openFd (out </> ".qualify.lock") ReadWrite defaultFileFlags {creat = Just 0o600})
    closeFd
    (\fd -> do
      setFdOption fd CloseOnExec True
      setLock fd (WriteLock, AbsoluteSeek, 0, 0)
      pure fd)
  case acquired of
    Left (exception :: IOException) -> pure (Left (displayException exception))
    Right fd -> restore action `finally` closeFd fd

runLocked :: Setup -> IO (Either String (Int, Value))
runLocked setup = do
  let options = setupOptions setup
      scratchBase = "private-data/scratch/PropertyService2/qualify" </> setupJobKey setup </> "attempts"
  createDirectoryIfMissing True scratchBase
  reserved <- reserveDirectory scratchBase "attempt-"
  case reserved of
    Left problem -> do
      outcome <- publishSetupError options ("cannot reserve an attempt directory: " ++ problem)
      pure (Right outcome)
    Right attemptParent -> do
      staged <- prepareStage options (digestsValue (setupStage setup)) (setupStageKey setup) (setupJobKey setup) attemptParent
      case staged of
        Left message -> do
          outcome <- publishAttemptSetupError setup attemptParent message
          pure (Right outcome)
        Right (files, status, logs, notes) -> do
          let view =
                JobView
                  { jobOptions = options
                  , jobKey = setupJobKey setup
                  , jobStageKey = setupStageKey setup
                  , jobIdentity = jobValue (setupJobs setup)
                  , jobStageStatus = status
                  , jobStageFiles = files
                  , jobStageLogs = logs
                  , jobNotes = notes
                  , jobExpectedInputs = expectedInputsFor options (setupStage setup) files (setupJobs setup)
                  , jobRunnerPath = optRunner options
                  , viewRunnerSha256 = jobRunnerSha256 (setupJobs setup)
                  }
          attempted <- try (runAttempt view attemptParent) :: IO (Either SomeException Attempt)
          let attempt = case attempted of
                Left exception -> failedAttempt attemptParent (displayException exception)
                Right outcome -> outcome
          Right <$> finalize view attempt

finalize :: JobView -> Attempt -> IO (Int, Value)
finalize view attempt = do
  checked <- try (verifyEffective view)
  let integrity = case checked of
        Left (exception :: IOException) -> Left (displayException exception)
        Right result -> result
  case integrity of
    Left problem -> publishAttemptError view attempt "identity" problem
    Right () -> do
      let code = judgeCode view attempt
          review = attemptReview view attempt code
          verdict = judgeVerdict view attempt
      published <- commitResults (optOut options) (attemptParent attempt) (verdict, review) (appendAttempt view attempt)
      case published of
        Right () -> pure (code, verdict)
        Left problem -> publicationError view attempt problem
  where
    options = jobOptions view

publicationError :: JobView -> Attempt -> String -> IO (Int, Value)
publicationError view attempt problem = do
  let message = "artifact publication failed: " ++ problem
      fallback = attemptErrorVerdict view attempt "publication" message
  _ <- commitResults (optOut (jobOptions view)) (attemptParent attempt) (fallback, attemptErrorReview view attempt "publication" message) (pure ())
  pure (2, fallback)

publishAttemptError :: JobView -> Attempt -> String -> String -> IO (Int, Value)
publishAttemptError view attempt predicate problem = do
  let verdict = attemptErrorVerdict view attempt predicate problem
      review = attemptErrorReview view attempt predicate problem
  published <- commitResults (optOut (jobOptions view)) (attemptParent attempt) (verdict, review) (appendAttempt view attempt)
  case published of
    Right () -> pure (2, verdict)
    Left message -> publicationError view attempt message

publishSetupError :: Options -> String -> IO (Int, Value)
publishSetupError options message = do
  outAttempt <- try (privateScratchRoot >>= \scratch -> resolveOutput "--out" scratch (optOut options))
  case outAttempt of
    Left (exception :: IOException) -> pure (2, preJobVerdict options "" (displayException exception))
    Right (Left problem) -> pure (2, preJobVerdict options "" problem)
    Right (Right out) -> do
      let resolved = options {optOut = out}
      target <- try (do
        createDirectoryIfMissing True (out </> "results")
        reserveDirectory (out </> "results") "error-")
      case target of
        Left (exception :: SomeException) -> pure (2, preJobVerdict resolved "" (displayException exception))
        Right (Left problem) -> pure (2, preJobVerdict resolved "" problem)
        Right (Right directory) -> do
          let verdict = preJobVerdict resolved directory message
              review = preJobReview resolved directory message
          published <- commitResults out directory (verdict, review) (pure ())
          pure $ case published of
            Left problem -> (2, preJobVerdict resolved directory ("artifact publication failed: " ++ problem))
            Right () -> (2, verdict)

publishAttemptSetupError :: Setup -> FilePath -> String -> IO (Int, Value)
publishAttemptSetupError setup attemptParent message = do
  let options = setupOptions setup
      resolved = options {optOut = setupOut setup}
      verdict = preJobVerdict resolved attemptParent message
      review = preJobReview resolved attemptParent message
  _ <- commitResults (setupOut setup) attemptParent (verdict, review) (pure ())
  pure (2, verdict)

verifyEffective :: JobView -> IO (Either String ())
verifyEffective view = do
  let options = jobOptions view
  stage <- stageIdentityFor options
  if identityHash (digestsValue stage) /= jobStageKey view
    then pure (Left "source or staging tool identity changed during the run")
    else do
      let stageDirectory = takeDirectory (stageCandidateDatPath (jobStageFiles view))
      checked <- verifyStage stageDirectory (digestsValue stage)
      case checked of
        Left problem -> pure (Left ("completed stage changed during the run: " ++ problem))
        Right _ -> do
          jobs <- jobIdentityFor options stage (jobStageKey view)
          pure $
            if identityHash (jobValue jobs) /= jobKey view
              then Left "candidate, defect, runner, oracle or option identity changed during the run"
              else Right ()

prepareStage :: Options -> Value -> String -> String -> FilePath -> IO (Either String (StageFiles, String, [FilePath], [String]))
prepareStage options identity stageKey jobKey logDirectory = do
  exists <- doesPathExist final
  if exists
    then do
      isDirectory <- doesDirectoryExist final
      if not isDirectory
        then pure (Left (final ++ ": stage path is not a directory"))
        else do
          checked <- verifyStage final identity
          case checked of
            Right files -> pure (Right (files, "reused", [], []))
            Left reason -> do
              moved <- quarantine final (optOut options </> "stages" </> "rejected")
              case moved of
                Left problem -> pure (Left (reason ++ "; cannot quarantine the rejected stage: " ++ problem))
                Right rejected -> stageFresh (reason ++ "; rejected to " ++ rejected)
    else stageFresh ""
  where
    final = optOut options </> "stages" </> stageKey
    stageFresh note = do
      createDirectoryIfMissing True (optOut options </> "stages")
      reserved <- reserveDirectory (optOut options </> "stages") (".partial-" ++ stageKey ++ "-")
      case reserved of
        Left problem -> pure (Left ("cannot reserve a staging directory: " ++ problem))
        Right temp -> do
          let scratch = "private-data/scratch/PropertyService2/qualify" </> jobKey </> "stage"
              stdoutLog = logDirectory </> "stage.stdout.log"
              stderrLog = logDirectory </> "stage.stderr.log"
              arguments =
                [ optStageEncoder options
                , optStageRebuild options
                , optStageDecoder options
                , optDat options
                , optImage4 options
                , optLoaderA options
                , optLoaderB options
                , optImage5 options
                , optPlain options
                , optImage2 options
                , optLinked options
                , optSymbols options
                , scratch
                , temp </> "candidate.dat"
                , temp </> "contract.json"
                , temp </> "image4.bin"
                ]
          createDirectoryIfMissing True scratch
          (timedOut, exit, stdoutBytes, stderrBytes) <- runBounded stageTimeoutMicros stageOutputCap (optStage options) arguments B.empty
          B.writeFile stdoutLog stdoutBytes
          B.writeFile stderrLog stderrBytes
          if timedOut
            then pure (Left ("staging timed out; logs " ++ stdoutLog ++ " " ++ stderrLog))
            else
              if exit /= ExitSuccess
                then pure (Left ("staging failed: " ++ bounded (tailText stderrBytes) ++ "; logs " ++ stdoutLog ++ " " ++ stderrLog))
                else do
                  let outputs =
                        [ ("candidate_dat", temp </> "candidate.dat")
                        , ("contract", temp </> "contract.json")
                        , ("image4", temp </> "image4.bin")
                        ]
                  writeStageManifest temp identity outputs
                  committed <- commitStage temp final
                  case committed of
                    Left problem -> pure (Left ("cannot publish the completed stage: " ++ problem))
                    Right _ -> do
                      checked <- verifyStage final identity
                      pure $ case checked of
                        Left problem -> Left ("published stage failed its integrity check: " ++ problem)
                        Right files -> Right (files, "staged", [stdoutLog, stderrLog], [note | not (null note)])

tailText :: B.ByteString -> String
tailText bytes =
  let rows = lines (BC.unpack bytes)
   in take detailLimit (unlines (drop (max 0 (length rows - 8)) rows))

oracleArguments :: JobView -> FilePath -> [String]
oracleArguments view directory =
  concat
    [ ["--runner", optRunner options]
    , ["--linked", optLinked options]
    , ["--symbols", optSymbols options]
    , ["--image4", optImage4 options]
    , ["--candidate-image4", stageImage4Path (jobStageFiles view)]
    , ["--contract", stageContractPath (jobStageFiles view)]
    , ["--dat", optDat options]
    , ["--loader-a", optLoaderA options]
    , ["--loader-b", optLoaderB options]
    , ["--image5", optImage5 options]
    , ["--plain", optPlain options]
    , ["--image2", optImage2 options]
    , ["--out", directory]
    , ["--cache", optCache options]
    ]
    ++ maybe [] (\selection -> ["--ids", intercalate "," (selectionIds selection)]) (optIds options)
    ++ concat [["--defect", name, bin, symbols] | (name, bin, symbols) <- optDefects options]
  where
    options = jobOptions view

runAttempt :: JobView -> FilePath -> IO Attempt
runAttempt view parent = do
  let options = jobOptions view
      directory = parent </> "results"
      stdoutLog = parent </> "stdout.log"
      stderrLog = parent </> "stderr.log"
  outcome <- try (runBounded oracleTimeoutMicros oracleOutputCap (optSlot options) (optOracle options : oracleArguments view directory) B.empty)
  case outcome of
    Left (exception :: SomeException) -> do
      B.writeFile stdoutLog B.empty
      B.writeFile stderrLog (BC.pack (displayException exception))
      pure (Attempt 1 False (Left (displayException exception)) directory parent stdoutLog stderrLog)
    Right (timedOut, exit, stdoutBytes, stderrBytes) -> do
      B.writeFile stdoutLog stdoutBytes
      B.writeFile stderrLog stderrBytes
      summary <- readAttemptSummary (directory </> "results.json")
      pure (Attempt (exitCode exit) timedOut summary directory parent stdoutLog stderrLog)

failedAttempt :: FilePath -> String -> Attempt
failedAttempt parent message = Attempt 1 False (Left message) (parent </> "results") parent (parent </> "stdout.log") (parent </> "stderr.log")

readAttemptSummary :: FilePath -> IO (Either String OracleSummary)
readAttemptSummary path = do
  exists <- doesFileExist path
  if not exists
    then pure (Left (path ++ ": the oracle wrote no results.json"))
    else do
      attempted <- try (readRegular (64 * 1024 * 1024) path)
      pure $ case attempted of
        Left (exception :: SomeException) -> Left (path ++ ": " ++ displayException exception)
        Right bytes -> case eitherDecodeStrict' bytes :: Either String Value of
          Left problem -> Left (path ++ ": " ++ problem)
          Right value -> either (Left . ((path ++ ": ") ++)) Right (parseOracleSummary value)

exitCode :: ExitCode -> Int
exitCode ExitSuccess = 0
exitCode (ExitFailure code) = code

-- Verdict ----------------------------------------------------------------------

data FailureRow = FailureRow
  { rowPredicate :: String
  , rowCase :: String
  , rowKind :: String
  , rowDetail :: String
  }

judgeCode :: JobView -> Attempt -> Int
judgeCode view attempt = case attemptSummary attempt of
  Left _ -> 2
  Right summary
    | attemptTimedOut attempt -> 2
    | not (null (summaryMismatch view summary)) -> 2
    | otherwise ->
        let rows = failureRows attempt summary
            passed = summaryPassed summary && attemptExit attempt == 0 && null rows
         in if passed then 0 else 1

judgeVerdict :: JobView -> Attempt -> Value
judgeVerdict view attempt = case attemptSummary attempt of
  Left problem -> attemptErrorVerdict view attempt "oracle" problem
  Right summary ->
    let mismatches = summaryMismatch view summary
     in if attemptTimedOut attempt
          then attemptErrorVerdict view attempt "timeout" "the oracle exceeded its bounded time"
          else
            if not (null mismatches)
              then attemptErrorVerdict view attempt "identity" (unlines (take 5 mismatches))
              else
                let rows = failureRows attempt summary
                    passed = summaryPassed summary && attemptExit attempt == 0 && null rows
                 in verdictValue view attempt summary (if passed then "passed" else "failed") rows

summaryMismatch :: JobView -> OracleSummary -> [String]
summaryMismatch view summary =
  [ "runner path differs: " ++ summaryRunnerPath summary ++ " vs " ++ jobRunnerPath view
  | summaryRunnerPath summary /= jobRunnerPath view
  ]
    ++ ["runner sha256 differs" | summaryRunnerSha256 summary /= viewRunnerSha256 view]
    ++ ["contract staged image4 sha256 differs" | summaryContractImage4Sha256 summary /= stageImage4Sha256 files]
    ++ ["cache root differs" | summaryCacheRoot summary /= optCache options]
    ++ ["subset flag differs" | summarySubset summary /= isJust (optIds options)]
    ++ ["subset ids differ" | summarySubset summary, sortOn id (nub (summaryIds summary)) /= sortOn id (nub (maybe [] selectionIds (optIds options)))]
    ++ ["input " ++ role ++ " is missing from results" | role <- M.keys expected, not (M.member role actual)]
    ++ [ "input " ++ role ++ " path or sha256 differs"
       | (role, (path, digest)) <- M.toList expected
       , Just (actualPath, actualDigest) <- [M.lookup role actual]
       , actualPath /= path || actualDigest /= digest
       ]
  where
    options = jobOptions view
    files = jobStageFiles view
    expected = jobExpectedInputs view
    actual = summaryInputs summary

failureRows :: Attempt -> OracleSummary -> [FailureRow]
failureRows attempt summary =
  modelRows ++ passRows ++ controlRows ++ exitRows ++ declaredRow
  where
    modelRows = differenceRows "model:" summary
    passRows = differenceRows "pass2:" summary
    controlRows = concatMap controlRowsOf (summaryControls summary)
    controlRowsOf control
      | controlHolds control = []
      | otherwise = case controlFirstDifferences control of
          sample : _ ->
            [FailureRow ("control:" ++ controlName control) (sampleCase sample) (sampleKind sample) (sampleDetail sample)]
          [] -> [FailureRow ("control:" ++ controlName control) "" "" "control recorded no failing case"]
    exitRows = [FailureRow "oracle_exit" "" "" ("oracle exited " ++ show (attemptExit attempt)) | attemptExit attempt /= 0]
    declaredRow =
      [ FailureRow "results" "" "" "results.json reports failure with no failure row"
      | not (summaryPassed summary)
      , null (modelRows ++ passRows ++ controlRows)
      ]

differenceRows :: String -> OracleSummary -> [FailureRow]
differenceRows prefix summary =
  [ FailureRow (takeWhile (/= ':') key) (sampleCase sample) key (sampleDetail sample)
  | (key, samples) <- summaryDifferences summary
  , prefix `isPrefixOf` key
  , sample <- samples
  ]

verdictValue :: JobView -> Attempt -> OracleSummary -> String -> [FailureRow] -> Value
verdictValue view attempt summary status rows =
  object
    [ "schema_version" .= (1 :: Int)
    , "verdict" .= status
    , "native_only" .= True
    , "scope" .= object ["subset" .= subset, "ids" .= ids, "full_native" .= not subset]
    , "identity" .= identityValue view
    , "stage" .= stageValue view
    , "execution" .= executionValue summary attempt
    , "counts" .= object ["cases" .= summaryCasesPerRole summary, "grid_cases" .= summaryGridCases summary, "controls" .= length (summaryControls summary)]
    , "failure" .= maybe Null (failureJson attempt) (headOf rows)
    , "failures" .= [failureJson attempt row | row <- take 20 rows]
    , "artifacts" .= artifactsValue view attempt
    , "limits" .= limitText subset
    ]
  where
    subset = summarySubset summary
    ids = if subset then summaryIds summary else []

headOf :: [a] -> Maybe a
headOf rows = case rows of
  row : _ -> Just row
  [] -> Nothing

executionValue :: OracleSummary -> Attempt -> Value
executionValue summary attempt =
  object
    [ "cache" .= summaryCacheRoot summary
    , "batches_executed" .= summaryExecutedBatches summary
    , "batches_reused" .= summaryReusedBatches summary
    , "cases_executed" .= summaryExecutedCases summary
    , "cases_reused" .= summaryReusedCases summary
    , "oracle_exit" .= attemptExit attempt
    , "attempt" .= attemptParent attempt
    ]

identityValue :: JobView -> Value
identityValue view =
  object
    [ "job" .= jobKey view
    , "stage" .= jobStageKey view
    , "sources" .= identityField "sources"
    , "candidate" .= identityField "candidate"
    , "contract_sha256" .= stageContractSha256 files
    , "candidate_dat_sha256" .= stageCandidateDatSha256 files
    , "staged_image4_sha256" .= stageImage4Sha256 files
    , "runner" .= identityField "runner"
    , "oracle" .= identityField "oracle"
    ]
  where
    files = jobStageFiles view
    identityField key = case jobIdentity view of
      Object objectValue -> maybe Null id (KM.lookup (K.fromString key) objectValue)
      _ -> Null

stageValue :: JobView -> Value
stageValue view =
  object
    [ "status" .= jobStageStatus view
    , "path" .= takeDirectory (stageImage4Path (jobStageFiles view))
    , "notes" .= jobNotes view
    ]

artifactsValue :: JobView -> Attempt -> Value
artifactsValue view attempt =
  object
    [ "verdict" .= (attemptParent attempt </> "verdict.json")
    , "review_packet" .= (attemptParent attempt </> "review-packet.json")
    , "current_pointer" .= (optOut (jobOptions view) </> "current")
    , "results" .= resultsPath attempt
    , "logs" .= logsOf view attempt
    , "attempt" .= attemptParent attempt
    ]

logsOf :: JobView -> Attempt -> [FilePath]
logsOf view attempt =
  filter (not . null) (jobStageLogs view ++ [attemptStdoutLog attempt, attemptStderrLog attempt])

failureJson :: Attempt -> FailureRow -> Value
failureJson attempt row =
  object
    [ "predicate" .= rowPredicate row
    , "case" .= rowCase row
    , "kind" .= rowKind row
    , "detail" .= bounded (rowDetail row)
    , "results" .= resultsPath attempt
    , "log" .= attemptStderrLog attempt
    ]

resultsPath :: Attempt -> FilePath
resultsPath attempt = attemptDir attempt </> "results.json"

limitText :: Bool -> [String]
limitText subset =
  [ "Native-only host comparison; it does not establish research, system, hardware, timing, persistence or full-replacement acceptance"
  ]
    ++ ["Subset: only the ids named by --ids were compared; every control still ran in full" | subset]

attemptErrorVerdict :: JobView -> Attempt -> String -> String -> Value
attemptErrorVerdict view attempt predicate detail =
  object
    [ "schema_version" .= (1 :: Int)
    , "verdict" .= ("error" :: String)
    , "native_only" .= True
    , "scope" .= object ["subset" .= sub, "ids" .= ids, "full_native" .= not sub]
    , "identity" .= identityValue view
    , "stage" .= stageValue view
    , "execution" .= object ["oracle_exit" .= attemptExit attempt, "attempt" .= attemptParent attempt]
    , "counts" .= Null
    , "failure" .= errorFailure predicate detail attempt
    , "failures" .= [errorFailure predicate detail attempt]
    , "artifacts" .= artifactsValue view attempt
    , "limits" .= limitText sub
    ]
  where
    sub = isJust (optIds (jobOptions view))
    ids = maybe [] selectionIds (optIds (jobOptions view))

errorFailure :: String -> String -> Attempt -> Value
errorFailure predicate detail attempt =
  object
    [ "predicate" .= predicate
    , "case" .= ("" :: String)
    , "kind" .= ("" :: String)
    , "detail" .= bounded detail
    , "results" .= resultsPath attempt
    , "log" .= attemptStderrLog attempt
    ]

attemptReview :: JobView -> Attempt -> Int -> Value
attemptReview view attempt code =
  let summary = either (const Nothing) Just (attemptSummary attempt)
      status = if code == 0 then "passed" else if code == 1 then "failed" else "error" :: String
   in object
        [ "schema_version" .= (1 :: Int)
        , "kind" .= ("property-service-review-packet" :: String)
        , "job" .= jobKey view
        , "verdict" .= status
        , "identity" .= jobIdentity view
        , "stage" .= object
            [ "key" .= jobStageKey view
            , "status" .= jobStageStatus view
            , "path" .= takeDirectory (stageImage4Path (jobStageFiles view))
            , "outputs" .= object
                [ "candidate_dat_sha256" .= stageCandidateDatSha256 (jobStageFiles view)
                , "contract_sha256" .= stageContractSha256 (jobStageFiles view)
                , "image4_sha256" .= stageImage4Sha256 (jobStageFiles view)
                ]
            , "notes" .= jobNotes view
            ]
        , "execution" .= maybe Null (flip executionValue attempt) summary
        , "scope" .= reviewScope summary
        , "controls" .= maybe [] (map controlRow . summaryControls) summary
        , "failures" .= maybe [] (map (failureJson attempt) . take 50 . failureRows attempt) summary
        , "artifacts" .= artifactsValue view attempt
        , "limits" .= limitText (maybe False summarySubset summary)
        ]

reviewScope :: Maybe OracleSummary -> Value
reviewScope summary =
  object
    [ "subset" .= subset
    , "ids" .= if subset then maybe [] summaryIds summary else []
    , "full_native" .= not subset
    ]
  where
    subset = maybe False summarySubset summary

controlRow :: ControlSummary -> Value
controlRow control =
  object
    [ "name" .= controlName control
    , "kind" .= controlKind control
    , "note" .= controlNote control
    , "cases" .= controlCases control
    , "failed_cases" .= controlFailedCases control
    , "holds" .= controlHolds control
    ]

attemptErrorReview :: JobView -> Attempt -> String -> String -> Value
attemptErrorReview view attempt predicate detail =
  object
    [ "schema_version" .= (1 :: Int)
    , "kind" .= ("property-service-review-packet" :: String)
    , "job" .= jobKey view
    , "verdict" .= ("error" :: String)
    , "identity" .= jobIdentity view
    , "stage" .= stageValue view
    , "failure" .= errorFailure predicate detail attempt
    , "artifacts" .= artifactsValue view attempt
    , "limits" .= limitText (isJust (optIds (jobOptions view)))
    ]

preJobVerdict :: Options -> FilePath -> String -> Value
preJobVerdict options directory message =
  object
    [ "schema_version" .= (1 :: Int)
    , "verdict" .= ("error" :: String)
    , "native_only" .= True
    , "scope" .= object ["subset" .= isJust (optIds options), "ids" .= maybe [] selectionIds (optIds options), "full_native" .= isNothing (optIds options)]
    , "identity" .= Null
    , "stage" .= Null
    , "execution" .= Null
    , "counts" .= Null
    , "failure" .= object ["predicate" .= ("driver" :: String), "case" .= ("" :: String), "kind" .= ("" :: String), "detail" .= bounded message, "results" .= Null, "log" .= Null]
    , "failures" .= [object ["predicate" .= ("driver" :: String), "detail" .= bounded message]]
    , "artifacts" .= preJobArtifacts options directory
    , "limits" .= limitText (isJust (optIds options))
    ]

preJobReview :: Options -> FilePath -> String -> Value
preJobReview options directory message =
  object
    [ "schema_version" .= (1 :: Int)
    , "kind" .= ("property-service-review-packet" :: String)
    , "verdict" .= ("error" :: String)
    , "identity" .= Null
    , "failure" .= object ["predicate" .= ("driver" :: String), "detail" .= bounded message]
    , "artifacts" .= preJobArtifacts options directory
    , "limits" .= limitText (isJust (optIds options))
    ]

preJobArtifacts :: Options -> FilePath -> Value
preJobArtifacts options directory =
  object
    [ "verdict" .= path "verdict"
    , "review_packet" .= path "review_packet"
    , "current_pointer" .= path "current"
    , "results" .= Null
    , "logs" .= ([] :: [String])
    , "attempt" .= Null
    ]
  where
    path name
      | null directory = Null
      | name == "current" = String (T.pack (optOut options </> "current"))
      | otherwise = String (T.pack (directory </> (if name == "review_packet" then "review-packet.json" else name ++ ".json")))

bounded :: String -> String
bounded message = take detailLimit message

-- Result publication -----------------------------------------------------------

-- | Writes the verdict and review packet into the immutable attempt directory, runs the journal
-- action, then atomically replaces the job's current-result pointer. Both files are in place
-- before the pointer changes.
commitResults :: FilePath -> FilePath -> (Value, Value) -> IO () -> IO (Either String ())
commitResults out directory (verdict, review) beforePointer = do
  attempted <- try $ do
    B.writeFile (directory </> "verdict.json") (encodeCanonical verdict)
    B.writeFile (directory </> "review-packet.json") (encodeCanonical review)
    beforePointer
    atomicWrite (out </> "current") $
      encodeCanonical
        ( object
            [ "result" .= (directory :: FilePath)
            , "verdict" .= (directory </> "verdict.json")
            , "review_packet" .= (directory </> "review-packet.json")
            ]
        )
  pure $ case attempted of
    Right () -> Right ()
    Left (exception :: SomeException) -> Left (displayException exception)

atomicWrite :: FilePath -> B.ByteString -> IO ()
atomicWrite path bytes = do
  (temporary, handle) <- openTempFile (takeDirectory path) ".qualify-"
  hClose handle
  B.writeFile temporary bytes
  renameFile temporary path

appendAttempt :: JobView -> Attempt -> IO ()
appendAttempt view attempt = do
  let line =
        encodeCanonical
          ( object
              [ "attempt" .= attemptParent attempt
              , "exit" .= attemptExit attempt
              , "verdict" .= attemptVerdictLabel view attempt
              , "results" .= resultsPath attempt
              ]
          )
  B.appendFile (optOut (jobOptions view) </> "attempts.jsonl") (line <> "\n")

attemptVerdictLabel :: JobView -> Attempt -> String
attemptVerdictLabel view attempt = case attemptSummary attempt of
  Left _ -> "error"
  Right summary
    | attemptTimedOut attempt -> "error"
    | not (null (summaryMismatch view summary)) -> "error"
    | summaryPassed summary && attemptExit attempt == 0 && null (failureRows attempt summary) -> "passed"
    | otherwise -> "failed"

parseSelection :: String -> Either String PropertySelection
parseSelection text = PropertySelection . nub <$> mapM parse (split text)
  where
    split value = case break (== ',') value of
      (item, []) -> [item]
      (item, _ : rest) -> item : split rest
    parse item = case item of
      '0' : 'x' : digits | not (null digits) -> case readHex digits :: [(Integer, String)] of
        [(identifier, "")] | identifier <= 0x286 -> Right (PropertyId (fromInteger identifier))
        _ -> Left ("--ids: invalid property " ++ item)
      _ -> Left ("--ids: invalid property " ++ item)

selectionIds :: PropertySelection -> [String]
selectionIds (PropertySelection identifiers) = ["0x" ++ showHex identifier "" | PropertyId identifier <- identifiers]
