{-# LANGUAGE GHC2021 #-}
module ImagePixelsScenarios
  ( imagePixelsPublications
  , imagePixelsScenarios
  , imagePixelsOwnedFailureScenarios
  , arithmeticInitial
  , blendCalls
  , copyPlaneOutput
  , shearPlaneOutput
  ) where

import qualified Data.ByteString as B
import Data.Int (Int32, Int64)
import Data.Word (Word8, Word32)
import ImagePlanesScenarios (priorityInitial, priorityCalls, priorityCallsWith, currentTaskCalls, debugDisabledInitial, reservedInitial, reservedWaitCalls, reservedSignalCalls, waitCalls)
import qualified CropProcessScenarios as Crop
import qualified ImageOperationsScenarios as Operations
import qualified StripPlannerScenarios as Strip
import qualified WarpEngineScenarios as Warp
import DiagnosticLoggingScenarios (traceDisabledInitial)
import PipelineModel
import Scenarios

publication :: Word32 -> String -> StagePublication
publication entry suffix = StagePublication (Address entry) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

imagePixelsPublications :: [StagePublication]
imagePixelsPublications = [publication entry suffix | (entry,suffix) <-
  [(0xe5a9c0,"blend_pixels"),(0xe5ac04,"reduce_pair"),(0xe5adcc,"reduce_pair_alternate"),
   (0xe5afe8,"affine_coefficients"),(0xe5b2b4,"select_affine_block"),(0xe5b620,"affine_copy_safe"),
   (0xe5b978,"prepare_affine"),(0xe5baf8,"shear_plane"),(0xe5bc94,"rotate_plane"),(0xe5bdcc,"copy_plane"),
   (0xdaeba8,"filter_motion_candidates"),(0xdaf000,"select_motion_band"),(0xdaf26c,"motion_sqrt"),
   (0xdae42c,"mesh_warp"),(0xdad71c,"fit_mesh_triangle"),(0xdadb24,"mesh_circle"),
   (0xdade04,"mesh_transform_point"),(0xdaded8,"build_mesh_affines"),(0xdadfdc,"draw_mesh_labels")]]

arithmeticInitial :: B.ByteString -> B.ByteString -> [Patch]
arithmeticInitial kernel image4 =
  [slice kernel 0x40000 0x4d978 0xf4,slice kernel 0x40000 0x43604 0x1c,
   slice image4 0x61c400 0x89aa90 0x148,slice image4 0x61c400 0x89be8c 0xa8c,
   slice image4 0x61c400 0x7221a0 0x78]
 where
  slice bytes imageBase entry size
    | B.length bytes < fromIntegral (entry-imageBase+size) = error "arithmetic image extent is missing"
    | otherwise = Patch (Address entry) (B.take (fromIntegral size) (B.drop (fromIntegral (entry-imageBase)) bytes))

blendCalls :: Word32 -> [Call]
blendCalls task = currentTaskCalls task ++ priorityCalls task 12 ++ [call 0xeb0d04 [] 0,call 0xeb0d04 [] 0] ++ priorityCalls task 6

copyPlaneOutput :: [Word32] -> [Word32] -> [Word32]
copyPlaneOutput [_,_,_,_,width,height,_] [mode,address,_,stride,_,_,flags] = [mode,address,height*stride,stride,width,height,flags]
copyPlaneOutput _ _ = error "image planes contain seven words"

shearPlaneOutput :: [Word32] -> [Word32] -> [Word32]
shearPlaneOutput [_,_,_,_,width,_,_] [mode,address,size,stride,_,height,flags] = [mode,address,size,stride,width,height,flags]
shearPlaneOutput _ _ = error "image planes contain seven words"

imagePixelsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imagePixelsScenarios image resolve = blendCases ++ blendPriorityErrors ++ affineCases ++ blockCases ++ safetyCases ++ filterCases ++ bandCases ++ sqrtCases ++ candidateCases ++ meshCases ++ triangleCases ++ rotateCases ++ shearCases ++ prepareAffineCases ++ copyCases
 where
  make name entry suffix arguments initial final returned calls = StageScenario (StageCaseName name) (publication entry suffix) arguments (configuration ++ initial) final returned [] calls
  configByte :: Word32 -> Word8 -> Patch
  configByte key value = Patch (Address (0x9e0000+key)) (B.singleton (fromIntegral value))
  configHalf :: Word32 -> Word32 -> Patch
  configHalf key value = Patch (Address (0x9e0000+key)) (B.take 2 (wordBytes value))
  configuration = [word 0x96ca50 0x9e0000,configHalf 0x9f12 16,configHalf 0x9fa7 16] ++
    [configByte key value | (key,value) <- [(0x9fa5,0),(0x9fa6,64),(0x9fa9,8),(0x9faa,8),(0x9fb6,0),(0x9fb7,2),(0x9fe5,0),(0x9fe4,3)]]
  a = params+0x400
  b = params+0x800
  firstBytes = B.pack [fromIntegral ((n*17+29) `mod` 256) | n <- [0..255 :: Int]]
  secondBytes = B.pack [fromIntegral ((n*7+211) `mod` 256) | n <- [0..255 :: Int]]
  rotateCases =
    [StageScenario (StageCaseName ("image-rotate-plane-"++show (rotation,completion))) (publication 0xe5bc94 "rotate_plane")
      [state,params,params+0x40,rotation]
      (configuration ++ initial ++ [wordsAt params source,wordsAt (params+0x40) destination,
        Patch (Address a) (B.pack [fromIntegral ((n*19+3) `mod` 256) | n <- [0..1023 :: Int]]),
        Patch (Address b) (B.replicate 1280 0xa5)])
      (final ++ [word 0x1570680 format,wordsAt (params+0x40) expected])
      (Just (if completion == Crop.Completed then 1 else 0)) pointers calls
    | rotation <- [0,1,2,3,7], completion <- [Crop.Completed,Crop.Failed],
      let source = [0,a,1024,64,64,16,0x1234],
      let destination = [0,b,1280,80,80,16,0x5678],
      let format = if rotation >= 1 && rotation <= 3 then rotation else 0,
      let swapped = rotation == 1 || rotation == 3,
      let frontendDestination = if swapped then [0,b,1280,80,64,16,0x5678] else destination,
      let expected = if swapped then [0,b,1280,80,32,32,0x5678] else destination,
      let (initial,final,calls,pointers) = Crop.copyProcessLaw image resolve state format (Crop.cropPlaneFromWords source) (Crop.cropPlaneFromWords frontendDestination) completion]
  scaleSource = params+0x3000
  scaleDestination = params+0x800
  shearCases =
    [StageScenario (StageCaseName ("image-shear-plane-"++show (vertical,selector,completion))) (publication 0xe5baf8 "shear_plane")
      [state,params,params+0x40,params+0x80]
      (configuration ++ initial ++ [configByte 0x9fdf selector,word (state+0x3228) motion,
        word (motion+0xfc) 32,word (motion+0x100) (fromIntegral vertical),word (motion+0x48) 64,
        wordsAt params source,wordsAt (params+0x40) destination,word (params+0x80) 0xcccccccc,
        Patch (Address scaleSource) (B.pack [fromIntegral (n*11+7) | n <- [0..4095 :: Int]]),
        Patch (Address scaleDestination) (B.replicate 6400 0xa5)])
      (final ++ [word (params+0x80) slope,wordsAt params [0,scaleDestination,6400,80,64,80,0x5678],wordsAt (params+0x40) destination])
      (Just 1) pointers calls
    | vertical <- [-4,0,4 :: Int32],selector <- [0,1,2],completion <- [Crop.Completed,Crop.Failed],
      let slope = fromIntegral (vertical*1024),
      let margin = if vertical == 0 then 2 else 4,
      let source = [0,scaleSource,4096,64,64,64,0x1234],
      let destination = [0,scaleDestination,6400,80,80,80,0x5678],
      let frontendDestination = [0,scaleDestination+80*margin,64*(64-2*margin),80,64,56,0x5678],
      let coefficients = Crop.scaleCoefficientsFromWords [65536,0,0,0,0,slope,65536,0,0,0,0,margin,0,0],
      let block = case selector of 0 -> 64; 1 -> 32; _ -> 16,
      let (initial,final,calls,pointers) = Crop.scaleProcessLaw image resolve state block coefficients (Crop.cropPlaneFromWords source) (Crop.cropPlaneFromWords frontendDestination) completion]
  prepareAffineCases =
    [StageScenario (StageCaseName ("image-prepare-affine-"++show (mode,completion))) (publication 0xe5b978 "prepare_affine")
      [state,params,params+0x40]
      (configuration ++ initial ++ debugDisabledInitial image 0x9e0000 [kind] ++
       [wordsAt (state+0x2fcc) [65536,0,0,65536,0,0],word (state+0x4b4) mode,word (state+0x710) 0,
        wordsAt params source,wordsAt (params+0x40) destination,
        Patch (Address scaleSource) (B.pack [fromIntegral (n*13+37) | n <- [0..5119 :: Int]]),
        Patch (Address scaleDestination) (B.replicate 6400 0xa5)])
      (final ++ [wordsAt (params+0x40) frontendDestination] ++
       Operations.packedEdgeEffects (\address -> fromIntegral ((address-scaleSource)*13+37)) (Operations.GeometryPlane scaleSource 80 64 64) 0 2 0 0)
      (Just 1) pointers ([call 0xeb0d04 [] 0,call 0xeb0d04 [] 0] ++ calls)
    | mode <- [0x19,1],completion <- [Crop.Completed,Crop.Failed],
      let kind = if mode == 0x19 then 0x1b else 0x24,
      let source = [0,scaleSource,5120,80,64,64,0x1234],
      let destination = [0,scaleDestination,6400,80,64,64,0x5678],
      let frontendDestination = [0,scaleDestination+720,5120,80,64,64,0x5678],
      let coefficients = Crop.scaleCoefficientsFromWords [65536,0,0,0,0,0,65536,0,0,0,0,0,0,0],
      let (initial,final,calls,pointers) = Crop.scaleProcessLaw image resolve state 16 coefficients (Crop.cropPlaneFromWords source) (Crop.cropPlaneFromWords frontendDestination) completion]
  operationComplete = publication 0xe63e68 "operation_complete"
  geometryInterrupt = StagePublication (Address 0xda9230) (StageSymbol "xm1_pipeline_image_geometry_irq")
  copyCases =
    [StageScenario (StageCaseName ("image-copy-plane-geometry-event-"++show events)) (publication 0xe5bdcc "copy_plane")
      [state,params,params+0x40]
      (configuration ++ traceDisabledInitial ++ Warp.routeInitial ++ Warp.interruptInitial ++ reservedInitial 44 ++ priorityInitial 0x45 ++
       [configByte 0xa8 0,wordsAt params source,wordsAt (params+0x40) destination,
        Patch (Address scaleSource) (B.pack [fromIntegral (n*23+5) | n <- [0..5119 :: Int]]),
        Patch (Address scaleDestination) (B.replicate 6400 0xa5)])
      (requestEffects ++ [wordsAt 0x14ce600 (replicate 6 0),wordsAt 0x14ce680 dimensions,wordsAt 0x14ce618 plan,
        wordsAt params frontendDestination,wordsAt (params+0x40) destination,word 0x157bd08 0,
        wordsAt 0x96eb9c [1,0,0],wordsAt 0x14ce570 [2,0],word 0x942d8c 4,word 0xfff48004 0x24,word 0xfff480c8 1,word 0xfff480c4 0x331] ++
       Warp.interruptReleaseEffects 68)
      (Just 1)
      [(Address (0x1575844+27*4),operationComplete),(Address (0x14ce578+27*4),operationComplete)]
      ([call 0x42188 [0xe66708] 0] ++ reservedWaitCalls 44 0xffffffff 0 ++ Warp.routeRequestCalls True ++ Warp.routeAcquireCalls ++
       [Strip.resetCall] ++ Strip.geometryCopyDeviceCalls law ++ Warp.interruptAcquireCalls 68 13 1 (addressWord (resolve geometryInterrupt)) ++
       [call 0x42188 [0xe66710] 0] ++ currentTaskCalls 0x45 ++ [call 0x42634 [0,0] 0] ++ waitCalls 0x45 0x10 events 0 ++
       [Strip.resetCall] ++ Warp.interruptReleaseCalls 68 ++ [call 0x42188 [0xe66718] 0,Strip.resetCall] ++
       Warp.routeReleaseCalls False ++ Warp.routeRequestCalls True ++ reservedSignalCalls 44 0 0 ++ [call 0x42188 [0xe66720] 0])
    | events <- [0x10,0x20],
      let source = [0,scaleSource,5120,80,64,64,0x1234],
      let destination = [0,scaleDestination,6400,80,80,80,0x5678],
      let frontendDestination = copyPlaneOutput source destination,
      let inputPlane = Operations.GeometryPlane scaleSource 80 64 64,
      let outputPlane = Operations.GeometryPlane scaleDestination 80 64 64,
      let request = Operations.operationRequestWords image (Operations.OperationRequest 7 0 0 0 0 0 0) [Just inputPlane,Just inputPlane] 2 outputPlane (addressWord (resolve operationComplete)),
      let law = case Strip.geometryCopyLaw request (256,0,0,0) of Right value -> value; Left problem -> error (show problem),
      let Strip.GeometryDimensionsWords dimensions = Strip.copyDimensions law,
      let Strip.GeometryPlanWords plan = Strip.copyMainPlan law,
      let requestEffects = [word (address+fromIntegral index*4) value | address <- [0x1575844,0x14ce578],(index,value) <- zip [0::Int ..] request,index /= 30]]
  motion = state+0x10000
  meshSource = params+0x1000
  meshDestination = params+0x1800
  meshWorkspace = params+0x4000
  meshLabels = params+0x5000
  meshPoints = motion+0x11134
  meshMapped = motion+0x1192c
  meshTriangles = motion+0x12124
  meshPixels = B.pack [fromIntegral ((n*13+37) `mod` 256) | n <- [0..1023 :: Int]]
  meshIdentity = [0x00100008,65536,0,0,0,65536,0]
  meshCorners = [8,0,8,15,24,0,24,15]
  meshTriangleWords = concat [[0x00100008,x,y,z] | (x,y,z) <- [(0,1,2),(0,1,3),(0,2,3),(1,2,3)]]
  meshInitial =
    [Patch (Address meshSource) meshPixels,Patch (Address meshDestination) (B.replicate 640 0xa5),
     Patch (Address meshLabels) (B.replicate (992*16) 0xcc),wordsAt (params+0x100) [0,16,0,0,0xcccccccc,0xdddddddd,0xeeeeeeee,0xffffffff,0,0,0,meshPoints,meshMapped,meshTriangles]]
  meshEffects =
    [wordsAt meshPoints meshCorners,wordsAt meshMapped meshCorners,wordsAt meshTriangles meshTriangleWords,
     wordsAt meshWorkspace (concat (replicate 4 meshIdentity)),
     Patch (Address meshLabels) (B.concat [if offset `mod` 992 < 64 then wordBytes 0x00100008 else wordBytes 0 | offset <- ([0,4..992*16-4] :: [Int])])] ++
    [Patch (Address (meshDestination+row*40)) (B.take 32 (B.drop (fromIntegral (row*64+16)) meshPixels)) | row <- [0..15]]
  meshCases =
    [make ("image-mesh-warp-threshold-"++show threshold) 0xdae42c "mesh_warp" [params,params+0x40,meshWorkspace,params+0xc0,params+0x100]
      (meshInitial ++ [configByte 0x9fa5 threshold,wordsAt params [meshSource,64,64,16],wordsAt (params+0x40) [meshDestination,40,40,16],wordsAt (params+0xc0) [meshLabels,992,992,16]])
      (if threshold == 0 then meshEffects ++ [wordsAt (params+0x48) [32,16],wordsAt (params+0x110) [8,0,4,4]]
       else [Patch (Address meshLabels) (B.replicate (992*16) 0),wordsAt (params+0x118) [0,0],wordsAt (params+0x110) [0xcccccccc,0xdddddddd]])
      (Just (if threshold == 0 then 4 else 0)) []
    | threshold <- [0,1]] ++
    [make "image-reduce-pair-owned-mesh-pixels" 0xe5ac04 "reduce_pair" [state,params,params+0x40,params+0x80,params+0xc0]
      (priorityInitial 0x45 ++ meshInitial ++
       [word (state+0x3228) motion,wordsAt (motion+0xfc) [0,0,32],wordsAt (motion+0x118) [0,16],
        wordsAt params [0,meshSource,1024,64,64,16,0],wordsAt (params+0x40) [0,meshDestination,640,40,40,16,0],
        wordsAt (params+0x80) [0,meshWorkspace,4096,0,0,0,0],wordsAt (params+0xc0) [0,meshLabels,992*16,992,992,16,0]])
      (meshEffects ++ [wordsAt (params+0x50) [32,16],wordsAt (motion+0x120) [8,0,4,4]]) (Just 1)
      (currentTaskCalls 0x45 ++ priorityCalls 0x45 13 ++ [call 0xeb0d04 [] 0,call 0xeb0d04 [] 0] ++ priorityCalls 0x45 6)]
  triangleCases =
    [make ("image-fit-mesh-triangle-"++show (tx,ty,scale)) 0xdad71c "fit_mesh_triangle"
      [0,0,16,0,0,16,tx,ty,tx+16*scale,ty,tx,ty+16*scale,params]
      [wordsAt params (replicate 7 0xcccccccc)]
      [wordsAt params [0xcccccccc,scale*65536,0,tx*65536,0,scale*65536,ty*65536] | scale <= 16]
      (Just (if scale <= 16 then 1 else 0)) []
    | (tx,ty,scale) <- [(0,0,1),(2,3,1),(0,0,32)]] ++
    [make "image-mesh-circle-rounded-center" 0xdadb24 "mesh_circle" [8,0,8,15,24,0,params,params+8,params+12]
      [] [wordsAt params [16,8,11,128]] (Just 1) [],
     make "image-mesh-circle-collinear" 0xdadb24 "mesh_circle" [0,0,0,4,0,8,params,params+8,params+12]
      [wordsAt params [11,22,33,44]] [wordsAt params [11,22,33,44]] (Just 0) []]
  candidateCases =
    [make ("image-motion-candidates-threshold-"++show threshold) 0xe5adcc "reduce_pair_alternate" [state,a,b]
      [configByte 0x9fa5 threshold,configHalf 0x9f12 32,word (state+0x3228) motion,wordsAt (motion+0xfc) [0,0],word (motion+0x108) 32,
       wordsAt a [0x00100000,0x00500000,0x00900000],wordsAt b [0,0x00400000,0x00800000]]
      ([word (motion+0x118) 3,wordsAt (motion+0x134) [2,0,10,0,18,0],wordsAt (motion+0x8134) [0,0,8,0,16,0],Patch (Address (motion+0x10134)) (B.pack [1,1,1])] ++
       if threshold <= 3 then [word (motion+0x11c) 0,wordsAt (motion+0xfc) [0,0]] else [])
      (Just (if threshold <= 3 then 1 else 0))
      []
    | threshold <- [3,4]]
  filterCases =
    [make ("image-motion-filter-"++name) 0xdaeba8 "filter_motion_candidates" [params]
      [wordsAt params [count,0xcccccccc,params+0x80,a,b,0,0],Patch (Address (params+0x80)) (B.pack valid),wordsAt a (concatMap pair first),wordsAt b (concatMap pair second)]
      [Patch (Address (params+0x80)) (B.pack expected)] (Just result) []
    | (name,count,first,second,valid,expected,result) <-
      [("uniform",3,[(2,0),(10,0),(18,0)],[(0,0),(8,0),(16,0)],[1,1,1],[1,1,1],3),
       ("duplicate",3,[(2,0),(2,0),(18,0)],[(0,0),(8,0),(16,0)],[1,1,1],[1,0,1],2),
       ("length-outlier",3,[(2,0),(10,0),(26,0)],[(0,0),(8,0),(16,0)],[1,1,1],[1,1,0],2),
       ("vertical",2,[(0,2),(8,2)],[(0,0),(8,0)],[1,1],[1,1],2),
       ("all-invalid",2,[(2,0),(10,0)],[(0,0),(8,0)],[0,0],[0,0],0),
       ("empty",0,[],[],[],[],0),
       ("negative-count",0xffffffff,[],[],[],[],0)]]
  pair (x,y) = [x,y]
  bandCases =
    [make "image-motion-band-active" 0xdaf000 "select_motion_band" [params]
      [wordsAt params [3,0xcccccccc,params+0x80,a,b,0,0],Patch (Address (params+0x80)) (B.pack [1,1,1]),wordsAt a [2,2,4,2,12,2],wordsAt b [0,0,2,0,10,0]]
      [word (params+4) 0,wordsAt (params+20) [0,2],wordsAt a [2,2,4,2,12,2],wordsAt b [0,0,2,0,10,0]] Nothing
      []]
  sqrtCases =
    [make ("image-motion-sqrt-"++show (value,fraction)) 0xdaf26c "motion_sqrt" [fromIntegral value,fraction] [] [] (Just result) []
    | (value,fraction,result) <- [(-1,0,0),(0,0,0),(1,0,1),(2,0,1),(15,0,3),(16,0,4),(17,0,4),(2147395600,0,46340),(9,2,12),(1,1,2)] :: [(Int32,Word32,Word32)]]
  blendCases =
    [make ("image-blend-"++show (firstWidth,width,height,vertical,selector)) 0xe5a9c0 "blend_pixels" [state,params,params+0x40,fromIntegral vertical,selector]
      (priorityInitial 0x45 ++ [wordsAt params [0,a,256,32,firstWidth,4,0],wordsAt (params+0x40) [0,b,256,32,width,height,0],Patch (Address a) firstBytes,Patch (Address b) secondBytes])
      (blendEffects firstWidth width height vertical selector) Nothing (blendCalls 0x45)
    | width <- [0,1,2,6,16,30], firstWidth <- if width > 2 then [2,width] else [width], height <- [1,3], vertical <- ([-1,0,1] :: [Int32]), selector <- [0,1]]
  blendPriorityErrors =
    [make "image-blend-priority-error-preserves-pixels-and-restore" 0xe5a9c0 "blend_pixels" [state,params,params+0x40,0,0]
      (priorityInitial 0x45 ++ [wordsAt params [0,a,256,32,16,4,0],wordsAt (params+0x40) [0,b,256,32,16,3,0],Patch (Address a) firstBytes,Patch (Address b) secondBytes])
      (word 0x40a0f82c 0xfffffffb : blendEffects 16 16 3 0 0) Nothing
      (currentTaskCalls 0x45 ++ priorityCallsWith 0x45 12 0xfffffffb ++ [call 0xeb0d04 [] 0,call 0xeb0d04 [] 0] ++ priorityCalls 0x45 6)]
  blendEffects :: Word32 -> Word32 -> Word32 -> Int32 -> Word32 -> [Patch]
  blendEffects firstWidth width height vertical selector =
    [Patch (Address (output+row*32+column*2)) (B.pack [pixel lane row column | lane <- [0..3]])
    | width > 1, height /= distance, column <- takeWhile (< width `div` 2) [0,2..], row <- [0..height-distance-1]]
   where
    distance = fromIntegral (abs vertical)
    offsetA = if vertical < 0 then 0 else distance*32
    offsetB = if vertical < 0 then distance*32 else 0
    output = if selector == 0 then a+offsetA else b+offsetB
    pixel lane row column =
      let weight = toInteger (column*128 `div` (firstWidth `div` 2))
          index offset = fromIntegral (offset+row*32+column*2+lane)
          left = toInteger (B.index firstBytes (index offsetA))
          right = toInteger (B.index secondBytes (index offsetB))
          sumValue = left*(128-weight)+right*weight
          rounded = (sumValue+64) `quot` 128
       in fromInteger (max 0 (min 255 rounded))
  affineInputs =
    [[65536,0,0,65536,0,0],[65536,16384,8192,65536,196608,-65536],[-65536,-16384,8192,-65536,-196608,65536],
     [131071,0,0,65536,0,0],[131072,0,0,65536,0,0],[65536,0,131072,65536,0,0],
     [65536,0,0,65536,536870912,0],[0,0,0,0,0,0],[65536,0,0,0,0,0]] :: [[Int32]]
  affineCases =
    [make ("image-affine-coefficients-"++show index++"-null-"++show nullable) 0xe5afe8 "affine_coefficients" [state,params,if nullable then 0 else params+0x100]
      [wordsAt params (map fromIntegral input),wordsAt (params+0x100) (replicate 14 0xa5a5a5a5)]
      [wordsAt (params+0x100) (map fromIntegral result) | accepted, not nullable]
      (Just (if accepted then 1 else 0)) []
    | (index,input) <- zip [0 :: Int ..] affineInputs, nullable <- [False,True], let (accepted,result) = decompose input]
  blockCases =
    [make ("image-affine-block-"++show scale) 0xe5b2b4 "select_affine_block" [state,0,params]
      [wordsAt (state+0x2fcc) [65536,0,0,scale,0,0],word params 0xcccccccc]
      [word params result] (Just (if result == 0 then 0 else 1)) []
    | (scale,result) <- [(65536,16),(131071,0)]]
  safetyCases =
    [make ("image-affine-buffer-separation-"++show separation++"-height-"++show height) 0xe5b620 "affine_copy_safe" [state,0,params,params+0x40]
      [wordsAt (state+0x2fcc) [65536,0,0,65536,0,0],wordsAt params [0,a+separation,1024,32,16,height,0],wordsAt (params+0x40) [0,a,1024,32,16,height,0]]
      [] (Just (if height == 0 || separation == 4096 then 1 else 0)) []
    | separation <- [0,4096], height <- [0,16]]

decompose :: [Int32] -> (Bool,[Int32])
decompose [a,b,c,d,tx,ty] =
  let wide = fromIntegral :: Int32 -> Int64
      quotient n divisor = if divisor == 0 then if n < 0 then minBound else if n > 0 then maxBound else 0 else n `quot` divisor
      xx = quotient (wide a*wide d-wide b*wide c) (wide d)
      xy = quotient (wide b*65536) (wide d)
      bias = quotient (wide tx*wide d-wide ty*wide b) (wide d)
      accepted = all (\v -> abs (toInteger v) < 131072) [xx,xy,wide c,wide d] && all (\v -> abs (toInteger v) < 536870912) [bias,wide ty]
   in (accepted,[fromIntegral xx,fromIntegral xy,0,0,fromIntegral bias,c,d,0,0,ty,0,0,0,0])
decompose _ = error "affine input contains six signed fixed-point words"

imagePixelsOwnedFailureScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imagePixelsOwnedFailureScenarios image _resolve =
  [StageScenario (StageCaseName ("image-reduce-pair-owned-outcome-" ++ label ++ "-origin-" ++ show origin))
    (publication 0xe5ac04 "reduce_pair") [state,params,params+0x40,params+0x80,params+0xc0]
    (configuration before ++ priorityInitial task ++
     [word (state+0x3228) motion,word (motion+0x110) 0,wordsAt (motion+0xfc) [0,0,32]
     ,wordsAt (motion+0x118) [retained,16],wordsAt (motion+0x120) (origin ++ [0x51515151,0x61616161])
     ,wordsAt params sourcePlane,wordsAt (params+0x40) destinationPlane
     ,wordsAt (params+0x80) [0,workspace,4096,0,0,0,0],wordsAt (params+0xc0) [0,labels,992*16,992,992,16,0]
     ,Patch (Address source) pixels,Patch (Address destination) (B.replicate 640 0xa5)
     ,Patch (Address labels) (B.replicate (992*16) 0xcc)
     ,wordsAt (motion+0x134) [16,8],wordsAt (motion+0x8134) [17,9]
     ,Patch (Address (motion+0x10134)) (B.singleton 1)
     ,wordsAt points (replicate 4 0x71717171),wordsAt mapped (replicate 4 0x81818181)])
    ([wordsAt params sourcePlane,word (motion+0x110) 0,Patch (Address source) pixels,configByte 0x9fa5 after] ++
     if completed then
       [wordsAt (params+0x40) [0,destination,640,40,32,16,0x5678]
       ,wordsAt (motion+0x120) [8,0,4,4],Patch (Address destination) warpedPixels]
     else
       [wordsAt (params+0x40) destinationPlane,wordsAt (motion+0x120) (origin ++ [retained,0])
       ,Patch (Address destination) (B.replicate 640 0xa5),Patch (Address labels) (B.replicate (992*16) 0)
       ,wordsAt points (if retained == 0 then replicate 4 0x71717171 else [16,8,0x71717171,0x71717171])
       ,wordsAt mapped (if retained == 0 then replicate 4 0x81818181 else [17,9,0x81818181,0x81818181])])
    (Just (if completed && before <= 4 then 1 else 0)) []
    (currentTaskCalls task ++ priorityCalls task 13 ++
     [Call (Address 0xeb0d04) [] [] [configByte 0x9fa5 after | before /= after],call 0xeb0d04 [] 0] ++ priorityCalls task 6)
  | (label,before,after,retained,completed) <-
      [("empty-insufficient",1,1,0,False),("partial-insufficient",2,2,1,False)
      ,("cache-reentry-empty-insufficient",0,1,0,False),("cache-reentry-partial-insufficient",1,2,1,False)
      ,("completed-replaces-stale-origin",0,0,0,True),("completed-retains-older-acceptance-threshold",5,0,0,True)]
  ,origin <- [[0x13579bdf,0xfdb97531],[8,0]]] ++ guardedBlendCases
 where
  task = 0x45
  motion = state+0x10000
  source = params+0x1000
  destination = params+0x1800
  workspace = params+0x4000
  labels = params+0x5000
  points = motion+0x11134
  mapped = motion+0x1192c
  sourcePlane = [0,source,1024,64,64,16,0x1234]
  destinationPlane = [0,destination,640,40,40,16,0x5678]
  pixels = B.pack [fromIntegral ((n*13+37) `mod` 256) | n <- [0..1023 :: Int]]
  warpedPixels = B.concat [B.take 32 (B.drop (row*64+16) pixels) <> B.replicate 8 0xa5 | row <- [0..15]]
  configByte :: Word32 -> Word8 -> Patch
  configByte key value = Patch (Address (0x9e0000+key)) (B.singleton (fromIntegral value))
  configHalf :: Word32 -> Word32 -> Patch
  configHalf key value = Patch (Address (0x9e0000+key)) (B.take 2 (wordBytes value))
  configuration threshold =
    [word 0x96ca50 0x9e0000,configHalf 0x9f12 16,configHalf 0x9fa7 16
    ,configByte 0x9fa5 threshold,configByte 0x9fa9 8,configByte 0x9faa 8,configByte 0x9fab 32]
  guardedBlendCases =
    [StageScenario (StageCaseName ("image-mesh-failure-real-guard-no-blend-" ++ show origin))
      (StagePublication (Address 0xe94a44) (StageSymbol "xm1_develop_pipeline_blend_reference")) [context]
      (base ++ [Patch (Address 0xea6738) planTable,word (state+0x5cc) 4,word (state+0x3228) motion
       ,wordsAt (motion+0x10c) [0,1,0],wordsAt (motion+0x120) (origin ++ [1,0])
       ,wordsAt (motion+0x1c) sourcePlane,wordsAt (motion+0x8c) destinationPlane
       ,Patch (Address source) pixels,Patch (Address destination) (B.replicate 640 0xa5)])
      [word (state+0x38) guardSuccessor,wordsAt (motion+0x10c) [0,1,0]
      ,wordsAt (motion+0x120) (origin ++ [1,0]),wordsAt (motion+0x1c) sourcePlane,wordsAt (motion+0x8c) destinationPlane
      ,Patch (Address source) pixels,Patch (Address destination) (B.replicate 640 0xa5)]
      (Just 0) [] []
    | origin <- [[0x13579bdf,0xfdb97531],[8,0]]]
  planTable = B.take (127*52) (B.drop (0xea6738-0xbf1a00) image)
  guardSuccessor = findPlan 0
   where
    findPlan index
      | index > 126 = error "mesh failure blend plan missing from authenticated image"
      | readWord image address == 0x5e && readWord image (address+28) == 13 = readWord image (address+16)
      | otherwise = findPlan (index+1)
     where address = 0xea6738+index*52
