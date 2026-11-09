{-# LANGUAGE OverloadedStrings #-}

-- | The timer heap's stock words and record offsets, as the oracle's layout.json prints them.
module Replacement.RtosTimer.Layout
  ( DomainFacts (..)
  , ContextOffsets (..)
  , LockOffsets (..)
  , SpanFacts (..)
  , ArenaFacts (..)
  , TaskPoolFacts (..)
  , ReservationFacts (..)
  , Facts (..)
  , SchedulerDomain (..)
  , SchedulerContext (..)
  , SchedulerTask (..)
  , SchedulerDefinition (..)
  , SchedulerReadyHead (..)
  , SchedulerStates (..)
  , SchedulerHelpers (..)
  , SchedulerFacts (..)
  , parseFacts
  , domainFacts
  , arenaFor
  , sfDomain
  , sfHelpers
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

data SpanFacts = SpanFacts
  { spanStart :: Word32
  , spanEnd :: Word32
  , spanSize :: Word32
  }
  deriving (Eq, Show)

data ArenaFacts = ArenaFacts
  { arenaGuest :: SpanFacts
  , arenaPhysical :: SpanFacts
  , arenaCapacity :: Word32
  , arenaNodeBytes :: Word32
  }
  deriving (Eq, Show)

data TaskPoolFacts = TaskPoolFacts
  { poolGuest :: SpanFacts
  , poolPhysical :: SpanFacts
  , poolAllocatable :: SpanFacts
  , poolAllocatablePhysical :: SpanFacts
  , poolReserved :: SpanFacts
  }
  deriving (Eq, Show)

data ReservationFacts = ReservationFacts
  { reservationTaskPool :: TaskPoolFacts
  , reservationArenaA :: ArenaFacts
  , reservationArenaB :: ArenaFacts
  , reservationNodeBytes :: Word32
  }
  deriving (Eq, Show)

data SchedulerDomain = SchedulerDomain
  { swContexts :: Word32
  , swCurrentContext :: Word32
  , swHandlerContext :: Word32
  , swSchedulerLock :: Word32
  , swObjectLock :: Word32
  , swReadyHeads :: Word32
  , swReadyBitmap :: Word32
  , swRunning :: Word32
  , swDispatchEnabled :: Word32
  , swSelected :: Word32
  , swReschedule :: Word32
  , swTimerHeap :: Word32
  , swTimerCount :: Word32
  , swTimerBase :: Word32
  , swTickNow :: Word32
  , swTaskTable :: Word32
  , swTaskDefinitions :: Word32
  }
  deriving (Eq, Show)

data SchedulerContext = SchedulerContext
  { cxSize :: Word32
  , cxId :: Word32
  , cxOwner :: Word32
  , cxSchedulerLock :: Word32
  , cxObjectLock :: Word32
  , cxReadyHeads :: Word32
  , cxReadyBitmap :: Word32
  , cxRunning :: Word32
  , cxSelected :: Word32
  , cxDispatchEnabled :: Word32
  , cxTimerHeap :: Word32
  , cxTimerCount :: Word32
  , cxTimerBase :: Word32
  , cxTickNow :: Word32
  , cxTasks :: Word32
  , cxTaskCount :: Word32
  , cxFirstEntry :: Word32
  }
  deriving (Eq, Show)

data SchedulerTask = SchedulerTask
  { tkSize :: Word32
  , tkNext :: Word32
  , tkPrev :: Word32
  , tkDefinition :: Word32
  , tkState :: Word32
  , tkPriority :: Word32
  , tkFlags :: Word32
  , tkWait :: Word32
  , tkSavedSp :: Word32
  , tkResumePc :: Word32
  , tkDomain :: Word32
  , tkTerminatePending :: Word32
  , tkPriorityChangePending :: Word32
  , tkCancelPending :: Word32
  , tkContextDiscarded :: Word32
  , tkPendingPriority :: Word32
  }
  deriving (Eq, Show)

data SchedulerDefinition = SchedulerDefinition
  { dfSize :: Word32
  , dfPriority :: Word32
  , dfExists :: Word32
  , dfSpTerms :: (Word32, Word32)
  }
  deriving (Eq, Show)

data SchedulerReadyHead = SchedulerReadyHead
  { rhSize :: Word32
  , rhNext :: Word32
  , rhPrev :: Word32
  }
  deriving (Eq, Show)

data SchedulerStates = SchedulerStates
  { stateDormant :: Word32
  , stateReady :: Word32
  , stateWait :: Word32
  , stateSuspended :: Word32
  , stateSleeping :: Word32
  , stateObjectQueued :: Word32
  , stateOwnerLockQueued :: Word32
  }
  deriving (Eq, Show)

data SchedulerHelpers = SchedulerHelpers
  { helpAcquire :: Word32
  , helpRelease :: Word32
  , helpSwitch :: Word32
  , helpFirstEntry :: Word32
  , helpNotify :: Word32
  , helpInsertReady :: Word32
  , helpRemoveReady :: Word32
  , helpResetTask :: Word32
  , helpRestartTask :: Word32
  }
  deriving (Eq, Show)

data SchedulerFacts = SchedulerFacts
  { sfA :: SchedulerDomain
  , sfB :: SchedulerDomain
  , sfContext :: SchedulerContext
  , sfTask :: SchedulerTask
  , sfDefinition :: SchedulerDefinition
  , sfLock :: LockOffsets
  , sfReadyHead :: SchedulerReadyHead
  , sfStates :: SchedulerStates
  , sfPriorities :: Word32
  , sfTaskSlots :: Word32
  , sfHelpersA :: SchedulerHelpers
  , sfHelpersB :: SchedulerHelpers
  }
  deriving (Eq, Show)

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
  , factsContext :: ContextOffsets
  , factsLock :: LockOffsets
  , factsTimerEntrySize :: Word32
  , factsTaskSlots :: Word32
  , factsScheduler :: SchedulerFacts
  , factsReservation :: ReservationFacts
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

instance FromJSON SpanFacts where
  parseJSON = withObject "span" $ \o ->
    SpanFacts <$> o .: "start" <*> o .: "end" <*> o .: "size"

instance FromJSON ArenaFacts where
  parseJSON = withObject "arena" $ \o ->
    ArenaFacts
      <$> o .: "guest"
      <*> o .: "physical"
      <*> o .: "capacity"
      <*> o .: "node_bytes"

instance FromJSON TaskPoolFacts where
  parseJSON = withObject "task pool" $ \o ->
    TaskPoolFacts
      <$> o .: "guest"
      <*> o .: "physical"
      <*> o .: "allocatable"
      <*> o .: "allocatable_physical"
      <*> o .: "reserved"

instance FromJSON ReservationFacts where
  parseJSON = withObject "reservation" $ \o -> do
    pool <- o .: "task_pool"
    arenas <- o .: "timer_arenas"
    a <- withObject "timer arenas" (.: "a") arenas
    b <- withObject "timer arenas" (.: "b") arenas
    nodeBytes <- o .: "timer_node_bytes"
    pure (ReservationFacts pool a b nodeBytes)

instance FromJSON SchedulerDomain where
  parseJSON = withObject "scheduler domain" $ \o ->
    SchedulerDomain
      <$> o .: "contexts"
      <*> o .: "current_context"
      <*> o .: "handler_context"
      <*> o .: "scheduler_lock"
      <*> o .: "object_lock"
      <*> o .: "ready_heads"
      <*> o .: "ready_bitmap"
      <*> o .: "running"
      <*> o .: "dispatch_enabled"
      <*> o .: "selected"
      <*> o .: "reschedule"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"
      <*> o .: "task_table"
      <*> o .: "task_definitions"

instance FromJSON SchedulerContext where
  parseJSON = withObject "scheduler context" $ \o ->
    SchedulerContext
      <$> o .: "size"
      <*> o .: "id"
      <*> o .: "owner"
      <*> o .: "scheduler_lock"
      <*> o .: "object_lock"
      <*> o .: "ready_heads"
      <*> o .: "ready_bitmap"
      <*> o .: "running"
      <*> o .: "selected"
      <*> o .: "dispatch_enabled"
      <*> o .: "timer_heap"
      <*> o .: "timer_count"
      <*> o .: "timer_base"
      <*> o .: "tick_now"
      <*> o .: "tasks"
      <*> o .: "task_count"
      <*> o .: "first_entry"

instance FromJSON SchedulerTask where
  parseJSON = withObject "scheduler task" $ \o ->
    SchedulerTask
      <$> o .: "size"
      <*> o .: "next"
      <*> o .: "prev"
      <*> o .: "definition"
      <*> o .: "state"
      <*> o .: "priority"
      <*> o .: "flags"
      <*> o .: "wait"
      <*> o .: "saved_sp"
      <*> o .: "resume_pc"
      <*> o .: "domain"
      <*> o .: "terminate_pending"
      <*> o .: "priority_change_pending"
      <*> o .: "cancel_pending"
      <*> o .: "context_discarded"
      <*> o .: "pending_priority"

instance FromJSON SchedulerDefinition where
  parseJSON = withObject "scheduler definition" $ \o -> do
    terms <- o .: "sp_terms"
    case terms of
      [first, second] -> SchedulerDefinition <$> o .: "size" <*> o .: "priority" <*> o .: "exists" <*> pure (first, second)
      _ -> fail "sp_terms needs two offsets"

instance FromJSON SchedulerReadyHead where
  parseJSON = withObject "scheduler ready head" $ \o ->
    SchedulerReadyHead <$> o .: "size" <*> o .: "next" <*> o .: "prev"

instance FromJSON SchedulerStates where
  parseJSON value = do
    states <- parseJSON value
    case states of
      [dormant, ready, wait, suspended, sleeping, objectQueued, ownerQueued] ->
        pure (SchedulerStates dormant ready wait suspended sleeping objectQueued ownerQueued)
      _ -> fail "states needs seven values"

instance FromJSON SchedulerHelpers where
  parseJSON = withObject "scheduler helpers" $ \o ->
    SchedulerHelpers
      <$> o .: "acquire"
      <*> o .: "release"
      <*> o .: "switch"
      <*> o .: "first_entry"
      <*> o .: "notify"
      <*> o .: "insert_ready"
      <*> o .: "remove_ready"
      <*> o .: "reset_task"
      <*> o .: "restart_task"

instance FromJSON SchedulerFacts where
  parseJSON = withObject "scheduler" $ \o -> do
    helpers <- o .: "helpers"
    SchedulerFacts
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "context"
      <*> o .: "task"
      <*> o .: "definition"
      <*> o .: "lock"
      <*> o .: "ready_head"
      <*> o .: "states"
      <*> o .: "priorities"
      <*> o .: "task_slots"
      <*> withObject "helpers" (.: "a") helpers
      <*> withObject "helpers" (.: "b") helpers

instance FromJSON Facts where
  parseJSON = withObject "timer layout" $ \o ->
    Facts
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "context"
      <*> o .: "lock"
      <*> o .: "timer_entry_size"
      <*> o .: "task_slots"
      <*> o .: "scheduler"
      <*> o .: "reservation"

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts

arenaFor :: Facts -> Domain -> ArenaFacts
arenaFor facts DomainA = reservationArenaA (factsReservation facts)
arenaFor facts DomainB = reservationArenaB (factsReservation facts)

sfDomain :: SchedulerFacts -> Domain -> SchedulerDomain
sfDomain facts DomainA = sfA facts
sfDomain facts DomainB = sfB facts

sfHelpers :: SchedulerFacts -> Domain -> SchedulerHelpers
sfHelpers facts DomainA = sfHelpersA facts
sfHelpers facts DomainB = sfHelpersB facts
