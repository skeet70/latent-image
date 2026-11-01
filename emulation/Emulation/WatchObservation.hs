{-# LANGUAGE OverloadedStrings #-}
-- | Ranges are half-open physical byte ranges.
module Emulation.WatchObservation
  ( WatchAddress
  , watchAddress
  , watchAddressValue
  , WatchRange
  , watchRange
  , watchRangeBounds
  , unionWatchRanges
  , overlapsWatchRange
  , WatchBytes
  , watchBytesValue
  , Instructions (..)
  , instructionCount
  , Tick (..)
  , Sectors
  , sectorsValue
  , Commands
  , commandsValue
  , Irqs
  , irqsValue
  , Boundaries
  , boundariesValue
  , WatchLimit
  , watchLimit
  , watchLimitValue
  , WatchGate (..)
  , ProtectedWriteSummary (..)
  , protectedCpuWritesValue
  , protectedHostWritesValue
  , protectedBusWritesValue
  , noProtectedWrites
  , RunPurpose (..)
  , WatchJudgement (..)
  , CoreId (..)
  , ByteOrder (..)
  , CpuAccess (..)
  , TransferAccess (..)
  , WatchRecord (..)
  , WatchObservation (..)
  , WatchAck
  , parseWatchAck
  , parseWatchObservation
  , checkWatch
  , judgeWatch
  , FixedWatchAck
  , FixedWatchObservation
  , fixedWatchRecords
  , parseFixedWatchAck
  , parseFixedWatchObservation
  , ForbiddenFetch (..)
  , ExecutionGate (..)
  , ExecutionInterval (..)
  , executionIntervalLabel
  , checkExecutionReceipt
  , checkExecution
  , judgeExecution
  , checkExecutionWrites
  , WritePolicy (..)
  , writePolicyLabel
  , ExecutionWriteGate (..)
  , ExecutionWriteSummary (..)
  , identityCpuStoresValue
  , ExecutionWriteFault (..)
  , ExecutionWriteViolation (..)
  , judgeExecutionWrites
  , executionWriteGateJson
  , forbiddenFetchJson
  , AcceptedWrites
  , acceptedWrites
  , acceptedWritesValue
  , AcceptedBytes
  , acceptedBytes
  , acceptedBytesValue
  , MediaObservation
  , mediaPresent
  , mediaWriteProtected
  , mediaDirtySectors
  , mediaStarted
  , mediaCompleted
  , mediaIrqs
  , mediaDmaBoundaries
  , mediaAcceptedWrites
  , mediaAcceptedBytes
  , parseMediaObservation
  , checkMedia
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), withText, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Bits (shiftL, shiftR)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.List (sort, sortOn)
import Data.Word (Word32, Word64, Word8)

newtype WatchAddress = WatchAddress { watchAddressValue :: Word32 } deriving (Eq, Ord, Show)

watchAddress :: Word32 -> WatchAddress
watchAddress = WatchAddress

data WatchRange = WatchRange { watchRangeLow :: WatchAddress, watchRangeHigh :: WatchAddress } deriving (Eq, Show)

watchRange :: WatchAddress -> WatchAddress -> Either String WatchRange
watchRange low high
  | watchAddressValue low < watchAddressValue high = Right (WatchRange low high)
  | otherwise = Left "watch range must be increasing"

watchRangeBounds :: WatchRange -> (Word64, Word64)
watchRangeBounds (WatchRange (WatchAddress low) (WatchAddress high)) = (fromIntegral low, fromIntegral high)

unionWatchRanges :: [WatchRange] -> [WatchRange]
unionWatchRanges = foldr merge [] . sortOn watchRangeLow
  where
    merge range [] = [range]
    merge range (next : rest)
      | watchRangeHigh range >= watchRangeLow next =
          merge (WatchRange (watchRangeLow range) (max (watchRangeHigh range) (watchRangeHigh next))) rest
      | otherwise = range : next : rest

-- | Upstream Records::overlaps: a half-open physical overlap, not containment.
overlapsWatchRange :: WatchRange -> WatchAddress -> WatchBytes -> Bool
overlapsWatchRange range (WatchAddress address) (WatchBytes bytes) =
  let (low, high) = watchRangeBounds range
      start = fromIntegral address :: Word64
   in start < high && low < start + fromIntegral bytes

newtype WatchBytes = WatchBytes { watchBytesValue :: Int } deriving (Eq, Ord, Show)

newtype Instructions = Instructions { instructionsValue :: Word64 } deriving (Eq, Ord, Show)

instructionCount :: Word64 -> Instructions
instructionCount = Instructions
newtype Tick = Tick { tickValue :: Word64 } deriving (Eq, Ord, Show)
newtype Sectors = Sectors { sectorsValue :: Word64 } deriving (Eq, Ord, Show)
newtype Commands = Commands { commandsValue :: Word64 } deriving (Eq, Ord, Show)
newtype Irqs = Irqs { irqsValue :: Word64 } deriving (Eq, Ord, Show)
newtype Boundaries = Boundaries { boundariesValue :: Word64 } deriving (Eq, Ord, Show)

-- | The pinned X-M1 document has two cores.
data CoreId = Core0 | Core1 deriving (Eq, Ord, Show)

coreId :: Int -> Either String CoreId
coreId 0 = Right Core0
coreId 1 = Right Core1
coreId _ = Left "watch core is not 0 or 1"

data ByteOrder = LittleEndian | BigEndian deriving (Eq, Show)

data CpuAccess = CpuWrite | CpuRead | CpuFetch deriving (Eq, Show)

data TransferAccess = HostRead | HostWrite | ModelBusRead | ModelBusWrite deriving (Eq, Show)

-- CPU records describe attempts, not committed readback.
data WatchRecord
  = CpuRecord
      { cpuAccess :: CpuAccess
      , cpuAddress :: WatchAddress
      , cpuPhysical :: WatchAddress
      , cpuBytes :: WatchBytes
      , cpuValue :: Maybe Word64
      , cpuBefore :: Maybe B.ByteString
      , cpuByteOrder :: ByteOrder
      , cpuCore :: CoreId
      , cpuPc :: WatchAddress
      , cpuInstructions :: Instructions
      , cpuTick :: Tick
      }
  | TransferRecord
      { transferAccess :: TransferAccess
      , transferAddress :: WatchAddress
      , transferPhysical :: WatchAddress
      , transferBytes :: WatchBytes
      }
  deriving (Eq, Show)

data WatchObservation = WatchObservation
  { watchEnabled :: Bool
  , watchRanges :: [WatchRange]
  , watchRecords :: [WatchRecord]
  , watchViolation :: Maybe String
  } deriving (Eq, Show)

data WatchAck = WatchAck { ackArmed :: Bool, ackRanges :: [WatchRange] } deriving (Eq, Show)

data GuardAccess = GuardWrites | GuardFetches deriving (Eq, Show)
data GuardClass = GuardClass WatchRange GuardAccess deriving (Eq, Show)
data FixedWatchAck = FixedWatchAck WatchAck [GuardClass] deriving (Eq, Show)
data FixedWatchObservation = FixedWatchObservation WatchObservation [GuardClass] deriving (Eq, Show)

fixedWatchRecords :: FixedWatchObservation -> [WatchRecord]
fixedWatchRecords (FixedWatchObservation observation _) = watchRecords observation

data ForbiddenFetch = ForbiddenFetch
  { forbiddenFetchCore :: CoreId
  , forbiddenFetchPc :: WatchAddress
  , forbiddenFetchAddress :: WatchAddress
  , forbiddenFetchPhysical :: WatchAddress
  , forbiddenFetchBytes :: WatchBytes
  , forbiddenFetchInstructions :: Instructions
  , forbiddenFetchTick :: Tick
  } deriving (Eq, Show)

data ExecutionGate = ExecutionAccepted | ExecutionRejected [ForbiddenFetch] deriving (Eq, Show)

data ExecutionInterval = BeforeUnload | ThroughAfterUnload deriving (Eq, Show)

executionIntervalLabel :: ExecutionInterval -> T.Text
executionIntervalLabel BeforeUnload = "before-unload"
executionIntervalLabel ThroughAfterUnload = "through-after-unload"

data WritePolicy = PreserveProtectedBytes | ForbidProtectedWrites deriving (Eq, Show)

writePolicyLabel :: WritePolicy -> T.Text
writePolicyLabel PreserveProtectedBytes = "preserve-protected-bytes"
writePolicyLabel ForbidProtectedWrites = "forbid-protected-writes"

newtype IdentityCpuStores = IdentityCpuStores { identityCpuStoresValue :: Int } deriving (Eq, Show)

data ExecutionWriteSummary = ExecutionWriteSummary
  { executionTotalWrites :: ProtectedWriteSummary
  , executionIdentityCpuStores :: IdentityCpuStores
  } deriving (Eq, Show)

data ExecutionWriteFault
  = StrictProtectedWrite
  | ChangedProtectedByte
  | UnverifiableCpuStore
  | ForbiddenHostWrite
  | ForbiddenModelBusWrite
  deriving (Eq, Show)

data ExecutionWriteViolation = ExecutionWriteViolation
  { executionWriteFault :: ExecutionWriteFault
  , executionWriteRecord :: WatchRecord
  } deriving (Eq, Show)

data ExecutionWriteGate
  = ExecutionWritesAccepted ExecutionWriteSummary
  | ExecutionWritesRejected ExecutionWriteViolation ExecutionWriteSummary
  deriving (Eq, Show)

data WatchLimit = WatchLimit { watchLimitValue :: Int } deriving (Eq, Show)

watchLimit :: Int -> Either String WatchLimit
watchLimit value
  | value >= 1 && value <= 1 `shiftL` 20 = Right (WatchLimit value)
  | otherwise = Left "watch limit must be 1..2^20"

newtype ProtectedCpuWrites = ProtectedCpuWrites { protectedCpuWritesValue :: Int } deriving (Eq, Show)
newtype ProtectedHostWrites = ProtectedHostWrites { protectedHostWritesValue :: Int } deriving (Eq, Show)
newtype ProtectedBusWrites = ProtectedBusWrites { protectedBusWritesValue :: Int } deriving (Eq, Show)

data ProtectedWriteSummary = ProtectedWriteSummary
  { protectedCpuWrites :: ProtectedCpuWrites
  , protectedHostWrites :: ProtectedHostWrites
  , protectedBusWrites :: ProtectedBusWrites
  } deriving (Eq, Show)

data WatchGate = WatchAcceptedClean | WatchRejectedProtectedWrites ProtectedWriteSummary deriving (Eq, Show)

data WatchJudgement = CandidateQualified | ControlRefusedProtectedWrite deriving (Eq, Show)

instance ToJSON WatchJudgement where
  toJSON CandidateQualified = String "candidate-qualified"
  toJSON ControlRefusedProtectedWrite = String "control-refused"

instance FromJSON WatchJudgement where
  parseJSON = withText "WatchJudgement" $ \text -> case text of
    "candidate-qualified" -> pure CandidateQualified
    "control-refused" -> pure ControlRefusedProtectedWrite
    other -> fail ("unknown watch judgement: " ++ T.unpack other)

field :: String -> Value -> Either String Value
field name (Object object) = maybe (Left ("missing field " ++ name)) Right (KM.lookup (K.fromString name) object)
field _ _ = Left "watch value is not an object"

exactKeys :: [String] -> Value -> Either String ()
exactKeys expected (Object object) =
  let actual = sort (map (T.unpack . K.toText) (KM.keys object))
   in unless (actual == sort expected) (Left "watch fields differ from the pinned schema")
exactKeys _ _ = Left "watch value is not an object"

hasField :: String -> Value -> Bool
hasField name (Object object) = KM.member (K.fromString name) object
hasField _ _ = False

asText :: Value -> Either String String
asText (String text) = Right (T.unpack text)
asText _ = Left "watch field is not text"

asWord64 :: Value -> Either String Word64
asWord64 (Number number) = maybe (Left "watch number is out of range") Right (toBoundedInteger number)
asWord64 _ = Left "watch number is not an integer"

asWord32 :: Value -> Either String Word32
asWord32 value = do
  number <- asWord64 value
  if number <= fromIntegral (maxBound :: Word32) then Right (fromIntegral number) else Left "watch address exceeds 32 bits"

asInt :: Value -> Either String Int
asInt value = do
  number <- asWord64 value
  if number <= fromIntegral (maxBound :: Int) then Right (fromIntegral number) else Left "watch count is too large"

asBool :: Value -> Either String Bool
asBool (Bool value) = Right value
asBool _ = Left "watch flag is not Boolean"

asArray :: Value -> Either String [Value]
asArray (Array values) = Right (V.toList values)
asArray _ = Left "watch field is not an array"

parseRange :: Value -> Either String WatchRange
parseRange entry = do
  pair <- asArray entry
  case pair of
    [low, high] -> do
      lowAddress <- asWord32 low
      highAddress <- asWord32 high
      watchRange (WatchAddress lowAddress) (WatchAddress highAddress)
    _ -> Left "watch range must be a low/high pair"

parseRanges :: Value -> Either String [WatchRange]
parseRanges value = asArray value >>= mapM parseRange

parseByteOrder :: Value -> Either String ByteOrder
parseByteOrder (String "little") = Right LittleEndian
parseByteOrder (String "big") = Right BigEndian
parseByteOrder _ = Left "watch byte_order must be little or big"

parseLimitBytes :: Int -> Value -> Either String WatchBytes
parseLimitBytes upper value = do
  bytes <- asInt value
  if bytes >= 1 && bytes <= upper then Right (WatchBytes bytes) else Left "watch access width out of range"

parseBefore :: Value -> Either String (Maybe B.ByteString)
parseBefore Null = Right Nothing
parseBefore value = do
  bytes <- asArray value
  Just . B.pack <$> forM bytes (\byte -> do
    number <- asInt byte
    if number <= 255 then Right (fromIntegral number :: Word8) else Left "watch before byte out of range")

numericBefore :: ByteOrder -> B.ByteString -> Word64
numericBefore order bytes = case order of
  LittleEndian -> B.foldr (\byte acc -> acc * 256 + fromIntegral byte) 0 bytes
  BigEndian -> B.foldl' (\acc byte -> acc * 256 + fromIntegral byte) 0 bytes

cpuKeys :: [String]
cpuKeys = ["source", "address", "physical", "size", "value", "before", "byte_order", "core", "pc", "instructions", "tick"]

transferKeys :: [String]
transferKeys = ["source", "physical", "address", "size", "core", "pc", "instructions", "tick"]

checkExtent :: Word32 -> Int -> Either String ()
checkExtent address size =
  let start = fromIntegral address :: Word64
   in unless (start + fromIntegral size <= 1 `shiftL` 32) (Left "watch access exceeds the 32-bit address space")

parseCpuRecord :: CpuAccess -> Value -> Either String WatchRecord
parseCpuRecord access value = do
  exactKeys cpuKeys value
  address <- field "address" value >>= asWord32
  physical <- field "physical" value >>= asWord32
  bytes <- field "size" value >>= parseLimitBytes 16
  order <- field "byte_order" value >>= parseByteOrder
  before <- field "before" value >>= parseBefore
  recordValue <- field "value" value >>= parseValue
  core <- field "core" value >>= asInt >>= coreId
  pc <- field "pc" value >>= asWord32
  instructions <- Instructions <$> (field "instructions" value >>= asWord64)
  tick <- Tick <$> (field "tick" value >>= asWord64)
  let WatchBytes size = bytes
  checkExtent address size
  checkExtent physical size
  case access of
    CpuFetch -> do
      unless (before == Nothing) (Left "cpu-fetch carries before bytes")
      unless (recordValue == Nothing) (Left "cpu-fetch carries a value")
    CpuRead -> do
      previous <- requireBefore before
      unless (B.length previous == size) (Left "cpu before length differs from size")
      when (size <= 8) $ do
        current <- requireValue recordValue
        unless (current == numericBefore order previous) (Left "cpu-read value disagrees with before bytes")
      when (size > 8) (unless (recordValue == Nothing) (Left "cpu-read wider than 8 carries a value"))
    CpuWrite -> do
      previous <- requireBefore before
      unless (B.length previous == size) (Left "cpu before length differs from size")
      when (size <= 8) (requireValue recordValue >> pure ())
      when (size > 8) (unless (recordValue == Nothing) (Left "cpu-write wider than 8 carries a value"))
      case recordValue of
        Just current | size < 8 ->
          unless (current < (1 `shiftL` (size * 8) :: Word64)) (Left "cpu-write value exceeds its width")
        _ -> pure ()
  pure (CpuRecord access (WatchAddress address) (WatchAddress physical) bytes recordValue before order core (WatchAddress pc) instructions tick)
  where
    requireBefore before = case before of
      Just bytes -> Right bytes
      Nothing -> Left "cpu read/write lacks before bytes"
    requireValue recordValue = case recordValue of
      Just number -> Right number
      Nothing -> Left "cpu read/write lacks its value"

parseValue :: Value -> Either String (Maybe Word64)
parseValue Null = Right Nothing
parseValue value = Just <$> asWord64 value

parseTransferRecord :: TransferAccess -> Value -> Either String WatchRecord
parseTransferRecord access value = do
  exactKeys transferKeys value
  address <- field "address" value >>= asWord32
  physical <- field "physical" value >>= asWord32
  bytes <- field "size" value >>= parseLimitBytes (fromIntegral (maxBound :: Word32))
  let WatchBytes size = bytes
  checkExtent address size
  checkExtent physical size
  forM_ ["core", "pc", "instructions", "tick"] $ \name ->
    unless (field name value == Right Null) (Left ("transfer record " ++ name ++ " is not null"))
  forM_ ["value", "before", "byte_order"] $ \name ->
    when (hasField name value) (Left ("transfer record carries " ++ name))
  pure (TransferRecord access (WatchAddress address) (WatchAddress physical) bytes)

parseRecord :: Value -> Either String WatchRecord
parseRecord value = do
  source <- field "source" value >>= asText
  case source of
    "cpu-write" -> parseCpuRecord CpuWrite value
    "cpu-read" -> parseCpuRecord CpuRead value
    "cpu-fetch" -> parseCpuRecord CpuFetch value
    "host-read" -> parseTransferRecord HostRead value
    "host-write" -> parseTransferRecord HostWrite value
    "model-bus-read" -> parseTransferRecord ModelBusRead value
    "model-bus-write" -> parseTransferRecord ModelBusWrite value
    _ -> Left "unknown watch record source"

parseWatchAck :: Value -> Either String WatchAck
parseWatchAck value = do
  exactKeys ["armed", "ranges"] value
  WatchAck
    <$> (field "armed" value >>= asBool)
    <*> (field "ranges" value >>= parseRanges)

parseWatchObservation :: WatchLimit -> Value -> Either String WatchObservation
parseWatchObservation limit value = do
  exactKeys ["enabled", "ranges", "records", "violation"] value
  enabled <- field "enabled" value >>= asBool
  ranges <- field "ranges" value >>= parseRanges
  records <- field "records" value >>= asArray >>= mapM parseRecord
  violation <- case field "violation" value of
    Right Null -> Right Nothing
    Right (String text) -> Right (Just (T.unpack text))
    Right _ -> Left "watch violation is not text or null"
    Left problem -> Left problem
  unless (length records <= watchLimitValue limit) (Left "watch records exceed the declared limit")
  pure (WatchObservation enabled ranges records violation)

parseGuardClasses :: Value -> Either String [GuardClass]
parseGuardClasses value = asArray value >>= mapM (\entry -> do
  exactKeys ["range", "kinds"] entry
  range <- field "range" entry >>= parseRange
  kinds <- field "kinds" entry
  access <- if kinds == toJSON (["cpu-write", "host-write", "model-bus-write"] :: [String])
    then Right GuardWrites
    else if kinds == toJSON (["cpu-fetch"] :: [String])
      then Right GuardFetches
      else Left "execution watch class has unsupported access sources"
  pure (GuardClass range access))

parseFixedWatchAck :: Value -> Either String FixedWatchAck
parseFixedWatchAck value = do
  exactKeys ["armed", "ranges", "views", "classes"] value
  unless (field "views" value == Right (String "fixed")) (Left "execution watch acknowledgement is not fixed-view")
  classes <- field "classes" value >>= parseGuardClasses
  case value of
    Object row -> FixedWatchAck <$> parseWatchAck (Object (KM.delete "classes" (KM.delete "views" row))) <*> pure classes
    _ -> Left "execution watch acknowledgement is not an object"

parseFixedWatchObservation :: WatchLimit -> Value -> Either String FixedWatchObservation
parseFixedWatchObservation limit value = do
  exactKeys ["enabled", "ranges", "records", "violation", "classes", "views"] value
  unless (field "views" value == Right (String "fixed")) (Left "execution watch is not fixed-view")
  classes <- field "classes" value >>= parseGuardClasses
  case value of
    Object row -> FixedWatchObservation <$> parseWatchObservation limit (Object (KM.delete "classes" (KM.delete "views" row))) <*> pure classes
    _ -> Left "execution watch is not an object"

checkExecutionWrites :: WritePolicy -> WatchLimit -> [WatchRange] -> [WatchRange] -> FixedWatchAck -> FixedWatchObservation -> Either String ExecutionWriteGate
checkExecutionWrites policy limit protected forbidden (FixedWatchAck ack ackClasses) (FixedWatchObservation observation classes) = do
  let expected = map (`GuardClass` GuardWrites) protected ++ map (`GuardClass` GuardFetches) forbidden
      watched = unionWatchRanges (protected ++ forbidden)
      matches record (GuardClass range access) = case (record, access) of
        (CpuRecord{cpuAccess = CpuFetch, cpuPhysical = physical, cpuBytes = bytes}, GuardFetches) -> overlapsWatchRange range physical bytes
        (CpuRecord{cpuAccess = CpuWrite, cpuPhysical = physical, cpuBytes = bytes}, GuardWrites) -> overlapsWatchRange range physical bytes
        (TransferRecord{transferAccess = HostWrite, transferPhysical = physical, transferBytes = bytes}, GuardWrites) -> overlapsWatchRange range physical bytes
        (TransferRecord{transferAccess = ModelBusWrite, transferPhysical = physical, transferBytes = bytes}, GuardWrites) -> overlapsWatchRange range physical bytes
        _ -> False
  unless (not (null watched) && length watched <= 8 && classes == expected && ackClasses == expected)
    (Left "execution watch classes differ from the declared write and fetch ranges")
  forM_ (watchRecords observation) $ \record -> do
    unless (any (matches record) classes) (Left "execution watch record is outside its declared access classes")
    case record of
      CpuRecord{cpuAccess = CpuFetch, cpuPc = pc, cpuAddress = address} ->
        unless (pc == address) (Left "execution fetch PC differs from its instruction address")
      _ -> pure ()
  _ <- checkWatch limit watched ack observation
  pure (summarizeExecutionWrites policy protected (watchRecords observation))

storePreservesProtectedBytes :: [WatchRange] -> WatchRecord -> Either ExecutionWriteFault ()
storePreservesProtectedBytes protected CpuRecord{cpuAccess = CpuWrite, cpuPhysical = physical, cpuBytes = WatchBytes size,
    cpuValue = Just value, cpuBefore = Just before, cpuByteOrder = order}
  | size >= 1 && size <= 8 && B.length before == size
      && (size == 8 || value < (1 `shiftL` (size * 8))) =
      let start = fromIntegral (watchAddressValue physical) :: Word64
          changed index
            | index == size = False
            | otherwise =
                let address = start + fromIntegral index
                    guarded = any (\range -> let (low, high) = watchRangeBounds range in low <= address && address < high) protected
                    shift = case order of LittleEndian -> index * 8; BigEndian -> (size - index - 1) * 8
                 in (guarded && B.index before index /= fromIntegral (value `shiftR` shift)) || changed (index + 1)
       in if changed 0 then Left ChangedProtectedByte else Right ()
storePreservesProtectedBytes _ _ = Left UnverifiableCpuStore

summarizeExecutionWrites :: WritePolicy -> [WatchRange] -> [WatchRecord] -> ExecutionWriteGate
summarizeExecutionWrites policy protected records =
  let (cpu, host, bus, identity, first) = foldl' step (0, 0, 0, 0, Nothing) records
      summary = ExecutionWriteSummary (ProtectedWriteSummary (ProtectedCpuWrites cpu) (ProtectedHostWrites host) (ProtectedBusWrites bus))
        (IdentityCpuStores identity)
   in maybe (ExecutionWritesAccepted summary) (\violation -> ExecutionWritesRejected violation summary) first
  where
    retain first record fault = case first of
      Just _ -> first
      Nothing -> Just (ExecutionWriteViolation fault record)
    step counts@(cpu, host, bus, identity, first) record = case record of
      CpuRecord{cpuAccess = CpuWrite} ->
        let preserved = storePreservesProtectedBytes protected record
            identity' = identity + if preserved == Right () then 1 else 0
            violation = case policy of
              ForbidProtectedWrites -> retain first record StrictProtectedWrite
              PreserveProtectedBytes -> either (retain first record) (const first) preserved
         in (cpu + 1, host, bus, identity', violation)
      TransferRecord{transferAccess = HostWrite} ->
        (cpu, host + 1, bus, identity, retain first record ForbiddenHostWrite)
      TransferRecord{transferAccess = ModelBusWrite} ->
        (cpu, host, bus + 1, identity, retain first record ForbiddenModelBusWrite)
      _ -> counts

judgeExecutionWrites :: Either String ExecutionWriteGate -> Either String ExecutionWriteSummary
judgeExecutionWrites result = do
  gate <- result
  case gate of
    ExecutionWritesAccepted summary -> Right summary
    ExecutionWritesRejected violation _ -> Left ("protected store rejected: " ++ show (executionWriteFault violation))

executionWriteGateJson :: ExecutionWriteGate -> Value
executionWriteGateJson gate =
  let (summary, violation) = case gate of
        ExecutionWritesAccepted value -> (value, Nothing)
        ExecutionWritesRejected fault value -> (value, Just fault)
      totals = executionTotalWrites summary
   in A.object
      [ "cpu_writes" .= protectedCpuWritesValue (protectedCpuWrites totals)
      , "protected_identity_cpu_stores" .= identityCpuStoresValue (executionIdentityCpuStores summary)
      , "host_writes" .= protectedHostWritesValue (protectedHostWrites totals)
      , "model_bus_writes" .= protectedBusWritesValue (protectedBusWrites totals)
      , "first_violation" .= fmap executionWriteViolationJson violation ]

executionWriteViolationJson :: ExecutionWriteViolation -> Value
executionWriteViolationJson (ExecutionWriteViolation fault record) = A.object
  [ "kind" .= (case fault of
      StrictProtectedWrite -> "strict-protected-write"
      ChangedProtectedByte -> "changed-protected-byte"
      UnverifiableCpuStore -> "unverifiable-cpu-store"
      ForbiddenHostWrite -> "forbidden-host-write"
      ForbiddenModelBusWrite -> "forbidden-model-bus-write" :: T.Text)
  , "record" .= case record of
      CpuRecord access address physical bytes value before order core pc instructions tick -> A.object
        [ "source" .= (case access of CpuWrite -> "cpu-write"; CpuRead -> "cpu-read"; CpuFetch -> "cpu-fetch" :: T.Text)
        , "address" .= watchAddressValue address, "physical" .= watchAddressValue physical, "size" .= watchBytesValue bytes
        , "value" .= value, "before" .= fmap B.unpack before
        , "byte_order" .= (case order of LittleEndian -> "little"; BigEndian -> "big" :: T.Text)
        , "core" .= (case core of Core0 -> 0 :: Int; Core1 -> 1), "pc" .= watchAddressValue pc
        , "instructions" .= instructionsValue instructions, "tick" .= tickValue tick ]
      TransferRecord access address physical bytes -> A.object
        [ "source" .= (case access of HostRead -> "host-read"; HostWrite -> "host-write"; ModelBusRead -> "model-bus-read"; ModelBusWrite -> "model-bus-write" :: T.Text)
        , "address" .= watchAddressValue address, "physical" .= watchAddressValue physical, "size" .= watchBytesValue bytes ] ]

checkExecution :: WritePolicy -> WatchLimit -> [WatchRange] -> [WatchRange] -> FixedWatchAck -> FixedWatchObservation -> Either String ExecutionGate
checkExecution policy limit protected forbidden ack fixed@(FixedWatchObservation observation _) = do
  unless (not (null forbidden)) (Left "execution watch has no forbidden ranges")
  _ <- judgeExecutionWrites (checkExecutionWrites policy limit protected forbidden ack fixed)
  let fetches =
        [ ForbiddenFetch core pc address physical bytes instructions tick
        | CpuRecord CpuFetch address physical bytes _ _ _ core pc instructions tick <- watchRecords observation
        , any (\range -> overlapsWatchRange range physical bytes) forbidden ]
  pure (if null fetches then ExecutionAccepted else ExecutionRejected fetches)

checkExecutionReceipt :: WritePolicy -> WatchLimit -> [WatchRange] -> [WatchRange] -> ExecutionInterval -> RunPurpose -> Maybe WatchAddress -> Value -> Either String ExecutionGate
checkExecutionReceipt policy limit protected forbidden interval purpose controlPc value = do
  exactKeys ["schema", "enforcement", "ranges", "protected_write_ranges", "watched_ranges", "interval", "start", "end", "cores", "views", "limit"
    , "acknowledgement", "acknowledgement_error", "final_state", "state_error", "complete", "forbidden_fetches"
    , "forbidden_fetch_count", "control_pc", "expect", "pre_unload", "trajectory_failure", "problem", "write_policy", "write_observation"] value
  unless (field "schema" value == Right (toJSON (1 :: Int))
    && field "enforcement" value == Right (String "original-body-fetch-exclusion")
    && field "start" value == Right (String "successful-install")
    && field "interval" value == Right (String (executionIntervalLabel interval))
    && field "end" value == Right (String (executionIntervalLabel interval))
    && field "cores" value == Right (toJSON ([0, 1] :: [Int]))
    && field "views" value == Right (String "fixed")
    && field "limit" value == Right (toJSON (watchLimitValue limit))
    && field "complete" value == Right (Bool True))
    (Left "execution receipt interval or observer is incomplete")
  unless (purpose /= CandidateRun || (interval == BeforeUnload && controlPc == Nothing))
    (Left "candidate execution interval extends beyond unload or declares a control PC")
  unless (field "write_policy" value == Right (String (writePolicyLabel policy)))
    (Left "execution receipt write policy differs")
  declared <- field "ranges" value >>= parseRanges
  writeRanges <- field "protected_write_ranges" value >>= parseRanges
  armed <- field "watched_ranges" value >>= parseRanges
  unless (declared == forbidden && writeRanges == protected && armed == unionWatchRanges (protected ++ forbidden))
    (Left "execution receipt ranges differ")
  forM_ ["acknowledgement_error", "state_error", "trajectory_failure", "problem"] $ \name ->
    unless (field name value == Right Null) (Left "execution receipt carries an error")
  unless (field "expect" value == Right (String (case purpose of CandidateRun -> "clean"; FaultControlRun -> "refused"))
    && field "control_pc" value == Right (toJSON (watchAddressValue <$> controlPc)))
    (Left "execution receipt expectation differs")
  case interval of
    BeforeUnload -> unless (field "pre_unload" value == Right Null) (Left "pre-unload receipt contains a nested interval")
    ThroughAfterUnload -> do
      unless (purpose == FaultControlRun) (Left "only a refusal control may observe restored entries after unload")
      preUnload <- field "pre_unload" value
      _ <- checkExecutionReceipt policy limit protected forbidden BeforeUnload CandidateRun Nothing preUnload
      unless (field "acknowledgement" preUnload == field "acknowledgement" value)
        (Left "control pre-unload snapshot belongs to a different armed observer")
  ack <- field "acknowledgement" value >>= parseFixedWatchAck
  observation <- field "final_state" value >>= parseFixedWatchObservation limit
  writes <- checkExecutionWrites policy limit protected forbidden ack observation
  unless (field "write_observation" value == Right (executionWriteGateJson writes))
    (Left "execution receipt write attribution differs")
  gate <- checkExecution policy limit protected forbidden ack observation
  judgeExecution purpose controlPc (Right gate)
  let fetches = case gate of ExecutionAccepted -> []; ExecutionRejected rows -> rows
  unless (field "forbidden_fetches" value == Right (toJSON (map forbiddenFetchJson fetches))
    && field "forbidden_fetch_count" value == Right (toJSON (length fetches)))
    (Left "execution receipt fetch attribution differs")
  pure gate

judgeExecution :: RunPurpose -> Maybe WatchAddress -> Either String ExecutionGate -> Either String ()
judgeExecution purpose controlPc result = do
  gate <- result
  case (purpose, gate) of
    (CandidateRun, ExecutionAccepted) -> Right ()
    (CandidateRun, ExecutionRejected _) -> Left "candidate fetched forbidden original instructions"
    (FaultControlRun, ExecutionAccepted) -> Left "execution control observed no forbidden fetch"
    (FaultControlRun, ExecutionRejected fetches) -> case controlPc of
      Just pc | any ((== pc) . forbiddenFetchPc) fetches -> Right ()
      _ -> Left "execution control did not fetch its declared exact PC"

forbiddenFetchJson :: ForbiddenFetch -> Value
forbiddenFetchJson fetch = Object (KM.fromList
  [ ("core", toJSON (case forbiddenFetchCore fetch of Core0 -> 0 :: Int; Core1 -> 1))
  , ("pc", toJSON (watchAddressValue (forbiddenFetchPc fetch)))
  , ("address", toJSON (watchAddressValue (forbiddenFetchAddress fetch)))
  , ("physical", toJSON (watchAddressValue (forbiddenFetchPhysical fetch)))
  , ("size", toJSON (watchBytesValue (forbiddenFetchBytes fetch)))
  , ("instructions", toJSON (instructionsValue (forbiddenFetchInstructions fetch)))
  , ("tick", toJSON (tickValue (forbiddenFetchTick fetch))) ])

checkWatch :: WatchLimit -> [WatchRange] -> WatchAck -> WatchObservation -> Either String WatchGate
checkWatch limit expected ack observation = do
  unless (ackArmed ack) (Left "watch acknowledgement is not armed")
  unless (ackRanges ack == expected) (Left "watch acknowledgement ranges differ")
  unless (watchEnabled observation) (Left "watch observation is not enabled")
  unless (watchRanges observation == expected) (Left "watch observation ranges differ")
  when (watchViolation observation /= Nothing) (Left "watch reported a violation")
  unless (length (watchRecords observation) <= watchLimitValue limit) (Left "watch records exceed the declared limit")
  forM_ (watchRecords observation) $ \record ->
    unless (any (\range -> recordOverlaps range record) expected) (Left "watch record does not overlap the expected ranges")
  checkCounters (watchRecords observation)
  let summary = summarize (watchRecords observation)
  if protectedCpuWritesValue (protectedCpuWrites summary) + protectedHostWritesValue (protectedHostWrites summary) + protectedBusWritesValue (protectedBusWrites summary) == 0
    then pure WatchAcceptedClean
    else pure (WatchRejectedProtectedWrites summary)
  where
    recordOverlaps range record = case record of
      CpuRecord{cpuPhysical = physical, cpuBytes = bytes} -> overlapsWatchRange range physical bytes
      TransferRecord{transferPhysical = physical, transferBytes = bytes} -> overlapsWatchRange range physical bytes
    checkCounters records = go Nothing Nothing records
      where
        go _ _ [] = Right ()
        go previousInstructions previousTick (record : rest) = case record of
          CpuRecord{cpuInstructions = instructions, cpuTick = tick} -> do
            when (maybe False (instructions <) previousInstructions) (Left "watch instruction count regressed")
            when (maybe False (tick <) previousTick) (Left "watch tick regressed")
            go (Just instructions) (Just tick) rest
          _ -> go previousInstructions previousTick rest

summarize :: [WatchRecord] -> ProtectedWriteSummary
summarize records =
  let (cpu, host, bus) = foldl' step (0 :: Int, 0 :: Int, 0 :: Int) records
   in ProtectedWriteSummary (ProtectedCpuWrites cpu) (ProtectedHostWrites host) (ProtectedBusWrites bus)
  where
    step counts record = case record of
      CpuRecord{cpuAccess = CpuWrite} -> add 1 counts
      TransferRecord{transferAccess = HostWrite} -> add 2 counts
      TransferRecord{transferAccess = ModelBusWrite} -> add 3 counts
      _ -> counts
    add :: Int -> (Int, Int, Int) -> (Int, Int, Int)
    add kind (cpu, host, bus) = case kind of
      1 -> (cpu + 1, host, bus)
      2 -> (cpu, host + 1, bus)
      3 -> (cpu, host, bus + 1)
      _ -> (cpu, host, bus)

noProtectedWrites :: ProtectedWriteSummary
noProtectedWrites = ProtectedWriteSummary (ProtectedCpuWrites 0) (ProtectedHostWrites 0) (ProtectedBusWrites 0)

data RunPurpose = CandidateRun | FaultControlRun deriving (Eq, Show)

judgeWatch :: RunPurpose -> Either String WatchGate -> Either String WatchJudgement
judgeWatch purpose gateResult = do
  gate <- gateResult
  case (purpose, gate) of
    (CandidateRun, WatchAcceptedClean) -> Right CandidateQualified
    (CandidateRun, WatchRejectedProtectedWrites _) -> Left "clean candidate received a protected write"
    (FaultControlRun, WatchAcceptedClean) -> Left "control was accepted as a clean interval"
    (FaultControlRun, WatchRejectedProtectedWrites summary)
      | protectedCpuWritesValue (protectedCpuWrites summary) >= 1 -> Right ControlRefusedProtectedWrite
      | otherwise -> Left "control observed no cpu-write"

newtype AcceptedWrites = AcceptedWrites { acceptedWritesValue :: Word64 } deriving (Eq, Ord, Show)

acceptedWrites :: Word64 -> AcceptedWrites
acceptedWrites = AcceptedWrites
newtype AcceptedBytes = AcceptedBytes { acceptedBytesValue :: Word64 } deriving (Eq, Ord, Show)

acceptedBytes :: Word64 -> AcceptedBytes
acceptedBytes = AcceptedBytes

data MediaObservation = MediaObservation
  { mediaPresent :: Bool
  , mediaWriteProtected :: Bool
  , mediaDirtySectors :: Sectors
  , mediaStarted :: Commands
  , mediaCompleted :: Commands
  , mediaIrqs :: Irqs
  , mediaDmaBoundaries :: Boundaries
  , mediaAcceptedWrites :: AcceptedWrites
  , mediaAcceptedBytes :: AcceptedBytes
  } deriving (Eq, Show)

parseMediaObservation :: Value -> Either String MediaObservation
parseMediaObservation value = MediaObservation
  <$> (field "present" value >>= asBool)
  <*> (field "write_protected" value >>= asBool)
  <*> (Sectors <$> (field "dirty_sectors" value >>= asWord64))
  <*> (Commands <$> (field "started" value >>= asWord64))
  <*> (Commands <$> (field "completed" value >>= asWord64))
  <*> (Irqs <$> (field "irqs" value >>= asWord64))
  <*> (Boundaries <$> (field "dma_boundaries" value >>= asWord64))
  <*> (AcceptedWrites <$> (field "accepted_writes" value >>= asWord64))
  <*> (AcceptedBytes <$> (field "accepted_write_bytes" value >>= asWord64))

checkMedia :: Bool -> MediaObservation -> MediaObservation -> Either String ()
checkMedia expectedPresent before after = do
  unless (mediaPresent before == expectedPresent && mediaPresent after == expectedPresent)
    (Left "SD host presence differs from the declared scope")
  unless (mediaDirtySectors before == mediaDirtySectors after)
    (Left "SD host dirty sectors changed during the protected interval")
  unless (mediaAcceptedWrites before == mediaAcceptedWrites after && mediaAcceptedBytes before == mediaAcceptedBytes after)
    (Left "SD host accepted a write during the protected interval")
  unless (mediaStarted after >= mediaStarted before && mediaCompleted after >= mediaCompleted before
          && mediaIrqs after >= mediaIrqs before && mediaDmaBoundaries after >= mediaDmaBoundaries before)
    (Left "SD host counters regressed")
  when (mediaWriteProtected before /= mediaWriteProtected after)
    (Left "SD host write protection changed during the protected interval")
