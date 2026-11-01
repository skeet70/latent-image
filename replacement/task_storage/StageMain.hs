module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.TaskStorage.Artifact
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,loaderAPath,loaderBPath,linkedPath,symbolsPath,scratchPath,outputPath,contractPath] -> do
      dat <- H.readRegular (64 * 1024 * 1024) datPath
      image <- H.readRegular 0x300000 imagePath
      loaderA <- H.readRegular 0x20000 loaderAPath
      loaderB <- H.readRegular 0x20000 loaderBPath
      linked <- H.readRegular 0x1000 linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either H.failH pure (canonicalSource dat image loaderA loaderB)
      artifact <- either H.failH pure (deriveArtifact linked symbols)
      let staged = stageImage4 source artifact
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch staged
      B.writeFile outputPath (candidateBytes candidate)
      L.writeFile contractPath (J.encode (candidateContractValue candidate))
    _ -> H.failH "usage: stage-task-storage ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT"
