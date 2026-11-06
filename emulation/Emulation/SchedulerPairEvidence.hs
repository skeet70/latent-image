{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairEvidence
  ( CallEvidence, emptyCallEvidence, accumulateEvidence, qualifyEvidence, evidenceFailureValue
  , ControlEvidence, emptyControlEvidence, accumulateControlEvidence, qualifyControlEvidence
  , checkControlEvidence, controlFailureValue ) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.Word (Word64)
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairPlan
import Replacement.RtosOwnFirst.Regions (Domain, domainLabel)

data CallFailure = CallFailure !Word64 !Refusal
data SemanticFailureDetails = SemanticFailureDetails !Word64 !Domain !Address !Semantic !Int !Refusal
data CallEvidence = CallEvidence
  { evidenceFailure :: !(Maybe CallFailure)
  , evidenceSemanticFailure :: !(Maybe SemanticFailureDetails)
  , evidenceTotals :: !CallTotals
  , evidenceSummary :: !CallSummary
  , evidenceDemand :: ![DemandWitness]
  }

emptyCallEvidence :: CallEvidence
emptyCallEvidence = CallEvidence Nothing Nothing (CallTotals False 0) emptyCallSummary []

accumulateEvidence :: Plan -> CallEvidence -> Call -> CallEvidence
accumulateEvidence plan prior call = accumulateKnown plan (checkCallFor (planRole plan) call) prior call

accumulateKnown :: Plan -> Either Refusal () -> CallEvidence -> Call -> CallEvidence
accumulateKnown plan semanticResult prior call = CallEvidence firstFailure firstSemantic totals counts witnesses
 where
  plannedResult = checkCallEnvelope plan call >> semanticResult
  firstFailure = case plannedResult of
    Right () -> evidenceFailure prior
    Left refusal ->
      let failure = CallFailure (callFirst call) refusal
      in failure `seq` earlierFailure (evidenceFailure prior) failure
  firstSemantic = case semanticResult of
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
  case evidenceFailure evidence of
    Nothing -> pure ()
    Just (CallFailure _ refusal) -> Left refusal
  qualifySuffix observation (evidenceTotals evidence)
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
  ControlEvidence (accumulateKnown plan checked compact call) (retainControl (planRole plan) checked retained call)
 where
  checked = checkCallFor (planRole plan) call

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
checkControlEvidence role observation (ControlEvidence _ retained) =
  controlFailure role (observation {obsCalls=calls})
 where
  calls = case retained of
    NoControlCalls -> []
    WakeFailure Nothing -> []
    WakeFailure (Just failure) -> [failure]
    NotifyFailures Nothing _ -> []
    NotifyFailures (Just target) failures -> target:failures

controlFailureValue :: ControlEvidence -> J.Value
controlFailureValue (ControlEvidence compact _) = evidenceFailureValue compact
