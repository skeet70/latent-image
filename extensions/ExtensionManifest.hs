-- The extension manifest: its fields, the parts it pins and the container
-- order its steps must follow. Parsing reads the manifest and its parts only;
-- no firmware is read.
module ExtensionManifest
  ( Sha256 (..)
  , LoadAddress (..)
  , FlashOffset (..)
  , SpanLength (..)
  , ImageRecord (..)
  , Step (..)
  , Extension (..)
  , parseExtension
  , plainDecodeBase
  , fontDecodeBase
  ) where

import Control.Monad (unless)
import Data.Aeson (Value)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import qualified FujiAF.Host as H
import qualified FujiAF.LabelRecord as L
import Numeric (readHex)
import System.FilePath (isAbsolute, takeDirectory, (</>))

newtype Sha256 = Sha256 String deriving (Eq, Show)
newtype LoadAddress = LoadAddress Integer deriving (Eq, Show)
newtype FlashOffset = FlashOffset Integer deriving (Eq, Show)
newtype SpanLength = SpanLength Integer deriving (Eq, Show)

data ImageRecord = Image4 | Image5 deriving (Eq, Show)

-- Steps run in container order; each consumes the previous step's DAT.
data Step
  = PatchImage ImageRecord (Maybe LoadAddress) FilePath
  | WriteLabel String
  | WriteRecord L.SpecialLabelId L.LabelName L.LabelLayout L.SpecialRecordBytes
  | ReplacePlain FlashOffset SpanLength FilePath
  -- The Sha256 is the patch set's decode_sha256, the plain window it applies to.
  | PatchPlain Sha256 FilePath
  -- The Sha256 is the patch set's decode_sha256, the font window it applies to.
  | PatchFont Sha256 FilePath
  deriving (Eq, Show)

data Extension = Extension
  { extensionId :: String
  , extensionTitle :: String
  , extensionVersion :: String
  , extensionBase :: Sha256
  , extensionSteps :: [Step]
  , extensionOutput :: Sha256
  }
  deriving (Eq, Show)

-- The RAM address of the first byte of the window rebuild-update extract-plain
-- writes.
plainDecodeBase :: Integer
plainDecodeBase = 0x01100000

