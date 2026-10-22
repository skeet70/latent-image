-- FDL font files as the X-M1 1.50 loads them (FONT_ICON.FDL, segment 3,
-- loaded whole to RAM 0x003bc000; reports/classic-chrome-icon.md). Offsets are
-- font file offsets, equal to offsets in the loaded window.
--
-- Header: +0x1c u16 glyph count, +0x2e u32 directory, +0x32 u32 commands,
-- +0x36 u16 format 4, +0x38 u16 directory format 0x100, +0x3a u32 properties.
-- The directory holds glyph count + 1 little-endian words, flags << 24 |
-- begin; a glyph's end is the low 24 bits of the next word. Properties: a u16
-- count, then entries of u16 key, u32 length (counting the 6-byte head) and a
-- payload. Key 10 carries u16 first and last code points, two u16 fields, then
-- one u16 per-mille size per code from first to last.
module FujiAF.FdlFont
  ( CodePoint (..)
  , FontOffset (..)
  , FdlFont
  , fdlBytes
  , fdlGlyphCount
  , fdlDirectoryOffset
  , fdlCommandsOffset
  , parseFdlFont
  , directoryWordOffset
  , directoryWord
  , GlyphFlags (..)
  , Glyph (..)
  , lookupGlyph
  , perMilleOffset
  , perMilleAt
  ) where

import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word16, Word32, Word8)
import Numeric (showHex)

newtype CodePoint = CodePoint Word16
  deriving (Eq, Ord, Show)

newtype FontOffset = FontOffset Int
  deriving (Eq, Ord, Show)

data FdlFont = FdlFont
  { fdlBytes :: B.ByteString
  , fdlGlyphCount :: Int
  , fdlDirectoryOffset :: FontOffset
  , fdlCommandsOffset :: FontOffset
  , fdlPerMille :: Maybe (CodePoint, CodePoint, FontOffset)
  }

-- The directory word's high byte.
newtype GlyphFlags = GlyphFlags Word8
  deriving (Eq, Show)

-- One present glyph as the loaders FUN_01197f98 (id4) and FUN_0119f74c
-- (plain) read it: the record at commands + begin, the optional advance pair
-- (flags bit 0), the optional 4 extra bytes (flags bit 1) and the program.
data Glyph = Glyph
  { glyphRecord :: FontOffset
  , glyphFlags :: GlyphFlags
  , glyphAdvance :: Maybe (Word16, Word16)
  , glyphExtra :: Maybe B.ByteString
  , glyphProgram :: B.ByteString
  }
  deriving (Eq, Show)

u16 :: B.ByteString -> Int -> Word16
u16 bytes i = fromIntegral (B.index bytes i) .|. (fromIntegral (B.index bytes (i + 1)) `shiftL` 8)

u32 :: B.ByteString -> Int -> Word32
u32 bytes i = fromIntegral (u16 bytes i) .|. (fromIntegral (u16 bytes (i + 2)) `shiftL` 16)

hexText :: Int -> String
hexText n = "0x" ++ showHex n ""

-- `bytes` is the loaded window: the font file and whatever follows it.
parseFdlFont :: B.ByteString -> Either String FdlFont
parseFdlFont bytes = do
  require (B.length bytes >= 0x3e) "the font is shorter than its header"
  require (B.take 8 bytes == B.pack (map (fromIntegral . fromEnum) "FDL Font")) "the font does not start with \"FDL Font\""
  let count = fromIntegral (u16 bytes 0x1c)
      directory = fromIntegral (u32 bytes 0x2e)
      commands = fromIntegral (u32 bytes 0x32)
      properties = fromIntegral (u32 bytes 0x3a)
  require (u16 bytes 0x36 == 4) ("font format " ++ show (u16 bytes 0x36) ++ " is not 4")
  require (u16 bytes 0x38 == 0x100) ("directory format " ++ hexText (fromIntegral (u16 bytes 0x38)) ++ " is not 0x100")
  require (directory + 4 * (count + 1) <= commands) "the directory overlaps the commands"
  require (commands <= B.length bytes) "the commands start past the window"
  perMille <- parsePerMille bytes properties
  Right (FdlFont bytes count (FontOffset directory) (FontOffset commands) perMille)
  where
    require ok message = if ok then Right () else Left message

