{-# LANGUAGE GHC2021 #-}

-- | FONT_ICON.FDL words in image3 whose bytes are base-relative directory fields and glyph program
-- headers of the font format.
module Replacement.PropertyService.RetainedFdl
  ( fdlClosures
  ) where

import Control.Monad (guard)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image (..), imageWord)

-- | Unicode code point indexing the dense directory.
newtype GlyphCode = GlyphCode Word32

data FdlRole
  = -- | Flags byte of the entry for the code, then the 24-bit begin offset of the next code's entry.
    DirectoryFlagsAndBegin GlyphCode
  | -- | Opcode 0xfa, extra-byte count 0 and the 16-bit outline length opening the code's program.
    ProgramHeader GlyphCode

data Extent = Extent ScanImage Word32 Word32 SHA.Sha256

data FontLayout = FontLayout
  { layoutCount :: Word32
  , layoutDirectory :: Word32
  , layoutCommands :: Word32
  }

data DirectoryEntry = DirectoryEntry
  { entryFlags :: Word32
  , entryBegin :: Word32
  }

fontBase :: Word32
fontBase = 0x3bc000

fdlClosures :: [(ScanImage, Image)] -> [ClosedSite]
fdlClosures images = fromMaybe [] $ do
  font <- imageFor ScanImage3 images
  guard (imageBase font == fontBase)
  guard (all (extentBound images) extents)
  layout <- fontLayout font
  pure
    [ ClosedSite ScanImage3 site word ConsumedData
    | (site@(GuestAddress address), role) <- sites
    , Just word <- [imageWord font address]
    , roleHolds font layout address role
    ]

-- Directory sites are aligned windows over the entries at fontBase + 0x51 + 4 * code; program
-- sites are the first word of each code's program.
sites :: [(GuestAddress, FdlRole)]
sites =
  [ (GuestAddress 0x3f8078, DirectoryFlagsAndBegin (GlyphCode 0xf009))
  , (GuestAddress 0x3f807c, DirectoryFlagsAndBegin (GlyphCode 0xf00a))
  , (GuestAddress 0x3fadf0, ProgramHeader (GlyphCode 0xe14d))
  , (GuestAddress 0x3fb1e8, ProgramHeader (GlyphCode 0xe157))
  , (GuestAddress 0x4031b8, ProgramHeader (GlyphCode 0xf02e))
  , (GuestAddress 0x3fb858, ProgramHeader (GlyphCode 0xe168))
  ]

-- Header: count u16 at 0x1c, directory u32 at 0x2e, command data u32 at 0x32, format u16 at 0x36,
-- directory format u16 at 0x38 (0x100 = dense four-byte entries), info chunk offset u32 at 0x44.
-- The command data runs from its base to the info chunk, which the last directory entry's begin
-- reaches.
fontLayout :: Image -> Maybe FontLayout
fontLayout font = do
  magic <- bytesAt font fontBase 8
  guard (magic == B.pack [0x46, 0x44, 0x4c, 0x20, 0x46, 0x6f, 0x6e, 0x74])
  count <- littleAt font (fontBase + 0x1c) 2
  directory <- littleAt font (fontBase + 0x2e) 4
  commands <- littleAt font (fontBase + 0x32) 4
  format <- littleAt font (fontBase + 0x36) 2
  directoryFormat <- littleAt font (fontBase + 0x38) 2
  info <- littleAt font (fontBase + 0x44) 4
  guard (format == 4 && directoryFormat == 0x100)
  guard (directory + 4 * (count + 1) <= commands)
  let layout = FontLayout count directory commands
  final <- directoryEntry font layout count
  guard (commands + entryBegin final == info)
  pure layout

-- Entry bytes 0-2 are the begin offset from the command base, byte 3 the flags; the next entry's
-- begin is the end.
directoryEntry :: Image -> FontLayout -> Word32 -> Maybe DirectoryEntry
directoryEntry font layout code = do
  guard (code <= layoutCount layout)
  word <- littleAt font (fontBase + layoutDirectory layout + 4 * code) 4
  pure (DirectoryEntry (word `shiftR` 24) (word .&. 0xffffff))

roleHolds :: Image -> FontLayout -> Word32 -> FdlRole -> Bool
roleHolds font layout address role = case role of
  DirectoryFlagsAndBegin (GlyphCode code) ->
    code + 1 < layoutCount layout
      && address == fontBase + layoutDirectory layout + 4 * code + 3
  ProgramHeader (GlyphCode code) -> fromMaybe False $ do
    guard (code < layoutCount layout)
    entry <- directoryEntry font layout code
    next <- directoryEntry font layout (code + 1)
    let recordStart = fontBase + layoutCommands layout + entryBegin entry
        recordLength = toInteger (entryBegin next) - toInteger (entryBegin entry)
        flags = entryFlags entry
    guard (recordLength > 0 && flags .&. 0x86 == 0)
    -- Flag 1 prefixes a 4-byte advance pair; nonzero flags prefix a halfword program length.
    (programStart, programLength) <-
      if flags == 0
        then pure (recordStart, recordLength)
        else do
          let lengthField = recordStart + 4 * (flags .&. 1)
          stored <- littleAt font lengthField 2
          guard (toInteger lengthField + 2 + toInteger stored <= toInteger recordStart + recordLength)
          pure (lengthField + 2, toInteger stored)
    guard (programStart == address)
    opcode <- littleAt font address 1
    extra <- littleAt font (address + 1) 1
    outline <- littleAt font (address + 2) 2
    pure (opcode == 0xfa && extra == 0 && 4 + toInteger outline <= programLength)

