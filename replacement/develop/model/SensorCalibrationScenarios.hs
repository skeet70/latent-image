module SensorCalibrationScenarios
  ( sensorCalibrationPublications
  , sensorCalibrationScenarios
  , CalibrationSelection (..)
  , selectCalibration
  , calibrationSelectionEffects
  ) where

import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word (Word8, Word16, Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), state, word, wordsAt, wordBytes)
import qualified BuffersScenarios as Buffers

data CalibrationSelection = CalibrationSelection
  { calibrationModeBin :: Word8
  , calibrationChannelBin :: Word8
  , calibrationTemperatureBin :: Word8
  } deriving (Eq, Show)

selectCalibration :: (Word32 -> Word8) -> (Word32 -> Word16) -> Word32 -> Word32 -> Int32 -> CalibrationSelection
selectCalibration readByte readHalf mode channel temperature = CalibrationSelection selectedMode selectedChannel selectedTemperature
 where
  selectedMode = fromIntegral (length (takeWhile ((< mode) . fromIntegral . readByte) [0x3e3100..0x3e310d]))
  selectedChannel = fromIntegral (length (takeWhile ((< channel) . fromIntegral . readByte) [0x3e310e..0x3e3112]))
  key = 0x3e3120 + fromIntegral selectedMode * 4
  selectedTemperature
    | temperature <= fromIntegral (readHalf key) = 0
    | temperature <= fromIntegral (readHalf (key+2)) = 1
    | otherwise = 2

calibrationSelectionEffects :: Word32 -> Word32 -> Word32 -> CalibrationSelection -> [Patch]
calibrationSelectionEffects mode channel temperature selected =
  [byteAt mode (calibrationModeBin selected),byteAt channel (calibrationChannelBin selected),byteAt temperature (calibrationTemperatureBin selected)]

entries :: [(Word32,String)]
entries =
  [(0xe563e0,"primary_geometry"),(0xe565dc,"secondary_geometry"),(0xe55850,"cached_primary_geometry"),(0xe56530,"calibration_unchanged")
  ,(0xdaa540,"generate_calibration"),(0xda0758,"extrapolate_cell"),(0xda0818,"sample_calibration"),(0xda08a8,"interpolate_calibration")
  ,(0xda09b4,"copy_calibration_cell"),(0xda0c58,"select_calibration"),(0xda0ed4,"correction_key"),(0xda0f78,"select_interpolated_calibration")
  ,(0xda1120,"exposure_is_zero"),(0xda117c,"calibration_axes"),(0xda12a8,"calibration_regions"),(0xda1438,"read_axis_corrections")
  ,(0xda1470,"read_channel_corrections"),(0xda14d4,"extend_calibration_border"),(0xda1790,"calibration_cell")]

sensorCalibrationPublications :: [StagePublication]
sensorCalibrationPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_sensor_"++name)) | (address,name) <- entries]
publication :: Word32 -> StagePublication
publication address = case filter ((== Address address) . stageStockEntry) sensorCalibrationPublications of
  first:_ -> first
  [] -> error "unknown sensor calibration publication"

byteAt :: Word32 -> Word8 -> Patch
byteAt address value = Patch (Address address) (B.singleton value)
halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
halvesAt :: Word32 -> [Word16] -> Patch
halvesAt address = Patch (Address address) . B.concat . map halfBytes
configurationHalf :: Word32 -> Word16 -> [(Word32,Word8)]
configurationHalf key value = [(key,fromIntegral value),(key+1,fromIntegral (value `shiftR` 8))]

configuration, output, firstExposure, secondExposure, cached, means, differences, regions, table :: Word32
configuration = 0x02400000
output = 0x02010000
firstExposure = 0x02001000
secondExposure = 0x02001010
cached = 0x02002000
means = 0x02003000
differences = means+32
regions = means+64
table = 0x02020000
compactBanks, fullBanks, cacheState :: Word32
compactBanks = 0x014cf9f0
fullBanks = 0x014d11f0
cacheState = 0x01568318
background :: Word32
background = 0xc0300801

