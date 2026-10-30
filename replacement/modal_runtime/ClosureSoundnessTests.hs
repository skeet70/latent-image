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
  | -- | No declared-object consumer at this site is discharged.
    UndischargedAt Word32
  | -- | A return-conditional consumer at this site whose premise has this
    -- kind, whose continuation witness has this outcome, and whose witness
    -- reports each site in its role.
    WitnessAt Word32 String String [(Role, Word32)]

data Role = Caller | Overwrite | ReadBeforeWrite | Unresolved | NonCallEntry

-- | A function that calls +0x20 from +0x04 and continues with
-- `continuationWords`, which fill +0x08 to +0x1f.
caller :: [Word32] -> [Word32]
caller continuationWords =
  [ 0xe92d4800 -- push {r11, lr}
  , 0xeb000005 -- bl +0x20
  ]
    ++ continuationWords

-- | A framed callee whose compare-only bound stays in r3 at its return on
-- the not-higher path; the bound's MOVT is at +0x0c.
framedBound :: [Word32]
framedBound =
  [ 0xe92d4800 -- push {r11, lr}
  , 0xe5902000 -- ldr r2, [r0]
  , 0xe3a03a07 -- mov r3, #0x7000
  , 0xe340307b -- movt r3, #0x7b
  , 0xe1520003 -- cmp r2, r3
  , 0x83a00000 -- movhi r0, #0
  , 0xe8bd8800 -- pop {r11, pc}
  ]

