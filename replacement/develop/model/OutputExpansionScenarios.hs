module OutputExpansionScenarios
  ( ExpansionPhase(..), ExpansionMemory(..), expansionEffects, expansionEffectsFromImage
  , outputExpansionPublications, outputExpansionScenarios ) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int8, Int16, Int32)
import Data.Word
import PipelineModel
import Scenarios (Address(..), Patch(..), wordBytes)
import OutputCoefficientsScenarios (CoefficientSources(..), CoefficientPart(..), coefficientEffects)
import OutputExpansionPairLaw (PairPacket(..), pairPacket, extendedPairPacket)

data ExpansionPhase = ResetExpansion | ExpandDescriptor | SubmitDescriptor deriving (Eq, Show)

newtype ExpansionMemory = ExpansionMemory { expansionMemoryRegions :: [Patch] }

readExpansionMemory :: ExpansionMemory -> Address -> Word8
readExpansionMemory (ExpansionMemory regions) (Address address) =
  case [B.index bytes (fromIntegral (address-base)) | Patch (Address base) bytes<-reverse regions,address>=base,address-base<fromIntegral (B.length bytes)] of
    value:_ -> value
    [] -> error "output expansion reads unmapped initial RAM"

data ExpansionState = ExpansionState
  { descriptorBase :: Address
  , descriptorBytes :: B.ByteString
  , outputBase :: Address
  , outputBytes :: B.ByteString
  , priorMemory :: Address -> Word8
  , expansionPatches :: [Patch]
  }

type Expansion = ExpansionState -> ExpansionState

plus :: Address -> Word32 -> Address
plus (Address address) offset = Address (address+offset)

little :: (Integral a, Bits a) => B.ByteString -> a
little = B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0

atWord :: B.ByteString -> Word32 -> Word32
atWord bytes offset = little (B.take 4 (B.drop (fromIntegral offset) bytes))
atHalf :: B.ByteString -> Word32 -> Word16
atHalf bytes offset = little (B.take 2 (B.drop (fromIntegral offset) bytes))
atByte :: B.ByteString -> Word32 -> Word8
atByte bytes offset = B.index bytes (fromIntegral offset)

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
putBytes :: Address -> [Word8] -> Patch
putBytes address = Patch address . B.pack
putHalf :: Address -> Word16 -> Patch
putHalf address = Patch address . halfBytes
putWord :: Address -> Word32 -> Patch
putWord address = Patch address . wordBytes
putHalves :: Address -> [Word16] -> Patch
putHalves address = Patch address . B.concat . map halfBytes

applyBytes :: Address -> B.ByteString -> Patch -> B.ByteString
applyBytes (Address base) original (Patch (Address address) content)
  | start < end = B.take offset original <> B.take count (B.drop sourceOffset content) <> B.drop (offset+count) original
  | otherwise = original
  where
    start=max base address
    end=min (base+fromIntegral (B.length original)) (address+fromIntegral (B.length content))
    offset=fromIntegral (start-base)
    sourceOffset=fromIntegral (start-address)
    count=fromIntegral (end-start)

emit :: [Patch] -> Expansion
emit patches state = state
  { descriptorBytes=foldl' (applyBytes (descriptorBase state)) (descriptorBytes state) patches
  , outputBytes=foldl' (applyBytes (outputBase state)) (outputBytes state) patches
  , expansionPatches=expansionPatches state++patches }

memoryByte :: ExpansionState -> Address -> Word8
memoryByte state address@(Address value) = foldl' overwrite (priorMemory state address) (expansionPatches state)
  where overwrite old (Patch (Address start) content)
          | value>=start && value-start<fromIntegral (B.length content) = B.index content (fromIntegral (value-start))
          | otherwise = old

run :: [Expansion] -> Expansion
run actions state = foldl' (flip ($)) state actions

sourceBytes :: CoefficientSources -> Word32 -> Int -> B.ByteString
sourceBytes sources address count
  | offset<0 || offset+count>B.length image = error "output expansion requires authenticated original image bytes"
  | otherwise = B.take count (B.drop offset image)
  where (image,base)=if address>=0xbf1a00 then (coefficientImage5 sources,0xbf1a00) else (coefficientImage4 sources,0x61c400)
        offset=fromIntegral address-base
romHalf :: CoefficientSources -> Word32 -> Word32
romHalf sources address = little (sourceBytes sources address 2)
romWord :: CoefficientSources -> Word32 -> Word32
romWord sources address = little (sourceBytes sources address 4)

expansionEffectsFromImage :: B.ByteString -> (Word32 -> Word8) -> ExpansionMemory -> Address -> B.ByteString -> Address -> B.ByteString -> ExpansionPhase -> [Patch]
expansionEffectsFromImage bundle = expansionEffects (CoefficientSources (B.take 2631680 bundle) (B.drop 2631680 bundle))

expansionEffects :: CoefficientSources -> (Word32 -> Word8) -> ExpansionMemory -> Address -> B.ByteString -> Address -> B.ByteString -> ExpansionPhase -> [Patch]
expansionEffects sources cfg memory descriptor descriptorInput output outputInput phase = finalPatches final
  where
    initial=ExpansionState descriptor descriptorInput output outputInput (readExpansionMemory memory) []
    reset state = emit ([Patch output (B.replicate 0x1260 0),putWord (plus descriptor 0x13c) level]++selection) state
      where
        input=descriptorBytes state
        w=atWord input
        level=romHalf sources (0xd62348+2*fromIntegral (length (takeWhile (<atByte input 0x11a) (map cfg [0xa924..0xa92c]))))
        selection=[putWord (plus descriptor 0x154) selected | w 0x140==7,w 0x160==3,w 0x154/=0,w 0x11c/=w 0x120]
        selected=case w 0x154 of 3->1;4->2;_->0
    final=case phase of
      ResetExpansion -> reset initial
      ExpandDescriptor -> expand sources cfg initial
      SubmitDescriptor -> expand sources cfg (reset initial)

coefficient :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> CoefficientPart -> Expansion
coefficient sources cfg offset part state = emit (coefficientEffects sources cfg (descriptorBase state) (descriptorBytes state) (plus (outputBase state) offset) part) state

coefficientPacket :: CoefficientSources -> (Word32 -> Word8) -> CoefficientPart -> ExpansionState -> (B.ByteString,[Patch])
coefficientPacket sources cfg part state = (packet,external)
  where
    scratch=Address 0x70000000
    patches=coefficientEffects sources cfg (descriptorBase state) (descriptorBytes state) scratch part
    packet=foldl' (applyBytes scratch) (B.replicate 512 0) patches
    external=[patch | patch@(Patch (Address address) _) <- patches,address<0x70000000 || address>=0x70000200]