metadata :: [(Word32,Word8)]
metadata = [(0x31a5,5),(0x31a6,7),(0x3201,1)] ++ configurationHalf 0x31a7 17 ++ configurationHalf 0x31a9 19 ++ configurationHalf 0x3604 32
common :: [Patch]
common =
  [Patch (Address configuration) (B.replicate 0x40000 0),Patch (Address (configuration+0x3e0000)) (B.replicate 0x9000 0),Patch (Address (configuration+0x404e00)) (B.replicate 0x5000 0)
  ,Patch (Address output) (B.replicate 0x5500 0xa5),word (state+0x78) 0,word (state+0x2728) 0,word (state+0x3240) 0,word (state+0x498) 64
  ,byteAt (state+0x6c0) 1,word (state+0x660) 0,halvesAt (state+0x2264) [0],wordsAt cacheState [1,0x11111111,0x22222222,0x33333333]
  ,Patch (Address firstExposure) (B.pack [1,2,3,4]),Patch (Address secondExposure) (B.pack [255,2,253,4])
  ,wordsAt compactBanks (replicate (6*256) background),wordsAt fullBanks (replicate (14*256) background)] ++ Buffers.configurationInitial configuration metadata

make :: String -> Word32 -> [Word32] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario
make name entry arguments initial final result = StageScenario (StageCaseName ("sensor-calibration-"++name)) (publication entry) arguments (common++initial) final result [] []
config :: [(Word32,Word8)] -> [Patch]
config = Buffers.configurationInitial configuration
poisonPrimary :: Word32
poisonPrimary = 0xa5a5a5a5

sensorCalibrationScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
sensorCalibrationScenarios _image _resolve = selectionCases ++ correctionCases ++ interpolationCases ++ exposureCases ++ axisCases ++ regionCases ++ cellCases ++ borderCases ++ cacheCases ++ primaryCases ++ secondaryCases ++ generatorCases

selectionCases :: [StageScenario]
selectionCases =
  [make ("selection-"++show (mode,channel,temperature)) 0xda0c58 [mode,channel,fromIntegral temperature,output,output+1,output+2]
    (config thresholds) (calibrationSelectionEffects output (output+1) (output+2) expected) Nothing
  | (mode,channel,temperature,expected) <- ([(0,0,-1,CalibrationSelection 0 0 0),(10,20,100,CalibrationSelection 0 0 0),(11,21,101,CalibrationSelection 1 1 1),(140,100,200,CalibrationSelection 13 4 1),(141,101,201,CalibrationSelection 14 5 2)] :: [(Word32,Word32,Int32,CalibrationSelection)])] ++
  [make "selection-aliased-bins" 0xda0c58 [11,0,150,output,output,output+1]
    (config (thresholds++configurationHalf 0x3e3120 200)) [byteAt output 0,byteAt (output+1) 0] Nothing]
 where
  thresholds = [(0x3e3100+index,fromIntegral ((index+1)*10)) | index <- [0..13]] ++ [(0x3e310e+index,fromIntegral ((index+1)*20)) | index <- [0..4]] ++ concat [configurationHalf (0x3e3120+index*4) 100 ++ configurationHalf (0x3e3122+index*4) 200 | index <- [0..14]]

correctionCases :: [StageScenario]
correctionCases = [make ("correction-key-"++show step) 0xda0ed4 [fromIntegral step] [] [] (Just key) | (step,key) <- ([(-6,0),(-5,0x4653),(-4,0x461b),(-3,0x45e3),(-2,0x45ab),(-1,0x4573),(0,0),(1,0x453b),(2,0x4503),(3,0x44cb),(4,0x4493),(5,0x445b),(6,0)] :: [(Int32,Word32)])]

