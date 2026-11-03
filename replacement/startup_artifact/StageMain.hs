module Main where

import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import Replacement.StartupArtifact.Stage (runStager)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, canonicalPath, manifestPath, scratchPath, outputPath] ->
      runStager (Tools.Tools encoder rebuild decoder) canonicalPath manifestPath scratchPath outputPath
    _ -> H.failH "usage: stage-startup-artifact ENCODER REBUILD DECODER CANONICAL_DAT MANIFEST SCRATCH_DIR OUTPUT_DIR"
