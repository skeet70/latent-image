-- | What each slot's runs must show, and the oracle's exit status.
module Replacement.RtosOwnFirst.Verdict
  ( ControlResult (..)
  , holds
  , SlotReport (..)
  , stockHolds
  , ownedMatches
  , verdictExit
  ) where

import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Controls (Expectation (..))
import Replacement.RtosOwnFirst.Oracle (CaseKey, Outcome (..), Run, Slot, allReturned)
import Replacement.RtosOwnFirst.Regions (Domain)
import System.Exit (ExitCode (..))

data ControlResult = ControlResult
  { resultControl :: String
  , resultExpectation :: Expectation
  , resultNote :: String
  , resultOutcome :: Outcome
  }

-- | A must-fail control holds only on a differing case whose subject returned.
holds :: ControlResult -> Bool
holds result = case resultExpectation result of
  MustPass -> outcomeCases outcome > 0 && outcomeMatches outcome == outcomeCases outcome
  MustFail -> outcomeDifferingReturned outcome > 0
  where
    outcome = resultOutcome result

data SlotReport = SlotReport
  { reportDomain :: Domain
  , reportSlot :: Slot
  , reportEntry :: Word32
  , reportStock :: M.Map CaseKey Run
  , reportOwned :: Outcome
  , reportControls :: [ControlResult]
  }

-- | The stock body returned on every case.
stockHolds :: SlotReport -> Bool
stockHolds = allReturned . reportStock

ownedMatches :: SlotReport -> Bool
ownedMatches report = outcomeMatches (reportOwned report) == outcomeCases (reportOwned report)

-- | 1 when a stock run stopped or a control misbehaves, else 2 when an owned body differs from stock.
verdictExit :: [SlotReport] -> ExitCode
verdictExit reports
  | not (all stockHolds reports && all (all holds . reportControls) reports) = ExitFailure 1
  | not (all ownedMatches reports) = ExitFailure 2
  | otherwise = ExitSuccess
