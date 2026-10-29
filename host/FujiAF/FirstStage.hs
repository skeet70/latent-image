{-# LANGUAGE OverloadedStrings #-}
-- Bounded first-record replacement in the opaque pre-table container part.
module FujiAF.FirstStage
  ( FirstStagePlan (..)
  , firstRecordSlotOffset
  , firstRecordSlotLength
  , resetVectorWordOffset
  , firstRecordSlotBytes
  , planPatches
  , replaceFirstRecordSlot
  ) where

import Control.Monad (foldM, forM_)
import qualified Data.ByteString as B
import Data.Bits (complement)
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.UpdateContainer (MediaImage (..), Parts (..), putMediaWord)

data FirstStagePlan = FirstStagePlan
  { planEntry :: Word32
  , planLoadOffset :: Int
  , planImage :: MediaImage
  , planRetainedExtents :: [(Int, Int)]
  }
  deriving (Eq, Show)

-- Media slot [0x40000,0x50000) inside the container's main pre-table.
firstRecordSlotOffset :: Int
firstRecordSlotOffset = 0x40000

firstRecordSlotLength :: Int
firstRecordSlotLength = 0x10000

resetVectorWordOffset :: Int
resetVectorWordOffset = 0x20

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
  | load + B.length image > firstRecordSlotLength =
      Left "first-stage image extends past the first-record slot"
  | load < resetVectorWordOffset + 4 =
      Left "first-stage image overlaps the reset vector word"
  | entry < load || entry >= load + B.length image =
      Left "first-stage entry is outside the candidate image"
  | entry `mod` 4 /= 0 = Left "first-stage entry is not word aligned"
  | otherwise = do
      forM_ (planRetainedExtents plan) $ \(start, end) ->
        if start < 0 || end > firstRecordSlotLength || start >= end
          then Left "retained extent is outside the first-record slot"
          else pure ()
      if any (overlaps (load, load + B.length image)) (planRetainedExtents plan)
        then Left "first-stage image overlaps a retained extent"
        else pure ()
      pure
        [ (resetVectorWordOffset, putMediaWord (planEntry plan))
        , (load, B.map complement image)
        ]
  where
    image = mediaImageBytes (planImage plan)
    load = planLoadOffset plan
    entry = fromIntegral (planEntry plan) :: Int

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
        B.take firstRecordSlotOffset pre
          <> patched
          <> B.drop (firstRecordSlotOffset + firstRecordSlotLength) pre
  pure parts {partPreTable = rebuilt}

applyPatches :: B.ByteString -> [(Int, B.ByteString)] -> Either String B.ByteString
applyPatches slot patches = do
  let ordered = sortOn fst patches
  forM_ (zip ordered (drop 1 ordered)) $ \((start, bytes), (nextStart, _)) ->
    if start + B.length bytes > nextStart
      then Left "first-stage patches overlap"
      else pure ()
  foldM applyOne slot ordered
  where
    applyOne bytes (offset, patch)
      | offset < 0 || offset + B.length patch > B.length bytes =
          Left "first-stage patch is outside the slot"
      | otherwise =
          Right
            ( B.take offset bytes
                <> patch
                <> B.drop (offset + B.length patch) bytes
            )
