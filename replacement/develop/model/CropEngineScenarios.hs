{-# LANGUAGE GHC2021 #-}
module CropEngineScenarios where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address(..), Patch(..), Call(..), word, wordsAt, call, params)
import BuffersScenarios (criticalInitial, criticalEnterCalls, criticalLeaveCalls)
import ImagePlanesScenarios (reservedInitial, reservedWaitCalls, reservedSignalCalls, priorityInitial, notificationCalls)
import qualified WarpEngineScenarios as W

newtype EngineAddress = EngineAddress Word32 deriving (Eq,Show)
data EnginePlane = EnginePlane EngineAddress Word32 Word32 Word32 deriving (Eq,Show)
data EnginePacket = EnginePacket
  { sourceMode :: Word32, targetMode :: Word32, transformMode :: Word32
  , sourcePlane :: EnginePlane, targetPlane :: EnginePlane, requestedStride :: Word32
  , filterOption :: Word32, filterLimit :: Word32, filterEnabled :: Word32
  , filterWeights :: (Word32,Word32,Word32), filterControls :: (Word32,Word32,Word32,Word32)
  , completedAddress :: EngineAddress
  } deriving (Eq,Show)

parsePacket :: [Word32] -> EnginePacket
parsePacket [source,target,transform,input,inputStride,inputWidth,inputHeight,output,outputStride,stride,outputWidth,outputHeight,option,limit,enabled,red,green,blue,c0,c1,c2,c3,completed] =
  EnginePacket source target transform (EnginePlane (EngineAddress input) inputStride inputWidth inputHeight) (EnginePlane (EngineAddress output) outputStride outputWidth outputHeight) stride option limit enabled (red,green,blue) (c0,c1,c2,c3) (EngineAddress completed)
parsePacket _ = error "crop engine packet requires twenty-three ABI words"

cropEnginePublications :: [StagePublication]
cropEnginePublications = [publication address name | (address,name) <- [(0xda63c0,"crop_interrupt"),(0xda6400,"crop_initialize"),(0xda642c,"crop_release"),(0xda6440,"crop_begin"),(0xda645c,"crop_wait"),(0xda64ac,"crop_configure"),(0xda67ec,"crop_publish"),(0x683830,"crop_device_acquire"),(0x6838dc,"crop_device_release"),(0x6839c4,"crop_device_begin"),(0x683a28,"crop_program")]]
publication :: Word32 -> String -> StagePublication
publication address name=StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++name))

cropHardwareWords :: [Word32] -> [Word32]
cropHardwareWords packet
 | any (\value -> value .&. 15/=0) [inputAddress,inputStride,inputWidth,outputAddress,outputStride,outputWidth,stride] = error "crop configuration enters alignment fail-stop"
 | inputHeight .&. 1/=0 || stride>outputStride || stride<outputWidth || normalizedInput/=ratio*normalizedOutput || inputHeight/=ratio*outputHeight = error "crop configuration enters dimension fail-stop"
 | otherwise = [sourceMode p,targetMode p,transformMode p,inputAddress,inputStride,normalizedInput,inputHeight,outputAddress,outputStride,normalizedOutput,outputHeight,step*ratio,step,0,0,2,if transformMode p==0 then 0 else 1,if transformMode p==0 then 0 else 60,grouping,filterOption p,filterLimit p,filterEnabled p,red,green,blue,c0,c1,c2,c3]
 where
  p=parsePacket packet
  EnginePlane (EngineAddress inputAddress) inputStride inputWidth inputHeight=sourcePlane p
  EnginePlane (EngineAddress outputAddress) outputStride outputWidth outputHeight=targetPlane p
  stride=requestedStride p
  normalizedInput=if sourceMode p==0 then inputWidth `div` 2 else inputWidth
  normalizedOutput=if targetMode p==0 then outputWidth `div` 2 else outputWidth
  ratio=if transformMode p==0 then 2 else 1
  candidates=if targetMode p==0 then allTiles else drop 2 allTiles
  allTiles=[(32,2),(24,2),(16,2),(8,2),(12,1),(4,1),(10,0),(6,0),(2,0)]
  (tile,grouping)=case filter (\(size,_)->outputWidth `div` 16<=size*((stride `div` 16) `div` size)) candidates of candidate:_ -> candidate; [] -> error "crop tile selection enters fail-stop"
  step=tile*(if targetMode p==0 then 8 else 16)
  (red,green,blue)=filterWeights p
  (c0,c1,c2,c3)=filterControls p

