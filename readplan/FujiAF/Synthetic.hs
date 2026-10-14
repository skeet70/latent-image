-- Synthetic decoded image for tests. It carries the real table addresses and entry counts with
-- invented offsets: every row-0 entry resolves into window 0x000300, except the monochrome filter
-- entries which resolve into the oracle window 0x000100, and every entry beyond row 0 resolves into
-- window 0x001000, so the two develop scopes differ by exactly one window.
module FujiAF.Synthetic
  ( syntheticBase
  , syntheticImage
  , rowZeroOffset
  , beyondRowZero
  ) where

import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as Map
import Data.Word (Word32, Word8)
import FujiAF.AdjustmentTables

syntheticBase :: CodeAddress
syntheticBase = CodeAddress 0x00d61e00

-- Covers every table in developTables, the last of which ends at 0x00d6ad18.
syntheticLength :: Int
syntheticLength = 0x9400

beyondRowZero :: Word32
beyondRowZero = 0x001000

rowZeroOffset :: DevelopTable -> Int -> Word32
rowZeroOffset table index = case tableRowZeroIndex table of
  ByDevelopCode -> 0x000300 + 2 * fromIntegral index
  ByMonochromeFilter -> 0x000110 + fromIntegral index
  Unindexed -> 0x000380

syntheticImage :: B.ByteString
syntheticImage = B.pack (map byteAt [0 .. syntheticLength - 1])
  where
    CodeAddress base = syntheticBase
    byteAt position =
      let address = base + fromIntegral position
          aligned = address - address `mod` 2
          value = Map.findWithDefault 0 aligned entries
      in if address == aligned then low value else high value
    low value = fromIntegral (value .&. 0xff) :: Word8
    high value = fromIntegral ((value `shiftR` 8) .&. 0xff) :: Word8

-- The 0x00d6a5c4 extent runs two entries into 0x00d6a708; row-0 values are listed last so they win
-- where extents overlap.
entries :: Map.Map Word32 Word32
entries = Map.fromList (concatMap (entriesWhere (not . rowZero)) developTables ++ concatMap (entriesWhere rowZero) developTables)
  where
    rowZero (table, index) = index < tableRowZeroEntries table
    entriesWhere selected table =
      [ (base + 2 * fromIntegral index, if rowZero (table, index) then rowZeroOffset table index else beyondRowZero)
      | index <- [0 .. tableExtentEntries table - 1]
      , selected (table, index)
      ]
      where CodeAddress base = tableAddress table
