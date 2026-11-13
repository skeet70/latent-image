{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The shared judge: one comparison report in, one verdict out.
--
-- The rule is the same for every suite. A verdict of @equal@ for an image
-- needs all of:
--
-- * for every case, stock instructions executed inside the extent of the
--   entry the case entered;
-- * a case for every export row of the image, the bound entries the static
--   audit reads;
-- * at least one deliberately wrong owned variant in the report that was
--   detected, with every undetected one listed as a survivor;
-- * the firmware digest and the owned image digest present in the report.
--
-- A comparison the report marks diagnostic is listed and does not change the
-- verdict. Without the firmware the comparison is @not-run@; a report that
-- does not name the firmware is @no-verdict@, as is one that fails any other
-- condition. Only a complete report whose cases all agree is @equal@.
--
-- A cutover report may instead declare a milestone: both sides reach the
-- observation at the declared index, the observations are equal through it,
-- and after it the stock tail is a prefix of the owned tail. That relation
-- is the verdict @equal-up-to-milestone@, never @equal@; a report that
-- declares no milestone cannot receive it.
module Emulation.Judge
  ( -- * Stock-side evidence
    StockEvidence (..)
  , evidenceAt
  , evidenceProblems
    -- * Comparison reports
  , Report (..)
  , ReportCase (..)
  , ReportControl (..)
  , Observation (..)
  , ObservationIndex (..)
  , Milestone (..)
  , FirstDifference (..)
  , parseReport
  , milestoneDifference
    -- * Verdicts
  , JudgeInput (..)
  , Firmware (..)
  , VerdictStatus (..)
  , ConditionName (..)
  , conditionLabel
  , Condition (..)
  , Verdict (..)
  , judgeVersion
  , judgeReport
  , verdictJson
  , verdictText
  , verdictExitCode
  ) where

import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, toJSON)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BL8
import Data.List (intercalate, nub, sortOn)
import Data.Maybe (catMaybes, fromMaybe, listToMaybe, mapMaybe)
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.NativeObservation (parseWord32)
import Emulation.SuiteAudit (MissingEntry (..), parseAddress, word32Hex)
import FujiAF.Host (get, list, num, str)
import System.Exit (ExitCode (..))

-- | The stock side's execution inside one authenticated extent.
data StockEvidence = StockEvidence
  { evidenceHandler :: Word32
  , evidenceExtent :: Word32
  , evidenceTrace :: Int
  , evidenceProvenance :: Int
  , evidenceInstructions :: Word64
  , evidenceStatus :: String
  } deriving (Eq, Show)

-- | Read one native result and count the instructions the stock side executed
-- inside the extent it claims to exercise. The count is the runner's own trace
-- and memory-provenance addresses, not a stubbed return.
evidenceAt :: FilePath -> Word32 -> Word32 -> IO StockEvidence
evidenceAt path handler size = do
  report <- either fail pure . eitherDecodeStrict' =<< B.readFile path
  let runner = get report "runner"
      tracePCs = [pc | entry <- list (get runner "trace"), Just pc <- [parseWord32 (str (get entry "pc"))]]
      provenancePCs = [pc | entry <- list (get runner "memory_provenance"), Just pc <- [parseWord32 (str (get entry "pc"))]]
      inside = [() | pc <- tracePCs ++ provenancePCs, pc >= handler, pc < handler + size]
      traceInside = length [() | pc <- tracePCs, pc >= handler, pc < handler + size]
  pure StockEvidence
    { evidenceHandler = handler
    , evidenceExtent = size
    , evidenceTrace = traceInside
    , evidenceProvenance = length inside - traceInside
    , evidenceInstructions = fromIntegral (num (get runner "instructions"))
    , evidenceStatus = str (get runner "status")
    }

-- | A case whose stock side ran no instruction inside the extent it claims to
-- exercise proves nothing, whatever the comparison then reports.
evidenceProblems :: StockEvidence -> [String]
evidenceProblems evidence =
  [ "stock handler " ++ show (evidenceHandler evidence) ++ " executed no instruction inside its extent"
  | evidenceTrace evidence + evidenceProvenance evidence == 0 ]

