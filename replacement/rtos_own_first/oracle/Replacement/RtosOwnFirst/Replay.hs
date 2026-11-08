{-# LANGUAGE OverloadedStrings #-}

-- | Replays the original and reduced native batches a generation summary retains. Every recorded
-- pair is preflighted before the first one runs: the runner, case, wire and region bindings are
-- checked, the entry SP comes from the admitted case registers, and only the selected case and its
-- declared artifacts are copied to a fresh directory, leaving preserved reports untouched.
module Replacement.RtosOwnFirst.Replay
  ( RecordedKind (..)
  , recordedKindText
  , RecordedCase (..)
  , PreparedCase (..)
  , ReplayCaseResult (..)
  , readRecordedCases
  , parseDifferenceKind
  , preflightCase
  , executeCase
  , replayCase
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.List (find, stripPrefix)
import Data.Text (unpack)
import Data.Word (Word32)
import Emulation.NativeCase (Region (..), loadCase, runnerDigest)
import Emulation.NativeObservation (NativeObservation, RegionId (..), RunnerStatus (..), parseWord32, registerLabels)
import Emulation.OwnedCase (CaseId (..), loadObservedBatch)
import FujiAF.Host (ensure, failH, get, list, str)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Controls (controlName, mutants)
import Replacement.RtosOwnFirst.Generate (generationSlot)
import Replacement.RtosOwnFirst.Generated (BodyRole (..), ReplayBatches (..), roleLabel)
import Replacement.RtosOwnFirst.Oracle (DifferenceKind (..), Run (..), compareRuns, differenceKind, differenceText, observedRun, slotOffset)
import Replacement.RtosOwnFirst.Regions (Domain (..), SlotOffset (..), domainLabel, hexText)
import System.Directory (copyFile, createDirectoryIfMissing, doesFileExist)
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
  , recordedBoundary :: [DifferenceKind]
  , -- | Display text only; the typed boundary decides whether a replay reproduces.
    recordedDifferences :: [String]
  , recordedBatches :: ReplayBatches
  }

data PreparedCase = PreparedCase
  { preparedRecorded :: RecordedCase
  , preparedReferenceValue :: Value
  , preparedSubjectValue :: Value
  , preparedEntrySp :: Word32
  , preparedReferenceIdentity :: String
  , preparedSubjectIdentity :: String
  }

data ReplayCaseResult = ReplayCaseResult
  { replayRecorded :: RecordedCase
  , replayReferenceIdentity :: String
  , replaySubjectIdentity :: String
  , replayKinds :: [DifferenceKind]
  , replayDifferences :: [String]
  , replayReproduces :: Bool
  }

-- | Reads every recorded original and reduced case of a schema 1 +0x16c generation summary.
readRecordedCases :: Value -> Either String [RecordedCase]
readRecordedCases summary = do
  unless (get summary "schema" == Number 1) (Left "replay source is not a schema 1 summary")
  unless (str (get summary "mode") == "generate") (Left "replay source is not a generation summary")
  unless (str (get summary "slot") == expectedSlot) (Left ("replay source is not the " ++ expectedSlot ++ " generated domain"))
  cases <- concat <$> mapM domainCases (list (get summary "domains"))
  unless (not (null cases)) (Left "replay source records no original or reduced case")
  Right cases
  where
    expectedSlot = case slotOffset generationSlot of SlotOffset offset -> hexText offset
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
      boundary <- mapM parseDifferenceKind =<< mapM text (list (get value "boundary"))
      unless (not (null boundary)) (Left ("recorded case " ++ ident ++ " has no boundary"))
      differences <- mapM text (list (get value "differences"))
      unless (not (null differences)) (Left ("recorded case " ++ ident ++ " has no differences"))
      let batches = get value "batches"
      reference <- textField batches "reference"
      subject <- textField batches "subject"
      Right (RecordedCase role domainId kind ident boundary differences (ReplayBatches reference subject))
    roleOf "owned" = Right OwnedRole
    roleOf name = case find ((== name) . controlName) mutants of
      Just control -> Right (ControlRole control)
      Nothing -> Left ("recorded role " ++ show name ++ " is neither the owned body nor a current control")
    textField value key = text (get value key)
    text (String value) = Right (unpack value)
    text _ = Left "recorded case field must be a string"

-- | The typed boundary kinds a generation summary stores, for example @RegisterKind R0@.
parseDifferenceKind :: String -> Either String DifferenceKind
parseDifferenceKind text = case text of
  "StatusKind" -> Right StatusKind
  "ControlKind" -> Right ControlKind
  "WritesKind" -> Right WritesKind
  "ReferenceTruncatedKind" -> Right ReferenceTruncatedKind
  "SubjectTruncatedKind" -> Right SubjectTruncatedKind
  _ -> case stripPrefix "RegisterKind " text of
    Just label -> maybe (Left ("unknown recorded register " ++ show label)) (Right . RegisterKind) (lookup label registerShows)
    Nothing -> case stripPrefix "MemoryKind (RegionId \"" text >>= stripSuffix "\")" of
      Just name -> Right (MemoryKind (RegionId name))
      Nothing -> Left ("unknown recorded boundary " ++ show text)
  where
    registerShows = [(show register, register) | register <- registerLabels]
    stripSuffix suffix value = reverse <$> stripPrefix (reverse suffix) (reverse value)

-- | Checks every binding and the entry SP of one pair without running a guest batch.
preflightCase :: String -> RecordedCase -> IO PreparedCase
preflightCase runnerHash recorded = do
  let batches = recordedBatches recorded
      expected = recordedCaseId recorded
  (referenceValue, referenceRegions, referenceIdentity) <- loadPairCase (replayReferenceBatch batches) expected
  (subjectValue, subjectRegions, subjectIdentity) <- loadPairCase (replaySubjectBatch batches) expected
  let problems = bindingDifferences referenceValue referenceRegions subjectValue subjectRegions
  unless (null problems) $
    failH ("recorded case " ++ expected ++ " changed its " ++ unwords problems ++ " binding")
  checkReport (replayReferenceBatch batches) expected runnerHash (identityField "case_sha256" referenceIdentity) (identityField "wire_sha256" referenceIdentity)
  checkReport (replaySubjectBatch batches) expected runnerHash (identityField "case_sha256" subjectIdentity) (identityField "wire_sha256" subjectIdentity)
  entrySp <- case parseWord32 (str (get (get referenceValue "registers") "sp")) of
    Just value -> pure value
    Nothing -> failH ("recorded case " ++ expected ++ " has no entry sp")
  pure
    PreparedCase
      { preparedRecorded = recorded
      , preparedReferenceValue = referenceValue
      , preparedSubjectValue = subjectValue
      , preparedEntrySp = entrySp
      , preparedReferenceIdentity = identityField "case_sha256" referenceIdentity
      , preparedSubjectIdentity = identityField "case_sha256" subjectIdentity
      }

-- | Copies the selected case and its declared artifacts, then re-executes the pair.
executeCase :: FilePath -> FilePath -> PreparedCase -> IO ReplayCaseResult
executeCase runner work prepared = do
  let recorded = preparedRecorded prepared
      expected = recordedCaseId recorded
      batches = recordedBatches recorded
      directory = roleLabel (recordedRole recorded) ++ "-" ++ domainLabel (recordedDomain recorded) ++ "-" ++ recordedKindText (recordedKind recorded) ++ "-" ++ expected
      freshReference = work </> directory </> "reference" </> "batch-0"
      freshSubject = work </> directory </> "subject" </> "batch-0"
  copyCaseFor (replayReferenceBatch batches) freshReference expected (preparedReferenceValue prepared)
  copyCaseFor (replaySubjectBatch batches) freshSubject expected (preparedSubjectValue prepared)
  referenceObserved <- loadObservedBatch runner (freshReference </> "batch.json")
  subjectObserved <- loadObservedBatch runner (freshSubject </> "batch.json")
  (referenceLabel, referenceObservation) <- oneObservation freshReference referenceObserved
  (subjectLabel, subjectObservation) <- oneObservation freshSubject subjectObserved
  unless (referenceLabel == expected && subjectLabel == expected) $
    failH ("replayed labels " ++ show (referenceLabel, subjectLabel) ++ " differ from the recorded case " ++ expected)
  referenceRun <- observedRun freshReference referenceLabel (recordedDomain recorded) (preparedEntrySp prepared) referenceObservation
  subjectRun <- observedRun freshSubject subjectLabel (recordedDomain recorded) (preparedEntrySp prepared) subjectObservation
  unless (runStatus referenceRun == Returned) $
    failH ("reference run of " ++ expected ++ " did not return")
  let differences = compareRuns referenceRun subjectRun
      kinds = map differenceKind differences
  pure
    ReplayCaseResult
      { replayRecorded = recorded
      , replayReferenceIdentity = preparedReferenceIdentity prepared
      , replaySubjectIdentity = preparedSubjectIdentity prepared
      , replayKinds = kinds
      , replayDifferences = map differenceText differences
      , replayReproduces = not (null (recordedBoundary recorded)) && kinds == recordedBoundary recorded
      }

-- | Preflight then execute one pair, for callers that replay a single case.
replayCase :: FilePath -> FilePath -> RecordedCase -> IO ReplayCaseResult
replayCase runner work recorded = do
  runnerHash <- runnerDigest runner
  prepared <- preflightCase runnerHash recorded
  executeCase runner work prepared

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

-- | Copies the selected case entry, its case file and only the artifacts it declares.
copyCaseFor :: FilePath -> FilePath -> String -> Value -> IO ()
copyCaseFor source destination expected caseValue = do
  createDirectoryIfMissing True destination
  let batchPath = source </> "batch.json"
  bytes <- B.readFile batchPath
  batch <- either (\problem -> failH (batchPath ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  case recordedEntries batch expected of
    [entry] -> do
      let caseFile = str (get entry "path")
      B.writeFile (destination </> "batch.json") (encodeCanonical (object ["schema_version" .= (1 :: Int), "cases" .= [entry]]))
      copyFile (source </> caseFile) (destination </> caseFile)
    _ -> failH (batchPath ++ ": the recorded case " ++ expected ++ " is not present exactly once")
  forM_ (list (get caseValue "artifacts")) $ \artifact -> do
    let path = str (get artifact "path")
    copyFile (source </> path) (destination </> path)

oneObservation :: FilePath -> [(CaseId, NativeObservation)] -> IO (String, NativeObservation)
oneObservation directory observed = case observed of
  [(CaseId label, observation)] -> pure (label, observation)
  _ -> failH (directory ++ ": expected one observation, found " ++ show (length observed))
