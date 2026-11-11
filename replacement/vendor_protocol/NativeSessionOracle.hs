{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value(..))
import Data.Bits
import qualified Data.ByteString as B
import Data.Word
import qualified Emulation.NativeCase as Native
import Emulation.OwnedCase (assemble, readWordAt)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import SessionStep (runBoundedGroup)
import System.Directory
import System.Environment
import System.Exit (ExitCode(..), exitFailure)
import System.FilePath

newtype Tick = Tick Word32 deriving Show
newtype SessionToken = SessionToken Word16 deriving Show
data Opcode = Open | Validate | Close | Command | Reply | Upload | Download | Empty | Reset | Unknown deriving (Eq, Show)
data Frame = Frame Opcode Word32 Word32 SessionToken deriving Show
data Observation = Observation [Word32] B.ByteString deriving (Eq, Show)
data Block = Block String Word32 String B.ByteString

frameBytes :: Frame -> B.ByteString
frameBytes (Frame opcode offset count (SessionToken token)) = B.pack ([code,0] ++ be offset ++ be count ++ [fromIntegral (token `shiftR` 8),fromIntegral token])
 where
  code = case opcode of Open->0xc0; Validate->0xc2; Close->0xc4; Command->0xd0; Reply->0xd1; Upload->0xe0; Download->0xe1; Empty->0xe8; Reset->0x1d; Unknown->0xff
  be value = [fromIntegral (value `shiftR` place) | place <- [24,16,8,0]]

address :: Word32 -> Value
address value = s ("0x" ++ replicate (8-length digits) '0' ++ digits) where digits=showHex value ""
registers :: [(String, Word32)] -> Value
registers values = obj [(name,address value) | (name,value)<-values]
word :: Word32 -> B.ByteString
word value = assemble [value]
putWord :: Int -> Word32 -> B.ByteString -> B.ByteString
putWord at value bytes = B.take at bytes <> word value <> B.drop (at+4) bytes
byteAt :: B.ByteString -> Int -> Word32
byteAt bytes at = fromIntegral (B.index bytes at)
hash :: B.ByteString -> String
hash = sha256Hex . sha256

slice :: Word32 -> Word32 -> Int -> B.ByteString -> B.ByteString
slice base start size bytes = B.take size (B.drop (fromIntegral (start-base)) bytes)

readPinned :: String -> FilePath -> IO B.ByteString
readPinned digest path = do
 bytes <- B.readFile path
 ensure (hash bytes==digest) ("pinned input mismatch: "++path)
 pure bytes

writeNew :: FilePath -> B.ByteString -> IO ()
writeNew path bytes = do
 exists <- doesPathExist path
 ensure (not exists) ("refusing to replace evidence: "++path)
 B.writeFile path bytes

memory :: Value -> Word32 -> IO B.ByteString
memory observation base = case [item | item<-list(get observation "memory"),get item "address"==address base] of
 [item] -> Native.byteString (get item "bytes")
 _ -> failH "native readback region is missing or duplicated"