interpolationCases :: [StageScenario]
interpolationCases =
  [make ("sample-"++show (alternate,level)) 0xda0818 [alternate,0,0,level,1] (config samples) [] (Just (fromIntegral expected)) | (alternate,level,expected) <- ([(0,0,-250),(0,1,26),(0,2,300),(1,0,-400),(1,1,-200),(1,2,0)] :: [(Word32,Word32,Int32)])] ++
  [make ("interpolation-"++show (alternate,position)) 0xda08a8 [alternate,0,0,position,1] (config samples) [] (Just (fromIntegral expected)) | (alternate,position,expected) <- ([(0,0,-250),(0,5,-112),(0,10,26),(0,15,163),(0,20,300),(1,5,-300),(1,15,-100)] :: [(Word32,Word32,Int32)])] ++
  [make ("interpolated-selection-"++show temperature) 0xda0f78 [5,5,temperature,output,output+1,output+2]
    (config ([(0x3e4300+index,fromIntegral (index*10)) | index <- [0..14]]++[(0x3e430f+index,fromIntegral (index*10)) | index <- [0..14]]++concat [configurationHalf (0x3e4324+index*2) value | (index,value) <- zip [0..] [100,200,300]]))
    [Patch (Address output) (B.pack [1,1,expected])] Nothing | (temperature,expected) <- [(0,0),(100,0),(150,5),(200,10),(250,15),(300,20)]]
 where samples = [(base+level*0x708,value) | base <- [0x3e5770,0x3e6c90],(level,value) <- [(0,0),(1,128),(2,255)]]

exposureCases :: [StageScenario]
exposureCases = [make ("exposure-zero-"++show values) 0xda1120 [firstExposure] [Patch (Address firstExposure) (B.pack values)] [] (Just expected) | (values,expected) <- [([0,0,0,0],1),([1,0,0,0],0),([0,1,0,0],0),([0,0,1,0],0),([0,0,0,1],0),([0,0,0,255],0)]] ++ [make "null-exposure" 0xda1120 [0] [] [] (Just 1)]

axisCases :: [StageScenario]
axisCases =
  [make "calibration-axes-negative-rounding" 0xda117c [0,0,0,means,differences]
    (config [(0x3e0009,0)]) [halvesAt means (replicate 8 (fromIntegral (-33 :: Int))),halvesAt differences (replicate 8 15)] Nothing
  ,make "calibration-axes-disabled" 0xda117c [0,0,0,means,differences]
    (config [(0x3e0009,2)]++[Patch (Address means) (B.replicate 16 0xa5),Patch (Address differences) (B.replicate 16 0xa5)]) [halvesAt means (replicate 8 0),halvesAt differences (replicate 8 0)] Nothing
  ,make "axis-corrections" 0xda1438 [0x1ed80,7,2,output]
    (config [(0x1ee87,246),(0x1eec7,12)]) [Patch (Address output) (B.pack [246,0,12])] Nothing
  ,make "channel-corrections" 0xda1470 [0x22d80,9,2,output]
    (config [(0x23c89+index*64,fromIntegral (index+120)) | index <- [0..29]]) [Patch (Address output) (B.pack [120..149])] Nothing]

regionCases :: [StageScenario]
regionCases =
  [make ("calibration-regions-"++show alternate) 0xda12a8 [means,differences,regions,alternate]
    ([halvesAt means (replicate 8 0xffff),halvesAt differences (replicate 8 1)] ++ config ([(0x3e0009,0)]++[(base+(index*30)*20+19,fromIntegral (index+1)) | index <- [0..7]])) [Patch (Address regions) (B.pack [1..8])] Nothing
  | (alternate,base) <- [(0,0x1c800),(1,0x1dac0)]] ++
  [make "calibration-regions-disabled" 0xda12a8 [means,differences,regions,0] (config [(0x3e0009,1)]++[Patch (Address regions) (B.replicate 8 0xa5)]) [Patch (Address regions) (B.replicate 8 0)] Nothing]

cellCases :: [StageScenario]
cellCases =
  [make "cell-extrapolation-wrapping" 0xda0758 [0x44,0x55,0x33,3,2,table]
    [halvesAt (table+0x44*60) [100,400,600],halvesAt (table+0x55*60) [200,50,650],halvesAt (table+0x44*60+36) [0,65535],halvesAt (table+0x55*60+36) [1,0]]
    [halvesAt (table+0x33*60) [0,750,550],halvesAt (table+0x33*60+36) [65535,65534]] Nothing
  ,make "cell-copy-byte-counts" 0xda09b4 [0x44,0x33,5,3,table]
    [Patch (Address (table+0x44*60)) (B.pack [1..8]),Patch (Address (table+0x44*60+36)) (B.pack [21..28]),Patch (Address (table+0x33*60)) (B.replicate 60 0xa5)]
    [Patch (Address (table+0x33*60)) (B.pack [1,2,3,4,5,0xa5]),Patch (Address (table+0x33*60+36)) (B.pack [21,22,23,0xa5])] Nothing] ++
  [make ("cell-map-"++show index) 0xda1790 [index] [] [] (Just expected) | (index,expected) <- [(0,0x44),(7,0x4b),(8,0x54),(63,0xbb)]]

