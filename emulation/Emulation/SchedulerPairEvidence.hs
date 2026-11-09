{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairEvidence
  ( CallEvidence, evidenceTotals, evidenceLimit, evidenceTimerTotal, evidenceDeferredTotal
  , timerWitnessBudget, deferredCallBudget, controlCandidateBudget
  , emptyCallEvidence, accumulateEvidence, qualifyEvidence, evidenceFailureValue
  , ControlEvidence, emptyControlEvidence, accumulateControlEvidence, qualifyControlEvidence
  , checkControlEvidence, controlFailureValue ) where

import Control.Monad (forM_)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.Word (Word64)
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairFamily (ReleaseWitness (..), TimerWitness)
import Emulation.SchedulerPairPlan
import Replacement.RtosOwnFirst.Regions (Domain, domainLabel)

-- | Explicit fail-closed budgets. The retained family stock observation has
-- 1098 timer witnesses; 4096 leaves a bounded margin for the same natural
-- route. A receipt past one refuses with the named budget and count; admitted
-- witnesses are retained whole, not as a prefix.
timerWitnessBudget, deferredCallBudget, controlCandidateBudget :: Int
timerWitnessBudget = 4096
deferredCallBudget = 256
controlCandidateBudget = 256

data CallFailure = CallFailure !Word64 !Refusal
data SemanticFailureDetails = SemanticFailureDetails !Word64 !Domain !Address !Semantic !Int !Refusal
data CallEvidence = CallEvidence
  { evidenceFailure :: !(Maybe CallFailure)
  , evidenceSemanticFailure :: !(Maybe SemanticFailureDetails)
  , evidenceTotals :: !CallTotals
  , evidenceSummary :: !CallSummary
  , evidenceDemand :: ![DemandWitness]
  , evidenceTimers :: ![TimerWitness]
  , evidenceDeferred :: ![Call]
  , evidenceLimit :: !(Maybe (BudgetKind, Int, Int))
  , evidenceTimerTotal :: !Int
  , evidenceDeferredTotal :: !Int
  }

emptyCallEvidence :: CallEvidence
emptyCallEvidence = CallEvidence Nothing Nothing (CallTotals False 0) emptyCallSummary [] [] [] Nothing 0 0

accumulateEvidence :: Plan -> CallEvidence -> Call -> CallEvidence
accumulateEvidence plan prior call =
  let timers = evidenceTimers prior ++ timerWitnesses [call]
      timerTotal = length timers
      semantic = checkCallForInWitnesses timers (planRole plan) call
      deferred = case semantic of
        Left FamilyTimer -> [call]
        _ -> []
      deferredTotal = evidenceDeferredTotal prior + length deferred
      limit = case evidenceLimit prior of
        Just exceeded -> Just exceeded
        Nothing
          | timerTotal > timerWitnessBudget -> Just (TimerWitnessBudget, timerTotal, timerWitnessBudget)
          | deferredTotal > deferredCallBudget -> Just (DeferredCallBudget, deferredTotal, deferredCallBudget)
          | otherwise -> Nothing
  in accumulateKnown timers deferred limit timerTotal deferredTotal plan semantic prior call

accumulateKnown :: [TimerWitness] -> [Call] -> Maybe (BudgetKind, Int, Int) -> Int -> Int -> Plan -> Either Refusal () -> CallEvidence -> Call -> CallEvidence
accumulateKnown timers deferred limit timerTotal deferredTotal plan semanticResult prior call = CallEvidence firstFailure firstSemantic totals counts witnesses (take timerWitnessBudget timers) (take deferredCallBudget (evidenceDeferred prior ++ deferred)) limit timerTotal deferredTotal
 where
  deferredFailure = case semanticResult of
    Left FamilyTimer -> not (null deferred)
    _ -> False
  checkedResult = if deferredFailure then Right () else semanticResult
  plannedResult = checkCallEnvelope plan call >> checkedResult
  firstFailure = case plannedResult of
    Right () -> evidenceFailure prior
    Left refusal ->
      let failure = CallFailure (callFirst call) refusal
      in failure `seq` earlierFailure (evidenceFailure prior) failure
  firstSemantic = case checkedResult of
    Right () -> evidenceSemanticFailure prior
    Left refusal ->
      let failure = SemanticFailureDetails (callFirst call) (callDomain call) (callAddress call)
            (callSemantic call) (callCore call) refusal
      in failure `seq` earlierSemantic (evidenceSemanticFailure prior) failure
  totals = CallTotals True (completedCallCount (evidenceTotals prior) + callCount call)
  counts = case plannedResult of
    Right () -> summarizeCall (planRole plan) (evidenceSummary prior) call
    Left _ -> evidenceSummary prior
  witnesses = foldl' (\previous witness -> witness `seq` witness:previous) (evidenceDemand prior) (demandWitnesses call)

earlierFailure :: Maybe CallFailure -> CallFailure -> Maybe CallFailure
earlierFailure previous candidate@(CallFailure first _) = case previous of
  Just (CallFailure old _) | old<=first -> previous
  _ -> Just candidate

earlierSemantic :: Maybe SemanticFailureDetails -> SemanticFailureDetails -> Maybe SemanticFailureDetails
earlierSemantic previous candidate@(SemanticFailureDetails first _ _ _ _ _) = case previous of
  Just (SemanticFailureDetails old _ _ _ _ _) | old<=first -> previous
  _ -> Just candidate

qualifyEvidence :: B.ByteString -> Plan -> Observation -> CallEvidence -> Either Refusal J.Value
qualifyEvidence bytes plan observation evidence = do
  qualifyPrefix bytes plan observation
  checkDemandWitnesses (obsDemandPaging observation) (evidenceDemand evidence)
  case evidenceLimit evidence of
    Just (kind, count, limit) -> Left (BudgetExceeded kind count limit)
    Nothing -> pure ()
  forM_ (evidenceDeferred evidence) $ \call -> do
    checkCallEnvelope plan call
    checkCallForInWitnesses (evidenceTimers evidence) (planRole plan) call
  case evidenceFailure evidence of
    Nothing -> pure ()
    Just (CallFailure _ refusal) -> Left refusal
  let releases = [ReleaseWitness (lockTask event) (lockCore event) (lockCall event) (lockAt event) (lockAddress event) (lockValue event) | event <- obsLockEvents observation]
  qualifySuffix plan releases observation (evidenceTotals evidence)
  pure (summaryWith observation (evidenceSummary evidence))

evidenceFailureValue :: CallEvidence -> J.Value
evidenceFailureValue evidence = case evidenceSemanticFailure evidence of
  Nothing -> J.Null
  Just (SemanticFailureDetails first domain address semantic core refusal) -> J.object
    ["first" J..= first,"domain" J..= domainLabel domain,"entry" J..= show address
    ,"semantic" J..= semanticValue semantic,"core" J..= core,"refusal" J..= show refusal]

data RetainedControl
  = NoControlCalls
  | WakeFailure ![Call]
  | NotifyFailures !(Maybe Call) ![Call]
data ControlEvidence = ControlEvidence !CallEvidence !RetainedControl !(Maybe (BudgetKind, Int, Int))

emptyControlEvidence :: Plan -> ControlEvidence
emptyControlEvidence plan = ControlEvidence emptyCallEvidence (case planRole plan of
  SilentNotify -> NotifyFailures Nothing []
  WrongPredicate -> NotifyFailures Nothing []
  WakeNoInsert -> WakeFailure []
  _ -> NoControlCalls) Nothing

accumulateControlEvidence :: Plan -> ControlEvidence -> Call -> ControlEvidence
accumulateControlEvidence plan (ControlEvidence compact retained controlLimit) call =
  ControlEvidence (accumulateKnown timers deferred limit timerTotal deferredTotal plan checked compact call)
    (retainControl (planRole plan) checked retained call) control
 where
  timers = evidenceTimers compact ++ timerWitnesses [call]
  timerTotal = length timers
  checked = checkCallForInWitnesses timers (planRole plan) call
  deferred = case checked of
    Left FamilyTimer -> [call]
    _ -> []
  deferredTotal = evidenceDeferredTotal compact + length deferred
  limit = case evidenceLimit compact of
    Just exceeded -> Just exceeded
    Nothing
      | timerTotal > timerWitnessBudget -> Just (TimerWitnessBudget, timerTotal, timerWitnessBudget)
      | deferredTotal > deferredCallBudget -> Just (DeferredCallBudget, deferredTotal, deferredCallBudget)
      | otherwise -> Nothing
  candidates = case retained of
    WakeFailure previous -> length previous
    NotifyFailures _ failures -> length failures
    NoControlCalls -> 0
  nextCandidates = candidates + (case checked of Left _ -> 1; Right () -> 0)
  control = case controlLimit of
    Just exceeded -> Just exceeded
    Nothing | nextCandidates > controlCandidateBudget -> Just (ControlBudget, nextCandidates, controlCandidateBudget)
            | otherwise -> Nothing

-- Notify adjudication retains its earliest target and preceding failed contracts.
retainControl :: Role -> Either Refusal () -> RetainedControl -> Call -> RetainedControl
retainControl role checked retained call = case retained of
  NoControlCalls -> retained
  WakeFailure previous -> case checked of
    Left _ -> WakeFailure (take controlCandidateBudget (call:previous))
    Right () -> retained
  NotifyFailures target failures
    | not isTarget, Right () <- checked -> retained
    | otherwise ->
        let selected = if isTarget then earlierCall target call else target
            relevant = maybe True (\chosen -> callFirst call<=callFirst chosen && call/=chosen) selected
            added = case checked of Left _ | relevant -> call:failures; _ -> failures
            earlier = isTarget && maybe True (\previous -> callFirst call<callFirst previous) target
            trimmed = if earlier
              then let !limit=callFirst call in foldl' (\prior failure -> if callFirst failure<=limit then failure:prior else prior) [] added
              else added
        in NotifyFailures selected (take controlCandidateBudget trimmed)
 where
  isTarget = controlTarget role call

earlierCall :: Maybe Call -> Call -> Maybe Call
earlierCall previous candidate = case previous of
  Just old | callFirst old<=callFirst candidate -> previous
  _ -> Just candidate

qualifyControlEvidence :: B.ByteString -> Plan -> Observation -> ControlEvidence -> Either Refusal J.Value
qualifyControlEvidence bytes plan observation (ControlEvidence compact _ controlLimit) = do
  case controlLimit of
    Just (kind, count, limit) -> Left (BudgetExceeded kind count limit)
    Nothing -> pure ()
  qualifyEvidence bytes plan observation compact

checkControlEvidence :: Role -> Observation -> ControlEvidence -> Either Refusal ()
checkControlEvidence role observation (ControlEvidence compact retained controlLimit) = do
  case controlLimit of
    Just (kind, count, limit) -> Left (BudgetExceeded kind count limit)
    Nothing -> pure ()
  controlFailure role (observation {obsCalls=calls})
 where
  rawCalls = case retained of
    NoControlCalls -> []
    WakeFailure failures -> failures
    NotifyFailures Nothing _ -> []
    NotifyFailures (Just target) failures -> target:failures
  deferredCalls = evidenceDeferred compact
  resolved call = call `elem` deferredCalls && case checkCallForInWitnesses (evidenceTimers compact) role call of
    Right () -> True
    Left _ -> False
  calls = filter (not . resolved) rawCalls

controlFailureValue :: ControlEvidence -> J.Value
controlFailureValue (ControlEvidence compact _ _) = evidenceFailureValue compact
