{-# LANGUAGE GHC2021 #-}
module PermitScenarios (permitScenarios, frameKindAddress, frameKindCount, frameKindTable, frameKindJournal, frameKindSlots) where

import qualified Data.ByteString as B
import Data.Char (toLower)
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import Scenarios

frameKindAddress, frameKindCount, frameKindTable, frameKindJournal, frameKindSlots :: Word32
frameKindAddress = 0x00e75490
frameKindCount = 0x009a3030
frameKindTable = 0x009a3040
frameKindJournal = 0x009a3400
frameKindSlots = 201

permitScenarios :: B.ByteString -> [Scenario]
permitScenarios image = requests ++ decodes ++ thunks
 where
  make name function arguments initial final result calls = Scenario ("permit-" ++ map toLower name) function arguments initial final result calls
  control = 0x0157bdf0
  frame = 0x01600000
  bitmap slot = 0x01596918 + slot * 0x1d0
  keyTable = Patch (Address 0x015967fc) (B.replicate (201 * 0x1d0) 0)
  slots = [keyTable, Patch (Address 0x0157dd34) (B.pack [1,0])]
  keyAt slot key = word (0x01596800 + slot * 0x1d0) key
  message :: Word32 -> Word32 -> Word32 -> B.ByteString
  message command first second = B.concat [B.pack [fromIntegral command,fromIntegral (command `div` 256),0,0],wordBytes 8,wordBytes 0,wordBytes first,wordBytes second,B.replicate 248 0]
  mirror :: Word32 -> Word32 -> Word32 -> Call
  mirror command first second = ScriptCall (Address 0xe88e1c) [(N.R0,0)] [(N.R0,0)] [] [Relative N.R1 0 (message command first second)]
  service address arguments result = call address arguments result
  setBit slot = Call (Address 0xeb0ea0) [(N.R0,bitmap slot),(N.R1,0xbe)] [(N.R0,0)] [Patch (Address (bitmap slot+23)) (B.singleton 0x40)]
  text = B.snoc (B.takeWhile (/=0) (B.drop (0xe67e1c-0xbf1a00) image)) 0
  requests =
    [ make ("request-"++show permit++"-"++show key) PermitRequest [permit,key] [] [] Nothing
        [service 0xe86d30 [] 0x123,
         ScriptCall (Address 0x726658) [(N.R0,0x23),(N.R1,0xe67e1c)] [(N.R0,0)] [Absolute (Patch (Address 0xe67e1c) text)] [],
         service 0xe86d30 [] 0x234,service 0xe86d30 [] 0x345,
         ScriptCall (Address 0xe8659c) [] [(N.R0,0)]
           [Relative N.R0 0 (B.concat [B.pack [0x67,0x13,0x34,0x45],wordBytes 8]),Indirect N.R0 8 (B.concat [wordBytes permit,wordBytes key])] [],
         service 0xe87404 [] 0]
    | (permit,key) <- [(0,0x12345678),(1,0),(0xffffffff,0x87654321)] ]
  finish command result group =
    if result == 0xffffffff then [] else
      [service 0xe73a74 [result] group] ++
      [service 0xe74a34 [command,result] 0x55 | group == 1]
  decodeCase name command first second initial body result group =
    make ("decode-"++name) PermitDecode [] (slots++initial) []
      (Just (if result /= 0xffffffff && group == 1 then 0x55 else result))
      (mirror command first second : body ++ finish command result group)
  kindDecode name kinds slot offset =
    let scenario = decodeCase name 0x1367 11 22
          [Patch (Address 0x0157dd34) (B.pack [0,0]),word (0x01596804+slot*0x1d0+offset) 22,
           wordsAt frameKindTable kinds,word frameKindCount 0,
           wordsAt frameKindJournal (replicate (fromIntegral frameKindSlots * 2) 0xffffffff)]
          [] slot 0
        visited = take (fromIntegral slot + 1) (zip [0..] kinds)
        journal = concatMap (\(index,kind) -> [index,kind]) visited
          ++ replicate (fromIntegral (frameKindSlots-slot-1) * 2) 0xffffffff
     in scenario {scenarioFinal = [word frameKindCount (slot+1),wordsAt frameKindJournal journal]}
  selected = [0x1334,0x1335,0x1336,0x1339,0x133b,0x133c,0x133d,0x133e,0x133f,0x1340,0x1341,0x1345,0x1346,0x1347,0x1348,0x134b,0x1377,0x1378,0x1379,0x137a,0x137c,0x137d,0x137f,0x1388,0x1389,0x138a,0x138b,0x138c]
  captures = [0x1b1b,0x1b35,0x1b36,0x1b37,0x1b38,0x1b39,0x1b3a,0x1b3b,0x1b3c,0x1b3d,0x1b3e,0x1b3f,0x9b33,0x9b34]
  unknown = [0,0x1333,0x1337,0x1338,0x133a,0x1342,0x1344,0x134a,0x134c,0x134d,0x134f,0x1366,0x1368,0x1376,0x1380,0x1385,0x1387,0x138d,0x13de,0x13e0,0x1b1a,0x1b1c,0x1b34,0x1b40,0x93df,0x93e1,0x9aff,0x9b01,0x9b07,0x9b09,0x9b32,0x9b35,0xffff]
  decodes =
    [decodeCase ("selected-"++show command) command 11 22 [] [service 0xe871c4 [0] 3] 3 0 | command <- selected] ++
    [decodeCase ("capture-"++show command) command 11 22 [] [service 0xe88bc0 [11] 3] 3 1 | command <- captures] ++
    [decodeCase ("unknown-"++show command) command 11 22 [] [] 0xffffffff 0 | command <- unknown] ++
    [decodeCase ("key-"++show command++"-"++show slot) command 11 22 [keyAt slot 22] [] slot 1 | command <- [0x134e,0x1367],slot <- [0,200]] ++
    [decodeCase ("secondary-key-"++show slot) 0x1367 11 22 [Patch (Address 0x0157dd34) (B.pack [0,1]),word (0x015967fc+slot*0x1d0) 22] [] slot 0 | slot <- [0,2,200]] ++
    [decodeCase "primary-precedence" 0x1367 11 22 [Patch (Address 0x0157dd34) (B.pack [1,1]),keyAt 2 22,word 0x015967fc 22] [] 2 0] ++
    [kindDecode ("kind-selected-"++show kind++"-"++show slot)
       (replicate (fromIntegral frameKindSlots) kind) slot offset
    | kind <- [0,1,2,3,4,9],slot <- [0,1,2,3,200],let offset = case kind of 0 -> 0; 2 -> 0; 1 -> 8; _ -> 0xfffffffc] ++
    [kindDecode "kind-selected-mixed-200"
       (take 200 (cycle [9,1,4,2,3,0]) ++ [1]) 200 8] ++
    [decodeCase ("missing-key-"++show command) command 11 22 [] [service 0xe881e8 [] 0] 0xffffffff 0 | command <- [0x134e,0x1367]] ++
    [decodeCase "capture-second" 0x9b00 11 22 [] [service 0xe88bc0 [22] 3] 3 1,
     decodeCase "capture-not-found" 0x9b00 11 22 [] [service 0xe88bc0 [22] 0xffffffff] 0xffffffff 0,
     decodeCase "first-of-kind" 0x1386 11 22 [] [service 0xe87058 [1] 3] 3 0,
     decodeCase "sequence" 0x13df 11 22 [Patch (Address (control+0x81c8)) (B.singleton 7)] [service 0xe871c4 [1] 3,service 0xe748c8 [3] 44,service 0xe7edd0 [44,7] 4] 4 1,
     decodeCase "remembered" 0x93e0 11 22 [word (control+0x1a9cc) 4] [] 4 0] ++
    [decodeCase ("cancel-control-"++show flag) 0x9b08 11 22 [Patch (Address (control+0x66e)) (B.singleton flag),word (control+0xdfc) 44]
       (if flag == 1 then [] else [service 0xe88bc0 [44] 4]) (if flag == 1 then 0xffffffff else 4) 1 | flag <- [0,1,2]] ++
    [decodeCase ("multi-exposure-"++show flag) 0x1349 flag 22 [] ([service 0xe871c4 [0] 3] ++ [service 0xe7c1e0 [3] 4 | flag == 1]) (if flag == 1 then 4 else 3) 0 | flag <- [0,1,2,0xffffffff]] ++
    [decodeCase ("subframe-"++show command++"-"++show kind++"-"++show ready) command 11 22 []
       ([service 0xe871c4 [0] 3,service 0xe75490 [3] kind] ++
        if kind /= 2 then [] else [service 0xe74d40 [3] 4] ++
          if command == 0x137b then [] else [service 0xeb0f38 [bitmap 4,2] ready] ++ [service 0xe74d40 [3] 4 | ready == 1])
       (if kind == 2 && (command == 0x137b || ready == 1) then 4 else 3) 1
    | command <- [0x137b,0x137e],kind <- [0,2,4],ready <- [0,1,2]]
  prefix permit kind = [mirror 0x1367 permit 22,service 0xe87ad4 [0] frame,service 0xe75490 [0] kind]
  thunk name function permit kind initial final body = make name function [] (slots++[keyAt 0 22,word (frame+0x27c8) 99,word (frame+0x2fb0) 99]++initial) final (Just 0) (prefix permit kind++body)
  thunks = concat
    [ [thunk ("thunk-zero-"++show function++"-"++show permit) function permit 0 []
         ([word 0x015ad53c 0,Patch (Address (bitmap 0+23)) (B.singleton 0x40)]++[word (frame+0x27c8) 0 | permit == 0]) [setBit 0,service 0xe8d82c [0,0] 0]
      | permit <- [0,1,0xffffffff]] ++
      [thunk ("thunk-four-"++show function) function 7 4 [] [] []] ++
      [make ("thunk-missing-key-"++show function) function [] slots [] (Just 0) [mirror 0x1367 0 22,service 0xe881e8 [] 0,service 0xe87ad4 [0xffffffff] frame,service 0xe75490 [0xffffffff] 4]] ++
      [thunk ("thunk-broadcast-"++show function++"-"++show kind++"-"++show permit) function permit kind []
        ([Patch (Address (bitmap 1+23)) (B.singleton 0x40)]++[word (frame+0x27c8) 0 | permit == 0])
        [service 0xe87114 [kind] 1,service 0xe88804 [0,kind] 1,service 0xe6e368 [1,5] 0,setBit 1,service 0xe87ad4 [1] frame,service 0xe87114 [kind] 1]
      | kind <- [2,3],permit <- [0,7]] ++
      [thunk ("thunk-one-empty-"++show function++"-"++show flag) function 7 1
         [word (frame+0x4a8) flag,wordsAt 0x01581f60 [100,100,100]]
         [wordsAt 0x01581f60 [7,7,7] | flag == 1]
         ([service 0xe75490 [0] 1]++replicate (if flag == 1 then 4 else 3) (service 0xe74c68 [0] 0)) | flag <- [0,1,2]] ++
      [thunk ("thunk-one-populated-"++show function++"-"++show flag++"-"++show permit) function permit 1
         [word (frame+0x4a8) flag,wordsAt 0x01581f60 [100,100,100],word 0x015ad53c 99]
         ([word (frame+0x2fb0) permit,word 0x015ad53c 1,Patch (Address (bitmap 1+23)) (B.singleton 0x40)]++[wordsAt 0x01581f60 [permit,permit,permit] | flag == 1 && permit > 1 && permit < 100])
         ([service 0xe75490 [0] 1,
           service 0xe74c68 [0] 1,service 0xe748c8 [0] 2,service 0xe886ec [2,0,1] 1,setBit 1,service 0xe74c68 [0] 1,
           service 0xe74c68 [0] 1,service 0xe748c8 [0] 2,service 0xe886ec [2,0,1] 1,service 0xe87ad4 [1] frame,service 0xe74c68 [0] 1]++
          [service 0xe74c68 [0] 1 | flag == 1]++
          [service 0xe74c68 [0] 1,service 0xe748c8 [0] 2,service 0xe886ec [2,0,1] 1,service 0xe8d82c [1,0] 0,service 0xe74c68 [0] 1])
      | flag <- [0,1,2],permit <- [0,1,7,100]] ++
      [thunk ("thunk-retained-negative-slot-"++show function) function 7 1
         [word (frame+0x4a8) 0,word 0x015ad53c 99]
         [word (frame+0x2fb0) 7,word 0x015ad53c 0xffffffff,Patch (Address 0x0159675f) (B.singleton 0x40)]
         [service 0xe75490 [0] 1,
          service 0xe74c68 [0] 1,service 0xe748c8 [0] 0xffffffff,service 0xe886ec [0xffffffff,0,1] 0xffffffff,
          Call (Address 0xeb0ea0) [(N.R0,0x01596748),(N.R1,0xbe)] [(N.R0,0)] [Patch (Address 0x0159675f) (B.singleton 0x40)],
          service 0xe74c68 [0] 1,service 0xe74c68 [0] 1,service 0xe748c8 [0] 0xffffffff,service 0xe886ec [0xffffffff,0,1] 0xffffffff,
          service 0xe87ad4 [0xffffffff] frame,service 0xe74c68 [0] 1,
          service 0xe74c68 [0] 1,service 0xe748c8 [0] 0xffffffff,service 0xe886ec [0xffffffff,0,1] 0xffffffff,
          service 0xe8d82c [0xffffffff,0] 0,service 0xe74c68 [0] 1]] ++
      [make ("thunk-null-context-"++show function) function [] (slots++[keyAt 0 22]) [] (Just 0)
         [mirror 0x1367 7 22,service 0xe87ad4 [0] 0,service 0xe75490 [0] 4]]
    | function <- [PermitPrimary,PermitSecondary] ]

