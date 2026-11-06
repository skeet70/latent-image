{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains against the stock loaders and the owned
-- stage bins, and fails on any difference.
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import Replacement.RtosObjects.Layout (parseFacts)
import Replacement.RtosObjects.Suite (FamilyRun (..), Image (..), extents, runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..))

main :: IO ()
main = do
  [runner, layoutPath, loaderA, loaderB, ownedA, ownedB] <- getArgs
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  temporary <- getTemporaryDirectory
  let directory = temporary </> "rtos-objects-family"
  createDirectoryIfMissing True directory
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image (minimum (map fst (extents DomainA))) ownedABytes
      ownedBImage = Image (minimum (map fst (extents DomainB))) ownedBBytes
  runA <- runFamily runner directory facts DomainA stockA ownedAImage
  runB <- runFamily runner directory facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
  forM_ runs $ \run -> forM_ (familyCases run) $ \(name, result) ->
    putStrLn (show (familyDomain run) ++ " " ++ name ++ ": " ++ either ("failed: " ++) (const "ok") result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure
