{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
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
deriveResult tails words' = deriveFunction (GuestAddress 0x1000) (bytes words')
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
  underPremise <- either fail pure (deriveFunction (GuestAddress 0x1000) (bytes [0xe92d4000,0xeb0003fd,0xe8bd8000])
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
    (refused (deriveFunction (GuestAddress 0x1000) (bytes [0xea000000,0xe1a00000,0xe12fff1e])
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
  structural <- either fail pure (inspectFunction (GuestAddress 0x1000) (bytes [0xe3a0e000,0xe12fff1e])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1008)) [])
  check "structural inventory retains an extent without admitting an overwritten return"
    (inspectedDeclaration structural == FunctionDeclaration (GuestAddress 0x1000) (FunctionSize 8)
      && inspectedCodeWords structural == [GuestAddress 0x1000, GuestAddress 0x1004]
      && null (inspectedLiteralWords structural)
      && refused (inspectedReturn structural))
  withLiteral <- either fail pure (inspectFunction (GuestAddress 0x1000) (bytes [0xe59f0000,0xe12fff1e,0x12345678])
    (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x100c)) [])
  check "structural inventory separates a referenced literal from executable words"
    (inspectedCodeWords withLiteral == [GuestAddress 0x1000, GuestAddress 0x1004]
      && inspectedLiteralWords withLiteral == [GuestAddress 0x1008]
      && inspectedReturn withLiteral == Right [])
  check "structural inventory also refuses unclassified trailing storage"
    (refused (inspectFunction (GuestAddress 0x1000) (bytes [0xe12fff1e,0x12345678])
      (FunctionWindow (GuestAddress 0x1000) (GuestAddress 0x1008)) []))
