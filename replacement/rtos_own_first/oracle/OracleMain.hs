{-# LANGUAGE OverloadedStrings #-}

-- | Runs every own-first slot's case grid against the stock body from the pinned loaders and against
-- the owned body, compares them case by case and runs the controls. The generated mode adds a valid
-- +0x16c corpus with reduction; the replay mode re-executes the retained original and reduced
-- batches. Exits 1 when a stock run stops or a control misbehaves, and 2 when an owned body differs
-- from stock.
module Main (main) where

import Control.Monad (filterM, forM, forM_, unless, when)
import Data.Aeson (Value (Null), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.List (intercalate)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.String (fromString)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.NativeCase (runnerDigest)
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, failH, get, keys, readRegular)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..), loaderSha256)
import Replacement.RtosOwnFirst.Command
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Generated
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions
import Replacement.RtosOwnFirst.Replay
import Replacement.RtosOwnFirst.Shrink
import Replacement.RtosOwnFirst.Verdict
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getArgs, getExecutablePath, lookupEnv, setEnv)
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

isOwnedRole :: BodyRole -> Bool
isOwnedRole OwnedRole = True
isOwnedRole _ = False

isControlRole :: BodyRole -> Bool
isControlRole (ControlRole _) = True
isControlRole _ = False

ownedSubject :: GeneratedDomain -> Maybe SubjectResult
ownedSubject result = listToMaybe [subject | subject <- generatedResults result, isOwnedRole (subjectRole subject)]

controlSubject :: GeneratedDomain -> Maybe SubjectResult
controlSubject result = listToMaybe [subject | subject <- generatedResults result, isControlRole (subjectRole subject)]