parsePerMille :: B.ByteString -> Int -> Either String (Maybe (CodePoint, CodePoint, FontOffset))
parsePerMille bytes properties
  | properties == 0 = Right Nothing
  | properties + 2 > B.length bytes = Left "the property table starts past the window"
  | otherwise = go (fromIntegral (u16 bytes properties)) (properties + 2)
  where
    go :: Int -> Int -> Either String (Maybe (CodePoint, CodePoint, FontOffset))
    go 0 _ = Right Nothing
    go remaining at
      | at + 6 > B.length bytes = Left ("property entry at " ++ hexText at ++ " runs past the window")
      | otherwise =
          let key = u16 bytes at
              size = fromIntegral (u32 bytes (at + 2))
           in if size < 6 || at + size > B.length bytes
                then Left ("property entry at " ++ hexText at ++ " has length " ++ hexText size)
                else
                  if key /= 10
                    then go (remaining - 1) (at + size)
                    else do
                      let first = u16 bytes (at + 6)
                          final = u16 bytes (at + 8)
                          values = at + 14
                      if final < first || values + 2 * (fromIntegral final - fromIntegral first + 1) > at + size
                        then Left ("property 10 at " ++ hexText at ++ " does not hold its value array")
                        else Right (Just (CodePoint first, CodePoint final, FontOffset values))

directoryWordOffset :: FdlFont -> CodePoint -> Either String FontOffset
directoryWordOffset font (CodePoint code)
  | fromIntegral code >= fdlGlyphCount font = Left ("code point " ++ hexText (fromIntegral code) ++ " is outside the glyph count")
  | otherwise = Right (FontOffset (directory + 4 * fromIntegral code))
  where
    FontOffset directory = fdlDirectoryOffset font

directoryWord :: FdlFont -> CodePoint -> Either String Word32
directoryWord font code = do
  FontOffset at <- directoryWordOffset font code
  Right (u32 (fdlBytes font) at)

-- Nothing when the loader treats the code as absent (end - begin = 0). The
-- span's sign is not checked when flags are nonzero; the flagged length
-- replaces it. No bound applies but the window.
lookupGlyph :: FdlFont -> CodePoint -> Either String (Maybe Glyph)
lookupGlyph font code@(CodePoint c) = do
  word <- directoryWord font code
  FontOffset at <- directoryWordOffset font code
  let next = u32 bytes (at + 4)
      begin = fromIntegral (word .&. 0xffffff) :: Int
      end = fromIntegral (next .&. 0xffffff) :: Int
      flags = fromIntegral (word `shiftR` 24) :: Word8
      span' = end - begin
      record = commands + begin
      advanceAt = record
      extraAt = advanceAt + (if flags .&. 1 /= 0 then 4 else 0)
      lengthAt = extraAt + (if flags .&. 2 /= 0 then 4 else 0)
      programAt = if flags /= 0 then lengthAt + 2 else record
  if span' == 0
    then Right Nothing
    else do
      inWindow record (programAt - record)
      let count = if flags /= 0 then fromIntegral (u16 bytes lengthAt) else span'
      if count < 0
        then Left ("U+" ++ showHex c "" ++ ": negative span " ++ show count ++ " with flags 0")
        else do
          inWindow programAt count
          Right
            ( Just
                Glyph
                  { glyphRecord = FontOffset record
                  , glyphFlags = GlyphFlags flags
                  , glyphAdvance = if flags .&. 1 /= 0 then Just (u16 bytes advanceAt, u16 bytes (advanceAt + 2)) else Nothing
                  , glyphExtra = if flags .&. 2 /= 0 then Just (B.take 4 (B.drop extraAt bytes)) else Nothing
                  , glyphProgram = B.take count (B.drop programAt bytes)
                  }
            )
  where
    bytes = fdlBytes font
    FontOffset commands = fdlCommandsOffset font
    inWindow start size =
      if start < 0 || start + size > B.length bytes
        then Left ("U+" ++ showHex c "" ++ ": record bytes " ++ hexText start ++ "+" ++ show size ++ " run outside the window")
        else Right ()

perMilleOffset :: FdlFont -> CodePoint -> Either String FontOffset
perMilleOffset font (CodePoint code) = case fdlPerMille font of
  Nothing -> Left "the font carries no property 10"
  Just (CodePoint first, CodePoint final, FontOffset values)
    | code < first || code > final -> Left ("U+" ++ showHex code "" ++ " is outside property 10")
    | otherwise -> Right (FontOffset (values + 2 * fromIntegral (code - first)))

perMilleAt :: FdlFont -> CodePoint -> Either String Word16
perMilleAt font code = do
  FontOffset at <- perMilleOffset font code
  Right (u16 (fdlBytes font) at)
