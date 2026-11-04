{-# LANGUAGE OverloadedStrings #-}

-- | Runs every own-first slot's case grid against the stock body from the pinned loaders and against
-- the owned body, compares them case by case and runs the controls. Exits 1 when a control
-- misbehaves and 2 when an owned body differs from stock.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.String (fromString)
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, failH, readRegular)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..), loaderSha256)
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (takeDirectory, (</>))

digest :: B.ByteString -> String
digest = sha256Hex . sha256

orFail :: Show e => String -> Either e a -> IO a
orFail context = either (\problem -> failH (context ++ ": " ++ show problem)) pure

orFailText :: String -> Either String a -> IO a
orFailText context = either (\problem -> failH (context ++ ": " ++ problem)) pure

slotText :: Slot -> String
slotText slot = case slotOffset slot of SlotOffset offset -> hexText offset

differenceValue :: (CaseKey, [Difference]) -> Value
differenceValue (CaseKey index, differences) = object ["row" .= index, "differences" .= map differenceText differences]

data ControlResult = ControlResult
  { resultControl :: String
  , resultExpectation :: Expectation
  , resultNote :: String
  , resultDomain :: Domain
  , resultSlot :: Slot
  , resultOutcome :: Outcome
  }

holds :: ControlResult -> Bool
holds result = case resultExpectation result of
  MustPass -> outcomeCases outcome > 0 && outcomeMatches outcome == outcomeCases outcome
  MustFail -> outcomeMatches outcome < outcomeCases outcome
  where
    outcome = resultOutcome result

expectationText :: Expectation -> String
expectationText expectation = case expectation of
  MustPass -> "must-pass"
  MustFail -> "must-fail"

controlValue :: ControlResult -> Value
controlValue result =
  object
    [ "name" .= resultControl result
    , "expectation" .= expectationText (resultExpectation result)
    , "note" .= resultNote result
    , "domain" .= domainLabel (resultDomain result)
    , "slot" .= slotText (resultSlot result)
    , "cases" .= outcomeCases (resultOutcome result)
    , "matches" .= outcomeMatches (resultOutcome result)
    , "first_difference" .= fmap differenceValue (outcomeFirst (resultOutcome result))
    , "holds" .= holds result
    ]

main :: IO ()
main = do
  arguments <- getArgs
  let input name = required arguments name >>= canonicalizePath
      output name = required arguments name >>= makeAbsolute
  runner <- input "--runner"
  regionsPath <- input "--regions"
  ownedPaths <- mapM input ["--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  loaderPaths <- mapM input ["--loader-a", "--loader-b"]
  work <- output "--work"
  summaryPath <- output "--summary"
  forM_ [work, summaryPath] $ \path -> do
    exists <- doesPathExist path
    when exists (failH (path ++ " already exists"))
  runnerBytes <- readRegular 0x10000000 runner
  regionsBytes <- readRegular 0x100000 regionsPath
  ownedBytes <- mapM (readRegular 0x100000) ownedPaths
  loaders <- mapM (readRegular 0x20000) loaderPaths
  forM_ (zip domains loaders) $ \(domain, loader) ->
    ensure (digest loader == loaderSha256 domain) ("--loader-" ++ domainLabel domain ++ " is not the pinned X-M1 1.50 loader")
  table <- orFailText "regions.json" (parseRegionTable regionsBytes)
  owned <- case ownedBytes of
    [binA, symbolsA, binB, symbolsB] -> forM (zip domains [LinkedDomain binA symbolsA, LinkedDomain binB symbolsB]) $ \(domain, linked) ->
      orFail ("owned " ++ domainLabel domain) (ownedDomain (tableFill table) domain (domainTable table domain) linked)
    _ -> failH "unreachable owned inputs"
  createDirectoryIfMissing True (takeDirectory work)
  createDirectory work
  rows <- fmap concat . forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
    let rowsTable = domainTable table domain
    forM slots $ \slot -> do
      stock <- orFailText (slotTag slot ++ " stock") (stockBody domain rowsTable loader slot)
      ownedSlot <- orFailText (slotTag slot ++ " owned") (ownedBody ownedRows slot)
      entry <- orFailText (slotTag slot ++ " entry") (slotEntry rowsTable slot)
      let directory role = work </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
          run role = runSlot runner (directory role) role domain slot
      stockRuns <- run "stock" stock
      ownedOutcome <- outcomeOf stockRuns <$> run "owned" ownedSlot
      repeatOutcome <- outcomeOf stockRuns <$> run "stock-repeat" stock
      controlResults <- forM [control | control <- equivalents ++ mutants, controlSlot control == slot] $ \control -> do
        code <- orFailText (controlName control) (controlCode domain rowsTable control)
        body <- orFailText (controlName control) (replaceEntry (tableFill table) code ownedSlot)
        ControlResult (controlName control) (controlExpectation control) (controlNote control) domain slot . outcomeOf stockRuns
          <$> run (controlName control) body
      let results = ControlResult "stock-repeat" MustPass "the stock body against itself" domain slot repeatOutcome : controlResults
      when (outcomeMatches ownedOutcome == outcomeCases ownedOutcome && all holds results) $
        forM_ ("stock" : "owned" : map resultControl results) $ \role -> do
          present <- doesPathExist (directory role)
          when present (removeDirectoryRecursive (directory role))
      pure (domain, slot, entry, ownedOutcome, results)
  let controlResults = concat [results | (_, _, _, _, results) <- rows]
      controlsHold = all holds controlResults
      ownedMatches = and [outcomeMatches outcome == outcomeCases outcome | (_, _, _, outcome, _) <- rows]
      inputs =
        object
          ( [ "regions_sha256" .= digest regionsBytes
            , "runner_sha256" .= digest runnerBytes
            ]
              ++ [fromString ("loader_" ++ domainLabel domain ++ "_sha256") .= digest loader | (domain, loader) <- zip domains loaders]
              ++ [fromString name .= digest bytes | (name, bytes) <- zip ["owned_a_bin_sha256", "owned_a_symbols_sha256", "owned_b_bin_sha256", "owned_b_symbols_sha256"] ownedBytes]
          )
      domainValue domain =
        object
          [ "domain" .= domainLabel domain
          , "slots" .=
              [ object
                  [ "slot" .= slotText slot
                  , "entry" .= hexText entry
                  , "cases" .= outcomeCases outcome
                  , "matches" .= outcomeMatches outcome
                  , "first_mismatch" .= fmap differenceValue (outcomeFirst outcome)
                  ]
              | (rowDomain, slot, entry, outcome, _) <- rows
              , rowDomain == domain
              ]
          ]
  createDirectoryIfMissing True (takeDirectory summaryPath)
  B.writeFile summaryPath $
    encodeCanonical $
      object
        [ "schema" .= (1 :: Int)
        , "inputs" .= inputs
        , "domains" .= map domainValue domains
        , "controls" .= map controlValue controlResults
        , "controls_hold" .= controlsHold
        , "owned_matches_stock" .= ownedMatches
        ]
  unless controlsHold (exitWith (ExitFailure 1))
  unless ownedMatches (exitWith (ExitFailure 2))
