{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import Data.Word (Word32)
import StockInventory

bytes :: [Word32] -> B.ByteString
bytes = B.pack . concatMap (\word -> [fromIntegral (word `shiftR` shift) | shift <- [0,8,16,24]])

check :: String -> Bool -> IO ()
check label accepted = unless accepted (fail label)

derive :: [Word32] -> Either String (Word32, Int)
derive = deriveWith []

deriveWith :: [Word32] -> [Word32] -> Either String (Word32, Int)
deriveWith tails words' = do
  result <- deriveResult tails words'
  let FunctionDeclaration (GuestAddress address) (FunctionSize size) = derivedDeclaration result
  pure (address, size)

deriveResult :: [Word32] -> [Word32] -> Either String FunctionDerivation
deriveResult tails words' = deriveFunction [] (GuestAddress 0x1000) (bytes words')
  (FunctionWindow (GuestAddress 0x1000) (GuestAddress (0x1000 + 4 * fromIntegral (length words'))))
  (map GuestAddress tails)

refused :: Either failure a -> Bool
refused (Left _) = True
refused (Right _) = False

retained :: [Word32] -> Either FootprintError CodeFootprint
retained words' = deriveRetainedFootprint (GuestAddress 0x1000) (bytes words') (GuestAddress 0x1000)
footprint :: [Word32] -> [Word32] -> [Word32] -> [Word32] -> CodeFootprint
footprint code literals unresolved dependencies = CodeFootprint (map GuestAddress code) (map GuestAddress literals)
  (map GuestAddress unresolved) (map GuestAddress dependencies)

main :: IO ()
main = do
  check "a return is code" (derive [0xe12fff1e] == Right (0x1000,4))
  check "a referenced trailing literal is not executable, even when it encodes a return"
    (derive [0xe59f0000,0xe12fff1e,0xe12fff1e] == Right (0x1000,8))
  check "an error tail after a conditional return remains code"
    (deriveWith [0x1100] [0xe3500000,0x012fff1e,0xea00003c] == Right (0x1000,12))
  check "all switch targets are code and table words are data"
    (derive [0xe3500001,0x979ff100,0xe12fff1e,0x1014,0x1018,0xe3a00000,0xe12fff1e] == Right (0x1000,28))
  check "unreferenced trailing words refuse rather than widening code"
    (refused (derive [0xe12fff1e,0x12345678]))
  check "unreachable instruction-looking words refuse"
    (refused (derive [0xea000000,0xe1a00000,0xe12fff1e]))
  check "a dispatch without an immediate dominating bound refuses"
    (refused (derive [0x979ff100,0xe12fff1e,0x1004]))
  check "a literal outside the inspected function window refuses"
    (refused (derive [0xe59f0000,0xe12fff1e]))
  check "an unsupported reachable instruction refuses"
    (refused (derive [0xe710f110,0xe12fff1e]))
  check "MOVW with PC as destination refuses"
    (refused (derive [0xe300f000,0xe12fff1e]))
  check "MOVT with PC as destination refuses"
    (refused (derive [0xe340f000,0xe12fff1e]))
  check "a stack-loaded local target is not an incoming return"
    (refused (derive [0xe59f100c,0xe59f0004,0xe52d0004,0xe8bd8000,0x00001014,0xef000000]))
  check "a saved incoming LR remains a return"
    (derive [0xe92d4000,0xe8bd8000] == Right (0x1000,8))
  check "a different allocated frame word does not invalidate the saved return"
    (derive [0xe92d4000,0xe24dd004,0xe58d0000,0xe28dd004,0xe8bd8000] == Right (0x1000,20))
  check "the frame pointer can restore the incoming return slot"
    (derive [0xe92d4800,0xe28db004,0xe24dd008,0xe24bd004,0xe8bd8800] == Right (0x1000,20))
  -- The unknown incoming r0 could alias the saved LR without UnexposedFrameIsolation.
  underPremise <- either fail pure (deriveFunction [] (GuestAddress 0x1000) (bytes [0xe92d4000,0xeb0003fd,0xe8bd8000])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x100c)) [])
  check "an unknown-pointer call is accepted only under the named isolation premise"
    (derivedDeclaration underPremise == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 12)
      && map returnPremiseLabel (derivationPremises underPremise) == ["unexposed-caller-frame-isolation"])
  check "a frame alias store invalidates the saved return"
    (refused (derive [0xe92d4000,0xe1a0100d,0xe5810000,0xe8bd8000]))
  check "an unknown pointer store can alias the saved return"
    (refused (derive [0xe92d4000,0xe5810000,0xe8bd8000]))
  check "a partial store invalidates a saved return word"
    (refused (derive [0xe92d4000,0xe5cd0000,0xe8bd8000]))
  check "a conditional overwrite is not a proven return"
    (refused (derive [0xe92d4000,0x058d0000,0xe8bd8000]))
  check "an overwritten LR is not an incoming register return"
    (refused (derive [0xe3a0e000,0xe12fff1e]))
  check "an unsaved LR after a call is not the incoming return"
    (refused (derive [0xeb0003fe,0xe12fff1e]))
  -- Writing through r0+4 in the callee would violate the saved-register isolation premise.
  exposedPremise <- either fail pure (deriveResult [] [0xe92d4010,0xe1a0000d,0xeb0003fc,0xe8bd8010])
  check "an exposed saved-frame pointer is accepted under the named isolation premise"
    (derivedDeclaration exposedPremise == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 16)
      && map returnPremiseLabel (derivationPremises exposedPremise) == ["unexposed-caller-frame-isolation"])
  check "a stored frame address invalidates exposed local return copies"
    (refused (derive [0xe92d4000,0xe24dd008,0xe58de000,0xe28d0004,0xe58d0004,0xe3a00000,0xeb0003f8,0xe8bd8000]))
  check "a previously exposed local copy is invalidated after restoration"
    (refused (derive [0xe92d4010,0xe1a0400e,0xe24dd004,0xe1a0000d,0xeb0003fa,0xe58d4000,0xe3a00000,0xeb0003f7,0xe8bd8000]))
  check "a conditional local address remains possible at a call"
    (refused (derive [0xe92d4000,0xe24dd004,0xe58de000,0xe3a00000,0x01a0000d,0xeb0003f9,0xe8bd8000]))
  check "a transformed local address invalidates its return copy at a call"
    (refused (derive [0xe92d4000,0xe24dd004,0xe58de000,0xe02d0001,0xeb0003fa,0xe8bd8000]))
  literalGlobal <- either fail pure (deriveResult [] [0xe92d4000,0xe59f0004,0xe5801000,0xe8bd8000,0x3000])
  check "a literal global store is accepted under global-object separation"
    (derivedDeclaration literalGlobal == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 16)
      && map returnPremiseLabel (derivationPremises literalGlobal) == ["global-object-stack-separation"]
      && derivationPremiseUses literalGlobal == [PremiseUse (ProgramCounter 0x1008) GlobalObjectStackSeparation])
  indexedGlobal <- either fail pure (deriveResult [] [0xe92d4000,0xe3032000,0xe3a03004,0xe0222093,0xe5821000,0xe8bd8000])
  check "an indexed global store is accepted under global-object separation"
    (derivedDeclaration indexedGlobal == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24)
      && map returnPremiseLabel (derivationPremises indexedGlobal) == ["global-object-stack-separation"])
  let globalStore result use = derivationPremises result == [GlobalObjectStackSeparation]
        && derivationPremiseUses result == [PremiseUse (ProgramCounter use) GlobalObjectStackSeparation]
  shiftedFirst <- either fail pure (deriveResult [] [0xe92d4000,0xe3033000,0xe1a02200,0xe0823003,0xe583100c,0xe8bd8000])
  check "a shifted index added to a register global base is accepted under global-object separation"
    (derivedDeclaration shiftedFirst == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && globalStore shiftedFirst 0x1010)
  indexBase <- either fail pure (deriveResult [] [0xe92d4000,0xe3032000,0xe1a05100,0xe7851002,0xe8bd8000])
  check "a shifted index base with a global offset register is accepted under global-object separation"
    (derivedDeclaration indexBase == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 20) && globalStore indexBase 0x100c)
  registerShift <- either fail pure (deriveResult [] [0xe92d4000,0xe3032000,0xe3a0c002,0xe1a01c10,0xe7813002,0xe8bd8000])
  check "a known register shift amount scales an index"
    (derivedDeclaration registerShift == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && globalStore registerShift 0x1010)
  check "an unscaled unknown pointer plus a register constant can alias the saved return"
    (refused (derive [0xe92d4000,0xe3033000,0xe0803003,0xe5831000,0xe8bd8000]))
  check "a shifted frame address plus a constant can alias the saved return"
    (refused (derive [0xe92d4000,0xe3032000,0xe1a0108d,0xe0811002,0xe5810000,0xe8bd8000]))
  check "an unknown register shift amount may leave a pointer unscaled"
    (refused (derive [0xe92d4000,0xe3033000,0xe1a01c10,0xe0811003,0xe5810000,0xe8bd8000]))
  check "a register shift amount with a zero low byte leaves a pointer unscaled"
    (refused (derive [0xe92d4000,0xe3033000,0xe3a0cc01,0xe1a01c10,0xe0811003,0xe5810000,0xe8bd8000]))
  check "a register shift amount of 32 or more leaves a pointer unknown"
    (refused (derive [0xe92d4000,0xe3033000,0xe3a0c020,0xe1a01c10,0xe0811003,0xe5810000,0xe8bd8000]))
  check "a logical right shift does not scale a pointer into an index"
    (refused (derive [0xe92d4000,0xe3033000,0xe1a01120,0xe0811003,0xe5810000,0xe8bd8000]))
  check "an arithmetic right shift does not scale a pointer into an index"
    (refused (derive [0xe92d4000,0xe3033000,0xe1a01140,0xe0811003,0xe5810000,0xe8bd8000]))
  check "a re-aligned pointer loaded from an exposed frame can alias the saved return"
    (refused (derive [0xe92d4000,0xe24dd004,0xe3033000,0xe583d000,0xe5930000,0xe1a00120,0xe1a00100,0xe5801004,0xe28dd004,0xe8bd8000]))
  check "a multiplied pointer loaded from an exposed frame can alias the saved return"
    (refused (derive [0xe92d4000,0xe24dd004,0xe3033000,0xe583d000,0xe5930000,0xe1a00120,0xe3a02004,0xe0000290,0xe5801004,0xe28dd004,0xe8bd8000]))
  check "a constant base plus a pointer loaded from an exposed frame can alias the saved return"
    (refused (derive [0xe92d4000,0xe24dd004,0xe3033000,0xe583d000,0xe5930000,0xe3003000,0xe0830000,0xe5801004,0xe28dd004,0xe8bd8000]))
  check "a constant base register offset by a pointer loaded from an exposed frame can alias the saved return"
    (refused (derive [0xe92d4000,0xe24dd004,0xe3033000,0xe583d000,0xe5930000,0xe3003004,0xe7831000,0xe28dd004,0xe8bd8000]))
  returnedObject <- either fail pure (deriveResult [] [0xe92d4000,0xeb0003fd,0xe5801004,0xe8bd8000])
  check "a returned-object store is accepted under frame isolation"
    (derivedDeclaration returnedObject == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 16)
      && map returnPremiseLabel (derivationPremises returnedObject) == ["unexposed-caller-frame-isolation"])
  check "returned objects can still alias exposed local return copies"
    (refused (derive [0xe92d4010,0xe1a0400e,0xe24dd004,0xe1a0000d,0xeb0003fa,0xe58d4000,0xe5801000,0xe8bd8000]))
  conditionalObject <- either fail pure (deriveResult [] [0xe92d4000,0xeb0003fd,0x03a02004,0x07801002,0xe8bd8000])
  check "a conditional returned-object index retains its isolation premise"
    (derivedDeclaration conditionalObject == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 20)
      && map returnPremiseLabel (derivationPremises conditionalObject) == ["unexposed-caller-frame-isolation"])
  check "a reused local word is not protected by an earlier saved-register lifetime"
    (refused (derive [0xe92d4000,0xe8bd4000,0xe24dd004,0xe58de000,0xe1a0000d,0xeb0003f9,0xe8bd8000]))
  check "a shortened window cannot silently discard its omitted epilogue"
    (refused (deriveFunction [] (GuestAddress 0x1000) (bytes [0xea000000,0xe1a00000,0xe12fff1e])
      (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1004)) []))
  check "an undeclared external tail refuses"
    (refused (derive [0xea00003e]))
  check "a permitted external destination still requires ARM alignment"
    (refused (deriveWith [0x1101] [0x159ff000,0xe12fff1e,0x1101]))
  check "a branch into a dispatch cannot bypass its adjacent comparison"
    (refused (derive [0x0a000001,0xe1a00000,0xe3500001,0x979ff100,0xe12fff1e,0x101c,0x1020,0xe3a00000,0xe12fff1e]))
  derived <- either fail pure (derive [0xe59f0000,0xe12fff1e,0x12345678])
  let declaration (address, size) = FunctionDeclaration (GuestAddress address) (FunctionSize size)
      compareExtent size = checkDeclarations [FunctionDeclaration (GuestAddress 0x1000) (FunctionSize size)] [declaration derived]
  check "the production comparison accepts the independently specified code extent" (compareExtent 8 == Right ())
  check "the production comparison rejects a shortened declaration" (refused (compareExtent 4))
  check "the production comparison rejects a declaration that includes the literal" (refused (compareExtent 12))
  check "retained footprints do not widen across unused trailing bytes"
    (retained [0xe12fff1e,0x12345678] == Right (footprint [0x1000] [] [] []))
  check "retained literal words are data even when they encode a return"
    (retained [0xe59f0000,0xe12fff1e,0xe12fff1e] == Right (footprint [0x1000,0x1004] [0x1008] [] []))
  check "a retained instruction can also be read as data"
    (retained [0xe51f0004,0xe12fff1e] == Right (footprint [0x1000,0x1004] [0x1004] [] []))
  check "an unresolved retained register tail stays explicit"
    (retained [0xe12fff13] == Right (footprint [0x1000] [] [0x1000] []))
  check "an unresolved retained call preserves its caller continuation"
    (retained [0xe12fff33,0xe12fff1e] == Right (footprint [0x1000,0x1004] [] [0x1000] []))
  check "an external retained call publishes its source dependency"
    (retained [0xeb00003e,0xe12fff1e] == Right (footprint [0x1000,0x1004] [] [] [0x1100]))
  check "an external retained tail publishes its source dependency"
    (retained [0xea00003e] == Right (footprint [0x1000] [] [] [0x1100]))
  check "retained pointer switches distinguish targets from table data"
    (retained [0xe3500001,0x979ff100,0xe12fff1e,0x1014,0x1018,0xe3a00000,0xe12fff1e]
      == Right (footprint [0x1000,0x1004,0x1008,0x1014,0x1018] [0x100c,0x1010] [] []))
  check "retained branch-table switches admit every bounded branch"
    (retained [0xe3500001,0x908ff100,0xe12fff1e,0xea000000,0xea000000,0xe12fff1e,0xe12fff1e]
      == Right (footprint [0x1000,0x1004,0x1008,0x100c,0x1010,0x1014,0x1018] [] [] []))
  check "retained unconditional switches require their upper-bound guard"
    (retained [0xe3500001,0x8a000003,0xe08ff100,0xe12fff1e,0xea000000,0xea000000,0xe12fff1e,0xe12fff1e]
      == Right (footprint [0x1000,0x1004,0x1008,0x1010,0x1014,0x1018,0x101c] [] [] []))
  check "a branch cannot bypass a retained switch comparison"
    (retained [0x0a000001,0xe1a00000,0xe3500001,0x979ff100,0xe12fff1e,0x101c,0x1020,0xe3a00000,0xe12fff1e]
      == Left (FootprintSwitchBypassed (GuestAddress 0x100c)))
  check "a retained switch cannot begin without an executed comparison"
    (refused (retained [0x979ff100,0xe12fff1e,0x1004]))
  check "retained Thumb transfers refuse without an A32 source extent"
    (retained [0xfa000000] == Left (FootprintThumb (GuestAddress 0x1000)))
  check "unclassified retained PC calculations remain explicit"
    (retained [0xe083f000] == Right (footprint [0x1000] [] [0x1000] []))
  check "a canonical postindexed stack pop is an explicit retained return"
    (retained [0xe49df004] == Right (footprint [0x1000] [] [] []))
  check "a stack-indexed dispatch remains an unresolved retained transfer"
    (retained [0xe79df000] == Right (footprint [0x1000] [] [0x1000] []))
  check "a noncanonical stack offset PC load remains unresolved"
    (retained [0xe59df004] == Right (footprint [0x1000] [] [0x1000] []))
  check "a byte PC load is not a retained word return"
    (retained [0xe4ddf004] == Right (footprint [0x1000] [] [0x1000] []))
  check "unconditional barriers remain linear retained instructions"
    (retained [0xf57ff04f,0xf57ff05f,0xf57ff06f,0xe12fff1e]
      == Right (footprint [0x1000,0x1004,0x1008,0x100c] [] [] []))
  structural <- either fail pure (inspectFunction [] (GuestAddress 0x1000) (bytes [0xe3a0e000,0xe12fff1e])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1008)) [])
  check "structural inventory retains an extent without admitting an overwritten return"
    (inspectedDeclaration structural == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 8)
      && inspectedCodeWords structural == [GuestAddress 0x1000, GuestAddress 0x1004]
      && null (inspectedLiteralWords structural)
      && refused (inspectedReturn structural))
  withLiteral <- either fail pure (inspectFunction [] (GuestAddress 0x1000) (bytes [0xe59f0000,0xe12fff1e,0x12345678])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x100c)) [])
  check "structural inventory separates a referenced literal from executable words"
    (inspectedCodeWords withLiteral == [GuestAddress 0x1000, GuestAddress 0x1004]
      && inspectedLiteralWords withLiteral == [GuestAddress 0x1008]
      && inspectedReturn withLiteral == Right [])
  check "structural inventory also refuses unclassified trailing storage"
    (refused (inspectFunction [] (GuestAddress 0x1000) (bytes [0xe12fff1e,0x12345678])
      (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1008)) []))
  let unpremised result size = derivedDeclaration result == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize size)
        && null (derivationPremises result) && null (derivationPremiseUses result)
  irqSave <- either fail pure (deriveResult [] [0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001,0xe2000080,0xe28bd000,0xe8bd0800,0xe12fff1e])
  check "an interrupt-masking control write from the read status word is accepted without premises" (unpremised irqSave 36)
  irqRestore <- either fail pure (deriveResult [] [0xe52db004,0xe28db000,0xe2000080,0xe10f1000,0xe3c11080,0xe1800001,0xe121f000,0xe28bd000,0xe8bd0800,0xe12fff1e])
  check "a caller mask confined to the I bit merges into the status word without premises" (unpremised irqRestore 40)
  serviceBody <- either fail pure (deriveResult [] [0xe92d4010,0xe10f3000,0xe3833080,0xe129f003,0xeb0003fa,0xe10f3000,0xe3c33080,0xe129f003,0xe8bd8010])
  check "flags and control writes from status words read on each side of a call keep only the call's premise"
    (derivedDeclaration serviceBody == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 36)
      && derivationPremiseUses serviceBody == [PremiseUse (ProgramCounter 0x1010) UnexposedFrameIsolation])
  flagsOnly <- either fail pure (deriveResult [] [0xe12cf000,0xe328f4f0,0xe12fff1e])
  check "status and flags field writes from an unknown register or an immediate are accepted" (unpremised flagsOnly 12)
  stackedStatus <- either fail pure (deriveResult [] [0xe92d4010,0xe24dd008,0xe10f0000,0xe58d0000,0xe59d1000,0xe121f001,0xe28dd008,0xe8bd8010])
  check "a status word reloaded from its private frame word is accepted" (unpremised stackedStatus 32)
  check "a control write from an unknown register refuses"
    (refused (derive [0xe121f000,0xe12fff1e]))
  check "a control write from an immediate refuses"
    (refused (derive [0xe321f0d3,0xe12fff1e]))
  check "a status word with mode bits set refuses"
    (refused (derive [0xe10f0000,0xe380001f,0xe121f000,0xe12fff1e]))
  check "a status word with mode bits cleared refuses"
    (refused (derive [0xe10f0000,0xe3c0001f,0xe121f000,0xe12fff1e]))
  check "a status word with a toggled mode bit refuses"
    (refused (derive [0xe10f0000,0xe2200001,0xe121f000,0xe12fff1e]))
  check "a status word with the T bit set refuses"
    (refused (derive [0xe10f0000,0xe3800020,0xe121f000,0xe12fff1e]))
  check "an extension write with the E bit set refuses"
    (refused (derive [0xe10f0000,0xe3800c02,0xe122f000,0xe12fff1e]))
  check "a control and extension write after masking off the E bit refuses"
    (refused (derive [0xe10f0000,0xe20000ff,0xe123f000,0xe12fff1e]))
  maskedStatus <- either fail pure (deriveResult [] [0xe10f0000,0xe30f1fff,0xe0000001,0xe121f000,0xe12fff1e])
  check "a status word masked by a register constant covering the mode, T and E bits is accepted without premises"
    (unpremised maskedStatus 20)
  check "a status word merged with an unknown value whose mode bits alone were cleared refuses"
    (refused (derive [0xe300201f,0xe1c30002,0xe10f1000,0xe1800001,0xe121f000,0xe12fff1e]))
  check "the exclusive or of two status words refuses"
    (refused (derive [0xe10f0000,0xe10f1000,0xe0200001,0xe121f000,0xe12fff1e]))
  check "a status write with an empty field mask refuses"
    (refused (derive [0xe120f000,0xe12fff1e]))
  check "an unmasked caller value merged into the status word refuses"
    (refused (derive [0xe52db004,0xe28db000,0xe10f1000,0xe3c11080,0xe1800001,0xe121f000,0xe28bd000,0xe8bd0800,0xe12fff1e]))
  check "a status word held in a callee-saved register across a call refuses"
    (refused (derive [0xe92d4010,0xe10f4000,0xeb0003fd,0xe121f004,0xe8bd8010]))
  check "a status word held in a frame word across a call refuses"
    (refused (derive [0xe92d4010,0xe24dd008,0xe10f0000,0xe58d0000,0xeb0003fb,0xe59d1000,0xe121f001,0xe28dd008,0xe8bd8010]))
  check "a status word joined with an unknown value refuses"
    (refused (derive [0xe10f0000,0x11a00001,0xe121f000,0xe12fff1e]))
  check "an SPSR write refuses"
    (refused (derive [0xe16ff000,0xe12fff1e]))
  check "an SPSR read refuses"
    (refused (derive [0xe14f0000,0xe12fff1e]))
  check "a banked register write refuses"
    (refused (derive [0xe12df200,0xe12fff1e]))
  check "a banked register read refuses"
    (refused (derive [0xe10d0200,0xe12fff1e]))
  check "a status write from PC refuses"
    (refused (derive [0xe121f00f,0xe12fff1e]))
  check "an SPSR-restoring PC write after a status write refuses"
    (refused (derive [0xe10f0000,0xe129f000,0xe1b0f00e]))
  check "an SPSR-restoring load-multiple refuses"
    (refused (derive [0xe92d4000,0xe8fd8000]))
  unproven <- either fail pure (inspectFunction [] (GuestAddress 0x1000) (bytes [0xe121f000,0xe12fff1e])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1008)) [])
  check "structural inventory classifies an unproven control write as linear code"
    (inspectedCodeWords unproven == [GuestAddress 0x1000, GuestAddress 0x1004] && refused (inspectedReturn unproven))
  signedIndex <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe6bf0070,0xe0833100,0xe5831000,0xe8bd8010])
  check "a sign-extended halfword index into a global is accepted under global-object separation"
    (derivedDeclaration signedIndex == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && globalStore signedIndex 0x1010)
  signedByteIndex <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe6af0070,0xe0833100,0xe5831000,0xe8bd8010])
  check "a sign-extended byte index into a global is accepted under global-object separation"
    (derivedDeclaration signedByteIndex == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && globalStore signedByteIndex 0x1010)
  check "a sign-extended frame address remains frame-derived"
    (refused (derive [0xe92d4010,0xe6bf007d,0xe5801000,0xe8bd8010]))
  check "a sign-extend-and-add result has no tracked source"
    (refused (derive [0xe92d4010,0xe3033000,0xe6b20070,0xe0833100,0xe5831000,0xe8bd8010]))
  let loadedStore result uses = derivationPremises result == [LoadedObjectFrameIsolation]
        && derivationPremiseUses result == [PremiseUse (ProgramCounter use) LoadedObjectFrameIsolation | use <- uses]
  loadedPointer <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe5932000,0xe5821004,0xe8bd8010])
  check "a store through a word loaded from a global is accepted under loaded-object isolation at the store"
    (derivedDeclaration loadedPointer == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 20) && loadedStore loadedPointer [0x100c]
      && map returnPremiseLabel (derivationPremises loadedPointer) == ["loaded-object-frame-isolation"])
  loadedIndex <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe5932000,0xe0822100,0xe5821000,0xe8bd8010])
  check "an indexed loaded object keeps loaded-object isolation while the frame is private"
    (derivedDeclaration loadedIndex == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && loadedStore loadedIndex [0x1010])
  loadedBeforeCall <- either fail pure (deriveResult [] [0xe92d4010,0xe24dd004,0xe3033000,0xe5934000,0xe1a0000d,0xeb0003f9,0xe5841000,0xe28dd004,0xe8bd8010])
  check "a word loaded before the frame is exposed keeps its premise after exposure"
    (derivedDeclaration loadedBeforeCall == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 36)
      && derivationPremiseUses loadedBeforeCall == [PremiseUse (ProgramCounter 0x1014) UnexposedFrameIsolation, PremiseUse (ProgramCounter 0x1018) LoadedObjectFrameIsolation])
  loadedLeaf <- either fail pure (deriveResult [] [0xe3033000,0xe5932000,0xe5821000,0xe12fff1e])
  check "a loaded-object store with no tracked frame words records no premise" (unpremised loadedLeaf 16)
  check "a word loaded after the frame is exposed can alias the saved return"
    (refused (derive [0xe92d4010,0xe24dd004,0xe1a0000d,0xeb0003fb,0xe3033000,0xe5932000,0xe5821000,0xe28dd004,0xe8bd8010]))
  check "a loaded object indexed by an unknown value after exposure can alias the saved return"
    (refused (derive [0xe92d4010,0xe24dd004,0xe3033000,0xe5934000,0xe1a0000d,0xeb0003f9,0xe0842005,0xe5821000,0xe28dd004,0xe8bd8010]))
  check "an unknown word reloaded from the frame is not a loaded object"
    (refused (derive [0xe92d4010,0xe24dd004,0xe59d2000,0xe5821000,0xe28dd004,0xe8bd8010]))
  check "a loaded byte is not a loaded object"
    (refused (derive [0xe92d4010,0xe3033000,0xe5d32000,0xe5821000,0xe8bd8010]))
  check "a loaded object joined with an incoming pointer can alias the saved return"
    (refused (derive [0xe92d4010,0xe3033000,0xe5932000,0xe3500000,0x01a02001,0xe5820000,0xe8bd8010]))
  loadedPair <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe1c300d0,0xe5812000,0xe8bd8010])
  check "the second register of a loaded pair is a loaded object"
    (derivedDeclaration loadedPair == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 20) && loadedStore loadedPair [0x100c])
  loadedChain <- either fail pure (deriveResult [] [0xe92d4010,0xe3033000,0xe5932000,0xe5922008,0xe5821000,0xe8bd8010])
  check "a word loaded through a loaded object is a loaded object while the frame is private"
    (derivedDeclaration loadedChain == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 24) && loadedStore loadedChain [0x1010])
  check "a word loaded through a loaded object after exposure can alias the saved return"
    (refused (derive [0xe92d4010,0xe24dd004,0xe3033000,0xe5934000,0xe1a0000d,0xeb0003f9,0xe5942008,0xe5821000,0xe28dd004,0xe8bd8010]))
  check "a conditional load joined with a frame address can alias the saved return"
    (refused (derive [0xe92d4010,0xe1a0200d,0xe3033000,0xe3500000,0x05932000,0xe5821000,0xe8bd8010]))
  let tasks = [SavedStackArray (GuestAddress 0x3000) 56 4 0x18]
      deriveTasks words' = deriveFunction tasks (GuestAddress 0x1000) (bytes words')
        (FunctionWindow (GuestAddress 0x1000) (GuestAddress (0x1000 + 4 * fromIntegral (length words')))) []
      savedSpStore = [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe5821000,0xe8bd8010]
  check "a store through a loaded record's declared saved stack pointer refuses"
    (refused (deriveTasks savedSpStore))
  check "without a declaration the same store is accepted under loaded-object isolation"
    (fmap derivationPremises (deriveResult [] savedSpStore) == Right [LoadedObjectFrameIsolation])
  check "a saved stack pointer reached by adding the field offset refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe2822018,0xe5922000,0xe5821000,0xe8bd8010]))
  otherField <- either fail pure (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe5922014,0xe5821000,0xe8bd8010])
  check "another field of a loaded record stays a loaded object" (loadedStore otherField [0x1010])
  check "a saved stack pointer read at its absolute address refuses"
    (refused (deriveTasks [0xe92d4010,0xe3033018,0xe5932000,0xe5821000,0xe8bd8010]))
  absoluteOther <- either fail pure (deriveTasks [0xe92d4010,0xe3033014,0xe5932000,0xe5821000,0xe8bd8010])
  check "another absolute word of a declared record stays a loaded object" (loadedStore absoluteOther [0x100c])
  check "a word read through a global indexed inside a declared array refuses"
    (refused (deriveTasks [0xe92d4010,0xe3033000,0xe0833180,0xe5932008,0xe5821000,0xe8bd8010]))
  outsideGlobal <- either fail pure (deriveTasks [0xe92d4010,0xe3043000,0xe0833180,0xe5932008,0xe5821000,0xe8bd8010])
  check "a word read through a global indexed outside the declared arrays stays a loaded object" (loadedStore outsideGlobal [0x1010])
  check "an indexed loaded record keeps its displacement for the saved stack pointer"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe0822180,0xe5922018,0xe5821000,0xe8bd8010]))
  check "a word read through an incoming pointer is not a loaded object"
    (refused (derive [0xe92d4010,0xe5902008,0xe5821000,0xe8bd8010]))
  joinedObject <- either fail pure (deriveTasks [0xe92d4010,0xe3024000,0xe3043000,0xe0833180,0xe3510000,0x15943000,0xe5830000,0xe8bd8010])
  check "a loaded record joined with an outside global records both premises at the store"
    (derivationPremiseUses joinedObject == [PremiseUse (ProgramCounter 0x1018) GlobalObjectStackSeparation, PremiseUse (ProgramCounter 0x1018) LoadedObjectFrameIsolation])
  check "the next record's saved stack pointer reached by adding a stride refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe2822038,0xe5922018,0xe5821000,0xe8bd8010]))
  check "the next record's saved stack pointer at an immediate offset refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe5922050,0xe5821000,0xe8bd8010]))
  check "the previous record's saved stack pointer at a negative offset refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe5122020,0xe5821000,0xe8bd8010]))
  check "a global base one record before a declared array reaches its saved stack pointers"
    (refused (deriveTasks [0xe92d4010,0xe3023fe0,0xe7932180,0xe5821000,0xe8bd8010]))
  check "a constant displacement after indexing moves a global base into a declared array"
    (refused (deriveTasks [0xe92d4010,0xe3023f00,0xe0833180,0xe5932118,0xe5821000,0xe8bd8010]))
  check "the second word of a loaded pair at the saved stack pointer refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe1c201d4,0xe5813000,0xe8bd8010]))
  check "an unaligned word overlapping the saved stack pointer refuses"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe5922016,0xe5821000,0xe8bd8010]))
  check "loaded records joined at different displacements refuse a read at the field offset"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe3500000,0x12822004,0xe5922018,0xe5821000,0xe8bd8010]))
  check "loaded records joined at different displacements refuse a read at offset zero"
    (refused (deriveTasks [0xe92d4010,0xe3023000,0xe5932000,0xe3500000,0x12822004,0xe5922000,0xe5821000,0xe8bd8010]))
  check "a loaded record joined with a global inside a declared array refuses"
    (refused (deriveTasks [0xe92d4010,0xe3024000,0xe3033000,0xe0833180,0xe3510000,0x15943000,0xe5932004,0xe5820000,0xe8bd8010]))
  check "a word straddling into the next record's saved stack pointer at offset zero refuses"
    (refused (deriveFunction [SavedStackArray (GuestAddress 0x3000) 56 4 0] (GuestAddress 0x1000)
      (bytes [0xe92d4010,0xe3023000,0xe5932000,0xe5922036,0xe5821000,0xe8bd8010])
      (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1018)) []))
  let excluded words' = refused (deriveTasks words') && either (const False) (const True) (deriveResult [] words')
      refusesWith message words' = either (message `isInfixOf`) (const False) (deriveTasks words')
      savedIndex = [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe3043000,0xe0833002,0xe5831000,0xe8bd8010]
  check "a saved stack pointer used as an index from a constant base refuses"
    (refused (deriveTasks savedIndex))
  check "without a declaration the same index is accepted under global-object separation"
    (fmap derivationPremises (deriveResult [] savedIndex) == Right [GlobalObjectStackSeparation])
  check "a saved stack pointer used as a register offset from a constant base refuses"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe3043000,0xe7831002,0xe8bd8010])
  check "a shifted saved stack pointer used as an index refuses"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe3043000,0xe0833102,0xe5831000,0xe8bd8010])
  check "a saved stack pointer plus a constant used as an index refuses"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe2822004,0xe3043000,0xe0833002,0xe5831000,0xe8bd8010])
  check "a masked saved stack pointer stays tainted as an index"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe2022080,0xe3043000,0xe0833002,0xe5831000,0xe8bd8010])
  check "a conditional saved stack pointer load taints the join"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe3500000,0x15922018,0xe3043000,0xe0833002,0xe5831000,0xe8bd8010])
  check "the second word of a loaded pair at the saved stack pointer is excluded as an index"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe1c221d4,0xe304c000,0xe08cc003,0xe58c1000,0xe8bd8010])
  check "a saved stack pointer spilled to the frame and reloaded stays excluded"
    (excluded [0xe92d4010,0xe24dd004,0xe3023000,0xe5932000,0xe5922018,0xe58d2000,0xe59d2000,0xe3043000,0xe0833002,0xe5831000,0xe28dd004,0xe8bd8010])
  check "a spilled saved stack pointer survives a store through an unknown pointer"
    (excluded [0xe24dd004,0xe3023000,0xe5932000,0xe5922018,0xe58d2000,0xe5801000,0xe59d2000,0xe3043000,0xe0833002,0xe5831000,0xe28dd004,0xe12fff1e])
  check "a word loaded by a block load from a record is excluded as an index"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe2822018,0xe8920001,0xe3043000,0xe0833000,0xe5831000,0xe8bd8010])
  check "a word loaded through an incoming pointer is not tainted"
    (fmap derivationPremises (deriveTasks [0xe92d4010,0xe5902000,0xe3043000,0xe0833002,0xe5831000,0xe8bd8010]) == Right [GlobalObjectStackSeparation])
  check "a saved stack pointer read through a frame-derived pointer is excluded as an index"
    (excluded [0xe92d4010,0xe24dd008,0xe3023000,0xe5932000,0xe3500000,0x01a0200d,0xe5922018,0xe3043000,0xe0833002,0xe5831000,0xe28dd008,0xe8bd8010])
  check "a saved stack pointer stored outside the frame refuses"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe3054000,0xe5842000,0xe5940000,0xe5801000,0xe8bd8010]
      && refusesWith "stored outside the frame" [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe3054000,0xe5842000,0xe5940000,0xe5801000,0xe8bd8010])
  check "a leaf store through a saved stack pointer refuses"
    (excluded [0xe3023000,0xe5932000,0xe5922018,0xe5821000,0xe12fff1e]
      && refusesWith "store through a saved stack pointer" [0xe3023000,0xe5932000,0xe5922018,0xe5821000,0xe12fff1e])
  check "a saved stack pointer passed to a call refuses"
    (excluded [0xe92d4010,0xe3023000,0xe5932000,0xe5920018,0xeb0003f0,0xe8bd8010]
      && refusesWith "passed to a call" [0xe92d4010,0xe3023000,0xe5932000,0xe5920018,0xeb0003f0,0xe8bd8010])
  check "a returned saved stack pointer refuses"
    (excluded [0xe3023000,0xe5932000,0xe5920018,0xe12fff1e]
      && refusesWith "returned" [0xe3023000,0xe5932000,0xe5920018,0xe12fff1e])
  check "a saved stack pointer returned by the popping load refuses"
    (excluded [0xe3023000,0xe5932000,0xe5923018,0xe92d4008,0xe8bd8001]
      && refusesWith "returned" [0xe3023000,0xe5932000,0xe5923018,0xe92d4008,0xe8bd8001])
  check "a saved stack pointer overwritten by the popping load is not returned"
    (either (const False) (const True) (deriveTasks [0xe3023000,0xe5932000,0xe5920018,0xe92d4010,0xe8bd8001]))
  let tailWords = [0xe3023000,0xe5932000,0xe5920018,0xea0003fb]
      tailDerive arrays = deriveFunction arrays (GuestAddress 0x1000) (bytes tailWords)
        (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1010)) [GuestAddress 0x2000]
  check "a saved stack pointer passed to an external tail refuses"
    (either ("passed to a call" `isInfixOf`) (const False) (tailDerive tasks) && either (const False) (const True) (tailDerive []))
  check "a saved stack pointer read through an array indexed after exposure is excluded"
    (excluded [0xe92d4010,0xe24dd008,0xe1a0000d,0xeb0003f0,0xe3033000,0xe0833004,0xe5930018,0xeb0003f0,0xe28dd008,0xe8bd8010]
      && refusesWith "passed to a call" [0xe92d4010,0xe24dd008,0xe1a0000d,0xeb0003f0,0xe3033000,0xe0833004,0xe5930018,0xeb0003f0,0xe28dd008,0xe8bd8010])
  let exposedSpill = [0xe92d4010,0xe24dd008,0xe3023000,0xe5932000,0xe5924018,0xe58d4000,0xe28d0004,0xeb0003f0,0xe59d2000,0xe1a00002,0xeb0003f0,0xe28dd008,0xe8bd8010]
  check "a spilled saved stack pointer survives a call that exposes the frame"
    (excluded exposedSpill && refusesWith "passed to a call at 1028" exposedSpill)
  check "a spilled saved stack pointer overwritten by an unknown word is no longer excluded"
    (fmap derivationPremises (deriveTasks [0xe24dd004,0xe3023000,0xe5932000,0xe5922018,0xe58d2000,0xe58d0000,0xe59d2000,0xe3043000,0xe0833002,0xe5831000,0xe28dd004,0xe12fff1e]) == Right [GlobalObjectStackSeparation])
  let accumulatedRecord = [0xe92d4010,0xe3024000,0xe5944000,0xe3a01038,0xe0234291,0xe5930018,0xe5801000,0xe8bd8010]
  check "a multiply-accumulate keeps its loaded record addend for the saved stack pointer"
    (excluded accumulatedRecord && refusesWith "store through a saved stack pointer" accumulatedRecord)
  let framedSavedStack = [0xe92d4010,0xe3023000,0xe5932000,0xe5922018,0xe082200d,0xeb0003f0,0xe8bd8010]
  check "a saved stack pointer combined with the frame address stays excluded"
    (excluded framedSavedStack && refusesWith "passed to a call" framedSavedStack)
