{-# LANGUAGE OverloadedStrings #-}
-- Bounded first-record replacement in the opaque pre-table container part.
module FujiAF.FirstStage
  ( ArmEntry
  , SlotOffset
  , RetainedExtent
  , FirstStagePlan
  , Symbol
  , symbolAddress
  , symbolSize
  , symbolName
  , parseSymbols
  , lookupSymbol
  , symbolSlotOffset
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
  , ExpectedMediaPatch
  , expectedMediaPatch
  , replaceExpectedFirstRecord
  ) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (complement, (.&.))
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.UpdateContainer (MediaImage (..), Parts (..), putMediaWord)
import Numeric (readHex)

newtype ArmEntry = ArmEntry Word32
  deriving (Eq, Show)

newtype SlotOffset = SlotOffset Int
  deriving (Eq, Show)

data RetainedExtent = RetainedExtent SlotOffset SlotOffset
  deriving (Eq, Show)

data FirstStagePlan = FirstStagePlan ArmEntry SlotOffset MediaImage [RetainedExtent]
  deriving (Eq, Show)

data ExpectedMediaPatch = ExpectedMediaPatch SlotOffset MediaImage MediaImage
  deriving (Eq, Show)

expectedMediaPatch :: SlotOffset -> MediaImage -> MediaImage -> Either String ExpectedMediaPatch
expectedMediaPatch offset expected replacement
  | start .&. 3 /= 0 || size .&. 3 /= 0 = Left "expected-media patch is not word aligned"
  | size == 0 = Left "expected-media patch is empty"
  | size /= B.length (mediaImageBytes replacement) = Left "expected-media patch lengths differ"
  | size > firstRecordSlotLength - start = Left "expected-media patch extends past the slot"
  | otherwise = Right (ExpectedMediaPatch offset expected replacement)
  where
    start = slotOffsetBytes offset
    size = B.length (mediaImageBytes expected)

replaceExpectedFirstRecord :: [ExpectedMediaPatch] -> Parts -> Either String Parts
replaceExpectedFirstRecord patches parts = do
  slot <- firstRecordSlotBytes parts
  forM_ patches $ \(ExpectedMediaPatch offset expected _) ->
    if B.map complement (B.take (B.length (mediaImageBytes expected)) (B.drop (slotOffsetBytes offset) slot))
        == mediaImageBytes expected
      then pure ()
      else Left "first-record expected media bytes differ"
  replaceSlotPatches
    [(slotOffsetBytes offset, B.map complement (mediaImageBytes replacement))
    | ExpectedMediaPatch offset _ replacement <- patches] parts

planEntry :: FirstStagePlan -> ArmEntry
planEntry (FirstStagePlan entry _ _ _) = entry

planLoadOffset :: FirstStagePlan -> SlotOffset
planLoadOffset (FirstStagePlan _ load _ _) = load

planImage :: FirstStagePlan -> MediaImage
planImage (FirstStagePlan _ _ image _) = image

planRetainedExtents :: FirstStagePlan -> [RetainedExtent]
planRetainedExtents (FirstStagePlan _ _ _ retained) = retained

data Symbol = Symbol Word32 Word32 String
  deriving (Eq, Show)

symbolAddress :: Symbol -> Word32
symbolAddress (Symbol address _ _) = address

symbolSize :: Symbol -> Word32
symbolSize (Symbol _ size _) = size

symbolName :: Symbol -> String
symbolName (Symbol _ _ name) = name

parseSymbols :: B.ByteString -> Either String [Symbol]
parseSymbols bytes = mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line =
      case C.words line of
        [address, size, _kind, name] -> do
          addressValue <- parseWord32 (C.unpack address)
          sizeValue <- parseWord32 (C.unpack size)
          pure (Symbol addressValue sizeValue (C.unpack name))
        _ -> Left ("symbol row has no four-column llvm-nm shape: " ++ C.unpack line)

parseWord32 :: String -> Either String Word32
parseWord32 text =
  case readHex (dropPrefix text) of
    [(value, "")]
      | value >= 0 && value <= 0xffffffff -> Right (fromIntegral (value :: Integer))
    _ -> Left ("value is not a 32-bit hexadecimal number: " ++ text)
  where
    dropPrefix value
      | take 2 value `elem` ["0x", "0X"] = drop 2 value
      | otherwise = value

lookupSymbol :: [Symbol] -> String -> Either String Symbol
lookupSymbol symbols name =
  case filter ((== name) . symbolName) symbols of
    [symbol] -> Right symbol
    [] -> Left ("candidate symbols lack " ++ name)
    _ -> Left ("candidate symbols repeat " ++ name)

symbolSlotOffset :: [Symbol] -> String -> Either String SlotOffset
symbolSlotOffset symbols name = do
  symbol <- lookupSymbol symbols name
  slotOffset (fromIntegral (symbolAddress symbol))

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
  patches <- planPatches plan
  replaceSlotPatches patches parts

replaceSlotPatches :: [(Int, B.ByteString)] -> Parts -> Either String Parts
replaceSlotPatches patches parts = do
  slot <- firstRecordSlotBytes parts
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
