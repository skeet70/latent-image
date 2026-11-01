{-# LANGUAGE OverloadedStrings #-}
module Emulation.PrimaryInitializerConsumer
  ( Contract, Receipt, Status (..), deriveContract, contractValue, parseReceipt
  , validateReceipt, compareReceipts, receiptStatus
  ) where

import Control.Monad (foldM, forM, forM_, unless)
import Data.Aeson (FromJSON, Object, Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32, Word64)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import Emulation.InterfaceInitializer
  ( Context (..), Views (..), Source (..), ByteOrder (..), Event (..), Mode (..)
  , PhysicalSpan (..), Window (..), BankSnapshot (..), TcmSnapshot (..), WatchClass (..)
  , FixedViews (..), NoFault (..), Rule (..), Watch (..), Alias (..), Decoder (..)
  )
import qualified Emulation.PrimaryInitializer as PI
import qualified Emulation.PrimaryInitializerConsumerSource as PSC
import qualified Emulation.PrimaryInitializerSource as PS
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)

data Contract = Contract PI.Contract PSC.ConsumerSource
newtype Physical = Physical Word64 deriving (Eq, Ord)
newtype Tick = Tick Word64 deriving (Eq, Ord)
newtype Clock = Clock Word64 deriving (Eq, Ord)
newtype AccessBytes = AccessBytes B.ByteString deriving (Eq)
data Status = Observed | Absent | Refused deriving (Eq, Show)
data Result = Passed | Failed deriving (Eq)
data Boundary = Installation | Entry | Selected | After deriving (Eq, Ord)
data Stop = Stop Boundary Context deriving (Eq)
data TcmBank = Atcm | Btcm deriving (Eq)
newtype BankOffset = BankOffset Word64 deriving (Eq)
data TcmLocation = TcmLocation TcmBank BankOffset deriving (Eq)
data RawEvent = RawEvent Event (Maybe AccessBytes) Tick (Maybe TcmLocation) deriving (Eq)
data Core = Core0 | Core1 deriving (Eq)
newtype RuleIndex = RuleIndex Word64 deriving (Eq)
newtype Address = Address Word64 deriving (Eq)
newtype AccessWidth = AccessWidth Word64 deriving (Eq)
newtype RecordLimit = RecordLimit Word64 deriving (Eq)
newtype ObservedRegister = ObservedRegister Word64 deriving (Eq)
data FaultSite = FaultSite Core Source Address AccessWidth deriving (Eq)
data FaultSample = FaultSample Event (Maybe AccessBytes) (Maybe Tick) (Maybe TcmLocation) deriving (Eq)
data WatchFault
  = Confinement RuleIndex FaultSample
  | CounterAccess RuleIndex FaultSample
  | CounterOverflow RuleIndex
  | CoverageWidth RuleIndex FaultSite
  | UnsupportedWidth FaultSite
  | Unresolved FaultSite
  | TcmStraddle FaultSite TcmBank
  | TcmChanged Core TcmBank Word32 ObservedRegister
  | PolicyChanged Core
  | Inspection FaultSite String
  | DecodingChanged
  | LogLimit RecordLimit
  deriving (Eq)
data RawWatch = RawWatch (Watch RawEvent) [Window] (Maybe WatchFault)
data CodeView = CodeView PSC.CodeSite Views deriving (Eq)

data Continuation = Continuation
  { continuationStart :: Context
  , rootsStart :: Views
  , preludeWatch :: RawWatch
  , boundedWatch :: Maybe RawWatch
  }

data Consumer = Consumer
  { entryContext :: Context
  , selectedContext :: Context
  , afterContext :: Context
  , rootsBefore :: Views
  , rootsAfter :: Views
  , tableBefore :: Views
  , tableAfter :: Views
  , stackBefore :: Views
  , stackAfter :: Views
  , poolBefore :: Views
  , poolAfter :: Views
  , codeViews :: [CodeView]
  , decoder :: Decoder
  , aliases :: [Alias]
  }

data Receipt = Receipt
  { receiptStatus :: Status
  , nestedInitializer :: PI.Receipt
  , boundaries :: [Stop]
  , continuation :: Maybe Continuation
  , consumer :: Maybe Consumer
  }

budget :: Word64
budget = 50000000

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

