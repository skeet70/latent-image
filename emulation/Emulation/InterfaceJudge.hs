{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The interface-stream relation: independent channels compared in order,
-- declared causal pairs, and the existing milestone relation.
--
-- A report without channel declarations is compared as one ordered stream,
-- exactly as before. A report that declares channels under @payload_projection@
-- groups its events by selector: the first declared channel whose @match@
-- field set every event field agrees with claims the event. The order between
-- channels is not compared.
--
-- Every declared channel's events are compared in order. A channel marked
-- @time_driven@ is compared occurrence by occurrence; with a milestone the
-- shorter sequence must be a prefix of the longer, because either side can
-- reach the milestone a little earlier. An ordered channel with a milestone
-- is compared through the milestone observation, and its stock events after
-- it must be a prefix of the owned events after it. A declared causal pair
-- holds when every event matching @then@ has an earlier event matching
-- @first@. A declared milestone both sides must carry as a
-- @{"milestone": NAME}@ event; a side that does not reach it is different.
module Emulation.InterfaceJudge
  ( InterfaceReport (..)
  , ReportDigest (..)
  , parseInterfaceReport
  , judgeInterfaceReport
  , interfaceVerdictJson
  ) where

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

-- | One declared channel: the events its selector claims, whether timing may
-- change its count, and where the declaration comes from.
data Channel = Channel
  { channelName :: ChannelName
  , channelMatch :: Selector
  , channelTimeDriven :: Bool
  , channelSources :: [T.Text]
  } deriving (Eq, Show)

-- | One declared ordering: every event matching @then@ needs an earlier event
-- matching @first@.
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

name :: String -> (T.Text -> a) -> Value -> J.Parser a
name label make = withText label $ \text -> do
  unless (not (T.null (T.strip text))) (fail (label ++ " must not be empty"))
  pure (make text)

instance FromJSON Subsystem where parseJSON = name "subsystem" Subsystem
instance FromJSON Scenario where parseJSON = name "scenario" Scenario
instance FromJSON Variant where parseJSON = name "variant" Variant
instance FromJSON Channel where
  parseJSON = withObject "channel" $ \value -> do
    label <- value .: "name"
    match <- value .: "match"
    driven <- value .:? "time_driven" .!= False
    sources <- value .: "sources"
    unless (not (null sources)) (fail "channel must cite a source")
    pure (Channel label match driven sources)
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

-- | The channel that claims an event; declaration order is precedence, so a
-- specific channel is declared before the general one it is a part of.
claim :: [Channel] -> Value -> Maybe Channel
claim declared value = case filter (\channel -> matchesSelector (channelMatch channel) value) declared of
  channel : _ -> Just channel
  [] -> Nothing

-- | Group a run's events by declared channel, keeping each event's position
-- in the run. An event no channel claims leaves the declaration unable to
-- judge the stream.
partition :: [Channel] -> StreamRun -> Either String (M.Map ChannelName [(Int, Value)])
partition declared run =
  foldM step M.empty (zip [0 :: Int ..] (map observationValue (runEvents run)))
  where
    step acc (index, value)
      | isMarker value = Right acc
      | otherwise = case claim declared value of
          Nothing -> Left ("event at observation " ++ show index ++ " matches no declared channel")
          Just channel -> Right (M.insertWith (\new old -> old ++ new) (channelName channel) [(index, value)] acc)

-- | The first differing event of one channel. With a milestone, an ordered
-- channel is equal through the milestone observation and its stock tail after
-- it must be a prefix of the owned tail; a time-driven channel is compared
-- occurrence by occurrence and the shorter sequence must be a prefix of the
-- longer.
compareSequence :: ChannelName -> Maybe Int -> Bool -> [(Int, Value)] -> [(Int, Value)] -> Maybe StreamDifference
compareSequence label boundary timeDriven stock owned
  | timeDriven = case boundary of
      Just _ -> walkEither 0 stock owned
      Nothing -> walkExact 0 stock owned
  | Just mark <- boundary =
      let (stockPrefix, stockTail) = span ((<= mark) . fst) stock
          (ownedPrefix, ownedTail) = span ((<= mark) . fst) owned
       in case walkExact 0 stockPrefix ownedPrefix of
            Just difference -> Just difference
            Nothing -> walkPrefix (length stockPrefix) stockTail ownedTail
  | otherwise = walkExact 0 stock owned
  where
    walkExact _ [] [] = Nothing
    walkExact occurrence (expected : _) []
      = Just (DifferingEvent label (Occurrence occurrence) (Just (snd expected)) Nothing)
    walkExact occurrence [] (actual : _)
      = Just (DifferingEvent label (Occurrence occurrence) Nothing (Just (snd actual)))
    walkExact occurrence (expected : expecteds) (actual : actuals)
      | snd expected == snd actual = walkExact (occurrence + 1) expecteds actuals
      | otherwise = Just (DifferingEvent label (Occurrence occurrence) (Just (snd expected)) (Just (snd actual)))
    walkPrefix _ [] _ = Nothing
    walkPrefix occurrence (expected : _) []
      = Just (DifferingEvent label (Occurrence occurrence) (Just (snd expected)) Nothing)
    walkPrefix occurrence (expected : expecteds) (actual : actuals)
      | snd expected == snd actual = walkPrefix (occurrence + 1) expecteds actuals
      | otherwise = Just (DifferingEvent label (Occurrence occurrence) (Just (snd expected)) (Just (snd actual)))
    walkEither _ [] _ = Nothing
    walkEither _ _ [] = Nothing
    walkEither occurrence (expected : expecteds) (actual : actuals)
      | snd expected == snd actual = walkEither (occurrence + 1) expecteds actuals
      | otherwise = Just (DifferingEvent label (Occurrence occurrence) (Just (snd expected)) (Just (snd actual)))

-- | The first event matching @then@ with no earlier event matching @first@.
causalViolation :: String -> Causal -> StreamRun -> Maybe CausalViolation
causalViolation side (Causal first then_ _) run =
  case [index | (index, value) <- indexed, matchesSelector then_ value, maybe True (>= index) earliest] of
    [] -> Nothing
    (index : _) -> Just (CausalViolation index first then_
      (side ++ "'s observation " ++ show index ++ " matches no earlier event for the declared pair"))
  where
    indexed = zip [0 :: Int ..] (map observationValue (runEvents run))
    earliest = fmap fst (find (matchesSelector first . snd) indexed)

-- | The first failure of the declared relation between a stock stream and a
-- candidate stream, or why the declaration cannot judge the streams.
compareChannels :: Maybe Int -> [Channel] -> [Causal] -> StreamRun -> String -> StreamRun
  -> Either String (Maybe StreamDifference)
compareChannels boundary channels_ pairs stock candidateLabel candidate = do
  stockTable <- partition channels_ stock
  candidateTable <- partition channels_ candidate
  let channelDifference channel =
        compareSequence (channelName channel) boundary (channelTimeDriven channel)
          (eventsOf stockTable channel) (eventsOf candidateTable channel)
      eventsOf table channel = M.findWithDefault [] (channelName channel) table
      pairDifference side run = firstJust [UncausedEvent <$> causalViolation side pair run | pair <- pairs]
  pure $ case firstJust (map channelDifference channels_) of
    Just difference -> Just difference
    Nothing -> case pairDifference "stock" stock of
      Just difference -> Just difference
      Nothing -> pairDifference candidateLabel candidate

firstJust :: [Maybe a] -> Maybe a
firstJust = foldr (\candidate rest -> maybe rest Just candidate) Nothing

reached :: Milestone -> StreamRun -> Bool
reached (Milestone label _) run =
  any (\observation -> markerName (observationValue observation) == Just (T.pack label)) (runEvents run)

judgeInterfaceReport :: Maybe Firmware -> ReportDigest -> InterfaceReport -> Verdict
judgeInterfaceReport firmware (ReportDigest reportDigest) report = base
  { verdictStatus = status
  , verdictDifferentCases = [scenarioName | status == VerdictDifferent]
  , verdictFirstDifference = if status == VerdictDifferent && not channelMode then wholeDifference else Nothing
  , verdictChannelDifference = channelFailure
  , verdictCausalViolation = causalFailure
  , verdictReasons = reasons
  }
  where
    Subsystem subsystemName = subsystem report
    Scenario scenarioText = scenario report
    scenarioName = T.unpack scenarioText
    stock = stockRun report
    owned = ownedRun report
    channelMode = not (null (channels report))
    boundary = case milestone report of
      Just (Milestone _ (ObservationIndex mark)) -> Just mark
      Nothing -> Nothing
    wholeDifference = case milestone report of
      Just marker -> milestoneDifference marker (runEvents stock) (runEvents owned)
      Nothing -> firstDifference (runEvents stock) (runEvents owned)
    comparedChannels
      | channelMode = compareChannels boundary (channels report) (causalPairs report) stock "owned" owned
      | otherwise = Right Nothing
    streamDifference = either (const Nothing) id comparedChannels
    channelReasons = either (: []) (const []) comparedChannels
    anyDifference = if channelMode then isJust streamDifference else isJust wholeDifference
    channelFailure = case streamDifference of
      Just (DifferingEvent label occurrence left right) ->
        Just (ChannelDifference label occurrence left right)
      _ -> Nothing
    causalFailure = case streamDifference of
      Just (UncausedEvent violation) -> Just violation
      _ -> Nothing
    missingMilestone = case milestone report of
      Just marker | channelMode ->
        [ side ++ " does not reach milestone " ++ milestoneName marker
        | (side, run) <- [("stock", stock), ("owned", owned)]
        , not (reached marker run)
        ]
      _ -> []
    controlEvidence = all (\(WrongRun _ run) -> executed (ownedExtents report) run) (wrongRuns report)
    detected (WrongRun _ run)
      | channelMode = either (const False) (not . isNothing) $
          compareChannels boundary (channels report) (causalPairs report) stock "control" run
      | otherwise = not (isNothing (case milestone report of
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
    reasons = channelReasons ++ missingMilestone
      ++ ["firmware not supplied; the comparison is not run" | isNothing firmware]
    status
      | isNothing firmware = VerdictNotRun
      | not (null unmet) = VerdictNoVerdict
      | not (null channelReasons) = VerdictNoVerdict
      | not (null missingMilestone) = VerdictDifferent
      | anyDifference = VerdictDifferent
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
      }

interfaceVerdictJson :: InterfaceReport -> Verdict -> Value
interfaceVerdictJson report verdict = case verdictJson verdict of
  Object value ->
    let Subsystem unit = subsystem report
        Scenario route = scenario report
     in Object (KM.insert "mode" (String "interface-stream")
       (KM.insert "subsystem" (String unit) (KM.insert "scenario" (String route) value)))
  value -> value
