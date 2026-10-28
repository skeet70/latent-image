module Main (main) where

import Control.Monad (unless)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.Process (CreateProcess (..), createProcess, proc, waitForProcess)

main :: IO ()
main = do
  harnesses <- getArgs
  unless (not (null harnesses)) (fail "no compiled X-M1 test harnesses were supplied")
  codes <- mapM run harnesses
  unless (all (== ExitSuccess) codes) exitFailure
  where
    run harness = do
      (_, _, _, process) <- createProcess (proc harness ["--test-threads", "1"])
      waitForProcess process
