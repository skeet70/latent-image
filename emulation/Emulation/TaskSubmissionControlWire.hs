{-# LANGUAGE OverloadedStrings #-}
module Emulation.TaskSubmissionControlWire
  ( ControlEnvelope(..), ProbeCase(..), ProbeTarget(..), ProbeStage(..), ProbeRegister(..)
  , RegisterChange(..), ControlPatch(..), ProbeRecord(..)
  ) where

import Control.Monad (unless)
import qualified Data.Aeson.Key as K
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Emulation.StartupTaskWire (Receipt, Context(..), View(..), ObjectKind(ControlPatch), TaskId, HitIndex, Cursor)
import qualified Replacement.KernelInstall.Wire as W

data ProbeCase = FreshEnabled | FreshMasked | ReusedRecord | ReservedRefusal | PartialPair
  | SentinelFresh | ReleaseUnlocked | ReleasePeerLock | ReleaseUnlockRefusal | CallbackUnreturned
  deriving (Eq, Ord, Show)
data ProbeTarget = SubmissionTarget | DeletionTarget deriving (Eq, Ord, Show)
data ProbeStage = InitialStage | ReleaseEntryStage | UnlockCallStage deriving (Eq, Ord, Show)
data ProbeRegister = ArgumentRegister | LinkRegister | ProgramCounter | StatusRegister deriving (Eq, Ord, Show)
data RegisterChange = RegisterChange
  { changeRegister :: ProbeRegister, changeBefore :: W.WordValue, changeAfter :: W.WordValue }
  deriving (Eq, Show)
data ControlPatch = MemoryPatch
  { patchAfterHit :: HitIndex, patchWatchCursor :: Cursor, patchAddress :: W.Address
  , patchBefore :: View, patchAfter :: View }
  deriving (Eq, Show)
data ProbeRecord = ProbeRecord
  { probeCase :: ProbeCase, probeTarget :: ProbeTarget, probeTask :: TaskId
  , probeDescriptor :: W.Address, probeReturnPc :: W.Address, probeInitialHit :: HitIndex
  , probeBefore :: Context, probeConfigured :: Context
  , probeEntryHit :: Maybe HitIndex, probeDeleteEntryHit :: Maybe HitIndex, probeReturnHit :: Maybe HitIndex
  , probeReleaseEntryHit :: Maybe HitIndex, probeReleaseReturnHit :: Maybe HitIndex
  , probeCallbackEntryHit :: Maybe HitIndex, probeCallbackEnteredHit :: Maybe HitIndex
  , probeRegisterChanges :: [RegisterChange], probePatches :: [ControlPatch] }
  deriving (Eq, Show)
data ControlEnvelope = ControlEnvelope { controlReceipt :: Receipt, controlProbe :: Maybe ProbeRecord }
  deriving (Eq, Show)

closed :: [String] -> Object -> Parser ()
closed names object = unless (all ((`elem` names) . K.toString) (KM.keys object)) (fail "unknown probe field")

instance FromJSON ProbeCase where
  parseJSON = withText "probe case" $ \value -> case value of
    "fresh-enabled" -> pure FreshEnabled; "fresh-masked" -> pure FreshMasked
    "reused-record" -> pure ReusedRecord; "reserved-refusal" -> pure ReservedRefusal
    "partial-pair" -> pure PartialPair; "sentinel-fresh" -> pure SentinelFresh
    "release-unlocked" -> pure ReleaseUnlocked; "release-peer-lock" -> pure ReleasePeerLock
    "release-unlock-refusal" -> pure ReleaseUnlockRefusal; "callback-unreturned" -> pure CallbackUnreturned
    _ -> fail "unknown controlled probe case"
instance FromJSON ProbeTarget where
  parseJSON = withText "probe target" $ \value -> case value of
    "submission" -> pure SubmissionTarget; "deletion" -> pure DeletionTarget; _ -> fail "unknown probe target"
instance FromJSON ProbeStage where
  parseJSON = withText "probe stage" $ \value -> case value of
    "initial" -> pure InitialStage; "release-entry" -> pure ReleaseEntryStage
    "unlock-call" -> pure UnlockCallStage; _ -> fail "unknown probe stage"
instance FromJSON ProbeRegister where
  parseJSON value = do
    register <- parseJSON value :: Parser Int
    case register of 0 -> pure ArgumentRegister; 14 -> pure LinkRegister; 15 -> pure ProgramCounter
                     16 -> pure StatusRegister; _ -> fail "undeclared probe register"
instance FromJSON RegisterChange where
  parseJSON = withObject "register change" $ \o -> do
    closed ["register","before","after"] o
    RegisterChange <$> o .: "register" <*> o .: "before" <*> o .: "after"
instance FromJSON ControlPatch where
  parseJSON = withObject "control patch" $ \o -> do
    closed ["after_hit","watch_cursor","address","before","after"] o
    patch <- MemoryPatch <$> o .: "after_hit" <*> o .: "watch_cursor" <*> o .: "address" <*> o .: "before" <*> o .: "after"
    let shape view = viewObject view == ControlPatch && case viewBytes view of W.Bytes raw -> B.length raw == 4
    unless (shape (patchBefore patch) && shape (patchAfter patch)) (fail "probe patch is not one word")
    pure patch
instance FromJSON ProbeRecord where
  parseJSON = withObject "probe record" $ \o -> do
    closed ["case","target","task","descriptor","return_pc","initial_hit","before","configured","entry_hit"
      ,"delete_entry_hit","return_hit","release_entry_hit","release_return_hit","callback_entry_hit","callback_entered_hit"
      ,"register_changes","patches"] o
    record <- ProbeRecord <$> o .: "case" <*> o .: "target" <*> o .: "task" <*> o .: "descriptor" <*> o .: "return_pc"
      <*> o .: "initial_hit" <*> o .: "before" <*> o .: "configured" <*> o .:? "entry_hit" <*> o .:? "delete_entry_hit"
      <*> o .:? "return_hit" <*> o .:? "release_entry_hit" <*> o .:? "release_return_hit"
      <*> o .:? "callback_entry_hit" <*> o .:? "callback_entered_hit" <*> o .: "register_changes" <*> o .: "patches"
    unless (length (probeRegisterChanges record) <= 4 && length (probePatches record) <= 10) (fail "probe mutation bound exceeded")
    let shape context = let state = contextState context in length (W.contextRegisters state) == 13
          && length (W.contextCounts state) == 2 && length (W.contextIdentities state) == 2
          && length (W.contextTcm state) == 2 && all ((==2) . length) (W.contextTcm state)
    unless (shape (probeBefore record) && shape (probeConfigured record)) (fail "incomplete probe context")
    pure record
instance FromJSON ControlEnvelope where
  parseJSON value = withObject "probe envelope" (\o -> ControlEnvelope <$> parseJSON value <*> o .:? "probe") value
