-- | A path model of the stock machine-61 action bytes in [0x011fe4d0,0x011ff0dc), table
-- indices 2..27 of 0x01215c2c.
module Replacement.PlaybackActor.Machine61 (machine61) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine61 :: Machine
machine61 =
  Machine
    { machineName = "machine61"
    , machineIndex = 61
    , machineTransitions = 0x01217b74
    , machineStates = 8
    , machineEvents = 17
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        , Region "selection" selection 0x138 True
        ]
    , machineActions = actions
    }

request, selection :: Word32
request = 0x014ea6e4
selection = 0x014ea9bc

-- | A byte inside the request region that no action names, for the name pointer to point at.
namePointee :: Word32
namePointee = request + 0x20

state :: Word32 -> Action ()
state = store 0x014ea626 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

req, sel :: Word32 -> [Word32] -> Action Word32
req offset values = load (request + offset) Word values
sel offset values = load (selection + offset) Word values

setSel :: Word32 -> Word32 -> Action ()
setSel offset = store (selection + offset) Word

minus :: Word32 -> Word32
minus n = negate n

s120c1a8, trace, s121eda0, s121ef68, s121f0fc, s121f118, s121f1c4, s121f1e8, s121f20c, s121f224 :: Word32
s120c1a8 = 0x0120c1a8
trace = 0x0121eb68
s121eda0 = 0x0121eda0
s121ef68 = 0x0121ef68
s121f0fc = 0x0121f0fc
s121f118 = 0x0121f118
s121f1c4 = 0x0121f1c4
s121f1e8 = 0x0121f1e8
s121f20c = 0x0121f20c
s121f224 = 0x0121f224

s121f254, s121f2a4, s121f390, s121f3ec, s121f500, s121f518, s121f7ac, s121f83c, s121f85c, s12202d0 :: Word32
s121f254 = 0x0121f254
s121f2a4 = 0x0121f2a4
s121f390 = 0x0121f390
s121f3ec = 0x0121f3ec
s121f500 = 0x0121f500
s121f518 = 0x0121f518
s121f7ac = 0x0121f7ac
s121f83c = 0x0121f83c
s121f85c = 0x0121f85c
s12202d0 = 0x012202d0

s1229f84, s123265c, s1243198, s1243590, s1243644, s1243760, s12437bc, s1243868, s1243890, s12438f0 :: Word32
s1229f84 = 0x01229f84
s123265c = 0x0123265c
s1243198 = 0x01243198
s1243590 = 0x01243590
s1243644 = 0x01243644
s1243760 = 0x01243760
s12437bc = 0x012437bc
s1243868 = 0x01243868
s1243890 = 0x01243890
s12438f0 = 0x012438f0

s1243948, s1243a6c, s1245c7c :: Word32
s1243948 = 0x01243948
s1243a6c = 0x01243a6c
s1245c7c = 0x01245c7c

directions :: [Word32]
directions = [minus 3, 3]

latch :: Word32 -> Action Bool
latch v
  | v `elem` directions = setSel 0 v >> pure True
  | otherwise = pure False

stepped :: Word32 -> Action () -> Action Word32
stepped st tailCalls = do
  v <- req 0x100 [1, minus 1]
  kind <-
    if v == 1
      then call s1243890 [0] >>= \r -> if r == 0 then pure (Right 1) else pure (Left 4)
      else
        if v == minus 1
          then call s12438f0 [0] >>= \r -> if r == 0 then pure (Right 2) else pure (Left 5)
          else pure (Right 0)
  case kind of
    Right k -> do
      state st
      void s1243868
      voidWith s1243948 k
      voidWith s1243198 0
      tailCalls
      finish 0
    Left code -> state 0 >> voidWith s1243948 code >> finish 4

flags :: Bool -> Action ()
flags withCancel = do
  done <- sel 0x130 [0]
  when (done /= 0) (state 0 >> setSel 0x130 0 >> finish 8)
  when withCancel $ do
    cancelled <- sel 0x134 [0]
    when (cancelled /= 0) (state 0 >> setSel 0x134 0 >> finish 4)

cancel :: Action a
cancel = state 0 >> void s121f224 >> void s121f20c >> finish 12

