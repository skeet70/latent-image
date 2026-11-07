module Main (main) where

import FujiAF.StorageBudget (checkStorageBudget)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [output, marker] -> checkStorageBudget output >> writeFile marker "admitted\n"
    _ -> ioError (userError "storage-budget-probe OUTPUT MARKER")
