module Main where

import Control.Monad (unless)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.GeneralHeap.Admission
import Replacement.GeneralHeap.Artifact
import Replacement.Record5Staging (Record5Refusal (..), Record5Window (..), validateRecord5Confinement)

check :: String -> Bool -> IO ()
check label condition = unless condition (fail label)

accepted :: Either a b -> Bool
accepted (Right _) = True
accepted _ = False

replace :: Word32 -> B.ByteString -> B.ByteString -> B.ByteString
replace address value bytes = let offset = fromIntegral (address-0x89cda4)
  in B.take offset bytes <> value <> B.drop (offset+B.length value) bytes

geometry :: [Word32]
geometry = [0x48454150,1,0xa71a00,0xbf1a00,1152,0xa71e80,32,17,16,16,16,8,32,16]

fixture :: (B.ByteString,B.ByteString)
fixture = (foldl (\bytes (address,value) -> replace address value bytes) empty patches,C.pack (unlines rows))
  where
    empty = B.replicate (0xbf1c48-0x89cda4) 0
    cores = zip ["xm1_heap_" ++ name ++ "_r" | name <- ["malloc","free","realloc","calloc","trim"]] [0x89ce04,0x89ce08..]
    functions = entrySymbols ++ cores
    metadata = 0x89d500
    patches = [(address,U.putWord32 0xe12fff1e) | (_,address) <- cores] ++ adapters ++ [(metadata,B.concat (map U.putWord32 geometry))]
    target operation = case lookup ("xm1_heap_" ++ operation ++ "_r") cores of Just address -> address; Nothing -> error "synthetic operation absent"
    tailWord address destination = U.putWord32 (0xea000000 .|. (((destination-address-8) `shiftR` 2) .&. 0xffffff))
    public core address twoArguments =
      let prefix = [0xe1a02001 | twoArguments] ++ [0xe1a01000,0xe30c0970,0xe3400089,0xe5900000]
      in (address,B.concat (map U.putWord32 prefix) <> tailWord (address+fromIntegral (4*length prefix)) core)
    adapters = [public 0x89cde4 0x89cda4 False,public 0x89e560 0x89cdc4 False,public 0x89da64 0x89da3c True,public 0xbf1c44 0xbf1c1c True]
      ++ [(address,tailWord address (target operation)) | (operation,address) <- [("malloc",0x89cde4),("free",0x89e560),("realloc",0x89da64),("calloc",0xbf1c44),("trim",0x89e464)]]
    row :: String -> Word32 -> Int -> String -> String
    row name address size kind = showHex address "" ++ " " ++ showHex size "" ++ " " ++ kind ++ " " ++ name
    rows = [row name address (4::Int) "T" | (name,address) <- functions]
      ++ [row "__heap_sentinel" 0x89d4f8 (4::Int) "D",row "__heap_link_end" 0xbf1c48 0 "A",row "XM1_HEAP_METADATA" metadata 56 "R"]

