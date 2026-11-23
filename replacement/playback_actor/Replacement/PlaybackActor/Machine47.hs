-- | A path model of the stock machine-47 action bytes in [0x011facec,0x011fb44c), table
-- indices 2..13 of 0x01217708.
module Replacement.PlaybackActor.Machine47 (machine47) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine47 :: Machine
machine47 =
  Machine
    { machineName = "machine47"
    , machineIndex = 47
    , machineTransitions = 0x01218874
    , machineStates = 6
    , machineEvents = 3
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "block14ff3c4" 0x014ff874 4 True
        ]
    , machineActions = actions
    }

state :: Word32 -> Action ()
state = store 0x014ea60a Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

s120fe64, s120ff70, s12100d8, s12136fc, trace, s123a724, s123b470, s123c2c8, s123c2e8, s123c308 :: Word32
s120fe64 = 0x0120fe64
s120ff70 = 0x0120ff70
s12100d8 = 0x012100d8
s12136fc = 0x012136fc
trace = 0x0121eb68
s123a724 = 0x0123a724
s123b470 = 0x0123b470
s123c2c8 = 0x0123c2c8
s123c2e8 = 0x0123c2e8
s123c308 = 0x0123c308

s123c328, s123c490, s123c4c8, s123cb4c, s123cba8, s123cbe0, s123cc18, s123ce14, s123cf48 :: Word32
s123c328 = 0x0123c328
s123c490 = 0x0123c490
s123c4c8 = 0x0123c4c8
s123cb4c = 0x0123cb4c
s123cba8 = 0x0123cba8
s123cbe0 = 0x0123cbe0
s123cc18 = 0x0123cc18
s123ce14 = 0x0123ce14
s123cf48 = 0x0123cf48

s123d078, s123d16c, s123d24c, s123d264, s123d390, s123e298 :: Word32
s123d078 = 0x0123d078
s123d16c = 0x0123d16c
s123d24c = 0x0123d24c
s123d264 = 0x0123d264
s123d390 = 0x0123d390
s123e298 = 0x0123e298

abort :: Action () -> Action a
abort calls = state 0 >> calls >> finish 12

-- | The blocks at 0x011fad5c..0x011faecc (action 2) and 0x011fb08c..0x011fb204 (action 9).
started :: Word32 -> Word32 -> Word32 -> Action () -> Action Word32
started start wait next onFail = do
  void start
  r <- call wait [0]
  if r == 0 then state next >> finish 0 else abort onFail

-- | The 0x0123cb4c jump table: the action-2 bytes when the flag is False, action 9 when True.
selected :: Bool -> Action Word32
selected nine = do
  k <- call s123cb4c [0 .. 9]
  case k of
    _ | k == 0 || k == 5 -> abort (when nine (voidWith s123c2e8 2))
    _ | k == 1 || k == 2 -> started s123c490 s120ff70 1 (void s123c4c8)
    3 -> started s123ce14 s120fe64 2 (void (if nine then s123cbe0 else s123cba8) >> voidWith s123c308 0)
    _ | k == 4 || k == 7 -> started s123cf48 s120fe64 5 (void s123cba8 >> voidWith s123c308 1)
    6 -> started s123d16c s12136fc 3 (void (if nine then s123cba8 else s123cbe0) >> voidWith s123c308 0)
    8 -> started s123d078 s12100d8 4 (void s123cc18)
    9 -> abort (when nine (voidWith s123c2e8 12))
    _ -> abort (pure ())

-- | Actions 5..8.
waited :: Word32 -> Action Word32
waited wait = do
  r <- call wait [0]
  if r /= 0 then state 0 >> finish 4 else finish 0

-- | Actions 10, 12 and 13.
released :: Word32 -> Action () -> Action Word32
released wait onFail = do
  r <- call wait [0, 4]
  case r of
    0 -> finish 0
    4 -> do
      q <- call s123d24c [0]
      state 0
      when (q /= 0) (void s123d264)
      finish 4
    _ -> abort onFail

actions :: [Action Word32]
actions =
  [ do
      void s123c328
      r <- call s120ff70 [0, 4]
      when (r == 0) (state 1 >> finish 0)
      if r == 4 then selected False else abort (void s123c4c8)
  , void trace >> finish 0
  , finish 4
  , voidWith s123c2c8 1 >> void s123a724 >> waited s120ff70
  , void s123d390 >> waited s120fe64
  , void s123e298 >> waited s12136fc
  , void s123b470 >> waited s12100d8
  , do
      r <- call s120ff70 [0, 4]
      when (r == 0) (finish 0)
      w <- load 0x014ff874 Word [1]
      when (w == 1) (state 0 >> finish 4)
      if r == 4 then selected True else abort (void s123c4c8)
  , released s120fe64 (void s123cba8 >> voidWith s123c308 0)
  , do
      r <- call s12136fc [0, 4]
      case r of
        0 -> finish 0
        4 -> state 0 >> finish 4
        _ -> abort (void s123cbe0 >> voidWith s123c308 0)
  , released s12100d8 (void s123cc18)
  , released s120fe64 (void s123cba8 >> voidWith s123c308 1)
  ]
