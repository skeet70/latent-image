{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.Placement (PlacementError (..), hitListing)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

refuse :: ArtifactError -> IO a
refuse failure = case failure of
  PlacementRejected (CarrierReferenced hits) -> do
    mapM_ (putStrLn . hitListing) hits
    H.failH ("carrier referenced by " ++ show (length hits) ++ " stock words")
  _ -> H.failH (show failure)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, datPath, image4Path, loaderAPath, loaderBPath, image5Path, plainPath, image2Path, linkedPath, symbolsPath, scratchPath, outputPath, contractPath, image4OutputPath] -> do
      forM_ [outputPath, contractPath, image4OutputPath] $ \path -> doesPathExist path >>= \exists -> when exists (H.failH (path ++ " already exists"))
      dat <- H.readRegular (64 * 1024 * 1024) datPath
      image4 <- H.readRegular 0x300000 image4Path
      loaderA <- H.readRegular 0x20000 loaderAPath
      loaderB <- H.readRegular 0x20000 loaderBPath
      image5 <- H.readRegular 0x800000 image5Path
      plain <- H.readRegular 0x800000 plainPath
      image2 <- H.readRegular 0x800000 image2Path
      linked <- H.readRegular 0x2000 linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either refuse pure (canonicalSource dat image4 loaderA loaderB image5 plain image2)
      artifact <- either refuse pure (deriveArtifact (sourceProperties source) linked symbols)
      staged <- either refuse pure (stageImage4 source artifact)
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch staged
      B.writeFile image4OutputPath (stagedBytes staged)
      B.writeFile outputPath (candidateBytes candidate)
      L.writeFile contractPath (J.encode (candidateContractValue candidate) <> "\n")
    _ -> H.failH "usage: stage-property-service ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B IMAGE5 PLAIN IMAGE2 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT OUT_IMAGE4"
