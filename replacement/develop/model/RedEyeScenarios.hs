{-# LANGUAGE GHC2021 #-}
module RedEyeScenarios
  ( redEyePublications
  , redEyeScenarios
  , redEyeResultInitial
  , redEyeSecondaryEffects
  , redEyePrimaryEffects
  ) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import ImagePlanesScenarios (priorityInitial, notificationCalls)
import PipelineModel
import Scenarios

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

redEyePublications :: [StagePublication]
redEyePublications =
  [ publication address suffix
  | (address,suffix) <-
    [(0xe50204,"prepare_geometry"),(0xe5092c,"start_work"),(0xe50c74,"finish_copy")
    ,(0x734b00,"normalize_faces"),(0xe4e008,"unpack_rectangle"),(0xe4e04c,"rectangle_to_bounds")
    ,(0xe4e67c,"complete_primary_copy"),(0xe4e6e0,"complete_secondary_copy")
    ,(0x1072794,"submit_redeye_request"),(0x1072844,"submit_redeye_value")
    ,(0x7257a8,"get_ticks"),(0x7257e0,"elapsed")]
  ]

redEyeResultInitial :: Word32 -> [Patch]
redEyeResultInitial task = priorityInitial task ++ [word 0x155ccac task, word 0x155ccb0 task]

redEyeSecondaryEffects :: B.ByteString -> [Word32] -> [Patch]
redEyeSecondaryEffects results values = [Patch (Address 0x154cf44) results, wordsAt 0x154c748 values]

redEyePrimaryEffects :: B.ByteString -> [Word32] -> [Patch]
redEyePrimaryEffects results values = [Patch (Address 0x154c780) results, wordsAt 0x154cc58 values]

redEyeScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
redEyeScenarios _image _owned = rectangleCases ++ normalizationCases ++ clockCases ++ callbackCases
 where
  make name address suffix arguments initial final result calls = StageScenario
    (StageCaseName name) (publication address suffix) arguments initial final result [] calls
  rectangleCases =
    [make "redeye-unpack-plane-widths" 0xe4e008 "unpack_rectangle" [params,params+0x100]
      [wordsAt params [7,0x40108000,0x4000,512,384,192,0x10000]]
      [wordsAt (params+0x100) [0x40108000,192,192,64]] Nothing []
    ,make "redeye-result-inclusive-bounds-tag-width" 0xe4e04c "rectangle_to_bounds" [params,params+0x100]
      [wordsAt params [11,17,9,5,0xabcdfedc],word (params+0x11c) 0x76543210]
      [wordsAt (params+0x100) [11,19,17,21,0,1,1,0x7654fedc]] Nothing []]
  normalizationCases =
    [make ("redeye-normalize-active-"++show count) 0x734b00 "normalize_faces" [state,params]
      ([word (state+0x78) 0,word (state+0x4f0) 0,word (state+0x644) 0,word (state+0x27ac) 0,word (state+0x600) 0,word (state+0x70c) 0
       ,word (state+0x458) 100,word (state+0x460) 100,word (state+0x464) 80,word (state+0x6c2) 0
       ,word 0x816f60 200,word 0x8163a8 160,word 0x96ca50 (params+0x2000),Patch (Address (params+0x2254)) (B.singleton 0),word params count]
       ++ [wordsAt (params+index*0xb8+12) values | (index,values)<-take (fromIntegral count) [(0,[3,8,11,18]),(1,[20,29,31,40]),(2,[41,50,51,60])]])
      [wordsAt (params+index*0xb8+12) values | (index,values)<-take (fromIntegral count) [(0,[6,16,22,36]),(1,[40,58,62,80]),(2,[82,100,102,120])]] Nothing []
    | count<-[0,1,3]]
  clockCases =
    [make "redeye-clock-read" 0x7257a8 "get_ticks" [params]
      (priorityInitial 0x45++[word (0x9b6000+0x174) (params+0x100),word (params+0x100) 1234])
      [word params 1234] Nothing [call 0x8f0100 [] 0]
    ,make "redeye-clock-wrap-elapsed" 0x7257e0 "elapsed" [0xfffffffe]
      (priorityInitial 0x45++[word (0x9b6000+0x174) (params+0x100),word (params+0x100) 3])
      [] (Just 50) [call 0x8f0100 [] 0]]
  callbackCases = concat
    [ let values = [0x10203040,0x50607080,0x90a0b0c0]
          primary = B.pack (take 0x464 (cycle [1,7,31,127,255]))
          secondary = B.pack (take 0x1bc (cycle [2,8,32,128,254]))
          event = if status == 1 then 0x10 else 0x20
          initial = redEyeResultInitial task ++ [wordsAt (params+0x800) values]
          calls = if task == 0 then [] else notificationCalls task event 0 0
      in [make ("redeye-primary-copy-"++show status++"-"++show task) 0xe4e67c "complete_primary_copy"
            [status,params+0x800,params,0] (initial ++ [Patch (Address params) primary])
            (redEyePrimaryEffects primary (take 2 values)) Nothing calls
         ,make ("redeye-secondary-copy-"++show status++"-"++show task) 0xe4e6e0 "complete_secondary_copy"
            [status,params,params+0x800] (initial ++ [Patch (Address params) secondary])
            (redEyeSecondaryEffects secondary values) Nothing calls]
    | status <- [0,1,2,0xffffffff], task <- [0,0x45]
    ]