borderCases :: [StageScenario]
borderCases = [make ("border-mode-"++show mode) 0xda14d4 [mode,3,table]
  [Patch (Address table) (B.replicate (256*60) 0xa5),halvesAt (table+0x44*60) (replicate 18 300),halvesAt (table+0x55*60) (replicate 18 200),halvesAt (table+0x44*60+36) (replicate 12 400),halvesAt (table+0x55*60+36) (replicate 12 250)]
  [halvesAt (table+0x33*60) (replicate 18 primary),halvesAt (table+0x33*60+36) (replicate 12 secondary)] Nothing
  | (mode,primary,secondary) <- [(0,400,550),(1,300,400),(2,0xa5a5,0xa5a5)]]

cacheCases :: [StageScenario]
cacheCases =
  [make ("cache-"++name) 0xe56530 [enabled,kind,mode,channel] [wordsAt cacheState initial]
    [wordsAt cacheState final] (Just result)
  | (name,enabled,kind,mode,channel,initial,final,result) <-
    [("reset",0,2,3,4,[0,7,8,9],[1,7,8,9],0),("invalidated",1,2,3,4,[1,2,3,4],[0,2,3,4],0),("hit",1,2,3,4,[0,2,3,4],[0,2,3,4],1),("kind-change",1,2,3,4,[0,8,3,4],[0,2,3,4],0),("mode-change",1,2,3,4,[0,2,8,4],[0,2,3,4],0),("channel-change",1,2,3,4,[0,2,3,8],[0,2,3,4],0)]]

primaryCases :: [StageScenario]
primaryCases =
  [make "primary-configured" 0xe563e0 [state,output,7,2,0]
    (config ([(0xc0,16),(0x3e0009,0)]++configurationHalf 0x3e315c 1++curveConfiguration 0x404e00 100))
    [wordsAt output ([64,0,5,7,32]++[100..127]++[poisonPrimary])] (Just 1)
  ,make "primary-cached-correction" 0xe563e0 [state,output,7,2,0]
    ([word (state+0x3240) 1,byteAt (state+0x3250) 254]++config ([(0xc0,16),(0x3e0009,1)]++curveConfiguration 0x45ab 200))
    [wordsAt output ([64,0,5,7,32]++[200..227]++[poisonPrimary])] (Just 1)] ++
  [make ("primary-refusal-"++name) 0xe563e0 [state,output,7,2,0] initial [Patch (Address output) (B.replicate 136 0)] (Just 0)
  | (name,initial) <- [("blocked",[word (state+0x2728) 1]),("feature-disabled",config [(0xc0,0)]),("zero-correction",[word (state+0x3240) 1,byteAt (state+0x3250) 0]++config [(0xc0,16),(0x3e0009,1)]),("mask-disabled",config [(0xc0,16),(0x3e0009,0)])]] ++
  [make ("primary-capture-cache-"++show flag) 0xe563e0 [state,output,7,2,0]
    ([word (state+0x78) 11,word (state+0x1884) cached,byteAt cached flag,halvesAt (cached+2) [11,12],Patch (Address (cached+6)) (B.pack [13,14]),halvesAt (cached+8) (15:[100..127])])
    [wordsAt output ([11,12,13,14,15]++[100..127]++[0])] (Just (if flag==0 then 0 else 1)) | flag <- [0,2]] ++
  [make "cached-primary-byte-result" 0xe55850 [state,output]
    [word (state+0x1884) cached,byteAt cached 172,halvesAt (cached+2) [11,12],Patch (Address (cached+6)) (B.pack [13,14]),halvesAt (cached+8) (15:[100..127])]
    [wordsAt output ([11,12,13,14,15]++[100..127]++[0])] (Just 172)]

curveConfiguration :: Word32 -> Word16 -> [(Word32,Word8)]
curveConfiguration base first = concat [configurationHalf (base+index*2) (first+fromIntegral index) | index <- [0..27]]