-- Reports -----------------------------------------------------------------

-- | One comparison case. @reportCaseEntry@ is the entry the case entered and
-- @reportCaseStockInside@ the stock instructions executed inside that entry's
-- extent. A report that keeps neither cannot establish the stock side ran.
data ReportCase = ReportCase
  { reportCaseName :: String
  , reportCaseEntry :: Maybe Word32
  , reportCaseExtent :: Maybe Word32
  , reportCaseStockInside :: Maybe Int
  , reportCaseDifferences :: [String]
  , reportCaseDiagnostics :: [String]
  } deriving (Eq, Show)

-- | One deliberately wrong owned variant. @reportControlDetected@ is whether
-- the suite reported it different from stock.
data ReportControl = ReportControl
  { reportControlName :: String
  , reportControlDetected :: Bool
  , reportControlExpected :: String
  , reportControlDifferences :: [String]
  , reportControlDiagnostics :: [String]
  } deriving (Eq, Show)

-- | One ordered observation of a cutover comparison. Two observations are
-- equal when their JSON values are equal.
newtype Observation = Observation { observationValue :: Value }
  deriving (Eq, Show)

-- | Zero-based position of an observation in a comparison's ordered sequence.
newtype ObservationIndex = ObservationIndex { observationIndex :: Int }
  deriving (Eq, Show)

-- | A cutover milestone: its name and the observation that marks it.
data Milestone = Milestone
  { milestoneName :: String
  , milestoneMark :: ObservationIndex
  } deriving (Eq, Show)

-- | The first observation at which the milestone relation fails. A side that
-- has no observation at that index is @Nothing@.
data FirstDifference = FirstDifference
  { differenceAt :: ObservationIndex
  , differenceStock :: Maybe Observation
  , differenceOwned :: Maybe Observation
  } deriving (Eq, Show)

data Report = Report
  { reportSuite :: Maybe String
  , reportImageDigests :: [String]
  , reportOwnedDigest :: Maybe String
  , reportCases :: [ReportCase]
  , reportControls :: [ReportControl]
  , reportDiagnostics :: [String]
  , reportMilestone :: Maybe Milestone
  , reportStockObservations :: [Observation]
  , reportOwnedObservations :: [Observation]
  } deriving (Eq, Show)

-- | Parse a comparison report. Field names follow the reports the suites
-- already write: a case names @id@, @entry@ (or @stock_handler@), its
-- @stock_instructions_inside@ (or @stock_handler_instructions@), its
-- @differences@ and its @diagnostic_differences@; a control names @name@ and
-- carries @reported@ (or @detected@); the report names its image digests as
-- @firmware_sha256@, @image_sha256@, @image4_sha256@, @image5_sha256@,
-- @stock_id4_sha256@ or @stock_id5_sha256@, and @owned_sha256@. A cutover
-- report declares @milestone@ as @{"name": NAME, "observation": INDEX}@ and
-- its ordered @stock_observations@ and @owned_observations@. Missing fields
-- are kept as missing so the verdict can name the unmet condition; a row that
-- is not an object, and a milestone that is not a named observation index,
-- are refused.
parseReport :: B.ByteString -> Either String Report
parseReport bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object root -> do
      milestone <- parseMilestone root
      let rows = list (get value "cases") ++ list (get value "replies")
          caseRows = mapMaybe parseCaseRow (zip [0 :: Int ..] rows)
          controlsValue = KM.lookup "controls" root
          controlRows = case controlsValue of
            Just (Array _) -> mapMaybe parseControlRow (zip [0 :: Int ..] (list (get value "controls")))
            _ -> receiptControls root
          controlRowsRefused = case controlsValue of
            Just (Array values) -> length controlRows /= length values
            _ -> False
       in if length caseRows /= length rows
            then Left "comparison report has a case row that is not an object"
            else if controlRowsRefused
              then Left "comparison report has a control row that is not an object"
              else Right Report
                { reportSuite = textField root "suite"
                , reportImageDigests = nub (catMaybes
                    [ textField root "firmware_sha256"
                    , textField root "image_sha256"
                    , textField root "image4_sha256"
                    , textField root "image5_sha256"
                    , textField root "stock_id4_sha256"
                    , textField root "stock_id5_sha256"
                    ])
                , reportOwnedDigest = listToMaybe (catMaybes [textField root "owned_sha256", textField root "owned_image_sha256"])
                , reportCases = caseRows
                , reportControls = controlRows
                , reportDiagnostics = []
                , reportMilestone = milestone
                , reportStockObservations = parseObservations root "stock_observations"
                , reportOwnedObservations = parseObservations root "owned_observations"
                }
    _ -> Left "comparison report must be a JSON object"

