{-# LANGUAGE OverloadedStrings #-}

-- | Cyclic lifecycle and admission controls through the cached native batch.
-- Aggregate mode takes --aggregate-{a,b}-{bin,symbols,placements} and
-- --aggregate-carriers. Standalone cyclic/timer flags are ignored in that mode.
-- Both modes require --runner, --layout, --loader-{a,b} and --results; --cache-root
-- defaults to the shared private-data/cache/native. Repeated flags use their last value.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (tails)
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosCyclic.Layout (parseFacts)
import Replacement.RtosCyclic.Suite
import Replacement.RtosCyclic.Aggregate (bindAggregate, admissionControls)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  scope <- optional "--scope" >>= \value -> case value of
    Nothing -> pure FullScope
    Just "full" -> pure FullScope
    Just "smoke" -> pure CallbackSmoke
    Just other -> failH ("unknown cyclic scope: " ++ other)
  runner <- required "--runner" >>= makeAbsolute
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  aggregatePaths <- mapM optional ["--aggregate-a-bin", "--aggregate-a-symbols", "--aggregate-a-placements", "--aggregate-b-bin", "--aggregate-b-symbols", "--aggregate-b-placements", "--aggregate-carriers"]
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional "--cache-root"
  resultsPath <- required "--results"
  facts <- either failH pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  (inputA, inputB, inputDigests) <- case aggregatePaths of
    [Nothing, Nothing, Nothing, Nothing, Nothing, Nothing, Nothing] -> do
      let standalone domain binFlag symbolsFlag timerFlag timerSymbolsFlag timerBase = do
            bin <- required binFlag
            symbols <- required symbolsFlag
            timer <- required timerFlag
            timerSymbols <- required timerSymbolsFlag
            bytes <- B.readFile bin
            timerBytes <- B.readFile timer
            functions <- parseOwnedSymbols domain symbols
            timerFunctions <- parseOwnedSymbols domain timerSymbols
            unless (not (null functions) && not (null timerFunctions)) (failH "standalone images have no admitted functions")
            digest <- sha256File bin
            timerDigest <- sha256File timer
            pure (Standalone (Image (minimum (map fst functions)) bytes) (Image timerBase timerBytes) functions timerFunctions,
              [(binFlag, sha256Hex digest), (timerFlag, sha256Hex timerDigest)])
      (a, da) <- standalone DomainA "--owned-a-bin" "--owned-a-symbols" "--timer-a-bin" "--timer-a-symbols" 0x44a50
      (b, db) <- standalone DomainB "--owned-b-bin" "--owned-b-symbols" "--timer-b-bin" "--timer-b-symbols" 0xd4a10
      pure (a, b, da ++ db)
    [Just ab, Just aSymbols, Just ap, Just bb, Just bs, Just bp, Just carriers] -> do
      union <- either (failH . show) pure . Union.parseUnion =<< B.readFile carriers
      let bind domain stock bin symbols placements = do
            bytes <- B.readFile bin
            functions <- either failH pure . Agg.parseAggregateSymbols =<< B.readFile symbols
            sections <- either failH pure . Agg.parsePlacements =<< B.readFile placements
            either failH pure (bindAggregate domain stock bytes functions sections)
      a <- bind (Union.unionA union) (Image 0x40000 loaderABytes) ab aSymbols ap
      b <- bind (Union.unionB union) (Image 0xd0000 loaderBBytes) bb bs bp
      digests <- mapM (\path -> do digest <- sha256File path; pure (path, sha256Hex digest)) [ab, aSymbols, ap, bb, bs, bp, carriers]
      let input = Aggregate (a ++ b)
      pure (input, input, digests)
    _ -> failH "aggregate mode requires binary, symbols and placements for both domains and --aggregate-carriers"
  layoutDigest <- sha256File layoutPath
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  let provenance = [object ["input" .= name, "sha256" .= digest] | (name, digest) <- inputDigests]
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
  (runA, controlsA) <- runDomain runner scope cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA inputA
  (runB, controlsB) <- runDomain runner scope cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB inputB
  let runs = runA ++ runB
      controls = admissionControls ++ controlsA ++ controlsB
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
          , "scope" .= show scope
          , "inputs" .=
              object
                [ "layout_sha256" .= sha256Hex layoutDigest
                , "loader_a_sha256" .= sha256Hex loaderADigest
                , "loader_b_sha256" .= sha256Hex loaderBDigest
                , "replacement_inputs" .= provenance
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
required flag = optional flag >>= maybe (failH (flag ++ " is required")) pure

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case reverse [rest | name : rest <- tails arguments, name == flag] of
    [] -> pure Nothing
    (value : _) : _ | take 2 value /= "--" -> pure (Just value)
    _ -> failH (flag ++ " needs a value")
