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
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import Replacement.RtosObjects.Layout (parseFacts, parseManifest)
import Replacement.RtosObjects.Suite (FamilyRun (..), JoinedRun (..), Image (..), buildEntries, extents, s3bBase, runFamily, runJoined)
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
  s3bA <- required "--s3b-a-bin"
  s3bB <- required "--s3b-b-bin"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  manifest <- either fail pure . parseManifest =<< B.readFile manifestPath
  entries <- either fail pure (buildEntries manifest)
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  familyABytes <- B.readFile familyA
  familyBBytes <- B.readFile familyB
  s3bABytes <- B.readFile s3bA
  s3bBBytes <- B.readFile s3bB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      familyAImage = Image (minimum (map fst (extents DomainA))) familyABytes
      familyBImage = Image (minimum (map fst (extents DomainB))) familyBBytes
      s3bAImage = Image (s3bBase DomainA) s3bABytes
      s3bBImage = Image (s3bBase DomainB) s3bBBytes
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts entries DomainA stockA familyAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts entries DomainB stockB familyBImage
  joinedA <- runJoined runner cacheRoot (takeDirectory resultsPath </> "joined-a") facts entries DomainA stockA familyAImage s3bAImage
  joinedB <- runJoined runner cacheRoot (takeDirectory resultsPath </> "joined-b") facts entries DomainB stockB familyBImage s3bBImage
  let runs = [runA, runB]
      joinedRuns = [joinedA, joinedB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(failingName, unlines failureLines) | run <- runs, (failingName, Right failureLines) <- familyCases run, not (null failureLines)]
          ++ [(name, message) | run <- joinedRuns, (name, Left message) <- joinedRows run]
          ++ [(failingName, unlines failureLines) | run <- joinedRuns, (failingName, Right failureLines) <- joinedRows run, not (null failureLines)]
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
      joinedCasesValue =
        [ object ["domain" .= show (joinedDomain run), "case" .= name, "result" .= outcomeText result]
        | run <- joinedRuns
        , (name, result) <- joinedRows run
        ]
      joinedReuseValue =
        [ let (stockReuse, ownedReuse, stubReuse) = joinedReuse run
           in object ["domain" .= show (joinedDomain run), "stock" .= show stockReuse, "owned" .= show ownedReuse, "stub" .= show stubReuse]
        | run <- joinedRuns
        ]
      joinedControlsValue =
        [ object ["control" .= name, "holds" .= ok]
        | run <- joinedRuns
        , (name, ok) <- joinedControls run
        ]
      joinedCounterexamplesValue =
        [ object ["domain" .= show (joinedDomain run), "case" .= name, "difference" .= difference]
        | run <- joinedRuns
        , (name, difference) <- joinedCounterexamples run
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
          , "joinedCases" .= joinedCasesValue
          , "joinedReuse" .= joinedReuseValue
          , "joinedControls" .= joinedControlsValue
          , "joinedCounterexamples" .= joinedCounterexamplesValue
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
      casesText = unlines
        ( [show (familyDomain run) ++ " " ++ name ++ ": " ++ outcomeText result | run <- runs, (name, result) <- familyCases run]
            ++ [show (joinedDomain run) ++ " joined " ++ name ++ ": " ++ outcomeText result | run <- joinedRuns, (name, result) <- joinedRows run]
        )
      controlText = unlines
        ( [show (familyDomain run) ++ " control " ++ name ++ ": " ++ (if ok then "ok" else "failed") | run <- runs, (name, ok) <- familyControls run]
            ++ [show (joinedDomain run) ++ " joined control " ++ name ++ ": " ++ (if ok then "ok" else "failed") | run <- joinedRuns, (name, ok) <- joinedControls run]
        )
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
