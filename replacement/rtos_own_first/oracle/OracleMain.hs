{-# LANGUAGE OverloadedStrings #-}

-- | Runs every own-first slot's case grid against the stock body from the pinned loaders and against
-- the owned body, compares them case by case and runs the controls. Exits 1 when a stock run stops
-- or a control misbehaves, and 2 when an owned body differs from stock.
module Main (main) where

import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (Null), object, (.=))
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, failH, readRegular)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..), loaderSha256)
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Generated
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions
import Replacement.RtosOwnFirst.Shrink
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

data Generation = Generation
  { generationSeed :: GenerationSeed
  , generationCount :: GenerationCount
  , generationBudget :: ShrinkBudget
  , generationControl :: Control
  }

optionalArgument :: [String] -> String -> Maybe String
optionalArgument arguments name = case dropWhile (/= name) arguments of
  (_ : value : _) -> Just value
  _ -> Nothing

-- | The generated corpus is the settled +0x16c leaf and its hand-written control.
parseGeneration :: [String] -> Either String (Maybe Generation)
parseGeneration arguments
  | "--timing-only" `elem` arguments && optionalArgument arguments "--generate" /= Nothing =
      Left "--generate cannot be combined with --timing-only"
  | otherwise = case optionalArgument arguments "--generate" of
      Nothing -> Right Nothing
      Just countText -> do
        count <- parseGenerationCount countText
        seed <- maybe (Left "missing --seed") parseGenerationSeed (optionalArgument arguments "--seed")
        budget <- maybe (Right defaultShrinkBudget) parseShrinkBudget (optionalArgument arguments "--shrink-budget")
        control <- case optionalArgument arguments "--control" of
          Nothing -> case [candidate | candidate <- mutants, controlSlot candidate == generationSlot] of
            candidate : _ -> Right candidate
            [] -> Left ("no control for " ++ slotTag generationSlot)
          Just name -> maybe (Left ("unknown control " ++ name)) Right (find ((== name) . controlName) mutants)
        when (controlSlot control /= generationSlot) (Left (controlName control ++ " does not test " ++ slotTag generationSlot))
        Right (Just (Generation seed count budget control))

subjectValue :: SubjectResult -> Value
subjectValue subject =
  object
    [ "name" .= subjectName subject
    , "cases" .= outcomeCases outcome
    , "matches" .= outcomeMatches outcome
    , "differing_returned" .= outcomeDifferingReturned outcome
    , "differing_stopped" .= outcomeDifferingStopped outcome
    , "statuses" .= statusesValue (outcomeStatuses outcome)
    , "first_difference" .= fmap differenceValue (outcomeFirst outcome)
    ]
  where
    outcome = subjectOutcome subject

shrinkValue :: ShrinkReport -> Value
shrinkValue report =
  object
    [ "role" .= shrinkRole report
    , "status" .= reductionStatusText (reductionStatus reduction)
    , "trials" .= reductionTrials reduction
    , "original" .= boundaryValue (shrinkOriginalLabel report) (shrinkOriginalRow report) (shrinkOriginalDifferences report) (shrinkOriginalBatches report)
    , "reduced" .= boundaryValue (shrinkReducedLabel report) (shrinkReducedRow report) (shrinkReducedDifferences report) (shrinkReducedBatches report)
    , "replay"
        .= object
          [ "case" .= shrinkReducedLabel report
          , "differences" .= map differenceText (shrinkReplayDifferences report)
          , "same_boundary" .= (map differenceKind (shrinkReplayDifferences report) == map differenceKind (shrinkOriginalDifferences report))
          ]
    ]
  where
    reduction = shrinkReduction report
    boundaryValue label row differences batches =
      object
        [ "case" .= label
        , "row" .= show row
        , "differences" .= map differenceText differences
        , "boundary" .= map (show . differenceKind) differences
        , "batches"
            .= object
              [ "reference" .= replayReferenceBatch batches
              , "subject" .= replaySubjectBatch batches
              ]
        ]

