{-# LANGUAGE OverloadedStrings #-}

-- | Runs the data-queue cases in both entry domains against the stock loaders and the owned
-- A32 entries, through the cached native batch, and writes a terminal and review JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Emulation.NativeObservation (parseWord32)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosDataQueue.Layout (parseFacts)
import Replacement.RtosDataQueue.Suite (CaseRun (..), Image (..), runDomain)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  cacheRoot <- required "--cache-root"
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  symbolsAPath <- required "--owned-a-symbols"
  symbolsBPath <- required "--owned-b-symbols"
  resultsPath <- required "--results"
  reviewPath <- required "--review"
  facts <- either failH pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  symbolsA <- parseOwnedSymbols symbolsAPath
  symbolsB <- parseOwnedSymbols symbolsBPath
  baseA <- ownedBase symbolsAPath
  baseB <- ownedBase symbolsBPath
  layoutDigest <- sha256File layoutPath
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  ownedADigest <- sha256File ownedA
  ownedBDigest <- sha256File ownedB
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  createDirectoryIfMissing True (takeDirectory reviewPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image baseA ownedABytes
      ownedBImage = Image baseB ownedBBytes
  runA <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage symbolsA
  runB <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage symbolsB
  let runs = runA ++ runB
      failures = [(domainLabel (runEntryDomain run), runName run, message) | run <- runs, message <- runDifferences run]
      caseValue run =
        object
          [ "domain" .= domainLabel (runEntryDomain run)
          , "case" .= runName run
          , "differences" .= runDifferences run
          , "reuse" .= show (runReuse run)
          ]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "inputs" .=
              object
                [ "layout_sha256" .= sha256Hex layoutDigest
                , "loader_a_sha256" .= sha256Hex loaderADigest
                , "loader_b_sha256" .= sha256Hex loaderBDigest
                , "owned_a_bin_sha256" .= sha256Hex ownedADigest
                , "owned_b_bin_sha256" .= sha256Hex ownedBDigest
                ]
          , "cases" .= map caseValue runs
          , "failures" .= length failures
          ]
      review =
        object
          [ "schema" .= (1 :: Int)
          , "kind" .= ("data-queue-native-review" :: String)
          , "inputs" .= object
              [ "layout" .= layoutPath
              , "loader_a" .= loaderA
              , "loader_b" .= loaderB
              , "owned_a" .= ownedA
              , "owned_b" .= ownedB
              ]
          , "counterexamples" .=
              [ object ["domain" .= domain, "case" .= name, "difference" .= message]
              | (domain, name, message) <- failures
              ]
          , "cases" .= map caseValue runs
          , "failures" .= length failures
          ]
  BL.writeFile resultsPath (encode document)
  BL.writeFile reviewPath (encode review)
  forM_ runs $ \run -> forM_ (runDifferences run) $ \message ->
    putStrLn (domainLabel (runEntryDomain run) ++ " " ++ runName run ++ ": " ++ message)
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  unless (null failures) exitFailure

parseOwnedSymbols :: FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, _] | kind == "T" || kind == "t" ->
        case (parseWord32 a, parseWord32 z) of
          (Just address, Just size) | size > 0 && address >= 0x40000 -> [(address, size)]
          _ -> []
      _ -> []

ownedBase :: FilePath -> IO Word32
ownedBase path = do
  rows <- lines <$> readFile path
  let addresses = [address | row <- rows, Just address <- [rowAddress row], address >= 0x40000, address < 0x01000000]
  case addresses of
    [] -> failH (path ++ " has no owned code symbols")
    _ -> pure (minimum addresses)
  where
    rowAddress row = case words row of
      [a, _, kind, _] | kind == "T" || kind == "t" -> parseWord32 a
      _ -> Nothing

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
