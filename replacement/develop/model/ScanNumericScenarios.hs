{-# LANGUAGE GHC2021 #-}
module ScanNumericScenarios
  ( scanNumericPublications, scanNumericScenarios
  , CalibrationMode(..), PrimaryAmount(..)
  , scanNumericInitial, scanNumericMeasuredEffects, scanNumericAdjustedEffects
  ) where

import qualified Data.ByteString as B
import Data.Word
import PipelineModel
import Scenarios (Address(..),Patch(..),word,wordBytes)

data CalibrationMode = ReferenceCalibration | SpatialCalibration deriving (Eq,Show)
data PrimaryAmount = QuarterAmount | HalfAmount deriving (Eq,Show)
data BufferEdit = BufferEdit Int B.ByteString

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_scan_"++name))
scanNumericPublications :: [StagePublication]
scanNumericPublications = [publication address name | (address,name) <-
  [(0xd6c2ec,"read_packed_sample"),(0xd6d2a0,"select_region"),(0xd6da54,"region_brightness"),
   (0xd6e564,"measure_region"),(0xd6e600,"analyze_samples"),(0xd6ed3c,"sampling_grid"),
   (0xd6ef64,"saturation"),(0xd6efd8,"sampling_period"),(0xd6f0c8,"accumulate_sample"),
   (0xd6ff08,"mosaic_channel"),(0xd6ff64,"normalize_samples"),(0xd6ff68,"normalize_samples_tail"),
   (0xd70538,"finish_measurement"),(0xd70658,"red_ratio_window"),(0xd70b10,"blue_ratio_window"),
   (0xd710a0,"select_histogram_samples"),(0xd71390,"weighted_ratios"),(0xd71980,"constrain_ratios"),
   (0xd71e3c,"project_curve"),(0xd72b7c,"adjust_components"),(0xd72c48,"choose_adjustment"),
   (0xd736cc,"primary_adjustment"),(0xd73f18,"secondary_adjustment"),(0xd745fc,"spatial_correction"),
   (0xd7510c,"scale_region"),(0xd752d0,"integer_sqrt"),(0xd75ae8,"exposure_gain"),
   (0xd75c0c,"reference_tables"),(0xd75cd0,"component_tables"),(0xd75d88,"first_anchor"),
   (0xd75e1c,"second_anchor"),(0xd75eb0,"target_anchor"),(0xd75f44,"blue_attenuation_curve"),
   (0xd7604c,"correction_points"),(0xd76118,"primary_responses"),(0xd76284,"primary_component_responses"),
   (0xd76388,"secondary_responses"),(0xd76578,"secondary_red_curve"),(0xd766ac,"secondary_blue_curve"),
   (0xd767e0,"correction_limits"),(0xd76df8,"neighborhood_weight"),(0xd773d0,"interpolate")]]

add :: Address -> Word32 -> Address
add (Address address) offset = Address (address+offset)
atWord :: Address -> Word32 -> Word32 -> Patch
atWord address offset = word (addressWord address+offset)
rawPixels, shading, brightnessCurve, calibration, gridRed, gridBlue :: Address
rawPixels=Address 0x9c0000
shading=Address 0x9c1000
brightnessCurve=Address 0x9c2000
calibration=Address 0x9e0000
gridRed=Address 0x9c6000
gridBlue=Address 0x9c7000

halfBytes :: Word16 -> B.ByteString
halfBytes = B.take 2 . wordBytes . fromIntegral
halfWords :: [Word16] -> B.ByteString
halfWords = B.concat . map halfBytes
fullWords :: [Word32] -> B.ByteString
fullWords = B.concat . map wordBytes
halfEdit :: Int -> Word16 -> BufferEdit
halfEdit offset value=BufferEdit offset (halfBytes value)
wordEdit :: Int -> Word32 -> BufferEdit
wordEdit offset value=BufferEdit offset (wordBytes value)
buffer :: Int -> Word8 -> [BufferEdit] -> B.ByteString
buffer size initial = foldl' apply (B.replicate size initial)
 where apply bytes (BufferEdit offset replacement)=B.take offset bytes<>replacement<>B.drop (offset+B.length replacement) bytes
editPatches :: Address -> [BufferEdit] -> [Patch]
editPatches address edits=[Patch (add address (fromIntegral offset)) bytes | BufferEdit offset bytes<-edits]
amountByte :: PrimaryAmount -> Word8
amountByte QuarterAmount=25
amountByte HalfAmount=50
profileKind :: CalibrationMode -> Word32
profileKind ReferenceCalibration=1
profileKind SpatialCalibration=0
finalRed :: CalibrationMode -> PrimaryAmount -> Word16
finalRed ReferenceCalibration QuarterAmount=320
finalRed ReferenceCalibration HalfAmount=384
finalRed SpatialCalibration QuarterAmount=277
finalRed SpatialCalibration HalfAmount=299
limitedRed :: CalibrationMode -> PrimaryAmount -> Word16
limitedRed ReferenceCalibration QuarterAmount=288
limitedRed ReferenceCalibration HalfAmount=320
limitedRed SpatialCalibration QuarterAmount=267
limitedRed SpatialCalibration HalfAmount=278

configurationEdits :: PrimaryAmount -> [BufferEdit]
configurationEdits amount =
  [BufferEdit key (B.singleton value) | (key,value)<-
    [(0xa573,255),(0xa576,50),(0xa577,100),(0xa57a,1),(0xa57b,8),(0xa57c,0),(0xa57d,1),(0xa57e,1),
     (0xa57f,0),(0xa580,0),(0xa581,0),(0xa582,100),(0xa583,0),(0xa584,0),(0xa585,0),(0xa586,1),
     (0xa624,1),(0xa625,0),(0xa626,100),(0xa627,1),
     (0xa64c,60),(0xa64d,80),(0xa64e,100),(0xa64f,100),(0xa650,90),(0xa651,90),
     (0xa644,60),(0xa645,80),(0xa646,100),(0xa647,100),(0xa648,90),(0xa649,90)]] ++
  [halfEdit key value | (key,value)<-[(0xa574,1024),(0xa578,100),(0xa59f,0),(0xa5a1,100),(0xa5a3,2000)]] ++
  concat [[halfEdit (0xa587+index*4) value,BufferEdit (0xa589+index*4) (B.singleton 255),BufferEdit (0xa58a+index*4) (B.singleton 1)] | (index,value)<-zip [0..] [0,50,100,200,300,511]] ++
  [BufferEdit 0xa628 (B.pack [0,64,128,192,255]),BufferEdit 0xa62d (B.replicate 5 1),
   BufferEdit 0xa632 (B.pack [0,64,128,192,255]),BufferEdit 0xa637 (B.replicate 5 100)] ++
  [BufferEdit key (halfWords [0,32,64,96,128,160,192,224,255]) | key<-[0xa685,0xa5bb]] ++
  [BufferEdit key (B.replicate 9 (amountByte amount)) | key<-[0xa697,0xa5cd]] ++
  [BufferEdit key (B.replicate 22 100) | key<-[0xa6a3,0xa5d9]] ++
  [BufferEdit key (B.replicate 9 100) | key<-[0xa6ba,0xa5ef]] ++
  [BufferEdit key (halfWords [0,50,100,150,200,250,300]) | key<-[0xa66f,0xa5a6,0xa6e7,0xa6fc,0xa5fa,0xa60f]] ++
  [BufferEdit key (B.replicate 7 0) | key<-[0xa67d,0xa5b4]] ++
  [BufferEdit key (B.pack [0,10,20,30,40,50,60]) | key<-[0xa6f5,0xa70a,0xa608,0xa61d]] ++
  [BufferEdit key (B.pack [fromIntegral (max 0 (255-index `div` 2)) | index<-[0..512::Int]]) | key<-[0x19000,0x1a000]] ++
  [BufferEdit key (halfWords [0..511]) | key<-[0x19400,0x1a400]] ++
  [BufferEdit key (B.replicate 512 100) | key<-[0x1b000,0x1b800]]

planeHeaderWords :: [Word32]
planeHeaderWords=[addressWord rawPixels,8,8,12,addressWord shading,0x55aa0008,0,0,0,0,0,0,0]
regionWords :: [Word32]
regionWords=[0,0,8,8,0,0,0,0,0,0,0,0,0]
profileWords :: CalibrationMode -> [Word32]
profileWords mode=[profileKind mode,addressWord gridRed,addressWord gridBlue,100*65536,8,8,8,8,0,0,2,0,0]
inputPixels :: B.ByteString
inputPixels=halfWords [case column `mod` 4 of 0->100;2->150;_->200 | _row<-[0..7::Int],column<-[0..7::Int]]
initialWorkspace :: B.ByteString
initialWorkspace=B.replicate 0xcb8 0xa5
initialScratch :: B.ByteString
initialScratch=buffer 0x4820 0xa5 [BufferEdit 0x4720 (B.replicate 256 0)]

scanNumericInitial :: Address -> Address -> Address -> CalibrationMode -> PrimaryAmount -> [Patch]
scanNumericInitial request workspace scratch mode amount =
  [Patch request (B.replicate 0x2ac 0),atWord request 0 1,Patch (add request 4) (fullWords regionWords),
   Patch (add request 0x20c) (halfWords [256,256,256]),Patch (add request 0x212) (halfWords [256,256,256]),
   Patch (add request 0x220) (fullWords (profileWords mode)),atWord request 0x254 (addressWord brightnessCurve),
   Patch (add request 0x258) (fullWords planeHeaderWords),atWord request 0x290 37,atWord request 0x298 (addressWord scratch),
   word 0x96ca50 (addressWord calibration),Patch calibration (buffer 0x1bc00 0 (configurationEdits amount)),
   Patch rawPixels inputPixels,Patch shading (B.replicate 256 0),
   Patch brightnessCurve (B.pack [fromIntegral (index `div` 16) | index<-[0..4095::Int]]),
   Patch workspace initialWorkspace,Patch scratch initialScratch,
   Patch gridRed (fullWords (replicate 256 10)),Patch gridBlue (fullWords (replicate 256 20))]

measurementWrites :: [BufferEdit]
measurementWrites = [wordEdit 20 200,wordEdit 0x414 100,wordEdit 0x814 150,
  halfEdit 0xc14 50,halfEdit 0xc16 75,halfEdit 0xc18 50,halfEdit 0xc1a 75,wordEdit 0xc1c 1,
  halfEdit 0xc20 12,halfEdit 0xc22 12,halfEdit 0xc24 255,halfEdit 0xc26 1,
  halfEdit 0xc50 1,halfEdit 0xc52 0,halfEdit 0xc54 511,halfEdit 0xc56 511,halfEdit 0xc58 0,
  halfEdit 0xc5a 1,halfEdit 0xc5c 13,halfEdit 0xc5e 1,wordEdit 0xc60 1]
measuredScratch :: B.ByteString
measuredScratch=buffer 0x4820 0xa5
  [BufferEdit 0x4720 (B.replicate 256 0),BufferEdit 0x320 (B.replicate 0x1400 0),
   wordEdit 0x320 100,wordEdit 0x720 200,wordEdit 0xb20 150,wordEdit 0x1720 50,wordEdit 0x1b20 75,
   BufferEdit 0x1f20 (fullWords (replicate 50 0++[1]++replicate 461 0)),
   BufferEdit 0x2720 (fullWords (replicate 75 0++[1]++replicate 436 0)),
   BufferEdit 0x2f20 (halfWords (replicate 50 0++replicate 462 1)),
   BufferEdit 0x3320 (halfWords (replicate 75 0++replicate 437 1)),
   BufferEdit 0x3720 (halfWords (replicate 512 1)),BufferEdit 0x3b20 (halfWords (replicate 512 1)),
   halfEdit 0x3f20 50,halfEdit 0x4120 75,halfEdit 0x4320 12,
   BufferEdit 0x4520 (halfWords (replicate 12 0++[1]++replicate 243 0))]
scanNumericMeasuredEffects :: Address -> Address -> [Patch]
scanNumericMeasuredEffects workspace scratch=editPatches workspace measurementWrites++[Patch scratch measuredScratch]

choiceWrites :: CalibrationMode -> [BufferEdit]
choiceWrites mode=[halfEdit 0xc84 0,halfEdit 0xc68 112,halfEdit 0xc6a 199,halfEdit 0xca2 112,halfEdit 0xca4 100,
  halfEdit 0xc80 (if mode==ReferenceCalibration then 50 else 75),halfEdit 0xc82 38,halfEdit 0xca6 100,
  halfEdit 0xc86 (if mode==ReferenceCalibration then 0 else 168),halfEdit 0xc88 (if mode==ReferenceCalibration then 0 else 100),
  halfEdit 0xc8a 0,halfEdit 0xc8c 0] ++
  (if mode==SpatialCalibration then [wordEdit 0xc6c 10,wordEdit 0xc70 20,wordEdit 0xc78 10,wordEdit 0xc7c 0] else [])
primaryWrites :: CalibrationMode -> PrimaryAmount -> [BufferEdit]
primaryWrites mode amount=[wordEdit 0xc90 (if mode==ReferenceCalibration then 111 else 125),halfEdit 0xc94 100,
  halfEdit 0xca8 100,halfEdit 0xcaa 100,halfEdit 6 256,halfEdit 8 (finalRed mode amount),halfEdit 10 256,
  halfEdit 12 256,halfEdit 14 (if mode==ReferenceCalibration then 512 else 341),halfEdit 16 256,
  halfEdit 0xc96 (fromIntegral (amountByte amount)),halfEdit 0xc8a 0,halfEdit 0xc8c 0]
secondaryWrites :: CalibrationMode -> PrimaryAmount -> Bool -> [BufferEdit]
secondaryWrites mode amount limited=[halfEdit 0xc98 100,halfEdit 0xc9a 0,halfEdit 0xc9c 0,
  halfEdit 0xc9e (if limited then 50 else 100),halfEdit 0xca0 100,
  halfEdit 0 256,halfEdit 2 (if limited then limitedRed mode amount else finalRed mode amount),halfEdit 4 256]
scanNumericAdjustedEffects :: Address -> CalibrationMode -> PrimaryAmount -> [Patch]
scanNumericAdjustedEffects workspace mode amount=editPatches workspace (choiceWrites mode++primaryWrites mode amount++secondaryWrites mode amount False)

scanNumericScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
scanNumericScenarios _ _=selectionCases++brightnessCases++measurementCases++adjustmentCases++projectionCases++primitiveCases++normalizationCases
 where
  request=Address 0x9a1000
  workspace=Address 0x9b0000
  scratch=Address 0xa00000
  output=Address 0x9a8000
  plane=add request 0x258
  region=add request 4
  levels=add request 0x20c
  fallback=add request 0x212
  profile=add request 0x220
  base mode amount=scanNumericInitial request workspace scratch mode amount
  make name entry suffix args initial final returned=StageScenario (StageCaseName ("scan-numeric-"++name)) (publication entry suffix) (map addressWord args) initial final returned [] []
  makeWords name entry suffix args initial final returned=StageScenario (StageCaseName ("scan-numeric-"++name)) (publication entry suffix) args initial final returned [] []
  expectedWorkspace edits=Patch workspace (buffer 0xcb8 0xa5 edits)
  measured=expectedWorkspace measurementWrites
  selectionCases=
    [make "single-needs-measurement" 0xd6d2a0 "select_region" [request,workspace] (base ReferenceCalibration HalfAmount)
      [expectedWorkspace [halfEdit 0xc28 0,halfEdit 0xc2a 0,wordEdit 0xc2c 0,wordEdit 0xc30 0,BufferEdit 0xc34 (B.pack [3,3]),BufferEdit 0xc38 (fullWords [0,0,8,8,4,4])]] Nothing,
     make "single-already-valid" 0xd6d2a0 "select_region" [request,workspace] (base ReferenceCalibration HalfAmount++[Patch (add region 0x20) (halfWords [1,1])])
      [expectedWorkspace [halfEdit 0xc28 65535,halfEdit 0xc2a 0,wordEdit 0xc2c 0,wordEdit 0xc30 0,BufferEdit 0xc34 (B.pack [3,3])]] Nothing] ++
    [let selected=if preferTall then 0 else 1
         pixelRows=halfWords [case column `mod` 4 of 0->100;2->150;_->if row<4 then 200 else 400 | row<-[0..7::Int],column<-[0..7::Int]]
         second=[0,4,8,4,0,0,0,0,0,0,40,0,0]
      in make ("brightness-height-control-"++show preferTall) 0xd6d2a0 "select_region" [request,workspace]
        (base ReferenceCalibration HalfAmount++[atWord request 0 2,Patch rawPixels pixelRows,Patch (add region 52) (fullWords second),atWord region 0x28 (if preferTall then 100 else 30)])
        [expectedWorkspace [halfEdit 0xc28 selected,halfEdit 0xc2a 26,wordEdit 0xc2c 1,wordEdit 0xc30 0,BufferEdit 0xc34 (B.pack [if preferTall then 1 else 0,if preferTall then 1 else 0]),BufferEdit 0xc38 (fullWords (if preferTall then [0,0,8,8,4,4] else [0,4,8,4,4,6]))],Patch scratch (buffer 0x4820 0xa5 [BufferEdit 0x4720 (B.replicate 256 0),wordEdit 0 0,wordEdit 4 0,wordEdit 400 0,wordEdit 404 4])]
        Nothing | preferTall<-[False,True]]
  brightnessCases=
    [make "brightness-12-bit" 0xd6da54 "region_brightness" [plane,region,brightnessCurve,levels,output] (base ReferenceCalibration HalfAmount) [Patch output (halfBytes 13)] Nothing] ++
    [make ("brightness-bits-"++show bits) 0xd6da54 "region_brightness" [plane,region,brightnessCurve,levels,output]
      (base ReferenceCalibration HalfAmount++[atWord plane 12 bits]) [Patch output (halfBytes expected)] Nothing | (bits,expected)<-[(10,51),(14,4)]]
  measurementCases=
    [makeWords "measure-nonzero-cfa" 0xd6e564 "measure_region" (map addressWord [plane,region,brightnessCurve,levels]++[1]++map addressWord [workspace,scratch])
      (base ReferenceCalibration HalfAmount) [measured,Patch scratch measuredScratch,Patch plane (fullWords planeHeaderWords),Patch rawPixels inputPixels] (Just 1),
     make "analyze-nonzero-cfa" 0xd6e600 "analyze_samples" [plane,region,levels,workspace,scratch,output]
      (base ReferenceCalibration HalfAmount)
      [expectedWorkspace [wordEdit 20 200,wordEdit 0x414 100,wordEdit 0x814 150],Patch output (halfBytes 1),Patch scratch (buffer 0x4820 0xa5 [BufferEdit 0x4720 (B.replicate 256 0),BufferEdit 0x320 (B.replicate 0x1400 0),wordEdit 0x320 100,wordEdit 0x720 200,wordEdit 0xb20 150,wordEdit 0x1720 50,wordEdit 0x1b20 75,BufferEdit 0x1f20 (fullWords (replicate 50 0++[1]++replicate 461 0)),BufferEdit 0x2720 (fullWords (replicate 75 0++[1]++replicate 436 0))])]
      Nothing,
     makeWords "measure-saturated-rejection" 0xd6e564 "measure_region" (map addressWord [plane,region,brightnessCurve,levels]++[1]++map addressWord [workspace,scratch])
      (base ReferenceCalibration HalfAmount++[Patch rawPixels (halfWords (replicate 64 4095))])
      [expectedWorkspace [wordEdit 20 0,wordEdit 0x414 0,wordEdit 0x814 0],Patch scratch (buffer 0x4820 0xa5 [BufferEdit 0x4720 (B.replicate 256 0),BufferEdit 0x320 (B.replicate 0x1400 0),BufferEdit 0x1f20 (B.replicate 4096 0)])] (Just 0)]
  adjustmentCases=concat
    [let initial=base mode amount++[measured,Patch scratch measuredScratch]
         choice=choiceWrites mode
         primary=primaryWrites mode amount
         complete=choice++primary++secondaryWrites mode amount False
         arguments=map addressWord [plane,region,profile,levels,workspace,fallback]++[0,addressWord (add request 0x290),0,0,addressWord scratch]
      in [makeWords ("adjust-active-"++show (mode,amount)) 0xd72b7c "adjust_components" arguments initial [expectedWorkspace (measurementWrites++complete)] (Just 1),
          make ("choose-active-"++show (mode,amount)) 0xd72c48 "choose_adjustment" [region,plane,profile,workspace] initial [expectedWorkspace (measurementWrites++choice)] (Just (if mode==ReferenceCalibration then 1 else 0)),
          makeWords ("primary-active-"++show (mode,amount)) 0xd736cc "primary_adjustment" ([if mode==ReferenceCalibration then 1 else 0,0]++map addressWord [fallback,levels,scratch,profile,workspace])
            (initial++[expectedWorkspace (measurementWrites++choice)]) [expectedWorkspace (measurementWrites++choice++primary)] Nothing,
          makeWords ("secondary-limit-control-"++show (mode,amount)) 0xd73f18 "secondary_adjustment" ([if mode==ReferenceCalibration then 1 else 0,0,0]++map addressWord [fallback,add request 0x290,profile]++[0,addressWord region,addressWord workspace])
            (initial++[expectedWorkspace (measurementWrites++choice++primary),Patch (add calibration 0xa626) (B.singleton 50)]) [expectedWorkspace (measurementWrites++choice++primary++secondaryWrites mode amount True)] Nothing,
          makeWords ("adjust-invalid-component-map-"++show (mode,amount)) 0xd72b7c "adjust_components" arguments
            (initial++[Patch (add calibration (if mode==ReferenceCalibration then 0x19400 else 0x1a400)) (B.replicate 1024 0)])
            [expectedWorkspace (measurementWrites++[halfEdit 0xc84 0]++if mode==SpatialCalibration then [wordEdit 0xc6c 10,wordEdit 0xc70 20,wordEdit 0xc78 10,wordEdit 0xc7c 0] else [])] (Just 0)]
    | mode<-[ReferenceCalibration,SpatialCalibration],amount<-[QuarterAmount,HalfAmount]]
  projectionCases=
    [makeWords ("projection-"++show (red,blue)) 0xd71e3c "project_curve" [red,blue,addressWord output,addressWord (add output 2),addressWord (add calibration 0x19000)]
      (base ReferenceCalibration HalfAmount) [Patch output (halfWords [expectedRed,expectedBlue])] (Just above)
    | (red,blue,expectedRed,expectedBlue,above)<-[(50,75,112,199,0),(100,100,142,184,0),(100,299,62,224,1)]]
  primitiveCases=
    [makeWords ("packed-"++show index) 0xd6c2ec "read_packed_sample" [addressWord output,index] [Patch output (B.pack [0xbc,0x3a,0x12,0x54,0xf6,0xed])] [] (Just value) | (index,value)<-[(0,0xabc),(1,0x123),(2,0x654),(3,0xedf)]] ++
    [makeWords ("saturation-"++show bits) 0xd6ef64 "saturation" [bits] [] [] (Just value) | (bits,value)<-[(0,0),(10,1023),(12,4095),(14,16383),(16,0),(255,0)]] ++
    [makeWords ("sqrt-"++show value) 0xd752d0 "integer_sqrt" [value] [] [] (Just expected) | (value,expected)<-[(0,0),(1,1),(2,1),(3,1),(4,2),(15,3),(16,4),(17,4),(9999,99),(65536,256)]] ++
    [makeWords ("exposure-"++show value) 0xd75ae8 "exposure_gain" [fromIntegral (value::Int)] [] [] (Just expected) | (value,expected)<-[(-100,50),(-50,71),(0,100),(50,141),(100,200),(200,400)]] ++
    [makeWords ("interpolation-"++show value) 0xd773d0 "interpolate" [value,addressWord output,addressWord (add output 16),4]
      [Patch output (halfWords [10,20,30,40]),Patch (add output 16) (B.pack [100,80,40,20])] [] (Just expected) | (value,expected)<-[(5,100),(10,100),(15,90),(17,86),(20,80),(25,60),(30,40),(40,20),(50,20)]] ++
    [makeWords ("sampling-period-"++show (pattern,mode)) 0xd6efd8 "sampling_period" [addressWord plane]
      (base ReferenceCalibration HalfAmount++[atWord plane 0x28 pattern,atWord plane 0x2c mode]) [] (Just expected)
    | (pattern,values)<-[(0,[1,2,4]),(1,[1,2,4]),(2,[1,4,8]),(3,[1,4,8]),(4,[1,2,4]),(5,[1,2,4]),(6,[1,2,4]),(7,[1,2,4]),(8,[1,1,1]),(9,[1,4,8])],(mode,expected)<-zip [0..] values]
  normalizationCases=
    [let accepted=valid/=0 && not outOfRange
         red=if outOfRange then 2000 else 100
         green=if outOfRange then 100 else 200
         blue=if outOfRange then 200 else 150
         initialCells=[wordEdit 0x320 (red*2),wordEdit 0x720 (green*4),wordEdit 0xb20 (blue*2),wordEdit 0xf20 17,wordEdit 0x1320 19]
         finalCells=[wordEdit 0x320 (if accepted then red else 0),wordEdit 0x720 (if accepted then green else 0),wordEdit 0xb20 (if accepted then blue else 0)]++[wordEdit 0xf20 (if accepted then 17 else 0),wordEdit 0x1320 (if accepted then 19 else 0)]++
           (if valid==0 then [] else [wordEdit 0x1720 (if outOfRange then 2000 else 50),wordEdit 0x1b20 (if outOfRange then 200 else 75)])++
           (if accepted then [wordEdit (0x1f20+50*4) 4,wordEdit (0x2720+75*4) 11] else [])
      in makeWords ("normalize-"++show (valid,outOfRange)) 0xd6ff64 "normalize_samples" [addressWord levels,2,4,2,1,1,0,valid,4095,addressWord scratch]
        (base ReferenceCalibration HalfAmount++[Patch scratch (buffer 0x4820 0xa5 (initialCells++[wordEdit (0x1f20+50*4) 3,wordEdit (0x2720+75*4) 10]))])
        [Patch scratch (buffer 0x4820 0xa5 (initialCells++[wordEdit (0x1f20+50*4) 3,wordEdit (0x2720+75*4) 10]++finalCells))] (Just (if accepted then 1 else 0))
    | (valid,outOfRange)<-[(0,False),(1,False),(1,True)]]
