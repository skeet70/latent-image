{-# LANGUAGE OverloadedStrings #-}

-- | The data-queue addresses and record offsets @layout.json@ prints from @rtos_layout@.
module Replacement.RtosDataQueue.Layout
  ( Facts (..)
  , DomainFacts (..)
  , ArenaFacts (..)
  , RecordFacts (..)
  , parseFacts
  , domainFacts
  , arenaFor
  , arenaBase
  , arenaBytes
  , dataQueueConfig
  , dataQueueControl
  , dataQueueWait
  , dataQueueTimer
  , task
  , definition
  , contextRecord
  ) where

import Data.Aeson (FromJSON (..), withObject, (.:))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.Word (Word32, Word8)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data DomainFacts = DomainFacts
  { dfId :: Word32
  , dfSchedulerLock :: Word32
  , dfObjectLock :: Word32
  , dfContexts :: Word32
  , dfReadyHeads :: Word32
  , dfReadyBitmap :: Word32
  , dfRunning :: Word32
  , dfDispatchEnabled :: Word32
  , dfTickNow :: Word32
  , dfTimerHeap :: Word32
  , dfTimerCount :: Word32
  , dfTimerBase :: Word32
  , dfSelected :: Word32
  , dfTaskTable :: Word32
  , dfContextA :: Word32
  , dfContextB :: Word32
  , dfHandlerContext :: Word32
  , dfCurrentContext :: Word32
  , dfReschedule :: Word32
  , dfDataQueueConfigs :: Word32
  , dfDataQueueControls :: Word32
  , dfDataQueueWaits :: Word32
  , dfDataQueueTimers :: Word32
  , dfDataQueuePlaceholder :: Word32
  }
  deriving (Eq, Show)

data RecordFacts = RecordFacts
  { rfSize :: Word32
  , rfAttributes :: Word32
  , rfCapacity :: Word32
  , rfRingBase :: Word32
  , rfExists :: Word32
  , rfSenderOrder :: Word32
  , rfControlSize :: Word32
  , rfSenderQueue :: Word32
  , rfCreation :: Word32
  , rfReceivers :: Word32
  , rfCount :: Word32
  , rfRead :: Word32
  , rfWrite :: Word32
  , rfWaitSize :: Word32
  , rfWaitData :: Word32
  }
  deriving (Eq, Show)

data ArenaFacts = ArenaFacts
  { arenaStart :: Word32
  , arenaSize :: Word32
  }
  deriving (Eq, Show)

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
  , factsArenaA :: ArenaFacts
  , factsArenaB :: ArenaFacts
  , factsOwnId :: Word32
  , factsContextSize :: Word32
  , factsContextId :: Word32
  , factsContextOwner :: Word32
  , factsContextSchedulerLock :: Word32
  , factsContextObjectLock :: Word32
  , factsContextReadyHeads :: Word32
  , factsContextReadyBitmap :: Word32
  , factsContextRunning :: Word32
  , factsContextSelected :: Word32
  , factsContextDispatchEnabled :: Word32
  , factsContextTickNow :: Word32
  , factsContextTimerHeap :: Word32
  , factsContextTimerCount :: Word32
  , factsContextTimerBase :: Word32
  , factsContextFirstEntry :: Word32
  , factsContextTasks :: Word32
  , factsContextTaskCount :: Word32
  , factsContextDataQueues :: Word32
  , factsContextDataQueueCount :: Word32
  , factsLockWord :: Word32
  , factsLockAcquire :: Word32
  , factsLockRelease :: Word32
  , factsTaskSize :: Word32
  , factsTaskNext :: Word32
  , factsTaskPrev :: Word32
  , factsTaskDefinition :: Word32
  , factsTaskState :: Word32
  , factsTaskPriority :: Word32
  , factsTaskFlags :: Word32
  , factsTaskWait :: Word32
  , factsTaskSavedSp :: Word32
  , factsTaskResumePc :: Word32
  , factsTaskDomain :: Word32
  , factsTaskTerminatePending :: Word32
  , factsTaskPriorityChangePending :: Word32
  , factsTaskCancelPending :: Word32
  , factsTaskContextDiscarded :: Word32
  , factsTaskPendingPriority :: Word32
  , factsDefinitionPriority :: Word32
  , factsDefinitionSpTerms :: (Word32, Word32)
  , factsHeadSize :: Word32
  , factsHeadNext :: Word32
  , factsHeadPrev :: Word32
  , factsPriorities :: Word32
  , factsTaskSlots :: Word32
  , factsRecords :: RecordFacts
  , factsTimerEntrySize :: Word32
  , factsStateDormant :: Word8
  , factsStateReady :: Word8
  , factsStateWait :: Word8
  , factsStateSuspended :: Word8
  , factsStateSleeping :: Word8
  , factsStateObjectQueued :: Word8
  , factsStateOwnerLockQueued :: Word8
  }
  deriving (Eq, Show)

