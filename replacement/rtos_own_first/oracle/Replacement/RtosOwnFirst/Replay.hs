{-# LANGUAGE OverloadedStrings #-}

-- | Replays the original and reduced native batches a generation summary retains. Every recorded
-- pair is preflighted before the first one runs: the runner, case, wire and region bindings are
-- checked, the entry SP comes from the admitted case registers, and only the selected case and its
-- declared artifacts are copied to a fresh directory, leaving preserved reports untouched.
module Replacement.RtosOwnFirst.Replay
  ( RecordedKind (..)
  , recordedKindText
  , RecordedCase (..)
  , PreparedCase
  , ReplayStatus (..)
  , ReplayCaseResult (..)
  , replayReproduces
  , readRecordedCases
  , parseDifferenceKind
  , preflightCase
  , executeCase
  , replayCase
  ) where

import Control.Exception (IOException, SomeAsyncException, SomeException, fromException, try, tryJust)
import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.List (find, isPrefixOf, stripPrefix)
import Data.Text (unpack)
import Emulation.NativeCase (Region (..), loadCase, runnerDigest)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation, RegionId (..), RunnerStatus (..), parseWord32, registerLabels)
import Emulation.OwnedCase (CaseId (..), loadObservedBatch)
import Emulation.Sha256 (Sha256, parseSha256Hex)
import FujiAF.Host (ensure, failH, get, keys, list, str)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Controls (controlName, controlSlot, mutants)
import Replacement.RtosOwnFirst.Generate (generationSlot)
import Replacement.RtosOwnFirst.Generated (BodyRole (..), ReplayBatches (..), roleLabel)
import Replacement.RtosOwnFirst.Oracle (DifferenceKind (..), Run (..), compareRuns, differenceKind, differenceText, observedRun, slotOffset)
import Replacement.RtosOwnFirst.Regions (Domain (..), SlotOffset (..), domainLabel, hexText)
import System.Directory (canonicalizePath, copyFile, createDirectoryIfMissing, doesFileExist)
import System.FilePath (isRelative, splitDirectories, takeDirectory, takeFileName, (</>))

data RecordedKind = RecordedOriginal | RecordedReduced
  deriving (Eq, Show)

recordedKindText :: RecordedKind -> String
recordedKindText RecordedOriginal = "original"
recordedKindText RecordedReduced = "reduced"

data RecordedCase = RecordedCase
  { recordedRole :: BodyRole
  , recordedDomain :: Domain
  , recordedKind :: RecordedKind
  , recordedCaseId :: CaseId
  , recordedBoundary :: [DifferenceKind]
  , -- | Display text only; the typed boundary decides whether a replay reproduces.
    recordedDifferences :: [String]
  , recordedBatches :: ReplayBatches
  }

data CaseIdentity = CaseIdentity
  { identityCase :: Sha256
  , identityWire :: Sha256
  }
  deriving Eq

data CaseFiles = CaseFiles
  { filesCase :: FilePath
  , filesArtifacts :: [FilePath]
  }

data AdmittedCase = AdmittedCase
  { admittedValue :: Value
  , admittedRegions :: [Region]
  , admittedIdentity :: CaseIdentity
  , admittedFiles :: CaseFiles
  }

data PreparedCase = PreparedCase
  { preparedRecorded :: RecordedCase
  , preparedReferenceFiles :: CaseFiles
  , preparedSubjectFiles :: CaseFiles
  , preparedEntrySp :: GuestAddress
  , preparedReferenceIdentity :: CaseIdentity
  , preparedSubjectIdentity :: CaseIdentity
  }

data ReplayStatus
  = ReplayReproduced
  | ReplayKindsDiffer
  | ReplayIncomplete String
  deriving (Eq, Show)

data ReplayCaseResult = ReplayCaseResult
  { replayRecorded :: RecordedCase
  , replayReferenceIdentity :: Sha256
  , replaySubjectIdentity :: Sha256
  , replayKinds :: [DifferenceKind]
  , replayDifferences :: [String]
  , replayStatus :: ReplayStatus
  }

