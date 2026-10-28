{-# LANGUAGE OverloadedStrings #-}
-- | The bounded RAM-byte-preservation guard. An accepted interval preserves every protected
-- byte at each CPU store; identity CPU writes remain visible in the counters.
module Emulation.PreservationObservation
  ( PreservationRanges
  , preservationRanges
  , preservationRangesJson
  , checkPreservationRangeCount
  , PreservationCounters
  , PreservationViolation
  , PreservationObservation (preservationCounters, preservationFirstViolation)
  , PreservationAck
  , PreservationGate
  , parsePreservationAck
  , parsePreservationObservation
  , checkPreservation
  , judgePreservation
  , preservationCountersJson
  , preservationViolationJson
  ) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..), object, (.=))
import Data.Bits (shiftL)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.List (sort)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word64)
import Numeric (showHex)
import Emulation.WatchObservation
  ( Instructions
  , RunPurpose (..)
  , instructionCount
  , instructionsValue
  , WatchAddress
  , WatchJudgement (..)
  , WatchRange
  , watchAddress
  , watchAddressValue
  , watchRange
  , watchRangeBounds
  )

-- | A nonempty set of at most eight half-open physical ranges.
newtype PreservationRanges = PreservationRanges [WatchRange] deriving (Eq, Show)

maxPreservationRanges :: Int
maxPreservationRanges = 8

preservationRanges :: [WatchRange] -> Either String PreservationRanges
preservationRanges ranges
  | null ranges = Left "preservation requires at least one range"
  | length ranges > maxPreservationRanges = Left "preservation takes at most eight ranges"
  | otherwise = Right (PreservationRanges ranges)

preservationRangesJson :: PreservationRanges -> Value
preservationRangesJson (PreservationRanges ranges) = Array (V.fromList
  [ Array (V.fromList [Number (fromIntegral lo), Number (fromIntegral hi)])
  | range <- ranges, let (lo, hi) = watchRangeBounds range ])

newtype IdentityCpuWrites = IdentityCpuWrites { identityCpuWritesValue :: Word64 } deriving (Eq, Show)
newtype ChangedCpuWrites = ChangedCpuWrites { changedCpuWritesValue :: Word64 } deriving (Eq, Show)
newtype HostWrites = HostWrites { hostWritesValue :: Word64 } deriving (Eq, Show)
newtype ModelBusWrites = ModelBusWrites { modelBusWritesValue :: Word64 } deriving (Eq, Show)

data PreservationCounters = PreservationCounters
  { preservationIdentityCpuWrites :: IdentityCpuWrites
  , preservationChangedCpuWrites :: ChangedCpuWrites
  , preservationHostWrites :: HostWrites
  , preservationModelBusWrites :: ModelBusWrites
  } deriving (Eq, Show)

newtype PreservationBytes = PreservationBytes { preservationBytesValue :: Word64 } deriving (Eq, Show)
newtype PreservationCore = PreservationCore { preservationCoreValue :: Int } deriving (Eq, Show)

data PreservationSource = HostWrite | ModelBusWrite deriving (Eq, Show)

data IntegrityReason = UnresolvedMapping | UnsupportedAccess | UnreadablePreimage | CounterOverflow deriving (Eq, Show)

data PreservationViolation
  = CpuChange
      { violationCore :: PreservationCore
      , violationPc :: WatchAddress
      , violationInstructions :: Instructions
      , violationAddress :: WatchAddress
      , violationPhysical :: WatchAddress
      , violationBytes :: PreservationBytes
      , violationBefore :: B.ByteString
      , violationAfter :: B.ByteString
      }
  | UnverifiableWrite
      { violationSource :: PreservationSource
      , violationAddress :: WatchAddress
      , violationPhysical :: WatchAddress
      , violationBytes :: PreservationBytes
      }
  | IntegrityFault { violationReason :: IntegrityReason }
  deriving (Eq, Show)

data PreservationObservation = PreservationObservation
  { preservationEnabled :: Bool
  , preservationObservedRanges :: Maybe PreservationRanges
  , preservationCounters :: PreservationCounters
  , preservationFirstViolation :: Maybe PreservationViolation
  } deriving (Eq, Show)

data PreservationAck = PreservationAck
  { preservationArmed :: Bool
  , preservationAckRanges :: PreservationRanges
  } deriving (Eq, Show)

-- | An accepted interval, or its first violation with the counters so far.
data PreservationGate
  = PreservationAccepted PreservationCounters
  | PreservationRejected PreservationViolation PreservationCounters
  deriving (Eq, Show)

field :: String -> Value -> Either String Value
field name (Object objectValue) = maybe (Left ("missing field " ++ name)) Right (KM.lookup (K.fromString name) objectValue)
field _ _ = Left "preservation value is not an object"

