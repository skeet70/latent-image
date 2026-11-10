{-# LANGUAGE OverloadedStrings #-}

-- | Runs the task-record cases in both domains against the stock loaders and the owned native
-- entries through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTaskRecords.Layout (parseFacts)
import Replacement.RtosTaskRecords.Suite
import qualified Replacement.RtosPlatform.Aggregate as Agg
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
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  symbolsA <- required "--owned-a-symbols"
  symbolsB <- required "--owned-b-symbols"
  cacheRoot <- required "--cache-root"
  resultsPath <- required "--results"
  facts <- either failH pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  ownedASymbols <- parseOwnedSymbols symbolsA
  ownedBSymbols <- parseOwnedSymbols symbolsB
  placementsA <- optionalPlacements "--placements-a"
  placementsB <- optionalPlacements "--placements-b"
  forM_ [(ownedABytes, ownedASymbols, placementsA), (ownedBBytes, ownedBSymbols, placementsB)] $ \(bytes, symbols, placements) ->
    when (any (\(name, _, _) -> "xm1_platform_veneer_" `isPrefixOf` name) symbols) $ do
      when (null placements) (failH "aggregate images require their placement manifests")
      let image = Agg.AggregateImage (minimum [address | (_, address, _) <- symbols]) bytes
          extents = [(address, size) | (_, address, size) <- symbols]
            ++ [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- placements]
      forM_ extents $ \(address, size) -> do
        _ <- either failH pure (Agg.sliceAggregate image address size)
        pure ()
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
  let stockA = Image 0x40000 loaderABytes []
      stockB = Image 0xd0000 loaderBBytes []
      ownedAImage = Image (minimum [address | (_, address, _) <- ownedASymbols]) ownedABytes placementsA
      ownedBImage = Image (minimum [address | (_, address, _) <- ownedBSymbols]) ownedBBytes placementsB
  (runA, controlsA) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage ownedASymbols
  (runB, controlsB) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage ownedBSymbols
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
  let smoke = [run | run <- runs, runCohort run == "smoke"]
      smokeHolds = not (null smoke) && all (\run -> null (runDifferences run) && all snd (runChecks run)) smoke
  putStrLn ("task-records smoke: " ++ show (length smoke) ++ " emitted create/delete cases " ++ (if smokeHolds then "returned and matched" else "FAILED") ++ "; summary " ++ resultsPath)
  unless (null failures && null failedChecks && null failedControls && smokeHolds) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case [value | (key, value) <- zip arguments (drop 1 arguments), key == flag] of
    [] -> failH (flag ++ " is required")
    values -> pure (last values)

optionalPlacements :: String -> IO [Agg.SectionPlacement]
optionalPlacements flag = do
  arguments <- getArgs
  case [value | (key, value) <- zip arguments (drop 1 arguments), key == flag] of
    [] -> pure []
    values -> either failH pure . Agg.parsePlacements =<< B.readFile (last values)
