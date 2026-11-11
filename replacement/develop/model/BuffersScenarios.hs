{-# LANGUAGE GHC2021 #-}
module BuffersScenarios (module BuffersScenarios,Plane(..),planeWords,planePatch) where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int16,Int32)
import Data.List (nub)
import Data.Word
import PipelineModel
import Scenarios (Address(..), Patch(..), Call(..), word, wordsAt, readWord, context, state, params)
import qualified Emulation.NativeObservation as N
import ImagePlanesScenarios (priorityInitial,priorityCalls,currentTaskCalls,fillEffects,reservedInitial,reservedWaitCalls,reservedSignalCalls)

data Crop = Crop { cropWidth :: Word32, cropHeight :: Word32, cropTop :: Word32, cropBottom :: Word32, cropLeft :: Word32, cropRight :: Word32 } deriving (Eq, Show)
cropWords :: Crop -> [Word32]
cropWords c = [cropWidth c,cropHeight c,cropTop c,cropBottom c,cropLeft c,cropRight c]

expandPlanes :: Word8 -> Word32 -> Plane -> Plane -> [Plane]
expandPlanes count parity source auxiliary = source : go 1 source
 where
  go :: Int -> Plane -> [Plane]
  go i previous | i >= fromIntegral count = []
                | otherwise = next : go (i+1) next
   where
    template = if i == 1 then auxiliary else previous
    address | i == 1 = planeAddress auxiliary + (if parity == 0 then 0 else align 31 (planeStride previous `shiftR` 1))
            | i == 3 = planeAddress previous + planeHeight previous * planeStride previous
            | otherwise = planeAddress previous + planeWidth previous
    width | i == 1 = align 31 (planeWidth previous `shiftR` 2) .&. 0x7fffffff
          | otherwise = align 31 (planeWidth previous `shiftR` 1)
    height = ((planeHeight previous `shiftR` 1) + 1) .&. complement 1
    next = template { planeAddress=address,planeWidth=width,planeHeight=height,planeStride=planeStride previous,planeLength=planeStride previous*height }
splitPlanes :: Word8 -> Plane -> Plane -> [Plane]
splitPlanes count source auxiliary = source : go 1 source
 where
  go :: Int -> Plane -> [Plane]
  go i previous | i >= fromIntegral count = []
                | otherwise = next : go (i+1) next
   where
    template = if i == 1 then auxiliary else previous
    address | i == 1 = planeAddress auxiliary
            | i == 4 = planeAddress previous + planeHeight previous * planeStride previous
            | otherwise = planeAddress previous + planeWidth previous
    height = ((planeHeight previous `shiftR` 1) + 1) .&. complement 1
    next = template { planeAddress=address,planeWidth=align 63 (planeWidth previous `shiftR` 1),planeHeight=height,planeStride=planeStride previous,planeLength=planeStride previous*height }
align :: Word32 -> Word32 -> Word32
align mask n = (n+mask) .&. complement mask

fitRatio :: Word32 -> Word32 -> Word32 -> Word32 -> Crop
fitRatio width height numerator denominator
 | difference > 0 = let h=width*denominator `div` numerator; b=height-h in Crop width h (b `div` 2) (b-b `div` 2) 0 0
 | difference < 0 = let w=height*numerator `div` denominator; b=width-w in Crop w height 0 0 (b `div` 2) (b-b `div` 2)
 | otherwise = Crop width height 0 0 0 0
 where difference = fromIntegral (height*numerator-width*denominator) :: Int32
fitAspect :: Word32 -> Word32 -> Word32 -> Crop
fitAspect w h aspect = uncurry (fitRatio w h) $ case aspect of 1 -> (3,2); 2 -> (16,9); 3 -> (1,1); _ -> (4,3)

bufferDimensions :: Word32 -> Word32 -> (Word32,Word32)
bufferDimensions profile aspect
 | profile == 2 || profile == 3 = case aspect of 1 -> (272,180); 2 -> (272,153); 3 -> (204,204); _ -> (272,204)
 | profile == 5 = case aspect of 1 -> (176,118); 2 -> (176,99); 3 -> (132,132); _ -> (176,132)
 | otherwise = (0,0)
bufferOrientation :: Word32 -> Word32 -> Word32
bufferOrientation selection orientation = case selection of
  2 -> case orientation of 1 -> 3; 2 -> 4; 3 -> 2; _ -> 1
  3 -> case orientation of 1 -> 4; 2 -> 1; 3 -> 3; _ -> 2
  4 -> case orientation of 1 -> 1; 2 -> 2; 3 -> 4; _ -> 3
  _ -> case orientation of 1 -> 2; 2 -> 3; 3 -> 1; _ -> 4

