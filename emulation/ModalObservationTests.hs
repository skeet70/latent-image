{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecode, encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Text as T
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import Data.Either (isLeft)
import Data.Word (Word32)
import Emulation.ModalObservation
import Emulation.WatchObservation
import EmulatorUsb (BridgeExchange, ModalResponse (..), bridgeModalObservationBegin, bridgeModalObservationReport, bridgeModalObservationEnd)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.UiTrajectory (checkModalObserverConfiguration, decodeTrajectories)
import qualified FujiAF.RamPush as Ram
import System.Exit (exitFailure)

range :: Word32 -> Word32 -> WatchRange
range low high = either error id (watchRange (watchAddress low) (watchAddress high))

limit :: WatchLimit
limit = either error id (watchLimit 4096)

cpu :: Int -> Word32 -> Word32 -> Int -> Integer -> Value
cpu core address physical bytes clock = object
  [ "source" .= ("cpu-write" :: String), "address" .= address, "physical" .= physical
  , "size" .= bytes, "value" .= (0 :: Int), "before" .= replicate bytes (0 :: Int)
  , "byte_order" .= ("little" :: String), "core" .= core, "pc" .= (0x10000 + core * 4)
  , "instructions" .= clock, "tick" .= clock
  ]

transfer :: String -> Word32 -> Int -> Value
transfer source physical bytes = object
  [ "source" .= source, "address" .= physical, "physical" .= physical, "size" .= bytes
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null
  ]

observe :: [WatchRange] -> [Value] -> Either String ArrayWriteReport
observe ranges records = do
  let spans = [let (low, high) = watchRangeBounds span_ in [low, high] | span_ <- ranges]
      classes = [object ["range" .= span_, "kinds" .= (["cpu-write", "host-write", "model-bus-write"] :: [String])] | span_ <- spans]
      watchedSpans = [let (low, high) = watchRangeBounds span_ in [low, high] | span_ <- unionWatchRanges ranges]
  acknowledgement <- parseFixedWatchAck (object
    [ "armed" .= True, "ranges" .= watchedSpans, "views" .= ("fixed" :: String), "classes" .= classes ])
  observation <- parseFixedWatchObservation limit (object
    [ "enabled" .= True, "ranges" .= watchedSpans, "records" .= records, "violation" .= Null
    , "views" .= ("fixed" :: String), "classes" .= classes ])
  summarizeArrayWrites limit ranges [] acknowledgement observation


set :: K.Key -> Value -> Value -> Value
set key value (Object fields) = Object (KM.insert key value fields)
set _ _ value = value

get :: K.Key -> Value -> Value
get key (Object fields) = maybe Null id (KM.lookup key fields)
get _ _ = Null

writerEnvelope :: [Value] -> (Value, Value)
writerEnvelope records =
  let spans = [[0x01467588, 0x01467840 :: Word32]]
      classes = [object ["range" .= span_, "kinds" .= (["cpu-write", "host-write", "model-bus-write"] :: [String])] | span_ <- spans]
  in (object ["armed" .= True, "ranges" .= spans, "views" .= ("fixed" :: String), "classes" .= classes],
      object ["enabled" .= True, "ranges" .= spans, "records" .= records, "violation" .= Null,
        "views" .= ("fixed" :: String), "classes" .= classes])

auditEnvelope :: WatchLimit -> Value -> Value -> Either String ArrayWriteReport
auditEnvelope bound ack state = do
  acknowledgement <- parseFixedWatchAck ack
  observation <- parseFixedWatchObservation bound state
  summarizeArrayWrites bound [modalArrayRange] [] acknowledgement observation

permitLimit :: ModalLimit
permitLimit = either error id (modalLimit 4)

permitCounts :: Value
permitCounts = object
  [ "observations" .= (0 :: Int), "entries" .= (0 :: Int), "owned_handoffs" .= (0 :: Int)
  , "fallback_handoffs" .= (0 :: Int), "returns" .= (0 :: Int), "active_frames" .= (0 :: Int)
  , "peak_core_depth" .= (0 :: Int) ]

permitEnvelope :: Value
permitEnvelope = object
  [ "enabled" .= False, "policy" .= ("stock-baseline" :: String), "counts" .= permitCounts
  , "stock_fetches" .= ([] :: [Value]), "captured_domains" .= ([] :: [Value]), "failure" .= Null
  , "physical_fetch_coverage" .= False, "task_identity_verified" .= False
  , "physical_guard" .= Null, "task_entries" .= ([] :: [Value]), "lifetime" .= Null, "watch" .= Null
  , "context_source_spans" .= (0 :: Int) ]

domain :: Value
domain = object
  [ "core" .= (1 :: Int), "pc" .= (0x00bfb0a0 :: Word32), "instructions" .= (18446744073709551615 :: Integer)
  , "controller" .= (0xffffffff :: Word32), "state" .= (0xffffffff :: Word32), "event" .= (0xffffffff :: Word32) ]

permitTests :: IO ()
permitTests = do
  stock <- either error pure (modalStock (B.replicate 0xfc 0))
  request <- either error pure (modalRequest stock Nothing permitLimit)
  let requestJson = modalBeginRequestJson request
  check "baseline request encodes all required null extents" $
    get "linked" requestJson == Null && get "seam" requestJson == Null && get "owned" requestJson == Null
      && get "stock" requestJson /= Null && get "command" requestJson == String "modal-observation-begin"
  check "partial stock source refuses" (isLeft (modalStock (B.replicate 0xf8 0)))
  check "zero and excessive modal limits refuse" (isLeft (modalLimit 0) && isLeft (modalLimit 65537))
  linked <- either error pure (modalLinked (watchAddress 0x10000) (B.replicate 128 0))
  let helpers = ModalHelpers (range 0x10060 0x10070) (range 0x10070 0x10080)
  scope <- either error pure (modalScope linked (range 0x10000 0x10038) (range 0x10038 0x10060) helpers (Just (watchAddress 0x10060)))
  scopedRequest <- either error pure (modalRequest stock (Just (linked, scope)) permitLimit)
  let scopedJson = modalBeginRequestJson scopedRequest
  check "separate linked owned prehook is encoded" (get "owned_prehook" scopedJson == Number 0x10060)
  check "both complete helper extents are encoded" (get "helpers" scopedJson == array
    [array [Number 0x10060, Number 0x10070], array [Number 0x10070, Number 0x10080]])
  check "stock helper extents are empty" (get "helpers" requestJson == array [])
  check "unmapped prehook refuses" (isLeft (modalScope linked (range 0x10000 0x10038) (range 0x10038 0x10060) helpers (Just (watchAddress 0x20000))))
  check "baseline request round trips" (parseModalBeginRequest requestJson == Right request)
  check "scoped request round trips" (parseModalBeginRequest scopedJson == Right scopedRequest)
  check "null optional baseline prehook is accepted" (parseModalBeginRequest (set "owned_prehook" Null requestJson) == Right request)
  let malformedRequests =
        [ ("unknown key", set "extra" Null requestJson)
        , ("wrong command", set "command" (String "other") requestJson)
        , ("non-byte source", set "stock" (toArray [Number 256]) requestJson)
        , ("partial source", set "stock" (toArray [Number 0]) requestJson)
        , ("zero limit", set "limit" (Number 0) requestJson)
        , ("u64 overflowing limit", set "limit" (Number 18446744073709551616) requestJson)
        , ("baseline linked source", set "linked" (get "linked" scopedJson) requestJson)
        , ("baseline owned prehook", set "owned_prehook" (Number 0x10060) requestJson)
        , ("scoped missing owned", set "owned" Null scopedJson)
        , ("missing helper extents", case scopedJson of Object fields -> Object (KM.delete "helpers" fields); other -> other)
        , ("empty helper extents", set "helpers" (array []) scopedJson)
        , ("partial helper list", set "helpers" (array [array [Number 0x10060, Number 0x10070]]) scopedJson)
        , ("extra helper extent", set "helpers" (array [array [Number 0x10060, Number 0x10070], array [Number 0x10070, Number 0x10078], array [Number 0x10078, Number 0x10080]]) scopedJson)
        , ("overlapping helpers", set "helpers" (array [array [Number 0x10060, Number 0x10074], array [Number 0x10070, Number 0x10080]]) scopedJson)
        , ("helper overlaps owned dispatcher", set "helpers" (array [array [Number 0x1005c, Number 0x10070], array [Number 0x10070, Number 0x10080]]) scopedJson)
        , ("helper outside linked text", set "helpers" (array [array [Number 0x10060, Number 0x10070], array [Number 0x10080, Number 0x10084]]) scopedJson)
        , ("helper entry differs from capture point", set "owned_prehook" (Number 0x10064) scopedJson)
        , ("stock request with helper code", set "helpers" (get "helpers" scopedJson) requestJson)
        , ("short seam", set "seam" (toArray [Number 0x10000, Number 0x10034]) scopedJson)
        , ("overlapping extents", set "owned" (toArray [Number 0x10034, Number 0x10060]) scopedJson)
        , ("unaligned prehook", set "owned_prehook" (Number 0x10061) scopedJson)
        , ("address overflow", set "owned_prehook" (Number 4294967296) scopedJson)
        ]
  mapM_ (\(label, value) -> check ("modal request refuses " ++ label) (isLeft (parseModalBeginRequest value))) malformedRequests
  check "linked source overflowing address space refuses" (isLeft (modalLinked (watchAddress 0xfffffff0) (B.replicate 32 0)))
  disabled <- either error pure (parseModalReport permitLimit permitEnvelope)
  check "disabled finished report is accepted" (checkModalEnd disabled == Right ())
  check "disabled begin refuses" (isLeft (checkModalBegin request disabled))
  begun <- either error pure (parseModalReport permitLimit (set "enabled" (Bool True) permitEnvelope))
  check "enabled empty baseline begins" (checkModalBegin request begun == Right ())
  stopped <- either error pure (parseModalReport permitLimit
    (set "counts" (set "observations" (Number 1) permitCounts) (set "enabled" (Bool True) permitEnvelope)))
  check "begin can sample an existing preexecution stop" (checkModalBegin request stopped == Right ())
  check "active observer cannot end" (isLeft (checkModalEnd begun))
  check "wrong policy cannot begin" (isLeft (checkModalBegin scopedRequest begun))
  let captures = set "captured_domains" (toArray [domain]) permitEnvelope
  captured <- either error pure (parseModalReport permitLimit captures)
  check "full u32 registers and u64 clock survive independently of monitor count" (modalReportJson captured == captures)
  let stockPoint = object ["core" .= (0 :: Int), "pc" .= (0x00bfb02c :: Word32), "instructions" .= (0 :: Int)]
      rejectedFetch = set "stock_fetches" (toArray [stockPoint]) (set "failure" (String "body fetch without entry") permitEnvelope)
  fetchFailure <- either error pure (parseModalReport permitLimit rejectedFetch)
  check "failed fetch evidence survives rejected monitor admission" (modalReportJson fetchFailure == rejectedFetch)
  let invalidReports =
        [ ("extra response key", set "extra" Null permitEnvelope)
        , ("missing response key", case permitEnvelope of Object fields -> Object (KM.delete "failure" fields); other -> other)
        , ("unknown policy", set "policy" (String "qualified") permitEnvelope)
        , ("physical coverage claim", set "physical_fetch_coverage" (Bool True) permitEnvelope)
        , ("task identity claim", set "task_identity_verified" (Bool True) permitEnvelope)
        , ("negative counter", set "counts" (set "entries" (Number (-1)) permitCounts) permitEnvelope)
        , ("fractional counter", set "counts" (set "observations" (Number 0.5) permitCounts) permitEnvelope)
        , ("u64 counter overflow", set "counts" (set "observations" (Number 18446744073709551616) permitCounts) permitEnvelope)
        , ("observation bound", set "counts" (set "observations" (Number 5) permitCounts) permitEnvelope)
        , ("count exceeds observations", set "counts" (set "returns" (Number 1) permitCounts) permitEnvelope)
        , ("domain bound", set "captured_domains" (toArray (replicate 5 domain)) permitEnvelope)
        , ("stock fetch bound", set "stock_fetches" (toArray (replicate 5 stockPoint)) permitEnvelope)
        , ("noncanonical stock fetch", set "stock_fetches" (toArray [set "pc" (Number 0x40bfb02c) stockPoint]) permitEnvelope)
        , ("entry branch is not stock body", set "stock_fetches" (toArray [set "pc" (Number 0x00bfb028) stockPoint]) permitEnvelope)
        , ("core outside machine", set "captured_domains" (toArray [set "core" (Number 2) domain]) permitEnvelope)
        , ("register overflow", set "captured_domains" (toArray [set "controller" (Number 4294967296) domain]) permitEnvelope)
        , ("unaligned PC", set "captured_domains" (toArray [set "pc" (Number 0x00bfb0a1) domain]) permitEnvelope)
        , ("malformed capture", set "captured_domains" (toArray [set "extra" Null domain]) permitEnvelope)
        , ("unknown failure form", set "failure" (Bool True) permitEnvelope)
        ]
  mapM_ (\(label, value) -> check ("modal report refuses " ++ label) (isLeft (parseModalReport permitLimit value))) invalidReports
  failed <- either error pure (parseModalReport permitLimit (set "failure" (String "unfinished frame") (set "enabled" (Bool True) permitEnvelope)))
  check "failed end preserves enabled evidence" (modalReportEnabled failed && modalReportFailure failed == Just "unfinished frame"
    && isLeft (checkModalEnd failed))
  unfinished <- either error pure (parseModalReport permitLimit (set "counts"
    (set "active_frames" (Number 1) (set "observations" (Number 1) permitCounts)) permitEnvelope))
  check "disabled report with unfinished frames refuses end" (isLeft (checkModalEnd unfinished))
 where
  toArray values = get "values" (object ["values" .= values])

scopedRouteTests :: IO ()
scopedRouteTests = do
  let counts = object
        [ "observations" .= (4 :: Int), "entries" .= (2 :: Int), "owned_handoffs" .= (1 :: Int)
        , "fallback_handoffs" .= (1 :: Int), "returns" .= (2 :: Int), "active_frames" .= (0 :: Int)
        , "peak_core_depth" .= (1 :: Int) ]
      captured75 = set "controller" (Number 75) domain
      capture values = get "values" (object ["values" .= values])
      scoped = set "policy" (String "scoped-owned75") (completedEnvelope 224)
      mixed = set "counts" counts (set "captured_domains" (capture [captured75]) scoped)
      withCount key value = set "counts" (set key (Number value) counts) mixed
      rejected =
        [ ("empty route", scoped)
        , ("owned-only route", set "counts" (set "owned_handoffs" (Number 2) (set "fallback_handoffs" (Number 0) counts)) mixed)
        , ("fallback-only route", set "counts" (set "owned_handoffs" (Number 0) (set "fallback_handoffs" (Number 2) counts)) mixed)
        , ("entries exceed handoffs", withCount "entries" 3)
        , ("handoffs exceed entries", withCount "owned_handoffs" 2)
        , ("missing return", withCount "returns" 1)
        , ("extra return", withCount "returns" 3)
        , ("missing captured domain", set "captured_domains" (capture ([] :: [Value])) mixed)
        , ("no controller75 domain", set "captured_domains" (capture [domain]) mixed)
        , ("stock policy", set "policy" (String "stock-baseline") mixed)
        , ("monitor failure", set "failure" (String "unfinished frame") mixed)
        , ("active monitor", set "enabled" (Bool True) mixed)
        , ("active frame", withCount "active_frames" 1)
        ]
  clean <- either error pure (parseModalReport permitLimit mixed)
  check "finished mixed scoped route is accepted" (checkScopedModalRoute clean == Right ())
  varied <- either error pure (parseModalReport permitLimit
    (set "captured_domains" (capture [domain, set "state" (Number 0) (set "event" (Number 0) captured75), captured75]) mixed))
  check "captured samples need not match entry totals or specific state/event values" (checkScopedModalRoute varied == Right ())
  stock <- either error pure (parseModalReport permitLimit permitEnvelope)
  check "stock diagnostic end accepts zero observations" (checkModalEnd stock == Right ())
  mapM_ (\(label, value) ->
    check ("scoped route refuses " ++ label) (either (const True) (isLeft . checkScopedModalRoute) (parseModalReport permitLimit value))) rejected

serve :: Value -> Value -> BridgeExchange
serve expected response request = do
  check "modal RPC sends the exact public protocol request" (eitherDecode request == Right expected)
  pure (BL.toStrict (encode response))

rpcTests :: IO ()
rpcTests = do
  stock <- either error pure (modalStock (B.replicate 0xfc 0))
  request <- either error pure (modalRequest stock Nothing permitLimit)
  let enabled = set "enabled" (Bool True) permitEnvelope
      reportCommand = object ["command" .= ("modal-observation-report" :: String)]
      endCommand = object ["command" .= ("modal-observation-end" :: String)]
  begun <- bridgeModalObservationBegin (serve (modalBeginRequestJson request) (set "ok" (Bool True) enabled)) request
  check "typed begin accepts enabled response" (either (const False) modalReportEnabled (modalParsedResponse begun))
  check "begin retains raw report" (modalRawResponse begun == enabled)
  disabled <- bridgeModalObservationReport (serve reportCommand (set "ok" (Bool True) permitEnvelope)) permitLimit
  check "typed report accepts disabled response" (either (const False) (not . modalReportEnabled) (modalParsedResponse disabled))
  ended <- bridgeModalObservationEnd (serve endCommand (set "ok" (Bool True) permitEnvelope)) permitLimit
  check "typed end accepts disabled finished response" (either (const False) (const True) (modalParsedResponse ended))
  let refusal = object ["ok" .= False, "message" .= ("active frame" :: String)]
  refused <- bridgeModalObservationEnd (serve endCommand refusal) permitLimit
  check "RPC refusal retains the complete raw reply" (modalRawResponse refused == refusal && isLeft (modalParsedResponse refused))
  activeEnd <- bridgeModalObservationEnd (serve endCommand (set "ok" (Bool True) enabled)) permitLimit
  check "RPC end cannot mistake active monitor for completion" (modalRawResponse activeEnd == enabled && isLeft (modalParsedResponse activeEnd))
  let malformed = set "extra" Null permitEnvelope
  malformedReport <- bridgeModalObservationReport (serve reportCommand (set "ok" (Bool True) malformed)) permitLimit
  check "malformed report remains available with parse failure" (modalRawResponse malformedReport == malformed && isLeft (modalParsedResponse malformedReport))
  undecodable <- bridgeModalObservationReport (\_ -> pure "\xff") permitLimit
  check "undecodable reply retains wire bytes" (get "wire_bytes" (modalRawResponse undecodable) == get "bytes" (object ["bytes" .= ([255] :: [Int])])
    && isLeft (modalParsedResponse undecodable))

check :: String -> Bool -> IO ()
check label ok = unless ok (putStrLn label >> exitFailure)

main :: IO ()
main = do
  let actual = observe [modalArrayRange]
        [ cpu 0 0x4146777a 0x0146777a 2 1
        , cpu 1 0x0146761e 0x0146761e 2 2
        , transfer "host-write" 0x01467588 2
        , transfer "model-bus-write" 0x014676e3 2
        ]
  report <- either (error . ("valid writer observation: " ++)) pure actual
  let writes = arrayWrites report
  check "all four array write operations retained" (length writes == 4)
  check "physical alias identifies event75" $ case writes of
    first : _ -> writeSource first == CpuWriter Core0 (watchAddress 0x10000)
      && map writtenArray (writeIntersections first) == [ModalEvents]
      && map (controllerIndexValue . firstController) (writeIntersections first) == [75]
      && map (controllerIndexValue . lastController) (writeIntersections first) == [75]
    _ -> False
  check "second core has its own writer attribution" $ case writes of
    _ : second : _ -> writeSource second == CpuWriter Core1 (watchAddress 0x10004)
      && map writtenArray (writeIntersections second) == [ModalStates]
      && map (controllerIndexValue . firstController) (writeIntersections second) == [75]
    _ -> False
  check "host writer remains unattributed to a CPU" $ case writes of
    _ : _ : host : _ -> writeSource host == HostWriter
    _ -> False
  check "boundary crossing names both arrays without inventing a CPU" $ case writes of
    [_, _, _, bus] -> writeSource bus == ModelBusWriter
      && map writtenArray (writeIntersections bus) == [ModalStates, ModalEvents]
      && map (controllerIndexValue . firstController) (writeIntersections bus) == [173, 0]
      && map (controllerIndexValue . lastController) (writeIntersections bus) == [173, 0]
    _ -> False
  empty <- either error pure (observe [modalArrayRange] [])
  check "complete empty observation remains empty" (null (arrayWrites empty))
  outside <- either error pure (observe [range 0x01467584 0x01467840]
    [cpu 0 0x01467584 0x01467584 2 1])
  check "an extra watched writer outside the arrays is not an array writer" (null (arrayWrites outside))
  partial <- either error pure (observe [range 0x01467584 0x01467844]
    [cpu 0 0x01467587 0x01467587 2 1, cpu 0 0x0146783f 0x0146783f 2 2])
  check "partial first and last cells remain bounded" $
    map (map (controllerIndexValue . firstController) . writeIntersections) (arrayWrites partial) == [[0], [173]]
  check "missing first byte of state coverage refuses" (isLeft (observe [range 0x01467589 0x01467840] []))
  check "gap between arrays refuses" (isLeft (observe [range 0x01467588 0x014676e4, range 0x014676e5 0x01467840] []))
  split <- either error pure (observe [range 0x01467588 0x014676e4, range 0x014676e4 0x01467840] [])
  check "separate complete state and event classes are accepted" (null (arrayWrites split))
  check "undeclared source class refuses" (isLeft (observe [modalArrayRange]
    [object ["source" .= ("unknown-write" :: String)]]))
  let classification = get "observer_write_classification" (arrayWriteReportJson report)
  check "legitimate writes retain strict classification without refusing the audit" $
    get "cpu_writes" classification == Number 2 && get "host_writes" classification == Number 1
      && get "model_bus_writes" classification == Number 1
      && get "kind" (get "first_violation" classification) == String "strict-protected-write"
  let (ack, state) = writerEnvelope []
      small = either error id (watchLimit 1)
      invalidObservers =
        [ ("disabled acknowledgement", set "armed" (Bool False) ack, state)
        , ("disabled observation", ack, set "enabled" (Bool False) state)
        , ("overflow violation", ack, set "violation" (String "record-limit") state)
        , ("non-fixed view", ack, set "views" (String "live") state)
        , ("missing classes", ack, set "classes" Null state)
        , ("extra observer key", ack, set "extra" Null state)
        , ("CPU attribution overflow", ack, set "records" (get "records" (snd (writerEnvelope [set "pc" (Number 4294967296) (cpu 0 0x01467588 0x01467588 2 1)]))) state)
        , ("counter regression", ack, snd (writerEnvelope [cpu 0 0x01467588 0x01467588 2 2, cpu 1 0x01467588 0x01467588 2 1]))
        , ("wrong physical coverage", ack, snd (writerEnvelope [cpu 0 0x41467588 0x01467580 2 1]))
        ]
  mapM_ (\(label, acknowledgement, observation) ->
    check ("writer audit refuses " ++ label) (isLeft (auditEnvelope limit acknowledgement observation))) invalidObservers
  check "writer record overflow refuses" (isLeft (auditEnvelope small ack
    (snd (writerEnvelope [cpu 0 0x01467588 0x01467588 2 1, cpu 1 0x01467588 0x01467588 2 2]))))
  permitTests
  scopedRouteTests
  rpcTests
  frameBoundsTests
  scopedSourceTests
  observerConfigurationTests
  lifetimeTests
  runtimeBindingTests
  putStrLn "modal array writer observations passed"

frameBoundsTests :: IO ()
frameBoundsTests = do
  let bound = either error id (modalLimit 65536)
      countsWith key value = set "counts" (set key (Number value) (set "observations" (Number value) permitCounts)) permitEnvelope
  check "per-core frame depth above 32 refuses" (isLeft (parseModalReport bound (countsWith "peak_core_depth" 33)))
  check "aggregate active frames above 64 refuses" (isLeft (parseModalReport bound (countsWith "active_frames" 65)))

syntheticModalText :: B.ByteString
syntheticModalText = B.replicate 128 0

syntheticModalElf :: Word32 -> B.ByteString
syntheticModalElf textFlags = syntheticModalElfWithSections textFlags [1,1,1,1]

syntheticModalElfWithSections :: Word32 -> [Word32] -> B.ByteString
syntheticModalElfWithSections textFlags symbolSections = header <> syntheticModalText <> symbols <> names <> sectionNames <> padding <> sections
 where
  fields = [("rb_modal_entry", 0x10000, 56), ("rb_modal_dispatch", 0x10038, 32),
    ("rb_modal_call_prehook", 0x10058, 16), ("rb_modal_call", 0x10068, 20)] :: [(B.ByteString, Word32, Word32)]
  nameOffsets = scanl (+) 1 [fromIntegral (B.length name + 1) | (name, _, _) <- fields]
  names = B.singleton 0 <> B.concat [name <> B.singleton 0 | (name, _, _) <- fields]
  symbols = B.replicate 16 0 <> B.concat
    [ word 4 nameOffset <> word 4 address <> word 4 size <> B.pack [0x12, 0] <> word 2 symbolSection
    | (nameOffset, (_, address, size), symbolSection) <- zip3 nameOffsets fields symbolSections ]
  sectionNames = "\0.text\0.symtab\0.strtab\0.shstrtab\0"
  symbolOffset = 52 + B.length syntheticModalText
  namesOffset = symbolOffset + B.length symbols
  sectionNamesOffset = namesOffset + B.length names
  sectionOffset = (sectionNamesOffset + B.length sectionNames + 3) `div` 4 * 4
  padding = B.replicate (sectionOffset - sectionNamesOffset - B.length sectionNames) 0
  header = B.pack [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1] <> B.replicate 9 0
    <> word 2 2 <> word 2 40 <> word 4 1 <> word 4 0x10000 <> word 4 0
    <> word 4 (fromIntegral sectionOffset) <> word 4 0
    <> word 2 52 <> word 2 32 <> word 2 0 <> word 2 40 <> word 2 5 <> word 2 4
  section values = B.concat (map (word 4) values)
  sections = B.replicate 40 0
    <> section [1, 1, textFlags, 0x10000, 52, 128, 0, 0, 4, 0]
    <> section [7, 2, 0, 0, fromIntegral symbolOffset, fromIntegral (B.length symbols), 3, 1, 4, 16]
    <> section [15, 3, 0, 0, fromIntegral namesOffset, fromIntegral (B.length names), 0, 0, 1, 0]
    <> section [23, 3, 0, 0, fromIntegral sectionNamesOffset, fromIntegral (B.length sectionNames), 0, 0, 1, 0]
  word :: Int -> Word32 -> B.ByteString
  word bytes value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. bytes - 1]]

