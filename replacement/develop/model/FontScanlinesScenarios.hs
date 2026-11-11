module FontScanlinesScenarios where

import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word (Word8, Word16, Word32)
import PipelineModel
import Scenarios

fontScanlinesPublication :: StagePublication
fontScanlinesPublication = StagePublication (Address 0x011a8818) (StageSymbol "xm1_pipeline_image_font_raster_flush_scanlines")
fontScanlinesPublications :: [StagePublication]
fontScanlinesPublications = [fontScanlinesPublication]

newtype VmAddress = VmAddress Word32
newtype BitmapAddress = BitmapAddress Word32
newtype RunStorage = RunStorage Word32
newtype RunCapacity = RunCapacity Word16
newtype RenderStyle = RenderStyle Word32

vmAddress :: VmAddress
vmAddress = VmAddress 0x009c00b8
bitmapAddress :: BitmapAddress
bitmapAddress = BitmapAddress 0x009d0000
runStorage :: RunStorage
runStorage = RunStorage 0x009d1000

halfPatch :: Word32 -> [Int32] -> Patch
halfPatch address values = Patch (Address address) (B.pack (concatMap encode values))
 where encode value = let half=fromIntegral value::Word16 in [fromIntegral half,fromIntegral (half `shiftR` 8)]

initial :: RenderStyle -> Word8 -> [Patch]
initial (RenderStyle style) fill =
 [Patch (Address (vm-0xb8)) (B.replicate 0x9000 0),word (vm+8) (vm-0xb8)
 ,wordsAt (vm+0x54) [8,8,style,1,0,bitmap,1],wordsAt (vm+0x74) [0x7fff,0x7fff,0,0]
 ,word (vm+0xb8) 2,word (vm+0x804c) 2,halfPatch (vm+0xbc) [2,3],halfPatch (vm+0x32bc) [4,5]
 ,Patch (Address bitmap) (B.replicate 8 fill)]
 where VmAddress vm=vmAddress; BitmapAddress bitmap=bitmapAddress

runInitial :: RunCapacity -> [Patch]
runInitial (RunCapacity capacity) =
 [word (vm+0x84) 1,wordsAt table [0,storage+32,32+6*fromIntegral capacity]
 ,halfPatch (table+12) [0,7,8,0],wordsAt (table+20) [storage,storage+16]
 ,halfPatch (table+28) [fromIntegral capacity,0,-1]
 ,Patch (Address storage) (B.replicate (32+6*fromIntegral capacity) 255)]
 where VmAddress vm=vmAddress; RunStorage storage=runStorage; table=vm-0xb8+0x83b0

fontScanlinesScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
fontScanlinesScenarios _ _ = bitmapCases ++ orientationCases ++ runCases ++ controlCases
 where
  VmAddress vm=vmAddress
  BitmapAddress bitmap=bitmapAddress
  RunStorage storage=runStorage
  make name patches final result = StageScenario (StageCaseName name) fontScanlinesPublication [vm] patches final (Just result) [] []
  bitmapResult bytes bounds = [Patch (Address bitmap) (B.pack bytes),wordsAt (vm+0x74) bounds]
  bitmapCases =
   [make ("font-scanlines-style-"++show style) (initial (RenderStyle style) fill) (bitmapResult bytes bounds) 1
   | (style,fill,bytes,bounds)<-
      [(0,0,[0,0,0x38,0x1c,0,0,0,0],[2,2,5,3])
      ,(1,255,[255,255,0xc7,0xe3,255,255,255,255],[0x7fff,0x7fff,0,0])
      ,(2,0,[0,0x7c,0x7e,0x7e,0x3e,0,0,0],[1,1,6,4])
      ,(3,0,[0,0,0x3c,0x1e,0,0,0,0],[2,2,6,3])
      ,(4,0,[0,0,0x38,0x3c,0x1c,0,0,0],[2,2,5,4])
      ,(5,0,[0,0x38,0x3c,0x1c,0,0,0,0],[2,1,5,3])
      ,(6,0,[0,0,0x3c,0x3e,0x1e,0,0,0],[2,2,6,4])
      ,(7,0,[0,0x3c,0x3e,0x1e,0,0,0,0],[2,1,6,3])]]
  orientationCases =
   [make "font-scanlines-row-shear" (initial (RenderStyle 0) 0++[word (vm+0x64) 2])
    (bitmapResult [0,0,0x1c,0x0e,0,0,0,0] [3,2,6,3]) 1
   ,make "font-scanlines-column-shear" (initial (RenderStyle 0) 0++[word (vm+0x64) 2,Patch (Address (vm+0x70)) (B.pack [1,0])])
    (bitmapResult [0,0,0x30,0x18,0x0c,0,0,0] [2,2,5,4]) 1
   ,make "font-scanlines-rotated" (initial (RenderStyle 0) 0++[Patch (Address (vm+0x71)) (B.singleton 1)])
    (bitmapResult [0,0,4,12,12,8,0,0] [4,2,5,5]) 1
   ,make "font-scanlines-rotated-row-shear" (initial (RenderStyle 0) 0++[word (vm+0x64) 2,Patch (Address (vm+0x71)) (B.singleton 1)])
    (bitmapResult [0,0,0,4,12,12,8,0] [4,3,5,6]) 1
   ,make "font-scanlines-clip-and-origin" (initial (RenderStyle 0) 0++[wordsAt (vm+0x44) [1,1,2,1]])
    (bitmapResult [0,0,0x1c,0x0e,0,0,0,0] [3,2,6,3]) 1]
  runCases =
   [make ("font-scanlines-run-capacity-"++show capacity)
    (initial (RenderStyle 0) 0++runInitial (RunCapacity capacity))
    ([Patch (Address bitmap) (B.replicate 8 0)]++final) result
   | (capacity,final,result)<-
     [(0,[halfPatch (vm-0xb8+0x83b0+30) [0],halfPatch storage [-1,-1,-1,-1,-1,-1,-1,-1]],0)
     ,(1,[halfPatch storage [-1,-1,0,-1,-1,-1,-1,-1],halfPatch (storage+16) [-1,-1,0,-1,-1,-1,-1,-1]
          ,halfPatch (storage+32) [2,4,-1],halfPatch (vm-0xb8+0x83b0+30) [1]],0)
     ,(2,[halfPatch storage [-1,-1,0,1,-1,-1,-1,-1],halfPatch (storage+16) [-1,-1,0,1,-1,-1,-1,-1]
          ,halfPatch (storage+32) [2,4,-1,3,5,-1],halfPatch (vm-0xb8+0x83b0+30) [2]],1)]] ++
   [make "font-scanlines-outline-retains-capacity-return" (initial (RenderStyle 2) 0++runInitial (RunCapacity 0))
    [Patch (Address bitmap) (B.replicate 8 0),halfPatch (vm-0xb8+0x83b0+30) [0]
    ,halfPatch (vm+0xbc) [1,2],halfPatch (vm+0x32bc) [5,6]] 1]
  controlCases =
   [make ("font-scanlines-unknown-style-"++show style) (initial (RenderStyle style) 0)
    (bitmapResult (replicate 8 0) [0x7fff,0x7fff,0,0]++[halfPatch (vm+0xbc) [2,3],halfPatch (vm+0x32bc) [4,5]]) 1
   | style<-[8,0xffffffff]] ++
   [make "font-scanlines-no-rows" (initial (RenderStyle 0) 0++[word (vm+0xb8) 0])
    (bitmapResult (replicate 8 0) [0x7fff,0x7fff,0,0]) 1]
