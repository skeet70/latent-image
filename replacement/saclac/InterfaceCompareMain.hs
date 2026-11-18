{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Data.Aeson
import qualified Data.Aeson.Types as J
import qualified Data.ByteString.Lazy as L
import Data.Char (isHexDigit)
import Data.List (find)
import qualified Data.Text as Text
import Data.Word (Word8, Word16, Word32)
import qualified FujiAF.Host as H
import System.Environment (getArgs)
import Text.Read (readMaybe)
import qualified Replacement.Saclac.Artifact as Artifact
import qualified Replacement.Saclac.PayloadProjection as Payload

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
  | ServiceRequest Address [Argument]
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
      "service-request" -> ServiceRequest <$> value .: "entry" <*> value .: "arguments"
      "store" -> Store <$> value .: "address" <*> value .: "bytes"
      _ -> fail "unknown interface event"
instance ToJSON Event where
  toJSON event = case event of
    QueueIn operation sender payload -> object ["kind" .= ("queue-in" :: String), "operation" .= operation, "sender" .= sender, "payload" .= payload]
    PeerRequest queue operation payload -> object ["kind" .= ("peer-request" :: String), "queue" .= queue, "operation" .= operation, "payload" .= payload]
    Callback entry -> object ["kind" .= ("callback" :: String), "entry" .= entry]
    ClientReply entry target arguments -> object ["kind" .= ("client-reply" :: String), "entry" .= entry, "target" .= target, "arguments" .= arguments]
    ServiceRequest entry arguments -> object ["kind" .= ("service-request" :: String), "entry" .= entry, "arguments" .= arguments]
    Store address bytes -> object ["kind" .= ("store" :: String), "address" .= address, "bytes" .= bytes]

data Observed = Observed { observedInstructions :: Integer, observedEvent :: Event }
instance FromJSON Observed where
  parseJSON = withObject "observed event" $ \value -> Observed <$> value .: "instructions" <*> value .: "event"

data ReachedMilestone = ReachedMilestone { reachedPhase :: String, reachedAt :: Maybe Integer }
instance FromJSON ReachedMilestone where
  parseJSON = withObject "milestone" $ \value -> ReachedMilestone <$> value .: "phase" <*> value .:? "reached"

newtype Digest = Digest Text.Text deriving (Eq, Show)
instance FromJSON Digest where
  parseJSON = withText "SHA-256" $ \text ->
    if Text.length text == 64 && Text.all isHexDigit text then pure (Digest text) else fail "invalid SHA-256"

newtype Binding = Binding Digest deriving (Eq, Show)
instance FromJSON Binding where
  parseJSON = withObject "input binding" $ \value -> Binding <$> value .: "sha256"

data Watch = Watch
  { watchStart :: Address
  , watchEnd :: Address
  , watchFetches :: Integer
  }
instance FromJSON Watch where
  parseJSON = withObject "watch" $ \value -> Watch <$> value .: "start" <*> value .: "end" <*> value .: "fetches"

data Receipt = Receipt
  { scenario :: String
  , events :: [Event]
  , timedEvents :: [Observed]
  , milestones :: [ReachedMilestone]
  , stockFetches :: Integer
  , watches :: [Watch]
  , observationStart :: Integer
  , checkpointAt :: Maybe Integer
  , inputBindings :: [Binding]
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
    watches <- value .: "watch" :: J.Parser [Watch]
    bindings <- value .: "inputs"
    start <- value .: "observation_start_instructions"
    checkpoint <- value .:? "checkpoint"
    checkpointTick <- traverse (withObject "checkpoint" $ \row ->
      withObject "checkpoint record" (.: "instructions") =<< row .: "record") checkpoint
    Receipt name (concatMap (normalize . observedEvent) rows) rows phases (sum fetches)
      watches start checkpointTick bindings <$> value .:? "error" <*> value .:? "machine_failure"

normalize :: Event -> [Event]
normalize (Store (Address address) bytes) = zipWith (\offset byte -> Store (Address (address + offset)) [byte]) [0..] bytes
normalize event = [event]

load :: FilePath -> IO Receipt
load path = do
  bytes <- H.readRegular (16 * 1024 * 1024) path
  either H.failH pure (eitherDecodeStrict bytes)

applyProjection :: Payload.Declaration -> Receipt -> IO Receipt
applyProjection declaration receipt = do
  rows <- traverse projectObserved (timedEvents receipt)
  pure receipt { events = concatMap (normalize . observedEvent) rows, timedEvents = rows }
  where
    projectObserved (Observed tick event) = Observed tick <$> case event of
      QueueIn operation sender bytes -> QueueIn operation sender <$> payloadFields Payload.Incoming sender operation bytes
      PeerRequest queue operation bytes -> PeerRequest queue operation <$> payloadFields Payload.Outgoing queue operation bytes
      _ -> pure event
    payloadFields direction queue operation = either H.failH pure . Payload.projectPayload declaration direction (Payload.QueueId queue) (Payload.Operation operation)

healthy :: Receipt -> Bool
healthy receipt = observationError receipt == Nothing && machineFailure receipt == Nothing
  && not (null (events receipt))

projectionInputs :: Receipt -> Maybe [Binding]
projectionInputs receipt = case inputBindings receipt of
  run : _firmware : rest -> Just (run : rest)
  _ -> Nothing

-- | A side's stream, closed by the milestone marker, and the marker's index. @window-end@ closes
-- a route resumed at the saved post-boot checkpoint after its last event; a census milestone the
-- side reached closes it at that instruction and drops later events. A side that reaches neither
-- carries no marker.
project :: Maybe String -> Receipt -> ([Value], Int)
project Nothing receipt = (map toJSON (events receipt), length (events receipt))
project (Just phase) receipt
  | phase == "window-end", Just tick <- checkpointAt receipt, tick > 0, tick == observationStart receipt =
      (map toJSON (events receipt) ++ [marker], length (events receipt))
  | Just tick <- reachedAt =<< find ((== phase) . reachedPhase) (milestones receipt) =
      let left = concatMap (normalize . observedEvent) (takeWhile ((<= tick) . observedInstructions) (timedEvents receipt))
       in (map toJSON left ++ [marker], length left)
  | otherwise = (map toJSON (events receipt), length (events receipt))
  where marker = object ["milestone" .= phase]

spanValue :: Artifact.Extent -> Value
spanValue extent = object ["start" .= Artifact.extentLow extent, "end" .= Artifact.extentHigh extent]

fetchReceipts :: [Artifact.Extent] -> Receipt -> [Value]
fetchReceipts extents receipt =
  [ object ["start" .= low, "end" .= high, "fetches" .= watchFetches row]
  | row <- watches receipt
  , let Address low = watchStart row
        Address high = watchEnd row
  , any (\extent -> Artifact.extentLow extent == low && Artifact.extentHigh extent == high) extents
  ]

runValue :: Bool -> [Artifact.Extent] -> Maybe String -> Receipt -> IO Value
runValue compatible extents phase receipt = do
  digest <- case inputBindings receipt of
    _ : Binding (Digest image) : _ -> pure image
    _ -> H.failH "census receipt has no firmware binding"
  let (observations, _) = project phase receipt
  pure (object
    [ "image_sha256" .= digest
    , "complete" .= (compatible && healthy receipt)
    , "fetch_receipts" .= fetchReceipts extents receipt
    , "events" .= observations
    ])

-- | The two options after the five positional paths, each with its value.
options :: [String] -> IO (Maybe String, Maybe String)
options [] = pure (Nothing, Nothing)
options (key : value : rest) = do
  (milestoneOption, controlOption) <- options rest
  case key of
    "--milestone" -> pure (Just value, controlOption)
    "--control" -> pure (milestoneOption, Just value)
    _ -> H.failH ("unknown option " ++ key)
options _ = H.failH "an option needs a value"

main :: IO ()
main = do
  arguments <- getArgs
  (stockPath, candidatePath, controlPath, projectionPath, output) <- case take 5 arguments of
    [stock, candidate, control, fields, target] -> pure (stock, candidate, control, fields, target)
    _ -> H.failH "usage: saclac-interface-compare STOCK_JSON CANDIDATE_JSON WRONG_JSON FIELDS_JSON OUTPUT_JSON [--milestone PHASE] [--control NAME]"
  (phase, controlName) <- options (drop 5 arguments)
  let variant = maybe "unconditional-command-present-store" id controlName
  declaration <- H.readRegular (1024 * 1024) projectionPath >>= either H.failH pure . Payload.parseDeclaration
  stock <- load stockPath >>= applyProjection declaration
  candidate <- load candidatePath >>= applyProjection declaration
  control <- load controlPath >>= applyProjection declaration
  let sameScenario = scenario stock == scenario candidate && scenario stock == scenario control
      sameInputs = case map projectionInputs [stock, candidate, control] of
        [Just left, Just middle, Just right] -> left == middle && left == right
        _ -> False
      compatible = sameScenario && sameInputs
      (_, mark) = project phase stock
      milestone = fmap (\name -> object ["name" .= name, "observation" .= mark]) phase
  stockRun <- runValue compatible Artifact.extents phase stock
  ownedRun <- runValue compatible [Artifact.fixedExtent] phase candidate
  wrongRun <- runValue compatible [Artifact.fixedExtent] phase control
  L.writeFile output (encode (object
    [ "schema" .= (1 :: Int)
    , "mode" .= ("interface-stream" :: String)
    , "subsystem" .= ("saclac" :: String)
    , "scenario" .= scenario stock
    , "payload_projection" .= Payload.declarationJson declaration
    , "retired_extents" .= map spanValue Artifact.extents
    , "owned_extents" .= [spanValue Artifact.fixedExtent]
    , "stock" .= stockRun
    , "owned" .= ownedRun
    , "controls" .= [object ["name" .= variant, "run" .= wrongRun]]
    , "milestone" .= milestone
    , "same_observer_and_scenario_inputs" .= sameInputs
    , "stock_executed_instructions_in_action_extents" .= stockFetches stock
    , "normalization" .= ("instruction clocks and cores omitted; each store expanded into ascending-address bytes without reordering events; transient reply pointer addresses omitted and contents retained" :: String)
    ]))
