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
  , modalLifetimeSourceJson
  , lifetimeModalRequest
  , parseModalLifetimeWatchAck
  , parseModalLifetimeWatch
  , checkCompletedModalLifetime
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

import Control.Monad (forM, forM_, unless)
import qualified ArmElf as ELF
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.List (nub, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word64)
import Data.Bits ((.&.))
import Emulation.WatchObservation
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import qualified FujiAF.RamPush as Ram
import Emulation.LoaderCode (loaderSpans)
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
  , modalWatchEvidence :: Maybe ModalWatchEvidence
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
  , "watch" .= fmap (\(ModalWatchEvidence _ raw) -> raw) (modalWatchEvidence report)
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
  watch <- modalField "watch" value >>= parseNullable (\raw -> ModalWatchEvidence <$> parseModalLifetimeWatch modalLifetimeWriterLimit raw <*> pure raw)
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
  forM_ lifetime $ \_ -> unless (modalPhysicalCoverage report) (Left "modal lifetime physical guard did not arm")
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
  unless (maybe False ((== 224) . countValue . receiptPublicationCount) (modalLifetimeReceipt report))
    (Left "scoped modal route requires the complete224 publication vector")
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

parseModalBeginRequest :: Value -> Either String ModalRequest
parseModalBeginRequest value = do
  let required = ["command", "stock", "linked", "seam", "owned", "limit"]
      optional = case value of Object fields -> [key | key <- ["owned_prehook", "helpers", "context_sources", "lifetime"], KM.member key fields]; _ -> []
  modalKeys (required ++ optional) value
  unless (modalField "command" value == Right (String "modal-observation-begin")) (Left "not a modal begin request")
  stock <- modalField "stock" value >>= byteArray 0xfc >>= modalStock
  limit <- modalField "limit" value >>= modalWord >>= modalLimit
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
    (Just context, Just lifetime) -> parseLifetimeBinding scoped context lifetime >>= lifetimeModalRequest request
    _ -> Left "modal lifetime request requires both context sources and lifetime"
 where
  byteArray = parseModalBytes
  parseExtent = parseModalExtent

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
data ModalLifetimeSource = ModalLifetimeSource
  { lifetimeRuntime :: WatchRange
  , lifetimeResident :: WatchRange
  , lifetimeReadonly :: [SourceImage]
  , lifetimeInstall :: PublicationRoutine
  , lifetimeRollback :: PublicationRoutine
  , lifetimePublications :: [PublicationBinding]
  , lifetimeContext :: [SourceImage]
  , lifetimeElfDigest :: Maybe Sha256
  , lifetimeRuntimeDigest :: Maybe Sha256
  } deriving (Eq)

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
  [ "runtime" .= extentJson (lifetimeRuntime source)
  , "resident" .= extentJson (lifetimeResident source)
  , "readonly" .= map sourceImageJson (lifetimeReadonly source)
  , "install" .= routine (lifetimeInstall source)
  , "rollback" .= routine (lifetimeRollback source)
  , "publications" .= [object ["address" .= watchAddressValue address, "old" .= old, "new" .= new]
      | PublicationBinding address (Ram.WordValue old) (Ram.WordValue new) <- lifetimePublications source]
  ]
 where
  instruction (SourceInstruction pc (InstructionWord word)) = object ["pc" .= watchAddressValue pc, "word" .= word]
  routine (PublicationRoutine entry writer) = object ["entry" .= instruction entry, "writer" .= instruction writer]

modalLifetimeSourceJson :: ModalLifetimeSource -> Value
modalLifetimeSourceJson source = object
  [ "bound_elf_sha256" .= fmap sha256Hex (lifetimeElfDigest source)
  , "runtime_sha256" .= fmap sha256Hex (lifetimeRuntimeDigest source)
  , "loader_a_sha256" .= loaderADigest
  , "loader_b_sha256" .= loaderBDigest
  , "context_sources" .= map sourceImageJson (lifetimeContext source)
  , "lifetime" .= lifetimeRequestJson source
  ]

loaderADigest, loaderBDigest :: String
loaderADigest = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
loaderBDigest = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"

immutableLoaderSpans :: Word32 -> Word32 -> [(Word32, Word32)]
immutableLoaderSpans base end = [(fromIntegral low, fromIntegral high) | (low, high) <- loaderSpans (fromIntegral base) (fromIntegral (end - base))]

bindModalLifetimeSource :: Ram.RamPushManifest -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ModalLifetimeSource
bindModalLifetimeSource manifest elfBytes loaderA loaderB = do
  unless (B.length loaderA == 0xdae0 && sha256Hex (sha256 loaderA) == loaderADigest
    && B.length loaderB == 0xdaa0 && sha256Hex (sha256 loaderB) == loaderBDigest)
    (Left "modal lifetime requires complete pinned LoaderA and LoaderB")
  ModalRuntimeSource source <- bindModalRuntimeSource manifest elfBytes
  let context = [SourceImage (watchAddress low) (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base_)) bytes))
        | (base_, end_, bytes) <- [(0x40000, 0x4dae0, loaderA), (0xd0000, 0xddaa0, loaderB)]
        , (low, high) <- immutableLoaderSpans base_ end_]
  pure source { lifetimeContext = context }

