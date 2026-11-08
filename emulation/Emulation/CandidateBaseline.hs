{-# LANGUAGE OverloadedStrings #-}
-- | Route observations as ordered fields, their baselines and the comparison
-- between them. A field is one JSON leaf of an observed receipt file, or the
-- SHA-256 of a non-JSON observed file. Exclusions are explicit file and
-- pointer patterns, each with a reason.
module Emulation.CandidateBaseline
  ( ReceiptDirectory (..)
  , FilePattern
  , filePattern
  , PointerPattern
  , pointerPattern
  , ExclusionReason (..)
  , Exclusion (..)
  , ObservationSpec (..)
  , flashBootSpec
  , instructionTimingExclusions
  , ramPushCardImageExclusions
  , Segment (..)
  , FieldName (..)
  , Field (..)
  , Observation
  , observe
  , withoutExcluded
  , Difference (..)
  , differenceField
  , compareObservations
  , NativeRunnerTransition
  , transitionBaseline
  , transitionCandidate
  , transitionRunnerPath
  , transitionBinding
  , NativeProvenanceChange (..)
  , NativeComparison (..)
  , admitNativeRunnerTransition
  , compareNativeObservations
  , renderFieldName
  , Baseline (..)
  , encodeBaseline
  , decodeBaseline
  , differenceValue
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither, withArray, withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (groupBy, isSuffixOf, sort, sortOn, stripPrefix)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.SegmentReceipt (SegmentName, decodeDeclaredSegments, segmentNameText)
import Emulation.Sha256 (parseSha256Hex, sha256, sha256Hex)
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.FilePath (splitDirectories, (</>))

newtype ReceiptDirectory = ReceiptDirectory FilePath

-- | Slash-separated receipt-relative path; @*@ matches any run of characters
-- within one component.
newtype FilePattern = FilePattern [String]

filePattern :: String -> FilePattern
filePattern = FilePattern . splitDirectories

-- | Slash-separated JSON pointer prefix; a @*@ component matches any one key
-- or index. The empty pattern matches the whole file.
newtype PointerPattern = PointerPattern [T.Text]

pointerPattern :: T.Text -> PointerPattern
pointerPattern text = PointerPattern (filter (not . T.null) (T.splitOn "/" text))

data ExclusionReason
  = Timing
  | Scheduling
  | RunLocation
  | CheckpointState
  | CandidateIdentity
  | SourcePhase
  deriving (Eq, Show)

data Exclusion = Exclusion
  { exclusionFile :: FilePattern
  , exclusionPointer :: PointerPattern
  , exclusionReason :: ExclusionReason
  }

data ObservationSpec = ObservationSpec
  { observedFiles :: [FilePattern]
  , observationExclusions :: [Exclusion]
  , observedSegmentFiles :: [FilePattern]
  }

-- Absolute, milestone and trace-hit instruction counters measure execution timing.
instructionTimingExclusions :: FilePattern -> [Exclusion]
instructionTimingExclusions file =
  [ Exclusion file (pointerPattern pointer) Timing
  | pointer <- ["/instructions", "/milestones/*/instructions", "/observations/*/trace_hits/*/instructions"]
  ]

ramPushCardImageExclusions :: [Exclusion]
ramPushCardImageExclusions =
  [ Exclusion (filePattern file) (pointerPattern pointer) RunLocation
  | file <- ["e2e-result.json", "cold-restart/e2e-cold-restart.json", "segment-*/e2e-result.json"]
  , pointer <- ["/card/image", "/media/card/image"] ]

flashBootSpec :: ObservationSpec
flashBootSpec = ObservationSpec
  [filePattern "flash-boot-receipt.json"]
  ([ omit "/emulator_source" CandidateIdentity
   , omit "/firmware/path" RunLocation
   , omit "/frame_png" RunLocation
   ] ++ instructionTimingExclusions (filePattern "flash-boot-receipt.json"))
  []
  where
    omit pointer = Exclusion (filePattern "flash-boot-receipt.json") (pointerPattern pointer)

data Segment = Key T.Text | Index Int
  deriving (Eq, Ord, Show)

-- | A JSON leaf of a file, or with no pointer the whole file's digest.
data FieldName = FieldName
  { fieldFile :: FilePath
  , fieldPointer :: Maybe [Segment]
  }
  deriving (Eq, Ord, Show)

data Field = Field
  { fieldName :: FieldName
  , fieldValue :: Value
  }
  deriving (Eq, Show)

-- | Fields in 'FieldName' order.
type Observation = [Field]

globComponent :: String -> String -> Bool
globComponent [] candidate = null candidate
globComponent ('*' : rest) candidate = any (globComponent rest) (suffixes candidate)
  where suffixes text = text : case text of [] -> []; _ : more -> suffixes more
globComponent (c : rest) (x : more) = c == x && globComponent rest more
globComponent _ [] = False

matchesFile :: FilePattern -> FilePath -> Bool
matchesFile (FilePattern components) path =
  let parts = splitDirectories path
   in length components == length parts && and (zipWith globComponent components parts)

matchesPointer :: PointerPattern -> [Segment] -> Bool
matchesPointer (PointerPattern components) pointer =
  length components <= length pointer && and (zipWith segment components pointer)
  where
    segment "*" _ = True
    segment expected (Key key) = expected == key
    segment expected (Index index) = expected == T.pack (show index)

excluded :: [Exclusion] -> FieldName -> Bool
excluded exclusions (FieldName file pointer) = any covers exclusions
  where
    covers (Exclusion filePat pointerPat _) = matchesFile filePat file && case pointer of
      Nothing -> let PointerPattern components = pointerPat in null components
      Just segments -> matchesPointer pointerPat segments

receiptFiles :: FilePath -> IO [FilePath]
receiptFiles root = go ""
  where
    go relative = do
      entries <- sort <$> listDirectory (root </> relative)
      concat <$> forM entries (\entry -> do
        let path = if null relative then entry else relative </> entry
        directory <- doesDirectoryExist (root </> path)
        if directory then go path else do
          file <- doesFileExist (root </> path)
          pure [path | file])

-- | The receipt's observed fields with the spec's exclusions removed.
observe :: ObservationSpec -> ReceiptDirectory -> IO Observation
observe spec (ReceiptDirectory root) = do
  present <- doesDirectoryExist root
  files <- if present then receiptFiles root else pure []
  segments <- if null (observedSegmentFiles spec) then pure [] else declaredSegments root
  let prefixes = ["segment-" <> T.unpack (segmentNameText name) | name <- segments]
      inChild path = any (\prefix -> case stripPrefix (prefix ++ "/") path of
        Just relative -> any (`matchesFile` relative) (observedSegmentFiles spec)
        Nothing -> False) prefixes
      selected = [ path
                 | path <- files
                 , any (`matchesFile` path) (observedFiles spec) || inChild path ]
  fields <- concat <$> forM selected (\path -> do
    bytes <- B.readFile (root </> path)
    if ".json" `isSuffixOf` path
      then case eitherDecodeStrict' bytes of
        Right value -> pure (flatten path [] value)
        Left problem -> pure [Field (FieldName path Nothing) (String (T.pack ("unparseable JSON: " ++ problem)))]
      else pure [Field (FieldName path Nothing) (String (T.pack ("sha256:" ++ sha256Hex (sha256 bytes))))])
  pure (withoutExcluded spec fields)

declaredSegments :: FilePath -> IO [SegmentName]
declaredSegments root = do
  let path = root </> "e2e-segments.json"
  present <- doesFileExist path
  if not present then pure [] else do
    bytes <- B.readFile path
    either (ioError . userError . ((path ++ ": ") ++)) pure (decodeDeclaredSegments bytes)

-- | The fields the spec does not exclude, in 'FieldName' order. Applied to a
-- stored baseline, it compares under the current exclusions.
withoutExcluded :: ObservationSpec -> Observation -> Observation
withoutExcluded spec = sortOn fieldName . filter (not . excluded (observationExclusions spec) . fieldName)

flatten :: FilePath -> [Segment] -> Value -> [Field]
flatten path pointer value = case value of
  Object members | not (KM.null members) ->
    concat [flatten path (pointer ++ [Key (K.toText key)]) member | (key, member) <- KM.toList members]
  Array items | not (V.null items) ->
    concat [flatten path (pointer ++ [Index index]) item | (index, item) <- zip [0 ..] (V.toList items)]
  leaf -> [Field (FieldName path (Just pointer)) leaf]

data Difference
  = Changed FieldName Value Value
  | AbsentFromRun FieldName Value
  | AbsentFromBaseline FieldName Value
  deriving (Eq, Show)

differenceField :: Difference -> FieldName
differenceField (Changed name _ _) = name
differenceField (AbsentFromRun name _) = name
differenceField (AbsentFromBaseline name _) = name

-- | Every difference in 'FieldName' order; the first is the first differing field.
compareObservations :: Observation -> Observation -> [Difference]
compareObservations baseline observed = go (sortOn fieldName baseline) (sortOn fieldName observed)
  where
    go [] [] = []
    go (Field name value : rest) [] = AbsentFromRun name value : go rest []
    go [] (Field name value : rest) = AbsentFromBaseline name value : go [] rest
    go left@(Field name expected : lefts) right@(Field name' actual : rights) = case compare name name' of
      EQ | expected == actual -> go lefts rights
         | otherwise -> Changed name expected actual : go lefts rights
      LT -> AbsentFromRun name expected : go lefts right
      GT -> AbsentFromBaseline name' actual : go left rights

newtype NativeCaseId = NativeCaseId T.Text
  deriving (Eq, Ord)

-- | An admitted SDK-pin runner transition. Construct only through
-- 'admitNativeRunnerTransition', which proves the candidate digest names the
-- runner at the immutable path.
data NativeRunnerTransition = NativeRunnerTransition
  { transitionBaseline :: T.Text
  , transitionCandidate :: T.Text
  , transitionRunnerPath :: FilePath
  , transitionBinding :: T.Text
  } deriving (Eq, Show)

-- | One case-root runner identity accepted as an execution-provenance change.
data NativeProvenanceChange = NativeProvenanceChange
  { provenanceCase :: T.Text
  , provenanceField :: FieldName
  , provenanceBaseline :: Value
  , provenanceCandidate :: Value
  } deriving (Eq, Show)

data NativeComparison = NativeComparison
  { nativeDifferences :: [Difference]
  , nativeProvenanceChanges :: [NativeProvenanceChange]
  } deriving (Eq, Show)

-- | Admits an explicit SDK-pin runner transition. The candidate digest must
-- name the runner at @runnerPath@, and the binding must be nonempty.
admitNativeRunnerTransition :: FilePath -> T.Text -> T.Text -> T.Text -> IO (Either String NativeRunnerTransition)
admitNativeRunnerTransition runnerPath baselineText candidateText binding =
  case (parseSha256Hex (T.unpack baselineText), parseSha256Hex (T.unpack candidateText)) of
    (Left problem, _) -> pure (Left ("baseline runner digest: " ++ problem))
    (_, Left problem) -> pure (Left ("candidate runner digest: " ++ problem))
    (Right _, Right _)
      | baselineText == candidateText -> pure (Left "runner transition requires distinct baseline and candidate digests")
      | T.null binding -> pure (Left "runner transition binding is empty")
      | otherwise -> do
          present <- doesFileExist runnerPath
          if not present
            then pure (Left ("runner path is not a file: " ++ runnerPath))
            else do
              bytes <- B.readFile runnerPath
              let actual = T.pack (sha256Hex (sha256 bytes))
              pure $ if actual == candidateText
                then Right (NativeRunnerTransition baselineText candidateText runnerPath binding)
                else Left ("runner path digest " ++ T.unpack actual ++ " is not the candidate digest " ++ T.unpack candidateText)

-- | Aligns native cases by id, including baselines recorded with positional pointers.
-- An admitted transition classifies only an exact case-root runner identity change
-- from its baseline digest to its candidate digest. Every other leaf remains
-- strict, and a missing, malformed or unexpected runner identity is refused.
compareNativeObservations :: Maybe NativeRunnerTransition -> Observation -> Observation -> Either String NativeComparison
compareNativeObservations transition baseline observed = do
  expected <- nativeCases baseline
  actual <- nativeCases observed
  (differences, changes) <- go (sortOn fieldName expected) (sortOn fieldName actual)
  pure (NativeComparison differences changes)
  where
    go [] [] = pure ([], [])
    go (Field name value : rest) [] = do
      (differences, changes) <- go rest []
      pure (AbsentFromRun name value : differences, changes)
    go [] (Field name value : rest) = do
      (differences, changes) <- go [] rest
      pure (AbsentFromBaseline name value : differences, changes)
    go (Field name expectedValue : lefts) (Field name' actualValue : rights) = case compare name name' of
      EQ -> do
        (caseDifferences, caseChanges) <- handleCase name expectedValue actualValue
        (differences, changes) <- go lefts rights
        pure (caseDifferences ++ differences, caseChanges ++ changes)
      LT -> do
        (differences, changes) <- go lefts (Field name' actualValue : rights)
        pure (AbsentFromRun name expectedValue : differences, changes)
      GT -> do
        (differences, changes) <- go (Field name expectedValue : lefts) rights
        pure (AbsentFromBaseline name' actualValue : differences, changes)
    handleCase name expectedValue actualValue
      | stripRunner expectedValue /= stripRunner actualValue =
          pure ([Changed name expectedValue actualValue], [])
      | otherwise = case transition of
          Nothing -> pure (if expectedValue == actualValue then ([], []) else ([Changed name expectedValue actualValue], []))
          Just admitted -> case (runnerOf expectedValue, runnerOf actualValue) of
            (Nothing, Nothing) -> pure ([], [])
            (Just (String baselineDigest), Just (String candidateDigest))
              | isDigest baselineDigest && isDigest candidateDigest ->
                  if baselineDigest == transitionBaseline admitted && candidateDigest == transitionCandidate admitted
                    then pure ([], [NativeProvenanceChange (caseName name) name (String baselineDigest) (String candidateDigest)])
                    else if baselineDigest == candidateDigest
                      then pure ([], [])
                      else Left ("native case " ++ T.unpack (caseName name) ++ " has an unexpected runner identity")
              | otherwise -> Left ("native case " ++ T.unpack (caseName name) ++ " has a malformed runner identity")
            _ -> Left ("native case " ++ T.unpack (caseName name) ++ " has a missing runner identity under the admitted transition")

stripRunner :: Value -> Value
stripRunner (Object members) = Object (KM.delete "runner_sha256" members)
stripRunner value = value

runnerOf :: Value -> Maybe Value
runnerOf (Object members) = KM.lookup "runner_sha256" members
runnerOf _ = Nothing

caseName :: FieldName -> T.Text
caseName (FieldName "results.json" (Just [Key "cases", Key name])) = name
caseName name = renderFieldName name

isDigest :: T.Text -> Bool
isDigest text = either (const False) (const True) (parseSha256Hex (T.unpack text))

nativeCases :: Observation -> Either String Observation
nativeCases fields = do
  cases <- traverse oneCase grouped
  let ordered = sortOn fst cases
  unless (all ((== 1) . length) (groupBy (\a b -> fst a == fst b) ordered))
    (Left "duplicate native case id")
  pure (collection ++ other ++ map snd ordered)
  where
    caseFields =
      [ (index, (pointer, value))
      | Field (FieldName "results.json" (Just (Key "cases" : Index index : pointer))) value <- fields ]
    grouped = groupBy (\a b -> fst a == fst b) (sortOn fst caseFields)
    collection =
      [Field (FieldName "results.json" (Just [Key "cases"])) (Array V.empty) | not (null grouped)]
    other = [field | field <- fields, not (isCaseField field)]
    isCaseField (Field (FieldName "results.json" (Just (Key "cases" : Index _ : _))) _) = True
    isCaseField _ = False
    oneCase entries = do
      let leaves = map snd entries
      identity <- case [value | ([Key "id"], value) <- leaves] of
        [String name] | not (T.null name) -> Right (NativeCaseId name)
        _ -> Left "native case requires one nonempty string id"
      value <- assemble leaves
      let NativeCaseId name = identity
      pure (identity, Field (FieldName "results.json" (Just [Key "cases", Key name])) value)
    assemble [([], value)] = Right value
    assemble leaves = do
      unless (all (not . null . fst) leaves) (Left "overlapping native case fields")
      let children = groupBy (\a b -> fst a == fst b)
            (sortOn fst [(segment, (rest, value)) | (segment : rest, value) <- leaves])
      values <- traverse (\entries -> case entries of
        (segment, _) : _ -> (,) segment <$> assemble (map snd entries)
        [] -> Left "empty native case field group") children
      case values of
        (Key _, _) : _ | all isKey values ->
          Right (Object (KM.fromList [(K.fromText key, value) | (Key key, value) <- values]))
        (Index _, _) : _ | map fst values == map Index [0 .. length values - 1] ->
          Right (Array (V.fromList (map snd values)))
        _ -> Left "invalid native case field tree"
    isKey (Key _, _) = True
    isKey _ = False

renderFieldName :: FieldName -> T.Text
renderFieldName (FieldName file Nothing) = T.pack file
renderFieldName (FieldName file (Just pointer)) = T.pack file <> "#" <> T.concat (map (("/" <>) . render) pointer)
  where
    render (Key key) = T.replace "/" "~1" (T.replace "~" "~0" key)
    render (Index index) = T.pack (show index)

-- | The baseline and run values of a difference, null where absent.
differenceValue :: Difference -> (Value, Value)
differenceValue (Changed _ expected actual) = (expected, actual)
differenceValue (AbsentFromRun _ expected) = (expected, Null)
differenceValue (AbsentFromBaseline _ actual) = (Null, actual)

data Baseline = Baseline
  { baselineRoute :: T.Text
  , baselineRecordedAt :: T.Text
  , baselineRecordedFrom :: FilePath
  , baselineFields :: Observation
  }

-- | One field per line.
encodeBaseline :: Baseline -> B.ByteString
encodeBaseline baseline = B.concat
  [ "{\"schema\":1,\"route\":", line (String (baselineRoute baseline))
  , ",\"recorded_at\":", line (String (baselineRecordedAt baseline))
  , ",\"recorded_from\":", line (String (T.pack (baselineRecordedFrom baseline)))
  , ",\"fields\":[\n"
  , C.intercalate ",\n" (map (line . fieldJson) (baselineFields baseline))
  , "\n]}\n" ]
  where
    line = BL.toStrict . encode
    fieldJson (Field (FieldName file pointer) value) = object
      [ "file" .= file
      , "pointer" .= fmap (map segmentJson) pointer
      , "value" .= value ]
    segmentJson (Key key) = String key
    segmentJson (Index index) = Number (fromIntegral index)

decodeBaseline :: B.ByteString -> Either String Baseline
decodeBaseline bytes = eitherDecodeStrict' bytes >>= parseEither parser
  where
    parser = withObject "baseline" $ \document -> do
      schema <- document .: "schema" :: Parser Int
      unless (schema == 1) (fail "baseline schema must be 1")
      Baseline
        <$> document .: "route"
        <*> document .: "recorded_at"
        <*> document .: "recorded_from"
        <*> (document .: "fields" >>= withArray "fields" (mapM field . V.toList))
    field = withObject "field" $ \entry -> do
      file <- entry .: "file"
      pointer <- entry .: "pointer" >>= traverse (withArray "pointer" (mapM segment . V.toList))
      Field (FieldName file pointer) <$> entry .: "value"
    segment (String key) = pure (Key key)
    segment (Number number) = maybe (fail "pointer index must be an integer") (pure . Index) (toBoundedInteger number)
    segment _ = fail "pointer segment must be a string or an integer"
