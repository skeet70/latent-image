{-# LANGUAGE OverloadedStrings #-}
module SuiteMain (runMain) where

import Control.Monad (forM_, unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString.Lazy as B
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import Replacement.AfResult.PoolSuite
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory)

runMain :: Mode -> IO ()
runMain mode = do
  args <- getArgs
  runner <- case dropWhile (/= "--runner") args of
    [] -> pure fujimodNativeCase
    _ : value : _ -> makeAbsolute value
    _ -> fail "--runner requires a value"
  image <- required args "--image5"
  oldElf <- required args "--old-owned-elf"
  elf <- required args "--owned-elf"
  cache <- required args "--cache-root"
  directory <- required args "--output"
  results <- required args "--results"
  checkStorageBudget directory
  checkStorageBudget (takeDirectory results)
  scope <- readScope image oldElf elf
  createDirectoryIfMissing True cache
  createDirectoryIfMissing True directory
  createDirectoryIfMissing True (takeDirectory results)
  runs <- runSuite mode runner cache directory scope
  let failures = [(runName result,problem) | result <- runs, problem <- runDifferences result ++ ["control not detected: " ++ label | (label,False) <- runControls result]]
  B.writeFile results (encode (object
    [ "source" .= consumerSourceIdentity
    , "mode" .= show mode
    , "scope" .= ("Synthetic differential execution only; no physical locking, timing, scheduler or stall claims." :: String)
    , "cases" .= [object ["name" .= runName result,"differences" .= runDifferences result,"controls" .= [object ["defect" .= label,"detected" .= detected] | (label,detected) <- runControls result]] | result <- runs]
    , "failures" .= length failures
    ]))
  forM_ failures $ \(ident,problem) -> putStrLn (ident ++ ": " ++ problem)
  putStrLn (show (length runs) ++ " cases; " ++ show (length failures) ++ " failures")
  unless (null failures && not (null runs)) exitFailure

required :: [String] -> String -> IO FilePath
required args flag = case dropWhile (/= flag) args of
  _ : value : _ -> pure value
  _ -> fail (flag ++ " requires a value")