deriveContract :: PI.Contract -> PSC.ConsumerSource -> Either String Contract
deriveContract initializer source = do
  require (PS.sourceDigest (PI.contractSource initializer) == PS.sourceDigest (PSC.consumerSource source)
    && PS.sourceLoaderDigest (PI.contractSource initializer) == PS.sourceLoaderDigest (PSC.consumerSource source))
    "primary consumer source differs from initializer source"
  forM_ (PSC.consumerCode source) $ \(PSC.CodeRegion site (PS.Guest start) words') ->
    case site of
      PSC.Selector -> sameLoader start words'
      PSC.Identity -> sameLoader start words'
      _ -> pure ()
  pure (Contract initializer source)
  where
    sameLoader start words' = do
      actual <- mapM (leWordAt (PI.contractLoaderBytes initializer) . (fromIntegral (start - 0x40000) +)) [0,4 .. 4 * length words' - 4]
      require (actual == words') "primary consumer selector code differs from actual producer loader"

contractValue :: Contract -> Value
contractValue (Contract initializer source) = object
  ["schema" .= (9 :: Word32),"initializer" .= PI.contractValue initializer,"consumer" .= PSC.consumerSourceValue source]

json :: FromJSON a => Value -> Either String a
json value = case J.fromJSON value of
  J.Error problem -> Left problem
  J.Success result -> Right result

fields :: [String] -> Value -> Either String Object
fields names (Object row) = do
  require (sort (map Key.toString (KM.keys row)) == sort names) "primary root0 object fields differ"
  pure row
fields _ _ = Left "primary root0 evidence is not an object"

field :: Object -> String -> Either String Value
field row name = maybe (Left ("missing primary root0 field " ++ name)) Right (KM.lookup (Key.fromString name) row)

get :: FromJSON a => Object -> String -> Either String a
get row name = field row name >>= json

nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value

parseContext :: Value -> Either String Context
parseContext value = do
  _ <- fields ["pc","registers","sp","lr","cpsr","counts","tcm"] value
  context@(Context pc registers sp _ _ clocks tcm) <- json value
  require (length registers == 13 && length clocks == 2 && map length tcm == [2,2]) "primary root0 context width differs"
  require (pc .&. 3 == 0 && sp .&. 3 == 0 && all (<= budget) clocks && sum clocks <= budget)
    "primary root0 context alignment or global budget differs"
  pure context

parseViews :: Value -> Either String Views
parseViews value = fields ["virtual","physical"] value >> json value

parseSnapshot :: Value -> Either String TcmSnapshot
parseSnapshot value = do
  row <- fields ["core","atcm","btcm"] value
  forM_ ["atcm","btcm"] $ \name -> field row name >>= fields ["register","window"] >> pure ()
  json value

parseRule :: Value -> Either String Rule
parseRule value = do
  row <- fields ["cores","kind","mode","spans","tcm"] value
  spans <- get row "spans" :: Either String [Value]
  forM_ spans $ \span' -> fields ["physical"] span' >> pure ()
  snapshots <- get row "tcm" >>= mapM parseSnapshot
  Rule <$> get row "cores" <*> get row "kind" <*> get row "mode" <*> get row "spans" <*> pure snapshots

parseBank :: Value -> Either String TcmBank
parseBank value = do
  name <- json value :: Either String String
  case name of
    "atcm" -> Right Atcm
    "btcm" -> Right Btcm
    _ -> Left "unsupported primary root0 TCM bank"

parseCore :: Value -> Either String Core
parseCore value = do
  core <- json value :: Either String Word32
  case core of
    0 -> Right Core0
    1 -> Right Core1
    _ -> Left "unsupported primary root0 fault core"

parseLocation :: Value -> Either String TcmLocation
parseLocation value = do
  row <- fields ["bank","offset"] value
  TcmLocation <$> (field row "bank" >>= parseBank) <*> (BankOffset <$> get row "offset")

parseSample :: Value -> Either String FaultSample
parseSample value = do
  let names = ["source","core","address","physical","size","value","pc","instructions","byte_order","before","tick"]
  row <- case value of
    Object entries -> fields (names ++ ["tcm" | KM.member "tcm" entries]) value
    _ -> Left "primary root0 event is not an object"
  before <- field row "before" >>= nullable (\bytes -> AccessBytes . B.pack <$> (json bytes :: Either String [Word8]))
  tick <- field row "tick" >>= nullable (fmap Tick . json)
  location <- maybe (Right Nothing) (nullable parseLocation) (KM.lookup "tcm" row)
  FaultSample <$> json value <*> pure before <*> pure tick <*> pure location

parseEvent :: Value -> Either String RawEvent
parseEvent value = do
  FaultSample event@(Event kind _ _ _ size eventValue _ _ _) before clock location <- parseSample value
  require (size `elem` [1,2,4,8]) "primary root0 access width is unsupported"
  forM_ before $ \(AccessBytes preimage) ->
    require (fromIntegral (B.length preimage) == size) "primary root0 access preimage extent differs"
  require (if kind == CpuFetch then before == Nothing && eventValue == Nothing else before /= Nothing)
    "primary root0 event preimage presence differs"
  tick <- maybe (Left "primary root0 raw record tick is absent") Right clock
  pure (RawEvent event before tick location)

parseFault :: Value -> Either String WatchFault
parseFault value = do
  row <- case value of Object entries -> Right entries; _ -> Left "primary root0 fault is not an object"
  kind <- get row "kind" :: Either String String
  let exact names action = fields ("kind":names) value >> action
      rule = RuleIndex <$> get row "rule"
      core = field row "core" >>= parseCore
      bank = field row "bank" >>= parseBank
      site = FaultSite <$> core <*> get row "source" <*> (Address <$> get row "address") <*> (AccessWidth <$> get row "size")
      siteNames = ["core","source","address","size"]
      sample = field row "sample" >>= parseSample
  case kind of
    "confinement" -> exact ["rule","sample"] (Confinement <$> rule <*> sample)
    "counter-access" -> exact ["rule","sample"] (CounterAccess <$> rule <*> sample)
    "counter-overflow" -> exact ["rule"] (CounterOverflow <$> rule)
    "coverage-width" -> exact ("rule":siteNames) (CoverageWidth <$> rule <*> site)
    "unsupported-width" -> exact siteNames (UnsupportedWidth <$> site)
    "unresolved" -> exact siteNames (Unresolved <$> site)
    "tcm-straddle" -> exact ("bank":siteNames) (TcmStraddle <$> site <*> bank)
    "tcm-changed" -> exact ["core","bank","expected","observed"] (TcmChanged <$> core <*> bank <*> get row "expected" <*> (ObservedRegister <$> get row "observed"))
    "policy-changed" -> exact ["core"] (PolicyChanged <$> core)
    "inspection" -> exact ("message":siteNames) (Inspection <$> site <*> get row "message")
    "decoding-changed" -> exact [] (pure DecodingChanged)
    "log-limit" -> exact ["limit"] (LogLimit . RecordLimit <$> get row "limit")
    _ -> Left "unsupported primary root0 watch fault"

parseWatch :: Value -> Either String RawWatch
parseWatch value = do
  row <- fields ["enabled","ranges","violation","records","classes","views","fault","tcm","confinement","counters","coverage"] value
  counters <- get row "counters" :: Either String [Value]
  coverage <- get row "coverage" :: Either String [Value]
  require (null counters && null coverage) "primary root0 watch has unsupported coverage or counters"
  classes <- get row "classes" :: Either String [Value]
  forM_ classes $ \entry -> fields ["range","kinds"] entry >> pure ()
  events <- get row "records" >>= mapM parseEvent
  rules <- get row "confinement" >>= mapM parseRule
  snapshots <- get row "tcm" >>= mapM parseSnapshot
  watch <- Watch <$> get row "enabled" <*> get row "violation" <*> pure events <*> pure rules <*> get row "classes"
    <*> get row "views" <*> pure NoFault <*> pure snapshots
  RawWatch watch <$> get row "ranges" <*> (field row "fault" >>= nullable parseFault)

parseCodeView :: Value -> Either String CodeView
parseCodeView value = do
  row <- fields ["start","views"] value
  start <- get row "start" :: Either String Word32
  site <- case start of
    0x725a04 -> Right PSC.Installation
    0x725b74 -> Right PSC.Descriptor
    0x412bc -> Right PSC.Selector
    0x427b8 -> Right PSC.Identity
    _ -> Left "unsupported primary root0 code site"
  CodeView site <$> (field row "views" >>= parseViews)

parseAlias :: Value -> Either String Alias
parseAlias value = fields ["core","guest","physical","size","sha256"] value >> json value

parseContinuation :: Value -> Either String Continuation
parseContinuation value = do
  row <- fields ["start","roots_start","prelude_watch","watch"] value
  Continuation <$> (field row "start" >>= parseContext) <*> (field row "roots_start" >>= parseViews)
    <*> (field row "prelude_watch" >>= parseWatch) <*> (field row "watch" >>= nullable parseWatch)

parseConsumer :: Value -> Either String Consumer
parseConsumer value = do
  row <- fields ["entry","selected","after","roots_before","roots_after","table_before","table_after","stack_before","stack_after"
    ,"pool_before","pool_after","code","decoder","aliases"] value
  let context name = field row name >>= parseContext
      views name = field row name >>= parseViews
  decoderValue <- field row "decoder"
  _ <- fields ["choices","pending"] decoderValue
  Consumer <$> context "entry" <*> context "selected" <*> context "after" <*> views "roots_before" <*> views "roots_after"
    <*> views "table_before" <*> views "table_after" <*> views "stack_before" <*> views "stack_after" <*> views "pool_before" <*> views "pool_after"
    <*> (get row "code" >>= mapM parseCodeView) <*> json decoderValue <*> (get row "aliases" >>= mapM parseAlias)

parseStop :: Value -> Either String Stop
parseStop value = do
  row <- fields ["boundary","context"] value
  name <- get row "boundary" :: Either String String
  (boundary,expectedPc) <- case name of
    "installation" -> Right (Installation,0x725a04)
    "entry" -> Right (Entry,0x725b74)
    "selected" -> Right (Selected,0x725b9c)
    "after" -> Right (After,0x725bb4)
    _ -> Left "unsupported primary root0 boundary"
  context <- field row "context" >>= parseContext
  require (contextPc context == expectedPc) "primary root0 boundary PC differs"
  pure (Stop boundary context)

parseReceipt :: B.ByteString -> Either String Receipt
parseReceipt bytes = do
  require (B.length bytes <= 64 * 1024 * 1024) "primary root0 receipt exceeds 64 MiB"
  document <- decodeStrictIntegers bytes
  row <- fields ["schema","result","status","error","machine_failure","instructions","cp14_identities","final","boundaries","initializer","continuation","consumer"] document
  schema <- get row "schema" :: Either String Word32
  require (schema == 9) "unsupported primary root0 receipt schema"
  resultName <- get row "result" :: Either String String
  result <- case resultName of "passed" -> Right Passed; "failed" -> Right Failed; _ -> Left "unsupported primary root0 result"
  statusName <- get row "status" :: Either String String
  status <- case statusName of
    "observed" -> Right Observed
    "absent" -> Right Absent
    "refused" -> Right Refused
    _ -> Left "unsupported primary root0 status"
  failure <- get row "error" :: Either String (Maybe String)
  machineFailure <- get row "machine_failure" :: Either String (Maybe String)
  instructions <- get row "instructions" :: Either String Word64
  identities <- get row "cp14_identities" :: Either String [Word32]
  final <- field row "final" >>= parseContext
  stops <- get row "boundaries" >>= mapM parseStop
  initializer <- field row "initializer" >>= PI.parseReceiptValue
  continued <- field row "continuation" >>= nullable parseContinuation
  observed <- field row "consumer" >>= nullable parseConsumer
  require (instructions == total final && instructions <= budget && identities == [0x4003,0x5003])
    "primary root0 final progress or CP14 identities differ"
  let names = [boundary | Stop boundary _ <- stops]
  require (names == take (length names) [Installation,Entry,Selected,After]) "primary root0 boundary order differs"
  progressing (PI.receiptFinal initializer) final
  forM_ (zip stops (drop 1 stops)) $ \(Stop _ first,Stop _ second) -> progressing first second
  forM_ stops $ \(Stop _ context) -> progressing context final
  case continued of
    Nothing -> require (null stops && absent observed && final == PI.receiptFinal initializer
      && ((PI.receiptStatus initializer == PI.Absent && status == Absent)
        || (PI.receiptStatus initializer `elem` [PI.Refused,PI.Observed] && status == Refused)))
      "primary root0 terminal producer status or continuation differs"
    Just next -> do
      start <- PI.receiptAfter initializer
      require (continuationStart next == start) "primary root0 prelude does not start at producer return"
      forM_ stops $ \(Stop _ context) -> progressing start context
      progressing start final
      let watches = preludeWatch next : maybe [] (:[]) (boundedWatch next)
          events = concatMap rawRecords watches
          clocks = [clock | RawEvent (Event _ _ _ _ _ _ _ (Just clock) _) _ _ _ <- events]
          ticks = [tick | RawEvent _ _ tick _ <- events]
      require (all (\clock -> clock >= total start && clock <= total final + 1) clocks
        && and (zipWith (<=) clocks (drop 1 clocks)) && and (zipWith (<=) ticks (drop 1 ticks)))
        "primary root0 partial event chronology leaves the observed interval"
      forM_ (boundedWatch next) $ \_ -> require (length stops >= 2) "primary root0 body watch precedes entry"
      require (status == Refused || all healthyWatch watches) "primary root0 watch refusal cannot become deadline absence"
  case status of
    Observed -> do
      require (result == Passed && failure == Nothing && machineFailure == Nothing && names == [Installation,Entry,Selected,After])
        "observed primary root0 receipt did not pass all boundaries"
      body <- maybe (Left "observed primary root0 receipt lacks consumer") Right observed
      require (drop 1 stops == [Stop Entry (entryContext body),Stop Selected (selectedContext body),Stop After (afterContext body)]
        && final == afterContext body) "primary root0 evidence boundaries differ"
    Absent -> require (result == Failed && failure /= Nothing && machineFailure == Nothing && instructions == budget
      && length stops < 4 && absent observed) "primary root0 absence is not an incomplete finite deadline"
    Refused -> require (result == Failed && failure /= Nothing && absent observed) "primary root0 refusal carries accepted consumer evidence"
  pure (Receipt status initializer stops continued observed)
  where
    absent Nothing = True
    absent (Just _) = False

contextPc :: Context -> Word32
contextPc (Context pc _ _ _ _ _ _) = pc

counts :: Context -> [Word64]
counts (Context _ _ _ _ _ clocks _) = clocks

total :: Context -> Word64
total = sum . counts

tcmRegisters :: Context -> [[Word32]]
tcmRegisters (Context _ _ _ _ _ _ banks) = banks

progressing :: Context -> Context -> Either String ()
progressing first second = require (and (zipWith (<=) (counts first) (counts second))) "primary root0 core progress decreased"

viewBytes :: Int -> Views -> Either String B.ByteString
viewBytes size (Views virtual physical) = do
  require (length physical == size && virtual == physical) "primary root0 physical/virtual snapshot differs"
  pure (B.pack physical)

snapshotsFor :: Context -> Either String [TcmSnapshot]
snapshotsFor context = forM (zip [0,1] (tcmRegisters context)) $ \(core,banks) -> case banks of
  [atcm,btcm] -> TcmSnapshot core <$> I.bankSnapshot atcm <*> I.bankSnapshot btcm
  _ -> Left "primary root0 TCM register width differs"

codeSpans :: PSC.ConsumerSource -> [(Word64,Word64)]
codeSpans source = [(fromIntegral start,fromIntegral start + 4 * fromIntegral (length words'))
  | PSC.CodeRegion _ (PS.Guest start) words' <- PSC.consumerCode source]

headers :: [(Word64,Word64)]
headers = [(0x40010,0x40014),(0xd0010,0xd0014)]

writes :: [Source]
writes = [CpuWrite,HostWrite,ModelBusWrite]

watchRules :: [TcmSnapshot] -> [Int] -> Source -> Mode -> [(Word64,Word64)] -> Rule
watchRules snapshots cores kind mode spans = Rule cores kind mode [PhysicalSpan [low,high] | (low,high) <- spans]
  [snapshot | snapshot@(TcmSnapshot core _ _) <- snapshots,core `elem` cores]

normalizedRanges :: [Window] -> [Window]
normalizedRanges = foldr merge [] . sort
  where
    merge (Window (low,high)) (Window (nextLow,nextHigh):rest)
      | high >= nextLow = Window (low,max high nextHigh):rest
    merge window rest = window:rest

rawRecords :: RawWatch -> [RawEvent]
rawRecords (RawWatch (Watch _ _ events _ _ _ _ _) _ _) = events

healthyWatch :: RawWatch -> Bool
healthyWatch (RawWatch (Watch enabled violation _ _ _ _ _ _) _ fault) =
  enabled && violation == Nothing && fault == Nothing


validateWatch :: [TcmSnapshot] -> [WatchClass] -> [Rule] -> RawWatch -> Either String [RawEvent]
validateWatch expectedTcm expectedClasses expectedRules (RawWatch (Watch enabled violation events rules classes FixedViews NoFault snapshots) ranges fault) = do
  require (enabled && violation == Nothing && fault == Nothing && snapshots == expectedTcm && rules == expectedRules
    && classes == expectedClasses && ranges == normalizedRanges [range | WatchClass range _ <- expectedClasses]) "primary root0 watch scope or confinement differs"
  forM_ events $ \(RawEvent (Event source core address physical size value pc clock order) before _ location) -> do
    require (location == Nothing) "primary root0 access resolves to TCM"
    translated <- maybe (Left "primary root0 event lacks physical attribution") Right physical
    require (core == Just 0 || core == Just 1) "primary root0 event lacks CPU attribution"
    require (source `elem` [CpuRead,CpuWrite,CpuFetch]) "primary root0 protected resource has a non-CPU effect"
    require (address == translated || address == translated + 0x40000000)
      "primary root0 event guest/physical attribution differs"
    require (size <= 8 && translated <= maxBound - size && pc /= Nothing && clock /= Nothing && order == Just LittleEndian)
      "primary root0 event attribution or width differs"
    require (any (\(WatchClass (Window window) kinds) -> not (disjoint (translated,translated + size) window) && source `elem` kinds) expectedClasses)
      "primary root0 raw event leaves its declared class"
    case (source,before) of
      (CpuFetch,Nothing) -> require (size == 4 && address .&. 3 == 0 && value == Nothing && address == translated && pc == Just (fromIntegral address))
        "primary root0 fetch attribution differs"
      (CpuRead,Just (AccessBytes bytes)) -> require (Just (bytesValue bytes) == value)
        "primary root0 read value differs from its preimage"
      (CpuWrite,Just _) -> require (maybe False (\word -> size == 8 || word < (1 `shiftL` fromIntegral (8 * size))) value)
        "primary root0 write value exceeds its access width"
      _ -> Left "primary root0 raw access preimage presence differs"
  validateChronology events
  pure events

validateChronology :: [RawEvent] -> Either String ()
validateChronology events = do
  let clocks = [Clock clock | RawEvent (Event _ _ _ _ _ _ _ (Just clock) _) _ _ _ <- events]
      ticks = [tick | RawEvent _ _ tick _ <- events]
  require (length clocks == length events && and (zipWith (<=) clocks (drop 1 clocks)) && and (zipWith (<=) ticks (drop 1 ticks)))
    "primary root0 raw event chronology decreased"

validateReceipt :: Contract -> Receipt -> Either String ()
validateReceipt (Contract initializer source) receipt = do
  PI.receiptInputIdentityMatches initializer (nestedInitializer receipt)
  PI.validateReceipt initializer (nestedInitializer receipt)
  require (receiptStatus receipt == Observed) "primary root0 validation requires observed evidence"
  next <- maybe (Left "primary root0 continuation is absent") Right (continuation receipt)
  body <- maybe (Left "primary root0 consumer is absent") Right (consumer receipt)
  start <- PI.receiptAfter (nestedInitializer receipt)
  require (continuationStart next == start) "primary root0 producer continuity differs"
  let entry@(Context _ incoming sp _ cpsr _ _) = entryContext body
      selected = selectedContext body
      after = afterContext body
  identifier <- case incoming of
    value:_ -> pure value
    [] -> Left "primary root0 entry has no argument register"
  ident <- PSC.descriptorId identifier
  let PSC.DescriptorAccess (PS.Guest tableGuest) (PS.Guest poolGuest) = PSC.descriptorAccess source ident
      descriptorPhysical = fromIntegral tableGuest - 0x40000000
      poolPhysical = fromIntegral poolGuest - 0x40000000
  require (sp >= 0x40000020 && sp <= 0x40100000 && sp .&. 3 == 0 && cpsr .&. 0x07f0feff == 0xd3)
    "primary root0 dynamic stack or A32 little-endian SVC context differs"
  let stackPhysical = fromIntegral sp - 0x40000000 - 32
      stackSpan = (stackPhysical,stackPhysical + 32)
      protected = [(0x4dae0,0x4f10c),(0x96bd20,0x96bd28),(poolPhysical,poolPhysical + 4)] ++ codeSpans source ++ headers
  require (all (disjoint stackSpan) protected) "primary root0 dynamic stack overlaps a protected resource"
  forM_ (continuationStart next : [context | Stop _ context <- boundaries receipt]) $ \context ->
    require (tcmRegisters context == tcmRegisters start) "primary root0 continuation changed raw TCM registers"
  expectedTcm <- snapshotsFor start
  roots <- viewBytes 8 (rootsBefore body)
  require (rootsBefore body == rootsAfter body && mapM (leWordAt roots) [0,4] == Right [0x4004dae0,0x400ddaa0])
    "primary root0 installed root pair differs"
  beforeTable <- viewBytes 0x162c (tableBefore body)
  _ <- viewBytes 0x162c (tableAfter body)
  require (tableBefore body == tableAfter body && leWordAt beforeTable 0x1618 == Right 0x412bc
    && leWordAt beforeTable (fromIntegral (descriptorPhysical - 0x4dae0)) == Right poolGuest)
    "primary root0 selected descriptor or selector differs from source publication"
  beforeStack <- viewBytes 32 (stackBefore body)
  afterStack <- viewBytes 32 (stackAfter body)
  pool <- viewBytes 4 (poolBefore body)
  _ <- viewBytes 4 (poolAfter body)
  require (poolBefore body == poolAfter body) "primary root0 bounded pool word changed"
  poolValue <- leWordAt pool 0
  validateContexts ident poolValue entry selected after
  code <- validateCode source (codeViews body)
  require (decoder body == Decoder [1] [Nothing]) "primary root0 live decoder differs"
  let beforeResources = [(0x96bd20,roots),(0x4dae0,beforeTable),(stackPhysical,beforeStack),(poolPhysical,pool)] ++ code
      afterResources = [(0x96bd20,roots),(0x4dae0,beforeTable),(stackPhysical,afterStack),(poolPhysical,pool)] ++ code
  validateAliases expectedTcm beforeResources afterResources (aliases body)
  let preludeClasses = [WatchClass (Window span') (CpuRead:writes) | span' <- [(0x4dae0,0x4f10c),(0x96bd20,0x96bd28)] ++ headers]
        ++ [WatchClass (Window span') ((if index < (2 :: Int) then [CpuFetch] else []) ++ writes)
           | (index,span') <- zip [0..] (codeSpans source)]
      preludeRules = [watchRules expectedTcm [0,1] CpuWrite Forbid (headers ++ drop 2 (codeSpans source))]
      resources = [(0x4dae0,0x4f10c),(0x96bd20,0x96bd28),stackSpan,(poolPhysical,poolPhysical + 4)] ++ codeSpans source ++ headers
      bodyClasses = [WatchClass (Window span') (CpuRead:writes) | span' <- take 4 resources]
        ++ [WatchClass (Window span') writes | span' <- drop 4 resources]
      bodyRules = [watchRules expectedTcm [0] CpuWrite Allow [stackSpan]
        ,watchRules expectedTcm [1] CpuWrite Forbid resources
        ,watchRules expectedTcm [0] CpuRead Allow [stackSpan,(0x96bd20,0x96bd28),(0x4dae0,0x4f10c),(poolPhysical,poolPhysical + 4)]]
  prelude <- validateWatch expectedTcm preludeClasses preludeRules (preludeWatch next)
  watch <- maybe (Left "primary root0 bounded watch is absent") Right (boundedWatch next)
  events <- validateWatch expectedTcm bodyClasses bodyRules watch
  producerTable <- PI.receiptTableAfter (nestedInitializer receipt)
  installation <- case boundaries receipt of
    Stop Installation context:_ -> Right context
    _ -> Left "primary root0 installation boundary is missing"
  validatePrelude source start installation entry descriptorPhysical producerTable next body prelude
  validateBody entry selected after poolGuest descriptorPhysical beforeStack afterStack roots beforeTable pool events
  validateChronology (prelude ++ events)

validateContexts :: PSC.DescriptorId -> Word32 -> Context -> Context -> Context -> Either String ()
validateContexts ident poolValue entry@(Context entryPc incoming sp _ cpsr entryCounts tcm)
    selected@(Context selectedPc selectedRegisters selectedSp selectedLr selectedCpsr selectedCounts selectedTcm)
    after@(Context afterPc afterRegisters afterSp afterLr afterCpsr afterCounts afterTcm) = do
  let identifier = PSC.descriptorIdentifier ident
      preserved = drop 7 (take 11 incoming) ++ [sp - 4,incoming !! 12]
      expectedSelected = [0,0x4003,0x4003,0x412bc,0x96bd20,identifier,0] ++ preserved
      expectedAfter = [0,poolValue,0x4004dae0,identifier - 1,0x96bd20,identifier,0] ++ preserved
      expectedCpsr = (cpsr .&. 0x0fffffff) .|. 0x60000000
  require (entryPc == 0x725b74 && selectedPc == 0x725b9c && afterPc == 0x725bb4
    && selectedRegisters == expectedSelected && afterRegisters == expectedAfter
    && selectedSp == sp - 24 && afterSp == selectedSp && selectedLr == 0x412c8 && afterLr == selectedLr
    && selectedCpsr == expectedCpsr && afterCpsr == expectedCpsr && selectedTcm == tcm && afterTcm == tcm)
    "primary root0 actual selection or descriptor read ABI differs"
  case (entryCounts,selectedCounts,afterCounts) of
    ([e0,e1],[s0,s1],[a0,a1]) -> require (s0 == e0 + 21 && a0 == s0 + 6 && e1 <= s1 && s1 <= a1)
      "primary root0 source instruction progress differs"
    _ -> Left "primary root0 count width differs"
  progressing entry selected
  progressing selected after

validateCode :: PSC.ConsumerSource -> [CodeView] -> Either String [(Word64,B.ByteString)]
validateCode source observed = do
  require (length observed == length (PSC.consumerCode source)) "primary root0 code coverage differs"
  forM (zip (PSC.consumerCode source) observed) $ \(PSC.CodeRegion site (PS.Guest start) words',CodeView observedSite views) -> do
    bytes <- viewBytes (4 * length words') views
    actual <- mapM (leWordAt bytes) [0,4 .. 4 * length words' - 4]
    require (site == observedSite && actual == words') "primary root0 live code differs from bound source"
    pure (fromIntegral start,bytes)

disjoint :: (Word64,Word64) -> (Word64,Word64) -> Bool
disjoint (low,high) (otherLow,otherHigh) = high <= otherLow || otherHigh <= low

validateAliases :: [TcmSnapshot] -> [(Word64,B.ByteString)] -> [(Word64,B.ByteString)] -> [Alias] -> Either String ()
validateAliases snapshots before after observed = do
  first <- bindings before
  second <- bindings after
  require (observed == first ++ second) "primary root0 live before/after aliases differ from captured physical snapshots"
  forM_ observed $ \(Alias core guest _ size _) ->
    forM_ [bank | TcmSnapshot selected atcm btcm <- snapshots,selected == core,bank <- [atcm,btcm]] $ \(BankSnapshot _ active) ->
      forM_ active $ \(Window window) -> require (disjoint (guest,guest + size) window) "primary root0 alias overlaps an enabled TCM bank"
  where
    bindings resources = do
      digests <- forM resources $ \(base,bytes) -> do
        digest <- F.parseSha256 (sha256Hex bytes)
        pure (base,fromIntegral (B.length bytes),digest)
      pure [Alias core (base + alias) base size digest | core <- [0,1],(base,size,digest) <- digests,alias <- [0,0x40000000]]

type Memory = M.Map Physical Word8

bytesValue :: B.ByteString -> Word64
bytesValue = B.foldr (\byte rest -> fromIntegral byte .|. (rest `shiftL` 8)) 0

memoryRegion :: Word64 -> B.ByteString -> Either String Memory
memoryRegion base bytes = pure (M.fromList [(Physical (base + offset),byte) | (offset,byte) <- zip [0..] (B.unpack bytes)])

replay :: Memory -> RawEvent -> Either String Memory
replay memory (RawEvent (Event source _ _ (Just address) size (Just value) _ _ _) (Just (AccessBytes before)) _ _) = do
  current <- B.pack <$> forM [0 .. size - 1] (\offset ->
    maybe (Left "primary root0 access has no captured preimage") Right (M.lookup (Physical (address + offset)) memory))
  require (before == current) "primary root0 raw preimage differs from replayed memory"
  case source of
    CpuRead -> require (value == bytesValue current) "primary root0 raw read differs from replayed memory" >> pure memory
    CpuWrite -> pure (foldr (\offset -> M.insert (Physical (address + offset)) (fromIntegral (value `shiftR` fromIntegral (8 * offset))))
      memory [0 .. size - 1])
    _ -> Left "primary root0 replay has an unsupported effect"
replay _ _ = Left "primary root0 replay lacks physical address or value"

checkWindow :: Context -> Context -> RawEvent -> Either String ()
checkWindow first lastContext (RawEvent (Event _ _ _ _ _ _ _ clock _) _ _ _) =
  require (maybe False (\value -> value > total first && value <= total lastContext) clock) "primary root0 event leaves its continuous instruction interval"

validatePrelude :: PSC.ConsumerSource -> Context -> Context -> Context -> Word64 -> B.ByteString -> Continuation -> Consumer -> [RawEvent] -> Either String ()
validatePrelude source start installation entry selectedPhysical producerTable next body events = do
  (installationCore0,entryCore1) <- case (counts installation,counts entry) of
    ([core0,_],[_,core1]) -> pure (core0,core1)
    _ -> Left "primary root0 installation core count width differs"
  rootBytes <- viewBytes 8 (rootsStart next)
  initialTable <- memoryRegion 0x4dae0 producerTable
  initialRoots <- memoryRegion 0x96bd20 rootBytes
  let initial = M.union initialTable initialRoots
      installationEvents = [event | event@(RawEvent (Event kind _ _ (Just address) _ _ pc _ _) _ _ _) <- events
        , kind == CpuWrite && address >= 0x96bd20 && address < 0x96bd28
          || pc `elem` [Just 0x725a1c,Just 0x725a2c] && address `elem` [0x40010,0xd0010]]
      signatures = [(kind,core,guest,address,size,value,pc) | RawEvent (Event kind core guest (Just address) size value pc _ _) _ _ _ <- installationEvents]
  require (signatures == [(CpuRead,Just 0,0x40010,0x40010,4,Just 0x4004dae0,Just 0x725a1c)
    ,(CpuWrite,Just 0,0x96bd20,0x96bd20,4,Just 0x4004dae0,Just 0x725a20)
    ,(CpuRead,Just 0,0xd0010,0xd0010,4,Just 0x400ddaa0,Just 0x725a2c)
    ,(CpuWrite,Just 0,0x96bd24,0x96bd24,4,Just 0x400ddaa0,Just 0x725a30)]) "primary root0 installation read/write provenance differs"
  forM_ installationEvents (checkWindow installation entry)
  let fetched = [(address,clock) | RawEvent (Event CpuFetch (Just 0) address _ _ _ _ clock _) _ _ _ <- events]
      expectedFetches = 0x725a04 : ([0x725a04,0x725a08 .. 0x725a30] ++ [0x725b74])
      installerFetches = take 12 (drop 1 fetched)
  firstFetch <- case fetched of
    first:_ -> pure first
    [] -> Left "primary root0 installation fetch is absent"
  require (map fst fetched == expectedFetches && firstFetch == (0x725a04,Just (total installation + 1))
    && last fetched == (0x725b74,Just (total entry + 1))) "primary root0 first installation or consumer fetch differs"
  forM_ (zip [0..] installerFetches) $ \(ordinal,(_,clock)) ->
    require (maybe False (\value -> value >= total installation + ordinal + 1
      && value <= installationCore0 + ordinal + 1 + entryCore1) clock)
      "primary root0 installation fetch progress differs"
  let installerPeers = [clock - installationCore0 - ordinal - 1
        | (ordinal,(_,Just clock)) <- zip [0..] installerFetches]
  require (and (zipWith (<=) installerPeers (drop 1 installerPeers)))
    "primary root0 installation peer progress decreased"
  forM_ installationEvents $ \(RawEvent (Event _ _ _ _ _ _ pc clock _) _ _ _) ->
    require (maybe Nothing (\site -> lookup (fromIntegral site) installerFetches) pc == Just clock)
      "primary root0 installation data clock differs from its instruction fetch"
  finalMemory <- foldM step initial events
  expectedTable <- viewBytes 0x162c (tableBefore body) >>= memoryRegion 0x4dae0
  expectedRoots <- viewBytes 8 (rootsBefore body) >>= memoryRegion 0x96bd20
  require (finalMemory == M.union expectedTable expectedRoots) "primary root0 intervening write replay differs from consumer snapshots"
  where
    step memory (RawEvent (Event CpuFetch (Just 1) _ _ _ _ _ clock _) _ _ _) = do
      require (maybe False (\value -> value > total start && value <= total entry) clock)
        "primary root0 peer fetch leaves the prelude interval"
      pure memory
    step memory (RawEvent (Event CpuFetch _ address _ _ _ _ clock _) _ _ _) = do
      require (maybe False (\value -> value >= total start && value <= total entry + 1) clock
        && (address == 0x725b74 || maybe False (>= total installation + 1) clock))
        "primary root0 preexecution fetch leaves its boundary interval"
      pure memory
    step memory event@(RawEvent (Event kind _ _ (Just address) size value _ clock _) _ _ _) = do
      checkWindow start entry event
      if address >= 0x4dae0 && address < 0x4f10c then do
        require (kind /= CpuWrite || all (disjoint (address,address + size)) [(0x4f0f8,0x4f0fc),(selectedPhysical,selectedPhysical + 4)])
          "primary root0 selected descriptor or selector has an intervening writer"
        replay memory event
      else if address >= 0x96bd20 && address < 0x96bd28 then replay memory event
      else if address == 0x40010 || address == 0xd0010 then do
        require (kind == CpuRead && size == 4 && value == Just (if address == 0x40010 then 0x4004dae0 else 0x400ddaa0))
          "primary root0 header changed or read a different root"
        pure memory
      else do
        let imageCode = take 2 (codeSpans source)
        require (kind == CpuWrite && any (not . disjoint (address,address + size)) imageCode
          && maybe False (<= total installation) clock) "primary root0 code changed after installation or outside image loading"
        pure memory
    step _ _ = Left "primary root0 prelude event attribution is incomplete"

newtype Ordinal = Ordinal Word64

data ExpectedAccess = ExpectedAccess Source Word64 Word64 Word32 Word32 Ordinal

validateBody :: Context -> Context -> Context -> Word32 -> Word64 -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> [RawEvent] -> Either String ()
validateBody entry@(Context _ incoming sp lr _ _ _) selected after poolGuest descriptorPhysical beforeStack afterStack roots table pool events = do
  (entryCore0,selectedCore1,afterCore1) <- case (counts entry,counts selected,counts after) of
    ([core0,_],[_,selectedPeer],[_,afterPeer]) -> pure (core0,selectedPeer,afterPeer)
    _ -> Left "primary root0 body core count width differs"
  let stackPhysical = fromIntegral sp - 0x40000000 - 32
      stackGuest = fromIntegral sp - 32
      write offset value pc ordinal = ExpectedAccess CpuWrite (stackGuest + offset) (stackPhysical + offset) value pc (Ordinal ordinal)
      readStack offset value = ExpectedAccess CpuRead (stackGuest + offset) (stackPhysical + offset) value 0x412c8 (Ordinal 19)
      saved = [incoming !! 4,incoming !! 5,incoming !! 6,incoming !! 7,incoming !! 11,lr]
      expected = zipWith (\offset value -> write offset value 0x725b74 0) [8,12..28] saved
        ++ [ExpectedAccess CpuRead 0x96bd20 0x96bd20 0x4004dae0 0x725b88 (Ordinal 5)
           ,ExpectedAccess CpuRead 0x4004f0f8 0x4f0f8 0x412bc 0x725b90 (Ordinal 7)
           ,write 0 (sp - 4) 0x412bc 9,write 4 0x725b98 0x412bc 9
           ,readStack 0 (sp - 4),readStack 4 0x725b98
           ,ExpectedAccess CpuRead 0x96bd20 0x96bd20 0x4004dae0 0x725b9c (Ordinal 21)
           ,ExpectedAccess CpuRead (descriptorPhysical + 0x40000000) descriptorPhysical poolGuest 0x725bac (Ordinal 25)]
  poolValue <- leWordAt pool 0
  let allExpected = expected ++ [ExpectedAccess CpuRead (fromIntegral poolGuest) (fromIntegral poolGuest - 0x40000000) poolValue 0x725bb0 (Ordinal 26)]
  require (length events == length allExpected) "primary root0 ordered data access coverage differs"
  forM_ (zip allExpected events) $ \(ExpectedAccess kind guest physical value pc (Ordinal ordinal),event@(RawEvent (Event actualKind core address actualPhysical size actualValue actualPc actualClock _) _ _ _)) -> do
    require (actualKind == kind && core == Just 0 && address == guest && actualPhysical == Just physical && size == 4
      && actualValue == Just (fromIntegral value) && actualPc == Just pc
      && maybe False (\clock -> clock >= total entry + ordinal + 1
        && clock <= entryCore0 + ordinal + 1 + afterCore1
        && (if ordinal < 21 then clock <= entryCore0 + ordinal + 1 + selectedCore1
            else clock >= entryCore0 + ordinal + 1 + selectedCore1)) actualClock)
      "primary root0 data access order, source instruction or live value differs"
    checkWindow entry after event
  let clocks = [(ordinal,clock) | (ExpectedAccess _ _ _ _ _ (Ordinal ordinal),RawEvent (Event _ _ _ _ _ _ _ (Just clock) _) _ _ _) <- zip allExpected events]
  forM_ (zip clocks (drop 1 clocks)) $ \((firstOrdinal,firstClock),(secondOrdinal,secondClock)) ->
    require (if firstOrdinal == secondOrdinal then firstClock == secondClock else firstClock < secondClock)
      "primary root0 data instruction groups have inconsistent clocks"
  let peerCounts = [clock - entryCore0 - ordinal - 1 | (ordinal,clock) <- clocks]
  require (and (zipWith (<=) peerCounts (drop 1 peerCounts))) "primary root0 reconstructed peer progress decreased"
  regions <- sequence [memoryRegion stackPhysical beforeStack,memoryRegion 0x96bd20 roots,memoryRegion 0x4dae0 table
    ,memoryRegion (fromIntegral poolGuest - 0x40000000) pool]
  finalMemory <- foldM replay (M.unions regions) events
  expectedStack <- memoryRegion stackPhysical afterStack
  require (all (\(key,value) -> M.lookup key finalMemory == Just value) (M.toList expectedStack)) "primary root0 dynamic saved-word readback differs"

data SemanticContext = SemanticContext Word32 [Word32] Word32 Word32 Word32 [[Word32]] deriving (Eq)

semanticContext :: Context -> SemanticContext
semanticContext (Context pc registers sp lr cpsr _ banks) = SemanticContext pc registers sp lr cpsr banks

data Semantics = Semantics SemanticContext SemanticContext SemanticContext [Views] [CodeView] Decoder [Alias] deriving (Eq)

semantics :: Consumer -> Semantics
semantics body = Semantics (semanticContext (entryContext body)) (semanticContext (selectedContext body)) (semanticContext (afterContext body))
  [rootsBefore body,rootsAfter body,tableBefore body,tableAfter body,stackBefore body,stackAfter body,poolBefore body,poolAfter body]
  (codeViews body) (decoder body) (aliases body)

compareReceipts :: Contract -> Receipt -> Contract -> Receipt -> Either String ()
compareReceipts first@(Contract retained _) firstReceipt second@(Contract owned _) secondReceipt = do
  require (PI.contractRole retained == PI.RetainedPrimary && PI.contractRole owned == PI.OwnedPrimary)
    "primary root0 comparison requires retained then owned roles"
  validateReceipt first firstReceipt
  validateReceipt second secondReceipt
  PI.compareReceipts retained (nestedInitializer firstReceipt) owned (nestedInitializer secondReceipt)
  firstBody <- maybe (Left "retained primary root0 consumer is absent") Right (consumer firstReceipt)
  secondBody <- maybe (Left "owned primary root0 consumer is absent") Right (consumer secondReceipt)
  firstContinuation <- maybe (Left "retained primary root0 continuation is absent") Right (continuation firstReceipt)
  secondContinuation <- maybe (Left "owned primary root0 continuation is absent") Right (continuation secondReceipt)
  require (rootsStart firstContinuation == rootsStart secondContinuation) "primary root0 continuation root preimage differs"
  require (semantics firstBody == semantics secondBody) "primary root0 consumer semantic output differs"
