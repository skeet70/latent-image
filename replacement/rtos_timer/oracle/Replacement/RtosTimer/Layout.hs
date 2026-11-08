{-# LANGUAGE OverloadedStrings #-}

-- | The timer heap's stock words and record offsets, as the oracle's layout.json prints them.
module Replacement.RtosTimer.Layout
  ( DomainFacts (..)
  , ContextOffsets (..)
  , LockOffsets (..)
  , Facts (..)
  , parseFacts
  , domainFacts
  ) where

import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data DomainFacts = DomainFacts
  { dfHandlerContext :: Word32
  , dfCurrentContext :: Word32
  , dfSchedulerLock :: Word32
  , dfTimerHeap :: Word32
  , dfTimerCount :: Word32
  , dfTimerBase :: Word32
  , dfTickNow :: Word32
  }
  deriving (Eq, Show)

data ContextOffsets = ContextOffsets
  { contextSchedulerLock :: Word32
  , contextTimerHeap :: Word32
  , contextTimerCount :: Word32
  , contextTimerBase :: Word32
  , contextTickNow :: Word32
  }
  deriving (Eq, Show)

data LockOffsets = LockOffsets
  { lockWord :: Word32
  , lockAcquire :: Word32
  , lockRelease :: Word32
  }
  deriving (Eq, Show)

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
  , factsContext :: ContextOffsets
  , factsLock :: LockOffsets
  , factsTimerEntrySize :: Word32
  , factsTaskSlots :: Word32
  }
  deriving (Eq, Show)

instance FromJSON DomainFacts where
  parseJSON = withObject "domain" $ \o ->
    DomainFacts
      <$> o .: "handler_context"
      <*> o .: "current_context"
      <*> o .: "scheduler_lock"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"

instance FromJSON ContextOffsets where
  parseJSON = withObject "context" $ \o ->
    ContextOffsets
      <$> o .: "scheduler_lock"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"

instance FromJSON LockOffsets where
  parseJSON = withObject "lock" $ \o ->
    LockOffsets
      <$> o .: "word"
      <*> o .: "acquire"
      <*> o .: "release"

instance FromJSON Facts where
  parseJSON = withObject "timer layout" $ \o ->
    Facts
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "context"
      <*> o .: "lock"
      <*> o .: "timer_entry_size"
      <*> o .: "task_slots"

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts
