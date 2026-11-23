-- | A path model of the stock machine-57 action bytes in [0x011fdc1c,0x011fdd38), table
-- indices 2..8 of 0x01216bf4.
module Replacement.PlaybackActor.Machine57 (machine57) where

import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine57 :: Machine
machine57 =
  Machine
    { machineName = "machine57"
    , machineIndex = 57
    , machineTransitions = 0x01217170
    , machineStates = 2
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
state = store 0x014ea61e Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

req :: Word32 -> [Word32] -> Action Word32
req offset values = load (request + offset) Word values

trace, s122b48c, s122b4d8, s122b504 :: Word32
trace = 0x0121eb68
s122b48c = 0x0122b48c
s122b4d8 = 0x0122b4d8
s122b504 = 0x0122b504

actions :: [Action Word32]
actions =
  [ finish 8
  , state 0 >> void s122b504 >> finish 0
  , state 0 >> void s122b504 >> finish 19
  , do
      kind <- req 0xf8 [48]
      state 1
      voidWith s122b48c (if kind == 48 then 1 else 0)
      finish 0
  , void trace >> finish 0
  , finish 4
  , do
      pending <- req 0x1fc [0]
      state 0
      if pending == 0 then finish 4 else void s122b4d8 >> finish 12
  ]
