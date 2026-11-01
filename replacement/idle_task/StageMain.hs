module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.IdleTask.Artifact
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,linkedPath,symbolsPath,scratchPath,outputPath,contractPath] -> do
      dat <- H.readRegular (64*1024*1024) datPath
      image <- H.readRegular 0x300000 imagePath
      linked <- H.readRegular 16 linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either (H.failH . show) pure (canonicalSource dat image)
      artifact <- either (H.failH . show) pure (deriveArtifact linked symbols)
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch (stageImage4 source artifact)
      B.writeFile outputPath (candidateBytes candidate)
      L.writeFile contractPath (J.encode (candidateContractValue candidate))
    _ -> H.failH "usage: stage-idle-task ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT; host admission only, no runtime acceptance"
