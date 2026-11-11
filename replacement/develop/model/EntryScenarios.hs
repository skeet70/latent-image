{-# LANGUAGE GHC2021 #-}
module EntryScenarios (entryScenarios) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Emulation.NativeObservation as N
import DispatcherScenarios (dispatcherScenarios)
import SaveScenarios (saveScenarios)
import Scenarios

entryScenarios :: B.ByteString -> [Scenario]
entryScenarios image = [pipeline,save]
 where
  choose name rows = case filter ((==name) . scenarioName) rows of current:_ -> current; [] -> error ("missing entry component: "++name)
  pipelineSource = choose "dispatch-complete-status-0-reentries-1" (dispatcherScenarios image)
  saveSource = choose "save-step-callback-0-cancel-no-sub-0-kind-2" (saveScenarios image)
  parameters kind = [kind,callback,5,0,0,0,0x45,0,0,0,0,0,0]
  received kind = [wordsAt (context+4) (take 6 (parameters kind)),wordsAt (context+0x34) (take 3 (drop 6 (parameters kind))),wordsAt (context+0x1c) (drop 9 (parameters kind))]
  diagnostic = ScriptCall (Address 0x726658) [(N.R0,0x45)] [(N.R0,0)] [Relative N.R1 0 (BC.pack "Recbf_RcvMsg_0000_0000\0")] []
  slotPatch (Patch (Address address) bytes) = Patch (Address (if address == state || address == state+0x38 then state+0xe8 else address)) bytes
  argument (register,value) = (register,if value == state+0x38 then state+0xe8 else value)
  contract (Absolute patch) = Absolute (slotPatch patch)
  contract other = other
  adapt (Call address inputs outputs writes) = Call address (map argument inputs) outputs (map slotPatch writes)
  adapt (ScriptCall address inputs outputs memoryReads memoryWrites) = ScriptCall address (map argument inputs) outputs (map contract memoryReads) (map contract memoryWrites)
  initial kind source = map slotPatch (scenarioInitial source) ++ [word (context+4) kind,word (context+0x34) 0x45,word state 0xabcdef01,wordsAt (context+0x2000) (context:parameters kind)]
  observed kind final = received kind ++ map slotPatch final ++ [word (context+0x40) 0,word (context+0x3000) 2]
  pipeline = Scenario "entry-pipeline-complete" Entry [9] (initial 10 pipelineSource) (observed 10 (scenarioFinal pipelineSource)) Nothing (diagnostic:map adapt (scenarioCalls pipelineSource))
  prologue = [call 0x7235d8 [] 0x33,call 0x7236b0 [0x33] 0,call 0x724638 [] 0,call 0xe4eca8 [state] 0]
  trace = case [index | index <- [0..50],readWord image (0xea5678+index*16)==48] of
    index:_ -> call 0x42188 [readWord image (0xea5680+index*16)] 0
    [] -> error "raw-save entry has no finish trace"
  save = Scenario "entry-save-callback-complete" Entry [9] (initial 48 saveSource ++ [word (state+0xe8) 0x100])
    (observed 48 (filter (\(Patch (Address address) _) -> address /= context+0x1800) (scenarioFinal saveSource))) Nothing
    ([diagnostic,call 0x724638 [] 0]++prologue++map adapt (scenarioCalls saveSource)++prologue++[trace,callbackCall 0xfe 0])
