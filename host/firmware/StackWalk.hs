-- | The stack effect of one A32 instruction, shared by the ELF bound
-- (EngineStack) and the stock-image bound (StockStack).
module StackWalk
  ( Strictness (..)
  , Effect (..)
  , Refused (..)
  , refusedReason
  , classify
  , written
  , writes
  , framePointer
  ) where

import qualified A32
import Data.Bits (shiftR, testBit, (.&.))
import Data.Int (Int32)
import Data.Word (Word32)

-- | `Permissive` is EngineStack's handling: VFP refuses, `UndefinedEncoding` ends the path,
-- every other rejected word and every conditional SP adjustment continues as written.
-- `Strict` continues only through the stack-neutral classes, refuses every other rejected
-- word, and refuses SP adjustments that only some paths make.
data Strictness = Permissive | Strict deriving (Eq, Show)

data Refused
  = VariableReservation
  | UnaccountedSpWrite
  | ComputedBranch
  | LoadedBranch
  | FloatingPoint
  | SupervisorCall
  | ThumbCall
  | ExceptionReturn
  | UnadmittedEncoding
  | BelowStackWrite
  deriving (Eq, Ord, Show)

refusedReason :: Refused -> String
refusedReason reason = case reason of
  VariableReservation -> "variable stack reservation"
  UnaccountedSpWrite -> "stack pointer write is neither an immediate adjustment nor a restore from r11"
  ComputedBranch -> "computed branch"
  LoadedBranch -> "loaded branch"
  FloatingPoint -> "floating-point instruction is not accounted"
  SupervisorCall -> "supervisor call or breakpoint"
  ThumbCall -> "BLX immediate"
  ExceptionReturn -> "exception return"
  UnadmittedEncoding -> "unadmitted encoding"
  BelowStackWrite -> "store below the stack pointer"

