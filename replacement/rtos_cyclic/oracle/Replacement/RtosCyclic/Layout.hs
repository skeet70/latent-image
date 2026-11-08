{-# LANGUAGE OverloadedStrings #-}

module Replacement.RtosCyclic.Layout
  ( DomainFacts (..)
  , ContextOffsets (..)
  , LockOffsets (..)
  , CountWords (..)
  , Facts (..)
  , parseFacts
  , domainFacts
  , countWord
  ) where

import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data DomainFacts = DomainFacts
  { dfHandlerContext :: Word32
  , dfCurrentContext :: Word32
  , dfSchedulerLock :: Word32
  , dfContexts :: Word32
  , dfTimerHeap :: Word32
  , dfTimerCount :: Word32
  , dfTimerBase :: Word32
  , dfTickNow :: Word32
  , dfCyclicConfigs :: Word32
  , dfCyclicControls :: Word32
  }
  deriving (Eq, Show)

data CountWords = CountWords
  { countA :: Word32
  , countB :: Word32
  }
  deriving (Eq, Show)

data ContextOffsets = ContextOffsets
  { contextSchedulerLock :: Word32
  , contextTimerHeap :: Word32
  , contextTimerCount :: Word32
  , contextTimerBase :: Word32
  , contextTickNow :: Word32
  , contextCyclics :: Word32
  , contextCyclicCount :: Word32
  , contextCyclicCallback :: Word32
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
  , factsCounts :: CountWords
  , factsContext :: ContextOffsets
  , factsLock :: LockOffsets
  , factsCyclicConfigSize :: Word32
  , factsCyclicControlSize :: Word32
  , factsCyclicSlots :: Word32
  , factsTimerEntrySize :: Word32
  }
  deriving (Eq, Show)

instance FromJSON DomainFacts where
  parseJSON = withObject "domain" $ \o ->
    DomainFacts
      <$> o .: "handler_context"
      <*> o .: "current_context"
      <*> o .: "scheduler_lock"
      <*> o .: "contexts"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"
      <*> o .: "cyclic_configs"
      <*> o .: "cyclic_controls"

instance FromJSON CountWords where
  parseJSON = withObject "count" $ \o ->
    CountWords <$> o .: "a" <*> o .: "b"

instance FromJSON ContextOffsets where
  parseJSON = withObject "context" $ \o ->
    ContextOffsets
      <$> o .: "scheduler_lock"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"
      <*> o .: "cyclics"
      <*> o .: "cyclic_count"
      <*> o .: "cyclic_callback"

instance FromJSON LockOffsets where
  parseJSON = withObject "lock" $ \o ->
    LockOffsets
      <$> o .: "word"
      <*> o .: "acquire"
      <*> o .: "release"

instance FromJSON Facts where
  parseJSON = withObject "cyclic layout" $ \o ->
    Facts
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "cyclic_count"
      <*> o .: "context"
      <*> o .: "lock"
      <*> o .: "cyclic_config_size"
      <*> o .: "cyclic_control_size"
      <*> o .: "cyclic_slots"
      <*> o .: "timer_entry_size"

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts

countWord :: Facts -> Domain -> Word32
countWord facts DomainA = countA (factsCounts facts)
countWord facts DomainB = countB (factsCounts facts)
