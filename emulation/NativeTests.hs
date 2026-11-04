{-# LANGUAGE OverloadedStrings,ScopedTypeVariables #-}
module Main where
import FujiAF.Host
import Emulation.NativeCase
import qualified Emulation.NativeObservation as N
import qualified Emulation.OwnedCase as O
import Control.Exception (SomeException, evaluate, try)
import Control.Monad
import Data.Aeson(Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Bits
import Data.IORef
import qualified Data.Map.Strict as M
import Data.Word
import Data.Char(toLower)
import Numeric(showHex)
import FujiAF.JSON(encodeCanonical)
import Data.List
import Data.Maybe(fromMaybe)
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
  B.writeFile path(encodeCanonical(set "limits" (obj[("instructions",n budget),("wall_ms",n finishingWallMs)])base))
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
       ,("limits",obj[("instructions",n 1000),("wall_ms",n finishingWallMs)])
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
 let batch=get defs "batch"
 expect "batch cases maxItems"(get(get(get batch "properties")"cases")"maxItems")(toInteger batchCaseCount)
 expect "batch maxJsonBytes"(get batch "maxJsonBytes")(toInteger batchJsonBytes)
 expect "batch maxArtifactBytes"(get batch "maxArtifactBytes")(toInteger batchArtifactBytes)
 expect "batch summary cases maxItems"(get(get(get(get defs "batchSummary")"properties")"cases")"maxItems")(toInteger batchCaseCount)

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

-- Batches

-- A report alone and in a batch differ only in elapsed time, and a wall-budget case also in how
-- far it got before its deadline.
hostIndependent :: Value -> Value
hostIndependent report=case get report "runner" of
 Null->report
 result->set "runner"(foldr remove result(if get result "status"==s "wall_budget" then ["elapsed_ns","instructions","trace_total","trace"] else ["elapsed_ns"]))report

collectBatch :: FilePath -> FilePath -> IO (BatchSummary,[CaseReport])
collectBatch runner path=do
 reports<-newIORef[]
 summary<-runBatch runner path(\report->modifyIORef' reports(report:))
 collected<-reverse<$>readIORef reports
 pure(summary,collected)

writeBatchFile :: FilePath -> [(String,FilePath)] -> IO FilePath
writeBatchFile root entries=do
 let path=root</>"batch.json"
 B.writeFile path(encodeCanonical(obj[("schema_version",n 1),("cases",arr[obj[("id",s ident),("path",s file)]|(ident,file)<-entries])]))
 pure path

-- Validates documents against a definition of schemas/native-case.schema.json.
schemaDefinition :: FilePath -> String -> [Value] -> IO ()
schemaDefinition checker definition documents=withTemporary "fuji-batch-schema"$ \root->do
 schema<-canonicalizePath "schemas/native-case.schema.json"
 let wrapper=root</>definition<.>"json"
 B.writeFile wrapper(encodeCanonical(obj[("$schema",s "https://json-schema.org/draft/2020-12/schema"),("$ref",s("file://"++schema++"#/$defs/"++definition))]))
 schemaCheck checker wrapper documents

-- Runs a batch, requires every case's report to equal its report alone and the summary to bind
-- the batch file's SHA256 to each case's id, status and case SHA256 in batch order.
sameAsAlone :: FilePath -> FilePath -> FilePath -> [(String,FilePath)] -> IO (BatchSummary,[CaseReport])
sameAsAlone runner checker root entries=do
 path<-writeBatchFile root entries
 (summary,reports)<-collectBatch runner path
 ensure(map(caseIdText . reportCase)reports==map fst entries)("batch report order: "++show(map(caseIdText . reportCase)reports))
 forM_(zip entries reports)$ \((ident,file),report)->do
  (aloneCode,alone)<-runCase runner(root</>file)
  ensure(reportCode report==aloneCode && hostIndependent(reportValue report)==hostIndependent alone)(ident++": batch report differs from the case alone:\n"++show(reportValue report)++"\n"++show alone)
 digest<-B.readFile path>>=sha256
 let bound r=BatchCase(reportCase r)(caseStatusOf(reportValue r))(reportDigest(reportValue r))
 ensure(summaryBatchDigest summary==Just(DigestHex digest) && summaryStatus summary==BatchComplete && summaryCases summary==map bound reports)("batch summary: "++show summary)
 batchFile<-B.readFile path>>=jsonRead
 schemaDefinition checker "batch"[batchFile]
 schemaDefinition checker "batchSummary"[summaryJson summary]
 pure(summary,reports)

statusesOf :: [CaseReport] -> [(String,String)]
statusesOf reports=[(caseIdText(reportCase r),str(get(reportValue r)"status"))|r<-reports]

-- Every public fixture, positive, refusal and budget alike, with a failing expectation in the middle.
fixtureBatch :: FilePath -> FilePath -> IO Int
fixtureBatch runner checker=withTemporary "fuji-batch-fixtures"$ \root->do
 names<-sort.filter((`elem`[".json",".bin"]).takeExtension)<$>listDirectory "emulation/fixtures"
 forM_ names$ \name->copyFile("emulation/fixtures"</>name)(root</>name)
 cases<-forM[name|name<-names,takeExtension name==".json"]$ \name->do
  fixture<-B.readFile(root</>name)>>=jsonRead
  pure(str(get fixture "id"),name)
 arithmetic<-B.readFile(root</>"arithmetic.json")>>=jsonRead
 B.writeFile(root</>"expectation-mismatch.json")(encodeCanonical(merge arithmetic[("id",s "expectation-mismatch"),("expected",set "registers"(obj[("r0",s "0x00000004")])(get arithmetic "expected"))]))
 let (early,late)=splitAt(length cases`div`2)cases
     entries=early++[("expectation-mismatch","expectation-mismatch.json")]++late
 (summary,reports)<-sameAsAlone runner checker root entries
 ensure(not(batchPassed summary) && statusesOf reports==[(ident,if ident=="expectation-mismatch" then "failed" else "passed")|(ident,_)<-entries])("fixture batch statuses: "++show(statusesOf reports))
 pure(length entries)

hexWord :: Word32 -> Value
hexWord value=s("0x"++replicate(8-length text)'0'++text) where text=showHex value ""

wordBytes :: [Word32] -> B.ByteString
wordBytes ws=B.pack[fromIntegral(w`shiftR`k)|w<-ws,k<-[0,8,16,24]]

-- One function of code at 0x8000, a data word at 0x9000 seeded with 0x11223344 from the shared
-- seed.bin, the exit at 0xa000 and a 32-byte stack below SP 0xb020. Unnamed registers start at
-- zero and CPSR at 0x10.
isolationCase :: FilePath -> String -> [Word32] -> [(String,Word32)] -> (Integer,Integer) -> Value -> IO (String,FilePath)
isolationCase root ident code initial(instructions,wallMs)expected=do
 let seed=wordBytes[0x11223344]
     bytes=wordBytes code
     size=toInteger(B.length bytes)
 B.writeFile(root</>"seed.bin")seed
 B.writeFile(root</>ident<.>"bin")bytes
 seedDigest<-sha256 seed
 digest<-sha256 bytes
 let region name address size' permissions initial'=obj[("id",s name),("address",hexWord address),("size",n size'),("permissions",s permissions),("initial",initial')]
     slice artifact len=obj[("artifact",s artifact),("offset",n 0),("length",n len)]
     zero=obj[("zero",b True)]
     registers=merge(obj([("r"++show i,hexWord 0)|i<-[0..12::Int]]++[("sp",hexWord 0xb020),("lr",hexWord 0xa000),("cpsr",hexWord 0x10)]))[(k,hexWord v)|(k,v)<-initial]
     fixture=obj
      [("schema_version",n 1),("id",s ident),("origin",s "synthetic"),("provenance",s "NativeTests.hs: batch isolation control")
      ,("cpu",obj[("architecture",s "arm"),("model",s "cortex-r5f"),("mode",s "arm"),("endian",s "little")])
      ,("artifacts",arr[obj[("id",s "code"),("path",s(ident<.>"bin")),("size",n size),("sha256",s digest)],obj[("id",s "seed"),("path",s "seed.bin"),("size",n 4),("sha256",s seedDigest)]])
      ,("entry",hexWord 0x8000),("exit",hexWord 0xa000),("registers",registers)
      ,("regions",arr[region "code" 0x8000 size "rx"(slice "code" size),region "data" 0x9000 4 "rw"(slice "seed" 4),region "exit" 0xa000 4 "rx" zero,region "stack" 0xb000 32 "rw" zero])
      ,("functions",arr[obj[("address",hexWord 0x8000),("size",n size)]]),("stubs",arr[])
      ,("limits",obj[("instructions",n instructions),("wall_ms",n wallMs)]),("expected",expected)
      ,("exclusions",arr[s "batch isolation control"])]
 B.writeFile(root</>ident<.>"json")(encodeCanonical fixture)
 pure(ident,ident<.>"json")

expectation :: String -> [(String,Word32)] -> [String] -> Value
expectation status registers memory=obj[("status",s status),("registers",obj[(k,hexWord v)|(k,v)<-registers]),("memory",arr[obj[("region",s "data"),("offset",n 0),("hex",s m)]|m<-memory]),("events",arr[])]

-- The largest wall_ms the runner admits, for cases that must finish on a loaded host.
finishingWallMs :: Integer
finishingWallMs=10000

writerCase, readerCase :: Integer -> FilePath -> IO (String,FilePath)
-- STR r0,[r2]: stores 0xdeadbeef over the seed.
writerCase wallMs root=isolationCase root "writer"[0xe5820000,0xe12fff1e][("r0",0xdeadbeef),("r2",0x9000)](100,wallMs)(expectation "returned"[("r0",0xdeadbeef),("pc",0xa000)]["efbeadde"])
-- LDR r1,[r2]: reads its own seed.
readerCase wallMs root=isolationCase root "reader"[0xe5921000,0xe12fff1e][("r2",0x9000)](100,wallMs)(expectation "returned"[("r1",0x11223344),("pc",0xa000)][])

-- Each case would observe a different result if anything of an earlier case carried over.
isolationBatch :: FilePath -> FilePath -> IO ()
isolationBatch runner checker=withTemporary "fuji-batch-isolation"$ \root->do
 entries<-sequence
  [ writerCase finishingWallMs root
  , readerCase finishingWallMs root
  -- MOVS r0,#0 sets Z; MVN r12,#0 and MOV r5,#0x55 change two more registers.
  , isolationCase root "dirty"[0xe3b00000,0xe3e0c000,0xe3a05055,0xe12fff1e][("r0",7)](100,finishingWallMs)(expectation "returned"[("r0",0),("r5",0x55),("r12",0xffffffff),("cpsr",0x40000010)][])
  -- ADDEQ r0,r0,#1 and MOVNE r5,#7 under its own Z=0.
  , isolationCase root "flags"[0x02800001,0x13a05007,0xe12fff1e][("r0",0x10)](100,finishingWallMs)(expectation "returned"[("r0",0x10),("r5",7),("r12",0),("cpsr",0x10)][])
  , isolationCase root "exhaust-instructions"[0xeafffffe][](1000,finishingWallMs)(expectation "instruction_budget"[("pc",0x8000)][])
  -- Three instructions under a budget of exactly three.
  , isolationCase root "exact-budget"[0xe3a00001,0xe2800002,0xe12fff1e][](3,finishingWallMs)(expectation "returned"[("r0",3),("pc",0xa000)][])
  -- Expects the writer's word; reading its own seed fails it.
  , isolationCase root "expects-carried-value"[0xe5921000,0xe12fff1e][("r2",0x9000)](100,finishingWallMs)(expectation "returned"[("r1",0xdeadbeef)][])
  , isolationCase root "exhaust-wall"[0xeafffffe][](4000000,1)(expectation "wall_budget"[("pc",0x8000)][])
  -- MOVW/MOVT r0,#250000, then SUBS/BNE to zero: 500,003 instructions under exactly that budget.
  , isolationCase root "countdown"[0xe30d0090,0xe3400003,0xe2500001,0x1afffffd,0xe12fff1e][](500003,finishingWallMs)(expectation "returned"[("r0",0),("cpsr",0x60000010),("pc",0xa000)][])
  ]
 (summary,reports)<-sameAsAlone runner checker root entries
 ensure(not(batchPassed summary) && statusesOf reports==[(ident,if ident=="expects-carried-value" then "failed" else "passed")|(ident,_)<-entries])("isolation statuses: "++show(statusesOf reports))

-- A whole-process failure fails the batch at the case whose frame was due; a runner refusal of
-- the batch framing fails no case.
batchFailures :: FilePath -> FilePath -> IO ()
batchFailures runner checker=withTemporary "fuji-batch-failures"$ \root->do
 entries<-sequence[writerCase finishingWallMs root,readerCase finishingWallMs root]
 path<-writeBatchFile root entries
 -- The hanging runner reaches the writer's host deadline, so that batch keeps a short wall_ms.
 let hangRoot=root</>"hang"
 createDirectory hangRoot
 hangPath<-sequence[writerCase 1000 hangRoot,readerCase 1000 hangRoot]>>=writeBatchFile hangRoot
 let substitute name script batch=do
      let file=root</>name
      BC.writeFile file(BC.pack script)
      setFileMode file 0o700
      collectBatch file batch
     statuses summary=map batchCaseStatus(summaryCases summary)
 (hang,hangReports)<-substitute "hanging-runner" "#!/bin/sh\ncat >/dev/null\nprintf 'FUJI_NATIVE_BATCH_CASE 0 1\\n'\ntrap '' TERM\nsleep 30\n" hangPath
 ensure(summaryStatus hang==BatchHostTimeout && summaryFailedCase hang==Just(CaseId "writer") && statuses hang==[CaseHostTimeout,CaseNotRun]
  && statusesOf hangReports==[("writer","host_timeout")] && "FUJI_NATIVE_BATCH_CASE 0 1"`isInfixOf`str(get(reportValue(head hangReports))"stdout"))("hanging runner: "++show hang)
 real<-canonicalizePath runner
 (crash,crashReports)<-substitute "crashing-runner"("#!/bin/sh\n'"++real++"' --batch | { IFS= read -r header; printf '%s\\n' \"$header\"; IFS= read -r observation; printf '%s\\n' \"$observation\"; }\nkill -SEGV $$\n")path
 ensure(summaryStatus crash==BatchRunnerFailure && summaryFailedCase crash==Just(CaseId "reader") && summaryRunnerExit crash==Just(ExitFailure(-11))
  && statusesOf crashReports==[("writer","passed"),("reader","runner_failure")])("crashing runner: "++show crash)
 (refusal,refusalReports)<-substitute "refusing-runner" "#!/bin/sh\ncat >/dev/null\nprintf 'FUJI_NATIVE_BATCH_INVALID substitute refusal\\n'\nexit 1\n" path
 ensure(summaryStatus refusal==BatchInvalid && summaryFailedCase refusal==Nothing && statuses refusal==[CaseNotRun,CaseNotRun] && null refusalReports
  && fmap("substitute refusal"`isInfixOf`)(summaryReason refusal)==Just True)("refusing runner: "++show refusal)
 schemaDefinition checker "batchSummary"(map summaryJson[hang,crash,refusal])
 reader<-sharedArtifactReader
 admitted<-forM entries(\(ident,file)->snd<$>admitCase reader(BatchEntry(CaseId ident)(root</>file)))
 let artifacts=batchArtifacts admitted
     index=M.fromList(zip(map artifactDigest artifacts)[0..])
     bodies=map(batchBody index)admitted
     frameHeaders out=[line|(i,line)<-zip[0::Int ..](BC.lines out),even i]
     runWire label wire=do
      (timed,exit,out,_)<-runBounded 120000000(64*1024*1024)runner["--batch"]wire
      ensure(not timed)(label++": runner timed out")
      pure(exit,out)
     refusedWire label wire=do
      (exit,out)<-runWire label wire
      ensure(exit==ExitFailure 1 && BC.pack "FUJI_NATIVE_BATCH_INVALID "`B.isPrefixOf`out)(label++": "++BC.unpack(B.take 4096 out))
     ranWire label count wire=do
      (exit,out)<-runWire label wire
      ensure(exit==ExitSuccess && frameHeaders out==map BC.pack(["FUJI_NATIVE_BATCH_CASE "++show k++" 0"|k<-[0..count-1]]++["FUJI_NATIVE_BATCH_END "++show count]))(label++": "++BC.unpack(B.take 4096 out))
 -- The runner reports a case whose slice names an artifact index equal to the artifact count as
 -- that case's invalid_case.
 let seedSlice=BC.pack(" s "++show(head[i|(i,artifact)<-zip[0::Int ..]artifacts,artifactBytes artifact==wordBytes[0x11223344]])++" 0 4")
     replaceSlice body=let (before,after)=B.breakSubstring seedSlice body in before<>BC.pack(" s "++show(length artifacts)++" 0 4")<>B.drop(B.length seedSlice)after
     wire=BL.toStrict(batchWire artifacts[head bodies,replaceSlice(bodies!!1)])
 ensure(seedSlice`B.isInfixOf`(bodies!!1))("reader body lacks the seed slice: "++BC.unpack(bodies!!1))
 (exit,out)<-runWire "per-case wire refusal" wire
 observations<-mapM jsonRead[line|(i,line)<-zip[0::Int ..](BC.lines out),odd i,i<4]
 ensure(exit==ExitSuccess && frameHeaders out==map BC.pack["FUJI_NATIVE_BATCH_CASE 0 0","FUJI_NATIVE_BATCH_CASE 1 1","FUJI_NATIVE_BATCH_END 2"]
  && map(`get`"status")observations==[s "returned",s "invalid_case"])("per-case wire refusal: "++BC.unpack out)
 forM_[BC.pack "FUJI_NATIVE_BATCH_V2"<>B.drop 20 wire,B.take(B.length wire-8)wire,BC.pack "FUJI_NATIVE_BATCH_V1\n0 0\n",wire<>BC.pack "trailing\n"]$
  refusedWire "batch framing refusal"
 -- The runner's own batch limits: the case count and the distinct artifact bytes.
 let writerBody=head bodies
 ranWire "case count at the limit"batchCaseCount(BL.toStrict(batchWire artifacts(replicate batchCaseCount writerBody)))
 refusedWire "case count past the limit"(BL.toStrict(batchWire artifacts(replicate(batchCaseCount+1)writerBody)))
 let used=sum(map(B.length . artifactBytes)artifacts)
     full=B.replicate sizeBytes 0x5a
     padding extra=let room=batchArtifactBytes-used+extra; (whole,part)=room`divMod`sizeBytes in replicate whole full++[B.replicate part 0x5a|part>0]
     padded extra=BL.toStrict(batchWire(artifacts++[Artifact(ArtifactDigest "padding")bytes|bytes<-padding extra])[writerBody])
 ranWire "artifact bytes at the limit"1(padded 0)
 refusedWire "artifact bytes past the limit"(padded 1)

-- The batch limits admit a batch at each limit and refuse one past it.
batchLimits :: FilePath -> IO ()
batchLimits runner=withTemporary "fuji-batch-limits"$ \root->do
 let refused label path=do
      (summary,reports)<-collectBatch runner path
      ensure(summaryStatus summary==BatchInvalid && null reports)(label++" past the limit: "++show summary)
     admitted label path=do
      (summary,_)<-collectBatch runner path
      ensure(summaryStatus summary==BatchComplete)(label++" at the limit: "++show summary)
     missing count=[("c"++show i,"missing-"++show i<.>"json")|i<-[1..count]]
 writeBatchFile root(missing batchCaseCount)>>=admitted "case count"
 writeBatchFile root(missing(batchCaseCount+1))>>=refused "case count"
 let sparse name size=B.writeFile(root</>name)B.empty>>setFileSize(root</>name)(fromIntegral size)
     sized=[("j"++show i,"j"++show i<.>"json")|i<-[1..16::Int]]
 path<-writeBatchFile root sized
 batchSize<-B.length<$>B.readFile path
 forM_(init sized)$ \(_,file)->sparse file caseBytes
 let lastSize=batchJsonBytes-15*caseBytes-batchSize
 sparse(snd(last sized))lastSize>>admitted "JSON bytes" path
 sparse(snd(last sized))(lastSize+1)>>refused "JSON bytes" path
 -- Distinct artifact bytes read: the writer's 8 code bytes and 4 seed bytes, plus one declared,
 -- unsliced blob per case. The last case's blob copies the first one's bytes under another path,
 -- which adds nothing.
 (_,writerFile)<-writerCase finishingWallMs root
 writer<-B.readFile(root</>writerFile)>>=jsonRead
 let blobs=batchArtifactBytes`div`sizeBytes
     blobCase i size=do
      let ident="blob"++show i; artifact=ident<.>"bin"
      B.writeFile(root</>artifact)(B.pack(map fromIntegral[i`div`256,i`mod`256]))
      setFileSize(root</>artifact)(fromIntegral size)
      blobCopy ident artifact size
     blobCopy ident artifact size=do
      digest<-B.readFile(root</>artifact)>>=sha256
      let fixture=merge writer[("id",s ident),("artifacts",arr(list(get writer "artifacts")++[obj[("id",s "blob"),("path",s artifact),("size",n(toInteger size)),("sha256",s digest)]]))]
      B.writeFile(root</>ident<.>"json")(encodeCanonical fixture)
      pure(ident,ident<.>"json")
     sizedBlobs lastSize=do
      full<-forM[1..blobs-1]$ \i->blobCase i sizeBytes
      final<-blobCase blobs lastSize
      copyFile(root</>"blob1.bin")(root</>"copy.bin")
      copied<-blobCopy "copy""copy.bin"sizeBytes
      pure(full++[final,copied])
 sizedBlobs(sizeBytes-12)>>=writeBatchFile root>>=admitted "artifact bytes"
 sizedBlobs(sizeBytes-11)>>=writeBatchFile root>>=refused "artifact bytes"

-- The owned-case batch API yields each case's observation as loadObserved does alone.
ownedBatchApi :: FilePath -> IO ()
ownedBatchApi runner=withTemporary "fuji-owned-batch"$ \root->do
 let spec ident code initial expected memory=O.CaseSpec
      { O.caseId=ident
      , O.caseProvenance="NativeTests.hs: owned batch API"
      , O.caseExclusion="batch API control"
      , O.caseEntry=N.GuestAddress 0x8000
      , O.caseExit=N.GuestAddress 0xa000
      , O.caseRegions=
         [ O.mkRegion(N.RegionId "code")(N.GuestAddress 0x8000)(4*length code)O.ReadExecute(O.assemble code)
         , O.mkRegion(N.RegionId "data")(N.GuestAddress 0x9000)4 O.ReadWrite(O.word32le 0x11223344)
         , O.mkRegion(N.RegionId "exit")(N.GuestAddress 0xa000)4 O.ReadExecute(B.replicate 4 0)
         , O.mkRegion(N.RegionId "stack")(N.GuestAddress 0xb000)32 O.ReadWrite(B.replicate 32 0)
         ]
      , O.caseFunctions=[(N.GuestAddress 0x8000,4*fromIntegral(length code))]
      , O.caseInitial=[(r,fromMaybe 0(lookup r initial))|r<-[N.R0 .. N.R12]]++[(N.Sp,0xb020),(N.Lr,0xa000),(N.Cpsr,0x10)]
      , O.caseExpectedStatus=N.Returned
      , O.caseExpectedRegisters=expected
      , O.caseExpectedMemory=memory
      }
     specs=
      [ spec "writer"[0xe5820000,0xe12fff1e][(N.R0,0xdeadbeef),(N.R2,0x9000)][(N.R0,0xdeadbeef)][(N.RegionId "data",O.word32le 0xdeadbeef)]
      , spec "reader"[0xe5921000,0xe12fff1e][(N.R2,0x9000)][(N.R1,0x11223344)][]
      ]
 alone<-forM specs$ \one->O.writeCase(root</>"alone"</>O.caseId one)one>>=O.loadObserved runner
 batched<-O.writeBatch(root</>"batch")specs>>=O.loadObservedBatch runner
 ensure(batched==zip(map(CaseId . O.caseId)specs)alone)("owned batch observations differ from single-case observations: "++show batched)
 -- A case without an observation fails the call after the batch, naming it.
 let broken=(spec "broken"[0xe12fff1e][][(N.R0,0)][]){O.caseFunctions=[(N.GuestAddress 0x7000,4)]}
 failed<-try(O.writeBatch(root</>"failing")(specs++[broken])>>=O.loadObservedBatch runner>>=evaluate . length)
 case failed of
  Left(e::SomeException)->ensure("broken: invalid_case"`isInfixOf`show e)("loadObservedBatch failure names: "++show e)
  Right _->failH "loadObservedBatch returned for a batch with an invalid case"

main :: IO ()
main=do
 [runner,generator,checker]<-getArgs
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
 batched<-fixtureBatch runner checker
 isolationBatch runner checker
 batchFailures runner checker
 batchLimits runner
 ownedBatchApi runner
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
 putStrLn(show batched++"-case fixture batch, 9 isolation controls, batch failure and framing controls, batch limit boundaries and the owned-case batch API passed")
