-- | A32 encodings for the oracle's hand-written control bodies, with forward and backward labels.
module Replacement.RtosOwnFirst.Assembly
  ( Reg (..)
  , r0
  , r1
  , r2
  , r3
  , r11
  , r12
  , sp
  , lr
  , pc
  , Cond (..)
  , Shift (..)
  , Label (..)
  , Item
  , on
  , mark
  , branch
  , branchLink
  , assembleAt
  , movw
  , movt
  , loadAddress
  , loadWord
  , ldr
  , ldrb
  , str
  , mrs
  , msrControl
  , mrcIdentity
  , andI
  , eorI
  , subI
  , addI
  , tstI
  , cmpI
  , orrI
  , movI
  , bicI
  , mvnI
  , subR
  , addR
  , orrR
  , movR
  , movShift
  , mul
  , push
  , pop
  , bxLr
  ) where

import Data.Bits (rotateL, shiftL, shiftR, (.&.), (.|.))
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Numeric (showHex)

newtype Reg = Reg Int deriving (Eq, Show)

r0, r1, r2, r3, r11, r12, sp, lr, pc :: Reg
r0 = Reg 0
r1 = Reg 1
r2 = Reg 2
r3 = Reg 3
r11 = Reg 11
r12 = Reg 12
sp = Reg 13
lr = Reg 14
pc = Reg 15

data Cond = CondEq | CondNe | CondGe | CondLt | CondAl deriving (Eq, Show)

condBits :: Cond -> Word32
condBits cond = case cond of
  CondEq -> 0x0
  CondNe -> 0x1
  CondGe -> 0xa
  CondLt -> 0xb
  CondAl -> 0xe

data Shift = Lsl | Lsr | Asr deriving (Eq, Show)

newtype Label = Label String deriving (Eq, Ord, Show)

data Target = ToLabel Label | ToAddress Word32 deriving (Eq, Show)

-- | An encoding without its condition field, or why it has none.
data Item
  = Instruction Cond (Either String Word32)
  | Transfer Cond Bool Target
  | Mark Label
  deriving (Eq, Show)

on :: Cond -> Item -> Item
on cond item = case item of
  Instruction _ encoded -> Instruction cond encoded
  Transfer _ link target -> Transfer cond link target
  Mark _ -> item

mark :: Label -> Item
mark = Mark

-- | B to a label.
branch :: Label -> Item
branch = Transfer CondAl False . ToLabel

-- | BL to an absolute address.
branchLink :: Word32 -> Item
branchLink = Transfer CondAl True . ToAddress

