{-# LANGUAGE OverloadedStrings #-}
module Emulation.PeerStageWitness
  ( Argument (..), Instructions (..), Tick (..)
  , StopLabel (..), Registers (..), PeerState (..), SavedFrame (..), Stop (..)
  , CpuAccess (..), ByteOrder (..), TransferAccess (..)
  , WatchRecord (..), WatchObservation (..), Receipt (..)
  , Core (..), Cpsr (..), GuestAddress (..), WitnessResult (..)
  , parseArgument, parseReceipt, receiptValue, validateReceipt, contractValue
  , maxReceiptBytes, stopName
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON, Object, Result (..), Value (..), fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Bits ((.&.))
import Data.List (sort)
import Data.Word (Word8, Word32, Word64)
import Emulation.FirstStageWitness (Core (..), Cpsr (..), GuestAddress (..), WitnessResult (..), encodeLeWord, leWordAt)
import Emulation.WatchObservation (ByteOrder (..), CpuAccess (..), Instructions (..), Tick (..), TransferAccess (..), WatchObservation (..), WatchRecord (..))
import qualified Emulation.WatchObservation as W
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readDec, readHex)

newtype Argument = Argument Word32 deriving (Eq, Show)

data StopLabel = Record2Entry | RendezvousTail | Record2Handoff | LoaderBEntry
  deriving (Eq, Show, Enum, Bounded)

data Registers = Registers
  { r0 :: Word32, r1 :: Word32, r2 :: Word32, r3 :: Word32
  , r4 :: Word32, r5 :: Word32, r6 :: Word32, r7 :: Word32
  , r8 :: Word32, r9 :: Word32, r10 :: Word32, r11 :: Word32, r12 :: Word32
  } deriving (Eq, Show)

data PeerState = PeerState
  { cell0 :: Word32, cell1 :: Word32, completion :: Word32
  , target :: GuestAddress, argument :: Argument
  } deriving (Eq, Show)

data SavedFrame = SavedFrame
  { savedR4 :: Word32, savedR5 :: Word32, savedR6 :: Word32
  , savedR7 :: Word32, savedR11 :: Word32, savedLr :: Word32
  } deriving (Eq, Show)

data Stop = Stop
  { stopLabel :: StopLabel, stopAddress :: GuestAddress, stopCore :: Core
  , stopInstructions :: Instructions, stopRegisters :: Registers
  , stopSp :: GuestAddress, stopLr :: GuestAddress, stopCpsr :: Cpsr
  , stopState :: PeerState, stopFrame :: Maybe SavedFrame
  } deriving (Eq, Show)

data Receipt = Receipt
  { receiptResult :: WitnessResult, receiptError :: Maybe String
  , receiptMachineFailure :: Maybe String, receiptInstructions :: Instructions
  , receiptIdentities :: (Word32, Word32), receiptStops :: [Stop]
  , receiptWatch :: WatchObservation
  } deriving (Eq, Show)

maxReceiptBytes :: Int
maxReceiptBytes = 16 * 1024 * 1024

stopName :: StopLabel -> String
stopName label = case label of
  Record2Entry -> "record-2-entry"
  RendezvousTail -> "record-2-rendezvous-tail"
  Record2Handoff -> "record-2-handoff"
  LoaderBEntry -> "loader-b-entry"

parseArgument :: String -> Either String Argument
parseArgument text = case parsed of
  [(value, "")] | value <= toInteger (maxBound :: Word32) -> Right (Argument (fromInteger value))
  _ -> Left "argument must be an unsigned 32-bit decimal or 0x hexadecimal word"
  where
    parsed = case text of
      '0' : 'x' : digits -> readHex digits
      '0' : 'X' : digits -> readHex digits
      _ -> readDec text

contractValue :: Value
contractValue = object ["schema" .= (1 :: Int), "budget" .= (20000000 :: Int)]

fields :: [String] -> Value -> Either String Object
fields names (Object value) = do
  unless (sort names == sort (map K.toString (KM.keys value))) (Left "peer witness fields differ from schema 1")
  pure value
fields _ _ = Left "peer witness value is not an object"

field :: Object -> String -> Either String Value
field value name = maybe (Left ("missing peer witness field: " ++ name)) Right (KM.lookup (K.fromString name) value)

json :: FromJSON a => Value -> Either String a
json value = case fromJSON value of
  Error problem -> Left problem
  Success result -> Right result

get :: FromJSON a => Object -> String -> Either String a
get value name = field value name >>= json

address :: Object -> String -> Either String GuestAddress
address value name = GuestAddress . fromIntegral <$> (get value name :: Either String Word32)

core :: Object -> String -> Either String Core
core value name = do
  number <- get value name :: Either String Word8
  case number of
    0 -> Right Core0
    1 -> Right Core1
    _ -> Left "peer witness core must be 0 or 1"

array :: Value -> Either String [Value]
array = json

nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value

parseRegisters :: Value -> Either String Registers
parseRegisters value = do
  wordsValue <- json value :: Either String [Word32]
  case wordsValue of
    [a,b,c,d,e,f,g,h,i,j,k,l,m] -> Right (Registers a b c d e f g h i j k l m)
    _ -> Left "peer stop must carry r0 through r12"

parseState :: Value -> Either String PeerState
parseState value = do
  state <- fields ["cell0", "cell1", "completion", "target", "argument"] value
  PeerState <$> get state "cell0" <*> get state "cell1" <*> get state "completion"
    <*> address state "target" <*> (Argument <$> get state "argument")

parseFrame :: Value -> Either String SavedFrame
parseFrame value = do
  bytes <- B.pack <$> (json value :: Either String [Word8])
  unless (B.length bytes == 24) (Left "peer tail saved frame must contain 24 bytes")
  SavedFrame <$> leWordAt bytes 0 <*> leWordAt bytes 4 <*> leWordAt bytes 8
    <*> leWordAt bytes 12 <*> leWordAt bytes 16 <*> leWordAt bytes 20

parseStop :: Value -> Either String Stop
parseStop value = do
  row <- fields ["label", "address", "core", "instructions", "registers", "sp", "lr", "cpsr", "state", "frame"] value
  name <- get row "label"
  label <- case [item | item <- [minBound .. maxBound], stopName item == name] of
    [item] -> Right item
    _ -> Left "unknown peer stop label"
  Stop label <$> address row "address" <*> core row "core"
    <*> (Instructions <$> get row "instructions") <*> (field row "registers" >>= parseRegisters)
    <*> address row "sp" <*> address row "lr" <*> (Cpsr <$> get row "cpsr")
    <*> (field row "state" >>= parseState) <*> (field row "frame" >>= nullable parseFrame)

parseWatch :: Value -> Either String WatchObservation
parseWatch value = do
  limit <- W.watchLimit 16384
  W.parseWatchObservation limit value

parseReceipt :: B.ByteString -> Either String Receipt
parseReceipt bytes = do
  unless (B.length bytes <= maxReceiptBytes) (Left "peer receipt exceeds 16 MiB")
  value <- decodeStrictIntegers bytes
  row <- fields ["schema", "result", "error", "machine_failure", "instructions", "cp14_identities", "stops", "memory_watch"] value
  schema <- get row "schema" :: Either String Word32
  unless (schema == 1) (Left "unsupported peer receipt schema")
  resultText <- get row "result" :: Either String String
  result <- case resultText of
    "passed" -> Right Passed
    "failed" -> Right Failed
    _ -> Left "unknown peer witness result"
  identities <- get row "cp14_identities" :: Either String [Word32]
  pair <- case identities of
    [first, second] -> Right (first, second)
    _ -> Left "peer receipt requires two CP14 identities"
  Receipt result <$> (field row "error" >>= nullable json)
    <*> (field row "machine_failure" >>= nullable json) <*> (Instructions <$> get row "instructions")
    <*> pure pair <*> (field row "stops" >>= array >>= mapM parseStop)
    <*> (field row "memory_watch" >>= parseWatch)

registerWords :: Registers -> [Word32]
registerWords regs = [r0 regs,r1 regs,r2 regs,r3 regs,r4 regs,r5 regs,r6 regs,r7 regs,r8 regs,r9 regs,r10 regs,r11 regs,r12 regs]

frameWords :: SavedFrame -> [Word32]
frameWords frame = [savedR4 frame,savedR5 frame,savedR6 frame,savedR7 frame,savedR11 frame,savedLr frame]

addressWord :: GuestAddress -> Word64
addressWord (GuestAddress value) = value

coreNumber :: Core -> Int
coreNumber Core0 = 0
coreNumber Core1 = 1

instructionsWord :: Instructions -> Word64
instructionsWord (Instructions value) = value

stateValue :: PeerState -> Value
stateValue state = let Argument arg = argument state in object
  ["cell0" .= cell0 state, "cell1" .= cell1 state, "completion" .= completion state
  ,"target" .= addressWord (target state), "argument" .= arg]

stopValue :: Stop -> Value
stopValue stop = let Cpsr cpsr = stopCpsr stop in object
  [ "label" .= stopName (stopLabel stop), "address" .= addressWord (stopAddress stop)
  , "core" .= coreNumber (stopCore stop), "instructions" .= instructionsWord (stopInstructions stop)
  , "registers" .= registerWords (stopRegisters stop), "sp" .= addressWord (stopSp stop)
  , "lr" .= addressWord (stopLr stop), "cpsr" .= cpsr, "state" .= stateValue (stopState stop)
  , "frame" .= fmap (B.unpack . B.concat . map encodeLeWord . frameWords) (stopFrame stop)
  ]

recordValue :: WatchRecord -> Value
recordValue record = case record of
  CpuRecord access guest physical bytes value before order cpu pc instructions (Tick tick) -> object
    [ "source" .= (case access of CpuWrite -> "cpu-write"; CpuRead -> "cpu-read"; CpuFetch -> "cpu-fetch" :: String)
    , "address" .= W.watchAddressValue guest, "physical" .= W.watchAddressValue physical, "size" .= W.watchBytesValue bytes
    , "value" .= value, "before" .= fmap B.unpack before
    , "byte_order" .= (case order of LittleEndian -> "little"; BigEndian -> "big" :: String)
    , "core" .= (case cpu of W.Core0 -> 0; W.Core1 -> 1 :: Int)
    , "pc" .= W.watchAddressValue pc, "instructions" .= instructionsWord instructions, "tick" .= tick
    ]
  TransferRecord access guest physical bytes -> object
    [ "source" .= (case access of HostRead -> "host-read"; HostWrite -> "host-write"; ModelBusRead -> "model-bus-read"; ModelBusWrite -> "model-bus-write" :: String)
    , "address" .= W.watchAddressValue guest, "physical" .= W.watchAddressValue physical, "size" .= W.watchBytesValue bytes
    , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null
    ]

receiptValue :: Receipt -> Value
receiptValue receipt = let
  watch = receiptWatch receipt
  (identity0, identity1) = receiptIdentities receipt
  in object
    [ "schema" .= (1 :: Int), "result" .= (case receiptResult receipt of Passed -> "passed"; Failed -> "failed" :: String)
    , "error" .= receiptError receipt, "machine_failure" .= receiptMachineFailure receipt
    , "instructions" .= instructionsWord (receiptInstructions receipt), "cp14_identities" .= [identity0,identity1]
    , "stops" .= map stopValue (receiptStops receipt)
    , "memory_watch" .= object
        [ "enabled" .= watchEnabled watch
        , "ranges" .= [[low,high] | range <- watchRanges watch, let (low,high) = W.watchRangeBounds range]
        , "records" .= map recordValue (watchRecords watch), "violation" .= watchViolation watch
        ]
    ]

expectedRanges :: [(Word64, Word64)]
expectedRanges = [(0x20000,0x20008),(0xa00c4,0xa00c8),(0x14c80,0x14c88)]

increasing :: Ord a => [a] -> Bool
increasing values = and (zipWith (<) values (drop 1 values))

nondecreasing :: Ord a => [a] -> Bool
nondecreasing values = and (zipWith (<=) values (drop 1 values))

validateReceipt :: Argument -> Receipt -> Either String ()
validateReceipt expected receipt = do
  unless (receiptResult receipt == Passed && receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing) (Left "peer witness failed")
  unless (receiptIdentities receipt == (0x4003,0x5003)) (Left "peer CP14 identities differ")
  unless (receiptInstructions receipt > Instructions 0 && receiptInstructions receipt <= Instructions 20000000) (Left "peer instruction budget exceeded")
  let stops = receiptStops receipt
  unless (map stopLabel stops == [Record2Entry,RendezvousTail,Record2Handoff,LoaderBEntry]) (Left "peer stop ordering differs")
  unless (map stopAddress stops == map GuestAddress [0x11800,0x11f40,0x11850,0xd0020]) (Left "peer stop addresses differ")
  unless (all ((== Core1) . stopCore) stops) (Left "peer stop belongs to wrong core")
  unless (increasing (map stopInstructions stops) && all ((<= receiptInstructions receipt) . stopInstructions) stops) (Left "peer stop instruction ordering differs")
  case stops of
    [entry,tailStop,handoff,loader] -> do
      unless (stopFrame entry == Nothing && stopFrame handoff == Nothing && stopFrame loader == Nothing) (Left "saved frame outside rendezvous tail")
      frame <- maybe (Left "rendezvous tail lacks saved frame") Right (stopFrame tailStop)
      let GuestAddress sp = stopSp tailStop
      unless (sp + 64 <= 0xffffffff && fromIntegral (r11 (stopRegisters tailStop)) == sp + 60 && r6 (stopRegisters tailStop) == 1) (Left "rendezvous tail frame pointer or discriminator differs")
      unless (savedLr frame == 0x1183c) (Left "rendezvous saved return address differs")
      forM_ [handoff,loader] $ \stop -> do
        let regs = stopRegisters stop
            Cpsr cpsr = stopCpsr stop
            state = stopState stop
            Argument arg = expected
        unless (cpsr .&. 0x010002ff == 0xd3) (Left "handoff must be masked A32 little-endian SVC")
        unless (r0 regs == arg && r3 regs == 0xd0020 && target state == GuestAddress 0xd0020 && argument state == expected) (Left "peer handoff target or argument differs")
        unless (cell0 state == 0 && cell1 state == 0) (Left "peer cell readback is not clear")
        unless (stopSp stop == GuestAddress (sp + 64)) (Left "peer stack was not restored")
        unless ([r4 regs,r5 regs,r6 regs,r7 regs,r11 regs] == take 5 (frameWords frame)) (Left "peer callee-saved registers differ from saved frame")
      validateWatch expected tailStop handoff receipt
    _ -> Left "peer receipt requires four stops"

validateWatch :: Argument -> Stop -> Stop -> Receipt -> Either String ()
validateWatch (Argument expected) tailStop handoff receipt = do
  let watch = receiptWatch receipt
      records = watchRecords watch
      cpus = [row | row@CpuRecord {} <- records]
  unless (watchEnabled watch && sort (map W.watchRangeBounds (watchRanges watch)) == sort expectedRanges && watchViolation watch == Nothing) (Left "peer watch is disabled, misconfigured or violated")
  unless (length records <= 16384) (Left "peer watch record limit exceeded")
  unless (nondecreasing (map cpuInstructions cpus) && nondecreasing (map cpuTick cpus)) (Left "CPU attempt chronology differs")
  forM_ cpus $ \row -> unless (cpuInstructions row <= receiptInstructions receipt) (Left "CPU attempt exceeds receipt instruction count")
  forM_ records $ \row -> do
    let (physical, bytes) = case row of
          CpuRecord {} -> (cpuPhysical row,cpuBytes row)
          TransferRecord {} -> (transferPhysical row,transferBytes row)
        start = fromIntegral (W.watchAddressValue physical) :: Word64
        size = fromIntegral (W.watchBytesValue bytes) :: Word64
        overlaps (low,high) = start < high && low < start + size
    unless (any overlaps expectedRanges) (Left "watch record lies outside armed ranges")
  sentinel0 <- writer W.Core0 0x1e64 0x20000 0x22b
  sentinel1 <- writer W.Core0 0x1e68 0x20004 0x22b
  peerClear <- writer W.Core1 0x11f4c 0x20004 0
  primaryClear <- writer W.Core0 0x1ff8 0x20000 0
  publicationTarget <- writer W.Core1 0x11f98 0x14c80 0xd0020
  publicationArgument <- writer W.Core1 0x11fb0 0x14c84 (fromIntegral expected)
  unless (cpuAddress peerClear == W.watchAddress 0x40020004) (Left "peer clear lacks the guest alias")
  let time = cpuInstructions
  unless (time sentinel0 < time sentinel1 && time sentinel0 < time primaryClear && time sentinel1 < time peerClear) (Left "sentinel/clear attempt ordering differs")
  unless (stopInstructions tailStop < time peerClear && max (time primaryClear) (time peerClear) < time publicationTarget && time publicationTarget < time publicationArgument && time publicationArgument < stopInstructions handoff) (Left "peer publication attempt ordering differs")
  where
    writer wantedCore pc physical value = do
      let matches = [row | row@CpuRecord {} <- watchRecords (receiptWatch receipt), cpuAccess row == CpuWrite, cpuPc row == W.watchAddress pc, cpuPhysical row == W.watchAddress physical]
      row <- case matches of
        [only] -> Right only
        _ -> Left "required CPU writer attempt missing or repeated"
      unless (cpuCore row == wantedCore && W.watchBytesValue (cpuBytes row) == 4 && cpuValue row == Just value && cpuByteOrder row == LittleEndian && maybe False ((== 4) . B.length) (cpuBefore row)) (Left "CPU writer attribution or attempt metadata differs")
      unless (cpuAddress row == W.watchAddress physical || cpuAddress row == W.watchAddress (physical + 0x40000000)) (Left "CPU writer guest/physical alias differs")
      pure row
