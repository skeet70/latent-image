-- | A path model of the stock machine-70 action bytes in [0x01200f58,0x01201814), table
-- indices 2..22 of 0x01216a10.
module Replacement.PlaybackActor.Machine70 (machine70) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine70 :: Machine
machine70 =
  Machine
    { machineName = "machine70"
    , machineIndex = 70
    , machineTransitions = 0x01216894
    , machineStates = 10
    , machineEvents = 11
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" request 0x200 True
        , Region "sequence" sequenceBlock 0x28 True
        ]
    , machineActions = actions
    }

request, sequenceBlock :: Word32
request = 0x014ea6e4
sequenceBlock = 0x01513154

state :: Word32 -> Action ()
state = store 0x014ea638 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

req, word :: Word32 -> [Word32] -> Action Word32
req offset values = load (request + offset) Word values
word offset values = load (sequenceBlock + offset) Word values

setWord :: Word32 -> Word32 -> Action ()
setWord offset = store (sequenceBlock + offset) Word

setByte :: Word32 -> Word32 -> Action ()
setByte offset = store (sequenceBlock + offset) Byte

s0ec03dc, trace, s122c468, s1233dcc, s1233e0c, s123265c, s1251c80, s1251cd0, s1251de8, s1251e10 :: Word32
s0ec03dc = 0x00ec03dc
trace = 0x0121eb68
s122c468 = 0x0122c468
s1233dcc = 0x01233dcc
s1233e0c = 0x01233e0c
s123265c = 0x0123265c
s1251c80 = 0x01251c80
s1251cd0 = 0x01251cd0
s1251de8 = 0x01251de8
s1251e10 = 0x01251e10

s1251e40, s1251e80, s1251ea8, s1251f24, s1251fc8, s125202c, s1252064, s12520b0, s12520ec, s1252328 :: Word32
s1251e40 = 0x01251e40
s1251e80 = 0x01251e80
s1251ea8 = 0x01251ea8
s1251f24 = 0x01251f24
s1251fc8 = 0x01251fc8
s125202c = 0x0125202c
s1252064 = 0x01252064
s12520b0 = 0x012520b0
s12520ec = 0x012520ec
s1252328 = 0x01252328

s125233c, s1252374, s1252430, s1252464, s1252490, s12524a0 :: Word32
s125233c = 0x0125233c
s1252374 = 0x01252374
s1252430 = 0x01252430
s1252464 = 0x01252464
s1252490 = 0x01252490
s12524a0 = 0x012524a0

modes :: [Word32]
modes = [0, 2, 3]

stop :: Action ()
stop = state 0 >> void s12520ec >> void s1252328

cancel :: Action a
cancel = voidWith s12520b0 13 >> finish 12

requestValue :: Action a
requestValue = state 4 >> voidWith s1251e10 1 >> finish 0

requestValueE0c :: Action a
requestValueE0c = state 4 >> void s1233e0c >> voidWith s1251e10 1 >> finish 0

finished :: Action ()
finished = do
  active <- word 0 [0]
  when (active /= 0) (word 8 [] >>= finish)

activeRequestsValue :: Action () -> Action ()
activeRequestsValue requested = do
  active <- word 0 [0]
  when (active /= 0) requested

slot :: Action Bool -> Action Word32
slot chosen = do
  r <- call s0ec03dc [0]
  when (r /= 0) (state 1 >> void s1251e80 >> finish 0)
  state 9
  c <- chosen
  if c then void s125233c else voidWith s1252374 1
  finish 0

increment :: Word32 -> Action ()
increment offset = word offset [] >>= setWord offset . (+ 1)

actions :: [Action Word32]
actions =
  [ finish 8
  , state 0 >> finish 8
  , voidWith s1252064 1 >> finish 0
  , void s1252490 >> voidWith s1252064 1 >> finish 0
  , state 1 >> void s1251e40 >> finish 0
  , void trace >> finish 0
  , state 1 >> void s1251ea8 >> finish 0
  , do
      r <- call s125202c [0, 1]
      case r of
        0 -> state 7 >> voidWith s1251f24 0 >> finish 0
        1 -> state 2 >> voidWith s1251c80 1 >> voidWith s1251cd0 0x7fffffff >> finish 0
        _ -> do
          state 5
          voidWith s1251c80 2
          void s1233dcc
          voidWith s123265c 0
          void s1251de8
          finish 0
  , state 1 >> void s1251fc8 >> finish 0
  , state 1 >> void s1251e80 >> finish 0
  , slot ((== 46) <$> req 0x34 [46])
  , slot ((/= 0) <$> call s122c468 [0])
  , do
      activeRequestsValue requestValue
      state 3
      voidWith s1251f24 1
      finish 0
  , do
      activeRequestsValue requestValueE0c
      state 6
      voidWith s1251f24 2
      finish 0
  , do
      stop
      void s12524a0
      finished
      m <- req 0x1e8 [0]
      when (m /= 0) cancel
      finish 4
  , do
      activeRequestsValue requestValue
      m <- req 0x1e8 modes
      case m of
        0 -> do
          n <- word 0x14 [3]
          i <- word 0x10 [0, 2]
          when (i >= n - 1) requestValue
          state 2
          voidWith s1251cd0 1
          finish 0
        2 -> state 4 >> setByte 0x26 1 >> voidWith s1251e10 1 >> finish 0
        3 -> do
          n <- word 0x14 [3]
          i <- word 0x10 [0, 2]
          when (i >= n - 1) $ do
            l <- call s1252430 [0]
            when (l /= 0) (stop >> finish 6)
            requestValue
          state 2
          increment 0x1c
          voidWith s1251cd0 1
          finish 0
        _ -> stop >> cancel
  , do
      stop
      finished
      m <- req 0x1e8 [0]
      when (m /= 0) cancel
      r <- call s1252464 [0]
      finish (if r == 0 then 4 else 5)
  , do
      activeRequestsValue requestValueE0c
      m <- req 0x1e8 modes
      case m of
        0 -> do
          p <- word 0x18 [0, 2]
          n <- word 0x14 [3]
          when (p >= n - 1) requestValueE0c
          state 5
          setWord 0x18 (p + 1)
          voidWith s123265c 1
          void s1251de8
          finish 0
        2 -> do
          state 4
          void s1233e0c
          setByte 0x26 1
          voidWith s1251e10 1
          finish 0
        3 -> do
          p <- word 0x18 [0, 2]
          n <- word 0x14 [3]
          when (p >= n - 1) $ do
            l <- call s1252430 [0]
            when (l /= 0) (stop >> void s1233e0c >> finish 6)
            requestValueE0c
          state 5
          increment 0x1c
          setWord 0x18 (p + 1)
          voidWith s123265c 1
          void s1251de8
          finish 0
        _ -> stop >> cancel
  , do
      stop
      finished
      m <- req 0x1e8 modes
      case m of
        0 -> finish 4
        2 -> finish 5
        3 -> finish 6
        _ -> cancel
  , do
      stop
      b <- load (sequenceBlock + 0x27) Byte [0]
      finish (if b == 0 then 4 else 12)
  , do
      stop
      m <- req 0x1e8 [0]
      when (m /= 0) cancel
      a <- word 0 [1]
      finish (if a == 1 then 18 else 4)
  ]
