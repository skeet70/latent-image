{-# LANGUAGE OverloadedStrings #-}

-- | Qualify timer ABI cases and controls over pinned stock loaders.
-- Aggregate mode takes --aggregate-{a,b}-{bin,symbols,placements} and
-- --aggregate-carriers. Standalone mode takes --owned-{a,b}-{bin,symbols}.
-- Both modes require --runner, --layout, --loader-a, --loader-b, --cache-root
-- and --results. The last occurrence of an option overrides Bazel defaults.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTimer.Layout (parseFacts)
import Replacement.RtosTimer.Suite
import Replacement.RtosTimer.Aggregate (Aggregate, loadAggregate)
import qualified Replacement.RtosPlatform.Union as Union
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  aggregatePaths <- mapM optional
    [ "--aggregate-a-bin", "--aggregate-a-symbols", "--aggregate-a-placements"
    , "--aggregate-b-bin", "--aggregate-b-symbols", "--aggregate-b-placements"
    , "--aggregate-carriers"
    ]
  (ownedAImage, ownedBImage, ownedASymbols, ownedBSymbols, aggregates, ownedAPath, ownedBPath) <-
    loadInputs aggregatePaths
  cacheRoot <- required "--cache-root"
  resultsPath <- required "--results"
  facts <- either failH pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedADigest <- sha256File ownedAPath
  ownedBDigest <- sha256File ownedBPath
  layoutDigest <- sha256File layoutPath
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
  (runA, controlsA) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage ownedASymbols aggregates
  (runB, controlsB) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage ownedBSymbols aggregates
  let runs = runA ++ runB
      controls = controlsA ++ controlsB
      failures =
        [ (domainLabel (caseDomain run), runName run, message)
        | run <- runs
        , message <- runDifferences run
        ]
      failedChecks =
        [ (domainLabel (caseDomain run), runName run, description)
        | run <- runs
        , (description, False) <- runChecks run
        ]
      failedControls =
        [ (controlCohort control, controlCase control, controlName control)
        | control <- controls
        , not (controlHolds control)
        ]
      caseValue run =
        object
          [ "domain" .= domainLabel (caseDomain run)
          , "cohort" .= runCohort run
          , "case" .= runName run
          , "kind" .= show (runKind run)
          , "checks" .= [object ["check" .= description, "holds" .= holds] | (description, holds) <- runChecks run]
          , "differences" .= runDifferences run
          , "reuse" .= show (runReuse run)
          ]
      controlValue control =
        object
          [ "cohort" .= controlCohort control
          , "case" .= controlCase control
          , "control" .= controlName control
          , "holds" .= controlHolds control
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
          , "controls" .= map controlValue controls
          , "failures" .= (length failures + length failedChecks + length failedControls)
          ]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (runChecks run) $ \(description, holds) ->
    putStrLn (domainLabel (caseDomain run) ++ " " ++ runCohort run ++ " " ++ runName run ++ ": " ++ description ++ " " ++ (if holds then "ok" else "FAILED"))
  forM_ controls $ \control ->
    putStrLn (controlCohort control ++ " " ++ controlCase control ++ ": control " ++ controlName control ++ " " ++ (if controlHolds control then "ok" else "FAILED"))
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  forM_ failedChecks $ \(domain, name, description) -> putStrLn (domain ++ " " ++ name ++ ": check failed: " ++ description)
  forM_ failedControls $ \(cohort, name, control) -> putStrLn (cohort ++ " " ++ name ++ ": control failed: " ++ control)
  unless (null failures && null failedChecks && null failedControls) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case reverse (argumentValues flag arguments) of
    [] -> failH (flag ++ " is required and needs a value")
    value : _ -> pure value

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case reverse (argumentValues flag arguments) of
    [] | flag `elem` arguments -> failH (flag ++ " needs a value")
       | otherwise -> pure Nothing
    value : _ -> pure (Just value)

argumentValues :: String -> [String] -> [String]
argumentValues _ [] = []
argumentValues _ [_] = []
argumentValues flag (name : value : rest)
  | name == flag = value : argumentValues flag rest
  | otherwise = argumentValues flag (value : rest)

loadInputs :: [Maybe FilePath] -> IO (Image, Image, [(Word32, Word32)], [(Word32, Word32)], [Aggregate], FilePath, FilePath)
loadInputs [Just a, Just sa, Just pa, Just b, Just sb, Just pb, Just carriers] = do
  union <- either (failH . show) pure . Union.parseUnion =<< B.readFile carriers
  aggregateA <- loadAggregate fixtureCodeExtents (Union.unionA union) a sa pa
  aggregateB <- loadAggregate fixtureCodeExtents (Union.unionB union) b sb pb
  pure (Image 0 B.empty, Image 0 B.empty, [], [], [aggregateA, aggregateB], a, b)
loadInputs paths | all (== Nothing) paths = do
  a <- required "--owned-a-bin"
  b <- required "--owned-b-bin"
  sa <- required "--owned-a-symbols" >>= parseOwnedSymbols
  sb <- required "--owned-b-symbols" >>= parseOwnedSymbols
  unless (not (null sa) && not (null sb)) (failH "standalone timer symbols are empty")
  ba <- B.readFile a
  bb <- B.readFile b
  pure (Image (minimum (map fst sa)) ba, Image (minimum (map fst sb)) bb, sa, sb, [], a, b)
loadInputs _ = failH "aggregate mode requires binaries, symbols, placements for both domains and --aggregate-carriers"
