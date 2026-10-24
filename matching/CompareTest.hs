module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import FujiAF.Matching.Compare
import System.Exit (exitFailure)

-- Little-endian A32 words.
wordsLE :: [Word32] -> B.ByteString
wordsLE = B.pack . concatMap (\w -> [fromIntegral ((w `shiftR` s) .&. 0xff) | s <- [0, 8, 16, 24]])

-- push {fp, lr}; bl <target>; movw r3, #imm; pop {fp, pc}
stock :: B.ByteString
stock = wordsLE [0xe92d4800, 0xeb0123ab, 0xe3053a50, 0xe8bd8800]

cases :: [(String, ByteComparison, ByteComparison)]
cases =
  [ ("identical section", compareLinked [] 16 stock stock, Exact)
  , ( "branch target differs inside a call relocation"
    , compareLinked [RelocatedField 4 RelCall] 16 (wordsLE [0xe92d4800, 0xebfffffe, 0xe3053a50, 0xe8bd8800]) stock
    , Normalized 3 )
  , ( "branch target differs with no relocation"
    , compareLinked [] 16 (wordsLE [0xe92d4800, 0xebfffffe, 0xe3053a50, 0xe8bd8800]) stock
    , Mismatch 3 4 )
  , ( "movw immediate differs inside its relocation"
    , compareLinked [RelocatedField 8 RelMovw] 16 (wordsLE [0xe92d4800, 0xeb0123ab, 0xe3003000, 0xe8bd8800]) stock
    , Normalized 3 )
  , ( "movw destination register differs inside its relocated word"
    , compareLinked [RelocatedField 8 RelMovw] 16 (wordsLE [0xe92d4800, 0xeb0123ab, 0xe3052a50, 0xe8bd8800]) stock
    , Mismatch 1 9 )
  , ( "matching section shorter than the stock body"
    , compareLinked [] 16 (B.take 12 stock) stock
    , PrefixOnly 4 )
  , ( "literal word beyond the stock body"
    , compareLinked [RelocatedField 16 RelAbs32] 16 (stock <> wordsLE [0x0095b098]) (stock <> wordsLE [0x0095b098])
    , Exact )
  ]

main :: IO ()
main = do
  let failures = [(name, got, want) | (name, got, want) <- cases, got /= want]
  mapM_ (\(name, got, want) -> putStrLn (name ++ ": got " ++ show got ++ ", want " ++ show want)) failures
  unless (null failures) exitFailure
  putStrLn (show (length cases) ++ " comparison cases pass")
