{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Channels, selectors and declared orderings of an interface stream, the
-- two ways the declared stream relation can fail, and the tail tolerance each
-- channel was judged under.
module Emulation.InterfaceStream
  ( ChannelName (..)
  , Occurrence (..)
  , Selector (..)
  , matchesSelector
  , ChannelDifference (..)
  , CausalViolation (..)
  , AllowanceBasis (..)
  , ChannelTolerance (..)
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Value (..), withObject, withText)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Text as T

-- | One independent source of boundary events. The order of events on a
-- channel is compared; the order between channels is not.
newtype ChannelName = ChannelName { channelNameText :: T.Text } deriving (Eq, Ord, Show)

-- | Zero-based position of an event within its channel.
newtype Occurrence = Occurrence { occurrenceIndex :: Int } deriving (Eq, Ord, Show)

-- | An event belongs to a channel when every named field is present with
-- that value.
newtype Selector = Selector { selectorFields :: KM.KeyMap Value } deriving (Eq, Show)

matchesSelector :: Selector -> Value -> Bool
matchesSelector (Selector fields) (Object value) =
  all (\(key, expected) -> KM.lookup key value == Just expected) (KM.toList fields)
matchesSelector _ _ = False

instance FromJSON ChannelName where
  parseJSON = withText "channel name" $ \text -> do
    unless (not (T.null (T.strip text))) (fail "channel name must not be empty")
    pure (ChannelName text)

instance FromJSON Occurrence where
  parseJSON value = do
    index <- parseJSON value
    unless (index >= 0) (fail "occurrence must not be negative")
    pure (Occurrence index)

instance FromJSON Selector where
  parseJSON = withObject "channel selector" $ \value -> do
    unless (not (KM.null value)) (fail "channel selector must name at least one event field")
    pure (Selector value)

-- | The first event at which two streams differ, on the first channel that
-- differs. An absent side has no event at that occurrence.
data ChannelDifference = ChannelDifference
  { differenceChannel :: ChannelName
  , differenceOccurrence :: Occurrence
  , differenceChannelStock :: Maybe Value
  , differenceChannelOwned :: Maybe Value
  } deriving (Eq, Show)

-- | A declared causal pair a stream does not satisfy: the observation that
-- matches the pair's second selector has no earlier event matching its first
-- that an earlier second-selector event has not already consumed.
data CausalViolation = CausalViolation
  { violationObservation :: Int
  , violationFirst :: Selector
  , violationThen :: Selector
  , violationReason :: String
  } deriving (Eq, Show)

-- | Where a channel's tail allowance comes from.
data AllowanceBasis
  = NoMilestone
  | OrderedDefault
  | TimeDrivenDefault
  | DeclaredAllowance T.Text
  deriving (Eq, Show)

-- | The number of events by which one side of a channel may exceed the other
-- at the end milestone, why, and each side's count of events on the channel.
data ChannelTolerance = ChannelTolerance
  { toleranceChannel :: ChannelName
  , toleranceTimeDriven :: Bool
  , toleranceAllowance :: Int
  , toleranceBasis :: AllowanceBasis
  , toleranceStockEvents :: Int
  , toleranceOwnedEvents :: Int
  } deriving (Eq, Show)
