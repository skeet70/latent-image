-- | Runs one domain's generated +0x16c corpus against a reference body and each subject, reduces the
-- first difference of every subject while preserving its compared boundary, and replays the reduced
-- case from its retained native-case batch.
module Replacement.RtosOwnFirst.Generated
  ( GeneratedInput (..)
  , GeneratedDomain (..)
  , SubjectResult (..)
  , ShrinkReport (..)
  , ReplayBatches (..)
  , BodyRole (..)
  , roleLabel
  , runGeneratedDomain
  ) where

import Control.Monad (forM, unless, when)
import qualified Data.Map.Strict as M
import Emulation.NativeObservation (RunnerStatus (..))
import FujiAF.Host (failH)
import Replacement.RtosOwnFirst.Controls (Control, controlName)
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions (Domain, domainLabel)
import Replacement.RtosOwnFirst.Shrink
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesPathExist, removeDirectoryRecursive)
import System.FilePath ((</>))

-- | Which body a case run carries. The control role carries the existing 'Control' value.
data BodyRole = StockRole | OwnedRole | ControlRole Control

roleLabel :: BodyRole -> String
roleLabel StockRole = "stock"
roleLabel OwnedRole = "owned"
roleLabel (ControlRole control) = controlName control

data GeneratedInput = GeneratedInput
  { generatedRunner :: FilePath
  , generatedWork :: FilePath
  , generatedDomainId :: Domain
  , generatedReference :: (BodyRole, Body)
  , -- | Named subjects, each run against the reference over the same corpus.
    generatedSubjects :: [(BodyRole, Body)]
  , generatedSeed :: GenerationSeed
  , generatedCount :: GenerationCount
  , generatedBudget :: ShrinkBudget
  }

data SubjectResult = SubjectResult
  { subjectRole :: BodyRole
  , subjectOutcome :: Outcome
  , subjectReduction :: Maybe ShrinkReport
  }

data GeneratedDomain = GeneratedDomain
  { generatedDomain :: Domain
  , generatedWorkDirectory :: FilePath
  , generatedCases :: Int
  , generatedReferenceReturned :: Bool
  , generatedResults :: [SubjectResult]
  }

data ReplayBatches = ReplayBatches
  { replayReferenceBatch :: FilePath
  , replaySubjectBatch :: FilePath
  }

data ShrinkReport = ShrinkReport
  { shrinkRole :: BodyRole
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
  createDirectoryIfMissing True (generatedWork input)
  work <- canonicalizePath (generatedWork input)
  let domain = generatedDomainId input
      slot = generationSlot
      seed = generatedSeed input
      keyed = zip (map CaseKey [0 ..]) (generateRows seed (generatedCount input))
      labelOf = generatedLabel seed domain
      directory role = work </> ("generated-" ++ roleLabel role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot)
      (referenceRole, referenceBody) = generatedReference input
  unless (all (validRow . snd) keyed) (failH "generated corpus holds an invalid +0x16c row")
  reference <- runRowsWith (generatedRunner input) (directory referenceRole) labelOf domain slot keyed referenceBody
  results <- forM (generatedSubjects input) $ \(role, body) -> do
    runs <- runRowsWith (generatedRunner input) (directory role) labelOf domain slot keyed body
    let outcome = outcomeOf reference runs
    reduction <- case outcomeFirst outcome of
      Nothing -> pure Nothing
      Just (key, differences) -> case M.lookup key reference of
        Just run | runStatus run == Returned -> do
          row <- maybe (failH ("generated row " ++ show key ++ " is missing")) pure (lookup key keyed)
          Just <$> shrinkAndReplay work input row key differences role body
        _ -> pure Nothing
    pure (SubjectResult role outcome reduction)
  removeScratch (work </> "shrink-scratch")
  pure (GeneratedDomain domain work (length keyed) (allReturned reference) results)

shrinkAndReplay :: FilePath -> GeneratedInput -> Row -> CaseKey -> [Difference] -> BodyRole -> Body -> IO ShrinkReport
shrinkAndReplay work input row key originalDifferences subjectRole subjectBody = do
  let domain = generatedDomainId input
      slot = generationSlot
      seed = generatedSeed input
      runner = generatedRunner input
      (referenceRole, referenceBody) = generatedReference input
      roleText = roleLabel subjectRole
      originalKinds = map differenceKind originalDifferences
      CaseKey index = key
      scratch = work </> "shrink-scratch" </> (roleText ++ "-" ++ domainLabel domain)
      trialLabel = "trial-" ++ roleText ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ "-" ++ show index
      predicate candidate = do
        expected <- runRowsWith runner (scratch </> "reference") (const trialLabel) domain slot [(key, candidate)] referenceBody
        actual <- runRowsWith runner (scratch </> "subject") (const trialLabel) domain slot [(key, candidate)] subjectBody
        pure $ case (M.lookup key expected, M.lookup key actual) of
          (Just reference, Just subject) ->
            runStatus reference == Returned && map differenceKind (compareRuns reference subject) == originalKinds
          _ -> False
  reduction <- shrinkRow (generatedBudget input) predicate row
  let reducedRow = reductionReduced reduction
      reducedId = reducedLabel roleText seed domain key
      referenceDirectory = work </> ("reduced-" ++ roleText ++ "-reference-" ++ domainLabel domain ++ "-" ++ slotTag slot)
      subjectDirectory = work </> ("reduced-" ++ roleText ++ "-subject-" ++ domainLabel domain ++ "-" ++ slotTag slot)
      originalBatches =
        ReplayBatches
          (work </> ("generated-" ++ roleLabel referenceRole ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot) </> "batch-0")
          (work </> ("generated-" ++ roleText ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot) </> "batch-0")
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
      { shrinkRole = subjectRole
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
