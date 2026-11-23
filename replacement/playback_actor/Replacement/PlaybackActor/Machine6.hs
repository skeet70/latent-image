-- | A path model of the stock machine-6 action bytes in [0x011eb500,0x011eb824), table indices
-- 2..11 of 0x012193c0.
module Replacement.PlaybackActor.Machine6 (machine6) where

import Control.Monad (when)
import Data.Int (Int32)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine6 :: Machine
machine6 =
  Machine
    { machineName = "machine6"
    , machineIndex = 6
    , machineTransitions = 0x01216548
    , machineStates = 6
    , machineEvents = 3
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        , Region "io" io 0x30 True
        ]
    , machineActions = actions
    }

request, io :: Word32
request = 0x014ea6e4
io = 0x014eaca8

state :: Word32 -> Action ()
state = store 0x014ea5b8 Half

void :: Word32 -> Action ()
void address = () <$ call address []

req, ioWord :: Word32 -> [Word32] -> Action Word32
req offset values = load (request + offset) Word values
ioWord offset values = load (io + offset) Word values

setIo :: Word32 -> Word32 -> Action ()
setIo offset = store (io + offset) Word

s1220314, s1220340, s1220378, s12203a4, s12203d0, s12203f4, s1220480, s12204cc, s1220560, trace :: Word32
s1220314 = 0x01220314
s1220340 = 0x01220340
s1220378 = 0x01220378
s12203a4 = 0x012203a4
s12203d0 = 0x012203d0
s12203f4 = 0x012203f4
s1220480 = 0x01220480
s12204cc = 0x012204cc
s1220560 = 0x01220560
trace = 0x0121eb68

submit :: Action a
submit = do
  state 1
  setIo 0xc 0
  setIo 0x10 0
  setIo 0x2c 0
  void s1220314
  finish 0

-- | The +0x1fc branch of actions 3, 4 and 5: nonzero takes the deferred path with the code.
deferring :: Word32 -> Action ()
deferring code = do
  w <- req 0x1fc [0]
  when (w /= 0) $ do
    state 5
    _ <- callWith s12203f4 code []
    void s12203d0
    void s1220480
    finish 0

actions :: [Action Word32]
actions =
  [ do
      c <- ioWord 0xc [1, 2]
      case c of
        1 -> submit
        2 -> setIo 0xc 0 >> finish 4
        _ -> finish 0
  , do
      deferring 1
      void s12204cc
      lo <- ioWord 0x20 [0, 1]
      hi <- ioWord 0x24 [0, 1, 0xffffffff]
      if (fromIntegral hi :: Int32) > 0 || (hi == 0 && lo /= 0)
        then state 2 >> void s1220340 >> finish 0
        else state 3 >> void s1220378 >> finish 0
  , deferring 2 >> state 3 >> void s1220378 >> finish 0
  , deferring 3 >> state 4 >> void s1220560 >> void s12203a4 >> finish 0
  , do
      w <- req 0x1fc [0]
      state 0
      if w == 0
        then setIo 0xc 0 >> finish 4
        else do
          _ <- callWith s12203f4 4 []
          setIo 0xc 0
          void s1220480
          finish 12
  , do
      t <- req 0x1f8 [14]
      when (t /= 14) (finish 0)
      p <- ioWord 0x10 [0]
      state 0
      setIo 0xc 0
      finish (if p /= 0 then 12 else 4)
  , submit
  , void trace >> finish 0
  , finish 4
  , state 5 >> void s12203d0 >> finish 0
  ]
