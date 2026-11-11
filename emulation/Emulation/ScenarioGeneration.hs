{-# LANGUAGE GHC2021 #-}
-- | Seeded boundary-scenario generation shared by every unit's comparison loop.
--
-- A unit supplies the version of its generator, the bounds on a scenario's
-- boundary events, the projection from a scenario to those events, and one
-- function that builds a scenario from a seed, a case index, the bounds and a
-- random stream. It gets back the seed, version, bounds and the generated
-- scenarios together, so any result names what regenerates it. The seed to
-- stream derivation and the case loop live here, not in the unit.
module Emulation.ScenarioGeneration
  ( ScenarioSeed (..)
  , GeneratorVersion (..)
  , SequenceBounds (..)
  , mkSequenceBounds
  , GenerationError (..)
  , scenarioCountLimit
  , Rng
  , rngFor
  , drawWord64
  , drawElement
  , Generator (..)
  , GeneratedScenarios (..)
  , generateScenarios
  ) where

import Data.Bits (shiftR, xor)
import Data.Word (Word64)

-- | A generator's input seed. It is recorded with every generated corpus.
newtype ScenarioSeed = ScenarioSeed Word64
  deriving (Eq, Ord, Show)

-- | The unit's generator identity. Bump it whenever a seed starts producing
-- different scenarios, so a recorded seed is never read against the wrong
-- generator.
newtype GeneratorVersion = GeneratorVersion String
  deriving (Eq, Ord, Show)

-- | Inclusive bounds on the number of boundary events in one scenario.
data SequenceBounds = SequenceBounds
  { minimumEvents :: Int
  , maximumEvents :: Int
  } deriving (Eq, Show)

mkSequenceBounds :: Int -> Int -> Either GenerationError SequenceBounds
mkSequenceBounds lower upper
  | lower < 0 = Left (InvalidSequenceBounds lower upper)
  | upper < lower = Left (InvalidSequenceBounds lower upper)
  | otherwise = Right (SequenceBounds lower upper)

data GenerationError
  = InvalidScenarioCount Int
  | InvalidSequenceBounds Int Int
  | GeneratedSequenceOutOfBounds Int Int
  deriving (Eq, Show)

-- | Upper bound on a corpus, matching the record-store loop's accepted count.
scenarioCountLimit :: Int
scenarioCountLimit = 64

-- | A deterministic stream derived from one seed and one case index.
newtype Rng = Rng Word64

rngFor :: ScenarioSeed -> Int -> Rng
rngFor (ScenarioSeed seed) index = Rng (mix64 (seed + fromIntegral index * 0x9e3779b97f4a7c15))

drawWord64 :: Rng -> (Word64, Rng)
drawWord64 (Rng state) = (mix64 state, Rng (state + 0x9e3779b97f4a7c15))

drawElement :: [a] -> Rng -> (a, Rng)
drawElement items rng = (items !! fromIntegral (draw `mod` fromIntegral (length items)), rng')
  where
    (draw, rng') = drawWord64 rng

mix64 :: Word64 -> Word64
mix64 z0 = z2 `xor` (z2 `shiftR` 31)
  where
    z1 = (z0 `xor` (z0 `shiftR` 30)) * 0xbf58476d1ce4e5b9
    z2 = (z1 `xor` (z1 `shiftR` 27)) * 0x94d049bb133111eb

-- | What a unit supplies: its generator identity, the event bounds it works
-- within, how to read a scenario's events, and how to build one scenario.
data Generator scenario event = Generator
  { generatorVersion :: GeneratorVersion
  , generatorBounds :: SequenceBounds
  , generatorEvents :: scenario -> [event]
  , generatorScenario :: ScenarioSeed -> Int -> SequenceBounds -> Rng -> scenario
  }

-- | What the unit gets: everything needed to regenerate the corpus.
data GeneratedScenarios scenario = GeneratedScenarios
  { generatedSeed :: ScenarioSeed
  , generatedVersion :: GeneratorVersion
  , generatedBounds :: SequenceBounds
  , generatedScenarios :: [scenario]
  } deriving (Eq, Show)

generateScenarios :: Generator scenario event -> ScenarioSeed -> Int -> Either GenerationError (GeneratedScenarios scenario)
generateScenarios generator seed count
  | count < 1 || count > scenarioCountLimit = Left (InvalidScenarioCount count)
  | otherwise = do
      scenarios <- mapM build [0 .. count - 1]
      Right
        GeneratedScenarios
          { generatedSeed = seed
          , generatedVersion = generatorVersion generator
          , generatedBounds = bounds
          , generatedScenarios = scenarios
          }
  where
    bounds = generatorBounds generator
    build index =
      let scenario = generatorScenario generator seed index bounds (rngFor seed index)
          events = length (generatorEvents generator scenario)
       in if events >= minimumEvents bounds && events <= maximumEvents bounds
            then Right scenario
            else Left (GeneratedSequenceOutOfBounds index events)
