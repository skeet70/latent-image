{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairEvidence
  ( CallEvidence, emptyCallEvidence, accumulateEvidence, qualifyEvidence, evidenceFailureValue
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
import Replacement.RtosScheduler.Regions (Helper (..))

data CallFailure = CallFailure !Word64 !Refusal
data SemanticFailureDetails = SemanticFailureDetails !Word64 !Domain !Address !Semantic !Int !Refusal
data CallEvidence = CallEvidence
  { evidenceFailure :: !(Maybe CallFailure)
  , evidenceSemanticFailure :: !(Maybe SemanticFailureDetails)
  , evidenceTotals :: !CallTotals
  , evidenceSummary :: !CallSummary
  , evidenceDemand :: ![DemandWitness]
  , evidenceTimers :: ![TimerWitness]
  , evidenceUnlocks :: ![ReleaseWitness]
  , evidenceDeferred :: ![Call]
  , evidenceOverflow :: !Bool
  }

emptyCallEvidence :: CallEvidence
emptyCallEvidence = CallEvidence Nothing Nothing (CallTotals False 0) emptyCallSummary [] [] [] [] False

unlockWitnesses :: Call -> [ReleaseWitness]
unlockWitnesses call = case callSemantic call of
  HelperSemantic Release -> stores
  HelperSemantic Acquire -> stores
  _ -> []
 where
  stores = [ReleaseWitness task (writerCore origin) (callFirst call) (writerInstructions origin) address value
    | (Written address _size value, origin) <- zip (callWrites call) (callWriteOrigins call)
    , ownsOrigin call origin
    , let task = case writerTask origin of Address owner -> owner]

accumulateEvidence :: Plan -> CallEvidence -> Call -> CallEvidence
accumulateEvidence plan prior call =
  let timers = evidenceTimers prior ++ timerWitnesses [call]
      unlocks = evidenceUnlocks prior ++ unlockWitnesses call
      semantic = checkCallForInWitnesses (take 64 timers) (planRole plan) call
      deferred = case semantic of
        Left FamilyTimer -> [call]
        _ -> []
      overflow = evidenceOverflow prior || length timers > 64 || length unlocks > 64 || length (evidenceDeferred prior) + length deferred > 64
  in accumulateKnown timers unlocks deferred overflow plan semantic prior call

accumulateKnown :: [TimerWitness] -> [ReleaseWitness] -> [Call] -> Bool -> Plan -> Either Refusal () -> CallEvidence -> Call -> CallEvidence
accumulateKnown timers unlocks deferred overflow plan semanticResult prior call = CallEvidence firstFailure firstSemantic totals counts witnesses (take 64 timers) (take 64 unlocks) (take 64 (evidenceDeferred prior ++ deferred)) (evidenceOverflow prior || overflow)
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
  expect Incomplete (not (evidenceOverflow evidence))
  forM_ (evidenceDeferred evidence) $ \call -> do
    checkCallEnvelope plan call
    checkCallForInWitnesses (evidenceTimers evidence) (planRole plan) call
  case evidenceFailure evidence of
    Nothing -> pure ()
    Just (CallFailure _ refusal) -> Left refusal
  qualifySuffix plan (evidenceUnlocks evidence) observation (evidenceTotals evidence)
  pure (summaryWith observation (evidenceSummary evidence))

evidenceFailureValue :: CallEvidence -> J.Value
evidenceFailureValue evidence = case evidenceSemanticFailure evidence of
  Nothing -> J.Null
  Just (SemanticFailureDetails first domain address semantic core refusal) -> J.object
    ["first" J..= first,"domain" J..= domainLabel domain,"entry" J..= show address
    ,"semantic" J..= semanticValue semantic,"core" J..= core,"refusal" J..= show refusal]

data RetainedControl
  = NoControlCalls
  | WakeFailure !(Maybe Call)
  | NotifyFailures !(Maybe Call) ![Call]
data ControlEvidence = ControlEvidence !CallEvidence !RetainedControl

emptyControlEvidence :: Plan -> ControlEvidence
emptyControlEvidence plan = ControlEvidence emptyCallEvidence $ case planRole plan of
  SilentNotify -> NotifyFailures Nothing []
  WrongPredicate -> NotifyFailures Nothing []
  WakeNoInsert -> WakeFailure Nothing
  _ -> NoControlCalls

accumulateControlEvidence :: Plan -> ControlEvidence -> Call -> ControlEvidence
accumulateControlEvidence plan (ControlEvidence compact retained) call =
  ControlEvidence (accumulateKnown timers unlocks deferred overflow plan checked compact call) (retainControl (planRole plan) checked retained call)
 where
  timers = evidenceTimers compact ++ timerWitnesses [call]
  unlocks = evidenceUnlocks compact ++ unlockWitnesses call
  checked = checkCallForInWitnesses (take 64 timers) (planRole plan) call
  deferred = case checked of
    Left FamilyTimer -> [call]
    _ -> []
  overflow = evidenceOverflow compact || length timers > 64 || length unlocks > 64 || length (evidenceDeferred compact) + length deferred > 64

-- Notify adjudication retains its earliest target and preceding failed contracts.
retainControl :: Role -> Either Refusal () -> RetainedControl -> Call -> RetainedControl
retainControl role checked retained call = case retained of
  NoControlCalls -> retained
  WakeFailure previous -> case checked of
    Left _ -> WakeFailure (earlierCall previous call)
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
        in NotifyFailures selected trimmed
 where
  isTarget = controlTarget role call

earlierCall :: Maybe Call -> Call -> Maybe Call
earlierCall previous candidate = case previous of
  Just old | callFirst old<=callFirst candidate -> previous
  _ -> Just candidate

qualifyControlEvidence :: B.ByteString -> Plan -> Observation -> ControlEvidence -> Either Refusal J.Value
qualifyControlEvidence bytes plan observation (ControlEvidence compact _) = qualifyEvidence bytes plan observation compact

checkControlEvidence :: Role -> Observation -> ControlEvidence -> Either Refusal ()
checkControlEvidence role observation (ControlEvidence compact retained) =
  controlFailure role (observation {obsCalls=calls})
 where
  rawCalls = case retained of
    NoControlCalls -> []
    WakeFailure Nothing -> []
    WakeFailure (Just failure) -> [failure]
    NotifyFailures Nothing _ -> []
    NotifyFailures (Just target) failures -> target:failures
  deferredCalls = evidenceDeferred compact
  resolved call = call `elem` deferredCalls && case checkCallForInWitnesses (evidenceTimers compact) role call of
    Right () -> True
    Left _ -> False
  calls = filter (not . resolved) rawCalls

controlFailureValue :: ControlEvidence -> J.Value
controlFailureValue (ControlEvidence compact _) = evidenceFailureValue compact
