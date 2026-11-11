{-# LANGUAGE GHC2021 #-}
module WarpEngineScenarios (module WarpEngineScenarios) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word32)
import PipelineModel
import qualified Emulation.NativeObservation as N
import Scenarios
import BuffersScenarios (criticalInitial, criticalEnterCalls, criticalLeaveCalls)
import ImagePlanesScenarios (reservedInitial, reservedSignalCalls, currentTaskCalls)
import DiagnosticLoggingScenarios (CalendarFields(..), textInitial, textCalls, textEffects)

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name))

configure, build, program, selectFormat, geometry, filterWrite, initialize, release, cancel, start, completion :: StagePublication
configure = publication 0x6df978 "warp_configure"
build = publication 0x6df530 "warp_build_parameters"
program = publication 0x6df898 "warp_program"
selectFormat = publication 0x6df18c "radial_select_format"
geometry = publication 0x68c264 "warp_write_geometry"
filterWrite = publication 0x68c738 "warp_write_filter"
initialize = publication 0x6df094 "warp_initialize"
release = publication 0x6df0c4 "warp_release"
cancel = publication 0x6df0d8 "warp_cancel"
start = publication 0x6df134 "warp_start"
completion = publication 0x6defbc "warp_completion"

warpEnginePublications :: [StagePublication]
warpEnginePublications = [configure,build,program,selectFormat,geometry,filterWrite,initialize,release,cancel,start,completion] ++
  [publication address name | (address,name) <- [(0x6499d0,"route_request"),(0x649c4c,"route_acquire"),(0x649ddc,"route_release"),(0x64a0bc,"route_resolve"),(0x64a224,"route_apply"),(0x65722c,"driver_allocate"),(0x657384,"driver_find"),(0x65744c,"driver_clear"),(0x64e918,"interrupt_acquire"),(0x64eb20,"interrupt_release"),(0x726c74,"diagnostic_event"),(0x726a38,"diagnostic_trigger"),(0x726b30,"diagnostic_selected"),(0x726a80,"diagnostic_driver_text")]]
  ++ [publication 0x723870 "irq_configure",publication 0x7239a0 "irq_release"]

warpConfigurationFixture :: [Word32]
warpConfigurationFixture = [1,0,4096,4096,0x101,640,640,480,0,0,0x92345670,320,320,240,320,240,639,479,0,0x12345678]
warpParameterFixture :: [Word32]
warpParameterFixture = [0,0x100,0,0,640,480,0xa5a5a5a5,240,0,0,639,479,1279,959,0xa5a5a5a5,0xa5a5a5a5,4095,4095,0,0,4096,4096,0,0x10001,639,479] ++ replicate 31 0x10000

warpRegisterPayload :: [Word32] -> [Word32] -> [Patch]
warpRegisterPayload c p = map (uncurry word) $
  [(0xfff65004,(c!!0 .&. 1) `shiftL` 6),(0xfff65034,c!!1 .&. 3),(0xfff65038,p!!16),(0xfff6503c,p!!17),
   (0xfff65010,p!!1 .&. 0x3fffffff),(0xfff65014,c!!5),(0xfff65018,p!!4 .|. (p!!5 `shiftL` 16)),
   (0xfff6501c,c!!10 .&. 0x3fffffff),(0xfff65020,c!!11),(0xfff65024,c!!12 .|. (c!!13 `shiftL` 16)),
   (0xfff65028,c!!14 .|. (p!!7 `shiftL` 16)),(0xfff6502c,p!!0 .&. 3),(0xfff65030,p!!8 .|. (p!!9 `shiftL` 16)),
   (0xfff65050,p!!10 .|. (p!!11 `shiftL` 16)),(0xfff65054,p!!12 .|. (p!!13 `shiftL` 16)),
   (0xfff65040,p!!20),(0xfff65044,p!!21),(0xfff65048,p!!22 .&. 0x7fff7fff),
   (0xfff6504c,p!!23),(0xfff65058,p!!24 .|. (p!!25 `shiftL` 16))] ++
  [(0xfff65060+i*4,p!!fromIntegral (26+i) .&. 0x1ffff) | i <- [0..30]]

diagnosticEventInitial :: [Patch]
diagnosticEventInitial = [wordsAt 0x40a5b660 [0,0],wordsAt 0x4089ea9c (replicate 80 0xffffffff)]

diagnosticEventCalls :: Word32 -> Word32 -> [Call]
diagnosticEventCalls category event = [call 0x422e0 [category,event] 0]

diagnosticEventEffects :: Word32 -> Word32 -> [Patch]
diagnosticEventEffects category event = [word (0x40a5b660+(category-1)*4) event | category == 1 || category == 2]

