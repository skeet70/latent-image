{-# LANGUAGE OverloadedStrings #-}
-- The editable custom-recipe bank: the typed slot ABI, the 0x28 payload codec,
-- the 26-byte name codec, the menu arrays the bank's lists need, and the
-- write-plan validator. Host-only; no camera, no container access.
--
-- The ABI is the one agreed with the storage lane (recipe-design section 16):
-- one slot is a full 0x28 C-payload read through the property layer, plus a
-- 12-unit UTF-16 name held apart from the payload so a rename never rewrites
-- payload bytes. Slot count is the bank's, not this module's; 'bankCapacity'
-- states the count this build is sized for.
--
-- The field placement, the nine edit fields' conversion tables and the payload's
-- unbound mask come from the authoritative map
-- (reports/custom-settings.fields.json) through FujiAF.RecipeBankFields, the
-- generated module the storage kernel's table is generated from too: this
-- module holds no placement literal of its own.
--
-- The conversion tables are the pinned 1.50 tables (recipe-design P7):
-- property 0xfe 0x0077ac30, 0x104 0x00775428, 0x110 0x00775e24, 0x111
-- 0x0077643c, 0x10e 0x0077a810, 0x10f 0x00778620, 0xef 0x00779f10,
-- 0xed 0x0077a60c, 0x141 0x0077af48. A row is 12 bytes {col0 stored,
-- 0x7ffffffe, col2 effective}, terminated by a col0 of 0xffff. The menu
-- carries row indices; the payload carries col0. This module is the only place
-- that crosses between the two directions.
module FujiAF.RecipeBank
  ( -- * Conversion tables
    TableRow (..)
  , ConversionTable (..)
  , readConversionTable
  , rowToCode
  , codeToRow
    -- * Field table
  , RecipeField (..)
  , recipeFields
  , recallOrder
  , appliedProperties
  , fieldForProperty
    -- * Slots
  , StoredCode (..)
  , Recipe (..)
  , recipePairs
  , recipeFromPairs
  , encodeSlotPayload
  , encodeSlotPayloadOver
  , payloadBytes
  , bankCapacity
  , bankBlobBytes
    -- * Names
  , nameUnits
  , nameBytes
  , encodeName
  , decodeName
  , nameRecordBytes
  , nameRecordBufferBytes
  , nameRecordLayoutWords
  , encodeNameRecord
  , decodeNameRecord
  , labelUnits
  , encodeLabelRecord
    -- * Menu arrays
  , valuesTerminator
  , encodeValuesArray
  , appendValuesPairs
  , encodeLabelsArray
  , appendLabelQuadruples
  , encodeParentAppend
  , encodeListNode
    -- * Code words
  , armBranchWord
  , armTailBranchWord
  , thumbBranchWord
    -- * Write plan
  , Region (..)
  , PlanWrite (..)
  , WritePlan (..)
  , planNewBytes
  , validatePlan
  , PlainLoadAddress (..)
  , PlainFlashOffset (..)
  , validatePlainCarrier
    -- * Bank file
  , FileSlot (..)
  , encodeFileBlob
  , decodeFileBlob
  , BankHeader (..)
  , headerBytes
  , bankFileBytes
  , bankFileA
  , bankFileB
  , BankFileId (..)
  , bankChecksum
  , encodeBankFile
  , decodeBankFile
  , pickCurrentFile
  , chooseWriteTarget
  , nextGeneration
  ) where

import Control.Monad (when)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (ord)
import Data.List (sort, sortOn)
import Data.Maybe (fromMaybe)
import Data.Word (Word16, Word32, Word8)
import qualified FujiAF.RecipeBankFields as F

-- conversion tables -----------------------------------------------------

-- | One table row: col0 is the stored code a payload carries, col2 the value
-- every downstream consumer sees.
data TableRow = TableRow
  { rowCode :: StoredCode
  , rowEffective :: Word32
  }
  deriving (Eq, Show)

data ConversionTable = ConversionTable
  { tableAddress :: Word32
  , tableRows :: [TableRow]
  }
  deriving (Eq, Show)

-- | Reads one pinned conversion table out of an image-4 decode. `base` is the
-- decode's load address and `bytes` its image; rows run until a col0 of
-- 0xffff, and a table that does not terminate inside the image is refused.
readConversionTable :: Word32 -> B.ByteString -> Word32 -> Either String ConversionTable
readConversionTable base image address = go 0
 where
  offset = fromIntegral (address - base) :: Int
  go k
    | offset + 12 * (k + 1) > B.length image = Left ("conversion table " ++ hex32 address ++ ": no terminator inside the image")
    | otherwise =
        let at i = fromIntegral (B.index image (offset + 12 * k + i)) :: Word32
            word i = at i .|. (at (i + 1) `shiftL` 8) .|. (at (i + 2) `shiftL` 16) .|. (at (i + 3) `shiftL` 24)
         in if word 0 == 0xffff
              then pure (ConversionTable address (rows k))
              else go (k + 1)
  rows k = [TableRow (StoredCode (fromIntegral (lowCode j))) (lowEffective j) | j <- [0 .. k - 1]]
  lowCode j = fromIntegral (B.index image (offset + 12 * j)) :: Word8
  lowEffective j =
    foldl'
      (\acc i -> acc .|. (fromIntegral (B.index image (offset + 12 * j + 8 + i)) `shiftL` (8 * i)))
      0
      [0 .. 3]

-- | The row index a values array carries, to the stored code a payload holds.
rowToCode :: ConversionTable -> Int -> Either String StoredCode
rowToCode table row
  | row < 0 = outOfRange
  | otherwise = case drop row (tableRows table) of
      (r : _) -> pure (rowCode r)
      [] -> outOfRange
 where
  outOfRange = Left ("conversion table " ++ hex32 (tableAddress table) ++ ": row " ++ show row ++ " out of range")

-- | The recall direction: a payload's stored code back to the row the commit
-- path takes. A code the table does not carry is refused, which is what keeps
-- a stale bank from being applied field by field.
codeToRow :: ConversionTable -> StoredCode -> Either String Int
codeToRow table code = case [k | (k, r) <- zip [0 :: Int ..] (tableRows table), rowCode r == code] of
  [k] -> pure k
  [] -> Left ("conversion table " ++ hex32 (tableAddress table) ++ ": code " ++ show code ++ " is not a stock row")
  ks -> Left ("conversion table " ++ hex32 (tableAddress table) ++ ": code " ++ show code ++ " is ambiguous over rows " ++ show ks)

-- field table -----------------------------------------------------------

-- | One recipe field: the property whose commit arm applies it, its place in
-- the 0x28 payload, and the pinned table that carries row to stored code.
data RecipeField = RecipeField
  { fieldProperty :: Word16
  , fieldOffset :: Int
  , fieldMask :: Word8
  , fieldShift :: Int
  , fieldTable :: Word32
  , fieldLabel :: String
  }
  deriving (Eq, Show)

-- | The nine fields the bank applies, from the authoritative payload field map
-- through FujiAF.RecipeBankFields (the same generated table the storage kernel
-- carries): the map's edit rows in edit order, so this module holds no
-- placement literal of its own.
recipeFields :: [RecipeField]
recipeFields = map editField F.editFields
 where
  editField f =
    RecipeField
      (F.fieldProperty f)
      (fromIntegral (F.fieldOffset f))
      (fromIntegral (F.fieldMask f))
      (fromIntegral (F.fieldShift f))
      (F.fieldTable f)
      (maybe (show (F.fieldProperty f)) id (F.fieldLabel f))

-- | The order a recall writes the fields in: the eight parametric fields, then
-- the film field last, so the film arm's mode-dependent work runs with the
-- parameters final (the rule recipe-design section 5 set for RC1, kept here).
recallOrder :: [RecipeField]
recallOrder =
  filter ((/= 0xfe) . fieldProperty) recipeFields
    ++ filter ((== 0xfe) . fieldProperty) recipeFields

appliedProperties :: [Word16]
appliedProperties = map fieldProperty recipeFields

-- slots -----------------------------------------------------------------

-- | A stored code: the conversion table's col0 value, which is what the 0x28
-- payload carries.
newtype StoredCode = StoredCode Word8
  deriving (Eq, Ord, Show)

-- | One slot's nine stored codes.
data Recipe = Recipe
  { recipeDynamicRange :: StoredCode
  , recipeHighlight :: StoredCode
  , recipeShadow :: StoredCode
  , recipeColour :: StoredCode
  , recipeSharpness :: StoredCode
  , recipeNoiseReduction :: StoredCode
  , recipeWhiteBalance :: StoredCode
  , recipeIso :: StoredCode
  , recipeFilm :: StoredCode
  }
  deriving (Eq, Show)

recipePairs :: Recipe -> [(Word16, StoredCode)]
recipePairs r =
  [ (0x104, recipeDynamicRange r)
  , (0x110, recipeHighlight r)
  , (0x111, recipeShadow r)
  , (0x10e, recipeColour r)
  , (0x10f, recipeSharpness r)
  , (0xef, recipeNoiseReduction r)
  , (0xed, recipeWhiteBalance r)
  , (0x141, recipeIso r)
  , (0xfe, recipeFilm r)
  ]

-- | Total by construction: every applied property must appear exactly once.
recipeFromPairs :: [(Word16, StoredCode)] -> Either String Recipe
recipeFromPairs pairs
  | sort (map fst pairs) /= sort (map fst expected) =
      Left ("recipe: expected fields " ++ show appliedProperties ++ ", got " ++ show (map fst pairs))
  | otherwise =
      pure
        Recipe
          { recipeDynamicRange = code 0x104
          , recipeHighlight = code 0x110
          , recipeShadow = code 0x111
          , recipeColour = code 0x10e
          , recipeSharpness = code 0x10f
          , recipeNoiseReduction = code 0xef
          , recipeWhiteBalance = code 0xed
          , recipeIso = code 0x141
          , recipeFilm = code 0xfe
          }
 where
  expected = map (\p -> (p, StoredCode 0)) appliedProperties
  code p = fromMaybe (StoredCode 0) (lookup p pairs)

-- | The payload size every slot record carries: the bank slot record, the
-- per-slot defaults row and the settings-image slot area all use 0x28.
payloadBytes :: Int
payloadBytes = 0x28

-- | The field a property belongs to; every applied property has exactly one.
fieldForProperty :: Word16 -> Either String RecipeField
fieldForProperty p = case filter ((== p) . fieldProperty) recipeFields of
  [f] -> pure f
  [] -> Left ("recipe bank: property " ++ show p ++ " is not an applied recipe field")
  _ -> Left ("recipe bank: property " ++ show p ++ " is bound twice")

-- | Encodes one slot's payload over the body's own defaults: every property the
-- recipe names has its code placed at its bound offset, mask and shift, and
-- every other byte keeps the base's value. A code that does not fit its mask is
-- refused, so a caller cannot silently truncate one.
encodeSlotPayloadOver :: B.ByteString -> Recipe -> Either String B.ByteString
encodeSlotPayloadOver base recipe
  | B.length base /= payloadBytes =
      Left ("payload: base is " ++ show (B.length base) ++ " bytes, expected " ++ show payloadBytes)
  | otherwise = foldl' step (Right base) (recipePairs recipe)
 where
  step bytes (property, StoredCode c) = do
    current <- bytes
    field <- fieldForProperty property
    let width = maskWidth (fieldMask field)
    when (fromIntegral c >= (2 ^ width :: Int)) $
      Left ("payload: code " ++ show c ++ " does not fit " ++ fieldLabel field ++ " mask " ++ show (fieldMask field))
    let old = B.index current (fieldOffset field)
        keep = old .&. complement (fieldMask field)
        placed = (c `shiftL` fieldShift field) .&. fieldMask field
    pure (replaceAt current (fieldOffset field) (keep .|. placed))

-- | The zero-base form: every byte the recipe does not claim is zero, and the
-- storage kernel's code tables refuse zero for several fields, so a card payload
-- is built with 'encodeSlotPayloadOver' over the body defaults.
encodeSlotPayload :: Recipe -> Either String B.ByteString
encodeSlotPayload = encodeSlotPayloadOver (B.replicate payloadBytes 0)

maskWidth :: Word8 -> Int
maskWidth m = length [() | bit <- [0 .. 7 :: Int], m .&. (1 `shiftL` bit) /= 0]

replaceAt :: B.ByteString -> Int -> Word8 -> B.ByteString
replaceAt bytes i w = B.concat [B.take i bytes, B.singleton w, B.drop (i + 1) bytes]

-- | The count this build is sized for: the seven-slot conservative target
-- (recipe-design section 15).
bankCapacity :: Int
bankCapacity = 7

bankBlobBytes :: Int
bankBlobBytes = bankCapacity * (payloadBytes + nameBytes)

-- names -----------------------------------------------------------------

-- | A name is at most 12 UTF-16 units of printable ASCII, then a zero unit and
-- zero fill: 2 * (12 + 1) = 26 bytes.
nameUnits :: Int
nameUnits = 12

nameBytes :: Int
nameBytes = 2 * (nameUnits + 1)

encodeName :: String -> Either String B.ByteString
encodeName name
  | length name > nameUnits =
      Left ("name: " ++ show (length name) ++ " characters exceeds " ++ show nameUnits)
  | not (all printableAscii name) = Left "name: only printable ASCII 0x20-0x7e is admitted"
  | otherwise = pure (B.pack (concatMap unit name ++ replicate (2 * (nameUnits - length name) + 2) 0))
 where
  unit c = let v = ord c in [fromIntegral (v .&. 0xff), fromIntegral ((v `shiftR` 8) .&. 0xff)]

decodeName :: B.ByteString -> Either String String
decodeName bytes
  | B.length bytes /= nameBytes =
      Left ("name: expected " ++ show nameBytes ++ " bytes, got " ++ show (B.length bytes))
  | B.drop (2 * nameUnits) bytes /= B.replicate 2 0 = Left "name: terminator is not zero"
  | not (all printableAscii text) = Left ("name: non-printable unit in " ++ show text)
  | B.drop (2 * (length text + 1)) bytes /= B.replicate (nameBytes - 2 * (length text + 1)) 0 =
      Left "name: data after the terminator"
  | otherwise = pure text
 where
  text = go 0
  go k
    | k >= nameUnits = ""
    | lo == 0 && hi == 0 = ""
    | otherwise = toEnum (lo + 256 * hi) : go (k + 1)
   where
    lo = fromIntegral (B.index bytes (2 * k)) :: Int
    hi = fromIntegral (B.index bytes (2 * k + 1)) :: Int

printableAscii :: Char -> Bool
printableAscii c = c >= ' ' && c <= '~'

-- | The stock 66-byte special-record shape: 56 inline UTF-16 bytes then five
-- layout halfwords (special-label report section 3).
nameRecordBytes :: Int
nameRecordBytes = 0x42

nameRecordBufferBytes :: Int
nameRecordBufferBytes = 56

-- | The layout halfwords the stock id-0x83d record carries: ordinary menu
-- metrics, so a bank name inherits the ordinary item box with no layout edit.
nameRecordLayoutWords :: [Word16]
nameRecordLayoutWords = [0x14, 0x14, 0x1e, 0x24, 0x30]

-- | One 66-byte record for a bank name id: the inline name buffer plus the
-- layout halfwords. The rest of the buffer is zero fill.
encodeNameRecord :: String -> Either String B.ByteString
encodeNameRecord name = do
  buffer <- encodeName name
  let padded = B.take nameRecordBufferBytes (B.append buffer (B.replicate nameRecordBufferBytes 0))
  pure (B.append padded (B.pack (concatMap halfword nameRecordLayoutWords)))
 where
  halfword w = [fromIntegral (w .&. 0xff), fromIntegral ((w `shiftR` 8) .&. 0xff)]

decodeNameRecord :: B.ByteString -> Either String String
decodeNameRecord bytes
  | B.length bytes /= nameRecordBytes = Left "name record: wrong length"
  | otherwise = decodeName (B.take nameBytes (B.take nameRecordBufferBytes bytes))

-- | A label record's inline buffer carries up to 27 UTF-16 units plus its zero
-- terminator, which is what the stock id-0x83d record's buffer holds; the
-- 12-unit bound belongs to the slot-name path alone.
labelUnits :: Int
labelUnits = 27

-- | One 66-byte record for a label id: the same buffer and layout halfwords as
-- a name record, with the longer text bound.
encodeLabelRecord :: String -> Either String B.ByteString
encodeLabelRecord text
  | length text > labelUnits =
      Left ("label: " ++ show (length text) ++ " characters exceeds " ++ show labelUnits)
  | any ((> 0xffff) . fromEnum) text = Left "label: character outside the BMP"
  | otherwise =
      let buffer = B.concat (map (le16 . fromIntegral . fromEnum) text) <> le16 0
          padded = B.take nameRecordBufferBytes (B.append buffer (B.replicate nameRecordBufferBytes 0))
       in pure (B.append padded (B.pack (concatMap halfword nameRecordLayoutWords)))
 where
  halfword w = [fromIntegral (w .&. 0xff), fromIntegral ((w `shiftR` 8) .&. 0xff)]

-- menu arrays -----------------------------------------------------------

-- | The values array of a list: one {row, child} pair per row, then the
-- 0x7ffffffe terminator with a zero child. A child of 0 commits the row
-- in place; a child node address opens that node.
valuesTerminator :: Word32
valuesTerminator = 0x7ffffffe

encodeValuesArray :: [(Word32, Word32)] -> B.ByteString
encodeValuesArray rows = B.concat (map pair rows ++ [pair (valuesTerminator, 0)])
 where
  pair (row, child) = word32 row <> word32 child

-- | Appends rows to a stock values array, keeping its terminator last. The
-- stock array is taken byte-exact from the pinned image.
appendValuesPairs :: B.ByteString -> [(Word32, Word32)] -> Either String B.ByteString
appendValuesPairs stock rows
  | B.length stock < 8 = Left "values array: too short to hold a terminator"
  | B.take 4 tailWord /= word32 valuesTerminator || B.drop 4 tailWord /= word32 0 =
      Left "values array: {0x7ffffffe, 0} terminator not found at the array's end"
  | otherwise = pure (B.take (B.length stock - 8) stock <> encodeValuesArray rows)
 where
  tailWord = B.drop (B.length stock - 8) stock

-- | A list node header, the 0x2c-byte shape the pinned node map carries: the
-- node id, type 0, the property, kind 7 (a standard setting list), the two zero
-- words, f18 1, the parent, the values and labels pointers, and w28. A node
-- built here is addressed by the parent's values pair, so nothing walks it
-- sequentially.
encodeListNode :: Word32 -> Word16 -> Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
encodeListNode nodeId property parent values labels w28 =
  B.concat
    [ word32 nodeId
    , word32 0
    , word32 (fromIntegral property)
    , word32 7
    , word32 0
    , word32 0
    , word32 1
    , word32 parent
    , word32 values
    , word32 labels
    , word32 w28
    ]

-- | One 16-byte label quadruple {preview icon, list icon, help, name}, and the
-- whole array's 16-byte -1 terminator.
encodeLabelsArray :: [[Word32]] -> Either String B.ByteString
encodeLabelsArray quads = do
  mapM_ checkQuad quads
  pure (B.concat (map quad quads) <> B.replicate 16 0xff)
 where
  checkQuad q
    | length q /= 4 = Left "labels: every row needs exactly four string ids"
    | otherwise = pure ()
  quad = B.concat . map word32

appendLabelQuadruples :: B.ByteString -> [[Word32]] -> Either String B.ByteString
appendLabelQuadruples stock quads
  | B.length stock < 16 = Left "labels array: too short to hold a terminator"
  | B.drop (B.length stock - 16) stock /= B.replicate 16 0xff =
      Left "labels array: -1 terminator not found at the array's end"
  | otherwise = B.append (B.take (B.length stock - 16) stock) <$> encodeLabelsArray quads

-- | The parent list's two relocations: the extended values and labels arrays
-- land at the addresses the plan names, and the parent node's +0x20/+0x24
-- words are retargeted there (recipe-design section 17).
encodeParentAppend :: B.ByteString -> [(Word32, Word32)] -> B.ByteString -> [[Word32]] -> Either String (B.ByteString, B.ByteString)
encodeParentAppend stockValues rows stockLabels quads =
  (,)
    <$> appendValuesPairs stockValues rows
    <*> appendLabelQuadruples stockLabels quads

-- code words ------------------------------------------------------------

-- | A32 BL word, refusing an unreachable or unaligned target instead of
-- encoding it wrong. The imm24 operand counts instructions, so the byte delta
-- is bounded by -33554432 .. 33554428 and must divide by four.
armBranchWord :: Word32 -> Word32 -> Either String Word32
armBranchWord from to
  | delta `mod` 4 /= 0 =
      Left ("branch: target " ++ hex32 to ++ " is not word-aligned from " ++ hex32 from)
  | delta < -33554432 || delta > 33554428 =
      Left ("branch: target " ++ hex32 to ++ " out of A32 BL range from " ++ hex32 from)
  | otherwise = pure (0xeb000000 .|. (fromIntegral ((delta `shiftR` 2) .&. 0x00ffffff) :: Word32))
 where
  delta = toInteger to - toInteger from - 8 :: Integer

armTailBranchWord :: Word32 -> Word32 -> Either String Word32
armTailBranchWord from to = (.&. 0xfeffffff) <$> armBranchWord from to

-- | The two-halfword BL a plain-image Thumb caller uses; kept for the accessor
-- hooks when the site is Thumb.
thumbBranchWord :: Word32 -> Word32 -> Either String Word32
thumbBranchWord from to
  | delta < -16777216 || delta > 16777214 = Left "branch: target out of Thumb BL range"
  | otherwise = pure (lo .|. (hi `shiftL` 16))
 where
  delta = toInteger to - toInteger from - 4 :: Integer
  bitAt n = fromIntegral ((delta `shiftR` n) .&. 1) :: Word32
  s = bitAt 24
  i1 = bitAt 23
  i2 = bitAt 22
  imm10 = fromIntegral ((delta `shiftR` 12) .&. 0x3ff) :: Word32
  imm11 = fromIntegral ((delta `shiftR` 1) .&. 0x7ff) :: Word32
  lo = 0xf000 .|. (s `shiftL` 10) .|. imm10
  hi = 0xd000 .|. (i1 `shiftL` 13) .|. (i2 `shiftL` 11) .|. imm11

-- write plan ------------------------------------------------------------

-- | A region a plan writes. The capacity is what the validator checks writes
-- against, so a plan cannot exceed its placement silently.
data Region = Region
  { regionName :: String
  , regionStart :: Word32
  , regionBytes :: Int
  , regionNote :: String
  }
  deriving (Eq, Show)

-- | One write: current bytes are stated so an applier refuses a source that
-- does not match, the same rule the patch-set schema's words carry.
data PlanWrite = PlanWrite
  { writeRegion :: String
  , writeAddress :: Word32
  , writeCurrent :: B.ByteString
  , writeNew :: B.ByteString
  , writeRole :: String
  }
  deriving (Eq, Show)

data WritePlan = WritePlan
  { planRegions :: [Region]
  , planWrites :: [PlanWrite]
  , planNotes :: [String]
  }
  deriving (Eq, Show)

planNewBytes :: WritePlan -> Int
planNewBytes = sum . map (B.length . writeNew) . planWrites

-- | Writes have equal current/new lengths and disjoint runtime spans.
validatePlan :: WritePlan -> Either String ()
validatePlan plan = do
  when (null (planRegions plan)) (Left "plan: no regions declared")
  mapM_ (checkRegion (planRegions plan)) (planWrites plan)
  mapM_ checkLength (planWrites plan)
  mapM_ checkOverlap (zip (sorted) (drop 1 sorted))
 where
  sorted = sortOn writeAddress (planWrites plan)
  checkLength w
    | B.null (writeNew w) = Left ("plan: write " ++ writeRole w ++ " is empty")
    | B.length (writeCurrent w) /= B.length (writeNew w) = Left ("plan: write " ++ writeRole w ++ " changes span length")
    | otherwise = pure ()
  checkRegion regions w = case filter ((== writeRegion w) . regionName) regions of
    [] -> Left ("plan: write to unknown region " ++ writeRegion w)
    [r] ->
      let start = toInteger (regionStart r)
          end = start + toInteger (regionBytes r)
          wStart = toInteger (writeAddress w)
          wEnd = wStart + toInteger (B.length (writeNew w))
       in if wStart < start || wEnd > end
            then Left ("plan: write " ++ writeRole w ++ " at " ++ hex32 (writeAddress w) ++ " leaves region " ++ regionName r)
            else pure ()
    _ -> Left ("plan: duplicate region name " ++ writeRegion w)
  checkOverlap (a, b)
    | toInteger (writeAddress a) + toInteger (B.length (writeNew a)) <= toInteger (writeAddress b) = pure ()
    | otherwise = Left ("plan: writes " ++ writeRole a ++ " and " ++ writeRole b ++ " overlap")

newtype PlainLoadAddress = PlainLoadAddress Word32
  deriving (Eq, Show)

newtype PlainFlashOffset = PlainFlashOffset Word32
  deriving (Eq, Ord, Show)

validatePlainCarrier :: PlainLoadAddress -> (PlainFlashOffset, PlainFlashOffset) -> [(Region, PlainFlashOffset)] -> [PlanWrite] -> Either String ()
validatePlainCarrier (PlainLoadAddress load) (PlainFlashOffset start, PlainFlashOffset stop) mappings writes = do
  mapM_ checkMapping mappings
  spans <- mapM writeSpan [w | w <- writes, writeRegion w `elem` map (regionName . fst) mappings]
  let sortedSpans = sortOn (\(flash, _, _) -> flash) spans
  mapM_ checkOverlap (zip sortedSpans (drop 1 sortedSpans))
 where
  checkMapping (region, PlainFlashOffset flash)
    | regionStart region < load = Left ("carrier: region " ++ regionName region ++ " starts below the plain load address")
    | expected /= toInteger flash = Left ("carrier: region " ++ regionName region ++ " flash offset disagrees with the loader mapping")
    | otherwise = pure ()
   where
    expected = toInteger start + toInteger (regionStart region) - toInteger load
  writeSpan w = case filter ((== writeRegion w) . regionName . fst) mappings of
    [(region, PlainFlashOffset flash)] ->
      let at = toInteger flash + toInteger (writeAddress w) - toInteger (regionStart region)
          len = toInteger (B.length (writeNew w))
       in if len == 0
            then Left ("carrier: write " ++ writeRole w ++ " has an empty span")
            else if at < toInteger start || at > toInteger stop || len > toInteger stop - at
              then Left ("carrier: write " ++ writeRole w ++ " leaves the plain window")
              else pure (at, at + len, writeRole w)
    _ -> Left ("carrier: region " ++ writeRegion w ++ " has duplicate flash mappings")
  checkOverlap ((_, end, role), (next, _, nextRole))
    | end <= next = pure ()
    | otherwise = Left ("carrier: writes " ++ role ++ " and " ++ nextRole ++ " overlap in flash")

-- helpers ---------------------------------------------------------------

word32 :: Word32 -> B.ByteString
word32 w = B.pack [fromIntegral ((w `shiftR` s) .&. 0xff) | s <- [0, 8, 16, 24]]

hex32 :: Word32 -> String
hex32 w = "0x" ++ map nib [28, 24, 20, 16, 12, 8, 4, 0]
 where
  nib s = "0123456789abcdef" !! fromIntegral ((w `shiftR` s) .&. 0xf)

-- bank file -------------------------------------------------------------

-- | One slot as the SD file carries it: the full 0x28 payload bytes plus the
-- 26-byte name. The file stores the snapshot byte for byte and a read back
-- round-trips it verbatim; what the property walk leaves in the payload's
-- unbound bytes is not established here.
data FileSlot = FileSlot
  { fileSlotName :: String
  , fileSlotPayload :: B.ByteString
  }
  deriving (Eq, Show)

-- | The pad 'encodeFileBlob' uses when a caller passes fewer than 'bankCapacity'
-- slots. Its zero payload is not a card payload: the storage kernel's code
-- tables refuse zero for several fields, so a real card pads with the body
-- defaults through 'encodeSlotPayloadOver'.
emptyFileSlot :: FileSlot
emptyFileSlot = FileSlot "" (B.replicate payloadBytes 0)

-- | The unbound mask comes from the same generated table the storage kernel's
-- RB_UNBOUND_MASK does: writes zero only those bits and reads require them
-- zero. +0x0a bits 5-7 carry property 0xe9, which no walk carries, so they are
-- preserved.
normalizePayload :: B.ByteString -> B.ByteString
normalizePayload payload = B.pack (B.zipWith (.&.) payload (B.pack (map complement F.unboundMask)))

unboundBitsCanonical :: B.ByteString -> Bool
unboundBitsCanonical payload = all (== 0) (B.zipWith (.&.) payload (B.pack F.unboundMask))

-- | The bank blob from file slots: payloads back to back, then names. Slots
-- past the filled count are zero payloads and empty names, so the blob length
-- does not depend on the filled count.
encodeFileBlob :: [FileSlot] -> Either String B.ByteString
encodeFileBlob slots
  | length slots > bankCapacity =
      Left ("bank: " ++ show (length slots) ++ " slots exceed capacity " ++ show bankCapacity)
  | otherwise = do
      let full = slots ++ replicate (bankCapacity - length slots) emptyFileSlot
      payloads <- mapM checkPayload (map fileSlotPayload full)
      names <- mapM encodeName (map fileSlotName full)
      pure (B.concat payloads `B.append` B.concat names)
 where
  checkPayload bytes
    | B.length bytes /= payloadBytes =
        Left ("payload: expected " ++ show payloadBytes ++ " bytes, got " ++ show (B.length bytes))
    | otherwise = pure (normalizePayload bytes)

decodeFileBlob :: B.ByteString -> Either String [FileSlot]
decodeFileBlob bytes
  | B.length bytes /= bankBlobBytes =
      Left ("bank: expected " ++ show bankBlobBytes ++ " bytes, got " ++ show (B.length bytes))
  | otherwise = mapM slotAt [0 .. bankCapacity - 1]
 where
  nameBlock = B.drop (bankCapacity * payloadBytes) bytes
  slotAt i = do
    name <- decodeName (B.take nameBytes (B.drop (i * nameBytes) nameBlock))
    let payload = B.take payloadBytes (B.drop (i * payloadBytes) bytes)
    if unboundBitsCanonical payload
      then pure (FileSlot name payload)
      else Left ("bank: slot " ++ show i ++ " carries data in the payload's unbound bits")

-- | The bank file header: 32 typed bytes, then the blob, then zero fill to
-- 512. Two files per card carry successive generations.
data BankHeader = BankHeader
  { bankGeneration :: Word32
  , bankSlotValid :: Word16
  }
  deriving (Eq, Show)

headerBytes :: Int
headerBytes = 0x20

bankFileBytes :: Int
bankFileBytes = 512

bankFileA, bankFileB :: String
bankFileA = "RECIPEA.BIN"
bankFileB = "RECIPEB.BIN"

data BankFileId = FileA | FileB
  deriving (Eq, Show)

le16 :: Word16 -> B.ByteString
le16 w = B.pack [fromIntegral (w .&. 0xff), fromIntegral (w `shiftR` 8)]

-- | The container main-range convention (E-CHECKSUM): the complement of the
-- sum of the eight-bit complemented bytes.
bankChecksum :: B.ByteString -> Word32
bankChecksum bytes = complement (B.foldl' step 0 bytes)
 where
  step acc w = acc + fromIntegral (complement w :: Word8)

-- | The file checksum: the whole 512 bytes with the checksum field zeroed, so
-- generation and the slot mask are covered.
bankFileChecksum :: B.ByteString -> Word32
bankFileChecksum file =
  bankChecksum (B.take 0x18 file `B.append` B.replicate 4 0 `B.append` B.drop 0x1c file)

encodeBankFile :: BankHeader -> B.ByteString -> Either String B.ByteString
encodeBankFile (BankHeader gen mask) blob
  | B.length blob /= bankBlobBytes =
      Left ("bank file: blob is " ++ show (B.length blob) ++ " bytes, expected " ++ show bankBlobBytes)
  | mask >= 2 ^ bankCapacity =
      Left ("bank file: slot mask " ++ show mask ++ " exceeds " ++ show bankCapacity ++ " slots")
  | otherwise =
      let header =
            B.concat
              [ "FJRB"
              , le16 1
              , le16 (fromIntegral headerBytes)
              , le16 150
              , le16 (fromIntegral bankCapacity)
              , le16 (fromIntegral nameUnits)
              , le16 (fromIntegral payloadBytes)
              , word32 gen
              , word32 (fromIntegral bankBlobBytes)
              , word32 0
              , le16 mask
              , le16 0
              ]
          padded =
            header
              `B.append` blob
              `B.append` B.replicate (bankFileBytes - headerBytes - bankBlobBytes) 0
          checksum = bankFileChecksum padded
          file = B.concat [B.take 0x18 padded, word32 checksum, B.drop 0x1c padded]
       in pure file

decodeBankFile :: B.ByteString -> Either String (BankHeader, [FileSlot])
decodeBankFile bytes
  | B.length bytes /= bankFileBytes =
      Left ("bank file: expected " ++ show bankFileBytes ++ " bytes, got " ++ show (B.length bytes))
  | B.take 4 bytes /= "FJRB" = Left "bank file: bad magic"
  | le16At 4 /= 1 = Left "bank file: format version"
  | le16At 6 /= fromIntegral headerBytes = Left "bank file: header length"
  | le16At 8 /= 150 = Left "bank file: firmware tag"
  | le16At 10 /= fromIntegral bankCapacity = Left "bank file: slot count"
  | le16At 12 /= fromIntegral nameUnits = Left "bank file: name units"
  | le16At 14 /= fromIntegral payloadBytes = Left "bank file: payload length"
  | le32At 20 /= fromIntegral bankBlobBytes = Left "bank file: blob length"
  | le16At 30 /= 0 = Left "bank file: reserved word"
  | mask >= 2 ^ bankCapacity = Left "bank file: slot mask"
  | B.drop (headerBytes + bankBlobBytes) bytes /= B.replicate (bankFileBytes - headerBytes - bankBlobBytes) 0 =
      Left "bank file: trailing padding is not zero"
  | bankFileChecksum bytes /= le32At 24 = Left "bank file: checksum"
  | otherwise = (,) (BankHeader (le32At 16) mask) <$> decodeFileBlob blob
 where
  mask = fromIntegral (le16At 28) :: Word16
  blob = B.take bankBlobBytes (B.drop headerBytes bytes)
  le16At :: Int -> Word32
  le16At o = fromIntegral (B.index bytes o) .|. (fromIntegral (B.index bytes (o + 1)) `shiftL` 8)
  le32At :: Int -> Word32
  le32At o = foldl' (.|.) 0 [fromIntegral (B.index bytes (o + i)) `shiftL` (8 * i) | i <- [0 .. 3 :: Int]]

-- | Load precedence: the valid file with the higher generation; a tie goes to
-- A. The write target is the file not holding the winner, and A when neither
-- is valid, so a write that dies leaves the previous bank loadable.
pickCurrentFile :: Maybe Word32 -> Maybe Word32 -> Maybe BankFileId
pickCurrentFile genA genB = case (genA, genB) of
  (Just a, Just b) -> Just (if a >= b then FileA else FileB)
  (Just _, Nothing) -> Just FileA
  (Nothing, Just _) -> Just FileB
  (Nothing, Nothing) -> Nothing

chooseWriteTarget :: Maybe Word32 -> Maybe Word32 -> BankFileId
chooseWriteTarget genA genB = case pickCurrentFile genA genB of
  Nothing -> FileA
  Just FileA -> FileB
  Just FileB -> FileA

nextGeneration :: Maybe Word32 -> Maybe Word32 -> Either String Word32
nextGeneration genA genB =
  let top = maximum (0 : [g | Just g <- [genA, genB]])
   in if top == 0xffffffff
        then Left "bank file: generation exhausted"
        else Right (top + 1)
