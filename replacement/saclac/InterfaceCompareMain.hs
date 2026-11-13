{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Data.Aeson
import qualified Data.Aeson.Types as J
import qualified Data.ByteString.Lazy as L
import Data.List (find, findIndex)
import qualified Data.Text as Text
import Data.Word (Word8, Word16, Word32)
import qualified Emulation.Judge as Judge
import qualified FujiAF.Host as H
import System.Environment (getArgs)
import System.Exit (exitWith)
import Text.Read (readMaybe)

newtype Address = Address Word32 deriving (Eq, Show)
instance FromJSON Address where
  parseJSON = withText "guest address" $ \text ->
    maybe (fail "invalid guest address") (pure . Address) (readMaybe (Text.unpack text))
instance ToJSON Address where toJSON (Address address) = toJSON address

data Argument = WordArgument Word32 | MemoryArgument (Maybe Address) [Word8] deriving (Eq, Show)
instance FromJSON Argument where
  parseJSON = withObject "reply argument" $ \value -> do
    kind <- value .: "kind" :: J.Parser String
    case kind of
      "word" -> WordArgument <$> value .: "value"
      "memory" -> MemoryArgument <$> value .:? "address" <*> value .: "bytes"
      _ -> fail "unknown reply argument"
instance ToJSON Argument where
  toJSON argument = case argument of
    WordArgument value -> object ["kind" .= ("word" :: String), "value" .= value]
    MemoryArgument address bytes -> object ["kind" .= ("memory" :: String), "address" .= address, "bytes" .= bytes]

data Event
  = QueueIn Word16 Word8 [Word8]
  | PeerRequest Word8 Word16 [Word8]
  | Callback Address
  | ClientReply Address Address [Argument]
  | Store Address [Word8]
  deriving (Eq, Show)
instance FromJSON Event where
  parseJSON = withObject "interface event" $ \value -> do
    kind <- value .: "kind" :: J.Parser String
    case kind of
      "queue-in" -> QueueIn <$> value .: "operation" <*> value .: "sender" <*> value .: "payload"
      "peer-request" -> PeerRequest <$> value .: "queue" <*> value .: "operation" <*> value .: "payload"
      "callback" -> Callback <$> value .: "entry"
      "client-reply" -> ClientReply <$> value .: "entry" <*> value .: "target" <*> value .: "arguments"
      "store" -> Store <$> value .: "address" <*> value .: "bytes"
      _ -> fail "unknown interface event"
instance ToJSON Event where
  toJSON event = case event of
    QueueIn operation sender payload -> object ["kind" .= ("queue-in" :: String), "operation" .= operation, "sender" .= sender, "payload" .= payload]
    PeerRequest queue operation payload -> object ["kind" .= ("peer-request" :: String), "queue" .= queue, "operation" .= operation, "payload" .= payload]
    Callback entry -> object ["kind" .= ("callback" :: String), "entry" .= entry]
    ClientReply entry target arguments -> object ["kind" .= ("client-reply" :: String), "entry" .= entry, "target" .= target, "arguments" .= arguments]
    Store address bytes -> object ["kind" .= ("store" :: String), "address" .= address, "bytes" .= bytes]

data Observed = Observed { observedInstructions :: Integer, observedEvent :: Event }
instance FromJSON Observed where
  parseJSON = withObject "observed event" $ \value -> Observed <$> value .: "instructions" <*> value .: "event"

data ReachedMilestone = ReachedMilestone { reachedPhase :: String, reachedAt :: Maybe Integer }
instance FromJSON ReachedMilestone where
  parseJSON = withObject "milestone" $ \value -> ReachedMilestone <$> value .: "phase" <*> value .:? "reached"

data Receipt = Receipt
  { scenario :: String
  , events :: [Event]
  , timedEvents :: [Observed]
  , milestones :: [ReachedMilestone]
  , stockFetches :: Integer
  , observationError :: Maybe String
  , machineFailure :: Maybe String
  }
instance FromJSON Receipt where
  parseJSON = withObject "census receipt" $ \value -> do
    name <- value .: "scenario"
    rows <- (withObject "interface" (.: "events") =<< value .: "interface") :: J.Parser [Observed]
    phases <- value .: "milestones"
    ranges <- value .: "action_ranges" :: J.Parser [Value]
    fetches <- traverse (withObject "action range" (.: "fetches")) ranges
    Receipt name (concatMap (normalize . observedEvent) rows) rows phases (sum fetches)
      <$> value .:? "error" <*> value .:? "machine_failure"

normalize :: Event -> [Event]
normalize (Store (Address address) bytes) = zipWith (\offset byte -> Store (Address (address + offset)) [byte]) [0..] bytes
normalize event = [event]

load :: FilePath -> IO Receipt
load path = do
  bytes <- H.readRegular (16 * 1024 * 1024) path
  either H.failH pure (eitherDecodeStrict bytes)

healthy :: Receipt -> Bool
healthy receipt = observationError receipt == Nothing && machineFailure receipt == Nothing
  && not (null (events receipt)) && stockFetches receipt > 0

firstDifference :: [Event] -> [Event] -> Maybe Int
firstDifference left right = case findIndex not (zipWith (==) left right) of
  Just index -> Just index
  Nothing | length left /= length right -> Just (min (length left) (length right))
  Nothing -> Nothing

milestoneResult :: String -> Receipt -> Receipt -> Maybe (Maybe Judge.FirstDifference)
milestoneResult phase stock candidate = do
  stockTick <- reachedAt =<< find ((== phase) . reachedPhase) (milestones stock)
  candidateTick <- reachedAt =<< find ((== phase) . reachedPhase) (milestones candidate)
  let project tick receipt =
        let (before, after) = span ((<= tick) . observedInstructions) (timedEvents receipt)
            left = concatMap (normalize . observedEvent) before
            right = concatMap (normalize . observedEvent) after
            marker = Judge.Observation (object ["milestone" .= phase])
         in (Judge.ObservationIndex (length left), map (Judge.Observation . toJSON) left ++ [marker] ++ map (Judge.Observation . toJSON) right)
      (mark, stockObservations) = project stockTick stock
      (_, candidateObservations) = project candidateTick candidate
  pure (Judge.milestoneDifference (Judge.Milestone phase mark) stockObservations candidateObservations)

statusLabel :: Judge.VerdictStatus -> String
statusLabel status = case status of
  Judge.VerdictEqual -> "equal"
  Judge.VerdictEqualUpToMilestone -> "equal-up-to-milestone"
  Judge.VerdictDifferent -> "different"
  Judge.VerdictNoVerdict -> "no-verdict"
  Judge.VerdictNotRun -> "not-run"

main :: IO ()
main = do
  arguments <- getArgs
  (stockPath, candidatePath, controlPath, output, phase) <- case arguments of
    [stock, candidate, control, output] -> pure (stock, candidate, control, output, Nothing)
    [stock, candidate, control, output, "--milestone", name] -> pure (stock, candidate, control, output, Just name)
    _ -> H.failH "usage: saclac-interface-compare STOCK_JSON CANDIDATE_JSON WRONG_JSON OUTPUT_JSON [--milestone PHASE]"
  stock <- load stockPath
  candidate <- load candidatePath
  control <- load controlPath
  let difference = firstDifference (events stock) (events candidate)
      controlDifference = firstDifference (events stock) (events control)
      sameScenario = scenario stock == scenario candidate && scenario stock == scenario control
      complete = sameScenario && all healthy [stock, candidate, control] && controlDifference /= Nothing
      milestone = phase >>= \name -> milestoneResult name stock candidate
      status
        | not complete = Judge.VerdictNoVerdict
        | Just _ <- phase = case milestone of
            Just Nothing -> Judge.VerdictEqualUpToMilestone
            _ -> Judge.VerdictDifferent
        | difference == Nothing = Judge.VerdictEqual
        | otherwise = Judge.VerdictDifferent
      milestoneDifferenceIndex = case milestone of
        Just (Just first) -> Just (Judge.observationIndex (Judge.differenceAt first))
        _ -> Nothing
      at receipt index = take 1 (drop index (events receipt))
  L.writeFile output (encode (object
    [ "schema" .= (1 :: Int)
    , "verdict" .= statusLabel status
    , "scenario" .= scenario stock
    , "same_scenario" .= sameScenario
    , "stock_executed_instructions_in_action_extents" .= stockFetches stock
    , "stock_events" .= length (events stock)
    , "candidate_events" .= length (events candidate)
    , "control_events" .= length (events control)
    , "complete" .= complete
    , "first_difference" .= difference
    , "stock_at_difference" .= maybe [] (at stock) difference
    , "candidate_at_difference" .= maybe [] (at candidate) difference
    , "wrong_variant_detected" .= (controlDifference /= Nothing)
    , "control_first_difference" .= controlDifference
    , "milestone" .= phase
    , "milestone_first_difference" .= milestoneDifferenceIndex
    , "judge_version" .= Judge.judgeVersion
    , "normalization" .= ("instruction clocks and cores omitted; each store expanded into ascending-address bytes without reordering events; transient reply pointer addresses omitted and contents retained" :: String)
    ]))
  exitWith (Judge.verdictExitCode status)
