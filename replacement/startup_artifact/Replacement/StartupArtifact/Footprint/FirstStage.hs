{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the first-stage carriers that its qualification assumes.
-- Its admission (parseFirstStageArtifact, replaceRecordSlot,
-- verifyStagedRecord) reads only symbols and its own carriers. Own carriers
-- are subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.FirstStage (footprint, premises) where

import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

footprint :: [Footprint]
footprint = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation)
  [ (RuntimePremise, Record1, 0x0, 0x20, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:27-28 (retained vector table)")
  , (RuntimePremise, Record1, 0x24, 0x40, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:29-30 (retained vector table)")
  , (RuntimePremise, Record1, 0x1800, 0x4300, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:31-32,35-41; replacement/first_stage/src.rs:204-232 (retained body callees)")
  , (RuntimePremise, Record1, 0x4300, 0x5280, "host/firmware/StartupStageContract.hs:166-197; replacement/first_stage/first-stage.ld:33-34; replacement/first_stage/entry.S:86-96 (retained data, handoff target and argument)")
  , (RuntimePremise, Record2, 0x10000, 0x103e8, "replacement/first_stage/src.rs:164-172 (peer vector copy from record 2)")
  , (RuntimePremise, Record2, 0x10000, 0x20000, "replacement/first_stage/src.rs:238-242; replacement/secondary_stage/adapter.rs:13-14 (record-2 code clears the peer cell 0x40020004 the first stage waits on)")
  , (RuntimePremise, LoaderA, 0x4000c, 0x40010, "emulation/Emulation/LoaderPrefixWitness.hs:681-683 (loader-A header entry, handoff target)")
  , (RuntimePremise, LoaderA, 0x40020, 0x40100, "emulation/Emulation/LoaderPrefixWitness.hs:142-143,757-760 (kernel vector installer)")
  , (RuntimePremise, LoaderA, 0x427b8, 0x427d4, "emulation/Emulation/LoaderPrefixWitness.hs:142-143,757-760 (kernel vector installer)")
  ]

premises :: [Premise]
premises = map Premise
  [ "the retained stock record-1 body [0x1800,0x4300) stages the loaders and images as stock does once the owned entry has run"
  , "the second core starts from record 2 and its vector table at 0x10000 after the peer release"
  ]
