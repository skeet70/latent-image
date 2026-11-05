{-# LANGUAGE GHC2021 #-}
module Main where

import Data.Aeson (Value(..))
import Control.Monad
import Data.Bits
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (nub)
import Data.Word
import Emulation.NativeCase (runCase, runnerDigest, byteString)
import Emulation.OwnedCase (assemble, readWordAt)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
import System.Process
import System.IO (hIsEOF)

newtype Command = Command Word32 deriving Show
newtype PacketPayload = PacketPayload B.ByteString deriving Show
data Input = Input { command :: Command, argument :: Word32, modifier :: Word32, payload :: PacketPayload, held :: Bool, receiveStatus :: Word32, predicateA :: Word32, predicateB :: Word32, suppress :: Word32, pendingBits :: Int } deriving Show
data RunMode = Once | Actor | Backedge deriving Eq

addr :: Word32 -> Value
addr x = s ("0x" ++ replicate (8-length h) '0' ++ h) where h=showHex x ""
zero :: Value
zero=obj []
reg :: [(String,Word32)] -> Value
reg xs=obj [(k,addr v)|(k,v)<-xs]
word :: Word32 -> B.ByteString
word x=assemble [x]
patch :: Int -> B.ByteString -> B.ByteString -> B.ByteString
patch at value bytes=B.take at bytes<>value<>B.drop (at+B.length value) bytes

stateRanges :: [(Int,Int)]
stateRanges=[(0x944580,80),(0x949dd4,4),(0x94f5e0,4),(0x950098,0xb4),(0x951528,4),(0x95200c,4),(0x954d68,4),(0x972e84,4)]

services :: [Word32]
services=[0xbfb124,0xc22bd0,0xd3cbd4,0x63270c,0xd0b318,0xd0b438,0xd0c954,0xd35f3c,0xd5cf50,0xd3ad34,0xd3ad10,0xd3b5ec,0xd19d18,0x636004,0xd1734c,0xd17370,0xd3b0dc,0xd3ab00,0xd3ad5c,0xd36ea4,0xd36e60,0xd35e14,0xd0cd10,0xd36310,0x633594,0xd48240,0xd38130]

-- Stock id5.dis 50482-50680 and 9635-9671, x70view f659f02b850fcba58de0f9b09ee24ad09a24c2f4.
oracle :: Input -> (B.ByteString,[Word32])
oracle i = (final, prefix++pendingCalls++receivedCalls++after)
 where
  Command raw=command i
  normalized=raw .&. complement 0xc0
  low=normalized<0x600
  cmd=if low then normalized else raw
  flags=if low then raw .&. 0xc0 else modifier i
  bit n=testBit (pendingBits i) n
  choice=if held i then Nothing else case [n|n<-[0..5],bit n] of []->Nothing;n:_->Just n
  pendingCalls=if held i then [] else case choice of
   Just 0->[0xd3b5ec]
   Just 1->[0xd19d18,0x636004]
   Just 2->[0xd1734c,0xd17370]
   Just 3->[0xd1734c,0xd3b0dc]
   Just 4->[0xd1734c,0xd3ab00]
   Just 5->[0xd1734c,0xd3ab00,0xd3ad5c]
   _->[0xd1734c,0xd3ab00,0xd3ad5c]
  received=choice==Nothing
  accepted=not received || (receiveStatus i==0 && (predicateA i==0 || predicateB i==0 || cmd `elem` [6,0x204,0x210,0x11,0xe]) && suppress i/=1 && (not(held i)||cmd `elem` [0x837,0x838]))
  receivedCalls=if not received then [] else [0xdb5124]++(if receiveStatus i/=0 then [] else [0xd0b318|low]++[0xd0b438,0xd0c954]++[0xd35f3c|predicateA i/=0]++[0xd5cf50]++[if cmd `elem` [0x837,0x838] then 0xd3ad34 else 0xd3ad10|held i])
  actualCmd=if received then cmd else raw
  actualFlags=if received then flags else modifier i
  prefix=[0xbfb124,0xc22bd0,0xd3cbd4,0x63270c]
  after=if accepted then [0xd36ea4,0xd36e60,0xd35e14]++[0xd0cd10|actualCmd==9 && actualFlags==0] else []
  initial=initialState i
  changes=[(0x94f5e0,case choice of Just 4->2;Just 5->1;_->0)]++[(0x950148,0)|choice==Just 0]++(if accepted then [(0x944580,actualCmd),(0x944588,actualFlags .|. 0xa5a50000)]++[(0x95200c,actualCmd)|actualCmd>=0x200 && actualCmd<=0x203] else [(0x944580,0),(0x944584,0),(0x944588,0xa5a50000)])
  final=foldl (\bs (a,v)->patch (a-0x940000) (word v) bs) initial changes

initialState :: Input -> B.ByteString
initialState i=foldl (\bs (a,v)->patch (a-0x940000) (word v) bs) (patch (0x94458c-0x940000) bytes (B.replicate 0x40000 0))
 [(0x944580,c),(0x944584,argument i),(0x944588,modifier i .|. 0xa5a50000),(0x949dd4,if held i then 1 else 0),(0x950148,flag 0),(0x951528,flag 1),(0x950098,flag 3),(0x95200c,0xdeadbeef)]
 where Command c=command i;PacketPayload bytes=payload i;flag n=if testBit (pendingBits i) n then 1 else 0

baseInput :: Command -> Input
baseInput c=Input c 0x12345678 0 (PacketPayload(B.replicate 68 0x5a)) False 0 0 0 0 0

main :: IO ()
main=do
 args<-getArgs
 case args of
  [runner,stockPath,candidatePath,retained,composed,out]->qualify runner stockPath candidatePath retained composed out
  _->die "usage: host-cases RUNNER STOCK-ID5 CANDIDATE-FLAT RETAINED.json.gz COMPOSED.json.gz OUTPUT"

packets :: FilePath -> IO [Input]
packets path=withCreateProcess (proc "gzip" ["-dc",path]){std_out=CreatePipe} $ \_ output _ process->do
 stream<-maybe (failH "gzip stdout unavailable") pure output
 let collect blocks=do
      ended<-hIsEOF stream
      if ended then pure blocks else do
       row<-BC.hGetLine stream >>= jsonRead
       let sample=get row "sample"
           found=[str(get p "physical")|str(get(get sample "site")"kind")=="queue",p<-list(get sample "packet"),length(str(get p "physical"))==152]
       collect (nub(blocks++found))
 blocks<-collect []
 status<-waitForProcess process
 ensure(status==ExitSuccess) ("gzip failed: "++path)
 ensure(not(null blocks)) ("no 76-byte queue packets in "++path)
 forM blocks $ \hex->do
  bytes<-byteString(s hex)
  pure((baseInput(Command(readWordAt bytes 0))){argument=readWordAt bytes 4,payload=PacketPayload(B.drop 8 bytes)})

qualify :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
qualify runner stockPath candidatePath retained composed out=do
 createDirectoryIfMissing True out
 stock<-B.readFile stockPath
 candidate<-B.readFile candidatePath
 recorded<- (++) <$> packets retained <*> packets composed
 let commands=[0,6,9,0x49,0x89,0xc9,0xe,0x11,0x200,0x201,0x202,0x203,0x204,0x210,0x5ff,0x600,0x637,0x837,0x838,0x839,0xffffffff]
     bounded=[(baseInput(Command c)){held=h,receiveStatus=st,predicateA=a,predicateB=b,suppress=su}|c<-commands,h<-[False,True],st<-[0,1,0xffffffff],(a,b)<-[(0,1),(1,0),(1,1)],su<-[0,1,2]]
     priorities=[(baseInput(Command 9)){pendingBits=mask}|mask<-[0..63]]
     inputs=recorded++bounded++priorities
     slice a z bytes base=B.take z(B.drop (a-base) bytes)
     stockActor=slice 0xc22f60 0x31c stock 0xbf1a00
     candidateActor=slice 0xc22f60 0x31c candidate 0xbfb124
     stockOnce=slice 0xbfb124 0x94 stock 0xbf1a00
     candidateOnce=slice 0xbfb124 0x94 candidate 0xbfb124
 results<-forM (zip [0::Int ..] inputs) $ \(index,input)->do
  let ident="actor-"++show index
  a<-execute runner out (ident++"-stock") stockActor input Actor 0
  b<-execute runner out (ident++"-candidate") candidateActor input Actor 0
  ensure(a&&b) ("required actor contract failed: "++ident)
  pure(a&&b)
 onceResults<-forM [(alloc,result)|alloc<-[0,0x980000],result<-[0,1,0xffffffff]] $ \(alloc,result)->do
  let input=(baseInput(Command alloc)){receiveStatus=result}
  a<-execute runner out ("once-stock-"++show alloc++"-"++show result) stockOnce input Once alloc
  b<-execute runner out ("once-candidate-"++show alloc++"-"++show result) candidateOnce input Once alloc
  ensure(a&&b) ("required once contract failed: allocation="++show alloc++" result="++show result)
  pure(a&&b)
 backedges<-forM [("backedge-stock",stockActor),("backedge-candidate",candidateActor)] $ \(ident,body)->execute runner out ident body (baseInput(Command 9)) Backedge 0
 mutant<-execute runner out "negative-entry-bypass" (patch 0 (assemble [0xe12fff1e]) candidateActor) (baseInput(Command 9)) Actor 0
 unsupported<-execute runner out "negative-unsupported" (patch 0 (assemble [0xe7f000f0]) candidateActor) (baseInput(Command 9)) Actor 0
 mutantReceipt<-if mutant then pure zero else B.readFile(out</>"negative-entry-bypass"</>"report.json") >>= jsonRead
 unsupportedReceipt<-if unsupported then pure zero else B.readFile(out</>"negative-unsupported"</>"report.json") >>= jsonRead
 runnerHash<-runnerDigest runner
 hostBytes<-getExecutablePath >>= B.readFile
 let mutantRejected=not mutant && str(get(get mutantReceipt "runner")"status")=="returned"
     unsupportedRejected=not unsupported && str(get(get unsupportedReceipt "runner")"status")=="unsupported_instruction"
     passed=and(results++onceResults++backedges)&&mutantRejected&&unsupportedRejected
     summary=obj
       [("passed",b passed),("recorded_packets",n(fromIntegral(length recorded))),("actor_cases",n(fromIntegral(length inputs))),("once_cases",n 6),("backedge_cases",n 1)
       ,("mutant_rejected",b mutantRejected),("unsupported_rejected",b unsupportedRejected),("candidate_sha256",s(sha256Hex(sha256 candidate))),("stock_sha256",s(sha256Hex(sha256 stock))),("runner_sha256",s runnerHash),("host_binary_sha256",s(sha256Hex(sha256 hostBytes)))
       ,("receipt_inputs",arr(map s [retained,composed])),("execution_policy",obj[("grant",s "Integration owner permits bounded native execution outside full-system slots"),("memory_max_bytes",n 2147483648),("native_jobs",n 1),("full_system_boot",b False)])
       ,("limitations",arr(map s ["Retained services are fixture wrappers and registers-only stubs; no scheduler, peripheral, service-body or full-system qualification.","Actor matrix stops at pump; one diagnostic backedge case stops at second loop-service entry. Actor scratch registers and private stack bytes are excluded; once-duty SP and r4-r11 are checked.","Deduplicated 76-byte queue snapshots are injected state, not natural reachability. Marker events are normalized to retained execution/receive/register addresses."]))]
 B.writeFile (out</>"summary.json") (encodeCanonical summary)
 unless passed(exitFailure)

execute :: FilePath -> FilePath -> String -> B.ByteString -> Input -> RunMode -> Word32 -> IO Bool
execute runner out ident body input mode allocation=do
 let dir=out</>ident
 createDirectoryIfMissing True dir
 let once=mode==Once
     backedge=mode==Backedge
     entry=if once then 0xbfb124 else 0xc22f60
     stop=0x100000
     expectedStatus=if once then "returned" else "stop_address"
     expectedPc=case mode of Actor->0xbfbd20;Backedge->0x104000;Once->stop
     bit n=if testBit(pendingBits input)n then 1 else 0
     response a=case a of 0x102000->receiveStatus input;0x103000->receiveStatus input;0xd0c954->predicateA input;0xd35f3c->predicateB input;0xd5cf50->suppress input;0xd1734c->bit 2;0xd3ab00->bit 4;0xd3ad5c->bit 5;0xd36310->allocation;_->0
     stub a=obj[("address",addr a),("inputs",reg(case a of 0x101000->[("r0",0),("r1",0),("r2",0),("r3",0)];0xd36310->[("r0",0x1000)];0xd38130->[("r0",0)];0xd3ad10->[("r0",0)];0x636004->[("r0",0)];_->[])),("outputs",reg[("r0",response a)]),("side_effects",s "registers-only")]
     -- The wrappers copy service argument records into a fixed observation region.
     copyWrapper base marker count dest=assemble ([0xe92d4010,0xe1a03000,0xe59f2000 .|. fromIntegral(8*count+4)]++concat[[0xe5931000 .|. fromIntegral(4*k),0xe5821000 .|. fromIntegral(4*k)]|k<-[0..count-1]]++[0xeb000000 .|. fromIntegral (((marker::Integer)-(base+fromIntegral(4*(3+2*count))+8)) `div` 4 .&. 0xffffff),0xe8bd8010,dest])
     executionCapture=assemble [0xe92d4010,0xe59d4008,0xe92d0003,0xe59f000c,0xe5804000,0xe8bd0003,0xeb000000 .|. fromIntegral (((0x101000::Integer)-(0x63270c+24+8)) `div` 4 .&. 0xffffff),0xe8bd8010,0x980044]
     wrappers=(if once then [(0xdb4b34,copyWrapper 0xdb4b34 0x103000 5 0x980000)] else [(0xdb5124,copyWrapper 0xdb5124 0x102000 4 0x980000),(0x63270c,executionCapture)])++[(0xd3cbd4,assemble [0xe59f2018,0xe5921000,0xe2811001,0xe5821000,0xe3510002,0x0b000000 .|. fromIntegral (((0x104000::Integer)-(0xd3cbd4+20+8)) `div` 4 .&. 0xffffff),0xe12fff1e,0xe320f000,0x980040,stop])|backedge]++[(expectedPc,assemble [0xe12fff1e])|not once]
     stubs=filter (\a->a/=0x63270c && (not once || a `elem` [0xd36310,0x633594,0xd48240,0xd38130]) && (not backedge || a/=0xd3cbd4)) services++[0xbfbd20|backedge]++(if once then [0x103000] else [0x101000,0x102000])
     (expectedState,sequence')=oracle input
     expectedCalls=if once then [0xd36310,0xdb4b34]++[0x633594|receiveStatus input/=0]++[0xd48240,0xd38130] else if backedge then filter (/=0xd3cbd4) sequence'++[0xbfbd20] else sequence'
     initial=if once then B.replicate 0x40000 0 else initialState input
     states=[("state-"++show ix,a,B.take size(B.drop(a-0x940000)initial))|(ix,(a,size))<-zip [0::Int ..] stateRanges]
     artifacts=[("body",body),("observed",patch 68 (word 0xdeadbeef) (B.replicate 256 0)),("exit",assemble [0xe12fff1e])]++[(name,bytes)|(name,_,bytes)<-states]++[("wrapper"++show ix,bytes)|(ix,(_,bytes))<-zip [0::Int ..] wrappers]
 forM_ artifacts $ \(name,bytes)->B.writeFile(dir</>name<.>"bin")bytes
 let artifact(name,bytes)=obj[("id",s name),("path",s(name<.>"bin")),("size",n(fromIntegral(B.length bytes))),("sha256",s(sha256Hex(sha256 bytes)))]
     region name a size perm source=obj[("id",s name),("address",addr a),("size",n size),("permissions",s perm),("initial",maybe(obj[("zero",b True)]) (\key->obj[("artifact",s key),("offset",n 0),("length",n(fromIntegral(B.length(maybe B.empty id(lookup key artifacts)))))] ) source)]
     regions=[region "body" entry (fromIntegral(B.length body)) "rx" (Just "body"),region "stack" 0x200000 0x1000 "rw" Nothing,region "observed" 0x980000 0x100 "rw" (Just "observed"),region "exit" stop 4 "rx" (Just "exit")]++[region name (fromIntegral a) (fromIntegral(B.length bytes)) "rw" (Just name)|(name,a,bytes)<-states]++[region ("wrapper"++show ix) a (fromIntegral(B.length bytes)) "rx" (Just("wrapper"++show ix))|(ix,(a,bytes))<-zip [0::Int ..] wrappers]
     function a size=obj[("address",addr a),("size",n size)]
     spec=obj[("schema_version",n 1),("id",s ident),("origin",s "stock"),("provenance",s "X-M1 1.50 stock instruction contract, id5.dis 50482-50680 and 9635-9671; x70view f659f02b850fcba58de0f9b09ee24ad09a24c2f4"),("cpu",obj[("architecture",s "arm"),("model",s "cortex-r5f"),("mode",s "arm"),("endian",s "little")]),("entry",addr entry),("exit",addr stop),("registers",reg[(r,case r of "sp"->0x201000;"lr"->stop;"cpsr"->0x10;_->0)|r<-map (("r"++).show) [0::Int ..12]++["sp","lr","cpsr"]]),("regions",arr regions),("artifacts",arr(map artifact artifacts)),("functions",arr(function entry (fromIntegral(B.length body)):[function a (fromIntegral(B.length bytes))|(a,bytes)<-wrappers])),("stubs",arr(map stub stubs)),("limits",obj[("instructions",n 10000),("wall_ms",n 1000)]),("expected",obj[("status",s "returned"),("registers",reg[("pc",stop)]),("memory",arr []),("events",arr [])]),("exclusions",arr[s "Stack and scratch registers excluded; retained service implementations are not executed."])]
 let calleeSaved=[("r"++show ix,0x44440000+fromIntegral ix*0x111111)|ix<-[4::Int ..11]]
     initialRegisters=foldl (\rs (name,value)->set name (addr value) rs) (set "cpsr" (addr 0x53) (get spec "registers")) calleeSaved
     admitted=set "limits" (obj[("instructions",n 10000),("wall_ms",n 10000)]) (set "kernel" (if once then zero else obj[("stop",addr expectedPc)]) (set "registers" initialRegisters spec))
 B.writeFile(dir</>"case.json")(encodeCanonical admitted)
 (_,report)<-runCase runner (dir</>"case.json")
 let observed=get report "runner"
     memory a=case [v|v<-list(get observed "memory"),get v "address"==addr a]of v:_->v;_->Null
     actualCalls=map (getAddress . getEventAddress) (list(get observed "events"))
     getEventAddress e=get e "address"
     getAddress v=maybe (str v) (str.addr) (lookup v [(addr 0x101000,0x63270c),(addr 0x102000,0xdb5124),(addr 0x103000,0xdb4b34)])
     status=str(get observed "status")
 actualState<-if status==expectedStatus then foldM (\bytes (a,_)->do chunk<-byteString(get(memory(fromIntegral a))"bytes");pure(patch(a-0x940000)chunk bytes)) (B.replicate 0x40000 0) stateRanges else pure B.empty
 observedBytes<-if status==expectedStatus then byteString(get(memory 0x980000)"bytes") else pure B.empty
 let record=if once then assemble [0x918ab8,0x93bea0,0x96b400,0x95b098,allocation] else assemble [0x107,0x944580,0x944584,0x94458c]
     receiveNeeded=held input || pendingBits input==0
     stateOk=if once then actualState==patch (0x972e84-0x940000) (word 1) (patch (0x954d68-0x940000) (word(receiveStatus input)) (B.replicate 0x40000 0)) else actualState==expectedState
     onceAbiOk=not once || all (\(name,value)->get (get observed "registers") name==addr value) (("sp",0x201000):calleeSaved)
     executionArgsOk=once || (B.length observedBytes>=72 && readWordAt observedBytes 68==0)
     passed=status==expectedStatus && get (get observed "registers") "pc"==addr expectedPc && stateOk && onceAbiOk && executionArgsOk && actualCalls==map (str.addr) expectedCalls && (not(once||receiveNeeded)||B.take(B.length record)observedBytes==record) && (not backedge || (B.length observedBytes>=68 && readWordAt observedBytes 64==2))
 let classification=if status `notElem` ["returned","stop_address"] then "unsupported" else if passed then "passed" else "failed"
 B.appendFile (out</>"results.jsonl") (encodeCanonical(obj[("id",s ident),("input",s(show input)),("status",s classification),("runner_status",s status),("events",get observed "events"),("contract_passed",b passed)])<>BC.pack "\n")
 if passed then removeDirectoryRecursive dir else B.writeFile(dir</>"report.json")(encodeCanonical(set "contract_passed" (b passed) report))
 pure passed
