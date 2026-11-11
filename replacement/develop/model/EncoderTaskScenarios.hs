{-# LANGUAGE GHC2021 #-}
module EncoderTaskScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int32,Int64)
import Data.Word
import PipelineModel
import Scenarios (Address(..),Patch(..),Call(..),MemoryContract(..),word,wordsAt,wordBytes,params)
import qualified Emulation.NativeObservation as N
import BuffersScenarios (criticalInitial,criticalCalls)
import ImagePlanesScenarios (currentTaskCalls,notificationCalls,reservedInitial,reservedWaitCalls)
import DebugStorageScenarios (QueueMode(..),queueInitial,queueCalls,receiveInitial,receiveCalls)
import WarpEngineScenarios (diagnosticEventInitial,diagnosticEventCalls,diagnosticEventEffects,interruptInitial,interruptReleaseCalls,interruptReleaseEffects)

encoderTaskPublications :: [StagePublication]
encoderTaskPublications=[StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++symbol)) | (address,symbol)<-
 [(0x665760,"dma_wait")
 ,(0x6e2390,"registry_initialize"),(0x6e24e0,"registry_allocate"),(0x6e2588,"registry_free"),(0x6e266c,"registry_get_key"),(0x6e2690,"registry_get_request"),(0x6e26b8,"registry_set_request"),(0x6e2718,"registry_lookup"),(0x6e274c,"request_register"),(0x6e27ac,"request_initialize"),(0x6e2808,"request_unregister"),(0x6e2b80,"request_send"),(0x6e2bf0,"request_send_reply"),(0x6e2834,"request_decode"),(0x6e28ac,"request_dispatch"),(0x6e2aa0,"registry_process"),(0x6e26dc,"registry_receive"),(0x6e2ae8,"registry_worker")
 ,(0x11dab4c,"metadata_initialize_request"),(0x11d28e8,"setup_descriptors"),(0x11d3ea8,"initialize_stream"),(0x11dbfc0,"calculate_metadata_limit"),(0x11dc264,"select_compression_tables"),(0x11d4938,"start_request"),(0x11d4a20,"start_compact_request"),(0x11d4a04,"start_secondary_request"),(0x11daa7c,"metadata_request_halt"),(0x11daa88,"metadata_request_completed"),(0x11d25a0,"stream_signal_completion"),(0x11daab8,"start_compression"),(0x11d42b4,"set_stream_active"),(0x11d268c,"dispatch_job"),(0x11d42d4,"pad_primary"),(0x11d4728,"primary_completed"),(0x11d4620,"header_completed"),(0x11d450c,"secondary_completed"),(0x11daf44,"begin_compression"),(0x11dad8c,"retry_compression"),(0x11dae6c,"complete_compression"),(0x11daeac,"compression_completed")
 ,(0xd98a80,"power_scale"),(0xd98e14,"compute_scale"),(0xd98ea4,"compute_mode_scale"),(0x11dac34,"metadata_request_value"),(0xd98a18,"scale_quantization"),(0x11dac68,"prepare_compression_tables"),(0x11dace4,"submit_compression"),(0xd98f14,"preview_enabled"),(0xd98ee4,"classify_aspect"),(0xd98f58,"fill_pixel_pairs"),(0xd98f8c,"scale_preview_pairs"),(0xd99104,"scale_preview"),(0xd99138,"make_preview"),(0x11d2640,"configure_stream_output")
 ,(0x65ac34,"channel8_invalid"),(0x65acec,"channel8_reset"),(0x65ac40,"channel8_request_resource"),(0x65ac98,"channel8_release_resource"),(0xd986f0,"channel8_acquire"),(0xd98744,"channel8_release"),(0x65adbc,"channel8_start"),(0xd987f4,"channel8_finish"),(0xd9879c,"channel8_submit"),(0xd994c8,"channel8_completed"),(0xd99368,"channel8_configure"),(0xd9878c,"metadata_start_channel8"),(0x65ae00,"channel8_publish")
 ,(0x665304,"dma_open"),(0x665424,"dma_close"),(0x665ac4,"dma_program"),(0x665540,"dma_begin"),(0x666458,"dma_aligned_transfer"),(0x666690,"dma_aligned_completed"),(0x11d2620,"stream_dma_completed"),(0x11d25d0,"stream_copy_blocks"),(0x11d2d5c,"measure_template"),(0x11d3204,"write_chunked_segments")]]
