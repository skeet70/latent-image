module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import Replacement.DisplayScanout.Artifact
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["publish", image4Path, symbolsPath, publicationsPath, contractPath] -> do
      image4 <- H.readRegular (16 * 1024 * 1024) image4Path
      symbols <- H.readRegular 0x400000 symbolsPath
      rows <- either (H.failH . show) pure (canonicalPublications image4 symbols)
      body <- either (H.failH . show) pure (bodyBounds symbols)
      L.writeFile publicationsPath (J.encode (publicationsValue rows))
      L.writeFile contractPath (J.encode (contractValue rows body (SHA.sha256Hex (SHA.sha256 symbols))))
    _ -> H.failH "usage: display-scanout-artifact publish IMAGE4 SYMBOLS OUT_PUBLICATIONS_JSON OUT_CONTRACT_JSON"
