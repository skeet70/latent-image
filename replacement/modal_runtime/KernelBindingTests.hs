{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.Sha256 (parseSha256Hex, sha256)
import Replacement.ModalRuntime.KernelBinding

main :: IO ()
main = do
  unless (all (isLeft . parseSha256Hex) ["", replicate 63 '0', replicate 65 '0', replicate 64 'g', " " ++ replicate 64 '0']) (error "malformed digest accepted")
  unless (case parseSha256Hex (replicate 64 '0') of Right _ -> True; Left _ -> False) (error "valid digest refused")
  bound <- either error pure (bind expected elf text symbols)
  unless (boundText bound == text && boundTextAddress bound == 0x00be0000 && M.lookup "rb_test" (boundSymbols bound) == Just (KernelSymbol 0x00be0004 4)) (error "bound input differs")
  let wrong = sha256 "different artifact"
  reject "ELF expected identity" (bind (expected {expectedElf = wrong}) elf text symbols)
  reject "text expected identity" (bind (expected {expectedText = wrong}) elf text symbols)
  reject "symbols expected identity" (bind (expected {expectedSymbols = wrong}) elf text symbols)
  reject "changed ELF" (bind expected (elf <> "changed") text symbols)
  let foreignText = B.replicate 8 0
  reject "text detached from ELF" (bind (expected {expectedText = sha256 foreignText}) elf foreignText symbols)
  mapM_ rejectSymbols
    [ ("symbol address detached from ELF", "00be0000 A __rb_image_start\n00be0000 00000004 T rb_test\n")
    , ("symbol size detached from ELF", "00be0000 A __rb_image_start\n00be0004 00000008 T rb_test\n")
    , ("missing symbol", "00be0000 A __rb_image_start\n")
    , ("duplicate symbol", symbols <> "00be0004 00000004 T rb_test\n")
    ]
  let outsideElf = fixtureElf 0x00be0008 4
      outsideSymbols = "00be0000 A __rb_image_start\n00be0008 00000004 T rb_test\n"
  reject "function outside text" (bind (KernelDigests (sha256 outsideElf) (sha256 text) (sha256 outsideSymbols)) outsideElf text outsideSymbols)
  let zeroElf = fixtureElf 0x00be0004 0
      zeroSymbols = "00be0000 A __rb_image_start\n00be0004 00000000 T rb_test\n"
  reject "zero-sized function" (bind (KernelDigests (sha256 zeroElf) (sha256 text) (sha256 zeroSymbols)) zeroElf text zeroSymbols)
  where
    bind digests = bindKernel digests ["rb_test"]
    expected = KernelDigests (sha256 elf) (sha256 text) (sha256 symbols)
    rejectSymbols (label, bytes) = reject label (bind (expected {expectedSymbols = sha256 bytes}) elf text bytes)

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

reject :: String -> Either String a -> IO ()
reject label result = unless (isLeft result) (error (label ++ " accepted"))

text :: B.ByteString
text = B.pack [0x1e, 0xff, 0x2f, 0xe1, 0x1e, 0xff, 0x2f, 0xe1]

symbols :: B.ByteString
symbols = "00be0000 A __rb_image_start\n00be0004 00000004 T rb_test\n"

elf :: B.ByteString
elf = fixtureElf 0x00be0004 4

-- Synthetic ARM ELF32 with two returning A32 functions and an llvm-nm listing.
fixtureElf :: Word32 -> Word32 -> B.ByteString
fixtureElf address size = B.concat
  [ B.pack [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1, 0] <> B.replicate 8 0
  , halfwords [2, 40] <> words32 [1, 0x00be0000, 0, 52, 0] <> halfwords [52, 32, 0, 40, 5, 2]
  , B.replicate 40 0
  , section [1, 1, 6, 0x00be0000, 252, 8, 0, 0, 4, 0]
  , section [7, 3, 0, 0, 260, fromIntegral (B.length sectionNames), 0, 0, 1, 0]
  , section [17, 3, 0, 0, stringOffset, fromIntegral (B.length symbolNames), 0, 0, 1, 0]
  , section [25, 2, 0, 0, symbolOffset, 48, 3, 1, 4, 16]
  , text, sectionNames, symbolNames
  , B.replicate 16 0
  , words32 [1, 0x00be0000, 0] <> B.pack [0x10, 0, 1, 0]
  , words32 [18, address, size] <> B.pack [0x12, 0, 1, 0]
  ]
  where
    section = words32
    sectionNames = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
    symbolNames = C.pack "\0__rb_image_start\0rb_test\0"
    stringOffset = 260 + fromIntegral (B.length sectionNames)
    symbolOffset = stringOffset + fromIntegral (B.length symbolNames)

words32 :: [Word32] -> B.ByteString
words32 = B.pack . concatMap (\word -> [fromIntegral (word `div` (256 ^ index)) | index <- [0 :: Int .. 3]])

halfwords :: [Word32] -> B.ByteString
halfwords = B.pack . concatMap (\word -> [fromIntegral word, fromIntegral (word `div` 256)])
