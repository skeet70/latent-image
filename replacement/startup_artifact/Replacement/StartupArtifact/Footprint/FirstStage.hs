{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the first-stage carriers that its qualification assumes,
-- the stores it makes and its one declared TCM region write. Its admission
-- (parseFirstStageArtifact, replaceRecordSlot, verifyStagedRecord) reads only
-- symbols and its own carriers. Own carriers are subtracted when the piece is
-- derived.
module Replacement.StartupArtifact.Footprint.FirstStage (footprint, premises, writes, writesPremise, tcmRegionWrite) where

import qualified A32 as A
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

footprint :: [Footprint]
footprint = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation)
  [ (RuntimePremise, Record1, 0x0, 0x20, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:28-29 (retained vector table)")
  , (RuntimePremise, Record1, 0x24, 0x40, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:30-31 (retained vector table)")
  , (RuntimePremise, Record1, 0x4228, 0x4300, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:34-35 (record configuration data retained)")
  , (RuntimePremise, Record1, 0x4300, 0x5280, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:36-37; replacement/first_stage/entry.S:82-88 (owned workspace and handoff words)")
  , (RuntimePremise, Record2, 0x10000, 0x103e8, "replacement/first_stage/src.rs (peer vector copy from record 2)")
  , (RuntimePremise, Record2, 0x10000, 0x20000, "replacement/first_stage/src.rs; replacement/secondary_stage/adapter.rs (record-2 code clears the peer cell 0x40020004 the first stage waits on)")
  , (RuntimePremise, LoaderA, 0x4000c, 0x40010, "emulation/Emulation/LoaderPrefixWitness.hs:681-683 (loader-A header entry, handoff target)")
  , (RuntimePremise, LoaderA, 0x40020, 0x40100, "emulation/Emulation/LoaderPrefixWitness.hs:142-143,757-760 (kernel vector installer)")
  , (RuntimePremise, LoaderA, 0x427b8, 0x427d4, "emulation/Emulation/LoaderPrefixWitness.hs:142-143,757-760 (kernel vector installer)")
  ]

premises :: [Premise]
premises = map Premise
  [ "the owned record-1 image zero-fills the stock body [0x1800,0x4228), retains the configuration data [0x4228,0x4300), and from its entry at 0x6000 reads the segment table, stages loaders A and B, decodes image 4 through the 0xfff43000 engine, fills the image bss, publishes the handoff words, tears down storage and clears the completion cell"
  , "the second core starts from record 2 and its vector table at 0x10000 after the peer release"
  ]

-- | Every store the owned first-stage code makes on core 0, physical unless
-- TCM. Core 1 runs the same entry; its stores are not enumerated here.
writes :: [WriteRange]
writes =
  [ WriteRange (TcmTarget Core0 InstructionBank) 0 0x2000
  , core0 0x4300 0x5280
  , core0 0x20000 0x20008
  , core0 0x42e4 0x42ec
  , core0 0xa0000 0xa0040
  , core0 0xa0040 0xa0084
  , core0 0xa00c4 0xa00c8
  , core0 0xa00c8 0xa00cc
  , core0 0x61c188 0x61c18c
  , core0 0x3fff0000 0x3fff03e8
  , core0 0x89ec00 0xbf1a00
  , core0 0xfff43000 0xfff43090
  , core0 0xfff52000 0xfff528ac
  , core0 0xfff88010 0xfff88014
  , core0 0xfff88024 0xfff88034
  , core0 0xfff88084 0xfff88088
  , core0 0xfff89000 0xfff893e0
  , core0 0xfff91000 0xfff9100c
  , core0 0xfffb2004 0xfffb2018
  , core0 0xfffb3000 0xfffb3c04
  , core0 0xfffb4000 0xfffb4c04
  ]
  where
    core0 = WriteRange (CorePhysicalTarget Core0)

writesPremise :: Premise
writesPremise = Premise $ T.intercalate "; "
  [ "ATCM [0,0x2000): the stm clear after the declared MCR and every frame on the SVC stack from 0x70000800 (replacement/first_stage/entry.S)"
  , "[0x4300,0x5280): the strlo clear, the skip table and saved controller words, and the handoff target and argument (entry.S; media.rs; images.rs)"
  , "peer cells [0x40020000,0x40020008) (src.rs peer_rendezvous and stage_next)"
  , "log pointer and core-0 index [0x400042e4,0x400042ec) (src.rs log_event and peer_rendezvous)"
  , "core-0 log slots [0xa0040,0xa0084): base 0x400a0040 as the first stage stores it and an even index from its canonical 0 (record-1 word 0x42e8) below 0x10 (src.rs log_event)"
  , "completion word 0xa00c4 (entry.S xm1_first_stage_completion_zero)"
  , "0x61c188 = [0x61c00c] + 4 with [0x61c00c] = 0x61c184 after images::segment_table loads the segment table (reports/firmware-extraction.md:245; images.rs segment_table; src.rs stage_next)"
  , "secondary SRAM [0x3fff0000,0x3fff03e8) through the core-0 view 0x7fff0000 (src.rs copy_peer_vectors)"
  , "image-4 bss [0x89ec00,0xbf1a00): whole words through the alias before the engine session (images.rs decompress; record-1 segment 4 bss 0x89ec00 length 0x352e00)"
  , "engine registers [0xfff43000,0xfff43090) and its clock gate 0xfff88010 (engine.rs Session open, submit and close)"
  , "NAND controller [0xfff52000,0xfff528ac) (media.rs storage_init, scan and read_units)"
  , "clock words [0xfff88024,0xfff88034), hold control 0xfff88084, pin banks [0xfff89000,0xfff893e0), timer words [0xfff91000,0xfff9100c), controller bank [0xfffb2004,0xfffb2018), interrupt controllers [0xfffb3000,0xfffb3c04) and [0xfffb4000,0xfffb4c04), sample snapshot [0x400a0000,0x400a0040) and 0x400a00c8 (media.rs initialize_peripherals, core_init and fail)"
  ]

-- | Core-0 ATCM at reset in the FujiMod EXR II catalog (catalog/soc/exr2/soc.json:24-27).
atcmReset :: Word32
atcmReset = 0xffff8011

-- | @entry.S:35@ loads this into r0 for the MCR.
atcmOwned :: Word32
atcmOwned = 0x70000011

-- | A32 @MCR p15,0,Rt,c9,c1,1@ (ATCM region) with cond and Rt masked out.
isAtcmRegionWrite :: Word32 -> Bool
isAtcmRegionWrite word = word .&. 0x0fff0fff == 0x0e090f31

-- | Any A32 @MCR p15,0,Rt,c9,c1,{0,1}@.
isTcmRegionWrite :: Word32 -> Bool
isTcmRegionWrite word = word .&. 0x0fff0fdf == 0x0e090f11

-- | The image's one TCM region write: an unconditional @mcr p15,0,r0,c9,c1,1@
-- right after @ldr r0,=0x70000011@, as the ATCM change from its reset value.
tcmRegionWrite :: LinkedBytes -> Either Text DeclaredMutation
tcmRegionWrite (LinkedBytes image symbols) = do
  rows <- either (Left . T.pack) Right (F.parseSymbols symbols)
  base <- either (Left . T.pack) (Right . F.symbolAddress) (F.lookupSymbol rows "__first_stage_start")
  let wordAt address
        | address >= base && address `mod` 4 == 0 && offset + 4 <= B.length image = Just (U.word32At image offset)
        | otherwise = Nothing
        where offset = fromIntegral (address - base)
      sites = [base + fromIntegral offset | offset <- [0, 4 .. B.length image - 4], isTcmRegionWrite (U.word32At image offset)]
  site <- case sites of
    [one] -> Right one
    _ -> Left (T.pack ("first-stage image has " <> show (length sites) <> " TCM region writes, not one"))
  unless' (maybe False ((== 0xee090f31)) (wordAt site) && maybe False isAtcmRegionWrite (wordAt site))
    "first-stage TCM region write is not an unconditional mcr p15,0,r0,c9,c1,1"
  let literal = case A.decode (site - 4) <$> wordAt (site - 4) of
        Just (A.Load (A.Cond 14) A.Word False destination source (A.OffsetImmediate offset) A.OffsetOnly)
          | destination == A.Reg 0 && source == A.pc -> wordAt (site - 4 + 8 + fromIntegral offset)
        _ -> Nothing
  unless' (literal == Just atcmOwned) "first-stage TCM region write does not follow ldr r0,=0x70000011"
  pure (TcmRegionWrite Core0 InstructionBank (TcmRegister atcmReset) (TcmRegister atcmOwned) (Pc site)
    (Citation "replacement/first_stage/entry.S:34-36; reset value catalog/soc/exr2/soc.json:24-27"))
  where
    unless' condition message = if condition then Right () else Left message
