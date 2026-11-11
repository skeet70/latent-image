module FontFdlScenarios (fontFdlPublications, fontFdlScenarios, FontFixture (..), fontFixture, StockFontGlyph (..), stockFontCode, fontIconPatches, fontRasterInitial, fontRasterCodePointers, fontOutlineInitial, fontOutlineEngineInitial, fontOutlineCodePointers, fontRectangleRasterInitial) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), base, word, wordsAt)

data GlyphDirectoryBackend = OutlineDirectory | RasterDirectory deriving (Eq, Show)
data GlyphDirectoryEncoding = CompactDirectory | DirectPageDirectory | SparsePageDirectory deriving (Eq, Show)
data FontConstructorBackend = OutlineMethods | BuiltinMethods | RasterMethods | IconMethods deriving (Eq, Show)

data StockFontGlyph = IconE14d deriving (Eq, Show)
stockFontCode :: StockFontGlyph -> Word32
stockFontCode IconE14d = 0xe14d

data FontFixture = FontFixture
  { fontEngineAddress :: Address, fontObjectAddress :: Address, fontLayoutAddress :: Address
  , fontSpanAddress :: Address, fontRequestAddress :: Address, fontTextAddress :: Address
  , fontWorkspaceAddress :: Address, fontBitmapAddress :: Address, fontOutputAddress :: Address
  } deriving (Eq, Show)

fontFixture :: FontFixture
fontFixture = FontFixture (Address 0xa00000) (Address 0xa02000) (Address 0xa04000) (Address 0xa05000) (Address 0xa07000) (Address 0xa07100) (Address 0xa10000) (Address 0xa40000) (Address 0xa48000)

at :: Address -> Word32 -> Word32
at address offset = addressWord address + offset

half :: Word32 -> Word32 -> Patch
half address value = Patch (Address address) (B.pack [fromIntegral value, fromIntegral (value `div` 256)])

fontIconPatches :: [Patch]
fontIconPatches =
  [ Patch (Address 0x3bc000) (B.pack
      [0x46,0x44,0x4c,0x20,0x46,0x6f,0x6e,0x74,0,0,0,0,0,0,0,0
      ,0,0,0,0,0,0,0,0,0,0,0,0,0x26,0xf7,0x4c,0
      ,0,0,0xed,0xdc,3,0,0x4e,0,0,0,0xed,0xdc,3,0,0x51,0
      ,0,0,0xed,0xdc,3,0,4,0,0,1,0x75,0xc1,7,0,0,0
      ,0x69,0x6e,0x66,0x6f,0x0e,0xc1,7,0,0x67,0,0,0,0,0,0,0,0])
  , Patch (Address 0x3f4585) (B.pack [0xfd,0x10,0,1,0x69,0x11,0,1])
  , Patch (Address 0x3fadea) (B.pack
      [0,0,0xf4,1,0x66,0,0xfa,0,0x62,0,0xb5,0,0x2f,0x68,0xb5,0x10
      ,0xdc,0x7d,0x92,2,0x67,0x6b,0x92,0x23,0xdc,0x6b,3,0x31,0x96,4,0x2f,0x7d
      ,7,0x41,0x70,0,0xb5,0,0x2f,0x18,0xb5,0x10,0xdc,0x47,0x92,2,0x67,0x15
      ,0x92,0x23,0xdc,0x39,3,0x31,0x96,4,0xb2,0x40,7,0x41,0x70,0,0xb5,0
      ,0x2f,0x68,0xb5,0x10,0xdc,0x47,0x92,2,0xb2,0x40,3,0x21,0x96,3,0x67,0x6b
      ,7,0x31,0x70,0,0xb5,0,0x2f,3,0xb5,0x10,0xdc,0x15,0x92,2,0xdc,3
      ,3,0x21,0x96,3,0x2f,0x18,0x96,0x34,0x67,0x15,7,0x41])
  , Patch (Address 0x438175) (B.pack [1,0,0x0a,0,0x5a,0x0e,0,0,0,0xf0,0x25,0xf7,0x0b,1,0xe8,3])
  ]

fontRasterInitial :: FontFixture -> [Patch]
fontRasterInitial fixture = base ++ fontIconPatches ++
  [ Patch (fontEngineAddress fixture) (B.replicate 0x1400 0)
  , Patch (fontLayoutAddress fixture) (B.replicate 0x50 0)
  , Patch (Address (layout+0x400)) (B.replicate 0x200 0)
  , Patch (fontWorkspaceAddress fixture) (B.replicate 0x83ec 0)
  , Patch (fontObjectAddress fixture) (B.replicate 0x11b0 0)
  , Patch (fontSpanAddress fixture) (B.replicate 0xd40 0)
  , Patch (fontRequestAddress fixture) (B.replicate 0x100 0)
  , word (at (fontObjectAddress fixture) 0) engine, word (at (fontObjectAddress fixture) 12) 3
  , word (at (fontObjectAddress fixture) 80) (addressWord (fontRequestAddress fixture)), word (at (fontObjectAddress fixture) 88) (addressWord (fontSpanAddress fixture))
  , word (engine+20) (addressWord (fontObjectAddress fixture)), word (engine+0x53c) (addressWord (fontSpanAddress fixture))
  , wordsAt (at (fontSpanAddress fixture) 0xd00) [32,32,32,1024,bitmap,0,0]
  , word (at (fontSpanAddress fixture) 8) 0x200
  , word (at (fontObjectAddress fixture) 0x84) 20, word (at (fontObjectAddress fixture) 0x8c) 0xd40
  , word (at (fontObjectAddress fixture) 0x1168) (layout+0x600), word (at (fontObjectAddress fixture) 0x116c) (layout+0x600)
  , word (at (fontObjectAddress fixture) 0x1170) (layout+0x600), word (at (fontObjectAddress fixture) 0x1174) 40
  , word (at (fontObjectAddress fixture) 0x1178) bitmap, word (at (fontObjectAddress fixture) 0x117c) 0x8000
  , word (at (fontObjectAddress fixture) 0x11ac) (layout+0x400)
  , Patch (Address (layout+0x600)) (B.replicate 40 0)
  , wordsAt engine [0x2117c,0,0x107c]
  , word (engine+0x18) (layout+0x400), word (engine+0x20) 0
  , word (engine+0x68) layout, word (engine+0x6c) 0x3bc000
  , word (engine+0xff4) 4, word (engine+0x1030) 0x20000, word (engine+0x1034) workspace
  , word (engine+0x103c) 32, word (engine+0x1040) 32, word (engine+0x105c) 1
  , word (engine+0x106c) 0x200, word (engine+0x1078) 4
  , word (layout+0x408) 0, word (layout+0x40c) 256, word (layout+0x470) (layout+0x480)
  , word (layout+0x48c) 0x2000, word (layout+0x490) (bitmap+0x10000)
  , wordsAt layout [engine,0,32,32,0,0,0,0,0,0,1]
  , Patch (Address (layout+44)) (B.pack (16:[0..15]++[0]))
  , wordsAt (layout+64) [0x3bc000,workspace,0x20000]
  , wordsAt workspace [0,32,32,0,0,0,0,0,0,1]
  , Patch (Address (workspace+40)) (B.pack (16:[0..15]++[0]))
  , wordsAt (workspace+60) [0x3bc000,workspace,0x20000,0x3bc000,0,0,0xf726,0x4c,0x3dced,0x4e,0x3dced,0x51,0x3dced,4,0x100,0x51]
  , half (workspace+0x7c) 0xffff
  , word (workspace+0x10c) 128, word (workspace+0x110) 128, word (workspace+0x13c) 1
  , Patch (Address (workspace+0x140)) (B.pack (16:[0..15]))
  , word (workspace+0x83d4) 0x7c175, half (workspace+0x83d8) 1, word (workspace+0x83dc) 0x7c177
  , half (workspace+0x83e0) 0xf000, half (workspace+0x83e2) 0xf725, half (workspace+0x83e4) 0x10b, half (workspace+0x83e6) 1000, word (workspace+0x83e8) 0x7c185
  , Patch (fontBitmapAddress fixture) (B.replicate 0x8000 0)
  ]
  ++ [word (addressWord cell) (addressWord (stageStockEntry target)) | (cell,target) <- fontRasterCodePointers fixture]
 where
  engine = addressWord (fontEngineAddress fixture)
  layout = addressWord (fontLayoutAddress fixture)
  workspace = addressWord (fontWorkspaceAddress fixture)
  bitmap = addressWord (fontBitmapAddress fixture)

fontRasterCodePointers :: FontFixture -> [(Address,StagePublication)]
fontRasterCodePointers fixture = [(Address (at (fontEngineAddress fixture) offset), publication target name) | (offset,target,name) <-
  [(0xff8,0x11953f4,"initialize_raster_layout"),(0x1000,0x1195bc8,"draw_raster_bitmap"),(0x1004,0x1194fb4,"raster_weights"),(0x1008,0x1195294,"chained_glyph_width")
  ,(0x100c,0x11956ac,"raster_height"),(0x1010,0x1195764,"raster_width"),(0x1014,0x11953a4,"raster_secondary_extent"),(0x1018,0x1194e60,"raster_right_overhang")
  ,(0x101c,0x119581c,"raster_top_overhang"),(0x1020,0x1194eb0,"raster_stroke_padding")]]

fontOutlineInitial :: FontFixture -> [Patch]
fontOutlineInitial fixture = base ++ fontIconPatches ++
  [Patch (fontWorkspaceAddress fixture) (B.replicate 0x814 0)
  ,word (workspace+8) 0x3bc000
  ,wordsAt (workspace+0xc) [0,0,0xf726,0x4c,0x3dced,0x4e,0x3dced,0x51,0x3dced,4,0x100,0x51]
  ,half (workspace+0x3c) 0xffff]
 where workspace = addressWord (fontWorkspaceAddress fixture)

fontOutlineEngineInitial :: FontFixture -> [Patch]
fontOutlineEngineInitial fixture = fontRasterInitial fixture ++ fontOutlineInitial fixture ++
  [ word (engine+0xff4) 1, wordsAt layout [engine,workspace,0x3bc000,0]
  , word (at (fontObjectAddress fixture) 12) 1
  ]
  ++ [word (addressWord cell) (addressWord (stageStockEntry target)) | (cell,target) <- fontOutlineCodePointers fixture]
 where
  engine = addressWord (fontEngineAddress fixture)
  layout = addressWord (fontLayoutAddress fixture)
  workspace = addressWord (fontWorkspaceAddress fixture)

