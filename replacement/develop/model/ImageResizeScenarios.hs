{-# LANGUAGE GHC2021 #-}
module ImageResizeScenarios
  ( imageResizePublications
  , imageResizeScenarios
  , resizeMode
  , warpRegisterInitial
  , warpRegisterValues
  ) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import PipelineModel
import Scenarios

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

imageResizePublications :: [StagePublication]
imageResizePublications =
  [publication 0xe64a44 "resize_planes"
  ,publication 0xe57ab0 "select_resize_mode"
  ,publication 0xe63f38 "resize_complete"
  ,publication 0xe64988 "snapshot_warp"
  ,publication 0xe55dc8 "expand_distortion"
  ,publication 0xe57b5c "distortion_band"
  ,publication 0xe57ba4 "distortion_enabled"
  ,publication 0x6df9bc "warp_read_registers"]

resizeMode :: [Word32] -> [Word32] -> Word32 -> Word32 -> Word32 -> Word32 -> Word32
resizeMode thresholds modes inputWidth inputHeight outputWidth outputHeight
  | horizontal < 20 || vertical < 20 = 0
  | (horizontal <= 39 || vertical <= 39) && chosen == 2 = 1
  | otherwise = chosen
 where
  horizontal = (inputWidth*10 `div` outputWidth) `mod` 65536
  vertical = (inputHeight*10 `div` outputHeight) `mod` 65536
  ratio = min horizontal vertical
  index = length (takeWhile (<= ratio) (take 5 thresholds))
  chosen = modes !! index

warpRegisterValues :: [Word32]
warpRegisterValues = [0x100,0x200] ++ [0x300..0x312] ++ [0x400..0x41e] ++ [0x500,0x600,0x700]

warpRegisterInitial :: [Patch]
warpRegisterInitial =
  [wordsAt 0xfff65000 [0x100,0x200]
  ,wordsAt 0xfff65010 [0x300..0x312]
  ,wordsAt 0xfff65060 [0x400..0x41e]
  ,wordsAt 0xfff65140 [0x500,0x600,0x700]]

imageResizeScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imageResizeScenarios _ _ = registerCases ++ snapshotCases ++ resizeModeCases ++ preparedCases
 where
  make name address suffix arguments initial final returned = StageScenario
    (StageCaseName name) (publication address suffix) arguments initial final returned [] []
  registerCases =
    [make "image-warp-mmio-snapshot" 0x6df9bc "warp_read_registers" [params]
      (warpRegisterInitial++[wordsAt params (replicate 55 0xa5a5a5a5)])
      [wordsAt params warpRegisterValues] Nothing]
  snapshotCases =
    [make ("image-warp-debug-counter-"++show count) 0xe64988 "snapshot_warp" [state]
      (warpRegisterInitial++[word 0x96ca50 (params+0x1000),Patch (Address (params+0x10a8)) (B.singleton 0x80),word (state+0x220c) buffer,word (buffer+0xd7e4c) count]++[wordsAt (buffer+0xd75b4+count*0xdc) (replicate 55 0xa5a5a5a5) | count < 10])
      ([word (buffer+0xc315c) 0x53545243,word (buffer+0xd7e50) 0x4c415243]
      ++ if count < 10 then [word (buffer+0xd7e4c) (count+1),wordsAt (buffer+0xd75b4+count*0xdc) warpRegisterValues] else [word (buffer+0xd7e4c) count]) Nothing
    | count <- [0,9,10],let buffer = state+0x10000]
  resizeModeCases =
    [make ("image-resize-mode-"++show ratio++"-choice-"++show chosen)
      0xe57ab0 "select_resize_mode" [params]
      [word 0x96ca50 (state+0x10000),Patch (Address (state+0x1c0a5)) (B.pack [20,30,40,50,60]),Patch (Address (state+0x1c0aa)) (B.replicate 6 (fromIntegral chosen)),wordsAt params (take 20 ([0,0x55,0,0,0,0,ratio,ratio,0,0,0,0,0,0,10,10]++repeat 0))]
      [word (params+4) (resizeMode [20,30,40,50,60] (replicate 6 chosen) ratio ratio 10 10)] Nothing
    | ratio <- [0,19,20,39,40,60,65536],chosen <- [0,1,2,3]]
  preparedCases =
    [StageScenario (StageCaseName name) (publication 0xe64a44 "resize_planes")
      [state,0,0,params,params+0x40,0,params+0x100,0]
      [word 0x96ca50 (state+0x10000)
      ,Patch (Address (state+0x1c0a5)) (B.pack [10,20,30,40,50])
      ,Patch (Address (state+0x1c0aa)) (B.replicate 6 2)
      ,Patch (Address (state+0x100a8)) (B.singleton 0)
      ,word (state+0x78) 1,word (state+0x42c) (inputWidth `div` 2),word (state+0x430) requestedWidth
      ,wordsAt params [0,0x40001000,0,inputStride,inputWidth,inputHeight,0]
      ,wordsAt (params+0x40) [0,0x41000000,0,128,outputWidth,outputHeight,0]
      ,wordsAt (params+0x100) (replicate 20 0xa5a5a5a5)]
      [wordsAt (params+0x100) expected] (Just 1) []
      [call 0x42188 [0xe66670] 0,call 0x42188 [0xe6668c] 0]
    | (name,inputWidth,inputHeight,inputStride,requestedWidth,outputWidth,outputHeight,expected) <-
      [("image-resize-prepared-twofold"
       ,400,160,512,192,100,40
       ,[1,1,7941,8297,0x40001000,256,200,160,198,158,0x41000000,64,0,0,50,40,49,39,0,0])
      ,("image-resize-prepared-expanded-width"
       ,192,60,256,112,128,24
       ,[1,0,7216,10507,0x40000ff0,128,112,60,111,59,0x41000000,64,0,0,64,24,63,23,0,0])]]
