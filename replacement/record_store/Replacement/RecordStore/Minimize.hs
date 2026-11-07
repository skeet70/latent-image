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

import Data.List (nub)
import qualified Data.ByteString as B
import Data.Word (Word16)
import Emulation.NativeObservation (Register)
import Replacement.RecordStore.Cases
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

newtype ShrinkBudget = ShrinkBudget Int deriving (Eq, Show)

mkShrinkBudget :: Int -> Either String ShrinkBudget
mkShrinkBudget count
  | count >= 1 && count <= 256 = Right (ShrinkBudget count)
  | otherwise = Left "shrink budget must be between 1 and 256"

data ReductionStatus
  = FixedPoint
  | BudgetExhausted
  | OriginalDoesNotReproduce
  deriving (Eq, Show)

data ShrinkResult = ShrinkResult
  { minimizedCase :: ValidCase
  , trialCount :: Int
  , reductionStatus :: ReductionStatus
  } deriving (Eq, Show)

-- | Semantic size, ordered lexicographically: operation count, occupied records,
-- then scalar weights. All three components are natural numbers and every
-- accepted proposal strictly decreases this order, so reduction terminates.
data SemanticSize = SemanticSize !Int !Int !Int deriving (Eq, Ord, Show)

semanticSize :: OracleCase -> SemanticSize
semanticSize c = SemanticSize operations' records' scalars'
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

-- | Fixed point here means no implemented proposal is accepted, not global
-- minimality and not a claim about the underlying root cause.
shrinkCase :: ShrinkBudget -> Boundary -> (ValidCase -> IO Trial) -> ValidCase -> IO ShrinkResult
shrinkCase (ShrinkBudget budget) originalBoundary oracle original = do
  baseline <- oracle original
  case baseline of
    Differs boundaries | originalBoundary `elem` boundaries -> descend 1 original
    _ -> pure (ShrinkResult original 1 OriginalDoesNotReproduce)
  where
    descend used current = search used current (proposals current)

    search used current [] = pure (ShrinkResult current used FixedPoint)
    search used current (candidate:rest) =
      case validCase candidate of
        Left _ -> search used current rest
        Right valid
          | semanticSize (oracleCase valid) >= semanticSize (oracleCase current) -> search used current rest
          | used >= budget -> pure (ShrinkResult current used BudgetExhausted)
          | otherwise -> do
              result <- oracle valid
              case result of
                Differs boundaries | originalBoundary `elem` boundaries -> descend (used + 1) valid
                _ -> search (used + 1) current rest

proposals :: ValidCase -> [OracleCase]
proposals valid =
  deletions c ++ truncations c ++ operationSimplifications c ++ callbackSimplifications c ++ mediumSimplifications c
  where
    c = oracleCase valid

deletions :: OracleCase -> [OracleCase]
deletions c = [c {operations = candidate} | candidate <- nub candidates]
  where
    (prefix, body) = splitAt 2 (operations c)
    candidates =
      [prefix ++ before ++ after
      | size <- [length body, length body - 1 .. 1]
      , start <- [0 .. length body - size]
      , let (before, rest) = splitAt start body
            after = drop size rest
      ]

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

operationSimplifications :: OracleCase -> [OracleCase]
operationSimplifications c = concat
  [simplify position operation | (position, operation) <- zip [0 ..] (operations c), position >= 2]
  where
    simplify position operation = case operation of
      Append (Seed seed) source ->
        [c {operations = setAt position (Append (Seed 0) source) (operations c)} | seed /= 0]
        ++ [c {operations = setAt position (Append (Seed seed) CallerSource) (operations c)} | source == StagingSource]
      ReadAt (BytePosition offset) lengthBytes ->
        [c {operations = setAt position (ReadAt (BytePosition 0) lengthBytes) (operations c)} | offset /= 0]
        ++ [c {operations = setAt position (ReadAt (BytePosition offset) 1) (operations c)} | lengthBytes > 1]
      _ -> []

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

setAt :: Int -> a -> [a] -> [a]
setAt position value items = take position items ++ [value] ++ drop (position + 1) items
