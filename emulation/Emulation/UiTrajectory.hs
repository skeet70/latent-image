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
  , LifecycleAction (..)
  , LifecycleRole (..)
  , ReadExpectation (..)
  , ReadPreservation (..)
  , checkReadExpectation
  , ExecutionInterval (..)
  , executionIntervalLabel
  , TraceSite (..)
  , SiteRead (..)
  , SiteLr (..)
  , SiteSp (..)
  , Trajectories (..)
  , TrajectoryProtection (..)
  , TrajectoryExpect (..)
  , expectLabel
  , phaseNames
  , instructionPinnedPhases
  , decodeTrajectories
  , variantMismatches
  , overlayExit
  , checkModalObserverConfiguration
  ) where

import Control.Applicative ((<|>))
import Control.Monad (forM, unless, when)
import Data.Aeson (FromJSON (..), Value, eitherDecodeStrict', withArray, withObject, (.!=), (.:?), (.:))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser)
import qualified Data.ByteString as B
import Data.Foldable (toList)
import Data.List (nub, sort, stripPrefix)
import Data.Maybe (isJust)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.PreservationObservation (PreservationRanges, checkPreservationRangeCount, preservationRangeList, preservationRanges)
import Emulation.WatchObservation (ExecutionInterval (..), executionIntervalLabel, Instructions (..), WatchAddress, WatchRange, unionWatchRanges, watchAddress, watchAddressValue, watchRange, watchRangeBounds)
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


data ReadExpectation = ReadExpectation
  { expectedReadLabel :: String
  , expectedReadBytes :: B.ByteString
  } deriving (Eq, Show)

checkReadExpectation :: ReadExpectation -> B.ByteString -> Either String ()
checkReadExpectation expected actual =
  unless (expectedReadBytes expected == actual) (Left ("read bytes differ: " ++ expectedReadLabel expected))

data ReadPreservation = ReadPreservation
  { preservedReadLabel :: String
  , preservedSourcePhase :: T.Text
  , preservedSourceRead :: String
  } deriving (Eq, Show)

instance FromJSON ReadPreservation where
  parseJSON = withObject "read preservation" $ \entry -> do
    unless (sort (KM.keys entry) == ["phase", "read", "source_read"])
      (fail "read preservation takes exactly read, phase and source_read")
    ReadPreservation <$> entry .: "read" <*> entry .: "phase" <*> entry .: "source_read"

instance FromJSON ReadExpectation where
  parseJSON = withObject "read expectation" $ \entry -> do
    unless (sort (KM.keys entry) == ["hex", "read"]) (fail "read expectation takes exactly read and hex")
    label <- entry .: "read"
    text <- entry .: "hex"
    unless (not (T.null text) && even (T.length text) && T.all (`elem` ("0123456789abcdef" :: String)) text)
      (fail "read expectation hex must be nonempty lowercase contiguous byte hex")
    let bytes [] = pure []
        bytes (a : b : rest) = case readHex [a, b] of
          [(byte, "")] -> (byte :) <$> bytes rest
          _ -> fail "invalid read expectation byte"
        bytes _ = fail "incomplete read expectation byte"
    ReadExpectation label . B.pack <$> bytes (T.unpack text)
-- | `stepIdle` is the guest instructions run with no input before the press (FujiMod only).
data Step = Step { stepKey :: String, stepExpected :: MenuState, stepTiming :: KeyTiming, stepIdle :: Maybe Instructions } deriving (Eq, Show)

-- | The bridge's largest `idle` request, its MAX_SETTLE.
maxStepIdle :: Integer
maxStepIdle = 90000000

instance FromJSON Step where
  parseJSON = withArray "step" $ \items -> case toList items of
    [key, menu, setting, cursor] -> step key menu setting cursor (defaultKeyTiming, Nothing)
    [key, menu, setting, cursor, timing] -> step key menu setting cursor =<< withObject "step timing" keyTiming timing
    _ -> fail "a step is [key, menu, setting, cursor] with an optional timing object"
    where
      step key menu setting cursor (timing, idle) =
        Step <$> parseJSON key <*> (MenuState <$> parseJSON menu <*> parseJSON setting <*> parseJSON cursor) <*> pure timing <*> pure idle
      keyTiming timing = do
        unless (all (`elem` ["hold_instructions", "release_instructions", "settle_instructions", "idle_instructions"]) (KM.keys timing))
          (fail "step timing takes only hold_instructions, release_instructions, settle_instructions and idle_instructions")
        pulse <- if KM.member "hold_instructions" timing || KM.member "release_instructions" timing
          then Just <$> keyPulse timing
          else pure Nothing
        settle <- timing .:? "settle_instructions"
        when (settle == Just 0) (fail "settle_instructions must be positive")
        idle <- timing .:? "idle_instructions"
        unless (all (\count -> count >= 1 && count <= maxStepIdle) idle) (fail ("idle_instructions must be 1.." ++ show maxStepIdle))
        pure (KeyTiming pulse (SettleBudget <$> settle), Instructions . fromInteger <$> idle)

data LifecycleRole = RetainedLifecycle | OwnedLifecycle deriving (Eq, Show)
data LifecycleAction = LifecycleAction
  { lifecycleImage5 :: FilePath
  , lifecycleImage6 :: FilePath
  , lifecycleRole :: LifecycleRole
  } deriving (Eq, Show)

instance FromJSON LifecycleRole where
  parseJSON value = do
    role <- parseJSON value
    case role :: T.Text of
      "retained" -> pure RetainedLifecycle
      "owned" -> pure OwnedLifecycle
      _ -> fail "lifecycle program must be retained or owned"

instance FromJSON LifecycleAction where
  parseJSON = withObject "save lifecycle" $ \row ->
    LifecycleAction <$> row .: "image5" <*> row .: "image6" <*> row .: "program"

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
  , segmentReadExpectations :: [ReadExpectation]
  , segmentReadPreservations :: [ReadPreservation]
  , segmentSaveLifecycle :: Maybe LifecycleAction
  }