execute :: FilePath -> FilePath -> String -> Word32 -> [(String,Word32)] -> [Block] -> IO Value
execute runner root ident entry arguments blocks = do
 let stackBase=0x2200000
     stop=0x100000
     blockDescription (Block name base permissions bytes) = obj
      [("id",s name),("address",address base),("size",n(fromIntegral(B.length bytes))),("permissions",s permissions)
      ,("initial",obj[("artifact",s name),("offset",n 0),("length",n(fromIntegral(B.length bytes)))])]
     artifact (Block name _ _ bytes) = obj [("id",s name),("path",s(name<.>"bin")),("size",n(fromIntegral(B.length bytes))),("sha256",s(hash bytes))]
     function (Block name base permissions bytes)
      | permissions/="rx" || base==stop = []
      | name=="stock-session" = [obj[("address",address start),("size",n size)] | (start,size)<-stockFunctions]
      | otherwise = [obj[("address",address base),("size",n(fromIntegral(B.length bytes)))]]
     stockFunctions =
      [(0xd7efd4,236),(0xd7f364,32),(0xd7f384,48),(0xd7f3b4,36),(0xd7f3d8,52)
      ,(0xd7f440,40),(0xd7f468,40),(0xd7f490,156),(0xd7f52c,64),(0xd7f56c,312)
      ,(0xd7f6a4,188),(0xd7f760,172),(0xd7f80c,152),(0xd7f8a4,124),(0xd7f920,140)
      ,(0xd7f9ac,160),(0xd7fa4c,156),(0xd7fae8,228)]
     initialRegisters = [(name,maybe fallback id(lookup name arguments)) | (name,fallback)<-
      [("r"++show index,0) | index<-[0::Int ..12]]++[("sp",stackBase+0x4000),("lr",stop),("cpsr",0x53)]]
     caseValue=obj
      [("schema_version",n 1),("id",s ident),("origin",s "synthetic")
      ,("provenance",s "X-M1 1.50 session and bulk-source CPU comparison; stock images authenticated before extent extraction; no service stubs")
      ,("cpu",obj[("architecture",s "arm"),("model",s "cortex-r5f"),("mode",s "arm"),("endian",s "little")])
      ,("entry",address entry),("exit",address stop)
      ,("registers",registers initialRegisters)
      ,("cp14_identity",address 0x4003)
      ,("artifacts",arr(map artifact allBlocks)),("regions",arr(map blockDescription allBlocks))
      ,("functions",arr(concatMap function allBlocks)),("stubs",arr [])
      ,("limits",obj[("instructions",n 100000),("wall_ms",n 1000)])
      ,("expected",obj[("status",s "returned"),("registers",registers[("pc",stop)]),("memory",arr []),("events",arr [])])
      ,("exclusions",arr(map s ["Session control and bulk source CPU only; no provider or persistent transaction qualification.","No DMA, scheduler, peripheral, full-system or installed-camera qualification."]))]
     allBlocks=blocks++[Block (ident++"-stack") stackBase "rw" (B.replicate 0x4000 0),Block "return" stop "rx" (assemble [0xe12fff1e])]
     casePath=root</>ident<.>"json"
 forM_ allBlocks $ \(Block name _ _ bytes)->do
  let path=root</>name<.>"bin"
  exists<-doesPathExist path
  if exists then do
   existing<-B.readFile path
   ensure(existing==bytes)("evidence artifact name collision: "++path)
  else writeNew path bytes
 writeNew casePath (encodeCanonical caseValue)
 (admitted,_,wire,identity)<-Native.loadCase casePath
 runnerHash<-Native.runnerDigest runner
 executable<-canonicalizePath runner
 (timed,exit,stdout,stderr)<-runBoundedGroup 3000000 (2*1024*1024) executable [] wire
 writeNew (root</>ident<.>"wire") wire
 writeNew (root</>ident<.>"stdout") stdout
 writeNew (root</>ident<.>"stderr") stderr
 let processReceipt=obj[("case",s ident),("runner_sha256",s runnerHash),("wire_sha256",s(hash wire))
      ,("native_stdout_sha256",s(hash stdout)),("runner_exit",n(case exit of ExitSuccess->0;ExitFailure code->fromIntegral code)),("timed_out",b timed)]
 writeNew (root</>ident++"-process.json") (encodeCanonical processReceipt)
 decoded<-try(jsonRead stdout)::IO(Either IOException Value)
 actual<-case decoded of
  Right value->pure value
  Left failure->do
   writeNew(root</>ident++"-decode-error.json")(encodeCanonical(obj[("decode_error",s(show failure)),("process",processReceipt)]))
   failH("native stdout cannot be decoded: "++ident)
 let returned=not timed && exit==ExitSuccess && get actual "wire_version"==n 1 && get actual "status"==s "returned" && get(get actual "registers")"pc"==address stop
     receipt=obj[("case",s ident),("admission",Native.caseReport admitted identity),("runner_sha256",s runnerHash)
      ,("wire_sha256",s(hash wire)),("native_stdout_sha256",s(hash stdout)),("runner_exit",n(case exit of ExitSuccess->0;ExitFailure code->fromIntegral code))
      ,("native_status",get actual "status"),("timed_out",b timed),("returned",b returned),("event_count",n(fromIntegral(length(list(get actual "events")))))]
 writeNew (root</>ident++"-execution.json") (encodeCanonical receipt)
 ensure returned ("native execution refused: "++ident)
 pure actual

