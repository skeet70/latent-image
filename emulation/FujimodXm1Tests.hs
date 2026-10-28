module Main (main) where

import Control.Monad (filterM, forM, unless)
import Data.List (sort)
import System.Directory (doesFileExist, listDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.Process (CreateProcess (..), createProcess, proc, waitForProcess)

main :: IO ()
main = do
  [manifest] <- getArgs
  let directory = takeDirectory manifest
  entries <- listDirectory directory
  harnesses <- filterM (doesFileExist . (directory </>)) =<< pure (sort (filter (/= takeFileName manifest) entries))
  unless (not (null harnesses)) (fail "no compiled X-M1 test harnesses were supplied")
  codes <- forM harnesses (
ame -> run (directory </> name))
  unless (all (== ExitSuccess) codes) exitFailure
  where
    run harness = do
      (_, _, _, process) <- createProcess (proc harness ["--test-threads", "1"])
      waitForProcess process