expand :: CoefficientSources -> (Word32 -> Word8) -> Expansion
expand sources cfg = run
  [ flagBytes [(8,4),(4,5),(0,6)]
  , whenWord 8 (==1) (coefficient sources cfg 4 NoiseCoefficients)
  , flagBytes [(16,0x30),(12,0x31)]
  , whenWord 16 (\value -> value==1 || value==2) (coefficient sources cfg 0x30 SelectedLookup)
  , coefficient sources cfg 0x30 LookupControls,coefficient sources cfg 0x30 ExportCoefficients
  , flagBytes [(20,0xd2),(24,0xd1),(28,0xd0),(32,0xcf)]
  , whenWord 20 (==1) (coefficient sources cfg 0xa6 MatrixCoefficients)
  , whenWord 24 (==1) (coefficient sources cfg 0xa6 BlendedLookup)
  , whenWord 32 (==1) (coefficient sources cfg 0xa6 PairedLookup)
  , whenWord 0xd8 (`testBit` 1) (run [coefficient sources cfg 0xa6 SensorLayout,coefficient sources cfg 0xa6 SignedChannels])
  , whenWord 0xd8 (`testBit` 2) (coefficient sources cfg 0x19c AuxiliaryChannels)
  , flagBytes [(0x24,0x1a6)],whenWord 0x24 (==1) (coefficient sources cfg 0x1a6 ThresholdCurves)
  , flagBytes [(0x2c,0x226)],whenWord 0x2c (==1) (leaf sources cfg ProfileBlock 0x226)
  , flagBytes [(0x30,0x234)],whenWord 0x30 (==1) (leaf sources cfg GridBlock 0x234)
  , flagBytes (zip [0x50,0x4c..0x34] [0xf24..0xf2b])
  , whenWord 0x48 (==1) (leaf sources cfg PairBlock 0xf24)
  , whenWord 0x44 (==1) (leaf sources cfg NibblePairs 0xf24)
  , whenWord 0x40 (==1) (leaf sources cfg Matrix15 0xf24)
  , whenWord 0x3c (==1) (leaf sources cfg PairedChannels 0xf24)
  , whenWord 0x38 (==1) (leaf sources cfg ChannelLimits 0xf24)
  , whenWord 0x34 (/=0) (leaf sources cfg ChannelControls 0xf24)
  , flagBytes [(0x5c,0xf9c),(0x58,0xf9d),(0x54,0xf9e)]
  , whenWord 0x5c (==1) (run [leaf sources cfg MixedGains 0xf9c,leaf sources cfg MixLimits 0xf9c])
  , whenWord 0x54 (==1) (run [leaf sources cfg ChannelGains 0xf9c,leaf sources cfg SignedLimits 0xf9c])
  , flagBytes (zip [0x60,0x64..0x7c] [0xfc6,0xfc7,0xfc8,0xfc9,0xfcf,0xfd0,0xfd1,0xfd2])
  , whenWord 0x60 (==1) (leaf sources cfg (GroupGains 1) 0xfc6)
  , whenWord 0x70 (==1) (leaf sources cfg (GroupGains 2) 0xfc6)
  , whenWord 0x68 (==1) (leaf sources cfg (GroupSelector 1) 0xfc6)
  , whenWord 0x78 (==1) (leaf sources cfg (GroupSelector 2) 0xfc6)
  , flagBytes [(0x84,0xfd8),(0x80,0xfd9)]
  , whenWord 0x84 (/=0) (run [leaf sources cfg FilterMatrix 0xfd8,leaf sources cfg FilterGain 0xfd8,leaf sources cfg FilterTriplet 0xfd8,leaf sources cfg FilterOffsets 0xfd8,leaf sources cfg FilterLimits 0xfd8])
  , whenWord 0x80 (==1) (leaf sources cfg ExposureBlock 0xfd8)
  , whenWord 0x84 (==2) (leaf sources cfg ExtendedFilter 0xfd8)
  , flagBytes (zip [0x88,0x8c..0xd4] [0x101d..0x1030])
  , marked 0x88 (==1) 17 (leaf sources cfg TemplateNoise 0x101c)
  , marked 0x8c (==1) 18 (leaf sources cfg TemplateTriplet 0x101c)
  , leaf sources cfg TemplateHeader 0x101c,leaf sources cfg TemplateControls 0x101c,mark 19
  , marked 0x90 (==1) 20 (run [leaf sources cfg TemplateLookup 0x101c,leaf sources cfg TemplateExport 0x101c])
  , marked 0x94 (==1) 21 (leaf sources cfg TemplateMatrix 0x101c)
  , marked 0x98 (==1) 22 (leaf sources cfg TemplateBlend 0x101c)
  , marked 0xa0 (==1) 23 (leaf sources cfg TemplateSmall 0x101c)
  , marked 0xa4 (==1) 24 (leaf sources cfg TemplatePair 0x101c)
  , marked 0xa8 (==1) 25 (leaf sources cfg TemplateChannelLimits 0x101c)
  , marked 0xac (==1) 26 (leaf sources cfg TemplateNibblePairs 0x101c)
  , marked 0xb0 (==1) 27 (leaf sources cfg TemplatePairedChannels 0x101c)
  , marked 0xc0 (==1) 28 (leaf sources cfg TemplateScaled 0x101c)
  , marked 0xc4 (/=0) 29 (run [leaf sources cfg TemplateFilterMatrix 0x101c,leaf sources cfg TemplateFilterGain 0x101c,leaf sources cfg TemplateFilterTriplet 0x101c,leaf sources cfg TemplateFilterOffsets 0x101c,leaf sources cfg TemplateFilterLimits 0x101c,whenWord 0xc4 (==2) (leaf sources cfg TemplateExtendedFilter 0x101c)])
  , marked 0xc8 (==1) 30 (leaf sources cfg TemplateClamps 0x101c)
  , marked 0xcc (/=0) 30 id
  , \state -> emit [putWord (outputBase state) (atWord (descriptorBytes state) 0xd8)] state
  ]

flagBytes :: [(Word32,Word32)] -> Expansion
flagBytes fields state = emit [putBytes (plus (outputBase state) destination) [fromIntegral (atWord (descriptorBytes state) source)] | (source,destination)<-fields] state
whenWord :: Word32 -> (Word32 -> Bool) -> Expansion -> Expansion
whenWord offset predicate action state = if predicate (atWord (descriptorBytes state) offset) then action state else state
mark :: Int -> Expansion
mark flagBit state = emit [putWord (plus (descriptorBase state) 0x14c) (setBit (atWord (descriptorBytes state) 0x14c) flagBit)] state
marked :: Word32 -> (Word32 -> Bool) -> Int -> Expansion -> Expansion
marked offset predicate flagBit action = whenWord offset predicate (run [action,mark flagBit])

data ExpansionPart = ProfileBlock | GridBlock | PairBlock | NibblePairs | Matrix15 | PairedChannels | ChannelLimits | ChannelControls
  | MixedGains | MixLimits | ChannelGains | SignedLimits | GroupGains Word32 | GroupSelector Word32
  | FilterMatrix | FilterGain | FilterTriplet | FilterOffsets | FilterLimits | ExposureBlock | ExtendedFilter
  | TemplateNoise | TemplateTriplet | TemplateHeader | TemplateControls | TemplateLookup | TemplateExport | TemplateMatrix | TemplateBlend
  | TemplateSmall | TemplatePair | TemplateChannelLimits | TemplateNibblePairs | TemplatePairedChannels | TemplateScaled
  | TemplateFilterMatrix | TemplateFilterGain | TemplateFilterTriplet | TemplateFilterOffsets | TemplateFilterLimits | TemplateExtendedFilter | TemplateClamps
  deriving (Eq,Show)

