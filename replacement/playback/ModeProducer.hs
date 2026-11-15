{-# LANGUAGE OverloadedStrings #-}
module ModeProducer (run) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.NativeCase as Native
import Emulation.OwnedCase (assemble)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.FilePath ((</>))

newtype Address = Address Word32 deriving (Eq, Show)
newtype Digest = Digest String deriving (Eq, Show)
newtype Token = Token Word32
newtype SubmitStatus = SubmitStatus Word32
newtype Task = Task Word32
data Scenario = Accepted | Rejected | OmittedProducer deriving (Eq, Show)
data BlockId = UiAction5 | PacketProducer | QueueDescriptor | SubmitStatusMemory | PrivateStack | ExitSentinel deriving (Eq, Show)
data Permission = ReadWrite | ReadExecute
data Block = Block BlockId Address Permission B.ByteString Bool

blockName :: BlockId -> String
blockName kind = case kind of
  UiAction5 -> "ui-m5-action-5"
  PacketProducer -> "packet-producer"
  QueueDescriptor -> "queue-descriptor"
  SubmitStatusMemory -> "submit-status"
  PrivateStack -> "stack"
  ExitSentinel -> "exit"

permissionName :: Permission -> String
permissionName ReadWrite = "rw"
permissionName ReadExecute = "rx"

digest :: B.ByteString -> Digest
digest = Digest . sha256Hex . sha256

digestText :: Digest -> String
digestText (Digest text) = text

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where digits = showHex value ""

address :: Address -> Value
address (Address value) = String (T.pack (hex value))

registers :: [(String, Word32)] -> Value
registers pairs = object [Key.fromString name .= hex value | (name, value) <- pairs]

ensure :: Bool -> String -> IO ()
ensure holds reason = unless holds (ioError (userError reason))

json :: FilePath -> Value -> IO ()
json path value = BL.writeFile path (encode value <> "\n")

replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace offset bytes original = B.take offset original <> bytes <> B.drop (offset + B.length bytes) original

readPinned :: FilePath -> Digest -> IO B.ByteString
readPinned path expected = do
  bytes <- B.readFile path
  ensure (digest bytes == expected) ("pinned input mismatch: " ++ path)
  pure bytes

field :: String -> Value -> Maybe Value
field name (Object fields) = KM.lookup (Key.fromString name) fields
field _ _ = Nothing

sourcePath, sourceRevision, packagePath, scope :: String
sourcePath = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"
sourceRevision = "55436fc85c0560dd252b0410af6f2427fe198cd3"
packagePath = "private-data/scratch/PlaybackMediaClosure/inputs/install/relinked"
scope = "conditional actual stock UI machine 5 action 5, packet producer and queue-4 descriptor boundary; no modeled USB route, natural device-input path, installed mode acceptance or whole mode acceptance"

sourceDigest, runtimeDigest, elfDigest, runnerDigest :: Digest
sourceDigest = Digest "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
runtimeDigest = Digest "6efe8757c85116d73713450dfaaa2c939bb930d9b72ca0836ac62930a37ff890"
elfDigest = Digest "917ddbd5f626f9159820ea9a2ec0887c32fd16489de3b6f6564745d302c14872"
runnerDigest = Digest "1cc6903f16aa6026fc0bdaf8ba0b2024ebbebefcbac22e76c49cea02ffe1426e"

stockBodies :: [(BlockId, Address, [Word32])]
stockBodies =
  [ (UiAction5, Address 0xc256a8,
      [0xe92d4800,0xe28db004,0xe24dd008,0xe3a03000,0xe50b3008,0xe30802c0
      ,0xeb044312,0xe24b3004,0xe5230004,0xe1a00003,0xeb063994,0xe3043580
      ,0xe3403094,0xe30027e8,0xe3402001,0xe7830002,0xe3500000,0x0a000000
      ,0xebe837a7,0xe3a00001,0xe24bd004,0xe8bd8800])
  , (PacketProducer, Address 0xdb3d28,
      [0xe92d4800,0xe28db004,0xe24dd0c8,0xe5903000,0xe50b30c4,0xe3000142
      ,0xe50b0080,0xe3a010c4,0xe24b20c8,0xebfffc60,0xe24bd004,0xe8bd8800])
  , (QueueDescriptor, Address 0xdb2ed4,
      [0xe92d4830,0xe28db00c,0xe24dd010,0xe1a04001,0xe1a05002,0xe14b01b8
      ,0xebe5c371,0xe54b0016,0xe3a00004,0xe54b0015,0xe50b4014,0xe50b5010
      ,0xe24b1018,0xebe5c84b,0xe24bd00c,0xe8bd8830])
  ]

stockBlocks :: B.ByteString -> IO [Block]
stockBlocks image = forM stockBodies $ \(name, location@(Address base), words') -> do
  let expected = assemble words'
      actual = B.take (B.length expected) (B.drop (fromIntegral (base - 0xbf1a00)) image)
  ensure (actual == expected) ("stock instruction mismatch: " ++ blockName name)
  pure (Block name location ReadExecute actual True)

scenarioName :: Scenario -> String
scenarioName selected = case selected of
  Accepted -> "submit-accepted"
  Rejected -> "submit-rejected"
  OmittedProducer -> "omitted-packet-producer"

submitStatus :: Scenario -> SubmitStatus
submitStatus Rejected = SubmitStatus 0x80000005
submitStatus _ = SubmitStatus 0

token :: Token
token = Token 0x82c1

task :: Task
task = Task 0x33

entrySp, payloadPointer, descriptorPointer, exitPc :: Word32
entrySp = 0x2204000
payloadPointer = entrySp - 220
descriptorPointer = entrySp - 252
exitPc = 0x100000

expectedPayload :: B.ByteString
expectedPayload = B.pack [0xa5,0xa5,0xa5,0xa5,0xc1,0x82,0,0]
  <> B.replicate 64 0xa5 <> B.pack [0x42,0x01,0,0] <> B.replicate 120 0xa5

expectedDescriptor :: B.ByteString
expectedDescriptor = B.pack [0x42,0x01,0x33,0x04,0xc4,0,0,0,0x24,0x3f,0x20,0x02]

memoryRead :: String -> Value -> Int -> B.ByteString -> Value
memoryRead selector target offset bytes = object
  [Key.fromString selector .= target, "offset" .= offset, "hex" .= Native.hexBytes bytes]

scriptCall :: [(String, Word32)] -> Word32 -> [Value] -> Value
scriptCall inputs result memoryReads = object
  ["inputs" .= registers inputs, "outputs" .= registers [("r0",result)], "reads" .= memoryReads, "writes" .= ([] :: [Value])]

stub :: Address -> [Value] -> Value
stub target calls = object ["address" .= address target, "calls" .= calls]

event :: Address -> [(String, Word32)] -> Word32 -> Value
event target inputs result = object
  ["address" .= address target,"inputs" .= registers inputs,"outputs" .= registers [("r0",result)]]

execute :: FilePath -> FilePath -> [Block] -> Scenario -> IO Value
execute runner output stock selected = do
  let ident = scenarioName selected
      directory = output </> ident
      SubmitStatus status = submitStatus selected
      Token allocated = token
      Task current = task
      omitted = selected == OmittedProducer
      actualStatus = status
      errorCalled = selected == Rejected
      altered (Block name base permissions bytes function) =
        Block name base permissions (if omitted && name == PacketProducer then replace 0 (assemble [0xe3a00000,0xe12fff1e]) bytes else bytes) function
      blocks = map altered stock ++
        [Block SubmitStatusMemory (Address 0x954d68) ReadWrite (assemble [0xdeadbeef]) False
        ,Block PrivateStack (Address 0x2200000) ReadWrite (B.replicate 0x4000 0xa5) False
        ,Block ExitSentinel (Address exitPc) ReadExecute (assemble [0xe12fff1e]) False]
      initial = [("r" ++ show i,0x10101010 + fromIntegral i) | i <- [0 :: Int .. 12]]
        ++ [("sp",entrySp),("lr",exitPc),("cpsr",0x10)]
      preserved = [(name,value) | (name,value) <- initial,name `elem` (["r" ++ show i | i <- [4 :: Int .. 11]] ++ ["sp"])]
      statusRead = memoryRead "address" (address (Address 0x954d68)) 0 (assemble [actualStatus])
      services =
        [stub (Address 0xd36310) [scriptCall [("r0",0x82c0)] allocated []]
        ,stub (Address 0x723cb8) [scriptCall [] current []]
        ,stub (Address 0x72503c) [scriptCall [("r0",4),("r1",descriptorPointer)] status
          [memoryRead "register" (String "r1") 0 expectedDescriptor
          ,memoryRead "address" (address (Address payloadPointer)) 0 expectedPayload]]
        ,stub (Address 0x633594) [scriptCall [("r0",actualStatus)] 0 [statusRead] | errorCalled]]
      calls = [event (Address 0xd36310) [("r0",0x82c0)] allocated
        ,event (Address 0x723cb8) [] current
        ,event (Address 0x72503c) [("r0",4),("r1",descriptorPointer)] status]
        ++ [event (Address 0x633594) [("r0",status)] 0 | selected == Rejected]
      artifact (Block name _ _ bytes _) = object
        ["id" .= blockName name,"path" .= (blockName name ++ ".bin"),"size" .= B.length bytes,"sha256" .= digestText (digest bytes)]
      region (Block name base permissions bytes _) = object
        ["id" .= blockName name,"address" .= address base,"size" .= B.length bytes,"permissions" .= permissionName permissions
        ,"initial" .= object ["artifact" .= blockName name,"offset" .= (0 :: Int),"length" .= B.length bytes]]
      document = object
        ["schema_version" .= (1 :: Int),"id" .= ident,"origin" .= ("stock" :: String)
        ,"provenance" .= ("Pinned stock ARM instructions, independently specified descriptor and full 196-byte private payload, explicit token/task/queue/error boundaries; " ++ sourceRevision)
        ,"cpu" .= object ["architecture" .= ("arm" :: String),"model" .= ("cortex-r5f" :: String),"mode" .= ("arm" :: String),"endian" .= ("little" :: String)]
        ,"entry" .= address (Address 0xc256a8),"exit" .= address (Address exitPc),"registers" .= registers initial
        ,"artifacts" .= map artifact blocks,"regions" .= map region blocks
        ,"functions" .= [object ["address" .= address base,"size" .= B.length bytes] | Block _ base _ bytes True <- blocks]
        ,"stubs" .= services,"limits" .= object ["instructions" .= (256 :: Int),"wall_ms" .= (1000 :: Int)]
        ,"ignored_memory" .= [object ["region" .= ("stack" :: String),"offset" .= (0 :: Int),"length" .= (0x4000 :: Int),"reason" .= ("private-frame" :: String)]]
        ,"expected" .= object ["status" .= ("returned" :: String),"registers" .= registers ([("r0",1),("pc",exitPc)] ++ preserved)
          ,"memory" .= [object ["region" .= ("submit-status" :: String),"offset" .= (0 :: Int),"hex" .= Native.hexBytes (assemble [status])]],"events" .= calls]
        ,"exclusions" .= [scope]]
  createDirectory directory
  forM_ blocks $ \(Block name _ _ bytes _) -> B.writeFile (directory </> blockName name ++ ".bin") bytes
  json (directory </> "case.json") document
  (code, receipt) <- Native.runCase runner (directory </> "case.json")
  json (directory </> "native-receipt.json") receipt
  let returned = (field "runner" receipt >>= field "status") == Just (String "returned")
      behavioralFailure = code /= 0 && returned && field "status" receipt == Just (String "failed")
      holds = if omitted then behavioralFailure else code == 0 && returned
      result = object ["case" .= ident,"accepted" .= holds,"returned" .= returned
        ,"negative_control" .= omitted,"control_rejected_behaviorally" .= (omitted && behavioralFailure)
        ,"boundary_holds" .= (not omitted && code == 0 && returned),"receipt" .= (ident </> "native-receipt.json")]
  json (directory </> "result.json") result
  pure result

run :: FilePath -> FilePath -> FilePath -> IO Value
run runner package output = do
  exists <- doesPathExist output
  ensure (not exists) "mode producer output directory exists"
  createDirectory output
  runnerHash <- Digest <$> Native.runnerDigest runner
  ensure (runnerHash == runnerDigest) "mode producer native runner identity mismatch"
  actualPackage <- canonicalizePath package
  requiredPackage <- canonicalizePath packagePath
  ensure (actualPackage == requiredPackage) "mode producer requires the exact installed relinked package"
  runtime <- readPinned (package </> "runtime.bin") runtimeDigest
  elf <- readPinned (package </> "kernel.elf") elfDigest
  source <- readPinned sourcePath sourceDigest
  stock <- stockBlocks source
  reports <- forM [Accepted,Rejected,OmittedProducer] (execute runner output stock)
  let accepted = all ((== Just (Bool True)) . field "accepted") reports
      result = object ["schema_version" .= (1 :: Int),"qualified" .= accepted,"scope" .= scope
        ,"installed_mode_acceptance" .= False,"source_revision" .= sourceRevision
        ,"source" .= object ["path" .= sourcePath,"sha256" .= digestText (digest source)]
        ,"package" .= actualPackage,"runtime_sha256" .= digestText (digest runtime)
        ,"elf_sha256" .= digestText (digest elf),"runner_sha256" .= digestText runnerHash
        ,"sdk" .= ("/nix/store/6g84apamlbqxf162nwy4964hf49rc91f-fujimod-xm1-0.1.0" :: String)
        ,"executions" .= reports]
  json (output </> "result.json") result
  ensure accepted "conditional mode producer boundary failed; inspect retained native receipts"
  pure result
