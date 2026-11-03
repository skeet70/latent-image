module Fixtures (Layout (..), Damage (..), fixture, symbolsFor) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.RootTask.Services (serviceSymbols)

data Layout = Compact | SavedFrame | TailCleanup | LiteralArguments | OrMarker deriving (Eq, Show)
data Damage = Intact | SkipResource | DuplicateResource | WrongConfig | CleanupSuccess
  | PcMoveWide | PcMoveTop | PcArithmetic | PcOperand | PcStore | PcMultiStore
  | DriftHalt | DriftResource | HiddenCode | ExternalStore | BadReturn | UnbalancedReturn
  | WrongOrder | BranchOnIgnored | LoopNeverEnds | LoadPcLiteral | Unsupported | OrUnknown | OrFlags | OrPc | OrStack | OrRegister
  deriving (Eq, Show)
data Label = Resources | ZeroArm | ZeroTail | Stop | Literal deriving (Eq, Ord, Show)
data Condition = Always | Equal | NotEqual
newtype Register = Register Word32
data Instruction = Raw Word32 | Call Word32 | Tail Word32 | Jump Condition Label
  | Mark Label | LoadLiteral Register Label

-- Authored A32 examples exercise the public admission boundary, not captured instruction bytes.
fixture :: Layout -> Damage -> B.ByteString
fixture layout damage = assemble (prefix ++ resources ++ common ++ arms ++ halt ++ pool ++ hidden)
  where
    counter = if layout == SavedFrame then Register 5 else Register 4
    saved = if layout == SavedFrame then 0x4830 else 0x4010
    prefix = [Raw (0xe92d0000 .|. saved)]
      ++ (if layout == SavedFrame then [Raw 0xe1a0b00d] else [])
      ++ (if damage == WrongOrder then calls [0x7284ec,0x649624] else calls [0x649624,0x7284ec])
      ++ (if layout == OrMarker then [Raw 0xe3a00401,Raw 0xe3800801] else constant (Register 0) 0x01010000) ++ calls [0x72872c]
      ++ (if layout == LiteralArguments then [LoadLiteral (Register 0) Literal] else constant (Register 0) 0x72835c)
      ++ calls [0x725b54] ++ [mov counter (if damage == SkipResource then 2 else 1)]
      ++ case damage of
        PcMoveWide -> [Raw 0xe300f000]
        PcMoveTop -> [Raw 0xe340f000]
        PcArithmetic -> [Raw 0xe28ff000]
        PcOperand -> [Raw 0xe1a0200f]
        PcStore -> [Raw 0xe52df004]
        PcMultiStore -> [Raw 0xe92d8000]
        ExternalStore -> [Raw 0xe5801000]
        LoadPcLiteral -> [Raw 0xe59ff000]
        Unsupported -> [Raw 0xffffffff]
        OrUnknown -> [Raw 0xe3800001]
        OrFlags -> [Raw 0xe3900001]
        OrPc -> [Raw 0xe38ff001]
        OrStack -> [Raw 0xe38d0000]
        OrRegister -> [mov (Register 0) 1,mov (Register 1) 2,Raw 0xe1800001]
        _ -> []
    resources = [Mark Resources,copy (Register 0) counter,mov (Register 1) 1]
      ++ calls (if damage == DuplicateResource then [0x7247ac,0x7247ac] else [0x7247ac])
      ++ (if damage == BranchOnIgnored then [Raw 0xe3500000,Jump NotEqual Stop] else [])
      ++ (if damage == DriftResource then [Raw 0xe24dd008] else [])
      ++ [add counter 1,compareWith counter (if damage == LoopNeverEnds then 0 else 98),Jump NotEqual Resources]
    common = calls [0x69ca24,0x676054,0x662e48,0x665224,0x649070]
      ++ [mov (Register 0) 1] ++ calls [0x6490dc,0x6a9354]
      ++ [Raw 0xe3500000,Jump NotEqual Stop] ++ calls [0x6ac2b4]
      ++ [Raw 0xe3500000,Jump NotEqual Stop] ++ calls [0x69f204,0x656788,0x7262cc]
      ++ constant (Register 0) 0x726478 ++ calls [0x724cbc,0x6a5634]
      ++ config [34,0,0] ++ calls [0x69cfa4] ++ [Raw 0xe3500000,Jump Equal ZeroArm]
    arms = calls [0x69ca44] ++ args [30,0,0x69cc18,0x96ca4c] ++ calls [0x725424]
      ++ constant (Register 0) 0x11010000 ++ calls [0x72872c] ++ [mov (Register 0) 0]
      ++ (if layout == TailCleanup then [Raw (0xe8bd0000 .|. saved),Tail 0x72612c]
          else calls [0x72612c] ++ (if damage == CleanupSuccess then [mov (Register 0) 0] else [])
            ++ (if damage == UnbalancedReturn then [Raw 0xe24dd008] else [])
            ++ [Raw (if damage == BadReturn then 0xe12fff1e else 0xe8bd0000 .|. (saved .&. 0x3fff) .|. 0x8000)])
      ++ [Mark ZeroArm,mov (Register 0) 34] ++ calls [0x6a5920]
      ++ [Raw 0xe3500000,Jump Equal ZeroTail]
      ++ concatMap config [[34,0,0],[31,if damage == WrongConfig then 59 else 60,0],[32,0,0],[34,1,0]]
      ++ [Mark ZeroTail] ++ calls [0x6a564c,0x6a5c08,0x6568ec]
    halt = [Mark Stop] ++ (if damage == DriftHalt then [Raw 0xe24dd008] else []) ++ [Jump Always Stop]
    pool = if layout == LiteralArguments then [Mark Literal,Raw 0x72835c] else []
    hidden = if damage == HiddenCode then [Raw 0xe1a00000] else []
    config values = args values ++ calls [0x6a59b8]