leaf :: CoefficientSources -> (Word32 -> Word8) -> ExpansionPart -> Word32 -> Expansion
leaf sources cfg part offset state = emit patches state
  where
    descriptor=descriptorBytes state
    w=atWord descriptor
    h=atHalf descriptor
    b=atByte descriptor
    block=plus (outputBase state) offset
    d=descriptorBase state
    cb key=fromIntegral (cfg key) :: Word32
    ch key=fromIntegral (cfg key)+fromIntegral (cfg (key+1))*256 :: Word16
    kb :: Word32 -> Word32 -> Word32
    kb table entry=cb (romHalf sources (table+entry*2))
    kh :: Word32 -> Word32 -> Word16
    kh table entry=ch (romHalf sources (table+entry*2))
    keyBytes :: Word32 -> Int -> [Word8]
    keyBytes table count=[fromIntegral (kb table (fromIntegral slot)) | slot<-[0..count-1]]
    keyHalves :: Word32 -> Int -> [Word16]
    keyHalves table count=[kh table (fromIntegral slot) | slot<-[0..count-1]]
    put offset'=putBytes (plus block offset')
    halves offset'=putHalves (plus block offset')
    word offset'=putWord (plus block offset')
    selected=if w 0xe4==0x35 && w 0x1b0==1 then w 0x180 else w 0xf4
    index=selected+w 0x160*10
    modeBand=configurationModeValue cfg descriptor 11
    modeCode=configurationModeCode descriptor
    typeMode=w 0x140
    active=w 0x1d0==1 && w 0x1d8/=0 && w 0x144==0
    activeFilter=active && w 0x1d4/=0
    activeTemplate=active && w 0xe0==0
    templateTable ordinary fifth sixth=case typeMode of 5->fifth;6->sixth;_->ordinary
    packet part'=coefficientPacket sources cfg part' state
    fromPacket part' scatter=let (value,effects)=packet part' in effects++scatter value
    patches=case part of
      ProfileBlock ->
        let row=if b 0x160>3 then 0 else fromIntegral (b 0x160)
            initial=cb (romWord sources (0xd6020c+4*(row*21+profileIndex descriptor)))
            chosen=if initial>=1 && initial<=6 then cb (romWord sources (0xd6035c+4*((initial-1)*11+modeBand))) else 255
            payload=if chosen>=1 && chosen<=7 then [fromIntegral (cb (romWord sources (0xd60464+(chosen-1)*32+lane*4))) | lane<-[0..7]] else replicate 8 15
        in [put 1 ([fromIntegral (cb (romWord sources (0xd60544+row*20+lane*4))) | lane<-[0..4]]++payload)]
      GridBlock ->
        let gridWidth=boundedGrid ((fromIntegral (h 0x234)+4) `div` 8)
            gridHeight=boundedGrid ((fromIntegral (h 0x236)+3) `div` 6)
            raw=h 0x1ea
            band=length (takeWhile (<=raw) [fromIntegral (romHalf sources (0x7c2de8+lane*2)) | lane<-[0..5]])
            gainIndex=fromIntegral band :: Word16
            band2=if raw<fromIntegral (romHalf sources 0x7c2de8) then 0 else gainIndex
            bandGain=if raw>=fromIntegral (romHalf sources 0x7c2de8) then band2 else 0
            split low high=let value=w 0xf4; lowValue=cb low; highValue=cb high in if lowValue>=highValue || value<=lowValue then 0 else if value<highValue then 1 else 2
            first=split 0xaf2a 0xaf2b
            second=split 0xaf82 0xaf83
            signed=signFill 0xfe00 (ch (romHalf sources (0xd60c48+second*2)))
            row=0x7c2dfc+fromIntegral bandGain*0xca8
            factor=gridGain sources cfg descriptor
        in [halves 2 [fromIntegral gridWidth,fromIntegral gridHeight,fromIntegral ((0x40000+gridWidth `div` 2) `div` gridWidth),fromIntegral ((0x40000+gridHeight `div` 2) `div` gridHeight),0,0],
            put 14 (keyBytes (0xd60c28+first*8) 4),put 18 [factor],halves 20 [ch 0xaf80],
            put 22 [fromIntegral (kb 0xd60c40 second),fromIntegral (kb 0xd60c20 second)],halves 24 [signed],
            word 0xcec row,putHalves (plus d 0x23c) [bandGain,0,raw,0]]
      PairBlock -> pairEffects sources cfg descriptor block False
      TemplatePair -> pairEffects sources cfg descriptor block True
      NibblePairs -> nibbleEffects sources cfg descriptor block 3 0x46
      TemplateNibblePairs -> nibbleEffects sources cfg descriptor block 10 0xfc
      Matrix15 ->
        let chosen=kb 0xd61db4 index
            valid=chosen>=1 && chosen<=6
            values=if valid then map (signFill 0xfc00) (keyHalves (0xd61e80+(chosen-1)*30) 15) else 1023:replicate 14 0
        in [put 4 [0] | not valid]++[halves 8 values]
      PairedChannels -> pairedEffects sources cfg descriptor block False
      TemplatePairedChannels -> pairedEffects sources cfg descriptor block True
      ChannelLimits -> channelLimitEffects sources cfg descriptor block False
      TemplateChannelLimits -> channelLimitEffects sources cfg descriptor block True
      ChannelControls -> [put 0x74 [fromIntegral (kb table index) | table<-[0xd62060,0xd62270,0xd620e4]]]
      MixedGains -> [put 3 (mixedGains sources cfg descriptor)]
      MixLimits ->
        let mix=gainMixMode cfg descriptor
            chosen=if mix==2 then cb 0xb671 else kb (if mix==1 then 0xd637bc else 0xd63814) index
            values=if chosen>=1 && chosen<=15 then keyBytes (0xd6386c+(chosen-1)*16) 8 else [64,128,0,4,0,4,0,4]
        in [put 11 (take 2 values)]++concat [[halves destination [fromIntegral (values!!low)],put (destination+2) [signFill8 0xe0 (values!!high)]] | (destination,low,high)<-[(14,3,2),(18,5,4),(22,7,6)]]
      ChannelGains ->
        let factors=map toInteger [kb 0xd63974 (w 0x194),kb 0xd63980 modeBand,kb 0xd63210 (w 0x13c),kb 0xd63224 modeCode]
            gains=keyBytes (0xd63658+index*8) 4
            scale=kb 0xd6396c (w 0x160)
            pair :: Word8 -> [Word8]
            pair value=
              let primary=fromInteger ((product factors*toInteger value `div` 65536) `mod` 4294967296) :: Word32
                  secondary=primary*scale `shiftR` 4
              in [fromIntegral (min 255 primary),fromIntegral (min 255 secondary)]
        in [put 0x1a (concatMap pair gains)]
      SignedLimits ->
        let values=keyBytes (0xd633d8+index*16) 8
        in [put 0x22 [values!!0,values!!1,values!!3,signFill8 0xc0 (values!!2),values!!5,signFill8 0xc0 (values!!4),values!!7,signFill8 0xc0 (values!!6)]]
      GroupSelector groups ->
        [put destination [fromIntegral (kb table index)] | (mask,destination,table)<-[(1,4,0xd63e00),(2,13,0xd63e50)],groups .&. mask/=0]
      GroupGains groups ->
        let ordinary=if w 0x104<8 && testBit (0xf1::Word32) (fromIntegral (w 0x104)) then (kb 0xd63d74 (w 0x104),kb 0xd63d84 (w 0x104)) else (64,64)
            special=if w 0xe4==53 then (kb 0xd63d6c (w 0x1b0),kb 0xd63d94 (w 0x1b0)) else (64,64)
            scale factorA factorB value=fromIntegral (min 63 (factorA*factorB*value*32 `shiftR` 17))
            group mask destination header first second tail' factorA factorB=
              [put destination [fromIntegral (kb header index),scale factorA factorB (kb first index),scale factorA factorB (kb second index),fromIntegral (kb tail' index)] | groups .&. mask/=0]
        in group 1 5 0xd63f90 0xd63ef0 0xd63db0 0xd64030 (fst ordinary) (fst special)
           ++group 2 14 0xd63fe0 0xd63f40 0xd63ea0 0xd64080 (snd ordinary) (snd special)
      FilterMatrix ->
        let chosen=if w 0x164==1 then cb 0xbbf3 else if activeFilter then kb 0xd5f678 (w 0x1d8) else kb 0xd5f9d4 index
        in [halves 2 (map (signFill 0xfe00) (keyHalves (0xd5f334+(chosen-1)*20) 10))]
      FilterGain -> let (value,effects)=filterGain sources cfg state in effects++[put 0x25 [value]]
      FilterTriplet -> [put 0x16 (keyBytes ((if w 0x154==0 then 0xd5f520 else 0xd5f508)+w 0x160*6) 3)]
      FilterOffsets ->
        let disabled=w 0x104==2 && cb 0xc043/=0
            base=if w 0x164==1 then 0xd5f21c+selected*8 else (if w 0x154==0 then 0xd5f894 else 0xd5f538)+index*8
            first=keyBytes base 4
            exposure=selectedExposure descriptor
            second=if exposure==100 then replicate 4 0 else [fromIntegral value :: Int8 | value<-keyBytes (0xd5f7ac+(exposure-exposure `div` 10*7)*8) 4]
            values=if disabled then replicate 4 0 else zipWith (\a z -> fromIntegral (max 0 (min 63 (fromIntegral a+fromIntegral z::Int)))) first second
        in [put 0x19 values]
      FilterLimits ->
        let chosen=kb (if activeFilter then 0xd5f460 else 0xd5f680) (if activeFilter then w 0x1d8 else index)
        in [put 0x1d (limitPacket sources cfg chosen)]
      ExposureBlock -> exposureEffects sources cfg descriptor block
      ExtendedFilter ->
        let chosen=kb 0xd5f75c index
            values=if chosen>=1 && chosen<=5 then keyBytes (0xd5f27c+(chosen-1)*20) 10 else 31:replicate 9 0
        in [put 0x38 (values++[fromIntegral (kb 0xd5f4b8 index),fromIntegral (kb 0xd5f824 index)])]
      TemplateTriplet -> [halves 0x3e [h 0x1c8,h 0x1ca,h 0x1cc]]
      TemplateHeader ->
        let values=keyBytes (templateTable 0xd6694c 0xd66d28 0xd66af8) 3
            common=keyBytes 0xd66b54 4
        in [put 0x44 (values++[common!!2,common!!1,common!!0,common!!3])]
      TemplateControls -> [put 0x4b (keyBytes (templateTable 0xd66604 0xd66a6c 0xd668b4) 4)]
      TemplateLookup -> fromPacket LookupControls (\value -> [Patch (plus block 0x4f) (B.take 4 (B.drop 3 value))])
      TemplateExport -> fromPacket ExportCoefficients (\value -> [Patch (plus block 0x54) (B.take 44 (B.drop 8 value))])
      TemplateMatrix -> fromPacket MatrixCoefficients (\value -> [put 4 [atByte value 0x2c],Patch (plus block 0x80) (B.take 18 (B.drop 0x2e value))])
      TemplateBlend -> fromPacket BlendedLookup (\value -> [Patch (plus block 0x92) (B.take 9 (B.drop 0x40 value)),Patch (plus block 0x9c) (B.take 56 (B.drop 0x4a value)),halves 0xd4 [atHalf value 0x84,0x3fff]])
      TemplateSmall ->
        let table=templateTable (0xd66a74+w 0x128*20) 0xd66824 0xd66c24
            chosen=kb table (w 0xf4)
            valid=chosen>=1 && chosen<=4
        in [put 13 [0] | not valid]++[put 0xd8 (if valid then keyBytes (0xd667a8+(chosen-1)*6) 3 else [14,1,0])]
      TemplateScaled ->
        let table=case typeMode of 1->0xd668cc;2->0xd668cc;3->0xd668cc;4->0xd668cc;5->0xd66bb8;6->0xd66b68;_->0xd66968
            values=keyBytes (table+w 0xf4*8) 4
            first=if typeMode==3 then cb 0xc305 else 64
            second=if typeMode==4 then cb 0xc306 else 64
        in [put 0x12a [values!!0,fromIntegral (min 63 (first*second*fromIntegral (values!!1) `shiftR` 12)),fromIntegral (min 63 (first*second*fromIntegral (values!!2) `shiftR` 12)),values!!3]]
      TemplateFilterMatrix ->
        let key=case typeMode of
              1->romHalf sources (0xd66b5c+2*(w 0x128+if active then w 0x1d8*2 else 0))
              2->romHalf sources (0xd66b5c+2*(w 0x128+if active then w 0x1d8*2 else 0))
              3->romHalf sources (0xd666e8+2*w 0x128)
              4->romHalf sources (0xd666e8+2*w 0x128)
              5->romHalf sources (0xd66d48+2*(w 0x148+if activeTemplate then w 0x1d8*4 else 0))
              6->romHalf sources (0xd66714+2*w 0x1dc)
              _->0xc3bc
            chosen=cb key
            values=if chosen>=1 && chosen<=6 then keyHalves (0xd66d60+(chosen-1)*24) 12 else 511:replicate 11 0
        in [halves 0x12e (map (signFill 0xfe00) values)]
      TemplateFilterGain ->
        let table=case typeMode of
              1->0xd66e70+w 0x128*20;2->0xd66e70+w 0x128*20;3->0xd66e70+w 0x128*20;4->0xd66e70+w 0x128*20
              5->0xd66864+w 0x148*20;6->0xd66df0+w 0x1dc*20;_->0xd66838
            first=kb table (w 0xf4)
            override=if activeTemplate && typeMode/=6 then kb (if typeMode==5 then 0xd66784 else 0xd66d10) (w 0x1d4+(w 0x128+w 0x1d8*2)*2) else 32
            third=if atByte (outputBytes state) (offset+18)==0 then 32 else cb 0xc56f
        in [put 0x155 [fromIntegral (min 255 (first*override*third `shiftR` 10))]]
      TemplateFilterTriplet -> [put 0x146 (keyBytes (if typeMode==5 || typeMode==6 then 0xd666f8 else 0xd66cdc) 3)]
      TemplateFilterOffsets ->
        let table=case typeMode of 5->0xd663f4+w 0x148*80;6->0xd66734;_->0xd66620+w 0x128*80
        in [put 0x149 (keyBytes (table+w 0xf4*8) 4)]
      TemplateFilterLimits ->
        let table=case typeMode of 5->0xd66c54+w 0x148*20;6->0xd66954;_->0xd66a30+w 0x128*20
            chosen=if activeTemplate then kb 0xd66e68 (w 0x1d8) else kb table (w 0xf4)
        in [put 0x14d (limitPacket sources cfg chosen)]
      TemplateExtendedFilter ->
        let (table,second,third)=case typeMode of
              1->(0xd66ce4,0xd66a58,0xd66e54);2->(0xd66ce4,0xd66a58,0xd66e54)
              3->(0xd66ce4,0xd66a58,0xd66e54);4->(0xd66ce4,0xd66a58,0xd66e54)
              5->(0xd66b00+w 0x148*20,0xd66720,0xd6660c);6->(0xd66578,0xd66cb8,0xd665f0)
              _->(0xd66a1c,0xd66934,0xd66c10)
            chosen=kb table (w 0xf4)
            values=if chosen>=1 && chosen<=5 then keyBytes (0xd5f27c+(chosen-1)*20) 6 else 31:replicate 5 0
        in [put 0x156 (values++[fromIntegral (kb second (w 0xf4)),fromIntegral (kb third (w 0xf4))])]
      TemplateClamps ->
        let (first,second,third,index')=case typeMode of
              1->(0xd6679c,0xd66cd8,0xd668bc,w 0x128);2->(0xd6679c,0xd66cd8,0xd668bc,w 0x128)
              3->(0xd666ec,0xd6684c,0xd66948,w 0x128);4->(0xd666ec,0xd6684c,0xd66948,w 0x128)
              5->(0xd66c38,0xd66e98,0xd66ca4,w 0x148);6->(0xd668c0,0xd66cac,0xd66534,w 0x1dc)
              _->(0,0,0,0)
            values=if first==0 then [cb 0xc60b,cb 0xc618,cb 0xc625] else [kb first index',kb second index',kb third index']
        in [put 0x15e [fromIntegral (min 128 (values!!0)),fromIntegral (min 128 (values!!1)),fromIntegral (values!!2)]]
      TemplateNoise -> templateNoiseEffects sources cfg state block

boundedGrid :: Word32 -> Word32
boundedGrid value = if (value-8) .&. 65535 > 1016 then 8 else value
signFill :: Word16 -> Word16 -> Word16
signFill mask value = if value .&. mask==0 then value else value .|. mask
signFill8 :: Word8 -> Word8 -> Word8
signFill8 mask value = if value .&. mask==0 then value else value .|. mask

configurationModeValue :: (Word32 -> Word8) -> B.ByteString -> Word32 -> Word32
configurationModeValue cfg descriptor limit
  | cfg 0xa923==0 = if limit<12 then min 10 ((fromIntegral (atByte descriptor 0x11b)+2) `div` 3) else min 30 (fromIntegral (atByte descriptor 0x11b))
  | limit<12 = band
  | band==0 = 0
  | otherwise = min 30 (band*3-2)
  where
    value=fromIntegral (atHalf descriptor 0x1ea) :: Int16
    band=fromIntegral (length (takeWhile (<=value) [47,150,247,350,447,550,642,750,842,950]))

configurationModeCode :: B.ByteString -> Word32
configurationModeCode descriptor = maybe alternate id (lookup (atWord descriptor 0xe4) (zip kinds [1..]))
  where
    kinds=[5,6,37,8,9,10,14,11,7,13,29,30,31,32,33,34,28,25,26,27,24,23,20,22,40,41,42,43,44,45,46,47,48]
    secondary=atWord descriptor 0xe8
    alternate | atWord descriptor 0xe4==12 = 8
              | secondary>=1 && secondary<=13 = secondary+33
              | otherwise = 0

selectedExposure :: B.ByteString -> Word32
selectedExposure descriptor
  | atWord descriptor 0xe4==53 && atWord descriptor 0x1b0==1 = atWord descriptor 0x184
  | otherwise = atWord descriptor 0x12c

profileIndex :: B.ByteString -> Word32
profileIndex descriptor = maybe fallback id (lookup (atWord descriptor 0x124) table)
  where
    table=zip [1..13] [9,10,11,12,13,14,1,15,16,17,7,8,6]++[(14,18),(18,18),(15,19),(19,19),(16,20),(20,20)]
    fallback=maybe 0 id (lookup (atWord descriptor 0xe4) [(8,3),(9,3),(10,3),(19,2),(24,1),(32,4),(33,4),(34,4),(43,5),(44,5),(45,5)])

gainMixMode :: (Word32 -> Word8) -> B.ByteString -> Word32
gainMixMode cfg descriptor
  | kind==37 = 2
  | cfg key .&. mask/=0 && atWord descriptor 0x150/=0 = 1
  | otherwise = 0
  where
    kind=atWord descriptor 0xe4
    (key,mask)=maybe (0xb432,1) id (lookup kind mappings)
    group key' kinds=[(value,(key',bit index)) | (index,value)<-zip [0..] kinds]
    mappings=[(1,(0xb432,2)),(49,(0xb432,32)),(50,(0xb432,32)),(5,(0xb432,64)),(6,(0xb432,128))]
      ++[(value,(0xb433,bit index)) | (index,value)<-[(1,8),(2,9),(3,10),(4,14),(5,11),(5,12),(6,7),(7,13)]]
      ++group 0xb434 [29,30,31,32,33,34,28,25]
      ++group 0xb435 [26,27,24,23,20,21,22,40]
      ++group 0xb436 [41..48]

limitPacket :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> [Word8]
limitPacket sources cfg chosen
  | chosen>=1 && chosen<=6 = [cfg (romHalf sources (0xd5f6fc+(chosen-1)*16+lane*2)) | lane<-[0,1,2,5,3,6,4,7]]
  | otherwise = [16,32,128,0,128,0,128,0]

thresholdBucket :: CoefficientSources -> (Word32 -> Word8) -> Word32 -> Word32
thresholdBucket sources cfg value = fromIntegral (length (takeWhile (<value) [fromIntegral (cfg (romWord sources (0x829a38+lane*4))) | lane<-[0..8]]))

cacheValue :: CoefficientSources -> (Word32 -> Word8) -> ExpansionState -> Word32 -> Word32 -> (Word8,[Patch])
cacheValue sources cfg state slot table
  | flag/=0 = (memoryByte state address,[])
  | otherwise = (value,[putBytes address [value],putWord (plus address 4) 1])
  where
    address=Address (0x95ada0+slot*8)
    flag=little (B.pack [memoryByte state (plus address (4+lane)) | lane<-[0..3]]) :: Word32
    value=cfg (romWord sources table)

filterGain :: CoefficientSources -> (Word32 -> Word8) -> ExpansionState -> (Word8,[Patch])
filterGain sources cfg state = (fromIntegral (min 255 value),cachePatches)
  where
    descriptor=descriptorBytes state
    w=atWord descriptor
    b=atByte descriptor
    kb table index=fromIntegral (cfg (romHalf sources (table+2*index))) :: Word32
    cb key=fromIntegral (cfg key) :: Word32
    selected=if w 0xe4==53 && w 0x1b0==1 then w 0x180 else w 0xf4
    exposure=selectedExposure descriptor
    exposureGain=if exposure==100 then 128 else kb 0xd5f874 (exposure-exposure `div` 10*7)
    active=w 0x1d0==1 && w 0x1d4/=0 && w 0x144==0
    factors=[kb 0xd5f468 (selected+w 0x160*10),kb 0xd5f2e0 (w 0xf0),kb 0xd5f26c (w 0x114),kb 0xd5f6e4 (configurationModeValue cfg descriptor 11),exposureGain,kb 0xd5faa0 (w 0x13c),kb 0xd5fa24 (configurationModeCode descriptor)]
    first=product (map toInteger factors)
    scales=[if w 0xe4==53 then kb 0xd5f6d0 selected else 64,if w 0x164==1 then cb 0xbcaa else 64,
      if active then kb 0xd5fa84 (w 0x128+w 0x1d8*2) else 64,if w 0x1b4==1 then cb 0xbcb1 else 64]
    prefix=toInteger (product scales `shiftR` 18)*((first `shiftR` 34) .&. 0x3fffc0)
    inputProfile=fromIntegral (b 0x11b)
    bucket=thresholdBucket sources cfg (fromIntegral (b 0x11a))
    (profile,firstPatches)=if inputProfile>30 then (64,[]) else cacheValue sources cfg state inputProfile (0x829a5c+inputProfile*4)
    next=emit firstPatches state
    (exposureValue,nextPatches)=cacheValue sources cfg next (31+bucket) (0x829a10+bucket*4)
    (profileGain,exposureScale,cachePatches)
      | w 0x1e4/=0 = (64,64,[])
      | b 0x1ae==1 = (b 0x1ec,b 0x1ed,[])
      | otherwise = (profile,exposureValue,firstPatches++nextPatches)
    value=fromInteger ((prefix*toInteger profileGain*toInteger exposureScale `shiftR` 24) `mod` 4294967296) :: Word32

gridGain :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Word8
gridGain sources cfg descriptor = fromIntegral (min 255 value)
  where
    w=atWord descriptor
    kb table index=toInteger (cfg (romHalf sources (table+index*2)))
    factors=[kb 0xd60bb0 (w 0xf4+w 0x160*10),kb 0xd60c50 (w 0x114),kb 0xd60c00 (configurationModeValue cfg descriptor 11),
      kb 0xd60b98 (w 0x13c),toInteger (cfg (romWord sources 0xd60c18)),
      if w 0x1b4==1 then toInteger (cfg 0xaf7d) else 64,if w 0xe4==53 then kb 0xd60bac (w 0x17c) else 64]
    value=fromInteger ((product factors*64 `shiftR` 42) `mod` 4294967296) :: Word32

exposureEffects :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Address -> [Patch]
exposureEffects sources cfg descriptor block =
  [putBytes (plus block 1) [0] | not enabled]++
  [putHalves (plus block 0x26) [atHalf descriptor 0x1c2,atHalf descriptor 0x1c0],
   putBytes (plus block 0x2a) (fromIntegral (kb 0xd5fa98 (w 0x160)):values++
      [fromIntegral (kb 0xd5fa90 (w 0x1e0*2)),fromIntegral (kb 0xd5fa92 (w 0x1e0*2))])]
  where
    w=atWord descriptor
    kb romBase entry=fromIntegral (cfg (romHalf sources (romBase+entry*2))) :: Word32
    chosen=kb 0xd5f278 (w 0x1e0)
    enabled=w 0xe8/=4 && chosen>=1 && chosen<=3 && w 0x1e4==0
    values | not enabled = [0,0,0,0,16,0,16,0,16,0,16]
           | atByte descriptor 0x1ae==1 = [atByte descriptor (0x1ee+(((chosen-1)*11+lane) .&. 255)) | lane<-[0..10]]
           | otherwise = [cfg (romWord sources (0x8179a0+(chosen-1)*440+thresholdBucket sources cfg (fromIntegral (atByte descriptor 0x11a))*44+lane*4)) | lane<-[0..10]]

pairEffects :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Address -> Bool -> [Patch]
pairEffects sources cfg descriptor block template =
  [putBytes (plus block enabledOffset) [pairEnabled packet],
   putHalves (plus block coefficientOffset) [pair00 packet,pair01 packet,pair11 packet,pair10 packet]]
  where
    packet=(if template then extendedPairPacket else pairPacket) sources cfg descriptor
    enabledOffset=if template then 8 else 2
    coefficientOffset=if template then 0xdc else 0x26

nibbleEffects :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Address -> Word32 -> Word32 -> [Patch]
nibbleEffects sources cfg descriptor block enabledOffset valuesOffset =
  [putBytes (plus block enabledOffset) [enabled],
   putHalves (plus block valuesOffset) (encode first++encode second),
   putBytes (plus block (valuesOffset+20)) controls]
  where
    w=atWord descriptor
    selected=cfg (romHalf sources (0xd61f84+2*(w 0xe8+w 0x108*14)))
    low=fromIntegral (selected .&. 15)
    high=fromIntegral (selected `shiftR` 4)
    enabled=if w 0xe8==0 || low/=0 || high/=0 then 1 else 0
    values selection=if selection==0 then replicate 5 0 else
      [fromIntegral (cfg key)+fromIntegral (cfg (key+1))*256 | lane<-[0..4],let key=romHalf sources (0xd621d8+(selection-1)*10+lane*2)]
    first=if w 0xe8==0 then [0,0,512,136,8072] else values low
    second=if w 0xe8==0 then replicate 5 0 else values high
    controls=if w 0xe8==0 || low==0 then [16,16] else [cfg (romHalf sources (0xd61c1c+(low-1)*4+lane*2)) | lane<-[0..1]]
    encode values'=[signFill mask value | (mask,value)<-zip [0xfe00,0xfe00,0,0xf000,0xf000] values']

pairedEffects :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Address -> Bool -> [Patch]
pairedEffects sources cfg descriptor block template =
  [putBytes (plus block byteOffset) controls,putHalves (plus block halfOffset) (concat (zipWith (\first second -> [signFill 0xfc00 first,second]) (take 5 values) (drop 5 values)))]
  where
    w=atWord descriptor
    chosen=fromIntegral (cfg (romHalf sources (0xd62040+2*(w 0x104+w 0x108*8)))) :: Word32
    valid=chosen>=1 && chosen<=8
    disabled=w 0x104==2
    controls=if disabled || not valid then replicate 4 0 else [cfg (romHalf sources (0xd61d74+(chosen-1)*8+lane*2)) | lane<-[0..3]]
    values | disabled = replicate 10 0
           | not valid = replicate 5 0++replicate 5 254
           | otherwise = [fromIntegral (cfg key)+fromIntegral (cfg (key+1))*256 | lane<-[0..9],let key=romHalf sources (0xd61cd4+(chosen-1)*20+lane*2)]
    byteOffset=if template then 0x112 else 0x5c
    halfOffset=if template then 0x116 else 0x60

channelLimitEffects :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> Address -> Bool -> [Patch]
channelLimitEffects sources cfg descriptor block template =
  [putBytes (plus block byteOffset) controls]++
  concat [ [putBytes (plus block (byteOffset+4+lane*4)) [gains!!fromIntegral lane],putHalf (plus block (byteOffset+6+lane*4)) (values!!fromIntegral lane)] | lane<-[0..4]]
  where
    w=atWord descriptor
    index=w 0xf4
    table | not template = 0xd61fdc
          | w 0x140==5 = 0xd667c0
          | w 0x140==6 = 0xd6658c
          | otherwise = 0xd669b8
    controls=[cfg (romHalf sources (0xd621d0+lane*2)) | lane<-[0..3]]
    gains=[cfg (romHalf sources (0xd61c70+index*10+lane*2)) | lane<-[0..4]]
    values=[fromIntegral (cfg key)+fromIntegral (cfg (key+1))*256 | lane<-[0..4],let key=romHalf sources (table+index*10+lane*2)]
    byteOffset=if template then 0xe4 else 0x2e

mixedGains :: CoefficientSources -> (Word32 -> Word8) -> B.ByteString -> [Word8]
mixedGains sources cfg descriptor
  | mode==2 = replicate 8 (cfg 0xb577)
  | otherwise = concat (zipWith (\primaryValue secondaryValue -> [fromIntegral primaryValue,fromIntegral secondaryValue]) mixedPrimary mixedSecondary)
  where
    w=atWord descriptor
    mode=gainMixMode cfg descriptor
    kb romBase entry=fromIntegral (cfg (romHalf sources (romBase+entry*2))) :: Word32
    bytes table count=[cfg (romHalf sources (table+lane*2)) | lane<-[0..count-1]]
    selected=if w 0xe4==53 && w 0x1b0==1 then w 0x180 else w 0xf4
    index=selected+w 0x160*10
    factors=[kb 0xd637a4 (configurationModeValue cfg descriptor 11),kb 0xd631fc (w 0x13c),kb 0xd63798 (w 0x194),
      if w 0x160==3 then kb 0xd63284 (w 0x154) else 32,if w 0xe4==53 then kb 0xd63864 (w 0x1b0) else 32]
    offset=if w 0xe4==53 then fromIntegral (fromIntegral (kb 0xd6380c (w 0x1b0)) :: Int8) else 0 :: Int
    productGain=product (map toInteger factors)
    weights=bytes 0xd63290 4
    primary table=[fromInteger ((productGain*toInteger (max 0 (min 31 (fromIntegral value+offset))) `shiftR` 25) `mod` 4294967296) :: Word32 | value<-bytes (table+index*8) 4]
    first=primary 0xd63298
    second=primary 0xd6305c
    firstScaled=[fromInteger ((toInteger value*toInteger (kb 0xd6395c (w 0x160))*toInteger weight `shiftR` 10) `mod` 4294967296) | (value,weight)<-zip first weights]
    secondScaled=[value*kb 0xd63964 (w 0x160)*fromIntegral weight `shiftR` 10 | (value,weight)<-zip second weights]
    bounds=bytes (0xd631b0+w 0x160*4) 2
    lower=fromIntegral (bounds!!0)
    upper=fromIntegral (bounds!!1)
    current=w 0x150
    mix a b
      | mode==0 || current<=lower = min 31 a
      | current>upper = min 31 b
      | otherwise = min 31 (b+(current-lower)*(b-a) `div` (upper-lower))
    mixedPrimary=zipWith mix (map (min 31) first) (map (min 31) second)
    mixedSecondary=zipWith mix (map (min 31) firstScaled) (map (min 31) secondScaled)

templateNoiseEffects :: CoefficientSources -> (Word32 -> Word8) -> ExpansionState -> Address -> [Patch]
templateNoiseEffects sources cfg state block = external++
  [putBytes (plus block 0x15) [atByte controls 4,fromIntegral (fromIntegral (atByte controls 5)*selectionGain `shiftR` 6)],
   putHalves (plus block 0x18) [atHalf controls 8,atHalf controls 10,atHalf controls 12,0],
   putHalves (plus block 0x20) (map (fromIntegral . min 511) [base+first,base,base+third]),
   putBytes (plus block 0x26) [fromIntegral (kb 0xd66920 (w 0xf4))],
   putHalves (plus block 0x28) ([atHalf controls value | value<-[24,22,28,26,32,30,36,34,40,38,42]])]
  where
    descriptor=descriptorBytes state
    w=atWord descriptor
    kb romBase entry=fromIntegral (cfg (romHalf sources (romBase+entry*2))) :: Word32
    cb key=fromIntegral (cfg key) :: Word32
    (controls,external)=coefficientPacket sources cfg NoiseControls state
    mode=w 0x140
    table=case mode of 3->0xd66850;4->0xd66850;5->0xd66548;6->0xd66548;_->0xd66cf8
    raw=kb table (w 0xf4)
    next=kb table (w 0xf4+1)
    remainder=fromIntegral ((fromIntegral (w 0x130)::Int32) `rem` 100) :: Word32
    interpolated=if w 0xf4==9 || raw>=next then raw else (raw+(next-raw)*remainder*cb 0xc0ee `div` 6400) .&. 65535
    kindGain=if w 0x104>=2 && w 0x104<=7 then kb 0xd66704 (w 0x104) else 64
    specialGain=if mode==7 || mode==8 then cb 0xc0f6 else 64
    base=fromInteger ((toInteger interpolated*toInteger (cb 0xc0ef)*toInteger kindGain*toInteger specialGain `shiftR` 18) `mod` 4294967296) :: Word32
    (first,third)=case mode of 3->(cb 0xc0ea,cb 0xc0eb);4->(cb 0xc0ea,cb 0xc0eb);5->(cb 0xc0ec,cb 0xc0ed);6->(cb 0xc0ec,cb 0xc0ed);_->(cb 0xc0e8,cb 0xc0e9)
    low=cb 0xc0f7
    high=cb 0xc0f8
    band=if low>=high || w 0xf4<=low then 0 else if w 0xf4<high then 1 else 2
    selectionGain=kb (case mode of 5->0xd66c08;6->0xd66c08;7->0xd66ea0;8->0xd66ea0;_->0xd666f0) band

finalPatches :: ExpansionState -> [Patch]
finalPatches state = [Patch (descriptorBase state) (descriptorBytes state),Patch (outputBase state) (outputBytes state)]++external
  where
    inside address base count = let Address start=base in address>=start && address-start<fromIntegral count
    bytes=[(Address (address+fromIntegral index),value) | Patch (Address address) content<-expansionPatches state,
      (index,value)<-zip [0::Int ..] (B.unpack content),
      not (inside (address+fromIntegral index) (descriptorBase state) (B.length (descriptorBytes state))),
      not (inside (address+fromIntegral index) (outputBase state) (B.length (outputBytes state)))]
    unique=foldl' (\previous (address,value) -> (address,value):filter ((/=address) . fst) previous) [] bytes
    external=[putBytes address [value] | (address,value)<-unique]

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++suffix))

