{-# LANGUAGE GHC2021 #-}
module RedEyeBackendScenarios
  ( redEyeBackendPublications, redEyeBackendScenarios
  , redEyeTaskRuntimeInitial, redEyeTaskDetectPacket, redEyeTaskApplyPacket
  , redEyeTaskSource, redEyeTaskFaceSnapshot, redEyeTaskPayloadPointer
  ) where

import qualified Data.ByteString as B
import Data.Word (Word16, Word32)
import BuffersScenarios (criticalInitial, criticalCalls, configurationInitial)
import DebugStorageScenarios (receiveInitial)
import DiagnosticLoggingScenarios (traceDisabledInitial, textDisabledInitial)
import PipelineModel
import Scenarios

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_redeye_" ++ suffix))

redEyeBackendPublications :: [StagePublication]
redEyeBackendPublications = [publication address suffix | (address,suffix)<-
  [(0x1071ca4,"run_task"),(0x1071c3c,"report_error"),(0x1071c70,"abort_requested")
  ,(0x10741f4,"prepare_history"),(0x10750b8,"configure_pair_filters")
  ,(0x1085de8,"prepare_edits"),(0x10860a0,"apply_pixels"),(0x1086668,"scale_edits")
  ,(0x10873e0,"init_buffers"),(0x1087460,"prepare_primary_buffer"),(0x108748c,"prepare_secondary_buffer")
  ,(0x10874b8,"prepare_stage_buffers"),(0x1087540,"prepare_pyramid_buffers")
  ,(0x89aa90,"divide"),(0x723ffc,"spin_delay")
  ,(0x1072978,"reset_records"),(0x1072994,"record_at"),(0x10729c0,"append_record")
  ,(0x1072a08,"compact_records"),(0x1072a84,"copy_records")
  ,(0x10742a4,"refine_pairs"),(0x10746e8,"filter_pair_geometry"),(0x1074954,"recover_pair")
  ,(0x1074d58,"dark_density"),(0x1074f6c,"filter_pair_samples")
  ,(0x10734a8,"append_faces"),(0x107374c,"match_faces"),(0x1073afc,"complete_pairs")
  ,(0x1073608,"deduplicate_pairs"),(0x1073e10,"remove_overlapping_pairs"),(0x10741c0,"associate_candidates")]]

redEyeTaskRuntimeInitial :: [Patch]
redEyeTaskRuntimeInitial = receiveInitial 0x38 ++ criticalInitial ++ traceDisabledInitial ++ textDisabledInitial ++
  [ word 0xfff91000 0, word 0xfff91004 1
  , word (0x9b6000+0x174) (params+0x3000), word (params+0x3000) 100
  , Patch (Address 0x14dfa90) (B.replicate 0x26a4 0)
  ]

redEyeTaskDetectPacket :: Address -> Address -> B.ByteString
redEyeTaskDetectPacket (Address arena) (Address callbackAddress) = wordBytes arena <> wordBytes callbackAddress

redEyeTaskApplyPacket :: Address -> B.ByteString
redEyeTaskApplyPacket (Address callbackAddress) = wordBytes callbackAddress

redEyeTaskSource :: Address -> Word32 -> Word32 -> Word32 -> Patch
redEyeTaskSource (Address pixels) before height after = wordsAt 0x14dfa90 [pixels,before,height,after]

redEyeTaskFaceSnapshot :: B.ByteString -> Patch
redEyeTaskFaceSnapshot bytes = Patch (Address 0x14dfaa0) bytes

redEyeTaskPayloadPointer :: Patch
redEyeTaskPayloadPointer = word 0x14e2120 0x14e2124

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.take 2 (wordBytes (fromIntegral value))

redEyeBackendScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
redEyeBackendScenarios _image _resolve = divideCases ++ bufferCases ++ pixelCases ++ scaleCases ++ historyCases ++ densityCases ++ controlCases ++ listCases
 where
  make name address suffix arguments initial final returned calls = StageScenario
    (StageCaseName ("redeye-backend-"++name)) (publication address suffix) arguments initial final returned [] calls
  divideCases =
    [make ("divide-"++show numerator++"-"++show denominator) 0x89aa90 "divide" [numerator,denominator] [] [] (Just expected) []
    | (numerator,denominator,expected)<-[(19,3,6),(0xffffffed,3,0xfffffffa),(19,0xfffffffd,0xfffffffa),(0xffffffed,0xfffffffd,6),(0x80000000,0xffffffff,0x80000000),(0x80000000,2,0xc0000000),(7,16,0),(0,13,0)]]
  bufferCases =
    [make "edit-buffer-addresses-and-capacity" 0x10873e0 "init_buffers" [params+0x2000,params]
      [Patch (Address params) (B.replicate 0xa40 0xa5)]
      ([word (params+0x460) 0,wordsAt (params+0x464) [params+0x16960,72,20],wordsAt (params+0x474) [params+0x16000,24,100]] ++
       [wordsAt (params+index*56+32) [params+0x2000+index*4096,64,64,1] | index<-[0..19]]) (Just 0x14f00) []]
    ++ [make suffix address suffix [params+0x2000,params] [] [wordsAt (params+offset) [params+0x2000,width,height]] (Just size) []
       | (address,suffix,offset,width,height,size)<-[(0x1087460,"prepare_primary_buffer",0x55c,24,1000,0x5dc0),(0x108748c,"prepare_secondary_buffer",0x594,72,500,0x8ca0)]]
    ++ [make "stage-buffer-regions" 0x10874b8 "prepare_stage_buffers" [params+0x2000,params] []
        [wordsAt (params+0x5a4) [params+0x2000,20,20,params+0x2320,40,40,params+0x2fa0,64,64,params+0x4fa0,64,64,1],wordsAt (params+0x9d8) [params+0x5fa0,24,200]] (Just 0x5260) []
       ,make "pyramid-buffer-regions" 0x1087540 "prepare_pyramid_buffers" [params+0x2000,params] []
        (wordsAt (params+0x9e8) [params+0x2000,64,64]:[wordsAt (params+0x9f4+index*16) [params+0x4000+index*4096,64,64,1] | index<-[0..3]]) (Just 0x6000) []]
  pixelCases =
    [ let pixels=params+0x1000
          mask=params+0x1800
          edit=params+0x400
          original=B.pack (concat (replicate 32 [100,100,128,255]))
          corrected=if gradient then B.pack [100,100,128,223] else B.pack [0,0,128,128]
          pairColumn=if center==48 then 1 else 2
          expected=B.concat [if row `elem` [2,3,4] && column==pairColumn then corrected else B.pack [100,100,128,255] | row<-[0..7::Int],column<-[0..3::Int]]
          maskBytes=B.pack (if gradient then concat (replicate 4 [0,64,128,192]) else replicate 16 255)
       in make ("actual-pixel-mask-"++show center++"-"++show gradient) 0x10860a0 "apply_pixels" [edit,pixels,8,8,10240]
          [wordsAt edit [0,0,0,center,4,48,4,1024,mask,4,4,1],word (edit+0x460) 1,Patch (Address pixels) original,Patch (Address mask) maskBytes]
          [Patch (Address pixels) expected,word (edit+0x460) 1,Patch (Address mask) maskBytes] Nothing []
    | center<-[48,64],gradient<-[False,True]]
    ++ [make "pixel-disabled-count" 0x10860a0 "apply_pixels" [params,0,0,0,0] [word (params+0x460) 0] [word (params+0x460) 0] Nothing []]
  scaleCases =
    [ let list=params
          record=params+0x100
          edits=params+0x400
          sides=case selected of 0->[0];1->[1];2->[0,1];_->[]
       in make ("scale-selected-"++show selected) 0x1086668 "scale_edits" [list,edits,1024]
          [wordsAt list [record,72,1,1],word (record+20) selected,wordsAt (record+44) [64,128,80,144,0x00800080,0x56781234,3],Patch (Address edits) (B.replicate 0x464 0xa5)]
          (word (edits+0x460) (fromIntegral (length sides)):
           concat [wordsAt (edits+index*56+12) [x,146,y,146]:[Patch (Address (edits+index*56+48)) (halfBytes tag<>halfBytes 3<>halfBytes side)] | (index,side,x,y,tag)<-if sides==[0,1] then [(0,0,56,72,0x1234),(1,1,120,136,0x5678)] else [(0,side,if side==0 then 56 else 120,if side==0 then 72 else 136,if side==0 then 0x1234 else 0x5678) | side<-sides]]) Nothing []
    | selected<-[0,1,2,3]]
  candidatePair=[1,160,0,0x03e80080,0,0x0000ffff,0,320,0,0,32,0,0,0,0,0,0,4]
  completedPair=[1,160,0,0x03e80080,0,2,0,320,0,0,32,32,288,0,0,0x01400140,0x02bc0258,4]
  historyCases =
    [make "associate-two-observations-one-complete-face" 0x10741c0 "associate_candidates" [params,params+0x40]
      [wordsAt params [params+0x100,72,4,1],wordsAt (params+0x40) [params+0x400,24,2,2],wordsAt (params+0x100) candidatePair,wordsAt (params+0x400) [1,32,0,0x02580140,0,0,1,288,0,0x02bc0140,0,0]]
      [word (params+12) 1,wordsAt (params+0x100) completedPair] Nothing []
    ,make "history-empty-observations-clears-destination" 0x10741f4 "prepare_history" [0,params,params+0x40,params+0x80,0,0,0,0,0,0,0]
      [wordsAt params [params+0x100,72,4,3],wordsAt (params+0x40) [params+0x400,24,2,0],wordsAt (params+0xa8) [params+0x800,72,4,2]]
      [word (params+12) 0,word (params+0xb4) 0] Nothing []]
  densityCases =
    [let pixels=B.pack (replicate 5 200++replicate 15 0++[200]++replicate 4 0)
         bins=[if index==0 then 19 else if index==200 then 6 else 0 | index<-[0..255::Int]]
     in make ("dark-density-selection-"++show selection) 0x1074d58 "dark_density" [params,params+0x100,selection]
       [wordsAt params [params+0x800,5,5],Patch (Address (params+0x800)) pixels,Patch (Address 0x14e2134) (B.pack [50,2,0,0,100])]
       [wordsAt (params+0x100) bins] (Just (if selection==1 then 1 else 0)) []
    | selection<-[0,1]]
  controlCases =
    [make ("abort-state-"++show flag) 0x1071c70 "abort_requested" []
      (criticalInitial++[Patch (Address 0x14e212c) (B.singleton flag)]) [] (Just (if flag==1 then 1 else 0)) criticalCalls
    | flag<-[0,1,2,255]]
    ++ [make "error-counter-wrap" 0x1071c3c "report_error" [] (textDisabledInitial++[word 0x14e2130 0xffffffff]) [word 0x14e2130 0] Nothing []]
    ++ [make "timer-quantized-short-delay" 0x723ffc "spin_delay" [1] [word 0xfff91000 0,word 0xfff91004 1] [] Nothing []]
    ++ [make "empty-pair-filter-config-consumption" 0x10750b8 "configure_pair_filters" [0,params,0,0]
      (word (params+12) 0:configurationInitial (params+0x1000) [(0xa454,3),(0xa455,100),(0xa457,2),(0xa458,50),(0xa459,100),(0xa453,0)])
      [word (params+12) 0,Patch (Address 0x14e2134) (B.pack [50,2,3,100,100,0])] Nothing []]
  listCases =
    [make "compact-records-preserves-visible-order" 0x1072a08 "compact_records" [params]
      [wordsAt params [params+0x100,8,4,4],wordsAt (params+0x100) [1,11,0,22,1,33,0,44]]
      [word (params+12) 2,wordsAt (params+0x100) [1,11,1,33]] Nothing []
    ,make "append-capacity-rejects-without-touching-record" 0x10729c0 "append_record" [params,0]
      [wordsAt params [params+0x100,8,2,2],wordsAt (params+0x100) [1,11,1,22]]
      [wordsAt params [params+0x100,8,2,2],wordsAt (params+0x100) [1,11,1,22]] Nothing []]
