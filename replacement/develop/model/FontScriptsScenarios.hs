module FontScriptsScenarios (fontScriptsPublications, fontScriptsScenarios) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (testBit)
import qualified FontFdlScenarios as Fdl
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), word, wordsAt)

publication :: Word32 -> String -> StagePublication
publication entry suffix = StagePublication (Address entry) (StageSymbol ("xm1_pipeline_image_font_" ++ suffix))

fontScriptsPublications :: [StagePublication]
fontScriptsPublications = [publication address suffix | (address,suffix) <-
  [(0x118cff4,"dispatch_script"),(0x119017c,"detect_mode"),(0x11902ac,"detect_script_run"),(0x118ce2c,"select_row")
  ,(0x118af98,"shape_basic"),(0x118ad24,"position_forward"),(0x1187c08,"shape_arabic"),(0x1187494,"position_arabic")
  ,(0x11896cc,"shape_thai"),(0x1189e68,"position_thai"),(0x118bda0,"shape_hebrew"),(0x118b6e4,"position_hebrew")
  ,(0x118caa8,"pack_count"),(0x118cad0,"unpack_count"),(0x118cbe8,"next_u16"),(0x118cfc4,"flags_for_mode")
  ,(0x118fba4,"classify_unicode"),(0x118fadc,"private_character_class"),(0x1180428,"glyph_cell_width")
  ,(0x118d760,"position_metric_span"),(0x118d6dc,"store_main_run"),(0x118d5ec,"compose_private_run")
  ,(0x118de64,"mirror_run"),(0x118ff30,"mirror_character"),(0x118cb08,"find_glyph")
  ,(0x11874d0,"read_run"),(0x118768c,"find_ligature"),(0x1187934,"compose_arabic_character")
  ,(0x1188cd8,"thai_mark_offsets"),(0x1188fc8,"thai_split_character"),(0x1189190,"measure_thai_lines"),(0x118baec,"prepare_hebrew_characters")]]

engine, stream, directory, output, text, request, glyphs, runAddress, workspaceHeader, workspace :: Word32
engine = 0x9a0000
stream = 0x9b0000
directory = 0x9a2000
output = 0x9a2100
text = 0x9a2200
request = 0x9a2300
glyphs = 0x9c0000
runAddress = 0x9a2400
workspaceHeader = 0x9a2500
workspace = 0x9a2600

half :: Word32 -> Word32 -> Patch
half address value = Patch (Address address) (B.take 2 (case word address value of Patch _ bytes -> bytes))

halves :: Word32 -> [Word32] -> Patch
halves address values = Patch (Address address) (B.concat [bytes | value <- values, let Patch _ bytes = half 0 value])

resource :: B.ByteString -> Word32 -> Word32 -> Patch
resource image address size
  | B.length image < fromIntegral (address - 0x1100000 + size) = error "overlay44 script resource is incomplete"
  | otherwise = Patch (Address address) (B.take (fromIntegral size) (B.drop (fromIntegral (address - 0x1100000)) image))

fontScriptsScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
fontScriptsScenarios image _resolve = classCases ++ privateCases ++ cellCases ++ packingCases ++ unpackingCases ++ modeFlagCases ++ modeCases ++ runCases ++ streamCases ++ selectionCases ++ mirrorCases ++ searchCases ++ compositionCases ++ dictionaryCases ++ thaiOffsetCases ++ thaiSplitCases ++ textBackendCases image ++ rectangleCases
 where
  make name entry suffix arguments initial final returned = StageScenario (StageCaseName name) (publication entry suffix) arguments initial final returned [] []
  zeroState = [Patch (Address engine) (B.replicate 0x1100 0),Patch (Address output) (B.replicate 32 0xa5)]
  classCases =
    [make ("font-unicode-class-" ++ show code) 0x118fba4 "classify_unicode" [output,code] zeroState [wordsAt output [category,kind]] (Just output)
    | (code,category,kind) <- [(0,2,5),(31,2,5),(32,1,4),(33,3,14),(35,3,16),(40,3,12),(41,3,13)
      ,(48,3,7),(65,3,8),(97,3,9),(126,3,14),(127,2,5),(128,2,5),(159,2,5),(160,3,11)
      ,(0x300,3,3),(0x36f,3,3),(0x590,9,11),(0x5d0,9,11),(0x610,8,1),(0x627,8,11)
      ,(0x640,8,10),(0x64b,8,1),(0x6de,8,2),(0x900,14,11),(0xb80,15,11),(0xe01,4,11)
      ,(0x2000,2,6),(0x21ff,2,6),(0x3000,1,4),(0xe000,8,11),(0xe100,0,0),(0xe128,3,12)
      ,(0xe129,3,13),(0xe200,3,11),(0xe900,14,11),(0xee00,15,11),(0xfb1d,9,11),(0xfeff,0,0),(0x10041,3,8)]]
  privateCases =
    [make ("font-private-class-" ++ show code) 0x118fadc "private_character_class" [output,code] zeroState [wordsAt output [category,kind]] (Just output)
    | (code,category,kind) <- [(0x41,0,0),(0xe100,0,0),(0xe128,3,12),(0xe129,3,13),(0xe15b,3,12),(0xe15d,3,13),(0xe17b,3,12),(0xe17d,3,13),(0xe200,3,11),(0xf72f,3,11),(0xf730,0,0)]]
  cellCases =
    [make ("font-private-cell-" ++ show code) 0x1180428 "glyph_cell_width" [code] [] [] (Just result)
    | (code,result) <- [(0x41,0),(0xe11f,0),(0xe120,4),(0xe128,2),(0xe129,3),(0xe1ff,4),(0xf72f,4),(0xf730,0)]]
  packingCases =
    [make ("font-count-pack-" ++ show (count,groups)) 0x118caa8 "pack_count" [count,groups] [] [] (Just packed)
    | count <- [0,1,255,256,65535],groups <- [0,1,2,255,256,65535]
    ,let packed = fromInteger ((toInteger count + if groups > 1 then 256 * toInteger groups else 0) `mod` 65536)]
  unpackingCases =
    [make ("font-count-unpack-" ++ show packed) 0x118cad0 "unpack_count" [packed,output,output+2] zeroState [halves output [count,groups]] Nothing
    | (packed,count,groups) <- [(0,0,1),(1,1,1),(255,255,1),(256,256,1),(257,1,1),(511,255,1),(512,0,2),(65535,255,255)]]
  modeFlagCases =
    [make ("font-mode-direction-" ++ show mode) 0x118cfc4 "flags_for_mode" [mode] [] [] (Just direction)
    | (mode,direction) <- [(0,0x200e),(1,0x200e),(2,0x200f),(3,0x200e),(4,0x200e),(5,0x200e),(6,0x200f),(7,0x200e),(0xffffffff,0x200e)]]
  modeCases =
    [make ("font-detect-mode-" ++ label) 0x119017c "detect_mode" [text,fromIntegral (length codes),output,output+4]
      (zeroState ++ [halves text codes]) [wordsAt output [mode,language]] (Just direction)
    | (label,codes,mode,language,direction) <-
      [("empty",[],0,0,0),("neutral",[32,40,41,0xfeff],0,0,0),("latin",[32,40,65],1,1,0x200e)
      ,("arabic",[32,0x627],2,5,0x200f),("thai",[0xe01],3,6,0x200e),("devanagari",[0x915],4,7,0x200e)
      ,("tamil",[0xb95],5,8,0x200e),("hebrew",[0x5d0],6,9,0x200f)]] ++
    [make "font-detect-mode-null-outputs" 0x119017c "detect_mode" [text,1,0,0] [halves text [0x5d0]] [] (Just 0x200f)
    ,make "font-detect-mode-negative-length" 0x119017c "detect_mode" [0,0xffffffff,output,output+4] zeroState [wordsAt output [0,0]] (Just 0)]
  runCases =
    [make ("font-detect-run-" ++ label) 0x11902ac "detect_script_run" [text,fromIntegral (length codes),start,output,base,output+4,output+8,output+12]
      (zeroState ++ [halves text codes]) [half output end,wordsAt (output+4) [mode,language],half (output+12) direction] (Just (end-start))
    | (label,codes,start,base,end,mode,language,direction) <-
      [("empty-ltr",[],0,0x200e,0,1,1,0x200e),("empty-rtl",[],0,0x200f,0,2,5,0x200f)
      ,("latin",[65,66,32],0,0x200e,3,1,1,0x200e),("arabic-boundary",[0x627,0x628,32],0,0x200e,2,2,5,0x200f)
      ,("arabic-base",[0x627,0x628,32],0,0x200f,3,2,5,0x200f),("script-change",[65,0x627],0,0x200e,1,1,1,0x200e)
      ,("leading-space",[32,0x627],0,0x200e,1,1,1,0x200e),("nested-close",[0x627,40,0x628,41,65],0,0x200e,4,2,5,0x200f)
      ,("absolute-fifty-cap",replicate 60 65,49,0x200e,50,1,1,0x200e),("at-fifty",replicate 60 65,50,0x200e,50,1,1,0x200e)]]
  streamCases =
    [make "font-stream-unaligned-little-endian" 0x118cbe8 "next_u16" [engine]
      (zeroState ++ [word (engine+0x70) stream,word (engine+0x74) 3,Patch (Address stream) (B.pack [0xaa,0xbb,0xcc,0x34,0x92,0xdd])])
      [word (engine+0x74) 5] (Just 0x9234)]
  selectionInitial = zeroState ++ [word (engine+0x70) stream,half (engine+0x502) 2,word (engine+0x504) directory
    ,wordsAt directory [0x20,0x40],halves (stream+0x20) [1,1,1],word (stream+0x26) 0x80
    ,halves (stream+0x40) [2,5,2],word (stream+0x46) 0x100,word (stream+0x4a) 0x180]
  selectionCases =
    [make "font-select-second-row" 0x118ce2c "select_row" [engine,2,5,output] selectionInitial
      [half output 1,halves (engine+0x508) [2,5,2],wordsAt (engine+0x478) [0x100,0x180],word (engine+0x510) (engine+0x478),word (engine+0x74) 0x4e] (Just 0)
    ,make "font-select-no-pair" 0x118ce2c "select_row" [engine,2,9,output] selectionInitial [word (engine+0x74) 0x44] (Just 0xffffffff)
    ,make "font-select-no-script" 0x118ce2c "select_row" [engine,6,9,output] selectionInitial [word (engine+0x74) 0x42] (Just 0xffffffff)
    ,make "font-select-null-stream" 0x118ce2c "select_row" [engine,1,1,output] zeroState [] (Just 0xffffffff)
    ,make "font-select-empty-directory" 0x118ce2c "select_row" [engine,1,1,output] (zeroState ++ [word (engine+0x70) stream]) [] (Just 0xffffffff)]
  mirrorResources = [resource image 0x11805e0 16]
  mirrorCases =
    [make ("font-character-mirror-" ++ show code) 0x118ff30 "mirror_character" [code] mirrorResources [] (Just mirrored)
    | (code,mirrored) <- [(40,41),(41,40),(91,93),(93,91),(123,125),(125,123),(65,65),(0xe128,0xe129),(0xe129,0xe128)]] ++
    [make "font-mirror-mapped-glyphs" 0x118de64 "mirror_run" [engine,0,2]
      (zeroState ++ mirrorResources ++ [half (engine+0x616) 3,half (engine+0x618) 40,half (engine+0x626) 4,half (engine+0x628) 65,halves (engine+0x5ac) [40,65]])
      [half (engine+0x618) 41,halves (engine+0x5ac) [41,65]] Nothing]
  searchCases =
    [make ("font-glyph-search-" ++ show code) 0x118cb08 "find_glyph" [4,stream,code] [halves stream [0,32,65,0x627,0xe01]] [] (Just result)
    | (code,result) <- [(0,0xffffffff),(32,1),(65,2),(0x627,3),(0xe01,4),(0xffff,0xffffffff)]] ++
    [make "font-glyph-search-empty" 0x118cb08 "find_glyph" [0,0,65] [] [] (Just 0xffffffff)]
  compositionCases =
    [make ("font-arabic-compose-" ++ show (first,second)) 0x1187934 "compose_arabic_character" [engine,0]
      (zeroState ++ [word (engine+0x10) request,word (request+16) text,halves text [first,second,0]])
      ([half (engine+0x540) composed,half (engine+0x5a4) 1] ++
       [halves (engine+0x610+16*index) [0,composed,count-index] | index <- [0..count-1]]) (Just count)
    | (first,second,composed,count) <- [(0x627,0x653,0x622,2),(0x627,0x654,0x623,2),(0x627,0x655,0x625,2)
      ,(0x648,0x654,0x624,2),(0x64a,0x674,0x678,2),(0x6c1,0x654,0x6c2,2),(0x6d2,0x654,0x6d3,2),(0x627,0,0x627,1)]]
  dictionaryCases =
    [make "font-script-dictionary-run" 0x11874d0 "read_run" [engine,0x20,0,output]
      (zeroState ++ [word (engine+0x70) stream,halves (stream+0x20) [1,0x10],halves (stream+0x30) [0xfefb,3,3,0x627,0x628],word (output+12) text])
      [half (output+2) 0xfefb,word (output+4) 3,half (output+8) 3,halves text [0x627,0x628]] (Just 0)]
  thaiResources = [resource image 0x118a98c 224]
  thaiOffsetCases =
    [make ("font-thai-offset-" ++ label) 0x1188cd8 "thai_mark_offsets" [output,request,text,0,last,index,decomposed,followed]
      (zeroState ++ thaiResources ++ [halves request [7,0,0,0,4],halves text codes]) [wordsAt output [x,y]] Nothing
    | (label,codes,last,index,decomposed,followed,x,y) <-
      [("first-mark",[0xe48],0,0,0,0,0,0),("upper-vowel",[0xe1b,0xe34],1,1,0,0,0xfffffffc,0)
      ,("tone",[0xe1b,0xe48],1,1,0,0,0xfffffffc,7),("followed-tone",[0xe1b,0xe48,0xe4d],2,1,0,1,0xfffffffc,0)
      ,("nikhahit",[0xe01,0xe4d],1,1,0,0,0,7),("second-mark",[0xe1b,0xe34,0xe48],2,2,0,0,0xfffffffc,0)
      ,("below-then-tone",[0xe01,0xe38,0xe48],2,2,0,0,0,7),("decomposed-sara-am",[0xe1b,0xe48,0xe4d],2,2,1,0,0xfffffffc,7)
      ,("uncomposed-nikhahit",[0xe1b,0xe48,0xe4d],2,2,0,0,0xfffffffc,0)]] ++
    [make "font-thai-unused-null-metrics" 0x1188cd8 "thai_mark_offsets" [output,0,text,0,1,1,0,0]
      (zeroState ++ thaiResources ++ [halves text [0xe01,65]]) [wordsAt output [0,0]] Nothing]
  thaiSplitCases =
    [make ("font-thai-split-" ++ label) 0x1188fc8 "thai_split_character" [engine,index,output]
      (zeroState ++ thaiResources ++ [word (engine+0x10) request,word (request+16) text,halves text codes])
      ([halves output mapping,halves (engine+0x540) normalized,halves (engine+0x5a6) normalized] ++
       if split then [half (engine+0x5a4) 1,half (engine+0x60a) 1] else []) Nothing
    | (label,codes,index,mapping,normalized,split) <-
      [("base",[0xe01],0,[0,0xe01,0,0xe01,1,0,0xe01,1],[0xe01],False)
      ,("standalone-sara-am",[0xe33],0,[0,0xe33,0,0xe33,1,0,0xe33,1],[0xe33],False)
      ,("sara-am-after-base",[0xe01,0xe33],1,[1,0xe33,1,0xe32,513,1,0xe32,513],[0xe4d,0xe32],True)
      ,("sara-am-after-tone",[0xe01,0xe48,0xe33],2,[2,0xe33,1,0xe32,513,1,0xe32,513],[0xe4d,0xe32],True)
      ,("sara-am-after-latin",[65,0xe33],1,[1,0xe33,0,0xe33,1,0,0xe33,1],[0xe33],False)]]
  rectangleCases =
    [StageScenario (StageCaseName "font-basic-real-fdl-rectangle-render") (publication 0x118af98 "shape_basic") [e,r]
      (Fdl.fontRectangleRasterInitial fixture ++
       [Patch (Address s) (B.replicate 0x1500 0),word (e+0x53c) s,word (e+0x538) 2,half (e+0x526) 1
       ,halves (e+0x616) [0,65,1],half (e+0x5a6) 65,half (s+4) 65
       ,wordsAt r [0x10000,1,1,16,32,0,0,0,0],wordsAt (s+0xd00) [32,32,32,1024,b]
       ,Patch (Address b) (B.replicate 1024 0)])
      [Patch (Address b) (B.replicate 1024 15)] (Just 0) (Fdl.fontRasterCodePointers fixture) []]
   where
    fixture = Fdl.fontFixture
    Address e = Fdl.fontEngineAddress fixture
    Address s = Fdl.fontSpanAddress fixture
    Address r = Fdl.fontOutputAddress fixture
    Address b = Fdl.fontBitmapAddress fixture

