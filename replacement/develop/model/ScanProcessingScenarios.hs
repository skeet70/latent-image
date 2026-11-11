module ScanProcessingScenarios
  ( scanProcessingPublications
  , scanProcessingScenarios
  , scanTranslateSmallKindPublication
  , scanTranslateSmallKindScenario
  , scanConfigurationInitial
  , scanComponentContextInitial
  ) where

import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import Data.Int (Int16)
import Data.Word (Word8, Word16, Word32)
import PipelineModel
import Scenarios (Address(..), Patch(..), word, wordBytes, wordsAt)

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_scan_" ++ suffix))

scanProcessingPublications :: [StagePublication]
scanProcessingPublications =
  [ publication 0xd7a820 "components"
  , publication 0xd7a2a0 "prepare_components"
  , publication 0xd79e38 "update_components"
  , publication 0xd6ca88 "process_components"
  , publication 0xd6cc70 "validate_regions"
  , publication 0xd753b8 "export_diagnostics"
  , scanTranslateSmallKindPublication
  , publication 0xd79890 "translate_operation"
  , publication 0xd79db4 "translate_selection"
  , publication 0xd79bd0 "packing"
  , publication 0xd79c14 "channel_layout"
  , publication 0xd79c3c "frame_mode"
  , publication 0xd79c58 "component_kind"
  , publication 0xd79d90 "initialize_pair"
  , publication 0xd7a1dc "zero_one"
  , publication 0xd7a208 "one_two"
  , publication 0xd7a26c "enabled_value"
  , publication 0xd79cf4 "select_word_table"
  , publication 0x69084c "word_lookup"
  ]

scanTranslateSmallKindPublication :: StagePublication
scanTranslateSmallKindPublication = publication 0xd78a10 "translate_small_kind"

scenario :: String -> StagePublication -> [Word32] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario
scenario name entry arguments initial final result = StageScenario (StageCaseName ("scan-" ++ name)) entry arguments initial final result [] []

scanTranslateSmallKindScenario :: Word32 -> Word32 -> StageScenario
scanTranslateSmallKindScenario destination kind = scenario ("small-kind-" ++ show kind) scanTranslateSmallKindPublication [destination,kind] [word destination 0xdeadbeef] [word destination result] Nothing
  where result = case kind of 2 -> 1; 3 -> 2; 4 -> 3; 5 -> 4; _ -> 0

scanConfigurationInitial :: Word32 -> [(Word32, Word8)] -> [Patch]
scanConfigurationInitial base values = word 0x96ca50 base : [Patch (Address (base + key)) (B.singleton value) | (key,value) <- values]

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8)]
halves :: Word32 -> [Word16] -> Patch
halves address = Patch (Address address) . B.concat . map halfBytes
blank :: Word32 -> Int -> Word8 -> Patch
blank address length value = Patch (Address address) (B.replicate length value)

context, request, workspace, output, plane, profile, debug, scratch, configuration :: Word32
context = 0x9a0000
request = 0x9a1000
workspace = 0x9a2000
output = 0x9a4000
plane = 0x9a5000
profile = 0x9a6000
debug = 0x2000000
scratch = 0x2100000
configuration = 0x2200000

scanComponentContextInitial :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
scanComponentContextInitial address planeAddress profileAddress debugAddress scratchAddress enabled =
  [ blank address 0x254 0
  , word address enabled
  , word (address+0x1d0) 0
  , word (address+0x1dc) 17,word (address+0x1e0) 19
  , word (address+0x1d4) 3,word (address+0x1d8) 7
  , word (address+0x1e4) 23,word (address+0x1e8) 37
  , word (address+0x1ec) 41,word (address+0x1f0) 43
  , halves (address+0x1f6) [47]
  , word (address+0x1f8) profileAddress
  , word (address+0x1fc) scratchAddress
  , word (address+0x200) debugAddress
  , word (address+0x204) planeAddress
  , word (address+0x208) 53,word (address+0x20c) 59
  , word (address+0x210) 5,word (address+0x214) 4
  , word (address+0x21c) (address+0x300)
  , word (address+0x224) 24,word (address+0x228) 9
  , word (address+0x230) 4
  , halves (address+0x1c2) [61,67,71]
  , halves (address+0x1ca) [73,79,83]
  , halves (address+0x238) [89,97,101]
  , wordsAt (address+0x300) [103,107]
  ]

scanProcessingScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
scanProcessingScenarios _image _owned = smallKinds ++ scalars ++ selectionCases ++ packingCases ++ componentKinds ++ operationCases ++ readers ++ updaterCases ++ validationCases ++ diagnosticCases ++ lookupCases ++ tableCases ++ enabledCases ++ preparationCases
  where
    entry address suffix = publication address suffix
    smallKinds = map (scanTranslateSmallKindScenario output) [0..7] ++ [scanTranslateSmallKindScenario output maxBound]
    scalars = [scenario (suffix ++ "-" ++ show value) (entry address suffix) [value] [] [] (Just expected) | (address,suffix,values) <- [(0xd7a1dc,"zero_one",[(0,0),(1,1),(2,2),(maxBound,2)]),(0xd7a208,"one_two",[(0,0),(1,1),(2,2),(3,0),(maxBound,0)])], (value,expected) <- values]
    selectionCases = [scenario ("selection-" ++ show value) (entry 0xd79db4 "translate_selection") [value,output] [word output 99] [word output expected] Nothing | (value,expected) <- [(0,0),(1,1),(2,2),(3,3),(4,4),(5,5),(6,6),(7,4),(8,4),(9,4),(10,4),(11,0),(maxBound,0)]]
    packingCases = [scenario ("packing-" ++ show flags) (entry 0xd79bd0 "packing") [flags,output,output+4,output+8] [blank output 12 0xcc] [wordsAt output expected] Nothing | (flags,expected) <- [(0,[0,2,1]),(16,[1,3,2]),(maxBound,[1,3,2])]] ++
      [scenario ("layout-" ++ show flags) (entry 0xd79c14 "channel_layout") [flags,output] [] [word output expected] Nothing | (flags,expected) <- [(0,0),(0x1000,1),(0x2000,0),(0x3000,2),(maxBound,2)]] ++
      [scenario ("frame-mode-" ++ show flags) (entry 0xd79c3c "frame_mode") [flags,output] [] [word output 2] Nothing | flags <- [0,maxBound]] ++
      [scenario "initialize-pair" (entry 0xd79d90 "initialize_pair") [output,output+4] [blank output 8 0xcc] [halves output [0,0xcccc],word (output+4) 4] Nothing]
    componentKinds = [scenario ("component-kind-" ++ show mode ++ "-" ++ show selection) (entry 0xd79c58 "component_kind") [mode,selection,output] [] [word output expected] Nothing | (mode,values) <- [(0,[(0,0),(1,8),(2,1),(3,8)]),(1,[(0,2),(1,8),(2,3),(3,8)]),(2,[(0,4),(1,6),(2,5),(3,7),(4,8)]),(3,[(0,4),(maxBound,4)])], (selection,expected) <- values]
    operationCases = [scenario ("operation-" ++ show value) (entry 0xd79890 "translate_operation") [output,value] [] [word output expected] Nothing | (value,expected) <- [(0,0),(1,1),(4,4),(5,5),(8,7),(24,21),(30,0),(33,27),(37,31),(63,22),(78,7),(95,5),(102,5),(103,0),(maxBound,0)]] ++
      [scenario ("operation-config-" ++ show value ++ "-" ++ show enabled) (entry 0xd79890 "translate_operation") [output,value] (scanConfigurationInitial configuration [(key,enabled)]) [word output (if enabled == 0 then 5 else expected)] Nothing | (value,key,expected) <- [(48,0x7955,7),(50,0x7951,9),(51,0x7951,10),(52,0x7954,13),(54,0x7953,14),(56,0x7952,16)], enabled <- [0,1,255]]
    readers = [scenario "read-disabled" (entry 0xd7a820 "components") [context,output] [word context 0,halves (context+0x238) [11,22,65535],blank output 8 0xcc] [halves output [11,22,65535,0xcccc]] Nothing]
    updaterCases = [scenario ("update-empty-" ++ show diagnostics) (entry 0xd79e38 "update_components") [request,output] ([blank request 0x2ac 0,halves (request+0x218) [123,456,789],word (request+0x2a8) diagnostics,word (request+0x28c) debug,blank 0x14687f8 0xcb8 0xcc,blank output 8 0xcc] ++ if diagnostics == 1 then [blank (debug+0xd7e54) 0xd1c 0xcc] else []) ([halves output [123,456,789,0xcccc],blank 0x14687f8 0xcb8 0] ++ if diagnostics == 1 then [Patch (Address (debug+0xd7e54)) emptyDiagnostics] else []) Nothing | diagnostics <- [0,1,2]]
    validationCases = [scenario "validate-empty" (entry 0xd6cc70 "validate_regions") [request] [blank request 0x2ac 0] [] (Just 0)] ++
      [scenario "validate-ordinary-two" (entry 0xd6cc70 "validate_regions") [request] [blank request 0x2ac 0,word request 2,word (request+0x280) 7] [word (request+0x34) 1,word (request+0x68) 1] (Just 1)] ++
      [scenario "validate-raw-rejected" (entry 0xd6cc70 "validate_regions") [request] ([blank request 0x2ac 0,word request 1,word (request+0x280) 8,wordsAt (request+4) [12,18,5,9],word (request+0x34) 7] ++ scanConfigurationInitial configuration [(0xa754,5)]) [word (request+0x34) 0] (Just 0)] ++
      [scenario "validate-raw-aligned" (entry 0xd6cc70 "validate_regions") [request] ([blank request 0x2ac 0,word request 1,word (request+0x280) 8,wordsAt (request+4) [12,18,100,120],word (request+0x25c) 200,word (request+0x260) 200,Patch (Address (request+0x288)) (B.pack [5,4])] ++ scanConfigurationInitial configuration [(0xa754,5),(0xa755,9),(0xa57a,2)]) [wordsAt (request+0x14) [13,20,96,108],word (request+0x34) 1] (Just 1)] ++
      [scenario "validate-raw-expanded" (entry 0xd6cc70 "validate_regions") [request] ([blank request 0x2ac 0,word request 1,word (request+0x280) 8,wordsAt (request+4) [12,18,5,9],word (request+0x25c) 200,word (request+0x260) 200] ++ scanConfigurationInitial configuration [(0xa754,0),(0xa755,9),(0xa57a,1)]) [wordsAt (request+0x14) [12,18,6,12],word (request+0x34) 1] (Just 1)] ++
      [scenario "validate-raw-expansion-outside" (entry 0xd6cc70 "validate_regions") [request] ([blank request 0x2ac 0,word request 1,word (request+0x280) 8,wordsAt (request+4) [1,18,5,9],word (request+0x25c) 200,word (request+0x260) 200] ++ scanConfigurationInitial configuration [(0xa754,0),(0xa755,9)]) [word (request+0x34) 0] (Just 0)] ++
      [scenario "process-empty" (entry 0xd6ca88 "process_components") [request,workspace] [blank request 0x2ac 0,blank workspace 0xcb8 0xcc] [blank workspace 0xcb8 0xcc] (Just 1)]
    diagnosticCases = [scenario "diagnostics-signed-and-histograms" (entry 0xd753b8 "export_diagnostics") [request,workspace,output] [blank request 0x2ac 0,halves (request+0x20c) [1,2,3,4,5,6],Patch (Address workspace) diagnosticWorkspace,blank output 0xd24 0xcc] [Patch (Address output) expectedDiagnostics,blank (output+0xd1c) 8 0xcc] Nothing]
    lookupCases = [scenario ("word-lookup-" ++ show mask) (entry 0x69084c "word_lookup") [if mask .&. 1 == 0 then 0 else output,if mask .&. 2 == 0 then 0 else output+4,if mask .&. 4 == 0 then 0 else output+8,3] [wordsAt (0x821bac+3*4104) [0x12345678,0xabcdef01],blank output 12 0xcc] ([word output 0x12345678 | mask .&. 1 /= 0] ++ [word (output+4) 0xabcdef01 | mask .&. 2 /= 0] ++ [word (output+8) (0x821bac+3*4104+8) | mask .&. 4 /= 0]) Nothing | mask <- [0..7 :: Word32]]
    tableCases = [scenario ("select-table-" ++ show mode) (entry 0xd79cf4 "select_word_table") [mode,output] (scanConfigurationInitial configuration [(key,3)] ++ [wordsAt (0x821bac+3*4104) [123,456],blank output 24 0xcc]) [blank output 16 0xcc,word (output+16) (0x821bac+3*4104+8),blank (output+20) 4 0xcc] Nothing | (mode,key) <- [(0,0x8060),(1,0x8061),(2,0x8062),(3,0x8063),(4,0x8064),(5,0x8060),(maxBound,0x8060)]]
    enabledCases = [scenario ("enabled-value-inactive-" ++ show kind) (entry 0xd7a26c "enabled_value") [0x87654321,kind] [] [] (Just 0) | kind <- [0,2,maxBound]] ++
      [scenario ("enabled-value-config-" ++ show value) (entry 0xd7a26c "enabled_value") [0x87654321,1] (scanConfigurationInitial configuration [(0x8300,value)]) [] (Just (if value == 0 then 0 else 0x87654321)) | value <- [0,1,128,255]]
    preparationCases = [scenario "prepare-descriptors-histogram-ratios" (entry 0xd7a2a0 "prepare_components") [context,request] preparationInitial preparationFinal Nothing] ++
      [scenario "read-active-empty-region-byte" (entry 0xd7a820 "components") [context,output] (preparationInitial ++ [word context 0x100,blank 0x14694b0 0x2ac 0xcc,blank 0x14687f8 0xcb8 0xcc,blank output 8 0xcc]) [halves output [89,97,101,0xcccc],Patch (Address 0x14694b0) preparedRequest,blank 0x14687f8 0xcb8 0] Nothing] ++
      [scenario "prepare-nonempty-region" (entry 0xd7a2a0 "prepare_components") [context,request] (preparationInitial ++ [wordsAt (context+4) [2,9,3,15,2,11],halves (context+28) [17,19],word (context+44) 1]) (Patch (Address request) nonemptyPreparedRequest : drop 1 preparationFinal) Nothing]

    preparationInitial = scanComponentContextInitial context plane profile debug scratch 0 ++
      [wordsAt plane [0,0x40002000,4096,96,80,40,0x3010],word (debug+0xa30a4) 109,blank request 0x2ac 0xcc,blank (debug+0x9e018) 768 0,word (debug+0x9e018) 8,word (debug+0x9e018+256) 5,word (debug+0x9e018+512) 11,blank (scratch+0x4720) 256 0xcc] ++ scanConfigurationInitial configuration [(0x8063,2)] ++ [wordsAt (0x821bac+2*4104) [1,2]]
    preparationFinal = [Patch (Address request) preparedRequest,halves (scratch+0x4720) [62],halves (scratch+0x47a0) [137],blank (scratch+0x4722) 126 0,blank (scratch+0x47a2) 126 0]

