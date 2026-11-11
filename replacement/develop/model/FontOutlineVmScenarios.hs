{-# LANGUAGE GHC2021 #-}
module FontOutlineVmScenarios
  ( OutlineRoot(..), OutlineScenario(..), OutlineFatalScenario(..), outlineScenarios, outlineFatalScenarios, genuineGlyphScenarios, outlineOriginal, outlineSymbol ) where

import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Int (Int16)
import Data.Word (Word8, Word32)
import Scenarios (Patch(..), Address(..), word, wordsAt)

data OutlineRoot = RenderOutline | ExecuteOutline | EmitOutline deriving (Eq, Show)
data OutlineScenario = OutlineScenario
  { outlineName :: String
  , outlineRoot :: OutlineRoot
  , outlineArguments :: [Word32]
  , outlineInitial :: [Patch]
  , outlineExpected :: [Patch]
  , outlineResult :: Maybe Word32
  } deriving (Eq, Show)

data OutlineFatalScenario = OutlineFatalScenario
  { fatalInput :: OutlineScenario
  , fatalKernelEntry :: Word32
  , fatalService :: Word32
  , fatalArgument :: Word32
  } deriving (Eq, Show)

outlineOriginal :: OutlineRoot -> Word32
outlineOriginal RenderOutline = 0x1198598
outlineOriginal ExecuteOutline = 0x1199868
outlineOriginal EmitOutline = 0x1199440

outlineSymbol :: OutlineRoot -> String
outlineSymbol RenderOutline = "xm1_pipeline_image_font_render_outline_program"
outlineSymbol ExecuteOutline = "xm1_pipeline_image_font_execute_outline_program"
outlineSymbol EmitOutline = "xm1_pipeline_image_font_emit_outline"

vm, program, code, output :: Word32
vm = 0x9a0000
program = vm - 0x78
code = 0x9a2000
output = 0x9a3000

bytes :: Word32 -> [Word8] -> Patch
bytes address values = Patch (Address address) (B.pack values)
halves :: Word32 -> [Int16] -> Patch
halves address values = bytes address (concatMap (\v -> [fromIntegral v, fromIntegral (v `shiftR` 8)]) values)
shorts :: [Int16] -> [Word8]
shorts values = concatMap (\v -> [fromIntegral v, fromIntegral (v `shiftR` 8)]) values

initial :: [Word8] -> [Patch]
initial instructions =
  [ Patch (Address vm) (B.replicate 0x800 0)
  , Patch (Address program) (B.replicate 0x100 0)
  , Patch (Address output) (B.replicate 0x100 0xa5)
  , word vm program
  , word (program+8) code
  , word (program+0xb0) (fromIntegral (length instructions))
  , word (program+0xb4) 0
  , word (vm+0x18) 2
  , bytes (vm+0x1c) [10,20,50,60]
  , wordsAt (vm+0x764) [100,200,356,712,256,512]
  , word (vm+0x50) output
  , word (vm+0x54) 128
  , bytes code instructions
  ]

execute :: String -> [Word8] -> [Patch] -> [Patch] -> Word32 -> OutlineScenario
execute name instructions extra expected result = OutlineScenario name ExecuteOutline [vm,0,0] (initial instructions ++ extra) expected (Just result)

outlineScenarios :: [OutlineScenario]
outlineScenarios = controlCases ++ coordinateCases ++ lineCases ++ curveCases ++ arithmeticCases ++ finalizerCases ++ renderCases
 where
  controlCases =
    [execute "empty-preserves-output" [] [] [word (vm+0x664) (vm+0x64), word (vm+0x668) (vm+0x364), word (vm+0x58) 0] 0
    ,execute "null-dispatch-consumes-two" [1,0x12] [] [word (program+0xb0) 0,word (program+0xb4) 2,word (vm+0x754) 1,word (vm+0x758) 2] 0xfffffffd
    ,execute "reserved-instructions-consume-only-header" [0x36,0x12,0x37,0x34] [] [word (program+0xb4) 4,word (vm+0x58) 0] 0
    ,execute "finish-8003-stops-before-invalid" [0x71,0,1,0] [] [word (program+0xb0) 2,word (program+0xb4) 2] 0
    ,execute "finish-8004-continues-to-invalid" [0x70,0,1,0] [] [word (program+0xb0) 0,word (program+0xb4) 4] 0xfffffffd
    ,execute "sentinel-rewinds-remaining-and-cursor" [0xf0,0x71,0] [] [word (vm+0x38) 0,word (vm+0x3c) 3,word (vm+0x77c) 0xffffffff,halves (vm+0x64) [0,-1,-1,-1,0,0]] 0
    ]
  coordinateCases =
    [execute "byte-axis-nonzero" [0xb5,0x22,64,32] [] [word (vm+0x678) 164,word (vm+0x6b8) 264] 0
    ,execute "byte-255-means-256" [0xb5,0x22,255,255] [] [word (vm+0x678) 356,word (vm+0x6b8) 712] 0
    ,execute "byte-rotated-nonzero" [0xb4,0x22,64,32] [] [word (vm+0x678) 100,word (vm+0x6b8) 360] 0
    ,execute "short-axis-negative-truncates" ([0x35,0x22]++shorts [-1,-1]) [word (vm+0x774) 257,word (vm+0x778) 129] [word (vm+0x678) 99,word (vm+0x6b8) 200] 0
    ,execute "short-rotated-negative" ([0x34,0x22]++shorts [-64,32]) [] [word (vm+0x678) 0xffffffe4,word (vm+0x6b8) 104] 0
    ,execute "copy-x-and-y" [0x32,0x02,0x33,0x12] [] [word (vm+0x678) 80,word (vm+0x6b8) 480] 0
    ,execute "normal-basis" [0x30,0x01] [] [wordsAt (vm+0x764) [80,160,400,480,320,320]] 0
    ,execute "cross-basis" [0x31,0x01] [] [wordsAt (vm+0x764) [80,480,400,160,320,0xfffffec0]] 0
    ]
  lineCases =
    [execute "short-axis-new-line" ([0x10,0x01]++shorts [16,32,64,96]) [] [halves (vm+0x364) [264,116,-1,-1,392,164],word (vm+0x674) 164,word (vm+0x6b4) 392] 0
    ,execute "byte-axis-new-line" [0x90,0x01,16,32,64,96] [] [halves (vm+0x364) [264,116,-1,-1,392,164]] 0
    ,execute "register-line" [3,0x01] [] [halves (vm+0x364) [160,80,-1,-1,480,400]] 0
    ,execute "register-line-offsets-and-origins" [7,0x01] [halves (vm+0x40) [3],word (vm+0x44) 5,word (vm+0x5c) 2,word (vm+0x60) 3] [halves (vm+0x64) [173,99,-1,-1,493,419],wordsAt (vm+0x77c) [104,176,104,176,424,496]] 0
    ,execute "rotated-line-endpoint" ([2,0x01]++shorts [64,32]) [] [halves (vm+0x364) [160,80,-1,-1,360,100]] 0
    ]
  curveCases =
    [execute "short-axis-curve-start" ([0x18,0x01]++shorts [3,16,32,32,64,64,96]) [] [halves (vm+0x364) [264,116,328,132,392,164],word (vm+0x674) 164,word (vm+0x6b4) 392] 0
    ,execute "byte-axis-curve-start" [0x98,0x01,3,16,32,32,64,64,96] [] [halves (vm+0x364) [264,116,328,132,392,164]] 0
    ,execute "axis-curve-close" ([0x1b,0x01]++shorts [2,32,64]) [] [halves (vm+0x364) [160,80,328,132,480,400]] 0
    ,execute "basis-axis-close" [0x7a,0x01,2,128,64] [] [halves (vm+0x364) [160,80,240,240,480,400]] 0
    ,execute "basis-rotated-close" [0x78,0x01,2,128,64] [] [halves (vm+0x364) [160,80,400,160,480,400]] 0
    ,execute "zero-count-curve-still-emits" [0x9a,0x01,0,32,64,64,96] [] [halves (vm+0x364) [160,80,328,132,392,164]] 0
    ]
  arithmeticCases =
    [execute "scaled-table-add-discards-second-short" ([0x20,0x02]++shorts [-128,1234]) [word (vm+0x710) 257] [word (vm+0x678) 0xffffffd0,word (program+0xb4) 6] 0
    ,execute "scaled-y-add-independent-destination" ([0x40,0x02]++shorts [128,-999]) [word (vm+0x730) 257] [word (vm+0x6b8) 288] 0
    ,execute "scaled-x-subtract" ([0x58,0x02]++shorts [128]) [word (vm+0x710) 257] [word (vm+0x678) 0xffffffd0] 0
    ,execute "unscaled-x-add-consumes-discard" ([0xa0,0x02]++shorts [-999]) [word (vm+0x730) 257] [word (vm+0x678) 337] 0
    ,execute "unscaled-x-add-no-discard" [0xd0,0x02] [word (vm+0x710) 257] [word (vm+0x678) 337] 0
    ,execute "unscaled-y-subtract" [0xe8,0x02] [word (vm+0x730) 257] [word (vm+0x6b8) 0xffffff9f] 0
    ]
  finalizerCases =
    [ OutlineScenario ("finalize-"++show vertical++"-capacity-"++show capacity) EmitOutline [vm]
        (initial [] ++ [word (vm+4) vertical,word (vm+0x54) capacity,word (vm+0x664) (vm+0x70),word (vm+0x668) (vm+0x370),halves (vm+0x64) [10,20,-1,-1,30,40],halves (vm+0x364) [30,40,50,60,70,80]])
        ([word (vm+0x58) 14] ++ if capacity >= 14 then [halves output (if vertical == 0 then [4,4,1,10,20,2,30,40,3,50,60,30,40,4] else [4,4,1,2028,10,2,2008,30,3,1988,50,2008,30,4]),word (vm+0x50) (output+28)] else [word (vm+0x50) output]) Nothing
    | vertical <- [0,1], capacity <- [0,14]]
  renderCases =
    [ OutlineScenario "render-primary-empty" RenderOutline [program,0,0] (initial []) [word (program+0x6c) 0] (Just 0)
    , OutlineScenario "render-skip-primary" RenderOutline [program,0,0] (initial [] ++ [bytes code [0xfd,2,0,0xaa,0xbb],word (program+0x60) 5,word (program+0x64) 0]) [word (program+0x60) 0,word (program+0x64) 5,word (program+0x6c) 0] (Just 0)
    , OutlineScenario "render-glyph-directory-nonzero" RenderOutline [program,0,0]
        (initial [] ++ [bytes code [0,10,20,50,60],word (program+0xc) 1,word (program+0x18) 0x100,word (program+0x1c) 0x200,halves (code+0x100) [0,6],bytes (code+0x200) [0x20,0,3,1,7,1],word (program+0x60) 5,word (program+0x64) 0])
        [word (program+0x60) 0,word (program+0xb0) 0,word (program+0xb4) 0x206,word (vm+0x58) 12,halves output [4,3,1,160,80,2,480,400,2,160,80,4]] (Just 0)
    , OutlineScenario "render-glyph-point-repeat-mask" RenderOutline [program,0,0]
        (initial [] ++ [bytes code [0,10,20],word (program+0xc) 1,word (program+0x18) 0x100,word (program+0x1c) 0x200,halves (code+0x100) [0,6],bytes (code+0x200) [0x20,3,3,1,7,1],word (program+0x60) 3,word (program+0x64) 0])
        [word (program+0x60) 0,word (vm+0x58) 12,halves output [4,3,1,160,80,2,160,80,2,160,80,4]] (Just 0)
    , OutlineScenario "render-compound-affine-parameters" RenderOutline [program,0,0]
        (initial [] ++ [bytes code [0xfc,0,0,3,20,100,30,120],word (program+0x10) 1,word (program+0x20) 0x100,word (program+0x24) 0x200,bytes (code+0x100) [0,0,0,9,0,0],bytes (code+0x200) [10,20,50,60,0,10,20,50,60],word (program+0xc) 1,word (program+0x18) 0x110,word (program+0x1c) 0x220,halves (code+0x110) [0,6],bytes (code+0x220) [0x20,0,3,1,7,1],word (program+0x60) 8,word (program+0x64) 0])
        [word (program+0x60) 0,word (program+0x70) 0,word (program+0x6c) 0,word (vm+0x58) 12,halves output [4,3,1,328,88,2,968,728,2,328,88,4]] (Just 0)
    , OutlineScenario "render-inline-invalid-opcode" RenderOutline [program,0,0]
        (initial [] ++ [bytes code [0xfa,0,2,0,1,0],word (program+0x60) 6,word (program+0x64) 0])
        [word (program+0x60) 0,word (program+0x64) 6,word (program+0xb0) 0,word (program+0xb4) 6,word (program+0x6c) 0] (Just 0xfffffffd)
    ]

genuineGlyphScenarios :: B.ByteString -> [OutlineScenario]
genuineGlyphScenarios font =
  [OutlineScenario "E14D-four-contour-glyph" RenderOutline [program,0,0]
    (initial [] ++ [Patch (Address code) (B.take 102 (B.drop 0x3edf0 font)),word (program+0x60) 102,word (program+0x64) 0,word (vm+0x18) 0])
    [word (program+0x60) 0,word (program+0x64) 102,word (vm+0x58) 81,word (vm+0x50) (output+162),halves output
      [7,6,1,831,375,2,999,375,2,999,1759,2,855,1759,2,855,823,2,831,375,4
      ,7,6,1,191,375,2,511,1423,2,567,1759,2,455,1759,2,167,823,2,191,375,4
      ,6,5,1,831,375,2,855,823,2,567,1759,2,511,1423,2,831,375,4
      ,7,6,1,23,375,2,191,375,2,167,823,2,167,1759,2,23,1759,2,23,375,4]]
    (Just 0)]

outlineFatalScenarios :: [OutlineFatalScenario]
outlineFatalScenarios =
  [ OutlineFatalScenario
      (OutlineScenario "render-transform-zero-divisor-fatal" RenderOutline [program,code+0x300,0]
        (initial [] ++ [bytes code [0,10,20,50,60],word (program+0xc) 1,word (program+0x18) 0x100,word (program+0x1c) 0x200,halves (code+0x100) [0,6],bytes (code+0x200) [0x20,0,3,1,7,1],halves (code+0x300) [0,0,0,0,0,0,0,0,0,0,128],word (program+0x60) 5,word (program+0x64) 0])
        [] Nothing)
      0x43604 0x1010 0
  ]
