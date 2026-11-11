module TransformsScenarios (transformsPublications, transformsScenarios) where

import qualified Data.ByteString as B
import Data.Bits (xor, shiftL, (.|.))
import qualified BuffersScenarios as Buffers
import qualified ImagePlanesScenarios as ImagePlanes
import qualified ImageOperationsScenarios as ImageOperations
import qualified CropProcessScenarios as Crop
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import PipelineModel
import Scenarios (Address (..), Call (..), MemoryContract (..), Patch (..), base, beginCalls, call, completeCalls, context, readWord, state, word, wordBytes, wordsAt)

transformsPublications :: [StagePublication]
transformsPublications = [StagePublication (Address address) (StageSymbol ("xm1_develop_pipeline_" ++ name)) | (address,name) <- entries]

entries :: [(Word32,String)]
entries = [(0xe96978,"finish_planes"),(0xe96a74,"blend_planes"),(0xe96bf0,"difference_planes"),(0xe96d28,"merge_planes"),(0xe96e9c,"prepare_planes"),(0xe96fe4,"select_composite"),(0xe972f4,"prepare_composite"),(0xe973f4,"split_composite"),(0xe97558,"fill_chroma"),(0xe97690,"blend_captures"),(0xe977c0,"crop_capture"),(0xe97a88,"split_capture"),(0xe97e94,"rotate_capture"),(0xe9837c,"reduce_preview"),(0xe986a4,"transform_border"),(0xe98808,"reduce_border"),(0xe988dc,"combine_border")]

image, changed :: [Word32]
image = [2,0x009c1000,0x480,0x80,0x60,12,0x123400]
changed = [2,0x009c2800,0x880,0x88,0x70,16,0x2000000]
objectAddress, previous, saved, marker :: Word32
objectAddress = 0x009a2000
previous = 0x009c8000
saved = 0x009c9000
marker = 0x009cfe00

transformsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
transformsScenarios bytes resolve = concatMap (\pub -> zipWith number [0 :: Int ..] (cases pub ++ edgeCases pub)) transformsPublications
 where
  number index scenario = scenario { stageCaseName = StageCaseName (show (stageCaseName scenario) ++ "-" ++ show index) }
  edgeCases pub =
    let address = addressWord (stageStockEntry pub)
        operation = case address of
          0xe96978 -> 0x6f; 0xe96a74 -> 0x6e; 0xe96bf0 -> 0x6d; 0xe96d28 -> 0x6c; 0xe96e9c -> 0x6b
          0xe96fe4 -> 0x6a; 0xe972f4 -> 0x69; 0xe973f4 -> 0x68; 0xe97558 -> 0x66; 0xe97690 -> 0x65
          0xe977c0 -> 0x54; 0xe97a88 -> 0x63; 0xe97e94 -> 0x53; 0xe9837c -> 0x50; 0xe986a4 -> 0x4a; 0xe98808 -> 0x49; _ -> 0x48
        args = if address `elem` [0xe97558,0xe97e94,0xe9837c,0xe986a4] then [0] else []
        inactive = case address of
          0xe97690 -> [word (state+0x4ac) 0]
          0xe977c0 -> [word (state+0x2794) 0]
          0xe97a88 -> [word (state+0x5cc) 0]
          _ -> [word (state+0x530) 0]
        guarded = address `elem` [0xe96978,0xe96a74,0xe96bf0,0xe96d28,0xe96e9c,0xe96fe4,0xe972f4,0xe973f4,0xe97558,0xe97690,0xe977c0,0xe97a88]
        skip = StageScenario (StageCaseName "inactive") pub (context:args) (common++inactive) [word (state+0x38) (plan operation!!4)] (Just 0) [] []
        invalid = StageScenario (StageCaseName "invalid-pass") pub [context,0xffffffff] common [word (state+0x38) 0] (Just 3) [] []
        primary = head (cases pub)
        transitions = [primary { stageCaseName = StageCaseName "begin-transition", stageInitial = stageInitial primary ++ [word (context+0x44) mode], stageFinal = [], stageReturn = Just 5, stageCodePointers = [], stageCalls = take 3 (beginCalls (plan operation)) } | mode <- [0..2], let table = readWord bytes (0xea2548+mode*8+4), table /= 0, operation `elem` tableOperations table 0]
     in (if guarded then [skip] else []) ++ (if address `elem` [0xe97e94,0xe9837c] then [invalid] else []) ++ transitions
  tableOperations table index
    | index > 256 = error "unterminated transition table"
    | readWord bytes (table+index*12) == 0x80 = []
    | otherwise = readWord bytes (table+index*12) : tableOperations table (index+1)
  plan operation = head [[readWord bytes (0xea6738+i*52+j*4) | j <- [0..12]] | i <- [0..126], readWord bytes (0xea6738+i*52) == operation, readWord bytes (0xea6738+i*52+28) == 13]
  common = base ++ [word (context+20) previous,word (context+24) 0x1234,word (state+0x3098) saved,word (state+0x3228) (saved+0x200),word (state+0x4ac) 1,word (state+0x2794) 1,word (state+0x3218) 1,word (state+0x5cc) 4,word (state+0x1604) 3,word (state+0x5d0) 3,word (state+0x1680) 1,word (saved+0xcc) 2,word (saved+0x310) 0,word (saved+0x314) 0,word (saved+0x30c) 0,wordsAt saved image,wordsAt objectAddress image,word marker 0] ++ [wordsAt (state+offset) image | offset <- [0x184,0x1a0,0x210,0x22c,0x248,0x264,0x280,0x2b8,0x2f0,0x30bc]] ++ [wordsAt (0x15b0cd8+i*28) image | i <- [0..5]] ++ [wordsAt (previous+0x184) image]
  make pub label operation args initial final status calls =
    let p = plan operation
     in StageScenario (StageCaseName (label ++ "-" ++ show (addressWord (stageStockEntry pub)))) pub (context:args) (common++initial) (word (state+0x38) (if status == 1 then p!!4 else p!!3):final) (Just 0) [] (beginCalls p ++ calls ++ completeCalls p status)
  obj group kind = call 0xe44dbc [state+0x7c,group,kind] objectAddress
  debug kind offset number = call 0xe53ed4 [state,kind,state+offset,number] 0
  cfg key value = call 0x69f8fc [key] value
  pair = [cfg 0x9f13 0,cfg 0x9f12 32]
  service address args status inputs outputs = ScriptCall (Address address) (zip [N.R0,N.R1,N.R2,N.R3] args) [(N.R0,status)] inputs outputs
  effect = Absolute (word marker 0x10203040)
  dataAt register values = Relative register 0 (B.concat (map wordBytes values))
  imageCall address args register status = service address args status [dataAt register image] [dataAt register changed,effect]
  active pub operation args initial final status calls = make pub "active" operation args initial (word marker 0x10203040:final) status calls
  cases pub = case addressWord (stageStockEntry pub) of
    0xe96978 -> concat [
      [active pub 0x6f [] [word (state+0x530) 5] [wordsAt (state+0x210) changed] status
        ([imageCall 0xe60e5c [state,state+0x210,state+0x280,state+0x210] N.R1 status,debug 0x32 0x210 0] ++ if status == 1 then [imageCall 0xe60f34 [state,state+0x22c,state+0x248,state+0x22c] N.R1 1,debug 0x31 0x22c 0] else []) | status <- [0,1]],
      [bypass pub 0x6f [] [word (state+0x530) 5] [] []]]
    0xe96a74 -> [active pub 0x6e [] [word (state+0x530) 5] [] status
      ([obj 0x31 0,imageCall 0xe60be8 [state,state+0x248,state+0x22c,state+0x280] N.R2 status,debug 0x2e 0x264 0] ++ if status == 1 then [imageCall 0xe60cc4 [state,state+0x22c,state+0x280,state+0x22c] N.R1 1,debug 0x2f 0x22c 0,service 0xe60d9c [state,state+0x22c,state+0x264,state+0x248] 1 [] [effect],debug 0x30 0x22c 0] else []) | status <- [0,1]] ++ [bypass pub 0x6e [] [word (state+0x530) 5] [wordsAt (state+0x264) image] [obj 0x31 0]]
    0xe96bf0 -> [active pub 0x6d [] [word (state+0x530) 5] [] status
      ([obj 0x30 0,imageCall 0xe60a2c [state,state+0x210,state+0x22c,state+0x248] N.R1 status,debug 0x2d 0x248 1] ++ if status == 1 then [service 0xe60a2c [state,state+0x22c,state+0x210,state+0x22c] 1 [] [effect],debug 0x2d 0x22c 2] else []) | status <- [0,1]] ++ [bypass pub 0x6d [] [word (state+0x530) 5] [] [obj 0x30 0]]
    0xe96d28 -> [active pub 0x6c [] [word (state+0x530) 5] [] status
      ([obj 0x2f 0,service 0xe60948 [state] status [dataAt N.R3 image] [dataAt N.R3 changed,effect],debug 0x2c 0x22c 0] ++ if status == 1 then [obj 0x34 0,service 0xe605b0 [state,state+0x22c,state+0x2f0] 1 [] [effect],debug 0x2b 0x2f0 0] else []) | status <- [0,1]] ++ [bypass pub 0x6c [] [word (state+0x530) 5] [] [obj 0x2f 0]]
    0xe96e9c -> [active pub 0x6b [] [word (state+0x530) 5] [] status [obj 0x33 0,service 0xe63544 [state,state+0x2b8] 0 [] [effect],obj 0x31 0,obj 0x35 0,service 0xe63b0c [state,state+0x2b8] status [dataAt N.R2 image] [dataAt N.R2 changed,effect]] | status <- [0,1]] ++ [bypass pub 0x6b [] [word (state+0x530) 5] [wordsAt (state+0x210) image] [obj 0x33 0]]
    0xe96fe4 -> [active pub 0x6a [] [word (state+0x530) mode] [wordsAt (state+0x184) changed] status (selectCalls mode status ++ [debug 0x18 0x184 0]) | mode <- [3,4,5,13],status <- [0,1]] ++ [bypass pub 0x6a [] [word (state+0x530) 5] [] [debug 0x18 0x184 0]]
    0xe972f4 -> [active pub 0x69 [] [word (state+0x530) mode] [] 1 [obj 0x2c 0,service (if mode == 3 then 0xe61fc0 else 0xe62330) [state,state+0x248] 0 [] [effect],debug 0x2b 0x248 0] | mode <- [3,4]] ++ [bypass pub 0x69 [] [word (state+0x530) 3] [] [obj 0x2c 0,debug 0x2b 0x248 0]]
    0xe973f4 -> [active pub 0x68 [] [word (state+0x530) mode] [wordsAt 0x15b0cd8 (concatMap Buffers.planeWords (Buffers.splitPlanes (if mode == 5 then 4 else 6) sourcePlane sourcePlane))] 1 [obj 0x36 0,service target [state] 1 [] [effect]] | (mode,target) <- [(4,0xe5ff9c),(5,0xe6043c),(13,0xe601bc)]]
    0xe97558 -> [chromaCase pub mode pass operation first second | (mode,pass,operation,first,second) <- [(3,1,0x66,0,128),(5,1,0x66,255,0),(5,2,0x67,0,255),(5,9,0x66,255,0)]] ++ [bypass pub 0x66 [1] [word (state+0x530) 5] [] []]
    0xe97690 -> [active pub 0x65 [] [] [wordsAt (state+0x184) changed] status [obj 0x1b 0,service 0xe5c9bc [state] status [dataAt N.R3 image] [dataAt N.R3 changed,effect]] | status <- [0,1]] ++ [bypass pub 0x65 [] [] [wordsAt (state+0x184) image] [obj 0x1b 0]]
    0xe977c0 -> [active pub 0x54 [] [] [] status [obj 0x28 0,service 0xe5bdcc [state] status [dataAt N.R1 [2,0x9c1000,0x600,0x80,0x60,16,0x123400],dataAt N.R2 [2,0x9c1400,0x600,0x80,0x60,16,0x123400]] [effect]] | status <- [0,1]]
    0xe97a88 -> [make pub "active-real-rotation" 0x63 [] rotationInitial (wordsAt (state+0x184) [2,0x9c4410,0x480,0x80,0x60,12,0x123400] : rotationWrites 0 ++ rotationWrites 1) 1 ([obj 0x38 0] ++ rotationCalls ++ [obj 0x38 0] ++ rotationCalls)]
    0xe97e94 -> [make pub "active-real-rotation" 0x53 [1] rotationInitial (wordsAt (state+0x184) [2,0x9c4410,0x480,0x80,0x60,12,0x123400] : wholeRotationWrites) 1 ([obj 0x29 0] ++ rotationCalls)] ++ [rotatePrimaryCase pub completion | completion <- [Crop.Completed,Crop.Failed]]
    0xe9837c -> [active pub 0x50 [0] [] [wordsAt (state+0x30bc) [2,0x9c1000,0xa8c00,0x5a0,0x5a0,0x1e0,0x123400]] 1 [service 0xe4fd5c [state] 1 [] [effect]],
      active pub 0x51 [1] [] [wordsAt (state+0x30f8) [2,0x9c0f80,0x60,0x80,0x20,8,0x123400]] 1 ([obj 0x26 0,service 0xe4fd5c [state] 1 [] [effect]]++pair++pair)]
    0xe986a4 -> [active pub operation [pass] [] [wordsAt (state+offset) changed,word (state+0x1d30) 2] status [obj 0x49 kind,service 0xe58e10 [state] status [dataAt N.R1 image] [dataAt N.R1 changed,effect]] | (pass,operation,kind,offset) <- [(1,0x4a,5,0x1a0),(0,0x4b,6,0x184)],status <- [0,1]] ++ [bypass pub 0x4a [1] [] [wordsAt (state+0x1a0) image,word (state+0x1d30) 2] [obj 0x49 5]]
    0xe98808 -> [active pub 0x49 [] [] [wordsAt (state+0x184) changed] status [obj 0x47 0,imageCall 0xe58cb8 [state] N.R1 status] | status <- [0,1]] ++ [bypass pub 0x49 [] [] [] [obj 0x47 0]]
    0xe988dc -> [active pub 0x48 [] [] [wordsAt (state+0x184) changed,word (saved+0xcc) 3] status [obj 0x48 0,service 0xe5c50c [state,saved+0x5c,state+0x184,saved+0x78] status [] [Indirect N.SP 0 (B.concat (map wordBytes changed)),effect]] | status <- [0,1]] ++ [bypass pub 0x48 [] [] [wordsAt (state+0x184) image,word (saved+0xcc) 3] [obj 0x48 0]]
    _ -> error "unmapped transform publication"
  bypass pub operation args initial final calls = make pub "bypass" operation args (initial++[word (state+0x27cc) 1]) final 1 calls
  chromaCase pub mode pass operation first second =
    let plane = ImageOperations.GeometryPlane 0x9c1000 0x80 0x60 12
        packed = first .|. (first `shiftL` 8) .|. (second `shiftL` 16) .|. (second `shiftL` 24)
        completed = StagePublication (Address 0xe63e34) (StageSymbol "xm1_pipeline_image_dma_complete")
        Address callback = resolve completed
        scenario = make pub "active-real-dma-chroma" operation [pass] (ImageOperations.dmaFillInitial ++ [word (state+0x530) mode,Patch (Address 0x9c1000) (B.replicate 0x1000 0x55)]) (ImageOperations.dmaFillEffects plane packed ++ [wordsAt 0x4157a0e0 (replicate 8 packed),word 0x157bd0c 0]) 1 (ImageOperations.dmaFillCalls plane packed callback)
     in scenario { stageCodePointers = [(Address 0x157ba00,completed)] }
  rotatePrimaryCase pub completion =
    let destination = [2,0x9c4000,0x480,0x80,0x60,12,0x123400]
        (initial,final,calls,pointers) = Crop.copyProcessLaw bytes resolve state 2 (Crop.cropPlaneFromWords image) (Crop.cropPlaneFromWords destination) completion
        status = if completion == Crop.Completed then 1 else 0
        scenario = make pub "active-real-primary-rotation" 0x52 [0] (initial ++ [word (state+0x5d0) 2,wordsAt objectAddress destination,Patch (Address 0x9c1000) (B.pack [fromIntegral (n*19+3) | n <- [0..4095 :: Int]]),Patch (Address 0x9c4000) (B.replicate 4096 0xa5)]) (final ++ [word 0x1570680 2,wordsAt (state+0x184) destination,wordsAt (state+0x30dc) [2,0x9c3e00,0x480,0x80,0x60,32,0x123400]]) status ([obj 0x25 0] ++ calls ++ pair ++ pair ++ [cfg 0x9f6f 0])
     in scenario { stageCodePointers = pointers }
  sourcePlane = Buffers.Plane 2 0x9c1000 0x480 0x80 0x60 12 0x123400
  pixelValue address = address `xor` 0x00a5005a
  rotationInitial = ImagePlanes.priorityInitial 0x123 ++ [word 0x40a5b5ec 0,wordsAt objectAddress [2,0x9c4000,0x480,0x80,0x60,12,0x123400],wordsAt 0x9c1000 [pixelValue address | address <- [0x9c1000,0x9c1004..0x9c1ffc]]]
  rotationCalls = ImagePlanes.currentTaskCalls 0x123 ++ [call 0x42634 [0,0] 0] ++ ImagePlanes.priorityCalls 0x123 13 ++ ImagePlanes.priorityCalls 0x123 6 ++ [call 0x42634 [0,0] 0]
  rotationWrites index = [word address value | (address,value) <- Buffers.rotateCopyWords (Buffers.Plane 2 (0x9c1000+index*48) 0x300 0x80 48 16 0x123400) (Buffers.Plane 2 (0x9c4210+index*48) 0x300 0x80 48 16 0x123400) pixelValue]
  wholeRotationWrites = [word address value | (address,value) <- Buffers.rotateCopyWords (Buffers.Plane 2 0x9c1000 0x600 0x80 96 16 0x123400) (Buffers.Plane 2 0x9c4210 0x600 0x80 96 16 0x123400) pixelValue]
  selectCalls mode status
    | mode == 3 = [service 0xe607e0 [state,state+0x184,state+0x22c,state+0x248] status [] [Absolute (wordsAt (state+0x22c) changed),effect]]
    | mode == 5 = [service 0xe6100c [state,state+0x22c,state+0x210,state+0x210] status [] [Absolute (wordsAt (state+0x210) changed),effect]]
    | mode == 4 = [obj 0x2d 0,service 0xe63008 [state] status [] [Absolute (wordsAt (state+0x22c) changed),effect]]
    | otherwise = [obj 0x2d 0,service 0xe62bc4 [state] status [] [Absolute (wordsAt (state+0x22c) changed),effect]]
