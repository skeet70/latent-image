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
  , slotRecipe
  , payloadBytes
  , bankCapacity
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
    -- * Bank state
  , BankSlot (..)
  , bankSlots
  , Selection (..)
  , AutoUpdate (..)
  , EligibleMode (..)
  , BankPayload
  , bankPayload
  , canonicalBankPayload
  , bankPayloadContents
  , SlotName
  , slotName
  , slotNameText
  , BankBase (..)
  , BankRecord (..)
  , BankRecords
  , bankRecordsFromList
  , bankRecordList
  , bankRecordAt
  , BankGeneration (..)
  , BankState (..)
  , validateBankState
    -- * Bank file
  , bankFileBytes
  , bankFileA
  , bankFileB
  , BankFileId (..)
  , bankChecksum
  , encodeBankFile
  , V1Slot (..)
  , V1Bank
  , v1Generation
  , v1Slots
  , migrateV1
  , DecodedBank (..)
  , decodedGeneration
  , decodedState
  , decodeBankFile
    -- * Payload safety
  , FieldCode (..)
  , SizePair (..)
  , QualityRaw (..)
  , PayloadDomain (..)
  , payloadDomainFromImage4
  , payloadUnsafeReason
  , withImageSizeRow
  , BankFileClass (..)
  , classifyBankFile
    -- * Choosing the bank
  , BankOutcome (..)
  , chooseBank
  , WriteRefusal (..)
  , NextWrite (..)
  , nextWrite
  ) where

