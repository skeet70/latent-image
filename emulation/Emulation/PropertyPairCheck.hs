{-# LANGUAGE OverloadedStrings #-}
-- | The property-service natural pair's consumer. It admits one role's observation from its raw
-- trace hits, watch rows and region bytes, and compares two admitted roles route by route.
module Emulation.PropertyPairCheck
  ( Observation (..)
  , Bindings (..)
  , IntervalName (..)
  , Refusal (..)
  , Call (..)
  , WriteEffect (..)
  , SettleStatus (..)
  , Press (..)
  , RouteClass (..)
  , RouteResult (..)
  , Qualified (..)
  , qualify
  , compareRoles
  , qualifiedValue
  , comparisonValue
  , ControlName (..)
  , controlNames
  , checkGuestControl
  , ControlOutcome (..)
  , runControl
  ) where

import Control.Monad (foldM, forM, forM_, unless, when, zipWithM_)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (parseEither)
import Data.Bits (complement, shiftL, shiftR, xor, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (find, sort)
import qualified Data.List.NonEmpty as NE
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isNothing)
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word64, Word8)
import Emulation.PropertyPairPlan
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import EmulatorUsb (CrawlKey)
import Numeric (readHex, showHex)

-- | The sidecars of one observation directory; the bridge session receipt only by digest.
data Observation = Observation
  { obsReceipt :: B.ByteString
  , obsSpec :: B.ByteString
  , obsReset :: B.ByteString
  , obsHits :: B.ByteString
  , obsSteps :: B.ByteString
  , obsSessionDigest :: String
  }

-- | The bound inputs: canonical and role DATs and the observer by digest, the rest by bytes.
data Bindings = Bindings
  { bindSourceDat :: String
  , bindOwnedDat :: String
  , bindRoutes :: B.ByteString
  , bindContract :: B.ByteString
  , bindLinked :: B.ByteString
  , bindSymbols :: String
  , bindImages :: M.Map ImageName B.ByteString
  , bindObserver :: String
  }

-- | A reset segment, or a step by its 1-based number.
data IntervalName = PhaseInterval AttachPhase | StepInterval Int deriving (Eq, Ord, Show)

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
  | AccessorStoreBound Int
  | AccessorStoreUnmatched IntervalName
  | RouteUnobserved RouteName
  | ForeignWriter RouteName Word32
  | RouteEffectsDiffer RouteName
  | DeclaredRouteInert RouteName
  | CohortDiffers String
  | RouteSetDiffers
  | GetterResultsDiffer RouteName
  | SetterSequenceDiffers RouteName
  | WriteSequenceDiffers RouteName
  | TransferSequenceDiffers RouteName
  | AppendSequenceDiffers RouteName
  | FinalBytesDiffer RouteName
  | PressDiffers RouteName
  deriving (Eq, Show)

-- | One getter or setter entry: r0-r3, the caller's return address and r0 at that return, absent
-- when the route ends first.
data Call = Call { callArguments :: [Word32], callReturn :: Word32, callResult :: Maybe Word32 } deriving (Eq, Show)

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

data Press = Press { pressStatus :: SettleStatus, pressMenu :: Value } deriving (Eq, Show)

data RouteClass = PhaseClass | CrawlClass | DeclaredClass deriving (Eq, Ord, Show, Enum, Bounded)

routeClassWire :: RouteClass -> T.Text
routeClassWire routeClass = case routeClass of
  PhaseClass -> "phase"
  CrawlClass -> "crawl"
  DeclaredClass -> "declared"

data RouteResult = RouteResult
  { resultRoute :: RouteName
  , resultClass :: RouteClass
  , resultGetters :: [Call]
  , resultSetters :: [Call]
  , resultAppends :: [[Word32]]
  , resultWrites :: [WriteEffect]
  , resultTransfers :: [TransferEffect]
  , resultFinal :: [(Word32, B.ByteString)]
  , resultPresses :: [Press]
  } deriving (Eq, Show)

data Qualified = Qualified
  { qualifiedRole :: Role
  , qualifiedCohort :: [(String, String)]
  , qualifiedHits :: Int
  , qualifiedSteps :: Int
  , qualifiedRoutes :: [RouteResult]
  }

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

