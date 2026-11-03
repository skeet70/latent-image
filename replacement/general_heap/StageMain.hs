module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.GeneralHeap.Artifact
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,image4Path,image5Path,linkedPath,symbolsPath,scratchPath,output4,output5,outputDat,outputContract] -> do
      dat <- H.readRegular (64*1024*1024) datPath
      four <- H.readRegular (16*1024*1024) image4Path
      five <- H.readRegular (16*1024*1024) image5Path
      linked <- H.readRegular (4*1024*1024) linkedPath
      symbols <- H.readRegular (1024*1024) symbolsPath
      source <- either (H.failH . show) pure (canonicalSource dat four five)
      artifact <- either (H.failH . show) pure (deriveArtifact linked symbols)
      staged <- either (H.failH . show) pure (stageImages source artifact)
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratchPath staged
      B.writeFile output4 (image4Bytes staged)
      B.writeFile output5 (image5Bytes staged)
      B.writeFile outputDat (candidateBytes candidate)
      absolute4 <- canonicalizePath output4
      absolute5 <- canonicalizePath output5
      L.writeFile outputContract (J.encode (contractValue absolute4 absolute5 staged candidate))
    _ -> H.failH "usage: stage-general-heap ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 IMAGE5 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_IMAGE4 OUT_IMAGE5 OUT_DAT OUT_CONTRACT"
