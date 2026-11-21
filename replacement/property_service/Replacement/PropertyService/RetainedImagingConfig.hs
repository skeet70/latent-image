{-# LANGUAGE GHC2021 #-}

-- | Image4 imaging-config record words. The seven 0xca8-byte records at 0x7c2dfc reach the CPU only
-- through the record pointer FUN_00d605d0 stores at sub-block + 0xcec; FUN_0069853c loads it and
-- reads the record by LDRH into the sub-block, FUN_00696db8 clamps each halfword to signed 11 bits
-- and packs pairs into the primary-table configuration at 0x14c6d10, and FUN_006deb08 copies those
-- words to the 0xfff44c00 register tables. A site's packed value is computed from its bytes; it
-- closes as consumed data when the value lies outside both retired bodies and otherwise yields an
-- MMIO-store dependency keeping the stock word the value names.
--
-- Everything yields nothing unless every bound extent matches and the census holds over all seven
-- images: every MOVW/MOVT pair, MOVT, PC-relative load or ADR that forms an address in the records,
-- and every load of a literal holding one, lies in a bound header or record reader or inside the
-- records; no word equals a record start; every immediate-offset load or store at offset 0xcec
-- other than from pc is the word load in FUN_0069853c or the word store in FUN_00d605d0; FUN_0069853c
-- loads through anything but its frame only the record pointer and halfwords; every word naming a
-- table register lies in the bound descriptor, no instruction forms one, and only the bound writer
-- and read-back form the table descriptor's address by MOVW/MOVT pair or PC-relative load.
--
-- Premises outside this census: computed addresses; immediate-offset word loads at record + 0xf20,
-- whose instances at image5 0xf3ef34, 0xf3f25c, 0xf40008, 0xf40420, 0xf404f8 (base 0x16c0fb8),
-- 0xfe25c0 and plain 0x11c3520 read other structures; copies of a publish record; branches.
module Replacement.PropertyService.RetainedImagingConfig
  ( imagingConfigClosures
  , imagingConfigDependencies
  ) where

import qualified A32 as A
import Control.Monad (guard)
import Data.Bits (complement, rotateR, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int16)
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word16, Word32)
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
import Replacement.PropertyService.RetainedClosure
  ( ClosedRole (..)
  , ClosedSite (..)
  , DevicePremise (..)
  , MmioRegister (..)
  , RetainedDependency (..)
  , extentMatches
  , imageFor
  )
import ServiceInventory (Image (..), imageWord)

data Extent = Extent ScanImage GuestAddress GuestAddress SHA.Sha256

-- | An instruction that forms an address in [low, high].
data Materialization = Materialization ScanImage GuestAddress Word32 Word32

-- | One 0x821a18 entry: the first register, the configuration offset and the word count.
data Descriptor = Descriptor Word32 Word32 Word32

-- | The packed value a site's bytes become and the register FUN_006deb08 stores it to.
data TableWrite = TableWrite
  { writeSite :: GuestAddress
  , writeWord :: Word32
  , writeValue :: Word32
  , writeRegister :: MmioRegister
  }

imagingConfigClosures :: [(ScanImage, Image)] -> [ClosedSite]
imagingConfigClosures images =
  [ ClosedSite ScanImage4 (writeSite write) (writeWord write) ConsumedData
  | write <- tableWrites images
  , not (retired (writeValue write))
  ]

imagingConfigDependencies :: [(ScanImage, Image)] -> [RetainedDependency]
imagingConfigDependencies images =
  [ RetainedDependency ScanImage4 (writeSite write) (writeWord write) [GuestAddress (writeValue write .&. complement 3)] (MmioStoreOnly (writeRegister write))
  | write <- tableWrites images
  , retired (writeValue write)
  ]

retired :: Word32 -> Bool
retired value = any (\body -> inCarrier body (GuestAddress value)) retiredBodies

retiredBodies :: [Carrier]
retiredBodies =
  [ Carrier (GuestAddress 0x61e114) (GuestAddress 0x6245f8)
  , Carrier (GuestAddress 0x624698) (GuestAddress 0x6309c4)
  ]

tableWrites :: [(ScanImage, Image)] -> [TableWrite]
tableWrites images = fromMaybe [] $ do
  guard (all (\label -> any ((== label) . fst) images) [minBound .. maxBound])
  image4 <- imageFor ScanImage4 images
  guard (all bound bindings)
  guard (recordCountHolds image4)
  guard (recordCensus images)
  guard (pointerFieldCensus images)
  guard (recordReadByHalfwords image4)
  guard (registerCensus images)
  descriptors <- mapM (descriptorAt image4) [0 .. descriptorEntries - 1]
  pure [write | (site, expected) <- sites, Just write <- [tableWrite image4 descriptors site expected]]
  where
    bound (Extent label start end digest) = maybe False (\image -> extentMatches image start end digest) (imageFor label images)

