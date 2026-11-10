{-# LANGUAGE OverloadedStrings #-}
-- | The property-service natural pair's consumer. It admits one role's observation from its raw
-- trace hits, watch rows and region bytes, and compares two admitted roles route by route.
module Emulation.PropertyPairCheck
  ( Observation (..)
  , Bindings (..)
  , BootSession(..)
  , bootResetPaths
  , IntervalName (..)
  , Refusal (..)
  , Call (..)
  , CallerContext(..)
  , BankApplyFault(..)
  , WriteEffect (..)
  , SettleStatus (..)
  , Press (..)
  , RouteClass (..)
  , RouteResult (..)
  , PersistenceEvent (..)
  , persistenceEvents
  , BlockRunAt (..)
  , Qualified (..)
  , GetterKey (..)
  , GetterResult (..)
  , AdmittedBindings
  , admitBindings
  , planFor
  , specFor
  , qualify
  , compareRoles
  , qualifiedValue
  , comparisonValue
  , ControlName (..)
  , controlNames
  , checkGuestControl
  , ControlOutcome (..)
  , StaleGetter (..)
  , runControl
  ) where

import Control.Monad (foldM, forM, forM_, unless, when, zipWithM_)
import qualified CustomAdmission as StorageBoundaries
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (parseEither)
import Data.Bits (complement, shiftL, shiftR, xor, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (find, sort, zip4)
import qualified Data.List.NonEmpty as NE
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isNothing, isJust, catMaybes)
import Data.Int (Int32)
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word64, Word8)
import Emulation.LocalUiMenu (LocalUiMenu, parseLocalUiMenu, LocalUiMode, parseLocalUiMode)
import Emulation.PropertyPairPlan
import Emulation.PropertyPairRoutes
import qualified Emulation.SettingsStorageMatrix as Storage
import Emulation.Sha256 (Sha256, sha256, sha256Hex, parseSha256Hex)
import EmulatorUsb (CrawlKey(..), ModeDial(..), crawlKeyWire, modeDialName)
import Numeric (readHex, showHex)
import Replacement.PropertyService.Placement (Carrier(..), GuestAddress(..))
import qualified Replacement.RecordStore.Hardware as RecordHardware
import qualified Replacement.RecordStore.Model as RecordModel
import Text.Printf (printf)

-- | The sidecars of one observation directory.
data Observation = Observation
  { obsReceipt :: B.ByteString
  , obsSpec :: B.ByteString
  , obsReset :: B.ByteString
  , obsHits :: B.ByteString
  , obsSteps :: B.ByteString
  , obsSession :: B.ByteString
  , obsBootResets :: M.Map BootSession B.ByteString
  }

newtype BootSession = BootSession { bootSessionNumber :: Word32 } deriving (Eq, Ord, Show)

-- | The bound inputs: canonical and role DATs and the observer by digest, the rest by bytes.
data Bindings = Bindings
  { bindSourceDat :: String
  , bindOwnedDat :: String
  , bindRoutes :: B.ByteString
  , bindContract :: B.ByteString
  , bindLinked :: B.ByteString
  , bindSymbols :: B.ByteString
  , bindImages :: M.Map ImageName B.ByteString
  , bindObserver :: String
  , bindElf :: Maybe B.ByteString
  }

-- | Immutable bindings admitted once per invocation, with both role plans derived from them.
-- Construct only with 'admitBindings'; a raw 'Bindings' mutation must be admitted anew.
data AdmittedBindings = AdmittedBindings
  { admittedBindings :: Bindings
  , admittedContract :: Contract
  , admittedWorkload :: Workload
  , admittedSourceDat :: Sha256
  , admittedOwnedDat :: Sha256
  , admittedObserver :: Sha256
  , admittedRoutesDigest :: Sha256
  , admittedContractDigest :: Sha256
  , admittedLinkedDigest :: Sha256
  , admittedSymbolsDigest :: Sha256
  , admittedImageDigests :: M.Map ImageName Sha256
  , admittedElfDigest :: Maybe Sha256
  , admittedBoundary :: Maybe StorageBoundaries.BankApplyBoundary
  , admittedRetainedPlan :: RolePlan
  , admittedOwnedPlan :: RolePlan
  }

data RolePlan = RolePlan
  { rolePlanPlan :: Plan
  , rolePlanSpec :: Value
  , rolePlanSpecWire :: B.ByteString
  , rolePlanImage4Digest :: Sha256
  }

-- | A reset segment, or a step by its 1-based number.
data IntervalName = PhaseInterval AttachPhase | BootPhaseInterval BootSession AttachPhase | StepInterval Int deriving (Eq, Ord, Show)

data Refusal
  = InputDigest String
  | ReceiptShape String
  | SidecarDigest String
  | ObservationRefused Value
  | SiteMapDiffers
  | HitShape Int String
  | StepShape Int String
  | WatchShape String String
  | HitBound Int
  | HitPartition IntervalName
  | RegionContinuity IntervalName
  | ReplayMismatch IntervalName Word32
  | UnarmedSite Int
  | GuardedFetch Int
  | UnarmedReturn Int
  | DuplicateOpenCall Int
  | BankApplyContextRefused Int BankApplyFault
  | AccessorStoreBound Int
  | AccessorStoreUnmatched IntervalName
  | PersistenceShape String
  | RouteUnobserved RouteName
  | ForeignWriter RouteName Word32
  | RouteEffectsDiffer RouteName
  | DeclaredRouteInert RouteName
  | CohortDiffers String
  | RouteSetDiffers
  -- | The first key whose collapsed result sequences differ, and the index of their first differing value.
  | GetterSequenceDiffers GetterKey Int
  | SetterSequenceDiffers RouteName
  | WriteSequenceDiffers RouteName
  | TransferSequenceDiffers RouteName
  | AppendSequenceDiffers RouteName
  | FinalBytesDiffer RouteName
  | PressDiffers RouteName
  | StorageShape String
  | StorageCheckFailed Storage.MatrixStepName Storage.StorageCheck
  | StorageCoverageMissing StorageCriterion
  | StorageStepsDiffer
  deriving (Eq, Show)

data CallerContext = ExactCaller | BankApplyCaller TraceCore LogicalTaskId deriving (Eq, Show)
-- | One getter or setter entry: r0-r3, the caller's return address and r0 at that return, absent
-- when the route ends first.
data Call = Call { callArguments :: [Word32], callReturn :: Word32, callResult :: Maybe Word32, callContext :: CallerContext } deriving (Eq, Show)

data BankApplyFault = ApplyTaskUnavailable | ApplyNotTaskExecution | ApplyEntryMissing | ApplyCallerInvalid | ApplyReturnUnexpected | ApplyReturnMissing deriving (Eq, Show)

data WriteEffect = WriteEffect
  { effectAddress :: Word32
  , effectWidth :: Int
  -- | Each written byte; a byte of a write wider than eight bytes is known only inside the watched regions.
  , effectBytes :: [Maybe Word8]
  , effectBefore :: [Word8]
  , effectWriter :: WriterClass
  , effectPc :: Word32
  } deriving (Eq, Show)

-- | Whether the write leaves a byte other than its preimage.
effectChanges :: WriteEffect -> Bool
effectChanges write = or [byte /= Just old | (byte, old) <- zip (effectBytes write) (effectBefore write)]

data TransferEffect = TransferEffect { transferSource :: T.Text, transferAddress :: Word32, transferWidth :: Int } deriving (Eq, Show)

-- | The bridge's `settle.status` of a pressed key.
data SettleStatus = PriorRenderPending | PacketConsumed | BudgetExhausted deriving (Eq, Show, Enum, Bounded)

settleStatusWire :: SettleStatus -> T.Text
settleStatusWire status = case status of
  PriorRenderPending -> "prior-render-pending"
  PacketConsumed -> "packet-consumed"
  BudgetExhausted -> "budget-exhausted"

data Press = Press { pressStatus :: SettleStatus, pressMenu :: Value, pressEndpointMenu :: LocalUiMenu, pressEndpointMode :: LocalUiMode } deriving (Eq, Show)

data RouteClass = PhaseClass | CrawlClass | DeclaredClass | StorageClass deriving (Eq, Ord, Show, Enum, Bounded)

routeClassWire :: RouteClass -> T.Text
routeClassWire routeClass = case routeClass of
  PhaseClass -> "phase"
  CrawlClass -> "crawl"
  DeclaredClass -> "declared"
  StorageClass -> "storage"

newtype CustomRecordWire = CustomRecordWire B.ByteString deriving (Eq, Show)
newtype CustomPayloadBytes = CustomPayloadBytes B.ByteString deriving (Eq, Show)
newtype ProgramBytes = ProgramBytes B.ByteString deriving (Eq, Show)
newtype ProgramOffset = ProgramOffset Word32 deriving (Eq, Show)
newtype MediumStatus = MediumStatus Int32 deriving (Eq, Show)
data RecordBank = RecordBank0 | RecordBank1 deriving (Eq, Show)
data RecordReadBuffer = PrimaryRecordReadBuffer | SecondaryRecordReadBuffer deriving (Eq, Show)
data PersistenceEvent
  = HistoricalAppend [Word32]
  | CustomAppend CustomRecordWire CustomRecordWire CustomPayloadBytes
  | CustomRead RecordBank RecordReadBuffer
  | CustomProgram RecordBank ProgramOffset ProgramBytes
  | CustomProgramResult MediumStatus
  | CustomErase RecordBank
  | CustomEraseResult MediumStatus
  deriving (Eq, Show)

data PendingMedium = ProgramPending | ErasePending
data MediaPairState = MediaPairState { pendingMedium :: Maybe PendingMedium, appendCandidate :: Maybe CustomRecordWire }
appendCount :: [PersistenceEvent] -> Int
appendCount = length . filter (\event -> case event of HistoricalAppend _ -> True; CustomAppend {} -> True; _ -> False)

data RouteResult = RouteResult
  { resultRoute :: RouteName
  , resultClass :: RouteClass
  , resultGetters :: [Call]
  , resultSetters :: [Call]
  , resultPersistence :: [PersistenceEvent]
  , resultWrites :: [WriteEffect]
  , resultTransfers :: [TransferEffect]
  , resultFinal :: [(Word32, B.ByteString)]
  , resultPresses :: [Press]
  -- | The time-driven block runs of the last step's setter and commit effects.
  , resultEffectBlocks :: [BlockRunAt]
  } deriving (Eq, Show)

data Qualified = Qualified
  { qualifiedRole :: Role
  , qualifiedCohort :: [(String, String)]
  , qualifiedHits :: Int
  , qualifiedSteps :: Int
  , qualifiedRoutes :: [RouteResult]
  -- | Every returned getter call of the observation in session order.
  , qualifiedGetters :: [GetterResult]
  , qualifiedStorage :: Maybe StorageEvidence
  }

-- | A getter call's caller and r0-r3.
data GetterKey = GetterKey { getterCaller :: Word32, getterArguments :: [Word32] } deriving (Eq, Ord, Show)

-- | A returned getter call: its key, r0 at its return and the return's hit index.
data GetterResult = GetterResult { getterKey :: GetterKey, getterValue :: Word32, getterReturnHit :: Int } deriving (Eq, Show)

-- Raw sidecar decoding.

type Parse = Either String

asObject :: Value -> Parse (KM.KeyMap Value)
asObject (Object fields) = Right fields
asObject _ = Left "not an object"

exact :: [T.Text] -> KM.KeyMap Value -> Parse ()
exact names fields = unless (sort (map K.toText (KM.keys fields)) == sort names) (Left ("keys differ from " ++ show names))

at :: T.Text -> KM.KeyMap Value -> Parse Value
at name fields = maybe (Left ("missing " ++ T.unpack name)) Right (KM.lookup (K.fromText name) fields)

asInt :: Value -> Parse Int
asInt (Number number) = maybe (Left "not a bounded integer") Right (toBoundedInteger number)
asInt _ = Left "not a number"

asWord :: Value -> Parse Word32
asWord value = asInt value >>= \number -> if number >= 0 && number <= 0xffffffff then Right (fromIntegral number) else Left "word out of range"

asWord64 :: Value -> Parse Word64
asWord64 (Number number) = maybe (Left "not a 64-bit integer") Right (toBoundedInteger number)
asWord64 _ = Left "not a number"

asText :: Value -> Parse T.Text
asText (String text) = Right text
asText _ = Left "not text"

asArray :: Value -> Parse [Value]
asArray (Array values) = Right (V.toList values)
asArray _ = Left "not an array"

hexWordText :: Value -> Parse Word32
hexWordText value = asText value >>= \text -> case T.unpack text of
  '0' : 'x' : digits | not (null digits), [(number, "")] <- readHex digits, number <= (0xffffffff :: Integer) -> Right (fromIntegral number)
  _ -> Left "not a 0x word"

hexBytes :: T.Text -> Parse B.ByteString
hexBytes text
  | odd (T.length text) = Left "odd hexadecimal length"
  | otherwise = B.pack <$> mapM pair (T.chunksOf 2 text)
 where
  pair chunk = case readHex (T.unpack chunk) of
    [(number, "")] -> Right (fromIntegral (number :: Int))
    _ -> Left "not hexadecimal"

-- | The trace sites the RAM-push bridge arms itself after attach.
data BridgeSite
  = RequestBootstrap | RequestBootstrapReturn | QueryCallbackResult | QueryResponseReady | Cpu1SyncWrapper
  | Cpu1RangeWrapper | Cpu1CacheControl | Cpu1Worker | Cpu1WorkerCompletion
  deriving (Eq, Show, Enum, Bounded)

bridgeSiteWire :: BridgeSite -> T.Text
bridgeSiteWire site = case site of
  RequestBootstrap -> "request-bootstrap"
  RequestBootstrapReturn -> "request-bootstrap-return"
  QueryCallbackResult -> "query-callback-result"
  QueryResponseReady -> "query-response-ready"
  Cpu1SyncWrapper -> "cpu1-sync-wrapper"
  Cpu1RangeWrapper -> "cpu1-range-wrapper"
  Cpu1CacheControl -> "cpu1-cache-control"
  Cpu1Worker -> "cpu1-worker"
  Cpu1WorkerCompletion -> "cpu1-worker-completion"

bridgeSiteAddress :: BridgeSite -> Word32
bridgeSiteAddress site = case site of
  RequestBootstrap -> 0x01469b20
  RequestBootstrapReturn -> 0x01469b38
  QueryCallbackResult -> 0x00d806e8
  QueryResponseReady -> 0x00d8073c
  Cpu1SyncWrapper -> 0x006b0aa8
  Cpu1RangeWrapper -> 0x006b0c3c
  Cpu1CacheControl -> 0x0061c490
  Cpu1Worker -> 0x006c6b5c
  Cpu1WorkerCompletion -> 0x006b0704

data HitSite = PlanSite SiteLabel | BridgeSite BridgeSite deriving (Eq, Show)

parseHitSite :: T.Text -> Parse HitSite
parseHitSite text = case parseSiteLabel (T.unpack text) of
  Just label -> Right (PlanSite label)
  Nothing -> maybe (Left ("unknown trace site " ++ T.unpack text)) (Right . BridgeSite)
    (lookup text [(bridgeSiteWire site, site) | site <- [minBound .. maxBound]])

data TraceCore = TraceCoreA | TraceCoreB deriving (Eq, Ord, Show)
newtype LogicalTaskId = LogicalTaskId Word32 deriving (Eq, Ord, Show)
newtype StackBytes = StackBytes Word32 deriving (Eq, Show)
data TaskStack = TaskStack GuestAddress StackBytes deriving (Eq, Show)
data TaskContext = TaskContext
  { taskKernel :: TraceCore
  , taskLogicalId :: LogicalTaskId
  , taskNode :: GuestAddress
  , taskCreation :: GuestAddress
  , taskStack :: TaskStack
  , taskContextDepth :: Word32
  , taskFlags :: Word32
  , taskSp :: GuestAddress
  } deriving (Eq, Show)
data TaskUnavailable = NoCurrentTask | InvalidTaskIdentity | UnavailableTaskMemory deriving (Eq, Show)
data CapturedTask = AvailableTask TaskContext | UnavailableTask TaskUnavailable deriving (Eq, Show)

parseCapturedTask :: TraceCore -> Word32 -> Word32 -> Value -> Parse CapturedTask
parseCapturedTask core flags sp value = do
  fields <- asObject value
  kind <- at "kind" fields >>= asText
  case kind of
    "unavailable" -> do
      exact ["kind","reason"] fields
      reason <- at "reason" fields >>= asText
      UnavailableTask <$> case reason of
        "no-current-task" -> Right NoCurrentTask
        "invalid-task-identity" -> Right InvalidTaskIdentity
        "unavailable-task-memory" -> Right UnavailableTaskMemory
        _ -> Left "unknown unavailable-task reason"
    "available" -> do
      exact ["kind","kernel","task_id","node","creation","stack","context_depth","cpsr","sp"] fields
      kernel <- at "kernel" fields >>= asText >>= \name -> case name of
        "a" -> Right TraceCoreA
        "b" -> Right TraceCoreB
        _ -> Left "unknown task kernel"
      ident <- at "task_id" fields >>= asWord
      unless (ident >= 1 && ident <= 120 && kernel == core) (Left "task identity or execution core differs")
      node <- at "node" fields >>= hexWordText
      creation <- at "creation" fields >>= hexWordText
      unless (physical node /= 0 && physical creation /= 0) (Left "available task has a null identity")
      stack <- at "stack" fields >>= asObject
      exact ["base","bytes"] stack
      base <- at "base" stack >>= hexWordText
      bytes <- at "bytes" stack >>= asWord
      unless (bytes > 0 && toInteger base + toInteger bytes <= 0x100000000
        && toInteger (physical base) + toInteger bytes <= 0x100000000)
        (Left "invalid task stack extent")
      depth <- at "context_depth" fields >>= asWord
      savedFlags <- at "cpsr" fields >>= asWord
      savedSp <- at "sp" fields >>= hexWordText
      unless (savedFlags == flags && savedSp == sp) (Left "task context is not the trace-entry snapshot")
      pure (AvailableTask (TaskContext kernel (LogicalTaskId ident) (GuestAddress node) (GuestAddress creation)
        (TaskStack (GuestAddress base) (StackBytes bytes)) depth savedFlags (GuestAddress savedSp)))
    _ -> Left "unknown task capture kind"
  where physical address = address .&. complement 0x40000000


data Hit = Hit
  { hitSite :: HitSite
  , hitAddress :: Word32
  , hitCore :: TraceCore
  , hitInstructions :: Instructions
  , hitArguments :: [Word32]
  , hitR8 :: Maybe Word32
  , hitTask :: Maybe CapturedTask
  , hitSp :: Word32
  , hitLr :: Word32
  , hitFlags :: Word32
  , hitMemory :: [B.ByteString]
  }

parseHit :: Value -> Parse Hit
parseHit value = do
  fields <- asObject value
  exact (["site", "address", "core", "instructions", "registers", "sp", "lr", "cpsr"]
    ++ ["memory" | KM.member "memory" fields] ++ ["task" | KM.member "task" fields]) fields
  registers <- at "registers" fields >>= asObject
  let general = KM.member "r8" registers
  exact (["r0", "r1", "r2", "r3"] ++ if general then ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12"] else []) registers
  arguments <- mapM (\name -> at name registers >>= hexWordText) ["r0", "r1", "r2", "r3"]
  rawCore <- at "core" fields >>= asInt
  core <- case rawCore of
    0 -> Right TraceCoreA
    1 -> Right TraceCoreB
    _ -> Left "core is neither 0 nor 1"
  instructions <- Instructions <$> (at "instructions" fields >>= asWord64)
  r8 <- if general then do
    forM_ ["r4", "r5", "r6", "r7", "r9", "r10", "r11", "r12"] $ \name ->
      at name registers >>= hexWordText >> pure ()
    Just <$> (at "r8" registers >>= hexWordText)
    else pure Nothing
  flags <- at "cpsr" fields >>= asWord
  sp <- at "sp" fields >>= hexWordText
  task <- traverse (parseCapturedTask core flags sp) (KM.lookup "task" fields)
  memory <- case KM.lookup "memory" fields of
    Nothing -> Right []
    Just ranges -> asArray ranges >>= mapM (\bytes -> asText bytes >>= hexBytes)
  Hit <$> (at "site" fields >>= asText >>= parseHitSite) <*> (at "address" fields >>= hexWordText)
    <*> pure core <*> pure instructions <*> pure arguments <*> pure r8
    <*> pure task <*> pure sp <*> (at "lr" fields >>= hexWordText) <*> pure flags <*> pure memory

persistenceEvents :: Contract -> Value -> Either Refusal [PersistenceEvent]
persistenceEvents contract value = do
  raw <- either (Left . PersistenceShape) Right (asArray value)
  hits <- forM (zip [0..] raw) $ \(index,entry) -> either (Left . HitShape index) Right (parseHit entry)
  normalizePersistence contract hits

normalizePersistence :: Contract -> [Hit] -> Either Refusal [PersistenceEvent]
normalizePersistence contract hits = map snd <$> normalizePersistenceLocated contract (zip [0..] hits)

normalizePersistenceLocated :: Contract -> [(Int,Hit)] -> Either Refusal [(Int,PersistenceEvent)]
normalizePersistenceLocated contract hits = case contractPublication contract of
  Nothing -> Right [(index,HistoricalAppend (hitArguments hit)) | (index,hit) <- hits, hitSite hit == PlanSite Area3Append]
  Just _ -> do
    events <- catMaybes <$> mapM (\(index,hit) -> fmap (fmap (\value -> (index,value))) (event hit)) hits
    final <- foldM pairMedia (MediaPairState Nothing Nothing) (map snd events)
    ensure "custom medium result is missing" (isNothing (pendingMedium final))
    pure events
  where
    need message = maybe (Left (PersistenceShape message)) Right
    ensure message condition = unless condition (Left (PersistenceShape message))
    physical address = address .&. complement 0x40000000
    bank value = case value of 0 -> Right RecordBank0; 1 -> Right RecordBank1; _ -> Left (PersistenceShape "invalid custom medium bank")
    pairMedia state event' = case (pendingMedium state,event') of
      (_,CustomAppend working _ _) -> Right state{appendCandidate=Just working}
      (Nothing,CustomProgram _ _ (ProgramBytes bytes)) -> do
        CustomRecordWire candidate <- need "custom program has no append request" (appendCandidate state)
        ensure "custom program changed the append metadata or payload" (B.take 42 (B.drop 2 bytes) == B.take 42 (B.drop 2 candidate))
        Right state{pendingMedium=Just ProgramPending,appendCandidate=Nothing}
      (Nothing,CustomErase {}) -> Right state{pendingMedium=Just ErasePending}
      (Just ProgramPending,CustomProgramResult _) -> Right state{pendingMedium=Nothing}
      (Just ErasePending,CustomEraseResult _) -> Right state{pendingMedium=Nothing}
      (_,CustomProgram {}) -> Left (PersistenceShape "nested custom medium program")
      (_,CustomErase {}) -> Left (PersistenceShape "nested custom medium erase")
      (_,CustomProgramResult _) -> Left (PersistenceShape "custom program result without request")
      (_,CustomEraseResult _) -> Left (PersistenceShape "custom erase result without request")
      _ -> Right state
    event hit = case (hitSite hit,hitArguments hit) of
      (PlanSite Area3Append,arguments) -> case hitMemory hit of
        [working,previous,live,handle] -> do
          ensure "custom append snapshots have wrong width" (map B.length [working,previous,live,handle] == [52,52,40,4])
          let retainedBoundary = physical (hitAddress hit) == area3Append
          case arguments of
            wrapper:seed:record:_ | retainedBoundary ->
              if physical wrapper /= physical (word32At handle 0) then Right Nothing else do
                ensure "custom append seed or record identity differs" (seed == 0 && physical record == 0x89f874)
                ensure "custom append live payload differs from candidate" (B.take 40 (B.drop 4 working) == live)
                pure (Just (CustomAppend (CustomRecordWire working) (CustomRecordWire previous) (CustomPayloadBytes live)))
            _:_:record:_ | not retainedBoundary -> do
              ensure "typed custom append record identity differs" (physical record == 0x89f874)
              ensure "custom append live payload differs from candidate" (B.take 40 (B.drop 4 working) == live)
              pure (Just (CustomAppend (CustomRecordWire working) (CustomRecordWire previous) (CustomPayloadBytes live)))
            _ -> Left (PersistenceShape "custom append registers missing")
        _ -> Left (PersistenceShape "custom append snapshots missing")
      (PlanSite RecordRead,address:output:_) | address `shiftR` 17 `elem` [0x58,0x59] -> do
        ensure "custom read block differs" (address .&. 0x1ffff == 0)
        target <- case physical output of
          0x8a0080 -> Right PrimaryRecordReadBuffer
          0x8c0080 -> Right SecondaryRecordReadBuffer
          _ -> Left (PersistenceShape "custom read cache destination differs")
        selected <- bank ((address `shiftR` 17)-0x58)
        pure (Just (CustomRead selected target))
      (PlanSite RecordProgram,source:selected:offset:count:_) -> do
        ensure "custom program identity or size differs" (physical source == 0x89fa80 && count == 0x600 && offset `mod` 52 == 0 && offset `div` 52 < 127)
        selectedBank <- bank selected
        payload <- need "custom program snapshot missing" (case hitMemory hit of [bytes] | B.length bytes == 0x600 -> Just bytes; _ -> Nothing)
        pure (Just (CustomProgram selectedBank (ProgramOffset offset) (ProgramBytes payload)))
      (PlanSite RecordProgramResult,status:_) | persistenceStatusExecutes (hitAddress hit) (hitFlags hit) ->
        pure (Just (CustomProgramResult (MediumStatus (fromIntegral status))))
      (PlanSite RecordErase,selected:_) -> Just . CustomErase <$> bank selected
      (PlanSite RecordEraseResult,status:_) | persistenceStatusExecutes (hitAddress hit) (hitFlags hit) ->
        pure (Just (CustomEraseResult (MediumStatus (fromIntegral status))))
      _ -> Right Nothing

