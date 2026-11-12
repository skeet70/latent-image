{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Emulation.ModalObservation
  ( ModalArray (..)
  , ControllerIndex
  , controllerIndexValue
  , Writer (..)
  , ArrayIntersection (..)
  , ArrayWrite (..)
  , ArrayWriteReport
  , arrayWrites
  , arrayWriteReportJson
  , modalArrayRange
  , summarizeArrayWrites
  , ModalDiagnostic (..)
  , ModalLimit
  , modalLimit
  , modalLimitValue
  , modalLifetimeObservationLimit
  , ModalStock
  , modalStock
  , ModalLinked
  , modalLinked
  , ModalScope
  , ModalHelpers (..)
  , modalScope
  , ModalRequest
  , modalRequest
  , modalRequestLimit
  , parseModalBeginRequest
  , parseModalBeginRequestForPackage
  , parseRecordedModalBeginRequest
  , modalBeginRequestJson
  , ModalPolicy (..)
  , ModalReport
  , modalReportEnabled
  , modalReportFailure
  , modalReportJson
  , parseModalReport
  , checkModalBegin
  , checkModalEnd
  , checkScopedModalRoute
  , ScopedModalSource
  , parseScopedModalSource
  , scopedModalRequest
  , scopedModalSourceJson
  , ModalLifetimeGuard (..)
  , ModalLifetimeSource
  , bindModalLifetimeSource
  , bindModalLifetimeSourceForPackage
  , stockSettingsWriters
  , ownedSettingsWriters
  , modalLifetimeSourceJson
  , lifetimeModalRequest
  , withRetiredExecution
  , withTask33StackGuard
  , parseModalLifetimeWatchAck
  , parseModalLifetimeWatch
  , checkCompletedModalLifetime
  , SettingsWrite
  , completedSettingsWrites
  , settingsWritesJson
  , parseSettingsWrites
  , compareSettingsWrites
  , FinalModalWatch
  , parseFinalModalWatch
  , finalModalWatchFromRecords
  , finalModalWatchSummary
  , modalReportDescribesFinalWatch
  , modalLifetimeCoverage
  , modalPhysicalCoverage
  , modalTaskIdentityVerified
  , modalLifetimeWatchRanges
  , checkModalLifetimeWatchSources
  , ModalRuntimeSource
  , bindModalRuntimeSource
  , modalRuntimeSourceJson
  , ModalInstallOrigin (..)
  , ModalUnloadRequest (..)
  , checkModalLifetimeRoute
  , lifetimeWatchAcknowledgement
  , checkLifetimeArrayAudit
  , modalLifetimeWriterLimit
  ) where

import Control.Monad (foldM, forM, forM_, unless, when)
import qualified ArmElf as ELF
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.List (find, nub, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (catMaybes, isJust)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word64)
import Data.Bits ((.&.))
import Emulation.WatchObservation
import Emulation.Sha256 (Sha256, sha256, sha256Hex, parseSha256Hex)
import qualified FujiAF.RamPush as Ram
import Emulation.LoaderCode (loaderSpans)
import qualified Emulation.ContextCatalogue as Catalogue
import qualified RamPushLayout as Layout
import qualified FujiAF.UpdateContainer as Update
import qualified Replacement.RtosTimer.RootPackage as Root
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as Startup
import Numeric (readHex)


data ModalArray = ModalStates | ModalEvents deriving (Eq, Ord, Show)
newtype ControllerIndex = ControllerIndex { controllerIndexValue :: Word16 } deriving (Eq, Ord, Show)
newtype OperationIndex = OperationIndex Int deriving (Eq, Show)
newtype WriteCount = WriteCount Int deriving (Eq, Show)

data Writer = CpuWriter CoreId WatchAddress | HostWriter | ModelBusWriter deriving (Eq, Ord, Show)

data ArrayIntersection = ArrayIntersection
  { writtenArray :: ModalArray
  , firstController :: ControllerIndex
  , lastController :: ControllerIndex
  } deriving (Eq, Show)

data ArrayWrite = ArrayWrite
  { writeOperation :: OperationIndex
  , writeSource :: Writer
  , writeIntersections :: [ArrayIntersection]
  } deriving (Eq, Show)

data ArrayWriteReport = ArrayWriteReport
  { arrayWrites :: [ArrayWrite]
  , observerWrites :: ExecutionWriteGate
  }

stateBegin, eventBegin, arraysEnd :: Integer
stateBegin = 0x01467588
eventBegin = 0x014676e4
arraysEnd = 0x01467840

modalArrayRange :: WatchRange
modalArrayRange = either error id (watchRange (watchAddress (fromInteger stateBegin)) (watchAddress (fromInteger arraysEnd)))

summarizeArrayWrites :: WatchLimit -> [WatchRange] -> [WatchRange] -> FixedWatchAck -> FixedWatchObservation -> Either String ArrayWriteReport
summarizeArrayWrites limit expectedWrites expectedFetches acknowledgement observation = do
  unless (covered expectedWrites stateBegin arraysEnd) (Left "modal array write classes do not cover both complete arrays")
  gate <- checkExecutionWrites ForbidProtectedWrites limit expectedWrites expectedFetches acknowledgement observation
  pure (ArrayWriteReport (writesInArrays observation) gate)

writesInArrays :: FixedWatchObservation -> [ArrayWrite]
writesInArrays observation =
  [ ArrayWrite (OperationIndex index) writer intersections
  | (index, record) <- zip [0 ..] (fixedWatchRecords observation)
  , Just (writer, physical, bytes) <- [writeAttribution record]
  , let intersections = intersectArrays physical bytes
  , not (null intersections)
  ]

covered :: [WatchRange] -> Integer -> Integer -> Bool
covered ranges low high = advance low (sortOn fst bounds)
 where
  bounds = [(toInteger begin, toInteger end) | range <- ranges, let (begin, end) = watchRangeBounds range]
  advance cursor _ | cursor >= high = True
  advance _ [] = False
  advance cursor ((begin, end) : rest)
    | end <= cursor = advance cursor rest
    | begin > cursor = False
    | otherwise = advance (max cursor end) rest

writeAttribution :: WatchRecord -> Maybe (Writer, Integer, Integer)
writeAttribution CpuRecord { cpuAccess = CpuWrite, cpuCore = core, cpuPc = pc, cpuPhysical = address, cpuBytes = bytes } =
  Just (CpuWriter core pc, toInteger (watchAddressValue address), toInteger (watchBytesValue bytes))
writeAttribution TransferRecord { transferAccess = HostWrite, transferPhysical = address, transferBytes = bytes } =
  Just (HostWriter, toInteger (watchAddressValue address), toInteger (watchBytesValue bytes))
writeAttribution TransferRecord { transferAccess = ModelBusWrite, transferPhysical = address, transferBytes = bytes } =
  Just (ModelBusWriter, toInteger (watchAddressValue address), toInteger (watchBytesValue bytes))
writeAttribution _ = Nothing

intersectArrays :: Integer -> Integer -> [ArrayIntersection]
intersectArrays physical bytes =
  [ ArrayIntersection kind (controller begin firstByte) (controller begin (lastByte - 1))
  | (kind, begin, end) <- [(ModalStates, stateBegin, eventBegin), (ModalEvents, eventBegin, arraysEnd)]
  , let firstByte = max begin physical
        lastByte = min end (physical + bytes)
  , firstByte < lastByte
  ]
 where
  controller begin address = ControllerIndex (fromInteger ((address - begin) `div` 2))

arrayWriteReportJson :: ArrayWriteReport -> Value
arrayWriteReportJson report = object
  [ "schema" .= (1 :: Int)
  , "scope" .= ("physical modal state/event array write attempts" :: String)
  , "controller_domain_proven" .= False
  , "array_write_operations" .= length (arrayWrites report)
  , "writers" .= [object ["writer" .= writerJson writer, "operations" .= count] | (writer, WriteCount count) <- M.toAscList counts]
  , "array_writes" .= map writeJson (arrayWrites report)
  , "observer_write_classification" .= executionWriteGateJson (observerWrites report)
  ]
 where
  counts = M.fromListWith add [(writeSource write, WriteCount 1) | write <- arrayWrites report]
  add (WriteCount left) (WriteCount right) = WriteCount (left + right)
  writeJson write = object
    [ "record_index" .= (case writeOperation write of OperationIndex index -> index)
    , "writer" .= writerJson (writeSource write)
    , "arrays" .= map intersectionJson (writeIntersections write)
    ]
  intersectionJson intersection = object
    [ "array" .= (case writtenArray intersection of ModalStates -> "state"; ModalEvents -> "event" :: String)
    , "first_controller" .= controllerIndexValue (firstController intersection)
    , "last_controller" .= controllerIndexValue (lastController intersection)
    ]

writerJson :: Writer -> Value
writerJson writer = case writer of
  CpuWriter core pc -> object
    [ "source" .= ("cpu-write" :: String)
    , "core" .= (case core of Core0 -> 0 :: Int; Core1 -> 1)
    , "pc" .= watchAddressValue pc
    ]
  HostWriter -> object ["source" .= ("host-write" :: String)]
  ModelBusWriter -> object ["source" .= ("model-bus-write" :: String)]

data ModalDiagnostic = NoModalDiagnostic | RetainedFilmModalDiagnostic deriving (Eq, Show)

data ModalLifetimeGuard = NoModalLifetimeGuard | ContinuousModalLifetimeGuard deriving (Eq, Show)
newtype ModalLimit = ModalLimit { modalLimitValue :: Word64 } deriving (Eq, Show)

modalLimit :: Word64 -> Either String ModalLimit
modalLimit value
  | value >= 1 && value <= 65536 = Right (ModalLimit value)
  | otherwise = Left "modal observation limit must be 1..65536"

-- | Installed-lifetime observation bound, equal to 'modalLifetimeWriterLimit'.
modalLifetimeObservationLimit :: ModalLimit
modalLifetimeObservationLimit = ModalLimit 1048576

newtype ModalStock = ModalStock B.ByteString deriving (Eq, Show)

modalStock :: B.ByteString -> Either String ModalStock
modalStock bytes = do
  unless (B.length bytes == 0xfc) (Left "modal stock source must contain the complete 0xfc bytes")
  pure (ModalStock bytes)

data ModalLinked = ModalLinked WatchAddress B.ByteString deriving (Eq, Show)

modalLinked :: WatchAddress -> B.ByteString -> Either String ModalLinked
modalLinked start bytes = do
  unless (not (B.null bytes) && B.length bytes <= 1048576
    && watchAddressValue start `mod` 4 == 0 && B.length bytes `mod` 4 == 0
    && toInteger (watchAddressValue start) + toInteger (B.length bytes) <= 0x100000000)
    (Left "modal linked source requires aligned complete bytes within the 32-bit address space")
  pure (ModalLinked start bytes)

data ModalHelpers = ModalHelpers
  { modalPrehookCode :: WatchRange
  , modalCallCode :: WatchRange
  } deriving (Eq, Show)
data ModalScope = ModalScope WatchRange WatchRange ModalHelpers (Maybe WatchAddress) deriving (Eq, Show)

modalScope :: ModalLinked -> WatchRange -> WatchRange -> ModalHelpers -> Maybe WatchAddress -> Either String ModalScope
modalScope (ModalLinked start bytes) seam owned helpers prehook = do
  let sourceLow = toInteger (watchAddressValue start)
      sourceHigh = sourceLow + toInteger (B.length bytes)
      contained range = let (low, high) = watchRangeBounds range
        in sourceLow <= toInteger low && toInteger high <= sourceHigh
          && low `mod` 4 == 0 && high `mod` 4 == 0
  unless (contained seam && contained owned) (Left "modal seam and owned extents must be aligned linked source ranges")
  let (seamLow, seamHigh) = watchRangeBounds seam
      (ownedLow, ownedHigh) = watchRangeBounds owned
  unless (seamHigh - seamLow == 56 && (seamHigh <= ownedLow || ownedHigh <= seamLow))
    (Left "modal seam must contain 56 bytes and be disjoint from owned code")
  unless ((seamHigh <= 0x00bfb028 || seamLow >= 0x00bfb124)
    && (ownedHigh <= 0x00bfb028 || ownedLow >= 0x00bfb124))
    (Left "modal seam and owned ranges overlap the resident stock dispatcher")
  let helperRanges = [modalPrehookCode helpers, modalCallCode helpers]
      allRanges = seam : owned : helperRanges
  unless (all contained helperRanges
    && all (\range -> let (low, high) = watchRangeBounds range in high <= 0x00bfb028 || low >= 0x00bfb124) helperRanges
    && and [not (rangesOverlap left right) | (index, left) <- zip [0 :: Int ..] allRanges, right <- drop (index + 1) allRanges])
    (Left "modal helpers must be complete aligned disjoint linked ranges outside stock code")
  forM_ prehook $ \address -> do
    let (helperStart, _) = watchRangeBounds (modalPrehookCode helpers)
    unless (fromIntegral (watchAddressValue address) == helperStart)
      (Left "modal owned prehook must equal the complete prehook helper entry")
  pure (ModalScope seam owned helpers prehook)

data ModalRequest = ModalRequest ModalStock (Maybe (ModalLinked, ModalScope)) ModalLimit (Maybe ModalLifetimeSource) deriving (Eq)

modalRequest :: ModalStock -> Maybe (ModalLinked, ModalScope) -> ModalLimit -> Either String ModalRequest
modalRequest stock scoped limit = do
  forM_ scoped $ \(linked, ModalScope seam owned helpers prehook) -> modalScope linked seam owned helpers prehook
  pure (ModalRequest stock scoped limit Nothing)

modalBeginRequestJson :: ModalRequest -> Value
modalBeginRequestJson (ModalRequest (ModalStock stock) scoped limit lifetime) = object
  ([ "command" .= ("modal-observation-begin" :: String)
   , "stock" .= B.unpack stock
   , "linked" .= fmap (\(ModalLinked start bytes, _) -> object ["start" .= watchAddressValue start, "bytes" .= B.unpack bytes]) scoped
   , "seam" .= fmap (\(_, ModalScope seam _ _ _) -> bounds seam) scoped
   , "owned" .= fmap (\(_, ModalScope _ owned _ _) -> bounds owned) scoped
   , "helpers" .= [bounds range | Just (_, ModalScope _ _ helpers _) <- [scoped], range <- [modalPrehookCode helpers, modalCallCode helpers]]
   , "limit" .= modalLimitValue limit
   ] ++ ["owned_prehook" .= watchAddressValue prehook | Just (_, ModalScope _ _ _ (Just prehook)) <- [scoped]]
     ++ maybe [] (\source -> ["context_sources" .= map sourceImageJson (lifetimeContext source), "lifetime" .= lifetimeRequestJson source]) lifetime)
 where
  bounds range = let (low, high) = watchRangeBounds range in [low, high]

data ModalPolicy = StockBaseline | ScopedOwned75 deriving (Eq, Show)

-- | Publication counts of the bank variants the lifetime guard binds.
stockPublicationCount :: Int
stockPublicationCount = Layout.publicationCountFor Layout.standardBankVariant

scopedPublicationCounts :: [Int]
scopedPublicationCounts = map Layout.publicationCountFor scopedBankVariants

scopedBankVariants :: [Layout.BankVariant]
scopedBankVariants = [Layout.modalScopedBankVariant, Layout.modalEngineBankVariant, Layout.modalRenderBankVariant, Layout.modalValueBankVariant]

guardPublicationCounts :: [Int]
guardPublicationCounts = stockPublicationCount : scopedPublicationCounts

newtype PublicationCardinality = PublicationCardinality Word64

newtype PropertyDeltaWords = PropertyDeltaWords Word32 deriving (Eq, Show)
data PropertyPublicationCounts = PropertyPublicationCounts PropertyDeltaWords Layout.PublicationCounts deriving (Eq, Show)

propertyCountsLayout :: PropertyPublicationCounts -> Layout.PublicationCounts
propertyCountsLayout (PropertyPublicationCounts _ counts) = counts

publicationCardinality :: Layout.BankVariant -> Maybe PropertyPublicationCounts -> PublicationCardinality
publicationCardinality variant counts = PublicationCardinality
  (fromIntegral (maybe (Layout.publicationCountFor variant) (Layout.publicationActualCount . propertyCountsLayout) counts))

parsePropertyCounts :: Layout.BankVariant -> Value -> Either String PropertyPublicationCounts
parsePropertyCounts variant value = do
  delta <- modalWord32 value
  unless (delta >= 1 && delta <= fromIntegral Layout.propertyCarrierWordBound)
    (Left "modal property_delta_words must be1..3767")
  layout <- Layout.propertyPublicationLayout variant
  counts <- Layout.propertyPublicationCounts layout (fromIntegral delta)
  pure (PropertyPublicationCounts (PropertyDeltaWords delta) counts)

propertyCountsFromRuntime :: Layout.PublicationCounts -> Either String PropertyPublicationCounts
propertyCountsFromRuntime counts = do
  let delta = Layout.publicationActualSetupCount counts - Layout.publicationSetupCount + 18
  parsed <- parsePropertyCounts Layout.modalRenderBankVariant (Number (fromIntegral delta))
  unless (propertyCountsLayout parsed == counts) (Left "modal runtime property counts disagree with their bounded delta")
  pure parsed

propertyDeltaValue :: PropertyPublicationCounts -> Value
propertyDeltaValue (PropertyPublicationCounts (PropertyDeltaWords delta) _) = Number (fromIntegral delta)

validatePropertyPublicationPlan :: WatchRange -> Maybe PropertyPublicationCounts -> [PublicationBinding] -> Either String ()
validatePropertyPublicationPlan _ Nothing _ = Right ()
validatePropertyPublicationPlan runtime (Just plan@(PropertyPublicationCounts (PropertyDeltaWords delta) _)) rows = do
  let counts = propertyCountsLayout plan
      (low, high) = watchRangeBounds runtime
      addressOf (PublicationBinding address _ _) = watchAddressValue address
      propertyRows = filter (\row -> addressOf row >= 0x62c120 && addressOf row < 0x62fbfc) rows
      obsolete row = let address = addressOf row in (address >= 0x61eac4 && address < 0x61eae8) || (address >= 0x625064 && address < 0x625088)
      changed (PublicationBinding _ old new) = old /= new
  unless (high - low == 0x3dc80
    && length rows == Layout.publicationActualCount counts
    && length propertyRows == fromIntegral delta
    && all changed propertyRows && not (any obsolete rows))
    (Left "modal property publication plan differs from its runtime, carrier delta or retired table")

routePublicationCounts :: Maybe scope -> [Int]
routePublicationCounts = maybe [stockPublicationCount] (const scopedPublicationCounts)

newtype ModalCount = ModalCount Word64 deriving (Eq, Show)
newtype CapturedController = CapturedController Word32 deriving (Eq, Show)
newtype CapturedState = CapturedState Word32 deriving (Eq, Show)
newtype CapturedEvent = CapturedEvent Word32 deriving (Eq, Show)
newtype ModalFailure = ModalFailure T.Text deriving (Eq, Show)

data ModalCounts = ModalCounts
  { modalObservations :: ModalCount
  , modalEntries :: ModalCount
  , modalOwnedHandoffs :: ModalCount
  , modalFallbackHandoffs :: ModalCount
  , modalReturns :: ModalCount
  , modalActiveFrames :: ModalCount
  , modalPeakCoreDepth :: ModalCount
  } deriving (Eq, Show)

data ModalPoint = ModalPoint CoreId WatchAddress Instructions deriving (Eq, Show)
data ModalDomain = ModalDomain ModalPoint CapturedController CapturedState CapturedEvent deriving (Eq, Show)
data ModalReport = ModalReport
  { modalReportEnabled :: Bool
  , modalReportPolicy :: ModalPolicy
  , modalReportCounts :: ModalCounts
  , modalStockFetches :: [ModalPoint]
  , modalCapturedDomains :: [ModalDomain]
  , modalGuard :: Maybe PhysicalGuard
  , modalTaskEntries :: [TaskEntry]
  , modalLifetimeReceipt :: Maybe LifetimeReceipt
  , modalWatchSummary :: Maybe ModalWatchSummary
  , modalPhysicalCoverage :: Bool
  , modalTaskIdentityVerified :: Bool
  , modalContextSourceSpans :: ModalCount
  , modalFailure :: Maybe ModalFailure
  } deriving (Eq, Show)

modalReportFailure :: ModalReport -> Maybe T.Text
modalReportFailure = fmap (\(ModalFailure text) -> text) . modalFailure

policyText :: ModalPolicy -> T.Text
policyText StockBaseline = "stock-baseline"
policyText ScopedOwned75 = "scoped-owned75"

modalReportJson :: ModalReport -> Value
modalReportJson report = object
  [ "enabled" .= modalReportEnabled report
  , "policy" .= policyText (modalReportPolicy report)
  , "counts" .= countsJson (modalReportCounts report)
  , "stock_fetches" .= map pointJson (modalStockFetches report)
  , "captured_domains" .= map domainJson (modalCapturedDomains report)
  , "failure" .= modalReportFailure report
  , "physical_fetch_coverage" .= modalPhysicalCoverage report
  , "task_identity_verified" .= modalTaskIdentityVerified report
  , "physical_guard" .= fmap physicalGuardJson (modalGuard report)
  , "task_entries" .= map taskEntryJson (modalTaskEntries report)
  , "lifetime" .= fmap lifetimeReceiptJson (modalLifetimeReceipt report)
  , "watch" .= fmap (\(ModalWatchSummary _ raw) -> raw) (modalWatchSummary report)
  , "context_source_spans" .= countValue (modalContextSourceSpans report)
  ]
 where
  countsJson counts = object
    [ "observations" .= countValue (modalObservations counts)
    , "entries" .= countValue (modalEntries counts)
    , "owned_handoffs" .= countValue (modalOwnedHandoffs counts)
    , "fallback_handoffs" .= countValue (modalFallbackHandoffs counts)
    , "returns" .= countValue (modalReturns counts)
    , "active_frames" .= countValue (modalActiveFrames counts)
    , "peak_core_depth" .= countValue (modalPeakCoreDepth counts)
    ]
  pointFields (ModalPoint core pc instructions) =
    [ "core" .= (case core of Core0 -> 0 :: Int; Core1 -> 1)
    , "pc" .= watchAddressValue pc
    , "instructions" .= instructionsValue instructions ]
  pointJson = object . pointFields
  domainJson (ModalDomain point (CapturedController controller) (CapturedState state) (CapturedEvent event)) =
    object (pointFields point ++ ["controller" .= controller, "state" .= state, "event" .= event])

countValue :: ModalCount -> Word64
countValue (ModalCount count) = count

modalField :: K.Key -> Value -> Either String Value
modalField name (Object fields) = maybe (Left ("modal response lacks " ++ K.toString name)) Right (KM.lookup name fields)
modalField _ _ = Left "modal response is not an object"

modalKeys :: [K.Key] -> Value -> Either String ()
modalKeys expected (Object fields) = unless (sort expected == sort (KM.keys fields)) (Left "modal response keys differ from the schema")
modalKeys _ _ = Left "modal response is not an object"

modalWord :: Value -> Either String Word64
modalWord (Number value) = maybe (Left "modal counter is not a u64 integer") Right (toBoundedInteger value)
modalWord _ = Left "modal counter is not an integer"

modalWord32 :: Value -> Either String Word32
modalWord32 value = do
  word <- modalWord value
  unless (word <= 0xffffffff) (Left "modal register or PC exceeds u32")
  pure (fromIntegral word)

modalBool :: Value -> Either String Bool
modalBool (Bool value) = Right value
modalBool _ = Left "modal flag is not Boolean"

modalList :: ModalLimit -> Value -> Either String [Value]
modalList limit (Array values) = do
  unless (toInteger (V.length values) <= toInteger (modalLimitValue limit)) (Left "modal evidence exceeds the observation limit")
  pure (V.toList values)
modalList _ _ = Left "modal evidence is not an array"

parseModalReport :: ModalLimit -> Value -> Either String ModalReport
parseModalReport limit value = do
  modalKeys ["enabled", "policy", "counts", "stock_fetches", "captured_domains", "failure", "physical_fetch_coverage",
    "task_identity_verified", "physical_guard", "task_entries", "lifetime", "watch", "context_source_spans"] value
  enabled <- modalField "enabled" value >>= modalBool
  policy <- modalField "policy" value >>= \entry -> case entry of
    String "stock-baseline" -> Right StockBaseline
    String "scoped-owned75" -> Right ScopedOwned75
    _ -> Left "unknown modal policy"
  physicalCoverage <- modalField "physical_fetch_coverage" value >>= modalBool
  taskVerified <- modalField "task_identity_verified" value >>= modalBool
  contextSpans <- ModalCount <$> (modalField "context_source_spans" value >>= modalWord)
  unless (countValue contextSpans `elem` [0,16]) (Left "modal context source span count is neither zero nor complete16")
  guard <- modalField "physical_guard" value >>= parseNullable parsePhysicalGuard
  tasks <- modalField "task_entries" value >>= modalList limit >>= mapM parseTaskEntry
  lifetime <- modalField "lifetime" value >>= parseNullable parseLifetimeReceipt
  watch <- modalField "watch" value >>= parseNullable parseModalWatchSummary
  countsValue <- modalField "counts" value
  modalKeys ["observations", "entries", "owned_handoffs", "fallback_handoffs", "returns", "active_frames", "peak_core_depth"] countsValue
  let count key = ModalCount <$> (modalField key countsValue >>= modalWord)
  counts <- ModalCounts <$> count "observations" <*> count "entries" <*> count "owned_handoffs"
    <*> count "fallback_handoffs" <*> count "returns" <*> count "active_frames" <*> count "peak_core_depth"
  let observations = countValue (modalObservations counts)
  unless (observations <= modalLimitValue limit) (Left "modal observation counter exceeds the requested limit")
  forM_ [modalEntries counts, modalOwnedHandoffs counts, modalFallbackHandoffs counts, modalReturns counts,
        modalActiveFrames counts, modalPeakCoreDepth counts] $ \counter ->
    unless (countValue counter <= observations) (Left "modal counter exceeds observations")
  unless (countValue (modalActiveFrames counts) <= 64 && countValue (modalPeakCoreDepth counts) <= 32)
    (Left "modal frame counters exceed the two-core depth bound")
  fetches <- modalField "stock_fetches" value >>= modalList limit >>= mapM parsePoint
  domains <- modalField "captured_domains" value >>= modalList limit >>= mapM parseDomain
  failure <- modalField "failure" value >>= \entry -> case entry of
    Null -> Right Nothing
    String text | not (T.null text) -> Right (Just (ModalFailure text))
    _ -> Left "modal failure must be nonempty text or null"
  unless (not physicalCoverage || failure == Nothing && maybe True ((== Nothing) . receiptFailure) lifetime
    && maybe False healthyPhysicalGuard guard && (lifetime == Nothing || watch /= Nothing))
    (Left "modal report makes an unsupported physical coverage claim")
  let taskCount = toInteger (length tasks)
      ownedCount = toInteger (countValue (modalOwnedHandoffs counts))
      entryCount = toInteger (countValue (modalEntries counts))
  unless (not taskVerified || policy == ScopedOwned75 && physicalCoverage && countValue contextSpans == 16 && taskCount > 0 && ownedCount <= taskCount && taskCount <= entryCount
    && (enabled || taskCount == ownedCount))
    (Left "modal report makes an unsupported task identity claim")
  pure (ModalReport enabled policy counts fetches domains guard tasks lifetime watch physicalCoverage taskVerified contextSpans failure)
 where
  pointFields entry = do
    core <- modalField "core" entry >>= modalWord >>= \number -> case number of
      0 -> Right Core0
      1 -> Right Core1
      _ -> Left "modal core is not 0 or 1"
    pc <- modalField "pc" entry >>= modalWord32
    unless (pc `mod` 4 == 0) (Left "modal observation PC is not word aligned")
    instructions <- Instructions <$> (modalField "instructions" entry >>= modalWord)
    pure (ModalPoint core (watchAddress pc) instructions)
  parsePoint entry = do
    modalKeys ["core", "pc", "instructions"] entry
    point <- pointFields entry
    let ModalPoint _ pc _ = point
    unless (watchAddressValue pc >= 0x00bfb02c && watchAddressValue pc < 0x00bfb124)
      (Left "modal stock fetch PC is outside the canonical stock body")
    pure point
  parseDomain entry = do
    modalKeys ["core", "pc", "instructions", "controller", "state", "event"] entry
    ModalDomain <$> pointFields entry
      <*> (CapturedController <$> (modalField "controller" entry >>= modalWord32))
      <*> (CapturedState <$> (modalField "state" entry >>= modalWord32))
      <*> (CapturedEvent <$> (modalField "event" entry >>= modalWord32))

checkModalBegin :: ModalRequest -> ModalReport -> Either String ()
checkModalBegin (ModalRequest _ scoped _ lifetime) report = do
  unless (modalReportEnabled report) (Left "modal observation did not enable")
  unless (modalReportPolicy report == maybe StockBaseline (const ScopedOwned75) scoped) (Left "modal policy differs from the request")
  unless (countValue (modalContextSourceSpans report) == maybe 0 (fromIntegral . length . lifetimeContext) lifetime)
    (Left "modal report context source count differs from request")
  forM_ lifetime $ \binding -> do
    unless (modalPhysicalCoverage report) (Left "modal lifetime physical guard did not arm")
    checkTask33StackGuardReceipt binding (modalLifetimeReceipt report)
    receipt <- maybe (Left "modal lifetime did not return its initial receipt") Right (modalLifetimeReceipt report)
    unless (receiptPropertyCounts receipt == lifetimePropertyCounts binding
      && countValue (receiptPublicationCount receipt) == fromIntegral (length (lifetimePublications binding)))
      (Left "modal initial receipt publication plan differs from source")
  forM_ (modalReportFailure report) (Left . T.unpack)

checkModalEnd :: ModalReport -> Either String ()
checkModalEnd report = do
  unless (not (modalReportEnabled report)) (Left "modal observation remains active")
  unless (countValue (modalActiveFrames (modalReportCounts report)) == 0) (Left "modal observation ended with active frames")
  forM_ (modalReportFailure report) (Left . T.unpack)

checkScopedModalRoute :: ModalReport -> Either String ()
checkScopedModalRoute report = do
  checkModalEnd report
  unless (modalReportPolicy report == ScopedOwned75) (Left "modal route requires scoped-owned75 policy")
  unless (modalLifetimeCoverage report && modalTaskIdentityVerified report)
    (Left "scoped modal route requires completed physical and task lifetime coverage")
  unless (maybe False (\receipt -> isJust (receiptPropertyCounts receipt)
    || fromIntegral (countValue (receiptPublicationCount receipt)) `elem` scopedPublicationCounts) (modalLifetimeReceipt report))
    (Left "scoped modal route requires a complete scoped publication vector")
  let counts = modalReportCounts report
      owned = toInteger (countValue (modalOwnedHandoffs counts))
      fallback = toInteger (countValue (modalFallbackHandoffs counts))
      entries = toInteger (countValue (modalEntries counts))
      returns = toInteger (countValue (modalReturns counts))
  unless (owned > 0 && fallback > 0) (Left "modal route requires owned and fallback handoffs")
  unless (entries == owned + fallback && entries == returns) (Left "modal route entries, handoffs and returns are unbalanced")
  unless (any controller75 (modalCapturedDomains report)) (Left "modal route lacks a captured controller75 domain")
 where
  controller75 (ModalDomain _ (CapturedController controller) _ _) = controller == 75

data BeginRequestFormat = CurrentBeginRequest | RecordedStockBeginRequest deriving (Eq)

parseModalBeginRequest :: Value -> Either String ModalRequest
parseModalBeginRequest = parseModalBeginRequestUsing CurrentBeginRequest Catalogue.parseContextCatalogue

parseModalBeginRequestForPackage :: Root.RootPackage -> Value -> Either String ModalRequest
parseModalBeginRequestForPackage package =
  parseModalBeginRequestUsing CurrentBeginRequest (Catalogue.parseContextCatalogueForPackage package)

type CatalogueParser = Value -> [Catalogue.ContextSourceSlice] -> Either String Catalogue.ContextCatalogue

parseModalBeginRequestUsing :: BeginRequestFormat -> CatalogueParser -> Value -> Either String ModalRequest
parseModalBeginRequestUsing format parseCatalogue value = do
  let required = ["command", "stock", "linked", "seam", "owned", "limit"]
      optional = case value of Object fields -> [key | key <- ["owned_prehook", "helpers", "context_sources", "lifetime"], KM.member key fields]; _ -> []
  modalKeys (required ++ optional) value
  unless (modalField "command" value == Right (String "modal-observation-begin")) (Left "not a modal begin request")
  stock <- modalField "stock" value >>= byteArray 0xfc >>= modalStock
  rawLimit <- modalField "limit" value >>= modalWord
  limit <- case (optionalField "context_sources" value, optionalField "lifetime" value) of
    (Nothing, Nothing) -> modalLimit rawLimit
    _ | rawLimit == modalLimitValue modalLifetimeObservationLimit -> Right modalLifetimeObservationLimit
      | otherwise -> Left "modal lifetime request requires its fixed observation bound"
  linkedValue <- modalField "linked" value
  seamValue <- modalField "seam" value
  ownedValue <- modalField "owned" value
  prehook <- case value of
    Object fields -> case KM.lookup "owned_prehook" fields of
      Nothing -> Right Nothing
      Just Null -> Right Nothing
      Just entry -> Just . watchAddress <$> modalWord32 entry
    _ -> Left "modal request is not an object"
  helpers <- case optionalField "helpers" value of
    Nothing -> Right []
    Just entry -> modalList (either error id (modalLimit 2)) entry >>= mapM parseExtent
  scoped <- case (linkedValue, seamValue, ownedValue) of
    (Null, Null, Null) -> do
      unless (prehook == Nothing) (Left "baseline modal request cannot capture an owned prehook")
      unless (null helpers) (Left "baseline modal request cannot bind owned helper code")
      pure Nothing
    (_, Null, _) -> Left "scoped modal request lacks a seam"
    (_, _, Null) -> Left "scoped modal request lacks owned code"
    (Null, _, _) -> Left "scoped modal request lacks linked source"
    _ -> do
      modalKeys ["start", "bytes"] linkedValue
      start <- watchAddress <$> (modalField "start" linkedValue >>= modalWord32)
      bytes <- modalField "bytes" linkedValue >>= byteArray 1048576
      linked <- modalLinked start bytes
      seam <- parseExtent seamValue
      owned <- parseExtent ownedValue
      helperExtents <- case helpers of
        [prehookCode, callCode] -> Right (ModalHelpers prehookCode callCode)
        _ -> Left "scoped modal request requires both complete helper extents"
      scope <- modalScope linked seam owned helperExtents prehook
      pure (Just (linked, scope))
  request <- modalRequest stock scoped limit
  case (optionalField "context_sources" value, optionalField "lifetime" value) of
    (Nothing, Nothing) -> pure request
    (Just context, Just lifetime) -> parseLifetimeBinding format parseCatalogue scoped context lifetime >>= lifetimeModalRequest request
    _ -> Left "modal lifetime request requires both context sources and lifetime"
 where
  byteArray = parseModalBytes
  parseExtent = parseModalExtent

parseRecordedModalBeginRequest :: Value -> Either String ModalRequest
parseRecordedModalBeginRequest observation = do
  schema <- modalField "schema" observation >>= modalWord
  unless (schema == 2) (Left "recorded modal observation schema is not 2")
  request <- modalField "request" observation
  lifetime <- modalField "lifetime" request
  case optionalField "context_catalogue" lifetime of
    Just _ -> parseModalBeginRequest request
    Nothing -> do
      binding <- modalField "source" observation >>= modalField "lifetime"
      first <- modalField "loader_a_sha256" binding
      second <- modalField "loader_b_sha256" binding
      Catalogue.validateRecordedStockLoaderDigests first second
      context <- modalField "context_sources" request
      declaredLifetime <- modalField "lifetime" binding
      let withoutReadonly (Object fields) = Right (KM.delete "readonly" fields)
          withoutReadonly _ = Left "recorded lifetime is not an object"
      declaredFields <- withoutReadonly declaredLifetime
      requestFields <- withoutReadonly lifetime
      unless (modalField "context_sources" binding == Right context && declaredFields == requestFields)
        (Left "recorded stock lifetime source differs from its request")
      decoded@(ModalRequest _ scoped _ source) <-
        parseModalBeginRequestUsing RecordedStockBeginRequest Catalogue.parseContextCatalogue request
      bound <- maybe (Left "recorded stock request has no lifetime source") Right source
      declaredImages <- modalField "readonly" declaredLifetime
        >>= modalList (either error id (modalLimit 64)) >>= mapM parseSourceImage
      let imageStart (SourceImage start _) = watchAddressValue start
          images = lifetimeContext bound
          completeImages = lifetimeReadonly bound
            ++ [SourceImage start bytes | Just (ModalLinked start bytes, _) <- [scoped]]
      unless (sortOn imageStart declaredImages == sortOn imageStart completeImages)
        (Left "recorded stock readonly images differ from the complete request source")
      forM_ Catalogue.stockContextInstructions $ \instruction -> do
        let pc = watchAddressValue (Catalogue.catalogueInstructionAddress instruction)
            slices = [B.take 4 (B.drop (fromInteger offset) bytes)
              | SourceImage start bytes <- images
              , let offset = toInteger pc - toInteger (watchAddressValue start)
              , offset >=0, offset +4 <= toInteger (B.length bytes)]
        unless (slices == [Catalogue.catalogueInstructionBytes instruction])
          (Left "recorded stock context instruction differs from the stock catalogue")
      pure decoded

modalRequestLimit :: ModalRequest -> ModalLimit
modalRequestLimit (ModalRequest _ _ limit _) = limit

data ScopedModalSource = ScopedModalSource ModalLinked ModalScope Sha256 Sha256

parseScopedModalSource :: WatchAddress -> B.ByteString -> B.ByteString -> Value -> Either String ScopedModalSource
parseScopedModalSource runtimeStart runtime elfBytes metadata = do
  modalKeys ["schema", "global_admission", "controller", "stock_entry", "stock_resume", "runtime_sha256", "bound_elf"] metadata
  unless (modalField "schema" metadata == Right (Number 1)
    && modalField "global_admission" metadata == Right (Bool False)
    && modalField "controller" metadata == Right (Number 75)
    && modalField "stock_entry" metadata == Right (String "0x00bfb028")
    && modalField "stock_resume" metadata == Right (String "0x00bfb02c"))
    (Left "modal scope metadata differs from the scoped75 contract")
  let runtimeDigest = sha256 runtime
      elfDigest = sha256 elfBytes
  unless (modalField "runtime_sha256" metadata == Right (String (T.pack (sha256Hex runtimeDigest))))
    (Left "modal scope runtime digest differs from the actual relinked runtime")
  bound <- modalField "bound_elf" metadata
  modalKeys ["sha256", "functions"] bound
  unless (modalField "sha256" bound == Right (String (T.pack (sha256Hex elfDigest))))
    (Left "modal scope ELF digest differs from the actual relinked ELF")
  functions <- modalField "functions" bound
  modalKeys ["rb_modal_entry", "rb_modal_dispatch", "rb_modal_call_prehook", "rb_modal_call"] functions
  elf <- ELF.parseElf elfBytes
  text <- case filter ((== ".text") . ELF.sectionName) (ELF.elfSections elf) of
    [section] | ELF.sectionAllocated section && ELF.sectionExecutable section && not (ELF.sectionWritable section) -> Right section
    _ -> Left "modal linked ELF requires one allocated read-only executable .text"
  let textStart = ELF.sectionAddress text
      textSize = ELF.sectionSize text
      textEnd = toInteger textStart + toInteger textSize
      textBytes = B.take (fromIntegral textSize) (B.drop (fromIntegral (ELF.sectionOffset text)) elfBytes)
      runtimeLow = toInteger (watchAddressValue runtimeStart)
      runtimeHigh = runtimeLow + toInteger (B.length runtime)
  linked <- modalLinked (watchAddress textStart) textBytes
  unless (B.length textBytes == fromIntegral textSize && runtimeLow <= toInteger textStart && textEnd <= runtimeHigh)
    (Left "modal ELF .text is truncated or outside the relinked runtime")
  unless (B.take (B.length textBytes) (B.drop (fromInteger (toInteger textStart - runtimeLow)) runtime) == textBytes)
    (Left "modal ELF .text differs from uploaded runtime instruction bytes")
  let extent name = do
        entry <- modalField name functions
        modalKeys ["address", "size"] entry
        address <- modalField "address" entry >>= parseHexAddress
        bytes <- modalField "size" entry >>= modalWord32
        let end = toInteger address + toInteger bytes
        unless (bytes > 0 && address `mod` 4 == 0 && bytes `mod` 4 == 0
          && textStart <= address && end <= textEnd && end <= 0xffffffff)
          (Left "modal manifest function is outside aligned linked .text")
        case filter ((== K.toString name) . ELF.symbolName) (ELF.elfSymbols elf) of
          [symbol] | ELF.symbolKind symbol == ELF.FunctionSymbol && ELF.symbolValue symbol == address
            && ELF.symbolSize symbol == bytes
            , ELF.InSection sectionIndex <- ELF.symbolSection symbol
            , sectionIndex >= 0 && sectionIndex < length (ELF.elfSections elf)
            , ELF.elfSections elf !! sectionIndex == text -> pure ()
          _ -> Left "modal manifest function differs from its bound ELF symbol"
        watchRange (watchAddress address) (watchAddress (fromInteger end))
  seam <- extent "rb_modal_entry"
  owned <- extent "rb_modal_dispatch"
  prehook <- extent "rb_modal_call_prehook"
  call <- extent "rb_modal_call"
  let (prehookLow, _) = watchRangeBounds prehook
  scope <- modalScope linked seam owned (ModalHelpers prehook call) (Just (watchAddress (fromIntegral prehookLow)))
  pure (ScopedModalSource linked scope elfDigest runtimeDigest)
 where
  parseHexAddress (String text)
    | T.length text == 10 && T.take 2 text == "0x"
      && T.all (`elem` ("0123456789abcdef" :: String)) (T.drop 2 text)
      , [(address, "")] <- readHex (T.unpack (T.drop 2 text)) = Right address
  parseHexAddress _ = Left "modal metadata address must be eight-digit lowercase u32 hex"

scopedModalRequest :: ModalStock -> ModalLimit -> ScopedModalSource -> Either String ModalRequest
scopedModalRequest stock limit (ScopedModalSource linked scope _ _) = modalRequest stock (Just (linked, scope)) limit

scopedModalSourceJson :: ScopedModalSource -> Value
scopedModalSourceJson (ScopedModalSource (ModalLinked start bytes) (ModalScope seam owned helpers prehook) elfDigest runtimeDigest) = object
  [ "bound_elf_sha256" .= sha256Hex elfDigest
  , "runtime_sha256" .= sha256Hex runtimeDigest
  , "linked_text_start" .= watchAddressValue start
  , "linked_text_bytes" .= B.length bytes
  , "seam" .= bounds seam
  , "owned" .= bounds owned
  , "owned_prehook" .= fmap watchAddressValue prehook
  , "helpers" .= map bounds [modalPrehookCode helpers, modalCallCode helpers]
  , "global_admission" .= False
  ]
 where
  bounds range = let (low, high) = watchRangeBounds range in [low, high]

data SourceImage = SourceImage WatchAddress B.ByteString deriving (Eq, Show)
newtype InstructionWord = InstructionWord Word32 deriving (Eq, Show)
data SourceInstruction = SourceInstruction WatchAddress InstructionWord deriving (Eq, Show)
data PublicationBinding = PublicationBinding WatchAddress Ram.WordValue Ram.WordValue deriving (Eq, Show)
data PublicationRoutine = PublicationRoutine SourceInstruction SourceInstruction deriving (Eq, Show)
type ContextCatalogue = Catalogue.ContextCatalogueKind
data CatalogueBinding = RuntimeContextUnbound | SourceContextBound Catalogue.ContextCatalogue deriving (Eq)

contextCatalogueTag :: ContextCatalogue -> T.Text
contextCatalogueTag Catalogue.StockContextCatalogue = "stock"
contextCatalogueTag Catalogue.OwnedUnionContextCatalogue = "owned_union"

catalogueBindingValue :: CatalogueBinding -> Value
catalogueBindingValue RuntimeContextUnbound = object
  ["source_bound" .= False, "installed_lifetime_admitted" .= False]
catalogueBindingValue (SourceContextBound catalogue) = Catalogue.contextCatalogueProvenance catalogue
data ModalLifetimeSource = ModalLifetimeSource
  { lifetimeVariant :: Layout.BankVariant
  , lifetimeCatalogue :: ContextCatalogue
  , lifetimeRuntime :: WatchRange
  , lifetimeResident :: WatchRange
  , lifetimeReadonly :: [SourceImage]
  , lifetimeInstall :: PublicationRoutine
  , lifetimeRollback :: PublicationRoutine
  , lifetimePublications :: [PublicationBinding]
  , lifetimeContext :: [SourceImage]
  , lifetimeElfDigest :: Maybe Sha256
  , lifetimeRuntimeDigest :: Maybe Sha256
  , lifetimeManifestDigest :: Maybe Sha256
  -- | Stock code no CPU may fetch through any alias from the first through the
  -- last publication-writer row.
  , lifetimeRetired :: [WatchRange]
  -- | Code whose value-changing live-settings stores the guard admits and records.
  , lifetimeSettingsWriters :: [WatchRange]
  -- | Reloads whose copy stores the guard admits between a core's call and return fetches.
  , lifetimeSettingsReloads :: [SettingsReload]
  , lifetimeTask33StackGuard :: Maybe Task33StackGuard
  , lifetimeLoaderDigests :: Maybe (Sha256, Sha256)
  , lifetimeCatalogueBinding :: CatalogueBinding
  , lifetimePropertyCounts :: Maybe PropertyPublicationCounts
  } deriving (Eq)

newtype Task33StackGuard = Task33StackGuard B.ByteString deriving (Eq, Show)

withTask33StackGuard :: B.ByteString -> ModalLifetimeSource -> Either String ModalLifetimeSource
withTask33StackGuard baseline source = do
  unless (B.length baseline == 64) (Left "task33 stack guard requires exactly 64 baseline bytes")
  pure source { lifetimeTask33StackGuard = Just (Task33StackGuard baseline) }

task33StackGuardRange :: WatchRange
task33StackGuardRange = either error id (watchRange (watchAddress 0x00a4ce00) (watchAddress 0x00a4ce40))

task33StackGuardProfile :: [(K.Key, Word32)]
task33StackGuardProfile =
  [ ("node", 0x4006060c), ("creation", 0x4005c3ac), ("entry", 0x01109130)
  , ("base", 0x00a4ce00), ("size", 4096), ("guard_bytes", 64) ]

task33StackGuardFields :: Task33StackGuard -> [(K.Key, Value)]
task33StackGuardFields (Task33StackGuard baseline) =
  [key .= value | (key, value) <- task33StackGuardProfile] ++ ["baseline" .= B.unpack baseline]

task33StackGuardJson :: Task33StackGuard -> Value
task33StackGuardJson = object . task33StackGuardFields

parseTask33StackGuard :: Value -> Either String Task33StackGuard
parseTask33StackGuard value = do
  modalKeys ("baseline" : map fst task33StackGuardProfile) value
  forM_ task33StackGuardProfile $ \(key, expected) -> do
    actual <- modalField key value >>= modalWord32
    unless (actual == expected) (Left "task33 stack guard differs from the pinned task profile")
  baseline <- modalField "baseline" value >>= parseModalBytes 64
  unless (B.length baseline == 64) (Left "task33 stack guard requires exactly 64 baseline bytes")
  pure (Task33StackGuard baseline)

newtype Task33StackGuardReceipt = Task33StackGuardReceipt Task33StackGuard deriving (Eq, Show)

task33StackGuardReceiptJson :: Task33StackGuardReceipt -> Value
task33StackGuardReceiptJson (Task33StackGuardReceipt guard) = object
  (task33StackGuardFields guard ++
    [ "scope" .= String "diagnostic_only_bottom_guard_write_attempts"
    , "priority" .= (8 :: Word32), "task_id" .= (33 :: Word32), "created" .= (1 :: Word32) ])

parseTask33StackGuardReceipt :: Value -> Either String Task33StackGuardReceipt
parseTask33StackGuardReceipt value = do
  let extra = ["scope", "priority", "task_id", "created"]
  modalKeys (extra ++ ("baseline" : map fst task33StackGuardProfile)) value
  scope <- modalField "scope" value
  unless (scope == String "diagnostic_only_bottom_guard_write_attempts") (Left "unknown task33 guard receipt scope")
  forM_ [("priority", 8), ("task_id", 33), ("created", 1)] $ \(key, expected) -> do
    actual <- modalField key value >>= modalWord32
    unless (actual == expected) (Left "task33 guard receipt differs from the pinned descriptor")
  fields <- case value of Object entries -> Right entries; _ -> Left "task33 guard receipt is not an object"
  Task33StackGuardReceipt <$> parseTask33StackGuard (Object (foldr KM.delete fields extra))

checkTask33StackGuardReceipt :: ModalLifetimeSource -> Maybe LifetimeReceipt -> Either String ()
checkTask33StackGuardReceipt binding receipt = unless
  ((receipt >>= receiptTask33StackGuard) == (Task33StackGuardReceipt <$> lifetimeTask33StackGuard binding))
  (Left "task33 guard receipt differs from its source-bound request")

-- | The stock property setter `0x624694` with its jump-table arms, fenced by the
-- next settings-manifest entry `0x6309c4` (E-SETTINGS-SERVICE-MANIFEST-150 claims
-- no end for the setter).
stockPropertySetter :: WatchRange
stockPropertySetter = either error id (watchRange (watchAddress 0x00624694) (watchAddress 0x006309c4))

-- | The stock image's live-settings writer.
stockSettingsWriters :: [WatchRange]
stockSettingsWriters = [stockPropertySetter]

-- | The store functions the owned storage boundary exports.
ownedStorageStoreFunctions :: [String]
ownedStorageStoreFunctions = ["xm1_storage_store8", "xm1_storage_store16", "xm1_storage_store32"]

-- | The code the owned image admits to change a live setting: the complete extents
-- of its storage store functions, from the image's ELF function symbols.
ownedSettingsWriters :: B.ByteString -> Either String [WatchRange]
ownedSettingsWriters elfBytes = do
  elf <- ELF.parseElf elfBytes
  extents <- fmap catMaybes (forM ownedStorageStoreFunctions (symbolExtent elf))
  case sortOn fst extents of
    [] -> Left "modal lifetime requires the owned image's storage store function symbols"
    ordered -> pure [either error id (watchRange (watchAddress low) (watchAddress high)) | (low, high) <- ordered]
 where
  symbolExtent elf name = case filter ((== name) . ELF.symbolName) (ELF.elfSymbols elf) of
    [] -> Right Nothing
    [entry] | ELF.symbolKind entry == ELF.FunctionSymbol
      , ELF.InSection index <- ELF.symbolSection entry
      , index >= 0, index < length (ELF.elfSections elf)
      , let section = ELF.elfSections elf !! index
      , ELF.sectionAllocated section, ELF.sectionExecutable section, not (ELF.sectionWritable section)
      , let low = ELF.symbolValue entry
      , let count = ELF.symbolSize entry
      , count >= 4, low `mod` 4 == 0, count `mod` 4 == 0
      , ELF.sectionAddress section <= low
      , toInteger low + toInteger count <= toInteger (ELF.sectionAddress section) + toInteger (ELF.sectionSize section)
      -> Right (Just (low, low + count))
    _ -> Left ("modal lifetime requires the owned image's complete ELF function: " ++ name)

-- | A settings-image reload: the call instruction, the instruction the call returns
-- to, the copy routine's store instruction, and the live-settings bytes it may change.
-- Between the call and return fetches, the copy stores on the calling core write
-- consecutive halfwords from the target's start.
data SettingsReload = SettingsReload
  { reloadCall :: WatchAddress
  , reloadReturn :: WatchAddress
  , reloadCopyStore :: WatchAddress
  , reloadTarget :: WatchRange
  } deriving (Eq, Show)

-- | The stock settings-image reload of property write `0x6309c4`: `bl 0x61d9ec` at
-- `0x6309ec` (`0xebffb3fe` in id4) returns to `0x6309f0` (`0xe1a00005`), and in between
-- the copy routine's `strh r5,[r12],#2` at `0x722134` stores 140 halfwords covering
-- `[0x96cc74, 0x96cd8c)` on the calling core (menu-observe-5, cardless no-install route).
stockSettingsReload :: SettingsReload
stockSettingsReload = SettingsReload (watchAddress 0x006309ec) (watchAddress 0x006309f0) (watchAddress 0x00722134)
  (either error id (watchRange (watchAddress 0x0096cc74) (watchAddress 0x0096cd8c)))

-- | The stock BACK settings restore of `0x637ed0`: `bl 0x61c90c` at `0x637ee4`
-- (`0xebff9288` in id4) returns to `0x637ee8` (`0xe1a00004`). With r0=1, `0x61c90c`
-- reaches `0x61daa8`, whose `strh r5,[r12],#2` at `0x722134` copies the 450-byte
-- backup at `0x96ce26` over the whole live block, 225 halfwords from `0x96cc64` to
-- `0x96ce24` on the calling core (menu-observe-6, BACK from the setting-246 picker).
stockSettingsRestore :: SettingsReload
stockSettingsRestore = SettingsReload (watchAddress 0x00637ee4) (watchAddress 0x00637ee8) (watchAddress 0x00722134) liveSettingsBlock

-- | The setter's header reload at `0x62fef0` calls `0x61da3c` and returns to
-- `0x62fef4`. Its eight ordered halfword stores cover `[0x96cc64, 0x96cc74)`.
stockSettingsHeaderReload :: SettingsReload
stockSettingsHeaderReload = SettingsReload (watchAddress 0x0062fef0) (watchAddress 0x0062fef4) (watchAddress 0x00722134)
  (either error id (watchRange (watchAddress 0x0096cc64) (watchAddress 0x0096cc74)))

-- | The call and return instruction words, in that order.
reloadSiteRanges :: SettingsReload -> [WatchRange]
reloadSiteRanges reload = [either error id (watchRange pc (watchAddress (watchAddressValue pc + 4))) | pc <- [reloadCall reload, reloadReturn reload]]

-- | The fetch watch classes of a lifetime binding: retired extents, then each reload's sites.
lifetimeFetchClasses :: ModalLifetimeSource -> [WatchRange]
lifetimeFetchClasses binding = lifetimeRetired binding ++ concatMap reloadSiteRanges (lifetimeSettingsReloads binding)

settingsReloadJson :: SettingsReload -> Value
settingsReloadJson reload = object
  [ "call" .= watchAddressValue (reloadCall reload), "return" .= watchAddressValue (reloadReturn reload)
  , "copy_store" .= watchAddressValue (reloadCopyStore reload), "target" .= extentJson (reloadTarget reload) ]

parseSettingsReload :: Value -> Either String SettingsReload
parseSettingsReload entry = do
  modalKeys ["call", "return", "copy_store", "target"] entry
  call <- modalField "call" entry >>= modalWord32
  resume <- modalField "return" entry >>= modalWord32
  store <- modalField "copy_store" entry >>= modalWord32
  target <- modalField "target" entry >>= parseModalExtent
  let (low, high) = watchRangeBounds target
      (blockLow, blockHigh) = watchRangeBounds liveSettingsBlock
  unless (all (\pc -> pc `mod` 4 == 0 && pc < 0x20000000) [call, resume, store] && call /= resume
    && blockLow <= low && low < high && high <= blockHigh)
    (Left "modal settings reload requires aligned canonical sites and a nonempty target inside the live-settings block")
  pure (SettingsReload (watchAddress call) (watchAddress resume) (watchAddress store) target)

-- | Up to 64 sorted, word-aligned, nonempty, pairwise disjoint canonical extents.
checkRetiredExtents :: [WatchRange] -> Either String ()
checkRetiredExtents retired = do
  let bounds = map watchRangeBounds retired
  unless (length retired <= 64 && all (\(low, high) -> low < high && low `mod` 4 == 0 && high `mod` 4 == 0 && high <= 0x20000000) bounds
    && map fst bounds == sort (map fst bounds)
    && and (zipWith (\(_, high) (low, _) -> high <= low) bounds (drop 1 bounds)))
    (Left "modal retired extents must be at most 64 sorted aligned disjoint canonical extents")

-- | Adds retired extents to a bound lifetime source; they must lie outside its runtime.
withRetiredExecution :: [WatchRange] -> ModalLifetimeSource -> Either String ModalLifetimeSource
withRetiredExecution retired source = do
  let sorted = sortOn watchRangeBounds retired
  checkRetiredExtents sorted
  unless (not (any (rangesOverlap (lifetimeRuntime source)) sorted))
    (Left "modal retired extents overlap the uploaded runtime")
  pure source { lifetimeRetired = sorted }

sourceImageJson :: SourceImage -> Value
sourceImageJson (SourceImage start bytes) = object ["start" .= watchAddressValue start, "bytes" .= B.unpack bytes]

extentJson :: WatchRange -> [Word64]
extentJson span_ = let (low, high) = watchRangeBounds span_ in [low, high]

rangesOverlap :: WatchRange -> WatchRange -> Bool
rangesOverlap left right =
  let (leftLow, leftHigh) = watchRangeBounds left
      (rightLow, rightHigh) = watchRangeBounds right
  in leftLow < rightHigh && rightLow < leftHigh

lifetimeRequestJson :: ModalLifetimeSource -> Value
lifetimeRequestJson source = object
  (("context_catalogue" .= contextCatalogueTag (lifetimeCatalogue source)) : lifetimeContractFields source)

lifetimeContractFields :: ModalLifetimeSource -> [(K.Key, Value)]
lifetimeContractFields source =
  [ "bank_variant" .= Layout.bankVariantTag (lifetimeVariant source)
  , "property_delta_words" .= fmap propertyDeltaValue (lifetimePropertyCounts source)
  , "runtime" .= extentJson (lifetimeRuntime source)
  , "resident" .= extentJson (lifetimeResident source)
  , "readonly" .= map sourceImageJson (lifetimeReadonly source)
  , "install" .= routine (lifetimeInstall source)
  , "rollback" .= routine (lifetimeRollback source)
  , "publications" .= [object ["address" .= watchAddressValue address, "old" .= old, "new" .= new]
      | PublicationBinding address (Ram.WordValue old) (Ram.WordValue new) <- lifetimePublications source]
  , "retired" .= map extentJson (lifetimeRetired source)
  , "settings_writers" .= map extentJson (lifetimeSettingsWriters source)
  , "settings_reloads" .= map settingsReloadJson (lifetimeSettingsReloads source)
  ]
  ++ ["task33_stack_guard" .= task33StackGuardJson guard | Just guard <- [lifetimeTask33StackGuard source]]
 where
  instruction (SourceInstruction pc (InstructionWord word)) = object ["pc" .= watchAddressValue pc, "word" .= word]
  routine (PublicationRoutine entry writer) = object ["entry" .= instruction entry, "writer" .= instruction writer]

modalLifetimeSourceJson :: ModalLifetimeSource -> Value
modalLifetimeSourceJson source = object
  [ "bound_elf_sha256" .= fmap sha256Hex (lifetimeElfDigest source)
  , "runtime_sha256" .= fmap sha256Hex (lifetimeRuntimeDigest source)
  , "manifest_sha256" .= fmap sha256Hex (lifetimeManifestDigest source)
  , "loader_a_sha256" .= fmap (sha256Hex . fst) (lifetimeLoaderDigests source)
  , "loader_b_sha256" .= fmap (sha256Hex . snd) (lifetimeLoaderDigests source)
  , "context_sources" .= map sourceImageJson (lifetimeContext source)
  , "context_catalogue_provenance" .= catalogueBindingValue (lifetimeCatalogueBinding source)
  , "lifetime" .= lifetimeRequestJson source
  ]


immutableLoaderSpans :: Word32 -> Word32 -> [(Word32, Word32)]
immutableLoaderSpans base end = [(fromIntegral low, fromIntegral high) | (low, high) <- loaderSpans (fromIntegral base) (fromIntegral (end - base))]

bindModalLifetimeSource :: [WatchRange] -> Ram.RamPushManifest -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ModalLifetimeSource
bindModalLifetimeSource writers manifest elfBytes loaderA loaderB = do
  catalogue <- Catalogue.bindStockContextCatalogue loaderA loaderB
  bound <- bindModalRuntimeSource writers manifest elfBytes
  bindLifetimeContext catalogue bound loaderA loaderB

bindModalLifetimeSourceForPackage :: Root.RootPackage -> B.ByteString -> Ram.RamPushManifest -> B.ByteString -> Either String ModalLifetimeSource
bindModalLifetimeSourceForPackage package finalDat manifest elfBytes = do
  parsed <- Update.parseContainer finalDat
  actual <- either (Left . show) Right (Targets.extractPreTable (Update.parsedParts parsed))
  let expected target = Targets.targetOf (Root.packageTargets package) target
  forM_ [Startup.LoaderA, Startup.LoaderB] $ \target ->
    unless (M.lookup target actual == Just (expected target))
      (Left "modal lifetime final DAT loaders differ from the admitted root platform package")
  bound <- bindModalRuntimeSource stockSettingsWriters manifest elfBytes
  catalogue <- Catalogue.bindOwnedUnionContextCatalogueForPackage package
  bindLifetimeContext catalogue bound (expected Startup.LoaderA) (expected Startup.LoaderB)

bindLifetimeContext :: Catalogue.ContextCatalogue -> ModalRuntimeSource -> B.ByteString -> B.ByteString -> Either String ModalLifetimeSource
bindLifetimeContext catalogue (ModalRuntimeSource source) loaderA loaderB = do
  unless (B.length loaderA == 0xdae0 && B.length loaderB == 0xdaa0)
    (Left "modal lifetime context requires both complete loader domains")
  let context = [SourceImage (watchAddress low) (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base_)) bytes))
        | (base_, end_, bytes) <- [(0x40000, 0x4dae0, loaderA), (0xd0000, 0xddaa0, loaderB)]
        , (low, high) <- immutableLoaderSpans base_ end_]
  pure source { lifetimeCatalogue = Catalogue.contextCatalogueKind catalogue
    , lifetimeContext = context, lifetimeLoaderDigests = Just (sha256 loaderA, sha256 loaderB)
    , lifetimeCatalogueBinding = SourceContextBound catalogue }