publication :: Word32 -> StagePublication
publication address=case filter ((==Address address).stageStockEntry) encoderTaskPublications of
 p:_->p
 []->error "unknown encoder task publication"

halfBytes :: Word16 -> B.ByteString
halfBytes value=B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
half :: Word32 -> Word16 -> Patch
half address value=Patch (Address address) (halfBytes value)
byte :: Word32 -> Word8 -> Patch
byte address value=Patch (Address address) (B.singleton value)

newtype Worker = Worker Word32 deriving (Eq,Show)
newtype RequestSlot = RequestSlot Word16 deriving (Eq,Show)
newtype RequestEvent = RequestEvent Word16 deriving (Eq,Show)
data RequestPacket = RequestPacket RequestEvent RequestSlot Word8 Word32 Word32 Int32 deriving (Eq,Show)
requestPacketBytes :: RequestPacket -> B.ByteString
requestPacketBytes (RequestPacket (RequestEvent code) (RequestSlot slot) queue first second third)=halfBytes code<>B.pack [fromIntegral slot,queue]<>B.concat (map wordBytes [first,second,fromIntegral third])
registryAddress :: Worker -> Word32
registryAddress (Worker worker)=0x96e04c+worker*12
requestRegistryInitial :: Worker -> Word32 -> Word32 -> [Patch]
requestRegistryInitial worker capacity entries=[wordsAt (registryAddress worker) [0,capacity,entries],wordsAt entries (concat (replicate (fromIntegral capacity) [0xffffffff,0])),half 0x8faccc 0]
requestQueueInitial :: Worker -> Word8 -> [Patch]
requestQueueInitial (Worker worker) queue=queueInitial (fromIntegral queue)++[byte (0x7d088c+worker) queue]
requestSendCalls :: Word8 -> Word32 -> RequestPacket -> [Call]
requestSendCalls queue sender packet@(RequestPacket (RequestEvent code) _ _ _ _ _)=currentTaskCalls sender++queueCalls (fromIntegral queue) code (fromIntegral sender) queue (requestPacketBytes packet) QueueNormal 0

metadataRequestEffects :: Word32 -> Word32 -> Word32 -> Word32 -> Int32 -> Word32 -> Word32 -> Int32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Maybe B.ByteString -> Word32 -> Word32 -> [Patch]
metadataRequestEffects request mode width height kind input target attempts option choice source destination limit stride controls completed halt =
 [word (request+offset) value | (offset,value)<-[(0,mode .&. 0xffff),(4,fromIntegral attempts),(12,fromIntegral kind),(16,input),(20,target),(24,option),(28,choice),(32,0),(36,0),(0xa0,width),(0xa4,height),(0xa8,source),(0xac,destination),(0xb0,limit),(0xbc,0),(0xc0,if controls==Nothing then 0 else 1),(0xc4,1),(0xd0,0),(0xd4,stride),(0xd8,completed)]]++
 [Patch (Address (request+0xdc)) (maybe (B.replicate 80 0) (B.take 80) controls),word (request+0x128) (if controls==Nothing then 0 else halt)]

