module OutputsScenarios (outputsPublications, outputsScenarios) where

import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified BuffersScenarios as Buffers
import qualified ImagePlanesScenarios as ImagePlanes
import DiagnosticLoggingScenarios (traceDisabledInitial, textDisabledInitial)
import qualified DiagnosticLoggingScenarios as Diagnostics
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios

outputsPublications :: [StagePublication]
outputsPublications = [StagePublication (Address address) (StageSymbol ("xm1_develop_pipeline_" ++ name)) | (address,name) <-
  [(0xe9c560,"prepare_copy"),(0xe9c4ac,"correct_planes"),(0xe9c2d8,"read_components"),(0xe9c274,"apply_levels")
  ,(0xe9c0dc,"finalize_capture"),(0xe9bd88,"calibrate"),(0xe9bc38,"prepare_secondary"),(0xe9c040,"merge_auxiliary")
  ,(0xe9d03c,"configure_output_geometry"),(0xe9cde0,"divide_raw"),(0xe9cbd4,"add_raw"),(0xe9c9d8,"resize_raw_review")
  ,(0xe9d7d8,"resize_display"),(0xe9d4a4,"resize_review"),(0xe9c73c,"resize_thumbnail"),(0xe9ec7c,"select_crop")
  ,(0xe9ffcc,"match_points"),(0xea07ec,"match_triangles")]]

outputsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
outputsScenarios image resolve = invalidCases ++ disabledCases ++ plannedCases ++ reviewCases ++ rawCases ++ cropCases ++ cropSelectionCases ++ cancellationCases ++ transitionCases ++ callbackRoleCases ++ diagnosticCases
 where
  publication address = case [entry | entry <- outputsPublications, stageStockEntry entry == Address address] of
    [entry] -> entry
    _ -> error "output publication is not unique"
  make address name args initial final result calls =
    let entry = publication address
        Address admittedEntry = resolve entry
     in admittedEntry `seq` StageScenario (StageCaseName name) entry args (base ++ disabledDiagnostics ++ endpointInitial calls ++ initial) final result [] (concatMap externalEffects calls)
  disabledDiagnostics = traceDisabledInitial ++ textDisabledInitial
  endpointInitial calls =
    Buffers.objectLookupInitial (state+0x7c) 7 0 (nub [(register N.R1 (callInputs effect),register N.R2 (callInputs effect),register N.R0 (callOutputs effect),descriptorPlane) | effect <- calls,callAddress effect == Address 0xe44dbc]) ++
    Buffers.configurationInitial 0x009e0000 [(register N.R0 (callInputs effect),fromIntegral (register N.R0 (callOutputs effect))) | effect <- calls,callAddress effect == Address 0x69f8fc]
  descriptorPlane = Buffers.Plane 3 0x009c4000 0x2000 128 96 64 0x02000000
  register name values = case lookup name values of Just value -> value; Nothing -> error "output endpoint register law missing"
  externalEffects effect
    | callAddress effect `elem` map Address [0xeb0cbc,0xeb0ccc,0x726658,0x69f8fc] = []
    | callAddress effect == Address 0xe44dbc = Buffers.criticalCalls
    | callAddress effect == Address 0x723cb8 = ImagePlanes.currentTaskCalls (register N.R0 (callOutputs effect))
    | callAddress effect == Address 0x724638 = ImagePlanes.signalCurrentCalls 0x123 (register N.R0 (callOutputs effect))
    | otherwise = [effect]
  planFor operation = findPlan 0
   where
    findPlan index
      | index > 126 = error "output step plan missing from pinned source image"
      | readWord image address == operation && readWord image (address+28) == 13 = [readWord image (address+offset*4) | offset <- [0..12]]
      | otherwise = findPlan (index+1)
     where address = 0xea6738+index*52
  skipped address name args operation initial = make address name args initial [word (state+0x38) (planFor operation !! 4)] (Just 0) []
  lookupObject group kind target = call 0xe44dbc [state+0x7c,group,kind] target
  cfg key value = call 0x69f8fc [key] value
  debugDisabled kind = [cfg key 0]
   where
    row = 0xe540b0+kind*12
    key = readWord image row
  descriptor = [3,0x009c4000,0x2000,128,96,64,0x02000000]
  descriptorAddress = params+0x600
  inputDescriptor = wordsAt descriptorAddress descriptor
  configActive = [word (state+0x27cc) 1,word (state+0x640) 2,word (state+0x63c) 2,word (state+0x578) 1,word (state+0x1680) 1]
  planned address name args operation initial middle final =
    let p = planFor operation
     in make address name args initial (word (state+0x38) (p!!4):final) (Just 0) (beginCalls p ++ middle ++ completeCalls p 1)
  invalidCases =
    [make address ("output-invalid-channel-"++show address++"-"++show channel) [context,channel] [] [] (Just 0) []
      | address <- [0xe9c560,0xe9c0dc,0xe9bc38], channel <- [2,0xffffffff]] ++
    [make 0xe9bd88 ("output-invalid-domain-"++show domain++"-"++show channel) [context,domain,channel] [] [] (Just 0) [] | (domain,channel) <- [(2,0),(0,2),(0xffffffff,1)]]
  disabledCases =
    [skipped 0xe9c560 ("copy-disabled-"++show channel) [context,channel] channel [] | channel <- [0,1]] ++
    [skipped 0xe9c0dc ("finalize-constant-inactive-"++show channel) [context,channel] (5+channel) configActive | channel <- [0,1]] ++
    [skipped 0xe9bc38 ("secondary-disabled-"++show channel) [context,channel] (11+channel) [] | channel <- [0,1]] ++
    [skipped 0xe9bd88 "calibration-capture-mode-rejected" [context,0,0] 7 [word (state+0x4a8) 5]] ++
    [skipped 0xe9c4ac "plane-correction-disabled" [context] 2 []] ++
    [make 0xe9c2d8 "component-read-disabled-closes-operation" [context] [] [word (state+0x38) (planFor 3!!4)] (Just 0) [objectCall 3 2 1]] ++
    [skipped 0xe9c040 "merge-constant-inactive" [context] 13 configActive] ++
    [skipped 0xe9d03c ("output-precondition-rejected-"++show kind) [context,kind] operation [word (state+0x578) (if kind <= 1 then 1 else 0)] | (kind,operation) <- [(2,0x2a),(4,0x2b),(1,0x2c),(0,0x2d)]] ++
    [skipped address ("matching-mode-rejected-"++show address) [context] operation [] | (address,operation) <- [(0xe9ffcc,0x5c),(0xea07ec,0x5d)]] ++
    [skipped address ("matching-already-complete-"++show address++"-"++show offset) [context] operation [word (state+0x5cc) 4,word (state+0x3228) params,word (params+offset) 1] | (address,operation) <- [(0xe9ffcc,0x5c),(0xea07ec,0x5d)], offset <- [0x110,0x114,0x10c]]
  plannedCases =
    [planned 0xe9c274 "levels-bypass" [context] 4 configActive [] []] ++
    [planned 0xe9c274 "levels-native-no-adjustment" [context] 4 [] [call 0x723cb8 [] 0x123] []] ++
    [planned 0xe9c4ac "plane-correction-bypass" [context] 2 (configActive++[word (state+0x2738) 1]) [] []] ++
    [planned 0xe9c2d8 ("component-read-bypass-"++show mask) [context] 3 (configActive++[word (state+0x2740) (mask .&. 1),word (state+0x2744) (mask `shiftR` 1)]) [] [] | mask <- [1,2,3]] ++
    [planned 0xe9bc38 ("secondary-bypass-"++show channel) [context,channel] (11+channel) configActive
      [lookupObject 0x14 (10+channel) descriptorAddress,lookupObject 0x14 (10+channel) descriptorAddress,lookupObject 0x28 0 descriptorAddress] [] | channel <- [0,1]] ++
    [calibrationCase channel | channel <- [0,1]] ++
    [copyCase channel | channel <- [0,1]] ++
    [planned 0xe9d03c ("output-bypass-geometry-"++show kind) [context,kind] operation
      [word (state+0x4b4) 0x20,word (state+0x578) 2,word (state+0x27cc) 1,wordsAt (state+0x184) descriptor]
      [lookupObject 0x21 objectKind descriptorAddress] [word (state+0x1770) 48,word (state+0x176c) 64]
      | (kind,operation,objectKind) <- [(0,0x2d,15),(1,0x2c,14),(2,0x2a,16)]]
  calibrationCase channel =
    let p = planFor (7+channel)
     in make 0xe9bd88 ("calibration-bypass-publishes-descriptor-"++show channel) [context,0,channel]
          (configActive++[wordsAt (state+0x88+channel*28) descriptor])
          [word (state+0x38) (p!!4),wordsAt (state+0xc0+channel*28) descriptor] (Just 0)
          (beginCalls p ++ completeCalls p 1 ++ debugDisabled (16+channel) ++ if channel == 1 then [call 0xeb0cbc [0x6000000] 0] else [])
  copyCase channel =
    let p = planFor channel
        initial = configActive++[word (state+0x4b4) 0x1a,wordsAt (state+0x88+channel*28) descriptor]
     in make 0xe9c560 ("copy-bypass-publishes-capture-"++show channel) [context,channel] initial
          [word (state+0x38) (p!!4),wordsAt (state+0x168) descriptor] (Just 0)
          (debugDisabled channel ++ beginCalls p ++ [lookupObject 0x28 0 descriptorAddress] ++ completeCalls p 1 ++ debugDisabled (6+channel))
  reviewSpecs = [(0xe9c9d8,0x4c,0x98,0xea1ff8,0xea2010,0xea2018,0x41,0x2b8,0x6022200)
               ,(0xe9d7d8,0x4d,0x9a,0xea20a0,0xea20bc,0xea20c4,0x46,0x2f0,0x6022300)
               ,(0xe9d4a4,0x4e,0x9c,0xea2078,0xea2090,0xea2098,0x43,0x30c,0x6022400)
               ,(0xe9c73c,0x4f,0x9e,0xea1fc4,0xea1fe0,0xea1fec,0x45,0x328,0x6022500)]
  startLog text label = [call 0x723cb8 [] 0x123,call 0x726658 [0x23,text] 0,call 0x42188 [label] 0]
  transitionStart operation current = [objectCall operation 0 1,objectCall operation 1 1,callbackCall current 0]
  transitionEnd operation completed label = [objectCall operation 2 1,callbackCall completed 0,call 0x724638 [] 0,call 0xe4eca8 [state] 0,call 0x42188 [label] 0]
  reviewCases =
    [make address ("review-disabled-"++show address) [context] [] [word (state+0x38) (current+1)] (Just 0) [] | (address,_,current,_,_,_,_,_,_) <- drop 1 reviewSpecs] ++
    [make address ("review-bypass-"++show address) [context]
      [word (state+0x2770) 2,word (state+0x277c) 1,word (state+0x27cc) 1,inputDescriptor,word (state+0x184) 0x12345678]
      [word (state+0x38) (current+1),wordsAt (state+offset) (if address == 0xe9c73c then [3,0x009c4000,7200,120,120,60,0x02000000] else descriptor)]
      (Just 0)
      (startLog text start ++ transitionStart operation current ++ [lookupObject group 0 descriptorAddress] ++ [call 0xeb0cbc [trace] 0,call 0xeb0ccc [trace] 0,objectCall operation 3 1] ++ transitionEnd operation (current+1) end)
      | (address,operation,current,text,start,end,group,offset,trace) <- reviewSpecs]
  rawCases =
    [make 0xe9cbd4 "raw-add-missing-peer" [context] [] [word (state+0x38) 0x6f] (Just 0) (startLog 0xea2020 0xea2038)] ++
    [make 0xe9cde0 "raw-divide-peer-present" [context] [word (context+0x14) params] [word (state+0x38) 0x6d] (Just 0) (startLog 0xea2050 0xea2060)] ++
    [make address ("raw-bypass-"++show address) [context] ([word (state+0x27cc) 1]++if address == 0xe9cbd4 then [word (context+0x14) params] else [])
      [word (state+0x38) (current+1)] (Just 0)
      (startLog text start ++ transitionStart operation current ++ [objectCall operation 3 1] ++ transitionEnd operation (current+1) end)
      | (address,operation,current,text,start,end) <- [(0xe9cbd4,0x37,0x6e,0xea2020,0xea2038,0xea2044),(0xe9cde0,0x36,0x6c,0xea2050,0xea2060,0xea206c)]] ++
    [make 0xe9cde0 ("raw-divide-native-nonwork-buffer-"++show option) [context]
      [wordsAt (state+0x88) descriptor,word (context+0x18) 0x8001]
      [word (state+0x38) 0x6d,word (state+0xa0) (if option == 1 then 0x02000010 else 0x02000000)] (Just 0)
      (startLog 0xea2050 0xea2060 ++ transitionStart 0x36 0x6c ++ [cfg 0xec option,objectCall 0x36 3 1] ++ transitionEnd 0x36 0x6d 0xea206c) | option <- [0,1]]
  cropCases =
    [planned 0xe9ec7c "crop-selection-disabled" [context] 0x57 [inputDescriptor] [lookupObject 0x28 0 descriptorAddress,cfg 0x9f6f 0] []] ++
    [cropCase pitch yaw top | (pitch,yaw,top) <- [(0,0,13),(7,0,18),(0,0xfffffffb,10)]]
  cropCase pitch yaw top =
    let p = planFor 0x57
        initial = [inputDescriptor,word (state+0x3218) 1,word (state+0x3150) params,word (state+0x3228) (params+0x100),word (params+0x10) 100,word (params+0x20) 0,word (params+0x48) pitch,word (params+0x4c) yaw,word (state+0x5d0) 2]
        expected = [3,0x009c4000+128*top+16,32*16*2,128,32,32,0x02000000]
        config = [cfg 0x9f6f 0,cfg 0x9f64 0,cfg 0x9f15 0,cfg 0x9f14 8,cfg 0x9f1d 0,cfg 0x9f1c 16,cfg 0x9f15 0,cfg 0x9f14 8,cfg 0x9f1d 0,cfg 0x9f1c 16,cfg 0x9f1f 0,cfg 0x9f1e 32,cfg 0x9f17 0,cfg 0x9f16 3,cfg 0x9f6c 0,cfg 0x9f6b 2]
     in make 0xe9ec7c ("crop-motion-boundary-"++show pitch++"-"++show yaw) [context] initial
          [wordsAt (state+0x184) expected,word (state+0x3214) 0,word (state+0x321c) 100,word (state+0x38) (p!!4)] (Just 0)
          (beginCalls p ++ [lookupObject 0x28 0 descriptorAddress] ++ config ++ completeCalls p 1)
  cancellationCases =
    [make address ("output-entry-cancelled-"++show address) args (initial++[word (context+0x48) 0]) [] (Just 6) (prefix++take 3 (beginCalls (planFor operation)))
      | (address,args,operation,initial,prefix) <-
        [(0xe9c274,[context],4,[],[]),(0xe9c4ac,[context],2,[word (state+0x2738) 1],[])
        ,(0xe9c2d8,[context],3,[word (state+0x2740) 1],[])
        ,(0xe9bc38,[context,0],11,configActive,[]),(0xe9ec7c,[context],0x57,[],[])
        ,(0xe9ffcc,[context],0x5c,[word (state+0x5cc) 4,word (state+0x3228) params],[])
        ,(0xea07ec,[context],0x5d,[word (state+0x5cc) 4,word (state+0x3228) params],[])]] ++
    [make address ("review-entry-cancelled-"++show address) [context] [word (state+0x2770) 2,word (context+0x48) 0] [] (Just 6) (startLog text start)
      | (address,_,_,text,start,_,_,_,_) <- reviewSpecs]
  transitionCases =
    [let p = planFor operation
         prepare = objectCall operation 0 (p!!5)
         open = objectCall operation 1 (p!!5)
         abort (Call address inputs outputs patches) = Call address inputs outputs (patches++[word (context+0x48) 0])
         abort other = other
         external = take 3 (beginCalls p) ++ if phase == 0 then [abort prepare] else [prepare,abort open,objectCall operation 3 (p!!5)]
      in make 0xe9c274 ("levels-cancelled-after-object-phase-"++show phase) [context] [] [] (Just 6) external
      | phase <- [0,1], let operation = 4] ++
    [let p = planFor 4
         cancelledCallback = Call (Address callback) [(N.R0,context),(N.R1,p!!1),(N.R2,0),(N.R3,callback)] [(N.R0,0)] [word (context+0x48) 0]
         beginning = take 5 (beginCalls p) ++ [cancelledCallback,call 0xeb0cbc [p!!6] 0]
      in make 0xe9c274 "levels-completion-cancelled" [context] [word (state+0x27cc) 1]
           [word (state+0x38) (p!!4)] (Just 6) (beginning++completeCalls p 1)] ++
    [let newFrame = state+0x5000
         p = planFor 0
         completedCallback = Call (Address callback) [(N.R0,context),(N.R1,p!!2),(N.R2,0),(N.R3,callback)] [(N.R0,0)] [word context newFrame]
         completion = [call 0xeb0ccc [p!!6] 0,objectCall 0 3 (p!!5),objectCall 0 2 (p!!5),completedCallback,call 0x724638 [] 0,call 0xe4eca8 [newFrame] 0,call 0x42188 [p!!12] 0]
      in make 0xe9c560 "copy-completion-replaces-public-frame" [context,0]
           (configActive++[word (state+0x4b4) 0x1a,word (newFrame+0x4b4) 0x1a,wordsAt (state+0x88) descriptor])
           [word context newFrame,word (state+0x38) (p!!4),wordsAt (newFrame+0x168) descriptor] (Just 0)
           (debugDisabled 0++beginCalls p++[lookupObject 0x28 0 descriptorAddress]++completion++debugDisabled 6)]
  cropSelectionCases =
    [multiCrop 4
      [cfg 0x9f55 0,cfg 0x9f54 8,cfg 0x9f5d 0,cfg 0x9f5c 16,cfg 0x9f55 0,cfg 0x9f54 8,cfg 0x9f5d 0,cfg 0x9f5c 16]
      [cfg 0x9f5f 0,cfg 0x9f5e 32,cfg 0x9f57 0,cfg 0x9f56 3]
      [wordsAt (params+0x100) [3,0x009c4690,1024,128,32,32,0x02000000]]
    ,multiCrop 3
      [cfg 0x9f55 0,cfg 0x9f54 128,cfg 0x9f5d 0,cfg 0x9f5c 64,cfg 0x9f45 0,cfg 0x9f44 8,cfg 0x9f4d 0,cfg 0x9f4c 16,cfg 0x9f45 0,cfg 0x9f44 8,cfg 0x9f4d 0,cfg 0x9f4c 16]
      [cfg 0x9f4f 0,cfg 0x9f4e 32,cfg 0x9f47 0,cfg 0x9f46 3]
      [word (params+0x210) 1]
    ,let p = planFor 0x57
      in make 0xe9ec7c "crop-no-fitting-width-preserves-descriptor" [context]
          [inputDescriptor,word (state+0x3218) 1,word (state+0x3150) params,word (state+0x3228) (params+0x100),word (state+0x3214) 4,word (state+0x5d0) 2,wordsAt (state+0x184) descriptor,word (state+0x321c) 0x76543210]
          [wordsAt (state+0x184) descriptor,word (state+0x3214) 4,word (state+0x321c) 0x76543210,word (state+0x38) (p!!4)] (Just 0)
          (beginCalls p++[lookupObject 0x28 0 descriptorAddress,cfg 0x9f6f 0,cfg 0x9f64 0,cfg 0x9f15 0,cfg 0x9f14 8,cfg 0x9f1d 0,cfg 0x9f1c 16,cfg 0x9f5f 0,cfg 0x9f5e 32,cfg 0x9f57 0,cfg 0x9f56 3,cfg 0x9f6c 0,cfg 0x9f6b 2]++completeCalls p 1)
    ,let p = planFor 0x57
      in make 0xe9ec7c "crop-last-tagged-motion-record-wins" [context]
          [inputDescriptor,word (state+0x3218) 1,word (state+0x3150) params,word (state+0x3228) (params+0x200),word (state+0x5d0) 2,word (params+0x10) 100,word (params+0x70) 200,word (params+0x80) 8,word (params+0xa0) 16,word (params+0xa8) 7]
          [wordsAt (state+0x184) [3,0x009c4910,1024,128,32,32,0x02000000],word (state+0x3214) 0,word (state+0x321c) 208,word (state+0x38) (p!!4)] (Just 0)
          (beginCalls p++[lookupObject 0x28 0 descriptorAddress,cfg 0x9f6f 0,cfg 0x9f64 2,cfg 0x9f64 2,cfg 0x9f64 2,cfg 0x9f15 0,cfg 0x9f14 8,cfg 0x9f1d 0,cfg 0x9f1c 16,cfg 0x9f15 0,cfg 0x9f14 8,cfg 0x9f1d 0,cfg 0x9f1c 16,cfg 0x9f1f 0,cfg 0x9f1e 32,cfg 0x9f17 0,cfg 0x9f16 3,cfg 0x9f6c 0,cfg 0x9f6b 2]++completeCalls p 1)]
  multiCrop selected thresholds dimensions final =
    let p = planFor 0x57
     in make 0xe9ec7c ("crop-matching-class-"++show selected) [context]
          [inputDescriptor,word (state+0x3218) 1,word (state+0x3150) params,word (state+0x3228) (params+0x100),word (state+0x5cc) 4,word (state+0x5d0) 2,word (params+0x10) 100]
          ([wordsAt (state+0x184) [3,0x009c4690,1024,128,32,32,0x02000000],word (state+0x3214) selected,word (state+0x321c) 100,word (state+0x38) (p!!4)]++final) (Just 0)
          (beginCalls p++[lookupObject 0x28 0 descriptorAddress,cfg 0x9f6f 0,cfg 0x9f66 0]++thresholds++dimensions++[cfg 0x9f6c 0,cfg 0x9f6b 2]++completeCalls p 1)
  callbackRoleCases =
    [let target = publication 0xe9c0dc
         callbackEntry = addressWord (resolve target)
         initial = [word (state+0x2770) 2,word (state+0x27cc) 1,inputDescriptor,word (context+8) callbackEntry]
         final = [word (state+0x38) 0x9f,wordsAt (state+0x328) [3,0x009c4000,7200,120,120,60,0x02000000]]
         effects = startLog 0xea1fc4 0xea1fe0 ++ [objectCall 0x4f 0 1,objectCall 0x4f 1 1,lookupObject 0x45 0 descriptorAddress,call 0xeb0cbc [0x6022500] 0,call 0xeb0ccc [0x6022500] 0,objectCall 0x4f 3 1,objectCall 0x4f 2 1,call 0x724638 [] 0,call 0xe4eca8 [state] 0,call 0x42188 [0xea1fec] 0]
         scenario = make 0xe9c73c "thumbnail-native-stage-callback-role" [context] initial final (Just 0) effects
      in scenario { stageCodePointers = [(Address (context+8),target)] }]
  diagnosticCases =
    [let p = planFor 4
         buffer = params+0x800
         initial = word (state+0x27cc) 1 : Diagnostics.traceInitial buffer 0 100 0x12345678
         final = [word (state+0x38) (p!!4),word (buffer+32) 2] ++ drop 1 (Diagnostics.traceEffects buffer 0 100 0x12345678 (p!!6)) ++ drop 1 (Diagnostics.traceEffects buffer 1 100 0x12345678 ((p!!6) .|. 0x10000000))
      in make 0xe9c274 "levels-owned-active-trace-ring" [context] initial final (Just 0) (beginCalls p++completeCalls p 1)]
    ++ [let p = planFor 4
            buffer = params+0x1800
            calendar = Diagnostics.CalendarFields 20 1 2 3 4 5
            text = B.takeWhile (/=0) (B.drop (fromIntegral ((p!!10)-0xbf1a00)) image)
            initial = word (state+0x27cc) 1 : Diagnostics.textInitial buffer 8 calendar
            final = word (state+0x38) (p!!4) : Diagnostics.textEffects buffer 8 0x23 text calendar
            effects = beginCalls p++completeCalls p 1
            activeText effect | callAddress effect == Address 0x726658 = Diagnostics.textCalls
                              | otherwise = externalEffects effect
            scenario = make 0xe9c274 "levels-owned-active-text-calendar" [context] initial final (Just 0) effects
         in scenario {stageCalls=concatMap activeText effects}]