rootPublication :: ExpansionPhase -> StagePublication
rootPublication phase = case phase of
  ResetExpansion -> publication 0xd622c0 "reset_output_descriptor"
  ExpandDescriptor -> publication 0xd5d294 "expand_output_descriptor"
  SubmitDescriptor -> StagePublication (Address 0xd5da2c) (StageSymbol "xm1_pipeline_sensor_submit_descriptor")

builderRoutes :: [(Word32,String,ExpansionPart)]
builderRoutes =
  [(0xd5e66c,"expand_filter_matrix",FilterMatrix),(0xd5e848,"expand_filter_gain",FilterGain),
   (0xd5eb64,"expand_filter_channels",ExposureBlock),(0xd5ed8c,"expand_detail_channels",ExtendedFilter),
   (0xd5eed0,"expand_filter_offsets",FilterOffsets),(0xd5f070,"expand_filter_triplet",FilterTriplet),
   (0xd5f0f4,"expand_filter_chroma",FilterLimits),(0xd60088,"expand_output_profile",ProfileBlock),
   (0xd612b8,"expand_pair_coefficients",PairBlock),(0xd613c4,"expand_dual_coefficients",NibblePairs),
   (0xd61644,"expand_channel_matrix",Matrix15),(0xd618a4,"expand_channel_pairs",PairedChannels),
   (0xd61ac8,"expand_header_channels",ChannelLimits),(0xd61b88,"expand_channel_gains",ChannelControls),
   (0xd623b0,"expand_channel_scaling",ChannelGains),(0xd6259c,"expand_channel_limits",SignedLimits),
   (0xd62954,"expand_mix_limits",MixLimits),(0xd62afc,"expand_mixed_channel_gains",MixedGains),
   (0xd64424,"expand_template_noise",TemplateNoise),(0xd64800,"expand_template_lookup_controls",TemplateLookup),
   (0xd64914,"expand_template_export_coefficients",TemplateExport),(0xd649b4,"expand_template_matrix_coefficients",TemplateMatrix),
   (0xd64b5c,"expand_template_lookup",TemplateBlend),(0xd653e8,"expand_output_pair_coefficients",TemplatePair),
   (0xd6541c,"expand_template_header_channels",TemplateChannelLimits),(0xd65524,"expand_template_dual_coefficients",TemplateNibblePairs),
   (0xd655f4,"expand_template_channel_pairs",TemplatePairedChannels),(0xd656ac,"expand_template_triplet",TemplateSmall),
   (0xd6579c,"expand_template_header",TemplateHeader),(0xd65834,"expand_template_controls",TemplateControls),
   (0xd658a0,"expand_template_channel_scaling",TemplateScaled),(0xd659bc,"expand_template_channel_matrix",TemplateFilterMatrix),
   (0xd65ce4,"expand_template_chroma_gain",TemplateFilterGain),(0xd65e90,"expand_template_chroma_controls",TemplateFilterTriplet),
   (0xd65ee8,"expand_template_chroma_offsets",TemplateFilterOffsets),(0xd65f94,"expand_template_chroma",TemplateFilterLimits),
   (0xd660fc,"expand_template_detail",TemplateExtendedFilter),(0xd662a0,"expand_template_detail_limits",TemplateClamps)]

