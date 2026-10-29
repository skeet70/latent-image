{-# LANGUAGE OverloadedStrings #-}
-- Bounded first-record replacement in the opaque pre-table container part.
module FujiAF.FirstStage
  ( ArmEntry
  , SlotOffset
  , RetainedExtent
  , FirstStagePlan
  , firstRecordSlotOffset
  , firstRecordSlotLength
  , resetVectorWordOffset
  , Sha256
  , parseSha256
  , sha256Hex
  , armEntry
  , armEntryAddress
  , slotOffset
  , slotOffsetBytes
  , retainedExtent
  , retainedExtentRange
  , firstStagePlan
  , planEntry
  , planLoadOffset
  , planImage
  , planRetainedExtents
  , firstRecordSlotBytes
  , planPatches
  , replaceFirstRecordSlot
  ) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import Data.Bits (complement, (.&.))
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.UpdateContainer (MediaImage (..), Parts (..), putMediaWord)

newtype ArmEntry = ArmEntry Word32
  deriving (Eq, Show)

newtype SlotOffset = SlotOffset Int
  deriving (Eq, Show)

data RetainedExtent = RetainedExtent SlotOffset SlotOffset
  deriving (Eq, Show)

data FirstStagePlan = FirstStagePlan
  { planEntry :: ArmEntry
  , planLoadOffset :: SlotOffset
  , planImage :: MediaImage
  , planRetainedExtents :: [RetainedExtent]
  }
  deriving (Eq, Show)

-- Media slot [0x40000,0x50000) inside the container's main pre-table.
firstRecordSlotOffset :: Int
firstRecordSlotOffset = 0x40000

firstRecordSlotLength :: Int
firstRecordSlotLength = 0x10000

resetVectorWordOffset :: Int
resetVectorWordOffset = 0x20

newtype Sha256 = Sha256 String
  deriving (Eq, Show)

parseSha256 :: String -> Either String Sha256
parseSha256 text
  | length text == 64 && all (`elem` ("0123456789abcdef" :: String)) text = Right (Sha256 text)
  | otherwise = Left "SHA-256 must be 64 lowercase hexadecimal characters"

sha256Hex :: Sha256 -> String
sha256Hex (Sha256 text) = text

armEntry :: Word32 -> Either String ArmEntry
armEntry value
  | value .&. 3 /= 0 = Left "first-stage entry is not word aligned"
  | otherwise = Right (ArmEntry value)

armEntryAddress :: ArmEntry -> Word32
armEntryAddress (ArmEntry value) = value

slotOffset :: Int -> Either String SlotOffset
slotOffset value
  | value < 0 = Left "slot offset is negative"
  | value > firstRecordSlotLength = Left "slot offset is past the first-record slot"
  | otherwise = Right (SlotOffset value)

slotOffsetBytes :: SlotOffset -> Int
slotOffsetBytes (SlotOffset value) = value

retainedExtent :: SlotOffset -> SlotOffset -> Either String RetainedExtent
retainedExtent start end
  | slotOffsetBytes start >= slotOffsetBytes end = Left "retained extent is empty or reversed"
  | otherwise = Right (RetainedExtent start end)

retainedExtentRange :: RetainedExtent -> (Int, Int)
retainedExtentRange (RetainedExtent start end) = (slotOffsetBytes start, slotOffsetBytes end)

firstStagePlan :: ArmEntry -> SlotOffset -> MediaImage -> [RetainedExtent] -> Either String FirstStagePlan
firstStagePlan entry load image retained = do
  let plan = FirstStagePlan entry load image retained
  _ <- planPatches plan
  pure plan

firstRecordSlotBytes :: Parts -> Either String B.ByteString
firstRecordSlotBytes parts
  | B.length (partPreTable parts) < firstRecordSlotOffset + firstRecordSlotLength =
      Left "carrier pre-table does not cover the first-record slot"
  | otherwise =
      Right (B.take firstRecordSlotLength (B.drop firstRecordSlotOffset (partPreTable parts)))

planPatches :: FirstStagePlan -> Either String [(Int, B.ByteString)]
planPatches plan
  | B.null image = Left "first-stage image is empty"
  | load < 0 = Left "first-stage load offset is negative"
  | load .&. 3 /= 0 = Left "first-stage load offset is not word aligned"
  | load + B.length image > firstRecordSlotLength =
      Left "first-stage image extends past the first-record slot"
  | load < resetVectorWordOffset + 4 =
      Left "first-stage image overlaps the reset vector word"
  | entry < load || entry >= load + B.length image =
      Left "first-stage entry is outside the candidate image"
  | otherwise = do
      forM_ retained $ \(start, end) ->
        if start < 0 || end > firstRecordSlotLength || start >= end
          then Left "retained extent is outside the first-record slot"
          else pure ()
      if any (overlaps (load, load + B.length image)) retained
        then Left "first-stage image overlaps a retained extent"
        else pure ()
      pure
        [ (resetVectorWordOffset, putMediaWord (armEntryAddress (planEntry plan)))
        , (load, B.map complement image)
        ]
  where
    image = mediaImageBytes (planImage plan)
    load = slotOffsetBytes (planLoadOffset plan)
    entry = fromIntegral (armEntryAddress (planEntry plan)) :: Int
    retained = map retainedExtentRange (planRetainedExtents plan)

overlaps :: (Int, Int) -> (Int, Int) -> Bool
overlaps (leftStart, leftEnd) (rightStart, rightEnd) =
  leftStart < rightEnd && rightStart < leftEnd

replaceFirstRecordSlot :: FirstStagePlan -> Parts -> Either String Parts
replaceFirstRecordSlot plan parts = do
  slot <- firstRecordSlotBytes parts
  patches <- planPatches plan
  patched <- applyPatches slot patches
  let pre = partPreTable parts
      rebuilt =
        B.concat
          [ B.take firstRecordSlotOffset pre
          , patched
          , B.drop (firstRecordSlotOffset + firstRecordSlotLength) pre
          ]
  pure parts {partPreTable = rebuilt}

applyPatches :: B.ByteString -> [(Int, B.ByteString)] -> Either String B.ByteString
applyPatches slot patches = do
  let ordered = sortOn fst patches
  forM_ ordered $ \(offset, patch) ->
    if offset < 0 || offset + B.length patch > B.length slot
      then Left "first-stage patch is outside the slot"
      else pure ()
  forM_ (zip ordered (drop 1 ordered)) $ \((start, bytes), (nextStart, _)) ->
    if start + B.length bytes > nextStart
      then Left "first-stage patches overlap"
      else pure ()
  pure (B.concat (assemble 0 ordered))
  where
    assemble previous [] = [B.drop previous slot]
    assemble previous ((offset, patch) : rest) =
      B.take (offset - previous) (B.drop previous slot)
        : patch
        : assemble (offset + B.length patch) rest
