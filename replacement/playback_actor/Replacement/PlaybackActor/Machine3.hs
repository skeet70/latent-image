-- | A path model of the stock machine-3 action bytes in [0x011eaa54,0x011eacb0), table
-- indices 2..10 of 0x01218e94.
module Replacement.PlaybackActor.Machine3 (machine3) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine3 :: Machine
machine3 =
  Machine
    { machineName = "machine3"
    , machineIndex = 3
    , machineTransitions = 0x01216574
    , machineStates = 4
    , machineEvents = 5
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "image" image 0xe8 True
        ]
    , machineActions = actions
    }

image :: Word32
image = 0x014ead40

state :: Word32 -> Action ()
state = store 0x014ea5b2 Half

void :: Word32 -> Action ()
void address = () <$ call address []

trace, s122062c, s1220bdc, s1220c94, s1220cb8, s1220cc8, s1220dd0, s1220e68 :: Word32
trace = 0x0121eb68
s122062c = 0x0122062c
s1220bdc = 0x01220bdc
s1220c94 = 0x01220c94
s1220cb8 = 0x01220cb8
s1220cc8 = 0x01220cc8
s1220dd0 = 0x01220dd0
s1220e68 = 0x01220e68

s1220f34, s1220f4c, s1220fd0, s1220fe0, s1220ff0 :: Word32
s1220f34 = 0x01220f34
s1220f4c = 0x01220f4c
s1220fd0 = 0x01220fd0
s1220fe0 = 0x01220fe0
s1220ff0 = 0x01220ff0

failed :: Word32 -> Action a
failed status = store (image + 0xdc) Word status >> finish 12

settle :: Word32 -> Word32 -> Action a
settle st status = if status == 0 then state st >> finish 0 else failed status

released :: Action a
released = state 0 >> void s1220fd0 >> finish 4

actions :: [Action Word32]
actions =
  [ void s122062c >> callWith s1220bdc 0 [0] >>= settle 1
  , void trace >> finish 0
  , do
      r <- callWith s1220f34 0 [0]
      when (r /= 0) (call s1220f4c [0] >>= settle 3)
      q <- call s1220dd0 [0]
      when (q /= 0) (state 3 >> void s1220e68 >> finish 0)
      call s1220cc8 [0] >>= settle 2
  , state 0 >> finish 4
  , void s1220c94 >> finish 0
  , void s1220cb8 >> finish 0
  , finish 0
  , do
      f <- load (image + 0xe0) Byte [1]
      when (f == 1) released
      n <- load (image + 0xe4) Word [0]
      when (n == 0) released
      void s1220fd0
      status <- callWith s1220bdc 1 [0]
      when (status == 0) (state 1 >> finish 0)
      state 0
      failed status
  , do
      status <- call s1220ff0 [0]
      state 0
      void s1220fe0
      when (status == 0) (finish 4)
      failed status
  ]
