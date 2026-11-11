{-# LANGUAGE GHC2021 #-}
module OutputJpegScenarios (outputJpegPublications, outputJpegScenarios, OutputSelection (..), outputSelectorInitial, outputConfiguredImage, outputScatterEffects) where

import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import PipelineModel
import qualified Data.ByteString.Char8 as C
import Scenarios

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

outputJpegPublications :: [StagePublication]
outputJpegPublications =
  [ publication address suffix
  | (address,suffix) <-
    [(0xe51b9c,"prepare_output_stage"),(0xe51d00,"configure_output"),(0xe51d5c,"configure_output_geometry"),(0xe51ee8,"prepare_output_planes")
    ,(0x11d2adc,"build_scatter_list"),(0x11d4fc4,"build_packet_list"),(0x11d57ac,"append_output_buffer"),(0x11d43ec,"initialize_raw_output")
    ,(0x11dbe40,"initialize_metadata_identity"),(0x69f354,"read_vendor_text"),(0x69f388,"read_product_text"),(0x69f3b8,"read_firmware_text"),(0x69f494,"read_firmware_word")
    ,(0x11dbd18,"read_device_code"),(0x11dbd7c,"read_serial"),(0x11dbddc,"read_copyright"),(0x11c1060,"set_metadata_identity")
    ,(0xe5834c,"capture_output_metadata"),(0x69d384,"format_calendar"),(0x11db930,"format_metadata_date")
    ,(0x11db820,"initialize_metadata_block"),(0x11db890,"initialize_secondary_metadata"),(0x11db8ec,"initialize_resolution")
    ,(0x11db9f0,"initialize_exif_dates"),(0x11dba24,"initialize_exif_defaults"),(0x11dbb94,"initialize_thumbnail_metadata"),(0x11dbbe0,"initialize_output_metadata")
    ,(0x11d4834,"select_raw_buffer"),(0x11d4448,"advance_raw_output"),(0x11d24f4,"fill_jpeg_padding")
    ,(0x11d2778,"relocate_jpeg"),(0x11d35e8,"align_jpeg"),(0x11d434c,"build_raw_exif_header"),(0x11d48c8,"prepare_jpeg_output")
    ,(0x11d3f10,"build_mpf_scatter"),(0x11d3118,"write_mpf_header"),(0x11d2f7c,"write_template"),(0x11d2f18,"write_ascii_utf16")
    ,(0x11c2c98,"tiff_type_size"),(0x11c2c40,"copy_tiff_value"),(0x11c2c58,"allocate_tiff_value"),(0x11cf490,"copy_padded_tiff_value"),(0x11cf5a8,"write_tiff_value")
    ,(0x11c1d48,"write_mpf_index"),(0x11c1d88,"write_mpf_attributes"),(0x11c1e00,"patch_app1_length"),(0x11c1e4c,"patch_thumbnail_length"),(0x11c1d18,"patch_thumbnail_offset"),(0x11db188,"finish_exif"),(0x11cef60,"copy_mpf_records")
    ,(0x11c7f28,"write_raf_integer"),(0x11c8260,"write_raf_header"),(0x11c849c,"write_raf_tag_records"),(0x11c8704,"write_raf_directory"),(0x11db4ec,"write_raf_tags")
    ,(0x11db3f8,"finish_raf_header"),(0x11db49c,"begin_raf_header"),(0x11db514,"pack_raf"),(0x11db6dc,"build_raf_header")
    ,(0x69f520,"public_version_bytes"),(0x11dbc30,"initialize_raf_metadata"),(0xe50cd4,"prepare_raf_metadata")
    ,(0xea9790,"metadata_quality_selection"),(0xeaa430,"copy_capture_date"),(0xeaa39c,"copy_exif_creation_date"),(0xeaa3e8,"copy_exif_digitization_date")
    ,(0xeaa2ec,"store_exposure_compensation"),(0xeb03d0,"rotate_metadata_orientation"),(0xeaa5bc,"populate_location_metadata")
    ,(0x11dbf48,"find_quality_profile"),(0x11dc010,"initialize_quality_tables"),(0x11dc338,"get_metadata_quality")]
  ]

outputJpegScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
outputJpegScenarios _ _ = scatterCases ++ packetCases ++ appendCases ++ rawCases ++ requestCases ++ identityCases ++ metadataCases ++ activeStageCases ++ serializerCases ++ rafCases ++ qualityCases
 where
  list = 0x9c0000
  image = 0x1600000
  raw = 0x15e0000
  job = 0x9b0000
  request = 0x9b5000
  configuration = 0x1700000
  make name address suffix arguments initial final result = StageScenario (StageCaseName name) (publication address suffix) arguments initial final result
    [(Address 0x11d0ea4,publication 0x11cef60 "copy_mpf_records") | address `elem` [0xe51b9c,0x11c1d48,0x11d3f10,0x11d3118]] []
  scatterCases =
    [make ("output-scatter-"++show (header,size,body,bodySize,secondary,secondarySize,present)) 0x11d2adc "build_scatter_list" [list,image]
      [wordsAt list [0x1234,0x5678],word (image+0xf2fc) 0,word image header,word (image+8) size,word (image+0x180) body,word (image+0x188) bodySize,word (image+0x2d8) secondary,word (image+0x2e0) secondarySize,word (image+0x424) present,word (image+0x38) 0x940100,word (image+0x40) 23]
      [wordsAt list ([0x1234,0x5678]++concatMap (uncurry (\a b -> [a,b])) entries++[0,0]),word (list+0x88) 0] Nothing
    | (header,size)<-[(0,0),(0,7),(0x940000,0),(0x940000,64)]
    , (body,bodySize)<-[(0,0),(0x950000,128)]
    , (secondary,secondarySize,present)<-[(0,0,0),(0x960000,32,0),(0x960000,32,1),(0,32,1)]
    , let entries = filter (\(a,b)->a/=0 && b/=0) ([(header,size),(body,bodySize)]++[(secondary,secondarySize)|present==1])++[(0x940100,23)]] ++
    [make "output-scatter-raw-parts" 0x11d2adc "build_scatter_list" [list,image]
      [wordsAt list [3,7],word (image+0xf2fc) 1,word (image+0xf310) 0x940000,wordsAt (image+0xf324) [16,32,64,128,256]]
      [wordsAt list [3,7,0x940000,496,0,0],word (list+0x88) 0] Nothing]
  packetCases =
    [make ("output-packet-secondary-"++show secondarySize) 0x11d4fc4 "build_packet_list" [list,raw]
      [word (raw+0xb608) image,word (image+0xf310) 0x940000,word (image+8) 11,word (image+0x188) 23,word (image+0x48) 7,word (raw+4) 19,word (raw+0x20) 29,word (raw+0x3c) 31,word (raw+0x58) 37,word (raw+0x74) 41,word (raw+0x8c) 0x950000,word (raw+0x90) 43,word (raw+0xa8) 0x960000,word (raw+0xac) secondarySize]
      ([wordsAt (list+8) ([0x940000,157,0x950000,43]++if secondarySize==0 then [0,0] else [0x94009d,41,0x960000,secondarySize,0,0]),word (list+0x88) 0]) Nothing
    | secondarySize<-[0,47]]
  appendCases =
    [make ("output-append-filled-"++show filled++"-half-"++show half) 0x11d57ac "append_output_buffer" [list,0x950000,256]
      [wordsAt list ([0x1111,0x2222]++concat (replicate filled [0x940000,16])++[if half then 0 else 0,if half then 17 else 0,0,0]),word (list+0x88) 0xa5]
      (if slot>=16 then [word (list+0x88) 0xa5] else [wordsAt (list+slot*8) [0x950000,256,0,0],word (list+0x88) 0])
      (Just (if slot>=16 then 0xffffffff else 0))
    | filled<-[0,1,13,14,15,16],half<-[False,True],let slot=fromIntegral filled+if half then 2 else 1]
  rawCases =
    [make ("output-raw-align-"++show buffer++"-encoding-"++show encoding) 0x11d43ec "initialize_raw_output" [image,buffer,encoding]
      [word (image+0xf300) 0xabababab,word (image+0xf308) 0xcdcdcdcd,word (image+0xf30c) 0xefefefef]
      [word (image+0xf2fc) 1,word (image+0xf300) 0xabababab,word (image+0xf304) encoding,word (image+0xf308) 0xcdcdcdcd,word (image+0xf30c) 0xefefefef,wordsAt (image+0xf310) [aligned,aligned,0,0,0,0,0,0,0,0]] Nothing
    | buffer<-[0x940000,0x940001,0x94003f,0xfffffffc],encoding<-[0,1,2],let aligned=(buffer+63) .&. 0xffffffc0]
  selector value = [word 0x9134f8 1,word 0x96ca50 configuration,word (configuration+0x80) value]
  requestCases =
    [make ("output-request-bypass-"++suffix++"-"++show option) address suffix [job,request,request+64,0,0]
      (selector option++[wordsAt request [0,2,0]]) [] (Just 1)
    | (address,suffix)<-[(0xe51b9c,"prepare_output_stage"),(0xe51d00,"configure_output"),(0xe51d5c,"configure_output_geometry")],option<-[0xa5,0xa7,0xa9]] ++
    [make ("output-configure-pointer-"++show pointer) 0xe51d00 "configure_output" [job,request]
      [word 0x9134f8 0,wordsAt request [0,1,pointer]] [word (0x155ccc4+0xb608) (if pointer==0 then 0x154d974 else pointer)] (Just 1)
    | pointer<-[0,image]] ++
    [make "output-plane-source-bypass" 0xe51ee8 "prepare_output_planes" [job,request,0,0,request+64,0,0,0,0,0,0]
      (selector 0xa7++[wordsAt request [1,0x940000,1024,64,32,16,0x2000],word (job+0x4a0) 7,word (job+0x4a4) 9])
      [wordsAt (request+64) [1,0x940000,1024,64,32,16,0x2000]] (Just 1)]
  identityCases =
    [make "output-identity-already-initialized" 0x11dbe40 "initialize_metadata_identity" []
      [Patch (Address 0x14de9ac) (B.singleton 1),Patch (Address 0x14e9d98) (B.replicate 162 0xa5)]
      [Patch (Address 0x14e9d98) (B.replicate 162 0xa5)] Nothing] ++
    [make "output-identity-initialize" 0x11dbe40 "initialize_metadata_identity" []
      [Patch (Address 0x14de9ac) (B.singleton 0),word 0x96ca50 configuration,Patch (Address configuration) (B.replicate 1024 0)
      ,Patch (Address (configuration+0x16f)) (B.pack [70,85,74,73,70,73,76,77,0]),Patch (Address (configuration+0x15c)) (B.pack [88,45,77,49,0])
      ,Patch (Address (configuration+0x158)) (B.pack [48,49,53,48]),Patch (Address (configuration+0x40)) (B.pack [65,49,63,0])]
      [Patch (Address 0x14de9ac) (B.singleton 1)
      ,Patch (Address 0x14e9d98) (B.pack ([70,85,74,73,70,73,76,77]++replicate 8 0++[88,45,77,49]++replicate 28 0
        ++[68,105,103,105,116,97,108,32,67,97,109,101,114,97,32,88,45,77,49,32,86,101,114,49,46,53,48]++replicate 37 0
        ++[48,49,53,48,65,49,32,32]++replicate 42 32))] Nothing] ++
    [make ("output-identity-field-"++suffix) address suffix [request]
      [word 0x96ca50 configuration,Patch (Address (configuration+key)) (B.pack bytes)]
      [Patch (Address request) (B.pack expected)] (Just 0)
    | (address,suffix,key,bytes,expected)<-
      [(0x69f354,"read_vendor_text",0x16f,[70,85,74,73,0,88,88,88,88,88,88,88],[70,85,74,73,0,0,0,0,0,0,0,0])
      ,(0x69f388,"read_product_text",0x15c,replicate 19 65,replicate 19 65++[0])
      ,(0x69f494,"read_firmware_word",0x158,[1,2,3,4],[1,2,3,4])
      ,(0x11dbd18,"read_device_code",0x40,[65,0,255,57],[65,32,32,57])
      ,(0x11dbd7c,"read_serial",0x44,[65,90,97,122,48,57,47,58,64,91,96,123],[65,90,97,122,48,57,32,32,32,32,32,32])]] ++
    [make ("output-firmware-version-"++show selectorByte) 0x69f3b8 "read_firmware_text" [request]
      [word 0x96ca50 configuration,Patch (Address (configuration+0x204)) (B.pack [3,0x42,selectorByte])]
      [Patch (Address request) (if selectorByte==255 then B.pack [86,101,114,51,46,52,50,0] else B.pack [86,101,114,49,46,53,48,0])] (Just 0)
    | selectorByte<-[0,1,255]]
  clock = request+0x100
  metadataCases =
    [make ("output-calendar-order-"++show order) 0x69d384 "format_calendar" [request,clock,order]
      [Patch (Address clock) (B.pack [26,11,9,23,7,5])]
      [Patch (Address request) (C.pack expected)] Nothing
    | (order,expected)<-[(0,"26/11/09 23:07:05\0"),(1,"11.09.26 23:07:05\0"),(2,"09/11/26 23:07:05\0"),(99,"09/11/26 23:07:05\0")]] ++
    [make "output-metadata-date" 0x11db930 "format_metadata_date" [clock,request]
      [Patch (Address clock) (B.pack [26,11,9,23,7,5])]
      [Patch (Address request) (C.pack "2026:11:09 23:07:05\0")] Nothing] ++
    [make "output-resolution-defaults" 0x11db8ec "initialize_resolution" [request]
      [Patch (Address request) (B.replicate 64 0xa5)]
      [wordsAt request [72,1,72,1],Patch (Address (request+0x28)) (B.pack [6,0,1,0,2,0,2,0]),word (request+0x10) 0xa5a5a5a5] Nothing] ++
    [make "output-thumbnail-defaults" 0x11dbb94 "initialize_thumbnail_metadata" [request,clock]
      [Patch (Address request) (B.replicate 192 0xa5),Patch (Address clock) (B.pack [26,11,9,23,7,5])]
      [wordsAt (request+0x70) [72,1,72,1],Patch (Address (request+0x84)) (B.pack [1,0,2,0,2,0]),Patch (Address (request+0x9e)) (C.pack "2026:11:09 23:07:05\0    \0")] Nothing] ++
    [make ("output-debug-metadata-disabled-"++show enabled++"-"++show pointer) 0xe5834c "capture_output_metadata" [job,request]
      [word 0x96ca50 configuration,word (configuration+0xa8) enabled,word (job+0x220c) pointer,word (image+0xdbf3c) 0x11223344]
      [word (image+0xdbf3c) 0x11223344] Nothing
    | (enabled,pointer)<-[(0,image),(0x80,0)]] ++
    [make "output-debug-metadata-active" 0xe5834c "capture_output_metadata" [job,request]
      [word 0x96ca50 configuration,word (configuration+0xa8) 0x80,word (job+0x220c) image,word (configuration+0x9a82) 3
      ,word (request+0x308) 0x12345678,word (request+0x568) 0x90ab,word (job+0x6c0) 5,word (configuration+0x19004) 7
      ,Patch (Address (configuration+0x9a84)) (B.pack [1,2,3,4,5,6,7,8,9,10])]
      [word (image+0xdbf3c) 0x53545053,Patch (Address (image+0xdbf40)) (B.pack [3,0,0x78,0x56,0xab,0x90,7,0,1,2,3,4,5,6,7,8,9,10]),word (image+0xdbf54) 0x4c415053] Nothing]
  activeStageCases =
    [make ("output-stage-source-scatter-bank-"++show kind) 0xe51b9c "prepare_output_stage" [job,request]
      [word 0x9134f8 0,word 0x96ca50 configuration,word (configuration+0xa8) 0,wordsAt request [0,kind,image]
      ,word (image+0xf2fc) 0,word image 0x940000,word (image+8) 64,word (image+0x180) 0x950000,word (image+0x188) 128,word (image+0x424) 0
      ,word (image+0x38) 0x960000,word (image+0x40) 23]
      [wordsAt (job+offset+8) [0x940000,64,0x950000,128,0x960000,23,0,0],word (job+offset+0x88) 0] (Just 1)
    | (kind,offset)<-[(0,0x27d0),(1,0x2920)]]
  serializerCases =
    [make ("output-padding-"++show size) 0x11d24f4 "fill_jpeg_padding" [request,size,0]
      [Patch (Address request) (B.replicate 520 0xa5)]
      [Patch (Address request) (B.pack (take (fromIntegral size) (concat [[fromIntegral n,fromIntegral (128+n)] | n<-take 260 (cycle [0..127])])))] Nothing
    | size<-[0,1,2,3,256,257,512]] ++
    [make ("output-padding-resume-"++show (preceding,lastByte)) 0x11d24f4 "fill_jpeg_padding" [request+2,6,1]
      [Patch (Address request) (B.pack [preceding,lastByte]<>B.replicate 6 0xa5)]
      [Patch (Address (request+2)) (B.pack expected)] Nothing
    | (preceding,lastByte,expected)<-[(0,128,[1,129,2,130,3,131]),(255,127,[128,0,129,1,130,2]),(1,1,[2,2,3,3,4,4])]] ++
    [make ("output-select-raw-phase-"++show phase) 0x11d4834 "select_raw_buffer" [phase,image]
      [wordsAt (image+0xf310) [0x940000,0x950000,0x960000,0x970000,0x980000]]
      [word (image+offset) value | (offset,value)<-expected] Nothing
    | (phase,expected)<-[(0,[(0xf308,0x940000)]),(1,[(0,0x950000)]),(2,[(0x248,0x960000)]),(3,[(0x420,0x970000)]),(4,[(0x100,0x97fffe)]),(5,[])]] ++
    [make ("output-template-null-"++show (destination,table)) 0x11d2f7c "write_template" [destination,table] [] [] (Just 0)
    | (destination,table)<-[(0,request),(request,0)]] ++
    [make "output-template-mixed-scalars" 0x11d2f7c "write_template" [request,clock]
      [wordsAt clock [0,3,0,clock+128,1,2,1,clock+132,2,1,2,clock+136,3,0,0,clock+140,0xffffffff,0,0,0]
      ,Patch (Address (clock+128)) (B.pack [0xaa,0xbb,0xcc,0,0x34,0x12,0xcd,0xab,0x78,0x56,0x34,0x12,65,90,0])]
      [Patch (Address request) (B.pack [0xaa,0xbb,0xcc,0x12,0x34,0xab,0xcd,0x78,0x56,0x34,0x12,65,0,90,0,0,0])] (Just 17)] ++
    [make "output-mpf-record-callback-overlap" 0x11cef60 "copy_mpf_records" [request,0,request+12,clock,clock+16]
      [Patch (Address (request+8)) (B.pack [0..31]),wordsAt clock [0x10001],wordsAt (clock+16) [0x7b002,32,0]]
      [Patch (Address (request+12)) (B.pack [0..31])] Nothing] ++
    [make "output-mpf-zero-images" 0x11d3f10 "build_mpf_scatter" [request,0,0,0]
      [wordsAt request [0x1111,0x2222],word 0x14ddb58 99]
      [wordsAt request [0x1111,0x2222,0,0],word (request+0x88) 0,word 0x14ddb58 0] Nothing] ++
    [make "output-mpf-index-directory" 0x11c1d48 "write_mpf_index" [request,list]
      [word 0x14e9e3c 0,wordsAt request [0x30303130,2,0x20020002,123,0,0,0x20002,456,789,0,list],Patch (Address list) (B.replicate 80 0xa5)]
      [Patch (Address list) (B.pack [3,0,0,0xb0,7,0,4,0,0,0,48,49,48,48,1,0xb0,4,0,1,0,0,0,2,0,0,0,2,0xb0,7,0,32,0,0,0,42,0,0,0,0xa5,0xa5,0xa5,0xa5])
      ,wordsAt (list+42) [0x20020002,123,0,0,0x20002,456,789,0],word (0x14e9d50+0x40) (list+38)] (Just 74)] ++
    [make "output-stage-two-image-mpf" 0xe51b9c "prepare_output_stage" [job,request]
      ([word 0x9134f8 0,word 0x96ca50 configuration,word (configuration+0xa8) 0,word 0x14e9e3c 0
       ,wordsAt request [0,4,image,request+64,request+96],wordsAt (request+64) [0,0x1804000,256],wordsAt (request+96) [0,0x1814000,256]
       ,word (job+0x3234) image,word (job+0x3238) (image+0x20000)]
       ++concat [[word work jpeg,word (work+8) 64,word (work+0x1c) slot,wordsAt (work+0x20) values
         ,word (work+0x38) tailAddress,word (work+0x40) 10,word (work+0x180) dataAddress,word (work+0x188) 128
         ,word (work+0x2d8) secondaryAddress,word (work+0x2e0) 32,Patch (Address jpeg) (B.replicate 64 0xa5)
         ,Patch (Address (jpeg+4)) (B.pack [0,32]),Patch (Address tailAddress) (B.replicate 64 0xa5),Patch (Address header) (B.replicate 256 0xa5)]
        | (work,slot,jpeg,dataAddress,secondaryAddress,tailAddress,header,values)<-
          [(image,0,0x1800000,0x1801000,0x1802000,0x1803000,0x1804000,[100,200,3,4,5,6])
          ,(image+0x20000,1,0x1810000,0x1811000,0x1812000,0x1813000,0x1814000,[101,201,13,14,15,16])]])
      [wordsAt (job+0x2a78) [0x1800000,64,0x1801000,128,0x1802000,32,0x1804000,192,0x1803000,62
        ,0x1810000,64,0x1811000,128,0x1812000,32,0x1814000,128,0x1813000,10,0,0]
      ,word (job+0x2af8) 0,word 0x14ddb58 10,Patch (Address 0x14ddb50) (B.pack [126,0])
      ,Patch (Address 0x180300a) (B.replicate 52 0),Patch (Address 0x1804000) primaryMpfHeader,Patch (Address 0x1814000) secondaryMpfHeader] (Just 1)]
  rafCases =
    [make ("output-configure-raf-secondary-"++show secondaryExtent) 0xe51d00 "configure_output" [job,request]
      [word 0x9134f8 0,wordsAt request [0,2,image],word (image+0xf310) 0x1800000,word (image+8) 0,word (image+0x40) 0,word (image+0x188) 0
      ,Patch (Address 0x155ccc4) (B.replicate 0xb60c 0),word (0x155ccc4+0x90) 8,word (0x155ccc4+0xac) secondaryExtent,word (0x155ccc4+0x10c) 2056,word (0x155ccc4+0x128) (if secondaryExtent==0 then 0 else 2064)
      ,Patch (Address (0x155ccc4+0xdc+0x54)) (C.pack "CCD-RAW "),Patch (Address (0x155ccc4+0xdc+0x5c)) (C.pack "0201")
      ,Patch (Address (0x155ccc4+0xdc+0x68)) (C.pack "0150"),Patch (Address 0x14e9d98) (C.pack "FUJIFILM"),Patch (Address 0x14e9da8) (C.pack "X-M1\0")
      ,Patch (Address 0x1800000) (B.replicate fileSize 0),Patch (Address (0x1800000+fromIntegral fileSize)) (B.singleton 0xa5)]
      [word (0x155ccc4+0xb608) image,word 0x155ccc4 0x1800000,word (0x155ccc4+4) 148,word (0x155ccc4+0x20) 22064,word (0x155ccc4+0x3c) (if secondaryExtent==0 then 0 else 22064)
      ,word (0x155ccc4+0x58) 2048,word (0x155ccc4+0x74) (if secondaryExtent==0 then 0 else 2048),word (image+0x48) padding
      ,Patch (Address 0x1800000) (rafHeaderFixture fields<>B.replicate (fromIntegral padding) 0<>rafTagsFixture<>if secondaryExtent==0 then B.replicate 2048 0 else rafTagsFixture<>B.replicate 4096 0)
      ,Patch (Address (0x1800000+fromIntegral fileSize)) (B.singleton 0xa5)] (Just 1)
    | (secondaryExtent,padding,fileSize,fields)<-
      [(0,316,24576,[148,0,464,22064,22528,2056,0,2056,0,0,0,0,0,0,0,0])
      ,(16,268,48640,[148,0,416,22064,44544,2056,0,2056,0,22480,22064,46600,2064,0,2064,0])]]
  qualityCases =
    [make "output-quality-tables-ready" 0x11dc010 "initialize_quality_tables" []
      [Patch (Address 0x14de9ad) (B.singleton 1),word 0x14df450 0x12345678]
      [word 0x14df450 0x12345678] Nothing] ++
    [make "output-quality-profile-thumbnail" 0x11dbf48 "find_quality_profile" [160,120] [] [] (Just 1)] ++
    [make ("output-quality-slot-"++show slot) 0x11dc338 "get_metadata_quality" [1,160,120,slot,99,request]
      [Patch (Address 0x14de9ad) (B.singleton 1)] [word request (0x14df3b4+slot*2)] (Just 0)
    | slot<-[0,1]]

data OutputSelection = OutputNormal | OutputFirst | OutputSecond | OutputThird deriving (Eq, Show)

outputSelectorInitial :: Word32 -> OutputSelection -> [Patch]
outputSelectorInitial configuration selection =
  [word 0x96ca50 configuration,word 0x9134f8 (if selection==OutputNormal then 0 else 1)
  ,word (configuration+0x80) (case selection of OutputNormal->0; OutputFirst->0xa5; OutputSecond->0xa7; OutputThird->0xa9)]

outputConfiguredImage :: Word32 -> [Patch]
outputConfiguredImage pointer = [word (0x155ccc4+0xb608) (if pointer==0 then 0x154d974 else pointer)]

outputScatterEffects :: Word32 -> [(Word32,Word32)] -> (Word32,Word32) -> [Patch]
outputScatterEffects list segments (tailAddress,tailSize) =
  [wordsAt (list+8) (concat [[address,size] | (address,size)<-segments,address/=0,size/=0]++[tailAddress,tailSize,0,0]),word (list+0x88) 0]

halfBytes :: Word32 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `div` 256)]