replayReproduces :: ReplayCaseResult -> Bool
replayReproduces result = replayStatus result == ReplayReproduced

-- | Reads every recorded original and reduced case of a schema 1 +0x16c generation summary.
readRecordedCases :: Value -> Either String [RecordedCase]
readRecordedCases summary = do
  unless (get summary "schema" == Number 1) (Left "replay source is not a schema 1 summary")
  unless (str (get summary "mode") == "generate") (Left "replay source is not a generation summary")
  unless (str (get summary "slot") == expectedSlot) (Left ("replay source is not the " ++ expectedSlot ++ " generated domain"))
  controlNameText <- text (get summary "control")
  control <- case find ((== controlNameText) . controlName) mutants of
    Just candidate | controlSlot candidate == generationSlot -> Right candidate
    _ -> Left ("replay source control " ++ show controlNameText ++ " is not the current " ++ expectedSlot ++ " control")
  cases <- concat <$> mapM (domainCases control) (list (get summary "domains"))
  unless (not (null cases)) (Left "replay source records no original or reduced case")
  Right cases
  where
    expectedSlot = case slotOffset generationSlot of SlotOffset offset -> hexText offset
    domainCases control domain = do
      label <- text (get domain "domain")
      domainId <- case label of
        "a" -> Right DomainA
        "b" -> Right DomainB
        other -> Left ("unknown recorded domain " ++ show other)
      unless (all (`elem` keys domain) ["candidate_reduction", "control_reduction"]) $
        Left ("recorded domain " ++ label ++ " lacks a candidate or control reduction")
      reductions <-
        sequence
          [ reduction domainId "candidate_reduction" OwnedRole (get domain "candidate_reduction")
          , reduction domainId "control_reduction" (ControlRole control) (get domain "control_reduction")
          ]
      Right (concat reductions)
    reduction _ _ _ Null = Right []
    reduction domainId key expectedRole value = do
      name <- text (get value "role")
      unless (name == roleLabel expectedRole) $
        Left ("recorded " ++ key ++ " role " ++ show name ++ " is not " ++ roleLabel expectedRole)
      original <- caseOf domainId expectedRole RecordedOriginal (get value "original")
      reduced <- caseOf domainId expectedRole RecordedReduced (get value "reduced")
      unless (recordedBoundary original == recordedBoundary reduced) $
        Left ("recorded " ++ key ++ " original and reduced boundaries differ")
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
      Right (RecordedCase role domainId kind (CaseId ident) boundary differences (ReplayBatches reference subject))
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
preflightCase :: Sha256 -> RecordedCase -> IO PreparedCase
preflightCase runnerHash recorded = do
  let batches = recordedBatches recorded
      expected = recordedCaseId recorded
      label = caseIdText expected
  reference <- loadPairCase (replayReferenceBatch batches) expected
  subject <- loadPairCase (replaySubjectBatch batches) expected
  let problems = bindingDifferences (admittedValue reference) (admittedRegions reference) (admittedValue subject) (admittedRegions subject)
  unless (null problems) $
    failH ("recorded case " ++ label ++ " changed its " ++ unwords problems ++ " binding")
  checkReport (replayReferenceBatch batches) expected runnerHash (admittedIdentity reference)
  checkReport (replaySubjectBatch batches) expected runnerHash (admittedIdentity subject)
  entrySp <- case parseWord32 (str (get (get (admittedValue reference) "registers") "sp")) of
    Just value -> pure (GuestAddress (fromIntegral value))
    Nothing -> failH ("recorded case " ++ label ++ " has no entry sp")
  pure
    PreparedCase
      { preparedRecorded = recorded
      , preparedReferenceFiles = admittedFiles reference
      , preparedSubjectFiles = admittedFiles subject
      , preparedEntrySp = entrySp
      , preparedReferenceIdentity = admittedIdentity reference
      , preparedSubjectIdentity = admittedIdentity subject
      }

