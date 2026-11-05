module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified StackWalk as W
import StockStack
import System.Exit (exitFailure)

base :: Word32
base = 0x10000

code :: [Word32] -> StockCode
code words' = stockCode [(base, B.concat [B.pack [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]] | w <- words'])] []

at :: Int -> Word32
at index = base + 4 * fromIntegral index

bl :: Int -> Int -> Word32
bl from to = 0xeb000000 .|. (((at to - (at from + 8)) `shiftR` 2) .&. 0x00ffffff)

branch :: Word32 -> Int -> Int -> Word32
branch cond from to = cond .|. 0x0a000000 .|. (((at to - (at from + 8)) `shiftR` 2) .&. 0x00ffffff)

noTargets :: IndirectTargets
noTargets = IndirectTargets 0x96bd20 M.empty M.empty []

depth :: [Word32] -> Supplied -> IndirectTargets -> SwitchModel -> Either StackRefusal StackReport
depth words' supplied indirect switches = stockDepth (code words') supplied indirect switches base

bytesOf :: Either StackRefusal StackReport -> Maybe Word32
bytesOf result = case result of
  Right report -> let StackDepth bytes = reportDepth report in Just bytes
  Left _ -> Nothing

none :: Supplied
none = Supplied M.empty

main :: IO ()
main = do
  let pushR4Lr = 0xe92d4010; popR4Pc = 0xe8bd8010; sub16 = 0xe24dd010; add16 = 0xe28dd010
      push47Lr = 0xe92d40f0; pop47Pc = 0xe8bd80f0; bxLr = 0xe12fff1e; cmp0 = 0xe3500000
      always = 0xe0000000; eq = 0x00000000
      -- root: push {r4, lr}; sub sp, #16; bl callee; add sp, #16; pop {r4, pc}; callee: push {r4-r7, lr}; pop {r4-r7, pc}
      nested = [pushR4Lr, sub16, bl 2 5, add16, popR4Pc, push47Lr, pop47Pc]
      cases =
        [ ("a frame adds its reservations and its callee at the call's offset", bytesOf (depth nested none noTargets noSwitches) == Just 44)
        , ("the report lists the entries it walked", fmap reportEntered (depth nested none noTargets noSwitches) == Right [at 0, at 5])
        , ("depth is the deepest path, not the sum of every path",
            -- cmp; beq small; push {r4-r11, lr}; pop {r4-r11, pc}; small: push {r4, lr}; pop {r4, pc}
            bytesOf (depth [cmp0, branch eq 1 4, 0xe92d4ff0, 0xe8bd8ff0, pushR4Lr, popR4Pc] none noTargets noSwitches) == Just 36)
        , ("two paths reaching one instruction at different offsets refuse",
            -- push {lr}; cmp; beq 4; push {r4}; 4: b 4
            case depth [0xe52de004, cmp0, branch eq 2 4, 0xe52d4004, branch always 4 4] none noTargets noSwitches of
              Left (OffsetMismatch _) -> True
              _ -> False)
        , ("recursion refuses", case depth [pushR4Lr, bl 1 0, popR4Pc] none noTargets noSwitches of
            Left (Recursion _) -> True
            _ -> False)
        , ("svc refuses", depth [pushR4Lr, 0xef001010, popR4Pc] none noTargets noSwitches == Left (Refusal (at 1) W.SupervisorCall))
        , ("blx immediate refuses", depth [pushR4Lr, 0xfa000000, popR4Pc] none noTargets noSwitches == Left (Refusal (at 1) W.ThumbCall))
        , ("an exception return refuses", depth [0xe1b0f00e] none noTargets noSwitches == Left (Refusal (at 0) W.ExceptionReturn))
        , ("an unresolved register call refuses", depth [pushR4Lr, 0xe12fff33, popR4Pc] none noTargets noSwitches == Left (UnresolvedBranch (at 1)))
        , ("a supplied entry is not walked and is reported",
            case depth [pushR4Lr, bl 1 3, popR4Pc, 0xffffffff] (Supplied (M.fromList [(at 3, StackDepth 100)])) noTargets noSwitches of
              Right report -> reportDepth report == StackDepth 108 && reportSupplied report == [at 3]
              _ -> False)
        , ("entering owned code refuses",
            stockDepth (stockCode [(base, B.pack (concatMap (\w -> [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]]) [pushR4Lr, bl 1 3, popR4Pc, bxLr]))] [(at 3, at 4)])
              none noTargets noSwitches base == Left (OwnedCode (at 3)))
        , ("a jump table bounded by its cmp is followed",
            -- cmp r0, #1; ldrls pc, [pc, r0, lsl #2]; b 6; .word 5; .word 7; target 5: push {r4-r7,lr}...; 7: bx lr
            bytesOf (depth [0xe3500001, 0x979ff100, branch always 2 7, at 5, at 7, push47Lr, pop47Pc, bxLr] none noTargets noSwitches) == Just 20)
        , ("a switch end ends the path at its depth",
            bytesOf (depth [pushR4Lr, 0xe591d018, 0xe3a0d000] none noTargets (SwitchModel (S.singleton (at 1)) M.empty S.empty)) == Just 8)
        , ("a resume edge continues at the saved offset",
            -- push {r4, lr}; str sp, [r1, #0x18]; ldr sp, [r1, #0x18] (switch end); resume: push {r5-r7}; pop; pop {r4, pc}
            bytesOf (depth [pushR4Lr, 0xe581d018, 0xe591d018, 0xe92d00e0, 0xe8bd00e0, popR4Pc] none noTargets
              (SwitchModel (S.singleton (at 2)) (M.singleton (at 1) (at 3)) S.empty)) == Just 20)
        , ("an IRQ mask through mrs/orr/msr keeps the mode",
            bytesOf (depth [pushR4Lr, 0xe10f0000, 0xe3800080, 0xe121f000, popR4Pc] none noTargets noSwitches) == Just 8)
        , ("an msr to another mode refuses",
            depth [pushR4Lr, 0xe3a00012, 0xe121f000, popR4Pc] none noTargets noSwitches == Left (ModeChange (at 2)))
        , ("stack-neutral system instructions continue",
            -- mrc p15; mcr p15; dmb; ldrex; strex; wfi
            bytesOf (depth [pushR4Lr, 0xee113f50, 0xee0d1f50, 0xf57ff05f, 0xe1932f9f, 0xe1832f91, 0xe320f003, popR4Pc] none noTargets noSwitches) == Just 8)
        , ("a root-slot call resolves through the table and records its condition",
            -- push {r4, lr}; movw r4, #0xbd20; movt r4, #0x96; ldr r3, [r4]; ldr r3, [r3, #0x10]; blx r3; pop {r4, pc}; callee
            case depth [pushR4Lr, 0xe30b4d20, 0xe3404096, 0xe5943000, 0xe5933010, 0xe12fff33, popR4Pc, push47Lr, pop47Pc]
                   none (IndirectTargets 0x96bd20 (M.singleton (SlotOffset 0x10) [at 7]) M.empty []) noSwitches of
              Right report -> reportDepth report == StackDepth 28 && reportConditions report == S.singleton SlotWordsFixed
              _ -> False)
        , ("a conditional load splits the path so each resolution counts",
            -- push {r4, lr}; movw r4; movt r4; ldr r2, [r4]; cmp r0, #0; ldreq r3, [r2, #0x10]; ldrne r3, [r2, #0x14]; blx r3; pop; deep; shallow
            case depth [pushR4Lr, 0xe30b4d20, 0xe3404096, 0xe5942000, cmp0, 0x05923010, 0x15923014, 0xe12fff33, popR4Pc, push47Lr, pop47Pc, pushR4Lr, popR4Pc]
                   none (IndirectTargets 0x96bd20 (M.fromList [(SlotOffset 0x10, [at 11]), (SlotOffset 0x14, [at 9])]) M.empty []) noSwitches of
              Right report -> reportDepth report == StackDepth 28
              _ -> False)
        , ("a return that leaves bytes reserved refuses",
            depth [pushR4Lr, 0xe24dd008, popR4Pc] none noTargets noSwitches == Left (UnbalancedReturn (at 2)))
        , ("bx lr with bytes still reserved refuses", depth [0xe52d4004, bxLr] none noTargets noSwitches == Left (UnbalancedReturn (at 1)))
        , ("bx lr after loading lr from memory is an unresolved jump",
            depth [0xe590e000, bxLr] none noTargets noSwitches == Left (UnresolvedBranch (at 1)))
        , ("bx lr after a call without restoring lr is an unresolved jump",
            depth [bl 0 2, bxLr, bxLr] none noTargets noSwitches == Left (UnresolvedBranch (at 1)))
        , ("bx lr after lr is popped from the stack returns",
            -- push {lr}; bl callee; pop {lr}; bx lr; callee: bx lr
            bytesOf (depth [0xe52de004, bl 1 4, 0xe49de004, bxLr, bxLr] none noTargets noSwitches) == Just 4)
        , ("a conditional push refuses", depth [cmp0, 0x092d4010, popR4Pc] none noTargets noSwitches == Left (Refusal (at 1) W.UnaccountedSpWrite))
        , ("stmia sp! refuses", depth [0xe8ad0010, bxLr] none noTargets noSwitches == Left (Refusal (at 0) W.UnaccountedSpWrite))
        , ("sub sp by a register refuses", depth [0xe04dd000, bxLr] none noTargets noSwitches == Left (Refusal (at 0) W.VariableReservation))
        , ("bic sp refuses", depth [0xe3cdd007, bxLr] none noTargets noSwitches == Left (Refusal (at 0) W.UnaccountedSpWrite))
        , ("a store below SP without writeback refuses", depth [0xe50d0004, bxLr] none noTargets noSwitches == Left (Refusal (at 0) W.BelowStackWrite))
        , ("stmdb sp without writeback refuses", depth [0xe90d0010, bxLr] none noTargets noSwitches == Left (Refusal (at 0) W.BelowStackWrite))
        , ("clrex continues", bytesOf (depth [pushR4Lr, 0xf57ff01f, popR4Pc] none noTargets noSwitches) == Just 8)
        , ("msr of a tracked SVC constant keeps the mode", bytesOf (depth [pushR4Lr, 0xe3a00013, 0xe121f000, popR4Pc] none noTargets noSwitches) == Just 8)
        , ("an mrc into the constant's register forgets it",
            depth [pushR4Lr, 0xe3a00013, 0xee110f10, 0xe121f000, popR4Pc] none noTargets noSwitches == Left (ModeChange (at 3)))
        , ("a jump table without an ls condition refuses",
            depth [0xe3500001, 0xe79ff100, at 3, bxLr] none noTargets noSwitches == Left (TableUnbounded (at 1)))
        , ("a jump table whose flags are reset after the cmp refuses",
            depth [0xe3500001, 0xe2911001, 0x979ff100, branch always 3 6, at 6, at 6, bxLr] none noTargets noSwitches == Left (TableUnbounded (at 2)))
        , ("pop of r11 forgets the frame",
            depth [0xe92d4800, 0xe28db004, 0xe8bd0800, 0xe24bd004, bxLr] none noTargets noSwitches == Left (FrameUnknown (at 3)))
        ]
      failures = [label | (label, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("stock stack tests passed: " ++ show (length cases))
