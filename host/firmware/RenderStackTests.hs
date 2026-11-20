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

entries :: [String]
entries =
  [ "rb_render_tree_entry", "rb_render_picker_entry", "rb_render_pane_entry", "rb_bank_draw_root", "rb_render_frame_entry"
  , "rb_render_frame_commit", "rb_render_frame_complete", "rb_render_frame_scanout_bridge", "rb_render_frame_scanout_delivery" ]

-- | One `sub sp, sp, #reservation; bx lr` function per entry, in `entries` order.
fixture :: [Word32] -> Either String ELF.Elf
fixture reservations = ELF.parseElf bytes
 where
  code = B.concat [word (0xe24dd000 + reservation) <> word 0xe12fff1e | reservation <- reservations]
  names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
  strings = C.pack (concatMap ('\0' :) entries ++ "\0")
  nameOffsets = scanl (\offset name -> offset + fromIntegral (length name) + 1) 1 entries
  symbol name address = B.concat (map word [name, address, 8]) <> B.pack [0x12, 0] <> half 1
  symbols = B.replicate 16 0 <> B.concat [symbol name (base + 8 * index) | (index, name) <- zip [0 ..] (take (length entries) nameOffsets)]
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
checkWith tree picker pane bank = fixture [tree, picker, pane, bank, 16, 16, 16, 16, 16] >>= \elf -> checkRenderStack elf inRuntime

checkFrame :: Word32 -> Word32 -> Either String RenderEntryReport
checkFrame frame commit = frameWith frame commit 16 16 16

frameWith :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Either String RenderEntryReport
frameWith frame commit complete bridge delivery =
  fixture [16, 16, 16, 16, frame, commit, complete, bridge, delivery] >>= \elf -> checkRenderFrameStack elf inRuntime

inRuntime :: Word32 -> Bool
inRuntime address = base <= address && address < base + 8 * fromIntegral (length entries)

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
        , ("frame entry is bounded against task 73's entry headroom", checkFrame 16 16 == Right (RenderEntryReport 16 4032 1024))
        , ("frame raster path calls no stock code and needs no margin", checkFrame 0xc0c 16 == Right (RenderEntryReport 3072 4032 1024))
        , ("frame entry exceeding headroom refuses", isLeft (checkFrame 0xa01 16))
        , ("frame commit keeps the stock margin above the entry frame", isLeft (checkFrame 16 0xc0c) && not (isLeft (checkFrame 16 0xc0a)))
        , ("frame completion keeps the stock margin above the entry frame", isLeft (frameWith 16 16 0xc0c 16 16) && not (isLeft (frameWith 16 16 0xc0a 16 16)))
        , ("scanout bridge and delivery stay within 0x6d80f0's frame",
            not (isLeft (frameWith 16 16 16 32 32)) && isLeft (frameWith 16 16 16 36 16) && isLeft (frameWith 16 16 16 16 36))
        ]
      failures = [name | (name, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("render stack tests passed: " ++ show (length cases))
