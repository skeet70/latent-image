{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as Elf
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word16, Word32, Word8)
import qualified Data.Text
import qualified Emulation.NativeCase as Native
import Emulation.OwnedCase (assemble)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

newtype Address = Address Word32 deriving (Eq, Show)
newtype MessageCode = MessageCode Word16
newtype Event = Event Word16
newtype Digest = Digest String deriving (Eq, Show)
data Preparation = Source | Mode deriving (Eq, Show)
data Role = Original | Owned | OmittedEntry deriving (Eq, Show)
data Cell = Primary | Secondary | Step | Command | Work | SourceWork | Preset deriving (Eq, Show)
data Scenario = Scenario
  { scenarioName :: String, scenarioPreparation :: Preparation, scenarioCode :: MessageCode
  , scenarioHeader :: Word32, scenarioKind :: Word32, scenarioCommand :: Word32, scenarioCancel :: Word32
  , scenarioPreset :: Word32, scenarioEvent :: Maybe Event, scenarioCopy :: Maybe (Int, Int)
  , scenarioChanges :: [(Cell, Word32)], scenarioCancelByte :: Maybe Word8
  , scenarioOutput :: Bool, scenarioDisplay :: Bool
  }
data Block = Block String Address String B.ByteString [(Address, Int)]

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""
addressValue :: Address -> Value
addressValue (Address value) = String (fromStringText (hex value))
fromStringText :: String -> Data.Text.Text
fromStringText = Data.Text.pack
hash :: B.ByteString -> Digest
hash = Digest . sha256Hex . sha256
hashText :: Digest -> String
hashText (Digest value) = value
ensure :: Bool -> String -> IO ()
ensure holds reason = unless holds (ioError (userError reason))
json :: FilePath -> Value -> IO ()
json path value = BL.writeFile path (encode value <> "\n")
put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes
wordAt :: Int -> Word32 -> B.ByteString -> B.ByteString
wordAt offset value = put offset (assemble [value])
cellOffset :: Cell -> Int
cellOffset cell = case cell of Primary -> 0x294; Secondary -> 0x298; Step -> 0x290; Command -> 0x2a0; Work -> 0x2ac; SourceWork -> 0x2d8; Preset -> 0x2d4
machine :: Preparation -> Word32
machine Source = 0
machine Mode = 11
originalEntry :: Preparation -> Address
originalEntry Source = Address 0x0120b40c
originalEntry Mode = Address 0x0120cab0
entryName :: Preparation -> String
entryName Source = "xm1_playback_source_entry"
entryName Mode = "xm1_playback_mode_entry"

scenario :: String -> Preparation -> Word16 -> Maybe Word16 -> Maybe (Int, Int) -> [(Cell, Word32)] -> Bool -> Scenario
scenario name preparation code event copied changes output = Scenario name preparation (MessageCode code) 0x87 0xcb 0xd5 0x80 0 (Event <$> event) copied changes Nothing output False

