-- | Boundary-preserving reduction of one +0x16c row. Candidates move a valid input down a declared
-- measure: task record index, then entry-flag population, then mode, F and the context form. The
-- result is a local fixed point of that order, not a minimum over the input space.
module Replacement.RtosOwnFirst.Shrink
  ( ShrinkBudget
  , mkShrinkBudget
  , shrinkBudgetValue
  , defaultShrinkBudget
  , parseShrinkBudget
  , ReductionStatus (..)
  , reductionStatusText
  , Reduction (..)
  , shrinkRow
  , reduceCandidates
  , rowMeasure
  ) where

import Data.Bits (popCount)
import Data.List (nub)
import Numeric (readDec)
import Replacement.RtosOwnFirst.Generate (taskSlotOf, validRow)
import Replacement.RtosOwnFirst.Oracle

newtype ShrinkBudget = ShrinkBudget Int
  deriving (Eq, Show)

mkShrinkBudget :: Int -> Either String ShrinkBudget
mkShrinkBudget budget
  | budget >= 1 && budget <= maxShrinkBudget = Right (ShrinkBudget budget)
  | otherwise = Left ("shrink budget must be 1.." ++ show maxShrinkBudget ++ ": " ++ show budget)

shrinkBudgetValue :: ShrinkBudget -> Int
shrinkBudgetValue (ShrinkBudget budget) = budget

defaultShrinkBudget :: ShrinkBudget
defaultShrinkBudget = ShrinkBudget 32

maxShrinkBudget :: Int
maxShrinkBudget = 256

parseShrinkBudget :: String -> Either String ShrinkBudget
parseShrinkBudget text = case readDec text of
  [(value, "")] | value <= toInteger (maxBound :: Int) -> mkShrinkBudget (fromInteger value)
  _ -> Left ("shrink budget must be 1.." ++ show maxShrinkBudget ++ ": " ++ text)

data ReductionStatus = FixedPoint | BudgetExhausted
  deriving (Eq, Show)

reductionStatusText :: ReductionStatus -> String
reductionStatusText FixedPoint = "fixed-point"
reductionStatusText BudgetExhausted = "budget-exhausted"

data Reduction = Reduction
  { reductionOriginal :: Row
  , reductionReduced :: Row
  , reductionTrials :: Int
  , reductionStatus :: ReductionStatus
  }
  deriving (Eq, Show)

-- | Reduces @row@ while @reproduces@ accepts candidates. A candidate is accepted only when it is
-- valid and lower in 'rowMeasure', so the walk terminates at a fixed point or the budget.
shrinkRow :: ShrinkBudget -> (Row -> IO Bool) -> Row -> IO Reduction
shrinkRow (ShrinkBudget budget) reproduces original = go budget 0 original
  where
    go remaining trials current
      | remaining <= 0 = pure (Reduction original current trials BudgetExhausted)
      | otherwise = tryCandidates remaining trials current (reduceCandidates current)
    tryCandidates _remaining trials current [] = pure (Reduction original current trials FixedPoint)
    tryCandidates remaining trials current (candidate : rest) = do
      holds <- reproduces candidate
      let remaining' = remaining - 1
          trials' = trials + 1
      if holds
        then
          if remaining' <= 0
            then pure (Reduction original candidate trials' BudgetExhausted)
            else go remaining' trials' candidate
        else case rest of
          [] -> pure (Reduction original current trials' FixedPoint)
          _
            | remaining' <= 0 -> pure (Reduction original current trials' BudgetExhausted)
            | otherwise -> tryCandidates remaining' trials' current rest

-- | The declared reduction order: task record, entry flags, F, mode, then context form.
rowMeasure :: Row -> (Int, Int, Int, Int, Int)
rowMeasure row =
  ( taskSlotOf row
  , popCount (entryFlags entry)
  , fromEnum (entryFiqMasked entry)
  , fromEnum (entryMode entry)
  , if rowContext row == 0 then 0 else 1
  )
  where
    entry = rowEntry row

reduceCandidates :: Row -> [Row]
reduceCandidates row = nub (filter smaller (taskCandidates ++ entryCandidates))
  where
    smaller candidate = validRow candidate && rowMeasure candidate < rowMeasure row
    entry = rowEntry row
    taskCandidates = case rowTask row of
      TaskAt offset ->
        let slot = offset `div` recordBytes
         in [ row {rowTask = TaskAt 0}
            , row {rowTask = TaskAt (recordBytes * (slot `div` 2))}
            , row {rowTask = TaskAt (recordBytes * (slot - 1))}
            ]
      _ -> []
    entryCandidates =
      [row {rowEntry = entry {entryFlags = 0}} | entryFlags entry /= 0]
        ++ [row {rowEntry = entry {entryFiqMasked = False}} | entryFiqMasked entry]
        ++ [row {rowEntry = entry {entryMode = Supervisor}} | entryMode entry /= Supervisor]
        ++ [row {rowContext = 0} | rowContext row /= 0]
