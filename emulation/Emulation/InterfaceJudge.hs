{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The interface-stream relation: independent channels compared in order,
-- counted causal pairs, and the end-milestone tail rule.
--
-- A report that declares channels under @payload_projection@ groups its
-- events by selector: the first declared channel whose @match@ field set
-- every event field agrees with claims the event. A report without channel
-- declarations has one ordered channel, @stream@, holding every event. The
-- order between channels is not compared, and a @{"milestone": NAME}@ marker
-- is not a channel event.
--
-- Without a milestone every channel must be equal. A declared milestone ends
-- the compared stream: each side is cut at its own marker and carries no
-- event after it, so a start marker cannot be declared as one. At the
-- milestone the shorter sequence of a channel must be a prefix of the longer,
-- and the longer may carry at most the channel's tail allowance more events:
-- the declared one, else 1 for a time-driven channel and 0 for an ordered
-- one. A side that does not reach the milestone is different. A declared
-- causal pair is counted: each event matching @then@ consumes one earlier
-- unconsumed event matching @first@.
module Emulation.InterfaceJudge
  ( InterfaceReport (..)
  , ReportDigest (..)
  , parseInterfaceReport
  , judgeInterfaceReport
  , interfaceVerdictJson
  ) where

import Control.Applicative ((<|>))
import Control.Monad (foldM, unless)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', withObject, withText, (.:), (.:?), (.!=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (find, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.InterfaceStream
import Emulation.Judge

newtype Subsystem = Subsystem T.Text deriving (Eq, Show)
newtype Scenario = Scenario T.Text deriving (Eq, Show)
newtype Variant = Variant T.Text deriving (Eq, Show)
newtype Digest = Digest T.Text deriving (Eq, Show)
newtype ReportDigest = ReportDigest String deriving (Eq, Show)
newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
newtype FetchCount = FetchCount Word64 deriving (Eq, Show)

-- | A declared bound on one channel's tail at the end milestone and the
-- report's reason for it.
data TailAllowance = TailAllowance
  { allowanceOccurrences :: Int
  , allowanceReason :: T.Text
  } deriving (Eq, Show)

-- | One declared channel: the events its selector claims, whether timing may
-- change its count, its declared tail allowance, and where the declaration
-- comes from.
data Channel = Channel
  { channelName :: ChannelName
  , channelMatch :: Selector
  , channelTimeDriven :: Bool
  , channelTailAllowance :: Maybe TailAllowance
  , channelSources :: [T.Text]
  } deriving (Eq, Show)

-- | One declared ordering: every event matching @then@ needs its own earlier
-- event matching @first@.
data Causal = Causal
  { causalFirst :: Selector
  , causalThen :: Selector
  , causalSources :: [T.Text]
  } deriving (Eq, Show)

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
  , channels :: [Channel]
  , causalPairs :: [Causal]
  } deriving (Eq, Show)

-- | The first failure of the channel relation: a differing event on the first
-- differing channel, or an event a declared causal pair does not explain.
data StreamDifference
  = DifferingEvent ChannelName Occurrence (Maybe Value) (Maybe Value)
  | UncausedEvent CausalViolation
  deriving (Eq, Show)

-- | A candidate run judged against the stock run: the first failure of the
-- relation, and the tolerance each channel was judged under.
data Comparison = Comparison
  { comparisonDifference :: Maybe StreamDifference
  , comparisonTolerances :: [ChannelTolerance]
  }

name :: String -> (T.Text -> a) -> Value -> J.Parser a
name label make = withText label $ \text -> do
  unless (not (T.null (T.strip text))) (fail (label ++ " must not be empty"))
  pure (make text)

instance FromJSON Subsystem where parseJSON = name "subsystem" Subsystem
instance FromJSON Scenario where parseJSON = name "scenario" Scenario
instance FromJSON Variant where parseJSON = name "variant" Variant
instance FromJSON TailAllowance where
  parseJSON = withObject "tail allowance" $ \value -> do
    occurrences <- value .: "occurrences"
    reason <- value .: "reason"
    unless (occurrences >= 0) (fail "tail allowance must not be negative")
    unless (not (T.null (T.strip reason))) (fail "tail allowance must give a reason")
    pure (TailAllowance occurrences reason)
instance FromJSON Channel where
  parseJSON = withObject "channel" $ \value -> do
    label <- value .: "name"
    match <- value .: "match"
    driven <- value .:? "time_driven" .!= False
    allowance <- value .:? "tail_allowance"
    sources <- value .: "sources"
    unless (not (null sources)) (fail "channel must cite a source")
    pure (Channel label match driven allowance sources)
