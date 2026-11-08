module Main (main) where

import qualified ArmElf as ELF
import Control.Monad (unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.Word (Word32)
import RenderStack
import System.Exit (exitFailure)

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

half :: Word32 -> B.ByteString
half = B.take 2 . word

fixture :: Word32 -> Word32 -> Word32 -> Word32 -> Either String ELF.Elf
fixture treeReservation pickerReservation paneReservation bankReservation = ELF.parseElf bytes
 where
  code = B.concat [word (0xe24dd000 + reservation) <> word 0xe12fff1e | reservation <- [treeReservation,pickerReservation,paneReservation,bankReservation]]
  names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
  strings = C.pack "\0rb_render_tree_entry\0rb_render_picker_entry\0rb_render_pane_entry\0rb_bank_draw_root\0"
  symbol name address = B.concat (map word [name, address, 8]) <> B.pack [0x12, 0] <> half 1
  symbols = B.replicate 16 0 <> symbol 1 base <> symbol 22 (base + 8) <> symbol 45 (base + 16) <> symbol 66 (base + 24)
  namesAt = 0x100 + B.length code
  stringsAt = namesAt + B.length names
  symbolsAt = stringsAt + B.length strings
  headersAt = symbolsAt + B.length symbols
  section :: Word32 -> Word32 -> Word32 -> Word32 -> Int -> Int -> Word32 -> Word32 -> B.ByteString
  section name kind flags address offset size link entrySize = B.concat (map word [name, kind, flags, address, fromIntegral offset, fromIntegral size, link, 0, 4, entrySize])
  sections = B.concat
    [ B.replicate 40 0
    , section 1 1 6 base 0x100 (B.length code) 0 0
    , section 7 3 0 0 namesAt (B.length names) 0 0
    , section 17 3 0 0 stringsAt (B.length strings) 0 0
    , section 25 2 0 0 symbolsAt (B.length symbols) 3 16
    ]
  header = B.pack [0x7f, 69, 76, 70, 1, 1, 1] <> B.replicate 9 0 <> half 2 <> half 40 <> word 1 <> word base <> word 0
    <> word (fromIntegral headersAt) <> word 0 <> half 52 <> half 32 <> half 0 <> half 40 <> half 5 <> half 2
  bytes = header <> B.replicate (0x100 - B.length header) 0 <> code <> names <> strings <> symbols <> sections

base :: Word32
base = 0x00c40000

check :: Word32 -> Word32 -> Either String RenderStackReport
check tree picker = checkWith tree picker 16 16

checkWith :: Word32 -> Word32 -> Word32 -> Word32 -> Either String RenderStackReport
checkWith tree picker pane bank = fixture tree picker pane bank >>= \elf -> checkRenderStack elf (\address -> base <= address && address < base + 32)

main :: IO ()
main = do
  let cases =
        [ ("independent entries retain separate depths", check 16 32 == Right (RenderStackReport (RenderEntryReport 16 3416 1024) (RenderEntryReport 32 3720 1024) (RenderEntryReport 16 3984 1024)))
        , ("tree exceeding headroom refuses", isLeft (check 0xc0c 16))
        , ("picker exceeding headroom refuses", isLeft (check 16 0xc0c))
        , ("margin is included in admission", isLeft (check 0xc0a 16))
        , ("picker may exceed the tree allowance", not (isLeft (check 16 0xc0a)))
        , ("pane exceeding headroom refuses", isLeft (checkWith 16 16 0xc0c 16))
        , ("bank root prefix reduces pane headroom", isLeft (checkWith 16 16 16 0xc0c))
        ]
      failures = [name | (name, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("render stack tests passed: " ++ show (length cases))
