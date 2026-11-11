{-# LANGUAGE GHC2021 #-}
module ImageGeometryScenarios
  ( imageGeometryPublications, imageGeometryScenarios
  , CropInvocation(..), GeometryPadding(..), prepareBorder, transformPlanes, reducePlanes
  , reductionReady, bufferView, padBytes, rowBorders, radialCorrection, configureBorders
  ) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (foldl', nub)
import qualified Data.Map.Strict as M
import Data.Word
import PipelineModel
import Scenarios (Address(..), Patch(..), Call, word, wordsAt, readWord, call, state, params)
import BuffersScenarios (Plane(..), planeWords, planePatch)
import ImagePlanesScenarios (priorityInitial, currentTaskCalls, priorityCalls, debugDisabledInitial)
import qualified CropProcessScenarios as C
import qualified CropEngineScenarios as E
import qualified ImageOperationsScenarios as O

imageGeometryPublications :: [StagePublication]
imageGeometryPublications = [publication address name | (address,name) <-
  [(0xe551ac,"configure_planes"),(0xe5716c,"configure_plane_flags"),(0xe58cb8,"reduce_border"),
   (0xe58e10,"transform_border"),(0xe58f48,"prepare_border"),(0xe590e0,"transform_planes"),
   (0xe59510,"reduce_planes"),(0xe59408,"reduction_ready"),(0xe58c50,"fill_row_borders"),
   (0xe4f2a0,"buffer_views"),(0xe56da8,"geometry_levels"),(0xe55e88,"geometry_delta"),(0xe6628c,"pad_bytes"),
   (0x733f40,"read_packed12"),(0x7340b0,"write_packed12"),(0xe4f3b0,"configure_borders"),(0xeb0d04,"cache_maintenance")]] ++ E.cropEnginePublications
publication :: Word32 -> String -> StagePublication
publication a n = StagePublication (Address a) (StageSymbol ("xm1_pipeline_image_"++n))

data CropInvocation = CropInvocation { cropParameters :: [Word32], cropInput :: Plane, cropOutput :: Plane } deriving (Eq,Show)
data GeometryPadding = GeometryPadding { paddingByteInput :: Bool, paddingInput :: Plane, paddingRight :: Word32, paddingBottom :: Word32 } deriving (Eq,Show)

prepareBorder :: (Word32 -> Word32) -> Plane -> Plane -> Plane -> [CropInvocation]
prepareBorder configuration source scratch destination = [first,second]
 where
  step input output = let aligned=input { planeWidth=planeWidth input .&. complement 31,planeHeight=planeHeight input .&. complement 1 }; result=output { planeWidth=planeWidth aligned `div` 2,planeHeight=planeHeight aligned `div` 2 }; stride=(planeWidth result+31) .&. complement 31 in CropInvocation ([0,0,0,stride,0,0,0,0,0,0]++map configuration [0x49b0..0x49b3]) aligned result
  first=step source scratch
  second=step (cropOutput first) destination

transformPlanes :: (Word32 -> Word32) -> [Plane] -> Plane -> [CropInvocation]
transformPlanes configuration planes intermediate = go 0 (head planes)
 where
  controls=[if configuration 0x49c4 == 1 then 1 else 0]++map configuration [0x49c5..0x49cb]
  parameters transform width=[0,0,transform,(width+31) .&. complement 31,0,0]++controls
  go index input
   | index == 3 = let aligned=input { planeWidth=planeWidth input .&. complement 15,planeHeight=planeHeight input .&. complement 1 }; output=(planes!!3) { planeWidth=planeWidth aligned,planeHeight=planeHeight aligned } in [CropInvocation (parameters 1 (planeWidth output)) aligned output]
   | otherwise = let aligned=input { planeWidth=planeWidth input .&. complement 31,planeHeight=planeHeight input .&. complement 1 }; output=(if index == 2 then intermediate else planes!!(index+1)) { planeWidth=planeWidth aligned `div` 2,planeHeight=planeHeight aligned `div` 2 }; invocation=CropInvocation (parameters 0 (planeWidth output)) aligned output in invocation:go (index+1) output

reducePlanes :: B.ByteString -> (Word32 -> Word32) -> Word32 -> Bool -> [Plane] -> [(GeometryPadding,CropInvocation)]
reducePlanes image configuration kind alternate planes = go 0 (head planes) (tail planes)
 where
  bank=(case kind of 26 -> 2; 29 -> 4; _ -> 0)+(if alternate then 1 else 0)
  keys=[readWord image (0xe5fb70+bank*36+index*4) | index <- [0..8]]
  go :: Int -> Plane -> [Plane] -> [(GeometryPadding,CropInvocation)]
  go _ _ []=[]
  go index input (template:rest)=
   let byteInput=index/=0
       divisor=if byteInput then 1 else 2
       logical=planeWidth input `div` divisor
       width=(((logical+1) `div` 2+15) `div` 16)*16
       height=(planeHeight input+1) `div` 2
       right=width*2-logical
       bottom=height*2-planeHeight input
       output=template { planeWidth=width,planeHeight=height,planeLength=planeStride template*height }
       padded=input { planeWidth=planeWidth input+divisor*right,planeHeight=planeHeight input+bottom }
       first=if byteInput then [0,0,0,0,0,0] else [(configuration (keys!!0) `shiftR` 3) .&. 1,configuration (keys!!1),(configuration (keys!!0) `shiftR` 1) .&. 1]++map ((4*) . configuration . (keys!!)) [2..4]
       request=[if byteInput then 1 else 0,1,0,(width+31) .&. complement 31]++first++map (configuration . (keys!!)) [5..8]
   in (GeometryPadding byteInput input right bottom,CropInvocation request padded output):go (index+1) output rest

reductionReady :: (Word32 -> Word32) -> Word32 -> Word32 -> [Maybe Plane] -> Bool
reductionReady configuration kind flags planes =
  case key of
   Nothing -> False
   Just k -> fromIntegral (length planes)-1 >= configuration k && all valid planes && maybe False ((==0) . (.&.16) . planeFlags) (head planes)
 where
  key=case kind of 25 -> Just 0x472f; 1 | flags .&. 0x400 /=0 -> Just 0x472f; 26 -> Just 0x493f; 29 -> Just 0x9fc5; _ -> Nothing
  valid=maybe False ((/=0) . planeAddress)

bufferView :: (Word32 -> Word32) -> Plane -> (Plane,[Word32])
bufferView field input = (output,[horizontal,vertical])
 where
  width=field 0x480; height=field 0x484; divisor=if planeFlags input .&. 16 /=0 then 3 else 4
  bytes=width*divisor
  output=input { planeWidth=bytes `div` 2,planeHeight=height,planeAddress=planeAddress input+planeStride input*(planeHeight input `div` 2)+planeWidth input `div` 2-planeStride input*(height `div` 2)-bytes `div` 4,planeLength=height*(bytes `div` 2) }
  horizontal=(field 0x4a0+(planeWidth input*2 `div` divisor-width) `div` 2) `mod` 6
  vertical=(field 0x4a4+(planeHeight input-height) `div` 2) `mod` 6

type Pixels = M.Map Word32 Word8
byte :: Pixels -> Word32 -> Word8
byte memory address=M.findWithDefault 0 address memory
writeHalf :: Word32 -> Word32 -> Pixels -> Pixels
writeHalf address value=M.insert (address+1) (fromIntegral (value `shiftR` 8)) . M.insert address (fromIntegral value)
half :: Pixels -> Word32 -> Word32
half memory address=fromIntegral (byte memory address) .|. (fromIntegral (byte memory (address+1)) `shiftL` 8)

packedLocation :: Plane -> Int32 -> Word32 -> (Word32,Int)
packedLocation plane x y=(planeAddress plane+y*(planeStride plane .&. complement 3)+fromIntegral (group*12+slot*12 `div` 8),fromIntegral (slot*12 `mod` 8))
 where
  group=x `quot` 8
  remainder=x `rem` 8
  slot=if remainder<0 then 7 else remainder

packedSample :: Plane -> Int32 -> Word32 -> Pixels -> Word32
packedSample plane x y memory=(value `shiftR` bitShift) .&. 0xfff
 where
  (address,bitShift)=packedLocation plane x y
  value=foldl' (\out i -> out .|. (fromIntegral (byte memory (address+i)) `shiftL` fromIntegral (i*8))) 0 [0..2]

packedReplace :: Plane -> Int32 -> Word32 -> Word32 -> Pixels -> Pixels
packedReplace plane x y pixel memory=foldl' (\out i -> M.insert (address+i) (fromIntegral (value `shiftR` fromIntegral (i*8))) out) memory [0..2]
 where
  (address,bitShift)=packedLocation plane x y
  previous=foldl' (\out i -> out .|. (fromIntegral (byte memory (address+i)) `shiftL` fromIntegral (i*8))) 0 [0..2] :: Word32
  value=(previous .&. complement (0xfff `shiftL` bitShift)) .|. ((pixel .&. 0xfff) `shiftL` bitShift)

applyPatches :: Pixels -> [Patch] -> Pixels
applyPatches=foldl' (\memory (Patch (Address address) bytes) -> foldl' (\out (offset,value) -> M.insert (address+offset) value out) memory (zip [0..] (B.unpack bytes)))

mergePatches :: [Patch] -> [Patch]
mergePatches patches=pack (M.toAscList (applyPatches M.empty patches))
 where
  pack []=[]
  pack ((address,value):rest)=let (values,remaining)=contiguous (address+1) rest in Patch (Address address) (B.pack (value:values)):pack remaining
  contiguous expected ((address,value):rest) | address==expected = let (values,remaining)=contiguous (expected+1) rest in (value:values,remaining)
  contiguous _ rest=([],rest)

cropLaw :: Word32 -> B.ByteString -> (StagePublication -> Address) -> Word32 -> C.Completion -> CropInvocation -> ([Patch],[Patch],[Call],[(Address,StagePublication)])
cropLaw serial image resolve stage completion invocation=C.cropProcessLawWithSerial serial image resolve stage (C.cropParametersFromWords (cropParameters invocation)) (C.cropPlaneFromWords (planeWords (cropInput invocation))) (C.cropPlaneFromWords (planeWords (cropOutput invocation))) completion

cropSequence :: B.ByteString -> (StagePublication -> Address) -> Word32 -> C.Completion -> [CropInvocation] -> ([Patch],[Patch],[Call],[(Address,StagePublication)])
cropSequence image resolve stage completion invocations=(concat initials,mergePatches (concat finals),concat calls,nub (concat pointers))
 where
  (initials,finals,calls,pointers)=unzip4 (zipWith (\serial -> cropLaw serial image resolve stage completion) (iterate (E.nextSerial . E.nextSerial) 0) invocations)
  unzip4=foldr (\(a,b,c,d) (as,bs,cs,ds)->(a:as,b:bs,c:cs,d:ds)) ([],[],[],[])

indices :: Word32 -> [Word32]
indices count=take (fromIntegral count) [0..]

rowBorders :: Word32 -> Word32 -> Word32 -> Word32 -> Pixels -> Pixels
rowBorders address stride width height memory
 | (fromIntegral height :: Int32) <= 0 = memory
 | otherwise = foldl' (\m row -> writeHalf (address+row*(stride .&. complement 1)+(width .&. complement 1)-2) 0x8080 (writeHalf (address+row*(stride .&. complement 1)+2) 0x8080 m)) memory (indices height)

padBytes :: Plane -> Word32 -> Word32 -> Word32 -> Word32 -> Pixels -> Pixels
padBytes input left right top bottom memory = sides rightStart right (\m a -> byte m (a-1)) $ sides leftStart left (\m a -> byte m (a+left)) extended
 where
  address=planeAddress input; stride=planeStride input; width=planeWidth input; height=planeHeight input
  first=address-stride*top; leftStart=first-left; rightStart=first+width
  rows=top+height+bottom
  copiedWidth destination=let prefix=min width (destination .&. 3); groups=if prefix < width .&. complement 3 then ((width .&. complement 3)-prefix+3) `div` 4 else 0 in max width (prefix+groups*4)
  copyRow n m destination source=foldl' (\out i -> M.insert (destination+i) (byte m (source+i)) out) m (indices n)
  extended=foldl' (\m row -> copyRow (copiedWidth (address+stride*height)) m (address+stride*(height+row)) (address+stride*(height-1))) (foldl' (\m row -> copyRow (copiedWidth first) m (first+stride*row) address) memory (indices top)) (indices bottom)
  sides start count sample m=foldl' (\out row -> let a=start+stride*row; prefix=min count (start .&. 3); groups=if prefix < count .&. complement 3 then ((count .&. complement 3)-prefix+3) `div` 4 else 0; n=max count (prefix+groups*4); value=sample out a in foldl' (\out' i -> M.insert (a+i) value out') out (indices n)) m (indices rows)

