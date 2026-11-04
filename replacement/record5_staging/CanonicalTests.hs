-- | The general_heap and application_pump candidates staged through the shared
-- record-5 route on canonical inputs keep their pre-extraction digests.
module Main where

import qualified Data.ByteString as B
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified Replacement.ApplicationPump.Artifact as Pump
import qualified Replacement.ApplicationPump.Staging as PumpStaging
import qualified Replacement.GeneralHeap.Artifact as Heap
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256

refused :: Show e => Either e a -> IO a
refused = either (H.failH . show) pure

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, heapBin, heapSymbols, pumpBin, pumpSymbols, datPath, image4Path, image5Path, scratchPath] -> do
      [encoder', rebuild', decoder', heapBin', heapSymbols', pumpBin', pumpSymbols'] <-
        mapM canonicalizePath [encoder, rebuild, decoder, heapBin, heapSymbols, pumpBin, pumpSymbols]
      let tools = Tools.Tools encoder' rebuild' decoder'
      dat <- H.readRegular (64*1024*1024) datPath
      four <- H.readRegular (16*1024*1024) image4Path
      five <- H.readRegular (16*1024*1024) image5Path
      heapSource <- refused (Heap.canonicalSource dat four five)
      heapArtifact <- H.readRegular (4*1024*1024) heapBin' >>= \bytes -> H.readRegular (1024*1024) heapSymbols' >>= refused . Heap.deriveArtifact bytes
      heapImages <- refused (Heap.stageImages heapSource heapArtifact)
      heapCandidate <- Heap.stageCandidate tools scratchPath heapImages
      H.ensure (digest (Heap.candidateBytes heapCandidate) == "2202d0f8b0cefd692fc9b533c18d8aff2c5bce33e14f1c5a62db612a9a117506")
        "general heap staged DAT digest changed"
      pumpScratch <- PumpStaging.validateScratchDirectory scratchPath
      pumpSource <- refused (Pump.canonicalSource dat five)
      pumpArtifact <- H.readRegular 0x1000 pumpBin' >>= \bytes -> H.readRegular 0x10000 pumpSymbols' >>= refused . Pump.deriveArtifact bytes
      pumpImage <- refused (Pump.stageImage5 pumpSource pumpArtifact)
      pumpCandidate <- Pump.stageCandidate tools pumpScratch pumpImage
      H.ensure (digest (Pump.candidateBytes pumpCandidate) == "27436773bd88e50e8daeb878e29247f44de4e737faadd109060e548644e1e492")
        "application pump staged DAT digest changed"
      putStrLn "record5-staging-canonical-tests: 2 checks passed"
    _ -> H.failH "usage: record5-staging-canonical-tests ENCODER REBUILD DECODER HEAP_BIN HEAP_SYMBOLS PUMP_BIN PUMP_SYMBOLS CANONICAL_DAT IMAGE4 IMAGE5 SCRATCH_DIR_UNDER_APP154"
