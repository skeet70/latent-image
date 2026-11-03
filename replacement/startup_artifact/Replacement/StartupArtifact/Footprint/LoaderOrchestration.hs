{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside a loader-orchestration carrier that
-- Replacement.LoaderOrchestration.Artifact reads during admission, or that the
-- orchestration's qualification assumes. Own carriers are subtracted when the
-- piece is derived.
module Replacement.StartupArtifact.Footprint.LoaderOrchestration (footprint) where

import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

footprint :: L.CanonicalSource -> L.LinkedArtifact -> [Footprint]
footprint source artifact = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation) $
  [ (AdmissionRead, Image4, 0x61c40c, 0x61c410, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Artifact.hs:47 (image-4 header names the dispatcher)")
  , (AdmissionRead, Image4, 0x69cdf8, 0x69cf84, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Artifact.hs:51-52 (canonical dispatcher frame proof)")
  , (RuntimePremise, loader, base + 0xc, base + 0x10, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:141-145,314-317 (the loader header entry reaches the carrier with the stock frame registers)")
  ]
  ++ [ (RuntimePremise, loader, low, high, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Artifact.hs calleeExtents (retained callees)")
     | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <- L.calleeExtents source artifact ]
  ++ peerPremises
  where
    side = L.linkedSide artifact
    base = LI.loaderBase side
    loader = case side of
      LI.LoaderA -> LoaderA
      LI.LoaderB -> LoaderB
    peerPremises = case side of
      LI.LoaderA -> []
      LI.LoaderB ->
        [ (RuntimePremise, LoaderA, 0x40a00, 0x40a10, "emulation/Emulation/LoaderPrefixWitness.hs:168-170 (loader-A completion store ends the peer wait)")
        , (RuntimePremise, LoaderA, 0x40afc, 0x40b00, "emulation/Emulation/LoaderPrefixWitness.hs:168-170 (loader-A completion literal)")
        , (RuntimePremise, Record1, 0x0, 0x10000, "replacement/first_stage/src.rs:237-239 (the first stage zeroes 0x400a00c4 before the peer rendezvous)")
        ]
