{-# LANGUAGE OverloadedStrings #-}

-- | Runs the global-lock cases in both domains against the stock loaders and the owned
-- A32 entries, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosGlobalLock.Suite
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import qualified Replacement.RtosPlatform.Union as Union
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  aggregateFlags <- mapM optional
    ["--aggregate-a-bin", "--aggregate-a-symbols", "--aggregate-a-placements",
     "--aggregate-b-bin", "--aggregate-b-symbols", "--aggregate-b-placements",
     "--aggregate-carriers"]
  (ownedA, ownedB, ownedAImage, ownedBImage, ownedASymbols, ownedBSymbols, aggregates) <- case aggregateFlags of
    [Nothing, Nothing, Nothing, Nothing, Nothing, Nothing, Nothing] -> do
      a <- required "--owned-a-bin"
      b <- required "--owned-b-bin"
      sa <- required "--owned-a-symbols" >>= parseOwnedSymbols DomainA
      sb <- required "--owned-b-symbols" >>= parseOwnedSymbols DomainB
      unless (not (null sa) && not (null sb)) (failH "owned symbols are empty")
      ba <- B.readFile a
      bb <- B.readFile b
      pure (a, b, Image (minimum (map fst sa)) ba, Image (minimum (map fst sb)) bb, sa, sb, [])
    [Just a, Just sa, Just pa, Just b, Just sb, Just pb, Just carriers] -> do
      union <- either (failH . show) pure . Union.parseUnion =<< B.readFile carriers
      imageA <- readAggregate DomainA union a sa pa
      imageB <- readAggregate DomainB union b sb pb
      pure (a, b, aggregateImage imageA, aggregateImage imageB,
        aggregateSymbols imageA, aggregateSymbols imageB, [imageA, imageB])
    _ -> failH "aggregate mode requires both binary/symbol/placement triples and --aggregate-carriers"
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional "--cache-root"
  resultsPath <- required "--results"
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  ownedADigest <- sha256File ownedA
  ownedBDigest <- sha256File ownedB
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
  (runA, controlsA) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-a") DomainA stockA ownedAImage ownedASymbols aggregates
  (runB, controlsB) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-b") DomainB stockB ownedBImage ownedBSymbols aggregates
  let runs = runA ++ runB
      controls = controlsA ++ controlsB
      failures =
        [(domainLabel (caseDomain run), runName run, message) | run <- runs, message <- runDifferences run]
      failedChecks =
        [(domainLabel (caseDomain run), runName run, description) | run <- runs, (description, False) <- runChecks run]
      failedControls =
        [(controlCase control, controlName control) | control <- controls, not (controlHolds control)]
      caseValue run =
        object
          [ "domain" .= domainLabel (caseDomain run)
          , "case" .= runName run
          , "checks" .= [object ["check" .= description, "holds" .= holds] | (description, holds) <- runChecks run]
          , "differences" .= runDifferences run
          , "reuse" .= show (runReuse run)
          ]
      controlValue control =
        object
          [ "case" .= controlCase control
          , "control" .= controlName control
          , "holds" .= controlHolds control
          ]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "inputs" .=
              object
                [ "loader_a_sha256" .= sha256Hex loaderADigest
                , "loader_b_sha256" .= sha256Hex loaderBDigest
                , "owned_a_bin_sha256" .= sha256Hex ownedADigest
                , "owned_b_bin_sha256" .= sha256Hex ownedBDigest
                ]
          , "cases" .= map caseValue runs
          , "controls" .= map controlValue controls
          , "failures" .= (length failures + length failedChecks + length failedControls)
          ]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (runChecks run) $ \(description, holds) ->
    putStrLn (domainLabel (caseDomain run) ++ " " ++ runName run ++ ": " ++ description ++ " " ++ (if holds then "ok" else "FAILED"))
  forM_ controls $ \control ->
    putStrLn (controlCase control ++ ": control " ++ controlName control ++ " " ++ (if controlHolds control then "ok" else "FAILED"))
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  forM_ failedChecks $ \(domain, name, description) -> putStrLn (domain ++ " " ++ name ++ ": check failed: " ++ description)
  forM_ failedControls $ \(name, control) -> putStrLn (name ++ ": control failed: " ++ control)
  unless (null failures && null failedChecks && null failedControls) exitFailure

required :: String -> IO FilePath
required flag = optional flag >>= maybe (failH (flag ++ " is required")) pure

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case [drop (index + 1) arguments | (index, argument) <- zip [0 :: Int ..] arguments, argument == flag] of
    [] -> pure Nothing
    occurrences -> case last occurrences of
      value : _ | not ("--" `isPrefixOf` value) -> pure (Just value)
      _ -> failH (flag ++ " needs a value")
