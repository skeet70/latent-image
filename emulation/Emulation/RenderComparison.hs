{-# LANGUAGE OverloadedStrings #-}
module Emulation.RenderComparison (compareRenderReceipts) where

import Control.Exception (bracket, onException)
import Control.Monad (foldM, unless)
import Data.Aeson (FromJSON (parseJSON), Object, Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither, withObject, withText, (.:))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex)
import FujiAF.JSON (decodeStrict, decodeStrictIntegers, encodeCanonical)
import System.Directory (canonicalizePath)
import System.FilePath (isAbsolute, takeDirectory, (</>))
import System.IO (hClose)
import System.Posix.IO (OpenFileFlags (..), OpenMode (WriteOnly), closeFd, defaultFileFlags, fdToHandle, openFd)

newtype CaptureLabel = CaptureLabel T.Text deriving (Eq, Ord)
newtype ArtifactPath = ArtifactPath FilePath deriving Eq
newtype ByteCount = ByteCount Integer deriving Eq
newtype Instructions = Instructions Integer deriving Eq
newtype ScanoutInstance = ScanoutInstance Integer deriving Eq
newtype NodeName = NodeName T.Text deriving Eq
newtype NativeObject = NativeObject B.ByteString deriving Eq
newtype SourcePlanes = SourcePlanes (B.ByteString, ByteCount) deriving Eq
newtype PixelMetadata = PixelMetadata B.ByteString deriving Eq

data ArtifactReference = ArtifactReference ArtifactPath Sha256 (Maybe ByteCount)
data Artifact = Artifact ArtifactPath Sha256 ByteCount B.ByteString
data Capture = Capture CaptureLabel ArtifactReference ArtifactReference ArtifactReference
data Receipt = Receipt Artifact [Capture]
data Scanout = Scanout NodeName ScanoutInstance Instructions PixelMetadata ArtifactReference ArtifactReference SourcePlanes NativeObject NativeObject
data LoadedCapture = LoadedCapture CaptureLabel Artifact Artifact Artifact Scanout

data PairResult = PairResult CaptureLabel LoadedCapture LoadedCapture [(K.Key, Bool)]

require :: Bool -> String -> Parser ()
require condition message = unless condition (fail message)

nonemptyText :: Value -> Parser T.Text
nonemptyText = withText "nonempty text" $ \text -> do
  require (not (T.null (T.strip text))) "empty text"
  pure text

unsigned :: Integer -> Value -> Parser Integer
unsigned maximumValue value = do
  number <- parseJSON value
  require (number >= 0 && number <= maximumValue) "unsigned integer out of range"
  pure number

u64 :: Value -> Parser Integer
u64 = unsigned 18446744073709551615

count :: Value -> Parser ByteCount
count value = ByteCount <$> u64 value

digest :: Value -> Parser Sha256
digest = withText "sha256" (either fail pure . parseSha256Hex . T.unpack)

optionalCount :: Object -> Parser (Maybe ByteCount)
optionalCount fields = traverse count (KM.lookup "bytes" fields)

reference :: Object -> Parser ArtifactReference
reference fields = ArtifactReference
  <$> (ArtifactPath . T.unpack <$> (fields .: "path" >>= nonemptyText))
  <*> (fields .: "sha256" >>= digest)
  <*> optionalCount fields

artifactReference :: Value -> Parser ArtifactReference
artifactReference = withObject "artifact reference" reference

nativeObject :: Value -> Parser NativeObject
nativeObject value = case value of
  Object fields | not (KM.null fields) -> pure (NativeObject (encodeCanonical value))
  _ -> fail "native state must be a nonempty object"

pixels :: Value -> Parser PixelMetadata
pixels = withObject "pixels" $ \fields -> do
  width <- fields .: "width" >>= unsigned 4294967295
  height <- fields .: "height" >>= unsigned 4294967295
  ByteCount size <- fields .: "rgb_bytes" >>= count
  _ <- fields .: "rgb_sha256" >>= digest
  require (width > 0 && height > 0 && size == width * height * 3) "invalid RGB dimensions or extent"
  pure (PixelMetadata (encodeCanonical (Object fields)))

