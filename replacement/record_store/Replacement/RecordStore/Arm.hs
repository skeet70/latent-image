{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Arm where

import Data.Bits
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word
import Emulation.OwnedCase (word32le)

newtype Label = Label String deriving (Eq, Ord, Show)
data Ins = Word Word32 | Define Label | Branch Word32 Bool Label | Address Word32 Label deriving (Eq, Show)
label :: String -> Label
label = Label
at :: String -> Ins
at = Define . label
bl, jump :: String -> Ins
bl = Branch 14 True . label
jump = Branch 14 False . label
whenEq, whenNe, whenGe, whenHs :: String -> Ins
whenEq = Branch 0 False . label
whenNe = Branch 1 False . label
whenGe = Branch 10 False . label
whenHs = Branch 2 False . label
address :: Word32 -> String -> Ins
address r = Address r . label
movHalf :: Word32 -> Word32 -> Word32 -> Word32
movHalf op rd value = 0xe0000000 .|. op `shiftL` 20 .|. rd `shiftL` 12 .|. (value .&. 0xf000) `shiftL` 4 .|. (value .&. 0xfff)
load :: Word32 -> Word32 -> [Ins]
load rd value = [Word (movHalf 0x30 rd value), Word (movHalf 0x34 rd (value `shiftR` 16))]
imm :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Ins
imm op flags rd rn value = Word (0xe2000000 .|. op `shiftL` 21 .|. flags `shiftL` 20 .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. encoded)
  where
    options = [(fromIntegral rot `shiftL` 8) .|. (value `rotateL` (2 * rot)) | rot <- [0..15], value `rotateL` (2 * rot) <= 255]
    encoded = case options of x:_ -> x; [] -> error ("unencodable immediate " ++ show value)
mov, add, sub, bic, cmp :: Word32 -> Word32 -> Word32 -> Ins
mov rd _ = imm 13 0 rd 0
add = imm 4 0
sub = imm 2 0
bic = imm 14 0
cmp rn _ = imm 10 1 0 rn
reg :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Ins
reg op flags rd rn rm = Word (0xe0000000 .|. op `shiftL` 21 .|. flags `shiftL` 20 .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. rm)
movR :: Word32 -> Word32 -> Ins
movR rd = reg 13 0 rd 0
addR, andR :: Word32 -> Word32 -> Word32 -> Ins
addR = reg 4 0
andR = reg 0 0
cmpR :: Word32 -> Word32 -> Ins
cmpR = reg 10 1 0
lsl :: Word32 -> Word32 -> Word32 -> Ins
lsl rd rm amount = Word (0xe1a00000 .|. rd `shiftL` 12 .|. amount `shiftL` 7 .|. rm)
lsr :: Word32 -> Word32 -> Word32 -> Ins
lsr rd rm amount = Word (0xe1a00020 .|. rd `shiftL` 12 .|. amount `shiftL` 7 .|. rm)
transfer :: Bool -> Bool -> Word32 -> Word32 -> Word32 -> Ins
transfer byte reading rn rd offset = Word (0xe5800000 .|. (if byte then 0x400000 else 0) .|. (if reading then 0x100000 else 0) .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. offset)
ldr, str, ldrb, strb :: Word32 -> Word32 -> Word32 -> Ins
ldr = transfer False True
str = transfer False False
ldrb = transfer True True
strb = transfer True False
push, pop :: [Int] -> Ins
push rs = Word (0xe92d0000 .|. foldr (\r bits -> setBit bits r) 0 rs)
pop rs = Word (0xe8bd0000 .|. foldr (\r bits -> setBit bits r) 0 rs)
ret :: Ins
ret = Word 0xe12fff1e
assemble :: Word32 -> [(String, Word32)] -> [Ins] -> (B.ByteString, M.Map Label Word32, [(Word32, Word32)])
assemble base seeds code = (B.concat (zipWith emit offsets code), symbols, functions)
  where
    size (Define _) = 0
    size (Address _ _) = 8
    size _ = 4
    offsets = scanl (\offset instruction -> offset + size instruction) 0 code
    definitions = [(name, base + offset) | (Define name, offset) <- zip code offsets]
    symbols = M.fromList ([(label name, value) | (name, value) <- seeds] ++ definitions)
    resolve name = M.findWithDefault (error ("missing assembler label " ++ show name)) name symbols
    starts = [value | (Label name, value) <- definitions, take 3 name == "fn_"]
    ends = drop 1 starts ++ [base + last offsets]
    functions = zip starts (zipWith (-) ends starts)
    emit _ (Word value) = word32le value
    emit _ (Define _) = B.empty
    emit _ (Address rd name) = let value = resolve name in B.concat [word32le (movHalf 0x30 rd value), word32le (movHalf 0x34 rd (value `shiftR` 16))]
    emit offset (Branch condition link name) =
      let delta = resolve name - base - offset - 8
      in word32le (condition `shiftL` 28 .|. 0x0a000000 .|. (if link then 0x1000000 else 0) .|. (delta `shiftR` 2 .&. 0xffffff))
