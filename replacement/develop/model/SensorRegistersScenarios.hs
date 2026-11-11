{-# LANGUAGE GHC2021 #-}
module SensorRegistersScenarios where
import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int16,Int8)
import Data.List (foldl')
import Data.Word
import PipelineModel
import Scenarios (Address(..),Patch(..),word,wordBytes,wordsAt)

sensorRegisterPublication :: Word32 -> String -> StagePublication
sensorRegisterPublication address name=StagePublication (Address address) (StageSymbol ("xm1_pipeline_sensor_"++name))

data RegisterPacket = InputPacket | OriginPacket | SecondaryTablePacket | DimensionPacket | RegionPacket | CoefficientPacket | ControlPacket | FilterPacket | WindowPacket | SnapshotPacket | PrimaryTablePacket deriving (Eq,Show,Enum,Bounded)
data RegisterField = RegisterField Word32 Int Int Word32 deriving Show

packetHalf :: B.ByteString -> Word32 -> Word32
packetHalf bytes offset=fromIntegral (B.index bytes (fromIntegral offset)) .|. (fromIntegral (B.index bytes (fromIntegral offset+1)) `shiftL` 8)

registerFields :: RegisterPacket -> B.ByteString -> [RegisterField]
registerFields packet bytes=case packet of
 InputPacket -> [u 0 0 1 0 1,u 0 16 2 1 2,u 0 24 1 2 1,u 4 0 7 4 81,b 5 5,h 4 16 14 6 16383,h 8 0 13 8 8191,rh 10 10,rh 12 12,h 16 0 9 14 256,h 16 16 9 16 256,h 20 0 9 18 256,b 22 20,h 24 0 14 24 16383,h 24 16 14 22 16383]++[rh (30+n*4) (26+n*4)|n<-[0..3]]++[s (28+n*4) 0 14 (28+n*4)|n<-[0..3]]++[raw 44 0 15 (packetHalf bytes 42)]
 OriginPacket -> concat [[u (bank*12) (fromIntegral n*8) 1 (bank*9+n) 1|n<-[0..3]]++[b (bank*12+4) (bank*9+4)]++[u (bank*12+8) (fromIntegral n*8) 6 (bank*9+5+n) 63|n<-[0..3]]|bank<-[0..1]]
 SecondaryTablePacket -> [u 0 0 1 0 1]++[b (4+n) (1+n)|n<-[0..3]]++[b 8 5]++[u (12+n `div` 4*4) (fromIntegral (n `mod` 4)*8) 5 (6+n) 31|n<-[0..7]]
 DimensionPacket -> [u 0 0 2 0 2,raw 0 8 1 (v 1)]++[s (4+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 10 (2+n*2)|n<-[0..9]]++[b (24+n) (22+n)|n<-[0..2]]++[u 28 (fromIntegral n*8) 6 (25+n) 63|n<-[0..3]]++concat [[b (d+n) (a+n)|n<-[0..count-1]]|(d,a,count)<-[(32,29,2),(36,31,4),(40,35,2),(44,37,1),(56,43,3),(60,46,4),(64,50,4)]]++[h 48 0 13 38 8191,h 48 16 13 40 8191,u 52 0 4 42 15,b 70 55,b 68 54]++[u (72+n `div` 4*4) (fromIntegral (n `mod` 4)*8) 5 (56+n) 31|n<-[0..9]]++[b 84 66,u 84 16 6 67 63]
 RegionPacket -> [u 0 (fromIntegral n*4) 1 n 1|n<-[0..6]]++[u 0 28 3 7 4]++[sh (4+n*2) 11 (8+n*2)|n<-[0..14]]++[sh (36+n*2) 13 (38+n*2)|n<-[0..3]]++[b (44+n) (46+n)|n<-[0..4]]++concat [[b (48+n*4) (50+n*4),h (48+n*4) 16 10 (52+n*4) 1023]|n<-[0..4]]++[sh (68+n*2) 10 (70+n*2)|n<-[0..1]]++[h 72 0 11 74 2047]++concat [[s 76 (fromIntegral n*16) 13 (76+n*2),s 80 (fromIntegral n*16) 10 (80+n*2)]|n<-[0..1]]++[h 84 0 11 84 2047]++concat [[s 88 (fromIntegral n*16) 13 (86+n*2),u 92 (fromIntegral n*16) 5 (90+n) 31]|n<-[0..1]]++[b (96+n) (92+n)|n<-[0..3]]++concat [[s (100+n*4) 0 11 (96+n*4),h (100+n*4) 16 10 (98+n*4) 1023]|n<-[0..4]]++[b (120+n) (116+n)|n<-[0..2]]
 CoefficientPacket -> [u 0 (fromIntegral n*8) 1 n 1|n<-[0..2]]++[u (4+n `div` 4*4) (fromIntegral (n `mod` 4)*8) 5 (3+n) 31|n<-[0..7]]++[b 12 11,b 13 12]++concat [[sh (16+n*2) 9 (14+n*4),sb (16+n*2) 9 6 (16+n*4)]|n<-[0..2]]++[b (24+n) (26+n)|n<-[0..9]]++concat [[b (36+n*2) (36+n*2),sb (36+n*2) 8 7 (37+n*2)]|n<-[0..2]]
 ControlPacket -> [u 0 0 2 0 2,u 0 8 5 1 16,u 0 16 3 2 7]++[u 4 (fromIntegral n*8) 6 (3+n) 32|n<-[0..2]]++[signedByte 4 24 6 6]++[h (8+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 13 (8+n*2) 8191|n<-[0..21]]++[u (52+n `div` 8*4) (fromIntegral (n `mod` 8)*4) 3 (52+n) 7|n<-[0..8]]++[h (60+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 14 (62+n*2) 16383|n<-[0..27]]
 FilterPacket -> [u (if n<6 then n `div` 3*4 else 8) (fromIntegral (if n<6 then n `mod` 3 else n-6)*8) 3 n 7|n<-[0..9]]++[s 12 0 12 10,s 12 16 12 12,h 16 0 12 14 4095,u 16 16 3 16 7]++[u d shift 5 (18+n) 31|(n,d,shift)<-[(0,20,0),(1,20,8),(2,20,16),(3,20,24),(4,24,0),(5,24,8),(6,24,16),(7,28,0),(8,28,8),(9,32,0),(10,32,8),(11,32,16),(12,32,24)]]++[rh 36 32,rh 40 34,rh 42 36]++[u 44 (fromIntegral n*8) 5 (40-n) 31|n<-[0..2]]++[u 48 (fromIntegral n*8) 5 n 16|n<-[0..2]]++[raw 52 0 8 (min 255 (packetHalf bytes 4)),raw 52 16 8 (min 255 (packetHalf bytes 6)),u 56 0 1 8 1]++[u 60 (fromIntegral (3-n)*8) 1 (41+n) 1|n<-[0..3]]++[s (64+bank*8+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 13 (46+bank*6+n*2)|bank<-[0..2],n<-[0..2]]++[u (88+n `div` 8*4) (fromIntegral (n `mod` 8)*4) 3 (64+n) 7|n<-[0..8]]++[h (96+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 14 (74+n*2) 16383|n<-[0..27]]++[h 152 16 14 130 16383,h 152 0 14 132 16383]++[h (156+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 14 (134+n*2) 16383|n<-[0..55]]
 WindowPacket -> [raw 0 0 1 (v 0),u 0 16 4 1 15,u 0 20 4 2 15,h 4 0 13 4 8191,h 4 16 13 6 8191,h 8 0 13 8 8191,raw 8 16 6 (max 1 (min 63 (packetHalf bytes 10))),raw 8 24 6 (max 1 (min 63 (packetHalf bytes 12)))]++[s (12+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 10 (14+n*2)|n<-[0..55]]++[h 124 0 6 126 63]
 PrimaryTablePacket -> [u 0 0 1 0 1]++concat [[h 4 (fromIntegral n*16) 11 (2+n*2) 1024,h 8 (fromIntegral n*16) 14 (6+n*2) 16383,h 12 (fromIntegral n*16) 10 (10+n*2) 1023]|n<-[0..1]]++[raw 16 0 8 (fromIntegral (min 0 (fromIntegral (v 14)::Int8)))]++[b (17+n) (15+n)|n<-[0..2]]++[b 20 18,h 24 0 10 20 1023,b 28 22,raw 28 8 8 (min 128 (v 23)),raw 28 16 10 (fromIntegral (max (-512) (min 128 (fromIntegral (fromIntegral (packetHalf bytes 24)::Int16)::Integer))))]++concat [[s (32+bank*164+n*4) 16 11 (26+bank*164+n*4),s (32+bank*164+n*4) 0 11 (28+bank*164+n*4)]|bank<-[0..19],n<-[0..40]]
 SnapshotPacket -> snapshotFields bytes
 where
  raw=RegisterField
  v offset=fromIntegral (B.index bytes (fromIntegral offset))
  u destination shift width source maximum=raw destination shift width (min maximum (v source))
  h destination shift width source maximum=raw destination shift width (min maximum (packetHalf bytes source))
  b destination source=raw (destination .&. complement 3) (fromIntegral (destination .&. 3)*8) 8 (v source)
  rh destination source=raw (destination .&. complement 3) (fromIntegral (destination .&. 3)*8) 16 (packetHalf bytes source)
  s destination shift width source=raw destination shift width (fromIntegral (signedLimit width (fromIntegral (fromIntegral (packetHalf bytes source)::Int16))))
  sh destination width source=s (destination .&. complement 3) (fromIntegral (destination .&. 3)*8) width source
  signedByte destination shift width source=raw destination shift width (fromIntegral (signedLimit width (fromIntegral (fromIntegral (v source)::Int8))))
  sb destination shift width source=signedByte (destination .&. complement 3) (fromIntegral (destination .&. 3)*8+shift) width source

signedLimit :: Int -> Integer -> Integer
signedLimit width value=max (negate (2^(width-1))) (min (2^(width-1)-1) value)

packRegisters :: Word32 -> [RegisterField] -> [Word32]
packRegisters size fields=[foldl' apply 0xa55af00f [f|f@(RegisterField location _ _ _)<-fields,location==offset]|offset<-[0,4..size-4]]
 where apply old (RegisterField _ shift width value)=let mask=(2^width-1) `shiftL` shift in (old .&. complement mask) .|. ((value `shiftL` shift) .&. mask)

packetRoute :: RegisterPacket -> (Word32,String,Word32)
packetRoute packet=case packet of
 InputPacket -> (0x6961f0,"pack_input",48)
 OriginPacket -> (0x6968b8,"pack_origin",24)
 SecondaryTablePacket -> (0x6991dc,"pack_secondary_table",20)
 DimensionPacket -> (0x68de38,"pack_dimensions",88)
 RegionPacket -> (0x68f694,"pack_region",124)
 CoefficientPacket -> (0x690ce4,"pack_coefficients",44)
 ControlPacket -> (0x695aa4,"pack_control",116)
 FilterPacket -> (0x695718,"pack_first_filter",268)
 WindowPacket -> (0x68f050,"pack_window",128)
 SnapshotPacket -> (0x693c94,"pack_snapshot",384)
 PrimaryTablePacket -> (0x696db8,"pack_primary_table",3312)

sensorRegisterPublications :: [StagePublication]
sensorRegisterPublications=primaryTablePreparationPublication:[sensorRegisterPublication address symbol|packet<-[minBound..maxBound],let (address,symbol,_)=packetRoute packet]++[sensorRegisterPublication 0x68ef08 "clip_signed_field",sensorRegisterPublication 0x6deb08 "apply_register_transfers",sensorRegisterPublication 0x6deb7c "capture_register_transfers"]

primaryTablePreparationPublication :: StagePublication
primaryTablePreparationPublication=sensorRegisterPublication 0x69853c "prepare_primary_table"

sensorRegisterScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
sensorRegisterScenarios _ _=packingCases++clippingCases++transferCases++preparationCases
 where
  input=0x9b0000;output=0x9b4000;mapping=0x9b8000
  make name address symbol arguments initial final result=StageScenario (StageCaseName ("sensor-register-"++name)) (sensorRegisterPublication address symbol) arguments initial final result [] []
  packingCases=[make (show (packet,seed)) address symbol [output,input]
    [Patch (Address input) bytes,wordsAt output (replicate (fromIntegral size `div` 4) 0xa55af00f)]
    [wordsAt output (packRegisters size (registerFields packet bytes))] Nothing
    |packet<-[minBound..maxBound],let(address,symbol,size)=packetRoute packet,seed<-[0,1,127,255],let bytes=B.pack [fromIntegral ((n*37+seed) `mod` 256)|n<-[0..4095::Int]]]
  clippingCases=[make ("clipping-"++show (width,value)) 0x68ef08 "clip_signed_field" [fromIntegral value,fromIntegral width,output] [] [word output packed] (Just (if toInteger value==limited then 1 else 0))
    |width<-[6,8,10,11,13,14],value<-[-32768,-8192,-512,-1,0,31,511,8192,32767::Int],let limited=signedLimit width (toInteger value),let packed=fromIntegral limited .&. (2^width-1)]
  transferCases=[make "ordered-scatter" 0x6deb08 "apply_register_transfers" [input,mapping,3]
    [wordsAt input [0x11223344,0x55667788,0x99aabbcc],wordsAt mapping [output,4,2,output+4,0,1,output+16,0,0],wordsAt output [0,0,0,0,0]]
    [wordsAt output [0x55667788,0x11223344,0,0,0]] Nothing,
    make "ordered-gather" 0x6deb7c "capture_register_transfers" [output,mapping,2]
    [wordsAt input [0x11223344,0x55667788,0x99aabbcc],wordsAt mapping [input,4,2,input+8,0,1],wordsAt output [0,0,0]]
    [wordsAt output [0x99aabbcc,0x11223344,0x55667788]] Nothing]
  preparationCases=[make "curve-pair-order" 0x69853c "prepare_primary_table" [input]
    [Patch (Address input) (B.replicate 0x1000 0xaa),word (input+0xcec) mapping,Patch (Address mapping) curve]
    [Patch (Address (input+26)) ordered] Nothing]
  curve=B.concat [B.take 2 (wordBytes n)|n<-[0..1619]]
  ordered=B.concat [B.concat [B.take 2 (wordBytes (bank*81+n*2+1))<>B.take 2 (wordBytes (bank*81+n*2))|n<-[0..39]]<>B.replicate 2 0<>B.take 2 (wordBytes (bank*81+80))|bank<-[0..19]]

snapshotFields :: B.ByteString -> [RegisterField]
snapshotFields bytes=
 [u 0 0 6 0 45]++[u 0 shift 1 source 1|(source,shift)<-[(1,8),(2,12),(3,16),(4,20),(5,24)]]++
 [u 4 shift 1 source 1|(source,shift)<-[(6,4),(7,8),(8,12),(9,16),(10,20),(11,22),(12,24),(13,28)]]++
 [u 4 30 2 14 3,u 8 0 1 15 1,u 8 8 1 17 1,u 8 4 2 16 3,u 8 12 2 18 2,u 12 0 1 19 1,u 12 16 1 20 1,u 16 0 7 21 81,b 18 22,h 20 0 13 24 8191,rh 22 26,rh 24 28,h 24 16 14 30 16383,h 28 0 9 32 256,h 28 16 9 34 256,h 32 0 9 36 256,b 34 38,h 36 0 14 40 16383,h 36 16 14 42 16383]++
 concat [[s (40+n*4) 0 14 (44+n*4),rh (42+n*4) (46+n*4)]|n<-[0..3]]++[h 56 0 15 60 32767]++
 [u 68 (fromIntegral n*8) 4 (68+n) 15|n<-[0..2]]++[b (72+n) (71+n)|n<-[0..3]]++
 [u 76 0 3 75 7,u 76 8 3 76 7,b 78 77,b 79 78]++[u 80 (fromIntegral n*8) 6 (79+n) 32|n<-[0..2]]++[sb 80 24 6 82]++
 [h (84+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 13 (84+n*2) 8191|n<-[0..23]]++
 [s (128+bank*8+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 13 (128+bank*6+n*2)|bank<-[0..2],n<-[0..2]]++
 [u (152+n `div` 8*4) (fromIntegral (n `mod` 8)*4) 3 (146+n) 7|n<-[0..8]]++
 [h (160+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 14 (156+n*2) 16383|n<-[0..29]]++
 [u 220 (fromIntegral n*8) 4 (216+n) 15|n<-[0..2]]++
 [s (224+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 13 (220+n*2)|n<-[0..3]]++
 [b (232+n) (228+n)|n<-[0..3]]++concat [[b (236+n*4) (232+n*4),h (236+n*4) 16 10 (234+n*4) 1023]|n<-[0..4]]++
 concat [[s (256+bank*12) (fromIntegral n*16) 10 (252+bank*10+n*2)|n<-[0..1]]++[h (260+bank*12) 0 11 (256+bank*10) 2047]++[s (264+bank*12) (fromIntegral n*16) 13 (258+bank*10+n*2)|n<-[0..1]]|bank<-[0..1]]++
 [u 280 0 5 272 31,u 280 16 5 273 31]++[b (284+n) (274+n)|n<-[0..3]]++
 concat [[s (288+n*4) 0 11 (278+n*4),h (288+n*4) 16 10 (280+n*4) 1023]|n<-[0..4]]++
 [u 308 (fromIntegral n*8) 6 (298+n) 63|n<-[0..3]]++
 [s (312+n `div` 2*4) (fromIntegral (n `mod` 2)*16) 10 (302+n*2)|n<-[0..11]]++
 [b (336+n) (326+n)|n<-[0..2]]++[u 340 (fromIntegral n*8) 6 (329+n) 63|n<-[0..3]]++[b (344+n) (333+n)|n<-[0..1]]++
 [b 348 335,b 350 337,sb 348 8 8 336,sb 348 24 8 338,b 352 339,sb 352 8 8 340,b 356 341]++
 [u (360+n `div` 4*4) (fromIntegral (n `mod` 4)*8) 5 (342+n) 31|n<-[0..5]]++
 [b 368 348,u 368 16 6 349 63,raw 372 0 8 (min 128 (v 350)),raw 372 8 8 (min 128 (v 351)),b 374 352,b 376 353,u 376 16 4 354 15,u 376 24 4 355 15]++[b (380+n) (356+n)|n<-[0..2]]
 where
  raw=RegisterField
  v offset=fromIntegral (B.index bytes (fromIntegral offset))
  u destination shift width source maximum=raw destination shift width (min maximum (v source))
  h destination shift width source maximum=raw destination shift width (min maximum (packetHalf bytes source))
  b destination source=raw (destination .&. complement 3) (fromIntegral (destination .&. 3)*8) 8 (v source)
  rh destination source=raw (destination .&. complement 3) (fromIntegral (destination .&. 3)*8) 16 (packetHalf bytes source)
  s destination shift width source=raw destination shift width (fromIntegral (signedLimit width (fromIntegral (fromIntegral (packetHalf bytes source)::Int16))))
  sb destination shift width source=raw destination shift width (fromIntegral (signedLimit width (fromIntegral (fromIntegral (v source)::Int8))))
