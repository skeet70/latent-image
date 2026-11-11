{-# LANGUAGE GHC2021 #-}
module OutputMetadataScenarios (outputMetadataPublications, outputMetadataScenarios, metadataRateInitial, metadataSensitivityInitial, metadataSensitivitySamples) where

import qualified Data.ByteString as B
import Data.Word (Word8, Word32)
import PipelineModel
import Scenarios
import qualified BuffersScenarios as Buffers
import qualified SensorSettingsScenarios as SensorSettings
import qualified SensorRequestsScenarios as SensorRequests

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

metadataRateInitial :: Word8 -> Word8 -> [(Word32,Word8)] -> [Patch]
metadataRateInitial bankSetting override values = Buffers.configurationInitial 0x1700000 ([(0x1c1d,bankSetting),(0x8e40,override)]++values)

metadataSensitivityInitial :: [Patch]
metadataSensitivityInitial = Buffers.configurationInitial 0x1700000
  [(0x1904,100),(0x1905,0),(0x1906,200),(0x1907,0),(0x1908,44),(0x1909,1),(0x190a,144),(0x190b,1),(0x190c,244),(0x190d,1)
  ,(0x190e,88),(0x190f,2),(0x1910,188),(0x1911,2),(0x1912,32),(0x1913,3),(0x1914,132),(0x1915,3)]

metadataSensitivitySamples :: [(Word32,Word32)]
metadataSensitivitySamples = [(0,7),(99,7),(100,8),(199,8),(200,10),(232,10),(233,11),(266,11),(267,12),(299,12),(300,13),(699,24),(700,25),(799,25),(800,27),(899,27),(900,28),(0xffffffff,28)]

outputMetadataPublications :: [StagePublication]
outputMetadataPublications = [publication address suffix | (address,suffix) <-
  [(0x11db098,"write_exif_header"),(0x11c14f0,"initialize_exif_writer"),(0x11c1574,"write_soi"),(0x11c15b8,"write_tiff_header"),(0x11c1670,"write_app1_header")
  ,(0x11c170c,"write_primary_ifd"),(0x11c17cc,"write_exif_ifd"),(0x11c18ac,"write_interoperability_ifd"),(0x11c19c8,"write_makernote"),(0x11c1b30,"write_gps_ifd"),(0x11c1c14,"write_thumbnail_ifd")
  ,(0x11cf6e0,"write_maker_signature"),(0x11cf748,"write_printim"),(0x11cf854,"select_ifd_table"),(0x11cf908,"ifd_table_count"),(0x11cf0dc,"tiff_value_capacity")
  ,(0x11cda48,"saved_tag_available"),(0x11cdea0,"should_write_tag"),(0x11c2154,"transfer_saved_parser"),(0x11c35f8,"saved_directory_slots"),(0x11c3678,"saved_directory_values")
  ,(0x11ceea0,"get_tiff_value_layout"),(0x11cf990,"write_filtered_directory"),(0x11cfe54,"write_saved_directory")
  ,(0x11c2208,"find_directory_tag"),(0x11c2490,"find_primary_directory_tag"),(0x11c24b8,"find_saved_tag"),(0x11c25d8,"find_primary_saved_tag"),(0xebfd78,"populate_special_metadata")
  ,(0x734824,"transform_metadata_rectangle"),(0x734908,"transform_metadata_faces")
  ,(0xeaa888,"metadata_primary_mode"),(0xeaa964,"metadata_secondary_mode"),(0xeaaa60,"metadata_color_mode"),(0xeaac74,"metadata_processing_mode")
  ,(0xeaae08,"metadata_kelvin"),(0xeaafb4,"metadata_tone_scale"),(0xeab004,"metadata_saturation"),(0xeab060,"metadata_balance_adjustment")
  ,(0xeab170,"metadata_flash_processing"),(0xeab258,"metadata_processing_profile")
  ,(0xea97c8,"store_maker_exposure_bias"),(0xea985c,"store_maker_flash_mode")
  ,(0xea9d9c,"pack_metadata_rectangle"),(0xea9dd0,"pack_paired_metadata_rectangle"),(0xea9e04,"pack_metadata_direction")
  ,(0xeab758,"populate_maker_metadata"),(0xeaec80,"populate_output_metadata")
  ,(0xef3434,"rate_grid_weights"),(0x74dbe4,"profile_alternate_bank"),(0x74db20,"profile_value_index")
  ,(0x74dc10,"export_profile_value"),(0xef4fc8,"profile_adjusted_limits"),(0x74dc78,"export_profile_mode")
  ,(0x722abc,"build_metadata_spline"),(0x723234,"evaluate_metadata_spline")
  ,(0x7339c4,"metadata_exposure_value"),(0x733d8c,"metadata_sensitivity_iso"),(0x733ee0,"metadata_sensitivity_delta")
  ,(0x735b78,"metadata_sensitivity_parameter"),(0x736fac,"metadata_sensitivity_base"),(0x733f30,"metadata_current_sensitivity")
  ,(0x733dd0,"metadata_sensitivity_threshold"),(0x733f04,"metadata_apply_sensitivity_delta"),(0x7345b4,"metadata_sensor_width"),(0x734710,"metadata_sensor_height")
  ,(0xebde7c,"populate_raf_metadata"),(0xeb02f8,"select_metadata_balance"),(0x73fce0,"metadata_threshold_band")
  ,(0xe57b5c,"metadata_sensor_band"),(0xe57cb4,"metadata_level_band"),(0xe57ba4,"metadata_distortion_enabled")
  ,(0x73ebcc,"metadata_snapshot_band"),(0x73fdb8,"metadata_cached_snapshot_value"),(0x73fe24,"metadata_cached_snapshot_band"),(0x73fe8c,"metadata_snapshot_projection")
  ,(0xd5e5a4,"export_metadata_snapshot"),(0xe560a0,"prepare_metadata_preview"),(0xe57fc8,"populate_sensor_snapshot")]]

outputMetadataScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
outputMetadataScenarios bundle _ = writerCases ++ markerCases ++ directoryCases ++ queryCases ++ specialCases ++ policyCases ++ scalarCases ++ packingCases ++ makerCases ++ outputCases ++ profileCases ++ splineCases ++ thresholdCases ++ snapshotCases ++ balanceCases ++ previewCases ++ sensorSnapshotCases ++ rafCases
 where
  image = B.drop 2631680 bundle
  writer = 0x14e9d50
  settings = 0x14e9e3c
  source = 0x9b0000
  destination = 0x9d0000
  result = 0x9f0000
  stateBase = 0x1600000
  dataBase = 0x1650000
  parser = 0x9a0000
  make :: String -> Word32 -> String -> [Word32] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario
  make name address suffix arguments initial final returned = StageScenario (StageCaseName name) (publication address suffix) arguments initial final returned [] []
  bytes :: Word32 -> [Word8] -> Patch
  bytes address values = Patch (Address address) (B.pack values)
  zero :: Word32 -> Int -> Patch
  zero address count = Patch (Address address) (B.replicate count 0)
  poison :: Word32 -> Int -> Patch
  poison address count = Patch (Address address) (B.replicate count 0xa5)
  half :: Word32 -> Word32 -> Patch
  half address value = bytes address [fromIntegral value,fromIntegral (value `div` 256)]
  writerCases =
    [make "metadata-exif-header-composes-real-primary-exif-maker-interoperability-thumbnail" 0x11db098 "write_exif_header"
      [result,source,source+0x1000,destination,source+0x2000,source+0x3000,0,0]
      [zero source 0x3400,zero destination 0xe8fc,zero 0x14e9d98 0x100,zero 0x14e9ea0 0x60,zero writer 72,poison result 0x10000]
      [bytes result [255,216,255,225,0,0,69,120,105,102,0,0,73,73,42,0,8,0,0,0]
       ,half (result+20) 12,bytes (result+22) [15,1,2,0,1,0,0,0,0,0,0,0]
       ,word (result+150) 300,word (result+166) 4390,half (result+312) 42,word (result+546) 3300,word (result+550) 1060,word (result+610) 4360
       ,bytes (result+1072) [70,85,74,73,70,73,76,77,12,0,0,0],half (result+1084) 80
       ,bytes (result+4372) [2,0,1,0,2,0,4,0,0,0,0,0,0,0,2,0,7,0,4,0,0,0,48,49,48,48,0,0,0,0]
       ,half (result+4402) 8,word (result+4472) 4508,word (result+4484) 0
       ,word writer (result+12),half (writer+68) 1,half (writer+70) 0,half settings 0,half (settings+2) 0,half (settings+4) 0] (Just 4520)] ++
    [make ("metadata-exif-header-GPS-direction-transition-"++show direction) 0x11db098 "write_exif_header"
      [result,source,source+0x1000,destination,source+0x2000,source+0x3000,0,1]
      [zero source 0x3400,word (source+0x2080) direction,zero destination 0xe8fc,zero 0x14e9d98 0x100,zero 0x14e9ea0 0x60,zero writer 72,poison result 0x10000]
      [half (result+20) 13,word (result+150) 312,word (result+162) 4402,word (result+178) thumbnailOffset
       ,half (result+324) 42,word (result+558) 3300,word (result+562) 1072,word (result+622) 4372
       ,half (result+4414) gpsCount,bytes (result+4416) [0,0,1,0,4,0,0,0,0,0,0,0]
       ,half (result+thumbnailOffset+12) 8,word writer (result+12),half (writer+68) 1,half (writer+70) 1,half (settings+12) 1] (Just total)
    | (direction,gpsCount,thumbnailOffset,total)<-[(0,13,4672,4802),(1,12,4652,4782)]] ++
    [make "metadata-writer-reset-preserves-signature-and-length-addresses" 0x11c14f0 "initialize_exif_writer" [3,1,2,0x940000,0x10001]
      [poison settings 16,poison writer 72]
      ([half settings 3,half (settings+2) 1,half (settings+4) 2,word (settings+8) 0x940000,half (settings+12) 1]
       ++ [word (writer+offset) 0 | offset <- [0,4,8,12,16,24,32,36,40,44,48,52,56,60,64]]
       ++ [word (writer+20) 0xa5a5a5a5,word (writer+28) 0xa5a5a5a5,half (writer+68) 0,half (writer+70) 0]) Nothing] ++
    [make ("metadata-writer-rejects-mode-"++suffix) address suffix [source,destination]
      [half settings mode,poison writer 72,poison destination 64] [poison destination 64,poison writer 72] (Just 0)
    | (address,suffix,mode) <- [(0x11c170c,"write_primary_ifd",4),(0x11c17cc,"write_exif_ifd",4),(0x11c1c14,"write_thumbnail_ifd",3)]] ++
    [make ("metadata-optional-directory-disabled-"++suffix) address suffix [source,destination]
      [zero writer 72,half settings 0,poison destination 64] [poison destination 64] (Just 0)
    | (address,suffix) <- [(0x11c19c8,"write_makernote"),(0x11c1b30,"write_gps_ifd")]]
  markerCases =
    [make "metadata-soi-marker" 0x11c1574 "write_soi" [destination] [poison destination 4] [bytes destination [255,216,165,165]] (Just 2)
    ,make "metadata-app1-marker-and-patch-slot" 0x11c1670 "write_app1_header" [destination] [poison destination 12]
      [bytes destination [255,225,0,0,69,120,105,102,0,0,165,165],word (writer+44) (destination+2)] (Just 10)
    ,make "metadata-maker-signature" 0x11cf6e0 "write_maker_signature" [destination] [poison destination 14]
      [bytes destination [70,85,74,73,70,73,76,77,12,0,0,0,165,165],word (writer+28) destination] (Just 12)] ++
    [make ("metadata-tiff-order-"++show order) 0x11c15b8 "write_tiff_header" [destination]
      [half (settings+2) order,poison destination 10]
      [bytes destination (marker++[42,0,8,0,0,0,165,165]),word writer destination] (Just 8)
    | (order,marker) <- [(0,[73,73]),(1,[77,77]),(7,[77,77])]] ++
    [make ("metadata-interoperability-"++show colour) 0x11c18ac "write_interoperability_ifd" [source,destination]
      [word writer (destination-128),word (writer+8) result,half (source+0x28a) colour,bytes 0x14e9e08 [82,57,56,0],poison destination 32]
      [word result 128,bytes destination ([2,0,1,0,2,0,4,0,0,0]++index++[2,0,7,0,4,0,0,0,48,49,48,48,0,0,0,0,165,165])]
      (Just 30)
    | (colour,index) <- [(0xffff,[82,48,51,0]),(1,[82,57,56,0])]]
  recordBytes = [5,0,0,1,4,0,1,0,0,0,68,51,34,17,0,16,3,0,1,0,0,0,3,0,0,0,1,16,7,0,3,0,0,0,97,98,99,0,32,16,4,0,1,0,0,0,136,119,102,85,33,16,7,0,8,0,0,0,66,0,0,0,0,0,0,0,1,2,3,4,5,6,7,8]
  recordSource = [zero source 832,word source 1,word (source+4) 0x11223344,half (source+8) 3,bytes (source+10) [97,98,99],word (source+44) 0x55667788,bytes (source+48) [1,2,3,4,5,6,7,8]]
  directoryCases =
    [make "metadata-filtered-image-record-empty" 0x11cf990 "write_filtered_directory" [3,source,destination,destination]
      [zero source 832,half settings 0,half (settings+12) 0,poison destination 8]
      [bytes destination [0,0,0,0,0,0,165,165]] (Just 6)
    ,make "metadata-filtered-image-record-inline-and-indirect" 0x11cf990 "write_filtered_directory" [3,source,destination,destination]
      (recordSource++[half settings 0,half (settings+12) 0,poison destination 76])
      [bytes destination (recordBytes++[165,165])] (Just 74)
    ,make "metadata-filtered-invalid-directory" 0x11cf990 "write_filtered_directory" [255,source,destination,destination]
      [poison destination 32] [poison destination 32] (Just 0)] ++
    [make ("metadata-value-layout-"++show kind++"-"++show count) 0x11ceea0 "get_tiff_value_layout" [source,result]
      [half (source+2) kind,word (source+4) count,poison result 4]
      [half result width,half (result+2) indirect] Nothing
    | (kind,count,width,indirect) <- [(1,4,1,0),(7,5,1,1),(3,2,2,0),(3,3,2,1),(4,1,4,0),(9,2,4,1),(5,0,8,1),(10,1,8,1),(6,99,0,0)]]
  queryInitial = [word 0x14e9e50 stateBase,word 0x14e9e58 0x1640000,wordsAt dataBase [8,74],bytes (dataBase+8) recordBytes,word (dataBase+66) 74,poison result 12]
  queryCases =
    [make ("metadata-query-record-tag-"++show tag) 0x11c2490 "find_primary_directory_tag" [dataBase,82,0,tag,result]
      queryInitial [word result count,word (result+4) address,half (result+8) kind,bytes (result+10) [165,165]] (Just 0)
    | (tag,count,address,kind) <- [(0x100,1,dataBase+18,4),(0x1000,1,dataBase+30,3),(0x1001,3,dataBase+42,7),(0x1020,1,dataBase+54,4),(0x1021,8,dataBase+74,7)]] ++
    [make ("metadata-query-rejects-"++show (offset,byteLength,size,tag)) 0x11c2490 "find_primary_directory_tag" [dataBase,size,0,tag,result]
      (queryInitial++[wordsAt dataBase [offset,byteLength]]) [word result 0,word (result+4) 0,half (result+8) 0] (Just 0xffffffff)
    | (offset,byteLength,size,tag) <- [(0,74,82,0x100),(8,0,82,0x100),(8,74,81,0x100),(8,65,82,0x100),(8,74,82,0x9999)]] ++
    [make "metadata-saved-parser-read-primary" 0x11c2154 "transfer_saved_parser" [parser,0,0]
      [poison parser 28,word 0x14e9e50 stateBase,word 0x14e9e58 dataBase]
      [wordsAt parser [0,dataBase,0,0,0,stateBase,0]] (Just 0)
    ,make "metadata-saved-parser-publish-secondary" 0x11c2154 "transfer_saved_parser" [parser,1,1]
      [wordsAt parser [1,dataBase,2,3,4,stateBase,5]] [word 0x14e9e4c dataBase,word 0x14e9e54 stateBase] (Just 0)
    ,make "metadata-saved-parser-rejects-bank" 0x11c2154 "transfer_saved_parser" [parser,2,0]
      [poison parser 28] [poison parser 28] (Just 0xffffffff)]
  savedInitial present = [word 0x14e9e50 stateBase,word 0x14e9e58 dataBase,zero (stateBase+0x250c) (155*12)
    ,wordsAt (stateBase+0x250c+75*12) [2,0,5],bytes (stateBase+0x2c44) [11,22,33,44,55,66,77,88]
    ,wordsAt (stateBase+0x250c+14*12) [if present then 1 else 0,8,3],half (stateBase+0x2c4c) 321]
  specialCases =
    [make ("metadata-special-no-records-"++show (enabled,present)) 0xebfd78 "populate_special_metadata" [source,destination]
      (savedInitial present++[word (source+0x19b0) dataBase,word (dataBase+0x680) enabled,half (dataBase+0x650) 0,poison (destination+0x1d38) 4])
      [word (destination+0x1d38) (if enabled==0 then 0 else 1),bytes (destination+0xe898) [11,22,33,44,55,66,77,88],half (destination+0x6e) (if present then 321 else 0xffff)] Nothing
    | enabled <- [0,1],present <- [False,True]] ++
    [make "metadata-special-record-selection-clears-only-presence" 0xebfd78 "populate_special_metadata" [source,destination]
      (savedInitial True++[word (source+0x19b0) dataBase,word (dataBase+0x680) 1,half (dataBase+0x650) 2,word (dataBase+0x654) result,half result 7,half (result+2) 12,poison (destination+0x1d3c) 1664])
      [word (destination+0x1d38) 1,word (destination+0x1d3c) 0,word (destination+0x207c) 0,half (destination+1420) 7,half (destination+1422) 12,bytes (destination+0xe898) [11,22,33,44,55,66,77,88],half (destination+0x6e) 321] Nothing] ++
    [make ("metadata-special-negative-selection-skips-"++show invalidDescriptor) 0xebfd78 "populate_special_metadata" [source,destination]
      (savedInitial True ++
       [word (source+0x19b0) dataBase,word (dataBase+0x680) 1,half (dataBase+0x650) 1,word (dataBase+0x654) result,half result 0xffff
       ,wordsAt (stateBase+0x250c+117*12) [164,16,7]
       ,wordsAt (stateBase+0x2c54) [if invalidDescriptor then 0 else 16,74,90,74]
       ,bytes (stateBase+0x2c64) recordBytes,word (stateBase+0x2c6e) 0xffffffff,word (stateBase+0x2c9e) 82
       ,bytes (stateBase+0x2cae) recordBytes,word (stateBase+0x2ce8) 156,poison (destination+0x1d3c) 832])
      [word (destination+0x1d38) 1,word (destination+0x1d3c) 1,word (destination+0x1d40) 0x11223344,half (destination+0x1d44) 3
       ,bytes (destination+0x1d46) [97,98,99,165],word (destination+0x1d68) 0x55667788,bytes (destination+0x1d6c) [1,2,3,4,5,6,7,8]
       ,half (destination+1420) 0xffff,bytes (destination+0xe898) [11,22,33,44,55,66,77,88],half (destination+0x6e) 321] Nothing
    | invalidDescriptor <- [False,True]]
  policyCases =
    [make ("metadata-public-policy-"++show (kind,tag,value)) 0x11cdea0 "should_write_tag" [kind,tag,source]
      initial [] (Just value)
    | (kind,tag,initial,value) <-
      [(0,0x13e,[zero source 0x200],0),(0,0x13e,[word (source+0xf4) 313],1)
      ,(1,0xa40a,[half (source+0x2b4) 0xffff],0),(1,0xa40a,[half (source+0x2b4) 0],1)
      ,(3,0x100,[zero source 64],0),(3,0x100,[word source 1,zero (source+56) 8],1)
      ,(3,0x1110,[word source 1,word (source+56) 1,half (source+60) 0xffff],0)
      ,(3,0x1110,[word source 1,word (source+56) 1,half (source+60) 1],1)
      ,(5,0xb,[wordsAt (source+104) [0,0]],0),(5,0xb,[wordsAt (source+104) [0,1]],1)
      ,(8,0xb000,[word (source+28) 0],0),(8,0xb000,[word (source+28) 1],1)]]
  scalarCases =
    [make ("metadata-primary-mode-"++show (mode,selection)) 0xeaa888 "metadata_primary_mode" [source]
      [word (source+0x4b4) mode,word (source+0x500) selection,word (source+0x508) 0] [] (Just expected)
    | (mode,selection,expected) <- [(2,0,2),(3,1,0x82),(4,3,0x84),(31,4,4),(33,9,3),(36,3,0x84),(23,0,4),(0,0,3),(1,3,3),(37,4,3)]] ++
    [make ("metadata-secondary-mode-"++show (mode,selection)) 0xeaa964 "metadata_secondary_mode" [source]
      [word (source+0x4b4) mode,word (source+0x4f4) selection] [] (Just expected)
    | (mode,selection,expected) <- [(0,1,0),(1,2,0),(2,1,0x100),(29,10,0xff0),(31,8,0xf00),(36,9,0xf01),(44,4,0x301),(45,6,0)]] ++
    [make ("metadata-kelvin-"++show (selection,expected)) 0xeaae08 "metadata_kelvin" [source]
      [word (source+0x4f4) 10,word (source+0x5fc) selection] [] (Just expected)
    | (selection,expected) <- [(0,2500),(11,3400),(16,4200),(24,6300),(30,10000),(31,0),(0xffffffff,0)]] ++
    [make ("metadata-saturation-"++show selection) 0xeab004 "metadata_saturation" [source]
      [word (source+0x524) selection] [] (Just expected)
    | (selection,expected) <- [(0,0x200),(1,0x280),(2,0),(3,0x180),(4,0x100),(5,0)]] ++
    [make ("metadata-balance-"++show (axis,input)) 0xeab060 "metadata_balance_adjustment" [source,axis]
      [word (source+0x4f8+axis*4) input] [] (Just expected)
    | (axis,input,expected) <- [(0,0,0),(0,9,180),(1,0xfffffff7,0xffffff4c),(1,10,0),(0,0xfffffff6,0)]] ++
    [make ("metadata-tone-scale-"++show mode) 0xeaafb4 "metadata_tone_scale" [source]
      [word (source+0x4b4) mode] [] (Just expected)
    | (mode,expected) <- [(0,0x80),(1,0x80),(2,0x100),(4,0x100),(29,0x80),(31,0x100),(33,0x100),(36,0x100),(37,0x80)]]
  packingCases =
    [make ("metadata-rectangle-packing-"++suffix) address suffix [destination,source]
      [wordsAt source [0x10001,0x20002,0x30003,0x40004],poison destination 12]
      [bytes destination [1,0,3,0,2,0,4,0,165,165,165,165]] Nothing
    | (address,suffix) <- [(0xea9d9c,"pack_metadata_rectangle"),(0xea9dd0,"pack_paired_metadata_rectangle")]] ++
    [make ("metadata-direction-"++show (direction,selection,byteOut,halfOut)) 0xea9e04 "pack_metadata_direction" [byteOut,halfOut,source]
      [word (source+0x10) direction,word (source+0x14) selection,poison destination 8]
      ([if byteOut==0 then poison destination 1 else bytes destination [fromIntegral expected]]
        ++ [if halfOut==0 then poison (destination+2) 2 else half (destination+2) expected]) Nothing
    | (direction,selection,expected) <- [(0,0,0),(1,1,0x11),(2,11,0x2b),(3,12,0),(0xffffffff,0xffffffff,0)]
    ,(byteOut,halfOut) <- [(destination,destination+2),(0,destination+2),(destination,0)]] ++
    [make ("metadata-exposure-bias-"++show (mode,input)) 0xea97c8 "store_maker_exposure_bias" [source,destination]
      [word (source+0x4b4) mode,word (source+0x4dc) input,poison destination 112]
      [word (destination+0x64) expected,word (destination+0x68) 100] Nothing
    | (mode,input,expected) <- [(2,0,0),(2,1,33),(2,2,67),(2,0xffffffff,0xffffffdf),(44,3,0)]]
  configurationBase = 0x1700000
  makerCases =
    [make "metadata-maker-panorama-empty-records-real-helpers" 0xeab758 "populate_maker_metadata" [source,destination,0,0,0]
      ([zero source 0x3300,poison destination 0xe8fc,word (source+0x4b4) 29,word (source+0x1770) 640,word (source+0x176c) 480]
       ++ Buffers.configurationInitial configurationBase [(0x254,0),(0x22e,0)])
      ([zero (destination+4) 48,bytes (destination+0x34) [78,79,82,77,65,76,0]
       ,half (destination+0x3c) 3,half (destination+0x3e) 0,half (destination+0x40) 0,half (destination+0x42) 0xffff
       ,word (destination+0x48) 0,half (destination+0x54) 0x80,word (destination+0x5c) 0,word (destination+0x64) 0,word (destination+0x68) 100
       ,half (destination+0x74) 0xffff,half (destination+0x76) 0xffff,word (destination+0xc4) 0x20,half (destination+0xc8) 0,half (destination+0xca) 0
       ,half (destination+0xcc) 120,half (destination+0xce) 3,word (destination+0x10c) 0,half (destination+0x110) 0,half (destination+0x112) 0xffff
       ,bytes (destination+0x1e8) [48,51,48,48],bytes (destination+0x1ec) [48,49,48,48],half (destination+0x58a) 0
       ,half (destination+0xe03c) 0,half (destination+0xe03e) 0,half (destination+0xe040) 0,half (destination+0xe042) 0
       ,word (destination+0x1d38) 0,zero (destination+0xa78) 0xf00,zero (destination+0x2ec) 0x280
       ,bytes (destination+0xe050) [70,70,70,70],word (destination+0xe8e8) 1,word (destination+0xe8f8) 0x80000000]) Nothing] ++
    [make ("metadata-maker-face-records-"++show paired) 0xeab758 "populate_maker_metadata" [source,destination,paired,0,0]
      ([zero source 0x3300,poison destination 0xe8fc,word (source+0x4b4) 29,word (source+0x1770) 640,word (source+0x176c) 480
       ,word (source+0x4f0) 1,word (source+0x27ac) 1,word (source+0x600) 1,word (source+0x74c) 1
       ,wordsAt (source+0x758) [72,144,48,96,0,0,0,321]
       ,wordsAt (source+0x784) [64,128,48,96,1,1,1,500]
       ,wordsAt (source+0x7a4) [128,256,96,192,0,1,0,600]]
       ++ Buffers.configurationInitial configurationBase [(0x254,0),(0x22e,0)])
      ([half (destination+0x58a) (if paired==0 then 1 else 3),half (destination+0x58c) 1,bytes (destination+0x604) [4]
       ,bytes (destination+0x640) [64,0,48,0,128,0,96,0],bytes (destination+0x238) [64,0,48,0,128,0,96,0]
       ,bytes (destination+0x820) [0x99,0x19,0x99,0x19,0x33,0x33,0x33,0x33],bytes (destination+0x288) [0x99,0x19,0x99,0x19,0x33,0x33,0x33,0x33]
       ,half (destination+0xe040) (if paired==0 then 4 else 12),half (destination+0xe03c) 4,half (destination+0x1a68) 321,half (destination+0x56c) 321
       ,word (destination+0x1978) 0,word (destination+0x1b58) 0,word (destination+0x1c48) 0]
       ++ if paired==0 then [] else
       [half (destination+0x58e) 2,half (destination+0x590) 3,bytes (destination+0x605) [4,4]
       ,bytes (destination+0x648) [64,0,48,0,128,0,96,0,128,0,96,0,0,1,192,0]
       ,bytes (destination+0x828) [0x99,0x19,0x99,0x19,0x33,0x33,0x33,0x33,0x33,0x33,0x33,0x33,0x66,0x66,0x66,0x66]
       ,half (destination+0x1a6a) 500,half (destination+0x1a6c) 600,half (destination+0x1ae2) 7,half (destination+0x1ae4) 2
       ,word (destination+0x207c) 0,word (destination+0x23bc) 0]) Nothing
    | paired <- [0,1]]
  outputCases =
    [make "metadata-output-panorama-real-EXIF-maker-location-population" 0xeaec80 "populate_output_metadata" [source,0x9a0000,0x9a1000,0x9a2000,destination,0x9c0000,0]
      ([zero source 0x3300,poison 0x9a0000 0x400,poison 0x9a1000 0x100,poison 0x9a2000 0x400,poison destination 0xe8fc,poison 0x9c0000 0x200
       ,word (source+0x4b4) 29,word (source+0x1770) 640,word (source+0x176c) 480,word (source+0x1604) 4,word (source+0x5d0) 1
       ,bytes (source+0x1684) [50,48,50,52,58,48,49,58,48,50,32,48,51,58,48,52,58,48,53,0]]
       ++ Buffers.configurationInitial configurationBase [(0x254,0),(0x22e,0)])
      [bytes 0x9a009e [50,48,50,52,58,48,49,58,48,50,32,48,51,58,48,52,58,48,53,0]
       ,half 0x9a0084 1,half 0x9a102a 1,word 0x9a228c 640,word 0x9a2290 480,word 0x9a2294 988,word 0x9a2298 1,word 0x9a229c 988,word 0x9a22a0 1
       ,wordsAt 0x9a2000 [10,10],half 0x9a2010 2,word 0x9a2180 100,half 0x9a22a4 3,half 0x9a22a6 2
       ,half (destination+0x74) 0xffff,half (destination+0x76) 0xffff,half (destination+0x58a) 0,word (destination+0x1d38) 0,word 0x9c0000 0] Nothing]
  thresholdCases =
    [make ("metadata-sensitivity-public-threshold-"++show input) 0x733dd0 "metadata_sensitivity_threshold" [input] metadataSensitivityInitial [] (Just expected)
    | (input,expected) <- metadataSensitivitySamples]
  profileCases =
    [make ("metadata-profile-bank-threshold-"++show profile) 0x74dbe4 "profile_alternate_bank" [profile]
      (Buffers.configurationInitial configurationBase [(0x1c1d,3)]) [] (Just expected)
    | (profile,expected) <- [(0,0),(29,0),(30,1),(31,1),(0xffffffff,0)]] ++
    [make ("metadata-rate-grid-direct-bank-"++show profile) 0xef3434 "rate_grid_weights" [5,profile,destination]
      (Buffers.configurationInitial configurationBase [(0x8e40,0),(0x1c1d,3),(0x1c38,0x34),(0x1c39,0x12),(0x1c3a,0x78),(0x1c3b,0x56),(0x1c3c,0xbc),(0x1c3d,0x9a)
        ,(0x1cb0,0x21),(0x1cb1,0x43),(0x1cb2,0x65),(0x1cb3,0x87),(0x1cb4,0xa9),(0x1cb5,0xcb)] ++ [poison destination 8])
      [bytes destination expected] Nothing
    | (profile,expected) <- [(29,[0x34,0x12,0x78,0x56,0xbc,0x9a,0xa5,0xa5]),(30,[0x21,0x43,0x65,0x87,0xa9,0xcb,0xa5,0xa5])]]
  splineCases =
    [make "metadata-spline-degenerate-knots-return-error-after-constant-copy" 0x722abc "build_metadata_spline" [2,source,source+16,destination,destination+16,destination+32,destination+48]
      [wordsAt source [0,0],wordsAt (source+16) [0x3f800000,0x40000000],poison destination 64]
      [wordsAt destination [0x3f800000,0x40000000],poison (destination+16) 48] (Just 0xffffffff)
    ,make "metadata-spline-two-knots-linear-fit" 0x722abc "build_metadata_spline" [2,source,source+16,destination,destination+16,destination+32,destination+48]
      [wordsAt source [0,0x3f800000],wordsAt (source+16) [0,0],poison destination 64]
      [wordsAt destination [0,0],wordsAt (destination+16) [0,0],word (destination+32) 0,word (destination+48) 0,word (destination+36) 0xa5a5a5a5,word (destination+52) 0xa5a5a5a5] (Just 0)
    ,make "metadata-spline-two-knots-nonzero-affine-fit" 0x722abc "build_metadata_spline" [2,source,source+16,destination,destination+16,destination+32,destination+48]
      [wordsAt source [0,0x3f800000],wordsAt (source+16) [0x3f800000,0x40400000],poison destination 64]
      [wordsAt destination [0x3f800000,0x40400000],wordsAt (destination+16) [0x40000000,0x40000000],word (destination+32) 0,word (destination+48) 0] (Just 0)
    ,make "metadata-spline-evaluation-affine-interior" 0x723234 "evaluate_metadata_spline" [2,source,0x3f000000,source+16,source+32,source+48,source+64]
      [wordsAt source [0,0x3f800000],wordsAt (source+16) [0x3f800000,0x40400000],wordsAt (source+32) [0x40000000,0x40000000],zero (source+48) 32]
      [] (Just 0x40000000)]
  snapshotCases =
    [make "metadata-snapshot-uncached-selection-fallback" 0x73fdb8 "metadata_cached_snapshot_value" [31] [] [] (Just 64)
    ,make "metadata-snapshot-cache-hit-preserves-value" 0x73fdb8 "metadata_cached_snapshot_value" [0]
      [bytes 0x95ada0 [77],word 0x95ada4 1] [bytes 0x95ada0 [77],word 0x95ada4 1] (Just 77)
    ,make "metadata-snapshot-cache-miss-publishes-value-and-validity" 0x73fdb8 "metadata_cached_snapshot_value" [0]
      (Buffers.configurationInitial configurationBase [(0x3e44ed,83)] ++ [word 0x95ada4 0])
      [bytes 0x95ada0 [83],word 0x95ada4 1] (Just 83)
    ,make "metadata-snapshot-five-projections-through-real-cache-and-readers" 0xd5e5a4 "export_metadata_snapshot" [0,0,destination]
      (Buffers.configurationInitial configurationBase ([(0x3e44ed,83),(0x3e451a,0),(0x3e450c,91)]
        ++ [(key+offset,fromIntegral (row*11+offset+1)) | (row,key)<-zip [0..] [0x3e4527,0x3e4595,0x3e4603,0x3e4671,0x3e46df],offset<-[0..10]])
        ++ [zero 0x95ada0 320,poison destination 60])
      [bytes destination ([83,91]++[1..55]++[165,165,165]),bytes 0x95ada0 [83],word 0x95ada4 1,bytes 0x95ae98 [91],word 0x95ae9c 1] Nothing
    ,make "metadata-distortion-panorama-bypasses-configuration" 0xe57ba4 "metadata_distortion_enabled" [source]
      [word (source+0x4b4) 29] [] (Just 0)
    ,make "metadata-sensor-level-unsigned-threshold-after-signed-reader" 0xe57cb4 "metadata_level_band" [source]
      (Buffers.configurationInitial configurationBase [(0x3e3a00,0),(0x3e3a10,0),(0x3e3a11,0x80)]
       ++ [bytes (source+0x6c0) [1],half (source+0x2264) 0x7fff]) [] (Just 0)
    ,make "metadata-snapshot-null-projection-bypasses-configuration" 0x73fe8c "metadata_snapshot_projection" [0,0,0] [] [] Nothing] ++
    [make ("metadata-threshold-inclusive-upper-band-"++show input) 0x73fce0 "metadata_threshold_band" [0,input]
      (Buffers.configurationInitial configurationBase [(0x3e3d50,0),(0x3e3d51,0),(0x3e3d52,100),(0x3e3d53,0),(0x3e3d54,200),(0x3e3d55,0),(0x3e3d56,44),(0x3e3d57,1)])
      [] (Just expected)
    | (input,expected)<-[(0xffffffff,0),(0,0),(1,1),(100,1),(101,2),(200,2),(201,3),(300,3),(301,4)]]
  balanceCases =
    [make "metadata-balance-custom-order-and-null-outputs" 0xeb02f8 "select_metadata_balance" [1,0,destination,0,destination+4]
      (Buffers.configurationInitial configurationBase [(0xa902,2),(0xa903,0x34),(0xa904,0x12),(0xa907,0x78),(0xa908,0x56)]++[poison destination 8])
      [bytes destination [0x34,0x12,165,165,0x78,0x56,165,165]] Nothing
    ,make "metadata-balance-live-parameter-triplet" 0xeb02f8 "select_metadata_balance" [1,0,destination,destination+2,destination+4]
      (Buffers.configurationInitial configurationBase [(0xa902,0)]++[bytes 0x409c2944 [10,0,20,0,30,0],poison destination 8])
      [bytes destination [10,0,20,0,30,0,165,165]] Nothing]
  previewCases =
    [let (initial,final,calls) = SensorSettings.collectSettingsLaw image source destination 0x1568ffc example 14
     in (make ("metadata-preview-real-settings-"++show example) 0xe560a0 "prepare_metadata_preview" [source,destination] initial final Nothing) {stageCalls=calls}
    | example <- [SensorSettings.OrdinarySettings,SensorSettings.PreparedSettings,SensorSettings.SavedSettings]]
  snapshotPayload = [83,91]++[1..55]
  snapshotConfiguration =
    [bytes (SensorSettings.settingsConfigurationBase+key) [value]
    | (key,value) <- [(0x3e44f3,83),(0x3e451a,2),(0x3e450c,91)]
      ++ [(key+offset,fromIntegral (row*11+offset+1)) | (row,key)<-zip [0..] [0x3e4527,0x3e4595,0x3e4603,0x3e4671,0x3e46df],offset<-[0..10]]]
  snapshotWarmMemory = snapshotConfiguration ++ [bytes 0x95add0 [83],word 0x95add4 1,bytes 0x95ae98 [91],word 0x95ae9c 1]
  (noiseInitial,noiseFinal,noiseCalls) = SensorRequests.noiseTableLawFromMemory bundle source 0x156af00 0x1569a74 snapshotWarmMemory
  snapshotRootInitial = noiseInitial ++
    [zero (source+0x3000) 0x300,zero 0x95ada0 328,zero 0x1568ffc 0x601,poison 0x156973c 60]
  snapshotExpected output =
    [bytes output [0,165],zero (output+2) 67,bytes (output+0x45) [165],zero (output+0x46) 121,bytes (output+0xbf) [165]
    ,zero (output+0xc0) 14,bytes (output+0xce) [165,165],zero (output+0xd0) 124,bytes (output+0x14c) (snapshotPayload++[165,165,165])]
  sensorSnapshotCases =
    [(make "metadata-sensor-snapshot-real-settings-template-expansion" 0xe57fc8 "populate_sensor_snapshot" [source,destination]
      (snapshotRootInitial++[poison destination 0x188])
      (noiseFinal ++ snapshotExpected destination ++
       [Patch (Address 0x1568ffc) (SensorSettings.settingsPacket source SensorSettings.OrdinarySettings 14),bytes 0x156973c (snapshotPayload++[165,165,165])]) Nothing) {stageCalls=noiseCalls}]
  rafConfiguration = snapshotConfiguration ++
    [bytes (SensorSettings.settingsConfigurationBase+key) [value]
    | (key,value) <- [(0x4d20,0x34),(0x4d21,0x12),(0x4d22,0x78),(0x4d23,0x56)
      ,(0x4d40,0xa1),(0x4d41,0xa2),(0x4d42,0xa3),(0x4d43,0xa4),(0x4d91,5),(0x4d92,0),(0x4d9d,7),(0x4d9e,0x23),(0x4d9f,1),(0x17936,17)]
      ++ zip [0x4d5c..0x4d63] [11..18] ++ zip [0x50d0..0x50f3] [1..36]]
  rafCases =
    [let entryMemory = rafConfiguration ++ snapshotWarmMemory ++ [zero 0x409c2944 6,zero 0x9189a0 248,zero 0x940b00 440]
         (initial,final,calls) = SensorRequests.noiseTableLawFromMemory bundle source 0x156af00 0x1569a74 entryMemory
     in (make ("metadata-RAF-real-settings-expansion-splines-mirrors-"++show withMirrors) 0xebde7c "populate_raf_metadata" [source,0x9c0000,destination,if withMirrors then 0x9e0000 else 0,result,if withMirrors then 0xa00000 else 0]
       (initial ++ [zero (source+0x3000) 0x300,zero 0x95ada0 328,zero 0x1568ffc 0x601,poison 0x156973c 60
        ,zero 0x9c0000 0x100,word 0x9c0030 0x1000,poison destination 0x5700,poison 0x9e0000 0x5700,poison result 0x460,poison 0xa00000 0x460
        ,half (source+0x164c) 10,half (source+0x164e) 20,half (source+0x1650) 3,word (source+0x648) 100,word (source+0x64c) 40
        ,wordsAt (source+0x160c) [40,80,120,160,200,240,280,320]])
       (filter (visibleRafEffect withMirrors) (final ++
        [bytes 0x9c0060 [11..18],word (source+0x19ac) 0x23a8,bytes (source+0x188c) [17],zero (source+0x1894) 42,zero (source+0x18c4) 186
        ,bytes destination [0x34,0x12,0x78,0x56],bytes 0x9e0000 [0x34,0x12,0x78,0x56]
        ,bytes (destination+20) [0xa4,0xa3,0xa2,0xa1],bytes 0x9e0014 [0xa4,0xa3,0xa2,0xa1]
        ,bytes (destination+24) [36,35..1],bytes 0x9e0018 [36,35..1]
        ,half (destination+60) 12,half (destination+62) 36,half (destination+64) 0xffe4,half (destination+66) 100
        ,half 0x9e003c 12,half 0x9e003e 36,half 0x9e0040 32,half 0x9e0042 100,word (destination+68) 0x14d0000,word 0x9e0044 0x14d0000
        ,Patch (Address 0x1568ffc) (SensorSettings.settingsPacket source SensorSettings.OrdinarySettings 14)
        ,Patch (Address 0x15b7e30) (SensorSettings.settingsDescriptor image SensorSettings.OrdinarySettings 14)]
        ++ snapshotExpected (destination+0x52f4) ++ snapshotExpected 0x9e52f4 ++
        concatMap rafExtensionExpected [result,0xa00000] ++
        [wordsAt (result+0x128) (replicate 36 45),poison 0xa00128 144,poison 0xa003a0 184,word (result+0x458) 0,word 0xa00458 0
        ,wordsAt (result+0x3a0) [291,11],wordsAt (result+0x3a8) (concat [[index,10] | index<-[0..10]])
        ,wordsAt (result+0x400) (concat (replicate 11 [10000,100]))]) ++
        if withMirrors then [] else [poison 0x9e0000 0x5700,poison 0xa00000 0x460]) Nothing) {stageCalls=calls}
    | withMirrors <- [False,True]]
  rafExtensionExpected output =
    [bytes output [73,73,42,0,8,0,0,0],bytes (output+8) [1,0,165,165,0,240,165,165]
    ,wordsAt (output+16) [13,1,26,0],half (output+32) 16
    ,word (output+0x30) 0x5678,word (output+0x40) 0x1234,word (output+0x50) 12,word (output+0x60) 4,word (output+0x70) 8301
    ,word (output+0x80) 0,word (output+0x90) 0x800,word (output+0xa0) 0x800,word (output+0xb0) 7
    ,word (output+0xc0) 224,word (output+0xd0) 368,word (output+0xe0) 552,word (output+0xf0) 584,word (output+0x100) 596,word (output+0x110) 608,word (output+0x120) 856,word (output+0x124) 0
    ,wordsAt (output+0x1b8) [291,11],wordsAt (output+0x1c0) (concat [[index,10] | index<-[0..10]])
    ,wordsAt (output+0x218) (concat (replicate 11 [0,1000])),wordsAt (output+0x270) [0,0,0,17],wordsAt (output+0x280) [0,0,0,21]
    ,zero (output+0x290) 24,wordsAt (output+0x2a8) [291,10],wordsAt (output+0x2b0) (concat [[index,10] | index<-[1..10]])
    ,wordsAt (output+0x300) (concat (replicate 20 [0,1000000]))]
  visibleRafEffect True _ = True
  visibleRafEffect False (Patch (Address address) _) =
    not ((address>=0x9e0000 && address<0x9e5700) || (address>=0xa00000 && address<0xa00460))