parseObservations :: KM.KeyMap Value -> String -> [Observation]
parseObservations root key = map Observation (list (get (Object root) key))

-- | A milestone is declared by name and the index of the observation that
-- marks it. A report that carries one must declare both.
parseMilestone :: KM.KeyMap Value -> Either String (Maybe Milestone)
parseMilestone root = case KM.lookup "milestone" root of
  Nothing -> Right Nothing
  Just Null -> Right Nothing
  Just (Object milestone) -> do
    name <- case KM.lookup "name" milestone of
      Just (String text) -> Right (T.unpack text)
      _ -> Left "comparison report milestone has no name"
    mark <- case KM.lookup "observation" milestone of
      Just (Number number) -> maybe (Left badIndex) Right (toBoundedInteger number)
      Just (String text) -> case reads (T.unpack text) of
        [(index, "")] -> Right index
        _ -> Left badIndex
      _ -> Left badIndex
    if mark < 0 then Left badIndex else Right (Just (Milestone name (ObservationIndex mark)))
  Just _ -> Left "comparison report milestone is not an object"
  where
    badIndex = "comparison report milestone has no observation index"

parseCaseRow :: (Int, Value) -> Maybe ReportCase
parseCaseRow (_, String name) = Just ReportCase
  { reportCaseName = T.unpack name
  , reportCaseEntry = Nothing
  , reportCaseExtent = Nothing
  , reportCaseStockInside = Nothing
  , reportCaseDifferences = []
  , reportCaseDiagnostics = []
  }
parseCaseRow (index, Object row) = Just ReportCase
  { reportCaseName = fromMaybe ("case-" ++ show index) (listToMaybe (catMaybes [textField row "id", textField row "name", textField row "case"]))
  , reportCaseEntry = listToMaybe (mapMaybe (addressField row) ["entry", "stock_handler", "stock_actor_handler", "handler"])
  , reportCaseExtent = listToMaybe (mapMaybe (addressField row) ["extent", "stock_extent"])
  , reportCaseStockInside = listToMaybe (catMaybes
      [ intField row "stock_instructions_inside"
      , intField row "stock_handler_instructions"
      , intField row "stock_actor_instructions"
      , sum <$> sequence [intField row "stock_trace_instructions", intField row "stock_provenance_instructions"]
      ])
  , reportCaseDifferences = stringsField row "differences"
  , reportCaseDiagnostics = stringsField row "diagnostic_differences"
  }
parseCaseRow _ = Nothing

-- | A report may record one deliberately wrong owned variant as exit codes
-- instead of a controls array: @mutant_exit@ with @mutant_failure@, or
-- @decline_exit@ with @decline_failure@. Either is one control.
receiptControls :: KM.KeyMap Value -> [ReportControl]
receiptControls root = mapMaybe receipt [("mutant", "mutant"), ("decline", "decline")]
  where
    receipt (name, prefix) =
      let exit = intField root (prefix ++ "_exit")
          failure = textField root (prefix ++ "_failure")
          flagged = boolField root (prefix ++ "_detected")
       in case (exit, failure, flagged) of
            (Nothing, Nothing, Nothing) -> Nothing
            _ -> Just ReportControl
              { reportControlName = name
              , reportControlDetected = fromMaybe (maybe False (/= 0) exit || maybe False (not . null) failure) flagged
              , reportControlExpected = ""
              , reportControlDifferences = maybe [] (: []) failure
              , reportControlDiagnostics = []
              }

