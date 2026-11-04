{-# LANGUAGE OverloadedStrings #-}
-- | property-pair-control: the natural pair's guest control, the staged owned image4 with one owned
-- property's setter table word restored to its retired stock arm, carried through replace-record4.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (object, (.=))
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Emulation.Image4Staging
import Emulation.PropertyPairPlan
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die)
import Text.Printf (printf)

usage :: String
usage = "usage: property-pair-control ENCODER REBUILD DECODER SCRATCH_DIR SOURCE_DAT ANALYSIS LINKED CONTRACT PROPERTY OUT_DAT OUT_MANIFEST"

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  [encoder, rebuild, decoder, scratch, sourceDat, analysis, linkedPath, contractPath, propertyText, outDat, outManifest] -> do
    forM_ [outDat, outManifest] $ \path -> doesPathExist path >>= \exists -> when exists (die ("property-pair-control: " ++ path ++ " exists"))
    property <- case propertyText of
      '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure (fromInteger value)
      _ -> die "property-pair-control: PROPERTY is a 0x-prefixed hexadecimal id"
    canonical <- B.readFile sourceDat
    images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
    linked <- B.readFile linkedPath
    contract <- either (die . ("property-pair-control: contract: " ++)) pure . decodeContract =<< B.readFile contractPath
    patch <- either (die . ("property-pair-control: " ++)) pure (controlPatch contract property)
    let control = controlWord patch
    decoded <- either (die . ("property-pair-control: " ++)) pure
      (roleImage4 (PlanInputs ControlRole contract images linked (Just control)))
    scratchDirectory <- validateScratchDirectory scratch
    staged <- runImage4Staging (Tools encoder rebuild decoder) scratchDirectory canonical canonical decoded
    unless (candidateImage4Sha256 staged == sha256Hex (sha256 decoded)) (die "property-pair-control: carried image4 differs")
    B.writeFile outDat (candidateDat staged)
    BL.writeFile outManifest (A.encode (object
      [ "schema" .= (1 :: Int)
      , "property" .= (printf "0x%x" property :: String)
      , "address" .= (printf "0x%08x" (patchAddress patch) :: String)
      , "target" .= (printf "0x%08x" (patchOriginal patch) :: String)
      , "owned_target" .= (printf "0x%08x" (patchPatched patch) :: String)
      , "dat_sha256" .= candidateSha256 staged
      , "image4_sha256" .= candidateImage4Sha256 staged
      , "owned_dat_sha256" .= contractCandidateDat contract
      , "staged_image4_sha256" .= contractStagedImage4 contract
      ]) <> "\n")
    putStrLn ("property-pair-control: " ++ outDat ++ " " ++ candidateSha256 staged)
  _ -> die usage