syntheticScopeMetadata :: B.ByteString -> B.ByteString -> Value
syntheticScopeMetadata runtime elf = object
  [ "schema" .= (1 :: Int), "global_admission" .= False, "controller" .= (75 :: Int)
  , "stock_entry" .= ("0x00bfb028" :: String), "stock_resume" .= ("0x00bfb02c" :: String)
  , "runtime_sha256" .= sha256Hex (sha256 runtime)
  , "bound_elf" .= object ["sha256" .= sha256Hex (sha256 elf), "functions" .= object
    [ "rb_modal_entry" .= function "0x00010000" 56
    , "rb_modal_dispatch" .= function "0x00010038" 32
    , "rb_modal_call_prehook" .= function "0x00010058" 16
    , "rb_modal_call" .= function "0x00010068" 20 ]]
  ]
 where
  function :: String -> Int -> Value
  function address size = object ["address" .= address, "size" .= size]

scopedSourceTests :: IO ()
scopedSourceTests = do
  let elf = syntheticModalElf 6
      runtime = syntheticModalText
      metadata = syntheticScopeMetadata runtime elf
      parse = parseScopedModalSource (watchAddress 0x10000)
  source <- either (error . ("synthetic bound modal source: " ++)) pure (parse runtime elf metadata)
  stock <- either error pure (modalStock (B.replicate 0xfc 0))
  request <- either error pure (scopedModalRequest stock permitLimit source)
  let requestJson = modalBeginRequestJson request
      pair low high = get "pair" (object ["pair" .= ([low, high] :: [Word32])])
  check "bound ELF extents configure the scoped request" $
    get "seam" requestJson == pair 0x10000 0x10038 && get "owned" requestJson == pair 0x10038 0x10058
      && get "owned_prehook" requestJson == Number 0x10058
  check "bound helper extents include instructions beyond their entries" $
    get "helpers" requestJson == array [pair 0x10058 0x10068, pair 0x10068 0x1007c]
      && get "helpers" (scopedModalSourceJson source) == get "helpers" requestJson
  check "linked instruction bytes come from bound ELF text" $
    get "start" (get "linked" requestJson) == Number 0x10000
      && get "bytes" (get "linked" requestJson) == get "bytes" (object ["bytes" .= B.unpack syntheticModalText])
  let changedRuntime = B.cons 1 (B.drop 1 runtime)
      changedElf = B.cons 1 (B.drop 1 elf)
      writableElf = syntheticModalElf 7
      undefinedPrehookElf = syntheticModalElfWithSections 6 [1,1,0,1]
      misboundCallElf = syntheticModalElfWithSections 6 [1,1,1,2]
      bound = get "bound_elf" metadata
      functions = get "functions" bound
      mismatchedFunction = set "bound_elf" (set "functions"
        (set "rb_modal_dispatch" (set "size" (Number 28) (get "rb_modal_dispatch" functions)) functions) bound) metadata
      malformed =
        [ ("stale runtime binding", changedRuntime, elf, metadata)
        , ("stale ELF binding", runtime, changedElf, metadata)
        , ("ELF text differs from rebound runtime", changedRuntime, elf, syntheticScopeMetadata changedRuntime elf)
        , ("writable linked text", runtime, writableElf, syntheticScopeMetadata runtime writableElf)
        , ("manifest function differs from ELF", runtime, elf, mismatchedFunction)
        , ("prehook helper truncated against ELF", runtime, elf, set "bound_elf" (set "functions"
            (set "rb_modal_call_prehook" (set "size" (Number 4) (get "rb_modal_call_prehook" functions)) functions) bound) metadata)
        , ("call helper truncated against ELF", runtime, elf, set "bound_elf" (set "functions"
            (set "rb_modal_call" (set "size" (Number 4) (get "rb_modal_call" functions)) functions) bound) metadata)
        , ("undefined prehook ELF symbol", runtime, undefinedPrehookElf, syntheticScopeMetadata runtime undefinedPrehookElf)
        , ("call helper symbol bound to nontext section", runtime, misboundCallElf, syntheticScopeMetadata runtime misboundCallElf)
        , ("global admission claim", runtime, elf, set "global_admission" (Bool True) metadata)
        , ("unknown metadata key", runtime, elf, set "extra" Null metadata)
        , ("malformed ELF with matching digest", runtime, changedElf, syntheticScopeMetadata runtime changedElf)
        ]
  mapM_ (\(label, runtimeBytes, elfBytes, description) ->
    check ("scoped source refuses " ++ label) (isLeft (parse runtimeBytes elfBytes description))) malformed
  check "linked text outside uploaded runtime refuses" $
    isLeft (parseScopedModalSource (watchAddress 0x20000) runtime elf metadata)