data EncoderInput = EncoderInput { inputWidth :: Word32,inputHeight :: Word32,inputSource :: Word32,inputDestination :: Word32,inputLimit :: Word32,inputHeaderStart :: Word32,inputLimits :: Word32,inputGeometry :: Word32,inputControls :: Word32,inputStride :: Word32 } deriving (Eq,Show)
encoderInputWords :: EncoderInput -> [Word32]
encoderInputWords i=[inputWidth i,0,inputHeight i,inputSource i,inputDestination i,inputLimit i,inputHeaderStart i,inputLimits i,inputGeometry i,inputControls i,inputStride i]
descriptorEffects :: Word32 -> Word32 -> Word32 -> Word32 -> Bool -> Maybe EncoderInput -> Maybe EncoderInput -> Maybe EncoderInput -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
descriptorEffects image source rawMode encoding initialize thumbnail primary secondary scale order flags kind completed halt = prefix++concatMap initializeInput jobs++aligned++shared
 where
  prefix=if initialize then [word (image+24) (if rawMode/=0 && encoding==1 then 2 else 0),word (image+0x424) 0,word (image+0x428) 0]++[word image source | rawMode==0] else []
  jobs=[(0x19c,0,160,120,0,10,1,0,i) | Just i<-[thumbnail]]++[(0x54,kind,inputWidth i,inputHeight i,1,scale,order,flags,i) | Just i<-[primary]]++[(0x2f4,1,inputWidth i,inputHeight i,2,10,order,flags,i) | Just i<-[secondary]]
  initializeInput (offset,mode,width,height,kind',attempts,option,choice,i)=metadataRequestEffects (image+offset) mode width height kind' (inputLimits i) (inputGeometry i) (fromIntegral attempts) option choice (inputSource i) (inputDestination i) (inputLimit i) (inputStride i) Nothing completed halt
  aligned=[word (image+0x420) ((inputHeaderStart i+63) .&. complement 63) | Just i<-[secondary]]
  shared=case primary of
   Just i | rawMode==0 -> [word (image+0x2c8) (inputWidth i),word (image+0x2cc) (inputHeight i),word (image+0x2d4) (inputSource i)]
   _ -> []

powerScale :: Int32 -> Int32 -> Word32
powerScale inputBase exponentBits=fromIntegral (max 3 (((sumTerms `shiftR` 13)+8192) `shiftR` 3))
 where
  wrap n=fromIntegral (fromIntegral n :: Word64) :: Int64
  x=fromIntegral inputBase*8-8192 :: Integer
  e=fromIntegral exponentBits*8 :: Integer
  powers=take 6 (iterate (\n -> fromIntegral (wrap (n*x)) `shiftR` 13) x)
  coefficients=scanl next e [2..6]
  next previous degree=fromIntegral (wrap (previous*(e-(degree-1)*8192)) `quot` fromIntegral degree) `shiftR` 13
  sumTerms=foldl (\s (coefficient,power) -> wrap (fromIntegral s+coefficient*power)) 0 (zip coefficients powers)

channel8RegisterEffects :: [Word32] -> [Patch]
channel8RegisterEffects configuration=[word address value | (address,value)<-registers]
 where
  f i=configuration!!i
  controls=foldr (.|.) ((f 53 .&. 1) `shiftL` 4) [(f (54+i) .&. 1) `shiftL` (16+i) | i<-[0..7]]
  registers=[(0xfff62000,(f 6 .&. 1) `shiftL` 2),(0xfff62028,controls),(0xfff62040,f 5 .&. 3),(0xfff62044,(f 1 .&. 65535) .|. (f 2 `shiftL` 16)),(0xfff62048,(f 7 .&. 15) .|. ((f 8 .&. 15) `shiftL` 4)),(0xfff6204c,f 9 .&. 1),(0xfff62050,(f 10 .&. 1) .|. ((f 11 .&. 255) `shiftL` 16)),(0xfff62080,f 12 .&. 65535),(0xfff62100,f 0),(0xfff6210c,f 3),(0xfff62110,f 4),(0xfff6211c,(f 46 .&. 15) .|. ((f 47 .&. 1) `shiftL` 4) .|. ((f 48 .&. 1) `shiftL` 5)),(0xfff62120,(f 49 .&. 1) .|. ((f 50 .&. 1) `shiftL` 1)),(0xfff62124,f 51 .&. 65535),(0xfff62128,f 52 .&. 65535),(0xfff62084,f 13 .&. 1)]++[(0xfff6208c,foldl (\n b -> (n `shiftL` 8) .|. fromIntegral b) 0 (take 4 (drop 124 tableBytes))) | f 13==1]
  tableBytes=B.unpack (B.concat (map wordBytes (take 32 (drop 14 configuration))))

encoderTaskScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
encoderTaskScenarios _ resolve = registryCases++metadataCases++descriptorCases++qualityCases++mathCases++producerCases++consumerCases++channelCases++templateCases++additionalCases
 where
  make name entry args initial final result pointers calls=StageScenario (StageCaseName ("encoder-task-"++name)) (publication entry) args initial final result pointers calls
  owned entry=addressWord (resolve (publication entry))
  worker=Worker 2
  queue=32
  entries=params+0x800
  image=0x9b0000
  request=params+0x1000
  completed=owned 0x11daa88
  halted=owned 0x11daa7c
  registryCases=
   [make "registry-initialize" 0x6e2390 [params,4,entries] [wordsAt entries (replicate 8 0xa5a5a5a5)] [wordsAt params [0,4,entries],wordsAt entries (concat (replicate 4 [0xffffffff,0xa5a5a5a5]))] Nothing [] []
   ,make "registry-scan-and-wrap" 0x6e24e0 [params,9] [wordsAt params [3,4,entries],wordsAt entries [0,101,0xffffffff,102,0,103,0,104]] [half params 2,word (entries+8) 9] (Just 1) [] []
   ,make "registry-exhaustion" 0x6e24e0 [params,9] [wordsAt params [2,4,entries],wordsAt entries [0,101,0,102,0,103,0,104]] [half params 2] (Just 0xffffffff) [] []
   ,make "registry-free-keeps-request-payload" 0x6e2588 [params,0] [wordsAt params [2,4,entries],wordsAt entries [0,101,0,102,0xffffffff,103,0xffffffff,104]] [word entries 0xffffffff,half (params+2) 1,word (entries+4) 101] Nothing [] []
   ,make "request-register-full" 0x6e274c [request] (requestRegistryInitial worker 0 entries++[word request 2]) [half (request+4) 0xffff,half 0x8faccc 1] (Just 0xffffffff) [] []
   ,make "request-initialize-owned-registry" 0x6e27ac [request,2,1,17,0,params,params+0x100,request] (requestRegistryInitial worker 4 entries)
      [wordsAt request [2,0x10000,17,0,params,params+0x100,0,request],word entries 0,word (entries+4) request,half (registryAddress worker) 1] Nothing [] []
   ]
  metadataCases=
   [make ("metadata-initialize-"++show withControls) 0x11dab4c [request,0x10002,640,480,1,params+0x400,params+0x500,10,1,0,params+0x600,params+0x6000,4096,23,if withControls then params else 0]
      [Patch (Address request) (B.replicate 0x12c 0xa5),Patch (Address params) controls]
      (metadataRequestEffects request 0x10002 640 480 1 (params+0x400) (params+0x500) 10 1 0 (params+0x600) (params+0x6000) 4096 23 (if withControls then Just controls else Nothing) completed halted) Nothing [] []
   | withControls<-[False,True],let controls=B.pack [0..79]]
  descriptorCases=
   [let input=EncoderInput 640 480 (params+0x600) (params+0x6000) 4096 (params+0x6041) (params+0x400) (params+0x500) 0 23
        thumb=input { inputWidth=111,inputHeight=222,inputSource=params+0x700 }
        secondary=input { inputWidth=320,inputHeight=240,inputSource=params+0x800,inputHeaderStart=params+0x7041 }
        primary=if includePrimary then Just input else Nothing
     in make ("descriptors-"++show (rawMode,initialize,includePrimary)) 0x11d28e8 [image,params+0x900,params,if includePrimary then params+0x100 else 0,params+0x200,7,2,3,4,if initialize then 1 else 0]
       [word (image+0xf2fc) rawMode,word (image+0xf304) 1,wordsAt params (encoderInputWords thumb),wordsAt (params+0x100) (encoderInputWords input),wordsAt (params+0x200) (encoderInputWords secondary),word (image+0xf4) 0,word (image+0xf8) 0]
       (descriptorEffects image (params+0x900) rawMode 1 initialize (Just thumb) primary (Just secondary) 7 2 3 4 completed halted) Nothing [] []
   | rawMode<-[0,1],initialize<-[False,True],includePrimary<-[True,False],rawMode/=0 || includePrimary]
  qualityCases=
   [let index=2*3+slot
        limits=(if kind==2 || kind==3 then 0x14ded70 else 0x14df450)+index*20
        geometry=0x14de9b0+index*12
     in make ("quality-select-"++show (kind,slot,cached)) 0x11dc264 [kind,640,480,slot,0,params,params+4,params+8]
        [byte 0x14de9ad 1,Patch (Address 0x11dc3a0) (halfBytes 640<>halfBytes 480<>B.pack [3,0]),word (limits+16) (if cached then 123 else 0),byte (0x14df3b0+index*2) 40]
        ([wordsAt params [limits,geometry,0x11dc39c]]++[word (limits+16) 153600 | not cached]) (Just 0) [] []
   | kind<-[0,1,2,3,4],slot<-[0,1],cached<-[False,True]]
  mathCases=
   [make ("power-scale-"++show (inputBase,exponentBits)) 0xd98a80 [fromIntegral inputBase,fromIntegral exponentBits] [] [] (Just (powerScale inputBase exponentBits)) [] [] | inputBase<-[0,512,1024,1536,2848],exponentBits<-[0,512,1024,1536,2048]]++
   [make ("request-first-quality-"++show value) 0x11dac34 [params,0,0,0,1] [word params value] [] (Just value) [] [] | value<-[1,10,40,0xffffffff]]
  producerCases=
   [make ("request-send-"++show code) 0x6e2b80 [request,fromIntegral code,17,29,0xffffffff]
      (requestQueueInitial worker queue++[wordsAt request [2,3]]) [] Nothing []
      (requestSendCalls queue 0x45 (RequestPacket (RequestEvent code) (RequestSlot 3) queue 17 29 (-1)))
   | code<-[0x2b00,0x2b05,0x2b10]]++
   [make ("compact-producer-"++show mode) 0x11d4a20 [2,image,0,mode,1]
      (requestQueueInitial worker queue++requestRegistryInitial worker 4 entries++[word (image+0xf2fc) 0])
      ([word 0x14ddba4 0,word (image+0xf2f0) 1,wordsAt 0x14ddb74 [2,0,1,0],wordsAt (0x14ddb74+24) [0,0x14ddb74,0],wordsAt (0x14ddb74+40) [image,mode],word entries 0,word (entries+4) 0x14ddb74,half (registryAddress worker) 1]++[word (image+0x424) 1 | mode==4]) Nothing []
      (requestSendCalls queue 0x45 (RequestPacket (RequestEvent 0x2b00) (RequestSlot 0) queue 0 0 0))
   | mode<-[0,4]]++
   [make "compression-producer-retains-payload-clears-history" 0x11daab8 [2,request,0x8f0800]
      (requestQueueInitial worker queue++requestRegistryInitial worker 4 entries++[Patch (Address (request+32)) (B.replicate 128 0xa5)])
      [word (0x14de980+40) request,word (0x14de980+32) 0x8f0800,word (request+8) 0,Patch (Address (request+32)) (B.replicate 128 0),word entries 0,word (entries+4) 0x14de980] Nothing []
      (requestSendCalls queue 0x45 (RequestPacket (RequestEvent 0x2b05) (RequestSlot 0) queue 0 0 0))]
  consumerCases=
   [make "compact-consumer-emits-real-primary-completion" 0x6e2aa0 [registryAddress worker,params]
      (requestQueueInitial worker queue++requestRegistryInitial worker 4 entries++[wordsAt entries [0,request],wordsAt request [2,0,1,0,params+0x100,params+0x200,0,request],word (params+0x200) (params+0x300),wordsAt (params+0x100) [0x2b00,5,0xffff,0xffffffff],wordsAt (params+0x300) [5,0,0x5b,0],word (request+40) image,word (request+44) 0,word (image+0xf2f0) 1,wordsAt 0x14ddb74 [2,0],Patch (Address params) (requestPacketBytes (RequestPacket (RequestEvent 0x2b00) (RequestSlot 0) queue 0 0 0))])
      [word (request+12) 2,word (request+24) params] Nothing [(Address (params+0x304),publication 0x11d268c)]
      (requestSendCalls queue 0x45 (RequestPacket (RequestEvent 0x2b10) (RequestSlot 0) queue 0 0 0))
   ,make "header-consumer-source-callback-after-unregister" 0x6e2aa0 [registryAddress worker,params]
      (requestRegistryInitial worker 4 entries++[word (registryAddress worker) 1,wordsAt entries [0,request],wordsAt request [2,0,1,0,params+0x100,params+0x200,0,request],word (params+0x200) (params+0x300),wordsAt (params+0x100) [0x2b10,0x16,0xffff,0xffffffff],wordsAt (params+0x300) [0x16,0,0x5b,0],word (request+32) 0x8f0800,word (request+40) image,word (request+44) 2,word (image+0x428) 2,word (image+0xf2fc) 0,word (image+0xf300) 0,word (image+0x100) (params+0x600),half (params+0x600) 0xffff,Patch (Address params) (requestPacketBytes (RequestPacket (RequestEvent 0x2b10) (RequestSlot 0) queue 0 0 0))])
      [word (request+24) params,word (image+0x428) 6,half (params+0x600) 0,word entries 0xffffffff,half (registryAddress worker+2) 1] Nothing [(Address (params+0x304),publication 0x11d4620)]
      [ScriptCall (Address 0x8f0800) [(N.R0,0)] [(N.R0,0)] [Absolute (word entries 0xffffffff),Absolute (word (image+0x428) 6)] []]
   ,make "hardware-completion-produces-typed-event" 0xd994c8 []
      (requestQueueInitial worker queue++[wordsAt 0x14de980 [2,3],word 0x14c578c request,word 0xfff62024 16,word 0xfff62094 4096,word (request+20) 8192,word (request+28) 99])
      [word (request+24) 4096,word (request+28) 0] Nothing [(Address (request+56),publication 0x11daa88)]
      (requestSendCalls queue 0x45 (RequestPacket (RequestEvent 0x2b10) (RequestSlot 3) queue 0 0 0))]
  channelCases=
   [let configuration=take 62 ([0x9a2000,640,480,0x9a4000,8192,1,1,2,2,0,0,0,0,0]++replicate 48 0)
     in make "channel-register-builder-active" 0x65ae00 [params] [wordsAt params configuration] (channel8RegisterEffects configuration) Nothing [] []
   ,make "channel-start-read-modify-write" 0x65adbc [] [word 0xfff62000 4] [word 0xfff62000 5] Nothing [] []
   ,make "dma-invalid-alignment" 0x666458 [params+1,params,1,0] [] [] (Just 0xffffffe2) [] []]
  templateCases=
   [make "measure-mixed-template" 0x11d2d5c [params]
      [wordsAt params [0,3,0,0,1,2,0,0,2,4,0,0,3,0,0,params+0x200,0xffffffff,0,0,0],Patch (Address (params+0x200)) (B.pack [65,66,67,0])] [] (Just 31) [] []
   ,make "measure-null-template" 0x11d2d5c [0] [] [] (Just 0) [] []]
  additionalCases=
   [make "request-receive-timeout-retains-caller-payload" 0x6e26dc [2,params]
      (receiveInitial (fromIntegral queue)++[byte (0x7d088c+2) queue,word params 0xa5a5a5a5]) [word params 0xa5a5a5a5] Nothing []
      (receiveCalls (fromIntegral queue) 0xffffffff 0 params 0 0 queue B.empty 0xffffffce)
   ,make "primary-error-unregisters-before-caller-failure" 0x11d4728 [request]
      (requestRegistryInitial worker 4 entries++diagnosticEventInitial++[word (registryAddress worker) 1,wordsAt entries [0,request],wordsAt request [2,0,1,2,0,0,params,request],word (params+4) 0xffffffff,word (request+32) 0x8f0800,word (request+40) image,word (request+44) 4,word (image+0x428) 0x40])
      ([word (request+12) 0,word entries 0xffffffff,word (image+0x428) 0x40]++diagnosticEventEffects 1 0x81) Nothing []
      (diagnosticEventCalls 1 0x81++[ScriptCall (Address 0x8f0800) [(N.R0,0xffffffff)] [(N.R0,0)] [Absolute (word entries 0xffffffff),Absolute (word (request+12) 0)] []])
   ,make "dma-program-consumes-initialized-register-tail" 0x665ac4 [7,params]
      [wordsAt 0x918bd8 [7,0,0,0,0,0,0,0],wordsAt params [0x455400,0,0,0x9a4000,0x9a8000,64,0x11111111,0x22222222,0x33333333,0,0]]
      [word address value | (address,value)<-[(0xfff0202c,0x455400),(0xfff02000,0x9a4000),(0xfff02004,0x9a8000),(0xfff02008,64),(0xfff0200c,0x11111111),(0xfff02010,0x22222222),(0xfff02014,0x33333333),(0xfff02030,0),(0xfff02034,0)]] (Just 0) [] []
   ,make "dma-open-runs-reservation-and-handle-allocation" 0x665304 [0,params]
      (criticalInitial++reservedInitial 11++[word 0x7c86bc 0,wordsAt 0x918bd8 (replicate 8 0),word 0x409c2804 10])
      [word params 11,word 0x918bd8 11,word 0x409c2804 11,word 0x918b98 1,word 0x918bb8 0] (Just 0) []
      (reservedWaitCalls 11 0xffffffff 0++criticalCalls)
   ,make "dma-wait-invalid-handle" 0x665760 [99]
      [wordsAt 0x918bd8 (replicate 8 0),word 0x918b98 2,word 0xfff02028 0x80]
      [word 0x918b98 2,word 0xfff02028 0x80] (Just 0xffffffec) [] []
   ,make "dma-wait-requires-active-state" 0x665760 [7]
      [wordsAt 0x918bd8 [7,0,0,0,0,0,0,0],word 0x918b98 1,word 0x918bf8 77,word 0xfff02028 0x80]
      [word 0x918b98 1,word 0x918bf8 77,word 0xfff02028 0x80] (Just 0xfffffff5) [] []
   ,make "dma-wait-idle-restores-open-without-stop-writes" 0x665760 [7]
      [wordsAt 0x918bd8 [0,7,0,0,0,0,0,0],word 0x918b9c 2,word 0x918bfc 0,word 0xfff02064 4,word 0xfff02068 0x80]
      [word 0x918b9c 1,word 0xfff02068 0x80] (Just 0) [] []
   ,make "dma-wait-releases-installed-interrupt" 0x665760 [7]
      (interruptInitial++[wordsAt 0x918bd8 [7,0,0,0,0,0,0,0],word 0x918b98 2,word 0x918bf8 77,word 0xfff02024 0,word 0xfff02028 0x80,word (0x940e9c+74*4) 77,word (0x940e9c+(74+128)*4) 57])
      ([word 0x918b98 1,word 0x918bf8 0,word 0xfff02028 0x80]++interruptReleaseEffects 74) (Just 0) [] (interruptReleaseCalls 74)
   ,make "dma-completion-notifies-real-task" 0x11d2620 [] (queueInitial 32++[word 0x14ddb54 0x45]) [] Nothing [] (notificationCalls 0x45 2 0 0)
   ,make "preview-pairs-source-row-and-chroma-selection" 0xd98f8c [params,0x9c0000,8,0,4,4,2,2]
      [Patch (Address params) (B.pack [0..63])]
      [Patch (Address (0x9c0000+19036)) (B.pack [32,36,34,35,40,44,42,43]),Patch (Address (0x9c0000+19356)) (B.pack [32,36,34,35,40,44,42,43])] Nothing [] []
   ,make "null-primary-retains-original-low-address-consumer-premise" 0x11d28e8 [image,params,0,0,0,0,0,0,0,0]
      [word (image+0xf2fc) 0,word (image+0xf4) 123,word (image+0xf8) 456,word 12 0xe59ff018]
      [wordsAt (image+0x2c8) [123,456],word (image+0x2d4) 0xe59ff018] Nothing [] []
   ]++
   [make ("quantization-"++show (scale,multiplier)) 0xd98a18 [params,params+0x100,scale,multiplier]
      [Patch (Address params) (B.pack [0..63]),Patch (Address 0xd99324) (B.pack [63,62..0]),Patch (Address 0x14c574c) (B.replicate 64 0xa5)]
      [Patch (Address (params+0x100)) (B.pack (1:[fromIntegral (max 1 (min 255 ((scale*multiplier*coefficient) `shiftR` 10))) | coefficient<-[62,61..0]])),Patch (Address 0x14c574d) (B.pack [62,61..0])] Nothing [] []
   | (scale,multiplier)<-[(0,1),(40,64),(1024,10)]]
