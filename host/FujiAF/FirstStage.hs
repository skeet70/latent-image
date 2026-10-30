{-# LANGUAGE OverloadedStrings #-}
-- Bounded boot-record replacement in the opaque pre-table container part.
module FujiAF.FirstStage
  ( ArmEntry
  , SlotOffset
  , RetainedExtent
  , BootRecord (..)
  , StagePlan
  , Symbol
  , symbolAddress
  , symbolSize
  , symbolName
  , parseSymbols
  , lookupSymbol
  , symbolSlotOffset
  , recordSlotOffset
  , recordGuestBase
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
  , stagePlan
  , planRecord
  , planEntry
  , planLoadOffset
  , planImage
  , planRetainedExtents
  , recordSlotBytes
  , firstRecordSlotBytes
  , planPatches
  , replaceRecordSlot
  , verifyStagedRecord
  , ExpectedMediaPatch
  , expectedMediaPatch
  , replaceExpectedFirstRecord
  ) where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (complement, (.&.))
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.UpdateContainer (MediaImage (..), Parts (..), putMediaWord)
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)

newtype ArmEntry = ArmEntry Word32
  deriving (Eq, Show)

newtype SlotOffset = SlotOffset Int
  deriving (Eq, Show)

data RetainedExtent = RetainedExtent SlotOffset SlotOffset
  deriving (Eq, Show)

-- | The two boot records the ROM stages from pre-table [0x40000,0x60000).
data BootRecord = FirstRecord | SecondRecord
  deriving (Eq, Show)

data StagePlan = StagePlan BootRecord ArmEntry SlotOffset MediaImage [RetainedExtent]
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
    FirstRecord
    [(slotOffsetBytes offset, B.map complement (mediaImageBytes replacement))
    | ExpectedMediaPatch offset _ replacement <- patches] parts

planRecord :: StagePlan -> BootRecord
planRecord (StagePlan record _ _ _ _) = record

planEntry :: StagePlan -> ArmEntry
planEntry (StagePlan _ entry _ _ _) = entry

planLoadOffset :: StagePlan -> SlotOffset
planLoadOffset (StagePlan _ _ load _ _) = load

planImage :: StagePlan -> MediaImage
planImage (StagePlan _ _ _ image _) = image

planRetainedExtents :: StagePlan -> [RetainedExtent]
planRetainedExtents (StagePlan _ _ _ _ retained) = retained

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

symbolSlotOffset :: BootRecord -> [Symbol] -> String -> Either String SlotOffset
symbolSlotOffset record symbols name = do
  symbol <- lookupSymbol symbols name
  let address = fromIntegral (symbolAddress symbol) - recordGuestBase record
  if address < 0
    then Left (name ++ " is below the record's load address")
    else slotOffset address

recordSlotOffset :: BootRecord -> Int
recordSlotOffset FirstRecord = 0x40000
recordSlotOffset SecondRecord = 0x50000

-- | Guest address of slot offset 0 after the ROM stages the record.
recordGuestBase :: BootRecord -> Int
recordGuestBase FirstRecord = 0
recordGuestBase SecondRecord = 0x10000

firstRecordSlotOffset :: Int
firstRecordSlotOffset = recordSlotOffset FirstRecord

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
  | value > firstRecordSlotLength = Left "slot offset is past the record slot"
  | otherwise = Right (SlotOffset value)

slotOffsetBytes :: SlotOffset -> Int
slotOffsetBytes (SlotOffset value) = value

retainedExtent :: SlotOffset -> SlotOffset -> Either String RetainedExtent
retainedExtent start end
  | slotOffsetBytes start >= slotOffsetBytes end = Left "retained extent is empty or reversed"
  | otherwise = Right (RetainedExtent start end)

retainedExtentRange :: RetainedExtent -> (Int, Int)
retainedExtentRange (RetainedExtent start end) = (slotOffsetBytes start, slotOffsetBytes end)

stagePlan :: BootRecord -> ArmEntry -> SlotOffset -> MediaImage -> [RetainedExtent] -> Either String StagePlan
stagePlan record entry load image retained = do
  let plan = StagePlan record entry load image retained
  _ <- planPatches plan
  pure plan

