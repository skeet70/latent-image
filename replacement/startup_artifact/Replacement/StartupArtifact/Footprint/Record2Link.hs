{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the record-2 link carriers that Emulation.CriticalPair,
-- Emulation.InterfaceInitializer and Emulation.LoaderPrefixWitness read during
-- admission, or that the link's qualification assumes. Own carriers are
-- subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.Record2Link (footprint) where

import Data.Bits ((.&.))
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import qualified Data.ByteString as B
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

-- | Services each loader publishes, read at the same offsets from both bases.
services :: Target -> Word32 -> [(FootprintBasis, Target, Word32, Word32, Text)]
services target base =
  [ (AdmissionRead, target, base + low, base + high, citation) | (low, high, citation) <-
    [ (0x10, 0x14, "emulation/Emulation/CriticalPair.hs:87 (published root header)")
    , (0x438, 0x440, "emulation/Emulation/CriticalPair.hs:86 (root MOVW/MOVT)")
    , (0x86c, 0x884, "emulation/Emulation/CriticalPair.hs:89-92 (IRQ save/restore slots)")
    , (0x8a8, 0x8e8, "emulation/Emulation/CriticalPair.hs:93-97,101-102 (extension slots)")
    , (0x1140, 0x1164, "emulation/Emulation/CriticalPair.hs:103-104 (IRQ mask service)")
    , (0x1164, 0x118c, "emulation/Emulation/CriticalPair.hs:105-106 (IRQ restore service)")
    , (0x12bc, 0x12cc, "emulation/Emulation/CriticalPair.hs:107 (identity wrapper)")
    , (0x12cc, 0x1320, "emulation/Emulation/CriticalPair.hs:108-109 (exclusive wrappers)")
    , (0x27b8, 0x27d4, "emulation/Emulation/CriticalPair.hs:110 (CP14 identity helper)")
    , (0x27d4, 0x27fc, "emulation/Emulation/CriticalPair.hs:111-112 (retry helper)")
    , (0x27fc, 0x2830, "emulation/Emulation/CriticalPair.hs:113-114 (release and observe helpers)")
    ] ]

-- | Address of the word an @LDR rt, [pc, #imm]@ at @site@ loads, as the
-- caller-frame premise decodes it.
pcLiteral :: B.ByteString -> Word32 -> Word32 -> Maybe Word32
pcLiteral loader base site
  | offset + 4 <= B.length loader && word .&. 0x0f7f0000 == 0x051f0000 =
      Just (if word .&. 0x00800000 /= 0 then site + 8 + immediate else site + 8 - immediate)
  | otherwise = Nothing
  where
    offset = fromIntegral (site - base)
    word = U.word32At loader offset
    immediate = word .&. 0xfff

-- | Canonical loader B and the record-2 retained extents as guest ranges.
footprint :: B.ByteString -> [(Word32, Word32)] -> [Footprint]
footprint loaderB retained = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation) $
  [ (AdmissionRead, LoaderA, 0x40000, 0x4dae0, "emulation/Emulation/CriticalPair.hs:59-75,126 (whole loader-A sha256)")
  , (AdmissionRead, LoaderB, 0xd0000, 0xddaa0, "emulation/Emulation/CriticalPair.hs:59-75,127 (whole loader-B sha256)")
  ]
  ++ services LoaderA 0x40000 ++ services LoaderB 0xd0000 ++
  [ (AdmissionRead, Image4, 0x7235d8, 0x72360c, "emulation/Emulation/CriticalPair.hs:129-131 (critical entry prologue)")
  , (AdmissionRead, Image4, 0x725a0c, 0x725a34, "emulation/Emulation/CriticalPair.hs:138-139 (root pair installation)")
  , (AdmissionRead, Image4, 0x725dd8, 0x725de4, "emulation/Emulation/CriticalPair.hs:132 (task103 wrapper prologue)")
  , (AdmissionRead, Image4, 0x725dfc, 0x725e04, "emulation/Emulation/CriticalPair.hs:133 (entry call)")
  , (AdmissionRead, Image4, 0x725e80, 0x725e88, "emulation/Emulation/CriticalPair.hs:134 (exit call)")
  , (AdmissionRead, Image4, 0x725fb0, 0x725fb8, "emulation/Emulation/CriticalPair.hs:135 (exit call)")
  , (AdmissionRead, Image4, 0x726060, 0x726068, "emulation/Emulation/CriticalPair.hs:136 (exit call)")
  , (AdmissionRead, Image4, 0x726090, 0x726098, "emulation/Emulation/CriticalPair.hs:137 (wrapper return)")
  , (RuntimePremise, Image4, 0x725dd8, 0x726098, "emulation/Emulation/CriticalPairWitness.hs:253 (task103 wrapper calling the pair)")
  , (AdmissionRead, LoaderB, 0xd0430, 0xd09ac, "emulation/Emulation/InterfaceInitializer.hs:469-470 (retained initializer body sha256)")
  , (AdmissionRead, LoaderB, 0xd00a8, 0xd00b0, "emulation/Emulation/InterfaceInitializer.hs:105,112 (caller stack literal load and install)")
  , (AdmissionRead, LoaderB, 0xd09ac, 0xd09b4, "emulation/Emulation/InterfaceInitializer.hs:113-114 (caller frame)")
  , (AdmissionRead, LoaderB, 0xd0440, 0xd08a8, "emulation/Emulation/InterfaceInitializer.hs:515-522 (original service stores)")
  , (AdmissionRead, LoaderB, 0xd08a8, 0xd08f4, "emulation/Emulation/InterfaceInitializer.hs:523 (original extension stores)")
  , (AdmissionRead, LoaderB, 0xd08fc, 0xd090c, "emulation/Emulation/InterfaceInitializer.hs:524-525 (pool and extended bases)")
  , (AdmissionRead, LoaderB, 0xd000c, 0xd0010, "emulation/Emulation/LoaderPrefixWitness.hs:496 (loader-B header entry)")
  , (AdmissionRead, LoaderB, 0xd09c0, 0xd09cc, "emulation/Emulation/LoaderPrefixWitness.hs:451-454 (stock wait register setup)")
  , (AdmissionRead, LoaderB, 0xd0a44, 0xd0a64, "emulation/Emulation/LoaderPrefixWitness.hs:455-459 (stock wait poll period)")
  , (RuntimePremise, LoaderB, 0xd09ac, 0xd0b00, "replacement/loader_prefix/entry.S:36-45 (the prefix exit enters 0xd09ac; the core-1 path calls loader+0x430 after a nonzero completion read)")
  , (AdmissionRead, Record1, 0x20, 0x24, "emulation/Emulation/LoaderPrefixWitness.hs:563 (record-1 reset literal 0x1800)")
  , (AdmissionRead, Record2, 0x10000, 0x14c80, "emulation/Emulation/LoaderPrefixWitness.hs:433-438,522-523 (record-2 digests bound into the prefix contract facts)")
  , (RuntimePremise, Record1, 0x0, 0x10000, "replacement/first_stage/src.rs:164-178 (record-1 starter: peer vector copy and peer release)")
  ]
  ++ [ (AdmissionRead, LoaderB, literal, literal + 4, "emulation/Emulation/InterfaceInitializer.hs:106-111 (caller stack literal)")
     | Just literal <- [pcLiteral loaderB 0xd0000 0xd00a8] ]
  ++ [ (RuntimePremise, Record2, start, end, "host/firmware/SecondaryStageContract.hs:15-27 (retained record-2 callees)")
     | (start, end) <- retained ]