-- | The only caller tests r0 and writes r3 on both successors.
overwritingCaller :: [Word32]
overwritingCaller =
  caller
    [ 0xe3500000 -- cmp r0, #0
    , 0x0a000001 -- beq +0x18
    , 0xe3003000 -- movw r3, #0
    , 0xe8bd8800 -- pop {r11, pc}
    , 0xe3013234 -- +0x18: movw r3, #0x1234
    , 0xe8bd8800 -- pop {r11, pc}
    ]
    ++ framedBound

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
      "a callee that changes r4 leaves a controller id held in r4 unresolved"
      0x00bf0000
      [ 0xe3a04003 -- mov r4, #3
      , 0xeb000002 -- bl +0x14
      , 0xe1a00004 -- mov r0, r4
      , 0xeb002c05 -- bl 0x00bfb028
      , 0xe12fff1e -- bx lr
      , 0xe3a040c8 -- +0x14: mov r4, #200
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "caller-controller-unresolved" 0x00bf000c)
  , Fixture
      "a callee that changes r4 leaves a state value held in r4 unresolved"
      0x00240000
      [ 0xe3a04001 -- mov r4, #1
      , 0xeb000003 -- bl +0x18
      , 0xe3075588 -- movw r5, #0x7588
      , 0xe3405146 -- movt r5, #0x0146
      , 0xe1c540b0 -- strh r4, [r5]
      , 0xe12fff1e -- bx lr
      , 0xe3a04009 -- +0x18: mov r4, #9
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "store-value-unresolved" 0x00240010)
  , Fixture
      "an array pointer held across a call is still reported when stored through"
      0x00250000
      [ 0xe3075588 -- movw r5, #0x7588
      , 0xe3405146 -- movt r5, #0x0146
      , 0xeb000002 -- bl +0x18
      , 0xe3a04002 -- mov r4, #2
      , 0xe1c540b0 -- strh r4, [r5]
      , 0xe12fff1e -- bx lr
      , 0xe12fff1e -- +0x18: bx lr
      ]
      (ObligationAt "computed-index-store" 0x00250010)
  , Fixture
      "an array pointer in sp is passed to a helper that stores through sp"
      0x00260000
      [ 0xe307d588 -- movw sp, #0x7588
      , 0xe340d146 -- movt sp, #0x0146
      , 0xeb000001 -- bl +0x14
      , 0xe3a0d000 -- mov sp, #0
      , 0xe12fff1e -- bx lr
      , 0xe1cd00b0 -- +0x14: strh r0, [sp]
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "pointer-escape" 0x00260008)
  , Fixture
      "an array pointer in lr is a transfer target, not a return"
      0x00270000
      [ 0xe307e588 -- movw lr, #0x7588
      , 0xe340e146 -- movt lr, #0x0146
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "pointer-escape" 0x00270008)
  , Fixture
      "an array pointer in lr used as a blx target escapes though lr is then overwritten"
      0x00280000
      [ 0xe307e588 -- movw lr, #0x7588
      , 0xe340e146 -- movt lr, #0x0146
      , 0xe12fff3e -- blx lr
      , 0xe3a0e000 -- mov lr, #0
      , 0xe12fff1e -- bx lr
      ]
      (ObligationAt "pointer-escape" 0x00280008)
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
      "a bound returned in r3 to a caller that loads through it reports the read and is not discharged"
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
      (WitnessAt 0x00200014 "return-continuation-does-not-read" "continuation-reads" [(Caller, 0x00200004), (ReadBeforeWrite, 0x00200008)])
  , Fixture
      "a bound returned in r3 to a caller that branches through it reports the read and is not discharged"
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
      (WitnessAt 0x00210014 "return-continuation-does-not-read" "continuation-reads" [(Caller, 0x00210004), (ReadBeforeWrite, 0x00210008)])
  , Fixture
      "a bound returned in r3 to a sole caller that overwrites r3 on both successors narrows the premise and stays return-conditional"
      0x00300000
      overwritingCaller
      (WitnessAt 0x0030002c "direct-continuations-overwrite" "direct-continuations-overwrite" [(Caller, 0x00300004), (Overwrite, 0x00300010), (Overwrite, 0x00300018)])
  , Fixture
      "an aligned word naming the callee keeps the premise unnarrowed though its caller overwrites r3"
      0x00310000
      (overwritingCaller ++ [0x00310020]) -- data word naming the callee
      (WitnessAt 0x0031002c "return-continuation-does-not-read" "not-narrowed" [(Caller, 0x00310004), (Overwrite, 0x00310010), (NonCallEntry, 0x0031003c)])
  , Fixture
      "a callee reached only by a tail-call branch keeps the premise unnarrowed"
      0x00320000
      ( framedBound
          ++ [ 0xeafffff7 -- +0x1c: b +0x00
             , 0xe92d4800 -- push {r11, lr}
             , 0xebfffffc -- bl +0x1c
             , 0xe3003000 -- movw r3, #0
             , 0xe8bd8800 -- pop {r11, pc}
             ]
      )
      (WitnessAt 0x0032000c "return-continuation-does-not-read" "not-narrowed" [(NonCallEntry, 0x0032001c)])
  , Fixture
      "a caller continuation that calls or jumps indirectly with r3 live stays unresolved"
      0x00330000
      ( caller
          [ 0xe3500000 -- cmp r0, #0
          , 0x0a000001 -- beq +0x18
          , 0xeb000009 -- bl +0x3c
          , 0xe8bd8800 -- pop {r11, pc}
          , 0xe12fff11 -- +0x18: bx r1
          , 0xe12fff1e -- bx lr
          ]
          ++ framedBound
          ++ [0xe12fff1e] -- +0x3c: bx lr
      )
      (WitnessAt 0x0033002c "return-continuation-does-not-read" "not-narrowed" [(Caller, 0x00330004), (Unresolved, 0x00330010), (Unresolved, 0x00330018)])
  , Fixture
      "a return through lr after a call in the callee keeps the premise unnarrowed though its caller overwrites r3"
      0x00340000
      [ 0xe92d4800 -- push {r11, lr}
      , 0xeb000003 -- bl +0x18
      , 0xe3003000 -- movw r3, #0
      , 0xe8bd8800 -- pop {r11, pc}
      , 0xe12fff1e -- +0x10: bx lr
      , 0xe12fff1e -- bx lr
      , 0xe92d4000 -- +0x18: push {lr}
      , 0xebfffffb -- bl +0x10
      , 0xe5902000 -- ldr r2, [r0]
      , 0xe3a03a07 -- mov r3, #0x7000
      , 0xe340307b -- movt r3, #0x7b
      , 0xe1520003 -- cmp r2, r3
      , 0xe12fff1e -- bx lr
      ]
      (WitnessAt 0x00340028 "return-continuation-does-not-read" "not-narrowed" [(Caller, 0x00340004), (Overwrite, 0x00340008), (Unresolved, 0x0034001c)])
  , Fixture
      "a caller continuation that can loop with r3 live stays unresolved though its exit overwrites r3"
      0x00350000
      ( caller
          [ 0xe3500000 -- cmp r0, #0
          , 0x1afffffe -- +0x0c: bne +0x0c
          , 0xe3003000 -- movw r3, #0
          , 0xe8bd8800 -- pop {r11, pc}
          , 0xe12fff1e -- bx lr
          , 0xe12fff1e -- bx lr
          ]
          ++ framedBound
      )
      (WitnessAt 0x0035002c "return-continuation-does-not-read" "not-narrowed" [(Caller, 0x00350004), (Overwrite, 0x00350010), (Unresolved, 0x0035000c)])
  , Fixture
      "caller paths that rejoin with r3 live and then overwrite it narrow the premise"
      0x00360000
      ( [ 0xe92d4800 -- push {r11, lr}
        , 0xeb000007 -- bl +0x28
        , 0xe3500000 -- cmp r0, #0
        , 0x0a000001 -- beq +0x18
        , 0xe3a01001 -- mov r1, #1
        , 0xea000000 -- b +0x1c
        , 0xe3a01002 -- +0x18: mov r1, #2
        , 0xe3003000 -- +0x1c: movw r3, #0
        , 0xe8bd8800 -- pop {r11, pc}
        , 0xe12fff1e -- bx lr
        ]
          ++ framedBound -- +0x28
      )
      (WitnessAt 0x00360034 "direct-continuations-overwrite" "direct-continuations-overwrite" [(Caller, 0x00360004), (Overwrite, 0x0036001c)])
  , Fixture
      "a branch to an alias of the callee keeps the premise unnarrowed though its caller overwrites r3"
      0x00370000
      overwritingCaller
      (WitnessAt 0x0037002c "return-continuation-does-not-read" "not-narrowed" [(Caller, 0x00370004), (Overwrite, 0x00370010), (NonCallEntry, 0x40370100)])
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
  , Fixture
      "an ORR completing a stream address after a data-word entry stays enumerated and open"
      0x002a0000
      [ 0xe3a0387b -- mov r3, #0x7b0000
      , 0xe3833a07 -- orr r3, r3, #0x7000
      , 0xe5931000 -- ldr r1, [r3]
      , 0xe12fff1e -- bx lr
      , 0x002a0004 -- data word naming the orr
      ]
      (ConsumerAt 0x002a0004 "open" (Just "address-use"))
  , Fixture
      "an ORR after a call does not combine the constant built before it"
      0x002b0000
      [ 0xe3a0387b -- mov r3, #0x7b0000
      , 0xeb000003 -- bl +0x18
      , 0xe3833a07 -- orr r3, r3, #0x7000
      , 0xe1520003 -- cmp r2, r3
      , 0xe3a03000 -- mov r3, #0
      , 0xe12fff1e -- bx lr
      , 0xe12fff1e -- +0x18: bx lr
      ]
      (UndischargedAt 0x002b0008)
  , Fixture
      "a construction naming the stream on one conditional path is enumerated and open"
      0x002c0000
      [ 0xe3520000 -- cmp r2, #0
      , 0x13a0387b -- movne r3, #0x7b0000
      , 0xe3833a07 -- orr r3, r3, #0x7000
      , 0xe5931000 -- ldr r1, [r3]
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x002c0008 "open" (Just "address-use"))
  , Fixture
      "an MVN/SUB-built bound used as a branch target stays open"
      0x002d0000
      [ 0xe3e034ff -- mvn r3, #0xff000000
      , 0xe2433884 -- sub r3, r3, #0x840000
      , 0xe1520003 -- cmp r2, r3
      , 0x812fff13 -- bxhi r3
      , 0xe3a03000 -- mov r3, #0
      , 0xe12fff1e -- bx lr
      ]
      (ConsumerAt 0x002d0004 "open" (Just "control-use"))
  ]

