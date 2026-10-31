{-# LANGUAGE OverloadedStrings #-}
module Emulation.InterfaceInitializerConsumer
  ( ConsumerKind (..), Contract, deriveContract, contractValue, validateImage4
  , validateReceipt, compareReceipts
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, withText, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (sort)
import Data.Word (Word32, Word64)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import Emulation.InterfaceInitializer
  ( Context (..), Views (..), Source (..), ByteOrder (..), Event (..), Mode (..)
  , PhysicalSpan (..), Window (..), BankSnapshot (..), TcmSnapshot (..), WatchClass (..)
  , FixedViews (..), NoFault (..), Rule (..), Watch (..), Alias (..), Decoder (..)
  )
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)


data ConsumerKind = InterruptMask deriving (Eq)
instance FromJSON ConsumerKind where
  parseJSON = withText "initializer consumer kind" $ \kind -> case kind of
    "interrupt-mask" -> pure InterruptMask
    _ -> fail "unsupported initializer consumer"

data Contract = Contract I.Contract F.Sha256 F.Sha256 F.Sha256

data CodeSite = Stage4 | Wrapper | Dispatch | Service deriving (Eq)

codeStart :: CodeSite -> Word32
codeStart site = case site of
  Stage4 -> 0x69cdf8
  Wrapper -> 0x725dd8
  Dispatch -> 0x7235d8
  Service -> 0xd1140

instance FromJSON CodeSite where
  parseJSON value = do
    address <- parseJSON value
    case (address :: Word32) of
      0x69cdf8 -> pure Stage4
      0x725dd8 -> pure Wrapper
      0x7235d8 -> pure Dispatch
      0xd1140 -> pure Service
      _ -> fail "unsupported consumer code region"

data CodeRegion = CodeRegion CodeSite [Word32]

codeRegions :: [CodeRegion]
codeRegions =
  [ CodeRegion Stage4
      [ 0xe92d4830,0xe28db00c,0xe24dd020,0xe3500000,0x1a00000d,0xebfee8dc,0xe3500001,0x1a000006
      , 0xe3022e00,0xe3402035,0xe30e0c00,0xe3400089,0xe3a01000,0xe1a02122,0xeb0214da,0xeb0222f2
      , 0xe3500000,0x0a00004c,0xeafffffe,0xe3500001,0x1a000010,0xe3a03068,0xe50b3028,0xe3013960
      , 0xe3403078,0xe50b3024,0xe3a03001,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30d3160,0xe3403069
      , 0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223d2,0xea000039,0xe3500002,0x030c4c6c
      , 0x03404069,0x0a000035,0xe3500003,0x1a000010,0xe3a03066,0xe50b3028,0xe3013968,0xe3403078
      , 0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30c3bf0,0xe3403069,0xe50b3018
      , 0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223bb,0xea000022,0xe3500004,0x13a04000,0x1a00001f
      , 0xe3a03067,0xe50b3028,0xe3013970,0xe3403078,0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a05b01
      , 0xe50b501c,0xe30c3bf0,0xe3403069,0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223a7
      ]
  , CodeRegion Wrapper
      [0xe92d48f0,0xe28db014,0xe24dd020,0xe1a04000,0xe30a3d9c,0xe340307c,0xe1d020b0,0xe2422001,0xe7936102,0xebfff5f5,0xe1a07000]
  , CodeRegion Dispatch
      [0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000,0xe3013618,0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000]
  , CodeRegion Service
      [0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001,0xe2000080,0xe28bd000,0xe8bd0800,0xe12fff1e]
  ]

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

deriveContract :: I.Contract -> Either String Contract
deriveContract initializer = do
  source <- F.parseSha256 "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
  loader <- F.parseSha256 "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
  image <- F.parseSha256 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  require (I.contractSourceSha256 initializer == source) "consumer requires the canonical original DAT source"
  pure (Contract initializer source loader image)

contractValue :: Contract -> Value
contractValue (Contract initializer source loader image) = object
  [ "schema" .= (7 :: Int), "initializer" .= I.contractValue initializer
  , "consumer" .= object
      [ "kind" .= ("interrupt-mask" :: String), "source_sha256" .= F.sha256Hex source
      , "loader_b_sha256" .= F.sha256Hex loader, "image4_sha256" .= F.sha256Hex image
      , "code" .= [object ["start" .= codeStart site,"words" .= words'] | CodeRegion site words' <- codeRegions]
      ]
  ]

validateImage4 :: Contract -> B.ByteString -> Either String ()
validateImage4 (Contract _ _ _ digest) image = do
  require (sha256Hex image == F.sha256Hex digest) "consumer image4 source digest differs"
  forM_ (take 3 codeRegions) $ \(CodeRegion site words') -> do
    observed <- mapM (leWordAt image . (fromIntegral (codeStart site) - 0x61c400 +)) [0,4 .. 4 * length words' - 4]
    require (observed == words') "consumer image4 source instructions differ"

data CodeViews = CodeViews CodeSite Views deriving (Eq)
instance FromJSON CodeViews where
  parseJSON = withObject "consumer code views" $ \row -> CodeViews <$> row .: "start" <*> row .: "views"

newtype ServiceWords = ServiceWords [Word32] deriving (Eq)
instance FromJSON ServiceWords where
  parseJSON value = do
    words' <- parseJSON value
    let expected = [word | CodeRegion Service words'' <- codeRegions,word <- words'']
    if words' == expected then pure (ServiceWords words') else fail "consumer service alias words differ"

data ServiceAlias = ServiceAlias Int Word64 Word64 ServiceWords deriving (Eq)
instance FromJSON ServiceAlias where
  parseJSON = withObject "consumer service alias" $ \row ->
    ServiceAlias <$> row .: "core" <*> row .: "guest" <*> row .: "physical" <*> row .: "words"

data EmptyEvidence = EmptyEvidence deriving (Eq)
instance FromJSON EmptyEvidence where
  parseJSON = J.withArray "empty consumer evidence" $ \items ->
    if null items then pure EmptyEvidence else fail "unsupported consumer watch counters or coverage"

newtype AccessBytes = AccessBytes B.ByteString deriving (Eq)
instance FromJSON AccessBytes where
  parseJSON value = do
    bytes <- parseJSON value
    if length bytes == 4 then pure (AccessBytes (B.pack bytes)) else fail "consumer access preimage width differs"

newtype WatchTick = WatchTick Word64 deriving (Eq, Ord)
instance FromJSON WatchTick where
  parseJSON value = WatchTick <$> parseJSON value

data TcmBank = Atcm | Btcm deriving (Eq)
instance FromJSON TcmBank where
  parseJSON = withText "consumer event TCM bank" $ \bank -> case bank of
    "atcm" -> pure Atcm
    "btcm" -> pure Btcm
    _ -> fail "unsupported consumer event TCM bank"

newtype BankOffset = BankOffset Word64 deriving (Eq)
instance FromJSON BankOffset where
  parseJSON value = BankOffset <$> parseJSON value

data TcmLocation = TcmLocation TcmBank BankOffset deriving (Eq)
instance FromJSON TcmLocation where
  parseJSON = withObject "consumer event TCM location" $ \row -> TcmLocation <$> row .: "bank" <*> row .: "offset"

data ConsumerEvent = ConsumerEvent Event (Maybe AccessBytes) WatchTick (Maybe TcmLocation) deriving (Eq)
instance FromJSON ConsumerEvent where
  parseJSON value = withObject "complete consumer event" (\row ->
    ConsumerEvent <$> parseJSON value <*> row .: "before" <*> row .: "tick" <*> row .:? "tcm") value

eventTick :: ConsumerEvent -> WatchTick
eventTick (ConsumerEvent _ _ tick _) = tick

data ConsumerWatch = ConsumerWatch (Watch ConsumerEvent) [(Word64,Word64)] EmptyEvidence EmptyEvidence deriving (Eq)
instance FromJSON ConsumerWatch where
  parseJSON value = withObject "complete consumer watch" (\row ->
    ConsumerWatch <$> parseJSON value <*> row .: "ranges" <*> row .: "counters" <*> row .: "coverage") value

data ConsumerReceipt = ConsumerReceipt ConsumerKind Context Context Context Context Context Views Views Views Views Views Views Views [CodeViews] Decoder [Alias] [ServiceAlias] ConsumerWatch ConsumerWatch deriving (Eq)
instance FromJSON ConsumerReceipt where
  parseJSON = withObject "initializer consumer evidence" $ \row ->
    ConsumerReceipt <$> row .: "kind" <*> row .: "start" <*> row .: "dispatch" <*> row .: "entered" <*> row .: "returned" <*> row .: "after"
      <*> row .: "roots" <*> row .: "slot_before" <*> row .: "slot_after" <*> row .: "frame_before" <*> row .: "frame_after"
      <*> row .: "table_before" <*> row .: "table_after" <*> row .: "code" <*> row .: "decoder" <*> row .: "aliases"
      <*> row .: "service_aliases" <*> row .: "prelude_watch" <*> row .: "watch"

data Result = Passed | Failed deriving (Eq)
instance FromJSON Result where
  parseJSON = withText "consumer result" $ \result -> case result of
    "passed" -> pure Passed
    "failed" -> pure Failed
    _ -> fail "unknown consumer result"

newtype InitializerEvidence = InitializerEvidence B.ByteString
instance FromJSON InitializerEvidence where
  parseJSON value = pure (InitializerEvidence (L.toStrict (J.encode value)))

data Receipt = Receipt Word32 Result (Maybe String) (Maybe String) InitializerEvidence (Maybe ConsumerReceipt)
instance FromJSON Receipt where
  parseJSON = withObject "consumer receipt" $ \row -> Receipt <$> row .: "schema" <*> row .: "result" <*> row .: "error" <*> row .: "machine_failure" <*> row .: "initializer" <*> row .: "consumer"

viewsBytes :: Views -> Either String B.ByteString
viewsBytes (Views virtual physical) = do
  require (virtual == physical) "consumer virtual and physical readback differs"
  pure (B.pack physical)

contextCounts :: Context -> [Word64]
contextCounts (Context _ _ _ _ _ counts _) = counts

contextTcm :: Context -> [[Word32]]
contextTcm (Context _ _ _ _ _ _ tcm) = tcm

snapshotsFor :: Context -> Either String [TcmSnapshot]
snapshotsFor context = case contextTcm context of
  [[a0,b0],[a1,b1]] -> sequence
    [TcmSnapshot 0 <$> I.bankSnapshot a0 <*> I.bankSnapshot b0,TcmSnapshot 1 <$> I.bankSnapshot a1 <*> I.bankSnapshot b1]
  _ -> Left "consumer requires both complete raw TCM bank pairs"

codeSpans :: [(Word64,Word64)]
codeSpans = [(fromIntegral (codeStart site),fromIntegral (codeStart site) + 4 * fromIntegral (length words')) | CodeRegion site words' <- codeRegions]

physical :: Word64 -> Word64 -> PhysicalSpan
physical low high = PhysicalSpan [low,high]

normalizeClass :: WatchClass -> WatchClass
normalizeClass (WatchClass range kinds) = WatchClass range (sort kinds)

normalizeRule :: Rule -> Rule
normalizeRule (Rule cores kind mode spans tcm) = Rule (sort cores) kind mode (sort spans) (sort tcm)

validateWatch :: [TcmSnapshot] -> [WatchClass] -> [Rule] -> ConsumerWatch -> Either String [ConsumerEvent]
validateWatch expectedTcm expectedClasses expectedRules (ConsumerWatch (Watch enabled violation events rules classes FixedViews NoFault tcm) ranges EmptyEvidence EmptyEvidence) = do
  require (enabled && violation == Nothing && sort tcm == sort expectedTcm) "consumer continuous watch or raw TCM scope differs"
  require (sort (map normalizeClass classes) == sort (map normalizeClass expectedClasses)) "consumer watch classes differ"
  require (map normalizeRule rules == map normalizeRule expectedRules) "consumer confinement rules differ"
  require (sort ranges == sort [range | WatchClass (Window range) _ <- expectedClasses]) "consumer watch ranges differ"
  forM_ events validateEventMetadata
  let ticks = map eventTick events
  require (and (zipWith (<=) ticks (drop 1 ticks))) "consumer event ticks decreased"
  pure events

validateEventMetadata :: ConsumerEvent -> Either String ()
validateEventMetadata (ConsumerEvent (Event source _ _ _ width value _ _ _) before _ tcm) = do
  require (tcm == Nothing && width == 4) "consumer event is not a physical RAM word access"
  case source of
    CpuFetch -> require (before == Nothing && value == Nothing) "consumer fetch contains a data preimage or value"
    CpuRead -> case before of
      Nothing -> Left "consumer read preimage is absent"
      Just (AccessBytes bytes) ->
        require (fmap fromIntegral (leWordAt bytes 0) == maybe (Left "consumer read value is absent") Right value) "consumer read preimage differs from its little-endian value"
    CpuWrite -> require (before /= Nothing) "consumer write preimage is absent"
    _ -> Left "consumer event has an unsupported non-CPU source"

validateReceipt :: Contract -> B.ByteString -> Either String ()
validateReceipt contract bytes = validateEvidence contract bytes >> pure ()

validateEvidence :: Contract -> B.ByteString -> Either String ConsumerReceipt
validateEvidence (Contract initializer _ _ _) bytes = do
  require (B.length bytes <= 32 * 1024 * 1024) "consumer receipt exceeds 32 MiB"
  document <- decodeStrictIntegers bytes
  Receipt schema result failure machineFailure (InitializerEvidence nested) evidence <- case J.fromJSON document of
    J.Error message -> Left message
    J.Success receipt -> Right receipt
  validated <- I.validateReceiptEvidence initializer nested
  require (schema == 7 && result == Passed && failure == Nothing && machineFailure == Nothing) "consumer receipt did not pass its distinct schema-7 role"
  consumer <- maybe (Left "consumer evidence is absent") Right evidence
  validateConsumer initializer validated consumer
  pure consumer

validateConsumer :: I.Contract -> I.ValidatedInitializer -> ConsumerReceipt -> Either String ()
validateConsumer initializer validated (ConsumerReceipt InterruptMask start dispatch entered returned after roots slotBefore slotAfter frameBefore frameAfter tableBefore tableAfter code decoder aliases serviceAliases prelude watch) = do
  require (start == I.initializerExit validated) "consumer does not continue the independently validated initializer exit"
  let Context dispatchPc incoming sp lr cpsr dispatchCounts tcm = dispatch
      Context enteringPc entering enteringSp enteringLr enteringCpsr enteringCounts enteringTcm = entered
      Context returnedPc outgoing outgoingSp outgoingLr outgoingCpsr returnedCounts returnedTcm = returned
      Context afterPc final finalSp finalLr finalCpsr afterCounts finalTcm = after
  require (length incoming == 13 && length entering == 13 && length outgoing == 13 && length final == 13) "consumer register width differs"
  require (dispatchPc == 0x723600 && sp == 0x400f3388 && lr == 0x7235f8 && incoming !! 11 == 0x400f33a4
    && incoming !! 0 == 1 && incoming !! 3 == 0x400ddaa0 && incoming !! 4 == 1 && incoming !! 5 == 0x96bd20
    && cpsr .&. 0x07f0fe20 == 0 && cpsr .&. 0xdf == 0xd3) "consumer source dispatch inputs or supported A32 little-endian SVC CPSR differ"
  let targetRegisters = take 3 incoming ++ [0xd1140] ++ drop 4 incoming
      mask = cpsr .&. 0x80
      masked = cpsr .|. 0x80
      returnedRegisters = [mask,masked] ++ drop 2 targetRegisters
      finalRegisters = take 10 returnedRegisters ++ [mask] ++ drop 11 returnedRegisters
  require (enteringPc == 0xd1140 && entering == targetRegisters && enteringSp == sp && enteringLr == 0x723608 && enteringCpsr == cpsr && enteringTcm == tcm) "consumer target load or BLX state differs"
  require (returnedPc == 0x723608 && outgoing == returnedRegisters && outgoingSp == sp && outgoingLr == 0x723608 && outgoingCpsr == masked && returnedTcm == tcm) "consumer leaf return ABI differs"
  require (afterPc == 0x72360c && final == finalRegisters && finalSp == sp && finalLr == 0x723608 && finalCpsr == masked && finalTcm == tcm) "consumer caller copy or final stop differs"
  require (contextTcm start == tcm) "consumer prelude changed TCM bank registers"
  forM_ [start,dispatch,entered,returned,after] $ \context -> do
    let counts = contextCounts context
    require (length counts == 2 && all (<= 50000000) counts && sum counts <= 50000000) "consumer global instruction budget or counter bounds differ"
  case (contextCounts start,dispatchCounts,enteringCounts,returnedCounts,afterCounts) of
    ([s0,s1],[d0,d1],[e0,e1],[r0,r1],[a0,a1]) ->
      require (d0 >= s0 && d1 > s1 && e0 >= d0 && e1 == d1 + 2 && r0 >= e0 && r1 == e1 + 9 && a0 >= r0 && a1 == r1 + 1) "consumer actual per-core instruction counts differ"
    _ -> Left "consumer per-core count width differs"
  rootBytes <- viewsBytes roots
  require (B.length rootBytes == 8 && leWordAt rootBytes 0 == Right 0x4004dae0 && leWordAt rootBytes 4 == Right 0x400ddaa0) "consumer root pair differs"
  slotBytes <- viewsBytes slotBefore
  _ <- viewsBytes slotAfter
  require (slotBefore == slotAfter && B.length slotBytes == 4 && leWordAt slotBytes 0 == Right 0xd1140) "consumer published InterruptMask slot differs"
  beforeFrame <- viewsBytes frameBefore
  afterFrame <- viewsBytes frameAfter
  require (B.length beforeFrame == 36 && B.length afterFrame == 36 && B.drop 4 beforeFrame == B.drop 4 afterFrame
    && leWordAt afterFrame 0 == Right (incoming !! 11)) "consumer saved-r11 or complete caller frame differs"
  forM_ (zip [4,8 .. 32] [0x400f33e4,0x400,2,incoming !! 7,incoming !! 8,incoming !! 10,0x400f33dc,0x725e00]) $ \(offset,value) ->
    require (leWordAt beforeFrame offset == Right value) "consumer caller frame does not match the authentic task103 source path"
  beforeTable <- viewsBytes tableBefore
  _ <- viewsBytes tableAfter
  require (tableBefore == I.initializerTable validated && tableBefore == tableAfter && B.length beforeTable == 1419 * 4
    && B.take 4 (B.drop 0x164 beforeTable) == slotBytes) "consumer complete published table differs from independently validated initializer evidence"
  require (length code == length codeRegions) "consumer code region coverage differs"
  forM_ (zip codeRegions code) $ \(CodeRegion source words',CodeViews observed views) -> do
    body <- viewsBytes views
    actual <- mapM (leWordAt body) [0,4 .. 4 * length words' - 4]
    require (observed == source && B.length body == 4 * length words' && actual == words') "consumer code source readback differs"
  require (decoder == Decoder [1] [Nothing]) "consumer decoder identity or pending conversion differs"
  expectedTcm <- snapshotsFor start
  validateAliases expectedTcm roots slotBefore slotAfter frameBefore frameAfter tableBefore tableAfter code aliases serviceAliases
  let rule cores kind mode spans = Rule cores kind mode spans [snapshot | snapshot@(TcmSnapshot core _ _) <- expectedTcm,core `elem` cores]
      writes = [CpuWrite,HostWrite,ModelBusWrite]
      immutable = map (uncurry physical) ([(0xddaa0,0xdf0cc),(0x96bd20,0x96bd28),(0x7caf34,0x7caf38)] ++ codeSpans ++ [(0xd0ab0,0xd0af0)])
      bodyReads = [physical 0xd0430 0xd09ac | I.contractRole initializer == I.Owned]
      ownedFetch = [rule [1] CpuFetch Forbid bodyReads | I.contractRole initializer == I.Owned]
      protection = [WatchClass (Window span') writes | span' <- codeSpans ++ [(0xd0ab0,0xd0af0)]]
      preludeClasses =
        [WatchClass (Window (0xddaa0,0xdf0cc)) (CpuRead:writes),WatchClass (Window (0x96bd20,0x96bd28)) (CpuRead:writes)]
        ++ [WatchClass (Window span') (CpuFetch:writes) | span' <- take 3 codeSpans]
        ++ [WatchClass (Window (0xd1140,0xd1164)) writes,WatchClass (Window (0xd0ab0,0xd0af0)) (CpuFetch:writes)
           ,WatchClass (Window (0x7caf34,0x7caf38)) (CpuRead:writes),WatchClass (Window (0xc080c,0xc0810)) [CpuRead]]
      preludeRules =
        [rule [0,1] CpuWrite Forbid immutable,rule [1] CpuRead Forbid (physical 0xddaa0 0xdf0cc : bodyReads)] ++ ownedFetch
      consumerRules =
        [rule [1] CpuWrite Allow [physical 0xf3384 0xf3388],rule [0] CpuWrite Forbid (immutable ++ [physical 0xf3384 0xf33a8])]
        ++ [rule [1] CpuRead Forbid bodyReads | I.contractRole initializer == I.Owned] ++ ownedFetch
      consumerClasses =
        [WatchClass (Window (0xddaa0,0xdf0cc)) (CpuRead:writes),WatchClass (Window (0x96bd20,0x96bd28)) writes
        ,WatchClass (Window (0xf3384,0xf33a8)) (CpuRead:writes)] ++ protection
  preludeEvents <- validateWatch expectedTcm preludeClasses preludeRules prelude
  validatePrelude start dispatch beforeTable preludeEvents
  events <- validateWatch expectedTcm consumerClasses consumerRules watch
  validateEvents dispatch entered returned after incoming beforeFrame events
  case (reverse preludeEvents,events) of
    (lastPrelude:_,firstConsumer:_) ->
      require (eventTick lastPrelude <= eventTick firstConsumer) "consumer event ticks decreased across watch windows"
    _ -> Left "consumer watch event coverage is incomplete"

validatePrelude :: Context -> Context -> B.ByteString -> [ConsumerEvent] -> Either String ()
validatePrelude start dispatch table events = do
  let lower = sum (contextCounts start)
      upper = sum (contextCounts dispatch)
      clocks = [clock | ConsumerEvent (Event _ _ _ _ _ _ _ (Just clock) _) _ _ _ <- events]
      fetched = [pc | ConsumerEvent (Event CpuFetch (Just 1) _ _ _ _ (Just pc) _ _) _ _ _ <- events]
      expectedFetches = [0xd0ab0,0xd0ab4 .. 0xd0aec]
        ++ [0x69cdf8,0x69cdfc .. 0x69ce08]
        ++ [0x69ce44,0x69ce48,0x69ce90,0x69ce94,0x69ce98,0x69ce9c,0x69cea0,0x69cea4]
        ++ [0x69ceec,0x69cef0 .. 0x69cf34]
        ++ [0x725dd8,0x725ddc .. 0x725dfc]
        ++ [0x7235d8,0x7235dc .. 0x723600]
      core1Reads = [(address,value,pc) | ConsumerEvent (Event CpuRead (Just 1) address _ _ (Just value) (Just pc) _ _) _ _ _ <- events]
      roots = [(address,value,pc) | (address,value,pc) <- core1Reads,address >= 0x96bd20 && address < 0x96bd28]
      selector = [(address,value,pc) | (address,value,pc) <- core1Reads,address == 0x7caf34]
      magic = [(value,pc) | (address,value,pc) <- core1Reads,address == 0x400c080c]
  require (fetched == expectedFetches) "consumer prelude source execution path differs"
  require (roots == [(0x96bd20,0x4004dae0,0x7235e8),(0x96bd24,0x400ddaa0,0x7235fc)]
    && selector == [(0x7caf34,2,0x725df8)]) "consumer prelude root or task103 selector reads differ"
  require (not (null magic) && last magic == (0x87654321,0xd391c)
    && all (\(_,pc) -> pc `elem` [0xd38b8,0xd38cc,0xd391c]) magic) "consumer prelude lacks the natural peer magic wait completion"
  require (length clocks == length events && all (>= lower) clocks
    && and (zipWith (<=) clocks (drop 1 clocks))) "consumer prelude event clocks differ"
  forM_ events $ \(ConsumerEvent (Event source core address physicalAddress width value pc clock order) _ _ _) -> do
    require (core == Just 0 || core == Just 1) "consumer prelude event has no supported CPU attribution"
    require (width == 4 && order == Just LittleEndian) "consumer prelude access width or byte order differs"
    let targetFetch = source == CpuFetch && pc == Just 0x723600
        upperBound = upper + if targetFetch then 1 else 0
    require (if targetFetch && core == Just 1 then clock == Just (upper + 1) else maybe False (<= upperBound) clock) "consumer preexecution fetch or completed instruction clock differs"
    case source of
      CpuFetch -> do
        let declared = any (\(low,high) -> address >= low && address < high && address .&. 3 == 0) ((0xd0ab0,0xd0af0):take 3 codeSpans)
        require (physicalAddress == Just address && pc == Just (fromIntegral address) && declared) "consumer prelude fetched outside its source declarations"
      CpuRead -> do
        require (pc /= Nothing && value /= Nothing) "consumer prelude read attribution is incomplete"
        translated <- maybe (Left "consumer prelude read has no physical binding") Right physicalAddress
        let isRoot = translated == 0x96bd20 || translated == 0x96bd24
            isSelector = translated == 0x7caf34
            isMagic = translated == 0xc080c
            isTable = core == Just 0 && translated >= 0xddaa0 && translated < 0xdf0cc && translated .&. 3 == 0
        require (isRoot || isSelector || isMagic || isTable) "consumer prelude read a forbidden published-table or unclassified resource"
        require (address == translated || address == translated + 0x40000000) "consumer prelude read physical binding differs"
        require (not isRoot || value == Just (if translated == 0x96bd20 then 0x4004dae0 else 0x400ddaa0)) "consumer prelude root value differs"
        require (not isSelector || value == Just 2) "consumer prelude selector value differs"
        require (not isTable || fmap fromIntegral (leWordAt table (fromIntegral (translated - 0xddaa0))) == maybe (Left "consumer table read value is absent") Right value) "consumer prelude peer table read differs"
      _ -> Left "consumer prelude changed a protected resource"

validateEvents :: Context -> Context -> Context -> Context -> [Word32] -> B.ByteString -> [ConsumerEvent] -> Either String ()
validateEvents dispatch entered returned after incoming frame events = case events of
  [slot,saved,restored] -> do
    check slot CpuRead 0x400ddc04 0xddc04 0xd1140 0x723600 (sum (contextCounts dispatch)) (sum (contextCounts entered))
    check saved CpuWrite 0x400f3384 0xf3384 (incoming !! 11) 0xd1140 (sum (contextCounts entered)) (sum (contextCounts returned))
    check restored CpuRead 0x400f3384 0xf3384 (incoming !! 11) 0xd115c (sum (contextCounts entered)) (sum (contextCounts returned))
    let ConsumerEvent _ savedBefore _ _ = saved
    require (savedBefore == Just (AccessBytes (B.take 4 frame))) "consumer saved-r11 write preimage differs from the actual caller-frame snapshot"
    let clocks = [clock | ConsumerEvent (Event _ _ _ _ _ _ _ (Just clock) _) _ _ _ <- events]
    require (length clocks == 3 && and (zipWith (<) clocks (drop 1 clocks)) && sum (contextCounts returned) < sum (contextCounts after)) "consumer event instruction clocks differ"
  _ -> Left "consumer CPU data events are not exactly slot-read, saved-r11 write and saved-r11 read"
  where
    check (ConsumerEvent (Event source core address physicalAddress width value pc clock order) _ _ _) expectedSource guest physicalAddress' expectedValue expectedPc lower upper =
      require (source == expectedSource && core == Just 1 && address == guest && physicalAddress == Just physicalAddress' && width == 4
        && value == Just (fromIntegral expectedValue) && pc == Just expectedPc && order == Just LittleEndian && maybe False (\n -> n >= lower && n < upper) clock) "consumer CPU data event attribution or value differs"

validateAliases :: [TcmSnapshot] -> Views -> Views -> Views -> Views -> Views -> Views -> Views -> [CodeViews] -> [Alias] -> [ServiceAlias] -> Either String ()
validateAliases snapshots roots slotBefore slotAfter frameBefore frameAfter tableBefore tableAfter code aliases serviceAliases = do
  let resources slot frame table = [(0x96bd20,roots),(0xddc04,slot),(0xf3384,frame),(0xddaa0,table)] ++ [(fromIntegral (codeStart site),views) | CodeViews site views <- code,site /= Service]
  before <- bindings (resources slotBefore frameBefore tableBefore)
  after <- bindings (resources slotAfter frameAfter tableAfter)
  require (aliases == before ++ after) "consumer dispatch/final low/high alias bindings differ from actual physical readbacks"
  let words' = ServiceWords [word | CodeRegion Service instructions <- codeRegions,word <- instructions]
      expectedService = concat (replicate 2 [ServiceAlias core (0xd1140 + alias) 0xd1140 words' | core <- [0,1],alias <- [0,0x40000000]])
      extents = [(core,guest,size) | Alias core guest _ size _ <- aliases] ++ [(core,guest,36) | ServiceAlias core guest _ _ <- serviceAliases]
  require (serviceAliases == expectedService) "consumer dispatch/final direct-word service alias bindings differ"
  forM_ extents $ \(core,guest,size) ->
    forM_ [bank | TcmSnapshot selected atcm btcm <- snapshots,selected == core,bank <- [atcm,btcm]] $ \bank -> case bank of
      BankSnapshot _ Nothing -> pure ()
      BankSnapshot _ (Just (Window (low,high))) ->
        require (guest + size <= low || high <= guest) "consumer low/high alias overlaps an enabled TCM bank"
  where
    bindings resources = do
      digests <- mapM (\(base,views) -> do
        bytes <- viewsBytes views
        digest <- F.parseSha256 (sha256Hex bytes)
        pure (base,fromIntegral (B.length bytes),digest)) resources
      pure [Alias core (base + alias) base size digest | core <- [0,1],(base,size,digest) <- digests,alias <- [0,0x40000000]]

data ConsumerSemantics = ConsumerSemantics Context Context Context Context Views Views Views Views Views Views Views [CodeViews] Decoder [Alias] [ServiceAlias] [ConsumerEvent] deriving (Eq)

compareReceipts :: (Contract,B.ByteString) -> (Contract,B.ByteString) -> Either String ()
compareReceipts (retained@(Contract first _ _ _),retainedBytes) (owned@(Contract second _ _ _),ownedBytes) = do
  require (I.contractRole first == I.Retained && I.contractRole second == I.Owned) "consumer comparison requires retained then owned initializer roles"
  firstEvidence <- validateEvidence retained retainedBytes
  secondEvidence <- validateEvidence owned ownedBytes
  require (semantic firstEvidence == semantic secondEvidence) "retained and owned actual consumer inputs, outputs or effects differ"
  where
    semantic (ConsumerReceipt InterruptMask _ dispatch entered returned after roots slotBefore slotAfter frameBefore frameAfter tableBefore tableAfter code decoder aliases serviceAliases _ (ConsumerWatch (Watch _ _ events _ _ _ _ _) _ _ _)) =
      let origin = contextCounts dispatch
          relative = relativeContext origin
       in ConsumerSemantics (relative dispatch) (relative entered) (relative returned) (relative after) roots slotBefore slotAfter frameBefore frameAfter tableBefore tableAfter code decoder aliases serviceAliases (relativeEvents (sum origin) events)
    relativeContext origin (Context pc registers sp lr cpsr counts tcm) = Context pc registers sp lr cpsr (zipWith (-) counts origin) tcm
    relativeEvents _ [] = []
    relativeEvents origin events@(ConsumerEvent _ _ tickOrigin _ : _) = map (relativeEvent origin tickOrigin) events
    relativeEvent origin (WatchTick tickOrigin) (ConsumerEvent (Event source core address physicalAddress size value pc clock order) before (WatchTick tick) tcm) =
      ConsumerEvent (Event source core address physicalAddress size value pc (subtract origin <$> clock) order) before (WatchTick (tick - tickOrigin)) tcm
