{-# LANGUAGE GHC2021 #-}

-- | Writes the stock menu tables of the pinned X-M1 1.50 image 4 as JSON.
module Main where

import Control.Monad (unless, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ModalRuntime.MenuTables
import System.Directory (doesPathExist)
import System.Environment (getArgs)

image4Sha256 :: String
image4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

-- | private-data/analysis/x-m1-1.50/filmsim/lang-en-0039c000.bin.
stringsSha256 :: String
stringsSha256 = "68ea4d03b50267f0adbb03f37662d81ae0b4cc41658c4a8663b2209e75ce3e63"

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  imagePath <- required "--image4"
  stringsPath <- required "--strings"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  imageBytes <- B.readFile imagePath
  stringsBytes <- B.readFile stringsPath
  let imageSha = sha256Hex (sha256 imageBytes)
      stringsSha = sha256Hex (sha256 stringsBytes)
  unless (imageSha == image4Sha256) (fail "image 4 identity differs")
  unless (stringsSha == stringsSha256) (fail "strings slot identity differs")
  let tables = decodeMenuTables (image4 imageBytes) (stringSlot stringsBytes)
      resolved = length [() | NodeEntry {entryRecord = Right _} <- tablesNodes tables]
  L.writeFile outputPath (J.encode (menuTablesJson imageSha stringsSha tables))
  putStrLn ("menu-tables: " ++ show (length (tablesNodes tables)) ++ " nodes (" ++ show resolved ++ " resolved), "
    ++ show (length [() | PropertyEntry {propertyTable = Table _ _} <- tablesProperties tables]) ++ " conversion tables")