radialCorrection :: Bool -> Word32 -> [Word32] -> Pixels -> Pixels
radialCorrection packed address strengths memory=foldl' correct memory [(x,y,c) | y <- [5..12],x <- [1..16],c <- [0..2]]
 where
  correct pixels (x,y,c)=let dx=x-4; dy=(y-4)*2; denominator=dx*dx+dy*dy; strength=fromIntegral (strengths!!c)::Int; correction=144*strength `div` denominator-13*strength; column=y+c*4; a=address+fromIntegral (x*128+column*2) in if correction<=0 then pixels else if packed then updatePacked pixels x column correction else writeHalf a (fromIntegral (max 0 (fromIntegral (half pixels a)-4*correction))) pixels
  updatePacked pixels x y correction=let bitOffset=x*12; a=address+fromIntegral (y*128+bitOffset `div` 8); bitShift=bitOffset `mod` 8; value=foldl' (\n i -> n .|. (fromIntegral (byte pixels (a+i)) `shiftL` fromIntegral (8*i))) (0::Word32) [0..2]; old=(value `shiftR` bitShift) .&. 0xfff; result=fromIntegral (max 0 (fromIntegral old-correction)); combined=(value .&. complement (0xfff `shiftL` bitShift)) .|. (result `shiftL` bitShift) in foldl' (\out i -> M.insert (a+i) (fromIntegral (combined `shiftR` fromIntegral (8*i)))) pixels [0..2]