-- | Words a fixture relies on that lie outside its own region.
detached :: [(String, Word32, [Word32])]
detached =
  [("alias branch into the 0x00370000 callee", 0x40370100, [0xeaffffc6])] -- b 0x40370020

main :: IO ()
main = do
  let regions =
        [Region (fixtureName f) StockImage (fixtureBase f) (littleEndian (fixtureWords f)) | f <- fixtures]
          ++ [Region name StockImage base (littleEndian ws) | (name, base, ws) <- detached]
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
        UndischargedAt site ->
          not (any (\c -> field "site" c == Just (text (hex site)) && field "status" c == Just (text "discharged")) consumers)
        WitnessAt site premise outcome roles ->
          any
            ( \c ->
                field "site" c == Just (text (hex site))
                  && field "status" c == Just (text "return-conditional")
                  && (field "kind" =<< field "premise" c) == Just (text premise)
                  && (field "outcome" =<< field "continuation_witness" c) == Just (text outcome)
                  && all (\(role, at) -> text (hex at) `elem` roleSites role (maybe Null id (field "continuation_witness" c))) roles
            )
            consumers
  results <- forM fixtures $ \f -> do
    let ok = satisfied (fixtureExpect f)
    putStrLn ((if ok then "pass: " else "FAIL: ") ++ fixtureName f)
    pure ok
  unless (and results) exitFailure

-- | The sites a continuation witness reports in `role`.
roleSites :: Role -> Value -> [Value]
roleSites role witness = case role of
  Caller -> concatMap (sitesAt "site") continuations
  Overwrite -> concatMap (array "overwritten") continuations
  ReadBeforeWrite -> concatMap (array "read") continuations
  Unresolved -> concatMap (sitesAt "at") (concatMap (array "unresolved") continuations ++ array "limits" witness)
  NonCallEntry -> concatMap (\entry -> sitesAt "site" entry ++ sitesAt "at" entry) (array "non_call_entries" witness)
 where
  continuations = array "callers" witness
  sitesAt key value = maybe [] pure (field key value)

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
