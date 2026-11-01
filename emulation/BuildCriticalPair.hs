module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Emulation.CriticalPair as C
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuilder,decoder,sourcePath,image4Path,imagePath,symbolsPath,scratchPath,outputPath,contractPath] -> do
      source <- H.readRegular (64*1024*1024) sourcePath
      image4 <- H.readRegular (16*1024*1024) image4Path
      image <- H.readRegular 0x1400 imagePath
      symbols <- H.readRegular 0x10000 symbolsPath
      original <- either H.failH pure (C.deriveOriginal source image4)
      artifact <- either H.failH pure (C.deriveLinkedArtifact image symbols)
      staging <- either H.failH pure (C.stageStartup original artifact)
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- Image4.runImage4Staging (Image4.Tools encoder rebuilder decoder)
        scratch source (C.startupDat staging) (C.patchedImage4 staging)
      let final = Image4.candidateDat candidate
      either H.failH pure (C.verifyFinalDat staging final)
      B.writeFile outputPath final
      BL.writeFile contractPath (J.encode (C.stagingValue staging final))
    _ -> H.failH "usage: build-critical-pair SOURCE_DAT ORIGINAL_IMAGE4 LINKED_BIN LINKED_SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT"
