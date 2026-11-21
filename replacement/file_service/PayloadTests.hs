module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32, Word8)
import Replacement.FileService.Payload (payloadDefined)

check :: String -> Bool -> IO ()
check label condition = unless condition (fail label)

word :: Word32 -> [Word8]
word w = [fromIntegral (w `div` (256 ^ i)) | i <- [0 .. 3 :: Int]]

-- | A 0x50-byte receive buffer: the posted bytes, then the given filler.
buffer :: Word8 -> [Word8] -> [Word8]
buffer filler posted = take 0x50 (posted ++ repeat filler)

-- | The 0x2c04 payload 0x70f55c posts: callback, path, value, selector 0, over stack residue.
attribute :: [Word8] -> [Word8]
attribute residue =
  word 0x0121aa58 ++ B.unpack path ++ [0] ++ take (0x30 - 1 - B.length path) residue ++ word 1 ++ [0] ++ take 3 (drop 0x20 residue)
  where
    path = C.pack "C:\\DCIM\\963_FUJI\\DSCF3097.JPG"

main :: IO ()
main = do
  let stock = attribute (cycle [0x30, 0x07, 0x00, 0x00, 0x20, 0x13])
      owned = attribute (cycle [0xcc, 0x45, 0x05, 0x40])
      flags = payloadDefined 0x2c04 0x3c (buffer 0 stock)
      visible bytes = [b | (b, True) <- zip (buffer 0 bytes) flags]
      defined = [o | (o, True) <- zip [0 :: Int ..] flags]
  check "attribute set defines callback, path through its terminator, value and selector" (defined == [0 .. 0x21] ++ [0x34 .. 0x38])
  check "attribute set residue is masked" (visible stock == visible owned)
  check "attribute set value is compared" (visible stock /= visible (take 0x34 stock ++ word 2 ++ drop 0x38 stock))
  check "attribute set selector is compared" (visible stock /= visible (take 0x38 stock ++ [1] ++ drop 0x39 stock))
  check "attribute set path is compared" (visible stock /= visible (take 4 stock ++ [0x44] ++ drop 5 stock))
