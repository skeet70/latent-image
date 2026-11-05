{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as L
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairPlan
import qualified FujiAF.Host as H
import System.Environment (getArgs)

load :: FilePath -> FilePath -> IO Observation
load planPath observationPath = do
  bytes <- B.readFile planPath
  plan <- either H.failH pure (J.eitherDecodeStrict bytes)
  observation <- B.readFile observationPath >>= either H.failH pure . J.eitherDecodeStrict
  either (\refusal -> H.failH (show refusal ++ ": " ++ L.unpack (J.encode (failureValue observation)))) (const (pure observation)) (qualify bytes plan observation)
main :: IO ()
main = do
  args <- getArgs
  result <- case args of
    ["qualify",planPath,observationPath] -> summary <$> load planPath observationPath
    ["compare",stockPlan,stockObservation,ownedPlan,ownedObservation] -> do
      stock <- load stockPlan stockObservation
      owned <- load ownedPlan ownedObservation
      either (H.failH . show) pure (compareRoutes stock owned)
      pure (J.object ["stock" J..= summary stock,"owned" J..= summary owned])
    ["control",planPath,observationPath] -> do
      bytes <- B.readFile planPath
      plan <- either H.failH pure (J.eitherDecodeStrict bytes)
      observation <- B.readFile observationPath >>= either H.failH pure . J.eitherDecodeStrict
      let expected = case planRole plan of Restored -> Just Readback; SilentNotify -> Just BankWrites; WrongPredicate -> Just ReturnValue; _ -> Nothing
      unless (expected /= Nothing) (H.failH "not a control role")
      case qualify bytes plan observation of
        Left refusal | Just refusal == expected -> do
          either (H.failH . show) pure (controlFailure (planRole plan) observation)
          pure (J.object ["control" J..= roleName (planRole plan),"refused" J..= show refusal,"first_semantic_failure" J..= failureValue observation])
        Left refusal -> H.failH ("control refused for wrong reason: " ++ show refusal)
        Right _ -> H.failH "control passed"
    _ -> H.failH "usage: scheduler-pair-check qualify PLAN OBSERVATION | compare STOCK_PLAN STOCK_OBSERVATION OWNED_PLAN OWNED_OBSERVATION | control PLAN OBSERVATION"
  L.putStrLn (J.encode result)
