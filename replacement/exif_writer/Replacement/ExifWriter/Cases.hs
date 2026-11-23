{-# LANGUAGE OverloadedStrings #-}

-- | The owned Exif writer's extents, entries and retained services, and the synthetic capture
-- inputs both roles run on.
module Replacement.ExifWriter.Cases
  ( Image (..)
  , images
  , overlay
  , ownedExtents
  , entries
  , stockFunctions
  , unreachableDirections
  , Service (..)
  , services
  , Block (..)
  , Scenario (..)
  , HeaderCall (..)
  , Argument (..)
  , Step (..)
  , scenarios
  , replaySources
  , parsedReplays
  , globalsBase
  , outputBase
  , outputBytes
  ) where

import Control.Monad (unless)
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word16, Word32, Word8)
import Numeric (showHex)
import Replacement.FileService.Native (Extent (..), word32le)

-- | A pinned stock image and the address of its first byte.
data Image = Image {imageLabel :: String, imageBase :: Word32, imageDigest :: String}

images :: [Image]
images =
  [ Image "loader-a" 0x40000 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  , Image "image4" 0x61c400 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  , Image "plain" 0x01100000 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
  ]

-- | Overlay 46 as the plain image holds it.
overlay :: Extent
overlay = Extent 0x011c0000 0x011dcca0

ownedExtents :: [(String, Extent)]
ownedExtents =
  [ ("headers", Extent 0x011c14f0 0x011c1d18)
  , ("length-patches", Extent 0x011c1e00 0x011c1e7c)
  , ("saved-parser", Extent 0x011c2154 0x011c2208)
  , ("swap", Extent 0x011c2b50 0x011c2b80)
  , ("values", Extent 0x011c2c40 0x011c2cb8)
  , ("saved-slots", Extent 0x011c35f8 0x011c36f8)
  , ("directories", Extent 0x011cda48 0x011d0404)
  , ("entry", Extent 0x011db098 0x011db1ac)
  ]

-- | Every closure function stock code outside the extents or a retained table word reaches,
-- with the veneer symbol the owned link places there.
entries :: [(String, Word32)]
entries =
  [ ("exif_entry_initialize", 0x011c14f0)
  , ("exif_entry_write_primary", 0x011c170c)
  , ("exif_entry_write_exif", 0x011c17cc)
  , ("exif_entry_write_makernote", 0x011c19c8)
  , ("exif_entry_write_gps", 0x011c1b30)
  , ("exif_entry_patch_thumbnail_length", 0x011c1e4c)
  , ("exif_entry_transfer_saved_parser", 0x011c2154)
  , ("exif_entry_swap_halfword", 0x011c2b50)
  , ("exif_entry_copy_value", 0x011c2c40)
  , ("exif_entry_type_size", 0x011c2c98)
  , ("exif_entry_saved_directory_slots", 0x011c35f8)
  , ("exif_entry_saved_directory_values", 0x011c3678)
  , ("exif_entry_cb_thumbnail_offset", 0x011cecb8)
  , ("exif_entry_cb_thumbnail_length", 0x011cece8)
  , ("exif_entry_cb_exif_pointer", 0x011ced18)
  , ("exif_entry_cb_gps_pointer", 0x011ced48)
  , ("exif_entry_cb_one", 0x011ced80)
  , ("exif_entry_cb_makernote", 0x011ceda0)
  , ("exif_entry_cb_interoperability_pointer", 0x011cede0)
  , ("exif_entry_cb_packed_halves", 0x011cee10)
  , ("exif_entry_cb_source_word", 0x011cee4c)
  , ("exif_entry_cb_staged_block", 0x011cef60)
  , ("exif_entry_cb_source_bytes_256", 0x011cefa8)
  , ("exif_entry_cb_source_bytes_122", 0x011ceff0)
  , ("exif_entry_cb_default_text", 0x011cf038)
  , ("exif_entry_cb_copyright", 0x011cf3cc)
  , ("exif_entry_cb_printim", 0x011cf82c)
  , ("exif_entry_select_table", 0x011cf854)
  , ("exif_entry_table_count", 0x011cf908)
  , ("exif_entry_write_filtered_directory", 0x011cf990)
  , ("exif_entry_cb_image_records", 0x011cfd58)
  , ("exif_entry_write_exif_header", 0x011db098)
  , ("exif_entry_finish", 0x011db188)
  ]

-- | The stock functions in the extents, start and size with literal pools (A32 bytes).
stockFunctions :: [(Word32, Word32)]
stockFunctions =
  [ (0x011c14f0, 132), (0x011c1574, 68), (0x011c15b8, 184), (0x011c1670, 156), (0x011c170c, 192)
  , (0x011c17cc, 224), (0x011c18ac, 284), (0x011c19c8, 360), (0x011c1b30, 228), (0x011c1c14, 260)
  , (0x011c1e00, 76), (0x011c1e4c, 48), (0x011c2154, 180), (0x011c2b50, 48), (0x011c2c40, 24)
  , (0x011c2c58, 64), (0x011c2c98, 32), (0x011c35f8, 128), (0x011c3678, 128), (0x011cda48, 1112)
  , (0x011cdea0, 3608), (0x011cecb8, 48), (0x011cece8, 48), (0x011ced18, 48), (0x011ced48, 56)
  , (0x011ced80, 32), (0x011ceda0, 64), (0x011cede0, 48), (0x011cee10, 60), (0x011cee4c, 84)
  , (0x011ceea0, 192), (0x011cef60, 72), (0x011cefa8, 72), (0x011ceff0, 72), (0x011cf038, 164)
  , (0x011cf0dc, 752), (0x011cf3cc, 124), (0x011cf448, 72), (0x011cf490, 280), (0x011cf5a8, 312)
  , (0x011cf6e0, 104), (0x011cf748, 228), (0x011cf82c, 40), (0x011cf854, 180), (0x011cf908, 136)
  , (0x011cf990, 968), (0x011cfd58, 252), (0x011cfe54, 1456), (0x011db098, 240), (0x011db188, 36)
  ]

-- | Conditional branch directions of 'stockFunctions' no input reaches: (branch address,
-- @taken@ or @not-taken@, the bytes that decide it).
unreachableDirections :: [(Word32, String, String)]
unreachableDirections = []

