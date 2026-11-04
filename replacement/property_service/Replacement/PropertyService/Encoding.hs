{-# LANGUAGE GHC2021 #-}

-- | The one-word form of a property entry in the owned service's table, shared by the generator,
-- which writes it, and admission, which reads it back from the linked image.
module Replacement.PropertyService.Encoding
  ( EncodedEntry (..)
  , encodeClass
  , encodingDescription
  ) where

import Data.Bits (shiftL, (.|.))
import Data.Word (Word32)
import Replacement.PropertyService.Classify

newtype EncodedEntry = EncodedEntry Word32 deriving (Eq, Show)

encodingDescription :: String
encodingDescription =
  "bits 0-1 kind (0 delegated, 1 unassigned, 2 field), bit 2 halfword, bit 3 bank-0 save to 0x89ec00, bits 4-7 lsb, bits 8-12 width in bits, bits 13-21 bank offset, bits 22-31 default; delegated and unassigned entries are zero apart from kind"

encodeClass :: Class -> EncodedEntry
encodeClass verdict = EncodedEntry $ case verdict of
  Delegated _ -> 0
  Owned OwnedUnassigned -> 1
  Owned (OwnedField (FieldArm (BankOffset offset) width (BitRange lsb bits) (FieldDefault value) save)) ->
    2
      .|. (if width == Half then 1 `shiftL` 2 else 0)
      .|. (if save == SaveBank0Previous then 1 `shiftL` 3 else 0)
      .|. (fromIntegral lsb `shiftL` 4)
      .|. (fromIntegral bits `shiftL` 8)
      .|. (fromIntegral offset `shiftL` 13)
      .|. (value `shiftL` 22)
