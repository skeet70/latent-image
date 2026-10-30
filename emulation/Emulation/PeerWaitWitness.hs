{-# LANGUAGE OverloadedStrings #-}
module Emulation.PeerWaitWitness
  ( ControlReceipt, parseControlReceipt, validateControlReceipt, controlReceiptValue
  , parseControlContract, controlContractValue
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON, Object, Result (..), Value (..), fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Bits ((.&.))
import Data.List (sort)
import Data.Word (Word32, Word64, Word8)
import Emulation.FirstStageWitness (encodeLeWord)
import Emulation.PeerStageWitness (Argument (..), Core (..), Cpsr (..), GuestAddress (..), Instructions (..), PeerState (..), Registers (..), SavedFrame (..), WitnessResult (..))
import qualified Emulation.PeerStageWitness as P
import qualified Emulation.WatchObservation as W
import FujiAF.JSON (decodeStrictIntegers)
import FujiAF.PeerWaitControl (PeerControl (..), controlName, parseControlName)

newtype WatchCursor = WatchCursor Int deriving (Eq, Ord, Show)
data Label = Entry | Tail | WaitLoaded | Gate | Acknowledged | Clear | Failure | Handoff | Loader
  deriving (Eq, Show, Enum, Bounded)
data ControlStop = ControlStop
  { label :: Label, site :: GuestAddress, cpu :: Core, count :: Instructions
  , registers :: Registers, sp :: GuestAddress, lr :: GuestAddress, cpsr :: Cpsr
  , state :: PeerState, frame :: Maybe SavedFrame, cursor :: WatchCursor
  } deriving (Eq, Show)
-- Instruction counts measure CPU entry, not retirement.
data CoreCounts = CoreCounts Word64 Word64 deriving (Eq, Show)
data Microstep = Microstep CoreCounts CoreCounts Word32 deriving (Eq, Show)
data GateBindings = GateBindings Instructions B.ByteString deriving (Eq, Show)
data ClearInstruction = OriginalClearInstruction deriving (Eq, Show)
data PrimaryContext = PrimaryContext GuestAddress Registers GuestAddress GuestAddress Cpsr Instructions deriving (Eq, Show)
data ClearReadback = ClearReadback
  { readbackCount :: Instructions, primaryAddress :: GuestAddress, readbackState :: PeerState
  , readbackCursor :: WatchCursor, microsteps :: [Microstep], primaryCpsr :: Cpsr, primaryInstruction :: ClearInstruction
  } deriving (Eq, Show)
data ControlReceipt = ControlReceipt
  { control :: PeerControl, result :: WitnessResult, problem :: Maybe String, machineFailure :: Maybe String
  , instructions :: Instructions, identities :: (Word32, Word32), stops :: [ControlStop]
  , clearReadback :: Maybe ClearReadback, memoryWatch :: W.WatchObservation, bindings :: Maybe GateBindings
  , finalPrimary :: Maybe PrimaryContext
  } deriving (Eq, Show)

name :: Label -> String
name item = case item of
  Entry -> "record-2-entry"
  Tail -> "record-2-rendezvous-tail"
  WaitLoaded -> "peer-wait-loaded"
  Gate -> "primary-gate"
  Acknowledged -> "peer-acknowledged"
  Clear -> "primary-clear"
  Failure -> "control-failure"
  Handoff -> "record-2-handoff"
  Loader -> "loader-b-entry"

fields :: [String] -> Value -> Either String Object
fields names (Object row) = do
  unless (sort names == sort (map K.toString (KM.keys row))) (Left "control fields differ from schema 2")
  pure row
fields _ _ = Left "control value is not an object"
field :: Object -> String -> Either String Value
field row key = maybe (Left ("missing control field: " ++ key)) Right (KM.lookup (K.fromString key) row)
json :: FromJSON a => Value -> Either String a
json value = case fromJSON value of
  Error message -> Left message
  Success parsed -> Right parsed
get :: FromJSON a => Object -> String -> Either String a
get row key = field row key >>= json
nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value
address :: Object -> String -> Either String GuestAddress
address row key = GuestAddress . fromIntegral <$> (get row key :: Either String Word32)
parseCursor :: Object -> String -> Either String WatchCursor
parseCursor row key = do
  value <- get row key :: Either String Word32
  unless (value <= 16384) (Left "watch cursor exceeds collector limit")
  pure (WatchCursor (fromIntegral value))
parseCounts :: Value -> Either String CoreCounts
parseCounts value = do
  values <- json value :: Either String [Word64]
  case values of
    [first,second] | first <= 10000000 && second <= 10000000 -> Right (CoreCounts first second)
    _ -> Left "microstep requires bounded two-core counts"
parseMicrostep :: Value -> Either String Microstep
parseMicrostep value = do
  row <- fields ["before","after","cell0"] value
  Microstep <$> (field row "before" >>= parseCounts) <*> (field row "after" >>= parseCounts) <*> get row "cell0"
parseReadback :: Value -> Either String ClearReadback
parseReadback value = do
  row <- fields ["instructions","primary_address","primary_cpsr","primary_instruction","state","watch_records","microsteps"] value
  steps <- field row "microsteps" >>= json >>= mapM parseMicrostep
  unless (not (null steps) && length steps <= 2048) (Left "clear microstep bound exceeded")
  instruction <- get row "primary_instruction" :: Either String [Word8]
  require (instruction == [0,32,131,229]) "clear instruction differs from original STR"
  ClearReadback <$> (Instructions <$> get row "instructions") <*> address row "primary_address"
    <*> (field row "state" >>= P.parseState) <*> parseCursor row "watch_records" <*> pure steps
    <*> (Cpsr <$> get row "primary_cpsr") <*> pure OriginalClearInstruction
parseStop :: Value -> Either String ControlStop
parseStop value = do
  row <- fields ["label","address","core","instructions","registers","sp","lr","cpsr","state","frame","watch_records"] value
  text <- get row "label"
  item <- case [item | item <- [minBound .. maxBound], name item == text] of
    [item] -> Right item
    _ -> Left "unknown control stop"
  coreNumber <- get row "core" :: Either String Word8
  core <- case coreNumber of
    0 -> Right Core0
    1 -> Right Core1
    _ -> Left "control core must be 0 or 1"
  ControlStop item <$> address row "address" <*> pure core <*> (Instructions <$> get row "instructions")
    <*> (field row "registers" >>= P.parseRegisters) <*> address row "sp" <*> address row "lr"
    <*> (Cpsr <$> get row "cpsr") <*> (field row "state" >>= P.parseState)
    <*> (field row "frame" >>= nullable P.parseFrame) <*> parseCursor row "watch_records"

parseControlContract :: B.ByteString -> Either String PeerControl
parseControlContract bytes = do
  require (B.length bytes <= P.maxReceiptBytes) "control contract exceeds 16 MiB"
  value <- decodeStrictIntegers bytes
  row <- fields ["schema","control","budget"] value
  schema <- get row "schema" :: Either String Word32
  budget <- get row "budget" :: Either String Word64
  unless (schema == 2 && budget == 10000000) (Left "unsupported control contract")
  get row "control" >>= parseControlName
controlContractValue :: PeerControl -> Value
controlContractValue selected = object ["schema" .= (2 :: Int), "control" .= controlName selected, "budget" .= (10000000 :: Int)]
parseControlReceipt :: B.ByteString -> Either String ControlReceipt
parseControlReceipt bytes = do
  unless (B.length bytes <= P.maxReceiptBytes) (Left "control receipt exceeds 16 MiB")
  value <- decodeStrictIntegers bytes
  row <- fields ["schema","control","result","error","machine_failure","instructions","cp14_identities","stops","clear_readback","memory_watch","bindings","final_primary"] value
  schema <- get row "schema" :: Either String Word32
  unless (schema == 2) (Left "unsupported control receipt schema")
  selected <- get row "control" >>= parseControlName
  resultText <- get row "result" :: Either String String
  status <- case resultText of
    "passed" -> Right Passed
    "failed" -> Right Failed
    _ -> Left "unknown control result"
  identityWords <- get row "cp14_identities" :: Either String [Word32]
  pair <- case identityWords of
    [first,second] -> Right (first,second)
    _ -> Left "control requires two CP14 identities"
  snapshots <- field row "stops" >>= json >>= mapM parseStop
  unless (length snapshots <= 12) (Left "control stop limit exceeded")
  limit <- W.watchLimit 16384
  ControlReceipt selected status <$> (field row "error" >>= nullable json)
    <*> (field row "machine_failure" >>= nullable json) <*> (Instructions <$> get row "instructions")
    <*> pure pair <*> pure snapshots <*> (field row "clear_readback" >>= nullable parseReadback)
    <*> (field row "memory_watch" >>= W.parseWatchObservation limit)
    <*> (field row "bindings" >>= nullable (parseBindings selected))
    <*> (field row "final_primary" >>= nullable parsePrimaryContext)

parsePrimaryContext :: Value -> Either String PrimaryContext
parsePrimaryContext value = do
  row <- fields ["address","registers","sp","lr","cpsr","instructions"] value
  PrimaryContext <$> address row "address" <*> (field row "registers" >>= P.parseRegisters)
    <*> address row "sp" <*> address row "lr" <*> (Cpsr <$> get row "cpsr")
    <*> (Instructions <$> get row "instructions")

primaryContextValue :: PrimaryContext -> Value
primaryContextValue (PrimaryContext pc regs stack link (Cpsr flags) time) = object
  ["address" .= addressWord pc,"registers" .= P.registerWords regs,"sp" .= addressWord stack
  ,"lr" .= addressWord link,"cpsr" .= flags,"instructions" .= countWord time]

parseBindings :: PeerControl -> Value -> Either String GateBindings
parseBindings selected value = do
  row <- fields ["core","instructions","decoder_choices","low_ram","atcm","btcm","stack_address","stack","code"] value
  core <- get row "core" :: Either String Word32
  choices <- get row "decoder_choices" :: Either String [Word32]
  atcm <- get row "atcm" :: Either String Word32
  btcm <- get row "btcm" :: Either String Word32
  stackAddress <- address row "stack_address"
  stack <- B.pack <$> (get row "stack" :: Either String [Word8])
  require (core == 0 && choices == [1] && atcm == 0x70000011 && btcm == 0 && stackAddress == GuestAddress 0x700007d8 && B.length stack == 40) "gate decoder/TCM/stack binding differs"
  low <- field row "low_ram" >>= fields ["address","physical","size","permissions"]
  lowAddress <- address low "address"
  lowPhysical <- address low "physical"
  lowSize <- get low "size" :: Either String Word32
  permissions <- get low "permissions" :: Either String Word32
  require (lowAddress == GuestAddress 0 && lowPhysical == GuestAddress 0 && lowSize == 16384 && permissions == 7) "gate active low RAM binding differs"
  code <- get row "code" :: Either String [Value]
  require (length code == 2) "gate code binding extent count differs"
  forM_ (zip code (codeExtents selected)) $ \(entry,(expectedAddress,expectedBytes)) -> do
    extent <- fields ["address","virtual","physical"] entry
    actualAddress <- address extent "address"
    virtual <- B.pack <$> (get extent "virtual" :: Either String [Word8])
    physical <- B.pack <$> (get extent "physical" :: Either String [Word8])
    require (actualAddress == expectedAddress && virtual == expectedBytes && physical == expectedBytes) "gate source/control code or active alias bytes differ"
  GateBindings <$> (Instructions <$> get row "instructions") <*> pure stack

codeExtents :: PeerControl -> [(GuestAddress,B.ByteString)]
codeExtents selected =
  [ (GuestAddress 0x100, if selected == HeldSentinel then B.replicate 104 0xff else wordsBytes
      [3912060943,3775856640,3808563200,3812896770,3808477739,3851624452,3813801984,167772170
      ,3780247564,436207621,3818922084,3810586624,3797032961,452984828,3796963329,452984820
      ,3777556480,3904720911,3942645758,3808497664,3796963329,452984829,3777556480,3904720911
      ,3812896770,3925870499])
  , (GuestAddress 0x1fe0,wordsBytes [0xe3003000,0xe344300a,0xe3a02000,0xe58320c4,0xe3003000
      ,if selected == HeldSentinel then 0xeafffffe else 0xeafff841,0xe5832000,0xf57ff04f])
  ]
  where wordsBytes = B.concat . map encodeLeWord

bindingsValue :: PeerControl -> GateBindings -> Value
bindingsValue selected (GateBindings time stack) = object
  [ "core" .= (0 :: Int), "instructions" .= countWord time, "decoder_choices" .= ([1] :: [Int])
  , "low_ram" .= object ["address" .= (0 :: Int),"physical" .= (0 :: Int),"size" .= (16384 :: Int),"permissions" .= (7 :: Int)]
  , "atcm" .= (0x70000011 :: Word32), "btcm" .= (0 :: Int), "stack_address" .= (0x700007d8 :: Word32)
  , "stack" .= B.unpack stack
  , "code" .= [object ["address" .= addressWord location,"virtual" .= B.unpack bytes,"physical" .= B.unpack bytes] | (location,bytes) <- codeExtents selected]
  ]

addressWord :: GuestAddress -> Word64
addressWord (GuestAddress value) = value
countWord :: Instructions -> Word64
countWord (Instructions value) = value
cursorWord :: WatchCursor -> Int
cursorWord (WatchCursor value) = value
stopValue :: ControlStop -> Value
stopValue stop = let Cpsr flags = cpsr stop in object
  [ "label" .= name (label stop), "address" .= addressWord (site stop), "core" .= (if cpu stop == Core0 then 0 else 1 :: Int)
  , "instructions" .= countWord (count stop), "registers" .= P.registerWords (registers stop)
  , "sp" .= addressWord (sp stop), "lr" .= addressWord (lr stop), "cpsr" .= flags, "state" .= P.stateValue (state stop)
  , "frame" .= fmap (B.unpack . B.concat . map encodeLeWord . P.frameWords) (frame stop), "watch_records" .= cursorWord (cursor stop)
  ]
countsValue :: CoreCounts -> [Word64]
countsValue (CoreCounts first second) = [first,second]
readbackValue :: ClearReadback -> Value
readbackValue observation = object
  [ "instructions" .= countWord (readbackCount observation), "primary_address" .= addressWord (primaryAddress observation)
  , "state" .= P.stateValue (readbackState observation), "watch_records" .= cursorWord (readbackCursor observation)
  , "microsteps" .= [object ["before" .= countsValue before,"after" .= countsValue after,"cell0" .= scalar] | Microstep before after scalar <- microsteps observation]
  , "primary_cpsr" .= (let Cpsr flags = primaryCpsr observation in flags)
  , "primary_instruction" .= (case primaryInstruction observation of OriginalClearInstruction -> [0,32,131,229] :: [Word8])
  ]
controlReceiptValue :: ControlReceipt -> Value
controlReceiptValue receipt = let watch = memoryWatch receipt; (first,second) = identities receipt in object
  [ "schema" .= (2 :: Int), "control" .= controlName (control receipt)
  , "result" .= (if result receipt == Passed then "passed" else "failed" :: String)
  , "error" .= problem receipt, "machine_failure" .= machineFailure receipt, "instructions" .= countWord (instructions receipt)
  , "cp14_identities" .= [first,second], "stops" .= map stopValue (stops receipt), "clear_readback" .= fmap readbackValue (clearReadback receipt)
  , "bindings" .= fmap (bindingsValue (control receipt)) (bindings receipt)
  , "final_primary" .= fmap primaryContextValue (finalPrimary receipt)
  , "memory_watch" .= object ["enabled" .= W.watchEnabled watch,"ranges" .= [[low,high] | range <- W.watchRanges watch, let (low,high) = W.watchRangeBounds range]
      ,"records" .= map P.recordValue (W.watchRecords watch),"violation" .= W.watchViolation watch]
  ]

one :: Label -> [ControlStop] -> Either String ControlStop
one wanted snapshots = case filter ((== wanted) . label) snapshots of
  [stop] -> Right stop
  _ -> Left ("missing or repeated control stop: " ++ name wanted)
require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)
nondecreasing :: Ord a => [a] -> Bool
nondecreasing values = and (zipWith (<=) values (drop 1 values))
increasing :: Ord a => [a] -> Bool
increasing values = and (zipWith (<) values (drop 1 values))
quiet :: Word32 -> PeerState -> Bool
quiet first snapshot = snapshot == PeerState first 0 0 (GuestAddress 0) (Argument 0)
siteCore :: Label -> (GuestAddress, Core)
siteCore item = case item of
  Entry -> (GuestAddress 0x11800,Core1)
  Tail -> (GuestAddress 0x11f40,Core1)
  WaitLoaded -> (GuestAddress 0x11f78,Core1)
  Gate -> (GuestAddress 0x1ff4,Core0)
  Acknowledged -> (GuestAddress 0x14c,Core0)
  Clear -> (GuestAddress 0x1ff8,Core0)
  Failure -> (GuestAddress 0x148,Core0)
  Handoff -> (GuestAddress 0x11850,Core1)
  Loader -> (GuestAddress 0xd0020,Core1)

validateControlReceipt :: PeerControl -> Argument -> ControlReceipt -> Either String ()
validateControlReceipt selected expected receipt = do
  require (control receipt == selected) "control selection differs"
  require (expected == Argument 0x69cdf8) "control argument differs from pinned source"
  require (result receipt == Passed && problem receipt == Nothing && machineFailure receipt == Nothing) "control witness failed"
  require (identities receipt == (0x4003,0x5003)) "control CP14 identities differ"
  require (instructions receipt > Instructions 0 && instructions receipt <= Instructions 10000000) "control aggregate budget exceeded"
  let snapshots = stops receipt
      waits = filter ((== WaitLoaded) . label) snapshots
      expectedLabels = [Entry,Tail,Gate] ++ replicate 4 WaitLoaded ++ if selected == HeldSentinel then [] else [Acknowledged,Clear,Handoff,Loader]
  require (sort (map (fromEnum . label) snapshots) == sort (map fromEnum expectedLabels)) "control stop multiplicities differ"
  require (increasing (map count snapshots) && all ((<= instructions receipt) . count) snapshots) "control stop chronology differs"
  require (nondecreasing (map cursor snapshots)) "control cursors regress"
  forM_ snapshots $ \stop -> do
    require ((site stop,cpu stop) == siteCore (label stop)) "control stop has wrong core or site"
    require (label stop == Tail || frame stop == Nothing) "saved frame outside tail"
  entry <- one Entry snapshots
  tailStop <- one Tail snapshots
  gate <- one Gate snapshots
  case bindings receipt of
    Just (GateBindings time _) -> require (time == count gate) "gate binding count differs"
    Nothing -> Left "gate runtime bindings missing"
  require (count entry < count tailStop && all ((> count tailStop) . count) waits) "peer entry/tail/wait order differs"
  require (count gate <= Instructions 4000000) "prefix-to-gate bound exceeded"
  saved <- maybe (Left "tail saved frame missing") Right (frame tailStop)
  let GuestAddress tailSp = sp tailStop
  require (tailSp + 64 <= 0xffffffff && fromIntegral (r11 (registers tailStop)) == tailSp + 60 && r6 (registers tailStop) == 1 && savedLr saved == 0x1183c) "tail frame relationship differs"
  require (state tailStop == PeerState 0x22b 0x22b 0 (GuestAddress 0) (Argument 0)) "tail sentinel/publication readback differs"
  let Cpsr gateFlags = cpsr gate
  require (sp gate == GuestAddress 0x700007f0 && gateFlags .&. 0x010002ff == 0xd3 && r2 (registers gate) == 0 && r3 (registers gate) == 0) "primary gate live context differs"
  require (cell0 (state gate) == 0x22b && cell1 (state gate) `elem` [0,0x22b] && completion (state gate) == 0 && target (state gate) == GuestAddress 0 && argument (state gate) == Argument 0) "gate state differs"
  forM_ waits $ \stop -> require (quiet 0x22b (state stop) && r1 (registers stop) == 0x40020000 && r2 (registers stop) == 0x22b && r3 (registers stop) == 0x22b) "loaded wait evidence differs"
  validateWatch receipt
  validateAttempts selected expected entry tailStop gate waits receipt
  case selected of
    HeldSentinel -> do
      require (clearReadback receipt == Nothing) "held control has clear readback"
      require (finalPrimary receipt == Just (PrimaryContext (GuestAddress 0x1ff4) (registers gate) (sp gate) (lr gate) (cpsr gate) (instructions receipt))) "held primary context changed"
      case reverse snapshots of
        final:_ -> require (label final == WaitLoaded && count final == instructions receipt && count gate < count final && count final <= plus (count gate) 4000000) "held completion or gate-to-load bound differs"
        [] -> Left "held stops missing"
    DelayedRelease -> do
      require (finalPrimary receipt == Nothing) "delayed control has held final context"
      ack <- one Acknowledged snapshots
      preclear <- one Clear snapshots
      handoff <- one Handoff snapshots
      loader <- one Loader snapshots
      require (count gate < count ack && count tailStop < count ack && count ack < count preclear && all ((< count preclear) . count) waits && count preclear < count handoff && count handoff < count loader && count loader == instructions receipt) "delayed partial stop order differs"
      require (quiet 0x22b (state ack) && quiet 0x22b (state preclear)) "delayed acknowledgment/preclear readback differs"
      require (registers preclear == (registers gate) {r3 = 0x40020000} && sp preclear == sp gate && lr preclear == lr gate && cpsr preclear == cpsr gate) "primary live context was not restored"
      require (count preclear <= plus (count gate) 5100000) "gate-to-clear bound exceeded"
      observation <- maybe (Left "committed clear readback missing") Right (clearReadback receipt)
      validateReadback preclear handoff loader observation receipt
      P.validateReceipt expected (P.Receipt Passed Nothing Nothing (instructions receipt) (identities receipt)
        (map natural [entry,tailStop,handoff,loader]) (memoryWatch receipt))
  where
    plus (Instructions value) delta = Instructions (value + delta)
    natural stop = P.Stop (case label stop of Entry -> P.Record2Entry; Tail -> P.RendezvousTail; Handoff -> P.Record2Handoff; _ -> P.LoaderBEntry)
      (site stop) (cpu stop) (count stop) (registers stop) (sp stop) (lr stop) (cpsr stop) (state stop) (frame stop)

expectedRanges :: [(Word64,Word64)]
expectedRanges = [(0x20000,0x20008),(0xa00c4,0xa00c8),(0x14c80,0x14c88)]
validateWatch :: ControlReceipt -> Either String ()
validateWatch receipt = do
  let watch = memoryWatch receipt
      rows = W.watchRecords watch
      cpus = [row | row@W.CpuRecord {} <- rows]
  require (W.watchEnabled watch && sort (map W.watchRangeBounds (W.watchRanges watch)) == sort expectedRanges && W.watchViolation watch == Nothing) "control watch configuration/violation differs"
  require (length rows <= 12288) "successful control watch row bound exceeded"
  require (nondecreasing (map W.cpuInstructions cpus) && nondecreasing (map W.cpuTick cpus) && all ((<= instructions receipt) . W.cpuInstructions) cpus) "control CPU chronology differs"
  forM_ rows $ \row -> do
    let (physical,width) = case row of
          W.CpuRecord {} -> (W.cpuPhysical row,W.cpuBytes row)
          W.TransferRecord {} -> (W.transferPhysical row,W.transferBytes row)
        low = fromIntegral (W.watchAddressValue physical) :: Word64
        high = low + fromIntegral (W.watchBytesValue width)
    require (any (\(start,end) -> low < end && start < high) expectedRanges) "control watch row outside armed ranges"
    case row of
      W.TransferRecord {W.transferAccess = W.HostWrite} -> Left "host write in control receipt"
      W.CpuRecord {W.cpuAccess = W.CpuFetch} -> Left "instruction fetch in handshake/publication memory"
      _ -> pure ()
  forM_ (stops receipt) $ \stop -> validateCursor rows (count stop) (cursor stop)
  forM_ (zip (WatchCursor 0 : map cursor (stops receipt)) (stops receipt)) $ \(previous,stop) ->
    validateObservationReads rows previous (cursor stop)

validateObservationReads :: [W.WatchRecord] -> WatchCursor -> WatchCursor -> Either String ()
validateObservationReads rows (WatchCursor previous) (WatchCursor current) =
  forM_ [0x20000,0x20004,0xa00c4,0x14c80,0x14c84] $ \physical ->
    require (any (\row -> case row of
      W.TransferRecord W.HostRead guest actual width -> guest == W.watchAddress physical && actual == W.watchAddress physical && W.watchBytesValue width == 4
      _ -> False) (take (current - previous) (drop previous rows))) "snapshot lacks physical observation reads"

validateCursor :: [W.WatchRecord] -> Instructions -> WatchCursor -> Either String ()
validateCursor rows time (WatchCursor offset) = do
  require (offset <= length rows) "watch cursor exceeds receipt rows"
  require (all (\row -> case row of W.CpuRecord {} -> W.cpuInstructions row <= time; _ -> True) (take offset rows)) "cursor includes future CPU attempt"
  require (all (\row -> case row of W.CpuRecord {} -> W.cpuInstructions row > time; _ -> True) (drop offset rows)) "cursor omits earlier CPU attempt"

validateAttempts :: PeerControl -> Argument -> ControlStop -> ControlStop -> ControlStop -> [ControlStop] -> ControlReceipt -> Either String ()
validateAttempts selected (Argument expected) entry tailStop gate waits receipt = do
  let rows = W.watchRecords (memoryWatch receipt)
      indexed = zip [0..] rows
      writers = [(index,row) | (index,row@W.CpuRecord {W.cpuAccess = W.CpuWrite}) <- indexed]
      specifications = [(W.Core0,0x1e64,0x20000,0x22b),(W.Core0,0x1e68,0x20004,0x22b),(W.Core1,0x11818,0x14c80,0),(W.Core1,0x11818,0x14c84,0),(W.Core0,0x1fec,0xa00c4,0),(W.Core1,0x11f4c,0x20004,0)]
        ++ if selected == HeldSentinel then [] else [(W.Core0,0x1ff8,0x20000,0),(W.Core1,0x11f98,0x14c80,0xd0020),(W.Core1,0x11fb0,0x14c84,fromIntegral expected)]
      beforeWord pc
        | pc `elem` [0x1e64,0x1e68,0x11f4c,0x1ff8] = 0x22b
        | pc == 0x11818 = 0xffffffff
        | otherwise = 0
      matches (core,pc,physical,value) row = W.cpuCore row == core && W.cpuPc row == W.watchAddress pc && W.cpuPhysical row == W.watchAddress physical && W.cpuAddress row == W.watchAddress (if physical == 0x14c80 || physical == 0x14c84 then physical else physical + 0x40000000) && W.watchBytesValue (W.cpuBytes row) == 4 && W.cpuValue row == Just value && W.cpuByteOrder row == W.LittleEndian && W.cpuBefore row == Just (encodeLeWord (beforeWord pc))
  require (length writers == length specifications && all (\(_,row) -> any (\spec -> matches spec row) specifications) writers) "unexplained or missing CPU write attempt"
  forM_ specifications $ \spec -> require (length [() | (_,row) <- writers, matches spec row] == 1) "required source-qualified writer missing or repeated"
  let findWriter pc = case [(index,row) | (index,row) <- writers, W.cpuPc row == W.watchAddress pc] of
        [item] -> Right item
        _ -> Left "source writer missing"
  (sentinelIndex,sentinel0) <- findWriter 0x1e64
  (_,sentinel1) <- findWriter 0x1e68
  (_,completionWrite) <- findWriter 0x1fec
  (peerIndex,peerClear) <- findWriter 0x11f4c
  require (W.cpuInstructions sentinel0 < W.cpuInstructions sentinel1 && W.cpuInstructions sentinel1 < count entry && W.cpuInstructions completionWrite < count gate && count tailStop < W.cpuInstructions peerClear) "sentinel/release/peer-clear order differs"
  forM_ [row | (_,row) <- writers, W.cpuPc row == W.watchAddress 0x11818] $ \row ->
    require (count entry < W.cpuInstructions row && W.cpuInstructions row < count tailStop) "publication initialization outside peer entry/tail interval"
  require (sentinelIndex < peerIndex && peerIndex < cursorWord (cursor (head waits))) "peer clear not observed before held loads"
  forM_ (drop sentinelIndex rows) $ \row -> case row of
    W.TransferRecord {W.transferAccess = W.ModelBusWrite} -> Left "unexplained model-bus write after sentinel stage"
    _ -> pure ()
  let waitReads = [(index,row) | (index,row@W.CpuRecord {W.cpuAccess = W.CpuRead}) <- indexed, W.cpuPc row == W.watchAddress 0x11f74]
      primaryPolls = [row | row@W.CpuRecord {W.cpuAccess = W.CpuRead} <- rows, W.cpuPc row == W.watchAddress 0x114]
  require (length waitReads >= 4 && length waitReads <= 128 && length primaryPolls <= 8192) "original wait CPU read or veneer poll bound differs"
  let committed = readbackCount <$> clearReadback receipt
      matchesRead row core guest physical value = W.cpuCore row == core && W.cpuAddress row == W.watchAddress guest
        && W.cpuPhysical row == W.watchAddress physical && W.watchBytesValue (W.cpuBytes row) == 4
        && W.cpuValue row == Just value && W.cpuBefore row == Just (encodeLeWord (fromIntegral value)) && W.cpuByteOrder row == W.LittleEndian
      afterClear row = maybe False (W.cpuInstructions row >) committed
  forM_ [row | row@W.CpuRecord {W.cpuAccess = W.CpuRead} <- rows] $ \row -> do
    let valid = case W.watchAddressValue (W.cpuPc row) of
          0x11f74 -> matchesRead row W.Core1 0x40020000 0x20000 (if afterClear row then 0 else 0x22b)
          0x114 -> selected == DelayedRelease && W.cpuInstructions row > count gate
            && W.cpuValue row `elem` [Just 0,Just 0x22b] && maybe False (matchesRead row W.Core0 0x40020004 0x20004) (W.cpuValue row)
          0x2020 -> selected == DelayedRelease && afterClear row && matchesRead row W.Core0 0x40020004 0x20004 0
          0x11844 -> selected == DelayedRelease && afterClear row && matchesRead row W.Core1 0x14c80 0x14c80 0xd0020
          0x1184c -> selected == DelayedRelease && afterClear row && matchesRead row W.Core1 0x14c84 0x14c84 (fromIntegral expected)
          _ -> False
    require valid "unexplained CPU read source, alias, word or phase"
  let intervals = zip (cursor tailStop : map cursor waits) waits
  forM_ intervals $ \(previous,stop) -> do
    let candidates = [row | (index,row) <- waitReads, index >= cursorWord previous && index < cursorWord (cursor stop), W.cpuInstructions row < count stop]
    case candidates of
      [row] -> require (W.cpuAddress row == W.watchAddress 0x40020000 && W.cpuPhysical row == W.watchAddress 0x20000 && W.watchBytesValue (W.cpuBytes row) == 4 && W.cpuValue row == Just 0x22b && W.cpuBefore row == Just (encodeLeWord 0x22b) && W.cpuByteOrder row == W.LittleEndian) "wait load lacks source/alias/word evidence"
      _ -> Left "held observation lacks exactly one distinct original wait load"
  case selected of
    HeldSentinel -> pure ()
    DelayedRelease -> do
      ack <- one Acknowledged (stops receipt)
      preclear <- one Clear (stops receipt)
      handoff <- one Handoff (stops receipt)
      observation <- maybe (Left "clear readback missing") Right (clearReadback receipt)
      (clearIndex,primaryClear) <- findWriter 0x1ff8
      (targetIndex,targetWrite) <- findWriter 0x11f98
      (_,argumentWrite) <- findWriter 0x11fb0
      require (peerIndex < cursorWord (cursor ack) && W.cpuInstructions peerClear < count ack) "ack lacks prior peer clear attempt"
      require (any (\row -> W.cpuValue row == Just 0 && W.cpuInstructions row > W.cpuInstructions peerClear && W.cpuInstructions row < count ack) primaryPolls) "ack lacks original veneer zero load"
      require (clearIndex >= cursorWord (cursor preclear) && clearIndex < cursorWord (readbackCursor observation) && W.cpuInstructions primaryClear == readbackCount observation) "original clear attempt outside final committed readback window"
      require (all (\row -> case row of W.CpuRecord {} -> False; _ -> True) (take (cursorWord (readbackCursor observation) - clearIndex - 1) (drop (clearIndex + 1) rows))) "CPU attempt intervenes before committed clear readback"
      require (targetIndex >= cursorWord (readbackCursor observation) && readbackCount observation <= W.cpuInstructions targetWrite && W.cpuInstructions targetWrite < W.cpuInstructions argumentWrite && W.cpuInstructions argumentWrite < count handoff) "publication precedes committed clear or handoff order differs"

aggregate :: CoreCounts -> Word64
aggregate (CoreCounts first second) = first + second
validateReadback :: ControlStop -> ControlStop -> ControlStop -> ClearReadback -> ControlReceipt -> Either String ()
validateReadback preclear handoff loader observation receipt = do
  let steps = microsteps observation
  require (not (null steps) && length steps <= 2048) "clear microstep bound differs"
  forM_ steps $ \(Microstep before@(CoreCounts first second) after@(CoreCounts nextFirst nextSecond) _) ->
    require (aggregate after == aggregate before + 1 && ((nextFirst == first + 1 && nextSecond == second) || (nextFirst == first && nextSecond == second + 1))) "microstep is not exactly one aggregate entered instruction"
  require (and (zipWith (\(Microstep _ after _) (Microstep before _ _) -> after == before) steps (drop 1 steps))) "microstep counts are not contiguous"
  case (steps,reverse steps) of
    (Microstep first _ _:_,Microstep (CoreCounts before0 before1) final@(CoreCounts after0 after1) scalar:_) -> do
      require (aggregate first == countWord (count preclear) && aggregate final == countWord (readbackCount observation)) "microstep boundary counts differ"
      require (scalar == 0 && after0 == before0 + 1 && after1 == before1) "final microstep did not clear on primary core"
    _ -> Left "clear microsteps missing"
  require (all (\(Microstep _ _ scalar) -> scalar == 0x22b) (take (length steps - 1) steps)) "cell cleared before final microstep"
  require (quiet 0 (readbackState observation) && primaryAddress observation == GuestAddress 0x1ffc && primaryCpsr observation == cpsr preclear) "committed original-store readback differs"
  require (count preclear < readbackCount observation && readbackCount observation < count handoff && countWord (count loader) - countWord (readbackCount observation) <= 20000) "clear-to-loader bound/order differs"
  require (cursor preclear < readbackCursor observation && readbackCursor observation <= cursor handoff) "clear readback cursor order differs"
  validateCursor (W.watchRecords (memoryWatch receipt)) (readbackCount observation) (readbackCursor observation)
  validateObservationReads (W.watchRecords (memoryWatch receipt)) (cursor preclear) (readbackCursor observation)
  let rows = W.watchRecords (memoryWatch receipt)
      interval = take (cursorWord (readbackCursor observation) - cursorWord (cursor preclear)) (drop (cursorWord (cursor preclear)) rows)
      physicalReads physical = length [() | W.TransferRecord W.HostRead guest actual width <- interval
        ,guest == W.watchAddress physical,actual == W.watchAddress physical,W.watchBytesValue width == 4]
  require (physicalReads 0x20000 == length steps + 1 && all ((== 1) . physicalReads) [0x20004,0xa00c4,0x14c80,0x14c84]) "clear microsteps lack immediate physical scalar observations"
