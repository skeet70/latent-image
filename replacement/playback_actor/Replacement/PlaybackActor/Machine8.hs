-- | A path model of the stock machine-8 action bytes in [0x011ec980,0x011ecb1c), table indices
-- 2..11 of 0x0121a390.
module Replacement.PlaybackActor.Machine8 (machine8) where

import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine8 :: Machine
machine8 =
  Machine
    { machineName = "machine8"
    , machineIndex = 8
    , machineTransitions = 0x01218bbc
    , machineStates = 1
    , machineEvents = 10
    , machineRegions = [Region "machines" 0x014ea4ec 0x1c0 False]
    , machineActions = actions
    }

-- | The state halfword of machine n.
peer :: Word32 -> Word32 -> Action ()
peer n = store (0x014ea5ac + 2 * n) Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

s121de50, s121de64, s121de80, s121ded4, s121df20, s121df30 :: Word32
s121de50 = 0x0121de50
s121de64 = 0x0121de64
s121de80 = 0x0121de80
s121ded4 = 0x0121ded4
s121df20 = 0x0121df20
s121df30 = 0x0121df30

dpof :: Word32 -> Action Word32
dpof argument = peer 34 3 >> peer 2 5 >> voidWith s121de80 argument >> finish 0

transfer :: Word32 -> Action Word32
transfer argument = peer 34 9 >> peer 66 4 >> peer 70 8 >> voidWith s121ded4 argument >> finish 0

actions :: [Action Word32]
actions =
  [ peer 9 1 >> void s121de50 >> finish 0
  , dpof 0
  , dpof 1
  , transfer 0
  , transfer 1
  , peer 22 1 >> void s121df20 >> finish 0
  , peer 53 1 >> void s121df30 >> finish 0
  , peer 52 1 >> void s121df30 >> finish 0
  , peer 9 1 >> void s121de64 >> finish 0
  , finish 255
  ]
