{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Emits the linker script and the census sites file from the declared
-- extents, so the two extents are typed once.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import FujiAF.Host (failH)
import Replacement.RtosPlatform.Aggregate (parseCarrierUnion)
import Replacement.Saclac.Placement (insertWatchRanges, renderLinkerScript)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["linker-script", carriersPath] -> do
      union <- either failH pure . parseCarrierUnion =<< B.readFile carriersPath
      putStr (renderLinkerScript union)
    ["census-sites", inputPath, outputPath] -> do
      value <- either failH pure . J.eitherDecode =<< L.readFile inputPath
      updated <- either failH pure (insertWatchRanges value)
      L.writeFile outputPath (J.encode updated)
    _ -> failH "usage: saclac-extents linker-script CARRIERS | census-sites INPUT OUTPUT"