cropRegisterWords :: [Word32] -> [(Word32,Word32)]
cropRegisterWords h
 | length h/=29 = error "crop hardware configuration requires twenty-nine words"
 | any (\i->h!!i .&. 15/=0) [3,4,7,8] || any (\i->h!!i>0x7fff) [4,8] || any (\i->h!!i>0x1fff) [5,6,9,10] || any (\i->h!!i .&. 7/=0) [5,9] = error "crop register image dimensions enter fail-stop"
 | h!!20>0x3ff || any (>127) [h!!22,h!!23,h!!24] || (h!!21==1 && h!!22+2*(h!!23+h!!24)/=128) || any (>255) [h!!25,h!!26,h!!27,h!!28] || h!!25+2*(h!!26+h!!27+h!!28)/=256 = error "crop register filter enters fail-stop"
 | any (\i->h!!i>512 || h!!i .&. 7/=0) [11,12] || h!!14>255 || h!!17>255 = error "crop register stepping enters fail-stop"
 | otherwise = [(0xfff43008,(h!!0 .&. 3) .|. ((h!!1 .&. 1) `shiftL` 4) .|. ((h!!19 .&. 1) `shiftL` 8) .|. ((h!!21 .&. 1) `shiftL` 12) .|. ((h!!2 .&. 1) `shiftL` 16)),(0xfff43010,h!!3),(0xfff43014,h!!4),(0xfff43018,h!!5 .|. (h!!6 `shiftL` 16)),(0xfff43020,h!!7),(0xfff43024,h!!8),(0xfff43028,h!!9 .|. (h!!10 `shiftL` 16)),(0xfff43030,h!!20),(0xfff43034,h!!22 .|. (h!!23 `shiftL` 8) .|. (h!!24 `shiftL` 16)),(0xfff43038,h!!25 .|. (h!!26 `shiftL` 8) .|. (h!!27 `shiftL` 16) .|. (h!!28 `shiftL` 24)),(0xfff4303c,h!!11 .|. (h!!12 `shiftL` 16)),(0xfff43080,h!!14 .|. ((h!!13 .&. 1) `shiftL` 8) .|. ((h!!15 .&. 3) `shiftL` 12) .|. (h!!17 `shiftL` 16) .|. ((h!!16 .&. 1) `shiftL` 24) .|. ((h!!18 .&. 3) `shiftL` 28))]

data CropEngineLaw = CropEngineLaw
  { engineInitial :: [Patch], engineFinal :: [Patch]
  , engineInitializeCalls :: [Call], enginePublishCalls :: [Call], engineBeginCalls :: [Call]
  , engineWaitCalls :: [Call], engineReleaseCalls :: [Call]
  , engineCodePointers :: [(Address,StagePublication)], engineFinalSerial :: Word32
  }

nextSerial :: Word32 -> Word32
nextSerial value=if value+1==0 then 1 else value+1

resetWaitCall :: Call
resetWaitCall=Call (Address 0x683958) [] [] [word 0xfff43000 1]

cropEngineLaw :: (StagePublication -> Address) -> [Word32] -> CropEngineLaw
cropEngineLaw=cropEngineLawWithSerial 0

