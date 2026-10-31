-- | Which stock instructions an experiment hook may displace into its trampoline.
module ExperimentHook (positionIndependent) where

import A32 (Instruction (..), Offset (..), Operand (..), Reg, pc)

-- | Whether the instruction behaves the same at another address: it neither
-- branches nor names the PC.
positionIndependent :: Instruction -> Bool
positionIndependent instruction = case instruction of
  DataProcessing _ _ _ d n operand -> notPc [d, n] && operandIndependent operand
  MoveWide _ d _ -> d /= pc
  MoveTop _ d _ -> d /= pc
  ZeroExtend _ d m _ _ -> notPc [d, m]
  Multiply _ d a b accumulate -> notPc ([d, a, b] ++ maybe [] pure accumulate)
  Load _ _ _ d n offset _ -> notPc [d, n] && offsetIndependent offset
  Store _ _ t n offset _ -> notPc [t, n] && offsetIndependent offset
  LoadMultiple _ n _ _ registers -> notPc (n : registers)
  StoreMultiple _ n _ _ registers -> notPc (n : registers)
  Hint _ -> True
  _ -> False
 where
  notPc :: [Reg] -> Bool
  notPc = notElem pc
  operandIndependent operand = case operand of
    Immediate _ -> True
    ShiftedImmediate m _ _ -> m /= pc
    ShiftedRegister m _ s -> notPc [m, s]
  offsetIndependent offset = case offset of
    OffsetImmediate _ -> True
    OffsetRegister _ m _ _ -> m /= pc