bufferGeometry :: (Word32 -> Word32 -> (Word32,Word32)) -> Plane -> Plane -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> (Plane,[Word32])
bufferGeometry lookupSize source destination profile aspect capture mode anchor = (result,[top,bottom,left,horizontal-left])
 where
  (width,height)
   | capture == 29 = let w=planeWidth destination `div` 2 in (w,planeHeight source*w `div` (planeWidth source `div` 2))
   | mode == 1 && aspect /= 3 = let w=planeWidth destination `div` 2; (rw,rh)=lookupSize profile aspect in (w,w*rh `div` rw)
   | aspect == 3 = (planeHeight destination,planeHeight destination)
   | otherwise = let w=planeWidth destination `div` 2 in (w,case aspect of 1 -> w*2 `div` 3; 2 -> w*9 `div` 16; _ -> w*3 `div` 4)
  vertical=planeHeight destination-height
  top=if anchor == 1 then 0 else vertical `div` 2
  bottom=vertical-top
  horizontal=planeWidth destination `div` 2-width
  left=horizontal `div` 2
  bytes=planeWidth destination-horizontal*2
  result=destination { planeAddress=planeAddress destination+left*2+planeStride destination*top,planeWidth=bytes,planeHeight=height,planeLength=bytes*height }

borderRegions :: Plane -> [Word32] -> [Plane]
borderRegions p amounts = [region side amount | (side,amount) <- zip [0::Int ..3] amounts, amount /= 0]
 where
  region side amount = let h=if side<2 then amount else planeHeight p; w=if side<2 then planeWidth p else amount*2; a=case side of 1 -> planeAddress p+planeStride p*(planeHeight p-amount); 3 -> planeAddress p+planeStride p-w; _ -> planeAddress p in p { planeAddress=a,planeHeight=h,planeWidth=w,planeLength=w*h }

outputMatches :: Word32 -> (Word32 -> Word32) -> (Word32 -> Word32 -> (Word32,Word32)) -> Plane -> Word32 -> Word32 -> Bool
outputMatches option field lookupSize p output mode
 | option /= 0 = True
 | special && output == 0 && planeFlags p .&. 0x2000000 == 0 = False
 | otherwise = planeWidth p == planeStride p && planeWidth p == width && planeHeight p == height
 where
  special=field 0x4b4 == 25 || field 0x710 .&. 0x400 /= 0
  (width,height) | output == 1 = (320,120)
                 | output == 2 = (640,240)
                 | otherwise = let (w,h)=if mode == 2 then lookupSize 33 1 else lookupSize (field 0x644) (field 0x4f0) in (w*2,h)
conversionRequired :: Word32 -> (Word32 -> Word32) -> (Word32 -> Word32 -> (Word32,Word32)) -> Plane -> Word32 -> Bool
conversionRequired option f lookupSize p mode
 | f 0x4b4 `elem` [29,32] = True
 | outputMatches option f lookupSize p 0 mode = False
 | mode == 1 && f 0x5a8 == 1 = True
 | (f 0x614 /= 0 || f 0x61c /= 0) && (mode /= 0 || f 0x275c /= 1) = True
 | f 0x4ac == 1 || f 0x78 == 11 = True
 | f 0x4a8 /= 0 = False
 | otherwise = f 0x550-151 < 2

operationMemorySize :: B.ByteString -> (Word32 -> Word32) -> Word32 -> Word32 -> Word32
operationMemorySize image f group kind
 | f 0x4b4 == 29 || f 0x604 == 1 || group /= 33 = 0
 | f 0x27ac == 1 && f 0x600 == 1 = 0x26d160
 | kind == 16 = 0x144d60
 | otherwise = search 0xea1da0
 where
  search a | readWord image a == 10 = 0
           | map (readWord image . (a+)) [0,4,8] == map f [0x644,0x50c,0x4f0] = readWord image (a+12)
           | otherwise = search (a+16)
remapObject :: B.ByteString -> (Word32 -> Word32) -> Word32 -> Word32
remapObject image f object = search 0xea1eec
 where
  search a = case [column | column <- [0,1,2], readWord image (a+column*4) == object] of
   column:_ | object == 93 -> object
            | f 0x4b4 == 25 || f 0x710 .&. 0x400 /= 0 -> if column == 0 then readWord image (a+4) else object
            | f 0x4b4 == 26 && column == 1 -> readWord image a
            | otherwise -> object
   [] | readWord image (a+16) == 93 -> object
      | otherwise -> search (a+8)

