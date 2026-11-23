-- | A path model of the stock machine-34 action bytes in [0x011f62c8,0x011f66ec), table
-- indices 2..14 of 0x012177ec.
module Replacement.PlaybackActor.Machine34 (machine34) where

import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine34 :: Machine
machine34 =
  Machine
    { machineName = "machine34"
    , machineIndex = 34
    , machineTransitions = 0x0121a564
    , machineStates = 10
    , machineEvents = 12
    , machineRegions = [Region "machines" 0x014ea4ec 0x1c0 False]
    , machineActions = actions
    }

state :: Word32 -> Action ()
state = store 0x014ea5f0 Half

s120b9e0, s120d22c, s120e42c, s120e830, s120eb10, s12108d0, s12110f8, s1211dd0, s1213c50, trace :: Word32
s120b9e0 = 0x0120b9e0
s120d22c = 0x0120d22c
s120e42c = 0x0120e42c
s120e830 = 0x0120e830
s120eb10 = 0x0120eb10
s12108d0 = 0x012108d0
s12110f8 = 0x012110f8
s1211dd0 = 0x01211dd0
s1213c50 = 0x01213c50
trace = 0x0121eb68

-- | @unsettled@ are further compared values, which store @pending@.
settle :: Word32 -> [Word32] -> [Word32] -> Word32 -> Action Word32
settle service settled unsettled pending = do
  r <- call service (settled ++ unsettled)
  state (if r `elem` settled then 0 else pending)
  finish r

actions :: [Action Word32]
actions =
  [ settle s120d22c [4, 8, 12, 255] [0] 1
  , settle s12110f8 [4, 8, 12, 255] [0] 2
  , settle s120b9e0 [4, 8, 12, 18, 255] [0] 3
  , settle s120e42c [4, 8, 12, 15, 16, 255] [0, 14] 4
  , settle s120e830 [4, 8, 12, 255] [0] 5
  , settle s120eb10 [4, 8, 12] [0] 6
  , settle s12108d0 [4, 8, 12] [0] 7
  , settle s1213c50 [4, 8, 12] [0] 8
  , settle s1211dd0 [4, 8, 12] [0] 9
  , finish 8
  , call trace [] >> finish 0
  , settle s120d22c [4, 5, 8, 12, 255] [0] 1
  , do
      r <- call s120b9e0 [4, 8, 12, 18, 255, 10, 11, 0]
      state (if r `elem` [4, 8, 12, 18, 255] then 0 else if r `elem` [10, 11] then 9 else 3)
      finish r
  ]