instance FromJSON Causal where
  parseJSON = withObject "causal pair" $ \value -> do
    first <- value .: "first"
    then_ <- value .: "then"
    sources <- value .: "sources"
    unless (not (null sources)) (fail "causal pair must cite a source")
    pure (Causal first then_ sources)
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
    projection <- value .:? "payload_projection"
    (declaredChannels, declaredCausal) <- case projection of
      Just (Object row) -> (,) <$> (row .:? "channels" .!= []) <*> (row .:? "causal_pairs" .!= [])
      _ -> pure ([], [])
    InterfaceReport <$> value .: "subsystem" <*> value .: "scenario"
      <*> value .:? "retired_extents" .!= [] <*> value .:? "owned_extents" .!= []
      <*> value .: "stock" <*> value .: "owned" <*> value .:? "controls" .!= [] <*> pure mark
      <*> pure declaredChannels <*> pure declaredCausal

parseInterfaceReport :: B.ByteString -> Either String InterfaceReport
parseInterfaceReport = eitherDecodeStrict'

executed :: [Span] -> StreamRun -> Bool
executed extents run = runComplete run && any positive (runFetches run)
  && all admitted (runFetches run)
  where
    positive (FetchReceipt _ (FetchCount count)) = count > 0
    admitted (FetchReceipt (Span start end) _) = any (\(Span low high) -> low <= start && end <= high) extents

digestText :: Digest -> String
digestText (Digest text) = T.unpack text

-- | The milestone marker a producer inserts: an object whose only field is
-- @milestone@.
markerName :: Value -> Maybe T.Text
markerName (Object row) = case KM.lookup "milestone" row of
  Just (String label) -> Just label
  _ -> Nothing
markerName _ = Nothing

isMarker :: Value -> Bool
isMarker (Object row) = KM.keys row == [K.fromText "milestone"]
isMarker _ = False

isMilestoneMarker :: Milestone -> Value -> Bool
isMilestoneMarker (Milestone label _) value = isMarker value && markerName value == Just (T.pack label)

reached :: Milestone -> StreamRun -> Bool
reached marker run = any (isMilestoneMarker marker . observationValue) (runEvents run)

-- | The one channel of a report that declares none.
wholeStream :: Channel
wholeStream = Channel (ChannelName "stream") (Selector KM.empty) False Nothing []

judgedChannels :: [Channel] -> [Channel]
judgedChannels [] = [wholeStream]
judgedChannels declared = declared

-- | The channel that claims an event; declaration order is precedence, so a
-- specific channel is declared before the general one it is a part of.
claim :: [Channel] -> Value -> Maybe Channel
claim [] _ = Just wholeStream
claim declared value = find (\channel -> matchesSelector (channelMatch channel) value) declared

-- | One side's events grouped by channel, in order. An event no channel
-- claims, or one after the side's milestone marker, leaves the declaration
-- unable to judge the stream.
partition :: String -> Maybe Milestone -> [Channel] -> StreamRun -> Either String (M.Map ChannelName [Value])
partition side end declared run =
  M.map reverse . fst <$> foldM step (M.empty, Nothing) (zip [0 :: Int ..] (map observationValue (runEvents run)))
  where
    step (table, ended) (index, value)
      | isMarker value = Right (table, ended <|> (if any (`isMilestoneMarker` value) end then Just index else Nothing))
      | Just mark <- ended = Left (side ++ "'s observation " ++ show index ++ " follows its milestone marker at observation "
          ++ show mark ++ "; a milestone ends the compared stream")
      | otherwise = case claim declared value of
          Nothing -> Left (side ++ "'s observation " ++ show index ++ " matches no declared channel")
          Just channel -> Right (M.insertWith (++) (channelName channel) [value] table, ended)

-- | The number of events one side of a channel may carry beyond the other at
-- the report's milestone, and where that number comes from.
allowanceOf :: Maybe Milestone -> Channel -> (Int, AllowanceBasis)
allowanceOf Nothing _ = (0, NoMilestone)
allowanceOf (Just _) channel = case channelTailAllowance channel of
  Just (TailAllowance occurrences reason) -> (occurrences, DeclaredAllowance reason)
  Nothing
    | channelTimeDriven channel -> (1, TimeDrivenDefault)
    | otherwise -> (0, OrderedDefault)

-- | The first event of one channel the allowance does not admit: the shorter
-- sequence must be a prefix of the longer, which may carry at most
-- @allowance@ more events. Past the shorter sequence, the event named is the
-- first beyond the allowance.
tailDifference :: ChannelName -> Int -> [Value] -> [Value] -> Maybe StreamDifference
tailDifference label allowance = go 0
  where
    go occurrence (expected : expecteds) (actual : actuals)
      | expected == actual = go (occurrence + 1) expecteds actuals
      | otherwise = Just (DifferingEvent label (Occurrence occurrence) (Just expected) (Just actual))
    go occurrence expecteds actuals = case (drop allowance expecteds, drop allowance actuals) of
      (extra : _, _) -> Just (DifferingEvent label (Occurrence (occurrence + allowance)) (Just extra) Nothing)
      (_, extra : _) -> Just (DifferingEvent label (Occurrence (occurrence + allowance)) Nothing (Just extra))
      ([], []) -> Nothing