cropEngineLawWithSerial :: Word32 -> (StagePublication -> Address) -> [Word32] -> CropEngineLaw
cropEngineLawWithSerial serial resolve packet=CropEngineLaw initial final initialize publish begin wait release [(Address 0x14ce4f8,publication 0xe63f04 "crop_complete") | completed/=0] irqHandle
 where
  interruptPublication=publication 0xda63c0 "crop_interrupt"
  callback=addressWord (resolve interruptPublication)
  EngineAddress completed=completedAddress (parsePacket packet)
  routeHandle=nextSerial serial
  irqHandle=nextSerial routeHandle
  initial=if serial/=0 then [] else W.routeInitial++W.interruptInitial++reservedInitial 0x2b++[word 0xfff88030 0x100,word 0xfff88010 0xabcdefff,wordsAt 0x14ce498 (replicate 54 0xa5a5a5a5)]
  final=[wordsAt 0x14ce498 ([irqHandle,0]++packet++cropHardwareWords packet),wordsAt 0x89f748 [0,1],word 0x409c2804 irqHandle,word (0x409c1e30+0x758+41*4) 0,word (0x409c1e30+0x69c+41*4) 0,word (0x409c1e84+4*0xa0+21*4) 0,word (0x409c1e84+4*0xa0+0x9c) 0,word 0xfff88030 0x100,word 0xfff88010 0xabcdefff,word 0xfff43000 1,word 0xfff43004 1,word 0xfff43088 0xffffffff,word 0xfff4308c 0xffffffff]++map (uncurry word) (cropRegisterWords (cropHardwareWords packet))++W.interruptReleaseEffects 69
  initialize=reservedWaitCalls 0x2b 0xffffffff 0++W.routeRequestCalls True++W.routeAcquireCalls++[resetWaitCall]
  publish=W.interruptAcquireCalls 69 13 1 callback
  begin=[call 0x42634 [0,0] 0]
  wait=[resetWaitCall]++W.interruptReleaseCalls 69
  release=W.routeReleaseCalls False++W.routeRequestCalls True++reservedSignalCalls 0x2b 0 0

cropEngineScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
cropEngineScenarios _image resolve=configurationCases++beginCases++waitCases++interruptCases
 where
  make name entry symbol arguments initial final result pointers calls=StageScenario (StageCaseName name) (publication entry symbol) arguments initial final result pointers calls
  packet source target transform=[source,target,transform,0x9c2000,256,128,16,0x9c4000,256,128,if transform==0 then 64 else 128,if transform==0 then 8 else 16,1,42,1,64,16,16,64,32,32,32,0]
  configurationCases=[make ("crop-engine-configure-"++show (source,target,transform)) 0xda64ac "crop_configure" [params] [wordsAt params p,wordsAt (0x14ce498+100) (replicate 29 0xa5a5a5a5)] (wordsAt (0x14ce498+100) h:map (uncurry word) (cropRegisterWords h)) (Just 0) [] [] | (source,target,transform)<-[(0,0,0),(1,1,0),(0,0,1)],let p=packet source target transform,let h=cropHardwareWords p]
  beginCases=[make "crop-engine-enable-registers" 0x6839c4 "crop_device_begin" [] [word 0xfff43004 0,word 0xfff4308c 0] [word 0xfff43004 1,word 0xfff4308c 1] Nothing [] []]
  waitCases=[make ("crop-engine-wait-"++show (active,valid)) 0xda645c "crop_wait" [] (W.interruptInitial++[wordsAt 0x14ce498 [7,active],word (0x940e9c+69*4) (if valid then 7 else 0),word (0x940e9c+(69+128)*4) 60]) ([word 0xfff43000 1,word (0x14ce498+4) (if active/=0 && not valid then 1 else 0)]++[patch | active/=0 && valid,patch<-W.interruptReleaseEffects 69]) (Just (if active/=0 && not valid then 0xffffffec else 0)) [] ([resetWaitCall]++[c | active/=0 && valid,c<-W.interruptReleaseCalls 69]) | (active,valid)<-[(0,False),(1,True),(1,False)]]
  callbackPublication=publication 0xe63f04 "crop_complete"
  callback=addressWord (resolve callbackPublication)
  interruptCases=[make ("crop-engine-interrupt-"++show active) 0xda63c0 "crop_interrupt" [] (criticalInitial++priorityInitial 0x123++[word (0x14ce498+96) (if active then callback else 0),word 0x157bcfc 0x123,word 0xfff43084 0x24]) [word (0x14ce498+96) 0,word 0xfff43088 0x24] Nothing [(Address 0x14ce4f8,callbackPublication) | active] (criticalEnterCalls++criticalLeaveCalls++[c | active,c<-notificationCalls 0x123 0x10 0 0]) | active<-[False,True]]
