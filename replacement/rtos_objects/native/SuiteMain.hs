{-# LANGUAGE OverloadedStrings #-}

-- | Runs the fourteen family slots in both entry domains against the stock loaders and the
-- current family stage bins, through the cached native batch, and writes one compact terminal JSON.
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
import Replacement.RtosObjects.Layout (parseFacts, parseManifest)
import Replacement.RtosObjects.Suite (FamilyRun (..), Image (..), buildEntries, extents, runFamily)
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
  manifestPath <- required "--manifest"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  familyA <- required "--family-a-bin"
  familyB <- required "--family-b-bin"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  manifest <- either fail pure . parseManifest =<< B.readFile manifestPath
  entries <- either fail pure (buildEntries manifest)
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  familyABytes <- B.readFile familyA
  familyBBytes <- B.readFile familyB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      familyAImage = Image (minimum (map fst (extents DomainA))) familyABytes
      familyBImage = Image (minimum (map fst (extents DomainB))) familyBBytes
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts entries DomainA stockA familyAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts entries DomainB stockB familyBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(failingName, unlines failureLines) | run <- runs, (failingName, Right failureLines) <- familyCases run, not (null failureLines)]
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
      outcomeText (Right failureLines) = "failed: " ++ unlines failureLines
      reuse =
        [ object ["domain" .= show (familyDomain run), "stock" .= show (fst (familyReuse run)), "owned" .= show (snd (familyReuse run))]
        | run <- runs
        ]
      controls =
        [ object ["control" .= name, "holds" .= ok]
        | run <- runs
        , (name, ok) <- familyControls run
        ]
      differences =
        [ object ["domain" .= show (familyDomain run), "case" .= name, "difference" .= tag]
        | run <- runs
        , (name, tag) <- familyDifferences run
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
                [ "subset" .= ("family-current" :: String)
                , "native" .= False
                , "system" .= False
                , "hardware" .= False
                ]
          , "manifest" .= manifestPath
          , "cases" .= cases
          , "reuse" .= reuse
          , "controls" .= controls
          , "differences" .= differences
          , "timerSeams"
              .= ( [ "stock-insert-timer A 0x44c60 B 0xd4c20"
                   , "stock-remove-timer A 0x44ca8 B 0xd4c68"
                   , "stock-timeout-callback A 0x44318 B 0xd42d8"
                   , "stock-timer-tick A 0x44d8c B 0xd4d4c"
                   ] :: [String] )
          , "failures" .= length failures
          , "firstFailure" .= firstFailure
          , "artifact" .= casesPath
          , "reviewPacket" .= resultsPath
          ]
      casesText = unlines [show (familyDomain run) ++ " " ++ name ++ ": " ++ outcomeText result | run <- runs, (name, result) <- familyCases run]
      controlText = unlines [show (familyDomain run) ++ " control " ++ name ++ ": " ++ (if ok then "ok" else "failed") | run <- runs, (name, ok) <- familyControls run]
  BL.writeFile resultsPath (encode document)
  B.writeFile casesPath (encodeUtf8 (pack (casesText ++ controlText)))
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
