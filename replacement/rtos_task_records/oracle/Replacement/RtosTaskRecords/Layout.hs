{-# LANGUAGE OverloadedStrings #-}

-- | The task-record tables and field offsets, as the oracle's layout.json prints them.
module Replacement.RtosTaskRecords.Layout
  ( DomainFacts (..)
  , TaskOffsets (..)
  , DefinitionOffsets (..)
  , Facts (..)
  , parseFacts
  , domainFacts
  ) where

import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data DomainFacts = DomainFacts
  { dfTaskTable :: Word32
  , dfTaskDefinitions :: Word32
  , dfTaskSlotIds :: Word32
  , dfDefaultStackWord :: Word32
  , dfDomainId :: Word32
  , dfMaskSave :: Word32
  , dfMaskRestore :: Word32
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
  , taskWait :: Word32
  , taskSavedSp :: Word32
  , taskResumePc :: Word32
  , taskDomain :: Word32
  , taskTerminatePending :: Word32
  , taskPriorityChangePending :: Word32
  , taskCancelPending :: Word32
  , taskContextDiscarded :: Word32
  , taskPendingPriority :: Word32
  }
  deriving (Eq, Show)

data DefinitionOffsets = DefinitionOffsets
  { definitionSize :: Word32
  , definitionAttributes :: Word32
  , definitionArgument :: Word32
  , definitionEntry :: Word32
  , definitionPriority :: Word32
  , definitionStackSize :: Word32
  , definitionStack :: Word32
  , definitionExceptionAttributes :: Word32
  , definitionExceptionHandler :: Word32
  , definitionSlot :: Word32
  , definitionExists :: Word32
  }
  deriving (Eq, Show)

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
  , factsTask :: TaskOffsets
  , factsDefinition :: DefinitionOffsets
  , factsTaskSlots :: Word32
  }
  deriving (Eq, Show)

instance FromJSON DomainFacts where
  parseJSON = withObject "domain" $ \o ->
    DomainFacts
      <$> o .: "task_table"
      <*> o .: "task_definitions"
      <*> o .: "task_slot_ids"
      <*> o .: "default_stack_word"
      <*> o .: "domain_id"
      <*> o .: "mask_save"
      <*> o .: "mask_restore"

instance FromJSON TaskOffsets where
  parseJSON = withObject "task" $ \o ->
    TaskOffsets
      <$> o .: "size"
      <*> o .: "next"
      <*> o .: "prev"
      <*> o .: "definition"
      <*> o .: "state"
      <*> o .: "priority"
      <*> o .: "flags"
      <*> o .: "exception_pattern"
      <*> o .: "wait"
      <*> o .: "saved_sp"
      <*> o .: "resume_pc"
      <*> o .: "domain"
      <*> o .: "terminate_pending"
      <*> o .: "priority_change_pending"
      <*> o .: "cancel_pending"
      <*> o .: "context_discarded"
      <*> o .: "pending_priority"

instance FromJSON DefinitionOffsets where
  parseJSON = withObject "definition" $ \o ->
    DefinitionOffsets
      <$> o .: "size"
      <*> o .: "attributes"
      <*> o .: "argument"
      <*> o .: "entry"
      <*> o .: "priority"
      <*> o .: "stack_size"
      <*> o .: "stack"
      <*> o .: "exception_attributes"
      <*> o .: "exception_handler"
      <*> o .: "slot"
      <*> o .: "exists"

instance FromJSON Facts where
  parseJSON = withObject "task record layout" $ \o ->
    Facts
      <$> o .: "a"
      <*> o .: "b"
      <*> o .: "task"
      <*> o .: "definition"
      <*> o .: "task_slots"

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts
