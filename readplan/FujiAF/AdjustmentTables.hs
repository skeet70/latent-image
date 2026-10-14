-- Develop-stage adjustment-offset tables of the 1.50 decoded images, resolved to offsets inside the
-- adjustment cache. Pure: the caller supplies the image bytes.
module FujiAF.AdjustmentTables
  ( AdjOffset(..)
  , CodeAddress(..)
  , TableName(..)
  , RowZeroIndex(..)
  , DevelopTable(..)
  , developTables
  , Extent(..)
  , DecodedImage(..)
  , ImageSet
  , imageSet
  , imageSetImages
  , decodedImage
  , Sha256
  , sha256Of
  , sha256Text
  , ResolvedEntry(..)
  , TableResolution(..)
  , resolveTable
  , resolveTables
  , adjustmentAreaLimit
  , showAddress
  ) where

import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32, Word64)
import Numeric (showHex)
import FujiAF.SHA256 (sha256)

newtype Sha256 = Sha256 String deriving (Eq, Show)

sha256Of :: B.ByteString -> Sha256
sha256Of = Sha256 . sha256

-- Lowercase hexadecimal, the form common.schema.json#/$defs/sha256 admits.
sha256Text :: Sha256 -> String
sha256Text (Sha256 text) = text

-- Offset inside the adjustment cache that FUN_00dc7664 translates to 0x41acd800.
newtype AdjOffset = AdjOffset Word32 deriving (Eq, Ord, Show)

-- Load address inside a decoded firmware image.
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)

newtype TableName = TableName String deriving (Eq, Ord, Show)

-- FUN_00dc7664 rejects a request whose offset + length - 1 reaches this.
adjustmentAreaLimit :: Word32
adjustmentAreaLimit = 0x420000

showAddress :: Word32 -> String
showAddress value = "0x" ++ showHex value ""

-- What the index of a row-0 entry means. The develop code is the 0-7 value FUN_00d78418 produces;
-- the monochrome filter index is the separate 0-3 value at request field +0x110.
data RowZeroIndex = ByDevelopCode | ByMonochromeFilter | Unindexed deriving (Eq, Show)

data DevelopTable = DevelopTable
  { tableName :: TableName
  , tableAddress :: CodeAddress
  , tableExtentEntries :: Int
  , tableRowZeroEntries :: Int
  , tableRowZeroIndex :: RowZeroIndex
  } deriving (Eq, Show)

-- Extents come from the next function entry in the private filmsim-proc function index; row-0 counts
-- come from the private develop trace. Both are recorded in
-- private-data/analysis/x-m1-1.50/adj-windows/findings.md.
developTables :: [DevelopTable]
developTables =
  [ DevelopTable (TableName "colour-matrix-data")     (CodeAddress 0x00d67f88) 184 184 Unindexed
  , DevelopTable (TableName "colour-matrix-selector") (CodeAddress 0x00d6891c)  16   8 ByDevelopCode
  , DevelopTable (TableName "filter-matrix-selector") (CodeAddress 0x00d680f8)   8   4 ByMonochromeFilter
  , DevelopTable (TableName "chroma-matrix-data")     (CodeAddress 0x00d61e14) 278 278 Unindexed
  , DevelopTable (TableName "chroma-selector")        (CodeAddress 0x00d621a0) 144   8 ByDevelopCode
  , DevelopTable (TableName "hue-range-selector")     (CodeAddress 0x00d62040) 176   8 ByDevelopCode
  , DevelopTable (TableName "d61e04-selector")        (CodeAddress 0x00d61e04)   8   8 ByDevelopCode
  , DevelopTable (TableName "hs-tone-selector")       (CodeAddress 0x00d6a708) 904   8 ByDevelopCode
  , DevelopTable (TableName "d6a5c4-selector")        (CodeAddress 0x00d6a5c4) 164   8 ByDevelopCode
  ]

data Extent = RowZeroOnly | WholeExtent deriving (Eq, Show)

data DecodedImage = DecodedImage
  { imagePath :: FilePath
  , imageLoadAddress :: CodeAddress
  , imageBytes :: B.ByteString
  , imageDigest :: Sha256
  }

newtype ImageSet = ImageSet [DecodedImage]

imageSetImages :: ImageSet -> [DecodedImage]
imageSetImages (ImageSet images) = images

decodedImage :: FilePath -> CodeAddress -> B.ByteString -> DecodedImage
decodedImage path address bytes = DecodedImage path address bytes (sha256Of bytes)

-- Images are held in load order and overlapping ranges are refused, so a resolved address has one
-- source byte.
imageSet :: [DecodedImage] -> Either String ImageSet
imageSet [] = Left "a decoded image set needs at least one image"
imageSet images =
  case [ (earlier, later)
       | (earlier, later) <- zip ordered (drop 1 ordered)
       , imageEnd earlier > imageStart later ] of
    ((earlier, later) : _) ->
      Left ("decoded images overlap: " ++ imagePath earlier ++ " and " ++ imagePath later)
    [] -> Right (ImageSet ordered)
  where
    ordered = sortOn imageStart images
    imageStart image = let CodeAddress base = imageLoadAddress image in fromIntegral base :: Word64
    imageEnd image = imageStart image + fromIntegral (B.length (imageBytes image))

readHalfword :: ImageSet -> CodeAddress -> Either String Word32
readHalfword (ImageSet images) (CodeAddress at) =
  case [ value
       | image <- images
       , let CodeAddress base = imageLoadAddress image
       , at >= base
       , let index = fromIntegral (at - base) :: Int
       , index + 1 < B.length (imageBytes image)
       , let value = fromIntegral (B.index (imageBytes image) index)
               .|. fromIntegral (B.index (imageBytes image) (index + 1)) `shiftL` 8 ] of
    (value : _) -> Right value
    [] -> Left ("table address " ++ showAddress at ++ " is outside every decoded image")

data ResolvedEntry = ResolvedEntry
  { entryIndex :: Int
  , entryOffset :: AdjOffset
  } deriving (Eq, Show)

data TableResolution = TableResolution
  { resolvedTable :: DevelopTable
  , resolvedExtent :: Extent
  , resolvedEntries :: [ResolvedEntry]
  } deriving (Eq, Show)

resolveTable :: ImageSet -> Extent -> DevelopTable -> Either String TableResolution
resolveTable images extent table =
  TableResolution table extent <$> traverse resolve [0 .. limit - 1]
  where
    limit = case extent of
      RowZeroOnly -> tableRowZeroEntries table
      WholeExtent -> tableExtentEntries table
    CodeAddress base = tableAddress table
    resolve index =
      ResolvedEntry index . AdjOffset <$> readHalfword images (CodeAddress (base + 2 * fromIntegral index))

resolveTables :: ImageSet -> Extent -> [DevelopTable] -> Either String [TableResolution]
resolveTables images extent = traverse (resolveTable images extent)