parseControlRow :: (Int, Value) -> Maybe ReportControl
parseControlRow (index, Object row) = Just ReportControl
  { reportControlName = fromMaybe ("control-" ++ show index) (listToMaybe (catMaybes [textField row "name", textField row "variant", textField row "control"]))
  , reportControlDetected = fromMaybe False (listToMaybe (catMaybes [boolField row "reported", boolField row "detected", boolField row "holds"]))
  , reportControlExpected = fromMaybe "" (textField row "expected")
  , reportControlDifferences = stringsField row "differences"
  , reportControlDiagnostics = stringsField row "diagnostic_differences"
  }
parseControlRow _ = Nothing


textField :: KM.KeyMap Value -> String -> Maybe String
textField row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> Just (T.unpack text)
  _ -> Nothing

boolField :: KM.KeyMap Value -> String -> Maybe Bool
boolField row key = case KM.lookup (K.fromString key) row of
  Just (Bool value) -> Just value
  _ -> Nothing

intField :: KM.KeyMap Value -> String -> Maybe Int
intField row key = case KM.lookup (K.fromString key) row of
  Just (Number value) -> toBoundedInteger value
  Just (String text) -> case reads (T.unpack text) of
    [(value, "")] -> Just value
    _ -> Nothing
  _ -> Nothing

addressField :: KM.KeyMap Value -> String -> Maybe Word32
addressField row key = case KM.lookup (K.fromString key) row of
  Just value -> parseAddress value
  Nothing -> Nothing

stringsField :: KM.KeyMap Value -> String -> [String]
stringsField row key = [T.unpack text | String text <- list (get (Object row) key)]

-- Verdicts ----------------------------------------------------------------

data VerdictStatus
  = VerdictEqual
  | VerdictEqualUpToMilestone
  | VerdictDifferent
  | VerdictNoVerdict
  | VerdictNotRun
  deriving (Eq, Show)

data ConditionName
  = StockInsideEntered
  | CaseForEveryExport
  | DetectedWrongVariant
  | NamedDigests
  | InterfaceExecution
  deriving (Eq, Show)

conditionLabel :: ConditionName -> String
conditionLabel StockInsideEntered = "stock-instructions-inside-entered-extent"
conditionLabel CaseForEveryExport = "case-for-every-export-row"
conditionLabel DetectedWrongVariant = "detected-wrong-variant"
conditionLabel NamedDigests = "named-digests"
conditionLabel InterfaceExecution = "interface-execution-inside-declared-extents"

data Condition = Condition
  { conditionName :: ConditionName
  , conditionMet :: Bool
  , conditionDetail :: String
  } deriving (Eq, Show)

-- | The firmware is a declared input to the judge, not something the report
-- may supply on its own.
data Firmware = Firmware
  { firmwarePath :: FilePath
  , firmwareDigest :: String
  } deriving (Eq, Show)

data JudgeInput = JudgeInput
  { inputSuite :: String
  , inputBound :: [MissingEntry]
  , inputFirmware :: Maybe Firmware
  , inputReportDigest :: String
  } deriving (Eq, Show)

data Verdict = Verdict
  { verdictStatus :: VerdictStatus
  , verdictSuite :: String
  , verdictFirmware :: Maybe String
  , verdictOwned :: Maybe String
  , verdictReportDigest :: String
  , verdictConditions :: [Condition]
  , verdictUnmet :: [String]
  , verdictDifferentCases :: [String]
  , verdictSurvivors :: [String]
  , verdictDiagnostics :: [String]
  , verdictReasons :: [String]
  , verdictMilestone :: Maybe Milestone
  , verdictFirstDifference :: Maybe FirstDifference
  } deriving (Eq, Show)