tiffEntry :: Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
tiffEntry tag kind count value = halfBytes tag<>halfBytes kind<>wordBytes count<>wordBytes value

primaryMpfHeader :: B.ByteString
primaryMpfHeader = B.pack [255,226,0,190,77,80,70,0,73,73,42,0,8,0,0,0]
  <>halfBytes 3<>tiffEntry 0xb000 7 4 0x30303130<>tiffEntry 0xb001 4 1 2<>tiffEntry 0xb002 7 32 50<>wordBytes 82
  <>B.concat (map wordBytes [0x20020002,478,0,0,0x20002,362,246,0])
  <>halfBytes 4<>tiffEntry 0xb101 4 1 100<>tiffEntry 0xb204 4 1 200<>tiffEntry 0xb205 10 1 136<>tiffEntry 0xb206 5 1 144<>wordBytes 0
  <>B.concat (map wordBytes [3,4,5,6])<>B.replicate 32 0

secondaryMpfHeader :: B.ByteString
secondaryMpfHeader = B.pack [255,226,0,126,77,80,70,0,73,73,42,0,8,0,0,0]
  <>halfBytes 5<>tiffEntry 0xb000 7 4 0x30303130<>tiffEntry 0xb101 4 1 101<>tiffEntry 0xb204 4 1 201<>tiffEntry 0xb205 10 1 74<>tiffEntry 0xb206 5 1 82<>wordBytes 0
  <>B.concat (map wordBytes [13,14,15,16])<>B.replicate 30 0

beHalfBytes :: Word32 -> B.ByteString
beHalfBytes value = B.reverse (halfBytes value)

beWordBytes :: Word32 -> B.ByteString
beWordBytes value = B.reverse (wordBytes value)

rafHeaderFixture :: [Word32] -> B.ByteString
rafHeaderFixture fields = C.pack "FUJIFILMCCD-RAW 0201"<>B.replicate 8 0<>C.pack "X-M1\0"<>B.replicate 27 0<>C.pack "0150"<>B.replicate 20 0<>B.concat (map beWordBytes fields)

rafTagsFixture :: B.ByteString
rafTagsFixture = beWordBytes 10<>B.concat [beHalfBytes tag<>beHalfBytes size<>B.replicate (fromIntegral size) 0 | (tag,size)<-[(0x100,4),(0x110,4),(0x111,4),(0x112,4),(0x113,4),(0x130,4),(0x131,36),(0x141,4),(0x9650,4),(0xc000,0x55c0)]]
