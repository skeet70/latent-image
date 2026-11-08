{-# LANGUAGE OverloadedStrings #-}

-- | Runs the fixed-pool family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (readHex)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
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
  baseA <- origin DomainA ownedASymbols
  baseB <- origin DomainB ownedBSymbols
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image baseA ownedABytes
      ownedBImage = Image baseB ownedBBytes
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

parseSymbols :: B.ByteString -> M.Map String Word32
parseSymbols bytes = M.fromList
  [ (name, value)
  | line <- lines (BC.unpack bytes)
  , address : _ : _ : name : _ <- [words line]
  , [(value, "")] <- [readHex address]
  ]

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
