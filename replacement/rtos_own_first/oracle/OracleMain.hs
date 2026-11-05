{-# LANGUAGE OverloadedStrings #-}

-- | Runs every own-first slot's case grid against the stock body from the pinned loaders and against
-- the owned body, compares them case by case and runs the controls. Exits 1 when a stock run stops
-- or a control misbehaves, and 2 when an owned body differs from stock.
module Main (main) where

import Control.Monad (forM, forM_, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, failH, readRegular)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..), loaderSha256)
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions
import Replacement.RtosOwnFirst.Verdict
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

statusesValue :: M.Map String Int -> Value
statusesValue counts = object [fromString status .= count | (status, count) <- M.toList counts]

expectationText :: Expectation -> String
expectationText expectation = case expectation of
  MustPass -> "must-pass"
  MustFail -> "must-fail"

controlValue :: SlotReport -> ControlResult -> Value
controlValue report result =
  object
    [ "name" .= resultControl result
    , "expectation" .= expectationText (resultExpectation result)
    , "note" .= resultNote result
    , "domain" .= domainLabel (reportDomain report)
    , "slot" .= slotText (reportSlot report)
    , "cases" .= outcomeCases outcome
    , "matches" .= outcomeMatches outcome
    , "differing_returned" .= outcomeDifferingReturned outcome
    , "differing_stopped" .= outcomeDifferingStopped outcome
    , "statuses" .= statusesValue (outcomeStatuses outcome)
    , "first_difference" .= fmap differenceValue (outcomeFirst outcome)
    , "holds" .= holds result
    ]
  where
    outcome = resultOutcome result

slotValue :: SlotReport -> Value
slotValue report =
  object
    [ "slot" .= slotText (reportSlot report)
    , "entry" .= hexText (reportEntry report)
    , "stock_statuses" .= statusesValue (statusCounts (reportStock report))
    , "stock_returned" .= stockHolds report
    , "owned_statuses" .= statusesValue (outcomeStatuses owned)
    , "cases" .= outcomeCases owned
    , "matches" .= outcomeMatches owned
    , "first_mismatch" .= fmap differenceValue (outcomeFirst owned)
    ]
  where
    owned = reportOwned report

unlessNull :: [a] -> IO () -> IO ()
unlessNull values action = when (not (null values)) action

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
  reports <- fmap concat . forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
    let rowsTable = domainTable table domain
    forM slots $ \slot -> do
      stock <- orFailText (slotTag slot ++ " stock") (stockBody domain rowsTable loader slot)
      ownedSlot <- orFailText (slotTag slot ++ " owned") (ownedBody ownedRows slot)
      entry <- orFailText (slotTag slot ++ " entry") (slotEntry rowsTable slot)
      let directory role = work </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
          run role = runSlot runner (directory role) role domain slot
      stockRuns <- run "stock" stock
      ownedRuns <- run "owned" ownedSlot
      let ownedOutcome = outcomeOf stockRuns ownedRuns
          evidence observed = object
            [ "status" .= show (runStatus observed)
            , "registers" .= [(show register, hexText value) | (register, value) <- runRegisters observed]
            , "cpsr_control" .= hexText (runControl observed)
            , "memory" .= [(show ident, B.unpack bytes) | (ident, bytes) <- runMemory observed]
            , "writes" .= show (runWrites observed)
            ]
          differences =
            [ object
                [ "row" .= index
                , "case" .= show row
                , "differences" .= map differenceText delta
                , "stock" .= evidence reference
                , "owned" .= evidence subject
                ]
            | (CaseKey index, row) <- zip (map CaseKey [0 ..]) (caseGrid slot)
            , Just reference <- [M.lookup (CaseKey index) stockRuns]
            , Just subject <- [M.lookup (CaseKey index) ownedRuns]
            , let delta = compareRuns reference subject
            , not (null delta)
            ]
      unlessNull differences $
        B.writeFile (work </> "differences-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ ".json") (encodeCanonical (object ["cases" .= differences]))
      repeatOutcome <- outcomeOf stockRuns <$> run "stock-repeat" stock
      controlResults <- forM [control | control <- equivalents ++ mutants, controlSlot control == slot] $ \control -> do
        code <- orFailText (controlName control) (controlCode domain rowsTable control)
        body <- orFailText (controlName control) (replaceEntry (tableFill table) code ownedSlot)
        ControlResult (controlName control) (controlExpectation control) (controlNote control) . outcomeOf stockRuns
          <$> run (controlName control) body
      let report = SlotReport domain slot entry stockRuns ownedOutcome (ControlResult "stock-repeat" MustPass "the stock body against itself" repeatOutcome : controlResults)
      when (verdictExit [report] == ExitSuccess) $
        forM_ ("stock" : "owned" : map resultControl (reportControls report)) $ \role -> do
          present <- doesPathExist (directory role)
          when present (removeDirectoryRecursive (directory role))
      pure report
  let exit = verdictExit reports
      inputs =
        object
          ( [ "regions_sha256" .= digest regionsBytes
            , "runner_sha256" .= digest runnerBytes
            ]
              ++ [fromString ("loader_" ++ domainLabel domain ++ "_sha256") .= digest loader | (domain, loader) <- zip domains loaders]
              ++ [fromString name .= digest bytes | (name, bytes) <- zip ["owned_a_bin_sha256", "owned_a_symbols_sha256", "owned_b_bin_sha256", "owned_b_symbols_sha256"] ownedBytes]
          )
      domainValue domain = object ["domain" .= domainLabel domain, "slots" .= [slotValue report | report <- reports, reportDomain report == domain]]
  createDirectoryIfMissing True (takeDirectory summaryPath)
  B.writeFile summaryPath $
    encodeCanonical $
      object
        [ "schema" .= (1 :: Int)
        , "inputs" .= inputs
        , "domains" .= map domainValue domains
        , "controls" .= [controlValue report result | report <- reports, result <- reportControls report]
        , "stock_returned" .= all stockHolds reports
        , "controls_hold" .= all (all holds . reportControls) reports
        , "owned_matches_stock" .= all ownedMatches reports
        ]
  exitWith exit