observerConfigurationTests :: IO ()
observerConfigurationTests = do
  let document = object ["schema" .= (1 :: Int), "before_unload" .= ([] :: [Value]), "after_unload" .= ([] :: [Value])]
      ranges = [["01467588", "01467840" :: String]]
      preserved = object ["kind" .= ("ram-byte-preservation" :: String), "ranges" .= ranges]
      configurations =
        [ ("legacy memory watch", set "protected_writes" (get "ranges" (object ["ranges" .= ranges])) document)
        , ("aggregate preservation", set "protected_memory" preserved document)
        , ("Fixed execution watch", set "forbidden_execution" (get "ranges" (object ["ranges" .= ranges])) document)
        , ("refusal control", set "expect" (String "refused") (set "protected_memory" preserved document))
        ]
      decode value = either (error . ("observer configuration fixture: " ++)) pure (decodeTrajectories (BL.toStrict (encode value)))
  clear <- decode document
  check "unprotected FILMSIM route permits independent modal observation" (checkModalObserverConfiguration clear == Right ())
  mapM_ (\(label, value) -> do
    configured <- decode value
    check ("modal observation refuses coexistence with " ++ label) (isLeft (checkModalObserverConfiguration configured))) configurations

array :: [Value] -> Value
array values = get "values" (object ["values" .= values])