between :: Word32 -> Word32 -> [Word32]
between start stop=if start<stop then [start..stop-1] else []

configureBorders :: Plane -> Word32 -> Word32 -> Word32 -> Word32 -> Pixels -> Pixels
configureBorders plane top bottom left right memory = foldl' middle cleared (between top end)
 where
  address=planeAddress plane; stride=planeStride plane; width=planeWidth plane; height=planeHeight plane; end=height-bottom
  clear locations m=foldl' (\out a -> M.insert a 0 out) m locations
  groups row start stop=[address+row*(stride .&. complement 7)+group*8+b | group <- between start stop,b <- [0..7]]
  clearRow row m=clear (groups row 0 (width `div` 8)++[address+row*stride+b | b <- between (width .&. complement 7) width]) m
  cleared=foldl' (flip clearRow) memory (indices top++between end height)
  middle m row
   | planeFlags plane .&. 0x10010 == 16 = clearPacked m row
   | otherwise = let factor=if planeFlags plane .&. 0x10000/=0 then 1 else 2; bytes=left*factor; first=width-right*factor; aligned=(first+7) .&. complement 7; remaining=max aligned (width .&. complement 7); locations=groups row 0 (bytes `div` 8)++[address+row*stride+b | b <- between (bytes .&. complement 7) bytes]++[address+row*stride+b | b <- between first aligned]++groups row (aligned `div` 8) (width `div` 8)++[address+row*stride+b | b <- between remaining width] in clear locations m
  clearPacked m row=foldl' (\out pixel -> let bitOffset=pixel*12; a=address+row*(stride .&. complement 3)+bitOffset `div` 8; offset=fromIntegral (bitOffset `mod` 8); value=foldl' (\v b -> v .|. (fromIntegral (byte out (a+b)) `shiftL` fromIntegral (8*b))) (0::Word32) [0..2]; result=value .&. complement (0xfff `shiftL` offset) in foldl' (\out' b -> M.insert (a+b) (fromIntegral (result `shiftR` fromIntegral (8*b)))) out [0..2]) m (indices left++between (width*2 `div` 3-right) (width*2 `div` 3))

imageGeometryScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imageGeometryScenarios image resolve = radialCases ++ rowCases ++ paddingCases ++ readyCases ++ viewCases ++ flagCases ++ borderCases ++ levelCases ++ singleReduction ++ packedCases ++ prepareCases ++ transformCases ++ reductionCases ++ E.cropEngineScenarios image resolve
 where
  make name address symbol arguments initial final result calls=StageScenario (StageCaseName name) (publication address symbol) arguments (word 0x96ca50 configurationAddress:initial) final result [] calls
  task=0x123
  cache=call 0x42634 [0,0] 0
  configurationAddress=0x9e0000
  cfgByte key value=Patch (Address (configurationAddress+key)) (B.singleton (fromIntegral (value::Word32)))
  cfgHalf key value=Patch (Address (configurationAddress+key)) (B.pack [fromIntegral (value::Word32),fromIntegral (value `shiftR` 8)])
  pixelsAddress=0x9c1000
  pixelsInitial :: Bool -> Word32 -> Pixels
  pixelsInitial packed low=M.fromList [(pixelsAddress+i,if packed then 0xff else if even i then fromIntegral low else fromIntegral (low `shiftR` 8)) | i <- [0..4095]]
  pixelPatch m=Patch (Address pixelsAddress) (B.pack [byte m (pixelsAddress+i) | i <- [0..4095]])
  radialCases=[make ("geometry-radial-"++show packed++"-"++show low) 0xe551ac "configure_planes" [state,params,0]
     (priorityInitial task++[planePatch params (Plane 0 pixelsAddress 4096 128 64 32 0x2800),word (state+0x2294) 1,wordsAt (state+0x2298) [400,320,240],word (state+0x49c) (if packed then 16 else 0),word (state+0x498) 0,pixelPatch initial]++map (uncurry cfgHalf) [(0x2eca,0),(0x2ecc,0),(0x2ece,16),(0x2ed0,8)]++concat [let keys=[readWord image (0xe58764+48+168+channel*28+j*4) | j <- [0..6]] in zipWith cfgHalf (take 4 keys) [4,4+channel*4,16,8+channel*4]++[cfgByte (keys!!6) 0] | channel <- [0..2]])
     [pixelPatch (radialCorrection packed pixelsAddress [100,80,60] initial)] (Just 1)
     (currentTaskCalls task++priorityCalls task 12++priorityCalls task 6)
     | packed <- [False,True],low <- [5000,16000],let initial=pixelsInitial packed low]
    ++[make "geometry-radial-disabled" 0xe551ac "configure_planes" [state,params,1] (priorityInitial task++[word (state+0x241c) 0,planePatch params (Plane 0 pixelsAddress 4096 128 64 32 0)]) [] (Just 1) (currentTaskCalls task)]
  rowCases=[make ("geometry-row-sentinels-"++show height) 0xe58c50 "fill_row_borders" [pixelsAddress,19,13,height] [pixelPatch initial] [pixelPatch (rowBorders pixelsAddress 19 13 height initial)] Nothing [cache,cache] | height <- [3,0,0xffffffff],let initial=pixelsInitial True 0]
  paddingCases=[make ("geometry-byte-border-"++show (offset,width,left,right,top,bottom)) 0xe6628c "pad_bytes" [pixelsAddress+offset,16,width,3,left,right,top,bottom]
    (priorityInitial task++[pixelPatch initial]) [pixelPatch (padBytes input left right top bottom initial)] Nothing
    (currentTaskCalls task++priorityCalls task 12++[cache,cache]++priorityCalls task 6)
    | (offset,width,left,right,top,bottom) <- [(64,7,0,0,0,0),(64,7,2,3,1,1),(64,7,0,2,0,1),(65,4,0,0,1,1)],let initial=M.fromList [(pixelsAddress+i,fromIntegral (i*13+7)) | i <- [0..4095]],let input=Plane 1 (pixelsAddress+offset) 48 16 width 3 0]
  readyCases=[make ("geometry-ready-"++show (kind,flags,count,format,valid)) 0xe59408 "reduction_ready" [state,1,params,count]
    ([word (state+0x4b4) kind,word (state+0x710) flags,wordsAt params [params+0x40,if valid then params+0x60 else 0],planePatch (params+0x40) first,planePatch (params+0x60) second]++configuration) [] (Just (if allowed then 1 else 0)) []
    | (kind,flags,count,format,valid) <- [(25,0,2,0,True),(26,0,2,0,True),(29,0,1,0,True),(1,0x400,2,0,True),(1,0,2,0,True),(25,0,2,16,True),(25,0,2,0,False)],let first=Plane 0 pixelsAddress 128 32 32 4 format,let second=first { planeAddress=pixelsAddress+256 },let key=case kind of 26 -> 0x493f; 29 -> 0x9fc5; _ -> 0x472f,let configuration=if kind==1 && flags==0 then [] else [cfgByte key 0],let allowed=not (kind==1 && flags==0) && format==0 && (valid || count==1)]
  viewCases=[make ("geometry-buffer-view-"++show flags) 0xe4f2a0 "buffer_views" [state,params,params+0x40,params+0x80,params+0x88]
    (planePatch params input:map (\(o,v)->word (state+o) v) fields) [planePatch (params+0x40) output,wordsAt (params+0x80) phase,Patch (Address (params+0x88)) (B.pack [1,0])] Nothing []
    | flags <- [0,16],let input=Plane 0 pixelsAddress 4096 128 96 32 flags,let fields=[(0x480,20),(0x484,12),(0x4a0,2),(0x4a4,4)],let (output,phase)=bufferView (\o->maybe 0 id (lookup o fields)) input]
  flagCases=[make ("geometry-plane-flags-"++show (disabled,level)) 0xe5716c "configure_plane_flags" [state,params,params+0x40]
    (priorityInitial task++[planePatch params input,planePatch (params+0x40) output,word (state+0x480) 20,word (state+0x484) 12,word (state+0x4a0) 0,word (state+0x4a4) 0,word (state+0x42c) 32,word (state+0x438) 16,word (state+0x640) 2,word (state+0x22ac) level,cfgHalf 0xa798 100,cfgByte 0xb2 (if disabled then 4 else 0)])
    [wordsAt (state+0x160c) (replicate 8 level),Patch (Address pixelsAddress) (B.pack (concat (replicate 20 [if disabled then 0 else 123,0]))),Patch (Address (state+0x19b6)) (B.pack [100,0]),wordsAt 0x1569610 (replicate 75 0)] (Just 1)
    (currentTaskCalls task++priorityCalls task 12++priorityCalls task 6)
    | disabled <- [False,True],level <- [123,0x1007b],let input=Plane 0 (pixelsAddress+512) 4096 128 96 32 0,let output=Plane 0 pixelsAddress 40 40 20 1 0]
  borderCases=[make ("geometry-clear-borders-"++show flags) 0xe4f3b0 "configure_borders" [params,1,1,3,5]
    (priorityInitial task++[planePatch params input,pixelPatch initial]) [pixelPatch (configureBorders input 1 1 3 5 initial)] Nothing
    (currentTaskCalls task++[cache]++priorityCalls task 12++priorityCalls task 6++[cache])
    | flags <- [0,16,0x10010],let input=Plane 0 pixelsAddress 256 64 (if flags==16 then 48 else 47) 4 flags,let initial=pixelsInitial True 0]
  levelCases=
    [make "geometry-levels-secondary-average" 0xe56da8 "geometry_levels" [state,0,1,params]
      [word (state+0x640) 1,Patch (Address (state+0x2584)) (B.pack [10,0,20,0,30,0,40,0,11,0,21,0,31,0,41,0])]
      [wordsAt params (take 8 (cycle [21,11,31,41]))] Nothing []]
    ++[make "geometry-levels-signed-delta" 0xe56da8 "geometry_levels" [state,0,0,params]
      [word (state+0x640) 1,word (state+0x49c) 0,wordsAt (state+0x226c) [1,1],Patch (Address (state+0x2274)) (B.pack [100,0,120,0,140,0,160,0,150,0,170,0,190,0,210,0]),wordsAt (state+0x2284) [900,1000,2000],cfgHalf (readWord image 0xe58614) 100]
      [wordsAt params (take 8 (cycle [70,50,90,110])),wordsAt (state+0x160c) (take 8 (cycle [70,50,90,110])),wordsAt (state+0x23e8) [130,180]] Nothing []]
  singleReduction=[make "geometry-single-level-no-crop" 0xe59510 "reduce_planes" [state,params,1,1,0]
    [word (state+0x4b4) 25,word params (params+0x40),planePatch (params+0x40) (Plane 0 pixelsAddress 256 64 64 4 0),cfgByte 0x472f 0] [] (Just 1) []]
  packedCases=concat
    [[make ("geometry-packed-read-"++show x) 0x733f40 "read_packed12" [params,fromIntegral x,1] [planePatch params input,pixelPatch initial] [] (Just (packedSample input x 1 initial)) []]
      ++[make ("geometry-packed-write-"++show (x,value)) 0x7340b0 "write_packed12" [params,fromIntegral x,1,value] [planePatch params input,pixelPatch initial] [pixelPatch (packedReplace input x 1 value initial)] Nothing [] | value <- [0,0x1234]]
    | x <- [-9,-1,0,1,2,3,4,5,6,7],let input=Plane 0 (pixelsAddress+64) 57 19 12 3 16,let initial=M.fromList [(pixelsAddress+i,fromIntegral (i*37+13)) | i <- [0..4095]]]
  imageMemory=Patch (Address pixelsAddress) (B.pack [fromIntegral (i*29+17) | i <- [0..0x9fff :: Int]])
  source=Plane 0 (pixelsAddress+256) 8192 256 130 19 0
  scratch=Plane 0 (pixelsAddress+0x3000) 4096 128 80 20 0
  destination=Plane 0 (pixelsAddress+0x5000) 2048 64 40 6 0
  prepareCases=
    [let invocations=prepareBorder configuration source scratch destination
         (initial,final,calls,pointers)=cropSequence image resolve state completion invocations
         scenario=make ("geometry-two-crops-"++show completion) 0xe58f48 "prepare_border" [state,params,params+0x40,params+0x80]
           (initial++[imageMemory,planePatch params source,planePatch (params+0x40) scratch,planePatch (params+0x80) destination]++zipWith cfgByte [0x49b0..0x49b3] controls++debugDisabledInitial image configurationAddress [0x25])
           (mergePatches (final++[planePatch (params+0x80) (cropOutput (last invocations))])) (Just 1) calls
     in scenario { stageCodePointers=pointers }
    | completion<-[C.Completed,C.Failed],let controls=[64,32,32,32],let configuration key=controls!!fromIntegral (key-0x49b0)]
  transformCases=
    [let planes=[source,scratch,destination,Plane 0 (pixelsAddress+0x7000) 1024 64 32 4 0]
         intermediate=Plane 0 (pixelsAddress+0x9000) 512 32 16 2 0
         configuration key=maybe 0 id (lookup key (zip [0x49c4..0x49cb] [1,64,16,16,64,32,32,32]))
         invocations=transformPlanes configuration planes intermediate
         (initial,final,calls,pointers)=cropSequence image resolve state completion invocations
         scenario=make ("geometry-four-crops-"++show completion) 0xe590e0 "transform_planes" [state,params,params+0x200,4]
           (initial++[imageMemory,wordsAt params [params+0x100+i*32 | i<-[0..3]],planePatch (params+0x200) intermediate]++zipWith planePatch [params+0x100+i*32 | i<-[0..3]] planes++[cfgByte key (configuration key) | key<-[0x49c4..0x49cb]]++debugDisabledInitial image configurationAddress [0x27,0x28])
           final (Just 1) calls
     in scenario { stageCodePointers=pointers }
    | completion<-[C.Completed,C.Failed]]
  reductionCases=
    [let planes=take count [Plane 0 (pixelsAddress+256) 1792 256 64 7 0,Plane 1 (pixelsAddress+0x3000) 256 64 32 4 0,Plane 1 (pixelsAddress+0x5000) 128 64 16 2 0]
         keys=[readWord image (0xe5fb70+i*4) | i<-[0..8]]
         values=[2,42,16,4,4,64,32,32,32]
         configuration key=maybe 0 id (lookup key (zip keys values))
         steps=reducePlanes image configuration 25 False planes
         memory=applyPatches M.empty [imageMemory]
         (_,_,initial,final,calls,pointers)=foldl' (reduceStep completion) (0,memory,[],[],[],[]) steps
         scenario=make ("geometry-pyramid-"++show (count,completion)) 0xe59510 "reduce_planes" [state,params,fromIntegral count,1,0]
           (initial++[imageMemory,word (state+0x4b4) 25,wordsAt params [params+0x100+fromIntegral i*32 | i<-[0..count-1]],cfgByte 0x472f 0]++zipWith planePatch [params+0x100+fromIntegral i*32 | i<-[0..count-1]] planes++[if i==1 then cfgHalf key value else cfgByte key value | (i,(key,value))<-zip [0::Int ..] (zip keys values)])
           (mergePatches final) (Just 1) calls
     in scenario { stageCodePointers=nub pointers }
    | count<-[2,3],completion<-[C.Completed,C.Failed]]
  reduceStep completion (serial,memory,initial,final,calls,pointers) (padding,invocation)=
    let input=paddingInput padding
        right=paddingRight padding; bottom=paddingBottom padding
        paddingEffects=if paddingByteInput padding then let output=padBytes input 0 right 0 bottom memory in mergePatches [Patch (Address address) (B.singleton value) | (address,value)<-M.toAscList output,M.lookup address memory/=Just value] else O.packedEdgeEffects (byte memory) (O.GeometryPlane (planeAddress input) (planeStride input) (planeWidth input) (planeHeight input)) 0 right 0 bottom
        paddingCalls=if paddingByteInput padding then currentTaskCalls 0x45++priorityCalls 0x45 12++[cache,cache]++priorityCalls 0x45 6 else [cache,cache]
        (cropInitial,cropFinal,cropCalls,cropPointers)=cropLaw serial image resolve state completion invocation
     in (E.nextSerial (E.nextSerial serial),applyPatches memory (paddingEffects++cropFinal),initial++cropInitial,final++paddingEffects++cropFinal,calls++paddingCalls++cropCalls,pointers++cropPointers)
