{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the loader-startup carriers that the joined loader,
-- dispatcher and installer admission reads, or that their qualification
-- assumes, beyond the two orchestration sides' own footprints. Own carriers
-- are subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.LoaderStartup (footprint, dispatcherCarrier, installerCarrier) where

import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import Data.Word (Word32)
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.StartupArtifact.Footprint.LoaderOrchestration as Orchestration
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

dispatcherCarrier, installerCarrier :: L.CodeWindow
dispatcherCarrier = L.CodeWindow (LI.CodeAddress 0x69cdf8) (LI.CodeAddress 0x69cf84)
installerCarrier = L.CodeWindow (LI.CodeAddress 0x725a04) (LI.CodeAddress 0x725b54)

-- | Canonical loader A and decoded image 4, and both admitted orchestration sides.
footprint :: B.ByteString -> B.ByteString -> L.CanonicalSource -> L.LinkedArtifact -> L.LinkedArtifact -> [Footprint]
footprint loaderA image4 orchestration sideA sideB =
  Orchestration.footprint orchestration sideA ++ Orchestration.footprint orchestration sideB
  ++ mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation)
  ( [ (AdmissionRead, LoaderA, 0x40000, 0x4dae0, "replacement/startup_dispatch/Replacement/StartupDispatch/Artifact.hs:40-46 (whole loader-A sha256)")
    , (AdmissionRead, LoaderB, 0xd0000, 0xddaa0, "replacement/startup_dispatch/Replacement/StartupDispatch/Artifact.hs:40-46 (whole loader-B sha256)")
    , (AdmissionRead, LoaderA, 0x40010, 0x40014, "replacement/startup_composition/Replacement/StartupComposition/Artifact.hs:61 (loader-A header root)")
    , (AdmissionRead, LoaderB, 0xd0010, 0xd0014, "replacement/startup_composition/Replacement/StartupComposition/Artifact.hs:62 (loader-B header root)")
    , (AdmissionRead, Image4, 0x7caf3c, 0x7caf40, "replacement/startup_composition/Replacement/StartupComposition/Artifact.hs:63 (installer marker)")
    ]
    ++ [ (RuntimePremise, Image4, address, address + 4, citation)
       | (address, citation) <-
           [ (0x69d160, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206 (task 104 entry)")
           , (0x69cbf0, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:208,210 (tasks 102 and 103 entry)")
           , (0x69cd58, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:211 (task 93 entry)")
           , (0x69cc6c, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:220 (Diagnostic result the loader publishes as a callback)") ] ]
    ++ [ (RuntimePremise, Image4, low, high, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:226-236 (retained mode getter, clear and task submission, closed over direct calls)")
       | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
           L.closedCalleeExtents (LI.CodeAddress 0x61c400) image4 [dispatcherCarrier, installerCarrier] S.empty (map LI.CodeAddress [0x657184, 0x7221a0, 0x725dd8]) ]
    ++ [ (RuntimePremise, LoaderA, low, high, "replacement/kernel_install/Replacement/KernelInstall/Admission.hs:184-189,259 (retained cache service 0x42634, closed over direct calls)")
       | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
           L.closedCalleeExtents (LI.CodeAddress 0x40000) loaderA [L.CodeWindow (LI.CodeAddress 0x409ac) (LI.CodeAddress 0x40b00)]
             (S.singleton (LI.CodeAddress 0x40430)) [LI.CodeAddress 0x42634] ] )