outputExpansionPublications :: [StagePublication]
outputExpansionPublications = map rootPublication [ResetExpansion,ExpandDescriptor,SubmitDescriptor]++
  [publication address suffix | (address,suffix,_)<-builderRoutes]++
  [publication 0xd63a3c "expand_template_selectors",publication 0xd63ac4 "expand_template_gains",
   publication 0x73f7b8 "configuration_value_within_limit",publication 0x73f78c "select_type_mode_value_copy",
   publication 0xd663e0 "finish_template"]

outputExpansionScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
outputExpansionScenarios bundle _resolve = resetCases++rootCases++builderCases++groupCases++scalarCases++gainCases
  where
    sources=CoefficientSources (B.take 2631680 bundle) (B.drop 2631680 bundle)
    descriptor=Address 0x9a0000
    record=Address 0x9a1000
    configBase=0x03000000
    cache=Address 0x95ada0
    cold=[Patch cache (B.replicate 328 0)]
    warm=cold++[putBytes cache [192],putWord (plus cache 4) 1,putBytes (plus cache (31*8)) [128],putWord (plus cache (31*8+4)) 1]
    recordInput=B.replicate 4704 0xa5
    initialDescriptor=foldl' (applyBytes descriptor) (B.replicate 572 0<>B.replicate 8 0x6d)
      [putHalf (plus descriptor 0x1a4) 100,putHalf (plus descriptor 0x1a6) 100]
    input :: [(Word32,Word32)] -> B.ByteString
    input fields=foldl' (applyBytes descriptor) initialDescriptor [putWord (plus descriptor offset) value | (offset,value)<-fields]
    tables=[Patch (Address address) (sourceBytes sources address count) | (address,count)<-
      [(0x7c2de0,22708),(0x8179a0,2200),(0x824bc4,2668),(0x825630,10852),(0x829a10,512),(0xd5f21c,0xbb00)]]
    gainKeys=[romHalf sources table | table<-[0xd5f468,0xd5f2e0,0xd5f26c,0xd5f6e4,0xd5f874,0xd5faa0,0xd5fa24]]++
      [romWord sources 0x829a5c,romWord sources 0x829a10]
    cfg :: Word32 -> Word32 -> Word8
    cfg flavor key
      | key==0xb5 || key==0xcb74 = 0
      | key>=0xa924 && key<=0xa92c = fromIntegral ((key-0xa923)*10)
      | key>=0x3e451a && key<=0x3e4522 = fromIntegral ((key-0x3e4519)*10)
      | flavor==1 && key `elem` gainKeys = 64
      | otherwise = 1
    configuration :: Word32 -> [Patch]
    configuration flavor=
      [putWord (Address 0x96ca50) configBase,
       Patch (Address configBase) (B.pack [cfg flavor key | key<-[0..0x20000]]),
       Patch (Address (configBase+0x3e3d50)) (B.pack [cfg flavor key | key<-[0x3e3d50..0x3e4522]]),
       Patch (Address (configBase+0x408a00)) (B.pack [cfg flavor key | key<-[0x408a00..0x412000]])]
    initial :: Word32 -> [Patch] -> B.ByteString -> [Patch]
    initial flavor cacheInput descriptorInput=tables++configuration flavor++cacheInput++
      [Patch descriptor descriptorInput,Patch record recordInput,putBytes (plus record 4704) [0x39],
       Patch (Address 0x1467a88) (B.replicate 88 0x5a)]
    args :: ExpansionPhase -> [Word32]
    args phase=if phase==SubmitDescriptor then [0x9a1000,0x9a0000] else [0x9a0000,0x9a1000]
    rootCase :: String -> ExpansionPhase -> [(Word32,Word32)] -> [Patch] -> Word32 -> StageScenario
    rootCase label phase fields cacheInput flavor=
      let descriptorInput=input fields
          expected=expansionEffects sources (cfg flavor) (ExpansionMemory cacheInput) descriptor descriptorInput record recordInput phase
      in StageScenario (StageCaseName ("output-expansion-"++label++"-configuration-"++show flavor)) (rootPublication phase) (args phase)
          (initial flavor cacheInput descriptorInput) (expected++[putBytes (plus record 4704) [0x39]]) Nothing [] []
    resetCases=
      [let descriptorInput=foldl' (applyBytes descriptor) (input [(0x140,7),(0x160,3),(0x154,selection),(0x11c,1),(0x120,2)])
              [putBytes (plus descriptor 0x11a) [exposure]]
           descriptorFinal=foldl' (applyBytes descriptor) descriptorInput
              [putWord (plus descriptor 0x13c) level,putWord (plus descriptor 0x154) selected]
       in StageScenario (StageCaseName ("expansion-reset-literal-"++show (exposure,selection))) (rootPublication ResetExpansion) (args ResetExpansion)
           (initial 0 cold descriptorInput) [Patch descriptor descriptorFinal,Patch record (B.replicate 4704 0),putBytes (plus record 4704) [0x39]] Nothing [] []
      | (exposure,level)<-[(0,0),(10,0),(11,1),(90,8),(91,9),(255,9)]
      , (selection,selected)<-[(0,0),(1,0),(2,0),(3,1),(4,2),(0xffffffff,0)]]
    rootCases=
      [rootCase ("phase-"++show phase++"-mode-"++show mode++"-all-components") phase
        ([(offset,1) | offset<-[0,4..0xd4]]++[(0xd8,6),(0x140,mode),(0x150,17)]) cacheInput flavor
      | phase<-[ExpandDescriptor,SubmitDescriptor],mode<-[0..8],(cacheInput,flavor)<-[(cold,0),(warm,1)]]++
      [rootCase ("single-component-"++show (offset,value)) SubmitDescriptor [(offset,value),(0x140,5)] cold 0
      | offset<-[0,4..0xd4],value<-[0,1,2,0xffffffff]]++
      [rootCase ("mask-"++show mask) SubmitDescriptor [(0xd8,mask)] cold 0 | mask<-[0,2,4,6,0xffffffff]]++
      [rootCase ("override-"++show mode) SubmitDescriptor
        ([(offset,1) | offset<-[0,4..0xd4]]++[(0xd8,6),(0x140,mode),(0x1d0,1),(0x1d4,1),(0x1d8,1),(0x150,1),(0xe4,53)]) cold 0 | mode<-[1..8]]
      ++[let descriptorInput=foldl' (applyBytes descriptor) (input [(0x30,1)])
               [putHalf (plus descriptor 0x1ea) raw,putHalf (plus descriptor 0x234) width,putHalf (plus descriptor 0x236) height]
         in StageScenario (StageCaseName ("expansion-grid-axis-"++show (raw,width,height)))
           (rootPublication SubmitDescriptor) (args SubmitDescriptor) (initial 0 cold descriptorInput)
           (expansionEffects sources (cfg 0) (ExpansionMemory cold) descriptor descriptorInput record recordInput SubmitDescriptor) Nothing [] []
        | raw<-[0,252,253,352,353,650,750,751,65535],(width,height)<-[(0,0),(60,45),(8191,6143),(65535,65535)]]
      ++[rootCase ("embedded-filter-gains-"++show (embedded,disabled)) SubmitDescriptor
         [(0x84,1),(0x1ae,embedded),(0x1e4,disabled),(0x1ec,0x8080)] warm 1
        | (embedded,disabled)<-[(1,0),(0,1),(1,1)]]
    builderCases=
      [let descriptorInput=input [(0x140,mode),(0xe4,kind),(0xe8,secondary),(0x150,mix)]
           state=ExpansionState descriptor descriptorInput record recordInput (readExpansionMemory (ExpansionMemory cold)) []
           expected=finalPatches (leaf sources (cfg 0) part 0 state)
       in StageScenario (StageCaseName ("expansion-builder-"++suffix++"-"++show (mode,kind,secondary,mix)))
           (publication address suffix) [0x9a0000,0x9a1000] (initial 0 cold descriptorInput) expected Nothing [] []
      | (address,suffix,part)<-builderRoutes,(mode,kind,secondary,mix)<-[(0,0,0,0),(5,37,0,0),(6,53,1,17),(7,24,2,0)]]
    groupCases=
      [let descriptorInput=input [(0x104,kind),(0xe4,mode),(0x1b0,1)]
           state=ExpansionState descriptor descriptorInput record recordInput (readExpansionMemory (ExpansionMemory cold)) []
       in StageScenario (StageCaseName ("expansion-template-groups-"++show (groups,kind,mode,suffix)))
           (publication address suffix) [0x9a0000,groups,0x9a1000] (initial 0 cold descriptorInput)
           (finalPatches (leaf sources (cfg 0) (part groups) 0 state)) Nothing [] []
      | (address,suffix,part)<-[(0xd63a3c,"expand_template_selectors",GroupSelector),(0xd63ac4,"expand_template_gains",GroupGains)]
      , groups<-[0,1,2,3],kind<-[0,1,2,7,8],mode<-[0,53]]
    scalarCases=
      [StageScenario (StageCaseName ("expansion-nonzero-limit-"++show (value,limit)))
         (publication 0x73f7b8 "configuration_value_within_limit") [value,limit] [] [] (Just expected) [] []
       | (value,limit,expected)<-[(0,0,0),(0,1,0),(1,0,0),(1,1,1),(2,1,0),(2,2,1),(0xffffffff,0xffffffff,1)]]++
      [StageScenario (StageCaseName ("expansion-type-mode-copy-"++show (kind,mode)))
         (publication 0x73f78c "select_type_mode_value_copy") [kind,mode,0x12345678,0x87654321] [] [] (Just expected) [] []
       | (kind,mode,expected)<-[(0,1,0x12345678),(53,0,0x12345678),(53,1,0x87654321),(53,2,0x12345678)]]++
      [StageScenario (StageCaseName "expansion-finish-template-no-effect") (publication 0xd663e0 "finish_template")
         [0x9a0000,0x9a1000] [Patch descriptor initialDescriptor,Patch record recordInput]
         [Patch descriptor initialDescriptor,Patch record recordInput] Nothing [] []]
    gainCases=
      [let descriptorInput=input []
           cacheFinal=if warmCache then warm else cold++
             [putBytes cache [64],putWord (plus cache 4) 1,putBytes (plus cache (31*8)) [64],putWord (plus cache (31*8+4)) 1]
           recordFinal=applyBytes record recordInput (putBytes (plus record 0x25) [expected])
           cacheBytes=foldl' (applyBytes cache) (B.replicate 328 0) cacheFinal
       in StageScenario (StageCaseName ("expansion-filter-gain-literal-"++show warmCache))
         (publication 0xd5e848 "expand_filter_gain") [0x9a0000,0x9a1000]
         (initial 1 (if warmCache then warm else cold) descriptorInput)
         [Patch descriptor descriptorInput,Patch record recordFinal,Patch cache cacheBytes] Nothing [] []
      | (warmCache,expected)<-[(False,4),(True,24)]]
