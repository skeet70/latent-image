{-# LANGUAGE OverloadedStrings #-}
-- | Synthetic receipts exercise the host admission boundary.
module Main where

import Control.Monad (unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits ((.|.))
import Data.Maybe (fromMaybe, isJust)
import Data.Word (Word32, Word64)
import qualified Data.Vector as V
import Emulation.FirstStageWitness (encodeLeWord)
import Emulation.LoaderPrefixWitness
import Emulation.PollPeriod (PollPeriod (..))
import qualified Emulation.PeerStartupWitness as S
import qualified FujiAF.CompletionControl as C
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)
import System.Exit (exitFailure)
import qualified StartupStageContract as Stage

expect :: String -> Bool -> IO ()
expect label ok = unless ok (putStrLn ("FAIL " ++ label) >> exitFailure)

accepted :: Either String a -> Bool
accepted = either (const False) (const True)

argument, stackTop, cpsr :: Word32
argument = 0x69cdf8
stackTop = 0x400f3420
-- | Masked SVC, A32 little-endian, Z set.
cpsr = 0x400001d3

retainedRecord, loaderB :: String
retainedRecord = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"
loaderB = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"

symbolAt :: B.ByteString -> String -> IO Word32
symbolAt symbols name = either fail pure (F.parseSymbols symbols >>= fmap F.symbolAddress . flip F.lookupSymbol name)

-- | Record 2 holding the image at 0x10400 and loader B publishing @entry@.
stagedFor :: B.ByteString -> Word32 -> StagedImages
stagedFor bin entry =
  StagedImages
    (B.replicate 0x400 0 <> bin <> B.replicate (0x4c80 - 0x400 - B.length bin) 0)
    (B.replicate 0xc 0 <> encodeLeWord entry <> B.replicate (0xdaa0 - 0x10) 0)

retainedContract :: Scenario -> Either String Contract
retainedContract scenario = parseContract . BL.toStrict . encode $ object
  [ "schema" .= (4 :: Int), "budget" .= (50000000 :: Int), "scenario" .= object ["kind" .= scenarioName scenario]
  , "program" .= object
      [ "kind" .= ("retained" :: String), "record_sha256" .= retainedRecord, "loader_sha256" .= loaderB
      , "handoff" .= (0x11850 :: Int), "entry" .= (0xd0020 :: Int), "load" .= (0xd0a44 :: Int), "post_load" .= (0xd0a48 :: Int)
      , "loaded_register" .= (3 :: Int), "poll_period" .= (305 :: Int), "image" .= (Nothing :: Maybe [Int])
      , "refusal" .= (Nothing :: Maybe Int), "forbid_fetch" .= ([] :: [[Int]]), "forbid_read" .= ([] :: [[Int]])
      ]
  ]

data Knobs = Knobs
  { knobNonzeroBeforeLast :: Bool
  , knobCovered :: Word64
  , knobFrameReturn :: Word32
  , knobExitCpsr :: Word32
  , knobStoreOutsideCommit :: Bool
  , knobCore1MovesAtCommit :: Bool
  , knobNoWriter :: Bool
  , knobFault :: Bool
  , knobExitR8 :: Word32
  , knobDelayLoads :: Word64
  , knobStoredValue :: Maybe Word32
  , knobPostLoadSkips :: Bool
  -- | Counter totals and scoped row count at the prefix entry.
  , knobEntryObservation :: (Maybe (Word64, Word64, Word64), Maybe Int)
  , knobEntryCore1 :: Word64
  -- | Added to the last waiting post-load stop's core-1 count.
  , knobWaitSkew :: Word64
  -- | ATCM and BTCM region registers of cores 0 and 1 at every stop.
  , knobTcm :: [[Word32]]
  -- | Core-1 ATCM moved at the last waiting post-load stop.
  , knobTcmMoved :: Bool
  , knobFirstStageAfterSecondary :: Bool
  }

knobs :: Knobs
knobs = Knobs False 0x5660 0xd00c0 0x200001d3 False False False False 8 4 Nothing False (Just (0, 0, 0), Just 0) 201 0
  [[0x70000011, 0], [0x70000011, 0]] False False

writes :: [String]
writes = ["cpu-write", "host-write", "model-bus-write"]

spanOf :: Word64 -> Word64 -> [Word64]
spanOf low high = [low, high]

receipt :: Scenario -> ProgramKind -> ProgramFacts -> Knobs -> B.ByteString
receipt = receiptFor Nothing

receiptFor :: Maybe Value -> Scenario -> ProgramKind -> ProgramFacts -> Knobs -> B.ByteString
receiptFor startup scenario kind facts knob = BL.toStrict (encode (receiptDocument startup scenario kind facts knob))

receiptDocument :: Maybe Value -> Scenario -> ProgramKind -> ProgramFacts -> Knobs -> Value
receiptDocument startup scenario kind facts knob = document
  where
    composed = isJust startup
    Guest handoffSite = factHandoff facts
    Guest entrySite = factEntry facts
    Guest loadSite = factLoad facts
    Guest postSite = factPostLoad facts
    PollPeriod period = factPeriod facts
    loaded = if kind == OwnedProgram then 0 else 3 :: Int
    held = scenario == Held
    delayed = scenario == Delayed
    stored = fromMaybe (if scenario == NonOne then 0x400a0000 else 1) (knobStoredValue knob)
    base = [argument, 0x400a0000, 0, entrySite, 4, 5, 1, 7, 8, 9, 10, 0x700007fc, 12]
    replace index value list = take index list ++ [value] ++ drop (index + 1) list
    tcm = knobTcm knob
    totals (totalLoads, zeroLoads, nonzeroLoads) = object ["count" .= (totalLoads :: Word64), "zero_count" .= (zeroLoads :: Word64), "nonzero_count" .= (nonzeroLoads :: Word64)]
    stop :: String -> Word32 -> Int -> (Word64, Word64) -> [Word32] -> Word32 -> Word32 -> Word32 -> Maybe (Word64, Word64, Word64) -> Maybe Int -> Value
    stop = stopWith tcm
    stopWith :: [[Word32]] -> String -> Word32 -> Int -> (Word64, Word64) -> [Word32] -> Word32 -> Word32 -> Word32 -> Maybe (Word64, Word64, Word64) -> Maybe Int -> Value
    stopWith tcmWords label address core (core0, core1) registers sp lr flags counter cursor = object
      [ "label" .= label, "address" .= address, "core" .= core
      , "instructions" .= (core0 + core1), "counts" .= [core0, core1], "registers" .= registers
      , "sp" .= sp, "lr" .= lr, "cpsr" .= flags, "completion" .= (0 :: Word32)
      , "tcm" .= (tcmWords :: [[Word32]]), "counter" .= fmap totals counter, "watch_records" .= cursor ]
    postLoadWith tcmWords index counts = stopWith tcmWords "post-load" postSite 1 counts (replace loaded 0 base) 0x400f3400 0 cpsr (Just (index, index, 0)) (Just 0)
    postLoad = postLoadWith tcm
    lastWaiting = 3 :: Word64
    movedTcm = take 1 tcm ++ [[0x70004011, 0]]
    waitingPosts =
      [ postLoadWith (if knobTcmMoved knob && i == lastWaiting then movedTcm else tcm)
          (i + 1 + (if knobPostLoadSkips knob && i >= 2 then 1 else 0))
          (1000 + 100 * i, 300 + period * i + (if i == lastWaiting then knobWaitSkew knob else 0))
      | i <- [0 .. lastWaiting] ]
    heldPosts = [postLoad (i + 6) (20000 + 100 * i, 300 + period * (i + 10)) | i <- [0 .. 3]]
    delayLoads = knobDelayLoads knob
    storeCount = if delayed then 4 + delayLoads else 6
    delayStops =
      [ stop "delay-entry" C.veneerEntry 0 (2000, 300 + 4 * period) (replace 5 0 base) 0x700007d0 (C.heldGate + 4) cpsr (Just (4, 4, 0)) (Just 0)
      , stop "delay-release" C.veneerRelease 0 (3000, 300 + 4 * period + 10) (replace 5 0 base) 0x700007d0 (C.heldGate + 4) cpsr (Just (storeCount, storeCount, 0)) (Just 0) ]
    exitRegisters = replace 4 argument (replace 5 argument (replace 6 0xffff0000 (replace 8 (knobExitR8 knob) (replace 11 0x400f341c base))))
    startupStops =
      [ stop "first-stage-handoff" 0x6100 0 (if knobFirstStageAfterSecondary knob then (500, 20) else (100, 10))
          (replace 2 0x40020 base) 0x70000800 0 cpsr Nothing Nothing
      , stop "secondary-entry" 0x10400 1 (if knobFirstStageAfterSecondary knob then (100, 10) else (500, 20))
          base 0xfff00f00 0 cpsr Nothing Nothing ]
    stops = [stop "record-1-entry" (if composed then 0x6000 else 0x1800) 0 (10, 0) base 0xfff00f00 0 cpsr Nothing Nothing]
      ++ (if composed then (if knobFirstStageAfterSecondary knob then reverse startupStops else startupStops) else [])
      ++ [ stop "record-2-handoff" handoffSite 1 (800, 200) base 0x70000800 0 cpsr Nothing Nothing
         , stop "prefix-entry" entrySite 1 (810, knobEntryCore1 knob) base 0x70000800 0 cpsr (fst (knobEntryObservation knob)) (snd (knobEntryObservation knob)) ]
      ++ (if held
            then stop "held-gate" C.heldGate 0 (9000, 300 + 5 * period) base 0x700007d0 0 cpsr (Just (5, 5, 0)) (Just 0) : heldPosts
            else (if delayed then delayStops else [])
              ++ [ stop "completion-store" 0x40a08 0 (5000, 300 + 5 * period) (replace 5 1 (replace 3 0x400a0000 base)) 0x700007d0 0x40a04 cpsr (Just (storeCount, storeCount, 0)) (Just 0)
                 , stop "prefix-exit" 0xd0aac 1 (5010, 300 + 6 * period) exitRegisters 0x400f3410 0xd09bc (knobExitCpsr knob) (Just (storeCount + 1, storeCount, 1)) (Just 1) ])
    load :: Int -> Word32 -> Value
    load index value = object
      [ "source" .= ("cpu-read" :: String), "address" .= (0x400a00c4 :: Word32), "physical" .= (0xa00c4 :: Word32), "size" .= (4 :: Int)
      , "value" .= value, "before" .= B.unpack (encodeLeWord value), "byte_order" .= ("little" :: String), "core" .= (1 :: Int)
      , "pc" .= loadSite, "instructions" .= (1000 + 100 * index), "tick" .= (40 + 4 * index) ]
    count = if held then 9 else fromIntegral storeCount + 1 :: Int
    loadValue index
      | held = 0
      | index == count - 1 = stored
      | knobNonzeroBeforeLast knob && index == count - 2 = stored
      | otherwise = 0
    loads = [load index (loadValue index) | index <- [0 .. count - 1]]
    nonzero = length (filter (/= 0) (map loadValue [0 .. count - 1]))
    firstNonzero = length (takeWhile (== 0) (map loadValue [0 .. count - 1]))
    writer = object
      [ "source" .= ("cpu-write" :: String), "address" .= (0x400a00c4 :: Word32), "physical" .= (0xa00c4 :: Word32), "size" .= (4 :: Int)
      , "value" .= stored, "before" .= [0, 0, 0, 0 :: Int], "byte_order" .= ("little" :: String), "core" .= (0 :: Int)
      , "pc" .= (0x40a08 :: Word32), "instructions" .= (5301 :: Int), "tick" .= (220 :: Int) ]
    physical low high = object ["physical" .= spanOf low high]
    snapshot core = object ["core" .= (core :: Int), "btcm" .= object ["register" .= (tcm !! core !! 1), "window" .= (Nothing :: Maybe [Int])]
      , "atcm" .= object ["register" .= (tcm !! core !! 0), "window" .= spanOf 0x70000000 0x70002000]]
    stackSpan = object ["tcm" .= object ["bank" .= ("atcm" :: String), "offsets" .= spanOf 0x7cc 0x800]]
    bound :: Word64 -> Word64 -> Word64 -> Word64 -> Word64 -> Value
    bound guest size address physicalBase mapSize = object
      [ "guest" .= guest, "size" .= size
      , "map" .= object ["core" .= (1 :: Int), "address" .= address, "physical" .= physicalBase, "size" .= mapSize] ]
    rule :: [Int] -> String -> String -> [Value] -> Value
    rule cores kindName mode spans = object ["cores" .= cores, "kind" .= kindName, "mode" .= mode, "spans" .= spans, "tcm" .= map snapshot cores]
    firstFetch = [physical 0x1800 0x1804 | composed]
    secondaryFetch = [physical low high | composed, S.Interval low high <- S.forbidFetchIntervals]
    secondaryRead = [physical low high | composed, S.Interval low high <- S.forbidReadIntervals]
    immutable = [physical low high | composed, (low, high) <- [(0, 0x40), (0x6000, 0x6800)]]
    ownedRules = if kind == OwnedProgram
      then [ rule [1] "cpu-fetch" "forbid" ([physical 0xd0020 0xd00c8, physical 0xd09ac 0xd09d0, physical 0xd0a44 0xd0a64] ++ firstFetch ++ secondaryFetch)
           , rule [1] "cpu-read" "forbid" (physical 0xd00c8 0xd00fc : secondaryRead) ]
        ++ [entry | composed, entry <- [rule [0] "cpu-fetch" "forbid" (firstFetch ++ secondaryFetch), rule [0] "cpu-read" "forbid" secondaryRead]]
      else []
    writeClass low high = object ["range" .= spanOf low high, "kinds" .= writes]
    startupClasses =
      [writeClass low high | composed, (low, high) <- [(0x6000, 0x6800), (0, 0x40)]]
      ++ [object ["range" .= spanOf 0x1800 0x1804, "kinds" .= ["cpu-fetch" :: String]] | composed]
      ++ [object ["range" .= spanOf low high, "kinds" .= [access]]
         | composed, (access, spans) <- [("cpu-fetch" :: String, S.forbidFetchIntervals), ("cpu-read", S.forbidReadIntervals)]
         , S.Interval low high <- spans]
    classes = [ writeClass 0xa00c4 0xa00c8, writeClass 0xd0000 0xddaa0, writeClass 0x10000 0x14c80
              , writeClass 0x40a00 0x40a10, writeClass 0x40afc 0x40b00
              , object ["range" .= spanOf 0xddaa0 0xf3420, "kinds" .= ["host-write", "model-bus-write" :: String]] ]
      ++ [writeClass 0x180 0x19c | delayed]
      ++ startupClasses
    scoped = object
      [ "enabled" .= False, "ranges" .= ([] :: [[Int]]), "records" .= [writer | not held && not (knobNoWriter knob)]
      , "violation" .= (Nothing :: Maybe String), "classes" .= classes, "views" .= ("fixed" :: String)
      , "fault" .= (if knobFault knob then Just (object ["kind" .= ("decoding-changed" :: String)]) else Nothing)
      , "tcm" .= [snapshot 0, snapshot 1]
      , "confinement" .= ([ rule [1] "cpu-write" "allow" [physical 0xddaa0 0xf3420, physical 0x3fff0024 0x3fff0040, stackSpan]
                          , rule [0] "cpu-write" "forbid" (physical 0xddaa0 0xf3420 : immutable) ] ++ ownedRules)
      , "counters" .= [object
          [ "cores" .= [1 :: Int], "cell" .= spanOf 0xa00c4 0xa00c8, "head_bound" .= (8 :: Int), "tail_bound" .= (2 :: Int)
          , "count" .= count, "zero_count" .= (count - nonzero), "nonzero_count" .= nonzero
          , "first_nonzero" .= (if nonzero == 0 then Nothing else Just (loads !! firstNonzero))
          , "first_nonzero_index" .= (if nonzero == 0 then Nothing else Just firstNonzero)
          , "head" .= take 8 loads, "tail" .= drop (count - 2) loads ]]
      , "coverage" .= [object ["cores" .= [1 :: Int], "range" .= spanOf 0xddaa0 0xf3420, "covered_words" .= knobCovered knob, "first_uncovered" .= (Nothing :: Maybe Int)]]
      ]
    record1Clear = object
      [ "source" .= ("cpu-write" :: String), "address" .= (0x400a00c4 :: Word32), "physical" .= (0xa00c4 :: Word32), "size" .= (4 :: Int)
      , "value" .= (0 :: Int), "before" .= [0, 0, 0, 0 :: Int], "byte_order" .= ("little" :: String), "core" .= (0 :: Int)
      , "pc" .= (0x1fec :: Word32), "instructions" .= (500 :: Int), "tick" .= (20 :: Int) ]
    classified rows watched = object
      [ "enabled" .= False, "ranges" .= ([] :: [[Int]]), "records" .= (rows :: [Value]), "violation" .= (Nothing :: Maybe String)
      , "classes" .= (watched :: [Value]), "views" .= ("observe" :: String) ]
    veneer = either (const B.empty) id C.delayVeneer
    words32 = B.unpack . B.concat . map encodeLeWord
    frame = words32 [stackTop, argument, 0x700007fc, knobFrameReturn knob]
    vectors = words32 [0xd0100, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0184]
    instruction = if scenario == NonOne then 0xe58330c4 else 0xe58350c4 :: Word32
    core1AtCommit = 300 + 5 * period
    commitStep = object ["before" .= [5000, core1AtCommit], "after" .= [5001, core1AtCommit + if knobCore1MovesAtCommit knob then 1 else 0], "cell" .= stored]
    document = object $
      [ "schema" .= (if composed then 5 else 4 :: Int), "scenario" .= object ["kind" .= scenarioName scenario], "program" .= programKindName kind
      , "result" .= ("passed" :: String), "error" .= (Nothing :: Maybe String), "machine_failure" .= (Nothing :: Maybe String)
      , "instructions" .= (30000 :: Int), "cp14_identities" .= [0x4003, 0x5003 :: Word32]
      , "boot_watch" .= classified [] [writeClass 0xa00c4 0xa00c8]
      , "loaded_watch" .= setField "views" (String (if composed then "fixed" else "observe"))
          (classified [record1Clear] ([writeClass 0xa00c4 0xa00c8] ++ [writeClass 0x180 0x19c | delayed] ++ startupClasses))
      , "scoped_watch" .= scoped, "stops" .= stops
      , "bindings" .= object
          [ "vector_slots" .= bound 0xffff0024 28 0xffff0000 0x3fff0000 0x10000, "stack_window" .= stackSpan
          , "zero_range" .= bound 0x400ddaa0 0x15980 0x40000000 0 0x10000000
          , "veneer" .= (if delayed then Just (object ["virtual" .= B.unpack veneer, "physical" .= B.unpack veneer]) else Nothing) ]
      , "completion" .= (if held then Nothing else Just (object
          [ "instruction" .= object ["virtual" .= instruction, "physical" .= instruction], "before_pc" .= (0x40a08 :: Word32)
          , "after_pc" .= (0x40a0c :: Word32), "before_cpsr" .= cpsr, "after_cpsr" .= cpsr, "microsteps" .= [commitStep]
          , "readback" .= object ["virtual" .= stored, "physical" .= stored]
          , "counter" .= totals (storeCount, storeCount, 0)
          , "watch_records" .= (if knobStoreOutsideCommit knob then 0 else 1 :: Int) ]))
      , "endpoint" .= (if held then Nothing else Just (object
          [ "frame" .= object ["virtual" .= frame, "physical" .= frame], "vectors" .= object ["virtual" .= vectors, "physical" .= vectors]
          , "zero" .= object ["range" .= spanOf 0xddaa0 0xf33ec, "first_nonzero" .= (Nothing :: Maybe Int)]
          , "record_sha256" .= F.sha256Hex (factRecord facts), "loader_sha256" .= F.sha256Hex (factLoader facts) ]))
      , "final_primary" .= (if held then Just (object
          [ "address" .= C.heldGate, "registers" .= base, "sp" .= (0x700007d0 :: Word32), "lr" .= (0 :: Word32)
          , "cpsr" .= cpsr, "instructions" .= (25000 :: Int) ]) else Nothing)
      ] ++ ["startup" .= selected | Just selected <- [startup]]

validate :: Contract -> B.ByteString -> Either String ()
validate contract bytes = parseReceipt bytes >>= validateReceipt contract >> pure ()

startupFixture :: Value
startupFixture = object
  [ "kind" .= ("composed" :: String), "record1_sha256" .= replicate 64 '1'
  , "image_sha256" .= replicate 64 '2', "symbols_sha256" .= replicate 64 '3', "source_sha256" .= replicate 64 '4'
  , "image" .= spanOf 0x6000 0x6800, "entry" .= (0x6000 :: Word32), "handoff" .= (0x6100 :: Word32)
  , "handoff_target" .= (0x40020 :: Word32), "handoff_argument" .= argument, "secondary_entry" .= (0x10400 :: Word32)
  , "secondary_image_sha256" .= replicate 64 '5', "secondary_symbols_sha256" .= replicate 64 '6'
  , "secondary_source_sha256" .= replicate 64 '4'
  ]

setField :: String -> Value -> Value -> Value
setField name value (Object fields) = Object (KM.insert (K.fromString name) value fields)
setField _ _ value = value

changeField :: String -> (Value -> Value) -> Value -> Value
changeField name change (Object fields) = Object (case KM.lookup key fields of
  Just value -> KM.insert key (change value) fields
  Nothing -> fields)
  where key = K.fromString name
changeField _ _ value = value

composedContract :: Scenario -> ProgramFacts -> Value -> Either String Contract
composedContract scenario facts startup =
  parseContract . BL.toStrict . encode . setField "startup" startup . setField "schema" (Number 5) $
    contractValue (Contract scenario (Program OwnedProgram facts))

filterArray :: (Value -> Bool) -> Value -> Value
filterArray predicate (Array values) = Array (V.filter predicate values)
filterArray _ value = value

hasField :: String -> Value -> Value -> Bool
hasField name expected (Object fields) = KM.lookup (K.fromString name) fields == Just expected
hasField _ _ _ = False

changeStop :: String -> (Value -> Value) -> Value -> Value
changeStop label change = changeField "stops" (mapArray (\value -> if hasField "label" (String (fromString label)) value then change value else value))
  where
    fromString = K.toText . K.fromString
    mapArray f (Array values) = Array (V.map f values)
    mapArray _ value = value

main :: IO ()
main = do
  args <- getArgs
  (bin, symbols, refusalBin, refusalSymbols) <- case args of
    [a, b, c, d] -> (,,,) <$> B.readFile a <*> B.readFile b <*> B.readFile c <*> B.readFile d
    _ -> fail "usage: loader-prefix-witness-tests IMAGE.bin IMAGE.symbols REFUSAL.bin REFUSAL.symbols"
  prefixEntry <- symbolAt symbols "xm1_loader_prefix_entry"
  dispatch <- symbolAt symbols "xm1_loader_prefix_dispatch"
  facts <- either fail pure (deriveOwnedFacts bin symbols (stagedFor bin prefixEntry))
  expect "combined prefix artifact is admitted at its typed boundary"
    (accepted (Stage.parseSecondaryPrefixArtifact (U.MediaImage bin) symbols))
  expect "secondary artifact cannot be admitted as a firststage artifact"
    (not (accepted (Stage.parseFirstStageArtifact (U.MediaImage bin) symbols)))
  expect "truncated secondary artifact is refused at its typed boundary"
    (not (accepted (Stage.parseSecondaryPrefixArtifact (U.MediaImage (B.take (B.length bin - 4) bin)) symbols)))
  let Guest entry = factEntry facts
      Guest post = factPostLoad facts
      PollPeriod period = factPeriod facts
  expect "owned entry is the published prefix entry" (entry == prefixEntry)
  expect "owned post-load site lies in dispatch" (post > dispatch && post < dispatch + 0x60)
  expect "owned poll period is derived" (period > 0)
  expect "correct image declares no refusal" (factRefusal facts == Nothing)
  expect "loader B naming another entry is refused" (not (accepted (deriveOwnedFacts bin symbols (stagedFor bin 0xd0020))))
  refusalEntry <- symbolAt refusalSymbols "xm1_loader_prefix_entry"
  refusalFacts <- either fail pure (deriveOwnedFacts refusalBin refusalSymbols (stagedFor refusalBin refusalEntry))
  expect "refusal control declares its refusal site" (factRefusal refusalFacts /= Nothing)
  let owned scenario = Contract scenario (Program OwnedProgram facts)
      ownedReceipt scenario = receipt scenario OwnedProgram facts
      natural = validate (owned Natural) . ownedReceipt Natural
  expect "contract round trip" (parseContract (BL.toStrict (encode (contractValue (owned Natural)))) == Right (owned Natural))
  expect "natural owned receipt accepted" (accepted (natural knobs))
  expect "a nonzero load before the last is refused" (not (accepted (natural knobs {knobNonzeroBeforeLast = True})))
  expect "a fill one word short is refused" (not (accepted (natural knobs {knobCovered = 0x565f})))
  expect "a wrong frame return word is refused" (not (accepted (natural knobs {knobFrameReturn = 0xd00c4})))
  expect "exit CPSR differing only in NZCV is accepted" (accepted (natural knobs {knobExitCpsr = cpsr .|. 0xf0000000}))
  expect "exit CPSR differing in Q is refused" (not (accepted (natural knobs {knobExitCpsr = cpsr .|. 0x08000000})))
  expect "a missing original store is refused" (not (accepted (natural knobs {knobNoWriter = True})))
  expect "a store row outside the commit window is refused" (not (accepted (natural knobs {knobStoreOutsideCommit = True})))
  expect "a commit during which core 1 advances is refused" (not (accepted (natural knobs {knobCore1MovesAtCommit = True})))
  expect "a post-load stop pairing with two loads is refused" (not (accepted (natural knobs {knobPostLoadSkips = True})))
  expect "a scoped fault is refused" (not (accepted (natural knobs {knobFault = True})))
  expect "a natural receipt under the held contract is refused" (not (accepted (validate (owned Held) (ownedReceipt Natural knobs))))
  expect "held owned receipt accepted" (accepted (validate (owned Held) (ownedReceipt Held knobs)))
  expect "non-one owned receipt accepted" (accepted (validate (owned NonOne) (ownedReceipt NonOne knobs)))
  expect "a store of 1 under the non-one contract is refused" (not (accepted (validate (owned NonOne) (ownedReceipt NonOne knobs {knobStoredValue = Just 1}))))
  expect "delayed owned receipt accepted" (accepted (validate (owned Delayed) (ownedReceipt Delayed knobs)))
  expect "a delay with three zero loads is refused" (not (accepted (validate (owned Delayed) (ownedReceipt Delayed knobs {knobDelayLoads = 3}))))
  let held = validate (owned Held) . ownedReceipt Held
      delayedWith = validate (owned Delayed) . ownedReceipt Delayed
      cellTcm = [[0x70000011, 0x000a0015], [0x70000011, 0]]
  expect "a prefix entry without a scoped counter is refused" (not (accepted (natural knobs {knobEntryObservation = (Nothing, Nothing)})))
  expect "a prefix entry with a nonempty scoped counter is refused" (not (accepted (natural knobs {knobEntryObservation = (Just (1, 1, 0), Just 0)})))
  expect "a prefix entry with scoped rows is refused" (not (accepted (natural knobs {knobEntryObservation = (Just (0, 0, 0), Just 1)})))
  expect "a prefix entry two core-1 instructions after the handoff is refused" (not (accepted (natural knobs {knobEntryCore1 = 202})))
  expect "a waiting poll off the derived period is refused" (not (accepted (natural knobs {knobWaitSkew = 1})))
  expect "a held waiting poll off the derived period is refused" (not (accepted (held knobs {knobWaitSkew = 1})))
  expect "TCM moved after the handoff is refused" (not (accepted (natural knobs {knobTcmMoved = True})))
  expect "a held TCM moved after the handoff is refused" (not (accepted (held knobs {knobTcmMoved = True})))
  expect "a core-0 TCM window away from every guarded view is accepted" (accepted (natural knobs {knobTcm = [[0x70000011, 0x60000011], [0x70000011, 0]]}))
  expect "a core-0 TCM window over the completion cell is refused" (not (accepted (natural knobs {knobTcm = cellTcm})))
  expect "a held core-0 TCM window over the completion cell is refused" (not (accepted (held knobs {knobTcm = cellTcm})))
  expect "a core-0 TCM window over the delayed veneer is refused" (not (accepted (delayedWith knobs {knobTcm = [[0x70000011, 0x00000011], [0x70000011, 0]]})))
  retained <- either fail pure (retainedContract Natural)
  retainedProgramFacts <- case retained of
    Contract _ (Program _ selectedFacts) -> pure selectedFacts
    ComposedContract {} -> fail "retained fixture names composed startup"
  let retainedReceipt = receipt Natural RetainedProgram retainedProgramFacts
  expect "natural retained receipt accepted" (accepted (validate retained (retainedReceipt knobs)))
  let pairOf contract bytes = (,) contract <$> parseReceipt bytes
      compareWith ownedKnobs = do
        left <- pairOf retained (retainedReceipt knobs)
        right <- pairOf (owned Natural) (ownedReceipt Natural ownedKnobs)
        compareReceipts left right
  expect "retained and owned exits compare equal" (accepted (compareWith knobs))
  expect "an owned exit r8 differing from retained is refused" (not (accepted (compareWith knobs {knobExitR8 = 9})))
  expect "comparison with swapped programs is refused" (not (accepted (do
    left <- pairOf (owned Natural) (ownedReceipt Natural knobs)
    right <- pairOf retained (retainedReceipt knobs)
    compareReceipts left right)))
  composed <- either fail pure (composedContract Natural facts startupFixture)
  let composedBytes scenario knob = receiptFor (Just startupFixture) scenario OwnedProgram facts knob
      altered change = BL.toStrict (encode (change (receiptDocument (Just startupFixture) Natural OwnedProgram facts knobs)))
      removeClass span' = changeField "classes" (filterArray (not . hasField "range" span'))
      withoutImageGuard = removeClass (toJSONSpan 0x6000 0x6800)
      toJSONSpan low high = Array (V.fromList [Number low, Number high])
      missingStop label = changeField "stops" (filterArray (not . hasField "label" (String label)))
      refuses label change = expect label (not (accepted (validate composed (altered change))))
      refusesStartup label change = expect label (not (accepted (composedContract Natural facts (change startupFixture))))
      expectRefused label result = expect label (not (accepted result))
      parseDocument = parseContract . BL.toStrict . encode
      addImageWrite (Array rows) = Array (V.snoc rows (object
        [ "source" .= ("host-write" :: String), "address" .= (0x40006000 :: Word64), "physical" .= (0x6000 :: Word64), "size" .= (4 :: Int)
        , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null ]))
      addImageWrite value = value
      setTarget (Array registers) = Array (registers V.// [(2, Number 0x40024)])
      setTarget value = value
  expect "composed contract round trip" (parseContract (BL.toStrict (encode (contractValue composed))) == Right composed)
  mapM_ (\scenario -> do
    selected <- either fail pure (composedContract scenario facts startupFixture)
    expect ("composed " ++ scenarioName scenario ++ " accepted") (accepted (validate selected (composedBytes scenario knobs)))) [Natural, Held, Delayed, NonOne]
  expect "secondary entry can precede the firststage handoff" (accepted (validate composed (composedBytes Natural knobs {knobFirstStageAfterSecondary = True})))
  expect "standalone receipt cannot establish composed startup" (not (accepted (validate composed (ownedReceipt Natural knobs))))
  expect "composed receipt cannot establish standalone startup" (not (accepted (validate (owned Natural) (composedBytes Natural knobs))))
  expectRefused "retained roles are refused under schema5"
    (parseDocument (setField "startup" startupFixture (setField "schema" (Number 5) (contractValue retained))))
  expectRefused "schema4 refuses composed startup fields"
    (parseDocument (setField "startup" startupFixture (contractValue (owned Natural))))
  refusesStartup "composed admission refuses the stock firststage entry" (setField "entry" (Number 0x1800))
  refusesStartup "composed admission refuses a firststage image outside record 1" (setField "image" (toJSONSpan 0x6000 0x10004))
  refusesStartup "composed admission refuses different original DAT identities"
    (setField "secondary_source_sha256" (String (K.toText (K.fromString (replicate 64 '7')))))
  refuses "composed receipt binds the firststage symbols"
    (changeField "startup" (setField "symbols_sha256" (String (K.toText (K.fromString (replicate 64 '7'))))))
  refuses "endpoint identity does not replace the firststage interval guard" (changeField "loaded_watch" withoutImageGuard)
  refuses "firststage image remains guarded after prefix entry" (changeField "scoped_watch" withoutImageGuard)
  refuses "firststage vectors remain guarded after prefix entry" (changeField "scoped_watch" (removeClass (toJSONSpan 0 0x40)))
  refuses "secondary retired fetches are guarded before the handoff"
    (changeField "loaded_watch" (removeClass (toJSONSpan 0x11800 0x11854)))
  refuses "firststage handoff must be observed" (missingStop "first-stage-handoff")
  refuses "secondary entry must be observed" (missingStop "secondary-entry")
  refuses "firststage handoff must name its source-bound site" (changeStop "first-stage-handoff" (setField "address" (Number 0x6104)))
  refuses "firststage handoff must name its source-bound target" (changeStop "first-stage-handoff" (changeField "registers" setTarget))
  refuses "secondary entry must name its source-bound site" (changeStop "secondary-entry" (setField "address" (Number 0x10404)))
  refuses "stock firststage entry is not a composed entry observation" (changeStop "record-1-entry" (setField "address" (Number 0x1800)))
  refuses "restored endpoint bytes do not excuse an earlier image write" (changeField "loaded_watch" (changeField "records" addImageWrite))
  refuses "restored endpoint bytes do not excuse an image write during prefix execution" (changeField "scoped_watch" (changeField "records" addImageWrite))
  refuses "decoder observation does not replace fixed composed decoding" (changeField "loaded_watch" (setField "views" (String "observe")))
  expectRefused "an endpoint TCM window over the firststage image is refused"
    (validate composed (composedBytes Natural knobs {knobTcm = [[0x70000011, 0x0000600d], [0x70000011, 0]]}))
  expectRefused "an endpoint TCM window over the firststage alias is refused"
    (validate composed (composedBytes Natural knobs {knobTcm = [[0x70000011, 0x4000600d], [0x70000011, 0]]}))
  expect "retained and composed boundaries compare equal" (accepted (do
    left <- pairOf retained (retainedReceipt knobs)
    right <- pairOf composed (composedBytes Natural knobs)
    compareReceipts left right))
  putStrLn "loader-prefix witness tests pass"
