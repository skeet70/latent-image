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
  , scenarios
  , globalsBase
  , outputBase
  , outputBytes
  ) where

import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word16, Word32, Word8)
import Replacement.FileService.Native (Call (..), Extent (..), word32le)

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

data Scenario = Scenario
  { scenarioName :: String
  , scenarioProvenance :: String
  , scenarioBlocks :: [Block]
  , scenarioCalls :: [Call]
  , scenarioHeader :: Maybe HeaderCall
  }

globalsBase, outputBase, primaryBase, exifBase, makerBase, gpsBase, thumbnailBase, savedBase :: Word32
globalsBase = 0x014e9d40
outputBase = 0x00400000
primaryBase = 0x00500000
exifBase = 0x00501000
gpsBase = 0x00502000
thumbnailBase = 0x00503000
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

primaryBlock :: String -> Word8 -> Word32 -> B.ByteString
primaryBlock copyright printMatching printBlock =
  block 0x200 0
    [ (0x70, rational 72 1), (0x78, rational 72 1)
    , (0x84, u16 1), (0x86, u16 2), (0x88, u16 2)
    , (0x9e, dateTime)
    , (0xb2, ascii copyright)
    , (0x15c, B.singleton printMatching), (0x160, u32 printBlock)
    , (0x164, B.pack [0x01, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x80, 0x80, 0x80, 0x00])
    , (0x1a8, B.singleton 1)
    ]

exifBlock :: Word16 -> B.ByteString
exifBlock colorSpace =
  block 0x400 0
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

-- | The maker-note fields of a capture: @faces@ detected faces (none means no detection data),
-- and the @0xe050@ byte that suppresses the @0x45xx@ group when it is @'F'@.
makerBlock :: Int -> Word8 -> B.ByteString
makerBlock faces group =
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
    , (0x1e8, u32 (if detected then 1 else 0)), (0x1f0, u16 (if detected then 1 else 0))
    , (0x1f2, B.singleton (if detected then 1 else 0))
    , (0x20a, u16 (if detected then fromIntegral (2 * faces) else 0xffff))
    , (0x222, u16 (fromIntegral faces)), (0x58a, u16 (fromIntegral faces))
    , (0x1d38, u32 (if detected then 1 else 0))
    , (0xe03c, u16 (fromIntegral faces)), (0xe03e, u16 0), (0xe040, u16 (fromIntegral faces)), (0xe042, u16 0)
    , (0xe044, u16 1), (0xe046, u16 1), (0xe048, u16 1), (0xe04a, u16 1), (0xe04c, u16 0), (0xe04e, u16 0)
    , (0xe050, B.pack [group, 0, 0, 0]), (0xe054, u16 1)
    , (0xe870, u32 (if detected then 1 else 0)), (0xe874, u32 0), (0xe878, u32 0), (0xe87c, u32 0)
    , (0xe880, rational 0 1), (0xe888, rational 0 1), (0xe890, u16 0), (0xe892, u16 0)
    , (0xe894, u16 0xffff), (0xe8a4, u32 0xffffffff), (0xe8a8, u16 0xffff), (0xe8e8, u32 0), (0xe8f0, u32 0xffffffff)
    ]
      ++ concat [faceFields index | detected, index <- [0 .. faces - 1]]
  where
    detected = faces > 0
    faceFields index =
      let record = 0x1d3c + 832 * index
       in [ (0x1f6 + 0, B.singleton 1), (0x1f7, B.singleton 0)
          , (0x20c + 8 * index, u16 (fromIntegral (1200 + 420 * index)) <> u16 900 <> u16 380 <> u16 380)
          , (0x224 + index, B.singleton 1), (0x238 + 8 * index, u16 1200 <> u16 900 <> u16 1580 <> u16 1280)
          , (0x58c + 8 * index, u16 (fromIntegral (1210 + 420 * index)) <> u16 910 <> u16 360 <> u16 360)
          , (record, u32 1), (record + 4, u32 1), (record + 8, u16 2)
          , (record + 10, padded 32 ("FACE" ++ show index)), (record + 0x2c, u32 0), (record + 0x30, padded 8 "")
          , (record + 0x38, u32 0), (record + 0x3c, u16 0)
          ]

