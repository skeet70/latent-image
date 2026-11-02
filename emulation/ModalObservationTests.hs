{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecode, encode, object, (.=))
import qualified Data.Aeson.Key as K
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
  , "physical_fetch_coverage" .= False, "task_identity_verified" .= False ]

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
  scope <- either error pure (modalScope linked (range 0x10000 0x10038) (range 0x10038 0x10060) (Just (watchAddress 0x10060)))
  scopedRequest <- either error pure (modalRequest stock (Just (linked, scope)) permitLimit)
  let scopedJson = modalBeginRequestJson scopedRequest
  check "separate linked owned prehook is encoded" (get "owned_prehook" scopedJson == Number 0x10060)
  check "unmapped prehook refuses" (isLeft (modalScope linked (range 0x10000 0x10038) (range 0x10038 0x10060) (Just (watchAddress 0x20000))))
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
      scoped = set "policy" (String "scoped-owned75") permitEnvelope
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
  mapM_ (\(label, value) -> do
    report <- either (\problem -> error ("scoped route fixture " ++ label ++ ": " ++ problem)) pure (parseModalReport permitLimit value)
    check ("scoped route refuses " ++ label) (isLeft (checkScopedModalRoute report))) rejected

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
syntheticModalElf textFlags = header <> syntheticModalText <> symbols <> names <> sectionNames <> padding <> sections
 where
  fields = [("rb_modal_entry", 0x10000, 56), ("rb_modal_dispatch", 0x10038, 32),
    ("rb_modal_call_prehook", 0x10058, 4), ("rb_modal_call", 0x1005c, 4)] :: [(B.ByteString, Word32, Word32)]
  nameOffsets = scanl (+) 1 [fromIntegral (B.length name + 1) | (name, _, _) <- fields]
  names = B.singleton 0 <> B.concat [name <> B.singleton 0 | (name, _, _) <- fields]
  symbols = B.replicate 16 0 <> B.concat
    [ word 4 nameOffset <> word 4 address <> word 4 size <> B.pack [0x12, 0] <> word 2 1
    | (nameOffset, (_, address, size)) <- zip nameOffsets fields ]
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
    , "rb_modal_call_prehook" .= function "0x00010058" 4
    , "rb_modal_call" .= function "0x0001005c" 4 ]]
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
  check "linked instruction bytes come from bound ELF text" $
    get "start" (get "linked" requestJson) == Number 0x10000
      && get "bytes" (get "linked" requestJson) == get "bytes" (object ["bytes" .= B.unpack syntheticModalText])
  let changedRuntime = B.cons 1 (B.drop 1 runtime)
      changedElf = B.cons 1 (B.drop 1 elf)
      writableElf = syntheticModalElf 7
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