-- | Bumped when the rule changes; the verdict document names it.
judgeVersion :: String
judgeVersion = "3"

judgeReport :: JudgeInput -> Report -> Verdict
judgeReport input report =
  case inputFirmware input of
    Nothing -> base
      { verdictStatus = VerdictNotRun
      , verdictReasons = ["firmware not supplied; the comparison is not run"]
      }
    Just _ ->
      if not (null unmet) then base { verdictStatus = VerdictNoVerdict }
        else case reportMilestone report of
          Just milestone ->
            case milestoneDifference milestone (reportStockObservations report) (reportOwnedObservations report) of
              Nothing -> base
                { verdictStatus = VerdictEqualUpToMilestone
                , verdictMilestone = Just milestone
                , verdictDifferentCases = []
                }
              Just difference -> base
                { verdictStatus = VerdictDifferent
                , verdictMilestone = Just milestone
                , verdictFirstDifference = Just difference
                , verdictDifferentCases = []
                , verdictReasons = milestoneMisses milestone (reportStockObservations report) (reportOwnedObservations report)
                }
          Nothing ->
            if not (null differing) then base { verdictStatus = VerdictDifferent, verdictDifferentCases = differing }
              else base { verdictStatus = VerdictEqual }
  where
    reportOwned = reportOwnedDigest report
    conditions =
      [ stockCondition report
      , coverageCondition (inputBound input) report
      , controlCondition report
      , digestCondition (inputFirmware input) (reportImageDigests report) reportOwned
      ]
    unmet = [conditionDetail condition | condition <- conditions, not (conditionMet condition)]
    differing = [reportCaseName c | c <- reportCases report, not (null (reportCaseDifferences c))]
    survivors = sortOn id [reportControlName c | c <- reportControls report, not (reportControlDetected c)]
    diagnostics =
      [ reportCaseName c ++ ": " ++ difference
      | c <- reportCases report, difference <- reportCaseDiagnostics c ]
        ++ [ reportControlName c ++ ": " ++ difference
           | c <- reportControls report, difference <- reportControlDiagnostics c ]
    base = Verdict
      { verdictStatus = VerdictNoVerdict
      , verdictSuite = inputSuite input
      , verdictFirmware = fmap firmwareDigest (inputFirmware input)
      , verdictOwned = reportOwned
      , verdictReportDigest = inputReportDigest input
      , verdictConditions = conditions
      , verdictUnmet = unmet
      , verdictDifferentCases = differing
      , verdictSurvivors = survivors
      , verdictDiagnostics = diagnostics
      , verdictReasons = []
      , verdictMilestone = Nothing
      , verdictFirstDifference = Nothing
      }

-- | The first observation at which the milestone relation fails, or
-- @Nothing@ when both sides reach the milestone, the observations are equal
-- through it, and the stock tail is a prefix of the owned tail.
milestoneDifference :: Milestone -> [Observation] -> [Observation] -> Maybe FirstDifference
milestoneDifference (Milestone _ (ObservationIndex mark)) stock owned = go 0 stock owned
  where
    go index (stockObservation : stockRest) (ownedObservation : ownedRest)
      | stockObservation /= ownedObservation =
          Just (FirstDifference (ObservationIndex index) (Just stockObservation) (Just ownedObservation))
      | index == mark = tailDifference (index + 1) stockRest ownedRest
      | otherwise = go (index + 1) stockRest ownedRest
    go index [] (ownedObservation : _) =
      Just (FirstDifference (ObservationIndex index) Nothing (Just ownedObservation))
    go index (stockObservation : _) [] =
      Just (FirstDifference (ObservationIndex index) (Just stockObservation) Nothing)
    go _ [] [] =
      Just (FirstDifference (ObservationIndex mark) Nothing Nothing)

    tailDifference index (stockObservation : stockRest) (ownedObservation : ownedRest)
      | stockObservation /= ownedObservation =
          Just (FirstDifference (ObservationIndex index) (Just stockObservation) (Just ownedObservation))
      | otherwise = tailDifference (index + 1) stockRest ownedRest
    tailDifference _ [] _ = Nothing
    tailDifference index (stockObservation : _) [] =
      Just (FirstDifference (ObservationIndex index) (Just stockObservation) Nothing)

