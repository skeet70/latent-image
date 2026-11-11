module ImageReferencesScenarios
  ( imageReferencesPublications
  , imageReferencesScenarios
  , ReferencePlane (..)
  , BlendSelection (..)
  , blendCoefficients
  , overlappingBlendOutput
  , noiseReferenceSources
  , motionRows
  , shiftedMotionWords
  , CurveParameters (..)
  , curveSample
  , curveTable
  ) where

import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word (Word32)
import PipelineModel hiding (Plane (..), planeWords, planePatch)
import Scenarios (Address (..), Patch (..), word, wordsAt)

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name))

imageReferencesPublications :: [StagePublication]
imageReferencesPublications = [publication address name | (address, name) <-
  [ (0xe5a7a0,"motion"), (0xe5c50c,"noise_reference"), (0xe5c8e0,"blend_reference")
  , (0xe5c9bc,"blend_overlapping"), (0xe5cb00,"lut_reference"), (0xe5d0f8,"estimate_affine")
  , (0xe5d8d8,"validate_affine"), (0xe5daac,"noise_pass"), (0xe5e76c,"reference_curve")
  , (0xe5f104,"motion_secondary"), (0xe5f45c,"motion_primary"), (0xe5f768,"reference_filter")
  , (0xe5c840,"blend_weights"), (0xe593a4,"describe_motion_rows"), (0xe59a78,"primary_motion_output")
  , (0xe59ac0,"secondary_motion_configuration"), (0xe5a71c,"copy_motion_words")
  , (0xe5a6cc,"select_reduction"), (0xe5983c,"primary_motion_allowed")
  , (0xe598cc,"secondary_motion_allowed"), (0xe59c74,"secondary_motion_options")
  , (0xe5a1bc,"primary_motion_options"), (0xe58c1c,"motion_conversion_complete")
  , (0xdac748,"initialize_motion"), (0xdac8d8,"set_motion_offsets"), (0xe5be9c,"affine_corner_matrix")
  , (0xe59b7c,"reduction_count"), (0xd9b9ec,"affine_corners_valid"), (0xd9e5f4,"curve_interval")
  , (0xd9e1f0,"sample_curve"), (0xd9e710,"build_curve")
  , (0x737cd0,"conversion_selection")
  , (0x69ff68,"configuration_signed_byte"), (0x69ff7c,"configuration_signed_half")
  , (0x736f98,"conversion_index"), (0x747b50,"channel_count"), (0x747d48,"channel_packed_value")
  ]]

data ReferencePlane = ReferencePlane
  { planeMode :: Word32
  , planeAddress :: Word32
  , planeSize :: Word32
  , planeStride :: Word32
  , planeWidth :: Word32
  , planeHeight :: Word32
  , planeFlags :: Word32
  } deriving (Eq, Show)

data BlendSelection = EqualBlend | SelectedBlend Word32 deriving (Eq, Show)
data ConfigurationView = SignedByte | SignedHalf

blendCoefficients :: BlendSelection -> Word32 -> [Word32]
blendCoefficients selection count
  | count < 2 = [0,0,0,0]
  | otherwise = [coefficient index | index <- [1..4]]
  where
    primary = (count + 255) `div` count
    secondary = (256-primary) `div` (count-1)
    coefficient index
      | index > count = 0
      | selection == SelectedBlend index = primary
      | otherwise = secondary

overlappingBlendOutput :: [ReferencePlane] -> ReferencePlane -> ReferencePlane
overlappingBlendOutput inputs destination = case inputs of
  [] -> destination
  first : _ -> destination
    { planeWidth = planeWidth first
    , planeHeight = planeHeight first
    , planeStride = stride
    , planeSize = stride * planeHeight first
    }
  where
    start = planeAddress destination
    end = start + planeStride destination * planeHeight destination
    overlaps input = start <= planeAddress input + planeStride input * planeHeight input && planeAddress input <= end
    stride = foldl (\previous input -> if overlaps input then planeStride input else previous) (planeStride destination) (take 5 inputs)

noiseReferenceSources :: ReferencePlane -> ReferencePlane -> [ReferencePlane]
noiseReferenceSources source reference = map expand [reference,source]
  where expand plane = plane
          { planeAddress = planeAddress plane - 16 - planeStride plane * 8
          , planeWidth = planeWidth plane + 32
          , planeHeight = planeHeight plane + 16
          , planeSize = planeStride plane * (planeHeight plane + 16)
          }

motionRows :: ReferencePlane -> [Word32]
motionRows plane = [mode, planeAddress plane, planeWidth plane `div` divisor, planeHeight plane, planeStride plane]
  where
    (mode, divisor)
      | planeFlags plane .&. 0x100000 /= 0 = (2,2)
      | planeFlags plane .&. 0x20000 /= 0 = (0,2)
      | otherwise = (1,1)