-- | Halfword pairs: record 0 group 16 pairs 15 and 24, record 3 group 6 pair 20, record 6 group 14
-- pairs 19 and 20.
sites :: [(GuestAddress, Word32)]
sites =
  [ (GuestAddress 0x7c3858, 0x0061ffe2)
  , (GuestAddress 0x7c387c, 0x0061ffe3)
  , (GuestAddress 0x7c5810, 0x0063016a)
  , (GuestAddress 0x7c8314, 0x0062ffc9)
  , (GuestAddress 0x7c8318, 0x00620156)
  ]

tableWrite :: Image -> [Descriptor] -> GuestAddress -> Word32 -> Maybe TableWrite
tableWrite image4 descriptors site@(GuestAddress address) expected = do
  word <- imageWord image4 address
  guard (word == expected && address >= recordBase && address + 4 <= recordsEnd)
  let offset = (address - recordBase) `mod` recordBytes
      group = offset `div` groupBytes
      within = offset `mod` groupBytes
  guard (within `mod` 4 == 0 && within < 4 * pairsPerGroup)
  Descriptor register _ _ <- case filter (feedsGroup group) descriptors of
    [entry] -> Just entry
    _ -> Nothing
  let (atZero, atTwo) = copiedHalves word
  pure (TableWrite site word (packed atZero atTwo) (MmioRegister (register + within)))
  where
    feedsGroup group (Descriptor _ configOffset count) = configOffset == configGroupOffset group && count == pairsPerGroup + 1

-- | FUN_0069853c stores a pair's halfword at +0 to copy + 2 and its halfword at +2 to copy + 0.
copiedHalves :: Word32 -> (Word16, Word16)
copiedHalves word = (fromIntegral (word `shiftR` 16), fromIntegral word)

-- | FUN_00696db8 sign-extends copy + 0 and copy + 2, clamps each through FUN_0068ef08 to
-- [-1024, 1023], masks 0x7ff and inserts them at bits 16 and 0.
packed :: Word16 -> Word16 -> Word32
packed atZero atTwo = (field atZero `shiftL` 16) .|. field atTwo
  where
    field half = fromIntegral (max (-1024) (min 1023 (fromIntegral (fromIntegral half :: Int16) :: Int))) .&. 0x7ff

recordBase, recordBytes, recordsEnd, headerBase :: Word32
recordBase = 0x7c2dfc
recordBytes = 0xca8
recordsEnd = 0x7c8694
headerBase = 0x7c2de0

-- | FUN_0069853c reads a record as 20 groups of 40 halfword pairs and one halfword; FUN_00696db8
-- writes group g to configuration offset 0x20 + 0xa4 * g.
groupBytes, pairsPerGroup :: Word32
groupBytes = 0xa2
pairsPerGroup = 40

configGroupOffset :: Word32 -> Word32
configGroupOffset group = 0x20 + 0xa4 * group

-- | FUN_00d605d0 indexes records by the product of the header halfwords at 0x7c2de2, 0x7c2de4 and
-- 0x7c2de6.
recordCountHolds :: Image -> Bool
recordCountHolds image4 = fromMaybe False $ do
  first <- imageWord image4 headerBase
  second <- imageWord image4 (headerBase + 4)
  let count = (first `shiftR` 16) * (second .&. 0xffff) * (second `shiftR` 16)
  pure (toInteger recordBase + toInteger count * toInteger recordBytes == toInteger recordsEnd)

-- | 0x6983f8 passes 0x821a18 with 28 entries to FUN_006deb08.
descriptorTable :: Word32
descriptorTable = 0x821a18

descriptorEntries :: Word32
descriptorEntries = 28

descriptorAt :: Image -> Word32 -> Maybe Descriptor
descriptorAt image4 index = do
  let entry = descriptorTable + 12 * index
  register <- imageWord image4 entry
  configOffset <- imageWord image4 (entry + 4)
  count <- imageWord image4 (entry + 8)
  pure (Descriptor register configOffset (count .&. 0xffff))

records :: Carrier
records = Carrier (GuestAddress headerBase) (GuestAddress recordsEnd)

recordObjects :: [Word32]
recordObjects = headerBase : [recordBase + recordBytes * index | index <- [0 .. 6]]

