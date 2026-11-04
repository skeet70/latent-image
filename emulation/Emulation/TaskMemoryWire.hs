{-# LANGUAGE OverloadedStrings #-}
module Emulation.TaskMemoryWire where

import Control.Monad (unless)
import Data.Aeson hiding (Object)
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Word (Word64)
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.KernelInstall.Wire as W

data Role = Retained | Owned deriving (Eq, Show)
data Object = CreationRecord | TaskNode | Current | Selected | Pool | Stack | Code | LoadedCode | InterfaceRoots | InterfaceTable
  | TaskDescriptor | CallbackResource | ServiceCode | ResourceState | ResourceDescriptor | TaskClasses deriving (Eq, Ord, Show)
data View = View {viewObject :: Object, viewCore :: S.KernelCore, viewPhysical :: W.Address, viewBytes :: W.Bytes, viewAliases :: [[W.Digest]]} deriving (Eq, Show)
data Context = Context {contextCore :: S.KernelCore, contextState :: W.Context, contextSpsr :: W.WordValue} deriving (Eq, Show)
data Identity = Identity {identityRun :: W.Digest, identityUpdate :: W.Digest, identityRom :: W.Digest, identityObserver :: W.Digest, identityLinked :: W.Digest, identitySymbols :: W.Digest, identityContract :: W.Digest, identitySources :: [S.SourceIdentity]} deriving (Eq, Show)

data SourceFiles = SourceFiles
  { sourceDat :: B.ByteString, runDat :: B.ByteString, runDecoded :: B.ByteString, sourceRun :: B.ByteString, sourceRom :: B.ByteString
  , sourceA :: B.ByteString, sourceB :: B.ByteString, sourceImage :: B.ByteString, sourceObserver :: B.ByteString
  , sourceLinked :: B.ByteString, sourceSymbols :: B.ByteString, sourceContract :: B.ByteString }
-- | 'observedBytes' is the compact receipt's byte sidecar, empty for an inline receipt.
data ObservedFiles = ObservedFiles
  {observedReceipt :: B.ByteString, observedWatch :: B.ByteString, observedReset :: B.ByteString, observedBytes :: B.ByteString}

newtype PhysicalBoundary = PhysicalBoundary Word64 deriving (Eq, Show)
data FetchRange = FetchRange PhysicalBoundary PhysicalBoundary deriving (Eq, Show)
data FetchAttribution = FetchAttribution [W.Source] [FetchRange] deriving (Eq, Show)
data WatchViews = ObserveWatch | FixedWatch deriving (Eq, Show)
newtype WatchFailure = WatchFailure String deriving (Eq, Show)
data FetchObservation = FetchObservation {fetchRecord :: W.MemoryEvent, fetchCoreInstructions :: W.Count} deriving (Eq, Show)
data CapturedEvent = CapturedEvent
  { eventRecord :: W.MemoryEvent, eventCoreInstructions :: Maybe W.Count, eventLatestFetch :: Maybe FetchObservation } deriving (Eq, Show)
data CapturedWatch = CapturedWatch
  { captureEnabled :: Bool, captureRanges :: [[Word64]], captureClasses :: [W.WatchClass], captureViews :: WatchViews
  , captureEvents :: [CapturedEvent], captureViolation :: Maybe WatchFailure, captureAttribution :: FetchAttribution } deriving (Eq, Show)

parseRole :: String -> Parser Role
parseRole "retained" = pure Retained
parseRole "owned" = pure Owned
parseRole _ = fail "invalid task role"
instance FromJSON Role where parseJSON value = parseJSON value >>= parseRole
instance ToJSON Role where toJSON Retained = String "retained"; toJSON Owned = String "owned"
instance FromJSON Object where
  parseJSON = withText "task view" $ \value -> case value of
    "creation-record" -> pure CreationRecord; "task-node" -> pure TaskNode; "current" -> pure Current; "selected" -> pure Selected
    "pool" -> pure Pool; "stack" -> pure Stack; "code" -> pure Code; "interface-roots" -> pure InterfaceRoots; "interface-table" -> pure InterfaceTable
    "loaded-code" -> pure LoadedCode; "task-descriptor" -> pure TaskDescriptor; "callback-resource" -> pure CallbackResource
    "service-code" -> pure ServiceCode; "resource-state" -> pure ResourceState; "resource-descriptor" -> pure ResourceDescriptor
    "task-classes" -> pure TaskClasses; _ -> fail "invalid task view"
instance FromJSON View where
  parseJSON = withObject "task view" $ \o -> View <$> o .: "object" <*> o .: "core" <*> o .: "physical" <*> o .: "bytes" <*> o .: "aliases"
instance FromJSON Context where
  parseJSON value = withObject "task context" (\o -> Context <$> o .: "core" <*> parseJSON value <*> o .: "spsr") value
instance FromJSON Identity where
  parseJSON = withObject "task identity" $ \o -> Identity <$> o .: "run" <*> o .: "update" <*> o .: "boot_rom" <*> o .: "observer"
    <*> o .: "linked" <*> o .: "symbols" <*> o .: "contract" <*> o .: "sources"
instance FromJSON FetchRange where
  parseJSON value = do
    bounds <- parseJSON value :: Parser [Word64]
    case bounds of
      [low,high] | low < high && high <= 0x100000000 -> pure (FetchRange (PhysicalBoundary low) (PhysicalBoundary high))
      _ -> fail "invalid physical fetch span"
instance FromJSON FetchAttribution where
  parseJSON = withObject "fetch attribution" $ \o -> FetchAttribution <$> o .: "sources" <*> o .: "ranges"
instance FromJSON WatchViews where
  parseJSON = withText "watch views" $ \value -> case value of
    "observe" -> pure ObserveWatch; "fixed" -> pure FixedWatch; _ -> fail "invalid watch views"
instance FromJSON WatchFailure where parseJSON value = WatchFailure <$> parseJSON value
instance FromJSON FetchObservation where
  parseJSON value = withObject "actual fetched instruction" (\o -> do
    unless (not (KM.member "latest_fetch" o)) (fail "recursive fetched instruction")
    FetchObservation <$> parseJSON value <*> o .: "core_instructions") value
instance FromJSON CapturedEvent where
  parseJSON value = withObject "attributed memory event" (\o -> CapturedEvent <$> parseJSON value
    <*> o .:? "core_instructions" <*> o .:? "latest_fetch") value
instance FromJSON CapturedWatch where
  parseJSON = withObject "attributed memory watch" $ \o -> CapturedWatch <$> o .: "enabled" <*> o .: "ranges" <*> o .: "classes"
    <*> o .: "views" <*> o .: "records" <*> o .:? "violation" <*> o .: "fetch_attribution"
