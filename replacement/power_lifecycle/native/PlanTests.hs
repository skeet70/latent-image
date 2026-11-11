module Main (main) where

import Replacement.PowerLifecycle.RecordHelperSuite (planProblems)
import Replacement.PowerLifecycle.RecordHelperTable (Helper (..), allHelpers)

main :: IO ()
main = do
  print (length allHelpers, sum (map (length . helperCases) allHelpers))
  case planProblems of
    [] -> pure ()
    problems -> fail (unlines problems)