instance FromJSON DomainFacts where
  parseJSON = withObject "domain facts" $ \o ->
    DomainFacts
      <$> o .: "id"
      <*> o .: "scheduler_lock"
      <*> o .: "object_lock"
      <*> o .: "contexts"
      <*> o .: "ready_heads"
      <*> o .: "ready_bitmap"
      <*> o .: "running"
      <*> o .: "dispatch_enabled"
      <*> o .: "tick_now"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "selected"
      <*> o .: "task_table"
      <*> o .: "context_a"
      <*> o .: "context_b"
      <*> o .: "handler_context"
      <*> o .: "current_context"
      <*> o .: "reschedule"
      <*> o .: "data_queue_configs"
      <*> o .: "data_queue_controls"
      <*> o .: "data_queue_waits"
      <*> o .: "data_queue_timers"
      <*> o .: "data_queue_placeholder"

instance FromJSON RecordFacts where
  parseJSON = withObject "record facts" $ \o -> do
    c <- o .: "data_queue_config"
    t <- o .: "data_queue_control"
    w <- o .: "data_queue_wait"
    RecordFacts
      <$> c .: "size"
      <*> c .: "attributes"
      <*> c .: "capacity"
      <*> c .: "ring_base"
      <*> c .: "exists"
      <*> c .: "sender_priority_order"
      <*> t .: "size"
      <*> t .: "sender_queue"
      <*> t .: "creation"
      <*> t .: "receivers"
      <*> t .: "count"
      <*> t .: "read"
      <*> t .: "write"
      <*> w .: "size"
      <*> w .: "data"

instance FromJSON Facts where
  parseJSON = withObject "facts" $ \o -> do
    c <- o .: "context"
    l <- o .: "lock"
    t <- o .: "task"
    d <- o .: "definition"
    h <- o .: "ready_head"
    states <- o .: "task_states"
    (dormant, ready, wait, suspended, sleeping, objectQueued, ownerQueued) <- case states of
      [a, b, c', d', e, g, i] -> pure (a, b, c', d', e, g, i)
      _ -> fail "task_states needs seven values"
    spTerms <- d .: "sp_terms"
    terms <- case spTerms of
      [first, second] -> pure (first, second)
      _ -> fail "sp_terms needs two offsets"
    rec <- parseRecords o
    arenas <- o .: "timer_arenas"
    (arenaA, arenaB) <- parseArenas arenas
    Facts
      <$> o .: "a"
      <*> o .: "b"
      <*> pure arenaA
      <*> pure arenaB
      <*> o .: "own_id"
      <*> c .: "size"
      <*> c .: "id"
      <*> c .: "owner"
      <*> c .: "scheduler_lock"
      <*> c .: "object_lock"
      <*> c .: "ready_heads"
      <*> c .: "ready_bitmap"
      <*> c .: "running"
      <*> c .: "selected"
      <*> c .: "dispatch_enabled"
      <*> c .: "tick_now"
      <*> c .: "timer_heap"
      <*> c .: "timer_count"
      <*> c .: "timer_base"
      <*> c .: "first_entry"
      <*> c .: "tasks"
      <*> c .: "task_count"
      <*> c .: "data_queues"
      <*> c .: "data_queue_count"
      <*> l .: "word"
      <*> l .: "acquire"
      <*> l .: "release"
      <*> t .: "size"
      <*> t .: "next"
      <*> t .: "prev"
      <*> t .: "definition"
      <*> t .: "state"
      <*> t .: "priority"
      <*> t .: "flags"
      <*> t .: "wait"
      <*> t .: "saved_sp"
      <*> t .: "resume_pc"
      <*> t .: "domain"
      <*> t .: "terminate_pending"
      <*> t .: "priority_change_pending"
      <*> t .: "cancel_pending"
      <*> t .: "context_discarded"
      <*> t .: "pending_priority"
      <*> d .: "priority"
      <*> pure terms
      <*> h .: "size"
      <*> h .: "next"
      <*> h .: "prev"
      <*> o .: "priorities"
      <*> o .: "task_slots"
      <*> pure rec
      <*> o .: "timer_entry_size"
      <*> pure dormant
      <*> pure ready
      <*> pure wait
      <*> pure suspended
      <*> pure sleeping
      <*> pure objectQueued
      <*> pure ownerQueued

