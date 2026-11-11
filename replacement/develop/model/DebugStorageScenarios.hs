{-# LANGUAGE GHC2021 #-}
module DebugStorageScenarios
  ( debugStoragePublications, debugStorageScenarios
  , QueueMode (..), queueInitial, queueCalls, queuePartialCalls, queueBuffer
  , receiveInitial, receiveCalls
  ) where

import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word16, Word32, Word8)
import qualified Emulation.NativeObservation as N
import ImagePlanesScenarios (priorityInitial, currentTaskCalls, waitCalls, notificationCalls, reservedInitial, reservedWaitCalls, reservedSignalCalls)
import WarpEngineScenarios (diagnosticEventInitial, diagnosticEventCalls, diagnosticEventEffects)
import PipelineModel
import Scenarios (Address (..), Patch (..), Call (..), MemoryContract (..), word, wordsAt, wordBytes, call)

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name))

debugStoragePublications :: [StagePublication]
debugStoragePublications = [publication address name | (address,name) <-
  [(0xe53d00,"write_debug_file"),(0x70fbfc,"prepare_debug_basename")
  ,(0x70eaa0,"storage_decimal"),(0x722218,"storage_decimal")
  ,(0x70f840,"storage_validate_name"),(0x71ddf0,"storage_ready")
  ,(0x715f0c,"storage_group"),(0x70ec08,"storage_drive_for_device")
  ,(0x71fb18,"storage_current_drive"),(0x71f708,"storage_current_drive")
  ,(0x712d08,"storage_drive_prefix"),(0x6a7f28,"storage_counter")
  ,(0x6a7fd8,"storage_directory"),(0x716324,"storage_choose_pair")
  ,(0x70fb68,"storage_read_numbers"),(0x71ef18,"storage_token")
  ,(0x71c648,"debug_open"),(0x71c75c,"debug_write"),(0x71c7bc,"debug_entry")
  ,(0x71fde4,"storage_submit"),(0x72503c,"send_message")
  ,(0x725318,"receive_message")
  ,(0x723398,"storage_dispatch_error"),(0xe4e528,"debug_open_complete")
  ,(0xe4e4f4,"debug_write_complete"),(0xe4e4c0,"debug_entry_complete")]]

halfBytes :: Word16 -> B.ByteString
halfBytes value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
half :: Word32 -> Word16 -> Patch
half address value = Patch (Address address) (halfBytes value)
byte :: Word32 -> Word8 -> Patch
byte address value = Patch (Address address) (B.singleton value)
text :: Word32 -> String -> Patch
text address value = Patch (Address address) (C.pack value <> B.singleton 0)

storage, output, directoryName, filenamePrefix :: Word32
storage = 0x9a0000
output = 0x9d0000
directoryName = 0x9d0100
filenamePrefix = 0x9d0120

namingInitial :: Word16 -> Word16 -> Word16 -> Word8 -> Word32 -> [Patch]
namingInitial directory number sequenceValue numberingMode prefixMode =
  [word 0x910204 storage,word (storage+0x114) 0,word (storage+0x16bd8) 0
  ,word (storage+0x2210) 1,byte (storage+0x2600) 1
  ,half (storage+0x21d4) directory,half (storage+0x21d6) number
  ,word (storage+0x2168) (fromIntegral directory),word (storage+0x21b8) (fromIntegral directory)
  ,word (storage+0x2794) 0,word (storage+0x16c6c) 0
  ,half (storage+0x127d4) sequenceValue,byte (storage+0x16bd0) numberingMode
  ,word (storage+0x16bd4) prefixMode,Patch (Address storage) (C.pack "DSCF")
  ,Patch (Address (storage+0x21da)) (C.pack "FUJI_")
  ,Patch (Address 0x814730) (B.pack [65,1,0,0]),Patch (Address 0x814738) (B.pack [66,1,1,0])
  ,Patch (Address 0x814740) (B.pack [67,0,2,0])
  ,Patch (Address output) (B.replicate 64 0xa5)]

