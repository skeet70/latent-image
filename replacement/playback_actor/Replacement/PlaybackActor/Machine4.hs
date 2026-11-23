-- | A path model of the stock machine-4 action bytes in [0x011eacb0,0x011eb31c), table
-- indices 2..19 of 0x0121a3c0.
module Replacement.PlaybackActor.Machine4 (machine4) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine4 :: Machine
machine4 =
  Machine
    { machineName = "machine4"
    , machineIndex = 4
    , machineTransitions = 0x012194d4
    , machineStates = 3
    , machineEvents = 10
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "storage" storage 0x1c True
        ]
    , machineActions = actions
    }

storage :: Word32
storage = 0x01511338

state :: Word32 -> Action ()
state = store 0x014ea5b4 Half

void :: Word32 -> Action ()
void address = () <$ call address []

trace, s120e6e8, s123261c, s12390d4, s1239178, s1249f84, s1249fa0, s124a010, s124a0a8, s124a0d4 :: Word32
trace = 0x0121eb68
s120e6e8 = 0x0120e6e8
s123261c = 0x0123261c
s12390d4 = 0x012390d4
s1239178 = 0x01239178
s1249f84 = 0x01249f84
s1249fa0 = 0x01249fa0
s124a010 = 0x0124a010
s124a0a8 = 0x0124a0a8
s124a0d4 = 0x0124a0d4

s124a3c0, s124a42c, s124a480, s124a4b0, s124a518, s124a54c, s124a628, s124a650, s124a66c, s124a688 :: Word32
s124a3c0 = 0x0124a3c0
s124a42c = 0x0124a42c
s124a480 = 0x0124a480
s124a4b0 = 0x0124a4b0
s124a518 = 0x0124a518
s124a54c = 0x0124a54c
s124a628 = 0x0124a628
s124a650 = 0x0124a650
s124a66c = 0x0124a66c
s124a688 = 0x0124a688

s124a698, s124a8c0, s124a8d4, s124a8fc :: Word32
s124a698 = 0x0124a698
s124a8c0 = 0x0124a8c0
s124a8d4 = 0x0124a8d4
s124a8fc = 0x0124a8fc

mark :: Word32 -> Action ()
mark bit = () <$ callWith s124a8d4 bit []

returnWord :: Action a
returnWord = state 0 >> load (storage + 0x10) Word [] >>= finish

-- | The bytes from the 0x0124a0a8 call on; @checked@ adds the 0x0124a518 test.
tailFrom :: Bool -> Action Word32
tailFrom checked = do
  complete <- call s124a0a8 [0]
  when (complete == 0) (finish 0)
  call s124a0d4 [] >>= store (storage + 0x10) Word
  when checked $ do
    r <- call s124a518 [0]
    when (r == 0) returnWord
  delay <- call s1239178 [0]
  when (delay /= 0) (finish 0)
  state 0
  void s12390d4
  load (storage + 0x10) Word [] >>= finish

marked :: Word32 -> Bool -> Action Word32
marked bit checked = mark bit >> tailFrom checked

pending :: Bool -> Action Word32
pending checked = do
  mark 4
  p <- load (storage + 0x0c) Word [0]
  when (p /= 0) (void s124a4b0 >> void s120e6e8)
  tailFrom checked

actions :: [Action Word32]
actions =
  [ do
      state 1
      mapM_ void [s1249f84, s1249fa0, s124a010, s124a8fc, s124a8c0]
      r <- call s124a54c [0]
      void (if r /= 0 then s124a650 else s124a698)
      finish 0
  , void trace >> finish 0
  , finish 8
  , state 2 >> store (storage + 1) Byte 1 >> finish 0
  , marked 1 True
  , marked 1 False
  , marked 8 True
  , marked 8 False
  , do
      void s124a42c
      void s123261c
      r <- call s124a3c0 [1]
      when (r == 1) (void s124a480 >> finish 0)
      marked 2 True
  , marked 2 False
  , marked 2 True
  , void s124a698 >> finish 0
  , do
      r <- call s124a628 [0]
      void (if r /= 0 then s124a66c else s124a688)
      finish 0
  , pending True
  , pending False
  , void s120e6e8 >> finish 0
  , do
      void s120e6e8
      r <- call s124a518 [0]
      when (r == 0) (finish 0)
      complete <- call s124a0a8 [0]
      when (complete == 0) (finish 0)
      state 0
      void s12390d4
      load (storage + 0x10) Word [] >>= finish
  , do
      void s120e6e8
      void s12390d4
      complete <- call s124a0a8 [0]
      if complete /= 0 then returnWord else finish 0
  ]
