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
  , ComposedStagePlan
  , composedStagePlan
  , composedFirstPlan
  , composedSecondPlan
  , composedPublication
  , replaceComposedRecords
  , verifyComposedRecords
  , LoaderId (..)
  , LoaderWordPatch
  , loaderPatchId
  , loaderPatchAddress
  , loaderPatchExpected
  , loaderPatchReplacement
  , loaderWordPatch
  , loaderEntryPublication
  , loaderWordPatchOffset
  , applyLoaderWordPatches
  , verifyLoaderWordPatches
  , verifyPreTablePatches
  , ExpectedMediaPatch
  , expectedMediaPatch
  , replaceExpectedFirstRecord
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (complement, (.&.))
import Data.List (findIndex, sortOn)
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

data ComposedStagePlan = ComposedStagePlan StagePlan StagePlan LoaderWordPatch
  deriving (Eq, Show)

data ExpectedMediaPatch = ExpectedMediaPatch SlotOffset MediaImage MediaImage
  deriving (Eq, Show)

-- | A segment-table id the boot loads as a raw image whose first bytes are its
-- header (ids 0 and 1).
newtype LoaderId = LoaderId Word32
  deriving (Eq, Show)

-- | One word of a loaded segment image, named by its guest address after the
-- load, replaced only over its expected preimage.
data LoaderWordPatch = LoaderWordPatch
  { loaderPatchId :: LoaderId
  , loaderPatchAddress :: Word32
  , loaderPatchExpected :: Word32
  , loaderPatchReplacement :: Word32
  }
  deriving (Eq, Show)

loaderWordPatch :: LoaderId -> Word32 -> Word32 -> Word32 -> Either String LoaderWordPatch
loaderWordPatch loader address expected replacement
  | address .&. 3 /= 0 = Left "loader word patch is not word aligned"
  | expected == replacement = Left "loader word patch replaces a word with itself"
  | otherwise = Right (LoaderWordPatch loader address expected replacement)

-- | Header word `+0xc` of the image loaded at `load`: the entry record 1
-- (`0x1d8c`, id 1 on the non-`0x4003` path) and record 2 (`0x11f90`) publish.
loaderEntryPublication :: LoaderId -> Word32 -> ArmEntry -> ArmEntry -> Either String LoaderWordPatch
loaderEntryPublication loader load stock entry =
  loaderWordPatch loader (load + 0xc) (armEntryAddress stock) (armEntryAddress entry)

-- | The patch word's offset in the main pre-table, which holds the image from
-- its segment's flash offset.
loaderWordPatchOffset :: Parts -> LoaderWordPatch -> Either String Int
loaderWordPatchOffset parts patch = do
  let LoaderId wanted = loaderPatchId patch
      address = toInteger (loaderPatchAddress patch)
  segment <- case filter ((== wanted) . U.segId) (partSegments parts) of
    [segment] -> Right segment
    _ -> Left ("segment table lacks a unique loader id " ++ show wanted)
  let load = toInteger (U.segLoad segment)
      size = toInteger (U.segSize segment)
      offset = toInteger (U.segFlash segment) + address - load
  unless (address >= load && address + 4 <= load + size)
    (Left ("loader word " ++ U.showHexWord (loaderPatchAddress patch) ++ " is outside loader id " ++ show wanted))
  unless (offset + 4 <= toInteger (B.length (partPreTable parts)))
    (Left "loader word is outside the main pre-table")
  pure (fromInteger offset)

