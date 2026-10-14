{-# LANGUAGE OverloadedStrings #-}
-- The id-0x83d special label record inside the pinned 1.50 image-2 extent: the
-- inline UTF-16 buffer a compiled recipe name takes, the five layout halfwords
-- it carries, and the load-relative geometry the container supplies.
--
-- The plain image's label accessor resolves ids 0x83c..0x83e to 66-byte records
-- from RAM 0x0037b784, the first 56 bytes an inline UTF-16 buffer and five
-- layout halfwords at +0x38 (reports/special-label-resolution.md section 3).
-- Id 0x83d is the only one a name can ride: 0x83c is the blank id every empty
-- label uses and 0x83e returns zero from every entry point a label passes.
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
  ) where

import qualified Data.ByteString as B
import Data.Word (Word16, Word32)
import FujiAF.UpdateContainer (MediaImage (..), putWord16, word16At)

-- The base plus one 0x42-byte record: 0x0037b784 + 0x42 * (0x83d - 0x83c).
labelBufferAddress :: Word32
labelBufferAddress = 0x0037b784 + 0x42 * (0x83d - 0x83c)

labelRecordBytes :: Int
labelRecordBytes = 0x42

-- The inline buffer: 28 UTF-16 code units, so a name of at most 27 plus its
-- terminator.
labelBufferBytes :: Int
labelBufferBytes = 56

labelNameMax :: Int
labelNameMax = 27

-- The layout halfwords the stock record carries; a write keeps them, and a
-- source that does not carry them is refused.
labelLayoutWords :: [Word16]
labelLayoutWords = [0x14, 0x14, 0x1e, 0x24, 0x30]

-- The id-0x83d record's offset inside a media-form id-2 image whose extent the
-- container maps to RAM `load`, when the record lies inside an extent of
-- `extentLength`.
labelRecordOffset :: Word32 -> Int -> Either String Int
labelRecordOffset load extentLength
  | labelBufferAddress < load =
      Left
        ( "the id-0x83d record at "
            ++ show labelBufferAddress
            ++ " lies below the image-2 load "
            ++ show load
        )
  | offset + labelRecordBytes > extentLength =
      Left
        ( "the id-0x83d record does not fit the image-2 extent: offset "
            ++ show offset
            ++ " plus "
            ++ show labelRecordBytes
            ++ " bytes against "
            ++ show extentLength
        )
  | otherwise = Right offset
  where
    offset = fromIntegral (labelBufferAddress - load)

-- The record's five layout halfwords, read from a media-form id-2 image.
labelLayoutAt :: Int -> MediaImage -> [Word16]
labelLayoutAt offset media =
  [word16At (mediaImageBytes media) (offset + labelBufferBytes + 2 * k) | k <- [0 .. 4]]

-- Writes a name into the id-0x83d inline buffer: UTF-16LE code units, a zero
-- terminator, zero fill to the buffer's 56 bytes. Only the buffer changes.
writeLabelName :: Word32 -> MediaImage -> String -> Either String MediaImage
writeLabelName load media name
  | null name = Left "the recipe name is empty"
  | length name > labelNameMax =
      Left
        ( "the recipe name is "
            ++ show (length name)
            ++ " characters; the "
            ++ show labelBufferBytes
            ++ "-byte buffer holds "
            ++ show labelNameMax
            ++ " plus a terminator"
        )
  | any (not . printableAscii) name =
      Left "the recipe name has a character outside printable ASCII (the resident FONT_LATIN cmap covers printable ASCII)"
  | otherwise = do
      offset <- labelRecordOffset load (B.length image)
      if labelLayoutAt offset media /= labelLayoutWords
        then Left ("the id-0x83d record does not carry the pinned layout halfwords " ++ show labelLayoutWords)
        else
          let units = map (fromIntegral . fromEnum) name ++ [0 :: Word16]
              buffer = B.concat (map putWord16 units) <> B.replicate (labelBufferBytes - 2 * length units) 0
           in Right (MediaImage (B.take offset image <> buffer <> B.drop (offset + labelBufferBytes) image))
  where
    image = mediaImageBytes media
    printableAscii c = c >= ' ' && c <= '~'

-- The name the id-0x83d buffer carries in a media-form id-2 image, decoded
-- from its UTF-16LE code units up to the zero terminator.
readLabelName :: Word32 -> MediaImage -> Either String String
readLabelName load media = do
  offset <- labelRecordOffset load (B.length image)
  let units = [word16At image (offset + 2 * k) | k <- [0 .. labelBufferBytes `div` 2 - 1]]
      textUnits = takeWhile (/= 0) units
  if length textUnits == length units
    then Left "the id-0x83d buffer carries no zero terminator"
    else Right (map (toEnum . fromIntegral) textUnits)
  where
    image = mediaImageBytes media
