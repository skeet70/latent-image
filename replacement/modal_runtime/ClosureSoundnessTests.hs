{-# LANGUAGE OverloadedStrings #-}

module Main where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.ModalRuntime.Closure
import System.Exit (exitFailure)

-- | A hand-encoded A32 fragment and what the proof must report for it.
data Fixture = Fixture
  { fixtureName :: String
  , fixtureBase :: Word32
  , fixtureWords :: [Word32]
  , fixtureExpect :: Expect
  }

data Expect
  = -- | An obligation of this kind at this site.
    ObligationAt String Word32
  | -- | A declared-object consumer at this site with this status and, when
    -- open, this reason.
    ConsumerAt Word32 String (Maybe String)

-- | Materializes 0x007b7000, inside register-write stream B, in r3 and
-- compares the word at r0 against it; `rest` runs on the not-higher path.
bound :: [Word32] -> [Word32]
bound rest =
  [ 0xe5902000 -- ldr r2, [r0]
  , 0xe3a03a07 -- mov r3, #0x7000
  , 0xe340307b -- movt r3, #0x7b
  , 0xe1520003 -- cmp r2, r3
  , 0x83a00000 -- movhi r0, #0
  , 0x812fff1e -- bxhi lr
  ]
    ++ rest

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
      (ObligationAt "store-value-unresolved" 0x00100010)
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
      (ObligationAt "pointer-escape" 0x00110008)
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
      (ObligationAt "pointer-escape" 0x00120010)
  , Fixture
      "an array pointer plus an unknown index stays a pointer"
      0x00130000
      [ 0xe3072588 -- movw r2, #0x7588
      , 0xe3402146 -- movt r2, #0x0146
      , 0xe0822081 -- add r2, r2, r1, lsl #1
      , 0xe1c200b0 -- strh r0, [r2]
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "computed-index-store" 0x0013000c)
  , Fixture
      "a compare-only bound that reaches a return in r3 is only return-conditional"
      0x00140000
      (bound [0xe3a03000, 0xe12fff1e]) -- mov r3, #0; bx lr
      (ConsumerAt 0x00140008 "return-conditional" Nothing)
  , Fixture
      "a compare-only bound overwritten on every path is discharged"
      0x00230000
      [ 0xe3a03a07 -- mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe1520003 -- cmp r2, r3
      , 0x83a00000 -- movhi r0, #0
      , 0xe3a03000 -- mov r3, #0
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x00230004 "discharged" Nothing)
  , Fixture
      "a bound returned in r3 to a caller that loads through it is not discharged"
      0x00200000
      [ 0xe92d4000 -- push {lr}
      , 0xeb000001 -- bl +0x10
      , 0xe5930000 -- ldr r0, [r3]
      , 0xe8bd8000 -- pop {pc}
      , 0xe3a03a07 -- +0x10: mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe1520003 -- cmp r2, r3
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x00200014 "return-conditional" Nothing)
  , Fixture
      "a bound returned in r3 to a caller that branches through it is not discharged"
      0x00210000
      [ 0xe92d4000 -- push {lr}
      , 0xeb000001 -- bl +0x10
      , 0xe12fff13 -- bx r3
      , 0xe8bd8000 -- pop {pc}
      , 0xe3a03a07 -- +0x10: mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe1520003 -- cmp r2, r3
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x00210014 "return-conditional" Nothing)
  , Fixture
      "the bound used as a load address stays open"
      0x00150000
      (bound [0xe5931000, 0xe12fff1e]) -- ldr r1, [r3]; bx lr
      (ConsumerAt 0x00150008 "open" (Just "address-use"))
  , Fixture
      "the bound used as an indirect transfer target stays open"
      0x00160000
      (bound [0xe12fff13]) -- bx r3
      (ConsumerAt 0x00160008 "open" (Just "control-use"))
  , Fixture
      "the bound stored to memory stays open"
      0x00170000
      (bound [0xe52d3004, 0xe12fff1e]) -- str r3, [sp, #-4]!; bx lr
      (ConsumerAt 0x00170008 "open" (Just "stored-use"))
  , Fixture
      "the bound copied to another register stays open"
      0x00180000
      (bound [0xe1a00003, 0xe12fff1e]) -- mov r0, r3; bx lr
      (ConsumerAt 0x00180008 "open" (Just "derived-use"))
  , Fixture
      "the bound live across a call stays open"
      0x00190000
      (bound [0xeb000000, 0xe12fff1e, 0xe12fff1e]) -- bl +0x20; bx lr; +0x20: bx lr
      (ConsumerAt 0x00190008 "open" (Just "live-across-call"))
  , Fixture
      "the bound live at an indirect jump stays open"
      0x001a0000
      (bound [0xe12fff11]) -- bx r1
      (ConsumerAt 0x001a0008 "open" (Just "live-at-indirect-jump"))
  , Fixture
      "a bound returned in r0 stays open"
      0x001b0000
      [ 0xe5902000 -- ldr r2, [r0]
      , 0xe3a00a07 -- mov r0, #0x7000
      , 0xe340007b -- movt r0, #0x7b
      , 0xe1520000 -- cmp r2, r0
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x001b0008 "open" (Just "live-at-return"))
  , Fixture
      "a compare loop that keeps the bound live until it is overwritten is discharged"
      0x001c0000
      [ 0xe3a03a07 -- mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe4902004 -- ldr r2, [r0], #4
      , 0xe1520003 -- cmp r2, r3
      , 0x9afffffc -- bls -0x08
      , 0xe3a03000 -- mov r3, #0
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x001c0004 "discharged" Nothing)
  , Fixture
      "control leaving the images with the bound live stays open"
      0x001d0000
      [ 0xe3a03a07 -- mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe1520003 -- cmp r2, r3
      ]
      (ConsumerAt 0x001d0004 "open" (Just "leaves-images"))
  , Fixture
      "a literal-pool bound used as a load address stays open"
      0x001e0000
      [ 0xe59f3004 -- ldr r3, [pc, #4]
      , 0xe5931000 -- ldr r1, [r3]
      , 0xe12fff1e -- bx lr
      , 0x007b7000 -- literal
      ]
      (ConsumerAt 0x001e0000 "open" (Just "address-use"))
  , Fixture
      "an aligned word naming the stream stays open"
      0x001f0000
      [ 0xe12fff1e -- bx lr
      , 0x007b7000 -- data word
      ]
      (ConsumerAt 0x001f0004 "open" (Just "held-by-object"))
  ]

main :: IO ()
main = do
  let regions = [Region (fixtureName f) StockImage (fixtureBase f) (littleEndian (fixtureWords f)) | f <- fixtures]
      proof = proofValue (buildProof (Inputs regions [] []))
      obligations = array "obligations" proof
      consumers =
        [ consumer
        | declared <- maybe [] (array "objects") (field "declared_objects" proof)
        , consumer <- array "consumers" declared
        ]
      satisfied expect = case expect of
        ObligationAt kind site ->
          any (\o -> field "kind" o == Just (text kind) && field "site" o == Just (text (hex site))) obligations
        ConsumerAt site status reason ->
          any
            ( \c ->
                field "site" c == Just (text (hex site))
                  && field "status" c == Just (text status)
                  && (field "kind" =<< field "reason" c) == fmap text reason
            )
            consumers
  results <- forM fixtures $ \f -> do
    let ok = satisfied (fixtureExpect f)
    putStrLn ((if ok then "pass: " else "FAIL: ") ++ fixtureName f)
    pure ok
  unless (and results) exitFailure

field :: T.Text -> Value -> Maybe Value
field key (Object fields) = KM.lookup (K.fromText key) fields
field _ _ = Nothing

array :: T.Text -> Value -> [Value]
array key value = case field key value of
  Just (Array items) -> V.toList items
  _ -> []

text :: String -> Value
text = String . T.pack

littleEndian :: [Word32] -> B.ByteString
littleEndian = B.pack . concatMap (\w -> [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]])

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
