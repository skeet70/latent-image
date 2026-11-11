module OutputExpansionPairLaw
  ( PairPacket(..), CoefficientQuad(..), BasePairFactors(..), TemplatePairFactors(..)
  , pairPacket, extendedPairPacket, pairValues, pairPacketPatches
  , pairBlockPatches, extendedPairBlockPatches, signedPair, extendedPair
  , PairLawCase(..), pairLawCases, pairLawCaseResult
  , pairLawPublications, pairLawScenarios
  ) where

import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Int (Int64)
import Data.List (foldl')
import Data.Word (Word8, Word16, Word32, Word64)
import OutputCoefficientsScenarios (CoefficientSources(..))
import PipelineModel
import Scenarios (Address(..), Patch(..), word, wordBytes)

data PairPacket = PairPacket
  { pairEnabled :: Word8
  , pair00 :: Word16
  , pair11 :: Word16
  , pair01 :: Word16
  , pair10 :: Word16
  } deriving (Eq, Show)

data CoefficientQuad = CoefficientQuad
  { coefficient00 :: Word16
  , coefficient01 :: Word16
  , coefficient10 :: Word16
  , coefficient11 :: Word16
  } deriving (Eq, Show)

data BasePairFactors = BasePairFactors
  { factor10c :: Word8
  , factorF4And160 :: Word8
  , factor104And154 :: Word8
  , factor13c :: Word8
  , factorModeIdentifier :: Word8
  } deriving (Eq, Show)

data TemplatePairFactors = TemplatePairFactors
  { factor128 :: Word8
  , factorTypeAndDc :: Word8
  , factor148 :: Word8
  , factorType6 :: Word8
  } deriving (Eq, Show)

pairValues :: PairPacket -> [Word16]
pairValues packet = [pair00 packet,pair11 packet,pair01 packet,pair10 packet]

baseProduct :: BasePairFactors -> Integer
baseProduct factors = product (map toInteger
  [factor10c factors,factorF4And160 factors,factor104And154 factors,factor13c factors,factorModeIdentifier factors])

templateProduct :: TemplatePairFactors -> Integer
templateProduct factors = product (map toInteger [factor128 factors,factorTypeAndDc factors,factor148 factors,factorType6 factors])

decodeCoefficient :: Word16 -> Integer
decodeCoefficient value
  | value < 4096 = toInteger value
  | otherwise = toInteger (value .&. 4095)-4096

signed64 :: Integer -> Integer
signed64 value = toInteger (fromIntegral (fromInteger value :: Word64) :: Int64)

clampCoefficient :: Integer -> Word16
clampCoefficient = fromInteger . max (-4096) . min 4095

packetFrom :: (Word16 -> Word16) -> CoefficientQuad -> PairPacket
packetFrom transform coefficients = PairPacket 1
  (transform (coefficient00 coefficients)) (transform (coefficient11 coefficients))
  (transform (coefficient01 coefficients)) (transform (coefficient10 coefficients))

signedPair :: CoefficientQuad -> BasePairFactors -> PairPacket
signedPair coefficients factors = packetFrom convert coefficients
  where convert value = clampCoefficient (signed64 (decodeCoefficient value * baseProduct factors) `shiftR` 34)

extendedPair :: CoefficientQuad -> BasePairFactors -> TemplatePairFactors -> PairPacket
extendedPair coefficients base template = packetFrom convert coefficients
  where
    convert value = clampCoefficient (signed64 (intermediate value * templateProduct template) `shiftR` 34)
    intermediate value = signed64 (decodeCoefficient value * baseProduct base) `shiftR` 28

atWord :: B.ByteString -> Int -> Word32
atWord bytes offset = B.foldr (\byte rest -> fromIntegral byte + (rest `shiftL` 8)) 0 (B.take 4 (B.drop offset bytes))

sourceHalf :: CoefficientSources -> Word32 -> Word32
sourceHalf sources address
  | offset < 0 || offset+2 > B.length image = error "pair law requires original source bytes at the addressed calibration-key table"
  | otherwise = fromIntegral (B.index image offset) + fromIntegral (B.index image (offset+1))*256
  where
    (image,base) = if address >= 0xbf1a00 then (coefficientImage5 sources,0xbf1a00) else (coefficientImage4 sources,0x61c400)
    offset = fromIntegral (address-base)

keyed :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> Word32 -> Word8
keyed sources cfg table index = cfg (sourceHalf sources (table+index*2))

cfgHalf :: (Word32 -> Word8) -> Word32 -> Word16
cfgHalf cfg key = fromIntegral (cfg key)+fromIntegral (cfg (key+1))*256

modeIdentifier :: B.ByteString -> Word32
modeIdentifier descriptor = case lookup (w 0xe4) primary of
  Just identifier -> identifier
  Nothing -> if w 0xe8 >= 1 && w 0xe8 <= 13 then w 0xe8+33 else 0
  where
    w = atWord descriptor
    primary = [(5,1),(6,2),(37,3),(8,4),(9,5),(10,6),(14,7),(11,8),(12,8),(7,9),(13,10),(29,11),(30,12),(31,13),(32,14),(33,15),(34,16),(28,17),(25,18),(26,19),(27,20),(24,21),(23,22),(20,23),(22,24),(40,25),(41,26),(42,27),(43,28),(44,29),(45,30),(46,31),(47,32),(48,33)]

selectedMode :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Word8
selectedMode sources cfg descriptor = keyed sources cfg table (w 0x104+w 0x108*8)
  where w=atWord descriptor; table=if w 0xe4==23 then 0xd61fbc else 0xd621a0

baseFactors :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> BasePairFactors
baseFactors sources cfg descriptor = BasePairFactors
  (keyed sources cfg 0xd62194 (w 0x10c))
  (keyed sources cfg 0xd61f34 (w 0xf4+w 0x160*10))
  selection
  (keyed sources cfg 0xd61e64 (w 0x13c))
  (keyed sources cfg 0xd62134 (modeIdentifier descriptor))
  where
    w=atWord descriptor
    selected=keyed sources cfg (if w 0xe4==23 then 0xd61c60 else 0xd61e04) (w 0x104)
    selection=if selected>=1 && selected<=5 then keyed sources cfg 0xd620b0 ((fromIntegral selected-1)*5+w 0x154) else 128

templateFactors :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> TemplatePairFactors
templateFactors sources cfg descriptor = TemplatePairFactors first second third fourth
  where
    w=atWord descriptor
    kind=w 0x140
    first=case kind of
      3 -> keyed sources cfg 0xd66d0c (w 0x128)
      4 -> keyed sources cfg 0xd66d0c (w 0x128)
      5 -> 128
      6 -> 128
      _ -> keyed sources cfg 0xd6691c (w 0x128)
    second
      | (kind==3 || kind==4) && w 0xdc==1 = cfg 0xc257
      | kind==5 = cfg (if w 0xec==0 then 0xc257 else case w 0xe4 of
          5 -> 0xc258
          6 -> 0xc259
          7 -> 0xc25c
          8 -> 0xc25a
          11 -> 0xc25b
          12 -> 0xc25b
          _ -> 0xc257)
      | otherwise = 128
    third=if kind==5 then keyed sources cfg 0xd66540 (w 0x148) else 128
    fourth=if kind==6 then cfg 0xc260 else 128

rawCoefficients :: CoefficientSources -> (Word32 -> Word8) -> Word8 -> CoefficientQuad
rawCoefficients sources cfg selected = CoefficientQuad (readLane 0) (readLane 2) (readLane 3) (readLane 1)
  where
    row=0xd61e14+fromIntegral ((selected-1) :: Word8)*8
    readLane lane=cfgHalf cfg (sourceHalf sources (row+lane*2))

packetWith :: (CoefficientQuad -> BasePairFactors -> PairPacket) -> CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> PairPacket
packetWith kernel sources cfg descriptor
  | B.length descriptor < 0x244 = error "pair law requires the complete 0x244-byte descriptor input"
  | atWord descriptor 0x104==2 || atWord descriptor 0x104==3 = PairPacket 1 0 0 0 0
  | selected<1 || selected>10 = PairPacket 0 1024 1024 0 0
  | otherwise = kernel (rawCoefficients sources cfg selected) (baseFactors sources cfg descriptor)
  where selected=selectedMode sources cfg descriptor

pairPacket :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> PairPacket
pairPacket = packetWith signedPair

extendedPairPacket :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> PairPacket
extendedPairPacket sources cfg descriptor = packetWith (\coefficients factors -> extendedPair coefficients factors (templateFactors sources cfg descriptor)) sources cfg descriptor

plus :: Address -> Word32 -> Address
plus (Address address) offset = Address (address+offset)
halfPatch :: Address -> Word16 -> Patch
halfPatch address value = Patch address (B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)])

