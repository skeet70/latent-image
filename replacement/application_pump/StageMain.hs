module Main where

import Control.Monad (forM_)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Shared
import qualified FujiAF.Host as H
import Replacement.ApplicationPump.Artifact
import qualified Replacement.ApplicationPump.Staging as Image5
import System.Directory (canonicalizePath, listDirectory)
import System.Environment (getArgs)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,linkedPath,symbolsPath,scratchPath] -> do
      scratch <- Image5.validateScratchDirectory scratchPath
      root <- canonicalizePath scratchPath
      let output = root </> "application-pump.DAT"
          imageOutput = root </> "application-pump-image5.bin"
          contract = root </> "application-pump-contract.json"
      entries <- listDirectory root
      forM_ ["application-pump.DAT","application-pump-image5.bin","application-pump-contract.json"] $ \name ->
        H.ensure (name `notElem` entries) ("refusing existing output: " ++ root </> name)
      dat <- H.readRegular (64*1024*1024) datPath
      image <- H.readRegular 0x500000 imagePath
      linked <- H.readRegular 0x1000 linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either (H.failH . show) pure (canonicalSource dat image)
      artifact <- either (H.failH . show) pure (deriveArtifact linked symbols)
      staged <- either (H.failH . show) pure (stageImage5 source artifact)
      candidate <- stageCandidate (Shared.Tools encoder rebuild decoder) scratch staged
      B.writeFile output (candidateBytes candidate)
      B.writeFile imageOutput (stagedBytes staged)
      L.writeFile contract (J.encode (candidateContractValue candidate))
    _ -> H.failH "usage: stage-application-pump ENCODER REBUILD DECODER SOURCE_DAT IMAGE5 LINKED_BIN SYMBOLS SCRATCH_DIR"
