{-# LANGUAGE GHC2021 #-}
module PackedSoftwareScenarios (packedSoftwarePublications, packedSoftwareScenarios, packedScalePixels) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import PipelineModel
import Scenarios

packedSoftwarePublications :: [StagePublication]
packedSoftwarePublications = zipWith (\address name -> StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name)))
  [0x6e1084,0x6e1404,0x6e2308,0x6e234c]
  ["scale_packed_buffer","rotate_packed_buffer","fixed10_sine_degrees","fixed10_cosine_degrees"]

newtype Pixels = Pixels (M.Map Word32 Word8)
load :: Word32 -> Pixels -> Word32
load address (Pixels memory) = sum [fromIntegral (M.findWithDefault 0 (address+i) memory) `shiftL` fromIntegral (8*i) | i <- [0..3]]
store :: Word32 -> Word32 -> Pixels -> Pixels
store address value (Pixels memory) = Pixels (foldl (\m i -> M.insert (address+i) (fromIntegral (value `shiftR` fromIntegral (8*i))) m) memory [0..3])
patch :: Word32 -> B.ByteString -> Pixels -> Pixels
patch address bytes (Pixels memory) = Pixels (M.union (M.fromList (zip [address..] (B.unpack bytes))) memory)
extract :: Word32 -> Int -> Pixels -> B.ByteString
extract address count (Pixels memory) = B.pack [M.findWithDefault 0 (address+fromIntegral i) memory | i <- [0..count-1]]

combine :: Word32 -> Word32 -> Word32 -> Word32 -> Word32
combine first x second z = sum (zipWith shiftL channels [0,8,16,24])
 where
  component value channel = (value `shiftR` channel) .&. 255
  channels = [component first (fromIntegral ((x .&. 1)*8)),component second (fromIntegral ((z .&. 1)*8)),(component first 16+component second 16) `div` 2,(component first 24+component second 24) `div` 2]
blend :: Word32 -> Word32 -> Word32 -> Word32
blend opacity old new
  | opacity >= 128 = new
  | otherwise = sum [((((old `shiftR` lane) .&. 255)*(128-opacity)+((new `shiftR` lane) .&. 255)*opacity) `div` 128) `shiftL` lane | lane <- [0,8,16,24]]

scaleMemory :: [Word32] -> Pixels -> Pixels
scaleMemory [src,dst,sw,sh,dw,dh,ss,ds,cy,_,cx,_,opacity,_] initial
  | opacity == 0 = initial
  | otherwise = foldl write initial [(row,column) | row <- [0..fromIntegral dh-1 :: Int],column <- [0..fromIntegral (dw `div` 2)-1 :: Int]]
 where
  sourceBase = (src+(cy*ss+cx)*2) .&. 0xbfffffff
  coordinate dimension output index = ((fromIntegral index * toInteger dimension * 1024) `mod` 4294967296 `div` toInteger output) `div` 1024
  write memory (row,column) =
    let y = min sh (fromInteger (coordinate sh dh row))
        x = min sw (fromInteger (coordinate sw dw (column*2)))
        z = min sw (fromInteger (coordinate sw dw (column*2+1)))
        first = load (sourceBase+(ss `div` 2*y+x `div` 2)*4) memory
        second = load (sourceBase+(ss `div` 2*y+z `div` 2)*4) memory
        output = (dst .&. 0xbfffffff)+(ds `div` 2*fromIntegral row+fromIntegral column)*4
     in store output (blend opacity (load output memory) (combine first x second z)) memory
scaleMemory _ _ = error "packed scale request contains fourteen words"

packedScalePixels :: [Word32] -> B.ByteString -> B.ByteString -> B.ByteString
packedScalePixels request source destination = extract dst (B.length destination) (scaleMemory request initial)
 where
  (src,dst) = case request of
    (first:second:_) -> (first .&. 0xbfffffff,second .&. 0xbfffffff)
    _ -> error "packed scale request is short"
  initial = patch dst destination (patch src source (Pixels M.empty))

