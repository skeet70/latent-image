{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256File, sha256Hex)
import Data.Word (Word32)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosIrqControl.Suite
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  symbolsA <- required "--owned-a-symbols"
  symbolsB <- required "--owned-b-symbols"
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional "--cache-root"
  resultsPath <- required "--results"
  batchRoot <- do
    work <- lookupEnv "XM1_RUN_WORK"
    pure $ case work of
      Just directory | not (null directory) -> directory </> "irq-control"
      _ -> takeDirectory resultsPath </> "batches"
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  ownedASymbols <- parseOwnedSymbols DomainA symbolsA
  ownedBSymbols <- parseOwnedSymbols DomainB symbolsB
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True batchRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image (minimum (map fst ownedASymbols)) ownedABytes
      ownedBImage = Image (minimum (map fst ownedBSymbols)) ownedBBytes
  runA <- runSuite runner cacheRoot batchRoot DomainA stockA ownedAImage ownedASymbols
  runB <- runSuite runner cacheRoot batchRoot DomainB stockB ownedBImage ownedBSymbols
  let runs = [runA, runB]
      failures =
        [ (domainLabel (suiteDomain run), name, message)
        | run <- runs
        , (name, Left message) <- suiteCases run
        ]
          ++ [ (domainLabel (suiteDomain run), name, unlines differences)
             | run <- runs
             , (name, Right differences) <- suiteCases run
             , not (null differences)
             ]
      cases =
        [ object
            [ "domain" .= domainLabel (suiteDomain run)
            , "case" .= name
            , "result" .= outcomeText result
            ]
        | run <- runs
        , (name, result) <- suiteCases run
        ]
      reuse =
        [ object ["domain" .= domainLabel (suiteDomain run), "stock" .= show (fst (suiteReuse run)), "owned" .= show (snd (suiteReuse run))]
        | run <- runs
        ]
      document = object ["schema" .= (1 :: Int), "cases" .= cases, "reuse" .= reuse, "failures" .= length failures]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (suiteCases run) $ \(name, result) ->
    putStrLn (domainLabel (suiteDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  unless (null failures) exitFailure
  where
    outcomeText (Left message) = "failed: " ++ message
    outcomeText (Right []) = "ok"
    outcomeText (Right differences) = "failed: " ++ unlines differences

parseOwnedSymbols :: Domain -> FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols domain path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    low = if domain == DomainA then 0x41108 else 0x50000
    high = if domain == DomainA then 0x52000 else 0xdd93c
    row line = case words line of
      [a, z, kind, _] -> case (N.parseWord32 a, N.parseWord32 z) of
        (Just address, Just size)
          | kind `elem` ["T", "t"], size > 0, address >= low, address + size <= high -> [(address, size)]
        _ -> []
      _ -> []

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> pure Nothing
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure (Just value)