-- The RAM address of the first byte of the window rebuild-update extract-font
-- writes (segment 3's load address).
fontDecodeBase :: Integer
fontDecodeBase = 0x003bc000

-- Part paths are resolved against the manifest's directory.
parseExtension :: FilePath -> IO Extension
parseExtension path = do
  value <- H.readRegular (1024 * 1024) path >>= H.jsonRead
  H.fields value "schema_version id title base steps output_sha256" "extension"
  _ <- H.integer (H.get value "schema_version") 1 1 "extension schema_version"
  let base = H.get value "base"
  H.fields base "model firmware_version sha256" "extension base"
  H.ensure (H.str (H.get base "model") == "X-M1") "extension base model must be X-M1"
  baseDigest <- digestField (H.get base "sha256") "extension base sha256"
  outputDigest <- digestField (H.get value "output_sha256") "extension output_sha256"
  steps <- mapM (parseStep (takeDirectory path) baseDigest) (H.list (H.get value "steps"))
  H.ensure (not (null steps)) "extension has no steps"
  H.ensure
    (not (any isPatchPlain steps && any isReplacePlain steps))
    "a plain-patch-set step cannot be combined with plain-span steps"
  H.ensure
    (containerOrder steps)
    "extension steps must run image 4, image 5, the label, then image-2 records by increasing id distinct from the label's 0x83d, then at most one font patch set, then plain spans in increasing non-overlapping flash order or one plain patch set; each image and the label at most once"
  pure
    Extension
      { extensionId = H.str (H.get value "id")
      , extensionTitle = H.str (H.get value "title")
      , extensionVersion = H.str (H.get base "firmware_version")
      , extensionBase = baseDigest
      , extensionSteps = steps
      , extensionOutput = outputDigest
      }
  where
    isPatchPlain step = case step of PatchPlain {} -> True; _ -> False
    isReplacePlain step = case step of ReplacePlain {} -> True; _ -> False

parseStep :: FilePath -> Sha256 -> Value -> IO Step
parseStep directory base value = case H.str (H.get value "kind") of
  "image-patch-set" -> do
    H.fields value "kind path sha256" "image-patch-set step"
    (path, bytes) <- pinnedFile directory value
    image <- patchSetImage base bytes
    segment <- H.integer (H.get image "segment_id") 4 5 "patch set image segment_id (images 4 and 5 are supported)"
    decodeBase <- hexValue (H.get image "decode_base") "patch set decode_base"
    outputBase <- patchSetOutputBase image decodeBase
    case segment of
      4 -> pure (PatchImage Image4 (if outputBase == decodeBase then Nothing else Just (LoadAddress outputBase)) path)
      _ -> do
        H.ensure (outputBase == decodeBase) "image-5 patch sets cannot move the image's load address"
        pure (PatchImage Image5 Nothing path)
  "image2-label" -> do
    H.fields value "kind text" "image2-label step"
    let text = H.str (H.get value "text")
    H.ensure (not (null text)) "image2-label text must be nonempty"
    pure (WriteLabel text)
  "image2-record" -> do
    let textKey = if "text_utf16" `elem` H.keys value then "text_utf16" else "text"
    H.fields value ("kind id " ++ textKey ++ " layout current_hex") "image2-record step"
    idValue <- hexValue (H.get value "id") "image2-record id"
    ident <- either H.failH pure (L.specialLabelId idValue)
    let text = H.str (H.get value textKey)
    name <- either H.failH pure (if textKey == "text" then L.labelName text else L.labelNameFromUnitHex text)
    halfwords <- mapM (\v -> H.integer v 0 0xffff "image2-record layout halfword") (H.list (H.get value "layout"))
    layout <- either H.failH pure (L.labelLayout halfwords)
    current <- either H.failH pure (L.specialRecordFromHex (H.str (H.get value "current_hex")))
    pure (WriteRecord ident name layout current)
  "plain-span" -> do
    H.fields value "kind flash_offset path sha256" "plain-span step"
    offset <- hexValue (H.get value "flash_offset") "plain-span flash_offset"
    (path, bytes) <- pinnedFile directory value
    H.ensure (not (B.null bytes)) "plain-span file is empty"
    pure (ReplacePlain (FlashOffset offset) (SpanLength (toInteger (B.length bytes))) path)
  "plain-patch-set" -> do
    H.fields value "kind path sha256" "plain-patch-set step"
    (path, bytes) <- pinnedFile directory value
    image <- patchSetImage base bytes
    _ <- H.integer (H.get image "segment_id") 6 6 "plain patch set image segment_id (the plain image is segment 6)"
    decodeBase <- hexValue (H.get image "decode_base") "plain patch set decode_base"
    H.ensure (decodeBase == plainDecodeBase) "plain patch set decode_base must be 0x01100000, the first byte of the extract-plain window"
    outputBase <- patchSetOutputBase image decodeBase
    H.ensure (outputBase == decodeBase) "plain patch sets cannot move the image's load address"
    decodeDigest <- digestField (H.get image "decode_sha256") "plain patch set decode_sha256"
    pure (PatchPlain decodeDigest path)
  "font-patch-set" -> do
    H.fields value "kind path sha256" "font-patch-set step"
    (path, bytes) <- pinnedFile directory value
    image <- patchSetImage base bytes
    _ <- H.integer (H.get image "segment_id") 3 3 "font patch set image segment_id (the icon font is segment 3)"
    decodeBase <- hexValue (H.get image "decode_base") "font patch set decode_base"
    H.ensure (decodeBase == fontDecodeBase) "font patch set decode_base must be 0x003bc000, the first byte of the extract-font window"
    outputBase <- patchSetOutputBase image decodeBase
    H.ensure (outputBase == decodeBase) "font patch sets cannot move the image's load address"
    decodeDigest <- digestField (H.get image "decode_sha256") "font patch set decode_sha256"
    pure (PatchFont decodeDigest path)
  other -> H.failH ("Unsupported extension step kind " ++ show other ++ "; this tool cannot build the extension")

-- The image block of an apply-patch-set manifest that targets `base`.
patchSetImage :: Sha256 -> B.ByteString -> IO Value
patchSetImage base bytes = do
  patchSet <- H.jsonRead bytes
  H.ensure (Sha256 (H.str (H.get (H.get patchSet "target") "firmware_sha256")) == base) "patch set targets a different base firmware"
  pure (H.get patchSet "image")

patchSetOutputBase :: Value -> Integer -> IO Integer
patchSetOutputBase image decodeBase =
  if "output_base" `elem` H.keys image
    then hexValue (H.get image "output_base") "patch set output_base"
    else pure decodeBase

containerOrder :: [Step] -> Bool
containerOrder steps = and (zipWith before steps (drop 1 steps))
  where
    rank (PatchImage Image4 _ _) = 1 :: Int
    rank (PatchImage Image5 _ _) = 2
    rank (WriteLabel _) = 3
    rank (WriteRecord {}) = 3
    rank (PatchFont {}) = 4
    rank (ReplacePlain {}) = 5
    rank (PatchPlain {}) = 5
    before (ReplacePlain (FlashOffset a) (SpanLength n) _) (ReplacePlain (FlashOffset b) _ _) = a + n <= b
    before (WriteLabel _) (WriteRecord i _ _ _) = i /= L.SpecialLabel83d
    before (WriteRecord a _ _ _) (WriteRecord b _ _ _) = a < b
    before x y = rank x < rank y

pinnedFile :: FilePath -> Value -> IO (FilePath, B.ByteString)
pinnedFile directory value = do
  let relative = H.str (H.get value "path")
  H.ensure (not (null relative) && not (isAbsolute relative)) "extension part path must be relative to the extension manifest"
  expected <- digestField (H.get value "sha256") "extension part sha256"
  let path = directory </> relative
  bytes <- H.readRegular (16 * 1024 * 1024) path
  digest <- H.sha256 bytes
  H.ensure (Sha256 digest == expected) ("extension part " ++ relative ++ " differs from its pinned SHA-256")
  pure (path, bytes)

digestField :: Value -> String -> IO Sha256
digestField value label = do
  let text = H.str value
  unless (length text == 64 && all (\c -> isHexDigit c && c `notElem` ['A' .. 'F']) text) $
    H.failH (label ++ ": lowercase 64-digit SHA-256 required")
  pure (Sha256 text)

hexValue :: Value -> String -> IO Integer
hexValue value label = case H.str value of
  '0' : 'x' : digits | not (null digits), [(n, "")] <- readHex digits -> pure n
  _ -> H.failH (label ++ ": 0x-prefixed hexadecimal required")
