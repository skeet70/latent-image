{-# LANGUAGE OverloadedStrings #-}

-- | Runs the fixed-pool family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosFixedPool.Oracle (Domain (..), Image (..), Run (..), Service (..), bodyRegion, entries, extents, parseFacts, runFamily)

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
  placementsAPath <- optional "--placements-a"
  placementsBPath <- optional "--placements-b"
  symbolsA <- required "--owned-a-symbols"
  symbolsB <- required "--owned-b-symbols"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  ownedASymbols <- parseSymbols <$> B.readFile symbolsA
  ownedBSymbols <- parseSymbols <$> B.readFile symbolsB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  placementsA <- traverse readPlacements placementsAPath
  placementsB <- traverse readPlacements placementsBPath
  ownedAImage <- ownedImage DomainA ownedABytes ownedASymbols placementsA
  ownedBImage <- ownedImage DomainB ownedBBytes ownedBSymbols placementsB
  let stockA = Image 0x40000 loaderABytes [] []
      stockB = Image 0xd0000 loaderBBytes [] []
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- runCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- runCases run, not (null differences)]
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
      document = object ["cases" .= cases, "reuse" .= reuse, "failures" .= length failures]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (runCases run) $ \(name, result) ->
    putStrLn (show (runDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

origin :: Domain -> M.Map String Word32 -> IO Word32
origin domain symbols = do
  let expected = minimum (bodyRegion : map fst (extents domain))
      entrySymbol = "xm1_rtos_fixed_pool_poll_unchecked"
      entryAddress = entries domain PollUnchecked
  case M.lookup entrySymbol symbols of
    Just address | address == entryAddress -> pure ()
    Just address -> failH (show domain ++ " owned entry is at " ++ show address ++ ", not " ++ show entryAddress)
    Nothing -> failH (show domain ++ " owned symbols lack " ++ entrySymbol)
  forM_ (expectedEntries domain) $ \(address, name) ->
    case M.lookup name symbols of
      Just actual | actual == address -> pure ()
      Just actual -> failH (show domain ++ " " ++ name ++ " is at " ++ show actual ++ ", not " ++ show address)
      Nothing -> failH (show domain ++ " owned symbols lack " ++ name)
  forM_ bodySymbols $ \name ->
    case M.lookup name symbols of
      Just address | address >= bodyRegion -> pure ()
      Just address -> failH (show domain ++ " " ++ name ++ " is at " ++ show address ++ ", below the body region")
      Nothing -> failH (show domain ++ " owned symbols lack " ++ name)
  pure expected

bodySymbols :: [String]
bodySymbols =
  [ "xm1_rtos_fixed_pool_create_body"
  , "xm1_rtos_fixed_pool_delete_body"
  , "xm1_rtos_fixed_pool_get_body"
  , "xm1_rtos_fixed_pool_poll_body"
  , "xm1_rtos_fixed_pool_timed_get_body"
  , "xm1_rtos_fixed_pool_release_body"
  , "xm1_rtos_fixed_pool_poll_unchecked_body"
  ]

expectedEntries :: Domain -> [(Word32, String)]
expectedEntries domain =
  [ (entries domain Create, "xm1_rtos_fixed_pool_create")
  , (entries domain Delete, "xm1_rtos_fixed_pool_delete")
  , (entries domain Get, "xm1_rtos_fixed_pool_get")
  , (entries domain Poll, "xm1_rtos_fixed_pool_poll")
  , (entries domain TimedGet, "xm1_rtos_fixed_pool_timed_get")
  , (entries domain Release, "xm1_rtos_fixed_pool_release")
  , (entries domain PollUnchecked, "xm1_rtos_fixed_pool_poll_unchecked")
  ]

parseSymbols :: B.ByteString -> [Agg.AggregateSymbol]
parseSymbols = either error id . Agg.parseAggregateSymbols

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either failH pure . Agg.parsePlacements =<< B.readFile path

ownedImage :: Domain -> B.ByteString -> [Agg.AggregateSymbol] -> Maybe [Agg.SectionPlacement] -> IO Image
ownedImage domain bytes symbols placements
  | Just sections <- placements = do
      unless (not (null sections)) (failH "aggregate placements are empty")
      image <- either failH pure (Agg.aggregateImage symbols bytes)
      forM_ sections $ \section -> do
        _ <- either failH pure (Agg.sliceAggregate image (Agg.placementAddress section) (Agg.placementBytes section))
        pure ()
      pure (Image (Agg.imageBase image) (Agg.imageBytes image) symbols sections)
  | any (isAggregate . Agg.symbolName) symbols =
      failH "aggregate fixed-pool qualification requires --placements-a and --placements-b"
  | otherwise = do
      base <- origin domain (M.fromList [(Agg.symbolName symbol, Agg.symbolAddress symbol) | symbol <- symbols])
      pure (Image base bytes [] [])
  where
    isAggregate name = take 20 name == "xm1_platform_veneer_"

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case reverse [drop (index + 1) arguments | (index, argument) <- zip [0 :: Int ..] arguments, argument == flag] of
    [] -> pure Nothing
    [] : _ -> failH (flag ++ " needs a value")
    (value : _) : _ -> Just <$> makeAbsolute value

required :: String -> IO FilePath
required flag = optional flag >>= maybe (failH (flag ++ " is required")) pure
