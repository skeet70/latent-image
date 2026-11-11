module CropProcessScenarios where

import Data.Bits ((.&.), (.|.), complement, shiftL)
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Data.Map.Strict as Map
import PipelineModel
import Scenarios
import ImagePlanesScenarios (priorityInitial, notificationCalls, currentTaskCalls, waitCalls, reservedInitial, reservedWaitCalls, reservedSignalCalls)
import qualified WarpEngineScenarios as Warp
import BuffersScenarios (criticalInitial, criticalEnterCalls, criticalLeaveCalls)
import DiagnosticLoggingScenarios (traceDisabledInitial)
import qualified CropEngineScenarios as Crop

newtype CropAddress = CropAddress Word32 deriving (Eq, Show)
newtype CropRowBytes = CropRowBytes Word32 deriving (Eq, Show)
newtype CropLines = CropLines Word32 deriving (Eq, Show)
newtype CropBufferBytes = CropBufferBytes Word32 deriving (Eq, Show)
data CropPlane = CropPlane Word32 CropAddress CropBufferBytes CropRowBytes CropRowBytes CropLines Word32 deriving (Eq, Show)
data CropParameters = CropParameters Word32 Word32 Word32 CropRowBytes Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 deriving (Eq, Show)
data Completion = Completed | Failed deriving (Eq, Show)

cropPlaneFromWords :: [Word32] -> CropPlane
cropPlaneFromWords [mode,address,size,stride,width,height,flags] = CropPlane mode (CropAddress address) (CropBufferBytes size) (CropRowBytes stride) (CropRowBytes width) (CropLines height) flags
cropPlaneFromWords _ = error "crop plane requires seven ABI words"
cropParametersFromWords :: [Word32] -> CropParameters
cropParametersFromWords [a,b,c,d,e,f,g,h,i,j,k,l,m,n] = CropParameters a b c (CropRowBytes d) e f g h i j k l m n
cropParametersFromWords _ = error "crop parameters require fourteen ABI words"
cropPlaneWords :: CropPlane -> [Word32]
cropPlaneWords (CropPlane a (CropAddress b) (CropBufferBytes c) (CropRowBytes d) (CropRowBytes e) (CropLines f) g) = [a,b,c,d,e,f,g]
cropParameterWords :: CropParameters -> [Word32]
cropParameterWords (CropParameters a b c (CropRowBytes d) e f g h i j k l m n) = [a,b,c,d,e,f,g,h,i,j,k,l,m,n]
cropPacketWords :: CropParameters -> CropPlane -> CropPlane -> Word32 -> [Word32]
cropPacketWords parameters input output completed = take 3 p ++ take 1 (drop 1 i) ++ drop 3 (take 6 i) ++ take 1 (drop 1 o) ++ take 1 (drop 3 o) ++ take 1 (drop 3 p) ++ drop 4 (take 6 o) ++ drop 4 p ++ [completed]
 where p=cropParameterWords parameters; i=cropPlaneWords input; o=cropPlaneWords output

cropProcessPublications :: [StagePublication]
cropProcessPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++name)) | (address,name) <- [(0xe650dc,"process_crop"),(0xe65268,"scale_configuration"),(0xe653d0,"copy_configuration"),(0xe64268,"process_planes"),(0xe65518,"geometry_debug"),(0xe64ffc,"crop_debug"),(0xe641a8,"processing_debug"),(0xe63f04,"crop_complete"),(0xe63ed0,"scale_complete"),(0xe63e9c,"copy_complete"),(0xe63f6c,"processing_complete"),(0xe563b0,"saved_request_accept"),(0xe563c8,"saved_request_unused"),(0x68d86c,"copy_acquire"),(0x68d8f8,"copy_device_release"),(0xda9824,"copy_initialize"),(0xda9854,"copy_release"),(0xda98f4,"copy_start"),(0xda9914,"copy_publish"),(0x68d9c0,"copy_program"),(0xda9774,"copy_interrupt"),(0xda9868,"copy_cancel"),(0x68cc48,"scale_acquire"),(0x68ccd4,"scale_device_release"),(0x6decf8,"scale_initialize"),(0x6ded24,"scale_release"),(0x6ded38,"scale_cancel"),(0x6ded8c,"scale_start"),(0x6dedb4,"scale_select_burst"),(0x6dee78,"scale_program"),(0x6def40,"scale_publish"),(0x6dec40,"scale_interrupt"),(0x68d184,"scale_coefficients")]]
cropProcessPublication :: Word32 -> StagePublication
cropProcessPublication address = case filter ((==Address address) . stageStockEntry) cropProcessPublications of
 p:_ -> p
 _ -> error "unknown crop process publication"