main :: IO ()
main = do
  check "stock tag decodes bounded usable bytes" (validateStockHeader 12 17 == Right 12)
  check "missing PREV_INUSE rejected" (validateStockHeader 12 16 == Left HeaderRejected)
  check "wrong low-bit class rejected" (validateStockHeader 12 19 == Left HeaderRejected)
  check "tag exceeding allocation rejected" (validateStockHeader 12 25 == Left HeaderRejected)
  check "underflowing header rejected" (validateStockHeader 12 1 == Left HeaderRejected)
  (_, arena) <- either (fail . show) pure (decodeGeometry geometry)
  layout <- either (fail . show) pure (boundOwnedAllocation arena (HeapAddress 0xa80010) (AllocationBytes 0x3dc80))
  let header = B.concat (map U.putWord32 [0x584d3148, 0x3dc80, 0x335d30a6, 0x3dc89])
  check "owned allocation prefix is derived from its bounded payload" (allocationHeaderAddress layout == HeapAddress 0xa80000)
  check "synthetic32 owned allocation header accepted" (accepted (admitOwnedAllocation layout header))
  check "owned allocation cannot overlap allocator state" (not (accepted (boundOwnedAllocation arena (HeapAddress 0xa71a10) (AllocationBytes 0x3dc80))))
  check "owned allocation cannot reach the arena guard" (not (accepted (boundOwnedAllocation arena (HeapAddress 0xbf19e0) (AllocationBytes 0x3dc80))))
  check "owned allocation rejects an unaligned payload" (not (accepted (boundOwnedAllocation arena (HeapAddress 0xa80011) (AllocationBytes 0x3dc80))))
  check "owned allocation rejects truncated prefix" (admitOwnedAllocation layout (B.take 15 header) == Left HeaderRejected)
  check "owned allocation rejects stale requested extent" (admitOwnedAllocation layout (B.take 4 header <> U.putWord32 0x2dcc0 <> B.drop 8 header) == Left HeaderRejected)
  check "owned allocation rejects corrupted integrity" (admitOwnedAllocation layout (B.take 8 header <> U.putWord32 0 <> B.drop 12 header) == Left HeaderRejected)
  check "owned allocation rejects changed stock tag" (admitOwnedAllocation layout (B.take 12 header <> U.putWord32 0x3dc88) == Left HeaderRejected)
  check "wrong first-level size class rejected" (decodeGeometry (take 7 geometry ++ [16] ++ drop 8 geometry) == Left GeometryRejected)
  check "wrong second-level size class rejected" (decodeGeometry (take 8 geometry ++ [32] ++ drop 9 geometry) == Left GeometryRejected)
  check "wrong stock prefix rejected" (decodeGeometry (take 10 geometry ++ [8] ++ drop 11 geometry) == Left GeometryRejected)
  check "pool overlaps state rejected" (decodeGeometry (take 5 geometry ++ [0xa71a00] ++ drop 6 geometry) == Left GeometryRejected)
  let returnOnly = U.putWord32 0xe12fff1e
  frames <- either (fail . show) pure (deriveFrames returnOnly [0x89cda4])
  check "admitted fetch accepted" (validateFetch frames 0x89cda4 0xe12fff1e 0xe12fff1e == Right ())
  check "retired interior fetch rejected" (validateFetch frames 0x89cda8 0 0 == Left FetchRejected)
  check "changed owned fetch rejected" (validateFetch frames 0x89cda4 0xe12fff1e 0 == Left FetchRejected)
  check "sentinel cannot execute" (not (accepted (deriveFrames (B.replicate 0x758 0) [0x89d4f8])))
  check "forbidden CPU instruction rejected" (not (accepted (deriveFrames (U.putWord32 0xef000000) [0x89cda4])))
  check "unaligned call frame rejected" (not (accepted (deriveFrames (B.concat (map U.putWord32 [0xe24dd004,0xeb000000,0xe28dd004,0xe12fff1e])) [0x89cda4])))
  check "unrestored return frame rejected" (not (accepted (deriveFrames (B.concat (map U.putWord32 [0xe24dd008,0xe12fff1e])) [0x89cda4])))
  let deriveWords words' = deriveFrames (B.concat (map U.putWord32 words')) [0x89cda4]
  check "ordinary dual-register load admitted" (accepted (deriveWords [0xe1c080d0,0xe12fff1e]))
  check "dual-register load cannot overwrite SP" (not (accepted (deriveWords [0xe1c0c0d0,0xe12fff1e])))
  check "dual-register load invalidates restored frame provenance" (not (accepted (deriveWords [0xe28db000,0xe1c0a0d0,0xe24bd000,0xe12fff1e])))
  check "ordinary PC-relative dual-register load admitted" (accepted (deriveWords [0xe1cf80d0,0xe12fff1e,0,0]))
  check "PC-relative dual-register load cannot overwrite SP" (not (accepted (deriveWords [0xe1cfc0d0,0xe12fff1e,0,0])))
  check "PC-relative dual-register load invalidates restored frame provenance" (not (accepted (deriveWords [0xe28db000,0xe1cfa0d8,0xe24bd000,0xe12fff1e,0,0,0])))
  check "call to retained stock memmove admitted" (accepted (deriveWords [0xe92d4010,0xeb0002d6,0xe8bd8010]))
  check "tail branch to retained stock memmove admitted" (accepted (deriveWords [0xea0002d7]))
  check "call outside carriers and retained callees rejected" (not (accepted (deriveWords [0xe92d4010,0xeb0002d7,0xe8bd8010])))
  check "predicated frame-pointer epilogue admitted on its executing paths" (accepted (deriveWords [0xe92d4830,0xe28db008,0xe3500001,0x11a0d00b,0x18bd8800,0xe8bd8830]))
  check "flag write between predicated epilogue instructions rejected" (not (accepted (deriveWords [0xe92d4830,0xe28db008,0xe3500001,0x11a0d00b,0xe3500002,0x18bd8800,0xe8bd8830])))
  check "SP restore from an unknown frame pointer rejected" (not (accepted (deriveWords [0xe92d4830,0xe1a0d00b,0xe8bd8830])))
  let deriveEntries words' = deriveFrames (B.concat (map U.putWord32 words'))
  check "call to a nonreturning entry has no fallthrough" (accepted (deriveEntries [0xe92d4010,0xebffffff,0xeafffffe] [0x89cda4,0x89cdac]))
  check "call to a returning entry falls through" (not (accepted (deriveEntries [0xe92d4010,0xebffffff,0xe12fff1e] [0x89cda4,0x89cdac])))
  check "call to a carrier address that is not an entry rejected" (not (accepted (deriveEntries [0xe92d4010,0xebffffff,0xeafffffe] [0x89cda4])))
  let originalDat = B.replicate 0x600 0
      changeFile offset = B.take offset originalDat <> B.singleton 1 <> B.drop (offset+1) originalDat
      record5 = Record5Window 0x400 0x500
  check "record5-window changes admitted" (validateRecord5Confinement record5 originalDat (changeFile 0x410) == Right ())
  check "main checksum change confined" (validateRecord5Confinement record5 originalDat (changeFile U.mainChecksumOffset) == Right ())
  check "retained plain suffix mutation rejected" (validateRecord5Confinement record5 originalDat (changeFile 0x510) == Left Record5OutsideWindow)
  check "preceding retained bytes mutation rejected" (validateRecord5Confinement record5 originalDat (changeFile 0x3f0) == Left Record5OutsideWindow)
  check "candidate length change rejected" (validateRecord5Confinement record5 originalDat (originalDat <> B.singleton 0) == Left Record5OutsideWindow)
  let (linked,symbols) = fixture
  check "complete synthetic production artifact admitted" (accepted (deriveArtifact linked symbols))
  check "wrong emitted size class rejected" (case deriveArtifact (replace (0x89d500+7*4) (U.putWord32 18) linked) symbols of Left (MetadataRejected GeometryRejected) -> True; _ -> False)
  check "sentinel initialized nonzero rejected" (case deriveArtifact (replace 0x89d4f8 (U.putWord32 1) linked) symbols of Left ExtentRejected -> True; _ -> False)
  check "missing operation entry rejected" (case deriveArtifact linked (C.unlines (drop 1 (C.lines symbols))) of Left SymbolsRejected -> True; _ -> False)
  check "canonical source mismatch rejected" (case canonicalSource B.empty B.empty B.empty of Left SourceMismatch -> True; _ -> False)
  check "contract names exactly the four approved redesign items"
    (map redesignId [minBound..maxBound] == ["checked-calloc-product","oversized-resize-rejected-before-lock","zero-request-minimum-allocation","trim-returns-zero"])