candidate :: FilePath -> FilePath -> [Block] -> String -> Tick -> [Frame] -> IO [Observation]
candidate runner root code ident (Tick tick) frames = do
 let input=assemble [0x2000020,fromIntegral(length frames),tick,0x41000000,16]<>B.replicate 12 0<>B.concat(map frameBytes frames)
     output=B.replicate(length frames*564)0
 actual<-execute runner root (ident++"-owned") 0x1000000 [("r0",0x2000000),("r1",0x2100000)]
  (code++[Block(ident++"-input")0x2000000 "r" input,Block(ident++"-output")0x2100000 "rw" output])
 ensure(get(get actual "registers")"r0"==address 0) "owned probe rejected descriptor"
 result<-memory actual 0x2100000
 forM [0..length frames-1] $ \index->do
  let row=B.take 564(B.drop(index*564)result)
      status=readWordAt row 0
      replyLength=fromIntegral(readWordAt row 44)
  ensure(status==0 && replyLength<=512) "owned probe did not complete a bounded session control"
  pure(Observation ([readWordAt row at|at<-[4,8..40]]++[readWordAt row 560]) (B.take replyLength(B.drop 48 row)))

stock :: FilePath -> FilePath -> [Block] -> String -> [Frame] -> IO [Observation]
stock runner root code ident frames = go (0::Int) initial frames
 where
  go :: Int -> B.ByteString -> [Frame] -> IO [Observation]
  initial=putWord 0x2c 16(putWord 0x28 0x41000000(B.replicate 0x450 0))
  stateBase=0x1469850
  go _ _ []=pure []
  go index before (frame@(Frame opcode offset count (SessionToken token)):rest)=do
   let name=ident++"-stock-"++show index
       dynamic=[Block(name++"-frame")0x2000000 "r"(frameBytes frame),Block(name++"-state")stateBase "rw" before,Block "payload"0x41000000 "r"(B.pack[0..15])]
   actual<-execute runner root name 0xd7fae8 [("r0",0x2000000)] (code++dynamic)
   state<-memory actual stateBase
   let active=byteAt state 0x48
       sid=fromIntegral(B.index state 0x4a) .|. (fromIntegral(B.index state 0x4b)`shiftL`8)
       valid=active/=0 && sid==fromIntegral token
       deferred=opcode==Reply && valid && count/=0
       delivery=if deferred then 0 else byteAt state 0x44
       status=if deferred then 0 else fromIntegral(B.index state 0x46) .|. (fromIntegral(B.index state 0x47)`shiftL`8)
       action=if delivery==1 then 0 else case opcode of Open->3;Validate->3;Close->3;Empty->3;Command->1;Upload->2;Download->4;Reply->3;_->0
       requested=readWordAt state 0x18
       armed=byteAt state 0x24
       ready=byteAt state 0x25
       payloadOffset=if action==4 then offset else 0
       sends=action==3 && armed/=0 && ready/=0
   (after,reply,payloadAddress,payloadLength)<-if sends || action==4 then do
    let sourceName=name++"-source"
    source<-execute runner root sourceName 0xd7efd4 [("r0",0x2000100),("r1",0x2000104)]
     (code++[Block(sourceName++"-state")stateBase "rw" state,Block(sourceName++"-span")0x2000100 "rw"(B.replicate 8 0),Block "payload"0x41000000 "r"(B.pack[0..15])])
    ensure(get(get source "registers")"r0"==address 0x3000000) "stock bulk source did not publish reply"
    next<-memory source stateBase
    spanBytes<-memory source 0x2000100
    let pointer=readWordAt spanBytes 0
        length'=fromIntegral(readWordAt spanBytes 4)
    if action==4 then pure(next,B.empty,pointer,fromIntegral length') else do
     ensure(pointer>=stateBase && pointer-stateBase+fromIntegral length'<=fromIntegral(B.length next)) "stock reply escaped mapped state"
     pure(next,B.take length'(B.drop(fromIntegral(pointer-stateBase))next),0,0)
   else pure(state,B.empty,0,0)
   let observed=[delivery,status,action,active,sid,requested,armed,ready,payloadOffset,payloadLength,payloadAddress]
   others<-go(index+1)after rest
   pure(Observation observed reply:others)

scenarios :: [(Tick,[Frame])]
scenarios =
 [(Tick tick,[Frame Open 0 8(SessionToken 0),Frame Validate 0 2(SessionToken expected),Frame Close 0 2(SessionToken expected)])
 |tick<-[0,1,0x1ffe,0x1fff,0xffffffff],let expected=let value=(fromIntegral tick+1).&.0x1fff in if value==0 then 1 else value] ++
 [(Tick 0,[Frame Open 0 8(SessionToken 0),Frame opcode 0 count(SessionToken token)])
 |opcode<-[Open,Validate,Close,Command,Reply,Upload,Empty,Reset,Unknown],(count,token)<-[(0,0),(2,1),(513,2),(0xffffffff,1)]] ++
 [(Tick 0,[Frame Validate 0 count(SessionToken token)])| (count,token)<-[(0,0),(2,1),(3,0xffff)]] ++
 [(Tick 0,[Frame Open 0 8(SessionToken 0),Frame Download offset count(SessionToken 1)]) | (offset,count)<-[(0,1),(15,1),(0,16)]]

main :: IO ()
main=do
 args<-getArgs
 (runner,id4Path,id5Path,loaderPath,ownedPath,root)<-case args of
  [a,b',c,d,e,f]->pure(a,b',c,d,e,f)
  _->failH "usage: native-session-oracle RUNNER STOCK-ID4 STOCK-ID5 LOADER-40000 OWNED-IMAGE OUTPUT"
 exists<-doesPathExist root
 ensure(not exists) "output directory must be a fresh evidence generation"
 createDirectoryIfMissing True root
 id4<-readPinned "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" id4Path
 id5<-readPinned "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" id5Path
 loader<-readPinned "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882" loaderPath
 owned<-B.readFile ownedPath
 ensure(not(B.null owned) && B.length owned<=128*1024 && B.length owned`mod`4==0) "owned session image exceeds qualified input envelope"
 let stockCode=[Block "stock-session"0xd7efd4 "rx"(slice 0xbf1a00 0xd7efd4 (0xd7fbcc-0xd7efd4) id5)
      ,Block "stock-tick"0xd7be08 "rx"(slice 0xbf1a00 0xd7be08 32 id5)
      ,Block "stock-interface-tick"0x7257a8 "rx"(slice 0x61c400 0x7257a8 56 id4)
      ,Block "stock-selector"0x412bc "rx"(slice 0x40000 0x412bc 16 loader)
      ,Block "stock-selector-cpu"0x427b8 "rx"(slice 0x40000 0x427b8 28 loader)
      ,Block "interface-anchors"0x96bd20 "r"(assemble[0x3000000,0x3000000])
      ,Block "interface"0x3000000 "r"(putWord 0x1618 0x412bc(putWord 0x174 0x400c0800(B.replicate 0x161c 0)))]
 results<-forM(zip[0::Int ..]scenarios)$ \(index,(tick@(Tick value),frames))->do
  let ident="session-"++show index
  a<-candidate runner root [Block "owned-session"0x1000000 "rx" owned] ident tick frames
  c<-stock runner root (stockCode++[Block(ident++"-tick")0x400c0800 "r"(word value)]) ident frames
  let same=a==c
  writeNew(root</>ident++"-comparison.json")(encodeCanonical(obj[("matched",b same),("frames",s(show frames)),("stock",s(show c)),("owned",s(show a))]))
  pure same
 executable<-getExecutablePath >>= B.readFile
 writeNew(root</>"summary.json")(encodeCanonical(obj[("passed",b(and results)),("cases",n(fromIntegral(length results))),("qualifier_sha256",s(hash executable)),("owned_sha256",s(hash owned)),("stock_id4_sha256",s(hash id4)),("stock_id5_sha256",s(hash id5)),("loader_sha256",s(hash loader)),("limitations",s "Session control and bulk-source CPU only. No command provider, persistent transaction, DMA, scheduler, peripheral, full-system or installed-camera qualification. Repaired E1 spans require separate negative controls.")]))
 unless(and results)exitFailure
