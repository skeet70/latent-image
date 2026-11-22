-- | A path model of the stock machine-30 action bytes in [0x011f31bc,0x011f4810), table
-- indices 2..35 of 0x01219888.
module Replacement.PlaybackActor.Machine30 (machine30) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine30 :: Machine
machine30 =
  Machine
    { machineName = "machine30"
    , machineIndex = 30
    , machineTransitions = 0x0121797c
    , machineStates = 20
    , machineEvents = 9
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" 0x014ea6e4 0x30 True
        , Region "transfer" transfer 0x6f8 True
        ]
    , machineActions = actions
    }

transfer :: Word32
transfer = 0x014eaed8

field :: Word32 -> Word32
field offset = transfer + offset

state :: Word32 -> Action ()
state = store 0x014ea5e8 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

-- Services
dispatch3, s120ce60, s120ec28, dispatch46, s12119a4, s1212a48, s121c9f8, trace, s12205e4 :: Word32
dispatch3 = 0x0120bc20
s120ce60 = 0x0120ce60
s120ec28 = 0x0120ec28
dispatch46 = 0x012101bc
s12119a4 = 0x012119a4
s1212a48 = 0x01212a48
s121c9f8 = 0x0121c9f8
trace = 0x0121eb68
s12205e4 = 0x012205e4

s122586c, s12258bc, s1225928, setPhase, startStep, s1225a98, s1225b20, s1225bfc, s1225d08 :: Word32
s122586c = 0x0122586c
s12258bc = 0x012258bc
s1225928 = 0x01225928
setPhase = 0x01225984
startStep = 0x012259a4
s1225a98 = 0x01225a98
s1225b20 = 0x01225b20
s1225bfc = 0x01225bfc
s1225d08 = 0x01225d08

s1225f64, s1225fb0, s1225fdc, s1226014, s1226230, s12262fc, s12263ec, s12264fc, s1226518 :: Word32
s1225f64 = 0x01225f64
s1225fb0 = 0x01225fb0
s1225fdc = 0x01225fdc
s1226014 = 0x01226014
s1226230 = 0x01226230
s12262fc = 0x012262fc
s12263ec = 0x012263ec
s12264fc = 0x012264fc
s1226518 = 0x01226518

s12265cc, s1226684, s122669c, s1229038, s1229068, s122925c, s12292a4, s12293a8, s12294cc :: Word32
s12265cc = 0x012265cc
s1226684 = 0x01226684
s122669c = 0x0122669c
s1229038 = 0x01229038
s1229068 = 0x01229068
s122925c = 0x0122925c
s12292a4 = 0x012292a4
s12293a8 = 0x012293a8
s12294cc = 0x012294cc

s122962c, s122968c, s12296dc, s1229800, s1229824, s12298a4, s1236698, s12377d4, s123c224 :: Word32
s122962c = 0x0122962c
s122968c = 0x0122968c
s12296dc = 0x012296dc
s1229800 = 0x01229800
s1229824 = 0x01229824
s12298a4 = 0x012298a4
s1236698 = 0x01236698
s12377d4 = 0x012377d4
s123c224 = 0x0123c224

s123c268, s1242fa4, s1243178, s1243198, s12431b8, s1243868, s1243e48, s1243fa0, s12441d0, s12441f0 :: Word32
s123c268 = 0x0123c268
s1242fa4 = 0x01242fa4
s1243178 = 0x01243178
s1243198 = 0x01243198
s12431b8 = 0x012431b8
s1243868 = 0x01243868
s1243e48 = 0x01243e48
s1243fa0 = 0x01243fa0
s12441d0 = 0x012441d0
s12441f0 = 0x012441f0

-- Fragments shared by several actions --------------------------------------------------------

phase :: Action Word32
phase = load (field 0) Word [1, 2]

-- | Phase 1 returns 8 and phase 2 returns 4, both in state 0.
gate :: Action ()
gate = do
  p <- phase
  when (p == 1) (state 0 >> finish 8)
  when (p == 2) (state 0 >> finish 4)

-- | State 4, 0x012259a4(step), 0x01225a98 and, when cleared, 0x01243868; returns 0.
restart :: Word32 -> Bool -> Action a
restart step cleared = do
  state 4
  voidWith startStep step
  void s1225a98
  when cleared (void s1243868)
  finish 0

-- | State 0, 0x012259a4(8) and, when cleared, 0x01243868; returns 12.
reset :: Bool -> Action a
reset cleared = do
  state 0
  voidWith startStep 8
  when cleared (void s1243868)
  finish 12

