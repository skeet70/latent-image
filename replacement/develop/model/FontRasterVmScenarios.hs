{-# LANGUAGE GHC2021 #-}
module FontRasterVmScenarios (fontRasterVmPublications, fontRasterVmScenarios, RasterFixtureGlyph (..), rasterFixtureGlyphCode) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import FontFdlScenarios (FontFixture (..), StockFontGlyph (..), fontFixture, fontRasterInitial, fontRasterCodePointers, fontRectangleRasterInitial, stockFontCode)
import PipelineModel
import Scenarios

data RasterFixtureGlyph = RectangleA deriving (Eq, Show)

rasterFixtureGlyphCode :: RasterFixtureGlyph -> Word32
rasterFixtureGlyphCode RectangleA = 65

fontRasterVmPublications :: [StagePublication]
fontRasterVmPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_font_" ++ name)) | (address,name) <-
  [(0x119f60c,"raster_read_byte"),(0x119f644,"raster_read_short"),(0x119f68c,"raster_read_stream")
  ,(0x11a05b0,"render_raster_program"),(0x11a0fa4,"raster_render_bitmap"),(0x11a344c,"raster_fill_span")
  ,(0x11a36a8,"raster_edge_min"),(0x11a36d4,"raster_edge_max"),(0x11a3c00,"raster_accumulate_line_edge"),(0x11a40b4,"raster_draw_curve")
  ,(0x11abdfc,"raster_merge_span"),(0x11ac190,"raster_subtract_run"),(0x11ac364,"raster_run_row_bounds")
  ,(0x11ac410,"raster_run_first"),(0x11ac4b0,"raster_run_next"),(0x11ac4e0,"raster_run_storage_used"),(0x11ac50c,"raster_run_table_init")]]

publication :: Word32 -> StagePublication
publication address = case filter ((== Address address) . stageStockEntry) fontRasterVmPublications of
  [entry] -> entry
  _ -> error "missing raster publication"

fontRasterVmScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
fontRasterVmScenarios _ _ = readerCases ++ runCases ++ spanCases ++ edgeCases ++ curveCases ++ programCases ++ bitmapCases ++ genuineCases
 where
  vmContext = 0x009c0000
  vm = vmContext + 0xb8
  table = 0x009d0000
  workspace = 0x009d1000
  heads = workspace
  tails = workspace + 6
  nodes = workspace + 12
  output = 0x009e0000
  dataAddress = 0x009f0000
  edge = 0x009e1000
  zero at size = Patch (Address at) (B.replicate size 0)
  bytes at values = Patch (Address at) (B.pack values)
  shorts at values = bytes at (concatMap (\value -> [fromIntegral value,fromIntegral (value `div` 256)]) (values :: [Int]))
  scenario name address arguments initial final returned = StageScenario (StageCaseName name) (publication address) arguments initial final returned [] []
  readerCases =
    [scenario "raster-byte-cursor-and-budget" 0x119f60c [vmContext]
      [word (vmContext+0x48) dataAddress,wordsAt (vmContext+0xf4) [3,2],bytes dataAddress [10,20,37,40]]
      [wordsAt (vmContext+0xf4) [2,3]] (Just 37)
    ,scenario "raster-short-little-endian" 0x119f644 [vmContext]
      [word (vmContext+0x48) dataAddress,wordsAt (vmContext+0xf4) [3,1],bytes dataAddress [99,0x23,0x81,77]]
      [wordsAt (vmContext+0xf4) [1,3]] (Just 0xffff8123)
    ,scenario "raster-stream-secondary" 0x119f68c [vmContext,2]
      [word (vmContext+0x48) dataAddress,wordsAt (vmContext+0xa0) [9,7,0,1,4,1],bytes dataAddress [99,0x12,0x34,77]]
      [wordsAt (vmContext+0xa0) [9,7],wordsAt (vmContext+0xb0) [2,3]] (Just 0x3412)
    ,scenario "raster-zero-count-still-reads-preceding-byte" 0x119f68c [vmContext,0]
      [word (vmContext+0x48) dataAddress,wordsAt (vmContext+0xa0) [9,2,0,0],bytes dataAddress [99,0x76,0x54]]
      [wordsAt (vmContext+0xa0) [9,2]] (Just 0x76)]
  runTable used capacity firstNode lastNode =
    [wordsAt table [0,nodes,40],shorts (table+12) [-1,1,3],wordsAt (table+20) [heads,tails]
    ,shorts (table+28) [capacity,used,65535],shorts heads [65535,firstNode,65535],shorts tails [65535,lastNode,65535]]
  runCases =
    [scenario "raster-run-init-leaves-tail-storage" 0x11ac50c [table,0xffffffff,1,workspace,40,7]
      [Patch (Address table) (B.replicate 36 0xaa),Patch (Address workspace) (B.replicate 40 0x55)]
      [wordsAt table [7,nodes,40],shorts (table+12) [-1,1,3],wordsAt (table+20) [heads,tails],shorts (table+28) [4,0,65535]
      ,shorts heads [65535,65535,65535],bytes tails (replicate 6 0x55)] Nothing
    ,scenario "raster-run-append-keeps-adjacent-tail" 0x11abdfc [table,0,5,6]
      (runTable 1 4 0 0 ++ [shorts nodes [2,4,65535]])
      [shorts nodes [2,4,1,5,6,65535],shorts (table+30) [2],shorts (tails+2) [1]] (Just 1)
    ,scenario "raster-run-contained-keeps-existing-adjacency" 0x11abdfc [table,0,1,1]
      (runTable 2 4 0 1 ++ [shorts nodes [0,2,1,3,4,65535]])
      [shorts nodes [0,2,1,3,4,65535],shorts (tails+2) [1]] (Just 1)
    ,scenario "raster-run-growing-overlap-coalesces" 0x11abdfc [table,0,3,9]
      (runTable 2 4 0 1 ++ [shorts nodes [2,4,1,8,10,65535]])
      [shorts nodes [2,10,65535],shorts (table+32) [1],shorts (tails+2) [0]] (Just 1)
    ,scenario "raster-run-inserts-before-head" 0x11abdfc [table,0,2,4]
      (runTable 1 4 0 0 ++ [shorts nodes [8,10,65535]])
      [shorts nodes [8,10,65535,2,4,0],shorts (heads+2) [1],shorts (tails+2) [0]] (Just 1)
    ,scenario "raster-run-pool-exhausted" 0x11abdfc [table,0,8,10]
      (runTable 1 1 0 0 ++ [shorts nodes [2,4,65535]])
      [shorts nodes [2,4,65535],shorts (table+30) [1]] (Just 0)
    ,scenario "raster-run-subtract-splits-with-original-tail" 0x11ac190 [table,0,5,7]
      (runTable 1 4 0 0 ++ [shorts nodes [2,10,65535]])
      [shorts nodes [2,4,1,8,10,65535],shorts (table+30) [2],shorts (tails+2) [0]] (Just 1)
    ,scenario "raster-run-split-exhaustion" 0x11ac190 [table,0,5,7]
      (runTable 1 1 0 0 ++ [shorts nodes [2,10,65535]])
      [shorts nodes [2,10,65535]] (Just 0)
    ,scenario "raster-run-subtract-removes-all" 0x11ac190 [table,0,0,20]
      (runTable 2 4 0 1 ++ [shorts nodes [2,4,1,8,10,65535]])
      [shorts (heads+2) [65535],shorts (table+32) [1],shorts (nodes+10) [0]] (Just 1)
    ,scenario "raster-run-bounds-single-populated-row" 0x11ac364 [table,output,output+4]
      (runTable 1 4 0 0) [wordsAt output [0,0]] Nothing
    ,scenario "raster-run-bounds-empty-multirow" 0x11ac364 [table,output,output+4]
      (runTable 0 4 65535 65535) [wordsAt output [1,0xffffffff]] Nothing
    ,scenario "raster-run-first-observes-original-layout" 0x11ac410 [table,0]
      (runTable 2 4 1 1 ++ [shorts (nodes+6) [8,10,65535]]) [] (Just (nodes+6))
    ,scenario "raster-run-first-outside-rows" 0x11ac410 [table,2]
      (runTable 2 4 1 1) [] (Just 0)
    ,scenario "raster-run-next-observes-link" 0x11ac4b0 [table,nodes]
      (runTable 2 4 0 1 ++ [shorts nodes [2,4,1,8,10,65535]]) [] (Just (nodes+6))
    ,scenario "raster-run-next-end" 0x11ac4b0 [table,nodes]
      (runTable 1 4 0 0 ++ [shorts nodes [2,4,65535]]) [] (Just 0)
    ,scenario "raster-run-storage-used" 0x11ac4e0 [table] (runTable 2 4 0 0) [] (Just 28)
    ,scenario "raster-run-storage-counter-arithmetic" 0x11ac4e0 [table] (runTable 5 4 0 0) [] (Just 46)]
  spanBase = [zero vm 0x100,wordsAt (vm+0x68) [output,4],wordsAt (vm+0x74) [32767,32767,0,0]]
  spanCases =
    [scenario "raster-span-crosses-byte-boundaries" 0x11a344c [vm,1,4,13]
      (spanBase ++ [wordsAt (vm+0x4c) [3,2],zero output 32,bytes (output+12) [0x80,0,0x40]])
      [bytes (output+12) [0x81,0xff,0xc0],wordsAt (vm+0x74) [7,3,16,3]] Nothing
    ,scenario "raster-span-rotates-into-column" 0x11a344c [vm,2,1,3]
      (spanBase ++ [wordsAt (vm+0x4c) [1,2],word (vm+0x58) 8,bytes (vm+0x71) [1],word (vm+0x6c) 2,Patch (Address output) (B.replicate 32 0x80)])
      [bytes (output+6) [0x82,0x80,0x82,0x80,0x82],wordsAt (vm+0x74) [6,3,6,5]] Nothing]
  edgeBase side =
    [zero vm 0x8300,word (vm+0x58) 8,wordsAt (vm+0x7ea0) [256,0,255,8],word (vm+0x8054) side
    ,wordsAt (vm+0x805c) [edge,edge],shorts edge (replicate 9 (if side == 0 then -32768 else 32767))]
  edgeCases =
    [scenario "raster-line-excludes-last-grid-endpoint" 0x11a3c00 [vm,256,0,1024,768]
      (edgeBase 0) [shorts edge [1,2,3,-32768],word (vm+0x806c) 2] Nothing
    ,scenario "raster-horizontal-line-takes-left-extreme" 0x11a3c00 [vm,1280,512,256,512]
      (edgeBase 1) [shorts (edge+4) [1],word (vm+0x8068) 2] Nothing
    ,scenario "raster-min-edge-preserves-smaller-value" 0x11a36a8 [vm,8,1]
      [word (vm+0x805c) edge,shorts edge [10,3,12]] [shorts edge [10,3,12]] Nothing
    ,scenario "raster-max-edge-updates-one-row" 0x11a36d4 [vm,8,1]
      [word (vm+0x8060) edge,shorts edge [10,3,12]] [shorts edge [10,8,12]] Nothing]
  curveBase = edgeBase 0 ++ [word (vm+0xb4) 2,wordsAt (vm+0x8050) [1,0,1],word (vm+0x8074) 2]
  curveCases =
    [scenario "raster-quadratic-collinear-diagonal" 0x11a40b4 [vm]
      (curveBase ++ [wordsAt (vm+0x7edc) [0,0,1024,1024,2048,2048]])
      [shorts edge [0,1,2,3,4,5,6,7,8],word (vm+0x806c) 8] Nothing
    ,scenario "raster-quadratic-subdivision-interior-extreme" 0x11a40b4 [vm]
      (curveBase ++ [wordsAt (vm+0x7edc) [0,0,1024,2048,2048,0]])
      [shorts (edge+8) [4],word (vm+0x806c) 8] Nothing
    ,scenario "raster-curve-coordinate-output-scales-and-translates" 0x11a40b4 [vm]
      [zero vm 0x8300,word (vm+0xb4) 1,wordsAt (vm+0x54) [8,4],wordsAt (vm+0x9c) [3,5],word (vm+0x8060) output
      ,wordsAt (vm+0x7edc) [0,0,128,128,256,256]]
      [shorts output [3,5,7,7,11,9],word (vm+0x8060) (output+12)] Nothing]
  rectangle = [0x90,0x01,0,0,0xff,0,0x94,0x23,0,0xff,0xff,0xff,0x71,0]
  programBase program =
    [zero vmContext 0x9000,word (vmContext+0x48) dataAddress,word (vmContext+0xc0) vmContext
    ,wordsAt (vmContext+0x10c) [8,8],wordsAt (vmContext+0x120) [output,1]
    ,wordsAt (vmContext+0x12c) [32767,32767,0,0],wordsAt (vmContext+0xa0) [fromIntegral (length program),0]
    ,bytes dataAddress program,zero output 8]
  programCases =
    [scenario "raster-program-inline-real-rectangle-vm-and-flush" 0x11a05b0 [vmContext,0,0]
      (programBase ([0xfa,0,14,0] ++ rectangle))
      [bytes output (replicate 8 0xff),wordsAt (vmContext+0x12c) [0,0,7,7],word (vmContext+0xa0) 0,word (vmContext+0xac) 0] (Just 0)
    ,scenario "raster-program-ordinary-glyph-directory-and-vm" 0x11a05b0 [vmContext,0,0]
      (programBase [0] ++ [word (vmContext+0x4c) 1,word (vmContext+0x58) 0x10,word (vmContext+0x5c) 0x40
      ,bytes (dataAddress+0x10) [0,0,16,0],bytes (dataAddress+0x40) ([0,0] ++ rectangle)])
      [bytes output (replicate 8 0xff),word (vmContext+0xa0) 0] (Just 0)
    ,scenario "raster-program-recursive-component-real-vm" 0x11a05b0 [vmContext,0,0]
      (programBase [0xfc,0,0,0] ++ [word (vmContext+0x50) 1,word (vmContext+0x60) 0x10,word (vmContext+0x64) 0x40
      ,bytes (dataAddress+0x10) [0,0,0,22,0,0],bytes (dataAddress+0x40) ([0,0,255,255,0xfa,0,14,0] ++ rectangle)])
      [bytes output (replicate 8 0xff),word (vmContext+0xa0) 0,word (vmContext+0xb0) 0,word (vmContext+0xac) 0] (Just 0)
    ,scenario "raster-program-owned-vm-invalid-opcode-error" 0x11a05b0 [vmContext,0,0]
      (programBase [0xfa,0,2,0,1,0])
      [bytes output (replicate 8 0),word (vmContext+0xa0) 0,word (vmContext+0xac) 0] (Just 0xfffffff9)
    ,scenario "raster-program-empty-primary-clears-secondary-selector" 0x11a05b0 [vmContext,0,0]
      [zero vmContext 0x9000,word (vmContext+0xac) 1]
      [word (vmContext+0xac) 0] (Just 0)]
  config = 0x009d2000
  request = 0x009d2100
  bitmapBase format width height orientation program =
    [zero vmContext 0x9000,zero config 0x48,wordsAt config [orientation,width,height,0,0,0,0,0,0,format]
    ,word (config+0x3c) dataAddress,wordsAt (config+0x40) [vmContext,0x9000]
    ,wordsAt vmContext [orientation,width,height],word (vmContext+0x24) format,word (vmContext+0x44) 0x9000
    ,bytes (config+0x28) (16:[0..15]),bytes (vmContext+0x140) (16:[0..15])
    ,word (vmContext+0x48) dataAddress,word (vmContext+0x54) 1,word (vmContext+0x6c) 0x20
    ,wordsAt (vmContext+0x10c) [width * factor,height * factor]
    ,wordsAt request [0,0,0,91,92,if format == 0 then (width+7) `div` 8 else width,output,93,0]
    ,bytes dataAddress [0,0,0,fromIntegral (length program),0,0],bytes (dataAddress+0x20) program
    ,word (vmContext+0x83b8) 40,shorts (vmContext+0x83cc) [4,2]
    ,zero output (fromIntegral (width * height))]
   where factor = case format of { 0 -> 1; 1 -> 4; _ -> 16 }
  bitmapCases =
    [scenario "raster-bitmap-owned-glyph-loader-vm-mask" 0x11a0fa4 [config,request]
      (bitmapBase 0 8 8 0 ([0xfa,0,14,0] ++ rectangle))
      [bytes output (replicate 8 0xff),wordsAt (request+4) [0,0,8,8],word (request+28) 0x8408] (Just 0)
    ,scenario "raster-bitmap-owned-gray4-coverage" 0x11a0fa4 [config,request]
      (bitmapBase 1 8 8 0 ([0xfa,0,14,0] ++ rectangle))
      [bytes output (replicate 64 15),wordsAt (request+4) [0,0,8,8],word (request+28) 0x852c] (Just 0)
    ,scenario "raster-bitmap-gray8-aligned-interior-uses-fifteen" 0x11a0fa4 [config,request]
      (bitmapBase 2 12 1 0 ([0xfa,0,14,0] ++ rectangle))
      [bytes output [255,255,255,255,240,240,240,240,255,255,255,255],wordsAt (request+4) [0,0,12,1],word (request+28) 0x8490] (Just 0)
    ,scenario "raster-bitmap-rotated-gray4-original-inclusive-box" 0x11a0fa4 [config,request]
      (bitmapBase 1 8 8 1 ([0xfa,0,14,0] ++ rectangle))
      [bytes output (replicate 64 15),wordsAt (request+4) [0,0,9,8],word (request+28) 0x852c] (Just 0)
    ,scenario "raster-bitmap-underline-is-observable-without-glyph-edges" 0x11a0fa4 [config,request]
      (bitmapBase 0 8 8 0 [0xfd,0,0] ++ [word (config+28) 1,word (vmContext+28) 1])
      [bytes output [0,0,0,0,0,0,0,255],wordsAt (request+4) [0,7,8,1],word (request+28) 0x8408] (Just 0)
    ,scenario "raster-bitmap-owned-vm-error-retains-empty-output" 0x11a0fa4 [config,request]
      (bitmapBase 0 8 8 0 [0xfa,0,2,0,1,0])
      [bytes output (replicate 8 0),wordsAt (request+4) [0,0,0,0],word (request+28) 0x8408] (Just 0xfffffff9)
    ,scenario "raster-bitmap-missing-glyph-result" 0x11a0fa4 [config,request]
      (bitmapBase 0 8 8 0 [] ++ [word request 1])
      [wordsAt request [1,0,0,91,92,1,output,93,0]] (Just 1)]
  genuineCases =
    let fixture = fontFixture
        layout = addressWord (fontLayoutAddress fixture)
        workspaceAddress = addressWord (fontWorkspaceAddress fixture)
        bitmap = addressWord (fontBitmapAddress fixture)
        requestAddress = addressWord (fontRequestAddress fixture)
        initial = fontRasterInitial fixture ++ [wordsAt requestAddress [stockFontCode IconE14d,0,0,0,0,32,bitmap,0,0]]
        final = [bytes bitmap (replicate 32 0)
          ,wordsAt (workspaceAddress+0xa0) [0,0x3ee56],word (workspaceAddress+0xac) 0]
     in [(scenario "raster-bitmap-genuine-e14d-owned-loader-vm-and-coverage" 0x11a0fa4 [layout+4,requestAddress] initial final (Just 0))
          {stageCodePointers = fontRasterCodePointers fixture}
        ,(scenario "raster-bitmap-valid-fdl-half-em-rectangle-fixture" 0x11a0fa4 [layout+4,requestAddress]
          (fontRectangleRasterInitial fixture ++ [wordsAt requestAddress [rasterFixtureGlyphCode RectangleA,0,0,0,0,32,bitmap,0,0]])
          [Patch (Address bitmap) (B.replicate 1024 15),wordsAt (requestAddress+4) [0,0,33,32],wordsAt (workspaceAddress+0xa0) [0,0x218]] (Just 0))
          {stageCodePointers = fontRasterCodePointers fixture}]
