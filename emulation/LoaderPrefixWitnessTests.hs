module Main where

import qualified Data.ByteString as B
import Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..), SymbolDocument (..), checkNaturalArtifact, checkRefusalArtifact, checkInterleavedTailArtifact )
import Emulation.LoaderPrefixReceiptFixtures (runReceiptCases)
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)


main :: IO ()
main = do
  args <- getArgs
  (bin, symbols, refusalBin, refusalSymbols, tailBin, tailSymbols) <- case args of
    [a, b, c, d, e, f] -> (,,,,,) <$> B.readFile a <*> B.readFile b <*> B.readFile c <*> B.readFile d
      <*> B.readFile e <*> B.readFile f
    _ -> fail "usage: loader-prefix-witness-tests IMAGE.bin IMAGE.symbols REFUSAL.bin REFUSAL.symbols INTERLEAVED_TAIL.bin INTERLEAVED_TAIL.symbols"
  facts <- checkNaturalArtifact (ArtifactInput (U.MediaImage bin) (SymbolDocument symbols))
  checkRefusalArtifact (ArtifactInput (U.MediaImage refusalBin) (SymbolDocument refusalSymbols))
  checkInterleavedTailArtifact (ArtifactInput (U.MediaImage tailBin) (SymbolDocument tailSymbols))
  runReceiptCases facts
  putStrLn "loader-prefix witness tests pass"
