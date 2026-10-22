-- The Classic Chrome Q-menu film tile (reports/classic-chrome-icon.md section
-- 14): an original GVFF tile encoded from host/firmware/classic-chrome-q-tile.txt,
-- placed in the font window's loaded tail at the icon's 'iconTailEnd', and the
-- image-2 word that points Q glyph 0x37c's record at it. The icon's Q labels
-- array names 0xff00037c for Classic Chrome and the stock tiles 0x779 and 0x778
-- for PRO Neg Std and Hi.
module FujiAF.ClassicChromeQTile
  ( -- * Q glyph ids
    QGlyph (..)
  , qGlyphLabel
  , qGlyphRecordAddress
  , ccQGlyph
  , proNegStdTile
  , proNegHiTile
    -- * The tile
  , qTileArt
  , qTileGvff
  , qTileOffset
  , qTileAddress
    -- * Write set
  , qTileFontWrites
  , Image2Write (..)
  , qTileImage2Writes
  , qTileRecordSize
  ) where

import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.Word (Word16, Word32)
import FujiAF.ClassicChromeIcon (FontWrite (..), fontRamAddress, iconGlyphRecord, iconRecordOffset, iconTailEnd, qLabelsArray, qLabelsOffset, qValuesArray, qValuesOffset)
import FujiAF.ClassicChromeQTileArt (qTileArtText)
import FujiAF.FdlFont (FontOffset (..))
import FujiAF.Gvff (TileArt, encodeTileGvff, parseTileArt)

-- A Q glyph: label id 0xff000000 | n, record at image-2 RAM 0x0022a8b4 + 12n
-- for n below 0x8c5 (plain FUN_0117e15c).
newtype QGlyph = QGlyph Word16
  deriving (Eq, Ord, Show)

qGlyphLabel :: QGlyph -> Word32
qGlyphLabel (QGlyph n) = 0xff000000 .|. fromIntegral n

qGlyphRecordAddress :: QGlyph -> Word32
qGlyphRecordAddress (QGlyph n) = 0x0022a8b4 + 12 * fromIntegral n

-- The stock EXR tile, named by no 1.50 code or data.
ccQGlyph :: QGlyph
ccQGlyph = QGlyph 0x37c

proNegStdTile, proNegHiTile :: QGlyph
proNegStdTile = QGlyph 0x779
proNegHiTile = QGlyph 0x778

-- The record's first word, (h << 16) | w, for the 80 x 48 film tiles.
qTileRecordSize :: Word32
qTileRecordSize = 48 `shiftL` 16 .|. 80

qTileArt :: TileArt
qTileArt = either (error . ("classic-chrome-q-tile.txt: " ++)) id (parseTileArt 80 48 qTileArtText)

qTileGvff :: B.ByteString
qTileGvff = encodeTileGvff qTileArt

-- The first 16-aligned font offset after the icon's Q labels array.
qTileOffset :: FontOffset
qTileOffset = iconTailEnd

qTileAddress :: Word32
qTileAddress = fontRamAddress qTileOffset

-- Over a font window that holds the icon's writes: the tile in zero bytes
-- after the icon's Q labels array.
qTileFontWrites :: B.ByteString -> Either String [FontWrite]
qTileFontWrites window = do
  let FontOffset labels = qLabelsOffset
      FontOffset tile = qTileOffset
      slice at size = B.take size (B.drop at window)
  require (slice labels (B.length qLabelsArray) == qLabelsArray) "the window does not hold the icon's Q labels array"
  mapM_
    (\(FontOffset at, bytes, what) -> require (tile >= at + B.length bytes || tile + B.length qTileGvff <= at) ("the tile overlaps " ++ what))
    [(iconRecordOffset, iconGlyphRecord, "the glyph record"), (qValuesOffset, qValuesArray, "the Q values array"), (qLabelsOffset, qLabelsArray, "the Q labels array")]
  require (tile `mod` 4 == 0) "the tile is not word-aligned"
  require (tile + B.length qTileGvff <= B.length window) "the tile runs past the font window"
  require (B.all (== 0) (slice tile (B.length qTileGvff))) "the tile lands on nonzero bytes"
  Right [FontWrite qTileOffset (slice tile (B.length qTileGvff)) qTileGvff "Classic Chrome Q tile (GVFF), the record target of Q glyph 0x37c"]
  where
    require ok message = if ok then Right () else Left message

-- One image-2 word: RAM address, the stock word and the new word.
data Image2Write = Image2Write {i2Address :: Word32, i2Old :: Word32, i2New :: Word32, i2Role :: String}
  deriving (Eq, Show)

qTileImage2Writes :: [Image2Write]
qTileImage2Writes =
  [Image2Write (qGlyphRecordAddress ccQGlyph + 8) 0x0027dbfc qTileAddress "Q glyph 0x37c record pointer: the EXR tile to the Classic Chrome tile"]
