{-# LANGUAGE GHC2021 #-}

-- | Image5 image-processor words whose only CPU consumer is an MMIO store. A site yields a
-- dependency when its container, readers and the trace chain match their digests and the census
-- holds over all seven scanned images: every MOVW/MOVT pair, MOVT, PC-relative load or ADR that
-- forms an address in a container, and every load of a literal holding one, lies in a bound reader
-- of that container or inside the container's own bytes; no word equals a container object start;
-- and every word equal to a premise register lies in a bound container.
module Replacement.PropertyService.RetainedImageProcessor
  ( imageProcessorDependencies
  ) where

import Data.Bits (complement, rotateR, shiftL, shiftR, (.&.), (.|.))
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
import Replacement.PropertyService.RetainedClosure
  ( DevicePremise (..)
  , MmioRegister (..)
  , RetainedDependency (..)
  , extentMatches
  , imageFor
  )
import ServiceInventory (Image (..), imageWord)

data Extent = Extent ScanImage GuestAddress GuestAddress SHA.Sha256

data Container = Container
  { containerBytes :: Extent
  , containerObjects :: [Word32]
  , containerReaders :: [Extent]
  }

-- | An instruction that forms an address in [low, high].
data Materialization = Materialization ScanImage GuestAddress Word32 Word32

imageProcessorDependencies :: [(ScanImage, Image)] -> [RetainedDependency]
imageProcessorDependencies images
  | not (all (\label -> any ((== label) . fst) images) [minBound .. maxBound]) = []
  | not (all bound (traceChain ++ concatMap (\c -> containerBytes c : containerReaders c) containers)) = []
  | not (all censusCloses containers) = []
  | not (all registerWordsBound premiseRegisters) = []
  | otherwise = case imageFor ScanImage5 images of
      Nothing -> []
      Just image5 ->
        [ RetainedDependency ScanImage5 site expected [GuestAddress (expected .&. complement 3)] premise
        | (site@(GuestAddress address), expected, premise) <- sites
        , imageWord image5 address == Just expected
        , any (\body -> inCarrier body (GuestAddress expected)) retiredBodies
        ]
  where
    bound (Extent label start end digest) = maybe False (\image -> extentMatches image start end digest) (imageFor label images)
    hits = concat [scanReferences (carrierOf container) images S.empty | container <- containers]
    literals =
      [ (label, site, word)
      | ScanHit label site word WordInto <- hits
      , scanDepth label == InstructionsAndWords
      ]
    materializations =
      [ Materialization label site target target
      | ScanHit label site _ reference <- hits
      , GuestAddress target <- case reference of
          MovPairInto built -> [built]
          PcRelativeInto read' -> [read']
          _ -> []
      ]
        ++ literalLoads images literals
        ++ concat [movtRanges label image | (label, image) <- images, scanDepth label == InstructionsAndWords]
    censusCloses container =
      all (accounted container) materializations
        && not (or [word `elem` containerObjects container | ScanHit _ _ word WordInto <- hits])
    accounted container (Materialization label site low high) =
      let Extent owner (GuestAddress start) (GuestAddress end) _ = containerBytes container
      in high < start || end <= low
           || (label == owner && inCarrier (carrierOf container) site)
           || any (\(Extent reader from to _) -> label == reader && inCarrier (Carrier from to) site) (containerReaders container)
    registerWordsBound (MmioRegister register) =
      and
        [ any (\container -> label == ScanImage5 && inCarrier (carrierOf container) site) containers
        | ScanHit label site _ WordInto <- scanReferences (Carrier (GuestAddress register) (GuestAddress (register + 1))) images S.empty
        ]

carrierOf :: Container -> Carrier
carrierOf container = let Extent _ start end _ = containerBytes container in Carrier start end

retiredBodies :: [Carrier]
retiredBodies =
  [ Carrier (GuestAddress 0x61e114) (GuestAddress 0x6245f8)
  , Carrier (GuestAddress 0x624698) (GuestAddress 0x6309c4)
  ]

premiseRegisters :: [MmioRegister]
premiseRegisters = [firstTableRegister, windowRegister]

firstTableRegister :: MmioRegister
firstTableRegister = MmioRegister 0xfff14020

windowRegister :: MmioRegister
windowRegister = MmioRegister 0xfff14068

-- | Halfword pairs of image_processor_first_table; image_window_state.first[0].second.
sites :: [(GuestAddress, Word32, DevicePremise)]
sites =
  [ (GuestAddress 0x10701c4, 0x00630064, DeviceReadTable firstTableRegister)
  , (GuestAddress 0x10701c8, 0x00630063, DeviceReadTable firstTableRegister)
  , (GuestAddress 0x10701cc, 0x00630063, DeviceReadTable firstTableRegister)
  , (GuestAddress 0x10701d0, 0x00620062, DeviceReadTable firstTableRegister)
  , (GuestAddress 0x10701d4, 0x00620062, DeviceReadTable firstTableRegister)
  , (GuestAddress 0x1065e98, 0x00620004, MmioStoreOnly windowRegister)
  ]

