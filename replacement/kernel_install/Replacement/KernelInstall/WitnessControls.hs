{-# LANGUAGE OverloadedStrings #-}
module Replacement.KernelInstall.WitnessControls (checkRefusals) where

import Control.Monad (forM)
import qualified Data.Aeson as J
import Data.Aeson.Key (Key)
import qualified Data.Aeson.KeyMap as K
import Data.Aeson.Types (parseMaybe)
import Data.Bits (xor)
import qualified Data.ByteString.Lazy as L
import Data.Word (Word64)
import Replacement.KernelInstall.Wire (Source (..))
import Replacement.KernelInstall.Witness

data Step = Field Key | Item Int

data Fault
  = WrongProducer | WrongCheckpoint | RefusedOutcome | DisabledWatch | WatchEndFailure
  | MissingCache | WrongCacheReturn | WrongRootValue | WrongRootDestination | UnknownFetchPc
  | RawWatchMismatch | AliasDisagreement | WrongMarkerMutation | WrongTerminalPc
  | BelowStackStore | WrongEventClock | AlienWriter | MissingFetch
  | MissingStoppedFetch | DuplicateExecutedFetch | WrongStopCounter
  deriving (Bounded, Enum, Show)

expected :: Fault -> WitnessError
expected fault = case fault of
  WrongProducer -> ProducerIdentityRejected; WrongCheckpoint -> CheckpointRejected
  RefusedOutcome -> ReceiptStatusRejected; DisabledWatch -> WatchRejected; WatchEndFailure -> ReceiptStatusRejected
  MissingCache -> CacheRejected; WrongCacheReturn -> CacheRejected; WrongRootValue -> StoreOrderRejected
  WrongRootDestination -> WriteRejected; UnknownFetchPc -> EventRejected; RawWatchMismatch -> RawWatchRejected
  AliasDisagreement -> ViewsRejected; WrongMarkerMutation -> MutationRejected; WrongTerminalPc -> TerminalRejected
  BelowStackStore -> WriteRejected; WrongEventClock -> EventOrderRejected; AlienWriter -> EventRejected
  MissingFetch -> FetchRejected
  MissingStoppedFetch -> FetchRejected; DuplicateExecutedFetch -> FetchRejected; WrongStopCounter -> FetchRejected

change :: [Step] -> (J.Value -> Either String J.Value) -> J.Value -> Either String J.Value
change [] action value = action value
change (Field key:rest) action (J.Object fields) = do
  value <- maybe (Left "control field absent") Right (K.lookup key fields)
  replacement <- change rest action value
  pure (J.Object (K.insert key replacement fields))
change (Item index:rest) action value = do
  values <- case J.fromJSON value of J.Success values -> Right values; J.Error _ -> Left "control array absent"
  if index < 0 || index >= length values then Left "control index absent" else do
    replacement <- change rest action (values !! index)
    pure (J.toJSON (take index values ++ [replacement] ++ drop (index+1) values))
change _ _ _ = Left "control object absent"

get :: [Step] -> J.Value -> Either String J.Value
get [] value = Right value
get (Field key:rest) (J.Object fields) = maybe (Left "control field absent") (get rest) (K.lookup key fields)
get (Item index:rest) value = case J.fromJSON value of
  J.Success values | index >= 0 && index < length values -> get rest (values !! index)
  _ -> Left "control index absent"
get _ _ = Left "control object absent"

replace :: J.Value -> J.Value -> Either String J.Value
replace value _ = Right value
numeric :: Word64 -> J.Value
numeric = J.toJSON
flipBit :: J.Value -> Either String J.Value
flipBit value = case J.fromJSON value of
  J.Success number -> Right (numeric (number `xor` 1)); J.Error _ -> Left "control number absent"

observation :: [Step]
observation = [Field "observation"]
watch :: [Step]
watch = observation ++ [Field "watch"]
events :: [Step]
events = watch ++ [Field "records"]

source :: J.Value -> Maybe Source
source = parseMaybe (J.withObject "event" (J..: "source"))
guest :: J.Value -> Maybe Word64
guest = parseMaybe (J.withObject "event" (J..: "address"))

firstEvent :: (J.Value -> Bool) -> (J.Value -> Either String J.Value) -> J.Value -> Either String J.Value
firstEvent predicate action = change events $ \value -> case J.fromJSON value of
  J.Success values -> do
    let (prefix,suffix) = break predicate values
    case suffix of
      [] -> Left "control event absent"
      event:rest -> do replacement <- action event; pure (J.toJSON (prefix ++ [replacement] ++ rest))
  J.Error _ -> Left "control events absent"

rootWrite :: J.Value -> Bool
rootWrite event = source event == Just CpuWrite && guest event == Just 0x96bd20

mutate :: Fault -> J.Value -> Either String J.Value
mutate fault = case fault of
  WrongProducer -> change [Field "identity",Field "observer"] (replace (J.toJSON (replicate 64 '0')))
  WrongCheckpoint -> change [Field "checkpoint_sha256"] (replace (J.toJSON (replicate 64 '0')))
  RefusedOutcome -> change [Field "outcome"] (replace (J.String "refused"))
  DisabledWatch -> change (watch ++ [Field "enabled"]) (replace (J.Bool False))
  WatchEndFailure -> change (observation ++ [Field "watch_end_error"]) (replace (J.String "control"))
  MissingCache -> change (observation ++ [Field "cache"]) (replace J.Null)
  WrongCacheReturn -> change (observation ++ [Field "cache_return",Field "registers",Item 0]) (replace (numeric 0))
  WrongRootValue -> firstEvent rootWrite (change [Field "value"] flipBit)
  WrongRootDestination -> firstEvent rootWrite $ \event -> do
    changed <- change [Field "address"] (replace (numeric 0x96bd28)) event
    change [Field "physical"] (replace (numeric 0x96bd28)) changed
  UnknownFetchPc -> firstEvent ((== Just CpuFetch) . source) (change [Field "pc"] (replace (numeric 0xdeadbeec)))
  RawWatchMismatch -> change (watch ++ [Field "enabled"]) (replace (J.Bool False))
  AliasDisagreement -> change (observation ++ [Field "committed",Item 0,Field "bytes",Item 0]) flipBit
  WrongMarkerMutation -> change (observation ++ [Field "mutation"]) $ \value -> case value of
    J.Null -> Right (J.object ["address" J..= (0x7caf3c :: Word64),"before" J..= (3 :: Word64),"after" J..= (0 :: Word64)])
    _ -> Right J.Null
  WrongTerminalPc -> change (observation ++ [Field "after",Field "pc"]) (replace (numeric 0xdeadbeec))
  BelowStackStore -> firstEvent ((== Just CpuWrite) . source) $ \event -> do
    changed <- change [Field "address"] (replace (numeric 0x40063300)) event
    change [Field "physical"] (replace (numeric 0x63300)) changed
  WrongEventClock -> change (events ++ [Item 0,Field "instructions"]) (replace (numeric 0))
  AlienWriter -> firstEvent ((== Just CpuWrite) . source) (change [Field "source"] (replace (J.String "host-write")))
  MissingFetch -> change events $ \value -> case J.fromJSON value of
    J.Success values -> let (prefix,suffix) = break ((== Just CpuFetch) . source) values in case suffix of
      _:rest -> Right (J.toJSON (prefix ++ rest)); [] -> Left "control fetch absent"
    J.Error _ -> Left "control events absent"
  MissingStoppedFetch -> change events $ \value -> case J.fromJSON value of
    J.Success values -> let (prefix,suffix) = break (\event -> source event == Just CpuFetch && guest event == Just 0x42634) values in case suffix of
      _:rest -> Right (J.toJSON (prefix ++ rest)); [] -> Left "control stopped fetch absent"
    J.Error _ -> Left "control events absent"
  DuplicateExecutedFetch -> change events $ \value -> case J.fromJSON value of
    J.Success values -> let (prefix,suffix) = break ((== Just CpuFetch) . source) values in case suffix of
      event:rest -> Right (J.toJSON (prefix ++ [event,event] ++ rest)); [] -> Left "control fetch absent"
    J.Error _ -> Left "control events absent"
  WrongStopCounter -> change (observation ++ [Field "cache",Field "counts",Item 0]) flipBit

checkRefusals :: Inputs -> ObservedFiles -> ObservedFiles -> Either String J.Value
checkRefusals inputs retained owned = do
  results <- forM [retained,owned] $ \files -> do
    case admitReceipt inputs files of Left failure -> Left ("control baseline refused: " ++ show failure); Right _ -> pure ()
    packet <- J.eitherDecodeStrict' (observedReceipt files)
    forM [minBound..maxBound] $ \fault -> do
      altered <- mutate fault packet
      alteredWatch <- get watch altered
      let changed = files {observedReceipt = L.toStrict (J.encode altered)
            ,observedWatch = case fault of RawWatchMismatch -> observedWatch files; _ -> L.toStrict (J.encode alteredWatch)}
      case admitReceipt inputs changed of
        Left failure | failure == expected fault -> Right (J.object ["fault" J..= show fault,"reason" J..= show failure])
        Left failure -> Left ("control " ++ show fault ++ " refused for " ++ show failure ++ ", expected " ++ show (expected fault))
        Right _ -> Left ("control admitted: " ++ show fault)
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("public-receipt-refusal-controls" :: String)
    ,"cases_per_program" J..= (fromEnum (maxBound :: Fault)+1),"results" J..= results])