applyLoaderWordPatches :: [LoaderWordPatch] -> Parts -> Either String Parts
applyLoaderWordPatches patches parts = do
  located <- forM patches $ \patch -> do
    offset <- loaderWordPatchOffset parts patch
    unless (U.mediaWordAt (partPreTable parts) offset == loaderPatchExpected patch)
      (Left ("loader word " ++ U.showHexWord (loaderPatchAddress patch) ++ " differs from its expected preimage"))
    pure (offset, putMediaWord (loaderPatchReplacement patch))
  rebuilt <- applyPatches (partPreTable parts) located
  pure parts {partPreTable = rebuilt}

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
-- only at the plan's record-slot patches and the loader words, no other part
-- changes, the reset vector names the entry, the staged bytes are the image
-- and each loader word holds its replacement.
verifyStagedRecord :: StagePlan -> [LoaderWordPatch] -> U.Parsed -> U.Parsed -> Either String ()
verifyStagedRecord plan loaderPatches source rebuilt = do
  let record = planRecord plan
      slotStart = recordSlotOffset record
  patches <- planPatches plan
  verifyPreTablePatches [(slotStart + offset, B.length bytes) | (offset, bytes) <- patches] loaderPatches source rebuilt
  stagedSlot <- recordSlotBytes record (U.parsedParts rebuilt)
  unless (U.mediaWordAt stagedSlot resetVectorWordOffset == armEntryAddress (planEntry plan))
    (Left "staged reset vector does not name the candidate entry")
  let image = mediaImageBytes (planImage plan)
      load = slotOffsetBytes (planLoadOffset plan)
  unless (B.map complement (B.take (B.length image) (B.drop load stagedSlot)) == image)
    (Left "staged bytes differ from the candidate image")

composedStagePlan :: StagePlan -> StagePlan -> LoaderWordPatch -> Either String ComposedStagePlan
composedStagePlan first second publication = do
  unless (planRecord first == FirstRecord && planRecord second == SecondRecord)
    (Left "composed startup requires distinct first and second record plans")
  _ <- planPatches first
  _ <- planPatches second
  let load = toInteger (recordGuestBase SecondRecord + slotOffsetBytes (planLoadOffset second))
      end = load + toInteger (B.length (mediaImageBytes (planImage second)))
      target = toInteger (loaderPatchReplacement publication)
  unless (loaderPatchId publication == LoaderId 1 && loaderPatchAddress publication == 0xd000c
      && loaderPatchExpected publication == 0xd0020 && target `mod` 4 == 0 && target >= load && target < end)
    (Left "composed startup requires loader-B entry publication into the second image")
  pure (ComposedStagePlan first second publication)

composedFirstPlan :: ComposedStagePlan -> StagePlan
composedFirstPlan (ComposedStagePlan first _ _) = first

composedSecondPlan :: ComposedStagePlan -> StagePlan
composedSecondPlan (ComposedStagePlan _ second _) = second

composedPublication :: ComposedStagePlan -> LoaderWordPatch
composedPublication (ComposedStagePlan _ _ publication) = publication

replaceComposedRecords :: ComposedStagePlan -> Parts -> Either String Parts
replaceComposedRecords (ComposedStagePlan first second publication) source = do
  _ <- composedLoaderOffset source publication
  replaceRecordSlot first source >>= replaceRecordSlot second >>= applyLoaderWordPatches [publication]

composedLoaderOffset :: Parts -> LoaderWordPatch -> Either String Int
composedLoaderOffset parts publication = do
  case filter ((== 1) . U.segId) (partSegments parts) of
    [segment] | U.segLoad segment == 0xd0000 -> pure ()
    _ -> Left "composed startup requires loader B loaded at its declared base"
  loaderWordPatchOffset parts publication