descriptorList :: (Word32 -> Word32) -> (Word32 -> Word32) -> Plane -> (Plane,Word32)
descriptorList f configuration p = (p { planeAddress=planeAddress p+planeStride p*(y-bytes `div` 4)+((x-bytes `div` 4) `div` 2)*4,planeLength=bytes*(bytes `div` 2),planeWidth=bytes,planeHeight=bytes `div` 2 },enlarged)
 where
  (width,x,y)
   | f 0x277c == 1 = let w=f 0x30ac+1-f 0x30a8 in (w,f 0x30a8+w `div` 2,f 0x30b0+(f 0x30b4+1-f 0x30b0) `div` 2)
   | otherwise = let key=if f 0x2778 == 1 then 0xa8ef else 0xa8e6 in ((planeWidth p .&. complement 1)*(configuration key*640 `div` 320) `div` 640,(f 0x194 `div` 2)*(configuration (key+1)*640 `div` 320) `div` 640,f 0x198*(configuration (key+2)*480 `div` 240) `div` 480)
  fallback=(width `div` 2)*4
  candidate=align 31 (width*3 `div` 2)
  choose n | n < width = (fallback,0)
           | fromIntegral (x-n `div` 2) >= (0::Int32) && x-1+n `div` 2 <= fallback `div` 2-1 && fromIntegral (y-n `div` 2) >= (0::Int32) && y-1+n `div` 2 <= planeHeight p-1 = (n*2,1)
           | otherwise = choose (n-32)
  (bytes,enlarged)=choose candidate

rotateCopyWords :: Plane -> Plane -> (Word32 -> Word32) -> [(Word32,Word32)]
rotateCopyWords source target memory = [(planeAddress target+row*targetAdvance+column*4,swap (memory (input-row*sourceAdvance-column*4))) | planeHeight source /= 0, wordsPerRow /= 0, row <- [0..planeHeight source-1], column <- [0..wordsPerRow-1]]
 where
  wordsPerRow=planeWidth source `div` 4
  gap=(planeStride source `div` 2-planeWidth source `div` 2)*2
  input=planeAddress source+(planeStride source `div` 2)*planeHeight source*2-4-gap
  sourceAdvance=wordsPerRow*4+(gap .&. complement 3)
  targetAdvance=wordsPerRow*4+((planeStride target `div` 2-planeWidth source `div` 2) .&. 0x7ffffffe)*2
  swap v=(v .&. 0xffff0000) .|. ((v .&. 255) `shiftL` 8) .|. ((v `shiftR` 8) .&. 255)

criticalInitial :: [Patch]
criticalInitial = priorityInitial 0x45 ++
  [word (0x9b6000+0x164) 0x8f0180,word (0x9b6000+0x168) 0x8f0184,word (0x9b6000+0x1624) 0x8f0188,word (0x9b6000+0x161c) 0x8f018c,word (0x9b6000+0x1620) 0x8f0190]
criticalEnterCalls :: [Call]
criticalEnterCalls = [endpointCall 0x8f0100 [] 0,endpointCall 0x8f0180 [] 0x123,endpointCall 0x8f0188 [0] 0x11]
criticalLeaveCalls :: [Call]
criticalLeaveCalls = [endpointCall 0x8f0100 [] 0,endpointCall 0x8f0188 [0] 0x11,endpointCall 0x8f0184 [0x123] 0]
criticalCalls :: [Call]
criticalCalls = criticalEnterCalls++criticalLeaveCalls
endpointCall :: Word32 -> [Word32] -> Word32 -> Call
endpointCall a args result=Call (Address a) (zip [N.R0,N.R1,N.R2,N.R3] args) [(N.R0,result)] []