-- | Ends with `result` in `s` when the status is zero, otherwise restarts.
zeroOr :: Word32 -> Word32 -> Action a -> Action a
zeroOr status s otherwise' = if status == 0 then state s >> finish 0 else otherwise'

machine46Restart :: Action a
machine46Restart = do
  void s1226518
  r <- call dispatch46 [0]
  zeroOr r 5 (restart 10 True)

bindTransfer :: Action a
bindTransfer = do
  v <- call s1243e48 [1]
  store (field 0x6d4) Word v
  if v == 1 then store (field 0x6be) Byte 0 >> void s1243fa0 else void s12294cc
  r <- call dispatch3 [0]
  zeroOr r 6 (reset True)

transferTail :: Action a
transferTail = do
  w6d0 <- load (field 0x6d0) Word [1]
  if w6d0 == 1
    then do
      w6f4 <- load (field 0x6f4) Word [1]
      if w6f4 == 1
        then do
          void s1229824
          r <- call dispatch3 [0]
          zeroOr r 14 (reset False)
        else do
          void s1226014
          r <- call s120ce60 [12]
          state (if r == 12 then 0 else 16)
          finish r
    else do
      e <- load (field 0x6c0) Word [73]
      if e == 73
        then state 12 >> call s1212a48 [] >>= finish
        else state 15 >> void s1225f64 >> finish 0

event7273 :: Word32 -> Word32 -> Action a
event7273 ready other = do
  e <- load (field 0x6c0) Word [72, 73]
  if e == 72 || e == 73
    then do
      void s1226230
      r <- call dispatch3 [0]
      zeroOr r ready (restart 8 False)
    else state other >> void s1225f64 >> finish 0

ec28Status :: Action a
ec28Status = do
  r <- call s120ec28 [0, 4]
  when (r == 0) (state 19 >> finish 0)
  p <- load (field 0) Word [1]
  when (p == 1) (state 0 >> finish 8)
  when (r == 4) (state 0 >> finish r)
  restart 8 True

-- | The machine-3 status read before the gate; 4 continues into `ready`.
machine3Gated :: Action Word32 -> Action Word32
machine3Gated ready = do
  r <- call dispatch3 [4, 12]
  gate
  case r of
    4 -> ready
    12 -> restart 8 True
    _ -> finish 0

latch :: Action ()
latch = do
  v <- load (field 0x9c) Word []
  store (field 0x6bd) Byte (v `mod` 256)

direction :: Action a
direction = do
  w <- load (field 4) Word [0]
  finish (if w == 0 then 10 else 11)

release :: Action ()
release = state 0 >> voidWith s12296dc 0 >> store (field 0x6f4) Word 0

statusOr :: Word32 -> Word32 -> Action a
statusOr status result = if status /= 0 then state 0 >> finish result else finish 0

-- Actions --------------------------------------------------------------------------------------

actions :: [Action Word32]
actions =
  [ finish 8
  , voidWith setPhase 1 >> void s123c224 >> call dispatch46 [0] >>= (`statusOr` 8)
  , voidWith setPhase 1 >> void s12205e4 >> call dispatch3 [0] >>= (`statusOr` 8)
  , voidWith setPhase 1 >> finish 0
  , voidWith setPhase 1 >> void s1212a48 >> finish 0
  , voidWith setPhase 1 >> void s12119a4 >> finish 0
  , voidWith setPhase 1 >> ec28Status
  , voidWith setPhase 2 >> void s123c224 >> call dispatch46 [0] >>= (`statusOr` 4)
  , voidWith setPhase 2 >> void s12205e4 >> call dispatch3 [0] >>= (`statusOr` 4)
  , voidWith setPhase 2 >> finish 0
  , voidWith setPhase 2 >> void s12119a4 >> finish 0
  , action13
  , void trace >> finish 0
  , action15
  , state 17 >> void s12265cc >> finish 0
  , action17
  , void s12377d4 >> gate >> state 15 >> void s1225f64 >> finish 0
  , action19
  , action20
  , gate >> state 9 >> void s1229068 >> finish 0
  , transferTail
  , machine3Gated $ do
      b <- load (field 0x6be) Byte [0, 2]
      if b `mod` 4 >= 2
        then void s1225d08 >> call dispatch3 [0] >>= \r -> zeroOr r 11 (restart 8 True)
        else transferTail
  , machine3Gated transferTail
  , void s1212a48 >> gate >> event7273 13 15
  , do
      state 0
      void s1226684
      p <- load (field 0) Word [1]
      finish (if p == 1 then 8 else 4)
  , do
      r <- call s12119a4 [0]
      gate
      if r /= 0 then state 0 >> void s122669c >> finish 4 else finish 0
  , ec28Status
  , action29
  , do
      p <- phase
      when (p == 1) (state 0 >> finish 8)
      release
      when (p == 2) (finish 4)
      direction
  , do
      r <- call s120ce60 [0]
      when (r == 0) (finish 0)
      state 0
      void s122968c
      void s121c9f8
      voidWith s12296dc 0
      void s1229800
      p <- load (field 0) Word [1]
      when (p == 1) (finish 8)
      direction
  , machine3Gated (state 3 >> load (field 0x6c0) Word [] >>= voidWith s12262fc >> finish 0)
  , machine3Gated (state 7 >> void s1225b20 >> finish 0)
  , machine3Gated (state 15 >> load (field 0x6c0) Word [] >>= voidWith s12262fc >> finish 0)
  , do
      r <- call dispatch3 [4, 12]
      gate
      case r of
        4 -> do
          void s12298a4
          void s1226014
          s <- call s120ce60 [12]
          state (if s == 12 then 0 else 16)
          finish s
        12 -> reset False
        _ -> finish 0
  ]

