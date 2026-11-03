{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the loader-A publisher carrier that
-- Emulation.PrimaryInitializerSource, Emulation.PrimaryInitializerConsumerSource
-- and Emulation.PrimaryInitializerArtifact read during admission, or that the
-- publisher's qualification assumes. Own carriers are subtracted when the piece
-- is derived.
module Replacement.StartupArtifact.Footprint.PrimaryPublisher (footprint) where

import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

-- | Address of the word the @LDR r4, [pc, #imm]@ at 0x400a8 loads.
stackLiteral :: B.ByteString -> Maybe Word32
stackLiteral loaderA
  | B.length loaderA >= 0xac && word .&. 0x0f7f0000 == 0x051f0000 =
      Just (if word .&. 0x00800000 /= 0 then 0x400b0 + immediate else 0x400b0 - immediate)
  | otherwise = Nothing
  where
    word = U.word32At loaderA 0xa8
    immediate = word .&. 0xfff

-- | Canonical loader A.
footprint :: B.ByteString -> [Footprint]
footprint loaderA = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation) $
  [ (AdmissionRead, LoaderA, 0x40000, 0x4dae0, "emulation/Emulation/PrimaryInitializerSource.hs:110-113 (whole loader-A sha256)")
  , (AdmissionRead, LoaderA, 0x40010, 0x40014, "emulation/Emulation/PrimaryInitializerSource.hs:114,119 (primary root header)")
  , (AdmissionRead, LoaderB, 0xd0010, 0xd0014, "emulation/Emulation/PrimaryInitializerSource.hs:115-119 (secondary root header)")
  , (AdmissionRead, LoaderA, 0x400a8, 0x400b0, "emulation/Emulation/PrimaryInitializerSource.hs:131-138 (caller stack literal load and install)")
  , (AdmissionRead, LoaderA, 0x409ac, 0x409b8, "emulation/Emulation/PrimaryInitializerSource.hs:138-139 (caller frame)")
  , (AdmissionRead, LoaderA, 0x409dc, 0x409e0, "emulation/Emulation/PrimaryInitializerSource.hs:140-142; emulation/Emulation/PrimaryInitializerArtifact.hs:479-483 (direct caller BL)")
  , (AdmissionRead, LoaderA, 0x40aac, 0x40ab0, "emulation/Emulation/PrimaryInitializerSource.hs:140-142; emulation/Emulation/PrimaryInitializerArtifact.hs:479-483 (direct caller BL)")
  , (AdmissionRead, LoaderA, 0x40250, 0x40254, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:95 (consumer stack load)")
  , (AdmissionRead, LoaderA, 0x402c8, 0x402d4, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:97 (consumer IRQ mask)")
  , (AdmissionRead, LoaderA, 0x40370, 0x40374, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:96 (consumer ATCM stack top)")
  , (AdmissionRead, LoaderA, 0x412bc, 0x412cc, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:41,93 (root selector service)")
  , (AdmissionRead, LoaderA, 0x427b8, 0x427d4, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:42,94 (CP14 identity helper)")
  , (AdmissionRead, Image4, 0x725a04, 0x725a34, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:34-36,91 (root pair installation)")
  , (AdmissionRead, Image4, 0x725b74, 0x725bb4, "emulation/Emulation/PrimaryInitializerConsumerSource.hs:37-40,92 (descriptor consumer)")
  , (RuntimePremise, LoaderA, 0x409ac, 0x409e4, "emulation/Emulation/PrimaryInitializerSource.hs:138-144 (caller frame and return at 0x409e0)")
  , (RuntimePremise, LoaderA, 0x40aac, 0x40ab4, "emulation/Emulation/PrimaryInitializerSource.hs:140-142 (alternate caller and return at 0x40ab0)")
  ]
  ++ [ (AdmissionRead, LoaderA, literal, literal + 4, "emulation/Emulation/PrimaryInitializerSource.hs:135-137,143 (caller stack top literal)")
     | Just literal <- [stackLiteral loaderA] ]
