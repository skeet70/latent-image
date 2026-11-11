module Main (main) where

import FujiAF.Host (b, jsonPrint, n, obj, s)
import Replacement.PropertyService.Qualification
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)

main :: IO ()
main = do
  arguments <- getArgs
  case parseOptions arguments of
    Left message -> do
      jsonPrint (obj [("schema_version", n 1), ("verdict", s "error"), ("native_only", b True), ("qualification_scope", s "staged-candidate"), ("dat_retirement_admitted", b False), ("reason", s message)])
      exitWith (ExitFailure 2)
    Right options -> do
      (code, verdict) <- runQualification options
      jsonPrint verdict
      exitWith (if code == 0 then ExitSuccess else ExitFailure code)
