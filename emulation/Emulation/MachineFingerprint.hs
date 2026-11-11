{-# LANGUAGE OverloadedStrings #-}
module Emulation.MachineFingerprint (parseMachineFingerprintEnvelope) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value (..), encode, withObject, (.:))
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Builder as BB
import Data.List (sort, unsnoc)
import qualified Data.Map.Strict as Map
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.ByteString as B
import Data.Word (Word64)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex)
import Text.Read (readMaybe)

newtype ComponentIndex = ComponentIndex Int deriving (Eq, Ord)
data ComponentLeaf = CheckpointLeaf | HeaderLeaf | BlobLeaf ComponentIndex (Maybe ComponentIndex) deriving (Eq, Ord)
data ComponentLocation = ComponentLocation [ComponentIndex] ComponentLeaf deriving (Eq, Ord)
newtype CheckpointKind = CheckpointKind B.ByteString deriving Eq
newtype ComponentBytes = ComponentBytes Word64

data Component = Component
  { location :: ComponentLocation
  , pathText :: T.Text
  , checkpointKind :: CheckpointKind
  , componentBytes :: ComponentBytes
  , componentDigest :: Sha256
  }

parseDigest :: T.Text -> Parser Sha256
parseDigest = either fail pure . parseSha256Hex . T.unpack

parseLocation :: T.Text -> Parser ComponentLocation
parseLocation path = case T.splitOn "/" path of
  "$" : rest -> walk [] rest
  _ -> fail "component path has no root"
 where
  index text = case readMaybe (T.unpack text) of
    Just value | value >= 0 && value < 8192 && T.pack (show value) == text -> pure (ComponentIndex value)
    _ -> fail "component index is not canonical or bounded"
  walk children rest = do
    when (length children > 64) (fail "component path depth exceeded")
    case rest of
      [] -> pure (ComponentLocation (reverse children) CheckpointLeaf)
      ["header"] -> pure (ComponentLocation (reverse children) HeaderLeaf)
      ["blobs", blob] -> ComponentLocation (reverse children) . (`BlobLeaf` Nothing) <$> index blob
      ["blobs", blob, "chunks", chunk] -> do
        blobIndex <- index blob
        chunkIndex <- index chunk
        pure (ComponentLocation (reverse children) (BlobLeaf blobIndex (Just chunkIndex)))
      "children" : child : tailPath -> index child >>= \childIndex -> walk (childIndex : children) tailPath
      _ -> fail "component path geometry differs"

parseComponent :: Value -> Parser Component
parseComponent = withObject "machine component" $ \fields -> do
  path <- fields .: "path"
  at@(ComponentLocation _ leaf) <- parseLocation path
  let expectedKind = case leaf of
        CheckpointLeaf -> "checkpoint"
        HeaderLeaf -> "header"
        BlobLeaf _ Nothing -> "blob"
        BlobLeaf _ (Just _) -> "chunk"
      isChunk = case leaf of BlobLeaf _ (Just _) -> True; _ -> False
      keys = ["path", "kind", "checkpoint_kind", "logical_bytes", "sha256"] ++ ["offset" | isChunk]
  unless (sort (KM.keys fields) == sort keys) (fail "component fields differ")
  kind <- fields .: "kind"
  unless (kind == String expectedKind) (fail "component kind differs from path")
  name <- fields .: "checkpoint_kind"
  let nameBytes = TE.encodeUtf8 name
  unless (not (B.null nameBytes) && B.length nameBytes <= 1024) (fail "checkpoint kind is unbounded")
  bytes <- fields .: "logical_bytes"
  unless (bytes <= 2147483648) (fail "component byte bound exceeded")
  case leaf of
    BlobLeaf _ (Just (ComponentIndex chunk)) -> do
      offset <- fields .: "offset" :: Parser Word64
      unless (offset == fromIntegral chunk * 1048576 && bytes > 0 && bytes <= 1048576)
        (fail "component chunk geometry differs")
    _ -> pure ()
  digest <- fields .: "sha256" >>= parseDigest
  pure (Component at path (CheckpointKind nameBytes) (ComponentBytes bytes) digest)