exactKeys :: [String] -> Value -> Either String ()
exactKeys expected (Object objectValue) =
  let actual = sort (map (T.unpack . K.toText) (KM.keys objectValue))
   in unless (actual == sort expected) (Left "preservation fields differ from the pinned schema")
exactKeys _ _ = Left "preservation value is not an object"

asText :: Value -> Either String String
asText (String text) = Right (T.unpack text)
asText _ = Left "preservation field is not text"

asBool :: Value -> Either String Bool
asBool (Bool value) = Right value
asBool _ = Left "preservation flag is not Boolean"

asWord64 :: Value -> Either String Word64
asWord64 (Number number) = maybe (Left "preservation number is out of range") Right (toBoundedInteger number)
asWord64 _ = Left "preservation number is not an integer"

asWord32 :: Value -> Either String Word32
asWord32 value = do
  number <- asWord64 value
  if number <= fromIntegral (maxBound :: Word32) then Right (fromIntegral number) else Left "preservation address exceeds 32 bits"

asArray :: Value -> Either String [Value]
asArray (Array values) = Right (V.toList values)
asArray _ = Left "preservation field is not an array"

parseRange :: Value -> Either String WatchRange
parseRange entry = do
  pair <- asArray entry
  case pair of
    [low, high] -> do
      lowAddress <- asWord32 low
      highAddress <- asWord32 high
      watchRange (watchAddress lowAddress) (watchAddress highAddress)
    _ -> Left "preservation range must be a low/high pair"

checkPreservationRangeCount :: Int -> Either String ()
checkPreservationRangeCount count
  | count < 1 = Left "preservation requires at least one range"
  | count > maxPreservationRanges = Left "preservation takes at most eight ranges"
  | otherwise = Right ()

checkRangeCount :: [Value] -> Either String ()
checkRangeCount entries = checkPreservationRangeCount (length entries)

parseRanges :: Value -> Either String PreservationRanges
parseRanges value = do
  entries <- asArray value
  checkRangeCount entries
  preservationRanges =<< mapM parseRange entries

-- | A disabled state carries an empty range list.
parseObservedRanges :: Value -> Either String (Maybe PreservationRanges)
parseObservedRanges value = do
  entries <- asArray value
  if null entries
    then Right Nothing
    else do
      checkRangeCount entries
      Just <$> (preservationRanges =<< mapM parseRange entries)

checkExtent :: WatchAddress -> Word64 -> Either String ()
checkExtent address size =
  let start = fromIntegral (watchAddressValue address) :: Word64
   in unless (start + size <= 1 `shiftL` 32) (Left "preservation access exceeds the 32-bit address space")

parseSize :: Word64 -> Word64 -> Value -> Either String PreservationBytes
parseSize low high value = do
  bytes <- asWord64 value
  if bytes >= low && bytes <= high then Right (PreservationBytes bytes) else Left "preservation access width out of range"

parseCore :: Value -> Either String PreservationCore
parseCore value = do
  number <- asWord64 value
  if number <= 1 then Right (PreservationCore (fromIntegral number)) else Left "preservation core is not 0 or 1"

parseHex :: Word64 -> Value -> Either String B.ByteString
parseHex size (String text) =
  let digits = T.unpack text
   in if length digits /= fromIntegral (2 * size)
        then Left "preservation hex length differs from size"
        else B.pack <$> pairs digits
  where
    pairs [] = Right []
    pairs (high : low : rest) = do
      byte <- nibble high low
      (byte :) <$> pairs rest
    pairs _ = Left "preservation hex has an odd digit count"
    nibble high low = do
      upper <- digit high
      lower <- digit low
      Right (upper * 16 + lower)
    digit character = case character of
      '0' -> Right 0
      '1' -> Right 1
      '2' -> Right 2
      '3' -> Right 3
      '4' -> Right 4
      '5' -> Right 5
      '6' -> Right 6
      '7' -> Right 7
      '8' -> Right 8
      '9' -> Right 9
      'a' -> Right 10
      'b' -> Right 11
      'c' -> Right 12
      'd' -> Right 13
      'e' -> Right 14
      'f' -> Right 15
      _ -> Left "preservation hex is not lowercase"
parseHex _ _ = Left "preservation hex is not text"

parseCpuChange :: Value -> Either String PreservationViolation
parseCpuChange value = do
  exactKeys ["kind", "core", "pc", "instructions", "address", "physical", "size", "before", "after"] value
  core <- field "core" value >>= parseCore
  pc <- field "pc" value >>= asWord32
  instructions <- instructionCount <$> (field "instructions" value >>= asWord64)
  address <- field "address" value >>= asWord32
  physical <- field "physical" value >>= asWord32
  bytes <- field "size" value >>= parseSize 1 8
  let size = preservationBytesValue bytes
  before <- field "before" value >>= parseHex size
  after <- field "after" value >>= parseHex size
  checkExtent (watchAddress address) size
  checkExtent (watchAddress physical) size
  when (before == after) (Left "preservation cpu-change does not change a protected byte")
  pure (CpuChange core (watchAddress pc) instructions (watchAddress address) (watchAddress physical) bytes before after)

