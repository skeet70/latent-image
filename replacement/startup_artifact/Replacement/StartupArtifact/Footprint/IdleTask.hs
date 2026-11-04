{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the idle carrier that the idle piece's adapter reads: the
-- loader-A and loader-B first-entry trampolines that start idle102 and idle103.
module Replacement.StartupArtifact.Footprint.IdleTask
  ( footprint, trampolineA, trampolinesMatch
  ) where

import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Citation -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis citation) <$> span' target start end

-- | Instructions [0x43728,0x4375c) and the literal words of the r1 and lr loads.
trampolineSpans :: Word32 -> [(Word32, Word32)]
trampolineSpans base = [(base + 0x3728, base + 0x375c), (base + 0x37fc, base + 0x3800), (base + 0x3808, base + 0x380c)]

-- | The loader-A trampoline as a runtime premise of a task it starts.
trampolineA :: FootprintBasis -> Citation -> [Footprint]
trampolineA basis citation = mapMaybe (\(start, end) -> entry basis LoaderA start end citation) (trampolineSpans 0x40000)

wordAt :: Word32 -> B.ByteString -> Word32 -> Maybe Word32
wordAt base image address
  | address >= base && offset + 4 <= B.length image = Just (U.word32At image offset)
  | otherwise = Nothing
  where
    offset = fromIntegral (address - base)

-- | Canonical loader A and loader B: equal trampoline instruction words, and the current-task cell and
-- task-return service literals A 0x40054760 and 0x453f0 against B 0x400e4720 and 0xd53b0
-- (private-data/analysis/x-m1-1.50/decode/loader-40000.dis:3504-3516,3557-3560; loader-d0000.dis:3504-3516,3557-3560).
trampolinesMatch :: B.ByteString -> B.ByteString -> Bool
trampolinesMatch loaderA loaderB =
  all (\offset -> a (0x43728 + offset) /= Nothing && a (0x43728 + offset) == b (0xd3728 + offset)) [0, 4 .. 0x30]
    && [a 0x437fc, a 0x43808] == [Just 0x40054760, Just 0x453f0]
    && [b 0xd37fc, b 0xd3808] == [Just 0x400e4720, Just 0xd53b0]
  where
    a = wordAt 0x40000 loaderA
    b = wordAt 0xd0000 loaderB

footprint :: [Footprint]
footprint = trampolineA AdmissionRead citation
  ++ mapMaybe (\(start, end) -> entry AdmissionRead LoaderB start end citation) (trampolineSpans 0xd0000)
  where
    citation = Citation "private-data/analysis/x-m1-1.50/decode/loader-40000.dis:3504-3516,3557-3560 and loader-d0000.dis:3504-3516,3557-3560 (first-entry trampolines: SVC with IRQ enabled, LR the task-return service, r0 the creation argument, PC the creation entry; reports/replacement-inventory-baseline.md:1128-1130)"
