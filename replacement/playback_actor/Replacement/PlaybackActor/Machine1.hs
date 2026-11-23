-- | A path model of the stock machine-1 action bytes in [0x011ea434,0x011ea5f0), table indices
-- 2..15 of 0x01218d90.
module Replacement.PlaybackActor.Machine1 (machine1) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine1 :: Machine
machine1 =
  Machine
    { machineName = "machine1"
    , machineIndex = 1
    , machineTransitions = 0x01219ba4
    , machineStates = 4
    , machineEvents = 9
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "image_control" imageControl 0x8 True
        , Region "media_output" mediaOutput 0x4 True
        ]
    , machineActions = actions
    }

imageControl, mediaOutput :: Word32
imageControl = 0x015114f4
mediaOutput = 0x015149a0

state :: Word32 -> Action ()
state = store 0x014ea5ae Half

s120b40c, s120cab0, s1210250, s12142f8, trace, s124c6fc :: Word32
s120b40c = 0x0120b40c
s120cab0 = 0x0120cab0
s1210250 = 0x01210250
s12142f8 = 0x012142f8
trace = 0x0121eb68
s124c6fc = 0x0124c6fc

-- | A service whose r0 the action returns.
returning :: Word32 -> Action Word32
returning address = call address [] >>= finish

void :: Word32 -> Action ()
void address = () <$ call address []

actions :: [Action Word32]
actions =
  [ do
      r <- call s120cab0 [3]
      when (r /= 3) (state 1)
      finish r
  , do
      r <- call s120cab0 [3]
      when (r == 3) (state 0)
      finish r
  , void trace >> finish 0
  , callWith s124c6fc 1 [] >> returning s1210250
  , state 2 >> returning s1210250
  , returning s1210250
  , do
      b <- load (imageControl + 4) Byte [1]
      state (if b == 1 then 0 else 2)
      returning s1210250
  , returning s12142f8
  , state 3 >> returning s12142f8
  , do
      w <- load mediaOutput Word [1]
      state (if w == 1 then 0 else 3)
      returning s12142f8
  , returning s120b40c
  , void s1210250 >> returning s120b40c
  , void s12142f8 >> returning s120b40c
  , finish 0
  ]
