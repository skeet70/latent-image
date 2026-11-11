{-# LANGUAGE GHC2021 #-}
module TransactionScenarios (transactionScenarios) where

import Data.Bits (shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import Scenarios

transactionScenarios :: B.ByteString -> [Scenario]
transactionScenarios image = simpleCases ++ interruptionCases ++ cancelAllCases ++ entryCases ++ waitCases
 where
  make name function arguments initial final result calls = Scenario name function arguments (base++initial) final result calls
  simpleRequest mode = readWord image (readWord image (0xea1ee0+mode*8))
  simpleCases = [make ("transition-simple-"++show mode) Transition [context,simpleRequest mode] [word (context+0x48) mode] [] (Just 6) [] | mode <- [0,1]]
  interruptionCases = concat [interruption mode | mode <- [0,1]]
  interruption mode =
    let request = simpleRequest mode
        plan = request : drop 1 planValues
        prefix = take 3 (beginCalls plan)
        mutate phase = case objectCall request phase (plan!!5) of
          Call address inputs outputs _ -> Call address inputs outputs [word (context+0x48) mode]
          _ -> error "object call is not a scalar contract"
        common = [wordsAt params plan]
        final = [word (context+0x48) mode]
     in [make ("begin-refuse-before-"++show mode) Begin [context,params] (common++[word (context+0x48) mode]) final (Just 6) prefix
        ,make ("begin-refuse-after-prepare-"++show mode) Begin [context,params] common final (Just 6) (prefix++[mutate 0])
        ,make ("begin-refuse-after-open-"++show mode) Begin [context,params] common final (Just 6) (prefix++[objectCall request 0 (plan!!5),mutate 1,objectCall request 3 (plan!!5)])]
  cancelAllCases = [make ("cancel-all-"++show current) CancelAll [context] [word (state+0x38) current] [word (context+0x4c) 0xffffffff] Nothing ((if current >= 0xe2 && current <= 0xf8 && even current then [call 0x724558 [11,0xffffffff] 0] else [])++[ScriptCall (Address 0xeb0e48) [(N.R0,context+0x4c),(N.R2,1)] [(N.R0,0)] [Relative N.R1 0 (wordBytes 0xffffffff)] [Absolute (word (context+0x4c) 0xffffffff)]]) | current <- [0,0xe2,0xe3,0xf8,0xfa]]
  diagnostic :: Word32 -> Word32 -> Call
  diagnostic phase current =
    let (pointer,withFrame) = if phase == 0 then (0xea20cc :: Word32,True) else if phase == 5 then case current of
          0xe2 -> (0xea2120,True);0xe4 -> (0xea2130,True);0xe6 -> (0xea2148,False);0xe8 -> (0xea2164,True);0xea -> (0xea2178,False);0xec -> (0xea2190,True);0xee -> (0xea21a4,True);0xf0 -> (0xea21bc,True);0xf6 -> (0xea21d4,True);0xf8 -> (0xea21e8,True);_ -> (0xea21fc,True)
          else case current of
            0xe2 -> (0xea2210,True);0xe4 -> (0xea2224,False);0xe6 -> (0xea223c,False);0xe8 -> (0xea2258,True);0xea -> (0xea226c,False);0xec -> (0xea2284,True);0xee -> (0xea2298,True);0xf0 -> (0xea22b0,False);0xf6 -> (0xea22c8,True);0xf8 -> (0xea22dc,True);_ -> (0xea22f0,True)
        text = B.takeWhile (/=0) (B.drop (fromIntegral (pointer-0xbf1a00)) image) <> (if withFrame then BC.pack "0000_0000" else B.empty) <> B.singleton 0
     in ScriptCall (Address 0x726658) [(N.R0,11)] [(N.R0,0)] [Relative N.R1 0 text] []
  entryCases = [entryCase subsystem task | subsystem <- [0,1,2,3,4,5],task <- [11,0x45]]
  entryCase subsystem task =
    let parameters = [0xffffffff,callback,subsystem,0,0,0,task,0,0,0,0,0,0]
        delivered = [wordsAt (context+4) (take 6 parameters),wordsAt (context+0x34) (take 3 (drop 6 parameters)),wordsAt (context+0x1c) (drop 9 parameters)]
        finish = if subsystem == 5 then [call 0x724638 [] 0] else (case subsystem of 0 -> [call 0xe90628 [0] 0];1 -> [call 0xe90508 [0] 0];3 -> [call 0xe8fbe4 [0] 0];4 -> [call 0xe8fee4 [0] 0];_ -> [])++[callbackCall 0xfe 0]
        observedDiagnostic = case diagnostic 0 0 of
          ScriptCall address _ outputs memoryReads memoryWrites -> ScriptCall address [(N.R0,task)] outputs memoryReads memoryWrites
          _ -> error "diagnostic call has no text contract"
     in make ("entry-receive-"++show subsystem++"-task-"++show task) Entry [9] [wordsAt (context+0x2000) (context:parameters)] (delivered++[word (context+0x3000) 2,word (context+0x40) 0]) Nothing (observedDiagnostic:finish)
  waitPlan step = case [pointer | index <- [0..126],let pointer=0xea6738+index*52,readWord image (pointer+28)==step] of
    pointer:_ -> [readWord image (pointer+field*4) | field <- [0..12]]
    [] -> error "wait step has no stock descriptor"
  waitCases = concat [waitStep step | step <- [0..11]]
  waitStep step =
    let descriptor = waitPlan step
        normal = descriptor!!3
        cancelled = descriptor!!4
        mask = 1 `shiftL` fromIntegral step
        cleared = callbackCall (descriptor!!1) 0
        cancelledCalls = [callbackCall (descriptor!!2) 0,call 0x724638 [] 0]
        waiting = diagnostic 5 normal
        cancelCheck = diagnostic 6 normal
        wait signal = ScriptCall (Address 0x7243a8) [(N.R0,descriptor!!8),(N.R2,0xffffffff)] [(N.R0,0)] [] [Absolute (word (context+0x4c) signal)]
     in [make ("wait-normal-"++show step) Wait [context,step] [] [word (state+0x38) normal] (Just 0) [cleared,waiting,wait 0,cancelCheck,call 0x724638 [] 0]
        ,make ("wait-cancel-before-"++show step) Wait [context,step] [word (context+0x4c) mask] [word (state+0x38) cancelled] (Just 0) (cleared:cancelledCalls)
        ,make ("wait-cancel-during-"++show step) Wait [context,step] [] [word (context+0x4c) mask,word (state+0x38) cancelled] (Just 0) ([cleared,waiting,wait mask,cancelCheck]++cancelledCalls)
        ,make ("wait-check-refusal-"++show step) Wait [context,step] [word (context+0x48) 0] [] (Just 6) []]