-- | The first event matching @then@ without an earlier, not yet consumed
-- event matching @first@. An event matching both selectors needs an earlier
-- @first@ event before it adds one.
causalViolation :: String -> Causal -> StreamRun -> Maybe CausalViolation
causalViolation side (Causal first then_ _) run =
  go (0 :: Int) (zip [0 :: Int ..] (map observationValue (runEvents run)))
  where
    go _ [] = Nothing
    go unmatched ((index, value) : rest)
      | effect && unmatched == 0 = Just (CausalViolation index first then_
          (side ++ "'s observation " ++ show index ++ " matches no earlier unmatched event for the declared pair"))
      | otherwise = go (unmatched - fromEnum effect + fromEnum (matchesSelector first value)) rest
      where effect = matchesSelector then_ value

-- | Judge a candidate run against the stock run under the report's
-- declarations, or say why the declarations cannot judge them.
compareRuns :: InterfaceReport -> String -> StreamRun -> Either String Comparison
compareRuns report candidateLabel candidate = do
  stockTable <- partition "stock" end declared stock
  candidateTable <- partition candidateLabel end declared candidate
  let judged channel =
        let (allowance, basis) = allowanceOf end channel
            stockEvents = M.findWithDefault [] (channelName channel) stockTable
            candidateEvents = M.findWithDefault [] (channelName channel) candidateTable
         in ( tailDifference (channelName channel) allowance stockEvents candidateEvents
            , ChannelTolerance (channelName channel) (channelTimeDriven channel) allowance basis
                (length stockEvents) (length candidateEvents) )
      results = map judged (judgedChannels declared)
      pairDifference side run = firstJust [UncausedEvent <$> causalViolation side pair run | pair <- causalPairs report]
  pure Comparison
    { comparisonDifference = firstJust (map fst results)
        <|> pairDifference "stock" stock <|> pairDifference candidateLabel candidate
    , comparisonTolerances = map snd results
    }
  where
    end = milestone report
    declared = channels report
    stock = stockRun report

firstJust :: [Maybe a] -> Maybe a
firstJust = foldr (\candidate rest -> maybe rest Just candidate) Nothing

judgeInterfaceReport :: Maybe Firmware -> ReportDigest -> InterfaceReport -> Verdict
judgeInterfaceReport firmware (ReportDigest reportDigest) report = base
  { verdictStatus = status
  , verdictDifferentCases = [scenarioName | status == VerdictDifferent]
  , verdictChannelDifference = channelFailure
  , verdictCausalViolation = causalFailure
  , verdictChannelTolerances = either (const []) comparisonTolerances compared
  , verdictReasons = reasons
  }
  where
    Subsystem subsystemName = subsystem report
    Scenario scenarioText = scenario report
    scenarioName = T.unpack scenarioText
    stock = stockRun report
    owned = ownedRun report
    compared = compareRuns report "owned" owned
    streamDifference = either (const Nothing) comparisonDifference compared
    channelReasons = either (: []) (const []) compared
    channelFailure = case streamDifference of
      Just (DifferingEvent label occurrence left right) ->
        Just (ChannelDifference label occurrence left right)
      _ -> Nothing
    causalFailure = case streamDifference of
      Just (UncausedEvent violation) -> Just violation
      _ -> Nothing
    missingMilestone = case milestone report of
      Just marker ->
        [ side ++ " does not reach milestone " ++ milestoneName marker
        | (side, run) <- [("stock", stock), ("owned", owned)]
        , not (reached marker run)
        ]
      Nothing -> []
    controlEvidence = all (\(WrongRun _ run) -> executed (ownedExtents report) run) (wrongRuns report)
    detected (WrongRun _ run) =
      not (all (`reached` run) (milestone report))
        || either (const False) (isJust . comparisonDifference) (compareRuns report "control" run)
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
    reasons = channelReasons ++ missingMilestone
      ++ ["firmware not supplied; the comparison is not run" | isNothing firmware]
    status
      | isNothing firmware = VerdictNotRun
      | not (null unmet) = VerdictNoVerdict
      | not (null channelReasons) = VerdictNoVerdict
      | not (null missingMilestone) = VerdictDifferent
      | isJust streamDifference = VerdictDifferent
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
      , verdictReasons = []
      , verdictMilestone = milestone report
      , verdictFirstDifference = Nothing
      , verdictChannelDifference = Nothing
      , verdictCausalViolation = Nothing
      , verdictChannelTolerances = []
      }

interfaceVerdictJson :: InterfaceReport -> Verdict -> Value
interfaceVerdictJson report verdict = case verdictJson verdict of
  Object value ->
    let Subsystem unit = subsystem report
        Scenario route = scenario report
     in Object (KM.insert "mode" (String "interface-stream")
       (KM.insert "subsystem" (String unit) (KM.insert "scenario" (String route) value)))
  value -> value
