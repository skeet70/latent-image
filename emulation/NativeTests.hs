{-# LANGUAGE OverloadedStrings,ScopedTypeVariables #-}
module Main where
import FujiAF.Host
import Emulation.NativeCase
import Control.Exception (SomeException, try)
import Control.Monad
import Data.Aeson(Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits
import Data.Word
import Data.Char(toLower)
import Numeric(showHex)
import FujiAF.JSON(encodeCanonical)
import Data.List
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
import System.Posix.Files

modifyAt :: String -> Int -> (Value -> Value) -> Value -> Value
modifyAt key index change c=set key (arr[if i==index then change v else v|(i,v)<-zip[0::Int ..](list(get c key))])c
remove :: String -> Value -> Value
remove key(Object x)=Object(KM.delete(K.fromString key)x)
remove _ x=x
data DispatchForm = PcTable | RegisterTable deriving (Eq,Show)
data DispatchControl = DeclaredTarget | InteriorTarget | LocalTarget | ConditionFalse | ThumbTarget | UnalignedTarget | UnknownTarget | ShortFunction | UnreadableTable | OverflowTable deriving (Eq,Show)

dispatchRegressions :: FilePath -> IO ()
dispatchRegressions runner=withTemporary "fuji-dispatch-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 let hex value=s("0x"++replicate(8-length text)'0'++text) where text=showHex(value::Word32)""
     fn address size=obj[("address",hex address),("size",n size)]
     region name address size permissions initial=obj[("id",s name),("address",hex address),("size",n size),("permissions",s permissions),("initial",initial)]
     controls=[DeclaredTarget,InteriorTarget,LocalTarget,ConditionFalse,ThumbTarget,UnalignedTarget,UnknownTarget,ShortFunction,UnreadableTable,OverflowTable]
 -- A PC-relative index shift wraps modulo 2^32 inside the table, so only the register base overflows.
 forM_[(form,control)|form<-[PcTable,RegisterTable],control<-controls,(form,control)/=(PcTable,OverflowTable)]$ \(form,control)->do
  let name=map toLower(show form++"-"++show control)
      target=case control of
       InteriorTarget->0x1044
       LocalTarget->0x1024
       ThumbTarget->0x1041
       UnalignedTarget->0x1042
       UnknownTarget->0x1048
       ShortFunction->0x1044
       _->0x1040
      instruction=case form of PcTable->0x979ff100;RegisterTable->0x9792f103
      words'=[0xe1a0400e,0xeb000004,0xe2800002,0xe1a0e004,0xe12fff1e,0,0, instruction,0xe12fff1e,target,0,0,0,0,0,0xe3a00007,0xe12fff1e,0xe12fff1e]::[Word32]
      bytes=B.pack[fromIntegral(word `shiftR` offset)|word<-words',offset<-[0,8,16,24]]
      artifactName=name<.>"bin"
      tableBase=if control `elem` [UnreadableTable,ConditionFalse] then 0x2000 else if control==OverflowTable then 0xfffffffc else 0x1028
      index=if control==OverflowTable || control==ConditionFalse then (case form of PcTable->0x40000000;RegisterTable->1) else if control==UnreadableTable && form==PcTable then 0x400 else 0
      status=case control of
       ThumbTarget->"unsupported_instruction"
       UnalignedTarget->"unsupported_instruction"
       UnknownTarget->"unknown_call"
       ShortFunction->"unknown_call"
       UnreadableTable->"memory_guard"
       OverflowTable->"memory_guard"
       _->"returned"
      returned=status=="returned"
      result=if control==DeclaredTarget then 9 else if control==ConditionFalse && form==PcTable then 0x40000002 else 2
  B.writeFile(root </> artifactName)bytes
  hash<-sha256 bytes
  let art=obj[("id",s "dispatch"),("path",s artifactName),("size",n(fromIntegral(B.length bytes))),("sha256",s hash)]
      registers=merge(get base "registers")[("r0",hex(if form==PcTable then index else 0)),("r2",hex tableBase),("r3",hex index),("cpsr",hex(if control==ConditionFalse then 0x20000010 else 0x10))]
      fixture=merge base[("id",s name),("provenance",s "NativeTests.hs: hand-encoded A32 dispatch, caller adds two after preserved-frame return"),("artifacts",arr[art]),("registers",registers),("regions",arr[region "code" 0x1004(fromIntegral(B.length bytes))"rx"(obj[("artifact",s "dispatch"),("offset",n 0),("length",n(fromIntegral(B.length bytes)))]),region "exit" 0x1080 4 "rx"(obj[("zero",b True)]),region "stack" 0x3000 32 "rw"(obj[("zero",b True)])]),("functions",arr[fn 0x1004 20,fn 0x1020 8,fn 0x1040(if control==ShortFunction then 4 else 8)]),("expected",obj[("status",s status),("registers",if returned then obj[("r0",hex result),("lr",hex 0x1080),("pc",hex 0x1080)] else obj[("r4",hex 0x1080)]),("memory",arr[]),("events",arr[])])]
      path=root </> name<.>"json"
  B.writeFile path(encodeCanonical fixture)
  (code,report)<-runCase runner path
  ensure(code==0 && get report "status"==s "passed")(name++": "++show report)
-- A case declaring functionCount functions runs; one more is an invalid case.
functionLimitRegressions :: FilePath -> IO ()
functionLimitRegressions runner=withTemporary "fuji-function-limit-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 copyFile "emulation/fixtures/arithmetic.bin"(root </> "arithmetic.bin")
 let hex value=s("0x"++replicate(8-length text)'0'++text) where text=showHex(value::Word32)""
     declared=list(get base "functions")
     paddingBase=0x10000
     padding=obj[("id",s "padding"),("address",hex paddingBase),("size",n(fromIntegral(4*functionCount))),("permissions",s "rx"),("initial",obj[("zero",b True)])]
     fixture count=merge base[("regions",arr(list(get base "regions")++[padding])),("functions",arr(declared++[obj[("address",hex(paddingBase+4*fromIntegral i)),("size",n 4)]|i<-[0..count-1]]))]
     path=root </> "function-limit.json"
 forM_[(functionCount-length declared,0,"passed"),(functionCount-length declared+1,1,"invalid_case")]$ \(count,expected,status)->do
  B.writeFile path(encodeCanonical(fixture count))
  (code,report)<-runCase runner path
  ensure(code==expected && get report "status"==s status)("function limit with "++show(count+length declared)++" functions: "++show report)

instructionLimitRegressions :: FilePath -> IO ()
instructionLimitRegressions runner=withTemporary "fuji-instruction-limit-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 copyFile "emulation/fixtures/arithmetic.bin"(root </> "arithmetic.bin")
 let path=root </> "instruction-limit.json"
 forM_[(4000000,0,"passed"),(4000001,1,"invalid_case")]$ \(budget,expected,status)->do
  B.writeFile path(encodeCanonical(set "limits" (obj[("instructions",n budget),("wall_ms",n 1000)])base))
  (code,report)<-runCase runner path
  ensure(code==expected && get report "status"==s status)("instruction limit "++show budget++": "++show report)
-- Each public contract limit admits a case at the limit and refuses one past it.
-- The size, total and JSON boundaries are adapter admissions; the counts that
-- stay small also run to a passing result through the engine.
contractLimitRegressions :: FilePath -> IO ()
contractLimitRegressions runner=withTemporary "fuji-contract-limit-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 copyFile "emulation/fixtures/arithmetic.bin"(root </> "arithmetic.bin")
 let hex value=s("0x"++replicate(8-length text)'0'++text) where text=showHex(value::Word32)""
     pad name address size=obj[("id",s name),("address",hex address),("size",n size),("permissions",s "rw"),("initial",obj[("zero",b True)])]
     art name file size digest=obj[("id",s name),("path",s file),("size",n size),("sha256",s digest)]
     baseBytes=sum[num(get r "size")|r<-list(get base "regions")]
     path=root </> "case.json"
     write item=B.writeFile path(encodeCanonical item)
     admits=do
       result<-try(loadCase path)
       case result of
        Left (_::SomeException)->pure False
        Right _->pure True
     boundary label at past=do
       write at
       yes<-admits
       ensure yes(label++" at the limit was refused")
       write past
       no<-admits
       ensure(not no)(label++" past the limit was admitted")
     running label at past=do
       write at
       (code,report)<-runCase runner path
       ensure(code==0 && get report "status"==s "passed")(label++" at the limit: "++show report)
       write past
       no<-admits
       ensure(not no)(label++" past the limit was admitted")
 big<-sha256(B.replicate sizeBytes 0x5a)
 bigPlus<-sha256(B.replicate (sizeBytes+1) 0x5a)
 B.writeFile(root </> "big.bin")(B.replicate sizeBytes 0x5a)
 B.writeFile(root </> "big-plus.bin")(B.replicate (sizeBytes+1) 0x5a)
 let artsOf xs=arr(list(get base "artifacts")++xs)
 running "artifact size"
   (merge base[("artifacts",artsOf[art "big" "big.bin" (toInteger sizeBytes) big])])
   (merge base[("artifacts",artsOf[art "big" "big-plus.bin" (toInteger(sizeBytes+1)) bigPlus])])
 let one=toInteger sizeBytes
     two=toInteger totalBytes-baseBytes-one
     total extra=merge base[("regions",arr(list(get base "regions")++extra))]
 boundary "total mapped bytes"
   (total[pad "pad-a" 0x00100000 one,pad "pad-b" 0x00300000 two])
   (total[pad "pad-a" 0x00100000 one,pad "pad-b" 0x00300000 (two+4)])
 extras<-forM [length(list(get base "artifacts")) .. artifactCount-1]$ \i->do
   let name="pad"++show i<.>"bin"
       bytes=B.pack[fromIntegral i]
   B.writeFile(root </> name)bytes
   digest<-sha256 bytes
   pure(art ("pad"++show i) name 1 digest)
 let moreBytes=B.pack[0xff::Word8]
 B.writeFile(root </> "one-more.bin")moreBytes
 more<-sha256 moreBytes
 running "artifact count"
   (merge base[("artifacts",arr(list(get base "artifacts")++extras))])
   (merge base[("artifacts",arr(list(get base "artifacts")++extras++[art "one-more" "one-more.bin" 1 more]))])
 let event i=obj[("address",hex(fromIntegral(0x5000+4*i))),("inputs",obj[]),("outputs",obj[("r0",hex 0)])]
     expecting count=merge base[("expected",set "events"(arr(map event [0::Int .. count-1]))(get base "expected"))]
 boundary "expected events" (expecting eventCount) (expecting(eventCount+1))
 let stub i=obj[("address",hex(fromIntegral(0x6000+4*i))),("inputs",obj[]),("outputs",obj[("r0",hex 0)]),("side_effects",s "registers-only")]
     declaring count=merge base[("stubs",arr(map stub [0::Int .. count-1]))]
 running "declared stubs" (declaring stubCount) (declaring(stubCount+1))
 let stripped=set "provenance"(s "")base
     fixed=B.length(encodeCanonical stripped)
     provenance bytes=set "provenance"(s(replicate bytes 'x'))stripped
 boundary "case JSON bytes" (provenance(caseBytes-fixed)) (provenance(caseBytes-fixed+1))

-- A read-only region admits an expected byte that equals its initial value, refuses an
-- expected change, and stops a store as memory_guard.
readOnlyRegressions :: FilePath -> IO ()
readOnlyRegressions runner=withTemporary "fuji-read-only-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 copyFile "emulation/fixtures/arithmetic.bin"(root </> "arithmetic.bin")
 let hex value=s("0x"++replicate(8-length text)'0'++text) where text=showHex(value::Word32)""
     roRegion=obj[("id",s "ro"),("address",hex 0x7000),("size",n 4),("permissions",s "r"),("initial",obj[("zero",b True)])]
     withRo=merge base[("regions",arr(list(get base "regions")++[roRegion]))]
     observed value=merge withRo[("expected",set "memory"(arr(list(get(get withRo "expected")"memory")++[obj[("region",s "ro"),("offset",n 0),("hex",s value)]]))(get withRo "expected"))]
     path=root </> "read-only.json"
 forM_[(observed "00000000",0,"passed"),(observed "01000000",1,"invalid_case")]$ \(fixture,expected,status)->do
  B.writeFile path(encodeCanonical fixture)
  (code,report)<-runCase runner path
  ensure(code==expected && get report "status"==s status)("read-only expectation "++status++": "++show report)
 let words'=[0xe3a00001,0xe5810000,0xe12fff1e]::[Word32]
     bytes=B.pack[fromIntegral(word `shiftR` offset)|word<-words',offset<-[0,8,16,24]]
 B.writeFile(root </> "write-ro.bin")bytes
 digest<-sha256 bytes
 let codeSize=fromIntegral(B.length bytes)::Integer
     artifact=obj[("id",s "write-ro"),("path",s "write-ro.bin"),("size",n codeSize),("sha256",s digest)]
     codeRegion=obj[("id",s "code"),("address",hex 0x8000),("size",n codeSize),("permissions",s "rx"),("initial",obj[("artifact",s "write-ro"),("offset",n 0),("length",n codeSize)])]
     roWriteRegion=obj[("id",s "ro"),("address",hex 0x9000),("size",n 4),("permissions",s "r"),("initial",obj[("zero",b True)])]
     exitRegion=obj[("id",s "exit"),("address",hex 0xa000),("size",n 4),("permissions",s "rx"),("initial",obj[("zero",b True)])]
     stackRegion=obj[("id",s "stack"),("address",hex 0xb000),("size",n 32),("permissions",s "rw"),("initial",obj[("zero",b True)])]
     registers=merge(obj([("r"++show i,hex 0)|i<-[0 .. 12 :: Int]]++[("sp",hex 0xb010),("lr",hex 0xa000),("cpsr",hex 0x10)]))[("r1",hex 0x9000)]
     fixture=obj
       [("schema_version",n 1),("id",s "read-only-write"),("origin",s "synthetic")
       ,("provenance",s "NativeTests.hs: store r0 into a read-only region")
       ,("cpu",get base "cpu"),("artifacts",arr[artifact])
       ,("entry",hex 0x8000),("exit",hex 0xa000),("registers",registers)
       ,("regions",arr[codeRegion,roWriteRegion,exitRegion,stackRegion])
       ,("functions",arr[obj[("address",hex 0x8000),("size",n codeSize)]])
       ,("stubs",arr[])
       ,("limits",obj[("instructions",n 1000),("wall_ms",n 1000)])
       ,("expected",obj[("status",s "memory_guard"),("registers",obj[("r0",hex 1)]),("memory",arr[]),("events",arr[])])
       ,("exclusions",arr[s "read-only write guard"])]
 B.writeFile path(encodeCanonical fixture)
 (code,report)<-runCase runner path
 ensure(code==0 && get report "status"==s "passed" && get(get report "runner")"status"==s "memory_guard")("store to a read-only region: "++show report)

-- The schema carries the same bounds as the adapter; drift fails here.
schemaContract :: IO ()
schemaContract=do
 schema<-B.readFile "schemas/native-case.schema.json" >>= jsonRead
 let props=get schema "properties"
     defs=get schema "$defs"
     item=get(get(get(get props "regions")"items")"properties")"initial"
     memory=get(get(get(get props "expected")"properties")"memory")"items"
     expect label actual wanted=ensure(num actual==wanted)(label++" differs from the adapter: "++show actual)
 sliced<-case[o|o<-list(get item "oneOf"),"offset"`elem`keys(get o "properties")] of
   [only]->pure only
   forms->failH("initial must carry exactly one artifact-slice form, saw "++show(length forms))
 expect "$defs/size maximum"(get(get defs "size")"maximum")(toInteger sizeBytes)
 expect "artifacts maxItems"(get(get props "artifacts")"maxItems")(toInteger artifactCount)
 expect "regions maxItems"(get(get props "regions")"maxItems")(toInteger regionCount)
 expect "functions maxItems"(get(get props "functions")"maxItems")(toInteger functionCount)
 expect "stubs maxItems"(get(get props "stubs")"maxItems")(toInteger stubCount)
 expect "expected memory maxItems"(get(get(get(get props "expected")"properties")"memory")"maxItems")(toInteger memoryCount)
 expect "expected events maxItems"(get(get(get(get props "expected")"properties")"events")"maxItems")(toInteger eventCount)
 expect "instructions maximum"(get(get(get(get props "limits")"properties")"instructions")"maximum") 4000000
 expect "wall_ms maximum"(get(get(get(get props "limits")"properties")"wall_ms")"maximum") 10000
 expect "initial offset maximum"(get(get(get sliced "properties")"offset")"maximum")(toInteger sizeBytes)
 expect "expected offset maximum"(get(get(get memory "properties")"offset")"maximum")(toInteger sizeBytes)
 expect "expected hex maxLength"(get(get(get memory "properties")"hex")"maxLength")(toInteger(2*sizeBytes))

-- The recording capacity (MAX_RECORDED_EVENTS 512) is a separate engine limit from the 128 public
-- expected-events bound: an execution records far more stub events than a case declares (the
-- 6a24474 regression lowered this to 128, breaking cases that recorded more). A case recording
-- more than 128 events still runs, and the 512th recorded event is admitted while the 513th is
-- refused as stub_contract.
recordingLimitRegressions :: FilePath -> IO ()
recordingLimitRegressions runner=withTemporary "fuji-recording-limit-tests"$ \root->do
 base<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
 let hex value=s("0x"++replicate(8-length text)'0'++text) where text=showHex(value::Word32)""
     bl a=0xeb000000 .|. ((0x4000-a-8)`div`4 .&. 0xffffff)
     enc k=B.pack[fromIntegral(word`shiftR`offset)|word<-[0xe1a0400e]++[bl(0x1008+fromIntegral i*4)|i<-[0::Int .. k-1]]++[0xe1a0e004,0xe12fff1e]::[Word32],offset<-[0,8,16,24]]
 forM_[(129,"returned",129),(512,"returned",512),(513,"stub_contract",512)]$ \(k,statusWanted,eventWanted)->do
  let bytes=enc k
  digest<-sha256 bytes
  let codeSize=fromIntegral(B.length bytes)::Integer
      exit=fromIntegral 0x1004+codeSize
      exitH=hex(fromIntegral exit::Word32)
      codeRegion=obj[("id",s "code"),("address",hex 0x1004),("size",n codeSize),("permissions",s "rx"),("initial",obj[("artifact",s "rec"),("offset",n 0),("length",n codeSize)])]
      exitRegion=obj[("id",s "exit"),("address",exitH),("size",n 4),("permissions",s "rx"),("initial",obj[("zero",b True)])]
      stackRegion=obj[("id",s "stack"),("address",hex 0x3000),("size",n 32),("permissions",s "rw"),("initial",obj[("zero",b True)])]
      artifact=obj[("id",s "rec"),("path",s "rec.bin"),("size",n codeSize),("sha256",s digest)]
      registers=merge(get base "registers")[("r4",exitH),("lr",exitH)]
      expected=obj[("status",s "returned"),("registers",obj[("r0",hex 0),("r4",exitH),("lr",exitH),("pc",exitH)]),("memory",arr[]),("events",arr[])]
      stub=obj[("address",hex 0x4000),("inputs",obj[]),("outputs",obj[("r0",hex 0)]),("side_effects",s "registers-only")]
      caseJSON=merge base[("artifacts",arr[artifact]),("entry",hex 0x1004),("exit",exitH),("registers",registers),("regions",arr[codeRegion,exitRegion,stackRegion]),("functions",arr[obj[("address",hex 0x1004),("size",n codeSize)]]),("stubs",arr[stub]),("limits",obj[("instructions",n 100000),("wall_ms",n 10000)]),("expected",expected)]
      path=root </> "rec.json"
  B.writeFile(root </> "rec.bin")bytes
  B.writeFile path(encodeCanonical caseJSON)
  (code,report)<-runCase runner path
  let runnerResult=get report "runner"
  ensure(get runnerResult "status"==s statusWanted)("recording capacity "++show k++" status: "++show report)
  ensure(length(list(get runnerResult "events"))==eventWanted)("recording capacity "++show k++" events: "++show report)

main :: IO ()
main=do
 [runner,generator]<-getArgs
 names<-sort.filter((==".json").takeExtension) <$> listDirectory "emulation/fixtures"
 forM_ names$ \name->do
  let path="emulation/fixtures" </> name
  (code,report)<-runCase runner path
  ensure(code==0 && get report "status"==s "passed")(name++": "++show report)
  fixture<-canonicalizePath path >>= readRegularWith(pure())False caseBytes >>= jsonRead
  let result=get report "runner"
  ensure(num(get result "instructions")<=num(get(get fixture "limits")"instructions"))"instruction bound"
  ensure(length(list(get result "trace"))<=64 && length(list(get result "accesses"))<=64)"trace cap"
  let provenance=list(get result "memory_provenance")
  ensure(length provenance<=4096 && fromIntegral(length provenance)<=num(get result "memory_provenance_total"))"provenance cap"
  when(name=="register-offset-word.json")$do
   ensure(map(\p->(get p "kind",get p "address",get p "size",get p "value"))provenance==
    [(s "write",s "0x00002008",n 4,s "0x0000000012345678"),(s "read",s "0x00002008",n 4,s "0x0000000012345678")])"memory provenance values and ordering"
   ensure(map(\p->get p "pc")provenance==map s["0x00001004","0x00001008"])"memory provenance instruction sites"
  when(name=="arithmetic.json")$ensure(get result "instructions"==s "3")"arithmetic instruction count"
  when(name=="instruction-budget.json")$ensure(get result "instructions"==s "3" && get result "trace"==arr(replicate 3(obj[("pc",s "0x00001004"),("instruction",s "0xeafffffe")])) )"exact budget trace"
  when(name=="wall-budget.json")$ensure(num(get result "elapsed_ns")<1000000000)"wall bound"
 dispatchRegressions runner
 functionLimitRegressions runner
 instructionLimitRegressions runner
 recordingLimitRegressions runner
 contractLimitRegressions runner
 readOnlyRegressions runner
 schemaContract
 withTemporary "fuji-native-tests"$ \root->do
  artifacts<-filter((==".bin").takeExtension) <$> listDirectory "emulation/fixtures"
  forM_ artifacts$ \name->copyFile("emulation/fixtures" </> name)(root </> name)
  let generated=root </> "generated"
  (generatorTimeout,generatorCode,_,generatorError)<-runBounded 15000000 8192 generator ["--output",generated] B.empty
  ensure(not generatorTimeout && generatorCode==ExitSuccess)("fixture generator failed: "++BC.unpack generatorError)
  generatedNames<-sort.filter(\name->takeExtension name `elem` [".json",".bin"]) <$> listDirectory generated
  ensure(generatedNames==sort(names++artifacts))"generated fixtures must all be checked in and exercised"
  forM_ (names++artifacts)$ \name->do
   originalBytes<-B.readFile("emulation/fixtures" </> name)
   generatedBytes<-B.readFile(generated </> name)
   if takeExtension name==".json" then do
    old<-jsonRead originalBytes; new<-jsonRead generatedBytes
    ensure(old==new)(name++": generated case semantic drift")
   else ensure(originalBytes==generatedBytes)(name++": generated binary drift")
  original<-B.readFile "emulation/fixtures/arithmetic.json" >>= jsonRead
  let path=root </> "case.json"
      nest key change c=set key(change(get c key))c
      region i=modifyAt "regions" i
      artifact=modifyAt "artifacts" 0
      highCode size=nest "regions"(\v->arr(list v++[obj[("id",s "high"),("address",s "0x7ffffff8"),("size",n size),("permissions",s "rx"),("initial",obj[("zero",b True)])]]))
      mutations=[("version",set "schema_version"(n 2)),("extra",set "unreviewed"(b True)),("unknown origin",set "origin"(s "firmware")),("retired CPU",nest "cpu"(set "model"(s "arm926"))),("Thumb",nest "cpu"(set "mode"(s "thumb"))),("zero instructions",nest "limits"(set "instructions"(n 0))),("zero wall",nest "limits"(set "wall_ms"(n 0))),("boolean length",region 0(set "size"(b True))),("integer zero tag",region 1(set "initial"(obj[("zero",n 1)]))),("overflow",region 2(\v->merge v[("address",s "0xfffffffe"),("size",n 4)])),("overlap",region 2(set "address"(s "0x00001004"))),("RWX",region 0(set "permissions"(s "rwx"))),("misaligned entry",set "entry"(s "0x00001005")),("unbounded function",modifyAt "functions" 0(set "size"(n 16))),("unknown register",nest "registers"(set "sctlr"(s "0x00000000"))),("missing state",nest "registers"(remove "cpsr")),("unsupported flags",nest "registers"(set "cpsr"(s "0x00000030"))),("stack alignment",nest "registers"(set "sp"(s "0x0000301c"))),("stack bounds",nest "registers"(set "sp"(s "0x00004000"))),("wrong hash",artifact(set "sha256"(s(replicate 64 '0')))),("slice overflow",region 0(nest "initial"(set "offset"(n 1)))),("parent escape",artifact(set "path"(s "../arithmetic.bin"))),("empty expectation",nest "expected"(set "registers"(obj[]))),("unexpected write permission",nest "expected"(\v->set "memory"(arr(list(get v "memory")++[obj[("region",s "code"),("offset",n 0),("hex",s "00")]]))v))]
      invalid label item=do
       B.writeFile path(encodeCanonical item)
       (code,report)<-runCase runner path
       ensure(code==1 && get report "status"==s "invalid_case")(label++": "++show report)
  forM_ mutations$ \(label,change)->invalid label(change original)
  invalid "execute-never region"(highCode 8 original)
  forM_["{\"schema_version\":1,\"schema_version\":1}","{\"schema_version\":NaN}"]$ \raw->do
   B.writeFile path raw; (_,report)<-runCase runner path; ensure(get report "status"==s "invalid_case")"invalid JSON accepted"
  B.writeFile path(encodeCanonical(set "origin"(s "stock")original))
  (stockCode,stockReport)<-runCase runner path
  ensure(stockCode==0 && get stockReport "status"==s "passed" && get stockReport "origin"==s "stock" && get stockReport "profile"==s "stock-bounded")"stock origin must be admitted and reported as a stock profile"
  -- Python's exact int checks rejected integral decimal/exponent tokens too.
  let replaceToken old new text=case stripPrefix old text of
       Just rest->new++rest
       Nothing->case text of []->[];ch:rest->ch:replaceToken old new rest
  forM_ ["1.0","1e0"]$ \spelling->do
   B.writeFile path(BC.pack(replaceToken "\"schema_version\":1" ("\"schema_version\":"++spelling)(BC.unpack(encodeCanonical original))))
   (code,numericReport)<-runCase runner path
   ensure(code==1 && get numericReport "status"==s "invalid_case")"noninteger JSON token accepted"
  B.writeFile path(encodeCanonical(nest "expected"(nest "registers"(set "r0"(s "0x00000004")))original))
  (wrong,report)<-runCase runner path
  ensure(wrong==1 && get report "status"==s "failed" && get(get report "runner")"status"==s "returned" && get(get(get report "runner")"registers")"r0"==s "0x00000003" && not(null(list(get report "mismatches"))) && not(null(list(get(get report "runner")"trace"))))"wrong independent expectation must fail"
  stub<-B.readFile "emulation/fixtures/explicit-stub.json" >>= jsonRead
  let mismatch=nest "registers"(set "r0"(s "0x00000007"))$set "expected"(obj[("status",s "stub_contract"),("registers",obj[("r0",s "0x00000007")]),("memory",arr[]),("events",arr[])])stub
  B.writeFile path(encodeCanonical mismatch); (stubCode,_)<-runCase runner path; ensure(stubCode==0)"stub mismatch did not trap"
  wall<-B.readFile "emulation/fixtures/wall-budget.json" >>= jsonRead
  B.writeFile path(encodeCanonical wall)
  let fake=root </> "fake-runner"
  BC.writeFile fake "#!/bin/sh\nprintf 'partial output\\n'\nprintf 'partial error\\n' >&2\ntrap '' TERM\nsleep 30\n"
  setFileMode fake 0o700
  (timeoutCode,timed)<-runCase fake path
  ensure(timeoutCode==1 && get timed "status"==s "host_timeout" && "partial output" `isInfixOf` str(get timed "stdout") && "partial error" `isInfixOf` str(get timed "stderr"))"timeout partial diagnostics"
  BC.writeFile fake "#!/bin/sh\nprintf 'malformed engine output\\n'\nprintf 'engine detail\\n' >&2\n"
  (malformed,bad)<-runCase fake path
  ensure(malformed==1 && get bad "status"==s "runner_failure" && "malformed engine output" `isInfixOf` str(get bad "runner_stdout") && "engine detail" `isInfixOf` str(get bad "stderr"))"malformed output diagnostics"
  createNamedPipe(root </> "artifact-pipe")0o600
  invalid "FIFO artifact"(artifact(set "path"(s "artifact-pipe"))original)
 (_,_,wire,_)<-loadCase "emulation/fixtures/arithmetic.json"
 let lines'=lines(BC.unpack wire)
     changed row value=unlines[if i==row then value else line|(i,line)<-zip[0::Int ..]lines']
     replace old new text=case stripPrefix old text of Just rest->new++rest;Nothing->case text of []->[];x:xs->x:replace old new xs
     bad=[changed row value|(row,value)<-[(0,"FUJI_NATIVE_V2"),(1,"cortex-r5f-thumb-le"),(1,"arm926-arm-le"),(2,"1004 1080 0 1000"),(2,"1004 1080 10 0"),(2,"1005 1080 10 1000"),(5,"fffffff8 c 5 "++last(words(lines'!!5))),(5,replace " c 5 " " c 7 "(lines'!!5)),(7,replace "2004" "1004"(lines'!!7)),(7,"7ffffff8 8 5 0000000000000000"),(9,"1004 10")]]++[changed 3(unwords[if i==15 then "30" else v|(i,v)<-zip[0::Int ..](words(lines'!!3))]),BC.unpack wire++"unexpected\n",take(B.length wire `div` 2)(BC.unpack wire)]
 forM_ bad$ \input->do
  (timed,code,out,_)<-runBounded 3000000 (8*1024*1024) runner[](BC.pack input)
  result<-jsonRead out
  ensure(not timed && code==ExitFailure 1 && get result "status"==s "invalid_case")"independent wire rejection failed"
 putStrLn(show(length names)++" hand-derived execution fixtures, 29 JSON rejections, "++show(length bad)++" independent wire rejections, execute-never boundary, stub/output/deadline checks, contract limit boundaries and schema agreement passed")
