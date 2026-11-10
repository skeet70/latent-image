{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.SchedulerPairDecode as D
import Emulation.SchedulerPairPlan (Plan)
import qualified FujiAF.Host as H
import Replacement.RtosTaskRecords.SystemCheck
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments<-getArgs
  case arguments of
    [stockPlanPath,ownedPlanPath,stockPath,ownedPath,output]->do
      stockBytes<-B.readFile stockPlanPath
      ownedBytes<-B.readFile ownedPlanPath
      stockPlan<-parse stockBytes
      ownedPlan<-parse ownedBytes
      (stock,stockEvidence)<-either H.failH pure . D.foldObservation (accumulateSystem (stockPlan::Plan)) emptySystemEvidence=<<L.readFile stockPath
      (owned,ownedEvidence)<-either H.failH pure . D.foldObservation (accumulateSystem ownedPlan) emptySystemEvidence=<<L.readFile ownedPath
      let stockResult=qualifySystemEvidence stockBytes stockPlan stock stockEvidence
          ownedResult=qualifySystemEvidence ownedBytes ownedPlan owned ownedEvidence
          comparison=compareSystemEvidence stock stockEvidence owned ownedEvidence
          fullGate=stockResult >> ownedResult >> comparison
          accepted=case fullGate of Right ()->True; Left _->False
      L.writeFile output (J.encode (J.object
        [ "accepted" J..= accepted, "stock" J..= receipt stockResult
        , "owned" J..= receipt ownedResult, "observed_sequence_comparison" J..= receipt comparison
        , "full_gate" J..= receipt fullGate
        ]))
      either (H.failH . show) pure fullGate
    _->H.failH "usage: task-records-system-check STOCK_PLAN OWNED_PLAN STOCK_OBSERVATION OWNED_OBSERVATION OUT_JSON"
 where
  parse bytes=either H.failH pure (J.eitherDecodeStrict' bytes)

receipt :: J.ToJSON a => Either SystemRefusal a -> J.Value
receipt (Left refusal)=J.object ["accepted" J..= False,"refusal" J..= show refusal]
receipt (Right evidence)=J.object ["accepted" J..= True,"evidence" J..= evidence]