recordSlotBytes :: BootRecord -> Parts -> Either String B.ByteString
recordSlotBytes record parts
  | B.length (partPreTable parts) < start + firstRecordSlotLength =
      Left "carrier pre-table does not cover the record slot"
  | otherwise =
      Right (B.take firstRecordSlotLength (B.drop start (partPreTable parts)))
  where
    start = recordSlotOffset record

firstRecordSlotBytes :: Parts -> Either String B.ByteString
firstRecordSlotBytes = recordSlotBytes FirstRecord

planPatches :: StagePlan -> Either String [(Int, B.ByteString)]
planPatches plan
  | B.null image = Left "first-stage image is empty"
  | load < 0 = Left "first-stage load offset is negative"
  | load .&. 3 /= 0 = Left "first-stage load offset is not word aligned"
  | load + B.length image > firstRecordSlotLength =
      Left "first-stage image extends past the record slot"
  | load < resetVectorWordOffset + 4 =
      Left "first-stage image overlaps the reset vector word"
  | entry < load || entry >= load + B.length image =
      Left "first-stage entry is outside the candidate image"
  | otherwise = do
      forM_ retained $ \(start, end) ->
        if start < 0 || end > firstRecordSlotLength || start >= end
          then Left "retained extent is outside the record slot"
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
    entry = fromIntegral (armEntryAddress (planEntry plan)) - recordGuestBase (planRecord plan)
    retained = map retainedExtentRange (planRetainedExtents plan)

overlaps :: (Int, Int) -> (Int, Int) -> Bool
overlaps (leftStart, leftEnd) (rightStart, rightEnd) =
  leftStart < rightEnd && rightStart < leftEnd

replaceRecordSlot :: StagePlan -> Parts -> Either String Parts
replaceRecordSlot plan parts = do
  patches <- planPatches plan
  replaceSlotPatches (planRecord plan) patches parts

-- | Checks a rebuilt container against its source: the main pre-table changes
-- only inside the plan's record slot and only at its patches, no other part
-- changes, the reset vector names the entry and the staged bytes are the image.
verifyStagedRecord :: StagePlan -> U.Parsed -> U.Parsed -> Either String ()
verifyStagedRecord plan source rebuilt = do
  let record = planRecord plan
      sourcePre = partPreTable (U.parsedParts source)
      rebuiltPre = partPreTable (U.parsedParts rebuilt)
      slotStart = recordSlotOffset record
      slotEnd = slotStart + firstRecordSlotLength
  unless (B.length sourcePre == B.length rebuiltPre) (Left "main pre-table length changed")
  unless (B.take slotStart sourcePre == B.take slotStart rebuiltPre)
    (Left "carrier before the record slot changed")
  unless (B.drop slotEnd sourcePre == B.drop slotEnd rebuiltPre)
    (Left "carrier after the record slot changed")
  forM_ (U.diffParts source rebuilt) $ \difference ->
    when (U.diffChangedBytes difference /= 0 && U.diffPart difference /= "main pre-table")
      (Left ("unexpected changed part: " ++ U.diffPart difference))
  sourceSlot <- recordSlotBytes record (U.parsedParts source)
  stagedSlot <- recordSlotBytes record (U.parsedParts rebuilt)
  patches <- planPatches plan
  let patched index = any (\(offset, bytes) -> index >= offset && index < offset + B.length bytes) patches
      outside = [index | (index, before, after) <- zip3 [0 ..] (B.unpack sourceSlot) (B.unpack stagedSlot), before /= after, not (patched index)]
  forM_ (take 1 outside) $ \index ->
    Left ("record byte changed outside the declared patches: " ++ show (index :: Int))
  unless (U.mediaWordAt stagedSlot resetVectorWordOffset == armEntryAddress (planEntry plan))
    (Left "staged reset vector does not name the candidate entry")
  let image = mediaImageBytes (planImage plan)
      load = slotOffsetBytes (planLoadOffset plan)
  unless (B.map complement (B.take (B.length image) (B.drop load stagedSlot)) == image)
    (Left "staged bytes differ from the candidate image")

replaceSlotPatches :: BootRecord -> [(Int, B.ByteString)] -> Parts -> Either String Parts
replaceSlotPatches record patches parts = do
  slot <- recordSlotBytes record parts
  patched <- applyPatches slot patches
  let pre = partPreTable parts
      start = recordSlotOffset record
      rebuilt =
        B.concat
          [ B.take start pre
          , patched
          , B.drop (start + firstRecordSlotLength) pre
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
