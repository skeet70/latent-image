-- | One helper's cases run under a reference role and each subject role in one runner session per
-- 4096 cases, compared case by case.
module Replacement.RtosScheduler.Suite
  ( Role (..)
  , HelperRun (..)
  , runHelper
  ) where

import qualified Data.Map.Strict as M
import Emulation.NativeObservation (NativeObservation)
import Replacement.RtosOwnFirst.Regions (Domain)
import Replacement.RtosScheduler.Oracle
import Replacement.RtosScheduler.Regions (Helper)

data Role = Role
  { roleName :: String
  , roleBody :: Body
  , -- | Whether the barrier order is asserted on its runs.
    roleAsserted :: Bool
  }

data HelperRun = HelperRun
  { runReference :: M.Map CaseKey NativeObservation
  , -- | Every reference case returned, or spun on its held word.
    runReferenceEnds :: Bool
  , -- | With an asserted reference, every reference case keeps the barrier order.
    runReferenceOrder :: Bool
  , runSubjects :: [(String, Outcome)]
  }

runHelper :: Env -> FilePath -> FilePath -> Domain -> Helper -> [CaseKey] -> Role -> [Role] -> IO HelperRun
runHelper env runner directory domain helper keys reference subjects = do
  let grid = M.fromList (zip (map CaseKey [0 ..]) (caseGrid (envLayout env) helper))
      rows = [(key, row) | key <- keys, Just row <- [M.lookup key grid]]
      specOf role key row = caseSpec env domain (roleBody role) (caseLabel (roleName role) domain helper key) helper row
      roles = reference : subjects
  observed <- runCases runner directory [(caseLabel (roleName role) domain helper key, specOf role key row) | role <- roles, (key, row) <- rows]
  let runsOf role = M.fromList [(key, observation) | (key, _) <- rows, Just observation <- [M.lookup (caseLabel (roleName role) domain helper key) observed]]
      referenceRuns = runsOf reference
      referenceSpec key = specOf reference key (grid M.! key)
      completed key = endsAsExpected env domain helper (grid M.! key) (referenceSpec key)
      outcome role = outcomeOf completed referenceRuns (runsOf role) $ \key expected actual ->
        compareCase env domain helper (grid M.! key) (referenceSpec key) (roleAsserted role) expected actual
  pure
    HelperRun
      { runReference = referenceRuns
      , runReferenceEnds = M.size referenceRuns == length rows && and [completed key observation | (key, observation) <- M.toList referenceRuns]
      , runReferenceOrder = not (roleAsserted reference) || and [orderHolds env domain helper (grid M.! key) observation | (key, observation) <- M.toList referenceRuns]
      , runSubjects = [(roleName role, outcome role) | role <- subjects]
      }