confirm :: Action Word32
confirm = do
  t <- req 0x1f8 [15, 2]
  case t of
    15 -> void s121f254 >> finish 0
    2 -> do
      v <- sel 0x7c []
      setSel 0x12c v
      void s121f83c
      r <- call s121f0fc [0]
      if r == 0 then state 0 >> finish 4 else finish 0
    _ -> state 0 >> finish 4

refuse :: Word32 -> Action a
refuse code = state 0 >> voidWith s1243948 code >> finish 4

reportA6c :: Action a
reportA6c = do
  state 0
  code <- call s1243a6c []
  voidWith s1243948 code
  finish 4

finishSelection :: Action Word32
finishSelection = do
  q <- call s12437bc [0]
  when (q == 0) (refuse 0)
  r <- call s120c1a8 [0, 12]
  case r of
    0 -> state 7 >> finish 0
    12 -> refuse 0
    _ -> reportA6c

actions :: [Action Word32]
actions =
  [ do
      state 3
      v <- req 0x100 directions
      latched <- latch v
      when (not latched) (voidWith s1245c7c v)
      kind <- req 0x10c [48]
      when (kind == 48) $ do
        p <- loadOnly (request + 0x178) Word [0, namePointee]
        b <- if p == 0 then pure 0 else load p Byte [0]
        when (b == 0) (void s121f85c >> finish 0)
      void s121eda0
      finish 0
  , do
      state 3
      v <- req 0x100 [0, 0x7fffffff]
      voidWith s1245c7c v
      when (v /= 0x7fffffff && v /= 0) (callWith s1229f84 v [] >>= setSel 0)
      void s121eda0
      finish 0
  , state 3 >> req 0x100 [] >>= voidWith s1245c7c >> void s121eda0 >> finish 0
  , void trace >> finish 0
  , do
      r <- call s121f500 [0]
      if r /= 0
        then state 4 >> req 0x100 [] >>= voidWith s1245c7c >> void s121f518 >> finish 0
        else setSel 0x128 0 >> finish 4
  , state 4 >> req 0x100 [] >>= voidWith s123265c >> void s121f7ac >> finish 0
  , state 3 >> void s121ef68 >> finish 0
  , stepped 5 (void s1243590)
  , state 5 >> voidWith s1243948 0 >> voidWith s1243198 0 >> void s1243590 >> void s1243868 >> finish 0
  , stepped 6 (void s1243644)
  , stepped 6 (void s1243760 >> void s121f7ac)
  , do
      state 3
      v <- req 0x100 directions
      _ <- latch v
      void s121f3ec
      finish 0
  , do
      r <- call s121f118 [1, 3, 4]
      state (case r of 3 -> 2; 1 -> 1; _ -> 0)
      finish 4
  , do
      r <- call s121f118 [3]
      state (if r == 3 then 2 else 0)
      finish 4
  , state 0 >> finish 4
  , state 2 >> finish 4
  , state 0 >> finish 8
  , void s121f1c4 >> finish 0
  , do
      void s121f1c4
      void s12202d0
      r <- call s120c1a8 [0]
      if r /= 0 then state 0 >> setSel 0x130 0 >> finish 8 else finish 0
  , void s121f1e8 >> finish 0
  , do
      flags True
      w <- req 0x1fc [0, 6]
      when (w /= 0) $ do
        kind <- req 0x10c [48]
        when (kind == 48 && w == 6) (void s121f85c >> finish 0)
        cancel
      confirm
  , do
      flags True
      w <- req 0x1fc [0]
      when (w /= 0) cancel
      confirm
  , do
      flags False
      w <- req 0x1fc [0]
      when (w /= 0) cancel
      void s121f0fc
      finishSelection
  , do
      flags False
      w <- req 0x1fc [0]
      when (w /= 0) cancel
      void s121f83c
      void s121f0fc
      finishSelection
  , do
      r <- call s120c1a8 [0, 4]
      when (r == 0) (finish 0)
      if r == 4 then flags False >> reportA6c else refuse 0
  , do
      flags True
      c <- req 0x1c4 [0]
      when (c == 0) $ do
        r <- call s121f2a4 [0]
        when (r /= 0) (void s121f390 >> finish 0)
      state 0
      void s121f20c
      finish 12
  ]
