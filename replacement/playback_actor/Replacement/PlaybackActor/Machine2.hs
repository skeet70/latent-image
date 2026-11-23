-- | A path model of the stock machine-2 action bytes in [0x011ea5f0,0x011eaa54), table
-- indices 2..15 of 0x0121739c.
module Replacement.PlaybackActor.Machine2 (machine2) where

import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine2 :: Machine
machine2 =
  Machine
    { machineName = "machine2"
    , machineIndex = 2
    , machineTransitions = 0x01216334
    , machineStates = 7
    , machineEvents = 12
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        ]
    , machineActions = actions
    }

request :: Word32
request = 0x014ea6e4

state :: Word32 -> Action ()
state = store 0x014ea5b0 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

req :: Word32 -> [Word32] -> Action Word32
req offset values = load (request + offset) Word values

s0ec026c, s0ec03dc, publish, workState, s121daa0, s121daf4, s121db3c, s121db7c, s121dbb4 :: Word32
s0ec026c = 0x00ec026c
s0ec03dc = 0x00ec03dc
publish = 0x0120c650
workState = 0x0121ca80
s121daa0 = 0x0121daa0
s121daf4 = 0x0121daf4
s121db3c = 0x0121db3c
s121db7c = 0x0121db7c
s121dbb4 = 0x0121dbb4

s121dc40, s121dc78, s121dcac, s121dd98, s121ddd0, s121ddf0, s121de10, s121eabc, trace :: Word32
s121dc40 = 0x0121dc40
s121dc78 = 0x0121dc78
s121dcac = 0x0121dcac
s121dd98 = 0x0121dd98
s121ddd0 = 0x0121ddd0
s121ddf0 = 0x0121ddf0
s121de10 = 0x0121de10
s121eabc = 0x0121eabc
trace = 0x0121eb68

-- | 0x011ea7ac (argument 0) and 0x011ea958 (argument 1).
consumeFlag :: Word32 -> Action Word32
consumeFlag argument = do
  c <- req 0x1c4 [0]
  state 0
  void s121dcac
  if c == 0
    then do
      f <- call s121ddf0 [0]
      if f == 0 then finish 4 else voidWith s121ddd0 0 >> finish 18
    else voidWith s121dc78 argument >> finish 12

-- | 0x011ea824 (3), 0x011ea8fc (1) and 0x011ea9d0 (2).
resolveSelection :: Word32 -> Action Word32
resolveSelection argument = do
  c <- req 0x1c4 [0]
  state 0
  void s121dcac
  if c == 0 then finish 4 else voidWith s121dc78 argument >> finish 12

transferFlags :: Action Word32
transferFlags = do
  state 0
  void s121dcac
  a <- call s0ec03dc [0]
  b <- if a == 0 then call s121eabc [0] else pure 0
  if a == 0 && b == 0
    then do
      f <- call s121de10 [0]
      if f /= 0
        then voidWith workState 5 >> void publish >> finish 11
        else voidWith workState 4 >> void publish >> finish 10
    else do
      f <- call s121de10 [0]
      finish (if f == 0 then 4 else 12)

actions :: [Action Word32]
actions =
  [ do
      k <- req 0x34 [50]
      state 3
      void (if k == 50 then s121daf4 else s121db3c)
      finish 0
  , void trace >> finish 0
  , do
      k <- req 0x34 [57, 64, 50]
      case k of
        57 -> state 4 >> voidWith s121daa0 1
        64 -> state 4 >> voidWith s121daa0 0
        50 -> state 1 >> void s121db7c
        _ -> state 6 >> void s121dd98
      finish 0
  , finish 8
  , state 0 >> finish 8
  , voidWith s121ddd0 1 >> finish 0
  , do
      r <- call s0ec026c [0]
      if r == 0
        then do
          state 2
          k <- req 0x34 [46]
          void (if k == 46 then s121dc40 else s121dbb4)
        else state 1 >> void s121db7c
      finish 0
  , consumeFlag 0
  , resolveSelection 3
  , transferFlags
  , resolveSelection 1
  , consumeFlag 1
  , resolveSelection 2
  , state 0 >> finish 4
  ]
