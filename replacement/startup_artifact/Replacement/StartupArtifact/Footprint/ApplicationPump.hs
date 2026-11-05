{-# LANGUAGE OverloadedStrings #-}
-- | Image5 bytes outside the application-pump carrier that its admission
-- assumes: task1's call into the pump and the entries of the three retained
-- hooks it calls.
module Replacement.StartupArtifact.Footprint.ApplicationPump
  ( pumpCarrier, callerSite, hookEntries, footprint
  ) where

import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import Replacement.StartupArtifact.Types

-- | [start, end) in image5.
pumpCarrier :: (Word32, Word32)
pumpCarrier = (0xbfbd20, 0xbfbd40)

-- | Task1's @bl 0xbfbd20@ (private-data/analysis/x-m1-1.50/lz/id5.dis:50679).
callerSite :: Word32
callerSite = 0xc23274

-- | Prehook, normalizer and posthook entries (id5.dis:9672,9693,9682).
hookEntries :: [Word32]
hookEntries = [0xbfb1b8, 0xbfb20c, 0xbfb1e0]

footprint :: [Footprint]
footprint = mapMaybe word
  ((callerSite, "task1's only pump call bl 0xbfbd20 (private-data/analysis/x-m1-1.50/lz/id5.dis:50679)")
    : [ (entry, "retained hook entry the owned pump calls (private-data/analysis/x-m1-1.50/lz/id5.dis:9672,9682,9693); its body is not part of the pump's contract")
      | entry <- hookEntries ])
  where
    word :: (Word32, Text) -> Maybe Footprint
    word (address, citation) = (\range -> Footprint range RuntimePremise (Citation citation)) <$> span' Image5 address (address + 4)
