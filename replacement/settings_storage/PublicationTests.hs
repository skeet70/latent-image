{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.List (sort)
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.SettingsStorage.Publication
import qualified Replacement.RecordStore.References as R
import qualified Replacement.RecordStore.Carrier as RecordCarrier
import Replacement.PropertyService.Placement (ScanImage(..))
import ServiceInventory (Image(..))

ensure :: Bool -> String -> IO ()
ensure value message = unless value (fail message)
right :: Either String a -> IO a
right = either fail pure
hostSymbols :: Word32 -> B.ByteString
hostSymbols count = C.pack (showHex (0x5d0000+count) " 00000000 A __custom_text_end\n")
data MapRow = MapRow Word32 Word32 Word32 String
hostMap :: [MapRow] -> B.ByteString
hostMap rows = C.pack ("VMA LMA Size Align Out In Symbol\n" ++ concatMap row rows)
  where row (MapRow address count alignment name) = showHex address " " ++ showHex address " " ++ showHex count " " ++ show alignment ++ " fixture.o:(" ++ name ++ ")\n"
hostLinker :: B.ByteString
hostLinker = C.pack ("SECTIONS {\n  .text 0x5d0000 : { *(.text*) }\n"
  ++ concat ["  .fixture_" ++ show index ++ " 0x" ++ showHex address "" ++ " : { KEEP(*(.text." ++ entry ++ ")) }\n" | (index,(address,entry,_)) <- zip [0 :: Int ..] entries]
  ++ "  .data : { *(.data*) }\n  .bss : { *(.bss*) }\n  /DISCARD/ : { *(.ARM.exidx*) }\n}\n")
outputAddresses :: B.ByteString -> [Word32]
outputAddresses linker = [address | line <- C.lines linker,(_:encoded:":":_) <- [C.words line],Just raw <- [C.stripPrefix "0x" encoded],(address,"") <- readHex (C.unpack raw)]
bytes :: Word32 -> B.ByteString
bytes value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]
patch :: B.ByteString -> Int -> B.ByteString -> B.ByteString
patch image offset value = B.take offset image <> value <> B.drop (offset+B.length value) image
entries :: [(Word32,String,String)]
entries = [(address,"settings_custom_"++name++"_entry","settings_custom_"++name) | (address,name) <- [(0x6a8484,"prepare"),(0x6a84d4,"check"),(0x6a865c,"read"),(0x6a86c4,"refresh"),(0x6a877c,"defaults"),(0x6a8abc,"boot")]]
       ++ [(address,"record_store_entry_"++name,"record_store_"++name) | (address,name) <- [(0x6ac2b4,"open"),(0x6ac328,"register"),(0x6ac478,"close"),(0x6ac4c8,"read_latest"),(0x6ac7d4,"append"),(0x6ac984,"erase_all"),(0x6acab8,"collect"),(0x6acda0,"read_at"),(0x6ae0fc,"zero")]]
       ++ [(address,"settings_bank_"++name++"_entry","xm1_storage_"++name) | (address,name) <- [(0x630a34,"initialize"),(0x61db04,"copy"),(0x61db3c,"capture"),(0x61db18,"restore"),(0x61daa8,"exchange"),(0x61d8dc,"write_body"),(0x61d92c,"write_active_body"),(0x61d944,"write_header"),(0x61d994,"read_custom"),(0x61d9ec,"read_body"),(0x61da3c,"read_header"),(0x6309c4,"apply")]]
