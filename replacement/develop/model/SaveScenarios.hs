{-# LANGUAGE GHC2021 #-}
module SaveScenarios (saveScenarios) where

import qualified Data.ByteString as B
import qualified Emulation.NativeObservation as N
import Scenarios

saveScenarios :: B.ByteString -> [Scenario]
saveScenarios image = concatMap kindCases [0..4]
 where
  output = context+0x1800
  saveStatus = 0x0154d970
  phaseOne = 0x0155ccb8
  phaseTwo = 0x0155ccbc
  task = 0x45
  plan kind = lookupPlan (case kind of 2 -> 0x32; 4 -> 0x33; 1 -> 0x34; _ -> 0x35) 0
  lookupPlan operation index
    | index > 126 = error "save plan not found"
    | readWord image (0xea6738+index*52) == operation && readWord image (0xea6738+index*52+28) == 13 = [readWord image (0xea6738+index*52+field*4) | field <- [0..12]]
    | otherwise = lookupPlan operation (index+1)
  active kind = base ++ [word (state+0x4b4) (if kind == 4 then 0x1e else 0x19),word (state+0x578) 2,word (state+0x275c) 1,word (state+0x322c) 2,word (state+0x27c8) 1,word output 0x12345678]
  make name kind extra final returned calls = Scenario ("save-step-"++name++"-kind-"++show kind) Save [context,kind,output] (active kind++extra) (word output 1:final) (Just returned) calls
  stateFinal kind index = word (state+0x38) (plan kind!!index)
  numbered first second =
    [ScriptCall (Address 0x7101e4) [(N.R0,selector)] [(N.R0,0)] [] [Relative N.R1 0 (wordBytes value)] | (selector,value) <- [(1,first),(2,second)]]
  trace kind = [call address arguments 0 | kind == 2 || kind == 4, (address,arguments) <- [(0xeb0ccc,[0x06000000]),(0xeb0c24,[1,0x61])]]
  kindCases kind =
    [make "numbering-flag" kind [] [stateFinal kind 4] 0 (beginCalls (plan kind)++numbered 1 0++completeCalls (plan kind) 1)
    ,make "numbering-storage-seven" kind [] [stateFinal kind 4] 0 (beginCalls (plan kind)++numbered 0 7++completeCalls (plan kind) 1)
    ,make "numbering-profile-a8" kind [] [stateFinal kind 4] 0 (beginCalls (plan kind)++numbered 0 0++[call 0x73ac60 [] 0xa8]++completeCalls (plan kind) 1)
    ,make "permit-absent" kind [word (state+0x27c8) 0] [stateFinal kind 4] 2 (beginCalls (plan kind)++numbered 0 0++[call 0x73ac60 [] 0]++trace kind)
    ] ++
    [make ("admission-status-"++show status) kind [word saveStatus status]
      [stateFinal kind 4] (if status == 0 then 0 else if status == 1 then 1 else 2)
      (beginCalls (plan kind)++numbered 0 0++[call 0x73ac60 [] 0,call 0x73abec [] 1,call 0x69f8fc [0xc3] 1,call 0x73ac60 [] 0xa2]++trace kind++if status == 0 then completeCalls (plan kind) 1 else [])
    | status <- [0,1,2,0xffffffff]] ++
    [full kind completion interrupted subframe | completion <- [0,0xfffffffc,1], interrupted <- [False,True], subframe <- [0,1]]
  full kind completion interrupted subframe =
    let bank = if kind == 1 then 1 else if kind >= 2 then 2 else 0
        descriptor = state+0x27d0+bank*0x150
        mode = if kind == 4 then 1 else if kind == 2 || kind == 3 then 3 else 0
        status = if completion == 0 then 0 else if completion == 0xfffffffc then 1 else 2
        secondPhase = interrupted && status == 0
        returned = if status == 0 then 0 else if status == 1 then 1 else 2
        completionResult = if interrupted then 0 else 1
        finalState = stateFinal kind (if status == 0 && interrupted then 3 else 4)
        metadata = if kind == 0 then 1 else 0
        name = if kind == 0 then [call 0x11d5934 [state+0x2f64,descriptor] 0] else []
        copied = if kind == 2 || kind == 4 then [call 0x11d591c [state+0x2f64,descriptor] 0] else []
        signal slot = ScriptCall (Address 0x724558) [(N.R0,task),(N.R1,0x10)] [(N.R0,0)] [Absolute (word slot task),Absolute (word saveStatus status)] []
        wait mask flags slot = ScriptCall (Address 0x7243a8) [(N.R0,mask),(N.R2,0xffffffff)] [(N.R0,0)] [Absolute (word slot task)] [Relative N.R1 0 (wordBytes flags)]
        cached = ScriptCall (Address 0x7101e4) [(N.R0,2)] [(N.R0,0)] [] [Relative N.R1 0 (wordBytes 0xabc)]
        facade = [call 0x73abec [] 0,call 0xe4e0b4 [state,metadata] 0]
          ++ (if subframe == 0 then [call 0x11d5850 [1] 0] else []) ++ name
          ++ [cached,call 0x6fae80 [1,8] 0,call 0x723cb8 [] task,signal phaseOne,wait 0x30 (if interrupted then 0x20 else 0) phaseOne,call 0x6fae80 [1,10] 0]
          ++ copied
          ++ (if secondPhase then [call 0x723cb8 [] task,signal phaseTwo,wait 0x10 0x10 phaseTwo] else [])
        effects = [finalState,word saveStatus status,word phaseOne 0,word phaseTwo 0,word (descriptor+4) mode,word (descriptor+0x148) 0,word (context+0x301c) 1]
          ++ [word (context+0x3020) 1 | secondPhase]
        extra = [word saveStatus 0x12345678,word (state+0x604) subframe,word (state+0x5f0) 1,word (descriptor+4) 0xffffffff,word (descriptor+0x148) 0xffffffff,word (context+0x3010) completion,word (context+0x3024) descriptor]
        finish = if status == 0 then completeCalls (plan kind) completionResult else []
     in make ("callback-"++show completion++"-cancel-"++(if interrupted then "yes" else "no")++"-sub-"++show subframe)
          kind extra effects returned (beginCalls (plan kind)++numbered 0 0++[call 0x73ac60 [] 0]++facade++trace kind++finish)
