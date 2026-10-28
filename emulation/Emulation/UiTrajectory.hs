{-# LANGUAGE OverloadedStrings #-}
-- | Trajectory files for the emulator e2e: menu segments, the guest windows read after them
-- and the code sites traced while they run.
module Emulation.UiTrajectory
  ( MenuState (..)
  , ReadAddress (..)
  , FilmRow (..)
  , filmStoredCode
  , Step (..)
  , Segment (..)
  , TraceSite (..)
  , Trajectories (..)
  , TrajectoryExpect (..)
  , expectLabel
  , SegmentName
  , segmentName
  , segmentNameText
  , ElapsedSeconds
  , elapsedSeconds
  , elapsedSecondsValue
  , SegmentResult (..)
  , SegmentReceipt (..)
  , decodeSegmentReceipts
  , phaseNames
  , decodeTrajectories
  , variantMismatches
  ) where

import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), eitherDecodeStrict', object, withArray, withObject, withScientific, withText, (.!=), (.:), (.:?), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser)
import qualified Data.ByteString as B
import Data.Char (isAlphaNum)
import Data.Foldable (toList)
import Data.List (nub, sort, stripPrefix)
import qualified Data.Text as T
import Data.Scientific (toRealFloat)
import Data.Word (Word32)
import Emulation.WatchObservation (WatchRange, watchAddress, watchRange)
import EmulatorUsb (KeyPulse (..), KeyTiming (..), ModeDial (..), SettleBudget (..), defaultKeyTiming, modeDialName)
import Numeric (readHex)

data MenuState = MenuState Int Int Int deriving (Eq, Show)

-- | A read window's start: a fixed guest address, or a symbol of the loaded build and a
-- byte offset into it. The build is the relinked runtime for a RAM push and the candidate's
-- linked kernel for a flash candidate.
data ReadAddress = FixedAddress Word32 | RuntimeSymbol String Word32 deriving (Eq, Show)

-- | A logical row of the film-simulation list.
data FilmRow = Provia | Velvia | Astia | Monochrome | Sepia | ProNegStd | ProNegHi | WarmStandard | RecipeB
  deriving (Bounded, Enum, Eq, Show)

instance FromJSON FilmRow where
  parseJSON value = do
    row <- parseJSON value
    if row >= fromEnum (minBound :: FilmRow) && row <= fromEnum (maxBound :: FilmRow)
      then pure (toEnum row)
      else fail "film_row must be a logical film row from 0 through 8"

-- | The film code the ordinary image stores for a row.
filmStoredCode :: FilmRow -> Word32
filmStoredCode row = case row of
  Provia -> 0
  Velvia -> 3
  Astia -> 1
  Monochrome -> 11
  Sepia -> 12
  ProNegStd -> 9
  ProNegHi -> 10
  WarmStandard -> 13
  RecipeB -> 14

-- | One key press and the menu state required (or, in an observing segment, recorded) after it.
-- The press timing is the only part of a step a FujiMod trajectory variant may change: a
-- fifth element {"hold_instructions", "release_instructions", "settle_instructions"} overrides
-- the segment's key pulse and the bridge's settle cap for this key.
data Step = Step { stepKey :: String, stepExpected :: MenuState, stepTiming :: KeyTiming } deriving (Eq, Show)

instance FromJSON Step where
  parseJSON = withArray "step" $ \items -> case toList items of
    [key, menu, setting, cursor] -> step key menu setting cursor defaultKeyTiming
    [key, menu, setting, cursor, timing] -> step key menu setting cursor =<< withObject "step timing" keyTiming timing
    _ -> fail "a step is [key, menu, setting, cursor] with an optional timing object"
    where
      step key menu setting cursor timing =
        Step <$> parseJSON key <*> (MenuState <$> parseJSON menu <*> parseJSON setting <*> parseJSON cursor) <*> pure timing
      keyTiming timing = do
        unless (all (`elem` ["hold_instructions", "release_instructions", "settle_instructions"]) (KM.keys timing))
          (fail "step timing takes only hold_instructions, release_instructions and settle_instructions")
        pulse <- if KM.member "hold_instructions" timing || KM.member "release_instructions" timing
          then Just <$> keyPulse timing
          else pure Nothing
        settle <- timing .:? "settle_instructions"
        when (settle == Just 0) (fail "settle_instructions must be positive")
        pure (KeyTiming pulse (SettleBudget <$> settle))

