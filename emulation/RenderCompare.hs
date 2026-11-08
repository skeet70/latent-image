module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless)
import Emulation.RenderComparison (compareRenderReceiptsWithExclusions)
import System.Environment (getArgs, lookupEnv)
import System.Exit (die, exitFailure)
import System.FilePath (isAbsolute, (</>))

main :: IO ()
main = do
  arguments <- getArgs
  (reference, candidate, output, exclusions) <- case arguments of
    [before, after, destination] -> pure (before, after, destination, Nothing)
    [before, after, destination, "--clock-exclusions", path] -> pure (before, after, destination, Just path)
    _ -> die "usage: render-compare REFERENCE_E2E_RESULT CANDIDATE_E2E_RESULT NEW_OUTPUT_JSON [--clock-exclusions JSON]; relative artifact paths resolve beside their referencing JSON document"
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let resolve path = if isAbsolute path then path else maybe path (</> path) workspace
  outcome <- try (compareRenderReceiptsWithExclusions (resolve reference) (resolve candidate) (resolve output) (resolve <$> exclusions)) :: IO (Either IOException Bool)
  case outcome of
    Left problem -> die ("render-compare: " ++ show problem)
    Right equal -> unless equal exitFailure
