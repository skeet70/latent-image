{-# LANGUAGE OverloadedStrings #-}

module Main where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.ModalRuntime.Closure
import System.Exit (exitFailure)

-- | A hand-encoded A32 fragment and the obligation it must produce.
data Fixture = Fixture
  { fixtureName :: String
  , fixtureBase :: Word32
  , fixtureWords :: [Word32]
  , fixtureKind :: String
  , fixtureSite :: Word32
  }

fixtures :: [Fixture]
fixtures =
  [ Fixture
      "a call clobbers the flags of an earlier compare"
      0x00100000
      [ 0xe3540003 -- cmp r4, #3
      , 0xeb000003 -- bl +0x18
      , 0xe3072588 -- movw r2, #0x7588
      , 0xe3402146 -- movt r2, #0x0146
      , 0x01c240b0 -- strheq r4, [r2]
      , 0xe12fff1e -- bx lr
      , 0xe12fff1e -- +0x18: bx lr
      ]
      "store-value-unresolved"
      0x00100010
  , Fixture
      "a predicated call leaves its arguments live on the skipped path"
      0x00110000
      [ 0xe3070588 -- movw r0, #0x7588
      , 0xe3400146 -- movt r0, #0x0146
      , 0xeb000000 -- bl +0x10
      , 0xe12fff1e -- bx lr
      , 0xe3520000 -- +0x10: cmp r2, #0
      , 0x1b000001 -- blne +0x20
      , 0xe5903000 -- ldr r3, [r0]
      , 0xe12fff1e -- bx lr
      , 0xe12fff1e -- +0x20: bx lr
      ]
      "pointer-escape"
      0x00110008
  , Fixture
      "a switch whose bound compare is separated by a flag write is unbounded"
      0x00120000
      [ 0xe3074588 -- movw r4, #0x7588
      , 0xe3404146 -- movt r4, #0x0146
      , 0xe3510001 -- cmp r1, #1
      , 0xe2955001 -- adds r5, r5, #1
      , 0x979ff101 -- ldrls pc, [pc, r1, lsl #2]
      , 0xea000002 -- b +0x24
      , 0x00120020 -- table
      , 0x00120020 -- table
      , 0xe12fff1e -- +0x20: bx lr
      , 0xe12fff1e -- +0x24: bx lr
      ]
      "pointer-escape"
      0x00120010
  , Fixture
      "an array pointer plus an unknown index stays a pointer"
      0x00130000
      [ 0xe3072588 -- movw r2, #0x7588
      , 0xe3402146 -- movt r2, #0x0146
      , 0xe0822081 -- add r2, r2, r1, lsl #1
      , 0xe1c200b0 -- strh r0, [r2]
      , 0xe12fff1e -- bx lr
      ]
      "computed-index-store"
      0x0013000c
  ]

main :: IO ()
main = do
  let regions = [Region (fixtureName f) StockImage (fixtureBase f) (littleEndian (fixtureWords f)) | f <- fixtures]
      obligations = case proofValue (buildProof (Inputs regions [] [])) of
        Object fields | Just (Array items) <- KM.lookup "obligations" fields -> V.toList items
        _ -> []
      reported kind site =
        any
          ( \value -> case value of
              Object fields ->
                KM.lookup "kind" fields == Just (String (T.pack kind))
                  && KM.lookup "site" fields == Just (String (T.pack (hex site)))
              _ -> False
          )
          obligations
  results <- forM fixtures $ \f -> do
    let ok = reported (fixtureKind f) (fixtureSite f)
    putStrLn ((if ok then "pass: " else "FAIL: ") ++ fixtureName f ++ " (" ++ fixtureKind f ++ " at " ++ hex (fixtureSite f) ++ ")")
    pure ok
  unless (and results) exitFailure

littleEndian :: [Word32] -> B.ByteString
littleEndian = B.pack . concatMap (\w -> [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]])

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
