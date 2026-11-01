module Fixtures (Fault (..), fixture, canonicalPaths) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits ((.&.))
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

-- These project-owned instructions exercise the published carrier and service-call ABI.
data Fault = Control | EquivalentMove | UnexecutedPadding | AlternateTargetRegister | GuestWrite | UnknownOffsetGuestWrite
  | MisalignedCall | BelowSpWrite | UninitializedParameter | WrongSave | EscapingBranch
  | ExecutedPadding | UnsupportedOpcode | BadReturn | UnknownServiceTarget
  | BadFunctionExtent | WrongDependency | DuplicateSymbol | ShortCarrier | LongCarrier
  | WrongLiteral | SpillExecution | ForgedServiceTarget | MissingParameterTail
  | WrongDirectDependency | WrongIndirectDependency
  deriving (Eq, Show, Enum, Bounded)

canonicalPaths :: [FilePath]
canonicalPaths = ["private-data/firmware/x-m1-1.50/FWUP0002.DAT","private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin","private-data/analysis/x-m1-1.50/decode/loader-40000.bin","private-data/analysis/x-m1-1.50/decode/loader-d0000.bin"]

fixture :: Fault -> (B.ByteString,B.ByteString)
fixture fault = (carrier,names)
  where
    base :: Word32
    base = 0x725dd8
    branch :: Word32 -> Word32 -> Word32
    branch at target = 0xeb000000 + ((target-at-8) `div` 4 .&. 0xffffff)
    body =
      [ 0xe92d4830,0xe24dd018,0xe3a03000
      , 0xe58d3000,0xe58d3004,0xe58d3008,0xe58d300c,0xe58d3010,0xe58d3014
      , 0xe30b4d20,0xe3404096,0xe5944000
      , 0xe5943080,0xe3a00001,0xe1a0100d,0xe12fff33
      , 0xe5945000,0xe3a00001,0xe1a0100d,0xe12fff35
      , 0xe594c00c,0xe3a00001,0xe12fff3c
      , 0xe3013618,0xe7943003,0xe12fff33
      , 0xe594c164,0xe12fff3c
      , 0xe5945168,0xe3a00000,0xe12fff35
      , branch (base+128) 0x7235d8
      , 0xe30336b0,0xe3403072,0xe12fff33
      , 0xe28dd018,0xe8bd8830
      ]
    program :: [(Int,Word32)]
    program = [(0,0xeaffffff)] ++ zip [4,8..] body
      ++ [(152,0xe12fff1e),(704,0xe92d4008),(708,branch (base+708) (base+152)),(712,0xe8bd8008),(848,0x409c280c)]
    changes :: [(Int,Word32)]
    changes = case fault of
      EquivalentMove -> [(56,0xe3000001)]
      UnexecutedPadding -> [(400,0xe320f000)]
      AlternateTargetRegister -> [(52,0xe594c080),(64,0xe12fff3c)]
      GuestWrite -> guestWrite 0xe3a00b01
      UnknownOffsetGuestWrite -> guestWrite 0xe0822180
      MisalignedCall -> [(8,0xe24dd014)]
      BelowSpWrite -> [(16,0xe50d3004)]
      UninitializedParameter -> [(16,0xe1a03003)]
      WrongSave -> [(148,0xe8bd8810)]
      EscapingBranch -> [(0,0xea0000d3)]
      ExecutedPadding -> [(0,0xea000062)]
      UnsupportedOpcode -> [(12,0xf57ff04f)]
      BadReturn -> [(148,0xe12fff10)]
      UnknownServiceTarget -> [(64,0xe12fff32)]
      WrongLiteral -> [(848,0x409c2810)]
      SpillExecution -> [(0,0xea0000c7),(804,0xe12fff1e)]
      ForgedServiceTarget -> [(52,0xe3a03000)]
      MissingParameterTail -> [(36,0xe1a03003)]
      WrongDirectDependency -> [(128,branch (base+128) 0x7235dc)]
      WrongIndirectDependency -> [(132,0xe30336b4)]
      _ -> []
    guestFixture = fault `elem` [GuestWrite,UnknownOffsetGuestWrite]
    guestWrite argument = [(144,branch (base+144) (base+200)-0x01000000)
      ,(200,0xe30b2204),(204,0xe34420a5),(208,argument),(212,0xe5820004),(216,0xe28dd018),(220,0xe8bd8830)]
    full = B.concat [U.putWord32 (maybe (maybe 0 id (lookup offset program)) id (lookup offset changes)) | offset <- [0,4..848]]
    carrier = case fault of ShortCarrier -> B.take 848 full; LongCarrier -> full <> U.putWord32 0; _ -> full
    row :: Word32 -> Word32 -> String -> String -> String
    row address size kind name = showHex address "" ++ " " ++ showHex size "" ++ " " ++ kind ++ " " ++ name ++ "\n"
    codeRows :: [(Word32,Word32,String,String)]
    codeRows =
      [(base,0,"T","__task_submission_start"),(base+852,0,"T","__task_submission_end"),(base+if guestFixture then 224 else 156,0,"T","__task_submission_bodies_end")
      ,(base,4,"T","xm1_task_submit_entry"),(base+4,if fault == BadFunctionExtent then 144 else if guestFixture then 220 else 148,"T","xm1_task_submit"),(base+152,4,"T","xm1_task_release")
      ,(base+704,12,"T","xm1_task_release_entry"),(base+848,4,"R","xm1_task_release_lock_literal")]
    dependencies :: [(String,Word32)]
    dependencies = [("critical_enter",0x7235d8),("critical_exit",0x7236b0),("reservation",0x725c6c),("fill",0x7221c8),("cache",0x42634)
      ,("log",0x41a34),("error_callback_dispatch",0x723398),("try_lock",0x427d4),("unlock",0x427fc),("class_table",0x7cad9c)
      ,("interface_roots",0x96bd20),("wrapper_base",0x40a5b204),("state_base",0x40a0fc44),("error_word",0x40a0f82c),("release_lock",0x409c280c)]
    names = C.pack (concat [row address size kind name | (address,size,kind,name) <- codeRows]
      ++ concat [row (address+if fault == WrongDependency && name == "cache" then 4 else 0) (0 :: Word32) "A" ("xm1_task_" ++ name) | (name,address) <- dependencies]
      ++ if fault == DuplicateSymbol then row base (0 :: Word32) "T" "xm1_task_submit_entry" else "")