-- | The words of the items placed at @base@.
assembleAt :: Word32 -> [Item] -> Either String [Word32]
assembleAt base items = do
  let placed = snd (foldl' place (base, []) items)
      place (at, acc) item = case item of
        Mark _ -> (at, (at, item) : acc)
        _ -> (at + 4, (at, item) : acc)
      ordered = reverse placed
      labels = M.fromList [(name, at) | (at, Mark name) <- ordered]
  sequence [encode labels at item | (at, item) <- ordered, not (isMark item)]
  where
    isMark item = case item of
      Mark _ -> True
      _ -> False
    encode labels at item = case item of
      Instruction cond encoded -> (\word -> (condBits cond `shiftL` 28) .|. word) <$> encoded
      Transfer cond link target -> do
        destination <- case target of
          ToAddress address -> Right address
          ToLabel name -> maybe (Left ("undefined label " ++ show name)) Right (M.lookup name labels)
        let delta = toInteger destination - toInteger at - 8
        if delta `mod` 4 /= 0 || delta < -0x2000000 || delta > 0x1fffffc
          then Left ("branch out of range at " ++ show at)
          else Right ((condBits cond `shiftL` 28) .|. (if link then 0x0b000000 else 0x0a000000) .|. (fromInteger (delta `div` 4) .&. 0xffffff))
      Mark _ -> Left "unreachable mark"

instruction :: Word32 -> Item
instruction = Instruction CondAl . Right

reg :: Reg -> Word32
reg (Reg n) = fromIntegral n

-- | The 12-bit modified immediate holding @value@.
modifiedImmediate :: Word32 -> Either String Word32
modifiedImmediate value = case [(rotation, rotated) | rotation <- [0 .. 15], let rotated = value `rotateL` (2 * rotation), rotated < 0x100] of
  (rotation, rotated) : _ -> Right ((fromIntegral rotation `shiftL` 8) .|. rotated)
  [] -> Left ("immediate 0x" ++ showHex value "" ++ " has no A32 encoding")

dataImmediate :: Word32 -> Bool -> Reg -> Reg -> Word32 -> Item
dataImmediate opcode setFlags rn rd value =
  Instruction CondAl ((\imm -> 0x02000000 .|. (opcode `shiftL` 21) .|. flagBit setFlags .|. (reg rn `shiftL` 16) .|. (reg rd `shiftL` 12) .|. imm) <$> modifiedImmediate value)

dataRegister :: Word32 -> Bool -> Reg -> Reg -> Reg -> Shift -> Word32 -> Item
dataRegister opcode setFlags rn rd rm shift amount =
  instruction ((opcode `shiftL` 21) .|. flagBit setFlags .|. (reg rn `shiftL` 16) .|. (reg rd `shiftL` 12) .|. ((amount .&. 31) `shiftL` 7) .|. (shiftBits shift `shiftL` 5) .|. reg rm)
  where
    shiftBits kind = case kind of
      Lsl -> 0
      Lsr -> 1
      Asr -> 2

flagBit :: Bool -> Word32
flagBit setFlags = if setFlags then 0x00100000 else 0

andI, eorI, subI, addI, orrI, bicI :: Reg -> Reg -> Word32 -> Item
andI rd rn = dataImmediate 0x0 False rn rd
eorI rd rn = dataImmediate 0x1 False rn rd
subI rd rn = dataImmediate 0x2 False rn rd
addI rd rn = dataImmediate 0x4 False rn rd
orrI rd rn = dataImmediate 0xc False rn rd
bicI rd rn = dataImmediate 0xe False rn rd

tstI, cmpI :: Reg -> Word32 -> Item
tstI rn = dataImmediate 0x8 True rn (Reg 0)
cmpI rn = dataImmediate 0xa True rn (Reg 0)

movI, mvnI :: Reg -> Word32 -> Item
movI rd = dataImmediate 0xd False (Reg 0) rd
mvnI rd = dataImmediate 0xf False (Reg 0) rd

subR, addR, orrR :: Reg -> Reg -> Reg -> Item
subR rd rn rm = dataRegister 0x2 False rn rd rm Lsl 0
addR rd rn rm = dataRegister 0x4 False rn rd rm Lsl 0
orrR rd rn rm = dataRegister 0xc False rn rd rm Lsl 0

movR :: Reg -> Reg -> Item
movR rd rm = dataRegister 0xd False (Reg 0) rd rm Lsl 0

movShift :: Reg -> Reg -> Shift -> Word32 -> Item
movShift rd rm = dataRegister 0xd False (Reg 0) rd rm

movw, movt :: Reg -> Word32 -> Item
movw rd value = instruction (0x03000000 .|. ((value `shiftR` 12 .&. 0xf) `shiftL` 16) .|. (reg rd `shiftL` 12) .|. (value .&. 0xfff))
movt rd value = instruction (0x03400000 .|. ((value `shiftR` 12 .&. 0xf) `shiftL` 16) .|. (reg rd `shiftL` 12) .|. (value .&. 0xfff))

loadAddress :: Reg -> Word32 -> [Item]
loadAddress rd address = [movw rd (address .&. 0xffff), movt rd (address `shiftR` 16)]

loadWord :: Reg -> Word32 -> [Item]
loadWord rd address = loadAddress rd address ++ [ldr rd rd 0]

transferImmediate :: Word32 -> Reg -> Reg -> Int -> Item
transferImmediate base rd rn offset
  | abs offset > 0xfff = Instruction CondAl (Left "transfer offset out of range")
  | otherwise = instruction (base .|. (if offset >= 0 then 0x00800000 else 0) .|. (reg rn `shiftL` 16) .|. (reg rd `shiftL` 12) .|. fromIntegral (abs offset))

ldr, ldrb, str :: Reg -> Reg -> Int -> Item
ldr = transferImmediate 0x05100000
ldrb = transferImmediate 0x05500000
str = transferImmediate 0x05000000

mrs :: Reg -> Item
mrs rd = instruction (0x010f0000 .|. (reg rd `shiftL` 12))

-- | @msr CPSR_c, rn@.
msrControl :: Reg -> Item
msrControl rn = instruction (0x0121f000 .|. reg rn)

-- | @mrc p14, 0, rt, c2, c0, 0@.
mrcIdentity :: Reg -> Item
mrcIdentity rt = instruction (0x0e120e10 .|. (reg rt `shiftL` 12))

mul :: Reg -> Reg -> Reg -> Item
mul rd rm rs = instruction (0x00000090 .|. (reg rd `shiftL` 16) .|. (reg rs `shiftL` 8) .|. reg rm)

registerMask :: [Reg] -> Word32
registerMask = foldl' (\mask register -> mask .|. (1 `shiftL` fromIntegral (reg register))) 0

push, pop :: [Reg] -> Item
push registers = instruction (0x092d0000 .|. registerMask registers)
pop registers = instruction (0x08bd0000 .|. registerMask registers)

bxLr :: Item
bxLr = instruction 0x012fff1e