emptyDiagnostics :: B.ByteString
emptyDiagnostics = encode 0xd1c ([(0,wordBytes 0x53544644),(0xd18,wordBytes 0x4c414644)])

encode :: Int -> [(Int,B.ByteString)] -> B.ByteString
encode length fields = B.pack [value index | index <- [0..length-1]]
  where value index = foldl (\prior (offset,bytes) -> if index >= offset && index < offset+B.length bytes then B.index bytes (index-offset) else prior) 0 fields

preparedRequest :: B.ByteString
preparedRequest = encode 0x2ac (wordFields ++ halfFields ++ byteFields)
  where
    wordFields = [(offset,wordBytes value) | (offset,value) <- [(0x220,0),(0x224,profile),(0x228,profile+0x400),(0x230,17),(0x234,19),(0x238,41),(0x23c,43),(0x240,20),(0x244,30),(0x24c,0),(0x250,0),(0x254,0x821bac+2*4104+8),(0x258,0x40002000),(0x25c,64),(0x260,40),(0x268,103),(0x26c,107),(0x278,1),(0x27c,2),(0x280,8),(0x284,0),(0x28c,debug),(0x290,53),(0x294,59),(0x298,scratch),(0x29c,1),(0x2a0,4),(0x2a4,21)]] ++ [(4+index*52+offset,wordBytes value) | index <- [0..9], (offset,value) <- [(8,1),(12,1)]]
    halfFields = [(offset,halfBytes value) | (offset,value) <- [(0x20c,73),(0x20e,79),(0x210,83),(0x212,61),(0x214,67),(0x216,71),(0x218,89),(0x21a,97),(0x21c,101),(0x22c,109),(0x22e,47),(0x248,16),(0x272,11)]]
    byteFields = [(0x21e,B.singleton 10),(0x264,B.singleton 14),(0x288,B.singleton 5),(0x289,B.singleton 4)]

