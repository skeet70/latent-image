-- | A path model of the stock machine-66 action bytes in [0x011ffd3c,0x011ffed0), table
-- indices 2..8 of 0x01218f5c.
module Replacement.PlaybackActor.Machine66 (machine66) where

import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine66 :: Machine
machine66 =
  Machine
    { machineName = "machine66"
    , machineIndex = 66
    , machineTransitions = 0x01219cbc
    , machineStates = 5
    , machineEvents = 4
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        ]
    , machineActions = actions
    }

request :: Word32
request = 0x014ea6e4

state :: Word32 -> Action ()
state = store 0x014ea630 Half

void :: Word32 -> Action ()
void address = () <$ call address []

s1211e78, s1211fdc, s1212120, s1212310, trace :: Word32
s1211e78 = 0x01211e78
s1211fdc = 0x01211fdc
s1212120 = 0x01212120
s1212310 = 0x01212310
trace = 0x0121eb68

plain :: Word32 -> Word32 -> Action Word32
plain service st = do
  r <- call service [0]
  state (if r == 0 then st else 0)
  finish r

routed :: Word32 -> Word32 -> Action Word32
routed service st = do
  r <- call service [0]
  if r == 0 then state st >> finish 0 else state 0 >> finish r

actions :: [Action Word32]
actions =
  [ finish 8
  , do
      r <- call s1211fdc [0, 1]
      state (if r <= 1 then 1 - r else 0)
      finish r
  , plain s1211e78 2
  , plain s1212120 3
  , plain s1212310 4
  , do
      code <- load (request + 0x34) Word [126, 151, 112]
      case code of
        126 -> routed s1212120 3
        151 -> routed s1211e78 2
        112 -> routed s1211fdc 1
        _ -> routed s1212310 4
  , void trace >> finish 0
  ]
