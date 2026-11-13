module Replacement.FixedImage.Layout
  ( imageBase, imageLimit, recordFlash, recordBlocks, blockBytes, headerBytes, recordMagic
  , rustConstants, linkerMemory
  ) where

import Replacement.FixedImage.Values
import Numeric (showHex)

rustConstants :: String
rustConstants = unlines ["pub const " ++ name ++ ": u32 = 0x" ++ showHex value ";" | (name, value) <-
  [("IMAGE_BASE", imageBase), ("IMAGE_LIMIT", imageLimit), ("RECORD_FLASH", recordFlash),
   ("RECORD_BLOCKS", recordBlocks), ("BLOCK_BYTES", blockBytes), ("HEADER_BYTES", headerBytes), ("RECORD_MAGIC", recordMagic)]]

linkerMemory :: String
linkerMemory = "OWNED_FIXED (rwx) : ORIGIN = 0x" ++ showHex (imageBase + headerBytes) "" ++ ", LENGTH = 0x" ++ showHex (imageLimit - imageBase - headerBytes) "\n"
