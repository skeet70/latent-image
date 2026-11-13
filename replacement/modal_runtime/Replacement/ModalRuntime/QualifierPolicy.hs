{-# LANGUAGE OverloadedStrings #-}
module Replacement.ModalRuntime.QualifierPolicy (executionIdentity, installedCheckpointMatches) where

import Data.Aeson (Value (..), object, (.=))
import Data.Aeson.Key (Key)
import qualified Data.Aeson.KeyMap as KM

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
  keys = ["run_document", "firmware", "adjustment_nand", "boot_rom"]
    ++ [key | field value "role" == String "owned", key <- ["owned_archive", "retired_picker"]]

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
