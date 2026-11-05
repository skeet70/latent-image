{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupArtifact.Footprint.Image5Init (footprint) where
import Data.Maybe (mapMaybe)
import Replacement.StartupArtifact.Types

footprint :: [Footprint]
footprint = mapMaybe (\(target,start,end) -> (\range -> Footprint range RuntimePremise (Citation "private-data/scratch/j9/spec.md sections 1.2, 2.2, 3.2 and 4")) <$> span' target start end)
  ([(Image5,0xbf1a0c,0xbf1a10),(Image4,0x69d2e0,0x69d2e4),(Image5,0xbf1a20,0xbf1a30)]
   ++ [(if address < 0xbf1a00 then Image4 else Image5,address,address+4) | address <- [0x72872c,0x6c62a8,0x73aca4,0xed4acc,0xed4afc,0x69cf84,0xd986b0,0xd80d44,0x1056c24,0xdb01fc,0xec5350,0xdca604,0xdb63e0,0xed10b4,0xef8c90,0xed2b00,0x1055d10,0x6b7df8,0x69f000,0x107277c,0x1066304,0x1062114]])
