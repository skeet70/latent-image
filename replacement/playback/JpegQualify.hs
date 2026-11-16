{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (nub)
import qualified Data.Text as T
import Data.Word (Word16, Word32)
import qualified Emulation.NativeCase as N
import qualified Emulation.NativeObservation as O
import Emulation.OwnedCase (assemble, regionBytesOf)
import qualified Emulation.PlaybackOwned as P
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import qualified PlaybackContract as C
import Numeric (showHex)
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs, getExecutablePath)
import System.FilePath ((</>))

newtype Address = Address Word32 deriving (Eq, Show)
newtype Bytes = Bytes Word32 deriving (Eq, Show)
newtype Pixels = Pixels Word16 deriving (Eq, Show)
newtype Digest = Digest String deriving (Eq, Show)
newtype CaseName = CaseName String deriving (Eq, Show)
data Dimensions = Dimensions Pixels Pixels deriving (Eq, Show)
data Fixture = Preview | Full deriving (Eq, Show)
data Role = Original | Owned | OmittedIrq | OmittedFirst | ReenteredFirst | UngatedFirst deriving (Eq, Show)
data Interrupt = Decoded | DecodedPriority | Limit | DeviceError | NeedInput | IgnoredInput deriving (Eq, Show)
data JobStatus = RequestComplete | RequestDeviceError | RequestOutputLimit | RequestSetupFailed deriving (Eq, Show)
newtype Coordinate = Coordinate Word32 deriving (Eq, Show)
data Window = Whole | Cropped Coordinate Coordinate Coordinate Coordinate deriving (Eq, Show)
data Scale = FullSize | HalfSize | QuarterSize | EighthSize | UnknownScale deriving (Eq, Show)
data Layout = EightRows | SixteenRows deriving (Eq, Show)
data Callbacks = Deliver | Absent | PrimaryOnly | SecondaryOnly | Reenter deriving (Eq, Show)
data CallbackPublication = RegisteredCallbacks | NullCallbacks deriving (Eq, Show)
data Record = NoRecord | WithRecord deriving (Eq, Show)
data Resource = NewResource | ExistingResource deriving (Eq, Show)
data Attachment = Attached | Refused deriving (Eq, Show)
data Continuation = Finish | Restart deriving (Eq, Show)
data Unlock = Signalled | AlreadySignalled deriving (Eq, Show)
data Register = R0 | R1 | R2 | R3 | Ip deriving (Eq, Show)
data RegionId = Driver | Playback | Descriptor | NextDescriptor | RecordMemory | Consumer | Stack | Runtime | StockDriver | StockPlayback | StockDevice | DeviceRegisters | Caller | FirstConsumer | SecondConsumer | Exit | Producer | Signal | Defect | DevelopOperand | DevelopPlane deriving (Eq, Show)
data Permission = ReadOnly | ReadWrite | ReadExecute deriving (Eq, Show)
data Block = Block RegionId Address Permission B.ByteString [(Address, Int)]
data Memory = Absolute Address B.ByteString | Relative Register B.ByteString
data Service = Service Address [(Register, Word32)] [(Register, Word32)] [Memory] [Memory]
data Scenario = Scenario
  { name :: CaseName, fixture :: Fixture, dimensions :: Dimensions, outputDimensions :: Dimensions
  , capacity :: Bytes, produced :: Bytes, endpoint :: Address, interrupt :: Interrupt
  , window :: Window, scale :: Scale, layout :: Layout, callbacks :: Callbacks, callbackPublication :: CallbackPublication, record :: Record
  , resource :: Resource, attachment :: Attachment, continuation :: Continuation, unlock :: Unlock
  }
data Observation = Observation B.ByteString B.ByteString B.ByteString B.ByteString deriving (Eq, Show)

hex :: Word32 -> T.Text
hex value = T.pack ("0x" ++ replicate (8 - length digits) '0' ++ digits) where digits = showHex value ""
av :: Address -> Value
av (Address value) = String (hex value)
hash :: B.ByteString -> Digest
hash = Digest . sha256Hex . sha256
hashText :: Digest -> String
hashText (Digest value) = value
ensure :: Bool -> String -> IO ()
ensure holds reason = unless holds (ioError (userError reason))
required :: Either String a -> IO a
required = either (ioError . userError) pure
json :: FilePath -> Value -> IO ()
json path value = BL.writeFile path (encode value <> "\n")
field :: T.Text -> Value -> Value
field key (Object fields) = maybe Null id (KM.lookup (K.fromText key) fields)
field _ _ = Null
replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes
wordsAt :: [(Int, Word32)] -> B.ByteString -> B.ByteString
wordsAt changes bytes = foldr (\(offset,value) -> replace offset (assemble [value])) bytes changes
byteAt :: Int -> Word32 -> B.ByteString -> B.ByteString
byteAt offset value = replace offset (B.singleton (fromIntegral value))
registerName :: Register -> T.Text
registerName selected = case selected of R0 -> "r0"; R1 -> "r1"; R2 -> "r2"; R3 -> "r3"; Ip -> "r12"
registerNumber :: Register -> Word32
registerNumber selected = case selected of R0 -> 0; R1 -> 1; R2 -> 2; R3 -> 3; Ip -> 12
registers :: [(Register, Word32)] -> Value
registers pairs = object [K.fromText (registerName register) .= hex value | (register,value) <- pairs]
regionName :: RegionId -> String
regionName selected = case selected of
  Driver -> "driver-state"; Playback -> "playback-request"; Descriptor -> "decode-requirements"; NextDescriptor -> "next-input-requirements"
  RecordMemory -> "record-descriptor"; Consumer -> "consumer-result"; Stack -> "stack"; Runtime -> "owned-runtime"
  StockDriver -> "stock-driver"; StockPlayback -> "stock-playback"; Caller -> "transaction-caller"
  StockDevice -> "stock-device"; DeviceRegisters -> "register-image"
  FirstConsumer -> "first-consumer"; SecondConsumer -> "second-consumer"; Exit -> "exit"
  DevelopOperand -> "develop-operand"; DevelopPlane -> "develop-plane"
  Producer -> "develop-job4"; Signal -> "record-notification"; Defect -> "consumer-defect"
permission :: Permission -> String
permission selected = case selected of ReadOnly -> "r"; ReadWrite -> "rw"; ReadExecute -> "rx"

requestAddress, driverAddress, descriptorAddress, nextDescriptorAddress, consumerAddress, recordAddress, callerAddress, firstAddress, secondAddress, exitAddress :: Address
requestAddress = Address 0x014eae2c
driverAddress = Address 0x014c56f0
descriptorAddress = Address 0x02000000
nextDescriptorAddress = Address 0x02000040
consumerAddress = Address 0x02000100
recordAddress = Address 0x02000200
callerAddress = Address 0x00110000
firstAddress = Address 0x00110100
secondAddress = Address 0x00110200
exitAddress = Address 0x00100000

preview :: Scenario
preview = Scenario (CaseName "actual-preview") Preview (Dimensions (Pixels 320) (Pixels 240)) (Dimensions (Pixels 320) (Pixels 240))
  (Bytes 153600) (Bytes 153600) (Address 0x43862400) Decoded Whole FullSize EightRows Deliver RegisteredCallbacks NoRecord NewResource Attached Finish Signalled
full :: Scenario
full = preview {name=CaseName "actual-full", fixture=Full, dimensions=Dimensions (Pixels 4896) (Pixels 2760)
  ,outputDimensions=Dimensions (Pixels 4896) (Pixels 2760),capacity=Bytes 27025920,produced=Bytes 27025920,endpoint=Address 0x45202e00}