configurationBytePatches :: Word32 -> Word32 -> [Patch]
configurationBytePatches key value = [word 0x96ca50 (params+0x4000),Patch (Address (params+0x4000+key)) (B.singleton (fromIntegral value))]

cropProcessScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
cropProcessScenarios image resolve = debugCases ++ completionCases ++ copyProgramCases ++ copyPublishErrors ++ activeCases ++ copyInterruptCases ++ scaleCases
 where
  make name entry arguments initial final result calls = StageScenario (StageCaseName name) (cropProcessPublication entry) arguments initial (Map.elems (Map.fromList [(address,patch) | patch@(Patch address _) <- final])) result [] calls
  debugCases = [make ("crop-debug-disabled-"++show entry) entry arguments (configurationBytePatches 0xa8 0) [] Nothing [] | entry <- [0xe65518,0xe64ffc,0xe641a8], let arguments=if entry==0xe641a8 then [8,state] else [state]] ++
   [make ("crop-debug-active-"++show counter) 0xe64ffc [state]
    (configurationBytePatches 0xa8 0x80 ++ [word (state+0x220c) debug,word (debug+0xdbb08) counter] ++ zipWith word cropRegisters values)
    ([word (debug+0xdb984) 0x53545052] ++ zipWith word [debug+0xdb988+min counter 24*16+offset | offset<-[0,4,8,12]] values ++ [word (debug+0xdbb0c) 0x4c415052,word (debug+0xdbb08) (if counter>=24 then head values+1 else counter+1)]) Nothing []
   | counter <- [0,23,24,0xffffffff]] ++
   [make ("geometry-debug-active-"++show counter) 0xe65518 [state]
    (configurationBytePatches 0xa8 0x80 ++ [word (state+0x220c) debug,word (debug+0xdbf34) counter,word 0x014ce578 0,word 0xfff48004 timestampRegister] ++ zipWith word geometryRegisters geometryValues)
    ([word (debug+0xdbb10) 0x53544443,word (debug+0xdbb14+min counter 12*88) timestamp] ++ zipWith word [debug+0xdbb18+min counter 12*88+offset | offset<-[0,4..80]] geometryValues ++ [word (debug+0xdbf38) 0x4c414443,word (debug+0xdbf34) (if counter>=12 then timestamp+1 else counter+1)]) Nothing []
   | counter <- [0,11,12,0xffffffff]] ++
   [make "processing-debug-null-buffer" 0xe641a8 [8,state] (configurationBytePatches 0xa8 0x80 ++ [word (state+0x220c) 0]) [] Nothing []]
  debug = 0x02000000
  cropRegisters = [0xfff43008,0xfff43030,0xfff43034,0xfff43038]
  values = [0x10203040,0x11223344,0x55667788,0xaabbccdd]
  geometryRegisters = [0xfff48060,0xfff48064] ++ [0xfff48070,0xfff48074..0xfff480b8]
  geometryValues = [0x12340000+index | index <- [0..20]]
  timestampRegister = 0xfacebea8
  timestamp = (timestampRegister .&. complement 7) .|. (readWord image 0xda93d0 .&. 7)
  completionCases = [make ("crop-complete-idle-"++show entry) entry [] [word slot 0] [] Nothing [] | (entry,slot)<-completionSlots] ++
   [make ("crop-complete-active-"++show (entry,mode,result)) entry [] (priorityInitial 0x45 ++ [word slot 0x45]) [] Nothing (notificationCalls 0x45 0x10 mode result) | (entry,slot)<-completionSlots,mode<-[0,1],result<-[0,0xfffffffe]] ++
   [make ("saved-request-constant-"++show entry) entry [state] [] [] (Just result) [] | (entry,result)<-[(0xe563b0,1),(0xe563c8,0)]]
  completionSlots = [(0xe63f04,0x0157bcfc),(0xe63ed0,0x0157bd00),(0xe63e9c,0x0157bd04),(0xe63f6c,0x0157bcf0)]
  copyProgramCases = [make ("copy-program-"++show values) 0x68d9c0 [params] [wordsAt params values]
    [word address value | (address,value)<-copyHardwareEffects values] (Just (copyHardwareStatus values)) []
   | values <- [[0,0,0,0,16,8,0x009c0000,0x009d0000],[5,2,7,0x3ff,8192,8192,0xe09c0000,0xe09d0000],[6,0,0,0,16,8,0x009c0000,0x009d0000],[0,1,0,0,8193,8,0x009c0000,0x009d0000],[0,1,0,0,16,8,0x009c0001,0x009d0000],[0,1,0,0,16,8,0x009c0000,0x009d0001]]]
  copyPublishErrors = [make ("copy-publish-invalid-"++show values) 0xda9914 [params]
    [wordsAt params values,wordsAt 0x014ce6c8 (replicate 12 0)]
    ([wordsAt 0x014ce6d0 values,word 0x014ce6ec 0,word 0x014ce6f0 0] ++
      [word 0x014ce6e8 shift | let combined=values!!1 .|. values!!2,combined .&. 7==0,let shift=if combined .&. 31==0 then 2 else if combined .&. 15==0 then 1 else 0] ++
      [word address value | (address,value)<-copyHardwareEffects (take 1 values++[0,0,0]++take 4 (drop 1 values)),(values!!1 .|. values!!2) .&. 7==0])
    (Just 0xffffffe2) []
   | values <- [[0,7,8,0x009c0000,0x009d0000,0],[6,16,8,0x009c0000,0x009d0000,0]]]
  activeCases = [fromLaw ("crop-process-"++show completion) 0xe650dc [state,params,params+0x100,params+0x200]
    [wordsAt params (cropParameterWords cropParameters),wordsAt (params+0x100) (cropPlaneWords cropInput),wordsAt (params+0x200) (cropPlaneWords cropOutput)]
    completion (cropProcessLaw image resolve state cropParameters cropInput cropOutput completion) | completion<-[Completed,Failed]] ++
   [fromLaw ("copy-process-"++show completion) 0xe653d0 [state,params,params+0x100,params+0x200]
    [word params 0,wordsAt (params+0x100) (cropPlaneWords copyInput),wordsAt (params+0x200) (cropPlaneWords copyOutput)]
    completion (copyProcessLaw image resolve state 0 copyInput copyOutput completion) | completion<-[Completed,Failed]]
  fromLaw name entry arguments inputs completion (initial,final,calls,pointers) =
   StageScenario (StageCaseName name) (cropProcessPublication entry) arguments (inputs++initial)
    (Map.elems (Map.fromList [(address,patch) | patch@(Patch address _) <- final]))
    (Just (if completion==Completed then 1 else 0)) pointers calls
  cropParameters=cropParametersFromWords [0,0,0,32,0,0,1,26,26,25,38,37,36,36]
  cropInput=cropPlaneFromWords [0,0x009c0000,2048,128,64,16,0]
  cropOutput=cropPlaneFromWords [0,0x009d0000,512,64,32,8,0]
  copyInput=cropPlaneFromWords [0,0x009c0000,256,32,32,8,0]
  copyOutput=cropPlaneFromWords [0,0x009d0000,256,32,32,8,0]
  copyInterruptCases =
   [StageScenario (StageCaseName ("copy-interrupt-"++show (active,status))) (cropProcessPublication 0xda9774) []
    (Warp.interruptInitial ++ [wordsAt 0x014ce6c8 [7,active,0,16,8,0x009c0000,0x009d0000,callbackAddress],word 0xfff6f01c status,word 0x0157bd04 0x45,
     word (0x940e9c+59*4) 7,word (0x940e9c+(59+128)*4) 0x24])
    ([word 0x014ce6cc 0,word 0x014ce6e4 0,word 0x014ce6f4 status,word 0xfff6f020 status,word 0xfff6f024 0,word 0xfff6f000 0x10000] ++ if active==0 then [] else Warp.interruptReleaseEffects 59)
    Nothing [(Address 0x014ce6e4,cropProcessPublication 0xe63e9c)]
    (criticalEnterCalls ++ [copyResetCall] ++ (if active==0 then [] else Warp.interruptReleaseCalls 59) ++ criticalLeaveCalls ++ notificationCalls 0x45 0x10 0 0)
   | active<-[0,1],status<-[0,0x10001]] ++
   [StageScenario (StageCaseName ("copy-cancel-"++show active)) (cropProcessPublication 0xda9868) []
    (Warp.interruptInitial ++ [wordsAt 0x014ce6c8 [7,active,0,16,8,0x009c0000,0x009d0000,callbackAddress],word 0x0157bd04 0x45,
     word (0x940e9c+59*4) 7,word (0x940e9c+(59+128)*4) 0x24])
    ([word 0x014ce6cc 0,word 0x014ce6e4 0,word 0xfff6f020 0x10030003,word 0xfff6f000 0x10000] ++ if active==0 then [] else Warp.interruptReleaseEffects 59)
    Nothing [(Address 0x014ce6e4,cropProcessPublication 0xe63e9c)]
    (criticalEnterCalls ++ [copyResetCall] ++ (if active==0 then [] else Warp.interruptReleaseCalls 59) ++ criticalLeaveCalls ++ notificationCalls 0x45 0x10 0 0)
   | active<-[0,1]]
  callbackAddress=addressWord (resolve (cropProcessPublication 0xe63e9c))
  scaleCases =
   [fromLaw ("scale-process-"++show (configuration,completion)) 0xe65268 [state,params,params+0x100,params+0x200]
    [wordsAt params [configuration,params+0x300],wordsAt (params+0x100) (cropPlaneWords copyInput),wordsAt (params+0x200) (cropPlaneWords copyOutput),wordsAt (params+0x300) (scaleCoefficientWords unityCoefficients)]
    completion (scaleProcessLaw image resolve state configuration unityCoefficients copyInput copyOutput completion)
   | configuration<-[0,16,32,64],completion<-[Completed,Failed]] ++
   [make ("scale-program-"++show burst) 0x6dee78 [params,params+0x300]
    [wordsAt params scalePacket,wordsAt (params+0x300) [burst,0,0],wordsAt (params+0x400) (scaleCoefficientWords unityCoefficients)]
    (scaleProgramEffects scalePacket unityCoefficients burst) (Just 0) []
   | burst<-[0,1,2]] ++
   [make ("scale-coefficients-"++show coefficients) 0x68d184 [params]
    [wordsAt params (scaleCoefficientWords coefficients)] (scaleCoefficientEffects coefficients) Nothing []
   | coefficients<-[unityCoefficients,scaleCoefficientsFromWords [0xfffe0000,0x1ffff,0xfffffe00,15,0xc0000000,0xfffc0000,0x1ffff,0x1ff,15,0x1fffffff,8191,8191,0,8191]]] ++
   [make ("scale-burst-"++show (configured,width,range)) 0x6dedb4 [params,params+0x300]
    [wordsAt params [1,0x009c0000,512,width,8,0x009d0000,64,32,8,0,0,configured,0,range,0]]
    [wordsAt (params+0x300) [scaleBurstMode configured width 32 (range/=0),0,0]] (Just 0) []
   | (configured,width,range)<-[(0,32,0),(0,128,0),(0,256,0),(0,32,1),(16,32,0),(32,32,0),(64,32,0)]] ++
   [StageScenario (StageCaseName ("scale-interrupt-"++show status)) (cropProcessPublication 0x6dec40) []
    (criticalInitial ++ [word (0x00943250+56) scaleCallbackAddress,word 0x0157bd00 0x45,word 0xfff47074 status])
    ([word 0xfff47078 status] ++ if status .&. 1==0 then [] else [word 0xfff4707c 0,word 0xfff47000 0,word (0x00943250+56) 0])
    Nothing [(Address (0x00943250+56),cropProcessPublication 0xe63ed0)]
    (criticalEnterCalls ++ criticalLeaveCalls ++ if status .&. 1==0 then [] else criticalEnterCalls ++ criticalLeaveCalls ++ notificationCalls 0x45 0x10 0 0)
   | status<-[0,1,0x20]]
  unityCoefficients=scaleCoefficientsFromWords [65536,0,0,0,0,0,65536,0,0,0,0,0,0,0]
  scalePacket=[1,0x009c0000,32,32,8,0x009d0000,40,32,8,0,0,32,params+0x400,0,0]
  scaleCallbackAddress=addressWord (resolve (cropProcessPublication 0xe63ed0))

