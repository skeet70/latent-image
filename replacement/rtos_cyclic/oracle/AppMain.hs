{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosCyclic.AppSuite
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

stockSha256 :: String
stockSha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

main :: IO ()
main = do
  arguments <- getArgs
  runner <- required arguments "--runner"
  stockImagePath <- required arguments "--stock-image"
  ownedPath <- required arguments "--owned-bin"
  symbolsPath <- required arguments "--owned-symbols"
  cacheRoot <- required arguments "--cache-root"
  resultsPath <- required arguments "--results"
  stockImage <- B.readFile stockImagePath
  ownedImage <- B.readFile ownedPath
  ownedFunctions <- readOwnedFunctions symbolsPath
  stockDigest <- sha256File stockImagePath
  ownedDigest <- sha256File ownedPath
  symbolsDigest <- sha256File symbolsPath
  unless (sha256Hex stockDigest == stockSha256) (failH (stockImagePath ++ " is not the pinned X-M1 1.50 image4"))
  unless (B.length ownedImage == 0x384) (failH (ownedPath ++ " is not the app-callback 0x384-byte image"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  runs <- run runner cacheRoot (takeDirectory resultsPath </> "app-callback") stockImage ownedImage ownedFunctions
  let failedChecks =
        [ (runName one, description)
        | one <- runs
        , (description, False) <- runChecks one
        ]
      differences = [(runName one, message) | one <- runs, message <- runDifferences one]
      caseValue one =
        object
          [ "case" .= runName one
          , "checks" .= [object ["check" .= description, "holds" .= holds] | (description, holds) <- runChecks one]
          , "differences" .= runDifferences one
          ]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "inputs" .=
              object
                [ "stock_sha256" .= sha256Hex stockDigest
                , "owned_bin_sha256" .= sha256Hex ownedDigest
                , "owned_symbols_sha256" .= sha256Hex symbolsDigest
                ]
          , "cases" .= map caseValue runs
          , "failures" .= (length failedChecks + length differences)
          ]
  BL.writeFile resultsPath (encode document)
  unless (null failedChecks && null differences) $ do
    mapM_ (\(name, description) -> putStrLn (name ++ ": " ++ description)) failedChecks
    mapM_ (\(name, message) -> putStrLn (name ++ ": " ++ message)) differences
    exitFailure
