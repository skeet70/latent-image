-- | A path model of the stock machine-5 action bytes in [0x011eb31c,0x011eb500), table
-- indices 2..10 of 0x012175bc.
module Replacement.PlaybackActor.Machine5 (machine5) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine5 :: Machine
machine5 =
  Machine
    { machineName = "machine5"
    , machineIndex = 5
    , machineTransitions = 0x01219c6c
    , machineStates = 3
    , machineEvents = 7
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "jpeg" jpeg 0x64 True
        ]
    , machineActions = actions
    }

jpeg :: Word32
jpeg = 0x014eae2c

state :: Word32 -> Action ()
state = store 0x014ea5b6 Half

void :: Word32 -> Action ()
void address = () <$ call address []

trace, s12200c4, s12200ec, s1220120, s1220238, s122026c, s1220294 :: Word32
trace = 0x0121eb68
s12200c4 = 0x012200c4
s12200ec = 0x012200ec
s1220120 = 0x01220120
s1220238 = 0x01220238
s122026c = 0x0122026c
s1220294 = 0x01220294

concluded :: Action ()
concluded = do
  phase <- load (jpeg + 0x58) Word [2]
  when (phase == 2) (state 0 >> finish 12)
  flag <- load (jpeg + 0x60) Byte [1]
  when (flag == 1) (state 0 >> finish 4)

actions :: [Action Word32]
actions =
  [ state 1 >> void s12200c4 >> void s1220294 >> finish 0
  , void trace >> finish 0
  , void s12200c4 >> void s12200ec >> finish 0
  , state 0 >> void s122026c >> finish 4
  , finish 4
  , void s1220238 >> finish 0
  , void s1220238 >> store (jpeg + 0x58) Word 2 >> finish 0
  , do
      concluded
      state 0
      void s122026c
      pending <- load (jpeg + 0x28) Word [0]
      when (pending /= 0) (store (jpeg + 0x58) Word 1 >> finish 12)
      finish 4
  , concluded >> state 2 >> void s1220120 >> finish 9
  ]
