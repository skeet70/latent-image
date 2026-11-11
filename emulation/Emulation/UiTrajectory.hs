{-# LANGUAGE OverloadedStrings #-}
-- | Trajectory files for the emulator e2e: menu segments, the guest windows read after them
-- and the code sites traced while they run.
module Emulation.UiTrajectory
  ( MenuState (..)
  , ReadAddress (..)
  , FilmRow (..)
  , filmStoredCode
  , Step (..)
  , StepCompletion (..)
  , presentedFrameMenu
  , Segment (..)
  , LifecycleAction (..)
  , AfOwnershipAction (..)
  , AfOwnershipPlan
  , decodeAfOwnershipPlan
  , afOwnershipPlanJson
  , afOwnershipIdentity
  , checkAfOwnershipRoute
  , validateAfOwnershipReceipt
  , AfMappingCatalogue
  , afMappingCatalogue
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
import Control.Monad (foldM, forM_, unless, when, zipWithM)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, withArray, withObject, withText, (.=), (.!=), (.:?), (.:))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits ((.&.), shiftL)
import Data.Foldable (toList)
import qualified Data.Map.Strict as Map
import Data.List (nub, sort, stripPrefix)
import Data.Maybe (isJust)
import qualified Data.Text as T
import Data.Word (Word8, Word32, Word64)
import Data.Int (Int32)
import Emulation.PreservationObservation (PreservationRanges, checkPreservationRangeCount, preservationRangeList, preservationRanges)
import Emulation.WatchObservation (ExecutionInterval (..), executionIntervalLabel, Instructions (..), WatchAddress, WatchRange, unionWatchRanges, watchAddress, watchAddressValue, watchRange, watchRangeBounds)
import EmulatorUsb (CrawlKey, SiteRead (..), KeyPulse (..), KeyTiming (..), ModeDial (..), SettleBudget (..), defaultKeyTiming, modeDialName)
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
data StepCompletion = PacketCompletion | PresentedFrameCompletion CrawlKey deriving (Eq, Show)
-- | `stepIdle` is the guest instructions run with no input before the press (FujiMod only).
data Step = Step { stepKey :: String, stepExpected :: MenuState, stepTiming :: KeyTiming, stepIdle :: Maybe Instructions, stepCompletion :: StepCompletion } deriving (Eq, Show)

data DisplaySettle = DisplayUnpressed | DisplayConsumed | DisplayUnsettled deriving (Eq)

presentedFrameMenu :: Value -> Either String MenuState
presentedFrameMenu = parseEither $ withObject "display-key reply" $ \reply -> do
  failure <- reply .: "failure" :: Parser (Maybe String)
  maybe (pure ()) fail failure
  status <- reply .: "settle" >>= withObject "display settle" (\settle ->
    settle .: "status" >>= withText "display settle status" (\value -> case value of
      "prior-render-pending" -> pure DisplayUnpressed
      "packet-consumed" -> pure DisplayConsumed
      "budget-exhausted" -> pure DisplayUnsettled
      _ -> fail "unknown display settle status"))
  unless (status == DisplayConsumed) (fail "presented-frame requires a consumed physical key packet")
  _ <- reply .: "display_wait_instructions" :: Parser Word32
  reply .: "menu" >>= withObject "display menu" (\menu ->
    MenuState <$> menu .: "menu_id" <*> menu .: "setting_id" <*> menu .: "cursor")

-- | The bridge's largest `idle` request, its MAX_SETTLE.
maxStepIdle :: Integer
maxStepIdle = 90000000

instance FromJSON Step where
  parseJSON = withArray "step" $ \items -> case toList items of
    [key, menu, setting, cursor] -> step key menu setting cursor (defaultKeyTiming, Nothing, PacketCompletion)
    [key, menu, setting, cursor, timing] -> step key menu setting cursor =<< withObject "step contract" (keyTiming key) timing
    _ -> fail "a step is [key, menu, setting, cursor] with an optional contract object"
    where
      step key menu setting cursor (timing, idle, completion) =
        Step <$> parseJSON key <*> (MenuState <$> parseJSON menu <*> parseJSON setting <*> parseJSON cursor) <*> pure timing <*> pure idle <*> pure completion
      keyTiming key timing = do
        unless (all (`elem` ["hold_instructions", "release_instructions", "settle_instructions", "idle_instructions", "completion"]) (KM.keys timing))
          (fail "unknown step contract field")
        completion <- timing .:? "completion" >>= \value -> case (value :: Maybe T.Text) of
          Nothing -> pure PacketCompletion
          Just "packet-consumed" -> pure PacketCompletion
          Just "presented-frame" -> PresentedFrameCompletion <$> parseJSON key
          _ -> fail "completion must be packet-consumed or presented-frame"
        pulse <- if KM.member "hold_instructions" timing || KM.member "release_instructions" timing
          then Just <$> keyPulse timing
          else pure Nothing
        settle <- timing .:? "settle_instructions"
        when (settle == Just 0) (fail "settle_instructions must be positive")
        idle <- timing .:? "idle_instructions"
        unless (all (\count -> count >= 1 && count <= maxStepIdle) idle) (fail ("idle_instructions must be 1.." ++ show maxStepIdle))
        when (completion /= PacketCompletion && (isJust pulse || isJust settle || isJust idle))
          (fail "presented-frame uses the bridge's bounded display-key contract without timing overrides")
        pure (KeyTiming pulse (SettleBudget <$> settle), Instructions . fromInteger <$> idle, completion)

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

data AfOwnershipAction = ArmAfOwnership | EndAfOwnership deriving (Eq, Show)

instance FromJSON AfOwnershipAction where
  parseJSON = withText "AF ownership action" $ \action -> case action of
    "arm" -> pure ArmAfOwnership
    "end" -> pure EndAfOwnership
    _ -> fail "af_ownership must be arm or end"

data AfSiteKind = Consumer | ModeHelper | Primary | Current | SetAf | Callback | ClearAf | RequestAf | WorkPost | QueuePost | Dequeue | SelectAf | DispatchCallback | ReservePrimary | ReserveSecondary
  deriving (Eq, Show, Enum, Bounded)

afSiteKindName :: AfSiteKind -> T.Text
afSiteKindName kind = ["consumer", "mode-helper", "primary", "current", "set", "callback", "clear", "request", "work-post", "queue-post", "dequeue", "select", "dispatch-callback", "reserve-primary", "reserve-secondary"] !! fromEnum kind

data AfRegisterRead = AfRegisterRead Word32 Int32 Word32
data AfSite = AfSite Word32 Word32 AfSiteKind [(Word32, Word32)] [AfRegisterRead]
data AfOwnershipPlan = AfOwnershipPlan [AfSite] Object

afOwnershipIdentity :: AfOwnershipPlan -> Value
afOwnershipIdentity (AfOwnershipPlan _ identity) = Object identity

instance FromJSON AfOwnershipPlan where
  parseJSON = withObject "AF ownership plan" $ \fields -> do
    unless (sort (KM.keys fields) == ["identity", "sites"]) (fail "AF ownership plan takes exactly sites and identity")
    identity <- fields .: "identity"
    unless (sort (KM.keys identity) == ["bridge", "model", "package", "sdk", "viewer"]) (fail "AF identity requires exactly sdk, viewer, bridge, model and package")
    mapM_ (\key -> do
      binding <- identity .: key
      unless (not (T.null binding)) (fail "AF identity binding is empty"))
      ["sdk", "viewer", "bridge", "model", "package"]
    sites <- fields .: "sites" >>= mapM (withObject "AF site" $ \site -> do
      unless (all (`elem` ["pc", "word", "kind", "reads", "register_reads"]) (KM.keys site))
        (fail "unknown AF site field")
      pc <- site .: "pc"
      unless (pc `mod` 4 == 0 && pc .&. 0x40000000 == 0 && pc <= 0xfffffffb) (fail "AF pc is not a bounded physical A32 address")
      word <- site .: "word"
      name <- site .: "kind"
      kind <- case [kind | kind <- [minBound .. maxBound], afSiteKindName kind == name] of
        [kind] -> pure kind
        _ -> fail "unknown AF site kind"
      fixedReads <- site .: "reads" >>= mapM (withArray "AF read" $ \pair -> case toList pair of
        [address, bytes] -> do
          low <- parseJSON address
          count <- parseJSON bytes
          unless (count > 0 && count <= 4096 && toInteger low + toInteger count <= 0x100000000)
            (fail "AF read exceeds bounded address range")
          pure (low, count)
        _ -> fail "AF read must be [address,bytes]")
      registers <- site .:? "register_reads" .!= [] >>= mapM (withObject "AF register read" $ \row -> do
        unless (sort (KM.keys row) == ["bytes", "offset", "register"]) (fail "unknown AF register read field")
        register <- row .: "register"
        offset <- row .: "offset"
        count <- row .: "bytes"
        unless (register <= 14 && count > 0 && count <= 4096) (fail "invalid AF register read")
        pure (AfRegisterRead register offset count))
      unless (length fixedReads + length registers <= 16) (fail "AF site read mask exceeds 16 spans")
      pure (AfSite pc word kind fixedReads registers))
    let pcs = [pc | AfSite pc _ _ _ _ <- sites]
    unless (not (null sites) && length sites <= 128 && nub pcs == pcs && sort pcs == pcs) (fail "AF sites must be ascending, unique and bounded to 128")
    pure (AfOwnershipPlan sites identity)

decodeAfOwnershipPlan :: B.ByteString -> Either String AfOwnershipPlan
decodeAfOwnershipPlan = eitherDecodeStrict'

afOwnershipPlanJson :: AfOwnershipPlan -> Value
afOwnershipPlanJson (AfOwnershipPlan sites identity) = object
  ["identity" .= Object identity, "sites" .= map siteJson sites]
  where
    siteJson (AfSite pc word kind fixedReads registers) = object
      (["pc" .= pc, "word" .= word, "kind" .= afSiteKindName kind, "reads" .= [[address, count] | (address, count) <- fixedReads]]
        ++ ["register_reads" .= [object ["register" .= register, "offset" .= offset, "bytes" .= count] | AfRegisterRead register offset count <- registers] | not (null registers)])

checkAfOwnershipRoute :: Bool -> [Segment] -> [Segment] -> Either String ()
checkAfOwnershipRoute enabled before after = do
  let marked = [(action, segment) | segment <- before ++ after, Just action <- [segmentAfOwnership segment]]
  if enabled then do
    unless (map fst marked == [ArmAfOwnership, EndAfOwnership]) (Left "AF capture requires exactly ordered arm and end segments")
    unless (all (not . isJust . segmentAfOwnership) after) (Left "AF capture must end before unload")
    mapM_ (\(action, segment) -> do
      let keys = map stepKey (segmentSteps segment)
      unless (keys == [if action == ArmAfOwnership then "down" else "left"])
        (Left "AF arm/end segments must contain only the original DOWN/LEFT key")
      unless (segmentModeDial segment == Nothing && segmentSaveLifecycle segment == Nothing && all ((== Nothing) . stepIdle) (segmentSteps segment))
        (Left "AF boundary segment cannot add a mode change, lifecycle or pre-key idle")) marked
  else unless (null marked) (Left "trajectory af_ownership requires --af-ownership-plan")
data AfCore = AfPrimary | AfSecondary deriving (Eq, Show, Enum)
data AfBank = AfBtcm | AfAtcm deriving (Eq, Show, Enum)
data AfMapping = AfMapping AfCore Integer Integer Integer Word32 deriving Eq
newtype AfMappingCatalogue = AfMappingCatalogue [AfMapping]
data AfReadProvenance = AfShared Word32 [[Word32]] | AfTcm AfCore AfBank Word32 Integer Integer Integer [[Word32]]
data AfObservationMode = AfPassive
data AfRetentionStop = AfEnd | AfRefused | AfCapacityExceeded | AfProvenanceUnavailable deriving Eq
data AfRetention = AfDisabled | AfUnarmed | AfArmed | AfStopped AfRetentionStop deriving Eq

instance FromJSON AfObservationMode where
  parseJSON = withText "AF observation mode" $ \mode -> case mode of
    "passive" -> pure AfPassive
    _ -> fail "AF observation mode is not passive"

instance FromJSON AfRetentionStop where
  parseJSON = withText "AF retention stop reason" $ \reason -> case reason of
    "end" -> pure AfEnd
    "refused" -> pure AfRefused
    "capacity-exceeded" -> pure AfCapacityExceeded
    "provenance-unavailable" -> pure AfProvenanceUnavailable
    _ -> fail "unknown AF retention stop reason"

instance FromJSON AfRetention where
  parseJSON = withObject "AF passive retention" $ \fields -> do
    status <- fields .: "status" :: Parser T.Text
    if status == "stopped"
      then do
        afExactFields fields ["status", "reason"]
        AfStopped <$> fields .: "reason"
      else do
        afExactFields fields ["status"]
        case status of
          "disabled" -> pure AfDisabled
          "unarmed" -> pure AfUnarmed
          "armed" -> pure AfArmed
          _ -> fail "unknown AF passive retention status"

afExactFields :: Object -> [K.Key] -> Parser ()
afExactFields fields keys = unless (sort (KM.keys fields) == sort keys) (fail "AF receipt fields differ")

afCore :: Value -> Parser AfCore
afCore = withText "AF core" $ \value -> case value of
  "primary" -> pure AfPrimary
  "secondary" -> pure AfSecondary
  _ -> fail "unknown AF core"

afCoreIndex :: Int -> Parser AfCore
afCoreIndex index = case index of
  0 -> pure AfPrimary
  1 -> pure AfSecondary
  _ -> fail "invalid AF core index"

afTcmWindow :: Word32 -> Parser (Maybe (Integer, Integer))
afTcmWindow register
  | register .&. 1 == 0 || code == 0 = pure Nothing
  | otherwise = do
      unless (code >= 3 && code <= 14 && base `mod` size == 0 && base + size <= 0x100000000)
        (fail "invalid AF TCM window")
      pure (Just (base, base + size))
  where
    code = fromIntegral ((register `div` 4) .&. 31)
    base = toInteger (register .&. 0xfffff000)
    size = 512 `shiftL` code

afObservedTcm :: Object -> Parser [[Word32]]
afObservedTcm fields = do
  values <- fields .: "observed_tcm"
  unless (length values == 2 && all ((== 2) . length) values) (fail "AF TCM matrix differs")
  mapM_ (mapM_ afTcmWindow) values
  pure values

afReadProvenance :: Value -> Parser AfReadProvenance
afReadProvenance = withObject "AF read provenance" $ \fields -> do
  kind <- fields .: "kind" :: Parser T.Text
  observed <- afObservedTcm fields
  case kind of
    "shared-physical" -> do
      afExactFields fields ["kind", "physical", "observed_tcm"]
      AfShared <$> fields .: "physical" <*> pure observed
    "tcm" -> do
      afExactFields fields ["kind", "core", "bank", "register", "window_start", "window_end", "offset", "observed_tcm"]
      core <- fields .: "core" >>= afCore
      name <- fields .: "bank" :: Parser T.Text
      bank <- case name of
        "btcm" -> pure AfBtcm
        "atcm" -> pure AfAtcm
        _ -> fail "unknown AF TCM bank"
      AfTcm core bank <$> fields .: "register" <*> fields .: "window_start" <*> fields .: "window_end" <*> fields .: "offset" <*> pure observed
    _ -> fail "unknown AF memory view"

afMapping :: Value -> Parser AfMapping
afMapping = withObject "AF mapping" $ \fields -> do
  afExactFields fields ["core", "address", "physical", "size", "permissions"]
  core <- fields .: "core" >>= afCoreIndex
  low <- fields .: "address"
  physical <- fields .: "physical"
  size <- fields .: "size"
  permissions <- fields .: "permissions"
  unless (low >= 0 && physical >= 0 && size > 0 && low + size <= 0x100000000 && physical + size <= 0x100000000 && permissions .&. 7 == permissions)
    (fail "AF mapping geometry differs")
  pure (AfMapping core low physical size permissions)

afTranslation :: [AfMapping] -> AfCore -> Integer -> Integer -> Integer -> Parser ()
afTranslation maps core low size physical = walk low
  where
    high = low + size
    walk cursor | cursor == high = pure ()
                | otherwise = case [mapping | mapping@(AfMapping selected base _ bytes _) <- maps, selected == core, base <= cursor, cursor < base + bytes] of
      [AfMapping _ base target bytes permissions] -> do
        let next = minimum (high : base + bytes : [start | AfMapping selected start _ _ _ <- maps, selected == core, start > cursor, start < high])
        unless (permissions .&. 1 /= 0 && target + cursor - base == physical + cursor - low)
          (fail "AF mapping translation differs")
        walk next
      _ -> fail "AF mapping is absent or ambiguous"

afValidateRead :: [AfMapping] -> AfCore -> Bool -> (Word32, Word32) -> Value -> Parser (Maybe [[Word32]])
afValidateRead maps core fixed (address, count) value
  | value == Null = do
      unless (not fixed && address == 0) (fail "AF null read is not a null register pointer")
      pure Nothing
  | otherwise = Just <$> withObject "AF read" (\fields -> do
      afExactFields fields ["address", "provenance", "bytes"]
      actualAddress <- fields .: "address"
      bytes <- fields .: "bytes" :: Parser [Word8]
      unless (actualAddress == address && count > 0 && count <= 4096 && toInteger address + toInteger count <= 0x100000000 && length bytes == fromIntegral count)
        (fail "AF read mask differs")
      provenance <- fields .: "provenance" >>= afReadProvenance
      let low = toInteger address
          high = low + toInteger count
          overlaps (start, end) = start < high && low < end
      case provenance of
        AfTcm selected bank register start end offset observed -> do
          windows <- mapM afTcmWindow (observed !! fromEnum core)
          unless (not fixed && selected == core && register == observed !! fromEnum core !! fromEnum bank
              && windows !! fromEnum bank == Just (start, end) && low >= start && high <= end && offset == low - start
              && length [() | Just window <- windows, overlaps window] == 1)
            (fail "AF TCM provenance differs")
          pure observed
        AfShared physical observed -> do
          unless (physical == address .&. 0xbfffffff && toInteger physical + toInteger count <= 0x20000000)
            (fail "AF shared physical provenance differs")
          windows <- mapM (mapM afTcmWindow) observed
          forM_ [AfPrimary, AfSecondary] $ \selected -> forM_ [0, 0x40000000] $ \alias -> do
            let start = toInteger physical + alias
                end = start + toInteger count
            unless (null [() | Just (base, limit) <- windows !! fromEnum selected, base < end && start < limit])
              (fail "AF shared read overlaps TCM")
            afTranslation maps selected start (toInteger count) (toInteger physical)
          unless fixed (afTranslation maps core low (toInteger count) (toInteger physical))
          pure observed) value

afTaskSample :: AfCore -> Value -> Parser ()
afTaskSample selected = withObject "AF sampled task" $ \fields -> do
  status <- fields .: "status" :: Parser T.Text
  case status of
    "refused" -> do
      afExactFields fields ["status", "error", "context_depth"]
      _ <- fields .: "context_depth" :: Parser (Maybe Word32)
      problem <- fields .: "error" :: Parser T.Text
      unless (not (T.null problem) && T.length problem <= 4096) (fail "invalid AF task refusal")
    "sampled" -> do
      afExactFields fields ["status", "kernel", "id", "node", "creation", "descriptor", "context_depth"]
      core <- fields .: "kernel" >>= afCore
      ident <- fields .: "id" :: Parser Word32
      node <- fields .: "node" :: Parser Word32
      creation <- fields .: "creation" :: Parser Word32
      descriptor <- fields .: "descriptor" :: Parser [Word32]
      _ <- fields .: "context_depth" :: Parser Word32
      let (nodes, creations) = if core == AfPrimary then (0x4005ff0c, 0x4005beac) else (0x400efecc, 0x400ebe6c)
      unless (core == selected && ident >= 1 && ident <= 120 && node == nodes + (ident - 1) * 56 && creation == creations + (ident - 1) * 40 && length descriptor == 10)
        (fail "AF sampled task identity differs")
      unless (descriptor !! 8 == ident && descriptor !! 9 == 1 && descriptor !! 4 > 0 && toInteger (descriptor !! 5 .&. 0xbfffffff) + toInteger (descriptor !! 4) <= 0x20000000)
        (fail "AF sampled task stack extent differs")
    _ -> fail "unknown AF task sample status"

data AfBoundary = AfBoundary [Word64] (Map.Map Word32 Word32)
data AfWrite = AfWrite Integer Integer Word32 Word32 [Word8] Word64 (Maybe [Word64])

afBoundary :: Value -> Parser AfBoundary
afBoundary = withObject "AF boundary" $ \fields -> do
  afExactFields fields ["counts", "words"]
  counts <- fields .: "counts" :: Parser [Word64]
  unless (length counts == 2 && sum (map toInteger counts) <= toInteger (maxBound :: Word64)) (fail "AF boundary clocks differ")
  snapshotWords <- fields .: "words" >>= mapM (withObject "AF snapshot word" $ \row -> do
    afExactFields row ["address", "value"]
    (,) <$> row .: "address" <*> row .: "value")
  let addresses = [0x016fec3c, 0x016fec4c] ++ [0x016fed28,0x016fed2c .. 0x016fee44]
        ++ [0x0170306c, 0x01703080, 0x01703084, 0x017030d0, 0x01703160, 0x0170319c, 0x017031a4, 0x0170327c]
  unless (map fst snapshotWords == addresses) (fail "AF boundary snapshot is incomplete or reordered")
  pure (AfBoundary counts (Map.fromList snapshotWords))

afWrite :: [AfMapping] -> Int -> Value -> Parser AfWrite
afWrite maps sequenceIndex = withObject "AF write row" $ \fields -> do
  unless (all (`elem` ["af_sequence", "source", "address", "physical", "size", "core", "pc", "instructions", "tick", "before", "value", "byte_order", "core_instructions", "latest_fetch"]) (KM.keys fields))
    (fail "unknown AF write row field")
  actualSequence <- fields .: "af_sequence"
  source <- fields .: "source" :: Parser T.Text
  address <- fields .: "address" :: Parser Word32
  physical <- fields .: "physical" :: Parser Word32
  size <- fields .: "size" :: Parser Word32
  instructions <- fields .: "instructions" :: Parser Word64
  tick <- fields .: "tick" :: Parser Word64
  core <- fields .: "core" :: Parser (Maybe Int)
  pc <- fields .: "pc" :: Parser (Maybe Word32)
  before <- fields .: "before" :: Parser [Word8]
  value <- fields .: "value" :: Parser Word64
  order <- fields .: "byte_order" :: Parser T.Text
  unless (actualSequence == sequenceIndex && source `elem` ["cpu-write", "host-write", "model-bus-write"]
      && size `elem` [1,2,4,8] && length before == fromIntegral size && order == "little"
      && (size == 8 || toInteger value < 2 ^ (size * 8)) && physical == address .&. 0xbfffffff
      && toInteger address + toInteger size <= 0x100000000 && toInteger physical + toInteger size <= 0x100000000)
    (fail "AF write row identity, extent or payload differs")
  cpuCounts <- if source == "cpu-write" then do
    selected <- maybe (fail "AF CPU write core absent") afCoreIndex core
    site <- maybe (fail "AF CPU write PC absent") pure pc
    clock <- fields .: "core_instructions" :: Parser Word64
    latest <- fields .: "latest_fetch"
    flip (withObject "AF latest instruction fetch") latest $ \fetch -> do
      afExactFields fetch ["source", "address", "physical", "size", "core", "pc", "instructions", "tick", "value", "before", "byte_order", "core_instructions"]
      fetchSource <- fetch .: "source" :: Parser T.Text
      fetchAddress <- fetch .: "address"
      fetchPhysical <- fetch .: "physical" :: Parser Word32
      fetchSize <- fetch .: "size" :: Parser Word32
      fetchCore <- fetch .: "core" >>= afCoreIndex
      fetchPc <- fetch .: "pc"
      fetchClock <- fetch .: "instructions"
      fetchTick <- fetch .: "tick"
      fetchCoreClock <- fetch .: "core_instructions"
      fetchOrder <- fetch .: "byte_order" :: Parser T.Text
      fetchValue <- fetch .: "value"
      fetchBefore <- fetch .: "before"
      unless (site `mod` 4 == 0 && fetchSource == "cpu-fetch" && fetchCore == selected && fetchPc == site && fetchAddress == site
          && fetchSize == 4 && fetchClock == instructions && fetchTick == tick && fetchCoreClock == clock && clock <= instructions
          && fetchOrder == "little" && fetchValue == Null && fetchBefore == Null)
        (fail "AF CPU write instruction provenance differs")
      afTranslation maps selected (toInteger site) 4 (toInteger fetchPhysical)
      unless (and [permissions .&. 4 /= 0 | AfMapping mappedCore base _ mappedBytes permissions <- maps,
          mappedCore == selected, base < toInteger site + 4, toInteger site < base + mappedBytes])
        (fail "AF instruction fetch mapping is not executable")
    pure (Just (if selected == AfPrimary then [clock, instructions - clock] else [instructions - clock, clock]))
  else do
    latest <- fields .:? "latest_fetch" :: Parser (Maybe Value)
    clock <- fields .:? "core_instructions" :: Parser (Maybe Word64)
    unless (latest == Nothing && clock == Nothing) (fail "AF non-CPU row claims instruction provenance")
    pure Nothing
  pure (AfWrite (toInteger instructions) (toInteger tick) physical size before value cpuCounts)

afEntry :: AfCore -> Word32 -> Word32 -> [Word64] -> Value -> Parser [Maybe (Integer, Integer)]
afEntry core pc cpsr counts = withObject "AF instruction entry" $ \fields -> do
  afExactFields fields ["opaque_id", "address", "bytes", "progress", "pstate", "physical", "tcm"]
  opaque <- fields .: "opaque_id" :: Parser T.Text
  address <- fields .: "address" :: Parser Word64
  bytes <- fields .: "bytes" :: Parser Word32
  progress <- fields .: "progress" :: Parser Word64
  pstate <- fields .: "pstate" :: Parser Word32
  physical <- fields .: "physical" :: Parser (Maybe Word64)
  windows <- fields .: "tcm" :: Parser [Maybe Value]
  unless (T.isPrefixOf "InstructionEntryId {" opaque && T.length opaque <= 512 && address == fromIntegral pc && bytes == 4
      && progress == counts !! fromEnum core && pstate == cpsr && physical == Just address && length windows == 2)
    (fail "AF instruction entry identity differs")
  mapM (traverse (withObject "AF instruction TCM window" $ \row -> do
    afExactFields row ["address", "bytes"]
    low <- row .: "address" :: Parser Word64
    size <- row .: "bytes" :: Parser Word64
    unless (size > 0 && toInteger low + toInteger size <= 0x100000000
        && not (toInteger low < toInteger pc + 4 && toInteger pc < toInteger low + toInteger size))
      (fail "AF instruction entry TCM window differs")
    pure (toInteger low, toInteger low + toInteger size))) windows

afMappingCatalogue :: Value -> Either String AfMappingCatalogue
afMappingCatalogue = parseEither $ \value -> do
  rows <- parseJSON value
  unless (not (null rows) && length rows <= 4096) (fail "AF mapping catalogue bound differs")
  AfMappingCatalogue <$> mapM afMapping rows

validateAfOwnershipReceipt :: AfMappingCatalogue -> AfOwnershipPlan -> AfOwnershipAction -> Value -> Either String ()
validateAfOwnershipReceipt (AfMappingCatalogue expectedMappings) plan@(AfOwnershipPlan sites _) action value = parseEither (withObject "AF diagnostic response" $ \fields -> do
  unless (BL.length (encode value) <= 134217728) (fail "AF output byte bound exceeded")
  afExactFields fields ["schema", "status", "mode", "retention", "identity", "initial", "final", "rows", "contexts", "failure", "read_refusal", "row_count", "context_count", "limits", "mappings"]
  schema <- fields .: "schema"
  status <- fields .: "status" :: Parser T.Text
  identity <- fields .: "identity"
  failure <- fields .: "failure"
  refusal <- fields .: "read_refusal"
  _ <- fields .: "mode" :: Parser AfObservationMode
  retention <- fields .: "retention" :: Parser AfRetention
  unless (retention == (if action == ArmAfOwnership then AfArmed else AfStopped AfEnd))
    (fail "AF passive retention did not reach its required boundary")
  unless (schema == (4 :: Int) && status == (if action == ArmAfOwnership then "armed" else "complete")
      && identity == afOwnershipIdentity plan && failure == Null && refusal == Null)
    (fail "AF diagnostic is incomplete, refused, or has a different identity")
  limits <- fields .: "limits"
  unless (limits == object ["rows" .= (65536 :: Int), "contexts" .= (8192 :: Int), "pairs" .= (128 :: Int), "output_bytes" .= (134217728 :: Int)])
    (fail "AF diagnostic limits differ")
  maps <- fields .: "mappings" >>= mapM afMapping
  unless (not (null maps) && length maps <= 4096) (fail "AF mapping bound differs")
  unless (maps == expectedMappings) (fail "AF mapping catalogue differs from the selected SDK/model")
  rows <- fields .: "rows" :: Parser [Value]
  contexts <- fields .: "contexts" :: Parser [Value]
  rowCount <- fields .: "row_count"
  contextCount <- fields .: "context_count"
  unless (length rows <= 65536 && length contexts <= 8192 && rowCount == length rows && contextCount == length contexts
      && (action == ArmAfOwnership || not (null rows) && not (null contexts)))
    (fail "AF diagnostic event counts differ")
  AfBoundary begin initialWords <- fields .: "initial" >>= afBoundary
  final <- fields .: "final"
  AfBoundary end finalWords <- if action == ArmAfOwnership
    then do
      unless (final == Null && null rows && null contexts) (fail "AF armed receipt already contains capture events")
      pure (AfBoundary begin initialWords)
    else afBoundary final
  unless (and (zipWith (<=) begin end)) (fail "AF boundary clocks decrease")
  writes <- sequence [afWrite maps index row | (index, row) <- zip [0..] rows]
  let lower = sum (map toInteger begin)
      upper = sum (map toInteger end)
      bytesOf word = [fromIntegral ((toInteger word `div` (256 ^ offset)) `mod` 256) :: Word8 | offset <- [0..3 :: Int]]
      initialMemory = Map.fromList [(address + fromIntegral offset, byte) | (address, word) <- Map.toList initialWords, (offset, byte) <- zip [0 :: Int ..] (bytesOf word)]
  (_, _, _, replayed) <- foldM (\(previousClock, previousTick, previousCoreCounts, memory) (AfWrite clock tick address size before payload cpuCounts) -> do
    unless (clock >= previousClock && clock <= upper && tick >= previousTick) (fail "AF writes are reordered or outside boundaries")
    let currentCounts = maybe previousCoreCounts id cpuCounts
    unless (and (zipWith (<=) previousCoreCounts currentCounts) && and (zipWith (<=) currentCounts end))
      (fail "AF CPU write clocks are reordered or outside capture boundaries")
    let touched = [address + offset | offset <- [0..size-1]]
        allowed byte = Map.member byte initialMemory || any (\(low, high) -> byte >= low && byte < high) [(0x016fe9bc,0x016fec34),(0x017030d4,0x017030ec),(0x01706a38,0x01706e44)]
    unless (any allowed touched) (fail "AF write is outside captured regions")
    next <- foldM (\state (offset, byte) -> do
      let at = address + fromIntegral offset
      case Map.lookup at state of
        Nothing -> pure state
        Just old -> do
          unless (old == byte) (fail "AF write preimage differs from capture history")
          pure (Map.insert at (fromIntegral ((toInteger payload `div` (256 ^ offset)) `mod` 256)) state))
      memory (zip [0 :: Int ..] before)
    pure (clock, tick, currentCounts, next)) (lower, 0, begin, initialMemory) writes
  unless (all (\(address, word) -> and [Map.lookup (address + fromIntegral offset) replayed == Just byte | (offset, byte) <- zip [0 :: Int ..] (bytesOf word)]) (Map.toList finalWords))
    (fail "AF final snapshot differs from captured writes")
  _ <- foldM (\(previousCounts, previousCursor, pairs) (sequenceIndex, context) -> flip (withObject "AF context") context $ \row -> do
    afExactFields row ["sequence", "core", "pc", "word", "kind", "cpsr", "registers", "sp", "lr", "sampled_task", "entry", "counts", "cursor", "reads"]
    actualSequence <- row .: "sequence"
    core <- row .: "core" >>= afCoreIndex
    pc <- row .: "pc"
    word <- row .: "word"
    kind <- row .: "kind"
    cpsr <- row .: "cpsr" :: Parser Word32
    registers <- row .: "registers" :: Parser [Word32]
    sp <- row .: "sp"
    lr <- row .: "lr"
    counts <- row .: "counts" :: Parser [Word64]
    cursor <- row .: "cursor" :: Parser Int
    unless (actualSequence == sequenceIndex && length registers == 13 && length counts == 2 && cursor >= 0 && cursor <= length rows && cpsr .&. 0x01000220 == 0)
      (fail "AF context ordering or A32 state differs")
    unless (and (zipWith (<=) previousCounts counts) && and (zipWith (<=) counts end) && cursor >= previousCursor)
      (fail "AF context clocks or cursor are reordered")
    let at = sum (map toInteger counts)
        pairCount = pairs + if pc == 0x0102f1b0 then 1 else 0
    unless (pairCount <= (128 :: Int)
        && (cursor == 0 || let AfWrite clock _ _ _ _ _ _ = writes !! (cursor - 1) in clock <= at)
        && (cursor == length writes || let AfWrite clock _ _ _ _ _ _ = writes !! cursor in clock >= at))
      (fail "AF context cursor or pair bound differs")
    entryWindows <- row .: "entry" >>= afEntry core pc cpsr counts
    row .: "sampled_task" >>= afTaskSample core
    case [site | site@(AfSite bound _ _ _ _) <- sites, bound == pc] of
      [AfSite _ boundWord boundKind fixed dynamic] -> do
        unless (word == boundWord && kind == afSiteKindName boundKind) (fail "AF context instruction identity differs")
        specs <- mapM (\(AfRegisterRead register offset bytes) -> do
          let base = (registers ++ [sp, lr]) !! fromIntegral register
              address = toInteger base + toInteger offset
          unless (address >= 0 && address + toInteger bytes <= 0x100000000) (fail "AF register read overflows")
          pure (fromInteger address, bytes)) dynamic
        capturedReads <- row .: "reads" :: Parser [Value]
        unless (length capturedReads == length fixed + length specs) (fail "AF read mask is incomplete")
        observed <- zipWithM (\(isFixed, spec) -> afValidateRead maps core isFixed spec) (map ((,) True) fixed ++ map ((,) False) specs) capturedReads
        let matrices = [matrix | Just matrix <- observed]
        unless (length (nub matrices) <= 1) (fail "AF stopped-context TCM registers disagree")
        case matrices of
          [] -> pure ()
          firstObserved : _ -> do
            stoppedWindows <- mapM afTcmWindow (firstObserved !! fromEnum core)
            unless (stoppedWindows == entryWindows) (fail "AF payload TCM windows differ from the actual instruction entry")
      _ -> fail "AF context site is not bound"
    pure (counts, cursor, pairCount)) (begin, 0, 0) (zip [0 :: Int ..] contexts)
  pure ()) value

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
  , segmentAfOwnership :: Maybe AfOwnershipAction
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
      <*> segment .:? "af_ownership"
    when (isJust (segmentKeyPulse result) && any ((/= PacketCompletion) . stepCompletion) (segmentSteps result))
      (fail "presented-frame cannot combine with a segment key pulse")
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
    let siteReads = entryReads :: [SiteRead]
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
      , ("save_lifecycle", segmentSaveLifecycle a == segmentSaveLifecycle b)
      , ("af_ownership", segmentAfOwnership a == segmentAfOwnership b) ]
    assertion step = (stepKey step, stepExpected step, stepCompletion step)

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