instance FromJSON Segment where
  parseJSON = withObject "segment" $ \segment -> do
    result <- Segment
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
      <*> segment .:? "read_expectations" .!= []
      <*> segment .:? "read_preservations" .!= []
      <*> segment .:? "save_lifecycle"
    let expectations = segmentReadExpectations result
        labels = map expectedReadLabel expectations
    unless (length (nub labels) == length labels) (fail "read expectation labels must be unique")
    mapM_ (\expectation -> case [len | (label, _, len) <- segmentReads result, label == expectedReadLabel expectation] of
      [len] | len == B.length (expectedReadBytes expectation) -> pure ()
      _ -> fail "read expectation must name exactly one same-segment read of equal length") expectations
    let preserved = map preservedReadLabel (segmentReadPreservations result)
    unless (length (nub preserved) == length preserved) (fail "read preservation labels must be unique")
    mapM_ (\label -> unless (length [() | (name, _, _) <- segmentReads result, name == label] == 1)
      (fail "read preservation must name exactly one same-segment read")) preserved
    pure result
    where
      modeDial position = case lookup (T.unpack position) [(modeDialName p, p) | p <- [minBound .. maxBound]] of
        Just p -> pure p
        Nothing -> fail ("unsupported mode-dial position: " ++ T.unpack position)
      window (label, address, len) = do
        start <- either fail pure (readAddress address)
        pure (label, start, len)

readAddress :: T.Text -> Either String ReadAddress
readAddress address = case T.unpack address of
  text | Just rest <- stripPrefix "runtime:" text, not (null rest) -> do
           let (name, suffix) = break (== '+') rest
               digits = dropWhile (== '+') suffix
           when (null name) (Left ("read address names no runtime symbol: " ++ text))
           offset <- case digits of
             "" -> Right 0
             _ -> maybe (Left ("read address offset is not hex: " ++ text)) Right (hexValue digits)
           Right (RuntimeSymbol name offset)
       | Just value <- hexValue text -> Right (FixedAddress value)
       | otherwise -> Left ("read address must be hex without 0x or runtime:SYMBOL+0xOFFSET: " ++ text)
 where
  hexValue digits = case readHex (case stripPrefix "0x" digits of Just rest -> rest; Nothing -> digits) of
    [(value, "")] -> Just value
    _ -> Nothing

keyPulse :: Object -> Parser KeyPulse
keyPulse budget = do
  pulse <- KeyPulse <$> budget .: "hold_instructions" <*> budget .: "release_instructions"
  when (keyPulseHold pulse == 0 || keyPulseRelease pulse == 0) (fail "key_pulse budgets must be positive")
  pure pulse