verifyComposedRecords :: ComposedStagePlan -> U.Parsed -> U.Parsed -> Either String ()
verifyComposedRecords plan source rebuilt = do
  forM_ [source, rebuilt] $ \parsed ->
    forM_ (U.checks parsed ++ U.carriedChecks parsed) $ \check ->
      unless (U.checkPassed check) (Left ("composed container check failed: " ++ U.checkField check))
  let plans = [composedFirstPlan plan, composedSecondPlan plan]
      publication = composedPublication plan
      sourceParts = U.parsedParts source
  patches <- fmap concat $ forM plans $ \recordPlan -> do
    patches <- planPatches recordPlan
    pure [(recordSlotOffset (planRecord recordPlan) + offset, bytes) | (offset, bytes) <- patches]
  offset <- composedLoaderOffset sourceParts publication
  unless (U.mediaWordAt (partPreTable sourceParts) offset == loaderPatchExpected publication)
    (Left "composed loader-B publication preimage differs")
  verifyPreTablePatches [(start, B.length bytes) | (start, bytes) <- patches] [publication] source rebuilt
  let rebuiltPre = partPreTable (U.parsedParts rebuilt)
  forM_ patches $ \(start, bytes) ->
    unless (B.take (B.length bytes) (B.drop start rebuiltPre) == bytes)
      (Left "composed record patch differs from its planned bytes")
  forM_ plans $ \recordPlan -> do
    let record = planRecord recordPlan
    before <- recordSlotBytes record sourceParts
    after <- recordSlotBytes record (U.parsedParts rebuilt)
    let image = mediaImageBytes (planImage recordPlan)
        load = slotOffsetBytes (planLoadOffset recordPlan)
    unless (B.map complement (B.take (B.length image) (B.drop load after)) == image)
      (Left "composed extracted image differs from its linked bytes")
    forM_ (planRetainedExtents recordPlan) $ \extent -> do
      let (start, end) = retainedExtentRange extent
          window = B.take (end - start) . B.drop start
      unless (window before == window after) (Left "composed retained record bytes changed")

-- | Checks a rebuilt container against its source: only the loader words
-- change, each to its replacement.
verifyLoaderWordPatches :: [LoaderWordPatch] -> U.Parsed -> U.Parsed -> Either String ()
verifyLoaderWordPatches = verifyPreTablePatches []

-- | As 'verifyLoaderWordPatches', also admitting (pre-table offset, length)
-- extents, whose contents the caller checks.
verifyPreTablePatches :: [(Int, Int)] -> [LoaderWordPatch] -> U.Parsed -> U.Parsed -> Either String ()
verifyPreTablePatches extents loaderPatches source rebuilt = do
  let sourceParts = U.parsedParts source
      sourcePre = partPreTable sourceParts
      rebuiltPre = partPreTable (U.parsedParts rebuilt)
  unless (B.length sourcePre == B.length rebuiltPre) (Left "main pre-table length changed")
  forM_ (U.diffParts source rebuilt) $ \difference ->
    when (U.diffChangedBytes difference /= 0 && U.diffPart difference /= "main pre-table")
      (Left ("unexpected changed part: " ++ U.diffPart difference))
  loaderOffsets <- mapM (loaderWordPatchOffset sourceParts) loaderPatches
  let declared = sortOn fst (extents ++ [(offset, 4) | offset <- loaderOffsets])
  forM_ (zip declared (drop 1 declared)) $ \((start, size), (next, _)) ->
    when (start + size > next) (Left "declared pre-table patches overlap")
  forM_ (changedOutside declared sourcePre rebuiltPre) $ \index ->
    Left ("main pre-table byte changed outside the declared patches: " ++ show index)
  forM_ (zip loaderPatches loaderOffsets) $ \(patch, offset) ->
    unless (U.mediaWordAt rebuiltPre offset == loaderPatchReplacement patch)
      (Left ("staged loader word " ++ U.showHexWord (loaderPatchAddress patch) ++ " differs from its replacement"))

-- | The first index where the two strings differ outside the sorted extents.
changedOutside :: [(Int, Int)] -> B.ByteString -> B.ByteString -> Maybe Int
changedOutside extents before after = go 0 extents
  where
    go cursor [] = firstDifference cursor (B.length before)
    go cursor ((start, size) : rest) =
      maybe (go (max cursor (start + size)) rest) Just (firstDifference cursor start)
    firstDifference from to
      | from >= to || window before == window after = Nothing
      | otherwise = (from +) <$> findIndex id (B.zipWith (/=) (window before) (window after))
      where
        window = B.take (to - from) . B.drop from

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