data Hit = Hit
  { hitSite :: HitSite
  , hitAddress :: Word32
  , hitArguments :: [Word32]
  , hitSp :: Word32
  , hitLr :: Word32
  }

parseHit :: Value -> Parse Hit
parseHit value = do
  fields <- asObject value
  exact ["site", "address", "core", "instructions", "registers", "sp", "lr", "cpsr"] fields
  registers <- at "registers" fields >>= asObject
  exact ["r0", "r1", "r2", "r3"] registers
  arguments <- mapM (\name -> at name registers >>= hexWordText) ["r0", "r1", "r2", "r3"]
  core <- at "core" fields >>= asInt
  unless (core == 0 || core == 1) (Left "core is neither 0 nor 1")
  _ <- at "instructions" fields >>= asWord64
  _ <- at "cpsr" fields >>= asWord
  Hit <$> (at "site" fields >>= asText >>= parseHitSite) <*> (at "address" fields >>= hexWordText) <*> pure arguments
    <*> (at "sp" fields >>= hexWordText) <*> (at "lr" fields >>= hexWordText)

type Regions = M.Map Word32 B.ByteString

parseRegions :: Value -> Parse Regions
parseRegions value = do
  entries <- asArray value
  regions <- fmap M.fromList . forM entries $ \entry -> do
    fields <- asObject entry
    exact ["start", "bytes"] fields
    (,) <$> (at "start" fields >>= asWord) <*> (at "bytes" fields >>= asText >>= hexBytes)
  unless ([(start, B.length bytes) | (start, bytes) <- M.toList regions]
          == [(rangeStart range, fromIntegral (rangeEnd range - rangeStart range)) | range <- settingsRanges])
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