-- | An A32 instruction address whose every entry the bridge records with r0-r3, under a label,
-- with the site's physical reads at entry; the e2e keeps lr for a site with reads or `"lr": true`,
-- and sp for `"sp": true`.
data TraceSite = TraceSite
  { traceSiteAddress :: ReadAddress
  , traceSiteLabel :: String
  , traceSiteReads :: [SiteRead]
  , traceSiteLr :: SiteLr
  , traceSiteSp :: SiteSp
  } deriving (Eq, Show)

data SiteLr = RecordsLr | OmitsLr deriving (Eq, Show)

data SiteSp = RecordsSp | OmitsSp deriving (Eq, Show)

-- | A physical range a trace site reads at each entry: start and byte count, 1..4096.
data SiteRead = SiteRead { siteReadStart :: Word32, siteReadCount :: Int } deriving (Eq, Show)

-- | Labels the e2e arms itself.
reservedLabels :: [String]
reservedLabels = ["menu-message", "d3-hook"]

instance FromJSON TraceSite where
  parseJSON value = do
    (address, label, options) <- (\(address, label) -> (address, label, Nothing)) <$> parseJSON value
      <|> (\(address, label, options) -> (address, label, Just options)) <$> parseJSON value
    site <- either fail pure (readAddress address)
    case site of
      FixedAddress word -> do
        unless (T.length address == 8) (fail ("trace site address must be eight hex digits without 0x: " ++ T.unpack address))
        when (word `mod` 4 /= 0) (fail ("trace site address must be word aligned: " ++ T.unpack address))
      RuntimeSymbol _ offset -> when (offset `mod` 4 /= 0) (fail ("trace site offset must be word aligned: " ++ T.unpack address))
    when (null label || label `elem` reservedLabels) (fail ("trace site label is empty or reserved: " ++ label))
    (entryReads, wantsLr, wantsSp) <- case options of
      Nothing -> pure ([], False, False)
      Just fields -> do
        unless (all (`elem` ["reads", "lr", "sp"]) (KM.keys fields)) (fail "trace site options take reads, lr and sp")
        (,,) <$> fields .:? "reads" .!= [] <*> fields .:? "lr" .!= False <*> fields .:? "sp" .!= False
    siteReads <- forM (entryReads :: [(T.Text, Int)]) $ \(start, count) -> case readAddress start of
      Right (FixedAddress word) | T.length start == 8, count >= 1, count <= 4096, toInteger word + toInteger count <= 0x100000000 ->
        pure (SiteRead word count)
      _ -> fail ("trace site read must be eight hex digits and 1..4096 bytes: " ++ T.unpack start)
    pure (TraceSite site label siteReads (if wantsLr || not (null siteReads) then RecordsLr else OmitsLr)
      (if wantsSp then RecordsSp else OmitsSp))

-- | The declared protected state. A route declares at most one protection form.
data TrajectoryProtection
  = Unprotected
  | WatchProtection [WatchRange]
  | PreservationProtection PreservationRanges
  deriving (Eq, Show)

data TrajectoryExpect = ExpectClean | ExpectRefused deriving (Eq, Show)

expectLabel :: TrajectoryExpect -> T.Text
expectLabel ExpectClean = "clean"
expectLabel ExpectRefused = "refused"


data Trajectories = Trajectories
  { beforeUnload :: [Segment]
  , afterUnload :: [Segment]
  , trajectoryTraceSites :: [TraceSite]
  , trajectoryProtection :: TrajectoryProtection
  , trajectoryExpect :: TrajectoryExpect
  , trajectoryForbiddenExecution :: [WatchRange]
  , trajectoryExecutionInterval :: ExecutionInterval
  , trajectoryExecutionControlPc :: Maybe WatchAddress
  }

