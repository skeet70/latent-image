{-# LANGUAGE OverloadedStrings #-}
-- Hand-encoded words and independently derived expectations, never emulator output.
module Main where
import FujiAF.Host
import Data.Aeson(Value)
import qualified Data.ByteString as B
import Data.Bits
import Numeric
import Control.Monad
import System.Directory
import System.Environment
import System.FilePath
import FujiAF.JSON(encodeCanonical)

h :: Integer -> Value
h value=s("0x"++replicate(8-length text)'0'++text) where text=showHex value ""
artifact :: FilePath -> String -> [Integer] -> IO Value
artifact root name words'=do
 let bytes=B.pack[fromInteger((word `shiftR` offset).&.255)|word<-words',offset<-[0,8,16,24]]
 B.writeFile(root </> name<.>"bin")bytes
 hash<-sha256 bytes
 pure$obj[("id",s name),("path",s(name++".bin")),("size",n(fromIntegral(B.length bytes))),("sha256",s hash)]
region :: String -> Integer -> Integer -> String -> Maybe Value -> Value
region name base size permissions art=obj[("id",s name),("address",h base),("size",n size),("permissions",s permissions),("initial",maybe(obj[("zero",b True)])(\a->obj[("artifact",get a "id"),("offset",n 0),("length",get a "size")])art)]
newCase :: FilePath -> String -> [Integer] -> String -> IO Value
newCase root name words' status=do
 art<-artifact root name words'
 let regs'=obj([( "r"++show i,h 0)|i<-[0::Int ..12]]++[("r0",h 0xfffffffe),("r1",h 5),("r2",h 0x2004),("sp",h 0x3020),("lr",h 0x1080),("cpsr",h 0x10)])
 pure$obj[("schema_version",n 1),("id",s name),("origin",s "synthetic"),("provenance",s "PROVENANCE.md: hand-encoded ARM instructions and independent arithmetic"),("cpu",obj[("architecture",s "arm"),("model",s "cortex-r5f"),("mode",s "arm"),("endian",s "little")]),("artifacts",arr[art]),("entry",h 0x1004),("exit",h 0x1080),("registers",regs'),("regions",arr[region "code" 0x1004(num(get art "size"))"rx"(Just art),region "exit" 0x1080 4 "rx" Nothing,region "data" 0x2004 4 "rw" Nothing,region "stack" 0x3000 32 "rw" Nothing]),("functions",arr[obj[("address",h 0x1004),("size",get art "size")]]),("stubs",arr[]),("limits",obj[("instructions",n 100),("wall_ms",n 1000)]),("expected",obj[("status",s status),("registers",obj[("r0",get regs' "r0"),("r1",get regs' "r1")]),("memory",arr[]),("events",arr[])]),("exclusions",arr(map s["Synthetic Cortex-R5F user-mode ARM/little-endian only; no X-M1 behavior or ABI claim","No exceptions, coprocessors, mode switching, PC-loading returns, MMIO, interrupts, DMA, caches, scheduling or hardware timing"]))]
nest :: String -> (Value -> Value) -> Value -> Value
nest key change c=set key(change(get c key))c
at :: String -> Int -> (Value -> Value) -> Value -> Value
at key i change c=set key(arr[if index==i then change value else value|(index,value)<-zip[0::Int ..](list(get c key))])c
expectedRegs :: [(String,Value)] -> Value -> Value
expectedRegs changes c=nest "expected"(set "registers"(merge(get c "registers")changes))c
dropRegion :: String -> Value -> Value
dropRegion name v=arr[item|item<-list v,str(get item "id")/=name]
memory :: String -> Integer -> String -> Value
memory name offset bytes=obj[("region",s name),("offset",n offset),("hex",s bytes)]
save :: FilePath -> Value -> IO ()
save root c=B.writeFile(root </> str(get c "id")<.>"json")(encodeCanonical c<>"\n")
main :: IO ()
main=do
 args<-getArgs
 root<-case args of
  ["--output",path]->pure path
  _->failH "usage: generate-fixtures --output NEW-DIRECTORY (historical fixture bytes are preserved)"
 exists<-doesPathExist root
 ensure(not exists) "output directory must be new"
 createDirectoryIfMissing True root
 let make name words' status=newCase root name words' status
     finish=save root
 a<-make "arithmetic" [0xe0800001,0xe5820000,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "03000000"]))$expectedRegs[("r0",h 3),("pc",h 0x1080)]a
 a2<-make "arithmetic-carry" [0xe0900001,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 3),("cpsr",h 0x20000010),("pc",h 0x1080)]a2
 overflow<-make "arithmetic-signed-overflow" [0xe0900001,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0x80000000),("cpsr",h 0x90000010),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0x7fffffff),("r1",h 1)])overflow
 skipped<-make "conditional-call-skipped" [0x0b000bfd,0xe12fff1e] "returned"
 finish$expectedRegs[("pc",h 0x1080)]skipped
 skippedMemory<-make "conditional-memory-skipped" [0x05920000,0xe12fff1e] "returned"
 finish$expectedRegs[("pc",h 0x1080)]$nest "registers"(set "r2"(h 0x40000000))skippedMemory
 stack<-make "stack-roundtrip" [0xe92d4003,0xe3a00000,0xe3a01000,0xe8bd4003,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "stack" 20 "feffffff0500000080100000"]))$expectedRegs[("pc",h 0x1080)]stack
 make "multiple-write-boundary" [0xe8a20003,0xe12fff1e] "memory_guard" >>= finish
 shared<-make "shared-page-readonly" [0xe5820000,0xe12fff1e] "memory_guard"
 finish$at "regions" 2(set "address"(h 0x1020))$nest "registers"(set "r2"(h 0x1004))shared
 forM_[("unsupported-psr-write",0xe129f000,"unsupported_instruction"),("unsupported-psr-immediate",0xe328f000,"unsupported_instruction"),("unsupported-arm-divide",0xe710f110,"unsupported_instruction"),("unsupported-register-offset-pc-index",0xe7d2000f,"unsupported_instruction"),("unsupported-register-offset-store-pc",0xe7c2f003,"unsupported_instruction"),("unsupported-store-pc",0xe582f000,"unsupported_instruction"),("unsupported-register-offset-shift",0xe7920311,"unsupported_instruction"),("unsupported-register-offset-postindex",0xe6920001,"unsupported_instruction"),("unsupported-halfword-register-postindex",0xe09250b3,"unsupported_instruction"),("unsupported-multiply-flags",0xe0100291,"unsupported_instruction"),("unsupported-multiply-long",0xe0810392,"unsupported_instruction"),("unsupported-sxth",0xe6bf0071,"unsupported_instruction"),("unsupported-jump-table-subtract",0x971ff101,"unsupported_instruction")]$ \(name,word,status)->make name[word]status >>= finish
 hwo<-make "halfword-odd-address" [0xe1d210b1,0xe12fff1e] "memory_guard"
 finish$expectedRegs[("pc",h 0x1004)]hwo
 wide<-make "movw-movt" [0xe3050678,0xe3410234,0xe30b1eef,0xe34c2afe,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0x12345678),("r1",h 0xbeef),("r2",h 0xcafe2004),("pc",h 0x1080)]wide
 call<-make "native-call" [0xe1a0400e,0xeb00000c,0xe2800002,0xe1a0e004,0xe12fff1e] "returned"
 callee<-artifact root "callee" [0xe0800001,0xe12fff1e]
 let call'=merge call[("artifacts",arr(list(get call "artifacts")++[callee])),("regions",arr(list(get call "regions")++[region "callee" 0x1040 8 "rx"(Just callee)])),("functions",arr(list(get call "functions")++[obj[("address",h 0x1040),("size",n 8)]]))]
 finish$expectedRegs[("r0",h 17),("r4",h 0x1080),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 6),("r1",h 9)])call'
 stub<-make "explicit-stub" [0xe1a0400e,0xeb000bfc,0xe2800002,0xe1a0e004,0xe12fff1e] "returned"
 let event=obj[("address",h 0x4000),("inputs",obj[("r0",h 6),("r1",h 9)]),("outputs",obj[("r0",h 15)])]
 finish$nest "expected"(set "events"(arr[event]))$expectedRegs[("r0",h 17),("r4",h 0x1080),("pc",h 0x1080)]$set "stubs"(arr[set "side_effects"(s "registers-only")event])$nest "registers"(\v->merge v[("r0",h 6),("r1",h 9)])stub
 forM_[("read-page-gap",0xe5920000,0x2008,4),("read-cross-boundary",0xe5920000,0x2004,2),("write-page-gap",0xe5820000,0x2008,4),("write-cross-boundary",0xe5820000,0x2004,2),("unknown-mmio",0xe5920000,0x40000000,4),("readonly-code",0xe5820000,0x1004,4),("unaligned-read",0xe5920000,0x2005,8)]$ \(name,ins,target',size)->do
  item<-make name[ins,0xe12fff1e]"memory_guard"
  finish$at "regions" 2(set "size"(n size))$nest "registers"(set "r2"(h target'))item
 fetch<-make "fetch-page-gap" [0xeaffffff] "fetch_guard"
 finish fetch
 finish$at "regions" 0(set "size"(n 6))$set "id"(s "fetch-cross-boundary")fetch
 edge<-make "fetch-execute-never-boundary" [0xe1a00000] "fetch_guard"
 finish$set "entry"(h 0x7ffffff8)$at "functions" 0(set "address"(h 0x7ffffff8))$at "regions" 0(set "address"(h 0x7ffffff8))edge
 forM_[("fetch-permission",0xea0003fe,"fetch_guard"),("unknown-call",0xeb000bfd,"unknown_call"),("unsupported-blx-pc",0xe12fff3f,"unsupported_instruction")]$ \(name,word,status)->make name[word]status >>= finish
 unknownBlx<-make "unknown-blx-call" [0xe12fff30] "unknown_call"
 finish$expectedRegs[("r0",h 0xfffffffc)]$nest "registers"(set "r0"(h 0xfffffffc))unknownBlx
 budget<-make "instruction-budget" [0xeafffffe] "instruction_budget"
 finish$nest "limits"(set "instructions"(n 3))budget
 wall<-make "wall-budget" [0xeafffffe] "wall_budget"
 finish$set "limits"(obj[("instructions",n 1000000),("wall_ms",n 1)])wall
 ux<-make "uxtb" [0xe6ef0071,0xe6ef2473,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0x85),("r2",h 0x33),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r1",h 0xffffff85),("r3",h 0x11223344)])ux
 rob<-make "register-offset-byte" [0xe7c20003,0xe7d21003,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 1 "ab"]))$expectedRegs[("r1",h 0xab),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0xab),("r3",h 1)])rob
 robs<-make "register-offset-byte-shift" [0xe7c201a3,0xe7d211a3,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 3 "ab"]))$expectedRegs[("r1",h 0xab),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0xab),("r3",h 0x18)])robs
 robg<-make "register-offset-byte-guard" [0xe7d20003,0xe12fff1e] "memory_guard"
 finish$expectedRegs[("pc",h 0x1004)]$nest "registers"(\v->merge v[("r2",h 0xfffffff0),("r3",h 0x2014)])robg
 regword<-make "register-offset-word" [0xe7820003,0xe7921003,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 4 "78563412"]))$expectedRegs[("r1",h 0x12345678),("pc",h 0x1080)]$at "regions" 2(set "size"(n 8))$nest "registers"(\v->merge v[("r0",h 0x12345678),("r3",h 4)])regword
 regwords<-make "register-offset-word-shift" [0xe7820103,0xe7921103,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 4 "78563412"]))$expectedRegs[("r1",h 0x12345678),("pc",h 0x1080)]$at "regions" 2(set "size"(n 8))$nest "registers"(\v->merge v[("r0",h 0x12345678),("r3",h 1)])regwords
 hw<-make "halfword" [0xe1c200b2,0xe1d210b2,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 2 "7856"]))$expectedRegs[("r1",h 0x5678),("pc",h 0x1080)]$nest "registers"(set "r0"(h 0x12345678))hw
 hws<-make "halfword-subtract" [0xe14200b2,0xe15210b2,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 2 "7856"]))$expectedRegs[("r1",h 0x5678),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0x12345678),("r2",h 0x2008)])hws
 hwb<-make "halfword-writeback" [0xe1c200b2,0xe1f210b2,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 2 "7856"]))$expectedRegs[("r1",h 0x5678),("r2",h 0x2006),("pc",h 0x1080)]$nest "registers"(set "r0"(h 0x12345678))hwb
 hwr<-make "halfword-register" [0xe18200b3,0xe19210b3,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 2 "7856"]))$expectedRegs[("r1",h 0x5678),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0x12345678),("r3",h 2)])hwr
 hwrs<-make "halfword-register-subtract" [0xe10200b3,0xe11210b3,0xe12fff1e] "returned"
 finish$nest "expected"(set "memory"(arr[memory "data" 2 "7856"]))$expectedRegs[("r1",h 0x5678),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0x12345678),("r2",h 0x2008),("r3",h 2)])hwrs
 mul2<-make "multiply" [0xe0000291,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0xa014),("pc",h 0x1080)]mul2
 mlx<-make "multiply-mla" [0xe0203291,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0xa01b),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r3",h 7)])mlx
 ubf<-make "ubfx" [0xe7eb0251,0xe7e52951,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0x567),("r2",h 0xd),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r1",h 0x12345678)])ubf
 uxthf<-make "uxth" [0xe6ff0471,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0x2233),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r1",h 0x11223344)])uxthf
 sxtbf<-make "sxtb" [0xe6af0071,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0xffffff85),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r1",h 0x85)])sxtbf
 blx<-make "blx-register" [0xe1a0400e,0xe12fff35,0xe2800002,0xe1a0e004,0xe12fff1e] "returned"
 calleeBlx<-artifact root "callee-blx" [0xe0800001,0xe12fff1e]
 let blx'=merge blx[("artifacts",arr(list(get blx "artifacts")++[calleeBlx])),("regions",arr(list(get blx "regions")++[region "callee" 0x1040 8 "rx"(Just calleeBlx)])),("functions",arr(list(get blx "functions")++[obj[("address",h 0x1040),("size",n 8)]]))]
 finish$expectedRegs[("r0",h 17),("r4",h 0x1080),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 6),("r1",h 9),("r5",h 0x1040)])blx'
 blxt<-make "unsupported-blx-thumb" [0xe12fff35] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]$nest "registers"(\v->merge v[("r5",h 0x1041)])blxt
 jt<-make "jump-table" [0xe3510001,0x979ff101,0xea000005,0x00001018,0x00001020,0xe3a00001,0xea000002,0xe3a00002,0xea000000,0xe3a00003,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 2),("cpsr",h 0x60000010),("pc",h 0x1080)]$nest "registers"(set "r1"(h 1))jt
 jtx<-make "jump-table-outside" [0xe3510001,0x979ff101,0xe12fff1e,0x00001018,0x00001040,0xe3a00001,0xe12fff1e] "unknown_call"
 finish$expectedRegs[("cpsr",h 0x60000010),("pc",h 0x1040)]$at "regions" 0(set "size"(n 64))$nest "registers"(set "r1"(h 1))jtx
 popc<-make "pop-pc-return" [0xe1a0400e,0xeb00000c,0xe2800002,0xe1a0e004,0xe12fff1e] "returned"
 calleePop<-artifact root "callee-pop" [0xe92d4800,0xe2800001,0xe8bd8800]
 let popc'=merge popc[("artifacts",arr(list(get popc "artifacts")++[calleePop])),("regions",arr(list(get popc "regions")++[region "callee" 0x1040 12 "rx"(Just calleePop)])),("functions",arr(list(get popc "functions")++[obj[("address",h 0x1040),("size",n 12)]]))]
 finish$nest "expected"(set "memory"(arr[memory "stack" 24 "000000000c100000"]))$expectedRegs[("r0",h 9),("r4",h 0x1080),("pc",h 0x1080)]$nest "registers"(set "r0"(h 6))popc'
 popu<-make "unknown-pop-return" [0xe1a0400e,0xeb00000c,0xe2800002,0xe1a0e004,0xe12fff1e] "unknown_call"
 calleeBad<-artifact root "callee-pop-bad" [0xe92d4800,0xe2800001,0xe58d0004,0xe8bd8800]
 let popu'=merge popu[("artifacts",arr(list(get popu "artifacts")++[calleeBad])),("regions",arr(list(get popu "regions")++[region "callee" 0x1040 16 "rx"(Just calleeBad)])),("functions",arr(list(get popu "functions")++[obj[("address",h 0x1040),("size",n 16)]]))]
 finish$nest "expected"(set "memory"(arr[memory "stack" 24 "0000000007000000"]))$expectedRegs[("r0",h 7),("r4",h 0x1080),("sp",h 0x3018),("lr",h 0x100c),("pc",h 0x104c)]$nest "registers"(set "r0"(h 6))popu'
 tail<-make "tail-transfer" [0xe1a0400e,0xe3a00000,0xeb000004,0xe2800007,0xe1a0e004,0xe12fff1e,0x00000000,0x00000000,0xe3a01011,0xea000001,0x00000000,0x00000000,0xe2811001,0xe12fff1e,0x00000000,0x00000000] "returned"
 finish$expectedRegs[("r0",h 7),("r1",h 0x12),("r4",h 0x1080),("lr",h 0x1080),("pc",h 0x1080)]$nest "functions"(\v->arr[obj[("address",h 0x1000),("size",n 0x20)],obj[("address",h 0x1020),("size",n 0x10)],obj[("address",h 0x1030),("size",n 0x10)]])$nest "regions"(dropRegion "data")$at "regions" 0(set "address"(h 0x1000))$set "entry"(h 0x1000)$nest "registers"(\v->merge v[("r0",h 0),("r1",h 0),("r2",h 0)])tail
 tout<-make "tail-outside" [0xea0003fd,0xe12fff1e] "fetch_guard"
 finish$expectedRegs[("r0",h 0xfffffffe),("r1",h 5),("pc",h 0x2000)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 4)]])$nest "regions"(dropRegion "data")$set "entry"(h 0x1004)tout
 tund<-make "tail-undeclared" [0xeaffffff,0x00000000,0xe12fff1e] "unknown_call"
 finish$expectedRegs[("r0",h 0xfffffffe),("r1",h 5),("pc",h 0x1008)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 4)]])$set "entry"(h 0x1004)tund
 bxt<-make "bx-tail-transfer" [0xe1a0400e,0xe3a00000,0xeb000004,0xe2800007,0xe1a0e004,0xe12fff1e,0x00000000,0x00000000,0xe3a01011,0xe301c030,0xe12fff1c,0x00000000,0xe2811001,0xe12fff1e,0x00000000,0x00000000] "returned"
 finish$expectedRegs[("r0",h 7),("r1",h 0x12),("r4",h 0x1080),("r12",h 0x1030),("lr",h 0x1080),("pc",h 0x1080)]$nest "functions"(\v->arr[obj[("address",h 0x1000),("size",n 0x20)],obj[("address",h 0x1020),("size",n 0x10)],obj[("address",h 0x1030),("size",n 0x10)]])$nest "regions"(dropRegion "data")$at "regions" 0(set "address"(h 0x1000))$set "entry"(h 0x1000)$nest "registers"(\v->merge v[("r0",h 0),("r1",h 0),("r2",h 0)])bxt
 bxo<-make "bx-tail-outside" [0xe302c000,0xe12fff1c,0xe12fff1e] "fetch_guard"
 finish$expectedRegs[("r12",h 0x2000),("pc",h 0x2000)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 8)]])$nest "regions"(dropRegion "data")$set "entry"(h 0x1004)bxo
 bxn<-make "bx-tail-nonexec" [0xe302c004,0xe12fff1c,0xe12fff1e] "fetch_guard"
 finish$expectedRegs[("r12",h 0x2004)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 8)]])$set "entry"(h 0x1004)bxn
 bxu<-make "bx-tail-undeclared" [0xe301c00c,0xe12fff1c,0xe12fff1e] "unknown_call"
 finish$expectedRegs[("r12",h 0x100c),("pc",h 0x100c)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 8)]])$set "entry"(h 0x1004)bxu
 bxtt<-make "unsupported-bx-thumb" [0xe12fff15] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]$nest "registers"(\v->merge v[("r5",h 0x1041)])bxtt
 bxtp<-make "unsupported-bx-pc" [0xe12fff1f] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]bxtp
 bxua<-make "unsupported-bx-unaligned" [0xe301c006,0xe12fff1c,0xe12fff1e] "unsupported_instruction"
 finish$expectedRegs[("r12",h 0x1006),("pc",h 0x1008)]$nest "functions"(\v->arr[obj[("address",h 0x1004),("size",n 8)]])$set "entry"(h 0x1004)bxua
 hp<-make "halfword-postindex" [0xe0d210b2,0xe0c200b2,0xe12fff1e] "returned"
 hpData<-artifact root "halfword-postindex-data" [0x0000f000]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "00f07856"]))$expectedRegs[("r1",h 0xf000),("r2",h 0x2008),("pc",h 0x1080)]$set "artifacts"(arr(list(get hp "artifacts")++[hpData]))$at "regions" 2(set "initial"(obj[("artifact",get hpData "id"),("offset",n 0),("length",n 4)]))$nest "registers"(\v->merge v[("r0",h 0x12345678),("r1",h 0xffffffff)])hp
 bfb<-make "bfi-bfc" [0xe7c72211,0xe7cb241f,0xe7df0e11,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0xb0000000),("r1",h 0xab),("r2",h 0xf0b0),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0),("r1",h 0xab),("r2",h 0xff00)])bfb
 sbx<-make "sbfx" [0xe7ab0251,0xe12fff1e] "returned"
 finish$expectedRegs[("r0",h 0xfffff800),("r1",h 0x8000),("pc",h 0x1080)]$nest "registers"(set "r1"(h 0x8000))sbx
 bfu<-make "unsupported-bfi-unpredictable" [0xe7c40411] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]bfu
 bfa<-make "unsupported-bitfield-unallocated" [0xe7e20111] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]bfa
 lht<-make "unsupported-ldrht" [0xe0f200b2] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]lht
 sht<-make "unsupported-strht" [0xe0e200b2] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]sht
 lsbU<-make "unsupported-ldrsb-postindex" [0xe0d200d2] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]lsbU
 lshU<-make "unsupported-ldrsh-postindex" [0xe0d200f2] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]lshU
 lsbP<-make "ldrsb-preindexed" [0xe1d210d2,0xe12fff1e] "returned"
 lsbPData<-artifact root "ldrsb-preindexed-data" [0x00800000]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "00008000"]))$expectedRegs[("r1",h 0xffffff80),("r2",h 0x2004),("pc",h 0x1080)]$set "artifacts"(arr(list(get lsbP "artifacts")++[lsbPData]))$at "regions" 2(set "initial"(obj[("artifact",get lsbPData "id"),("offset",n 0),("length",n 4)]))lsbP
 lshP<-make "ldrsh-preindexed" [0xe1d210f2,0xe12fff1e] "returned"
 lshPData<-artifact root "ldrsh-preindexed-data" [0xabcd0000]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "0000cdab"]))$expectedRegs[("r1",h 0xffffabcd),("r2",h 0x2004),("pc",h 0x1080)]$set "artifacts"(arr(list(get lshP "artifacts")++[lshPData]))$at "regions" 2(set "initial"(obj[("artifact",get lshPData "id"),("offset",n 0),("length",n 4)]))lshP
 lrt<-make "unsupported-ldrt" [0xe4b20004] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]lrt
 dld<-make "ldrd" [0xe1c240d8,0xe1e260dc,0xe12fff1e] "returned"
 dldData<-artifact root "ldrd-data" [0x11111111,0x22222222,0x33333333,0x44444444,0x55555555,0x66666666]
 finish$expectedRegs[("r2",h 0x2010),("r4",h 0x33333333),("r5",h 0x44444444),("r6",h 0x44444444),("r7",h 0x55555555),("pc",h 0x1080)]$set "artifacts"(arr(list(get dld "artifacts")++[dldData]))$at "regions" 2(set "size"(n 24) . set "initial"(obj[("artifact",get dldData "id"),("offset",n 0),("length",n 24)]))dld
 dsd<-make "strd" [0xe1c240f8,0xe1e260fc,0xe12fff1e] "returned"
 dsdData<-artifact root "strd-data" [0x11111111,0x22222222,0x33333333,0x44444444,0x55555555,0x66666666]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "11111111222222220300aaaa0500aaaa0600aaaa66666666"]))$expectedRegs[("r2",h 0x2010),("pc",h 0x1080)]$set "artifacts"(arr(list(get dsd "artifacts")++[dsdData]))$at "regions" 2(set "size"(n 24) . set "initial"(obj[("artifact",get dsdData "id"),("offset",n 0),("length",n 24)]))$nest "registers"(\v->merge v[("r4",h 0xaaaa0003),("r5",h 0xaaaa0004),("r6",h 0xaaaa0005),("r7",h 0xaaaa0006)])dsd
 dbb<-make "unsupported-ldrd-base-in-pair" [0xe1e440d8] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]dbb
 dpc<-make "unsupported-ldrd-pc-operand" [0xe1c2e0d8] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]dpc
 rdg<-make "register-dispatch" [0xe301201c,0xe792f000,0xe3a00009,0xe792f101,0xe3a00002,0xe12fff1e,0x00001010,0x00001014] "returned"
 finish$expectedRegs[("r0",h 2),("r1",h 1),("r2",h 0x101c),("pc",h 0x1080)]$nest "registers"(\v->merge v[("r0",h 0),("r1",h 1)])rdg
 rdo<-make "register-dispatch-outside" [0xe3012014,0xe792f000,0xe12fff1e,0x00000000,0x00001040,0x00000000] "unknown_call"
 finish$expectedRegs[("r2",h 0x1014),("pc",h 0x1040)]$at "regions" 0(set "size"(n 64))$nest "registers"(set "r0"(h 0))rdo
 forM_ [("register",0xe792f101),("pc-relative",0xe79ff101)] $ \(form,word)->do
  forM_ [("thumb",0x1011),("unaligned",0x1012)] $ \(state,target)->do
   dispatch<-make ("unsupported-"++form++"-dispatch-"++state) [word,0xe12fff1e,target,0x0000f8c3,0xbf00bf00] "unsupported_instruction"
   finish$nest "expected"(set "memory"(arr[memory "data" 0 "00000000"]))$expectedRegs[("pc",h 0x1004)]$nest "registers"(\v->merge v[("r1",h 0),("r2",h 0x100c),("r3",h 0x2004)])dispatch
 dpo<-make "unsupported-dispatch-pc-operand" [0xe792f00f] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]dpo
 dwb<-make "unsupported-dispatch-writeback" [0xe7b2f000] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]dwb
 rwb<-make "register-offset-writeback" [0xe7b20001,0xe7e25103,0xe12fff1e] "returned"
 rwbData<-artifact root "register-offset-writeback-data" [0x11111111,0x22222222,0x33333333,0x44444444]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "111111112222222233333333aa444444"]))$expectedRegs[("r0",h 0x22222222),("r1",h 4),("r2",h 0x2010),("r3",h 2),("r5",h 0xaa),("pc",h 0x1080)]$set "artifacts"(arr(list(get rwb "artifacts")++[rwbData]))$at "regions" 2(set "size"(n 16) . set "initial"(obj[("artifact",get rwbData "id"),("offset",n 0),("length",n 16)]))$nest "registers"(\v->merge v[("r1",h 4),("r3",h 2),("r5",h 0xaa)])rwb
 rwd<-make "unsupported-register-offset-writeback-rd" [0xe7b22003] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]rwd
 rwu<-make "unsupported-register-offset-unprivileged" [0xe6b20001] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]rwu
 rws<-make "register-offset-writeback-subtract" [0xe7713082,0xe12fff1e] "returned"
 rwsData<-artifact root "register-offset-writeback-subtract-data" [0x11111111,0x22222222,0x33333333,0x44444444]
 finish$expectedRegs[("r1",h 0x2004),("r2",h 0xffffffff),("r3",h 0x11),("pc",h 0x1080)]$set "artifacts"(arr(list(get rws "artifacts")++[rwsData]))$at "regions" 2(set "size"(n 16) . set "initial"(obj[("artifact",get rwsData "id"),("offset",n 0),("length",n 16)]))$nest "registers"(\v->merge v[("r1",h 0x2002),("r2",h 0xffffffff)])rws
 rsg<-make "register-offset-subtract-guard" [0xe7520003,0xe12fff1e] "memory_guard"
 finish$expectedRegs[("pc",h 0x1004)]$nest "registers"(\v->merge v[("r2",h 0x2004),("r3",h 0x10000)])rsg
 hrw<-make "halfword-register-writeback" [0xe1b250b3,0xe1a260b4,0xe12fff1e] "returned"
 hrwData<-artifact root "halfword-register-writeback-data" [0x11112222,0x33334444]
 finish$nest "expected"(set "memory"(arr[memory "data" 0 "22221111cdab3333"]))$expectedRegs[("r2",h 0x2008),("r5",h 0x1111),("r6",h 0xabcd),("pc",h 0x1080)]$set "artifacts"(arr(list(get hrw "artifacts")++[hrwData]))$at "regions" 2(set "size"(n 8) . set "initial"(obj[("artifact",get hrwData "id"),("offset",n 0),("length",n 8)]))$nest "registers"(\v->merge v[("r3",h 2),("r4",h 2),("r6",h 0xabcd)])hrw
 hrd<-make "unsupported-halfword-register-writeback-rd" [0xe1b220b3] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]hrd
 forM_ [("unsupported-word-store-writeback-base",0xe5a22004),("unsupported-register-store-writeback-base",0xe7a22003),("unsupported-halfword-store-writeback-base",0xe1e220b2)] $ \(name,word)->do
  item<-make name [word] "unsupported_instruction"
  finish$nest "expected"(set "memory"(arr[memory "data" 0 "00000000"]))$expectedRegs[("pc",h 0x1004)]item
 unalignedBlx<-make "unsupported-blx-unaligned" [0xe12fff30] "unsupported_instruction"
 finish$expectedRegs[("pc",h 0x1004)]$nest "registers"(set "r0"(h 0x1042))unalignedBlx
 forM_ [("immediate-subtract-wrap",0xe5120004,0xfffffffc,[("r0",h 0x12345678)]),("halfword-subtract-wrap",0xe15210b4,0xfffffffc,[("r1",h 0x5678)]),("doubleword-subtract-wrap",0xe14240d8,0xfffffff8,[("r4",h 0x12345678),("r5",h 0xabcdef01)])] $ \(name,word,address,changes)->do
  item<-make name [word,0xe12fff1e] "returned"
  dat<-artifact root (name++"-data") (if address==0xfffffff8 then [0x12345678,0xabcdef01] else [0x12345678])
  finish$expectedRegs(changes++[("pc",h 0x1080)])$set "artifacts"(arr(list(get item "artifacts")++[dat]))$at "regions" 2(set "address"(h address) . set "size"(get dat "size") . set "initial"(obj[("artifact",get dat "id"),("offset",n 0),("length",get dat "size")]))$nest "registers"(set "r2"(h 0))item
 generated<-length.filter((==".json").takeExtension)<$>listDirectory root
 putStrLn("Generated "++show generated++" structurally identical independent cases and exact instruction binaries; JSON uses canonical Haskell serialization. Historical JSON bytes were preserved.")
