module OutputCoefficientsScenarios
  ( CoefficientPart (..), CoefficientSources (..), coefficientEffects
  , coefficientSourceBundle, outputCoefficientPublications, outputCoefficientScenarios
  , coefficientProfile, exportCoefficientCurve, matrixCoefficients
  ) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int8, Int16, Int32)
import Data.List (find, foldl')
import Data.Word
import PipelineModel

data CoefficientSources = CoefficientSources { coefficientImage4 :: B.ByteString, coefficientImage5 :: B.ByteString }
data CoefficientPart = MatrixCoefficients | BlendedLookup | PairedLookup | SensorLayout | SignedChannels
  | AuxiliaryChannels | NoiseControls | NoiseCoefficients | ThresholdCurves | SelectedLookup
  | LookupControls | ExportCoefficients deriving (Eq, Show, Enum, Bounded)

coefficientSourceBundle :: B.ByteString -> B.ByteString -> B.ByteString
coefficientSourceBundle = B.append

sourceBytes :: CoefficientSources -> Word32 -> Int -> B.ByteString
sourceBytes images address count
  | offset < 0 || offset+count > B.length image = error "coefficient model requires the original image4 and image5 source bytes"
  | otherwise = B.take count (B.drop offset image)
  where
    (image,base) = if address >= 0xbf1a00 then (coefficientImage5 images,0xbf1a00) else (coefficientImage4 images,0x61c400)
    offset = fromIntegral address-base

little :: (Integral a, Bits a) => B.ByteString -> a
little = B.foldr (\value rest -> fromIntegral value .|. (rest `shiftL` 8)) 0
romHalf :: CoefficientSources -> Word32 -> Word32
romHalf images address = little (sourceBytes images address 2)
romWord :: CoefficientSources -> Word32 -> Word32
romWord images address = little (sourceBytes images address 4)
wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = little (B.take 4 (B.drop offset bytes))
halfAt :: B.ByteString -> Int -> Word16
halfAt bytes offset = little (B.take 2 (B.drop offset bytes))
byteAt :: B.ByteString -> Int -> Word8
byteAt = B.index
halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` amount) | amount <- [0,8,16,24]]
bytes :: Address -> [Word8] -> Patch
bytes address = Patch address . B.pack
halves :: Address -> [Word16] -> Patch
halves address = Patch address . B.concat . map halfBytes
word :: Address -> Word32 -> Patch
word address = Patch address . wordBytes
plus :: Address -> Word32 -> Address
plus (Address address) offset = Address (address+offset)
cfgHalf :: (Word32 -> Word8) -> Word32 -> Word16
cfgHalf cfg key = fromIntegral (cfg key) + fromIntegral (cfg (key+1))*256
keyed :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> Word32 -> Word32
keyed images cfg table index = fromIntegral (cfg (romHalf images (table+index*2)))
keyBytes :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> Int -> [Word8]
keyBytes images cfg table count = [cfg (romHalf images (table+fromIntegral index*2)) | index <- [0..count-1]]
keyHalves :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> Int -> [Word16]
keyHalves images cfg table count = [cfgHalf cfg (romHalf images (table+fromIntegral index*2)) | index <- [0..count-1]]

coefficientProfile :: CoefficientSources -> Word32 -> ([Word8],[Word16])
coefficientProfile images identifier = (B.unpack (sourceBytes images (row+4) 9), [fromIntegral (romHalf images (row+14+lane*2)) | lane <- [0..27]])
  where
    count = romWord images 0x824bc4
    rows = [0x824bc8+index*72 | index <- [0..count-1]]
    row = maybe 0x824bc8 id (find (\address -> romWord images address == identifier) rows)

exportCoefficientCurve :: CoefficientSources -> Word32 -> [Word16]
exportCoefficientCurve images identifier = [fromIntegral (romHalf images (row+4+lane*2)) | lane <- [0..21]]
  where
    count = romWord images 0x825630
    rows = [0x825634+index*48 | index <- [0..count-1]]
    row = maybe 0x825634 id (find (\address -> romWord images address == identifier) rows)

stageMode :: B.ByteString -> Word32
stageMode descriptor = case wordAt descriptor 0x140 of
  5 -> 2
  6 -> 2
  3 -> staged
  4 -> staged
  1 -> 1
  2 -> 1
  _ -> if wordAt descriptor 0x1d0 == 1 && wordAt descriptor 0x1d4 /= 0 then staged else 0
  where staged = if wordAt descriptor 0xdc == 1 then 2 else 1

kindMode :: B.ByteString -> Word32
kindMode descriptor = maybe alternate id (lookup (wordAt descriptor 0xe4) table)
  where
    table = [(5,2),(6,3),(7,26),(8,4),(9,5),(10,6),(11,25),(12,25),(14,28),(20,32),(21,33),(22,34),(23,31),(24,30),(25,13),(26,14),(27,15),(28,29),(29,7),(30,8),(31,9),(32,10),(33,11),(34,12),(37,27),(40,16),(41,17),(42,18),(43,19),(44,20),(45,21),(46,22),(47,23),(48,24),(49,35),(50,35),(51,1)]
    alternate = case wordAt descriptor 0xe8 of
      value | value >= 1 && value <= 7 -> 35+value
      value | value >= 8 && value <= 13 -> 43
      _ -> if wordAt descriptor 0xec /= 0 then 1 else 0

selectionIndex :: B.ByteString -> Word32
selectionIndex descriptor
  | wordAt descriptor 0x19c == 2 && wordAt descriptor 0x228 /= 0 = 0
  | selected >= 1 && selected <= 4 = selected+1
  | otherwise = 1
  where selected = wordAt descriptor 0x154

modeIdentifier :: B.ByteString -> Word32
modeIdentifier descriptor = maybe alternate id (lookup (wordAt descriptor 0xe4) (zip modes [1..]))
  where
    modes = [5,6,37,8,9,10,14,11,7,13,29,30,31,32,33,34,28,25,26,27,24,23,20,22,40,41,42,43,44,45,46,47,48]
    alternate = if wordAt descriptor 0xe4 == 12 then 8 else let value=wordAt descriptor 0xe8 in if value >= 1 && value <= 13 then value+33 else 0

modeValue :: (Word32 -> Word8) -> B.ByteString -> Word32 -> Word32
modeValue cfg descriptor limit
  | cfg 0xa923 == 0 = if limit < 12 then min 10 ((fromIntegral (byteAt descriptor 0x11b)+2) `div` 3) else min 30 (fromIntegral (byteAt descriptor 0x11b))
  | limit < 12 = band
  | band == 0 = 0
  | otherwise = min 30 (band*3-2)
  where
    value=fromIntegral (halfAt descriptor 0x1ea :: Word16) :: Int16
    band=fromIntegral (length (takeWhile (<=value) [47,150,247,350,447,550,642,750,842,950]))

matrixCoefficients :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> (Word8,[Word16])
matrixCoefficients images cfg descriptor
  | selection == 0 || selection > 20 = (0,[1024,0,0,0,1024,0,0,0,1024])
  | otherwise = (1,map (fromIntegral . (\value -> value*percent `quot` 100) . signed) corrected)
  where
    w=wordAt descriptor
    key | w 0x104 == 2 && w 0x110 /= 0 = romHalf images (0xd680f8+(w 0x110+w 0x108*4)*2)
        | otherwise = romHalf images ((if w 0xe4 == 23 then 0xd67e64 else 0xd6891c)+(w 0x104+w 0x108*8)*2)
    selection=fromIntegral (cfg key) :: Word32
    raw=map (cfgHalf cfg) [romHalf images (0xd67f88+(selection-1)*18+lane*2) | lane <- [0..8]]
    corrected=if w 0xe4 == 24 then [1024,0,0]++drop 3 raw else raw
    signed value = if value < 4096 then toInteger value else toInteger (value .&. 4095)-4096
    percent | w 0x140 /= 7 || byteAt descriptor 0x1ac == 0 = 100
            | otherwise = case w 0x1a8 of
                0->800; 1->632; 2->503; 3->400; 4->316; 5->251; 6->200; 7->158; 8->126
                value | value >= 10 && value <= 12 -> toInteger (byteAt descriptor 0x1ad)
                _ -> 100

coefficientEffects :: CoefficientSources -> (Word32 -> Word8) -> Address -> B.ByteString -> Address -> CoefficientPart -> [Patch]
coefficientEffects images cfg descriptorAddress descriptor block part = case part of
  MatrixCoefficients -> let (enabled,values)=matrixCoefficients images cfg descriptor in [putBytes 0x2c [enabled],putHalves 0x2e values]
  BlendedLookup -> let (header,curve,extra,mutations)=blended in [putBytes 0x40 header,putHalves 0x4a curve,putHalves 0x82 [65535,extra]]++mutations
  PairedLookup -> let (first,second)=paired in [putHalves 0x86 (snd (coefficientProfile images first)),putHalves 0xbe (snd (coefficientProfile images second))]
  SensorLayout -> let values=keyBytes images cfg (0xd68158+w 0xf4*32) 16 in [putBytes 0x12 (take 13 values),putHalves 0x20 (keyHalves images cfg (0xd685a4+w 0xf4*6) 3),putBytes 0x26 (reverse (drop 13 values))]
  SignedChannels -> let values=keyBytes images cfg (0xd688c4+w 0xf4*8) 4 in [putBytes 0 (keyBytes images cfg 0xd68590 10),putHalves 10 [signedByte (values!!0),signedByte (values!!1),fromIntegral (values!!2)],putBytes 16 [values!!3]]
  AuxiliaryChannels -> let values=keyBytes images cfg (0xd689a4+w 0xf4*12) 6 in [putBytes 0 (take 3 values),putHalves 4 (map fromIntegral (take 2 (drop 3 values))),putBytes 8 [values!!5]]
  NoiseControls -> noiseControls
  NoiseCoefficients -> noiseControls++[putHalves 14 (map (fromIntegral . min 511) noiseGains),putBytes 20 [fromIntegral (kb 0xd696c0 (w 0xf4))]]
  ThresholdCurves -> thresholdEffects
  SelectedLookup -> let identifier=if equalPair then cb 0xc63f else kb 0xd6a83c (w 0x15c*5+w 0x154); (header,curve)=coefficientProfile images identifier in [putBytes 0x34 header,putHalves 0x3e curve]
  LookupControls -> [putBytes 2 (map cfg [0xc65b,0xc658,0xc659,0xc65a]++[fromIntegral lookupControl])]
  ExportCoefficients -> let (values,mutations)=exported; scale=max 100 (halfAt descriptor 0x1a6) in [putHalves 8 (map (\value -> fromIntegral (min 8192 ((toInteger value*toInteger scale+50) `div` 100))) values)]++mutations++[halves (plus descriptorAddress 0x1a6) [scale]]
  where
    w=wordAt descriptor
    h=halfAt descriptor
    cb key=fromIntegral (cfg key) :: Word32
    kb=keyed images cfg
    putBytes offset=bytes (plus block offset)
    putHalves offset=halves (plus block offset)
    signedByte value=fromIntegral (fromIntegral value :: Int8)
    stage=stageMode descriptor
    kind=if stage==2 && w 0x124==7 then 30 else kindMode descriptor
    column=selectionIndex descriptor
    bounded table limit row=if row>=1 && row<=limit then kb table ((row-1)*6+column) else 0
    parameter=bounded 0xd685f4 60
    exportIdentifier=bounded 0xd6a920 80
    equalPair=w 0x160==3 && w 0x11c==w 0x120
    paired | w 0x1d0==1 && w 0x1d4/=0 = (parameter (cb 0xc64d),parameter (cb 0xc653))
           | equalPair = (parameter (cb 0xc64c),parameter (cb 0xc652))
           | otherwise = (parameter (kb 0xd680f0 (w 0x160)),parameter (kb 0xd68914 (w 0x160)))
    blended | cfg 0xb5==1 = let (header,curve)=coefficientProfile images (cb 0xcb75) in (header,curve,0,[])
            | cfg 0xcb74==1 = let (header,curve)=coefficientProfile images (cb 0xcb76) in (header,curve,0,[])
            | otherwise = (header,zipWith combine firstCurve secondCurve,extra,[bytes (plus descriptorAddress 0x1c4) (map fromIntegral [first,second,weight])])
      where
        index=w 0x104*5+stage*40+w 0xf8
        first=parameter (if kind==0 then kb 0xd684a0 index else kb 0xd68296 (stage*43+kind))
        second=parameter (if kind==0 then kb 0xd683b0 index else kb 0xd67e82 (stage*43+kind))
        weight=kb 0xd685e0 (w 0x13c)
        (header,firstCurve)=coefficientProfile images first
        (_,secondCurve)=coefficientProfile images second
        combine a b=fromIntegral (min 16383 (((toInteger weight*toInteger a+(128-toInteger weight)*toInteger b+64) `mod` 4294967296) `div` 128))
        extra=if stage==0 then fromIntegral (fromIntegral (h 0x118)*kb 0xd68108 (w 0x104*5+w 0x154) `shiftR` 6) else 0
    lookupControl = if chosen>=1 && chosen<=16 then kb 0xd6a850 ((chosen-1)*6+column) else 0
      where chosen=if kind==0 then kb 0xd6a5c4 (w 0x104+stage*8) else kb 0xd6a5f2 (stage*43+kind)
    preset=case cfg 0xa94e of 1->0; 2->1; _->w 0x1bc
    presetMutation=if cfg 0xa94e==1 || cfg 0xa94e==2 then [word (plus descriptorAddress 0x1bc) preset] else []
    level=fromIntegral (cfgHalf cfg (romHalf images (0xd694b8+w 0x158*2))) :: Integer
    gain | w 0x22c/=1 = toInteger (h 0x1a4)
         | otherwise = selectedProduct*1953 `div` 10000
      where selectedProduct=foldl' (\previous index -> if B.head (sourceBytes images (0xd69640+index*2) 1)==byteAt descriptor 0x1a2 then toInteger (B.head (sourceBytes images (0xd69641+index*2) 1))*toInteger (h 0x1a0) else previous) 0 [0..6]
    divisor | w 0x22c==1 = max 1 (level*1953 `div` 10000)
            | rounded<=4089 = toInteger (romHalf images (0xd6916c+fromIntegral (rounded `div` 10)*2))
            | otherwise = 0x12cc6
      where rounded=((level+5) `div` 10)*10
    ratio=gain*100 `div` divisor
    noiseBand=if ratio<50 then 0 else 1+fromIntegral (length (takeWhile (<=ratio) [toInteger (B.head (sourceBytes images (0xd69768+index) 1)) | index<-[1..7]]))
    controls=keyHalves images cfg (0xd696e8+preset*6) 3
    signedControls=keyHalves images cfg (0xd694cc+(w 0x158+preset*10)*16) 8
    signedPair value=if value .&. 0xe000/=0 then value .|. 0xe000 else value
    noiseControls=presetMutation++[putBytes 4 [cfg 0xa94d,fromIntegral (min 255 (((kb 0xd69770 (w 0xf4)*kb 0xd696d4 noiseBand) `shiftR` 6) .&. 65535))],putHalves 6 [cfgHalf cfg 0xa947,min 8191 (controls!!0),scaled (controls!!1),scaled (controls!!2)],putHalves 22 [cfgHalf cfg 0xa94b,cfgHalf cfg 0xa949],putHalves 26 (concat [[signedControls!!(pair*2),signedPair (signedControls!!(pair*2+1))] | pair<-[0..3]]),putHalves 42 [256]]
      where scaled value=fromIntegral (min 65535 (gain*toInteger value `div` 100))
    noiseGains | w 0x18c==1 = [256,256,256]
               | otherwise = [first+middle,middle,third+middle]
      where
        initial=kb 0xd6911c (w 0xf4+w 0x160*10)
        next=kb 0xd6911c (w 0xf4+w 0x160*10+1)
        remainder=fromIntegral ((fromIntegral (w 0x130) :: Int32) `rem` 100) :: Word32
        interpolation=if w 0xf4==9 || initial>=next then initial else (initial+((next-initial)*remainder*cb 0xa99c) `div` 6400) .&. 65535
        special=w 0xe4==53 && w 0x17c==0
        base=if special then (interpolation-cb 0xa99b) .&. 65535 else interpolation
        kindGain=if w 0x194<5 && testBit (27::Word32) (fromIntegral (w 0x194)) then kb 0xd696f4 (w 0xf4+w 0x194*10) else 64
        matrixGain=if w 0x104>=2 && w 0x104<=7 then kb 0xd69758 (w 0x104) else 64
        alternate=if w 0x164==1 then cb 0xa99e else 64
        prefix=product (map toInteger [base,cb 0xa99d,kb 0xd69634 (w 0x154),kb 0xd694a0 (modeValue cfg descriptor 11),kindGain,matrixGain]) `div` 16777216
        middle=fromInteger (prefix*toInteger (kb 0xd69620 (w 0x13c))*toInteger (kb 0xd69660 (modeIdentifier descriptor))*toInteger alternate `div` 16777216) :: Word32
        first=if special then (kb 0xd69650 (w 0x160*2)+cb 0xa999) .&. 65535 else kb 0xd69650 (w 0x160*2)
        third=if special then (kb 0xd69650 (w 0x160*2+1)+cb 0xa99a) .&. 65535 else kb 0xd69650 (w 0x160*2+1)
    priority | w 0x170==1 = 5 | w 0x170==2 = 6 | w 0x174==1 = 2 | w 0x174==2 = 4 | w 0x160==1 = 3 | w 0x160==2 = 1 | otherwise = 0
    thresholdEffects=[putHalves 14 firstCurve,putHalves 70 secondCurve,putHalves 12 [fromIntegral (cb firstKey)],putHalves 10 [fromIntegral (cb secondKey)],putHalves 8 [cfgHalf cfg thirdKey],putBytes 1 [cfg 0xc0a4,cfg 0xc0a4],putHalves 6 [h 0x168],putHalves 4 [h 0x16a],putHalves 126 [if w 0x28==0 then 29 else 17]]
      where
        index=fromIntegral (byteAt descriptor 0x16c)
        value=fromIntegral (w 0x230) :: Int32
        thresholds=[fromIntegral (cfgHalf cfg (romWord images (0x829ad8+index*16+lane*4))) :: Int32 | lane<-[0..3]]
        band=fromIntegral (length (takeWhile (<value) thresholds))
        base=0x408a00+(band*15+index)*112
        firstCurve=[cfgHalf cfg (base+lane*2) | lane<-[0..27]]
        secondCurve=[cfgHalf cfg (base+56+lane*2) | lane<-[0..27]]
        row=(if w 0x164==1 then 0xd69b68 else 0xd69bbc)+priority*12
        firstKey=romWord images row
        secondKey=romWord images (row+4)
        thirdKey=romWord images (row+8)
    exported | cfg 0xb5==1 = (exportCoefficientCurve images (cb 0xc656),[bytes (plus descriptorAddress 0x199) [255]])
             | cfg 0xcb74==1 = (exportCoefficientCurve images (cb 0xc657),[bytes (plus descriptorAddress 0x199) [255]])
             | direct = (exportCoefficientCurve images (exportIdentifier gridIdentifier),[])
             | stage==1 || (stage/=0 && w 0x140==7) = (exportCoefficientCurve images selectedFirst,[selectedMutation])
             | otherwise = (zipWith blend firstCurve secondCurve,[selectedMutation,halves (Address 0x1467ab4) firstCurve,halves (Address 0x1467a88) secondCurve])
      where
        kind=kindMode descriptor
        branch=stage>1 || w 0xe4 `elem` [1,2,3,4,16,19,54,55] || (w 0xe4 `elem` [0,53] && w 0xec==0)
        direct=branch && w (if stage>1 then 0xec else 0xe8)==0
        gridFirst=if stage==2 && w 0x124==7 then kb 0xd6a6f8 (w 0x104) else kb 0xd6a708 (w 0x104+stage*8)
        gridIdentifier=kb 0xd6acec (w 0xfc*5+(gridFirst-1)*25+w 0x100)
        selectedFirst=exportIdentifier (kb 0xd6a736 (stage*43+kind))
        selectedSecond=exportIdentifier (kb 0xd6a4be (stage*43+kind))
        selectedMutation=bytes (plus descriptorAddress 0x199) [fromIntegral kind]
        firstCurve=exportCoefficientCurve images selectedFirst
        secondCurve=exportCoefficientCurve images selectedSecond
        blend a b=fromIntegral ((toInteger a*128+(toInteger b-toInteger a)*toInteger (byteAt descriptor 0x198)+64) `div` 128)

routes :: [(Word32,String,CoefficientPart)]
routes = [(0xd675a4,"expand_matrix_coefficients",MatrixCoefficients),(0xd67898,"expand_blended_lookup",BlendedLookup),(0xd679f8,"expand_paired_lookup",PairedLookup),(0xd67c90,"expand_sensor_layout",SensorLayout),(0xd67d80,"expand_signed_channels",SignedChannels),(0xd6893c,"expand_auxiliary_channels",AuxiliaryChannels),(0xd68a68,"expand_noise_controls",NoiseControls),(0xd68da8,"expand_noise_coefficients",NoiseCoefficients),(0xd697f0,"expand_threshold_curves",ThresholdCurves),(0xd69f8c,"expand_selected_lookup",SelectedLookup),(0xd6a11c,"expand_lookup_controls",LookupControls),(0xd6a3f0,"expand_export_coefficients",ExportCoefficients)]
publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++name))
outputCoefficientPublications :: [StagePublication]
outputCoefficientPublications = [publication address name | (address,name,_)<-routes]++[publication address name | (address,name)<-[(0x690924,"lookup_coefficient_profile"),(0x690b74,"lookup_export_curve"),(0xd6703c,"store_matrix_coefficients"),(0xd670a0,"read_matrix_coefficients"),(0xd671b4,"build_matrix_coefficients"),(0xd675d8,"build_blended_lookup"),(0xd66eac,"select_parameter_coefficient"),(0xd69784,"threshold_priority"),(0xd69c10,"blend_export_curves"),(0xd69c84,"select_export_coefficient"),(0xd69cd8,"build_selected_export"),(0xd69df0,"build_grid_export"),(0xd6a250,"build_export_coefficients")]]

apply :: Address -> B.ByteString -> [Patch] -> B.ByteString
apply (Address base) = foldl' step
  where
    step original (Patch (Address address) content)
      | address>=base && address+fromIntegral (B.length content)<=base+fromIntegral (B.length original) = let offset=fromIntegral (address-base) in B.take offset original<>content<>B.drop (offset+B.length content) original
      | otherwise = original

outputCoefficientScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
outputCoefficientScenarios bundle _resolve = rootCases++lookupCases++priorityCases++matrixAliasCases
  where
    images=CoefficientSources (B.take 2631680 bundle) (B.drop 2631680 bundle)
    originalRanges=[(0x824bc4,2668),(0x825630,10852),(0x829ad8,240),(0xd67e64,2776),(0xd689a4,120),(0xd6911c,1640),(0xd69b68,168),(0xd6a4be,2394)]
    original=[Patch (Address address) (sourceBytes images address count) | (address,count)<-originalRanges]
    descriptorAddress=Address 0x9a0000
    blockAddress=Address 0x9a1000
    descriptor variant=apply descriptorAddress (B.replicate 580 0)
      ([word (plus descriptorAddress offset) value | (offset,value)<-
        [(0x28,if variant==3 then 1 else 0),(0x130,if variant==2 then 0xffffffdf else 50),
         (0x140,if variant==1 || variant==4 then 7 else if variant==2 then 5 else 0),
         (0xe4,if variant==2 then 24 else if variant==3 then 53 else 0),
         (0xf4,if variant==3 then 9 else 0),(0x160,if variant==1 then 3 else 0),
         (0x170,if variant==2 then 1 else 0),(0x174,if variant==4 then 2 else 0),
         (0x1a8,if variant==1 then 0 else 10),(0x22c,if variant==3 then 1 else 0),
         (0x18c,if variant==4 then 1 else 0),(0x230,if variant==4 then 0xffffffff else if variant==3 then 0x7fffffff else 100),
         (0x164,if variant==2 then 1 else 0)]]++
       [halves (plus descriptorAddress 0x118) [65535],halves (plus descriptorAddress 0x168) [123,456],
        halves (plus descriptorAddress 0x1a0) [1000],halves (plus descriptorAddress 0x1a4) [300,if variant==4 then 0 else if variant==2 then 65535 else 200],
        halves (plus descriptorAddress 0x1ea) [if variant==1 then 950 else 46],
        bytes (plus descriptorAddress 0x198) [64,0xab],bytes (plus descriptorAddress 0x1ac) [1,127],
        bytes (plus descriptorAddress 0x1c4) [0x99,0x88,0x77]])
    configuration flavor key
      | key==0xb5 = if flavor==2 then 1 else 0
      | key==0xcb74 = if flavor==3 then 1 else 0
      | key==0xa923 = if flavor>=2 then 1 else 0
      | key>=0x408a00 = fromIntegral (key*17+fromIntegral flavor*31)
      | key==0xa94e = if flavor==0 then 1 else 2
      | key==romHalf images 0xd6891c || key==romHalf images 0xd67e64 = 1
      | flavor==0 = 3
      | otherwise = fromIntegral (key*17+127)
    configurationPatches flavor=
      [word (Address 0x96ca50) 0x10000000,
       Patch (Address 0x10000000) (B.pack [configuration flavor key | key<-[0..0x20000]]),
       Patch (Address 0x103e3d50) (B.pack [configuration flavor key | key<-[0x3e3d50..0x3e3dc7]]),
       Patch (Address 0x10408a00) (B.pack [configuration flavor key | key<-[0x408a00..0x412000]])]
    rootCases=[let input=descriptor variant; cfg=configuration flavor; expected=coefficientEffects images cfg descriptorAddress input blockAddress part
              in StageScenario (StageCaseName ("output-coefficients-"++show part++"-variant-"++show variant++"-configuration-"++show flavor)) (publication address name) [0x9a0000,0x9a1000]
                   (original++configurationPatches flavor++[Patch descriptorAddress input,Patch blockAddress (B.replicate 256 0xa5),Patch (Address 0x1467a88) (B.replicate 88 0x5a)])
                   ([Patch descriptorAddress (apply descriptorAddress input expected),Patch blockAddress (apply blockAddress (B.replicate 256 0xa5) expected)]++[patch | patch@(Patch (Address address) _)<-expected,address>=0x1467a88 && address<0x1467ae0]) Nothing [] []
              | (address,name,part)<-routes,variant<-[0..4],flavor<-[0,1,2,3]]
    lookupCases=[let (header,curve)=coefficientProfile images identifier; found=any (\index -> romWord images (0x824bc8+index*72)==identifier) [0..romWord images 0x824bc4-1]
                    result=if curveAddress/=0 then if found then curveAddress+54 else 62 else if headerAddress/=0 && found then headerAddress+8 else identifier
                in StageScenario (StageCaseName ("coefficient-profile-"++show identifier++"-"++show headerAddress++"-"++show curveAddress)) (publication 0x690924 "lookup_coefficient_profile") [identifier,headerAddress,curveAddress] original
                     ([bytes (Address headerAddress) header | headerAddress/=0]++[halves (Address curveAddress) curve | curveAddress/=0]) (Just result) [] []
                | identifier<-[0,1,36,0xffffffff],headerAddress<-[0,0x9a2000],curveAddress<-[0,0x9a2100]]++
                [StageScenario (StageCaseName ("export-curve-"++show identifier)) (publication 0x690b74 "lookup_export_curve") [identifier,0x9a2000] original [halves (Address 0x9a2000) (exportCoefficientCurve images identifier)] Nothing [] [] | identifier<-[0,1,225,0xffffffff]]
    priorityCases=
      [let expected=case lookup primary [(1,5),(2,6)] of
             Just value -> value
             Nothing -> case lookup secondary [(1,2),(2,4)] of
               Just value -> value
               Nothing -> maybe 0 id (lookup readout [(1,3),(2,1)])
       in StageScenario (StageCaseName ("threshold-priority-"++show primary++"-"++show secondary++"-"++show readout))
            (publication 0xd69784 "threshold_priority") [0x9a0000]
            [Patch descriptorAddress (B.replicate 580 0),word (plus descriptorAddress 0x170) primary,
             word (plus descriptorAddress 0x174) secondary,word (plus descriptorAddress 0x160) readout]
            [] (Just expected) [] []
      | primary<-[0,1,2,3],secondary<-[0,1,2,3],readout<-[0,1,2,3]]
    matrixAliasCases=
      [StageScenario (StageCaseName "matrix-scatter-overlapping-packet")
        (publication 0xd6703c "store_matrix_coefficients") [0x9a202a,0x9a2000]
        [Patch (Address 0x9a2000) (B.replicate 64 0xa5),
         bytes (Address 0x9a202a) [1,0xa5],halves (Address 0x9a202c) [11,22,33,44,55,66,77,88,99]]
        [bytes (Address 0x9a202a) [1,0xa5],halves (Address 0x9a202c) [1,1,22,22,22,55,55,55,88,88]] Nothing [] []]