instance FromJSON Trajectories where
  parseJSON = withObject "trajectories" $ \document -> do
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported trajectory schema")
    sites <- document .:? "trace_sites" .!= []
    unless (distinct (map traceSiteAddress sites) && distinct (map traceSiteLabel sites))
      (fail "trace sites must have distinct addresses and labels")
    watch <- document .:? "protected_writes" :: Parser (Maybe [[T.Text]])
    preservation <- document .:? "protected_memory" :: Parser (Maybe Value)
    protection <- case (watch, preservation) of
      (Just _, Just _) -> fail "protected_writes and protected_memory are conflicting declarations"
      (Just entries, Nothing) -> do
        ranges <- mapM hexRange entries
        pure (if null ranges then Unprotected else WatchProtection ranges)
      (Nothing, Just value) -> PreservationProtection <$> protectedMemory value
      (Nothing, Nothing) -> pure Unprotected
    expectText <- document .:? "expect" .!= ("clean" :: T.Text)
    expect <- case expectText of
      "clean" -> pure ExpectClean
      "refused" -> pure ExpectRefused
      _ -> fail "expect must be clean or refused"
    execution <- mapM hexRange =<< document .:? "forbidden_execution" .!= []
    when (length execution > 8 || length (nub execution) /= length execution)
      (fail "forbidden_execution requires at most eight distinct ranges")
    intervalText <- document .:? "forbidden_execution_interval" .!= ("before-unload" :: T.Text)
    interval <- case intervalText of
      "before-unload" -> pure BeforeUnload
      "through-after-unload" | expect == ExpectRefused && not (null execution) -> pure ThroughAfterUnload
      _ -> fail "forbidden_execution_interval must be before-unload, or through-after-unload for a fetch refusal control"
    when (expect == ExpectRefused && protection == Unprotected && null execution)
      (fail "expect refused requires protected_writes, protected_memory or forbidden_execution")
    let writeRanges = case protection of
          WatchProtection ranges -> ranges
          PreservationProtection ranges -> preservationRangeList ranges
          Unprotected -> []
    when (length (unionWatchRanges (writeRanges ++ execution)) > 8)
      (fail "combined write and execution watch exceeds eight ranges")
    controlPc <- traverse hexWord =<< document .:? "forbidden_execution_control_pc"
    case (expect, controlPc) of
      (ExpectRefused, Just pc) | not (null execution) ->
        unless (any (\range -> let (low, high) = watchRangeBounds range
                              in fromIntegral (watchAddressValue pc) >= low && fromIntegral (watchAddressValue pc) < high) execution)
          (fail "forbidden_execution_control_pc is outside forbidden_execution")
      (_, Nothing) | null execution || expect == ExpectClean -> pure ()
      _ -> fail "fetch refusal requires forbidden_execution_control_pc and clean trajectories must omit it"
    Trajectories <$> document .: "before_unload" <*> document .: "after_unload" <*> pure sites <*> pure protection <*> pure expect
      <*> pure execution <*> pure interval <*> pure controlPc
    where
      distinct items = length (nub items) == length items

protectedMemory :: Value -> Parser PreservationRanges
protectedMemory = withObject "protected_memory" $ \memory -> do
  unless (sort (map (T.unpack . K.toText) (KM.keys memory)) == ["kind", "ranges"])
    (fail "protected_memory takes exactly kind and ranges")
  kind <- memory .: "kind"
  unless (kind == ("ram-byte-preservation" :: T.Text)) (fail "protected_memory kind is not ram-byte-preservation")
  entries <- memory .: "ranges"
  either fail pure (checkPreservationRangeCount (length entries))
  ranges <- mapM hexRange entries
  either fail pure (preservationRanges ranges)

hexRange :: [T.Text] -> Parser WatchRange
hexRange pair = case pair of
  [lo, hi] -> do
    low <- hexWord lo
    high <- hexWord hi
    either fail pure (watchRange low high)
  _ -> fail "each protection range is [low, high]"

hexWord :: T.Text -> Parser WatchAddress
hexWord text = case readHex (T.unpack text) of
  [(value, "")] | length (T.unpack text) == 8 -> pure (watchAddress value)
  _ -> fail "protection addresses are eight hex digits without 0x"

phaseNames :: Trajectories -> [T.Text]
phaseNames trajectories = nub (map segmentPhase (beforeUnload trajectories ++ afterUnload trajectories))

-- | Phases whose segment `key_pulse` or a step's hold/release pins an instruction hold, which
-- only the retained engine takes.
instructionPinnedPhases :: Trajectories -> [T.Text]
instructionPinnedPhases trajectories = nub
  [ segmentPhase segment | segment <- beforeUnload trajectories ++ afterUnload trajectories
  , isJust (segmentKeyPulse segment) || any (isJust . keyTimingPulse . stepTiming) (segmentSteps segment) ]

