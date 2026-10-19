{-# LANGUAGE OverloadedStrings #-}
-- The special label records inside the pinned 1.50 image-2 extent: the inline
-- UTF-16 buffer a name takes, the five layout halfwords each record carries,
-- and the load-relative geometry the container supplies.
--
-- The plain image's label accessor resolves ids 0x83c..0x83e to 66-byte records
-- from RAM 0x0037b784, the first 56 bytes an inline UTF-16 buffer and five
-- layout halfwords at +0x38 (reports/special-label-resolution.md section 3).
-- On the stock plain image 0x83c is the blank id every empty label uses and
-- 0x83e returns zero from every entry point a label passes; the Classic Chrome
-- patch raises the gates that admit 0x83e (reports/classic-chrome-patch.md
-- section 4.7). writeLabelName writes the id-0x83d name and keeps its stock
-- layout; writeSpecialRecord writes a whole record of any of the three ids.
module FujiAF.LabelRecord
  ( labelBufferAddress
  , labelRecordBytes
  , labelBufferBytes
  , labelNameMax
  , labelLayoutWords
  , labelRecordOffset
  , labelLayoutAt
  , writeLabelName
  , readLabelName
    -- * Special records 0x83c..0x83e
  , SpecialLabelId (..)
  , specialLabelId
  , specialLabelIdValue
  , specialRecordAddress
  , specialRecordOffset
  , LabelName
  , labelName
  , labelNameText
  , LabelLayout (..)
  , labelLayout
  , labelLayoutList
  , SpecialRecordBytes
  , specialRecordBytes
  , specialRecordBytesValue
  , specialRecordFromHex
  , specialRecordHex
  , writeSpecialRecord
  , readSpecialRecord
  ) where

import qualified Data.ByteString as B
import Data.Char (intToDigit)
import Data.Word (Word16, Word32)
import FujiAF.UpdateContainer (MediaImage (..), putWord16, word16At)
import Numeric (showHex)

-- The base plus one 0x42-byte record: 0x0037b784 + 0x42 * (0x83d - 0x83c).
labelBufferAddress :: Word32
labelBufferAddress = specialRecordAddress SpecialLabel83d

labelRecordBytes :: Int
labelRecordBytes = 0x42

-- The inline buffer: 28 UTF-16 code units, so a name of at most 27 plus its
-- terminator.
labelBufferBytes :: Int
labelBufferBytes = 56

labelNameMax :: Int
labelNameMax = 27

-- The layout halfwords the stock id-0x83d record carries; writeLabelName keeps
-- them, and a source that does not carry them is refused.
labelLayoutWords :: [Word16]
labelLayoutWords = [0x14, 0x14, 0x1e, 0x24, 0x30]

-- The id-0x83d record's offset inside a media-form id-2 image whose extent the
-- container maps to RAM `load`, when the record lies inside an extent of
-- `extentLength`.
labelRecordOffset :: Word32 -> Int -> Either String Int
labelRecordOffset = specialRecordOffset SpecialLabel83d

-- The id-0x83d record's five layout halfwords, read from a media-form id-2
-- image at the record offset.
labelLayoutAt :: Int -> MediaImage -> [Word16]
labelLayoutAt offset media =
  [word16At (mediaImageBytes media) (offset + labelBufferBytes + 2 * k) | k <- [0 .. 4]]

-- Writes a name into the id-0x83d inline buffer: UTF-16LE code units, a zero
-- terminator, zero fill to the buffer's 56 bytes. Only the buffer changes.
writeLabelName :: Word32 -> MediaImage -> String -> Either String MediaImage
writeLabelName load media text = do
  name <- labelName text
  offset <- labelRecordOffset load (B.length image)
  if labelLayoutAt offset media /= labelLayoutWords
    then Left ("the id-0x83d record does not carry the pinned layout halfwords " ++ show labelLayoutWords)
    else Right (MediaImage (B.take offset image <> nameBuffer name <> B.drop (offset + labelBufferBytes) image))
  where
    image = mediaImageBytes media

-- The name the id-0x83d buffer carries in a media-form id-2 image, decoded
-- from its UTF-16LE code units up to the zero terminator.
readLabelName :: Word32 -> MediaImage -> Either String String
readLabelName load media = do
  offset <- labelRecordOffset load (B.length image)
  bufferText SpecialLabel83d image offset
  where
    image = mediaImageBytes media

data SpecialLabelId = SpecialLabel83c | SpecialLabel83d | SpecialLabel83e
  deriving (Eq, Ord, Show, Enum, Bounded)

specialLabelId :: Integer -> Either String SpecialLabelId
specialLabelId value = case [i | i <- [minBound .. maxBound], specialLabelIdValue i == value] of
  [i] -> Right i
  _ -> Left ("special label id " ++ hexText value ++ " is outside 0x83c..0x83e")

specialLabelIdValue :: SpecialLabelId -> Integer
specialLabelIdValue i = 0x83c + toInteger (fromEnum i)

-- 0x0037b784 + 0x42 * (id - 0x83c).
specialRecordAddress :: SpecialLabelId -> Word32
specialRecordAddress i = 0x0037b784 + 0x42 * fromIntegral (fromEnum i)

-- The record's offset inside a media-form id-2 image whose extent the
-- container maps to RAM `load`, when the record lies inside an extent of
-- `extentLength`.
specialRecordOffset :: SpecialLabelId -> Word32 -> Int -> Either String Int
specialRecordOffset i load extentLength
  | address < load =
      Left
        ( "the "
            ++ idText i
            ++ " record at "
            ++ show address
            ++ " lies below the image-2 load "
            ++ show load
        )
  | offset + labelRecordBytes > extentLength =
      Left
        ( "the "
            ++ idText i
            ++ " record does not fit the image-2 extent: offset "
            ++ show offset
            ++ " plus "
            ++ show labelRecordBytes
            ++ " bytes against "
            ++ show extentLength
        )
  | otherwise = Right offset
  where
    address = specialRecordAddress i
    offset = fromIntegral (address - load)

-- Printable ASCII (the resident FONT_LATIN cmap), 1..27 characters.
newtype LabelName = LabelName String
  deriving (Eq, Show)

labelName :: String -> Either String LabelName
labelName text
  | null text = Left "the recipe name is empty"
  | length text > labelNameMax =
      Left
        ( "the recipe name is "
            ++ show (length text)
            ++ " characters; the "
            ++ show labelBufferBytes
            ++ "-byte buffer holds "
            ++ show labelNameMax
            ++ " plus a terminator"
        )
  | any (not . printableAscii) text =
      Left "the recipe name has a character outside printable ASCII (the resident FONT_LATIN cmap covers printable ASCII)"
  | otherwise = Right (LabelName text)
  where
    printableAscii c = c >= ' ' && c <= '~'

labelNameText :: LabelName -> String
labelNameText (LabelName text) = text

data LabelLayout = LabelLayout Word16 Word16 Word16 Word16 Word16
  deriving (Eq, Show)

labelLayout :: [Integer] -> Either String LabelLayout
labelLayout values
  | any (\v -> v < 0 || v > 0xffff) values = Left ("layout halfwords must be 0..65535: " ++ show values)
  | otherwise = case map fromInteger values of
      [a, b, c, d, e] -> Right (LabelLayout a b c d e)
      _ -> Left ("a record carries five layout halfwords, not " ++ show (length values))

labelLayoutList :: LabelLayout -> [Word16]
labelLayoutList (LabelLayout a b c d e) = [a, b, c, d, e]

-- One whole 66-byte record in media form.
newtype SpecialRecordBytes = SpecialRecordBytes B.ByteString
  deriving (Eq, Show)

specialRecordBytes :: B.ByteString -> Either String SpecialRecordBytes
specialRecordBytes bytes
  | B.length bytes == labelRecordBytes = Right (SpecialRecordBytes bytes)
  | otherwise = Left ("a special record is " ++ show labelRecordBytes ++ " bytes, not " ++ show (B.length bytes))

specialRecordBytesValue :: SpecialRecordBytes -> B.ByteString
specialRecordBytesValue (SpecialRecordBytes bytes) = bytes

-- 132 lowercase hex digits.
specialRecordFromHex :: String -> Either String SpecialRecordBytes
specialRecordFromHex text
  | length text /= 2 * labelRecordBytes =
      Left ("the record's current bytes need " ++ show (2 * labelRecordBytes) ++ " hex digits, not " ++ show (length text))
  | otherwise = SpecialRecordBytes . B.pack <$> pairs text
  where
    pairs (h : l : rest) = (:) <$> ((\a b -> fromIntegral (16 * a + b)) <$> digit h <*> digit l) <*> pairs rest
    pairs _ = Right []
    digit c
      | '0' <= c && c <= '9' = Right (fromEnum c - fromEnum '0')
      | 'a' <= c && c <= 'f' = Right (fromEnum c - fromEnum 'a' + 10)
      | otherwise = Left ("the record's current bytes must be lowercase hex: " ++ show c)

specialRecordHex :: SpecialRecordBytes -> String
specialRecordHex (SpecialRecordBytes bytes) =
  concatMap (\w -> [intToDigit (fromIntegral w `div` 16), intToDigit (fromIntegral w `mod` 16)]) (B.unpack bytes)

-- Replaces the whole record: UTF-16LE name, a zero terminator, zero fill to 56
-- bytes, then the layout halfwords. Refused unless the record currently holds
-- `expected`; no byte outside the record changes.
writeSpecialRecord :: Word32 -> MediaImage -> SpecialLabelId -> SpecialRecordBytes -> LabelName -> LabelLayout -> Either String MediaImage
writeSpecialRecord load media i (SpecialRecordBytes expected) name layout = do
  offset <- specialRecordOffset i load (B.length image)
  let current = B.take labelRecordBytes (B.drop offset image)
      record = nameBuffer name <> B.concat (map putWord16 (labelLayoutList layout))
  if current /= expected
    then Left ("the " ++ idText i ++ " record's current bytes " ++ specialRecordHex (SpecialRecordBytes current) ++ " differ from the expected " ++ specialRecordHex (SpecialRecordBytes expected))
    else Right (MediaImage (B.take offset image <> record <> B.drop (offset + labelRecordBytes) image))
  where
    image = mediaImageBytes media

-- The name (UTF-16LE code units up to the zero terminator) and the layout
-- halfwords a record carries in a media-form id-2 image.
readSpecialRecord :: Word32 -> MediaImage -> SpecialLabelId -> Either String (String, LabelLayout)
readSpecialRecord load media i = do
  offset <- specialRecordOffset i load (B.length image)
  text <- bufferText i image offset
  let half k = word16At image (offset + labelBufferBytes + 2 * k)
  Right (text, LabelLayout (half 0) (half 1) (half 2) (half 3) (half 4))
  where
    image = mediaImageBytes media

nameBuffer :: LabelName -> B.ByteString
nameBuffer (LabelName text) =
  B.concat (map putWord16 units) <> B.replicate (labelBufferBytes - 2 * length units) 0
  where
    units = map (fromIntegral . fromEnum) text ++ [0 :: Word16]

bufferText :: SpecialLabelId -> B.ByteString -> Int -> Either String String
bufferText i image offset =
  if length textUnits == length units
    then Left ("the " ++ idText i ++ " buffer carries no zero terminator")
    else Right (map (toEnum . fromIntegral) textUnits)
  where
    units = [word16At image (offset + 2 * k) | k <- [0 .. labelBufferBytes `div` 2 - 1]]
    textUnits = takeWhile (/= 0) units

idText :: SpecialLabelId -> String
idText i = "id-" ++ hexText (specialLabelIdValue i)

hexText :: Integer -> String
hexText v
  | v < 0 = show v
  | otherwise = "0x" ++ showHex v ""