type Regions = M.Map Word32 B.ByteString

parseRegions :: Value -> Parse Regions
parseRegions value = do
  entries <- asArray value
  regions <- fmap M.fromList . forM entries $ \entry -> do
    fields <- asObject entry
    exact ["start", "bytes"] fields
    (,) <$> (at "start" fields >>= asWord) <*> (at "bytes" fields >>= asText >>= hexBytes)
  unless (M.map B.length regions
          == M.fromList [(rangeStart range, fromIntegral (rangeEnd range - rangeStart range)) | range <- settingsRanges])
    (Left "regions differ from the settings ranges")
  pure regions

-- | A CPU write's physical address, width, value (absent above eight bytes), preimage, byte order
-- and PC; or a host or model write's source, physical address and width.
data Row
  = CpuWriteRow Word32 Int (Maybe Word64) B.ByteString Bool Word32
  | TransferRow T.Text Word32 Int

-- | The bridge watch's decoder-view policy: reset segments observe decoder changes, step watches are fixed.
data ViewPolicy = ObserveViews | FixedViews deriving (Eq, Show, Enum, Bounded)

viewPolicyWire :: ViewPolicy -> T.Text
viewPolicyWire policy = case policy of
  ObserveViews -> "observe"
  FixedViews -> "fixed"

parseWatch :: ViewPolicy -> Value -> Parse [Row]
parseWatch policy value = do
  fields <- asObject value
  exact ["enabled", "ranges", "records", "violation", "classes", "views"] fields
  unless (KM.lookup "enabled" fields == Just (Bool True)) (Left "watch is not enabled")
  unless (KM.lookup "violation" fields == Just Null) (Left "watch recorded a violation")
  viewText <- at "views" fields >>= asText
  views <- maybe (Left "unknown watch views") Right (lookup viewText [(viewPolicyWire known, known) | known <- [minBound .. maxBound]])
  unless (views == policy) (Left "watch views differ from the expected policy")
  ranges <- at "ranges" fields >>= asArray >>= mapM (\pair -> asArray pair >>= mapM asWord)
  unless (ranges == [[rangeStart range, rangeEnd range] | range <- settingsRanges]) (Left "watch ranges differ from the settings ranges")
  rows <- at "records" fields >>= asArray
  when (length rows > watchLimit) (Left "watch rows exceed the declared bound")
  mapM parseRow rows
 where
  parseRow row = do
    fields <- asObject row
    source <- at "source" fields >>= asText
    physical <- at "physical" fields >>= asWord
    width <- at "size" fields >>= asInt
    unless (width >= 1) (Left "row width is not positive")
    case source of
      "cpu-write" -> do
        exact ["source", "address", "physical", "size", "value", "before", "byte_order", "core", "pc", "instructions", "tick"] fields
        before <- at "before" fields >>= asArray >>= mapM (\byte -> asInt byte >>= \n -> if n >= 0 && n <= 255 then Right (fromIntegral n) else Left "byte out of range")
        unless (length before == width && width <= 16) (Left "cpu-write before bytes differ from its width")
        value' <- case KM.lookup "value" fields of
          Just Null | width > 8 -> Right Nothing
          Just number | width <= 8 -> Just <$> asWord64 number
          _ -> Left "cpu-write value is absent or wider than its width allows"
        order <- at "byte_order" fields >>= asText
        little <- case order of
          "little" -> Right True
          "big" -> Right False
          _ -> Left "unknown byte order"
        pc <- at "pc" fields >>= asWord
        address <- at "address" fields >>= asWord
        unless (address == physical || address == physical + 0x40000000) (Left "cpu-write address is not the physical address or its bit-30 alias")
        pure (CpuWriteRow physical width value' (B.pack before) little pc)
      _ | source `elem` ["host-write", "model-bus-write"] -> pure (TransferRow source physical width)
      _ -> Left ("unexpected watch row source " ++ T.unpack source)

-- Interval replay.

-- | A reset segment or one step; a step carries its press.
data Interval = Interval { intervalName :: IntervalName, intervalBefore :: Regions, intervalAfter :: Regions, intervalRows :: [Row], intervalHits :: (Int, Int), intervalPress :: Maybe Press }

regionByte :: Regions -> Word32 -> Maybe Word8
regionByte regions address = case M.lookupLE address regions of
  Just (start, bytes) | address - start < fromIntegral (B.length bytes) -> Just (B.index bytes (fromIntegral (address - start)))
  _ -> Nothing

valueBytes :: Bool -> Int -> Word64 -> [Word8]
valueBytes little width value = (if little then id else reverse) [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. width - 1]]