scenarios :: [Scenario]
scenarios =
  [ scenario "source-start-cb" Source 0x100 (Just 0) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,0),(Command,0xd5),(Work,0x87654321),(SourceWork,2)] True
  , (scenario "source-start-cf" Source 0x100 (Just 0) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,0),(Command,0xd5),(Work,0x87654321),(SourceWork,6)] True) {scenarioKind=0xcf}
  , (scenario "source-cancel-set" Source 0x101 (Just 1) (Just (0xd0,0xc4)) [(Secondary,0x12345678),(Step,0)] True) {scenarioCancelByte=Just 1}
  , (scenario "source-cancel-clear" Source 0x101 (Just 1) (Just (0xd0,0xc4)) [(Secondary,0x12345678),(Step,0)] True) {scenarioCancel=0x7f,scenarioCancelByte=Just 0}
  , scenario "source-query" Source 0x102 (Just 2) (Just (0xd0,0xc4)) [] True
  , scenario "source-first-movement" Source 0x103 (Just 3) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True
  , scenario "source-last-movement" Source 0x119 (Just 3) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True
  , scenario "source-reentry" Source 0x11e (Just 4) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,8),(Command,0xd5),(SourceWork,2)] True
  , scenario "source-option" Source 0x145 (Just 5) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True
  , scenario "source-catalogue" Source 0x128 (Just 6) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True
  , scenario "source-request" Source 0x11b (Just 7) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True
  , scenario "source-unknown-request" Source 0x120 Nothing (Just (0xd0,0xc4)) [] True
  , scenario "source-completion" Source 0x2a09 (Just 8) (Just (0x194,0x3c)) [] False
  , scenario "source-buffer-completion" Source 0x560c (Just 9) (Just (0x1d0,0x24)) [] False
  , scenario "source-result" Source 0x4f08 (Just 10) (Just (0x254,4)) [] False
  , scenario "source-other" Source 0x2a0a (Just 11) Nothing [] False
  , scenario "mode-primary" Mode 0x142 (Just 0) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,0)] True
  , scenario "mode-secondary" Mode 0x143 (Just 3) (Just (0xd0,0xc4)) [(Secondary,0x12345678),(Step,0)] True
  , scenario "mode-kind87" Mode 0x144 (Just 1) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,0)] True
  , (scenario "mode-kind88" Mode 0x144 (Just 2) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Step,0)] True) {scenarioHeader=0x88}
  , (scenario "mode-kind-refused" Mode 0x144 Nothing (Just (0xd0,0xc4)) [(Step,0)] True) {scenarioHeader=0x89}
  , (scenario "mode-first-display" Mode 0x103 (Just 4) (Just (0xd0,0xc4)) [(Primary,0x12345678),(Preset,1)] True) {scenarioDisplay=True}
  , (scenario "mode-repeated-display" Mode 0x119 (Just 4) (Just (0xd0,0xc4)) [(Primary,0x12345678)] True) {scenarioPreset=1}
  , scenario "mode-query" Mode 0x102 (Just 5) (Just (0xd0,0xc4)) [] True
  , scenario "mode-completion" Mode 0x2a09 (Just 6) (Just (0x194,0x3c)) [] False
  , scenario "mode-second-completion" Mode 0x2a0a (Just 7) (Just (0x194,0x3c)) [] False
  , scenario "mode-other" Mode 0x560c (Just 8) Nothing [] False
  , scenario "mode-unselected" Mode 0x120 Nothing Nothing [] False
  ]

payload :: Scenario -> B.ByteString
payload selected = foldr (uncurry wordAt) (B.pack [fromIntegral (i*37+11) | i <- [0::Int ..195]])
  [(4,0x12345678),(0x14,scenarioCancel selected),(0x18,scenarioKind selected),(0x1c,scenarioCommand selected),(0x5c,0x87654321)]

initialGlobals :: Scenario -> B.ByteString
initialGlobals selected = foldr (uncurry wordAt) withCode
  [(0x2ec,0x02000000),(0x318,scenarioHeader selected),(0x2d4,scenarioPreset selected)]
  where
    MessageCode code = scenarioCode selected
    withCode = put 0x2e4 (B.pack [fromIntegral code,fromIntegral (code `shiftR` 8)]) (B.replicate 0x1000 0xa5)

expectedGlobals :: Scenario -> B.ByteString
expectedGlobals selected = eventBytes (foldr (\(cell,value) -> wordAt (cellOffset cell) value) copied (scenarioChanges selected))
  where
    copied = case scenarioCopy selected of
      Nothing -> initialGlobals selected
      Just (offset,count) -> put (0x2e4+offset) (B.take count (payload selected)) (initialGlobals selected)
    eventBytes bytes = case scenarioEvent selected of
      Nothing -> bytes
      Just (Event event) -> put (0xec + fromIntegral (machine (scenarioPreparation selected))*2)
        (B.pack [fromIntegral event,fromIntegral (event `shiftR` 8)]) bytes

boundaryEvent :: Address -> [(String,Word32)] -> Word32 -> Value
boundaryEvent address inputs result = object ["address" .= addressValue address,"inputs" .= registerObject inputs,"outputs" .= registerObject [("r0",result)]]
registerObject :: [(String,Word32)] -> Value
registerObject entries = object [Data.Aeson.Key.fromString name .= hex value | (name,value) <- entries]