rotateMemory :: [Word32] -> Pixels -> Pixels
rotateMemory [src,dst,sw,sh,_,_,ss,ds,cy,_,cx,_,opacity,angle] initial = foldl write initial [(y,x) | y <- [minY..maxY],x <- take (fromIntegral pairs) [minX,minX+2..]]
 where
  sine = round (1024 * sin (fromIntegral (angle `mod` 360) * pi / 180) :: Double)
  cosine = round (1024 * cos (fromIntegral (angle `mod` 360) * pi / 180) :: Double)
  corners = [(x*cosine-y*sine,x*sine+y*cosine) | (x,y) <- [(0,0),(fromIntegral sw-1,0),(fromIntegral sw-1,fromIntegral sh-1),(0,fromIntegral sh-1)]] :: [(Int32,Int32)]
  xs = map ((`shiftR` 10) . fst) corners
  ys = map ((`shiftR` 10) . snd) corners
  minX = minimum xs; maxX = maximum xs; minY = minimum ys; maxY = maximum ys
  pairs = fromIntegral (maxX-minX+1) `div` 2 :: Word32
  sourceBase = (src+(cy*ss+cx)*2) .&. 0xbfffffff
  write memory (y,x) =
    let a = (x*cosine+y*sine) `shiftR` 10
        b = (-x*sine+y*cosine) `shiftR` 10
        c = ((x+1)*cosine+y*sine) `shiftR` 10
        d = (-(x+1)*sine+y*cosine) `shiftR` 10
        output = (dst .&. 0xbfffffff)+(ds `div` 2*fromIntegral (y-minY)+fromIntegral ((x-minX) `div` 2))*4
        sample px py = if px < 0 || py < 0 || px > fromIntegral sw || py > fromIntegral sh then load output memory else load (sourceBase+(ss `div` 2*fromIntegral py+fromIntegral (px `div` 2))*4) memory
     in store output (blend opacity (load output memory) (combine (sample a b) (fromIntegral a) (sample c d) (fromIntegral c))) memory
rotateMemory _ _ = error "packed rotate request contains fourteen words"

packedSoftwareScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
packedSoftwareScenarios _ _ = scaleCases ++ rotateCases ++ trigCases
 where
  descriptor = 0x009a1000
  source = 0x01600000
  destination = 0x01601000
  patterned = B.pack [fromIntegral ((index*73 + (index `div` 4)*19 + (index `div` 20)*31) `mod` 256) | index <- [0..1023 :: Int]]
  background = B.pack (take 1024 (cycle [201,9,155,77,31,243,111,21]))
  make name publication request operation overlap =
    let target = if overlap then source+4 else destination
        actual = take 1 request ++ [target] ++ drop 2 request
        initial = patch source patterned (Pixels M.empty)
        memory = if overlap then initial else patch target background initial
        result = operation actual memory
        callBase = (source+(actual!!8*(actual!!6)+actual!!10)*2) .&. 0xbfffffff
        finalAddress = if overlap then source else target
        maps = [(0x95e1f8,actual!!2,actual!!4),(0x9621f8,actual!!3,actual!!5)]
        initialMaps = [wordsAt address (replicate 16 0xdeadbeef) | (address,_,_) <- maps]
        finalMaps = [wordsAt address [if publication == 0 && actual!!12 /= 0 && index < output then fromInteger ((toInteger index * toInteger input * 1024) `mod` 4294967296 `div` toInteger output) else 0xdeadbeef | index <- [0..15]] | (address,input,output) <- maps]
     in StageScenario (StageCaseName ("packed-"++name)) (packedSoftwarePublications!!publication) [descriptor]
          ([wordsAt descriptor actual,wordsAt (descriptor+56) [0x1234abcd,0x8765fedc],Patch (Address source) patterned,Patch (Address target) (extract target 1024 memory)] ++ initialMaps)
          ([Patch (Address finalAddress) (extract finalAddress 1024 result),wordsAt descriptor actual,wordsAt (descriptor+56) [0x1234abcd,0x8765fedc]] ++ finalMaps)
          (Just 0xa1b2c3d4) []
          [call 0x42634 [callBase,actual!!3*(actual!!6)*2] 0x11111111,call 0x4263c [target,actual!!5*(actual!!7)*2] 0xa1b2c3d4]
  scaleCases =
    [make ("scale-"++show (sw,sh,dw,dh,opacity,overlap)) 0 [source,destination,sw,sh,dw,dh,11,13,1,0xfeed,2,0xbeef,opacity,0] scaleMemory overlap
    | (sw,sh,dw,dh) <- [(8,5,8,5),(8,5,4,3),(4,3,8,5),(7,5,5,4),(8,3,0,3),(8,3,1,3),(8,3,4,0),(0,0,4,3)],opacity <- [0,1,64,127,128,255,0xffffffff],overlap <- [False,True]]
  rotateCases =
    [make ("rotate-"++show (sw,sh,angle,opacity,overlap)) 1 [source,destination,sw,sh,1,1,11,13,1,0,2,0,opacity,angle] rotateMemory overlap
    | (sw,sh) <- [(8,5),(1,1),(0,0)],angle <- [0,30,45,90,180,270,315,360,450],opacity <- [0,64,128,255],overlap <- [False,True]]
  trigCases =
    [StageScenario (StageCaseName ("packed-trig-"++show (index,angle))) (packedSoftwarePublications!!index) [angle] [] [] (Just (fromIntegral value)) [] []
    | (angle,sine,cosine) <- ([(0,0,1024),(90,1024,0),(180,0,-1024),(270,-1024,0),(360,0,1024),(450,1024,0)] :: [(Word32,Int32,Int32)]),(index,value) <- ([(2,sine),(3,cosine)] :: [(Int,Int32)])]