-- Readers are x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 xm1-150 function-ranges extents.
containers :: [Container]
containers =
  [ -- image_processor_first_table, 1024 words. read_image_first_table_address (exact) builds the
    -- base at 0x106f3d0/0x106f3d4 and stores it to the caller's slot. Each of the eight functions
    -- below passes 0x01070000 in r0 from mov/movt to a bl; through 0xdd305c/0xdd3080,
    -- rec_trace_operation(_begin) and diagnostic_trace_operation its only use is 0x728794
    -- `str r0, [r1, #0x24]` into a trace-ring record, which FUN_00ed3de4 hands whole to
    -- storage_write_file_async.
    Container
      (extent ScanImage5 0x106f3e8 0x10703e8 "9dc7285dc6366b51b6bf4fa1ea13f59129f74e565fc136ccee22600e416dc567")
      [0x106f3e8]
      [ extent ScanImage5 0x106f3c8 0x106f3e8 "59aaee373805eb868bbf30b4e319bd0655b03822c087fecdfba266321c36fd90"
      , firstTableAddress
      , extent ScanImage4 0x73d9e4 0x73da20 "4d6e5b587bd54a06564db3a602fa4469a8ad67660265fe04319a10d408b65463"
      , extent ScanImage5 0xe37624 0xe37dc8 "994bc40e8d5728ce8125a897a3205c1dea1b9a336e1cb10fbb14dda2fc79434a"
      , extent ScanImage5 0xe37e44 0xe37f24 "4775e5ae67023ea52e25c05791b9320ec8592470fb826611362b18d86aa984ca"
      , extent ScanImage5 0xe37f24 0xe37fd8 "f02f5b1ac32c23dcb899cb0bca6741ac9c7a1a58ddd8a6a5503bf2faa630d6dd"
      , extent ScanImage5 0xe38120 0xe381f0 "9739b766a4319039c647b085cbf8393184cd6ba499d4dd3b322f6206ba2a2dca"
      , extent ScanImage5 0xe38374 0xe384b4 "94940417df5f3a6525f095b65f48bf21705b24f035c80a9e00ea509817a3bb01"
      , extent ScanImage5 0xe388ec 0xe38aa8 "04d0af2650b6974acbec8bf02dfc467d279210d6ad3e008e62450f3494e97964"
      ]
  , -- image_window_state: registers[8], first[3], second[3]. apply_image_processor_windows (exact)
    -- loads first/second[slot] words, slot from read_image_processor_slot, and stores each only to
    -- 0xfff14060..0xfff1407c; first[slot].second goes by 0x1064ff8 ldr and 0x1064ffc
    -- `str r12, [r3, #-0xf97]` with r3 = 0xfff14fff to 0xfff14068.
    Container
      (extent ScanImage5 0x1065e74 0x1065ef4 "b0247d10b57759e4981969b6cc79ffac6da7bddd279a90a43530059f6b34ee21")
      [0x1065e74]
      [ extent ScanImage5 0x1064fd0 0x1065038 "f50df2ae4ecd56cec2f96be80d90e19f3236ad220babb512d6b014388477affe"
      , extent ScanImage5 0x106bd84 0x106bda4 "063dd186f50532ff27528871aa2f5cbee356cc5f2d26a34eb8840b016dc9a3a5"
      ]
  , -- image_table_state {registers[2], tables[2]}. apply_image_processor_table_addresses (exact)
    -- stores the first table base to tables[0] at 0x1064c24 and through registers[0] = 0xfff14020
    -- at 0x1064c2c; it loads only registers[].
    Container
      (extent ScanImage5 0x1065ef4 0x1065f04 "3ab3e3a95cf32218bea703bf397940b9a8a4dab0bbf3164123acb609bcf8ac1b")
      [0x1065ef4]
      [firstTableAddress]
  ]
  where
    firstTableAddress = extent ScanImage5 0x1064c04 0x1064c50 "c24677c06f177b574703181cc8aca26da3287936cdc6c728c6af8049a8e125fd"

-- | The trace calls 0xdd305c and 0xdd3080, rec_trace_operation, rec_trace_operation_begin,
-- diagnostic_trace_operation and FUN_00ed3de4.
traceChain :: [Extent]
traceChain =
  [ extent ScanImage5 0xdd305c 0xdd3080 "a36e6e8fd1991754adc181030cf568ba0cd29e5f6453958ac4a443f28edec3cb"
  , extent ScanImage5 0xdd3080 0xdd30a4 "9188d16d512b756585e99dc4ddf226fb14fed4a343bee07086ce8c7de29a0a28"
  , extent ScanImage5 0xeb0cbc 0xeb0ccc "5db0d64b598e802abdd9faee23bdc2f48115c24b21c9c6d0ef434cfcff8219f5"
  , extent ScanImage5 0xeb0ccc 0xeb0ce0 "262403f9a188bf12af8f68ba9dd527a52c9a086484d6eafdf696d63896eba53b"
  , extent ScanImage4 0x72872c 0x7287a4 "1a82c880495fcac5a7d78559761026dc8b5af7d2e006692bfd9123562cc57cda"
  , extent ScanImage5 0xed3de4 0xed42d4 "5f21605cef94275a04bbf1ff82fd153a16e6611a54df96958114477ff9ef146f"
  ]

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

-- | [0x1065e74, 0x10703e8) spans every container.
hull :: Carrier
hull = Carrier (GuestAddress 0x1065e74) (GuestAddress 0x10703e8)

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

extent :: ScanImage -> Word32 -> Word32 -> String -> Extent
extent label start end digest = Extent label (GuestAddress start) (GuestAddress end) (either (error . ("retained image processor digest: " ++)) id (SHA.parseSha256Hex digest))
