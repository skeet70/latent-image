{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.InterfaceJudge
  ( InterfaceReport
  , ReportDigest (..)
  , parseInterfaceReport
  , judgeInterfaceReport
  , interfaceVerdictJson
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', withObject, withText, (.:), (.:?), (.!=))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (sort)
import Data.Maybe (isNothing)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.Judge

newtype Subsystem = Subsystem T.Text deriving (Eq, Show)
newtype Scenario = Scenario T.Text deriving (Eq, Show)
newtype Variant = Variant T.Text deriving (Eq, Show)
newtype Digest = Digest T.Text deriving (Eq, Show)
newtype ReportDigest = ReportDigest String deriving (Eq, Show)
newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
newtype FetchCount = FetchCount Word64 deriving (Eq, Show)

data Span = Span GuestAddress GuestAddress deriving (Eq, Show)
data FetchReceipt = FetchReceipt Span FetchCount deriving (Eq, Show)
data StreamRun = StreamRun
  { runDigest :: Digest
  , runComplete :: Bool
  , runFetches :: [FetchReceipt]
  , runEvents :: [Observation]
  } deriving (Eq, Show)
data WrongRun = WrongRun Variant StreamRun deriving (Eq, Show)
data InterfaceReport = InterfaceReport
  { subsystem :: Subsystem
  , scenario :: Scenario
  , retiredExtents :: [Span]
  , ownedExtents :: [Span]
  , stockRun :: StreamRun
  , ownedRun :: StreamRun
  , wrongRuns :: [WrongRun]
  , milestone :: Maybe Milestone
  } deriving (Eq, Show)

name :: String -> (T.Text -> a) -> Value -> J.Parser a
name label make = withText label $ \text -> do
  unless (not (T.null (T.strip text))) (fail (label ++ " must not be empty"))
  pure (make text)

instance FromJSON Subsystem where parseJSON = name "subsystem" Subsystem
instance FromJSON Scenario where parseJSON = name "scenario" Scenario
instance FromJSON Variant where parseJSON = name "variant" Variant
instance FromJSON Digest where
  parseJSON = withText "SHA-256" $ \text -> do
    unless (T.length text == 64 && T.all isHexDigit text) (fail "64 hexadecimal SHA-256 digits required")
    pure (Digest (T.toLower text))
instance FromJSON GuestAddress where parseJSON value = GuestAddress <$> parseJSON value
instance FromJSON FetchCount where parseJSON value = FetchCount <$> parseJSON value
instance FromJSON Span where
  parseJSON = withObject "guest extent" $ \value -> do
    start <- value .: "start"
    end <- value .: "end"
    unless (start < end) (fail "guest extent must be nonempty and nonwrapping")
    pure (Span start end)
instance FromJSON FetchReceipt where
  parseJSON = withObject "fetch receipt" $ \value -> FetchReceipt <$> parseJSON (Object value) <*> value .: "fetches"
instance FromJSON StreamRun where
  parseJSON = withObject "interface run" $ \value -> StreamRun
    <$> value .: "image_sha256"
    <*> value .:? "complete" .!= False
    <*> value .:? "fetch_receipts" .!= []
    <*> (map Observation <$> value .: "events")
instance FromJSON WrongRun where
  parseJSON = withObject "wrong interface run" $ \value -> WrongRun <$> value .: "name" <*> value .: "run"
instance FromJSON InterfaceReport where
  parseJSON = withObject "interface-stream report" $ \value -> do
    schema <- value .: "schema" :: J.Parser Int
    mode <- value .: "mode" :: J.Parser T.Text
    unless (schema == 1 && mode == "interface-stream") (fail "schema-1 interface-stream report required")
    marker <- value .:? "milestone"
    mark <- traverse (withObject "milestone" $ \row -> do
      label <- row .: "name"
      index <- row .: "observation"
      unless (not (null label) && index >= 0) (fail "named nonnegative milestone required")
      pure (Milestone label (ObservationIndex index))) marker
    InterfaceReport <$> value .: "subsystem" <*> value .: "scenario"
      <*> value .:? "retired_extents" .!= [] <*> value .:? "owned_extents" .!= []
      <*> value .: "stock" <*> value .: "owned" <*> value .:? "controls" .!= [] <*> pure mark

parseInterfaceReport :: B.ByteString -> Either String InterfaceReport
parseInterfaceReport = eitherDecodeStrict'

executed :: [Span] -> StreamRun -> Bool
executed extents run = runComplete run && any positive (runFetches run)
  && all admitted (runFetches run)
  where
    positive (FetchReceipt _ (FetchCount count)) = count > 0
    admitted (FetchReceipt (Span start end) _) = any (\(Span low high) -> low <= start && end <= high) extents

firstDifference :: [Observation] -> [Observation] -> Maybe FirstDifference
firstDifference = go 0
  where
    go _ [] [] = Nothing
    go index (left : lefts) (right : rights)
      | left == right = go (index + 1) lefts rights
    go index left right = Just (FirstDifference (ObservationIndex index) (first left) (first right))
    first [] = Nothing
    first (value : _) = Just value

digestText :: Digest -> String
digestText (Digest text) = T.unpack text

judgeInterfaceReport :: Maybe Firmware -> ReportDigest -> InterfaceReport -> Verdict
judgeInterfaceReport firmware (ReportDigest reportDigest) report = base
  { verdictStatus = status
  , verdictDifferentCases = [scenarioName | status == VerdictDifferent]
  , verdictFirstDifference = if status == VerdictDifferent then difference else Nothing
  }
  where
    Subsystem subsystemName = subsystem report
    Scenario scenarioText = scenario report
    scenarioName = T.unpack scenarioText
    stock = stockRun report
    owned = ownedRun report
    controlEvidence = all (\(WrongRun _ run) -> executed (ownedExtents report) run) (wrongRuns report)
    detected (WrongRun _ run) = not (isNothing (case milestone report of
      Just marker -> milestoneDifference marker (runEvents stock) (runEvents run)
      Nothing -> firstDifference (runEvents stock) (runEvents run)))
    controls = wrongRuns report
    survivors = sort [T.unpack label | wrong@(WrongRun (Variant label) _) <- controls, not (detected wrong)]
    evidence = executed (retiredExtents report) stock && executed (ownedExtents report) owned && controlEvidence
    named = maybe False ((== digestText (runDigest stock)) . firmwareDigest) firmware
    conditions =
      [ Condition InterfaceExecution evidence "complete stock, owned and control fetch receipts inside declared subsystem extents"
      , Condition DetectedWrongVariant (any detected controls) "at least one executed wrong variant differs from the stock stream"
      , Condition NamedDigests named "supplied firmware matches the stock run image digest"
      ]
    unmet = [conditionDetail condition | condition <- conditions, not (conditionMet condition)]
    difference = case milestone report of
      Just marker -> milestoneDifference marker (runEvents stock) (runEvents owned)
      Nothing -> firstDifference (runEvents stock) (runEvents owned)
    status
      | isNothing firmware = VerdictNotRun
      | not (null unmet) = VerdictNoVerdict
      | not (isNothing difference) = VerdictDifferent
      | isNothing (milestone report) = VerdictEqual
      | otherwise = VerdictEqualUpToMilestone
    base = Verdict
      { verdictStatus = VerdictNoVerdict
      , verdictSuite = T.unpack subsystemName ++ ":" ++ scenarioName
      , verdictFirmware = firmwareDigest <$> firmware
      , verdictOwned = Just (digestText (runDigest owned))
      , verdictReportDigest = reportDigest
      , verdictConditions = conditions
      , verdictUnmet = unmet
      , verdictDifferentCases = []
      , verdictSurvivors = survivors
      , verdictDiagnostics = []
      , verdictReasons = ["firmware not supplied; the comparison is not run" | isNothing firmware]
      , verdictMilestone = milestone report
      , verdictFirstDifference = Nothing
      }

interfaceVerdictJson :: InterfaceReport -> Verdict -> Value
interfaceVerdictJson report verdict = case verdictJson verdict of
  Object value ->
    let Subsystem unit = subsystem report
        Scenario route = scenario report
     in Object (KM.insert "mode" (String "interface-stream")
       (KM.insert "subsystem" (String unit) (KM.insert "scenario" (String route) value)))
  value -> value
