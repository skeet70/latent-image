module Main (main) where

import Replacement.AfResult.PoolSuite (Mode (Qualification))
import SuiteMain (runMain)

main :: IO ()
main = runMain Qualification