data QueueMode = QueueNormal | QueueInterrupt deriving (Eq, Show)

queueBuffer :: Word32
queueBuffer = 0x9dc000

queueInitial :: Word32 -> [Patch]
queueInitial channel = priorityInitial 0x45 ++
  [word (0x9b6000+0x170) 0x8f0204,word (0x9b6000+0xac) 0x8f0208
  ,word (0x9b6000+0xb0) 0x8f020c,word (0x9b6000+0xf8) 0x8f0210
  ,word 0x4089ea94 0x8f0200,word (0x7cad9c+((channel .&. 0xffff)-1)*4) 4
  ,word (0x40a0fa4c+channel*4) 0x100,word queueBuffer 0xdecafbad]

queueCalls :: Word32 -> Word16 -> Word8 -> Word8 -> B.ByteString -> QueueMode -> Word32 -> [Call]
queueCalls channel event sender destination payload = queuePartialCalls channel event sender destination
  (fromIntegral (B.length payload)) [Absolute (Patch (Address (queueBuffer+12)) payload)]

queuePartialCalls :: Word32 -> Word16 -> Word8 -> Word8 -> Word32 -> [MemoryContract] -> QueueMode -> Word32 -> [Call]
queuePartialCalls channel event sender destination lengthBytes expected mode status =
  [call 0x8f0100 [] 0
  ,ScriptCall (Address 0x8f0200) [] [(N.R0,0)] [Relative N.R0 0 header] []
  ,ScriptCall (Address 0x8f0204) [(N.R0,tag)] [(N.R0,0)] [] [Relative N.R1 0 (wordBytes queueBuffer)]
  ,call 0x8f0114 [] (if mode == QueueInterrupt then 1 else 0)
  ,ScriptCall (Address (if mode == QueueInterrupt then 0x8f020c else 0x8f0208)) [(N.R0,tag),(N.R1,queueBuffer)] [(N.R0,status)]
      ([Absolute (word queueBuffer 0xdecafbad),Absolute (Patch (Address (queueBuffer+4)) header)]++expected) []]
  ++ if status == 0 then [] else
      [call 0x41a34 [0x1c,channel,status] 0,call 0x8f0210 [tag,queueBuffer] 0
      ,ScriptCall (Address 0x8f010c) [(N.R0,status)] [(N.R0,0)] [Absolute (word 0x40a0f82c status)] []]
 where
  tag = (channel .&. 0xffff) .|. 0x40000
  header = halfBytes event <> B.pack [sender,destination] <> wordBytes lengthBytes

receiveInitial :: Word32 -> [Patch]
receiveInitial channel = queueInitial channel ++ [word (0x9b6000+0xc8) 0x8f0218]

receiveCalls :: Word32 -> Word32 -> Word32 -> Word32 -> Word16 -> Word8 -> Word8 -> B.ByteString -> Word32 -> [Call]
receiveCalls channel timeout message destination event sender category payload status =
  [call 0x8f0100 [] 0
  ,ScriptCall (Address 0x8f0218) [(N.R0,tag),(N.R2,timeout)] [(N.R0,status)] []
      (if status == 0 then [Relative N.R1 0 (wordBytes queueBuffer),Absolute (Patch (Address (queueBuffer+4)) (header<>payload))] else [])]
  ++ if status == 0 then
      [ScriptCall (Address 0x8f0210) [(N.R0,tag),(N.R1,queueBuffer)] [(N.R0,0)]
        [Absolute (Patch (Address message) header),Absolute (word (message+8) destination),Absolute (Patch (Address destination) payload)] []]
     else if status == 0xffffffce then [] else
      [call 0x41a34 [0x1f,channel,status] 0
      ,ScriptCall (Address 0x8f010c) [(N.R0,status)] [(N.R0,0)] [Absolute (word 0x40a0f82c status)] []]
 where
  tag = (channel .&. 0xffff) .|. 0x40000
  header = halfBytes event <> B.pack [sender,category] <> wordBytes (fromIntegral (B.length payload))

debugStorageScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
debugStorageScenarios _ _resolve = namingCases ++ decimalCases ++ validationCases ++ routeCases ++ counterCases ++ readyCases ++ callbackCases ++ queueCases ++ receiveCases ++ requestCases ++ tokenCases ++ debugErrorCases
 where
  make name address symbol arguments initial final returned pointers calls = StageScenario
    (StageCaseName ("debug-storage-"++name)) (publication address symbol) arguments initial final returned pointers calls
  namingCases =
    [make name 0x70fbfc "prepare_debug_basename" [output,0,0]
      (namingInitial directory number sequenceValue mode 0)
      ([text output expected,half (storage+0x127d4) finalSequence,byte (storage+0x16bd0) finalMode])
      (Just 0) [] []
    | (name,directory,number,sequenceValue,mode,expected,finalSequence,finalMode) <-
      [("ordinary",100,41,7,0,"A:\\DCIM\\100FUJI_\\DSCF0042",7,0)
      ,("thousand-rollover",100,999,7,0,"A:\\DCIM\\101FUJI_\\DSCF1001",1,0)
      ,("group-digit-ahead",100,2000,7,0,"A:\\DCIM\\102FUJI_\\DSCF2001",1,0)
      ,("group-digit-behind",102,1,7,0,"A:\\DCIM\\102FUJI_\\DSCF2001",7,0)
      ,("mode-two-first-increment",100,41,98,2,"A:\\DCIM\\100FUJI_\\DSCF0042",99,1)
      ,("sequence-hundred-rollover",100,41,99,2,"A:\\DCIM\\101FUJI_\\DSCF1001",1,1)
      ,("sequence-halfword-wrap",100,41,65535,2,"A:\\DCIM\\100FUJI_\\DSCF0042",0,1)]]
    ++ [make ("custom-name-"++show valid) 0x70fbfc "prepare_debug_basename" [output,directoryName,filenamePrefix]
      (namingInitial 100 41 7 0 0 ++ [text directoryName (if valid then "ABCDE" else "AB-DE"),text filenamePrefix (if valid then "TEST" else "!BAD")])
      [text output (if valid then "A:\\DCIM\\100ABCDE\\TEST0042" else "A:\\DCIM\\100FUJI_\\DSCF0042")]
      (Just 0) [] [] | valid <- [False,True]]
    ++ [make "missing-context-preserves-name" 0x70fbfc "prepare_debug_basename" [output,0,0]
      [word 0x910204 0,text output "old-name"] [text output "old-name"] (Just 5) [] []]
    ++ [make ("configured-prefix-"++show mode) 0x70fbfc "prepare_debug_basename" [output,0,0]
      (namingInitial 100 41 7 0 mode++[word 0x96ca50 0x9e0000,Patch (Address 0x9e0142) (C.pack "ABCD")])
      [text output expected] (Just 0) [] []
      | (mode,expected) <- [(1,"A:\\DCIM\\100FUJI_\\AB070042"),(2,"A:\\DCIM\\100FUJI_\\CD070042")]]
    ++ [make "directory-limit-error-preserves-output" 0x70fbfc "prepare_debug_basename" [output,0,0]
      (namingInitial 999 9999 7 0 0++diagnosticEventInitial)
      (Patch (Address output) (B.replicate 64 0xa5):diagnosticEventEffects 2 0x51)
      (Just 6) [] (diagnosticEventCalls 2 0x51)]
  decimalCases =
    [make ("decimal-"++show value++"-"++show width++"-"++show flags) 0x70eaa0 "storage_decimal" [output,value,width,flags]
      [Patch (Address output) (B.replicate 16 0xa5)] [Patch (Address output) expected] (Just result) [] []
    | (value,width,flags,expected,result) <-
      [(0,4,1,C.pack "0000",0),(42,4,1,C.pack "0042",2),(42,4,0,C.pack "0042\0",2)
      ,(42,4,2,C.pack "  42\0",2),(42,4,4,C.pack "42\0",2)
      ,(257,1,4,B.replicate 4 0xa5,0xffffffff),(9999,4,1,C.pack "9999",4)]]
  validationCases =
    [make ("validate-"++show index) 0x70f840 "storage_validate_name" [output,fromIntegral (length name)]
      [text output name] [] (Just result) [] []
    | (index,name,result) <- ([(0,"AZaz09_",0),(1,"",0),(2,"a-b",8),(3,"a b",8),(4,"a/b",8)] :: [(Int,String,Word32)])]
  routeCases =
    [make ("drive-"++show device) 0x70ec08 "storage_drive_for_device" [device]
      (namingInitial 100 41 7 0 0) [] (Just expected) [] [] | (device,expected) <- [(0,65),(1,66),(2,67),(0xffffffff,67)]]
  counterCases =
    [make ("counter-"++show count) 0x6a7f28 "storage_counter" [] [half 0x409c281c count] [] (Just expected) [] []
      | (count,expected) <- [(0,0),(9999,9999),(10000,0),(65535,0)]]
    ++ [make ("directory-"++show count) 0x6a7fd8 "storage_directory" [] [half 0x409c281e count] [] (Just expected) [] []
      | (count,expected) <- [(0,100),(99,100),(100,100),(999,999),(1000,100),(65535,100)]]
  readyCases =
    [make ("readiness-"++show flags++"-mode-"++show mode) 0x71ddf0 "storage_ready" [mode]
      (namingInitial 100 41 7 0 0++diagnosticEventInitial++[byte (storage+0x16bd8) flags])
      (if code == 0 then [] else diagnosticEventEffects 2 code) (Just result) []
      (if code == 0 then [] else diagnosticEventCalls 2 code)
      | (flags,mode,result,code) <- [(0,0,0,0),(1,0,1,0x50),(2,0,5,0x55),(2,1,5,0x55),(2,2,0,0),(6,2,0,0),(4,0,5,0x53),(8,0,2,0x52),(8,1,0,0),(15,2,1,0x50)]]
  callbackCases =
    [make ("open-callback-status-"++show status++"-task-"++show task) 0xe4e528 "debug_open_complete" [status,0x12345678]
      (priorityInitial task++[word 0x154d898 task,word 0x154c744 0xa5a5a5a5])
      [word 0x154c744 0x12345678,word 0x154d898 task] Nothing []
      (if task == 0 then [] else notificationCalls task 0x10 0 0)
    | status <- [0,5,11,0xffffffff],task <- [0,0x45]]
    ++ [make ("completion-"++name++"-status-"++show status) address name [status,0x321]
      (priorityInitial 0x45++[word 0x154d898 0x45,word 0x154c744 0x12345678])
      [word 0x154c744 0x12345678,word 0x154d898 0x45] Nothing [] (notificationCalls 0x45 0x10 0 0)
      | (address,name) <- [(0xe4e4f4,"debug_write_complete"),(0xe4e4c0,"debug_entry_complete")],status <- [0,5,11,0xffffffff]]
  queueCases =
    [make ("queue-"++show mode++"-status-"++show status) 0x72503c "send_message" [0x65,output]
      (queueInitial 0x65++[Patch (Address output) (halfBytes 0x2c19<>B.pack [0x45,0x65]<>wordBytes 8<>wordBytes (output+0x100)),wordsAt (output+0x100) [0x12345678,0x87654321]])
      ([word queueBuffer 0xdecafbad]++[word 0x40a0f82c status | status /= 0]) (Just status) []
      (queueCalls 0x65 0x2c19 0x45 0x65 (wordBytes 0x12345678<>wordBytes 0x87654321) mode status)
    | mode <- [QueueNormal,QueueInterrupt],status <- [0,0xfffffff9]]
    ++ [make ("queue-allocation-error-"++show allocationStatus) 0x72503c "send_message" [0x65,output]
      (queueInitial 0x65++[Patch (Address output) (halfBytes 0x2c19<>B.pack [0x45,0x65]<>wordBytes 0<>wordBytes 0)])
      [word 0x40a0f82c expected,word queueBuffer 0xdecafbad] (Just expected) []
      [call 0x8f0100 [] 0,call 0x8f0200 [] 0,call 0x8f0204 [0x40065] allocationStatus
      ,call 0x41a34 [0x1c,0x65,expected] 0
      ,ScriptCall (Address 0x8f010c) [(N.R0,expected)] [(N.R0,0)] [Absolute (word 0x40a0f82c expected)] []]
      | (allocationStatus,expected) <- [(0xffffffce,0xffffffc2),(0xfffffff9,0xfffffff9)]]
    ++ [make "queue-oversize-does-not-allocate" 0x72503c "send_message" [0x65,output]
      (queueInitial 0x65++[wordsAt output [0x65452c19,249,0]])
      [word 0x40a0f82c 0xffffffc4,word queueBuffer 0xdecafbad] (Just 0xffffffc4) []
      [call 0x8f0100 [] 0,call 0x41a34 [0x1c,0x65,0xffffffc4] 0
      ,ScriptCall (Address 0x8f010c) [(N.R0,0xffffffc4)] [(N.R0,0)] [Absolute (word 0x40a0f82c 0xffffffc4)] []]]
  requestCases =
    [make ("write-request-"++show mode++"-status-"++show status) 0x71c75c "debug_write" [0,0x12345678,4,output]
      (namingInitial 100 41 7 0 0++queueInitial 0x65)
      [word 0x40a0f82c status | status /= 0] Nothing []
      (currentTaskCalls 0x45++currentTaskCalls 0x45++queueCalls 0x65 0x2c19 0x45 0x65
        (B.concat (map wordBytes [0,0x12345678,4,output,0,0,0])) mode status)
      | mode <- [QueueNormal,QueueInterrupt],status <- [0,0xfffffff9]]
    ++ [make "entry-request" 0x71c7bc "debug_entry" [0,0x12345678,0,0]
      (namingInitial 100 41 7 0 0++queueInitial 0x65++[word 0x8fc8a4 0]) [] Nothing []
      (currentTaskCalls 0x45++currentTaskCalls 0x45++queuePartialCalls 0x65 0x2c1a 0x45 0x65 0x28
       [Absolute (wordsAt (queueBuffer+12) [0,0x12345678,0]),Absolute (byte (queueBuffer+24) 0)
       ,Absolute (wordsAt (queueBuffer+44) [0,0])] QueueNormal 0)]
    ++ [make "open-request" 0x71c648 "debug_open" [0,output,1]
      (namingInitial 100 41 7 0 0++reservedInitial 0x3c++queueInitial 0x65
       ++[word 0x8fc8a4 0,wordsAt 0x9101c0 (replicate 16 0),word 0x910200 0,text output "A:\\DCIM\\100FUJI_\\DSCF0042.RAW"])
      [word 0x9101c0 1,word 0x910200 1] Nothing []
      (currentTaskCalls 0x45++reservedWaitCalls 0x3c 0xffffffff 0++reservedSignalCalls 0x3c 0 0
       ++currentTaskCalls 0x45++queuePartialCalls 0x65 0x2c17 0x45 0x65 0x50
        [Absolute (word (queueBuffer+12) 0),Absolute (text (queueBuffer+16) "A:\\DCIM\\100FUJI_\\DSCF0042.RAW")
        ,Absolute (wordsAt (queueBuffer+64) [0,1,1,0,0,0,1])] QueueNormal 0)]
    ++ [make "write-preserves-external-completion" 0x71c75c "debug_write"
      [0x8f0300,0x12345678,4,output]
      (namingInitial 100 41 7 0 0++queueInitial 0x65) [] Nothing []
      (currentTaskCalls 0x45++currentTaskCalls 0x45++queueCalls 0x65 0x2c19 0x45 0x65
        (B.concat (map wordBytes [0x8f0300,0x12345678,4,output,0,0,0])) QueueNormal 0)]
  tokenCases =
    [make ("token-"++name) 0x71ef18 "storage_token" []
      (reservedInitial 0x3c++[wordsAt 0x9101c0 slots,word 0x910200 previous]) final (Just result) []
      (reservedWaitCalls 0x3c 0xffffffff 0++reservedSignalCalls 0x3c 0 0)
    | (name,slots,previous,result,final) <-
      [("empty",replicate 16 0,0,1,[word 0x9101c0 1,word 0x910200 1])
      ,("skip-collision",[2,3]++replicate 14 0,1,4,[word 0x9101c8 4,word 0x910200 4])
      ,("signed-wrap",replicate 16 0,0x7fffffff,1,[word 0x9101c0 1,word 0x910200 1])
      ,("full",[1..16],16,0,[wordsAt 0x9101c0 [1..16],word 0x910200 17])]]
    ++ [make "token-lock-timeout" 0x71ef18 "storage_token" []
      (reservedInitial 0x3c++[word 0x910200 17]) [word 0x910200 17] (Just 0) [] (reservedWaitCalls 0x3c 0xffffffff 0xffffffce)]
  debugErrorCases =
    [make ("constructor-error-suffix-"++show number++"-wait-"++show waitStatus) 0xe53d00 "write_debug_file" [0,output+0x200,4,number,directoryName]
      (reservedInitial 0x66++[word 0x910204 0,word 0x8fc8a4 0,text 0x154cdb0 "old-name",text directoryName "RAW",word 0x154c744 0x11111111])
      [text 0x154cce0 ("old-name."++extension),word 0x154c744 0,word 0x154d898 0x45] Nothing []
      (reservedWaitCalls 0x66 0xffffffff 0++currentTaskCalls 0x45++currentTaskCalls 0x45
       ++notificationCalls 0x45 0x10 0 0++waitCalls 0x45 0x10 0x10 waitStatus
       ++notificationCalls 0x45 0x10 0 0++waitCalls 0x45 0x10 0x10 waitStatus
       ++notificationCalls 0x45 0x10 0 0++waitCalls 0x45 0x10 0x10 waitStatus++reservedSignalCalls 0x66 0 0)
    | (number,extension) <- [(0,"RAW"),(1,"RA1"),(9,"RA9"),(10,"R10"),(99,"R99"),(100,"RAW")],waitStatus <- [0,0xffffffce]]
  receiveCases =
    [make ("receive-status-"++show status++"-length-"++show (B.length payload)) 0x725318 "receive_message" [0x65,output,0xffffffff]
      (receiveInitial 0x65++[wordsAt output [0xa5a5a5a5,0xa5a5a5a5,output+0x100],Patch (Address (output+0x100)) (B.replicate 16 0xa5)])
      (if status == 0 then [Patch (Address output) (halfBytes 0x2c19<>B.pack [0x45,0x65]<>wordBytes (fromIntegral (B.length payload))),word (output+8) (output+0x100),Patch (Address (output+0x100)) payload]
       else [wordsAt output [0xa5a5a5a5,0xa5a5a5a5,output+0x100],Patch (Address (output+0x100)) (B.replicate 16 0xa5)]++[word 0x40a0f82c status | status /= 0xffffffce])
      (Just status) [] (receiveCalls 0x65 0xffffffff output (output+0x100) 0x2c19 0x45 0x65 payload status)
      | status <- [0,0xffffffce,0xfffffff9],payload <- [B.empty,B.pack [1,3,5,7,9]]]