fontOutlineCodePointers :: FontFixture -> [(Address,StagePublication)]
fontOutlineCodePointers fixture = [(Address (at (fontEngineAddress fixture) offset), publication target name) | (offset,target,name) <-
  [(0xff8,0x1196f08,"handle_init"),(0xffc,0x11970f8,"handle_append_bitmap"),(0x1004,0x1196d28,"handle_weights")
  ,(0x1008,0x11972ac,"handle_glyph_width"),(0x100c,0x1196c20,"outline_height"),(0x1010,0x1196c38,"outline_width")
  ,(0x1014,0x1196c50,"outline_secondary_extent"),(0x1018,0x1196c68,"outline_right_overhang"),(0x101c,0x1196c80,"outline_top_overhang")
  ,(0x1020,0x1196c98,"outline_stroke_padding"),(0x1024,0x1197384,"handle_dimensions")]]

fontRectangleRasterInitial :: FontFixture -> [Patch]
fontRectangleRasterInitial fixture = fontRasterInitial fixture ++
  [ half (font+0x1c) 66
  , word (font+0x2e) 0x80, word (font+0x32) 0x200, word (font+0x3a) 0
  , Patch (Address (font+0x80)) (B.replicate (67*4) 0)
  , Patch (Address (font+0x80+65*4)) (B.pack [0,0,0,1,24,0,0,0])
  , Patch (Address (font+0x200)) (B.pack [0,0,0xf4,1,18,0,0xfa,0,14,0,0x90,1,0,0,0xff,0,0x94,0x23,0,0xff,0xff,0xff,0x71,0])
  , word (workspace+0x54) 66, wordsAt (workspace+0x68) [0x80,0x200]
  , word (workspace+0x78) 0x80, wordsAt (workspace+0x83d4) [0,0,0,0,0,0]
  ]
 where
  font = 0x3bc000
  workspace = addressWord (fontWorkspaceAddress fixture)

publication :: Word32 -> String -> StagePublication
publication address name = StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_font_"++name))

constructorMethods :: Word32 -> FontConstructorBackend -> [(Address,StagePublication)]
constructorMethods engine backend = [(Address (engine+offset),publication target name) | (offset,target,name) <- rows]
 where
  rows = case backend of
    OutlineMethods ->
      [(0xff8,0x1196f08,"handle_init"),(0xffc,0x11970f8,"handle_append_bitmap"),(0x1004,0x1196d28,"handle_weights")
      ,(0x1008,0x11972ac,"handle_glyph_width"),(0x100c,0x1196c20,"outline_height"),(0x1010,0x1196c38,"outline_width")
      ,(0x1014,0x1196c50,"outline_secondary_extent"),(0x1018,0x1196c68,"outline_right_overhang"),(0x101c,0x1196c80,"outline_top_overhang")
      ,(0x1020,0x1196c98,"outline_stroke_padding"),(0x1024,0x1197384,"handle_dimensions"),(0x1028,0x1196eec,"handle_destroy"),(0x102c,0x1197228,"handle_bitmap")]
    BuiltinMethods ->
      [(0xff8,0x11932c4,"builtin_init"),(0x1000,0x1193364,"draw_text_bitmap"),(0x1004,0x1192c70,"builtin_properties")
      ,(0x1008,0x1193830,"builtin_glyph_width"),(0x100c,0x1190be8,"builtin_height"),(0x1010,0x1190c00,"builtin_width")
      ,(0x1014,0x1190c18,"builtin_secondary_extent"),(0x1018,0x11939f8,"builtin_right_overhang"),(0x101c,0x1190c44,"builtin_top_overhang")
      ,(0x1020,0x1190c5c,"builtin_stroke_padding"),(0x1024,0x1190bd0,"builtin_dimensions"),(0x1028,0x1190bb8,"builtin_destroy")]
    RasterMethods ->
      [(0xff8,0x11953f4,"initialize_raster_layout"),(0x1000,0x1195bc8,"draw_raster_bitmap"),(0x1004,0x1194fb4,"raster_weights")
      ,(0x1008,0x1195294,"chained_glyph_width"),(0x100c,0x11956ac,"raster_height"),(0x1010,0x1195764,"raster_width")
      ,(0x1014,0x11953a4,"raster_secondary_extent"),(0x1018,0x1194e60,"raster_right_overhang"),(0x101c,0x119581c,"raster_top_overhang")
      ,(0x1020,0x1194eb0,"raster_stroke_padding"),(0x1024,0x1194e48,"raster_dimensions"),(0x1028,0x1194e30,"raster_destroy")]
    IconMethods ->
      [(0xff8,0x1193f98,"icon_init"),(0x1000,0x119426c,"draw_icon_bitmap"),(0x1004,0x11940a0,"icon_weights")
      ,(0x1008,0x1194190,"icon_glyph_width"),(0x100c,0x1193ff0,"icon_height"),(0x1010,0x119400c,"icon_width")
      ,(0x1014,0x1194028,"icon_secondary_extent"),(0x1018,0x1194040,"icon_right_overhang"),(0x101c,0x1194058,"icon_top_overhang")
      ,(0x1020,0x1194070,"icon_stroke_padding"),(0x1024,0x1193fd8,"icon_dimensions"),(0x1028,0x1193fc0,"icon_destroy")]

methodWords :: [(Address,StagePublication)] -> [Patch]
methodWords bindings = [word (addressWord cell) (addressWord (stageStockEntry target)) | (cell,target) <- bindings]

entries :: [(Word32,String)]
entries = [(0x1197e68,"binary_info"),(0x119f0fc,"binary_info"),(0x1197b98,"program_initialize"),(0x1197f98,"program_load_glyph"),(0x119f74c,"raster_load_glyph"),(0x1197f18,"program_read_byte"),(0x1197f50,"program_read_short"),(0x117f51c,"fill_memory"),(0x117f64c,"copy_memory"),(0x118ed64,"engine_metrics"),(0x119fcbc,"raster_configure"),(0x11a0340,"raster_glyph_advance")]
  ++ [(0x118dd48,"clear_context"),(0x1182108,"export_outline"),(0x1182578,"export_bitmap"),(0x11822ec,"prepare_outline"),(0x11826cc,"prepare_bitmap")
  ,(0x1181ed0,"fill_leading_rows"),(0x1181c98,"fill_trailing_rows"),(0x1181458,"get_option"),(0x117f390,"draw"),(0x11829c8,"draw")
  ,(0x117f3f0,"allocate"),(0x117f45c,"resize"),(0x117f4bc,"release"),(0x118e49c,"shape"),(0x118df74,"grow_workspace"),(0x118e12c,"finalize_runs")
  ,(0x11956ac,"raster_height"),(0x1195764,"raster_width"),(0x1194e60,"raster_right_overhang"),(0x11953a4,"raster_secondary_extent"),(0x119581c,"raster_top_overhang")
  ,(0x11953f4,"initialize_raster_layout"),(0x119eed8,"raster_find_property"),(0x11a0240,"raster_adjust_size")
  ,(0x1196c98,"outline_stroke_padding"),(0x1194eb0,"raster_stroke_padding"),(0x1196ce4,"binary_read"),(0x1194f70,"binary_read")
  ,(0x11984e4,"program_glyph_width"),(0x1199068,"program_render"),(0x1196fa4,"handle_query_width"),(0x1197044,"handle_render")
  ,(0x11972ac,"handle_glyph_width"),(0x11970f8,"handle_append_bitmap"),(0x1197228,"handle_bitmap"),(0x11a27fc,"raster_property")
  ,(0x119512c,"query_fallback"),(0x1196d28,"handle_weights"),(0x1194fb4,"raster_weights"),(0x1195224,"layout_glyph_width"),(0x1195294,"chained_glyph_width")
  ,(0x1196f08,"handle_init"),(0x1197384,"handle_dimensions"),(0x1197494,"cache_initialize"),(0x1197598,"cache_find_or_insert")
  ,(0x1197768,"cache_touch"),(0x11978d8,"cache_find"),(0x119799c,"cache_insert"),(0x11947e4,"layout_dimensions"),(0x1195938,"render_with_fallback_cache"),(0x1195bc8,"draw_raster_bitmap")
  ,(0x1196c20,"outline_height"),(0x1196c38,"outline_width"),(0x1196c50,"outline_secondary_extent"),(0x1196c68,"outline_right_overhang"),(0x1196c80,"outline_top_overhang")
  ,(0x1196f90,"outline_storage_size"),(0x1197f00,"outline_storage_size"),(0x1197e50,"program_destroy"),(0x1196eec,"handle_destroy")]
  ++ [(0x118df0c,"create_engine"),(0x118effc,"initialize_engine_backends"),(0x11818c8,"object_create"),(0x11811a0,"reset_options")
  ,(0x11805f0,"object_mode"),(0x1180290,"open_reference"),(0x11803fc,"close_reference"),(0x117f850,"compare_prefix")
  ,(0x1185b5c,"reference_at_index"),(0x118cc68,"read_directory"),(0x118d488,"install_methods")
  ,(0x1195210,"raster_storage_size"),(0x119f08c,"raster_storage_size"),(0x1194e30,"raster_destroy"),(0x1194e48,"raster_dimensions")]

fontFdlPublications :: [StagePublication]
fontFdlPublications = [publication address name | (address,name) <- entries]

fontFdlScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
fontFdlScenarios _bytes _resolve = concatMap cases fontFdlPublications
 where
  fixture = fontFixture
  engine = addressWord (fontEngineAddress fixture)
  layout = addressWord (fontLayoutAddress fixture)
  workspace = addressWord (fontWorkspaceAddress fixture)
  bitmap = addressWord (fontBitmapAddress fixture)
  output = addressWord (fontOutputAddress fixture)
  glyph = stockFontCode IconE14d
  outputs = [output,output+2,output+4,output+6]
  marker = Patch (Address output) (B.replicate 16 0xa5)
  make pub label args initial final result pointers = StageScenario (StageCaseName (label++"-"++show (addressWord (stageStockEntry pub)))) pub args initial final result pointers []
  cases pub = rootCases pub ++ supplementalCases pub
  rootCases pub = case addressWord (stageStockEntry pub) of
    address | address `elem` [0x1197e68,0x119f0fc] -> [make pub "actual-fdl-header" [0x3bc000,output] (base++fontIconPatches++[marker]) [wordsAt output [4,0xf726,0x100]] (Just 1) []]
    0x1197b98 -> [make pub "actual-program-initialize" [0x3bc000,0x814,workspace] (base++fontIconPatches++[Patch (fontWorkspaceAddress fixture) (B.replicate 0x814 0xa5)]) [word (workspace+8) 0x3bc000,word (workspace+0x14) 0xf726,word (workspace+0x2c) 0x3dced,word (workspace+0x38) 0x51,half (workspace+0x3c) 0xffff] (Just workspace) [], make pub "short-program-storage" [0x3bc000,0x813,workspace] (base++[marker]) [marker] (Just 0) []]
    0x1197f98 -> [make pub "genuine-e14d-outline-directory" (workspace:glyph:outputs) (fontOutlineInitial fixture++[marker]) [word (workspace+0x64) 0x3edf0,word (workspace+0x60) 0x66,half output 0,half (output+2) 500,half (output+4) 0,half (output+6) 0] (Just 1) [], make pub "outline-glyph-outside-font" (workspace:0xf726:outputs) (fontOutlineInitial fixture++[marker]) [marker] (Just 0) []]
    0x119f74c -> [make pub "genuine-e14d-raster-directory-byte-flags" (workspace:glyph:outputs) (fontRasterInitial fixture++[marker,Patch (Address (workspace+0x128)) (B.replicate 8 0x5a)]) [word (workspace+0xa0) 0x66,word (workspace+0xa4) 0x3edf0,half output 0,half (output+2) 500,half (output+4) 0,half (output+6) 0,Patch (Address (workspace+0x128)) (B.pack [0,0,0x5a,0x5a,0x5a,0x5a,0x5a,0x5a])] (Just 1) []]
    0x1197f18 -> [make pub "actual-fa-opcode" [workspace] (fontOutlineInitial fixture++[word (workspace+0xb4) 0x3edf0,word (workspace+0xb0) 102]) [word (workspace+0xb4) 0x3edf1,word (workspace+0xb0) 101] (Just 0xfa) []]
    0x1197f50 -> [make pub "actual-fa-body-length" [workspace] (fontOutlineInitial fixture++[word (workspace+0xb4) 0x3edf2,word (workspace+0xb0) 100]) [word (workspace+0xb4) 0x3edf4,word (workspace+0xb0) 98] (Just 98) []]
    0x117f51c -> [make pub "zero-count-alignment-prefix" [bitmap+1,0x7f,0] (base++[Patch (fontBitmapAddress fixture) (B.replicate 8 0xa5)]) [Patch (fontBitmapAddress fixture) (B.pack [0xa5,0x7f,0x7f,0x7f,0xa5,0xa5,0xa5,0xa5])] (Just (bitmap+1)) []]
    0x117f64c -> [make pub "forward-overlap-halfwords" [bitmap+4,bitmap,6] (base++[Patch (fontBitmapAddress fixture) (B.pack [0..11])]) [Patch (fontBitmapAddress fixture) (B.pack [0,1,2,3,0,1,2,3,0,1,10,11])] Nothing []]
    0x118ed64 -> [make pub "real-raster-metrics" [engine,output] (fontRasterInitial fixture++[marker]) [wordsAt output [32,28,4,0,0,32,0,4]] (Just 0) (fontRasterCodePointers fixture)]
    0x119fcbc -> [make pub "actual-raster-header-properties" [layout+4] (fontRasterInitial fixture++[word (workspace+0x54) 0xdeadbeef]) [word (workspace+0x48) 0x3bc000,word (workspace+0x54) 0xf726,word (workspace+0x6c) 0x3dced,word (workspace+0x83dc) 0x7c177] (Just 0) []]
    0x11a0340 -> [make pub "actual-half-em-icon-advance" [layout+4,glyph] (fontRasterInitial fixture) [word (workspace+0xa0) 0x66,word (workspace+0xa4) 0x3edf0] (Just 16) []]
    _ -> frontendCases pub
  object = addressWord (fontObjectAddress fixture)
  spanAddress = addressWord (fontSpanAddress fixture)
  request = addressWord (fontRequestAddress fixture)
  text = addressWord (fontTextAddress fixture)
  header = layout+0x600
  raw at values = Patch (Address at) (B.pack values)
  zero at bytes = Patch (Address at) (B.replicate bytes 0)
  halves at values = Patch (Address at) (B.concat [bytes | (index,value) <- zip [0..] values, let Patch _ bytes = half (at+index*2) value])
  outlineBytes = case halves 0
      [7,6,1,831,375,2,999,375,2,999,1759,2,855,1759,2,855,823,2,831,375,4
      ,7,6,1,191,375,2,511,1423,2,567,1759,2,455,1759,2,167,823,2,191,375,4
      ,6,5,1,831,375,2,855,823,2,567,1759,2,511,1423,2,831,375,4
      ,7,6,1,23,375,2,191,375,2,167,823,2,167,1759,2,23,1759,2,23,375,4] of Patch _ bytes -> bytes
  outlineOutput = Patch (fontBitmapAddress fixture) outlineBytes
  outlineInput = fontOutlineEngineInitial fixture
  outlinePointers = fontOutlineCodePointers fixture
  outlineRequest capacity = wordsAt request [glyph,0,bitmap,capacity,0xa5a5a5a5]
  relocatedIcon = [Patch (Address (addressWord address+0xa80000-0x3bc000)) bytes | Patch address bytes <- fontIconPatches]
  fallbackInput initial = initial ++ relocatedIcon ++
    [word (engine+28) (layout+0x900),word (engine+32) 1,word (layout+0x964) 0xa80000
    ,raw 0x3f4585 [0xfd,0x10,0,1,0xfd,0x10,0,1]]
  shapeInput = fontRectangleRasterInitial fixture ++ [wordsAt request [1,1,0x200e,1,text],halves text [65,0]]
  outlineShapeInput = outlineInput ++ [wordsAt request [1,1,0x200e,1,text],halves text [glyph,0]]
  shapeDimensions = [word spanAddress 1,half (spanAddress+0xc20) 0x200e,wordsAt (spanAddress+0xcfc) [8,16,32,16,512]]
  properties = [word 0x3bc03a 0x500,half 0x3bc500 1,half 0x3bc502 6,word 0x3bc504 16,halves 0x3bc508 [2,3,4,1,2]]
  frontendCases pub = case addressWord (stageStockEntry pub) of
    0x118dd48 ->
      [make pub "clear-raster-retains-bitmap" [engine,spanAddress] (fontRasterInitial fixture++[word spanAddress 7,half (spanAddress+0xc20) 0x200e])
        [word spanAddress 0,half (spanAddress+0xc20) 0,wordsAt (spanAddress+0xcfc) [0,0,0,0,0,bitmap,0,0]] (Just 0) []
      ,make pub "clear-null-engine-has-prefix-effects" [0,spanAddress] (fontRasterInitial fixture++[word spanAddress 7,half (spanAddress+0xc20) 0x200e])
        [word spanAddress 0,half (spanAddress+0xc20) 0,word (spanAddress+0xd00) 32] (Just 0xffffffff) []
      ,make pub "clear-outline-buffer-and-index" [engine,spanAddress]
        (outlineInput++[word (spanAddress+0xc30) bitmap,word (spanAddress+0xc2c) 7,raw bitmap (replicate 12 0xa5),raw (spanAddress+0xc34) (replicate 200 0xa5)])
        [raw bitmap (replicate 7 0++replicate 5 0xa5),zero (spanAddress+0xc34) 200,wordsAt (spanAddress+0xc24) [0,0,0,bitmap]] (Just 0) []]
    address | address `elem` [0x1182108,0x1182578] ->
      let outline = address == 0x1182108
          records = object + if outline then 0x478 else 0x90
          size = if outline then 64 else 20
          initial = fontRasterInitial fixture ++
            [word spanAddress 2,halves (spanAddress+4) [65,66],half (spanAddress+0xc20) 0x200e
            ,wordsAt (spanAddress+0xc24) [123,456],wordsAt (spanAddress+0xcfc) [8,12,7,12,84]
            ,halves (spanAddress+0x68) [65,0,0,0x102],wordsAt (spanAddress+0x70) [100,200]
            ,halves (spanAddress+0x68+60) [66,0,1,0],wordsAt (spanAddress+0x70+60) [300,400]]
          dimensions = if outline then [123,456,0,0,0] else [12,7,12,8,84,0]
       in [make pub "export-group-and-continuation" [object] initial
            [wordsAt header [2,spanAddress+4,records],half (header+12) 0x200e,wordsAt (header+16) dimensions
            ,wordsAt records [0,0x410001,2,100,200],wordsAt (records+size) [1,0x410001,2,300,400]] (Just 0) []]
    0x11822ec ->
      [make pub "genuine-e14d-full-outline-prepare" [object,text,1,3,output]
        (outlineShapeInput++[word output 0]) [word output header,word header 1,wordsAt (header+16) [1024,2048,spanAddress+0xc34,162,bitmap],outlineOutput] (Just 0) outlinePointers
      ,make pub "outline-rejects-bitmap-object" [object,text,1,3,output] (fontRasterInitial fixture++[marker]) [marker] (Just 0xfffffff4) []
      ,make pub "outline-null-text-error" [object,0,1,1,output] (outlineInput++[marker]) [marker] (Just 0xfffffff7) []]
    0x11826cc ->
      [make pub "authored-rectangle-full-bitmap-prepare" [object,text,1,3,output,0,0,0,0,0] (shapeInput++[word output 0])
        (shapeDimensions++[word output header,wordsAt (header+16) [16,32,16,8,512,bitmap],raw bitmap (replicate 512 15)]) (Just 0) (fontRasterCodePointers fixture)
      ,make pub "bitmap-prepare-external-target" [object,text,1,3,output,bitmap+0x2000,16,32,0,0] (shapeInput++[word output 0])
        [word output header,word (spanAddress+0xd10) (bitmap+0x2000),raw (bitmap+0x2000) (replicate 512 15)] (Just 0) (fontRasterCodePointers fixture)
      ,make pub "bitmap-rejects-short-external-target" [object,text,1,3,output,bitmap,8,1,0,0] (shapeInput++[word output 0])
        [word output header,word (header+32) 512] (Just 0xfffffff6) (fontRasterCodePointers fixture)
      ,make pub "bitmap-null-text-error" [object,0,1,1,output,0,0,0,0,0] (fontRasterInitial fixture++[marker]) [marker] (Just 0xfffffff7) []]
    address | address `elem` [0x1181ed0,0x1181c98] ->
      let (count,margin,offset) = if address == 0x1181ed0 then (0x2c,0x30,0x28) else (0x38,0x3c,0x34)
          initial = fontRasterInitial fixture ++
            [word spanAddress 1,wordsAt (spanAddress+0x70) [3,0,3,0,4],wordsAt (spanAddress+0xcfc) [8,10,6,12,72,bitmap,1,0]
            ,word (object+count) 2,word (object+margin) 2,word (object+offset) 1,word (object+0x24) 5,word (object+0x40) 1,raw bitmap (replicate 80 0x22)]
       in [make pub "padding-byte-rows" [object,spanAddress] initial [raw (bitmap+37) (replicate 9 15),raw (bitmap+49) (replicate 9 15)] Nothing []
          ,make pub "padding-bit-rows-write-two-not-four" [object,spanAddress] (initial++[word (spanAddress+0xcfc) 1,word (spanAddress+0xd08) 2,word (spanAddress+0xd14) 3])
            [raw (bitmap+6) [255,255,255,255,0x22,0x22]] Nothing []
          ,make pub "padding-null-target-bypass" [object,spanAddress] (initial++[word (spanAddress+0xd10) 0]) [raw bitmap (replicate 80 0x22)] Nothing []]
    0x1181458 ->
      [make pub "query-derived-ascent" [object,101] (fontRasterInitial fixture) [] (Just 28) (fontRasterCodePointers fixture)
      ,make pub "query-block-rounded-cache-budget" [object,200] (fontRasterInitial fixture++[word (layout+0x408) 1025]) [] (Just 2048) (fontRasterCodePointers fixture)
      ,make pub "query-unsupported-option" [object,44] (fontRasterInitial fixture) [] (Just 0xfffffffc) (fontRasterCodePointers fixture)
      ,make pub "query-null-object" [0,101] base [] (Just 0xfffffffe) []]
    address | address `elem` [0x117f390,0x11829c8] ->
      [make pub "authored-rectangle-shape-render-blend-cleanup" [object,text,1,0,0,0,0,0,output+0x200,output+0x204,0,output+0x100,output+0x400]
        (shapeInput++[wordsAt (output+0x200) [0x11111111,0x22222222],wordsAt (output+0x400) [bitmap+0x30000,output+0x800,0,0,0]
          ,zero (output+0x800) 0x1100,word (output+0x18d4) 16,raw (bitmap+0x30000) (replicate 2048 0xa5)
          ,word 0x166703c 0,word 0x1667044 0])
        [wordsAt (output+0x100) [16,32,1],half (output+0x10c) 65,word (output+0x408) 1
        ,raw (bitmap+0x30000) (replicate 2048 0x11),word header 0,word spanAddress 0,word (spanAddress+0xd10) bitmap] (Just 0) (fontRasterCodePointers fixture)
      ,make pub "draw-null-object" [0,text,1,0,0,0,0,0,0,0,0,0,0] base [] (Just 0xfffffffe) []]
    0x118e49c ->
      [make pub "authored-rectangle-full-shape-and-render" [engine,request,spanAddress,3] shapeInput
        (shapeDimensions++[half (engine+0xea4) 1,half (engine+0x60a) 1,raw bitmap (replicate 512 15)]) (Just 0) (fontRasterCodePointers fixture)
      ,make pub "genuine-e14d-shape-measure" [engine,request,spanAddress,1] outlineShapeInput
        [word spanAddress 1,wordsAt (spanAddress+0xc24) [1024,2048],half (engine+0x60a) 1] (Just 0) outlinePointers
      ,make pub "shape-count-cap-error" [engine,request,spanAddress,1] (shapeInput++[word (request+12) 51]) [word spanAddress 0] (Just 0xfffffff3) []]
    0x118e12c ->
      [make pub "finalize-aligned-byte-extent" [engine]
        (fontRasterInitial fixture++[half (engine+0x60a) 1,half (engine+0xea4) 1,word (engine+0xff0) 0x200e
          ,wordsAt (engine+0xea8) [0,1,1,9,32,0,0,0,0],word (engine+0x1078) 0])
        [word spanAddress 1,wordsAt (spanAddress+0xcfc) [8,9,32,12,384],word (engine+0x1078) 4] (Just 0) (fontRasterCodePointers fixture)]
    0x11953f4 ->
      [make pub "genuine-fdl-layout-initialize" [layout,0x3bc000,0x20000,workspace,engine] (fontRasterInitial fixture)
        [wordsAt layout [engine,0,32,32,0,0,0,0,0,0,1],word (workspace+0x54) 0xf726,word (workspace+0x48) 0x3bc000] (Just 0) []
      ,make pub "raster-initialize-short-workspace" [layout,0x3bc000,0x100,workspace,engine] (fontRasterInitial fixture) [word layout engine] (Just 0xfffffffa) []]
    0x119eed8 ->
      [make pub "find-real-adjustment-record" [workspace,10] (fontRasterInitial fixture) [] (Just 0x7c177) []
      ,make pub "missing-weight-record" [workspace,6] (fontRasterInitial fixture) [] (Just 0) []]
    0x11a0240 ->
      [make pub "authored-half-size-adjustment" [layout+4,0xf000,output,workspace] (fontRasterInitial fixture++[half 0x438185 2000,marker])
        [word output 1,wordsAt (layout+8) [16,16],wordsAt (workspace+0x10c) [64,64]] (Just 0) []
      ,make pub "adjustment-rejection-restores-request" [layout+4,0xf000,output,workspace] (fontRasterInitial fixture++[half 0x438185 1,marker])
        [word output 1,wordsAt (layout+8) [32,32],wordsAt (workspace+0x10c) [128,128]] (Just 0xffffffff) []
      ,make pub "genuine-e14d-bypasses-adjustment-range" [layout+4,glyph,output,workspace] (fontRasterInitial fixture++[marker])
        [word output 0,wordsAt (layout+8) [32,32]] (Just 0) []]
    address | address `elem` [0x1196ce4,0x1194f70] ->
      [make pub "unaligned-little-endian-word" [bitmap,1,4] [raw bitmap [0xaa,0x12,0x34,0x56,0x78]] [] (Just 0x78563412) []
      ,make pub "zero-byte-count-reads-first-byte" [bitmap,1,0] [raw bitmap [0xaa,0x76]] [] (Just 0x76) []
      ,make pub "arm-register-shift-wraps-after-thirty-two-bytes" [bitmap,0,33] [raw bitmap ([1]++replicate 31 0++[2])] [] (Just 3) []]
    address | address `elem` [0x11956ac,0x1195764,0x1194e60,0x11953a4,0x119581c] ->
      let value = if address == 0x11956ac then 36 else if address == 0x1195764 then 42 else if address == 0x119581c then 4 else 6
       in [make pub "styled-raster-metric-and-secondary" [layout]
          (fontRasterInitial fixture++[word (layout+16) 1,word (layout+20) 2,word (layout+24) 4,word (layout+32) 1,word (layout+36) 6])
          [word (layout+16) 1] (Just value) []]
    0x1194eb0 ->
      [make pub "right-bottom-stroke-padding" [layout,output,output+4,output+8,output+12] (fontRasterInitial fixture++[word (layout+16) 5,word (layout+20) 3,marker])
        [wordsAt output [0,3,0,3]] (Just 0) []
      ,make pub "nullable-raster-stroke-padding" [layout,0,0,0,0] (fontRasterInitial fixture) [] (Just 0) []]
    0x1196c98 -> [make pub "source-constant-outline-padding" [layout,output,output+4,output+8,output+12] [marker] [wordsAt output [0,0,0,0]] (Just 0) []]
    0x11984e4 ->
      [make pub "genuine-e14d-program-advance" [workspace,glyph,0] (fontOutlineInitial fixture) [word (workspace+4) 0] (Just 1024) []
      ,make pub "missing-outline-glyph" [workspace,0xf726,0] (fontOutlineInitial fixture) [] (Just 0xfffffffe) []
      ,make pub "null-outline-program" [0,glyph,0] base [] (Just 0xffffffff) []]
    0x1199068 ->
      [make pub "genuine-e14d-complete-outline-program" [workspace,request] (fontOutlineInitial fixture++[outlineRequest 256])
        [word (request+16) 81,word (workspace+0xd0) 81,outlineOutput] (Just 0) []
      ,make pub "outline-program-capacity-partial-write" [workspace,request] (fontOutlineInitial fixture++[outlineRequest 80,raw bitmap (replicate 162 0xa5)])
        [word (request+16) 81,Patch (fontBitmapAddress fixture) (B.take 120 outlineBytes),raw (bitmap+120) (replicate 42 0xa5)] (Just 0xfffffffc) []]
    0x1196fa4 ->
      [make pub "genuine-e14d-handle-advance" [layout,glyph,0] outlineInput [] (Just 1024) []
      ,make pub "outline-width-real-fallback-and-restoration" [layout,glyph,0] (fallbackInput outlineInput)
        [word (layout+4) workspace,word (workspace+8) 0x3bc000] (Just 1024) []]
    0x1197044 ->
      [make pub "genuine-e14d-handle-render-and-width" [layout,output,request] (outlineInput++[outlineRequest 256,marker])
        [word output 1024,word (request+16) 81,outlineOutput] (Just 0) []
      ,make pub "outline-render-real-fallback-and-restoration" [layout,output,request] (fallbackInput outlineInput++[outlineRequest 256,marker])
        [word output 1024,word (workspace+8) 0x3bc000,word (request+16) 81,outlineOutput] (Just 0) []]
    0x11972ac -> [make pub "outline-advance-unit-scaled-bias" [layout,glyph] (outlineInput++[word (engine+0x1068) 3,word (engine+0x103c) 32]) [] (Just 1216) []]
    0x11970f8 ->
      [make pub "genuine-outline-appends-byte-count" [layout,glyph,0,output,output+4,output+8]
        (outlineInput++[word (engine+0xfe4) 0x8000,wordsAt output [2,bitmap,99],raw bitmap [0xaa,0xbb]])
        [wordsAt output [164,bitmap,81],Patch (Address (bitmap+2)) outlineBytes,raw bitmap [0xaa,0xbb]] (Just 0) []
      ,make pub "outline-scratch-shortage-preserves-outputs" [layout,glyph,0,output,output+4,output+8]
        (outlineInput++[word (layout+0x48c) 8191,wordsAt output [2,bitmap,99]]) [wordsAt output [2,bitmap,99]] (Just 0xfffffff6) []]
    0x1197228 ->
      [make pub "genuine-outline-bitmap-uses-byte-capacity" [layout,glyph,512,bitmap,output] (outlineInput++[marker]) [word output 81,outlineOutput,zero (bitmap+162) 350] (Just 0) []
      ,make pub "outline-capacity-error-preserves-word-output" [layout,glyph,160,bitmap,output] (outlineInput++[marker])
        [marker,Patch (fontBitmapAddress fixture) (B.take 120 outlineBytes),zero (bitmap+120) 40] (Just 0xfffffffc) []
      ,make pub "missing-outline-clears-word-output" [layout,0xf726,32,bitmap,output] (outlineInput++[marker])
        [word output 0,zero bitmap 32] (Just 0) []]
    address | address `elem` [0x1196d28,0x1194fb4] ->
      [make pub "authored-font-weight-proportions" [layout,output] ((if address == 0x1196d28 then outlineInput else fontRasterInitial fixture)++properties++[marker])
        [halves output (if address == 0x1196d28 then [409,614,819,206,409] else [6,9,12,5,6])] (Just 0) []]
    0x11a27fc ->
      [make pub ("authored-adjustment-property-"++show key) [layout+4,0xf000,key] (fontRasterInitial fixture++[half 0x438185 2000]) [] (Just value) []
      | (key,value) <- [(1,2000),(2,1),(4,16),(5,16),(6,0),(10,0x7c177),(3,0)]]
    0x119512c ->
      [make pub "primary-default-width" [layout,layout+4,glyph,4] (fontRasterInitial fixture) [word (layout+64) 0x3bc000] (Just 32) []
      ,make pub "fallback-default-width-restores-primary" [layout,layout+4,glyph,4] (fallbackInput (fontRasterInitial fixture)) [word (layout+64) 0x3bc000] (Just 32) []]
    0x1195224 ->
      [make pub "raster-fallback-advance-and-header-restoration" [layout,layout+4,glyph] (fallbackInput (fontRasterInitial fixture)) [word (layout+64) 0x3bc000,word (workspace+0x48) 0x3bc000] (Just 16) []]
    0x1195294 ->
      [make pub "real-raster-advance-adds-bias" [layout,glyph] (fontRasterInitial fixture++[word (engine+0x1068) 3]) [] (Just 19) []
      ,make pub "missing-raster-glyph-has-no-bias" [layout,0xf726] (fontRasterInitial fixture++[word (engine+0x1068) 3]) [] (Just 0) []]
    0x1196f08 ->
      [make pub "actual-outline-handle-initialize" [layout,0x3bc000,0x814,workspace,engine] outlineInput
        [wordsAt layout [engine,workspace,0x3bc000,0],word (workspace+0x14) 0xf726] (Just 0) []
      ,make pub "outline-handle-short-storage" [layout,0x3bc000,0x813,workspace,engine] outlineInput [wordsAt layout [engine,0,0x3bc000,0]] (Just 0xfffffffa) []]
    0x1197384 ->
      [make pub "horizontal-outline-dimensions-and-bias" [layout,glyph,output,output+4] (outlineInput++[word (engine+0x1068) 3,word (engine+0x103c) 32,marker])
        [wordsAt output [1216,0]] (Just 1024) []
      ,make pub "vertical-missing-glyph-dimensions" [layout,0xf726,output,output+4] (outlineInput++[word (layout+12) 1,marker]) [wordsAt output [0,0]] (Just 0) []]
    0x11947e4 ->
      [make pub "raster-geometry-stroke-and-secondary" [layout+4,output,output+4,output+8,output+12,output+16,output+20]
        (fontRasterInitial fixture++[word (layout+16) 1,word (layout+20) 2,word (layout+24) 4,word (layout+32) 1,word (layout+36) 6])
        [wordsAt output [42,36,42,2,2,6]] Nothing []]
    0x1195938 ->
      [make pub "authored-rectangle-cached-render-frontend" [layout,output,layout+4,request]
        (fontRectangleRasterInitial fixture++[wordsAt request [65,0,0,0,0,32,bitmap,0,0],marker])
        [word output 16,wordsAt (request+4) [0,0,32,32],raw bitmap (replicate 1024 15)] (Just 0) []]
    0x1195bc8 ->
      [make pub "authored-rectangle-byte-copy-frontend" [layout,65,32,32,0,0,32,bitmap,0,0x200] (fontRectangleRasterInitial fixture)
        [raw bitmap (replicate 1024 15)] (Just 16) []
      ,make pub "raster-copy-clips-to-target-stride" [layout,65,16,32,0,0,16,bitmap,0,0x200] (fontRectangleRasterInitial fixture)
        [raw bitmap (replicate 512 15)] (Just 16) []
      ,make pub "raster-scratch-shortage" [layout,65,32,32,0,0,32,bitmap,0,0x200] (fontRectangleRasterInitial fixture++[word (layout+0x48c) 1023])
        [zero bitmap 1024] (Just 0) []]
    address | address `elem` [0x1196c20,0x1196c38] -> [make pub "source-constant-em-extent" [layout] base [] (Just 2048) []]
    address | address `elem` [0x1196c50,0x1196c68,0x1196c80] -> [make pub "source-constant-zero-overhang" (if address == 0x1196c68 then [layout,glyph] else [layout]) base [] (Just 0) []]
    address | address `elem` [0x1196f90,0x1197f00] -> [make pub "source-constant-outline-storage" [] base [] (Just 0x814) []]
    0x1197e50 -> [make pub "source-constant-program-destroy" [workspace] base [] (Just 0) []]
    0x1196eec -> [make pub "destroy-outline-handle" [layout] outlineInput [word (layout+4) workspace] (Just 0) []]
    _ -> allocationAndCacheCases pub
  heap = 0x2063800
  freshHeap = [word 0x1667040 0,word 0x1666ff8 0,raw heap (replicate 0x6000 0x5a)]
  allocatedHeap =
    [zero heap 0x900,word 0x1667040 heap,word 0x1666ff8 (heap+0x40)
    ,wordsAt (heap+8) [0x848,0x400,0xae000,0x810,0,34,0xae000,0,0,0]
    ,word (heap+0x38) 0x400,word (heap+0x440) 0x400,raw (heap+0x448) (replicate 0x400 0x5a)
    ,word (heap+0x848) 0xad7b0,word (heap+0x850) 0x848]
  exhaustedHeap = allocatedHeap ++
    [word (heap+8) 0xadbf0,word (heap+20) 0xadbb8,word (heap+28) 0xad3c2
    ,word (heap+0x848) 0xad3a0,word (heap+0xadbf0) 0x408,word (heap+0xadbf8) 0xadbf0]
  cache = output+0x2000
  cacheConfiguration = cache+0x100
  cacheEntries = cache+0x200
  cacheScratch = cache+0x300
  cacheRing = cache+0x400
  cacheDescriptors = cache+0x500
  cacheIncoming = cache+0x800
  cachePixels = cache+0x1000
  cacheBase =
    [zero cache 0x900,zero cachePixels 128,raw bitmap [1,2,3,4]
    ,wordsAt cache [0,1,0,0,128,cachePixels,cacheConfiguration,cacheEntries,cacheScratch,cacheRing,cacheDescriptors]
    ,wordsAt cacheConfiguration [0,0,2,8],half (cacheEntries+6) 0,half (cacheEntries+14) 1
    ,wordsAt cacheIncoming [0x20002,0x02020000,0x1234,bitmap]]
  cacheFull = cacheBase ++
    [wordsAt (cache+8) [1,2,0,cachePixels+128]
    ,wordsAt cacheEntries [0x02020041,0,0x02020042,0x10001]
    ,wordsAt cacheRing [0x02020041,0x1234,0x02020042,0x1234]
    ,wordsAt cacheDescriptors [0x20002,0x02020000,0x1234,cachePixels,0x20002,0x02020000,0x1234,cachePixels+64]]
  promotedCache =
    [half (cacheEntries+4) 1,half (cacheEntries+12) 0,wordsAt cacheRing [0x02020042,0x1234,0x02020041,0x1234]]
  allocationAndCacheCases pub = case addressWord (stageStockEntry pub) of
    0x117f3f0 ->
      [make pub "first-allocation-owns-real-heap" [17] freshHeap
        [word 0x1667040 heap,word 0x1666ff8 (heap+0x40),word (heap+8) 0x440,word (heap+20) 0x408
        ,word (heap+28) 17,word (heap+0x38) 0x400,word (heap+0x40) 0x38,word (heap+0x440) 0xadbb8] (Just (heap+0x40)) []
      ,make pub "allocation-exhaustion-without-reclaimer" [0xae000] freshHeap [word 0x1667040 heap,word 0x1666ff8 0] (Just 0) []]
    0x117f45c ->
      [make pub "resize-retains-minimum-sized-allocation" [heap+0x448,512] allocatedHeap
        [word (heap+0x440) 0x400,raw (heap+0x448) (replicate 0x400 0x5a)] (Just (heap+0x448)) []
      ,make pub "resize-copies-requested-length-and-releases-old" [heap+0x448,0x800] allocatedHeap
        [raw (heap+0x850) (replicate 0x400 0x5a),word (heap+0xc50) 0x800,word (heap+24) 0x408
        ,word (heap+20) 0x1018,word (heap+0x448) 0x1050] (Just (heap+0x850)) []
      ,make pub "resize-zero-releases-real-allocation" [heap+0x448,0] allocatedHeap
        [word (heap+8) 0x440,word (heap+0x440) 0xadbb8,word (heap+24) 0x408] (Just 0) []]
    0x117f4bc ->
      [make pub "release-coalesces-following-free-block" [heap+0x448] allocatedHeap
        [word (heap+8) 0x440,word (heap+0x440) 0xadbb8,word (heap+0x448) 0x440,word (heap+24) 0x408] Nothing []
      ,make pub "first-font-allocation-is-protected" [heap+0x40] allocatedHeap
        [word (heap+8) 0x848,word (heap+24) 0,word (heap+0x38) 0x400] Nothing []]
    0x118df74 ->
      [make pub "raster-workspace-grows-by-source-increment" [engine]
        (fontRasterInitial fixture++freshHeap++[word (engine+0x1030) 0x400,word (engine+0x1034) 0])
        [wordsAt (engine+0x1030) [0x2400,heap+0x40],word 0x1666ff8 (heap+0x40)] (Just 0) []
      ,make pub "workspace-growth-failure-retains-release-effects" [engine]
        (fontRasterInitial fixture++exhaustedHeap++[word (engine+0x1030) 0x400,word (engine+0x1034) (heap+0x448)])
        [wordsAt (engine+0x1030) [0x400,heap+0x448],word (heap+24) 0x408,word (heap+0x448) 0xadbf0,word (heap+0xadbf8) 0x440] (Just 0xfffffffa) []]
    0x1197494 ->
      [make pub "initialize-real-cache-slot-directory" [cache]
        (cacheBase++[word (cache+4) 0,half (cacheEntries+6) 7,half (cacheEntries+14) 8])
        [wordsAt (cache+4) [1,0,0],half (cacheEntries+6) 0,half (cacheEntries+14) 1] Nothing []
      ,make pub "initialized-cache-bypass-preserves-slots" [cache]
        (cacheBase++[half (cacheEntries+6) 7,half (cacheEntries+14) 8])
        [half (cacheEntries+6) 7,half (cacheEntries+14) 8] Nothing []]
    0x1197598 ->
      [make pub "cache-search-existing-key-and-variant" [cache,0x02020041,0x1234,cacheEntries,cache+12,0xffffffff,2] cacheFull
        [word (cache+12) 2] (Just 0) []
      ,make pub "cache-search-missing-variant-control" [cache,0x02020041,0x1235,cacheEntries,cache+12,0xffffffff,2] cacheFull
        [wordsAt cacheEntries [0x02020041,0,0x02020042,0x10001]] (Just 0xffffffff) []
      ,make pub "cache-table-first-insertion" [cache,0x02020041,0x1234,cacheEntries,cache+12,0,2] cacheBase
        [word (cache+12) 1,word cacheEntries 0x02020041,half (cacheEntries+6) 0] (Just 0) []]
    0x1197768 ->
      [make pub "cache-hit-promotes-one-ring-position" [cache,65,2,2,0x1234,0] cacheFull promotedCache (Just 1) []
      ,make pub "cache-touch-empty-directory" [cache,65,2,2,0x1234,0] cacheBase [word (cache+12) 0] (Just 0) []]
    0x11978d8 ->
      [make pub "cache-find-returns-stored-descriptor-and-promotes" [cache,65,2,2,0x1234,0] cacheFull promotedCache (Just cacheDescriptors) []
      ,make pub "cache-variant-mismatch-preserves-ring" [cache,65,2,2,0x1235,0] cacheFull
        [wordsAt cacheRing [0x02020041,0x1234,0x02020042,0x1234]] (Just 0) []
      ,make pub "cache-size-bound-bypass" [cache,65,9,2,0x1234,0] cacheFull [word (cache+12) 2] (Just 0) []]
    0x119799c ->
      [make pub "cache-insert-copies-real-pixels-and-reserves-slot" [cache,65,2,2,cacheIncoming] cacheBase
        [wordsAt (cache+8) [1,1,64,cachePixels+64],word cacheEntries 0x02020041,half (cacheEntries+4) 1
        ,wordsAt cacheDescriptors [0x20002,0x02020000,0x1234,cachePixels],raw cachePixels [1,2,3,4]] (Just 1) []
      ,make pub "cache-full-replaces-first-sorted-entry" [cache,64,2,2,cacheIncoming] cacheFull
        [word cacheEntries 0x02020040,word cacheRing 0x02020040,raw cachePixels [1,2,3,4],word (cache+12) 2] (Just 1) []
      ,make pub "cache-full-inserts-before-eviction" [cache,63,2,2,cacheIncoming] (cacheFull++[word (cache+8) 0])
        [wordsAt cacheEntries [0x0202003f,0x10001,0x02020041,0],word (cacheRing+8) 0x0202003f,raw (cachePixels+64) [1,2,3,4]] (Just 1) []
      ,make pub "cache-full-inserts-after-eviction" [cache,67,2,2,cacheIncoming] cacheFull
        [wordsAt cacheEntries [0x02020042,0x10001,0x02020043,0],word cacheRing 0x02020043,raw cachePixels [1,2,3,4]] (Just 1) []]
    _ -> constructorCases pub
  directoryField backend offset = workspace + offset + if backend == RasterDirectory then 0x40 else 0
  directoryInput backend encoding =
    (if backend == RasterDirectory then fontRasterInitial fixture else fontOutlineInitial fixture) ++
    [word (directoryField backend 0x14) (if encoding == CompactDirectory then 66 else 0x20000)
    ,word (directoryField backend 0x28) 0x80,word (directoryField backend 0x2c) 0x300
    ,word (directoryField backend 0x34) (if encoding == CompactDirectory then 0 else 0x101)
    ,word (directoryField backend 0x38) (if encoding == CompactDirectory then 0x80 else 0x100)
    ,zero 0x3bc080 0x200] ++
    if encoding == CompactDirectory then
      [half 0x3bc01c 66,word 0x3bc02e 0x80,word 0x3bc032 0x300,half 0x3bc038 0
      ,raw 0x3bc143 [0,0,0,6,0,0],raw 0x3bc300 [0xfa,0,2,0,0x71,0]]
    else
      [half 0x3bc01c 0,word 0x3bc02e 0x80,word 0x3bc032 0x300,half 0x3bc038 0x101
      ,wordsAt 0x3bc080 [0x20000,0x80],halves 0x3bc088 [if encoding == DirectPageDirectory then 1 else 0,2]
      ,wordsAt 0x3bc094 [0x40,0],half (directoryField backend 0x3c) (if encoding == DirectPageDirectory then 1 else 0)
      ,half (directoryField backend 0x3e) 2,word (directoryField backend 0x40) 0x8c,word (directoryField backend 0x44) 0xffffffff
      ,raw 0x3bc300 [0,0xfa,0,2,0,0x71,0]] ++
      if encoding == DirectPageDirectory then
        [halves 0x3bc0c0 [8,1,0],raw 0x3bc1c3 [0,0,0,7,0,0]]
      else [halves 0x3bc0c0 [1,65,65,0],raw 0x3bc100 [0,0,0,7,0,0]]
  supplementalCases pub = case addressWord (stageStockEntry pub) of
    0x117f51c ->
      [make pub "fill-aligned-words-with-unaligned-prefix-and-tail" [bitmap+1,0x7f,9]
        (base++[raw bitmap (replicate 12 0xa5)]) [raw bitmap (0xa5:replicate 9 0x7f++[0xa5,0xa5])] (Just (bitmap+1)) []
      ,make pub "fill-aligned-zero-count-still-writes-four-byte-prefix" [bitmap,0x3c,0]
        (base++[raw bitmap (replicate 8 0xa5)]) [raw bitmap (replicate 4 0x3c++replicate 4 0xa5)] (Just bitmap) []]
    address | address `elem` [0x1197f98,0x119f74c] ->
      let backend = if address == 0x1197f98 then OutlineDirectory else RasterDirectory
          cursor = directoryField backend 0x64
          budget = directoryField backend 0x60
       in [make pub ("authored-"++show encoding) (workspace:code:outputs) (directoryInput backend encoding++[marker])
            [word cursor expectedCursor,word budget 6,halves output [0,0,0,0]] (Just 1) []
          | (encoding,code,expectedCursor) <- [(CompactDirectory,65,0x300),(DirectPageDirectory,0x10041,0x301),(SparsePageDirectory,0x10041,0x301)]] ++
          [make pub "cached-direct-page-uses-cached-header" (workspace:0x10041:outputs)
            (directoryInput backend DirectPageDirectory++[marker,word (directoryField backend 0x44) 1,word (directoryField backend 0x48) 0
              ,halves (directoryField backend 0x4c) [8,255,1],word (directoryField backend 0x54) 0xc4,word 0x3bc094 0x55555555])
            [word cursor 0x301,word budget 6,halves output [0,0,0,0]] (Just 1) []
          ,make pub "missing-direct-page-sentinel-preserves-advances" (workspace:0x10041:outputs)
            (directoryInput backend DirectPageDirectory++[marker,half 0x3bc0c4 0xffff])
            [marker,word (directoryField backend 0x44) 1] (Just 0) []
          ,make pub "flag-only-record-has-failure-prefix-effects" (workspace:0x10041:outputs)
            (directoryInput backend DirectPageDirectory++[marker,raw 0x3bc1c3 [0,0,0,1,0,0]])
            [halves output [0,0,0,0]] (Just 0) []]
    0x119fcbc ->
      [make pub ("raster-configure-rejection-"++label) [layout+4]
        (fontRasterInitial fixture++[word (workspace+0x54) 0xdeadbeef]++patches) [word (workspace+0x54) 0xdeadbeef] (Just result) []
      | (label,patches,result) <-
        [("width",[word (layout+8) 401],0xffffffff),("edge-mode",[word (layout+16) 6],0xfffffffe)
        ,("stroke",[word (layout+16) 1,word (layout+20) 11],0xfffffffd),("padding",[word (layout+24) 33],0xfffffff8)
        ,("format",[word (layout+40) 3],0xfffffffb),("palette",[raw (layout+44) [15]],0xfffffffa)
        ,("null-resource",[word (layout+64) 0],0xfffffff9),("workspace",[word (layout+72) 0],0xfffffffc)]]
    0x118ed64 ->
      [make pub "refresh-raster-metrics-through-real-initializer" [engine,output] (fontRasterInitial fixture++[word (engine+0x1070) 1,marker])
        [wordsAt output [32,28,4,0,0,32,0,4],word (workspace+0x54) 0xf726] (Just 0) (fontRasterCodePointers fixture)]
    0x117f51c ->
      [make pub "aligned-zero-count-still-writes-four" [bitmap,0x11,0] [raw bitmap (replicate 8 0xa5)]
        [raw bitmap [0x11,0x11,0x11,0x11,0xa5,0xa5,0xa5,0xa5]] (Just bitmap) []]
    0x117f64c ->
      [make pub "forward-byte-overlap-propagates-first-byte" [bitmap+1,bitmap,5] [raw bitmap [0..9]]
        [raw bitmap [0,0,0,0,0,0,6,7,8,9]] Nothing []]
    0x1195938 ->
      [make pub "real-raster-coverage-is-inserted-into-real-cache" [layout,output,layout+4,request]
        (rasterCacheInput++[wordsAt request [65,0,0,0,0,32,bitmap,0,0],zero bitmap 1024])
        [word output 16,wordsAt (cache+8) [1,1,1024,cachePixels+1024]
        ,wordsAt cacheDescriptors [0x200020,0x20200000,1,cachePixels],raw cachePixels (replicate 1024 15)] (Just 0) []]
    0x1195bc8 ->
      [make pub "mono-byte-expansion-uses-previous-destination-bits" [layout,65,32,32,0,0,32,bitmap,0,0x200] monoRectangleInput
        [raw bitmap (concat (replicate 32 (255:replicate 7 0++255:replicate 23 0)))] (Just 16) []
      ,make pub "mono-packed-output-preserves-existing-ink" [layout,65,32,32,0,0,4,bitmap,0,0x100]
        (monoRectangleInput++[raw bitmap (replicate 128 0x20)])
        [raw bitmap (concat (replicate 32 [255,255,0x20,0x20]))] (Just 16) []
      ,make pub "mono-packed-clip-clears-source-tail-rows" [layout,65,32,32,0,0,4,bitmap,output+0x40,0x100]
        (monoRectangleInput++[raw bitmap (replicate 128 0x20),wordsAt (output+0x40) [0,0,0,16,0,0,0]])
        [raw bitmap (concat (replicate 16 [255,255,0x20,0x20])++replicate 64 0x20),zero (bitmap+0x10040) 64] (Just 16) []
      ,make pub "output-format-bypass-still-executes-real-vm" [layout,65,32,32,0,0,32,bitmap,0,0] (fontRectangleRasterInitial fixture)
        [zero bitmap 1024,raw (bitmap+0x10000) (replicate 1024 15)] (Just 16) []
      ,make pub "raster-vm-error-control-preserves-target" [layout,65,32,32,0,0,32,bitmap,0,0x200]
        (fontRectangleRasterInitial fixture++[raw 0x3bc20a [1],raw bitmap (replicate 1024 0x33)])
        [raw bitmap (replicate 1024 0x33),zero (bitmap+0x10000) 1024] (Just 0) []
      ,make pub "real-raster-draw-populates-real-cache" [layout,65,32,32,0,0,32,bitmap,0,0x200] rasterCacheInput
        [raw bitmap (replicate 1024 15),raw cachePixels (replicate 1024 15),word (cache+12) 1] (Just 16) []
      ,make pub "cached-coverage-bypasses-corrupted-command-stream" [layout,65,32,32,0,0,32,bitmap,0,0x200]
        (rasterCacheInput++[wordsAt (cache+8) [1,1,1024,cachePixels+1024],word cacheEntries 0x20200041,half (cacheEntries+4) 1
          ,wordsAt (cacheRing+8) [0x20200041,1],wordsAt cacheDescriptors [0x200020,0x20200000,1,cachePixels]
          ,raw cachePixels (replicate 1024 15),raw 0x3bc20a [1],zero bitmap 1024])
        [raw bitmap (replicate 1024 15),wordsAt (workspace+0xa0) [18,0x206]] (Just 16) []]
    _ -> []
  monoRectangleInput = fontRectangleRasterInitial fixture ++
    [word (layout+40) 0,word (engine+0x105c) 0,word (workspace+36) 0,wordsAt (workspace+0x10c) [32,32],word (workspace+0x13c) 0]
  rasterCacheInput = fontRectangleRasterInitial fixture ++ cacheBase ++
    [zero (cache-44) 44,word (layout+0x470) (cache-44),word (cache-32) 8192,word (cache-28) (bitmap+0x10000)
    ,word (cacheConfiguration+12) 32,word (cache+16) 2048,zero cachePixels 2048,zero bitmap 1024]
  ascii address value = raw address [fromIntegral (fromEnum character) | character <- value]
  constructorHeap = [word 0x1667040 0,word 0x1666ff8 0,zero heap 0xae000]
  constructorReference = output+0x1000
  constructorManager = output+0x1200
  constructorRegistry kind =
    [zero constructorReference 0x74,word constructorReference 0,word (constructorReference+4) kind
    ,ascii (constructorReference+16) "FONT_ICON.FDL\0",word constructorManager constructorReference
    ,word (constructorReference+0x70) constructorManager]
  openedConstructorReference = constructorRegistry 0 ++
    [word (constructorReference+0x64) 0x3bc000,word (constructorReference+0x68) 0x7cfd1,word (constructorReference+0x6c) 1]
  constructorInput kind = base ++ fontIconPatches ++ constructorHeap ++ constructorRegistry kind
  constructorAuxiliary = constructorReference+0x74
  constructorAuxiliaryInput kind =
    [zero constructorAuxiliary 0x74,wordsAt constructorAuxiliary [1,kind]
    ,ascii (constructorAuxiliary+16) "FONT_ICON.FDL\0",word (constructorAuxiliary+0x70) constructorManager]
  iconConstructorResource =
    [zero 0x3bc000 0x700,ascii 0x3bc000 "DCW ICON",raw 0x3bc008 [9,11,1,0]
    ,raw 0x3bc020 [1,0,0,0],word (0x3bc024+65*4) 0x600,raw 0x3bc620 [5,3,168,80,248]]
  bdfConstructorResource = ascii 0x3bc000
    "STARTFONT 2.2\nFONT fixture\nSIZE 8 75 75\nFONTBOUNDINGBOX 5 7 -1 -2\nSTARTPROPERTIES 0\nENDPROPERTIES\nCHARS 1\nSTARTCHAR A\nENCODING 65\nSWIDTH 500 0\nDWIDTH 4 0\nBBX 5 3 1 -1\nBITMAP\nA8\n50\nF8\nENDCHAR\nENDFONT\n"
  fallbackMethods = [(Address (addressWord cell-0xfcc),target) | (cell,target) <- constructorMethods engine IconMethods]
  initializedEngine = heap+0x40
  initializedMethods = constructorMethods initializedEngine RasterMethods
  initializerInput = base ++ fontIconPatches ++ constructorHeap ++ openedConstructorReference ++ [word output 0]
  initializerArguments = [0x118360c,constructorReference,4,32,output,0,0,0,0]
  objectRasterEngine = heap+0x8a050
  objectOutlineEngine = heap+0x66050
  objectBuiltinEngine = heap+0x13450
  objectConstructed = heap+0x40
  constructorCases pub = case addressWord (stageStockEntry pub) of
    0x118df0c ->
      [make pub "create-engine-allocates-and-initializes-real-storage" [output] (base++freshHeap++[word output 0])
        [word output initializedEngine,word initializedEngine 0,word (initializedEngine+8) 0x107c
        ,word (initializedEngine+12) initializedEngine,word (initializedEngine+0x64) (initializedEngine+0x107c)
        ,word (initializedEngine+0x68) (initializedEngine+0x1088),word (initializedEngine+0x1188) 0x5a5a5a5a] (Just 0x1188) []
      ,make pub "create-existing-engine-does-not-allocate" [output] (base++[word output engine,word 0x1667040 0])
        [word output engine,word 0x1667040 0] (Just 0x1188) []
      ,make pub "create-engine-oom-preserves-output" [output] (base++exhaustedHeap++[word output 0]) [word output 0] (Just 0xfffffffb) []]
    0x118effc ->
      [make pub "initialize-actual-fdl-raster-engine-and-script-directory" initializerArguments initializerInput
        (methodWords initializedMethods ++
        [word output initializedEngine,word (initializedEngine+0xff4) 4,word (initializedEngine+0x1030) 0x83ec
        ,word (initializedEngine+0x1034) (heap+0x1448),wordsAt (initializedEngine+0x103c) [32,32]
        ,word (initializedEngine+0x70) 0x118360c,word (initializedEngine+0x74) 16,word (initializedEngine+0x78) 16
        ,half (initializedEngine+0x500) 1,half (initializedEngine+0x502) 1,word (initializedEngine+0x504) (initializedEngine+0x78)
        ,word (initializedEngine+0x1088) initializedEngine,word (heap+0x1448+0x48) 0x3bc000]) (Just 0) initializedMethods
      ,make pub "initializer-null-resource-precedes-null-output" [0x118360c,constructorReference,4,32,0,0,0,0,0]
        (base++constructorRegistry 0++[marker]) [marker] (Just 0xfffffffd) []
      ,make pub "initializer-bad-format-keeps-prefix-and-releases-workspace" initializerArguments
        (initializerInput++[half 0x3bc036 2])
        (methodWords initializedMethods++[word output 0,word (heap+8) 0x1440,word (initializedEngine+0x1088) initializedEngine]) (Just 0xfffffffc) initializedMethods
      ,make pub "initializer-rejects-unknown-mode-after-real-engine-allocation" [0x118360c,constructorReference,8,32,output,0,0,0,0]
        initializerInput [word output 0,word (initializedEngine+0xff4) 8,word (heap+8) 0x1440] (Just 0xfffffff8) []]
      ++ [make pub ("initializer-actual-raster-depth-"++show mode) [0x118360c,constructorReference,mode,32,output,0,0,0,0]
          initializerInput (methodWords initializedMethods++
          [word output initializedEngine,word (initializedEngine+0xff4) mode,word (initializedEngine+0x105c) depth
          ,word (initializedEngine+0x106c) format,word (heap+0x1448+36) depth]) (Just 0) initializedMethods
         | (mode,depth,format) <- [(3,0,0x100),(5,2,0x200)]]
      ++ [let bindings = constructorMethods initializedEngine IconMethods
           in make pub ("initializer-actual-icon-backend-"++show mode) [0x118360c,constructorReference,mode,32,output,0,0,0,0]
              (initializerInput++iconConstructorResource++[word (constructorReference+0x68) 0x700])
              (methodWords bindings++[word output initializedEngine,word (initializedEngine+0xff4) mode
              ,word (initializedEngine+0x1030) 0,word (initializedEngine+0x1034) 0,word (initializedEngine+0x106c) format
              ,word (initializedEngine+0x1088) initializedEngine,word (initializedEngine+0x1090) 0x3bc000]) (Just 0) bindings
         | (mode,format) <- [(6,0x100),(7,0x200)]]
    0x11818c8 ->
      let rasterMethods = constructorMethods objectRasterEngine RasterMethods
          outlineMethods = constructorMethods objectOutlineEngine OutlineMethods
          builtinMethods = constructorMethods objectBuiltinEngine BuiltinMethods
       in [make pub "actual-object-constructor-opens-genuine-fdl-and-builds-raster-engine" [constructorReference,8,0,0] (constructorInput 0)
            (methodWords rasterMethods++
            [word objectConstructed objectRasterEngine,word (objectConstructed+4) 0x1f08,word (objectConstructed+12) 4
            ,wordsAt (objectConstructed+20) [27,27],word (objectConstructed+0x40) 1,word (objectConstructed+0x5c) 0x200e
            ,word (objectConstructed+0x80) (objectConstructed+0x11b0),word (objectConstructed+0x88) (objectConstructed+0x11c4)
            ,word (objectConstructed+0x1168) (objectConstructed+0x1ee0),wordsAt (objectConstructed+0x1178) [heap+0x2048,0x87dd4]
            ,word (objectRasterEngine+20) objectConstructed,wordsAt (objectRasterEngine+0x103c) [27,27],word (objectRasterEngine+0x1070) 1
            ,word (constructorReference+0x6c) 1,word (constructorReference+0x64) 0x3bc000]) (Just objectConstructed) rasterMethods
          ,make pub "actual-object-constructor-builds-outline-program-with-real-directory" [constructorReference,1,0,0] (constructorInput 0)
            (methodWords outlineMethods++
            [word objectConstructed objectOutlineEngine,word (objectConstructed+12) 1,wordsAt (objectConstructed+20) [27,27]
            ,word (objectConstructed+0x60) 2,word (objectConstructed+0x116c) (objectConstructed+0x1ee0)
            ,word (objectOutlineEngine+0x108c) (heap+0x67458),word (heap+0x67458+8) 0x3bc000
            ,word (heap+0x67458+0x14) 0xf726,word (constructorReference+0x6c) 1]) (Just objectConstructed) outlineMethods
          ,make pub "actual-object-constructor-parses-authored-bdf-backend" [constructorReference,2,0,0]
            (constructorInput 1++[bdfConstructorResource])
            (methodWords builtinMethods++
            [word objectConstructed objectBuiltinEngine,word (objectConstructed+12) 2
            ,word (objectConstructed+0x5c) 0x200e,word (objectConstructed+0x68) 4
            ,half (objectBuiltinEngine+0x1088+12) 5,half (objectBuiltinEngine+0x1088+14) 7]) (Just objectConstructed) builtinMethods
          ,make pub "object-constructor-rejects-incompatible-render-request-before-allocation" [constructorReference,3,0,0]
            (constructorInput 0) [word 0x1667040 0,word (constructorReference+0x6c) 0] (Just 0) []
          ,make pub "object-constructor-open-failure-retains-count-increment" [constructorReference,8,0,0]
            (constructorInput 0++[word (constructorReference+0x6c) 1,word (constructorReference+0x64) 0x3bc000])
            [word (constructorReference+0x6c) 2,word (objectConstructed+0x1178) 0,word (objectConstructed+0x117c) 0] (Just 0) []
          ,make pub "object-constructor-backend-error-closes-resource-and-releases-pixel-storage" [constructorReference,8,0,0]
            (constructorInput 0++[half 0x3bc036 2])
            (methodWords rasterMethods++[wordsAt (constructorReference+0x64) [0,0,0]
            ,word (objectConstructed+0x1178) 0,word (objectConstructed+0x117c) 0]) (Just 0) rasterMethods
          ,make pub "object-constructor-fallback-open-error-closes-primary-only" [constructorReference,8,constructorAuxiliary,0]
            (constructorInput 0++constructorAuxiliaryInput 0++[word (constructorAuxiliary+0x6c) 1,word (constructorAuxiliary+0x64) 0x3bc000])
            [wordsAt (constructorReference+0x64) [0,0,0],word (constructorAuxiliary+0x6c) 2
            ,word (objectConstructed+0x1178) 0] (Just 0) []
          ,make pub "object-constructor-icon-open-error-retains-its-count-increment" [constructorReference,8,0,constructorAuxiliary]
            (constructorInput 0++constructorAuxiliaryInput 3++[word (constructorAuxiliary+0x6c) 1,word (constructorAuxiliary+0x64) 0x3bc000])
            [wordsAt (constructorReference+0x64) [0,0,0],word (constructorAuxiliary+0x6c) 2
            ,word (objectConstructed+0x1178) 0] (Just 0) []
          ,make pub "object-constructor-null-reference-does-not-touch-heap" [0,8,0,0] (base++[word 0x1667040 0])
            [word 0x1667040 0] (Just 0) []]
    0x11811a0 ->
      [make pub "reset-raster-options-routes-through-real-setters" [object] (fontRasterInitial fixture++[word (object+12) 4])
        [wordsAt (object+20) [27,27],word (object+0x40) 1,word (object+0x5c) 0x200e,word (object+0x68) 4
        ,wordsAt (engine+0x103c) [27,27],word (engine+0x1070) 1] Nothing (fontRasterCodePointers fixture)
      ,make pub "reset-builtin-only-changes-direction-and-alignment" [object]
        (fontRasterInitial fixture++[word (object+12) 2,word (engine+0xff4) 2,wordsAt (object+20) [19,23]])
        [wordsAt (object+20) [19,23],word (object+0x5c) 0x200e,word (object+0x68) 4] Nothing []]
    0x11805f0 ->
      [make pub ("rendering-mode-"++show kind++"-"++show request) [kind,request] (base++[marker]) [marker] (Just result) []
      | (kind,request,result) <- [(0,1,1),(0,2,3),(0,8,4),(0,4,5),(1,2,2),(1,8,8),(3,2,8),(0,3,8)]]
    0x1180290 ->
      [make pub "open-reference-binds-original-embedded-icon-resource" [constructorReference,0xffffffff,0,0]
        (base++constructorRegistry 0) [wordsAt (constructorReference+0x64) [0x3bc000,0x7cfd1,1]] (Just 1) []
      ,make pub "open-existing-reference-increments-but-does-not-reopen" [constructorReference,0xffffffff,0,0]
        (base++openedConstructorReference++[word (constructorReference+0x6c) 2])
        [wordsAt (constructorReference+0x64) [0x3bc000,0x7cfd1,3]] (Just 0) []
      ,make pub "open-unknown-font-does-not-fabricate-resource" [constructorReference,0xffffffff,0,0]
        (base++constructorRegistry 0++[ascii (constructorReference+16) "UNKNOWN.FDL\0"])
        [wordsAt (constructorReference+0x64) [0,0,0]] (Just 0) []]
    0x11803fc ->
      [make pub "close-last-reference-clears-resource" [constructorReference,0xffffffff] (base++openedConstructorReference)
        [wordsAt (constructorReference+0x64) [0,0,0]] Nothing []
      ,make pub "close-shared-reference-retains-resource" [constructorReference,0xffffffff]
        (base++openedConstructorReference++[word (constructorReference+0x6c) 2])
        [wordsAt (constructorReference+0x64) [0x3bc000,0x7cfd1,1]] Nothing []
      ,make pub "close-zero-count-wraps-without-invented-guard" [constructorReference,0xffffffff]
        (base++openedConstructorReference++[word (constructorReference+0x6c) 0])
        [wordsAt (constructorReference+0x64) [0x3bc000,0x7cfd1,0xffffffff]] Nothing []]
    0x117f850 ->
      [make pub "prefix-match-does-not-require-whole-string-equality" [output+0x100,output+0x120,4]
        (base++[ascii (output+0x100) "ABCDX\0",ascii (output+0x120) "ABCDY\0"]) [] (Just 0) []
      ,make pub "prefix-mismatch-returns-pointer-difference-not-character-difference" [output+0x100,output+0x120,5]
        (base++[ascii (output+0x100) "ABCDX\0",ascii (output+0x120) "ABCDY\0"]) [] (Just 0xffffffe0) []
      ,make pub "prefix-empty-string-rejects-even-zero-request" [output+0x100,output+0x120,0]
        (base++[ascii (output+0x100) "\0",ascii (output+0x120) "Y\0"]) [] (Just 0xffffffe0) []]
    0x1185b5c ->
      [make pub "reference-index-uses-original-74-byte-stride" [constructorManager,2]
        (base++constructorRegistry 0) [] (Just (constructorReference+0xe8)) []
      ,make pub "reference-index-retains-wrapping-arithmetic" [constructorManager,0xffffffff]
        (base++constructorRegistry 0) [] (Just (constructorReference-0x74)) []]
    0x118cc68 ->
      [make pub "parse-real-dfshaper-directory-into-engine" [engine]
        (base++[zero engine 0x600,word (engine+0x70) 0x118360c])
        [word (engine+0x74) 16,word (engine+0x78) 16,half (engine+0x500) 1,half (engine+0x502) 1,word (engine+0x504) (engine+0x78)] (Just 0) []
      ,make pub "directory-signature-error-retains-prefix-read" [engine]
        (base++[zero engine 0x600,word (engine+0x70) output,ascii output "X"])
        [word (engine+0x74) 1,raw (engine+0x4f8) [88],word (engine+0x504) 0] (Just 0xfffffffe) []]
    0x118d488 ->
      [make pub "install-actual-owned-icon-fallback-methods" [engine]
        (base++[zero engine 0x100,word (engine+0x24) constructorReference,word (engine+0x28) 1])
        (methodWords fallbackMethods) Nothing fallbackMethods
      ,make pub "missing-icon-size-preserves-method-poison" [engine]
        (base++[raw engine (replicate 0x100 0xa5),word (engine+0x24) constructorReference,word (engine+0x28) 0])
        [word (engine+0x2c) 0xa5a5a5a5,word (engine+0x5c) 0xa5a5a5a5] Nothing []]
    address | address `elem` [0x1195210,0x119f08c] ->
      [make pub ("raster-storage-source-mode-"++show mode) [width,height,mode] (base++[marker]) [marker] (Just result) []
      | (width,height,mode,result) <- [(32,32,0,0x83ec),(32,32,1,0xdd0c),(32,32,2,0x1996c),(1000,1000,1,0x1466c),(1000,1000,9,0x281ec)]]
    address | address `elem` [0x1194e30,0x1194e48] -> [make pub "source-constant-raster-lifecycle-result" [] (base++[marker]) [marker] (Just 0) []]
    _ -> error "unmapped FontFDL publication"
