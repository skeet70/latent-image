{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import Data.List (nub)
import Emulation.ScenarioGeneration

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

testBounds :: SequenceBounds
testBounds = either (error . show) id (mkSequenceBounds 3 5)

-- A scenario is a list of drawn small integers.
testGenerator :: Generator [Int] Int
testGenerator =
  Generator
    { generatorVersion = GeneratorVersion "scenario-generation-test/1"
    , generatorBounds = testBounds
    , generatorEvents = id
    , generatorScenario = \_ _ bounds rng -> drawScenario bounds rng
    }

drawScenario :: SequenceBounds -> Rng -> [Int]
drawScenario bounds rng0 =
  let (lengthDraw, rng1) = drawWord64 rng0
      width = maximumEvents bounds - minimumEvents bounds + 1
      count = minimumEvents bounds + fromIntegral (lengthDraw `mod` fromIntegral width)
   in fst (drawItems count rng1)
  where
    drawItems 0 rng = ([], rng)
    drawItems n rng =
      let (item, rng') = drawElement [0 .. 9] rng
          (rest, rng'') = drawItems (n - 1) rng'
       in (item : rest, rng'')

main :: IO ()
main = do
  let generated = generateScenarios testGenerator (ScenarioSeed 42) 6
  corpus <- case generated of
    Left problem -> error (show problem)
    Right value -> pure value
  assert "the corpus records its seed" (generatedSeed corpus == ScenarioSeed 42)
  assert "the corpus records its generator version" (generatedVersion corpus == GeneratorVersion "scenario-generation-test/1")
  assert "the corpus records its bounds" (generatedBounds corpus == testBounds)
  assert "the corpus holds the requested count" (length (generatedScenarios corpus) == 6)
  assert
    "every scenario stays inside the event bounds"
    ( all
        (\scenario -> length scenario >= minimumEvents testBounds && length scenario <= maximumEvents testBounds)
        (generatedScenarios corpus)
    )
  let again = generateScenarios testGenerator (ScenarioSeed 42) 6
  assert "the same seed reproduces the same corpus" (again == Right corpus)
  let other = generateScenarios testGenerator (ScenarioSeed 43) 6
  assert "a different seed changes the corpus" (other /= Right corpus)
  assert
    "different case indices draw different streams"
    (length (nub [fst (drawWord64 (rngFor (ScenarioSeed 42) index)) | index <- [0 .. 5]]) == 6)
  assert "scenario counts below one are refused" (generateScenarios testGenerator (ScenarioSeed 1) 0 == Left (InvalidScenarioCount 0))
  assert "scenario counts above the limit are refused" (generateScenarios testGenerator (ScenarioSeed 1) 65 == Left (InvalidScenarioCount 65))
  assert "the scenario count limit is sixty-four" (scenarioCountLimit == 64)
  assert "a negative minimum bound is refused" (mkSequenceBounds (-1) 3 == Left (InvalidSequenceBounds (-1) 3))
  assert "an inverted bound pair is refused" (mkSequenceBounds 5 3 == Left (InvalidSequenceBounds 5 3))
  assert
    "a generator outside its bounds is refused"
    ( generateScenarios
        (testGenerator {generatorBounds = either (error . show) id (mkSequenceBounds 4 5), generatorScenario = \_ _ _ _ -> [0]})
        (ScenarioSeed 1)
        1
        == Left (GeneratedSequenceOutOfBounds 0 1)
    )
  putStrLn "scenario generation tests passed"
