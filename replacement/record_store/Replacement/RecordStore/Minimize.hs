{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Minimize
  ( Boundary(..)
  , Trial(..)
  , TrialBlock(..)
  , ShrinkBudget
  , mkShrinkBudget
  , ShrinkResult(..)
  , ReductionStatus(..)
  , shrinkCase
  ) where

import qualified Data.ByteString as B
import Data.Word (Word16)
import Emulation.NativeObservation (Register)
import Emulation.ScenarioMinimization (ShrinkBudget, mkShrinkBudget)
import qualified Emulation.ScenarioMinimization as Scenario
import Replacement.RecordStore.Cases hiding (bank)
import Replacement.RecordStore.Generate (ValidCase, oracleCase, validCase)
import Replacement.RecordStore.Model

-- | Which observation differed. One tag per existing OracleMain comparison;
-- preserved registers identify the register that differs.
data Boundary
  = Outcome
  | Results
  | Caller
  | Medium
  | Source
  | Requests
  | Callbacks
  | Preserved Register
  deriving (Eq, Ord, Show)

-- | Why a trial did not decide the predicate.
data TrialBlock
  = StockModelDisagreement String
  | UnsupportedNativeStatus String
  | DomainRefusal String
  | InfrastructureFailure String
  deriving (Eq, Show)

data Trial
  = Matches
  | Differs [Boundary]
  | Inconclusive TrialBlock
  deriving (Eq, Show)

data ReductionStatus
  = FixedPoint
  | UnresolvedTrials
  | BudgetExhausted
  | OriginalDoesNotReproduce
  | BaselineInconclusive TrialBlock
  deriving (Eq, Show)

data ShrinkResult = ShrinkResult
  { minimizedCase :: ValidCase
  , trialCount :: Int
  , reductionStatus :: ReductionStatus
  } deriving (Eq, Show)

-- | The returned case is always the last one that reproduced the boundary.
-- 'FixedPoint' means every strictly smaller proposal for the final case had a
-- complete predicate outcome and none reproduced the original boundary; the
-- claim is minimality under the implemented reductions only. 'UnresolvedTrials'
-- means at least one such proposal stayed 'Inconclusive', so local minimality is
-- not established. 'BaselineInconclusive' carries the block that stopped the
-- original trial, so neither reproduction nor non-reproduction is established.
shrinkCase :: ShrinkBudget -> Boundary -> (ValidCase -> IO Trial) -> ValidCase -> IO ShrinkResult
shrinkCase budget boundary oracle original = do
  result <- Scenario.shrinkScenario budget (strategy boundary) original (\valid -> toShared <$> oracle valid)
  pure
    ShrinkResult
      { minimizedCase = Scenario.minimizedScenario result
      , trialCount = Scenario.trialsUsed result
      , reductionStatus = fromSharedStatus (Scenario.reductionStatus result)
      }
  where
    toShared Matches = Scenario.Matches
    toShared (Differs boundaries) = Scenario.Differs boundaries
    toShared (Inconclusive block) = Scenario.Inconclusive block
    fromSharedStatus Scenario.FixedPoint = FixedPoint
    fromSharedStatus Scenario.UnresolvedTrials = UnresolvedTrials
    fromSharedStatus Scenario.BudgetExhausted = BudgetExhausted
    fromSharedStatus Scenario.OriginalDoesNotReproduce = OriginalDoesNotReproduce
    fromSharedStatus (Scenario.BaselineInconclusive block) = BaselineInconclusive block

strategy :: Boundary -> Scenario.ShrinkStrategy ValidCase Operation [Boundary]
strategy boundary =
  Scenario.ShrinkStrategy
    { Scenario.scenarioSize = semanticSize . oracleCase
    , Scenario.scenarioEvents = operations . oracleCase
    , Scenario.setScenarioEvents = \events valid ->
        case validCase (oracleCase valid) {operations = events} of
          Left _ -> Nothing
          Right accepted -> Just accepted
    , Scenario.simplifyEvent = simplifyOperation
    , Scenario.setupReductions = setupCandidates
    , Scenario.prepareScenario = \valid ->
        case validCase (oracleCase valid) of
          Left _ -> Nothing
          Right accepted -> Just accepted
    , Scenario.reproducesDifference = \_ candidate -> boundary `elem` candidate
    }

-- | Semantic size, ordered lexicographically: operation count, occupied records,
-- then scalar weights. All three components are natural numbers and every
-- accepted proposal strictly decreases this order, so reduction terminates.
semanticSize :: OracleCase -> (Int, Int, Int)
semanticSize c = (operations', records', scalars')
  where
    g = geometry (kind c)
    medium' = initialMedium c
    operations' = length (operations c)
    records' = sum [bankOccupancy g bank medium' | bank <- [Bank0, Bank1]]
    scalars' = callbackWeight (callbacks c)
      + sum (map operationWeight (operations c))
      + sum [mediumWeight g bank medium' | bank <- [Bank0, Bank1]]

operationWeight :: Operation -> Int
operationWeight op = case op of
  Append (Seed seed) source -> fromIntegral seed + sourceWeight source
  ReadAt (BytePosition position) lengthBytes -> fromIntegral position + max 0 lengthBytes
  _ -> 0

sourceWeight :: Source -> Int
sourceWeight CallerSource = 0
sourceWeight StagingSource = 1

callbackWeight :: Callback -> Int
callbackWeight callback = case callback of
  NoCallback -> 0
  MarkerCallback -> 1
  CorrectingCallback -> 2
  Hardware2 -> 3
  Hardware3 -> 4

bankOccupancy :: Geometry -> Bank -> Medium -> Int
bankOccupancy g bank medium = length (takeWhile occupied [0 .. recordCount g - 1])
  where
    bytes = bankBytes g bank medium
    occupied position
      | (position + 1) * recordBytes g > B.length bytes = False
      | otherwise = sequenceOf (recordAt g bank (Position position) medium) /= Sequence 0xffff

mediumWeight :: Geometry -> Bank -> Medium -> Int
mediumWeight g bank medium =
  sum [fromIntegral sequenceNumber | position <- [0 .. bankOccupancy g bank medium - 1], let Sequence sequenceNumber = sequenceOf (recordAt g bank (Position position) medium)]

simplifyOperation :: Operation -> [Operation]
simplifyOperation operation = case operation of
  Append (Seed seed) source ->
    [Append (Seed 0) source | seed /= 0]
      ++ [Append (Seed seed) CallerSource | source == StagingSource]
  ReadAt (BytePosition offset) lengthBytes ->
    [ReadAt (BytePosition 0) lengthBytes | offset /= 0]
      ++ [ReadAt (BytePosition offset) 1 | lengthBytes > 1]
  _ -> []

setupCandidates :: ValidCase -> [ValidCase]
setupCandidates valid =
  [ accepted
  | proposal <- truncations c ++ callbackSimplifications c ++ mediumSimplifications c
  , Right accepted <- [validCase proposal]
  ]
  where
    c = oracleCase valid

truncations :: OracleCase -> [OracleCase]
truncations c = concat [truncationsIn bank | bank <- [Bank0, Bank1]]
  where
    g = geometry (kind c)
    truncationsIn bank =
      [c {initialMedium = truncateBank g bank kept (initialMedium c)}
      | kept <- [0 .. bankOccupancy g bank (initialMedium c) - 1]
      ]

truncateBank :: Geometry -> Bank -> Int -> Medium -> Medium
truncateBank g bank kept medium = putBank g bank truncated medium
  where
    bytes = bankBytes g bank medium
    used = min blockBytes (kept * recordBytes g)
    truncated = B.take used bytes <> B.replicate (blockBytes - used) 0xff

callbackSimplifications :: OracleCase -> [OracleCase]
callbackSimplifications c = case callbacks c of
  CorrectingCallback -> [c {callbacks = MarkerCallback}]
  MarkerCallback -> [c {callbacks = NoCallback}]
  _ -> []

mediumSimplifications :: OracleCase -> [OracleCase]
mediumSimplifications c = concat [simplificationsIn bank | bank <- [Bank0, Bank1]]
  where
    g = geometry (kind c)
    simplificationsIn bank =
      [c {initialMedium = setSequence g bank position 0 (initialMedium c)}
      | position <- [0 .. bankOccupancy g bank (initialMedium c) - 1]
      , sequenceOf (recordAt g bank (Position position) (initialMedium c)) /= Sequence 0
      ]

setSequence :: Geometry -> Bank -> Int -> Word16 -> Medium -> Medium
setSequence g bank position sequenceNumber medium =
  putBank g bank (replace (position * recordBytes g) (half sequenceNumber) (bankBytes g bank medium)) medium
