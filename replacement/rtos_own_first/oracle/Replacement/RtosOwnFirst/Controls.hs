-- | The oracle's hand-written control bodies: one must-fail mutant per slot and two equivalent
-- bodies built differently from both the stock and the owned code.
module Replacement.RtosOwnFirst.Controls
  ( Control (..)
  , Expectation (..)
  , mutants
  , equivalents
  , controlCode
  ) where

import Data.Word (Word32)
import Replacement.RtosOwnFirst.Assembly
import Replacement.RtosOwnFirst.Oracle (DataWords (..), Slot (..), dataWords, slotEntry)
import Replacement.RtosOwnFirst.Regions (Domain, DomainTable)

data Expectation = MustPass | MustFail deriving (Eq, Show)

data Control = Control
  { controlName :: String
  , controlSlot :: Slot
  , controlExpectation :: Expectation
  , controlNote :: String
  , -- | Items placed at the slot's entry; the table gives +0x16c's callee entries.
    controlItems :: DataWords -> DomainTable -> Either String [Item]
  }

controlCode :: Domain -> DomainTable -> Control -> Either String [Word32]
controlCode domain table control = do
  entry <- slotEntry table (controlSlot control)
  items <- controlItems control (dataWords domain) table
  assembleAt entry items

fixed :: ([Item]) -> DataWords -> DomainTable -> Either String [Item]
fixed items _ _ = Right items

withWords :: (DataWords -> [Item]) -> DataWords -> DomainTable -> Either String [Item]
withWords build words' _ = Right (build words')

contextError :: Word32
contextError = 24

-- | Returns -25 unless the context word is zero.
taskGuard :: DataWords -> [Item]
taskGuard w = loadWord r3 (wordContext w) ++ [cmpI r3 0, on CondNe (mvnI r0 contextError), on CondNe bxLr]

-- | Returns -25 unless the context word is nonzero.
handlerGuard :: DataWords -> [Item]
handlerGuard w = loadWord r3 (wordContext w) ++ [cmpI r3 0, on CondEq (mvnI r0 contextError), on CondEq bxLr]

-- | Returns -25 when I is set; leaves the entry CPSR in r3.
irqGuard :: [Item]
irqGuard = [mrs r3, tstI r3 0x80, on CondNe (mvnI r0 contextError), on CondNe bxLr]

-- | Sets I from the entry CPSR in r3.
maskIrq :: [Item]
maskIrq = [orrI r12 r3 0x80, msrControl r12]

-- | r1 = (((T - table) >> 3) * 0xb6db6db7) + 1 for T in r2.
taskIndex :: DataWords -> [Item]
taskIndex w = loadAddress r1 (wordTaskTable w) ++ [subR r2 r2 r1, movShift r2 r2 Asr 3] ++ loadAddress r1 0xb6db6db7 ++ [mul r1 r1 r2, addI r1 r1 1]

setCpuLock :: [Item] -> Bool -> [Item]
setCpuLock guard locked = guard ++ [mrs r3, (if locked then orrI else bicI) r12 r3 0x80, msrControl r12, movI r0 0, bxLr]