-- | A retained leaf service: the image it lives in, its entry, its extent and the label its
-- recorder writes.
data Service = Service
  { serviceName :: String
  , serviceImage :: String
  , serviceEntry :: Word32
  , serviceEnd :: Word32
  }

-- | The services the writer calls, and @0x7221a0@, which @memset@ calls.
services :: [Service]
services =
  [ Service "copy-bytes" "loader-a" 0x4357c 0x435ac
  , Service "strcpy" "image4" 0x721b04 0x721b40
  , Service "strncpy" "image4" 0x721b40 0x721bac
  , Service "strlen" "image4" 0x721f30 0x721f6c
  , Service "memcpy" "image4" 0x722054 0x7221a0
  , Service "fill-words" "image4" 0x7221a0 0x7221c8
  , Service "memset" "image4" 0x7221c8 0x722218
  ]

-- | Initial bytes of a writable region.
data Block = Block {blockName :: String, blockAddress :: Word32, blockBytes :: B.ByteString}

-- | The arguments of @0x011db098@.
data HeaderCall = HeaderCall {headerFormat :: Word32, headerOption :: Word32}

-- | A driver argument: a word, the output cursor, or the bytes the cursor has advanced past its
-- start.
data Argument = Literal Word32 | Cursor | Advanced

-- | One driver call. An advancing call adds its result to the cursor.
data Step = Step
  { stepEntry :: Word32
  , stepArguments :: [Argument]
  , stepStack :: [Argument]
  , stepReturns :: Bool
  , stepAdvances :: Bool
  }

data Scenario = Scenario
  { scenarioName :: String
  , scenarioProvenance :: String
  , scenarioBlocks :: [Block]
  , scenarioCursor :: Word32
  , scenarioSteps :: [Step]
  , scenarioHeader :: Maybe HeaderCall
  }

globalsBase, outputBase, primaryBase, exifBase, makerBase, gpsBase, thumbnailBase, mpfIndexBase, mpfAttributeBase, parserBase, savedBase :: Word32
globalsBase = 0x014e9d40
outputBase = 0x00400000
primaryBase = 0x00500000
exifBase = 0x00501000
gpsBase = 0x00502000
thumbnailBase = 0x00503000
mpfIndexBase = 0x00504000
mpfAttributeBase = 0x00504100
parserBase = 0x00505000
makerBase = 0x00510000
savedBase = 0x00600000

outputBytes, makerBytes, savedBytes :: Int
outputBytes = 0x8000
makerBytes = 0x10000
savedBytes = 0xc000

-- Field encoders --------------------------------------------------------------

u16 :: Word16 -> B.ByteString
u16 value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8)]

u32 :: Word32 -> B.ByteString
u32 = word32le

rational :: Word32 -> Word32 -> B.ByteString
rational numerator denominator = u32 numerator <> u32 denominator

ascii :: String -> B.ByteString
ascii text = BC.pack text <> B.singleton 0

padded :: Int -> String -> B.ByteString
padded size text = B.take size (BC.pack text <> B.replicate size 0)

-- | @size@ bytes of @fill@ with each field written at its offset.
block :: Int -> Word8 -> [(Int, B.ByteString)] -> B.ByteString
block size fill = foldl' put (B.replicate size fill)
  where
    put bytes (offset, value) = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes


-- Synthetic capture ----------------------------------------------------------------

-- | The one-shot identity at @0x014e9d98@ and the rest of the writer globals, zero.
globals :: B.ByteString
globals =
  block 0x1c0 0
    [ (0x58, padded 16 "FUJIFILM")
    , (0x68, padded 32 "X-M1")
    , (0x88, padded 64 "Digital Camera X-M1 Ver1.50")
    , (0xc8, ascii "R98")
    , (0xcc, BC.pack "FF02B3540125     595331303037 2013:06:28 4D59")
    ]

dateTime :: B.ByteString
dateTime = ascii "2026:11:22 10:30:15"

-- | The IFD0 source block: Copyright text, PrintIM matching byte and block word, then @fields@.
primaryBlock :: String -> Word8 -> Word32 -> [(Int, B.ByteString)] -> B.ByteString
primaryBlock copyright printMatching printBlock fields =
  block 0x200 0 $
    [ (0x70, rational 72 1), (0x78, rational 72 1)
    , (0x84, u16 1), (0x86, u16 2), (0x88, u16 2)
    , (0x9e, dateTime)
    , (0xb2, ascii copyright)
    , (0x15c, B.singleton printMatching), (0x160, u32 printBlock)
    , (0x164, B.pack [0x01, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x80, 0x80, 0x80, 0x00])
    , (0x1a8, B.singleton 1)
    ]
      ++ fields

-- | WhitePoint, PrimaryChromaticities and YCbCrCoefficients (tags 0x13e, 0x13f, 0x211).
primaryChromaticity :: [(Int, B.ByteString)]
primaryChromaticity =
  [ (0xf4, rational 313 1000 <> rational 329 1000)
  , (0x104, rational 64 100 <> rational 33 100 <> rational 21 100 <> rational 71 100 <> rational 15 100 <> rational 6 100)
  , (0x134, rational 299 1000 <> rational 587 1000 <> rational 114 1000)
  ]

-- | The Exif source block with ColorSpace @colorSpace@, then @fields@.
exifBlock :: Word16 -> [(Int, B.ByteString)] -> B.ByteString
exifBlock colorSpace fields =
  block 0x400 0 $
    [ (0x0, rational 1 250), (0x8, rational 56 10), (0x10, u16 3), (0x12, u16 400)
    , (0x112, BC.pack "0230"), (0x116, dateTime), (0x12a, dateTime)
    , (0x13e, B.pack [1, 2, 3, 0]), (0x144, rational 2 1)
    , (0x14c, rational 797 100), (0x154, rational 497 100), (0x15c, rational 653 100)
    , (0x164, rational 0 100), (0x16c, rational 300 100)
    , (0x174, u16 5), (0x176, u16 0), (0x178, u16 16), (0x17c, rational 230 10)
    , (0x284, u16 35), (0x286, BC.pack "0100"), (0x28a, u16 colorSpace)
    , (0x28c, u32 4896), (0x290, u32 3264)
    , (0x294, rational 1655 1), (0x29c, rational 1655 1), (0x2a4, u16 3), (0x2a6, u16 2)
    , (0x2a8, B.singleton 3), (0x2aa, B.singleton 1)
    , (0x2ac, u16 0), (0x2ae, u16 0), (0x2b0, u16 0), (0x2b2, u16 0), (0x2b4, u16 0), (0x2b6, u16 0)
    , (0x2bc, rational 16 1 <> rational 50 1 <> rational 35 10 <> rational 56 10)
    , (0x2dc, ascii "FUJIFILM"), (0x2ea, ascii "XC16-50mmF3.5-5.6 OIS"), (0x32a, ascii "36A04521")
    ]
      ++ fields

