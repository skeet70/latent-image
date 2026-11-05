{-# LANGUAGE OverloadedStrings #-}

-- | The stock data addresses and record offsets of @replacement/rtos_layout@, as its
-- @layout-facts@ program prints them.
module Replacement.RtosScheduler.Layout
  ( DomainWords (..)
  , ContextOffsets (..)
  , LockOffsets (..)
  , TaskOffsets (..)
  , DefinitionOffsets (..)
  , TaskStates (..)
  , Layout (..)
  , domainWords
  , parseLayout
  ) where

import Data.Aeson (FromJSON (..), withObject, (.:))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.Word (Word32, Word8)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data DomainWords = DomainWords
  { -- | Context record +0x04 and T+0x20.
    wordsId :: Word32
  , wordsSchedulerLock :: Word32
  , wordsObjectLock :: Word32
  , wordsContexts :: Word32
  , wordsReadyHeads :: Word32
  , wordsReadyBitmap :: Word32
  , wordsRunning :: Word32
  , wordsDispatchEnabled :: Word32
  , wordsSelected :: Word32
  , wordsTaskTable :: Word32
  , wordsContextA :: Word32
  , wordsContextB :: Word32
  }
  deriving (Eq, Show)

data ContextOffsets = ContextOffsets
  { contextSize :: Word32
  , contextId :: Word32
  , contextOwner :: Word32
  , contextSchedulerLock :: Word32
  , contextObjectLock :: Word32
  , contextReadyHeads :: Word32
  , contextReadyBitmap :: Word32
  , contextRunning :: Word32
  , contextSelected :: Word32
  , contextDispatchEnabled :: Word32
  , contextFirstEntry :: Word32
  }
  deriving (Eq, Show)

data LockOffsets = LockOffsets
  { lockWord :: Word32
  , lockAcquire :: Word32
  , lockRelease :: Word32
  }
  deriving (Eq, Show)

data TaskOffsets = TaskOffsets
  { taskSize :: Word32
  , taskNext :: Word32
  , taskPrev :: Word32
  , taskDefinition :: Word32
  , taskState :: Word32
  , taskPriority :: Word32
  , taskFlags :: Word32
  , taskExceptionPattern :: Word32
  , taskSavedSp :: Word32
  , taskResumePc :: Word32
  , taskDomain :: Word32
  , taskTerminatePending :: Word32
  , taskPriorityChangePending :: Word32
  , taskCancelPending :: Word32
  }
  deriving (Eq, Show)

data DefinitionOffsets = DefinitionOffsets
  { definitionPriority :: Word32
  , definitionSpTerms :: (Word32, Word32)
  }
  deriving (Eq, Show)

-- | The T+0x0c bits.
data TaskStates = TaskStates
  { stateDormant :: Word8
  , stateReady :: Word8
  , stateWait :: Word8
  , stateSuspended :: Word8
  , stateSleeping :: Word8
  , stateObjectQueued :: Word8
  , stateOwnerLockQueued :: Word8
  }
  deriving (Eq, Show)

data Layout = Layout
  { layoutA :: DomainWords
  , layoutB :: DomainWords
  , -- | Context record +0x00 of the executing kernel's own record.
    layoutOwnId :: Word32
  , layoutContext :: ContextOffsets
  , layoutLock :: LockOffsets
  , layoutTask :: TaskOffsets
  , layoutDefinition :: DefinitionOffsets
  , layoutHeadSize :: Word32
  , layoutHeadNext :: Word32
  , layoutHeadPrev :: Word32
  , layoutPriorities :: Word32
  , layoutTaskSlots :: Word32
  , layoutStates :: TaskStates
  }
  deriving (Eq, Show)

domainWords :: Layout -> Domain -> DomainWords
domainWords layout domain = case domain of
  DomainA -> layoutA layout
  DomainB -> layoutB layout

instance FromJSON DomainWords where
  parseJSON = withObject "domain words" $ \o ->
    DomainWords
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

instance FromJSON Layout where
  parseJSON = withObject "layout" $ \o -> do
    c <- o .: "context"
    l <- o .: "lock"
    t <- o .: "task"
    d <- o .: "definition"
    h <- o .: "ready_head"
    states <- o .: "task_states"
    stateBits <- case states of
      [dormant, ready, wait, suspended, sleeping, objectQueued, ownerQueued] -> pure (TaskStates dormant ready wait suspended sleeping objectQueued ownerQueued)
      _ -> fail "task_states needs seven values"
    spTerms <- d .: "sp_terms"
    terms <- case spTerms of
      [first, second] -> pure (first, second)
      _ -> fail "sp_terms needs two offsets"
    Layout
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "own_id"
      <*> ( ContextOffsets
              <$> c .: "size"
              <*> c .: "id"
              <*> c .: "owner"
              <*> c .: "scheduler_lock"
              <*> c .: "object_lock"
              <*> c .: "ready_heads"
              <*> c .: "ready_bitmap"
              <*> c .: "running"
              <*> c .: "selected"
              <*> c .: "dispatch_enabled"
              <*> c .: "first_entry"
          )
      <*> (LockOffsets <$> l .: "word" <*> l .: "acquire" <*> l .: "release")
      <*> ( TaskOffsets
              <$> t .: "size"
              <*> t .: "next"
              <*> t .: "prev"
              <*> t .: "definition"
              <*> t .: "state"
              <*> t .: "priority"
              <*> t .: "flags"
              <*> t .: "exception_pattern"
              <*> t .: "saved_sp"
              <*> t .: "resume_pc"
              <*> t .: "domain"
              <*> t .: "terminate_pending"
              <*> t .: "priority_change_pending"
              <*> t .: "cancel_pending"
          )
      <*> (DefinitionOffsets <$> d .: "priority" <*> pure terms)
      <*> h .: "size"
      <*> h .: "next"
      <*> h .: "prev"
      <*> o .: "priorities"
      <*> o .: "task_slots"
      <*> pure stateBits

parseLayout :: B.ByteString -> Either String Layout
parseLayout = J.eitherDecodeStrict