completedReceipt :: Int -> Value
completedReceipt count = object
  [ "phase" .= ("ended" :: String), "publication_count" .= count, "forward_writes" .= count, "reverse_writes" .= count
  , "pending_stores" .= (0 :: Int), "install_core" .= (0 :: Int), "rollback_core" .= (1 :: Int)
  , "checked_resource_writes" .= (7 :: Int), "checked_identity_writes" .= (0 :: Int)
  , "checked_scheduler_writes" .= (10 :: Int), "checked_context_writes" .= (40 :: Int), "checked_vfp_owner_writes" .= (2 :: Int)
  , "audited_writes" .= (12 :: Int), "admitted_settings_writes" .= (0 :: Int), "admitted_reload_writes" .= (0 :: Int)
  , "admitted_call_writes" .= (0 :: Int)
  , "failure" .= Null ]

healthyGuard :: Value
healthyGuard = object
  [ "hook_entries" .= (10 :: Int), "protected_entries" .= (8 :: Int), "accepted_entries" .= (8 :: Int)
  , "refused_entries" .= (0 :: Int), "failure" .= Null ]

taskEntry :: Value
taskEntry = object
  [ "core" .= (0 :: Int), "pc" .= (0x00bfb028 :: Word32), "instructions" .= (9000 :: Int)
  , "controller" .= (75 :: Int), "sp" .= (0x40030000 :: Word32), "lr" .= (0x00bfc000 :: Word32), "cpsr" .= (0x13 :: Int)
  , "identity" .= object ["kernel" .= ("primary" :: String), "id" .= (1 :: Int)
      , "node" .= (0x4005ff0c :: Word32), "creation" .= (0x4005beac :: Word32), "stack" .= ([0x30000,0x31000] :: [Word32])]
  , "fetch" .= object ["guest" .= (0x00bfb028 :: Word32), "physical" .= (0x00bfb028 :: Word32)
      , "bytes" .= ([0,0,0,0] :: [Int]), "core" .= (0 :: Int), "entry_progress" .= (123 :: Int), "byte_order" .= ("little" :: String)] ]

completedEnvelope :: Int -> Value
completedEnvelope count = set "watch" cacheOnlyWatchFixture (set "lifetime" (completedReceipt count) (set "physical_guard" healthyGuard
  (set "physical_fetch_coverage" (Bool True) (set "context_source_spans" (Number 16)
    (set "task_identity_verified" (Bool True) (set "task_entries" (array [taskEntry]) permitEnvelope))))))

cacheOnlyWatchFixture :: Value
cacheOnlyWatchFixture = withoutRecords (digested (set "fetch_attribution"
  (object ["sources" .= ([] :: [Value]), "ranges" .= ([[0x00bfb028,0x00bfb124]] :: [[Word32]])]) (snd (writerEnvelope []))))

-- | Adds the SDK row count; the digest is a fixed fixture value.
digested :: Value -> Value
digested value = set "records_sha256" (String fixtureRowDigest) (set "record_count" (Number (fromIntegral rows)) value)
 where
  rows = case get "records" value of
    Array entries -> length entries
    _ -> 0

fixtureRowDigest :: T.Text
fixtureRowDigest = "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a"

withoutRecords :: Value -> Value
withoutRecords (Object fields) = Object (KM.delete "records" fields)
withoutRecords value = value

lifetimeTests :: IO ()
lifetimeTests = do
  check "fresh install with requested unload is admissible" (checkModalLifetimeRoute FreshModalInstall ModalUnloadRequested == Right ())
  check "installed checkpoint restore refuses" (isLeft (checkModalLifetimeRoute RestoredModalInstall ModalUnloadRequested))
  check "unrequested unload refuses" (isLeft (checkModalLifetimeRoute FreshModalInstall ModalUnloadOmitted))
  let counts = object ["observations" .= (4 :: Int), "entries" .= (2 :: Int), "owned_handoffs" .= (1 :: Int)
        , "fallback_handoffs" .= (1 :: Int), "returns" .= (2 :: Int), "active_frames" .= (0 :: Int), "peak_core_depth" .= (1 :: Int)]
      complete = set "counts" counts (set "captured_domains" (array [set "controller" (Number 75) domain])
        (set "policy" (String "scoped-owned75") (completedEnvelope 224)))
      changeReceipt key value = set "lifetime" (set key value (completedReceipt 224)) complete
      unfinished =
        [ ("prepared", changeReceipt "phase" (String "prepared"))
        , ("publishing", changeReceipt "phase" (String "publishing"))
        , ("installed", changeReceipt "phase" (String "installed"))
        , ("rolling rollback", changeReceipt "phase" (String "rolling_back"))
        , ("rollback stores without readback", changeReceipt "phase" (String "rollback_stored"))
        , ("verified but not ended", changeReceipt "phase" (String "rollback_verified"))
        , ("partial install", changeReceipt "forward_writes" (Number 223))
        , ("extra install", changeReceipt "forward_writes" (Number 225))
        , ("partial rollback", changeReceipt "reverse_writes" (Number 223))
        , ("wrong source count", changeReceipt "publication_count" (Number 223))
        , ("pending CPU store", changeReceipt "pending_stores" (Number 1))
        , ("unbound installer core", changeReceipt "install_core" Null)
        , ("unbound rollback core", changeReceipt "rollback_core" Null)
        , ("no task samples", set "task_entries" (array []) complete)
        , ("no physical observer", set "physical_guard" Null complete)
        , ("no protected instruction entries", set "physical_guard"
            (object ["hook_entries" .= (0 :: Int), "protected_entries" .= (0 :: Int), "accepted_entries" .= (0 :: Int)
              , "refused_entries" .= (0 :: Int), "failure" .= Null]) complete)
        , ("missing raw lifetime watch", set "watch" Null complete)
        , ("unverified task", set "task_identity_verified" (Bool False) complete)
        , ("unverified context sources", set "context_source_spans" (Number 0) complete)
        , ("missing context span", set "context_source_spans" (Number 15) complete)
        , ("extra task sample", set "task_entries" (array [taskEntry,taskEntry]) complete)
        , ("wrong task namespace", set "task_entries" (array [set "identity" (set "kernel" (String "secondary") (get "identity" taskEntry)) taskEntry]) complete)
        , ("wrong task ID", set "task_entries" (array [set "identity" (set "id" (Number 2) (get "identity" taskEntry)) taskEntry]) complete)
        , ("IRQ masked task", set "task_entries" (array [set "cpsr" (Number 0x93) taskEntry]) complete)
        , ("Jazelle task", set "task_entries" (array [set "cpsr" (Number 0x1000013) taskEntry]) complete)
        , ("stack outside extent", set "task_entries" (array [set "sp" (Number 0x40031004) taskEntry]) complete)
        , ("fetch on wrong core", set "task_entries" (array [set "fetch" (set "core" (Number 1) (get "fetch" taskEntry)) taskEntry]) complete)
        , ("fetch alias", set "task_entries" (array [set "fetch" (set "guest" (Number 0x40bfb028) (get "fetch" taskEntry)) taskEntry]) complete)
        , ("short fetch", set "task_entries" (array [set "fetch" (set "bytes" (array [Number 0]) (get "fetch" taskEntry)) taskEntry]) complete)
        ]
  parsed <- either error pure (parseModalReport permitLimit complete)
  check "completed source-bound scoped lifetime is admitted" (checkScopedModalRoute parsed == Right () && modalLifetimeCoverage parsed)
  check "lifetime report preserves every typed field" (modalReportJson parsed == complete)
  mapM_ (\(label, value) -> check ("lifetime refuses " ++ label)
    (either (const True) (isLeft . checkScopedModalRoute) (parseModalReport permitLimit value))) unfinished
  check "unknown lifetime phase refuses" (isLeft (parseModalReport permitLimit (changeReceipt "phase" (String "complete"))))
  check "unknown lifetime failure refuses" (isLeft (parseModalReport permitLimit (changeReceipt "failure" (object ["kind" .= ("other" :: String)]))))
  let failed = set "physical_fetch_coverage" (Bool False) (set "task_identity_verified" (Bool False)
        (changeReceipt "failure" (object ["kind" .= ("wrong_order" :: String)])))
  failedReport <- either error pure (parseModalReport permitLimit failed)
  check "sticky typed failure remains evidence" (modalReportJson failedReport == failed && not (modalLifetimeCoverage failedReport))
  let awaiting = set "enabled" (Bool True) (set "counts" (set "owned_handoffs" (Number 0) counts) complete)
  active <- either error pure (parseModalReport permitLimit awaiting)
  check "active owned entry may await handoff but does not qualify" (not (modalLifetimeCoverage active))
  let (ack, state) = writerEnvelope []
      extension = object ["sources" .= ([] :: [Value]), "ranges" .= ([[0x00bfb028,0x00bfb124]] :: [[Word32]])]
      cacheOnly = digested . set "fetch_attribution" extension
  check "cache-only raw watch projects to the FixedWatch parser" $
    either (const False) (const True) (parseModalLifetimeWatchAck (cacheOnly ack))
      && either (const False) (const True) (parseModalLifetimeWatch limit (cacheOnly state))
  check "cache-only watch rejects attributed data sources" (isLeft (parseModalLifetimeWatch limit
    (set "fetch_attribution" (set "sources" (array [String "cpu-write"]) extension) state)))
  check "cache-only watch rejects unknown extension keys" (isLeft (parseModalLifetimeWatch limit
    (set "fetch_attribution" (set "extra" Null extension) state)))
  check "projection retains strict unknown-field refusal" (isLeft (parseModalLifetimeWatch limit (set "extra" Null (cacheOnly state))))
  let manyRows = [cpu 0 0x01467588 0x01467588 2 clock | clock <- [1 .. 65537]]
      longState = cacheOnly (snd (writerEnvelope manyRows))
      intervalBound = either error id (watchLimit 65536)
  check "continuous writer history is independent of the modal observation bound" $
    isLeft (parseModalLifetimeWatch intervalBound longState)
      && either (const False) (const True) (parseModalLifetimeWatch modalLifetimeWriterLimit longState)
  check "watch row count must match its rows" $
    isLeft (parseModalLifetimeWatch limit (set "record_count" (Number 1) (cacheOnly state)))
  check "report watch summary requires the SDK row digest" $
    isLeft (parseModalReport permitLimit (set "watch" (case cacheOnlyWatchFixture of
      Object fields -> Object (KM.delete "records_sha256" fields); other -> other) complete))
  check "report watch summary refuses embedded rows" $
    isLeft (parseModalReport permitLimit (set "watch" (digested (cacheOnly state)) complete))

