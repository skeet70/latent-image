{-# LANGUAGE GHC2021 #-}

-- | Plain-image table words whose value lies in retired property code, closed as consumed data. A
-- table closes only when its bytes, its readers' bytes and the reference census all hold: every
-- MOVW/MOVT materialization of a table address is a bound reader's, no scanned word equals a record
-- start, no branch lands in the table, and no PC-relative load from outside the table reads a site.
-- Computed addresses without a materialized base are outside this census.
--
-- Citations: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, decomp/firmwares/xm1-150.
module Replacement.PropertyService.RetainedPlainTables
  ( plainTableClosures
  ) where

import Data.Bits (shiftR, testBit, (.&.))
import Data.Maybe (isJust)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (Carrier (..), GuestAddress (..), Reference (..), ScanHit (..), ScanImage (..), hitTarget, scanReferences)
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image, imageWord)

-- | How the bound readers consume the site's four bytes.
data FieldRole
  = -- | Signed halfword width and height of a 12-byte layout cell (`ldrsh` at +4, +6).
    LayoutCellExtent
  | -- | Characters of a NUL-terminated format string (`ldrb`, `ldrh`).
    FormatText
  | -- | Unsigned halfword fields of an 18-byte geometry record (`ldrh`).
    GeometryHalfwords
  | -- | Unsigned halfword parser actions (`ldrh`).
    ParserActions
  | -- | Byte fields of 6-byte conversion descriptors (`ldrb`).
    ConversionBytes

data Extent = Extent ScanImage GuestAddress GuestAddress SHA.Sha256

data Reader
  = HashedReader Extent
  | ExactReader ScanImage FunctionBinding

-- | Container, record stride, readers, MOVW sites of the readers' materializations, and sites.
data Table = Table Extent Word32 [Reader] [GuestAddress] [(GuestAddress, Word32, FieldRole)]

plainTableClosures :: [(ScanImage, Image)] -> [ClosedSite]
plainTableClosures images = case imageFor ScanPlain images of
  Nothing -> []
  Just plain ->
    [ ClosedSite ScanPlain site word ConsumedData
    | table@(Table _ _ _ _ sites) <- tables
    , tableBound images table
    , censusClear table hits
    , (site@(GuestAddress address), word, _) <- sites
    , imageWord plain address == Just word
    ]
  where
    hits = scanReferences (Carrier (GuestAddress 0x1109a08) (GuestAddress 0x1445756)) images S.empty

tableBound :: [(ScanImage, Image)] -> Table -> Bool
tableBound images (Table bytes _ readers materializations sites) =
  extentBound images bytes
    && all (readerBound images) readers
    && all (\site -> any (readerHolds site) readers) materializations
    && all (\(site, _, _) -> extentHolds bytes site) sites

extentBound :: [(ScanImage, Image)] -> Extent -> Bool
extentBound images (Extent label start end digest) = maybe False (\image -> extentMatches image start end digest) (imageFor label images)

readerBound :: [(ScanImage, Image)] -> Reader -> Bool
readerBound images reader = case reader of
  HashedReader extent -> extentBound images extent
  ExactReader label binding -> maybe False (\image -> isJust (reachableInstructions image binding)) (imageFor label images)

