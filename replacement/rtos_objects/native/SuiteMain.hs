{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.Aeson as J
import Data.Text (pack)
import Data.Text.Encoding (encodeUtf8)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModPaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import Replacement.RtosObjects.Layout (parseFacts)
import Replacement.RtosObjects.Suite (FamilyRun (..), Image (..), extents, runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..))

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    [_] -> failH "--runner needs a value"
    _ : value : _ -> makeAbsolute value
  cacheRoot <- required "--cache-root"
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image (minimum (map fst (extents DomainA))) ownedABytes
      ownedBImage = Image (minimum (map fst (extents DomainB))) ownedBBytes
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
      cases =
        [ object
            [ "domain" .= show (familyDomain run)
            , "case" .= name
            , "result" .= outcomeText result
            ]
        | run <- runs
        , (name, result) <- familyCases run
        ]
      outcomeText (Left message) = "failed: " ++ message
      outcomeText (Right []) = "ok"
      outcomeText (Right differences) = "failed: " ++ unlines differences
      reuse =
        [ object ["domain" .= show (familyDomain run), "stock" .= show (fst (familyReuse run)), "owned" .= show (snd (familyReuse run))]
        | run <- runs
        ]
      firstFailure = case failures of
        (name, message) : _ -> object ["case" .= name, "message" .= message]
        [] -> J.Null
      casesPath = resultsPath ++ ".cases.txt"
      document =
        object
          [ "passed" .= null failures
          , "execution" .= ("host" :: String)
          , "comparison" .= ("current" :: String)
          , "labels"
              .= object
                [ "subset" .= ("f2-historical" :: String)
                , "native" .= False
                , "system" .= False
                , "hardware" .= False
                ]
          , "cases" .= cases
          , "reuse" .= reuse
          , "failures" .= length failures
          , "firstFailure" .= firstFailure
          , "artifact" .= casesPath
          , "reviewPacket" .= resultsPath
          ]
      casesText = unlines [show (familyDomain run) ++ " " ++ name ++ ": " ++ outcomeText result | run <- runs, (name, result) <- familyCases run]
  BL.writeFile resultsPath (encode document)
  B.writeFile casesPath (encodeUtf8 (pack casesText))
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