data Witness = WeightedExposure | ConfiguredCorrection | SaturatingCorrection | IdentityCorrection | OversizedExponent deriving (Eq, Show)
usesSpecifiedExposures :: Witness -> Bool
usesSpecifiedExposures witness = witness == WeightedExposure || witness == OversizedExponent
data CoefficientTriplet = CoefficientTriplet Word16 Word16 Word16 deriving (Eq, Show)
tripletComponent :: CoefficientTriplet -> Word32 -> Word16
tripletComponent (CoefficientTriplet first second third) index = case index of
  0 -> first
  1 -> second
  2 -> third
  _ -> error "triplet component outside ABI"
packedTriplet :: CoefficientTriplet -> Word32
packedTriplet (CoefficientTriplet first second third) = 0xc0000000 .|. fromIntegral first .|. (fromIntegral second `shiftL` 10) .|. (fromIntegral third `shiftL` 20)
fullCoefficients :: Witness -> [CoefficientTriplet]
fullCoefficients witness = case witness of
  WeightedExposure -> [t 256 416 256,t 224 256 224,t 256 416 256,t 256 224 256,t 416 256 416,t 256 224 256]++replicate 8 (t 256 256 256)
  ConfiguredCorrection -> [t 272 280 272,t 263 272 263,t 272 280 272,t 272 263 272,t 280 272 280,t 272 263 272]++replicate 8 (t 240 240 240)
  SaturatingCorrection -> [t 256 1023 256,t 0 256 0,t 256 1023 256,t 256 0 256,t 1023 256 1023,t 256 0 256]++replicate 8 (t 256 256 256)
  IdentityCorrection -> replicate 14 (t 256 256 256)
  OversizedExponent -> replicate 6 (t 0 0 0)++replicate 8 (t 256 256 256)
 where t=CoefficientTriplet
activeCell :: Word32 -> Bool
activeCell cell = cell `div` 16 >= 3 && cell `div` 16 <= 12 && cell .&. 15 >= 3 && cell .&. 15 <= 12
bankEffects :: Word32 -> [CoefficientTriplet] -> [Patch]
bankEffects base coefficients = [wordsAt (base+index*1024) [if activeCell cell then packedTriplet values else background | cell <- [0..255]] | (index,values) <- zip [0..] coefficients]

witnessConfiguration :: Witness -> [(Word32,Word8)]
witnessConfiguration witness = case witness of
  WeightedExposure -> [(0xc0,2),(0x3e0009,1),(0x3004,4)]++[(0x3340+index,9) | index <- [0..63]]++[(0x3380+index,247) | index <- [0..63]]
  ConfiguredCorrection -> lookupConfiguration 8 16 240
  SaturatingCorrection -> lookupConfiguration 0 0 0
  IdentityCorrection -> [(0xc0,2),(0x3e0009,1),(0x3004,8),(0x3005,8)]++[(0x31d80+index,1) | index <- [0..63]]
  OversizedExponent -> witnessConfiguration WeightedExposure++[(0x3004,32)]
 where
  lookupConfiguration exponentBits primary secondary = [(0xc0,2),(0x3e0009,1),(0x3004,exponentBits),(0x3005,exponentBits)]++[(0x31d80+index,1) | index <- [0..63]]++[(0x1ed80+index,8) | index <- [0..63]]++[(0x1edc0+index,248) | index <- [0..63]]++[(0x22d80+component*64+cell,primary) | component <- [0..17],cell <- [0..63]]++[(0x23200+component*64+cell,secondary) | component <- [0..11],cell <- [0..63]]

recordBytes :: Bool -> Bool -> [Word16] -> B.ByteString
recordBytes primary full curve = B.pack [if primary then 1 else 0,1]<>B.concat (map halfBytes [0,0,0,0])<>B.pack [if full then 5 else 0,if full then 7 else 0]<>B.concat (map halfBytes [if primary then 32 else 0,if full then 17 else 0,if full then 19 else 0])<>B.pack [if full then 1 else 0,0]<>B.concat (map halfBytes curve)<>
  (if full then B.concat [wordBytes (fullBanks+bank*1024) | bank <- [0..13]] else B.replicate 56 0)<>B.replicate 40 0<>
  (if full then B.replicate 24 0 else B.concat [wordBytes (compactBanks+bank*1024) | bank <- [0..5]])

