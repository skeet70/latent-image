module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.Saclac.Artifact
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, datPath, image5Path, binPath, symbolsPath, disasmPath, sectionsPath, scratchPath, outputImage, outputDat, outputContract] -> do
      [encoder', rebuild', decoder'] <- mapM canonicalizePath [encoder, rebuild, decoder]
      dat <- H.readRegular (64 * 1024 * 1024) datPath
      image5 <- H.readRegular (16 * 1024 * 1024) image5Path
      bin <- H.readRegular (4 * 1024 * 1024) binPath
      symbols <- H.readRegular (1024 * 1024) symbolsPath
      disasm <- H.readRegular (1024 * 1024) disasmPath
      sections <- H.readRegular (1024 * 1024) sectionsPath
      staged <- stageSaclac (Image4.Tools encoder' rebuild' decoder') scratchPath dat image5 bin symbols disasm sections
      B.writeFile outputImage (stagedImage5 staged)
      B.writeFile outputDat (stagedDat staged)
      image' <- canonicalizePath outputImage
      dat' <- canonicalizePath outputDat
      L.writeFile outputContract (J.encode (contractValue image' dat' staged))
    _ -> H.failH "usage: stage-saclac ENCODER REBUILD DECODER SOURCE_DAT IMAGE5 LINKED_BIN SYMBOLS DISASM SECTIONS SCRATCH_DIR OUT_IMAGE5 OUT_DAT OUT_CONTRACT"