fixture :: PublicationPlan -> (B.ByteString,B.ByteString,B.ByteString,Stage4Contract)
fixture plan = (image, linked, symbols, contract)
  where
    base = case publicationRegions plan of [Region start _] -> start; _ -> error "single-section fixture must occupy one carrier"
    end = base + 0x2000
    image = B.concat (replicate (0x100000 `div` 4) (bytes 0xe7f000f0))
    linked = foldl (\acc (address,_,_) -> patch acc (fromIntegral (address-0x61d8dc)) (bytes (0xea000000 .|. (((base-address-8) `shiftR` 2) .&. 0xffffff)))) (B.replicate (fromIntegral (0x6ae100-0x61d8dc :: Word32)) 0) entries
    line :: Word32 -> String -> String
    line address name = showHex address " 00000004 T " ++ name ++ "\n"
    symbols = C.pack (line base "__custom_text_start" ++ line end "__custom_text_end"
      ++ line base "__custom_segment_0_start" ++ line end "__custom_segment_0_end"
      ++ concat [line address (C.unpack name) | (name,address) <- RecordCarrier.cacheSymbols]
      ++ line 0x61d8dc "__published_image_start"
      ++ concat [line address entry ++ line base target | (address,entry,target) <- entries])
    contract = Stage4Contract "dat" "image" (Region 0x62c120 0x62fbfc) (Region 0x61e110 0x6309c4) [] [("xm1_storage_store8",0x62c120),("xm1_storage_store16",0x62c1ac)]