parseComponents :: Word64 -> Value -> Parser ()
parseComponents wholeBytes value = withObject "machine components" (\fields -> do
  unless (BL.length (encode value) <= 16777216) (fail "component retained-byte bound exceeded")
  unless (sort (KM.keys fields) == sort ["schema", "algorithm", "root_sha256", "chunk_bytes", "entries"])
    (fail "component envelope fields differ")
  schema <- fields .: "schema"
  unless (schema == (1 :: Int)) (fail "component schema differs")
  algorithm <- fields .: "algorithm"
  unless (algorithm == String "sha256-hierarchical-v1") (fail "component algorithm differs")
  chunkBytes <- fields .: "chunk_bytes"
  unless (chunkBytes == (1048576 :: Word64)) (fail "component chunk size differs")
  root <- fields .: "root_sha256" >>= parseDigest
  rawEntries <- fields .: "entries"
  unless (not (null rawEntries) && length rawEntries <= 8192) (fail "component entry bound exceeded")
  entries <- mapM parseComponent rawEntries
  firstEntry <- case entries of
    [] -> fail "component root is absent"
    first : _ -> pure first
  let locations = map location entries
      ComponentBytes rootBytes = componentBytes firstEntry
      metadataCharge entry = 2048 + 8 * toInteger (B.length (TE.encodeUtf8 (pathText entry)))
        + let CheckpointKind name = checkpointKind entry in 8 * toInteger (B.length name)
  unless (4096 + sum (map metadataCharge entries) <= 16777216) (fail "component metadata bound exceeded")
  unless (and (zipWith (<) locations (drop 1 locations))) (fail "component order or identity differs")
  unless (location firstEntry == ComponentLocation [] CheckpointLeaf
      && checkpointKind firstEntry == CheckpointKind "emulator-session-v2"
      && componentDigest firstEntry == root && rootBytes + 33 == wholeBytes)
    (fail "component root binding differs")
  let table = Map.fromList [(location entry, entry) | entry <- entries]
      lookupAt at = maybe (fail "component parent or header is absent") pure (Map.lookup at table)
      byteCount entry = let ComponentBytes bytes = componentBytes entry in bytes
      blobFrames = Map.fromListWith (+)
        [(children, 8 + byteCount entry) | entry <- entries,
          ComponentLocation children (BlobLeaf _ Nothing) <- [location entry]]
      childFrames = Map.fromListWith (+)
        [(parents, byteCount entry) | entry <- entries,
          ComponentLocation children CheckpointLeaf <- [location entry], Just (parents, _) <- [unsnoc children]]
  let headerTotal = sum [toInteger bytes | entry <- entries,
        ComponentLocation _ HeaderLeaf <- [location entry], ComponentBytes bytes <- [componentBytes entry]]
  unless (headerTotal <= 16777216) (fail "component checkpoint-header bound exceeded")
  forM_ entries $ \entry -> do
    let ComponentLocation children leaf = location entry
    node <- lookupAt (ComponentLocation children CheckpointLeaf)
    unless (checkpointKind node == checkpointKind entry) (fail "component checkpoint kind differs")
    case leaf of
      CheckpointLeaf -> do
        header <- lookupAt (ComponentLocation children HeaderLeaf)
        let CheckpointKind name = checkpointKind entry
            expectedBytes = 32 + fromIntegral (B.length name) + byteCount header
              + Map.findWithDefault 0 children blobFrames + Map.findWithDefault 0 children childFrames
        unless (byteCount entry == expectedBytes) (fail "component canonical framing length differs")
        case reverse children of
          [] -> pure ()
          ComponentIndex child : parents -> do
            _ <- lookupAt (ComponentLocation (reverse parents) CheckpointLeaf)
            when (child > 0) $ do
              _ <- lookupAt (ComponentLocation (reverse parents ++ [ComponentIndex (child - 1)]) CheckpointLeaf)
              pure ()
      HeaderLeaf -> pure ()
      BlobLeaf (ComponentIndex blob) chunk -> do
        when (blob > 0) $ do
          _ <- lookupAt (ComponentLocation children (BlobLeaf (ComponentIndex (blob - 1)) Nothing))
          pure ()
        case chunk of
          Nothing -> do
            let count = (byteCount entry + 1048575) `div` 1048576
            when (count > 0) $ forM_ [0 .. count - 1] $ \number -> do
              part <- lookupAt (ComponentLocation children (BlobLeaf (ComponentIndex blob) (Just (ComponentIndex (fromIntegral number)))))
              unless (byteCount part == min 1048576 (byteCount entry - number * 1048576))
                (fail "component blob chunk length differs")
          Just (ComponentIndex number) -> do
            parent <- lookupAt (ComponentLocation children (BlobLeaf (ComponentIndex blob) Nothing))
            unless (fromIntegral number * 1048576 < byteCount parent) (fail "component chunk exceeds blob")
  ) value

decodeArchiveHex :: T.Text -> Parser B.ByteString
decodeArchiveHex text = do
  unless (even (T.length text) && T.all (\c -> c >= '0' && c <= '9' || c >= 'a' && c <= 'f') text)
    (fail "exact archive requires lowercase hexadecimal bytes")
  let digit c = if c <= '9' then fromEnum c - fromEnum '0' else fromEnum c - fromEnum 'a' + 10
      decode [] = []
      decode (a:b:rest) = fromIntegral (digit a * 16 + digit b) : decode rest
      decode _ = []
  pure (B.pack (decode (T.unpack text)))