-- | Decodes a trajectory file and checks that every required phase names a segment.
decodeTrajectories :: B.ByteString -> Either String Trajectories
decodeTrajectories bytes = do
  trajectories <- eitherDecodeStrict' bytes
  let entries = beforeUnload trajectories ++ afterUnload trajectories
  mapM_ (\(index, entry) -> mapM_ (\preservation -> do
    let sources = [(sourceIndex, len) | (sourceIndex, source) <- zip [0 :: Int ..] entries
                                     , segmentPhase source == preservedSourcePhase preservation
                                     , (label, _, len) <- segmentReads source, label == preservedSourceRead preservation]
        target = [len | (label, _, len) <- segmentReads entry, label == preservedReadLabel preservation]
    case (sources, target) of
      ([(sourceIndex, sourceLength)], [targetLength])
        | sourceIndex < index && sourceLength > 0 && sourceLength == targetLength -> Right ()
      _ -> Left "read preservation source is missing, forward, ambiguous or has a different length")
    (segmentReadPreservations entry)) (zip [0 :: Int ..] entries)
  case [ phase | entry <- beforeUnload trajectories ++ afterUnload trajectories
               , phase <- segmentRequires entry, phase `notElem` phaseNames trajectories ] of
    [] -> Right trajectories
    phase : _ -> Left ("trajectory requires an unknown segment: " ++ T.unpack phase)

-- | Where VARIANT differs from RETAINED in anything but press timing (segment key pulses and step
-- timing) and the retained-only MENU after each rendezvous ('withoutOverlayExits'); empty when
-- VARIANT asserts exactly what RETAINED asserts.
variantMismatches :: Trajectories -> Trajectories -> [String]
variantMismatches retained variant
  | null (mismatches retained) = []
  | otherwise = mismatches (withoutOverlayExits retained)
  where
    mismatches old =
      [ "trace sites differ" | trajectoryTraceSites old /= trajectoryTraceSites variant ]
      ++ [ "protection differs" | trajectoryProtection old /= trajectoryProtection variant ]
      ++ [ "expect differs" | trajectoryExpect old /= trajectoryExpect variant ]
      ++ [ "forbidden execution differs" | trajectoryForbiddenExecution old /= trajectoryForbiddenExecution variant ]
      ++ [ "execution interval differs" | trajectoryExecutionInterval old /= trajectoryExecutionInterval variant ]
      ++ [ "execution control PC differs" | trajectoryExecutionControlPc old /= trajectoryExecutionControlPc variant ]
      ++ segments "before_unload" (beforeUnload old) (beforeUnload variant)
      ++ segments "after_unload" (afterUnload old) (afterUnload variant)
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
      , ("read_expectations", segmentReadExpectations a == segmentReadExpectations b)
      , ("read_preservations", segmentReadPreservations a == segmentReadPreservations b)
      , ("requires", segmentRequires a == segmentRequires b)
      , ("film_row", segmentFilmRow a == segmentFilmRow b)
      , ("save_lifecycle", segmentSaveLifecycle a == segmentSaveLifecycle b) ]
    assertion step = (stepKey step, stepExpected step)

-- | The first pressed step of SEGMENTS, with its segment's index, when it is a MENU its segment
-- does not end on: the press that leaves the retained bridge's rendezvous overlay. The retained
-- bridge holds UP across each rendezvous, which opens focus-area selection at live view; the
-- FujiMod bridge pushes the command dial, which does not.
overlayExit :: [Segment] -> Maybe (Int, Step)
overlayExit segments = case dropWhile (null . segmentSteps . snd) (zip [0 ..] segments) of
  (index, segment) : _ | step : _ : _ <- segmentSteps segment, stepKey step == "menu" -> Just (index, step)
  _ -> Nothing

-- | The trajectory without its 'overlayExit' on either side of unload.
withoutOverlayExits :: Trajectories -> Trajectories
withoutOverlayExits trajectories =
  trajectories { beforeUnload = dropExit (beforeUnload trajectories), afterUnload = dropExit (afterUnload trajectories) }
  where
    dropExit entries = case overlayExit entries of
      Just (index, _) -> [ if position == index then segment { segmentSteps = drop 1 (segmentSteps segment) } else segment
                         | (position, segment) <- zip [0 ..] entries ]
      Nothing -> entries

checkModalObserverConfiguration :: Trajectories -> Either String ()
checkModalObserverConfiguration trajectories =
  unless (trajectoryProtection trajectories == Unprotected && null (trajectoryForbiddenExecution trajectories)
    && trajectoryExpect trajectories == ExpectClean)
    (Left "modal observation refuses another memory/execution observer or refusal control")