main :: IO ()
main = do
  let oneMap = hostMap [MapRow 0x5d0000 0x2000 4 ".text.fixture"]
  plan <- right (planPublication (hostSymbols 0x2000) oneMap)
  ensure (publicationRegions plan == [Region 0x627578 0x629eac]) "whole 8192-byte section did not fit its only eligible carrier"
  ensure (isLeft (planPublication (hostSymbols 0x3000) (hostMap [MapRow 0x5d0000 0x3000 4 ".text.large"]))) "oversized indivisible text section was accepted"
  ensure (isLeft (planPublication (hostSymbols 0) "VMA LMA Size Align Out In Symbol\n")) "empty publication was accepted"
  ensure (isLeft (planPublication (hostSymbols 0x2000) "missing map")) "unmeasured publication was accepted"
  ensure (isLeft (planPublication (hostSymbols 0x2000) (hostMap [MapRow 0x5d0004 0x1ffc 4 ".text.gap"]))) "unmeasured leading gap was accepted"
  ensure (isLeft (planPublication (hostSymbols 0x2000) (hostMap [MapRow 0x5d0000 0x1800 4 ".text.first",MapRow 0x5d1000 0x1000 4 ".text.overlap"]))) "overlapping measured sections were accepted"
  ensure (isLeft (planPublication (hostSymbols 0x2000) (hostMap [MapRow 0x5d0000 0x2000 3 ".text.alignment"]))) "non-power-of-two alignment was accepted"
  ensure (isLeft (planPublication (hostSymbols 0x2000) (hostMap [MapRow 0x5d0000 0x2000 4 ".text.*"]))) "linker wildcard injection was accepted"
  ensure (isLeft (planPublication (hostSymbols 0x4400) (hostMap [MapRow 0x5d0000 0x2200 4 ".rodata.first",MapRow 0x5d2200 0x2200 4 ".rodata.second"]))) "merged read-only data was split to force a fit"
  let multiMap = hostMap [MapRow 0x5d0000 0x2000 4 ".text.first",MapRow 0x5d2000 0x1b00 4 ".text.second",MapRow 0x5d3b00 0x100 4 ".rodata.values"]
  multi <- right (planPublication (hostSymbols 0x3c00) multiMap)
  ensure (length (publicationRegions multi) >= 2 && all (`elem` [Region 0x627578 0x629eac,Region 0x61e114 0x61ffe0,Region 0x629eb4 0x62b900]) (publicationRegions multi)) "multi-carrier publication enlarged an approved interval"
  linker <- right (publicationLinker multi hostLinker)
  ensure (outputAddresses linker == sort (outputAddresses linker)) "published load sections are not globally ascending"
  ensure (all (\(address,_,_) -> length (filter (==address) (outputAddresses linker)) == 1) entries) "fixed veneers were omitted or duplicated"
  ensure ("*(.text.first)" `B.isInfixOf` linker && "*(.text.second)" `B.isInfixOf` linker && "__custom_segment_0_start" `B.isInfixOf` linker) "whole measured sections lack published output"
  ensure (isLeft (publicationLinker plan "missing")) "missing host linker marker was accepted"
  let (source,linked,symbols,contract) = fixture plan
      base = case publicationRegions plan of [Region start _] -> start; _ -> error "single-section fixture must occupy one carrier"
      sourceOffset :: Word32 -> Int
      sourceOffset address = fromIntegral (address-0x61c400)
  staged <- right (stagePublication contract plan source linked symbols)
  ensure (publishedRegions plan symbols == Right [Region base (base+0x2000)]) "published segment markers do not describe actual used bytes"
  ensure (isLeft (publishedRegions plan (symbols <> C.pack (showHex (base+0x2004) " 00000000 A __custom_segment_0_end\n")))) "published segment exceeded its measured budget"
  ensure (B.length staged == B.length source) "publication changed decoded image extent"
  ensure (B.take (sourceOffset 0x61d8dc) staged == B.take (sourceOffset 0x61d8dc) source) "publication changed prefix outside owned ranges"
  ensure (B.take 4 (B.drop (sourceOffset 0x6a8484) staged) == B.take 4 (B.drop (fromIntegral (0x6a8484-0x61d8dc :: Word32)) linked)) "custom entry was not patched"
  ensure (B.take 4 (B.drop (sourceOffset 0x6ac2b4) staged) == B.take 4 (B.drop (fromIntegral (0x6ac2b4-0x61d8dc :: Word32)) linked)) "record entry was not patched"
  ensure (B.take 4 (B.drop (sourceOffset 0x61d8dc) staged) == B.take 4 linked) "bank entry was not patched"
  ensure (B.take 4 (B.drop (sourceOffset 0x61d8e0) staged) == bytes 0xe7f000f0) "retired bank interior was not trapped"
  ensure (B.take 4 (B.drop (sourceOffset 0x6a8488) staged) == bytes 0xe7f000f0) "retired custom interior was not trapped"
  ensure (B.take 4 (B.drop (sourceOffset 0x6ac2b8) staged) == bytes 0xe7f000f0) "retired record interior was not trapped"
  ensure (B.take 0x10 (B.drop (sourceOffset 0x62c120) staged) == B.take 0x10 (B.drop (sourceOffset 0x62c120) source)) "property carrier was changed"
  ensure (isLeft (stagePublication contract{stage4Retained=[Region base (base+4)]} plan source linked symbols)) "retained storage overlap was accepted"
  ensure (isLeft (stagePublication contract{stage4Carrier=Region base (base+4)} plan source linked symbols)) "property carrier overlap was accepted"
  ensure (isLeft (stagePublication contract plan (patch source (sourceOffset base) (bytes 0)) linked symbols)) "nonretired carrier was accepted"
  ensure (isLeft (stagePublication contract plan source (patch linked (fromIntegral (0x6a8484-0x61d8dc :: Word32)) (bytes 0xe12fff1e)) symbols)) "invalid custom veneer was accepted"
  ensure (isLeft (stagePublication contract plan source (B.drop 4 linked) symbols)) "truncated sparse binary was accepted"
  ensure (isLeft (stagePublication contract plan source linked (symbols <> C.pack ("008e0084 00000008 A record_store_cache_tags\n")))) "moved cache tags were accepted"
  ensure (isLeft (stagePublication contract plan source linked (symbols <> C.pack ("0089f8f0 00000004 A record_store_cache_tag\n")))) "obsolete single cache tag was accepted"
  ranges <- right (R.parseRanges "range\ta\tb\tc\t6407168\t16\tx\ty\n")
  let scan value = publicationReferences ranges [(ScanImage4, Image "synthetic" "synthetic" 0x61c400 (bytes value))]
  ensure (null (scan 0x6a8484)) "public custom entry reference was refused"
  ensure (not (null (scan 0x6a8488))) "retired custom interior reference escaped closure"
  ensure (not (null (scan 0x89f8f0)) && not (null (scan 0x89f8f4))) "external obsolete-gap reference escaped closure"
  putStrLn "custom publication placement and retirement guards passed"