action13 :: Action Word32
action13 = do
  void s12292a4
  e <- load (field 0x6c0) Word [72, 19, 243, 244]
  when (e == 72) (state 12 >> call s1212a48 [] >>= finish)
  void s122586c
  void s1225fdc
  void s12258bc
  voidWith s12441d0 0
  w6d0 <- if e == 19 then pure 0 else load (field 0x6d0) Word [0]
  if e == 19 || w6d0 /= 0 || e == 243 || e == 244
    then machine46Restart
    else do
      f <- call s1225fb0 [0]
      when (f /= 0) (state 18 >> void s1225928 >> call s12119a4 [] >>= finish)
      void s12263ec
      r <- call dispatch46 [0]
      zeroOr r 1 (restart 10 True)

action15 :: Action Word32
action15 = do
  void s122925c
  void s1225fdc
  r <- call s12431b8 [1]
  if r == 1
    then void s1236698 >> bindTransfer
    else void s122586c >> voidWith s12441d0 1 >> machine46Restart

action17 :: Action Word32
action17 = do
  r <- call dispatch46 [0, 4, 12]
  when (r == 0) (finish 0)
  latch
  gate
  let tailPath = void s12264fc >> machine46Restart
  if r == 4
    then do
      step <- load (field 0x14) Word [1]
      when (step == 1) (void s123c268 >> tailPath)
      voidWith s12296dc 1
      event7273 2 3
    else do
      pending <- call s123c268 [2]
      when (r == 12 && pending /= 2) (restart 10 True)
      tailPath

action19 :: Action Word32
action19 = do
  r <- call dispatch46 [0, 4]
  when (r == 0) (finish 0)
  gate
  when (r /= 4) (restart 10 True)
  latch
  voidWith s1243198 1
  q <- call s12441f0 [1]
  when (q == 1) bindTransfer
  c <- call s122962c [0]
  if c /= 0
    then do
      e <- load (field 0x6c0) Word []
      store 0x014ea6e4 Half 0x119
      store 0x014ea710 Word e
      s <- call s120ec28 [0]
      zeroOr s 19 (restart 8 True)
    else do
      void s12377d4
      void s12294cc
      s <- call dispatch3 [0]
      zeroOr s 6 (restart 8 True)

action20 :: Action Word32
action20 = do
  gate
  voidWith s1243178 (field 0x14)
  voidWith s1242fa4 (field 0x14)
  voidWith s12293a8 (field 0x14)
  b <- load (field 0x6bd) Byte [0]
  when (b /= 0) $ do
    void s1225bfc
    r <- call dispatch3 [0]
    zeroOr r 10 (restart 8 True)
  a4 <- load (field 0xa4) Word [4]
  when (a4 == 4) (state 8 >> void s1229038 >> finish 0)
  transferTail

action29 :: Action Word32
action29 = do
  gate
  e <- load (field 0x6c0) Word [16]
  m <- load 0x014ea694 Word []
  if e == 16
    then state 0 >> packet 0x820 m >> finish 4
    else packet 0x820 m >> void s12264fc >> machine46Restart