parseExactArchive :: [Word64] -> Word64 -> Value -> Value -> Parser ()
parseExactArchive counts now components = withObject "exact machine archive" $ \fields -> do
  unless (sort (KM.keys fields) == sort ["schema", "checkpoint_model_id", "counts", "now", "model_definition_sha256", "executable_sha256", "cpu", "exclusive"])
    (fail "exact archive fields differ")
  schema <- fields .: "schema"
  unless (schema == (1 :: Int)) (fail "exact archive schema differs")
  model <- fields .: "checkpoint_model_id" :: Parser T.Text
  unless (not (T.null model) && T.length model <= 1024) (fail "exact archive model binding is absent or unbounded")
  forM_ ["model_definition_sha256", "executable_sha256"] $ \key -> do
    _ <- fields .: key >>= parseDigest
    pure ()
  capturedCounts <- fields .: "counts"
  capturedNow <- fields .: "now"
  unless (capturedCounts == counts && capturedNow == now) (fail "exact archive capture clock differs")
  entries <- withObject "archive component binding" (.: "entries") components :: Parser [Value]
  cpu <- fields .: "cpu" :: Parser [Value]
  unless (length cpu == 2) (fail "exact archive requires both CPUs")
  exclusive <- fields .: "exclusive"
  forM_ (zip [0 :: Int ..] (cpu ++ [exclusive])) $ \(index, blob) ->
    flip (withObject "exact archive blob") blob $ \row -> do
      unless (sort (KM.keys row) == sort ["path", "format", "length", "sha256", "component_sha256", "bytes_hex"])
        (fail "exact archive blob fields differ")
      path <- row .: "path"
      format <- row .: "format"
      let expectedPath = T.pack ("$/children/0/blobs/" ++ show index)
      unless (path == expectedPath && format == String (if index == 2 then "FJMONI01" else "rprofile-r5f-v1"))
        (fail "exact archive blob identity differs")
      size <- row .: "length" :: Parser Word64
      unless (size > 0 && size <= 8388608 && (index /= 2 || size == 80))
        (fail "exact archive blob byte bound differs")
      encoded <- row .: "bytes_hex"
      unless (toInteger (T.length encoded) == toInteger size * 2) (fail "exact archive encoded length differs")
      raw <- decodeArchiveHex encoded
      unless (index /= 2 || B.take 8 raw == "FJMONI01") (fail "exact shared-exclusive archive format differs")
      rawDigest <- row .: "sha256" >>= parseDigest
      unless (sha256 raw == rawDigest) (fail "exact archive raw digest differs")
      let number = BB.word64LE
          field value = number (fromIntegral (B.length value)) <> BB.byteString value
          chunks value | B.null value = []
                       | otherwise = let (part, rest) = B.splitAt 1048576 value in part : chunks rest
          parts = chunks raw
      hashed <- mapM (decodeArchiveHex . T.pack . sha256Hex . sha256) parts
      let framed = field "af-machine-components-v1" <> field "blob" <> number size
            <> number (fromIntegral (length parts))
            <> mconcat [number (fromIntegral (B.length part)) <> BB.byteString digest | (part, digest) <- zip parts hashed]
          componentDigest = sha256 (BL.toStrict (BB.toLazyByteString framed))
      declaredComponent <- row .: "component_sha256" >>= parseDigest
      unless (componentDigest == declaredComponent) (fail "exact archive framed digest differs")
      let matching = [entry | entry@(Object objectFields) <- entries, KM.lookup "path" objectFields == Just (String path)]
      case matching of
        [entry] -> flip (withObject "exact archive component") entry $ \binding -> do
          kind <- binding .: "kind"
          checkpoint <- binding .: "checkpoint_kind"
          logicalBytes <- binding .: "logical_bytes"
          digest <- binding .: "sha256" >>= parseDigest
          unless (kind == String "blob" && checkpoint == String "board-keyframe-v1" && logicalBytes == size && digest == componentDigest)
            (fail "exact archive component binding differs")
        _ -> fail "exact archive component identity is absent or duplicated"

parseMachineFingerprintEnvelope :: Value -> Parser (Word64, Word64)
parseMachineFingerprintEnvelope value = withObject "machine fingerprint envelope" (\fields -> do
  unless (BL.length (encode value) <= 16777216) (fail "machine fingerprint retained metadata exceeds 16 MiB")
  unless (sort (KM.keys fields) == sort ["schema", "status", "machine_sha256", "counts", "now", "logical_bytes", "included_state", "excluded_host_fields", "components", "exact_state"])
    (fail "machine fingerprint fields differ")
  schema <- fields .: "schema"
  unless (schema == (1 :: Int)) (fail "machine fingerprint schema differs")
  status <- fields .: "status"
  unless (status == String "complete") (fail "machine fingerprint is incomplete")
  _ <- fields .: "machine_sha256" >>= parseDigest
  counts <- fields .: "counts"
  pair <- case counts of
    [first, second] -> pure (first, second)
    _ -> fail "machine fingerprint CPU counts are absent"
  now <- fields .: "now" :: Parser Word64
  bytes <- fields .: "logical_bytes"
  unless (bytes <= 2147483648) (fail "machine fingerprint byte bound exceeded")
  included <- fields .: "included_state"
  unless (included == String "session-v2/board-v1-complete-machine") (fail "machine fingerprint coverage differs")
  excluded <- fields .: "excluded_host_fields"
  unless (excluded == (["board.entries", "board.hooks", "board.observations"] :: [T.Text]))
    (fail "machine fingerprint exclusions differ")
  components <- fields .: "components"
  parseComponents bytes components
  fields .: "exact_state" >>= parseExactArchive counts now components
  pure pair) value
