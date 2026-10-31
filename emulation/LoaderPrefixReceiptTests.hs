module Main where

import Emulation.LoaderPrefixReceiptFixtures (runReceiptCases, syntheticProgramFacts)

main :: IO ()
main = do
  facts <- either fail pure syntheticProgramFacts
  runReceiptCases facts
  putStrLn "loader-prefix receipt tests pass"