textBackendCases :: B.ByteString -> [StageScenario]
textBackendCases image = measuredCases ++ dispatchCases ++ placementCases ++ renderCases ++ outlineErrors ++ dictionaryShapeCases ++ ligatureCases ++ helperCases
 where
  state = 0x9a3000
  storage = 0x9a4000
  auxiliary = 0x9a9000
  scriptData = 0x9d0000
  bitmap = 0x9e0000
  codepoints = [40,41,65,66,0x591,0x5be,0x5c0,0x5c6,0x5d0,0x622,0x624,0x626,0x627,0x628,0xe01,0xe0d,0xe10,0xe32,0xe33,0xe34,0xe38,0xe48,0xe4d,0x25cc,0xe128,0xe129,0xe14d]
  header = C.pack ("STARTFONT 2.2\r\nFONT script-fixture\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 5 7 0 0\r\nSTARTPROPERTIES 2\r\nTHAI_TOP 3\r\nTHAI_BASE 2\r\nENDPROPERTIES\r\nCHARS " ++ show (length codepoints) ++ "\r\n")
  glyph code = C.pack ("STARTCHAR fixture\r\nENCODING " ++ show code ++ "\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 0 0\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\n")
  chunks = map glyph codepoints
  offsets = init (scanl (+) (fromIntegral (B.length header)) (map (fromIntegral . B.length) chunks))
  fontData = B.concat (header : chunks ++ [C.pack "ENDFONT\r\n"])
  pages = [(0,0x100),(5,0x200),(6,0x300),(14,0x400),(37,0x500),(225,0x600)]
  table = storage+0x300
  pageFor code = case lookup (code `div` 256) pages of Just page -> page; Nothing -> error "fixture glyph has no BDF directory page"
  initial =
    [Patch (Address engine) (B.replicate 0x1400 0),Patch (Address glyphs) (B.replicate 0x1600 0)
    ,Patch (Address state) (B.replicate 0x80 0),Patch (Address storage) (B.replicate 0x3000 0)
    ,Patch (Address stream) fontData,word state engine,word (state+0x30) table,word (state+0x34) 0x2000
    ,word (state+0x38) stream,word (state+0x3c) storage,word (state+0x40) (storage+0x200)
    ,halves (state+12) [5,7,0,0],half (state+26) 4
    ,word (engine+0x68) state,word (engine+0xff4) 2,word (engine+0x53c) glyphs
    ,word (engine+0x10) request,word (request+12) 0,word (request+16) text
    ,word (engine+0x18) auxiliary,word (auxiliary+0x70) workspaceHeader
    ,word (workspaceHeader+12) 0x800,word (workspaceHeader+16) workspace
    ,Patch (Address workspace) (B.replicate 0x800 0)
    ,wordsAt (glyphs+0xd00) [64,16,8,128,bitmap],word (engine+0x106c) 0x100
    ,Patch (Address bitmap) (B.replicate 128 0x20)
    ,resource image 0x11887d8 1280,resource image 0x118a98c 548,resource image 0x11805e0 16] ++
    [word (table+page*4) offset | (page,offset) <- pages] ++
    [word (table+(pageFor code+code `mod` 256)*4) offset | (code,offset) <- zip codepoints offsets]
  pointers = [(Address (engine+slot),publication address suffix) | (slot,address,suffix) <-
    [(0x1000,0x1193364,"draw_text_bitmap"),(0x1004,0x1192c70,"builtin_properties")
    ,(0x1008,0x1193830,"builtin_glyph_width"),(0x100c,0x1190be8,"builtin_height")
    ,(0x1010,0x1190c00,"builtin_width"),(0x1014,0x1190c18,"builtin_secondary_extent")
    ,(0x1018,0x11939f8,"builtin_right_overhang"),(0x101c,0x1190c44,"builtin_top_overhang")
    ,(0x1020,0x1190c5c,"builtin_stroke_padding")]]
  make name entry suffix arguments extra final returned = StageScenario (StageCaseName name) (publication entry suffix) arguments (initial++extra) final returned pointers []
  basicMap index code = [index,code,index,code,1,index,code,1]
  metric index glyphIndex advance pen direction =
    [word (engine+0x92c+index*28) index,half (engine+0x930+index*28) glyphIndex
    ,word (engine+0x934+index*28) advance,word (engine+0x938+index*28) 0
    ,half (engine+0x93c+index*28) direction,word (engine+0x940+index*28) pen,word (engine+0x944+index*28) 0]
  runResult count script language width = wordsAt (engine+0xea8) [count*65536,script,language,width,7]
  measureInput codes script language =
    [halves text (codes++[0]),halves (engine+0x524) [0,fromIntegral (length codes)],wordsAt (engine+0x52c) [script,language,0,1]]
  measuredCases =
    [make ("font-empty-measure-" ++ suffix) entry suffix [engine,runAddress] (measureInput [] script language)
      [word (engine+0x534) 0,runResult 0 script language 0] (Just 0)
    | (entry,suffix,script,language) <- [(0x118af98,"shape_basic",1,1),(0x1187c08,"shape_arabic",2,5),(0x11896cc,"shape_thai",3,6),(0x118bda0,"shape_hebrew",6,9)]] ++
    [make "font-basic-bdf-measure" 0x118af98 "shape_basic" [engine,runAddress] (measureInput [65,66] 1 1)
      ([halves (engine+0x540) [65,66],half (engine+0x5a4) 2,halves (engine+0x5a6) [65,66],half (engine+0x60a) 2
       ,halves (engine+0x60c) (basicMap 0 65 ++ basicMap 1 66),word (engine+0x534) 8,runResult 2 1 1 8] ++ metric 0 0 4 0 0x200e ++ metric 1 1 4 4 0x200e) (Just 0)
    ,make "font-basic-missing-glyph-width" 0x118af98 "shape_basic" [engine,runAddress] (measureInput [0xffff] 1 1)
      ([halves (engine+0x60c) (basicMap 0 0xffff),word (engine+0x534) 0,runResult 1 1 1 0] ++ metric 0 0 0 0 0x200e) (Just 0)
    ,make "font-arabic-bdf-compose-pair" 0x1187c08 "shape_arabic" [engine,runAddress] (measureInput [0x627,0x653,0x628] 2 5)
      ([halves (engine+0x540) [0x622,0x628],half (engine+0x5a4) 2,halves (engine+0x5a6) [0x622,0x628],half (engine+0x60a) 2
       ,halves (engine+0x60c) [0,0x627,0,0x622,2,0,0x622,2,1,0x653,0,0x622,1,0,0x622,1,2,0x628,1,0x628,1,1,0x628,1]
       ,word (engine+0x534) 8,runResult 3 2 5 8] ++ metric 0 0 4 0 0x200f ++ metric 1 0 4 4 0x200f ++ metric 2 1 4 4 0x200f) (Just 0)
    ,make "font-thai-bdf-combining-advances" 0x11896cc "shape_thai" [engine,runAddress] (measureInput [0xe01,0xe34,0xe48] 3 6)
      ([halves (engine+0x540) [0xe01,0xe34,0xe48],half (engine+0x5a4) 3,halves (engine+0x5a6) [0xe01,0xe34,0xe48],half (engine+0x60a) 3
       ,halves (engine+0x60c) (concat [basicMap index code | (index,code) <- zip [0..] [0xe01,0xe34,0xe48]])
       ,word (engine+0x534) 4,runResult 3 3 6 4] ++ metric 0 0 4 0 0x200e ++ metric 1 1 4 4 0x200e ++ metric 2 2 4 4 0x200e) (Just 0)
    ,make "font-thai-bdf-sara-am-expansion" 0x11896cc "shape_thai" [engine,runAddress] (measureInput [0xe01,0xe33] 3 6)
      ([halves (engine+0x540) [0xe01,0xe4d,0xe32],half (engine+0x5a4) 3,halves (engine+0x5a6) [0xe01,0xe4d,0xe32],half (engine+0x60a) 3
       ,halves (engine+0x60c) (basicMap 0 0xe01 ++ [1,0xe33,2,0xe32,513,2,0xe32,513])
       ,word (engine+0x534) 8,runResult 2 3 6 8] ++ metric 0 0 4 0 0x200e ++ metric 1 2 4 4 0x200e) (Just 0)
    ,make "font-hebrew-bdf-dotted-circle-and-mirror" 0x118bda0 "shape_hebrew" [engine,runAddress] (measureInput [0x5d0,0x591,0x5be,0x5c0,0x5c6,40] 6 9)
      ([halves (engine+0x540) [0x5d0,0x25cc,0x5be,0x5c0,0x5c6,40],half (engine+0x5a4) 6
       ,halves (engine+0x5a6) [0x5d0,0x25cc,0x5be,0x5c0,0x5c6,41],half (engine+0x60a) 6
       ,halves (engine+0x60c) (concat [ [index,original,index,normalized,1,index,rendered,1]
         | (index,original,normalized,rendered) <- [(0,0x5d0,0x5d0,0x5d0),(1,0x591,0x25cc,0x25cc),(2,0x5be,0x5be,0x5be),(3,0x5c0,0x5c0,0x5c0),(4,0x5c6,0x5c6,0x5c6),(5,40,40,41)]])
       ,word (engine+0x534) 24,runResult 6 6 9 24] ++ concat [metric index index 4 (index*4) 0x200f | index <- [0..5]]) (Just 0)]
  dispatchCases =
    [make "font-dispatch-original-basic-default" 0x118cff4 "dispatch_script" [engine,99,99,0,runAddress] (measureInput [65] 99 99)
      [halves (engine+0x60c) (basicMap 0 65),word (engine+0x534) 4,runResult 1 99 99 4] (Just 0)
    ,StageScenario (StageCaseName "font-dispatch-unsupported-phase") (publication 0x118cff4 "dispatch_script") [0,2,5,2,0] [] [] (Just 0) [] []]
  placement original packed pen x advance = [original,packed,pen,0,x,0,advance,0,0,0,0,0,0,0,0]
  positionInput codes width direction x =
    [halves (engine+0x5a6) codes,halves (engine+0x60c) (concat [basicMap index code | (index,code) <- zip [0..] codes])
    ,halves (engine+0x524) [0,fromIntegral (length codes)],half (glyphs+0xc20) direction
    ,wordsAt runAddress [fromIntegral (length codes)*65536,1,1,width,7,x,0,0,0]]
  placementCases =
    [make ("font-forward-position-" ++ show direction) 0x118ad24 "position_forward" [engine,runAddress] (positionInput [65,66] 8 direction 20)
      [halves (glyphs+4) [65,66],wordsAt (glyphs+0x68) (placement 0 65536 (start+4) start 4 ++ placement 1 65537 (start+8) (start+4) 4),wordsAt (runAddress+28) [start+8,0]] (Just 0)
    | direction <- [0x200e,0x200f],let start = if direction == 0x200f then 12 else 20] ++
    [make ("font-reverse-position-" ++ show (entry,direction)) entry suffix [engine,runAddress] (positionInput [65,66] 8 direction 20)
      [halves (glyphs+4) [66,65],wordsAt (glyphs+0x68) (placement 1 65537 start start 4 ++ placement 0 65536 (start+4) (start+4) 4),wordsAt (runAddress+28) [start+8,0]] (Just 0)
    | (entry,suffix) <- [(0x1187494,"position_arabic"),(0x118d760,"position_metric_span"),(0x118b6e4,"position_hebrew")],direction <- [0x200e,0x200f],let start = if direction == 0x200f then 12 else 20] ++
    [make "font-hebrew-position-zero-advance-mark" 0x118b6e4 "position_hebrew" [engine,runAddress] (positionInput [0x5d0,0x591] 4 0x200f 20)
      [halves (glyphs+4) [0x591,0x5d0],wordsAt (glyphs+0x68) (placement 1 65537 16 16 4 ++ placement 0 65536 16 16 4),wordsAt (runAddress+28) [20,0]] (Just 0)
    ,make "font-forward-position-word-overflow" 0x118ad24 "position_forward" [engine,runAddress] (positionInput [65,66] 8 0x200e 0xfffffffc)
      [wordsAt (glyphs+0x68) (placement 0 65536 0 0xfffffffc 4 ++ placement 1 65537 4 0 4),wordsAt (runAddress+28) [4,0]] (Just 0)
    ,make "font-thai-position-decomposed-cluster" 0x1189e68 "position_thai" [engine,runAddress]
      (positionInput [0xe01,0xe33] 8 0x200e 0 ++ [halves (engine+0x5a6) [0xe01,0xe4d,0xe32],halves (engine+0x61c) [1,0xe33,2,0xe32,513,2,0xe32,513],halves (engine+0xfc8) [7,0,0,2,4]])
      [halves (glyphs+4) [0xe01,0xe4d,0xe32]
      ,wordsAt (glyphs+0x68) (placement 0 65536 4 0 4)
      ,wordsAt (glyphs+0xa4) [1,33619969,4,0,0,7,4,0,0,7,0,0,0,0,0]
      ,wordsAt (glyphs+0xe0) (placement 1 2 8 4 4),wordsAt (runAddress+28) [8,0]] (Just 0)]
  inkBitmap :: [(Int,Int)] -> Bool -> B.ByteString
  inkBitmap origins clipped = B.pack [fromIntegral (sum [if ink (byte*8+bit) row then 2^(7-bit) else 0 | bit <- [0..7]] :: Int) | row <- [0..15],byte <- [0..7]]
   where
    ink x y = not (clipped && y == 5 && x >= 1 && x < 5) && (x `mod` 8 == 2 || any (painted x y) origins)
    painted x y (originX,originY) = let dx = x-originX; dy = y-originY-4 in dy >= 0 && dy < 3 && dx >= 0 && dx < 5 && testBit ([21,10,31 :: Int] !! dy) (4-dx)
  renderInput code clipped =
    positionInput [code,code] 8 0x200e 0 ++
    [word (engine+0x538) 2,halves (glyphs+4) [code,code]
    ,wordsAt (glyphs+0x68) (placement 0 65536 5 1 4 ++ placement 1 65537 13 9 4)] ++
    if clipped then [wordsAt (glyphs+0x90) [1,0,5,5,1]] else []
  renderCases =
    [make "font-empty-run-original-inclusive-glyph-edge" 0x118af98 "shape_basic" [engine,runAddress]
      (renderInput 65 False ++ [half (engine+0x526) 0,wordsAt runAddress [0,1,1,0,7,0,0,0,0]])
      [Patch (Address bitmap) (inkBitmap [(1,0)] False)] (Just 0)
    ,make "font-render-unsupported-backend-mode" 0x118af98 "shape_basic" [engine,runAddress]
      (renderInput 65 False ++ [word (engine+0xff4) 0]) [Patch (Address bitmap) (B.replicate 128 0x20)] (Just 0)] ++
    [make ("font-bdf-render-" ++ suffix) entry suffix [engine,runAddress] (renderInput code False)
      [Patch (Address bitmap) (inkBitmap [(1,0),(9,0)] False)] (Just 0)
    | (entry,suffix,code) <- [(0x118af98,"shape_basic",65),(0x1187c08,"shape_arabic",0x627),(0x11896cc,"shape_thai",0xe01),(0x118bda0,"shape_hebrew",0x5d0)]] ++
    [make "font-thai-bdf-clipped-render" 0x11896cc "shape_thai" [engine,runAddress] (renderInput 0xe0d True)
      [Patch (Address bitmap) (inkBitmap [(1,0),(9,0)] True)] (Just 0)]
  outlineErrors =
    [StageScenario (StageCaseName ("font-outline-source-error-" ++ suffix)) (publication entry suffix) [engine,runAddress]
      (initial ++ renderInput code True ++ [word (engine+0xff4) 1,word (workspaceHeader+12) 0x1fff,word (glyphs+0xc2c) 22,word (glyphs+0xc30) (output+0x100),word (glyphs+0xc34) 0xa5a5a5a5])
      [word (engine+0xfe8) (output+0x100),word (glyphs+0xc2c) 22,word (glyphs+0xc30) (output+0x100),word (glyphs+0xc34) 0xa5a5a5a5]
      (Just 0xfffffff6) [(Address (engine+0xffc),publication 0x11970f8 "handle_append_bitmap")] []
    | (entry,suffix,code) <- [(0x118af98,"shape_basic",65),(0x1187c08,"shape_arabic",0x627),(0x11896cc,"shape_thai",0xe01),(0x118bda0,"shape_hebrew",0x5d0)]]
  dictionaryInitial =
    [word (engine+4) scriptData,word (engine+0x70) scriptData,word (engine+0x518) 1,word (engine+0x51c) 0x10
    ,halves (scriptData+0x10) [1,0x20,1,0,0x50],halves (scriptData+0x30) [1,1,0,0x622]
    ,halves (scriptData+0x60) [1,0x10],halves (scriptData+0x70) [0xe14d,0,1]]
  dictionaryShapeCases =
    [make "font-arabic-bdf-dictionary-substitution" 0x1187c08 "shape_arabic" [engine,runAddress]
      (measureInput [0x627,0x653] 2 5 ++ dictionaryInitial)
      [half (engine+0x5a4) 1,half (engine+0x60a) 1,half (engine+0x540) 0x622,half (engine+0x5a6) 0xe14d
      ,halves (engine+0x60c) [0,0x627,0,0x622,2,0,0xe14d,2,1,0x653,0,0x622,1,0,0xe14d,1]
      ,word (engine+0x534) 4,word (engine+0x74) 0x62,runResult 2 2 5 4] (Just 0)
    ,make "font-arabic-unsupported-dictionary-version" 0x1187c08 "shape_arabic" [engine,runAddress]
      (measureInput [0x627,0x653] 2 5 ++ dictionaryInitial ++ [half (scriptData+0x10) 2])
      [word (engine+0x520) 0,word (engine+0x534) 4,half (engine+0x5a6) 0x622,runResult 2 2 5 4] (Just 4)
    ,make "font-arabic-short-workspace" 0x1187c08 "shape_arabic" [engine,runAddress]
      (measureInput [0x627] 2 5 ++ [word (workspaceHeader+12) 15,word (engine+0x534) 99])
      [word (engine+0x534) 99,half (engine+0x60a) 0] (Just 0)]
  ligatureInput =
    dictionaryInitial ++ [halves (engine+0x610) [0,0x622],half (engine+0x540) 0x622
    ,word (workspace+12) 0,wordsAt (output+0x40) [1,0,scriptData+0x16]]
  ligatureCases =
    [make ("font-ligature-" ++ label) 0x118768c "find_ligature" [engine,0,1,workspace,output+0x40,output]
      (ligatureInput ++ [halves (scriptData+0x70) [candidate,category,1]])
      [half output expected,word (engine+0x74) 0x62] (Just 1)
    | (label,candidate,category,expected) <- [("active",0xe14d,0,0xe14d),("missing-glyph",0xffff,0,0x622),("wrong-state",0xe14d,1,0x622)]] ++
    [make "font-ligature-contextual-restart" 0x118768c "find_ligature" [engine,0,1,workspace,output+0x40,output]
      (ligatureInput ++ [halves (engine+0x610) [0,0x628],halves (engine+0x540) [0x628,0x628],word (workspace+12) 2,word (workspace+28) 1
      ,halves (scriptData+0x60) [2,0x10,0x20],halves (scriptData+0x70) [0xe14d,2,2,0x628],halves (scriptData+0x80) [0xe14d,0,2,0x628]])
      [half output 0xe14d,word (engine+0x74) 0x62] (Just 2)]
  helperCases =
    [make "font-store-positioned-run-preserves-tail" 0x118d6dc "store_main_run" [engine]
      [halves (engine+0x524) [2,4],wordsAt (engine+0x52c) [6,9,0xfffffffc],wordsAt (engine+0xebc) [11,12,13,14]]
      [wordsAt (engine+0xea8) [0x40002,6,9,0xfffffffc,7],wordsAt (engine+0xebc) [11,12,13,14]] Nothing
    ,make "font-compose-private-measures-shared-glyph" 0x118d5ec "compose_private_run" [engine]
      [halves (engine+0x524) [0,3],halves (engine+0x60c) (basicMap 0 65 ++ [1,65,0,65,1,0,65,1] ++ [2,66,1,66,1,1,66,1])]
      ([word (engine+0x534) 8] ++ metric 0 0 4 0 0x200f ++ metric 1 0 4 4 0x200f ++ metric 2 1 4 4 0x200f) Nothing
    ,make "font-thai-measure-source-combining-control" 0x1189190 "measure_thai_lines" [engine]
      (positionInput [0xe01,0xe34,0xe48] 4 0x200e 0)
      ([word (engine+0x534) 4] ++ metric 0 0 4 0 0x200e ++ metric 1 1 4 4 0x200e ++ metric 2 2 4 4 0x200e) Nothing
    ,make "font-hebrew-prepare-source-mark-control" 0x118baec "prepare_hebrew_characters" [engine]
      (measureInput [0x591,0x5be,0x5c0,0x5c6] 6 9)
      [halves (engine+0x540) [0x25cc,0x5be,0x5c0,0x5c6],halves (engine+0x5a6) [0x25cc,0x5be,0x5c0,0x5c6],half (engine+0x5a4) 4,half (engine+0x60a) 4] Nothing]
