{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import Emulation.ScenarioGeneration
  ( Generator (..)
  , GeneratorVersion (..)
  , Rng
  , ScenarioSeed (..)
  , SequenceBounds (..)
  , drawElement
  , drawWord64
  , generateScenarios
  , generatedScenarios
  , mkSequenceBounds
  )
import Emulation.ScenarioMinimization

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

data Event = Push Int | Emit Int | Sync
  deriving (Eq, Show)

type Scenario = [Event]

-- | The synthetic unit's planted difference: the candidate adds one to every
-- emitted value, so any sequence with an emit event differs.
stockEmission :: Scenario -> Int
stockEmission = sum . map emission
  where
    emission (Emit value) = value
    emission _ = 0

candidateEmission :: Scenario -> Int
candidateEmission = sum . map emission
  where
    emission (Emit value) = value + 1
    emission _ = 0

syntheticPair :: PairCheck Scenario Int (Difference String Int) String
syntheticPair =
  PairCheck
    { runRoles = \scenario -> pure (Right (stockEmission scenario, candidateEmission scenario))
    , observationsDiffer = \stock candidate ->
        if stock == candidate then Nothing else Just (Difference "emitted-total" stock candidate)
    }

alphabet :: [Event]
alphabet = [Push 0, Push 1, Push 2, Emit 0, Emit 1, Emit 2, Sync]

syntheticBounds :: SequenceBounds
syntheticBounds = either (error . show) id (mkSequenceBounds 3 6)

syntheticGenerator :: Generator Scenario Event
syntheticGenerator =
  Generator
    { generatorVersion = GeneratorVersion "synthetic-unit/1"
    , generatorBounds = syntheticBounds
    , generatorEvents = id
    , generatorScenario = \_ _ bounds rng -> drawScenario bounds rng
    }

drawScenario :: SequenceBounds -> Rng -> Scenario
drawScenario bounds rng0 =
  let (lengthDraw, rng1) = drawWord64 rng0
      width = maximumEvents bounds - minimumEvents bounds + 1
      count = minimumEvents bounds + fromIntegral (lengthDraw `mod` fromIntegral width)
   in fst (drawEvents count rng1)
  where
    drawEvents 0 rng = ([], rng)
    drawEvents n rng =
      let (event, rng') = drawElement alphabet rng
          (rest, rng'') = drawEvents (n - 1) rng'
       in (event : rest, rng'')

complexity :: Scenario -> Int
complexity = sum . map weight
  where
    weight (Push value) = value
    weight (Emit value) = value
    weight Sync = 0

simplify :: Event -> [Event]
simplify (Push value) = [Push 0 | value /= 0]
simplify (Emit value) = [Emit 0 | value /= 0]
simplify Sync = []

syntheticStrategy :: ShrinkStrategy Scenario Event (Difference String Int)
syntheticStrategy =
  ShrinkStrategy
    { scenarioSize = \scenario -> (length scenario, complexity scenario, 0)
    , scenarioEvents = id
    , setScenarioEvents = \events _ -> Just events
    , simplifyEvent = simplify
    , setupReductions = const []
    , prepareScenario = Just
    , reproducesDifference = \original candidate ->
        differingObservation original == differingObservation candidate
    }

budget256 :: ShrinkBudget
budget256 = either error id (mkShrinkBudget 256)

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft _ = False

main :: IO ()
main = do
  budgetTests
  pairTests
  syntheticUnit
  proposalOrder
  nonreproduction
  baselineBlock
  budgetExhaustion
  unresolvedTrials
  idempotence
  putStrLn "scenario minimization tests passed"

budgetTests :: IO ()
budgetTests = do
  assert "budget zero is refused" (isLeft (mkShrinkBudget 0))
  assert "budget above the maximum is refused" (isLeft (mkShrinkBudget 257))
  assert "budget one is accepted" (not (isLeft (mkShrinkBudget 1)))
  assert "budget maximum is accepted" (not (isLeft (mkShrinkBudget 256)))

pairTests :: IO ()
pairTests = do
  matching <- pairTrial syntheticPair [Push 1, Sync]
  assert "equal projections match" (matching == Matches)
  differing <- pairTrial syntheticPair [Push 1, Emit 2]
  assert
    "differing projections report the observation and both values"
    (differing == Differs (Difference "emitted-total" 2 3))
  blocked <- pairTrial (syntheticPair {runRoles = \_ -> pure (Left "blocked")}) []
  assert "a blocked role run is inconclusive" (blocked == Inconclusive "blocked")

syntheticUnit :: IO ()
syntheticUnit = do
  corpus <- case generateScenarios syntheticGenerator (ScenarioSeed 7) 8 of
    Left problem -> error (show problem)
    Right value -> pure (generatedScenarios value)
  found <- case filter differs corpus of
    scenario : _ -> pure scenario
    [] -> error "generation did not find the planted difference"
  result <- shrinkScenario budget256 syntheticStrategy found (pairTrial syntheticPair)
  let minimised = minimizedScenario result
  assert "generation finds the planted difference" (differs found)
  assert "minimisation reduces to the smallest differing sequence" (minimised == [Emit 0])
  assert "minimisation reaches a fixed point" (reductionStatus result == FixedPoint)
  assert
    "the result reports the first differing observation and both values"
    (minimizedDifference result == Just (Difference "emitted-total" 0 1))
  assert "reduction stays inside its budget" (trialsUsed result <= 256)
  assert "reduction tries more than the baseline" (trialsUsed result > 1)

differs :: Scenario -> Bool
differs scenario = stockEmission scenario /= candidateEmission scenario

proposalOrder :: IO ()
proposalOrder = do
  let ordered = take 3 (proposals syntheticStrategy [Push 2, Emit 3])
  assert "the longest deletion comes first" (ordered == [[], [Emit 3], [Push 2]])

nonreproduction :: IO ()
nonreproduction = do
  result <- shrinkScenario budget256 syntheticStrategy [Push 1] (\_ -> pure (Matches :: Trial (Difference String Int) String))
  assert "a matching baseline does not reproduce" (reductionStatus result == OriginalDoesNotReproduce)
  assert "a matching baseline is the only trial" (trialsUsed result == 1)
  assert "a matching baseline returns its scenario" (minimizedScenario result == [Push 1])
  assert "a matching baseline has no difference" (minimizedDifference result == Nothing)

baselineBlock :: IO ()
baselineBlock = do
  result <- shrinkScenario budget256 syntheticStrategy [Emit 1] (\_ -> pure (Inconclusive "blocked"))
  assert "a blocked baseline is reported with its cause" (reductionStatus result == BaselineInconclusive "blocked")
  assert "a blocked baseline stops after one trial" (trialsUsed result == 1)
  assert "a blocked baseline returns its scenario" (minimizedScenario result == [Emit 1])

budgetExhaustion :: IO ()
budgetExhaustion = do
  budget <- either error pure (mkShrinkBudget 1)
  result <- shrinkScenario budget syntheticStrategy [Emit 1] (pairTrial syntheticPair)
  assert "a budget of one exhausts at the baseline" (reductionStatus result == BudgetExhausted)
  assert "a budget of one counts one trial" (trialsUsed result == 1)
  assert "a budget of one returns the baseline" (minimizedScenario result == [Emit 1])

unresolvedTrials :: IO ()
unresolvedTrials = do
  let oracle scenario
        | scenario == [Emit 1] = pure (Differs (Difference "emitted-total" 1 2))
        | otherwise = pure (Inconclusive "blocked")
  result <- shrinkScenario budget256 syntheticStrategy [Emit 1] oracle
  assert "inconclusive proposals leave minimality unresolved" (reductionStatus result == UnresolvedTrials)
  assert "inconclusive proposals keep the last proven scenario" (minimizedScenario result == [Emit 1])
  assert "inconclusive proposals were tried" (trialsUsed result > 1)

idempotence :: IO ()
idempotence = do
  first <- shrinkScenario budget256 syntheticStrategy [Push 2, Emit 3] (pairTrial syntheticPair)
  second <- shrinkScenario budget256 syntheticStrategy (minimizedScenario first) (pairTrial syntheticPair)
  assert "a fixed point is stable under another run" (reductionStatus second == FixedPoint)
  assert "a fixed point returns the same scenario" (minimizedScenario second == minimizedScenario first)