-- | Gamma set (tag 0xa500) and SubjectDistanceRange unset (tag 0xa40a).
exifGamma :: [(Int, B.ByteString)]
exifGamma = [(0x33c, rational 22 10), (0x2b4, u16 0xffff)]

-- | One face-recognition frame record at @0x1d3c + 832*i@ of the maker block, the source of one
-- image-record directory (table @0x011d23c4@): its words at @+0@ and @+0x38@ and the halfword at
-- @+0x3c@, the element count of tags 0x1110..0x1112.
data FrameRecord = FrameRecord Word32 Word32 Word16

-- | Face detection as the maker block records it.
data Faces = Faces
  { facesCaptured :: Bool
  -- ^ @0x1e8@
  , facesDetected :: Int
  -- ^ @0x222@, the 0x41xx group
  , facesRecognized :: Int
  -- ^ @0x58a@, the 0x42xx group
  , facesRectangles :: Word16
  -- ^ @0x20a@
  , facesCounts :: [Word16]
  -- ^ @0xe03c..0xe042@
  , facesKinds :: [Word16]
  -- ^ @0xe044..0xe04e@, one per tag 0x4002..0x4007
  , facesShown :: Word8
  -- ^ @0x1f2@
  , facesFrames :: Word32
  -- ^ @0x1d38@
  , facesFlags :: Word16
  -- ^ @0x1f0@
  , facesRecords :: [FrameRecord]
  }

noFaces :: Faces
noFaces = Faces False 0 0 0xffff [0, 0, 0, 0] [1, 1, 1, 1, 0, 0] 0 0 0 []

detectedFaces :: Int -> Faces
detectedFaces count = Faces True count count (2 * n) [n, 0, n, 0] [1, 1, 1, 1, 0, 0] 1 1 1 (replicate count (FrameRecord 1 0 0))
  where
    n = fromIntegral count

-- | One face with its recognition record; three of the six per-tag counts zero.
oneFace :: Faces
oneFace = (detectedFaces 1) {facesCounts = [1, 1, 1, 0], facesKinds = [1, 0, 1, 0, 1, 0]}

-- | The array bounds of the maker block: ten detected faces (0x224..0x22e) and sixty frame
-- records (0x1d3c..0xe03c), cycling through every record shape.
maximumFaces :: Faces
maximumFaces =
  Faces True 10 60 20 [0, 1, 240, 1] [1, 1, 1, 1, 1, 1] 1 1 1 $
    take 60 (cycle [FrameRecord 1 1 3, FrameRecord 0 1 0xffff, FrameRecord 1 0 0, FrameRecord 1 1 0, FrameRecord 0 0 0])

-- | Recognition records without detected faces, zero rectangles, nothing shown.
recognizedOnly :: Faces
recognizedOnly = Faces True 0 2 0 [0, 0, 2, 0] [0, 0, 0, 0, 0, 0] 0 0 0 [FrameRecord 1 0 0, FrameRecord 0 0 0]

-- | Detected faces without recognition records, rectangles unset, frame word 2.
detectedOnly :: Faces
detectedOnly = Faces True 3 0 0xffff [3, 1, 0, 0] [1, 1, 0, 0, 1, 1] 1 2 1 []

-- | Counts left from an earlier frame with the capture byte clear.
staleFaces :: Faces
staleFaces = (detectedFaces 2) {facesCaptured = False}