parseRecords :: J.Object -> Parser RecordFacts
parseRecords o = do
  c <- o .: "data_queue_config"
  t <- o .: "data_queue_control"
  w <- o .: "data_queue_wait"
  RecordFacts
    <$> c .: "size"
    <*> c .: "attributes"
    <*> c .: "capacity"
    <*> c .: "ring_base"
    <*> c .: "exists"
    <*> c .: "sender_priority_order"
    <*> t .: "size"
    <*> t .: "sender_queue"
    <*> t .: "creation"
    <*> t .: "receivers"
    <*> t .: "count"
    <*> t .: "read"
    <*> t .: "write"
    <*> w .: "size"
    <*> w .: "data"

parseArenas :: J.Value -> Parser (ArenaFacts, ArenaFacts)
parseArenas = withObject "timer arenas" $ \o -> do
  a <- o .: "a"
  b <- o .: "b"
  pure (a, b)

instance FromJSON ArenaFacts where
  parseJSON = withObject "timer arena" $ \o -> do
    guest <- o .: "guest"
    ArenaFacts
      <$> guest .: "start"
      <*> guest .: "size"

parseFacts :: B.ByteString -> Either String Facts
parseFacts = J.eitherDecodeStrict

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts domain = case domain of
  DomainA -> factsA facts
  DomainB -> factsB facts

arenaFor :: Facts -> Domain -> ArenaFacts
arenaFor facts domain = case domain of
  DomainA -> factsArenaA facts
  DomainB -> factsArenaB facts

arenaBase :: Facts -> Domain -> Word32
arenaBase facts = arenaStart . arenaFor facts

arenaBytes :: Facts -> Domain -> Word32
arenaBytes facts = arenaSize . arenaFor facts

dataQueueConfig :: Facts -> Domain -> Word32 -> Word32
dataQueueConfig facts domain slot =
  dfDataQueueConfigs (domainFacts facts domain) + (slot - 1) * rfSize (factsRecords facts)

dataQueueControl :: Facts -> Domain -> Word32 -> Word32
dataQueueControl facts domain slot =
  dfDataQueueControls (domainFacts facts domain) + (slot - 1) * rfControlSize (factsRecords facts)

dataQueueWait :: Facts -> Domain -> Word32 -> Word32
dataQueueWait facts domain slot =
  dfDataQueueWaits (domainFacts facts domain) + (slot - 1) * rfWaitSize (factsRecords facts)

dataQueueTimer :: Facts -> Domain -> Word32 -> Word32
dataQueueTimer facts domain slot =
  dfDataQueueTimers (domainFacts facts domain) + (slot - 1) * factsTimerEntrySize facts

task :: Facts -> Domain -> Word32 -> Word32
task facts domain slot = dfTaskTable (domainFacts facts domain) + (slot - 1) * factsTaskSize facts

definition :: Facts -> Domain -> Word32 -> Word32
definition facts domain slot = 0x0096c000 + (if domain == DomainA then 0 else factsTaskSlots facts) * 0x40 + (slot - 1) * 0x40

contextRecord :: Facts -> Domain -> Domain -> Word32
contextRecord facts block described = case described of
  DomainA -> dfContextA (domainFacts facts block)
  DomainB -> dfContextB (domainFacts facts block)