scenarios :: [Scenario]
scenarios =
  [ preview, full
  , preview {name=CaseName "done-priority",interrupt=DecodedPriority,record=WithRecord}
  , preview {name=CaseName "output-limit",interrupt=Limit,produced=Bytes 0,record=WithRecord}
  , preview {name=CaseName "input-error",interrupt=DeviceError,produced=Bytes 0,record=WithRecord}
  , preview {name=CaseName "input-needed",interrupt=NeedInput,produced=Bytes 0,record=WithRecord}
  , preview {name=CaseName "input-ignored",interrupt=IgnoredInput,produced=Bytes 0}
  , preview {name=CaseName "input-restart-done",interrupt=NeedInput,continuation=Restart,record=WithRecord}
  , preview {name=CaseName "attachment-refused",attachment=Refused,produced=Bytes 0,record=WithRecord}
  , preview {name=CaseName "retained-resource-already-signalled",resource=ExistingResource,unlock=AlreadySignalled}
  , preview {name=CaseName "no-playback-subscribers",callbacks=Absent,record=WithRecord}
  , preview {name=CaseName "null-completion-callback",callbackPublication=NullCallbacks,record=WithRecord}
  , preview {name=CaseName "null-input-callback",interrupt=NeedInput,produced=Bytes 0,callbackPublication=NullCallbacks,record=WithRecord}
  , preview {name=CaseName "sixteen-row-layout",dimensions=Dimensions (Pixels 17) (Pixels 9),outputDimensions=Dimensions (Pixels 32) (Pixels 16)
      ,layout=SixteenRows,capacity=Bytes 1024,produced=Bytes 1024,endpoint=Address 0x4383d000}
  , preview {name=CaseName "half-size",dimensions=Dimensions (Pixels 31) (Pixels 17),outputDimensions=Dimensions (Pixels 16) (Pixels 8)
      ,scale=HalfSize,capacity=Bytes 1536,produced=Bytes 256,endpoint=Address 0x4383cd00}
  , preview {name=CaseName "quarter-window",dimensions=Dimensions (Pixels 64) (Pixels 40),outputDimensions=Dimensions (Pixels 16) (Pixels 8)
      ,scale=QuarterSize,window=Cropped (Coordinate 35) (Coordinate 4) (Coordinate 3) (Coordinate 26),capacity=Bytes 5120,produced=Bytes 256,endpoint=Address 0x4383cd00}
  , preview {name=CaseName "eighth-size",dimensions=Dimensions (Pixels 63) (Pixels 63),outputDimensions=Dimensions (Pixels 16) (Pixels 8)
      ,scale=EighthSize,capacity=Bytes 8192,produced=Bytes 256,endpoint=Address 0x4383cd00}
  , preview {name=CaseName "unknown-scale-default",dimensions=Dimensions (Pixels 32) (Pixels 9),outputDimensions=Dimensions (Pixels 32) (Pixels 16)
      ,scale=UnknownScale,capacity=Bytes 1024,produced=Bytes 1024,endpoint=Address 0x4383d000}
  , preview {name=CaseName "window-word-wrap",dimensions=Dimensions (Pixels 16) (Pixels 8),outputDimensions=Dimensions (Pixels 0) (Pixels 0)
      ,window=Cropped (Coordinate 0xffffffff) (Coordinate 0) (Coordinate 20) (Coordinate 19),capacity=Bytes 256,produced=Bytes 0,endpoint=Address 0x4383cc00}
  , preview {name=CaseName "endpoint-word-wrap",produced=Bytes 0xffffffff,endpoint=Address 0x4383cbff}
  ]

source :: Fixture -> (Address, Bytes)
source Preview = (Address 0x46780c00, Bytes 21489)
source Full = (Address 0x45fc9c00, Bytes 3515192)
scaleInput, scaleDevice :: Scale -> Word32
scaleInput selected = case selected of FullSize -> 0; HalfSize -> 1; QuarterSize -> 2; EighthSize -> 3; UnknownScale -> 4
scaleDevice UnknownScale = 0
scaleDevice selected = scaleInput selected
windowWords :: Window -> [(Int, Word32)]
windowWords Whole = [(0x38,0),(0x3c,0),(0x40,0),(0x44,0),(0x48,0)]
windowWords (Cropped (Coordinate right) (Coordinate left) (Coordinate top) (Coordinate bottom)) = [(0x38,1),(0x3c,right),(0x40,left),(0x44,top),(0x48,bottom)]
kind :: Scenario -> Word32
kind selected = if interrupt selected == NeedInput then 1 else 2
sourceWords :: Scenario -> Bool -> (Word32, Word32)
sourceWords selected next = let (Address address,Bytes count)=source (fixture selected)
  in if next then (address+32,count-32) else (address,count)
callbackWords :: Callbacks -> (Word32, Word32)
callbackWords Absent = (0,0)
callbackWords PrimaryOnly = let Address first=firstAddress in (first,0)
callbackWords SecondaryOnly = let Address second=secondAddress in (0,second)
callbackWords Reenter = callbackWords Deliver
callbackWords Deliver = let Address first=firstAddress; Address second=secondAddress in (first,second)
descriptor :: Scenario -> Bool -> B.ByteString
descriptor selected next = wordsAt [(8,kind selected),(12,if layout selected==EightRows then 0 else 1)
  ,(0x14,address),(0x18,count),(0x1c,0x4383cc00),(0x20,first),(0x24,second)]
  (replace 16 (B.pack [fromIntegral width,fromIntegral(width `shiftR` 8),fromIntegral height,fromIntegral(height `shiftR` 8)]) (B.replicate 0x28 0xa5))
  where
    Dimensions (Pixels width) (Pixels height)=dimensions selected
    (address,count)=sourceWords selected next
    (first,second)=callbackWords (callbacks selected)

initialPlayback :: B.ByteString
initialPlayback = wordsAt [(0x54,1)] (B.replicate 0xb0 0xa5)
initialDriver :: Scenario -> B.ByteString
initialDriver selected = wordsAt [(0,if record selected==WithRecord then 0x02000200 else 0)
  ,(4,if resource selected==ExistingResource then 0x1e123456 else 0),(0x198,0),(0x19c,0)] (B.replicate 0x210 0xa5)
configuration :: Scenario -> Bool -> B.ByteString
configuration selected next = assemble
  ([address,count,0x4383cc00,fromIntegral width,fromIntegral height,capacityBytes,packing,scaleDevice (scale selected)]
  ++ crop ++ [0,0,0,0,1,0,1,0,1,1,1,1,1,1,0])
  where
    (address,count)=sourceWords selected next
    Dimensions (Pixels width) (Pixels height)=dimensions selected
    Bytes capacityBytes=capacity selected
    packing=if layout selected==EightRows then 1 else 2
    crop=case window selected of Whole->[0,0,0,0,0];Cropped (Coordinate right) (Coordinate left) (Coordinate top) (Coordinate bottom)->[1,left,right,top,bottom]

completionStatus :: Scenario -> JobStatus
completionStatus selected
  | attachment selected==Refused = RequestSetupFailed
  | continuation selected==Restart = RequestComplete
  | otherwise = case interrupt selected of Limit->RequestOutputLimit;DeviceError->RequestDeviceError;_->RequestComplete
statusWord :: JobStatus -> Word32
statusWord selected = case selected of RequestComplete->0;RequestDeviceError->1;RequestOutputLimit->2;RequestSetupFailed->3
primaryDelivered, secondaryDelivered :: Scenario -> Bool
primaryDelivered selected = callbacks selected `elem` [Deliver,Reenter] && (interrupt selected/=NeedInput || continuation selected==Restart || attachment selected==Refused || callbackPublication selected==NullCallbacks)
secondaryDelivered selected = callbacks selected `elem` [Deliver,Reenter] && callbackPublication selected==RegisteredCallbacks && interrupt selected==NeedInput && attachment selected==Attached
outputEvent, inputEvent :: Scenario -> Word32
outputEvent selected = if attachment selected==Attached && (interrupt selected `elem` [Decoded,DecodedPriority] || continuation selected==Restart) then 0x87654321 else 0
inputEvent selected = if attachment selected==Attached && interrupt selected==DeviceError then 0x13579bdf else 0
finalProduced :: Scenario -> Word32
finalProduced selected = let Bytes count=produced selected in count
expectedPlayback :: Scenario -> B.ByteString
expectedPlayback selected = byteAt 0xa1 0 . byteAt 0x60 1 $ wordsAt
  ([(0,kind selected),(4,if layout selected==EightRows then 0 else 1),(8,fromIntegral width),(12,fromIntegral height)
   ,(0x10,fromIntegral outputWidth),(0x14,fromIntegral outputHeight),(0x18,address),(0x1c,count),(0x20,0x4383cc00)
   ,(0x24,finalProduced selected),(0x28,statusWord (completionStatus selected)),(0x2c,inputEvent selected),(0x30,outputEvent selected)
   ,(0x34,scaleInput (scale selected)),(0x4c,if callbackPublication selected==RegisteredCallbacks then 0x121fec8 else 0)
   ,(0x50,if callbackPublication selected==RegisteredCallbacks then 0x121ff20 else 0),(0x58,0),(0x5c,0x33001234)
   ,(0xa4,if primaryDelivered selected then 0 else first),(0xa8,if secondaryDelivered selected && continuation selected/=Restart then 0 else second)]
   ++ windowWords (window selected)) initialPlayback
  where
    Dimensions (Pixels width) (Pixels height)=dimensions selected
    Dimensions (Pixels outputWidth) (Pixels outputHeight)=outputDimensions selected
    (address,count)=sourceWords selected (continuation selected==Restart)
    (first,second)=callbackWords (callbacks selected)
expectedDriver :: Scenario -> B.ByteString
expectedDriver selected = replace 0x1a0 (configuration selected (continuation selected==Restart))
  (wordsAt [(4,0),(0x198,0x014eae2c),(0x19c,0)] (initialDriver selected))
expectedRecord :: Scenario -> B.ByteString
expectedRecord selected
  | record selected==NoRecord || attachment selected==Refused || interrupt selected==IgnoredInput = B.replicate 0x40 0xa5
  | completionStatus selected==RequestComplete = byteAt 0x36 0 . byteAt 0x35 1 $ B.replicate 0x40 0xa5
  | otherwise = byteAt 0x36 1 . byteAt 0x35 2 $ B.replicate 0x40 0xa5
expectedConsumer :: Scenario -> B.ByteString
expectedConsumer selected = wordsAt ([(0,if primaryDelivered selected then 1 else 0),(4,if secondaryDelivered selected then 1 else 0)]
  ++ [(offset,value)|(offset,value)<-zip [8,12,16,20,24] [statusWord (completionStatus selected),finalProduced selected,inputEvent selected,outputEvent selected,phase],primaryDelivered selected]
  ++ [(offset,0)|offset<-[28,32,36,40,44],secondaryDelivered selected]) (B.replicate 48 0)
  where phase=if callbackPublication selected==NullCallbacks || (interrupt selected==IgnoredInput && attachment selected==Attached) then 1 else 0