-- | The maker-note source block: @faces@, the @0xe050@ byte that suppresses the 0x45xx group
-- when it is @'F'@, then @fields@.
makerBlock :: Faces -> Word8 -> [(Int, B.ByteString)] -> B.ByteString
makerBlock faces group fields =
  block makerBytes 0 $
    [ (0x0, BC.pack "0130"), (0x34, padded 8 "NORMAL ")
    , (0x3c, u16 3), (0x3e, u16 0), (0x40, u16 0), (0x42, u16 0), (0x44, u16 0)
    , (0x48, u32 1), (0x4c, u32 0 <> u32 0), (0x54, u16 0xffff), (0x5c, u32 1), (0x60, u16 0)
    , (0x64, rational 0 100), (0x6c, u16 0), (0x6e, u16 0), (0x70, u16 0), (0x72, u16 1)
    , (0x74, u16 2448), (0x76, u16 1632), (0x78, u16 0), (0x7a, u16 0 <> u16 0 <> u16 0)
    , (0x92, u16 0), (0x9c, u16 0), (0x9e, u16 3), (0xa0, u16 1)
    , (0xa2, u16 0xffff), (0xa4, u16 0xffff), (0xa6, u16 0xffff)
    , (0xa8, u32 0), (0xac, u32 0), (0xb0, u32 0), (0xb8, u32 0)
    , (0xbe, u16 0), (0xc0, u16 0), (0xc4, u32 0xffffffff), (0xcc, u16 0xffff), (0xce, u16 0xffff)
    , (0xd0, u16 0), (0xd4, u32 0xffffffff)
    , (0xde, u16 0), (0xe0, u16 0), (0xe2, u16 0), (0xe4, u16 0), (0xe6, u16 0), (0xe8, u16 0)
    , (0xea, u16 1), (0xec, u32 1), (0xf0, u16 0), (0xf2, u16 0), (0xf4, u16 100), (0xf6, u16 100)
    , (0xfc, u32 0), (0x100, u16 0 <> u16 0 <> u16 0), (0x108, u16 0xffff), (0x10c, u32 0)
    , (0x134, u32 0), (0x138, padded 0x81 "")
    , (0x1bc, rational 16 1), (0x1c4, rational 50 1), (0x1cc, rational 35 10), (0x1d4, rational 56 10)
    , (0x1e8, u32 flag), (0x1f0, u16 (facesFlags faces)), (0x1f2, B.singleton (facesShown faces))
    , (0x20a, u16 (facesRectangles faces)), (0x222, u16 (fromIntegral detected)), (0x58a, u16 (fromIntegral recognized))
    , (0x1d38, u32 (facesFrames faces))
    , (0xe03c, B.concat (map u16 (facesCounts faces))), (0xe044, B.concat (map u16 (facesKinds faces)))
    , (0xe050, B.pack [group, 0, 0, 0]), (0xe054, u16 1)
    , (0xe870, u32 flag), (0xe874, u32 0), (0xe878, u32 0), (0xe87c, u32 0)
    , (0xe880, rational 0 1), (0xe888, rational 0 1), (0xe890, u16 0), (0xe892, u16 0)
    , (0xe894, u16 0xffff), (0xe8a4, u32 0xffffffff), (0xe8a8, u16 0xffff), (0xe8e8, u32 0), (0xe8f0, u32 0xffffffff)
    ]
      ++ [(0x1f6, B.pack [1, 0]) | facesCaptured faces]
      ++ [(0x20c + 8 * i, u16 (fromIntegral (1200 + 420 * i)) <> u16 900 <> u16 380 <> u16 380) | i <- [0 .. min detected 2 - 1]]
      ++ concat
        [ [ (0x224 + i, B.singleton 1), (0x238 + 8 * i, u16 1200 <> u16 900 <> u16 1580 <> u16 1280)
          , (0x288 + 8 * i, u16 (fromIntegral (100 * i)) <> u16 0 <> u16 100 <> u16 100), (0x2d8 + 2 * i, u16 1)
          , (0x56c + 2 * i, u16 2), (0x580 + i, B.singleton 1)
          ]
        | i <- [0 .. min detected 10 - 1]
        ]
      ++ concat
        [ [ (0x58c + 2 * i, u16 (fromIntegral (i + 1))), (0x604 + i, B.singleton 1)
          , (0x640 + 8 * i, u16 (fromIntegral (1210 + 7 * i)) <> u16 910 <> u16 360 <> u16 360)
          , (0x1978 + 4 * i, u32 (fromIntegral i)), (0x1a68 + 2 * i, u16 3), (0x1b58 + 4 * i, u32 1)
          ]
        | i <- [0 .. min recognized 60 - 1]
        ]
      ++ concat (zipWith frame [0 ..] (facesRecords faces))
      ++ fields
  where
    flag = if facesCaptured faces then 1 else 0
    detected = facesDetected faces
    recognized = facesRecognized faces
    frame :: Int -> FrameRecord -> [(Int, B.ByteString)]
    frame index (FrameRecord valid named texts) =
      let record = 0x1d3c + 832 * index
       in [ (record, u32 valid), (record + 4, u32 1), (record + 8, u16 6)
          , (record + 10, padded 32 ("FACE" ++ show index)), (record + 0x2c, u32 0), (record + 0x30, padded 8 "")
          , (record + 0x38, u32 named), (record + 0x3c, u16 texts), (record + 0x3e, ascii ("PERSON" ++ show index))
          ]

-- | Every field a maker rule's filter reads, at a recorded value.
makerPresent :: [(Int, B.ByteString)]
makerPresent =
  [ (0x4, padded 48 "FF02B3540125     595331303037 2013:06:28 4D59")
  , (0x3c, u16 3), (0x40, u16 0), (0x42, u16 0), (0x44, u16 1), (0x48, u32 1), (0x54, u16 0)
  , (0x58, B.pack [1, 0, 0, 0]), (0x5c, u32 1), (0x6e, u16 0), (0x78, u16 0), (0x92, u16 1), (0x94, u32 1)
  , (0xa2, u16 0), (0xa4, u16 0), (0xa6, u16 0), (0xa8, u32 0), (0xac, u32 0), (0xb0, u32 1), (0xb8, u32 1)
  , (0xc4, u32 2), (0xca, u16 4), (0xcc, u16 0), (0xce, u16 0), (0xd4, u32 0), (0xd8, u32 1), (0xec, u32 1)
  , (0xf2, u16 0), (0xf4, u16 100), (0xf6, u16 0), (0xf8, u16 1), (0xfc, u32 1), (0x106, u16 0), (0x108, u16 0)
  , (0x10c, u32 1), (0x112, u16 4), (0x134, u32 0)
  , (0xe050, BC.pack "A\0\0\0"), (0xe870, u32 1), (0xe874, u32 1), (0xe878, u32 1), (0xe87c, u32 1)
  , (0xe894, u16 1), (0xe8a4, u32 0), (0xe8a8, u16 0), (0xe8e8, u32 1), (0xe8f0, u32 0)
  ]

-- | Every field a maker rule's filter reads, at its unset sentinel.
makerAbsent :: [(Int, B.ByteString)]
makerAbsent =
  [ (0x3c, u16 0xffff), (0x40, u16 0xffff), (0x42, u16 0xffff), (0x44, u16 0), (0x48, u32 0), (0x54, u16 0xffff)
  , (0x58, B.pack [0, 0, 0, 0]), (0x5c, u32 0), (0x6e, u16 0xffff), (0x78, u16 0xffff), (0x92, u16 0xffff)
  , (0xa2, u16 0xffff), (0xa4, u16 0xffff), (0xa6, u16 0xffff), (0xa8, u32 0xffff), (0xac, u32 0xffff)
  , (0xb0, u32 0), (0xb8, u32 0), (0xc4, u32 0xffffffff), (0xcc, u16 0xffff), (0xce, u16 0xffff)
  , (0xd4, u32 0xffffffff), (0xec, u32 0), (0xf2, u16 0xffff), (0xf4, u16 0xffff), (0xf6, u16 0xffff)
  , (0xf8, u16 0), (0xfc, u32 0), (0x106, u16 0xffff), (0x108, u16 0xffff), (0x10c, u32 0), (0x134, u32 0xffffffff)
  , (0xe050, BC.pack "F\0\0\0"), (0xe870, u32 0), (0xe874, u32 0), (0xe878, u32 0), (0xe87c, u32 0)
  , (0xe894, u16 0xffff), (0xe8a4, u32 0xffffffff), (0xe8a8, u16 0xffff), (0xe8e8, u32 0), (0xe8f0, u32 0xffffffff)
  ]

