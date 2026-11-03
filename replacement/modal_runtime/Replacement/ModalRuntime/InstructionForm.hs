{-# LANGUAGE GHC2021 #-}

-- | The A32 instruction-form definition shared by the controller form census
-- and the controller inventories.
module Replacement.ModalRuntime.InstructionForm
  ( FormName (..)
  , DecoderStatus (..)
  , Occurrence (..)
  , classify
  , statusName
  , formDefinition
  ) where

import qualified A32 as A
import Data.Bits (shiftL, shiftR, testBit, (.&.))
import Data.Word (Word32)
import Numeric (showHex)

-- Register numbers, immediates, offsets and condition codes are not part of a form.
newtype FormName = FormName String deriving (Eq, Ord, Show)
data DecoderStatus = Semantic | DestinationsOnly | StockInventoryOnly | RejectedForm deriving (Eq, Ord, Show)
data Occurrence = Occurrence FormName DecoderStatus Bool

formDefinition :: String
formDefinition = "operation, flag-setting, operand/offset shape (immediate, register, shift kind with immediate or register amount, RRX), width and signedness, indexing and writeback, block mode, PC/SP roles of Rd, Rn and the load/store base, PC as load target, PC in a load-multiple and LR in a store-multiple; register numbers (including Rm and register offsets), immediates, offset signs and condition codes are parameters. Conditional occurrences are counted per form. Forms are ranked by instruction count, ties by name."

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

field :: Int -> Int -> Word32 -> Word32
field high low word = (word `shiftR` low) .&. ((1 `shiftL` (high - low + 1)) - 1)

shiftName :: A.Shift -> String
shiftName shift = case shift of
  A.LSL -> "lsl"
  A.LSR -> "lsr"
  A.ASR -> "asr"
  A.ROR -> "ror"

operandShape :: A.Operand -> String
operandShape operand = case operand of
  A.Immediate _ -> "imm"
  A.ShiftedImmediate _ A.LSL 0 -> "reg"
  A.ShiftedImmediate _ A.ROR 0 -> "reg,rrx"
  A.ShiftedImmediate _ shift _ -> "reg," ++ shiftName shift ++ "#imm"
  A.ShiftedRegister _ shift _ -> "reg," ++ shiftName shift ++ " reg"

offsetShape :: A.Offset -> String
offsetShape offset = case offset of
  A.OffsetImmediate _ -> "imm"
  A.OffsetRegister _ _ A.LSL 0 -> "reg"
  A.OffsetRegister _ _ A.ROR 0 -> "reg,rrx"
  A.OffsetRegister _ _ shift _ -> "reg," ++ shiftName shift ++ "#imm"

indexingName :: A.Indexing -> String
indexingName indexing = case indexing of
  A.OffsetOnly -> "offset"
  A.PreIndexed -> "pre!"
  A.PostIndexed -> "post"

widthName :: A.Width -> Bool -> String
widthName width signed = case (width, signed) of
  (A.Word, _) -> ""
  (A.Dual, _) -> "d"
  (A.Half, False) -> "h"
  (A.Half, True) -> "sh"
  (A.Byte, False) -> "b"
  (A.Byte, True) -> "sb"

blockName :: A.BlockMode -> String
blockName mode = case mode of
  A.IncrementAfter -> "ia"
  A.IncrementBefore -> "ib"
  A.DecrementAfter -> "da"
  A.DecrementBefore -> "db"

-- Names for the encodings `A.decode` returns as Compute, from the same masks.
computeName :: Word32 -> String
computeName word
  | field 27 24 word == 0 && field 7 4 word == 9 = case (testBit word 23, field 22 21 word) of
      (True, accumulate) -> (if testBit word 22 then "s" else "u") ++ (if testBit accumulate 0 then "mlal" else "mull") ++ flags
      (False, 2) -> "umaal"
      (False, 3) -> "mls"
      _ -> "multiply-other"
  | word .&. 0x0fff0ff0 == 0x016f0f10 = "clz"
  | word .&. 0x0ff00090 == 0x01000080 = "smla<x><y>"
  | word .&. 0x0ff000b0 == 0x01200080 = "smlaw<y>"
  | word .&. 0x0ff000b0 == 0x012000a0 = "smulw<y>"
  | word .&. 0x0ff00090 == 0x01600080 = "smul<x><y>"
  | word .&. 0x0fff03f0 == 0x06af0070 = "sxtb"
  | word .&. 0x0fff03f0 == 0x06bf0070 = "sxth"
  | word .&. 0x0ff003f0 == 0x06e00070 = "uxtab"
  | word .&. 0x0ff003f0 == 0x06f00070 = "uxtah"
  | word .&. 0x0ff003f0 == 0x06a00070 = "sxtab"
  | word .&. 0x0ff003f0 == 0x06b00070 = "sxtah"
  | word .&. 0x0fff0ff0 == 0x06bf0f30 = "rev"
  | word .&. 0x0fff0ff0 == 0x06bf0fb0 = "rev16"
  | word .&. 0x0fff0ff0 == 0x06ff0f30 = "rbit"
  | word .&. 0x0fe00070 == 0x07a00050 = "sbfx"
  | word .&. 0x0fe00070 == 0x07e00050 = "ubfx"
  | word .&. 0x0fe0007f == 0x07c0001f = "bfc"
  | word .&. 0x0fe00070 == 0x07c00010 = "bfi"
  | otherwise = "compute-" ++ hex (word .&. 0x0ff000f0)
  where
    flags = if testBit word 20 then ".s" else ""

pcNote :: [(String, Bool)] -> String
pcNote notes = concat [" " ++ label | (label, present) <- notes, present]

classify :: Word32 -> Word32 -> Occurrence
classify address word = case A.decode address word of
  A.Rejected reason
    | Just (A.StatusWrite _ target fields _) <- A.statusWrite word ->
        Occurrence (FormName ("msr " ++ (if target == A.CurrentStatus then "cpsr" else "spsr") ++ "_" ++ concatMap fieldLetter fields)) StockInventoryOnly conditional
    | otherwise -> Occurrence (FormName ("rejected " ++ A.forbiddenReason reason)) RejectedForm conditional
  A.Compute _ _
    | Just _ <- A.signExtension word -> Occurrence (FormName (computeName word)) StockInventoryOnly conditional
    | Just _ <- A.bitFieldInsert word -> Occurrence (FormName (computeName word)) StockInventoryOnly conditional
    | otherwise -> Occurrence (FormName (computeName word)) DestinationsOnly conditional
  instruction -> Occurrence (FormName (semanticName instruction)) Semantic conditional
  where
    conditional = field 31 28 word /= 14
    fieldLetter statusField = case statusField of
      A.ControlBits -> "c"
      A.ExtensionBits -> "x"
      A.StatusBits -> "s"
      A.FlagBits -> "f"
    semanticName instruction = case instruction of
      A.DataProcessing _ operation setsFlags destination source operand ->
        let tests = operation `elem` [A.TST, A.TEQ, A.CMP, A.CMN]
            moves = operation `elem` [A.MOV, A.MVN]
         in map toLowerAscii (show operation) ++ (if setsFlags && not tests then ".s" else "") ++ " " ++ operandShape operand
              ++ pcNote [("rd=pc", not tests && destination == A.pc), ("rn=pc", not moves && source == A.pc), ("rn=sp", not moves && source == A.sp), ("rd=sp", not tests && destination == A.sp)]
      A.MoveWide {} -> "movw"
      A.MoveTop {} -> "movt"
      A.ReadStatus {} -> "mrs"
      A.WriteApplicationFlags {} -> "msr apsr_nzcvq"
      A.ZeroExtend _ _ _ rotation bits -> (if bits == 8 then "uxtb" else "uxth") ++ (if rotation /= 0 then " ror" else "")
      A.Multiply _ _ _ _ accumulator -> maybe "mul" (const "mla") accumulator
      A.Load _ width signed destination base offset indexing ->
        "ldr" ++ widthName width signed ++ " " ++ offsetShape offset ++ " " ++ indexingName indexing ++ pcNote [("base=pc", base == A.pc), ("base=sp", base == A.sp), ("rt=pc", destination == A.pc)]
      A.Store _ width source base offset indexing ->
        "str" ++ widthName width False ++ " " ++ offsetShape offset ++ " " ++ indexingName indexing ++ pcNote [("base=sp", base == A.sp), ("rt=pc", source == A.pc)]
      A.LoadMultiple _ base mode writeback registers ->
        "ldm" ++ blockName mode ++ (if writeback then "!" else "") ++ pcNote [("base=sp", base == A.sp), ("+pc", A.pc `elem` registers)]
      A.StoreMultiple _ base mode writeback registers ->
        "stm" ++ blockName mode ++ (if writeback then "!" else "") ++ pcNote [("base=sp", base == A.sp), ("+lr", A.lr `elem` registers)]
      A.Branch _ link _ -> if link then "bl" else "b"
      A.BranchExchange _ link register -> (if link then "blx" else "bx") ++ (if register == A.lr then " lr" else " reg")
      A.Hint _ -> "hint"
      A.Compute {} -> "compute"
      A.Rejected _ -> "rejected"

toLowerAscii :: Char -> Char
toLowerAscii c = if c >= 'A' && c <= 'Z' then toEnum (fromEnum c + 32) else c

statusName :: DecoderStatus -> String
statusName status = case status of
  Semantic -> "a32-operand-semantics"
  DestinationsOnly -> "a32-destinations-only"
  StockInventoryOnly -> "stock-inventory-operand-step"
  RejectedForm -> "rejected"