load :: Register -> Word32 -> [Word32]
load register value = [wide 0xe3000000 (value .&. 0xffff),wide 0xe3400000 (value `shiftR` 16)]
  where wide opcode immediate = opcode .|. (registerNumber register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)
call :: Address -> [Word32]
call (Address target) = load Ip target ++ [0xe12fff3c]
caller :: Scenario -> B.ByteString
caller selected = assemble $ [0xe92d4010] ++ load R0 0x02000000 ++ call (Address 0x1220004)
  ++ call (Address 0x12200c4) ++ overrides ++ call (Address 0x1220294)
  ++ [word|attachment selected==Attached,word<-call (Address 0xd99570)]
  ++ [word|continuation selected==Restart,word<-load R0 0x02000040 ++ call (Address 0x1220004) ++ call (Address 0x12200ec) ++ call (Address 0xd99570)]
  ++ call (Address 0x1220238) ++ [0xe3a0005a,0xe8bd8010]
  where
    changes=(0x34,scaleInput (scale selected)):windowWords (window selected)
      ++ [(offset,0)|offset<-[0x4c,0x50],callbackPublication selected==NullCallbacks]
    overrides=load R1 0x014eae2c ++ concat [load R0 value ++ [0xe5810000 .|. fromIntegral offset]|(offset,value)<-changes]
consumer :: Bool -> Bool -> B.ByteString
consumer secondary reentry = assemble $ [0xe92d4010] ++ load R1 0x014eae2c ++ load Ip 0x02000100
  ++ [0xe59c0000 .|. countOffset,0xe2800001,0xe58c0000 .|. countOffset]
  ++ concat [[0xe5910000 .|. input,0xe58c0000 .|. output]|(input,output)<-zip [0x28,0x24,0x2c,0x30] [snapshot,snapshot+4..snapshot+12]]
  ++ [0xe5d10060,0xe58c0000 .|. (snapshot+16)]
  ++ (if reentry && not secondary then load R1 0x2000100 ++ [0xe5910000,0xe3500001,0x1a000002] ++ call(Address 0x121fec8) else [])
  ++ [0xe8bd8010]
  where countOffset=if secondary then 4 else 0; snapshot=if secondary then 28 else 8

memoryJson :: Memory -> Value
memoryJson selected = case selected of
  Absolute address bytes -> object ["address" .= av address,"offset" .= (0::Int),"hex" .= N.hexBytes bytes]
  Relative register bytes -> object ["register" .= registerName register,"offset" .= (0::Int),"hex" .= N.hexBytes bytes]
serviceJson :: [Service] -> [Value]
serviceJson plan = [object ["address" .= av target,"calls" .= [object ["inputs" .= registers inputs,"outputs" .= registers outputs
  ,"reads" .= map memoryJson memoryReads,"writes" .= map memoryJson memoryWrites]|Service address inputs outputs memoryReads memoryWrites<-plan,address==target]]
  |target<-nub [address|Service address _ _ _ _<-plan]]
eventJson :: Service -> Value
eventJson (Service address inputs outputs _ _) = object ["address" .= av address,"inputs" .= registers inputs,"outputs" .= registers outputs]
service :: Word32 -> [(Register,Word32)] -> [Memory] -> [Memory] -> Service
service address inputs = Service (Address address) inputs [(R0,0)]
critical :: [Service]
critical = [Service (Address 0x7235d8) [] [(R0,0x77)] [] [],service 0x7236b0 [(R0,0x77)] [] []]
services :: Scenario -> Role -> [Service]
services selected role = [service 0x724b68 [(R0,9),(R1,0xffffffff)] [] []]
  ++ [service 0x65a3cc [(R0,0x14c56f4)] [] [Relative R0 (assemble [0x1e123456])]|resource selected==NewResource]
  ++ [service 0x65a478 [] [] [],service 0x65a58c [(R0,0x14c5890)] [Relative R0 (configuration selected False)] []
     ,Service (Address 0x64e918) [(R0,0x48),(R1,0xd99570),(R2,0x14c588c)] [(R0,if attachment selected==Attached then 0 else 0x86540003)] []
        [Relative R2 (assemble [0x76123456])|attachment selected==Attached]]
  ++ (if attachment selected==Attached then [service 0x42634 [(R0,0),(R1,0)] [] [],service 0x65a548 [] [] []] else irqCritical)
  ++ [timer]
  ++ (if attachment selected==Attached && role/=OmittedIrq then irq (interrupt selected) else [])
  ++ (if continuation selected==Restart then [timerCache,service 0x65aa44 [(R0,0x14c5890)] [Relative R0 (configuration selected True)] [],timer] ++ irq Decoded else [])
  ++ [service 0x69b7a4 [(R0,0x33001234)] [] [],service 0x65a478 [] [] []]
  ++ [service 0x64eb20 [(R0,0x76123456)] [] []|attachment selected==Attached]
  ++ [service 0x65a424 [(R0,0x1e123456)] [] [],Service (Address 0x724c00) [(R0,9)] [(R0,if unlock selected==Signalled then 0 else 0xffffffd5)] [] []]
  ++ critical ++ [entry | role /= OmittedFirst, entry <- critical]
  where
    timer=service 0x69b328 [(R0,3000),(R1,0),(R2,0x121ab58),(R3,0x14eae88)] [] [Relative R3 (assemble [0x33001234])]
    timerCache=service 0x42634 [(R0,0),(R1,0)] [] []
    irqCritical=if role/=OmittedFirst && callbackPublication selected==RegisteredCallbacks
      then critical++[entry|callbacks selected==Reenter,entry<-critical] else []
    irq outcome = [service 0x65ab20 [] [] [Relative R0 (assemble [bits outcome])],service 0x65ab50 [(R0,bits outcome)] [] []]
      ++ case outcome of
        Decoded -> done
        DecodedPriority -> done
        Limit -> irqCritical
        DeviceError -> [service 0x65ab7c [] [] [Relative R0 (assemble [0x13579bdf])],service 0x65abac [(R0,0x13579bdf)] [] []] ++ irqCritical
        NeedInput -> irqCritical
        IgnoredInput -> []
    done=[service 0x65abd8 [] [] [Relative R0 (assemble [0x87654321])],service 0x65ac08 [(R0,0x87654321)] [] []
      ,service 0x65a9e4 [(R0,0x14eae50)] [] [Relative R0 (let Address value=endpoint selected in assemble [value])]] ++ irqCritical
    bits outcome=case outcome of Decoded->0x10;DecodedPriority->0x01020010;Limit->0x20000;DeviceError->8;NeedInput->0x1000000;IgnoredInput->0x1000000

scope :: String
scope = "Source-bound channel9 CPU comparisons. Exact8 additionally executes the generic record builder and retained develop job4 producer, plus owned first-completion reached through retained Playback facade/IRQ callers and real publication bytes. Develop job4 setup-failure executes its source notification callback before the conditional wait; notification and wait providers remain declared boundaries. Its stale record error byte survives synchronous setup failure, and release remains the retained companion. First-completion reentry calls the published entry from the subscriber after critical leave. Native device-service, resource, IRQ registry, cache, timer and critical providers are conditional boundaries. Polling and device acceptance require the separate full-system model run. No native pixels, IRQ timing, event-flag latching, natural USB transition, capture, retirement or unload claim."

