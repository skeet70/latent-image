{-# LANGUAGE GHC2021 #-}
module PreparationScenarios (preparationPublications, preparationScenarios) where

import qualified Data.ByteString as B
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.List as List
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import qualified ImagePlanesScenarios as ImagePlanes
import qualified BuffersScenarios as Buffers
import qualified DiagnosticLoggingScenarios as Logging
import Scenarios

preparationEntries :: [(Word32, Word32, String)]
preparationEntries =
  [(0xe93f18,0x62,"resize_aligned_pair"),(0xe94558,0x61,"copy_aligned_pair")
  ,(0xe94778,0x60,"blend_aligned_pair"),(0xe948c0,0x5f,"correct_pair_shear")
  ,(0xe94a44,0x5e,"blend_reference"),(0xe94bfc,0x5b,"analyze_pair_features")
  ,(0xe94dd0,0x5a,"extract_pair_features"),(0xe94f0c,0x59,"find_pair_alignment")
  ,(0xe95738,0x56,"blend_frame_edge"),(0xe95974,0x55,"measure_frame_motion")
  ,(0xe96054,0x58,"measure_predicted_motion")]

data ResizeShape = CropToFit | PadEven | PadOdd deriving (Eq, Show)

preparationPublications :: [StagePublication]
preparationPublications = [StagePublication (Address address) (StageSymbol ("xm1_develop_pipeline_" ++ name)) | (address,_,name) <- preparationEntries]

preparationScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
preparationScenarios image _resolve = map (lowerRtos . lowerDiagnostics . lowerObjects) (bypassCases ++ transitionCases ++ pairBlendCases ++ edgeBlendCases ++ motionCases ++ copyCases ++ referenceCases ++ featureCases ++ shearCases ++ alignmentCases ++ resizeCases ++ extractionCases)
 where
  lowerDiagnostics scenario = scenario
    {stageInitial = stageInitial scenario ++ [Patch (Address traceBuffer) (B.replicate 1024 0xa5)] ++ Logging.traceInitial traceBuffer 7 29999 0x12345678 ++ Logging.textInitial textBuffer 64 calendar
    ,stageFinal = stageFinal scenario ++ overlay (traceEffects ++ textEffects)
    ,stageCalls = concatMap lower (stageCalls scenario)}
   where
    traceBuffer = 0x009d0000
    textBuffer = 0x009e0000
    calendar = Logging.CalendarFields 26 11 10 14 15 16
    traces = [register N.R0 (callInputs retained) .|. if callAddress retained == Address 0xeb0ccc then 0x10000000 else 0 | retained <- stageCalls scenario,callAddress retained `elem` [Address 0xeb0cbc,Address 0xeb0ccc]]
    texts = [retained | retained <- stageCalls scenario,callAddress retained == Address 0x726658]
    traceEffects = concat [Logging.traceEffects traceBuffer (7+index) 29999 0x12345678 operation | (index,operation) <- zip [0..] traces]
    textEffects = concat [Logging.textEffects textBuffer (64+index*40) (register N.R0 (callInputs retained)) (B.take 32 (B.drop (fromIntegral (register N.R1 (callInputs retained)-0xbf1a00)) image)) calendar | (index,retained) <- zip [0..] texts]
    lower retained
      | callAddress retained == Address 0x726658 = Logging.textCalls
      | callAddress retained `elem` [Address 0xeb0cbc,Address 0xeb0ccc] = Logging.traceCalls
      | otherwise = [retained]
  overlay patches = [Patch (Address address) (B.singleton value) | (address,value) <- List.nubBy (\first second -> fst first == fst second) (reverse [(address+fromIntegral offset,value) | Patch (Address address) content <- patches,(offset,value) <- zip [0..] (B.unpack content)])]
  lowerObjects scenario
    | null bindings = scenario
    | otherwise = scenario
        {stageInitial = stageInitial scenario ++ Buffers.objectLookupInitial (state+0x7c) 7 0 [(group,kind,address,planeAt address) | (group,kind,address) <- bindings]
        ,stageCalls = concatMap lower (stageCalls scenario)}
   where
    bindings = List.nub [(register N.R1 (callInputs retained),register N.R2 (callInputs retained),register N.R0 (callOutputs retained)) | retained <- stageCalls scenario,callAddress retained == Address 0xe44dbc]
    lower retained
      | callAddress retained == Address 0xe44dbc = Buffers.criticalCalls
      | otherwise = [retained]
    planeAt address = Buffers.Plane (field 0) (field 4) (field 8) (field 12) (field 16) (field 20) (field 24)
     where field offset = sum [fromIntegral (byteAt (address+offset+index)) `shiftL` fromIntegral (index*8) | index <- [0..3]]
    byteAt address = case [B.index content (fromIntegral (address-start)) | Patch (Address start) content <- stageInitial scenario,address >= start,fromIntegral (address-start) < B.length content] of
      [] -> 0
      values -> last values
  lowerRtos scenario = scenario
    {stageCalls = concatMap rtosCall (stageCalls scenario)
    ,stageFinal = stageFinal scenario ++ [word (register N.R0 (callInputs retained)+(0x123+5)*4) 1 | retained <- stageCalls scenario,callAddress retained == Address 0xe4eca8]}
  rtosCall retained
    | callAddress retained == Address 0x723cb8 = ImagePlanes.currentTaskCalls (register N.R0 (callOutputs retained))
    | callAddress retained == Address 0x723a14 = ImagePlanes.priorityCalls (register N.R0 (callInputs retained)) (register N.R1 (callInputs retained))
    | callAddress retained == Address 0x724638 = ImagePlanes.signalCurrentCalls 0x123 (register N.R0 (callOutputs retained))
    | callAddress retained == Address 0xe4eca8 = ImagePlanes.currentTaskCalls 0x123
    | otherwise = [retained]
  register name values = case List.lookup name values of
    Just value -> value
    Nothing -> error "missing RTOS service argument"
  preparation = 0x009c0000
  frames = 0x009c1000
  objects = 0x009c2000
  firstPixels = 0x02001000
  secondPixels = 0x02003000
  destinationPixels = 0x02005000
  referencePixels = 0x02007000
  plane address width height = [1,address,64*height,64,width,height,0]
  bytes = B.concat . map wordBytes
  common = base ++ [word (context+0x44) 3,word (context+0x48) 2,word (state+0x3228) preparation,word (state+0x3150) frames,word (state+0x5cc) 4,word (state+0x5d0) 0,word (state+0x4b4) 0x1d,wordsAt (state+0x184) (plane firstPixels 32 8),wordsAt (preparation+0x1c) (plane firstPixels 32 8),wordsAt (preparation+0x38) (plane secondPixels 32 8),wordsAt (preparation+0x8c) (plane referencePixels 32 8),wordsAt (preparation+0xfc) [4,0,0,0,0,0],wordsAt (preparation+0x11c) [6,2,0],word (preparation+0x130) 0,Patch (Address firstPixels) (B.replicate 2048 32),Patch (Address secondPixels) (B.replicate 2048 224),Patch (Address destinationPixels) (B.replicate 2048 64),Patch (Address referencePixels) (B.replicate 2048 224)]
    ++ [wordsAt (preparation+0x54) (plane firstPixels 32 8),wordsAt (preparation+0x70) (plane secondPixels 32 8)]
    ++ ImagePlanes.priorityInitial 0x123 ++ ImagePlanes.priorityInitial 11
  publication operation = case [p | (entry,p) <- zip preparationEntries preparationPublications, let (_,code,_) = entry, code == operation] of
    [p] -> p
    _ -> error "preparation publication missing"
  plan operation = search 0
   where
    search index
      | index > 126 = error "preparation plan missing"
      | readWord image address == operation && readWord image (address+28) == 13 = [readWord image (address+field*4) | field <- [0..12]]
      | otherwise = search (index+1)
     where address = 0xea6738+index*52
  make operation name initial final result calls = StageScenario (StageCaseName ("preparation-" ++ name)) (publication operation) [context] (common++initial) final (Just result) [] calls
  active operation name initial final completion body = make operation name initial (word (state+0x38) ((plan operation) !! if completion == 1 then 4 else 3):final) 0 (beginCalls (plan operation) ++ body ++ completeCalls (plan operation) completion)
  bypassCases =
    [make operation (name++"-disabled") [word (state+0x5cc) 0,word (state+0x2780) 0,word (state+0x2790) 0] [word (state+0x38) ((plan operation)!!4)] 0 [] | (_,operation,name) <- preparationEntries] ++
    [make operation (name++"-flag-"++show flag) [word (preparation+flag) 1] [word (state+0x38) ((plan operation)!!4)] 0 [] | (_,operation,name) <- take 8 preparationEntries, flag <- [0x110,0x114,0x10c]] ++
    [make 0x55 "measure-frame-other-phase" [word (state+0x2780) 1,word (state+0x2784) 2] [word (state+0x38) ((plan 0x55)!!4)] 0 [],make 0x58 "measure-predicted-other-phase" [word (state+0x2780) 1,word (state+0x2784) 0] [word (state+0x38) ((plan 0x58)!!4)] 0 []]
  transitionCases = concat
    [ [make operation (name++"-transition") (enable operation ++ [word (context+0x44) 0,word (state+0x38) 0x102,word 0xea254c (context+0x1900),wordsAt (context+0x1900) [operation,0x101,0]]) [] 5 (take 3 (beginCalls (plan operation))),
       make operation (name++"-simple-refusal") (enable operation ++ [word (context+0x48) 0,word 0xea1ee0 (context+0x1900),wordsAt (context+0x1900) [operation,0x80]]) [] 6 (take 3 (beginCalls (plan operation)))]
    | (_,operation,name) <- preparationEntries]
  enable operation
    | operation == 0x56 = [word (state+0x2790) 1]
    | operation == 0x55 = [word (state+0x2780) 1,word (state+0x2784) 0]
    | operation == 0x58 = [word (state+0x2780) 1,word (state+0x2784) 1]
    | otherwise = []
  cfg key value = call 0x69f8fc [key] value
  cfg16 key value = [cfg (key+1) (value `shiftR` 8),cfg key (value .&. 255)]
  half key value = call 0x69fec8 [key] value
  lookup group descriptor = call 0xe44dbc [state+0x7c,group,0] descriptor
  blendEvents = [call 0x723cb8 [] 0x123,call 0x723a14 [0x123,12] 0,call 0xeb0d04 [] 0,call 0xeb0d04 [] 0,call 0x723a14 [0x123,6] 0]
  goldenRow = B.concat [B.replicate 4 value | value <- [32,80,128,176]]
  blended address rows = [Patch (Address (address+row*64)) goldenRow | row <- [0..rows-1]]
  pairBlendCases =
    [active 0x60 ("blend-pair-"++show displacement) [word (preparation+0x11c) 0,word (preparation+0xfc) 0,word (preparation+0x100) displacement]
       (blended (secondPixels + if displacement == 0xffffffff then 64 else 0) (if displacement == 0 then 8 else 7)) 1
       ([cfg 0x9fa9 8,cfg 0x9fa9 8]++blendEvents)
    | displacement <- [0,1,0xffffffff]]
  edgeBlendCases =
    [active 0x56 ("blend-edge-"++show displacement) (enable 0x56 ++ [word (state+0x5cc) 0,Patch (Address (state+0x1680)) (B.pack [2,0]),word (state+0x3198) displacement,word (state+0x3178) 0,word (state+0x3174) 0,word (state+0x3154) 0,word (state+0x3168) 4,Patch (Address (state+0x3194)) (B.singleton 16),wordsAt (state+0x184) (plane secondPixels 32 8),wordsAt objects (plane firstPixels 32 32)])
      (blended (firstPixels+496+if displacement == 1 then 64 else 0) 4) 1
      ([cfg 0x9f64 2,lookup 0x28 objects,cfg 0x9f0f 8,cfg 0x9f0f 8,cfg 0x9f0f 8]++blendEvents)
    | displacement <- [0,1,0xffffffff]] ++
    [active 0x56 ("edge-bypass-count-"++show count) (enable 0x56 ++ [Patch (Address (state+0x1680)) (B.pack [count,0])]) [] 1 [] | count <- [0,1]] ++
    [active 0x56 "edge-bypass-frame-kind" (enable 0x56 ++ [word (state+0x5cc) 0,Patch (Address (state+0x1680)) (B.pack [2,0]),Patch (Address (state+0x3194)) (B.singleton 8)]) [] 1 [cfg 0x9f64 2]]
  motionPlane address = [1,address,8192,256,128,32,0]
  motionInitial predicted = enable (if predicted then 0x58 else 0x55) ++
    [word (state+0x5cc) 0,word (state+0x5d0) (if predicted then 1 else 0),Patch (Address (state+0x1680)) (B.pack [1,0]),word (state+0x2788) 1,word (state+0x3224) 0x80027ffc,word (state+0x31ac) 1,word (state+0x31b0) 0xffffffff,word (state+0x1760) 100,word (state+0x315c) 10,word (state+0x3160) 20,word (state+0x3198) 1,word (state+0x319c) 0xfffffff6,word (state+0x31a0) 12,wordsAt (state+0x184) (motionPlane firstPixels)] ++
    [wordsAt (state+offset) (motionPlane pixels) | (offset,pixels) <- [(0x30dc,firstPixels),(0x30f8,secondPixels),(0x3114,referencePixels),(0x3130,destinationPixels)]] ++
    [Patch (Address address) (B.replicate 8192 32) | address <- [firstPixels,secondPixels,destinationPixels,referencePixels]]
  motionEvents alternate =
    [call 0x723cb8 [] 0x123,call 0x723a14 [0x123,13] 0,cfg 0x9f7b 4,cfg 0x9f05 1,cfg 0x9f06 1,cfg 0x9f04 0,call 0xeb0d04 [] 0,
     cfg (if alternate then 0x9fee else 0x9f8b) 0,cfg (if alternate then 0x9fe0 else 0x9f07) 0,cfg (if alternate then 0x9fe2 else 0x9f09) 0,cfg (if alternate then 0x9fe1 else 0x9f08) 1,cfg (if alternate then 0x9fe3 else 0x9f0a) 1,
     cfg (if alternate then 0x9fee else 0x9f8b) 0,cfg 0x9f0d 0,cfg 0x9f0e 1,cfg 0x9f0d 0,cfg 0x9f0b 0,cfg 0x9f0c 1,cfg 0x9f0b 0,
     call 0xeb0d04 [] 0,call 0x723a14 [0x123,6] 0]
  predictionEvents = [cfg 0x9f77 128,cfg 0x9f79 2,cfg 0x9f78 128,cfg 0x9f7a 2] ++ cfg16 0x9f84 100 ++ [cfg 0x9f94 0,cfg 0x9f95 0]
  motionCases =
    [active operation ("motion-flat-patch-"++show predicted) (motionInitial predicted)
      [word (state+0x314c) 0,wordsAt (state+0x31b4) [10,0,20,20,1600,31,5,15,37,0,10,0,1,1,0,37],Patch (Address (state+0x31f4)) (B.singleton 0),wordsAt (state+0x31f8) [1,1,1,0,if predicted then 4 else 0,0,0]] 1
      ((if predicted then predictionEvents else []) ++ motionEvents False ++ cfg16 0x9f02 1600 ++ cfg16 0x9f6d 2 ++ cfg16 0x9f18 100 ++ cfg16 0x9f20 20 ++ cfg16 0x9f00 1600 ++ cfg16 0x9f14 16)
    | predicted <- [False,True],let operation = if predicted then 0x58 else 0x55]
  planeOperation input output effects = ScriptCall (Address 0xe657ac) [(N.R0,state),(N.R3,2)] [(N.R0,1)]
    [Relative N.R1 0 (wordBytes 7),Indirect N.R2 0 (bytes input),Indirect N.SP 0 (bytes output)] (map Absolute effects)
  copyCases =
    [active 0x61 ("copy-pair-"++show displacement) [word (preparation+0x100) displacement,wordsAt objects (plane destinationPixels 32 16)]
      [wordsAt (preparation+0x38) (plane outputAddress 32 16),word (preparation+0xfc) 6,Patch (Address destinationPixels) (B.replicate 32 32)] 1
      [lookup 0x36 objects,half 0x9f12 16,planeOperation (plane firstPixels 32 16) (plane (destinationPixels+negative) 32 16) [Patch (Address destinationPixels) (B.replicate 32 32)],half 0x9f12 16,planeOperation (plane (secondPixels+4) 32 16) (plane outputAddress 32 16) []]
    | displacement <- [0,1,0xffffffff],let negative = if displacement == 0xffffffff then 64 else 0,let positive = if displacement == 1 then 64 else 0,let outputAddress = destinationPixels+positive+12]
  referenceCases =
    [active 0x5e "blend-reference-active" [wordsAt objects (plane destinationPixels 32 8)]
      ([wordsAt (preparation+0x1c) (plane destinationPixels 32 8)]++blended referencePixels 8) 1
      [lookup 0x33 objects,planeOperation (plane firstPixels 32 8) (plane destinationPixels 32 8) [Patch (Address destinationPixels) (B.replicate 512 32)],cfg 0x9fa9 8,cfg 0x9fa9 8] `withEvents` blendEvents]
  withEvents scenario events = scenario {stageCalls = let original = stageCalls scenario; suffix = completeCalls (plan 0x5e) 1; prefix = take (length original-length suffix) original in prefix++events++[planeOperation (plane referencePixels 32 8) (plane (destinationPixels+4) 32 8) []]++suffix}
  featureCases =
    [active 0x5b "feature-pyramid-minimum-level" [wordsAt objects (plane destinationPixels 32 8)] [] 1
      [cfg 0x9fc5 0,lookup 0x3a objects,cfg 0x9fc5 0,lookup 0x3a objects,cfg 0x9fc5 0]]
  scaleOperation input output effects = ScriptCall (Address 0xe65268) [(N.R0,state)] [(N.R0,1)]
    [Relative N.R1 0 (wordBytes 64),Relative N.R2 0 (bytes input),Relative N.R3 0 (bytes output)] (map Absolute effects)
  shearCases =
    [active 0x5f ("shear-pair-"++show displacement)
      [word (preparation+0x100) displacement,wordsAt (preparation+0x1c) (plane firstPixels 32 32),wordsAt (preparation+0x38) (plane secondPixels 32 32),wordsAt objects (plane destinationPixels 32 32),wordsAt (objects+28) (plane referencePixels 32 32)]
      [wordsAt (preparation+0x1c) (plane destinationPixels 32 32),wordsAt (preparation+0x38) (plane referencePixels 32 32),word (preparation+0x130) slope,word (preparation+0x100) displacement,Patch (Address (destinationPixels+256)) (B.replicate 32 32),Patch (Address (referencePixels+256)) (B.replicate 32 224)] 1
      [lookup 0x34 objects,lookup 0x35 (objects+28),cfg 0x9fdf 0,
       scaleOperation (plane firstPixels 32 32) [1,destinationPixels+256,768,64,32,24,0] [Patch (Address (destinationPixels+256)) (B.replicate 32 32)],
       cfg 0x9fdf 0,scaleOperation (plane secondPixels 32 32) [1,referencePixels+256,768,64,32,24,0] [Patch (Address (referencePixels+256)) (B.replicate 32 224)]]
    | displacement <- [2,0xfffffffe],let slope = if displacement == 2 then 6553 else 0xffffe667]
  resizeOperation input output effects = ScriptCall (Address 0xe64a44) [(N.R0,state),(N.R1,0),(N.R2,0)] [(N.R0,1)]
    [Relative N.R3 4 (bytes (take 5 (drop 1 input))),Indirect N.SP 0 (bytes output),Relative N.SP 8 (bytes [0,1])] (map Absolute effects)
  reduceOperation input output = ScriptCall (Address 0xe64a44) [(N.R0,state),(N.R1,0),(N.R2,0)] [(N.R0,1)]
    [Relative N.R3 4 (bytes (take 5 (drop 1 input))),Indirect N.SP 0 (bytes output),Relative N.SP 8 (bytes [0,2])] []
  widePlane address width height = [1,address,512*height,512,width,height,0]
  trimPlane address = [1,address,6912,512,108,64,0]
  alignmentInitial = [wordsAt objects (widePlane firstPixels 256 128),wordsAt (objects+28) (widePlane referencePixels 128 64),wordsAt (objects+56) (widePlane destinationPixels 128 64),wordsAt (objects+84) (widePlane destinationPixels 108 64),wordsAt (objects+112) (widePlane referencePixels 108 64),word (state+0x321c) 256,Patch (Address firstPixels) (B.replicate 65536 32)]
  alignmentPrefix =
    [lookup 0x28 objects,cfg 0x9f6f 0] ++ cfg16 0x9f12 64 ++ cfg16 0x9f12 64 ++ cfg16 0x9f12 64 ++
    [cfg 0x9f66 0,lookup 0x2a (objects+28),lookup 0x2b (objects+56)] ++ cfg16 0x9fa2 2 ++
    [reduceOperation [1,firstPixels+4096,8192,512,128,64,0] (widePlane destinationPixels 128 64)]
  alignmentBaseFinal =
    [wordsAt (preparation+0x20) [firstPixels+4336,30464,512,272,112],wordsAt (preparation+0x38) [1,firstPixels+4096,30464,512,272,112,0],word (preparation+0x104) 128]
  alignmentCases =
    [active 0x59 "alignment-no-search-positions" alignmentInitial
      (alignmentBaseFinal++[word (preparation+0x110) 1]) 1
      (alignmentPrefix++[cfg 0x9fa4 0]++cfg16 0x9fa0 4++cfg16 0x9fe8 8),
     active 0x59 "alignment-flat-patch-found" alignmentInitial
      (alignmentBaseFinal++[word (preparation+0xfc) 82,word (preparation+0x100) 0,word (preparation+0x108) 46,wordsAt (preparation+0x54) (trimPlane destinationPixels),wordsAt (preparation+0x70) (trimPlane referencePixels),Patch (Address destinationPixels) (B.replicate 8 32),Patch (Address referencePixels) (B.replicate 8 32)]) 1
      (alignmentPrefix++[cfg 0x9fa4 1,reduceOperation [1,firstPixels+4480,8192,512,128,64,0] (widePlane referencePixels 128 64)]++
       motionEvents True++cfg16 0x9f12 64++cfg16 0x9f12 64++cfg16 0x9f12 64++
       [cfg 0x9fa4 1]++cfg16 0x9fa0 4++cfg16 0x9fe8 8++
       [lookup 0x3f (objects+84),lookup 0x40 (objects+112),
        planeOperation (trimPlane (firstPixels+4500)) (widePlane destinationPixels 108 64) [Patch (Address destinationPixels) (B.replicate 8 32)],
        planeOperation (trimPlane (firstPixels+4096)) (widePlane referencePixels 108 64) [Patch (Address referencePixels) (B.replicate 8 32)]])]
  resizeCases =
    [active 0x62 ("resize-pair-"++show selected++"-"++show shape)
      ([wordsAt objects (plane firstPixels 32 128),wordsAt (objects+28) (plane destinationPixels 32 128),wordsAt (frames+0x48) [10,50],Patch (Address firstPixels) (B.replicate 8192 32)]++
       if selected then [Patch (Address (frames+0xa0)) (B.singleton 16),wordsAt (frames+0xa8) [12,52]] else [])
      ([wordsAt (state+0x184) [1,destinationPixels+528,16*outputHeight,64,16,outputHeight,0],word 0x15b0cd4 (if shape == PadOdd then 1 else 0),Patch (Address (destinationPixels+528)) (B.replicate 16 32)]++
       [Patch (Address (firstPixels+start+delta+row*64)) (B.concat (replicate 10 (wordBytes 0x80800909))) | (start,rows) <- fillRegions,row <- [0..rows-1]]) 1
      ([lookup 0x36 objects,lookup 0x37 (objects+28)]++(if selected then replicate 3 (cfg 0x9f66 2) else [cfg 0x9f66 0])++cfg16 0x9f1a 0++
       [half 0x9f12 32,half 0x9f62 outputHeight,half 0x9f5c 8,half 0x9f5c 8,half 0x9f62 outputHeight]++
       (if padding then [cfg 0x9fac 9,cfg 0x9fac 9,call 0x723a14 [11,12] 0,call 0x723a14 [11,0] 0,call 0x723a14 [11,12] 0,call 0x723a14 [11,0] 0] else [])++
       [resizeOperation [1,firstPixels+sourceOffset,8*targetHeight,64,8,targetHeight,0] [1,destinationPixels+528,16*outputHeight,64,16,outputHeight,0] [Patch (Address (destinationPixels+528)) (B.replicate 16 32)]])
    | shape <- [CropToFit,PadEven,PadOdd],selected <- [False,True],let padding = shape /= CropToFit,let delta = if selected then 128 else 0
    , let (outputHeight,targetHeight,sourceBase,fillRegions) = case shape of
            CropToFit -> (16,8,2512,[])
            PadEven -> (128,64,720,[(576,5),(4608,5)])
            PadOdd -> (126,63,784,[(640,4),(4608,5)])
    , let sourceOffset = delta+sourceBase]
  extractionCases =
    [active 0x5a "extract-invalid-output" [wordsAt objects (plane destinationPixels 32 8),wordsAt (objects+28) (plane 0 32 8)]
      [Patch (Address 0x1571534) (B.replicate 0x15c 0)] 0
      [lookup 0x3a objects,lookup 0x2c (objects+28),Call (Address 0x7221c8) [(N.R0,0x1571534),(N.R1,0),(N.R2,0x15c)] [(N.R0,0)] [Patch (Address 0x1571534) (B.replicate 0x15c 0)]]]
