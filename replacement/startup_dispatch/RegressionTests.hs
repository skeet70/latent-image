module Main where

import Control.Monad (forM, unless)
import Fixtures (Fault (..), carrier)
import Replacement.StartupDispatch.Artifact

main :: IO ()
main = do
  outcomes <- forM [Control .. UnknownWordLoad] $ \fault -> do
    let (bytes,symbols) = carrier fault
        result = deriveArtifact bytes symbols
        expected = if fault == Control then "admitted" else "CodeRejected"
        passed = case (fault,result) of
          (Control,Right _) -> True
          (Control,Left _) -> False
          (_,Left (CodeRejected _)) -> True
          _ -> False
        actual = either show (const "admitted") result
    putStrLn (show fault ++ ": expected=" ++ expected ++ "; actual=" ++ actual)
    pure passed
  unless (and outcomes) (fail "startup dispatcher admission regressions failed")