pairPacketPatches :: Address -> PairPacket -> [Patch]
pairPacketPatches output packet =
  [ Patch output (B.singleton (pairEnabled packet))
  , halfPatch (plus output 2) (pair00 packet)
  , halfPatch (plus output 6) (pair01 packet)
  , halfPatch (plus output 8) (pair10 packet)
  , halfPatch (plus output 4) (pair11 packet)
  ]

pairBlockPatches :: Address -> PairPacket -> [Patch]
pairBlockPatches block packet =
  [Patch (plus block 2) (B.singleton (pairEnabled packet))] ++
  zipWith (halfPatch . plus block) [0x26,0x28,0x2a,0x2c] [pair00 packet,pair01 packet,pair11 packet,pair10 packet]

extendedPairBlockPatches :: Address -> PairPacket -> [Patch]
extendedPairBlockPatches block packet =
  [Patch (plus block 8) (B.singleton (pairEnabled packet))] ++
  zipWith (halfPatch . plus block) [0xdc,0xde,0xe0,0xe2] [pair00 packet,pair01 packet,pair11 packet,pair10 packet]

data PairLawCase = PairLawCase
  { pairCaseName :: String
  , pairCaseCoefficients :: CoefficientQuad
  , pairCaseBaseFactors :: BasePairFactors
  , pairCaseTemplateFactors :: Maybe TemplatePairFactors
  , pairCaseExpected :: PairPacket
  } deriving (Eq, Show)