recordCensus :: [(ScanImage, Image)] -> Bool
recordCensus images =
  all accounted materializations
    && not (or [word `elem` recordObjects | ScanHit _ _ word WordInto <- hits])
  where
    hits = scanReferences records images S.empty
    literals = [(label, site, word) | ScanHit label site word WordInto <- hits, scanDepth label == InstructionsAndWords]
    materializations =
      [ Materialization label site target target
      | ScanHit label site _ reference <- hits
      , GuestAddress target <- case reference of
          MovPairInto built -> [built]
          PcRelativeInto read' -> [read']
          _ -> []
      ]
        ++ literalLoads images literals
        ++ concat [movtRanges records label image | (label, image) <- images, scanDepth label == InstructionsAndWords]
    accounted (Materialization label site low high) =
      let Carrier (GuestAddress start) (GuestAddress end) = records
      in high < start || end <= low
           || (label == ScanImage4 && inCarrier records site)
           || any (\(Extent reader from to _) -> label == reader && inCarrier (Carrier from to) site) recordReaders

pointerFieldCensus :: [(ScanImage, Image)] -> Bool
pointerFieldCensus images =
  and
    [ accepted label (GuestAddress site) instruction
    | (label, Image _ _ base bytes) <- images
    , scanDepth label == InstructionsAndWords
    , index <- [0 .. B.length bytes `div` 4 - 1]
    , let site = base + 4 * fromIntegral index
          instruction = A.decode site (U.word32At bytes (4 * index))
    , pointerField instruction
    ]
  where
    pointerField instruction = case instruction of
      A.Load _ _ _ _ base (A.OffsetImmediate 0xcec) _ -> base /= A.pc
      A.Store _ _ _ base (A.OffsetImmediate 0xcec) _ -> base /= A.pc
      _ -> False
    accepted label site instruction = case instruction of
      A.Load _ A.Word _ _ _ _ A.OffsetOnly -> inExtent label site recordReader
      A.Store _ A.Word _ _ _ A.OffsetOnly -> inExtent label site pointerWriter
      _ -> False

recordReadByHalfwords :: Image -> Bool
recordReadByHalfwords image4 =
  and
    [ case A.decode address word of
        A.Load _ A.Word _ _ _ (A.OffsetImmediate 0xcec) A.OffsetOnly -> True
        A.Load _ A.Half False _ _ (A.OffsetImmediate 0) A.OffsetOnly -> True
        A.Load _ _ _ _ base _ _ -> base == frame
        _ -> True
    | address <- [start, start + 4 .. end - 4]
    , Just word <- [imageWord image4 address]
    ]
  where
    Extent _ (GuestAddress start) (GuestAddress end) _ = recordReader
    frame = A.Reg 11

tableRegisters, descriptorEntriesCarrier :: Carrier
tableRegisters = Carrier (GuestAddress 0xfff44c00) (GuestAddress 0xfff46000)
descriptorEntriesCarrier = Carrier (GuestAddress descriptorTable) (GuestAddress (descriptorTable + 12 * descriptorEntries))

registerCensus :: [(ScanImage, Image)] -> Bool
registerCensus images =
  all registerHit (scanReferences tableRegisters images S.empty)
    && null (concat [movtRanges tableRegisters label image | (label, image) <- images, scanDepth label == InstructionsAndWords])
    && all descriptorHit (scanReferences descriptorEntriesCarrier images S.empty)
  where
    registerHit (ScanHit label site _ reference) = case reference of
      WordInto -> inExtent label site descriptor
      BranchInto _ -> True
      _ -> False
    descriptorHit (ScanHit label site _ reference) = case reference of
      MovPairInto _ -> any (inExtent label site) [registerWriter, registerReadback]
      PcRelativeInto _ -> any (inExtent label site) [registerWriter, registerReadback]
      BranchInto _ -> True
      WordInto -> False

inExtent :: ScanImage -> GuestAddress -> Extent -> Bool
inExtent label site (Extent owner start end _) = label == owner && inCarrier (Carrier start end) site

-- Extents are x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 xm1-150 function-ranges entries.

-- | FUN_00d60594 (0xd605ac) and FUN_00d609e0 form header halfword addresses read by LDRH;
-- FUN_00d605d0 forms them and the record base.
recordReaders :: [Extent]
recordReaders =
  [ extent ScanImage5 0xd60594 0xd605d0 "c8a60ce91f38671990f6fb6218be8b852bd6202a8ed1dfe4feae9d5b22cfff7b"
  , pointerWriter
  , extent ScanImage5 0xd609e0 0xd60b98 "4bae4eab486454cebbd47029abb319bebdb2b58ae73171bce569bf2388db3826"
  ]