-- | Copies the selected case and its declared artifacts, then re-executes the pair. A copy or
-- execution failure becomes an incomplete result rather than aborting before summary publication.
executeCase :: FilePath -> FilePath -> PreparedCase -> IO ReplayCaseResult
executeCase runner work prepared = do
  outcome <- tryJust synchronous (executePair runner work prepared) :: IO (Either SomeException ReplayCaseResult)
  pure $ case outcome of
    Right result -> result
    Left problem -> incomplete prepared (take 4096 (show problem))
  where
    synchronous problem = case fromException problem :: Maybe SomeAsyncException of
      Just _ -> Nothing
      Nothing -> Just problem

incomplete :: PreparedCase -> String -> ReplayCaseResult
incomplete prepared reason =
  ReplayCaseResult
    { replayRecorded = preparedRecorded prepared
    , replayReferenceIdentity = identityCase (preparedReferenceIdentity prepared)
    , replaySubjectIdentity = identityCase (preparedSubjectIdentity prepared)
    , replayKinds = []
    , replayDifferences = []
    , replayStatus = ReplayIncomplete reason
    }

executePair :: FilePath -> FilePath -> PreparedCase -> IO ReplayCaseResult
executePair runner work prepared = do
  let recorded = preparedRecorded prepared
      expected = recordedCaseId recorded
      directory = roleLabel (recordedRole recorded) ++ "-" ++ domainLabel (recordedDomain recorded) ++ "-" ++ recordedKindText (recordedKind recorded) ++ "-" ++ caseIdText expected
      freshReference = work </> directory </> "reference" </> "batch-0"
      freshSubject = work </> directory </> "subject" </> "batch-0"
  copyCaseFor freshReference expected (preparedReferenceFiles prepared)
  copyCaseFor freshSubject expected (preparedSubjectFiles prepared)
  referenceCopy <- loadPairCase freshReference expected
  subjectCopy <- loadPairCase freshSubject expected
  ensure (admittedIdentity referenceCopy == preparedReferenceIdentity prepared) "copied reference case differs from its preflight identity"
  ensure (admittedIdentity subjectCopy == preparedSubjectIdentity prepared) "copied subject case differs from its preflight identity"
  referenceObserved <- loadObservedBatch runner (freshReference </> "batch.json")
  subjectObserved <- loadObservedBatch runner (freshSubject </> "batch.json")
  (referenceLabel, referenceObservation) <- oneObservation freshReference referenceObserved
  (subjectLabel, subjectObservation) <- oneObservation freshSubject subjectObserved
  unless (referenceLabel == expected && subjectLabel == expected) $
    failH ("replayed labels " ++ show (referenceLabel, subjectLabel) ++ " differ from the recorded case " ++ caseIdText expected)
  referenceRun <- observedRun freshReference referenceLabel (recordedDomain recorded) (preparedEntrySp prepared) referenceObservation
  subjectRun <- observedRun freshSubject subjectLabel (recordedDomain recorded) (preparedEntrySp prepared) subjectObservation
  if runStatus referenceRun /= Returned
    then pure (incomplete prepared "reference did not return")
    else do
      let differences = compareRuns referenceRun subjectRun
          kinds = map differenceKind differences
          status = if kinds == recordedBoundary recorded then ReplayReproduced else ReplayKindsDiffer
      pure
        ReplayCaseResult
          { replayRecorded = recorded
          , replayReferenceIdentity = identityCase (preparedReferenceIdentity prepared)
          , replaySubjectIdentity = identityCase (preparedSubjectIdentity prepared)
          , replayKinds = kinds
          , replayDifferences = map differenceText differences
          , replayStatus = status
          }

-- | Preflight then execute one pair, for callers that replay a single case.
replayCase :: FilePath -> FilePath -> RecordedCase -> IO ReplayCaseResult
replayCase runner work recorded = do
  runnerHash <- runnerDigest runner >>= either failH pure . parseSha256Hex
  prepared <- preflightCase runnerHash recorded
  executeCase runner work prepared