execute :: FilePath -> FilePath -> Elf.Elf -> B.ByteString -> Address -> [Block] -> Scenario -> Role -> IO Value
execute runner output elf runtime base stock selected role = do
  let preparation = scenarioPreparation selected
      ident = scenarioName selected ++ "-" ++ case role of Original->"original";Owned->"owned";OmittedEntry->"omitted-entry"
      ownedEntry = case [Elf.symbolValue symbol | symbol <- Elf.elfSymbols elf,Elf.symbolName symbol==entryName preparation,Elf.symbolKind symbol==Elf.FunctionSymbol] of
        [address] -> Address address
        _ -> error "missing unique owned entry"
      entry = if role == Original then originalEntry preparation else ownedEntry
      Address baseWord = base
      Address entryWord = ownedEntry
      image = if role == OmittedEntry then put (fromIntegral (entryWord-baseWord)) (assemble [0xe12fff1e]) runtime else runtime
      globals = initialGlobals selected
      cancel = B.replicate 256 0xa5
      cancelAfter = maybe cancel (\value -> put 0xf1 (B.singleton value) cancel) (scenarioCancelByte selected)
      returnWord = if maybe False (const True) (scenarioEvent selected) then 0x86543210 else 0
      calls = [boundaryEvent (Address 0x64982c) [("r0",2),("r1",8),("r2",0)] 0 | scenarioOutput selected]
        ++ [boundaryEvent (Address 0x6d1294) [("r0",1)] 0 | scenarioDisplay selected]
        ++ [boundaryEvent (Address 0x11e8bf8) [("r0",machine preparation)] 0x86543210 | maybe False (const True) (scenarioEvent selected)]
      services = [object ["address" .= addressValue address,"inputs" .= registerObject inputs,"outputs" .= registerObject [("r0",result)],"side_effects" .= ("registers-only"::String)]
        | (address,inputs,result) <- [(Address 0x64982c,[("r0",2),("r1",8),("r2",0)],0),(Address 0x6d1294,[("r0",1)],0),(Address 0x11e8bf8,[("r0",machine preparation)],0x86543210)]]
      codeFunctions = [(Address (Elf.symbolValue symbol),fromIntegral (Elf.symbolSize symbol)) | symbol<-Elf.elfSymbols elf,Elf.symbolKind symbol==Elf.FunctionSymbol,Elf.symbolSize symbol>0,Elf.symbolValue symbol>=baseWord,Elf.symbolValue symbol<baseWord+fromIntegral(B.length runtime)]
      blocks = stock ++ [Block "owned-runtime" base "rx" image codeFunctions
        ,Block "globals" (Address 0x14ea400) "rw" globals [],Block "cancel" (Address 0x1511400) "rw" cancel []
        ,Block "payload" (Address 0x2000000) "r" (payload selected) [],Block "stack" (Address 0x2200000) "rw" (B.replicate 0x4000 0) []
        ,Block "exit" (Address 0x100000) "rx" (assemble [0xe12fff1e]) []]
      artifact (Block name _ _ bytes _) = object ["id" .= name,"path" .= (ident++"-"++name++".bin"),"size" .= B.length bytes,"sha256" .= hashText(hash bytes)]
      region (Block name address permissions bytes _) = object ["id" .= name,"address" .= addressValue address,"size" .= B.length bytes,"permissions" .= permissions,"initial" .= object ["artifact" .= name,"offset" .= (0::Int),"length" .= B.length bytes]]
      registers = [("r"++show i,0x10101010+fromIntegral i) | i<-[0::Int ..12]]++[("sp",0x2204000),("lr",0x100000),("cpsr",0x10)]
      preserved = [(name,value) | (name,value)<-registers,name `elem` (["r"++show i|i<-[4::Int ..11]]++["sp"])]
      document = object
        ["schema_version" .= (1::Int),"id" .= ident,"origin" .= ("synthetic"::String)
        ,"provenance" .= ("Pinned X-M1 preparation bytes and exact linked experiment runtime; retained dispatch/output/preset contracts are explicit native boundary stubs"::String)
        ,"cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)]
        ,"entry" .= addressValue entry,"exit" .= addressValue (Address 0x100000),"registers" .= registerObject registers
        ,"artifacts" .= map artifact blocks,"regions" .= map region blocks
        ,"functions" .= [object ["address" .= addressValue address,"size" .= size] | Block _ _ _ _ functions<-blocks,(address,size)<-functions]
        ,"stubs" .= services,"limits" .= object ["instructions" .= (100000::Int),"wall_ms" .= (1000::Int)]
        ,"ignored_memory" .= [object ["region" .= ("stack"::String),"offset" .= (0::Int),"length" .= (0x4000::Int),"reason" .= ("private-frame"::String)]]
        ,"expected" .= object ["status" .= ("returned"::String),"registers" .= registerObject ([("r0",returnWord),("pc",0x100000)]++preserved)
          ,"memory" .= [object ["region" .= ("globals"::String),"offset" .= (0::Int),"hex" .= Native.hexBytes(expectedGlobals selected)],object ["region" .= ("cancel"::String),"offset" .= (0::Int),"hex" .= Native.hexBytes cancelAfter]],"events" .= calls]
        ,"exclusions" .= ["Native preparation boundary with declared retained-service stubs, not machine actions, full playback or hardware acceptance"::String]]
  forM_ blocks $ \(Block name _ _ bytes _) -> B.writeFile (output </> ident++"-"++name++".bin") bytes
  let path=output </> ident++".json"
  json path document
  (status,report)<-Native.runCase runner path
  json (output </> ident++"-result.json") report
  let reportField key (Object fields) = KM.lookup key fields
      reportField _ _ = Nothing
      returned = (reportField "runner" report >>= reportField "status") == Just (String "returned")
      controlRejected = status /= 0 && reportField "status" report == Just (String "failed") && returned
  if role == OmittedEntry then ensure controlRejected "omitted-entry control did not execute a returned behavioral failure"
    else ensure (status==0 && returned) (ident++" failed; inspect its native receipt")
  pure (object ["case" .= ident,"role" .= show role,"boundary_holds" .= (status==0),"control_rejected" .= (role==OmittedEntry && status/=0),"runner_sha256" .= KM.lookup "runner_sha256" (case report of Object fields->fields;_->KM.empty)])