data ObjectRecord = ObjectRecord { recordOwner :: Int16, recordGroup :: Word32, recordKind :: Word32, recordSlot :: Int16, recordPlane :: Plane } deriving (Eq,Show)
objectRecordWords :: ObjectRecord -> [Word32]
objectRecordWords r=[fromIntegral (fromIntegral (recordOwner r)::Word16),recordGroup r,recordKind r,fromIntegral (fromIntegral (recordSlot r)::Word16)]++planeWords (recordPlane r)
emptyObjectRecord :: ObjectRecord
emptyObjectRecord=ObjectRecord 0 0 0 0 (Plane 0 0 0 0 0 0 0)
lookupObjectRecord :: Int16 -> Word32 -> Word32 -> Int16 -> [ObjectRecord] -> ObjectRecord
lookupObjectRecord owner group kind slot entries=case filter matches (takeWhile ((/=0).recordOwner) entries) of r:_ -> r; _ -> emptyObjectRecord
 where matches r=recordGroup r==group && ((recordOwner r==(-1) && recordSlot r==slot) || (recordOwner r==owner && recordKind r==kind))
removeObjectRecord :: ObjectRecord -> [ObjectRecord] -> [ObjectRecord]
removeObjectRecord record entries=case break matches entries of
  (_,[]) -> entries
  (before,_:after) -> before++after++[emptyObjectRecord]
 where matches r=recordGroup r==recordGroup record && ((recordOwner r==(-1) && recordSlot r==recordSlot record) || (recordOwner r==recordOwner record && recordKind r==recordKind record))

objectGroupAddress :: Word32 -> Word32 -> Word32
objectGroupAddress mode group
 | group < 20 = 0xe48370+group*16
 | otherwise = 0xe4a948+(mode*73+group-20)*16

objectLookupRecords :: Int16 -> [(Word32,Word32,Word32,Plane)] -> [ObjectRecord]
objectLookupRecords owner bindings =
  [ObjectRecord owner group kind (fromIntegral (1+(address-base group) `div` 28)) plane | (group,kind,address,plane)<-bindings]
 where base group=minimum [address | (g,_,address,_)<-bindings,g==group]

objectLookupInitial :: Word32 -> Int16 -> Word32 -> [(Word32,Word32,Word32,Plane)] -> [Patch]
objectLookupInitial objects owner mode bindings = criticalInitial++
  [word objects (fromIntegral (fromIntegral owner::Word16)),word 0x15480d4 mode]++
  [wordsAt (objectGroupAddress mode group) [group+170,fromIntegral (maximum [recordSlot r | r<-records,recordGroup r==group]),0,minimum [address | (g,_,address,_)<-bindings,g==group]] | group<-nub [g | (g,_,_,_)<-bindings]]++
  [planePatch address plane | (_,_,address,plane)<-bindings]++
  [wordsAt base (concatMap objectRecordWords (take limit (selected++[emptyObjectRecord]))) | (base,limit,selected)<-[(0x154a9bc,20,filter ((<20).recordGroup) records),(0x154ad2c,128,filter ((>=20).recordGroup) records)]]
 where records=objectLookupRecords owner bindings

objectStoreEffects :: Int16 -> Word32 -> Word32 -> Plane -> [ObjectRecord] -> [Patch]
objectStoreEffects owner group kind plane records = case [index | (index,r)<-zip [0..] (filter ((>=20).recordGroup) records),recordOwner r==owner,recordGroup r==group,recordKind r==kind] of
  index:_ -> [planePatch (0x154ad2c+index*44+16) plane]
  [] -> []

configurationInitial :: Word32 -> [(Word32,Word8)] -> [Patch]
configurationInitial base values=word 0x96ca50 base:[Patch (Address (base+key)) (B.singleton value) | (key,value)<-values]

diagnosticEffects :: B.ByteString -> Word32 -> B.ByteString -> [Patch]
diagnosticEffects file line reason =
  [Patch (Address 0x15b5bc4) (copy15 (B.drop (lastSlash+1) file))
  ,Patch (Address 0x15b5bb4) (B.pack (map (fromIntegral . fromEnum) (replicate (15-length digits) '0'++digits)))
  ,Patch (Address 0x15b5bd4) (copy15 reason)]
 where
  lastSlash=maximum (0:B.elemIndices 47 file)
  copy15 bytes=B.take 15 (bytes<>B.replicate 15 0)
  digits=show line