extentBound :: [(ScanImage, Image)] -> Extent -> Bool
extentBound images (Extent label start end digest) = case imageFor label images of
  Nothing -> False
  Just image -> extentMatches image (GuestAddress start) (GuestAddress end) digest

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, decomp/firmwares/xm1-150/app/graphics/fonts
-- (NONMATCHING Ghidra drafts; the stated reads are from plain.dis and id4.dis).
extents :: [Extent]
extents =
  [ -- The FDL font, registry length 0x7cfd1 rounded to the segment size 0x7cfd4.
    extent ScanImage3 0x3bc000 0x438fd4 "28cc50c8d3acb34b7c5c6b023414b24e124cc103062dadae3cd10b30a3d620d8"
  , -- tu_0117e684.c g_011804a4: eight {name, length, address, size} records; ICON at 0x118054c
    -- holds {0x11805d0, 13, 0x3bc000, 0x7cfd1}.
    extent ScanPlain 0x11804a4 0x1180564 "ac2a08e1f12affde9e8ec9aa800a2429f7f789407e4039b29b53082be1168a2f"
  , extent ScanPlain 0x1180564 0x11805e0 "72b88251a4b71e420f58fddf00035d990ab8d5a0dcc3d3c7321774f8eb0ee610"
  , -- tu_01180290.c ov48b_open_font_file: address and size go to object +0x64 and +0x68.
    extent ScanPlain 0x1180290 0x11803fc "7b5467f0db14ee321a69fb7718768858eaf496119c206bb212466b40a5f116f4"
  , -- tu_011978d8.c overlay_font_program_initialize: object +8 = font base, header offsets.
    extent ScanPlain 0x1197b98 0x1197f18 "2c556bd68ef148de008cad4b227c1e1480e0a88d535b7015f5f4cc1f5d5a6bee"
  , -- tu_011978d8.c overlay_font_program_load_glyph; 0x1198348..0x11983ac byte-composes the entry.
    extent ScanPlain 0x1197f98 0x11984e4 "d8fc1123eeaf69af1d9b97f6bb3e6a7ecd2c4f34cd326605af922b1942b39258"
  , -- tu_011984e4.c overlay_font_render_program; 0x119898c..0x1198a8c reads the program header.
    extent ScanPlain 0x1198598 0x1199068 "be82f65d2d9c7c69c18cdf38bbc519174e51685bff6a3c3c3cb952d87f94451c"
  , -- tu_0119ce70.c overlay_font_raster_load_glyph; 0x119fb0c..0x119fb7c byte-composes the entry.
    extent ScanPlain 0x119f74c 0x119fcbc "be0b3caa731e59d4e27c5bbc55174fff4b48031f7456f88a7c495f06f12944be"
  , -- tu_0119fcbc.c raster initializer: object +0x48 = descriptor +0x3c font base, header offsets.
    extent ScanPlain 0x119fcbc 0x11a0240 "1be97f1db3a77366fe25be3b4ab2a0775e6e9af8bbbdaca6aa158fa33bff211b"
  , -- tu_0119fcbc.c overlay_font_render_raster_program; 0x11a09b0..0x11a0ab4 reads the header.
    extent ScanPlain 0x11a05b0 0x11a0fa4 "f7354a4f9b8d4474e009b974915acb3c961ff2d326bb04347f056402682ca223"
  , -- FUN_006aa6f8: copies segment entry 3 to its load address, (size + 0x1ffff) >> 17 blocks.
    extent ScanImage4 0x6aa6f8 0x6aa750 "95376b1d1b267257040d03148611a1e417b53c51ee3fb092852b976cf93b9290"
  , -- Three {source, blocks, 0x3bc000, enable} records; every enable word is 0.
    extent ScanImage4 0x7c907c 0x7c90ac "faec0a8129f2428226710c2a40a0a72ebfc7e8d1071aacc90dd93fb31e4bf5a5"
  , -- FUN_006aa63c: 0x6aa67c..0x6aa684 skip the record transfer when its enable word is 0.
    extent ScanImage4 0x6aa63c 0x6aa6f8 "bc24e2984d7ad25f7d7da4b57b7b2dff98486ad7fd68482b90c52aa56dc135fb"
  ]

extent :: ScanImage -> Word32 -> Word32 -> String -> Extent
extent label start end digest = Extent label start end (reviewedHash digest)

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained FDL digest: " ++)) id . SHA.parseSha256Hex

bytesAt :: Image -> Word32 -> Int -> Maybe B.ByteString
bytesAt image address count = do
  guard (address >= imageBase image)
  let offset = toInteger (address - imageBase image)
  guard (offset + toInteger count <= toInteger (B.length (imageBytes image)))
  pure (B.take count (B.drop (fromInteger offset) (imageBytes image)))

littleAt :: Image -> Word32 -> Int -> Maybe Word32
littleAt image address count = do
  bytes <- bytesAt image address count
  pure (B.foldr (\byte value -> (value `shiftL` 8) .|. fromIntegral byte) 0 bytes)
