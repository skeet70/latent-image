module ProfileCalibrationScenarios
  ( profileCalibrationPublications
  , profileCalibrationScenarios
  , CalibrationValues (..)
  , FactorPair (..)
  , ProfileFactors (..)
  , ClampParameters (..)
  , ClampMode (..)
  , clampMode
  , ClampResult (..)
  , calibrateProfileValues
  , clampProfileValues
  , profileHistoryPercent
  , scaleProfileValues
  , negativeProfileValues
  , profileCalibrationInitial
  , calibrationValuesPatch
  , StreamParameters (..)
  , streamParameterWords
  , streamProgramEffects
  , StreamFault (..)
  , StreamFaultScenario (..)
  , profileCalibrationFaultScenarios
  ) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int16, Int32)
import Data.List (foldl')
import Data.Word (Word8, Word16, Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), call, word, wordsAt)

data CalibrationValues = CalibrationValues Word16 Word16 Word16 deriving (Eq, Show)
data FactorPair = FactorPair Word16 Word16 deriving (Eq, Show)
data ProfileFactors = ProfileFactors
  { deltaFactors :: FactorPair
  , splitFactors :: FactorPair
  , selectedFactors :: FactorPair
  } deriving (Eq, Show)
data ClampParameters = ClampParameters
  { minimumGains :: CalibrationValues
  , maximumGains :: CalibrationValues
  , primaryCeiling :: Word16
  } deriving (Eq, Show)
newtype IndependentClass = IndependentClass Word32 deriving (Eq, Show)
data ClampMode = PrimaryClamp | SecondaryClamp | IndependentClamp IndependentClass deriving (Eq, Show)

clampMode :: Word32 -> ClampMode
clampMode 0 = PrimaryClamp
clampMode 1 = SecondaryClamp
clampMode value = IndependentClamp (IndependentClass value)
data ClampResult = ClampResult
  { clampedValues :: CalibrationValues
  , upperFlags :: Word32
  , lowerFlags :: Word32
  } deriving (Eq, Show)

values :: CalibrationValues -> [Word16]
values (CalibrationValues first second third) = [first,second,third]

triplet :: [Word16] -> CalibrationValues
triplet [first,second,third] = CalibrationValues first second third
triplet _ = error "a calibration triplet contains three gains"

pair :: FactorPair -> [Word16]
pair (FactorPair first second) = [first,second]

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]

halves :: Word32 -> [Word16] -> Patch
halves address = Patch (Address address) . B.concat . map halfBytes

calibrationValuesPatch :: Address -> CalibrationValues -> Patch
calibrationValuesPatch (Address address) = halves address . values

calibrateProfileValues :: ProfileFactors -> CalibrationValues -> CalibrationValues
calibrateProfileValues factors input = triplet (head source : zipWith convert (tail source) combined)
  where
    source = values input
    combined = zipWith3 (\a b c -> fromInteger ((toInteger a*toInteger b*toInteger c) `div` 1048576) :: Word16)
      (pair (deltaFactors factors)) (pair (splitFactors factors)) (pair (selectedFactors factors))
    convert value factor = fromInteger ((toInteger value*toInteger factor+512) `div` 1024)

modeClass :: ClampMode -> Word32
modeClass PrimaryClamp = 0
modeClass SecondaryClamp = 1
modeClass (IndependentClamp (IndependentClass value)) = value

flag :: ClampMode -> Int -> Word32
flag mode lane = if amount < 32 then bit amount else 0
  where amount = fromIntegral (((modeClass mode .&. 255)*3+fromIntegral lane) .&. 255)

rescaled :: Int -> Word16 -> [Word16] -> [Word16]
rescaled lane target source = [if index == lane then target else convert value | (index,value) <- zip [0..] source]
  where
    denominator = source !! lane
    convert value
      | denominator == 0 = fromIntegral (fromIntegral value * fromIntegral target :: Word32)
      | otherwise = fromIntegral ((fromIntegral (fromIntegral value * fromIntegral target :: Word32) :: Int32) `quot` fromIntegral denominator)