parseUnverifiable :: Value -> Either String PreservationViolation
parseUnverifiable value = do
  exactKeys ["kind", "source", "address", "physical", "size"] value
  source <- field "source" value >>= asText
  writeSource <- case source of
    "host-write" -> Right HostWrite
    "model-bus-write" -> Right ModelBusWrite
    _ -> Left "preservation unverifiable source is not host-write or model-bus-write"
  address <- field "address" value >>= asWord32
  physical <- field "physical" value >>= asWord32
  bytes <- field "size" value >>= parseSize 1 (1 `shiftL` 32)
  checkExtent (watchAddress address) (preservationBytesValue bytes)
  checkExtent (watchAddress physical) (preservationBytesValue bytes)
  pure (UnverifiableWrite writeSource (watchAddress address) (watchAddress physical) bytes)

parseIntegrityFault :: Value -> Either String PreservationViolation
parseIntegrityFault value = do
  exactKeys ["kind", "reason"] value
  reason <- field "reason" value >>= asText
  IntegrityFault <$> case reason of
    "unresolved-mapping" -> Right UnresolvedMapping
    "unsupported-access" -> Right UnsupportedAccess
    "unreadable-preimage" -> Right UnreadablePreimage
    "counter-overflow" -> Right CounterOverflow
    _ -> Left "preservation integrity reason is not pinned"

parseViolationValue :: Value -> Either String (Maybe PreservationViolation)
parseViolationValue Null = Right Nothing
parseViolationValue value = do
  kind <- field "kind" value >>= asText
  Just <$> case kind of
    "cpu-change" -> parseCpuChange value
    "unverifiable-write" -> parseUnverifiable value
    "integrity-fault" -> parseIntegrityFault value
    _ -> Left "preservation violation kind is not pinned"

parsePreservationAck :: Value -> Either String PreservationAck
parsePreservationAck value = do
  exactKeys ["armed", "ranges"] value
  PreservationAck
    <$> (field "armed" value >>= asBool)
    <*> (field "ranges" value >>= parseRanges)

parsePreservationObservation :: Value -> Either String PreservationObservation
parsePreservationObservation value = do
  exactKeys
    [ "enabled", "ranges", "identity_cpu_writes", "changed_cpu_writes"
    , "host_writes", "model_bus_writes", "violation" ]
    value
  enabled <- field "enabled" value >>= asBool
  ranges <- field "ranges" value >>= parseObservedRanges
  counters <- PreservationCounters
    <$> (IdentityCpuWrites <$> (field "identity_cpu_writes" value >>= asWord64))
    <*> (ChangedCpuWrites <$> (field "changed_cpu_writes" value >>= asWord64))
    <*> (HostWrites <$> (field "host_writes" value >>= asWord64))
    <*> (ModelBusWrites <$> (field "model_bus_writes" value >>= asWord64))
  violation <- field "violation" value >>= parseViolationValue
  let zeroCounters = identityCpuWritesValue (preservationIdentityCpuWrites counters) == 0
        && changedCpuWritesValue (preservationChangedCpuWrites counters) == 0
        && hostWritesValue (preservationHostWrites counters) == 0
        && modelBusWritesValue (preservationModelBusWrites counters) == 0
  case (enabled, ranges) of
    (True, Just _) -> pure ()
    (False, Nothing) ->
      unless (zeroCounters && violation == Nothing)
        (Left "disabled preservation state carries counters or a violation")
    _ -> Left "preservation enabled flag and ranges disagree"
  pure (PreservationObservation enabled ranges counters violation)

