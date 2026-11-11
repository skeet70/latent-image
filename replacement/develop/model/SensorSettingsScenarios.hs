{-# LANGUAGE GHC2021 #-}
module SensorSettingsScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (foldl')
import Data.Word
import PipelineModel
import Scenarios (Address(..), Call, Patch(..), readWord, word, wordBytes, wordsAt)
import BuffersScenarios (configurationInitial)

data SettingsExample = OrdinarySettings | PreparedSettings | SavedSettings deriving (Eq, Show)

sensorSettingsPublication :: Word32 -> String -> StagePublication
sensorSettingsPublication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_sensor_" ++ name))

sensorSettingsPublications :: [StagePublication]
sensorSettingsPublications = [sensorSettingsPublication address name | (address,name) <-
  [(0xe558d8,"copy_capture_settings"),(0xe55918,"collect_settings"),(0xd79408,"apply_settings"),
   (0xd78088,"translate_format"),(0xd786ac,"import_primary_level"),(0xd787b8,"import_strength"),
   (0xd787dc,"import_level_value"),(0xd7887c,"import_level"),(0xd5ddec,"select_level"),
   (0xd5deb0,"select_adjusted_level"),(0xd5defc,"compute_level"),(0xef6964,"profile_zero_first"),(0xef697c,"profile_zero_second")]] ++ sensorTemplatePublications

settingsConfigurationBase, settingsCaptureAddress :: Word32
settingsConfigurationBase = 0x3000000
settingsCaptureAddress = 0x28a0000

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]

recordImage :: Int -> [(Int,B.ByteString)] -> B.ByteString
recordImage size = foldl' replace (B.replicate size 0)
 where replace bytes (offset,value) = B.take offset bytes <> value <> B.drop (offset+B.length value) bytes

wordFields :: [(Int,Word32)] -> [(Int,B.ByteString)]
wordFields = map (fmap wordBytes)
halfFields :: [(Int,Word16)] -> [(Int,B.ByteString)]
halfFields = map (fmap halfBytes)
byteFields :: [(Int,Word8)] -> [(Int,B.ByteString)]
byteFields = map (fmap B.singleton)

settingsParameters :: B.ByteString
settingsParameters = recordImage 0x498 (halfFields [(0x28,320)])
settingsCapture :: B.ByteString
settingsCapture = B.pack [0..56]

settingsConfiguration :: SettingsExample -> [Patch]
settingsConfiguration example = configurationInitial settingsConfigurationBase
  ([(0xa910+fromIntegral index*2+byteIndex,fromIntegral (threshold `shiftR` (8*fromIntegral byteIndex))) | (index,threshold)<-zip [0::Int ..] ([100,200..900]::[Word16]),byteIndex<-[0,1]] ++
   [(0x3e3d40+index,fromIntegral (index*2)) | index<-[0..13]] ++
   [(0xa922,1),(0xa92d,6),(0xa92e,8),(0xa92f,9),(0xa930,10),(0xa931,5),(0x18ac0,12),(0xbbf6,if example==PreparedSettings then 16 else 11)])

settingsStageInitial :: Word32 -> SettingsExample -> [Patch]
settingsStageInitial stage example =
  [Patch (Address stage) (B.replicate 0x3000 0)] ++
  [word (stage+offset) value | (offset,value)<-
    [(0x78,if example==SavedSettings then 11 else 1),(0x4a8,if example==PreparedSettings then 8 else 0),
     (0x504,4),(0x500,3),(0x4b4,case example of OrdinarySettings->5;PreparedSettings->1;SavedSettings->63),
     (0x710,if example==PreparedSettings then 0x400 else 0),(0x638,24),(0x4bc,13),(0x518,6),(0x51c,9),(0x508,5),
     (0x548,1),(0x524,4),(0x5f4,4),(0x5f8,1),(0x5f0,7),(0x530,12),(0x4ac,1),(0x62c,1),(0x668,0x12345),
     (0x664,0xff9c),(0x448,640),(0x44c,480),(0x2710,0x12345678),(0x223c,1200),(0x2240,800),
     (0x224c,350),(0x2254,5),(0x2260,2),(0x2708,2),(0x1884,settingsCaptureAddress),(0x1d3c,0xfffffffe),
     (0x1b74,2)]] ++
  [Patch (Address (stage+offset)) (halfBytes value) | (offset,value)<-
    [(0x1682,12),(0x6c2,1),(0x2222,0x4321),(0x2244,175),(0x2246,0xabcd),(0x2264,0xffce),(0x2700,0x123),(0x2704,600),(0x1b74+30,17),(0x1b74+44+30,43)]] ++
  [Patch (Address (stage+offset)) (B.singleton value) | (offset,value)<-[(0x2220,2),(0x2248,9),(0x2249,10),(0x2258,6),(0x2702,5),(0x270e,75),(0x188c,27),(0x1d30,2)]] ++
  [wordsAt (stage+0x6cc) [11..15],wordsAt (stage+0x6e0) [100..109],Patch (Address (stage+0x2268)) settingsParameters,
   Patch (Address (settingsCaptureAddress+0x14c)) settingsCapture]

settingsPacket :: Word32 -> SettingsExample -> Word32 -> B.ByteString
settingsPacket stage example operation = recordImage 0x601
  (wordFields [(0x8,if example==PreparedSettings then 8 else 0),(0xc,4),(0x10,3),(0x14,case example of OrdinarySettings->5;PreparedSettings->25;SavedSettings->63),
    (0x18,13),(0x1c,6),(0x20,9),(0x24,5),(0x28,1),(0x2c,4),(0x30,4),(0x34,1),(0x38,7),(0x3c,12),
    (0x48,stage+0x1b74),(0x50,if example==PreparedSettings then 1 else 0),(0x54,1),(0x68,if example==PreparedSettings then 0 else 1),
    (0x6c,if operation==8 then 1 else 0),(0x74,if example==OrdinarySettings then 9 else 11),
    (0x7c,case operation of 10->2;11->1;12->1;_->0),(0x80,1),(0x98,1),(0x9c,1),(0xf0,0x12345678),
    (0xf8,1200),(0xfc,800),(0x108,350),(0x110,5),(0x5fc,2)] ++
   halfFields [(0x40,12),(0xa0,0x2345),(0xa2,0xff9c),(0xea,640),(0xec,480),(0xf6,0x4321),(0x100,175),(0x102,0xabcd),(0x154,0xffce),(0x5f4,0x123),(0x5f8,600)] ++
   byteFields [(0x64,if example==PreparedSettings then case operation of 12->2;11->1;_->0 else 0),
     (0x71,if example==OrdinarySettings then 0 else 1),(0x78,if example==SavedSettings then 1 else 0),(0x79,if example==SavedSettings then 27 else 0),
     (0xf4,2),(0x104,9),(0x105,10),(0x114,6),(0x5f6,5),(0x600,75)] ++
   [(0x118,B.concat (map wordBytes [11..15])),(0x12c,B.concat (map wordBytes [100..109])),(0x158,settingsParameters),
    (0xb0,if example==SavedSettings then settingsCapture else B.replicate 57 0)])

settingsDescriptor :: B.ByteString -> SettingsExample -> Word32 -> B.ByteString
settingsDescriptor image example operation = recordImage 0x23c
  (wordFields ([(0xe4,if example==OrdinarySettings then 5 else 53),(0xe8,3),(0xec,if example==PreparedSettings then 1 else 0),
    (0xf4,level 1),(0xf8,2),(0xfc,4),(0x100,1),(0x104,3),(0x10c,4),(0x114,1),(0x11c,1200),(0x120,800),
    (0x124,tableOrigin),(0x128,1),(0x12c,100),(0x130,175),(0x150,43),(0x154,4),(0x158,level 3),(0x15c,1),
    (0x17c,if example==PreparedSettings then 0 else 1),(0x18c,if operation==8 then 1 else 0),(0x194,4),(0x19c,6),
    (0x1a8,if example==OrdinarySettings then 9 else 11),(0x1b0,case operation of 10->2;11->1;12->1;_->0),(0x1b4,1),(0x1bc,1),
    (0x1e0,if example==PreparedSettings then 0 else 1),(0x1e4,1),(0x228,1),(0x230,0xffffffce),(0x238,0x12345678)] ++
    (if example==OrdinarySettings then [] else [(0x180,level conditionalIndex),(0x184,100),(0x188,conditionalValue)])) ++
   halfFields [(0x118,0xabcd),(0x19a,320),(0x1a0,0x123),(0x1a4,600),(0x1a6,100),(0x1e8,0x2345),(0x1ea,0xff9c),(0x234,640),(0x236,480)] ++
   byteFields [(0x11a,2),(0x11b,6),(0x16c,1),(0x178,conditionalEnabled),(0x198,9),(0x1a2,5),
    (0x1ac,if example==OrdinarySettings then 0 else 1),(0x1ad,75),(0x1ae,if example==SavedSettings then 1 else 0),(0x1af,if example==SavedSettings then 27 else 0)] ++
   [(0x1ec,if example==SavedSettings then settingsCapture else B.replicate 57 0)])
 where
  level index = readWord image (0xd5e630+index*4)
  tableOrigin = readWord image 0xd797a4 `shiftR` 16
  conditionalEnabled = if example==PreparedSettings then case operation of 12->2;11->1;_->0 else 0
  conditionalValue = case conditionalEnabled of 1->110;2->90;_->70
  conditionalIndex = if conditionalValue>=100 then 1 else 0

collectSettingsLaw :: B.ByteString -> Word32 -> Word32 -> Word32 -> SettingsExample -> Word32 -> ([Patch],[Patch],[Call])
collectSettingsLaw image stage descriptor packet example operation =
  (settingsStageInitial stage example ++ settingsConfiguration example ++
    [Patch (Address descriptor) (B.replicate 0x23c 0),Patch (Address packet) (B.replicate 0x601 0)],
   [Patch (Address packet) (settingsPacket stage example operation),Patch (Address descriptor) (settingsDescriptor image example operation)],[])

sensorSettingsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
sensorSettingsScenarios image _resolve = collectCases ++ applyCases ++ copyCases ++ levelCases ++ adjustmentCases ++ computeCases ++ importCases ++ formatCases ++ zeroCases ++ sensorTemplateScenarios image
 where
  stage=0x1580000; descriptor=0x9a8000; packet=0x9a9000; output=0x9aa000; bucket=0x9aa004
  make name address symbol arguments initial final result = StageScenario (StageCaseName ("sensor-settings-"++name)) (sensorSettingsPublication address symbol) arguments initial final result [] []
  collectCases = [let (initial,final,calls)=collectSettingsLaw image stage descriptor packet example operation
     in (make ("collect-"++show example++"-"++show operation) 0xe55918 "collect_settings" [stage,descriptor,packet,operation,0] initial final Nothing) {stageCalls=calls}
     | example<-[OrdinarySettings,PreparedSettings,SavedSettings],operation<-[8,10,11,12,14]]
  applyCases = [make ("apply-"++show example) 0xd79408 "apply_settings" [descriptor,packet,stage+0x2220]
    (settingsStageInitial stage example ++ settingsConfiguration example ++ [Patch (Address packet) (settingsPacket stage example 12),Patch (Address descriptor) (B.replicate 0x23c 0)])
    [Patch (Address descriptor) (settingsDescriptor image example 12),Patch (Address packet) (settingsPacket stage example 12)] Nothing
    | example<-[OrdinarySettings,PreparedSettings,SavedSettings]]
  copyCases = [make "capture-bytes" 0xe558d8 "copy_capture_settings" [stage,output]
    [word (stage+0x1884) settingsCaptureAddress,Patch (Address (settingsCaptureAddress+0x14c)) settingsCapture,Patch (Address output) (B.replicate 61 0xaa)]
    [Patch (Address output) (settingsCapture<>B.replicate 4 0xaa)] Nothing]
  levelCases = [make ("threshold-index-"++show value) 0xd5ddec "select_level" [22,fromIntegral value,0]
    (settingsConfiguration OrdinarySettings) [] (Just index)
    | (value,index)<-[(-1,0),(0,0),(99,0),(100,1),(199,1),(200,2),(899,8),(900,9),(2147483647,9)]::[(Int32,Word32)]]
  adjustmentCases = [make ("adjustment-"++show adjustment) 0xd5deb0 "select_adjusted_level" [0,175,fromIntegral adjustment]
    (settingsConfiguration OrdinarySettings) [] (Just expected)
    | let base=readWord image 0xd5e634,(adjustment,expected)<-[(0,base),(1,min 9 (base-1)),(10,base),(20,min 9 (base+1)),(-10,min 9 (base-2))]::[(Int32,Word32)]]
  computeCases = [make ("third-bucket-"++show value) 0xd5defc "compute_level" [output,bucket,0,fromIntegral value,10]
    (settingsConfiguration OrdinarySettings) [word output (readWord image (0xd5e630+index*4)),word bucket expected] Nothing
    | (value,index,expected)<-[(99,0,100),(100,1,0),(132,1,0),(133,1,1),(166,1,1),(167,1,2),(200,2,10),(599,5,42),(600,6,0)]::[(Int32,Word32,Word32)]]
  importCases = [make ("import-value-"++show (value,enabled)) 0xd787dc "import_level_value" [output,fromIntegral value,enabled]
    (settingsConfiguration OrdinarySettings) [word output expected] Nothing
    | (value,enabled,expected)<-[(175,0,70),(175,1,110),(175,2,90),(175,3,80),(175,4,70),(-175,0,50),(-2147483648,1,50)]::[(Int32,Word32,Word32)]]
  formatCases = [make ("format-"++show kind) 0xd78088 "translate_format" [output,kind] [] [word output expected] Nothing
    | (kind,expected)<-[(0,0),(0x10,29),(0x51,29),(0x34,30),(0x58,30),(0x35,31),(0x59,31),(0x11,32),(0x52,32),(0x36,33),(0x5a,33),(0x37,34),(0x5b,34),(0x3e,36),(0x18,37),(0x3f,53),(0x68,0),(0xffffffff,0)]]
  zeroCases = [make name address name [] [] [] (Just 0) | (address,name)<-[(0xef6964,"profile_zero_first"),(0xef697c,"profile_zero_second")]]

data TableOperation = ColorTableOperation | NoiseTableOperation deriving (Eq, Show)
tableOperationWord :: TableOperation -> Word32
tableOperationWord ColorTableOperation = 13
tableOperationWord NoiseTableOperation = 14

sensorTemplatePublications :: [StagePublication]
sensorTemplatePublications = [sensorSettingsPublication address name | (address,name)<-
  [(0xd77a04,"template_index"),(0xd77a48,"configure_descriptor"),(0xd77590,"update_template_flags"),(0xd5db1c,"expand_template"),
   (0x73fc64,"selection_index"),(0x73f760,"select_type_mode_value"),(0xd61350,"read_selection_flag"),(0xd61388,"read_configuration_flag8"),
   (0xd68a1c,"read_primary_configuration"),(0xd68a30,"read_secondary_configuration"),(0xd68a44,"read_level_configuration"),
   (0xd60c5c,"read_mode_configuration"),(0xd5e658,"read_noise_configuration"),(0xd60594,"board_configuration_enabled"),
   (0xd66f00,"mode_selection_enabled"),(0xd66f90,"selection_mask_enabled"),(0xd69ec0,"read_mode_selector"),(0xd69f44,"read_alternate_selector"),
   (0xd69b24,"threshold_enabled"),(0xd5fab4,"configuration_type_enabled"),(0xd6235c,"read_selected_configuration"),
   (0xd63998,"compute_configuration_mask"),(0xd6415c,"collect_template_inputs")]]

templateConfiguration :: [Patch]
templateConfiguration = configurationInitial settingsConfigurationBase
  [(0xbf,1),(0xa935,1),(0xa936,0),(0xa938,0),(0xc630,2),(0xc631,2),(0xce71,32),(0xc647,3),
   (0xc0,32),(0x3e3dc8,2),(0xae58,32),(0xae5d,4),(0x235,1),(0xaf93,0x55),(0xaf94,0),
   (0xb40b,5),(0xb415,5),(0xb8ab,10),(0xb8b5,10),(0xb8d3,5),(0xb8dd,5),(0xba9d,0),
   (0xc0be,1),(0xc105,0x55),(0xc12f,1),(0xc264,1),(0xc315,0),(0xc5fe,1),(0xc0bf,0),(0xc0c1,1)]

templateNumber :: Word32 -> Word32
templateNumber kind | kind<=15 = kind | kind==16 = 15 | kind==17 = 16 | otherwise = 0

originalTemplate :: B.ByteString -> Word32 -> B.ByteString
originalTemplate image kind = B.take 60 (B.drop (fromIntegral (0xd77ad4+templateNumber kind*60-0xbf1a00)) image)

templateFlagImage :: Word32 -> B.ByteString -> B.ByteString
templateFlagImage kind original = B.pack [result index | index<-[0..55]] <> B.drop 56 original
 where
  flags index = B.index original index
  sections = foldr (.|.) 0 [fromIntegral (B.index original (56+index)) `shiftL` (8*index) | index<-[0..3]] :: Word32
  enabled group = sections .&. group /= 0
  alternating = [1,0,1,0,1,0,1]
  colorMask = [0,1,0,1,1,0,1,0]
  collected = [1,1,1,1,1,0,1,0,1,0,1,0,1,1,1,0,1,0,1,0]
  result index
    | index==0 && enabled 1 && flags 0==1 && flags 2==1 = 0
    | index==1 && enabled 1 && flags 1/=0 && flags 2==1 = 0
    | index==3 && enabled 0x100 && flags 4/=0 = 0
    | index==4 && enabled 0x100 && flags 4/=0 = 2
    | index==10 && enabled 8 && flags 9==1 = if enabled 2 then 0 else 1
    | index==13 && enabled 0x20 && flags 13/=0 = 0
    | index>=14 && index<=20 && enabled 0x20 && flags index==1 = alternating!!(20-index)
    | index==21 || index==23, enabled 0x40, kind==9 = 0
    | index==22 && enabled 0x40 && kind/=9 = 0
    | index>=24 && index<=31 && enabled 0x10 && kind==9 = 0
    | index>=24 && index<=31 && enabled 0x10 && flags index==1 = colorMask!!(index-24)
    | index>=32 && index<=33 && enabled 0x80 && kind `elem` [3,9,13] = 0
    | index==33 && enabled 0x80 && flags 33/=0 = 0
    | index>=34 && index<=53 && enabled 0x1000 = collected!!(index-34)
    | otherwise = flags index

configuredDescriptor :: B.ByteString -> SettingsExample -> TableOperation -> Word32 -> B.ByteString
configuredDescriptor image example operation kind = B.concat [wordBytes (fromIntegral (B.index flags index)) | index<-[0..53]] <> B.drop 56 flags <> B.drop 220 descriptor
 where
  descriptor=settingsDescriptor image example (tableOperationWord operation)
  flags=templateFlagImage kind (originalTemplate image kind)

configureDescriptorLaw :: B.ByteString -> Word32 -> SettingsExample -> TableOperation -> Word32 -> ([Patch],[Patch],[Call])
configureDescriptorLaw image descriptor example operation kind =
  (templateConfiguration ++ [Patch (Address descriptor) (settingsDescriptor image example (tableOperationWord operation))],
   [Patch (Address descriptor) (configuredDescriptor image example operation kind)],[])

sensorTemplateScenarios :: B.ByteString -> [StageScenario]
sensorTemplateScenarios image = configureCases ++ flagCases ++ expansionCases ++ directoryCases ++ selectionCases ++ modeValueCases ++ getterCases
 where
  descriptor=0x9a8000; record=0x9ab000; output=0x9ac000
  make name address symbol arguments initial final result = StageScenario (StageCaseName ("sensor-template-"++name)) (sensorSettingsPublication address symbol) arguments initial final result [] []
  configureCases = [let (initial,final,calls)=configureDescriptorLaw image descriptor example NoiseTableOperation kind
    in (make ("configure-"++show (example,kind)) 0xd77a48 "configure_descriptor" [descriptor,kind,0] initial final Nothing) {stageCalls=calls}
    | example<-[OrdinarySettings,PreparedSettings,SavedSettings],kind<-[0..17]]
  flagCases = [let original=B.replicate 56 1<>wordBytes sections
    in make ("flags-"++show (kind,sections)) 0xd77590 "update_template_flags" [record,0,kind,descriptor]
      (templateConfiguration ++ [Patch (Address descriptor) (settingsDescriptor image OrdinarySettings 14),Patch (Address record) original])
      [Patch (Address record) (templateFlagImage kind original)] Nothing
    | kind<-[3,5,9,13],sections<-[0,0x17fb,1,0x40,0x1000]]
  expansionCases = [make "expand-byte-width" 0xd5db1c "expand_template" [record,output]
    [Patch (Address record) (B.pack [0..55]<>wordBytes 0x12345678),Patch (Address output) (B.replicate 224 0xaa)]
    [Patch (Address output) (B.concat (map wordBytes [0..53])<>wordBytes 0x12345678<>B.replicate 4 0xaa)] Nothing]
  directoryCases = [make ("directory-"++show kind) 0xd77a04 "template_index" [kind] [] [] (Just (templateNumber kind)) | kind<-[0..18]++[0xffffffff]]
  selectionCases = [make ("selection-"++show (mode,enabled,kind)) 0x73fc64 "selection_index" [descriptor,output]
    [word (descriptor+0x19c) mode,word (descriptor+0x228) enabled,word (descriptor+0x154) kind] [Patch (Address output) (B.singleton expected)] Nothing
    | (mode,enabled,kind,expected)<-[(2,1,4,0),(2,0,4,5),(0,1,0,1),(0,1,1,2),(0,1,2,3),(0,1,3,4),(0,1,4,5),(0,1,5,1)]]
  modeValueCases = [make ("mode-value-"++show (kind,mode)) 0x73f760 "select_type_mode_value" [kind,mode,3,8] [] [] (Just expected)
    | (kind,mode,expected)<-[(53,1,8),(53,0,3),(53,2,3),(5,1,3)]]
  getterCases = [make ("configuration-"++name) address name [descriptor]
    (templateConfiguration ++ [Patch (Address descriptor) (settingsDescriptor image OrdinarySettings 14)]) [] (Just expected)
    | (address,name,expected)<-[(0xd68a1c,"read_primary_configuration",1),(0xd68a30,"read_secondary_configuration",0),(0xd68a44,"read_level_configuration",0),
      (0xd60c5c,"read_mode_configuration",0),(0xd5e658,"read_noise_configuration",0),(0xd66f00,"mode_selection_enabled",1),
      (0xd66f90,"selection_mask_enabled",1),(0xd69ec0,"read_mode_selector",2),(0xd69f44,"read_alternate_selector",0),(0xd69b24,"threshold_enabled",1),
      (0xd5fab4,"configuration_type_enabled",1),(0xd6235c,"read_selected_configuration",5)]]
