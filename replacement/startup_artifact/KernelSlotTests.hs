{-# LANGUAGE OverloadedStrings #-}
-- | Synthetic tests of the kernel-slot caller liveness rule and the body walk.
module Main where

import Control.Monad (unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.StartupArtifact.KernelSlots as KS
import Replacement.StartupArtifact.Types

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (fail ("failed: " ++ name))
  modifyIORef' counter (+ 1)

base :: Word32
base = 0x1000

-- | Words laid out from 'base'.
program :: [Word32] -> Word32 -> Maybe Word32
program words' = \address -> M.lookup address table
  where table = M.fromList (zip [base, base + 4 ..] words')

image :: [Word32] -> KS.CodeImage
image words' = KS.CodeImage base (B.concat (map U.putWord32 words'))

at :: Int -> Word32
at index = base + 4 * fromIntegral index

-- | B with the condition from the word at the site to the word at the target.
branch :: Word32 -> Int -> Int -> Word32
branch condition site target = (condition * 0x10000000) .|. 0x0a000000 .|. (fromIntegral (target - site - 2) .&. 0xffffff)

call :: Int -> Int -> Word32
call site target = 0xeb000000 .|. ((fromIntegral (target - site - 2)) .&. 0xffffff)

cmpR6, cmpR0, movneR1, moveqR1, movR1, readR1, bxLr, popPc, moveqR1Two :: Word32
cmpR6 = 0xe3560000 -- cmp r6, #0
cmpR0 = 0xe3500000 -- cmp r0, #0
movneR1 = 0x13a01002 -- movne r1, #2
moveqR1 = 0x03a01001 -- moveq r1, #1
moveqR1Two = 0x03a01002 -- moveq r1, #2
movR1 = 0xe3a01000 -- mov r1, #0
readR1 = 0xe1a00001 -- mov r0, r1
bxLr = 0xe12fff1e
popPc = 0xe8bd8010 -- pop {r4, pc}

-- | Liveness of r1 from the program's first word, entered directly.
deadR1 :: [Word32] -> Bool
deadR1 words' = KS.deadAfter (program words') [] [] (KS.rootTargets []) KS.EnteredDirectly [ClobberR1] base

main :: IO ()
main = do
  counter <- newIORef 0
  let ok = check counter
  ok "complementary conditional writes kill r1" (deadR1 [cmpR6, movneR1, moveqR1, readR1, bxLr])
  ok "a single conditional write leaves r1 live" (not (deadR1 [cmpR6, movneR1, readR1, bxLr]))
  ok "a flag write between the pair leaves r1 live" (not (deadR1 [cmpR6, movneR1, cmpR0, moveqR1, readR1, bxLr]))
  ok "a read before the write leaves r1 live" (not (deadR1 [readR1, movR1, bxLr]))
  ok "a write before the read kills r1" (deadR1 [movR1, readR1, bxLr])
  ok "two writes under the same condition leave r1 live" (not (deadR1 [cmpR6, moveqR1, moveqR1Two, readR1, bxLr]))
  ok "a conditional write on the taken side of a branch under its condition kills r1"
    (deadR1 [cmpR6, moveqR1, branch 0 2 4, movR1, readR1, bxLr])
  ok "a conditional write on the other side of the branch leaves r1 live"
    (not (deadR1 [cmpR6, moveqR1, branch 1 2 4, movR1, readR1, bxLr]))
  ok "a read under the condition of the last write kills r1" (deadR1 [cmpR6, moveqR1, 0x01a00001, bxLr])
  ok "a read under the other condition leaves r1 live" (not (deadR1 [cmpR6, moveqR1, 0x11a00001, bxLr]))
  ok "a return ends the path" (deadR1 [popPc, readR1])
  ok "pending flags read by a condition are live"
    (not (KS.deadAfter (program [movneR1, bxLr]) [] [] (KS.rootTargets []) KS.EnteredDirectly [ClobberFlags] base))
  ok "a direct call into a callee that reads r1 leaves r1 live" (not (deadR1 [call 0 2, bxLr, readR1, bxLr]))
  ok "a direct call into a callee that writes r1 first kills r1" (deadR1 [call 0 2, bxLr, movR1, readR1, bxLr])
  ok "after a root-slot call the next call ends the path"
    (KS.deadAfter (program [call 0 2, bxLr, readR1, bxLr]) [] [] (KS.rootTargets []) KS.EnteredThroughRoot [ClobberR1] base)
  ok "an unresolved indirect call after a direct call leaves r1 live" (not (deadR1 [0xe12fff33, bxLr]))
  ok "a jump table is followed into each target"
    (not (deadR1 [0xe3530001, 0x979ff103, bxLr, at 5, at 6, movR1, readR1, bxLr]))
  ok "a jump table whose targets write r1 kills it"
    (deadR1 [0xe3530001, 0x979ff103, bxLr, at 5, at 5, movR1, readR1, bxLr])

  let walk words' = KS.walkAbi (image words') [(base, base + 4 * fromIntegral (length words'))] (const False) base
      exits = either (const Nothing) Just . walk
      stockPreserves = exits [0xe52db004, 0xe49db004, bxLr]
      ownedSetsR1 = exits [0xe3a01005, bxLr]
      ownedSetsR1Same = exits [0xe3a01005, bxLr]
  ok "the walk admits a body that saves and restores r11 with writeback" (stockPreserves /= Nothing)
  ok "the walk refuses a body that changes r4" (exits [0xe3a04000, bxLr] == Nothing)
  ok "a body that sets r1 changes r1" (fmap KS.changedState ownedSetsR1 == Just [ClobberR1])
  ok "r1 differs between a body that keeps it and one that sets it"
    ((KS.differingState <$> stockPreserves <*> ownedSetsR1) == Just [ClobberR1])
  ok "r1 does not differ between two bodies that set it to the same constant"
    ((KS.differingState <$> ownedSetsR1 <*> ownedSetsR1Same) == Just [])
  passed <- readIORef counter
  putStrLn ("startup-artifact-kernel-slot-tests: " ++ show passed ++ " checks passed")