data Effect
  = -- | SP decreases by the bytes.
    Reserve Word32
  | -- | SP increases by the bytes.
    Release Word32
  | -- | SP becomes r11 plus the offset.
    FromFrame Int32
  | -- | r11 becomes SP plus the bytes.
    SetFrame Word32
  | Neutral
  | Call Word32
  | Jump A32.Cond Word32
  | -- | Returns when the condition holds, releasing the bytes on that path.
    Return A32.Cond Word32
  | RegisterCall A32.Reg
  | RegisterJump A32.Cond A32.Reg
  | -- | @ldr<cond> pc, [pc, rN, lsl #2]@ and its index register.
    JumpTable A32.Cond A32.Reg
  | -- | An MSR to CPSR's control field or a CPS with a mode: the new mode's source.
    ModeWrite (Either A32.Reg Word32)
  | Refuse Refused
  | -- | `Permissive` only: the path ends.
    Stop
  deriving (Eq, Show)

framePointer :: A32.Reg
framePointer = A32.Reg 11

-- | Registers an instruction writes, as far as the bound tracks them.
written :: A32.Instruction -> [A32.Reg]
written instruction = case instruction of
  A32.DataProcessing _ _ _ rd _ _ -> [rd]
  A32.MoveWide _ rd _ -> [rd]
  A32.MoveTop _ rd _ -> [rd]
  A32.ReadStatus _ rd -> [rd]
  A32.ZeroExtend _ rd _ _ _ -> [rd]
  A32.Multiply _ rd _ _ _ -> [rd]
  A32.Compute _ registers -> registers
  A32.Load _ _ _ rt _ _ _ -> [rt]
  A32.LoadMultiple _ _ _ _ registers -> registers
  A32.Branch _ True _ -> [A32.Reg n | n <- [0 .. 3] ++ [12, 14]]
  A32.BranchExchange _ True _ -> [A32.Reg n | n <- [0 .. 3] ++ [12, 14]]
  _ -> []

-- | Registers the word writes under `Strict`: `written`, the second register of @ldrd@, and the
-- destination of the stack-neutral rejected classes (@mrc@, @mrs Rt, spsr@, @ldrex@/@strex@, @swp@, @vmrs@).
writes :: Word32 -> Word32 -> [A32.Reg]
writes address word = case A32.decode address word of
  instruction@(A32.Load _ A32.Dual _ (A32.Reg n) _ _ _) -> written instruction ++ [A32.Reg (n + 1)]
  A32.Rejected (A32.Coprocessor coprocessor)
    | coprocessor `elem` [14, 15] && testBit word 20 && word .&. 0x0f000010 == 0x0e000010 -> destination
    | coprocessor `elem` [10, 11] && word .&. 0x0ff00fff == 0x0ef00a10 -> destination
  A32.Rejected A32.Exclusive -> destination
  A32.Rejected A32.SystemRegister | word .&. 0x0fbf0fff == 0x010f0000 -> destination
  A32.Rejected _ | word .&. 0x0fb00ff0 == 0x01000090 -> destination
  instruction -> written instruction
  where
    rt = fromIntegral ((word `shiftR` 12) .&. 0xf)
    destination = [A32.Reg rt | rt /= 15]

always :: A32.Cond -> Bool
always (A32.Cond c) = c == 14

-- | The effect of the word at the address.
classify :: Strictness -> Word32 -> Word32 -> Effect
classify strictness address word = case A32.decode address word of
  A32.StoreMultiple cond base A32.DecrementBefore True regs | base == A32.sp && admitted cond -> Reserve (4 * fromIntegral (length regs))
  A32.StoreMultiple _ base mode False _ | strictness == Strict && base == A32.sp && mode `elem` [A32.DecrementBefore, A32.DecrementAfter] ->
    Refuse BelowStackWrite
  A32.Store _ _ _ base offset A32.OffsetOnly | strictness == Strict && base == A32.sp && below offset -> Refuse BelowStackWrite
  A32.Store cond _ _ base (A32.OffsetImmediate offset) A32.PreIndexed | base == A32.sp && offset < 0 && admitted cond ->
    Reserve (fromIntegral (negate offset :: Int32))
  A32.DataProcessing cond A32.SUB _ rd rn (A32.Immediate amount) | rd == A32.sp && rn == A32.sp && admitted cond -> Reserve amount
  A32.DataProcessing _ A32.SUB _ rd rn _ | rd == A32.sp && rn == A32.sp -> Refuse VariableReservation
  A32.DataProcessing cond A32.ADD _ rd rn (A32.Immediate amount) | rd == A32.sp && rn == A32.sp && admitted cond -> Release amount
  A32.DataProcessing cond op _ rd rn (A32.Immediate amount)
    | rd == A32.sp && rn == framePointer && op `elem` [A32.ADD, A32.SUB] && admitted cond ->
        FromFrame (if op == A32.ADD then fromIntegral amount else negate (fromIntegral amount))
  A32.DataProcessing cond A32.MOV _ rd _ (A32.ShiftedImmediate source A32.LSL 0)
    | rd == A32.sp && source == framePointer && admitted cond -> FromFrame 0
  A32.DataProcessing _ _ _ rd _ _ | rd == A32.sp -> Refuse UnaccountedSpWrite
  A32.DataProcessing cond A32.MOV _ rd _ (A32.ShiftedImmediate source A32.LSL 0) | rd == A32.pc && source == A32.lr -> Return cond 0
  A32.DataProcessing _ _ _ rd _ _ | rd == A32.pc -> Refuse ComputedBranch
  A32.DataProcessing cond A32.ADD False rd rn (A32.Immediate amount)
    | strictness == Strict && rd == framePointer && rn == A32.sp && always cond -> SetFrame amount
  A32.DataProcessing cond A32.MOV False rd _ (A32.ShiftedImmediate source A32.LSL 0)
    | strictness == Strict && rd == framePointer && source == A32.sp && always cond -> SetFrame 0
  A32.Branch _ True target -> Call target
  A32.Branch cond False target -> Jump cond target
  A32.BranchExchange _ True register -> RegisterCall register
  A32.BranchExchange cond False register
    | register == A32.lr -> Return cond 0
    | otherwise -> RegisterJump cond register
  A32.LoadMultiple _ base mode True _ | base == A32.sp && mode /= A32.IncrementAfter -> Refuse UnaccountedSpWrite
  A32.LoadMultiple _ _ _ _ regs | A32.sp `elem` regs -> Refuse UnaccountedSpWrite
  A32.LoadMultiple cond base _ writeback regs
    | A32.pc `elem` regs -> if strictness == Strict && base /= A32.sp then Refuse LoadedBranch
        else Return cond (if base == A32.sp && writeback then 4 * fromIntegral (length regs) else 0)
  A32.Load cond _ _ rt base offset A32.PostIndexed | rt == A32.pc && base == A32.sp -> case offset of
    A32.OffsetImmediate bytes -> Return cond (fromIntegral bytes)
    _ | strictness == Strict -> Refuse UnaccountedSpWrite
      | otherwise -> Return cond 0
  A32.Load _ _ _ _ base _ A32.PreIndexed | base == A32.sp -> Refuse UnaccountedSpWrite
  A32.Store _ _ _ base _ indexing | base == A32.sp && indexing /= A32.OffsetOnly -> Refuse UnaccountedSpWrite
  A32.StoreMultiple _ base _ True _ | base == A32.sp -> Refuse UnaccountedSpWrite
  A32.Load cond A32.Word False rt base (A32.OffsetRegister True index A32.LSL 2) A32.OffsetOnly
    | strictness == Strict && rt == A32.pc && base == A32.pc -> JumpTable cond index
  A32.Load _ _ _ rt _ _ _ | rt == A32.pc -> Refuse LoadedBranch
  A32.Rejected (A32.Coprocessor coprocessor) | coprocessor `elem` [10, 11] ->
    if strictness == Strict && vfpStatusTransfer then Neutral else Refuse FloatingPoint
  A32.Rejected A32.UndefinedEncoding -> if strictness == Strict then Refuse UnadmittedEncoding else Stop
  A32.Rejected reason | strictness == Strict -> strictRejected reason
  A32.LoadMultiple cond base A32.IncrementAfter True regs
    | strictness == Strict && base == A32.sp -> if always cond then Release (4 * fromIntegral (length regs)) else Refuse UnaccountedSpWrite
  A32.Load cond _ _ _ base offset A32.PostIndexed
    | strictness == Strict && base == A32.sp -> case offset of
        A32.OffsetImmediate bytes | always cond && bytes >= 0 -> Release (fromIntegral bytes)
        _ -> Refuse UnaccountedSpWrite
  instruction | A32.sp `elem` written instruction -> Refuse UnaccountedSpWrite
  _ | strictness == Strict && A32.sp `elem` writes address word -> Refuse UnaccountedSpWrite
  _ -> Neutral
 where
  admitted cond = strictness == Permissive || always cond
  below offset = case offset of
    A32.OffsetImmediate bytes -> bytes < 0
    A32.OffsetRegister add _ _ _ -> not add
  field high low = (word `shiftR` low) .&. ((2 :: Word32) ^ (high - low + 1) - 1)
  -- VMSR/VMRS: cond 1110 111L reg Rt 1010 0001 0000.
  vfpStatusTransfer = word .&. 0x0fe00fff == 0x0ee00a10 && field 15 12 /= 13
  -- MCR/MRC to coprocessor 14 or 15 with a register other than SP: a register transfer with no memory access.
  systemTransfer = word .&. 0x0f000010 == 0x0e000010 && field 11 8 `elem` [14, 15] && field 15 12 /= 13
  exclusiveTarget = field 15 12 `notElem` [13, 15]
  strictRejected reason = case reason of
    A32.Coprocessor coprocessor | coprocessor `elem` [14, 15] && systemTransfer -> Neutral
    A32.Barrier -> Neutral
    A32.WaitForEvent -> Neutral
    A32.Exclusive | exclusiveTarget || word == 0xf57ff01f -> Neutral
    A32.SystemRegister -> case A32.statusWrite word of
      Just (A32.StatusWrite _ A32.CurrentStatus fields source)
        | A32.ControlBits `elem` fields -> ModeWrite (case source of
            A32.StatusFromRegister register -> Left register
            A32.StatusFromImmediate value -> Right value)
        | otherwise -> Neutral
      Just (A32.StatusWrite _ A32.SavedStatus _ _) -> Neutral
      Nothing
        -- MRS from SPSR into a register other than SP or PC.
        | word .&. 0x0fff0fff == 0x014f0000 && field 15 12 `notElem` [13, 15] -> Neutral
        | otherwise -> Refuse UnadmittedEncoding
    A32.InterruptMask
      | testBit word 17 -> ModeWrite (Right (word .&. 0x1f))
      | otherwise -> Neutral
    A32.SupervisorCall -> Refuse SupervisorCall
    A32.Breakpoint -> Refuse SupervisorCall
    A32.ThumbBranch -> Refuse ThumbCall
    A32.ExceptionReturn -> Refuse ExceptionReturn
    _ -> Refuse UnadmittedEncoding