copyHardwareStatus :: [Word32] -> Word32
copyHardwareStatus [format,_,_,_,width,height,source,destination]
 | format>=6 || width>8192 || height>8192 || (width .|. height .|. source .|. destination) .&. 7/=0 = 0xffffffe2
 | otherwise = 0
copyHardwareStatus _ = error "copy hardware configuration requires eight ABI words"
copyHardwareEffects :: [Word32] -> [(Word32,Word32)]
copyHardwareEffects [format,shift,enabled,length,width,height,source,destination]
 | format>=6 = []
 | otherwise = [(0xfff6f008,(format .&. 7) .|. ((shift .&. 3) `shiftL` 4)),(0xfff6f00c,((enabled .&. 7) `shiftL` 28) .|. ((length .&. 0x3ff) `shiftL` 16))] ++
   if width>8192 || height>8192 || (width .|. height) .&. 7/=0 then [] else
    [(0xfff6f010,(((width-1) .&. 0x1fff) `shiftL` 16) .|. ((height-1) .&. 0x1fff))] ++
    if source .&. 7/=0 then [] else [(0xfff6f014,source .&. 0x1fffffff)] ++
     if destination .&. 7/=0 then [] else [(0xfff6f018,destination .&. 0x1fffffff)]
copyHardwareEffects _ = error "copy hardware configuration requires eight ABI words"

