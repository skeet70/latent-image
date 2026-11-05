{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Hardware (hardware) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Word
import Emulation.OwnedCase (readWordAt, word32le)
import Replacement.RecordStore.Model

-- id4.dis 0x6a83f4..0x6a8480 and 0x6a6b74..0x6a6c00.
hardware :: HardwareCheck
hardware role cb bytes
  | role == Prepare = (Accept, sealed)
  | storedRows == rows && storedColumn == column = (Accept, bytes)
  | [columnBit] <- setBits (storedColumn `xor` column), [wordIndex] <- rowDifferences =
      let offset = wordIndex * 4 + columnBit `div` 8
          corrected = B.index bytes offset `xor` bit (columnBit `mod` 8)
      in (Accept, replace offset (B.singleton corrected) bytes)
  | otherwise = (Reject, bytes)
  where
    (protected, rowSize) = case cb of Hardware2 -> (0x1ec,16); Hardware3 -> (0x2c,4); _ -> error "hardware callback kind"
    words' = [readWordAt bytes offset | offset <- [0,4..protected-4]]
    rows = B.pack [foldl (\value bitIndex -> if odd (popCount (words' !! (byteIndex * 8 + bitIndex))) then setBit value bitIndex else value) 0 [0 .. min 7 (length words' - byteIndex * 8 - 1)] | byteIndex <- [0..rowSize-1]]
    column = foldl xor (0::Word32) words'
    storedRows = B.take rowSize (B.drop protected bytes)
    storedColumn = readWordAt bytes (protected+rowSize)
    sealed = replace protected (rows <> word32le column) bytes
    rowDifferences = [i | i <- [0..length words'-1], testBit (B.index storedRows (i `div` 8) `xor` B.index rows (i `div` 8)) (i `mod` 8)]
    setBits value = [i | i <- [0..31], testBit value i]
