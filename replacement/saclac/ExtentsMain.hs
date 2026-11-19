{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Emits the linker script and the census sites file from the declared
-- extents, so the two extents are typed once. The sites file's owned post
-- entries are bound from the linked image variants it observes.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import FujiAF.Host (failH)
import Replacement.RtosPlatform.Aggregate (CarrierUnion, parseCarrierUnion)
import Replacement.Saclac.Artifact (Admitted, admit)
import Replacement.Saclac.Placement (bindOwnedPosts, insertWatchRanges, renderLinkerScript)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["linker-script", carriersPath] -> do
      union <- readCarriers carriersPath
      putStr (renderLinkerScript union)
    "census-sites" : inputPath : outputPath : carriersPath : images@(_ : _) | length images `mod` 5 == 0 -> do
      union <- readCarriers carriersPath
      value <- either failH pure . J.eitherDecode =<< L.readFile inputPath
      admitted <- mapM (admitImage union) (groups images)
      updated <- either failH pure (insertWatchRanges value >>= bindOwnedPosts admitted)
      L.writeFile outputPath (J.encode updated)
    _ -> failH "usage: saclac-extents linker-script CARRIERS | census-sites INPUT OUTPUT CARRIERS (ELF BIN SYMBOLS DISASM SECTIONS)..."

readCarriers :: FilePath -> IO CarrierUnion
readCarriers path = either failH pure . parseCarrierUnion =<< B.readFile path

groups :: [a] -> [[a]]
groups [] = []
groups items = let (group, rest) = splitAt 5 items in group : groups rest

admitImage :: CarrierUnion -> [FilePath] -> IO Admitted
admitImage union paths = case paths of
  [elf, bin, symbols, disasm, sections] ->
    either (failH . show) pure =<<
      (admit union <$> B.readFile elf <*> B.readFile bin <*> B.readFile symbols <*> B.readFile disasm <*> B.readFile sections)
  _ -> failH "an image is ELF BIN SYMBOLS DISASM SECTIONS"