clampProfileValues :: ClampMode -> ClampParameters -> CalibrationValues -> ClampResult
clampProfileValues mode parameters input = case mode of
  IndependentClamp _ -> ClampResult (triplet independent) independentUpper independentLower
  _ -> ClampResult (triplet capped) cappedUpper loweredFlags
  where
    original = values input
    lower = values (minimumGains parameters)
    upper = values (maximumGains parameters)
    independent = zipWith3 (\value low high -> if value > high then high else max value low) original lower upper
    independentUpper = foldl' (.|.) 0 [flag mode lane | (lane,(value,high)) <- zip [0..] (zip original upper), value > high]
    independentLower = foldl' (.|.) 0 [flag mode lane | (lane,(value,low,high)) <- zip [0..] (zip3 original lower upper), value <= high, value < low]
    raise (source,mask) lane
      | source !! lane >= lower !! lane = (source,mask)
      | otherwise = (rescaled lane (lower !! lane) source,mask .|. flag mode lane)
    (raised,raisedFlags) = foldl' raise (original,0) [0..2]
    (limited,limitedFlags)
      | raisedFlags /= 0 && head raised > primaryCeiling parameters = (rescaled 0 (primaryCeiling parameters) raised,raisedFlags .|. flag mode 3)
      | otherwise = (raised,raisedFlags)
    repaired = [if raisedFlags /= 0 && lane > 0 then max value (lower !! lane) else value | (lane,value) <- zip [0..] limited]
    loweredFlags = limitedFlags .|. foldl' (.|.) 0 [flag mode (lane+3) | lane <- [1,2], raisedFlags /= 0, limited !! lane < lower !! lane]
    capped = zipWith min repaired upper
    cappedUpper = foldl' (.|.) 0 [flag mode lane | (lane,(value,high)) <- zip [0..] (zip repaired upper), value > high]

sourceSlice :: B.ByteString -> Word32 -> Int -> B.ByteString
sourceSlice image5 address size
  | B.length result /= size = error "profile calibration requires pinned image5 bytes"
  | otherwise = result
  where result = B.take size (B.drop (fromIntegral (address-0xbf1a00)) image5)

profileHistoryPercent :: B.ByteString -> Int32 -> Word32
profileHistoryPercent image5 exposure
  | quotient < 0 = (shifted 65536 (fromIntegral (negate quotient) .&. 255) False * tableValue) `shiftR` 16
  | otherwise = shifted 1 (fromIntegral quotient .&. 255) True * tableValue
  where
    fullQuotient = exposure `quot` 100
    quotient = fromIntegral fullQuotient :: Int16
    index = fromIntegral (exposure-fullQuotient*100+100)
    tableValue = fromIntegral (B.index (sourceSlice image5 0xee9dc8 200) index)
    shifted value amount left
      | amount >= 32 = 0
      | left = value `shiftL` amount
      | otherwise = value `shiftR` amount

scaleProfileValues :: Word32 -> CalibrationValues -> CalibrationValues
scaleProfileValues percent = triplet . map (\value -> fromInteger (toInteger value*toInteger percent `div` 100)) . values

negativeProfileValues :: B.ByteString -> Int32 -> CalibrationValues -> CalibrationValues -> (CalibrationValues,Word8)
negativeProfileValues image5 exposure limits input
  | exposure >= 0 = (input,100)
  | otherwise = (scaleProfileValues applied input,if requested < minimumPercent then fromIntegral (requested*100 `div` minimumPercent) else 100)
  where
    ratios = zipWith (\limit value -> fromIntegral (if value == 0 then fromIntegral limit*100 else fromIntegral limit*100 `div` (fromIntegral value :: Word32)) :: Word16) (values limits) (values input)
    minimumPercent = fromIntegral (fromIntegral (maximum ratios) :: Word8) :: Word32
    requested = profileHistoryPercent image5 (fromIntegral (fromIntegral exposure :: Int16)) .&. 255
    applied = max minimumPercent requested

