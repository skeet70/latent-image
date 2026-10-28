module Main where

import FujiAF.Host (jsonPrint, n, obj, s)
import Replacement.Area3.Qualify
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)

main :: IO ()
main = do
  arguments <- getArgs
  case parseOptions arguments of
    Left message -> do
      jsonPrint (obj [("schema_version", n 1), ("status", s "refused"), ("reason", s message)])
      exitWith (ExitFailure 2)
    Right options -> do
      (code, report) <- runQualification options
      jsonPrint report
      exitWith (if code == 0 then ExitSuccess else ExitFailure code)