generatedDomainValue :: String -> GeneratedDomain -> Value
generatedDomainValue control result =
  object
    [ "domain" .= domainLabel (generatedDomain result)
    , "cases" .= generatedCases result
    , "stock_returned" .= generatedReferenceReturned result
    , "subjects" .= map subjectValue (generatedResults result)
    , "candidate_reduction" .= maybe Null shrinkValue (subjectReduction =<< find ((== "owned") . subjectName) (generatedResults result))
    , "control_reduction" .= maybe Null shrinkValue (subjectReduction =<< find ((== control) . subjectName) (generatedResults result))
    ]

generatedSubject :: String -> GeneratedDomain -> Maybe SubjectResult
generatedSubject name result = find ((== name) . subjectName) (generatedResults result)

generatedOwnedMatches :: GeneratedDomain -> Bool
generatedOwnedMatches result = case generatedSubject "owned" result of
  Just subject -> outcomeMatches (subjectOutcome subject) == outcomeCases (subjectOutcome subject)
  Nothing -> False

generatedControlHolds :: String -> GeneratedDomain -> Bool
generatedControlHolds control result = case generatedSubject control result of
  Just subject -> outcomeDifferingReturned (subjectOutcome subject) > 0
  Nothing -> False

generatedExit :: String -> [GeneratedDomain] -> ExitCode
generatedExit control results
  | not (all generatedReferenceReturned results) = ExitFailure 1
  | not (all (generatedControlHolds control) results) = ExitFailure 1
  | not (all generatedOwnedMatches results) = ExitFailure 2
  | otherwise = ExitSuccess

generatedSummary :: FilePath -> Value -> Generation -> [GeneratedDomain] -> Value
generatedSummary work inputs generation results =
  object
    [ "schema" .= (1 :: Int)
    , "mode" .= ("generate" :: String)
    , "slot" .= slotText generationSlot
    , "seed" .= ("0x" ++ seedText (generationSeed generation))
    , "count" .= count
    , "control" .= controlName (generationControl generation)
    , "shrink_budget" .= budget
    , "work" .= work
    , "inputs" .= inputs
    , "domains" .= map (generatedDomainValue (controlName (generationControl generation))) results
    , "stock_returned" .= all generatedReferenceReturned results
    , "owned_matches_stock" .= all generatedOwnedMatches results
    , "controls_hold" .= all (generatedControlHolds (controlName (generationControl generation))) results
    ]
  where
    GenerationCount count = generationCount generation
    ShrinkBudget budget = generationBudget generation

