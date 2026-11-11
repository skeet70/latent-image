{-# LANGUAGE ScopedTypeVariables #-}
module Emulation.NativeCase where
import FujiAF.Host hiding (sha256)
import Emulation.Sha256 (sha256, sha256Hex)
import Data.Aeson (Value(..))
import FujiAF.JSON (decodeStrictIntegers)
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Control.Concurrent
import Control.Exception
import Control.Monad
import Data.Bits
import Data.Char
import Data.Either (isRight)
import Data.IORef
import Data.List
import Numeric
import System.Directory
import System.Exit
import System.FilePath
import System.IO
import System.Posix.Signals (sigKILL, signalProcessGroup)
import System.Process
import System.Timeout (timeout)

regs :: [String]
regs=map (("r"++).show) [0::Int ..12]++["sp","lr","cpsr"]
-- The public bounded-native-case contract. schemas/native-case.schema.json carries the same
-- numbers; NativeTests.hs holds the schema and the adapter together, and runs the runner at each
-- limit. The runner's stub-event recording capacity (its engine MAX_RECORDED_EVENTS) is a separate,
-- larger engine limit: an execution records far more events than a case declares (e.g., 184 recorded
-- vs. 0 declared), measured over 2738 recipe-bank SD cases with a maximum of 284 events.
-- Largest case measured over the recipe-bank and UI generators and 2,314 generated cases:
-- 1131576 artifact bytes, 1888202 total mapped bytes, 19 artifacts, 16 measured regions, 210 functions,
-- 12 stubs, 59 expected events, 49 expected memory entries, 525916 expected memory bytes.
sizeBytes :: Int
sizeBytes=2*1024*1024
totalBytes :: Int
totalBytes=4*1024*1024
caseBytes :: Int
caseBytes=16*1024*1024
data MemoryProfile = DefaultMemory | Resource8 deriving (Eq,Show)
memoryProfile :: Value -> IO MemoryProfile
memoryProfile c
 | "memory_profile" `notElem` keys c = pure DefaultMemory
 | get c "memory_profile"==s "resource8" = pure Resource8
 | otherwise = failH "unsupported memory_profile"
profileSizeBytes, profileTotalBytes, profileJsonBytes, profileOutputBytes :: MemoryProfile -> Int
profileSizeBytes DefaultMemory=sizeBytes
profileSizeBytes Resource8=8*1024*1024
profileTotalBytes DefaultMemory=totalBytes
profileTotalBytes Resource8=8*1024*1024
profileJsonBytes DefaultMemory=caseBytes
profileJsonBytes Resource8=64*1024*1024
profileOutputBytes DefaultMemory=8*1024*1024
profileOutputBytes Resource8=40*1024*1024
profileInstructions :: MemoryProfile -> Integer
profileInstructions DefaultMemory=4000000
profileInstructions Resource8=16000000
profileWireBytes :: MemoryProfile -> Bool -> Int
profileWireBytes Resource8 _=40*1024*1024
profileWireBytes DefaultMemory scripted=(if scripted then 20 else 16)*1024*1024
wireHeader :: MemoryProfile -> Bool -> String
wireHeader DefaultMemory False="FUJI_NATIVE_V1"
wireHeader DefaultMemory True="FUJI_NATIVE_SCRIPT_V1"
wireHeader Resource8 False="FUJI_NATIVE_RESOURCE8_V1"
wireHeader Resource8 True="FUJI_NATIVE_RESOURCE8_SCRIPT_V1"
artifactCount, regionCount, functionCount, stubCount, eventCount, memoryCount :: Int
artifactCount=20
regionCount=64
functionCount=512
stubCount=32
eventCount=128
memoryCount=64
stackContextCount :: Int
stackContextCount=16
hex :: Integer -> String
hex x=showHex x ""
hexBytes :: B.ByteString -> String
hexBytes = BC.unpack . BL.toStrict . BB.toLazyByteString . BB.byteStringHex
address :: Value -> IO Integer
address v = do
 let text=str v
 ensure(length text==10 && take 2 text=="0x" && all (`elem` "0123456789abcdef") (drop 2 text)) "address/register: canonical 32-bit hexadecimal required"
 case readHex(drop 2 text) of [(value,"")]->pure value; _->failH "invalid hexadecimal"
byteString :: Value -> IO B.ByteString
byteString=profileByteString DefaultMemory
profileByteString :: MemoryProfile -> Value -> IO B.ByteString
profileByteString profile v=do
 let text=str v
 ensure(case v of String _->length text<=2*profileSizeBytes profile && even(length text) && all (`elem` "0123456789abcdef") text; _->False) "bytes: lowercase hex pairs required"
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

data MemoryTarget = AbsoluteTarget Integer | RegisterTarget Int | IndirectTarget Int deriving Show
data ContractMemory = ContractMemory MemoryTarget Integer B.ByteString deriving Show
data ScriptCall = ScriptCall [(String,Integer)] [(String,Integer)] [ContractMemory] [ContractMemory] deriving Show
data CaseStub = LegacyStub Integer [(String,Integer)] [(String,Integer)] | ScriptStub Integer [ScriptCall] deriving Show
stubAddress :: CaseStub -> Integer
stubAddress (LegacyStub target _ _)=target
stubAddress (ScriptStub target _)=target
scriptedStub :: CaseStub -> Bool
scriptedStub (ScriptStub _ _)=True
scriptedStub _=False
contractMemory :: MemoryProfile -> Value -> IO ContractMemory
contractMemory profile item=do
 let selectors=filter (`elem` keys item)["address","register","indirect_register"]
 selector<-case selectors of [one]->pure one;_->failH "memory target must have exactly one selector"
 fields item (selector++" offset hex") "stub memory"
 target<-if selector=="address" then AbsoluteTarget <$> address(get item selector) else do
   let name=str(get item selector)
   index<-maybe(failH "memory register must be r0-r12, sp or lr")pure(elemIndex name(take 15 regs))
   pure(if selector=="register" then RegisterTarget index else IndirectTarget index)
 offset<-integer(get item "offset")(-2147483648)2147483647 "stub memory offset"
 bytes<-profileByteString profile(get item "hex")
 ensure(not(B.null bytes)) "stub memory must not be empty"
 pure(ContractMemory target offset bytes)
stubWire :: Bool -> CaseStub -> [String]
stubWire False (LegacyStub target inputs outputs)=
 unwords[hex target,show(length inputs),show(length outputs)]:bindingRows inputs++bindingRows outputs
stubWire True (ScriptStub target calls)=unwords[hex target,show(length calls)]:concatMap callRows calls
 where
  callRows (ScriptCall inputs outputs reads writes)=
   unwords(map show[length inputs,length outputs,length reads,length writes]):bindingRows inputs++bindingRows outputs++map memoryRow(reads++writes)
  memoryRow (ContractMemory target' offset bytes)=
   let (kind,base)=case target' of AbsoluteTarget value->("a",value);RegisterTarget index->("r",fromIntegral index);IndirectTarget index->("i",fromIntegral index)
   in unwords[kind,hex base,hex(offset `mod` 4294967296),hex(fromIntegral(B.length bytes)),hexBytes bytes]
stubWire _ _=error "mixed stub formats"
bindingRows :: [(String,Integer)] -> [String]
bindingRows values=[unwords[show(maybe 0 id(elemIndex name regs)),hex value]|(name,value)<-values]

data IgnoredFrameSpan = IgnoredFrameSpan Value Integer Integer deriving Show
validateIgnoredMemory :: Value -> [Region] -> IO [IgnoredFrameSpan]
validateIgnoredMemory c regions=do
 envelope<-memoryProfile c
 let sizeBytes=profileSizeBytes envelope; totalBytes=profileTotalBytes envelope
     byteString=profileByteString envelope
 let expected=get c "expected"
 if "ignored_memory" `notElem` keys expected then pure [] else do
  rows<-arrayCheck(get expected "ignored_memory")1 memoryCount "ignored memory limit"
  sp<-address(get(get c "registers")"sp")
  spans<-foldM(\prior item->do
   fields item "region offset length" "ignored memory"
   let ident=get item "region"
   region<-maybe(failH "unknown ignored region")pure(find(\r->get(description r)"id"==ident)regions)
   offset<-integer(get item "offset")0(fromIntegral sizeBytes) "ignored offset"
   len<-integer(get item "length")1(fromIntegral sizeBytes) "ignored length"
   let end=offset+len
   ensure(str(get(description region)"permissions")=="rw" && end<=regionSize region && baseAddress region<sp && sp<=baseAddress region+regionSize region && baseAddress region+end<=sp) "ignored memory must be private writable frame below entry SP"
   ensure(all(\(IgnoredFrameSpan name low high)->name/=ident || high<=offset || end<=low)prior) "ignored memory overlaps"
   forM_(list(get expected "memory"))$ \change->when(get change "region"==ident)$do
    bytes<-byteString(get change "hex")
    let low=num(get change "offset"); high=low+fromIntegral(B.length bytes)
    ensure(high<=offset || end<=low) "ignored memory overlaps expected output"
   pure(prior++[IgnoredFrameSpan ident offset end]))[] rows
  ensure(sum[high-low|IgnoredFrameSpan _ low high<-spans]<=fromIntegral totalBytes) "total ignored memory limit"
  pure spans

data Region = Region {description::Value,baseAddress::Integer,regionSize::Integer,regionBytes::B.ByteString} deriving Show
regionAt :: [Region] -> Integer -> Integer -> Char -> Bool
regionAt rs base size permission=any(\r->baseAddress r<=base && base+size<=baseAddress r+regionSize r && permission `elem` str(get(description r)"permissions"))rs
newtype ArtifactDigest = ArtifactDigest String deriving (Eq,Ord,Show)
data Artifact = Artifact {artifactDigest::ArtifactDigest,artifactBytes::B.ByteString}
-- A slice's offset and length are in bytes; the region's remainder is zero.
data RegionSource = ZeroSource | SliceSource Artifact Integer Integer
-- Reads and hashes the canonical artifact path a case names.
type ArtifactReader = FilePath -> IO Artifact
readArtifact :: ArtifactReader
readArtifact file=do
 bytes<-readRegularWith (pure())False (profileSizeBytes Resource8) file
 pure(Artifact(ArtifactDigest(sha256Hex(sha256 bytes)))bytes)
data LoadedCase = LoadedCase
 { loadedValue::Value
 , loadedRegions::[(Region,RegionSource)]
 , loadedWire::B.ByteString
 , loadedIdentity::[(String,Value)]
 , loadedJsonBytes::Int
 -- The wire rows before and after the region rows.
 , loadedRows::([String],[String])
 }
loadCase :: FilePath -> IO (Value,[Region],B.ByteString,[(String,Value)])
loadCase input = do
 loaded<-loadCaseWith readArtifact input
 pure(loadedValue loaded,map fst(loadedRegions loaded),loadedWire loaded,loadedIdentity loaded)
loadCaseWith :: ArtifactReader -> FilePath -> IO LoadedCase
loadCaseWith reader input = do
 path<-canonicalizePath input
 raw<-readRegularWith (pure()) False (profileJsonBytes Resource8) path
 c<-either failH pure(decodeStrictIntegers raw)
 envelope<-memoryProfile c
 ensure(B.length raw<=profileJsonBytes envelope) "case JSON bytes exceed the limit"
 let sizeBytes=profileSizeBytes envelope; totalBytes=profileTotalBytes envelope
     byteString=profileByteString envelope
     optional=[key|key<-["cp14_identity","kernel","memory_profile"],key `elem` keys c]
     ignored=["ignored_memory"|"ignored_memory" `elem` keys c]
 fields c (unwords(words "schema_version id origin provenance cpu artifacts entry exit registers regions functions stubs limits expected exclusions"++optional++ignored)) "case"
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
 profile<-if reg "cpsr" .&. 0x0fffffff==0x10 then pure UserEntry else do
   mode<-maybe(failH "CPSR profile is user mode ARM with flags only, or SVC or SYS ARM with flags, Q, GE, I and F only")pure(privilegedMode(reg "cpsr"))
   identity<-if "cp14_identity" `notElem` optional then pure Nothing else Just . Cp14Identity<$>address(get c "cp14_identity")
   kernel<-if "kernel" `notElem` optional then pure Nothing else Just<$>kernelEntry(get c "kernel")
   pure(PrivilegedEntry mode identity kernel)
 ensure(profile/=UserEntry || all(=="memory_profile")optional) "a CP14 identity or kernel needs a privileged entry"
 ensure(reg "lr"==exit && reg "sp" `mod` 8==0) "LR must be exit and SP eight-byte aligned"
 fields(get c "limits") "instructions wall_ms" "limits"
 budget<-integer(get(get c "limits")"instructions")1(profileInstructions envelope) "instructions"
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
   artifact<-reader file
   ensure(B.length(artifactBytes artifact)==fromInteger size) "artifact length mismatch"
   ensure(artifactDigest artifact==ArtifactDigest digest) "artifact digest mismatch"
   pure(done++[(ident,artifact)])) [] inventory
 regionsInput<-arrayCheck(get c "regions")1 regionCount "regions limit"
 placed<-foldM(\done item->do
   let prior=map fst done
   fields item "id address size permissions initial" "region"
   ensure(validId(get item "id") && all(\r->get(description r)"id"/=get item "id")prior) "region id invalid/duplicate"
   base<-address(get item "address"); size<-integer(get item "size")1(fromIntegral sizeBytes) "region size"
   ensure(base+size<=4294967296) "region address overflow"
   ensure(get item "permissions" `elem` map s ["r","rw","rx","device"]) "permissions must be r/rw/rx/device"
   ensure(get item "permissions"/=s "device" || (isKernel profile && size==4 && base `elem` bankWords)) "a device region is one interrupt-controller bank word of a kernel case"
   ensure(get item "permissions"/=s "rx" || base+size<=0x7ffffffc) "executable region must end by 0x7ffffffc: PMSAv7 default map is execute-never from 0x80000000"
   ensure(all(\r->base+size<=baseAddress r || baseAddress r+regionSize r<=base)prior) "logical regions overlap"
   let initialBytes=get item "initial"
   (bytes,source)<-if initialBytes==obj[("zero",b True)] then pure(B.replicate(fromInteger size)0,ZeroSource) else do
     fields initialBytes "artifact offset length" "initial"
     artifact<-maybe(failH "unknown artifact")pure(lookup(str(get initialBytes "artifact"))artifacts)
     let original=artifactBytes artifact
     offset<-integer(get initialBytes "offset")0(fromIntegral sizeBytes) "artifact offset"
     len<-integer(get initialBytes "length")1 size "artifact length"
     ensure(offset+len<=fromIntegral(B.length original)) "artifact slice exceeds file"
     pure(B.take(fromInteger len)(B.drop(fromInteger offset)original)<>B.replicate(fromInteger(size-len))0,SliceSource artifact offset len)
   ensure(sum(map regionSize prior)+size<=fromIntegral totalBytes) "total mapped bytes exceeds host limit"
   pure(done++[(Region item base size bytes,source)]))[] regionsInput
 let regions=map fst placed
 forM_ [contexts|PrivilegedEntry _ _ (Just(KernelEntry _ _ _ _ contexts))<-[profile]] $ checkStackContexts regions (reg "sp")
 ensure(regionAt regions entry 4 'x' && regionAt regions exit 4 'x') "entry/exit must have explicit executable bytes"
 ensure(reg "sp">=8 && regionAt regions (reg "sp"-8) 8 'w') "stack top lacks writable eight bytes"
 forM_[stop|PrivilegedEntry _ _ (Just(KernelEntry(Just stop)_ _ _ _))<-[profile]]$ \stop->
   ensure(stop `mod` 4==0 && stop/=entry && stop/=exit && regionAt regions stop 4 'x') "stop address must be aligned, executable and differ from the entry and exit"
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
   let scripted="calls" `elem` keys item
   fields item (if scripted then "address calls" else "address inputs outputs side_effects") "stub"
   target'<-address(get item "address")
   ensure(target' `mod` 4==0 && target'/=exit && target' `notElem` map stubAddress done && not(any(\(x,len)->x<=target' && target'<x+len)functions)) "stub address alignment/alias"
   ensure(target' `notElem` [stop|PrivilegedEntry _ _ (Just(KernelEntry(Just stop)_ _ _ _))<-[profile]]) "stub address is the stop address"
   stub<-if scripted then do
     callsInput<-arrayCheck(get item "calls")0 eventCount "stub calls limit"
     calls<-forM callsInput$ \call->do
       fields call "inputs outputs reads writes" "stub call"
       inputs<-bindings(get call "inputs")(take 15 regs)
       outputs<-bindings(get call "outputs")(take 13 regs)
       reads<-arrayCheck(get call "reads")0 memoryCount "stub reads limit"
       writes<-arrayCheck(get call "writes")0 memoryCount "stub writes limit"
       ensure(length reads+length writes<=memoryCount) "stub memory rows limit"
       ScriptCall inputs outputs <$> mapM (contractMemory envelope) reads <*> mapM (contractMemory envelope) writes
     pure(ScriptStub target' calls)
    else do
     ensure(get item "side_effects"==s "registers-only") "unsupported stub side effects"
     inputs<-bindings(get item "inputs")(take 13 regs); outputs<-bindings(get item "outputs")(take 13 regs)
     ensure(not(null outputs)) "stub must have explicit outputs"
     pure(LegacyStub target' inputs outputs)
   pure(done++[stub]))[] stubsInput
 let scripted=any scriptedStub stubs || "ignored_memory" `elem` keys(get c "expected")
 ensure(not scripted || all scriptedStub stubs) "legacy and scripted stubs cannot be mixed"
 ensure(sum[length calls|ScriptStub _ calls<-stubs]<=eventCount) "total scripted calls limit"
 ensure(sum[B.length bytes|ScriptStub _ calls<-stubs,ScriptCall _ _ reads writes<-calls,ContractMemory _ _ bytes<-reads++writes]<=totalBytes) "total stub contract bytes limit"
 let expected=get c "expected"
 fields expected(unwords(words "status registers memory events"++[key|key<-["kernel","ignored_memory"],key `elem` keys expected])) "expected"
 ensure(str(get expected "status") `elem` words "returned instruction_budget wall_budget memory_guard fetch_guard unknown_call unsupported_instruction stub_contract stop_address") "unsupported expected exit"
 when("kernel" `elem` keys expected)$do
  ensure(isKernel profile) "expected kernel registers need a kernel case"
  void(bindings(get expected "kernel")["tpidrurw","fpexc","reserved"])
 expectedRegs<-bindings(get expected "registers")(regs++["pc"])
 differences<-arrayCheck(get expected "memory")0 memoryCount "expected memory limit"
 ensure(not(null expectedRegs) || (isKernel profile && get expected "status"==s "instruction_budget" && not(null differences))) "independent registers required except bounded kernel instruction stops with expected memory"
 void$foldM(\occupied item->do
   fields item "region offset hex" "expected memory"
   r<-maybe(failH "unknown expected region")pure(find(\r->get(description r)"id"==get item "region")regions)
   offset<-integer(get item "offset")0(fromIntegral sizeBytes) "expected offset"
   bytes<-byteString(get item "hex")
   let end=offset+fromIntegral(B.length bytes); ident=get item "region"
       writable=str(get(description r)"permissions") `elem` ["rw","device"]
       initialSlice=B.take(B.length bytes)(B.drop(fromInteger offset)(regionBytes r))
   ensure(not(B.null bytes) && end<=regionSize r) "expected memory bounds"
   -- An expected byte in a read-only region observes the initial value; it never authorizes a write.
   ensure(writable || initialSlice==bytes) "expected memory in a read-only region must equal its initial bytes"
   ensure(all(\(name,lo,hi)->name/=ident || hi<=offset || end<=lo)occupied) "expected memory overlaps"
   pure((ident,offset,end):occupied)) [] differences
 void(validateIgnoredMemory c regions)
 events<-arrayCheck(get expected "events")0 eventCount "event limit"
 forM_ events $ \event->do
  fields event "address inputs outputs" "expected event"
  void(address(get event "address")); void(bindings(get event "inputs")(take (if scripted then 15 else 13) regs)); void(bindings(get event "outputs")(take 13 regs))
 exclusions<-arrayCheck(get c "exclusions")1 1000000 "coverage exclusions required"
 ensure(all(not.null.str)exclusions) "coverage exclusions required"
 let rows=[wireHeader envelope scripted,"cortex-r5f-arm-le",unwords[hex entry,hex exit,show budget,show wall],unwords[hex(reg r)|r<-regs]]++profileRows profile++[unwords(map(show.length)[regionsInput,functionsInput,stubsInput])]
     functionRows=[unwords[hex x,hex size]|(x,size)<-functions]
     stubRows=concatMap(stubWire scripted)stubs
     after=functionRows++stubRows
     wire=BL.toStrict(BB.toLazyByteString(BB.string7(unlines rows)<>mconcat[BB.string7(regionHead r)<>BB.char7 ' '<>BB.byteStringHex(regionBytes r)<>BB.char7 '\n'|r<-regions]<>BB.string7(unlines after)))
 ensure(B.length wire<=profileWireBytes envelope scripted) "case wire bytes exceed the limit"
 let digest=sha256Hex(sha256 raw); wireDigest=sha256Hex(sha256 wire)
 pure LoadedCase
  { loadedValue=c
  , loadedRegions=placed
  , loadedWire=wire
  , loadedIdentity=[("case_sha256",s digest),("wire_sha256",s wireDigest),("artifacts",arr inventory)]
  , loadedJsonBytes=B.length raw
  , loadedRows=(rows,after)
  }
-- A wire region row up to its initial bytes.
regionHead :: Region -> String
regionHead r=unwords[hex(baseAddress r),hex(regionSize r),case str(get(description r)"permissions")of "r"->"1";"rw"->"3";"device"->"10";_->"5"]
-- The entry profile a case's CPSR names.
data EntryProfile = UserEntry | PrivilegedEntry PrivilegedMode (Maybe Cp14Identity) (Maybe KernelEntry) deriving (Eq,Show)
data PrivilegedMode = SupervisorMode | SystemMode deriving (Eq,Show)
-- The value MRC p14, 0, Rt, c2, c0, 0 reads in a privileged case.
newtype Cp14Identity = Cp14Identity Integer deriving (Eq,Show)
-- A kernel case's stop address, its TPIDRURW, CPACR and FPEXC values, and its declared stack contexts.
data KernelEntry = KernelEntry (Maybe Integer) (Maybe Integer) (Maybe Integer) (Maybe Integer) [StackBounds] deriving (Eq,Show)
-- One declared private stack: @low < SP <= high@.
data StackBounds = StackBounds {stackLow::Integer,stackHigh::Integer} deriving (Eq,Show)
isKernel :: EntryProfile -> Bool
isKernel(PrivilegedEntry _ _ (Just _))=True
isKernel _=False
-- Set-pending and clear-pending words of the two interrupt-controller banks.
bankWords :: [Integer]
bankWords=[0xfffb30cc,0xfffb40cc,0xfffb30ec,0xfffb40ec]
-- FPEXC.EN, the only FPEXC bit a kernel case may set.
fpexcEnable :: Integer
fpexcEnable=0x40000000
kernelEntry :: Value -> IO KernelEntry
kernelEntry v=do
 ensure(case v of Object _->all(`elem` ["stop","tpidrurw","cpacr","fpexc","stack_contexts"])(keys v);_->False) "kernel: only stop, tpidrurw, cpacr, fpexc and stack_contexts"
 let optionalAddress k=if k `elem` keys v then Just<$>address(get v k) else pure Nothing
 contexts<-if "stack_contexts" `elem` keys v then stackContexts(get v "stack_contexts") else pure []
 kernel<-KernelEntry<$>optionalAddress "stop"<*>optionalAddress "tpidrurw"<*>optionalAddress "cpacr"<*>optionalAddress "fpexc"<*>pure contexts
 let KernelEntry _ _ cpacr fpexc _=kernel
 ensure(maybe True(\value->value .&. complement fpexcEnable==0)fpexc) "kernel fpexc may set EN only"
 ensure(fpexc==Nothing || cpacr/=Nothing) "kernel fpexc needs a cpacr"
 pure kernel
-- A non-empty array of eight-byte aligned, disjoint @low < high@ bounds.
stackContexts :: Value -> IO [StackBounds]
stackContexts v=do
 items<-arrayCheck v 1 stackContextCount "kernel stack_contexts must have one to sixteen contexts"
 contexts<-forM items$ \item->do
  fields item "low high" "stack context"
  low<-address(get item "low"); high<-address(get item "high")
  ensure(low .&. 7==0 && high .&. 7==0) "stack context bounds must be eight-byte aligned"
  ensure(low<high) "stack context low bound must be below its high bound"
  pure(StackBounds low high)
 let ordered=sortOn stackLow contexts
 ensure(and[high<=nextLow|(StackBounds _ high,StackBounds nextLow _)<-zip ordered(drop 1 ordered)]) "kernel stack contexts overlap"
 pure contexts
-- Every declared context lies inside one writable, non-device region and the initial SP names one.
checkStackContexts :: [Region] -> Integer -> [StackBounds] -> IO ()
checkStackContexts regions sp contexts=do
 when(not(null contexts))$ensure(length(filter(\(StackBounds low high)->low<sp && sp<=high)contexts)==1) "initial SP must belong to exactly one declared stack context"
 forM_ contexts$ \(StackBounds low high)->
  ensure(any(\r->str(get(description r)"permissions")=="rw" && baseAddress r<=low && high<=baseAddress r+regionSize r)regions) "stack context must be backed by one writable non-device region"
-- SVC or SYS with only NZCVQ, GE, I and F besides the mode.
privilegedMode :: Integer -> Maybe PrivilegedMode
privilegedMode cpsr
 | cpsr .&. complement 0xf80f00df/=0 = Nothing
 | otherwise = case cpsr .&. 0x1f of 0x13->Just SupervisorMode; 0x1f->Just SystemMode; _->Nothing
profileRows :: EntryProfile -> [String]
profileRows UserEntry=[]
profileRows(PrivilegedEntry _ identity kernel)=
 [maybe "n"(\(Cp14Identity v)->"i "++hex v)identity]
 ++[row|Just(KernelEntry stop tpidrurw cpacr fpexc contexts)<-[kernel],row<-unwords("k":map(maybe "n" hex)[stop,tpidrurw,cpacr,fpexc]):contextRows contexts]
 where
  contextRows []=[]
  contextRows contexts=[unwords("c":show(length contexts):concat[[hex low,hex high]|StackBounds low high<-contexts])]
checkResult :: Value -> [Region] -> Value -> IO [String]
checkResult c regions result=do
 envelope<-memoryProfile c
 let byteString=profileByteString envelope
 expectedIgnored<-validateIgnoredMemory c regions
 initial<-bindings(get c "registers")regs
 let expected=get c "expected"
     reg k=maybe 0 id(lookup k initial)
     status=["status differs"|get result "status"/=get expected "status"]
     registers=["register "++k++": differs"|k<-keys(get expected "registers"),get(get result "registers")k/=get(get expected "registers")k]
     events=["stub events differ"|get result "events"/=get expected "events"]
     kernel=["kernel register "++k++": differs"|"kernel" `elem` keys expected,k<-keys(get expected "kernel"),get(get result "kernel")k/=get(get expected "kernel")k]
     memory=list(get result "memory")
 ignored<-if "ignored_memory" `notElem` keys c then pure [] else do
   rows<-arrayCheck(get c "ignored_memory")0 memoryCount "ignored memory limit"
   foldM(\done item->do
     fields item (unwords (["region","offset","length"] ++ ["reason"|"reason" `elem` keys item])) "ignored memory"
     let ident=str(get item "region")
         reason=if "reason" `elem` keys item then str(get item "reason") else "private-frame"
     ensure(reason `elem` ["private-frame","caller-output"]) "ignored memory reason must be private-frame or caller-output"
     r<-maybe(failH "unknown ignored region")pure(find(\region->get(description region)"id"==s ident)regions)
     offset<-integer(get item "offset")0(fromIntegral(regionSize r)) "ignored offset"
     length'<-integer(get item "length")1(fromIntegral(regionSize r)) "ignored length"
     let end=offset+length'
         offsetI=fromIntegral offset
         lengthI=fromIntegral length'
         sp=fromInteger(reg "sp")
         base=fromIntegral(baseAddress r)
         dead=max 0(min(toInteger(regionSize r))(sp-base))
         writable=str(get(description r)"permissions") `elem` ["rw","device"]
         asserted=[(fromInteger(num(get change "offset")),fromInteger(num(get change "offset"))+toInteger(length(str(get change "hex"))`div`2))|change<-list(get expected "memory"),get change "region"==s ident]
     ensure(end<=toInteger(regionSize r)) "ignored memory exceeds its region"
     case reason of
       "private-frame"->do
         ensure(ident=="stack") "only stack private frames may be ignored as private frames"
         ensure(get expected "status"==s "returned") "ignored memory requires a returned case"
         ensure(end<=toInteger(regionSize r) && end<=dead) "ignored memory is not a dead private frame"
         ensure(all(\(lo,hi)->hi<=offset || end<=lo)asserted) "private frame overlaps asserted memory"
       _->do
         ensure(writable) "caller-output ignored memory must be writable"
         ensure(get expected "status"==s "returned") "ignored memory requires a returned case"
         ensure(not(null asserted)) "caller-output ignored memory needs an asserted prefix"
         ensure(all(\(_,hi)->hi<=offset)asserted) "caller-output ignored memory must follow the asserted prefix"
     ensure(all(\(name,lo,extent)->name/=ident || lo+extent<=offsetI || offsetI+lengthI<=lo)done) "ignored memory overlaps"
     pure((ident,offsetI,lengthI):done))[]rows
 let sortedIgnored=sortOn(\(name,offset,_)->(name,offset))
       (ignored++[(str ident,fromInteger low,fromInteger(high-low))|IgnoredFrameSpan ident low high<-expectedIgnored])
 mem<-if length memory/=length regions then pure["memory region count differs"] else fmap concat$forM(zip regions memory)$ \(r,actual)->do
   desired<-foldM(\bytes change->if get change "region"==get(description r)"id" then do
     value<-byteString(get change "hex")
     let offset=fromInteger(num(get change "offset"))
     pure(B.take offset bytes<>value<>B.drop(offset+B.length value)bytes)
    else pure bytes)(regionBytes r)(list(get expected "memory"))
   let ident=str(get(description r)"id")
       ranges=[(offset,length')|(name,offset,length')<-sortedIgnored,name==ident]
       intervals=complementRanges(B.length desired)ranges
       actualHex=str(get actual "bytes")
       desiredHex=hexBytes desired
       same
         | null ranges = actualHex==desiredHex
         | otherwise = all(\(offset,length')->take(2*length')(drop(2*offset)actualHex)==take(2*length')(drop(2*offset)desiredHex))intervals
   pure["memory "++ident++": differs from exact allowed final bytes"|get actual "address"/=get(description r)"address" || length actualHex/=2*B.length desired || any (`notElem` "0123456789abcdef") actualHex || not same]
 pure(status++registers++events++kernel++mem)

complementRanges :: Int -> [(Int,Int)] -> [(Int,Int)]
complementRanges total ranges=go 0 ranges
 where
  go position []=[(position,total-position)|position<total]
  go position ((offset,length'):rest)=
    (if offset>position then [(position,offset-position)] else [])
    ++go(max position(offset+length'))rest
unknownReport :: Value
unknownReport=obj[("schema_version",n 1),("profile",s "unknown")]
caseReport :: Value -> [(String,Value)] -> Value
caseReport c identity=
 let profile=if get c "origin"==s "stock" then "stock-bounded" else "synthetic-only"
 in merge unknownReport(identity++[("case_id",get c "id"),("origin",get c "origin"),("profile",s profile),("provenance",get c "provenance"),("exclusions",get c "exclusions")])
runnerDigest :: FilePath -> IO String
runnerDigest executable=sha256Hex . sha256 <$> readRegularWith(pure())False (256*1024*1024) executable
-- The report for one completed runner observation and its exit.
judgeRunner :: Value -> [Region] -> Value -> ExitCode -> B.ByteString -> B.ByteString -> IO (Int,Value)
judgeRunner c regions report' exit out err=do
 let runnerExit=n(case exit of ExitSuccess->0;ExitFailure i->fromIntegral i)
     failed=merge report'[("status",s "runner_failure"),("runner_exit",runnerExit),("runner_stdout",s(BC.unpack out)),("stderr",s(BC.unpack err))]
 parsed<-try$do
  value<-jsonRead out
  ensure(case value of Object _->get value "wire_version"==n 1;_->False) "invalid runner output"
  problems<-checkResult c regions value
  ensure(exit==(if get value "status"==s "returned" then ExitSuccess else ExitFailure 1)) "runner status/exit mismatch"
  pure(if null problems then 0 else 1,merge report'[("status",s(if null problems then "passed" else "failed")),("runner",value),("runner_exit",runnerExit),("mismatches",arr(map s problems)),("stderr",s(BC.unpack err))])
 case parsed of Left(e::IOException)->pure(1,set "reason" (s(show e)) failed);Right value->pure value
runCase :: FilePath -> FilePath -> IO (Int,Value)
runCase runner path = do
 loaded<-try(loadCase path)
 case loaded of
  Left(e::IOException)->pure(1,merge unknownReport[("status",s "invalid_case"),("reason",s(show e))])
  Right(c,regions,wire,identity)->do
   let report=caseReport c identity
   result<-try$do
    executable<-canonicalizePath runner
    hash<-runnerDigest executable
    envelope<-memoryProfile c
    (timed,exit,out,err)<-runBounded(fromInteger(num(get(get c "limits")"wall_ms")*1000+2000000))(profileOutputBytes envelope) executable [] wire
    let report'=set "runner_sha256" (s hash) report
    if timed then pure(1,merge report'[("status",s "host_timeout"),("stdout",s(BC.unpack out)),("stderr",s(BC.unpack err))]) else judgeRunner c regions report' exit out err
   case result of Left(e::IOException)->pure(1,merge report[("status",s "invalid_case"),("reason",s(show e))]);Right value->pure value

-- Batches. The public batch contract: at most batchCaseCount cases, batchJsonBytes of batch
-- and case JSON together, and batchArtifactBytes of distinct artifact bytes, by SHA256, read for
-- its cases. schemas/native-case.schema.json carries the same numbers.
batchCaseCount :: Int
batchCaseCount=4096
batchJsonBytes :: Int
batchJsonBytes=256*1024*1024
batchArtifactBytes :: Int
batchArtifactBytes=256*1024*1024
-- Time for the runner to read the whole batch wire.
batchInputMicros :: Int
batchInputMicros=60000000
-- Cap on one observation line and on the batch's stderr, as runCase caps each stream.
batchStreamBytes :: Int
batchStreamBytes=8*1024*1024

newtype CaseId = CaseId {caseIdText::String} deriving (Eq,Ord,Show)

-- A lowercase hexadecimal SHA256.
newtype DigestHex = DigestHex {digestHex::String} deriving (Eq,Show)

data BatchEntry = BatchEntry {entryId::CaseId,entryPath::FilePath}

data BatchStatus = BatchComplete | BatchInvalid | BatchRunnerFailure | BatchHostTimeout deriving (Eq,Show)

batchStatusLabel :: BatchStatus -> String
batchStatusLabel BatchComplete="complete"
batchStatusLabel BatchInvalid="invalid_batch"
batchStatusLabel BatchRunnerFailure="runner_failure"
batchStatusLabel BatchHostTimeout="host_timeout"

-- A case's status in the batch summary: its report's status, or not_run.
data CaseStatus = CasePassed | CaseFailed | CaseInvalid | CaseRunnerFailure | CaseHostTimeout | CaseNotRun deriving (Eq,Show,Enum,Bounded)

caseStatusLabel :: CaseStatus -> String
caseStatusLabel CasePassed="passed"
caseStatusLabel CaseFailed="failed"
caseStatusLabel CaseInvalid="invalid_case"
caseStatusLabel CaseRunnerFailure="runner_failure"
caseStatusLabel CaseHostTimeout="host_timeout"
caseStatusLabel CaseNotRun="not_run"

caseStatusOf :: Value -> CaseStatus
caseStatusOf report=case[status|status<-[minBound..maxBound],caseStatusLabel status==str(get report "status")] of
 status:_->status
 []->error("native batch: report status "++show(get report "status"))

data BatchCase = BatchCase {batchCaseId::CaseId,batchCaseStatus::CaseStatus,batchCaseDigest::Maybe DigestHex} deriving (Eq,Show)

-- The batch file's SHA256 bound to every case id and status, in batch order.
data BatchSummary = BatchSummary
 { summaryStatus::BatchStatus
 , summaryBatchDigest::Maybe DigestHex
 , summaryRunnerDigest::Maybe DigestHex
 , summaryReason::Maybe String
 -- The case whose frame was due when the runner process failed.
 , summaryFailedCase::Maybe CaseId
 , summaryRunnerExit::Maybe ExitCode
 , summaryStderr::Maybe B.ByteString
 , summaryCases::[BatchCase]
 } deriving Show

batchPassed :: BatchSummary -> Bool
batchPassed summary=summaryStatus summary==BatchComplete && all((==CasePassed) . batchCaseStatus)(summaryCases summary)

-- The summary as $defs/batchSummary of schemas/native-case.schema.json.
summaryJson :: BatchSummary -> Value
summaryJson summary=obj(
 [("schema_version",n 1),("status",s(batchStatusLabel(summaryStatus summary)))
 ,("cases",arr[obj([("id",s(caseIdText(batchCaseId c))),("status",s(caseStatusLabel(batchCaseStatus c)))]++[("case_sha256",s(digestHex d))|Just d<-[batchCaseDigest c]])|c<-summaryCases summary])]
 ++[("batch_sha256",s(digestHex d))|Just d<-[summaryBatchDigest summary]]
 ++[("runner_sha256",s(digestHex d))|Just d<-[summaryRunnerDigest summary]]
 ++[("reason",s r)|Just r<-[summaryReason summary]]
 ++[("failed_case",s(caseIdText c))|Just c<-[summaryFailedCase summary]]
 ++[("runner_exit",exitNumber c)|Just c<-[summaryRunnerExit summary]]
 ++[("stderr",s(BC.unpack e))|Just e<-[summaryStderr summary]])

-- One case's report in the runCase format, with the regions of an admitted case.
data CaseReport = CaseReport {reportCase::CaseId,reportRegions::Maybe [Region],reportCode::Int,reportValue::Value}

-- A region without its bytes: case JSON description, base, size and initial-bytes source.
type RegionPlan = (Value,Integer,Integer,RegionSource)

-- The region loadCase builds from the same plan.
materialize :: RegionPlan -> Region
materialize(item,base,size,source)=Region item base size bytes
 where
  bytes=case source of
   ZeroSource->B.replicate(fromInteger size)0
   SliceSource artifact offset len->B.take(fromInteger len)(B.drop(fromInteger offset)(artifactBytes artifact))<>B.replicate(fromInteger(size-len))0

-- A case the adapter admitted, without its single-case wire or region bytes.
data Admitted = Admitted
 { admittedId::CaseId
 , admittedValue::Value
 , admittedRegions::[RegionPlan]
 , admittedReport::Value
 , admittedRows::([String],[String])
 }

-- The batch file's bytes and entries: {"schema_version":1,"cases":[{"id":ID,"path":PATH}]},
-- each path relative and inside the batch file's directory.
readBatch :: FilePath -> IO (B.ByteString,[BatchEntry])
readBatch input=do
 path<-canonicalizePath input
 size<-getFileSize path
 ensure(size<=fromIntegral batchJsonBytes) "batch JSON bytes exceed the limit"
 raw<-readRegularWith(pure())False(fromInteger size)path
 batch<-either failH pure(decodeStrictIntegers raw)
 fields batch "schema_version cases" "batch"
 void(integer(get batch "schema_version")1 1 "batch schema version")
 items<-arrayCheck(get batch "cases")1 batchCaseCount "batch case count"
 (_,entries)<-foldM(\(seen,done) item->do
   fields item "id path" "batch case"
   let ident=CaseId(str(get item "id")); relative=str(get item "path")
   ensure(validId(get item "id") && not(M.member ident seen)) "batch case id invalid/duplicate"
   ensure(not(null relative) && isRelative relative && ".." `notElem` splitDirectories relative) "case path must stay under batch directory"
   file<-canonicalizePath(takeDirectory path</>relative)
   ensure(splitDirectories(takeDirectory path) `isPrefixOf` splitDirectories file) "case symlink escapes batch directory"
   pure(M.insert ident()seen,BatchEntry ident file:done))(M.empty,[])items
 pure(raw,reverse entries)

-- Raised when the distinct artifact bytes read for a batch pass batchArtifactBytes.
data ArtifactLimit = ArtifactLimit deriving Show
instance Exception ArtifactLimit

-- Reads and hashes each canonical path once; paths with identical bytes share one Artifact.
sharedArtifactReader :: IO ArtifactReader
sharedArtifactReader=do
 paths<-newIORef M.empty
 digests<-newIORef M.empty
 total<-newIORef 0
 pure$ \file->do
  known<-M.lookup file<$>readIORef paths
  case known of
   Just artifact->pure artifact
   Nothing->do
    fresh<-readArtifact file
    let ArtifactDigest digest=artifactDigest fresh
    _<-evaluate(length digest)
    seen<-M.lookup(artifactDigest fresh)<$>readIORef digests
    artifact<-case seen of
     Just shared->pure shared
     Nothing->do
      bytes<-(+B.length(artifactBytes fresh))<$>readIORef total
      when(bytes>batchArtifactBytes)(throwIO ArtifactLimit)
      writeIORef total bytes
      modifyIORef' digests(M.insert(artifactDigest fresh)fresh)
      pure fresh
    modifyIORef' paths(M.insert file artifact)
    pure artifact

-- Loads one entry as loadCase would; the case's id must be its entry's. The single-case wire
-- is hashed for the report; neither it nor the region bytes are retained.
admitCase :: ArtifactReader -> BatchEntry -> IO (Int,Admitted)
admitCase reader entry=do
 LoadedCase{loadedValue=c,loadedRegions=placed,loadedIdentity=identity,loadedJsonBytes=bytes,loadedRows=rows}<-loadCaseWith reader(entryPath entry)
 ensure(get c "id"==s(caseIdText(entryId entry))) "case id differs from its batch entry"
 forM_ identity(evaluate . snd)
 plans<-forM placed$ \(r,source)->do
  item<-evaluate(description r); base<-evaluate(baseAddress r); size<-evaluate(regionSize r)
  pure(item,base,size,source)
 pure(bytes,Admitted(entryId entry)c plans(caseReport c identity)rows)

-- The distinct artifacts the admitted cases slice, in order of first use.
batchArtifacts :: [Admitted] -> [Artifact]
batchArtifacts admitted=reverse(snd(foldl' add(M.empty,[])[artifact|a<-admitted,(_,_,_,SliceSource artifact _ _)<-admittedRegions a]))
 where add(seen,done)artifact
        |M.member(artifactDigest artifact)seen=(seen,done)
        |otherwise=(M.insert(artifactDigest artifact)()seen,artifact:done)

-- The case's wire with each region's bytes replaced by `z` or `s ARTIFACT OFFSET LENGTH`.
batchBody :: M.Map ArtifactDigest Int -> Admitted -> B.ByteString
batchBody index a=BL.toStrict(BB.toLazyByteString(BB.string7(unlines before)<>mconcat[BB.string7(regionHead(materialize plan)++initial source++"\n")|plan@(_,_,_,source)<-admittedRegions a]<>BB.string7(unlines after)))
 where
  (before,after)=admittedRows a
  initial ZeroSource=" z"
  initial(SliceSource artifact offset len)=unwords["","s",show(index M.! artifactDigest artifact),show offset,show len]

batchWire :: MemoryProfile -> [Artifact] -> [B.ByteString] -> BL.ByteString
batchWire envelope artifacts bodies=BB.toLazyByteString(BB.string7(header++"\n"++show(length artifacts)++" "++show(length bodies)++"\n")<>mconcat(map(framed . artifactBytes)artifacts)<>mconcat(map framed bodies))
 where
  header=case envelope of DefaultMemory->"FUJI_NATIVE_BATCH_V1";Resource8->"FUJI_NATIVE_RESOURCE8_BATCH_V1"
  framed bytes=BB.intDec(B.length bytes)<>BB.char7 '\n'<>BB.byteString bytes<>BB.char7 '\n'

-- Each entry is invalid (its runCase report) or admitted; past the limits the batch is refused.
prepareBatch :: B.ByteString -> [BatchEntry] -> IO [(CaseId,Either Value Admitted)]
prepareBatch raw entries=do
 declared<-forM entries$ \entry->getFileSize(entryPath entry)`catch`(\(_::IOException)->pure 0)
 ensure(fromIntegral(B.length raw)+sum declared<=toInteger batchJsonBytes) "batch JSON bytes exceed the limit"
 reader<-sharedArtifactReader
 loaded<-forM entries(\entry->do
  result<-try(admitCase reader entry)
  pure(entryId entry,case result of
   Left(e::IOException)->(0,Left(merge unknownReport[("status",s "invalid_case"),("reason",s(show e))]))
   Right(bytes,admitted)->(bytes,Right admitted)))
  `catch`\ArtifactLimit->failH "batch artifact bytes exceed the limit"
 ensure(B.length raw+sum[bytes|(_,(bytes,_))<-loaded]<=batchJsonBytes) "batch JSON bytes exceed the limit"
 pure[(ident,result)|(ident,(_,result))<-loaded]

data LineEnd = OutputEnded | LineTooLong deriving Eq

-- Runner stdout read line by line; the current frame's bytes are kept for diagnostics.
data Output = Output
 { outputHandle::Handle
 , outputBuffer::IORef B.ByteString
 , outputPending::IORef [B.ByteString]
 , outputFrame::IORef [B.ByteString]
 }

readLine :: Output -> Int -> IO (Either LineEnd B.ByteString)
readLine out cap=go
 where
  go=do
   buffered<-readIORef(outputBuffer out)
   case BC.elemIndex '\n' buffered of
    Just i->do
     earlier<-readIORef(outputPending out)
     let line=B.concat(reverse(B.take i buffered:earlier))
     writeIORef(outputBuffer out)(B.drop(i+1)buffered)
     writeIORef(outputPending out)[]
     modifyIORef'(outputFrame out)(BC.snoc line '\n':)
     pure(if B.length line>cap then Left LineTooLong else Right line)
    Nothing->do
     modifyIORef'(outputPending out)(buffered:)
     writeIORef(outputBuffer out)B.empty
     held<-sum . map B.length<$>readIORef(outputPending out)
     if held>cap then pure(Left LineTooLong) else do
      chunk<-B.hGetSome(outputHandle out)65536
      if B.null chunk then pure(Left OutputEnded) else writeIORef(outputBuffer out)chunk>>go

frameText :: Output -> IO B.ByteString
frameText out=do
 frame<-readIORef(outputFrame out); pending<-readIORef(outputPending out); buffered<-readIORef(outputBuffer out)
 pure(B.take batchStreamBytes(B.concat(reverse frame++reverse pending++[buffered])))

-- `FUJI_NATIVE_BATCH_CASE INDEX EXIT` and the observation line.
readFrame :: Output -> MemoryProfile -> Int -> IO (Either (BatchStatus,String) (ExitCode,B.ByteString))
readFrame out envelope index=do
 header<-readLine out 128
 case BC.words<$>header of
  Right[tag,position,exit]|tag==BC.pack "FUJI_NATIVE_BATCH_CASE",position==BC.pack(show index),Just code<-frameExit exit->do
   observation<-readLine out(profileOutputBytes envelope)
   pure$case observation of
    Right line->Right(code,line)
    Left OutputEnded->Left(BatchRunnerFailure,"runner output ended inside a frame")
    Left LineTooLong->Left(BatchHostTimeout,"observation exceeds the stream cap")
  Left OutputEnded->pure(Left(BatchRunnerFailure,"runner output ended before the frame"))
  Right(tag:_)|index==0,tag==BC.pack "FUJI_NATIVE_BATCH_INVALID"->pure(Left(BatchInvalid,"runner refused the batch: "++either(const "")BC.unpack header))
  _->pure(Left(BatchRunnerFailure,"malformed batch frame header"))
 where
  frameExit code|code==BC.pack "0"=Just ExitSuccess|code==BC.pack "1"=Just(ExitFailure 1)|otherwise=Nothing

data BatchProcess = BatchProcess
 { processHandle::ProcessHandle
 , processGroup::Maybe Pid
 , processOutput::Output
 , processInput::MVar ()
 , processErrors::IORef B.ByteString
 , processErrorsExcess::IORef Bool
 , processErrorsDone::MVar ()
 , processThreads::[ThreadId]
 }

startBatch :: FilePath -> BL.ByteString -> IO BatchProcess
startBatch executable wire=do
 (Just hin,Just hout,Just herr,ph)<-createProcess(proc executable ["--batch"]){std_in=CreatePipe,std_out=CreatePipe,std_err=CreatePipe,create_group=True}
 mapM_(`hSetBinaryMode`True)[hin,hout,herr]
 group<-getPid ph
 input<-newEmptyMVar
 writer<-forkIO((BL.hPut hin wire`catch`\(_::IOException)->pure())`finally`((hClose hin`catch`\(_::IOException)->pure())>>putMVar input()))
 errors<-newIORef B.empty; excess<-newIORef False; errorsDone<-newEmptyMVar
 let drain=do
      chunk<-B.hGetSome herr 65536
      unless(B.null chunk)$do
       current<-readIORef errors
       let combined=current<>chunk
       writeIORef errors(B.take batchStreamBytes combined)
       when(B.length combined>batchStreamBytes)(writeIORef excess True>>terminateProcess ph)
       drain
 reader<-forkIO(drain`catch`(\(_::IOException)->pure())`finally`putMVar errorsDone())
 output<-Output hout<$>newIORef B.empty<*>newIORef[]<*>newIORef[]
 pure(BatchProcess ph group output input errors excess errorsDone[writer,reader])

-- Kills the process group unless the runner was already reaped, then reaps it.
stopBatch :: BatchProcess -> IO (Maybe ExitCode)
stopBatch p=do
 reaped<-getProcessExitCode(processHandle p)
 when(reaped==Nothing)$do
  forM_(processGroup p)(\group->signalProcessGroup sigKILL group`catch`\(_::IOException)->pure())
  terminateProcess(processHandle p)`catch`\(_::IOException)->pure()
 code<-maybe(timeout 1000000(waitForProcess(processHandle p)))(pure . Just)reaped
 void(timeout 1000000(readMVar(processErrorsDone p)))
 mapM_ killThread(processThreads p)
 pure code

-- Runs every admitted case of a batch in one runner process and passes each case's report to the
-- sink in batch order.
runBatch :: FilePath -> FilePath -> (CaseReport -> IO ()) -> IO BatchSummary
runBatch runner input sink=do
 readResult<-try(readBatch input)
 case readResult of
  Left(e::IOException)->pure(invalidSummary Nothing(show e))
  Right(raw,entries)->do
   let batchDigest=DigestHex(sha256Hex(sha256 raw))
   prepared<-try$do
    cases<-prepareBatch raw entries
    executable<-canonicalizePath runner
    hash<-runnerDigest executable
    pure(cases,executable,DigestHex hash)
   case prepared of
    Left(e::IOException)->pure(invalidSummary(Just batchDigest)(show e))
    Right(cases,executable,hash)->do
     executed<-try(executeBatch executable hash batchDigest cases sink)
     either(\(e::IOException)->pure(invalidSummary(Just batchDigest)(show e)))pure executed
 where
  invalidSummary digest reason=BatchSummary
   { summaryStatus=BatchInvalid,summaryBatchDigest=digest,summaryRunnerDigest=Nothing,summaryReason=Just reason
   , summaryFailedCase=Nothing,summaryRunnerExit=Nothing,summaryStderr=Nothing,summaryCases=[]}

exitNumber :: ExitCode -> Value
exitNumber code=n(case code of ExitSuccess->0;ExitFailure i->fromIntegral i)

reportDigest :: Value -> Maybe DigestHex
reportDigest report=case get report "case_sha256" of
 String _->Just(DigestHex(str(get report "case_sha256")))
 _->Nothing

executeBatch :: FilePath -> DigestHex -> DigestHex -> [(CaseId,Either Value Admitted)] -> (CaseReport -> IO ()) -> IO BatchSummary
executeBatch executable hash batchDigest cases sink=do
 statuses<-newIORef[]
 let admitted=[a|(_,Right a)<-cases]
     artifacts=batchArtifacts admitted
     index=M.fromList(zip(map artifactDigest artifacts)[0..])
     record entry=do
      _<-evaluate(batchCaseStatus entry)
      forM_(batchCaseDigest entry)$ \(DigestHex digest)->evaluate(foldr seq()digest)
      modifyIORef' statuses(entry:)
     emit ident regions code value=do
      sink(CaseReport ident regions code value)
      record(BatchCase ident(caseStatusOf value)(reportDigest value))
     -- Reports of refused entries; admitted cases that never got a frame are not_run.
     remaining=mapM_(\(ident,result)->either(emit ident Nothing 1)(\a->record(BatchCase ident CaseNotRun(reportDigest(admittedReport a))))result)
     withRunner a=set "runner_sha256"(s(digestHex hash))(admittedReport a)
 envelopes<-mapM(memoryProfile . admittedValue)admitted
 let envelope=if Resource8 `elem` envelopes then Resource8 else DefaultMemory
     wire=batchWire envelope artifacts(map(batchBody index)admitted)
 ensure(BL.length wire<=768*1024*1024) "batch wire bytes exceed the limit"
 (status,(reason,failed,exit),errors)<-if null admitted then remaining cases>>pure(BatchComplete,(Nothing,Nothing,Nothing),B.empty) else
  bracket(startBatch executable wire)(void . stopBatch)$ \p->do
   transferred<-timeout batchInputMicros(readMVar(processInput p))
   let failAt ident a(status,reason)rest=do
        ended<-case status of BatchHostTimeout->pure Nothing;_->timeout 1000000(waitForProcess(processHandle p))
        partial<-frameText(processOutput p)
        stopped<-stopBatch p
        errors<-readIORef(processErrors p)
        let code=maybe stopped Just ended
            exitValue=[("runner_exit",exitNumber c)|Just c<-[code]]
            diagnostics=case status of
             BatchHostTimeout->[("status",s "host_timeout"),("stdout",s(BC.unpack partial)),("stderr",s(BC.unpack errors))]
             _->[("status",s "runner_failure"),("runner_stdout",s(BC.unpack partial)),("stderr",s(BC.unpack errors))]++exitValue
        if status==BatchInvalid then do
         remaining((ident,Right a):rest)
         pure(status,(Just reason,Nothing,code),errors)
        else do
         emit ident(Just(map materialize(admittedRegions a)))1(merge(withRunner a)(diagnostics++[("reason",s reason)]))
         remaining rest
         pure(status,(Just reason,Just ident,code),errors)
       frames _ []=finish
       frames k((ident,Left value):rest)=emit ident Nothing 1 value>>frames k rest
       frames k((ident,Right a):rest)=do
        writeIORef(outputFrame(processOutput p))[]
        let wall=num(get(get(admittedValue a)"limits")"wall_ms")
        envelope<-memoryProfile(admittedValue a)
        frame<-timeout(fromInteger(wall*1000+2000000))(readFrame(processOutput p)envelope k)
        excess<-readIORef(processErrorsExcess p)
        case frame of
         Just(Right(exit,line))->do
          let regions=map materialize(admittedRegions a)
          (code,value)<-judgeRunner(admittedValue a)regions(withRunner a)exit(BC.snoc line '\n')B.empty
          emit ident(Just regions)code value
          frames(k+1)rest
         _|excess->failAt ident a(BatchHostTimeout,"stderr exceeds the stream cap")rest
         Just(Left failure)->failAt ident a failure rest
         Nothing->failAt ident a(BatchHostTimeout,"case deadline elapsed")rest
       finish=do
        end<-timeout 2000000(readLine(processOutput p)128)
        exit<-timeout 2000000(waitForProcess(processHandle p))
        trailing<-timeout 2000000(readLine(processOutput p)128)
        void(stopBatch p)
        errors<-readIORef(processErrors p)
        excess<-readIORef(processErrorsExcess p)
        let ended=case trailing of Just(Left OutputEnded)->True;_->False
            proper=end==Just(Right(BC.pack("FUJI_NATIVE_BATCH_END "++show(length admitted))))
        pure$if excess then(BatchHostTimeout,(Just "stderr exceeds the stream cap",Nothing,exit),errors)
         else if proper && exit==Just ExitSuccess && ended then(BatchComplete,(Nothing,Nothing,Nothing),errors)
         else(BatchRunnerFailure,(Just "runner did not end the batch cleanly",Nothing,exit),errors)
   case transferred of
    Nothing->case break(isRight . snd)cases of
     (before,(ident,Right a):rest)->do
      remaining before
      failAt ident a(BatchHostTimeout,"runner did not read the batch wire")rest
     _->finish
    Just()->frames(0::Int)cases
 entries<-reverse<$>readIORef statuses
 pure BatchSummary
  { summaryStatus=status,summaryBatchDigest=Just batchDigest,summaryRunnerDigest=Just hash,summaryReason=reason
  , summaryFailedCase=failed,summaryRunnerExit=exit,summaryStderr=Just errors,summaryCases=entries}
