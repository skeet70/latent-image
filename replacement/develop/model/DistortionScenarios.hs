module DistortionScenarios (distortionPublications, distortionScenarios) where

import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), wordsAt, word)

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

build, region, coefficients, u24, project, rounded, bounds, validate, choose :: StagePublication
build = publication 0xe57d10 "build_distortion"
region = publication 0xe57cb4 "distortion_region"
coefficients = publication 0xda194c "read_distortion_coefficients"
u24 = publication 0x69fef0 "configuration_u24"
project = publication 0x6dfabc "project_radial_coordinate"
rounded = publication 0x6dfda8 "fixed12_round"
bounds = publication 0x6dfe18 "find_signed_bounds"
validate = publication 0x6dfe7c "validate_radial_grid"
choose = publication 0x6e0250 "choose_supplied_radial_grid"

distortionPublications :: [StagePublication]
distortionPublications = [build,region,coefficients,u24,project,rounded,bounds,validate,choose]

stage, config, output, scratch, arguments, samples, dataBase :: Word32
stage = 0x03200000
config = 0x03204000
output = 0x03205000
scratch = 0x03206000
arguments = 0x03207000
samples = 0x03208000
dataBase = 0x04000000

bytes :: Word32 -> [Word32] -> Patch
bytes address values = Patch (Address address) (B.pack (map fromIntegral values))
halfBytes :: Word32 -> [Word32]
halfBytes value = [value .&. 255,(value `shiftR` 8) .&. 255]
threeBytes :: Word32 -> [Word32]
threeBytes value = halfBytes value ++ [(value `shiftR` 16) .&. 255]

calibration :: Word32 -> [Word32] -> Word32 -> Word32 -> Word32 -> Word32 -> Patch
calibration key curve gainX gainY coefficientX coefficientY = bytes (dataBase+key)
  (concatMap threeBytes curve ++ concatMap halfBytes [gainX,gainY,coefficientX,coefficientY])

constantCurve :: [Word32]
constantCurve = replicate 31 65536

resultWords :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Word32]
resultWords mode gainX gainY coefficientX coefficientY =
  [mode,gainX,gainY,coefficientX+coefficientY*65536,1+65536,99,79] ++ constantCurve

scratchPatch :: Word32 -> Word32 -> [Word32] -> [Patch]
scratchPatch gainX gainY curve =
  [ Patch (Address scratch) (B.replicate 228 0)
  , wordsAt (scratch+32) [4,4,99,79,99,79,gainX,gainY,4096,4096]
  , wordsAt (scratch+88) [1+2*65536,1+65536,99,79]
  , wordsAt (scratch+104) curve
  , wordsAt samples curve
  ]

scenario :: String -> StagePublication -> [Word32] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario
scenario name target args initial final returned = StageScenario
  { stageCaseName = StageCaseName ("distortion-" ++ name)
  , stagePublication = target
  , stageArguments = args
  , stageInitial = initial
  , stageFinal = final
  , stageReturn = returned
  , stageCodePointers = []
  , stageCalls = []
  }

floor12, ceil12 :: Int32 -> Int32
floor12 value = fromInteger (toInteger value `div` 4096)
ceil12 value = fromInteger (negate ((negate (toInteger value)) `div` 4096))

rampProjection :: Integer -> Integer -> Bool -> Word32
rampProjection x y vertical = fromInteger (origin + sign * projected)
  where
    dx = abs (x*4096-99*2048)
    dy = abs (y*4096-79*2048)
    rx = dx `div` 512
    ry = dy `div` 512
    radius = rx*rx `div` 16 + ry*ry `div` 16
    position = min 81919 (radius * (if vertical then 2 else 1) `div` 524288)
    fraction = position `mod` 8192
    inverse = 8192-fraction
    square = inverse*inverse `div` 1024
    w0 = inverse*square `div` 8192
    w1 = square*fraction*3 `div` 8192
    w2 = (fraction*fraction `div` 1024)*inverse*3 `div` 8192
    weights = [w0,w1,w2,65536-w0-w1-w2]
    base = position `div` 8192 * 3
    interpolation = sum [((65536+(base+index)*1024)*weight) `div` 4096 | (index,weight) <- zip [0..3] weights]
    gain = (4096*(interpolation `div` 16)) `div` 4096
    projected = min 134217727 (gain*(if vertical then dy else dx) `div` 65536)
    origin = (if vertical then 79 else 99)*2048
    sign = if (if vertical then y*4096 else x*4096) > origin then 1 else -1

chooseInitial :: Word32 -> [Patch]
chooseInitial gain =
  [ wordsAt arguments [1+2*65536,gain,gain,99,79,99,79,99,79,4096,4096,1+65536,4,4]
  , wordsAt samples constantCurve
  ]
chooseArguments :: Word32 -> [Word32]
chooseArguments format = [arguments,arguments+4,arguments+12,arguments+20,arguments+28,arguments+36,arguments+44,samples,arguments+48,format]

frontendInitial :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
frontendInitial key operation manualMode manualSelection flags =
  [ Patch (Address stage) (B.replicate 0x3248 0)
  , bytes (stage+0x6c0) [1]
  , word (stage+0x4b4) operation
  , word (stage+0x3240) manualMode
  , word (stage+0x3244) manualSelection
  , word (stage+0x49c) flags
  , wordsAt config [0,0,4096,4096,0,200,100,80,99,79,0,200,0,0,100,80,99,79,0,0]
  , word 0x96ca50 dataBase
  , bytes (dataBase+0x3e3a00) [0]
  , bytes (dataBase+0x3e3a10) [200,0,250,0]
  , calibration key constantCurve 4096 4096 1 2
  , Patch (Address output) (B.replicate 152 0xa5)
  ]

distortionScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
distortionScenarios _ _ =
  [ scenario "configuration-u24-byte-order" u24 [0x400123]
      [word 0x96ca50 dataBase,bytes (dataBase+0x400123) [0x21,0x43,0x65]] [] (Just 0x654321)
  , scenario "region-negative-distance" region [stage]
      (regionInitial 0xffff 0 0) [] (Just 0)
  , scenario "region-middle" region [stage]
      (regionInitial 150 100 200) [] (Just 1)
  , scenario "region-above-thresholds" region [stage]
      (regionInitial 250 100 200) [] (Just 2)
  , coefficientCase "normal-bank" [0,2,1,0,output] (0x400000+17*101)
  , coefficientCase "alternate-bank" [0,0,0,1,output] 0x401200
  , coefficientCase "second-bank" [0,0,0,2,output] 0x402400
  , coefficientCase "manual-bank" [6,0,0,0,output] (0x403600+5*101)
  , scenario "projection-horizontal-identity" project [17,23,0,scratch,samples,0]
      (scratchPatch 4096 4096 constantCurve) [] (Just (17*4096))
  , scenario "projection-vertical-identity" project [17,23,1,scratch,samples,0]
      (scratchPatch 4096 4096 constantCurve) [] (Just (23*4096))
  , scenario "projection-negative-coordinate" project [0xffffffff,23,0,scratch,samples,0]
      (scratchPatch 4096 4096 constantCurve) [] (Just (fromIntegral (-4096 :: Int32)))
  , scenario "projection-zero-radius-gain" project [17,23,1,scratch,samples,0]
      (scratchPatch 4096 0 constantCurve) [] (Just (79*2048))
  , scenario "projection-cubic-ramp" project [10000,9000,0,scratch,samples,0]
      (scratchPatch 4096 4096 [65536+index*1024 | index <- [0..30]]) [] (Just (rampProjection 10000 9000 False))
  , scenario "projection-cubic-ramp-vertical" project [10000,9000,1,scratch,samples,0]
      (scratchPatch 4096 4096 [65536+index*1024 | index <- [0..30]]) [] (Just (rampProjection 10000 9000 True))
  , scenario "projection-register-shift-low-byte" project [17,23,0,scratch,samples,256]
      (scratchPatch 4096 4096 constantCurve) [] (Just (17*4096))
  , scenario "projection-register-shift32" project [17,23,0,scratch,samples,32]
      (scratchPatch 4096 4096 constantCurve) [] (Just (99*2048))
  , scenario "grid-format-two-rejects64" validate [scratch,2,64]
      (scratchPatch 4096 4096 constantCurve) [] (Just 0)
  , scenario "grid-identity-accept" validate [scratch,0,64]
      (scratchPatch 4096 4096 constantCurve) [] (Just 1)
  , scenario "grid-overexpansion-reject" validate [scratch,0,16]
      (scratchPatch 65535 65535 constantCurve) [] (Just 0)
  , scenario "choose64" choose (chooseArguments 0) (chooseInitial 4096) [] (Just 64)
  , scenario "choose32-format-two" choose (chooseArguments 2) (chooseInitial 4096) [] (Just 32)
  , scenario "choose-no-valid-grid" choose (chooseArguments 0) (chooseInitial 65535) [] (Just 0)
  , frontend "frontend-default-bank" 0x400000 0 0 0
  , frontend "frontend-alternate-bank" 0x401200 25 0 0
  , frontend "frontend-second-bank" 0x402400 26 0 0
  , frontend "frontend-manual-bank" 0x403600 0 1 6
  , scenario "empty-bounds" bounds [samples,output,0] [] [wordsAt output [0x80000000,0x7fffffff]] Nothing
  , scenario "signed-bounds" bounds [samples,output,5]
      [wordsAt samples [19,0xffffffff,0x80000001,77,0]] [wordsAt output [77,0x80000001]] Nothing
  ] ++ [scenario ("round-" ++ show value ++ "-" ++ show mode) rounded [fromIntegral value,mode] [] [] (Just (fromIntegral expected))
       | value <- [-8193,-4096,-1,0,1,4096,8193], mode <- [0,1], let expected = if mode == 0 then floor12 value else ceil12 value]
  where
    regionInitial distance first second =
      [ Patch (Address stage) (B.replicate 0x2268 0),bytes (stage+0x6c0) [1],bytes (stage+0x2264) (halfBytes distance)
      , word 0x96ca50 dataBase,bytes (dataBase+0x3e3a00) [0],bytes (dataBase+0x3e3a10) (halfBytes first ++ halfBytes second)
      ]
    coefficientCase name args key = scenario name coefficients args
      [word 0x96ca50 dataBase,calibration key constantCurve 0x8123 0xabcd 0x4321 0x5678,Patch (Address output) (B.replicate 144 0xa5)]
      [bytes output (halfBytes 0x8123 ++ halfBytes 0xabcd),bytes (output+12) (halfBytes 0x4321 ++ halfBytes 0x5678),wordsAt (output+20) constantCurve] Nothing
    frontend name key operation manualMode selection = scenario name build [stage,config,output]
      (frontendInitial key operation manualMode selection 0) [wordsAt output (resultWords 0 4096 4096 1 2)] Nothing