secondaryPlanarEffects :: Witness -> [Patch]
secondaryPlanarEffects witness =
  [halvesAt (output+32+bank*1536+component*512) [if activeCell cell then tripletComponent coefficients component else fromIntegral (component+1) | cell <- [0..255]] | (bank,coefficients) <- zip [0..] (take 6 values),component <- [0..2]] ++
  [halvesAt (output+offset+bank*512) [if activeCell cell then tripletComponent coefficients component else fromIntegral (component+1) | cell <- [0..255]] | (bank,coefficients) <- zip [0..] (drop 6 values),(component,offset) <- [(0,0x4420),(1,0x3420),(2,0x2420)]]
 where values=fullCoefficients witness

secondaryCases :: [StageScenario]
secondaryCases =
  [make ("secondary-"++show (witness,hit)) 0xe565dc [state,output,7,2,0]
    ([word (state+0x3240) 1,Patch (Address (state+0x3248)) (if usesSpecifiedExposures witness then B.pack [1,255,2,2,4,4,3,253] else B.replicate 8 0),wordsAt cacheState [if hit then 0 else 1,2,0,0]]++config (witnessConfiguration witness))
    ([wordsAt output [1,1,64,0,17,19,5,7],word (output+0x5420) (if hit then 1 else 0),wordsAt cacheState [0,2,0,0]]++bankEffects fullBanks (fullCoefficients witness)++secondaryPlanarEffects witness) (Just 1)
  | witness <- [WeightedExposure,ConfiguredCorrection,SaturatingCorrection,IdentityCorrection,OversizedExponent],hit <- [False,True]] ++
  [make "secondary-configured-selector-path" 0xe565dc [state,output,7,2,0]
    (config (witnessConfiguration ConfiguredCorrection++[(0x3e0009,0)]))
    ([wordsAt output [1,1,64,0,17,19,5,7],word (output+0x5420) 0,wordsAt cacheState [0,2,0,0]]++bankEffects fullBanks (fullCoefficients ConfiguredCorrection)++secondaryPlanarEffects ConfiguredCorrection) (Just 1)
  ,make "secondary-blocked-reset" 0xe565dc [state,output,7,2,0]
    [word (state+0x2728) 1] [Patch (Address output) (B.replicate 0x5424 0),wordsAt cacheState [1,0x11111111,0x22222222,0x33333333]] (Just 0)
  ,make "secondary-disabled-updates-cache" 0xe565dc [state,output,7,2,0]
    (config [(0xc0,0)]) [Patch (Address output) (B.replicate 0x5424 0),wordsAt cacheState [0,2,0,0]] (Just 0)]
  ++ [make "secondary-byte-input-full-word-cache-key" 0xe565dc [state,output,7,2,0]
    ([word (state+0x3240) 1,byteAt (state+0x6c0) 0,word (state+0x660) 0x12340002,Patch (Address (state+0x3248)) (B.pack [1,255,2,2,4,4,3,253])]++config (witnessConfiguration WeightedExposure))
    ([wordsAt output [1,1,64,0,17,19,5,7],word (output+0x5420) 0,wordsAt cacheState [0,2,0xffffffff,0x12340002]]++bankEffects fullBanks (fullCoefficients WeightedExposure)++secondaryPlanarEffects WeightedExposure) (Just 1)
  ,make "secondary-cache-unavailable" 0xe565dc [state,output,7,2,0]
    ([word (state+0x3240) 2]++config [(0xc0,2),(0x3e0009,1)])
    [Patch (Address output) (B.replicate 0x5424 0),wordsAt cacheState [0,2,0,0],wordsAt fullBanks (replicate (14*256) background)] (Just 0)]

