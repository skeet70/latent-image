-- | The oracle's control bodies: one must-fail mutant per helper, each a one-word patch of the owned
-- code, and two equivalent bodies written independently of the stock and owned code.
module Replacement.RtosScheduler.Controls
  ( Expectation (..)
  , Control (..)
  , mutants
  , equivalents
  , controlBody
  , nop
  , dmbSy
  ) where

import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Assembly
import Replacement.RtosScheduler.Layout
import Replacement.RtosScheduler.Oracle (Body, ownEntryFunction, patchOwn, replaceEntry)
import Replacement.RtosScheduler.Regions (Helper (..))

data Expectation = MustPass | MustFail deriving (Eq, Show)

data Build
  = -- | Rewrites the one word of the helper's own code that matches.
    Patch (Word32 -> Bool) (Word32 -> Word32)
  | -- | Replaces the entry function.
    Replace (Layout -> Either String [Item])

data Control = Control
  { controlName :: String
  , controlHelper :: Helper
  , controlExpectation :: Expectation
  , controlNote :: String
  , controlBuild :: Build
  }

nop, dmbSy :: Word32
nop = 0xe320f000
dmbSy = 0xf57ff05f

controlBody :: Word32 -> Layout -> Control -> Body -> Either String Body
controlBody fill layout control body = case controlBuild control of
  Patch matches rewrite -> patchOwn matches rewrite body
  Replace items -> do
    (entry, _) <- ownEntryFunction body
    code <- items layout >>= assembleAt entry
    replaceEntry fill code body

always :: Word32 -> Word32 -> Word32
always replacement _ = replacement

-- | Single data transfer, immediate offset, P=1, W=0, always: word store or load per @byte@ and @load@.
transfer :: Word32 -> Bool -> Bool -> Word32 -> Bool
transfer cond byte load word = word .&. 0xff700000 == (cond `shiftL` 28) .|. 0x05000000 .|. (if byte then 0x00400000 else 0) .|. (if load then 0x00100000 else 0)

mutants :: [Control]
mutants =
  [ Control "mutant-acquire" Acquire MustFail "returns 1 after taking a free word" $ Patch (== 0xe3a00000) (always 0xe3a00001)
  , Control "mutant-release" Release MustFail "leaves the lock word as it was" $ Patch (transfer 0xe False False) (always nop)
  , Control "mutant-find" Find MustFail "takes clz of the bitmap without rbit" $ Patch (\w -> w .&. 0x0fff0ff0 == 0x06ff0f30) (always nop)
  , Control "mutant-insert" Insert MustFail "preempts on equal priority" $ Patch (\w -> w .&. 0xff000000 == 0x2a000000) (\w -> (w .&. 0x0fffffff) .|. 0x80000000)
  , Control "mutant-remove" Remove MustFail "leaves the ready bit of an emptied list set" $
      Patch (\w -> w .&. 0x0fe00090 == 0x01c00010) (\w -> 0xe1a00000 .|. (w .&. 0xf000) .|. ((w `shiftR` 16) .&. 0xf))
  , Control "mutant-reset" Reset MustFail "keeps T+0xe bits 1 and 2" $ Patch (\w -> w .&. 0x0fe00000 == 0x02000000) (always nop)
  , Control "mutant-prepare" Prepare MustFail "drops the dmb; equal state, refused by the order assertion" $ Patch (== dmbSy) (always nop)
  , Control "mutant-notify" Notify MustFail "returns 0 for a remote id without ringing" $ Patch (\w -> w .&. 0x0f000000 == 0x0b000000) (always nop)
  , Control "mutant-notify-dsb" Notify MustFail "drops the dsb; equal state, refused by the order assertion" $ Patch (== 0xf57ff04f) (always nop)
  , Control "mutant-requeue" Requeue MustFail "returns the raw dispatch-enabled word in the selected-task branch" $ Patch (\w -> w .&. 0x0ff00000 == 0x03000000) (always nop)
  , Control "mutant-rotate" Rotate MustFail "keeps the old selected cell" $ Patch (transfer 0x0 False False) (always nop)
  ]

-- | Find by isolating the lowest bit with a negation, and reset with its stores in reverse order.
equivalents :: [Control]
equivalents =
  [ Control "equivalent-find" Find MustPass "frameless; lowest bit by x & -x, then 31 - clz" $ Replace $ \layout ->
      let c = layoutContext layout
       in Right
            [ ldr r2 r0 (fromIntegral (contextReadyBitmap c))
            , ldr r2 r2 0
            , rsbI r3 r2 0
            , andR r2 r2 r3
            , clz r2 r2
            , rsbI r2 r2 31
            , ldr r0 r0 (fromIntegral (contextReadyHeads c))
            , addShifted r0 r0 r2 Lsl 3
            , ldr r0 r0 (fromIntegral (layoutHeadNext layout))
            , bxLr
            ]
  , Control "equivalent-reset" Reset MustPass "frameless; stores in reverse, bic for the flag mask, byte load of the priority" $ Replace $ \layout ->
      let t = layoutTask layout
          at field = fromIntegral (field t)
       in Right
            [ movI r3 0
            , str r3 r0 (at taskCancelPending)
            , str r3 r0 (at taskTerminatePending)
            , str r3 r0 (at taskPriorityChangePending)
            , str r3 r0 (at taskExceptionPattern)
            , ldrb r2 r0 (at taskFlags)
            , bicI r2 r2 6
            , strb r2 r0 (at taskFlags)
            , strb r3 r0 (at taskState)
            , ldr r12 r0 (at taskDefinition)
            , ldrb r12 r12 (fromIntegral (definitionPriority (layoutDefinition layout)))
            , strb r12 r0 (at taskPriority)
            , bxLr
            ]
  ]
