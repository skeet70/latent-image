{-# LANGUAGE GHC2021 #-}
module MotionSearchScenarios (motionSearchPublications, motionSearchScenarios) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios

motionSearchPublications :: [StagePublication]
motionSearchPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name)) | (address,name) <-
  [(0xdac8f8,"find_motion"),(0xdad060,"refine_motion"),(0xdac07c,"motion_cost_hardware"),(0xdac1a8,"motion_cost"),(0xdac32c,"motion_search_local"),(0xdac040,"motion_complete")
  ,(0xda9e44,"resample_complete"),(0xda9ec4,"resample_release_channel"),(0xda9ed8,"resample_cancel"),(0xda9f30,"resample_start"),(0xdaa0a8,"resample_build"),(0xdaa200,"resample_set_parameters"),(0xdaa2a8,"motion_configure_candidate"),(0xdaa304,"resample_get_status"),(0xdaa330,"resample_defaults"),(0xdaa43c,"resample_initialize")
  ,(0x69a0d4,"resample_acquire"),(0x69a150,"resample_release"),(0x69a19c,"resample_reset_hardware"),(0x69a210,"resample_mode"),(0x69a27c,"resample_finish_hardware"),(0x69a2cc,"resample_registers"),(0x69a8e0,"resample_plane"),(0x69ab04,"resample_dimensions"),(0x69ab80,"resample_pair"),(0x69ae54,"resample_read_status"),(0x69aee8,"resample_stream_flags"),(0x69afbc,"resample_status_word"),(0x69afec,"resample_acknowledge"),(0x69b030,"resample_initialize_control"),(0x69b0c8,"resample_command_one"),(0x69b134,"resample_command_zero")]]

publication :: Word32 -> StagePublication
publication address = case filter ((== Address address) . stageStockEntry) motionSearchPublications of
  [entry] -> entry
  _ -> error "missing motion publication"

motionSearchScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
motionSearchScenarios _ resolve = costCases ++ localCases ++ findCases ++ refineCases ++ hardwareCases ++ registerCases
 where
  source = 0x009c0000
  target = 0x009c2000
  sourceRegion = 0x009c4000
  targetRegion = 0x009c4020
  stateAddress = 0x009c4100
  threshold = 0x009c4200
  point = 0x009c4220
  result = 0x009c4240
  vectors = 0x009c4260
  region = [wordsAt sourceRegion [source,128,64,4],wordsAt targetRegion [target,128,64,4]]
  ramp patchAddress offset = Patch (Address patchAddress) (B.pack [fromIntegral ((column `div` 4)*2 + offset) | _row <- [0..31 :: Int], column <- [0..127 :: Int]])
  constant patchAddress value = Patch (Address patchAddress) (B.replicate 4096 value)
  motionState at width height sx sy maximumCost =
    [wordsAt at [64,4,0,0,width,height,17,1,sourceRegion,targetRegion,0,0,0,0,0,maximumCost,sx,sy,0]]
  scenario name address arguments initial final returned calls = StageScenario (StageCaseName name) (publication address) arguments initial final returned [] calls
  costCases =
    [scenario ("motion-cost-" ++ name) 0xdac1a8 [threshold,0,0,stateAddress]
      (region ++ motionState stateAddress width height sx sy 100 ++ [constant source a,constant target b,word threshold limit])
      [word (stateAddress+56) 1,word 0x014e2a80 0] (Just expected) []
    | (name,width,height,sx,sy,a,b,limit,expected) <-
      [("match",16,12,2,2,17,17,0x7fffffff,0)
      ,("mismatch",16,12,2,2,20,10,0x7fffffff,960)
      ,("threshold",16,12,2,2,20,10,25,40)
      ,("empty-width",0,12,2,2,20,10,100,0)
      ,("negative-height",16,0xffffffff,2,2,20,10,100,0)]]
  localCases =
    [scenario "motion-local-nontrivial-match" 0xdac32c [point,threshold,stateAddress]
      (region ++ motionState stateAddress 4 2 2 2 100 ++ [ramp source 0,ramp target 8,wordsAt point [0,0],word threshold 32,wordsAt 0x014e2a80 (replicate 100 0)])
      [wordsAt point [8,0],word threshold 0,word (stateAddress+56) 3,wordsAt 0x014e2a80 [4,8,12]] Nothing []
    ,scenario "motion-local-accelerated-descent" 0xdac32c [point,threshold,stateAddress]
      (region ++ motionState stateAddress 4 2 2 2 100 ++ [word (stateAddress+24) 61,ramp source 0,ramp target 40,wordsAt point [0,0],word threshold 160,wordsAt 0x014e2a80 (replicate 100 0)])
      [wordsAt point [40,0],word threshold 0,word (stateAddress+56) 8,wordsAt 0x014e2a80 [4,8,12,36,60,40,44,32]] Nothing []
    ,scenario "motion-local-tie-keeps-center" 0xdac32c [point,threshold,stateAddress]
      (region ++ motionState stateAddress 4 2 2 2 100 ++ [constant source 37,constant target 37,wordsAt point [0,0],word threshold 0,wordsAt 0x014e2a80 (replicate 100 0)])
      [wordsAt point [0,0],word threshold 0,word (stateAddress+56) 1,word 0x014e2a80 4] Nothing []
    ,scenario "motion-local-boundary-no-evaluation" 0xdac32c [point,threshold,stateAddress]
      (region ++ motionState stateAddress 4 2 2 2 100 ++ [word (stateAddress+24) 1,wordsAt point [0,0],word threshold 9])
      [wordsAt point [0,0],word threshold 9,word (stateAddress+56) 0] Nothing []]
  findCalls = [call 0x69f8fc [key] value | (key,value) <- [(0x9f8b,0),(0x9f07,8),(0x9f09,0),(0x9f08,4),(0x9f0a,1)]]
  findCases =
    [scenario "motion-find-shifted-ramp" 0xdac8f8 [sourceRegion,targetRegion,0,result,0]
      (region ++ motionState 0x014e2a34 4 2 2 2 100 ++ [ramp source 0,ramp target 8,wordsAt 0x014e2a28 [0,0]])
      [wordsAt result [32,0,5],wordsAt 0x014e2a28 [32,0],word 0x014e29d8 0] (Just 0) findCalls
    ,scenario "motion-find-exhausted-budget" 0xdac8f8 [sourceRegion,targetRegion,0,result,0]
      (region ++ motionState 0x014e2a34 4 2 2 2 0 ++ [wordsAt 0x014e2a28 [0,0]])
      [wordsAt result [0,0,0],wordsAt 0x014e2a28 [0,0]] (Just 536870880) findCalls
    ,scenario "motion-find-alternate-variant" 0xdac8f8 [sourceRegion,targetRegion,0,result,7]
      (region ++ motionState 0x014e2a34 4 2 2 2 1 ++ [constant source 31,constant target 31,wordsAt 0x014e2a28 [0,0]])
      [wordsAt result [0,0,1],word 0x014e29d8 7] (Just 0)
      [call 0x69f8fc [key] value | (key,value) <- [(0x9fee,0),(0x9fe0,0),(0x9fe2,0),(0x9fe1,1),(0x9fe3,1)]]]
  refineCalls = [call 0x69f8fc [key] value | (key,value) <- [(0x9f8b,0),(0x9f0d,0),(0x9f0e,1),(0x9f0d,0),(0x9f0b,0),(0x9f0c,1),(0x9f0b,0)]]
  refineCases =
    [scenario "motion-refine-preserves-skipped-vector-word" 0xdad060 [sourceRegion,targetRegion,result,vectors]
      (region ++ motionState 0x014e29dc 16 12 8 6 1 ++ [constant source 20,constant target 10,wordsAt 0x014e2a28 [0,0],word 0x014e29d8 0,wordsAt vectors [91,92,0xa5b6c7d8,94]])
      [wordsAt result [0,0],wordsAt vectors [0,0,0xa5b6c7d8,1]] (Just 1280) refineCalls
    ,scenario "motion-refine-shifted-ramp" 0xdad060 [sourceRegion,targetRegion,result,vectors]
      (region ++ motionState 0x014e29dc 16 12 8 6 100 ++ [ramp source 0,ramp target 8,wordsAt 0x014e2a28 [0,0],word 0x014e29d8 0,wordsAt vectors [91,92,0x13579bdf,94]])
      [wordsAt result [8,0],wordsAt vectors [8,0,0x13579bdf,5],wordsAt 0x014e2a28 [8,0]] (Just 0)
      [call 0x69f8fc [key] value | (key,value) <- [(0x9f8b,0),(0x9f0d,0),(0x9f0e,1),(0x9f0d,0),(0x9f0b,8),(0x9f0c,4),(0x9f0b,8),(0x9f0c,4),(0x9f0b,8),(0x9f0c,4),(0x9f0b,8)]]]
  hardwareCases = [hardware "error-retains-threshold" 0xffffffff 100 100,hardware "cost-stops-at-threshold" 7 10 14]
  hardware name status limit expected =
    let count = if status == 0xffffffff then 1 else 2
        completion = publication 0xda9e44
        callbackAddress = addressWord (resolve completion)
        invoke = [call 0x723cb8 [] 9,call 0x64e918 [0x66,callbackAddress] 0,call 0x42634 [0,0] 0
          ,Call (Address 0x7243a8) [(N.R0,0x30),(N.R2,0xffffffff)] [(N.R0,0)] []
          ,call 0x7235d8 [] 0,call 0x7236b0 [0] 0,call 0x64eb20 [0] 0]
     in (scenario ("motion-hardware-" ++ name) 0xdac07c [threshold,4,0,stateAddress]
          (motionState stateAddress 4 2 1 1 100 ++ [word (stateAddress+72) 1,word threshold limit,word 0xfff200a0 status,word 0xfff20004 0,wordsAt 0x014ce72c (replicate 561 0)])
          [word 0x014e2a30 0,word 0x014ce730 0] (Just expected)
          (concat (replicate count invoke) ++ [call 0x69f8fc [0x9f6f] 0]))
          {stageCodePointers = [(Address 0xda9e44,completion),(Address 0xdac040,publication 0xdac040)]}
  registerCases =
    [scenario "motion-resample-plane-field-widths" 0x69a8e0 [sourceRegion]
      [wordsAt sourceRegion (replicate 11 0xffffffff)]
      [wordsAt 0xfff20008 [0x3fffffff,0x3fffffff,0x3fff3fff,0x1fff1fff,0x0fff0fff,0x00000f03],word 0xfff20560 0x3fffffff] Nothing []
    ,scenario "motion-resample-register-field-widths" 0x69a2cc [sourceRegion]
      [wordsAt sourceRegion (replicate 81 0xffffffff)]
      [word 0xfff20004 0x3780,wordsAt 0xfff20020 (replicate 10 0x1fff1fff),wordsAt 0xfff20048 [0xffff000f,0xffffffff,255,0x01ff0f03,0x3fffffff]
      ,wordsAt 0xfff2005c (replicate 10 0x00ffffff),word 0xfff20084 0x7f7f,word 0xfff2008c 0x7f7f7f7f,word 0xfff20090 0x007f7f7f] Nothing []
    ,scenario "motion-resample-build-cpu-packing" 0xdaa0a8 [sourceRegion,0x009c4800]
      [wordsAt sourceRegion [0..90],wordsAt 0x009c4800 [200..208]]
      [word 0xfff20004 0x3280,wordsAt 0xfff20008 [200,201,0x00cb00ca,0x00cd00cc,0x00cf00ce,0x00000d00]
      ,wordsAt 0xfff20020 [0x000f000e,0x00110010,0x00130012,0x00150014,0x00170016,0x00190018,0x001b001a,0x001d001c,0x001f001e,0x00210020]
      ,wordsAt 0xfff20048 [0x2524000e,0x29282726,42,0x002e0d01,0x0310c02f]
      ,wordsAt 0xfff2005c [0x343332,0x373635,0x3a3938,0x3d3c3b,0x403f3e,0x434241,0x464544,0x494847,0x4c4b4a,0x4f4e4d]
      ,word 0xfff20084 0x5150,word 0xfff2008c 0x55545352,word 0xfff20090 0x00585756,word 0xfff20560 208] (Just 0) []
    ,scenario "motion-resample-invalid-index-still-reads-status" 0x69ae54 [result,255,255]
      [word 0xfff22c74 0x12345678]
      [word result 0x12345678] Nothing []
    ,scenario "motion-candidate-reorders-pairs" 0xdaa2a8 [11,22,33,44,3] []
      [word 0xfff20014 0x000b0016,word 0xfff2002c 0x0021002c] (Just 0) []]
