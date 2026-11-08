-- | Runs one domain's generated +0x16c corpus against a reference body and each subject, reduces the
-- first difference of every subject while preserving its compared boundary, and replays the reduced
-- case from its retained native-case batch.
module Replacement.RtosOwnFirst.Generated
  ( GeneratedInput (..)
  , GeneratedDomain (..)
  , SubjectResult (..)
  , ShrinkReport (..)
  , ReplayBatches (..)
  , runGeneratedDomain
  ) where

import Control.Monad (forM, unless, when)
import qualified Data.Map.Strict as M
import Emulation.NativeObservation (RunnerStatus (..))
import FujiAF.Host (failH)
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions (Domain, domainLabel)
import Replacement.RtosOwnFirst.Shrink
import System.Directory (doesPathExist, removeDirectoryRecursive)
import System.FilePath ((</>))

data GeneratedInput = GeneratedInput
  { generatedRunner :: FilePath
  , generatedWork :: FilePath
  , generatedDomainId :: Domain
  , generatedReferenceName :: String
  , generatedReferenceBody :: Body
  , -- | Named subjects, each run against the reference over the same corpus.
    generatedSubjects :: [(String, Body)]
  , generatedSeed :: GenerationSeed
  , generatedCount :: GenerationCount
  , generatedBudget :: ShrinkBudget
  }

data SubjectResult = SubjectResult
  { subjectName :: String
  , subjectOutcome :: Outcome
  , subjectReduction :: Maybe ShrinkReport
  }

data GeneratedDomain = GeneratedDomain
  { generatedDomain :: Domain
  , generatedCases :: Int
  , generatedReferenceReturned :: Bool
  , generatedResults :: [SubjectResult]
  }

data ReplayBatches = ReplayBatches
  { replayReferenceBatch :: FilePath
  , replaySubjectBatch :: FilePath
  }

data ShrinkReport = ShrinkReport
  { shrinkRole :: String
  , shrinkOriginalKey :: CaseKey
  , shrinkOriginalLabel :: String
  , shrinkReducedLabel :: String
  , shrinkOriginalRow :: Row
  , shrinkReducedRow :: Row
  , shrinkOriginalDifferences :: [Difference]
  , shrinkReducedDifferences :: [Difference]
  , shrinkReplayDifferences :: [Difference]
  , shrinkReduction :: Reduction
  , shrinkOriginalBatches :: ReplayBatches
  , shrinkReducedBatches :: ReplayBatches
  }

runGeneratedDomain :: GeneratedInput -> IO GeneratedDomain
runGeneratedDomain input = do
  let domain = generatedDomainId input
      slot = generationSlot
      seed = generatedSeed input
      keyed = zip (map CaseKey [0 ..]) (generateRows seed (generatedCount input))
      labelOf = generatedLabel seed domain
      directory role = generatedWork input </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
  reference <- runRowsWith (generatedRunner input) (directory ("generated-" ++ generatedReferenceName input)) labelOf domain slot keyed (generatedReferenceBody input)
  results <- forM (generatedSubjects input) $ \(name, body) -> do
    runs <- runRowsWith (generatedRunner input) (directory ("generated-" ++ name)) labelOf domain slot keyed body
    let outcome = outcomeOf reference runs
    reduction <- case outcomeFirst outcome of
      Nothing -> pure Nothing
      Just (key, differences) -> do
        row <- maybe (failH ("generated row " ++ show key ++ " is missing")) pure (lookup key keyed)
        Just <$> shrinkAndReplay input row key differences name body
    pure (SubjectResult name outcome reduction)
  removeScratch (generatedWork input </> "shrink-scratch")
  pure (GeneratedDomain domain (length keyed) (allReturned reference) results)

