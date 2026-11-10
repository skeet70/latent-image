{-# LANGUAGE OverloadedStrings #-}

-- | Runs the semaphore family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosSemaphore.Oracle (Domain (..), Image (..), Run (..), extents, parseFacts, runCases, runControls, runDomain, runFamily, runReuse)

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
  symbolsAPath <- optional "--symbols-a"
  symbolsBPath <- optional "--symbols-b"
  placementsAPath <- optional "--placements-a"
  placementsBPath <- optional "--placements-b"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  symbolsA <- traverse (readSymbols) symbolsAPath
  symbolsB <- traverse (readSymbols) symbolsBPath
  placementsA <- traverse readPlacements placementsAPath
  placementsB <- traverse readPlacements placementsBPath
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes [] []
      stockB = Image 0xd0000 loaderBBytes [] []
      ownedAImage = ownedImage DomainA ownedABytes symbolsA placementsA
      ownedBImage = ownedImage DomainB ownedBBytes symbolsB placementsB
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- runCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- runCases run, not (null differences)]
          ++ [(name, "semantic control did not reject corruption") | run <- runs, (name, False) <- runControls run]
      cases =
        [ object
            [ "domain" .= show (runDomain run)
            , "case" .= name
            , "result" .= outcomeText result
            ]
        | run <- runs
        , (name, result) <- runCases run
        ]
      outcomeText (Left message) = "failed: " ++ message
      outcomeText (Right []) = "ok"
      outcomeText (Right differences) = "failed: " ++ unlines differences
      reuse =
        [ object ["domain" .= show (runDomain run), "stock" .= show (fst (runReuse run)), "owned" .= show (snd (runReuse run))]
        | run <- runs
        ]
      controls = [object ["domain" .= show (runDomain run), "control" .= name, "holds" .= passed] | run <- runs, (name, passed) <- runControls run]
      document = object ["cases" .= cases, "controls" .= controls, "reuse" .= reuse, "failures" .= length failures]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (runCases run) $ \(name, result) ->
    putStrLn (show (runDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

ownedImage :: Domain -> B.ByteString -> Maybe [Agg.AggregateSymbol] -> Maybe [Agg.SectionPlacement] -> Image
ownedImage domain bytes Nothing _ = Image (minimum (map fst (extents domain))) bytes [] []
ownedImage _ bytes (Just symbols) placements =
  let image = either error id (Agg.aggregateImage symbols bytes)
      selected = symbols
   in Image (Agg.imageBase image) (Agg.imageBytes image) selected (maybe [] id placements)

readSymbols :: FilePath -> IO [Agg.AggregateSymbol]
readSymbols path = either fail pure . Agg.parseAggregateSymbols =<< B.readFile path

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either fail pure . Agg.parsePlacements =<< B.readFile path

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> pure Nothing
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> Just <$> makeAbsolute value

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
