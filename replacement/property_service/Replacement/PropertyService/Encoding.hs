{-# LANGUAGE GHC2021 #-}

-- | The packed form of the owned service's tables, shared by the generator, which writes them, and
-- admission, which reads them back from the linked image: one word per property, one word per slot
-- arm in a second table and two words per special field in a third, both indexed from the property word.
module Replacement.PropertyService.Encoding
  ( EncodedEntry (..)
  , EncodedSlot (..)
  , EncodedSpecial (..)
  , SlotIndex (..)
  , SpecialIndex (..)
  , SpecialArm (..)
  , TableEntry (..)
  , EncodedTables (..)
  , EncodingError (..)
  , encodeEntry
  , encodeSlot
  , encodeSpecial
  , encodeTables
  , stockCode
  , encodingDescription
  ) where

import Data.Bits (shiftL, (.|.))
import Data.Word (Word32)
import Replacement.PropertyService.Classify

newtype EncodedEntry = EncodedEntry Word32 deriving (Eq, Show)

newtype EncodedSlot = EncodedSlot Word32 deriving (Eq, Show)

data EncodedSpecial = EncodedSpecial Word32 Word32 deriving (Eq, Show)

-- | Position of a slot arm's word in the slot table, below `slotTableCapacity`.
newtype SlotIndex = SlotIndex Word32 deriving (Eq, Ord, Show)

-- | Position of a special field's words in the special table, below `specialTableCapacity`.
newtype SpecialIndex = SpecialIndex Word32 deriving (Eq, Ord, Show)

-- | A property the property word cannot hold: a field with a stock or configuration default or a
-- hook, a read-only field, or a write-only pair.
data SpecialArm = HookedField FieldArm | ReadOnlyField FieldArm | WriteOnlyPair PairArm deriving (Eq, Show)

data TableEntry = UnassignedEntry | FieldEntry FieldArm | SlotEntry BankBranch MirrorPolicy SlotIndex | SpecialEntry SpecialIndex deriving (Eq, Show)

data EncodedTables = EncodedTables { encodedEntries :: [EncodedEntry], encodedSlots :: [EncodedSlot], encodedSpecials :: [EncodedSpecial] } deriving (Eq, Show)

-- | `DelegatedProperty`: the classification delegates a property, which no word encodes.
data EncodingError = DelegatedProperty | SlotTableFull | SpecialTableFull | SpecialDefaultUnencodable deriving (Eq, Show)

-- | The slot index has 7 bits.
slotTableCapacity :: Int
slotTableCapacity = 128

-- | The special index has 6 bits, and 0 is no entry.
specialTableCapacity :: Int
specialTableCapacity = 63

encodingDescription :: String
encodingDescription =
  "property word: bits 0-1 kind (0 special, 1 unassigned, 2 field, 3 slot field); kind 0 has bits 2-7 the special-table index + 1, never 0, and is otherwise zero; for kinds 2 and 3 bit 2 halfword, bits 4-7 lsb, bits 8-12 width in bits, bits 13-21 bank offset; kind 2 has bit 3 bank-0 save to 0x89ec00 and bits 22-31 the default: bit 31 clear, the constant in bits 22-30; bit 31 set, the container at 0x81b074 + bits 22-29, a halfword when bit 2 is set; kind 3 has bits 22-28 the slot-table index and bit 29 the record-0 mirror; unassigned words are zero apart from kind. Slot word, in property order of the slot fields: bits 0-5 record offset, bit 6 halfword, bits 7-10 lsb, bits 11-15 width in bits, bit 16 bank-0 save, bit 17 slot-1 default is the byte at 0x81b074 + bits 18-27, else bits 18-27 are the slot-1 default, bits 28-31 the stock default code (0 for the stage-2 defaults). Special entry, two words in property order of the special properties: word 0 has the kind-2 field layout and save bit, bits 0-1 the subkind (0 hooked field, 1 read-only field, 2 write-only pair, whose layout is the first whole byte) and bits 22-30 the constant default or the provisional value; word 1 has bits 0-1 the default source (0 constant, 1 stock, 2 configuration), bits 2-5 the stock code, bits 6-14 the configuration offset from 0x96b400, bits 15-16 the hook (0 none, 1 bank-0 bracket by 0x61da3c and 0x61d944, 2 gated resave) and bits 17-25 the gate offset from 0x96b400 or the pair's second byte offset. Stock codes 1-8: 0x61df44, 0x61d8a4, 0x61d59c, 0x61d618, 0x61d5f0, 0x61d65c with r0 = 2, 3, 4"

field :: Bool -> BitRange -> BankOffset -> Word32
field half (BitRange lsb bits) (BankOffset offset) =
  (if half then 1 `shiftL` 2 else 0)
    .|. (fromIntegral lsb `shiftL` 4)
    .|. (fromIntegral bits `shiftL` 8)
    .|. (fromIntegral offset `shiftL` 13)

saveBit :: SavePolicy -> Word32
saveBit save = if save == SaveBank0Previous then 1 `shiftL` 3 else 0

stockCode :: StockDefault -> Word32
stockCode stock = case stock of
  Stock61df44 -> 1
  Stock61d8a4 -> 2
  Stock61d59c -> 3
  Stock61d618 -> 4
  Stock61d5f0 -> 5
  Stock61d65c Profile2 -> 6
  Stock61d65c Profile3 -> 7
  Stock61d65c Profile4 -> 8

-- | Whether the property word holds the field: a stage-2 default and no hook.
plainField :: FieldArm -> Bool
plainField arm = case (fieldSource arm, fieldHook arm) of
  (FieldDefaultOnly _, NoHook) -> True
  _ -> False

encodeEntry :: TableEntry -> EncodedEntry
encodeEntry entry = EncodedEntry $ case entry of
  UnassignedEntry -> 1
  FieldEntry (FieldArm offset width bits source save _) ->
    2
      .|. field (width == Half) bits offset
      .|. saveBit save
      .|. defaultBits source
  SlotEntry (BankBranch offset width bits _) mirror (SlotIndex index) ->
    3 .|. field (width == Half) bits offset .|. (index `shiftL` 22) .|. (if mirror == MirrorRecord0 then 1 `shiftL` 29 else 0)
  SpecialEntry (SpecialIndex index) -> (index + 1) `shiftL` 2
  where
    defaultBits source = case source of
      FieldDefaultOnly (DefaultConstant value) -> value `shiftL` 22
      FieldDefaultOnly (DefaultTail (TailOffset tail') _) -> (1 `shiftL` 31) .|. (fromIntegral tail' `shiftL` 22)
      _ -> 0

encodeSlot :: SlotArm -> EncodedSlot
encodeSlot (SlotArm (BankBranch _ _ _ fallback) (RecordBranch (RecordOffset offset) width (BitRange lsb bits)) save slotDefault' _) = EncodedSlot $
  fromIntegral offset
    .|. (if width == Half then 1 `shiftL` 6 else 0)
    .|. (fromIntegral lsb `shiftL` 7)
    .|. (fromIntegral bits `shiftL` 11)
    .|. (if save == SaveBank0Previous then 1 `shiftL` 16 else 0)
    .|. defaultBits
    .|. (code `shiftL` 28)
  where
    defaultBits = case fallback of
      BankConstant value -> value `shiftL` 18
      BankTailByte (TailOffset tail') -> (1 `shiftL` 17) .|. (fromIntegral tail' `shiftL` 18)
    code = case slotDefault' of
      SlotDefaultsRecord -> 0
      SlotStock stock -> stockCode stock

encodeSpecial :: SpecialArm -> Either EncodingError EncodedSpecial
encodeSpecial special = case special of
  HookedField arm -> fieldSpecial 0 arm
  ReadOnlyField arm -> fieldSpecial 1 arm
  WriteOnlyPair (PairArm first (BankOffset second) value save) ->
    Right (EncodedSpecial (2 .|. field False (BitRange 0 8) first .|. saveBit save .|. (value `shiftL` 22)) (bracketHook .|. (fromIntegral second `shiftL` 17)))
  where
    bracketHook = 1 `shiftL` 15
    fieldSpecial subkind (FieldArm offset width bits source save hook) = do
      (value, sourceBits) <- case source of
        FieldDefaultOnly (DefaultConstant c) -> Right (c, 0)
        FieldDefaultOnly (DefaultTail _ _) -> Left SpecialDefaultUnencodable
        FieldStock stock p -> Right (p, 1 .|. (stockCode stock `shiftL` 2))
        FieldConfig (ConfigOffset config) p -> Right (p, 2 .|. (fromIntegral config `shiftL` 6))
      let hookBits = case hook of
            NoHook -> 0
            Bank0Bracket -> bracketHook
            ConfigGatedResave (ConfigOffset gate) -> (2 `shiftL` 15) .|. (fromIntegral gate `shiftL` 17)
      pure (EncodedSpecial ((subkind :: Word32) .|. field (width == Half) bits offset .|. saveBit save .|. (value `shiftL` 22)) (sourceBits .|. hookBits))

-- | The three tables for the classes in property order, refused when any property is delegated.
encodeTables :: [Class] -> Either EncodingError EncodedTables
encodeTables classes
  | not (null [() | Delegated _ <- classes]) = Left DelegatedProperty
  | length slots > slotTableCapacity = Left SlotTableFull
  | length specials > specialTableCapacity = Left SpecialTableFull
  | otherwise = EncodedTables (go 0 0 [owned | Owned owned <- classes]) (map encodeSlot slots) <$> mapM encodeSpecial specials
  where
    slots = [arm | Owned (OwnedSlotField arm) <- classes]
    specials = concatMap specialOf classes
    specialOf verdict = case verdict of
      Owned (OwnedField arm) | not (plainField arm) -> [HookedField arm]
      Owned (OwnedReadOnlyField arm) -> [ReadOnlyField arm]
      Owned (OwnedWriteOnlyPair arm) -> [WriteOnlyPair arm]
      _ -> []
    go _ _ [] = []
    go slot special (owned : rest) = case owned of
      OwnedUnassigned -> encodeEntry UnassignedEntry : go slot special rest
      OwnedField arm
        | plainField arm -> encodeEntry (FieldEntry arm) : go slot special rest
        | otherwise -> encodeEntry (SpecialEntry (SpecialIndex special)) : go slot (special + 1) rest
      OwnedReadOnlyField _ -> encodeEntry (SpecialEntry (SpecialIndex special)) : go slot (special + 1) rest
      OwnedWriteOnlyPair _ -> encodeEntry (SpecialEntry (SpecialIndex special)) : go slot (special + 1) rest
      OwnedSlotField arm -> encodeEntry (SlotEntry (bankArm arm) (slotMirror arm) (SlotIndex slot)) : go (slot + 1) special rest