data CopyEngineLaw = CopyEngineLaw
 { copyEngineInitial :: [Patch]
 , copyEngineFinal :: [Patch]
 , copyEngineInitializeCalls :: [Call]
 , copyEnginePublishCalls :: [Call]
 , copyEngineStartCalls :: [Call]
 , copyEngineReleaseCalls :: [Call]
 , copyEngineCodePointers :: [(Address,StagePublication)]
 }

copyResetCall :: Call
copyResetCall = Call (Address 0x68d954) [] [] [word 0xfff6f000 0x10000]

copyEngineLaw :: (StagePublication -> Address) -> [Word32] -> CopyEngineLaw
copyEngineLaw resolve packet@[format,width,height,source,destination,completed]
 | (width .|. height) .&. 7/=0 || format>=6 || width>8192 || height>8192 || (source .|. destination) .&. 7/=0 = error "copy engine law requires an admitted hardware configuration"
 | otherwise = CopyEngineLaw
   (Warp.routeInitial ++ Warp.interruptInitial ++ reservedInitial 6 ++ [wordsAt 0x00955964 [0,0,0],word 0xfff8800c 0,word 0xfff8802c 0])
   ([wordsAt 0x00955964 [1,0,0],wordsAt 0x014ce6c8 ([2,1]++packet++[shift,0,0,0]),word 0x409c2804 2,
     word routeSlot 0,word routeSelection 0,word 0xfff8800c 7,word 0xfff8802c 0,
     word (0x409c1e30+0x758+23*4) 0,word (0x409c1e30+0x69c+23*4) 0,
     word (0x940e9c+59*4) 2,word (0x940e9c+(59+128)*4) 0x24,
     word 0xfff6f000 0x10000,word 0xfff6f020 0x10030003,word 0xfff6f024 0x10000000,word 0xfff6f004 1] ++
      [word address value | (address,value)<-copyHardwareEffects [format,shift,0,0,width,height,source,destination]])
   (reservedWaitCalls 6 0xffffffff 0 ++ Warp.routeRequestCalls True ++ Warp.routeAcquireCalls ++ [copyResetCall])
   (Warp.interruptAcquireCalls 59 13 1 interruptAddress)
   [call 0x42634 [0,0] 0]
   (Warp.routeReleaseCalls False ++ Warp.routeRequestCalls True ++ reservedSignalCalls 6 0 0)
   [(Address 0x014ce6e4,cropProcessPublication 0xe63e9c)]
 where
  shift=if (width .|. height) .&. 31==0 then 2 else if (width .|. height) .&. 15==0 then 1 else 0
  interruptAddress=addressWord (resolve (cropProcessPublication 0xda9774))
  routeSlot=0x409c1e84+3*0xa0+(35+2)*4
  routeSelection=0x409c1e84+3*0xa0+0x9c
