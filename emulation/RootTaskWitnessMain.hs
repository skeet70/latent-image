module Main (main) where

import qualified Emulation.RootTaskControls as Controls
import qualified Emulation.RootTaskWitness as Witness
import Emulation.TaskMemoryWire (Role(..))
import qualified Emulation.TaskWitnessCLI as CLI

diagnose :: Show failure => Either failure success -> Either CLI.Diagnostic success
diagnose = either (Left . CLI.Diagnostic . show) Right
main :: IO ()
main = CLI.run CLI.RootBody CLI.Operations
  { CLI.qualifyEvidence = \role source observed -> do
      inputs <- diagnose (Witness.admitInputs role source)
      receipt <- diagnose (Witness.admitReceipt inputs observed)
      pure (Witness.qualificationValue receipt)
  , CLI.runControls = \role source observed -> diagnose (Controls.refusalControls role source observed)
  , CLI.compareEvidence = \firstSource firstObserved secondSource secondObserved -> do
      firstInputs <- diagnose (Witness.admitInputs Retained firstSource)
      secondInputs <- diagnose (Witness.admitInputs Owned secondSource)
      first <- diagnose (Witness.admitReceipt firstInputs firstObserved)
      second <- diagnose (Witness.admitReceipt secondInputs secondObserved)
      diagnose (Witness.compareReceipts first second)
  }
