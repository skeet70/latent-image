{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.DeepSeq (force)
import Control.Exception (evaluate)
import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as L
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairDecode (decodeCheckedObservation, decodeControlObservation)
import Emulation.SchedulerPairEvidence
  ( qualifyEvidence, evidenceTotals, evidenceLimit, evidenceTimerTotal, evidenceDeferredTotal
  , evidenceFailureValue, qualifyControlEvidence, checkControlEvidence, controlFailureValue )
import Emulation.SchedulerPairFamily (Pending (..))
import Emulation.SchedulerPairPlan
import Emulation.Sha256 (sha256, sha256Hex)
import qualified FujiAF.Host as H
import System.Environment (getArgs)
import System.IO (hPutStrLn, stderr)

data QualifiedObservation = QualifiedObservation
  { qualifiedRoute :: Observation
  , qualifiedSummary :: J.Value
  , qualifiedTimers :: Int
  , qualifiedDeferred :: Int
  , qualifiedLimit :: Maybe (BudgetKind, Int, Int)
  }

load :: B.ByteString -> FilePath -> FilePath -> IO QualifiedObservation
load observer planPath observationPath = do
  bytes <- B.readFile planPath
  plan <- either H.failH pure (J.eitherDecodeStrict bytes)
  (observation, evidence) <- L.readFile observationPath >>= either H.failH pure . decodeCheckedObservation plan
  let recorded = drop 5 (obsInputs observation)
  unless (recorded == [sha256Hex (sha256 observer)])
    (H.failH ("observer identity differs: observation records " ++ show recorded))
  let (familyPending, otherPending) = obsPending observation
      counts = "entries=" ++ show (obsEntries observation) ++ ",returns=" ++ show (obsReturns observation)
        ++ ",pending=" ++ show (length familyPending + length otherPending)
        ++ ",instructions=" ++ show (obsInstructions observation)
        ++ ",family=" ++ show [(pendingSemantic pending, pendingEntered pending) | pending <- familyPending]
        ++ ",other=" ++ show otherPending
        ++ ",classes=" ++ show (completedCallCount (evidenceTotals evidence))
        ++ ",timers=" ++ show (evidenceTimerTotal evidence)
        ++ ",deferred=" ++ show (evidenceDeferredTotal evidence)
        ++ ",limit=" ++ show (evidenceLimit evidence)
  result <- either (\refusal -> H.failH (show refusal ++ ": " ++ counts ++ ": " ++ L.unpack (J.encode (if obsRole observation==planRole plan then evidenceFailureValue evidence else J.Null))))
    pure (qualifyEvidence bytes plan observation evidence)
  checkedSummary <- evaluate (force result)
  pure (QualifiedObservation observation checkedSummary (evidenceTimerTotal evidence) (evidenceDeferredTotal evidence) (evidenceLimit evidence))

report :: QualifiedObservation -> IO ()
report qualified = hPutStrLn stderr ("scheduler-pair-check totals timers=" ++ show (qualifiedTimers qualified)
  ++ " deferred=" ++ show (qualifiedDeferred qualified) ++ " limit=" ++ show (qualifiedLimit qualified))

main :: IO ()
main = do
  args <- getArgs
  (observerPath, command) <- case args of
    "--observer":path:rest | not (null rest) -> pure (path, rest)
    _ -> H.failH "usage: scheduler-pair-check --observer PATH qualify PLAN OBSERVATION | compare STOCK_PLAN STOCK_OBSERVATION OWNED_PLAN OWNED_OBSERVATION | control PLAN OBSERVATION"
  observer <- B.readFile observerPath
  result <- case command of
    ["qualify",planPath,observationPath] -> do
      qualified <- load observer planPath observationPath
      report qualified
      pure (qualifiedSummary qualified)
    ["compare",stockPlan,stockObservation,ownedPlan,ownedObservation] -> do
      stock <- load observer stockPlan stockObservation
      owned <- load observer ownedPlan ownedObservation
      report stock
      report owned
      either (H.failH . show) pure (compareRoutes (qualifiedRoute stock) (qualifiedRoute owned))
      pure (J.object ["stock" J..= qualifiedSummary stock,"owned" J..= qualifiedSummary owned])
    ["control",planPath,observationPath] -> do
      bytes <- B.readFile planPath
      plan <- either H.failH pure (J.eitherDecodeStrict bytes)
      (observation, evidence) <- L.readFile observationPath >>= either H.failH pure . decodeControlObservation plan
      let expected = controlRefusals (planRole plan)
      unless (not (null expected)) (H.failH "not a control role")
      let recorded = drop 5 (obsInputs observation)
      unless (recorded == [sha256Hex (sha256 observer)])
        (H.failH ("observer identity differs: observation records " ++ show recorded))
      case qualifyControlEvidence bytes plan observation evidence of
        Left refusal | refusal `elem` expected -> do
          either (H.failH . show) pure (checkControlEvidence (planRole plan) observation evidence)
          pure (J.object ["control" J..= roleName (planRole plan),"refused" J..= show refusal,"first_semantic_failure" J..= controlFailureValue evidence])
        Left refusal -> H.failH ("control refused for wrong reason: " ++ show refusal)
        Right _ -> H.failH "control passed"
    _ -> H.failH "usage: scheduler-pair-check --observer PATH qualify PLAN OBSERVATION | compare STOCK_PLAN STOCK_OBSERVATION OWNED_PLAN OWNED_OBSERVATION | control PLAN OBSERVATION"
  L.putStrLn (J.encode result)
