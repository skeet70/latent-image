module OutputTripletsScenarios (publications, scenarios) where

import qualified Data.ByteString as B
import Data.Word (Word8, Word16, Word32)
import Data.Bits (shiftR)
import PipelineModel
import Scenarios (Address (..), Patch (..), call, readWord, wordsAt)
import ImagePlanesScenarios (currentTaskCalls, priorityInitial, priorityCalls, notificationCalls, reservedInitial, reservedWaitCalls, reservedSignalCalls)
import OutputMetadataScenarios (metadataRateInitial, metadataSensitivityInitial, metadataSensitivitySamples)
import qualified ProfileCalibrationScenarios as Profile
import qualified BuffersScenarios as Buffers
import qualified DebugStorageScenarios as Debug

publications :: [StagePublication]
publications =
  [ StagePublication (Address 0x00daf660) (StageSymbol "xm1_pipeline_image_write_noise_pixels")
  , StagePublication (Address 0x00daf2f4) (StageSymbol "xm1_pipeline_image_statistics_gamma")
  , StagePublication (Address 0x00e530d8) (StageSymbol "xm1_pipeline_image_compression_band")
  , StagePublication (Address 0x00ef4cc0) (StageSymbol "xm1_pipeline_image_scale_triplet_tenths")
  , StagePublication (Address 0x00ef4c84) (StageSymbol "xm1_pipeline_image_calibration_rate_adjustment")
  , StagePublication (Address 0x00e543b0) (StageSymbol "xm1_pipeline_image_output_format")
  , StagePublication (Address 0x00e55fc0) (StageSymbol "xm1_pipeline_image_prepare_noise_tables")
  , StagePublication (Address 0x00e56030) (StageSymbol "xm1_pipeline_image_finalize_noise_tables")
  , StagePublication (Address 0x00e544b0) (StageSymbol "xm1_pipeline_image_apply_triplet_tuning")
  , StagePublication (Address 0x0073aa5c) (StageSymbol "xm1_pipeline_image_window_selection")
  , StagePublication (Address 0x0073aa88) (StageSymbol "xm1_pipeline_image_window_zero_selection")
  , StagePublication (Address 0x00737378) (StageSymbol "xm1_pipeline_image_profile_delta_index")
  , StagePublication (Address 0x00737504) (StageSymbol "xm1_pipeline_image_profile_thirds")
  , StagePublication (Address 0x00736b5c) (StageSymbol "xm1_pipeline_image_profile_feature_index")
  , StagePublication (Address 0x0073749c) (StageSymbol "xm1_pipeline_image_profile_geometry_index")
  , StagePublication (Address 0x00733e24) (StageSymbol "xm1_pipeline_image_stamp_threshold")
  , StagePublication (Address 0x0069fcfc) (StageSymbol "xm1_pipeline_image_calibration_data")
  , StagePublication (Address 0x00e54870) (StageSymbol "xm1_pipeline_image_process_output_triplets")
  , StagePublication (Address 0x006571bc) (StageSymbol "xm1_pipeline_image_find_overlay_unit")
  , StagePublication (Address 0x0073b7a0) (StageSymbol "xm1_pipeline_image_wait_overlay")
  , StagePublication (Address 0x0073b728) (StageSymbol "xm1_pipeline_image_wait_overlay_channel")
  , StagePublication (Address 0x0073b238) (StageSymbol "xm1_pipeline_image_cancel_overlay_channel")
  , StagePublication (Address 0x0073aca4) (StageSymbol "xm1_pipeline_image_cancel_overlay")
  , StagePublication (Address 0x00e4e644) (StageSymbol "xm1_pipeline_image_notify_output_result")
  , StagePublication (Address 0x00e54c84) (StageSymbol "xm1_pipeline_image_prepare_output_stamp")
  , StagePublication (Address 0x00e53794) (StageSymbol "xm1_pipeline_image_tune_triplets")
  , StagePublication (Address 0x00e531ac) (StageSymbol "xm1_pipeline_image_prepare_triplets")
  , StagePublication (Address 0x006570f4) (StageSymbol "xm1_pipeline_image_boot_descriptor")
  , StagePublication (Address 0x006571a0) (StageSymbol "xm1_pipeline_image_overlay_group")
  , StagePublication (Address 0x00e61154) (StageSymbol "xm1_pipeline_image_finish_output_preview")
  ]

