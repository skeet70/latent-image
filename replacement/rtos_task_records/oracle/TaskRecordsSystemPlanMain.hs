module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.SchedulerPairPlan (planValue)
import qualified FujiAF.Host as H
import Replacement.RtosTaskRecords.System (systemPlans)
import qualified Replacement.RtosPlatform.Aggregate as A
import qualified Replacement.RtosPlatform.Artifact as F
import qualified Replacement.RtosPlatform.Union as U
import System.Directory (doesPathExist)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments<-getArgs
  case arguments of
    [carriers,binA,symA,disA,placeA,binB,symB,disB,placeB,dat,loaderA,loaderB,candidate,stockOutput,ownedOutput]->do
      union<-either (H.failH . show) pure . U.parseUnion=<<B.readFile carriers
      sourceDat<-B.readFile dat
      a<-B.readFile loaderA
      b<-B.readFile loaderB
      source<-either (H.failH . show) pure (F.canonicalSource sourceDat a b)
      placementsA<-spans placeA
      placementsB<-spans placeB
      linked<-F.Linked <$> B.readFile binA <*> B.readFile symA <*> B.readFile disA <*> pure placementsA <*> B.readFile binB <*> B.readFile symB <*> B.readFile disB <*> pure placementsB
      candidateDat<-B.readFile candidate
      (stock,owned)<-either H.failH pure (systemPlans union source linked candidateDat)
      present<-mapM doesPathExist [stockOutput,ownedOutput]
      unless (stockOutput/=ownedOutput && not (or present)) (H.failH "plan output exists or repeats")
      L.writeFile stockOutput (J.encode (planValue stock))
      L.writeFile ownedOutput (J.encode (planValue owned))
    _->H.failH "usage: task-records-system-plan CARRIERS BIN_A SYMBOLS_A DISASM_A PLACEMENT_A BIN_B SYMBOLS_B DISASM_B PLACEMENT_B CANONICAL_DAT LOADER_A LOADER_B CANDIDATE_DAT NEW_STOCK_PLAN NEW_OWNED_PLAN"
 where
  spans path=map (\placement->(A.placementAddress placement,fromIntegral (A.placementBytes placement))) <$> (either H.failH pure . A.parsePlacements=<<B.readFile path)