sourcePlanes :: Value -> Parser SourcePlanes
sourcePlanes value = case value of
  Array entries | not (V.null entries) -> do
    extent <- foldM plane 0 (V.toList entries)
    pure (SourcePlanes (encodeCanonical value, ByteCount extent))
  _ -> fail "source_planes must be a nonempty array"
  where
    plane end = withObject "source plane" $ \fields -> do
      kind <- fields .: "kind" :: Parser T.Text
      case kind of
        "main" -> pure ()
        "graphics" -> do
          _ <- fields .: "layer" >>= u64
          _ <- fields .: "channel" >>= u64
          pure ()
        _ -> fail "unknown source plane kind"
      _ <- fields .: "native_source_address" >>= u64
      offset <- fields .: "offset" >>= u64
      size <- fields .: "bytes" >>= u64
      require (offset == end && size > 0 && offset + size <= 18446744073709551615) "invalid source plane extent"
      pure (offset + size)

parseScanout :: Value -> Parser Scanout
parseScanout = withObject "scanout metadata" $ \fields -> do
  schema <- fields .: "schema" :: Parser Integer
  require (schema == 1) "unsupported scanout schema"
  Scanout
    <$> (NodeName <$> (fields .: "node" >>= nonemptyText))
    <*> (ScanoutInstance <$> (fields .: "instance" >>= u64))
    <*> (Instructions <$> (fields .: "instructions" >>= u64))
    <*> (fields .: "pixels" >>= pixels)
    <*> (fields .: "png" >>= artifactReference)
    <*> (fields .: "source_bytes" >>= artifactReference)
    <*> (fields .: "source_planes" >>= sourcePlanes)
    <*> (fields .: "native_frame_state" >>= nativeObject)
    <*> (fields .: "native_scanout_state" >>= nativeObject)

parseReceipt :: Value -> Parser [Capture]
parseReceipt = withObject "e2e receipt" $ \fields -> do
  schema <- fields .: "schema" :: Parser Integer
  require (schema == 1) "unsupported e2e receipt schema"
  captures <- fields .: "menu_receipts" >>= mapM capture
  require (not (null captures)) "receipt has no captures"
  let labels = [label | Capture label _ _ _ <- captures]
  require (M.size (M.fromList [(label, ()) | label <- labels]) == length labels) "duplicate capture label"
  pure captures
  where
    capture = withObject "capture" $ \fields -> do
      label <- CaptureLabel <$> (fields .: "label" >>= nonemptyText)
      png <- reference fields
      (metadata, source) <- fields .: "scanout_evidence" >>= withObject "scanout evidence" (\evidence ->
        (,) <$> (evidence .: "metadata" >>= artifactReference) <*> (evidence .: "source_bytes" >>= artifactReference))
      pure (Capture label png metadata source)

failComparison :: String -> IO a
failComparison = ioError . userError

parseDocument :: (B.ByteString -> Either String Value) -> (Value -> Parser a) -> Artifact -> IO a
parseDocument decoder parser (Artifact (ArtifactPath path) _ _ bytes) =
  either (failComparison . ((path ++ ": ") ++)) pure (decoder bytes >>= parseEither parser)

resolve :: FilePath -> ArtifactPath -> IO ArtifactPath
resolve base (ArtifactPath path) = ArtifactPath <$> canonicalizePath (if isAbsolute path then path else base </> path)

readArtifact :: ArtifactPath -> IO Artifact
readArtifact path@(ArtifactPath file) = do
  bytes <- B.readFile file
  pure (Artifact path (sha256 bytes) (ByteCount (toInteger (B.length bytes))) bytes)

verifyArtifact :: FilePath -> ArtifactReference -> IO Artifact
verifyArtifact base (ArtifactReference path expectedDigest expectedSize) = do
  actual@(Artifact (ArtifactPath file) actualDigest actualSize _) <- resolve base path >>= readArtifact
  unless (actualDigest == expectedDigest && maybe True (== actualSize) expectedSize)
    (failComparison (file ++ ": artifact binding differs"))
  pure actual

