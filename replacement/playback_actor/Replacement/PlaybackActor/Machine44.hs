-- | A path model of the stock machine-44 action bytes in [0x011f97b4,0x011fa1b4), table
-- indices 2..20 of 0x01217c84.
module Replacement.PlaybackActor.Machine44 (machine44) where

import Control.Monad (when)
import Data.Bits ((.&.))
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine44 :: Machine
machine44 =
  Machine
    { machineName = "machine44"
    , machineIndex = 44
    , machineTransitions = 0x0121996c
    , machineStates = 13
    , machineEvents = 5
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        , Region "stream" stream 0x80 True
        ]
    , machineActions = actions
    }

request, stream :: Word32
request = 0x014ea6e4
stream = 0x014ef894

-- | Words inside the stream region that no action names, for the pointers at +0 and +0x4c.
currentPointee, targetPointee :: Word32
currentPointee = stream + 0x20
targetPointee = stream + 0x30

state :: Word32 -> Action ()
state = store 0x014ea604 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

ld :: Word32 -> [Word32] -> Action Word32
ld offset values = load (stream + offset) Word values

dispatch3, s120bfd8, s120c1a8, trace, s121ff9c, s121ffc0, s12202d0, s12205e4, s12293a8 :: Word32
dispatch3 = 0x0120bc20
s120bfd8 = 0x0120bfd8
s120c1a8 = 0x0120c1a8
trace = 0x0121eb68
s121ff9c = 0x0121ff9c
s121ffc0 = 0x0121ffc0
s12202d0 = 0x012202d0
s12205e4 = 0x012205e4
s12293a8 = 0x012293a8

s123d3d4, s123d3f4, s123d414, s123d454, s123d4d0, s123d54c, s123d584, s123d87c, s123d89c :: Word32
s123d3d4 = 0x0123d3d4
s123d3f4 = 0x0123d3f4
s123d414 = 0x0123d414
s123d454 = 0x0123d454
s123d4d0 = 0x0123d4d0
s123d54c = 0x0123d54c
s123d584 = 0x0123d584
s123d87c = 0x0123d87c
s123d89c = 0x0123d89c

s123d8bc, s123da10, s123db1c, s123dc24, s123dd1c, s123dd38, s123ddc0, s123de08, s123de38 :: Word32
s123d8bc = 0x0123d8bc
s123da10 = 0x0123da10
s123db1c = 0x0123db1c
s123dc24 = 0x0123dc24
s123dd1c = 0x0123dd1c
s123dd38 = 0x0123dd38
s123ddc0 = 0x0123ddc0
s123de08 = 0x0123de08
s123de38 = 0x0123de38

s123df40, s123e084, s123e098 :: Word32
s123df40 = 0x0123df40
s123e084 = 0x0123e084
s123e098 = 0x0123e098

failWith :: Word32 -> Action a
failWith code = state 0 >> voidWith s123d3f4 code >> finish 12

abandon :: Action a
abandon = state 0 >> voidWith s123d3f4 1 >> void s121ff9c >> void s120bfd8 >> finish 12

busy :: Action ()
busy = do
  w <- ld 0x44 [1]
  when (w == 1) (state 0 >> finish 4)

settle :: Word32 -> Action a
settle r = if r /= 0 then state 0 >> finish 4 else finish 0

resume :: Action a
resume = do
  p <- loadOnly stream Word [currentPointee]
  v <- load p Word [2]
  if v == 2
    then state 4 >> void s123ddc0 >> finish 0
    else state 2 >> void s123d89c >> call s120bfd8 [] >>= finish

step :: Word32 -> Action a
step st = do
  r <- call dispatch3 [0]
  if r == 0 then state st >> finish 0 else failWith 11

selectB7c :: Action a
selectB7c = do
  b <- load (stream + 0x7c) Byte [2, 0]
  if b .&. 2 /= 0 then void s123dc24 >> step 12 else state 0 >> finish 4

mark :: Action ()
mark = voidWith s123d3d4 1

-- | The head of actions 13 to 20: the service's result, past its zero and busy exits.
gated :: Word32 -> [Word32] -> Action Word32
gated service results = do
  r <- call service (0 : results)
  when (r == 0) (finish 0)
  busy
  pure r

dispatched :: Action ()
dispatched = do
  r <- gated dispatch3 [4]
  when (r /= 4) (failWith 11)

actions :: [Action Word32]
actions =
  [ do
      w <- ld 0x58 [1]
      when (w == 1) resume
      r <- call s123d454 [0]
      when (r == 0) resume
      q <- call s123d4d0 [0]
      when (q == 0) (voidWith s123d3f4 1 >> finish 12)
      state 1
      call s120c1a8 [] >>= finish
  , void trace >> finish 0
  , finish 4
  , mark >> void s12202d0 >> call s120c1a8 [0] >>= settle
  , mark >> void s121ffc0 >> call s120bfd8 [0] >>= settle
  , mark >> void s121ff9c >> void s120bfd8 >> void s12202d0 >> call s120c1a8 [0] >>= settle
  , mark >> finish 0
  , mark >> void s12205e4 >> call dispatch3 [0] >>= settle
  , do
      busy
      w <- ld 0x5c [1]
      when (w == 1) (state 0 >> finish 4)
      void s123d8bc
      step 9
  , busy >> state 8 >> void s123de38 >> finish 0
  , do
      busy
      void s123dd1c
      p <- ld 0 []
      voidWith s12293a8 p
      r <- call s123d414 [0]
      when (r /= 0) (void s123dd38)
      b <- load (stream + 0x64) Byte [0]
      when (b /= 0) (void s123db1c >> step 11)
      selectB7c
  , do
      r <- gated dispatch3 [4]
      if r == 4 then selectB7c else failWith 11
  , do
      r <- gated dispatch3 [4]
      if r == 4 then state 0 >> finish r else failWith 11
  , do
      r <- gated s120bfd8 [4, 9]
      case r of
        4 -> do
          q <- call s123de08 [0]
          if q /= 0
            then state 7 >> void s123df40 >> finish 0
            else state 8 >> void s123de38 >> finish 0
        9 -> do
          q <- call s123d584 [0]
          when (q == 0) abandon
          state 3
          t <- loadOnly (stream + 0x4c) Word [targetPointee]
          store t Word 0
          call s120c1a8 [] >>= finish
        _ -> failWith 9
  , do
      r <- gated s120c1a8 [4]
      when (r /= 4) (failWith 1)
      void s123d54c
      resume
  , do
      h <- load request Half [0x1410]
      when (h /= 0x1410) (finish 0)
      r <- gated s120c1a8 [4]
      when (r /= 4) abandon
      state 2
      void s123d87c
      call s120bfd8 [] >>= finish
  , dispatched >> void s123e084 >> step 6
  , dispatched >> state 10 >> void s123da10 >> finish 0
  , dispatched >> state 10 >> void s123e098 >> void s123da10 >> finish 0
  ]