main :: IO ()
main = do
  arguments <- getArgs
  let input name = required arguments name >>= canonicalizePath
      output name = required arguments name >>= makeAbsolute
      timingOnly = "--timing-only" `elem` arguments
      selectedSlots = if timingOnly then [S128, S148, S164, S168, S16c, S1618] else slots
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
  generation <- orFailText "generate" (parseGeneration arguments)
  createDirectoryIfMissing True (takeDirectory work)
  createDirectory work
  let inputs =
        object
          ( [ "regions_sha256" .= digest regionsBytes
            , "runner_sha256" .= digest runnerBytes
            ]
              ++ [fromString ("loader_" ++ domainLabel domain ++ "_sha256") .= digest loader | (domain, loader) <- zip domains loaders]
              ++ [fromString name .= digest bytes | (name, bytes) <- zip ["owned_a_bin_sha256", "owned_a_symbols_sha256", "owned_b_bin_sha256", "owned_b_symbols_sha256"] ownedBytes]
          )
  case generation of
    Just selected -> do
      results <- forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
        let rowsTable = domainTable table domain
            control = generationControl selected
        stock <- orFailText (slotTag generationSlot ++ " stock") (stockBody domain rowsTable loader generationSlot)
        ownedSlot <- orFailText (slotTag generationSlot ++ " owned") (ownedBody ownedRows generationSlot)
        code <- orFailText (controlName control) (controlCode domain rowsTable control)
        controlBody <- orFailText (controlName control) (replaceEntry (tableFill table) code ownedSlot)
        runGeneratedDomain
          GeneratedInput
            { generatedRunner = runner
            , generatedWork = work
            , generatedDomainId = domain
            , generatedReferenceName = "stock"
            , generatedReferenceBody = stock
            , generatedSubjects = [("owned", ownedSlot), (controlName control, controlBody)]
            , generatedSeed = generationSeed selected
            , generatedCount = generationCount selected
            , generatedBudget = generationBudget selected
            }
      B.writeFile summaryPath (encodeCanonical (generatedSummary work inputs selected results))
      exitWith (generatedExit (controlName (generationControl selected)) results)
    Nothing -> do
      reports <- fmap concat . forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
        let rowsTable = domainTable table domain
        forM selectedSlots $ \slot -> do
          stock <- orFailText (slotTag slot ++ " stock") (stockBody domain rowsTable loader slot)
          ownedSlot <- orFailText (slotTag slot ++ " owned") (ownedBody ownedRows slot)
          entry <- orFailText (slotTag slot ++ " entry") (slotEntry rowsTable slot)
          let directory role = work </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
              timingKeys = [CaseKey index | (index, row) <- zip [0 ..] (take (length (caseGrid slot) - 64) (caseGrid slot)), rowCalls row == SingleCall]
              run role = if timingOnly
                then runRows runner (directory role) role domain slot timingKeys
                else runSlot runner (directory role) role domain slot
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
          when timingOnly $
            B.writeFile (work </> "timing-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ ".json") $
              encodeCanonical $ object
                [ "domain" .= domainLabel domain, "slot" .= slotText slot
                , "cases" .=
                    [ object ["row" .= index, "case" .= show row, "stock_instructions" .= runInstructions reference, "owned_instructions" .= runInstructions subject]
                    | (CaseKey index, row) <- zip (map CaseKey [0 ..]) (caseGrid slot)
                    , Just reference <- [M.lookup (CaseKey index) stockRuns]
                    , Just subject <- [M.lookup (CaseKey index) ownedRuns]
                    ]
                ]
          repeatOutcome <- if timingOnly then pure (outcomeOf stockRuns stockRuns) else outcomeOf stockRuns <$> run "stock-repeat" stock
          controlResults <- forM [control | not timingOnly, control <- equivalents ++ mutants, controlSlot control == slot] $ \control -> do
            code <- orFailText (controlName control) (controlCode domain rowsTable control)
            body <- orFailText (controlName control) (replaceEntry (tableFill table) code ownedSlot)
            ControlResult (controlName control) (controlExpectation control) (controlNote control) . outcomeOf stockRuns
              <$> run (controlName control) body
          let controls = if timingOnly then [] else ControlResult "stock-repeat" MustPass "the stock body against itself" repeatOutcome : controlResults
              report = SlotReport domain slot entry stockRuns ownedOutcome controls
          when (not timingOnly && verdictExit [report] == ExitSuccess) $
            forM_ ("stock" : "owned" : map resultControl (reportControls report)) $ \role -> do
              present <- doesPathExist (directory role)
              when present (removeDirectoryRecursive (directory role))
          pure report
      let exit = verdictExit reports
          domainValue domain = object ["domain" .= domainLabel domain, "slots" .= [slotValue report | report <- reports, reportDomain report == domain]]
      createDirectoryIfMissing True (takeDirectory summaryPath)
      B.writeFile summaryPath $
        encodeCanonical $
          object
            [ "schema" .= (1 :: Int)
            , "inputs" .= inputs
            , "timing_only" .= timingOnly
            , "domains" .= map domainValue domains
            , "controls" .= [controlValue report result | report <- reports, result <- reportControls report]
            , "stock_returned" .= all stockHolds reports
            , "controls_hold" .= all (all holds . reportControls) reports
            , "owned_matches_stock" .= all ownedMatches reports
            ]
      exitWith exit