import Control.Monad (forM, unless, when)
import Data.Bits (complement, shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (ord)
import Data.List (sort, sortOn)
import Data.Maybe (catMaybes, fromMaybe, isNothing, listToMaybe)
import Data.Ord (Down (..))
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

-- | The stored codes a payload carries at the nine applied fields: the read
-- direction of 'encodeSlotPayloadOver' placement.
slotRecipe :: B.ByteString -> Either String Recipe
slotRecipe bytes
  | B.length bytes /= payloadBytes =
      Left ("payload: " ++ show (B.length bytes) ++ " bytes, expected " ++ show payloadBytes)
  | otherwise = recipeFromPairs [(fieldProperty f, codeAt f) | f <- recipeFields]
 where
  codeAt f = StoredCode ((B.index bytes (fieldOffset f) .&. fieldMask f) `shiftR` fieldShift f)

maskWidth :: Word8 -> Int
maskWidth m = length [() | bit <- [0 .. 7 :: Int], m .&. (1 `shiftL` bit) /= 0]

replaceAt :: B.ByteString -> Int -> Word8 -> B.ByteString
replaceAt bytes i w = B.concat [B.take i bytes, B.singleton w, B.drop (i + 1) bytes]

-- | The count this build is sized for: the seven-slot conservative target
-- (recipe-design section 15).
bankCapacity :: Int
bankCapacity = 7

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

-- bank state ------------------------------------------------------------

-- | A custom bank; the stock C detent is not one of these.
data BankSlot = C1 | C2 | C3 | C4 | C5 | C6 | C7
  deriving (Eq, Ord, Show, Enum, Bounded)

bankSlots :: [BankSlot]
bankSlots = [minBound .. maxBound]

data Selection = NothingSelected | Custom BankSlot
  deriving (Eq, Show)

data AutoUpdate = Disabled | Enabled
  deriving (Eq, Show)

-- | The shooting mode a base snapshot was captured in.
data EligibleMode = P | S | A | M
  deriving (Eq, Show, Enum, Bounded)

-- | A 0x28 payload whose unbound bits are zero.
newtype BankPayload = BankPayload B.ByteString
  deriving (Eq, Show)

-- | The unbound mask is the generated one the storage kernel's
-- RB_UNBOUND_MASK carries; +0x0a bits 5-7 carry property 0xe9 and are bound.
bankPayload :: B.ByteString -> Either String BankPayload
bankPayload bytes
  | B.length bytes /= payloadBytes =
      Left ("payload: expected " ++ show payloadBytes ++ " bytes, got " ++ show (B.length bytes))
  | not (all (== 0) (B.zipWith (.&.) bytes (B.pack F.unboundMask))) =
      Left "payload: data in the unbound bits"
  | otherwise = pure (BankPayload bytes)

-- | The payload with its unbound bits cleared, as a write stores it.
canonicalBankPayload :: B.ByteString -> Either String BankPayload
canonicalBankPayload bytes
  | B.length bytes /= payloadBytes = bankPayload bytes
  | otherwise = bankPayload (B.pack (B.zipWith (.&.) bytes (B.pack (map complement F.unboundMask))))

bankPayloadContents :: BankPayload -> B.ByteString
bankPayloadContents (BankPayload bytes) = bytes

-- | A name 'encodeName' admits.
newtype SlotName = SlotName String
  deriving (Eq, Show)

slotName :: String -> Either String SlotName
slotName text = SlotName text <$ encodeName text

slotNameText :: SlotName -> String
slotNameText (SlotName text) = text

data BankBase = BaseInvalid | BaseValid EligibleMode BankPayload
  deriving (Eq, Show)

-- | A populated record: name, saved payload, working payload.
data BankRecord = Unpopulated | Populated SlotName BankPayload BankPayload
  deriving (Eq, Show)

-- | Exactly 'bankCapacity' records, C1 first.
newtype BankRecords = BankRecords [BankRecord]
  deriving (Eq, Show)

bankRecordsFromList :: [BankRecord] -> Either String BankRecords
bankRecordsFromList records
  | length records /= bankCapacity =
      Left ("bank: " ++ show (length records) ++ " records, expected " ++ show bankCapacity)
  | otherwise = pure (BankRecords records)

bankRecordList :: BankRecords -> [BankRecord]
bankRecordList (BankRecords records) = records

bankRecordAt :: BankSlot -> BankRecords -> BankRecord
bankRecordAt slot (BankRecords records) = records !! fromEnum slot

newtype BankGeneration = BankGeneration Word32
  deriving (Eq, Ord, Show)

data BankState = BankState
  { bankGeneration :: BankGeneration
  , bankSelection :: Selection
  , bankAutoUpdate :: AutoUpdate
  , bankBase :: BankBase
  , bankRecords :: BankRecords
  }
  deriving (Eq, Show)

-- | The state-level structural rules: a custom selection names a populated
-- slot and has a valid base, and ENABLE has working equal to saved in every
-- populated record.
validateBankState :: BankState -> Either String ()
validateBankState st = do
  case bankSelection st of
    NothingSelected -> pure ()
    Custom selected -> do
      when (bankRecordAt selected (bankRecords st) == Unpopulated) $
        Left ("bank: selection " ++ show selected ++ " names an unpopulated slot")
      when (bankBase st == BaseInvalid) $
        Left ("bank: selection " ++ show selected ++ " without a valid base")
  when (bankAutoUpdate st == Enabled) $
    sequence_
      [ Left ("bank: ENABLE with working differing from saved in " ++ show slot)
      | (slot, Populated _ saved working) <- zip bankSlots (bankRecordList (bankRecords st))
      , saved /= working
      ]

-- bank file -------------------------------------------------------------

bankFileBytes :: Int
bankFileBytes = 1024

v1FileBytes :: Int
v1FileBytes = 512

bankFileA, bankFileB :: String
bankFileA = "RECIPEA.BIN"
bankFileB = "RECIPEB.BIN"

data BankFileId = FileA | FileB
  deriving (Eq, Ord, Show)

firmwareTag :: Word32
firmwareTag = 150

-- | The header fields that differ between the two versions.
data FileLayout = FileLayout
  { layoutVersion :: Word32
  , layoutHeader :: Int
  , layoutBody :: Int
  }

v1Layout, v2Layout :: FileLayout
v1Layout = FileLayout 1 0x20 (bankCapacity * (payloadBytes + nameBytes))
v2Layout = FileLayout 2 0x40 (payloadBytes + bankCapacity * recordBytes)

-- | One v2 record: name, saved payload, working payload.
recordBytes :: Int
recordBytes = nameBytes + 2 * payloadBytes

v2RecordsOffset :: Int
v2RecordsOffset = layoutHeader v2Layout + payloadBytes

le16 :: Word16 -> B.ByteString
le16 w = B.pack [fromIntegral (w .&. 0xff), fromIntegral (w `shiftR` 8)]

le16At :: B.ByteString -> Int -> Word32
le16At bytes o = fromIntegral (B.index bytes o) .|. (fromIntegral (B.index bytes (o + 1)) `shiftL` 8)

le32At :: B.ByteString -> Int -> Word32
le32At bytes o = foldl' (.|.) 0 [fromIntegral (B.index bytes (o + i)) `shiftL` (8 * i) | i <- [0 .. 3 :: Int]]

-- | The container main-range convention (E-CHECKSUM): the complement of the
-- sum of the eight-bit complemented bytes.
bankChecksum :: B.ByteString -> Word32
bankChecksum bytes = complement (B.foldl' step 0 bytes)
 where
  step acc w = acc + fromIntegral (complement w :: Word8)

-- | The whole file with the checksum field 0x18..0x1b zeroed.
bankFileChecksum :: B.ByteString -> Word32
bankFileChecksum file =
  bankChecksum (B.take 0x18 file `B.append` B.replicate 4 0 `B.append` B.drop 0x1c file)

-- | The v2 file of a state; a state 'validateBankState' refuses is refused.
encodeBankFile :: BankState -> Either String B.ByteString
encodeBankFile st = do
  validateBankState st
  records <- mapM encodeRecord (bankRecordList (bankRecords st))
  let BankGeneration gen = bankGeneration st
      (baseValid, baseMode, basePayload) = case bankBase st of
        BaseInvalid -> (0, 0, B.replicate payloadBytes 0)
        BaseValid mode payload -> (1, fromIntegral (fromEnum mode + 1), bankPayloadContents payload)
      header =
        B.concat
          [ "FJRB"
          , le16 (fromIntegral (layoutVersion v2Layout))
          , le16 (fromIntegral (layoutHeader v2Layout))
          , le16 (fromIntegral firmwareTag)
          , le16 (fromIntegral bankCapacity)
          , le16 (fromIntegral nameUnits)
          , le16 (fromIntegral payloadBytes)
          , word32 gen
          , word32 (fromIntegral (layoutBody v2Layout))
          , word32 0
          , le16 (populatedMask (bankRecordList (bankRecords st)))
          , le16 0
          , B.pack [selectionByte (bankSelection st), autoUpdateByte (bankAutoUpdate st), baseValid, baseMode]
          , B.replicate (layoutHeader v2Layout - 0x24) 0
          ]
      body = B.concat (header : basePayload : records)
      file = body `B.append` B.replicate (bankFileBytes - B.length body) 0
  pure (B.concat [B.take 0x18 file, word32 (bankFileChecksum file), B.drop 0x1c file])
 where
  encodeRecord Unpopulated = pure (B.replicate recordBytes 0)
  encodeRecord (Populated (SlotName text) saved working) = do
    encoded <- encodeName text
    pure (B.concat [encoded, bankPayloadContents saved, bankPayloadContents working])
  selectionByte NothingSelected = 0
  selectionByte (Custom slot) = fromIntegral (fromEnum slot + 1)
  autoUpdateByte Disabled = 0
  autoUpdateByte Enabled = 1

populatedMask :: [BankRecord] -> Word16
populatedMask records = foldl' (.|.) 0 [1 `shiftL` k | (k, Populated {}) <- zip [0 ..] records]

-- | A v1 slot as migration reads it; an unpopulated slot's bytes are dropped.
data V1Slot = V1Unpopulated | V1Populated SlotName BankPayload
  deriving (Eq, Show)

-- | A decoded 512-byte v1 file: its generation and 'bankCapacity' slots.
data V1Bank = V1Bank
  { v1Generation :: BankGeneration
  , v1Slots :: [V1Slot]
  }
  deriving (Eq, Show)

-- | The resident state of a v1 winner: working = saved, NOTHING SELECTED,
-- DISABLE, base invalid, the v1 generation.
migrateV1 :: V1Bank -> BankState
migrateV1 v1 =
  BankState
    { bankGeneration = v1Generation v1
    , bankSelection = NothingSelected
    , bankAutoUpdate = Disabled
    , bankBase = BaseInvalid
    , bankRecords = BankRecords (map migrated (v1Slots v1))
    }
 where
  migrated V1Unpopulated = Unpopulated
  migrated (V1Populated named payload) = Populated named payload payload

data DecodedBank = DecodedV1 V1Bank | DecodedV2 BankState
  deriving (Eq, Show)

decodedGeneration :: DecodedBank -> BankGeneration
decodedGeneration (DecodedV1 v1) = v1Generation v1
decodedGeneration (DecodedV2 st) = bankGeneration st

-- | The resident state a decoded file loads as; a v1 file migrates.
decodedState :: DecodedBank -> BankState
decodedState (DecodedV1 v1) = migrateV1 v1
decodedState (DecodedV2 st) = st

-- | The structural rules of either version; the length selects the version.
decodeBankFile :: B.ByteString -> Either String DecodedBank
decodeBankFile bytes
  | B.length bytes == bankFileBytes = DecodedV2 <$> decodeV2 bytes
  | B.length bytes == v1FileBytes = DecodedV1 <$> decodeV1 bytes
  | otherwise =
      Left ("bank file: " ++ show (B.length bytes) ++ " bytes, expected " ++ show v1FileBytes ++ " or " ++ show bankFileBytes)

-- | The header, padding and checksum rules; returns the populated mask.
checkHeader :: FileLayout -> B.ByteString -> Either String Word32
checkHeader layout bytes = do
  when (B.take 4 bytes /= "FJRB") $ Left "bank file: bad magic"
  when (le16At bytes 4 /= layoutVersion layout) $ Left "bank file: format version"
  when (le16At bytes 6 /= fromIntegral (layoutHeader layout)) $ Left "bank file: header length"
  when (le16At bytes 8 /= firmwareTag) $ Left "bank file: firmware tag"
  when (le16At bytes 10 /= fromIntegral bankCapacity) $ Left "bank file: slot count"
  when (le16At bytes 12 /= fromIntegral nameUnits) $ Left "bank file: name units"
  when (le16At bytes 14 /= fromIntegral payloadBytes) $ Left "bank file: payload length"
  when (le32At bytes 20 /= fromIntegral (layoutBody layout)) $ Left "bank file: body length"
  when (le16At bytes 30 /= 0) $ Left "bank file: reserved halfword"
  when (populated >= 2 ^ bankCapacity) $ Left "bank file: populated mask"
  unless (B.all (== 0) (B.drop (layoutHeader layout + layoutBody layout) bytes)) $
    Left "bank file: trailing padding is not zero"
  when (bankFileChecksum bytes /= le32At bytes 24) $ Left "bank file: checksum"
  pure populated
 where
  populated = le16At bytes 28

inSlot :: Int -> Either String a -> Either String a
inSlot k = either (Left . (("bank file: slot " ++ show k ++ ": ") ++)) Right

decodeV1 :: B.ByteString -> Either String V1Bank
decodeV1 bytes = do
  populated <- checkHeader v1Layout bytes
  slots <- forM [0 .. bankCapacity - 1] $ \k -> do
    text <- inSlot k (decodeName (slice (nameBlock + k * nameBytes) nameBytes))
    payload <- inSlot k (bankPayload (slice (layoutHeader v1Layout + k * payloadBytes) payloadBytes))
    pure (if testBit populated k then V1Populated (SlotName text) payload else V1Unpopulated)
  pure (V1Bank (BankGeneration (le32At bytes 16)) slots)
 where
  nameBlock = layoutHeader v1Layout + bankCapacity * payloadBytes
  slice o n = B.take n (B.drop o bytes)

decodeV2 :: B.ByteString -> Either String BankState
decodeV2 bytes = do
  populated <- checkHeader v2Layout bytes
  selection <- case B.index bytes 0x20 of
    0 -> pure NothingSelected
    n
      | n <= 7 -> pure (Custom (toEnum (fromIntegral n - 1)))
      | otherwise -> Left ("bank file: selection byte " ++ show n)
  autoUpdate <- case B.index bytes 0x21 of
    0 -> pure Disabled
    1 -> pure Enabled
    n -> Left ("bank file: auto-update byte " ++ show n)
  unless (B.all (== 0) (slice 0x24 (layoutHeader v2Layout - 0x24))) $
    Left "bank file: reserved bytes 0x24..0x3f are not zero"
  let basePayload = slice (layoutHeader v2Layout) payloadBytes
  base <- case (B.index bytes 0x22, B.index bytes 0x23) of
    (0, 0)
      | B.all (== 0) basePayload -> pure BaseInvalid
      | otherwise -> Left "bank file: invalid base with a nonzero payload"
    (0, mode) -> Left ("bank file: invalid base with capture mode " ++ show mode)
    (1, mode)
      | mode >= 1 && mode <= 4 ->
          BaseValid (toEnum (fromIntegral mode - 1))
            <$> either (Left . ("bank file: base: " ++)) Right (bankPayload basePayload)
      | otherwise -> Left ("bank file: base capture mode " ++ show mode)
    (valid, _) -> Left ("bank file: base valid byte " ++ show valid)
  records <- forM [0 .. bankCapacity - 1] $ \k -> do
    let record = slice (v2RecordsOffset + k * recordBytes) recordBytes
    if testBit populated k
      then
        Populated
          <$> (SlotName <$> inSlot k (decodeName (B.take nameBytes record)))
          <*> inSlot k (bankPayload (B.take payloadBytes (B.drop nameBytes record)))
          <*> inSlot k (bankPayload (B.drop (nameBytes + payloadBytes) record))
      else
        if B.all (== 0) record
          then pure Unpopulated
          else Left ("bank file: unpopulated slot " ++ show k ++ " is not zero")
  let st = BankState (BankGeneration (le32At bytes 16)) selection autoUpdate base (BankRecords records)
  validateBankState st
  pure st
 where
  slice o n = B.take n (B.drop o bytes)

-- payload safety --------------------------------------------------------

-- | A walked field's value in its conversion table's col0 terms.
newtype FieldCode = FieldCode Word32
  deriving (Eq, Ord, Show)

-- | One IMAGE SIZE row's (0xf6, 0xf9) entry in 0x007a00cc.
data SizePair = SizePair
  { sizePairSize :: FieldCode
  , sizePairAspect :: FieldCode
  }
  deriving (Eq, Show)

-- | An IMAGE QUALITY code and the 0x1d code its 0xfa arm writes.
data QualityRaw = QualityRaw
  { qualityRawQuality :: FieldCode
  , qualityRawRaw :: FieldCode
  }
  deriving (Eq, Show)

-- | The payload values a stock menu produces: per walked property with a
-- conversion table the codes that table holds, the offered IMAGE SIZE pairs,
-- and the IMAGE QUALITY codes with their RAW codes.
data PayloadDomain = PayloadDomain
  { domainTables :: [(Word16, [FieldCode])]
  , domainSizePairs :: [SizePair]
  , domainQualityRaw :: [QualityRaw]
  }
  deriving (Eq, Show)

propSize, propAspect, propQuality, propRaw :: Word16
propSize = 0xf6
propAspect = 0xf9
propQuality = 0xfa
propRaw = 0x1d

tablePointerBlock :: Word32
tablePointerBlock = 0x0081b09c

sizePairTable :: Word32
sizePairTable = 0x007a00cc

imageSizeOfferedRows :: [Int]
imageSizeOfferedRows = [1, 2, 3, 5, 6, 7, 9, 0xa, 0xb]

-- | The RAW cell the 0xfa arm sets per IMAGE QUALITY row 0-4.
qualityRawCells :: [Word32]
qualityRawCells = [0, 0, 2, 2, 1]

-- | The domain out of an image-4 decode loaded at `base`: a table pointer per
-- walked property at 0x0081b09c + 4 * property (refused when it differs from
-- the generated field map), the offered rows' pairs at 0x007a00cc + 16 * row,
-- and per IMAGE QUALITY row the first 0x1d row whose effective column is that
-- row's RAW cell.
payloadDomainFromImage4 :: Word32 -> B.ByteString -> Either String PayloadDomain
payloadDomainFromImage4 base image = do
  tables <- fmap catMaybes . forM F.payloadFields $ \f -> do
    pointer <- wordAt (tablePointerBlock + 4 * fromIntegral (F.fieldProperty f))
    when (pointer /= F.fieldTable f) $
      Left
        ( "domain: property "
            ++ show (F.fieldProperty f)
            ++ " table pointer "
            ++ hex32 pointer
            ++ " differs from the field map's "
            ++ hex32 (F.fieldTable f)
        )
    if pointer == 0
      then pure Nothing
      else Just . (,) (F.fieldProperty f) <$> rowsAt pointer 0
  let tableOf p = maybe (Left ("domain: property " ++ show p ++ " has no conversion table")) pure (lookup p tables)
  sizeRows <- tableOf propSize
  pairs <- forM imageSizeOfferedRows $ \row -> do
    when (row >= length sizeRows) $
      Left ("domain: IMAGE SIZE row " ++ show row ++ " is past its table")
    let at = sizePairTable + 16 * fromIntegral row
    SizePair <$> (FieldCode <$> wordAt at) <*> (FieldCode <$> wordAt (at + 4))
  qualityRows <- tableOf propQuality
  rawRows <- tableOf propRaw
  pure
    PayloadDomain
      { domainTables = [(p, map fst rows) | (p, rows) <- tables]
      , domainSizePairs = pairs
      , domainQualityRaw =
          [ QualityRaw quality raw
          | ((quality, _), cell) <- zip qualityRows qualityRawCells
          , Just raw <- [lookup cell [(effective, code) | (code, effective) <- rawRows]]
          ]
      }
 where
  wordAt address
    | address < base || toInteger (address - base) + 4 > toInteger (B.length image) =
        Left ("domain: " ++ hex32 address ++ " is outside the image")
    | otherwise = pure (le32At image (fromIntegral (address - base)))
  rowsAt :: Word32 -> Word32 -> Either String [(FieldCode, Word32)]
  rowsAt address k = do
    code <- wordAt (address + 12 * k)
    if code == 0xffff
      then pure []
      else do
        effective <- wordAt (address + 12 * k + 8)
        ((FieldCode code, effective) :) <$> rowsAt address (k + 1)

walkedField :: Word16 -> Maybe F.PayloadField
walkedField p = listToMaybe [f | f <- F.payloadFields, F.fieldProperty f == p]

fieldCode :: B.ByteString -> F.PayloadField -> FieldCode
fieldCode bytes f = FieldCode ((stored .&. fromIntegral (F.fieldMask f)) `shiftR` fromIntegral (F.fieldShift f))
 where
  at = fromIntegral (F.fieldOffset f)
  stored
    | F.fieldBytes f == 2 = le16At bytes at
    | otherwise = fromIntegral (B.index bytes at)

-- | Why a payload is one no stock menu produces: a walked code its table does
-- not hold, an unoffered IMAGE SIZE pair, or a 0x1d code other than the one
-- its IMAGE QUALITY row writes.
payloadUnsafeReason :: PayloadDomain -> BankPayload -> Maybe String
payloadUnsafeReason domain (BankPayload bytes) =
  listToMaybe (catMaybes (map tableReason F.payloadFields ++ [sizeReason, qualityReason]))
 where
  codeOf p = fieldCode bytes <$> walkedField p
  tableReason f = case lookup (F.fieldProperty f) (domainTables domain) of
    Just codes
      | fieldCode bytes f `notElem` codes ->
          Just ("property " ++ show (F.fieldProperty f) ++ " " ++ show (fieldCode bytes f) ++ " is not a row of its table")
    _ -> Nothing
  sizeReason = case (codeOf propSize, codeOf propAspect) of
    (Just size, Just aspect) | SizePair size aspect `elem` domainSizePairs domain -> Nothing
    pair -> Just ("IMAGE SIZE pair " ++ show pair ++ " is not an offered row")
  qualityReason = case (codeOf propQuality, codeOf propRaw) of
    (Just quality, Just raw)
      | lookup quality [(qualityRawQuality q, qualityRawRaw q) | q <- domainQualityRaw domain] == Just raw -> Nothing
    pair -> Just ("IMAGE QUALITY/RAW pair " ++ show pair ++ " is not one the 0xfa arm writes")

-- | Places the size and aspect codes of offered IMAGE SIZE menu row `row`.
withImageSizeRow :: PayloadDomain -> Int -> B.ByteString -> Either String B.ByteString
withImageSizeRow domain row bytes = case lookup row (zip imageSizeOfferedRows (domainSizePairs domain)) of
  Nothing -> Left ("IMAGE SIZE row " ++ show row ++ " is not offered")
  Just (SizePair size aspect) -> place propAspect aspect =<< place propSize size bytes
 where
  place p (FieldCode c) current = case walkedField p of
    Just f | F.fieldBytes f == 1, fromIntegral (F.fieldOffset f) < B.length current -> do
      let at = fromIntegral (F.fieldOffset f)
          mask = fromIntegral (F.fieldMask f) :: Word8
          placed = (fromIntegral c `shiftL` fromIntegral (F.fieldShift f)) .&. mask
      when (placed `shiftR` fromIntegral (F.fieldShift f) /= fromIntegral c) $
        Left ("IMAGE SIZE row " ++ show row ++ ": code " ++ show c ++ " does not fit property " ++ show p)
      pure (replaceAt current at ((B.index current at .&. complement mask) .|. placed))
    _ -> Left ("IMAGE SIZE row " ++ show row ++ ": property " ++ show p ++ " is not a one-byte walked field")

-- | v1: the populated slot payloads; v2: the valid base and both payloads of
-- every populated record.
decodedUnsafeReason :: PayloadDomain -> DecodedBank -> Maybe String
decodedUnsafeReason domain decoded =
  listToMaybe (catMaybes [(place ++) <$> payloadUnsafeReason domain p | (place, p) <- payloads])
 where
  payloads = case decoded of
    DecodedV1 v1 -> [("slot " ++ show k ++ ": ", p) | (k, V1Populated _ p) <- zip [0 :: Int ..] (v1Slots v1)]
    DecodedV2 st ->
      [("base: ", p) | BaseValid _ p <- [bankBase st]]
        ++ concat
          [ [(show slot ++ " saved: ", saved), (show slot ++ " working: ", working)]
          | (slot, Populated _ saved working) <- zip bankSlots (bankRecordList (bankRecords st))
          ]

data BankFileClass
  = StructurallyInvalid String
  | UnsafeFile DecodedBank String
  | ValidFile DecodedBank
  deriving (Eq, Show)

classifyBankFile :: PayloadDomain -> B.ByteString -> BankFileClass
classifyBankFile domain bytes = case decodeBankFile bytes of
  Left reason -> StructurallyInvalid reason
  Right decoded -> maybe (ValidFile decoded) (UnsafeFile decoded) (decodedUnsafeReason domain decoded)

-- choosing the bank -----------------------------------------------------

data BankOutcome
  = BankWinner BankFileId BankState
  | CannotMigrate BankFileId
  | NoBank
  | NoCandidate
  deriving (Eq, Show)

-- | Valid and unsafe files ordered by generation, A first on a tie: an unsafe
-- first file blocks the bank, a valid one wins. `Nothing` is a missing file;
-- files present with no candidate among them are 'NoCandidate'.
chooseBank :: Maybe BankFileClass -> Maybe BankFileClass -> BankOutcome
chooseBank fileA fileB = case candidates of
  (fid, _, UnsafeFile _ _) : _ -> CannotMigrate fid
  (fid, _, ValidFile decoded) : _ -> BankWinner fid (decodedState decoded)
  _
    | isNothing fileA && isNothing fileB -> NoBank
    | otherwise -> NoCandidate
 where
  candidates =
    sortOn
      (\(fid, generation, _) -> (Down generation, fid))
      [ (fid, decodedGeneration found, cls)
      | (fid, Just cls) <- [(FileA, fileA), (FileB, fileB)]
      , Just found <- [candidateOf cls]
      ]
  candidateOf (ValidFile found) = Just found
  candidateOf (UnsafeFile found _) = Just found
  candidateOf (StructurallyInvalid _) = Nothing

data WriteRefusal = BankCannotMigrate | GenerationExhausted
  deriving (Eq, Show)

data NextWrite = NextWrite
  { nextWriteTarget :: BankFileId
  , nextWriteGeneration :: BankGeneration
  }
  deriving (Eq, Show)

-- | The nonwinner at winner + 1, with no wrap past 0xffffffff; file A at
-- generation 1 when there is no winner.
nextWrite :: BankOutcome -> Either WriteRefusal NextWrite
nextWrite outcome = case outcome of
  BankWinner FileA st -> NextWrite FileB <$> successor (bankGeneration st)
  BankWinner FileB st -> NextWrite FileA <$> successor (bankGeneration st)
  CannotMigrate _ -> Left BankCannotMigrate
  NoBank -> pure (NextWrite FileA (BankGeneration 1))
  NoCandidate -> pure (NextWrite FileA (BankGeneration 1))
 where
  successor (BankGeneration g)
    | g == maxBound = Left GenerationExhausted
    | otherwise = pure (BankGeneration (g + 1))
