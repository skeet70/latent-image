{-# LANGUAGE GHC2021 #-}
module DateOverlayScenarios
  ( dateOverlayPublications
  , dateOverlayScenarios
  , packedStampEffects
  , dateOverlayDisabledCalls
  , DateOrder (..)
  , DateContent (..)
  , DateTextExample (..)
  , dateTextExamples
  ) where

import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word16, Word32)
import PipelineModel
import Scenarios
import qualified Emulation.NativeObservation as N
import ImagePlanesScenarios (priorityInitial, priorityCalls, currentTaskCalls, notificationCalls)
import DebugStorageScenarios (QueueMode (..), queueInitial, queueCalls, queuePartialCalls, queueBuffer)
import BuffersScenarios (criticalInitial, criticalEnterCalls, criticalLeaveCalls)
import DiagnosticLoggingScenarios (CalendarFields (..), textInitial, textDisabledInitial, textEffects, textCalls)

data DateOrder = YearMonthDay | MonthDayYear | DayMonthYear deriving (Eq, Show)
data DateContent = DateOnly | DateAndTime deriving (Eq, Show)
data DateTextExample = DateTextExample
  { dateOrder :: DateOrder
  , dateContent :: DateContent
  , dateTimestamp :: B.ByteString
  , dateExpectedUnits :: [Word16]
  } deriving (Eq, Show)

dateTextExamples :: [DateTextExample]
dateTextExamples =
  [DateTextExample order content (C.pack timestamp) (take 18 (map (fromIntegral . fromEnum) rendered ++ repeat 0))
  | (timestamp,order,dateOnly,dateAndTime) <-
    [("2026/03/04 05:06",YearMonthDay,"2026. 3. 4","2026. 3. 4   5:06")
    ,("2026/03/04 05:06",MonthDayYear," 3/ 4/2026"," 3/ 4/2026   5:06")
    ,("2026/03/04 05:06",DayMonthYear," 4. 3.2026"," 4. 3.2026   5:06")
    ,("1999-12-31 23:59",YearMonthDay,"1999.12.31","1999.12.31  23:59")
    ,("1999-12-31 23:59",MonthDayYear,"12/31/1999","12/31/1999  23:59")
    ,("1999-12-31 23:59",DayMonthYear,"31.12.1999","31.12.1999  23:59")]
  , (content,rendered) <- [(DateOnly,dateOnly),(DateAndTime,dateAndTime)]]
  ++ [DateTextExample YearMonthDay DateOnly
       (B.pack [128,48,50,54,47,48,51,47,48,52,32,48,53,58,48,54])
       [0xff80,48,50,54,46,32,51,46,32,52,0,0,0,0,0,0,0,0]]

publication :: Word32 -> String -> StagePublication
publication address suffix = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ suffix))

dateOverlayPublications :: [StagePublication]
dateOverlayPublications =
  [ publication 0xe51490 "transform_output"
  , publication 0xe50ea4 "prepare_text"
  , publication 0xe510b8 "apply_stamp"
  , publication 0xebdd68 "stamp_rectangle"
  , publication 0x6e1884 "stamp_packed"
  , publication 0x1163188 "queue_text"
  , publication 0x11631f0 "publish_text"
  , publication 0x1113a9c "measure_text"
  , publication 0xe4e568 "date_notify"
  , publication 0x723ce8 "date_delay"
  , publication 0xdb6804 "date_post"
  , publication 0xdb6464 "date_send"
  , publication 0x117e9c4 "font_configure"
  , publication 0x117e8e0 "font_metrics"
  , publication 0x117e734 "font_rasterize"
  , publication 0x1180c18 "font_set_option"
  , publication 0x117f948 "blend_glyph"
  , publication 0x117f368 "font_query"
  , publication 0x118d984 "font_backend_option"
  , publication 0x1180b6c "font_auto_weight"
  , publication 0x1180b0c "font_spacing"
  , publication 0x1180798 "font_trailing_padding"
  , publication 0x11807dc "font_leading_padding"
  , publication 0x117e62c "font_error"
  , publication 0x632fc0 "ui_font_notify_current_task"
  , publication 0x6333d0 "ui_font_notification"
  , publication 0x633390 "init_font_notification_source"
  , publication 0x72b680 "dispatch_font_notification"
  , publication 0x72b318 "font_notification_history"
  , publication 0x72b210 "font_notification_router"
  ]

