module Main (main) where

import Control.Monad (unless)
import Replacement.Image5Init.Oracle (caseCount, runSyntheticOracle)
import System.Environment (getArgs, getEnv)
import System.Exit (exitFailure)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,owned,symbols] -> do
      temporary <- getEnv "TEST_TMPDIR"
      passed <- runSyntheticOracle runner owned symbols (temporary ++ "/oracle") (temporary ++ "/report.json")
      putStrLn (show caseCount ++ " owned/spec cases and swapped-call control: " ++ if passed then "pass" else "fail")
      unless passed exitFailure
    _ -> fail "expected runner, linked image and symbols"
