{-# LANGUAGE OverloadedStrings,ScopedTypeVariables #-}
module Main where
import FujiAF.Host
import Emulation.NativeCase
import Control.Monad
import Data.Aeson(Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
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
main :: IO ()
main=do
 [runner,generator]<-getArgs
 names<-sort.filter((==".json").takeExtension) <$> listDirectory "emulation/fixtures"
 forM_ names$ \name->do
  let path="emulation/fixtures" </> name
  (code,report)<-runCase runner path
  ensure(code==0 && get report "status"==s "passed")(name++": "++show report)
  fixture<-canonicalizePath path >>= readRegularWith(pure())False(4*limit) >>= jsonRead
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
  ensure(not timed && code==ExitFailure 1 && get result "status"==s "invalid_case")"independent C wire rejection failed"
 putStrLn(show(length names)++" hand-derived execution fixtures, 29 JSON rejections, "++show(length bad)++" independent C rejections, execute-never boundary, stub/output/deadline checks passed")