diagnosticWorkspace :: B.ByteString
diagnosticWorkspace = B.pack [fromIntegral ((index*37+11) `mod` 256) | index <- [0..0xcb7 :: Int]]

expectedDiagnostics :: B.ByteString
expectedDiagnostics = encode 0xd1c ([(0,wordBytes 0x53544644),(0xd18,wordBytes 0x4c414644)] ++ histogram ++ fields)
  where
    readUnsigned offset count = sum [fromIntegral (B.index diagnosticWorkspace (offset+index)) * 256^index | index <- [0..count-1]] :: Word32
    readSigned offset = fromIntegral (fromIntegral (readUnsigned offset 2) :: Int16)
    histogram = [(destination+index*4,B.take 4 (B.drop (source+index*4) diagnosticWorkspace)) | (destination,source) <- [(0x118,0x414),(0x518,0x14),(0x918,0x814)], index <- [0..255]]
    fields = [(slot*4,wordBytes value) | (slot,value) <- unsignedFields ++ signedFields ++ requestFields]
    unsignedFields = [(slot,readUnsigned offset size) | (slot,offset,size) <- [(1,0xc2a,2),(2,0xc34,1),(3,0xc35,1),(4,0xc38,4),(5,0xc3c,4),(6,0xc48,4),(7,0xc4c,4),(8,0xc40,4),(9,0xc44,4),(10,0xc50,2),(11,0xc52,2),(12,0xc54,2),(13,0xc56,2),(14,0xc58,2),(15,0xc5a,2),(16,0xc5c,2),(17,0xc5e,2),(18,0xc14,2),(19,0xc16,2),(20,0xc60,4),(21,0xc68,2),(22,0xc6a,2),(23,0xc6c,4),(24,0xc70,4),(25,0xc7c,4),(26,0xc78,4),(27,0xc80,2),(28,0xc82,2),(29,0xca2,2),(30,0xca4,2),(31,0xc84,2),(32,0xc86,2),(33,0xc88,2),(36,0xca6,2),(38,0xca8,2),(39,0xcaa,2),(43,12,2),(44,14,2),(45,16,2),(46,6,2),(47,8,2),(48,10,2),(52,0xc90,4),(58,0xca0,2),(59,0,2),(60,2,2),(61,4,2),(62,0xcb0,4),(63,0xc18,2),(64,0xc1a,2),(65,0xc1c,4),(66,0xc20,2),(67,0xc22,2),(68,0xc24,2),(69,0xc26,2)]]
    signedFields = [(slot,readSigned offset) | (slot,offset) <- [(34,0xc8a),(35,0xc8c),(37,0xc96),(53,0xc94),(54,0xc98),(55,0xc9a),(56,0xc9c),(57,0xc9e)]]
    requestFields = [(40,1),(41,2),(42,3),(49,4),(50,5),(51,6)]

nonemptyPreparedRequest :: B.ByteString
nonemptyPreparedRequest = foldl replace preparedRequest
  [(4,B.concat (map wordBytes [2,3,8,13])),(0x24,B.concat (map halfBytes [330,65356])),(0x28,wordBytes 17),(0x2c,halfBytes 19),(0x30,wordBytes 1)]
  where replace bytes (offset,value) = B.take offset bytes <> value <> B.drop (offset+B.length value) bytes