shiftedMotionWords :: Word32 -> Bool -> [(Word32, Word32)] -> [Word32]
shiftedMotionWords reduction unchanged pairs = map (adjust . fst) pairs
  where
    adjust value
      | unchanged || value == maxBound = value
      | amount >= 32 = 0
      | otherwise = ((value `shiftR` 16 `shiftL` fromIntegral amount) `shiftL` 16) .|. ((value .&. 0xffff) `shiftL` fromIntegral amount)
    amount = reduction .&. 255

data CurveParameters = CurveParameters
  { curveCount :: Word32
  , curveSelected :: Word32
  , curveLimits :: (Int32,Int32,Int32)
  , curveMapped :: Int32
  , curveChannel :: Int32
  , curveScale :: Int32
  , curveKnots :: [Int32]
  , curveMode :: Word32
  , curveThreshold :: Word32
  , curveRanges :: (Int32,Int32,Int32,Int32)
  } deriving (Eq, Show)

curveWords :: CurveParameters -> [Word32]
curveWords parameters =
  [curveCount parameters,curveSelected parameters] ++ map fromIntegral [first,second,third,curveMapped parameters,curveChannel parameters,curveScale parameters] ++
  map fromIntegral (curveKnots parameters) ++ [curveMode parameters,curveThreshold parameters] ++ map fromIntegral [target,left,middle,right]
  where
    (first,second,third) = curveLimits parameters
    (target,left,middle,right) = curveRanges parameters

curveSample :: CurveParameters -> Word32 -> Word32
curveSample parameters value = (curveMode parameters * min divided 0x1ffffff + 16) `shiftR` 5
  where
    x = fromIntegral value :: Int32
    segments = case curveCount parameters of 2 -> 1; 3 -> 2; 4 -> 3; _ -> 4
    segment = case curveCount parameters of
      2 -> 0
      3 -> if value .&. 128 == 0 then 0 else 1
      4 -> if value < 85 then 0 else if value < 170 then 1 else 2
      _ -> if value < 64 then 0 else if value .&. 128 == 0 then 1 else if value < 191 then 2 else 3
    distance = x * (8*segments) - fromIntegral segment*2040
    knots = curveKnots parameters
    interpolated = (knots !! segment * (2040-distance) + knots !! (segment+1) * distance + 127) `quot` 255
    scaled = (interpolated * curveScale parameters * 8 + 500) `quot` 1000
    reference = (curveScale parameters * knots !! fromIntegral (curveSelected parameters) * 64 + 500) `quot` 1000
    weight = (abs (reference-scaled) * 125 + 64) `quot` 128
    normalized = scaled*8 `quot` curveChannel parameters
    (limit,contrast,denominatorScale) = curveLimits parameters
    denominator = (contrast*1024 `quot` denominatorScale) * normalized + 1048576
    response = denominator `quot` (normalized+1024)
    corrected = (2048 - (response*response+512) `quot` 1024) * 524288 `quot` denominator
    weighted = corrected * weight * 32 `quot` curveMapped parameters
    divided = fromIntegral (weighted `quot` limit)

curveTable :: CurveParameters -> (CurveParameters,B.ByteString)
curveTable parameters = (selected, B.pack (map (fromIntegral . first) samples ++ map (fromIntegral . second) samples ++ concatMap half samples))
  where
    (target,left,middle,right) = curveRanges parameters
    maximumValue = max (curveSample (parameters {curveMode=32}) 0) (curveSample (parameters {curveMode=32}) 255)
    selected = if curveThreshold parameters == 1
      then parameters {curveMode=(fromIntegral target*32 + (maximumValue `shiftR` 1)) `div` maximumValue}
      else parameters
    samples = [let (lower,upper,weight)=intervalModel (fromIntegral (curveSample selected sample)) left middle right
               in (lower,if weight>255 then lower else upper,min weight 255) | sample <- [0..255]]
    first (lower,_,_) = lower
    second (_,upper,_) = upper
    half (_,_,weight) = [fromIntegral weight,fromIntegral (weight `shiftR` 8)]

planeWords :: ReferencePlane -> [Word32]
planeWords plane = [planeMode plane,planeAddress plane,planeSize plane,planeStride plane,planeWidth plane,planeHeight plane,planeFlags plane]

imageReferencesScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imageReferencesScenarios _ _ = weightCases ++ rowCases ++ copyCases ++ offsetCases ++ validationCases ++ cornerCases ++ intervalCases ++ curveRefusals ++ curveCases ++ configurationCases ++ selectionCases ++ indexCases ++ channelCases ++ affineValidationCases
  where
    scenario label address name arguments initial final returned = StageScenario
      (StageCaseName ("image-reference-" ++ label)) (publication address name) arguments initial final returned [] []
    workspace = 0x009a8000
    output = workspace + 0x100
    job = 0x009b0000
    weightCases =
      [ scenario ("weights-" ++ show count ++ "-" ++ show selected) 0xe5c840 "blend_weights"
          [output,selected,count] [wordsAt output [0xdeadbeef,0xdeadbeef,0xdeadbeef,0xdeadbeef]]
          [wordsAt output (blendCoefficients (if selected == 0 then EqualBlend else SelectedBlend selected) count)] Nothing
      | count <- [2,3,4,5], selected <- [0,1,2,3,4]
      ]
    rowCases =
      [ let plane = ReferencePlane 7 0x10900000 512 64 31 8 flags
        in scenario ("rows-" ++ show flags) 0xe593a4 "describe_motion_rows" [workspace,output,2]
          [wordsAt workspace (planeWords plane),wordsAt output (replicate 7 0xdeadbeef)]
          [wordsAt output (motionRows plane ++ [0xdeadbeef,0xdeadbeef])] Nothing
      | flags <- [0,0x20000,0x100000,0x120000]
      ]
    pairs = [(0x00120034,0x11223344),(0xffffffff,0x55667788),(0x80000001,0x99aabbcc)]
    copyCases =
      [ scenario ("copy-" ++ show reduction ++ "-" ++ show unchanged ++ "-" ++ show alias)
          0xe5a71c "copy_motion_words" [workspace,if alias then workspace else output,3,reduction,if unchanged then 1 else 0]
          [wordsAt workspace (concatMap (\(first,second) -> [first,second]) pairs),wordsAt output (replicate 3 0xdeadbeef)]
          [wordsAt (if alias then workspace else output) (shiftedMotionWords reduction unchanged pairs)] Nothing
      | reduction <- [0,1,7,31,32], unchanged <- [False,True], alias <- [False,True]
      ]
    offsetCases =
      [ scenario ("offset-transition-" ++ show horizontal) 0xdac8d8 "set_motion_offsets"
          [fromIntegral horizontal,fromIntegral vertical] [wordsAt 0x014e2a28 [0x80000000,0x7fffffff]]
          [wordsAt 0x014e2a28 [fromIntegral horizontal,fromIntegral vertical]] Nothing
      | (horizontal,vertical) <- [(0,0),(-17,31),(2147483647,-2147483648)] :: [(Int32,Int32)]
      ]
    validationCases =
      [ scenario "primary-rejected-kind" 0xe5f45c "motion_primary" [job,workspace,output,1]
          [word (job+0x4b4) 0,word workspace (workspace+0x200),wordsAt (workspace+0x200) [0,0,0,0,0,0,0],Patch (Address 0x01571534) (B.replicate 0x15c 0xa5)]
          [Patch (Address 0x01571534) (B.replicate 0x15c 0)] (Just 0)
      , scenario "curve-rejected-capacity" 0xe5e76c "reference_curve" [job,3,1,workspace]
          [wordsAt workspace [0,output,1023,64,32,8,0]] [wordsAt workspace [0,output,1023,64,32,8,0]] (Just 0)
      ]
    cornerCases =
      [ scenario ("corners-" ++ show margin ++ "-" ++ show translation) 0xd9b9ec "affine_corners_valid"
          [64,32,workspace,0,workspace+0x40]
          [wordsAt workspace [65536,0,0,65536,fromIntegral translation,0],wordsAt (workspace+0x40) [margin,margin]]
          [] (Just (if margin >= 5 && margin < 28 && translation == 0 then 1 else 0))
      | (margin,translation) <- [(8,0),(4,0),(8,-6553600),(28,0)] :: [(Word32,Int32)]
      ]
    intervalCases =
      [ let (lower,upper,weight) = intervalModel value 64 128 192
        in scenario ("interval-" ++ show value) 0xd9e5f4 "curve_interval"
          [fromIntegral value,output,output+4,output+8,64,128,192]
          [wordsAt output [0xdeadbeef,0xdeadbeef,0xdeadbeef]]
          [wordsAt output [lower,upper,fromIntegral weight]] Nothing
      | value <- [-100,0,32,64,65,96,128,160,192,193,1000]
      ]
    curveRefusals =
      [ scenario ("curve-table-refusal-" ++ show count ++ "-" ++ show selected ++ "-" ++ show threshold)
          0xd9e710 "build_curve" [workspace,output]
          [wordsAt workspace ([count,selected] ++ replicate 12 0 ++ [threshold] ++ replicate 4 0),Patch (Address output) (B.replicate 1024 0xa5)]
          [Patch (Address output) (B.replicate 1024 0xa5)] (Just 0)
      | (count,selected,threshold) <- [(1,0,0),(6,0,0),(2,2,0),(2,0,2)]
      ]
    curveParameters count threshold = CurveParameters count 0 (100,100,100) 32 8 16 [0,128,256,384,512] 32 threshold (256,64,128,192)
    curveCases =
      [ let parameters=curveParameters count 0
        in scenario ("curve-sample-" ++ show count ++ "-" ++ show sample) 0xd9e1f0 "sample_curve"
          [sample,workspace] [wordsAt workspace (curveWords parameters)] [] (Just (curveSample parameters sample))
      | count <- [2,3,4,5], sample <- [0,63,64,84,85,127,128,169,170,190,191,255]
      ] ++
      [ let parameters=curveParameters count threshold
            (finalParameters,table)=curveTable parameters
        in scenario ("curve-table-" ++ show count ++ "-" ++ show threshold) 0xd9e710 "build_curve"
          [workspace,output] [wordsAt workspace (curveWords parameters),Patch (Address output) (B.replicate 1024 0xa5)]
          [wordsAt workspace (curveWords finalParameters),Patch (Address output) table] (Just 1)
      | count <- [2,3,4,5], threshold <- [0,1]
      ]
    configurationCases =
      [ let initial=[word 0x96ca50 workspace,Patch (Address (workspace+17)) (B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)])]
            byte=value .&. 255
        in scenario ("configuration-" ++ name ++ "-" ++ show value) address name [17] initial initial
          (Just (case view of
            SignedByte -> if byte < 128 then byte else byte-256
            SignedHalf -> if value < 32768 then value else value-65536))
      | (address,name,view) <- [(0x69ff68,"configuration_signed_byte",SignedByte),(0x69ff7c,"configuration_signed_half",SignedHalf)]
      , value <- [0,127,128,255,256,32767,32768,65152,65535]
      ]
    selectionCases =
      [ scenario ("selection-" ++ show value) 0x737cd0 "conversion_selection" [value] [] []
          (Just (if value == 0 then 160 else if value == 128 || value == 255 then value else value-1))
      | value <- [0,1,2,127,128,129,254,255,256,0xffffffff]
      ]
    indexCases =
      [ scenario ("index-" ++ show value) 0x736f98 "conversion_index" [value] [] [] (Just (if value == 0 then 0 else value-1))
      | value <- [0,1,2,127,128,255,256,0x80000000,0xffffffff]
      ]
    channelCases =
      [ let initial=[word 0x96ca50 0x10000000,Patch (Address 0x103e0f80) (B.singleton count)]
        in scenario ("channel-count-" ++ show count) 0x747b50 "channel_count" [] initial initial (Just (max 1 (min 100 (fromIntegral count))))
      | count <- [0,1,2,99,100,101,255]
      ] ++
      [ let initial=[word 0x96ca50 0x10000000,Patch (Address 0x103e0f80) (B.singleton count)]
            valid=channel < max 1 (min 100 (fromIntegral count))
            values=if valid then [Patch (Address (0x103e2007+channel*3)) (B.pack [0x56,0x34,0x12])] else []
        in scenario ("channel-value-" ++ show count ++ "-" ++ show channel) 0x747d48 "channel_packed_value" [channel]
          (initial++values) (initial++values) (Just (if valid then 0x123456 else 0xffff))
      | count <- [0,1,2,100,255], channel <- [0,1,98,99,100,0x80000000,0xffffffff]
      ]
    affineValidationCases =
      [ let debug=0x10000000
            pixels=0x10100000
            source=[0,pixels+4096,512,32,16,16,0]
            destination=[0,pixels,512,32,16,16,0]
            result=if bits == 7 && previous == 1 then 1 else 0
        in scenario ("affine-validation-" ++ show scale ++ "-prior-" ++ show previous) 0xe5d8d8 "validate_affine"
          [job,workspace,workspace+0x40,job+0x2fcc]
          [wordsAt workspace source,wordsAt (workspace+0x40) destination,word (job+0x220c) debug,
           wordsAt (job+0x2fcc) [65536,0,0,scale,0,0],word (job+0x2fe4) 1,word (job+0x2fe8) 1,
           word (job+0x2fbc) previous,word (job+0x2fc0) 0xaaaaaaaa,word (debug+0xdace4) 0xbbbbbbbb]
          [wordsAt workspace source,wordsAt (workspace+0x40) destination,word (job+0x2fbc) result,
           word (job+0x2fc0) bits,word (debug+0xdace4) bits] (Just result)
      | (scale,bits) <- [(65536,7),(131071,5)], previous <- [0,1,2]
      ]

intervalModel :: Int32 -> Int32 -> Int32 -> Int32 -> (Word32,Word32,Int32)
intervalModel value left middle right
  | value <= left = (0,1,clamp ((left-value)*256 `quot` left))
  | value <= middle = (1,2,clamp ((middle-value)*256 `quot` (middle-left)))
  | value <= right = (2,3,clamp ((right-value)*256 `quot` (right-middle)))
  | otherwise = (2,3,0)
  where clamp = max 0 . min 256