-- | One trajectory segment: its phase name, the mode-dial detent set before its first key, the
-- steps, the message ids the menu message drawer is entered with during the segment (none when
-- absent), and an optional capture label taken after the last key. An observing segment records
-- each key's actual state instead of requiring the expected one; `reads` are guest RAM windows
-- recorded after the last key, and `film_row` is the row whose stored film code the segment
-- requires after its last key. `key_pulse` gives every key of the segment an explicit hold and
-- release cap instead of the bridge's per-class default.
data Segment = Segment
  { segmentPhase :: T.Text
  , segmentModeDial :: Maybe ModeDial
  , segmentCapture :: Maybe String
  , segmentSteps :: [Step]
  , segmentMessages :: [Word32]
  , segmentObserve :: Bool
  , segmentReads :: [(String, ReadAddress, Int)]
  , segmentRequires :: [T.Text]
  , segmentFilmRow :: Maybe FilmRow
  , segmentKeyPulse :: Maybe KeyPulse
  }

instance FromJSON Segment where
  parseJSON = withObject "segment" $ \segment -> Segment
    <$> segment .: "phase"
    <*> (traverse modeDial =<< segment .:? "mode_dial")
    <*> segment .:? "capture"
    <*> segment .: "steps"
    <*> segment .:? "messages" .!= []
    <*> segment .:? "observe" .!= False
    <*> (mapM window =<< segment .:? "reads" .!= [])
    <*> segment .:? "requires" .!= []
    <*> segment .:? "film_row"
    <*> (traverse keyPulse =<< segment .:? "key_pulse")
    where
      modeDial position = case lookup (T.unpack position) [(modeDialName p, p) | p <- [minBound .. maxBound]] of
        Just p -> pure p
        Nothing -> fail ("unsupported mode-dial position: " ++ T.unpack position)
      window (label, address, len) = case T.unpack address of
        text | Just rest <- stripPrefix "runtime:" text, not (null rest) -> do
                 let (name, suffix) = break (== '+') rest
                     digits = dropWhile (== '+') suffix
                 when (null name) (fail ("read address names no runtime symbol: " ++ text))
                 offset <- case digits of
                   "" -> pure 0
                   _ -> maybe (fail ("read address offset is not hex: " ++ text)) pure (hexValue digits)
                 pure (label, RuntimeSymbol name offset, len)
             | Just value <- hexValue text -> pure (label, FixedAddress value, len)
             | otherwise -> fail ("read address must be hex without 0x or runtime:SYMBOL+0xOFFSET: " ++ text)
      hexValue digits = case readHex (case stripPrefix "0x" digits of Just rest -> rest; Nothing -> digits) of
        [(value, "")] -> Just value
        _ -> Nothing

keyPulse :: Object -> Parser KeyPulse
keyPulse budget = do
  pulse <- KeyPulse <$> budget .: "hold_instructions" <*> budget .: "release_instructions"
  when (keyPulseHold pulse == 0 || keyPulseRelease pulse == 0) (fail "key_pulse budgets must be positive")
  pure pulse

-- | An A32 instruction address whose every entry the bridge records with r0-r3, under a label.
data TraceSite = TraceSite { traceSiteAddress :: Word32, traceSiteLabel :: String } deriving (Eq, Show)

-- | Labels the e2e arms itself.
reservedLabels :: [String]
reservedLabels = ["menu-message", "d3-hook"]

