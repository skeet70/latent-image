{-# LANGUAGE GHC2021 #-}

-- | Image5 halfword tables whose bound readers load them only by LDRH. A table closes when its bytes
-- and its readers' bytes match and the census holds over all seven scanned images: every MOVW/MOVT
-- pair, MOVT, PC-relative load or ADR that forms an address in the table, and every load of a
-- literal holding one, lies in a bound reader, and no word equals a table object start. Words that
-- decode as branches into a table and computed addresses are outside the census.
module Replacement.PropertyService.RetainedImage5Tables
  ( image5TableClosures
  ) where

import Data.Bits (rotateR, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
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
  , scanDepth
  , scanReferences
  )
import Replacement.PropertyService.RetainedClosure (ClosedRole (..), ClosedSite (..), extentMatches, imageFor)
import ServiceInventory (Image (..), imageWord)

-- | The table a site's bytes belong to.
data TableRole
  = LogMantissa
  | AfCurve
  | AfScale
  | CaptureRecord

data Extent = Extent GuestAddress GuestAddress SHA.Sha256

data Table = Table
  { tableBytes :: Extent
  , tableObjects :: [Word32]
  , tableReaders :: [Extent]
  }

-- | An instruction that forms an address in [low, high].
data Materialization = Materialization ScanImage GuestAddress Word32 Word32

image5TableClosures :: [(ScanImage, Image)] -> [ClosedSite]
image5TableClosures images
  | not (all (\label -> any ((== label) . fst) images) [minBound .. maxBound]) = []
  | otherwise = case imageFor ScanImage5 images of
      Nothing -> []
      Just image5 ->
        [ ClosedSite ScanImage5 site expected ConsumedData
        | (site@(GuestAddress address), expected, role) <- sites
        , let table = roleTable role
        , all (bound image5) (tableBytes table : tableReaders table)
        , censusCloses table
        , imageWord image5 address == Just expected
        ]
  where
    bound image (Extent start end digest) = extentMatches image start end digest
    hits = scanReferences hull images S.empty
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
    censusCloses table =
      all (accounted table) materializations
        && not (or [word `elem` tableObjects table | ScanHit _ _ word WordInto <- hits])
    accounted table (Materialization label site low high) =
      let Extent (GuestAddress start) (GuestAddress end) _ = tableBytes table
      in high < start || end <= low
           || (label == ScanImage5 && any (\(Extent from to _) -> inCarrier (Carrier from to) site) (tableReaders table))

-- | [0xf56280, 0x100a4b8) spans every table below.
hull :: Carrier
hull = Carrier (GuestAddress 0xf56280) (GuestAddress 0x100a4b8)

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

-- | Every MOVT whose result can lie in the hull: its low half comes from the MOVW or MOV immediate
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
  , not (high < hullStart || hullEnd <= low)
  ]
  where
    Carrier (GuestAddress hullStart) (GuestAddress hullEnd) = hull
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

sites :: [(GuestAddress, Word32, TableRole)]
sites =
  [ (GuestAddress 0x1009d74, 0x00630062, LogMantissa)
  , (GuestAddress 0xf56510, 0x00630061, AfCurve)
  , (GuestAddress 0xf56768, 0x00630062, AfCurve)
  , (GuestAddress 0xf57e6c, 0x00630060, AfScale)
  , (GuestAddress 0xfe69a4, 0x00630062, CaptureRecord)
  ]

-- Readers are x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 xm1-150 function-ranges extents.
roleTable :: TableRole -> Table
roleTable role = case role of
  -- 1000 halfwords. FUN_01003348 (no C) materializes the base at 0x10033a8 and reads it once,
  -- 0x10033e0 ldrh at 2 * (q - 1000), q = 1000 * x / 2^floor(log2 x) from 0x89bed0; the halfword
  -- feeds mla into the output word.
  LogMantissa ->
    Table
      (extent 0x1009ce8 0x100a4b8 "8d7958e50086442515af8c649b1d81614ab61aadf598060ee0d76aa7d23226b2")
      [0x1009ce8]
      [extent 0x1003348 0x10033fc "c65a2d485338735da932ed981b52ef99a48f375ce775bca104b4396a62fbdd6c"]
  -- Three 0x200-byte halfword curves. FUN_00f53498 (NONMATCHING, app/control/af/workflow/
  -- tu_00f4f16c.c) keeps base + 0x200 * pass at r11 - 0x4c; its only uses are the ldrh at 0xf53764
  -- (through r11 - 0x50) and 0xf5376c, whose values go to 0xee99f0 as arguments. FUN_00fe6cdc
  -- (NONMATCHING, app/capture/pipeline/tu_00fe3e48.c) loads literal 0xf5687f at 0xfe6d3c and reads
  -- bytes [0xf56880, 0xf568c0) by pre-indexed ldrb until r5 reaches 0xf568bf.
  AfCurve ->
    Table
      (extent 0xf56280 0xf56880 "fa3ad1d1360814853d17851d471995b91ac0950fe3ee35a3333dceedf7507748")
      [0xf56280, 0xf56480, 0xf56680]
      [afPass, captureLiteral]
  -- Three 0x200-byte halfword curves. FUN_00f53498 keeps base + 2 * index at r11 - 0x38 (ldrh at
  -- 0xf53708) and base + 0x200 * pass at r11 - 0x4c as above. FUN_00f55aec (NONMATCHING, same file)
  -- holds base + 2 * index in r6 for the ldrh at 0xf55bf0, 0xf55c14 and 0xf55c38 until 0xf55d60
  -- replaces it.
  AfScale ->
    Table
      (extent 0xf57c10 0xf58210 "55b751805318d74348ad5e30393bb741b3424a664618e733cd15731e8f3e7579")
      [0xf57c10, 0xf57e10, 0xf58010]
      [afPass, extent 0xf55aec 0xf55e98 "2249e07fa4a9b8d1ddc45ee47c2259fab03b942ab55fce971eb6fe2069acc7e9"]
  -- 64 records of four halfwords. FUN_00fe4ea4 (no C) keeps the base at r11 - 0x40 and base + 6
  -- at r11 - 0x48; 0xfe515c reads record r4 by ldrh until the end pointer and indexes a byte table
  -- with each halfword.
  CaptureRecord ->
    Table
      (extent 0xfe68ec 0xfe6aec "145450cf37e51f1ca6cfd44d417f5fb66780d97995185f2e8835bf8019109082")
      [0xfe68ec, 0xfe68f4 .. 0xfe6ae4]
      [extent 0xfe4ea4 0xfe52c0 "97a7e24f3bbeb3848c099a1995e9c1f4cf5d0eeeb2017514d271059365aae87d"]
  where
    afPass = extent 0xf53498 0xf53850 "4dd7e3541f78d4b8253834b173f53bb86aa6cbe1a6f3de13ce103e54c79dd4c2"
    captureLiteral = extent 0xfe6cdc 0xfe7300 "082e991829eb313ec88cc8b34c39e19ed4a55553c7380a9930a46548b6e0ed50"

extent :: Word32 -> Word32 -> String -> Extent
extent start end digest = Extent (GuestAddress start) (GuestAddress end) (either (error . ("retained image5 table digest: " ++)) id (SHA.parseSha256Hex digest))
