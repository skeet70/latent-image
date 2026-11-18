{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
module Main where

import Control.Exception (IOException, catch)
import Control.Monad (filterM)
import Coverage (coverageReport)
import Data.Aeson (encode)
import qualified Data.ByteString.Lazy.Char8 as L
import Data.Maybe (maybeToList)
import qualified FujiAF.Host as H
import ShimCatalog
import System.Directory (getCurrentDirectory, doesFileExist)
import System.Environment (getArgs, getExecutablePath, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (ReadMode, WriteMode), hGetContents, hPutStr, hSetEncoding, utf8, withFile)

data Action = Check | Render FilePath | Inspect FilePath deriving (Eq, Show)
data Options = Options { catalogRoot :: FilePath, manifestPath :: FilePath, receiptPath :: Maybe FilePath, action :: Action, checkRenderPath :: Maybe FilePath, buildRoot :: FilePath }

main :: IO ()
main = run `catch` \(failure :: IOException) -> do
  L.putStrLn (encode (report (Left (Refusal InvalidCatalog (show failure)))))
  exitFailure

run :: IO ()
run = do
  root <- defaultRoot
  source <- defaultBuildRoot root
  arguments <- getArgs
  options <- parse (Options root "replacement/shim-images.json" Nothing Check Nothing source) arguments
  units <- loadUnits (catalogRoot options)
  manifest <- readJson (catalogRoot options </> manifestPath options)
  case checkRenderPath options of
    Just path -> withFile path ReadMode $ \handle -> do
      hSetEncoding handle utf8
      actual <- hGetContents handle
      H.ensure (actual == renderRegister units manifest) "generated shim register differs from declarations"
    Nothing -> pure ()
  case action options of
    Render path -> withFile path WriteMode $ \handle -> do
      hSetEncoding handle utf8
      hPutStr handle (renderRegister units manifest)
    Inspect path -> do
      inventory <- inspectArtifacts (catalogRoot options) manifest
      L.writeFile path (encode inventory <> "\n")
    Check -> do
      result <- checkAllWithBuildRoot (buildRoot options) (catalogRoot options) units manifest
      let receipt = encode (coverageReport units manifest result)
      maybe (pure ()) (\path -> L.writeFile path (receipt <> "\n")) (receiptPath options)
      L.putStrLn receipt
      either (const exitFailure) (const (pure ())) result

parse :: Options -> [String] -> IO Options
parse options [] = pure options
parse options ("--catalog":path:rest) = parse options {catalogRoot = path} rest
parse options ("--build-root":path:rest) = parse options {buildRoot = path} rest
parse options ("--images":path:rest) = parse options {manifestPath = path} rest
parse options ("--receipt-output":path:rest) = parse options {receiptPath = Just path} rest
parse options ("--render":path:rest) = setAction options (Render path) rest
parse options ("--check-render":path:rest) = parse options {checkRenderPath = Just path} rest
parse options ("--artifact-inventory":path:rest) = setAction options (Inspect path) rest
parse _ _ = H.failH "usage: shim-check [--catalog ROOT] [--build-root CHECKOUT] [--images MANIFEST] [--receipt-output PATH] [--render PATH | --artifact-inventory PATH | --check-render PATH]"

setAction :: Options -> Action -> [String] -> IO Options
setAction options selected rest = do
  H.ensure (action options == Check) "only one shim-check action is permitted"
  parse options {action = selected} rest

defaultRoot :: IO FilePath
defaultRoot = do
  current <- getCurrentDirectory
  executable <- getExecutablePath
  runfiles <- lookupEnv "RUNFILES_DIR"
  candidates <- filterM (\path -> doesFileExist (path </> "replacement/shim-images.json")) ([path </> "_main" | path <- maybeToList runfiles] ++ [executable ++ ".runfiles/_main",current])
  pure (case candidates of root:_ -> root; [] -> current)

defaultBuildRoot :: FilePath -> IO FilePath
defaultBuildRoot fallback = do
  current <- getCurrentDirectory
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  candidates <- filterM (\path -> doesFileExist (path </> "AGENTS.md")) (maybeToList workspace ++ [current])
  pure (case candidates of source:_ -> source; [] -> fallback)
