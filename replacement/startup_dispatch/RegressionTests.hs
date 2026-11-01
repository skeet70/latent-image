module Main where

import Control.Monad (forM, unless)
import Fixtures (Fault (..), carrier)
import Replacement.StartupDispatch.Artifact

main :: IO ()
main = do
  outcomes <- forM [minBound .. maxBound] $ \fault -> do
    let (bytes,symbols) = carrier fault
        result = deriveArtifact bytes symbols
        admits = fault `elem` [Control,ScalarExtend,RotatedExtend,ConditionalExtend]
        expected = if admits then "admitted" else "CodeRejected"
        passed = case result of
          Right _ -> admits
          Left (CodeRejected _) -> not admits
          _ -> False
        actual = either show (const "admitted") result
    putStrLn (show fault ++ ": expected=" ++ expected ++ "; actual=" ++ actual)
    pure passed
  unless (and outcomes) (fail "startup dispatcher admission regressions failed")
