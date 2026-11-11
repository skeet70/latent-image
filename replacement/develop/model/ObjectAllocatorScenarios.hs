{-# LANGUAGE GHC2021 #-}
module ObjectAllocatorScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Word
import PipelineModel
import Scenarios (Address(..), Patch(..), Call(..), word, wordsAt, params)
import BuffersScenarios (criticalInitial,criticalCalls)
import ImagePlanesScenarios (currentTaskCalls,reservedInitial,reservedWaitCalls,reservedSignalCalls,notificationCalls,waitCalls)
import qualified Emulation.NativeObservation as N
import DiagnosticLoggingScenarios (textDisabledInitial)

objectAllocatorPublications :: [StagePublication]
objectAllocatorPublications = [StagePublication (Address a) (StageSymbol ("xm1_develop_"++s)) | (a,s) <- [(0xe4525c,"object_operation"),(0xe45330,"object_priority"),(0xe45364,"object_priority_index"),(0xe453d8,"order_object"),(0xe458ac,"wait_for_object"),(0xe459e0,"release_object"),(0xe45efc,"add_object_waiter"),(0xe45f8c,"remove_object_waiter"),(0xe46018,"notify_object_waiters"),(0xe463f4,"object_range_available"),(0xe465a0,"find_object_range"),(0xe46808,"acquire_object")]]

publication :: Word32 -> StagePublication
publication a = case filter ((==Address a).stageStockEntry) objectAllocatorPublications of
 p:_ -> p
 [] -> error "unknown object allocator entry"

bytes :: Word32 -> [Word8] -> Patch
bytes a = Patch (Address a) . B.pack
half :: Word32 -> Word16 -> Patch
half a v = bytes a [fromIntegral v,fromIntegral (v `shiftR` 8)]

data Range = Range { rangeFormat :: Word32, rangeStart :: Word32, rangeLength :: Word32, rangeStride :: Word32, rangeLines :: Word32 } deriving (Eq,Show)
rangeWords :: Range -> [Word32]
rangeWords r = [rangeFormat r,rangeStart r,rangeLength r,rangeStride r,0,rangeLines r,0]
rangeSize :: Range -> Word32
rangeSize r | rangeStride r /= 0 && rangeLines r /= 0 = rangeStride r*rangeLines r
            | otherwise = rangeLength r
rangeAvailable :: Range -> Word32 -> [(Word32,Range)] -> Bool
rangeAvailable r owner = all disjoint . takeWhile ((/=0).fst)
 where
  start=rangeStart r .|. 0x40000000
  end=rangeStart r+rangeSize r-1
  disjoint (otherOwner,other)
   | owner /= 0 && owner == otherOwner = True
   | rangeFormat r /= rangeFormat other = True
   | start < otherStart = end < otherStart
   | otherwise = start > otherStart+rangeSize other-1
   where otherStart=rangeStart other .|. 0x40000000

allocationBlocks :: Word32 -> Word32 -> Word32
allocationBlocks size unit = max 1 ((size `div` unit)+if size `mod` unit == 0 then 0 else 1)
selectRange :: Bool -> Word32 -> Word32 -> [Bool] -> Maybe Word32
selectRange ascending capacity count occupied = case filter free candidates of
 x:_ -> Just x
 [] -> Nothing
 where
  candidates | count > capacity = []
             | ascending = [0..capacity-count]
             | otherwise = reverse [0..capacity-count]
  free first = not (or (take (fromIntegral count) (drop (fromIntegral first) occupied)))

objectAllocatorScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
objectAllocatorScenarios _ _ = priorityCases++overlapCases++rangeCases++waiterCases++notifyCases++orderCases++waitCases++acquireCases++releaseCases++cancellationCases++splitCases
 where
  make name entry args initial final result calls = StageScenario (StageCaseName ("object-allocator-"++name)) (publication entry) args initial final result [] calls
  base=criticalInitial++[word 0x15480d4 3,wordsAt 0x154c32c (replicate 30 0)]
  groupAddress g | g<20 = 0xe48370+g*16
                 | otherwise = 0xe4a948+(3*73+g-20)*16
  descriptors=[wordsAt 0xe48370 (replicate (20*4) 0),wordsAt (groupAddress 20) (replicate (73*4) 0)]
  descriptor g handle capacity shared = wordsAt (groupAddress g) [handle,capacity,shared,params+0x400]
  bitmap g value=bytes (0x15480dc+g*26) (value:replicate 25 0)
  reference shared slot=0x1548a4c+((shared-1)*201+slot)*2
  priorityCases=
   [make ("operation-"++show g) 0xe4525c [g] [] [] (Just result) [] | (g,result)<-[(0,1),(2,2),(26,4),(33,5),(18,6),(19,0)]]++
   [make ("priority-"++show i) 0xe45330 [i] [word 0x15480d4 3,wordsAt (0xe49320+3*28) [0,1,2,4,5,6,3]] [] (Just ([0,1,2,4,5,6,3]!!fromIntegral i)) [] | i<-[0..6]]++
   [make ("priority-index-"++show op) 0xe45364 [op] [word 0x15480d4 3,wordsAt (0xe49320+3*28) [0,1,2,4,5,6,3]] [] (Just result) [] | (op,result)<-[(0,0),(1,1),(5,4),(3,6),(99,0)]]
  overlapCases=
   [let candidate=Range 7 start rangeLength stride rangeLines
        other=Range format 0x40001040 64 0 0
        result=rangeAvailable candidate owner [(9,other)]
     in make ("overlap-"++show (start,rangeLength,stride,rangeLines,format,owner)) 0xe463f4 [params,owner]
       (base++[wordsAt params (rangeWords candidate),wordsAt 0x154ad2c ([9,20,0,1]++rangeWords other),half (0x154ad2c+44) 0]) [] (Just (if result then 1 else 0)) criticalCalls
   | (start,rangeLength,stride,rangeLines)<-[(0x40001000,64,0,0),(0x40001001,64,0,0),(0x40001080,16,0,0),(0x40001000,1,32,3),(0x1000,128,0,0)],format<-[7,8],owner<-[0,9]]
  rangeCases=
   [let occupied=[testBit mask (7-i) | i<-[0..3]]
        count=allocationBlocks size 64
        expected=maybe 0xffffffff id (selectRange ascending 4 count occupied)
        planes=concat [rangeWords (Range 7 (0x40002000+i*64) 64 0 0) | i<-[0..3]]
     in make ("range-"++show (ascending,size,mask)) 0xe465a0 [20,9,size,if ascending then 1 else 0]
       (base++descriptors++[descriptor 20 1 4 0,bitmap 20 mask,wordsAt (params+0x400) planes,half 0x154ad2c 0])
       [bytes 0x15480b8 (mask:replicate 25 0)] (Just expected) (criticalCalls++concat (replicate (overlapCount ascending count occupied) criticalCalls))
   | ascending<-[False,True],size<-[0,64,65,128],mask<-[0,0x80,0x50,0xf0]]
  overlapCount ascending count occupied = go candidates
   where
    candidates | count>4=[] | ascending=[0..4-count] | otherwise=reverse [0..4-count]
    go []=0
    go (first:rest) | or (take (fromIntegral count) (drop (fromIntegral first) occupied))=go rest
                    | otherwise=fromIntegral count
  waiterCases=
   [make ("waiter-add-"++show occupied) 0xe45efc [params]
      (base++[wordsAt params [20,0x45],wordsAt 0x154c32c (concat (replicate occupied [21,0x46])++replicate ((15-occupied)*2) 0)])
      [wordsAt (0x154c32c+fromIntegral occupied*8) [20,0x45] | occupied<15] Nothing criticalCalls
   | occupied<-[0,1,14,15]]++
   [make "waiter-remove-hole" 0xe45f8c [params]
      (base++[wordsAt params [20,0x45],wordsAt 0x154c32c [21,0x46,20,0x45,22,0x47]])
      [wordsAt (0x154c32c+8) [0,0]] Nothing criticalCalls]
  notifyCases=
   [make "notify-snapshot-order" 0xe46018 [] (base++[wordsAt 0x154c32c [20,0x45,0,0,22,0x47],word (0x7cad9c+((0x47 .&. 0xffff)-1)*4) 3]) [] Nothing
      (criticalCalls++notificationCalls 0x45 16 0 0++notificationCalls 0x47 16 0 0)]
  orderCases=
   [make ("order-"++show blocked) 0xe453d8 [33,9,params]
      (base++[wordsAt (0xe49320+3*28) [0,1,2,4,5,6,3],word 0x154a9b8 2,wordsAt 0x154a04c [8,if blocked then 1 else 6,0,9,1,0]])
      [word params 0,wordsAt (0x154a04c+16) [5,if blocked then 0x45 else 0]] (Just (if blocked then 0 else 1)) (currentTaskCalls 0x45++criticalCalls)
   | blocked<-[False,True]]
  waitCases=
   [make "wait-unordered" 0xe458ac [2,9,0] (base++descriptors++reservedInitial 1++[descriptor 2 1 4 0]) [] (Just 0) (reservedWaitCalls 1 0xffffffff 0)]
  acquireCases=
   [make ("acquire-shared-reentry-"++show inherited) 0xe46808 [21,0,9,inherited,params,1,64]
      (base++descriptors++[descriptor 20 1 4 1,descriptor 21 1 4 1,half (reference 1 selected) 2,half params 0])
      [half (reference 1 selected) 3,half params (fromIntegral selected)] (Just 0) criticalCalls
   | inherited<-[0,1,3],let selected=if inherited==0 then 1 else inherited]++
   [make ("acquire-small-"++show mask) 0xe46808 [2,0,9,0,params,0,0]
      (base++descriptors++reservedInitial 1++[descriptor 2 1 4 0,bitmap 2 mask,half params 0])
      [bitmap 2 (mask .|. (0x80 `shiftR` fromIntegral selected)),half params (fromIntegral (selected+1))] (Just 0)
      (reservedWaitCalls 1 0xffffffff 0++criticalCalls)
   | mask<-[0,0x80,0xc0],let selected=(case mask of 0->0;0x80->1;_->2)::Int]
  releaseCases=
   [make ("release-reference-"++show refs) 0xe459e0 [20,2,64]
      (base++descriptors++reservedInitial 1++[descriptor 20 1 4 1,bitmap 20 0x40,half (reference 1 2) refs])
      ([half (reference 1 2) (refs-1)]++[bitmap 20 0 | refs==1]) (Just 0)
      (criticalCalls++[c | refs==1,c<-reservedSignalCalls 1 0 0])
   | refs<-[1,2,3]]
  cancellationCases=
   [make "ordered-wait-cancel" 0xe458ac [33,9,1]
      (base++descriptors++reservedInitial 1++[descriptor 33 1 4 0,wordsAt (0xe49320+3*28) [0,1,2,4,5,6,3],word 0x154a9b8 2,wordsAt 0x154a04c [8,1,0,9,1,0]])
      [wordsAt (0x154a04c+16) [5,0x45]] (Just 0xffffffce)
      (reservedWaitCalls 1 0xffffffff 0++currentTaskCalls 0x45++criticalCalls++reservedSignalCalls 1 0 0++waitCalls 0x45 16 0 0xffffffce)
   ,make "exhausted-range-wait-cancel-retains-waiter" 0xe46808 [20,0,9,0,params,0,64]
      (base++descriptors++reservedInitial 1++textDisabledInitial++[descriptor 20 1 4 0,bitmap 20 0xf0,half params 0xa5a5,wordsAt (params+0x400) (concat [rangeWords (Range 7 (0x40002000+i*64) 64 0 0) | i<-[0..3]])])
      [bytes 0x15480b8 (0xf0:replicate 25 0),wordsAt 0x154c32c [20,0x45],half params 0xa5a5] (Just 0xffffffce)
      (reservedWaitCalls 1 0xffffffff 0++criticalCalls++currentTaskCalls 0x45++[Call (Address 0x42188) [(N.R0,0xe498e0)] [(N.R0,0)] []]++currentTaskCalls 0x45++criticalCalls++waitCalls 0x45 16 0 0xffffffce)
   ]
  splitCases=
   [make ("acquire-split-range-"++show kind) 0xe46808 [33,kind,9,0,params,0,65]
      (base++descriptors++reservedInitial 1++[descriptor 33 1 4 0,bitmap 33 0x80,half params 0,half 0x154ad2c 0,wordsAt (params+0x400) (concat [rangeWords (Range 7 (0x40002000+i*64) 64 0 0) | i<-[0..3]])])
      [bytes 0x15480b8 (0x80:replicate 25 0),bitmap 33 (if kind==0 then 0xe0 else 0xb0),half params (if kind==0 then 2 else 3)] (Just 0)
      (reservedWaitCalls 1 0xffffffff 0++concat (replicate 4 criticalCalls))
   | kind<-[0,9]]++
   [make "release-split-range" 0xe459e0 [33,2,65]
      (base++descriptors++reservedInitial 1++[descriptor 33 1 4 0,bitmap 33 0xe0,wordsAt (params+0x400) (rangeWords (Range 7 0x40002000 64 0 0))])
      [bitmap 33 0x80] (Just 0) (criticalCalls++reservedSignalCalls 1 0 0)
   ,make "order-handoff-promotes-waiter" 0xe453d8 [33,9,params]
      (base++[wordsAt (0xe49320+3*28) [0,1,2,4,5,6,3],word 0x154a9b8 2,wordsAt 0x154a04c [9,1,0,10,5,0x46]])
      [word params 0x46,wordsAt (0x154a04c+4) [5,0],wordsAt (0x154a04c+16) [4,0]] (Just 1) (currentTaskCalls 0x45++criticalCalls)
   ]
