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
deriveWith tails words' = deriveFunction (GuestAddress 0x1000) (bytes words')
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