newtype ModalRuntimeSource = ModalRuntimeSource ModalLifetimeSource

modalRuntimeSourceJson :: ModalRuntimeSource -> Value
modalRuntimeSourceJson (ModalRuntimeSource source) = object
  [ "bound_elf_sha256" .= fmap sha256Hex (lifetimeElfDigest source)
  , "runtime_sha256" .= fmap sha256Hex (lifetimeRuntimeDigest source)
  , "manifest_sha256" .= fmap sha256Hex (lifetimeManifestDigest source)
  , "context_binding" .= ("runtime-only-no-loader-catalogue" :: T.Text)
  , "runtime_contract" .= object (lifetimeContractFields source) ]

bindModalRuntimeSource :: [WatchRange] -> Ram.RamPushManifest -> B.ByteString -> Either String ModalRuntimeSource
bindModalRuntimeSource writers manifest elfBytes = do
  variant <- maybe (Left "modal lifetime requires a typed bank variant") Right (Ram.manifestBankVariant manifest)
  extension <- Ram.manifestPropertyCounts manifest
  propertyPlan <- traverse (propertyCountsFromRuntime . snd) extension
  manifestHash <- parseSha256Hex (Ram.manifestDigest manifest)
  let expectedCount = maybe (Layout.publicationCountFor variant) (Layout.publicationActualCount . snd) extension
  expectedSize <- case extension of
    Nothing -> Right (Layout.runtimeAllocationSizeFor variant)
    Just (layout, _) -> Layout.propertyAllocationSize layout (Ram.unAddress (Ram.allocationBase manifest))
  unless (variant `elem` (Layout.standardBankVariant : scopedBankVariants)
    && length (Ram.manifestPublications manifest) == expectedCount
    && Ram.allocationSize manifest == fromIntegral expectedSize)
    (Left "modal runtime source bank variant, publication count or allocation differs")
  let runtime = Ram.runtimeBytes manifest
      runtimeDigest = sha256 runtime
      base = Ram.unAddress (Ram.allocationBase manifest)
      size = Ram.allocationSize manifest
      end = toInteger base + toInteger size
      runtimeSlice address count = B.take (fromIntegral count) (B.drop (fromIntegral (address - base)) runtime)
      contained address count = base <= address && toInteger address + toInteger count <= end
      publications = Ram.manifestPublications manifest
      addresses = map (Ram.unAddress . Ram.publicationAddress) publications
  unless (size > 0 && base `mod` 4 == 0 && size `mod` 4 == 0 && B.length runtime == size && end <= 0x20000000
    && sha256Hex runtimeDigest == Ram.runtimeDigest manifest)
    (Left "modal lifetime runtime must match the full manifest allocation and digest")
  unless (length publications == expectedCount && length (nub addresses) == length addresses
    && all (\address -> address `mod` 4 == 0 && address < 0x20000000) addresses)
    (Left "modal runtime source publication vector requires unique canonical aligned addresses")
  runtimeExtent <- watchRange (watchAddress base) (watchAddress (fromInteger end))
  elf <- ELF.parseElf elfBytes
  let symbol kind name = case filter ((== name) . ELF.symbolName) (ELF.elfSymbols elf) of
        [entry] | ELF.symbolKind entry == kind, ELF.InSection index <- ELF.symbolSection entry
          , index >= 0, index < length (ELF.elfSections elf)
          , let section = ELF.elfSections elf !! index
          , ELF.sectionAllocated section
          , kind /= ELF.FunctionSymbol || (ELF.sectionExecutable section && not (ELF.sectionWritable section))
          , ELF.sectionAddress section <= ELF.symbolValue entry
          , toInteger (ELF.symbolValue entry) + toInteger (ELF.symbolSize entry)
              <= toInteger (ELF.sectionAddress section) + toInteger (ELF.sectionSize section)
          , contained (ELF.symbolValue entry) (ELF.symbolSize entry) -> Right entry
        _ -> Left ("modal lifetime requires a complete allocated ELF symbol: " ++ name)
      instruction address = do
        unless (address `mod` 4 == 0 && contained address (4 :: Word32)) (Left "modal lifetime instruction is outside aligned runtime")
        word <- maybe (Left "modal lifetime instruction is not allocated ELF code") Right (ELF.wordAt elf address)
        unless (runtimeSlice address (4 :: Word32) == littleWord word) (Left "modal lifetime ELF instruction differs from runtime")
        pure (SourceInstruction (watchAddress address) (InstructionWord word))
      routine entryName entryAddress writerName = do
        entrySymbol <- symbol ELF.FunctionSymbol entryName
        unless (ELF.symbolValue entrySymbol == entryAddress && ELF.symbolSize entrySymbol >= 4)
          (Left "modal lifetime manifest entry differs from ELF function entry")
        entry <- instruction entryAddress
        writer <- symbol ELF.FunctionSymbol writerName
        let low = ELF.symbolValue writer
            count = ELF.symbolSize writer
        unless (count >= 4 && low `mod` 4 == 0 && count `mod` 4 == 0)
          (Left "modal lifetime writer requires a complete aligned function extent")
        let matches = [address | offset <- [0,4 .. count - 4], let address = low + offset, ELF.wordAt elf address == Just 0xe5813000]
        writerPc <- case matches of
          [address] -> Right address
          _ -> Left "modal lifetime writer symbol must contain exactly one STR r3,[r1]"
        PublicationRoutine entry <$> instruction writerPc
  resident <- symbol ELF.ObjectSymbol "rb_resident"
  unless (ELF.symbolSize resident == 1024
    && runtimeSlice (ELF.symbolValue resident) (1024 :: Word32) == B.replicate 1024 0)
    (Left "modal lifetime resident requires the exact initially zero 1024-byte ELF object")
  residentExtent <- watchRange (watchAddress (ELF.symbolValue resident)) (watchAddress (ELF.symbolValue resident + 1024))
  readonly <- forM [section | section <- ELF.elfSections elf, ELF.sectionAllocated section,
      not (ELF.sectionWritable section), ELF.sectionSize section > 0] $ \section -> do
    let address = ELF.sectionAddress section
        count = ELF.sectionSize section
        bytes = if ELF.sectionType section == 8 then B.replicate (fromIntegral count) 0
          else B.take (fromIntegral count) (B.drop (fromIntegral (ELF.sectionOffset section)) elfBytes)
    unless (contained address count && B.length bytes == fromIntegral count
      && runtimeSlice address count == bytes)
      (Left "modal lifetime allocated readonly ELF section differs from full runtime")
    pure (SourceImage (watchAddress address) bytes)
  unless (not (null readonly)) (Left "modal lifetime requires allocated readonly sources")
  readonlyRanges <- mapM sourceImageRange readonly
  unless (and [not (rangesOverlap left right) | (index, left) <- zip [0 :: Int ..] readonlyRanges, right <- drop (index + 1) readonlyRanges]
    && all (not . rangesOverlap residentExtent) readonlyRanges)
    (Left "modal lifetime readonly ELF sources overlap each other or resident")
  install <- routine "ram_push_install" (Ram.unAddress (Ram.installEntry (Ram.manifestEntries manifest))) "store_new_words"
  rollback <- routine "ram_push_rollback" (Ram.unAddress (Ram.rollbackEntry (Ram.manifestEntries manifest))) "restore_old_words"
  let bindings = [PublicationBinding (watchAddress (Ram.unAddress (Ram.publicationAddress publication)))
        (Ram.publicationOld publication) (Ram.publicationNew publication) | publication <- publications]
  validatePropertyPublicationPlan runtimeExtent propertyPlan bindings
  pure (ModalRuntimeSource (ModalLifetimeSource variant Catalogue.StockContextCatalogue
    runtimeExtent residentExtent readonly install rollback bindings [] (Just (sha256 elfBytes)) (Just runtimeDigest)
    (Just manifestHash) [] writers [stockSettingsReload, stockSettingsRestore, stockSettingsHeaderReload]
    Nothing Nothing RuntimeContextUnbound propertyPlan))

