{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Numeric (showHex)
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.Placement (Carrier (..), GuestAddress (..), PlacementError (..), carrierBytes, hitListing, stage4Carrier)
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs)

refuse :: ArtifactError -> IO a
refuse failure = case failure of
  PlacementRejected (CarrierReferenced hits) -> do
    mapM_ (putStrLn . hitListing) hits
    H.failH ("carrier referenced by " ++ show (length hits) ++ " stock words")
  PlacementRejected (RetiredReferenced hits) -> do
    mapM_ (putStrLn . hitListing) hits
    H.failH ("retired property code referenced by " ++ show (length hits) ++ " retained words")
  _ -> H.failH (show failure)

refuseSource :: CanonicalSource -> ArtifactError -> IO a
refuseSource source failure = do
  case failure of
    PlacementRejected (RetiredReferenced hits) -> mapM_ (putStrLn . show) (sourceRetainedFieldEvidence source hits)
    _ -> pure ()
  refuse failure

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, datPath, image4Path, loaderAPath, loaderBPath, image5Path, plainPath, image2Path, linkedPath, symbolsPath, elfPath, scratchPath, outputPath, contractPath, image4OutputPath] -> do
      forM_ [outputPath, contractPath, image4OutputPath] $ \path -> doesPathExist path >>= \exists -> when exists (H.failH (path ++ " already exists"))
      dat <- H.readRegular (64 * 1024 * 1024) datPath
      image4 <- H.readRegular 0x300000 image4Path
      loaderA <- H.readRegular 0x20000 loaderAPath
      loaderB <- H.readRegular 0x20000 loaderBPath
      image5 <- H.readRegular 0x800000 image5Path
      plain <- H.readRegular 0x800000 plainPath
      image2 <- H.readRegular 0x800000 image2Path
      linked <- H.readRegular (carrierBytes stage4Carrier) linkedPath
      symbols <- H.readRegular 0x10000 symbolsPath
      elf <- H.readRegular (16 * 1024 * 1024) elfPath
      source <- either refuse pure (canonicalSource dat image4 loaderA loaderB image5 plain image2)
      let carrier@(Carrier (GuestAddress low) (GuestAddress high)) = stage4Carrier
      putStrLn ("carrier [0x" ++ showHex low "" ++ ", 0x" ++ showHex high "" ++ ")")
      artifact <- either refuse pure (derivePublicArtifact ProductionArtifact (sourceProperties source) carrier (ArtifactInput linked symbols elf))
      staged <- either (refuseSource source) pure (stageNativeImage4 source artifact)
      createDirectoryIfMissing True scratchPath
      scratch <- Image4.validateScratchDirectory scratchPath
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch staged
      B.writeFile image4OutputPath (stagedBytes staged)
      B.writeFile outputPath (candidateBytes candidate)
      L.writeFile contractPath (J.encode (publicCandidateContractValue candidate) <> "\n")
    _ -> H.failH "usage: stage-property-service-public ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B IMAGE5 PLAIN IMAGE2 LINKED_BIN SYMBOLS LINKED_ELF SCRATCH_DIR OUT_DAT OUT_CONTRACT OUT_IMAGE4"