-- | FUN_00d605d0 stores 0x7c2dfc + 0xca8 * index at 0xd60878 `str r3, [r5, #0xcec]`.
pointerWriter :: Extent
pointerWriter = extent ScanImage5 0xd605d0 0xd608a4 "9867e4318a7816e76ad2b3bafb5ec44c2ff964cf997c09f29dd8d43a6393cb52"

-- | FUN_0069853c: 0x698550 `ldr r3, [r3, #0xcec]`, then LDRH of the record.
recordReader :: Extent
recordReader = extent ScanImage4 0x69853c 0x6991dc "42ef815a6222b62d44e9a29f56058e1cb7a7a0f1bd0fa93c76062bb63a86d5eb"

-- | FUN_006983f8: FUN_006deb08 over 0x821a0c (control) and 0x821a18.
registerWriter :: Extent
registerWriter = extent ScanImage4 0x6983f8 0x698448 "ae9736bab29ac023afbcbddb74fa8c9574ad74e12f3cd523aa00957e0aa7bad7"

-- | xm1_image_capture_c6ec: FUN_006deb7c over 0x821a18 loads the registers into its argument.
registerReadback :: Extent
registerReadback = extent ScanImage4 0x6984e4 0x698510 "0bc741ebc96a086e09296dc13f6f4cb89bd10efb0abb064511c1e808e034360a"

-- | The descriptors [0x821a0c, 0x821b68).
descriptor :: Extent
descriptor = extent ScanImage4 0x821a0c 0x821b68 "10314f12c1ad10901b092b4fa2b84242575e6d455a696ae2728b8204412a5925"

bindings :: [Extent]
bindings =
  [ extent ScanImage4 headerBase recordsEnd "7074246429ca19d39cc3f13be510492648a0cb35b68ecb2d361bc5cde465d375"
  , recordReader
  , registerWriter
  , registerReadback
  , descriptor
  , -- image_pipeline_publish_record: record + 0x234 to [sp, #0x18].
    extent ScanImage5 0xd5da58 0xd5dae8 "436826b93a468e2f57c0b145f5ca55b7d8a3ab169b8cb66cc39a8d415cf3eae7"
  , -- image_pipeline_publish_configurations: flag 0x200 passes it with 0x14c6d10 to 0x698474.
    extent ScanImage5 0xda1f54 0xda208c "950092ada3a1c8206e5cd68652e719f2d41502e6a48b358d9801105676395dc2"
  , -- image_pipeline_reset_configurations: flag 0x200 passes 0x14c6d10 to 0x6984a4.
    extent ScanImage5 0xda208c 0xda21cc "68ed3ab60eeb7628ebd5563cac2dc5d5c45f6758f2bab71cb6a1b37621ad0ed8"
  , -- FUN_00698474: FUN_0069853c, then FUN_00696db8.
    extent ScanImage4 0x698474 0x6984a4 "bbdf7eeba6e0441d4e8b726a00386075260db71f610640c5cc642b50b4370e8d"
  , extent ScanImage4 0x696db8 0x6983f8 "ecd0378288074dcd4ae88eb3f6c59b4afb5ac9e22c90dcd69e4b5e0789c98da4"
  , extent ScanImage4 0x68ef08 0x68f050 "f10467522e4f6f366dab6a485d7e738a3c9db4a3dc124482ba205e58096d9779"
  , -- xm1_image_apply_c6ec_wrapper: 0x6983f8.
    extent ScanImage4 0x6984a4 0x6984c4 "e0c7d529a73012f7403a9156a39acd302c549595277e4e3265fc0d985da9c720"
  , -- FUN_006deb08: `ldr r5, [r12], #4` from configuration, `str r5, [r4], #4` to the register.
    extent ScanImage4 0x6deb08 0x6deb7c "35d3bfacf8a4dfad99cc0589b699d41d2004dba1d9886397eaf5def7caf89504"
  ]
    ++ recordReaders

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
movtRanges :: Carrier -> ScanImage -> Image -> [Materialization]
movtRanges hull label (Image _ _ base bytes) =
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

extent :: ScanImage -> Word32 -> Word32 -> String -> Extent
extent label start end digest = Extent label (GuestAddress start) (GuestAddress end) (either (error . ("retained imaging config digest: " ++)) id (SHA.parseSha256Hex digest))
