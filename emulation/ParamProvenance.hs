{-# LANGUAGE OverloadedStrings #-}
module Main where
import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import Control.Monad
import Data.Aeson (Value)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits
import Data.List (find)
import Data.Word
import Numeric
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

newtype Address = Address Word32 deriving (Eq, Show)
data Image = Image Address B.ByteString
data Region = Region Address Int Int B.ByteString
data Function = Function Address Int

a :: Word32 -> Address
a = Address
hexAddress :: Address -> String
hexAddress (Address x) = "0x" ++ replicate (8 - length text) '0' ++ text where text = showHex x ""
wireAddress :: Address -> String
wireAddress (Address x) = showHex x ""
word :: Word32 -> B.ByteString
word x = B.pack [fromIntegral (x `shiftR` k) | k <- [0,8,16,24]]
bytesHex :: B.ByteString -> String
bytesHex = concatMap (\x -> let text = showHex x "" in replicate (2-length text) '0' ++ text) . B.unpack
imageSlice :: Image -> Address -> Int -> B.ByteString
imageSlice (Image (Address base) image) (Address target) size = B.take size (B.drop (fromIntegral (target-base)) image)
patch :: Int -> B.ByteString -> B.ByteString -> B.ByteString
patch offset value original = B.take offset original <> value <> B.drop (offset+B.length value) original
codeRegion :: Image -> [Function] -> Region
codeRegion image functions = Region (Address start) (fromIntegral(end-start)) 5 (imageSlice image (Address start) (fromIntegral(end-start)))
 where starts=[base|Function (Address base) _<-functions]
       ends=[base+fromIntegral size|Function (Address base) size<-functions]
       start=minimum starts; end=maximum ends
imageBase :: Image -> Word32
imageBase (Image (Address base) _)=base
regionWire :: Region -> String
regionWire (Region base size permission bytes) = unwords [wireAddress base,showHex size "",show permission,bytesHex bytes]
functionWire :: Function -> String
functionWire (Function base size) = unwords [wireAddress base,showHex size ""]
regionDescription :: Region -> Value
regionDescription (Region base size permission bytes) = obj [("address",s(hexAddress base)),("size",n(fromIntegral size)),("permissions",n(fromIntegral permission)),("hex",s(bytesHex bytes))]
readPinned :: FilePath -> Address -> String -> IO (Image,Value)
readPinned path base expected = do
 bytes <- B.readFile path
 digest <- sha256 bytes
 ensure (digest==expected) (path++": pinned input hash differs")
 pure (Image base bytes,obj[("path",s path),("base",s(hexAddress base)),("sha256",s digest),("size",n(fromIntegral(B.length bytes)))])

main :: IO ()
main = do
 [runner,out] <- getArgs
 createDirectoryIfMissing True out
 (id4,id4Identity) <- readPinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (a 0x61c400) "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
 (loader,loaderIdentity) <- readPinned "private-data/analysis/x-m1-1.50/decode/loader-40000.bin" (a 0x40000) "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
 (id5,id5Identity) <- readPinned "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin" (a 0xbf1a00) "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
 (bss,bssIdentity) <- readPinned "private-data/sessions/20261008T045420Z-dump-ram-id4-bss-0/id4-bss-0.bin" (a 0x89ec00) "f653449d39e65de76768aeeb69c5abc89309ed3b6503c5da6144aad0a0aa0fad"
 runnerDigest <- B.readFile runner >>= sha256
 let imported = Function (a 0x74e284) 0xbc
     exported = Function (a 0x752928) 0x68
     leaf = Function (a 0x75ae78) 0x14
     copy = Function (a 0x4357c) 0x30
     helpers = [Function (a 0x75a8c0) 0x50,Function (a 0x75a910) 0x50,Function (a 0x75aab4) 0x48,Function (a 0x75a984) 0x18,Function (a 0x75a99c) 0x18,Function (a 0x75a9b4) 0x18,Function (a 0x75a9cc) 0x18,Function (a 0x75e4f0) 0x28,Function (a 0x75e564) 0x64,Function (a 0x75e5c8) 0x50,Function (a 0x75e618) 0x30,Function (a 0x75e1ec) 0x20]
     source = a 0x015b5cec
     processor = a 0x0096ed3c
     sink = a 0x30000000
     exit = a 0x3c000000
     tagged seed film = patch 0x9c (word film) (B.concat [word (seed+fromIntegral i) | i <- [0::Int ..41]])
     initial = [Region source 0xa8 3 (tagged 0xa1000000 0xb),Region processor 0xa8 3 (tagged 0xb2000000 0xc),Region sink 0xa8 3 (tagged 0xc3000000 0x13),Region (a 0x00915d40) 0x100 3 (B.replicate 0x100 0),Region (a 0x3b000000) 0x400 3 (B.replicate 0x400 0),Region exit 4 5 (B.replicate 4 0)]
     serviceState = foldr (uncurry patch) (B.replicate 0x20000 0) [(0xa0c4,word 0x014e4410),(0xa0b4,word 0xd5000001),(0xa0b8,word 0xd5000002),(0xa0bc,word 0xd5000003),(0xa0c0,word 0xd5000004),(0xa10c,word 0xd5000005),(0xa110,word 0xd5000006)]
     serviceRegions = [Region (a 0x01526000) 0x20000 3 serviceState,Region (a 0x014e4410) 1000 3 (B.concat[word(0xa4000000+fromIntegral i)|i<-[0::Int ..249]]),Region (a 0x00916000) 0x1000 3 (imageSlice bss (a 0x00916000) 0x1000),Region (a 0x00e3a400) 0x6c00 1 (imageSlice id5 (a 0x00e3a400) 0x6c00)]
     exclusions = arr(map s ["emulation: exploratory stock instructions on configured input state, not a service session or accepted stock case", "No scheduler, interrupts, MMIO, DMA, sensor, ISP, caches, silicon timing or next-frame handle binding", "No stubs; the admitted instruction set is the one recorded in emulation/README.md, and an unsupported instruction stops before execution", "Canonical source, processor slot and sink addresses are experiment inputs, not a proved next-capture ownership relation", "Snapshot timing beyond the reached code remains unproved"])
     cases = [("processor-import",imported,0x00fe0000,0x015b5cec,0,"returned",0x3c000000,helpers),
              ("processor-export",exported,0x30000000,0x00fe0000,0,"returned",0x3c000000,helpers),
              ("processor-export-fallback",exported,0x30000000,0xffffffff,0,"returned",0x3c000000,[Function(a 0x75a910)0x50,Function(a 0x75e1ec)0x20]),
              ("canonical-copy-control",leaf,0x0096ed3c,0x015b5cec,0,"returned",0x3c000000,[]),
              ("service-flat-handoff",Function(a 0x00df0030)0xe4,0x42,1,0,"memory_guard",0x00dcd5b8,[Function(a 0x0072daac)0x20,Function(a 0x00dfb3e0)0x158,Function(a 0x00df74ec)0xdc,Function(a 0x00e32328)0x18,Function(a 0x00df638c)0x24,Function(a 0x0072dafc)0x20,Function(a 0x00df0114)0x15c,Function(a 0x007221c8)0x50,Function(a 0x007221a0)0x28,Function(a 0x00df054c)0xb54,Function(a 0x00e2322c)0x64,Function(a 0x00dcd59c)0x5c,Function(a 0x00dcc268)0x58,Function(a 0x0072f2f8)0x14,Function(a 0x00dcb6c0)0x3c,Function(a 0x0072f260)0x24])]
 forM_ cases $ \(name,entry@(Function entryAddress _),r0,r1,r2,status,pc,closure) -> do
  let service = name=="service-flat-handoff"
      functions = entry:closure++[leaf | not service && entryAddress/=a 0x75ae78]++[copy | not service]
      functionImage (Function (Address address) _) = if address>=0xbf1a00 then id5 else if address<0x50000 then loader else id4
      located = [(imageBase(functionImage function),function)|function<-functions]
      code = [codeRegion image [function|(base',function)<-located,base'==base]|(base,image)<-[(0x61c400,id4),(0xbf1a00,id5),(0x40000,loader)],any(\(base',_)->base'==base)located]
      regions = code++initial++[region|service,region<-serviceRegions]
      registers = [r0,r1,r2]++replicate 10 0++[0x3b000400,0x3c000000,0x10]
      wire = BC.pack(unlines(["FUJI_NATIVE_V1","cortex-r5f-arm-le",unwords[wireAddress entryAddress,wireAddress exit,"10000","1000"],unwords(map(\x->showHex x "")registers),unwords(map show[length regions,length functions,0::Int])]++map regionWire regions++map functionWire functions))
      input = obj[("classification",s "emulation"),("id",s name),("entry",s(hexAddress entryAddress)),("registers",arr(map(s.hexAddress.a)registers)),("regions",arr(map regionDescription regions)),("functions",arr[obj[("address",s(hexAddress address)),("size",n(fromIntegral size))]|Function address size<-functions]),("artifacts",arr[id4Identity,id5Identity,loaderIdentity,bssIdentity]),("exclusions",exclusions),("expected_stop",obj[("status",s status),("pc",s(hexAddress(a pc)))])]
  B.writeFile(out </> name <.> "input.json")(encodeCanonical input)
  B.writeFile(out </> name <.> "wire")wire
  (timed,exitCode,raw,err) <- runBounded 3000000 (8*1024*1024) runner [] wire
  ensure (not timed) (name++": host timeout")
  result <- jsonRead raw
  let report = obj[("classification",s "emulation"),("id",s name),("runner_sha256",s runnerDigest),("input",input),("runner",result),("stderr",s(BC.unpack err))]
  B.writeFile(out </> name <.> "result.json")(encodeCanonical report)
  ensure (get result "status"==s status && get(get result "registers")"pc"==s(hexAddress(a pc))) (name++": unexpected stop; result retained")
  ensure (exitCode==(if status=="returned" then ExitSuccess else ExitFailure 1)) (name++": runner exit disagrees")
  let records=list(get result "memory_provenance")
  ensure (fromIntegral(length records)==num(get result "memory_provenance_total")) (name++": provenance truncated")
  when service $ do
   let reachesDrive p = get p "kind"==s "write" && get p "address"==s "0x0152d698" && get p "pc"==s "0x00df00d4" && get p "value"==s "0x00000000a400001b"
   ensure (any reachesDrive records) "service flat +0x6c provenance absent"
  when(name=="canonical-copy-control") $ do
   forM_ [0::Int ..3] $ \offset -> do
    let matching kind address = find(\p->get p "kind"==s kind && get p "address"==s(hexAddress(a(address+fromIntegral offset))))records
    sourceRead <- maybe (failH "copy source read absent") pure (matching "read" 0x015b5d88)
    targetWrite <- maybe (failH "copy target write absent") pure (matching "write" 0x0096edd8)
    let expectedValue=if offset==0 then s "0x000000000000000b" else s "0x0000000000000000"
    ensure (get sourceRead "value"==expectedValue && get targetWrite "value"==expectedValue && get sourceRead "pc"==s "0x00043590" && get targetWrite "pc"==s "0x00043594") "copy provenance differs"
  putStrLn(name++": "++status++" at "++hexAddress(a pc)++", "++show(length records)++" provenance records")
