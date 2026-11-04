{-# LANGUAGE GHC2021 #-}

-- | Writes the owned menu engine's generated Rust tree and record side table from the pinned
-- menu tables JSON of //replacement/modal_runtime:menu-tables.
module Main where

import Control.Monad (forM_, unless, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ModalRuntime.MenuEngineTree
import System.Directory (doesPathExist)
import System.Environment (getArgs)

-- | private-data/scratch/ControllerStrategy/menu-tables.json.
tablesSha256 :: String
tablesSha256 = "7fa536b6f4ab72205be57f407ce7224fa0bbc397aca2061ad2fac3e14ba2021d"

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  tablesPath <- required "--tables"
  treePath <- required "--tree-out"
  recordsPath <- required "--records-out"
  forM_ [treePath, recordsPath] $ \path -> do
    exists <- doesPathExist path
    when exists (fail (path ++ " already exists"))
  bytes <- B.readFile tablesPath
  let tablesSha = sha256Hex (sha256 bytes)
  unless (tablesSha == tablesSha256) (fail "menu tables identity differs")
  document <- either (fail . ("menu tables: " ++)) pure (J.eitherDecodeStrict bytes)
  let digest text = sha256Hex (sha256 (B.pack (map (fromIntegral . fromEnum) text)))
  generated <- either fail pure (generate digest tablesSha document)
  writeFile treePath (generatedTree generated)
  writeFile recordsPath (generatedRecords generated)
  putStrLn ("menu-engine-tree: " ++ show (length (documentNodes document)) ++ " nodes; identity " ++ generatedIdentity generated)