profileCalibrationInitial :: Address -> [Patch]
profileCalibrationInitial (Address base) = word 0x96ca50 base :
  [halves (base+key) [value] | (key,value) <-
    [(0x7930,2000),(0x7934,3000),(0x792a,100),(0x7928,200),(0x792c,300),
     (0x7924,120),(0x7922,240),(0x7926,360),(0x7938,50),(0x7936,50),
     (0x7cba,1000),(0x7cbe,2000),(0x7cb4,300),(0x7cb8,400),
     (0x7f04,512),(0x7f06,2048),(0x80d1,2048),(0x80d3,512),(0x7ed7,2048),(0x7ed9,512),
     (0xce81,100),(0xce7f,200),(0xce83,300)]] ++
  [Patch (Address (base+0x3e0f82)) (B.singleton 2),Patch (Address (base+0x792e)) (B.singleton 1)] ++
  [Patch (Address (base+0x4a24+index)) (B.singleton value) | (index,value) <- zip [0..] [0,10,20,30,40]]

data StreamParameters = StreamParameters
  { streamFormat :: Word32
  , streamSource :: Address
  , streamStride :: Word32
  , streamDestination :: Address
  , streamEntries :: Word32
  , streamCoefficients :: Address
  , streamAlternate :: Word32
  } deriving (Eq, Show)

streamParameterWords :: StreamParameters -> [Word32]
streamParameterWords request = [streamFormat request,source,streamStride request,destination,streamEntries request,coefficients,streamAlternate request]
  where
    Address source = streamSource request
    Address destination = streamDestination request
    Address coefficients = streamCoefficients request

data StreamFault = StreamRange | StreamAlignment | StreamNull | StreamNotEnabled | StreamPending deriving (Eq, Show)

streamProgramEffects :: StreamParameters -> [Word32] -> Either StreamFault [Patch]
streamProgramEffects request coefficients
  | source == 0 || streamStride request == 0 = Left StreamNull
  | source > 0x3ffffff8 || streamStride request > 0x1fff = Left StreamRange
  | streamFormat request > 1 || streamStride request .&. alignment /= 0 = Left StreamAlignment
  | length coefficients /= 128 = error "PFP requires 128 coefficient words"
  | any invalid coefficients = Left StreamAlignment
  | destination == 0 = Left StreamNull
  | destination > 0x3ffffff8 = Left StreamRange
  | destination .&. 7 /= 0 = Left StreamAlignment
  | otherwise = Right
      [wordsAt 0xfff1a000 [0x10000 .|. ((streamFormat request .&. 1) `shiftL` 8),source .&. 0x3fffffff,streamStride request],
       wordsAt 0xfff1a030 (map (.&. 0xf7efdfbf) coefficients),
       wordsAt 0xfff1a00c [destination .&. 0x3fffffff,extent,streamAlternate request .&. 0x13ff13ff]]
  where
    Address rawSource = streamSource request
    Address rawDestination = streamDestination request
    source = rawSource .&. 0xbfffffff
    destination = rawDestination .&. 0xbfffffff
    extent = if streamEntries request >= 0x40000 then 0x3ffe0 else streamEntries request
    alignment = if streamFormat request == 0 then 15 else 3
    invalid value = sum [value `shiftR` shift .&. 63 | shift <- [0,7,14,21]] `notElem` [0,32]

data StreamFaultScenario = StreamFaultScenario
  { streamFaultScenario :: StageScenario
  , streamExpectedFault :: StreamFault
  }

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name))