-- | The second operand of each two-field maker filter decisive, and count fields at zero.
makerMixed :: [(Int, B.ByteString)]
makerMixed =
  [ (0x40, u16 0), (0xd8, u32 0), (0x92, u16 1), (0x94, u32 0), (0xc4, u32 1), (0xca, u16 0), (0xe894, u16 0)
  , (0x112, u16 0), (0x10c, u32 1)
  ]

-- | The GPS source block, then @fields@.
gpsBlock :: [(Int, B.ByteString)] -> B.ByteString
gpsBlock fields =
  block 0x100 0 $
    [ (0x0, B.pack [2, 2, 0, 0]), (0x4, ascii "N"), (0x8, rational 35 1 <> rational 39 1 <> rational 2940 100)
    , (0x20, ascii "E"), (0x24, rational 139 1 <> rational 44 1 <> rational 3060 100)
    , (0x3c, B.singleton 0), (0x40, rational 40 1), (0x48, rational 1 1 <> rational 30 1 <> rational 15 1)
    , (0x60, B.pack [0, 0, 0]), (0x63, ascii "A"), (0x65, ascii "2"), (0x68, rational 5 1)
    , (0x70, ascii "K"), (0x74, rational 0 1), (0x7c, ascii "T"), (0x80, rational 1 1 <> u32 0)
    , (0x88, ascii "WGS-84"), (0x94, ascii "2026:11:22")
    ]
      ++ fields

-- | Satellites, an eight-byte map datum and a set image direction.
gpsPresent :: [(Int, B.ByteString)]
gpsPresent = [(0x60, ascii "08"), (0x84, u32 1), (0x88, ascii "TOKYO-1")]

-- | Status, measure mode, DOP, track reference and an empty map datum unset; image direction zero.
gpsAbsent :: [(Int, B.ByteString)]
gpsAbsent = [(0x63, B.singleton 0), (0x65, B.singleton 0), (0x68, u32 0 <> u32 0), (0x7c, B.singleton 0), (0x84, u32 0), (0x88, ascii "")]

-- | DOP numerator zero over a nonzero denominator, image direction reference unset, three-letter datum.
gpsMixed :: [(Int, B.ByteString)]
gpsMixed = [(0x68, u32 0 <> u32 1), (0x80, u32 0), (0x88, ascii "WGS")]

thumbnailBlock :: B.ByteString
thumbnailBlock =
  block 0x40 0 [(0x0, rational 72 1), (0x8, rational 72 1), (0x28, u16 6), (0x2a, u16 1), (0x2c, u16 2), (0x2e, u16 2)]

-- Saved parse -----------------------------------------------------------------------

-- | Per directory kind, the slot and value areas of the saved-parse state (0x011c35f8,
-- 0x011c3678).
savedAreas :: [(Int, Int, Int)]
savedAreas = [(0, 0xffc, 0x10d4), (1, 0x1c4c, 0x1e5c), (2, 0x250c, 0x2c44), (5, 0xa444, 0xa51c), (6, 0xa6f8, 0xa758)]

-- | A parsed directory entry: tag, type, count and value bytes.
data Parsed = Parsed Word16 Word16 Word32 B.ByteString

