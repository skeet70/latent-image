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
  ) where

import Control.Monad (forM_, unless)
import qualified ArmElf as ELF
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word64)
import Emulation.WatchObservation
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
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
  let writes =
        [ ArrayWrite (OperationIndex index) writer intersections
        | (index, record) <- zip [0 ..] (fixedWatchRecords observation)
        , Just (writer, physical, bytes) <- [writeAttribution record]
        , let intersections = intersectArrays physical bytes
        , not (null intersections)
        ]
  pure (ArrayWriteReport writes gate)

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

data ModalScope = ModalScope WatchRange WatchRange (Maybe WatchAddress) deriving (Eq, Show)

modalScope :: ModalLinked -> WatchRange -> WatchRange -> Maybe WatchAddress -> Either String ModalScope
modalScope (ModalLinked start bytes) seam owned prehook = do
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
  forM_ prehook $ \address -> do
    let pc = toInteger (watchAddressValue address)
    unless (pc `mod` 4 == 0 && sourceLow <= pc && pc + 4 <= sourceHigh)
      (Left "modal owned prehook must be an aligned linked source instruction")
  pure (ModalScope seam owned prehook)

data ModalRequest = ModalRequest ModalStock (Maybe (ModalLinked, ModalScope)) ModalLimit deriving (Eq, Show)

modalRequest :: ModalStock -> Maybe (ModalLinked, ModalScope) -> ModalLimit -> Either String ModalRequest
modalRequest stock scoped limit = do
  forM_ scoped $ \(linked, ModalScope seam owned prehook) -> modalScope linked seam owned prehook
  pure (ModalRequest stock scoped limit)

modalBeginRequestJson :: ModalRequest -> Value
modalBeginRequestJson (ModalRequest (ModalStock stock) scoped limit) = object
  ([ "command" .= ("modal-observation-begin" :: String)
   , "stock" .= B.unpack stock
   , "linked" .= fmap (\(ModalLinked start bytes, _) -> object ["start" .= watchAddressValue start, "bytes" .= B.unpack bytes]) scoped
   , "seam" .= fmap (\(_, ModalScope seam _ _) -> bounds seam) scoped
   , "owned" .= fmap (\(_, ModalScope _ owned _) -> bounds owned) scoped
   , "limit" .= modalLimitValue limit
   ] ++ ["owned_prehook" .= watchAddressValue prehook | Just (_, ModalScope _ _ (Just prehook)) <- [scoped]])
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
  , "physical_fetch_coverage" .= False
  , "task_identity_verified" .= False
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
  modalKeys ["enabled", "policy", "counts", "stock_fetches", "captured_domains", "failure", "physical_fetch_coverage", "task_identity_verified"] value
  enabled <- modalField "enabled" value >>= modalBool
  policy <- modalField "policy" value >>= \entry -> case entry of
    String "stock-baseline" -> Right StockBaseline
    String "scoped-owned75" -> Right ScopedOwned75
    _ -> Left "unknown modal policy"
  forM_ ["physical_fetch_coverage", "task_identity_verified"] $ \key ->
    unless (modalField key value == Right (Bool False)) (Left "modal report makes an unsupported coverage claim")
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
  pure (ModalReport enabled policy counts fetches domains failure)
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
checkModalBegin (ModalRequest _ scoped _) report = do
  unless (modalReportEnabled report) (Left "modal observation did not enable")
  unless (modalReportPolicy report == maybe StockBaseline (const ScopedOwned75) scoped) (Left "modal policy differs from the request")
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
      optional = case value of Object fields -> ["owned_prehook" | KM.member "owned_prehook" fields]; _ -> []
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
  scoped <- case (linkedValue, seamValue, ownedValue) of
    (Null, Null, Null) -> do
      unless (prehook == Nothing) (Left "baseline modal request cannot capture an owned prehook")
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
      scope <- modalScope linked seam owned prehook
      pure (Just (linked, scope))
  modalRequest stock scoped limit
 where
  byteArray upper (Array bytes) = do
    unless (V.length bytes <= upper) (Left "modal source exceeds its byte bound")
    B.pack <$> mapM (\entry -> do
      byte <- modalWord entry
      unless (byte <= 255) (Left "modal source byte exceeds u8")
      pure (fromIntegral byte)) (V.toList bytes)
  byteArray _ _ = Left "modal source is not a byte array"
  parseExtent (Array pair) | V.length pair == 2 = do
    low <- modalWord32 (pair V.! 0)
    high <- modalWord32 (pair V.! 1)
    watchRange (watchAddress low) (watchAddress high)
  parseExtent _ = Left "modal extent must be a u32 start/end pair"

modalRequestLimit :: ModalRequest -> ModalLimit
modalRequestLimit (ModalRequest _ _ limit) = limit

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
            && ELF.symbolSize symbol == bytes -> pure ()
          _ -> Left "modal manifest function differs from its bound ELF symbol"
        watchRange (watchAddress address) (watchAddress (fromInteger end))
  seam <- extent "rb_modal_entry"
  owned <- extent "rb_modal_dispatch"
  prehook <- extent "rb_modal_call_prehook"
  _ <- extent "rb_modal_call"
  let (prehookLow, _) = watchRangeBounds prehook
  scope <- modalScope linked seam owned (Just (watchAddress (fromIntegral prehookLow)))
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
scopedModalSourceJson (ScopedModalSource (ModalLinked start bytes) (ModalScope seam owned prehook) elfDigest runtimeDigest) = object
  [ "bound_elf_sha256" .= sha256Hex elfDigest
  , "runtime_sha256" .= sha256Hex runtimeDigest
  , "linked_text_start" .= watchAddressValue start
  , "linked_text_bytes" .= B.length bytes
  , "seam" .= bounds seam
  , "owned" .= bounds owned
  , "owned_prehook" .= fmap watchAddressValue prehook
  , "global_admission" .= False
  ]
 where
  bounds range = let (low, high) = watchRangeBounds range in [low, high]