readerHolds :: GuestAddress -> Reader -> Bool
readerHolds site reader = case reader of
  HashedReader extent@(Extent label _ _ _) -> label == ScanPlain && extentHolds extent site
  ExactReader label (FunctionBinding _ (GuestAddress entry) words') ->
    let GuestAddress address = site
    in label == ScanPlain && address >= entry && toInteger address < toInteger entry + 4 * toInteger (length words')

extentHolds :: Extent -> GuestAddress -> Bool
extentHolds (Extent _ start end _) site = start <= site && site < end

-- | A hit sited in the table's own bytes decodes table data, whose role the readers fix.
censusClear :: Table -> [ScanHit] -> Bool
censusClear (Table bytes@(Extent _ (GuestAddress start) _ _) stride _ materializations sites) hits =
  all accepted [hit | hit <- hits, extentHolds bytes (hitTarget hit), not (ownBytes hit)]
  where
    ownBytes hit = hitImage hit == ScanPlain && extentHolds bytes (hitSite hit)
    accepted hit = case hitReference hit of
      MovPairInto _ -> hitImage hit == ScanPlain && hitSite hit `elem` materializations
      WordInto -> (hitWord hit - start) `mod` stride /= 0
      PcRelativeInto (GuestAddress target) -> maybe False (\width -> all (disjoint target width) sites) (literalLoadWidth (hitWord hit))
      BranchInto _ -> False
    disjoint target width (GuestAddress site, _, _) =
      toInteger target + width <= toInteger site || toInteger site + 4 <= toInteger target

-- | Bytes read by an A32 load with an immediate offset; Nothing for any other encoding.
literalLoadWidth :: Word32 -> Maybe Integer
literalLoadWidth word
  | word .&. 0x0e000000 == 0x04000000 && testBit word 20 = Just (if testBit word 22 then 1 else 4)
  | word .&. 0x0e400090 == 0x00400090 = case (testBit word 20, (word `shiftR` 5) .&. 3) of
      (True, 1) -> Just 2
      (True, 2) -> Just 1
      (True, 3) -> Just 2
      (False, 2) -> Just 8
      _ -> Nothing
  | otherwise = Nothing

tables :: [Table]
tables = [layoutCells, secondaryColorFormat, playbackGeometry, parserActions, conversionDescriptors]

-- | app/native_overlay44/ov44_tu_01130fe8.c, FUN_01132f04 (NONMATCHING), ov44_data_01109a08.
-- [0x1109a08,0x110b1f0) is 85 rows of six 12-byte cells {s16 x, y, width, height; u32 palette}.
-- 0x1132fc8, 0x1133044 and 0x11331ac form base + 72 * row + 12 * column with column in 0..4; the
-- cell is read by `ldrh` at +0, +2, `ldrsh` at +4, +6 and `ldr` at +8, which 0x11331dc passes to
-- 0x1137d2c. 0x110a120 is row 25, column 1, +4.
layoutCells :: Table
layoutCells =
  Table
    (container 0x1109a08 0x110b1f0 "d06b738cd01047421fc6be2ff46702c32f78f2411b2fad85ec5f281bdc44ae3f")
    12
    [ hashed ScanPlain 0x1132f04 0x1133344 "4a6e7666ea6115477d585f07dab9248104140d34c32d4935fffdc85b146312fb"
    , hashed ScanPlain 0x1137d2c 0x1137d60 "e0b5607f9a270ece47347e8c129fb8d7f2fd354631cb48f377aa67e6d6a30819"
    ]
    (map GuestAddress [0x1132fc8, 0x1133044, 0x11331ac])
    [(GuestAddress 0x110a120, 0x0062000f, LayoutCellExtent)]

-- | app/ui/render/ov44_overlay_drawing_api_rest.c, overlay_select_secondary_palette_color passes
-- "%ib" (.rodata.secondary_color_format) to app/ui/render/tu_011764c0.c, ov48b_parse_text_format
-- (NONMATCHING), whose format bytes reach ov48b_queue_dynamic_text mode 0, string_length and
-- ov48b_encode_byte_text (0x721f30, 0x1178c0c).
secondaryColorFormat :: Table
secondaryColorFormat =
  Table
    (container 0x1113bec 0x1113bf0 "c1a87a2ab270b1711e455df564d8a21bdbd1eed45459af27138f2dbbea0832ba")
    4
    [ ExactReader ScanPlain (FunctionBinding 0x1100000 (GuestAddress 0x1112d24) [0xe92d4800, 0xe28db004, 0xe1a01000, 0xe3030bec, 0xe3400111, 0xe3a02000, 0xeb019433, 0xe8bd8800])
    , hashed ScanPlain 0x1177e10 0x1178338 "d859a29677ee430f5fca0350653f86fc28b401fb6d44a41b25c7182b1ea77cd4"
    , hashed ScanPlain 0x117ab64 0x117ad6c "3246a26d34fe6f948286193f16158c2b819b4ed1bb4c5ac81e8d69b29520a961"
    , hashed ScanPlain 0x1178c0c 0x1178c7c "a228c9150cfab392f5d206890194075ab0df66e6301c11cfad77404df51b91be"
    , hashed ScanPlain 0x1178c7c 0x1178ce0 "1142226d402b05e3af9447824394e41d483788b720b4ed0af28792e24c3a3d04"
    , hashed ScanImage4 0x721f30 0x721f6c "23ccac29d026ef2fa8b6ffc7ed85c589ebe91c4e3569a770c9d1823102b72137"
    , hashed ScanImage4 0x722a14 0x722a50 "45733165c8f28871a99c6fa2e68fbb0c91855faf09b3869145f52cbc3da11ca4"
    ]
    [GuestAddress 0x1112d30]
    [(GuestAddress 0x1113bec, 0x00626925, FormatText)]

-- | app/native_overlay47/ov47_tu_012479b4.c (FUN_0124834c through FUN_01248c4c) and
-- ov47_tu_01249410.c (FUN_01249410, FUN_01249674, FUN_01249724), all NONMATCHING, blk_012481fc.
-- [0x12481fc,0x1248232) is three 18-byte records of nine u16 fields, indexed by 18 * mode and read
-- only by `ldrh`. 0x121df50, 0x121cac8, 0x12296b0 and 0x69f8fc receive a record address as a dead
-- argument register. 0x1248208 and 0x124822c are fields 6 and 7 of records 0 and 2.
playbackGeometry :: Table
playbackGeometry =
  Table
    (container 0x12481fc 0x1248232 "6f95529dd713e54e07d9f0586bb90d18602c274332ffb75b4c0b04163679a9f4")
    18
    [ hashed ScanPlain 0x124834c 0x12483bc "fbbfdf982ecc0f1ed971902101edaa91ead9fc78d6b7139e056d73f0a381fa73"
    , hashed ScanPlain 0x12485e4 0x1248638 "13f795fac3e6b21c1ccdad5fcf0a2104735b8416fa3a45ca4af2dae86d4d2977"
    , hashed ScanPlain 0x1248770 0x124886c "b9490b8dcd57293c6e6609d68bcb137e78f1958c7c93e424ebdb0716b52d2ee4"
    , hashed ScanPlain 0x124886c 0x1248944 "b0beb30d92f6d4b75624bb2ede88ce920745d64ed79fde02de2b97e44cbe298b"
    , hashed ScanPlain 0x1248944 0x1248a84 "08ce3b9ddefbb7d5513ba47d690e48b255329a73a7e60c3b28e49613bd3a2251"
    , hashed ScanPlain 0x1248c4c 0x1248cf8 "f9995d1e404fc9a3535b2050b994838f3108e4357ac1dd7af3fef8350f8c49ba"
    , hashed ScanPlain 0x1249410 0x1249674 "7e5fbde2381cf88badd536d317246e2ba6c43599ca317a101718b91b20dbfb32"
    , hashed ScanPlain 0x1249674 0x1249724 "3d0216b22f0de03815252a2fd60479f9cf7b61cec0625a69f6fc420969eaf446"
    , hashed ScanPlain 0x1249724 0x1249810 "2b764e43042139ebe2650042dd88cdcfbfa7cd6fb8877cbce2031306030f8771"
    , hashed ScanPlain 0x121df50 0x121e0d4 "8ac2e7cfd6e876e5565b2cff762725b338563c7424274570f42df5f0e8b7b896"
    , hashed ScanPlain 0x121cac8 0x121cae8 "bed13f34f19a3a18353e06905888ec23fbe8693ecaf8b1579354498db3ecdf20"
    , hashed ScanPlain 0x12296b0 0x12296c8 "aab533f64e7d3f50628f3b74c122bcfaae90f7a06e1282e0e04e4d03d9706543"
    , hashed ScanImage4 0x69f8fc 0x69f920 "0491e27ccb707594c468745512f2212604611381f49a88242f83241025c74966"
    ]
    (map GuestAddress [0x1248384, 0x12485f8, 0x12487e8, 0x12488a4, 0x12489d4, 0x1248c8c, 0x1249430, 0x12494e4, 0x1249580, 0x12496b0, 0x1249784])
    [ (GuestAddress 0x1248208, 0x00620080, GeometryHalfwords)
    , (GuestAddress 0x124822c, 0x00620080, GeometryHalfwords)
    ]

-- | app/third_party/sqlite/func/tu_0141ee70.c, FUN_014275b0 and tu_01428528.c, FUN_014321ac
-- (sqlite3Parser), both NONMATCHING. [0x1444948,0x1445556) is 0x607 u16 parser actions read by
-- `ldrh [base, index * 2]`; FUN_014275b0 bounds its index by 0x606.
parserActions :: Table
parserActions =
  Table
    (container 0x1444948 0x1445556 "3b154c8fb931d258b3eb444eac2060145ea75818eeca007be58dfbb0cafeadeb")
    2
    [ hashed ScanPlain 0x14275b0 0x14276a4 "cb507aba0f98e71f7a2a17821856916f2043f1013b10f11a2fd4d3d706d8a816"
    , hashed ScanPlain 0x14321ac 0x14355e0 "401215589ed672bda702a895ea537ccbdc21f430cc6c8bf3c4904f9c6f054726"
    ]
    (map GuestAddress [0x1427668, 0x1427690, 0x14354c4])
    [ (GuestAddress 0x1444ec8, 0x0062025a, ParserActions)
    , (GuestAddress 0x1445094, 0x0063025a, ParserActions)
    ]

-- | app/connectivity/remote/transfer/tu_013f9414.c, FUN_0140da1c (NONMATCHING), g_014456e4.
-- [0x14456e4,0x1445756) is 19 six-byte conversion descriptors {type, base, flags, kind, charset,
-- prefix}, read only by `ldrb`. The descriptor address reaches the bound callees as a dead argument
-- register, and r0 on one return path, which sqlite3VMPrintf, sqlite_format_arguments and
-- sqlite_format_bounded discard. 0x144570c holds the charset and prefix of 'w' and the type and
-- base of 'c'.
conversionDescriptors :: Table
conversionDescriptors =
  Table
    (container 0x14456e4 0x1445756 "f2e4eab5af9bb303d5c31bfca4a1903c9ac83e6cf51912586c460f9312d3b47f")
    6
    [ hashed ScanPlain 0x140da1c 0x140e4fc "acd63da90e6898e545e424755b882598f6acba3e18195cee90dabe06b7c2ae49"
    , hashed ScanPlain 0x140cf84 0x140cfa0 "5cf4e5525e4b09ac49ddeeee54f0b682522236eb8c3e4e0c03c1ee10d9603eae"
    , hashed ScanPlain 0x140d090 0x140d0f8 "88778432632274265f3042073cbf739a3d4211421ae8d0b912a8e39ed5f11001"
    , hashed ScanPlain 0x140d128 0x140d190 "4463ac24d7c9e46e1b9899926cf1ede0c480c8ef9efa059c12499d891b83909f"
    , hashed ScanPlain 0x140d83c 0x140d88c "3701de36d1fbedd69b2f301455ea957d8c21a8672e5de1410950d64a282fdeb5"
    , hashed ScanPlain 0x140d9bc 0x140da1c "d1606c10b128f0e71684932c87ce99d06026e8dea707efd7c044f51e1145fd68"
    , hashed ScanPlain 0x140e4fc 0x140e560 "3673cc703f7599efdf330b6708cbd342721836ada9c53a0bd6992c9595a817c4"
    , hashed ScanPlain 0x140e5d0 0x140e620 "4d81d01c30717c51123b4a9ac508dea66a654b70037dfba9f1b3b212e3ee04bf"
    , hashed ScanPlain 0x140e620 0x140e678 "781f2b6030f63a831737b2dd9a7e1415bc0fa1ba5583eb664946eef48973024f"
    ]
    [GuestAddress 0x140da50]
    [(GuestAddress 0x144570c, 0x00630000, ConversionBytes)]

container :: Word32 -> Word32 -> String -> Extent
container start end digest = Extent ScanPlain (GuestAddress start) (GuestAddress end) (reviewedHash digest)

hashed :: ScanImage -> Word32 -> Word32 -> String -> Reader
hashed label start end digest = HashedReader (Extent label (GuestAddress start) (GuestAddress end) (reviewedHash digest))

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained plain table digest: " ++)) id . SHA.parseSha256Hex
