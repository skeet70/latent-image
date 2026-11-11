module FontBuiltinScenarios (fontBuiltinPublications, fontBuiltinScenarios) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), word, wordsAt)

fontBuiltinPublications :: [StagePublication]
fontBuiltinPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_" ++ name)) | (address,name) <- entries]

entries :: [(Word32,String)]
entries =
  [(0x11932c4,"font_builtin_init"),(0x1193830,"font_builtin_glyph_width")
  ,(0x11939f8,"font_builtin_right_overhang"),(0x1190be8,"font_builtin_height")
  ,(0x1190c00,"font_builtin_width"),(0x1190c18,"font_builtin_secondary_extent")
  ,(0x1193364,"font_draw_text_bitmap"),(0x1190d18,"font_builtin_thai_metric")
  ,(0x11915b8,"font_builtin_parse_line"),(0x1192c70,"font_builtin_properties")
  ,(0x1192fb4,"font_builtin_scan_header"),(0x1192e68,"font_builtin_scan_glyph")
  ,(0x11930e8,"font_builtin_index_glyphs"),(0x11913cc,"font_builtin_decode_bitmap")
  ,(0x119126c,"font_builtin_parse_text_lines"),(0x1190c44,"font_builtin_top_overhang")
  ,(0x1190c5c,"font_builtin_stroke_padding"),(0x1193f98,"font_icon_init")
  ,(0x1194190,"font_icon_glyph_width"),(0x119426c,"font_draw_icon_bitmap")
  ,(0x1193ff0,"font_icon_height"),(0x119400c,"font_icon_width")
  ,(0x1194028,"font_icon_secondary_extent"),(0x1194040,"font_icon_right_overhang")
  ,(0x1194058,"font_icon_top_overhang"),(0x1194070,"font_icon_stroke_padding")
  ,(0x119083c,"font_builtin_read_text_line"),(0x1190ba0,"font_builtin_storage_size")
  ,(0x1190bb8,"font_builtin_destroy"),(0x1190bd0,"font_builtin_dimensions")
  ,(0x1193f80,"font_icon_storage_size"),(0x1193fc0,"font_icon_destroy")
  ,(0x1193fd8,"font_icon_dimensions"),(0x11940a0,"font_icon_weights")]

state, engine, resource, storage, spanAddress, metrics, pixels, output, cache, work, textConfig, extra :: Word32
state = 0x9a0000
engine = 0x9a2000
resource = 0x9b0000
storage = 0x9b4000
spanAddress = 0x9a5000
metrics = 0x9a8000
pixels = 0x9c0000
output = 0x9a8100
cache = 0x9a9000
work = 0x9aa000
textConfig = 0x9a9100
extra = 0x9a8200

bdfHeader, bdfGlyph, bdf :: B.ByteString
bdfHeader = C.pack "STARTFONT 2.2\r\nFONT synthetic\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 5 7 -1 -2\r\nSTARTPROPERTIES 2\r\nTHAI_TOP 3\r\nTHAI_BASE -2\r\nENDPROPERTIES\r\nCHARS 1\r\n"
bdfGlyph = C.pack "STARTCHAR A\r\nENCODING 65\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 1 -1\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\n"
bdf = B.concat [bdfHeader,bdfGlyph,C.pack "ENDFONT\r\n"]

glyphOffset, bitmapOffset :: Word32
glyphOffset = fromIntegral (B.length bdfHeader)
bitmapOffset = glyphOffset + fromIntegral (B.length (C.pack "STARTCHAR A\r\nENCODING 65\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 1 -1\r\nBITMAP\r\n"))

patch :: Word32 -> [Word32] -> Patch
patch address = Patch (Address address) . B.pack . map fromIntegral

baseBdf :: [Patch]
baseBdf =
  [Patch (Address state) (B.replicate 0x80 0),Patch (Address storage) (B.replicate 0x1000 0)
  ,Patch (Address resource) bdf,word state engine,word (state+0x30) (storage+0x300)
  ,word (state+0x34) 0x800,word (state+0x38) resource,word (state+0x3c) storage,word (state+0x40) (storage+0x200)
  ,patch (state+12) [5,0,7,0,255,255,254,255],patch (state+26) [4,0]
  ,word (storage+0x300) 0x100,word (storage+0x300+0x504) glyphOffset
  ,word (engine+0x1068) 2,word (engine+0x28) 0,word (engine+0x20) 0
  ,word (engine+0x53c) spanAddress,word (spanAddress+0xd14) 0,word (spanAddress+0xd18) 0
  ,word (engine+0x18) cache,word (cache+0x70) work,word (work+12) 64,word (work+16) (work+0x100)
  ,word (engine+0x10) textConfig,word (textConfig+12) 0
  ,Patch (Address pixels) (B.replicate 64 0),Patch (Address metrics) (B.replicate 64 0)
  ,Patch (Address output) (B.replicate 32 0)]

