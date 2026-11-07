{-# LANGUAGE OverloadedStrings #-}

-- | The F2 addresses and record offsets, as @replacement/rtos_scheduler/oracle:layout.json@
-- prints them from @rtos_layout@.
module Replacement.RtosObjects.Layout
  ( Facts (..)
  , DomainFacts (..)
  , parseFacts
  , domainFacts
  , record
  , flagConfig
  , flagControl
  , flagWait
  , mailboxConfig
  , mailboxControl
  , mailboxWait
  , task
  , readyHead
  ) where

import Data.Aeson (FromJSON (..), withObject, (.:))
import qualified Data.Aeson as J
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
  , dfSelected :: Word32
  , dfTaskTable :: Word32
  , dfContextA :: Word32
  , dfContextB :: Word32
  , dfHandlerContext :: Word32
  , dfReschedule :: Word32
  , dfTimerBase :: Word32
  , dfTimerCount :: Word32
  , dfTimerHeap :: Word32
  , dfTickNow :: Word32
  , dfFlagConfigs :: Word32
  , dfFlagControls :: Word32
  , dfFlagWaits :: Word32
  , dfFlagTimers :: Word32
  , dfMailboxConfigs :: Word32
  , dfMailboxControls :: Word32
  , dfMailboxWaits :: Word32
  , dfMailboxTimers :: Word32
  }
  deriving (Eq, Show)

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
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
  , factsHeadSize :: Word32
  , factsHeadNext :: Word32
  , factsHeadPrev :: Word32
  , factsPriorities :: Word32
  , factsTaskSlots :: Word32
  , factsFlagConfigSize :: Word32
  , factsFlagConfigAttributes :: Word32
  , factsFlagConfigInitial :: Word32
  , factsFlagConfigExists :: Word32
  , factsFlagPriorityOrder :: Word32
  , factsFlagClearOnMatch :: Word32
  , factsMailboxConfigSize :: Word32
  , factsMailboxConfigAttributes :: Word32
  , factsMailboxConfigPriorityLimit :: Word32
  , factsMailboxConfigExists :: Word32
  , factsMailboxPriorityOrder :: Word32
  , factsMailboxMessagePriority :: Word32
  , factsWaitBlockSize :: Word32
  , factsWaitBlockResult :: Word32
  , factsWaitBlockQueue :: Word32
  , factsWaitBlockOwner :: Word32
  , factsWaitQueueSize :: Word32
  , factsWaitQueueNext :: Word32
  , factsWaitQueuePrev :: Word32
  , factsWaitQueueConfig :: Word32
  , factsFlagWaitSize :: Word32
  , factsFlagWaitPattern :: Word32
  , factsFlagWaitMode :: Word32
  , factsFlagWaitResult :: Word32
  , factsMailboxWaitSize :: Word32
  , factsMailboxWaitMessage :: Word32
  , factsFlagControlSize :: Word32
  , factsFlagControlQueue :: Word32
  , factsFlagControlPattern :: Word32
  , factsMailboxControlSize :: Word32
  , factsMailboxControlQueue :: Word32
  , factsMailboxControlHead :: Word32
  , factsMailboxControlTail :: Word32
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
      <*> o .: "selected"
      <*> o .: "task_table"
      <*> o .: "context_a"
      <*> o .: "context_b"
      <*> o .: "handler_context"
      <*> o .: "reschedule"
      <*> o .: "timer_base"
      <*> o .: "timer_count"
      <*> o .: "timer_heap"
      <*> o .: "tick_now"
      <*> o .: "flag_configs"
      <*> o .: "flag_controls"
      <*> o .: "flag_waits"
      <*> o .: "flag_timers"
      <*> o .: "mailbox_configs"
      <*> o .: "mailbox_controls"
      <*> o .: "mailbox_waits"
      <*> o .: "mailbox_timers"

