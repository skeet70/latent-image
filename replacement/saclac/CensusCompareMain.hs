{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Compares two saclac census reports: the first difference in scenario,
-- per-machine dispatcher sequence, queue-43 ops, milestones, then trap fetches.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as T
import qualified Data.ByteString.Lazy as L
import Data.List (find, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Text as Text
import Data.Maybe (fromMaybe, isJust, listToMaybe)
import FujiAF.Host (failH)
import System.Environment (getArgs)

data Event = Event
  { eventMachine :: Int
  , eventState :: Int
  , eventRow :: Int
  , eventAction :: Integer
  , eventInstructions :: Integer
  } deriving (Eq, Show)

instance FromJSON Event where
  parseJSON = withObject "event" $ \value -> Event
    <$> value .: "machine" <*> value .: "state" <*> value .: "row"
    <*> (value .: "action" >>= hexInteger) <*> value .: "instructions"

data Milestone = Milestone
  { milestonePhase :: String
  , milestoneExpected :: [Integer]
  , milestoneObserved :: Maybe [Integer]
  , milestoneReached :: Maybe Integer
  , milestoneFirst :: Maybe Integer
  , milestoneFinished :: Integer
  } deriving (Eq, Show)

instance FromJSON Milestone where
  parseJSON = withObject "milestone" $ \value -> do
    phase <- value .: "phase"
    expected <- value .: "expected" >>= triple
    observed <- (value .:? "observed" >>= traverse triple)
    reached <- (value .:? "reached" :: T.Parser (Maybe Integer))
    first <- value .:? "first"
    finished <- value .: "finished"
    pure (Milestone phase expected observed reached first finished)

triple :: Value -> T.Parser [Integer]
triple = withObject "menu" $ \value -> sequence [value .: "menu", value .: "setting", value .: "cursor"]

hexInteger :: Value -> T.Parser Integer
hexInteger = \case
  String text -> case reads (Text.unpack text) :: [(Integer, String)] of
    [(value, "")] -> pure value
    _ -> fail "address is not hexadecimal"
  Number number -> pure (round number)
  _ -> fail "address is neither a string nor a number"

countKey :: Value -> Integer
countKey = fromMaybe 0 . T.parseMaybe (withObject "counted" (\value -> value .: "key" >>= hexInteger))

data Queue = Queue { queuePosts :: Integer, queueOps :: [Integer] } deriving (Eq, Show)

instance FromJSON Queue where
  parseJSON = withObject "queue43" $ \value -> Queue
    <$> value .: "posts"
    <*> (map countKey <$> value .: "ops")

data Watch = Watch { watchLabel :: String, watchFetches :: Integer, watchAddresses :: [Integer] } deriving (Eq, Show)

instance FromJSON Watch where
  parseJSON = withObject "watch" $ \value -> Watch
    <$> value .: "label"
    <*> value .: "fetches"
    <*> (map countKey <$> value .: "addresses")

data Census = Census
  { censusScenario :: String
  , censusObservationStart :: Integer
  , censusInstructions :: Integer
  , censusEntries :: Integer
  , censusDispatch :: [Event]
  , censusQueue :: Queue
  , censusMilestones :: [Milestone]
  , censusWatch :: [Watch]
  } deriving (Eq, Show)

instance FromJSON Census where
  parseJSON = withObject "census" $ \value -> do
    scenario <- value .: "scenario"
    observationStart <- value .:? "observation_start_instructions" .!= 0
    instructions <- value .: "instructions"
    entries <- withObject "dispatcher" (.: "entries") =<< value .: "dispatcher"
    dispatch <- (value .:? "dispatch" :: T.Parser (Maybe Value)) >>= \case
      Nothing -> pure []
      Just trace -> withObject "dispatch" (.: "events") trace
    queue <- value .: "queue43"
    milestones <- value .: "milestones"
    watch <- fromMaybe [] <$> (value .:? "watch" :: T.Parser (Maybe [Watch]))
    pure (Census scenario observationStart instructions entries dispatch queue milestones watch)

data Contract = Contract { contractOwned :: [(Integer, Integer)] } deriving (Eq, Show)

instance FromJSON Contract where
  parseJSON = withObject "contract" $ \value -> do
    extents <- value .: "extents" :: T.Parser [Value]
    spans <- mapM (withObject "extent" (\extent -> extent .: "owned")) extents
    Contract . concat <$> mapM (mapM spanOf) spans
    where
      spanOf = withObject "span" (\span' -> (,) <$> span' .: "low" <*> span' .: "high")

machinesOf :: [Event] -> [Event] -> [Int]
machinesOf left right = sortOn id (M.keys (M.fromList [(eventMachine event, ()) | event <- left ++ right]))

tupleOf :: Event -> (Int, Int, Integer)
tupleOf event = (eventState event, eventRow event, eventAction event)

-- | The ordered (state, row, action) projection: instruction counts differ between
-- sides by construction and are not part of the sequence.
keysOf :: [Event] -> [(Int, Int, Integer)]
keysOf = map tupleOf

commonPrefix :: [Event] -> [Event] -> Int
commonPrefix left right = length (takeWhile id (zipWith (==) (keysOf left) (keysOf right)))

machineSequence :: [Event] -> [Event] -> Int -> Value
machineSequence stock candidate machine =
  let left = [event | event <- stock, eventMachine event == machine]
      right = [event | event <- candidate, eventMachine event == machine]
      common = commonPrefix left right
   in object
        [ "machine" .= machine
        , "stock" .= length left
        , "candidate" .= length right
        , "stock_prefix_of_candidate" .= (keysOf left == keysOf (take (length left) right))
        , "first_difference" .= (if common == min (length left) (length right) then Nothing else Just common)
        , "stock_extra" .= map tupleOf (drop common left)
        , "candidate_extra" .= map tupleOf (drop common right)
        ]

-- | Every machine whose stock sequence is not a prefix of the candidate's.
compareSequences :: [Event] -> [Event] -> [Value]
compareSequences stock candidate =
  [ object
      [ "observation" .= ("dispatcher-sequence" :: String)
      , "machine" .= machine
      , "index" .= common
      , "stock" .= map tupleOf (drop common left)
      , "candidate" .= map tupleOf (drop common right)
      ]
  | machine <- machinesOf stock candidate
  , let left = [event | event <- stock, eventMachine event == machine]
  , let right = [event | event <- candidate, eventMachine event == machine]
  , let common = commonPrefix left right
  , keysOf left /= keysOf (take (length left) right)
  ]

-- | Every machine whose candidate sequence continues past the stock one.
trailingMachines :: [Event] -> [Event] -> [Int]
trailingMachines stock candidate =
  [ machine
  | machine <- machinesOf stock candidate
  , let left = [event | event <- stock, eventMachine event == machine]
  , let right = [event | event <- candidate, eventMachine event == machine]
  , length left < length right
  , keysOf left == keysOf (take (length left) right)
  ]

compareMilestones :: [Milestone] -> [Milestone] -> Maybe Value
compareMilestones left right = go 0 left right
  where
    go :: Int -> [Milestone] -> [Milestone] -> Maybe Value
    go _ [] [] = Nothing
    go index (a : as) (b : bs) | a == b = go (index + 1) as bs
    go index as bs = Just (object
      [ "observation" .= ("milestone" :: String)
      , "index" .= index
      , "stock" .= toJSON (map milestoneTuple (take 1 as))
      , "candidate" .= toJSON (map milestoneTuple (take 1 bs))
      ])
    milestoneTuple (Milestone phase expected observed reached _ finished) = (phase, expected, observed, isJust reached, finished)

-- | One machine's ordered (state, row, action) sequence up to a milestone.
data MachineSequence = MachineSequence
  { sequenceMachine :: Int
  , sequenceStock :: [Event]
  , sequenceCandidate :: [Event]
  }

sequenceEqual :: MachineSequence -> Bool
sequenceEqual row = keysOf (sequenceStock row) == keysOf (sequenceCandidate row)

sequenceValue :: MachineSequence -> Value
sequenceValue row = object
  [ "machine" .= sequenceMachine row
  , "stock" .= length (sequenceStock row)
  , "candidate" .= length (sequenceCandidate row)
  , "equal" .= sequenceEqual row
  , "first_difference" .= difference
  , "stock_at_difference" .= fmap tupleOf (listToMaybe (drop differenceIndex (sequenceStock row)))
  , "candidate_at_difference" .= fmap tupleOf (listToMaybe (drop differenceIndex (sequenceCandidate row)))
  ]
  where
    differenceIndex = commonPrefix (sequenceStock row) (sequenceCandidate row)
    difference = if sequenceEqual row then Nothing else Just differenceIndex

milestoneCut :: Milestone -> Integer
milestoneCut milestone = fromMaybe (milestoneFinished milestone) (milestoneFirst milestone)

milestonePoint :: Milestone -> Value
milestonePoint milestone = object
  [ "first" .= milestoneFirst milestone
  , "reached" .= milestoneReached milestone
  , "finished" .= milestoneFinished milestone
  ]

-- | Per machine, the ordered (state, row, action) sequences each side recorded up to its
-- own first sighting of the named milestone.
milestoneComparison :: String -> [Event] -> [Event] -> [Milestone] -> [Milestone] -> Maybe Value
milestoneComparison phase stock candidate stockMilestones candidateMilestones = do
  stockMilestone <- find ((== phase) . milestonePhase) stockMilestones
  candidateMilestone <- find ((== phase) . milestonePhase) candidateMilestones
  let stockCut = milestoneCut stockMilestone
      candidateCut = milestoneCut candidateMilestone
      sequenceOf machine = MachineSequence machine
        [event | event <- stock, eventMachine event == machine, eventInstructions event <= stockCut]
        [event | event <- candidate, eventMachine event == machine, eventInstructions event <= candidateCut]
      sequences = map sequenceOf (machinesOf stock candidate)
      different = filter (not . sequenceEqual) sequences
  pure (object
    [ "phase" .= phase
    , "stock" .= milestonePoint stockMilestone
    , "candidate" .= milestonePoint candidateMilestone
    , "machines" .= map sequenceValue sequences
    , "equal" .= null different
    , "first_difference" .= fmap sequenceValue (listToMaybe different)
    ])

compareOps :: Queue -> Queue -> Maybe Value
compareOps left right =
  case find (`notElem` queueOps right) (queueOps left) of
    Just op -> Just (object ["observation" .= ("queue-ops" :: String), "op" .= op, "missing" .= ("candidate" :: String)])
    Nothing -> case find (`notElem` queueOps left) (queueOps right) of
      Just op -> Just (object ["observation" .= ("queue-ops" :: String), "op" .= op, "missing" .= ("stock" :: String)])
      Nothing -> Nothing

fillFetches :: Contract -> [Watch] -> [Integer]
fillFetches contract watches =
  [address | watch <- watches, address <- watchAddresses watch, not (insideOwned address)]
  where
    insideOwned address = any (\(low, high) -> low <= address && address < high) (contractOwned contract)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [stockPath, candidatePath, contractPath, outputPath, phase] ->
      run stockPath candidatePath contractPath outputPath phase
    [stockPath, candidatePath, contractPath, outputPath] ->
      run stockPath candidatePath contractPath outputPath "shooting-menu"
    _ -> failH "usage: saclac-census-compare STOCK_JSON CANDIDATE_JSON CONTRACT_JSON OUTPUT_JSON [MILESTONE_PHASE]"

run :: FilePath -> FilePath -> FilePath -> FilePath -> String -> IO ()
run stockPath candidatePath contractPath outputPath phase = do
      stock <- either failH pure . eitherDecode =<< L.readFile stockPath
      candidate <- either failH pure . eitherDecode =<< L.readFile candidatePath
      contract <- either failH pure . eitherDecode =<< L.readFile contractPath
      unless (censusScenario stock == censusScenario candidate) $
        failH "the two census reports are for different scenarios"
      unless (censusObservationStart stock == censusObservationStart candidate) $
        failH "the two census reports have different observation_start_instructions"
      let fills = fillFetches contract (censusWatch candidate)
          sequenceDifferences = compareSequences (censusDispatch stock) (censusDispatch candidate)
          trailing = trailingMachines (censusDispatch stock) (censusDispatch candidate)
          opsDifference = compareOps (censusQueue stock) (censusQueue candidate)
          milestoneDifference = compareMilestones (censusMilestones stock) (censusMilestones candidate)
          stockTrace = fromIntegral (length (censusDispatch stock))
          candidateTrace = fromIntegral (length (censusDispatch candidate))
          machine0Entries = fromIntegral (length [event | event <- censusDispatch candidate, eventMachine event == 0])
          trailingDifference = case trailing of
            (machine : _) -> Just (object
              [ "observation" .= ("trailing-dispatch" :: String)
              , "machine" .= machine
              , "extra" .= map tupleOf (drop (length [event | event <- censusDispatch stock, eventMachine event == machine])
                                          [event | event <- censusDispatch candidate, eventMachine event == machine])
              ])
            [] -> Nothing
          difference' = case sequenceDifferences of
            (value : _) -> Just value
            [] -> case trailingDifference of
              Just value -> Just value
              Nothing -> case opsDifference of
                Just value -> Just value
                Nothing -> case milestoneDifference of
                  Just value -> Just value
                  Nothing -> case fills of
                    (address : _) -> Just (object ["observation" .= ("trap-fetch" :: String), "address" .= address])
                    [] -> Nothing
          countsDiffer = censusEntries stock /= censusEntries candidate
            || censusInstructions stock /= censusInstructions candidate
            || queuePosts (censusQueue stock) /= queuePosts (censusQueue candidate)
          verdict = object
            [ "schema" .= (1 :: Int)
            , "scenario" .= censusScenario candidate
            , "verdict" .= (if difference' == Nothing then "equal" else "different" :: String)
            , "difference" .= difference'
            , "sequence_differences" .= sequenceDifferences
            , "dispatcher_entries" .= object
                [ "stock" .= censusEntries stock
                , "candidate" .= censusEntries candidate
                , "difference" .= (censusEntries stock - censusEntries candidate)
                , "stock_trace_entries" .= stockTrace
                , "candidate_trace_entries" .= candidateTrace
                , "machine0_trace_entries" .= machine0Entries
                , "stock_entries_equal_trace" .= (censusEntries stock == stockTrace)
                , "candidate_entries_plus_machine0_equal_trace" .=
                    (censusEntries candidate + machine0Entries == candidateTrace)
                , "explained_by_router_body_entry" .=
                    ( censusEntries stock == stockTrace
                        && censusEntries candidate + machine0Entries == candidateTrace
                    )
                ]
            , "instructions" .= object ["stock" .= censusInstructions stock, "candidate" .= censusInstructions candidate]
            , "queue43_posts" .= object ["stock" .= queuePosts (censusQueue stock), "candidate" .= queuePosts (censusQueue candidate)]
            , "counts_differ_while_sequences_agree" .= (null sequenceDifferences && countsDiffer)
            , "trailing_machines" .= trailing
            , "machines" .= [machineSequence (censusDispatch stock) (censusDispatch candidate) machine
                            | machine <- machinesOf (censusDispatch stock) (censusDispatch candidate)]
            , "trap_fetches" .= length fills
            , "watch" .= [object
                [ "label" .= watchLabel watch
                , "fetches" .= watchFetches watch
                , "addresses" .= length (watchAddresses watch)
                , "fill" .= length [address | address <- watchAddresses watch, not (any (\(low, high) -> low <= address && address < high) (contractOwned contract))]
                ] | watch <- censusWatch candidate]
            ]
      let milestone = milestoneComparison phase (censusDispatch stock) (censusDispatch candidate)
            (censusMilestones stock) (censusMilestones candidate)
          document = case milestone of
            Nothing -> verdict
            Just value -> case verdict of
              Object fields -> Object (KM.insert (K.fromString "milestone") value fields)
              other -> other
          milestoneText = maybe "" (\value -> "; milestone " <> phase <> ": "
            <> (if milestoneEqual value then "equal" else "different")) milestone
      L.writeFile outputPath (encode document)
      putStrLn ("saclac-census-compare: " <> censusScenario candidate <> ": "
        <> (if difference' == Nothing then "equal" else "different") <> milestoneText)
  where
    milestoneEqual value = case value of
      Object fields -> fromMaybe False (KM.lookup (K.fromString "equal") fields >>= asBool)
      _ -> False
    asBool (Bool value) = Just value
    asBool _ = Nothing