gpsBlock :: B.ByteString
gpsBlock =
  block 0x100 0
    [ (0x0, B.pack [2, 2, 0, 0]), (0x4, ascii "N"), (0x8, rational 35 1 <> rational 39 1 <> rational 2940 100)
    , (0x20, ascii "E"), (0x24, rational 139 1 <> rational 44 1 <> rational 3060 100)
    , (0x3c, B.singleton 0), (0x40, rational 40 1), (0x48, rational 1 1 <> rational 30 1 <> rational 15 1)
    , (0x60, B.pack [0, 0, 0]), (0x63, ascii "A"), (0x65, ascii "2"), (0x68, rational 5 1)
    , (0x70, ascii "K"), (0x74, rational 0 1), (0x7c, ascii "T"), (0x80, rational 1 1 <> u32 0)
    , (0x88, ascii "WGS-84"), (0x94, ascii "2026:11:22")
    ]

thumbnailBlock :: B.ByteString
thumbnailBlock =
  block 0x40 0 [(0x0, rational 72 1), (0x8, rational 72 1), (0x28, u16 6), (0x2a, u16 1), (0x2c, u16 2), (0x2e, u16 2)]

-- | A parse the saved replay reads: per directory, a twelve-byte slot (count, value offset,
-- type) for each rule of the stock table, then the values of the tags the parsed file carried.
-- Pass 0 generates the Exif IFD pointer from a saved slot and pass 1 only from an empty one, so
-- the slot is present for pass 0 alone; the MakerNote and GPS pointers are always present.
savedState :: B.ByteString -> Int -> B.ByteString
savedState plain pass = block savedBytes 0 (concatMap directory saved)
  where
    exifPointer = [(0x8769, u32 0) | pass == 0]
    saved =
      [ (0, 0xffc, 0x10d4, [(0x10f, ascii "FUJIFILM"), (0x110, ascii "X-M1"), (0x112, u16 6), (0x11a, rational 72 1), (0x11b, rational 72 1), (0x128, u16 2), (0x132, dateTime), (0x213, u16 2), (0x8825, u32 0)] ++ exifPointer)
      , (1, 0x1c4c, 0x1e5c, [(0x829a, rational 1 60), (0x829d, rational 28 10), (0x8827, u16 1600), (0x9000, BC.pack "0230"), (0x9003, dateTime), (0x927c, u32 0), (0xa001, u16 1), (0xa002, u32 4896), (0xa003, u32 3264), (0xa434, ascii "XF35mmF1.4 R")])
      , (2, 0x250c, 0x2c44, [(0x0, BC.pack "0130"), (0x1000, padded 8 "FINE    "), (0x1001, u16 0), (0x1401, u16 0x200), (0x1430, padded 0x81 "")])
      , (5, 0xa444, 0xa51c, [(0x0, B.pack [2, 2, 0, 0]), (0x1, ascii "S"), (0x1d, ascii "2026:11:20")])
      , (6, 0xa6f8, 0xa758, [(0x103, u16 6), (0x11a, rational 72 1)])
      ]
    directory (kind, slots, values, present) =
      let rules = tableRules plain kind
          placed = snd (foldl' place (0, []) [(index, tag, valueType, bytes) | (index, (tag, valueType)) <- zip [0 :: Int ..] rules, Just bytes <- [lookup tag present]])
          place (offset, done) (index, tag, valueType, bytes) =
            let size = typeSize valueType
                count = fromIntegral (B.length bytes `div` max 1 size)
             in (offset + B.length bytes + (B.length bytes .&. 1), done ++ [(index, tag, valueType, count, offset, bytes)])
       in concat
            [ [ (slots + 12 * index, u32 count <> u32 (fromIntegral offset) <> u16 valueType)
              , (values + offset, bytes)
              ]
            | (index, _, valueType, count, offset, bytes) <- placed
            ]

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

savedPointer :: B.ByteString
savedPointer = u32 savedBase

-- Scenarios ------------------------------------------------------------------------

provenance :: String
provenance =
  "Synthetic capture shaped like an X-M1 1.50 JPEG (FUJIFILM X-M1, XC16-50mm, 1/250 s, f/5.6, ISO 400, 4896x3264); "
    ++ "values placed at the source-block offsets the retained tag tables at 0x011d041c..0x011d24e4 name; "
    ++ "identity text at 0x014e9d98 as the one-shot identity copy leaves it. No camera data."

headerCall :: Word32 -> Word32 -> Call
headerCall format option =
  Call 0x011db098 [outputBase, primaryBase, exifBase, makerBase] [gpsBase, thumbnailBase, format, option] True

captureBlocks :: B.ByteString -> String -> Word8 -> Word32 -> Word16 -> Int -> Word8 -> [Block]
captureBlocks globalsBytes copyright printMatching printBlock colorSpace faces group =
  [ Block "globals" globalsBase globalsBytes
  , Block "output" outputBase (B.replicate outputBytes 0xcd)
  , Block "primary" primaryBase (primaryBlock copyright printMatching printBlock)
  , Block "exif" exifBase (exifBlock colorSpace)
  , Block "gps" gpsBase gpsBlock
  , Block "thumbnail" thumbnailBase thumbnailBlock
  , Block "maker" makerBase (makerBlock faces group)
  ]

withSaved :: B.ByteString -> Int -> [Block] -> [Block]
withSaved plain pass blocks =
  [ if blockName b == "globals" then b {blockBytes = put (blockBytes b)} else b | b <- blocks ] ++ [Block "saved" savedBase (savedState plain pass)]
  where
    put bytes = B.take 0x110 bytes <> savedPointer <> B.drop 0x114 bytes

scenarios :: B.ByteString -> [Scenario]
scenarios plain =
  [ header "capture-no-gps" 0 0 (captureBlocks globals "" 1 0 1 0 (fromIntegral (fromEnum 'F')))
  , header "capture-gps" 0 1 (captureBlocks globals "" 1 0 1 0 (fromIntegral (fromEnum 'F')))
  , header "capture-two-faces" 0 1 (captureBlocks globals "J. Doe" 2 1 0xffff 2 0)
  , header "saved-replay-pass0" 1 1 (withSaved plain 0 (captureBlocks globals "" 1 0 1 0 (fromIntegral (fromEnum 'F'))))
  , header "saved-replay-pass1" 2 1 (withSaved plain 1 (captureBlocks globals "J. Doe" 2 1 0xffff 1 0))
  , Scenario
      { scenarioName = "finish-patches-lengths"
      , scenarioProvenance = "Writer state as a written Exif block leaves it: APP1 length slot at output+4, IFD1 JPEGInterchangeFormatLength value slot at output+0x1f0; total 0x2a14 and thumbnail 0x1d80 bytes."
      , scenarioBlocks = [Block "globals" globalsBase finishGlobals, Block "output" outputBase (B.replicate outputBytes 0xcd)]
      , scenarioCalls = [Call 0x011db188 [0x2a14, 0x1d80] [] False]
      , scenarioHeader = Nothing
      }
  ]
  where
    header name format option blocks =
      Scenario
        { scenarioName = name
        , scenarioProvenance = provenance ++ " Directory format " ++ show format ++ ", GPS option " ++ show option ++ "."
        , scenarioBlocks = blocks
        , scenarioCalls = [headerCall format option]
        , scenarioHeader = Just (HeaderCall format option)
        }
    finishGlobals = block 0x1c0 0 [(0x10 + 0x2c, u32 (outputBase + 4)), (0x10 + 0x20, u32 (outputBase + 0x1f0))]
