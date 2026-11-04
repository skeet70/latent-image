{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the image5 and plain producer carriers that their
-- qualification assumes: retained services and published callbacks closed over
-- direct calls, the flashloader call site that enters each producer, and for
-- the image5 producer the decoded record 5 it publishes. Own carrier is
-- subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.Producers
  ( Producer (..), producerCarrier, callerSite, windows, closureExtents, closureCalls, footprint
  ) where

import qualified A32 as A
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.Image5Load.Admission as Image5
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.PlainLoad.Admission as Plain
import Replacement.StartupArtifact.Footprint.LoaderStartup (dispatcherCarrier)
import Replacement.StartupArtifact.Footprint.TaskSubmission (closure)
import Replacement.StartupArtifact.Types

data Producer = Image5Load | PlainLoad deriving (Eq, Show, Enum, Bounded)

image4Base, image4End :: Word32
image4Base = 0x61c400
image4End = 0x89ec00

producerCarrier :: Producer -> (Word32, Word32)
producerCarrier producer = case producer of
  Image5Load -> (0x6a96c0, 0x6a9844)
  PlainLoad -> (0x6a9844, 0x6a9938)

-- | The flashloader BL that enters the producer (private-data/analysis/x-m1-1.50/lz/id4.dis:293936,293978).
callerSite :: Producer -> Word32
callerSite producer = case producer of
  Image5Load -> 0x73b4ac
  PlainLoad -> 0x73b554

-- | Both producer carriers, and the idle, root and dispatcher carriers, whose entries are not direct-BL targets.
windows :: [L.CodeWindow]
windows = [window (producerCarrier producer) | producer <- [minBound .. maxBound]]
  ++ [window (0x69cbf0, 0x69cc00), window (0x69d160, 0x69d2b4), dispatcherCarrier]
  where window (low, high) = L.CodeWindow (LI.CodeAddress low) (LI.CodeAddress high)

-- | Retained services and published code roots of the producer.
roots :: Producer -> [Word32]
roots producer = case producer of
  Image5Load -> map Image5.serviceAddress [minBound .. maxBound] ++ [Image5.fillCallback, Image5.feedCallback, image5Completion]
  PlainLoad -> map Plain.serviceAddress [minBound .. maxBound]

-- | The decoder's IRQ98 completion (reports/replacement-inventory-baseline.md:1473).
image5Completion :: Word32
image5Completion = 0x6ae310

inImage4 :: Word32 -> Bool
inImage4 address = address >= image4Base && address < image4End

-- | Canonical decoded image 4.
closureExtents :: Producer -> B.ByteString -> [(Word32, Word32)]
closureExtents producer image4 =
  [ (low, high) | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
      L.closedCalleeExtents (LI.CodeAddress image4Base) image4 windows S.empty (map LI.CodeAddress (filter inImage4 (roots producer))) ]

-- | Every direct BL inside the closure extents, as (site, target), targets in any image.
closureCalls :: Producer -> B.ByteString -> [(Word32, Word32)]
closureCalls producer image4 =
  [ (site, target) | (low, high) <- closureExtents producer image4, site <- [low, low + 4 .. high - 4]
  , A.Branch _ True target <- [A.decode site (U.word32At image4 (fromIntegral (site - image4Base)))] ]

entry :: Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry target start end citation = (\range -> Footprint range RuntimePremise (Citation citation)) <$> span' target start end

-- | Canonical loader A, loader B and decoded image 4.
footprint :: Producer -> B.ByteString -> B.ByteString -> B.ByteString -> [Footprint]
footprint producer loaderA loaderB image4 =
  mapMaybe (\(start, end) -> entry Image4 start end services) (closureExtents producer image4)
  ++ mapMaybe (\(_, target, start, end, citation) -> entry target start end citation)
       ( closure LoaderA 0x40000 loaderA [] (loaderRoots 0x40000 loaderA) loaderCitation
      ++ closure LoaderB 0xd0000 loaderB [] (loaderRoots 0xd0000 loaderB) loaderCitation )
  ++ mapMaybe id (entry Image4 (callerSite producer) (callerSite producer + 8) caller : published)
  where
    loaderRoots base loader = nub ([address | address <- roots producer, address >= base, address < base + fromIntegral (B.length loader)]
      ++ [target | (_, target) <- closureCalls producer image4, target >= base, target < base + fromIntegral (B.length loader)])
    loaderCitation = "direct calls from the producer's services and image-4 closure into the loader, closed over direct calls"
    (services, caller, published) = case producer of
      Image5Load ->
        ( "replacement/image5_load/Replacement/Image5Load/Admission.hs:81-87,92-102 (retained services, fill callback 0x6a8bec, feed callback 0x6ae500) and Artifact.hs:140 (decoder, IRQ98 completion and resource 0x48 retained; completion 0x6ae310, reports/replacement-inventory-baseline.md:1473), closed over direct calls with the producer, idle, root and dispatcher carriers excluded"
        , "private-data/analysis/x-m1-1.50/lz/id4.dis:293936-293937 (flashloader bl 0x6a96c0 and its continuation); replacement/image5_load/Replacement/Image5Load/Admission.hs:370-378 (return restores SP, r4-r11 and LR 0x422b8)"
        , [entry Image5 0xbf1a00 0x10e3860 "reports/replacement-inventory-baseline.md:1321 (decoded record 5 at [0xbf1a00,0x10e3860)); replacement/image5_load/Replacement/Image5Load/Admission.hs:175-179 (decoder init with the destination field and staging 0x43e40000)"] )
      PlainLoad ->
        ( "replacement/plain_load/Replacement/PlainLoad/Admission.hs:238-247 (retained services), closed over direct calls with the producer, idle, root and dispatcher carriers excluded"
        , "private-data/analysis/x-m1-1.50/lz/id4.dis:293978-293979 (flashloader bl 0x6a9844 and its continuation); replacement/plain_load/Replacement/PlainLoad/Admission.hs:276-280 (return restores SP, r4-r11 and LR 0x6a992c)"
        , [] )
