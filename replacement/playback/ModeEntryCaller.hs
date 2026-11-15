{-# LANGUAGE OverloadedStrings #-}
module ModeEntryCaller (run) where

import qualified ArmElf as Elf
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Foldable (toList)
import Data.List.NonEmpty (NonEmpty (..))
import qualified Data.List.NonEmpty as NE
import qualified Data.Text as T
import Data.Word (Word16, Word32)
import qualified Emulation.NativeCase as Native
import Emulation.OwnedCase (assemble)
import qualified Emulation.PlaybackOwned as Playback
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import System.Directory (createDirectory, doesPathExist)
import System.FilePath ((</>))

newtype Address = Address Word32 deriving (Eq, Show)
newtype Message = Message Word16
newtype Event = Event Word16
newtype DispatchResult = DispatchResult Word32
newtype Digest = Digest String deriving (Eq, Show)
data Action = Action2 | Action3 deriving (Eq, Show)
data Role = Original | Owned | Omitted deriving (Eq, Show)
data MessageCase = MessageCase
  { messageName :: String, message :: Message, header :: Word32
  , event :: Maybe Event, copied :: Maybe (Int, Int), changes :: [(Int, Word32)]
  , outputCall :: Bool, presetCall :: Bool, initialPreset :: Word32
  }
data Scenario = Scenario Action MessageCase DispatchResult
data BlockId = StockPlain | StockCopy | StockMove | OwnedRuntime | Globals | Payload | PrivateStack | ExitSentinel deriving (Eq, Show)
data Permission = ReadOnly | ReadWrite | ReadExecute
data Block = Block BlockId Address Permission B.ByteString [(Address, Int)]

blockName :: BlockId -> String
blockName kind = case kind of
  StockPlain -> "stock-plain"
  StockCopy -> "stock-copy"
  StockMove -> "stock-move"
  OwnedRuntime -> "owned-runtime"
  Globals -> "globals"
  Payload -> "payload"
  PrivateStack -> "stack"
  ExitSentinel -> "exit"

permissionName :: Permission -> String
permissionName ReadOnly = "r"
permissionName ReadWrite = "rw"
permissionName ReadExecute = "rx"

ensure :: Bool -> String -> IO ()
ensure yes reason = unless yes (ioError (userError reason))
hex :: Word32 -> String
hex value = "0x" ++ replicate (8-length digits) '0' ++ digits where digits=showHex value ""
av :: Address -> Value
av (Address value) = String (T.pack (hex value))
hash :: B.ByteString -> Digest
hash = Digest . sha256Hex . sha256
hashText :: Digest -> String
hashText (Digest value) = value
json :: FilePath -> Value -> IO ()
json path value = BL.writeFile path (encode value <> "\n")
field :: T.Text -> Value -> Maybe Value
field key (Object fields) = KM.lookup (K.fromText key) fields
field _ _ = Nothing
required :: Either String a -> IO a
required = either (ioError . userError) pure
put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put offset bytes target = B.take offset target <> bytes <> B.drop (offset+B.length bytes) target
wordAt :: Int -> Word32 -> B.ByteString -> B.ByteString
wordAt offset value = put offset (assemble [value])
halfAt :: Int -> Word16 -> B.ByteString -> B.ByteString
halfAt offset value = put offset (B.pack [fromIntegral value,fromIntegral(value `shiftR` 8)])
registers :: [(String, Word32)] -> Value
registers entries = object [K.fromString name .= hex value | (name,value)<-entries]
readPinned :: FilePath -> Digest -> IO B.ByteString
readPinned path digest = do
  bytes<-B.readFile path
  ensure (hash bytes==digest) ("pinned input mismatch: "++path)
  pure bytes

messages :: NonEmpty MessageCase
messages =
  selected "primary" 0x142 (Just 0) (Just (0xd0,0xc4)) [(0x294,0x12345678),(0x290,0)] True :|
  [ selected "secondary" 0x143 (Just 3) (Just (0xd0,0xc4)) [(0x298,0x12345678),(0x290,0)] True
  , selected "kind87" 0x144 (Just 1) (Just (0xd0,0xc4)) [(0x294,0x12345678),(0x290,0)] True
  , (selected "kind88" 0x144 (Just 2) (Just (0xd0,0xc4)) [(0x294,0x12345678),(0x290,0)] True) {header=0x88}
  , (selected "kind-refused" 0x144 Nothing (Just (0xd0,0xc4)) [(0x290,0)] True) {header=0x89}
  , (selected "first-display" 0x103 (Just 4) (Just (0xd0,0xc4)) [(0x294,0x12345678),(0x2d4,1)] True) {presetCall=True}
  , (selected "repeated-display" 0x119 (Just 4) (Just (0xd0,0xc4)) [(0x294,0x12345678)] True) {initialPreset=1}
  , selected "query" 0x102 (Just 5) (Just (0xd0,0xc4)) [] True
  , selected "completion" 0x2a09 (Just 6) (Just (0x194,0x3c)) [] False
  , selected "second-completion" 0x2a0a (Just 7) (Just (0x194,0x3c)) [] False
  , selected "other" 0x560c (Just 8) Nothing [] False
  , selected "unselected" 0x120 Nothing Nothing [] False
  ]
  where
    selected name code emitted copy writes output = MessageCase name (Message code) 0x87 (Event <$> emitted) copy writes output False 0

initialCursor :: Action -> Word16
initialCursor Action2 = 0
initialCursor Action3 = 1
caller :: Action -> Address
caller Action2 = Address 0x011ea434
caller Action3 = Address 0x011ea458

messageRoutedToCaller :: Action -> MessageCase -> Bool
messageRoutedToCaller Action2 selected = let Message code = message selected in code >= 0x142 && code <= 0x144
messageRoutedToCaller Action3 _ = True
scenarioId :: Scenario -> String
scenarioId (Scenario action selected (DispatchResult result)) = (case action of Action2->"action2";Action3->"action3")++"-"++messageName selected++"-"++drop 2(hex result)
payload :: B.ByteString
payload = foldr (uncurry wordAt) (B.pack [fromIntegral(i*37+11)|i<-[0::Int ..195]]) [(4,0x12345678),(0x14,0x80),(0x18,0xcb),(0x1c,0xd5),(0x5c,0x87654321)]
globals :: Scenario -> B.ByteString
globals (Scenario action selected _) = halfAt 0x1ae (initialCursor action) $ halfAt 0x2e4 code $ foldr (uncurry wordAt) (B.replicate 0x1000 0xa5) [(0x2ec,0x02000000),(0x318,header selected),(0x2d4,initialPreset selected)]
  where Message code=message selected
returnValue :: Scenario -> Word32
returnValue (Scenario _ selected (DispatchResult result)) = case event selected of Nothing->0;Just _->result
expectedGlobals :: Scenario -> B.ByteString
expectedGlobals scenario@(Scenario action selected _) = halfAt 0x1ae cursor emitted
  where
    copiedBytes = case copied selected of Nothing->globals scenario;Just(offset,count)->put (0x2e4+offset) (B.take count payload) (globals scenario)
    changed = foldr (uncurry wordAt) copiedBytes (changes selected)
    emitted = case event selected of Nothing->changed;Just(Event value)->halfAt 0x102 value changed
    cursor = case action of Action2->if returnValue scenario/=3 then 1 else 0;Action3->if returnValue scenario==3 then 0 else 1

boundary :: Address -> [(String,Word32)] -> Word32 -> Value
boundary address inputs result = object ["address" .= av address,"inputs" .= registers inputs,"outputs" .= registers [("r0",result)]]

execute :: FilePath -> FilePath -> Elf.Elf -> Address -> B.ByteString -> B.ByteString -> [Block] -> Address -> Scenario -> Role -> IO Value
execute runner output elf base@(Address baseWord) runtime published stock (Address ownedWord) scenario@(Scenario action selected (DispatchResult dispatch)) role = do
  let ident=scenarioId scenario++case role of Original->"-original";Owned->"-owned";Omitted->"-omitted"
      image=if role==Omitted then put (fromIntegral(ownedWord-baseWord)) (assemble [0xe12fff1e]) runtime else runtime
      publishBlock (Block name address permissions bytes functions) = Block name address permissions
        (if name==StockPlain && role/=Original then put (0x120cab0-0x11ea434) published bytes else bytes) functions
      codeFunctions=[(Address(Elf.symbolValue symbol),fromIntegral(Elf.symbolSize symbol))|symbol<-Elf.elfSymbols elf,Elf.symbolKind symbol==Elf.FunctionSymbol,Elf.symbolSize symbol>0,Elf.symbolValue symbol>=baseWord,Elf.symbolValue symbol<baseWord+fromIntegral(B.length runtime)]
      blocks=map publishBlock stock++[Block OwnedRuntime base ReadExecute image codeFunctions,Block Globals (Address 0x14ea400) ReadWrite (globals scenario) [],Block Payload (Address 0x2000000) ReadOnly payload [],Block PrivateStack (Address 0x2200000) ReadWrite (B.replicate 0x4000 0) [],Block ExitSentinel (Address 0x100000) ReadExecute (assemble [0xe12fff1e]) []]
      serviceRows=[(Address 0x64982c,[("r0",2),("r1",8),("r2",0)],0),(Address 0x6d1294,[("r0",1)],0),(Address 0x11e8bf8,[("r0",11)],dispatch)]
      calls=[boundary (Address 0x64982c) [("r0",2),("r1",8),("r2",0)] 0|outputCall selected]++[boundary (Address 0x6d1294) [("r0",1)] 0|presetCall selected]++[boundary (Address 0x11e8bf8) [("r0",11)] dispatch|Just _<-[event selected]]
      initial=[("r"++show i,0x10101010+fromIntegral i)|i<-[0::Int ..12]]++[("sp",0x2204000),("lr",0x100000),("cpsr",0x10)]
      preserved=[(name,value)|(name,value)<-initial,name `elem` (["r"++show i|i<-[4::Int ..11]]++["sp"])]
      artifact (Block name _ _ bytes _) = object ["id" .= blockName name,"path" .= (ident++"-"++blockName name++".bin"),"size" .= B.length bytes,"sha256" .= hashText(hash bytes)]
      region (Block name address permissions bytes _) = object ["id" .= blockName name,"address" .= av address,"size" .= B.length bytes,"permissions" .= permissionName permissions,"initial" .= object ["artifact" .= blockName name,"offset" .= (0::Int),"length" .= B.length bytes]]
      document=object
        ["schema_version" .= (1::Int),"id" .= ident,"origin" .= ("synthetic"::String)
        ,"provenance" .= ("Pinned stock machine1 action caller and mode body versus exact installed publication/runtime; conditional retained dispatch/output/preset services"::String)
        ,"cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)]
        ,"entry" .= av(caller action),"exit" .= av(Address 0x100000),"registers" .= registers initial
        ,"artifacts" .= map artifact blocks,"regions" .= map region blocks
        ,"functions" .= [object ["address" .= av address,"size" .= size]|Block _ _ _ _ functions<-blocks,(address,size)<-functions]
        ,"stubs" .= [object ["address" .= av address,"inputs" .= registers inputs,"outputs" .= registers [("r0",result)],"side_effects" .= ("registers-only"::String)]|(address,inputs,result)<-serviceRows]
        ,"limits" .= object ["instructions" .= (100000::Int),"wall_ms" .= (1000::Int)]
        ,"ignored_memory" .= [object ["region" .= ("stack"::String),"offset" .= (0::Int),"length" .= (0x4000::Int),"reason" .= ("private-frame"::String)]]
        ,"expected" .= object ["status" .= ("returned"::String),"registers" .= registers ([("r0",returnValue scenario),("pc",0x100000)]++preserved),"memory" .= [object ["region" .= ("globals"::String),"offset" .= (0::Int),"hex" .= Native.hexBytes(expectedGlobals scenario)]],"events" .= calls]
        ,"exclusions" .= [scope]]
  forM_ blocks $ \(Block name _ _ bytes _) -> B.writeFile (output </> ident++"-"++blockName name++".bin") bytes
  let path=output </> ident++".json"
  json path document
  (status,report)<-Native.runCase runner path
  json (output </> ident++"-result.json") report
  let returned=(field "runner" report >>= field "status")==Just(String "returned")
  if role==Omitted
    then ensure (status/=0 && field "status" report==Just(String "failed") && returned) (ident++": omission must return with a behavioral mismatch")
    else ensure (status==0 && field "status" report==Just(String "passed") && returned) (ident++": native case failed; inspect receipt")
  pure report

scope :: String
scope = "Conditional retained machine1 action2/action3 caller coverage only. Dispatcher result is a declared contract, not an executed machine11 action outcome. No natural application-mode7 path, integrated JPEG pair acceptance, whole mode-machine acceptance, retirement or unload."

observable :: Value -> Value
observable report = object ["registers" .= object [K.fromString name .= (runner >>= field "registers" >>= field(T.pack name))|name<-["r0","r4","r5","r6","r7","r8","r9","r10","r11","sp","pc"]],"memory" .= memory,"events" .= (runner >>= field "events"),"status" .= (runner >>= field "status")]
  where
    runner=field "runner" report
    memory=case runner >>= field "memory" of Just(Array rows)->filter (\row->field "address" row `elem` [Just(av(Address 0x14ea400)),Just(av(Address 0x2000000))]) (toList rows);_->[]

run :: FilePath -> FilePath -> FilePath -> IO Value
run runner package output = do
  exists<-doesPathExist output
  ensure (not exists) "mode caller output directory exists"
  createDirectory output
  runnerSha<-Native.runnerDigest runner
  ensure (runnerSha=="1cc6903f16aa6026fc0bdaf8ba0b2024ebbebefcbac22e76c49cea02ffe1426e") "native runner identity differs"
  runtime<-readPinned (package </> "runtime.bin") (Digest "6efe8757c85116d73713450dfaaa2c939bb930d9b72ca0836ac62930a37ff890")
  elfBytes<-readPinned (package </> "kernel.elf") (Digest "917ddbd5f626f9159820ea9a2ec0887c32fd16489de3b6f6564745d302c14872")
  manifest<-B.readFile(package </> "ram-push-manifest.json")
  metadata<-B.readFile(package </> "experiment.json")
  parsed<-required(Playback.parsePackage manifest metadata runtime elfBytes)
  elf<-required(Elf.parseElf elfBytes)
  plain<-readPinned "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin" (Digest "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  image4<-readPinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (Digest "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  required(Playback.checkOriginalBodies parsed [B.take count(B.drop(fromIntegral(address-0x1100000))plain)|(address,count)<-Playback.originalBodies parsed])
  (base,runtimeArtifact)<-case Playback.guestArtifacts parsed of (address,bytes):_->pure(Address address,bytes);_->ioError(userError "package runtime artifact missing")
  ensure (runtimeArtifact==runtime) "package runtime artifact differs"
  published<-B.concat <$> forM [0x120cab0,0x120cab4] (\address->case [bytes|(location,bytes)<-Playback.guestArtifacts parsed,location==address] of [bytes]|B.length bytes==4->pure bytes;_->ioError(userError "mode publication missing"))
  ownedEntry<-case [Address(Elf.symbolValue symbol)|symbol<-Elf.globalSymbols elf,Elf.symbolName symbol=="xm1_playback_mode_entry",Elf.symbolKind symbol==Elf.FunctionSymbol] of [address]->pure address;_->ioError(userError "mode ELF target missing")
  let stockFunctions=[(0x11ea434,36),(0x11ea458,36),(0x120cab0,708),(0x121cae8,32),(0x121cc2c,32),(0x121d794,28),(0x121d7b0,36),(0x121d7d4,36),(0x121e678,32),(0x121e698,156),(0x124b0dc,32),(0x124b110,40),(0x1255e94,56)]
      stock=[Block StockPlain (Address 0x11ea434) ReadExecute (B.take (0x1255ecc-0x11ea434)(B.drop(0x11ea434-0x1100000)plain)) [(Address address,count)|(address,count)<-stockFunctions]
        ,Block StockCopy (Address 0x722054) ReadExecute (B.take 292(B.drop(0x722054-0x61c400)image4)) [(Address 0x722054,292)]
        ,Block StockMove (Address 0x89d908) ReadExecute (B.take 308(B.drop(0x89d908-0x61c400)image4)) [(Address 0x89d908,308)]]
      scenarios=[Scenario action selected (DispatchResult result)|action<-[Action2,Action3],selected<-toList messages,messageRoutedToCaller action selected,result<-case event selected of Nothing->[0];Just _->[0,3,0x00010003,0x86543210]]
  forM_ [("runtime.bin",runtime),("kernel.elf",elfBytes),("ram-push-manifest.json",manifest),("experiment.json",metadata),("mode-publication.bin",published)] $ \(name,bytes)->B.writeFile(output </> name)bytes
  comparisons<-forM scenarios $ \scenario->do
    original<-execute runner output elf base runtime published stock ownedEntry scenario Original
    owned<-execute runner output elf base runtime published stock ownedEntry scenario Owned
    ensure (observable original==observable owned) (scenarioId scenario++": original/owned caller observations differ")
    pure(object ["case" .= scenarioId scenario,"consumer_observations_equal" .= True,"original_receipt" .= (scenarioId scenario++"-original-result.json"),"owned_receipt" .= (scenarioId scenario++"-owned-result.json")])
  controls<-forM [Scenario action (NE.head messages) (DispatchResult 3)|action<-[Action2,Action3]] $ \scenario->do
    _<-execute runner output elf base runtime published stock ownedEntry scenario Omitted
    pure(object ["case" .= scenarioId scenario,"returned_behavioral_omission_rejected" .= True,"receipt" .= (scenarioId scenario++"-omitted-result.json")])
  let result=object ["schema" .= (1::Int),"qualified" .= True,"conditional_caller_coverage" .= True,"scope" .= scope,"natural_application_mode7" .= False,"integrated_jpeg_pair_acceptance" .= False,"whole_mode_machine_acceptance" .= False,"runner_sha256" .= runnerSha,"runtime_sha256" .= hashText(hash runtime),"elf_sha256" .= hashText(hash elfBytes),"manifest_sha256" .= hashText(hash manifest),"metadata_sha256" .= hashText(hash metadata),"plain_sha256" .= hashText(hash plain),"image4_sha256" .= hashText(hash image4),"publication_hex" .= Native.hexBytes published,"owned_entry" .= av ownedEntry,"comparisons" .= comparisons,"controls" .= controls]
  json (output </> "result.json") result
  pure result
