{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the root carrier that root104's qualification assumes: its
-- retained services and published callbacks closed over direct calls, and the
-- trampoline that starts it. Own carrier is subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.RootTask
  ( footprint, closureExtents, closureCalls
  ) where

import qualified A32 as A
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.RootTask.Services (serviceSymbols)
import qualified Replacement.StartupArtifact.Footprint.IdleTask as Idle
import Replacement.StartupArtifact.Footprint.LoaderStartup (dispatcherCarrier)
import Replacement.StartupArtifact.Footprint.TaskSubmission (closure)
import Replacement.StartupArtifact.Types

image4Base :: Word32
image4Base = 0x61c400

-- | Root's own carrier, the idle, app-callback and dispatcher carriers that would otherwise end the
-- extents of 0x69ca44 and 0x69cc18 only at the next direct-call target 0x69cfa4.
excluded :: [L.CodeWindow]
excluded = [window 0x69d160 0x69d2b4, window 0x69cbf0 0x69cc00, window 0x725424 0x7257a8, dispatcherCarrier]
  where
    window low high = L.CodeWindow (LI.CodeAddress low) (LI.CodeAddress high)

-- | Every code entry root calls (replacement/root_task/Replacement/RootTask/Services.hs, without the
-- context word 0x96ca4c and the app-callback entry 0x725424) and the callbacks it publishes.
roots :: [Word32]
roots = [address | (_, address) <- serviceSymbols, address /= 0x96ca4c, address /= 0x725424] ++ [0x72835c, 0x726478]

-- | Canonical decoded image 4.
closureExtents :: B.ByteString -> [(Word32, Word32)]
closureExtents image4 =
  [ (low, high) | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
      L.closedCalleeExtents (LI.CodeAddress image4Base) image4 excluded S.empty (map LI.CodeAddress roots) ]

-- | Every direct BL inside the closure extents, as (site, target), targets in any image.
closureCalls :: B.ByteString -> [(Word32, Word32)]
closureCalls image4 =
  [ (site, target) | (low, high) <- closureExtents image4, site <- [low, low + 4 .. high - 4]
  , A.Branch _ True target <- [A.decode site (U.word32At image4 (fromIntegral (site - image4Base)))] ]

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Citation -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis citation) <$> span' target start end

-- | Canonical loader A, loader B and decoded image 4.
footprint :: B.ByteString -> B.ByteString -> B.ByteString -> [Footprint]
footprint loaderA loaderB image4 =
  mapMaybe (\(start, end) -> entry RuntimePremise Image4 start end services) (closureExtents image4)
  ++ mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end (Citation citation))
       ( closure LoaderA 0x40000 loaderA [] (into 0x40000 loaderA) loaderCitation
      ++ closure LoaderB 0xd0000 loaderB [] (into 0xd0000 loaderB) loaderCitation )
  ++ Idle.trampolineA RuntimePremise (Citation "private-data/analysis/x-m1-1.50/decode/loader-40000.dis:3504-3516,3557-3560 (first-entry trampoline: r0 the creation argument 0, LR the task-return service; reports/replacement-inventory-baseline.md:1128-1130)")
  ++ mapMaybe id [entry RuntimePremise LoaderA 0x453f0 0x453f4 (Citation "replacement/root_task/Replacement/RootTask/Admission.hs:239-241 (a returning root hands back through the trampoline-supplied LR 0x453f0; reports/replacement-inventory-baseline.md:1130,1684)")]
  where
    services = Citation "replacement/root_task/Replacement/RootTask/Services.hs:7-35 and Artifact.hs:132-145 (retained AAPCS services, the published callbacks 0x72835c, 0x726478 and periodic callback 0x69cc18; reports/replacement-inventory-baseline.md:1664-1690), closed over direct calls with the root, idle and dispatcher carriers excluded"
    loaderCitation = "direct calls from root's image-4 closure into the loader (for example 0x41a34 log), closed over direct calls"
    into base loader = nub [target | (_, target) <- closureCalls image4, target >= base, target < base + fromIntegral (B.length loader)]
