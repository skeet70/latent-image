module Main (main) where

import Control.Monad (unless)
import Data.List (sort)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.Process (readProcessWithExitCode)
import System.Timeout (timeout)
import Text.Read (readMaybe)

data Candidate = A0Structural | A1 | A2 deriving (Eq, Ord, Show)
data Curve = SinglePeak | TwoDepths | EdgePeak | NoisyPeak | Flat deriving (Eq, Ord, Show)
data Result = Result Curve Candidate

ensure :: Bool -> String -> IO ()
ensure yes message = unless yes (fail message)

number :: String -> IO Integer
number text = maybe (fail "invalid comparison integer") pure (readMaybe text)

result :: String -> IO Result
result line = case words (map (\c -> if c == ',' then ' ' else c) line) of
  ["simulated", "flat", candidate, samples, travel, "FlatCurve"] -> do
    checkCounts samples travel
    Result Flat <$> parseCandidate candidate
  ["simulated", curve, "A0-structural", samples, travel, "Budget"] -> do
    checkCounts samples travel
    name <- case curve of
      "single-peak" -> pure SinglePeak
      "noisy-peak" -> pure NoisyPeak
      _ -> fail "unexpected structural budget refusal"
    pure (Result name A0Structural)
  ["simulated", curve, candidate, samples, travel, err, "focused"] -> do
    checkCounts samples travel
    selected <- parseCandidate candidate
    errorUnits <- number err
    ensure (errorUnits >= 0 && errorUnits <= 10) "synthetic final-position error outside policy contract"
    name <- case curve of
      "single-peak" -> pure SinglePeak
      "two-depths" -> pure TwoDepths
      "edge-peak" -> pure EdgePeak
      "noisy-peak" -> pure NoisyPeak
      _ -> fail "unknown or incorrectly focused curve"
    pure (Result name selected)
  _ -> fail "invalid comparison row or unexpected refusal"
  where
    parseCandidate "A0-structural" = pure A0Structural
    parseCandidate "A1" = pure A1
    parseCandidate "A2" = pure A2
    parseCandidate _ = fail "unknown candidate"
    checkCounts samples travel = do
      count <- number samples
      distance <- number travel
      ensure (count > 0 && count <= 64 && distance >= 0) "comparison accounting outside bounds"

main :: IO ()
main = do
  args <- getArgs
  case args of
    harness : decision : fixtures -> do
      ensure (length fixtures == 5) "expected five runfiles fixtures"
      completed <- timeout 45000000 (readProcessWithExitCode harness (decision : fixtures) "")
      (status, output, errors) <- maybe (fail "comparison smoke deadline") pure completed
      ensure (status == ExitSuccess) ("comparison smoke failed: " ++ errors)
      case lines output of
        header : rows -> do
          ensure (header == "origin,curve,candidate,sample_count,simulated_travel_units,final_position_error_units,outcome") "comparison header"
          results <- mapM result rows
          let observed = sort [(curve, candidate) | Result curve candidate <- results]
              expected = sort [(curve, candidate) | curve <- [SinglePeak, TwoDepths, EdgePeak, NoisyPeak, Flat], candidate <- [A0Structural, A1, A2]]
          ensure (observed == expected) "comparison did not report all candidates for every curve"
        [] -> fail "empty comparison output"
    _ -> fail "pass comparison harness, decision adapter and five fixtures"
