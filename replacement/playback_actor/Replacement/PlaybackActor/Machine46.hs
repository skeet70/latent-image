-- | A path model of the stock machine-46 action bytes in [0x011fa618,0x011facec), table
-- indices 2..15 of 0x01216ea0.
module Replacement.PlaybackActor.Machine46 (machine46) where

import Control.Monad (when)
import Data.Bits ((.&.))
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine46 :: Machine
machine46 =
  Machine
    { machineName = "machine46"
    , machineIndex = 46
    , machineTransitions = 0x012199f0
    , machineStates = 8
    , machineEvents = 4
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" 0x014ea6e4 0x30 True
        , Region "block" block 0x58 True
        ]
    , machineActions = actions
    }

block :: Word32
block = 0x014ff36c

-- | A word inside the block region that no action names, for the +0x10 pointer to point at.
counterPointee :: Word32
counterPointee = block + 0x20

state :: Word32 -> Action ()
state = store 0x014ea608 Half

clearCode :: Action ()
clearCode = store 0x014ea6e4 Half 0

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

word :: Word32 -> [Word32] -> Action Word32
word offset = load (block + offset) Word

byte :: Word32 -> [Word32] -> Action Word32
byte offset = load (block + offset) Byte

dispatch3, s120bfd8, s120c1a8, trace, s121ffc0, s12202d0, s12205e4, s123b4b4, s123b4d4 :: Word32
dispatch3 = 0x0120bc20
s120bfd8 = 0x0120bfd8
s120c1a8 = 0x0120c1a8
trace = 0x0121eb68
s121ffc0 = 0x0121ffc0
s12202d0 = 0x012202d0
s12205e4 = 0x012205e4
s123b4b4 = 0x0123b4b4
s123b4d4 = 0x0123b4d4

s123b4f4, s123b5a8, s123b688, s123b76c, s123b960, s123ba6c, s123bb74, s123bf2c :: Word32
s123b4f4 = 0x0123b4f4
s123b5a8 = 0x0123b5a8
s123b688 = 0x0123b688
s123b76c = 0x0123b76c
s123b960 = 0x0123b960
s123ba6c = 0x0123ba6c
s123bb74 = 0x0123bb74
s123bf2c = 0x0123bf2c

failWith :: Word32 -> Action a
failWith code = voidWith s123b4b4 code >> finish 12

abort :: Word32 -> Action a
abort code = state 0 >> failWith code

idle :: Action a
idle = state 0 >> finish 4

release :: Action ()
release = voidWith s123b4d4 1

-- | 0x011fa668..0x011fa6c4 and 0x011fab4c..0x011faba8.
second :: Action Word32
second = do
  a <- call s123bf2c [0]
  when (a == 0) (abort 2)
  b <- call s123b5a8 [0]
  when (b == 0) (failWith 2)
  state 2
  call s120bfd8 [] >>= finish

settle :: Word32 -> Action Word32
settle service = do
  r <- call service [0]
  if r /= 0 then idle else finish 0

submit :: Word32 -> Word32 -> Action a
submit service st = do
  void service
  r <- call dispatch3 [0]
  if r == 0 then state st >> finish 0 else abort 4

pendingIdle :: Action ()
pendingIdle = do
  p <- word 0x4 [1]
  when (p == 1) idle

-- | The service's result once it is nonzero and nothing is pending.
settled :: Word32 -> Action Word32
settled service = do
  r <- call service [0, 4]
  when (r == 0) (finish 0)
  pendingIdle
  pure r

actions :: [Action Word32]
actions =
  [ do
      w <- word 0x14 [0]
      if w == 0
        then second
        else do
          r <- call s123b4f4 [0]
          when (r == 0) (failWith 1)
          state 1
          call s120c1a8 [] >>= finish
  , void trace >> finish 0
  , finish 4
  , release >> void s12202d0 >> settle s120c1a8
  , release >> void s121ffc0 >> settle s120bfd8
  , release >> finish 0
  , release >> void s12205e4 >> settle dispatch3
  , do
      pendingIdle
      q <- word 0x50 [1]
      when (q == 1) idle
      clearCode
      submit s123b76c 4
  , do
      pendingIdle
      f <- byte 0x54 [0]
      when (f /= 0) (clearCode >> submit s123ba6c 6)
      g <- byte 0x56 [0, 2]
      when (g .&. 2 /= 0) (clearCode >> submit s123bb74 7)
      idle
  , do
      r <- settled dispatch3
      when (r /= 4) (abort 4)
      g <- byte 0x56 [0, 2]
      if g .&. 2 /= 0 then submit s123bb74 7 else idle
  , do
      r <- settled dispatch3
      if r == 4 then idle else abort 4
  , do
      r <- settled s120c1a8
      when (r /= 4) (abort 1)
      p <- loadOnly (block + 0x10) Word [counterPointee]
      v <- load p Word []
      s <- word 0x14 []
      store p Word (v + s)
      second
  , do
      r <- settled s120bfd8
      when (r /= 4) (abort 3)
      state 3
      void s123b688
      finish 0
  , do
      r <- settled dispatch3
      when (r /= 4) (abort 4)
      state 5
      void s123b960
      finish 0
  ]
