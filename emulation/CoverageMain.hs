{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Stock-side branch coverage from native runner reports and a census of
-- stock function extents. Prints a table and optionally writes JSON.
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Coverage
import FujiAF.Host (failH)
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.Environment (getArgs)
import System.FilePath ((</>), takeExtension)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  extentsPath <- required "--extents" arguments
  let reportPaths = values "--report" arguments
      reportDirectories = values "--reports" arguments
  unless (not (null reportPaths) || not (null reportDirectories)) $
    failH "--report or --reports is required"
  let jsonPath = lastValue "--json" arguments
  directories <- forM reportDirectories $ \directory -> do
    exists <- doesDirectoryExist directory
    unless exists (failH ("--reports is not a directory: " ++ directory))
    collect directory
  files <- forM reportPaths $ \path -> do
    exists <- doesFileExist path
    unless exists (failH ("--report is not a file: " ++ path))
    pure [path]
  extents <- either failH pure . parseExtents =<< B.readFile extentsPath
  observations <- forM (concat files ++ concat directories) $ \path -> do
    bytes <- B.readFile path
    either (\problem -> failH (path ++ ": " ++ problem)) pure (parseObservation bytes)
  let result = coverage extents observations
  putStr (coverageTable result)
  case jsonPath of
    Nothing -> pure ()
    Just path -> BL.writeFile path (coverageJson result)
  hPutStrLn stderr
    (show (length (coverageFunctions result)) ++ " extents from " ++ show (length observations) ++ " reports")

collect :: FilePath -> IO [FilePath]
collect directory = do
  entries <- listDirectory directory
  paths <- forM entries $ \entry -> do
    let path = directory </> entry
    isDirectory <- doesDirectoryExist path
    if isDirectory
      then collect path
      else pure [path | takeExtension path == ".json"]
  pure (concat paths)

required :: String -> [String] -> IO String
required flag arguments = case values flag arguments of
  [] -> failH (flag ++ " is required")
  found -> pure (last found)

lastValue :: String -> [String] -> Maybe String
lastValue flag arguments = case values flag arguments of
  [] -> Nothing
  found -> Just (last found)

values :: String -> [String] -> [String]
values flag arguments = [value | (key, value) <- zip arguments (drop 1 arguments), key == flag]
