module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.StartupComposition.Artifact
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,loaderAPath,loaderBPath,dispatchPath,dispatchSymbolsPath,installPath,installSymbolsPath,scratchPath,outputPath,contractPath] -> do
      sources <- SourceFiles <$> H.readRegular 0x4000000 datPath <*> H.readRegular 0x300000 imagePath
        <*> H.readRegular 0x20000 loaderAPath <*> H.readRegular 0x20000 loaderBPath
      dispatcher <- NativeFiles <$> H.readRegular 0x1000 dispatchPath <*> H.readRegular 0x10000 dispatchSymbolsPath
      installation <- NativeFiles <$> H.readRegular 0x1000 installPath <*> H.readRegular 0x10000 installSymbolsPath
      source <- either (H.failH . show) pure (canonicalSource sources)
      artifact <- either (H.failH . show) pure (deriveArtifact dispatcher installation)
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch (stageImage4 source artifact)
      B.writeFile outputPath (candidateBytes candidate)
      L.writeFile contractPath (J.encode (candidateContractValue candidate))
    _ -> H.failH "usage: stage-startup-composition ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT"