instance FromJSON TraceSite where
  parseJSON value = do
    (address, label) <- parseJSON value
    site <- case readHex (T.unpack address) of
      [(word, "")] | T.length address == 8 -> pure word
      _ -> fail ("trace site address must be eight hex digits without 0x: " ++ T.unpack address)
    when (site `mod` 4 /= 0) (fail ("trace site address must be word aligned: " ++ T.unpack address))
    when (null label || label `elem` reservedLabels) (fail ("trace site label is empty or reserved: " ++ label))
    pure (TraceSite site label)

data TrajectoryExpect = ExpectClean | ExpectRefused deriving (Eq, Show)

expectLabel :: TrajectoryExpect -> T.Text
expectLabel ExpectClean = "clean"
expectLabel ExpectRefused = "refused"

newtype SegmentName = SegmentName T.Text deriving (Eq, Show)

segmentName :: T.Text -> Either String SegmentName
segmentName text
  | T.null text = Left "segment name is empty"
  | not (T.all (\c -> isAlphaNum c || c == '-' || c == '_') text) = Left "segment name is not a judgeable token"
  | otherwise = Right (SegmentName text)

segmentNameText :: SegmentName -> T.Text
segmentNameText (SegmentName text) = text

instance ToJSON SegmentName where
  toJSON = String . segmentNameText

instance FromJSON SegmentName where
  parseJSON = withText "segment name" (either fail pure . segmentName)

newtype ElapsedSeconds = ElapsedSeconds Double deriving (Eq, Show)

elapsedSeconds :: Double -> Either String ElapsedSeconds
elapsedSeconds value
  | isNaN value || isInfinite value = Left "elapsed seconds must be finite"
  | value < 0 = Left "elapsed seconds must not be negative"
  | otherwise = Right (ElapsedSeconds value)

elapsedSecondsValue :: ElapsedSeconds -> Double
elapsedSecondsValue (ElapsedSeconds value) = value

instance ToJSON ElapsedSeconds where
  toJSON = Number . toRealFloat . toRational . elapsedSecondsValue

instance FromJSON ElapsedSeconds where
  parseJSON = withScientific "elapsed seconds" (either fail pure . elapsedSeconds . toRealFloat)

data SegmentResult = SegmentPassed | SegmentFailed T.Text deriving (Eq, Show)

resultWord :: SegmentResult -> T.Text
resultWord SegmentPassed = "passed"
resultWord (SegmentFailed _) = "failed"

resultMessage :: SegmentResult -> Maybe T.Text
resultMessage SegmentPassed = Nothing
resultMessage (SegmentFailed text) = Just text

data SegmentReceipt = SegmentReceipt
  { segmentReceiptName :: SegmentName
  , segmentReceiptSeconds :: ElapsedSeconds
  , segmentReceiptResult :: SegmentResult
  } deriving (Eq, Show)

instance ToJSON SegmentReceipt where
  toJSON receipt = object
    [ "segment" .= segmentReceiptName receipt
    , "seconds" .= segmentReceiptSeconds receipt
    , "result" .= resultWord (segmentReceiptResult receipt)
    , "message" .= resultMessage (segmentReceiptResult receipt) ]

instance FromJSON SegmentReceipt where
  parseJSON = withObject "segment receipt" $ \value -> do
    unless (sort (KM.keys value) == sort (map K.fromString ["segment", "seconds", "result", "message"]))
      (fail "a segment receipt takes exactly segment, seconds, result and message")
    name <- value .: "segment"
    seconds <- value .: "seconds"
    word <- value .: "result"
    message <- value .: "message"
    result <- case (word, message) of
      ("passed", Nothing) -> pure SegmentPassed
      ("passed", Just _) -> fail "a passed segment carries no message"
      ("failed", Just text) -> pure (SegmentFailed text)
      ("failed", Nothing) -> fail "a failed segment carries a message"
      (other, _) -> fail ("unknown segment result: " ++ T.unpack other)
    pure (SegmentReceipt name seconds result)

decodeSegmentReceipts :: B.ByteString -> Either String [SegmentReceipt]
decodeSegmentReceipts = eitherDecodeStrict'

