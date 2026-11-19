{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (join, unless)
import Data.Aeson
import qualified Data.Aeson.Types as J
import qualified Data.ByteString.Lazy as L
import Data.Char (isHexDigit)
import Data.List (find)
import qualified Data.Text as Text
import Data.Word (Word8, Word16, Word32)
import qualified FujiAF.FirstStage as F
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
    -- The client-visible call: stock observes it at its reply helpers' `blx` into the client and
    -- Root at its client call veneer; the helper site is retired saclac code, so it is not compared.
    ClientReply _ target arguments -> object ["kind" .= ("client-reply" :: String), "target" .= target, "arguments" .= arguments]
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
  -- | The board tick of a board-clock schedule's `end` stimulus, when the run stopped exactly there.
  , scheduleEnd :: Maybe Integer
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
    executed <- value .: "instructions"
    stimuli <- value .:? "stimuli"
    ending <- traverse (withObject "stimuli" $ \row -> do
      clock <- row .: "clock"
      points <- row .: "points" :: J.Parser [Value]
      final <- case reverse points of
        point : _ -> withObject "stimulus point" (\entry -> do
          action <- withObject "stimulus" (.: "action") =<< entry .: "stimulus"
          tick <- entry .: "tick"
          at <- entry .: "instructions"
          pure (if (action :: String) == "end" && at == (executed :: Integer) then Just tick else Nothing)) point
        [] -> pure Nothing
      pure (if (clock :: String) == "board-ticks" then final else Nothing)) stimuli
    Receipt name (concatMap (normalize . observedEvent) rows) rows phases (sum fetches)
      watches start checkpointTick (join ending) bindings <$> value .:? "error" <*> value .:? "machine_failure"

normalize :: Event -> [Event]
normalize (Store (Address address) bytes) = zipWith (\offset byte -> Store (Address (address + offset)) [byte]) [0..] bytes
normalize event = [event]

load :: FilePath -> IO Receipt
load path = do
  bytes <- H.readRegular (16 * 1024 * 1024) path
  either H.failH pure (eitherDecodeStrict bytes)

applyProjection :: Payload.Role -> Payload.Declaration -> Receipt -> IO Receipt
applyProjection role declaration receipt = do
  rows <- traverse projectObserved (timedEvents receipt)
  pure receipt { events = concatMap (normalize . observedEvent) rows, timedEvents = rows }
  where
    projectObserved (Observed tick event) = Observed tick <$> case event of
      QueueIn operation sender bytes -> QueueIn operation sender <$> payloadFields Payload.Incoming sender operation bytes
      PeerRequest queue operation bytes -> PeerRequest queue operation <$> payloadFields Payload.Outgoing queue operation bytes
      _ -> pure event
    payloadFields direction queue operation = either H.failH pure . Payload.projectPayload role declaration direction (Payload.QueueId queue) (Payload.Operation operation)

-- | The owned role of one compared lane: its linked symbol table, bound to the candidate its census
-- observed through the staging receipt (candidate DAT and fixed body) and the stage contract
-- (fixed body and linked symbols). Any break in that chain refuses the comparison.
data ImageBinding = ImageBinding { bindingContract :: FilePath, bindingStaging :: FilePath, bindingSymbols :: FilePath }

imageBinding :: String -> IO ImageBinding
imageBinding text = case splitOn ',' text of
  [contract, staging, symbols] -> pure (ImageBinding contract staging symbols)
  _ -> H.failH "an image binding is CONTRACT,STAGING,SYMBOLS"
  where
    splitOn separator value = case break (== separator) value of
      (part, []) -> [part]
      (part, _ : rest) -> part : splitOn separator rest

ownedRole :: Receipt -> ImageBinding -> IO (Payload.Role, Value)
ownedRole receipt binding = do
  observed <- case inputBindings receipt of
    _ : Binding (Digest candidate) : _ -> pure candidate
    _ -> H.failH "census receipt has no candidate binding"
  contract <- jsonFile (bindingContract binding)
  staging <- jsonFile (bindingStaging binding)
  symbolsBytes <- H.readRegular (16 * 1024 * 1024) (bindingSymbols binding)
  let symbolsDigest = Text.pack (Artifact.digest symbolsBytes)
  let field path value = either H.failH pure (J.parseEither path value)
  stagedCandidate <- field (withObject "staging" (.: "dat_sha256")) staging
  stagedBody <- field (withObject "staging" (.: "body_sha256")) staging
  contractBody <- field (withObject "contract" (\row -> row .: "fixed_body" >>= withObject "fixed body" (.: "sha256"))) contract
  contractSymbols <- field (withObject "contract" (.: "linked_symbols_sha256")) contract
  H.ensure (stagedCandidate == observed) "staging receipt names another candidate than the census observed"
  H.ensure (stagedBody == (contractBody :: Text.Text)) "staging receipt and stage contract name different fixed bodies"
  H.ensure (contractSymbols == symbolsDigest) "symbol table is not the stage contract's linked symbols"
  symbols <- either H.failH pure (F.parseSymbols symbolsBytes)
  let resolve name = either Left (Right . F.symbolAddress) (F.lookupSymbol symbols (Text.unpack name))
  pure (Payload.OwnedRole resolve, object ["candidate_sha256" .= observed, "fixed_body_sha256" .= stagedBody, "linked_symbols_sha256" .= symbolsDigest])
  where jsonFile path = H.readRegular (1024 * 1024) path >>= either H.failH pure . eitherDecodeStrict

healthy :: Receipt -> Bool
healthy receipt = observationError receipt == Nothing && machineFailure receipt == Nothing
  && not (null (events receipt))

projectionInputs :: Receipt -> Maybe [Binding]
projectionInputs receipt = case inputBindings receipt of
  run : _firmware : rest -> Just (run : rest)
  _ -> Nothing

-- | A side's stream, closed by the milestone marker, and the marker's index. @window-end@ closes a
-- side after its last event when it is a route resumed at the saved post-boot checkpoint, or a
-- continuous board-clock route that stopped exactly at its schedule's `end` stimulus; the caller
-- requires every side's end at the same board tick. A census milestone the side reached closes it
-- at that instruction and drops later events. A side that reaches neither carries no marker.
project :: Maybe String -> Receipt -> ([Value], Int)
project Nothing receipt = (map toJSON (events receipt), length (events receipt))
project (Just phase) receipt
  | phase == "window-end", Just tick <- checkpointAt receipt, tick > 0, tick == observationStart receipt =
      (map toJSON (events receipt) ++ [marker], length (events receipt))
  | phase == "window-end", checkpointAt receipt == Nothing, Just _ <- scheduleEnd receipt =
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

-- | The options after the five positional paths, each with its value.
options :: [String] -> IO [(String, String)]
options [] = pure []
options (key : value : rest)
  | key `elem` ["--milestone", "--control", "--candidate-image", "--control-image"] = ((key, value) :) <$> options rest
  | otherwise = H.failH ("unknown option " ++ key)
options _ = H.failH "an option needs a value"

main :: IO ()
main = do
  arguments <- getArgs
  let usage = "usage: saclac-interface-compare STOCK_JSON CANDIDATE_JSON WRONG_JSON FIELDS_JSON OUTPUT_JSON --candidate-image CONTRACT,STAGING,SYMBOLS --control-image CONTRACT,STAGING,SYMBOLS [--milestone PHASE] [--control NAME]"
  (stockPath, candidatePath, controlPath, projectionPath, output) <- case take 5 arguments of
    [stock, candidate, control, fields, target] -> pure (stock, candidate, control, fields, target)
    _ -> H.failH usage
  given <- options (drop 5 arguments)
  let phase = lookup "--milestone" given
      variant = maybe "unconditional-command-present-store" id (lookup "--control" given)
      required key = maybe (H.failH usage) imageBinding (lookup key given)
  candidateBinding <- required "--candidate-image"
  controlBinding <- required "--control-image"
  declaration <- H.readRegular (1024 * 1024) projectionPath >>= either H.failH pure . Payload.parseDeclaration
  stockReceipt <- load stockPath
  candidateReceipt <- load candidatePath
  controlReceipt <- load controlPath
  (candidateRole, candidateBound) <- ownedRole candidateReceipt candidateBinding
  (controlRole, controlBound) <- ownedRole controlReceipt controlBinding
  stock <- applyProjection Payload.StockRole declaration stockReceipt
  candidate <- applyProjection candidateRole declaration candidateReceipt
  control <- applyProjection controlRole declaration controlReceipt
  -- A continuous window end is one bound only when every side stopped at the same scheduled tick.
  let sides = [stock, candidate, control]
      continuous = all ((== Nothing) . checkpointAt) sides
      sameEnd = case map scheduleEnd sides of
        first : rest -> first /= Nothing && all (== first) rest
        [] -> False
  unless (phase /= Just "window-end" || not continuous || sameEnd)
    (H.failH "window-end needs every continuous side to stop at the same scheduled end tick")
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
    , "owned_identity_bindings" .= object ["owned" .= candidateBound, "controls" .= [controlBound]]
    , "stock_executed_instructions_in_action_extents" .= stockFetches stock
    , "normalization" .= ("instruction clocks and cores omitted; each store expanded into ascending-address bytes without reordering events; transient reply pointer addresses omitted and contents retained; a declared owned-identity word (owned code or storage, admitted only with cited retained-consumer use) projects to its stock value only when it equals that value (stock) or the declared replacement symbol's address in the lane's own bound symbol table (owned), and any other word refuses the comparison" :: String)
    ]))
