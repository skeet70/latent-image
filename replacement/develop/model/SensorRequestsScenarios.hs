{-# LANGUAGE GHC2021 #-}
module SensorRequestsScenarios where
import qualified Data.ByteString as B
import Data.Bits
import Data.Word
import PipelineModel
import Scenarios (Address(..),Call,Patch(..),word,wordBytes,wordsAt)
import BuffersScenarios (configurationInitial)
import qualified SensorSettingsScenarios as Settings
import OutputExpansionScenarios (ExpansionPhase(..),ExpansionMemory(..),expansionEffectsFromImage)

sensorRequestPublication :: Word32 -> String -> StagePublication
sensorRequestPublication address name=StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_"++name))
sensorOptionPublication :: Word32 -> String -> StagePublication
sensorOptionPublication address name=StagePublication (Address address) (StageSymbol ("xm1_pipeline_sensor_"++name))

sensorRequestPublications :: [StagePublication]
sensorRequestPublications=[sensorRequestPublication 0xe57758 "prepare_processing",sensorRequestPublication 0xe560cc "collect_processing_flags",sensorRequestPublication 0xe561b4 "setup_processing",sensorRequestPublication 0xe5639c "saved_request_submit"]++
 [sensorOptionPublication address name|(address,name)<-[(0xe56cb0,"capture_gain"),(0xda31b0,"find_alignment"),(0xda1f54,"publish_configurations"),(0xda208c,"reset_configurations"),(0xd5da58,"publish_descriptor"),(0xd5dae8,"publish_descriptor"),(0xd5daf8,"reset_descriptor"),(0xd5db0c,"reset_descriptor"),(0xd5d244,"descriptor_acquire"),(0xd5d254,"descriptor_release"),(0xd6b02c,"capture_descriptor")]]
 ++[sensorOptionPublication address name|(address,name)<-[(0x67472c,"start_output"),(0x67486c,"ack_irq"),(0x6756fc,"reset_irq")]]

sensorRequestScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
sensorRequestScenarios _ _=prepareCases++gainCases++alignmentCases++flagCases++outputCases++irqCases
 where
  stage=0x1580000;request=0x9c0000;features=0x9c1000;auxiliary=0x9c2000;source=0x9c3000;output=0x9c4000
  make publication name arguments initial final result=StageScenario (StageCaseName ("sensor-request-"++name)) publication arguments initial final result [] []
  configuration=configurationInitial 0x3000000 [(0xc0,4),(0xb2,0),(0x2f8d,251),(0x2f8e,255),(0x1dea,2),(0x1deb,1),(0x1dd7,0x34),(0x1dd8,0x12),(0x1dda,9),(0x1dd9,8),(0x1dec,7),(0x1ded,0x78),(0x1dee,0x56)]
  gainInitial=[word (stage+0x4a8) 0,word (stage+0x78) 0,Patch (Address (stage+0x2224)) (B.pack [0xc8,1,123,0,0x15,3])]
  prepareCases=[make (sensorRequestPublication 0xe57758 "prepare_processing") ("flags-"++show flags)
    [stage,request,source,features,42,0,0,0,output,source]
    (configuration++gainInitial++[word (stage+0x640) 2,word (stage+0x648) 0xfffffffc,Patch (Address (stage+0x2268+0x3c)) (B.pack [0x21,0x43]),word (request+24) auxiliary,word (auxiliary+4) 0x12345678,word features flags,Patch (Address output) (B.replicate 40 0xaa)])
    ([word features (flags .&. complement 0xc00),word 0x1568308 0x12345678,wordsAt output [if flags .&. 0x200/=0 then 0x15682e8 else 0,0,0,0,0,0,0,0,0,if flags .&. 0x40/=0 then 0x1569600 else 0]]++
     [wordsAt 0x1569600 [123,456,789,256]|flags .&. 0x40/=0]++[wordsAt 0x15682e8 [2,7,0x4321,0x1234,0x5678,8,9]|flags .&. 0x200/=0]) Nothing
    |flags<-[0,0x40,0x200,0x240,0xc00]]
  gainCases=[make (sensorOptionPublication 0xe56cb0 "capture_gain") "gain-record-halfword-order" [stage,output] gainInitial [wordsAt output [123,456,789,256]] Nothing]
  alignmentCases=[make (sensorOptionPublication 0xda31b0 "find_alignment") ("alignment-"++show value) [value,output] [word output 0xa5a5a5a5] [word output (if gcd value 256>=16 then gcd value 256 else 0xa5a5a5a5)] Nothing|value<-[0,1,2,4,6,16,24,128,255,256,512,1024,0xfffffffe]]
  flagCases=[make (sensorRequestPublication 0xe560cc "collect_processing_flags") ("expanded-flags-"++show values) [stage,request,output]
    [Patch (Address request) (B.replicate 0x1260 0xaa)]
    [wordsAt output [1,1,1,1,1,1,1,1,0,1,1,1,1]] Nothing|values<-[0::Word32]]
  outputCases=[make (sensorOptionPublication 0x67472c "start_output") ("output-control-"++show (flags,first,second,alternate,old))
    [flags,first,second,alternate] [word 0xfff40004 old,word 0xfff400bc 0xaabbccdd]
    [word 0xfff40004 ((if flags==0x80 then old .&. complement 0x18000 else flags .&. 0x3ff) .|. ((first .&. 1) `shiftL` 15) .|. ((second .&. 1) `shiftL` 16) .|. (if flags==0x80 then 0 else (alternate .&. 1) `shiftL` 24)),
     word 0xfff400bc (if flags==0x80 then 1 else 0xaabbccdd)] Nothing
    |flags<-[0,0x40,0x80,0x3ff,0xffffffff],(first,second,alternate)<-[(0,0,0),(1,0,1),(0,1,1),(3,2,4)],old<-[0,0xa55af00f]]
  irqCases=[make (sensorOptionPublication 0x67486c "ack_irq") ("acknowledge-register-"++show status) [] [word 0xfff40084 status] [word 0xfff40084 status] (Just 0)|status<-[0,1,0xffffffff]]++
    [make (sensorOptionPublication 0x6756fc "reset_irq") "reset-interrupt-mask" [] [word 0xfff40084 0xa55af00f] [word 0xfff40084 0x703f000f] Nothing]

noiseTableLaw :: B.ByteString -> Word32 -> Word32 -> Word32 -> ([Patch],[Patch],[Call])
noiseTableLaw bundle stage descriptor output=noiseTableLawFromMemory bundle stage descriptor output []

noiseTableLawFromMemory :: B.ByteString -> Word32 -> Word32 -> Word32 -> [Patch] -> ([Patch],[Patch],[Call])
noiseTableLawFromMemory bundle stage descriptor output entryMemory =
  (configurationZero:settingsInitial++Settings.templateConfiguration++cacheInitial++
     [Patch (Address (descriptor+0x23c)) descriptorTail,Patch (Address output) recordInitial]++entryMemory,
   [patch|patch@(Patch (Address address) _)<-settingsFinal,address/=descriptor]++
     expansionEffectsFromImage bundle configurationByte (ExpansionMemory (cacheInitial++entryMemory)) (Address descriptor) configured (Address output) recordInitial SubmitDescriptor,
   settingsCalls)
 where
  image=B.drop 2631680 bundle
  (settingsInitial,settingsFinal,settingsCalls)=Settings.collectSettingsLaw image stage descriptor 0x15689a8 Settings.OrdinarySettings 14
  configurationZero=Patch (Address Settings.settingsConfigurationBase) (B.replicate 0x400000 0)
  configurationMemory=configurationZero:Settings.settingsConfiguration Settings.OrdinarySettings++Settings.templateConfiguration++entryMemory
  configurationByte key=lookupByte (Settings.settingsConfigurationBase+key) (reverse configurationMemory)
  lookupByte address (Patch (Address base) bytes:rest)
    | address>=base && fromIntegral (address-base)<B.length bytes=B.index bytes (fromIntegral (address-base))
    | otherwise=lookupByte address rest
  lookupByte address []=error ("Unmapped noise-table configuration byte: "++show address)
  descriptorTail=B.replicate 8 0xaa
  configured=Settings.configuredDescriptor image Settings.OrdinarySettings Settings.NoiseTableOperation 2<>descriptorTail
  recordInitial=B.replicate 4704 0xaa
  cacheInitial=[Patch (Address 0x95ada0) (B.replicate 328 0)]