shrinkAndReplay :: GeneratedInput -> Row -> CaseKey -> [Difference] -> String -> Body -> IO ShrinkReport
shrinkAndReplay input row key originalDifferences subjectName subjectBody = do
  let domain = generatedDomainId input
      slot = generationSlot
      seed = generatedSeed input
      runner = generatedRunner input
      work = generatedWork input
      referenceBody = generatedReferenceBody input
      originalKinds = map differenceKind originalDifferences
      CaseKey index = key
      scratch = work </> "shrink-scratch" </> subjectName ++ "-" ++ domainLabel domain
      trialLabel = "trial-" ++ subjectName ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ "-" ++ show index
      predicate candidate = do
        expected <- runRowsWith runner (scratch </> "reference") (const trialLabel) domain slot [(key, candidate)] referenceBody
        actual <- runRowsWith runner (scratch </> "subject") (const trialLabel) domain slot [(key, candidate)] subjectBody
        pure $ case (M.lookup key expected, M.lookup key actual) of
          (Just reference, Just subject) ->
            runStatus reference == Returned && map differenceKind (compareRuns reference subject) == originalKinds
          _ -> False
  reduction <- shrinkRow (generatedBudget input) predicate row
  let reducedRow = reductionReduced reduction
      reducedId = reducedLabel subjectName seed domain key
      referenceDirectory = work </> ("reduced-" ++ subjectName ++ "-reference-" ++ domainLabel domain ++ "-" ++ slotTag slot)
      subjectDirectory = work </> ("reduced-" ++ subjectName ++ "-subject-" ++ domainLabel domain ++ "-" ++ slotTag slot)
      originalBatches =
        ReplayBatches
          (work </> ("generated-" ++ generatedReferenceName input ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot) </> "batch-0")
          (work </> ("generated-" ++ subjectName ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot) </> "batch-0")
      reducedBatches = ReplayBatches (referenceDirectory </> "batch-0") (subjectDirectory </> "batch-0")
  unless (validRow reducedRow) (failH ("reduction produced an invalid row: " ++ show reducedRow))
  expectedRuns <- runRowsWith runner referenceDirectory (const reducedId) domain slot [(key, reducedRow)] referenceBody
  actualRuns <- runRowsWith runner subjectDirectory (const reducedId) domain slot [(key, reducedRow)] subjectBody
  let reducedDifferences = differencesOf key expectedRuns actualRuns
  unless (not (null reducedDifferences) && map differenceKind reducedDifferences == originalKinds) $
    failH ("reduced case " ++ reducedId ++ " does not reproduce the original boundary")
  replayedExpected <- loadBatchRuns runner (replayReferenceBatch reducedBatches) (const reducedId) domain [(key, reducedRow)]
  replayedActual <- loadBatchRuns runner (replaySubjectBatch reducedBatches) (const reducedId) domain [(key, reducedRow)]
  let replayDifferences = differencesOf key replayedExpected replayedActual
      replayHolds = case (M.lookup key replayedExpected, M.lookup key replayedActual) of
        (Just reference, Just _) ->
          runStatus reference == Returned
            && not (null replayDifferences)
            && map differenceKind replayDifferences == originalKinds
        _ -> False
  unless replayHolds (failH ("replay of " ++ reducedId ++ " does not reproduce the original boundary"))
  removeScratch scratch
  pure
    ShrinkReport
      { shrinkRole = subjectName
      , shrinkOriginalKey = key
      , shrinkOriginalLabel = generatedLabel seed domain key
      , shrinkReducedLabel = reducedId
      , shrinkOriginalRow = reductionOriginal reduction
      , shrinkReducedRow = reducedRow
      , shrinkOriginalDifferences = originalDifferences
      , shrinkReducedDifferences = reducedDifferences
      , shrinkReplayDifferences = replayDifferences
      , shrinkReduction = reduction
      , shrinkOriginalBatches = originalBatches
      , shrinkReducedBatches = reducedBatches
      }

differencesOf :: CaseKey -> M.Map CaseKey Run -> M.Map CaseKey Run -> [Difference]
differencesOf key expected actual = case (M.lookup key expected, M.lookup key actual) of
  (Just reference, Just subject) -> compareRuns reference subject
  _ -> []

removeScratch :: FilePath -> IO ()
removeScratch path = do
  exists <- doesPathExist path
  when exists (removeDirectoryRecursive path)