runtimeTextFixture :: B.ByteString
runtimeTextFixture = B.replicate 0x24 0 <> wordBytes 0xe5813000 <> B.replicate 0x1c 0
  <> wordBytes 0xe5813000 <> B.replicate 0x38 0

wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. 3 :: Int]]

runtimeSymbolsFixture :: [(B.ByteString, Word32, Word32, Word32, Word32)]
runtimeSymbolsFixture =
  [ ("ram_push_install",0x10000,16,1,0x12), ("ram_push_rollback",0x10010,16,1,0x12)
  , ("store_new_words",0x10020,16,1,0x12), ("restore_old_words",0x10040,16,1,0x12)
  , ("rb_resident",0x10100,1024,3,0x11) ]

runtimeElfFixture :: B.ByteString -> [(B.ByteString, Word32, Word32, Word32, Word32)] -> B.ByteString
runtimeElfFixture text fields = header <> text <> readonly <> symbols <> names <> sectionNames <> padding <> sections
 where
  readonly = B.pack [1 .. 16]
  nameOffsets = scanl (+) 1 [fromIntegral (B.length name + 1) | (name, _, _, _, _) <- fields]
  names = B.singleton 0 <> B.concat [name <> B.singleton 0 | (name, _, _, _, _) <- fields]
  symbols = B.replicate 16 0 <> B.concat
    [ word 4 nameOffset <> word 4 address <> word 4 size <> B.pack [fromIntegral kind,0] <> word 2 sectionIndex
    | (nameOffset, (_,address,size,sectionIndex,kind)) <- zip nameOffsets fields ]
  sectionNames = "\0.text\0.rodata\0.bss\0.symtab\0.strtab\0.shstrtab\0"
  symbolOffset = 52 + B.length text + B.length readonly
  namesOffset = symbolOffset + B.length symbols
  sectionNamesOffset = namesOffset + B.length names
  sectionOffset = (sectionNamesOffset + B.length sectionNames + 3) `div` 4 * 4
  padding = B.replicate (sectionOffset - sectionNamesOffset - B.length sectionNames) 0
  header = B.pack [0x7f,0x45,0x4c,0x46,1,1,1] <> B.replicate 9 0
    <> word 2 2 <> word 2 40 <> word 4 1 <> word 4 0x10000 <> word 4 0
    <> word 4 (fromIntegral sectionOffset) <> word 4 0
    <> word 2 52 <> word 2 32 <> word 2 0 <> word 2 40 <> word 2 7 <> word 2 6
  section = B.concat . map (word 4)
  sections = B.replicate 40 0
    <> section [1,1,6,0x10000,52,128,0,0,4,0]
    <> section [7,1,2,0x10080,180,16,0,0,4,0]
    <> section [15,8,3,0x10100,0,1024,0,0,4,0]
    <> section [20,2,0,0,fromIntegral symbolOffset,fromIntegral (B.length symbols),5,1,4,16]
    <> section [28,3,0,0,fromIntegral namesOffset,fromIntegral (B.length names),0,0,1,0]
    <> section [36,3,0,0,fromIntegral sectionNamesOffset,fromIntegral (B.length sectionNames),0,0,1,0]
  word :: Int -> Word32 -> B.ByteString
  word bytes value = B.take bytes (wordBytes value)

runtimeManifestFixture :: B.ByteString -> Ram.RamPushManifest
runtimeManifestFixture runtime = Ram.RamPushManifest
  { Ram.manifestPath = "manifest.json", Ram.runtimePath = "runtime.bin", Ram.manifestDigest = "fixture"
  , Ram.sourceIdentity = [], Ram.allocationBase = Ram.Address 0x10000, Ram.allocationSize = B.length runtime
  , Ram.runtimeDigest = sha256Hex (sha256 runtime), Ram.runtimeBytes = runtime
  , Ram.manifestEntries = Ram.RamPushEntries (Ram.Address 0x10000) (Ram.Address 0x10010) (Ram.Address 0x10060)
  , Ram.manifestPublications =
      [ Ram.Publication index Ram.Setup (Ram.Address address) (Ram.WordValue 0x11111111) (Ram.WordValue 0x22222222) Ram.Both
      | (index,address) <- zip [222,221 .. 0] [0x20000,0x20004 .. 0x20378] ] }