instance FromJSON Facts where
  parseJSON = withObject "facts" $ \o -> do
    c <- o .: "context"
    l <- o .: "lock"
    t <- o .: "task"
    d <- o .: "definition"
    h <- o .: "ready_head"
    f <- o .: "flag_config"
    m <- o .: "mailbox_config"
    wb <- o .: "wait_block"
    wq <- o .: "wait_queue"
    fw <- o .: "flag_wait"
    mw <- o .: "mailbox_wait"
    fc <- o .: "flag_control"
    mc <- o .: "mailbox_control"
    states <- o .: "task_states"
    (_dormant, ready, wait, suspended, sleeping, objectQueued, ownerQueued) <- case states of
      [a, b, c', d', e, g, i] -> pure (a, b, c', d', e, g, i)
      _ -> fail "task_states needs seven values"
    Facts
      <$> o .: "a"
      <*> o .: "b"
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
      <*> h .: "size"
      <*> h .: "next"
      <*> h .: "prev"
      <*> o .: "priorities"
      <*> o .: "task_slots"
      <*> f .: "size"
      <*> f .: "attributes"
      <*> f .: "initial"
      <*> f .: "exists"
      <*> f .: "priority_order"
      <*> f .: "clear_on_match"
      <*> m .: "size"
      <*> m .: "attributes"
      <*> m .: "priority_limit"
      <*> m .: "exists"
      <*> m .: "priority_order"
      <*> m .: "message_priority"
      <*> wb .: "size"
      <*> wb .: "result"
      <*> wb .: "queue"
      <*> wb .: "owner"
      <*> wq .: "size"
      <*> wq .: "next"
      <*> wq .: "prev"
      <*> wq .: "config"
      <*> fw .: "size"
      <*> fw .: "pattern"
      <*> fw .: "mode"
      <*> fw .: "result"
      <*> mw .: "size"
      <*> mw .: "message"
      <*> fc .: "size"
      <*> fc .: "queue"
      <*> fc .: "pattern"
      <*> mc .: "size"
      <*> mc .: "queue"
      <*> mc .: "head"
      <*> mc .: "tail"
      <*> o .: "timer_entry_size"
      <*> pure _dormant
      <*> pure ready
      <*> pure wait
      <*> pure suspended
      <*> pure sleeping
      <*> pure objectQueued
      <*> pure ownerQueued

parseFacts :: B.ByteString -> Either String Facts
parseFacts = J.eitherDecodeStrict

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts domain = case domain of
  DomainA -> factsA facts
  DomainB -> factsB facts

record :: Facts -> Domain -> Word32
record facts domain = dfContexts (domainFacts facts domain) + factsContextSize facts * index
  where
    index = case domain of
      DomainA -> 0
      DomainB -> 1

flagConfig :: Facts -> Domain -> Word32 -> Word32
flagConfig facts domain slot = dfFlagConfigs (domainFacts facts domain) + (slot - 1) * factsFlagConfigSize facts

flagControl :: Facts -> Domain -> Word32 -> Word32
flagControl facts domain slot = dfFlagControls (domainFacts facts domain) + (slot - 1) * factsFlagControlSize facts

flagWait :: Facts -> Domain -> Word32 -> Word32
flagWait facts domain slot = dfFlagWaits (domainFacts facts domain) + (slot - 1) * factsFlagWaitSize facts

mailboxConfig :: Facts -> Domain -> Word32 -> Word32
mailboxConfig facts domain slot = dfMailboxConfigs (domainFacts facts domain) + (slot - 1) * factsMailboxConfigSize facts

mailboxControl :: Facts -> Domain -> Word32 -> Word32
mailboxControl facts domain slot = dfMailboxControls (domainFacts facts domain) + (slot - 1) * factsMailboxControlSize facts

mailboxWait :: Facts -> Domain -> Word32 -> Word32
mailboxWait facts domain slot = dfMailboxWaits (domainFacts facts domain) + (slot - 1) * factsMailboxWaitSize facts

task :: Facts -> Domain -> Word32 -> Word32
task facts domain slot = dfTaskTable (domainFacts facts domain) + (slot - 1) * factsTaskSize facts

readyHead :: Facts -> Domain -> Word32 -> Word32
readyHead facts domain priority = dfReadyHeads (domainFacts facts domain) + priority * factsHeadSize facts