-- | The sides that do not reach the milestone observation.
milestoneMisses :: Milestone -> [Observation] -> [Observation] -> [String]
milestoneMisses (Milestone name (ObservationIndex mark)) stock owned =
  [ side ++ " does not reach milestone " ++ name ++ " at observation " ++ show mark
  | (side, observations) <- [("stock", stock), ("owned", owned)]
  , length observations <= mark
  ]

stockCondition :: Report -> Condition
stockCondition report
  | null (reportCases report) =
      Condition StockInsideEntered False "the report carries no case"
  | otherwise =
      Condition StockInsideEntered (null bad) $
        if null bad then show (length (reportCases report)) ++ " cases name an entered entry with stock instructions inside its extent"
          else show (length bad) ++ " cases lack stock instructions inside the entered extent: " ++ intercalate ", " bad
  where
    bad =
      [ reportCaseName c
      | c <- reportCases report
      , reportCaseEntry c == Nothing || maybe True (< 1) (reportCaseStockInside c) ]

coverageCondition :: [MissingEntry] -> Report -> Condition
coverageCondition bound report =
  Condition CaseForEveryExport (null missing) $
    if null missing then show (length bound) ++ " bound entries each have a case"
      else show (length missing) ++ " bound entries have no case: " ++ intercalate ", " (map (word32Hex . missingAddress) missing)
  where
    entered = S.fromList [address | c <- reportCases report, Just address <- [reportCaseEntry c]]
    missing = [entry | entry <- bound, not (S.member (missingAddress entry) entered)]

controlCondition :: Report -> Condition
controlCondition report =
  Condition DetectedWrongVariant (not (null detected)) $
    case (controls, detected) of
      ([], _) -> "the report carries no deliberately wrong owned variant"
      (_, []) -> "no deliberately wrong owned variant was detected; survivors: " ++ intercalate ", " (map reportControlName controls)
      _ -> show (length detected) ++ " of " ++ show (length controls) ++ " deliberately wrong owned variants detected"
  where
    controls = reportControls report
    detected = [c | c <- controls, reportControlDetected c]

digestCondition :: Maybe Firmware -> [String] -> Maybe String -> Condition
digestCondition firmware named reportOwned =
  Condition NamedDigests (null problems) $
    if null problems then "the report names the firmware and owned image digests"
      else intercalate "; " problems
  where
    problems = catMaybes
      [ case firmware of
          Nothing -> Just "no firmware was supplied"
          Just supplied
            | firmwareDigest supplied `elem` named -> Nothing
            | otherwise ->
                Just ("the report names no digest of the supplied firmware " ++ firmwareDigest supplied
                      ++ (if null named then "" else " (named: " ++ intercalate ", " named ++ ")"))
      , case reportOwned of
          Nothing -> Just "the report names no owned image digest"
          Just _ -> Nothing
      ]

verdictStatusLabel :: VerdictStatus -> String
verdictStatusLabel VerdictEqual = "equal"
verdictStatusLabel VerdictEqualUpToMilestone = "equal-up-to-milestone"
verdictStatusLabel VerdictDifferent = "different"
verdictStatusLabel VerdictNoVerdict = "no-verdict"
verdictStatusLabel VerdictNotRun = "not-run"

