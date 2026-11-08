{-# LANGUAGE OverloadedStrings #-}

-- | Replays the original and reduced native batches a generation summary retains. The recorded
-- runner, case, wire and region bindings are checked before any batch runs; each pair is copied to
-- a fresh directory so the preserved reports are never overwritten.
module Replacement.RtosOwnFirst.Replay
  ( RecordedKind (..)
  , recordedKindText
  , RecordedCase (..)
  , ReplayCaseResult (..)
  , readRecordedCases
  , replayCase
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.List (find)
import Data.Text (unpack)
import Emulation.NativeCase (Region (..), loadCase, runnerDigest)
import Emulation.NativeObservation (NativeObservation, Register (..), RunnerStatus (..), lookupRegister)
import Emulation.OwnedCase (CaseId (..), loadObservedBatch)
import FujiAF.Host (ensure, failH, get, list, str)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Controls (controlName, mutants)
import Replacement.RtosOwnFirst.Generated (BodyRole (..), ReplayBatches (..), roleLabel)
import Replacement.RtosOwnFirst.Oracle (Run (..), compareRuns, differenceText, observedRun)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.Directory (copyFile, createDirectoryIfMissing, doesFileExist, listDirectory)
import System.FilePath ((</>))

data RecordedKind = RecordedOriginal | RecordedReduced
  deriving (Eq, Show)

recordedKindText :: RecordedKind -> String
recordedKindText RecordedOriginal = "original"
recordedKindText RecordedReduced = "reduced"

data RecordedCase = RecordedCase
  { recordedRole :: BodyRole
  , recordedDomain :: Domain
  , recordedKind :: RecordedKind
  , recordedCaseId :: String
  , recordedDifferences :: [String]
  , recordedBatches :: ReplayBatches
  }

data ReplayCaseResult = ReplayCaseResult
  { replayRecorded :: RecordedCase
  , replayReferenceIdentity :: String
  , replaySubjectIdentity :: String
  , replayDifferences :: [String]
  , replayReproduces :: Bool
  }

-- | Reads every recorded original and reduced case of a generation summary.
readRecordedCases :: Value -> Either String [RecordedCase]
readRecordedCases summary = concat <$> mapM domainCases (list (get summary "domains"))
  where
    domainCases domain = do
      label <- text (get domain "domain")
      domainId <- case label of
        "a" -> Right DomainA
        "b" -> Right DomainB
        other -> Left ("unknown recorded domain " ++ show other)
      reductions <- mapM (reduction domainId) [get domain "candidate_reduction", get domain "control_reduction"]
      Right (concat reductions)
    reduction _ Null = Right []
    reduction domainId value = do
      name <- text (get value "role")
      role <- roleOf name
      original <- caseOf domainId role RecordedOriginal (get value "original")
      reduced <- caseOf domainId role RecordedReduced (get value "reduced")
      Right [original, reduced]
    caseOf domainId role kind value = do
      ident <- textField value "case"
      differences <- mapM text (list (get value "differences"))
      let batches = get value "batches"
      reference <- textField batches "reference"
      subject <- textField batches "subject"
      Right (RecordedCase role domainId kind ident differences (ReplayBatches reference subject))
    roleOf "owned" = Right OwnedRole
    roleOf name = case find ((== name) . controlName) mutants of
      Just control -> Right (ControlRole control)
      Nothing -> Left ("recorded role " ++ show name ++ " is neither the owned body nor a current control")
    textField value key = text (get value key)
    text (String value) = Right (unpack value)
    text _ = Left "recorded case field must be a string"

-- | Checks the frozen report and both case bindings, then re-executes a fresh copy of the pair.
replayCase :: FilePath -> FilePath -> RecordedCase -> IO ReplayCaseResult
replayCase runner work recorded = do
  runnerHash <- runnerDigest runner
  let batches = recordedBatches recorded
      expected = recordedCaseId recorded
  (referenceValue, referenceRegions, referenceIdentity) <- loadPairCase (replayReferenceBatch batches) expected
  (subjectValue, subjectRegions, subjectIdentity) <- loadPairCase (replaySubjectBatch batches) expected
  let problems = bindingDifferences referenceValue referenceRegions subjectValue subjectRegions
  unless (null problems) $
    failH ("recorded case " ++ expected ++ " changed its " ++ unwords problems ++ " binding")
  checkReport (replayReferenceBatch batches) expected runnerHash (identityField "case_sha256" referenceIdentity) (identityField "wire_sha256" referenceIdentity)
  checkReport (replaySubjectBatch batches) expected runnerHash (identityField "case_sha256" subjectIdentity) (identityField "wire_sha256" subjectIdentity)
  let directory = roleLabel (recordedRole recorded) ++ "-" ++ domainLabel (recordedDomain recorded) ++ "-" ++ recordedKindText (recordedKind recorded) ++ "-" ++ expected
      freshReference = work </> directory </> "reference" </> "batch-0"
      freshSubject = work </> directory </> "subject" </> "batch-0"
  copyBatchFor (replayReferenceBatch batches) freshReference expected
  copyBatchFor (replaySubjectBatch batches) freshSubject expected
  referenceObserved <- loadObservedBatch runner (freshReference </> "batch.json")
  subjectObserved <- loadObservedBatch runner (freshSubject </> "batch.json")
  (referenceLabel, referenceObservation) <- oneObservation freshReference referenceObserved
  (subjectLabel, subjectObservation) <- oneObservation freshSubject subjectObserved
  unless (referenceLabel == expected && subjectLabel == expected) $
    failH ("replayed labels " ++ show (referenceLabel, subjectLabel) ++ " differ from the recorded case " ++ expected)
  referenceSp <- maybe (failH ("reference case " ++ expected ++ " has no sp")) pure (lookupRegister Sp referenceObservation)
  referenceRun <- observedRun freshReference referenceLabel (recordedDomain recorded) referenceSp referenceObservation
  subjectRun <- observedRun freshSubject subjectLabel (recordedDomain recorded) referenceSp subjectObservation
  unless (runStatus referenceRun == Returned) $
    failH ("reference run of " ++ expected ++ " did not return")
  let differences = map differenceText (compareRuns referenceRun subjectRun)
  pure
    ReplayCaseResult
      { replayRecorded = recorded
      , replayReferenceIdentity = identityField "case_sha256" referenceIdentity
      , replaySubjectIdentity = identityField "case_sha256" subjectIdentity
      , replayDifferences = differences
      , replayReproduces = differences == recordedDifferences recorded
      }

loadPairCase :: FilePath -> String -> IO (Value, [Region], [(String, Value)])
loadPairCase directory expected = do
  path <- batchCasePath directory expected
  loaded <- try (loadCase path) :: IO (Either IOException (Value, [Region], B.ByteString, [(String, Value)]))
  case loaded of
    Left problem -> failH (path ++ ": " ++ show problem)
    Right (value, regions, _, identity) -> do
      ensure (str (get value "id") == expected) (path ++ ": case id differs from " ++ expected)
      pure (value, regions, identity)

batchCasePath :: FilePath -> String -> IO FilePath
batchCasePath directory expected = do
  let batchPath = directory </> "batch.json"
  exists <- doesFileExist batchPath
  unless exists (failH (batchPath ++ ": missing batch"))
  bytes <- B.readFile batchPath
  batch <- either (\problem -> failH (batchPath ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  case recordedEntries batch expected of
    [entry] -> pure (directory </> str (get entry "path"))
    entries ->
      failH
        ( batchPath ++ ": expected the recorded case " ++ expected ++ ", found "
            ++ show [str (get entry "id") | entry <- entries]
        )

recordedEntries :: Value -> String -> [Value]
recordedEntries batch expected = [entry | entry <- list (get batch "cases"), str (get entry "id") == expected]

identityField :: String -> [(String, Value)] -> String
identityField name identity = maybe "" str (lookup name identity)

-- | The report a generation run froze beside the batch: its runner, case and wire identities must
-- still describe the current runner and the case files about to be replayed.
checkReport :: FilePath -> String -> String -> String -> String -> IO ()
checkReport directory expected runnerHash caseHash wireHash = do
  let path = directory </> expected ++ ".native-result.json"
  exists <- doesFileExist path
  unless exists (failH (path ++ ": missing preserved report"))
  bytes <- B.readFile path
  report <- either (\problem -> failH (path ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  let recordedRunner = str (get report "runner_sha256")
      recordedCase = str (get report "case_sha256")
      recordedWire = str (get report "wire_sha256")
      recordedId = str (get report "case_id")
  ensure (recordedId == expected) (path ++ ": report case id " ++ show recordedId ++ " differs from " ++ expected)
  ensure (recordedRunner == runnerHash) (path ++ ": runner binding differs from the current runner")
  ensure (recordedCase == caseHash) (path ++ ": case binding differs from the case file")
  ensure (recordedWire == wireHash) (path ++ ": wire binding differs from the case file")

bindingDifferences :: Value -> [Region] -> Value -> [Region] -> [String]
bindingDifferences referenceValue referenceRegions subjectValue subjectRegions =
  [key | key <- bindingFields, get referenceValue key /= get subjectValue key]
    ++ ["regions" | map regionBinding referenceRegions /= map regionBinding subjectRegions]
    ++ [ "region bytes " ++ regionId region
       | (region, subject) <- zip referenceRegions subjectRegions
       , permission region /= "rx"
       , regionBytes region /= regionBytes subject
       ]
  where
    bindingFields =
      [ "schema_version"
      , "id"
      , "origin"
      , "provenance"
      , "cpu"
      , "entry"
      , "exit"
      , "registers"
      , "limits"
      , "expected"
      , "exclusions"
      , "functions"
      , "stubs"
      , "cp14_identity"
      , "kernel"
      ]
    regionBinding region = (regionId region, baseAddress region, regionSize region, permission region)
    regionId region = str (get (description region) "id")
    permission region = str (get (description region) "permissions")

copyBatchFor :: FilePath -> FilePath -> String -> IO ()
copyBatchFor source destination expected = do
  createDirectoryIfMissing True destination
  entries <- listDirectory source
  forM_ entries $ \entry -> copyFile (source </> entry) (destination </> entry)
  let batchPath = destination </> "batch.json"
  bytes <- B.readFile batchPath
  batch <- either (\problem -> failH (batchPath ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  case recordedEntries batch expected of
    [entry] -> B.writeFile batchPath (encodeCanonical (object ["schema_version" .= (1 :: Int), "cases" .= [entry]]))
    _ -> failH (batchPath ++ ": the recorded case " ++ expected ++ " is not present exactly once")

oneObservation :: FilePath -> [(CaseId, NativeObservation)] -> IO (String, NativeObservation)
oneObservation directory observed = case observed of
  [(CaseId label, observation)] -> pure (label, observation)
  _ -> failH (directory ++ ": expected one observation, found " ++ show (length observed))