subjectValue :: SubjectResult -> Value
subjectValue subject =
  object
    [ "name" .= roleLabel (subjectRole subject)
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
    [ "role" .= roleLabel (shrinkRole report)
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

generatedDomainValue :: GeneratedDomain -> Value
generatedDomainValue result =
  object
    [ "domain" .= domainLabel (generatedDomain result)
    , "cases" .= generatedCases result
    , "stock_returned" .= generatedReferenceReturned result
    , "subjects" .= map subjectValue (generatedResults result)
    , "candidate_reduction" .= maybe Null shrinkValue (subjectReduction =<< ownedSubject result)
    , "control_reduction" .= maybe Null shrinkValue (subjectReduction =<< controlSubject result)
    ]

generatedOwnedMatches :: GeneratedDomain -> Bool
generatedOwnedMatches result = case ownedSubject result of
  Just subject -> outcomeMatches (subjectOutcome subject) == outcomeCases (subjectOutcome subject)
  Nothing -> False

generatedControlHolds :: GeneratedDomain -> Bool
generatedControlHolds result = case controlSubject result of
  Just subject -> outcomeDifferingReturned (subjectOutcome subject) > 0
  Nothing -> False

generatedExit :: [GeneratedDomain] -> ExitCode
generatedExit results
  | not (all generatedReferenceReturned results) = ExitFailure 1
  | not (all generatedControlHolds results) = ExitFailure 1
  | not (all generatedOwnedMatches results) = ExitFailure 2
  | otherwise = ExitSuccess

generatedSummary :: FilePath -> Value -> GenerateOptions -> [GeneratedDomain] -> Value
generatedSummary work inputs options results =
  object
    [ "schema" .= (1 :: Int)
    , "mode" .= ("generate" :: String)
    , "slot" .= slotText generationSlot
    , "seed" .= ("0x" ++ seedText (optionSeed options))
    , "count" .= generationCountValue (optionCount options)
    , "control" .= controlName (optionControl options)
    , "shrink_budget" .= shrinkBudgetValue (optionBudget options)
    , "work" .= work
    , "inputs" .= inputs
    , "domains" .= map generatedDomainValue results
    , "stock_returned" .= all generatedReferenceReturned results
    , "owned_matches_stock" .= all generatedOwnedMatches results
    , "controls_hold" .= all generatedControlHolds results
    ]

generatedTerminal :: FilePath -> GenerateOptions -> [GeneratedDomain] -> IO ()
generatedTerminal summaryPath options results = do
  let control = optionControl options
      controls = [subjectOutcome subject | result <- results, subject <- generatedResults result, isControlRole (subjectRole subject)]
      reductions = [report | result <- results, subject <- generatedResults result, Just report <- [subjectReduction subject]]
      boundary report = intercalate "," (map (show . differenceKind) (shrinkOriginalDifferences report))
      reduced = [shrinkReducedLabel report ++ " (" ++ boundary report ++ ", " ++ reductionStatusText (reductionStatus (shrinkReduction report)) ++ ")" | report <- reductions]
      prefix =
        "own-first generate: +0x16c seed 0x" ++ seedText (optionSeed options)
          ++ " " ++ show (generationCountValue (optionCount options)) ++ " cases x 2 loaders"
      outcome
        | not (all generatedReferenceReturned results) = "; a stock run did not return"
        | not (all generatedControlHolds results) = "; control " ++ controlName control ++ " did not differ on a returned case"
        | not (all generatedOwnedMatches results) = "; owned differs from stock"
        | otherwise =
            "; stock returned; owned matches; control differs on "
              ++ show [outcomeDifferingReturned controlOutcome | controlOutcome <- controls]
              ++ " returned cases per loader"
  putStrLn (prefix ++ outcome ++ (if null reduced then "" else "; reduced " ++ intercalate ", " reduced) ++ "; summary " ++ summaryPath)

replayCaseValue :: ReplayCaseResult -> Value
replayCaseValue result =
  object
    [ "role" .= roleLabel (recordedRole recorded)
    , "domain" .= domainLabel (recordedDomain recorded)
    , "kind" .= recordedKindText (recordedKind recorded)
    , "case" .= recordedCaseId recorded
    , "batches"
        .= object
          [ "reference" .= replayReferenceBatch batches
          , "subject" .= replaySubjectBatch batches
          ]
    , "identity"
        .= object
          [ "reference_sha256" .= replayReferenceIdentity result
          , "subject_sha256" .= replaySubjectIdentity result
          ]
    , "expected_boundary" .= map show (recordedBoundary recorded)
    , "kinds" .= map show (replayKinds result)
    , "expected_differences" .= recordedDifferences recorded
    , "differences" .= replayDifferences result
    , "reproduces" .= replayReproduces result
    ]
  where
    recorded = replayRecorded result
    batches = recordedBatches recorded

replaySummary :: FilePath -> FilePath -> Value -> [ReplayCaseResult] -> Value
replaySummary source work inputs results =
  object
    [ "schema" .= (1 :: Int)
    , "mode" .= ("replay" :: String)
    , "source" .= source
    , "work" .= work
    , "inputs" .= inputs
    , "cases" .= map replayCaseValue results
    , "cases_total" .= length results
    , "reproduced" .= length (filter replayReproduces results)
    ]

replayExit :: [ReplayCaseResult] -> ExitCode
replayExit results
  | any (not . replayReproduces) results = ExitFailure 1
  | any (isOwnedRole . recordedRole . replayRecorded) results = ExitFailure 2
  | otherwise = ExitSuccess

replayTerminal :: FilePath -> [ReplayCaseResult] -> IO ()
replayTerminal summaryPath results =
  case [result | result <- results, not (replayReproduces result)] of
    result : _ ->
      putStrLn
        ( "own-first replay: " ++ recordedCaseId (replayRecorded result)
            ++ " (" ++ recordedKindText (recordedKind (replayRecorded result)) ++ ") did not reproduce its recorded difference; summary " ++ summaryPath
        )
    [] ->
      putStrLn
        ( "own-first replay: " ++ show (length results) ++ " cases replayed and reproduced; summary " ++ summaryPath
        )

verifyInputs :: Value -> Value -> IO ()
verifyInputs recorded current = do
  ensure (keys recorded == keys current) "replay summary input set differs from the current inputs"
  let differing = [key | key <- keys recorded, get recorded key /= get current key]
  unless (null differing) (failH ("replay inputs differ: " ++ unwords differing))

enterSlot :: [String] -> IO ()
enterSlot arguments = do
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments

-- | Resolves a relative input against the current directory, the running executable's runfiles tree
-- and the workspace. A slot re-exec keeps the arguments, so every input becomes absolute before it.
resolveInput :: FilePath -> IO FilePath
resolveInput path = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  executable <- getExecutablePath
  let runfiles = executable ++ ".runfiles"
      candidates =
        path
          : [runfiles </> "_main" </> path, runfiles </> path]
          ++ [directory </> path | directory <- maybe [] (: []) workspace]
  present <- filterM doesPathExist candidates
  canonicalizePath (case present of candidate : _ -> candidate; [] -> path)

rewriteFlags :: [(String, FilePath)] -> [String] -> [String]
rewriteFlags replacements = go
  where
    go [] = []
    go (argument : rest) = case lookup argument replacements of
      Just value -> argument : value : drop 1 rest
      Nothing -> argument : go rest

main :: IO ()
main = do
  arguments <- getArgs
  command <- orFailText "arguments" (parseCommand arguments)
  let input name = required arguments name >>= resolveInput
      output name = required arguments name >>= makeAbsolute
      timingOnly = case command of
        GridCommand -> "--timing-only" `elem` arguments
        _ -> False
      selectedSlots = if timingOnly then [S128, S148, S164, S168, S16c, S1618] else slots
  runner <- input "--runner"
  regionsPath <- input "--regions"
  ownedPaths <- mapM input ["--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  loaderPaths <- mapM input ["--loader-a", "--loader-b"]
  work <- output "--work"
  summaryPath <- output "--summary"
  replaySource <- case command of
    ReplayCommand options -> Just <$> resolveInput (optionSource options)
    _ -> pure Nothing
  let replacements =
        [("--runner", runner), ("--regions", regionsPath), ("--work", work), ("--summary", summaryPath)]
          ++ zip ["--loader-a", "--loader-b"] loaderPaths
          ++ zip ["--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols"] ownedPaths
          ++ maybe [] (\source -> [("--replay", source)]) replaySource
  case command of
    GenerateCommand _ -> do
      setEnv "XM1_STORAGE_PATH" (takeDirectory work)
      enterSlot (rewriteFlags replacements arguments)
    ReplayCommand _ -> do
      setEnv "XM1_STORAGE_PATH" (takeDirectory work)
      enterSlot (rewriteFlags replacements arguments)
    GridCommand -> pure ()
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
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
  canonicalWork <- canonicalizePath work
  createDirectoryIfMissing True (takeDirectory summaryPath)
  canonicalSummary <- canonicalizePath summaryPath
  let inputs =
        object
          ( [ "regions_sha256" .= digest regionsBytes
            , "runner_sha256" .= digest runnerBytes
            ]
              ++ [fromString ("loader_" ++ domainLabel domain ++ "_sha256") .= digest loader | (domain, loader) <- zip domains loaders]
              ++ [fromString name .= digest bytes | (name, bytes) <- zip ["owned_a_bin_sha256", "owned_a_symbols_sha256", "owned_b_bin_sha256", "owned_b_symbols_sha256"] ownedBytes]
          )
  case command of
    GenerateCommand options -> do
      let control = optionControl options
      results <- forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
        let rowsTable = domainTable table domain
        stock <- orFailText (slotTag generationSlot ++ " stock") (stockBody domain rowsTable loader generationSlot)
        ownedSlot <- orFailText (slotTag generationSlot ++ " owned") (ownedBody ownedRows generationSlot)
        code <- orFailText (controlName control) (controlCode domain rowsTable control)
        controlBody <- orFailText (controlName control) (replaceEntry (tableFill table) code ownedSlot)
        runGeneratedDomain
          GeneratedInput
            { generatedRunner = runner
            , generatedWork = canonicalWork
            , generatedDomainId = domain
            , generatedReference = (StockRole, stock)
            , generatedSubjects = [(OwnedRole, ownedSlot), (ControlRole control, controlBody)]
            , generatedSeed = optionSeed options
            , generatedCount = optionCount options
            , generatedBudget = optionBudget options
            }
      B.writeFile canonicalSummary (encodeCanonical (generatedSummary canonicalWork inputs options results))
      generatedTerminal canonicalSummary options results
      exitWith (generatedExit results)
    ReplayCommand options -> do
      source <- maybe (failH ("missing " ++ optionSource options)) pure replaySource
      sourceBytes <- readRegular 0x1000000 source
      sourceValue <- either (\problem -> failH (source ++ ": " ++ problem)) pure (eitherDecodeStrict' sourceBytes)
      recorded <- orFailText "replay summary" (readRecordedCases sourceValue)
      ensure (not (null recorded)) "replay summary records no original or reduced case"
      verifyInputs (get sourceValue "inputs") inputs
      runnerHash <- runnerDigest runner
      prepared <- forM recorded (preflightCase runnerHash)
      results <- forM prepared (executeCase runner canonicalWork)
      B.writeFile canonicalSummary (encodeCanonical (replaySummary source canonicalWork inputs results))
      replayTerminal canonicalSummary results
      exitWith (replayExit results)
    GridCommand -> do
      reports <- fmap concat . forM (zip3 domains loaders owned) $ \(domain, loader, ownedRows) -> do
        let rowsTable = domainTable table domain
        forM selectedSlots $ \slot -> do
          stock <- orFailText (slotTag slot ++ " stock") (stockBody domain rowsTable loader slot)
          ownedSlot <- orFailText (slotTag slot ++ " owned") (ownedBody ownedRows slot)
          entry <- orFailText (slotTag slot ++ " entry") (slotEntry rowsTable slot)
          let directory role = canonicalWork </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
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
            B.writeFile (canonicalWork </> "differences-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ ".json") (encodeCanonical (object ["cases" .= differences]))
          when timingOnly $
            B.writeFile (canonicalWork </> "timing-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ ".json") $
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
      B.writeFile canonicalSummary $
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
