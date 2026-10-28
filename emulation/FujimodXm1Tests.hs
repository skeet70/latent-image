module Main (main) where

import Control.Monad (forM, unless)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeDirectory, (</>))
import System.Process (createProcess, proc, waitForProcess)

main :: IO ()
main = do
  [manifest] <- getArgs
  names <- lines <$> readFile manifest
  let harnesses = [takeDirectory manifest </> name | name <- names, not (null name)]
  unless (not (null harnesses)) (fail "the harness manifest names no tests")
  codes <- forM harnesses $ \harness -> do
    (_, _, _, process) <- createProcess (proc harness ["--test-threads", "1"])
    waitForProcess process
  unless (all (== ExitSuccess) codes) exitFailure