halves :: [Word16] -> B.ByteString
halves = B.pack . concatMap (\value -> [fromIntegral value, fromIntegral (value `shiftR` 8)])

bytes :: Word32 -> [Word8] -> Patch
bytes address = Patch (Address address) . B.pack

scenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
scenarios image _ = gammaCases ++ pixelCases ++ transformCases ++ bandCases ++ scaleCases ++ rateCases ++ formatCases ++ selectedFormatCases ++ scalarCases ++ stampThresholdCases ++ calibrationDataCases ++ inactiveTripletCases ++ tuningCases ++ activeCalibrationCases ++ overlayCases ++ outputNotificationCases ++ outputBypassCases ++ stampCases ++ previewSnapshotCases
 where
  pixelPublication = head publications
  gammaPublication = publications !! 1
  make name publication arguments initial final result = StageScenario
    (StageCaseName name) publication arguments initial final result [] []
  gammaTable maximum offset = Patch (Address 0x009b0000)
    (B.pack (1 : replicate 9 0) <> halves ([0,100..2700] ++ [offset,maximum]))
  gammaCases =
    [make ("gamma-segment-" ++ show segment ++ "-offset-" ++ show offset) gammaPublication
      [segment * 64 + offset, 0x009b0000] [gammaTable 4095 0] [] (Just (segment * 300 + result))
    | segment <- [0..8], (offset,result) <- [(0,0),(20,95),(21,100),(41,195),(42,200),(63,300)]] ++
    [make "gamma-upper-endpoint" gammaPublication [576,0x009b0000] [gammaTable 4095 0] [] (Just 2700)
    ,make "gamma-saturates-final-sample" gammaPublication [0xffffffff,0x009b0000] [gammaTable 1000 0] [] (Just 1000)
    ,make "gamma-input-offset" gammaPublication [1,0x009b0000] [gammaTable 4095 20] [] (Just 100)
    ,make "gamma-input-offset-wrap" gammaPublication [0xffffffff,0x009b0000] [gammaTable 4095 1] [] (Just 0)]
  pixelCases =
    [make name pixelPublication [0x009b1000,12,0x009b2000,100,0x009b2100,0x009b2200,0x009b2300,0x009b2400]
      ([wordsAt 0x009b2000 [0x009b3000,0x009b3100,0x009b3200,0x009b3300]
       ,bytes 0x009b2010 [2,1,2,0,2,0,1,1,1,1]
       ,wordsAt 0x009b3000 [red,red],wordsAt 0x009b3100 [green,green]
       ,wordsAt 0x009b3200 [blue,blue],wordsAt 0x009b3300 [green,green]
       ,bytes 0x009b2100 [0],bytes 0x009b2200 [0],bytes 0x009b2300 [0],bytes 0x009b2400 [0]
       ,bytes 0x009b1000 (replicate 12 0xa5)])
      [bytes 0x009b1000 ([luma,luma,cb,cr] ++ replicate 8 0xa5)
      ,bytes 0x41716618 [luma,luma],bytes 0x41716718 [cr,cr],bytes 0x41716818 [cb,cb]] Nothing
    | (name,red,green,blue,luma,cb,cr) <-
      [("statistics-black",0,0,0,0,128,128)
      ,("statistics-gray",6400,6400,6400,100,128,128)
      ,("statistics-weighted-color",6400,12800,3200,154,69,89)
      ,("statistics-red",6400,0,0,30,111,178)
      ,("statistics-white-clamped",65535,65535,65535,255,128,128)]]
  transformCases =
    [make name pixelPublication [0x009b1000,12,0x009b2000,exposure,0x009b2100,0x009b2200,0x009b2300,0x009b2400]
      ([wordsAt 0x009b2000 [0x009b3000,0x009b3100,0x009b3200,0x009b3300]
       ,bytes 0x009b2010 [2,1,2,0,2,0,1,1,1,1]
       ,wordsAt 0x009b3000 [6400,6400],wordsAt 0x009b3100 [0,0]
       ,wordsAt 0x009b3200 [0,0],wordsAt 0x009b3300 [0,0]
       ,bytes 0x009b2100 [gainFlag,0],Patch (Address 0x009b2102) (halves [512,256,256,256])
       ,bytes 0x009b2200 [matrixFlag,0],Patch (Address 0x009b2202) (halves (16383:matrixValues))
       ,Patch (Address 0x009b2300) (B.pack (gammaFlag:replicate 9 0) <> halves (replicate 28 6400 ++ [0,65535]))
       ,bytes 0x009b2400 [chromaFlag,0],Patch (Address 0x009b2402) (halves [0,1024,0,1024])
       ,bytes 0x009b1000 (replicate 12 0xa5)])
      [bytes 0x009b1000 ([luma,luma,cb,cr] ++ replicate 8 0xa5)
      ,bytes 0x41716618 [luma,luma],bytes 0x41716718 [cr,cr],bytes 0x41716818 [cb,cb]] Nothing
    | (name,exposure,gainFlag,matrixFlag,gammaFlag,chromaFlag,matrixValues,luma,cb,cr) <-
      [("statistics-gain",100,1,0,0,0,identity,60,94,228)
      ,("statistics-half-exposure",50,0,0,0,0,identity,15,120,153)
      ,("statistics-matrix-identity",100,0,1,0,0,identity,30,111,178)
      ,("statistics-matrix-negative-logical-clamp",100,0,1,0,0,[64512,0,0,0,1024,0,0,0,1024],77,85,255)
      ,("statistics-gamma-owned-helper",100,0,0,1,0,identity,100,128,128)
      ,("statistics-chroma-exchange",100,0,0,0,1,identity,30,178,111)]]
  identity = [1024,0,0,0,1024,0,0,0,1024]
  bandCases =
    [make ("compression-band-" ++ show input) (publications !! 2) [input] [] [] (Just result)
    | (input,result) <- [(0,0),(5,0),(6,1),(8,1),(9,0),(10,2),(12,2),(13,3),(15,3),(16,4),(18,4),(19,5),(21,5),(22,6),(24,6),(25,7),(26,8),(27,8),(28,9),(29,0),(0xffffffff,0)]]
  scaleCases =
    [make ("triplet-tenths-" ++ show factor) (publications !! 3) [0x009b0000,factor]
      [Patch (Address 0x009b0000) (halves [1024,65535,7])]
      [Patch (Address 0x009b0000) (halves expected)] Nothing
    | (factor,expected) <- [(0,[0,0,0]),(1,[102,6553,0]),(10,[1024,65535,7]),(25,[2560,32765,17])]]
  curveSample index =
    let sample = fromIntegral (B.index image (0x00ef50e8 - 0x00bf1a00 + index)) :: Int
    in fromIntegral ((if sample < 128 then sample else sample - 256) * 10)
  rateCases =
    [make ("calibration-rate-" ++ show input) (publications !! 4) [input] [] [] (Just (curveSample index))
    | (input,index) <- [(0,0),(9,0),(10,0),(11,1),(30,20),(50,40),(51,0),(266,0),(267,1),(0xffffffff,0)]]
  formatCases =
    [make ("output-format-" ++ show operation ++ "-capture-" ++ show capture ++ "-flags-" ++ show flags)
      (publications !! 5) [0x009b0000,operation]
      [wordsAt 0x009b04b4 [capture],wordsAt 0x009b0710 [flags]] [] (Just expected)
    | (operation,constant) <- [(0,0),(1,7),(2,7),(4,11),(5,0),(6,2),(7,2),(8,2),(9,9),(10,10),(11,8),(12,8),(13,2),(14,2),(15,0)]
    , (capture,flags) <- [(0,0),(1,0),(1,0x400),(25,0)]
    , let expected = if operation `elem` [6,7,8,13,14] && (capture == 25 || capture == 1 && flags == 0x400) then 3 else constant]
  selectedFormatCases =
    [make ("output-format-capture-option-" ++ show setting) (publications !! 5) [0x009b0000,3]
      [wordsAt 0x009134f8 [1],wordsAt 0x0096ca50 [0x009c0000],bytes 0x009c0080 [setting]] [] (Just expected)
    | (setting,expected) <- [(0xa5,1),(0xa7,5),(0xa9,5),(0,5)]]
  scalarCases =
    [make ("window-selection-" ++ show input) (publications !! 9) [input] [] [] (Just result)
    | (input,result) <- [(0,0),(1,1),(2,2),(3,0),(0xffffffff,0)]] ++
    [make ("window-zero-selection-" ++ show input) (publications !! 10) [input] [] [] (Just result)
    | (input,result) <- [(0,1),(1,2),(2,0),(0xffffffff,0)]] ++
    [make ("profile-delta-" ++ show input) (publications !! 11) [fromIntegral input] [] [] (Just result)
    | (input,result) <- zip ([-9..9] :: [Int]) [0..18] ++ [(-10,9),(10,9),(-2147483648,9),(2147483647,9)]] ++
    [make ("profile-thirds-" ++ show input) (publications !! 12) [fromIntegral input] [] [] (Just (fromIntegral result))
    | (input,result) <- [(-4,0),(-3,-100),(-2,-66),(-1,-33),(0,0),(1,33),(2,66),(3,100),(4,133),(5,166),(6,200),(7,233),(8,266),(9,300),(10,0)] :: [(Int,Int)]] ++
    [make ("profile-selector-" ++ show publicationIndex ++ "-" ++ show input) (publications !! publicationIndex) [input] [] [] (Just result)
    | publicationIndex <- [13,14], (input,result) <- [(0,0),(1,1),(0x80000000,0),(0xffffffff,0)]]
  stampThresholdCases =
    [make ("stamp-threshold-" ++ show input) (publications !! 15) [input - 50] metadataSensitivityInitial [] (Just result)
    | (input,result) <- metadataSensitivitySamples]
  calibrationDataCases =
    [make ("calibration-data-bank-" ++ show bank ++ "-base-" ++ show base) (publications !! 16) [bank]
      [wordsAt 0x0096ca50 [base]] [] (Just (base + offset))
    | (bank,offset) <- [(0,0x76800),(1,0x11c840),(2,0x2bb8e0),(3,0x3b4940),(4,0x3b8ba8),(5,0x3c31b0),(6,0x76500),(7,0x76600),(8,0x76700),(9,0x76800),(0xffffffff,0x76800)]
    , base <- [0x01700000,0xffff0000]]
  inactiveTripletCases =
    [(make ("output-triplets-inactive-" ++ show mode) (publications !! 17) [0x009a0000,0]
      (priorityInitial 0x45 ++ [wordsAt 0x009a2748 [0],wordsAt 0x009a04a8 [mode],wordsAt 0x009a220c [0],wordsAt 0x009a066c [0x12345678]])
      [wordsAt 0x009a066c [0x12345678]] (Just 1)) { stageCalls = currentTaskCalls 0x45 }
    | mode <- [0,1,3,7,0xffffffff]]
  tuningCases =
    [(make ("triplet-tuning-copy-profile-" ++ show profile ++ "-flags-" ++ show flags ++ "-override-" ++ show override)
      (publications !! 8) [0x009a0000,0x009b0000,0x009b0040,0x009b0080,0x009b00c0,0x009b0200,override]
      (priorityInitial 0x45 ++ metadataRateInitial 3 0
        [(0x1c38,0x34),(0x1c39,0x12),(0x1c3a,0x78),(0x1c3b,0x56),(0x1c3c,0xbc),(0x1c3d,0x9a)
        ,(0x1cb0,0x21),(0x1cb1,0x43),(0x1cb2,0x65),(0x1cb3,0x87),(0x1cb4,0xa9),(0x1cb5,0xcb)]
       ++ [bytes 0x009a0000 (replicate 0x2800 0)
          ,wordsAt 0x009a0480 [16,8],wordsAt 0x009a04a0 [1,2],wordsAt 0x009a049c [0xffffffff]
          ,wordsAt 0x009a04b4 [1],wordsAt 0x009a0638 [7]
          ,wordsAt 0x009a1658 [10],Patch (Address 0x009a0736) (halves [profile])
          ,Patch (Address 0x009a21f8) (halves [111,222,333])
          ,Patch (Address 0x009a2224) (halves [101,202,303]),Patch (Address 0x009a071c) (halves [404,505,606])
          ,Patch (Address 0x009a19b6) (halves [17])
          ,wordsAt 0x009b0000 [0,0x42000000,7680,96,96,80,flags]
          ,wordsAt 0x009b0040 [0,0x42010000,7680,96,96,80,flags]
          ,wordsAt 0x009b0080 [0,0x42020000,7680,96,96,80,flags]
          ,wordsAt 0x009b00c0 [0,0x42030000,7680,96,96,80,flags]
          ,bytes 0x009b0200 (replicate 6 0xa5)])
      [Patch (Address 0x009b0200) (halves levels)
      ,wordsAt 0x01569778 [0]
      ,Patch (Address (0x01569778+0x1c2)) (halves [111,222,333])
      ,bytes (0x01569778+0x1c8) [10]
      ,Patch (Address (0x01569778+0x1ca)) (halves weights)
      ,wordsAt (0x01569778+0x1f8) [0x42010000,0x42020000]
      ,wordsAt (0x01569778+0x210) [phase,2]
      ,wordsAt (0x01569778+0x224) [7]
      ,Patch (Address (0x01569778+0x238)) (halves levels)
      ,Patch (Address (0x01569778+0x23e)) (halves [1])
      ,wordsAt (0x01569778+0x240) [1]
      ,wordsAt (0x01569778+0x248) [readWord image 0x00e54074]] (Just 1))
      { stageCalls = currentTaskCalls 0x45 ++ priorityCalls 0x45 12 ++ priorityCalls 0x45 6 }
    | profile <- [29,30], flags <- [0,16], override <- [0,1]
    , let levels = if override == 1 then [404,505,606] else [101,202,303]
          weights = if profile == 29 then [0x1234,0x5678,0x9abc] else [0x4321,0x8765,0xcba9]
          phase = if flags == 0 then 5 else 1]
  activeCalibrationCases =
    [(make ("output-triplets-calibration-alternate-" ++ show alternate ++ "-backup-" ++ show backup ++ "-gain-" ++ show exposure ++ "-negative-" ++ show negative)
      (publications !! 17) [0x009a0000,if alternate then 1 else 0]
      (priorityInitial 0x45 ++ Profile.profileCalibrationInitial (Address 0x01700000) ++ metadataSensitivityInitial
       ++ [Patch (Address 0x00ef5048) (B.take 124 (B.drop (0x00ef5048-0x00bf1a00) image))
          ,Patch (Address 0x00ee9dc8) (B.take 200 (B.drop (0x00ee9dc8-0x00bf1a00) image))
          ,bytes 0x009a0000 (replicate 0x3300 0),bytes 0x0156ae98 (replicate 28 0xa5)
          ,Patch (Address (0x0156ae98+22)) (halves [0xa55a])
          ,wordsAt 0x009a2748 [1],wordsAt 0x009a04a8 [if negative then 8 else 1],wordsAt 0x009a04f8 [0xfffffff7,1]
          ,wordsAt 0x009a0530 [1],bytes 0x009a065c [2],wordsAt 0x009a1654 [10,33]
          ,Patch (Address 0x009a0736) (halves [200]),Patch (Address 0x009a2244) (halves [100])
          ,wordsAt 0x009a0604 [backup],wordsAt 0x009a066c [0x12345678]
          ,Patch (Address 0x009a164c) (halves [fromIntegral exposure])
          ,wordsAt 0x009a1d3c [if negative then 0xffffffff else 0]
          ,Profile.calibrationValuesPatch (Address 0x009a2224) original])
      ([Profile.calibrationValuesPatch (Address (if alternate then 0x009a1d34 else 0x009a2224)) result
       ,wordsAt 0x0156ae98 [0,10,1,1],Profile.calibrationValuesPatch (Address (0x0156ae98+16)) original
       ,Patch (Address (0x0156ae98+22)) (halves [0xa55a]),wordsAt (0x0156ae98+24) [0]
       ,wordsAt 0x009a1658 [if alternate then 33 else 10]
       ,Patch (Address 0x009a2244) (halves [100]),bytes 0x009a270e [attenuation]]
       ++ [Profile.calibrationValuesPatch (Address 0x009a222a) result | backup == 1]
       ++ if alternate then [Profile.calibrationValuesPatch (Address 0x009a2224) original,wordsAt 0x009a066c [0x12345678]]
          else [Patch (Address 0x009a0732) (halves [100]),wordsAt 0x009a066c [8]]) (Just 1))
      { stageCalls = currentTaskCalls 0x45 ++ if alternate then [] else priorityCalls 0x45 12 ++ priorityCalls 0x45 6 }
    | alternate <- [False,True], backup <- [0,1], exposure <- [0,33,100], negative <- [False,True]
    , let original = Profile.CalibrationValues 1000 2000 3000
          factors = Profile.ProfileFactors (Profile.FactorPair 2048 512) (Profile.FactorPair 512 2048) (Profile.FactorPair 2048 512)
          calibrated = Profile.calibrateProfileValues factors original
          primary = Profile.ClampParameters (Profile.CalibrationValues 100 200 300) (Profile.CalibrationValues 32767 2000 3000) 50
          bounded = Profile.clampedValues (Profile.clampProfileValues Profile.PrimaryClamp primary calibrated)
          independent = Profile.ClampParameters (Profile.CalibrationValues 256 256 256) (Profile.CalibrationValues 32767 32767 32767) 0
          amplified = if exposure == 0 then bounded else Profile.scaleProfileValues (Profile.profileHistoryPercent image exposure) bounded
          (attenuated,attenuation) = if negative then Profile.negativeProfileValues image (-33) (Profile.CalibrationValues 100 200 300) amplified else (amplified,0)
          result = Profile.clampedValues (Profile.clampProfileValues (Profile.IndependentClamp 3) independent attenuated)]
  overlayRegistry ready =
    [wordsAt 0x0061c004 [0x009b5000],wordsAt 0x009b5000 [0x009b5100]
    ,wordsAt 0x009b5100 [0,0,0,0,1,200,2,0,0,0,ready,201,0xffffffff,0,0,0,0,0]]
  overlayCases =
    [make ("overlay-registry-" ++ show overlay) (publications !! 18) [overlay] (overlayRegistry 1) [] (Just result)
    | (overlay,result) <- [(0,0x009b5100),(2,0x009b5118),(1,0),(9,0),(0xffffffff,0)]] ++
    [make "overlay-descriptor-address" (publications !! 27) [] [] [] (Just 0x0061c000)
    ,make "overlay-group-pointer" (publications !! 28) [] (overlayRegistry 1) [] (Just 0x009b5000)] ++
    [make ("overlay-already-ready-" ++ show ready ++ "-mask-" ++ show mask) (publications !! 19) [2,mask]
      (overlayRegistry ready) [] (Just 0) | ready <- [1,2,0xffffffff], mask <- [0,0xffffffff]] ++
    [make "overlay-poll-not-ready" (publications !! 19) [2,0] (overlayRegistry 0) [] (Just 0xffffffce)
    ,make "overlay-outside-wait-domain" (publications !! 19) [4,0xffffffff] [] [] (Just 0)] ++
    [(make ("overlay-wait-" ++ show status) (publications !! 19) [2,0xffffffff]
      (overlayRegistry 0 ++ reservedInitial 201) [] (Just status))
      { stageCalls = reservedWaitCalls 201 0xffffffff status ++ if status == 0 then reservedSignalCalls 201 0 0 ++ [call 0x42188 [0x782070] 0] else [] }
    | status <- [0,0xffffffce]] ++
    [(make "overlay-channel-wait" (publications !! 20) [6]
      (reservedInitial 201 ++ [wordsAt (0x00817700+6*4) [201]]) [] Nothing)
      { stageCalls = reservedWaitCalls 201 0xffffffff 0 ++ reservedSignalCalls 201 0 0 }] ++
    [make ("overlay-cancel-ready-" ++ show publicationIndex ++ "-" ++ show channel) (publications !! publicationIndex)
      [channel] [wordsAt (0x00944440+channel*4) [1]] [] (Just 0)
    | publicationIndex <- [21,22], channel <- [0..8]] ++
    [make "overlay-cancel-invalid-channel" (publications !! 22) [9] [wordsAt (0x00944440+9*4) [0]] [] (Just 0xffffffe2)] ++
    [(make "overlay-cancel-posts-zero-completion-request" (publications !! 22) [6]
      (Debug.queueInitial 0x3c ++ reservedInitial 201 ++ [wordsAt (0x00944440+6*4) [0],wordsAt (0x00817700+6*4) [201]])
      [wordsAt (Debug.queueBuffer+4) [0x3c454c06,8,0,0]] (Just 0))
      { stageCalls = currentTaskCalls 0x45 ++ Debug.queueCalls 0x3c 0x4c06 0x45 0x3c (B.replicate 8 0) Debug.QueueNormal 0
        ++ reservedWaitCalls 201 0xffffffff 0 ++ reservedSignalCalls 201 0 0 }]
  outputNotificationCases =
    [(make ("output-completion-" ++ show status ++ "-mode-" ++ show mode) (publications !! 23) [status]
      (priorityInitial 0x45 ++ [wordsAt (0x0155ccac+8) [0x45]]) [wordsAt 0x0154d960 [if status == 0 then 0 else 7]] Nothing)
      { stageCalls = notificationCalls 0x45 16 mode 0 }
    | status <- [0,1,0xffffffff], mode <- [0,1]]
  outputBypassCases =
    [make ("output-capture-option-bypass-" ++ show publicationIndex ++ "-" ++ show setting) (publications !! publicationIndex)
      (if publicationIndex == 26 then [0x009a0000,0x009b0000,0,0x009b0100,0x009b0200,0x009b0300,1]
       else [0x009a0000,0x009b0100,0x009b0000,0x009b0200,0x009b0300])
      [wordsAt 0x009134f8 [1],wordsAt 0x0096ca50 [0x01700000],bytes 0x01700080 [setting]
      ,wordsAt 0x009a3230 [0],wordsAt 0x009b0300 [0xffffffff]] [wordsAt 0x009b0300 [0]] (Just 1)
    | publicationIndex <- [25,26], setting <- [0xa5,0xa7,0xa9]]
  stampCases =
    [make "output-stamp-capture-option-three-clears-request" (publications !! 24) [0x009a0000,0x009b0000,0,0]
      [wordsAt 0x009134f8 [1],wordsAt 0x0096ca50 [0x01700000],bytes 0x01700080 [0xa9],bytes 0x0156aeb4 (replicate 32 0xa5)]
      [bytes 0x0156aeb4 (replicate 32 0)] (Just 1)] ++
    [make ("output-stamp-rendering-disabled-packed-" ++ show packed) (publications !! 24) [0x009a0000,0x009b0000,0,0]
      (metadataSensitivityInitial ++ Buffers.configurationInitial 0x01700000
        ([(0xb4,0),(0x28e0,3),(0x28e1,0),(0x28e2,0),(0x28e3,5),(0x28e4,0),(0x28e5,0)] ++ thresholdBytes)
       ++ [wordsAt 0x009134f8 [0],bytes 0x009a0000 (replicate 0x3300 0)
          ,wordsAt 0x009a0498 [16,if packed then 16 else 0],wordsAt 0x009a0648 [600]
          ,wordsAt 0x009b0000 [0,0x42000000,0,if packed then 192 else 256,0,0,0]
          ,wordsAt (0x00944440+6*4) [1]])
      [wordsAt 0x0156aeb4 [1,if packed then 0 else 1,if packed then 0x42000018 else 0x42000020,128,0,0,0,0x00e58890]] (Just 1)
    | packed <- [False,True]
    , let key index = readWord image (0x00e58bb0+index*2) `mod` 65536
          thresholdBytes = [(key 0,100),(key 1,50),(key 2,0)]]
  snapshotWords =
    [0xb,0x30,0x33,0x58,0x7d,0xa2,0xc7,0xec,0x11,0x36,0x5b,0x80,0xa5,0xefca,0x14,0x39,0x5e,0xffffff83,0xffffffa8,0xcd,0xf2,0x17,0x3c,0x61,0x86,0xab,0xd0,0xf5,0x1a,0x3f,0x64,0xffffff89,0xffffffae,0xffffffd3,0xfffffff8,0x1d,0x6742,0xffffb18c,0xfffffbd6,0x4520,0xffff8f6a,0xffffd9b4,0xfe,0x23,0x48,0x6d,0x92,0xb7,0xdc,0x1,0x26,0x4b,0x70,0xffffff95,0xffffffba,0xffffffdf,0x4,0x29,0x4e,0xffff9873,0xffffe2bd,0x2c07,0x7651,0xffffc09b,0xae5,0x2f,0x54,0x79,0x9e,0xc3,0xe8,0xd,0x32,0x57,0x7c,0xa1,0xffffffc6,0xffffffeb,0x10,0x35,0x5a,0x7f,0xffffc9a4,0x13ee,0x5d38,0xffffa782,0xfffff1cc,0x3b16,0x63,0x88,0xad,0xd2,0xf7,0x1c,0x41,0x66,0x8b,0xd5b0,0x1ffa,0x6944,0xb38e,0xfdd8,0x4722,0x916c,0xdbb6,0x2500,0x6f4a,0xb994,0x3de,0x4d28,0x9772,0xe1bc,0x2b06,0x7550,0xbf9a,0x9e4,0x532e,0x9d78,0xe7c2,0x310c,0x7b56,0xc5a0,0xfea,0x5934,0xa37e,0xedc8,0x3712,0x5c,0x5f,0xa681,0xa984,0xf0cb,0xf3ce,0x3a15,0x3d18,0x1b,0x40,0x65,0x8a,0xaf,0xd4,0xf9,0x1e,0x6843,0x8d]
  previewSnapshotCases =
    [make ("output-preview-snapshot-enabled-" ++ show enabled) (publications !! 29) [0x009a0000]
      (Buffers.configurationInitial 0x01700000
        ((0xa8,if enabled then 128 else 0) : [(0x8c00+index,fromIntegral (index*37+11)) | index <- [0..0xda]])
       ++ [wordsAt 0x009a220c [0x01c00000],wordsAt 0x009a0530 [0x12345678],wordsAt 0x009a04f0 [3]
          ,bytes (0x01c00000+0xdbf54) (replicate 600 0xa5),wordsAt (0x01c00000+0xdf280) [0xa5a5a5a5]])
      (if enabled then [wordsAt (0x01c00000+0xdbf58) ([0x53544146,0x12345678,3] ++ snapshotWords)
                       ,wordsAt (0x01c00000+0xdf280) [0x4c414146]
                       ,wordsAt (0x01c00000+0xdbf54) [0xa5a5a5a5],wordsAt (0x01c00000+0xdc1a8) [0xa5a5a5a5]]
       else [bytes (0x01c00000+0xdbf54) (replicate 600 0xa5),wordsAt (0x01c00000+0xdf280) [0xa5a5a5a5]]) Nothing
    | enabled <- [False,True]]