littleWord :: Word32 -> B.ByteString
littleWord word = B.pack [fromIntegral (word `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]

lifetimeModalRequest :: ModalRequest -> ModalLifetimeSource -> Either String ModalRequest
lifetimeModalRequest (ModalRequest stock scoped _ _) source = do
  unless (length (lifetimeContext source) == 16) (Left "modal lifetime requires complete loader context binding")
  case lifetimeCatalogueBinding source of
    RuntimeContextUnbound -> Left "modal lifetime requires a source-bound context catalogue"
    SourceContextBound catalogue -> unless (Catalogue.contextCatalogueKind catalogue == lifetimeCatalogue source)
      (Left "modal lifetime catalogue differs from its instruction-source proof")
  let PublicationCardinality expectedCount = publicationCardinality (lifetimeVariant source) (lifetimePropertyCounts source)
  unless (lifetimeVariant source `elem` maybe [Layout.standardBankVariant] (const scopedBankVariants) scoped
    && fromIntegral (length (lifetimePublications source)) == expectedCount)
    (Left "modal lifetime bank variant or publication count differs from stock or scoped route")
  when (lifetimeVariant source == Layout.modalRenderBankVariant) $
    unless (all (\(Layout.PublicationAddress hook) -> any (\(PublicationBinding address old _) ->
      watchAddressValue address == hook && old == Ram.WordValue 0xe92d4ff0) (lifetimePublications source)) Layout.renderHookAddresses)
      (Left "modal render lifetime lacks a required render hook")
  forM_ scoped $ \(ModalLinked start bytes, _) ->
    unless (SourceImage start bytes `elem` lifetimeReadonly source)
      (Left "modal linked source differs from the bound runtime readonly section")
  let readonly = case scoped of
        Nothing -> lifetimeReadonly source
        Just (ModalLinked start bytes, _) -> filter (/= SourceImage start bytes) (lifetimeReadonly source)
  pure (ModalRequest stock scoped modalLifetimeObservationLimit (Just source { lifetimeReadonly = readonly }))

parseModalLifetimeWatchAck :: Value -> Either String FixedWatchAck
parseModalLifetimeWatchAck value = projectCacheOnlyWatch value >>= parseFixedWatchAck

parseModalLifetimeWatch :: WatchLimit -> Value -> Either String FixedWatchObservation
parseModalLifetimeWatch limit value = projectCacheOnlyWatch value >>= parseFixedWatchObservation limit

projectCacheOnlyWatch :: Value -> Either String Value
projectCacheOnlyWatch value = do
  attribution <- modalField "fetch_attribution" value
  modalKeys ["sources", "ranges"] attribution
  unless (modalField "sources" attribution == Right (Array V.empty))
    (Left "modal lifetime watch requires cache-only fetch attribution")
  ranges <- modalField "ranges" attribution >>= modalList (either error id (modalLimit 65536))
  parsed <- mapM parseModalExtent ranges
  unless (not (null parsed) && length (nub parsed) == length parsed)
    (Left "modal lifetime cache-only fetch ranges must be nonempty and unique")
  WatchRowDigest count _ <- parseWatchRowDigest value
  case value of
    Object fields -> do
      forM_ (KM.lookup "records" fields) $ \records -> case records of
        Array rows | toInteger (V.length rows) == toInteger count -> Right ()
        _ -> Left "modal lifetime watch row count differs from its rows"
      Right (Object (foldr KM.delete fields ["fetch_attribution", "record_count", "records_sha256"]))
    _ -> Left "modal lifetime watch is not an object"

-- | Row count and SHA-256 the SDK maintains over the guard watch's append-only rows.
data WatchRowDigest = WatchRowDigest Word64 T.Text deriving (Eq, Show)

parseWatchRowDigest :: Value -> Either String WatchRowDigest
parseWatchRowDigest value = do
  count <- modalField "record_count" value >>= modalWord
  digest <- modalField "records_sha256" value >>= \entry -> case entry of
    String text | T.length text == 64 && T.all (`elem` ("0123456789abcdef" :: String)) text -> Right text
    _ -> Left "modal lifetime watch digest is not lowercase SHA-256 hex"
  pure (WatchRowDigest count digest)

-- | A modal report's guard watch: every watch-state field except the rows.
data ModalWatchSummary = ModalWatchSummary WatchRowDigest Value deriving (Eq, Show)

parseModalWatchSummary :: Value -> Either String ModalWatchSummary
parseModalWatchSummary raw = do
  modalKeys ["enabled", "ranges", "record_count", "records_sha256", "violation", "classes", "views", "fetch_attribution"] raw
  digest <- parseWatchRowDigest raw
  _ <- projectCacheOnlyWatch raw
  pure (ModalWatchSummary digest raw)

-- | The final watch-state's rows, and its remaining fields for comparison with report summaries.
data FinalModalWatch = FinalModalWatch FixedWatchObservation !Value

parseFinalModalWatch :: Value -> Either String FinalModalWatch
parseFinalModalWatch value = do
  observation <- parseModalLifetimeWatch modalLifetimeWriterLimit value
  case value of
    Object fields -> let !summary = KM.delete "records" fields in Right (FinalModalWatch observation (Object summary))
    _ -> Left "modal lifetime watch is not an object"

finalModalWatchFromRecords :: Value -> [WatchRecord] -> Either String FinalModalWatch
finalModalWatchFromRecords summary records = do
  _ <- parseModalWatchSummary summary
  WatchRowDigest count _ <- parseWatchRowDigest summary
  unless (toInteger (length records) == toInteger count)
    (Left "modal lifetime watch row count differs from its rows")
  header <- projectCacheOnlyWatch summary
  observation <- parseFixedWatchObservationFromRecords modalLifetimeWriterLimit header records
  pure (FinalModalWatch observation summary)

finalModalWatchSummary :: FinalModalWatch -> Value
finalModalWatchSummary (FinalModalWatch _ summary) = summary

-- | Compares every field but the rows, including the SDK row count and digest.
summaryDescribes :: FinalModalWatch -> ModalWatchSummary -> Bool
summaryDescribes (FinalModalWatch _ final) (ModalWatchSummary _ summary) = final == summary

modalReportDescribesFinalWatch :: FinalModalWatch -> ModalReport -> Bool
modalReportDescribesFinalWatch final = maybe False (summaryDescribes final) . modalWatchSummary

parseModalExtent :: Value -> Either String WatchRange
parseModalExtent (Array pair) | V.length pair == 2 = do
  low <- modalWord32 (pair V.! 0)
  high <- modalWord32 (pair V.! 1)
  watchRange (watchAddress low) (watchAddress high)
parseModalExtent _ = Left "modal extent must be a u32 start/end pair"

data LifetimePhase = Prepared | Publishing | Installed | RollingBack | RollbackStored | RollbackVerified | Ended
  deriving (Eq, Show, Enum, Bounded)
data LifetimeFailure
  = InvalidRequest | InvalidExtent | InvalidSource | InitialResident | InitialReadonly | Memory | InvalidCore
  | MalformedRow | NonCpuWrite | InvalidWrite | OutsideWatch | ImmutableWrite | ResourceChanged | IdentityWrite
  | MissingPermit | PendingStore | WrongPhase | WrongEntry | WrongWriter | WrongOrder | WrongCore | WrongBefore
  | WrongValue | WrongAddress | WrongWidth | InvalidContext | ActiveFrames | IncompleteRollback | RetiredFetch
  deriving (Eq, Show, Enum, Bounded)
data LifetimeReceipt = LifetimeReceipt
  { receiptPhase :: LifetimePhase
  , receiptPublicationCount :: ModalCount
  , receiptForwardWrites :: ModalCount
  , receiptReverseWrites :: ModalCount
  , receiptPendingStores :: ModalCount
  , receiptInstallCore :: Maybe CoreId
  , receiptRollbackCore :: Maybe CoreId
  , receiptResourceWrites :: ModalCount
  , receiptIdentityWrites :: ModalCount
  , receiptSchedulerWrites :: ModalCount
  , receiptContextWrites :: ModalCount
  , receiptVfpOwnerWrites :: ModalCount
  , receiptAuditedWrites :: ModalCount
  , receiptSettingsWrites :: ModalCount
  , receiptReloadWrites :: ModalCount
  , receiptFailure :: Maybe LifetimeFailure
  , receiptTask33StackGuard :: Maybe Task33StackGuardReceipt
  , receiptPropertyCounts :: Maybe PropertyPublicationCounts
  } deriving (Eq, Show)

lifetimePhaseText :: LifetimePhase -> T.Text
lifetimePhaseText phase = case phase of
  Prepared -> "prepared"; Publishing -> "publishing"; Installed -> "installed"; RollingBack -> "rolling_back"
  RollbackStored -> "rollback_stored"; RollbackVerified -> "rollback_verified"; Ended -> "ended"

lifetimeFailureText :: LifetimeFailure -> T.Text
lifetimeFailureText failure = case failure of
  InvalidRequest -> "invalid_request"; InvalidExtent -> "invalid_extent"; InvalidSource -> "invalid_source"
  InitialResident -> "initial_resident"; InitialReadonly -> "initial_readonly"; Memory -> "memory"; InvalidCore -> "invalid_core"
  MalformedRow -> "malformed_row"; NonCpuWrite -> "non_cpu_write"; InvalidWrite -> "invalid_write"; OutsideWatch -> "outside_watch"
  ImmutableWrite -> "immutable_write"; ResourceChanged -> "resource_changed"; IdentityWrite -> "identity_write"
  MissingPermit -> "missing_permit"; PendingStore -> "pending_store"; WrongPhase -> "wrong_phase"; WrongEntry -> "wrong_entry"
  WrongWriter -> "wrong_writer"; WrongOrder -> "wrong_order"; WrongCore -> "wrong_core"; WrongBefore -> "wrong_before"
  WrongValue -> "wrong_value"; WrongAddress -> "wrong_address"; WrongWidth -> "wrong_width"; InvalidContext -> "invalid_context"
  ActiveFrames -> "active_frames"; IncompleteRollback -> "incomplete_rollback"; RetiredFetch -> "retired_fetch"

parseClosed :: (Bounded a, Enum a) => (a -> T.Text) -> Value -> Either String a
parseClosed render (String text) = case [value | value <- [minBound .. maxBound], render value == text] of
  [value] -> Right value
  _ -> Left "unknown modal enum value"
parseClosed _ _ = Left "modal enum value must be text"

parseNullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
parseNullable _ Null = Right Nothing
parseNullable parser value = Just <$> parser value

parseCore :: Value -> Either String CoreId
parseCore value = modalWord value >>= \number -> case number of
  0 -> Right Core0
  1 -> Right Core1
  _ -> Left "modal core is not 0 or 1"

coreValue :: CoreId -> Int
coreValue Core0 = 0
coreValue Core1 = 1

parseLifetimeReceipt :: Value -> Either String LifetimeReceipt
parseLifetimeReceipt value = do
  let guardValue = optionalField "task33_stack_guard" value
      propertyValue = optionalField "property_delta_words" value
  modalKeys (["phase", "publication_count", "forward_writes", "reverse_writes", "pending_stores",
    "install_core", "rollback_core", "checked_resource_writes", "checked_identity_writes",
    "checked_scheduler_writes", "checked_context_writes", "checked_vfp_owner_writes", "audited_writes", "admitted_settings_writes",
    "admitted_reload_writes", "failure"] ++ ["task33_stack_guard" | isJust guardValue]
    ++ ["property_delta_words" | isJust propertyValue]) value
  stackGuard <- traverse parseTask33StackGuardReceipt guardValue
  propertyCounts <- traverse (parsePropertyCounts Layout.modalRenderBankVariant) propertyValue
  let count key = ModalCount <$> (modalField key value >>= modalWord)
  phase <- modalField "phase" value >>= parseClosed lifetimePhaseText
  failure <- modalField "failure" value >>= parseNullable (\entry -> do
    modalKeys ["kind"] entry
    modalField "kind" entry >>= parseClosed lifetimeFailureText)
  LifetimeReceipt phase <$> count "publication_count" <*> count "forward_writes" <*> count "reverse_writes"
    <*> count "pending_stores" <*> (modalField "install_core" value >>= parseNullable parseCore)
    <*> (modalField "rollback_core" value >>= parseNullable parseCore)
    <*> count "checked_resource_writes" <*> count "checked_identity_writes"
    <*> count "checked_scheduler_writes" <*> count "checked_context_writes" <*> count "checked_vfp_owner_writes"
    <*> count "audited_writes" <*> count "admitted_settings_writes" <*> count "admitted_reload_writes" <*> pure failure <*> pure stackGuard <*> pure propertyCounts

lifetimeReceiptJson :: LifetimeReceipt -> Value
lifetimeReceiptJson receipt = object $
  [ "phase" .= lifetimePhaseText (receiptPhase receipt)
  , "publication_count" .= countValue (receiptPublicationCount receipt)
  , "forward_writes" .= countValue (receiptForwardWrites receipt)
  , "reverse_writes" .= countValue (receiptReverseWrites receipt)
  , "pending_stores" .= countValue (receiptPendingStores receipt)
  , "install_core" .= fmap coreValue (receiptInstallCore receipt)
  , "rollback_core" .= fmap coreValue (receiptRollbackCore receipt)
  , "checked_resource_writes" .= countValue (receiptResourceWrites receipt)
  , "checked_identity_writes" .= countValue (receiptIdentityWrites receipt)
  , "checked_scheduler_writes" .= countValue (receiptSchedulerWrites receipt)
  , "checked_context_writes" .= countValue (receiptContextWrites receipt)
  , "checked_vfp_owner_writes" .= countValue (receiptVfpOwnerWrites receipt)
  , "audited_writes" .= countValue (receiptAuditedWrites receipt)
  , "admitted_settings_writes" .= countValue (receiptSettingsWrites receipt)
  , "admitted_reload_writes" .= countValue (receiptReloadWrites receipt)
  , "failure" .= fmap (\failure -> object ["kind" .= lifetimeFailureText failure]) (receiptFailure receipt)
  ]
  ++ ["task33_stack_guard" .= task33StackGuardReceiptJson guard | Just guard <- [receiptTask33StackGuard receipt]]
  ++ ["property_delta_words" .= propertyDeltaValue counts | Just counts <- [receiptPropertyCounts receipt]]

data PhysicalGuard = PhysicalGuard ModalCount ModalCount ModalCount ModalCount (Maybe ModalFailure) deriving (Eq, Show)

parsePhysicalGuard :: Value -> Either String PhysicalGuard
parsePhysicalGuard value = do
  modalKeys ["hook_entries", "protected_entries", "accepted_entries", "refused_entries", "failure"] value
  let count key = ModalCount <$> (modalField key value >>= modalWord)
  PhysicalGuard <$> count "hook_entries" <*> count "protected_entries" <*> count "accepted_entries" <*> count "refused_entries"
    <*> (modalField "failure" value >>= parseNullable (\entry -> case entry of
      String text | not (T.null text) -> Right (ModalFailure text)
      _ -> Left "modal physical guard failure must be nonempty text"))

physicalGuardJson :: PhysicalGuard -> Value
physicalGuardJson (PhysicalGuard hooks protected accepted refused failure) = object
  [ "hook_entries" .= countValue hooks, "protected_entries" .= countValue protected
  , "accepted_entries" .= countValue accepted, "refused_entries" .= countValue refused
  , "failure" .= fmap (\(ModalFailure text) -> text) failure ]

healthyPhysicalGuard :: PhysicalGuard -> Bool
healthyPhysicalGuard (PhysicalGuard hooks protected accepted refused failure) =
  failure == Nothing && countValue refused == 0 && countValue protected == countValue accepted
    && countValue hooks >= countValue protected

observedPhysicalGuard :: PhysicalGuard -> Bool
observedPhysicalGuard guard@(PhysicalGuard _ _ accepted _ _) = healthyPhysicalGuard guard && countValue accepted > 0

data Kernel = PrimaryKernel | SecondaryKernel deriving (Eq, Show)
newtype TaskId = TaskId Word32 deriving (Eq, Show)
newtype RegisterValue = RegisterValue Word32 deriving (Eq, Show)
data TaskIdentity = TaskIdentity Kernel TaskId WatchAddress WatchAddress WatchRange deriving (Eq, Show)
data TaskFetch = TaskFetch WatchAddress WatchAddress B.ByteString CoreId Instructions deriving (Eq, Show)
data TaskEntry = TaskEntry CoreId WatchAddress Instructions CapturedController RegisterValue RegisterValue RegisterValue TaskIdentity TaskFetch deriving (Eq, Show)

parseModalBytes :: Int -> Value -> Either String B.ByteString
parseModalBytes upper (Array bytes) = do
  unless (V.length bytes <= upper) (Left "modal source exceeds its byte bound")
  B.pack <$> mapM (\entry -> do
    byte <- modalWord entry
    unless (byte <= 255) (Left "modal source byte exceeds u8")
    pure (fromIntegral byte)) (V.toList bytes)
parseModalBytes _ _ = Left "modal source is not a byte array"

parseTaskEntry :: Value -> Either String TaskEntry
parseTaskEntry value = do
  modalKeys ["core", "pc", "instructions", "controller", "sp", "lr", "cpsr", "identity", "fetch"] value
  core <- modalField "core" value >>= parseCore
  pc <- modalField "pc" value >>= modalWord32
  instructions <- Instructions <$> (modalField "instructions" value >>= modalWord)
  controller <- CapturedController <$> (modalField "controller" value >>= modalWord32)
  sp <- modalField "sp" value >>= modalWord32
  lr <- modalField "lr" value >>= modalWord32
  cpsr <- modalField "cpsr" value >>= modalWord32
  identity <- modalField "identity" value
  modalKeys ["kernel", "id", "node", "creation", "stack"] identity
  kernel <- modalField "kernel" identity >>= \entry -> case entry of
    String "primary" -> Right PrimaryKernel
    String "secondary" -> Right SecondaryKernel
    _ -> Left "unknown modal task kernel"
  taskId <- modalField "id" identity >>= modalWord32
  node <- modalField "node" identity >>= modalWord32
  creation <- modalField "creation" identity >>= modalWord32
  stack <- modalField "stack" identity >>= parseModalExtent
  let (stackLow, stackHigh) = watchRangeBounds stack
      physicalSp = fromIntegral (sp .&. 0xbfffffff) :: Word64
      (nodeBase, creationBase) = case kernel of
        PrimaryKernel -> (0x4005ff0c, 0x4005beac)
        SecondaryKernel -> (0x400efecc, 0x400ebe6c)
  unless (pc == 0x00bfb028 && controller == CapturedController 75 && taskId >= 1 && taskId <= 120
    && node == nodeBase + (taskId - 1) * 56 && creation == creationBase + (taskId - 1) * 40
    && sp `mod` 4 == 0 && stackHigh <= 0x20000000 && stackLow <= physicalSp && physicalSp <= stackHigh
    && cpsr .&. 0x010002bf == 0x13 && (kernel == PrimaryKernel) == (core == Core0))
    (Left "modal task entry has invalid local task execution metadata")
  fetch <- modalField "fetch" value
  modalKeys ["guest", "physical", "bytes", "core", "entry_progress", "byte_order"] fetch
  guest <- modalField "guest" fetch >>= modalWord32
  physical <- modalField "physical" fetch >>= modalWord32
  bytes <- modalField "bytes" fetch >>= parseModalBytes 4
  fetchCore <- modalField "core" fetch >>= parseCore
  progress <- Instructions <$> (modalField "entry_progress" fetch >>= modalWord)
  unless (guest == pc && physical == pc && physical < 0x20000000 && B.length bytes == 4
    && core == fetchCore && modalField "byte_order" fetch == Right (String "little"))
    (Left "modal task fetch does not bind its canonical instruction entry")
  pure (TaskEntry core (watchAddress pc) instructions controller (RegisterValue sp) (RegisterValue lr) (RegisterValue cpsr)
    (TaskIdentity kernel (TaskId taskId) (watchAddress node) (watchAddress creation) stack)
    (TaskFetch (watchAddress guest) (watchAddress physical) bytes fetchCore progress))

taskEntryJson :: TaskEntry -> Value
taskEntryJson (TaskEntry core pc instructions (CapturedController controller) (RegisterValue sp) (RegisterValue lr) (RegisterValue cpsr)
  (TaskIdentity kernel (TaskId taskId) node creation stack) (TaskFetch guest physical bytes fetchCore progress)) = object
  [ "core" .= coreValue core, "pc" .= watchAddressValue pc, "instructions" .= instructionsValue instructions
  , "controller" .= controller, "sp" .= sp, "lr" .= lr, "cpsr" .= cpsr
  , "identity" .= object ["kernel" .= (case kernel of PrimaryKernel -> "primary"; SecondaryKernel -> "secondary" :: String)
      , "id" .= taskId, "node" .= watchAddressValue node, "creation" .= watchAddressValue creation, "stack" .= extentJson stack]
  , "fetch" .= object ["guest" .= watchAddressValue guest, "physical" .= watchAddressValue physical, "bytes" .= B.unpack bytes
      , "core" .= coreValue fetchCore, "entry_progress" .= instructionsValue progress, "byte_order" .= ("little" :: String)] ]

modalLifetimeCoverage :: ModalReport -> Bool
modalLifetimeCoverage report = case modalLifetimeReceipt report of
  Nothing -> False
  Just receipt ->
    let count = countValue (receiptPublicationCount receipt)
        validCount = case receiptPropertyCounts receipt of
          Nothing -> fromIntegral count `elem` guardPublicationCounts
          Just counts -> let PublicationCardinality expected = publicationCardinality Layout.modalRenderBankVariant (Just counts) in count == expected
    in receiptPhase receipt == Ended && validCount
      && countValue (receiptForwardWrites receipt) == count && countValue (receiptReverseWrites receipt) == count
      && countValue (receiptPendingStores receipt) == 0 && receiptInstallCore receipt /= Nothing
      && receiptRollbackCore receipt /= Nothing && receiptFailure receipt == Nothing
      && countValue (receiptIdentityWrites receipt) == 0
      && modalReportFailure report == Nothing && maybe False observedPhysicalGuard (modalGuard report)
      && not (modalReportEnabled report) && countValue (modalActiveFrames (modalReportCounts report)) == 0
      && modalPhysicalCoverage report && countValue (modalContextSourceSpans report) == 16

checkCompletedModalLifetime :: ModalRequest -> ArrayWriteReport -> FinalModalWatch -> ModalReport -> Either String ()
checkCompletedModalLifetime (ModalRequest (ModalStock stock) scoped _ source) audit final@(FinalModalWatch observation _) report = do
  checkModalEnd report
  unless (modalReportPolicy report == maybe StockBaseline (const ScopedOwned75) scoped)
    (Left "completed modal policy differs from the source-bound request")
  binding <- maybe (Left "modal request has no installed lifetime binding") Right source
  receipt <- maybe (Left "modal report has no lifetime receipt") Right (modalLifetimeReceipt report)
  checkTask33StackGuardReceipt binding (Just receipt)
  unless (receiptPropertyCounts receipt == lifetimePropertyCounts binding)
    (Left "modal lifetime receipt property publication plan differs from source")
  unless (modalLifetimeCoverage report && countValue (receiptPublicationCount receipt) == fromIntegral (length (lifetimePublications binding)))
    (Left "modal lifetime did not complete exact publication and reverse rollback")
  unless (modalPhysicalCoverage report) (Left "modal lifetime lacks healthy physical fetch coverage")
  unless (countValue (modalContextSourceSpans report) == fromIntegral (length (lifetimeContext binding)))
    (Left "modal lifetime completed with a mismatched context source count")
  entryBytes <- case scoped of
    Nothing -> Right (B.take 4 stock)
    Just _ -> case [new | PublicationBinding address _ (Ram.WordValue new) <- lifetimePublications binding
                       , watchAddressValue address == 0x00bfb028] of
      [new] -> Right (littleWord new)
      _ -> Left "scoped modal lifetime lacks its unique shared-entry publication"
  forM_ (modalTaskEntries report) $ \(TaskEntry _ _ _ _ _ _ _ _ (TaskFetch _ _ bytes _ _)) ->
    unless (bytes == entryBytes) (Left "owned modal task fetch differs from the installed entry publication")
  summary <- maybe (Left "modal lifetime lacks its watch summary") Right (modalWatchSummary report)
  unless (summaryDescribes final summary) (Left "modal report watch summary differs from the final watch-state rows")
  let rows = fixedWatchRecords observation
      expectedWrites = sum (map (toInteger . countValue)
        [receiptForwardWrites receipt, receiptReverseWrites receipt, receiptResourceWrites receipt
        , receiptSchedulerWrites receipt, receiptContextWrites receipt, receiptVfpOwnerWrites receipt, receiptAuditedWrites receipt])
      writes = [row | row@CpuRecord { cpuAccess = CpuWrite } <- rows]
  forM_ (lifetimeTask33StackGuard binding) $ \_ -> do
    let (low, high) = watchRangeBounds task33StackGuardRange
        touches row = case writeAttribution row of
          Just (_, address, bytes) -> address < toInteger high && toInteger low < address + bytes
          Nothing -> False
    unless (not (any touches rows)) (Left "modal lifetime attempted a write to the task33 bottom guard")
  stockFetches <- lifetimeFetchRows binding rows
  unless (toInteger (length writes) == expectedWrites && length writes + stockFetches == length rows
    && toInteger (length (arrayWrites audit)) == toInteger (countValue (receiptAuditedWrites receipt)))
    (Left "modal lifetime accepted write counters differ from raw CPU rows or array audit")
  settingsWrites <- lifetimeSettingsWrites binding rows
  let admitted kind = fromIntegral (length (filter ((== kind) . settingsWriter) settingsWrites))
  unless (admitted SetterWrite == countValue (receiptSettingsWrites receipt) && admitted ReloadWrite == countValue (receiptReloadWrites receipt))
    (Left "modal lifetime admitted setter or reload writes differ from the value-changing settings rows")
  case scoped of
    Nothing -> do
      let counts = modalReportCounts report
      unless (countValue (modalOwnedHandoffs counts) == 0 && modalEntries counts == modalFallbackHandoffs counts
        && modalEntries counts == modalReturns counts && null (modalTaskEntries report) && not (modalTaskIdentityVerified report))
        (Left "stock modal lifetime has unbalanced fallback observations or unsupported owned task claims")
    Just _ -> do
      checkScopedModalRoute report
      unless (modalTaskIdentityVerified report && not (null (modalTaskEntries report)))
        (Left "scoped modal lifetime lacks verified owned task execution")

-- | Counts fetch rows. Each must be a settings-reload call or return word, or start
-- inside a retired extent before the first or after the last publication-writer row.
lifetimeFetchRows :: ModalLifetimeSource -> [WatchRecord] -> Either String Int
lifetimeFetchRows binding rows = do
  let writer (PublicationRoutine _ (SourceInstruction pc _)) = pc
      writers = [writer (lifetimeInstall binding), writer (lifetimeRollback binding)]
      published = [index | (index, CpuRecord { cpuAccess = CpuWrite, cpuPc = pc }) <- zip [0 :: Int ..] rows, pc `elem` writers]
      outside index = case published of
        [] -> True
        first : _ -> index < first || index > maximum published
      retired physical = any (\range -> let (low, high) = watchRangeBounds range
                                            address = toInteger (watchAddressValue physical)
                                        in toInteger low <= address && address < toInteger high) (lifetimeRetired binding)
      reloadSite physical = physical `elem` concat [[reloadCall reload, reloadReturn reload] | reload <- lifetimeSettingsReloads binding]
      fetches = [(index, physical) | (index, CpuRecord { cpuAccess = CpuFetch, cpuPhysical = physical }) <- zip [0 :: Int ..] rows]
  unless (all (\(index, physical) -> reloadSite physical || (retired physical && outside index)) fetches)
    (Left "modal lifetime fetched a retired body while the owned entry was published, or recorded a fetch outside every retired extent and reload site")
  pure (length fetches)

-- | The declared code admitted to change a live setting.
data SettingsWriterKind = SetterWrite | ReloadWrite deriving (Eq, Show, Enum, Bounded)

settingsWriterKindText :: SettingsWriterKind -> T.Text
settingsWriterKindText SetterWrite = "setter"
settingsWriterKindText ReloadWrite = "reload"

-- | A value-changing store into the live-settings block.
data SettingsWrite = SettingsWrite
  { settingsWriter :: SettingsWriterKind
  , settingsInstructions :: Instructions
  , settingsPc :: WatchAddress
  , settingsAddress :: WatchAddress
  , settingsBefore :: B.ByteString
  , settingsValue :: Word64
  } deriving (Eq, Show)

liveSettingsBlock :: WatchRange
liveSettingsBlock = either error id (watchRange (watchAddress 0x0096cc64) (watchAddress 0x0096ce26))

-- | The value-changing live-settings stores in row order. Each lies wholly in the
-- block and comes from a declared settings writer, or from a reload's copy store at
-- the core's halfword cursor while that reload's call fetch is the core's latest
-- reload-site fetch. A call fetch sets the cursor to the target's start; a copy store
-- into the target anywhere else, or of another width, closes the bracket.
lifetimeSettingsWrites :: ModalLifetimeSource -> [WatchRecord] -> Either String [SettingsWrite]
lifetimeSettingsWrites binding rows = reverse . snd <$> foldM step ([], []) rows
 where
  reloads = lifetimeSettingsReloads binding
  closed core = filter ((/= core) . fst)
  step (open, writes) row = case row of
    CpuRecord { cpuAccess = CpuFetch, cpuPhysical = physical, cpuCore = core }
      | Just reload <- find ((== physical) . reloadCall) reloads ->
          pure ((core, (reload, toInteger (fst (watchRangeBounds (reloadTarget reload))))) : closed core open, writes)
      | any ((== physical) . reloadReturn) reloads -> pure (closed core open, writes)
    CpuRecord { cpuAccess = CpuWrite, cpuPhysical = physical, cpuBytes = bytes, cpuPc = pc, cpuCore = core } -> do
      let start = toInteger (watchAddressValue physical)
          size = toInteger (watchBytesValue bytes)
          inside range = let (low, high) = watchRangeBounds range in toInteger low <= start && start + size <= toInteger high
          (open', reloadStore) = case lookup core open of
            Just (reload, cursor) | pc == reloadCopyStore reload && overlapsWatchRange (reloadTarget reload) physical bytes ->
              if start == cursor && size == 2 && inside (reloadTarget reload)
                then ((core, (reload, cursor + 2)) : closed core open, True)
                else (closed core open, False)
            _ -> (open, False)
      case row of
        CpuRecord { cpuValue = Just value, cpuBefore = Just before, cpuByteOrder = order, cpuInstructions = instructions }
          | overlapsWatchRange liveSettingsBlock physical bytes && changes order before value -> do
              let at = toInteger (watchAddressValue pc)
                  within range = let (from, to) = watchRangeBounds range in toInteger from <= at && at < toInteger to
                  writer
                    | not (inside liveSettingsBlock) = Nothing
                    | any within (lifetimeSettingsWriters binding) = Just SetterWrite
                    | reloadStore = Just ReloadWrite
                    | otherwise = Nothing
              kind <- maybe (Left "modal lifetime changed a live setting outside a declared settings writer or open settings reload") Right writer
              pure (open', SettingsWrite kind instructions pc physical before value : writes)
        _ -> pure (open', writes)
    _ -> pure (open, writes)
  changes order before value = or
    [ B.index before index /= fromIntegral (value `div` (256 ^ shift))
    | index <- [0 .. B.length before - 1]
    , let shift = case order of LittleEndian -> index; BigEndian -> B.length before - 1 - index ]

completedSettingsWrites :: ModalRequest -> FinalModalWatch -> Either String [SettingsWrite]
completedSettingsWrites (ModalRequest _ _ _ source) (FinalModalWatch observation _) = do
  binding <- maybe (Left "modal request has no installed lifetime binding") Right source
  lifetimeSettingsWrites binding (fixedWatchRecords observation)

settingsWritesJson :: [SettingsWrite] -> Value
settingsWritesJson writes = object
  [ "schema" .= (2 :: Int)
  , "writes" .= [ object [ "writer" .= settingsWriterKindText (settingsWriter write)
                         , "instructions" .= instructionsValue (settingsInstructions write), "pc" .= watchAddressValue (settingsPc write)
                         , "address" .= watchAddressValue (settingsAddress write), "size" .= B.length (settingsBefore write)
                         , "before" .= B.unpack (settingsBefore write), "value" .= settingsValue write ]
                | write <- writes ] ]

parseSettingsWrites :: Value -> Either String [SettingsWrite]
parseSettingsWrites value = do
  modalKeys ["schema", "writes"] value
  schema <- modalField "schema" value >>= modalWord
  unless (schema == 2) (Left "settings writes schema is not 2")
  modalField "writes" value >>= modalList (either error id (modalLimit 65536)) >>= mapM (\entry -> do
    modalKeys ["writer", "instructions", "pc", "address", "size", "before", "value"] entry
    writer <- modalField "writer" entry >>= parseClosed settingsWriterKindText
    instructions <- modalField "instructions" entry >>= modalWord
    pc <- modalField "pc" entry >>= modalWord32
    address <- modalField "address" entry >>= modalWord32
    size <- modalField "size" entry >>= modalWord
    before <- modalField "before" entry >>= parseModalBytes 8
    written <- modalField "value" entry >>= modalWord
    unless (size >= 1 && fromIntegral size == B.length before) (Left "settings write size differs from its before bytes")
    pure (SettingsWrite writer (instructionCount instructions) (watchAddress pc) (watchAddress address) before written))

-- | Stock and owned roles must make the same ordered settings stores: address,
-- width and value. The writer identity that admitted each store is the role's own
-- image's.
compareSettingsWrites :: [SettingsWrite] -> [SettingsWrite] -> Either String ()
compareSettingsWrites stock owned =
  let key write = (settingsAddress write, B.length (settingsBefore write), settingsValue write)
  in unless (map key stock == map key owned)
       (Left ("settings write sequences differ: stock " ++ show (length stock) ++ ", owned " ++ show (length owned)))

optionalField :: K.Key -> Value -> Maybe Value
optionalField key (Object fields) = KM.lookup key fields
optionalField _ _ = Nothing

sourceImageRange :: SourceImage -> Either String WatchRange
sourceImageRange (SourceImage start bytes) = do
  let end = toInteger (watchAddressValue start) + toInteger (B.length bytes)
  unless (not (B.null bytes) && end <= 0x20000000) (Left "modal source image is empty or noncanonical")
  watchRange start (watchAddress (fromInteger end))

parseSourceImage :: Value -> Either String SourceImage
parseSourceImage value = do
  modalKeys ["start", "bytes"] value
  start <- watchAddress <$> (modalField "start" value >>= modalWord32)
  bytes <- modalField "bytes" value >>= parseModalBytes 1048576
  let image = SourceImage start bytes
  _ <- sourceImageRange image
  pure image

parseLifetimeBinding :: BeginRequestFormat -> CatalogueParser -> Maybe (ModalLinked, ModalScope) -> Value -> Value -> Either String ModalLifetimeSource
parseLifetimeBinding format parseCatalogue scoped contextValue value = do
  let guardValue = optionalField "task33_stack_guard" value
      propertyValue = optionalField "property_delta_words" value
  modalKeys (["bank_variant", "runtime", "resident", "readonly", "install", "rollback", "publications", "retired", "settings_writers", "settings_reloads"]
    ++ ["context_catalogue" | format == CurrentBeginRequest]
    ++ ["task33_stack_guard" | isJust guardValue]
    ++ ["property_delta_words" | format == CurrentBeginRequest && isJust propertyValue]) value
  stackGuard <- traverse parseTask33StackGuard guardValue
  variant <- modalField "bank_variant" value >>= \entry -> case entry of
    String tag -> Layout.parseBankVariantTag (T.unpack tag)
    _ -> Left "modal lifetime bank_variant must be text"
  runtime <- modalField "runtime" value >>= parseModalExtent
  resident <- modalField "resident" value >>= parseModalExtent
  propertyCounts <- maybe (Right Nothing) (parseNullable (parsePropertyCounts variant)) propertyValue
  let (runtimeLow, runtimeHigh) = watchRangeBounds runtime
      (residentLow, residentHigh) = watchRangeBounds resident
      contained span_ = let (low, high) = watchRangeBounds span_ in runtimeLow <= low && high <= runtimeHigh
  unless (runtimeLow `mod` 4 == 0 && runtimeHigh `mod` 4 == 0 && runtimeHigh <= 0x20000000
    && residentLow `mod` 4 == 0 && residentHigh - residentLow == 1024 && contained resident)
    (Left "modal lifetime requires aligned canonical runtime and exact resident1024 within it")
  context <- modalList (either error id (modalLimit 16)) contextValue >>= mapM parseSourceImage
  contextRanges <- mapM sourceImageRange context
  let expectedContext = [(fromIntegral low, fromIntegral high) | (base, end) <- [(0x40000, 0x4dae0), (0xd0000, 0xddaa0)], (low, high) <- immutableLoaderSpans base end]
  unless (map watchRangeBounds contextRanges == expectedContext)
    (Left "modal lifetime requires exactly the 16 ordered immutable loader spans")
  catalogueValue <- case format of
    CurrentBeginRequest -> modalField "context_catalogue" value
    RecordedStockBeginRequest -> Right (String "stock")
  catalogue <- parseCatalogue catalogueValue [Catalogue.ContextSourceSlice start bytes | SourceImage start bytes <- context]
  readonly <- modalField "readonly" value >>= modalList (either error id (modalLimit 1024)) >>= mapM parseSourceImage
  let linkedImages = [SourceImage start bytes | Just (ModalLinked start bytes, _) <- [scoped]]
      allImages = readonly ++ linkedImages
  allRanges <- mapM sourceImageRange allImages
  unless (not (null allImages) && all contained allRanges
    && all (\span_ -> not (rangesOverlap resident span_)) allRanges
    && and [not (rangesOverlap left right) | (index, left) <- zip [0 :: Int ..] allRanges, right <- drop (index + 1) allRanges])
    (Left "modal lifetime readonly and linked images overlap or leave runtime")
  let sourceWord pc = case [bytes | SourceImage start image <- allImages,
          let offset = toInteger pc - toInteger (watchAddressValue start), offset >= 0,
          offset + 4 <= toInteger (B.length image), let bytes = B.take 4 (B.drop (fromInteger offset) image)] of
        [bytes] -> Right bytes
        _ -> Left "modal lifetime instruction lacks a unique immutable source"
      instruction entry = do
        modalKeys ["pc", "word"] entry
        pc <- modalField "pc" entry >>= modalWord32
        word <- modalField "word" entry >>= modalWord32
        bytes <- sourceWord pc
        unless (pc `mod` 4 == 0 && bytes == littleWord word)
          (Left "modal lifetime instruction differs from immutable source")
        pure (SourceInstruction (watchAddress pc) (InstructionWord word))
      routine entry = do
        modalKeys ["entry", "writer"] entry
        begin <- modalField "entry" entry >>= instruction
        writer <- modalField "writer" entry >>= instruction
        unless (case writer of SourceInstruction _ (InstructionWord word) -> word == 0xe5813000)
          (Left "modal lifetime publication writer is not STR r3,[r1]")
        pure (PublicationRoutine begin writer)
  install <- modalField "install" value >>= routine
  rollback <- modalField "rollback" value >>= routine
  let PublicationCardinality expectedCount = publicationCardinality variant propertyCounts
  publications <- modalField "publications" value >>= modalList (either error id (modalLimit expectedCount)) >>= mapM (\entry -> do
    modalKeys ["address", "old", "new"] entry
    address <- modalField "address" entry >>= modalWord32
    unless (address `mod` 4 == 0 && address < 0x20000000) (Left "modal lifetime publication is not aligned canonical RAM")
    PublicationBinding (watchAddress address) <$> (Ram.WordValue <$> (modalField "old" entry >>= modalWord32))
      <*> (Ram.WordValue <$> (modalField "new" entry >>= modalWord32)))
  let addresses = [address | PublicationBinding address _ _ <- publications]
  unless (fromIntegral (length publications) == expectedCount
    && (isJust propertyCounts || length publications `elem` routePublicationCounts scoped)
    && length (nub addresses) == length addresses)
    (Left "modal lifetime publication count or uniqueness differs from route")
  validatePropertyPublicationPlan runtime propertyCounts publications
  retired <- modalField "retired" value >>= modalList (either error id (modalLimit 64)) >>= mapM parseModalExtent
  checkRetiredExtents retired
  unless (not (any (rangesOverlap runtime) retired)) (Left "modal retired extents overlap the uploaded runtime")
  settingsWriters <- modalField "settings_writers" value >>= modalList (either error id (modalLimit 8)) >>= mapM parseModalExtent
  let writerBounds = map watchRangeBounds settingsWriters
  unless (all (\(low, high) -> low `mod` 4 == 0 && high `mod` 4 == 0) writerBounds
    && and (zipWith (\(_, high) (low, _) -> high <= low) writerBounds (drop 1 writerBounds)))
    (Left "modal settings writers must be sorted aligned disjoint extents")
  settingsReloads <- modalField "settings_reloads" value >>= modalList (either error id (modalLimit 4)) >>= mapM parseSettingsReload
  let reloadSites = concatMap reloadSiteRanges settingsReloads
      inWriter pc = any (\(low, high) -> low <= fromIntegral pc && fromIntegral pc < high) writerBounds
  unless (length (nub reloadSites) == length reloadSites && not (any (rangesOverlap runtime) reloadSites)
    && not (any (inWriter . watchAddressValue . reloadCopyStore) settingsReloads))
    (Left "modal settings reload sites repeat or enter the runtime, or a copy store lies in a settings writer")
  pure (ModalLifetimeSource variant (Catalogue.contextCatalogueKind catalogue)
    runtime resident allImages install rollback publications context Nothing Nothing Nothing
    retired settingsWriters settingsReloads stackGuard Nothing (SourceContextBound catalogue) propertyCounts)

modalLifetimeWatchRanges :: ModalRequest -> Either String [WatchRange]
modalLifetimeWatchRanges (ModalRequest _ scoped _ source) = do
  binding <- maybe (Left "modal lifetime request lacks source binding") Right source
  imageRanges <- mapM sourceImageRange (lifetimeReadonly binding ++ lifetimeContext binding
    ++ [SourceImage start bytes | Just (ModalLinked start bytes, _) <- [scoped]])
  let range low high = watchRange (watchAddress low) (watchAddress high)
  fixed <- mapM (uncurry range)
    [ (0x0096cc64,0x0096ce26), (0x00cf59e4,0x00cf59e8), (0x00cfe3d4,0x00cfe3d8)
    , (0x00d00268,0x00d0026c), (0x00cd5f0e,0x00cd5f10), (0x00cd1ae4,0x00cd3f8c)
    , (0x00d04080,0x00d0452c), (0x00d01370,0x00d01378), (0x01467588,0x01467840)
    , (0x00bfb028,0x00bfb124)
    , (0x54760,0x54764), (0x54768,0x5476c), (0x545a4,0x545a8), (0x5ff0c,0x6194c), (0x5beac,0x5d16c)
    , (0xe4720,0xe4724), (0xe4728,0xe472c), (0xe4564,0xe4568), (0xefecc,0xf190c), (0xebe6c,0xed12c) ]
  publications <- mapM (\(PublicationBinding address _ _) -> range (watchAddressValue address) (watchAddressValue address + 4)) (lifetimePublications binding)
  let guards = [task33StackGuardRange | isJust (lifetimeTask33StackGuard binding)]
      watched = unionWatchRanges (lifetimeResident binding : fixed ++ imageRanges ++ publications ++ guards)
  unless (not (or [rangesOverlap retired span_ | retired <- lifetimeRetired binding, span_ <- watched]))
    (Left "modal retired extents overlap a write-watched extent")
  let reloadSites = concatMap reloadSiteRanges (lifetimeSettingsReloads binding)
  unless (not (or [rangesOverlap site span_ | site <- reloadSites, span_ <- lifetimeRuntime binding : lifetimeRetired binding ++ watched]))
    (Left "modal settings reload sites overlap the runtime, a retired extent or a write-watched extent")
  pure watched

checkModalLifetimeWatchSources :: ModalRequest -> Value -> Either String ()
checkModalLifetimeWatchSources (ModalRequest _ scoped _ source) value = do
  binding <- maybe (Left "modal lifetime request lacks source binding") Right source
  catalogue <- case lifetimeCatalogueBinding binding of
    RuntimeContextUnbound -> Left "modal cache-only watch lacks a source-bound context catalogue"
    SourceContextBound proof -> Right proof
  _ <- projectCacheOnlyWatch value
  attribution <- modalField "fetch_attribution" value
  actual <- modalField "ranges" attribution >>= modalList (either error id (modalLimit 65536)) >>= mapM parseModalExtent
  let instructionPc (SourceInstruction pc _) = watchAddressValue pc
      routinePcs (PublicationRoutine entry writer) = map instructionPc [entry, writer]
      fixedPcs = [0x00bfb028,0x00bfb02c .. 0x00bfb120]
        ++ map (watchAddressValue . Catalogue.catalogueInstructionAddress) (Catalogue.contextCatalogueInstructions catalogue)
        ++ routinePcs (lifetimeInstall binding) ++ routinePcs (lifetimeRollback binding)
      scopedPcs = case scoped of
        Nothing -> []
        Just (_, ModalScope seam owned helpers prehook) ->
          [fromIntegral pc | span_ <- [seam, owned, modalPrehookCode helpers, modalCallCode helpers], let (low, high) = watchRangeBounds span_, pc <- [low,low + 4 .. high - 4]]
            ++ maybe [] (pure . watchAddressValue) prehook
  expected <- mapM (\pc -> watchRange (watchAddress pc) (watchAddress (pc + 4))) (fixedPcs ++ scopedPcs)
  unless (map watchRangeBounds actual == map watchRangeBounds (unionWatchRanges expected))
    (Left "modal cache-only fetch ranges differ from complete source-bound instruction points")

data ModalInstallOrigin = FreshModalInstall | RestoredModalInstall deriving (Eq, Show)
data ModalUnloadRequest = ModalUnloadRequested | ModalUnloadOmitted deriving (Eq, Show)

checkModalLifetimeRoute :: ModalInstallOrigin -> ModalUnloadRequest -> Either String ()
checkModalLifetimeRoute origin unload = do
  unless (origin == FreshModalInstall) (Left "modal lifetime guard refuses installed checkpoint restore")
  unless (unload == ModalUnloadRequested) (Left "modal lifetime guard requires requested rollback")

lifetimeWatchAcknowledgement :: Value -> Either String Value
lifetimeWatchAcknowledgement state = do
  _ <- parseModalLifetimeWatch modalLifetimeWriterLimit state
  case state of
    Object fields -> do
      enabled <- maybe (Left "modal lifetime watch lacks enabled") Right (KM.lookup "enabled" fields)
      pure (Object (KM.insert "armed" enabled (foldr KM.delete fields ["enabled", "records", "violation"])))
    _ -> Left "modal lifetime watch is not an object"

checkLifetimeArrayAudit :: ModalRequest -> Either String Value -> Either String FinalModalWatch -> Either String ArrayWriteReport
checkLifetimeArrayAudit request armed final = do
  expected <- modalLifetimeWatchRanges request
  armedValue <- armed
  FinalModalWatch observation finalSummary <- final
  checkModalLifetimeWatchSources request armedValue
  checkModalLifetimeWatchSources request finalSummary
  acknowledgement <- lifetimeWatchAcknowledgement armedValue >>= parseModalLifetimeWatchAck
  let fetchClasses = maybe [] lifetimeFetchClasses (case request of ModalRequest _ _ _ source -> source)
  rangeLimit <- watchRangeLimit (length expected + length fetchClasses)
  gate <- checkExecutionWritesWithRangeLimit rangeLimit ForbidProtectedWrites modalLifetimeWriterLimit expected fetchClasses acknowledgement observation
  pure (ArrayWriteReport (writesInArrays observation) gate)

modalLifetimeWriterLimit :: WatchLimit
modalLifetimeWriterLimit = either error id (watchLimit 1048576)
