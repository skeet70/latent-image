{-# LANGUAGE GHC2021 #-}
module BankApplyFixture (bankApplyFixture, bankApplyFixtureAt, bankApplyAncestorFixtures) where

import qualified CustomAdmission as Boundary
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word16, Word32)
import Emulation.OwnedCase (word32le)
import Numeric (showHex)
import Oracle (replace)
import Replacement.PropertyService.Placement (Carrier(..), GuestAddress(..))

bankApplyFixture :: Boundary.BankApplyElfInput
bankApplyFixture = bankApplyFixtureAt (GuestAddress 0x5d0000)

bankApplyFixtureAt :: GuestAddress -> Boundary.BankApplyElfInput
bankApplyFixtureAt (GuestAddress entry) = Boundary.BankApplyElfInput
  { Boundary.bankApplyImageBase = GuestAddress entry
  , Boundary.bankApplyCarriers = [Carrier (GuestAddress entry) (GuestAddress (entry + 0x100))]
  , Boundary.bankApplyLinkedBytes = code
  , Boundary.bankApplySymbolBytes = C.pack (showHex entry "" ++ " c T xm1_storage_apply\n")
  , Boundary.bankApplyElfBytes = foldl (\bytes (offset, content) -> replace offset content bytes) (B.replicate 0x348 0)
      [(0, header), (52, segment), (0x100, code), (0x140, alternate), (0x180, symbols), (0x200, sectionNames), (0x230, symbolNames), (0x280, sections)]
  }
  where
    branch site = 0xeb000000 .|. (((0x624694 - site - 8) `shiftR` 2) .&. 0xffffff)
    code = B.concat (map word32le [0xe92d4800, branch (entry + 4), 0xe8bd8800, 0xe12fff1e, branch (entry + 16)])
    alternate = replace 8 (word32le 0xe1a00000) code
    half :: Word16 -> B.ByteString
    half value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8)]
    header = B.pack [0x7f,0x45,0x4c,0x46,1,1,1,0] <> B.replicate 8 0
      <> half 2 <> half 40 <> word32le 1 <> word32le entry <> word32le 52 <> word32le 0x280 <> word32le 0
      <> half 52 <> half 32 <> half 1 <> half 40 <> half 5 <> half 3
    segment = B.concat (map word32le [1,0x100,entry,entry,20,20,5,4])
    symbol name address size info = B.concat (map word32le [name,address,size,info])
    symbols = B.replicate 16 0 <> symbol 19 entry 0 0x00010000
      <> symbol 19 (entry + 8) 0 0x00010000 <> symbol 1 entry 12 0x00010012
    sectionNames = C.pack "\0.text\0.symtab\0.shstrtab\0.strtab\0"
    symbolNames = C.pack "\0xm1_storage_apply\0$a\0$d\0$t\0"
    sections = B.concat (map (B.concat . map word32le)
      [ replicate 10 0
      , [1,1,6,entry,0x100,20,0,0,4,0]
      , [7,2,0,0,0x180,64,4,3,4,16]
      , [15,3,0,0,0x200,fromIntegral (B.length sectionNames),0,0,1,0]
      , [25,3,0,0,0x230,fromIntegral (B.length symbolNames),0,0,1,0]
      ])

