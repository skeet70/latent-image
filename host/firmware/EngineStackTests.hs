module Main (main) where

import qualified ArmElf as ELF
import Control.Monad (unless)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.List (mapAccumL)
import Data.Word (Word32)
import EngineStack
import System.Exit (exitFailure)

w32 :: Word32 -> B.ByteString
w32 value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

w16 :: Word32 -> B.ByteString
w16 = B.take 2 . w32

-- | One executable `.text` at `textBase` holding `code`, with global function symbols.
textElf :: Word32 -> [Word32] -> [(String, Word32, Word32)] -> B.ByteString
textElf textBase code functions = B.concat [header, padTo 0x100 header, text, names, strings, symtab, sectionHeaders]
 where
  text = B.concat (map w32 code)
  names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
  strings = B.singleton 0 <> C.pack (concatMap (\(name, _, _) -> name ++ "\0") functions)
  offsets = snd (mapAccumL (\offset (name, _, _) -> (offset + length name + 1, offset)) 1 functions)
  symtab = B.replicate 16 0 <> B.concat [w32 (fromIntegral offset) <> w32 address <> w32 size <> B.pack [0x12, 0] <> w16 1 | (offset, (_, address, size)) <- zip offsets functions]
  textAt = 0x100
  namesAt = textAt + B.length text
  stringsAt = namesAt + B.length names
  symtabAt = stringsAt + B.length strings
  headersAt = symtabAt + B.length symtab
  section name kind flags address offset size link entsize = B.concat (map w32 [name, kind, flags, address, fromIntegral offset, fromIntegral size, link, 0, 4, entsize])
  sectionHeaders = B.concat
    [ B.replicate 40 0
    , section 1 1 6 textBase textAt (B.length text) 0 0
    , section 7 3 0 0 namesAt (B.length names) 0 0
    , section 17 3 0 0 stringsAt (B.length strings) 0 0
    , section 25 2 0 0 symtabAt (B.length symtab) 3 16 ]
  header = B.pack [0x7f, 69, 76, 70, 1, 1, 1] <> B.replicate 9 0 <> w16 2 <> w16 40 <> w32 1 <> w32 textBase <> w32 0
    <> w32 (fromIntegral headersAt) <> w32 0 <> w16 52 <> w16 32 <> w16 0 <> w16 40 <> w16 5 <> w16 2
  padTo size bytes = B.replicate (size - B.length bytes) 0

bl :: Word32 -> Word32 -> Word32
bl from to = 0xeb000000 .|. (((to - (from + 8)) `shiftR` 2) .&. 0x00ffffff)

base :: Word32
base = 0x00c40000

inRuntime :: Word32 -> Bool
inRuntime address = base <= address && address < base + 0x1000

depthOf :: [Word32] -> [(String, Word32, Word32)] -> String -> Either String Word32
depthOf code functions root = do
  elf <- ELF.parseElf (textElf base code functions)
  stackDepth elf inRuntime root

main :: IO ()
main = do
  let push4lr = 0xe92d4010; pop4pc = 0xe8bd8010; sub16 = 0xe24dd010; add16 = 0xe28dd010
      push47lr = 0xe92d40f0; pop47pc = 0xe8bd80f0
      -- root: push {r4, lr}; sub sp, #16; bl callee; add sp, #16; pop {r4, pc}
      root = [push4lr, sub16, bl (base + 8) (base + 20), add16, pop4pc]
      -- callee: push {r4-r7, lr}; bl stock; pop {r4-r7, pc}
      callee = [push47lr, bl (base + 24) 0x00d52684, pop47pc]
      functions = [("root", base, 20), ("callee", base + 20, 12)]
      cases =
        [ ("a frame adds its pushes, its reservation and its deepest callee", depthOf (root ++ callee) functions "root" == Right 44)
        , ("a stock callee adds nothing", depthOf (root ++ callee) functions "callee" == Right 20)
        , ("recursion refuses", isLeft (depthOf [push4lr, bl base base, pop4pc] [("loop", base, 12)] "loop"))
        , ("a variable reservation refuses", isLeft (depthOf [push4lr, 0xe04dd000, pop4pc] [("alloca", base, 12)] "alloca"))
        , ("a restore of sp from r11 is accepted", depthOf [push4lr, sub16, 0xe1a0d00b, pop4pc] [("restore", base, 16)] "restore" == Right 24)
        , ("a register move into sp refuses", isLeft (depthOf [push4lr, 0xe1a0d000, pop4pc] [("moved", base, 12)] "moved"))
        , ("a load into sp refuses", isLeft (depthOf [push4lr, 0xe590d000, pop4pc] [("loaded", base, 12)] "loaded"))
        , ("a call into the middle of a function refuses",
            isLeft (depthOf ([push4lr, sub16, bl (base + 8) (base + 24), add16, pop4pc] ++ callee) functions "root"))
        , ("a missing root refuses", isLeft (depthOf (root ++ callee) functions "absent"))
        ]
      failures = [label | (label, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("engine stack tests passed: " ++ show (length cases))