-- | Each accessor store hit of the interval, in order, must be bounded by its settings range and be
-- the CPU write row the accessor's store instruction made; every row from a store instruction must
-- have its hit.
checkStores :: Plan -> V.Vector Hit -> Interval -> Either Refusal ()
checkStores plan hits interval = do
  let (from, to) = intervalHits interval
      stores = [(index, accessor, hit) | index <- [from .. to - 1], let hit = hits V.! index, PlanSite (AccessorStore accessor) <- [hitSite hit]]
      physical address = address .&. complement 0x40000000
  expected <- forM stores $ \(index, accessor, hit) -> do
    let range = storeRange accessor
        width = storeWidth accessor
    (offset, value) <- case hitArguments hit of
      r0 : r1 : _ -> Right (r0, r1)
      _ -> Left (AccessorStoreBound index)
    unless (toInteger offset + toInteger width <= toInteger (rangeEnd range - rangeStart range)) (Left (AccessorStoreBound index))
    pure (physical (hitAddress hit), rangeStart range + offset, width, Just (fromIntegral value .&. ((1 `shiftL` (8 * width)) - 1)))
  let rows = [ (physical pc, address, width, value')
             | CpuWriteRow address width value' _ _ pc <- intervalRows interval, M.member (physical pc) (planStores plan) ]
  unless (expected == rows) (Left (AccessorStoreUnmatched (intervalName interval)))

-- Call pairing.

data PairState = PairState { pairOpen :: M.Map (Word32, Word32) (SiteLabel, Int, [Word32]), pairDone :: M.Map Int (Maybe Word32) }

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

pairHits :: Plan -> V.Vector Hit -> (Int, Int) -> PairState -> Either Refusal PairState
pairHits plan hits (from, to) initial = foldM visit initial [from .. to - 1]
 where
  visit state index = do
    let hit = hits V.! index
    kind <- classifyHit plan index hit
    case kind of
      GuardHit -> Left (GuardedFetch index)
      EntryHit returnLabel -> do
        unless (M.lookup (hitLr hit) (planSites plan) == Just returnLabel) (Left (UnarmedReturn index))
        let key = (hitSp hit, hitLr hit)
        when (M.member key (pairOpen state)) (Left (DuplicateOpenCall index))
        pure state { pairOpen = M.insert key (returnLabel, index, hitArguments hit) (pairOpen state) }
      ReturnHit label -> case M.lookup (hitSp hit, hitAddress hit) (pairOpen state) of
        Just (expected, entry, _) | expected == label ->
          pure state { pairOpen = M.delete (hitSp hit, hitAddress hit) (pairOpen state)
                     , pairDone = M.insert entry (Just (sum (take 1 (hitArguments hit)))) (pairDone state) }
        _ -> pure state
      OtherHit -> pure state

-- Qualification.

field :: T.Text -> Value -> Parse Value
field name value = asObject value >>= at name

digestAt :: [T.Text] -> Value -> Parse String
digestAt path value = T.unpack <$> (foldM (flip field) value path >>= asText)

digestOf :: B.ByteString -> String
digestOf = sha256Hex . sha256

decodeValue :: B.ByteString -> Parse Value
decodeValue = A.eitherDecodeStrict'

qualify :: Bindings -> Role -> Observation -> Either Refusal Qualified
qualify bindings role observation = do
  receipt <- either (Left . ReceiptShape) Right (decodeValue (obsReceipt observation))
  let receiptDigest path = either (Left . ReceiptShape) Right (digestAt path receipt)
      require refusal condition = unless condition (Left refusal)
  contract <- either (Left . InputDigest . ("contract: " ++)) Right (decodeContract (bindContract bindings))
  require (InputDigest "source DAT") (bindSourceDat bindings == contractSourceDat contract)
  require (InputDigest "owned DAT") (bindOwnedDat bindings == contractCandidateDat contract)
  require (InputDigest "linked") (digestOf (bindLinked bindings) == contractLinkedSha256 contract)
  require (InputDigest "symbols") (bindSymbols bindings == contractSymbolsSha256 contract)
  require (ReceiptShape "role") (either (const False) (== String (T.pack (roleWire role))) (field "role" receipt))
  require (InputDigest "role") (role /= ControlRole)
  datDigest <- receiptDigest ["inputs", "dat", "sha256"]
  require (InputDigest "DAT") (datDigest == (if role == RetainedRole then bindSourceDat bindings else bindOwnedDat bindings))
  forM_ [ (["inputs", "routes", "sha256"], digestOf (bindRoutes bindings), "routes")
        , (["inputs", "contract", "sha256"], digestOf (bindContract bindings), "contract")
        , (["inputs", "linked", "sha256"], digestOf (bindLinked bindings), "linked")
        , (["inputs", "symbols", "sha256"], bindSymbols bindings, "symbols")
        , (["inputs", "observer", "sha256"], bindObserver bindings, "observer") ] $ \(path, expected, name) -> do
    recorded <- receiptDigest path
    require (InputDigest name) (recorded == expected)
  forM_ (M.toList (bindImages bindings)) $ \(name, bytes) -> do
    recorded <- receiptDigest ["inputs", "analysis", imageNameWire name]
    require (InputDigest (T.unpack (imageNameWire name))) (recorded == digestOf bytes)
  forM_ [ ("spec.json", obsSpec observation), ("reset-observation.json", obsReset observation)
        , ("hits.jsonl", obsHits observation), ("steps.json", obsSteps observation) ] $ \(name, bytes) -> do
    recorded <- receiptDigest ["files", name]
    require (SidecarDigest (T.unpack name)) (recorded == digestOf bytes)
  sessionDigest <- receiptDigest ["files", "emulator-session.json"]
  require (SidecarDigest "emulator-session.json") (sessionDigest == obsSessionDigest observation)
  outcome <- either (Left . ReceiptShape) Right (field "outcome" receipt)
  require (ObservationRefused outcome) (either (const False) (== String "completed") (field "status" outcome))
  bounds <- either (Left . ReceiptShape) Right (field "bounds" receipt)
  require (ReceiptShape "bounds") (bounds == object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit])
  require (ReceiptShape "probe") (either (const False) (== Null) (field "probe" receipt))
  require (ReceiptShape "pre-key idle") (either (const False) (== A.toJSON (instructionsValue preKeyIdle)) (field "pre_key_idle_instructions" receipt))
  routes <- either (Left . InputDigest . ("routes: " ++)) Right (decodeRouteFile (bindRoutes bindings))
  let inputs = PlanInputs role contract (bindImages bindings) (bindLinked bindings) Nothing
  plan <- either (Left . InputDigest) Right (derivePlan inputs)
  roleImage <- either (Left . InputDigest) Right (roleImage4 inputs)
  recordedImage <- receiptDigest ["inputs", "role_image4_sha256"]
  require (InputDigest "role image4") (recordedImage == digestOf roleImage)
  spec <- either (Left . ReceiptShape . ("spec: " ++)) Right (decodeValue (obsSpec observation))
  require SiteMapDiffers (spec == specValue plan)
  hits <- fmap V.fromList . forM (zip [0 ..] (filter (not . B.null) (C.lines (obsHits observation)))) $ \(index, line) ->
    either (Left . HitShape index) Right (decodeValue line >>= parseHit)
  require (HitBound (V.length hits)) (V.length hits <= hitLimit)
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
  phaseState <- foldM (\state (_, interval) -> pairHits plan hits (intervalHits interval) state) (PairState M.empty M.empty) phases
  results <- forM (routeList routes) $ \route -> do
    let name = routeName route
    (prefix, own) <- case routeBody route of
      PhaseRoute phase -> do
        let index = fromEnum phase
        pure (map snd (take index phases), [snd (phases !! index)])
      KeyRoute keys _ -> do
        path <- forM (drop 1 (scanl (\acc key -> acc ++ [key]) [] keys)) $ \prefixKeys ->
          maybe (Left (RouteUnobserved name)) (Right . snd) (M.lookup prefixKeys stepMap)
        pure (map snd phases, path)
    before <- case routeBody route of
      KeyRoute _ _ -> pure phaseState
      PhaseRoute _ -> foldM (\state interval -> pairHits plan hits (intervalHits interval) state) (PairState M.empty M.empty) prefix
    final <- foldM (\state interval -> pairHits plan hits (intervalHits interval) state) before own
    let window interval = let (from, to) = intervalHits interval in [from .. to - 1]
        routeWindow = concatMap window own
        entries label = [ Call (hitArguments hit) (hitLr hit) (M.findWithDefault Nothing index (pairDone final))
                        | index <- routeWindow, let hit = hits V.! index, hitSite hit == PlanSite label ]
        appends = [hitArguments hit | index <- routeWindow, let hit = hits V.! index, hitSite hit == PlanSite Area3Append]
        lastSites = [hitSite (hits V.! index) | index <- window (last own)]
        lastEffects = concat [ case site of
                                 PlanSite SetterEntry -> map SetterEffect (take 1 (hitArguments hit))
                                 PlanSite PropertyCommit -> [CommitEffect]
                                 _ -> []
                             | index <- window (last own), let hit = hits V.! index, let site = hitSite hit ]
        effects = [replayed M.! intervalName interval | interval <- own]
        writes = concatMap fst effects
    routeClass <- case routeBody route of
      PhaseRoute _ -> pure PhaseClass
      KeyRoute _ (CrawlTrial _ expected) -> do
        require (RouteEffectsDiffer name) (lastEffects == expected)
        pure CrawlClass
      KeyRoute _ (Declared _) -> do
        require (DeclaredRouteInert name) (PlanSite SetterEntry `elem` lastSites && PlanSite Area3Append `elem` lastSites)
        pure DeclaredClass
    forM_ writes $ \write -> require (ForeignWriter name (effectPc write)) (effectWriter write /= CarrierBody)
    pure RouteResult
      { resultRoute = name
      , resultClass = routeClass
      , resultGetters = entries GetterEntry
      , resultSetters = entries SetterEntry
      , resultAppends = appends
      , resultWrites = writes
      , resultTransfers = concatMap snd effects
      , resultFinal = M.toList (intervalAfter (last own))
      , resultPresses = [press | Just press <- map intervalPress own]
      }
  cohort <- forM [["inputs", "run", "sha256"], ["inputs", "boot_rom", "sha256"], ["inputs", "adjustment_nand", "sha256"], ["inputs", "bridge", "sha256"]
                 , ["inputs", "routes", "sha256"], ["inputs", "linked", "sha256"], ["inputs", "contract", "sha256"], ["inputs", "observer", "sha256"]] $ \path ->
    (,) (T.unpack (T.intercalate "." path)) <$> receiptDigest path
  pure (Qualified role cohort (V.length hits) (length steps) results)

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
  exact ["step", "parent", "keys", "hits", "idle", "press", "watch", "regions_before", "regions_after"] fields
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
  menu <- field "menu" press
  rows <- at "watch" fields >>= parseWatch FixedViews
  before <- at "regions_before" fields >>= parseRegions
  after <- at "regions_after" fields >>= parseRegions
  pure (keys, parent, Interval (StepInterval number) before after rows range (Just (Press status menu)))

-- Comparison.

-- | Per route: the ordered setter calls with each run of identical time-driven blocks collapsed to
-- one block, area-3 appends, byte-changing writes and transfers, the final regions and each press's
-- menu must be equal; for each (caller, arguments) of a returned getter call, the results in call
-- order with consecutive repeats collapsed must be equal; settle statuses only where neither press
-- exhausted its budget.
compareRoles :: Qualified -> Qualified -> Either Refusal [(RouteResult, RouteResult)]
compareRoles retained owned = do
  unless (qualifiedRole retained == RetainedRole && qualifiedRole owned == OwnedRole) (Left (CohortDiffers "roles"))
  forM_ (zip (qualifiedCohort retained) (qualifiedCohort owned)) $ \((name, left), (_, right)) ->
    unless (left == right) (Left (CohortDiffers name))
  unless (map resultRoute (qualifiedRoutes retained) == map resultRoute (qualifiedRoutes owned)) (Left RouteSetDiffers)
  forM (zip (qualifiedRoutes retained) (qualifiedRoutes owned)) $ \(left, right) -> do
    let name = resultRoute left
        differs refusal select = when (select left /= select right) (Left (refusal name))
        writeKey write = (effectAddress write, effectWidth write, effectBytes write)
        getterResults result = M.map (map NE.head . NE.group) (M.fromListWith (flip (++)) [((callReturn call, callArguments call), [value]) | call <- resultGetters result, Just value <- [callResult call]])
        pressAgrees (Press leftStatus leftMenu) (Press rightStatus rightMenu) = leftMenu == rightMenu
          && (leftStatus == rightStatus || BudgetExhausted `elem` [leftStatus, rightStatus])
    differs SetterSequenceDiffers (map setterItemShape . collapseBlocks . resultSetters)
    differs GetterResultsDiffer getterResults
    differs WriteSequenceDiffers (map writeKey . filter effectChanges . resultWrites)
    differs TransferSequenceDiffers resultTransfers
    differs AppendSequenceDiffers resultAppends
    differs FinalBytesDiffer resultFinal
    unless (length (resultPresses left) == length (resultPresses right) && and (zipWith pressAgrees (resultPresses left) (resultPresses right)))
      (Left (PressDiffers name))
    pure (left, right)

-- | One setter call, or a run of one `timeDrivenSequence` block whose calls' arguments and results
-- repeat it consecutively, with the run's block count.
data SetterItem = SingleSetter Call | BlockRun [Call] Int

setterItemShape :: SetterItem -> Either [([Word32], Maybe Word32)] Call
setterItemShape item = case item of
  BlockRun block _ -> Left (map callValues block)
  SingleSetter call -> Right call

callValues :: Call -> ([Word32], Maybe Word32)
callValues call = (callArguments call, callResult call)

collapseBlocks :: [Call] -> [SetterItem]
collapseBlocks calls = case splitAt 4 calls of
  (block, rest) | map (take 1 . callArguments) block == map (: []) timeDrivenSequence ->
    let (repeats, rest') = span' block rest in BlockRun block (1 + repeats) : collapseBlocks rest'
  _ -> case calls of
    call : rest -> SingleSetter call : collapseBlocks rest
    [] -> []
 where
  span' block remaining = case splitAt 4 remaining of
    (next, rest) | map callValues next == map callValues block -> let (count, rest') = span' block rest in (count + 1, rest')
    _ -> (0 :: Int, remaining)

-- | Each block run's index among the collapsed items, index of its first setter call, and block count.
blockRuns :: [Call] -> [(Int, Int, Int)]
blockRuns calls = [ (position, first, count)
                  | (position, first, BlockRun _ count) <- zip3 [0 ..] (scanl (+) 0 (map width items)) items ]
 where
  items = collapseBlocks calls
  width item = case item of
    SingleSetter _ -> 1
    BlockRun block count -> length block * count

-- | Admits the guest control's refusal: its observation stopped at its first guarded hit, which is
-- the restored setter word's retired arm entered for that word's property, and every earlier hit is
-- an armed site of the control plan.
checkGuestControl :: Bindings -> Qualified -> ControlWord -> String -> String -> Observation -> Either Refusal Value
checkGuestControl bindings owned control datDigest image4Digest observation = do
  receipt <- either (Left . ReceiptShape) Right (decodeValue (obsReceipt observation))
  let receiptDigest path = either (Left . ReceiptShape) Right (digestAt path receipt)
      require refusal condition = unless condition (Left refusal)
  contract <- either (Left . InputDigest . ("contract: " ++)) Right (decodeContract (bindContract bindings))
  let inputs = PlanInputs ControlRole contract (bindImages bindings) (bindLinked bindings) (Just control)
  plan <- either (Left . InputDigest) Right (derivePlan inputs)
  image <- either (Left . InputDigest) Right (roleImage4 inputs)
  require (InputDigest "control image4") (digestOf image == image4Digest)
  require (ReceiptShape "role") (either (const False) (== String "control") (field "role" receipt))
  recordedDat <- receiptDigest ["inputs", "dat", "sha256"]
  require (InputDigest "control DAT") (recordedDat == datDigest)
  recordedImage <- receiptDigest ["inputs", "role_image4_sha256"]
  require (InputDigest "role image4") (recordedImage == image4Digest)
  forM_ [ (["inputs", "linked", "sha256"], digestOf (bindLinked bindings), "linked")
        , (["inputs", "contract", "sha256"], digestOf (bindContract bindings), "contract")
        , (["inputs", "observer", "sha256"], bindObserver bindings, "observer")
        , (["inputs", "routes", "sha256"], digestOf (bindRoutes bindings), "routes")
        , (["inputs", "symbols", "sha256"], bindSymbols bindings, "symbols")
        , (["files", "hits.jsonl"], digestOf (obsHits observation), "hits.jsonl")
        , (["files", "spec.json"], digestOf (obsSpec observation), "spec.json")
        , (["files", "steps.json"], digestOf (obsSteps observation), "steps.json")
        , (["files", "reset-observation.json"], digestOf (obsReset observation), "reset-observation.json")
        , (["files", "emulator-session.json"], obsSessionDigest observation, "emulator-session.json") ] $ \(path, expected, name) -> do
    recorded <- receiptDigest path
    require (InputDigest name) (recorded == expected)
  forM_ [path | (path, _) <- qualifiedCohort owned, path `elem` ["inputs.run.sha256", "inputs.boot_rom.sha256", "inputs.adjustment_nand.sha256", "inputs.bridge.sha256"]] $ \path -> do
    recorded <- receiptDigest (T.splitOn "." (T.pack path))
    require (CohortDiffers path) (lookup path (qualifiedCohort owned) == Just recorded)
  spec <- either (Left . ReceiptShape . ("spec: " ++)) Right (decodeValue (obsSpec observation))
  require SiteMapDiffers (spec == specValue plan)
  reason <- either (Left . ReceiptShape) Right (field "outcome" receipt >>= field "refusal" >>= field "reason")
  require (ObservationRefused reason) (reason == String "guarded-fetch")
  hits <- forM (zip [0 ..] (filter (not . B.null) (C.lines (obsHits observation)))) $ \(index, line) ->
    either (Left . HitShape index) Right (decodeValue line >>= parseHit)
  (earlier, final) <- case reverse hits of
    lastHit : rest -> Right (reverse rest, lastHit)
    [] -> Left (HitShape 0 "the control recorded no hit")
  forM_ (zip [0 ..] earlier) $ \(index, hit) -> do
    kind <- classifyHit plan index hit
    case kind of
      GuardHit -> Left (GuardedFetch index)
      _ -> pure ()
  let finalIndex = length earlier
  require (UnarmedSite finalIndex) (M.lookup (hitAddress final) (planSites plan) == Just (Guard RetiredSetterArm)
    && hitSite final == PlanSite (Guard RetiredSetterArm))
  require (GuardedFetch finalIndex) (hitAddress final == controlTarget control && take 1 (hitArguments final) == [controlProperty control])
  pure (object [ "verdict" .= ("refused-at-restored-arm" :: T.Text), "property" .= controlProperty control
               , "target" .= controlTarget control, "hits" .= length hits ])

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
  , "area3_appends" .= length (resultAppends result)
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
  , "area3_appends" .= sum (map (length . resultAppends) (qualifiedRoutes result))
  , "properties_by_class" .= object
      [ K.fromText (routeClassWire routeClass) .= S.toList (S.fromList (concatMap propertiesWritten [route | route <- qualifiedRoutes result, resultClass route == routeClass]))
      | routeClass <- [minBound .. maxBound] ]
  , "per_route" .= map routeSummary (qualifiedRoutes result)
  ]

comparisonValue :: [(RouteResult, RouteResult)] -> Value
comparisonValue pairs = object
  [ "verdict" .= ("equal" :: T.Text)
  , "routes" .= length pairs
  , "per_route" .=
      [ object
          [ "retained" .= routeSummary left, "owned" .= routeSummary right
          , "setter_block_runs" .= object ["retained" .= runsValue left, "owned" .= runsValue right] ]
      | (left, right) <- pairs ]
  ]
 where
  runsValue result = [object ["position" .= position, "first_call" .= first, "repeats" .= count] | (position, first, count) <- blockRuns (resultSetters result)]

-- Serialized refusal controls.

data ControlName
  = ReorderedWrite | DroppedWrite | ExtraWrite | ChangedValue | MissingSetterCall | ChangedGetterResult
  | WrongDatDigest | WrongLinkedBin | InjectedGuardedFetch
  deriving (Eq, Show, Enum, Bounded)

controlNames :: [ControlName]
controlNames = [minBound .. maxBound]

-- | A control passes when its observed refusal's rendering starts with one of the expected prefixes.
data ControlOutcome = ControlOutcome { controlName :: ControlName, controlExpected :: [String], controlObserved :: Either Refusal () }

-- | Runs one control against ROLE's observation, which must be admitted with the other role's.
runControl :: Bindings -> Role -> Observation -> Observation -> ControlName -> Either String ControlOutcome
runControl bindings role mutated other control = do
  let otherRole = if role == RetainedRole then OwnedRole else RetainedRole
      admitBoth observation = do
        mine <- qualify bindings role observation
        theirs <- qualify bindings otherRole other
        _ <- if role == RetainedRole then compareRoles mine theirs else compareRoles theirs mine
        pure ()
      outcome expected result = Right (ControlOutcome control expected result)
      -- An owned leaf's writes come from the accessors, whose hits qualify matches row by row.
      writeRefusals = "WriteSequenceDiffers" : ["AccessorStoreUnmatched" | role == OwnedRole]
  steps <- decodeValue (obsSteps mutated) >>= asArray
  case control of
    WrongDatDigest
      | role == RetainedRole -> outcome ["InputDigest \"source DAT\""] (() <$ qualify bindings { bindSourceDat = bindOwnedDat bindings } role mutated)
      | otherwise -> outcome ["InputDigest \"owned DAT\""] (() <$ qualify bindings { bindOwnedDat = bindSourceDat bindings } role mutated)
    WrongLinkedBin -> outcome ["InputDigest \"linked\""] (() <$ qualify bindings { bindLinked = flipFirst (bindLinked bindings) } role mutated)
    InjectedGuardedFetch -> do
      plan <- planFor bindings role
      let guardSite = find (\(_, candidate) -> case candidate of Guard _ -> True; _ -> False) (M.toList (planSites plan))
          (address, label) = fromMaybe (0x62d670, Guard CarrierFill) guardSite
          hit = object [ "site" .= siteLabelWire label, "address" .= hexText address, "core" .= (0 :: Int)
                       , "instructions" .= (0 :: Int), "registers" .= object [K.fromString r .= ("0x0" :: T.Text) | r <- ["r0", "r1", "r2", "r3"]]
                       , "sp" .= ("0x0" :: T.Text), "lr" .= ("0x0" :: T.Text), "cpsr" .= (0 :: Int) ]
      (_, leaf) <- leafStep steps
      (from, _) <- stepRange leaf
      changed <- insertHits mutated from [hit]
      outcome [if role == RetainedRole then "UnarmedSite" else "GuardedFetch"] (() <$ qualify bindings role changed)
    _ | control `elem` [ReorderedWrite, DroppedWrite, ExtraWrite, ChangedValue] -> do
      (leafIndex, leaf) <- leafStep steps
      changed <- mutateLeafWrites control leaf
      observation <- replaceStep mutated leafIndex changed
      outcome writeRefusals (admitBoth observation)
    MissingSetterCall -> do
      plan <- planFor bindings role
      hits <- decodeHitValues mutated
      phasesEnd <- attachedEnd mutated
      (entry, returned) <- matchedCall plan (take phasesEnd hits) SetterEntry
      observation <- removeHits mutated [entry, returned]
      outcome ["SetterSequenceDiffers"] (admitBoth observation)
    ChangedGetterResult -> do
      plan <- planFor bindings role
      hits <- decodeHitValues mutated
      (_, returned) <- matchedCall plan hits GetterEntry
      let flipped = setR0 (\value -> value `xor` 1) (hits !! returned)
      observation <- replaceHit mutated returned flipped
      outcome ["GetterResultsDiffer"] (admitBoth observation)
    _ -> Left "unhandled control"
 where
  hexText :: Word32 -> T.Text
  hexText value = T.pack ("0x" ++ showHex value "")

flipFirst :: B.ByteString -> B.ByteString
flipFirst bytes = case B.uncons bytes of
  Just (first, rest) -> B.cons (first `xor` 1) rest
  Nothing -> bytes

planFor :: Bindings -> Role -> Either String Plan
planFor bindings role = do
  contract <- decodeContract (bindContract bindings)
  derivePlan (PlanInputs role contract (bindImages bindings) (bindLinked bindings) Nothing)

-- | The hit count when the attached phase ended.
attachedEnd :: Observation -> Either String Int
attachedEnd observation = do
  segments <- decodeValue (obsReset observation) >>= field "segments" >>= asArray
  case reverse segments of
    attached : _ -> field "hits_end" attached >>= asInt
    [] -> Left "the reset observation has no segment"

-- | The last step with a CPU write whose bytes it changes and no step beneath it.
leafStep :: [Value] -> Either String (Int, Value)
leafStep steps = do
  parents <- S.fromList <$> mapM (\step -> field "parent" step >>= asInt) steps
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
  reset <- decodeValue (obsReset observation)
  segments <- field "segments" reset >>= asArray
  segments' <- forM segments $ \segment -> do
    end <- field "hits_end" segment >>= asInt
    pure (setField "hits_end" (Number (fromIntegral (shift end))) segment)
  let hitBytes = BL.toStrict (mconcat [A.encode hit <> "\n" | hit <- hits])
      stepBytes = BL.toStrict (A.encode (Array (V.fromList steps')) <> "\n")
      resetBytes = BL.toStrict (A.encode (setField "segments" (Array (V.fromList segments')) reset))
  rebind observation { obsHits = hitBytes, obsSteps = stepBytes, obsReset = resetBytes }

rebind :: Observation -> Either String Observation
rebind observation = do
  receipt <- decodeValue (obsReceipt observation)
  files <- field "files" receipt
  let files' = foldr (\(name, bytes) acc -> setField name (String (T.pack (digestOf bytes))) acc) files
        [("hits.jsonl", obsHits observation), ("steps.json", obsSteps observation), ("reset-observation.json", obsReset observation)]
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
