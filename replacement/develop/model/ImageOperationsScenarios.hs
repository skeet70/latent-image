{-# LANGUAGE GHC2021 #-}
module ImageOperationsScenarios where

import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word
import qualified Emulation.NativeObservation as N
import qualified Data.Map.Strict as M
import PipelineModel
import ImagePlanesScenarios (priorityInitial, currentTaskCalls, waitCalls)
import BuffersScenarios (criticalInitial, criticalCalls)
import Scenarios (Address(..), Patch(..), Call(..), MemoryContract(..), word, wordsAt, wordBytes, readWord, call)

data GeometryPlane = GeometryPlane
  { geometryAddress :: Word32
  , geometryStride :: Word32
  , geometryWidth :: Word32
  , geometryHeight :: Word32
  } deriving (Eq, Show)

data OperationRequest = OperationRequest
  { operationKind :: Word32
  , operationArgument :: Word32
  , operationTable :: Word32
  , operationCoefficients :: Word32
  , operationKindEight :: Word32
  , operationKindNine :: Word32
  , operationParameters :: Word32
  } deriving (Eq, Show)

operationRequestWords :: B.ByteString -> OperationRequest -> [Maybe GeometryPlane] -> Word32 -> GeometryPlane -> Word32 -> [Word32]
operationRequestWords image request sources count destination completed =
  [kind] ++ centered ++ addresses ++ strides ++ heights ++
  [geometryWidth first,geometryAddress destination,geometryStride destination,geometryWidth destination,geometryHeight destination,0,count,completed,
   operationArgument request,operationTable request,operationCoefficients request,operationKindEight request,operationKindNine request,operationParameters request]
 where
  kind=operationKind request
  mode=readWord image (0xda93c8+kind*40+8)
  isCentered=mode `elem` [1,3,4]
  first=case sources of Just p:_ -> p; _ -> error "operation requires the first source descriptor"
  selected=[if fromIntegral i<count || (i==3 && mode==3) then p else Nothing | (i,p)<-zip [0::Int ..4] (take 5 (sources++repeat Nothing))]
  centered=[case p of Just plane | isCentered && i<4 -> geometryAddress plane+(geometryWidth plane `shiftR` 1)+geometryStride plane*(geometryHeight plane `shiftR` 1); _ -> 0 | (i,p)<-zip [0::Int ..3] selected]
  addresses=[case p of Just plane | not isCentered -> geometryAddress plane; Just _ | i==4 -> error "fifth centered input is not supported by stock"; _ -> 0 | (i,p)<-zip [0::Int ..4] selected]
  strides=map (maybe 0 geometryStride) selected
  heights=map (maybe 0 geometryHeight) selected

operationRequestPatch :: B.ByteString -> OperationRequest -> [Maybe GeometryPlane] -> Word32 -> GeometryPlane -> Word32 -> Patch
operationRequestPatch image request sources count destination completed = wordsAt 0x1575844 (operationRequestWords image request sources count destination completed)

data ConversionParameters = ConversionParameters
  { conversionMode :: Word32
  , conversionBlockSize :: Word32
  , conversionValue :: Word32
  , conversionAlternate :: Word32
  , conversionOffset :: Word32
  } deriving (Eq, Show)

conversionParameterWords :: ConversionParameters -> [Word32]
conversionParameterWords p=[conversionMode p,conversionBlockSize p,conversionValue p,conversionAlternate p,conversionOffset p]++replicate 6 0

conversionCommand :: ConversionParameters -> [Word32]
conversionCommand p
 | conversionMode p /= 3 = error "conversion mode enters the stock fail-stop loop"
 | otherwise = [1,conversionValue p,secondary,0,0,blockShift,3,0,conversionOffset p,0,0,0,0,0,0,1-secondary]
 where
  secondary=if conversionAlternate p==0 then 0 else 1
  blockShift=case conversionBlockSize p of 16->4;8->3;4->2;2->1;_->0

conversionConfigureCalls :: ConversionParameters -> [Call]
conversionConfigureCalls p=
 [ScriptCall (Address (if conversionAlternate p==0 then 0x68303c else 0x683408)) [] []
   [Relative N.R0 0 (B.concat (map wordBytes (conversionCommand p)))] [],call 0x6837d4 [1] 0]

conversionInitializeCalls :: [Call]
conversionInitializeCalls=[call 0x681478 [] 0,call 0x68157c [] 0]
conversionBeginCalls :: Word32 -> Word32 -> [Call]
conversionBeginCalls first second=[call 0x42634 [0,0] 0,call 0x6815b8 [first,second] 0]
conversionWaitCalls :: Word32 -> Word32 -> [Call]
conversionWaitCalls active result=criticalCalls++[call 0x68157c [] 0]++[call 0x64eb20 [0x123] result | active/=0]

radialMask :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [[Word8]]
radialMask width height normalWidth normalHeight strength shiftAmount=
 [[fromIntegral (min 255 (shiftValue (factor*(square x (width `shiftR` 1)+square y (height `shiftR` 1))))) | x<-[0..width]] | y<-[0..height]]
 where
  factor=(fromIntegral (strength .&. 255)*10000000 :: Word64) `div` fromIntegral (normalWidth*normalWidth+normalHeight*normalHeight)
  square position center=let delta=fromIntegral (fromIntegral position::Int32)-fromIntegral center::Integer; value=delta*delta in fromIntegral value::Word64
  shiftValue value
   | (fromIntegral (shiftAmount-32)::Int32)>=0 = lsr high (shiftAmount-32)
   | otherwise = lsr low shiftAmount .|. lsl high (32-shiftAmount)
   where
    low=fromIntegral value::Word32
    high=fromIntegral (value `shiftR` 32)::Word32
    lsr bits count=if count .&. 255<32 then bits `shiftR` fromIntegral (count .&. 255) else 0
    lsl bits count=if count .&. 255<32 then bits `shiftL` fromIntegral (count .&. 255) else 0

imageOperationsPublications :: [StagePublication]
imageOperationsPublications=[StagePublication (Address entry) (StageSymbol ("xm1_pipeline_image_"++name)) | (entry,name)<-
 [(0xe5ff7c,"fill_chroma"),(0xe5ff9c,"split_four"),(0xe601bc,"split_thirteen"),(0xe6043c,"split_five"),(0xe605b0,"merge_finish"),
  (0xe607e0,"select_three"),(0xe60948,"merge"),(0xe60a2c,"difference"),(0xe60be8,"blend_planes"),(0xe60cc4,"blend_secondary"),
  (0xe60d9c,"blend_finish"),(0xe60e5c,"finish_primary"),(0xe60f34,"finish_secondary"),(0xe6100c,"select_five"),(0xe61fc0,"prepare_three"),
  (0xe62330,"prepare_four"),(0xe62bc4,"select_thirteen"),(0xe63008,"select_four"),(0xe63544,"prepare"),(0xe63b0c,"prepare_finish"),
  (0xe53f90,"integer_root"),(0xe6399c,"next_noise"),(0xe63e34,"dma_complete"),(0xe63e68,"operation_complete"),(0xe65cc8,"fill_word"),(0xda5edc,"conversion_complete"),
  (0xe65f1c,"pad_edges"),(0xe657ac,"operate"),(0xda5f14,"conversion_finish_primary"),(0xda5f24,"conversion_finish_secondary"),
  (0xda5f34,"conversion_acquire"),(0xda5f4c,"conversion_release"),(0xda5f5c,"conversion_begin"),(0xda5f88,"conversion_configure"),
  (0xda608c,"conversion_submit"),(0xda6158,"conversion_initialize"),(0xda6184,"conversion_wait"),(0xda61e8,"conversion_get_parameters"),(0xda6208,"conversion_prepare_mode")]]

imageOperationsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
imageOperationsScenarios _ addressOf = initializeCases++parameterCases++beginCases++configureCases++waitCases++paddingCases++dmaCases++rootCases++randomCases
 where
  publication name=case [p|p<-imageOperationsPublications,stageOwnedSymbol p==StageSymbol ("xm1_pipeline_image_"++name)] of p:_->p;_->error "unpublished image operation"
  make name entry arguments initial final result calls=StageScenario (StageCaseName name) (publication entry) arguments initial final result [] calls
  initializeCases=[make "conversion-initialize-clears-retained-state" "conversion_initialize" [] [wordsAt 0x14ce454 (replicate 17 0xdeadbeef)] [wordsAt 0x14ce454 (replicate 17 0)] (Just 1) conversionInitializeCalls]
  parameterCases=[make "conversion-get-parameters" "conversion_get_parameters" [0x9a1000] [word 0x9a1000 0] [word 0x9a1000 0x14ce45c] Nothing []]
  beginCases=[make ("conversion-begin-"++show first++"-"++show second) "conversion_begin" [first,second] [] [] Nothing (conversionBeginCalls first second) | first<-[0,1,3],second<-[0,2,3]]
  configureCases=[let p=ConversionParameters 3 block 0x123 alternate 0x9c0000 in make ("conversion-configure-"++show block++"-"++show alternate) "conversion_configure" [0x9a1000] [wordsAt 0x9a1000 (conversionParameterWords p)] [] (Just 0) (conversionConfigureCalls p) | block<-[0,1,2,4,8,16,32],alternate<-[0,1,0xffffffff]]
  waitCases=[make ("conversion-wait-"++show active++"-"++show result) "conversion_wait" [] (criticalInitial++[word 0x14ce454 0x123,word 0x14ce458 active,word 0x14ce494 0xdeadbeef]) [word 0x14ce494 0,word 0x14ce458 (if active/=0 && result==0 then 0 else active)] (Just (if active==0 then 0 else result)) (conversionWaitCalls active result) | active<-[0,1,2],result<-[0,7]]
  paddingCases=[let plane=GeometryPlane 0x9c0800 40 16 3; sample a=fromIntegral ((a*37+19) .&. 255); start=0x9c0000; size=0x1000
                 in make ("packed-edge-pixels-"++show left++"-"++show right++"-"++show top++"-"++show bottom) "pad_edges"
                     [geometryAddress plane,geometryStride plane,geometryWidth plane,geometryHeight plane,left,right,top,bottom]
                     [Patch (Address start) (B.pack [sample a|a<-[start..start+size-1]])]
                     (packedEdgeEffects sample plane left right top bottom) Nothing [call 0x42634 [0,0] 0,call 0x42634 [0,0] 0]
               | (left,right,top,bottom)<-[(0,0,0,0),(2,6,1,2),(8,8,2,0),(6,2,0,2)]]
  dmaCases=[let plane=GeometryPlane address stride width height; Address callback=addressOf (publication "dma_complete"); value=0xabcdef13
             in make ("dma-word-fill-"++show width++"-"++show stride++"-"++show height) "fill_word" [0x9a1000,value]
                 (dmaFillInitial++[wordsAt 0x9a1000 [2,address,stride*height,stride,width,height,0],Patch (Address 0x9c0000) (B.replicate 0x2000 0x55)])
                 (dmaFillEffects plane value++[wordsAt 0x4157a0e0 (replicate 8 value),word 0x157bd0c 0]) (Just 1) (dmaFillCalls plane value callback)
           | (address,width,stride,height)<-[(0x9c0800,32,32,3),(0x9c0804,20,32,2),(0x9c0808,24,40,3),(0x9c0800,16,32,0)]]
  rootCases=[make ("integer-root-"++show input) "integer_root" [input] [] [] (Just expected) [] | (input,expected)<-[(0,0),(1,1),(2,1),(3,1),(4,2),(15,3),(16,4),(10000,100)]]
  randomCases=[make "xorshift-mutates-retained-state" "next_noise" []
               [word 0xe63e30 0x12345678,word 0xe63d24 0x98765432,word 0xe63e28 0xabcdef01,word 0xe63e2c 0xfedcba98]
               [word 0xe63e30 0x98765432,word 0xe63d24 0xabcdef01,word 0xe63e28 0xfedcba98,word 0xe63e2c next] (Just next) []]
  next=let x=0x12345678;w=0xfedcba98;mixed=x `xor` (x `shiftL` 11) in w `xor` (w `shiftR` 19) `xor` mixed `xor` (mixed `shiftR` 8)

data LinearFrontend = LinearFrontend
  { frontendSources :: [Maybe GeometryPlane]
  , frontendSourceCount :: Word32
  , frontendCoefficients :: [Int32]
  , frontendDestination :: GeometryPlane
  } deriving (Eq,Show)

linearFrontend :: GeometryPlane -> GeometryPlane -> GeometryPlane -> [Int32] -> Bool -> LinearFrontend
linearFrontend source second destination coefficients reversed =
 LinearFrontend (map Just (if reversed then [second,source] else [source,second])++replicate 3 Nothing) 2 coefficients
  destination {geometryWidth=geometryWidth source,geometryHeight=geometryHeight source}

centerPlane :: Word32 -> Word32 -> GeometryPlane -> GeometryPlane
centerPlane width height plane=plane
 {geometryAddress=geometryAddress plane+((geometryWidth plane-width*2) `shiftR` 1)+((geometryStride plane*(geometryHeight plane-height)) `shiftR` 1),
  geometryWidth=width*2,geometryHeight=height}

gradientMask :: Word32 -> Word32 -> Word32 -> Int32 -> Int32 -> Int32 -> Int32 -> Word8 -> Bool -> [[Word8]]
gradientMask width height pivot slopeFirst slopeSecond offsetFirst offsetSecond maximumValue transposed=
 [[value (if transposed then x else y) | x<-[0..width]] | y<-[0..height]]
 where
  extent=if transposed then width else height
  value position=fromIntegral (max 0 (min (fromIntegral maximumValue) signed))
   where
    numerator=if position<=pivot then fromIntegral slopeFirst*(pivot-position) else fromIntegral slopeSecond*(position-pivot)
    offset=if position<=pivot then offsetFirst else offsetSecond
    signed=fromIntegral (numerator `div` extent)+offset::Int32

packedEdgeEffects :: (Word32 -> Word8) -> GeometryPlane -> Word32 -> Word32 -> Word32 -> Word32 -> [Patch]
packedEdgeEffects initial plane left right top bottom=[Patch (Address location) (B.singleton byte) | (location,byte)<-M.toAscList final]
 where
  address=geometryAddress plane
  width=geometryWidth plane
  stride=geometryStride plane .&. complement 7
  wordStride=geometryStride plane .&. complement 3
  height=geometryHeight plane
  fetch memory position=M.findWithDefault (initial position) position memory
  put memory position values=foldl (\m (a,v)->M.insert a v m) memory (zip [position..] values)
  copy memory source destination=put memory destination [fetch memory (source+i)|i<-[0..7]]
  columnCount=fromIntegral ((fromIntegral width::Int32) `quot` 8)::Int32
  vertical=foldl column M.empty [fromIntegral block | block<-[0..columnCount]]
  column memory block=foldl (copyBottom block) (foldl (copyTop block) memory [1..top]) (if bottom==0 then [] else [0..bottom-1])
  copyTop block memory row=copy memory (address+block*8) (address+block*8-stride*row)
  copyBottom block memory row=copy memory (address+stride*(height-1)+block*8) (address+stride*(height+row)+block*8)
  upper=if top==0 then bottom else top
  lower=if bottom==0 then top else bottom
  final=foldl horizontal vertical [negate (fromIntegral upper::Int32)..fromIntegral (height+lower)-1]
  horizontal memory signedRow=rightPartial (leftPartial rightFull)
   where
    row=fromIntegral signedRow
    start=address+stride*row
    wordStart=address+wordStride*row
    lastBlock=start+((width `shiftR` 3)-1)*8
    leftBytes=[fetch memory start,fetch memory start,fetch memory (start+2),fetch memory (start+3)]
    rightBytes=[fetch memory (lastBlock+5),fetch memory (lastBlock+5),fetch memory (lastBlock+6),fetch memory (lastBlock+7)]
    leftFull=foldl (\m block->put m (start-(block+1)*8) (leftBytes++leftBytes)) memory (if left<4 then [] else [0..(left `shiftR` 2)-1])
    rightFull=foldl (\m block->put m (lastBlock+(block+1)*8) (rightBytes++rightBytes)) leftFull (if right<4 then [] else [0..(right `shiftR` 2)-1])
    leftPartial m=if left .&. 3==0 then m else put m (wordStart-((left `shiftR` 2)+1)*8+4) leftBytes
    rightPartial m=if right .&. 3==0 then m else put m (wordStart+(width `shiftR` 2)*4+(right `shiftR` 2)*8) rightBytes

dmaFillEffects :: GeometryPlane -> Word32 -> [Patch]
dmaFillEffects plane value=
 [Patch (Address address) (B.take (fromIntegral bytes) (B.concat (replicate (fromIntegral (bytes `div` 4)) (wordBytes value)))) | address<-destinations]
 where
  contiguous=geometryStride plane==geometryWidth plane
  bytes=(if contiguous then geometryWidth plane*geometryHeight plane else geometryWidth plane) .&. complement 3
  destinations=if contiguous then [geometryAddress plane] else [geometryAddress plane+row*geometryStride plane | row<-if geometryHeight plane==0 then [] else [0..geometryHeight plane-1]]

dmaFillInitial :: [Patch]
dmaFillInitial=priorityInitial 0x45++[word 0x4089ebec 0]

dmaFillCalls :: GeometryPlane -> Word32 -> Word32 -> [Call]
dmaFillCalls plane value completed=
 [call 0x42188 [0xe66728] 0,call 0x42188 [0xe66734] 0,
  ScriptCall (Address 0x665304) [(N.R0,5)] [(N.R0,0)] [] [Relative N.R1 0 (wordBytes channel)]]
 ++ concatMap transfer destinations ++
 [call 0x665424 [channel] 0,call 0x42188 [0xe6673c] 0,call 0x42188 [0xe66744] 0]
 where
  channel=0x123
  contiguous=geometryStride plane==geometryWidth plane
  totalBytes=if contiguous then geometryWidth plane*geometryHeight plane else geometryWidth plane
  bytes=totalBytes .&. complement 3
  wordCount=totalBytes `shiftR` 2
  destinations=if contiguous then [geometryAddress plane] else [geometryAddress plane+row*geometryStride plane | row<-if geometryHeight plane==0 then [] else [0..geometryHeight plane-1]]
  transfer destination=
   let burst | destination .&. 31==0 && wordCount .&. 7==0 = 5
             | destination .&. 15==0 && wordCount .&. 3==0 = 4
             | destination .&. 7==0 && wordCount .&. 1==0 = 3
             | otherwise = 2
       control=0x00500400 .|. (burst `shiftL` 12) .|. (burst `shiftL` 16)
       request=[control,0,0,0x157a0e0,destination,bytes,0,0,0,0,0]
       pixels=Patch (Address destination) (B.concat (replicate (fromIntegral wordCount) (wordBytes value)))
    in [ScriptCall (Address 0x665ac4) [(N.R0,channel),(N.R1,0x157b9d0)] [(N.R0,0)] [Absolute (wordsAt 0x157b9d0 request)] []]
       ++ currentTaskCalls 0x45
       ++ [ScriptCall (Address 0x665540) [(N.R0,channel),(N.R1,0x157b9fc)] [(N.R0,0)] [Absolute (wordsAt 0x157b9fc [4,completed])] [Absolute pixels]]
       ++ waitCalls 0x45 0x10 0 0
       ++ [call 0x665760 [channel] 0]
