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
  , Segment (..)
  , FieldName (..)
  , Field (..)
  , Observation
  , observe
  , withoutExcluded
  , Difference (..)
  , differenceField
  , compareObservations
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
import Data.List (isSuffixOf, sort, sortOn)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.Sha256 (sha256, sha256Hex)
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
  }

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
  let selected = filter (\path -> any (`matchesFile` path) (observedFiles spec)) files
  fields <- concat <$> forM selected (\path -> do
    bytes <- B.readFile (root </> path)
    if ".json" `isSuffixOf` path
      then case eitherDecodeStrict' bytes of
        Right value -> pure (flatten path [] value)
        Left problem -> pure [Field (FieldName path Nothing) (String (T.pack ("unparseable JSON: " ++ problem)))]
      else pure [Field (FieldName path Nothing) (String (T.pack ("sha256:" ++ sha256Hex (sha256 bytes))))])
  pure (withoutExcluded spec fields)

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
