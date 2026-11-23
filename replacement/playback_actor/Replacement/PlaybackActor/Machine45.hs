-- | A path model of the stock machine-45 action bytes in [0x011fa1b4,0x011fa618), table
-- indices 2..10 of 0x01217b48.
module Replacement.PlaybackActor.Machine45 (machine45) where

import Data.Int (Int32)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine45 :: Machine
machine45 =
  Machine
    { machineName = "machine45"
    , machineIndex = 45
    , machineTransitions = 0x01219948
    , machineStates = 3
    , machineEvents = 6
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "overlay47" overlay 0x80 True
        ]
    , machineActions = actions
    }

overlay :: Word32
overlay = 0x014ff2f4

-- | The pointer at overlay + 0x50 such that its word at 0x434 is overlay + 0x40, which no
-- action names.
pointee :: Word32
pointee = overlay + 0x40 - 0x434

state :: Word32 -> Action ()
state = store 0x014ea606 Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

s120c1a8, trace, s12202d0, s123a7cc, s123a7ec, s123a80c, s123aa04, s123aa84, s123ab00, s123ab84, s123b228 :: Word32
s120c1a8 = 0x0120c1a8
trace = 0x0121eb68
s12202d0 = 0x012202d0
s123a7cc = 0x0123a7cc
s123a7ec = 0x0123a7ec
s123a80c = 0x0123a80c
s123aa04 = 0x0123aa04
s123aa84 = 0x0123aa84
s123ab00 = 0x0123ab00
s123ab84 = 0x0123ab84
s123b228 = 0x0123b228

idle :: Action a
idle = state 0 >> finish 4

fault :: Word32 -> Action a
fault code = state 0 >> voidWith s123a7ec code >> finish 12

settle :: Action a
settle = do
  r <- call s123b228 [0, 1, 2, 3]
  if r - 1 <= 1 then idle else fault 2

begin :: Word32 -> Action a
begin ready
  | ready /= 0 = state 2 >> call s120c1a8 [] >>= finish
  | otherwise = fault 1

resume :: Word32 -> Action Word32 -> Action Word32
resume which rest = do
  r <- call s120c1a8 [0, 4]
  if r == 0
    then finish 0
    else do
      w <- load (overlay + 4) Word [1]
      if w == 1
        then idle
        else if r /= 4 then fault 1 else voidWith s123ab84 which >> rest

actions :: [Action Word32]
actions =
  [ do
      r <- call s123a80c [0, 1, 2]
      case r of
        0 -> fault 1
        1 -> state 1 >> call s120c1a8 [] >>= finish
        _ -> settle
  , void trace >> finish 0
  , settle
  , do
      r <- call s123aa04 [0]
      if r /= 0 then call s123aa84 [0] >>= begin else settle
  , do
      p <- loadOnly (overlay + 0x50) Word [pointee]
      c <- load (p + 0x434) Word [0, 1, 0x80000000]
      if (fromIntegral c :: Int32) > 0
        then call s123ab00 [0] >>= begin
        else do
          r <- call s123b228 [1, 2]
          case r of
            1 -> idle
            2 -> void s123aa04 >> call s123aa84 [0] >>= begin
            _ -> fault 2
  , finish 4
  , do
      voidWith s123a7cc 1
      void s12202d0
      r <- call s120c1a8 [0]
      if r /= 0 then idle else finish 0
  , resume 0 settle
  , resume 1 (call s123b228 [1] >>= \r -> if r == 1 then idle else fault 2)
  ]