-- | The declared physical protection; a violation extent is the protected intersection of its store.
checkPreservation :: PreservationRanges -> PreservationAck -> PreservationObservation -> Either String PreservationGate
checkPreservation expected ack observation = do
  unless (preservationArmed ack) (Left "preservation acknowledgement is not armed")
  unless (preservationAckRanges ack == expected) (Left "preservation acknowledgement ranges differ")
  unless (preservationEnabled observation) (Left "preservation observation is not enabled")
  unless (preservationObservedRanges observation == Just expected) (Left "preservation observation ranges differ")
  let PreservationRanges ranges = expected
      counters = preservationCounters observation
      changed = changedCpuWritesValue (preservationChangedCpuWrites counters)
      hosts = hostWritesValue (preservationHostWrites counters)
      buses = modelBusWritesValue (preservationModelBusWrites counters)
  case preservationFirstViolation observation of
    Nothing -> do
      unless (changed == 0 && hosts == 0 && buses == 0)
        (Left "preservation state reports writes without a first violation")
      pure (PreservationAccepted counters)
    Just (IntegrityFault reason) -> Left ("preservation integrity fault: " ++ integrityReasonLabel reason)
    Just violation@CpuChange{} -> do
      unless (changed > 0) (Left "preservation cpu-change has no changed cpu-write")
      unless (contains violation ranges) (Left "preservation cpu-change lies outside the protected ranges")
      pure (PreservationRejected violation counters)
    Just violation@UnverifiableWrite{} -> do
      let observed = case violationSource violation of
            HostWrite -> hosts
            ModelBusWrite -> buses
          source = case violationSource violation of
            HostWrite -> "host"
            ModelBusWrite -> "model-bus"
      unless (observed > 0) (Left ("preservation " ++ source ++ " write has no matching counter"))
      unless (any (overlaps violation) ranges) (Left "preservation unverifiable write does not overlap the protected ranges")
      pure (PreservationRejected violation counters)
  where
    extent violation = (violationPhysical violation, preservationBytesValue (violationBytes violation))
    contains violation ranges =
      let (address, size) = extent violation
          start = fromIntegral (watchAddressValue address) :: Word64
       in rangeCovered start (start + size) ranges
    overlaps violation range =
      let (low, high) = watchRangeBounds range
          (address, size) = extent violation
          start = fromIntegral (watchAddressValue address) :: Word64
       in start < high && low < start + size

rangeCovered :: Word64 -> Word64 -> [WatchRange] -> Bool
rangeCovered start end ranges =
  any (\(low, high) -> low <= start && end <= high) (mergedRanges ranges)

mergedRanges :: [WatchRange] -> [(Word64, Word64)]
mergedRanges = reverse . foldl step [] . sort . map watchRangeBounds
  where
    step [] interval = [interval]
    step acc@((low, high) : rest) (low', high')
      | low' <= high = (low, max high high') : rest
      | otherwise = (low', high') : acc

integrityReasonLabel :: IntegrityReason -> String
integrityReasonLabel reason = case reason of
  UnresolvedMapping -> "unresolved-mapping"
  UnsupportedAccess -> "unsupported-access"
  UnreadablePreimage -> "unreadable-preimage"
  CounterOverflow -> "counter-overflow"

judgePreservation :: RunPurpose -> Either String PreservationGate -> Either String WatchJudgement
judgePreservation purpose gateResult = do
  gate <- gateResult
  case (purpose, gate) of
    (CandidateRun, PreservationAccepted _) -> Right CandidateQualified
    (CandidateRun, PreservationRejected _ _) -> Left "protected memory changed during a clean candidate interval"
    (FaultControlRun, PreservationAccepted _) -> Left "control was accepted as a preserved interval"
    (FaultControlRun, PreservationRejected CpuChange{} _) -> Right ControlRefusedProtectedWrite
    (FaultControlRun, PreservationRejected _ _) -> Left "control observed no changing cpu-write"

preservationCountersJson :: PreservationCounters -> Value
preservationCountersJson counters = object
  [ "identity_cpu_writes" .= identityCpuWritesValue (preservationIdentityCpuWrites counters)
  , "changed_cpu_writes" .= changedCpuWritesValue (preservationChangedCpuWrites counters)
  , "host_writes" .= hostWritesValue (preservationHostWrites counters)
  , "model_bus_writes" .= modelBusWritesValue (preservationModelBusWrites counters) ]

preservationViolationJson :: PreservationViolation -> Value
preservationViolationJson violation = case violation of
  CpuChange core pc instructions address physical bytes before after -> object
    [ "kind" .= ("cpu-change" :: T.Text)
    , "core" .= preservationCoreValue core
    , "pc" .= watchAddressValue pc
    , "instructions" .= instructionsValue instructions
    , "address" .= watchAddressValue address
    , "physical" .= watchAddressValue physical
    , "size" .= preservationBytesValue bytes
    , "before" .= hexBytes before
    , "after" .= hexBytes after ]
  UnverifiableWrite source address physical bytes -> object
    [ "kind" .= ("unverifiable-write" :: T.Text)
    , "source" .= sourceLabel source
    , "address" .= watchAddressValue address
    , "physical" .= watchAddressValue physical
    , "size" .= preservationBytesValue bytes ]
  IntegrityFault reason -> object
    [ "kind" .= ("integrity-fault" :: T.Text)
    , "reason" .= (T.pack (integrityReasonLabel reason)) ]

sourceLabel :: PreservationSource -> T.Text
sourceLabel HostWrite = "host-write"
sourceLabel ModelBusWrite = "model-bus-write"

hexBytes :: B.ByteString -> T.Text
hexBytes = T.concat . map byte . B.unpack
  where
    byte value =
      let digits = showHex value ""
       in T.pack (if length digits == 1 then '0' : digits else digits)
