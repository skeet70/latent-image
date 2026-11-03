module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.Image5Load.Artifact
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory, takeFileName)

-- Created by the stager; doubles as the staging scratch directory.
newtype OutputDirectory = OutputDirectory FilePath

data OutputFile = CandidateDat | LinkedImage | LinkedSymbols | Contract

outputPath :: OutputDirectory -> OutputFile -> FilePath
outputPath (OutputDirectory directory) file = directory </> case file of
  CandidateDat -> "candidate.DAT"
  LinkedImage -> "image5-load-v1.bin"
  LinkedSymbols -> "image5-load-v1.symbols"
  Contract -> "owned-image5-stage-v1-contract.json"

newOutput :: FilePath -> IO OutputDirectory
newOutput configured = do
  parent <- canonicalizePath (takeDirectory configured)
  _ <- Image4.validateScratchDirectory parent
  let leaf = takeFileName configured
      output = parent </> leaf
  H.ensure (leaf/="."&&leaf/=".."&&not (null leaf)) "output requires a new directory name"
  exists <- doesPathExist output
  H.ensure (not exists) "staging output already exists"
  createDirectory output
  pure (OutputDirectory output)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,linkedPath,symbolsPath,directory] -> do
      dat <- H.readRegular (64*1024*1024) datPath
      image <- H.readRegular 0x300000 imagePath
      linked <- H.readRegular 0x1000 linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either (H.failH . show) pure (canonicalSource dat image)
      artifact <- either (H.failH . show) pure (deriveArtifact linked symbols)
      output@(OutputDirectory created) <- newOutput directory
      scratch <- Image4.validateScratchDirectory created
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch (stageImage4 source artifact)
      B.writeFile (outputPath output CandidateDat) (candidateBytes candidate)
      B.writeFile (outputPath output LinkedImage) linked
      B.writeFile (outputPath output LinkedSymbols) symbols
      L.writeFile (outputPath output Contract) (J.encode (candidateContractValue candidate))
    _ -> H.failH "usage: stage-image5-load ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS NEW_OUTPUT_DIR"