profileCalibrationPublications :: [StagePublication]
profileCalibrationPublications = [publication address name | (address,name) <-
  [(0xef4518,"calibrate_profile"),(0xef4460,"build_profile_factors"),(0xef4600,"clamp_profile"),
   (0xef4b1c,"clamp_geometry"),(0xef4bf4,"clamp_profile_output"),(0xef4d6c,"apply_negative_profile_gain"),
   (0xef4d18,"profile_apply_positive"),(0xef4d38,"profile_apply_difference"),(0xef3410,"profile_apply_converted_value"),
   (0xef73e4,"decode_profile_level"),(0xef74e4,"profile_converted_decade"),
   (0xef3f04,"profile_delta_factors"),(0xef4220,"profile_split_factors"),(0xef4274,"profile_selected_factors"),
   (0xef33b4,"profile_scale_percent"),(0xee9ab0,"profile_history_linearize"),
   (0x74677c,"profile_split_count"),(0x7467a4,"classify_curve_index"),(0x746834,"collapse_curve_index"),
   (0x74690c,"configuration_mapped_word"),(0x74695c,"transform_mapped_word"),(0x748818,"scaled_channel_curve"),
   (0x747354,"channel_curve_origin"),(0x747374,"channel_bias"),(0x747a08,"channel_biased_word"),
   (0x74782c,"interpolate_channel_curve"),(0x747a88,"channel_curve_value"),
   (0x741fa0,"round_tenths"),(0x69fcb8,"configuration_base"),(0x6a0010,"configuration_pointer"),
   (0xe63fd4,"process_calibration_overlay"),(0xe63fa0,"calibration_overlay_complete"),
   (0xda9ae8,"calibration_stream_initialize"),(0xda9b44,"calibration_stream_release"),(0xda9b68,"calibration_stream_finish_request"),
   (0xda9c38,"calibration_stream_stop"),(0xda9cd4,"calibration_stream_write_parameters"),(0xda9da0,"calibration_stream_complete"),
   (0x6995c0,"pfp_fault"),(0x6995e4,"pfp_initialize_control"),(0x699680,"pfp_release_control"),(0x699708,"pfp_wait_ready"),
   (0x6997b4,"pfp_begin"),(0x699898,"pfp_stop_control"),(0x6998d0,"pfp_set_source"),(0x699ab8,"pfp_set_span"),
   (0x699bec,"pfp_set_coefficients"),(0x699df4,"pfp_set_pair"),(0x699ee0,"pfp_set_interrupt_mask"),
   (0x699fe8,"pfp_read_status"),(0x69a044,"pfp_acknowledge")]]

profileCalibrationScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
profileCalibrationScenarios image5 _resolve = factorCases ++ clampCases ++ geometryCases ++ outputCases ++ gainCases ++ numericCases ++ mappedCases ++ channelCases ++ streamCases
  where
    sourceData = [Patch (Address 0xef5048) (sourceSlice image5 0xef5048 124),Patch (Address 0xee9dc8) (sourceSlice image5 0xee9dc8 200)]
    configuration = profileCalibrationInitial (Address 0x10000000)
    cfgHalf key value = halves (0x10000000+key) [value]
    cfgByte key value = Patch (Address (0x10000000+key)) (B.singleton value)
    input = 0x9a8000
    output = input+0x100
    metadata = 0x10100000
    make label entry name arguments initial final returned = StageScenario
      (StageCaseName ("profile-calibration-"++label)) (publication entry name) arguments (sourceData++configuration++initial) final returned [] []
    factors = ProfileFactors (FactorPair 2048 512) (FactorPair 512 2048) (FactorPair 2048 512)
    unity = ProfileFactors (FactorPair 1024 1024) (FactorPair 1024 1024) (FactorPair 1024 1024)
    request :: Word32 -> B.ByteString
    request mode = B.concat [B.concat [wordBytes value | value <- [0,10,1,1]],B.concat (map halfBytes [1000,2000,3000,0xaaaa]),wordBytes mode]
    wordBytes :: Word32 -> B.ByteString
    wordBytes value = B.pack [fromIntegral (value `shiftR` amount) | amount <- [0,8,16,24]]
    factorCases =
      [ let enabled=mode == 0 || mode == 2
            result=if enabled then [2048,512,1024,512,2048,0xa5a5,2048,512,0xa5a5,2048,512,1024]
              else [1024,1024,0xa5a5,1024,1024,0xa5a5,1024,1024,0xa5a5,1024,1024,1024]
        in make ("factor-bank-"++show mode) 0xef4460 "build_profile_factors" [input,output]
          [Patch (Address input) (request mode),halves output (replicate 12 0xa5a5)] [halves output result] Nothing
      | mode <- [0,1,2,3,0xffffffff]
      ] ++
      [ let active=mode == 0 || mode == 2
            selected=if active then factors else unity
            result=calibrateProfileValues selected (CalibrationValues 1000 2000 3000)
            fields=if active then [512,2048,2048,512,2048,512,1024] else replicate 7 1024
        in make ("calibrate-values-"++show mode++"-metadata-"++show diagnostic) 0xef4518 "calibrate_profile" [input,output,if diagnostic then metadata else 0]
          [Patch (Address input) (request mode),halves output [0xa5a5,0xa5a5,0xa5a5]]
          ([calibrationValuesPatch (Address output) result]++if diagnostic then [wordsAt (metadata+0x9e688) (fields++map fromIntegral (values result))] else []) Nothing
      | mode <- [0,1,2], diagnostic <- [False,True]
      ]
    fixture mode original expected high low =
      [make ("clamp-"++show mode++"-"++show original++"-flags-"++show nullable) 0xef4600 "clamp_profile" [mode,input,if nullable then 0 else output,10]
        [calibrationValuesPatch (Address input) original,wordsAt output [0xaaaaaaaa,0xbbbbbbbb]]
        ([calibrationValuesPatch (Address input) expected]++if nullable then [wordsAt output [0xaaaaaaaa,0xbbbbbbbb]] else [wordsAt output [high,low]]) Nothing
      | nullable <- [False,True]]
    clampCases = concat
      [fixture 0 (CalibrationValues 10 20 30) (CalibrationValues 50 200 300) 0 57,
       fixture 1 (CalibrationValues 10 20 30) (CalibrationValues 50 240 360) 0 456,
       fixture 0 (CalibrationValues 0 1 2) (CalibrationValues 50 200 300) 0 59,
       fixture 1 (CalibrationValues 0 1 2) (CalibrationValues 50 240 360) 0 472,
       fixture 0 (CalibrationValues 65535 5000 6000) (CalibrationValues 32767 2000 3000) 7 0,
       fixture 1 (CalibrationValues 65535 5000 6000) (CalibrationValues 32767 2000 3000) 56 0,
       fixture 2 (CalibrationValues 0 2000 0) (CalibrationValues 256 1000 400) 128 320,
       fixture 3 (CalibrationValues 0 40000 0) (CalibrationValues 256 32767 256) 1024 2560,
       fixture 86 (CalibrationValues 0 40000 0) (CalibrationValues 256 32767 256) 8 20,
       fixture 255 (CalibrationValues 0 40000 0) (CalibrationValues 256 32767 256) 0 0] ++
      [make "clamp-signed-product-overflow" 0xef4600 "clamp_profile" [0,input,output,65535]
        [calibrationValuesPatch (Address input) (CalibrationValues 10 65535 65535),cfgHalf 0x792a 65535,cfgHalf 0x7928 0,cfgHalf 0x792c 0,cfgHalf 0x7930 65535,cfgHalf 0x7934 65535,cfgHalf 0x7938 10]
        [calibrationValuesPatch (Address input) (CalibrationValues 32767 52429 52429),wordsAt output [1,1]] Nothing]
    geometryCases =
      [let applied=max 10 rate
           parameters=ClampParameters (if exposure < 100 then CalibrationValues 120 240 360 else CalibrationValues 100 200 300) (CalibrationValues 32767 2000 3000) (fromIntegral (applied*5))
           result=clampProfileValues (if exposure < 100 then SecondaryClamp else PrimaryClamp) parameters (CalibrationValues 10 20 30)
       in make ("geometry-"++show exposure++"-"++show rate++"-null-"++show nullable) 0xef4b1c "clamp_geometry"
         [0xdeadbeef,input,rate,output,if nullable then 0 else output+16,fromIntegral (exposure :: Int32),metadata]
         [calibrationValuesPatch (Address input) (CalibrationValues 10 20 30),Patch (Address 0) (B.singleton 0xe5)]
         ([calibrationValuesPatch (Address output) (clampedValues result),wordsAt (metadata+0x9e6b0) [applied,if nullable then 0xe5 else applied .&. 255,upperFlags result,lowerFlags result]]++
          if nullable then [] else [Patch (Address (output+16)) (B.singleton (fromIntegral applied))]) Nothing
      | exposure <- [50,100], rate <- [0,10,20], nullable <- [False,True]]
    outputCases =
      [make ("output-clamp-alias-"++show alias) 0xef4bf4 "clamp_profile_output" [input,if alias then input else output,metadata]
        [calibrationValuesPatch (Address input) (CalibrationValues 0 40000 0)]
        [calibrationValuesPatch (Address input) (CalibrationValues 256 32767 256),calibrationValuesPatch (Address (if alias then input else output)) (CalibrationValues 256 32767 256),wordsAt (metadata+0xa0428) [1024,2560,256,32767,256]] Nothing
      | alias <- [False,True]]
    gainCases =
      [let original=CalibrationValues first second third
           (result,percent)=negativeProfileValues image5 exposure (CalibrationValues 100 200 300) original
       in make ("negative-gain-"++show exposure++"-"++show original) 0xef4d6c "apply_negative_profile_gain" [fromIntegral exposure,input,output]
         [calibrationValuesPatch (Address input) original,Patch (Address output) (B.singleton 0xa5)]
         [calibrationValuesPatch (Address input) result,Patch (Address output) (B.singleton percent)] Nothing
      | exposure <- [-65536,-32768,-200,-100,-66,-33,-1,0,1], (first,second,third) <- [(0,0,0),(100,200,300),(1000,2000,3000),(65535,32768,16384)]] ++
      [make ("positive-gain-"++show exposure) 0xef4d18 "profile_apply_positive" [fromIntegral exposure,input]
        [calibrationValuesPatch (Address input) (CalibrationValues 1000 2000 3000)]
        [calibrationValuesPatch (Address input) (if exposure <= 0 then CalibrationValues 1000 2000 3000 else scaleProfileValues (profileHistoryPercent image5 exposure) (CalibrationValues 1000 2000 3000))] Nothing
      | exposure <- [-100,0,1,33,100,200,300,3200]] ++
      [let decade value=if value >= 2 && value <= 5 then (value-1)*100 else 0
           exposure=fromIntegral (decade first-decade second) :: Int32
       in make ("difference-gain-"++show first++"-"++show second) 0xef4d38 "profile_apply_difference" [first,second,input]
         [calibrationValuesPatch (Address input) (CalibrationValues 1000 2000 3000)]
         [calibrationValuesPatch (Address input) (scaleProfileValues (profileHistoryPercent image5 exposure) (CalibrationValues 1000 2000 3000))] Nothing
      | first <- [0,1,2,3,4,5,6,0xffffffff], second <- [1,2,5]]
    numericCases =
      [make ("decimal-round-"++show value) 0x741fa0 "round_tenths" [fromIntegral value] [] [] (Just (fromIntegral (roundAway value)))
      | value <- [-2147483648,-105,-104,-15,-14,-5,-4,0,4,5,14,15,104,105,2147483647]] ++
      [make ("configuration-pointer-"++show key) 0x6a0010 "configuration_pointer" [key] [] [] (Just (0x10000000+key))
      | key <- [0,0x792e,0x3e0317,0x3f2400,0xffffffff]] ++
      [make ("history-"++show value) 0xee9ab0 "profile_history_linearize" [fromIntegral value] [] [] (Just (profileHistoryPercent image5 value))
      | value <- [-32768,-1000,-200,-100,-99,-1,0,1,33,99,100,200,3100,3200,32767]] ++
      [let count=if rawCount >= 1 && rawCount <= 31 then fromIntegral rawCount else 1
           classification=if index < count then 0 else if index >= 31 && index < count+31 then 1 else 2
           result=if collapsed then if classification == 0 then index else if classification == 1 then index-31 else 0 else classification
       in make ("curve-index-"++show rawCount++"-"++show index++"-collapsed-"++show collapsed)
         (if collapsed then 0x746834 else 0x7467a4) (if collapsed then "collapse_curve_index" else "classify_curve_index")
         [index] [cfgByte 0x3e0f82 rawCount] [] (Just result)
      | rawCount <- [0,1,2,31,32,255], index <- [0,1,2,30,31,32,33,61,62,0xffffffff], collapsed <- [False,True]]
    roundAway value = let (quotient,remainder)=value `quotRem` 10 in quotient + if remainder >= 5 then 1 else if remainder <= -5 then -1 else 0 :: Int32
    mappedCases =
      [let exponent=fromIntegral (mapped :: Int16)/2000 :: Float
           tenths=truncate ((2**exponent)*1000) :: Word32
           expected=fromIntegral (roundAway (fromIntegral tenths)) :: Word16
       in make ("mapped-power-"++show mapped) 0x74695c "transform_mapped_word" [0,0]
         [cfgHalf 0x3f2400 (fromIntegral mapped)] [] (Just (fromIntegral expected))
      | mapped <- [-32768,-32000,-4000,-2000,-1000,-1,0,1,33,999,1000,1001,2000,3000,4000,16000,32000,32767]]
    channelConfiguration scale origin spacing = [cfgByte 0x3e0f80 1,cfgByte 0x1fc 0,cfgHalf 0x3e2640 scale,cfgHalf 0x3e0317 17,cfgHalf 0x3e0572 (fromIntegral origin),cfgHalf 0x3e2643 spacing] ++
      [cfgHalf (0x3f1b00+index*2) (100+fromIntegral index*100) | index <- [0..10]]
    channelCases =
      [let sample=fromIntegral (origin :: Int16) :: Int32
           expected=if sample <= 0 then 100 else if sample >= 1900 then 1100 else if sample < 100 then 100+sample else 200+(sample-100) `quot` 2
       in make ("channel-curve-"++show origin) 0x748818 "scaled_channel_curve" [0]
         (channelConfiguration 1000 origin 100) [] (Just (fromIntegral expected))
      | origin <- [-32768,-1,0,1,50,99,100,101,200,299,300,500,1899,1900,32767]] ++
      [make ("channel-rejection-"++show channel) 0x748818 "scaled_channel_curve" [channel]
        (channelConfiguration 1000 100 100) [] (Just 65535) | channel <- [1,100,0x80000000,0xffffffff]] ++
      [make "channel-sentinel-scale" 0x748818 "scaled_channel_curve" [0] (channelConfiguration 65535 100 100) [] (Just 65535),
       make "channel-sentinel-value" 0x748818 "scaled_channel_curve" [0] (channelConfiguration 1000 0 100++[cfgHalf 0x3f1b00 65535]) [] (Just 65535),
       make "channel-zero-scale" 0x748818 "scaled_channel_curve" [0] (channelConfiguration 0 0 100) [] (Just 0),
       make "channel-signed-scale-product" 0x748818 "scaled_channel_curve" [0] (channelConfiguration 65000 0 100++[cfgHalf 0x3f1b00 65000]) [] (Just 61106),
       make "channel-repeated-knots" 0x748818 "scaled_channel_curve" [0] (channelConfiguration 1000 1 0) [] (Just 1100)] ++
      [make ("channel-bias-"++show signed++"-bank-"++show selector++"-scale-"++show scale) 0x747374 "channel_bias" [0,selector]
        (channelConfiguration scale 0 100++[cfgByte 0x1fc signed,cfgHalf 0x3e03e0 23,cfgHalf 0x3f1b00 first]) [] (Just (fromIntegral (expected :: Int32)))
      | (signed,selector,scale,first,expected) <- [(255,0,1000,100,18),(255,254,1000,100,24),(1,0,1000,100,16),(127,0,1000,100,-110),(128,0,1000,100,145),(127,0,0,100,17),(127,0,1000,0,17),(127,0,65535,65535,114),(128,0,65535,65535,-81)]]
    driverInitial = [wordsAt 0x89f75c [1,0,0,0]]
    weights = [0x08102040 .|. ((index .&. 15) `shiftL` 28) .|. (if even index then 32 else 16 .|. (16 `shiftL` 7)) | index <- [0..127]]
    streamRequest entries = StreamParameters 0 (Address 0x50000000) 32 (Address 0x10100000) entries (Address input) 0xffffffff
    streamCases =
      [make ("pfp-mask-"++show mask) 0x699ee0 "pfp_set_interrupt_mask" [input]
        (driverInitial++[word input mask,word 0xfff1a020 0xaaaaaaaa]) [word 0xfff1a020 (mask .&. 31)] Nothing
      | mask <- [0,1,16,31,0xffffffe0,0xffffffff]] ++
      [make ("pfp-source-return-"++show format) 0x6998d0 "pfp_set_source" [input]
        (driverInitial++[wordsAt input [format,0x50000000,32],word 0xfff1a000 0xffffffff])
        [wordsAt 0xfff1a000 [0x10000 .|. (format `shiftL` 8),0x10000000,32]] (Just input)
      | format <- [0,1]] ++
      [make "pfp-pair-packed-bits" 0x699df4 "pfp_set_pair" [input] (driverInitial++[word input 0xffffffff]) [word 0xfff1a014 0x13ff13ff] Nothing] ++
      [make "pfp-coefficient-lanes" 0x699bec "pfp_set_coefficients" [input] (driverInitial++[wordsAt input weights]) [wordsAt 0xfff1a030 (map (.&. 0xf7efdfbf) weights)] Nothing] ++
      [let request=streamRequest entries
           programmed=case streamProgramEffects request weights of Right result -> result; Left _ -> error "source-backed PFP fixture is invalid"
           remaining=if entries >= 0x40000 then entries-0x3ffe0 else 0
           next=if remaining == 0 then 0 else 0x1020ff78
       in make ("pfp-program-entries-"++show entries) 0xda9cd4 "calibration_stream_write_parameters" [input+0x400]
         (driverInitial++[wordsAt input weights,wordsAt (input+0x400) (streamParameterWords request),wordsAt 0x14ce6f8 (1:replicate 12 0)])
         (programmed++[wordsAt 0x14ce6f8 ([2,0]++streamParameterWords request++[0,0,remaining,next])]) (Just 0)
      | entries <- [0,1,0x3ffff,0x40000,0x40001]] ++
      [make "pfp-completion-no-callback" 0xda9da0 "calibration_stream_complete" []
        [wordsAt 0x89f75c [1,1,0,0],wordsAt 0x14ce6f8 [3,0,0,0,0,0,1,0,0,0,1,0,0],word 0xfff1a018 1]
        [word 0x14ce6f8 4,word 0xfff1a01c 31,wordsAt 0x89f75c [1,1,0,0]] Nothing] ++
      [let request=(streamRequest 33) {streamDestination=Address 0x1020ff78}
       in (make "pfp-completion-republishes-tail" 0xda9da0 "calibration_stream_complete" []
          [wordsAt 0x89f75c [1,1,0,0],wordsAt input weights,
           wordsAt 0x14ce6f8 ([3,1]++streamParameterWords (streamRequest 0x40001)++[0,1,33,0x1020ff78]),word 0xfff1a018 1,word 0xfff1a020 31]
          [wordsAt 0xfff1a000 [0x10001,0x10000000,32,0x1020ff78,33,0x13ff13ff],
           wordsAt 0xfff1a030 (map (.&. 0xf7efdfbf) weights),word 0xfff1a01c 31,word 0xfff1a020 31,wordsAt 0x89f75c [1,1,0,0],
           wordsAt 0x14ce6f8 ([2,1]++streamParameterWords request++[0,1,0,0])] Nothing)
          {stageCalls=[call 0x42634 [0,0] 0]}]

