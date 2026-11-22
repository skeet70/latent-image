-- | A path model of the stock machine-7 action bytes in [0x011eb824,0x011ec980), table indices
-- 2..50 of 0x0121a490.
module Replacement.PlaybackActor.Machine7 (machine7) where

import Control.Monad (when)
import Data.Word (Word32)
import Replacement.PlaybackActor.Model

machine7 :: Machine
machine7 =
  Machine
    { machineName = "machine7"
    , machineIndex = 7
    , machineTransitions = 0x0121955c
    , machineStates = 14
    , machineEvents = 23
    , machineRegions =
        [ Region "machines" 0x014ea4ec 0x1c0 False
        , Region "request" 0x014ea6e4 0x30 True
        , Region "step" 0x014eb5d0 8 True
        ]
    , machineActions = actions
    }

state :: Word32 -> Action ()
state = store 0x014ea5ba Half

void :: Word32 -> Action ()
void address = () <$ call address []

voidWith :: Word32 -> Word32 -> Action ()
voidWith address argument = () <$ callWith address argument []

s120d618, s120e6e8, s120e964, s120ec28, s120f328, s120f420, s120fc08, s121164c, s1211c8c :: Word32
s120d618 = 0x0120d618
s120e6e8 = 0x0120e6e8
s120e964 = 0x0120e964
s120ec28 = 0x0120ec28
s120f328 = 0x0120f328
s120f420 = 0x0120f420
s120fc08 = 0x0120fc08
s121164c = 0x0121164c
s1211c8c = 0x01211c8c

s12128a0, s1212a48, s12138ac, s121406c, trace, s12290cc, s12290fc, s12390d4, s1239134, s1239178 :: Word32
s12128a0 = 0x012128a0
s1212a48 = 0x01212a48
s12138ac = 0x012138ac
s121406c = 0x0121406c
trace = 0x0121eb68
s12290cc = 0x012290cc
s12290fc = 0x012290fc
s12390d4 = 0x012390d4
s1239134 = 0x01239134
s1239178 = 0x01239178

status8 :: Word32 -> Maybe Word32 -> Action Word32
status8 address other = do
  r <- call address [8]
  if r == 8 then state 0 else maybe (pure ()) state other
  finish r

settle :: Word32 -> Word32 -> Action Word32
settle address kind = do
  r <- call address [0, 8, 12]
  when (r == 0) (finish 0)
  pending <- call s1239178 [0]
  when (pending /= 0) (state 10 >> voidWith s12290cc r >> voidWith s12290fc kind >> finish 0)
  void s12390d4
  when (r == 8) (state 0 >> finish 8)
  finish r

gatedDone :: Action Word32
gatedDone = do
  r <- call s121164c [8]
  when (r /= 8) (finish r)
  pending <- call s1239178 [0]
  when (pending == 0) (state 0 >> void s12390d4 >> finish 8)
  state 10
  voidWith s12290cc 8
  voidWith s12290fc 1
  finish 0

data Route = A | B | C | D | Refuse

route :: Route -> Action Word32
route r = case r of
  A -> via s120e964 2 2
  B -> via s120f328 4 3
  C -> via s120d618 7 6
  D -> via s120ec28 5 5
  Refuse -> void trace >> finish 255
  where
    via address kind other = do
      s <- call address [8]
      voidWith s12290fc kind
      when (s /= 8) (state other >> finish s)
      pending <- call s1239178 [0]
      when (pending == 0) (state 0 >> void s12390d4 >> finish 8)
      state 10
      voidWith s12290cc 8
      finish 0

-- | Request events the routing actions compare, and their neighbours.
events :: [Word32]
events =
  [0 .. 13] ++ [15, 16, 17, 18, 19, 20, 21, 28, 29, 30, 32, 33, 34, 35, 36, 37, 39, 40, 41, 71, 72, 73, 74, 76, 77, 78]
    ++ [139, 140, 141, 142, 143, 144, 145, 242, 243, 244, 245]

routed :: (Word32 -> Route) -> Action Word32
routed classify = do
  void s121164c
  e <- load 0x014ea710 Word events
  route (classify e)