copyEngineLaw _ _ = error "copy packet requires six ABI words"

type ProcessLaw = ([Patch],[Patch],[Call],[(Address,StagePublication)])

cropProcessLaw :: B.ByteString -> (StagePublication -> Address) -> Word32 -> CropParameters -> CropPlane -> CropPlane -> Completion -> ProcessLaw
cropProcessLaw = cropProcessLawWithSerial 0

cropProcessLawWithSerial :: Word32 -> B.ByteString -> (StagePublication -> Address) -> Word32 -> CropParameters -> CropPlane -> CropPlane -> Completion -> ProcessLaw
cropProcessLawWithSerial serial _image resolve _stage parameters input output completion =
 (Crop.engineInitial engine ++ disabledProcessInitial,
  wordsAt 0x0157a100 packet:word 0x0157bcfc 0:Crop.engineFinal engine,
  [call 0x42188 [0xe66698] 0] ++ Crop.engineInitializeCalls engine ++ Crop.enginePublishCalls engine ++
   [call 0x42188 [0xe666a4] 0] ++ currentTaskCalls 0x45 ++ Crop.engineBeginCalls engine ++ waitCalls 0x45 0x10 (completionEvents completion) 0 ++
   Crop.engineWaitCalls engine ++ [call 0x42188 [0xe666ac] 0] ++ Crop.engineReleaseCalls engine ++ [call 0x42188 [0xe666b4] 0],
  (Address 0x0157a158,publication):Crop.engineCodePointers engine)
 where
  publication=cropProcessPublication 0xe63f04
  packet=cropPacketWords parameters input output (addressWord (resolve publication))
  engine=Crop.cropEngineLawWithSerial serial resolve packet

