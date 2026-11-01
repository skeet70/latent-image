{-# LANGUAGE OverloadedStrings #-}
module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.PrimaryInitializerArtifact (artifactValue, deriveArtifact, stageArtifact, stagedBytes)
import Emulation.PrimaryInitializerConsumerSource (bindConsumerSource, consumerSourceValue)
import Emulation.PrimaryInitializerSource (parseSource, sourceValue)
import qualified FujiAF.Host as H
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [sourcePath, image4Path, imagePath, symbolsPath, outputPath, contractPath] -> do
      original <- H.readRegular (64 * 1024 * 1024) sourcePath
      image4 <- H.readRegular (3 * 1024 * 1024) image4Path
      image <- H.readRegular 0x57c imagePath
      symbols <- H.readRegular 0x10000 symbolsPath
      source <- either H.failH pure (parseSource original)
      consumer <- either H.failH pure (bindConsumerSource source image4)
      artifact <- either H.failH pure (deriveArtifact source image symbols)
      staged <- either H.failH pure (stageArtifact source artifact)
      let contract = J.object
            [ "artifact" J..= artifactValue artifact
            , "consumer_source" J..= consumerSourceValue consumer
            , "source" J..= sourceValue source
            ]
      B.writeFile outputPath (stagedBytes staged)
      BL.writeFile contractPath (J.encode contract)
    _ -> H.failH "usage: build-primary-initializer SOURCE_DAT IMAGE4 IMAGE_BIN IMAGE_SYMBOLS OUT_DAT OUT_CONTRACT"
