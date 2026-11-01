module Fixtures (Fault (..), carrier) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

-- Original synthetic A32 encodings; no firmware instructions were copied.
data Fault = Control | BelowSpStore | BelowSpLoad | ResurrectFrame | MisalignedSp | UnprovenSp | OutsideSp
  | StoreWritebackOverlap | LoadWritebackOverlap | PushSp | PopSp | YieldHint | WaitHint | UnknownWordStore | UnknownWordLoad
  | ScalarExtend | RotatedExtend | ConditionalExtend | CapabilityExtend | ReturnExtend | ExtendSp | ExtendPc | InternalReturnExtend
  deriving (Eq, Show, Enum, Bounded)
data Label = Body | Setup | Install | Spin | Root | Idle0 | Diagnostic | Zero | Return | Submit deriving (Eq, Ord, Show)
data Instruction = Word Word32 | Mark Label | Branch Word32 Label | Call Word32

origin :: Word32
origin = 0x69cdf8

carrier :: Fault -> (B.ByteString,B.ByteString)
carrier fault =
  let program =
        map Word [0xe92d4830,0xe28db00c,0xe24dd020,0xe28d4004,0xe320f000,0xe320f000,0xe320f000]
        ++ [Mark Body] ++ map Word (injection fault)
        ++ [Word 0xe3500000,Branch 0x0a000000 Setup,Word 0xe3500002,Branch 0x0a000000 Diagnostic
           ,Word 0xe3500001,Branch 0x0a000000 Root,Word 0xe3500003,Branch 0x0a000000 Idle0
           ,Word 0xe3500004,Branch 0x1a000000 Zero]
        ++ task 103 0x1970 15 0xcbf0 ++ task 93 0x1978 13 0xcd58 ++ [Branch 0xea000000 Zero,Mark Root]
        ++ task 104 0x1960 1 0xd160 ++ [Branch 0xea000000 Zero,Mark Idle0]
        ++ task 102 0x1968 15 0xcbf0 ++ [Branch 0xea000000 Zero,Mark Diagnostic]
        ++ [Word (wide 0 0xcc6c),Word (top 0 0x69),Branch 0xea000000 Return,Mark Zero,Word 0xe3a00000
           ,Mark Return,Word 0xe24bd00c,Word 0xe8bd8830,Mark Setup,Call 0x657184,Word 0xe3500001
           ,Branch 0x1a000000 Install,Word (wide 0 0xec00),Word (top 0 0x89),Word 0xe3a01000
           ,Word (wide 2 0x4b80),Word (top 2 0xd),Call 0x7221a0,Mark Install,Call 0x725a04
           ,Word 0xe3500000,Branch 0x1a000000 Spin,Branch 0xea000000 Zero,Mark Spin,Branch 0xea000000 Spin
           ,Mark Submit]
        ++ [Word 0xe6efc07e | fault == InternalReturnExtend]
        ++ [Word 0xe92d4020,Word 0xe5840000,Word 0xe5841004,Word 0xe5842008
           ,Word 0xe3a05b01,Word 0xe584500c,Word 0xe5843010,Word 0xe3a05000,Word 0xe5845014
           ,Word 0xe1a00004,Call 0x725dd8,Word 0xe8bd8020]
      (_,labels) = foldl locate (origin,M.empty) program
      locate (pc,found) (Mark label) = (pc,M.insert label pc found)
      locate (pc,found) _ = (pc+4,found)
      encode pc instruction = case instruction of
        Word word -> (pc+4,[word])
        Mark _ -> (pc,[])
        Branch opcode label -> (pc+4,[branch opcode pc (labels M.! label)])
        Call target -> (pc+4,[branch 0xeb000000 pc target])
      (_,chunks) = foldl (\(pc,wordsSoFar) instruction -> let (next,emitted) = encode pc instruction in (next,wordsSoFar ++ emitted)) (origin,[]) program
      bytes = B.concat (map U.putWord32 chunks)
      row :: Word32 -> Word32 -> String -> String
      row address size name = showHex address "" ++ " " ++ showHex size "" ++ " T " ++ name ++ "\n"
      symbols = C.pack (row origin 0 "__startup_dispatch_start"
        ++ row (origin + fromIntegral (B.length bytes)) 0 "__startup_dispatch_end"
        ++ row origin 28 "xm1_startup_dispatch_entry"
        ++ row (labels M.! Body) 0 "xm1_startup_dispatch")
  in (bytes,symbols)
  where
    task taskId name priority entry =
      map Word (case fault of
        ScalarExtend -> [wide 0 (taskId .|. 0xab00),0xe6ef0070]
        RotatedExtend -> [wide 0 ((taskId `shiftL` 8) .|. 0xab),0xe6ef0470]
        _ -> [0xe3a00000 .|. taskId])
      ++ map Word [wide 1 name,top 1 0x78,0xe3a02000 .|. priority,wide 3 entry,top 3 0x69]
      ++ [Branch 0xeb000000 Submit]
    wide register value = 0xe3000000 .|. (register `shiftL` 12) .|. ((value .&. 0xf000) `shiftL` 4) .|. (value .&. 0xfff)
    top register value = 0xe3400000 .|. (register `shiftL` 12) .|. ((value .&. 0xf000) `shiftL` 4) .|. (value .&. 0xfff)
    branch opcode site target = opcode .|. (fromInteger ((toInteger target-toInteger site-8) `div` 4) .&. 0xffffff)

injection :: Fault -> [Word32]
injection fault =
  let control =
        -- Allocate two scratch words, spill an alias, reload it, then release them.
        [0xe24dd008,0xe1a0c00d,0xe58cc004,0xe59dc004,0xe28dd008
        ,0xe320f000,0xe320f000,0xe320f000]
      changes = case fault of
        Control -> []
        BelowSpStore -> [(5,0xe50dc004)]
        BelowSpLoad -> [(5,0xe51dc004)]
        ResurrectFrame -> [(5,0xe28dd030),(6,0xe24dd030)]
        MisalignedSp -> [(5,0xe24dd001),(6,0xe28dd001)]
        UnprovenSp -> [(5,0xe1a0c00d),(6,0xe1a0d000),(7,0xe1a0d00c)]
        OutsideSp -> [(5,0xe24dda01),(6,0xe28dda01)]
        StoreWritebackOverlap -> [(2,0xe5acc004)]
        LoadWritebackOverlap -> [(3,0xe5bcc004)]
        PushSp -> [(5,0xe92d2000),(6,0xe8bd1000)]
        PopSp -> [(5,0xe1a0c00d),(6,0xe92d1000),(7,0xe8bd2000)]
        YieldHint -> [(5,0xe320f001)]
        WaitHint -> [(5,0xe320f003)]
        UnknownWordStore -> [(5,0xe58d001c)]
        UnknownWordLoad -> [(5,0xe59dc01c)]
        ScalarExtend -> []
        RotatedExtend -> []
        ConditionalExtend -> [(5,0xe3a0c000),(6,0xe35c0000),(7,0x16efc074)]
        CapabilityExtend -> [(5,0xe6efc074)]
        ReturnExtend -> [(5,0xe6efc07e)]
        ExtendSp -> [(5,0xe6efd070)]
        ExtendPc -> [(5,0xe6eff070)]
        InternalReturnExtend -> []
  in [maybe word id (lookup index changes) | (index,word) <- zip [0 :: Int ..] control]
