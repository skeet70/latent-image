{-# LANGUAGE OverloadedStrings #-}
-- | property-pair-control: the natural pair's guest control, the staged owned image4 with one word
-- restored so the owned role fetches a retired stock word, carried through replace-record4. A
-- schema-1 contract restores one property's setter table word; a schema-2 contract restores the
-- setter entry adapter.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
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

usage :: String
usage = "usage: property-pair-control ENCODER REBUILD DECODER SCRATCH_DIR SOURCE_DAT ANALYSIS LINKED CONTRACT PROPERTY|setter-entry OUT_DAT OUT_MANIFEST"

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  [encoder, rebuild, decoder, scratch, sourceDat, analysis, linkedPath, contractPath, which, outDat, outManifest] -> do
    forM_ [outDat, outManifest] $ \path -> doesPathExist path >>= \exists -> when exists (die ("property-pair-control: " ++ path ++ " exists"))
    canonical <- B.readFile sourceDat
    images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
    linked <- B.readFile linkedPath
    contract <- either (die . ("property-pair-control: contract: " ++)) pure . decodeContract =<< B.readFile contractPath
    control <- either (die . ("property-pair-control: " ++)) pure $ case which of
      "setter-entry" -> setterEntryControl contract
      '0' : 'x' : digits | [(value, "")] <- readHex digits -> controlWord <$> controlPatch contract (fromInteger value)
      _ -> Left "PROPERTY is a 0x-prefixed hexadecimal id or setter-entry"
    decoded <- either (die . ("property-pair-control: " ++)) pure
      (roleImage4 (PlanInputs ControlRole contract images linked (Just control) Nothing))
    scratchDirectory <- validateScratchDirectory scratch
    staged <- runImage4Staging (Tools encoder rebuild decoder) scratchDirectory canonical canonical decoded
    unless (candidateImage4Sha256 staged == sha256Hex (sha256 decoded)) (die "property-pair-control: carried image4 differs")
    B.writeFile outDat (candidateDat staged)
    BL.writeFile outManifest (A.encode (controlManifestValue control (candidateSha256 staged) (candidateImage4Sha256 staged)
      (contractCandidateDat contract) (contractStagedImage4 contract)) <> "\n")
    putStrLn ("property-pair-control: " ++ outDat ++ " " ++ candidateSha256 staged)
  _ -> die usage
