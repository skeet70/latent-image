-- | What each helper's runs must show, and the oracle's exit status.
module Replacement.RtosScheduler.Verdict
  ( ControlResult (..)
  , holds
  , HelperReport (..)
  , ownedMatches
  , verdictExit
  ) where

import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions (Domain)
import Replacement.RtosScheduler.Controls (Expectation (..))
import Replacement.RtosScheduler.Oracle (Outcome (..))
import Replacement.RtosScheduler.Regions (Helper)
import System.Exit (ExitCode (..))

data ControlResult = ControlResult
  { resultControl :: String
  , resultExpectation :: Expectation
  , resultNote :: String
  , resultOutcome :: Outcome
  }

-- | A must-fail control holds only on a differing case whose subject ended as the row expects.
holds :: ControlResult -> Bool
holds result = case resultExpectation result of
  MustPass -> outcomeCases outcome > 0 && outcomeMatches outcome == outcomeCases outcome
  MustFail -> outcomeDifferingCompleted outcome > 0
  where
    outcome = resultOutcome result

data HelperReport = HelperReport
  { reportDomain :: Domain
  , reportHelper :: Helper
  , reportEntry :: Word32
  , reportStockStatuses :: M.Map String Int
  , -- | Every stock case returned, or spun on its held word.
    reportStockHolds :: Bool
  , reportOwned :: Outcome
  , reportControls :: [ControlResult]
  }

ownedMatches :: HelperReport -> Bool
ownedMatches report = outcomeCases owned > 0 && outcomeMatches owned == outcomeCases owned
  where
    owned = reportOwned report

-- | 1 when a stock case ends otherwise or a control misbehaves, else 2 when an owned body differs from stock.
verdictExit :: [HelperReport] -> ExitCode
verdictExit reports
  | not (all reportStockHolds reports && all (all holds . reportControls) reports) = ExitFailure 1
  | not (all ownedMatches reports) = ExitFailure 2
  | otherwise = ExitSuccess
