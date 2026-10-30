{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Vector as V
import Data.Word (Word32, Word64, Word8)
import Emulation.PeerStageWitness
import qualified Emulation.WatchObservation as W

expected :: Argument
expected = Argument 0x69cdf8

savedFrame :: [Word8]
savedFrame = [0x44,0,0,0,0x55,0,0,0,0x66,0,0,0,0x77,0,0,0,0x0b,0,0,0,0x3c,0x18,1,0]

state :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Value
state first second done dest arg = object
  ["cell0" .= first,"cell1" .= second,"completion" .= done,"target" .= dest,"argument" .= arg]

stop :: String -> Word32 -> Word64 -> [Word32] -> Word32 -> Value -> Value -> Value
stop name pc instructions regs sp snapshot frame = object
  [ "label" .= name, "address" .= pc, "core" .= (1 :: Int), "instructions" .= instructions
  , "registers" .= regs, "sp" .= sp, "lr" .= (0 :: Word32), "cpsr" .= (0xd3 :: Word32)
  , "state" .= snapshot, "frame" .= frame
  ]

stops :: [Value]
stops =
  [ stop "record-2-entry" 0x11800 10 (replicate 13 0) 0
      (state 0x1234 0x5678 0x9999 0xdeadbeef 0xfedcba98) Null
  , stop "record-2-rendezvous-tail" 0x11f40 100 [0,0,0,0,0,0,1,0,0,0,0,0x7000103c,0] 0x70001000
      (state 0x22b 0x22b 0 0 0) (toJSONBytes savedFrame)
  , stop "record-2-handoff" 0x11850 200 restored 0x70001040
      (state 0 0 0 0xd0020 0x69cdf8) Null
  , stop "loader-b-entry" 0xd0020 210 restored 0x70001040
      (state 0 0 0x98765432 0xd0020 0x69cdf8) Null
  ]
  where restored = [0x69cdf8,0,0,0xd0020,0x44,0x55,0x66,0x77,0,0,0,0xb,0]

toJSONBytes :: [Word8] -> Value
toJSONBytes bytes = Array (V.fromList (map (Number . fromIntegral) bytes))

cpuWrite :: Int -> Word32 -> Word32 -> Word32 -> Word64 -> Word64 -> Value
cpuWrite cpu pc guest physical value instructions = object
  [ "source" .= ("cpu-write" :: String), "core" .= cpu, "pc" .= pc
  , "address" .= guest, "physical" .= physical, "size" .= (4 :: Int), "value" .= value
  , "before" .= ([0xde,0xad,0xbe,0xef] :: [Word8]), "byte_order" .= ("little" :: String)
  , "instructions" .= instructions, "tick" .= instructions
  ]

writes :: [Value]
writes =
  [ cpuWrite 0 0x1e64 0x40020000 0x20000 0x22b 20
  , cpuWrite 0 0x1e68 0x40020004 0x20004 0x22b 21
  , cpuWrite 0 0x1ff8 0x40020000 0x20000 0 110
  , cpuWrite 1 0x11f4c 0x40020004 0x20004 0 120
  , cpuWrite 1 0x11f98 0x14c80 0x14c80 0xd0020 150
  , cpuWrite 1 0x11fb0 0x14c84 0x14c84 0x69cdf8 160
  ]

device :: Value
device = object
  [ "source" .= ("model-bus-write" :: String), "address" .= (0x400a00c4 :: Word32)
  , "physical" .= (0xa00c4 :: Word32), "size" .= (4 :: Int)
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null
  ]

valid :: Value
valid = object
  [ "schema" .= (1 :: Int), "result" .= ("passed" :: String), "error" .= Null, "machine_failure" .= Null
  , "instructions" .= (210 :: Word64), "cp14_identities" .= ([0x4003,0x5003] :: [Word32]), "stops" .= stops
  , "memory_watch" .= object
      [ "enabled" .= True, "ranges" .= ([[0x14c80,0x14c88],[0x20000,0x20008],[0xa00c4,0xa00c8]] :: [[Word32]])
      , "records" .= (device:writes), "violation" .= Null
      ]
  ]

set :: String -> Value -> Value -> Value
set name replacement (Object fields) = Object (KM.insert (K.fromString name) replacement fields)
set _ _ value = value

change :: String -> (Value -> Value) -> Value -> Value
change name f original@(Object fields) = case KM.lookup (K.fromString name) fields of
  Just value -> Object (KM.insert (K.fromString name) (f value) fields)
  Nothing -> original
change _ _ value = value

at :: Int -> (Value -> Value) -> Value -> Value
at index f (Array entries) = Array (entries V.// [(index,f (entries V.! index))])
at _ _ value = value

stopAt :: Int -> (Value -> Value) -> Value -> Value
stopAt index = change "stops" . at index

writeAt :: Int -> (Value -> Value) -> Value -> Value
writeAt index = change "memory_watch" . change "records" . at (index + 1)

number :: Word64 -> Value
number = Number . fromIntegral

parse :: Value -> Either String Receipt
parse = parseReceipt . BL.toStrict . encode

accept :: Value -> Either String ()
accept value = parse value >>= validateReceipt expected

assertRight :: String -> Either String a -> IO a
assertRight label result = case result of
  Right value -> pure value
  Left problem -> error (label ++ ": " ++ problem)

assertLeft :: String -> Either String a -> IO ()
assertLeft label result = case result of
  Left _ -> pure ()
  Right _ -> error (label ++ ": accepted")

main :: IO ()
main = do
  receipt <- assertRight "positive serialized receipt" (parse valid)
  _ <- assertRight "positive acceptance" (validateReceipt expected receipt)
  roundtrip <- assertRight "typed serialized roundtrip" (parse (receiptValue receipt))
  unless (receipt == roundtrip) (error "roundtrip changed attempted-event metadata")
  case watchRecords (receiptWatch roundtrip) of
    TransferRecord ModelBusWrite _ _ _ : attempt@CpuRecord {cpuAccess = CpuWrite, cpuBefore = Just before, cpuValue = Just 0x22b, cpuCore = W.Core0, cpuInstructions = Instructions 20, cpuTick = Tick 20} : _ ->
      unless (cpuPc attempt == W.watchAddress 0x1e64 && before == B.pack [0xde,0xad,0xbe,0xef]) (error "event origin or before bytes lost")
    _ -> error "CPU attempt and device transfer distinction lost"
  _ <- assertRight "decimal argument" (parseArgument "6934008")
  unless (parseArgument "0x0069cdf8" == Right expected) (error "hex argument differs")
  forM_ ["-1","4294967296","0x100000000","1.0","0x","1garbage"] $ \word -> assertLeft "invalid argument" (parseArgument word)
  let reversedClears = device : take 2 writes
        ++ [cpuWrite 1 0x11f4c 0x40020004 0x20004 0 110, cpuWrite 0 0x1ff8 0x40020000 0x20000 0 120]
        ++ drop 4 writes
  _ <- assertRight "independent clear order" (accept (change "memory_watch" (set "records" (Array (V.fromList reversedClears))) valid))
  let cpuRead = set "source" (String "cpu-read") . set "value" (number 0) . set "before" (toJSONBytes [0,0,0,0]) $ cpuWrite 1 0x11f74 0x40020000 0x20000 0 130
      cpuFetch = set "source" (String "cpu-fetch") . set "value" Null . set "before" Null $ cpuWrite 1 0x14c80 0x14c80 0x14c80 0 140
      withReads = change "memory_watch" (set "records" (Array (V.fromList (device : take 4 writes ++ [cpuRead,cpuFetch] ++ drop 4 writes)))) valid
  _ <- assertRight "read and fetch attempts retained" (accept withReads)
  let controls =
        [ ("wrong-core consumer control", stopAt 0 (set "core" (number 0)))
        , ("wrong-handoff consumer control", stopAt 2 (set "address" (number 0x40020)))
        , ("wrong loader core", stopAt 3 (set "core" (number 0)))
        , ("wrong handoff argument", stopAt 2 (change "registers" (at 0 (const (number 1)))))
        , ("wrong handoff register target", stopAt 2 (change "registers" (at 3 (const (number 0x40020)))))
        , ("wrong published target readback", stopAt 2 (change "state" (set "target" (number 0x40020))))
        , ("wrong published argument readback", stopAt 3 (change "state" (set "argument" (number 1))))
        , ("attempts cannot replace cell readback", stopAt 2 (change "state" (set "cell1" (number 0x22b))))
        , ("wrong frame relation", stopAt 1 (change "registers" (at 11 (const (number 0x70001038)))))
        , ("wrong discriminator", stopAt 1 (change "registers" (at 6 (const (number 0)))))
        , ("wrong saved return", stopAt 1 (change "frame" (at 20 (const (number 0)))))
        , ("wrong saved register", stopAt 2 (change "registers" (at 4 (const (number 0)))))
        , ("unrestored stack", stopAt 2 (set "sp" (number 0x70001000)))
        , ("loader stack differs", stopAt 3 (set "sp" (number 0x70001044)))
        , ("missing saved frame", stopAt 1 (set "frame" Null))
        , ("short saved frame", stopAt 1 (set "frame" (toJSONBytes (take 16 savedFrame))))
        , ("frame at entry", stopAt 0 (set "frame" (toJSONBytes savedFrame)))
        , ("short register bank", stopAt 0 (set "registers" (Array V.empty)))
        , ("wrong stop chronology", stopAt 2 (set "instructions" (number 100)))
        , ("wrong identity", set "cp14_identities" (Array (V.fromList [number 0x4003,number 0x4003])))
        , ("failed receipt", set "result" (String "failed"))
        , ("machine failure", set "machine_failure" (String "synthetic failure"))
        , ("budget exceeded", set "instructions" (number 20000001))
        , ("watch overflow violation", change "memory_watch" (set "violation" (String "watch record limit")))
        , ("watch disabled", change "memory_watch" (set "enabled" (Bool False)))
        , ("wrong ranges", change "memory_watch" (set "ranges" (Array V.empty)))
        , ("wrong clear core", writeAt 3 (set "core" (number 0)))
        , ("wrong sentinel core", writeAt 0 (set "core" (number 1)))
        , ("wrong publication writer", writeAt 4 (set "pc" (number 0x11f94)))
        , ("wrong publication value", writeAt 5 (set "value" (number 0)))
        , ("wrong physical cell", writeAt 3 (set "physical" (number 0x20000)))
        , ("missing peer alias", writeAt 3 (set "address" (number 0x20004)))
        , ("wrong alias", writeAt 0 (set "address" (number 0x50020000)))
        , ("wrong byte order", writeAt 3 (set "byte_order" (String "big")))
        , ("wrong attempt width", writeAt 3 (set "size" (number 2)))
        , ("attempt lacks before bytes", writeAt 3 (set "before" Null))
        , ("attempt exceeds width", writeAt 3 (set "value" (number 0x100000000)))
        , ("attempt after final stop", writeAt 5 (set "instructions" (number 211)))
        , ("clear before tail", writeAt 3 (set "instructions" (number 99)))
        , ("unknown CPU origin", writeAt 3 (set "source" (String "cpu-vendor")))
        , ("device cannot impersonate CPU clear", writeAt 3 (const device))
        , ("device cannot carry core", change "memory_watch" (change "records" (at 0 (set "core" (number 1)))))
        , ("device cannot carry CPU value", change "memory_watch" (change "records" (at 0 (set "value" (number 0)))))
        , ("byte overflow", writeAt 0 (set "before" (Array (V.fromList [number 256,number 0,number 0,number 0]))))
        , ("negative address", stopAt 0 (set "address" (Number (-1))))
        , ("address overflow", stopAt 0 (set "address" (number 0x100000000)))
        , ("unknown field", set "extra" Null)
        ]
  forM_ controls $ \(label,mutate) -> assertLeft label (accept (mutate valid))
  forM_ [0x13,0xf3,0x2d3,0x010000d3] $ \cpsr -> assertLeft "handoff CPSR" (accept (stopAt 2 (set "cpsr" (number cpsr)) valid))
  _ <- assertRight "condition flags are unconstrained" (accept (stopAt 2 (set "cpsr" (number 0xf00000d3)) valid))
  assertLeft "read scalar/before disagreement" (accept (change "memory_watch" (set "records" (Array (V.fromList (device : take 4 writes ++ [set "value" (number 1) cpuRead] ++ drop 4 writes)))) valid))
  assertLeft "row overflow" (accept (change "memory_watch" (set "records" (Array (V.replicate 16385 device))) valid))
  assertLeft "receipt size overflow" (parseReceipt (B.replicate (maxReceiptBytes + 1) 32))
  assertLeft "duplicate JSON key" (parseReceipt "{\"schema\":1,\"schema\":1}")
  assertLeft "noninteger JSON token" (parseReceipt "{\"schema\":1e0}")
  putStrLn "peer-stage witness consumer tests: pass"
