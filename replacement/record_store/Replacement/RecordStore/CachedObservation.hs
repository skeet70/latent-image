{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.CachedObservation
  ( CachedObservationError(..)
  , observeCachedStock
  ) where

import Control.Monad (unless)
import Data.Aeson (Value(..))
import qualified Data.Text as T
import Emulation.NativeCase (Region)
import Emulation.NativeObservation
import Emulation.Sha256 (Sha256, parseSha256Hex)
import FujiAF.Host (get)

data CachedObservationError
  = MissingRunnerIdentity
  | MalformedRunnerIdentity
  | RunnerIdentityMismatch
  | InvalidObservation ObservationError
  deriving (Eq, Show)

observeCachedStock :: Sha256 -> [Region] -> Value -> Either CachedObservationError NativeObservation
observeCachedStock executingRunner regions report = do
  identity <- case get report "runner_sha256" of
    Null -> Left MissingRunnerIdentity
    String encoded -> either (const (Left MalformedRunnerIdentity)) Right (parseSha256Hex (T.unpack encoded))
    _ -> Left MalformedRunnerIdentity
  unless (identity == executingRunner) (Left RunnerIdentityMismatch)
  exit <- either (Left . InvalidObservation) Right (reportRunnerExit report)
  either (Left . InvalidObservation) Right (observeNative exit regions (get report "runner"))
