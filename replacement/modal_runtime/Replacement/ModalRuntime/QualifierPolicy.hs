{-# LANGUAGE OverloadedStrings #-}
module Replacement.ModalRuntime.QualifierPolicy
  ( Role (..), OwnedBase (..), OwnedArchive (..), MatrixPin (..), Qualification (..)
  , parseInvocation, qualificationOptions, roleFirmware, ownedBaseInputs
  , executionIdentity, installedCheckpointMatches
  ) where

import Data.Aeson (Value (..), object, (.=))
import Data.Aeson.Key (Key)
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Pair)
import Data.List (isPrefixOf)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256Hex)

data Role = Stock | Owned deriving (Eq, Show)

-- | The DAT the owned role boots: the stock role's, or a staged DAT carrying owned fixed-range records.
data OwnedBase = SharedBase | StagedBase FilePath Sha256 deriving (Eq)

-- | The archive the owned role links: the qualifier's pinned candidate, or a control archive whose
-- pairs must fail comparison.
data OwnedArchive = PinnedArchive | ControlArchive FilePath Sha256 deriving (Eq)

-- | The pinned archive, trajectory inventory and pairs: the engine225 matrix, or the matrix that adds
-- the moved open-seek route and links the engine owning kind 1 seek 0x17.
data MatrixPin = Engine225Matrix | SeekMovedMatrix deriving (Eq, Show)

-- | Settings every child process of one qualification receives again.
data Qualification = Qualification
  { qualificationRoot :: FilePath
  , qualificationOwnedBase :: OwnedBase
  , qualificationArchive :: OwnedArchive
  , qualificationMatrix :: MatrixPin
  } deriving (Eq)

-- | Leading @--root DIR@, @--owned-base DAT SHA256@, @--control-archive ARCHIVE SHA256@ and
-- @--matrix seek-moved@ options, then the command.
parseInvocation :: FilePath -> [String] -> Either String (Qualification, [String])
parseInvocation defaultRoot = go (Qualification defaultRoot SharedBase PinnedArchive Engine225Matrix)
  where
    go qualification ("--root" : root : rest) = go qualification { qualificationRoot = root } rest
    go qualification ("--owned-base" : path : digest : rest) = case parseSha256Hex digest of
      Right parsed -> go qualification { qualificationOwnedBase = StagedBase path parsed } rest
      Left problem -> Left ("--owned-base digest: " ++ problem)
    go qualification ("--control-archive" : path : digest : rest) = case parseSha256Hex digest of
      Right parsed -> go qualification { qualificationArchive = ControlArchive path parsed } rest
      Left problem -> Left ("--control-archive digest: " ++ problem)
    go qualification ("--matrix" : "seek-moved" : rest) = go qualification { qualificationMatrix = SeekMovedMatrix } rest
    go _ (option : _) | "--" `isPrefixOf` option = Left ("unknown or incomplete option " ++ option)
    go qualification command = Right (qualification, command)

qualificationOptions :: Qualification -> [String]
qualificationOptions qualification = ["--root", qualificationRoot qualification]
  ++ (case qualificationOwnedBase qualification of
        SharedBase -> []
        StagedBase path digest -> ["--owned-base", path, sha256Hex digest])
  ++ (case qualificationArchive qualification of
        PinnedArchive -> []
        ControlArchive path digest -> ["--control-archive", path, sha256Hex digest])
  ++ (case qualificationMatrix qualification of
        Engine225Matrix -> []
        SeekMovedMatrix -> ["--matrix", "seek-moved"])

-- | The DAT a role boots and the digest its receipt must name, given the stock role's.
roleFirmware :: OwnedBase -> (FilePath, Sha256) -> Role -> (FilePath, Sha256)
roleFirmware (StagedBase path digest) _ Owned = (path, digest)
roleFirmware _ stock _ = stock

ownedBaseKey :: Key
ownedBaseKey = "owned_base_firmware"

ownedBaseInputs :: OwnedBase -> [Pair]
ownedBaseInputs SharedBase = []
ownedBaseInputs (StagedBase _ digest) = [ownedBaseKey .= sha256Hex digest]

executionIdentity :: Value -> Value
executionIdentity value = object
  [ "role" .= field value "role"
  , "viewer" .= field tools "viewer"
  , "sdk_store" .= field tools "sdk_store"
  , "sdk_driver" .= field tools "sdk_driver"
  , "sdk_ram_push" .= field tools "sdk_ram_push"
  , "inputs" .= object [(key, field inputs key) | key <- keys]
  , "installed_checkpoint" .= field value "installed_checkpoint"
  , "trajectory" .= field (field value "trajectory") "sha256"
  ]
 where
  tools = field value "tools"
  inputs = field value "inputs"
  owned = field value "role" == String "owned"
  keys = ["run_document", "firmware", "adjustment_nand", "boot_rom"]
    ++ [key | owned, key <- ["owned_archive", "retired_picker"]]
    ++ [ownedBaseKey | owned, field inputs ownedBaseKey /= Null]

installedCheckpointMatches :: Value -> Value -> Bool
installedCheckpointMatches identity observation = case field identity "installed_checkpoint" of
  Null -> True
  checkpoint -> field observation "install_checkpoint_restored" == Bool True
    && field checkpoint "snapshot_sha256" /= Null
    && field checkpoint "installation_sha256" /= Null
    && field checkpoint "snapshot_sha256" == field proof "snapshot_sha256"
    && field checkpoint "installation_sha256" == field proof "record_sha256"
 where
  proof = field observation "install_checkpoint"

field :: Value -> Key -> Value
field (Object fields) key = maybe Null id (KM.lookup key fields)
field _ _ = Null
