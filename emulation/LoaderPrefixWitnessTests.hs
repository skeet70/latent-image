module Main where

import qualified Data.ByteString as B
import Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..), SymbolDocument (..), checkNaturalArtifact, checkRefusalArtifact )
import Emulation.LoaderPrefixReceiptFixtures (runReceiptCases)
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)

main :: IO ()
main = do
  args <- getArgs
  (bin, symbols, refusalBin, refusalSymbols) <- case args of
    [a, b, c, d] -> (,,,) <$> B.readFile a <*> B.readFile b <*> B.readFile c <*> B.readFile d
    _ -> fail "usage: loader-prefix-witness-tests IMAGE.bin IMAGE.symbols REFUSAL.bin REFUSAL.symbols"
  facts <- checkNaturalArtifact (ArtifactInput (U.MediaImage bin) (SymbolDocument symbols))
  checkRefusalArtifact (ArtifactInput (U.MediaImage refusalBin) (SymbolDocument refusalSymbols))
  runReceiptCases facts
  putStrLn "loader-prefix witness tests pass"