generatorCases :: [StageScenario]
generatorCases =
  [make ("generator-full-"++show witness) 0xdaa540 [2,0,0,0,1,if usesSpecifiedExposures witness then firstExposure else 0,if usesSpecifiedExposures witness then secondExposure else 0,0,2,output]
    ([Patch (Address output) (B.replicate 196 0)]++config (witnessConfiguration witness))
    ([Patch (Address output) (recordBytes False True (replicate 28 0))]++bankEffects fullBanks (fullCoefficients witness)) Nothing
  | witness <- [WeightedExposure,ConfiguredCorrection,SaturatingCorrection,IdentityCorrection,OversizedExponent]] ++
  [make "generator-both-features" 0xdaa540 [2,0,0,0,1,firstExposure,secondExposure,1,18,output]
    ([Patch (Address output) (B.replicate 196 0)]++config (witnessConfiguration WeightedExposure++[(0xc0,18)]++curveConfiguration 0x453b 300))
    ([Patch (Address output) (recordBytes True True [300..327])]++bankEffects fullBanks (fullCoefficients WeightedExposure)) Nothing
  ,make "generator-compact-preserves-sixth-bank" 0xdaa540 [5,0,0,0,1,firstExposure,secondExposure,0,2,output]
    ([Patch (Address output) (B.replicate 196 0xa5)]++config (witnessConfiguration WeightedExposure))
    ([Patch (Address output) (B.pack [0,1]<>B.replicate 4 0xa5<>B.replicate 6 0<>B.replicate 2 0xa5<>B.replicate 5 0<>B.replicate 153 0xa5<>B.concat [wordBytes (compactBanks+bank*1024) | bank <- [0..5]]),wordsAt (compactBanks+5*1024) (replicate 256 background)]++bankEffects compactBanks (replicate 3 (CoefficientTriplet 256 256 256)++replicate 2 (CoefficientTriplet 224 256 416))) Nothing] ++
  [make "generator-permuted-components" 0xdaa540 [0,0,0,0,1,0,0,0,2,output]
    ([Patch (Address output) (B.replicate 196 0)]++config (witnessConfiguration ConfiguredCorrection++[(0x13433+index,value) | (index,value) <- zip [0..] [1,2,0,4,5,3,7,8,6]]++[(0x1343c+index,value) | (index,value) <- zip [0..] [2,0,1,5,3,4]]++[(0x23200+component*64+cell,value) | (component,value) <- zip [0..] [240,248,0,8,16,24,32,40,48,56,64,72],cell <- [0..63]]))
    ([Patch (Address output) (recordBytes False False (replicate 28 0)),wordsAt (compactBanks+5*1024) (replicate 256 background)]++bankEffects compactBanks [CoefficientTriplet 280 272 272,CoefficientTriplet 272 263 263,CoefficientTriplet 280 272 272,CoefficientTriplet 256 240 248,CoefficientTriplet 280 264 272]) Nothing
  ,make "generator-global-primary-bank" 0xdaa540 [0,11,21,101,0,0,0,0,16,output]
    ([Patch (Address output) (B.replicate 196 0)]++config ([(0xc0,16),(0x3e0009,0),(0x3e3100,10),(0x3e3101,20),(0x3e310e,20),(0x3e310f,40),(0x3e3168,2),(0x309c,7),(0x309d,9)]++configurationHalf 0x3e3124 100++configurationHalf 0x3e3126 200++configurationHalf 0x3e315e 2++configurationHalf 0x3014 41++configurationHalf 0x3016 42++configurationHalf 0x3600 44++curveConfiguration 0x4078e0 500))
    [Patch (Address output) (B.pack [1,0]<>B.concat (map halfBytes [41,42,0,0])<>B.pack [7,9]<>B.concat (map halfBytes [44,0,0])<>B.replicate 2 0<>B.concat (map halfBytes [500..527])<>B.replicate 120 0)] Nothing] ++
  [make ("generator-invalid-profile-"++show profile) 0xdaa540 [profile,0,0,0,1,firstExposure,secondExposure,1,18,output]
    ([Patch (Address output) (B.replicate 196 0xa5)]++config [(0xc0,18),(0x3e0009,1)])
    [Patch (Address output) (B.pack [0,0]<>B.replicate 194 0xa5)] Nothing | profile <- [4,13,14,0xffffffff]] ++
  [make "generator-feature-mask-disabled-preserves-descriptor" 0xdaa540 [2,0,0,0,1,firstExposure,secondExposure,1,0,output]
    [Patch (Address output) (B.replicate 196 0xa5)] [Patch (Address output) (B.pack [0,0]<>B.replicate 194 0xa5)] Nothing]
