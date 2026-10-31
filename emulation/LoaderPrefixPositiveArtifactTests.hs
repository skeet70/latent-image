module Main where

import qualified Data.ByteString as B
import Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..), SymbolDocument (..), checkNaturalArtifact )
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)

main :: IO ()
main = do
  args <- getArgs
  input <- case args of
    [imagePath, symbolsPath] -> ArtifactInput
      <$> (U.MediaImage <$> B.readFile imagePath)
      <*> (SymbolDocument <$> B.readFile symbolsPath)
    _ -> fail "usage: loader-prefix-positive-artifact-tests IMAGE.bin IMAGE.symbols"
  _ <- checkNaturalArtifact input
  pure ()
