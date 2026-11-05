module Main (main) where

import Control.Monad (unless)
import Data.List (isPrefixOf)
import Replacement.Image5Init.Oracle (caseCount, runOracle)
import System.Directory (makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,owned,symbols,stock,output] -> do
      report <- makeAbsolute output
      unless ("/private-data/scratch/" `contains` report && not ("/../" `contains` report))
        (fail "report must be under private-data/scratch/")
      passed <- runOracle runner stock owned symbols (report ++ ".cases") report
      putStrLn (show caseCount ++ " cases; verdict " ++ (if passed then "pass" else "fail") ++ "; report " ++ report)
      unless passed exitFailure
    _ -> fail "usage: image5-init-oracle RUNNER OWNED_BIN OWNED_SYMBOLS STOCK_IMAGE5 REPORT_JSON"
  where
    contains needle haystack = any (needle `isPrefixOf`) (tails haystack)
    tails [] = [[]]
    tails xs@(_:rest) = xs : tails rest