sameArtifact :: Artifact -> Artifact -> Bool
sameArtifact (Artifact path digest' size _) (Artifact otherPath otherDigest otherSize _) =
  path == otherPath && digest' == otherDigest && size == otherSize

loadReceipt :: FilePath -> IO Receipt
loadReceipt path = do
  artifact <- resolve "." (ArtifactPath path) >>= readArtifact
  Receipt artifact <$> parseDocument decodeStrict parseReceipt artifact

loadCapture :: Artifact -> Capture -> IO LoadedCapture
loadCapture (Artifact (ArtifactPath receiptPath) _ _ _) (Capture label pngRef metadataRef sourceRef) = do
  let base = takeDirectory receiptPath
  png <- verifyArtifact base pngRef
  metadata@(Artifact (ArtifactPath metadataPath) _ _ _) <- verifyArtifact base metadataRef
  source@(Artifact _ _ (ByteCount sourceSize) _) <- verifyArtifact base sourceRef
  scanout@(Scanout _ _ _ _ metadataPng metadataSource (SourcePlanes (_, ByteCount extent)) _ _) <- parseDocument decodeStrictIntegers parseScanout metadata
  boundPng <- verifyArtifact (takeDirectory metadataPath) metadataPng
  boundSource <- verifyArtifact (takeDirectory metadataPath) metadataSource
  unless (sameArtifact png boundPng && sameArtifact source boundSource)
    (failComparison (metadataPath ++ ": receipt and scanout artifact references differ"))
  unless (extent == sourceSize) (failComparison (metadataPath ++ ": source plane extent differs from source bytes"))
  pure (LoadedCapture label png source metadata scanout)

pair :: LoadedCapture -> LoadedCapture -> PairResult
pair left@(LoadedCapture label (Artifact _ _ _ leftPng) (Artifact _ _ _ leftSource) _
    (Scanout leftNode leftInstance _ leftPixels _ _ leftPlanes leftFrame leftScanout))
    right@(LoadedCapture _ (Artifact _ _ _ rightPng) (Artifact _ _ _ rightSource) _
    (Scanout rightNode rightInstance _ rightPixels _ _ rightPlanes rightFrame rightScanout)) =
  PairResult label left right
    [ ("png_bytewise_equal", leftPng == rightPng)
    , ("source_planes_bytewise_equal", leftSource == rightSource)
    , ("native_frame_state_equal", leftFrame == rightFrame)
    , ("native_scanout_state_equal", leftScanout == rightScanout)
    , ("source_planes_equal", leftPlanes == rightPlanes)
    , ("pixels_equal", leftPixels == rightPixels)
    , ("node_equal", leftNode == rightNode)
    , ("instance_equal", leftInstance == rightInstance)
    ]

artifactValue :: Artifact -> Value
artifactValue (Artifact (ArtifactPath path) digest' (ByteCount size) _) = object
  ["path" .= path, "sha256" .= sha256Hex digest', "bytes" .= size]

captureArtifacts :: LoadedCapture -> Value
captureArtifacts (LoadedCapture _ png source metadata _) = object
  ["png" .= artifactValue png, "source_bytes" .= artifactValue source, "metadata" .= artifactValue metadata]

pairPassed :: PairResult -> Bool
pairPassed (PairResult _ _ _ checks) = all snd checks

pairValue :: PairResult -> Value
pairValue result@(PairResult (CaptureLabel label) left right checks) = object
  (["capture" .= label, "equal" .= pairPassed result
   ,"instructions" .= object ["reference" .= before, "candidate" .= after, "delta" .= (after - before)]
   ,"artifacts" .= object ["reference" .= captureArtifacts left, "candidate" .= captureArtifacts right]]
   ++ [key .= equal | (key, equal) <- checks])
  where
    clock (LoadedCapture _ _ _ _ (Scanout _ _ (Instructions instructions) _ _ _ _ _ _)) = instructions
    before = clock left
    after = clock right

writeNew :: FilePath -> B.ByteString -> IO ()
writeNew path bytes = bracket acquire hClose (\handle -> B.hPut handle bytes)
  where
    acquire = do
      fd <- openFd path WriteOnly defaultFileFlags {creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True}
      fdToHandle fd `onException` closeFd fd

-- Relative artifact paths are resolved beside the document that references them.
compareRenderReceipts :: FilePath -> FilePath -> FilePath -> IO Bool
compareRenderReceipts referencePath candidatePath outputPath = do
  Receipt referenceArtifact referenceCaptures <- loadReceipt referencePath
  Receipt candidateArtifact candidateCaptures <- loadReceipt candidatePath
  let label (Capture name _ _ _) = name
      candidates = M.fromList [(label capture, capture) | capture <- candidateCaptures]
  unless (M.keysSet candidates == M.keysSet (M.fromList [(label capture, ()) | capture <- referenceCaptures]))
    (failComparison "capture label sets differ")
  pairs <- mapM (\capture -> do
    left <- loadCapture referenceArtifact capture
    right <- loadCapture candidateArtifact (candidates M.! label capture)
    pure (pair left right)) referenceCaptures
  let passed = all pairPassed pairs
      result = object
        ["schema" .= (1 :: Int), "equal" .= passed
        ,"receipts" .= object ["reference" .= artifactValue referenceArtifact, "candidate" .= artifactValue candidateArtifact]
        ,"captures" .= map pairValue pairs]
  writeNew outputPath (encodeCanonical result <> "\n")
  pure passed
