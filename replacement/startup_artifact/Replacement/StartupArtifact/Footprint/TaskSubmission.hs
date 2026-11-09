{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the task-submission carrier that the wrapper/release
-- admission reads, or that its qualification assumes. Own carrier is
-- subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.TaskSubmission (footprint, closure) where

import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import Data.Word (Word32)
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

carrier :: L.CodeWindow
carrier = L.CodeWindow (LI.CodeAddress 0x725dd8) (LI.CodeAddress 0x72612c)

-- | Direct-BL closure of retained roots in one image, own carrier excluded so
-- that a closure entering another piece's carrier is a conflict.
closure :: Target -> Word32 -> B.ByteString -> [L.CodeWindow] -> [Word32] -> Text -> [(FootprintBasis, Target, Word32, Word32, Text)]
closure target base image excluded roots citation =
  [ (RuntimePremise, target, low, high, citation)
  | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
      L.closedCalleeExtents (LI.CodeAddress base) image excluded S.empty (map LI.CodeAddress roots) ]

-- | Canonical loader A, loader B and decoded image 4.
footprint :: B.ByteString -> B.ByteString -> B.ByteString -> [Footprint]
footprint loaderA loaderB image4 = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation) $
  [ (RuntimePremise, Image4, 0x7235d8, 0x7236b0, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:95 (critical enter call target)")
  , (RuntimePremise, Image4, 0x7236b0, 0x723790, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:95 (critical exit call target)")
  , (RuntimePremise, Image4, 0x725c6c, 0x725dd8, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:95 (reservation: retained prelude, storage transition, retained epilogue and literals)")
  , (RuntimePremise, Image4, 0x69cdf8, 0x69cf84, "replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206-211,233-237 (dispatcher callers pass descriptor D = S-44 in r0)")
  , (RuntimePremise, Image4, 0x725a04, 0x725b54, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:123 (class roots 0x96bd20/+4, which the installer stores)")
  , (RuntimePremise, Image4, 0x7cad9c, 0x7caf3c, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:123 (class table, immutable)")
  , (RuntimePremise, LoaderA, 0x409ac, 0x40b00, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:219-240,261 (table A filled before the first dispatch)")
  , (RuntimePremise, LoaderB, 0xd09ac, 0xd0b00, "replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:219-240,261 (table B filled before the peer-task dispatch)")
  , (RuntimePremise, LoaderA, 0x40430, 0x409ac, "emulation/Emulation/PrimaryInitializerArtifact.hs:297-302,470 (table A service slots)")
  , (RuntimePremise, LoaderB, 0xd0430, 0xd0434, "emulation/Emulation/InterfaceInitializer.hs:515-525 (table B service slots behind the initializer entry)")
  , (RuntimePremise, Image4, 0x89ea8c, 0x89ea90, "reports/replacement-inventory-baseline.md:887 (error callback word, canonical 0x723330)")
  , (RuntimePremise, Image4, 0x69cbf0, 0x69cc00, "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:110-111,127-129 (error dispatch demand assumes the idle body's frame)")
  , (RuntimePremise, Image4, 0x723330, 0x723344, "reports/rtos-platform-options.md#421-owned-cyclic-handler-family-lifecycle-expiry-and-reentrancy (default error callback returns at 0x723340 without calls or literals)")
  ]
  ++ closure Image4 0x61c400 image4 [carrier] [0x7221c8, 0x723398, 0x72835c, 0x6470fc]
       "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:96 (fill, error dispatch); callback 0x72835c (reports/replacement-inventory-baseline.md:889) and log trace 0x6470fc (emulation/Emulation/TaskSubmissionProgram.hs:487-496), closed over direct calls"
  ++ closure LoaderA 0x40000 loaderA [] [0x41a34, 0x42634, 0x427d4, 0x427fc, 0x4d39c, 0x4d094, 0x44f2c, 0x412bc, 0x41140, 0x41164]
       "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:96-97,99-107 (log, cache, lock services and root-0 services; reports/replacement-inventory-baseline.md:1078-1082), closed over direct calls"
  ++ closure LoaderB 0xd0000 loaderB [] [0xdd360, 0xdd054, 0xd4eec, 0xd12bc, 0xd1140, 0xd1164]
       "replacement/task_submission/Replacement/TaskSubmission/Admission.hs:99-107 (root-1 services; reports/replacement-inventory-baseline.md:1078-1082), closed over direct calls"
