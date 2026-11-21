{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Text as T
import Data.Word (Word32, Word8)
import Replacement.ModalRuntime.FrameSubmission
import Replacement.ModalRuntime.MenuEnginePair (Hit (..), Site (..))
import Replacement.ModalRuntime.RenderStackEvidence (InstructionCounter (..))
import System.Exit (exitFailure)

hit :: T.Text -> Int -> (Word32, Word32, Word32, Word32) -> [B.ByteString] -> Integer -> Hit
hit label core registers memory instructions =
  Hit (OtherSite label) core registers Nothing Nothing memory Nothing (Just (InstructionCounter (fromIntegral instructions)))

le32 :: Word32 -> [Word8]
le32 value = [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

-- | `rb_render_frame_record` layout 1 with `owned` frames at +0x3c.
record :: Word32 -> B.ByteString
record owned = B.pack (concat [le32 0x52464252, le32 1, replicate 0x34 0, le32 owned, replicate 0x38 0])

frameReads :: Maybe Word32 -> [B.ByteString]
frameReads owned = [B.pack [0], B.replicate 8 0, B.replicate 4 0, B.replicate 4 0, B.replicate 4 0] ++ maybe [] (pure . record) owned

entry, leave :: Maybe Word32 -> Integer -> Hit
entry owned = hit "frame-step-entry" 0 (0, 0, 0, 0) (frameReads owned)
leave owned = hit "frame-dispatch-return" 0 (0, 0, 0, 0) (frameReads owned)

translate :: Word32 -> Integer -> Hit
translate x = hit "vg-translate" 0 (x, 7, 0, 0) []

-- | `vg-set-integers` at a frame-local pointer `pointer` with rectangle `values`.
scissor :: Word32 -> [Word32] -> Integer -> Hit
scissor pointer values = hit "vg-set-integers" 0 (0x1106, 4, pointer, 0) [B.pack (concatMap le32 values)]

dispatch :: Int -> Word8 -> Integer -> Hit
dispatch core kind = hit "vg-job-dispatch" core (0, 0, 0, 0) [B.pack (kind : replicate 111 0)]

phase :: [Hit] -> PhaseSubmission
phase hits = either error id (phaseSubmission "picker-route" hits)

main :: IO ()
main = do
  let stock = phase [entry (Just 0) 100, translate 1 110, scissor 0x9000 [1, 2, 3, 4] 120, dispatch 1 3 130, leave (Just 0) 400]
      owned = phase [entry (Just 4) 100, translate 1 105, scissor 0xa000 [1, 2, 3, 4] 106, dispatch 1 3 107, leave (Just 5) 200]
      checks =
        [ ("work on the frame's core and engine work on the other core belong to the open frame",
            map (\frame -> (length (frameCalls frame), length (frameEngine frame))) (phaseFrames owned) == [(2, 1)])
        , ("a frame the owned record counted is an owned step", map framePath (phaseFrames owned) == [OwnedStep])
        , ("instructions run from entry to dispatcher return",
            map frameInstructions (phaseFrames stock ++ phaseFrames owned) == [Just 300, Just 100])
        , ("arrays compare by content, not by where each role's frame put them", compareSubmission [stock] [owned] == Right Nothing)
        , ("a different value is the first difference",
            fmap (fmap mismatchIndex) (compareSubmission [stock] [phase [entry (Just 4) 1, translate 2 2, scissor 0xa000 [1, 2, 3, 4] 3, dispatch 1 3 4, leave (Just 5) 5]]) == Right (Just 0))
        , ("different pointed bytes differ",
            fmap (fmap mismatchIndex) (compareSubmission [stock] [phase [entry (Just 4) 1, translate 1 2, scissor 0x9000 [1, 2, 3, 5] 3, dispatch 1 3 4, leave (Just 5) 5]]) == Right (Just 1))
        , ("a different engine command differs in the engine stream",
            fmap (fmap mismatchStream) (compareSubmission [stock] [phase [entry (Just 4) 1, translate 1 2, scissor 0x9000 [1, 2, 3, 4] 3, dispatch 1 4 4, leave (Just 5) 5]]) == Right (Just EngineStream))
        , ("a missing operation differs", case compareSubmission [stock] [phase [entry (Just 4) 1, translate 1 2, dispatch 1 3 4, leave (Just 5) 5]] of
            Right (Just mismatch) -> mismatchIndex mismatch == 1 && mismatchOwned mismatch == Nothing
            _ -> False)
        , ("a different number of frame steps is refused",
            either (const True) (const False) (compareSubmission [stock] [phase []]))
        , ("work outside every frame step is compared too",
            compareSubmission [phase [translate 1 1]] [phase [translate 2 1]] /= Right Nothing)
        , ("a count beyond the read is refused",
            either (const True) (const False) (phaseSubmission "p" [hit "vg-set-vector" 0 (0x1114, 9, 0, 0) [B.replicate 32 0] 1]))
        , ("a dash pattern compares only its counted floats",
            compareSubmission [phase [hit "vg-set-vector" 0 (0x1114, 1, 0, 0) [B.pack (1 : replicate 31 0)] 1]]
                              [phase [hit "vg-set-vector" 0 (0x1114, 1, 0, 0) [B.pack ([1, 0, 0, 0] ++ replicate 28 9)] 1]] == Right Nothing)
        ]
  failures <- fmap concat . mapM (\(name, ok) -> if ok then pure [] else putStrLn ("FAIL: " ++ name) >> pure [name]) $ checks
  putStrLn (show (length checks - length failures) ++ " of " ++ show (length checks) ++ " frame submission checks passed")
  unless (null failures) exitFailure