-- | The saved-parse state holding @directories@: per kind, a twelve-byte slot (count, value
-- offset, type) at each stock table rule whose tag the directory carries, and the values packed
-- at even offsets.
savedStateOf :: B.ByteString -> [(Int, [Parsed])] -> B.ByteString
savedStateOf plain directories = block savedBytes 0 (concatMap directory directories)
  where
    directory (kind, present) =
      let (slots, values, end) = case [(s, v, e) | ((k, s, v), e) <- zip savedAreas (map (\(_, s, _) -> s) (drop 1 savedAreas) ++ [savedBytes]), k == kind] of
            (area : _) -> area
            [] -> error ("no saved area for kind " ++ show kind)
          placed = snd (foldl' place (0, []) [(index, parsed) | (index, (tag, _)) <- zip [0 :: Int ..] (tableRules plain kind), Just parsed <- [lookup tag [(t, p) | p@(Parsed t _ _ _) <- present]]])
          place (offset, done) (index, Parsed _ valueType count bytes) =
            (offset + B.length bytes + (B.length bytes .&. 1), done ++ [(index, valueType, count, offset, bytes)])
          used = sum [B.length bytes + (B.length bytes .&. 1) | (_, _, _, _, bytes) <- placed]
       in if values + used > end
            then error ("saved values of kind " ++ show kind ++ " overflow their area")
            else
              concat
                [ [(slots + 12 * index, u32 count <> u32 (fromIntegral offset) <> u16 valueType), (values + offset, bytes)]
                | (index, valueType, count, offset, bytes) <- placed
                ]

-- | Synthetic parse slots: Pass 0 generates the Exif IFD pointer from a saved slot and pass 1
-- only from an empty one, so the slot is present for pass 0 alone; the MakerNote and GPS
-- pointers are always present.
savedState :: B.ByteString -> Int -> B.ByteString
savedState plain pass = savedStateOf plain [(kind, map (typed kind) present) | (kind, present) <- saved]
  where
    exifPointer = [(0x8769, u32 0) | pass == 0]
    saved =
      [ (0, [(0x10f, ascii "FUJIFILM"), (0x110, ascii "X-M1"), (0x112, u16 6), (0x11a, rational 72 1), (0x11b, rational 72 1), (0x128, u16 2), (0x132, dateTime), (0x213, u16 2), (0x8825, u32 0)] ++ exifPointer)
      , (1, [(0x829a, rational 1 60), (0x829d, rational 28 10), (0x8827, u16 1600), (0x9000, BC.pack "0230"), (0x9003, dateTime), (0x927c, u32 0), (0xa001, u16 1), (0xa002, u32 4896), (0xa003, u32 3264), (0xa434, ascii "XF35mmF1.4 R")])
      , (2, [(0x0, BC.pack "0130"), (0x1000, padded 8 "FINE    "), (0x1001, u16 0), (0x1401, u16 0x200), (0x1430, padded 0x81 "")])
      , (5, [(0x0, B.pack [2, 2, 0, 0]), (0x1, ascii "S"), (0x1d, ascii "2026:11:20")])
      , (6, [(0x103, u16 6), (0x11a, rational 72 1)])
      ]
    typed kind (tag, bytes) =
      let valueType = maybe 1 id (lookup tag (tableRules plain kind))
       in Parsed tag valueType (fromIntegral (B.length bytes `div` max 1 (typeSize valueType))) bytes

-- | The directories of the Exif block at the start of @output@, read as TIFF: IFD0, Exif, the
-- maker note (offsets from the note), GPS when IFD0 points to it, and IFD1.
parseExif :: B.ByteString -> Either String [(Int, [Parsed])]
parseExif output = do
  unless (B.take 4 output == B.pack [0xff, 0xd8, 0xff, 0xe1]) (Left "SOI and APP1 markers differ")
  (primary, next) <- directoryAt 0 8
  exif <- pointed primary 0x8769 >>= fmap fst . directoryAt 0
  maker <- case [field | (Parsed 0x927c _ _ _, field) <- exif] of
    [field] -> fst <$> directoryAt (fromIntegral field) (fromIntegral field + 12)
    _ -> Left "Exif carries no single MakerNote"
  gps <- case [field | (Parsed 0x8825 _ _ _, field) <- primary] of
    [field] -> fst <$> directoryAt 0 (fromIntegral field)
    _ -> pure []
  (thumbnail, _) <- directoryAt 0 (fromIntegral next)
  pure [(0, map fst primary), (1, map fst exif), (2, map fst maker), (5, map fst gps), (6, map fst thumbnail)]
  where
    tiff = B.drop 12 output
    pointed directory tag = case [field | (Parsed t _ _ _, field) <- directory, t == tag] of
      [field] -> Right (fromIntegral field)
      _ -> Left ("IFD0 carries no single tag " ++ show tag)
    directoryAt :: Int -> Int -> Either String ([(Parsed, Word32)], Word32)
    directoryAt base offset = do
      count <- fromIntegral <$> halfAt offset :: Either String Int
      parsed <- mapM (\index -> entryAt base (offset + 2 + 12 * index)) [0 .. count - 1]
      next <- wordAt (offset + 2 + 12 * count)
      pure (parsed, next)
    entryAt base at = do
      tag <- halfAt at
      valueType <- halfAt (at + 2)
      count <- wordAt (at + 4)
      field <- wordAt (at + 8)
      let size = fromIntegral count * typeSize valueType
      bytes <-
        if size > 4
          then sliceAt (base + fromIntegral field) size
          else sliceAt (at + 8) 4
      pure (Parsed tag valueType count bytes, field)
    sliceAt at size
      | at < 0 || at + size > B.length tiff = Left ("value at " ++ show at ++ " lies outside the block")
      | otherwise = Right (B.take size (B.drop at tiff))
    halfAt :: Int -> Either String Word16
    halfAt at = (\bytes -> fromIntegral (B.index bytes 0) + 256 * fromIntegral (B.index bytes 1)) <$> sliceAt at 2
    wordAt :: Int -> Either String Word32
    wordAt at = (\bytes -> foldr (\index acc -> acc * 256 + fromIntegral (B.index bytes index)) 0 [0 .. 3]) <$> sliceAt at 4

typeSize :: Word16 -> Int
typeSize valueType = case valueType of
  3 -> 2
  4 -> 4
  5 -> 8
  8 -> 2
  9 -> 4
  10 -> 8
  _ -> 1

-- | The (tag, type) of each rule of a stock tag table, read from the plain image.
tableRules :: B.ByteString -> Int -> [(Word16, Word16)]
tableRules plain kind = [(half (rule + 0), half (rule + 4)) | index <- [0 .. count - 1], let rule = base + 32 * index]
  where
    (base, count) = case kind of
      0 -> (0x011d09dc, 18)
      1 -> (0x011d041c, 44)
      2 -> (0x011d0fbc, 154)
      5 -> (0x011d0c1c, 18)
      6 -> (0x011d0ebc, 8)
      _ -> (0, 0)
    half address = let offset = fromIntegral (address - 0x01100000 :: Word32) in fromIntegral (B.index plain offset) + 256 * fromIntegral (B.index plain (offset + 1))

-- | @blocks@ with the saved-parse state word at @0x014e9e50@ naming @state@, mapped at
-- 'savedBase'.
withSavedState :: B.ByteString -> [Block] -> [Block]
withSavedState state blocks =
  [if blockName b == "globals" then b {blockBytes = put (blockBytes b)} else b | b <- blocks] ++ [Block "saved" savedBase state]
  where
    put bytes = B.take 0x110 bytes <> u32 savedBase <> B.drop 0x114 bytes

-- Scenarios ------------------------------------------------------------------------

provenance :: String
provenance =
  "Synthetic capture shaped like an X-M1 1.50 JPEG (FUJIFILM X-M1, XC16-50mm, 1/250 s, f/5.6, ISO 400, 4896x3264); "
    ++ "values placed at the source-block offsets the retained tag tables at 0x011d041c..0x011d24e4 name; "
    ++ "identity text at 0x014e9d98 as the one-shot identity copy leaves it. No camera data."

call :: Word32 -> [Argument] -> Step
call entry arguments = Step entry arguments [] True False

advancing :: Word32 -> [Argument] -> Step
advancing entry arguments = Step entry arguments [] True True

headerStep :: Word32 -> Word32 -> Step
headerStep format option =
  Step 0x011db098 (map Literal [outputBase, primaryBase, exifBase, makerBase]) (map Literal [gpsBase, thumbnailBase, format, option]) True False

-- | A capture's source blocks.
data Capture = Capture
  { captureCopyright :: String
  , capturePrintMatching :: Word8
  , capturePrintBlock :: Word32
  , capturePrimary :: [(Int, B.ByteString)]
  , captureColorSpace :: Word16
  , captureExif :: [(Int, B.ByteString)]
  , captureFaces :: Faces
  , captureGroup :: Word8
  , captureMaker :: [(Int, B.ByteString)]
  , captureGps :: [(Int, B.ByteString)]
  }

filmGroup :: Word8
filmGroup = fromIntegral (fromEnum 'F')

plainCapture :: Capture
plainCapture = Capture "" 1 0 [] 1 [] noFaces filmGroup [] []

captureBlocks :: Capture -> [Block]
captureBlocks c =
  [ Block "globals" globalsBase globals
  , Block "output" outputBase (B.replicate outputBytes 0xcd)
  , Block "primary" primaryBase (primaryBlock (captureCopyright c) (capturePrintMatching c) (capturePrintBlock c) (capturePrimary c))
  , Block "exif" exifBase (exifBlock (captureColorSpace c) (captureExif c))
  , Block "gps" gpsBase (gpsBlock (captureGps c))
  , Block "thumbnail" thumbnailBase thumbnailBlock
  , Block "maker" makerBase (makerBlock (captureFaces c) (captureGroup c) (captureMaker c))
  ]

twoFaces, fieldsPresent, fieldsAbsent, fieldsMixed, maximumCapture, recognizedCapture, detectedCapture, staleCapture :: Capture
twoFaces = plainCapture {captureCopyright = "J. Doe", capturePrintMatching = 2, capturePrintBlock = 1, captureColorSpace = 0xffff, captureFaces = detectedFaces 2, captureGroup = 0}
fieldsPresent = plainCapture {captureCopyright = "ABC", capturePrintBlock = 1, capturePrimary = primaryChromaticity, captureExif = exifGamma, captureFaces = oneFace, captureGroup = 0, captureMaker = makerPresent, captureGps = gpsPresent}
fieldsAbsent = plainCapture {captureMaker = makerAbsent, captureGps = gpsAbsent}
fieldsMixed = plainCapture {captureCopyright = "J. Does", captureFaces = recognizedOnly, captureGroup = 0, captureMaker = makerMixed, captureGps = gpsMixed}
maximumCapture = plainCapture {captureCopyright = "M", captureFaces = maximumFaces, captureGroup = 0}
recognizedCapture = plainCapture {captureFaces = recognizedOnly}
detectedCapture = plainCapture {captureFaces = detectedOnly, captureGroup = 0}
staleCapture = plainCapture {captureFaces = staleFaces}

-- | The captures whose stock Exif block the parsed replays read back.
replaySources :: [String]
replaySources = ["capture-two-faces", "capture-fields-present"]

scenarios :: B.ByteString -> [Scenario]
scenarios plain =
  [ header "capture-no-gps" 0 0 "" plainCapture
  , header "capture-gps" 0 1 "" plainCapture
  , header "capture-two-faces" 0 1 "Two detected and recognized faces; ColorSpace uncalibrated." twoFaces
  , header "capture-fields-present" 0 1 "Every maker, IFD0, Exif and GPS filter field recorded; one face; four-byte Copyright." fieldsPresent
  , header "capture-fields-absent" 0 1 "Every maker and GPS filter field at its unset sentinel; empty Copyright and map datum." fieldsAbsent
  , header "capture-fields-mixed" 0 1 "Two-field filters decided by their second field; zero-count face fields; eight-byte Copyright." fieldsMixed
  , header "capture-faces-maximum" 0 0 "Ten detected faces and sixty recognition records, the maker block's array bounds, in every record shape." maximumCapture
  , header "capture-faces-recognized" 0 0 "Recognition records without detected faces." recognizedCapture
  , header "capture-faces-detected" 0 1 "Detected faces without recognition records." detectedCapture
  , header "capture-faces-stale" 0 0 "Face counts left from an earlier frame with the capture byte clear." staleCapture
  , saved "saved-replay-pass0" 1 1 "" (withSavedState (savedState plain 0) (captureBlocks plainCapture))
  , saved "saved-replay-pass1" 2 1 "" (withSavedState (savedState plain 1) (captureBlocks twoFaces {captureFaces = detectedFaces 1}))
  ]
    ++ [ saved ("saved-maker-" ++ name) 1 1 ("Maker block of capture-" ++ name ++ " against the synthetic pass-0 slots.") (withSavedState (savedState plain 0) (captureBlocks capture))
       | (name, capture) <- [("fields-present", fieldsPresent), ("fields-absent", fieldsAbsent), ("fields-mixed", fieldsMixed), ("faces-maximum", maximumCapture), ("faces-recognized", recognizedCapture), ("faces-detected", detectedCapture), ("faces-stale", staleCapture)]
       ]
    ++ [ Scenario
          { scenarioName = "capture-format-unknown"
          , scenarioProvenance = provenance ++ " Directory format 3, a value of the caller's format word (image_work+0xf2e8) outside 0..2: no directory body, and the pointer patches write through the zero slots, mapped here as a page at 0."
          , scenarioBlocks = Block "zero-page" 0 (B.replicate 0x100 0) : captureBlocks plainCapture
          , scenarioCursor = outputBase
          , scenarioSteps = [headerStep 3 1]
          , scenarioHeader = Nothing
          }
       , Scenario
          { scenarioName = "raw-exif-mode3"
          , scenarioProvenance = provenance ++ " The RAF writer's sequence at 0x011cb740..0x011cb7a0: initialize mode 3 with GPS option 1, IFD0, Exif, maker note and GPS directories chained, the thumbnail offset patch 0x011c1d18 with the bytes written and the thumbnail length patch."
          , scenarioBlocks = captureBlocks twoFaces
          , scenarioCursor = outputBase + 0x40
          , scenarioSteps =
              [ Step 0x011c14f0 (map Literal [3, 0, 0, 0]) [Literal 1] False False
              , advancing 0x011c170c [Literal primaryBase, Cursor]
              , advancing 0x011c17cc [Literal exifBase, Cursor]
              , advancing 0x011c19c8 [Literal makerBase, Cursor]
              , advancing 0x011c1b30 [Literal gpsBase, Cursor]
              , Step 0x011c1d18 [Advanced] [] False False
              , Step 0x011c1e4c [Literal 0x1d80] [] False False
              ]
          , scenarioHeader = Nothing
          }
       , mpf "mpf-first-image" "The MPF writer 0x011d3118 after a capture's Exif block: the index IFD 0x011c1d48 then the first image's attribute IFD 0x011c1d88, which patches its offset into the index." 0 [advancing 0x011c1d48 [Literal mpfIndexBase, Cursor]]
       , mpf "mpf-second-image" "The MPF writer 0x011d3118 after a capture's Exif block for a second image: the attribute IFD 0x011c1d88 alone." 1 []
       , Scenario
          { scenarioName = "saved-parser-transfer"
          , scenarioProvenance = "The saved-parser bank transfer 0x011c2154 as the overlay's parsers call it (0x011c2234, 0x011c268c, 0x011c8220, 0x011cb8e8): load bank 1, save banks 0 and 1, a bank argument of 2, a null parser and a direction of 2."
          , scenarioBlocks = [Block "globals" globalsBase transferGlobals, Block "parser" parserBase (block 0x40 0x5a [])]
          , scenarioCursor = 0
          , scenarioSteps =
              [ call 0x011c2154 (map Literal [parserBase, 1, 0])
              , call 0x011c2154 (map Literal [parserBase, 0, 1])
              , call 0x011c2154 (map Literal [parserBase + 0x20, 1, 1])
              , call 0x011c2154 (map Literal [parserBase, 2, 0])
              , call 0x011c2154 (map Literal [0, 0, 1])
              , call 0x011c2154 (map Literal [parserBase, 0, 2])
              ]
          , scenarioHeader = Nothing
          }
       , Scenario
          { scenarioName = "saved-directory-lookups"
          , scenarioProvenance = "The saved parse's slot and value lookups 0x011c35f8 and 0x011c3678 for directory kinds 0..8, as the parsers at 0x011c0d90 and 0x011c24f8 call them with a kind argument."
          , scenarioBlocks = [Block "parser" parserBase (block 0x20 0 [(0x14, u32 savedBase)])]
          , scenarioCursor = 0
          , scenarioSteps = concat [[call 0x011c35f8 [Literal kind, Literal parserBase], call 0x011c3678 [Literal kind, Literal parserBase]] | kind <- [0 .. 8]]
          , scenarioHeader = Nothing
          }
       , Scenario
          { scenarioName = "table-lookups"
          , scenarioProvenance = "The tag-table select and count 0x011cf854 and 0x011cf908 for kinds 0..9, as the parser at 0x011c05a4 calls them with its kind byte."
          , scenarioBlocks = []
          , scenarioCursor = 0
          , scenarioSteps = concat [[call 0x011cf854 [Literal kind], call 0x011cf908 [Literal kind]] | kind <- [0 .. 9]]
          , scenarioHeader = Nothing
          }
       , Scenario
          { scenarioName = "finish-patches-lengths"
          , scenarioProvenance = "Writer state as a written Exif block leaves it: APP1 length slot at output+4, IFD1 JPEGInterchangeFormatLength value slot at output+0x1f0; total 0x2a14 and thumbnail 0x1d80 bytes."
          , scenarioBlocks = [Block "globals" globalsBase finishGlobals, Block "output" outputBase (B.replicate outputBytes 0xcd)]
          , scenarioCursor = 0
          , scenarioSteps = [Step 0x011db188 [Literal 0x2a14, Literal 0x1d80] [] False False]
          , scenarioHeader = Nothing
          }
       ]
  where
    header name format option detail capture =
      Scenario
        { scenarioName = name
        , scenarioProvenance = provenance ++ sentence detail ++ " Directory format " ++ show format ++ ", GPS option " ++ show option ++ "."
        , scenarioBlocks = captureBlocks capture
        , scenarioCursor = outputBase
        , scenarioSteps = [headerStep format option]
        , scenarioHeader = Just (HeaderCall format option)
        }
    saved name format option detail blocks =
      Scenario
        { scenarioName = name
        , scenarioProvenance = provenance ++ sentence detail ++ " Synthetic saved-parse slots for the retained tables. Directory format " ++ show format ++ ", GPS option " ++ show option ++ "."
        , scenarioBlocks = blocks
        , scenarioCursor = outputBase
        , scenarioSteps = [headerStep format option]
        , scenarioHeader = Just (HeaderCall format option)
        }
    mpf name detail later index =
      Scenario
        { scenarioName = name
        , scenarioProvenance = provenance ++ " " ++ detail ++ " MPF index and attribute blocks of a two-image MPO, their base 0x" ++ showHex mpfOutput "" ++ "."
        , scenarioBlocks =
            captureBlocks twoFaces
              ++ [ Block "mpf-index" mpfIndexBase (block 0x30 0 [(0x0, BC.pack "0100"), (0x4, u32 2), (0x8, u32 0x20030000 <> u32 0x2a14 <> u32 0 <> u32 0 <> u32 0x00020002 <> u32 0x1c000 <> u32 0x3000 <> u32 0), (0x28, u32 mpfOutput)])
                 , Block "mpf-attribute" mpfAttributeBase (block 0x30 0 [(0x0, BC.pack "0100"), (0x4, u32 (later + 1)), (0x8, u32 1), (0xc, rational 0 1), (0x14, rational 0 1), (0x1c, u32 later), (0x20, u32 mpfOutput)])
                 ]
        , scenarioCursor = mpfOutput + 8
        , scenarioSteps = [headerStep 0 1] ++ index ++ [advancing 0x011c1d88 [Literal mpfAttributeBase, Cursor]]
        , scenarioHeader = Just (HeaderCall 0 1)
        }
    sentence detail = if null detail then "" else ' ' : detail
    mpfOutput = outputBase + 0x6000
    transferGlobals = block 0x1c0 0 [(0x10c, u32 0x00611000), (0x110, u32 savedBase), (0x114, u32 0x00620000), (0x118, u32 0x00612000)]
    finishGlobals = block 0x1c0 0 [(0x10 + 0x2c, u32 (outputBase + 4)), (0x10 + 0x20, u32 (outputBase + 0x1f0))]

-- | Saved replays of passes 0 and 1 over the saved-parse state read back from @output@, the
-- stock Exif block of @source@, on @source@'s blocks.
parsedReplays :: B.ByteString -> Scenario -> B.ByteString -> Either String [Scenario]
parsedReplays plain source output = do
  directories <- parseExif output
  let state = savedStateOf plain directories
      option = maybe 1 headerOption (scenarioHeader source)
      replay pass =
        Scenario
          { scenarioName = scenarioName source ++ "-parsed-pass" ++ show pass
          , scenarioProvenance = "Saved replay of the Exif block the stock role wrote for " ++ scenarioName source ++ " in this run, read back as TIFF into the saved-parse slots (count, value offset, type) of every retained-table rule whose tag it carries; " ++ scenarioName source ++ "'s source blocks. Directory format " ++ show (pass + 1) ++ ", GPS option " ++ show option ++ "."
          , scenarioBlocks = withSavedState state (scenarioBlocks source)
          , scenarioCursor = outputBase
          , scenarioSteps = [headerStep (pass + 1) option]
          , scenarioHeader = Just (HeaderCall (pass + 1) option)
          }
  pure [replay 0, replay 1]