pairLawCaseResult :: PairLawCase -> PairPacket
pairLawCaseResult example = case pairCaseTemplateFactors example of
  Nothing -> signedPair (pairCaseCoefficients example) (pairCaseBaseFactors example)
  Just template -> extendedPair (pairCaseCoefficients example) (pairCaseBaseFactors example) template

pairLawCases :: [PairLawCase]
pairLawCases =
  [ PairLawCase "signed-unity" ordinary unity Nothing (PairPacket 1 1024 512 64 65504)
  , PairLawCase "signed-half-gain" ordinary (unity {factor10c=64}) Nothing (PairPacket 1 512 256 32 65520)
  , PairLawCase "signed-selection-half" ordinary (unity {factor104And154=64}) Nothing (PairPacket 1 512 256 32 65520)
  , PairLawCase "signed-negative-floor" (CoefficientQuad 8191 0 0 0) (unity {factor10c=64}) Nothing (PairPacket 1 65535 0 0 0)
  , PairLawCase "signed-encoding-boundaries" (CoefficientQuad 4095 4096 65535 2048) unity Nothing (PairPacket 1 4095 2048 61440 65535)
  , PairLawCase "signed-saturation" (CoefficientQuad 4095 4096 6144 3072) (unity {factor10c=255}) Nothing (PairPacket 1 4095 4095 61440 61456)
  , PairLawCase "signed-zero-gain" ordinary (unity {factor10c=0}) Nothing (PairPacket 1 0 0 0 0)
  , PairLawCase "extended-unity" ordinary unity (Just extraUnity) (PairPacket 1 1024 512 64 65504)
  , PairLawCase "extended-intermediate-floor" (CoefficientQuad 33 8159 0 0) (unity {factor10c=1}) (Just (TemplatePairFactors 128 255 255 128)) (PairPacket 1 0 0 65534 0)
  ]
  where ordinary=CoefficientQuad 1024 64 8160 512; unity=BasePairFactors 128 128 128 128 64; extraUnity=TemplatePairFactors 128 128 128 128

signedPairPublication, extendedPairPublication :: StagePublication
signedPairPublication = StagePublication (Address 0xd60ea8) (StageSymbol "xm1_pipeline_image_build_pair_coefficients")
extendedPairPublication = StagePublication (Address 0xd64e2c) (StageSymbol "xm1_pipeline_image_build_output_pair_coefficients")

pairLawPublications :: [StagePublication]
pairLawPublications = [signedPairPublication,extendedPairPublication]

pairLawScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
pairLawScenarios bundle _ = map literal pairLawCases ++ controls ++ templateBranches
  where
    sources=CoefficientSources (B.take 2631680 bundle) (B.drop 2631680 bundle)
    descriptor=Address 0x02510000
    output=Address 0x02511000
    dataBase=0x04000000
    descriptorBytes=B.take 0x140 (B.replicate 0x244 0) <> wordBytes 5 <> B.replicate (0x244-0x144) 0
    selectedKey=sourceHalf sources 0xd621a0
    keys=[sourceHalf sources (0xd61e14+lane*2) | lane<-[0..3]]
    configPatch (key,value)=Patch (Address (dataBase+key)) (B.singleton value)
    wordConfig key value=[(key,fromIntegral value),(key+1,fromIntegral (value `shiftR` 8))]
    baseEntries factors=
      [ (selectedKey,1)
      , (sourceHalf sources 0xd61e04,1)
      , (sourceHalf sources 0xd620b0,factor104And154 factors)
      , (sourceHalf sources 0xd62194,factor10c factors)
      , (sourceHalf sources 0xd61f34,factorF4And160 factors)
      , (sourceHalf sources 0xd61e64,factor13c factors)
      , (sourceHalf sources 0xd62134,factorModeIdentifier factors)
      ]
    literal example = make (pairCaseName example) publication initial (pairCaseExpected example)
      where
        publication=case pairCaseTemplateFactors example of Nothing->signedPairPublication;Just _->extendedPairPublication
        coefficients=pairCaseCoefficients example
        entries=baseEntries (pairCaseBaseFactors example) ++ concat (zipWith wordConfig keys [coefficient00 coefficients,coefficient11 coefficients,coefficient01 coefficients,coefficient10 coefficients]) ++ case pairCaseTemplateFactors example of
          Nothing -> []
          Just factors -> [(0xc257,factorTypeAndDc factors),(sourceHalf sources 0xd66540,factor148 factors)]
        initial=[Patch descriptor descriptorBytes,Patch output (B.replicate 10 0xa5),word 0x96ca50 dataBase] ++ map configPatch entries
    make name publication initial expected = StageScenario
      { stageCaseName=StageCaseName ("output-expansion-pair-"++name)
      , stagePublication=publication
      , stageArguments=[0x02511000,0x02510000]
      , stageInitial=initial
      , stageFinal=pairPacketPatches output expected ++ [Patch (plus output 1) (B.singleton 0xa5)]
      , stageReturn=Nothing
      , stageCodePointers=[]
      , stageCalls=[]
      }
    controls=[make (suffix++"-"++show mode++"-"++show selected) publication
      [Patch descriptor descriptorBytes,word (0x02510000+0x104) mode,Patch output (B.replicate 10 0xa5),word 0x96ca50 dataBase,configPatch (selectedKey,selected)] expected
      | (suffix,publication)<-[("signed-control",signedPairPublication),("extended-control",extendedPairPublication)]
      , (mode,selected,expected)<-[(2,0,PairPacket 1 0 0 0 0),(3,255,PairPacket 1 0 0 0 0),(0,0,PairPacket 0 1024 1024 0 0),(0,11,PairPacket 0 1024 1024 0 0)]]
    descriptorOf fields=foldl' (\bytes (offset,value) -> B.take offset bytes <> wordBytes value <> B.drop (offset+4) bytes) descriptorBytes fields
    branch name kind dc ec primary inputKey expected = make name extendedPairPublication initial expected
      where
        input=descriptorOf [(0x140,kind),(0xdc,dc),(0xec,ec),(0xe4,primary)]
        entries=baseEntries (BasePairFactors 128 128 128 128 64)
          ++ concat (zipWith wordConfig keys ([1024,512,64,8160] :: [Word16]))
          ++ [(sourceHalf sources 0xd6691c,128),(sourceHalf sources 0xd66d0c,128),(sourceHalf sources 0xd66540,128)
             ,(sourceHalf sources (0xd62134+modeIdentifier input*2),64)
             ,(0xc257,128),(0xc258,128),(0xc259,128),(0xc25a,128),(0xc25b,128),(0xc25c,128),(0xc260,128),(inputKey,64)]
        initial=[Patch descriptor input,Patch output (B.replicate 10 0xa5),word 0x96ca50 dataBase] ++ map configPatch entries
    halfPacket=PairPacket 1 512 256 32 65520
    unityPacket=PairPacket 1 1024 512 64 65504
    templateBranches=
      [ branch "template-default-factor" 0 0 0 0 (sourceHalf sources 0xd6691c) halfPacket
      , branch "template-kind3-factor" 3 0 0 0 (sourceHalf sources 0xd66d0c) halfPacket
      , branch "template-kind3-dc" 3 1 0 0 0xc257 halfPacket
      , branch "template-kind4-dc-disabled" 4 0 0 0 0xc257 unityPacket
      , branch "template-kind5-ec-disabled" 5 0 0 5 0xc257 halfPacket
      , branch "template-kind5-primary5" 5 0 1 5 0xc258 halfPacket
      , branch "template-kind5-primary6" 5 0 1 6 0xc259 halfPacket
      , branch "template-kind5-primary7" 5 0 1 7 0xc25c halfPacket
      , branch "template-kind5-primary8" 5 0 1 8 0xc25a halfPacket
      , branch "template-kind5-primary11" 5 0 1 11 0xc25b halfPacket
      , branch "template-kind5-primary12" 5 0 1 12 0xc25b halfPacket
      , branch "template-kind5-primary-default" 5 0 1 51 0xc257 halfPacket
      , branch "template-kind5-third-factor" 5 0 0 0 (sourceHalf sources 0xd66540) halfPacket
      , branch "template-kind6-fourth-factor" 6 0 0 0 0xc260 halfPacket
      , branch "template-kind7-fourth-ignored" 7 0 0 0 0xc260 unityPacket
      ]