route32 :: Word32 -> Route
route32 e
  | e `elem` [0, 16, 19, 20, 72, 73, 243, 244] = A
  | e `elem` [1, 2, 4, 6, 7, 8, 9, 12, 77] = B
  | e `elem` [5, 29] = C
  | e `elem` [33, 34, 35, 36, 40, 140, 143, 144] = D
  | otherwise = Refuse

route33 :: Word32 -> Route
route33 e
  | e == 143 = Refuse
  | otherwise = route32 e

route35 :: Word32 -> Route
route35 e
  | e `elem` [0, 19] = A
  | e == 5 = C
  | otherwise = Refuse

twelve :: Word32 -> Word32 -> Action Word32
twelve address other = do
  r <- call address [8, 12]
  state (if r == 8 then 0 else if r == 12 then 2 else other)
  finish r

actions :: [Action Word32]
actions =
  [ finish 8
  , status8 s121164c Nothing
  , void s121164c >> status8 s120e964 Nothing
  , void s121164c >> status8 s120f328 Nothing
  , void s121164c >> status8 s120fc08 Nothing
  , do
      void s121164c
      r <- call s120ec28 [0, 8, 12]
      when (r == 0) (finish 0)
      pending <- call s1239178 [0]
      when (pending /= 0) (state 10 >> voidWith s12290cc r >> voidWith s12290fc 5 >> finish 0)
      void s12390d4
      when (r == 12 || r == 8) (state 0)
      finish r
  , void s121164c >> status8 s120d618 Nothing
  , status8 s12138ac (Just 7)
  , status8 s1212a48 (Just 8)
  , status8 s12128a0 (Just 9)
  , void trace >> finish 0
  , void s121164c >> status8 s1211c8c (Just 12)
  , void s1239134 >> void s120e6e8 >> void s12390d4 >> finish 8
  , void s120f328 >> gatedDone
  , void s121164c >> settle s120e964 2
  , void s121164c >> settle s120f328 4
  , void s121164c >> settle s120fc08 4
  , void s121164c >> settle s120d618 7
  , voidWith s12290cc 8 >> finish 0
  , status8 s120f420 (Just 11)
  , status8 s1211c8c (Just 12)
  , status8 s121406c (Just 13)
  , settle s120e964 2
  , settle s120d618 7
  , voidWith s12290cc 4 >> finish 0
  , do
      r <- call s121164c [12]
      state (if r == 12 then 0 else 1)
      finish r
  , gatedDone
  , do
      r <- call s121164c [0, 8]
      when (r == 0) (state 1 >> finish 0)
      pending <- call s1239178 [0]
      when (pending /= 0) (state 10 >> voidWith s12290cc r >> voidWith s12290fc 1 >> finish 0)
      void s12390d4
      if r == 8 then state 0 >> finish 8 else state 1 >> finish r
  , call s120ec28 [] >>= finish
  , gatedDone
  , routed route32
  , routed route33
  , void s121164c >> status8 s120ec28 (Just 5)
  , routed route35
  , void s121164c >> twelve s120ec28 5
  , status8 s120ec28 (Just 5)
  , twelve s120fc08 4
  , status8 s120fc08 (Just 4)
  , void s121164c >> status8 s120f328 (Just 3)
  , do
      r <- call s120e964 [8]
      voidWith s12290fc 2
      state (if r == 8 then 0 else 2)
      finish r
  , status8 s120f420 Nothing
  , void s120e6e8 >> finish 0
  , do
      void s120e6e8
      void s12390d4
      w <- load 0x014eb5d0 Word [8]
      when (w == 8) (state 0 >> store 0x014eb5d4 Half 0 >> finish 8)
      kind <- load 0x014eb5d4 Half [0, 1, 2, 3, 4, 5, 6, 7, 8]
      state (case kind of 1 -> 1; 2 -> 2; 4 -> 3; 7 -> 6; _ -> 0)
      finish w
  , state 2 >> finish 0
  , do
      r <- call s121164c [8]
      when (r /= 8) (finish r)
      pending <- call s1239178 [0]
      when (pending == 0) (state 0 >> void s12390d4 >> finish 8)
      state 10
      voidWith s12290fc 1
      voidWith s12290cc 8
      finish 0
  , settle s120e964 2
  , settle s120f328 4
  , status8 s120fc08 (Just 4)
  , settle s120ec28 5
  ]
