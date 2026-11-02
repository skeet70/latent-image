module Emulation.LoaderCode (mutableTableOffsets, loaderSpans) where

import Data.List (nub, sort)
import Data.Word (Word64)

mutableTableOffsets :: [Word64]
mutableTableOffsets = [0x3a0,0x3b0,0x3c0,0x3d0,0x3e0,0x3a0,0x3f0,0x400]

loaderSpans :: Word64 -> Word64 -> [(Word64,Word64)]
loaderSpans base extent = let cells = map (base+) (sort (nub mutableTableOffsets))
  in zip (base:map (+4) cells) (cells++[base+extent])