routeInitial :: [Patch]
routeInitial = criticalInitial ++ [wordsAt 0x409c1e30 (replicate (0x814 `div` 4) 0),word 0x409c2804 0,word 0x4089e810 0] ++
  [word (0x409c1e84+index*0xa0) 11 | index <- [0..9]]

interruptInitial :: [Patch]
interruptInitial = criticalInitial ++ irqInitial ++ diagnosticEventInitial ++ [wordsAt 0x940e9c (replicate 256 0),word 0x409c2804 0]

interruptAcquireCalls :: Word32 -> Word32 -> Word32 -> Word32 -> [Call]
interruptAcquireCalls line priority mode callbackAddress = criticalEnterCalls ++ criticalLeaveCalls ++ criticalEnterCalls ++ irqConfigureCalls line priority mode callbackAddress ++ criticalLeaveCalls

interruptReleaseCalls :: Word32 -> [Call]
interruptReleaseCalls line = irqReleaseCalls line

interruptReleaseEffects :: Word32 -> [Patch]
interruptReleaseEffects line = [word (0x940e9c+line*4) 0,word (0x940e9c+(line+128)*4) 0xffffffff]

irqInitial :: [Patch]
irqInitial = [word (0x9b6000+0x160) 0x8f0194,word (0x9b6000+0x158) 0x8f0198,word (0x9b6000+0x15c) 0x8f019c]

irqConfigureCalls :: Word32 -> Word32 -> Word32 -> Word32 -> [Call]
irqConfigureCalls line priority mode callbackAddress =
  [call 0x8f0100 [] 0,call 0x8f0180 [] 0x123,
   ScriptCall (Address 0x8f0194) [(N.R0,line)] [] [Relative N.R1 0 (B.concat (map wordBytes configuration))] [],
   call 0x8f0198 [line] 0,call 0x8f0184 [0x123] 0]
 where configuration = [0,callbackAddress,priority,if mode > 1 then 1 else 0,case mode of 0 -> 2;1 -> 1;2 -> 2;3 -> 1;4 -> 3;_ -> 0]

irqReleaseCalls :: Word32 -> [Call]
irqReleaseCalls line = [call 0x8f0100 [] 0,call 0x8f0180 [] 0x123,call 0x8f019c [line] 0,call 0x8f0194 [line,0] 0,call 0x8f0184 [0x123] 0]


routeAcquireCalls :: [Call]
routeAcquireCalls = criticalEnterCalls ++ [call 0x42574 [] 3,call 0x4257c [3] 0] ++ criticalLeaveCalls

routeAcquireEffects :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
routeAcquireEffects index allocatedHandle bank mask previous =
  [word 0x409c2804 allocatedHandle,word (0x409c1e30+0x758+index*4) allocatedHandle,word (0x409c1e30+0x69c+index*4) 1,word (if bank == 0 then 0xfff8802c else 0xfff88030) (previous .|. mask)]

routeRequestCalls :: Bool -> [Call]
routeRequestCalls changesRegister = criticalEnterCalls ++ (if changesRegister then [call 0x42574 [] 3,call 0x4257c [3] 0] else []) ++ criticalLeaveCalls

routeReleaseCalls :: Bool -> [Call]
routeReleaseCalls startupPreservesClock = routeRequestCalls (not startupPreservesClock)


warpEngineScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
warpEngineScenarios _ resolve = buildCases ++ programCases ++ configureCases ++ formatCases ++ filterCases ++ releaseCases ++ startCases ++ completionCases ++ driverCases ++ routeCases ++ interruptCases ++ diagnosticCases
 where
  make name stagePublication args initial final result pointers calls = StageScenario (StageCaseName name) stagePublication args initial final result pointers calls
  c = warpConfigurationFixture
  p = warpParameterFixture
  request = 0x9a1000
  output = 0x9a2000
  correction = 0x9a3000
  runtime = 0x942c04
  sentinel = 0xa5a5a5a5
  replace index value values = take index values ++ [value] ++ drop (index+1) values
  buildCase name config expected extras = make name build [request,output] ([wordsAt request config,wordsAt output (replicate 57 sentinel)]++extras) [wordsAt output expected] (Just 0) [] []
  unity = replace 19 0 c
  automatic = replace 3 0 (replace 2 0 unity)
  autoP = replace 17 8209 (replace 16 8204 (replace 13 719 (replace 12 959 p)))
  correctionValues = [2,0x1234,0x1fff,0xffff8001,0x70002,123,456] ++ [0x20000+i | i <- [0..30]]
  corrected = replace 18 correction unity
  correctedP = [2] ++ take 7 (drop 1 p) ++ [4,4] ++ take 10 (drop 10 p) ++ drop 1 correctionValues
  buildCases = [buildCase "warp-build-explicit-unity" unity p [],buildCase "warp-build-derived-ratios" automatic autoP [],buildCase "warp-build-correction-copy" corrected correctedP [wordsAt correction correctionValues]] ++
    [let config = replace 9 496 (replace 8 672 (replace 4 0x100 unity))
         expected = replace 11 495 (replace 10 671 (replace 5 496 (replace 4 672 (replace 3 8 (replace 2 16 p)))))
      in buildCase "warp-build-limits-padding" config expected []]
  programCases = [make "warp-program-register-payload" program [request,output] [wordsAt request c,wordsAt output p] (warpRegisterPayload c p) (Just 0) [] []]
  configureCases = [make "warp-configure-copy80-register-payload" configure [request] [wordsAt request c,wordsAt runtime (replicate 80 sentinel)] ([wordsAt (runtime+8) c,wordsAt (runtime+88) p]++warpRegisterPayload c p) (Just 0) [] []]
  formatCases = [make ("warp-format-"++show format++"-"++show sx++"-"++show sy) selectFormat [request,output] [wordsAt request (replace 1 format unity),word (output+64) sx,word (output+68) sy] [word output mode] Nothing [] [] | (format,sx,sy,mode) <- [(0,4096,4096,0),(2,4096,4096,1),(0,10241,4096,1),(0,20481,4096,2),(0,4096,16895,0)]]
  filterValues = [0x1fff,0x1234,0xffff8001,0x70002,0xffff,0xabcd]++[0xffff0000+i | i <- [0..30]]
  filterFinal = [word 0xfff65040 0x1fff,word 0xfff65044 0x1234,word 0xfff65048 0x7fff0001,word 0xfff6504c 0x70002,word 0xfff65058 0xabcdffff]++[word (0xfff65060+i*4) (0x10000+i) | i <- [0..30]]
  filterCases = [make "warp-filter-masks-packed-halfwords" filterWrite [output] [wordsAt output filterValues] filterFinal Nothing [] []]
  releaseCases = [make ("warp-release-"++show active) release [] (routeInitial++reservedInitial 4++[wordsAt 0x942d64 [99,active,17],word (0x409c1e30+0x758+42*4) 99,word (0x409c1e30+0x69c+42*4) 1,word 0xfff88030 0x100]) ([word 0x942d68 0]++if active == 0 then [] else [word (0x409c1e30+0x758+42*4) 0,word (0x409c1e30+0x69c+42*4) 0,word 0xfff88030 0]) (Just 0) [] ((if active == 0 then [] else criticalEnterCalls++[call 0x42574 [] 3,call 0x4257c [3] 0]++criticalLeaveCalls)++concat (replicate 2 (criticalEnterCalls++criticalLeaveCalls))++reservedSignalCalls 4 0 0) | active <- [0,1]]
  callbackAddress = addressWord (resolve completion)
  startCases = [make "warp-start-registers-resource" start [] (interruptInitial++[word 0xfff65000 0x10000]) [word 0xfff65140 0,word 0xfff65148 0x21,word (runtime+4) 1,word runtime 1,word 0xfff65000 0x10001,word 0x409c2804 1,word (0x940e9c+74*4) 1,word (0x940e9c+(74+128)*4) 57] Nothing [] (interruptAcquireCalls 74 13 1 callbackAddress++[call 0x42634 [0,0] 0])]
  completionCases = [make ("warp-completion-"++show status++"-"++show active) completion [] (interruptInitial++[wordsAt runtime [88,active,1],word (runtime+84) 0x800000,word (runtime+316) 0xffffffff,word 0xfff65144 status,word (0x940e9c+74*4) 88,word (0x940e9c+(74+128)*4) 57]) ([word (runtime+316) 0,word (runtime+4) 0,word (runtime+84) 0,word 0xfff65148 0,word 0xfff65144 status]++if active == 0 then [] else interruptReleaseEffects 74) Nothing [] (criticalEnterCalls++(if active == 0 then [] else interruptReleaseCalls 74)++criticalLeaveCalls++if status .&. 1 == 0 then [call 0x800000 [] 0] else []) | status <- [0,1,0x20], active <- [0,1]]
  pub address = case filter ((==Address address) . stageStockEntry) warpEnginePublications of x:_ -> x; [] -> error "warp publication absent"
  driverCases = [make "driver-allocate-wrap-collision" (pub 0x65722c) [2,4,output] [wordsAt output [1,2,0,4],word 0x409c2804 0xffffffff] [wordsAt output [1,2,3,4],word 0x409c2804 3] (Just 0) [] []]++
    [make ("driver-allocate-error-"++show slot) (pub 0x65722c) [slot,4,output] [wordsAt output [1,2,3,4],word 0x409c2804 42] [word 0x409c2804 42] (Just result) [] [] | (slot,result) <- [(0,0xffffffec),(4,0xffffffe2)]]++
    [make ("driver-find-"++show handle) (pub 0x657384) [handle,4,output,request] [wordsAt output [7,88,9,10],word request sentinel] [word request expected] (Just result) [] [] | (handle,expected,result) <- [(88,1,0),(0,sentinel,0xffffffe2),(99,sentinel,0xffffffe2)]]++
    [make ("driver-clear-"++show slot) (pub 0x65744c) [slot,4,output] [wordsAt output [7,8,9,10]] [wordsAt output expected] (Just result) [] [] | (slot,expected,result) <- [(2,[7,8,0,10],0),(4,[7,8,9,10],0xffffffe2)]]
  routeCases = [make "route-request-select-level10" (pub 0x6499d0) [0x508,20,10] (routeInitial++[word 0xfff88014 0xabcdef07]) [word (0x409c1e84+5*0xa0+22*4) 10,word (0x409c1e84+5*0xa0+156) 10,word 0xfff88014 0xabcdef00] (Just 0) [] (criticalEnterCalls++[call 0x42574 [] 3,call 0x4257c [3] 0]++criticalLeaveCalls)]++
    [make ("route-request-invalid-"++show selector++"-"++show slot++"-"++show level) (pub 0x6499d0) [selector,slot,level] routeInitial [] (Just 0xffffffe2) [] (criticalEnterCalls++criticalLeaveCalls) | (selector,slot,level) <- [(0x513,20,10),(0x508,37,10),(0x508,20,12)]]++
    [make "route-acquire42-clock-mask" (pub 0x649c4c) [42,output] (routeInitial++[word 0xfff88030 0xf]) [word output 1,word 0x409c2804 1,word (0x409c1e30+0x758+42*4) 1,word (0x409c1e30+0x69c+42*4) 1,word 0xfff88030 0x10f] (Just 0) [] (criticalEnterCalls++[call 0x42574 [] 3,call 0x4257c [3] 0]++criticalLeaveCalls)]
  interruptCases = [make "interrupt-acquire-source57" (pub 0x64e918) [57,0x800000,output] interruptInitial [word output 1,word 0x409c2804 1,word (0x940e9c+74*4) 1,word (0x940e9c+(74+128)*4) 57] (Just 0) [] (interruptAcquireCalls 74 13 1 0x800000)]++
    [make "interrupt-acquire-invalid-source" (pub 0x64e918) [123,0x800000,output] interruptInitial (diagnosticEventEffects 2 15) (Just 0xffffffe2) [] (diagnosticEventCalls 2 15)]
  calendar = CalendarFields 26 11 10 12 34 56
  diagnosticCases = [make ("diagnostic-event-first-"++show category) (pub 0x726c74) [category,0x51] diagnosticEventInitial (diagnosticEventEffects category 0x51) Nothing [] (diagnosticEventCalls category 0x51) | category <- [0,1,2,3]] ++
    [make "diagnostic-first-error-preserved" (pub 0x726c74) [2,0x51] (diagnosticEventInitial++[word 0x40a5b664 0x42]) [word 0x40a5b664 0x42] Nothing [] (diagnosticEventCalls 2 0x51),
     make "diagnostic-trigger-match" (pub 0x726c74) [2,0x51] (diagnosticEventInitial++[wordsAt 0x4089ea9c [2,0x51,0]]) (diagnosticEventEffects 2 0x51++[word 0x4089eaa4 1]) Nothing [] (diagnosticEventCalls 2 0x51)] ++
    [make ("diagnostic-selected-driver-text-"++show slot) (pub 0x726c74) [2,0x51]
      (diagnosticEventInitial++textInitial 0x9a6000 8 calendar++[word (0x4089ea9c+12+slot*4) 0x51,Patch (Address 0x9a6000) (B.replicate 0x2100 0xa5)])
      (diagnosticEventEffects 2 0x51++textEffects 0x9a6000 8 0x45 (BC.pack "errno_set(ERRLVL_DRIVER,0x0051)") calendar)
      Nothing [] (diagnosticEventCalls 2 0x51++currentTaskCalls 0x45++textCalls) | slot <- [0,76]]
