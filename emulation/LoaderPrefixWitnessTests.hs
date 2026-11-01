module Main where

import qualified Data.ByteString as B
import Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..), SymbolDocument (..), checkNaturalArtifact, checkRefusalArtifact, checkNegativeArtifact, checkInterleavedTailArtifact )
import Emulation.LoaderPrefixReceiptFixtures (runReceiptCases)
import Emulation.LoaderPrefixWitness (Scenario (..))
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)


main :: IO ()
main = do
  args <- getArgs
  (bin, symbols, refusalBin, refusalSymbols, invalidBin, invalidSymbols, predicateBin, predicateSymbols, tailBin, tailSymbols) <- case args of
    [a, b, c, d, e, f, g, h, i, j] -> (,,,,,,,,,) <$> B.readFile a <*> B.readFile b <*> B.readFile c <*> B.readFile d
      <*> B.readFile e <*> B.readFile f <*> B.readFile g <*> B.readFile h <*> B.readFile i <*> B.readFile j
    _ -> fail "usage: loader-prefix-witness-tests IMAGE.bin IMAGE.symbols REFUSAL.bin REFUSAL.symbols INVALID_CORE.bin INVALID_CORE.symbols PREDICATE_ZERO.bin PREDICATE_ZERO.symbols INTERLEAVED_TAIL.bin INTERLEAVED_TAIL.symbols"
  facts <- checkNaturalArtifact (ArtifactInput (U.MediaImage bin) (SymbolDocument symbols))
  checkRefusalArtifact (ArtifactInput (U.MediaImage refusalBin) (SymbolDocument refusalSymbols))
  checkNegativeArtifact NegativeInvalidCore (ArtifactInput (U.MediaImage invalidBin) (SymbolDocument invalidSymbols))
  checkNegativeArtifact NegativePredicateZero (ArtifactInput (U.MediaImage predicateBin) (SymbolDocument predicateSymbols))
  checkInterleavedTailArtifact (ArtifactInput (U.MediaImage tailBin) (SymbolDocument tailSymbols))
  runReceiptCases facts
  putStrLn "loader-prefix witness tests pass"
