module Main (main) where

import Control.Monad (forM_, unless)
import System.Directory (doesPathExist, getTemporaryDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

main :: IO ()
main = do
  [driver] <- getArgs
  temporary <- getTemporaryDirectory
  let output = temporary </> "controller-lifecycle-rejected-selection"
      rejected =
        [ ["--scenario", "not-a-scenario", "--no-controls"]
        , ["--scenario", ""]
        , ["--scenario"]
        , ["--scenario", "--no-controls"]
        , ["--editor", "--scenario", "not-a-scenario"]
        ]
  forM_ rejected $ \arguments -> do
    (code, stdout, _) <- readProcessWithExitCode driver (["--out", output, "--execute"] ++ arguments) ""
    unless (code == ExitFailure 2 && null stdout) $
      fail ("invalid selection did not refuse at CLI admission: " ++ show arguments)
    exists <- doesPathExist output
    unless (not exists) (fail "invalid selection emitted output")
  (accepted, _, _) <- readProcessWithExitCode driver ["--scenario", "leave-generic"] ""
  unless (accepted == ExitFailure 1) (fail "known scenario did not proceed to required input admission")