-- | Replays one interval's rows from its before regions to its after regions, giving each write's
-- bytes; bytes a wide write or a transfer left unknown are read from the next row's preimage or
-- the after regions.
replay :: Plan -> Interval -> Either Refusal ([WriteEffect], [TransferEffect])
replay plan interval = do
  let start = M.fromList [(address, Just byte) | (base, bytes) <- M.toList (intervalBefore interval), (address, byte) <- zip [base ..] (B.unpack bytes)]
  (state, pending, resolved) <- foldM step (start, M.empty, M.empty) (zip [0 :: Int ..] (intervalRows interval))
  let finalCheck resolved' (address, current) = do
        after <- maybe (Left (ReplayMismatch (intervalName interval) address)) Right (regionByte (intervalAfter interval) address)
        case current of
          Just byte | byte /= after -> Left (ReplayMismatch (intervalName interval) address)
          Just _ -> Right resolved'
          Nothing -> Right (maybe resolved' (\owner -> M.insert (owner, address) after resolved') (M.lookup address pending))
  resolvedAll <- foldM finalCheck resolved (M.toList state)
  let effects = [ WriteEffect address width bytes (B.unpack before) (classifyWriter plan pc) pc
                | (index, CpuWriteRow address width value' before little pc) <- zip [0 :: Int ..] (intervalRows interval)
                , let bytes = case value' of
                        Just number -> map Just (valueBytes little width number)
                        Nothing -> [M.lookup (index, address + fromIntegral offset) resolvedAll | offset <- [0 .. width - 1]] ]
      transfers = [TransferEffect source address width | TransferRow source address width <- intervalRows interval]
  pure (effects, transfers)
 where
  inRegions = (/= Nothing) . regionByte (intervalBefore interval)
  step (state, pending, resolved) (index, row) = case row of
    CpuWriteRow address width value' before little _ -> do
      let addresses = [address + fromIntegral offset | offset <- [0 .. width - 1]]
          newBytes = maybe (replicate width Nothing) (map Just . valueBytes little width) value'
      (pending', resolved') <- foldM (\(p, r) (offset, target) ->
          if not (inRegions target) then Right (p, r) else
            let preimage = B.index before offset in
            case M.findWithDefault Nothing target state of
              Just current | current /= preimage -> Left (ReplayMismatch (intervalName interval) target)
              Just _ -> Right (M.delete target p, r)
              Nothing -> Right (M.delete target p, maybe r (\owner -> M.insert (owner, target) preimage r) (M.lookup target p)))
        (pending, resolved) (zip [0 ..] addresses)
      let tracked = [(target, byte) | (target, byte) <- zip addresses newBytes, inRegions target]
          state' = foldl (\s (target, byte) -> M.insert target byte s) state tracked
          pending'' = foldl (\p (target, byte) -> if isNothing byte then M.insert target index p else p) pending' tracked
      Right (state', pending'', resolved')
    TransferRow _ address width -> do
      let tracked = filter inRegions [address + fromIntegral offset | offset <- [0 .. width - 1]]
      Right (foldl (\s target -> M.insert target Nothing s) state tracked, foldl (flip M.delete) pending tracked, resolved)

-- Caller-buffer destinations can lie outside the settings watch.
checkStores :: Plan -> V.Vector Hit -> Interval -> Either Refusal ()
checkStores plan hits interval = do
  let (from, to) = intervalHits interval
      stores = [(index, accessor, hit) | index <- [from .. to - 1], let hit = hits V.! index, PlanSite (AccessorStore accessor) <- [hitSite hit]]
      physical address = address .&. complement 0x40000000
  expected <- forM stores $ \(index, accessor, hit) -> do
    (address, width, value) <- maybe (Left (AccessorStoreBound index)) Right (storeEffect accessor (hitArguments hit))
    pure (physical (hitAddress hit), physical address, width, Just (fromIntegral value .&. ((1 `shiftL` (8 * width)) - 1)))
  let rows = [ (physical pc, address, width, value')
             | CpuWriteRow address width value' _ _ pc <- intervalRows interval, M.member (physical pc) (planStores plan) ]
  let watched (_,address,width,_) = any (\range -> address < rangeEnd range && toInteger address+toInteger width > toInteger (rangeStart range)) settingsRanges
  unless (filter watched expected == rows) (Left (AccessorStoreUnmatched (intervalName interval)))

-- Call pairing.

checkCaptures :: Plan -> V.Vector Hit -> Either Refusal ()
checkCaptures plan = V.imapM_ visit
  where
    general = any (`elem` M.elems (planSites plan)) [StorageSite Storage.EeCompare,StorageSite Storage.S1ActionEntry]
    tasks = StorageSite Storage.BankApplyEntry `elem` M.elems (planSites plan)
    visit index hit = do
      unless (isJust (hitR8 hit) == general) (Left (HitShape index "register capture differs from the admitted plan"))
      unless (isJust (hitTask hit) == tasks) (Left (HitShape index "task capture differs from the admitted plan"))

-- | A paired call's return hit and r0 there.
data Returned = Returned { returnHit :: Int, returnValue :: Word32 }

data ApplyFrame = ApplyFrame { applyEntryHit :: Hit, applySetters :: [Int] }
type ApplyFrameKey = (TraceCore, Maybe LogicalTaskId)
data PairState = PairState
  { pairOpen :: M.Map (TraceCore, Maybe LogicalTaskId, Word32, Word32) (SiteLabel, Int, [Word32])
  , pairDone :: M.Map Int Returned
  , pairApplyFrames :: M.Map ApplyFrameKey [ApplyFrame]
  , pairApplyDone :: M.Map Int CallerContext
  }

emptyPairState :: PairState
emptyPairState = PairState M.empty M.empty M.empty M.empty

hitTaskId :: Hit -> Maybe LogicalTaskId
hitTaskId hit = case hitTask hit of
  Just (AvailableTask context) -> Just (taskLogicalId context)
  _ -> Nothing

physicalAddress :: Word32 -> Word32
physicalAddress address = address .&. complement 0x40000000

executionTask :: Hit -> Either BankApplyFault TaskContext
executionTask hit = case hitTask hit of
  Just (AvailableTask context)
    | taskContextDepth context == 0 && hitFlags hit .&. 0x0100023f == 0x13
      && hitSp hit `mod` 4 == 0 && stackContains (taskStack context) (hitSp hit) -> Right context
    | otherwise -> Left ApplyNotTaskExecution
  _ -> Left ApplyTaskUnavailable
  where
    stackContains (TaskStack (GuestAddress base) (StackBytes bytes)) sp =
      physicalAddress sp >= physicalAddress base
        && toInteger (physicalAddress sp) <= toInteger (physicalAddress base) + toInteger bytes

sameTask :: TaskContext -> TaskContext -> Bool
sameTask first second = taskKernel first == taskKernel second
  && taskLogicalId first == taskLogicalId second
  && physicalGuest (taskNode first) == physicalGuest (taskNode second)
  && physicalGuest (taskCreation first) == physicalGuest (taskCreation second)
  && sameStack (taskStack first) (taskStack second)
  where
    physicalGuest (GuestAddress address) = physicalAddress address
    sameStack (TaskStack base bytes) (TaskStack otherBase otherBytes) =
      physicalGuest base == physicalGuest otherBase && bytes == otherBytes

bankCaller :: Maybe StorageBoundaries.BankApplyBoundary -> Role -> Hit -> Either BankApplyFault Bool
bankCaller Nothing _ _ = Right False
bankCaller (Just boundary) role hit
  | caller > 0x6309c4 && caller <= 0x630a34 =
      if role == RetainedRole && caller == 0x630a04 then Right True
      else if role == RetainedRole && caller == 0x630a2c then Right False
      else Left ApplyCallerInvalid
  | caller > entry && caller <= end =
      if role /= RetainedRole && StorageBoundaries.bankApplyCallReturn boundary (GuestAddress caller)
        then Right True else Left ApplyCallerInvalid
  | otherwise = Right False
  where
    caller = physicalAddress (hitLr hit)
    GuestAddress entry = StorageBoundaries.bankApplyEntry boundary
    GuestAddress end = StorageBoundaries.bankApplyEnd boundary

pairApply :: Plan -> Maybe StorageBoundaries.BankApplyBoundary -> Int -> Hit -> PairState -> Either Refusal PairState
pairApply plan boundary index hit state = case hitSite hit of
  PlanSite (StorageSite Storage.BankApplyEntry) ->
    pure state {pairApplyFrames=M.insertWith (++) key [ApplyFrame hit []] (pairApplyFrames state)}
  PlanSite SetterEntry -> do
    isBank <- fault (bankCaller boundary (planRole plan) hit)
    if not isBank then pure state else do
      current <- fault (executionTask hit)
      frames <- maybe (Left (BankApplyContextRefused index ApplyEntryMissing)) Right (M.lookup key (pairApplyFrames state))
      case frames of
        frame:rest -> do
          entered <- fault (executionTask (applyEntryHit frame))
          unless (sameTask entered current) (Left (BankApplyContextRefused index ApplyNotTaskExecution))
          pure state {pairApplyFrames=M.insert key (frame{applySetters=index:applySetters frame}:rest) (pairApplyFrames state)}
        [] -> Left (BankApplyContextRefused index ApplyEntryMissing)
  PlanSite (StorageSite Storage.BankApplyReturn) -> do
    let matches frame = hitCore (applyEntryHit frame) == hitCore hit
          && hitSp (applyEntryHit frame) == hitSp hit
          && physicalAddress (hitLr (applyEntryHit frame)) == physicalAddress (hitAddress hit)
        candidates = case M.lookup key (pairApplyFrames state) of
          Just (frame:rest) -> Just (key,frame,rest)
          _ -> find (\(_,frame,_) -> null (applySetters frame) && matches frame)
            [(candidate,frame,rest) | (candidate,frame:rest) <- M.toList (pairApplyFrames state)]
    (owner,frame,rest) <- maybe (Left (BankApplyContextRefused index ApplyReturnUnexpected)) Right candidates
    unless (matches frame) (Left (BankApplyContextRefused index ApplyReturnUnexpected))
    done <- case applySetters frame of
      [] -> pure (pairApplyDone state)
      setters -> do
        entered <- fault (executionTask (applyEntryHit frame))
        returned <- fault (executionTask hit)
        unless (sameTask entered returned) (Left (BankApplyContextRefused index ApplyNotTaskExecution))
        unless (all (`M.member` pairDone state) setters) (Left (BankApplyContextRefused index ApplyReturnMissing))
        let context = BankApplyCaller (taskKernel entered) (taskLogicalId entered)
        pure (foldr (\setter -> M.insert setter context) (pairApplyDone state) setters)
    pure state {pairApplyFrames=if null rest then M.delete owner (pairApplyFrames state) else M.insert owner rest (pairApplyFrames state),pairApplyDone=done}
  _ -> pure state
  where
    key = (hitCore hit,hitTaskId hit)
    fault = either (Left . BankApplyContextRefused index) Right

callsIn :: Plan -> Maybe StorageBoundaries.BankApplyBoundary -> V.Vector Hit -> PairState -> [Int] -> SiteLabel -> Either Refusal [Call]
callsIn plan boundary hits state indices label = forM [index | index <- indices, hitSite (hits V.! index) == PlanSite label] $ \index -> do
  let hit = hits V.! index
  isBank <- if label == SetterEntry
    then either (Left . BankApplyContextRefused index) Right (bankCaller boundary (planRole plan) hit)
    else pure False
  context <- if isBank then maybe (Left (BankApplyContextRefused index ApplyReturnMissing)) Right (M.lookup index (pairApplyDone state)) else pure ExactCaller
  pure (Call (hitArguments hit) (hitLr hit) (returnValue <$> M.lookup index (pairDone state)) context)

data HitClass = EntryHit SiteLabel | ReturnHit SiteLabel | GuardHit | OtherHit

classifyHit :: Plan -> Int -> Hit -> Either Refusal HitClass
classifyHit plan index hit = case hitSite hit of
  PlanSite label | M.lookup (hitAddress hit) (planSites plan) == Just label -> Right $ case label of
    GetterEntry -> EntryHit GetterReturn
    SetterEntry -> EntryHit SetterReturn
    GetterReturn -> ReturnHit GetterReturn
    SetterReturn -> ReturnHit SetterReturn
    Guard _ -> GuardHit
    _ -> OtherHit
  BridgeSite site | hitAddress hit == bridgeSiteAddress site -> Right OtherHit
  _ -> Left (UnarmedSite index)

pairHits :: Plan -> Maybe StorageBoundaries.BankApplyBoundary -> V.Vector Hit -> (Int, Int) -> PairState -> Either Refusal PairState
pairHits plan boundary hits (from, to) initial = foldM visit initial [from .. to - 1]
 where
  visit state index = do
    let hit = hits V.! index
    kind <- classifyHit plan index hit
    applied <- pairApply plan boundary index hit state
    case kind of
      GuardHit -> Left (GuardedFetch index)
      EntryHit returnLabel -> do
        unless (M.lookup (hitLr hit) (planSites plan) == Just returnLabel) (Left (UnarmedReturn index))
        let key = (hitCore hit,hitTaskId hit,hitSp hit,hitLr hit)
        when (M.member key (pairOpen applied)) (Left (DuplicateOpenCall index))
        pure applied { pairOpen = M.insert key (returnLabel, index, hitArguments hit) (pairOpen applied) }
      ReturnHit label -> case M.lookup (hitCore hit,hitTaskId hit,hitSp hit,hitAddress hit) (pairOpen applied) of
        Just (expected, entry, _) | expected == label -> do
          isBank <- either (Left . BankApplyContextRefused index) Right (bankCaller boundary (planRole plan) (hits V.! entry))
          when isBank $ do
            entered <- either (Left . BankApplyContextRefused index) Right (executionTask (hits V.! entry))
            returned <- either (Left . BankApplyContextRefused index) Right (executionTask hit)
            unless (sameTask entered returned) (Left (BankApplyContextRefused index ApplyNotTaskExecution))
          pure applied { pairOpen = M.delete (hitCore hit,hitTaskId hit,hitSp hit,hitAddress hit) (pairOpen applied)
                       , pairDone = M.insert entry (Returned index (sum (take 1 (hitArguments hit)))) (pairDone applied) }
        _ -> pure applied
      OtherHit -> pure applied

-- Qualification.

field :: T.Text -> Value -> Parse Value
field name value = asObject value >>= at name

digestAt :: [T.Text] -> Value -> Parse String
digestAt path value = T.unpack <$> (foldM (flip field) value path >>= asText)

digestOf :: B.ByteString -> String
digestOf = sha256Hex . sha256

decodeValue :: B.ByteString -> Parse Value
decodeValue = A.eitherDecodeStrict'

digestIdentity :: String -> String -> Either Refusal Sha256
digestIdentity name text = either (const (Left (InputDigest name))) Right (parseSha256Hex text)

receiptDigestSha256 :: Refusal -> [T.Text] -> Value -> Either Refusal Sha256
receiptDigestSha256 refusal path receipt = do
  recorded <- either (Left . ReceiptShape) Right (digestAt path receipt)
  either (const (Left refusal)) Right (parseSha256Hex recorded)

admitBindElf :: Bindings -> Contract -> Maybe Sha256 -> Either Refusal (Maybe StorageBoundaries.BankApplyBoundary)
admitBindElf bindings contract elfDigest = case (contractPublishedElf contract,elfDigest,bindElf bindings,contractPublication contract) of
  (Nothing,Nothing,Nothing,Nothing) -> pure Nothing
  (Just identity,Just digest,Just bytes,Just publication) -> do
    unless (sha256Hex digest == publishedElfSha256 identity) (Left (InputDigest "ELF"))
    boundary <- either (Left . InputDigest . ("bank apply ELF: " ++)) Right
      (StorageBoundaries.admitBankApplyElf StorageBoundaries.BankApplyElfInput
        { StorageBoundaries.bankApplyImageBase = GuestAddress 0x61c400
        , StorageBoundaries.bankApplyCarriers = [Carrier (GuestAddress low) (GuestAddress high) | PhysicalRange low high <- publicationCarriers publication]
        , StorageBoundaries.bankApplyLinkedBytes = bindLinked bindings
        , StorageBoundaries.bankApplySymbolBytes = bindSymbols bindings
        , StorageBoundaries.bankApplyElfBytes = bytes
        })
    unless ([GuestAddress address | ("xm1_storage_apply",address) <- contractEntries contract]
      == [StorageBoundaries.bankApplyEntry boundary]) (Left (InputDigest "bank apply ELF entry"))
    pure (Just boundary)
  _ -> Left (InputDigest "ELF input and publication differ")

data Workload = RouteWorkload RouteFile | StorageWorkload Storage.StorageMatrix

decodeWorkload :: B.ByteString -> Parse Workload
decodeWorkload bytes = do
  fields <- decodeValue bytes >>= asObject
  case KM.lookup "kind" fields of
    Nothing -> RouteWorkload <$> decodeRouteFile bytes
    Just (String "settings-storage-lifecycle") -> StorageWorkload <$> Storage.decodeStorageMatrix bytes
    _ -> Left "unknown property pair workload kind"

deriveRolePlan :: Bindings -> Contract -> Workload -> Maybe StorageBoundaries.BankApplyBoundary -> Role -> Either Refusal RolePlan
deriveRolePlan bindings contract workload boundary role = do
  let inputs = PlanInputs role contract (bindImages bindings) (bindLinked bindings) Nothing boundary
  plan <- either (Left . InputDigest) Right (case workload of
    RouteWorkload _ -> derivePlan inputs
    StorageWorkload matrix -> deriveStoragePlan inputs matrix)
  image4 <- either (Left . InputDigest) Right (roleImage4 inputs)
  let spec = specValue plan
  pure (RolePlan plan spec (BL.toStrict (A.encode spec)) (sha256 image4))

admitBindings :: Bindings -> Either Refusal AdmittedBindings
admitBindings bindings = do
  contract <- either (Left . InputDigest . ("contract: " ++)) Right (decodeContract (bindContract bindings))
  sourceDat <- digestIdentity "source DAT" (bindSourceDat bindings)
  ownedDat <- digestIdentity "owned DAT" (bindOwnedDat bindings)
  observer <- digestIdentity "observer" (bindObserver bindings)
  contractSource <- digestIdentity "source DAT" (contractSourceDat contract)
  contractOwned <- digestIdentity "owned DAT" (contractCandidateDat contract)
  contractLinked <- digestIdentity "linked" (contractLinkedSha256 contract)
  contractSymbols <- digestIdentity "symbols" (contractSymbolsSha256 contract)
  let linkedDigest = sha256 (bindLinked bindings)
      symbolsDigest = sha256 (bindSymbols bindings)
      routesDigest = sha256 (bindRoutes bindings)
      contractDigest = sha256 (bindContract bindings)
      imageDigests = M.map sha256 (bindImages bindings)
      elfDigest = sha256 <$> bindElf bindings
      require refusal condition = unless condition (Left refusal)
  require (InputDigest "source DAT") (sourceDat == contractSource)
  require (InputDigest "owned DAT") (ownedDat == contractOwned)
  require (InputDigest "linked") (linkedDigest == contractLinked)
  require (InputDigest "symbols") (symbolsDigest == contractSymbols)
  workload <- either (Left . InputDigest . ("workload: " ++)) Right (decodeWorkload (bindRoutes bindings))
  boundary <- admitBindElf bindings contract elfDigest
  retainedPlan <- deriveRolePlan bindings contract workload boundary RetainedRole
  ownedPlan <- deriveRolePlan bindings contract workload boundary OwnedRole
  pure AdmittedBindings
    { admittedBindings = bindings
    , admittedContract = contract
    , admittedWorkload = workload
    , admittedSourceDat = sourceDat
    , admittedOwnedDat = ownedDat
    , admittedObserver = observer
    , admittedRoutesDigest = routesDigest
    , admittedContractDigest = contractDigest
    , admittedLinkedDigest = linkedDigest
    , admittedSymbolsDigest = symbolsDigest
    , admittedImageDigests = imageDigests
    , admittedElfDigest = elfDigest
    , admittedBoundary = boundary
    , admittedRetainedPlan = retainedPlan
    , admittedOwnedPlan = ownedPlan
    }

specFor :: AdmittedBindings -> Role -> Either Refusal B.ByteString
specFor admitted role = rolePlanSpecWire <$> rolePlanFor admitted role

rolePlanFor :: AdmittedBindings -> Role -> Either Refusal RolePlan
rolePlanFor admitted role = case role of
  RetainedRole -> Right (admittedRetainedPlan admitted)
  OwnedRole -> Right (admittedOwnedPlan admitted)
  ControlRole -> Left (InputDigest "role")

specMatches :: RolePlan -> B.ByteString -> Either Refusal ()
specMatches rolePlan bytes
  | bytes == rolePlanSpecWire rolePlan || B.stripSuffix "\n" bytes == Just (rolePlanSpecWire rolePlan) = Right ()
  | otherwise = do
      spec <- either (Left . ReceiptShape . ("spec: " ++)) Right (decodeValue bytes)
      unless (spec == rolePlanSpec rolePlan) (Left SiteMapDiffers)

qualify :: AdmittedBindings -> Role -> Observation -> Either Refusal Qualified
qualify admitted role observation = do
  let bindings = admittedBindings admitted
      contract = admittedContract admitted
      workload = admittedWorkload admitted
      require refusal condition = unless condition (Left refusal)
  receipt <- either (Left . ReceiptShape) Right (decodeValue (obsReceipt observation))
  require (ReceiptShape "role") (either (const False) (== String (T.pack (roleWire role))) (field "role" receipt))
  require (InputDigest "role") (role /= ControlRole)
  rolePlan <- rolePlanFor admitted role
  datDigest <- receiptDigestSha256 (InputDigest "DAT") ["inputs", "dat", "sha256"] receipt
  require (InputDigest "DAT") (datDigest == (if role == RetainedRole then admittedSourceDat admitted else admittedOwnedDat admitted))
  forM_ [ (["inputs", "routes", "sha256"], admittedRoutesDigest admitted, "routes")
        , (["inputs", "contract", "sha256"], admittedContractDigest admitted, "contract")
        , (["inputs", "linked", "sha256"], admittedLinkedDigest admitted, "linked")
        , (["inputs", "symbols", "sha256"], admittedSymbolsDigest admitted, "symbols")
        , (["inputs", "observer", "sha256"], admittedObserver admitted, "observer") ] $ \(path, expected, name) -> do
    recorded <- receiptDigestSha256 (InputDigest name) path receipt
    require (InputDigest name) (recorded == expected)
  forM_ (M.toList (admittedImageDigests admitted)) $ \(name, digest) -> do
    let refusal = InputDigest (T.unpack (imageNameWire name))
    recorded <- receiptDigestSha256 refusal ["inputs", "analysis", imageNameWire name] receipt
    require refusal (recorded == digest)
  forM_ [ ("spec.json", obsSpec observation), ("reset-observation.json", obsReset observation)
        , ("hits.jsonl", obsHits observation), ("steps.json", obsSteps observation)
        , ("emulator-session.json", obsSession observation) ] $ \(name, bytes) -> do
    let refusal = SidecarDigest (T.unpack name)
    recorded <- receiptDigestSha256 refusal ["files", name] receipt
    require refusal (recorded == sha256 bytes)
  case bindElf bindings of
    Nothing -> do
      inputs <- either (Left . ReceiptShape) Right (field "inputs" receipt >>= asObject)
      unless (not (KM.member "elf" inputs)) (Left (InputDigest "unexpected ELF input"))
    Just _ -> do
      recorded <- receiptDigestSha256 (InputDigest "ELF") ["inputs", "elf", "sha256"] receipt
      case admittedElfDigest admitted of
        Just digest -> require (InputDigest "ELF") (recorded == digest)
        Nothing -> Left (InputDigest "ELF")
  outcome <- either (Left . ReceiptShape) Right (field "outcome" receipt)
  require (ObservationRefused outcome) (either (const False) (== String "completed") (field "status" outcome))
  bounds <- either (Left . ReceiptShape) Right (field "bounds" receipt)
  require (ReceiptShape "bounds") (bounds == object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit])
  require (ReceiptShape "probe") (either (const False) (== Null) (field "probe" receipt))
  require (ReceiptShape "pre-key idle") (either (const False) (== A.toJSON (instructionsValue preKeyIdle)) (field "pre_key_idle_instructions" receipt))
  require (ReceiptShape "post-settle prefix ticks") (either (const False) (== A.toJSON (tickValue postSettle)) (field "post_settle_prefix_ticks" receipt))
  recordedImage <- receiptDigestSha256 (InputDigest "role image4") ["inputs", "role_image4_sha256"] receipt
  require (InputDigest "role image4") (recordedImage == rolePlanImage4Digest rolePlan)
  specMatches rolePlan (obsSpec observation)
  let plan = rolePlanPlan rolePlan
      bankBoundary = planBankBoundary plan
  hits <- fmap V.fromList . forM (zip [0 ..] (filter (not . B.null) (C.lines (obsHits observation)))) $ \(index, line) ->
    either (Left . HitShape index) Right (decodeValue line >>= parseHit)
  require (HitBound (V.length hits)) (V.length hits <= hitLimit)
  checkCaptures plan hits
  case workload of
    RouteWorkload routes -> qualifyRoutes role observation receipt contract plan bankBoundary hits routes
    StorageWorkload matrix -> qualifyStorage role observation receipt contract plan bankBoundary hits matrix

qualifyRoutes :: Role -> Observation -> Value -> Contract -> Plan -> Maybe StorageBoundaries.BankApplyBoundary -> V.Vector Hit -> RouteFile -> Either Refusal Qualified
qualifyRoutes role observation receipt contract plan bankBoundary hits routes = do
  let receiptDigest path = either (Left . ReceiptShape) Right (digestAt path receipt)
      require refusal condition = unless condition (Left refusal)
  reset <- either (Left . WatchShape "reset") Right (decodeValue (obsReset observation))
  phases <- either (Left . WatchShape "reset") Right (phaseIntervals reset)
  steps <- either (Left . WatchShape "steps") Right (decodeValue (obsSteps observation) >>= asArray)
  stepIntervals <- forM (zip [1 ..] steps) $ \(expected, step) -> either (Left . StepShape expected) Right (parseStep expected step)
  let lastPhase = snd (last phases)
      stepMap = M.fromList [(keys, (parent, interval)) | (keys, parent, interval) <- stepIntervals]
      ordered = map snd phases ++ [interval | (_, _, interval) <- stepIntervals]
  zipWithM_ (\previous next -> require (HitPartition (intervalName next)) (snd (intervalHits previous) == fst (intervalHits next)))
    ordered (drop 1 ordered)
  forM_ (take 1 ordered) $ \first -> require (HitPartition (intervalName first)) (fst (intervalHits first) == 0)
  require (HitPartition (intervalName (last ordered))) (snd (intervalHits (last ordered)) == V.length hits)
  zipWithM_ (\previous next -> require (RegionContinuity (intervalName next)) (intervalAfter previous == intervalBefore next))
    (map snd phases) (drop 1 (map snd phases))
  forM_ stepIntervals $ \(keys, parent, interval) -> do
    let parentAfter = if parent == 0 then intervalAfter lastPhase else maybe M.empty (intervalAfter . snd) (M.lookup (init keys) stepMap)
    require (RegionContinuity (intervalName interval)) (intervalBefore interval == parentAfter
      && (parent == 0) == (length keys == 1) && (parent == 0 || maybe False ((== StepInterval parent) . intervalName . snd) (M.lookup (init keys) stepMap)))
  replayed <- fmap M.fromList . forM ordered $ \interval -> (,) (intervalName interval) <$> replay plan interval
  forM_ ordered (checkStores plan hits)
  let pairInterval state interval = pairHits plan bankBoundary hits (intervalHits interval) state
      window interval = let (from, to) = intervalHits interval in [from .. to - 1]
  (phaseState, phaseStates) <- foldM (\(state, acc) (_, interval) -> do
      next <- pairInterval state interval
      pure (next, M.insert (intervalName interval) next acc)) (emptyPairState, M.empty) phases
  -- A step's pairing continues from its parent's, as its snapshot restore does.
  pairStates <- foldM (\acc (_, parent, interval) -> do
      next <- pairInterval (if parent == 0 then phaseState else acc M.! StepInterval parent) interval
      pure (M.insert (intervalName interval) next acc)) phaseStates stepIntervals
  let returnedIn interval = case intervalName interval of
        PhaseInterval _ -> phaseState
        name -> pairStates M.! name
      getters = [ GetterResult (GetterKey (hitLr hit) (hitArguments hit)) (returnValue returned) (returnHit returned)
                | interval <- ordered, index <- window interval, let hit = hits V.! index, hitSite hit == PlanSite GetterEntry
                , Just returned <- [M.lookup index (pairDone (returnedIn interval))] ]
  results <- forM (routeList routes) $ \route -> do
    let name = routeName route
    own <- case routeBody route of
      PhaseRoute phase -> pure [snd (phases !! fromEnum phase)]
      KeyRoute keys _ -> forM (drop 1 (scanl (\acc key -> acc ++ [key]) [] keys)) $ \prefixKeys ->
        maybe (Left (RouteUnobserved name)) (Right . snd) (M.lookup prefixKeys stepMap)
    let final = pairStates M.! intervalName (last own)
        routeWindow = concatMap window own
        lastSites = [hitSite (hits V.! index) | index <- window (last own)]
        lastEffects = concat [ case site of
                                 PlanSite SetterEntry -> map SetterEffect (take 1 (hitArguments hit))
                                 PlanSite PropertyCommit -> [CommitEffect]
                                 _ -> []
                             | index <- window (last own), let hit = hits V.! index, let site = hitSite hit ]
        effects = [replayed M.! intervalName interval | interval <- own]
        writes = concatMap fst effects
    getterCalls <- callsIn plan bankBoundary hits final routeWindow GetterEntry
    setterCalls <- callsIn plan bankBoundary hits final routeWindow SetterEntry
    persistence <- normalizePersistence contract [hits V.! index | index <- routeWindow]
    routeClass <- case routeBody route of
      PhaseRoute _ -> pure PhaseClass
      KeyRoute _ (CrawlTrial _ expected) -> do
        require (RouteEffectsDiffer name) (map effectItemShape (collapseEffects lastEffects) == map effectItemShape (collapseEffects expected))
        pure CrawlClass
      KeyRoute _ (Declared _) -> do
        require (DeclaredRouteInert name) (PlanSite SetterEntry `elem` lastSites && PlanSite Area3Append `elem` lastSites)
        pure DeclaredClass
    forM_ writes $ \write -> require (ForeignWriter name (effectPc write)) (effectWriter write /= CarrierBody)
    pure RouteResult
      { resultRoute = name
      , resultClass = routeClass
      , resultGetters = getterCalls
      , resultSetters = setterCalls
      , resultPersistence = persistence
      , resultWrites = writes
      , resultTransfers = concatMap snd effects
      , resultFinal = M.toList (intervalAfter (last own))
      , resultPresses = [press | Just press <- map intervalPress own]
      , resultEffectBlocks = effectBlockRuns lastEffects
      }
  cohort <- forM [["inputs", "run", "sha256"], ["inputs", "boot_rom", "sha256"], ["inputs", "adjustment_nand", "sha256"], ["inputs", "bridge", "sha256"]
                 , ["inputs", "routes", "sha256"], ["inputs", "linked", "sha256"], ["inputs", "contract", "sha256"], ["inputs", "observer", "sha256"]] $ \path ->
    (,) (T.unpack (T.intercalate "." path)) <$> receiptDigest path
  pure (Qualified role cohort (V.length hits) (length steps) results getters Nothing)

phaseIntervals :: Value -> Parse [(AttachPhase, Interval)]
phaseIntervals reset = do
  fields <- asObject reset
  exact ["schema", "reset", "regions_at_reset", "segments"] fields
  resetPoint <- at "reset" fields >>= asObject
  unless (KM.lookup "instructions" resetPoint == Just (Number 0)) (Left "reset point is not at instruction 0")
  initial <- at "regions_at_reset" fields >>= parseRegions
  segments <- at "segments" fields >>= asArray
  unless (length segments == 3) (Left "reset observation does not have three segments")
  (_, _, intervals) <- foldM (\(before, hitsStart, acc) (phase, segment) -> do
      segmentFields <- asObject segment
      exact ["phase", "instructions", "hits_end", "watch", "regions"] segmentFields
      recorded <- at "phase" segmentFields >>= asText
      unless (recorded == attachPhaseWire phase) (Left "reset segments are out of phase order")
      hitsEnd <- at "hits_end" segmentFields >>= asInt
      rows <- at "watch" segmentFields >>= parseWatch ObserveViews
      after <- at "regions" segmentFields >>= parseRegions
      let interval = Interval (PhaseInterval phase) before after rows (hitsStart, hitsEnd) Nothing
      pure (after, hitsEnd, acc ++ [(phase, interval)]))
    (initial, 0, []) (zip [minBound .. maxBound] segments)
  pure intervals

parseStep :: Int -> Value -> Parse ([CrawlKey], Int, Interval)
parseStep expected value = do
  fields <- asObject value
  exact ["step", "parent", "keys", "hits", "idle", "press", "bounded_post_settle_prefix", "watch", "regions_before", "regions_after"] fields
  number <- at "step" fields >>= asInt
  unless (number == expected) (Left "steps are out of order")
  parent <- at "parent" fields >>= asInt
  unless (parent >= 0 && parent < number) (Left "step parent does not precede it")
  keys <- at "keys" fields >>= parseEither A.parseJSON
  unless (not (null keys)) (Left "step presses no key")
  hits <- at "hits" fields >>= asArray >>= mapM asInt
  range <- case hits of
    [from, to] | from <= to -> Right (from, to)
    _ -> Left "step hit range is malformed"
  ran <- at "idle" fields >>= \idle -> field "ran" idle >>= asInt
  unless (toInteger ran == toInteger (instructionsValue preKeyIdle)) (Left "step did not run the pre-key idle")
  press <- at "press" fields
  unless (either (const False) (== Null) (field "failure" press)) (Left "step press failed")
  statusText <- field "settle" press >>= field "status" >>= asText
  status <- maybe (Left "unknown settle status") Right (lookup statusText [(settleStatusWire known, known) | known <- [minBound .. maxBound]])
  unless (status /= PriorRenderPending) (Left "step key was never pressed: prior render pending")
  endpoint <- at "bounded_post_settle_prefix" fields >>= parsePostSettlePrefix
  endpointMenu <- field "ui" endpoint >>= parseLocalUiMenu
  endpointMode <- field "source_mode" endpoint >>= parseLocalUiMode
  menu <- field "menu" press
  rows <- at "watch" fields >>= parseWatch FixedViews
  before <- at "regions_before" fields >>= parseRegions
  after <- at "regions_after" fields >>= parseRegions
  pure (keys, parent, Interval (StepInterval number) before after rows range (Just (Press status menu endpointMenu endpointMode)))

parsePostSettlePrefix :: Value -> Parse Value
parsePostSettlePrefix prefix = do
  elapsed <- field "elapsed" prefix
  ticks <- field "ticks" elapsed >>= asWord64
  unless (ticks == tickValue postSettle) (Left "post-settle prefix differs from its exact tick boundary")
  instructions <- field "instructions" elapsed >>= asWord64
  ran <- field "ran" elapsed >>= asWord64
  unless (ran <= instructions) (Left "post-settle prefix instruction range is invalid")
  state <- field "state" prefix
  snapshotInstructions <- field "instructions" state >>= asWord64
  _ <- field "tick" state >>= asWord64
  unless (snapshotInstructions == instructions) (Left "UI snapshot advanced beyond the prefix boundary")
  pure state

-- Comparison.

-- | Per route: the ordered setter calls with each run of identical time-driven blocks collapsed to
-- one block, area-3 appends, byte-changing writes and transfers, the final regions and each press's
-- menu must be equal; settle statuses only where neither press exhausted its budget. Over the
-- whole observation: the getter keys must be equal and so must each key's collapsed results.
compareRoles :: Qualified -> Qualified -> Either Refusal [(RouteResult, RouteResult)]
compareRoles retained owned = do
  unless (qualifiedRole retained == RetainedRole && qualifiedRole owned == OwnedRole) (Left (CohortDiffers "roles"))
  unless (map fst (qualifiedCohort retained) == map fst (qualifiedCohort owned)) (Left (CohortDiffers "keys"))
  case (qualifiedStorage retained,qualifiedStorage owned) of
    (Nothing,Nothing) -> pure ()
    (Just left,Just right) -> unless (storageEvidenceSteps left == storageEvidenceSteps right
      && storageEvidenceBootEe left == storageEvidenceBootEe right
      && storageEvidenceBootHandback left == storageEvidenceBootHandback right) (Left StorageStepsDiffer)
    _ -> Left StorageStepsDiffer
  forM_ (zip (qualifiedCohort retained) (qualifiedCohort owned)) $ \((name, left), (_, right)) ->
    unless (left == right) (Left (CohortDiffers name))
  unless (map resultRoute (qualifiedRoutes retained) == map resultRoute (qualifiedRoutes owned)) (Left RouteSetDiffers)
  pairs <- forM (zip (qualifiedRoutes retained) (qualifiedRoutes owned)) $ \(left, right) -> do
    let name = resultRoute left
        differs refusal select = when (select left /= select right) (Left (refusal name))
        writeKey write = (effectAddress write, effectWidth write, effectBytes write)
        persists event = case event of CustomRead {} -> False; _ -> True
        pressAgrees (Press leftStatus leftMenu leftEndpoint leftMode) (Press rightStatus rightMenu rightEndpoint rightMode) = leftMenu == rightMenu && leftEndpoint == rightEndpoint && leftMode == rightMode
          && (leftStatus == rightStatus || BudgetExhausted `elem` [leftStatus, rightStatus])
    differs SetterSequenceDiffers (map setterItemShape . collapseBlocks . resultSetters)
    differs WriteSequenceDiffers (map writeKey . filter effectChanges . resultWrites)
    differs TransferSequenceDiffers resultTransfers
    differs AppendSequenceDiffers (filter persists . resultPersistence)
    differs FinalBytesDiffer resultFinal
    unless (length (resultPresses left) == length (resultPresses right) && and (zipWith pressAgrees (resultPresses left) (resultPresses right)))
      (Left (PressDiffers name))
    pure (left, right)
  let left = M.map collapseRepeats (getterSequences retained)
      right = M.map collapseRepeats (getterSequences owned)
  forM_ (S.toList (M.keysSet left <> M.keysSet right)) $ \key -> do
    let leftValues = M.findWithDefault [] key left
        rightValues = M.findWithDefault [] key right
    unless (leftValues == rightValues)
      (Left (GetterSequenceDiffers key (length (takeWhile id (zipWith (==) leftValues rightValues)))))
  pure pairs

-- | Each getter key's results in session order.
getterSequences :: Qualified -> M.Map GetterKey [Word32]
getterSequences result = M.fromListWith (flip (++)) [(getterKey getter, [getterValue getter]) | getter <- qualifiedGetters result]

collapseRepeats :: Eq a => [a] -> [a]
collapseRepeats = map NE.head . NE.group

-- | One setter call, or a run of one `timeDrivenSequence` block whose calls repeat it
-- consecutively, with the run's block count.
data SetterItem = SingleSetter Call | BlockRun [Call] Int

type CallIdentity = ([Word32], Either Word32 (TraceCore, LogicalTaskId), Maybe Word32)

callIdentity :: Call -> CallIdentity
callIdentity call = (callArguments call,caller,callResult call)
  where caller = case callContext call of
          ExactCaller -> Left (callReturn call)
          BankApplyCaller core task -> Right (core,task)

setterItemShape :: SetterItem -> Either [CallIdentity] CallIdentity
setterItemShape item = case item of
  BlockRun block _ -> Left (map callIdentity block)
  SingleSetter call -> Right (callIdentity call)

collapseBlocks :: [Call] -> [SetterItem]
collapseBlocks calls = case splitAt 4 calls of
  (block, rest) | map (take 1 . callArguments) block == map (: []) timeDrivenSequence ->
    let (repeats, rest') = span' block rest in BlockRun block (1 + repeats) : collapseBlocks rest'
  _ -> case calls of
    call : rest -> SingleSetter call : collapseBlocks rest
    [] -> []
 where
  span' block remaining = case splitAt 4 remaining of
    (next, rest) | next == block -> let (count, rest') = span' block rest in (count + 1, rest')
    _ -> (0 :: Int, remaining)

-- | A block run's index among the collapsed items, the index of its first element in the
-- uncollapsed sequence, and its block count.
data BlockRunAt = BlockRunAt { runPosition :: Int, runFirst :: Int, runRepeats :: Int } deriving (Eq, Show)

blockRuns :: [Call] -> [BlockRunAt]
blockRuns calls = [ BlockRunAt position first count
                  | (position, first, BlockRun _ count) <- zip3 [0 ..] (scanl (+) 0 (map width items)) items ]
 where
  items = collapseBlocks calls
  width item = case item of
    SingleSetter _ -> 1
    BlockRun block count -> length block * count

-- | One effect, or a run of consecutive `timeDrivenSequence` setter effects with its block count.
data EffectItem = SingleEffect CrawlEffect | EffectBlockRun Int

effectItemShape :: EffectItem -> Maybe CrawlEffect
effectItemShape item = case item of
  SingleEffect effect -> Just effect
  EffectBlockRun _ -> Nothing

collapseEffects :: [CrawlEffect] -> [EffectItem]
collapseEffects effects
  | isBlock effects = let (repeats, rest) = count (drop 4 effects) in EffectBlockRun (1 + repeats) : collapseEffects rest
  | effect : rest <- effects = SingleEffect effect : collapseEffects rest
  | otherwise = []
 where
  block = map SetterEffect timeDrivenSequence
  isBlock remaining = take 4 remaining == block
  count remaining
    | isBlock remaining = let (repeats, rest) = count (drop 4 remaining) in (repeats + 1, rest)
    | otherwise = (0 :: Int, remaining)

effectBlockRuns :: [CrawlEffect] -> [BlockRunAt]
effectBlockRuns effects = [ BlockRunAt position first count
                          | (position, first, EffectBlockRun count) <- zip3 [0 ..] (scanl (+) 0 (map width items)) items ]
 where
  items = collapseEffects effects
  width item = case item of
    SingleEffect _ -> 1
    EffectBlockRun count -> length timeDrivenSequence * count

-- | Admits the guest control's refusal: its observation stopped at its first guarded hit, which is
-- the restored setter word's retired arm entered for that word's property, and every earlier hit is
-- an armed site of the control plan.
checkGuestControl :: AdmittedBindings -> Qualified -> ControlWord -> String -> String -> Observation -> Either Refusal Value
checkGuestControl admitted owned control datDigest image4Digest observation = do
  receipt <- either (Left . ReceiptShape) Right (decodeValue (obsReceipt observation))
  let receiptDigest path = either (Left . ReceiptShape) Right (digestAt path receipt)
      require refusal condition = unless condition (Left refusal)
      bindings = admittedBindings admitted
      contract = admittedContract admitted
      workload = admittedWorkload admitted
  let inputs = PlanInputs ControlRole contract (bindImages bindings) (bindLinked bindings) (Just control) (admittedBoundary admitted)
  plan <- either (Left . InputDigest) Right (case workload of
    RouteWorkload _ -> derivePlan inputs
    StorageWorkload matrix -> deriveStoragePlan inputs matrix)
  image <- either (Left . InputDigest) Right (roleImage4 inputs)
  expectedImage <- digestIdentity "control image4" image4Digest
  require (InputDigest "control image4") (sha256 image == expectedImage)
  require (ReceiptShape "role") (either (const False) (== String "control") (field "role" receipt))
  expectedDat <- digestIdentity "control DAT" datDigest
  recordedDat <- receiptDigestSha256 (InputDigest "control DAT") ["inputs", "dat", "sha256"] receipt
  require (InputDigest "control DAT") (recordedDat == expectedDat)
  recordedImage <- receiptDigestSha256 (InputDigest "role image4") ["inputs", "role_image4_sha256"] receipt
  require (InputDigest "role image4") (recordedImage == expectedImage)
  forM_ [ (["inputs", "linked", "sha256"], admittedLinkedDigest admitted, "linked")
        , (["inputs", "contract", "sha256"], admittedContractDigest admitted, "contract")
        , (["inputs", "observer", "sha256"], admittedObserver admitted, "observer")
        , (["inputs", "routes", "sha256"], admittedRoutesDigest admitted, "routes")
        , (["inputs", "symbols", "sha256"], admittedSymbolsDigest admitted, "symbols")
        , (["files", "hits.jsonl"], sha256 (obsHits observation), "hits.jsonl")
        , (["files", "spec.json"], sha256 (obsSpec observation), "spec.json")
        , (["files", "steps.json"], sha256 (obsSteps observation), "steps.json")
        , (["files", "reset-observation.json"], sha256 (obsReset observation), "reset-observation.json")
        , (["files", "emulator-session.json"], sha256 (obsSession observation), "emulator-session.json") ] $ \(path, expected, name) -> do
    recorded <- receiptDigestSha256 (InputDigest name) path receipt
    require (InputDigest name) (recorded == expected)
  forM_ [path | (path, _) <- qualifiedCohort owned, path `elem` ["inputs.run.sha256", "inputs.boot_rom.sha256", "inputs.adjustment_nand.sha256", "inputs.bridge.sha256"]] $ \path -> do
    recorded <- receiptDigest (T.splitOn "." (T.pack path))
    require (CohortDiffers path) (lookup path (qualifiedCohort owned) == Just recorded)
  forM_ (lookup "inputs.initial_nand.kind" (qualifiedCohort owned)) $ \seed -> do
    recorded <- receiptDigest ["inputs",T.pack seed,"sha256"]
    require (CohortDiffers "inputs.initial_nand.sha256") (lookup "inputs.initial_nand.sha256" (qualifiedCohort owned) == Just recorded)
  spec <- either (Left . ReceiptShape . ("spec: " ++)) Right (decodeValue (obsSpec observation))
  require SiteMapDiffers (spec == specValue plan)
  reason <- either (Left . ReceiptShape) Right (field "outcome" receipt >>= field "refusal" >>= field "reason")
  require (ObservationRefused reason) (reason == String "guarded-fetch")
  hits <- fmap V.fromList . forM (zip [0 ..] (filter (not . B.null) (C.lines (obsHits observation)))) $ \(index, line) ->
    either (Left . HitShape index) Right (decodeValue line >>= parseHit)
  require (HitBound (V.length hits)) (V.length hits <= hitLimit)
  checkCaptures plan hits
  final <- if V.null hits then Left (HitShape 0 "the control recorded no hit") else Right (hits V.! (V.length hits-1))
  let finalIndex=V.length hits-1
  V.imapM_ (\index hit -> do
    kind <- classifyHit plan index hit
    case kind of
      GuardHit -> Left (GuardedFetch index)
      _ -> pure ()) (V.take finalIndex hits)
  require (UnarmedSite finalIndex) (M.lookup (hitAddress final) (planSites plan) == Just (Guard RetiredSetterArm)
    && hitSite final == PlanSite (Guard RetiredSetterArm))
  require (GuardedFetch finalIndex) (hitAddress final == controlTarget control
    && maybe True (\property -> take 1 (hitArguments final) == [property]) (controlProperty control))
  pure (object [ "verdict" .= ("refused-at-restored-arm" :: T.Text), "property" .= controlProperty control
               , "target" .= controlTarget control, "hits" .= V.length hits ])

persistenceValue :: PersistenceEvent -> Value
persistenceValue event = case event of
  HistoricalAppend registers -> object ["kind" .= ("historical-append"::T.Text),"registers" .= registers]
  CustomAppend (CustomRecordWire working) (CustomRecordWire previous) (CustomPayloadBytes live) ->
    object ["kind" .= ("custom-append"::T.Text),"candidate_wire" .= hex working,"previous_wire" .= hex previous,"live_payload" .= hex live]
  CustomRead bank target -> object ["kind" .= ("custom-read"::T.Text),"bank" .= bankNumber bank,"cache" .= case target of PrimaryRecordReadBuffer -> (0x8a0080::Word32); SecondaryRecordReadBuffer -> 0x8c0080]
  CustomProgram bank (ProgramOffset offset) (ProgramBytes bytes) ->
    object ["kind" .= ("custom-program"::T.Text),"bank" .= bankNumber bank,"offset" .= offset,"record_wire" .= hex (B.take 52 bytes),"staging_tail" .= hex (B.drop 52 bytes)]
  CustomProgramResult (MediumStatus status) -> object ["kind" .= ("custom-program-result"::T.Text),"status" .= status]
  CustomErase bank -> object ["kind" .= ("custom-erase"::T.Text),"bank" .= bankNumber bank]
  CustomEraseResult (MediumStatus status) -> object ["kind" .= ("custom-erase-result"::T.Text),"status" .= status]
  where
    bankNumber bank = case bank of RecordBank0 -> (0::Int); RecordBank1 -> 1
    hex = concatMap (\byte -> let text = showHex byte "" in replicate (2-length text) '0' ++ text) . B.unpack

routeSummary :: RouteResult -> Value
routeSummary result = object
  [ "route" .= resultRoute result
  , "class" .= routeClassWire (resultClass result)
  , "properties_written" .= propertiesWritten result
  , "getter_calls" .= length (resultGetters result)
  , "setter_calls" .= length (resultSetters result)
  , "unreturned_calls" .= length [() | call <- resultGetters result ++ resultSetters result, isNothing (callResult call)]
  , "writes" .= length (resultWrites result)
  , "byte_changing_writes" .= length (filter effectChanges (resultWrites result))
  , "accessor_writes" .= length [() | write <- resultWrites result, effectWriter write == OwnedAccessor]
  , "transfers" .= length (resultTransfers result)
  , "area3_appends" .= appendCount (resultPersistence result)
  , "persistence" .= map persistenceValue (resultPersistence result)
  , "effect_block_runs" .= [object ["position" .= runPosition run, "first_effect" .= runFirst run, "repeats" .= runRepeats run] | run <- resultEffectBlocks result]
  ]

propertiesWritten :: RouteResult -> [Word32]
propertiesWritten result = S.toList (S.fromList (concatMap (take 1 . callArguments) (resultSetters result)))

qualifiedValue :: Qualified -> Value
qualifiedValue result = object
  [ "role" .= roleWire (qualifiedRole result)
  , "verdict" .= ("qualified" :: T.Text)
  , "cohort" .= object [K.fromString name .= digest | (name, digest) <- qualifiedCohort result]
  , "hits" .= qualifiedHits result
  , "steps" .= qualifiedSteps result
  , "routes" .= length (qualifiedRoutes result)
  , "getter_calls" .= sum (map (length . resultGetters) (qualifiedRoutes result))
  , "setter_calls" .= sum (map (length . resultSetters) (qualifiedRoutes result))
  , "writes" .= sum (map (length . resultWrites) (qualifiedRoutes result))
  , "byte_changing_writes" .= sum (map (length . filter effectChanges . resultWrites) (qualifiedRoutes result))
  , "area3_appends" .= sum (map (appendCount . resultPersistence) (qualifiedRoutes result))
  , "properties_by_class" .= object
      [ K.fromText (routeClassWire routeClass) .= S.toList (S.fromList (concatMap propertiesWritten [route | route <- qualifiedRoutes result, resultClass route == routeClass]))
      | routeClass <- [minBound .. maxBound] ]
  , "per_route" .= map routeSummary (qualifiedRoutes result)
  , "storage" .= maybe Null storageEvidenceValue (qualifiedStorage result)
  ]

-- | The compared routes, and per getter key each role's call count and collapsed length.
comparisonValue :: Qualified -> Qualified -> [(RouteResult, RouteResult)] -> Value
comparisonValue retained owned pairs = object
  [ "verdict" .= ("equal" :: T.Text)
  , "routes" .= length pairs
  , "per_route" .=
      [ object
          [ "retained" .= routeSummary left, "owned" .= routeSummary right
          , "setter_block_runs" .= object ["retained" .= runsValue left, "owned" .= runsValue right] ]
      | (left, right) <- pairs ]
  , "getter_keys" .=
      [ object
          [ "caller" .= getterCaller key, "arguments" .= getterArguments key
          , "retained" .= countsValue key retainedSequences, "owned" .= countsValue key ownedSequences ]
      | key <- S.toList (M.keysSet retainedSequences <> M.keysSet ownedSequences) ]
  ]
 where
  runsValue result = [object ["position" .= runPosition run, "first_call" .= runFirst run, "repeats" .= runRepeats run] | run <- blockRuns (resultSetters result)]
  retainedSequences = getterSequences retained
  ownedSequences = getterSequences owned
  countsValue key sequences = let values = M.findWithDefault [] key sequences in
    object ["calls" .= length values, "collapsed" .= length (collapseRepeats values)]

-- Serialized refusal controls.

data ControlName
  = ReorderedWrite | DroppedWrite | ExtraWrite | ChangedValue | MissingSetterCall | ChangedGetterResult | StaleGetterResult
  | WrongDatDigest | WrongLinkedBin | InjectedGuardedFetch
  deriving (Eq, Show, Enum, Bounded)

controlNames :: [ControlName]
controlNames = [minBound .. maxBound]

-- | A control passes when its observed refusal's rendering starts with one of the expected prefixes.
data ControlOutcome = ControlOutcome
  { controlName :: ControlName
  , controlExpected :: [String]
  , controlObserved :: Either Refusal ()
  , controlStale :: Maybe StaleGetter
  }

-- | The stale getter control's mutation: the key, the call's index among the key's returned calls,
-- its return hit, and r0 there before and after.
data StaleGetter = StaleGetter { staleKey :: GetterKey, staleCall :: Int, staleHit :: Int, staleFrom :: Word32, staleTo :: Word32 } deriving (Eq, Show)

-- | The first key, then the first call, whose call lies strictly inside a run of its value; the new
-- value is the run's previous distinct value, or its next one for the first run.
staleGetter :: [GetterResult] -> Either String StaleGetter
staleGetter getters = case candidates of
  chosen : _ -> Right chosen
  [] -> Left "no getter key has two distinct values and a call inside a run"
 where
  byKey = M.fromListWith (flip (++)) [(getterKey getter, [getter]) | getter <- getters]
  candidates =
    [ StaleGetter key position (getterReturnHit call) (getterValue call) replacement
    | (key, calls) <- M.toList byKey
    , let values = map getterValue calls
          runs = collapseRepeats values
    , length runs >= 2
    , (position, previous, call, next) <- zip4 [1 ..] values (drop 1 calls) (drop 2 values)
    , previous == getterValue call && next == getterValue call
    , let run = length (collapseRepeats (take (position + 1) values)) - 1
          replacement = if run > 0 then runs !! (run - 1) else runs !! (run + 1) ]

-- | Runs one control against ROLE's observation, which must be admitted with the other role's.
runControl :: AdmittedBindings -> Role -> Observation -> Observation -> ControlName -> Either String ControlOutcome
runControl admitted role mutated other control = do
  let workload = admittedWorkload admitted
      otherRole = if role == RetainedRole then OwnedRole else RetainedRole
      admitBoth observation = do
        mine <- qualify admitted role observation
        theirs <- qualify admitted otherRole other
        _ <- if role == RetainedRole then compareRoles mine theirs else compareRoles theirs mine
        pure ()
      reAdmit bindings' observationRole observation = do
        context <- admitBindings bindings'
        qualify context observationRole observation
      outcome expected result = Right (ControlOutcome control expected result Nothing)
      -- An owned leaf's writes come from the accessors, whose hits qualify matches row by row.
      writeRefusals = "WriteSequenceDiffers" : ["AccessorStoreUnmatched" | role == OwnedRole]
        ++ storageRefusals
      storageRefusals = case workload of
        RouteWorkload _ -> []
        StorageWorkload _ -> ["StorageShape","RegionContinuity","StorageCheckFailed","StorageStepsDiffer"]
  steps <- decodeValue (obsSteps mutated) >>= asArray
  case control of
    WrongDatDigest
      | role == RetainedRole -> outcome ["InputDigest \"source DAT\""] (() <$ reAdmit ((admittedBindings admitted) { bindSourceDat = bindOwnedDat (admittedBindings admitted) }) role mutated)
      | otherwise -> outcome ["InputDigest \"owned DAT\""] (() <$ reAdmit ((admittedBindings admitted) { bindOwnedDat = bindSourceDat (admittedBindings admitted) }) role mutated)
    WrongLinkedBin -> outcome ["InputDigest \"linked\""] (() <$ reAdmit ((admittedBindings admitted) { bindLinked = flipFirst (bindLinked (admittedBindings admitted)) }) role mutated)
    InjectedGuardedFetch -> do
      plan <- either (Left . show) Right (planFor admitted OwnedRole)
      (address,label) <- maybe (Left "owned plan has no guarded site") Right
        (find (\(_,candidate) -> case candidate of Guard _ -> True; _ -> False) (M.toList (planSites plan)))
      (_,leaf) <- leafStep workload steps
      (from,_) <- stepRange leaf
      values <- decodeHitValues mutated
      template <- case drop from values of
        first:_ -> Right first
        [] -> case reverse (take from values) of previous:_ -> Right previous; [] -> Left "guard control has no captured execution context"
      let hit=setField "site" (String (T.pack (siteLabelWire label))) (setField "address" (String (hexText address)) template)
      changed <- insertHits mutated from [hit]
      outcome [if role == RetainedRole then "UnarmedSite" else "GuardedFetch"] (() <$ qualify admitted role changed)
    _ | control `elem` [ReorderedWrite, DroppedWrite, ExtraWrite, ChangedValue] -> do
      (leafIndex, leaf) <- leafStep workload steps
      changed <- mutateLeafWrites control leaf
      observation <- replaceStep mutated leafIndex changed
      outcome writeRefusals (admitBoth observation)
    MissingSetterCall -> do
      plan <- either (Left . show) Right (planFor admitted role)
      hits <- decodeHitValues mutated
      phasesEnd <- attachedEnd mutated
      (entry, returned) <- matchedCall plan (take phasesEnd hits) SetterEntry
      observation <- removeHits mutated [entry, returned]
      outcome ("SetterSequenceDiffers":storageRefusals) (admitBoth observation)
    ChangedGetterResult -> do
      plan <- either (Left . show) Right (planFor admitted role)
      hits <- decodeHitValues mutated
      (_, returned) <- matchedCall plan hits GetterEntry
      let flipped = setR0 (\value -> value `xor` 1) (hits !! returned)
      observation <- replaceHit mutated returned flipped
      outcome ("GetterSequenceDiffers":storageRefusals) (admitBoth observation)
    StaleGetterResult -> do
      mine <- either (Left . show) Right (qualify admitted role mutated)
      stale <- staleGetter (qualifiedGetters mine)
      hits <- decodeHitValues mutated
      observation <- replaceHit mutated (staleHit stale) (setR0 (const (staleTo stale)) (hits !! staleHit stale))
      Right (ControlOutcome control ("GetterSequenceDiffers":storageRefusals) (admitBoth observation) (Just stale))
    _ -> Left "unhandled control"
 where
  hexText :: Word32 -> T.Text
  hexText value = T.pack ("0x" ++ showHex value "")

flipFirst :: B.ByteString -> B.ByteString
flipFirst bytes = case B.uncons bytes of
  Just (first, rest) -> B.cons (first `xor` 1) rest
  Nothing -> bytes

planFor :: AdmittedBindings -> Role -> Either Refusal Plan
planFor admitted role = rolePlanPlan <$> rolePlanFor admitted role

-- | The hit count when the attached phase ended.
attachedEnd :: Observation -> Either String Int
attachedEnd observation = do
  segments <- decodeValue (obsReset observation) >>= field "segments" >>= asArray
  case reverse segments of
    attached : _ -> field "hits_end" attached >>= asInt
    [] -> Left "the reset observation has no segment"

-- | The last changing branch leaf, or the last changing linear storage step.
leafStep :: Workload -> [Value] -> Either String (Int, Value)
leafStep workload steps = do
  parents <- case workload of
    RouteWorkload _ -> S.fromList <$> mapM (\step -> field "parent" step >>= asInt) steps
    StorageWorkload _ -> pure S.empty
  candidates <- fmap concat . forM (zip [0 ..] steps) $ \(index, step) -> do
    number <- field "step" step >>= asInt
    rows <- field "watch" step >>= field "records" >>= asArray
    let changing = [() | row <- rows, Right True <- [changingWrite row]]
    pure [(index, step) | not (S.member number parents), length changing >= 2]
  case reverse candidates of
    chosen : _ -> Right chosen
    [] -> Left "no leaf step has two byte-changing CPU writes"

changingWrite :: Value -> Parse Bool
changingWrite row = do
  source <- field "source" row >>= asText
  width <- field "size" row >>= asInt
  if source /= "cpu-write" || width > 8 then Right False else do
    value' <- field "value" row >>= asWord64
    before <- field "before" row >>= asArray >>= mapM asInt
    little <- (== "little") <$> (field "byte_order" row >>= asText)
    pure (map fromIntegral (valueBytes little width value') /= before)

stepRange :: Value -> Either String (Int, Int)
stepRange step = field "hits" step >>= asArray >>= mapM asInt >>= \range -> case range of
  [from, to] -> Right (from, to)
  _ -> Left "malformed step range"

-- | Rewrites the leaf step's rows, then recomputes every later preimage and the after regions from
-- the before regions so that only the comparison can see the change.
mutateLeafWrites :: ControlName -> Value -> Either String Value
mutateLeafWrites control step = do
  rows <- field "watch" step >>= field "records" >>= asArray
  changing <- fmap concat . forM (zip [0 ..] rows) $ \(index, row) -> (\yes -> [index | yes]) <$> changingWrite row
  (first, second) <- case changing of
    a : b : _ -> Right (a, b)
    _ -> Left "leaf step lost its writes"
  let row = rows !! first
  width <- field "size" row >>= asInt
  value' <- field "value" row >>= asWord64
  little <- (== "little") <$> (field "byte_order" row >>= asText)
  before <- field "before" row >>= asArray >>= mapM asInt
  let mask = if width >= 8 then maxBound else (1 `shiftLeft` (8 * width)) - 1
      preimage = foldr (\byte acc -> acc * 256 + fromIntegral byte) 0 ((if little then id else reverse) before) :: Word64
      withValue number = setField "value" (Number (fromIntegral number)) row
  rows' <- case control of
    ReorderedWrite -> do
      address <- field "physical" row >>= asWord
      later <- field "physical" (rows !! second) >>= asWord
      laterWidth <- field "size" (rows !! second) >>= asInt
      unless (later + fromIntegral laterWidth <= address || address + fromIntegral width <= later)
        (Left "the two writes overlap")
      Right [if index == first then rows !! second else if index == second then row else other | (index, other) <- zip [0 ..] rows]
    DroppedWrite -> Right [other | (index, other) <- zip [0 :: Int ..] rows, index /= first]
    ExtraWrite -> do
      extra <- case [candidate | flipBits <- [1, 2], let candidate = (value' `xor` flipBits) .&. mask, candidate /= preimage] of
        candidate : _ -> Right candidate
        [] -> Left "no extra value differs from the preimage"
      Right (take first rows ++ [withValue extra] ++ drop first rows)
    ChangedValue -> Right [if index == first then withValue ((value' `xor` 1) .&. mask) else other | (index, other) <- zip [0 ..] rows]
    _ -> Left "not a write control"
  regionsBefore <- field "regions_before" step >>= parseRegions
  after <- field "regions_after" step >>= parseRegions
  (rowsCoherent, state) <- foldM coherent ([], M.fromList [(address, byte) | (base, bytes) <- M.toList regionsBefore, (address, byte) <- zip [base ..] (B.unpack bytes)]) rows'
  let after' = M.mapWithKey (\base bytes -> B.pack [M.findWithDefault byte address state | (address, byte) <- zip [base ..] (B.unpack bytes)]) after
      regionsValue regions = Array (V.fromList [object ["start" .= start, "bytes" .= C.unpack (hexEncode bytes)] | (start, bytes) <- M.toList regions])
      watch = either (const Null) id (field "watch" step)
  pure (setField "regions_after" (regionsValue after') (setField "watch" (setField "records" (Array (V.fromList (reverse rowsCoherent))) watch) step))
 where
  shiftLeft :: Word64 -> Int -> Word64
  shiftLeft value count = value * (2 ^ count)
  coherent (done, state) row = do
    source <- field "source" row >>= asText
    if source /= "cpu-write" then Right (row : done, state) else do
      address <- field "physical" row >>= asWord
      width <- field "size" row >>= asInt
      before <- field "before" row >>= asArray >>= mapM asInt
      little <- (== "little") <$> (field "byte_order" row >>= asText)
      value' <- case field "value" row of
        Right Null -> Right Nothing
        Right number -> Just <$> asWord64 number
        Left problem -> Left problem
      let addresses = [address + fromIntegral offset | offset <- [0 .. width - 1]]
          before' = [maybe (fromIntegral old) id (M.lookup target state) | (target, old) <- zip addresses before]
          state' = case value' of
            Just number -> foldl (\s (target, byte) -> if M.member target s then M.insert target byte s else s) state (zip addresses (valueBytes little width number))
            Nothing -> foldl (flip M.delete) state addresses
      Right (setField "before" (toJSONBytes before') row : done, state')
  toJSONBytes bytes = Array (V.fromList [Number (fromIntegral byte) | byte <- bytes])

hexEncode :: B.ByteString -> B.ByteString
hexEncode = B.concatMap (\byte -> C.pack (let text = showHex byte "" in replicate (2 - length text) '0' ++ text))

setField :: T.Text -> Value -> Value -> Value
setField name new (Object fields) = Object (KM.insert (K.fromText name) new fields)
setField _ _ other = other

setR0 :: (Word32 -> Word32) -> Value -> Value
setR0 change hit = case field "registers" hit of
  Right registers -> case field "r0" registers >>= hexWordText of
    Right value -> setField "registers" (setField "r0" (String (T.pack ("0x" ++ showHex (change value) ""))) registers) hit
    Left _ -> hit
  Left _ -> hit

decodeHitValues :: Observation -> Either String [Value]
decodeHitValues observation = mapM decodeValue (filter (not . B.null) (C.lines (obsHits observation)))

-- | The last entry of the kind with a later return hit at its (sp, lr) among the given hits.
matchedCall :: Plan -> [Value] -> SiteLabel -> Either String (Int, Int)
matchedCall plan values entryLabel = do
  hits <- mapM parseHit values
  let returnLabel = if entryLabel == GetterEntry then GetterReturn else SetterReturn
      label hit = M.lookup (hitAddress hit) (planSites plan)
      entries = [index | (index, hit) <- zip [0 ..] hits, label hit == Just entryLabel]
      closing entry = let hit = hits !! entry in
        find (\index -> let candidate = hits !! index in label candidate == Just returnLabel
               && hitAddress candidate == hitLr hit && hitSp candidate == hitSp hit) [entry + 1 .. length hits - 1]
  case [(entry, returned) | entry <- reverse entries, Just returned <- [closing entry]] of
    pair : _ -> Right pair
    [] -> Left "no matched call to mutate"

rewriteObservation :: Observation -> [Value] -> (Int -> Int) -> Either String Observation
rewriteObservation observation hits shift = do
  steps <- decodeValue (obsSteps observation) >>= asArray
  steps' <- forM steps $ \step -> do
    (from, to) <- stepRange step
    pure (setField "hits" (Array (V.fromList [Number (fromIntegral (shift from)), Number (fromIntegral (shift to))])) step)
  session <- decodeValue (obsSession observation)
  fields <- asObject session
  case KM.lookup "kind" fields of
    Just (String "settings-storage-lifecycle") -> rewriteStorageObservation observation hits steps' shift session
    Just _ -> Left "unknown observation session kind"
    Nothing -> do
      reset <- decodeValue (obsReset observation)
      segments <- field "segments" reset >>= asArray
      segments' <- forM segments $ \segment -> do
        end <- field "hits_end" segment >>= asInt
        pure (setField "hits_end" (Number (fromIntegral (shift end))) segment)
      let hitBytes = BL.toStrict (mconcat [A.encode hit <> "\n" | hit <- hits])
          stepBytes = BL.toStrict (A.encode (Array (V.fromList steps')) <> "\n")
          resetBytes = BL.toStrict (A.encode (setField "segments" (Array (V.fromList segments')) reset))
      rebind observation { obsHits = hitBytes, obsSteps = stepBytes, obsReset = resetBytes }

rewriteStorageObservation :: Observation -> [Value] -> [Value] -> (Int -> Int) -> Value -> Either String Observation
rewriteStorageObservation observation hits steps shift session = do
  manifests <- field "sessions" session >>= asArray
  numbered <- forM manifests $ \manifest -> (,) <$> (field "session" manifest >>= parseBootSession) <*> pure manifest
  resets <- fmap M.fromList . forM numbered $ \(number,manifest) -> do
    from <- field "hit_start" manifest >>= asInt
    raw <- maybe (Left "control has no raw SDK reset") Right
      (M.lookup number (M.insert (BootSession 0) (obsReset observation) (obsBootResets observation)))
    reset <- decodeValue raw
    segments <- field "segments" reset >>= asArray
    adjusted <- forM segments $ \segment -> do
      end <- field "hits_end" segment >>= asInt
      pure (setField "hits_end" (Number (fromIntegral (shift (from+end)-shift from))) segment)
    pure (number,BL.toStrict (A.encode (setField "segments" (A.toJSON adjusted) reset)))
  updated <- forM numbered $ \(number,manifest) -> do
    from <- field "hit_start" manifest >>= asInt
    to <- field "hit_end" manifest >>= asInt
    start <- field "start" manifest
    resetIdentity <- field "reset_observation" start
    resetBytes <- maybe (Left "control reset is absent") Right (M.lookup number resets)
    let start'=setField "reset_observation" (setField "sha256" (String (T.pack (digestOf resetBytes))) resetIdentity) start
    sdk <- field "sdk" manifest
    pure (number,setField "hit_start" (Number (fromIntegral (shift from)))
      (setField "hit_end" (Number (fromIntegral (shift to)))
        (setField "start" start' (setField "sdk" (setField "checkpoint" start' sdk) manifest))))
  let updatedMap=M.fromList updated
  adjustedSteps <- forM steps $ \step -> do
    action <- field "action" step >>= parseEither A.parseJSON
    case action of
      Storage.ColdReboot -> do
        number <- field "session" step >>= parseBootSession
        result <- field "result" step
        next <- field "next_session" result >>= parseBootSession
        from <- field "new_hits_start" result >>= asInt
        parent <- maybe (Left "control parent SDK session is absent") Right (M.lookup number updatedMap)
        child <- maybe (Left "control child SDK session is absent") Right (M.lookup next updatedMap)
        prior <- field "sdk" parent
        start <- field "start" child
        resetBytes <- maybe (Left "control child raw reset is absent") Right (M.lookup next resets)
        reset <- decodeValue resetBytes
        pure (setField "result" (setField "new_hits_start" (Number (fromIntegral (shift from)))
          (setField "prior_session" prior (setField "new_start" start (setField "new_reset" reset result)))) step)
      _ -> pure step
  initial <- maybe (Left "control initial reset is absent") Right (M.lookup (BootSession 0) resets)
  rebind observation
    { obsHits=BL.toStrict (mconcat [A.encode hit <> "\n" | hit <- hits])
    , obsSteps=BL.toStrict (A.encode adjustedSteps <> "\n")
    , obsReset=initial
    , obsBootResets=M.delete (BootSession 0) resets
    , obsSession=BL.toStrict (A.encode (setField "sessions" (A.toJSON (map snd updated)) session) <> "\n")
    }

rebind :: Observation -> Either String Observation
rebind observation = do
  receipt <- decodeValue (obsReceipt observation)
  files <- field "files" receipt
  let files' = foldr (\(name, bytes) acc -> setField name (String (T.pack (digestOf bytes))) acc) files
        [("hits.jsonl", obsHits observation), ("steps.json", obsSteps observation), ("reset-observation.json", obsReset observation)
        ,("emulator-session.json",obsSession observation)]
  pure observation { obsReceipt = BL.toStrict (A.encode (setField "files" files' receipt)) }

insertHits :: Observation -> Int -> [Value] -> Either String Observation
insertHits observation at' inserted = do
  hits <- decodeHitValues observation
  let count = length inserted
  rewriteObservation observation (take at' hits ++ inserted ++ drop at' hits) (\index -> if index > at' then index + count else index)

removeHits :: Observation -> [Int] -> Either String Observation
removeHits observation removed = do
  hits <- decodeHitValues observation
  let gone = S.fromList removed
      shift index = index - S.size (S.filter (< index) gone)
  rewriteObservation observation [hit | (index, hit) <- zip [0 ..] hits, not (S.member index gone)] shift

replaceHit :: Observation -> Int -> Value -> Either String Observation
replaceHit observation index hit = do
  hits <- decodeHitValues observation
  rewriteObservation observation [if position == index then hit else other | (position, other) <- zip [0 ..] hits] id

replaceStep :: Observation -> Int -> Value -> Either String Observation
replaceStep observation index step = do
  steps <- decodeValue (obsSteps observation) >>= asArray
  rebind observation { obsSteps = BL.toStrict (A.encode (Array (V.fromList [if position == index then step else other | (position, other) <- zip [0 ..] steps])) <> "\n") }

newtype StorageBankBytes = StorageBankBytes B.ByteString deriving (Eq, Show)
newtype ScannerRowBytes = ScannerRowBytes B.ByteString deriving (Eq, Show)
newtype ModeClassWord = ModeClassWord Word32 deriving (Eq, Show)
newtype InputBitmap = InputBitmap Word32 deriving (Eq, Show)
newtype ControllerStateWord = ControllerStateWord Word16 deriving (Eq, Show)
newtype ControllerEventWord = ControllerEventWord Word16 deriving (Eq, Show)

data StorageSnapshot = StorageSnapshot
  { snapshotBank0 :: StorageBankBytes
  , snapshotBank1 :: StorageBankBytes
  , snapshotPrevious :: StorageBankBytes
  , snapshotBacking :: StorageBankBytes
  , snapshotCustomPrevious :: CustomRecordWire
  , snapshotCustomWorking :: CustomRecordWire
  , snapshotModeClass :: ModeClassWord
  , snapshotScannerRow :: ScannerRowBytes
  , snapshotInputBitmap :: InputBitmap
  , snapshotController30State :: ControllerStateWord
  , snapshotController30Event :: ControllerEventWord
  , snapshotController74State :: ControllerStateWord
  } deriving (Eq, Show)

data StorageEndpoint = StorageEndpoint LocalUiMenu LocalUiMode deriving (Eq, Show)
data StorageCriterion
  = ChangedCustomSave | UnchangedCustomSave | CustomRecall | CustomModeCycle
  | AlternateCancellation | S1ApplicationHandback | RepeatedSettingRoundTrips
  | ColdCommittedReload
  deriving (Eq, Show, Enum, Bounded)
data StorageStepEvidence = StorageStepEvidence
  { storageEvidenceName :: Storage.MatrixStepName
  , storageEvidenceBefore :: StorageSnapshot
  , storageEvidenceAfter :: StorageSnapshot
  , storageEvidenceEndpoint :: Maybe StorageEndpoint
  , storageEvidenceChecks :: [Storage.StorageCheck]
  , storageEvidenceDial :: Maybe ModeDialWitness
  , storageEvidenceEe :: [EeWitness]
  , storageEvidenceHandback :: [S1Witness]
  } deriving (Eq, Show)
data BootChain = BootChain BootSession Sha256 Sha256 deriving Eq
data StorageEvidence = StorageEvidence
  { storageEvidenceSteps :: [StorageStepEvidence]
  , storageEvidenceBoots :: [BootChain]
  , storageEvidenceBootEe :: [(BootSession,[EeWitness])]
  , storageEvidenceBootHandback :: [(BootSession,[S1Witness])]
  } deriving Eq

newtype SdkModel = SdkModel T.Text deriving Eq
data BootSeed = AdjustmentBoot Sha256 | CommittedBoot Sha256 deriving Eq
data BootIdentity = BootIdentity
  { identityModel :: SdkModel
  , identityRun :: Sha256
  , identityDat :: Sha256
  , identityRom :: Sha256
  , identityProcedure :: Sha256
  , identitySeed :: BootSeed
  } deriving Eq
data FreshStart = FreshStart
  { freshIdentity :: BootIdentity
  , _freshKey :: Sha256
  , freshResetDigest :: Sha256
  } deriving Eq
data BootManifest = BootManifest
  { manifestNumber :: BootSession
  , manifestHitRange :: (Int,Int)
  , manifestIdentity :: BootIdentity
  , manifestStart :: FreshStart
  , manifestInstructions :: Instructions
  } deriving Eq
data CommittedExport = CommittedExport
  { exportedDigest :: Sha256
  , exportedParent :: BootIdentity
  , exportedInstructions :: Instructions
  }

parseDigest :: Value -> Parse Sha256
parseDigest value = asText value >>= either Left Right . parseSha256Hex . T.unpack

parseBootSession :: Value -> Parse BootSession
parseBootSession value = BootSession <$> asWord value

bootResetPaths :: B.ByteString -> Either Refusal [(BootSession,FilePath)]
bootResetPaths bytes = either (Left . StorageShape) Right $ do
  fields <- decodeValue bytes >>= asObject
  case KM.lookup "kind" fields of
    Nothing -> pure []
    Just (String "settings-storage-lifecycle") -> do
      exact ["schema","kind","sessions"] fields
      schema <- at "schema" fields >>= asInt
      unless (schema == 1) (Left "storage session manifest schema differs")
      sessions <- at "sessions" fields >>= asArray
      unless (not (null sessions)) (Left "storage session manifest is empty")
      forM (drop 1 (zip [0 :: Word32 ..] sessions)) $ \(expected,entry) -> do
        number <- field "session" entry >>= parseBootSession
        unless (number == BootSession expected) (Left "storage sessions are out of order")
        pure (number,printf "boot-%04d/reset-observation.json" expected)
    _ -> Left "unknown emulator-session kind"

parseBootIdentity :: Value -> Parse BootIdentity
parseBootIdentity value = do
  fields <- asObject value
  exact ["schema","model","run_document","installed_dat","boot_rom","adjustment_nand","record_nand","committed_nand","source_kind","card_sha256","procedure"] fields
  schema <- at "schema" fields >>= asInt
  unless (schema == 3) (Left "SDK boot identity schema differs")
  model <- at "model" fields >>= asText
  unless (not (T.null model)) (Left "SDK model identity is empty")
  unless (KM.lookup "record_nand" fields == Just Null && KM.lookup "card_sha256" fields == Just Null)
    (Left "storage boot adds a record seed or card input")
  kind <- at "source_kind" fields >>= asText
  seed <- case kind of
    "initial-seeded" -> do
      unless (KM.lookup "committed_nand" fields == Just Null) (Left "seeded storage boot also consumes committed NAND")
      AdjustmentBoot <$> (at "adjustment_nand" fields >>= parseDigest)
    "full-committed" -> do
      unless (KM.lookup "adjustment_nand" fields == Just Null) (Left "committed storage boot reseeds adjustment NAND")
      CommittedBoot <$> (at "committed_nand" fields >>= parseDigest)
    _ -> Left "unknown SDK NAND source kind"
  BootIdentity (SdkModel model) <$> (at "run_document" fields >>= parseDigest)
    <*> (at "installed_dat" fields >>= parseDigest) <*> (at "boot_rom" fields >>= parseDigest)
    <*> (at "procedure" fields >>= parseDigest) <*> pure seed

parseFreshStart :: BootSession -> Value -> Parse FreshStart
parseFreshStart (BootSession number) value = do
  fields <- asObject value
  exact ["key","inputs","directory","key_seconds","start","boot_seconds","saved","reset_observation"] fields
  unless (KM.lookup "start" fields == Just (String "fresh-boot") && KM.lookup "saved" fields == Just (Bool False))
    (Left "storage boot restored or saved a live checkpoint")
  reset <- at "reset_observation" fields >>= asObject
  exact ["path","sha256"] reset
  path <- at "path" reset >>= asText
  let relative = T.pack (printf "boot-%04d/reset-observation.json" number)
  unless (path == relative || ("/" <> relative) `T.isSuffixOf` path) (Left "SDK reset sidecar names another boot")
  FreshStart <$> (at "inputs" fields >>= parseBootIdentity) <*> (at "key" fields >>= parseDigest)
    <*> (at "sha256" reset >>= parseDigest)

parseBootManifest :: Int -> Value -> Parse BootManifest
parseBootManifest expected value = do
  fields <- asObject value
  exact ["session","hit_start","hit_end","sdk","start","input_committed_sha256"] fields
  number <- at "session" fields >>= parseBootSession
  unless (toInteger (bootSessionNumber number) == toInteger expected) (Left "storage boot sequence differs")
  from <- at "hit_start" fields >>= asInt
  to <- at "hit_end" fields >>= asInt
  unless (from >= 0 && from <= to) (Left "storage boot hit range is invalid")
  sdk <- at "sdk" fields >>= asObject
  schema <- at "schema" sdk >>= asInt
  unless (schema == 1) (Left "SDK final session schema differs")
  forM_ ["bridge_error","capture_evidence_error"] $ \name ->
    unless (KM.lookup name sdk == Just Null) (Left "SDK storage session has a capture or bridge failure")
  boundary <- at "boundary" sdk >>= asObject
  unless (KM.lookup "failure" boundary == Just Null) (Left "SDK storage session has a machine failure")
  instructions <- Instructions <$> (at "instructions" boundary >>= asWord64)
  startValue <- at "start" fields
  checkpoint <- at "checkpoint" sdk
  unless (checkpoint == startValue) (Left "SDK final checkpoint differs from its actual start")
  start <- parseFreshStart number startValue
  identity <- at "inputs" sdk >>= parseBootIdentity
  unless (identity == freshIdentity start) (Left "SDK storage input identity changed within a boot")
  committed <- at "input_committed_sha256" fields
  case identitySeed identity of
    AdjustmentBoot _ -> unless (committed == Null) (Left "adjustment boot advertises a committed input")
    CommittedBoot digest -> do
      recorded <- parseDigest committed
      unless (recorded == digest) (Left "SDK consumed another committed export")
  pure (BootManifest number (from,to) identity start instructions)

parseCommittedExport :: Storage.MatrixStepName -> Value -> Parse CommittedExport
parseCommittedExport name value = do
  fields <- asObject value
  exact ["status","artifact","medium","source_kind","base","inputs","boundary","after","fingerprint","format","geometry"] fields
  unless (KM.lookup "status" fields == Just (String "exported")
    && KM.lookup "format" fields == Just (String "type1-nand-committed-image"))
    (Left "storage NAND export was refused or has another format")
  geometry <- at "geometry" fields
  unless (geometry == object ["blocks" .= (1024::Int),"pages_per_block" .= (64::Int),"page_bytes" .= (2048::Int),"spare_bytes" .= (512::Int)])
    (Left "storage NAND export geometry differs")
  medium <- at "medium" fields >>= asText
  unless (not (T.null medium)) (Left "storage NAND export has no medium")
  artifact <- at "artifact" fields >>= asObject
  exact ["path","sha256","bytes"] artifact
  path <- at "path" artifact >>= asText
  let relative = "nand-" <> Storage.matrixStepNameText name <> ".nand.gz"
  unless (path == relative || ("/" <> relative) `T.isSuffixOf` path) (Left "NAND export names another matrix step")
  size <- at "bytes" artifact >>= asWord64
  unless (size > 0) (Left "committed NAND export is empty")
  before <- at "boundary" fields
  after <- at "after" fields
  unless (before == after) (Left "committed NAND export advanced the machine")
  boundary <- asObject before
  exact ["instructions","clock"] boundary
  instructions <- Instructions <$> (at "instructions" boundary >>= asWord64)
  identity <- at "inputs" fields >>= parseBootIdentity
  kind <- at "source_kind" fields >>= asText
  unless (kind == case identitySeed identity of AdjustmentBoot _ -> "initial-seeded"; CommittedBoot _ -> "full-committed")
    (Left "NAND export source differs from its parent boot")
  CommittedExport <$> (at "sha256" artifact >>= parseDigest) <*> pure identity <*> pure instructions

parseStorageSnapshot :: Value -> Parse StorageSnapshot
parseStorageSnapshot value = do
  fields <- asObject value
  exact ["bank0","bank1","previous","backing","custom_previous","custom_working","mode_class","c_scanner_row","c_input_bitmap","controller30_state","controller30_event","controller74_state"] fields
  let bytes name count = do
        decoded <- at name fields >>= asText >>= hexBytes
        unless (B.length decoded == count) (Left ("storage snapshot width differs: " ++ T.unpack name))
        pure decoded
      half name = do
        number <- at name fields >>= asWord
        unless (number <= 0xffff) (Left "controller snapshot is not a halfword")
        pure (fromIntegral number)
  StorageSnapshot <$> (StorageBankBytes <$> bytes "bank0" 0x1c2) <*> (StorageBankBytes <$> bytes "bank1" 0x1c2)
    <*> (StorageBankBytes <$> bytes "previous" 0x1c2) <*> (StorageBankBytes <$> bytes "backing" 0x1c2)
    <*> (CustomRecordWire <$> bytes "custom_previous" 52) <*> (CustomRecordWire <$> bytes "custom_working" 52)
    <*> (ModeClassWord <$> (at "mode_class" fields >>= asWord)) <*> (ScannerRowBytes <$> bytes "c_scanner_row" 28)
    <*> (InputBitmap <$> (at "c_input_bitmap" fields >>= asWord))
    <*> (ControllerStateWord <$> half "controller30_state") <*> (ControllerEventWord <$> half "controller30_event")
    <*> (ControllerStateWord <$> half "controller74_state")

newtype MenuIdentifier = MenuIdentifier Word32 deriving (Eq, Show)
newtype SettingIdentifier = SettingIdentifier Word32 deriving (Eq, Show)
newtype MenuCursor = MenuCursor Word32 deriving (Eq, Show)
data SdkMenu = NoSdkMenu | SdkMenu MenuIdentifier SettingIdentifier MenuCursor deriving (Eq, Show)
newtype AdcValue = AdcValue Word16 deriving (Eq, Show)
data ModeDialWitness = ModeDialWitness AdcValue AdcValue AdcValue SdkMenu deriving (Eq, Show)

data StorageColdBoot = StorageColdBoot
  { coldNextSession :: BootSession
  , coldPhases :: [(AttachPhase,Interval)]
  , coldExport :: CommittedExport
  , coldInputDigest :: Sha256
  }
data ObservedStorageStep = ObservedStorageStep
  { observedStorageNumber :: Int
  , observedStorageSpec :: Storage.StorageStep
  , observedStorageSession :: BootSession
  , observedStorageInterval :: Interval
  , observedStorageWholeHits :: (Int,Int)
  , observedStorageAfterRegions :: Regions
  , observedStorageBefore :: StorageSnapshot
  , observedStorageAfter :: StorageSnapshot
  , observedStorageEndpoint :: Maybe StorageEndpoint
  , observedStorageDial :: Maybe ModeDialWitness
  , observedStorageInput :: Maybe Instructions
  , observedStorageCold :: Maybe StorageColdBoot
  }

parseSdkMenu :: Value -> Parse SdkMenu
parseSdkMenu value = do
  fields <- asObject value
  exact ["menu_id","setting_id","cursor"] fields
  case (KM.lookup "menu_id" fields,KM.lookup "setting_id" fields,KM.lookup "cursor" fields) of
    (Just Null,Just Null,Just Null) -> pure NoSdkMenu
    _ -> SdkMenu <$> (MenuIdentifier <$> (at "menu_id" fields >>= asWord))
      <*> (SettingIdentifier <$> (at "setting_id" fields >>= asWord))
      <*> (MenuCursor <$> (at "cursor" fields >>= asWord))

parseStorageEndpoint :: Value -> Parse StorageEndpoint
parseStorageEndpoint value = do
  state <- parsePostSettlePrefix value
  StorageEndpoint <$> (field "ui" state >>= parseLocalUiMenu)
    <*> (field "source_mode" state >>= parseLocalUiMode)

data StorageKey = StorageCrawlKey CrawlKey | StorageHalfShutter

parseStoragePress :: StorageKey -> A.Object -> Parse (Press,Instructions)
parseStoragePress action result = do
  exact ["kind","idle","press","bounded_post_settle_prefix"] result
  unless (KM.lookup "kind" result == Just (String "key")) (Left "storage key result has another kind")
  ran <- at "idle" result >>= field "ran" >>= asWord64
  unless (ran == instructionsValue preKeyIdle) (Left "storage key missed its pre-key idle")
  began <- at "idle" result >>= field "instructions" >>= asWord64
  unless (began >= ran) (Left "storage key idle instruction boundary is invalid")
  press <- at "press" result
  fields <- asObject press
  exact ["snapshot","key","menu","settle","instructions","display_wait_instructions","failure"] fields
  let expected = case action of
        StorageCrawlKey key -> T.pack (crawlKeyWire key)
        StorageHalfShutter -> "half-press"
  key <- at "key" fields >>= asText
  unless (key == expected && KM.lookup "snapshot" fields == Just Null && KM.lookup "failure" fields == Just Null)
    (Left "SDK did not execute the declared storage key")
  pressed <- at "instructions" fields >>= asWord64
  displayWait <- at "display_wait_instructions" fields >>= asWord64
  unless (displayWait <= pressed) (Left "storage display wait exceeds the actual key operation")
  status <- at "settle" fields >>= field "status" >>= asText >>= \name ->
    maybe (Left "unknown storage settle status") Right (lookup name [(settleStatusWire known,known) | known <- [minBound..maxBound]])
  unless (status /= PriorRenderPending) (Left "storage key was not pressed")
  menu <- at "menu" fields
  _ <- parseSdkMenu menu
  prefix <- at "bounded_post_settle_prefix" result
  StorageEndpoint endpointMenu endpointMode <- parseStorageEndpoint prefix
  elapsed <- field "elapsed" prefix
  end <- field "instructions" elapsed >>= asWord64
  prefixRan <- field "ran" elapsed >>= asWord64
  unless (toInteger began+toInteger pressed+toInteger prefixRan == toInteger end)
    (Left "storage key and bounded prefix instruction ranges are discontinuous")
  pure (Press status menu endpointMenu endpointMode,Instructions began)

parseModeDialWitness :: ModeDial -> A.Object -> Parse (ModeDialWitness,StorageEndpoint)
parseModeDialWitness expected result = do
  exact ["kind","dial","bounded_post_settle_prefix"] result
  unless (KM.lookup "kind" result == Just (String "mode-dial")) (Left "storage dial result has another kind")
  dial <- at "dial" result >>= asObject
  exact ["position","sample","low","high","menu"] dial
  position <- at "position" dial >>= asText
  unless (position == T.pack (modeDialName expected)) (Left "SDK moved another physical dial detent")
  let adc name = do
        value <- at name dial >>= asWord
        unless (value <= 0xffff) (Left "mode dial ADC value is out of range")
        pure (fromIntegral value :: Word16)
  sample <- adc "sample"
  low <- adc "low"
  high <- adc "high"
  unless (low <= sample && sample <= high) (Left "mode dial sample leaves its calibrated window")
  menu <- at "menu" dial >>= parseSdkMenu
  endpoint <- at "bounded_post_settle_prefix" result >>= parseStorageEndpoint
  pure (ModeDialWitness (AdcValue sample) (AdcValue low) (AdcValue high) menu,endpoint)

parseStoragePhases :: BootManifest -> B.ByteString -> Parse [(AttachPhase,Interval)]
parseStoragePhases manifest bytes = do
  value <- decodeValue bytes
  schema <- field "schema" value >>= asInt
  unless (schema == 1 && sha256 bytes == freshResetDigest (manifestStart manifest))
    (Left "fresh SDK reset identity differs from its raw sidecar")
  phases <- phaseIntervals value
  segments <- field "segments" value >>= asArray
  counters <- mapM (\segment -> field "instructions" segment >>= asWord64) segments
  unless (and (zipWith (<=) (0:counters) counters)
    && all (<= instructionsValue (manifestInstructions manifest)) counters)
    (Left "fresh boot instruction boundaries are out of order")
  let (offset,_) = manifestHitRange manifest
      name phase = if manifestNumber manifest == BootSession 0 then PhaseInterval phase else BootPhaseInterval (manifestNumber manifest) phase
      shifted (phase,interval) = (phase,interval{intervalName=name phase,intervalHits=let (from,to)=intervalHits interval in (from+offset,to+offset)})
  pure (map shifted phases)

parseStorageStep :: M.Map BootSession BootManifest -> M.Map BootSession B.ByteString -> BootSession -> Int -> Storage.StorageStep -> Value -> Parse ObservedStorageStep
parseStorageStep manifests resets expectedSession expected spec value = do
  fields <- asObject value
  exact ["step","name","session","action","hits","watch","regions_before","regions_after","storage_before","storage_after","result"] fields
  number <- at "step" fields >>= asInt
  name <- at "name" fields >>= asText
  session <- at "session" fields >>= parseBootSession
  unless (number == expected && name == Storage.matrixStepNameText (Storage.storageStepName spec) && session == expectedSession)
    (Left "storage steps are out of order or name another session")
  action <- at "action" fields >>= parseEither A.parseJSON
  unless (action == Storage.storageStepAction spec) (Left "observed storage action differs from its declared input")
  whole <- at "hits" fields >>= asArray >>= mapM asInt
  range@(from,to) <- case whole of
    [begin,end] | begin >= 0 && begin <= end -> Right (begin,end)
    _ -> Left "storage step hit range is invalid"
  rows <- at "watch" fields >>= parseWatch FixedViews
  before <- at "regions_before" fields >>= parseRegions
  after <- at "regions_after" fields >>= parseRegions
  storageBefore <- at "storage_before" fields >>= parseStorageSnapshot
  storageAfter <- at "storage_after" fields >>= parseStorageSnapshot
  result <- at "result" fields >>= asObject
  let warm press endpoint dial input = ObservedStorageStep number spec session
        (Interval (StepInterval number) before after rows range press) range after storageBefore storageAfter endpoint dial input Nothing
  case action of
    Storage.PressKey key -> do
      (press,input) <- parseStoragePress (StorageCrawlKey key) result
      pure (warm (Just press) (Just (StorageEndpoint (pressEndpointMenu press) (pressEndpointMode press))) Nothing (Just input))
    Storage.PressHalfShutter -> do
      (press,input) <- parseStoragePress StorageHalfShutter result
      pure (warm (Just press) (Just (StorageEndpoint (pressEndpointMenu press) (pressEndpointMode press))) Nothing (Just input))
    Storage.SetModeDial position -> do
      (dial,endpoint) <- parseModeDialWitness position result
      pure (warm Nothing (Just endpoint) (Just dial) Nothing)
    Storage.ColdReboot -> do
      exact ["kind","export","prior_session","new_start","new_reset","input_committed_sha256","next_session","new_hits_start","regions_before_reboot","storage_before_reboot"] result
      unless (KM.lookup "kind" result == Just (String "cold-reboot")) (Left "storage cold reboot has another result kind")
      next <- at "next_session" result >>= parseBootSession
      unless (toInteger (bootSessionNumber next) == toInteger (bootSessionNumber session)+1) (Left "cold reboot skips a session")
      oldManifest <- maybe (Left "cold reboot parent session is absent") Right (M.lookup session manifests)
      nextManifest <- maybe (Left "cold reboot child session is absent") Right (M.lookup next manifests)
      start <- at "new_start" result >>= parseFreshStart next
      unless (start == manifestStart nextManifest) (Left "cold reboot supplied another fresh start")
      exported <- at "export" result >>= parseCommittedExport (Storage.storageStepName spec)
      inputDigest <- at "input_committed_sha256" result >>= parseDigest
      unless (exportedParent exported == manifestIdentity oldManifest
        && exportedInstructions exported == manifestInstructions oldManifest
        && inputDigest == exportedDigest exported
        && identitySeed (manifestIdentity nextManifest) == CommittedBoot inputDigest)
        (Left "committed export does not bind the actual parent and child boots")
      newStart <- at "new_hits_start" result >>= asInt
      unless (newStart == snd (manifestHitRange oldManifest) && newStart == fst (manifestHitRange nextManifest)
        && from <= newStart && newStart <= to) (Left "cold reboot hit boundary differs from actual sessions")
      resetBytes <- maybe (Left "cold reboot raw reset sidecar is absent") Right (M.lookup next resets)
      reset <- decodeValue resetBytes
      inlineReset <- at "new_reset" result
      unless (reset == inlineReset) (Left "cold reboot inline reset differs from its raw SDK sidecar")
      phases <- parseStoragePhases nextManifest resetBytes
      unless (case reverse phases of
        (_,final):_ -> snd (intervalHits final) == to && intervalAfter final == after
        [] -> False) (Left "cold reboot after-state is not its fresh attach state")
      beforeReboot <- at "regions_before_reboot" result >>= parseRegions
      oldSnapshot <- at "storage_before_reboot" result >>= parseStorageSnapshot
      unless (storageBefore == oldSnapshot) (Left "cold reboot advanced before exporting its medium")
      let cold = StorageColdBoot next phases exported inputDigest
      pure (ObservedStorageStep number spec session (Interval (StepInterval number) before beforeReboot rows (from,newStart) Nothing)
        range after storageBefore storageAfter Nothing Nothing Nothing (Just cold))

snapshotSegments :: StorageSnapshot -> [(Word32,B.ByteString)]
snapshotSegments snapshot =
  [ (0x96cc64,bank (snapshotBank0 snapshot)),(0x96ce26,bank (snapshotBank1 snapshot))
  , (0x89ec00,bank (snapshotPrevious snapshot)),(0x9c281c,bank (snapshotBacking snapshot))
  , (0x89f838,record (snapshotCustomPrevious snapshot)),(0x89f874,record (snapshotCustomWorking snapshot))
  ]
  where bank (StorageBankBytes bytes)=bytes; record (CustomRecordWire bytes)=bytes

checkSnapshotRegions :: Int -> StorageSnapshot -> Regions -> Either Refusal ()
checkSnapshotRegions number snapshot regions = do
  forM_ (snapshotSegments snapshot) $ \(address,bytes) ->
    unless (case M.lookupLE address regions of
      Just (base,region) -> let offset=fromIntegral (address-base) in offset+B.length bytes <= B.length region && B.take (B.length bytes) (B.drop offset region) == bytes
      _ -> False) (Left (StorageShape ("storage snapshot differs from watched bytes in step " ++ show number ++ " at " ++ show address)))
  unless (snapshotScannerRow snapshot == canonicalScannerRow) (Left (StorageShape "C scanner row differs from its source binding"))

canonicalScannerRow :: ScannerRowBytes
canonicalScannerRow = ScannerRowBytes (B.pack (concatMap (valueBytes True 4) [0x317,0,0x101,3,0x37243726,8,0]))

data StorageCall = StorageCall
  { boundaryEntryIndex :: Int
  , boundaryReturnIndex :: Int
  , boundaryEntry :: Hit
  , boundaryReturn :: Hit
  }
data RefreshImages = RefreshImages CustomRecordWire CustomRecordWire CustomPayloadBytes
data CopyImages = CopyImages StorageBankBytes StorageBankBytes StorageBankBytes StorageBankBytes deriving (Eq, Show)
data CopyWitness = CopyWitness
  { copyFamily :: Storage.ImageFamily
  , copyDirection :: Storage.CopyDirection
  , copyBefore :: CopyImages
  , copyAfter :: CopyImages
  , copyEntryIndex :: Int
  , copyReturnIndex :: Int
  }

storageSite :: Hit -> Maybe Storage.LifecycleSite
storageSite hit = case hitSite hit of PlanSite (StorageSite site) -> Just site; _ -> Nothing

pairedStorageCalls :: Storage.LifecycleSite -> Storage.LifecycleSite -> V.Vector Hit -> (Int,Int) -> Either Refusal [StorageCall]
pairedStorageCalls entrySite returnSite hits (from,to) = reverse . snd <$> foldM visit (M.empty,[]) [from..to-1]
  where
    visit (open,done) index =
      let hit=hits V.! index
          entryKey=(hitCore hit,hitTaskId hit,hitSp hit,hitLr hit)
          returnKey=(hitCore hit,hitTaskId hit,hitSp hit,hitAddress hit)
      in case storageSite hit of
        Just site | site == entrySite -> do
          unless (M.notMember entryKey open) (Left (StorageShape "storage boundary reenters an active caller frame"))
          pure (M.insert entryKey (index,hit) open,done)
        Just site | site == returnSite -> case M.lookup returnKey open of
          Just (entered,first) -> pure (M.delete returnKey open,StorageCall entered index first hit:done)
          Nothing -> pure (open,done)
        _ -> pure (open,done)

sameExecution :: Hit -> Hit -> Bool
sameExecution first second = case (executionTask first,executionTask second) of
  (Right before,Right after) -> hitCore first == hitCore second && sameTask before after
  _ -> False

refreshImages :: Hit -> Maybe RefreshImages
refreshImages hit = case hitMemory hit of
  [previous,working,live] | map B.length [previous,working,live] == [52,52,40] ->
    Just (RefreshImages (CustomRecordWire previous) (CustomRecordWire working) (CustomPayloadBytes live))
  _ -> Nothing

copyImages :: Hit -> Maybe CopyImages
copyImages hit = case hitMemory hit of
  [active,alternate,previous,backing] | all ((==0x1c2) . B.length) [active,alternate,previous,backing] ->
    Just (CopyImages (StorageBankBytes active) (StorageBankBytes alternate) (StorageBankBytes previous) (StorageBankBytes backing))
  _ -> Nothing

recordBytes :: CustomRecordWire -> B.ByteString
recordBytes (CustomRecordWire bytes) = bytes

recordPayload :: CustomRecordWire -> CustomPayloadBytes
recordPayload (CustomRecordWire bytes) = CustomPayloadBytes (B.take 40 (B.drop 4 bytes))

sealedCustomRecord :: CustomRecordWire -> Bool
sealedCustomRecord (CustomRecordWire bytes) = B.length bytes == 52 && B.take 2 bytes /= B.pack [0xff,0xff]
  && word32At bytes 44 .&. 0xfffff800 == 0
  && RecordHardware.hardware RecordModel.Prepare RecordModel.Hardware3 bytes == (RecordModel.Accept,bytes)

eventsBetween :: [(Int,PersistenceEvent)] -> (Int,Int) -> [PersistenceEvent]
eventsBetween events (from,to) = [event | (index,event) <- events,index >= from,index < to]

confirmedRecords :: [PersistenceEvent] -> [CustomRecordWire]
confirmedRecords = go Nothing
  where
    go _ [] = []
    go _ (CustomProgram _ _ (ProgramBytes bytes):rest) = go (Just (CustomRecordWire (B.take 52 bytes))) rest
    go pending (CustomProgramResult (MediumStatus status):rest) =
      [record | status == 0,Just record <- [pending],sealedCustomRecord record] ++ go Nothing rest
    go pending (_:rest) = go pending rest

isPersistentCustom :: PersistenceEvent -> Bool
isPersistentCustom event = case event of
  CustomAppend {} -> True
  CustomProgram {} -> True
  CustomProgramResult {} -> True
  CustomErase {} -> True
  CustomEraseResult {} -> True
  _ -> False

refreshMatches :: Storage.RecordChange -> [(Int,PersistenceEvent)] -> StorageCall -> Bool
refreshMatches change allEvents call = sameExecution (boundaryEntry call) (boundaryReturn call)
  && take 1 (hitArguments (boundaryReturn call)) == [0]
  && case (refreshImages (boundaryEntry call),refreshImages (boundaryReturn call)) of
    (Just (RefreshImages previous working live),Just (RefreshImages afterPrevious afterWorking afterLive)) ->
      let metadata record=B.take 2 (B.drop 2 (recordBytes record))
          candidateMatches record=metadata record == metadata working && recordPayload record == live
          events=eventsBetween allEvents (boundaryEntryIndex call,boundaryReturnIndex call+1)
          sameCandidate=candidateMatches afterWorking && afterLive == live
          unchangedTail=B.drop 44 (recordBytes afterWorking) == B.drop 44 (recordBytes working)
      in sameCandidate && unchangedTail && case change of
        Storage.Unchanged -> candidateMatches previous && not (any isPersistentCustom events)
          && afterPrevious == previous && B.take 2 (recordBytes afterWorking) == B.take 2 (recordBytes working)
        Storage.Changed -> not (candidateMatches previous)
          && any (\event -> case event of
            CustomAppend request old visible -> B.take 4 (recordBytes request) == B.take 4 (recordBytes working)
              && recordPayload request == live && B.drop 44 (recordBytes request) == B.drop 44 (recordBytes working)
              && old == previous && visible == live
            _ -> False) events
          && sealedCustomRecord afterPrevious && afterPrevious `elem` confirmedRecords events
          && B.take 2 (recordBytes afterWorking) == B.take 2 (recordBytes afterPrevious)
          && candidateMatches afterPrevious
    _ -> False

copyWitness :: Storage.ImageFamily -> Storage.CopyDirection -> StorageCall -> Maybe CopyWitness
copyWitness family direction call = do
  before@(CopyImages (StorageBankBytes active) (StorageBankBytes alternate) (StorageBankBytes previous) (StorageBankBytes backing)) <- copyImages (boundaryEntry call)
  after@(CopyImages (StorageBankBytes newActive) (StorageBankBytes newAlternate) (StorageBankBytes newPrevious) (StorageBankBytes newBacking)) <- copyImages (boundaryReturn call)
  let argumentMatches=case family of
        Storage.PreviousImage -> True
        Storage.AlternateImage -> take 1 (hitArguments (boundaryEntry call)) == [case direction of Storage.Capture -> 0; Storage.Restore -> 1]
      source=case family of Storage.PreviousImage -> previous; Storage.AlternateImage -> alternate
      copyMatches=case direction of
        Storage.Capture -> newActive == active && newBacking == backing
          && case family of
            Storage.PreviousImage -> newPrevious == active && newAlternate == alternate
            Storage.AlternateImage -> newAlternate == active && newPrevious == previous
        Storage.Restore -> newActive == source && newAlternate == alternate && newPrevious == previous
          && B.take 0x118 newBacking == B.take 0x118 (B.drop 16 source)
  if argumentMatches && copyMatches && sameExecution (boundaryEntry call) (boundaryReturn call)
    then Just (CopyWitness family direction before after (boundaryEntryIndex call) (boundaryReturnIndex call)) else Nothing

snapshotPayload :: Storage.PayloadTarget -> StorageSnapshot -> CustomPayloadBytes
snapshotPayload target snapshot = case target of
  Storage.ActiveCustomPayload -> fromBank (snapshotBank0 snapshot)
  Storage.BackingCustomPayload -> fromBank (snapshotBacking snapshot)
  Storage.PreviousRecordPayload -> recordPayload (snapshotCustomPrevious snapshot)
  Storage.WorkingRecordPayload -> recordPayload (snapshotCustomWorking snapshot)
  where fromBank (StorageBankBytes bytes) = CustomPayloadBytes (B.take 40 (B.drop 0x140 bytes))

currentProfile :: StorageSnapshot -> Storage.ProfileSlot
currentProfile snapshot = case snapshotModeClass snapshot of ModeClassWord 3 -> Storage.StoredCustom; _ -> Storage.LiveProfile

profileObserved :: Contract -> Storage.ProfileSlot -> StorageSnapshot -> [Call] -> [Call] -> Bool
profileObserved contract wanted snapshot getters setters = currentProfile snapshot == wanted && physicalDial
  && (any getterMatches getters || any setterMatches setters)
  where
    selector=case wanted of Storage.StoredCustom -> 0; Storage.LiveProfile -> 1
    InputBitmap bitmap=snapshotInputBitmap snapshot
    physicalDial=(bitmap .&. 0x02000000 /= 0) == (wanted == Storage.StoredCustom)
    slots=S.fromList [armsProperty arms | arms <- contractProperties contract,armsClass arms == OwnedSlotField]
    getterMatches call = physicalAddress (callReturn call) == 0x624648 && isJust (callResult call)
      && case callArguments call of [property,0,0,record] -> S.member property slots && record == selector; _ -> False
    setterMatches call = callResult call == Just 1
      && case (callContext call,callArguments call) of
        (BankApplyCaller _ _,[property,_,0,record]) -> S.member property slots && record == selector
        _ -> False

repeatedSetting :: Storage.StorageSetting -> V.Vector Hit -> PairState -> (Int,Int) -> Bool
repeatedSetting setting hits state (from,to) = walk from Nothing 0
  where
    property=case setting of Storage.FilmSimulation -> 0xfe; Storage.WhiteBalanceShift -> 0xee
    walk index pending completed
      | completed >= (2::Int) = True
      | index >= to = False
      | otherwise =
          let hit=hits V.! index in case (hitSite hit,hitArguments hit,M.lookup index (pairDone state)) of
            (PlanSite SetterEntry,[ident,value,bank,record],Just returned)
              | ident == property && bank == 0 && record <= 1 && returnValue returned == 1 ->
                  walk (index+1) (Just (value,bank,record,returnHit returned)) completed
            (PlanSite GetterEntry,[ident,0,bank,record],Just returned)
              | ident == property,Just (value,expectedBank,expectedRecord,assignedAt) <- pending
              , index > assignedAt && bank == expectedBank && record == expectedRecord && returnValue returned == value ->
                  walk (index+1) Nothing (completed+1)
            _ -> walk (index+1) pending completed

taskIdentityValue :: (TraceCore,LogicalTaskId) -> Value
taskIdentityValue (core,LogicalTaskId ident) = object
  ["core" .= (case core of TraceCoreA -> (0::Int); TraceCoreB -> 1),"task_id" .= ident]

storageEvidenceValue :: StorageEvidence -> Value
storageEvidenceValue evidence = object
  [ "criteria" .= map storageCriterionWire [minBound..maxBound]
  , "steps" .= map stepValue (storageEvidenceSteps evidence)
  , "cold_boots" .= [object ["session" .= bootSessionNumber boot,"parent_export_sha256" .= sha256Hex exported
      ,"child_input_sha256" .= sha256Hex consumed] | BootChain boot exported consumed <- storageEvidenceBoots evidence]
  , "boot_applications" .= [object ["session" .= bootSessionNumber boot,"ee" .= map eeWitnessValue applications
      ,"s1" .= map s1WitnessValue (fromMaybe [] (lookup boot (storageEvidenceBootHandback evidence)))]
      | (boot,applications) <- storageEvidenceBootEe evidence]
  ]
  where
    stepValue step = object
      [ "name" .= Storage.matrixStepNameText (storageEvidenceName step)
      , "checks" .= storageEvidenceChecks step
      , "before" .= snapshotValue (storageEvidenceBefore step)
      , "after" .= snapshotValue (storageEvidenceAfter step)
      , "endpoint" .= fmap (\(StorageEndpoint menu mode) -> object ["menu" .= menu,"source_mode" .= mode]) (storageEvidenceEndpoint step)
      , "dial" .= fmap (\(ModeDialWitness (AdcValue sample) (AdcValue low) (AdcValue high) menu) ->
          object ["sample" .= sample,"low" .= low,"high" .= high,"menu" .= sdkMenuValue menu]) (storageEvidenceDial step)
      , "ee" .= map eeWitnessValue (storageEvidenceEe step)
      , "s1" .= map s1WitnessValue (storageEvidenceHandback step)
      ]
    snapshotValue snapshot =
      let ModeClassWord modeClass=snapshotModeClass snapshot
          InputBitmap bitmap=snapshotInputBitmap snapshot
          ControllerStateWord state30=snapshotController30State snapshot
          ControllerEventWord event30=snapshotController30Event snapshot
          ControllerStateWord state74=snapshotController74State snapshot
          ScannerRowBytes scanner=snapshotScannerRow snapshot
      in object
        ["images" .= [object ["address" .= address,"sha256" .= digestOf bytes] | (address,bytes) <- snapshotSegments snapshot]
        ,"custom_previous" .= C.unpack (hexEncode (recordBytes (snapshotCustomPrevious snapshot)))
        ,"custom_working" .= C.unpack (hexEncode (recordBytes (snapshotCustomWorking snapshot)))
        ,"mode_class" .= modeClass,"c_input_bitmap" .= bitmap,"c_scanner_row" .= C.unpack (hexEncode scanner)
        ,"controller30_state" .= state30,"controller30_event" .= event30,"controller74_state" .= state74]
    sdkMenuValue menu = case menu of
      NoSdkMenu -> Null
      SdkMenu (MenuIdentifier ident) (SettingIdentifier setting) (MenuCursor cursor) ->
        object ["menu_id" .= ident,"setting_id" .= setting,"cursor" .= cursor]

storageCriterionWire :: StorageCriterion -> T.Text
storageCriterionWire criterion = case criterion of
  ChangedCustomSave -> "changed-custom-save"
  UnchangedCustomSave -> "unchanged-custom-save"
  CustomRecall -> "custom-recall"
  CustomModeCycle -> "custom-live-custom-cycle"
  AlternateCancellation -> "alternate-cancellation"
  S1ApplicationHandback -> "s1-application-handback"
  RepeatedSettingRoundTrips -> "repeated-setting-round-trips"
  ColdCommittedReload -> "cold-committed-reload"

eeWitnessValue :: EeWitness -> Value
eeWitnessValue witness =
  let ObservedShiftCode new=eeNewCode witness
      EeFrameClass frameClass=eeFrameClass witness
      CachedShiftValue cached=eeCachedValue witness
  in object
    [ "task" .= taskIdentityValue (eeTask witness),"new_code" .= new,"frame_class" .= frameClass
    , "frame_property" .= fmap (\(EeFrameProperty property) -> property) (eeFrameProperty witness)
    , "cached_value" .= cached,"old_value" .= fmap (\(StoredShiftValue old) -> old) (eeOldValue witness)
    , "mode_class" .= fmap (\(ModeClassWord modeClass) -> modeClass) (eeModeClass witness)
    , "white_balance_mode" .= fmap (\(WhiteBalanceMode mode) -> mode) (eeWhiteBalanceMode witness)
    , "compare_bank" .= fmap (\(StorageBankBytes bytes) -> C.unpack (hexEncode bytes)) (eeCompareBank witness)
    , "device_effects" .= map effectValue (eeDeviceEffects witness)
    ]
  where
    effectValue effect = case effect of
      ResetWhiteBalance index scope -> object
        ["kind" .= ("reset"::T.Text),"index" .= (case index of WbZero -> (0::Int); WbEight -> 8; WbNine -> 9)
        ,"record" .= (case scope of LiveWbReset -> (0::Int); CustomWbReset -> 1)]
      ApplyWhiteBalanceFirst (WhiteBalanceMode mode) -> object ["kind" .= ("apply-first"::T.Text),"mode" .= mode]
      ApplyWhiteBalanceSecond (WhiteBalanceMode mode) -> object ["kind" .= ("apply-second"::T.Text),"mode" .= mode]

newtype ObservedShiftCode = ObservedShiftCode Word8 deriving (Eq, Show)
newtype EeFrameClass = EeFrameClass Word32 deriving (Eq, Show)
newtype EeFrameProperty = EeFrameProperty Word32 deriving (Eq, Show)
newtype CachedShiftValue = CachedShiftValue Word32 deriving (Eq, Show)
newtype StoredShiftValue = StoredShiftValue Word32 deriving (Eq, Show)
newtype WhiteBalanceMode = WhiteBalanceMode Word32 deriving (Eq, Show)
data WbResetIndex = WbZero | WbEight | WbNine deriving (Eq, Show)
data WbResetScope = LiveWbReset | CustomWbReset deriving (Eq, Show)
data EeDeviceEffect = ResetWhiteBalance WbResetIndex WbResetScope | ApplyWhiteBalanceFirst WhiteBalanceMode | ApplyWhiteBalanceSecond WhiteBalanceMode deriving (Eq, Show)
data EeWitness = EeWitness
  { eeTask :: (TraceCore,LogicalTaskId)
  , eeNewCode :: ObservedShiftCode
  , eeFrameClass :: EeFrameClass
  , eeFrameProperty :: Maybe EeFrameProperty
  , eeCachedValue :: CachedShiftValue
  , eeOldValue :: Maybe StoredShiftValue
  , eeModeClass :: Maybe ModeClassWord
  , eeWhiteBalanceMode :: Maybe WhiteBalanceMode
  , eeCompareBank :: Maybe StorageBankBytes
  , eeDeviceEffects :: [EeDeviceEffect]
  } deriving (Eq, Show)
data LocatedEe = LocatedEe Int Int EeWitness

data MatrixProof = MatrixProof
  { proofSteps :: M.Map Storage.MatrixStepName ObservedStorageStep
  , proofCommitted :: M.Map Storage.MatrixStepName CustomRecordWire
  , proofCopies :: [CopyWitness]
  , proofCoverage :: [StorageCriterion]
  , proofEvidence :: [StorageStepEvidence]
  }

storageStepIntervals :: ObservedStorageStep -> [Interval]
storageStepIntervals step = observedStorageInterval step : maybe [] (map snd . coldPhases) (observedStorageCold step)

qualifyStorage :: Role -> Observation -> Value -> Contract -> Plan -> Maybe StorageBoundaries.BankApplyBoundary -> V.Vector Hit -> Storage.StorageMatrix -> Either Refusal Qualified
qualifyStorage role observation receipt contract plan bankBoundary hits matrix = do
  let parse = either (Left . StorageShape) Right
      require condition message = unless condition (Left (StorageShape message))
      receiptDigest path = either (Left . ReceiptShape) Right (digestAt path receipt)
      window (from,to) = [from..to-1]
  require (isJust bankBoundary) "storage lifecycle lacks its actual published bank-apply ELF"
  rawSessions <- parse $ do
    fields <- decodeValue (obsSession observation) >>= asObject
    exact ["schema","kind","sessions"] fields
    unless (KM.lookup "schema" fields == Just (Number 1)
      && KM.lookup "kind" fields == Just (String "settings-storage-lifecycle"))
      (Left "storage session manifest kind or schema differs")
    at "sessions" fields >>= asArray
  manifests <- parse (sequence [parseBootManifest index value | (index,value) <- zip [0..] rawSessions])
  firstManifest <- case manifests of
    first:_ -> pure first
    [] -> Left (StorageShape "storage workload has no initial SDK boot")
  let manifestMap=M.fromList [(manifestNumber entry,entry) | entry <- manifests]
      rawManifestMap=M.fromList (zip (map manifestNumber manifests) rawSessions)
      firstIdentity=manifestIdentity firstManifest
      (seedName,otherSeed,seedDigest)=case identitySeed firstIdentity of
        AdjustmentBoot digest -> ("adjustment_nand","committed_nand",digest)
        CommittedBoot digest -> ("committed_nand","adjustment_nand",digest)
  expectedRun <- receiptDigest ["inputs","run","sha256"]
  expectedDat <- receiptDigest ["inputs","dat","sha256"]
  expectedRom <- receiptDigest ["inputs","boot_rom","sha256"]
  expectedSeed <- receiptDigest ["inputs",seedName,"sha256"]
  require (sha256Hex seedDigest == expectedSeed) "initial SDK NAND differs from the receipt"
  parse $ do
    other <- field "inputs" receipt >>= field otherSeed
    unless (other == Null) (Left "initial SDK boot has a second NAND seed")
  forM_ manifests $ \manifest -> do
    let identity=manifestIdentity manifest
        (from,to)=manifestHitRange manifest
    require (sha256Hex (identityRun identity) == expectedRun && sha256Hex (identityDat identity) == expectedDat
      && sha256Hex (identityRom identity) == expectedRom
      && identityModel identity == identityModel firstIdentity && identityProcedure identity == identityProcedure firstIdentity)
      "SDK boot changes the actual program, model, ROM, or procedure"
    require (from >= 0 && from <= to && to <= V.length hits) "SDK boot hit range leaves the captured trace"
    _ <- foldM (\previous index -> do
      let counter=hitInstructions (hits V.! index)
      require (counter >= previous && counter <= manifestInstructions manifest) "SDK trace instruction counter leaves its boot boundary"
      pure counter) (Instructions 0) (window (from,to))
    pure ()
  zipWithM_ (\before after -> require (snd (manifestHitRange before) == fst (manifestHitRange after))
    "SDK boot hit ranges are discontinuous") manifests (drop 1 manifests)
  require (fst (manifestHitRange firstManifest) == 0
    && case reverse manifests of final:_ -> snd (manifestHitRange final) == V.length hits; [] -> False)
    "SDK boots do not cover the complete trace"
  let expectedChildren=M.keysSet (M.delete (BootSession 0) manifestMap)
  require (M.keysSet (obsBootResets observation) == expectedChildren) "raw child reset sidecars differ from actual SDK boots"
  let resetMap=M.insert (BootSession 0) (obsReset observation) (obsBootResets observation)
  phasesByBoot <- fmap M.fromList . forM manifests $ \manifest -> do
    bytes <- maybe (Left (StorageShape "SDK reset sidecar is absent")) Right (M.lookup (manifestNumber manifest) resetMap)
    phases <- parse (parseStoragePhases manifest bytes)
    pure (manifestNumber manifest,phases)
  attachedClocks <- parse $ M.traverseWithKey (\_ bytes -> do
    segments <- decodeValue bytes >>= field "segments" >>= asArray
    case reverse segments of
      final:_ -> Instructions <$> (field "instructions" final >>= asWord64)
      [] -> Left "SDK reset has no attached instruction boundary") resetMap
  rawSteps <- parse (decodeValue (obsSteps observation) >>= asArray)
  require (length rawSteps == length (Storage.matrixSteps matrix)) "storage workload did not execute every declared step"
  (finalSession,finalClock,stepsReversed) <- foldM (\(session,clock,done) (number,spec,value) -> do
    step <- parse (parseStorageStep manifestMap resetMap session number spec value)
    let (from,to)=intervalHits (observedStorageInterval step)
        (sessionFrom,sessionTo)=manifestHitRange (manifestMap M.! session)
    require (sessionFrom <= from && from <= to && to <= sessionTo) "storage action leaves its actual SDK boot"
    endClock <- parse $ do
      result <- field "result" value
      case observedStorageCold step of
        Just cold -> do
          unless (clock == exportedInstructions (coldExport cold)) (Left "cold export advanced beyond the previous action")
          maybe (Left "fresh child has no attached clock") Right (M.lookup (coldNextSession cold) attachedClocks)
        Nothing -> do
          prefix <- field "bounded_post_settle_prefix" result
          elapsed <- field "elapsed" prefix
          end <- field "instructions" elapsed >>= asWord64
          ran <- field "ran" elapsed >>= asWord64
          case Storage.storageStepAction spec of
            Storage.SetModeDial _ -> unless (ran <= end && Instructions (end-ran) == clock)
              (Left "physical dial prefix does not continue the previous action")
            _ -> do
              idleEnd <- field "idle" result >>= field "instructions" >>= asWord64
              unless (toInteger idleEnd == toInteger (instructionsValue clock)+toInteger (instructionsValue preKeyIdle))
                (Left "physical key idle does not continue the previous action")
          pure (Instructions end)
    let oldEnd=case observedStorageCold step of Just cold -> exportedInstructions (coldExport cold); Nothing -> endClock
    forM_ [from..to-1] $ \index -> require (hitInstructions (hits V.! index) >= clock && hitInstructions (hits V.! index) <= oldEnd)
      "storage action trace leaves its actual instruction boundary"
    forM_ (observedStorageCold step) $ \cold -> parse $ do
      parent <- maybe (Left "cold reboot parent document is absent") Right (M.lookup session rawManifestMap)
      child <- maybe (Left "cold reboot child document is absent") Right (M.lookup (coldNextSession cold) rawManifestMap)
      result <- field "result" value
      prior <- field "prior_session" result
      actualPrior <- field "sdk" parent
      nextStart <- field "new_start" result
      actualStart <- field "start" child
      unless (prior == actualPrior && nextStart == actualStart) (Left "cold reboot does not bind the actual closed parent and fresh child SDK sessions")
    pure (maybe session coldNextSession (observedStorageCold step),endClock,step:done))
    (BootSession 0,attachedClocks M.! BootSession 0,[]) (zip3 [1..] (Storage.matrixSteps matrix) rawSteps)
  require (toInteger (bootSessionNumber finalSession)+1 == toInteger (length manifests))
    "SDK session manifest contains a boot not produced by the declared cold actions"
  require (finalClock == manifestInstructions (manifestMap M.! finalSession)) "SDK final boundary advances beyond the last declared action"
  let steps=reverse stepsReversed
      initialPhases=map snd (M.findWithDefault [] (BootSession 0) phasesByBoot)
      ordered=initialPhases ++ concatMap storageStepIntervals steps
      fresh interval=case intervalName interval of BootPhaseInterval _ FirstStartPhase -> True; _ -> False
  forM_ ordered $ \interval -> do
    let (from,to)=intervalHits interval
    require (0 <= from && from <= to && to <= V.length hits) "storage interval leaves the captured trace"
  zipWithM_ (\before after -> do
    unless (snd (intervalHits before) == fst (intervalHits after)) (Left (HitPartition (intervalName after)))
    unless (fresh after || intervalAfter before == intervalBefore after) (Left (RegionContinuity (intervalName after))))
    ordered (drop 1 ordered)
  require (case (ordered,reverse ordered) of
    (first:_,final:_) -> fst (intervalHits first) == 0 && snd (intervalHits final) == V.length hits
    _ -> False) "storage phases and steps do not partition the complete trace"
  zipWithM_ (\before after -> require (observedStorageAfter before == observedStorageBefore after)
    "storage snapshots change between declared actions") steps (drop 1 steps)
  forM_ steps $ \step -> do
    checkSnapshotRegions (observedStorageNumber step) (observedStorageBefore step) (intervalBefore (observedStorageInterval step))
    checkSnapshotRegions (observedStorageNumber step) (observedStorageAfter step) (observedStorageAfterRegions step)
  replayed <- fmap M.fromList . forM ordered $ \interval -> do
    effects <- replay plan interval
    checkStores plan hits interval
    pure (intervalName interval,effects)
  sessionStates <- forM manifests $ \manifest ->
    pairHits plan bankBoundary hits (manifestHitRange manifest) emptyPairState
  let paired=emptyPairState
        { pairDone=M.unions (map pairDone sessionStates)
        , pairApplyDone=M.unions (map pairApplyDone sessionStates)
        }
  events <- fmap concat . forM manifests $ \manifest ->
    normalizePersistenceLocated contract [(index,hits V.! index) | index <- window (manifestHitRange manifest)]
  applications <- fmap concat . forM manifests $ \manifest -> eeWitnesses hits paired (manifestHitRange manifest)
  handbacks <- fmap concat . forM manifests $ \manifest -> s1Witnesses hits (manifestHitRange manifest)
  copies <- fmap concat . forM manifests $ \manifest -> fmap concat . forM
    [(Storage.PreviousImage,Storage.Capture,Storage.CapturePreviousEntry,Storage.CapturePreviousReturn)
    ,(Storage.PreviousImage,Storage.Restore,Storage.RestorePreviousEntry,Storage.RestorePreviousReturn)
    ,(Storage.AlternateImage,Storage.Capture,Storage.ExchangeAlternateEntry,Storage.ExchangeAlternateReturn)
    ,(Storage.AlternateImage,Storage.Restore,Storage.ExchangeAlternateEntry,Storage.ExchangeAlternateReturn)] $
    \(family,direction,entry,returned) -> do
      calls <- pairedStorageCalls entry returned hits (manifestHitRange manifest)
      pure [witness | call <- calls,Just witness <- [copyWitness family direction call]]
  refreshes <- fmap concat . forM manifests $ \manifest ->
    pairedStorageCalls Storage.CustomRefreshEntry Storage.CustomRefreshReturn hits (manifestHitRange manifest)
  let route name routeClass own = do
        let indices=concatMap (window . intervalHits) own
            effects=[replayed M.! intervalName interval | interval <- own]
            lastRegions=case reverse own of final:_ -> intervalAfter final; [] -> M.empty
            writes=concatMap fst effects
            lastEffects=concat [case hitSite (hits V.! index) of
              PlanSite SetterEntry -> map SetterEffect (take 1 (hitArguments (hits V.! index)))
              PlanSite PropertyCommit -> [CommitEffect]
              _ -> [] | index <- indices]
        getterCalls <- callsIn plan bankBoundary hits paired indices GetterEntry
        setterCalls <- callsIn plan bankBoundary hits paired indices SetterEntry
        forM_ writes $ \write -> unless (effectWriter write /= CarrierBody) (Left (ForeignWriter name (effectPc write)))
        pure (RouteResult name routeClass getterCalls setterCalls
          (concatMap (eventsBetween events . intervalHits) own) writes (concatMap snd effects)
          (M.toList lastRegions) [press | interval <- own,Just press <- [intervalPress interval]] (effectBlockRuns lastEffects))
  phaseResults <- fmap concat . forM (M.toList phasesByBoot) $ \(BootSession boot,phases) ->
    forM phases $ \(phase,interval) -> route (RouteName (printf "storage-boot-%04d-%s" boot (T.unpack (attachPhaseWire phase)))) PhaseClass [interval]
  stepResults <- forM steps $ \step ->
    route (RouteName (printf "storage-step-%05d" (observedStorageNumber step))) StorageClass (storageStepIntervals step)
  proof <- foldM (checkStorageStep contract hits paired events applications handbacks copies refreshes phasesByBoot)
    (MatrixProof M.empty M.empty [] [] []) (zip steps stepResults)
  let profileSteps boot=[(slot,Storage.storageStepAction (observedStorageSpec step)) | step <- steps
        ,observedStorageSession step == boot,Storage.CurrentSlot slot <- Storage.storageStepChecks (observedStorageSpec step)]
      coverage=proofCoverage proof ++ [CustomModeCycle | any (modeCycle . profileSteps) (M.keys phasesByBoot)]
  forM_ [minBound..maxBound] $ \criterion ->
    unless (criterion `elem` coverage) (Left (StorageCoverageMissing criterion))
  cohort <- forM [["inputs","run","sha256"],["inputs","boot_rom","sha256"],["inputs","bridge","sha256"]
    ,["inputs","routes","sha256"],["inputs","linked","sha256"],["inputs","contract","sha256"],["inputs","observer","sha256"]
    ,["inputs","elf","sha256"]] $ \path -> (,) (T.unpack (T.intercalate "." path)) <$> receiptDigest path
  let SdkModel model=identityModel firstIdentity
      seedCohort=[("inputs.initial_nand.kind",T.unpack seedName),("inputs.initial_nand.sha256",expectedSeed)
        ,("sdk.model",T.unpack model),("sdk.procedure",sha256Hex (identityProcedure firstIdentity))]
      getters=[GetterResult (GetterKey (hitLr hit) (hitArguments hit)) (returnValue returned) (returnHit returned)
        | index <- [0..V.length hits-1],let hit=hits V.! index,hitSite hit == PlanSite GetterEntry
        ,Just returned <- [M.lookup index (pairDone paired)]]
      bootEe=[(boot,[witness | LocatedEe _ finish witness <- applications
        ,any (\(_,interval) -> let (from,to)=intervalHits interval in from <= finish && finish < to) phases])
        | (boot,phases) <- M.toList phasesByBoot]
      bootHandbacks=[(boot,[witness | LocatedS1 _ finish witness <- handbacks
        ,any (\(_,interval) -> let (from,to)=intervalHits interval in from <= finish && finish < to) phases])
        | (boot,phases) <- M.toList phasesByBoot]
      chains=[BootChain (coldNextSession cold) (exportedDigest (coldExport cold))
        (coldInputDigest cold)
        | step <- steps,Just cold <- [observedStorageCold step]]
  pure (Qualified role (cohort ++ seedCohort) (V.length hits) (length steps) (phaseResults ++ stepResults) getters
    (Just (StorageEvidence (reverse (proofEvidence proof)) chains bootEe bootHandbacks)))

data ModeCycleStage = AwaitCustomDetent | AwaitLiveDetent | AwaitCustomReturn

modeCycle :: [(Storage.ProfileSlot,Storage.StorageAction)] -> Bool
modeCycle = go AwaitCustomDetent
  where
    go _ [] = False
    go stage ((slot,action):rest) = case (stage,slot,action) of
      (AwaitCustomDetent,Storage.StoredCustom,Storage.SetModeDial ModeDialCustom) -> go AwaitLiveDetent rest
      (AwaitLiveDetent,Storage.LiveProfile,Storage.SetModeDial position) | position /= ModeDialCustom -> go AwaitCustomReturn rest
      (AwaitCustomReturn,Storage.StoredCustom,Storage.SetModeDial ModeDialCustom) -> True
      _ -> go stage rest

checkStorageStep :: Contract -> V.Vector Hit -> PairState -> [(Int,PersistenceEvent)] -> [LocatedEe] -> [LocatedS1] -> [CopyWitness] -> [StorageCall] -> M.Map BootSession [(AttachPhase,Interval)] -> MatrixProof -> (ObservedStorageStep,RouteResult) -> Either Refusal MatrixProof
checkStorageStep contract hits paired events applications handbacks allCopies refreshes phasesByBoot proof (step,result) = do
  let spec=observedStorageSpec step
      name=Storage.storageStepName spec
      (from,to)=observedStorageWholeHits step
      completed start finish=from <= start && start <= finish && finish < to
      localEe=[witness | LocatedEe _ finish witness <- applications,from <= finish && finish < to]
      localCopies=[witness | witness <- allCopies,completed (copyEntryIndex witness) (copyReturnIndex witness)]
      localRefreshes=[call | call <- refreshes,completed (boundaryEntryIndex call) (boundaryReturnIndex call)]
      before=observedStorageBefore step
      after=observedStorageAfter step
      previous=M.elems (proofSteps proof)
      halfInputs=[input | earlier <- previous ++ [step],observedStorageSession earlier == observedStorageSession step
        ,Storage.storageStepAction (observedStorageSpec earlier) == Storage.PressHalfShutter
        ,Just input <- [observedStorageInput earlier]]
      origin=case reverse (sort halfInputs) of first:_ -> Just first; [] -> Nothing
      observedHandbacks=[witness | LocatedS1 _ finish witness <- handbacks,finish >= from && finish < to]
      localHandbacks=[witness | LocatedS1 start finish witness <- handbacks,finish >= from && finish < to
        ,Just input <- [origin],hitInstructions (hits V.! start) >= input]
      reject check condition=unless condition (Left (StorageCheckFailed name check))
      selectedPayload target reference=do
        earlier <- M.lookup (Storage.payloadReferenceStep reference) (proofSteps proof)
        pure (snapshotPayload target after,snapshotPayload (Storage.payloadReferenceTarget reference) (observedStorageAfter earlier))
      bootStart=case reverse (M.findWithDefault [] (observedStorageSession step) phasesByBoot) of
        (_,interval):_ -> snd (intervalHits interval)
        [] -> from
  criteria <- fmap concat . forM (Storage.storageStepChecks spec) $ \check -> case check of
    Storage.CustomCommit change -> do
      let matching=filter (refreshMatches change events) localRefreshes
          persisted=snapshotCustomPrevious after
          finishedRecord call=case refreshImages (boundaryReturn call) of Just (RefreshImages record _ _) -> record == persisted; _ -> False
      reject check (any finishedRecord matching && case change of
        Storage.Changed -> sealedCustomRecord persisted && persisted `elem` confirmedRecords (eventsBetween events (from,to))
        Storage.Unchanged -> snapshotCustomPrevious before == persisted && not (any isPersistentCustom (eventsBetween events (from,to))))
      pure [case change of Storage.Changed -> ChangedCustomSave; Storage.Unchanged -> UnchangedCustomSave]
    Storage.BankCopy family direction -> do
      let matching=[witness | witness <- localCopies,copyFamily witness == family,copyDirection witness == direction]
          canceled restored=case copyBefore restored of
            CopyImages edited alternate _ _ -> any (\captured -> copyFamily captured == Storage.AlternateImage
              && copyDirection captured == Storage.Capture && copyEntryIndex captured >= bootStart && copyReturnIndex captured < copyEntryIndex restored
              && case copyAfter captured of CopyImages original saved _ _ -> saved == alternate && edited /= original) (proofCopies proof)
      reject check (not (null matching))
      pure [AlternateCancellation | family == Storage.AlternateImage,direction == Storage.Restore
        ,Storage.storageStepAction spec == Storage.PressKey KeyBack,any canceled matching]
    Storage.CustomPayload relation target reference -> do
      reject check (case selectedPayload target reference of
        Just (actual,expected) -> (actual == expected) == (relation == Storage.SameAs)
        Nothing -> False)
      pure []
    Storage.CurrentSlot slot -> do
      reject check (profileObserved contract slot after (resultGetters result) (resultSetters result))
      let saved=M.elems (proofCommitted proof)
          recalled=slot == Storage.StoredCustom && any (\record ->
            snapshotPayload Storage.ActiveCustomPayload after == recordPayload record
            && snapshotPayload Storage.BackingCustomPayload after == recordPayload record) saved
      pure [CustomRecall | recalled]
    Storage.UiHandback -> do
      reject check (not (null localHandbacks) && isJust (observedStorageEndpoint step))
      pure [S1ApplicationHandback]
    Storage.RepeatSetting setting -> do
      reject check (repeatedSetting setting hits paired (bootStart,to))
      pure [RepeatedSettingRoundTrips]
    Storage.ColdBootRecord reference -> do
      let valid=do
            cold <- observedStorageCold step
            record <- M.lookup reference (proofCommitted proof)
            let freshEvents=concatMap (eventsBetween events . intervalHits . snd) (coldPhases cold)
            pure (sealedCustomRecord record && snapshotCustomPrevious after == record
              && all (\target -> snapshotPayload target after == recordPayload record)
                [Storage.ActiveCustomPayload,Storage.BackingCustomPayload,Storage.PreviousRecordPayload,Storage.WorkingRecordPayload]
              && any (\event -> case event of CustomRead {} -> True; _ -> False) freshEvents)
      reject check (valid == Just True)
      pure [ColdCommittedReload]
  let commits=if Storage.CustomCommit Storage.Changed `elem` Storage.storageStepChecks spec
        then M.insert name (snapshotCustomPrevious after) (proofCommitted proof) else proofCommitted proof
      evidence=StorageStepEvidence name before after (observedStorageEndpoint step) (Storage.storageStepChecks spec)
        (observedStorageDial step) localEe observedHandbacks
  pure (MatrixProof (M.insert name step (proofSteps proof)) commits (proofCopies proof ++ localCopies)
    (criteria ++ proofCoverage proof) (evidence:proofEvidence proof))

eeWitnesses :: V.Vector Hit -> PairState -> (Int,Int) -> Either Refusal [LocatedEe]
eeWitnesses hits state (from,to) = do
  (open,done) <- foldM visit (M.empty,[]) [from..to-1]
  unless (M.null open) (Left (StorageShape "EE application ends before its final property call returns"))
  pure (reverse done)
  where
    visit (open,done) index =
      let hit=hits V.! index
          key=(hitCore hit,hitTaskId hit,hitSp hit)
      in case storageSite hit of
        Just Storage.EeCachedCall -> do
          unless (M.notMember key open) (Left (StorageShape "EE application overlaps its own frame"))
          pure (M.insert key [(index,hit)] open,done)
        Just site | site >= Storage.EeCachedCall && site <= Storage.EeFinalReturn -> case M.lookup key open of
          Nothing -> Left (StorageShape "EE observation has no enclosing application")
          Just previous -> do
            let observed=(index,hit):previous
            if site /= Storage.EeFinalReturn then pure (M.insert key observed open,done) else do
              witness <- either (Left . StorageShape) Right (checkEe (reverse observed))
              pure (M.delete key open,witness:done)
        _ -> pure (open,done)
    checkEe observed = do
      ((start,first),(finish,_)) <- case observed of
        initial:rest -> Right (initial,foldl (\_ next -> next) initial rest)
        [] -> Left "EE application has no source observations"
      let labels=map (storageSite . snd) observed
          require condition message=unless condition (Left message)
          one site=case [(index,hit) | (index,hit) <- observed,storageSite hit == Just site] of
            [entry] -> Right entry
            _ -> Left "EE source event is absent or repeated"
          arg0 hit=case hitArguments hit of value:_ -> Right value; _ -> Left "EE event lacks r0"
      context <- either (Left . show) Right (executionTask first)
      require (all (\(_,hit) -> sameExecution first hit && hitSp first == hitSp hit) observed) "EE source events change execution frame"
      new <- maybe (Left "EE event lacks actual general-register capture") Right (hitR8 first)
      require (new <= 30 && all ((==Just new) . hitR8 . snd) observed) "EE new stored code leaves its source row domain or changes"
      cached <- arg0 first
      require (cached == 30-new) "EE cached value differs from its source row conversion"
      (_,classHit) <- one Storage.EeFrameClassResult
      frameClass <- arg0 classHit
      frameProperty <- if frameClass == 37 then Just <$> (one Storage.EeFramePropertyResult >>= arg0 . snd) else pure Nothing
      let prefix=[Storage.EeCachedCall,Storage.EeFrameClassCall,Storage.EeFrameClassResult]
            ++ [site | frameClass == 37,site <- [Storage.EeFramePropertyCall,Storage.EeFramePropertyResult]]
          make bank old modeClass mode effects=EeWitness (taskKernel context,taskLogicalId context) (ObservedShiftCode (fromIntegral new))
            (EeFrameClass frameClass) (EeFrameProperty <$> frameProperty) (CachedShiftValue cached)
            (StoredShiftValue <$> old) modeClass (WhiteBalanceMode <$> mode) bank effects
      (writeAt,writeHit) <- one Storage.EeFinalWrite
      require (take 3 (hitArguments writeHit) == [0xee,new,0]) "EE final write does not preserve the observed new code"
      roots <- either (Left . show) Right (pairedStorageCalls Storage.BankApplyEntry Storage.BankApplyReturn hits (writeAt+1,finish))
      root <- case roots of
        [call] | sameExecution first (boundaryEntry call) && sameExecution first (boundaryReturn call)
          && take 3 (hitArguments (boundaryEntry call)) == [0xee,new,0]
          && physicalAddress (hitLr (boundaryEntry call)) == 0x61c928 -> Right call
        _ -> Left "EE final write lacks its completed bank-apply caller"
      let setters=[returned | index <- [boundaryEntryIndex root+1..boundaryReturnIndex root-1],
            let hit=hits V.! index,hitSite hit == PlanSite SetterEntry,
            sameExecution first hit,take 3 (hitArguments hit) == [0xee,new,0],
            M.lookup index (pairApplyDone state) == Just (BankApplyCaller (taskKernel context) (taskLogicalId context)),
            Just returned <- [M.lookup index (pairDone state)],returnHit returned < boundaryReturnIndex root]
      require (case setters of [returned] -> returnValue returned == 1; _ -> False)
        "EE final write lacks its completed assigned property setter"
      if frameClass == 37 && frameProperty == Just 0xee then do
        require (labels == map Just (prefix ++ [Storage.EeFinalWrite,Storage.EeFinalReturn])) "excluded EE editor bypass has extra effects"
        pure (LocatedEe start finish (make Nothing Nothing Nothing Nothing []))
      else do
        (oldCall,requested) <- one Storage.EeOldValueCall
        (compareAt,compared) <- one Storage.EeCompare
        (_,conditional) <- one Storage.EeConditional
        require (take 2 (hitArguments requested) == [0xee,0]) "EE old-value request differs"
        old <- arg0 compared
        (bank,modeClass,mode,cachedAfter) <- case hitMemory compared of
          [bank,modeBytes,wbBytes,cacheBytes] | map B.length [bank,modeBytes,wbBytes,cacheBytes] == [0x1c2,4,4,4] ->
            Right (StorageBankBytes bank,word32At modeBytes 0,word32At wbBytes 0,word32At cacheBytes 0)
          _ -> Left "EE compare lacks full bank, class, mode, and cached-value snapshots"
        require (cachedAfter == cached) "EE cached hook did not publish its actual value"
        let selector=if modeClass == 3 then 0 else 1
            oldGetters=[returned | index <- [oldCall+1..compareAt-1],let hit=hits V.! index,
              hitSite hit == PlanSite GetterEntry,hitArguments hit == [0xee,0,0,selector],
              physicalAddress (hitLr hit) == 0x624648,sameExecution first hit,
              Just returned <- [M.lookup index (pairDone state)],returnHit returned < compareAt]
        require (case oldGetters of [returned] -> returnValue returned == old; _ -> False) "EE compare does not use its actual completed old-value getter"
        let equal=old == new
        require ((hitFlags conditional .&. 0x40000000 /= 0) == equal) "EE conditional flags disagree with old and new values"
        effects <- if equal then pure [] else do
          let resets=
                [ (Storage.EeResetZeroLive,WbZero,LiveWbReset,0,0)
                , (Storage.EeResetEightLive,WbEight,LiveWbReset,8,0)
                , (Storage.EeResetNineLive,WbNine,LiveWbReset,9,0)
                , (Storage.EeResetZeroCustom,WbZero,CustomWbReset,0,1)
                , (Storage.EeResetEightCustom,WbEight,CustomWbReset,8,1)
                , (Storage.EeResetNineCustom,WbNine,CustomWbReset,9,1)
                ]
          resetEffects <- forM resets $ \(site,channel,scope,index,record) -> do
            (_,hit) <- one site
            require (take 3 (hitArguments hit) == [0,index,record]) "EE reset arguments differ from their source call"
            pure (ResetWhiteBalance channel scope)
          (_,firstApply) <- one Storage.EeApplyFirst
          (_,secondApply) <- one Storage.EeApplySecond
          require (take 2 (hitArguments firstApply) == [0,mode] && take 2 (hitArguments secondApply) == [0,mode])
            "EE device application does not use its actual white-balance mode"
          pure (resetEffects ++ [ApplyWhiteBalanceFirst (WhiteBalanceMode mode),ApplyWhiteBalanceSecond (WhiteBalanceMode mode)])
        let effectSites=if equal then [] else
              [Storage.EeResetZeroLive,Storage.EeResetEightLive,Storage.EeResetNineLive,Storage.EeResetZeroCustom
              ,Storage.EeResetEightCustom,Storage.EeResetNineCustom,Storage.EeApplyFirst,Storage.EeApplySecond]
            expected=prefix ++ [Storage.EeOldValueCall,Storage.EeCompare,Storage.EeConditional] ++ effectSites ++ [Storage.EeFinalWrite,Storage.EeFinalReturn]
        require (labels == map Just expected) "EE conditional effects are omitted, repeated, or reordered"
        pure (LocatedEe start finish (make (Just bank) (Just old) (Just (ModeClassWord modeClass)) (Just mode) effects))

newtype S1FrameBytes = S1FrameBytes B.ByteString deriving (Eq, Show)
newtype S1QueueSlot = S1QueueSlot Word16 deriving (Eq, Show)
newtype S1StackOffset = S1StackOffset Integer
newtype S1ControllerState = S1ControllerState Word16 deriving (Eq, Show)
data S1Continuation = S1MinusOne | S1Two | S1FiveD | S1FiveE | S1D9 deriving (Eq, Show)

data S1Witness = S1Witness
  { s1ProducerIdentity :: (TraceCore,LogicalTaskId)
  , s1ConsumerIdentity :: (TraceCore,LogicalTaskId)
  , s1EnteredFrame :: S1FrameBytes
  , s1ReplayedFrame :: S1FrameBytes
  , s1ConsumedFrame :: S1FrameBytes
  , s1DispatchContinuation :: S1Continuation
  , s1ControllerState :: S1ControllerState
  } deriving (Eq, Show)

data LocatedS1 = LocatedS1 Int Int S1Witness
data S1Point = S1Point { s1PointIndex :: Int, s1PointHit :: Hit }
newtype S1Sites = S1Sites (M.Map Storage.LifecycleSite [S1Point])
data S1Producer = S1Producer
  { s1ProducerCall :: StorageCall
  , s1ProducerCopy :: S1Point
  , s1ProducerCopied :: S1Point
  , s1ProducerSlot :: S1QueueSlot
  , s1ProducerEntered :: S1FrameBytes
  , s1ProducerReplayed :: S1FrameBytes
  , s1ProducerContinuation :: S1Continuation
  }
data S1Application = S1Application S1Point S1ControllerState
data S1Consumer = S1Consumer StorageCall S1Point S1QueueSlot S1FrameBytes [S1Application]

s1Require :: Bool -> String -> Either Refusal ()
s1Require condition message = unless condition (Left (StorageShape ("S1 " ++ message)))

s1Arguments :: [Word32] -> Hit -> Either Refusal ()
s1Arguments expected hit = s1Require (take (length expected) (hitArguments hit) == expected) "source call arguments differ"

s1Argument :: Int -> Hit -> Either Refusal Word32
s1Argument index hit = case drop index (hitArguments hit) of
  value:_ -> Right value
  [] -> Left (StorageShape "S1 source event lacks its captured argument")

s1Frame :: B.ByteString -> Either Refusal S1FrameBytes
s1Frame bytes = do
  s1Require (B.length bytes == 0x50) "frame snapshot is not 80 bytes"
  pure (S1FrameBytes bytes)

s1SingleFrame :: Hit -> Either Refusal S1FrameBytes
s1SingleFrame hit = case hitMemory hit of
  [bytes] -> s1Frame bytes
  _ -> Left (StorageShape "S1 source event lacks its exact frame read")

s1IsPress :: S1FrameBytes -> Bool
s1IsPress (S1FrameBytes bytes) = word32At bytes 0 == 7 && word32At bytes 8 .&. 0xffff == 0

s1Half :: B.ByteString -> Either Refusal Word16
s1Half bytes = case B.unpack bytes of
  [lo,hi] -> Right (fromIntegral lo + fromIntegral hi * 0x100)
  _ -> Left (StorageShape "S1 halfword snapshot has another width")

s1QueueIndex :: B.ByteString -> Either Refusal S1QueueSlot
s1QueueIndex bytes = do
  index <- s1Half bytes
  s1Require (index < 256) "queue index leaves its admitted 256-slot domain"
  pure (S1QueueSlot index)

s1SlotAddress :: S1QueueSlot -> Word32
s1SlotAddress (S1QueueSlot index) = 0x949dd8 + fromIntegral index * 0x50

s1SlotAt :: Word32 -> Either Refusal S1QueueSlot
s1SlotAt address = do
  let physical = physicalAddress address
  s1Require (physical >= 0x949dd8 && physical < 0x94edd8 && (physical-0x949dd8) `mod` 0x50 == 0) "copy does not address a whole queue slot"
  pure (S1QueueSlot (fromIntegral ((physical-0x949dd8) `div` 0x50)))

s1Actor :: Hit -> Either Refusal (TraceCore,LogicalTaskId)
s1Actor hit = do
  context <- either (Left . StorageShape . ("S1 execution context: " ++) . show) Right (executionTask hit)
  pure (taskKernel context,taskLogicalId context)

s1Points :: S1Sites -> Storage.LifecycleSite -> [S1Point]
s1Points (S1Sites sites) site = M.findWithDefault [] site sites

s1One :: S1Sites -> Hit -> (Int,Int) -> Storage.LifecycleSite -> S1StackOffset -> Either Refusal S1Point
s1One sites root (from,to) site (S1StackOffset offset) =
  case [point | point <- s1Points sites site,from <= s1PointIndex point,s1PointIndex point < to
    ,let hit=s1PointHit point,sameExecution root hit
    ,toInteger (physicalAddress (hitSp hit)) == toInteger (physicalAddress (hitSp root))+offset] of
    [point] -> Right point
    _ -> Left (StorageShape ("S1 source event is absent or repeated in its execution frame: " ++ Storage.lifecycleSiteWire site))

s1Ordered :: [S1Point] -> Either Refusal ()
s1Ordered points = s1Require (and (zipWith (<) indices (drop 1 indices))) "source events are reordered"
 where indices = map s1PointIndex points

s1NextExecution :: V.Vector Hit -> Hit -> (Int,Int) -> Maybe S1Point
s1NextExecution hits root (from,to) = case
  [S1Point index hit | index <- [from..to-1],let hit=hits V.! index,sameExecution root hit] of
    point:_ -> Just point
    [] -> Nothing

s1Continuation :: Word32 -> Maybe S1Continuation
s1Continuation value = case value of
  0xffffffff -> Just S1MinusOne
  2 -> Just S1Two
  0x5d -> Just S1FiveD
  0x5e -> Just S1FiveE
  0xd9 -> Just S1D9
  _ -> Nothing

s1ContinuationWord :: S1Continuation -> Word32
s1ContinuationWord continuation = case continuation of
  S1MinusOne -> 0xffffffff
  S1Two -> 2
  S1FiveD -> 0x5d
  S1FiveE -> 0x5e
  S1D9 -> 0xd9

s1ReadProducer :: V.Vector Hit -> S1Sites -> StorageCall -> Either Refusal (Maybe S1Producer)
s1ReadProducer hits sites call = do
  let first=boundaryEntry call
      final=boundaryReturn call
      range=(boundaryEntryIndex call+1,boundaryReturnIndex call)
      inner site=s1One sites first range site (S1StackOffset (-16))
      copying site=s1One sites first range site (S1StackOffset (-64))
  entered <- s1SingleFrame first
  if not (s1IsPress entered) then pure Nothing else do
    _ <- s1Actor first
    s1Require (sameExecution first final && physicalAddress (hitSp first) == physicalAddress (hitSp final)
      && physicalAddress (hitLr first) == 0xbfb0cc) "action 161 lacks its source callback return"
    guardCall <- inner Storage.S1GuardCall
    guardResult <- inner Storage.S1GuardResult
    s1Arguments [0x53] (s1PointHit guardCall)
    s1Ordered [guardCall,guardResult]
    guarded <- s1Argument 0 (s1PointHit guardResult)
    if guarded /= 0 then pure Nothing else do
      request <- inner Storage.S1RequestCall
      dispatch <- inner Storage.S1DispatchCall
      dispatched <- inner Storage.S1DispatchResult
      s1Arguments [2,0x1d] (s1PointHit request)
      s1Ordered [guardResult,request,dispatch,dispatched]
      result <- s1Argument 0 (s1PointHit dispatched)
      case s1Continuation result of
        Nothing -> pure Nothing
        Just continuation -> do
          flagCall <- inner Storage.S1FlagCall
          flagReturn <- inner Storage.S1FlagReturn
          replayCall <- inner Storage.S1ReplayCall
          copy <- copying Storage.S1FrameCopyCall
          copied <- copying Storage.S1FrameCopyReturn
          replayed <- inner Storage.S1ReplayReturn
          returning <- inner Storage.S1ActionReturnInstruction
          s1Ordered [dispatched,flagCall,flagReturn,replayCall,copy,copied,replayed,returning]
          s1Arguments [1] (s1PointHit flagCall)
          s1Require (case hitMemory (s1PointHit flagReturn) of [bytes] -> B.length bytes == 4 && word32At bytes 0 == 1; _ -> False)
            "flag setter did not publish one"
          s1Arguments [0] (s1PointHit replayCall)
          (frame,slot) <- case hitMemory (s1PointHit copy) of
            [bytes,tailBytes] -> (,) <$> s1Frame bytes <*> s1QueueIndex tailBytes
            _ -> Left (StorageShape "S1 replay lacks its complete source frame and queue-tail read")
          destination <- s1Argument 0 (s1PointHit copy)
          source <- s1Argument 1 (s1PointHit copy)
          count <- s1Argument 2 (s1PointHit copy)
          s1Require (physicalAddress destination == s1SlotAddress slot && physicalAddress source == 0x944580 && count == 0x50)
            "replay does not copy the complete source frame into its selected slot"
          advanced <- s1Argument 0 (s1PointHit copied)
          s1Require (physicalAddress advanced == s1SlotAddress slot+0x50) "replay copy lacks the retained callee's advanced destination return"
          s1Arguments [s1ContinuationWord continuation] (s1PointHit returning)
          s1Arguments [s1ContinuationWord continuation] final
          s1Require (case s1NextExecution hits first (s1PointIndex returning+1,boundaryReturnIndex call+1) of
            Just next -> s1PointIndex next == boundaryReturnIndex call
            Nothing -> False) "shared callback return is not the action 161 continuation"
          pure (Just (S1Producer call copy copied slot entered frame continuation))

s1ReadApplications :: S1Sites -> Hit -> (Int,Int) -> S1FrameBytes -> Either Refusal [S1Application]
s1ReadApplications sites consumer (from,to) frame = fmap concat . forM entries $ \entry -> do
  let first=s1PointHit entry
      nextEntries=[s1PointIndex point | point <- entries,s1PointIndex point > s1PointIndex entry]
      end=minimum (to:nextEntries)
      range=(s1PointIndex entry+1,end)
      inner site=s1One sites first range site (S1StackOffset (-16))
      router site=s1One sites first range site (S1StackOffset (-32))
      available site=any (\point -> let hit=s1PointHit point in s1PointIndex entry < s1PointIndex point
        && s1PointIndex point < end && sameExecution first hit) (s1Points sites site)
  s1Require (physicalAddress (hitLr first) == 0xbfb0cc) "application action lacks its retained controller callback"
  if not (available Storage.S1ApplicationDispatch) then pure [] else do
    guardCall <- inner Storage.S1ApplicationGuardCall
    guardResult <- inner Storage.S1ApplicationGuardResult
    guarded <- s1Argument 0 (s1PointHit guardResult)
    s1Require (guarded /= 0) "application guard rejects the replay"
    routed <- inner Storage.S1ApplicationRouterCall
    key <- router Storage.S1ApplicationKeyTest
    qualifier <- router Storage.S1ApplicationQualifierTest
    stored <- router Storage.S1ApplicationEventStore
    dispatched <- router Storage.S1ApplicationDispatch
    s1Ordered [entry,guardCall,guardResult,routed,key,qualifier,stored,dispatched]
    case hitMemory (s1PointHit routed) of
      [bytes,stateBytes] -> do
        actual <- s1Frame bytes
        state <- s1Half stateBytes
        s1Require (actual == frame && state == 3) "replay does not enter the source M0 state-three application route"
      _ -> Left (StorageShape "S1 application route lacks its frame and M0 state")
    s1Argument 3 (s1PointHit key) >>= \value -> s1Require (value == 7) "application key test does not select S1"
    s1Argument 3 (s1PointHit qualifier) >>= \value -> s1Require (value == 0) "application qualifier does not select press"
    value <- s1Argument 2 (s1PointHit stored)
    target <- s1Argument 3 (s1PointHit stored)
    s1Require (value == 0 && physicalAddress target == 0x14676e4) "application does not store the source M74 press event"
    s1Arguments [74] (s1PointHit dispatched)
    state <- case hitMemory (s1PointHit dispatched) of
      [bytes,stateBytes,eventBytes] -> do
        actual <- s1Frame bytes
        current <- s1Half stateBytes
        event <- s1Half eventBytes
        s1Require (actual == frame && current < 38 && event == 0) "application dispatch lacks its consumed frame or admitted M74 state/event"
        pure (S1ControllerState current)
      _ -> Left (StorageShape "S1 application dispatch lacks its complete frame, state, and event reads")
    pure [S1Application dispatched state]
 where
  entries=[point | point <- s1Points sites Storage.S1ApplicationActionEntry
    ,from <= s1PointIndex point,s1PointIndex point < to,sameExecution consumer (s1PointHit point)]

s1ReadConsumer :: V.Vector Hit -> S1Sites -> Int -> StorageCall -> Either Refusal (Maybe S1Consumer)
s1ReadConsumer hits sites limit call = do
  status <- s1Argument 0 (boundaryReturn call)
  if status /= 1 then pure Nothing else do
    let first=boundaryEntry call
        final=boundaryReturn call
        range=(boundaryEntryIndex call+1,boundaryReturnIndex call)
        copying site=s1One sites first range site (S1StackOffset (-80))
    _ <- s1Actor first
    s1Require (sameExecution first final && physicalAddress (hitSp first) == physicalAddress (hitSp final)
      && physicalAddress (hitLr first) == 0xd3ab20) "positive dequeue lacks its retained wrapper return"
    source <- s1Argument 0 first
    queue <- s1Argument 1 first
    s1Require (physicalAddress source == 0x944580 && physicalAddress queue == 0x949dd8) "dequeue does not select the retained replay queue and frame"
    case hitMemory first of
      [bytes] | B.length bytes == 4 -> do
        headIndex <- s1QueueIndex (B.take 2 bytes)
        tailIndex <- s1QueueIndex (B.drop 2 bytes)
        s1Require (headIndex /= tailIndex) "positive dequeue enters with an empty queue"
      _ -> Left (StorageShape "S1 dequeue lacks its queue head/tail snapshot")
    copy <- copying Storage.S1DequeueCopyCall
    copied <- copying Storage.S1DequeueCopyReturn
    succeeded <- copying Storage.S1DequeueSuccess
    returning <- s1One sites first range Storage.S1DequeueReturnInstruction (S1StackOffset (-12))
    s1Ordered [copy,copied,succeeded,returning]
    destination <- s1Argument 0 (s1PointHit copy)
    selected <- s1Argument 1 (s1PointHit copy)
    count <- s1Argument 2 (s1PointHit copy)
    slot <- s1SlotAt selected
    s1Require (physicalAddress destination == 0x944580 && count == 0x50) "dequeue does not copy the full selected slot into the retained frame"
    advanced <- s1Argument 0 (s1PointHit copied)
    s1Require (physicalAddress advanced == 0x9445d0) "dequeue copy lacks the retained callee's advanced destination return"
    frame <- s1SingleFrame (s1PointHit copied)
    s1Arguments [1] (s1PointHit returning)
    returned <- case s1NextExecution hits first (boundaryReturnIndex call+1,limit) of
      Just point | storageSite (s1PointHit point) == Just Storage.S1DequeueReturned -> Right point
      _ -> Left (StorageShape "S1 positive dequeue lacks its direct application caller return")
    s1Require (toInteger (physicalAddress (hitSp (s1PointHit returned))) == toInteger (physicalAddress (hitSp first))+8)
      "dequeue caller return changes its source stack frame"
    s1Arguments [1] (s1PointHit returned)
    if not (s1IsPress frame) then pure Nothing else do
      let nextEntries=[s1PointIndex point | point <- s1Points sites Storage.S1DequeueEntry
            ,s1PointIndex point > s1PointIndex returned,sameExecution first (s1PointHit point)]
          end=minimum (limit:nextEntries)
      applications <- s1ReadApplications sites first (s1PointIndex returned+1,end) frame
      pure (Just (S1Consumer call copy slot frame applications))

s1Overwrites :: S1QueueSlot -> S1Point -> Bool
s1Overwrites slot point = case hitArguments (s1PointHit point) of
  destination:_:count:_ ->
    let first=toInteger (physicalAddress destination)
        lastByte=first+toInteger count
        saved=toInteger (s1SlotAddress slot)
    in count /= 0 && first < saved+0x50 && saved < lastByte
  _ -> False

s1Witnesses :: V.Vector Hit -> (Int,Int) -> Either Refusal [LocatedS1]
s1Witnesses hits (from,to) = do
  s1Require (0 <= from && from <= to && to <= V.length hits) "session hit range is invalid"
  let sites=S1Sites (M.map reverse (V.ifoldl' insert M.empty (V.slice from (to-from) hits)))
      insert table index hit=case storageSite hit of
        Just site -> M.insertWith (++) site [S1Point (from+index) hit] table
        Nothing -> table
  producerCalls <- pairedStorageCalls Storage.S1ActionEntry Storage.S1ActionReturned hits (from,to)
  producerResults <- mapM (s1ReadProducer hits sites) producerCalls
  consumerCalls <- pairedStorageCalls Storage.S1DequeueEntry Storage.S1DequeueWrapperReturn hits (from,to)
  consumerResults <- mapM (s1ReadConsumer hits sites to) consumerCalls
  let producers=[producer | Just producer <- producerResults]
      consumers=[consumer | Just consumer <- consumerResults]
  fmap concat . forM consumers $ \(S1Consumer call copy slot frame applications) -> do
    let matches=[producer | producer <- producers,s1ProducerSlot producer == slot,s1ProducerReplayed producer == frame
          ,s1PointIndex (s1ProducerCopied producer) < boundaryEntryIndex call
          ,not (any (\point -> s1PointIndex (s1ProducerCopy producer) < s1PointIndex point
            && s1PointIndex point < s1PointIndex copy && s1Overwrites slot point) (s1Points sites Storage.S1FrameCopyCall))]
    case matches of
      [] -> pure []
      [producer] -> do
        producerIdentity <- s1Actor (boundaryEntry (s1ProducerCall producer))
        consumerIdentity <- s1Actor (boundaryEntry call)
        pure [LocatedS1 (boundaryEntryIndex (s1ProducerCall producer))
          (max (boundaryReturnIndex (s1ProducerCall producer)) (s1PointIndex dispatched))
          (S1Witness producerIdentity consumerIdentity (s1ProducerEntered producer) (s1ProducerReplayed producer)
            frame (s1ProducerContinuation producer) state) | S1Application dispatched state <- applications]
      _ -> Left (StorageShape "S1 consumed queue slot has ambiguous producer lineage")

s1WitnessValue :: S1Witness -> Value
s1WitnessValue witness = object
  [ "producer" .= taskIdentityValue (s1ProducerIdentity witness)
  , "consumer" .= taskIdentityValue (s1ConsumerIdentity witness)
  , "entered_frame" .= frameValue (s1EnteredFrame witness)
  , "replayed_frame" .= frameValue (s1ReplayedFrame witness)
  , "consumed_frame" .= frameValue (s1ConsumedFrame witness)
  , "continuation" .= s1ContinuationWord (s1DispatchContinuation witness)
  , "m74_state" .= (case s1ControllerState witness of S1ControllerState state -> state)
  ]
 where frameValue (S1FrameBytes bytes) = C.unpack (hexEncode bytes)