baseIcon :: [Patch]
baseIcon =
  [wordsAt state [engine,0,resource],Patch (Address resource) (B.replicate 0x700 0)
  ,Patch (Address resource) (C.pack "DCW ICON"),patch (resource+8) [9,11,1,0]
  ,patch (resource+32) [1,0,0,0],word (resource+36+65*4) 0x600
  ,patch (resource+0x620) [5,3,168,80,248]
  ,word (engine+0x1068) 2,word (engine+0x105c) 1,word (engine+0x1074) 0
  ,word (engine+0x53c) spanAddress,word (spanAddress+0xd14) 0,word (spanAddress+0xd18) 0
  ,Patch (Address pixels) (B.replicate 64 0),Patch (Address extra) (B.replicate 28 0)]

fontBuiltinScenarios :: [StageScenario]
fontBuiltinScenarios = concatMap cases fontBuiltinPublications
 where
  scenario pub label args initial final result = StageScenario (StageCaseName label) pub args initial final (if stageStockEntry pub `elem` [Address 0x1190d18,Address 0x119126c] then Nothing else Just result) [] []
  bdfCase pub label args final result = scenario pub label args baseBdf final result
  iconCase pub label args final result = scenario pub label args baseIcon final result
  fallbackCase pub label args final =
    let iconState = 0x9ab000
        iconData = 0x9b8000
        initial = baseBdf ++
          [Patch (Address (resource+glyphOffset)) (C.pack "ENDFONT\r\n"),word (engine+0x20) 1,word (engine+0x28) 1,word (engine+100) iconState
          ,wordsAt iconState [engine,0,iconData],Patch (Address iconData) (B.replicate 0x700 0)
          ,Patch (Address iconData) (C.pack "DCW ICON"),patch (iconData+8) [9,11,1,0]
          ,patch (iconData+32) [1,0,0,0],word (iconData+36+65*4) 0x600,patch (iconData+0x620) [5,3,168,80,248]]
        widthMethod = StagePublication (Address 0x1194190) (StageSymbol "xm1_pipeline_image_font_icon_glyph_width")
        drawMethod = StagePublication (Address 0x119426c) (StageSymbol "xm1_pipeline_image_font_draw_icon_bitmap")
     in (scenario pub label args initial final 7) { stageCodePointers = [(Address (engine+0x3c),widthMethod),(Address (engine+0x34),drawMethod)] }
  drawArgs = [state,65,16,8,1,0,2,pixels,0,0x100]
  metricBytes = [65,0,0,0,0,0,0,0,0,0,0,0,0,0,65,0,244,1,0,0,4,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,5,0,3,0,1,0,255,255]
  metricsInitial = [patch metrics metricBytes,word (metrics+48) bitmapOffset]
  padding pub initial = [scenario pub "four-output-padding" [state,output,output+4,output+8,output+12] (initial++[wordsAt output [7,8,9,10]]) [wordsAt output [0,0,0,0]] 0]
  cases pub = case stageStockEntry pub of
    Address 0x11932c4 ->
      [scenario pub "literal-bdf-initialization" [state,resource,0xb00,storage,engine]
        (baseBdf++[Patch (Address state) (B.replicate 0x80 0),Patch (Address storage) (B.replicate 0x1000 0)])
        [word state engine,patch (state+4) [34,0,8,0,75,0,75,0,5,0,7,0,255,255,254,255],word (state+44) 1,word (storage+0x300) 0x100,word (storage+0x804) glyphOffset] 0
      ,bdfCase pub "short-workspace" [state,resource,0x7ff,storage,engine] [word state engine] 0xfffffffa
      ,scenario pub "declared-count-mismatch" [state,resource,0xb00,storage,engine]
        (baseBdf++[Patch (Address (resource+glyphOffset-3)) (C.pack "2"),Patch (Address storage) (B.replicate 0x1000 0)]) [] 0xfffffffd]
    Address 0x1193830 -> [bdfCase pub "nonzero-bdf-advance-and-bias" [state,65] [] 6,bdfCase pub "missing-bdf-glyph" [state,66] [] 0,fallbackCase pub "bdf-width-falls-back-to-real-icon" [state,65] []]
    Address 0x11939f8 -> [bdfCase pub "positive-right-overhang" [state,65] [] 2]
    Address 0x1190be8 -> [bdfCase pub "font-height" [state] [] 7]
    Address 0x1190c00 -> [bdfCase pub "font-width" [state] [] 5]
    Address 0x1190c18 -> [bdfCase pub "nonpositive-secondary-clamped" [state] [] 0,scenario pub "positive-secondary" [state] (baseBdf++[patch (state+16) [2,0]]) [] 3]
    Address 0x1193364 ->
      [bdfCase pub "bdf-nonzero-packed-glyph" drawArgs [patch (pixels+6) [42,0,20,0,62,0]] 4
      ,fallbackCase pub "bdf-draw-falls-back-to-real-icon" drawArgs [patch pixels [84,0,40,0,124,0]]
      ,scenario pub "bdf-preserves-existing-ink" drawArgs (baseBdf++[patch (pixels+6) [128]]) [patch (pixels+6) [170,0,20,0,62,0]] 4
      ,scenario pub "bdf-direct-decode" [state,65,16,8,0xffffffff,0,2,pixels,0,0x100] (baseBdf++[word (textConfig+12) 1]) [patch (pixels+6) [168,0,80,0,248,0]] 4
      ,scenario pub "bdf-scratch-shortage" drawArgs (baseBdf++[word (work+12) 4]) [Patch (Address pixels) (B.replicate 16 0)] 0
      ,scenario pub "bdf-rectangle-clear-after-blend" [state,65,16,8,1,0,2,pixels,extra,0x100]
        (baseBdf++[wordsAt extra [2,3,0,0,1,1,1]]) [patch (pixels+6) [10,0,20,0,62,0]] 4]
    Address 0x1190d18 ->
      [scenario pub "signed-thai-base-property" [output,resource,12,resource+32,9,10]
        [Patch (Address resource) (C.pack "THAI_BASE -2"),Patch (Address (resource+32)) (C.pack "THAI_BASE\0\0\0"),wordsAt output [0,0,0]] [patch (output+4) [254,255]] 0
      ,scenario pub "thai-base-rejects-nonzero-native-name-padding" [output,resource,12,resource+32,9,10]
        [Patch (Address resource) (C.pack "THAI_BASE -2"),Patch (Address (resource+32)) (C.pack "THAI_BASE\0RT"),word (output+4) 0x12345678] [word (output+4) 0x12345678] 0]
    Address 0x11915b8 ->
      [scenario pub "bdf-glyph-box-line" [state,resource,12,0,metrics,1,output]
        (baseBdf++[Patch (Address resource) (C.pack "BBX 5 3 1 -1")]) [patch (metrics+36) [5,0,3,0,1,0,255,255],word output 18] 0
      ,bdfCase pub "invalid-parse-mode" [state,resource,1,metrics,metrics,2,output] [word output 0] 0xffffffff]
    Address 0x1192c70 -> [bdfCase pub "native-thai-properties-preserve-word-padding-guard" [state,output] [patch output [3,0,0,0,0,0]] 0]
    Address 0x1192fb4 -> [bdfCase pub "header-consumed-through-chars" [state,resource,output] [word output glyphOffset,word (state+44) 1] 0]
    Address 0x1192e68 -> [bdfCase pub "glyph-metrics-and-next" [state,resource,glyphOffset,metrics,output]
      [patch metrics metricBytes,word (metrics+48) bitmapOffset,word output (glyphOffset+fromIntegral (B.length bdfGlyph))] 0]
    Address 0x11930e8 -> [scenario pub "index-literal-glyph" [state,resource,glyphOffset,storage+0x300,0x800]
      (baseBdf++[Patch (Address (storage+0x300)) (B.replicate 0x800 0)]) [word (storage+0x300) 0x100,word (storage+0x804) glyphOffset] 1]
    Address 0x11913cc ->
      [scenario pub "decode-literal-nonzero-rows" [state,metrics,pixels,2,1] (baseBdf++metricsInitial) [patch (pixels+2) [168,0,80,0,248]] 0
      ,scenario pub "decode-endchar-error-after-pixel-effects" [state,metrics,pixels,2,1]
        (baseBdf++metricsInitial++[Patch (Address (resource+bitmapOffset+12)) (C.pack "BADCHAR")]) [patch (pixels+2) [168,0,80,0,248]] 0xfffffffe]
    Address 0x119126c -> [scenario pub "consume-two-property-lines" [state,output,resource,0,2,output+16,output+20]
      (baseBdf++[Patch (Address resource) (C.pack "THAI_TOP 3\r\nTHAI_BASE -2\r\n")]) [patch output [3,0,0,0,254,255],word (output+16) 26,word (output+20) 26] 0]
    Address 0x1190c44 -> [bdfCase pub "bdf-top-overhang" [state] [] 0]
    Address 0x1190c5c -> padding pub baseBdf ++ [bdfCase pub "nullable-bdf-padding" [state,0,0,0,0] [] 0]
    Address 0x1193f98 -> [iconCase pub "icon-resource-installed" [state,resource,0,0,engine] [wordsAt state [engine,0,resource]] 0]
    Address 0x1194190 -> [iconCase pub "dcw-icon-width-and-bias" [state,65] [] 7,iconCase pub "missing-dcw-icon" [state,66] [] 0]
    Address 0x119426c ->
      [iconCase pub "dcw-icon-packed-unaligned" [state,65,16,8,1,1,2,pixels,0,0x100] [patch (pixels+2) [84,0,40,0,124,0]] 7
      ,scenario pub "dcw-icon-preserves-existing-ink" [state,65,16,8,1,1,2,pixels,0,0x100] (baseIcon++[patch (pixels+2) [1]]) [patch (pixels+2) [85,0,40,0,124,0]] 7
      ,iconCase pub "dcw-icon-four-bit-coverage" [state,65,16,8,0,1,8,pixels,0,0x200] [patch (pixels+8) [15,0,15,0,15,0,0,0,0,15,0,15,0,0,0,0,15,15,15,15,15]] 7
      ,scenario pub "dcw-icon-clipped-height" [state,65,16,2,0,1,2,pixels,extra,0x100] (baseIcon++[word (extra+12) 1,word (extra+24) 1]) [patch (pixels+2) [168,0,0,0]] 7]
    Address 0x1193ff0 -> [iconCase pub "dcw-icon-height" [state] [] 11]
    Address 0x119400c -> [iconCase pub "dcw-icon-width" [state] [] 9]
    Address 0x1194028 -> [iconCase pub "dcw-icon-secondary-extent" [state] [] 0]
    Address 0x1194040 -> [iconCase pub "dcw-icon-right-overhang" [state,65] [] 0]
    Address 0x1194058 -> [iconCase pub "dcw-icon-top-overhang" [state] [] 0]
    Address 0x1194070 -> padding pub baseIcon
    Address 0x119083c ->
      [bdfCase pub "copy-crlf-line" [resource,0,output,output+4,metrics,1] [Patch (Address metrics) (C.pack "STARTFONT 2.2\0"),word output 15,word (output+4) 13] 0
      ,bdfCase pub "scan-without-copy" [resource,0,output,output+4,0,0] [word output 15,word (output+4) 0] 0
      ,bdfCase pub "null-copy-destination" [resource,0,output,output+4,0,1] [word output 0,word (output+4) 0] 0xffffffff]
    Address 0x1190ba0 -> [bdfCase pub "bdf-workspace-size" [] [] 0xb00]
    Address 0x1190bb8 -> [bdfCase pub "bdf-destroy" [] [] 0]
    Address 0x1190bd0 -> [bdfCase pub "bdf-dimensions-status" [] [] 0]
    Address 0x1193f80 -> [iconCase pub "icon-no-workspace" [7,1,2,3] [] 0]
    Address 0x1193fc0 -> [iconCase pub "icon-destroy" [] [] 0]
    Address 0x1193fd8 -> [iconCase pub "icon-dimensions-status" [] [] 0]
    Address 0x11940a0 -> [iconCase pub "icon-weights-unavailable" [] [] 0xffffffff]
    _ -> error "unmapped builtin font publication"