buffersPublications :: [StagePublication]
buffersPublications =
  [StagePublication (Address address) (StageSymbol ("xm1_develop_"++name)) | (address,name) <- routes] ++
  [StagePublication (Address 0x69f8fc) (StageSymbol "xm1_buffers_configuration_byte"),StagePublication (Address 0x69fec8) (StageSymbol "xm1_buffers_configuration_half")]
 where routes =
        [(0xe9267c,"expand_planes"),(0xe927dc,"split_planes"),(0xe924f4,"fill_border"),(0xe92b28,"buffer_policy_disabled"),(0xe92b40,"output_matches"),(0xe92c48,"conversion_required"),(0xe92d68,"open_object"),(0xe92dd4,"close_object"),(0xe93570,"remap_object"),(0xe91a48,"descriptor_list"),(0xe8f4a4,"geometry"),(0xe90148,"dimensions"),(0xe91df4,"rotate_copy"),(0xeb03d0,"orientation"),(0xe90014,"fill_border_regions"),(0x734540,"lookup_width"),(0x73469c,"lookup_height"),(0x734394,"fit_aspect"),(0x734484,"fit_ratio"),(0xe91894,"operation_memory_size"),(0xeb0498,"report_error"),(0xe44d6c,"object_group"),(0xe44e58,"canonical_object_group"),(0xe45bac,"object_registry_lookup"),(0xe45ccc,"object_registry_insert"),(0xe45d78,"object_registry_remove"),(0xe46d40,"object_open"),(0xe46374,"object_close"),(0xe44b14,"find_object_entry_index"),(0xe44bcc,"store_object"),(0xe44dbc,"object_entry"),(0x7235d8,"enter_critical"),(0x7236b0,"leave_critical"),(0xe4586c,"trace_object_action"),(0xe456cc,"trace_object_text"),(0xe457c0,"trace_object_event")]
bufferPublication :: Word32 -> StagePublication
bufferPublication address = case filter ((==Address address) . stageStockEntry) buffersPublications of p:_ -> p; _ -> error "unknown buffer publication"

buffersScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
buffersScenarios image _resolve = expansionCases ++ dimensionCases ++ orientationCases ++ fitCases ++ geometryCases ++ descriptorCases ++ memoryCases ++ remapCases ++ borderCases ++ policyCases ++ lookupCases ++ activeBorderCases ++ rotationCases ++ configurationCases ++ diagnosticCases ++ objectCases ++ activeObjectCases ++ registryCases ++ criticalCases
 where
  make name address arguments initial final result calls = StageScenario (StageCaseName name) (bufferPublication address) arguments (word context state:initial) final result [] calls
  source=Plane 100 (params+0x3000) 9999 1025 641 481 0x12345678
  auxiliary=Plane 101 (params+0x8000) 7 888 64 32 0xabcdef01
  expansionCases = [make ("buffers-expand-"++show count++"-"++show parity) 0xe9267c [context,params,params+0x100,params+0x200,fromIntegral count,parity] [planePatch (params+0x100) source,planePatch (params+0x200) auxiliary] [wordsAt params (concatMap planeWords (expandPlanes count parity source auxiliary))] Nothing [] | count <- [0,1,2,3,4,7], parity <- [0,1,0xffffffff]] ++ [make ("buffers-split-"++show count) 0xe927dc [context,params,params+0x100,params+0x200,fromIntegral count] [planePatch (params+0x100) source,planePatch (params+0x200) auxiliary] [wordsAt params (concatMap planeWords (splitPlanes count source auxiliary))] Nothing [] | count <- [0,1,2,4,5,7]]
  dimensionCases = [let (w,h)=bufferDimensions profile aspect in make ("buffers-dimensions-"++show (profile,aspect,mode)) 0xe90148 [profile,aspect,mode,params,params+4] [] [wordsAt params [w,h]] Nothing [] | profile <- [0,2,3,5,0xffffffff], aspect <- [0..4], mode <- [0,1,2]]
  orientationCases = [make ("buffers-orientation-"++show (selection,orientation)) 0xeb03d0 [selection,orientation] [] [] (Just (bufferOrientation selection orientation)) [] | selection <- [0..5],orientation <- [0..5]]
  fitCases = [make ("buffers-fit-aspect-"++show (w,h,a)) 0x734394 [w,h,a,params] [] [wordsAt params (cropWords (fitAspect w h a))] Nothing [] | (w,h) <- [(640,480),(641,481),(100,900),(0,0)], a <- [0..4]] ++ [make ("buffers-fit-ratio-"++show (w,h,n,d)) 0x734484 [w,h,n,d,params] [] [wordsAt params (cropWords (fitRatio w h n d))] Nothing [] | (w,h,n,d) <- [(640,480,4,3),(641,480,16,9),(480,641,1,1),(0,0,4,3)]]
  geometryCases = [let (p,b)=bufferGeometry (\_ _ -> error "lookup not reached") source source 0 aspect capture 0 anchor in make ("buffers-geometry-"++show (aspect,capture,anchor)) 0xe8f4a4 [params+0x100,params,params+0x200,0,aspect,capture,0,anchor] [planePatch (params+0x100) source,planePatch params source] [planePatch params p,wordsAt (params+0x200) b] Nothing [] | aspect <- [0..3],capture <- [0,29],anchor <- [0,1]]
  descriptorCases =
    [ let fields=[(0x277c,1),(0x30a8,left),(0x30ac,right),(0x30b0,top),(0x30b4,bottom)]
          f o=maybe 0 id (lookup o fields)
          (p,e)=descriptorList f (const 0) source
       in make ("buffers-descriptor-region-"++show (left,right,top,bottom)) 0xe91a48 [context,params,params+0x200] (planePatch (state+0x184) source:[word (state+o) v | (o,v)<-fields]) [planePatch params p,word (params+0x200) e] (Just 0) []
    | (left,right,top,bottom) <- [(100,299,100,299),(0,199,0,199),(250,449,250,449)]
    ]
  memoryCases = [make ("buffers-memory-"++show (capture,group,kind)) 0xe91894 [context,group,kind] [word (state+0x4b4) capture] [] (Just (operationMemorySize image (\o -> if o == 0x4b4 then capture else 0) group kind)) [] | capture <- [0,29],group <- [0,33],kind <- [0,16]]
  remapCases = [make ("buffers-remap-"++show (capture,object)) 0xe93570 [context,object] [word (state+0x4b4) capture] [] (Just (remapObject image (\o -> if o == 0x4b4 then capture else 0) object)) [] | capture <- [0,25,26],object <- [0,1,20,93,0xffffffff]]
  borderCases = [make "buffers-border-null" 0xe924f4 [context,params,0,0,0] [planePatch params (source { planeAddress=0 })] [] (Just 1) [],make "buffers-border-regions-empty" 0xe90014 [11,6,params,0,params+0x200] [planePatch params source,wordsAt (params+0x200) [0,0,0,0]] [] (Just 0) []]
  policyCases = [make "buffers-policy-disabled" 0xe92b28 [] [] [] (Just 0) []] ++ [make ("buffers-conversion-special-"++show capture) 0xe92c48 [context,params,0] [word (state+0x4b4) capture,planePatch params source] [] (Just 1) [] | capture <- [29,32]] ++
    [ let p=Plane 100 0 38400 stride width height 0
          matches=stride==width && width==320 && height==120
       in make ("buffers-output-matches-"++show (stride,width,height)) 0xe92b40 [context,params,1,0] [word 0x9134f8 0,planePatch params p] [] (Just (if matches then 1 else 0)) []
    | (stride,width,height) <- [(320,320,120),(321,320,120),(320,319,120),(320,320,119)]
    ] ++
    [ make ("buffers-option-bypass-"++show entry) entry [context,0,0,0]
        [word 0x9134f8 1,word 0x96ca50 (params+0x400),Patch (Address (params+0x480)) (B.singleton 0xa5)] [] (Just result) []
    | (entry,result)<-[(0xe92b40,1),(0xe92c48,0)]
    ] ++
    [ let p=Plane 100 0 307200 640 640 480 flags
          fields=[(0x4b4,capture),(0x710,0),(0x644,0),(0x4f0,0),(0x550,recipe)]
          f o=maybe 0 id (lookup o fields)
          result=if conversionRequired 0 f (\_ _ -> (320,480)) p 0 then 1 else 0
       in make ("buffers-conversion-policy-"++show (capture,flags,recipe)) 0xe92c48 [context,params,0]
          ([word 0x9134f8 0,planePatch params p,word 0x816f60 320,word 0x8163a8 480]++[word (state+o) value | (o,value)<-fields])
          [] (Just result) []
    | capture<-[0,25],flags<-[0,0x2000000],recipe<-[0,151,152,153]
    ]
  lookupCases =
    [ make ("buffers-table-lookup-"++show (entry,profile,aspect)) entry [profile,aspect] [wordsAt (table+profile*16) [101,202,303,404]] [] (Just ([101,202,303,404]!!fromIntegral (min aspect 3))) []
    | (entry,table) <- [(0x734540,0x816f60),(0x73469c,0x8163a8)], profile <- [0,1,33], aspect <- [0,1,2,3,0xffffffff]
    ]
  activeBorderCases =
    [ let p=Plane 100 (params+0x400) 128 32 24 4 0
          regions=borderRegions p amounts
       in make ("buffers-border-active-"++show amounts) 0xe90014 [0x45,6,params,0,params+0x200]
            (priorityInitial 0x45++[planePatch params p,wordsAt (params+0x200) amounts,Patch (Address (params+0x400)) (B.replicate 256 0xa5)])
            (concat [fillEffects (planeAddress r) (planeWidth r) (planeStride r) (planeHeight r) 0x80800000 | r<-regions])
            (Just 0) (concatMap (const (priorityCalls 0x45 12++priorityCalls 0x45 6)) regions)
    | amounts <- [[1,0,0,0],[0,1,0,0],[0,0,2,0],[0,0,0,2],[1,1,2,2]]
    ] ++
    [ let p=Plane 100 (params+0x400) 128 32 24 4 0
          c=fitAspect 12 4 3
          regions=borderRegions p [cropTop c,cropBottom c,cropLeft c,cropRight c]
       in make "buffers-fill-border-active-square" 0xe924f4 [context,params,0,3,0]
          (priorityInitial 0x45++[word (context+0x34) 0x45,word (context+0x3c) 6,planePatch params p])
          (concat [fillEffects (planeAddress r) (planeWidth r) (planeStride r) (planeHeight r) 0x80800000 | r<-regions])
          (Just 1) (concatMap (const (priorityCalls 0x45 12++priorityCalls 0x45 6)) regions)
    ]
  rotationCases =
    [ let src=Plane 100 (params+0x400) (width*height) stride width height 0
          dst=Plane 100 (params+0x800) (width*height) (stride+4) width height 0
          values=[0x12345678+n*0x01020304 | n<-[0..63]]
          memory a=values!!fromIntegral ((a-planeAddress src) `div` 4)
       in make ("buffers-rotate-"++show (width,height,stride)) 0xe91df4 [state,params,params+0x100]
          (priorityInitial 0x45++[word 0x40a5b5ec 0,planePatch params src,planePatch (params+0x100) dst,wordsAt (planeAddress src) values])
          [word a value | (a,value)<-rotateCopyWords src dst memory] Nothing
          (currentTaskCalls 0x45++[cacheCall]++priorityCalls 0x45 13++priorityCalls 0x45 6++[cacheCall])
    | (width,height,stride) <- [(0,0,16),(2,2,16),(8,2,16),(12,3,18)]
    ]
  cacheCall=Call (Address 0x42634) [(N.R0,0),(N.R1,0)] [(N.R0,0)] []
  configurationCases =
    [ make ("buffers-configuration-byte-"++show (key,value)) 0x69f8fc [key]
        [word 0x96ca50 (params+0x400),Patch (Address (params+0x400+key)) (B.singleton value)] [] (Just (fromIntegral value)) []
    | key<-[0,0x80,0xa8ef],value<-[0,1,127,255]
    ]
  diagnosticCases =
    [ let file=B.pack (map (fromIntegral . fromEnum) path)
          reason=B.pack (map (fromIntegral . fromEnum) message)
       in make ("buffers-error-report-"++show (path,line,message)) 0xeb0498 [params,line,params+0x100,0]
          [Patch (Address 0xeb1750) (B.singleton 1),word 0x40a5b5ec 0,Patch (Address params) (file<>B.singleton 0),Patch (Address (params+0x100)) (reason<>B.singleton 0)]
          (diagnosticEffects file line reason) Nothing []
    | path<-["/recbkfrm.c","a/b/c","plain"],line<-[0,1,12345,0xffffffff],message<-["","recrOpen","long-error-label-that-is-truncated"]
    ] ++ [make "buffers-error-report-disabled" 0xeb0498 [0,0,0,0] [Patch (Address 0xeb1750) (B.singleton 0)] [] Nothing []]
  objectCases =
    [ make ("buffers-object-open-absent-"++show group) 0xe46d40 [state+0x7c,group,0,0,0]
        (criticalInitial++[word 0x15480d4 0,word (if group<20 then 0xe48370+group*16 else 0xe4a948+(group-20)*16) 0,wordsAt (if group<20 then 0x154a9bc else 0x154ad2c) (replicate 11 0)])
        [] (Just 0) criticalCalls
    | group<-[0,19,20,33,92]
    ] ++
    [ make ("buffers-object-close-absent-"++show group) 0xe46374 [state+0x7c,group,0,0]
        (criticalInitial++[wordsAt (if group<20 then 0x154a9bc else 0x154ad2c) (replicate 11 0)])
        [] (Just 0) criticalCalls
    | group<-[0,19,20,33,92]
    ]
  activeObjectCases =
    [ let binding=(1,0,params+0x400,source)
          record=ObjectRecord 7 1 0 1 source
          bitmap=0x15480f6
          initial=objectLookupInitial (state+0x7c) 7 3 [binding]++reservedInitial 171++
            [wordsAt 0x154a9bc (replicate 11 0),Patch (Address bitmap) (B.singleton 0),word 0x15480d8 0,word 0x40a5b5ec 0,Patch (Address 0xeb1750) (B.singleton 0)]
          calls=criticalCalls++reservedWaitCalls 171 0xffffffff result++
            (if result==0 then criticalCalls++criticalCalls else currentTaskCalls 0x45++[endpointCall 0x42188 [0xe498ac] 0])
          final=if result==0 then [wordsAt 0x154a9bc (objectRecordWords record),Patch (Address bitmap) (B.singleton 0x80)] else [wordsAt 0x154a9bc (replicate 11 0),Patch (Address bitmap) (B.singleton 0)]
       in make ("buffers-open-active-"++show (entry,result)) entry (if entry==0xe92d68 then [context,1,0,0] else [state+0x7c,1,0,0,0]) initial final (Just result) calls
    | entry<-[0xe92d68,0xe46d40],result<-[0,0xffffffce]
    ] ++
    [ make ("buffers-close-active-"++show entry) entry (if entry==0xe92dd4 then [context,1,0] else [state+0x7c,1,0,0])
        (objectLookupInitial (state+0x7c) 7 3 [(1,0,params+0x400,source)]++reservedInitial 171++[Patch (Address 0x15480f6) (B.singleton 0x80)])
        [wordsAt 0x154a9bc (replicate 11 0),Patch (Address 0x15480f6) (B.singleton 0)]
        (Just 0) (criticalCalls++criticalCalls++reservedSignalCalls 171 0 0++criticalCalls)
    | entry<-[0xe92dd4,0xe46374]
    ]
  registryCases =
    [ make ("buffers-object-entry-"++show (group,kind)) 0xe44dbc [state+0x7c,group,kind]
        (objectLookupInitial (state+0x7c) 7 0 [(group,kind,params+0x400,source)])
        [] (Just (params+0x400)) criticalCalls
    | group<-[1,19,20,55,92],kind<-[0,1,5,9]
    ] ++
    [ let bindings=[(33,0,params+0x400,source),(33,1,params+0x400,source)]
          records=objectLookupRecords 7 bindings
       in make "buffers-store-registry-plane" 0xe44bcc [state+0x7c,33,1,params]
          (objectLookupInitial (state+0x7c) 7 0 bindings++[planePatch params auxiliary])
          (objectStoreEffects 7 33 1 auxiliary records) Nothing criticalCalls
    ] ++
    [ make ("buffers-configuration-half-"++show value) 0x69fec8 [0x140]
        (configurationInitial (params+0x400) [(0x140,fromIntegral value),(0x141,fromIntegral (value `shiftR` 8))])
        [] (Just value) []
    | value<-[0,1,0x1280,0xffff]
    ]
  criticalCases =
    [ make ("buffers-critical-enter-owner-"++show owner) 0x7235d8 []
        (criticalInitial++[word 0x40a0f824 2])
        [word 0x40a0f824 (if owner==0x11 then 2 else 3)] (Just 0x123)
        ([endpointCall 0x8f0100 [] 0,endpointCall 0x8f0180 [] 0x123,endpointCall 0x8f0188 [0] owner]++
          [endpointCall 0x8f0188 [0] owner | owner/=0x11]++
          [endpointCall 0x8f018c [0,0x21] result | owner==0,result<-[1,0]])
    | owner<-[0x11,0x21,0]
    ] ++
    [ make ("buffers-critical-leave-"++show (owner,nesting)) 0x7236b0 [0x123]
        (criticalInitial++[word 0x40a0f824 nesting])
        [word 0x40a0f824 (if owner==0x11 || nesting==0 then nesting else nesting-1)] Nothing
        ([endpointCall 0x8f0100 [] 0,endpointCall 0x8f0188 [0] owner]++
          [endpointCall 0x8f0190 [0] 0 | owner/=0x11,nesting==1]++
          [endpointCall 0x8f0184 [0x123] 0])
    | owner<-[0x11,0x21],nesting<-[0,1,3]
    ]
