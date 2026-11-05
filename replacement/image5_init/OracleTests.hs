module Main (main) where

import Control.Monad (unless)
import Replacement.Image5Init.Oracle (caseCount, runOracle)
import System.Environment (getArgs, getEnv)
import System.Exit (exitFailure)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,stock,owned,symbols] -> do
      temporary <- getEnv "TEST_TMPDIR"
      passed <- runOracle runner stock owned symbols (temporary ++ "/oracle") (temporary ++ "/report.json")
      putStrLn (show caseCount ++ " stock/owned cases, spec verdict and swapped-call control: " ++ if passed then "pass" else "fail")
      unless passed exitFailure
    _ -> fail "expected runner, stock fixture, linked image and symbols"
