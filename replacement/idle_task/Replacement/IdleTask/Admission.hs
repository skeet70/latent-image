module Replacement.IdleTask.Admission
  ( AdmissionError (..), IdleProof, deriveIdleProof
  , scratchLow, executionPcs, wfiPc, resumePc
  ) where

import qualified A32 as A
import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError
  = InvalidCarrier | UnsupportedInstruction | ExecutionOutOfRange
  | WaitCountRejected | ResumeRejected | CycleBeforeWait | LoopStateDrift
  | RegisterCorruption | StackProvenanceRejected | StackEscape
  | UnreachableCode
  deriving (Eq, Show)

newtype StackOffset = StackOffset Int deriving (Eq, Show)
data Value = Incoming A.Reg | Stack StackOffset deriving (Eq, Show)
data Machine = Machine
  { registers :: M.Map A.Reg Value
  , stack :: M.Map Int Value
  , bottom :: Int
  } deriving (Eq, Show)
data Operation = Wait | Jump Word32 | Ordinary A.Instruction deriving (Eq, Show)
data IdleProof = IdleProof Int [Word32] Word32 Word32 deriving (Eq, Show)

scratchLow :: IdleProof -> Int
scratchLow (IdleProof low _ _ _) = low
executionPcs :: IdleProof -> [Word32]
executionPcs (IdleProof _ pcs _ _) = pcs
wfiPc :: IdleProof -> Word32
wfiPc (IdleProof _ _ pc _) = pc
resumePc :: IdleProof -> Word32
resumePc (IdleProof _ _ _ pc) = pc

origin :: Word32
origin = 0x69cbf0

require :: Bool -> AdmissionError -> Either AdmissionError ()
require condition refusal = unless condition (Left refusal)

initial :: Machine
initial = Machine (M.fromList [(A.Reg n,Incoming (A.Reg n)) | n <- [0..14], n /= 13]
  <> M.singleton A.sp (Stack (StackOffset 0))) M.empty 0

get :: Machine -> A.Reg -> Either AdmissionError Value
get machine register = maybe (Left RegisterCorruption) Right (M.lookup register (registers machine))

spOffset :: Machine -> Either AdmissionError Int
spOffset machine = do
  value <- get machine A.sp
  case value of
    Stack (StackOffset offset) -> Right offset
    _ -> Left StackProvenanceRejected

setSp :: Int -> Machine -> Either AdmissionError Machine
setSp offset machine = do
  require (offset <= 0 && offset >= -992 && offset `mod` 4 == 0) StackEscape
  pure machine {registers = M.insert A.sp (Stack (StackOffset offset)) (registers machine)
    , bottom = min (bottom machine) offset}

-- Only this fixed-site proof interprets WFI. The native instruction profile remains unchanged.
decode :: Word32 -> Word32 -> Either AdmissionError Operation
decode pc word
  | word == 0xe320f003 = Right Wait
  | otherwise = case A.decode pc word of
      A.Branch (A.Cond 14) False target -> Right (Jump target)
      instruction@(A.DataProcessing (A.Cond 14) operation False _ _ _)
        | operation `elem` [A.MOV,A.ADD,A.SUB] -> Right (Ordinary instruction)
      instruction@(A.StoreMultiple (A.Cond 14) base A.DecrementBefore True saved)
        | base == A.sp && not (null saved) && all (`elem` [A.Reg 11,A.lr]) saved -> Right (Ordinary instruction)
      instruction@(A.Store (A.Cond 14) A.Word source base (A.OffsetImmediate (-4)) A.PreIndexed)
        | base == A.sp && source `elem` [A.Reg 11,A.lr] -> Right (Ordinary instruction)
      _ -> Left UnsupportedInstruction

