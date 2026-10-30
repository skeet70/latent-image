{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Result (..), Value (..), encode, fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Vector as V
import Data.Word (Word8, Word32, Word64)
import Emulation.PeerStageWitness (Argument (..), maxReceiptBytes)
import qualified Emulation.PeerStageWitness as Natural
import Emulation.PeerWaitWitness
import FujiAF.PeerWaitControl (PeerControl (..), controlName, parseControlName)
import System.Environment (getArgs)

expected :: Argument
expected = Argument 0x69cdf8
number :: Word64 -> Value
number = Number . fromIntegral
values :: [Value] -> Value
values = Array . V.fromList
set :: String -> Value -> Value -> Value
set key value (Object row) = Object (KM.insert (K.fromString key) value row)
set _ _ value = value
change :: String -> (Value -> Value) -> Value -> Value
change key f original@(Object row) = maybe original (\value -> set key (f value) original) (KM.lookup (K.fromString key) row)
change _ _ value = value
at :: Int -> (Value -> Value) -> Value -> Value
at index f (Array entries) = Array (entries V.// [(index,f (entries V.! index))])
at _ _ value = value
filterValues :: (Value -> Bool) -> Value -> Value
filterValues predicate (Array entries) = Array (V.filter predicate entries)
filterValues _ value = value
has :: String -> Value -> Value -> Bool
has key wanted (Object row) = KM.lookup (K.fromString key) row == Just wanted
has _ _ _ = False
stopped :: String -> (Value -> Value) -> Value -> Value
stopped wanted f = change "stops" (\value -> case value of
  Array entries -> Array (V.map (\stop -> if has "label" (String (fromString wanted)) stop then f stop else stop) entries)
  other -> other)
  where fromString = K.toText . K.fromString
recorded :: Word64 -> String -> (Value -> Value) -> Value -> Value
recorded pc source f = change "memory_watch" (change "records" (\value -> case value of
  Array entries -> Array (V.map (\row -> if has "pc" (number pc) row && has "source" (String (K.toText (K.fromString source))) row then f row else row) entries)
  other -> other))

state :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Value
state first second done target argument = object ["cell0" .= first,"cell1" .= second,"completion" .= done,"target" .= target,"argument" .= argument]
quiet :: Value
quiet = state 0x22b 0 0 0 0
hostRead :: Word32 -> Value
hostRead physical = object ["source" .= ("host-read" :: String),"address" .= physical,"physical" .= physical,"size" .= (4 :: Int),"core" .= Null,"pc" .= Null,"instructions" .= Null,"tick" .= Null]
observationReads :: [Value]
observationReads = map hostRead [0x20000,0x20004,0xa00c4,0x14c80,0x14c84]
bytes :: Word32 -> [Word8]
bytes word = [fromIntegral word,fromIntegral (word `div` 256),fromIntegral (word `div` 65536),fromIntegral (word `div` 16777216)]
cpu :: String -> Int -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word64 -> Value
cpu source core pc guest physical value before time = object
  [ "source" .= source,"core" .= core,"pc" .= pc,"address" .= guest,"physical" .= physical,"size" .= (4 :: Int)
  , "value" .= value,"before" .= bytes before,"byte_order" .= ("little" :: String),"instructions" .= time,"tick" .= time]
write :: Int -> Word32 -> Word32 -> Word32 -> Word32 -> Word64 -> Value
write core pc physical value before = cpu "cpu-write" core pc (if physical >= 0x20000 then physical + 0x40000000 else physical) physical value before
waitRead :: Word64 -> Value
waitRead = cpu "cpu-read" 1 0x11f74 0x40020000 0x20000 0x22b 0x22b
gateRegisters :: [Word32]
gateRegisters = [7,8,0,0,4,5,6,7,8,9,10,11,12]
restored :: [Word32]
restored = [0x69cdf8,0,0,0xd0020,0x44,0x55,0x66,0x77,0,0,0,0xb,0]
savedFrame :: [Word8]
savedFrame = concatMap bytes [0x44,0x55,0x66,0x77,0xb,0x1183c]
snapshot :: String -> Int -> Word32 -> Word64 -> [Word32] -> Word32 -> Value -> Value -> Value
snapshot name core pc time regs sp stateValue frame = object
  [ "label" .= name,"core" .= core,"address" .= pc,"instructions" .= time,"registers" .= regs,"sp" .= sp
  , "lr" .= (0x1234 :: Word32),"cpsr" .= (0xa00000d3 :: Word32),"state" .= stateValue,"frame" .= frame]
entry, tailStop, gate, firstWait, secondWait, thirdWait, fourthWait, ack, preclear, handoff, loader :: Value
entry = snapshot "record-2-entry" 1 0x11800 30 (replicate 13 0) 0 (state 0x22b 0x22b 1 1 1) Null
tailStop = snapshot "record-2-rendezvous-tail" 1 0x11f40 60 [0,0,0,0,0,0,1,0,0,0,0,0x7000103c,0] 0x70001000 (state 0x22b 0x22b 0 0 0) (values (map (number . fromIntegral) savedFrame))
gate = snapshot "primary-gate" 0 0x1ff4 70 gateRegisters 0x700007f0 (state 0x22b 0x22b 0 0 0) Null
firstWait = waitStop 100
secondWait = waitStop 110
thirdWait = waitStop 120
fourthWait = waitStop 130
waitStop :: Word64 -> Value
waitStop time = snapshot "peer-wait-loaded" 1 0x11f78 time [0,0x40020000,0x22b,0x22b,0,0,0,0,0,0,0,0,0] 0x70001000 quiet Null
ack = snapshot "peer-acknowledged" 0 0x14c 105 gateRegisters 0x700007d8 quiet Null
preclear = snapshot "primary-clear" 0 0x1ff8 150 [7,8,0,0x40020000,4,5,6,7,8,9,10,11,12] 0x700007f0 quiet Null
handoff = snapshot "record-2-handoff" 1 0x11850 180 restored 0x70001040 (state 0 0 0 0xd0020 0x69cdf8) Null
loader = snapshot "loader-b-entry" 1 0xd0020 190 restored 0x70001040 (state 0 0 17 0xd0020 0x69cdf8) Null
microstep :: [Word64] -> [Word64] -> Word32 -> Value
microstep before after scalar = object ["before" .= before,"after" .= after,"cell0" .= scalar]
readback :: Int -> Value
readback cursor = object
  [ "instructions" .= (152 :: Word64),"primary_address" .= (0x1ffc :: Word32),"state" .= state 0 0 0 0 0
  , "watch_records" .= cursor,"microsteps" .= [microstep [75,75] [75,76] 0x22b,microstep [75,76] [76,76] 0]
  , "primary_cpsr" .= (0xa00000d3 :: Word32),"primary_instruction" .= ([0,32,131,229] :: [Word8])]

gateBindings :: PeerControl -> Word64 -> Value
gateBindings selected time = object
  [ "core" .= (0 :: Int),"instructions" .= time,"decoder_choices" .= ([1] :: [Int])
  , "low_ram" .= object ["address" .= (0 :: Int),"physical" .= (0 :: Int),"size" .= (16384 :: Int),"permissions" .= (7 :: Int)]
  , "atcm" .= (0x70000011 :: Word32),"btcm" .= (0 :: Int),"stack_address" .= (0x700007d8 :: Word32),"stack" .= (replicate 40 0x5a :: [Word8])
  , "code" .= [extent 0x100 padding,extent 0x1fe0 predecessors]]
  where
    extent address contents = object ["address" .= (address :: Word32),"virtual" .= contents,"physical" .= contents]
    padding = if selected == HeldSentinel then replicate 104 0xff else concatMap bytes
      [3912060943,3775856640,3808563200,3812896770,3808477739,3851624452,3813801984,167772170
      ,3780247564,436207621,3818922084,3810586624,3797032961,452984828,3796963329,452984820
      ,3777556480,3904720911,3942645758,3808497664,3796963329,452984829,3777556480,3904720911
      ,3812896770,3925870499]
    predecessors = concatMap bytes [0xe3003000,0xe344300a,0xe3a02000,0xe58320c4,0xe3003000
      ,if selected == HeldSentinel then 0xeafffffe else 0xeafff841,0xe5832000,0xf57ff04f]

heldPrimary :: Value
heldPrimary = object ["address" .= (0x1ff4 :: Word32),"registers" .= gateRegisters,"sp" .= (0x700007f0 :: Word32)
  ,"lr" .= (0x1234 :: Word32),"cpsr" .= (0xa00000d3 :: Word32),"instructions" .= (130 :: Word64)]

-- Synthetic serialized consumer controls, not guest fault evidence.
fixture :: PeerControl -> Bool -> Value
fixture selected earlyGate = fixtureWithReads selected earlyGate 0

fixtureWithReads :: PeerControl -> Bool -> Int -> Value
fixtureWithReads selected earlyGate extraReads = object
  [ "schema" .= (2 :: Int),"control" .= controlName selected,"result" .= ("passed" :: String),"error" .= Null,"machine_failure" .= Null
  , "instructions" .= (if selected == HeldSentinel then 130 else 190 + delta :: Word64),"cp14_identities" .= ([0x4003,0x5003] :: [Word32])
  , "stops" .= snapshots,"clear_readback" .= committed
  , "bindings" .= gateBindings selected (if earlyGate then 50 else 70)
  , "final_primary" .= (if selected == HeldSentinel then heldPrimary else Null)
  , "memory_watch" .= object ["enabled" .= True,"ranges" .= ([[0x14c80,0x14c88],[0x20000,0x20008],[0xa00c4,0xa00c8]] :: [[Word32]]),"records" .= rows,"violation" .= Null]]
  where
    delta = if extraReads == 0 then 0 else 99850
    prefix = [(entry,[write 0 0x1e64 0x20000 0x22b 0x22b 10,write 0 0x1e68 0x20004 0x22b 0x22b 11])]
    initialization = [write 1 0x11818 0x14c80 0 0xffffffff 40,write 1 0x11818 0x14c84 0 0xffffffff 41,write 0 0x1fec 0xa00c4 0 0 45]
    beforeWaits = if earlyGate then [(set "instructions" (number 50) gate,initialization),(tailStop,[])] else [(tailStop,initialization),(gate,[])]
    waitSegments = [(firstWait,[write 1 0x11f4c 0x20004 0 0x22b 80,waitRead 100])]
      ++ (if selected == DelayedRelease then [(ack,[cpu "cpu-read" 0 0x114 0x40020004 0x20004 0 0 104])] else [])
      ++ [(secondWait,[waitRead 110]),(thirdWait,[waitRead 120]),(fourthWait,[waitRead 130])]
    segments = prefix ++ beforeWaits ++ waitSegments
    (heldRows,heldStops) = assemble [] [] segments
    (preclearRows,preclearStops) = assemble heldRows heldStops
      [(set "instructions" (number (150 + delta)) preclear,[waitRead (200 + 610 * fromIntegral index) | index <- [0 .. extraReads - 1]])]
    committedRows = preclearRows ++ [hostRead 0x20000,write 0 0x1ff8 0x20000 0 0x22b (152 + delta),hostRead 0x20000] ++ observationReads
    (delayedRows,delayedStops) = assemble committedRows preclearStops
      [(set "instructions" (number (180 + delta)) handoff,[write 1 0x11f98 0x14c80 0xd0020 0 (170 + delta),write 1 0x11fb0 0x14c84 0x69cdf8 0 (175 + delta)])
      ,(set "instructions" (number (190 + delta)) loader,[])]
    clearObservation = set "instructions" (number (152 + delta))
      (set "microsteps" (values [microstep [75 + delta `div` 2,75 + delta `div` 2] [75 + delta `div` 2,76 + delta `div` 2] 0x22b
        ,microstep [75 + delta `div` 2,76 + delta `div` 2] [76 + delta `div` 2,76 + delta `div` 2] 0]) (readback (length committedRows)))
    (rows,snapshots,committed) = if selected == HeldSentinel then (heldRows,heldStops,Null) else (delayedRows,delayedStops,clearObservation)
    assemble accumulated snapshots [] = (accumulated,snapshots)
    assemble accumulated snapshots ((stop,attempts):remaining) =
      let next = accumulated ++ attempts ++ observationReads
      in assemble next (snapshots ++ [set "watch_records" (number (fromIntegral (length next))) stop]) remaining

parse :: Value -> Either String ControlReceipt
parse = parseControlReceipt . BL.toStrict . encode
accept :: PeerControl -> Value -> Either String ()
accept selected value = parse value >>= validateControlReceipt selected expected
assertRight :: String -> Either String a -> IO a
assertRight label result = either (error . ((label ++ ": ") ++)) pure result
assertLeft :: String -> Either String a -> IO ()
assertLeft label result = case result of Left _ -> pure (); Right _ -> error (label ++ ": accepted")

shiftTimes :: Word64 -> Word64 -> Value -> Value
shiftTimes threshold delta = walk
  where
    walk (Object row) = Object (KM.mapWithKey (\key value -> if key == "instructions" || key == "tick" then shift value else walk value) row)
    walk (Array entries) = Array (V.map walk entries)
    walk value = value
    shift value = case fromJSON value :: Result Word64 of
      Success time | time >= threshold -> number (time + delta)
      _ -> value

shiftMicrosteps :: Word64 -> Value -> Value
shiftMicrosteps delta = change "clear_readback" (change "microsteps" (\value -> case value of
  Array entries -> Array (V.map (change "before" shift . change "after" shift) entries)
  other -> other))
  where
    shift (Array entries) = Array (V.map (\value -> case fromJSON value :: Result Word64 of
      Success time -> number (time + delta `div` 2)
      Error message -> error message) entries)
    shift value = value

main :: IO ()
main = do
  args <- getArgs
  case args of
    [] -> consumerControls
    ["--emit-consumer-control",selected] -> case parseControlName selected of
      Right control -> BL.putStr (encode (fixture control False))
      Left problem -> error problem
    _ -> error "expected --emit-consumer-control held-sentinel|delayed-release"

consumerControls :: IO ()
consumerControls = do
  forM_ [HeldSentinel,DelayedRelease] $ \selected -> do
    forM_ [False,True] $ \early -> do
      receipt <- assertRight "serialized positive consumer control" (parse (fixture selected early))
      _ <- assertRight "partial order consumer control" (validateControlReceipt selected expected receipt)
      roundtrip <- assertRight "serialized control roundtrip" (parse (controlReceiptValue receipt))
      unless (receipt == roundtrip) (error "control receipt roundtrip changed observations")
      let earlier = recorded 0x11f74 "cpu-read" (change "instructions" (\value -> case value of Number time -> Number (time - 1); other -> other)) (fixture selected early)
      assertRight "load count before stop consumer control" (accept selected earlier)
    _ <- assertRight "closed contract roundtrip" (parseControlContract (BL.toStrict (encode (controlContractValue selected))))
    let valid = fixture selected False
        controls =
          [ ("wrong core",stopped "record-2-entry" (set "core" (number 0)))
          , ("wrong wait core",stopped "peer-wait-loaded" (set "core" (number 0)))
          , ("wrong site",stopped "peer-wait-loaded" (set "address" (number 0x11f74)))
          , ("missing repeated loads",change "stops" (\value -> case value of Array entries -> Array (V.filter (not . has "instructions" (number 110)) entries); other -> other))
          , ("wrong loaded register",stopped "peer-wait-loaded" (change "registers" (at 2 (const (number 0)))))
          , ("wrong wait readback",stopped "peer-wait-loaded" (change "state" (set "cell0" (number 0))))
          , ("wrong gate SP",stopped "primary-gate" (set "sp" (number 0x70000800)))
          , ("wrong gate mode",stopped "primary-gate" (set "cpsr" (number 0x13)))
          , ("wrong gate input",stopped "primary-gate" (change "registers" (at 3 (const (number 1)))))
          , ("missing saved frame",stopped "record-2-rendezvous-tail" (set "frame" Null))
          , ("wrong saved return",stopped "record-2-rendezvous-tail" (change "frame" (at 20 (const (number 0)))))
          , ("cursor outside rows",stopped "peer-wait-loaded" (set "watch_records" (number 16384)))
          , ("cursor omits attempts",stopped "peer-wait-loaded" (set "watch_records" (number 0)))
          , ("wrong wait alias",recorded 0x11f74 "cpu-read" (set "address" (number 0x20000)))
          , ("wrong physical alias",recorded 0x11f74 "cpu-read" (set "physical" (number 0x20004)))
          , ("wrong wait word",recorded 0x11f74 "cpu-read" (set "value" (number 0)))
          , ("wrong wait before bytes",recorded 0x11f74 "cpu-read" (set "before" (values (replicate 4 (number 0)))))
          , ("coherent wrong loaded word",recorded 0x11f74 "cpu-read" (set "value" (number 0) . set "before" (values (replicate 4 (number 0)))))
          , ("unexplained read site",recorded 0x11f74 "cpu-read" (set "pc" (number 0x11f70)))
          , ("load count after stop",recorded 0x11f74 "cpu-read" (change "instructions" (\value -> case value of Number time -> Number (time + 1); other -> other)))
          , ("unexpected early handoff",stopped "peer-wait-loaded" (set "label" (String "record-2-handoff")))
          , ("wrong wait source core",recorded 0x11f74 "cpu-read" (set "core" (number 0)))
          , ("wrong sentinel writer",recorded 0x1e64 "cpu-write" (set "core" (number 1)))
          , ("wrong peer-clear source",recorded 0x11f4c "cpu-write" (set "pc" (number 0x11f48)))
          , ("wrong clear preimage",recorded 0x11f4c "cpu-write" (set "before" (values (replicate 4 (number 0)))))
          , ("host writer",change "memory_watch" (change "records" (at 0 (const (set "source" (String "host-write") (hostRead 0x20000))))))
          , ("missing physical readback",change "memory_watch" (change "records" (filterValues (not . has "source" (String "host-read")))))
          , ("model-only peer clear",recorded 0x11f4c "cpu-write" (const (object ["source" .= ("model-bus-write" :: String),"address" .= (0x40020004 :: Word32),"physical" .= (0x20004 :: Word32),"size" .= (4 :: Int),"core" .= Null,"pc" .= Null,"instructions" .= Null,"tick" .= Null])))
          , ("watch disabled",change "memory_watch" (set "enabled" (Bool False)))
          , ("watch violation",change "memory_watch" (set "violation" (String "synthetic overflow")))
          , ("aggregate overflow",set "instructions" (number 10000001))
          , ("prefix overflow",stopped "primary-gate" (set "instructions" (number 4000001)))
          , ("missing bindings",set "bindings" Null)
          , ("wrong gate binding count",change "bindings" (set "instructions" (number 71)))
          , ("wrong decoder choice",change "bindings" (set "decoder_choices" (values [number 0])))
          , ("wrong RAM backing",change "bindings" (change "low_ram" (set "physical" (number 0x40000000))))
          , ("wrong TCM",change "bindings" (set "atcm" (number 0x70000001)))
          , ("wrong stack extent",change "bindings" (set "stack" (values [])))
          , ("wrong original word",change "bindings" (change "code" (at 1 (change "virtual" (at 0 (const (number 1)))))))
          , ("wrong patched physical word",change "bindings" (change "code" (at 1 (change "physical" (at 20 (const (number 0)))))))
          , ("wrong veneer word",change "bindings" (change "code" (at 0 (change "virtual" (at 0 (const (number 0)))))))
          , ("wrong code extent",change "bindings" (change "code" (at 0 (set "address" (number 0)))))
          , ("unknown top field",set "extra" Null)
          , ("unknown control",set "control" (String "other"))
          , ("schema1 receipt",set "schema" (number 1))
          , ("machine failure",set "machine_failure" (String "synthetic failure"))
          ]
    forM_ controls $ \(label,mutate) -> assertLeft (label ++ " consumer control") (accept selected (mutate valid))
    assertLeft "schema1 consumer rejects schema2 control" (Natural.parseReceipt (BL.toStrict (encode valid)))
    assertLeft "wrong selected control consumer control" (accept (if selected == HeldSentinel then DelayedRelease else HeldSentinel) valid)
    assertLeft "wrong argument consumer control" (parse valid >>= validateControlReceipt selected (Argument 1))
  let delayed = fixture DelayedRelease False
      readbackChange = change "clear_readback"
      stepChange = readbackChange . change "microsteps"
      delayedControls =
        [ ("wrong handoff core",stopped "record-2-handoff" (set "core" (number 0)))
        , ("wrong handoff address",stopped "record-2-handoff" (set "address" (number 0x40020)))
        , ("wrong handoff target",stopped "record-2-handoff" (change "registers" (at 3 (const (number 1)))))
        , ("wrong handoff argument",stopped "loader-b-entry" (change "state" (set "argument" (number 1))))
        , ("unrestored peer frame",stopped "record-2-handoff" (change "registers" (at 4 (const (number 0)))))
        , ("unrestored primary register",stopped "primary-clear" (change "registers" (at 12 (const (number 0)))))
        , ("unrestored primary LR",stopped "primary-clear" (set "lr" (number 0)))
        , ("unrestored primary flags",stopped "primary-clear" (set "cpsr" (number 0xd3)))
        , ("wrong clear core",recorded 0x1ff8 "cpu-write" (set "core" (number 1)))
        , ("wrong clear PC",recorded 0x1ff8 "cpu-write" (set "pc" (number 0x1ff4)))
        , ("early clear",stopped "primary-clear" (set "instructions" (number 125)))
        , ("handoff before clear",stopped "record-2-handoff" (set "instructions" (number 140)))
        , ("attempt without readback",readbackChange (const Null))
        , ("uncleared readback",readbackChange (change "state" (set "cell0" (number 0x22b))))
        , ("wrong post-store PC",readbackChange (set "primary_address" (number 0x1ff8)))
        , ("wrong post-store CPSR",readbackChange (set "primary_cpsr" (number 0xd3)))
        , ("wrong original instruction",readbackChange (set "primary_instruction" (values (replicate 4 (number 0)))))
        , ("readback cursor before store",readbackChange (set "watch_records" (number 0)))
        , ("zero microsteps",stepChange (const (values [])))
        , ("microstep limit",stepChange (\value -> case value of Array entries -> Array (V.replicate 2049 (entries V.! 0)); other -> other))
        , ("zero aggregate progress",stepChange (at 0 (set "after" (values [number 75,number 75]))))
        , ("two aggregate progress",stepChange (at 0 (set "after" (values [number 76,number 76]))))
        , ("core counter decrease",stepChange (at 0 (set "after" (values [number 74,number 77]))))
        , ("noncontiguous counts",stepChange (at 1 (set "before" (values [number 74,number 77]))))
        , ("early scalar clear",stepChange (at 0 (set "cell0" (number 0))))
        , ("final scalar held",stepChange (at 1 (set "cell0" (number 0x22b))))
        , ("final core wrong",stepChange (at 1 (set "after" (values [number 75,number 77]))))
        , ("wrong microstep start",stepChange (at 0 (set "before" (values [number 74,number 75]))))
        , ("wrong microstep end",readbackChange (set "instructions" (number 153)))
        , ("clear attempt in earlier window",recorded 0x1ff8 "cpu-write" (set "instructions" (number 151)))
        , ("missing original zero poll",recorded 0x114 "cpu-read" (set "value" (number 0x22b) . set "before" (values (map (number . fromIntegral) (bytes 0x22b)))))
        ]
  forM_ delayedControls $ \(label,mutate) -> assertLeft (label ++ " consumer control") (accept DelayedRelease (mutate delayed))
  forM_ ["{\"schema\":2,\"schema\":2}","{\"schema\":2e0}"] $ \bad -> assertLeft "strict JSON consumer control" (parseControlReceipt bad)
  assertLeft "receipt byte bound consumer control" (parseControlReceipt (B.replicate (maxReceiptBytes + 1) 32))
  forM_ [set "budget" (number 10000001),set "schema" (number 1),set "extra" Null] $ \mutate ->
    assertLeft "closed contract consumer control" (parseControlContract (BL.toStrict (encode (mutate (controlContractValue HeldSentinel)))))
  forM_ [(3999930,True),(3999931,False)] $ \(delta,valid) ->
    let outcome = accept HeldSentinel (shiftTimes 0 delta (fixture HeldSentinel False))
    in if valid then assertRight "prefix maximum consumer control" outcome else assertLeft "prefix overflow consumer control" outcome
  forM_ [(3999940,True),(3999941,False)] $ \(delta,valid) ->
    let outcome = accept HeldSentinel (shiftTimes 80 delta (fixture HeldSentinel False))
    in if valid then assertRight "held phase maximum consumer control" outcome else assertLeft "held phase overflow consumer control" outcome
  forM_ [(5099920,True),(5099922,False)] $ \(delta,valid) ->
    let outcome = accept DelayedRelease (shiftMicrosteps delta (shiftTimes 80 delta delayed))
    in if valid then assertRight "delayed phase maximum consumer control" outcome else assertLeft "delayed phase overflow consumer control" outcome
  forM_ [(19962,True),(19963,False)] $ \(delta,valid) ->
    let outcome = accept DelayedRelease (shiftTimes 170 delta delayed)
    in if valid then assertRight "clear-to-loader maximum consumer control" outcome else assertLeft "clear-to-loader overflow consumer control" outcome
  _ <- assertRight "wait sample maximum consumer control" (accept DelayedRelease (fixtureWithReads DelayedRelease False 124))
  assertLeft "wait sample overflow consumer control" (accept DelayedRelease (fixtureWithReads DelayedRelease False 125))
  forM_ [set "final_primary" Null,change "final_primary" (set "address" (number 0x1ff8)),change "final_primary" (change "registers" (at 0 (const (number 0)))),change "final_primary" (set "cpsr" (number 0xd3))] $ \mutate ->
    assertLeft "held final context consumer control" (accept HeldSentinel (mutate (fixture HeldSentinel False)))
  let padded total = change "memory_watch" (change "records" (\value -> case value of Array entries -> Array (entries V.++ V.replicate (total - V.length entries) (hostRead 0x20000)); other -> other)) delayed
  _ <- assertRight "successful watch row maximum consumer control" (accept DelayedRelease (padded 12288))
  assertLeft "successful watch row bound consumer control" (accept DelayedRelease (padded 12289))
  assertLeft "collector row bound consumer control" (parse (padded 16385))
  putStrLn "peer-wait witness serialized consumer controls: pass"
