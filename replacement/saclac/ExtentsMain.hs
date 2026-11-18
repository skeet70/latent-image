{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Emits the linker script and the census sites file from the declared
-- extents, so the two extents are typed once. The sites file's owned post
-- entry is bound from the linked image it observes.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import FujiAF.Host (failH)
import Replacement.Saclac.Artifact (admit)
import Replacement.Saclac.Placement (bindOwnedPost, insertWatchRanges, renderLinkerScript)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["linker-script"] -> putStr renderLinkerScript
    ["census-sites", inputPath, elfPath, binPath, symbolsPath, disasmPath, sectionsPath, outputPath] -> do
      value <- either failH pure . J.eitherDecode =<< L.readFile inputPath
      admitted <- either (failH . show) pure =<<
        (admit <$> B.readFile elfPath <*> B.readFile binPath <*> B.readFile symbolsPath <*> B.readFile disasmPath <*> B.readFile sectionsPath)
      updated <- either failH pure (insertWatchRanges value >>= bindOwnedPost admitted)
      L.writeFile outputPath (J.encode updated)
    _ -> failH "usage: saclac-extents linker-script | census-sites INPUT ELF BIN SYMBOLS DISASM SECTIONS OUTPUT"
