{-# LANGUAGE GHC2021 #-}
module ProcessingScenarios (processingPublications, processingHelperPublications, processingScenarios) where

import Data.Bits ((.|.), shiftL)
import qualified Data.ByteString as B
import qualified BuffersScenarios as Buffers
import qualified ImagePlanesScenarios as ImagePlanes
import qualified DiagnosticLoggingScenarios as Diagnostics
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios

processingPublications :: [StagePublication]
processingPublications = [StagePublication (Address address) (StageSymbol ("xm1_develop_pipeline_" ++ name)) | (address,name) <-
  [(0xe989d0,"merge_references"),(0xe98ae4,"append_reference"),(0xe98c7c,"filter_reference"),(0xe98d90,"align_reference"),(0xe98ef4,"apply_alignment"),(0xe99030,"fuse_pyramids"),(0xe99228,"filter_pyramid"),(0xe993bc,"prepare_pyramid"),(0xe99504,"merge_pyramid"),(0xe99718,"resolve_references"),(0xe9983c,"build_pyramid"),(0xe999e4,"combine_planes"),(0xe99b04,"configure_references"),(0xe99b94,"convert_reference"),(0xe99f68,"finish_output"),(0xe9a070,"resize_output"),(0xe9a36c,"copy_output"),(0xe9a634,"prepare_output"),(0xe9a990,"apply_output"),(0xe9aa34,"apply_output_planes"),(0xe9aad8,"prepare_output_planes"),(0xe9ac44,"resize_preview"),(0xe9adb8,"prepare_preview"),(0xe9afc8,"convert_output"),(0xe9b368,"process_capture"),(0xe9b88c,"prepare_capture"),(0xe9bb04,"filter_capture")]]

processingHelperPublications :: [StagePublication]
processingHelperPublications = [StagePublication (Address address) (StageSymbol name) | (address,name) <- [(0xe51858,"xm1_develop_capture_option_selection"),(0x73abec,"xm1_develop_loadtest_current"),(0x73ac60,"xm1_develop_loadtest_setting")]]

newtype BodyIndex = BodyIndex Int
newtype Operation = Operation Word32

data Bypass = Bypass BodyIndex Operation [Word32] [Patch] [Call] [Patch] [Call]

auxiliary, objects, configuration, softwareOutput :: Word32
auxiliary = 0x9b4000
objects = 0x9b8000
configuration = 0x9e0000
softwareOutput = 0xa00000

sourcePlane, stalePlane :: [Word32]
sourcePlane = [1,0x9c2000,8192,128,128,64,0x100000]
stalePlane = [101,0x9c6000,4096,128,64,32,0x81234567]
objectAddress :: Word32 -> Word32
objectAddress group = objects + group*32
objectPlane :: Word32 -> [Word32]
objectPlane group = [1,0x9c8000+group*512,8192,128,128,64,0x100000]
lookupObject :: Word32 -> Word32 -> Call
lookupObject group kind = call 0xe44dbc [state+0x7c,group,kind] (objectAddress group)
planeBytes :: [Word32] -> B.ByteString
planeBytes = B.concat . map wordBytes

objectBindings :: [(Word32,Word32,Word32,Buffers.Plane)]
objectBindings = [(group,kind,objectAddress group,toPlane (objectPlane group)) | (group,kind) <- [(group,0) | group <- [0x14..0x56]]++[(0x25,5),(0x29,1),(0x50,9)]]

toPlane :: [Word32] -> Buffers.Plane
toPlane [format,address,byteLength,stride,width,height,flags] = Buffers.Plane format address byteLength stride width height flags
toPlane _ = error "processing plane fixture must contain seven words"

objectRecords :: [Buffers.ObjectRecord]
objectRecords = Buffers.objectLookupRecords 0x45 objectBindings

processingEndpointCalls :: [Call] -> [Call]
processingEndpointCalls = concatMap endpoint
 where
  endpoint request
    | callAddress request `elem` [Address 0xe44dbc,Address 0xe44bcc] = Buffers.criticalCalls
    | callAddress request `elem` [Address 0x726658,Address 0xeb0cbc,Address 0xeb0ccc] = []
    | otherwise = [request]

processingScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
processingScenarios image _resolve = concatMap bypassCases bodies ++ pureCases ++ alignmentCases ++ softwareCases ++ selectionCases
 where
  publication (BodyIndex index) = processingPublications!!index
  plan (Operation operation) = case [pointer | index <- [0..126], let pointer=0xea6738+index*52, readWord image pointer == operation, readWord image (pointer+28) == 13] of
    pointer:_ -> [readWord image (pointer+offset*4) | offset <- [0..12]]
    [] -> error "processing operation has no pinned plan"
  common = base ++ [word 0x96ca50 configuration,word 0x9134f8 0,word (configuration+0x80) 0,word (configuration+0xa8) 0,word (configuration+0x493f) 2,word (state+0x3098) auxiliary,word (auxiliary+0xcc) 2,word (auxiliary+0x54) 0x9c9000,word (state+0x2fbc) 1,word (state+0x2fe8) 2,word (state+0x644) 4,word (state+0x4f0) 0,word (state+0x275c) 1,word (state+0x2764) 1,word (state+0x578) 2,word 0x15b4f7c 0,word (0x816f60+4*16) 64,word (0x8163a8+4*16) 42,word (0x816f60+4*16+12) 64,word (0x8163a8+4*16+12) 64] ++ [wordsAt (state+offset) sourcePlane | offset <- [0x88,0xa4,0xc0,0xdc,0x184,0x1a0,0x1bc,0x1d8,0x1f4,0x210,0x22c,0x248,0x264,0x280,0x3d0]] ++ [wordsAt (auxiliary+offset) sourcePlane | offset <- [0,0x38,0x5c,0x78,0x94,0xb0,0xd0]] ++ [wordsAt (objectAddress group) (objectPlane group) | group <- [0x14..0x56]] ++ [wordsAt (stackTop-0x50) stalePlane]
  make name index arguments initial final result calls = StageScenario (StageCaseName name) (publication index) (context:arguments) (common++Buffers.objectLookupInitial (state+0x7c) 0x45 0 objectBindings++Diagnostics.traceDisabledInitial++Diagnostics.textDisabledInitial++initial) final (Just result) [] (processingEndpointCalls calls)
  framePlane offset group = wordsAt (state+offset) (objectPlane group)
  auxPlane offset group = wordsAt (auxiliary+offset) (objectPlane group)
  snapshot = ScriptCall (Address 0x7101e4) [(N.R0,1)] [(N.R0,0)] [] [Relative N.R1 0 (wordBytes 0)]
  preview group width height = [1,objectPlane group!!1,width*height,width,width,height,0x100000]
  bypassCases (Bypass index operation arguments initial middle final after) =
    let descriptor=plan operation
        BodyIndex ordinal=index
        finalState=word (state+0x38) (descriptor!!4)
        Operation request=operation
        refusalTable=wordsAt params [request,0x101,0x100,0x80,0x101,0x100]
        prefix="processing-"++show ordinal++"-operation-"++show request++"-arguments-"++show arguments
     in [make (prefix++"-processing-disabled") index arguments (initial++[word (state+0x27cc) 1]) (finalState:final) 0 (beginCalls descriptor++middle++completeCalls descriptor 1++after)
        ,make (prefix++"-transition-refused") index arguments (initial++[word (context+0x44) 0,word 0xea254c params,refusalTable]) [] 5 (take 3 (beginCalls descriptor))]
  bypass index operation arguments initial middle final after = Bypass (BodyIndex index) (Operation operation) arguments initial middle final after
  bodies =
    [bypass 0 0x47 [] [] [lookupObject 0x50 9] [auxPlane 0x78 0x50] []
    ,bypass 1 0x46 [] [] [lookupObject 0x4f 0,lookupObject 0x50 9,lookupObject 0x46 0] [auxPlane 0x5c 0x46,auxPlane 0xb0 0x50,word (auxiliary+0xcc) 3] []
    ,bypass 2 0x45 [] [] [lookupObject 0x45 0] [framePlane 0x184 0x45] []
    ,bypass 3 0x44 [] [] [lookupObject 0x45 0,lookupObject 0x4d 0] [] []
    ,bypass 4 0x43 [] [] [lookupObject 0x4b 0,lookupObject 0x4d 0] [] []
    ,bypass 5 0x42 [0] [] [lookupObject 0x4b 0] [] []
    ,bypass 6 0x41 [] [word (state+0x2fb8) 1] [lookupObject 0x4c 0] [auxPlane 0x38 0x4c] []
    ,bypass 7 0x40 [] [word (state+0x2fb8) 1] [lookupObject 0x4a 0] [word (auxiliary+0x54) (objectPlane 0x4a!!1)] []
    ,bypass 8 0x3f [] [] [lookupObject 0x30 0,lookupObject 0x39 0] [framePlane 0x184 0x39,word (auxiliary+0xcc) 3] []
    ,bypass 9 0x3e [] [] [lookupObject 0x3d 0] [framePlane 0x184 0x3d] []
    ,bypass 10 0x3d [] [] [] [] []
    ,bypass 11 0x3c [] [] [lookupObject 0x3a 0,lookupObject 0x3b 0] [framePlane 0x184 0x3b] []
    ,bypass 12 0x3b [] [] [lookupObject 0x30 0] [] []
    ,bypass 13 0x39 [2] [] [lookupObject 0x35 0] [framePlane 0x184 0x35] []
    ,bypass 13 0x38 [0] [] [lookupObject 0x38 0] [auxPlane 0x5c 0x38,framePlane 0x1a0 0x38] []
    ,bypass 14 0x30 [1] [word (state+0x4b4) 0x19] [] [] []
    ,bypass 15 0x23 [0,0] [] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 16 0x21 [0,0] [word (0x8163a8+4*16) 64] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 17 0x1c [1,0] [word (state+0x4b4) 0x19,word (state+0x4f0) 3] [lookupObject 0x25 5,lookupObject 0x20 0] [auxPlane 0xd0 0x25,framePlane 0x3d0 0x20] []
    ,bypass 18 0x1b [] [] [] [] []
    ,bypass 19 0x1a [] [] [] [] []
    ,bypass 20 0x19 [] [] [lookupObject group 0 | group <- [0x51..0x55]] [framePlane offset group | (offset,group) <- zip [0x210,0x22c..0x280] [0x51..0x55]] []
    ,bypass 21 0x1e [] [word (state+0x4a8) 4] [lookupObject 0x1e 0] [wordsAt (state+0x3ec) (preview 0x1e 544 204),wordsAt (state+0x408) stalePlane] []
    ,bypass 22 0x18 [] [] [lookupObject 0x1c 0] [wordsAt (state+0x1bc) (preview 0x1c 128 42),wordsAt (state+0x3d0) (preview 0x1c 128 42)] []
    ,bypass 23 0x17 [0] [word (state+0x4b4) 0x20] [lookupObject 0x1a 0] [framePlane 0x184 0x1a] []
    ,bypass 24 0x12 [1] (Buffers.objectStoreEffects 0x45 0x19 0 (Buffers.Plane 7 0x9ca000 17 8 4 2 0x100000) objectRecords) [lookupObject 0x28 0,lookupObject 0x19 0] ([framePlane 0x184 0x19]++Buffers.objectStoreEffects 0x45 0x19 0 (toPlane (objectPlane 0x19)) objectRecords) [ScriptCall (Address 0xe44bcc) [(N.R0,state+0x7c),(N.R1,0x19),(N.R2,0)] [(N.R0,0)] [Relative N.R3 0 (planeBytes (objectPlane 0x19))] []]
    ,bypass 25 0x10 [0,9] [word (state+0x578) 0] [lookupObject 0x29 1,lookupObject 0x28 0] [] []
    ,bypass 14 0x2e [2] [] [] [] []
    ,bypass 14 0x2f [4] [word (state+0x4b4) 0x1e,word (state+0x322c) 2] [] [] []
    ,bypass 14 0x31 [0] [] [] [] []
    ,bypass 15 0x22 [0,1] [word (state+0x2754) 1] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 15 0x24 [0,2] [word (0x816f60+33*16+4) 64,word (0x8163a8+33*16+4) 42] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 15 0x25 [1,1] [word (state+0x2754) 1] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 15 0x26 [1,0] [] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 15 0x27 [1,2] [] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 16 0x1f [0,2] [word (0x816f60+33*16+4) 64,word (0x8163a8+33*16+4) 64] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 16 0x20 [0,1] [word (state+0x2754) 1,word (0x8163a8+4*16) 64] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 16 0x28 [2,1] [word (state+0x2754) 1,wordsAt (state+0x1d8) [1,0x9c2000,153600,640,640,240,0x100000]] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 16 0x29 [2,0] [wordsAt (state+0x1d8) [1,0x9c2000,153600,640,640,240,0x100000]] [lookupObject 0x22 0,snapshot] [] []
    ,bypass 17 0x1d [0,0] [word (state+0x4b4) 0x19,word (state+0x4f0) 3] [lookupObject 0x25 6,lookupObject 0x20 0] [framePlane 0x1d8 0x25,framePlane 0x3d0 0x20] []
    ,bypass 17 0x64 [0,1] [word (state+0x4b4) 0x19,word (state+0x4f0) 3] [lookupObject 0x1b 0,lookupObject 0x3c 0] [framePlane 0x1d8 0x1b,framePlane 0x3d0 0x3c] []
    ,bypass 23 0x16 [1] [word (state+0x4b4) 0x20] [lookupObject 0x1b 0] [framePlane 0x184 0x1b] []
    ,bypass 24 0x12 [0] [] [lookupObject 0x28 0,lookupObject 0x19 0] [framePlane 0x184 0x19,framePlane 0x1f4 0x19] [ScriptCall (Address 0xe44bcc) [(N.R0,state+0x7c),(N.R1,0x19),(N.R2,0)] [(N.R0,0)] [Relative N.R3 0 (planeBytes (objectPlane 0x19))] [],lookupObject 0x19 0]
    ,bypass 24 0x3a [2] [] [lookupObject 0x28 0,lookupObject 0x19 0] [framePlane 0x184 0x19] [ScriptCall (Address 0xe44bcc) [(N.R0,state+0x7c),(N.R1,0x19),(N.R2,0)] [(N.R0,0)] [Relative N.R3 0 (planeBytes (objectPlane 0x19))] []]
    ,bypass 25 0x10 [0,0] [word (state+0x578) 0] [lookupObject 0x29 1,lookupObject 0x28 0] [] []
    ,bypass 25 0x11 [1,0] [word (state+0x578) 0] [lookupObject 0x29 2,lookupObject 0x28 0] [] []
    ]
  pureCases =
    [make ("processing-capture-source-gate-"++show selection) (BodyIndex 26) [selection] [] [word (state+0x38) ((plan (Operation (0xe+selection)))!!4)] 0 [] | selection <- [0,1]] ++
    [make ("processing-invalid-selection-"++show index) (BodyIndex index) [0xffffffff] [] [] 0 [] | index <- [25,26]] ++
    [let descriptor=plan (Operation operation) in make ("processing-disabled-"++show index) (BodyIndex index) [] initial [word (state+0x38) (descriptor!!4)] 0 [] | (index,operation,initial) <- [(1,0x46,[word (state+0x2fb8) 1]),(2,0x45,[word (state+0x2fbc) 0]),(3,0x44,[word (state+0x2fbc) 0]),(4,0x43,[word (state+0x2fb8) 1]),(7,0x40,[word (state+0x2fb8) 0]),(18,0x1b,[word (state+0x275c) 0]),(21,0x1e,[word (state+0x4a8) 7]),(22,0x18,[word (state+0x578) 0])]] ++
    [let descriptor=plan (Operation 0x1c) in make "processing-special-setting-skip-callback" (BodyIndex 17) [1,0] [word (configuration+0x80) 0xa5] [word (state+0x38) (descriptor!!4)] 0 [callbackCall (descriptor!!2) 0]] ++
    [let descriptor=plan (Operation 0x47) in make "processing-single-reference" (BodyIndex 0) [] [word (auxiliary+0xcc) 1] [word (state+0x38) (descriptor!!4),wordsAt (auxiliary+0x78) sourcePlane] 0 (beginCalls descriptor++[lookupObject 0x50 9]++completeCalls descriptor 1)] ++
    [let descriptor=plan (Operation 0x30) in make "processing-finish-output-option-disabled" (BodyIndex 14) [1] [word (state+0x4b4) 0x19,word 0x9134f8 1,word (configuration+0x80) 0xa5] [word (state+0x38) (descriptor!!4)] 0 (beginCalls descriptor++completeCalls descriptor 1)] ++
    [let descriptor=plan (Operation 0x3b)
         undersized=[1,0x9c8000,1023,128,128,64,0x100000]
      in make "processing-reference-curve-insufficient-capacity" (BodyIndex 12) [] [wordsAt (objectAddress 0x30) undersized,Patch (Address 0x9c8000) (B.replicate 1023 0xa5)] [word (state+0x38) (descriptor!!3),wordsAt (objectAddress 0x30) undersized,Patch (Address 0x9c8000) (B.replicate 1023 0xa5)] 0 (beginCalls descriptor++[lookupObject 0x30 0]++completeCalls descriptor 0)] ++
    [let descriptor=plan (Operation 0x40)
      in make "processing-primary-motion-unsupported-capture-kind" (BodyIndex 7) [] [word (state+0x2fb8) 1,word (state+0x4b4) 0,Patch (Address 0x1571534) (B.replicate 0x15c 0xa5)] [word (state+0x38) (descriptor!!3),word (auxiliary+0x54) (objectPlane 0x4a!!1),Patch (Address 0x1571534) (B.replicate 0x15c 0)] 0 (beginCalls descriptor++[lookupObject 0x4a 0]++completeCalls descriptor 0)]
  alignmentCases =
    [let descriptor=plan (Operation 0x44)
         source=[1,0x9c3000,512,32,16,16,0x100000]
         destination=[1,if overlaps then 0x9c3000-9*32 else 0x9c2000,512,32,16,16,0x100000]
         pixels=B.pack (take 512 (cycle [0x29,0xa7,0x63,0xd1]))
      in make ("processing-alignment-"++show (overlaps,scale)) (BodyIndex 3) []
           [word (state+0x4b4) 0x1a,word (state+0x2fe4) 1,word (state+0x2fe8) 1,word (state+0x220c) 0x9f0000,wordsAt (state+0x2fcc) [65536,0,0,scale,0,0],wordsAt (state+0x184) source,wordsAt (objectAddress 0x45) destination,Patch (Address 0x9c3000) pixels,word (state+0x2fc0) 0xdeadbeef,word (0x9f0000+0xdace4) 0xdeadbeef]
           [word (state+0x38) (descriptor!!4),word (state+0x2fc0) bits,word (0x9f0000+0xdace4) bits,word (state+0x2fbc) valid,word (auxiliary+0xcc) (if valid == 1 then 3 else 2),Patch (Address 0x9c3000) pixels]
           0 (beginCalls descriptor++[lookupObject 0x45 0,lookupObject 0x4d 0]++completeCalls descriptor 1)
    | (overlaps,scale,bits,valid) <- [(False,65536,7,1),(True,65536,3,0),(False,131071,5,0)]]
  softwareCases =
    [let descriptor=plan (Operation 0x1e)
         (width,height)=Buffers.bufferDimensions 3 0
         destination=[1,softwareOutput,width*2*height,width*2,width*2,height,0x100000]
         packed left right = 0x80600000 .|. left .|. (right `shiftL` 8)
         sourcePixels=concat [replicate 16 (packed (32+row) (32+row))++replicate 16 (packed (128+row) (128+row)) | row <- [0..63]]
         expectedPixels=concat [let sourceRow=row*16 `div` 51 in replicate 68 (packed (32+sourceRow) (32+sourceRow))++replicate 68 (packed (128+sourceRow) (128+sourceRow)) | row <- [0..203]]
         cacheRead=call 0x42634 [sourcePlane!!1,8192] 0
         cacheWrite=call 0x4263c [softwareOutput,110976] 0
         middle=[lookupObject 0x1e 0]++ImagePlanes.currentTaskCalls 11++ImagePlanes.currentTaskCalls 11++[call 0x726658 [11,0xe54348] 0]++ImagePlanes.priorityCalls 11 12++[cacheRead,cacheWrite]++ImagePlanes.priorityCalls 11 6++ImagePlanes.currentTaskCalls 11++[call 0x726658 [11,0xe5435c] 0]
      in make "processing-software-preview-striped-pixels" (BodyIndex 21) [] (ImagePlanes.priorityInitial 11++[word (state+0x4a8) 4,wordsAt (objectAddress 0x1e) destination,wordsAt (sourcePlane!!1) sourcePixels,Patch (Address softwareOutput) (B.replicate 110976 0xa5)]) [word (state+0x38) (descriptor!!4),wordsAt (state+0x3ec) destination,wordsAt (state+0x408) stalePlane,wordsAt softwareOutput expectedPixels] 0 (beginCalls descriptor++middle++completeCalls descriptor 1)]
  selectionCases =
    [StageScenario (StageCaseName ("processing-loadtest-current-"++show value)) (processingHelperPublications!!1) [] [word 0x9134f8 value] [] (Just value) [] [] | value <- [0,1,0x80000001,0xffffffff]] ++
    [StageScenario (StageCaseName ("processing-loadtest-setting-"++show value)) (processingHelperPublications!!2) [] [word 0x96ca50 configuration,word (configuration+0x80) value] [] (Just value) [] [] | value <- [0,0xa5,0xa7,0xa9,255]] ++
    [StageScenario (StageCaseName ("processing-option-"++show value)) (processingHelperPublications!!0) [] [word 0x96ca50 configuration,word (configuration+0x80) value,word 0x9134f8 1] [] (Just selected) [] [] | (value,selected) <- [(0xa5,1),(0xa7,2),(0xa9,3),(0,0),(255,0)]] ++
    [StageScenario (StageCaseName "processing-option-disabled") (processingHelperPublications!!0) [] [word 0x9134f8 0] [] (Just 0) [] []]
