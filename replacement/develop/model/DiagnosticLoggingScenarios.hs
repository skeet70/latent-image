{-# LANGUAGE GHC2021 #-}
module DiagnosticLoggingScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Word
import PipelineModel
import Scenarios (Address(..),Patch(..),Call,word,wordsAt,params)
import BuffersScenarios (criticalInitial,criticalCalls,criticalEnterCalls,criticalLeaveCalls)

data CalendarFields = CalendarFields { calendarYear :: Word8,calendarMonth :: Word8,calendarDay :: Word8,calendarHour :: Word8,calendarMinute :: Word8,calendarSecond :: Word8 } deriving (Eq,Show)
calendarBytes :: CalendarFields -> B.ByteString
calendarBytes (CalendarFields y m d h n s)=B.pack [y,m,d,h,n,s]
calendarPatch :: Word32 -> CalendarFields -> Patch
calendarPatch address = Patch (Address address) . calendarBytes

monthDays :: Word8 -> Word8 -> Word8
monthDays year month | month==2 && year .&. 3==0 =29
                     | otherwise=[31,28,31,30,31,30,31,31,30,31,30,31]!!(fromIntegral month-1)
adjustCalendar :: Bool -> Word8 -> Word8 -> CalendarFields -> CalendarFields
adjustCalendar backwards hours minutes (CalendarFields year month day hour minute second)
 | not backwards = CalendarFields year' month' day' hour' minute' second
 | otherwise = CalendarFields backYear backMonth backDay backHour backMinute second
 where
  minutesSum=minute+minutes
  minute'=if minutesSum>59 then minutesSum-60 else minutesSum
  hoursSum=hour+hours+(if minutesSum>59 then 1 else 0)
  hour'=if hoursSum>23 then hoursSum-24 else hoursSum
  daySum=day+(if hoursSum>23 then 1 else 0)
  rolled=daySum>monthDays year month
  day'=if rolled then 1 else daySum
  monthSum=month+(if rolled then 1 else 0)
  month'=if monthSum>12 then 1 else monthSum
  yearSum=year+(if monthSum>12 then 1 else 0)
  year'=if yearSum>50 then 0 else yearSum
  borrow=if minute<minutes then 1 else 0
  backMinute=minute+(if borrow==1 then 60 else 0)-minutes
  previousDay=hour<hours+borrow
  backHour=hour+(if previousDay then 24 else 0)-hours-borrow
  previousMonth=previousDay && day==1
  previousYear=previousMonth && month==1
  backYear=if previousYear then (if year==0 then 50 else year-1) else year
  backMonth=if previousMonth then (if month==1 then 12 else month-1) else month
  backDay | previousMonth=monthDays backYear backMonth
          | previousDay=day-1
          | otherwise=day

traceDisabledInitial :: [Patch]
traceDisabledInitial=[word 0x4089ebec 0]
traceCalls :: [Call]
traceCalls=[]
traceInitial :: Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
traceInitial buffer count timer ticks=[word 0x4089ebec 1,word 0x40a5c8b0 buffer,word (buffer+32) count,word 0xfff91000 timer,word 0x400c0800 ticks]
traceEffects :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
traceEffects buffer count timer ticks operation
 | count>=0x1540=[]
 | otherwise=[word (buffer+32) (count+1),wordsAt (buffer+36+count*12) [operation,30000-timer,ticks]]

textDisabledInitial :: [Patch]
textDisabledInitial=[word 0x40a5b5ec 0]
textInitial :: Word32 -> Word32 -> CalendarFields -> [Patch]
textInitial buffer offset fields=criticalInitial++[word 0x40a5b5ec buffer,word 0x40a5b5e4 offset,Patch (Address 0x409c2878) (B.singleton 0),calendarPatch 0x409c34b8 fields]
textCalls :: [Call]
textCalls=criticalEnterCalls++criticalCalls++criticalLeaveCalls
textEffects :: Word32 -> Word32 -> Word32 -> B.ByteString -> CalendarFields -> [Patch]
textEffects buffer offset category text calendar =
 [word (buffer+offset) 0xaaaaaaaa | wrapped]++
 [Patch (Address (buffer+start)) (B.pack [0xae,0xde,fromIntegral category,fromIntegral category,32,0,0,0]<>payload),word 0x40a5b5e4 (start+40),calendarPatch buffer calendar,word (buffer+start+40) 0x55555555]
 where
  wrapped=(offset+43) .&. complement 3 > 0x1ffc
  start=if wrapped then 8 else offset
  payload=B.take 32 (B.takeWhile (/=0) text<>B.replicate 32 0)

markerEffects :: Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
markerEffects buffer offset count marker
 | buffer==0=[]
 | otherwise=(if padding/=0 then [Patch (Address (buffer+offset)) (B.replicate (fromIntegral padding) 0),word 0x40a5b5e4 (offset+padding)] else [])++[word (buffer+offset+padding) marker]
 where padding=if count .&. 3==0 then 0 else 4-(count .&. 3)

diagnosticLoggingPublications :: [StagePublication]
diagnosticLoggingPublications=[StagePublication (Address address) (StageSymbol symbol) | (address,symbol)<-
 [(0x726658,"xm1_develop_append_stream_text"),(0x7263ec,"xm1_pipeline_image_stream_marker"),(0x69d328,"xm1_pipeline_image_calendar_read"),(0x6b199c,"xm1_pipeline_image_calendar_selected"),(0x6b1738,"xm1_pipeline_image_calendar_current"),(0x6b1770,"xm1_pipeline_image_calendar_adjusted"),(0x6a7854,"xm1_pipeline_image_clock_enabled"),(0x6a7878,"xm1_pipeline_image_clock_fields"),(0x72872c,"xm1_pipeline_image_trace_operation"),(0xeb0cbc,"xm1_pipeline_image_trace_begin"),(0xeb0ccc,"xm1_pipeline_image_trace_end"),(0xeb0ce0,"xm1_pipeline_image_trace_operation_end")]]
publication :: Word32 -> StagePublication
publication address=case filter ((==Address address).stageStockEntry) diagnosticLoggingPublications of
 p:_->p
 []->error "unknown diagnostic logging entry"

diagnosticLoggingScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
diagnosticLoggingScenarios _ _=traceCases++markerCases++textCases++calendarCases++profileCases
 where
  make name entry args initial final result calls=StageScenario (StageCaseName ("diagnostic-logging-"++name)) (publication entry) args initial final result [] calls
  buffer=params+0x1000
  calendar=CalendarFields 26 11 10 14 15 16
  traceCases=
   [make ("trace-"++show (entry,count,timer,ticks)) entry [0x6020700]
     (traceInitial buffer count timer ticks) (traceEffects buffer count timer ticks (0x6020700 .|. flag)) Nothing []
   | (entry,flag)<-[(0x72872c,0),(0xeb0cbc,0),(0xeb0ccc,0x10000000),(0xeb0ce0,0x20000000)],count<-[0,1,0x153f,0x1540],(timer,ticks)<-[(0,0),(29999,0x12345678),(30001,0xffffffff)]]++
   [make "trace-disabled" 0xeb0cbc [0x6020700] traceDisabledInitial [] Nothing []]
  markerCases=
   [make ("marker-"++show count) 0x7263ec [count,0x55555555] [word 0x40a5b5ec buffer,word 0x40a5b5e4 64,Patch (Address (buffer+64)) (B.replicate 8 0xa5)] (markerEffects buffer 64 count 0x55555555) Nothing [] | count<-[0..7]]++
   [make "marker-disabled" 0x7263ec [7,0x55555555] textDisabledInitial [] Nothing []]
  textCases=
   [make ("text-"++show (offset,category,B.length text)) 0x726658 [category,params]
      (textInitial buffer offset calendar++[Patch (Address params) (text<>B.singleton 0),Patch (Address buffer) (B.replicate 0x2100 0xa5)])
      (textEffects buffer offset category text calendar) Nothing textCalls
   | offset<-[8,8148,8152],category<-[0,0x45,0x123],text<-[B.empty,B.pack [65,66,67],B.replicate 31 65,B.replicate 32 66,B.replicate 40 67]]++
   [make "text-disabled" 0x726658 [0x45,0] textDisabledInitial [] Nothing []]
  calendarCases=
   [make ("calendar-"++show (entry,enabled,backwards,hours,minutes,fields)) entry [params]
      (criticalInitial++[calendarPatch 0x409c34b8 fields,Patch (Address 0x409c2878) (B.singleton (if enabled then 0x80 else 0)),Patch (Address 0x409c2877) (B.singleton (if backwards then 0x80 else 0)),Patch (Address 0x409c2844) (B.singleton hours),Patch (Address 0x409c283c) (B.singleton ((minutes `div` 15) `shiftL` 6))])
      [calendarPatch params (if entry==0x6b1770 || (entry/=0x6b1738 && enabled) then adjustCalendar backwards hours minutes fields else fields)]
      (if entry==0x69d328 then Nothing else Just 0) criticalCalls
   | entry<-[0x69d328,0x6b199c,0x6b1738,0x6b1770],enabled<-[False,True],backwards<-[False,True],(hours,minutes)<-[(0,0),(1,15),(23,45)],fields<-[calendar,CalendarFields 24 2 29 23 50 59,CalendarFields 0 1 1 0 0 0,CalendarFields 50 12 31 23 50 0]]
  profileCases=
   [make ("clock-fields-"++show (direction,hours,quarters)) 0x6a7878 [params,params+4,params+8]
      [Patch (Address 0x409c2877) (B.singleton direction),Patch (Address 0x409c2844) (B.singleton hours),Patch (Address 0x409c283c) (B.singleton (quarters `shiftL` 6))]
      [wordsAt params [fromIntegral (direction `shiftR` 7),if hours .&. 31>23 then 0 else fromIntegral (hours .&. 31),fromIntegral quarters*15]] Nothing []
   | direction<-[0,0x80,0xff],hours<-[0,23,24,31,255],quarters<-[0..3]]++
   [make ("clock-enabled-"++show value) 0x6a7854 [] [Patch (Address 0x409c2878) (B.singleton value)] [] (Just (fromIntegral (value `shiftR` 7))) [] | value<-[0,1,127,128,255]]
