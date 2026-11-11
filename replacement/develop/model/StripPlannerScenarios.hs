{-# LANGUAGE GHC2021 #-}
module StripPlannerScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Word
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios (Address(..), Call(..), MemoryContract(..), Patch(..), word, wordsAt, wordBytes)
import BuffersScenarios (criticalInitial, criticalEnterCalls, criticalLeaveCalls)
import ImagePlanesScenarios (reservedInitial, reservedWaitCalls, reservedSignalCalls)
import WarpEngineScenarios (routeInitial, routeRequestCalls, routeAcquireCalls, routeAcquireEffects, routeReleaseCalls, interruptInitial, interruptAcquireCalls, interruptReleaseCalls, interruptReleaseEffects, diagnosticEventCalls, diagnosticEventEffects)

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_geometry_" ++ name))

publications :: [StagePublication]
publications = [publication address name | (address,name) <-
  [(0xda6840,"initialize"),(0xda6868,"release"),(0xda687c,"begin"),(0xda68b8,"wait"),
   (0xda6908,"margins"),(0xda6a6c,"dimensions"),(0xda713c,"strips"),(0xda7c30,"adjust"),
   (0xda7e18,"build_strip_request"),(0xda7e8c,"program_paired"),(0xda810c,"program_extended"),
   (0xda8380,"program_primary"),(0xda8634,"program_secondary"),(0xda8914,"program_simple"),
   (0xda8b00,"program_profile"),(0xda8e50,"program_alternate"),(0xda9124,"dispatch"),
   (0xda9230,"irq"),(0xda92f0,"configure"),(0xda9370,"mode"),(0xda9398,"finish")]]

requestAddress, marginsAddress, dimensionsAddress, planAddress, tailAddress :: Word32
requestAddress = 0x9a1000
marginsAddress = 0x9a1200
dimensionsAddress = 0x9a1300
planAddress = 0x9a1400
tailAddress = 0x9a1500

parameterAddress :: Word32 -> Word32
parameterAddress index = 0x9a6000 + (index-28)*0x100

sampleRequest :: Word32 -> [Word32]
sampleRequest kind =
  [kind,0x2000000,0x2100000,0x2200000,0,0x2300000,0x2400000,0x2500000,0,0x2600000,
   4096,4096,4096,2048,1024,32,0,0,0,0,128,0x2700000,4096,64,32,0,3,0] ++ map parameterAddress [28..33]

sampleInitial :: Word32 -> [Patch]
sampleInitial kind = [wordsAt requestAddress (sampleRequest kind),wordsAt dimensionsAddress (replicate 15 0),wordsAt planAddress (replicate 13 0),wordsAt tailAddress (replicate 13 0)] ++ [wordsAt (parameterAddress i) [0..36] | i <- [28..33]]

sampleMargins :: Word32 -> [Word32]
sampleMargins kind = case kind of
  0 -> [9,9,9,9,9,0]
  4 -> [0,0,0,0,0,1]
  5 -> [0,0,0,0,0,3]
  8 -> [8,8,8,8,0,0]
  9 -> [8,8,8,8,0,0]
  10 -> [8,8,8,8,0,0]
  _ -> replicate 6 0

sampleDimensions :: Word32 -> [Word32]
sampleDimensions kind = case kind of
  0 -> [0x2000000-102484,0x2100000-102484,0x2200000-102484,0,0,42,25,168,50,64,10,9,0,0,0]
  1 -> uncentered 64 0 0
  2 -> uncentered 64 0 0
  3 -> uncentered 64 0 0
  4 -> centered 16 2
  5 -> centered 16 4
  6 -> centered 32 0
  7 -> centered 16 0
  8 -> uncentered 12 8 8
  9 -> uncentered 24 8 8
  10 -> uncentered 96 8 8
  _ -> error "geometry fixture kind outside source table"
 where
  uncentered width x y = [0x2300000,0x2400000,0x2500000,0,0x2600000,0,0,128,32,width,x,y,0,0,0]
  centered x padding = [0x2000000-65568,0x2100000-65568,0x2200000-65568,0,0,x,16,64,32,64,0,0,0,0,padding]

samplePlan :: Word32 -> [Word32]
samplePlan kind = case kind of
  0 -> [0,0,1,168,50,0,0,64,0,50,0,8,64]
  1 -> [0,0,1,160,32,0,0,64,0,32,0,128,64]
  2 -> [0,0,1,160,32,0,0,64,0,32,0,128,64]
  3 -> [0,0,1,192,32,0,0,64,0,32,0,128,64]
  4 -> [0,0,1,96,32,0,0,64,0,32,0,8,64]
  5 -> [0,0,1,128,32,0,0,64,0,32,0,16,64]
  6 -> [0,0,1,64,32,0,0,64,0,32,0,64,64]
  7 -> [0,0,1,64,32,0,0,64,0,32,0,64,64]
  8 -> [0,0,1,128,32,0,0,12,0,1,0,128,4]
  9 -> [0,0,0,288,32,32,2,64,0,8,0,128,64]
  10 -> [0,0,1,192,32,40,0,96,0,16,0,128,32]
  _ -> error "geometry fixture kind outside source table"

samplePayload :: Word32 -> [Maybe Word32]
samplePayload kind = map Just prefix ++ suffix
 where
  d = sampleDimensions kind
  first = d!!0; second = d!!1; third = d!!2
  (prefix,suffix) = case kind of
    0 -> ([first,2048,second,2048,third,2048,0x2700000,4096,84,50,10,9,0,3,0,0,0,0,84,64,3,4,5,2,3,1,0],[])
    1 -> ([first,2048,second,2048,third,2048,0,1024,0x2700000,2048,0x2600000,1024,64,32,0,0,4,3,0,0,0,0,80,32,0,parameterAddress 29],[])
    4 -> ([first,2048,second,2048,third,2048,0,0,0x2700000,2048,32,32,0,0,0,3,0,0,0,0,48,32,3,1],[])
    7 -> ([first,2048,second,2048,third,2048,0,0,0x2700000,2048,32,32,0,0,3,3,0,0,0,0,32,32,3,0,0,1,2,3],[])
    8 -> ([first,2048,second,2048,0x2700000,1024,64,32,3,1,4,0,0,0,0,0,4,0,4,0,64,3,8,1,2,3,1,2,4,5,6,8,7],[])
    9 -> ([first,2048,second,2048,third,1024,0x2700000,4096,64,32,4,2,24,8,4,0,0,0,0,0,4,0,4,3,144,64,8,8,1,2,3,4,1,2] ++ [5..36],[])
    10 -> ([first,2048,second,2048,third,4096,0x2700000,2048,64,32,24,8], [Nothing,Nothing] ++ map Just ([4,2,0,0,0,0,4,0,4,0,96,48,40,8,16,0,0,1,1,2,0] ++ [3..19]))
    _ -> error "geometry payload fixture kind has no representative"

payloadEndpoint :: Word32 -> (Word32,Word32)
payloadEndpoint kind = case kind of
  0 -> (0x666a0c,0x942d90)
  1 -> (0x666cb4,0x942dfc)
  4 -> (0x666f98,0x942e64)
  7 -> (0x667128,0x942ec4)
  8 -> (0x667498,0x942f34)
  9 -> (0x6677c8,0x942fb8)
  10 -> (0x668144,0x9430c0)
  _ -> error "geometry endpoint fixture kind has no representative"

payloadCall :: Word32 -> [Maybe Word32] -> Call
payloadCall kind values = ScriptCall (Address endpoint) [] []
  [Relative N.R0 (index*4) (wordBytes value) | (index,Just value) <- zip [0..] values]
  ([Absolute (word (snapshot+fromIntegral index*4) value) | (index,Just value) <- zip [0::Int ..] values] ++
   [Absolute patch | patch <- secondaryRegisters, kind==7])
 where
  (endpoint,snapshot) = payloadEndpoint kind
  known index = case values!!index of Just value -> value; Nothing -> error "secondary payload contains an indeterminate word"
  secondaryRegisters = [word 0xfff48004 ((known 22 .&. 7) `shiftL` 4 .|. (known 23 .&. 3) `shiftL` 8),word 0xfff48060 ((known 24 .&. 1023) .|. (known 25 .&. 1023) `shiftL` 16),word 0xfff48064 ((known 26 .&. 1023) .|. (known 27 .&. 1023) `shiftL` 16)]

resetCall :: Call
resetCall = Call (Address 0x6689d4) [] [] [word 0xfff48000 1]

scenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
scenarios _ resolve = marginCases ++ dimensionCases ++ planCases ++ builderCases ++ payloadCases ++ edgeCases ++ lifecycleCases ++ driverCases
 where
  make name address symbol arguments initial final result calls = StageScenario (StageCaseName ("strip-planner-"++name)) (publication address symbol) arguments initial final result [] calls
  marginCases = [make ("margins-"++show kind) 0xda6908 "margins" [requestAddress,marginsAddress]
    (sampleInitial kind ++ [wordsAt marginsAddress (replicate 6 0xabcdef01)]) [wordsAt marginsAddress (sampleMargins kind)] Nothing [] | kind <- [0..10]]
  dimensionCases = [make ("dimensions-"++show kind) 0xda6a6c "dimensions" [requestAddress,marginsAddress,dimensionsAddress]
    (sampleInitial kind ++ [wordsAt marginsAddress (sampleMargins kind)])
    ([wordsAt dimensionsAddress (sampleDimensions kind)] ++ [wordsAt (parameterAddress 31+28) [0,0] | kind==8]) Nothing [] | kind <- [0..10]] ++
    [make "dimensions-zero-size" 0xda6a6c "dimensions" [requestAddress,marginsAddress,dimensionsAddress]
      (sampleInitial 7 ++ [word (requestAddress+92) 0,word (requestAddress+96) 0,wordsAt marginsAddress (replicate 6 0)])
      [wordsAt dimensionsAddress [0x2000000,0x2100000,0x2200000,0,0,0,0,0,0,0,0,0,0,0,0]] Nothing []]
  planCases = [make ("strips-"++show kind) 0xda713c "strips" [requestAddress,marginsAddress,dimensionsAddress,planAddress]
    (sampleInitial kind ++ [wordsAt marginsAddress (sampleMargins kind),wordsAt dimensionsAddress (sampleDimensions kind)])
    [wordsAt planAddress (samplePlan kind)] Nothing [] | kind <- [0..10]]
  builderCases = [make ("request-"++show kind) 0xda7e18 "build_strip_request" [requestAddress,planAddress,tailAddress]
    (sampleInitial kind ++ [wordsAt 0x14ce680 (replicate 15 0)])
    ([wordsAt 0x14ce600 (sampleMargins kind),wordsAt 0x14ce680 (sampleDimensions kind),wordsAt planAddress (samplePlan kind)] ++ [wordsAt (parameterAddress 31+28) [0,0] | kind==8]) (Just 0) [] | kind <- [0..10]]
  payloadCases = [make ("device-request-"++show kind) address symbol ([requestAddress,dimensionsAddress,planAddress]++[0 | kind<8])
    (sampleInitial kind ++ [wordsAt dimensionsAddress (sampleDimensions kind),wordsAt planAddress (samplePlan kind)]) [] Nothing [payloadCall kind (samplePayload kind)]
    | (kind,address,symbol) <- [(0,0xda7e8c,"program_paired"),(1,0xda810c,"program_extended"),(4,0xda8380,"program_primary"),(7,0xda8634,"program_secondary"),(8,0xda8914,"program_simple"),(9,0xda8b00,"program_profile"),(10,0xda8e50,"program_alternate")]]
  edgeCases =
    [make ("strips-wide-"++show width) 0xda713c "strips" [requestAddress,marginsAddress,dimensionsAddress,planAddress]
      (sampleInitial 7 ++ [word (dimensionsAddress+36) width])
      [wordsAt planAddress [0,0,width `div` 1024,1024,32,0,0,1024,width `mod` 1024,32,0,128,128]] Nothing [] | width <- [1024,1032,1040,2048]] ++
    [make ("tail-"++show width) 0xda7c30 "adjust" [requestAddress,dimensionsAddress,planAddress,tailAddress]
      (sampleInitial 7 ++ [wordsAt planAddress [0,0,3,1024,32,0,0,1024,width,32,0,128,128]])
      [wordsAt tailAddress [0,0,1,width,32,0,0,width,width,32,0,width,width]] Nothing [] | width <- [8,16,32,64,128]]
  lifecycleCases =
    [make ("mode-"++show kind) 0xda9370 "mode" [kind] [] [] (Just engine) [] | (kind,engine) <- zip [0..10] [1,2,2,2,3,3,4,4,5,6,7]] ++
    [make ("finish-"++show kind) 0xda9398 "finish" [] [word 0x14ce578 kind,word 0xfff48004 0x1234abcd] [] (Just ((0x1234abcd .&. complement 7) .|. engine)) [] | (kind,engine) <- [(0,1),(7,4),(10,7)]] ++
    [make "begin-secondary" 0xda687c "begin" [] [word 0x14ce578 7,word 0xfff48004 0x1234abcd]
      [word 0x942d8c 4,word 0xfff480c8 1,word 0xfff48004 0x1234abcc] Nothing [Call (Address 0x42634) [(N.R0,0),(N.R1,0)] [] []],
     make "wait-unregistered" 0xda68b8 "wait" [] [word 0x14ce574 0] [] (Just 0) [resetCall]] ++
    [make ("irq-complete-"++show callback) 0xda9230 "irq" []
      (criticalInitial ++ [word 0x14ce618 9,word 0x14ce638 0,word 0x14ce5e4 callback,word 0xfff480c0 0x331])
      [word 0x14ce618 10,word 0x14ce5e4 0,word 0xfff480c4 0x331] Nothing
      (criticalEnterCalls++criticalLeaveCalls++[resetCall]++criticalEnterCalls++criticalLeaveCalls++[Call (Address callback) [] [] [] | callback/=0]) | callback <- [0,0x8f0300]]
  driverCases =
    [make "configure-secondary" 0xda92f0 "configure" [requestAddress]
      (interruptInitial ++ sampleInitial 7 ++ [wordsAt 0x14ce570 (replicate 83 0)])
      [wordsAt 0x14ce578 (sampleRequest 7),wordsAt 0x14ce600 (sampleMargins 7),wordsAt 0x14ce618 (samplePlan 7),wordsAt 0x14ce680 (sampleDimensions 7),wordsAt 0x942ec4 [value | Just value <- samplePayload 7],word 0xfff480c8 0,wordsAt 0x14ce570 [1,1],word (0x940e9c+68*4) 1,word (0x940e9c+(68+128)*4) 61,word 0x409c2804 1]
      (Just 0) (payloadCall 7 (samplePayload 7) : interruptAcquireCalls 68 13 1 (addressWord (resolve (publication 0xda9230 "irq")))),
     make "configure-irq-handle-busy" 0xda92f0 "configure" [requestAddress]
      (interruptInitial ++ sampleInitial 7 ++ [wordsAt 0x14ce570 (replicate 83 0),word (0x940e9c+68*4) 7])
      ([wordsAt 0x14ce570 [0,0],word (0x940e9c+68*4) 7,word (0x940e9c+(68+128)*4) 0,word 0x409c2804 0] ++ diagnosticEventEffects 2 15)
      (Just 0xffffffec) ([payloadCall 7 (samplePayload 7)] ++ criticalEnterCalls ++ criticalLeaveCalls ++ diagnosticEventCalls 2 15),
     make "wait-registered" 0xda68b8 "wait" []
      (interruptInitial ++ [wordsAt 0x14ce570 [7,1],word (0x940e9c+68*4) 7,word (0x940e9c+(68+128)*4) 61])
      (word 0x14ce574 0 : interruptReleaseEffects 68) (Just 0) (resetCall : interruptReleaseCalls 68),
     make "initialize" 0xda6840 "initialize" []
      (routeInitial ++ reservedInitial 44 ++ [wordsAt 0x14ce570 (replicate 83 0xabcdef01),word 0xfff88010 0x1234abcd,word 0xfff88030 0x400])
      ([wordsAt 0x96eb9c [1,1,0],wordsAt 0x14ce570 (replicate 83 0),word 0xfff480c4 0x331,word (0x409c1e30+0x320) 8,word (0x409c1e30+0x370) 8,word 0xfff88010 0x1234abc8] ++ routeAcquireEffects 22 1 1 1 0x400)
      (Just 0) (reservedWaitCalls 44 0xffffffff 0 ++ routeRequestCalls True ++ routeAcquireCalls ++ [resetCall]),
     make "release" 0xda6868 "release" []
      (routeInitial ++ reservedInitial 44 ++ [wordsAt 0x96eb9c [1,1,0],word (0x409c1e30+0x758+22*4) 1,word (0x409c1e30+0x69c+22*4) 1,word (0x409c1e30+0x320) 8,word (0x409c1e30+0x370) 8,word 0xfff88010 0x1234abc8,word 0xfff88030 0x401])
      [word 0x96eba0 0,word (0x409c1e30+0x758+22*4) 0,word (0x409c1e30+0x69c+22*4) 0,word 0xfff88030 0x400,word (0x409c1e30+0x320) 0,word (0x409c1e30+0x370) 0,word 0xfff88010 0x1234abcf]
      (Just 0) ([resetCall] ++ routeReleaseCalls False ++ routeRequestCalls True ++ reservedSignalCalls 44 0 0),
     make "irq-tail-secondary" 0xda9230 "irq" []
      (criticalInitial ++ sampleInitial 7 ++ [wordsAt 0x14ce578 (sampleRequest 7),wordsAt 0x14ce680 (sampleDimensions 7),wordsAt 0x14ce618 [0,0,1,1024,32,0,0,1024,16,32,0,128,128],wordsAt 0x14ce64c [0,0,1,16,32,0,0,16,16,32,0,16,16],word 0xfff480c0 0x331])
      [word 0x14ce618 1,word 0x14ce638 0,word 0xfff480c4 0x331,word 0xfff480c8 1,word 0x942d8c 4,word 0xfff48004 0x34,wordsAt 0x942ec4 tailPayload]
      Nothing (criticalEnterCalls ++ criticalLeaveCalls ++ [resetCall,payloadCall 7 (map Just tailPayload),Call (Address 0x42634) [(N.R0,0),(N.R1,0)] [] []])]
  tailPayload = [0x2000000-65568+1024,2048,0x2100000-65568+1024,2048,0x2200000-65568+1024,2048,0,0,0x2700000+1024,2048,8,32,0,0,1,1,0,0,0,0,8,8,3,0,0,1,2,3]

newtype GeometryDimensionsWords = GeometryDimensionsWords [Word32] deriving (Eq,Show)
newtype GeometryPlanWords = GeometryPlanWords [Word32] deriving (Eq,Show)
newtype GeometryPayloadWords = GeometryPayloadWords [Word32] deriving (Eq,Show)
data GeometryCopyLaw = GeometryCopyLaw
  { copyDimensions :: GeometryDimensionsWords
  , copyMainPlan :: GeometryPlanWords
  , copyTailPlan :: Maybe GeometryPlanWords
  , copyMainRequest :: GeometryPayloadWords
  , copyTailRequest :: Maybe GeometryPayloadWords
  } deriving (Eq,Show)
data GeometryModelError = InvalidRequestWordCount | NotSecondaryCopyKind | ZeroOutputWidth | UnalignedTail deriving (Eq,Show)

geometryCopyLaw :: [Word32] -> (Word32,Word32,Word32,Word32) -> Either GeometryModelError GeometryCopyLaw
geometryCopyLaw request coefficients
  | length request /= 34 = Left InvalidRequestWordCount
  | request!!0 /= 7 = Left NotSecondaryCopyKind
  | total == 0 = Left ZeroOutputWidth
  | remainder /= 0 && remainder .&. 7 /= 0 = Left UnalignedTail
  | otherwise = Right GeometryCopyLaw
      { copyDimensions = GeometryDimensionsWords dimensions
      , copyMainPlan = GeometryPlanWords mainPlan
      , copyTailPlan = if remainder==0 then Nothing else Just (GeometryPlanWords tailPlan)
      , copyMainRequest = GeometryPayloadWords (payload mainPlan 0)
      , copyTailRequest = if remainder==0 then Nothing else Just (GeometryPayloadWords (payload tailPlan (count*(width*10 `div` 20))))
      }
 where
  width = min total 1024
  total = if request!!25==0 then request!!23 else (request!!23+request!!25+7) .&. complement 7
  count = total `div` width
  remainder = total `mod` width
  cx = ((request!!23*10 `div` 20) `div` 2) + request!!25*10 `div` 20
  cy = request!!24 `div` 2
  inputs = [request!!(index+1) - (request!!(index+10)*cy + cx*20 `div` 10) | index <- [0..2]] ++ [if request!!4==0 then 0 else request!!4-(request!!13*cy+cx*20 `div` 10)]
  dimensions = inputs ++ [0,cx,cy,(cx*20 `div` 10)*2,cy*2,total,0,0,0,0,0]
  inputBytes = ((20*(width*10 `div` 20) `div` 10)+7) .&. complement 7
  align n = maximum [unit | unit <- [1,2,4,8,16,32,64,128], n `mod` unit==0]
  mainPlan = [0,0,count,inputBytes,request!!24,0,0,width,remainder,request!!24,0,align inputBytes,align width]
  tailPlan = [0,0,1,remainder,request!!24,0,0,remainder,remainder,request!!24,0,align remainder,align remainder]
  code n = case n of 128->4;64->3;32->2;16->1;_->0
  (a,b,c,d) = coefficients
  payload plan offset = concat [[if request!!(index+1)==0 then 0 else inputs!!index+20*offset `div` 10,if request!!(index+1)==0 then 0 else request!!(index+10)*10 `div` 20] | index <- [0..3]] ++
    [request!!21+20*offset `div` 10,request!!22*10 `div` 20,plan!!2*(plan!!7*10 `div` 20),cy*2,0,0,code (plan!!11),code (plan!!12),0,0,0,0,plan!!3*10 `div` 20,plan!!7*10 `div` 20,request!!26,0,a,b,c,d]

geometryCopyDeviceCalls :: GeometryCopyLaw -> [Call]
geometryCopyDeviceCalls law = let GeometryPayloadWords values = copyMainRequest law in [payloadCall 7 (map Just values)]

geometryCopyTailDeviceCalls :: GeometryCopyLaw -> [Call]
geometryCopyTailDeviceCalls law = case copyTailRequest law of Nothing -> []; Just (GeometryPayloadWords values) -> [payloadCall 7 (map Just values)]

data GeometryFailureOutcome
  = GeometryAlignmentLoop Address StageSymbol
  | GeometryDivideSupervisorTrap Address
  deriving (Eq,Show)

data GeometryFailureScenario = GeometryFailureScenario
  { failureCase :: StageCaseName
  , failureEntry :: StagePublication
  , failureArguments :: [Word32]
  , failureInitial :: [Patch]
  , failureOutcome :: GeometryFailureOutcome
  } deriving (Eq,Show)

failureScenarios :: [GeometryFailureScenario]
failureScenarios =
  [GeometryFailureScenario (StageCaseName ("strip-tail-unaligned-"++show width)) (publication 0xda7c30 "adjust")
    [requestAddress,dimensionsAddress,planAddress,tailAddress]
    (sampleInitial 7 ++ [wordsAt planAddress [0,0,1,1024,32,0,0,1024,width,32,0,128,128]])
    (GeometryAlignmentLoop (Address 0xda7cb4) (StageSymbol "xm1_pipeline_image_geometry_fault")) | width <- [1,7,9,15]] ++
  [GeometryFailureScenario (StageCaseName "strip-zero-output-divide-trap") (publication 0xda7e18 "build_strip_request")
    [requestAddress,planAddress,tailAddress]
    (sampleInitial 7 ++ [word (requestAddress+92) 0,word (requestAddress+96) 0,wordsAt 0x14ce680 (replicate 15 0)])
    (GeometryDivideSupervisorTrap (Address 0x43614)),
   GeometryFailureScenario (StageCaseName "strip-extended-nondivisor-loop") (publication 0xda713c "strips")
    [requestAddress,marginsAddress,dimensionsAddress,planAddress]
    (sampleInitial 1 ++ [word (dimensionsAddress+36) 65,word (planAddress+28) 64])
    (GeometryAlignmentLoop (Address 0xda78f0) (StageSymbol "xm1_pipeline_image_geometry_fault"))]