execute :: FilePath -> FilePath -> P.Package -> E.Elf -> Address -> B.ByteString -> [(Address,B.ByteString)] -> [Block] -> Scenario -> Role -> IO Observation
execute runner output package elf base@(Address baseWord) runtime publications stock selected role = do
  let CaseName label=name selected
      ident=label++roleSuffix role
      irqEntry=case [E.symbolValue symbol|symbol<-E.globalSymbols elf,E.symbolName symbol=="xm1_playback_jpeg_interrupt",E.symbolKind symbol==E.FunctionSymbol] of [target]->target;_->error "IRQ entry missing"
      firstEntry=case [E.symbolValue symbol|symbol<-E.globalSymbols elf,E.symbolName symbol=="xm1_playback_jpeg_complete_first",E.symbolKind symbol==E.FunctionSymbol] of [target]->target;_->error "first completion entry missing"
      image=case role of
        OmittedIrq -> replace (fromIntegral (irqEntry-baseWord)) (assemble [0xe12fff1e]) runtime
        OmittedFirst -> replace (fromIntegral (firstEntry-baseWord)) (assemble [0xe12fff1e]) runtime
        ReenteredFirst -> replace (fromIntegral (firstEntry-baseWord)) (assemble(load Ip 0x110300++[0xe12fff1c])) runtime
        UngatedFirst -> replace (fromIntegral (firstEntry-baseWord)) (assemble(load Ip 0x110300++[0xe12fff1c])) runtime
        _ -> runtime
      deviceTargets = [(C.bodyStart body,target) | role /= Original
        , (selectedService,target,_) <- P.deviceServices (Just package)
        , let (body,_) = C.jpegDeviceContract selectedService]
      boundaryTargets = map snd deviceTargets
      publish block@(Block kind' address@(Address start) permissions bytes extents)
        | kind' `elem` [StockDriver,StockPlayback] && role/=Original = Block kind' address permissions
            (foldr (\(Address location,value) original->replace (fromIntegral (location-start)) value original) bytes
              [(location,value)|(location@(Address target),value)<-publications,target>=start,target<start+fromIntegral(B.length bytes)]) extents
        | otherwise = block
      functions=[(Address(E.symbolValue symbol),fromIntegral(E.symbolSize symbol))|symbol<-E.elfSymbols elf,E.symbolKind symbol==E.FunctionSymbol,E.symbolSize symbol>0
        ,E.symbolValue symbol>=baseWord,E.symbolValue symbol<baseWord+fromIntegral(B.length runtime)
        ,not(any (\target -> E.symbolValue symbol<=target && target<E.symbolValue symbol+E.symbolSize symbol) boundaryTargets)]
      codeBlock kind' address bytes=Block kind' address ReadExecute bytes [(address,B.length bytes)]
      blocks=map publish stock++[Block Runtime base ReadExecute image functions,Block Driver driverAddress ReadWrite (initialDriver selected) []
        ,Block Playback requestAddress ReadWrite initialPlayback [],Block Descriptor descriptorAddress ReadOnly (descriptor selected False) []
        ,Block NextDescriptor nextDescriptorAddress ReadOnly (descriptor selected True) [],Block RecordMemory recordAddress ReadWrite (B.replicate 0x40 0xa5) []
        ,Block Consumer consumerAddress ReadWrite (B.replicate 48 0) [],Block Stack (Address 0x02200000) ReadWrite (B.replicate 0x4000 0) []
        ,codeBlock Caller callerAddress (caller selected),codeBlock FirstConsumer firstAddress (consumer False (callbacks selected==Reenter)),codeBlock SecondConsumer secondAddress (consumer True False)
        ,Block Exit exitAddress ReadExecute (assemble [0xe12fff1e]) []]
        ++ [codeBlock Defect (Address 0x110300) (if role==ReenteredFirst then lateClearFirst else ungatedFirst)|role `elem` [ReenteredFirst,UngatedFirst]]
      calls=[Service (maybe address Address (lookup original deviceTargets)) inputs outputs memoryReads memoryWrites
        |Service address@(Address original) inputs outputs memoryReads memoryWrites<-services selected role]
      expected=[(Driver,expectedDriver selected),(Playback,expectedPlayback selected),(Consumer,expectedConsumer selected),(RecordMemory,expectedRecord selected)]
  observed<-runNative runner output ident blocks calls expected (role `elem` [OmittedIrq,OmittedFirst,ReenteredFirst,UngatedFirst])
  let returned=O.nativeStatus observed==O.Returned
  json (output </> ident++"-identity.json") (object ["role" .= show role,"package_runtime_sha256" .= hashText(hash runtime),"executed_runtime_sha256" .= hashText(hash image)
    ,"irq_entry" .= av(Address irqEntry),"mutation" .= show role,"runner_returned" .= returned])
  pure (Observation (regionBytesOf observed (O.RegionId (regionName Driver))) (regionBytesOf observed (O.RegionId (regionName Playback)))
    (regionBytesOf observed (O.RegionId (regionName Consumer))) (regionBytesOf observed (O.RegionId (regionName RecordMemory))))

roleSuffix :: Role -> String
roleSuffix role = case role of
  Original->"-stock";Owned->"-owned";OmittedIrq->"-omitted-irq";OmittedFirst->"-omitted-first";ReenteredFirst->"-late-clear-first";UngatedFirst->"-ungated-first"

lateClearFirst :: B.ByteString
lateClearFirst = assemble $ [0xe92d4010] ++ call(Address 0x7235d8) ++ load R1 0x14eae2c
  ++ [0xe59140a4,0xe3540000,0x0a000008] ++ call(Address 0x7236b0)
  ++ [0xe12fff34] ++ load R1 0x14eae2c ++ [0xe3a00000,0xe58100a4,0xe8bd8010]
  ++ call(Address 0x7236b0) ++ [0xe8bd8010]

ungatedFirst :: B.ByteString
ungatedFirst = assemble $ [0xe92d4010] ++ call(Address 0x7235d8) ++ load R1 0x14eae2c
  ++ [0xe59140a4,0xe3540000,0x0a000006,0xe3a02000,0xe58120a4] ++ call(Address 0x7236b0)
  ++ [0xe12fff34,0xe8bd8010] ++ call(Address 0x7236b0) ++ [0xe8bd8010]

runNative :: FilePath -> FilePath -> String -> [Block] -> [Service] -> [(RegionId,B.ByteString)] -> Bool -> IO O.NativeObservation
runNative runner output ident blocks calls expected defect = do
  let artifact (Block kind' _ _ bytes _)=object ["id" .= regionName kind',"path" .= (ident++"-"++regionName kind'++".bin"),"size" .= B.length bytes,"sha256" .= hashText(hash bytes)]
      region (Block kind' address permissions bytes _)=object ["id" .= regionName kind',"address" .= av address,"size" .= B.length bytes,"permissions" .= permission permissions
        ,"initial" .= object ["artifact" .= regionName kind',"offset" .= (0::Int),"length" .= B.length bytes]]
      memory (kind',bytes)=object ["region" .= regionName kind',"offset" .= (0::Int),"hex" .= N.hexBytes bytes]
      preserved=[("r"++show index,0x10101010+fromIntegral index)|index<-[4::Int ..11]]++[("sp",0x02204000)]
      initialRegisters=[("r"++show index,0x10101010+fromIntegral index)|index<-[0::Int ..12]]++[("sp",0x02204000),("lr",0x00100000),("cpsr",0x10)]
      rawRegisters pairs=object [K.fromString register .= hex value|(register,value)<-pairs]
      document=object ["schema_version" .= (1::Int),"id" .= ident,"origin" .= ("synthetic"::String),"provenance" .= scope
        ,"cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)]
        ,"entry" .= av callerAddress,"exit" .= av exitAddress,"registers" .= rawRegisters initialRegisters,"artifacts" .= map artifact blocks,"regions" .= map region blocks
        ,"functions" .= [object ["address" .= av address,"size" .= size]|Block _ _ _ _ extents<-blocks,(address,size)<-extents]
        ,"stubs" .= serviceJson calls,"limits" .= object ["instructions" .= (100000::Int),"wall_ms" .= (1000::Int)]
        ,"expected" .= object ["status" .= ("returned"::String),"registers" .= rawRegisters ([("r0",0x5a),("pc",0x00100000)]++preserved)
          ,"memory" .= map memory expected
          ,"ignored_memory" .= [object ["region" .= regionName Stack,"offset" .= (0::Int),"length" .= (0x4000::Int)]]
          ,"events" .= map eventJson calls],"exclusions" .= [scope]]
  forM_ blocks $ \(Block kind' _ _ bytes _)->B.writeFile (output </> ident++"-"++regionName kind'++".bin") bytes
  let path=output </> ident++".json"
  json path document
  (_,regions,_,_)<-N.loadCase path
  (status,report)<-N.runCase runner path
  json (output </> ident++"-result.json") report
  runnerExit<-either (ioError.userError.O.renderObservationError) pure (O.reportRunnerExit report)
  observed<-either (ioError.userError.O.renderObservationError) pure (O.observeNative runnerExit regions (field "runner" report))
  let returned=O.nativeStatus observed==O.Returned
  ensure (returned && if defect then status/=0 && field "status" report==String "failed" else status==0 && field "status" report==String "passed")
    (ident++": native receipt must return with "++if defect then "a consumer-visible mismatch" else "matching consumer observations")
  pure observed

data BuilderCaller = DirectBuilder | DevelopJob4 deriving (Eq, Show)
data RecordMode = Mode7 | Mode8 | Mode5 | ModeNegative deriving (Eq, Show)
data RecordPacking = PackingZero | PackingOne | PackingOther deriving (Eq, Show)
data BuilderCase = BuilderCase CaseName BuilderCaller Scenario RecordMode RecordPacking
data BuilderDefect = MissingMode | MissingPacking | MissingWidth | MissingHeight | MissingSource | MissingCount | MissingDestination | MissingClear | MissingCallback deriving (Eq, Show)
newtype SourceOffset = SourceOffset Int
data BuilderMutation = BuilderMutation SourceOffset B.ByteString

builderDefectName :: BuilderDefect -> CaseName
builderDefectName selected = CaseName $ case selected of
  MissingMode -> "missing-mode"
  MissingPacking -> "missing-packing"
  MissingWidth -> "missing-width"
  MissingHeight -> "missing-height"
  MissingSource -> "missing-source"
  MissingCount -> "missing-count"
  MissingDestination -> "missing-destination"
  MissingClear -> "missing-clear"
  MissingCallback -> "missing-callback"

builderMode7Case, builderMode8Case, builderSetupFailureCase :: BuilderCase
builderMode7Case = BuilderCase (CaseName "builder-mode7") DirectBuilder preview Mode7 PackingZero
builderMode8Case = BuilderCase (CaseName "builder-mode8") DirectBuilder full Mode8 PackingOne
builderSetupFailureCase = BuilderCase (CaseName "builder-setup-failed") DirectBuilder (preview {attachment=Refused,produced=Bytes 0}) Mode5 PackingZero

builderCases :: [BuilderCase]
builderCases =
  [ builderMode7Case
  , builderMode8Case
  , BuilderCase (CaseName "builder-signed-default-strict-packing") DirectBuilder preview ModeNegative PackingOther
  , builderSetupFailureCase
  , BuilderCase (CaseName "develop-job4-setup-failed") DevelopJob4 (preview {attachment=Refused,produced=Bytes 0}) Mode5 PackingZero
  ]

builderMode :: RecordMode -> (Word32,Word32)
builderMode selected = case selected of Mode7->(7,1);Mode8->(8,2);Mode5->(5,0);ModeNegative->(0xff,0)
builderPacking :: RecordPacking -> Word32
builderPacking selected = case selected of PackingZero->0;PackingOne->1;PackingOther->2

executeBuilder :: FilePath -> FilePath -> P.Package -> E.Elf -> Address -> B.ByteString -> [(Address,B.ByteString)] -> [Block] -> B.ByteString -> BuilderCase -> Role -> Maybe BuilderDefect -> IO Observation
executeBuilder runner output package elf base@(Address baseWord) runtime publications stock image5 (BuilderCase (CaseName label) route selected mode packing) role defect = do
  let ident=label++roleSuffix role++maybe "" (\selectedDefect -> let CaseName suffix=builderDefectName selectedDefect in "-"++suffix) defect
      Dimensions (Pixels width) (Pixels height)=dimensions selected
      (sourceAddress,sourceBytes)=sourceWords selected False
      (recordMode,requestMode)=builderMode mode
      recordBase=if route==DevelopJob4 then 0x15b0bf8 else 0x2000200
      completed=if route==DevelopJob4 then 0xe8eb18 else 0x110100
      recordSize=if route==DevelopJob4 then 0xd8 else 0x65
      initialRecord=wordsAt [(0xc,0x4383cc00),(0x10,sourceAddress),(0x14,sourceBytes),(0x3c,completed),(0x40,0x110200)]
        . byteAt 0x34 recordMode . byteAt 0x64 (builderPacking packing) $ B.replicate recordSize 0xa5
      recordBefore=replace 0 (B.pack [fromIntegral height,fromIntegral(height `shiftR` 8),fromIntegral width,fromIntegral(width `shiftR` 8)]) initialRecord
      producerRecord=wordsAt [(0x5c,0),(0xd4,0x12345678)] . byteAt 0x34 5 . byteAt 0x60 0 . byteAt 0x61 0 . byteAt 0x4a 0 . byteAt 0x4b 1
        $ replace 0x48 (B.pack [3,0]) . replace 4 (B.pack [0,0,fromIntegral width,fromIntegral(width `shiftR` 8)]) $ recordBefore
      recordReady=if route==DevelopJob4 then producerRecord else recordBefore
      configurationBytes=configuration (selected {layout=if packing==PackingOne then SixteenRows else EightRows,capacity=if packing==PackingOne then Bytes 27104256 else capacity selected}) False
      outputWidth=if fixture selected==Full then 4896 else 320
      outputHeight=if fixture selected==Full then 2768 else 240
      requestBefore=wordsAt [(0,requestMode),(4,if packing==PackingOne then 1 else 0),(8,fromIntegral width),(12,fromIntegral height)
        ,(0x18,sourceAddress),(0x1c,sourceBytes),(0x20,0x4383cc00),(0x24,0),(0x34,0),(0x38,0),(0x3c,0),(0x40,0),(0x44,0),(0x48,0)
        ,(0x4c,completed),(0x50,0x110200)] (B.replicate 0x58 0xa5)
      setupRequest=wordsAt [(0x10,outputWidth),(0x14,outputHeight),(0x24,0),(0x28,if attachment selected==Refused then 3 else 0),(0x2c,0),(0x30,0)] requestBefore
      finalRequest=if attachment selected==Refused then setupRequest else wordsAt [(0x24,finalProduced selected),(0x30,0x87654321)] setupRequest
      initialState=wordsAt [(0,0),(4,0),(0x198,0),(0x19c,0)] (B.replicate 0x210 0xa5)
      finalState=replace 0x1a0 configurationBytes . replace 8 finalRequest $ wordsAt [(0,0),(4,0),(0x198,0x14c56f8),(0x19c,0)] initialState
      finalRecord=if attachment selected==Refused then recordReady else byteAt 0x35 1 . byteAt 0x36 0 $ recordReady
      finalConsumer=assemble [1,if route==DevelopJob4 then 8 else statusWord(completionStatus selected)]
      callbackCode=assemble $ load R1 0x2000100 ++ [0xe5910000,0xe2800001,0xe5810000] ++ load R1 0x14c56f8
        ++ [0xe5910028] ++ load R1 0x2000100 ++ [0xe5810004,0xe12fff1e]
      entry=assemble $ [0xe92d4010] ++ (if route==DevelopJob4
        then load R0 0x2002000 ++ call (Address 0xe8fee4) ++ load R1 0x2000100 ++ [0xe5810004]
        else load R0 recordBase ++ call (Address 0xd98904) ++ [word|attachment selected==Attached,word<-call (Address 0xd99570)] ++ call (Address 0xd989f8))
        ++ [0xe3a0005a,0xe8bd8010]
      producerBytes=B.take 304(B.drop (0xe8fee4-0xbf1a00) image5)
      signalBytes=B.take 32(B.drop (0xe8eb18-0xbf1a00) image5)
      code kind' address bytes=Block kind' (Address address) ReadExecute bytes [(Address address,B.length bytes)]
      deviceTargets=[(C.bodyStart body,target)|role/=Original,(selectedService,target,_)<-P.deviceServices(Just package),let(body,_)=C.jpegDeviceContract selectedService]
      mappedCalls=[Service (maybe address Address(lookup original deviceTargets)) inputs outputs memoryReads memoryWrites|Service address@(Address original) inputs outputs memoryReads memoryWrites<-calls]
      calls=[service 0x724b68 [(R0,9),(R1,0xffffffff)] [] []
        ,service 0x65a3cc [(R0,0x14c56f4)] [] [Relative R0(assemble[0x1e123456])]
        ,service 0x65a478 [] [] [],service 0x65a58c [(R0,0x14c5890)] [] []
        ,Service (Address 0x64e918) [(R0,0x48),(R1,0xd99570),(R2,0x14c588c)] [(R0,if attachment selected==Attached then 0 else 0x86540003)] []
          [Relative R2(assemble[0x76123456])|attachment selected==Attached]]
        ++ (if attachment selected==Attached then [service 0x42634 [(R0,0),(R1,0)] [] [],service 0x65a548 [] [] []
          ,service 0x65ab20 [] [] [Relative R0(assemble[0x10])],service 0x65ab50 [(R0,0x10)] [] []
          ,service 0x65abd8 [] [] [Relative R0(assemble[0x87654321])],service 0x65ac08 [(R0,0x87654321)] [] []
          ,service 0x65a9e4 [(R0,0x14c571c)] [] [Relative R0(let Address endpointWord=endpoint selected in assemble[endpointWord])]] else [])
        ++ [service 0x724558 [(R0,0x12345678),(R1,0x10)] [] [Absolute consumerAddress(assemble[1,0])]|route==DevelopJob4]
        ++ [service 0x7243a8 [(R0,0x10),(R2,0xffffffff)] [] []|route==DevelopJob4]
        ++ [service 0x65a478 [] [] []]
        ++ [service 0x64eb20 [(R0,0x76123456)] [] []|attachment selected==Attached]
        ++ [service 0x65a424 [(R0,0x1e123456)] [] [],service 0x724c00 [(R0,9)] [] []]
      originalBuilder=B.take 236(B.drop (0xd98904-0xbf1a00) image5)
      mutation selectedDefect=case selectedDefect of
        MissingMode->BuilderMutation (SourceOffset 0x40) (assemble[0xe320f000])
        MissingPacking->BuilderMutation (SourceOffset 0x84) (assemble[0xe320f000])
        MissingWidth->BuilderMutation (SourceOffset 0x9c) (assemble[0xe320f000])
        MissingHeight->BuilderMutation (SourceOffset 0x94) (assemble[0xe320f000])
        MissingSource->BuilderMutation (SourceOffset 0xa4) (assemble[0xe320f000])
        MissingCount->BuilderMutation (SourceOffset 0xac) (assemble[0xe320f000])
        MissingDestination->BuilderMutation (SourceOffset 0xb4) (assemble[0xe320f000])
        MissingClear->BuilderMutation (SourceOffset 0xc0) (assemble[0xe320f000])
        MissingCallback->BuilderMutation (SourceOffset 0xd8) (assemble[0xe3a02000])
      defectBytes=case defect of Nothing->B.empty;Just selectedDefect->let BuilderMutation (SourceOffset offset) value=mutation selectedDefect in replace offset value originalBuilder <> assemble(call(Address 0xd9882c)++[0xe8bd8800])
      publish block@(Block kind' address@(Address start) permissions bytes extents)
        | kind'==StockDriver && role/=Original = Block kind' address permissions (foldr (\(Address location,value) original->replace(fromIntegral(location-start)) value original) bytes
            [(location,if location==Address 0xd98908 && defect/=Nothing then assemble[0x110300] else value)
            |(location@(Address target),value)<-publications,target>=start,target<start+fromIntegral(B.length bytes)]) extents
        | otherwise=block
      runtimeFunctions=[(Address(E.symbolValue symbol),fromIntegral(E.symbolSize symbol))|symbol<-E.elfSymbols elf,E.symbolKind symbol==E.FunctionSymbol,E.symbolSize symbol>0
        ,E.symbolValue symbol>=baseWord,E.symbolValue symbol<baseWord+fromIntegral(B.length runtime)
        ,not(any(\(_,target)->E.symbolValue symbol<=target && target<E.symbolValue symbol+E.symbolSize symbol)deviceTargets)]
      operand=wordsAt [(8,0x12345678),(0x7f8,0x2001000)] (B.replicate 0x800 0)
      plane=wordsAt [(4,sourceAddress),(8,sourceBytes),(0xc,fromIntegral width),(0x10,fromIntegral height),(0x14,0),(0x1c,0x4383cc00),(0x24,0)] (B.replicate 0x34 0)
      finalPlane=wordsAt [(0x20,fromIntegral width*fromIntegral height*2),(0x24,fromIntegral width*2),(0x28,fromIntegral width*2),(0x2c,fromIntegral height),(0x30,0x100000)] plane
      blocks=map publish stock++[Block Runtime base ReadExecute runtime runtimeFunctions,Block Driver driverAddress ReadWrite initialState []
        ,Block Playback requestAddress ReadWrite initialPlayback [],Block RecordMemory (Address recordBase) ReadWrite recordBefore []
        ,Block Consumer consumerAddress ReadWrite (assemble[0,0]) [],Block Stack (Address 0x2200000) ReadWrite (B.replicate 0x4000 0) []
        ,code Caller 0x110000 entry,code FirstConsumer 0x110100 callbackCode,code SecondConsumer 0x110200 (assemble[0xe12fff1e])
        ,code Producer 0xe8fee4 producerBytes,code Signal 0xe8eb18 signalBytes,Block Exit exitAddress ReadExecute (assemble[0xe12fff1e]) []]
        ++ (if route==DevelopJob4 then [Block DevelopOperand (Address 0x2002000) ReadOnly operand [],Block DevelopPlane (Address 0x2001000) ReadWrite plane []] else [])
        ++ [code Defect 0x110300 defectBytes|defect/=Nothing]
      expected=[(Driver,finalState),(Playback,initialPlayback),(RecordMemory,finalRecord),(Consumer,finalConsumer)]
        ++ (if route==DevelopJob4 then [(DevelopOperand,operand),(DevelopPlane,finalPlane)] else [])
  ensure (hash producerBytes==Digest "47bb5cc27a1cd0062590dd9bf2019597b5537f698b807e3edfa8cb27980807eb") "develop job4 source body identity mismatch"
  ensure (hash signalBytes==Digest "3e54ac9af3dc1e6630f562c6a5897c97980635773395246431befb7bfc582841") "record notification source body identity mismatch"
  observed<-runNative runner output ident blocks mappedCalls expected (defect/=Nothing)
  json (output </> ident++"-identity.json") (object ["role" .= show role,"caller" .= show route,"defect" .= fmap show defect
    ,"runtime_sha256" .= hashText(hash runtime),"builder_source_sha256" .= hashText(hash(B.take 244(B.drop(0xd98904-0xbf1a00)image5)))
    ,"builder_control" .= case defect of
        Nothing -> Null
        Just selectedDefect -> let BuilderMutation (SourceOffset offset) _ = mutation selectedDefect in object
          ["definition" .= ("relocated original builder with one source instruction omitted or replaced" :: String)
          ,"entry" .= av(Address 0x110300),"source_instruction_offset" .= offset
          ,"executed_bytes" .= B.length defectBytes,"executed_sha256" .= hashText(hash defectBytes)]
    ,"conditional_wait_and_notification" .= (route==DevelopJob4),"runner_returned" .= True])
  pure(Observation (regionBytesOf observed(O.RegionId(regionName Driver))) (regionBytesOf observed(O.RegionId(regionName Playback)))
    (regionBytesOf observed(O.RegionId(regionName Consumer))) (regionBytesOf observed(O.RegionId(regionName RecordMemory))))

readPinned :: FilePath -> Digest -> IO B.ByteString
readPinned path expected = do
  bytes<-B.readFile path
  ensure (hash bytes==expected) ("pinned input mismatch: "++path)
  pure bytes

main :: IO ()
main = do
  arguments<-getArgs
  (runner,package,decode,output)<-case arguments of
    ["--runner",runner,"--package",package,"--decode-source",decode,"--output",output,"--native-jobs","1"]->pure(runner,package,decode,output)
    _->ioError(userError "qualify-jpeg --runner NATIVE_RUNNER --package LINKED_PACKAGE --decode-source SAVED_DECODE_DIRECTORY --output NEW_DIRECTORY --native-jobs 1")
  exists<-doesPathExist output
  ensure (not exists) "JPEG qualification output exists"
  createDirectory output
  executable<-getExecutablePath
  qualifierDigest<-sha256Hex <$> sha256File executable
  json (output </> "invocation.json") (object ["qualifier" .= executable,"qualifier_sha256" .= qualifierDigest,"arguments" .= arguments])
  runtime<-B.readFile(package </> "runtime.bin")
  elfBytes<-B.readFile(package </> "kernel.elf")
  manifest<-B.readFile(package </> "ram-push-manifest.json")
  metadata<-B.readFile(package </> "experiment.json")
  parsed<-required(P.parsePackage manifest metadata runtime elfBytes)
  ensure (P.qualificationScope parsed `elem` [P.JpegTransaction,P.JpegConsumers]) "exact6 or exact8 composed JPEG package required"
  runnerDigest<-N.runnerDigest runner
  let consumers=P.qualificationScope parsed==P.JpegConsumers
  ensure (not consumers || runnerDigest=="0b0e7a52908fb4232097a80ed4b8f0eb29069b414c4dd9a0550603c2159374e2") "consumer qualification requires pinned native runner"
  elf<-required(E.parseElf elfBytes)
  plain<-readPinned "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin" (Digest "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  image5<-readPinned "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin" (Digest "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
  image4<-readPinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (Digest "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  required(P.checkOriginalBodies parsed [let (base,image)=if address<0xbf1a00 then (0x61c400,image4) else if address<0x1100000 then (0xbf1a00,image5) else (0x1100000,plain)
    in B.take count (B.drop (fromIntegral(address-base)) image)|(address,count)<-P.originalBodies parsed])
  fixtures<-forM [("0-source.bin",21489,"7123dc28a6cee3a1d62baa0d463cf00649ae0cf6bcd8f4abac2ed4fc13cd67de")
    ,("1-source.bin",3515192,"40bc1f32a0091307e1d2e18b5855ce110373afa60d81878296ee62f7e2f05969")] $ \(file,count,digest)->do
      bytes<-readPinned (decode </> file) (Digest digest)
      ensure (B.length bytes==count) "consumed JPEG byte extent differs"
      pure(object ["path" .= (decode </> file),"bytes" .= count,"sha256" .= digest])
  (base,publications)<-case P.guestArtifacts parsed of (address,bytes):rest | bytes==runtime->pure(Address address,[(Address location,value)|(location,value)<-rest]);_->ioError(userError "runtime artifact missing")
  let driverFunctions=[(start,fromIntegral(end-start))|entry<-C.jpegEntries,let(start,end,_,_)=C.playbackContract entry]
        ++[(C.bodyStart body,fromIntegral(C.bodyEnd body-C.bodyStart body))|body<-C.jpegInternalBodies]
        ++[(0xd98904,244),(0xd989f8,32)]
      playbackFunctions=[(0x121fec8,88),(0x121ff20,88),(0x1220004,160),(0x12200c4,40),(0x12200ec,52),(0x122013c,252),(0x1220238,52),(0x1220294,60)]
      block kind' bytes imageBase start end functions=Block kind' (Address start) ReadExecute (B.take (fromIntegral(end-start))(B.drop(fromIntegral(start-imageBase))bytes)) [(Address address,count)|(address,count)<-functions]
      stock=[block StockDriver image5 0xbf1a00 0xd9882c 0xd99a5c driverFunctions,block StockPlayback plain 0x1100000 0x121fec8 0x12202d0 playbackFunctions]
  forM_ [("runtime.bin",runtime),("kernel.elf",elfBytes),("ram-push-manifest.json",manifest),("experiment.json",metadata)] $ \(file,bytes)->B.writeFile(output </> file)bytes
  let selectedScenarios=if consumers then [preview,full
        ,preview {name=CaseName "first-setup-failed-reentry",attachment=Refused,produced=Bytes 0,callbacks=Reenter}
        ,preview {name=CaseName "first-no-opposite-subscriber",callbacks=PrimaryOnly}
        ,preview {name=CaseName "first-no-primary-subscriber",callbacks=SecondaryOnly}
        ,preview {name=CaseName "first-irq-reentry",callbacks=Reenter}]
        else if P.deviceOwnership parsed==P.OwnedJpegDevice then [preview,full] else scenarios
  comparisons<-forM selectedScenarios $ \selected->do
    original<-execute runner output parsed elf base runtime publications stock selected Original
    owned<-execute runner output parsed elf base runtime publications stock selected Owned
    let CaseName label=name selected
    ensure (original==owned) (label++": stock and owned consumer observations differ")
    pure(object ["case" .= label,"consumer_observations_equal" .= True,"stock_receipt" .= (label++"-stock-result.json"),"owned_receipt" .= (label++"-owned-result.json")])
  controls<-forM (if consumers || P.deviceOwnership parsed/=P.OwnedJpegDevice then [preview,full] else []) $ \selected->do
    _<-execute runner output parsed elf base runtime publications stock selected OmittedIrq
    let CaseName label=name selected
    pure(object ["case" .= label,"returned_transaction_defect_rejected" .= True,"receipt" .= (label++"-omitted-irq-result.json")])
  firstControls<-if consumers then forM [(preview,OmittedFirst),(preview {name=CaseName "first-irq-reentry",callbacks=Reenter},ReenteredFirst)
    ,(preview {name=CaseName "first-no-opposite-subscriber",callbacks=PrimaryOnly},UngatedFirst)] $ \(selected,role)->do
      _<-execute runner output parsed elf base runtime publications stock selected role
      let CaseName label=name selected
      pure(object ["case" .= label,"defect" .= show role,"returned_consumer_defect_rejected" .= True,"receipt" .= (label++roleSuffix role++"-result.json")])
    else pure []
  builderComparisons<-if consumers then forM builderCases $ \selected@(BuilderCase (CaseName label) route _ _ _)->do
      original<-executeBuilder runner output parsed elf base runtime publications stock image5 selected Original Nothing
      owned<-executeBuilder runner output parsed elf base runtime publications stock image5 selected Owned Nothing
      ensure(original==owned)(label++": stock and owned builder consumer observations differ")
      pure(object ["case" .= label,"caller" .= show route,"consumer_observations_equal" .= True
        ,"stock_receipt" .= (label++"-stock-result.json"),"owned_receipt" .= (label++"-owned-result.json")])
    else pure []
  builderControls<-if consumers then forM [MissingMode,MissingPacking,MissingWidth,MissingHeight,MissingSource,MissingCount,MissingDestination,MissingClear,MissingCallback] $ \defect->do
      let selected=case defect of MissingPacking->builderMode8Case;MissingCallback->builderSetupFailureCase;_->builderMode7Case
          BuilderCase (CaseName label) _ _ _ _=selected
          CaseName defectLabel=builderDefectName defect
      _<-executeBuilder runner output parsed elf base runtime publications stock image5 selected Owned(Just defect)
      pure(object ["case" .= label,"defect" .= show defect,"returned_source_omission_rejected" .= True
        ,"receipt" .= (label++"-owned-"++defectLabel++"-result.json")])
    else pure []
  deviceCpu<-if not consumers && P.deviceOwnership parsed==P.OwnedJpegDevice
    then qualifyDeviceCpu runner output parsed elf base runtime image4
    else pure Null
  json (output </> "result.json") (object ["schema" .= (1::Int),"qualified" .= True,"scope" .= scope,"native_jobs" .= (1::Int),"runner_sha256" .= runnerDigest
    ,"source_rev" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3"::String)
    ,"runtime_sha256" .= hashText(hash runtime),"elf_sha256" .= hashText(hash elfBytes),"manifest_sha256" .= hashText(hash manifest),"metadata_sha256" .= hashText(hash metadata)
    ,"plain_sha256" .= hashText(hash plain),"image4_sha256" .= hashText(hash image4),"image5_sha256" .= hashText(hash image5)
    ,"consumed_source_fixtures" .= fixtures,"comparisons" .= comparisons,"controls" .= controls,"first_completion_controls" .= firstControls
    ,"builder_comparisons" .= builderComparisons,"builder_controls" .= builderControls,"device_cpu" .= deviceCpu
    ,"qualifier_sha256" .= qualifierDigest,"consumer_increment" .= consumers,"historical_device_matrix_repeated" .= (not consumers && P.deviceOwnership parsed==P.OwnedJpegDevice)
    ,"retained_entries" .= map av [Address 0x1220004,Address 0x121ff20,Address 0x1220238,Address 0xd989f8]
    ,"builder_failure_lifetime" .= ("Synchronous setup failure calls back before channel9/resource1e release; the retained companion releases them. Reacquisition without an intervening release reaches the retained infinite-deadline lock wait, outside the qualified returned completion reentry."::String)])
  putStrLn ("JPEG transaction passed "++show(length selectedScenarios+length builderComparisons)++" stock/owned consumer comparisons; device qualification remains separately scoped")

data DeviceCpuCase
  = ProgramFixture Fixture | ProgramMasks | ProgramWindow
  | StatusTransfer | InputTransfer | OutputTransfer | EndpointTransfer | StartControl
  deriving (Eq, Show)
data DeviceCpuRole = DeviceOriginal | DeviceOwned | DeviceOmittedProgram deriving (Eq, Show)
data DevicePort
  = ControlPort | StatusPort | ModePort | InputPort | OutputPort | ScalePort
  | DestinationPort | CapacityPort | EndpointPort | SourcePort | SourceBytesPort
  | FormatPort | WindowEnabledPort | HorizontalPort | VerticalPort
  deriving (Eq, Show)
newtype PortWord = PortWord Word32 deriving (Eq, Show)
data DeviceCpuObservation = DeviceCpuObservation B.ByteString B.ByteString deriving (Eq, Show)

portOffset :: DevicePort -> Int
portOffset selected = case selected of
  ControlPort->0;StatusPort->4;ModePort->8;InputPort->0x10;OutputPort->0x14;ScalePort->0xa0
  DestinationPort->0x100;CapacityPort->0x104;EndpointPort->0x108;SourcePort->0x10c;SourceBytesPort->0x110
  FormatPort->0x11c;WindowEnabledPort->0x120;HorizontalPort->0x124;VerticalPort->0x128

deviceCpuName :: DeviceCpuCase -> CaseName
deviceCpuName selected = CaseName $ case selected of
  ProgramFixture Preview->"device-program-preview";ProgramFixture Full->"device-program-full"
  ProgramMasks->"device-program-field-masks";ProgramWindow->"device-program-window-lanes"
  StatusTransfer->"device-status-read-ack";InputTransfer->"device-input-read-ack"
  OutputTransfer->"device-output-read-ack";EndpointTransfer->"device-endpoint-read";StartControl->"device-start-rmw"

deviceCpuCases :: [DeviceCpuCase]
deviceCpuCases = [ProgramFixture Preview,ProgramFixture Full,ProgramMasks,ProgramWindow
  ,StatusTransfer,InputTransfer,OutputTransfer,EndpointTransfer,StartControl]

registerImage :: B.ByteString
registerImage = assemble [0x5a000000+index|index<-[0..74]]

deviceCpuInput :: DeviceCpuCase -> B.ByteString
deviceCpuInput selected = case selected of
  ProgramFixture which->with (configuration (if which==Preview then preview else full) False)
  ProgramMasks->with (wordsAt ([(0x1c,0xffffffff),(0x20,0xfffffffe),(0x24,1),(0x28,0xfffe),(0x2c,0xffff),(0x30,0)]
    ++zip [0x34,0x38,0x3c] [0xfffffff5,0xfffffffe,0xffffffff]
    ++zip [0x40,0x44..0x6c] [3,2,0xffffffff,4,5,6,7,8,9,10,11,12]) (configuration preview False))
  ProgramWindow->with (wordsAt [(0x20,3),(0x24,0xffff),(0x28,0xffff),(0x2c,0xffff),(0x30,0xffff)] (configuration preview False))
  _->initialDriver preview
  where with bytes=replace 0x1a0 bytes (initialDriver preview)

deviceCpuWrites :: DeviceCpuCase -> [(DevicePort,PortWord)]
deviceCpuWrites selected = case selected of
  ProgramFixture Preview->program 0x46780c00 21489 153600 0xfd0020 0 0 0 0 0
  ProgramFixture Full->program 0x45fc9c00 3515192 27025920 0xfd0020 0 0 0 0 0
  ProgramMasks->program 0x46780c00 21489 153600 0xaa0110 3 0x25 0 0xfffe0001 0xffff
  ProgramWindow->program 0x46780c00 21489 153600 0xfd0020 0 0 1 0xffffffff 0xffffffff
  StatusTransfer->[(StatusPort,PortWord 0x13579bdf)]
  InputTransfer->[(InputPort,PortWord 0x13579bdf)]
  OutputTransfer->[(OutputPort,PortWord 0x13579bdf)]
  EndpointTransfer->[]
  StartControl->[(ControlPort,PortWord 0x5a000001)]
  where
    program sourceWord count capacityWord mode scaleWord format enabled horizontal vertical =
      [(ModePort,PortWord mode),(ScalePort,PortWord scaleWord),(DestinationPort,PortWord 0x4383cc00)
      ,(CapacityPort,PortWord capacityWord),(SourcePort,PortWord sourceWord),(SourceBytesPort,PortWord count)
      ,(FormatPort,PortWord format),(WindowEnabledPort,PortWord enabled)
      ,(HorizontalPort,PortWord horizontal),(VerticalPort,PortWord vertical)]

deviceCpuExpected :: DeviceCpuCase -> DeviceCpuObservation
deviceCpuExpected selected = DeviceCpuObservation driver registersImage
  where
    driver=case selected of
      StatusTransfer->readResult StatusPort;InputTransfer->readResult InputPort
      OutputTransfer->readResult OutputPort;EndpointTransfer->readResult EndpointPort
      _->deviceCpuInput selected
    readResult port=replace 0 (B.take 4(B.drop(portOffset port) registerImage)) (deviceCpuInput selected)
    registersImage=wordsAt [(portOffset port,value)|(port,PortWord value)<-deviceCpuWrites selected] registerImage

deviceCpuTarget :: P.Package -> DeviceCpuRole -> C.JpegDeviceService -> IO Address
deviceCpuTarget package role selected
  | role==DeviceOriginal = let (body,_)=C.jpegDeviceContract selected in pure(Address(C.bodyStart body))
  | otherwise = case [Address target|(service',target,_)<-P.deviceServices(Just package),service'==selected] of
      [target]->pure target
      _->ioError(userError "owned device service target missing")

deviceCpuCaller :: P.Package -> DeviceCpuRole -> DeviceCpuCase -> IO B.ByteString
deviceCpuCaller package role selected = do
  body<-case selected of
    ProgramFixture _->program
    ProgramMasks->program
    ProgramWindow->program
    StatusTransfer->transfer C.JpegStatusRead C.JpegStatusAck
    InputTransfer->transfer C.JpegInputRead C.JpegInputAck
    OutputTransfer->transfer C.JpegOutputRead C.JpegOutputAck
    EndpointTransfer->do
      target<-deviceCpuTarget package role C.JpegEndpointRead
      pure(load R0 0x014c56f0++call target)
    StartControl->call <$> deviceCpuTarget package role C.JpegStart
  pure(assemble([0xe92d4010]++body++[0xe3a0005a,0xe8bd8010]))
  where
    program=do
      target<-deviceCpuTarget package role C.JpegProgram
      pure(load R0 0x014c5890++call target)
    transfer reader acknowledger=do
      readTarget<-deviceCpuTarget package role reader
      ackTarget<-deviceCpuTarget package role acknowledger
      pure(load R0 0x014c56f0++call readTarget++load R0 0x13579bdf++call ackTarget)

deviceCpuScope :: String
deviceCpuScope = "Nonpolling original and exact linked owned device-service CPU execution against an identified register image. Configuration words, bitfield masks, window lanes, read results, write values and start read-modify-write are observed. The register image is ordinary RAM, not a device model or a ready-state fixture. No polling, reset, W1C, DMA, IRQ, timing, restart-command4 or status17/24 device-law claim."

executeDeviceCpu :: FilePath -> FilePath -> P.Package -> E.Elf -> Address -> B.ByteString -> B.ByteString -> DeviceCpuCase -> DeviceCpuRole -> IO DeviceCpuObservation
executeDeviceCpu runner output package elf base@(Address baseWord) runtime image4 selected role = do
  let CaseName label=deviceCpuName selected
      ident=label++case role of DeviceOriginal->"-stock";DeviceOwned->"-owned";DeviceOmittedProgram->"-omitted-program"
  Address programTarget<-deviceCpuTarget package DeviceOwned C.JpegProgram
  callerBytes<-deviceCpuCaller package role selected
  let executedRuntime=if role==DeviceOmittedProgram then replace(fromIntegral(programTarget-baseWord))(assemble[0xe12fff1e])runtime else runtime
      functions=[(Address(E.symbolValue symbol),fromIntegral(E.symbolSize symbol))|symbol<-E.elfSymbols elf,E.symbolKind symbol==E.FunctionSymbol,E.symbolSize symbol>0
        ,E.symbolValue symbol>=baseWord,E.symbolValue symbol<baseWord+fromIntegral(B.length runtime)]
      stockFunctions=[(Address(C.bodyStart body),fromIntegral(C.bodyEnd body-C.bodyStart body))|service'<-C.jpegDeviceServices,let(body,_)=C.jpegDeviceContract service']
      stockBytes=B.take(0x65ac40-0x65a3c0)(B.drop(0x65a3c0-0x61c400)image4)
      blocks=[Block Runtime base ReadExecute executedRuntime functions
        ,Block StockDevice(Address 0x65a3c0)(if role==DeviceOriginal then ReadExecute else ReadOnly)stockBytes
          (if role==DeviceOriginal then (Address 0x65a3c0,12):stockFunctions else [])
        ,Block Driver driverAddress ReadWrite(deviceCpuInput selected)[]
        ,Block DeviceRegisters(Address 0xfff63000)ReadWrite registerImage []
        ,Block Caller callerAddress ReadExecute callerBytes [(callerAddress,B.length callerBytes)]
        ,Block Stack(Address 0x02200000)ReadWrite(B.replicate 0x4000 0)[]
        ,Block Exit exitAddress ReadExecute(assemble[0xe12fff1e])[]]
      artifact(Block kind' _ _ bytes _)=object["id".=regionName kind',"path".=(ident++"-"++regionName kind'++".bin"),"size".=B.length bytes,"sha256".=hashText(hash bytes)]
      region(Block kind' address permissions bytes _)=object["id".=regionName kind',"address".=av address,"size".=B.length bytes,"permissions".=permission permissions
        ,"initial".=object["artifact".=regionName kind',"offset".=(0::Int),"length".=B.length bytes]]
      memory kind' bytes=object["region".=regionName kind',"offset".=(0::Int),"hex".=N.hexBytes bytes]
      preserved=[("r"++show index,0x10101010+fromIntegral index)|index<-[4::Int ..11]]++[("sp",0x02204000)]
      initialRegisters=[("r"++show index,0x10101010+fromIntegral index)|index<-[0::Int ..12]]++[("sp",0x02204000),("lr",0x00100000),("cpsr",0x10)]
      rawRegisters pairs=object[K.fromString register.=hex value|(register,value)<-pairs]
      DeviceCpuObservation expectedDriverBytes expectedRegisterBytes=deviceCpuExpected selected
      document=object["schema_version".=(1::Int),"id".=ident,"origin".=("synthetic"::String),"provenance".=deviceCpuScope
        ,"cpu".=object["architecture".=("arm"::String),"model".=("cortex-r5f"::String),"mode".=("arm"::String),"endian".=("little"::String)]
        ,"entry".=av callerAddress,"exit".=av exitAddress,"registers".=rawRegisters initialRegisters
        ,"artifacts".=map artifact blocks,"regions".=map region blocks
        ,"functions".=[object["address".=av address,"size".=size]|Block _ _ _ _ extents<-blocks,(address,size)<-extents]
        ,"stubs".=([]::[Value]),"limits".=object["instructions".=(100000::Int),"wall_ms".=(1000::Int)]
        ,"expected".=object["status".=("returned"::String),"registers".=rawRegisters([("r0",0x5a),("pc",0x00100000)]++preserved)
          ,"memory".=[memory Driver expectedDriverBytes,memory DeviceRegisters expectedRegisterBytes]
          ,"ignored_memory".=[object["region".=regionName Stack,"offset".=(0::Int),"length".=(0x4000::Int)]],"events".=([]::[Value])]
        ,"exclusions".=[deviceCpuScope]]
  forM_ blocks $ \(Block kind' _ _ bytes _)->B.writeFile(output </> ident++"-"++regionName kind'++".bin")bytes
  let path=output </> ident++".json"
  json path document
  (_,regions,_,_)<-N.loadCase path
  (status,report)<-N.runCase runner path
  json(output </> ident++"-result.json")report
  runnerExit<-either (ioError . userError . O.renderObservationError) pure (O.reportRunnerExit report)
  observed<-either (ioError . userError . O.renderObservationError) pure (O.observeNative runnerExit regions (field "runner" report))
  let returned=O.nativeStatus observed==O.Returned
  if role==DeviceOmittedProgram
    then ensure(status/=0 && field "status" report==String "failed" && returned)(ident++": omitted programming must return with a register-image mismatch")
    else ensure(status==0 && field "status" report==String "passed" && returned)(ident++": device CPU effects failed")
  json(output </> ident++"-identity.json")(object["role".=show role,"scope".=deviceCpuScope
    ,"package_runtime_sha256".=hashText(hash runtime),"executed_runtime_sha256".=hashText(hash executedRuntime)
    ,"register_image_sha256".=hashText(hash registerImage),"program_entry".=av(Address programTarget)
    ,"mutation".=(if role==DeviceOmittedProgram then "program entry replaced by BX LR" else "none"::String)
    ,"runner_returned".=returned])
  pure(DeviceCpuObservation(regionBytesOf observed(O.RegionId(regionName Driver)))(regionBytesOf observed(O.RegionId(regionName DeviceRegisters))))

qualifyDeviceCpu :: FilePath -> FilePath -> P.Package -> E.Elf -> Address -> B.ByteString -> B.ByteString -> IO Value
qualifyDeviceCpu runner output package elf base runtime image4 = do
  cases<-forM deviceCpuCases $ \selected->do
    original<-executeDeviceCpu runner output package elf base runtime image4 selected DeviceOriginal
    owned<-executeDeviceCpu runner output package elf base runtime image4 selected DeviceOwned
    let CaseName label=deviceCpuName selected
    ensure(original==owned)(label++": device CPU effects differ")
    pure(object["case".=label,"register_image_effects_equal".=True
      ,"stock_receipt".=(label++"-stock-result.json"),"owned_receipt".=(label++"-owned-result.json")])
  _<-executeDeviceCpu runner output package elf base runtime image4(ProgramFixture Preview)DeviceOmittedProgram
  pure(object["scope".=deviceCpuScope,"cases".=cases,"omitted_program_control_returned_and_rejected".=True
    ,"control_receipt".=("device-program-preview-omitted-program-result.json"::String)])
