module Replacement.FixedImage.Layout
  ( imageBase, imageLimit, recordFlash, recordBlocks, blockBytes, headerBytes, recordMagic
  , rustConstants, linkerMemory
  ) where

import Data.Word (Word32)
import Numeric (showHex)

imageBase, imageLimit, recordFlash, recordBlocks, blockBytes, headerBytes, recordMagic :: Word32
imageBase = 0x00ba0000
imageLimit = 0x00be0000
recordFlash = 0x00900000
recordBlocks = 2
blockBytes = 0x20000
headerBytes = 32
recordMagic = 0x584d314f

rustConstants :: String
rustConstants = unlines ["pub const " ++ name ++ ": u32 = 0x" ++ showHex value ";" | (name, value) <-
  [("IMAGE_BASE", imageBase), ("IMAGE_LIMIT", imageLimit), ("RECORD_FLASH", recordFlash),
   ("RECORD_BLOCKS", recordBlocks), ("BLOCK_BYTES", blockBytes), ("HEADER_BYTES", headerBytes), ("RECORD_MAGIC", recordMagic)]]

linkerMemory :: String
linkerMemory = "OWNED_FIXED (rwx) : ORIGIN = 0x" ++ showHex (imageBase + headerBytes) "" ++ ", LENGTH = 0x" ++ showHex (imageLimit - imageBase - headerBytes) "\n"
