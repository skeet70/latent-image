module Main (main) where

import Replacement.AfResult.PoolSuite (Mode (Smoke))
import SuiteMain (runMain)

main :: IO ()
main = runMain Smoke
