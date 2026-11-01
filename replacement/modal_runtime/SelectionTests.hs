module Main where

import Control.Monad (unless)
import Replacement.ModalRuntime.Qualify (parseCaseSelection)
import System.Exit (exitFailure)

main :: IO ()
main = do
  unless (accepted [] && accepted ["mr-negative-state-abi", "mr-reentry", "mr-action-nonpositive", "control-abi"]) exitFailure
  unless (all refused [["mr-reentry", "mr-reentry"], ["missing-case"], ["control-abi"], ["control-return"], ["control-state"], ["control-caller-stack"]]) exitFailure
  where
    accepted names = case parseCaseSelection names of
      Right _ -> True
      Left _ -> False
    refused = not . accepted