loadPairCase :: FilePath -> CaseId -> IO AdmittedCase
loadPairCase directory expected = do
  path <- batchCasePath directory (caseIdText expected)
  loaded <- try (loadCase path) :: IO (Either IOException (Value, [Region], B.ByteString, [(String, Value)]))
  case loaded of
    Left problem -> failH (path ++ ": " ++ show problem)
    Right (value, regions, _, identity) -> do
      ensure (str (get value "id") == caseIdText expected) (path ++ ": case id differs from " ++ caseIdText expected)
      caseHash <- either failH pure (parseSha256Hex (identityField "case_sha256" identity))
      wireHash <- either failH pure (parseSha256Hex (identityField "wire_sha256" identity))
      let files = CaseFiles path [str (get artifact "path") | artifact <- list (get value "artifacts")]
      pure (AdmittedCase value regions (CaseIdentity caseHash wireHash) files)

batchCasePath :: FilePath -> String -> IO FilePath
batchCasePath directory expected = do
  let batchPath = directory </> "batch.json"
  exists <- doesFileExist batchPath
  unless exists (failH (batchPath ++ ": missing batch"))
  bytes <- B.readFile batchPath
  batch <- either (\problem -> failH (batchPath ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  case recordedEntries batch expected of
    [entry] -> do
      let relative = str (get entry "path")
      unless (not (null relative) && isRelative relative && ".." `notElem` splitDirectories relative) $
        failH (batchPath ++ ": case path " ++ show relative ++ " is not a contained relative path")
      root <- canonicalizePath directory
      path <- canonicalizePath (directory </> relative)
      unless (splitDirectories root `isPrefixOf` splitDirectories path) $
        failH (batchPath ++ ": case path " ++ show relative ++ " escapes the batch directory")
      pure path
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
checkReport :: FilePath -> CaseId -> Sha256 -> CaseIdentity -> IO ()
checkReport directory expected runnerHash identity = do
  let label = caseIdText expected
      path = directory </> label ++ ".native-result.json"
  exists <- doesFileExist path
  unless exists (failH (path ++ ": missing preserved report"))
  bytes <- B.readFile path
  report <- either (\problem -> failH (path ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes)
  let digestField key = either (\problem -> failH (path ++ ": " ++ problem)) pure (parseSha256Hex (str (get report key)))
      recordedId = CaseId (str (get report "case_id"))
  recordedRunner <- digestField "runner_sha256"
  recordedCase <- digestField "case_sha256"
  recordedWire <- digestField "wire_sha256"
  ensure (recordedId == expected) (path ++ ": report case id " ++ show recordedId ++ " differs from " ++ label)
  ensure (recordedRunner == runnerHash) (path ++ ": runner binding differs from the current runner")
  ensure (recordedCase == identityCase identity) (path ++ ": case binding differs from the case file")
  ensure (recordedWire == identityWire identity) (path ++ ": wire binding differs from the case file")

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
copyCaseFor :: FilePath -> CaseId -> CaseFiles -> IO ()
copyCaseFor destination expected files = do
  createDirectoryIfMissing True destination
  let caseFile = takeFileName (filesCase files)
      entry = object ["id" .= caseIdText expected, "path" .= caseFile]
  B.writeFile (destination </> "batch.json") (encodeCanonical (object ["schema_version" .= (1 :: Int), "cases" .= [entry]]))
  copyFile (filesCase files) (destination </> caseFile)
  forM_ (filesArtifacts files) $ \path -> do
    let target = destination </> path
    createDirectoryIfMissing True (takeDirectory target)
    copyFile (takeDirectory (filesCase files) </> path) target

oneObservation :: FilePath -> [(CaseId, NativeObservation)] -> IO (CaseId, NativeObservation)
oneObservation directory observed = case observed of
  [(ident, observation)] -> pure (ident, observation)
  _ -> failH (directory ++ ": expected one observation, found " ++ show (length observed))