newtype ModalRuntimeSource = ModalRuntimeSource ModalLifetimeSource

modalRuntimeSourceJson :: ModalRuntimeSource -> Value
modalRuntimeSourceJson (ModalRuntimeSource source) = object
  [ "bound_elf_sha256" .= fmap sha256Hex (lifetimeElfDigest source)
  , "runtime_sha256" .= fmap sha256Hex (lifetimeRuntimeDigest source)
  , "lifetime" .= lifetimeRequestJson source ]

bindModalRuntimeSource :: Ram.RamPushManifest -> B.ByteString -> Either String ModalRuntimeSource
bindModalRuntimeSource manifest elfBytes = do
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
  unless (length publications `elem` [223, 224] && length (nub addresses) == length addresses
    && all (\address -> address `mod` 4 == 0 && address < 0x20000000) addresses)
    (Left "modal lifetime publication vector requires 223 or 224 unique canonical aligned addresses")
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
  pure (ModalRuntimeSource (ModalLifetimeSource runtimeExtent residentExtent readonly install rollback bindings [] (Just (sha256 elfBytes)) (Just runtimeDigest)))

littleWord :: Word32 -> B.ByteString
littleWord word = B.pack [fromIntegral (word `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]

lifetimeModalRequest :: ModalRequest -> ModalLifetimeSource -> Either String ModalRequest
lifetimeModalRequest (ModalRequest stock scoped limit _) source = do
  unless (length (lifetimeContext source) == 16) (Left "modal lifetime requires complete loader context binding")
  unless (length (lifetimePublications source) == maybe 223 (const 224) scoped)
    (Left "modal lifetime publication count differs from stock or scoped route")
  forM_ scoped $ \(ModalLinked start bytes, _) ->
    unless (SourceImage start bytes `elem` lifetimeReadonly source)
      (Left "modal linked source differs from the bound runtime readonly section")
  let readonly = case scoped of
        Nothing -> lifetimeReadonly source
        Just (ModalLinked start bytes, _) -> filter (/= SourceImage start bytes) (lifetimeReadonly source)
  pure (ModalRequest stock scoped limit (Just source { lifetimeReadonly = readonly }))

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
  case value of
    Object fields -> Right (Object (KM.delete "fetch_attribution" fields))
    _ -> Left "modal lifetime watch is not an object"

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
  | WrongValue | WrongAddress | WrongWidth | InvalidContext | ActiveFrames | IncompleteRollback
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
  , receiptFailure :: Maybe LifetimeFailure
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
  ActiveFrames -> "active_frames"; IncompleteRollback -> "incomplete_rollback"

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
  modalKeys ["phase", "publication_count", "forward_writes", "reverse_writes", "pending_stores",
    "install_core", "rollback_core", "checked_resource_writes", "checked_identity_writes",
    "checked_scheduler_writes", "checked_context_writes", "checked_vfp_owner_writes", "audited_writes", "failure"] value
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
    <*> count "audited_writes" <*> pure failure

lifetimeReceiptJson :: LifetimeReceipt -> Value
lifetimeReceiptJson receipt = object
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
  , "failure" .= fmap (\failure -> object ["kind" .= lifetimeFailureText failure]) (receiptFailure receipt)
  ]

data PhysicalGuard = PhysicalGuard ModalCount ModalCount ModalCount ModalCount (Maybe ModalFailure) deriving (Eq, Show)
data ModalWatchEvidence = ModalWatchEvidence FixedWatchObservation Value deriving (Eq, Show)

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
    in receiptPhase receipt == Ended && count `elem` [223,224]
      && countValue (receiptForwardWrites receipt) == count && countValue (receiptReverseWrites receipt) == count
      && countValue (receiptPendingStores receipt) == 0 && receiptInstallCore receipt /= Nothing
      && receiptRollbackCore receipt /= Nothing && receiptFailure receipt == Nothing
      && countValue (receiptIdentityWrites receipt) == 0
      && modalReportFailure report == Nothing && maybe False observedPhysicalGuard (modalGuard report)
      && not (modalReportEnabled report) && countValue (modalActiveFrames (modalReportCounts report)) == 0
      && modalPhysicalCoverage report && countValue (modalContextSourceSpans report) == 16

checkCompletedModalLifetime :: ModalRequest -> ArrayWriteReport -> ModalReport -> Either String ()
checkCompletedModalLifetime (ModalRequest (ModalStock stock) scoped _ source) audit report = do
  checkModalEnd report
  unless (modalReportPolicy report == maybe StockBaseline (const ScopedOwned75) scoped)
    (Left "completed modal policy differs from the source-bound request")
  binding <- maybe (Left "modal request has no installed lifetime binding") Right source
  receipt <- maybe (Left "modal report has no lifetime receipt") Right (modalLifetimeReceipt report)
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
  ModalWatchEvidence observation _ <- maybe (Left "modal lifetime lacks raw watch evidence") Right (modalWatchEvidence report)
  let rows = fixedWatchRecords observation
      expectedWrites = sum (map (toInteger . countValue)
        [receiptForwardWrites receipt, receiptReverseWrites receipt, receiptResourceWrites receipt
        , receiptSchedulerWrites receipt, receiptContextWrites receipt, receiptVfpOwnerWrites receipt, receiptAuditedWrites receipt])
  unless (toInteger (length rows) == expectedWrites
    && toInteger (length (arrayWrites audit)) == toInteger (countValue (receiptAuditedWrites receipt))
    && all (\row -> case row of CpuRecord { cpuAccess = CpuWrite } -> True; _ -> False) rows)
    (Left "modal lifetime accepted write counters differ from raw CPU rows or array audit")
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

parseLifetimeBinding :: Maybe (ModalLinked, ModalScope) -> Value -> Value -> Either String ModalLifetimeSource
parseLifetimeBinding scoped contextValue value = do
  modalKeys ["runtime", "resident", "readonly", "install", "rollback", "publications"] value
  runtime <- modalField "runtime" value >>= parseModalExtent
  resident <- modalField "resident" value >>= parseModalExtent
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
  publications <- modalField "publications" value >>= modalList (either error id (modalLimit 224)) >>= mapM (\entry -> do
    modalKeys ["address", "old", "new"] entry
    address <- modalField "address" entry >>= modalWord32
    unless (address `mod` 4 == 0 && address < 0x20000000) (Left "modal lifetime publication is not aligned canonical RAM")
    PublicationBinding (watchAddress address) <$> (Ram.WordValue <$> (modalField "old" entry >>= modalWord32))
      <*> (Ram.WordValue <$> (modalField "new" entry >>= modalWord32)))
  let addresses = [address | PublicationBinding address _ _ <- publications]
  unless (length publications == maybe 223 (const 224) scoped && length (nub addresses) == length addresses)
    (Left "modal lifetime publication count or uniqueness differs from route")
  pure (ModalLifetimeSource runtime resident allImages install rollback publications context Nothing Nothing)

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
    , (0x54760,0x54764), (0x545a4,0x545a8), (0x5ff0c,0x6194c), (0x5beac,0x5d16c)
    , (0xe4720,0xe4724), (0xe4564,0xe4568), (0xefecc,0xf190c), (0xebe6c,0xed12c) ]
  publications <- mapM (\(PublicationBinding address _ _) -> range (watchAddressValue address) (watchAddressValue address + 4)) (lifetimePublications binding)
  pure (unionWatchRanges (lifetimeResident binding : fixed ++ imageRanges ++ publications))

checkModalLifetimeWatchSources :: ModalRequest -> Value -> Either String ()
checkModalLifetimeWatchSources (ModalRequest _ scoped _ source) value = do
  binding <- maybe (Left "modal lifetime request lacks source binding") Right source
  _ <- projectCacheOnlyWatch value
  attribution <- modalField "fetch_attribution" value
  actual <- modalField "ranges" attribution >>= modalList (either error id (modalLimit 65536)) >>= mapM parseModalExtent
  let instructionPc (SourceInstruction pc _) = watchAddressValue pc
      routinePcs (PublicationRoutine entry writer) = map instructionPc [entry, writer]
      fixedPcs = [0x00bfb028,0x00bfb02c .. 0x00bfb120]
        ++ [0x40240,0x4031c,0x436b8,0x43690,0x436f0,0x43720,0x43848,0x42440,0xd0240,0xd031c,0xd36b8,0xd3690,0xd36f0,0xd3720,0xd3848,0xd2440]
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

checkLifetimeArrayAudit :: ModalRequest -> Either String Value -> Either String Value -> Either String ArrayWriteReport
checkLifetimeArrayAudit request armed final = do
  expected <- modalLifetimeWatchRanges request
  armedValue <- armed
  finalValue <- final
  checkModalLifetimeWatchSources request armedValue
  checkModalLifetimeWatchSources request finalValue
  acknowledgement <- armed >>= lifetimeWatchAcknowledgement >>= parseModalLifetimeWatchAck
  observation <- final >>= parseModalLifetimeWatch bound
  rangeLimit <- watchRangeLimit (length expected)
  gate <- checkExecutionWritesWithRangeLimit rangeLimit ForbidProtectedWrites bound expected [] acknowledgement observation
  pure (ArrayWriteReport (writesInArrays observation) gate)
 where
  bound = modalLifetimeWriterLimit

modalLifetimeWriterLimit :: WatchLimit
modalLifetimeWriterLimit = either error id (watchLimit 1048576)
