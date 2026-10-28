{-# LANGUAGE OverloadedStrings #-}
-- | Typed, fail-closed parsing and checking of FujiMod memory-watch records
-- and SD host state. Missing, malformed, overflowing or inconsistent input is
-- an error, never an empty success.
module Emulation.WatchObservation
  ( WatchAddress (..)
  , WatchRange
  , watchRange
  , watchRangeBounds
  , WatchLimit (..)
  , WatchSource (..)
  , WatchRecord (..)
  , WatchObservation (..)
  , WatchAck (..)
  , WatchPolicy (..)
  , WatchOutcome (..)
  , parseWatchAck
  , parseWatchObservation
  , checkWatch
  , MediaObservation (..)
  , parseMediaObservation
  , checkMedia
  ) where

import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Scientific (floatingOrInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word64)

newtype WatchAddress = WatchAddress { watchAddressValue :: Word32 } deriving (Eq, Ord, Show)

data WatchRange = WatchRange { watchRangeLow :: WatchAddress, watchRangeHigh :: WatchAddress } deriving (Eq, Show)

watchRange :: WatchAddress -> WatchAddress -> Either String WatchRange
watchRange low high
  | watchAddressValue low < watchAddressValue high = Right (WatchRange low high)
  | otherwise = Left "watch range must be increasing"

watchRangeBounds :: WatchRange -> (Word64, Word64)
watchRangeBounds (WatchRange (WatchAddress low) (WatchAddress high)) = (fromIntegral low, fromIntegral high)

newtype WatchLimit = WatchLimit { watchLimitRecords :: Int } deriving (Eq, Show)

data WatchSource = SourceWrite | SourceRead | SourceFetch deriving (Eq, Show)

data WatchRecord = WatchRecord
  { recordSource :: WatchSource
  , recordAddress :: WatchAddress
  , recordPhysical :: WatchAddress
  , recordSize :: Int
  , recordValue :: Maybe Word64
  , recordBefore :: Maybe B.ByteString
  , recordCore :: Int
  , recordPc :: WatchAddress
  , recordInstructions :: Word64
  , recordTick :: Word64
  } deriving (Eq, Show)

data WatchObservation = WatchObservation
  { watchEnabled :: Bool
  , watchRanges :: [WatchRange]
  , watchRecords :: [WatchRecord]
  , watchViolation :: Maybe String
  } deriving (Eq, Show)

data WatchAck = WatchAck { ackArmed :: Bool, ackRanges :: [WatchRange] } deriving (Eq, Show)

data WatchPolicy = CleanWatch WatchLimit [WatchRange] | RefusedWatch WatchLimit [WatchRange] deriving (Eq, Show)

data WatchOutcome = WatchClean | WatchControlRefused Int deriving (Eq, Show)

field :: String -> Value -> Either String Value
field name (Object object) = maybe (Left ("missing field " ++ name)) Right (KM.lookup (K.fromString name) object)
field _ _ = Left "watch value is not an object"

optionalField :: String -> Value -> Maybe Value
optionalField name (Object object) = KM.lookup (K.fromString name) object
optionalField _ _ = Nothing

asWord64 :: Value -> Either String Word64
asWord64 (Number number) = case floatingOrInteger number :: Either Double Integer of
  Right value | value >= 0 && value <= fromIntegral (maxBound :: Word64) -> Right (fromIntegral value)
  _ -> Left "watch number is out of range"
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

-- The range parser needs the two parsed addresses; keep it explicit.
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

parseSource :: Value -> Either String WatchSource
parseSource (String "cpu-write") = Right SourceWrite
parseSource (String "cpu-read") = Right SourceRead
parseSource (String "cpu-fetch") = Right SourceFetch
parseSource _ = Left "unknown watch record source"

parseBefore :: Value -> Either String (Maybe B.ByteString)
parseBefore Null = Right Nothing
parseBefore value = do
  bytes <- asArray value
  Just . B.pack <$> forM bytes (\byte -> do
    number <- asInt byte
    if number <= 255 then Right (fromIntegral number) else Left "watch before byte out of range")

parseValue :: Value -> Either String (Maybe Word64)
parseValue Null = Right Nothing
parseValue (String text) = Just <$> parseHexWord64 (T.unpack text)
parseValue value = Just <$> asWord64 value

parseHexWord64 :: String -> Either String Word64
parseHexWord64 ('0' : 'x' : digits)
  | not (null digits) = foldM step 0 digits
  | otherwise = Left "watch value is not hex"
parseHexWord64 _ = Left "watch value is not 0x-prefixed hex"

step :: Word64 -> Char -> Either String Word64
step acc character = do
  digit <- hexDigit character
  pure (acc * 16 + fromIntegral digit)

hexDigit :: Char -> Either String Word8
hexDigit character
  | character >= '0' && character <= '9' = Right (fromIntegral (fromEnum character - fromEnum '0'))
  | character >= 'a' && character <= 'f' = Right (fromIntegral (fromEnum character - fromEnum 'a' + 10))
  | character >= 'A' && character <= 'F' = Right (fromIntegral (fromEnum character - fromEnum 'A' + 10))
  | otherwise = Left "watch value is not hex"

foldM :: (b -> a -> Either String b) -> b -> [a] -> Either String b
foldM _ acc [] = Right acc
foldM stepFunction acc (value : rest) = stepFunction acc value >>= \next -> foldM stepFunction next rest

parseRecord :: Value -> Either String WatchRecord
parseRecord value = do
  source <- field "source" value >>= parseSource
  address <- field "address" value >>= asWord32
  physical <- field "physical" value >>= asWord32
  size <- field "size" value >>= asInt
  unless (size >= 1 && size <= 16) (Left "watch record size is outside 1..16")
  before <- field "before" value >>= parseBefore
  when (source /= SourceFetch && before == Nothing) (Left "watch record lacks its before bytes")
  case before of
    Just bytes -> unless (B.length bytes == size) (Left "watch before length differs from size")
    Nothing -> pure ()
  recordValue <- field "value" value >>= parseValue
  core <- field "core" value >>= asInt
  unless (core >= 0 && core <= 1) (Left "watch record core is outside 0..1")
  pc <- field "pc" value >>= asWord32
  instructions <- field "instructions" value >>= asWord64
  tick <- field "tick" value >>= asWord64
  pure (WatchRecord source (WatchAddress address) (WatchAddress physical) size recordValue before core (WatchAddress pc) instructions tick)

parseWatchAck :: Value -> Either String WatchAck
parseWatchAck value = WatchAck
  <$> (field "armed" value >>= asBool)
  <*> (field "ranges" value >>= parseRanges)

parseWatchObservation :: WatchLimit -> Value -> Either String WatchObservation
parseWatchObservation limit value = do
  enabled <- field "enabled" value >>= asBool
  ranges <- field "ranges" value >>= parseRanges
  records <- field "records" value >>= asArray >>= mapM parseRecord
  violation <- case field "violation" value of
    Right Null -> Right Nothing
    Right (String text) -> Right (Just (show text))
    Right _ -> Left "watch violation is not text or null"
    Left problem -> Left problem
  unless (length records <= watchLimitRecords limit) (Left "watch records exceed the declared limit")
  pure (WatchObservation enabled ranges records violation)

checkWatch :: WatchPolicy -> WatchAck -> WatchObservation -> Either String WatchOutcome
checkWatch policy ack observation = do
  let (limit, expected) = case policy of
        CleanWatch limit ranges -> (limit, ranges)
        RefusedWatch limit ranges -> (limit, ranges)
  unless (ackArmed ack) (Left "watch acknowledgement is not armed")
  unless (ackRanges ack == expected) (Left "watch acknowledgement ranges differ")
  unless (watchEnabled observation) (Left "watch observation is not enabled")
  unless (watchRanges observation == expected) (Left "watch observation ranges differ")
  when (watchViolation observation /= Nothing) (Left "watch reported a violation")
  unless (length (watchRecords observation) <= watchLimitRecords limit) (Left "watch records exceed the declared limit")
  forM_ (watchRecords observation) $ \record ->
    unless (any (`contains` record) expected) (Left "watch record lies outside the expected ranges")
  case policy of
    CleanWatch _ _ -> do
      when (not (null [record | record <- watchRecords observation, recordSource record == SourceWrite]))
        (Left "protected range received a write during the clean interval")
      pure WatchClean
    RefusedWatch _ _ -> case [record | record <- watchRecords observation, recordSource record == SourceWrite] of
      [] -> Left "control watch observed no protected write"
      writes -> pure (WatchControlRefused (length writes))
  where
    contains range record =
      let (low, high) = watchRangeBounds range
          (WatchAddress address) = recordPhysical record
          start = fromIntegral address :: Word64
       in start >= low && start + fromIntegral (recordSize record) <= high

forM_ :: [a] -> (a -> Either String ()) -> Either String ()
forM_ values action = foldr (\value rest -> action value >> rest) (Right ()) values

data MediaObservation = MediaObservation
  { mediaPresent :: Bool
  , mediaWriteProtected :: Bool
  , mediaDirtySectors :: Word64
  , mediaStarted :: Word64
  , mediaCompleted :: Word64
  , mediaIrqs :: Word64
  , mediaDmaBoundaries :: Word64
  , mediaAcceptedWrites :: Word64
  , mediaAcceptedBytes :: Word64
  } deriving (Eq, Show)

parseMediaObservation :: Value -> Either String MediaObservation
parseMediaObservation value = MediaObservation
  <$> (field "present" value >>= asBool)
  <*> (field "write_protected" value >>= asBool)
  <*> (field "dirty_sectors" value >>= asWord64)
  <*> (field "started" value >>= asWord64)
  <*> (field "completed" value >>= asWord64)
  <*> (field "irqs" value >>= asWord64)
  <*> (field "dma_boundaries" value >>= asWord64)
  <*> (field "accepted_writes" value >>= asWord64)
  <*> (field "accepted_write_bytes" value >>= asWord64)

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