execute :: A.Instruction -> Machine -> Either AdmissionError Machine
execute instruction machine = case instruction of
  A.StoreMultiple _ _ _ _ saved -> save saved
  A.Store _ _ source _ _ _ -> save [source]
  A.DataProcessing _ operation False destination source operand -> do
    value <- case (operation,operand) of
      (A.MOV,A.ShiftedImmediate register A.LSL 0) -> get machine register
      (A.ADD,A.Immediate amount) -> adjust source (toInteger amount)
      (A.SUB,A.Immediate amount) -> adjust source (negate (toInteger amount))
      _ -> Left UnsupportedInstruction
    old <- get machine destination
    if old == value then pure machine else case (destination,value) of
      (register,Stack (StackOffset offset)) | register == A.sp -> do
        previous <- spOffset machine
        require (offset < previous) StackEscape
        setSp offset machine
      (A.Reg 11,Stack (StackOffset offset)) -> do
        current <- spOffset machine
        require (offset >= current && offset <= 0
          && Incoming (A.Reg 11) `elem` M.elems (stack machine)) StackProvenanceRejected
        pure machine {registers = M.insert destination value (registers machine)}
      _ -> Left RegisterCorruption
  _ -> Left UnsupportedInstruction
  where
    adjust source displacement = do
      value <- get machine source
      case value of
        Stack (StackOffset offset) -> do
          let result = toInteger offset+displacement
          require (result >= -992 && result <= 0) StackEscape
          pure (Stack (StackOffset (fromInteger result)))
        _ -> Left StackProvenanceRejected
    save saved = do
      old <- spOffset machine
      values <- mapM (get machine) saved
      let low = old-4*length saved
          slots = zip [low,low+4..] values
      allocated <- setSp low machine
      pure allocated {stack = M.union (M.fromList slots) (stack machine)}

-- The original four semantic steps have one private frame and an invariant wait/backedge cycle.
originalWait :: Either AdmissionError Machine
originalWait = fst <$> traceWaitCycle (M.fromList
  [ (origin,Ordinary (A.StoreMultiple (A.Cond 14) A.sp A.DecrementBefore True [A.Reg 11]))
  , (origin+4,Ordinary (A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 11) (A.Reg 0)
      (A.ShiftedImmediate A.sp A.LSL 0)))
  , (origin+8,Wait), (origin+12,Jump (origin+8))
  ]) (origin+8)

observable :: Machine -> Either AdmissionError [Value]
observable machine = do
  values <- mapM (get machine) [A.Reg n | n <- [0..14], n /= 11, n /= 13]
  current <- spOffset machine
  fp <- get machine (A.Reg 11)
  require (fp == Incoming (A.Reg 11) || case fp of
    Stack (StackOffset offset) -> offset >= current && offset <= 0
      && Incoming (A.Reg 11) `elem` M.elems (stack machine)
    _ -> False) RegisterCorruption
  pure values
traceWaitCycle :: M.Map Word32 Operation -> Word32 -> Either AdmissionError (Machine,S.Set Word32)
traceWaitCycle code wait = walk origin initial M.empty
  where
    walk pc machine seen = case M.lookup pc seen of
      Just previous -> do
        require (pc == wait) CycleBeforeWait
        require (machine == previous) LoopStateDrift
        pure (machine,M.keysSet seen)
      Nothing -> do
        operation <- maybe (Left ExecutionOutOfRange) Right (M.lookup pc code)
        let visited = M.insert pc machine seen
        case operation of
          Wait -> walk (pc+4) machine visited
          Jump target -> walk target machine visited
          Ordinary instruction -> execute instruction machine >>= \changed -> walk (pc+4) changed visited


-- Finite symbolic entry and two waits establish the local fixed point, not runtime acceptance.
deriveIdleProof :: B.ByteString -> Either AdmissionError IdleProof
deriveIdleProof bytes = do
  require (B.length bytes >= 8 && B.length bytes <= 16 && B.length bytes `mod` 4 == 0) InvalidCarrier
  let pcs = [origin,origin+4..origin+fromIntegral (B.length bytes)-4]
  code <- M.fromList <$> mapM (\pc -> (,) pc <$> decode pc (U.word32At bytes (fromIntegral (pc-origin)))) pcs
  wait <- case [pc | (pc,Wait) <- M.toList code] of
    [pc] -> Right pc
    _ -> Left WaitCountRejected
  require (M.lookup (wait+4) code == Just (Jump wait)) ResumeRejected
  expected <- originalWait >>= observable
  (state,executed) <- traceWaitCycle code wait
  actual <- observable state
  require (actual == expected) RegisterCorruption
  require (executed == M.keysSet code) UnreachableCode
  pure (IdleProof (bottom state) (S.toAscList executed) wait (wait+4))
