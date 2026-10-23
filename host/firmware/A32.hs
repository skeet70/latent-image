-- | A closed A32 (ARMv7-R, ARM state) decoder for the bank allowlist. It names
-- the instruction forms the bank's clang -Oz output uses and sorts every other
-- word into a forbidden or unadmitted class.
module A32
  ( Reg (..)
  , Cond (..)
  , Shift (..)
  , Operand (..)
  , DataOp (..)
  , Width (..)
  , Offset (..)
  , Indexing (..)
  , BlockMode (..)
  , Forbidden (..)
  , Instruction (..)
  , decode
  , sp
  , lr
  , pc
  , regName
  , forbiddenReason
  , negateCond
  ) where

import Data.Bits (rotateR, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import Data.Int (Int32)
import Data.Word (Word32)

newtype Reg = Reg Int deriving (Eq, Ord, Show)

sp, lr, pc :: Reg
sp = Reg 13
lr = Reg 14
pc = Reg 15

regName :: Reg -> String
regName (Reg 13) = "sp"
regName (Reg 14) = "lr"
regName (Reg 15) = "pc"
regName (Reg n) = 'r' : show n

-- | Condition field values 0..14; 14 is always.
newtype Cond = Cond Word32 deriving (Eq, Ord, Show)

negateCond :: Cond -> Cond
negateCond (Cond c) = Cond (c `xor` 1)

data Shift = LSL | LSR | ASR | ROR deriving (Eq, Show)

data Operand
  = Immediate Word32
  | ShiftedImmediate Reg Shift Int
  | ShiftedRegister Reg Shift Reg
  deriving (Eq, Show)

data DataOp = AND | EOR | SUB | RSB | ADD | ADC | SBC | RSC | TST | TEQ | CMP | CMN | ORR | MOV | BIC | MVN
  deriving (Eq, Show, Enum, Bounded)

data Width = Byte | Half | Word | Dual deriving (Eq, Show)

data Offset = OffsetImmediate Int32 | OffsetRegister Bool Reg Shift Int
  deriving (Eq, Show)

-- | Offset addressing, pre-indexed with writeback, or post-indexed.
data Indexing = OffsetOnly | PreIndexed | PostIndexed deriving (Eq, Show)

data BlockMode = IncrementAfter | IncrementBefore | DecrementAfter | DecrementBefore
  deriving (Eq, Show)

data Forbidden
  = Coprocessor Int
  | SupervisorCall
  | Breakpoint
  | SystemRegister
  | InterruptMask
  | Barrier
  | Exclusive
  | WaitForEvent
  | ExceptionReturn
  | UnprivilegedTransfer
  | Divide
  | ThumbBranch
  | UndefinedEncoding
  | Unadmitted
  deriving (Eq, Show)

forbiddenReason :: Forbidden -> String
forbiddenReason reason = case reason of
  Coprocessor 15 -> "coprocessor 15 transfer (cache, MPU or system control)"
  Coprocessor 14 -> "coprocessor 14 transfer (debug)"
  Coprocessor n | n == 10 || n == 11 -> "VFP/Advanced SIMD instruction (coprocessor " ++ show n ++ ")"
  Coprocessor n -> "coprocessor " ++ show n ++ " instruction"
  SupervisorCall -> "SVC"
  Breakpoint -> "BKPT"
  SystemRegister -> "MRS/MSR status register access"
  InterruptMask -> "CPS interrupt or mode change"
  Barrier -> "DMB/DSB/ISB barrier"
  Exclusive -> "exclusive or swap memory access"
  WaitForEvent -> "WFI/WFE/SEV"
  ExceptionReturn -> "exception return or banked transfer"
  UnprivilegedTransfer -> "unprivileged load/store"
  Divide -> "A32 SDIV/UDIV (absent on the inferred Cortex-R4F profile)"
  ThumbBranch -> "BLX immediate (switches to Thumb)"
  UndefinedEncoding -> "permanently undefined encoding"
  Unadmitted -> "encoding outside the admitted A32 set"

data Instruction
  = DataProcessing Cond DataOp Bool Reg Reg Operand
  | MoveWide Cond Reg Word32
  | MoveTop Cond Reg Word32
  | ZeroExtend Cond Reg Reg Int Int
  | -- | MUL or MLA: destination, the two factors and the accumulator.
    Multiply Cond Reg Reg Reg (Maybe Reg)
  | -- | Destinations of an instruction whose result the analysis does not track.
    Compute Cond [Reg]
  | Load Cond Width Bool Reg Reg Offset Indexing
  | Store Cond Width Reg Reg Offset Indexing
  | LoadMultiple Cond Reg BlockMode Bool [Reg]
  | StoreMultiple Cond Reg BlockMode Bool [Reg]
  | Branch Cond Bool Word32
  | BranchExchange Cond Bool Reg
  | Hint Cond
  | Rejected Forbidden
  deriving (Eq, Show)

field :: Int -> Int -> Word32 -> Word32
field high low word = (word `shiftR` low) .&. ((1 `shiftL` (high - low + 1)) - 1)

reg :: Int -> Word32 -> Reg
reg low word = Reg (fromIntegral (field (low + 3) low word))

shiftType :: Word32 -> Shift
shiftType word = case field 6 5 word of
  0 -> LSL
  1 -> LSR
  2 -> ASR
  _ -> ROR

-- | Decodes the word at `address` (needed for branch targets).
decode :: Word32 -> Word32 -> Instruction
decode address word
  | condition == 15 = unconditional
  | otherwise = case field 27 25 word of
      0 -> group0
      1 -> group1
      2 -> singleTransfer (OffsetImmediate (signed (field 11 0 word)))
      3
        | testBit word 4 -> media
        | otherwise -> singleTransfer (OffsetRegister (testBit word 23) (reg 0 word) (shiftType word) (fromIntegral (field 11 7 word)))
      4
        | testBit word 22 -> Rejected ExceptionReturn
        | otherwise ->
            let mode = case field 24 23 word of
                  0 -> DecrementAfter
                  1 -> IncrementAfter
                  2 -> DecrementBefore
                  _ -> IncrementBefore
                registers = [Reg n | n <- [0 .. 15], testBit word n]
             in if testBit word 20
                  then LoadMultiple cond (reg 16 word) mode (testBit word 21) registers
                  else StoreMultiple cond (reg 16 word) mode (testBit word 21) registers
      5 -> Branch cond (testBit word 24) (address + 8 + branchOffset)
      6 -> Rejected (Coprocessor (fromIntegral (field 11 8 word)))
      _
        | testBit word 24 -> Rejected SupervisorCall
        | otherwise -> Rejected (Coprocessor (fromIntegral (field 11 8 word)))
 where
  condition = field 31 28 word
  cond = Cond condition
  signed value = if testBit word 23 then fromIntegral value else negate (fromIntegral value)
  branchOffset =
    let imm = field 23 0 word
        extended = if testBit imm 23 then imm .|. 0xff000000 else imm
     in extended `shiftL` 2
  unconditional
    | field 27 25 word == 5 = Rejected ThumbBranch
    | word .&. 0xfff1fe20 == 0xf1000000 = Rejected InterruptMask
    | word .&. 0xfffffff0 `elem` [0xf57ff040, 0xf57ff050, 0xf57ff060] = Rejected Barrier
    | word == 0xf57ff01f = Rejected Exclusive
    | word .&. 0xfe5fffff == 0xf84d0500 || word .&. 0xfe50ffff == 0xf8100a00 = Rejected ExceptionReturn
    | word .&. 0xfd70f000 `elem` [0xf550f000, 0xf510f000, 0xf450f000] = Hint (Cond 14)
    | word .&. 0xff70f010 `elem` [0xf750f000, 0xf710f000, 0xf650f000] = Hint (Cond 14)
    | field 27 25 word `elem` [6, 7] = Rejected (Coprocessor (fromIntegral (field 11 8 word)))
    | otherwise = Rejected Unadmitted
  group0
    | field 7 4 word == 9 && field 27 20 word == 0 = Multiply cond (reg 16 word) (reg 0 word) (reg 8 word) Nothing
    | field 7 4 word == 9 && field 27 20 word == 2 = Multiply cond (reg 16 word) (reg 0 word) (reg 8 word) (Just (reg 12 word))
    | field 7 4 word == 9 && field 24 24 word == 0 && testBit word 20 = Rejected Unadmitted
    | field 7 4 word == 9 && field 24 24 word == 0 =
        if testBit word 23 || field 22 21 word == 2
          then Compute cond [reg 16 word, reg 12 word]
          else Compute cond [reg 16 word]
    | field 7 4 word == 9 = Rejected Exclusive
    | testBit word 7 && testBit word 4 = extraTransfer
    | field 24 23 word == 2 && not (testBit word 20) = miscellaneous
    | otherwise = dataProcessing operand
   where
    operand
      | testBit word 4 = ShiftedRegister (reg 0 word) (shiftType word) (reg 8 word)
      | otherwise = ShiftedImmediate (reg 0 word) (shiftType word) (fromIntegral (field 11 7 word))
  miscellaneous
    | word .&. 0x0ffffff0 == 0x012fff10 = BranchExchange cond False (reg 0 word)
    | word .&. 0x0ffffff0 == 0x012fff30 = BranchExchange cond True (reg 0 word)
    | word .&. 0x0fff0ff0 == 0x016f0f10 = Compute cond [reg 12 word]
    | word .&. 0x0ff000f0 == 0x01200070 = Rejected Breakpoint
    | word .&. 0x0ff000f0 == 0x01600070 = Rejected ExceptionReturn
    | testBit word 7 = halfwordMultiply
    | field 7 4 word == 0 = Rejected SystemRegister
    | otherwise = Rejected Unadmitted
  -- SMLA<x><y>, SMLAW<y>, SMULW<y> and SMUL<x><y>; SMLAL<x><y> and any PC
  -- operand stay unadmitted.
  halfwordMultiply
    | pc `elem` [reg 16 word, reg 8 word, reg 0 word] = Rejected Unadmitted
    | otherwise = case field 22 21 word of
        0 | accumulator /= pc -> Compute cond [reg 16 word]
        1 | not (testBit word 5), accumulator /= pc -> Compute cond [reg 16 word]
        1 | testBit word 5, field 15 12 word == 0 -> Compute cond [reg 16 word]
        3 | field 15 12 word == 0 -> Compute cond [reg 16 word]
        _ -> Rejected Unadmitted
   where
    accumulator = reg 12 word
  extraTransfer =
    let load = testBit word 20
        offset
          | testBit word 22 = OffsetImmediate (signed ((field 11 8 word `shiftL` 4) .|. field 3 0 word))
          | otherwise = OffsetRegister (testBit word 23) (reg 0 word) LSL 0
        indexing = transferIndexing
        rt = reg 12 word
        rn = reg 16 word
     in case (field 6 5 word, load) of
          _ | indexing == Nothing -> Rejected UnprivilegedTransfer
          (1, False) -> Store cond Half rt rn offset (unwrap indexing)
          (1, True) -> Load cond Half False rt rn offset (unwrap indexing)
          (2, False) -> Load cond Dual False rt rn offset (unwrap indexing)
          (2, True) -> Load cond Byte True rt rn offset (unwrap indexing)
          (3, False) -> Store cond Dual rt rn offset (unwrap indexing)
          (3, True) -> Load cond Half True rt rn offset (unwrap indexing)
          _ -> Rejected Unadmitted
  transferIndexing = case (testBit word 24, testBit word 21) of
    (True, False) -> Just OffsetOnly
    (True, True) -> Just PreIndexed
    (False, False) -> Just PostIndexed
    (False, True) -> Nothing
  unwrap = maybe OffsetOnly id
  singleTransfer offset =
    case transferIndexing of
      Nothing -> Rejected UnprivilegedTransfer
      Just indexing ->
        let width = if testBit word 22 then Byte else Word
            rt = reg 12 word
            rn = reg 16 word
         in if testBit word 20
              then Load cond width False rt rn offset indexing
              else Store cond width rt rn offset indexing
  group1
    | field 27 20 word == 0x30 = MoveWide cond (reg 12 word) wide
    | field 27 20 word == 0x34 = MoveTop cond (reg 12 word) wide
    | word .&. 0x0fffff00 == 0x0320f000 = case field 7 0 word of
        0 -> Hint cond
        1 -> Hint cond
        _ -> Rejected WaitForEvent
    | field 27 23 word == 6 && field 21 20 word == 2 = Rejected SystemRegister
    | otherwise = dataProcessing (Immediate (field 7 0 word `rotateR` (2 * fromIntegral (field 11 8 word))))
   where
    wide = (field 19 16 word `shiftL` 12) .|. field 11 0 word
  dataProcessing operand =
    let op = toEnum (fromIntegral (field 24 21 word))
        setsFlags = testBit word 20
        tests = op `elem` [TST, TEQ, CMP, CMN]
     in if tests && not setsFlags
          then Rejected Unadmitted
          else
            if setsFlags && not tests && reg 12 word == pc
              then Rejected ExceptionReturn
              else DataProcessing cond op setsFlags (reg 12 word) (reg 16 word) operand
  media
    | word .&. 0x0ff000f0 == 0x07f000f0 = Rejected UndefinedEncoding
    | word .&. 0x0fd0f0f0 == 0x0710f010 = Rejected Divide
    | word .&. 0x0fff03f0 == 0x06ef0070 = ZeroExtend cond (reg 12 word) (reg 0 word) rotation 8
    | word .&. 0x0fff03f0 == 0x06ff0070 = ZeroExtend cond (reg 12 word) (reg 0 word) rotation 16
    | word .&. 0x0fff03f0 `elem` [0x06af0070, 0x06bf0070] = Compute cond [reg 12 word]
    | word .&. 0x0ff003f0 `elem` [0x06e00070, 0x06f00070, 0x06a00070, 0x06b00070] = Compute cond [reg 12 word]
    | word .&. 0x0fff0ff0 `elem` [0x06bf0f30, 0x06bf0fb0, 0x06ff0f30] = Compute cond [reg 12 word]
    | word .&. 0x0fa00070 == 0x07a00050 = Compute cond [reg 12 word]
    | word .&. 0x0fe00070 == 0x07c00010 = Compute cond [reg 12 word]
    | otherwise = Rejected Unadmitted
   where
    rotation = 8 * fromIntegral (field 11 10 word)
