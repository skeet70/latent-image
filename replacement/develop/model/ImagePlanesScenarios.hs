{-# LANGUAGE GHC2021 #-}
module ImagePlanesScenarios
  ( imagePlanesPublications
  , imagePlanesScenarios
  , priorityInitial
  , priorityCalls
  , priorityCallsWith
  , currentTaskCalls
  , waitCalls
  , notificationCalls
  , reservedInitial
  , reservedWaitCalls
  , reservedSignalCalls
  , signalCurrentCalls
  , fillEffects
  , debugDisabledInitial
  ) where

import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios

imagePlanesPublications :: [StagePublication]
imagePlanesPublications =
  [ publication 0xe8ef98 "fill_pattern"
  , publication 0xe4fa90 "copy_rows"
  , publication 0xe547e0 "store_triplets"
  , publication 0xe54c6c "output_disabled"
  , publication 0xe55164 "stage_ready"
  , publication 0xe5517c "stage_complete"
  , publication 0xe55194 "stage_inactive"
  , publication 0xe4fae8 "resize_capture"
  , publication 0xe4fd5c "resize_through_crop"
  , publication 0xe50010 "resize_software"
  , publication 0xe500f0 "rotate_software"
  , publication 0xe4f898 "prepare_auxiliary_plane"
  , publication 0xe4f950 "prepare_special_plane"
  , publication 0xe523d4 "configure_output_policy"
  , publication 0xe5254c "prepare_output_mask"
  , publication 0xe5277c "prepare_output_request"
  , publication 0xe53ed4 "publish_debug_file"
  , publication 0x723a14 "configure_task"
  , publication 0x723cb8 "current_task"
  , publication 0x7243a8 "wait_all"
  , publication 0x724558 "send_notification"
  , publication 0x724b68 "wait_reserved"
  , publication 0x724c00 "signal_reserved"
  , publication 0x724638 "signal_current_zero"
  ]

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

priorityInitial :: Word32 -> [Patch]
priorityInitial task =
  [ word 0x96bd20 0x9b6000
  , word (0x9b6000+0x1618) 0x8f0100
  , word (0x9b6000+0x16c) 0x8f0104
  , word (0x9b6000+0x1c) 0x8f0108
  , word (0x9b6000+0x9c) 0x8f0110
  , word (0x9b6000+0x148) 0x8f0114
  , word (0x9b6000+0x88) 0x8f0118
  , word (0x9b6000+0x8c) 0x8f011c
  , word (0x9b6000+0x7c) 0x8f0120
  , word (0x9b6000+0x6c) 0x8f0124
  , word (0x9b6000+0x70) 0x8f0128
  , word (0x9b6000+0x90) 0x8f012c
  , word (0x7cad9c+((effectiveTask task .&. 0xffff)-1)*4) 3
  , word 0x4089ea8c 0x8f010c
  ]

effectiveTask :: Word32 -> Word32
effectiveTask 0 = 0x45
effectiveTask task = task

currentTaskCalls :: Word32 -> [Call]
currentTaskCalls task = [call 0x8f0100 [] 0,call 0x8f0104 [] task]

priorityCalls :: Word32 -> Word32 -> [Call]
priorityCalls task priority = priorityCallsWith task priority 0

priorityCallsWith :: Word32 -> Word32 -> Word32 -> [Call]
priorityCallsWith task priority result =
  [call 0x8f0100 [] 0] ++ [call 0x8f0104 [] selected | task == 0]
  ++ [call 0x8f0108 [(selected .&. 0xffff) .|. 0x30000,priority] result]
  ++ [call 0x41a34 [5,selected,result] 0 | result /= 0]
  ++ [ScriptCall (Address 0x8f010c) [(N.R0,result)] [(N.R0,0)] [Absolute (word 0x40a0f82c result)] [] | result /= 0]
 where selected = effectiveTask task

dispatchErrorCalls :: Word32 -> Word32 -> Word32 -> [Call]
dispatchErrorCalls event task result =
  [call 0x41a34 [event,task,result] 0
  ,ScriptCall (Address 0x8f010c) [(N.R0,result)] [(N.R0,0)] [Absolute (word 0x40a0f82c result)] []]

waitCalls :: Word32 -> Word32 -> Word32 -> Word32 -> [Call]
waitCalls task mask events result =
  [call 0x8f0100 [] 0,call 0x8f0104 [] task,call 0x8f0104 [] task
  ,ScriptCall (Address 0x8f0110) [(N.R0,(task .&. 0xffff) .|. 0x30000),(N.R1,mask),(N.R2,1)] [(N.R0,result)] [] [Relative N.R3 0 (wordBytes events)]]
  ++ (if result /= 0 && result /= 0xffffffce then call 0x8f0104 [] task:dispatchErrorCalls 0x11 task result else [])

notificationCalls :: Word32 -> Word32 -> Word32 -> Word32 -> [Call]
notificationCalls task value mode result =
  [call 0x8f0100 [] 0,call 0x8f0114 [] mode
  ,call (if mode == 1 then 0x8f011c else 0x8f0118) [(task .&. 0xffff) .|. 0x30000,value] result]
  ++ (if result /= 0 then dispatchErrorCalls 0x13 task result else [])

reservedInitial :: Word32 -> [Patch]
reservedInitial handle = priorityInitial 0x45 ++
  [word (0x93e6f4+((handle .&. 0xffff)-1)*4) 2 | handle < 170 || handle >= 0x80000000]

reservedEncoded :: Word32 -> Word32
reservedEncoded handle = (handle .&. 0xffff) .|. if handle < 170 || handle >= 0x80000000 then 0x20000 else 0x10000

reservedWaitCalls :: Word32 -> Word32 -> Word32 -> [Call]
reservedWaitCalls handle timeout result =
  [call 0x8f0100 [] 0,call 0x8f0120 [reservedEncoded handle,timeout] result]
  ++ (if result /= 0 && result /= 0xffffffce then dispatchErrorCalls 0x18 handle result else [])

reservedSignalCalls :: Word32 -> Word32 -> Word32 -> [Call]
reservedSignalCalls handle mode result =
  [call 0x8f0100 [] 0,call 0x8f0114 [] mode
  ,call (if mode == 1 then 0x8f0128 else 0x8f0124) [reservedEncoded handle] result]
  ++ (if result /= 0 then dispatchErrorCalls 0x19 handle result else [])

signalCurrentCalls :: Word32 -> Word32 -> [Call]
signalCurrentCalls task result =
  [call 0x8f0100 [] 0,call 0x8f0104 [] task,call 0x8f0104 [] task
  ,call 0x8f012c [(task .&. 0xffff) .|. 0x30000,0] result]
  ++ (if result /= 0 then call 0x8f0104 [] task:dispatchErrorCalls 0x14 task result else [])

fillEffects :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
fillEffects address width stride lineCount value =
  [wordsAt (address+row*(stride-(width .&. 3))) (replicate (fromIntegral (width `div` 4)) value) | row <- take (fromIntegral lineCount) [0..]]

debugDisabledInitial :: B.ByteString -> Word32 -> [Word32] -> [Patch]
debugDisabledInitial image configuration kinds =
  word 0x96ca50 configuration :
  [Patch (Address (configuration+readWord image (0xe540b0+kind*12))) (B.singleton 0) | kind <- kinds]

imagePlanesScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imagePlanesScenarios image _ = constantCases ++ tripletCases ++ fillCases ++ rowCases ++ priorityCases ++ waitCases ++ notificationCases ++ reservedCases ++ debugCases
 where
  make name address suffix arguments initial final result calls = StageScenario
    (StageCaseName name) (publication address suffix) arguments initial final result [] calls
  constantCases =
    [make ("image-plane-constant-"++suffix) address suffix [0,0] [] [] (Just returned) []
    | (address,suffix,returned) <- [(0xe54c6c,"output_disabled",0),(0xe55164,"stage_ready",1),(0xe5517c,"stage_complete",1),(0xe55194,"stage_inactive",0)]]
  tripletCases =
    [make ("image-triplets-bank-"++show bank) 0xe547e0 "store_triplets" [state,params,bank]
      [Patch (Address params) values]
      [Patch (Address (state+offset)) values | offset <- if bank == 0 then [0x2224,0x2230] else [0x71c]] (Just 1) []
    | bank <- [0,1,0xffffffff], let values = B.pack [1,2,0xff,0x7f,0,0x80]]
  fillCases =
    [make ("image-fill-"++show pattern++"-"++show width++"-"++show lineCount++"-cancel-"++show cancelled)
      0xe8ef98 "fill_pattern" [params+0x100,params,pattern,0x76543210,0x45,6]
      (priorityInitial 0x45 ++ [wordsAt params [0,params+0x400,0,stride,width,lineCount,0],Patch (Address (params+0x100)) (B.singleton cancelled),Patch (Address (params+0x400)) (B.replicate 256 0xa5)])
      (if cancelled == 1 || lineCount == 0 then [Patch (Address (params+0x400)) (B.replicate 256 0xa5)] else fillEffects (params+0x400) width stride lineCount value)
      (Just (if cancelled == 1 && lineCount /= 0 then 1 else 0))
      (priorityCalls 0x45 12 ++ priorityCalls 0x45 6)
    | (pattern,value) <- [(0,0x80800000),(1,0x8080fefe),(2,0x80808080),(3,0xff554c4c),(4,0x162c9595),(5,0x6bff1d1d),(6,0x76543210),(99,0x80800000)]
    , width <- [0,4,31,32,36], lineCount <- [0,1,3], cancelled <- [0,1,2], let stride = width+8]
  rowCases =
    [make ("image-copy-rows-"++show rows) 0xe4fa90 "copy_rows" [0,params,params+0x40,0]
      [wordsAt params [0,params+0x100,0,12,7,rows,0],wordsAt (params+0x40) [0,params+0x200,0,16,3,1,0],Patch (Address (params+0x100)) (B.pack [0..63]),Patch (Address (params+0x200)) (B.replicate 64 0xff)]
      [Patch (Address (params+0x200+row*16)) (B.pack [fromIntegral (row*12)..fromIntegral (row*12+6)]) | row <- take (fromIntegral rows) [0..]]
      (Just 1) []
    | rows <- [0,1,3]]
  priorityCases =
    [make ("image-priority-task-"++show task++"-result-"++show result)
      0x723a14 "configure_task" [task,12] (priorityInitial task)
      [word 0x40a0f82c result | result /= 0] (Just result) (priorityCallsWith task 12 result)
    | task <- [0,0x45,0x30045],result <- [0,0xfffffff9]]
    ++ [make "image-current-task" 0x723cb8 "current_task" [] (priorityInitial 0x45) [] (Just 0x45) (currentTaskCalls 0x45)]
  waitCases =
    [make ("image-wait-result-"++show result++"-events-"++show events)
      0x7243a8 "wait_all" [0x30,params,0xffffffff]
      (priorityInitial 0x45++[word params 0xa5a5a5a5])
      (word params events:[word 0x40a0f82c result | result /= 0 && result /= 0xffffffce])
      (Just result) (waitCalls 0x45 0x30 events result)
    | result <- [0,0xffffffce,0xfffffff9],events <- [0x10,0x20,0x30]]
  notificationCases =
    [make ("image-notification-mode-"++show mode++"-result-"++show result)
      0x724558 "send_notification" [0x45,0x10] (priorityInitial 0x45)
      [word 0x40a0f82c result | result /= 0] (Just result) (notificationCalls 0x45 0x10 mode result)
    | mode <- [0,1,2],result <- [0,0xfffffff9]]
  reservedCases =
    [make ("image-reserved-wait-"++show handle++"-result-"++show result)
      0x724b68 "wait_reserved" [handle,0xffffffff] (reservedInitial handle)
      [word 0x40a0f82c result | result /= 0 && result /= 0xffffffce] (Just result) (reservedWaitCalls handle 0xffffffff result)
    | handle <- [1,0x66,169,170,0x100ab],result <- [0,0xffffffce,0xfffffff9]]
    ++ [make ("image-reserved-signal-"++show handle++"-mode-"++show mode++"-result-"++show result)
      0x724c00 "signal_reserved" [handle] (reservedInitial handle)
      [word 0x40a0f82c result | result /= 0] (Just result) (reservedSignalCalls handle mode result)
    | handle <- [1,0x66,169,170],mode <- [0,1,2],result <- [0,0xfffffff9]]
    ++ [make ("image-signal-current-result-"++show result)
      0x724638 "signal_current_zero" [] (priorityInitial 0x45)
      [word 0x40a0f82c result | result /= 0] (Just result) (signalCurrentCalls 0x45 result)
    | result <- [0,0xfffffff9]]
  debugCases =
    [make ("image-debug-disabled-kind-"++show kind) 0xe53ed4 "publish_debug_file"
      [state,kind,params,0] (debugDisabledInitial image (state+0x10000) [kind]) [] Nothing []
    | kind <- [0..31]]