data Trajectories = Trajectories
  { beforeUnload :: [Segment]
  , afterUnload :: [Segment]
  , trajectoryTraceSites :: [TraceSite]
  , trajectoryProtectedWrites :: [WatchRange]
  , trajectoryExpect :: TrajectoryExpect
  }

instance FromJSON Trajectories where
  parseJSON = withObject "trajectories" $ \document -> do
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported trajectory schema")
    sites <- document .:? "trace_sites" .!= []
    unless (distinct (map traceSiteAddress sites) && distinct (map traceSiteLabel sites))
      (fail "trace sites must have distinct addresses and labels")
    ranges <- mapM range =<< document .:? "protected_writes" .!= []
    expectText <- document .:? "expect" .!= ("clean" :: T.Text)
    expect <- case expectText of
      "clean" -> pure ExpectClean
      "refused" -> pure ExpectRefused
      _ -> fail "expect must be clean or refused"
    when (expect == ExpectRefused && null ranges) (fail "expect refused requires protected_writes")
    Trajectories <$> document .: "before_unload" <*> document .: "after_unload" <*> pure sites <*> pure ranges <*> pure expect
    where
      distinct items = length (nub items) == length items
      range pair = case pair of
        [lo, hi] -> do
          low <- hexWord lo
          high <- hexWord hi
          either fail pure (watchRange low high)
        _ -> fail "each protected_writes entry is [low, high]"
      hexWord text = case readHex (T.unpack text) of
        [(value, "")] | length (T.unpack text) == 8 -> pure (watchAddress value)
        _ -> fail "protected_writes addresses are eight hex digits without 0x"

phaseNames :: Trajectories -> [T.Text]
phaseNames trajectories = nub (map segmentPhase (beforeUnload trajectories ++ afterUnload trajectories))

-- | Decodes a trajectory file and checks that every required phase names a segment.
decodeTrajectories :: B.ByteString -> Either String Trajectories
decodeTrajectories bytes = do
  trajectories <- eitherDecodeStrict' bytes
  case [ phase | entry <- beforeUnload trajectories ++ afterUnload trajectories
               , phase <- segmentRequires entry, phase `notElem` phaseNames trajectories ] of
    [] -> Right trajectories
    phase : _ -> Left ("trajectory requires an unknown segment: " ++ T.unpack phase)

-- | Where VARIANT differs from RETAINED in anything but press timing (segment key pulses and step
-- timing); empty when VARIANT asserts exactly what RETAINED asserts.
variantMismatches :: Trajectories -> Trajectories -> [String]
variantMismatches retained variant =
  [ "trace sites differ" | trajectoryTraceSites retained /= trajectoryTraceSites variant ]
  ++ [ "protected writes differ" | trajectoryProtectedWrites retained /= trajectoryProtectedWrites variant ]
  ++ [ "expect differs" | trajectoryExpect retained /= trajectoryExpect variant ]
  ++ segments "before_unload" (beforeUnload retained) (beforeUnload variant)
  ++ segments "after_unload" (afterUnload retained) (afterUnload variant)
  where
    segments list old new
      | length old /= length new = [list ++ " has " ++ show (length new) ++ " segments, not " ++ show (length old)]
      | otherwise =
          [ list ++ " segment " ++ show index ++ " (" ++ T.unpack (segmentPhase a) ++ ") differs in " ++ field
          | (index, a, b) <- zip3 [0 :: Int ..] old new, (field, same) <- fields a b, not same ]
    fields a b =
      [ ("phase", segmentPhase a == segmentPhase b)
      , ("mode_dial", segmentModeDial a == segmentModeDial b)
      , ("capture", segmentCapture a == segmentCapture b)
      , ("steps", map assertion (segmentSteps a) == map assertion (segmentSteps b))
      , ("messages", segmentMessages a == segmentMessages b)
      , ("observe", segmentObserve a == segmentObserve b)
      , ("reads", segmentReads a == segmentReads b)
      , ("requires", segmentRequires a == segmentRequires b)
      , ("film_row", segmentFilmRow a == segmentFilmRow b) ]
    assertion step = (stepKey step, stepExpected step)