verdictText :: Verdict -> String
verdictText verdict =
  "suite " ++ verdictSuite verdict
    ++ " verdict " ++ verdictStatusLabel (verdictStatus verdict)
    ++ " firmware " ++ maybe "none" id (verdictFirmware verdict)
    ++ " image " ++ maybe "none" id (verdictOwned verdict)
    ++ " report " ++ verdictReportDigest verdict
    ++ " judge " ++ judgeVersion
    ++ (if null (verdictUnmet verdict) then "" else " unmet: " ++ intercalate "; " (verdictUnmet verdict))
    ++ (if null (verdictDifferentCases verdict) then "" else " different: " ++ intercalate ", " (verdictDifferentCases verdict))
    ++ (if null (verdictSurvivors verdict) then "" else " survivors: " ++ intercalate ", " (verdictSurvivors verdict))
    ++ (if null (verdictDiagnostics verdict) then "" else " diagnostics: " ++ intercalate ", " (verdictDiagnostics verdict))
    ++ (if null (verdictReasons verdict) then "" else " reasons: " ++ intercalate "; " (verdictReasons verdict))
    ++ maybe "" (\milestone -> " milestone " ++ milestoneName milestone
        ++ " at observation " ++ show (observationIndex (milestoneMark milestone))) (verdictMilestone verdict)
    ++ maybe "" (\difference -> " first-difference " ++ differenceText difference) (verdictFirstDifference verdict)

milestoneJson :: Milestone -> Value
milestoneJson milestone = object
  [ ("name", toJSON (milestoneName milestone))
  , ("observation", toJSON (observationIndex (milestoneMark milestone)))
  ]

differenceText :: FirstDifference -> String
differenceText difference =
  show (observationIndex (differenceAt difference))
    ++ " stock " ++ maybe "absent" observationText (differenceStock difference)
    ++ " owned " ++ maybe "absent" observationText (differenceOwned difference)

observationText :: Observation -> String
observationText observation = take 200 (BL8.unpack (encode (observationValue observation)))

firstDifferenceJson :: FirstDifference -> Value
firstDifferenceJson difference = object
  [ ("observation", toJSON (observationIndex (differenceAt difference)))
  , ("stock", maybe Null observationValue (differenceStock difference))
  , ("owned", maybe Null observationValue (differenceOwned difference))
  ]

verdictJson :: Verdict -> Value
verdictJson verdict = object
  [ ("schema", Number 1)
  , ("judge_version", toJSON judgeVersion)
  , ("suite", toJSON (verdictSuite verdict))
  , ("verdict", toJSON (verdictStatusLabel (verdictStatus verdict)))
  , ("firmware_sha256", maybe Null toJSON (verdictFirmware verdict))
  , ("image_sha256", maybe Null toJSON (verdictOwned verdict))
  , ("report_sha256", toJSON (verdictReportDigest verdict))
  , ("conditions", toJSON
      [ object
          [ ("condition", toJSON (conditionLabel (conditionName condition)))
          , ("met", toJSON (conditionMet condition))
          , ("detail", toJSON (conditionDetail condition))
          ]
      | condition <- verdictConditions verdict ])
  , ("unmet", toJSON (verdictUnmet verdict))
  , ("different_cases", toJSON (verdictDifferentCases verdict))
  , ("survivors", toJSON (verdictSurvivors verdict))
  , ("diagnostics", toJSON (verdictDiagnostics verdict))
  , ("reasons", toJSON (verdictReasons verdict))
  , ("milestone", maybe Null milestoneJson (verdictMilestone verdict))
  , ("first_difference", maybe Null firstDifferenceJson (verdictFirstDifference verdict))
  ]

-- | Only @equal@ is a pass. A @no-verdict@ and a @not-run@ are never reported
-- or exit-coded as one; @equal-up-to-milestone@ has its own code so a caller
-- cannot mistake it for @equal@.
verdictExitCode :: VerdictStatus -> ExitCode
verdictExitCode VerdictEqual = ExitSuccess
verdictExitCode VerdictDifferent = ExitFailure 1
verdictExitCode VerdictNoVerdict = ExitFailure 2
verdictExitCode VerdictNotRun = ExitFailure 3
verdictExitCode VerdictEqualUpToMilestone = ExitFailure 4
