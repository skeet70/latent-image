{-# LANGUAGE OverloadedStrings #-}
module Replacement.KernelInstall.Wire where

import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.Word (Word8, Word32, Word64)

newtype Address = Address Word32 deriving (Eq, Ord, Show)
newtype WordValue = WordValue Word32 deriving (Eq, Ord, Show)
newtype Count = Count Word64 deriving (Eq, Ord, Show)
newtype Digest = Digest String deriving (Eq, Show)
newtype Bytes = Bytes B.ByteString deriving (Eq, Ord, Show)
data Core = Primary | Secondary deriving (Eq, Ord, Show)
data Program = Retained | Owned deriving (Eq, Show)
data Scenario = Natural | InvalidMarker deriving (Eq, Show)
data Outcome = Returned | Nonreturning | Refused deriving (Eq, Show)
data Source = CpuRead | CpuWrite | CpuFetch | HostRead | HostWrite | ModelBusRead | ModelBusWrite deriving (Eq, Show)
data ByteOrder = Little | Big deriving (Eq, Show)
data ObjectKind = Roots | ServiceStorage | TaskStorage | Stack | Code | Cache | Header0 | Header1 | Marker | Mode | PeerCode | Completion deriving (Eq, Ord, Show)
data Context = Context
  { contextPc :: Address, contextRegisters :: [WordValue], contextSp :: Address, contextLr :: Address
  , contextCpsr :: WordValue, contextCounts :: [Count], contextTcm :: [[WordValue]], contextIdentities :: [WordValue]
  } deriving (Eq, Show)
data Identity = Identity
  { identityRun :: Digest, identityUpdate :: Digest, identityRom :: Digest, identityImage :: Digest
  , identitySymbols :: Digest, identityContract :: Digest, identityObserver :: Digest
  } deriving (Eq, Show)
data MemoryEvent = MemoryEvent
  { eventSource :: Source, eventAddress :: Word64, eventPhysical :: Maybe Word64, eventSize :: Word64
  , eventValue :: Maybe Word64, eventBefore :: Maybe Bytes, eventByteOrder :: Maybe ByteOrder
  , eventCore :: Maybe Core, eventPc :: Maybe Address, eventInstructions :: Maybe Count, eventTick :: Maybe Count
  } deriving (Eq, Show)
data WatchClass = WatchClass {classRange :: [Word64],classKinds :: [Source]} deriving (Eq, Show)
data Watch = Watch
  { watchEnabled :: Bool, watchRanges :: [[Word64]], watchClasses :: [WatchClass]
  , watchViews :: String, watchRecords :: [MemoryEvent], watchViolation :: Maybe String
  } deriving (Eq, Show)
data View = View
  { viewObject :: ObjectKind, viewCore :: Core, viewGuest :: Address, viewPhysical :: Address, viewBytes :: Bytes
  } deriving (Eq, Show)
data Mutation = Mutation Address WordValue WordValue deriving (Eq, Show)
data Observation = Observation
  { observationEntry :: Maybe Context, observationCache :: Maybe Context, observationCacheReturn :: Maybe Context
  , observationAfter :: Maybe Context, observationMutation :: Maybe Mutation
  , observationBefore :: [View], observationCommitted :: [View], observationWatch :: Maybe Watch
  , observationWatchEndError :: Maybe String
  } deriving (Eq, Show)
data Receipt = Receipt
  { receiptSchema :: Int, receiptProgram :: Program, receiptScenario :: Scenario, receiptOutcome :: Outcome
  , receiptError :: Maybe String, receiptMachineFailure :: Maybe String, receiptIdentity :: Identity
  , receiptCheckpoint :: Digest, receiptCheckpointReused :: Bool, receiptInstructions :: Count
  , receiptObservation :: Observation
  } deriving (Eq, Show)

instance FromJSON Address where parseJSON value = Address <$> parseJSON value
instance FromJSON WordValue where parseJSON value = WordValue <$> parseJSON value
instance FromJSON Count where parseJSON value = Count <$> parseJSON value
instance FromJSON Digest where
  parseJSON value = do
    text <- parseJSON value
    if length text == 64 && all (\character -> (character >= '0' && character <= '9') || character `elem` ['a'..'f']) text
      then pure (Digest text) else fail "invalid digest"
instance FromJSON Bytes where
  parseJSON value = Bytes . B.pack <$> (parseJSON value :: Parser [Word8])
instance FromJSON Core where
  parseJSON value = do
    index <- parseJSON value :: Parser Int
    case index of 0 -> pure Primary; 1 -> pure Secondary; _ -> fail "invalid core"
instance FromJSON Program where
  parseJSON = withText "program" $ \value -> case value of "retained" -> pure Retained; "owned" -> pure Owned; _ -> fail "invalid program"
instance FromJSON Scenario where
  parseJSON = withText "scenario" $ \value -> case value of "natural" -> pure Natural; "invalid-marker" -> pure InvalidMarker; _ -> fail "invalid scenario"
instance FromJSON Outcome where
  parseJSON = withText "outcome" $ \value -> case value of "returned" -> pure Returned; "nonreturning" -> pure Nonreturning; "refused" -> pure Refused; _ -> fail "invalid outcome"
instance FromJSON Source where
  parseJSON = withText "access source" $ \value -> case value of
    "cpu-read" -> pure CpuRead; "cpu-write" -> pure CpuWrite; "cpu-fetch" -> pure CpuFetch
    "host-read" -> pure HostRead; "host-write" -> pure HostWrite
    "model-bus-read" -> pure ModelBusRead; "model-bus-write" -> pure ModelBusWrite
    _ -> fail "invalid access source"
instance FromJSON ByteOrder where
  parseJSON = withText "byte order" $ \value -> case value of "little" -> pure Little; "big" -> pure Big; _ -> fail "invalid byte order"
instance FromJSON ObjectKind where
  parseJSON = withText "object" $ \value -> case value of
    "roots" -> pure Roots; "service-storage" -> pure ServiceStorage; "task-storage" -> pure TaskStorage
    "stack" -> pure Stack; "code" -> pure Code; "cache" -> pure Cache; "header0" -> pure Header0
    "header1" -> pure Header1; "marker" -> pure Marker; "mode" -> pure Mode
    "peer-code" -> pure PeerCode; "completion" -> pure Completion
    _ -> fail "invalid object"
instance FromJSON Context where
  parseJSON = withObject "context" $ \o -> Context <$> o .: "pc" <*> o .: "registers" <*> o .: "sp" <*> o .: "lr"
    <*> o .: "cpsr" <*> o .: "counts" <*> o .: "tcm" <*> o .: "identities"
instance FromJSON Identity where
  parseJSON = withObject "identity" $ \o -> Identity <$> o .: "run" <*> o .: "update" <*> o .: "boot_rom"
    <*> o .: "image" <*> o .: "symbols" <*> o .: "contract" <*> o .: "observer"
instance FromJSON MemoryEvent where
  parseJSON = withObject "memory event" $ \o -> MemoryEvent <$> o .: "source" <*> o .: "address" <*> o .:? "physical"
    <*> o .: "size" <*> o .:? "value" <*> o .:? "before" <*> o .:? "byte_order" <*> o .:? "core"
    <*> o .:? "pc" <*> o .:? "instructions" <*> o .:? "tick"
instance FromJSON WatchClass where
  parseJSON = withObject "watch class" $ \o -> WatchClass <$> o .: "range" <*> o .: "kinds"
instance FromJSON Watch where
  parseJSON = withObject "watch" $ \o -> Watch <$> o .: "enabled" <*> o .: "ranges" <*> o .: "classes"
    <*> o .: "views" <*> o .: "records" <*> o .:? "violation"
instance FromJSON View where
  parseJSON = withObject "view" $ \o -> View <$> o .: "object" <*> o .: "core" <*> o .: "guest" <*> o .: "physical" <*> o .: "bytes"
instance FromJSON Mutation where
  parseJSON = withObject "mutation" $ \o -> Mutation <$> o .: "address" <*> o .: "before" <*> o .: "after"
instance FromJSON Observation where
  parseJSON = withObject "observation" $ \o -> Observation <$> o .:? "entry" <*> o .:? "cache" <*> o .:? "cache_return"
    <*> o .:? "after" <*> o .:? "mutation" <*> o .: "before" <*> o .: "committed" <*> o .:? "watch" <*> o .:? "watch_end_error"
instance FromJSON Receipt where
  parseJSON = withObject "receipt" $ \o -> Receipt <$> o .: "schema" <*> o .: "program" <*> o .: "scenario"
    <*> o .: "outcome" <*> o .:? "error" <*> o .:? "machine_failure" <*> o .: "identity"
    <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions" <*> o .: "observation"
