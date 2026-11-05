-- | The stock extents of the 17 own-first slots per loader, from private-data/scratch/OwnFirstSlots/design.md,
-- and the +0x1618 body window: the last 32 bytes of the +0x12c extent.
module Replacement.RtosOwnFirst.Pins
  ( DomainPins (..)
  , RegionPins
  , stockPins
  ) where

import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions

data DomainPins = DomainPins
  { pinRows :: [(SlotOffset, Extent)]
  , pinTail :: (SlotOffset, Extent)
  , -- | Host slot and window.
    pinBody :: (SlotOffset, Extent)
  }
  deriving (Eq, Show)

type RegionPins = Domain -> DomainPins

extent :: Word32 -> Word32 -> Extent
extent start end = Extent (GuestAddress start) (GuestAddress end)

pins :: [(Word32, Word32, Word32)] -> (Word32, Word32) -> (Word32, Word32) -> DomainPins
pins rows (tailStart, tailEnd) (bodyStart, bodyEnd) =
  DomainPins
    [(SlotOffset slot, extent start end) | (slot, start, end) <- rows]
    (SlotOffset 0x1618, extent tailStart tailEnd)
    (SlotOffset 0x12c, extent bodyStart bodyEnd)

stockPins :: RegionPins
stockPins domain = case domain of
  DomainA ->
    pins
          [ (0x60, 0x47540, 0x47580)
          , (0xfc, 0x4bd4c, 0x4bdc4)
          , (0x100, 0x4bdc4, 0x4be3c)
          , (0x128, 0x4c620, 0x4c6a8)
          , (0x12c, 0x4c6a8, 0x4c73c)
          , (0x130, 0x4c8ac, 0x4c8f8)
          , (0x134, 0x4c8f8, 0x4c944)
          , (0x138, 0x4c944, 0x4c990)
          , (0x13c, 0x4c990, 0x4c9dc)
          , (0x148, 0x4cbd8, 0x4cc00)
          , (0x14c, 0x4cc00, 0x4cc1c)
          , (0x150, 0x4cc1c, 0x4cc44)
          , (0x154, 0x4cc44, 0x4cc94)
          , (0x164, 0x41140, 0x41164)
          , (0x168, 0x41164, 0x4118c)
          , (0x16c, 0x4118c, 0x411cc)
          ]
          (0x412bc, 0x412cc)
          (0x4c71c, 0x4c73c)
  DomainB ->
      pins
          [ (0x60, 0xd7500, 0xd7540)
          , (0xfc, 0xdbd0c, 0xdbd84)
          , (0x100, 0xdbd84, 0xdbdfc)
          , (0x128, 0xdc5e0, 0xdc668)
          , (0x12c, 0xdc668, 0xdc6fc)
          , (0x130, 0xdc86c, 0xdc8b8)
          , (0x134, 0xdc8b8, 0xdc904)
          , (0x138, 0xdc904, 0xdc950)
          , (0x13c, 0xdc950, 0xdc99c)
          , (0x148, 0xdcb98, 0xdcbc0)
          , (0x14c, 0xdcbc0, 0xdcbdc)
          , (0x150, 0xdcbdc, 0xdcc04)
          , (0x154, 0xdcc04, 0xdcc54)
          , (0x164, 0xd1140, 0xd1164)
          , (0x168, 0xd1164, 0xd118c)
          , (0x16c, 0xd118c, 0xd11cc)
          ]
          (0xd12bc, 0xd12cc)
          (0xdc6dc, 0xdc6fc)