copyProcessLaw :: B.ByteString -> (StagePublication -> Address) -> Word32 -> Word32 -> CropPlane -> CropPlane -> Completion -> ProcessLaw
copyProcessLaw _image resolve _stage format input output completion =
 (copyEngineInitial engine ++ disabledProcessInitial,
  wordsAt 0x01575b24 packet:word 0x0157bd04 0:copyEngineFinal engine,
  [call 0x42188 [0xe666e8] 0] ++ copyEngineInitializeCalls engine ++ copyEnginePublishCalls engine ++
   [call 0x42188 [0xe666f0] 0] ++ currentTaskCalls 0x45 ++ copyEngineStartCalls engine ++ waitCalls 0x45 0x10 (completionEvents completion) 0 ++
   copyEngineReleaseCalls engine ++ [call 0x42188 [0xe666f8] 0,call 0x42188 [0xe66700] 0],
  (Address 0x01575b38,publication):copyEngineCodePointers engine)
 where
  publication=cropProcessPublication 0xe63e9c
  i=cropPlaneWords input
  o=cropPlaneWords output
  packet=[format,i!!4 `div` 2,i!!5,i!!1,o!!1,addressWord (resolve publication)]
  engine=copyEngineLaw resolve packet

disabledProcessInitial :: [Patch]
disabledProcessInitial = traceDisabledInitial ++ priorityInitial 0x45 ++
 [word 0x96ca50 0x009e0000,Patch (Address 0x009e00a8) (B.singleton 0)]
completionEvents :: Completion -> Word32
completionEvents Completed = 0x10
completionEvents Failed = 0x20

data ScaleCoefficients = ScaleCoefficients Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 Word32 deriving (Eq,Show)
scaleCoefficientsFromWords :: [Word32] -> ScaleCoefficients
scaleCoefficientsFromWords [a,b,c,d,e,f,g,h,i,j,k,l,m,n] = ScaleCoefficients a b c d e f g h i j k l m n
scaleCoefficientsFromWords _ = error "scale coefficients require fourteen ABI words"
scaleCoefficientWords :: ScaleCoefficients -> [Word32]
scaleCoefficientWords (ScaleCoefficients a b c d e f g h i j k l m n) = [a,b,c,d,e,f,g,h,i,j,k,l,m,n]

scaleCoefficientEffects :: ScaleCoefficients -> [Patch]
scaleCoefficientEffects coefficients = [word address value | (address,value)<-
 [(0xfff47040,c!!0 .&. 0x3ffff),(0xfff47044,c!!1 .&. 0x3ffff),
  (0xfff47048,(c!!2 .&. 0x3ff) .|. ((c!!3 .&. 15) `shiftL` 16)),(0xfff4704c,c!!4 .&. 0x3fffffff),
  (0xfff47050,c!!5 .&. 0x3ffff),(0xfff47054,c!!6 .&. 0x3ffff),
  (0xfff47058,(c!!7 .&. 0x3ff) .|. ((c!!8 .&. 15) `shiftL` 16)),(0xfff4705c,c!!9 .&. 0x3fffffff),
  (0xfff47060,c!!10 .|. (c!!11 `shiftL` 16)),(0xfff47064,c!!12 .|. (c!!13 `shiftL` 16))]]
 where c=scaleCoefficientWords coefficients

