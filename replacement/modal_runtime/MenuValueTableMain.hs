{-# LANGUAGE GHC2021 #-}

-- | Writes the owned value transaction's Rust tables from the pinned image-4
-- bytes and the menu-tables document.
module Main where

import Control.Monad (unless, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ModalRuntime.MenuTables (image4)
import Replacement.ModalRuntime.MenuValueTable
import Replacement.ModalRuntime.NewFile (writeNewFile)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  imagePath <- required "--image4"
  tablesPath <- required "--tables"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail (outputPath ++ " already exists"))
  imageBytes <- B.readFile imagePath
  let imageSha = sha256Hex (sha256 imageBytes)
  unless (imageSha == image4Pin) (fail ("image 4 identity differs: sha256 " ++ imageSha))
  tableBytes <- B.readFile tablesPath
  let tablesSha = sha256Hex (sha256 tableBytes)
  unless (tablesSha == tablesPin) (fail ("menu tables identity differs: sha256 " ++ tablesSha))
  document <- either (fail . ("menu tables: " ++)) pure (J.eitherDecodeStrict tableBytes)
  generated <- either fail pure (generate tablesSha (image4 imageBytes) document)
  writeNewFile outputPath generated
  putStrLn ("menu-value-table: " ++ show (length (documentProperties document)) ++ " conversion tables")