dateOverlayDisabledCalls :: [Call]
dateOverlayDisabledCalls = [call 0x42634 [0,0] 0]

packedStampEffects :: Word32 -> Word32 -> [Patch]
packedStampEffects destination rotation =
  [wordsAt destination values,wordsAt 0x95e1f8 [0,1024],wordsAt 0x9621f8 [0,1024]]
 where
  values = case rotation of
    90 -> [0xb5aa4c1d,0x4b5695ff]
    180 -> [0x76bf1dff,0x8a404c95]
    270 -> [0x4b56ff95,0xb5aa1d4c]
    _ -> [0x8a40954c,0x76bfff1d]

dateOverlayScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
dateOverlayScenarios _ _ = disabledCases ++ packedCases ++ applyCases ++ measureInvalidCases ++ publishInvalidCases ++ notificationCases ++ delayCases ++ queueCases ++ fontCases ++ fontStateCases ++ glyphCases ++ fontDiagnosticCases
 where
  state = 0x9a0000
  plane = 0x9a2000
  params = 0x9a3000
  pixels = 0x9a4000
  destination = 0x9a5000
  fontObject = 0x9a7000
  fontBackend = 0x9a9000
  fontReference = 0x9ab000
  fontWorkspace = 0x9ac000
  fontInitial capacity = [Patch (Address fontObject) (B.replicate 0x1200 0),Patch (Address fontBackend) (B.replicate 0x1100 0)
    ,word fontObject fontBackend,word (fontObject+8) fontReference,word (fontObject+12) 5
    ,word (fontReference+0x70) fontWorkspace,word (fontWorkspace+12) capacity,word (fontBackend+0xff4) 5]
  make name entry suffix arguments initial final result calls = StageScenario
    (StageCaseName name) (publication entry suffix) arguments initial final result [] calls
  disabledCases =
    [make ("date-overlay-disabled-" ++ show secondary) 0xe51490 "transform_output" [state,plane,0,pixels]
      [word (state+0x614) 0,word (state+0x61c) secondary,word (state+0x1604) 0,word (state+0x4f0) 0,wordsAt plane [0,destination,16,8,8,2,0]]
      [] (Just 1) (dateOverlayDisabledCalls ++ if secondary == 0 then [] else dateOverlayDisabledCalls)
    | secondary <- [0,1]]
  packedCases =
    [make ("date-packed-bgra-primary-colors-rotation-" ++ show rotation ++ "-cache-status-" ++ show status)
      0x6e1884 "stamp_packed" [params]
      [wordsAt params [pixels,destination,2,2,2,2,2,2,0,0,0,0,128,rotation]
      ,Patch (Address pixels) (B.pack [255,0,0,255,255,0,255,0,255,255,0,0,255,255,255,255])
      ,wordsAt destination [0x11111111,0x22222222]
      ,word (0x7d02ec+rotation*4) sine,word (0x7cfd4c+rotation*4) cosine]
      (packedStampEffects destination rotation) (Just status)
      [call 0x42634 [pixels,16] 0,call 0x4263c [destination,16] status]
    | (rotation,sine,cosine) <- [(0,0,1024),(90,1024,0),(180,0,0xfffffc00),(270,0xfffffc00,0)]
    , status <- [0,7]]
    ++ [make ("date-packed-transparent-" ++ show alpha) 0x6e1884 "stamp_packed" [params]
      [wordsAt params [pixels,destination,2,1,2,1,2,2,0,0,0,0,128,0]
      ,Patch (Address pixels) (B.pack [alpha,0,0,255,alpha,0,0,255])
      ,word destination 0x80800000,word 0x7d02ec 0,word 0x7cfd4c 1024]
      [word destination expected,wordsAt 0x95e1f8 [0,1024],word 0x9621f8 0] (Just 0)
      [call 0x42634 [pixels,8] 0,call 0x4263c [destination,8] 0]
    | (alpha,expected) <- [(0,0x80800000),(128,0xbf692626),(255,0xff554c4c)]]
  applyCases =
    [make ("date-apply-stamp-orientation-" ++ show orientation) 0xe510b8 "apply_stamp" [plane,orientation,0,0,params]
      (priorityInitial 0x45 ++
      [word 0x96ca50 0x9a6000,Patch (Address (0x9a6000+0x2dc)) (B.singleton 50)
      ,wordsAt plane [0,destination,32,8,8,4,0],wordsAt params [pixels,1,0x40004,0x20002]
      ,Patch (Address pixels) (B.pack [255,0,0,255,255,0,255,0,255,255,0,0,255,255,255,255])
      ,Patch (Address destination) (B.replicate 32 0xa5)
      ,word (0x7d02ec+rotation*4) sine,word (0x7cfd4c+rotation*4) cosine])
      [wordsAt 0x154cee8 [pixels,1,0x40004,0x20002]
      ,wordsAt 0x154cd60 [pixels,destination,0x20004,0x20002,(orientation-1)*90,left,right,top,bottom]
      ,word (destination+offset) first,word (destination+offset+8) second
      ,wordsAt 0x95e1f8 [0,1024],wordsAt 0x9621f8 [0,1024]]
      (Just 1) (currentTaskCalls 0x45 ++ priorityCalls 0x45 12
        ++ [call 0x42634 [pixels,16] 0,call 0x4263c [destination+offset,32] 7]
        ++ priorityCalls 0x45 6)
    | (orientation,rotation,sine,cosine,left,right,top,bottom,offset,first,second) <-
      [(1,0,0,1024,2,4,2,4,20,0x8a40954c,0x76bfff1d)
      ,(2,270,0xfffffc00,0,2,4,0,2,4,0x4b56ff95,0xb5aa1d4c)
      ,(3,180,0,0xfffffc00,0,2,0,2,0,0x76bf1dff,0x8a404c95)
      ,(4,90,1024,0,0,2,2,4,16,0xb5aa4c1d,0x4b5695ff)]]
  measureInvalidCases =
    [make ("date-measure-invalid-" ++ show first ++ "-" ++ show buffer) 0x1113a9c "measure_text" [params,params+0x40]
      [wordsAt params [3,3,1,first,0,buffer,0],wordsAt (params+0x40) [0xa5,0xb6,0xc7]]
      [wordsAt (params+0x40) [0xa5,0xb6,0xc7]] Nothing []
    | (first,buffer) <- [(0,pixels),(pixels,0)]]
  publishInvalidCases =
    [make ("date-publish-invalid-" ++ show width ++ "-" ++ show height) 0x11631f0 "publish_text" []
      [wordsAt 0x15e64bc [width,height,1,pixels,0,destination,0x8f0200]]
      [] Nothing [call 0x8f0200 [0,0,0] 0]
    | (width,height) <- [(0,3),(3,0),(0,0)]]
  notificationCases =
    [make "date-secondary-notification" 0xe4e568 "date_notify" [1,37,9]
      (priorityInitial 0x45 ++ [word (0x155ccac+0x14) 0x45])
      [wordsAt 0x154d964 [1,37,9]] Nothing (notificationCalls 0x45 0x10 0 0)]
  delayCases =
    [make ("date-delay-" ++ show ticks ++ "-status-" ++ show status) 0x723ce8 "date_delay" [ticks]
      (priorityInitial 0x45 ++ [word (0x9b6000+0x4c) 0x8f0220,word (0x9b6000+0x11c) 0x8f0224])
      [word 0x40a0f82c status | ticks /= 0 && status /= 0]
      (Just (if ticks == 0 then 0 else status))
      ([call 0x8f0100 [] 0,call (if ticks == 0 then 0x8f0224 else 0x8f0220) [ticks] status]
       ++ if ticks /= 0 && status /= 0 then [call 0x41a34 [9,ticks,status] 0,ScriptCall (Address 0x8f010c) [(N.R0,status)] [(N.R0,0)] [Absolute (word 0x40a0f82c status)] []] else [])
    | ticks <- [0,5,100], status <- [0,0xfffffff9]]
  queueCases =
    [make ("date-post-sequence-" ++ show sequence ++ "-previous-" ++ show previous) 0xdb6804 "date_post" [0x9000,0,0,sequence]
      (queueInitial 0x21 ++ [word 0x14e32b8 0,word 0x14e32bc previous])
      [word 0x14e32b8 (0x90000000 + if sequence == 1 then 0x9000 else 0),Patch (Address 0x14e32bc) (B.singleton (fromIntegral (if sequence == 0 then previous else next)))]
      Nothing (currentTaskCalls 0x45 ++ queueCalls 0x21 0x3100 0x45 0x21 (B.concat (map wordBytes [0x9000+next*65536,0,0])) QueueNormal 0)
    | sequence <- [0,1,2],previous <- [0,254,255]
    , let next = case sequence of { 1 -> if previous == 255 then 1 else previous+1; 2 -> previous; _ -> 0 }]
    ++ [make "date-post-duplicate-pending" 0xdb6804 "date_post" [0x9000,0,0,1]
      [word 0x14e32b8 0x12349000,word 0x14e32bc 5,Patch (Address 0x14e3338) (B.singleton 4)]
      [word 0x14e32b8 0x12349000,word 0x14e32bc 5] Nothing []]
  fontCases =
    [make "date-font-configure-owned-state" 0x117e9c4 "font_configure" [params]
      (fontInitial 12800 ++ [word 0x1665a3c fontObject,word (0x1665a38+0xf0) 0
        ,wordsAt params [6,8,1,1,0x11223344,0,0x99000000,0,0,0,0,0,0]])
      [word (fontObject+0x14) 6,word (fontObject+0x18) 8,word (fontObject+0x1c) 1,word (fontObject+0x20) 1
      ,word (fontObject+0x24) 0,word (fontObject+0x40) 1,word (fontObject+0x5c) 0x200f
      ,word (fontBackend+0xff4) 4,word (fontBackend+0x103c) 6,word (fontBackend+0x1040) 8
      ,word (fontBackend+0x1044) 1,word (fontBackend+0x1048) 1,word (fontBackend+0x105c) 1
      ,word (fontBackend+0x106c) 512,word (fontBackend+0x1070) 1
      ,wordsAt (0x1665a38+0xf4) [0x99000000,0x11223344,0]]
      (Just 1) []]
    ++ [make ("date-font-option-workspace-width-" ++ show width) 0x1180c18 "font_set_option" [fontObject,1,width]
      (fontInitial 116 ++ [word (fontObject+0x14) 5,word (fontObject+0x18) 2])
      (if width == 10 then [word (fontObject+0x14) 10,word (fontBackend+0x103c) 10,word (fontBackend+0x1070) 1] else [word (fontObject+0x14) 5,word (fontBackend+0x103c) 0])
      (Just (if width == 10 then 0 else 0xfffffff1)) [] | width <- [10,11]]
    ++ [make "date-font-option-unknown" 0x1180c18 "font_set_option" [fontObject,48,1] (fontInitial 116) [] (Just 0xfffffffc) []
       ,make "date-font-option-null" 0x1180c18 "font_set_option" [0,1,10] [] [] (Just 0xfffffffe) []
       ,make "date-font-empty-metrics" 0x117e8e0 "font_metrics" [params,params+0x40]
         [word params 0,wordsAt (params+0x40) [17,19,23],word 0x1665a3c 0]
         [wordsAt (params+0x40) [17,19,23]] (Just 1) []
       ,make "date-font-empty-raster" 0x117e734 "font_rasterize" [params,0,0]
         [word params 0,word 0x1665a3c 1] [] (Just 1) []]
  fontStateCases =
    [make ("date-font-auto-weight-" ++ show size) 0x1180c18 "font_set_option" [fontObject,3,2]
      (fontInitial 12800 ++ [word (fontObject+20) size,word (fontObject+24) size])
      [word (fontObject+0x70) 1,word (fontObject+28) mode,word (fontObject+32) edge
      ,word (fontBackend+0x1044) mode,word (fontBackend+0x1048) edge,word (fontBackend+0x1070) 1]
      (Just 0) [] | (size,mode,edge) <- [(24,3,2),(25,5,1),(32,5,1),(33,2,1)]]
    ++ [make ("date-font-spacing-workspace-" ++ show capacity) 0x1180c18 "font_set_option" [fontObject,4,2]
      (fontInitial capacity ++ [word (fontObject+20) 6,word (fontObject+24) 8])
      [word (fontObject+0x74) 1,word (fontObject+36) 4,word (fontObject+48) backendSpacing
      ,word (fontBackend+0x104c) backendSpacing] (Just 0) []
    | capacity <- [79,80],let backendSpacing = if capacity == 80 then 4 else 0]
    ++ [make ("date-font-padding-" ++ suffix) address suffix [fontObject,1]
      (fontInitial 12800 ++ [word (fontObject+24) 32,word (fontObject+36) 3])
      expected Nothing []
    | (address,suffix,expected) <-
       [(0x1180798,"font_trailing_padding",[word (fontObject+40) 0,word (fontObject+44) 2,word (fontObject+48) 3])
       ,(0x11807dc,"font_leading_padding",[word (fontObject+52) 12,word (fontObject+56) 2,word (fontObject+60) 3])]]
    ++ [make "date-font-auto-weight-direct" 0x1180b6c "font_auto_weight" [fontObject,1]
      (fontInitial 12800 ++ [word (fontObject+20) 48,word (fontObject+24) 48])
      [word (fontObject+28) 2,word (fontObject+32) 1,word (fontBackend+0x1044) 2,word (fontBackend+0x1048) 1] Nothing []
       ,make "date-font-spacing-direct" 0x1180b0c "font_spacing" [fontObject,1]
      (fontInitial 80 ++ [word (fontObject+20) 6,word (fontObject+24) 8])
      [word (fontObject+36) 4,word (fontObject+48) 4,word (fontBackend+0x104c) 4] Nothing []]
    ++ [make ("date-font-backend-" ++ show key ++ "-" ++ show value) 0x118d984 "font_backend_option" [fontBackend,key,value]
      (fontInitial 116) expected Nothing []
    | (key,value,expected) <-
       [(2,101,[word (fontBackend+0x1040) 100,word (fontBackend+0x1070) 1])
       ,(4,6,[word (fontBackend+0x1048) 5,word (fontBackend+0x1070) 1])
       ,(9,99,[word (fontBackend+0xff4) 4,word (fontBackend+0x105c) 1,word (fontBackend+0x106c) 512,word (fontBackend+0x1070) 1])
       ,(10,0xffffffff,[word (fontBackend+0x1068) 0xffffffff,word (fontBackend+0x1070) 0])
       ,(12,0,[word (fontBackend+0x1078) 4,word (fontBackend+0x1070) 0])
       ,(1,0xffffffff,[word (fontBackend+0x103c) 0,word (fontBackend+0x1070) 0])]]
    ++ [make ("date-font-backend-unknown-mode-" ++ show mode) 0x118d984 "font_backend_option" [fontBackend,99,0]
      (fontInitial 116 ++ [word (fontBackend+0xff4) mode,word (fontBackend+0x106c) 3])
      [word (fontBackend+0x106c) expected] Nothing []
    | (mode,expected) <- [(2,256),(7,512),(5,3)]]
    ++ [make ("date-font-query-invalid-" ++ show object ++ "-" ++ show count) 0x117f368 "font_query" [object,params,count,0,0,0]
      [] [] (Just result) []
    | (object,count,result) <- [(0,1,0xfffffffe),(fontObject,0,0xfffffff7),(fontObject,0xffffffff,0xfffffff7)]]
  glyphCases =
    [make ("date-text-coverage-pass-" ++ show passes ++ "-left-" ++ show left) 0x117f948 "blend_glyph" [2,left,top,0,0,params,params+0x40]
      [wordsAt params [4,1,4,0,1,0,0xff204080,0x00102030,4,pixels]
      ,wordsAt (params+0x40) [destination,fontBackend,passes,0,0],word (fontBackend+0x10d4) 4
      ,Patch (Address pixels) (B.pack [0,1,8,15]),Patch (Address destination) (B.replicate 64 0xa5)
      ,word 0x166703c 0,word 0x1667044 0]
      ([word (params+0x48) (passes+1),word 0x166703c 0x00102030,word 0x1667044 0xff204080]
       ++ [word (destination+(top*4+left)*4) (if passes == 0 then 0x30201000 else 0xa5a5a5a5)]
       ++ [word (destination+(top*4+left+1)*4) 0x3522110f
          ,word (destination+(top*4+left+2)*4) 0x5830187f]
       ++ [word (destination+12) 0x804020ff | left == 0])
      (Just 0) []
    | passes <- [0,1],(left,top) <- [(0,0),(1,1)]]
    ++ [make "date-text-negative-channel-interpolation" 0x117f948 "blend_glyph" [2,0,0,0,0,params,params+0x40]
      [wordsAt params [4,1,4,0,1,0,0x00010203,0xff102030,4,pixels]
      ,wordsAt (params+0x40) [destination,fontBackend,0,0,0],word (fontBackend+0x10d4) 4
      ,Patch (Address pixels) (B.pack [0,1,8,15]),word 0x166703c 0,word 0x1667044 0]
      [wordsAt destination [0x302010ff,0x2d1e0fef,0x1911087f,0x03020100],word (params+0x48) 1]
      (Just 0) []]
  diagnosticCalendar = CalendarFields 26 11 10 12 34 56
  diagnosticBuffer = 0x9c8000
  diagnosticLogInitial = textInitial diagnosticBuffer 8 diagnosticCalendar
  diagnosticLogEffects text = textEffects diagnosticBuffer 8 0x45 text diagnosticCalendar
  diagnosticSnapshot = C.pack "snapshot" <> B.singleton 0 <> B.replicate 83 0xa5
  diagnosticSnapshotFinal = C.pack "snapshot" <> B.singleton 0 <> B.replicate 79 0xa5 <> B.pack [0x45,0xa5,0xa5,0xa5]
  diagnosticError = C.pack "bad font" <> B.replicate 21 0 <> B.replicate 63 0xa5
  diagnosticErrorFinal = C.pack "bad font" <> B.replicate 21 0 <> B.replicate 59 0xa5 <> B.pack [0x45,0xa5,0xa5,0xa5]
  diagnosticCleared = B.singleton 0 <> B.replicate 31 0xa5 <> B.replicate 56 0 <> B.pack [0x45,0xa5,0xa5,0xa5]
  diagnosticSnapshotInitial = Patch (Address 0x95482c) (B.replicate 92 0xa5)
  diagnosticHistoryInitial =
    [word 0x829a0c 0x72b318,word 0x9134c0 0,Patch (Address 0x9121c0) (B.replicate 76 0xc3)]
  diagnosticHistoryEffects value =
    [word 0x9134c0 1,wordsAt 0x9121c0 [0x91b,value],Patch (Address 0x9121c8) (B.replicate 68 0xc3)]
  diagnosticCriticalCalls = criticalEnterCalls ++ criticalLeaveCalls
  fontDiagnosticCases = historyCases ++ uiHistoryCases ++ diagnosticInitializerCases ++ diagnosticRouterCases ++ diagnosticDropCases ++ diagnosticProducerCases
  historyCases =
    [make ("font-history-capacity-" ++ show count) 0x72b318 "font_notification_history" [params]
      (criticalInitial ++ textDisabledInitial ++ [word 0x9134c0 count,wordsAt params [0x91b,0x123]
      ,Patch (Address (0x9121c0+slot*76)) (B.replicate 76 0xc3)])
      (if count < 64 then [word 0x9134c0 (count+1),wordsAt (0x9121c0+slot*76) [0x91b,0x123]
        ,Patch (Address (0x9121c8+slot*76)) (B.replicate 68 0xc3)]
        else [word 0x9134c0 count,Patch (Address (0x9121c0+slot*76)) (B.replicate 76 0xc3)])
      (Just (if count < 64 then 0 else 0xffffffc1))
      (criticalEnterCalls ++ (if count < 64 then [] else currentTaskCalls 0x45) ++ criticalLeaveCalls)
    | count <- [0,63,64,0xffffffff], let slot = min count 63]
    ++ [make "font-history-overflow-real-stream-log" 0x72b318 "font_notification_history" [params]
      (diagnosticLogInitial ++ [word 0x9134c0 64,wordsAt params [0x91b,0x123]])
      (word 0x9134c0 64 : diagnosticLogEffects (C.pack "ERR_EVENT_FULL")) (Just 0xffffffc1)
      (criticalEnterCalls ++ currentTaskCalls 0x45 ++ textCalls ++ criticalLeaveCalls)]
  uiHistoryCases =
    [make "font-ui-null-partial-clear-history" 0x6333d0 "ui_font_notification" [0,0x42]
      (criticalInitial ++ diagnosticHistoryInitial ++ [diagnosticSnapshotInitial])
      (Patch (Address 0x95482c) diagnosticCleared : diagnosticHistoryEffects 0x42) Nothing
      (currentTaskCalls 0x45 ++ diagnosticCriticalCalls)
    ,make "font-ui-snapshot-real-stream-log-history" 0x6333d0 "ui_font_notification" [params,0x42]
      (diagnosticLogInitial ++ diagnosticHistoryInitial ++ [diagnosticSnapshotInitial,Patch (Address params) diagnosticSnapshot])
      (Patch (Address 0x95482c) diagnosticSnapshotFinal : diagnosticHistoryEffects 0x42 ++ diagnosticLogEffects (C.pack "snapshot")) Nothing
      (currentTaskCalls 0x45 ++ textCalls ++ currentTaskCalls 0x45 ++ diagnosticCriticalCalls)
    ,make "font-error-bounded-padding-real-log-history" 0x117e62c "font_error" [params]
      (diagnosticLogInitial ++ diagnosticHistoryInitial ++ [diagnosticSnapshotInitial
      ,Patch (Address 0x16659dc) (B.replicate 92 0xa5),Patch (Address params) (C.pack "bad font" <> B.singleton 0)])
      ([Patch (Address 0x16659dc) diagnosticError,Patch (Address 0x95482c) diagnosticErrorFinal]
        ++ diagnosticHistoryEffects 0 ++ diagnosticLogEffects (C.pack "bad font")) Nothing
      (currentTaskCalls 0x45 ++ textCalls ++ currentTaskCalls 0x45 ++ diagnosticCriticalCalls)
    ,make "font-error-exact29-truncation-retains-record-tail" 0x117e62c "font_error" [params]
      (diagnosticLogInitial ++ diagnosticHistoryInitial ++ [diagnosticSnapshotInitial
      ,Patch (Address 0x16659dc) (B.replicate 92 0xa5)
      ,Patch (Address params) (C.pack "012345678901234567890123456789-unread" <> B.singleton 0)])
      ([Patch (Address 0x16659dc) (C.pack "01234567890123456789012345678" <> B.replicate 63 0xa5)
       ,Patch (Address 0x95482c) (C.pack "01234567890123456789012345678" <> B.replicate 59 0xa5 <> B.pack [0x45,0xa5,0xa5,0xa5])]
        ++ diagnosticHistoryEffects 0
        ++ diagnosticLogEffects (C.pack "01234567890123456789012345678" <> B.replicate 3 0xa5)) Nothing
      (currentTaskCalls 0x45 ++ textCalls ++ currentTaskCalls 0x45 ++ diagnosticCriticalCalls)
    ,make "font-notify-current-task-low-byte-real-log" 0x632fc0 "ui_font_notify_current_task" [params]
      (diagnosticLogInitial ++ [Patch (Address params) (C.pack "snapshot" <> B.singleton 0)])
      (diagnosticLogEffects (C.pack "snapshot")) Nothing (currentTaskCalls 0x12345 ++ textCalls)
    ,make "font-notify-disabled-log-does-not-read-null" 0x632fc0 "ui_font_notify_current_task" [0]
      textDisabledInitial [] Nothing (currentTaskCalls 0x45)]
  diagnosticInitializerCases =
    [make ("font-source-initialize-byte-" ++ show initial) 0x633390 "init_font_notification_source" []
      (priorityInitial 0x45 ++ [word 0x954884 initial])
      [word 0x954884 (if initial == 0xaabbcc00 then 0xaabbcc45 else initial)] Nothing
      (if initial == 0xaabbcc00 then currentTaskCalls 0x12345 else [])
    | initial <- [0xaabbcc00,0xaabbcc99]]
  diagnosticRouterCases =
    [make ("font-router-" ++ suffix ++ "-" ++ show (event,loadtest,status,initialTask)) entry suffix [params]
      (queueInitial channel ++ [word 0x829a0c 0x72b210,Patch (Address params) packet,word 0x9134f8 loadtest
      ,word 0x954884 (0xaabbcc00+initialTask),word 0x9121bc counter,word 0x9134c4 counter])
      ([word 0x9121bc nextCounter,word 0x954884 (0xaabbcc00+finalTask)]
        ++ if status == 0 then [] else [word 0x40a0f82c status]) (Just status)
      (currentTaskCalls 0x45 ++ queuePartialCalls channel 0 0x45 (fromIntegral channel) 76
        [Absolute (Patch (Address (queueBuffer+12)) packet),Absolute (word 0x9121bc nextCounter)] mode status
        ++ if event == 0x91b && initialTask == 0 then currentTaskCalls 0x45 else [])
    | (entry,suffix) <- [(0x72b210,"font_notification_router"),(0x72b680,"dispatch_font_notification")]
    , (event,loadtest,mode,status,initialTask) <-
      [(0x91b,0,QueueNormal,0,0),(0x91b,1,QueueInterrupt,0,0),(0x91b,1,QueueNormal,0xfffffffb,0x99)
      ,(0x82e,0,QueueNormal,0,0),(0x82e,1,QueueInterrupt,0,0),(0x777,2,QueueNormal,0,0)]
    , let channel = if loadtest == 1 && event .&. 0x900 /= 0 then 7 else 1
          packet = wordBytes event <> wordBytes 0x123 <> B.pack [1..68]
          counter = if event == 0x82e && channel == 1 then 0xffffffff else 3
          nextCounter = if event == 0x82e && channel == 1 then 0 else counter
          finalTask = if event == 0x91b && initialTask == 0 then 0x45 else initialTask]
  diagnosticDropCases =
    [make ("font-router-update-counter-drop-" ++ suffix) entry suffix [params]
      (priorityInitial 0x45 ++ [word 0x829a0c 0x72b210,wordsAt params [0x82e,0x123],word 0x9134f8 0
      ,word 0x9121bc 3,word 0x9134c4 4,word 0x954884 0xaabbcc00])
      [word 0x9121bc 3,word 0x954884 0xaabbcc00] (Just 0)
      (currentTaskCalls 0x45 ++ [call 0x42188 [0x782048] 0])
    | (entry,suffix) <- [(0x72b210,"font_notification_router"),(0x72b680,"dispatch_font_notification")]]
  diagnosticProducerCases =
    [make ("font-ui-queue-producer-header-law-" ++ show (loadtest,status)) 0x6333d0 "ui_font_notification" [0,0x42]
      (queueInitial channel ++ [word 0x829a0c 0x72b210,word 0x9134f8 loadtest,diagnosticSnapshotInitial])
      (Patch (Address 0x95482c) diagnosticCleared : if status == 0 then [] else [word 0x40a0f82c status]) Nothing
      (currentTaskCalls 0x45 ++ currentTaskCalls 0x45
        ++ queuePartialCalls channel 0 0x45 (fromIntegral channel) 76
          [Absolute (wordsAt (queueBuffer+12) [0x91b,0x42])] QueueNormal status)
    | loadtest <- [0,1],status <- [0,0xfffffffb],let channel = if loadtest == 1 then 7 else 1]

call :: Word32 -> [Word32] -> Word32 -> Call
call address arguments result = ScriptCall (Address address) (zip [N.R0,N.R1,N.R2,N.R3] arguments) [(N.R0,result)] [] []

word :: Word32 -> Word32 -> Patch
word address value = Patch (Address address) (wordBytes value)

wordsAt :: Word32 -> [Word32] -> Patch
wordsAt address values = Patch (Address address) (B.concat (map wordBytes values))

wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral ((value `div` (1 `shiftL` shift)) .&. 255) | shift <- [0,8,16,24]]
