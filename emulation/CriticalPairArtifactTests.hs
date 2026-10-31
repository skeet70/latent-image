module Main where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft, isRight)
import Data.Word (Word32)
import Emulation.CriticalPair (deriveLinkedArtifact)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

data Boundary = Entry | Exit deriving (Show)
data Admission = Admit | Refuse deriving (Eq)
newtype CaseName = CaseName String
newtype Program = Program [Word32]
data Scenario = Scenario CaseName Admission (Boundary -> Program)
data Artifact = Artifact B.ByteString B.ByteString

main :: IO ()
main = do
  results <- forM [Entry,Exit] $ \boundary -> forM scenarios $ \(Scenario (CaseName name) expected program) -> do
    let Artifact image symbols = artifact boundary (program boundary)
        actual = deriveLinkedArtifact image symbols
        passed = case expected of
          Admit -> isRight actual
          Refuse -> isLeft actual
    putStrLn (show boundary ++ ": " ++ name ++ ": " ++ if passed then "passed" else "FAILED")
    pure passed
  unless (and (concat results)) (fail "critical-pair artifact admission regressions failed")

scenarios :: [Scenario]
scenarios =
  [ Scenario (CaseName "preserved caller frame and defined Scratch field access") Admit (const validControl)
  , Scenario (CaseName "stack-derived EOR cannot hide a saved-return overwrite") Refuse (const (Program
      [ 0xe92d4000 -- push {lr}
      , 0xe1a0000d -- mov r0, sp
      , 0xe2200000 -- eor r0, r0, #0
      , 0xe3a01000 -- mov r1, #0
      , 0xe5801000 -- str r1, [r0]
      , 0xe8bd8000 -- pop {pc}
      ]))
  , Scenario (CaseName "Scratch store cannot overwrite the adapter saved frame") Refuse (const (Program
      [0xe5800020,0xe12fff1e])) -- str r0, [r0, #32]; bx lr
  , Scenario (CaseName "Scratch padding is outside the defined writable fields") Refuse (const (Program
      [0xe580001c,0xe12fff1e])) -- str r0, [r0, #28]; bx lr
  , Scenario (CaseName "load writeback cannot retain the caller r9 value") Refuse (const (Program
      [0xe4990004,0xe12fff1e])) -- ldr r0, [r9], #4; bx lr
  , Scenario (CaseName "store writeback cannot retain the caller r9 value") Refuse (const (Program
      [0xe4890004,0xe12fff1e])) -- str r0, [r9], #4; bx lr
  , Scenario (CaseName "bridge call cannot dispatch to an unbound target") Refuse (\boundary -> Program
      [0xe92d4010,0xe3a01000,unboundCall boundary,0xe8bd8010])
  , Scenario (CaseName "bridge call requires the adapter Scratch argument") Refuse (\boundary -> Program
      [0xe92d4010,0xe3a00000,0xe3011304,0xe340100d,wrongScratchCall boundary,0xe8bd8010])
  ]

validControl :: Program
validControl = Program
  [ 0xe92d4010 -- push {r4, lr}
  , 0xe1a04000 -- mov r4, r0
  , 0xe5941018 -- ldr r1, [r4, #24]
  , 0xe5841018 -- str r1, [r4, #24]
  , 0xe8bd8010 -- pop {r4, pc}
  ]

unboundCall :: Boundary -> Word32
unboundCall Entry = 0xebffffe7
unboundCall Exit = 0xebffffa7

wrongScratchCall :: Boundary -> Word32
wrongScratchCall Entry = 0xebffffe5
wrongScratchCall Exit = 0xebffffa5

-- Adapter bodies and symbol names encode the external A32 artifact ABI.
artifact :: Boundary -> Program -> Artifact
artifact boundary candidate =
  let enterBody = case boundary of Entry -> candidate; Exit -> validControl
      exitBody = case boundary of Entry -> validControl; Exit -> candidate
      Program enterWords = enterBody
      Program exitWords = exitBody
      image = B.concat (map U.putWord32 (entryAdapter ++ exitAdapter ++ bridge
        ++ replicate 10 0xe320f000
        ++ enterWords ++ replicate (64 - length enterWords) 0xe320f000
        ++ exitWords ++ replicate (64 - length exitWords) 0xe320f000))
      row address size name = unwords [showHex (address :: Word32) "",showHex (size :: Int) "","T",name]
      symbols = C.pack (unlines
        [ row 0x10400 0 "__secondary_stage_start"
        , row 0x10700 0 "__secondary_stage_end"
        , row 0x10400 112 "xm1_critical_enter_adapter"
        , row 0x10470 60 "xm1_critical_exit_adapter"
        , row 0x104ac 44 "xm1_critical_retained_call"
        , row 0x10500 (4 * length enterWords) "xm1_critical_enter_continue"
        , row 0x10600 (4 * length exitWords) "xm1_critical_exit"
        ])
   in Artifact image symbols

entryAdapter :: [Word32]
entryAdapter =
  [ 0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000
  , 0xe3013618,0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000
  , 0xe24dd020,0xe88d100f,0xe58de014,0xe10f2000,0xe58d2018
  , 0xe1a0000d,0xe1a01004,0xe1a0200a,0xeb000029,0xe59d2018,0xe12cf002,0xe89d100f
  , 0xe59de014,0xe28dd020,0xe8bd8df0
  ]

exitAdapter :: [Word32]
exitAdapter =
  [ 0xe92d4878,0xe28db014,0xe24dd020,0xe88d100f,0xe58de014
  , 0xe10f2000,0xe58d2018,0xe1a0000d,0xeb00005a,0xe59d2018,0xe12cf002,0xe89d100f
  , 0xe59de014,0xe28dd020,0xe8bd8878
  ]

bridge :: [Word32]
bridge =
  [ 0xe92d4070,0xe1a04000,0xe1a05001,0xe594e018,0xe12cf00e
  , 0xe894100f,0xe12fff35,0xe884100f,0xe10f0000,0xe5840018,0xe8bd8070
  ]