runtimeBindingTests :: IO ()
runtimeBindingTests = do
  let text = runtimeTextFixture
      runtime = text <> B.pack [1 .. 16] <> B.replicate (0x500 - 144) 0
      elf = runtimeElfFixture text runtimeSymbolsFixture
      manifest = runtimeManifestFixture runtime
      bind = bindModalRuntimeSource
      symbolsWith name change = [if symbolName == name then change row else row | row@(symbolName,_,_,_,_) <- runtimeSymbolsFixture]
      shortWriter = runtimeElfFixture text (symbolsWith "store_new_words" (\(name,address,_,section,kind) -> (name,address,4,section,kind)))
      shortResident = runtimeElfFixture text (symbolsWith "rb_resident" (\(name,address,_,section,kind) -> (name,address,1020,section,kind)))
      falseWriter = runtimeElfFixture text (symbolsWith "store_new_words" (\(name,address,size,section,_) -> (name,address,size,section,0x11)))
      duplicateText = B.take 0x28 text <> wordBytes 0xe5813000 <> B.drop 0x2c text
      duplicateRuntime = duplicateText <> B.drop 128 runtime
      duplicateElf = runtimeElfFixture duplicateText runtimeSymbolsFixture
      shiftedEntry = manifest { Ram.manifestEntries = Ram.RamPushEntries (Ram.Address 0x10004) (Ram.Address 0x10010) (Ram.Address 0x10060) }
      mismatchedReadonly = B.take 128 runtime <> B.singleton 99 <> B.drop 129 runtime
      nonzeroResident = B.take 0x100 runtime <> B.singleton 1 <> B.drop 0x101 runtime
      malformed =
        [ ("partial writer symbol extent",manifest,shortWriter)
        , ("resident1020",manifest,shortResident)
        , ("writer is not function symbol",manifest,falseWriter)
        , ("duplicate source-bound STR",runtimeManifestFixture duplicateRuntime,duplicateElf)
        , ("manifest entry inside rather than at ELF function",shiftedEntry,elf)
        , ("stale runtime digest",manifest { Ram.runtimeDigest = replicate 64 '0' },elf)
        , ("allocation extent differs from bytes",manifest { Ram.allocationSize = 0x504 },elf)
        , ("readonly nontext bytes differ from runtime",runtimeManifestFixture mismatchedReadonly,elf)
        , ("resident initial bytes nonzero",runtimeManifestFixture nonzeroResident,elf)
        , ("missing writer symbol",manifest,runtimeElfFixture text (filter (\(name,_,_,_,_) -> name /= "store_new_words") runtimeSymbolsFixture))
        , ("duplicate writer symbol",manifest,runtimeElfFixture text (runtimeSymbolsFixture ++ [("store_new_words",0x10020,16,1,0x12)]))
        ]
  bound <- either (error . ("runtime source fixture: " ++)) pure (bind manifest elf)
  let evidence = modalRuntimeSourceJson bound
      lifetime = get "lifetime" evidence
      rows = values (get "publications" lifetime)
      images = values (get "readonly" lifetime)
  check "runtime binding retains complete nontext readonly source" $
    length images == 2 && any (\entry -> get "start" entry == Number 0x10080
      && get "bytes" entry == array (map (Number . fromIntegral) ([1 .. 16] :: [Word32]))) images
  check "manifest physical publication vector order survives nonmonotone indices" $
    map (get "address") rows == map (Number . fromIntegral) ([0x20000,0x20004 .. 0x20378] :: [Word32])
  check "source binding retains runtime and complete ELF digests" $
    get "bound_elf_sha256" evidence == get "digest" (object ["digest" .= sha256Hex (sha256 elf)])
      && get "runtime_sha256" evidence == get "digest" (object ["digest" .= sha256Hex (sha256 runtime)])
  mapM_ (\(label,description,source) -> check ("runtime source refuses " ++ label) (isLeft (bind description source))) malformed
  check "whole loader pins reject same-size changed source" $
    isLeft (bindModalLifetimeSource manifest elf (B.replicate 0xdae0 0) (B.replicate 0xdaa0 0))
  stock <- either error pure (modalStock (B.replicate 252 0))
  base <- either error pure (modalRequest stock Nothing permitLimit)
  let contextRanges =
        [(0x40000,0x403a0),(0x403a4,0x403b0),(0x403b4,0x403c0),(0x403c4,0x403d0)
        ,(0x403d4,0x403e0),(0x403e4,0x403f0),(0x403f4,0x40400),(0x40404,0x4dae0)
        ,(0xd0000,0xd03a0),(0xd03a4,0xd03b0),(0xd03b4,0xd03c0),(0xd03c4,0xd03d0)
        ,(0xd03d4,0xd03e0),(0xd03e4,0xd03f0),(0xd03f4,0xd0400),(0xd0404,0xddaa0)] :: [(Word32,Word32)]
      context = [object ["start" .= low,"bytes" .= B.unpack (B.replicate (fromIntegral (high-low)) 0)] | (low,high) <- contextRanges]
      lifetimeLimit = Number (fromIntegral (modalLimitValue modalLifetimeObservationLimit))
      declaration = set "limit" lifetimeLimit (set "context_sources" (array context) (set "lifetime" lifetime (modalBeginRequestJson base)))
      changeLifetime key value = set "lifetime" (set key value lifetime) declaration
      malformedDeclarations =
        [ ("ordinary observation bound",set "limit" (Number 65536) declaration)
        , ("partial context",set "context_sources" (array (drop 1 context)) declaration)
        , ("extra context",set "context_sources" (array (context ++ take 1 context)) declaration)
        , ("reordered context",set "context_sources" (array (reverse context)) declaration)
        , ("mutable vector included",set "context_sources" (array (map (set "bytes" (array (replicate 0x3a4 (Number 0)))) (take 1 context) ++ drop 1 context)) declaration)
        , ("resident extent1020",changeLifetime "resident" (array [Number 0x10100,Number 0x104fc]))
        , ("resident outside runtime",changeLifetime "resident" (array [Number 0x20000,Number 0x20400]))
        , ("runtime alias",changeLifetime "runtime" (array [Number 0x40010000,Number 0x40010500]))
        , ("missing readonly source",changeLifetime "readonly" (array (drop 1 images)))
        , ("duplicate readonly source",changeLifetime "readonly" (array (images ++ take 1 images)))
        , ("wrong writer opcode",changeLifetime "install" (set "writer" (object ["pc" .= (0x10024 :: Word32),"word" .= (0 :: Int)]) (get "install" lifetime)))
        , ("entry source mismatch",changeLifetime "install" (set "entry" (object ["pc" .= (0x10000 :: Word32),"word" .= (1 :: Int)]) (get "install" lifetime)))
        , ("publication duplicate",changeLifetime "publications" (array (take 1 rows ++ take 1 rows ++ drop 2 rows)))
        , ("publication partial vector",changeLifetime "publications" (array (drop 1 rows)))
        , ("publication alias",changeLifetime "publications" (array (map (set "address" (Number 0x40020000)) (take 1 rows) ++ drop 1 rows)))
        , ("missing settings reloads",set "lifetime" (case lifetime of Object fields -> Object (KM.delete "settings_reloads" fields); other -> other) declaration)
        , ("unaligned reload call",changeReload "call" (Number 0x006309ee))
        , ("reload return equal to its call",changeReload "return" (Number 0x006309ec))
        , ("unaligned reload copy store",changeReload "copy_store" (Number 0x00722136))
        , ("reload target before the settings block",changeReload "target" (array [Number 0x0096cc60, Number 0x0096cd8c]))
        , ("reload target past the settings block",changeReload "target" (array [Number 0x0096cc74, Number 0x0096ce28]))
        , ("empty reload target",changeReload "target" (array [Number 0x0096cc74, Number 0x0096cc74]))
        , ("reload copy store inside the setter",changeReload "copy_store" (Number 0x0062ff54))
        , ("reload call inside the runtime",changeReload "call" (Number 0x10000))
        , ("repeated reload sites",changeLifetime "settings_reloads" (array (replicate 2 stockReload)))
        , ("unknown reload key",changeLifetime "settings_reloads" (array [set "writer" (String "reload") stockReload]))
        , ("five reloads",changeLifetime "settings_reloads" (array [set "call" (Number (fromIntegral (0x00500000 + index * 8 :: Int))) stockReload | index <- [0 .. 4]]))
        , ("missing settings calls",set "lifetime" (case lifetime of Object fields -> Object (KM.delete "settings_calls" fields); other -> other) declaration)
        , ("unaligned settings call",changeCall "call" (Number 0x0061c976))
        , ("settings call return equal to its call",changeCall "return" (Number 0x0061c974))
        , ("settings call without writers",changeCall "writers" (array []))
        , ("unaligned settings call writer",callWriters [[0x00722054, 0x00722176]])
        , ("overlapping settings call writers",callWriters [[0x00722054, 0x00722178], [0x00722174, 0x00722218]])
        , ("nine settings call writers",callWriters [[0x00700000 + index * 8, 0x00700004 + index * 8] | index <- [0 .. 8]])
        , ("settings call writer inside the setter",callWriters [[0x0062ff00, 0x00630000]])
        , ("settings call writer inside the runtime",callWriters [[0x10000, 0x10010]])
        , ("settings call target past the settings block",changeCall "target" (array [Number 0x0096cc74, Number 0x0096ce28]))
        , ("settings call site repeating a reload site",changeCall "call" (Number 0x006309ec))
        , ("settings call inside the runtime",changeCall "return" (Number 0x10000))
        , ("unknown settings call key",changeLifetime "settings_calls" (array [set "copy_store" (Number 0x00722134) stockCall]))
        , ("five settings calls",changeLifetime "settings_calls" (array [set "call" (Number (fromIntegral (0x00500000 + index * 8 :: Int))) stockCall | index <- [0 .. 4]]))
        ]
      stockReload = case get "settings_reloads" lifetime of Array entries | entry : _ <- foldr (:) [] entries -> entry; _ -> Null
      changeReload key value = changeLifetime "settings_reloads" (array [set key value stockReload])
      stockCall = case get "settings_calls" lifetime of Array entries | entry : _ <- foldr (:) [] entries -> entry; _ -> Null
      changeCall key value = changeLifetime "settings_calls" (array [set key value stockCall])
      callWriters pairs = changeCall "writers" (get "w" (object ["w" .= (pairs :: [[Word32]])]))
  request <- either (error . ("lifetime declaration fixture: " ++)) pure (parseModalBeginRequest declaration)
  check "typed lifetime request preserves source images and physical vector order" (modalBeginRequestJson request == declaration)
  mapM_ (\(label,value) -> check ("lifetime request refuses " ++ label) (isLeft (parseModalBeginRequest value))) malformedDeclarations
  expectedWrites <- either error pure (modalLifetimeWatchRanges request)
  let fetches = unionWatchRanges (range 0x00bfb028 0x00bfb124 :
        [range pc (pc+4) | pc <- [0x10000,0x10010,0x10024,0x10044,0x40240,0x4031c,0x436b8,0x43690,0x436f0
          ,0x43720,0x43848,0x42440,0xd0240,0xd031c,0xd36b8,0xd3690,0xd36f0,0xd3720,0xd3848,0xd2440]])
      fetchJson spans = object ["sources" .= ([] :: [Value]), "ranges" .= map pair spans]
      reloadSpans = [range 0x006309ec 0x006309f0, range 0x006309f0 0x006309f4, range 0x00637ee4 0x00637ee8, range 0x00637ee8 0x00637eec
        , range 0x0061c974 0x0061c978, range 0x0061c978 0x0061c97c]
      watch spans records = digested (object
        [ "enabled" .= True, "views" .= ("fixed" :: String), "ranges" .= map pair (unionWatchRanges (spans ++ reloadSpans)), "records" .= records, "violation" .= Null
        , "classes" .= ([object ["range" .= pair span_, "kinds" .= (["cpu-write","host-write","model-bus-write"] :: [String])] | span_ <- spans]
            ++ [object ["range" .= pair span_, "kinds" .= (["cpu-fetch"] :: [String])] | span_ <- reloadSpans])
        , "fetch_attribution" .= fetchJson fetches ])
      addresses = [0x20000,0x20004 .. 0x20378] :: [Word32]
      writes =
        [set "pc" (Number 0x10024) (set "value" (Number 0x22222222) (set "before" (array (replicate 4 (Number 0x11)))
          (cpu 0 address address 4 clock))) | (address,clock) <- zip addresses [1 ..]]
        ++ [set "pc" (Number 0x10044) (set "value" (Number 0x11111111) (set "before" (array (replicate 4 (Number 0x22)))
          (cpu 1 address address 4 clock))) | (address,clock) <- zip (reverse addresses) [224 ..]]
      fullWatch = watch expectedWrites writes
      exactReceipt = foldr (\key value -> set key (Number 0) value) (completedReceipt 223)
        ["checked_resource_writes","checked_identity_writes","checked_scheduler_writes","checked_context_writes","checked_vfp_owner_writes","audited_writes","admitted_settings_writes","admitted_reload_writes","admitted_call_writes"]
      clean = set "watch" (withoutRecords fullWatch) (set "lifetime" exactReceipt (set "task_identity_verified" (Bool False)
        (set "task_entries" (array []) (completedEnvelope 223))))
  let watchedCount = length (unionWatchRanges (expectedWrites ++ reloadSpans))
  rangeBound <- either error pure (watchRangeLimit watchedCount)
  shortRangeBound <- either error pure (watchRangeLimit (watchedCount - 1))
  fullAcknowledgement <- either error pure (lifetimeWatchAcknowledgement fullWatch >>= parseModalLifetimeWatchAck)
  fullObservation <- either error pure (parseModalLifetimeWatch modalLifetimeWriterLimit fullWatch)
  check "continuous source watch exceeds only the unchanged UI range cap" $
    length expectedWrites > 8 && isLeft (checkExecutionWrites ForbidProtectedWrites modalLifetimeWriterLimit expectedWrites reloadSpans fullAcknowledgement fullObservation)
  check "source-bound checker refuses more ranges than its explicit bound" $
    isLeft (checkExecutionWritesWithRangeLimit shortRangeBound ForbidProtectedWrites modalLifetimeWriterLimit expectedWrites reloadSpans fullAcknowledgement fullObservation)
  check "source-bound checker accepts its complete finite range bound" $
    not (isLeft (checkExecutionWritesWithRangeLimit rangeBound ForbidProtectedWrites modalLifetimeWriterLimit expectedWrites reloadSpans fullAcknowledgement fullObservation))
  check "array audit refuses a watch without the reload fetch classes" $
    isLeft (checkLifetimeArrayAudit request (Right (set "classes" (array (take (length expectedWrites) (values (get "classes" fullWatch)))) fullWatch))
      (parseFinalModalWatch fullWatch))
  audit <- either error pure (checkLifetimeArrayAudit request (Right fullWatch) (parseFinalModalWatch fullWatch))
  report <- either error pure (parseModalReport permitLimit clean)
  finalFull <- either error pure (parseFinalModalWatch fullWatch)
  check "stock223 full lifetime binds to its request and every raw write" (checkCompletedModalLifetime request audit finalFull report == Right ())
  otherDigest <- either error pure (parseFinalModalWatch (set "records_sha256" (String (T.replicate 64 "0")) fullWatch))
  check "an equal row count with another row digest refuses" (isLeft (checkCompletedModalLifetime request audit otherDigest report))
  check "source-bound cache-only attribution binds every protected point" (checkModalLifetimeWatchSources request fullWatch == Right ())
  check "missing protected instruction fetch refuses" (isLeft (checkModalLifetimeWatchSources request
    (set "fetch_attribution" (fetchJson (drop 1 fetches)) fullWatch)))
  linked <- either error pure (modalLinked (watchAddress 0x10000) text)
  scope <- either error pure (modalScope linked (range 0x10000 0x10038) (range 0x10038 0x10060)
    (ModalHelpers (range 0x10060 0x10070) (range 0x10070 0x10080)) (Just (watchAddress 0x10060)))
  scopedBase <- either error pure (modalRequest stock (Just (linked, scope)) permitLimit)
  let installedBranch = 0xea000abc :: Word32
      scopedPublications = rows ++ [object ["address" .= (0x00bfb028 :: Word32), "old" .= (0 :: Word32), "new" .= installedBranch]]
      scopedLifetime = set "publications" (array scopedPublications) (set "readonly" (array (drop 1 images)) lifetime)
      scopedDeclaration = set "limit" lifetimeLimit (set "context_sources" (array context) (set "lifetime" scopedLifetime (modalBeginRequestJson scopedBase)))
      helperFetches = unionWatchRanges (range 0x10000 0x10080 : fetches)
      truncatedHelperFetches = unionWatchRanges (range 0x10000 0x1007c : fetches)
  scopedRequest <- either error pure (parseModalBeginRequest scopedDeclaration)
  check "continuous static fetch coverage includes every helper instruction" $
    checkModalLifetimeWatchSources scopedRequest (set "fetch_attribution" (fetchJson helperFetches) fullWatch) == Right ()
  check "missing final call-helper instruction refuses source coverage" $
    isLeft (checkModalLifetimeWatchSources scopedRequest (set "fetch_attribution" (fetchJson truncatedHelperFetches) fullWatch))
  scopedWrites <- either error pure (modalLifetimeWatchRanges scopedRequest)
  let forwardEntry = set "pc" (Number 0x10024) (set "value" (Number (fromIntegral installedBranch))
        (cpu 0 0x00bfb028 0x00bfb028 4 224))
      reverseEntry = set "pc" (Number 0x10044) (set "before" (array (map (Number . fromIntegral) (B.unpack (wordBytes installedBranch))))
        (cpu 1 0x00bfb028 0x00bfb028 4 225))
      reverseRows = zipWith (\clock row -> set "tick" (Number (fromIntegral clock)) (set "instructions" (Number (fromIntegral clock)) row))
        [226 :: Integer ..] (drop 223 writes)
      scopedWatch = set "fetch_attribution" (fetchJson helperFetches)
        (watch scopedWrites (take 223 writes ++ [forwardEntry,reverseEntry] ++ reverseRows))
      scopedSummary = withoutRecords scopedWatch
      scopedCounts = object ["observations" .= (4 :: Int), "entries" .= (2 :: Int), "owned_handoffs" .= (1 :: Int)
        , "fallback_handoffs" .= (1 :: Int), "returns" .= (2 :: Int), "active_frames" .= (0 :: Int), "peak_core_depth" .= (1 :: Int)]
      scopedReceipt = set "publication_count" (Number 224) (set "forward_writes" (Number 224) (set "reverse_writes" (Number 224) exactReceipt))
      installedTask = set "fetch" (set "bytes" (array (map (Number . fromIntegral) (B.unpack (wordBytes installedBranch))))
        (get "fetch" taskEntry)) taskEntry
      completedScoped = set "policy" (String "scoped-owned75") (set "counts" scopedCounts
        (set "captured_domains" (array [set "controller" (Number 75) domain])
        (set "task_entries" (array [installedTask]) (set "watch" scopedSummary (set "lifetime" scopedReceipt (completedEnvelope 224))))))
  scopedAudit <- either error pure (checkLifetimeArrayAudit scopedRequest (Right scopedWatch) (parseFinalModalWatch scopedWatch))
  installedReport <- either error pure (parseModalReport permitLimit completedScoped)
  finalScoped <- either error pure (parseFinalModalWatch scopedWatch)
  check "owned fetch binds the installed branch publication" (checkCompletedModalLifetime scopedRequest scopedAudit finalScoped installedReport == Right ())
  let stockTask = set "fetch" (set "bytes" (array (replicate 4 (Number 0))) (get "fetch" taskEntry)) taskEntry
  stockReport <- either error pure (parseModalReport permitLimit (set "task_entries" (array [stockTask]) completedScoped))
  check "owned fetch rejects the pre-install stock word" (isLeft (checkCompletedModalLifetime scopedRequest scopedAudit finalScoped stockReport))
  let retiredPairs = [[0x00c7aef4, 0x00c7af6c], [0x00c7af6c, 0x00c7afc8]] :: [[Word32]]
      retiredSpans = [range 0x00c7aef4 0x00c7af6c, range 0x00c7af6c 0x00c7afc8]
      retiredDeclarationWith pairs = set "limit" lifetimeLimit (set "context_sources" (array context)
        (set "lifetime" (set "retired" (get "r" (object ["r" .= (pairs :: [[Word32]])])) scopedLifetime) (modalBeginRequestJson scopedBase)))
  retiredRequest <- either (error . ("retired declaration fixture: " ++)) pure (parseModalBeginRequest (retiredDeclarationWith retiredPairs))
  check "typed lifetime request preserves adjacent retired extents" (modalBeginRequestJson retiredRequest == retiredDeclarationWith retiredPairs)
  mapM_ (\(label, pairs) -> check ("lifetime request refuses retired " ++ label) (isLeft (parseModalBeginRequest (retiredDeclarationWith pairs))))
    [ ("overlap", [[0x00c7aef4, 0x00c7af70], [0x00c7af6c, 0x00c7afc8]])
    , ("unaligned", [[0x00c7aef6, 0x00c7af6c]])
    , ("empty extent", [[0x00c7aef4, 0x00c7aef4]])
    , ("inside runtime", [[0x10000, 0x10010]])
    , ("65 extents", [[0x00c00000 + index * 8, 0x00c00004 + index * 8] | index <- [0 .. 64]]) ]
  check "retired extent over the watched action table refuses its watch ranges"
    (isLeft (parseModalBeginRequest (retiredDeclarationWith [[0x00d04080, 0x00d04090]]) >>= modalLifetimeWatchRanges))
  check "adjacent retired extents merge in the watched union" (unionWatchRanges retiredSpans == [range 0x00c7aef4 0x00c7afc8])
  let fetchRow physical clock = set "source" (String "cpu-fetch") (set "value" Null (set "before" Null
        (set "pc" (Number (fromIntegral physical)) (cpu 0 physical physical 4 clock))))
      retiredWatch records = set "fetch_attribution" (fetchJson helperFetches) (digested (object
        [ "enabled" .= True, "views" .= ("fixed" :: String), "ranges" .= map pair (unionWatchRanges (scopedWrites ++ retiredSpans ++ reloadSpans))
        , "records" .= records, "violation" .= Null
        , "classes" .= ([object ["range" .= pair span_, "kinds" .= (["cpu-write","host-write","model-bus-write"] :: [String])] | span_ <- scopedWrites]
            ++ [object ["range" .= pair span_, "kinds" .= (["cpu-fetch"] :: [String])] | span_ <- retiredSpans ++ reloadSpans])
        , "fetch_attribution" .= fetchJson fetches ]))
      published = take 223 writes ++ [forwardEntry, reverseEntry] ++ reverseRows
      completes request_ rowsWatch = either (const False) (const True) $ do
        audit_ <- checkLifetimeArrayAudit request_ (Right rowsWatch) (parseFinalModalWatch rowsWatch)
        final_ <- parseFinalModalWatch rowsWatch
        report_ <- parseModalReport permitLimit (set "watch" (withoutRecords rowsWatch) completedScoped)
        checkCompletedModalLifetime request_ audit_ final_ report_
  check "completed lifetime accepts the retired watch without fetch rows" (completes retiredRequest (retiredWatch published))
  check "completed lifetime accepts a retired fetch before the first forward write"
    (completes retiredRequest (retiredWatch (fetchRow 0x00c7af6c 0 : published)))
  check "completed lifetime accepts a retired fetch after the last reverse write"
    (completes retiredRequest (retiredWatch (published ++ [fetchRow 0x00c7aef4 9000])))
  check "completed lifetime refuses a retired fetch between publication writes"
    (not (completes retiredRequest (retiredWatch (take 10 published ++ [fetchRow 0x00c7af6c 10] ++ drop 10 published))))
  check "completed lifetime refuses a fetch outside every retired extent"
    (not (completes retiredRequest (retiredWatch (published ++ [fetchRow 0x00c7afc8 9000]))))
  check "a request without retired extents refuses a retired-extent fetch row"
    (not (completes scopedRequest (set "classes" (get "classes" scopedWatch) (set "ranges" (get "ranges" scopedWatch)
      (retiredWatch (published ++ [fetchRow 0x00c7aef4 9000]))))))
  let settingsRow pc = set "pc" (Number pc) (set "value" (Number 0x21) (set "before" (array [Number 0x20]) (cpu 0 0x0096ccca 0x0096ccca 1 100)))
      settingsWatch pc = set "fetch_attribution" (fetchJson helperFetches) (watch scopedWrites (take 100 published ++ [settingsRow pc] ++ drop 100 published))
      settingsCompletesWithCall resource setter reload call rowsWatch = either (const False) (const True) $ do
        audit_ <- checkLifetimeArrayAudit scopedRequest (Right rowsWatch) (parseFinalModalWatch rowsWatch)
        final_ <- parseFinalModalWatch rowsWatch
        report_ <- parseModalReport permitLimit (set "watch" (withoutRecords rowsWatch) (set "lifetime"
          (set "admitted_call_writes" (Number call) (set "admitted_reload_writes" (Number reload) (set "admitted_settings_writes" (Number setter)
            (set "checked_resource_writes" (Number resource) scopedReceipt)))) completedScoped))
        checkCompletedModalLifetime scopedRequest audit_ final_ report_
      settingsCompletesWith resource setter reload = settingsCompletesWithCall resource setter reload 0
      settingsCompletes admitted = settingsCompletesWith 1 admitted 0
  check "a counted settings change from the stock property setter completes" (settingsCompletes 1 (settingsWatch 0x0062ff54))
  check "an uncounted settings change from the stock property setter refuses" (not (settingsCompletes 0 (settingsWatch 0x0062ff54)))
  check "a settings change from outside every settings writer refuses" (not (settingsCompletes 1 (settingsWatch 0x0072212c)))
  check "the completed settings writes name the setter row"
    (fmap (map (get "pc") . values . get "writes" . settingsWritesJson)
      (parseFinalModalWatch (settingsWatch 0x0062ff54) >>= completedSettingsWrites scopedRequest) == Right [Number 0x0062ff54])
  let fetchOn core physical = set "core" (Number core) (fetchRow physical 100)
      reloadStore core address = set "pc" (Number 0x00722134) (set "value" (Number 0xa9c9) (set "before" (array [Number 0x20, Number 0x20])
        (cpu core address address 2 100)))
      reloadWatch reloadRows = set "fetch_attribution" (fetchJson helperFetches) (watch scopedWrites (take 100 published ++ reloadRows ++ drop 100 published))
      bracketed = [fetchOn 0 0x006309ec, reloadStore 0 0x0096cc74, fetchOn 0 0x006309f0]
      setterThenReload = [settingsRow 0x0062ff54] ++ bracketed
  check "a counted reload copy store inside its call bracket completes" (settingsCompletesWith 1 0 1 (reloadWatch bracketed))
  check "an uncounted reload copy store refuses" (not (settingsCompletesWith 1 0 0 (reloadWatch bracketed)))
  check "a reload copy store counted as a setter write refuses" (not (settingsCompletesWith 1 1 0 (reloadWatch bracketed)))
  check "setter and reload stores complete with both counters" (settingsCompletesWith 2 1 1 (reloadWatch setterThenReload))
  check "the completed settings writes order setter and reload rows with their writer kinds"
    (fmap (map (\write -> (get "writer" write, get "pc" write)) . values . get "writes" . settingsWritesJson)
      (parseFinalModalWatch (reloadWatch setterThenReload) >>= completedSettingsWrites scopedRequest)
      == Right [(String "setter", Number 0x0062ff54), (String "reload", Number 0x00722134)])
  mapM_ (\(label, rows_) -> check ("a reload copy store refuses " ++ label) (not (settingsCompletesWith 1 0 1 (reloadWatch rows_))))
    [ ("without its call fetch", [reloadStore 0 0x0096ccd2, fetchOn 0 0x006309f0])
    , ("after its return fetch", [fetchOn 0 0x006309ec, fetchOn 0 0x006309f0, reloadStore 0 0x0096cc74])
    , ("after a return fetch alone", [fetchOn 0 0x006309f0, reloadStore 0 0x0096cc74])
    , ("on the other core", [fetchOn 1 0x006309ec, reloadStore 0 0x0096cc74, fetchOn 1 0x006309f0])
    , ("from another PC", [fetchOn 0 0x006309ec, set "pc" (Number 0x00722138) (reloadStore 0 0x0096cc74), fetchOn 0 0x006309f0])
    , ("outside its target", [fetchOn 0 0x006309ec, reloadStore 0 0x0096cc70, fetchOn 0 0x006309f0]) ]
  let identityStore core address = set "value" (Number 0x2020) (reloadStore core address)
      wordStore = set "pc" (Number 0x00722134) (set "value" (Number 0x20202020) (set "before" (array (replicate 4 (Number 0x20)))
        (cpu 0 0x0096cc74 0x0096cc74 4 100)))
  check "an unchanged copy store advances the cursor to the next admitted halfword"
    (settingsCompletesWith 2 0 1 (reloadWatch [fetchOn 0 0x006309ec, identityStore 0 0x0096cc74, reloadStore 0 0x0096cc76, fetchOn 0 0x006309f0]))
  mapM_ (\(label, resource, rows_) -> check ("a later copy store refuses after " ++ label ++ " closes the bracket")
      (not (settingsCompletesWith resource 0 1 (reloadWatch rows_))))
    [ ("an out-of-order store", 2, [fetchOn 0 0x006309ec, identityStore 0 0x0096cc76, reloadStore 0 0x0096cc74, fetchOn 0 0x006309f0])
    , ("a width-4 store", 2, [fetchOn 0 0x006309ec, wordStore, reloadStore 0 0x0096cc74, fetchOn 0 0x006309f0])
    , ("a skipped halfword", 3, [fetchOn 0 0x006309ec, identityStore 0 0x0096cc74, identityStore 0 0x0096cc78, reloadStore 0 0x0096cc76, fetchOn 0 0x006309f0]) ]
  let restoreCopy call changed = [fetchOn 1 call]
        ++ [if address == changed then reloadStore 1 address else identityStore 1 address | address <- [0x0096cc64, 0x0096cc66 .. 0x0096ce24]]
        ++ [fetchOn 1 0x00637ee8]
  check "the BACK restore admits its changed halfword inside a whole-block copy"
    (settingsCompletesWith 225 0 1 (reloadWatch (restoreCopy 0x00637ee4 0x0096ccd2)))
  check "the BACK restore admits a changed halfword at the block start"
    (settingsCompletesWith 225 0 1 (reloadWatch (restoreCopy 0x00637ee4 0x0096cc64)))
  check "the property-write reload call does not admit a whole-block copy's change before its target"
    (not (settingsCompletesWith 225 0 1 (reloadWatch (restoreCopy 0x006309ec 0x0096cc64))))
  check "a fetch row beside a reload site refuses" (not (settingsCompletesWith 0 0 0 (reloadWatch [fetchOn 0 0x006309f4])))
  check "a reload call and return fetch pair without stores completes" (settingsCompletesWith 0 0 0 (reloadWatch [fetchOn 1 0x006309ec, fetchOn 1 0x006309f0]))
  let callStore core pc address value = set "pc" (Number pc) (set "value" (Number value) (set "before" (array [Number 0x20, Number 0x20])
        (cpu core address address 2 100)))
      textEdit core = [fetchOn (fromIntegral core) 0x0061c974, callStore core 0x007221b0 0x0096cd7c 0x2020, callStore core 0x00722134 0x0096cd7c 0x3130
        , callStore core 0x00722134 0x0096cd7e 0x3030, fetchOn (fromIntegral core) 0x0061c978]
      callCompletes resource setter call rows_ = settingsCompletesWithCall resource setter 0 call (reloadWatch rows_)
  check "a text setter call's writer stores inside its bracket complete with the call counter"
    (callCompletes 3 0 2 (textEdit 0) && callCompletes 3 0 2 (textEdit 1))
  check "an uncounted text setter call store refuses" (not (callCompletes 3 0 0 (textEdit 0)))
  check "a text setter call store counted as a setter write refuses" (not (callCompletes 3 2 0 (textEdit 0)))
  check "the completed settings writes name text setter call rows"
    (fmap (map (\write -> (get "writer" write, get "pc" write)) . values . get "writes" . settingsWritesJson)
      (parseFinalModalWatch (reloadWatch (textEdit 0)) >>= completedSettingsWrites scopedRequest)
      == Right [(String "call", Number 0x00722134), (String "call", Number 0x00722134)])
  mapM_ (\(label, rows_) -> check ("a text setter store refuses " ++ label) (not (callCompletes 1 0 1 rows_)))
    [ ("without its call fetch", [callStore 0 0x00722134 0x0096cd7c 0x3130, fetchOn 0 0x0061c978])
    , ("after its return fetch", [fetchOn 0 0x0061c974, fetchOn 0 0x0061c978, callStore 0 0x00722134 0x0096cd7c 0x3130])
    , ("after a return fetch alone", [fetchOn 0 0x0061c978, callStore 0 0x00722134 0x0096cd7c 0x3130])
    , ("on the other core", [fetchOn 1 0x0061c974, callStore 0 0x00722134 0x0096cd7c 0x3130, fetchOn 1 0x0061c978])
    , ("from outside every call writer", [fetchOn 0 0x0061c974, callStore 0 0x00722178 0x0096cd7c 0x3130, fetchOn 0 0x0061c978])
    , ("outside its target", [fetchOn 0 0x0061c974, callStore 0 0x00722134 0x0096cd8c 0x3130, fetchOn 0 0x0061c978])
    , ("inside a reload bracket", [fetchOn 0 0x006309ec, callStore 0 0x007221b0 0x0096cd7c 0x3130, fetchOn 0 0x006309f0]) ]
  check "a text setter call and return fetch pair without stores completes"
    (settingsCompletesWith 0 0 0 (reloadWatch [fetchOn 1 0x0061c974, fetchOn 1 0x0061c978]))
  let settingsWrite writer address value = object ["writer" .= (writer :: String), "instructions" .= (1 :: Int), "pc" .= (0x0062ff54 :: Word32)
        , "address" .= (address :: Word32), "size" .= (1 :: Int), "before" .= [0x20 :: Int], "value" .= (value :: Int)]
      sequenceOf writes_ = either error id (parseSettingsWrites (object ["schema" .= (2 :: Int), "writes" .= writes_]))
      stockSequence = sequenceOf [settingsWrite "setter" 0x0096ccca 0x21, settingsWrite "reload" 0x0096ccd2 0xc9]
  check "equal stock and owned settings sequences compare equal"
    (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "setter" 0x0096ccca 0x21, settingsWrite "reload" 0x0096ccd2 0xc9]) == Right ())
  check "a reordered owned settings sequence fails comparison"
    (isLeft (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "reload" 0x0096ccd2 0xc9, settingsWrite "setter" 0x0096ccca 0x21])))
  check "an extra owned settings write fails comparison"
    (isLeft (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "setter" 0x0096ccca 0x21, settingsWrite "reload" 0x0096ccd2 0xc9, settingsWrite "setter" 0x0096ccca 0x20])))
  check "a different owned settings value fails comparison"
    (isLeft (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "setter" 0x0096ccca 0x21, settingsWrite "reload" 0x0096ccd2 0xc8])))
  check "a missing owned reload write fails comparison"
    (isLeft (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "setter" 0x0096ccca 0x21])))
  check "the same store from another writer kind fails comparison"
    (isLeft (compareSettingsWrites stockSequence (sequenceOf [settingsWrite "setter" 0x0096ccca 0x21, settingsWrite "setter" 0x0096ccd2 0xc9])))
  check "a schema1 settings sequence refuses"
    (isLeft (parseSettingsWrites (object ["schema" .= (1 :: Int), "writes" .= ([] :: [Value])])))
  let withoutSettings = filter (\span_ -> let (low,high) = watchRangeBounds span_ in not (low <= 0x96cc64 && 0x96ce26 <= high)) expectedWrites
      incompleteWatch = watch withoutSettings writes
  check "array audit requires settings and all other expected write classes" $
    isLeft (checkLifetimeArrayAudit request (Right incompleteWatch) (parseFinalModalWatch incompleteWatch))
  wrongCount <- either error pure (parseModalReport permitLimit (set "lifetime" (set "publication_count" (Number 224) exactReceipt) clean))
  check "completed receipt count must match source vector" (isLeft (checkCompletedModalLifetime request audit finalFull wrongCount))
  let shortWatch = watch expectedWrites (drop 1 writes)
  finalShort <- either error pure (parseFinalModalWatch shortWatch)
  check "report summary must describe the final watch rows" (isLeft (checkCompletedModalLifetime request audit finalShort report))
  missingRow <- either error pure (parseModalReport permitLimit (set "watch" (withoutRecords shortWatch) clean))
  check "accepted receipt counts cannot exceed raw write rows" (isLeft (checkCompletedModalLifetime request audit finalShort missingRow))
  wrongAudit <- either error pure (parseModalReport permitLimit (set "lifetime" (set "audited_writes" (Number 1) exactReceipt) clean))
  check "mutable array audit count must match the raw writer summary" (isLeft (checkCompletedModalLifetime request audit finalFull wrongAudit))
  missingContext <- either error pure (parseModalReport permitLimit (set "context_source_spans" (Number 0) (set "enabled" (Bool True) clean)))
  check "begin acknowledgement must bind the exact context source count" (isLeft (checkModalBegin request missingContext))
  let vfpOwnerRow = set "pc" (Number 0x42440) (set "value" (Number 7) (set "before" (array (replicate 4 (Number 0xff)))
        (cpu 0 0x4dad4 0x4dad4 4 500)))
      vfpWatch = watch expectedWrites (writes ++ [vfpOwnerRow])
  vfpAudit <- either error pure (checkLifetimeArrayAudit request (Right vfpWatch) (parseFinalModalWatch vfpWatch))
  finalVfp <- either error pure (parseFinalModalWatch vfpWatch)
  vfpCounted <- either error pure (parseModalReport permitLimit (set "watch" (withoutRecords vfpWatch)
    (set "lifetime" (set "checked_vfp_owner_writes" (Number 1) exactReceipt) clean)))
  check "a counted lazy-VFP owner store accounts for its raw write row" (checkCompletedModalLifetime request vfpAudit finalVfp vfpCounted == Right ())
  vfpUncounted <- either error pure (parseModalReport permitLimit (set "watch" (withoutRecords vfpWatch) clean))
  check "an uncounted lazy-VFP owner write row refuses" (isLeft (checkCompletedModalLifetime request vfpAudit finalVfp vfpUncounted))
  let withoutVfpCounter = case exactReceipt of Object fields -> Object (KM.delete "checked_vfp_owner_writes" fields); other -> other
  check "lifetime receipt requires its lazy-VFP owner counter"
    (isLeft (parseModalReport permitLimit (set "lifetime" withoutVfpCounter clean)))
 where
  values (Array entries) = foldr (:) [] entries
  values _ = []
  pair span_ = let (low,high) = watchRangeBounds span_ in [low,high]
