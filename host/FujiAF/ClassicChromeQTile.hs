-- Full-size Q-menu film tiles for the Classic Chrome icon candidate
-- (reports/classic-chrome-icon.md section 14): Q rows 5 and 6 (PRO Neg Std
-- and Hi) name the stock 80 x 48 tiles 0x779 and 0x778, and row 7 (Classic
-- Chrome) names Q glyph 0x37c, whose record is pointed at an original GVFF
-- tile encoded from host/firmware/classic-chrome-q-tile.txt and placed in the
-- font window's loaded tail after the icon's Q arrays.
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
  , QLabelWrite (..)
  , qLabelWrites
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
import FujiAF.UpdateContainer (putWord32)

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

-- One Q labels row's field 0: the icon's label and the tile's.
data QLabelWrite = QLabelWrite {qlRow :: Int, qlOld :: Word32, qlNew :: QGlyph, qlRole :: String}
  deriving (Eq, Show)

qLabelWrites :: [QLabelWrite]
qLabelWrites =
  [ QLabelWrite 5 0x49a proNegStdTile "Q row 5 (PRO Neg Std): stock tile 0xff000779"
  , QLabelWrite 6 0x496 proNegHiTile "Q row 6 (PRO Neg Hi): stock tile 0xff000778"
  , QLabelWrite 7 0x83d ccQGlyph "Q row 7 (Classic Chrome): Q glyph 0xff00037c"
  ]

-- Over a font window that holds the icon's writes: the three Q label words,
-- each over the icon's Q labels blob, then the tile in zero bytes.
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
  labelWrites <- mapM (labelWrite labels slice) qLabelWrites
  Right (labelWrites ++ [FontWrite qTileOffset (slice tile (B.length qTileGvff)) qTileGvff "Classic Chrome Q tile (GVFF), the record target of Q glyph 0x37c"])
  where
    labelWrite labels slice w = do
      let at = labels + 16 * qlRow w
      require (slice at 4 == putWord32 (qlOld w)) ("Q row " ++ show (qlRow w) ++ " field 0 is not " ++ show (qlOld w))
      Right (FontWrite (FontOffset at) (putWord32 (qlOld w)) (putWord32 (qGlyphLabel (qlNew w))) (qlRole w))
    require ok message = if ok then Right () else Left message

-- One image-2 word: RAM address, the stock word and the new word.
data Image2Write = Image2Write {i2Address :: Word32, i2Old :: Word32, i2New :: Word32, i2Role :: String}
  deriving (Eq, Show)

qTileImage2Writes :: [Image2Write]
qTileImage2Writes =
  [Image2Write (qGlyphRecordAddress ccQGlyph + 8) 0x0027dbfc qTileAddress "Q glyph 0x37c record pointer: the EXR tile to the Classic Chrome tile"]