readPinned :: FilePath -> Digest -> IO B.ByteString
readPinned path expected = do
  bytes<-B.readFile path
  ensure(hash bytes==expected)("pinned input mismatch: "++path)
  pure bytes

main :: IO ()
main = do
  arguments<-getArgs
  (runner,package,output)<-case arguments of
    ["--runner",runner,"--package",package,"--output",output,"--native-jobs","1"] -> pure(runner,package,output)
    _ -> ioError(userError "qualify --runner NATIVE_RUNNER --package LINKED_EXPERIMENT_DIR --output NEW_DIR --native-jobs 1")
  exists<-doesPathExist output
  ensure(not exists)"output directory exists"
  createDirectory output
  runtime<-B.readFile(package </> "runtime.bin")
  elfBytes<-B.readFile(package </> "kernel.elf")
  elf<-either (ioError.userError) pure (Elf.parseElf elfBytes)
  metadata<-B.readFile(package </> "experiment.json") >>= either (ioError.userError) pure . eitherDecodeStrict'
  base<-case metadata of
    Object fields | Just(String text)<-KM.lookup "base" fields,[(value,"")]<-readHex(drop 2(Data.Text.unpack text)) -> pure(Address value)
    _ -> ioError(userError "experiment metadata lacks base")
  let Address baseWord=base
  forM_ (Elf.elfSections elf) $ \section -> if Elf.sectionExecutable section && Elf.sectionSize section>0 && Elf.sectionAddress section>=baseWord then do
      let offset=fromIntegral(Elf.sectionAddress section-baseWord)
          original=B.take(fromIntegral(Elf.sectionSize section))(B.drop(fromIntegral(Elf.sectionOffset section))elfBytes)
      ensure(B.take(B.length original)(B.drop offset runtime)==original)("runtime differs from linked ELF section "++Elf.sectionName section)
    else pure ()
  plain<-readPinned "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin" (Digest "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  image4<-readPinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (Digest "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  let helpers=[(0x120b40c,876),(0x120cab0,708),(0x121cae8,32),(0x121cc2c,32),(0x121d794,28),(0x121d7b0,36),(0x121d7d4,36),(0x121e678,32),(0x121e698,156),(0x124b0dc,32),(0x124b110,40),(0x1255e94,56)]
      codeBlock ident bytes imageBase (address,size)=Block ident (Address address) "rx" (B.take size(B.drop(fromIntegral(address-imageBase))bytes)) [(Address address,size)]
      stock=zipWith (\index extent->codeBlock("stock-"++show index)plain 0x1100000 extent)[0::Int ..]helpers
        ++[codeBlock "stock-copy" image4 0x61c400 (0x722054,292),codeBlock "runtime-move" image4 0x61c400 (0x89d908,308)]
  reports<-fmap concat . forM scenarios $ \selected -> mapM (execute runner output elf runtime base stock selected) [Original,Owned]
  controls<-forM (take 1 scenarios ++ take 1(filter((==Mode).scenarioPreparation)scenarios)) $ \selected -> execute runner output elf runtime base stock selected OmittedEntry
  runnerSha<-Native.runnerDigest runner
  json(output </> "result.json")(object ["schema" .= (1::Int),"qualified" .= True,"scope" .= ("native preparation-pair ABI/state/retained-service boundary, exact packaged runtime; integrated PLAY is separate"::String),"runtime_sha256" .= hashText(hash runtime),"elf_sha256" .= hashText(hash elfBytes),"runner_sha256" .= runnerSha,"native_jobs" .= (1::Int),"scenarios" .= length scenarios,"executions" .= reports,"controls" .= controls])
  putStrLn("playback preparation boundary passed "++show(length scenarios)++" original/owned pairs and "++show(length controls)++" omission controls")
