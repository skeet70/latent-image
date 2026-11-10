{-# LANGUAGE BangPatterns #-}
module Replacement.RtosTaskRecords.SystemCheck
  ( Lifecycle (..), SystemRefusal (..), SystemEvidence, emptySystemEvidence, accumulateSystem
  , coverage, qualifySystemEvidence, compareSystemEvidence ) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.Bits ((.&.))
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import qualified Emulation.SchedulerPairCheck as C
import qualified Emulation.SchedulerPairEvidence as E
import Emulation.SchedulerPairModel (Arguments (..))
import Emulation.SchedulerPairPlan
import Replacement.RtosOwnFirst.Regions (Domain, domains)

data Lifecycle = Created | ResetDeleted | Cleared | Recreated deriving (Eq,Ord,Show,Enum,Bounded)
data SystemRefusal = ObservationRefused C.Refusal | MissingLifecycle Domain Lifecycle | RepeatedInvocation | ForeignSemantic | LifecycleDifference | LifetimeBound deriving (Eq,Show)
data Event = Event
  { eventDomain :: !Domain, eventPoint :: !TaskRecordPoint, eventPacked :: !Word32
  , eventRequest :: ![Word32], eventStatus :: !Word32, eventFirst :: !Word64, eventCount :: !Int
  } deriving (Eq,Show)
data SystemEvidence = SystemEvidence !E.CallEvidence !(Either SystemRefusal [Event]) !Int

emptySystemEvidence :: SystemEvidence
emptySystemEvidence=SystemEvidence E.emptyCallEvidence (Right []) 0

accumulateSystem :: Plan -> SystemEvidence -> C.Call -> SystemEvidence
accumulateSystem plan (SystemEvidence previous events count) call =
  let !checked=E.accumulateEvidence plan previous call
      !next=do
        prior<-events
        unless (count<512) (Left LifetimeBound)
        event<-eventOf call
        let !forced=foldl' (\() word->word `seq` ()) () (eventRequest event)
        forced `seq` pure (event:prior)
  in SystemEvidence checked next (count+1)

eventOf :: C.Call -> Either SystemRefusal Event
eventOf call = do
  point<-case C.callSemantic call of TaskRecordSemantic value->Right value; _->Left ForeignSemantic
  let Arguments arguments=C.callArguments call
  case arguments of
    packed:parameter:_->do
      request<-case point of
        DeleteTaskRecord->Right [parameter]
        CreateTaskRecord->mapM (\offset->maybe (Left (ObservationRefused C.EntryUnreadable)) Right (M.lookup (Address (parameter+offset)) (C.callBefore call))) [0,4..20]
      pure (Event (C.callDomain call) point packed request (C.callR0 call) (C.callFirst call) (C.callCount call))
    _->Left (ObservationRefused C.EntryUnreadable)

coverage :: [C.Call] -> Either SystemRefusal ()
coverage calls = mapM eventOf calls >>= eventCoverage

eventCoverage :: [Event] -> Either SystemRefusal ()
eventCoverage events = do
  unless (length events<=512) (Left LifetimeBound)
  unless (all ((==1) . eventCount) events) (Left RepeatedInvocation)
  forM_ domains $ \domain->do
    let successful=[event | event<-sortOn eventFirst events,eventDomain event==domain,eventStatus event==0]
        created=any (is CreateTaskRecord) successful
        reset=any (\event->is DeleteTaskRecord event && eventRequest event==[1]) successful
        cleared=any (\event->is DeleteTaskRecord event && eventRequest event/=[1]) successful
        reused=any reuse (M.elems (M.fromListWith (++) [(eventPacked event .&. 0xffff,[event]) | event<-successful]))
    forM_ [(Created,created),(ResetDeleted,reset),(Cleared,cleared),(Recreated,reused)] $ \(step,seen)->unless seen (Left (MissingLifecycle domain step))
 where
  is point event=eventPoint event==point
  reuse events'=go False False (sortOn eventFirst events')
  go _ _ []=False
  go created deleted (event:rest)
    | is CreateTaskRecord event && created && deleted=True
    | is CreateTaskRecord event=go True False rest
    | is DeleteTaskRecord event=go created True rest
    | otherwise=go created deleted rest

qualifySystemEvidence :: B.ByteString -> Plan -> C.Observation -> SystemEvidence -> Either SystemRefusal J.Value
qualifySystemEvidence bytes plan observation (SystemEvidence checked captured _) = do
  unless (all (\entry->case entrySemantic entry of TaskRecordSemantic _->True; _->False) (planEntries plan)) (Left ForeignSemantic)
  result<-either (Left . ObservationRefused) Right (E.qualifyEvidence bytes plan observation checked)
  events<-captured
  eventCoverage events
  pure result

compareSystemEvidence :: C.Observation -> SystemEvidence -> C.Observation -> SystemEvidence -> Either SystemRefusal ()
compareSystemEvidence stock (SystemEvidence _ stockEvents _) owned (SystemEvidence _ ownedEvents _) = do
  either (Left . ObservationRefused) Right (C.compareRoutes stock owned)
  a<-signatures <$> stockEvents
  b<-signatures <$> ownedEvents
  unless (a==b) (Left LifecycleDifference)
 where
  signatures=foldl' collect M.empty . sortOn eventFirst
  collect table event=M.insertWith (flip (++)) (eventDomain event,eventPacked event .&. 0xffff)
    [(eventPoint event,eventPacked event,eventRequest event,eventStatus event)] table