profileCalibrationFaultScenarios :: [StreamFaultScenario]
profileCalibrationFaultScenarios =
  [fault "disabled-mask" 0x699ee0 "pfp_set_interrupt_mask" [input] [wordsAt 0x89f75c [0,0,0,0],word input 31] StreamNotEnabled,
   fault "pending-mask" 0x699ee0 "pfp_set_interrupt_mask" [input] [wordsAt 0x89f75c [1,1,0,0],word input 31] StreamPending,
   fault "null-source-reads-low-word-four" 0x6998d0 "pfp_set_source" [0] [wordsAt 0x89f75c [1,0,0,0],word 4 0xe59ff018] StreamNull,
   fault "null-span-reads-low-word-zero" 0x699ab8 "pfp_set_span" [0] [wordsAt 0x89f75c [1,0,0,0],word 0 0xe59ff018] StreamNull,
   fault "unaligned-span" 0x699ab8 "pfp_set_span" [input] [wordsAt 0x89f75c [1,0,0,0],wordsAt input [0x10000001,1]] StreamAlignment,
   fault "out-of-range-span" 0x699ab8 "pfp_set_span" [input] [wordsAt 0x89f75c [1,0,0,0],wordsAt input [0x10000000,0x40000]] StreamRange,
   fault "invalid-coefficient-sum" 0x699bec "pfp_set_coefficients" [input] [wordsAt 0x89f75c [1,0,0,0],word input 1] StreamAlignment]
  where
    input=0x9a8000
    code StreamRange=1
    code StreamAlignment=2
    code StreamNull=3
    code StreamNotEnabled=4
    code StreamPending=5
    fault label address name arguments initial expected = StreamFaultScenario
      (StageScenario (StageCaseName ("profile-calibration-fault-"++label)) (publication address name) arguments initial [word 0x89f768 (code expected)] Nothing [] []) expected
