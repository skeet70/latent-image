{-# LANGUAGE ScopedTypeVariables #-}
module Emulation.NativeCase where
import FujiAF.Host hiding (sha256)
import Emulation.Sha256 (sha256, sha256Hex)
import Data.Aeson (Value(..))
import FujiAF.JSON (decodeStrictIntegers)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Control.Exception
import Control.Monad
import Data.Bits
import Data.Char
import Data.List
import Numeric
import System.Directory
import System.Exit
import System.FilePath

regs :: [String]
regs=map (("r"++).show) [0::Int ..12]++["sp","lr","cpsr"]
-- The public bounded-native-case contract. schemas/native-case.schema.json carries the same
-- numbers; NativeTests.hs holds the schema and the adapter together, and runs the runner at each
-- limit. The runner's stub-event recording capacity (its engine MAX_RECORDED_EVENTS) is a separate,
-- larger engine limit: an execution records far more events than a case declares (e.g., 184 recorded
-- vs. 0 declared), measured over 2738 recipe-bank SD cases with a maximum of 284 events.
-- Largest case measured over the recipe-bank and UI generators and 2,314 generated cases:
-- 1131576 artifact bytes, 1888202 total mapped bytes, 19 artifacts, 16 regions, 210 functions,
-- 12 stubs, 59 expected events, 49 expected memory entries, 525916 expected memory bytes.
sizeBytes :: Int
sizeBytes=2*1024*1024
totalBytes :: Int
totalBytes=4*1024*1024
caseBytes :: Int
caseBytes=16*1024*1024
artifactCount, regionCount, functionCount, stubCount, eventCount, memoryCount :: Int
artifactCount=20
regionCount=16
functionCount=256
stubCount=32
eventCount=128
memoryCount=64
hex :: Integer -> String
hex x=showHex x ""
hexBytes :: B.ByteString -> String
hexBytes = concatMap (\x -> let h=showHex x "" in replicate(2-length h)'0'++h) . B.unpack
address :: Value -> IO Integer
address v = do
 let text=str v
 ensure(length text==10 && take 2 text=="0x" && all (`elem` "0123456789abcdef") (drop 2 text)) "address/register: canonical 32-bit hexadecimal required"
 case readHex(drop 2 text) of [(value,"")]->pure value; _->failH "invalid hexadecimal"
byteString :: Value -> IO B.ByteString
byteString v=do
 let text=str v
 ensure(case v of String _->length text<=2*sizeBytes && even(length text) && all (`elem` "0123456789abcdef") text; _->False) "bytes: lowercase hex pairs required"
 pure(B.pack(go text))
 where go []=[]; go (a:c:rest)=fromIntegral(digitToInt a*16+digitToInt c):go rest; go _=[]
bindings :: Value -> [String] -> IO [(String,Integer)]
bindings v allowed=do
 ensure(case v of Object _->all (`elem` allowed)(keys v); _->False) "unknown register"
 mapM (\k -> (,) k <$> address(get v k)) (keys v)
validId :: Value -> Bool
validId v=case str v of c:rest->c>='a' && c<='z' && length rest<=63 && all(\x->isAsciiLower x || isDigit x || x=='-')rest; _->False
arrayCheck :: Value -> Int -> Int -> String -> IO [Value]
arrayCheck v low high label=do
 ensure(case v of Array _->low<=length(list v) && length(list v)<=high; _->False) label
 pure(list v)

data Region = Region {description::Value,baseAddress::Integer,regionSize::Integer,regionBytes::B.ByteString} deriving Show
regionAt :: [Region] -> Integer -> Integer -> Char -> Bool
regionAt rs base size permission=any(\r->baseAddress r<=base && base+size<=baseAddress r+regionSize r && permission `elem` str(get(description r)"permissions"))rs
loadCase :: FilePath -> IO (Value,[Region],B.ByteString,[(String,Value)])
loadCase input = do
 path<-canonicalizePath input
 raw<-readRegularWith (pure()) False caseBytes path
 c<-either failH pure(decodeStrictIntegers raw)
 fields c "schema_version id origin provenance cpu artifacts entry exit registers regions functions stubs limits expected exclusions" "case"
 void(integer(get c "schema_version")1 1 "schema version")
 ensure(get c "origin" `elem` [s "synthetic",s "stock"]) "origin must be synthetic or stock; a stock case needs an independently known result"
 ensure(validId(get c "id")) "invalid case id"
 ensure(not(null(str(get c "provenance")))) "provenance required"
 fields(get c "cpu") "architecture model mode endian" "cpu"
 ensure(get c "cpu"==obj[("architecture",s "arm"),("model",s "cortex-r5f"),("mode",s "arm"),("endian",s "little")]) "unsupported synthetic CPU profile"
 entry<-address(get c "entry"); exit<-address(get c "exit")
 ensure(entry `mod` 4==0 && exit `mod` 4==0 && entry/=exit) "entry/exit alignment or alias"
 fields(get c "registers") (unwords regs) "registers"
 initial<-bindings(get c "registers")regs
 let reg k=maybe 0 id(lookup k initial)
 ensure(reg "cpsr" .&. 0x0fffffff==0x10) "CPSR profile is user mode ARM, flags only"
 ensure(reg "lr"==exit && reg "sp" `mod` 8==0) "LR must be exit and SP eight-byte aligned"
 fields(get c "limits") "instructions wall_ms" "limits"
 budget<-integer(get(get c "limits")"instructions")1 4000000 "instructions"
 wall<-integer(get(get c "limits")"wall_ms")1 10000 "wall_ms"
 inventory<-arrayCheck(get c "artifacts")0 artifactCount "artifacts limit"
 artifacts<-foldM (\done item->do
   fields item "id path size sha256" "artifact"
   let ident=str(get item "id"); relative=str(get item "path")
   ensure(validId(get item "id") && ident `notElem` map fst done) "artifact id invalid/duplicate"
   ensure(not(null relative) && isRelative relative && ".." `notElem` splitDirectories relative) "artifact path must stay under case directory"
   file<-canonicalizePath(takeDirectory path </> relative)
   ensure(not(isRelative file) && splitDirectories(takeDirectory path) `isPrefixOf` splitDirectories file) "artifact symlink escapes case directory"
   size<-integer(get item "size")1(fromIntegral sizeBytes) "artifact size"
   let digest=str(get item "sha256")
   ensure(length digest==64 && all (`elem` "0123456789abcdef")digest) "artifact SHA256 invalid"
   bytes<-readRegularWith (pure())False sizeBytes file
   ensure(B.length bytes==fromInteger size) "artifact length mismatch"
   ensure(sha256Hex(sha256 bytes)==digest) "artifact digest mismatch"
   pure(done++[(ident,bytes)])) [] inventory
 regionsInput<-arrayCheck(get c "regions")1 regionCount "regions limit"
 regions<-foldM(\done item->do
   fields item "id address size permissions initial" "region"
   ensure(validId(get item "id") && all(\r->get(description r)"id"/=get item "id")done) "region id invalid/duplicate"
   base<-address(get item "address"); size<-integer(get item "size")1(fromIntegral sizeBytes) "region size"
   ensure(base+size<=4294967296) "region address overflow"
   ensure(get item "permissions" `elem` map s ["r","rw","rx"]) "permissions must be r/rw/rx"
   ensure(get item "permissions"/=s "rx" || base+size<=0x7ffffffc) "executable region must end by 0x7ffffffc: PMSAv7 default map is execute-never from 0x80000000"
   ensure(all(\r->base+size<=baseAddress r || baseAddress r+regionSize r<=base)done) "logical regions overlap"
   let initialBytes=get item "initial"
   bytes<-if initialBytes==obj[("zero",b True)] then pure(B.replicate(fromInteger size)0) else do
     fields initialBytes "artifact offset length" "initial"
     original<-maybe(failH "unknown artifact")pure(lookup(str(get initialBytes "artifact"))artifacts)
     offset<-integer(get initialBytes "offset")0(fromIntegral sizeBytes) "artifact offset"
     len<-integer(get initialBytes "length")1 size "artifact length"
     ensure(offset+len<=fromIntegral(B.length original)) "artifact slice exceeds file"
     pure(B.take(fromInteger len)(B.drop(fromInteger offset)original)<>B.replicate(fromInteger(size-len))0)
   ensure(sum(map regionSize done)+size<=fromIntegral totalBytes) "total mapped bytes exceeds host limit"
   pure(done++[Region item base size bytes]))[] regionsInput
 ensure(regionAt regions entry 4 'x' && regionAt regions exit 4 'x') "entry/exit must have explicit executable bytes"
 ensure(reg "sp">=8 && regionAt regions (reg "sp"-8) 8 'w') "stack top lacks writable eight bytes"
 functionsInput<-arrayCheck(get c "functions")1 functionCount "functions limit"
 functions<-foldM(\done item->do
   fields item "address size" "function"
   base<-address(get item "address"); size<-integer(get item "size")4(fromIntegral sizeBytes) "function size"
   ensure(base `mod` 4==0 && size `mod` 4==0 && regionAt regions base size 'x') "function alignment/containment"
   ensure(not(base<=exit && exit<base+size)) "exit is not a function"
   ensure(all(\(x,len)->base+size<=x || x+len<=base)done) "functions overlap"
   pure(done++[(base,size)]))[] functionsInput
 ensure(entry `elem` map fst functions) "entry must start a declared function"
 stubsInput<-arrayCheck(get c "stubs")0 stubCount "stubs limit"
 stubs<-foldM(\done item->do
   fields item "address inputs outputs side_effects" "stub"
   target'<-address(get item "address")
   ensure(target' `mod` 4==0 && target'/=exit && target' `notElem` map(\(t,_,_)->t)done && not(any(\(x,len)->x<=target' && target'<x+len)functions)) "stub address alignment/alias"
   ensure(get item "side_effects"==s "registers-only") "unsupported stub side effects"
   inputs<-bindings(get item "inputs")(take 13 regs); outputs<-bindings(get item "outputs")(take 13 regs)
   ensure(not(null outputs)) "stub must have explicit outputs"
   pure(done++[(target',inputs,outputs)]))[] stubsInput
 let expected=get c "expected"
 fields expected "status registers memory events" "expected"
 ensure(str(get expected "status") `elem` words "returned instruction_budget wall_budget memory_guard fetch_guard unknown_call unsupported_instruction stub_contract") "unsupported expected exit"
 expectedRegs<-bindings(get expected "registers")(regs++["pc"])
 ensure(not(null expectedRegs)) "at least one independently expected register required"
 differences<-arrayCheck(get expected "memory")0 memoryCount "expected memory limit"
 void$foldM(\occupied item->do
   fields item "region offset hex" "expected memory"
   r<-maybe(failH "unknown expected region")pure(find(\r->get(description r)"id"==get item "region")regions)
   ensure('w' `elem` str(get(description r)"permissions")) "expected difference must be writable"
   offset<-integer(get item "offset")0(fromIntegral sizeBytes) "expected offset"
   bytes<-byteString(get item "hex")
   let end=offset+fromIntegral(B.length bytes); ident=get item "region"
   ensure(not(B.null bytes) && end<=regionSize r) "expected memory bounds"
   ensure(all(\(name,lo,hi)->name/=ident || hi<=offset || end<=lo)occupied) "expected memory overlaps"
   pure((ident,offset,end):occupied)) [] differences
 events<-arrayCheck(get expected "events")0 eventCount "event limit"
 forM_ events $ \event->do
  fields event "address inputs outputs" "expected event"
  void(address(get event "address")); void(bindings(get event "inputs")(take 13 regs)); void(bindings(get event "outputs")(take 13 regs))
 exclusions<-arrayCheck(get c "exclusions")1 1000000 "coverage exclusions required"
 ensure(all(not.null.str)exclusions) "coverage exclusions required"
 let rows=["FUJI_NATIVE_V1","cortex-r5f-arm-le",unwords[hex entry,hex exit,show budget,show wall],unwords[hex(reg r)|r<-regs],unwords(map(show.length)[regionsInput,functionsInput,stubsInput])]
     regionRows=[unwords[hex(baseAddress r),hex(regionSize r),case str(get(description r)"permissions")of "r"->"1";"rw"->"3";_->"5",hexBytes(regionBytes r)]|r<-regions]
     functionRows=[unwords[hex x,hex size]|(x,size)<-functions]
     stubRows=concat[unwords[hex t,show(length inputs),show(length outputs)]:[unwords[show(maybe 0 id(elemIndex k regs)),hex v]|bindingGroup<-[inputs,outputs],(k,v)<-bindingGroup]|(t,inputs,outputs)<-stubs]
     wire=BC.pack(unlines(rows++regionRows++functionRows++stubRows))
 let digest=sha256Hex(sha256 raw); wireDigest=sha256Hex(sha256 wire)
 pure(c,regions,wire,[("case_sha256",s digest),("wire_sha256",s wireDigest),("artifacts",arr inventory)])
checkResult :: Value -> [Region] -> Value -> IO [String]
checkResult c regions result=do
 let expected=get c "expected"
     status=["status differs"|get result "status"/=get expected "status"]
     registers=["register "++k++": differs"|k<-keys(get expected "registers"),get(get result "registers")k/=get(get expected "registers")k]
     events=["stub events differ"|get result "events"/=get expected "events"]
     memory=list(get result "memory")
 mem<-if length memory/=length regions then pure["memory region count differs"] else fmap concat$forM(zip regions memory)$ \(r,actual)->do
   desired<-foldM(\bytes change->if get change "region"==get(description r)"id" then do
     value<-byteString(get change "hex")
     let offset=fromInteger(num(get change "offset"))
     pure(B.take offset bytes<>value<>B.drop(offset+B.length value)bytes)
    else pure bytes)(regionBytes r)(list(get expected "memory"))
   pure["memory "++str(get(description r)"id")++": differs from exact allowed final bytes"|get actual "address"/=get(description r)"address" || get actual "bytes"/=s(hexBytes desired)]
 pure(status++registers++events++mem)
runCase :: FilePath -> FilePath -> IO (Int,Value)
runCase runner path = do
 ref<-pure$obj[("schema_version",n 1),("profile",s "unknown")]
 loaded<-try(loadCase path)
 case loaded of
  Left(e::IOException)->pure(1,merge ref[("status",s "invalid_case"),("reason",s(show e))])
  Right(c,regions,wire,identity)->do
   let profile=if get c "origin"==s "stock" then "stock-bounded" else "synthetic-only"
       report=merge ref(identity++[("case_id",get c "id"),("origin",get c "origin"),("profile",s profile),("provenance",get c "provenance"),("exclusions",get c "exclusions")])
   result<-try$do
    executable<-canonicalizePath runner
    hash<-sha256Hex . sha256 <$> readRegularWith(pure())False (256*1024*1024) executable
    (timed,exit,out,err)<-runBounded(fromInteger(num(get(get c "limits")"wall_ms")*1000+2000000))(8*1024*1024) executable [] wire
    let report'=set "runner_sha256" (s hash) report
    if timed then pure(1,merge report'[("status",s "host_timeout"),("stdout",s(BC.unpack out)),("stderr",s(BC.unpack err))]) else do
     let failed=merge report'[("status",s "runner_failure"),("runner_exit",n(case exit of ExitSuccess->0;ExitFailure i->fromIntegral i)),("runner_stdout",s(BC.unpack out)),("stderr",s(BC.unpack err))]
     parsed<-try$do
      value<-jsonRead out
      ensure(case value of Object _->get value "wire_version"==n 1;_->False) "invalid runner output"
      problems<-checkResult c regions value
      ensure(exit==(if get value "status"==s "returned" then ExitSuccess else ExitFailure 1)) "runner status/exit mismatch"
      pure(if null problems then 0 else 1,merge report'[("status",s(if null problems then "passed" else "failed")),("runner",value),("mismatches",arr(map s problems)),("stderr",s(BC.unpack err))])
     case parsed of Left(e::IOException)->pure(1,set "reason" (s(show e)) failed);Right value->pure value
   case result of Left(e::IOException)->pure(1,merge report[("status",s "invalid_case"),("reason",s(show e))]);Right value->pure value
