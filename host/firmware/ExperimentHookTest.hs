module Main (main) where

import A32 (decode)
import Control.Monad (unless)
import Data.Word (Word32)
import ExperimentHook (positionIndependent)
import Numeric (showHex)
import System.Exit (exitFailure)

-- | (address, word, may it be displaced?)
cases :: [(Word32, Word32, Bool)]
cases =
  [ (0x00bfb028, 0xe92d4ff0, True) -- push {r4-r11, lr}
  , (0x00bfb02c, 0xe28db020, True) -- add r11, sp, #32
  , (0x00f41c48, 0xe92d48f0, True) -- push {r4-r7, r11, lr}
  , (0x00001000, 0xe1a04000, True) -- mov r4, r0
  , (0x00001000, 0xe59f3010, False) -- ldr r3, [pc, #16]
  , (0x00001000, 0xe28f0008, False) -- add r0, pc, #8
  , (0x00001000, 0xeb000010, False) -- bl
  , (0x00001000, 0xea000010, False) -- b
  , (0x00001000, 0xe12fff1e, False) -- bx lr
  , (0x00001000, 0xe8bd8010, False) -- pop {r4, pc}
  , (0x00001000, 0xe1a0f00e, False) -- mov pc, lr
  ]

main :: IO ()
main = do
  let failures = [(address, word, expected) | (address, word, expected) <- cases, positionIndependent (decode address word) /= expected]
  mapM_ (\(address, word, expected) -> putStrLn ("0x" ++ showHex word "" ++ " at 0x" ++ showHex address "" ++ ": expected displaceable=" ++ show expected)) failures
  unless (null failures) exitFailure
  putStrLn ("experiment hook: " ++ show (length cases) ++ " cases")
