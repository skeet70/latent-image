{-# LANGUAGE GHC2021 #-}
-- | Shared scenario minimisation for the stock-versus-owned comparison loop.
--
-- A unit supplies how a scenario is measured and read as boundary events, how
-- to rebuild it with fewer or simpler events, its non-event reductions, its
-- validity check, and how to run both roles into one projected observation and
-- decide whether two observations differ. It gets back a minimised scenario
-- that still shows the original difference, the differing observation with both
-- values, the trial count and why reduction stopped.
--
-- The strategy here owns the order: whole-sequence deletions from longest to
-- shortest, then simpler events, then the unit's setup reductions. The
-- stopping rule is fixed point or budget exhaustion; a proposal whose trial is
-- inconclusive blocks local minimality instead of being treated as equal.
module Emulation.ScenarioMinimization
  ( -- * Trials
    Trial (..)
  , Difference (..)
  , PairCheck (..)
  , pairTrial
    -- * Budget and stopping
  , ShrinkBudget (..)
  , mkShrinkBudget
  , ReductionStatus (..)
    -- * Strategy
  , ShrinkStrategy (..)
  , proposals
    -- * Result
  , ShrinkResult (..)
  , shrinkScenario
  ) where

-- | One comparison of a scenario on both roles.
data Trial difference block
  = Matches
  | Differs difference
  | Inconclusive block
  deriving (Eq, Show)

-- | The first differing observation, with the value each role produced.
data Difference observation value = Difference
  { differingObservation :: observation
  , stockValue :: value
  , candidateValue :: value
  } deriving (Eq, Show)

-- | What a unit supplies to run a scenario on both roles: the projection of
-- each role's observation, and what makes two projections differ.
data PairCheck scenario observation difference block = PairCheck
  { runRoles :: scenario -> IO (Either block (observation, observation))
  , observationsDiffer :: observation -> observation -> Maybe difference
  }

pairTrial :: PairCheck scenario observation difference block -> scenario -> IO (Trial difference block)
pairTrial check scenario = do
  outcome <- runRoles check scenario
  pure $ case outcome of
    Left block -> Inconclusive block
    Right (stock, candidate) -> case observationsDiffer check stock candidate of
      Just difference -> Differs difference
      Nothing -> Matches

newtype ShrinkBudget = ShrinkBudget Int
  deriving (Eq, Show)

mkShrinkBudget :: Int -> Either String ShrinkBudget
mkShrinkBudget count
  | count >= 1 && count <= 256 = Right (ShrinkBudget count)
  | otherwise = Left "shrink budget must be between 1 and 256"

-- | 'FixedPoint' means every strictly smaller proposal for the final scenario
-- had a complete trial and none reproduced the original difference; minimality
-- is claimed under the supplied reductions only. 'UnresolvedTrials' means at
-- least one such proposal stayed inconclusive. 'BaselineInconclusive' carries
-- the block that stopped the original trial, so neither reproduction nor
-- non-reproduction is established.
data ReductionStatus block
  = FixedPoint
  | UnresolvedTrials
  | BudgetExhausted
  | OriginalDoesNotReproduce
  | BaselineInconclusive block
  deriving (Eq, Show)

-- | The unit's reductions. 'scenarioSize' is compared lexicographically and
-- every accepted proposal must strictly decrease it. 'scenarioEvents' is the
-- boundary sequence deletions and event simplifications act on.
data ShrinkStrategy scenario event difference = ShrinkStrategy
  { scenarioSize :: scenario -> (Int, Int, Int)
  , scenarioEvents :: scenario -> [event]
  , setScenarioEvents :: [event] -> scenario -> Maybe scenario
  , simplifyEvent :: event -> [event]
  , setupReductions :: scenario -> [scenario]
  , prepareScenario :: scenario -> Maybe scenario
  , reproducesDifference :: difference -> difference -> Bool
  }

-- | The strategy's proposal order: shorter sequences first (longest deletion
-- first), then one simpler event, then the unit's setup reductions.
proposals :: ShrinkStrategy scenario event difference -> scenario -> [scenario]
proposals strategy scenario = deletions ++ simplifications ++ setup
  where
    events = scenarioEvents strategy scenario
    count = length events
    deletions =
      [ candidate
      | size <- [count, count - 1 .. 1]
      , start <- [0 .. count - size]
      , Just candidate <- [setScenarioEvents strategy (take start events ++ drop (start + size) events) scenario]
      ]
    simplifications =
      [ candidate
      | (index, event) <- zip [0 ..] events
      , replacement <- simplifyEvent strategy event
      , Just candidate <-
          [ setScenarioEvents strategy
              (take index events ++ [replacement] ++ drop (index + 1) events)
              scenario
          ]
      ]
    setup = setupReductions strategy scenario

data ShrinkResult scenario difference block = ShrinkResult
  { minimizedScenario :: scenario
  , minimizedDifference :: Maybe difference
  , trialsUsed :: Int
  , reductionStatus :: ReductionStatus block
  } deriving (Eq, Show)

shrinkScenario
  :: ShrinkBudget
  -> ShrinkStrategy scenario event difference
  -> scenario
  -> (scenario -> IO (Trial difference block))
  -> IO (ShrinkResult scenario difference block)
shrinkScenario (ShrinkBudget budget) strategy original oracle = do
  baseline <- oracle original
  case baseline of
    Differs difference
      | reproducesDifference strategy difference difference -> descend difference 1 original (Just difference)
    Inconclusive block -> pure (ShrinkResult original Nothing 1 (BaselineInconclusive block))
    _ -> pure (ShrinkResult original Nothing 1 OriginalDoesNotReproduce)
  where
    descend originalDifference used current lastDifference =
      search originalDifference used current False (proposals strategy current) lastDifference

    search _originalDifference used current unresolved [] lastDifference =
      pure
        ( ShrinkResult
            current
            lastDifference
            used
            (if unresolved then UnresolvedTrials else FixedPoint)
        )
    search originalDifference used current unresolved (candidate : rest) lastDifference =
      case prepareScenario strategy candidate of
        Nothing -> search originalDifference used current unresolved rest lastDifference
        Just valid
          | scenarioSize strategy valid >= scenarioSize strategy current ->
              search originalDifference used current unresolved rest lastDifference
          | used >= budget -> pure (ShrinkResult current lastDifference used BudgetExhausted)
          | otherwise -> do
              result <- oracle valid
              case result of
                Differs difference
                  | reproducesDifference strategy originalDifference difference ->
                      descend originalDifference (used + 1) valid (Just difference)
                Inconclusive _ -> search originalDifference (used + 1) current True rest lastDifference
                _ -> search originalDifference (used + 1) current unresolved rest lastDifference
