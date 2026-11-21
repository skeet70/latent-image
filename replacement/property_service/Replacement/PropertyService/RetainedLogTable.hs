{-# LANGUAGE GHC2021 #-}

-- | Image5 words of the 10000-halfword log-mantissa table [0x101e134, 0x1022f54) whose value or
-- decoded branch lies in retired property code, closed as consumed data. The table closes when its
-- bytes, its reader's bytes and the census over all seven images hold: every MOVW/MOVT pair, MOVT,
-- PC-relative load or ADR forming an address in the table, and every load of a literal holding one,
-- lies in the table, the bound reader or one of the bound constant uses below; no word equals the
-- table start. Words that decode as branches into the table and computed addresses are outside the
-- census.
--
-- Citations: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 xm1-150 function-ranges extents; no
-- cited function has matching C.
module Replacement.PropertyService.RetainedLogTable
  ( logTableClosures
  , logTableDependencies
  ) where

import Control.Monad (guard)
import Data.Bits (rotateR, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Placement
  ( Carrier (..)
  , GuestAddress (..)
  , Reference (..)
  , ScanDepth (..)
  , ScanHit (..)
  , ScanImage (..)
  , inCarrier
  , retiredHull
  , scanDepth
  , scanReferences
  )
import Replacement.PropertyService.RetainedClosure (ClosedRole (..), ClosedSite (..), RetainedDependency, extentMatches, imageFor)
import ServiceInventory (Image (..), imageWord)

data Extent = Extent ScanImage GuestAddress GuestAddress SHA.Sha256

-- | An instruction that forms an address in [low, high].
data Materialization = Materialization ScanImage GuestAddress Word32 Word32

-- | An instruction whose result equals a table address, its word, that value, and the code the
-- value flows through until no register or memory slot holds it.
data ConstantUse = ConstantUse
  { useImage :: ScanImage
  , useSite :: GuestAddress
  , useWord :: Word32
  , useValue :: Word32
  , useConsumers :: [Extent]
  }

logTableClosures :: [(ScanImage, Image)] -> [ClosedSite]
logTableClosures images = fromMaybe [] $ do
  guard (all (\label -> any ((== label) . fst) images) [minBound .. maxBound])
  image5 <- imageFor ScanImage5 images
  guard (all bound (table : reader : concatMap useConsumers constantUses))
  guard (all useHolds constantUses)
  guard (censusCloses images)
  pure
    [ ClosedSite ScanImage5 site word ConsumedData
    | (site@(GuestAddress address), word) <- sites
    , imageWord image5 address == Just word
    ]
  where
    bound (Extent label start end digest) = maybe False (\image -> extentMatches image start end digest) (imageFor label images)
    useHolds use =
      let GuestAddress address = useSite use
       in (imageFor (useImage use) images >>= \image -> imageWord image address) == Just (useWord use)
            && not (inCarrier retiredHull (GuestAddress (useValue use)))

-- | The SDHC command register 0xfff5000c receives 0x01020000, which lies outside the retired bodies.
logTableDependencies :: [(ScanImage, Image)] -> [RetainedDependency]
logTableDependencies _ = []

sites :: [(GuestAddress, Word32)]
sites =
  [ (GuestAddress 0x101e1bc, 0x00630062)
  , (GuestAddress 0x101f378, 0x0bd80bd7)
  , (GuestAddress 0x10210dc, 0x1ad81ad7)
  ]

tableCarrier :: Carrier
tableCarrier = Carrier (GuestAddress 0x101e134) (GuestAddress 0x1022f54)

table :: Extent
table = extent ScanImage5 0x101e134 0x1022f54 "1ab89415fc4897f9a313cf108d90712bde4d68f1843146d75b38ac3d34217fcb"

-- | FUN_010033fc materializes the base at 0x100345c/0x1003460 and reads it once, 0x1003498 ldrh at
-- 2 * (q - 10000), q = 10000 * x / 2^floor(log2 x) from __aeabi_uldivmod; the halfword feeds mla
-- into the output word.
reader :: Extent
reader = extent ScanImage5 0x10033fc 0x10034b4 "5ac37fc364534d4a9cd1d4b1995b64777b14519a6f48a8ec503c3db3dffbfb2c"

constantUses :: [ConstantUse]
constantUses =
  [ -- FUN_006f2aec builds {0x01020000, 0, 16, 100, 0} at r11 - 0x28 and passes it to FUN_006f2614,
    -- which loads word 0 at 0x6f26d8 for FUN_006de5f0 (str r0 to 0xfff5000c; r0 reloaded at
    -- 0x6f26e0) and at 0x6f272c for the compare with 0x133a0010 (r2 rewritten by every callee of
    -- sdhc_disable_command_interrupts before use). The block pointer reaches
    -- nand_arm_controller_event -> dispatch_current_task -> [loader A 0x4004dae0 + 0x1618], which
    -- kernel_publish_interfaces sets to kernel_core_index_adapter -> FUN_000427b8; neither reads r0.
    -- The jump-table callees at [0x6f1df4, 0x6f2164) receive no block field.
    ConstantUse ScanImage4 (GuestAddress 0x6f2b08) 0xe3403102 0x01020000
      [ extent ScanImage4 0x6f2aec 0x6f2b60 "23dfc3ba8c704e7223e2e690cef390439b7c14915299e1e7b967dc115079f57e"
      , extent ScanImage4 0x6f2614 0x6f2788 "4dd0c8af153c85e13ba7ea280cfb82c78b7bcbbe2dbe4391e5e4c8e1dd5da72c"
      , extent ScanImage4 0x6de5f0 0x6de60c "7d2caa77e73828ba7211ae39b861ba9ae713f676ac647784c2de7446ad42e652"
      , extent ScanImage4 0x6f1df4 0x6f2164 "0d64f46551417a3c0df0eefc7f58210e5224ffdaf71b9ef37b8a6809b76a6754"
      , extent ScanImage4 0x6f2164 0x6f21c8 "36b5c4d7ca46bae85f67bab252151cd392d583e251230e642e633a6c090fceba"
      , extent ScanImage4 0x6de7b8 0x6de948 "f9339f2ffd6d3d5abf784ed331b14868ea87b710a81786aebf2b215b0a637cc0"
      , extent ScanImage4 0x6ee210 0x6ee230 "e1cef80dba59248da4db227449429432045226df79cadba77e36c073193d64e1"
      , extent ScanImage4 0x723cb8 0x723ce8 "92681accf48089835d3c844a4fd99a7c3211ea1009cccd0f88273ce3de93a245"
      , extent ScanLoaderA 0x40430 0x409ac "28c0323505b8d766e3fb86386620e6a399ea767ef30b3d39b771a184c6d89806"
      , extent ScanLoaderA 0x412bc 0x412cc "588a8f7ceca8c41d2ca7986a688a72e3d05cfc3ebce9547046e1b14ac8486b48"
      , extent ScanLoaderA 0x427b8 0x427d4 "304e0eafee9b09b251f9f6c6fb9590a31ef115f472cb51d17778e29e94418740"
      ]
  , -- initialization_report_mode sets r0 to 0x01020000 when r0 is 0 and traces it; diagnostic trace
    -- leaves r0, so FUN_0073e1ec (the only caller passing 0) carries it into FUN_0073d000 and
    -- FUN_0064e4ac, whose first call, kernel_save_irq_mask, overwrites r0. FUN_0073e25c passes 1.
    ConstantUse ScanImage4 (GuestAddress 0x73ce3c) 0x03400102 0x01020000
      ( [ extent ScanImage4 0x73ce30 0x73ce50 "9894d1ea15429e4d824aa7f45d791a66f39779aa8f1e707685ac20a02e72bf19"
        , extent ScanImage4 0x73e1ec 0x73e208 "e66434137b9a9f17f8f01f46aea8e803ac0b118b44f2f8b7f40a1181cd72a664"
        , extent ScanImage4 0x73e25c 0x73e370 "c4eb0b21aaa41dce061487dd16bb3030baf35a43f70ea90a56eeb1187b7642d2"
        , extent ScanImage4 0x73d000 0x73d0bc "3392a598a6cbdf3145c97043d3beeb3a3a14e2a63cf0f4b2f95d5073e28d5113"
        , extent ScanImage4 0x64e4ac 0x64e7a4 "fb58b4ce6e6b44cb8c798e464ebf8e2a892e6e83915ea57b97c112540218610d"
        , extent ScanLoaderA 0x42574 0x4257c "9ecc20d5831032d00dfdc522f17be27fef0593b866d49aac935d2ac351ba0f75"
        ]
          ++ traceRing
      )
  , -- syscont_register_transition_notifications traces the constant; FUN_00ec56b4 loads r0 at
    -- 0xec56c8 before reading it.
    ConstantUse ScanImage5 (GuestAddress 0xec2c3c) 0xe3400102 0x01020000
      ( [ extent ScanImage5 0xec2c30 0xec2c64 "3fcc4ab9a4904d65dad295685ec0e00ae7a0e8752e0a6c3f87bb8a70339b8304"
        , extent ScanImage5 0xec56b4 0xec57ec "753c7d27f97cf938bf416898a1c00fb35fbf5d0cb74ab600be471456d3c79fbf"
        ]
          ++ traceRing
      )
  , -- FUN_00ec797c traces the constant and reloads r0 at 0xec7df0 before reading it.
    ConstantUse ScanImage5 (GuestAddress 0xec7da4) 0xe3400102 0x01020000
      (extent ScanImage5 0xec797c 0xec7f90 "3c4c1465d1e7a294c8ab3eff23a81154370c735be2b96cd1c3b76413f5aa27cc" : traceRing)
  ]

-- | diagnostic_trace_operation stores r0 at entry + 0x24 of the ring [0x40a5c8b0]. Of the ring's
-- other users, FUN_007284ec initializes it, FUN_00ed3dac clears the count, and FUN_00ed3de4 passes
-- [ring, ring + 36 + 12 * count) to storage_write_file_async, which FUN_0071bfe0 posts as
-- write-file request 0x2c19; none loads an entry field.
traceRing :: [Extent]
traceRing =
  [ extent ScanImage4 0x72872c 0x7287a4 "1a82c880495fcac5a7d78559761026dc8b5af7d2e006692bfd9123562cc57cda"
  , extent ScanImage4 0x7284ec 0x72872c "a11a03d5cc3c300072254b9df63c50de20c38d8452a1f654d9d8488edb4fb201"
  , extent ScanImage5 0xed3dac 0xed3de4 "99ca03cb85a12ae8064fefc6bcd82f24f7a5bd99df36addfb138712ef9adcc3f"
  , extent ScanImage5 0xed3de4 0xed42d4 "5f21605cef94275a04bbf1ff82fd153a16e6611a54df96958114477ff9ef146f"
  , extent ScanImage4 0x71c75c 0x71c780 "39948ee0cba419bb1f1a2649dae58e6ef5577339ace080f0e96e9f04f7948944"
  , extent ScanImage4 0x71bfe0 0x71c134 "7774cabce0dfd2808dbf5782d5230f67bb7bdaadf374d5d04f6452940bcc82cb"
  ]

censusCloses :: [(ScanImage, Image)] -> Bool
censusCloses images =
  all accounted materializations
    && notElem tableStart [word | ScanHit _ _ word WordInto <- hits]
  where
    Carrier (GuestAddress tableStart) (GuestAddress tableEnd) = tableCarrier
    hits = scanReferences tableCarrier images S.empty
    literals =
      [ (label, site, word)
      | ScanHit label site word WordInto <- hits
      , scanDepth label == InstructionsAndWords
      ]
    materializations =
      [ Materialization label site target target
      | ScanHit label site _ reference <- hits
      , target <- case reference of
          MovPairInto (GuestAddress built) -> [built]
          PcRelativeInto (GuestAddress read') -> [read']
          _ -> []
      ]
        ++ literalLoads images literals
        ++ concat [movtRanges label image | (label, image) <- images, scanDepth label == InstructionsAndWords]
    Extent _ readerStart readerEnd _ = reader
    accounted (Materialization label site low high) =
      high < tableStart
        || tableEnd <= low
        || (label == ScanImage5 && (inCarrier tableCarrier site || inCarrier (Carrier readerStart readerEnd) site))
        || any (\use -> useImage use == label && useSite use == site) constantUses

-- | PC-relative loads of the literals.
literalLoads :: [(ScanImage, Image)] -> [(ScanImage, GuestAddress, Word32)] -> [Materialization]
literalLoads images literals =
  [ Materialization label site word word
  | (label, image) <- images
  , let held = [(literal, word) | (owner, literal, word) <- literals, owner == label]
  , not (null held)
  , let window = Carrier (minimum (map fst held)) (after (maximum (map fst held)))
  , ScanHit _ site _ (PcRelativeInto target) <- scanReferences window [(label, image)] S.empty
  , Just word <- [lookup target held]
  ]
  where
    after (GuestAddress address) = GuestAddress (address + 4)

-- | Every MOVT whose result can lie in the table: its low half comes from the MOVW or MOV immediate
-- that last wrote the register, through register MOVs, within the 8 preceding words; otherwise the
-- whole 64 KiB its high half selects.
movtRanges :: ScanImage -> Image -> [Materialization]
movtRanges label (Image _ _ base bytes) =
  [ Materialization label (GuestAddress (base + 4 * fromIntegral index)) low high
  | index <- [0 .. count - 1]
  , let word = raw index
  , word `shiftR` 28 /= 15
  , word .&. 0x0ff00000 == 0x03400000
  , let top = wide word `shiftL` 16
        (low, high) = case lowHalf index (word `shiftR` 28) (destination word) of
          Just half -> (top .|. half, top .|. half)
          Nothing -> (top, top .|. 0xffff)
  , not (high < tableStart || tableEnd <= low)
  ]
  where
    Carrier (GuestAddress tableStart) (GuestAddress tableEnd) = tableCarrier
    count = B.length bytes `div` 4
    raw index = U.word32At bytes (4 * index)
    lowHalf index condition = walk (index - 1) (8 :: Int)
      where
        walk at remaining register
          | at < 0 || remaining == 0 = Nothing
          | sameCondition && word .&. 0x0ff0f000 == 0x03000000 .|. (register `shiftL` 12) = Just (wide word)
          | sameCondition && word .&. 0x0fef0000 == 0x03a00000 && destination word == register && immediate word < 0x10000 = Just (immediate word)
          | sameCondition && word .&. 0x0fef0ff0 == 0x01a00000 && destination word == register = walk (at - 1) (remaining - 1) (word .&. 15)
          | mayWrite word register = Nothing
          | otherwise = walk (at - 1) (remaining - 1) register
          where
            word = raw at
            sameCondition = word `shiftR` 28 == condition
    mayWrite word register =
      word .&. 0x0e000000 == 0x08000000
        || word .&. 0x0e000000 == 0x0a000000
        || word .&. 0x0ffffff0 == 0x012fff10
        || word .&. 0x0ffffff0 == 0x012fff30
        || destination word == register
        || (word `shiftR` 16) .&. 15 == register
    destination word = (word `shiftR` 12) .&. 15
    wide word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
    immediate word = (word .&. 0xff) `rotateR` fromIntegral (2 * ((word `shiftR` 8) .&. 15))

extent :: ScanImage -> Word32 -> Word32 -> String -> Extent
extent label start end digest = Extent label (GuestAddress start) (GuestAddress end) (either (error . ("retained log table digest: " ++)) id (SHA.parseSha256Hex digest))