bankApplyAncestorFixtures :: [Boundary.BankApplyElfInput]
bankApplyAncestorFixtures = map make [GuestAddress 0x627500,GuestAddress 0x62a000]
 where
  make (GuestAddress entry) = Boundary.BankApplyElfInput
    { Boundary.bankApplyImageBase = GuestAddress entry
    , Boundary.bankApplyCarriers =
        [Carrier (GuestAddress entry) (GuestAddress (entry+0x10)),Carrier (GuestAddress (entry+0x20)) (GuestAddress (entry+0x64))]
    , Boundary.bankApplyLinkedBytes = replace (fromIntegral (fixed-entry)) fixedCode
        (replace 0 code (B.replicate (fromIntegral (fixed-entry+4)) 0))
    , Boundary.bankApplySymbolBytes = C.pack (concat
        [ row entry 12 "T" "xm1_storage_apply"
        , row (entry+0x20) 8 "T" "tail_leaf"
        , row (entry+0x30) 4 "T" "tail_parent"
        , row (entry+0x50) 8 "T" "calling_parent"
        , row fixed 4 "T" "settings_bank_initialize_entry"
        , row (entry+0x40) 4 "R" "branch_data"
        ])
    , Boundary.bankApplyElfBytes = foldl (\bytes (offset,content) -> replace offset content bytes) (B.replicate 0x600 0)
        [(0,header),(52,segments),(0x100,code),(0x180,fixedCode),(0x200,symbols),(0x300,sectionNames),(0x380,symbolNames),(0x500,sections)]
    }
   where
    fixed = 0x630a34
    row :: Word32 -> Word32 -> String -> String -> String
    row address size kind name = showHex address "" ++ " " ++ showHex size "" ++ " " ++ kind ++ " " ++ name ++ "\n"
    branch opcode site target = opcode .|. (((target-site-8) `shiftR` 2) .&. 0xffffff)
    code = foldl (\bytes (offset,content) -> replace offset (word32le content) bytes) (B.replicate 0x64 0)
      [ (0,0xe92d4800),(4,branch 0xeb000000 (entry+4) 0x624694),(8,0xe8bd8800)
      , (0x0c,branch 0xea000000 (entry+0x0c) entry)
      , (0x20,0xe1a00000),(0x24,branch 0xea000000 (entry+0x24) entry)
      , (0x30,branch 0xea000000 (entry+0x30) (entry+0x20))
      , (0x40,branch 0xea000000 (entry+0x40) entry)
      , (0x50,branch 0xeb000000 (entry+0x50) entry),(0x54,0xe12fff1e)
      , (0x60,branch 0xea000000 (entry+0x60) entry)
      ]
    fixedCode = word32le (branch 0xea000000 fixed (entry+0x30))
    half :: Word16 -> B.ByteString
    half value = B.pack [fromIntegral value,fromIntegral (value `shiftR` 8)]
    header = B.pack [0x7f,0x45,0x4c,0x46,1,1,1,0] <> B.replicate 8 0
      <> half 2 <> half 40 <> word32le 1 <> word32le entry <> word32le 52 <> word32le 0x500 <> word32le 0
      <> half 52 <> half 32 <> half 2 <> half 40 <> half 6 <> half 4
    segments = B.concat (map (B.concat . map word32le)
      [[1,0x100,entry,entry,0x64,0x64,5,4],[1,0x180,fixed,fixed,4,4,5,4]])
    names = ["","xm1_storage_apply","tail_leaf","tail_parent","calling_parent","settings_bank_initialize_entry","branch_data","$a","$d","$t"]
    nameOffset name = fromIntegral (sum (map ((+1) . length) (takeWhile (/=name) names)))
    symbolNames = C.pack (concatMap (++"\0") names)
    symbol name address size section info = B.concat (map word32le [nameOffset name,address,size,section*0x10000+info])
    symbols = B.replicate 16 0
      <> symbol "$a" entry 0 1 0 <> symbol "$d" (entry+0x40) 0 1 0
      <> symbol "$a" (entry+0x44) 0 1 0 <> symbol "$a" fixed 0 2 0
      <> symbol "xm1_storage_apply" entry 12 1 0x12
      <> symbol "tail_leaf" (entry+0x20) 8 1 0x12
      <> symbol "tail_parent" (entry+0x30) 4 1 0x12
      <> symbol "calling_parent" (entry+0x50) 8 1 0x12
      <> symbol "settings_bank_initialize_entry" fixed 4 2 0x12
      <> symbol "branch_data" (entry+0x40) 4 1 0x11
    sectionNames = C.pack "\0.text\0.fixed\0.symtab\0.shstrtab\0.strtab\0"
    sections = B.concat (map (B.concat . map word32le)
      [ replicate 10 0
      , [1,1,6,entry,0x100,0x64,0,0,4,0]
      , [7,1,6,fixed,0x180,4,0,0,4,0]
      , [14,2,0,0,0x200,fromIntegral (B.length symbols),5,5,4,16]
      , [22,3,0,0,0x300,fromIntegral (B.length sectionNames),0,0,1,0]
      , [32,3,0,0,0x380,fromIntegral (B.length symbolNames),0,0,1,0]
      ])