mutants :: [Control]
mutants =
  [ Control "mutant-s60" S60 MustFail "tests T+0xe bit 3 instead of bit 2" $ withWords $ \w ->
      loadWord r3 (wordCurrentTask w)
        ++ [cmpI r3 0, on CondEq (movI r0 1), on CondEq bxLr, ldrb r0 r3 0xe, andI r0 r0 8, eorI r0 r0 8, movShift r0 r0 Lsr 3, bxLr]
  , Control "mutant-sfc" Sfc MustFail "skips the time-offset write" $ withWords $ \w ->
      taskGuard w ++ irqGuard ++ maskIrq ++ [msrControl r3, movI r0 0, bxLr]
  , Control "mutant-s100" S100 MustFail "flips I handling: returns with I still set" $ withWords $ \w ->
      taskGuard w ++ irqGuard ++ maskIrq ++ loadWord r1 (wordTimerBase w) ++ loadWord r2 (wordTimeOffset w) ++ [addR r1 r1 r2, str r1 r0 0, movI r0 0, bxLr]
  , Control "mutant-s128" S128 MustFail "divides (T - table) by 56 instead of the multiply" $ withWords $ \w ->
      taskGuard w ++ irqGuard ++ maskIrq ++ loadWord r2 (wordCurrentTask w) ++ loadAddress r1 (wordTaskTable w)
        ++ [ subR r2 r2 r1
           , movI r1 0
           , mark (Label "loop")
           , cmpI r2 56
           , on CondLt (branch (Label "done"))
           , subI r2 r2 56
           , addI r1 r1 1
           , branch (Label "loop")
           , mark (Label "done")
           , addI r1 r1 1
           , str r1 r0 0
           , msrControl r3
           , movI r0 0
           , bxLr
           ]
  , Control "mutant-s12c" S12c MustFail "drops the null-task check" $ withWords $ \w ->
      handlerGuard w ++ irqGuard ++ maskIrq ++ loadWord r2 (wordCurrentTask w) ++ taskIndex w ++ [str r1 r0 0, msrControl r3, movI r0 0, bxLr]
  , Control "mutant-s130" S130 MustFail "flips I handling: clears I" $ withWords $ \w -> setCpuLock (taskGuard w) False
  , Control "mutant-s134" S134 MustFail "flips I handling: clears I" $ withWords $ \w -> setCpuLock (handlerGuard w) False
  , Control "mutant-s138" S138 MustFail "flips I handling: sets I" $ withWords $ \w -> setCpuLock (taskGuard w) True
  , Control "mutant-s13c" S13c MustFail "flips I handling: sets I" $ withWords $ \w -> setCpuLock (handlerGuard w) True
  , Control "mutant-s148" S148 MustFail "returns (context word == 0)" $ withWords $ \w ->
      loadWord r0 (wordContext w) ++ [cmpI r0 0, on CondEq (movI r0 1), on CondNe (movI r0 0), bxLr]
  , Control "mutant-s14c" S14c MustFail "flips I handling: returns I clear" $ fixed
      [mrs r0, andI r0 r0 0x80, eorI r0 r0 0x80, movShift r0 r0 Lsr 7, bxLr]
  , Control "mutant-s150" S150 MustFail "returns (dispatch-enabled != 0)" $ withWords $ \w ->
      loadWord r0 (wordDispatch w) ++ [cmpI r0 0, on CondNe (movI r0 1), bxLr]
  , Control "mutant-s154" S154 MustFail "flips I handling: ignores I" $ withWords $ \w ->
      loadWord r3 (wordContext w) ++ [cmpI r3 0, on CondNe (movI r0 1), on CondNe bxLr]
        ++ loadWord r0 (wordDispatch w) ++ [cmpI r0 0, on CondEq (movI r0 1), on CondNe (movI r0 0), bxLr]
  , Control "mutant-s164" S164 MustFail "flips I handling: returns the inverted prior I bit" $ fixed
      [mrs r0, orrI r1 r0 0x80, msrControl r1, andI r0 r0 0x80, eorI r0 r0 0x80, bxLr]
  , Control "mutant-s168" S168 MustFail "drops the flags from r0" $ fixed
      [andI r0 r0 0x80, mrs r1, bicI r1 r1 0x80, bicI r1 r1 0xf8000000, orrR r0 r0 r1, msrControl r0, bxLr]
  , Control "mutant-s16c" S16c MustFail "returns +0x128's status instead of the id" $ \_ table -> do
      flag <- slotEntry table S148
      index <- slotEntry table S128
      Right
        [ push [r11, lr]
        , subI sp sp 8
        , branchLink flag
        , cmpI r0 1
        , on CondEq (movI r0 0)
        , on CondEq (branch (Label "out"))
        , movR r0 sp
        , branchLink index
        , mark (Label "out")
        , addI sp sp 8
        , pop [r11, pc]
        ]
  , Control "mutant-s1618" S1618 MustFail "returns 1 for the identity" $ fixed [movI r0 1, bxLr]
  ]

equivalents :: [Control]
equivalents =
  [ Control "equivalent-s60" S60 MustPass "frameless, tst and two conditional moves" $ withWords $ \w ->
      loadWord r1 (wordCurrentTask w)
        ++ [cmpI r1 0, on CondEq (movI r0 1), on CondEq bxLr, ldrb r0 r1 0xe, tstI r0 4, on CondEq (movI r0 1), on CondNe (movI r0 0), bxLr]
  , Control "equivalent-s14c" S14c MustPass "frameless shift then mask" $ fixed
      [mrs r0, movShift r0 r0 Lsr 7, andI r0 r0 1, bxLr]
  ]
