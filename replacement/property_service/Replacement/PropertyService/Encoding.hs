{-# LANGUAGE GHC2021 #-}

-- | The packed form of the owned service's tables, shared by the generator, which writes them, and
-- admission, which reads them back from the linked image: one word per property, and one word per
-- slot arm in a second table the property word indexes.
module Replacement.PropertyService.Encoding
  ( EncodedEntry (..)
  , EncodedSlot (..)
  , SlotIndex (..)
  , TableEntry (..)
  , EncodedTables (..)
  , EncodingError (..)
  , encodeEntry
  , encodeSlot
  , encodeTables
  , encodingDescription
  ) where

import Data.Bits (shiftL, (.|.))
import Data.Word (Word32)
import Replacement.PropertyService.Classify

newtype EncodedEntry = EncodedEntry Word32 deriving (Eq, Show)

newtype EncodedSlot = EncodedSlot Word32 deriving (Eq, Show)

-- | Position of a slot arm's word in the slot table, below `slotTableCapacity`.
newtype SlotIndex = SlotIndex Word32 deriving (Eq, Ord, Show)

data TableEntry = DelegatedEntry | UnassignedEntry | FieldEntry FieldArm | SlotEntry BankBranch SlotIndex deriving (Eq, Show)

data EncodedTables = EncodedTables { encodedEntries :: [EncodedEntry], encodedSlots :: [EncodedSlot] } deriving (Eq, Show)

data EncodingError = SlotTableFull deriving (Eq, Show)

-- | The slot index has 7 bits.
slotTableCapacity :: Int
slotTableCapacity = 128

encodingDescription :: String
encodingDescription =
  "property word: bits 0-1 kind (0 delegated, 1 unassigned, 2 field, 3 slot field), and for kinds 2 and 3 bit 2 halfword, bits 4-7 lsb, bits 8-12 width in bits, bits 13-21 bank offset; kind 2 has bit 3 bank-0 save to 0x89ec00 and bits 22-31 the default: bit 31 clear, the constant in bits 22-30; bit 31 set, the container at 0x81b074 + bits 22-29, a halfword when bit 2 is set; kind 3 has bits 22-28 the slot-table index; delegated and unassigned words are zero apart from kind. Slot word, in property order of the slot fields: bits 0-5 record offset, bit 6 halfword, bits 7-10 lsb, bits 11-15 width in bits, bit 16 bank-0 save, bit 17 slot-1 default is the byte at 0x81b074 + bits 18-27, else bits 18-27 are the slot-1 default"

field :: Bool -> BitRange -> BankOffset -> Word32
field half (BitRange lsb bits) (BankOffset offset) =
  (if half then 1 `shiftL` 2 else 0)
    .|. (fromIntegral lsb `shiftL` 4)
    .|. (fromIntegral bits `shiftL` 8)
    .|. (fromIntegral offset `shiftL` 13)

encodeEntry :: TableEntry -> EncodedEntry
encodeEntry entry = EncodedEntry $ case entry of
  DelegatedEntry -> 0
  UnassignedEntry -> 1
  FieldEntry (FieldArm offset width bits fallback save) ->
    2
      .|. field (width == Half) bits offset
      .|. (if save == SaveBank0Previous then 1 `shiftL` 3 else 0)
      .|. defaultBits fallback
  SlotEntry (BankBranch offset width bits _) (SlotIndex index) ->
    3 .|. field (width == Half) bits offset .|. (index `shiftL` 22)
  where
    defaultBits fallback = case fallback of
      DefaultConstant value -> value `shiftL` 22
      DefaultTail (TailOffset tail') _ -> (1 `shiftL` 31) .|. (fromIntegral tail' `shiftL` 22)

encodeSlot :: SlotArm -> EncodedSlot
encodeSlot (SlotArm (BankBranch _ _ _ fallback) (RecordBranch (RecordOffset offset) width (BitRange lsb bits)) save) = EncodedSlot $
  fromIntegral offset
    .|. (if width == Half then 1 `shiftL` 6 else 0)
    .|. (fromIntegral lsb `shiftL` 7)
    .|. (fromIntegral bits `shiftL` 11)
    .|. (if save == SaveBank0Previous then 1 `shiftL` 16 else 0)
    .|. defaultBits
  where
    defaultBits = case fallback of
      BankConstant value -> value `shiftL` 18
      BankTailByte (TailOffset tail') -> (1 `shiftL` 17) .|. (fromIntegral tail' `shiftL` 18)

-- | Both tables for the classes in property order.
encodeTables :: [Class] -> Either EncodingError EncodedTables
encodeTables classes
  | length slots > slotTableCapacity = Left SlotTableFull
  | otherwise = Right (EncodedTables (go 0 classes) (map encodeSlot slots))
  where
    slots = [arm | Owned (OwnedSlotField arm) <- classes]
    go _ [] = []
    go next (verdict : rest) = case verdict of
      Delegated _ -> encodeEntry DelegatedEntry : go next rest
      Owned OwnedUnassigned -> encodeEntry UnassignedEntry : go next rest
      Owned (OwnedField arm) -> encodeEntry (FieldEntry arm) : go next rest
      Owned (OwnedSlotField arm) -> encodeEntry (SlotEntry (bankArm arm) (SlotIndex next)) : go (next + 1) rest