calls :: [Word32] -> [Instruction]
calls = map Call
mov :: Register -> Word32 -> Instruction
mov (Register register) value = Raw (0xe3a00000 .|. (register `shiftL` 12) .|. value)
copy :: Register -> Register -> Instruction
copy (Register destination) (Register source) = Raw (0xe1a00000 .|. (destination `shiftL` 12) .|. source)
add :: Register -> Word32 -> Instruction
add (Register register) value = Raw (0xe2800000 .|. (register `shiftL` 16) .|. (register `shiftL` 12) .|. value)
compareWith :: Register -> Word32 -> Instruction
compareWith (Register register) value = Raw (0xe3500000 .|. (register `shiftL` 16) .|. value)
constant :: Register -> Word32 -> [Instruction]
constant register@(Register number) value
  | value <= 255 = [mov register value]
  | otherwise = [Raw (wide 0xe3000000 (value .&. 0xffff)),Raw (wide 0xe3400000 (value `div` 0x10000))]
  where wide opcode half = opcode .|. ((half .&. 0xf000) `shiftL` 4) .|. (number `shiftL` 12) .|. (half .&. 0xfff)
args :: [Word32] -> [Instruction]
args values = concat [constant (Register register) value | (register,value) <- zip [0..] values]
assemble :: [Instruction] -> B.ByteString
assemble instructions = B.concat (emit 0x69d160 instructions)
  where
    labels = collect 0x69d160 instructions
    collect _ [] = M.empty
    collect pc (Mark label:rest) = M.insert label pc (collect pc rest)
    collect pc (_:rest) = collect (pc+4) rest
    target label = M.findWithDefault (error "fixture label missing") label labels
    branch pc opcode destination = opcode .|. (fromIntegral ((toInteger destination-toInteger pc-8) `div` 4) .&. 0xffffff)
    emit _ [] = []
    emit pc (Mark _:rest) = emit pc rest
    emit pc (instruction:rest) = U.putWord32 (encode pc instruction) : emit (pc+4) rest
    encode pc instruction = case instruction of
      Raw word -> word
      Call destination -> branch pc 0xeb000000 destination
      Tail destination -> branch pc 0xea000000 destination
      Jump condition label -> branch pc (case condition of Always -> 0xea000000; Equal -> 0x0a000000; NotEqual -> 0x1a000000) (target label)
      LoadLiteral (Register register) label -> 0xe59f0000 .|. (register `shiftL` 12) .|. (target label-pc-8)
      Mark _ -> error "fixture label emitted"

symbolsFor :: B.ByteString -> B.ByteString
symbolsFor bytes = C.unlines (map C.pack
  ([row 0x69d160 0 "T" "__root_task_start",row end 0 "T" "__root_task_end"
   ,row 0x69d160 (fromIntegral (B.length bytes)) "T" "xm1_root_task"]
   ++ [row address 0 "A" name | (name,address) <- serviceSymbols]))
  where
    end = 0x69d160+fromIntegral (B.length bytes)
    row :: Word32 -> Word32 -> String -> String -> String
    row address size kind name = showHex address " " ++ showHex size " " ++ kind ++ " " ++ name