scaleBurstMode :: Word32 -> Word32 -> Word32 -> Bool -> Word32
scaleBurstMode configured inputWidth outputWidth hasRange = encode (if configured/=0 then configured else choose (if hasRange then 32 else 64))
 where
  ratio=(inputWidth `shiftL` 12) `div` outputWidth
  choose size | size<16 = error "scale burst cannot hold source extent"
              | ratio*size+4095<=0xa0fff = size
              | otherwise = choose (size `div` 2)
  encode 64=0
  encode 32=1
  encode 16=2
  encode _=error "invalid explicit scale burst"

scaleProgramEffects :: [Word32] -> ScaleCoefficients -> Word32 -> [Patch]
scaleProgramEffects packet coefficients burst = [word address value | (address,value)<-
 [(0xfff47008,burst),(0xfff47010,packet!!1 .&. 0x3fffffff),(0xfff47014,packet!!2),
  (0xfff47018,packet!!3 .|. (packet!!4 `shiftL` 16)),(0xfff47020,packet!!5 .&. 0x3fffffff),(0xfff47024,packet!!6),
  (0xfff47028,packet!!7 .|. (packet!!8 `shiftL` 16)),(0xfff4702c,packet!!9 .|. (packet!!10 `shiftL` 16)),
  (0xfff47070,0),(0xfff4707c,0x1301),(0xfff47080,255)]] ++ scaleCoefficientEffects coefficients

scaleProcessLaw :: B.ByteString -> (StagePublication -> Address) -> Word32 -> Word32 -> ScaleCoefficients -> CropPlane -> CropPlane -> Completion -> ProcessLaw
scaleProcessLaw _image resolve _stage configuration coefficients input output completion =
 (Warp.routeInitial ++ Warp.interruptInitial ++ reservedInitial 5 ++ disabledProcessInitial ++ [word 0xfff88010 0,word 0xfff88030 0],
  [wordsAt 0x01575ae8 prefix,word 0x01575b1c 0,word 0x01575b20 completedAddress,
   wordsAt 0x00943250 prefix,wordsAt 0x00943284 [0,0,burst,0,0,2,0],
   wordsAt 0x009412c8 [1,0,0],word 0x0157bd00 0,word 0x409c2804 2,
   word (0x409c1e84+4*0xa0+(16+2)*4) 0,word (0x409c1e84+4*0xa0+0x9c) 0,
   word (0x409c1e30+0x758+21*4) 0,word (0x409c1e30+0x69c+21*4) 0,
   word 0xfff88010 7,word 0xfff88030 0,word 0xfff47000 0,word 0xfff47004 1] ++
   Warp.interruptReleaseEffects 67 ++ scaleProgramEffects packet coefficients burst,
  [call 0x42188 [0xe666c0] 0] ++ reservedWaitCalls 5 0xffffffff 0 ++ Warp.routeRequestCalls True ++ Warp.routeAcquireCalls ++
   Warp.interruptAcquireCalls 67 13 1 interruptAddress ++ [call 0x42188 [0xe666cc] 0] ++ currentTaskCalls 0x45 ++ [call 0x42634 [0,0] 0] ++
   waitCalls 0x45 0x10 (completionEvents completion) 0 ++ criticalEnterCalls ++ criticalLeaveCalls ++ Warp.interruptReleaseCalls 67 ++
   [call 0x42188 [0xe666d4] 0] ++ Warp.routeReleaseCalls False ++ Warp.routeRequestCalls True ++ reservedSignalCalls 5 0 0 ++ [call 0x42188 [0xe666dc] 0],
  [(Address 0x01575b20,cropProcessPublication 0xe63ed0)])
 where
  i=cropPlaneWords input
  o=cropPlaneWords output
  prefix=[1,i!!1,i!!3 `div` 2,i!!4 `div` 2,i!!5,o!!1,o!!3 `div` 2,o!!4 `div` 2,o!!5,0,0,configuration]
  packet=prefix++[0,0,completedAddress]
  completedAddress=addressWord (resolve (cropProcessPublication 0xe63ed0))
  interruptAddress=addressWord (resolve (cropProcessPublication 0x6dec40))
  burst=scaleBurstMode configuration (i!!4 `div` 2) (o!!4 `div` 2) False
