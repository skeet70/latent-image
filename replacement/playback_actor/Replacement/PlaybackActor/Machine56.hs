-- | A path model of the stock machine-56 action bytes in [0x011fd988,0x011fdc1c), table
-- indices 2..18 of 0x01217350.
module Replacement.PlaybackActor.Machine56 (machine56) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine56 :: Machine
machine56 =
  Machine
    { machineName = "machine56"
    , machineIndex = 56
    , machineTransitions = 0x012162ec
    , machineStates = 6
    , machineEvents = 6
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" 0x014ea6e4 0x200 True
        ]
    , machineActions = actions
    }

state :: Word32 -> Action ()
state = store 0x014ea61c Half

void :: Word32 -> Action ()
void address = () <$ call address []

s1211250, s121132c, s12135e8, trace, s122b3f0, s122b440, s122b468, s122b800 :: Word32
s1211250 = 0x01211250
s121132c = 0x0121132c
s12135e8 = 0x012135e8
trace = 0x0121eb68
s122b3f0 = 0x0122b3f0
s122b440 = 0x0122b440
s122b468 = 0x0122b468
s122b800 = 0x0122b800

passOn :: Word32 -> Action Word32
passOn address = call address [] >>= finish

advance :: Word32 -> Word32 -> Action Word32
advance address st = do
  r <- call address [0, 4]
  when (r == 0) (state st >> finish 0)
  finish (if r == 4 then 4 else 12)

arm :: Word32 -> Action Word32
arm address = do
  r <- call address [0]
  when (r /= 0) (state 4 >> void s122b3f0)
  finish 0

actions :: [Action Word32]
actions =
  [ finish 8
  , state 5 >> passOn s1211250
  , state 5 >> passOn s121132c
  , passOn s12135e8
  , state 5 >> finish 0
  , void trace >> finish 0
  , passOn s1211250
  , passOn s121132c
  , finish 0
  , advance s1211250 1
  , advance s121132c 2
  , advance s12135e8 3
  , arm s1211250
  , arm s121132c
  , arm s12135e8
  , do
      state 0
      w <- load (0x014ea6e4 + 0x1fc) Word [0]
      if w == 0 then void s122b440 else void s122b468
      r <- call s122b800 [0]
      finish (if r == 0 then 12 else 4)
  , state 0 >> finish 8
  ]
