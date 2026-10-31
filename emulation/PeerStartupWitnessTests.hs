{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (sortOn)
import qualified Data.Vector as V
import Data.Word (Word32, Word64, Word8)
import Emulation.FirstStageWitness (GuestAddress (..), encodeLeWord, leWordAt)
import Emulation.PeerStartupWitness
import qualified FujiAF.FirstStage as F
import Numeric (showHex)
import System.Environment (getArgs)

data Candidate = Candidate B.ByteString B.ByteString

argumentWord :: Word32
argumentWord = 0x69cdf8

assertRight :: String -> Either String a -> IO a
assertRight label result = case result of
  Right value -> pure value
  Left problem -> error (label ++ ": " ++ problem)

assertLeft :: String -> Either String a -> IO ()
assertLeft label result = case result of
  Left _ -> pure ()
  Right _ -> error (label ++ ": accepted")

addressOf :: GuestAddress -> Word32
addressOf (GuestAddress value) = fromIntegral value

symbolAddress :: B.ByteString -> String -> IO Word32
symbolAddress symbols name = assertRight ("symbol " ++ name) (F.symbolAddress <$> (F.parseSymbols symbols >>= (`F.lookupSymbol` name)))

-- | Guest record bytes with the reset vector naming the entry and the image at it.
stagedFor :: Candidate -> Either String StagedRecord
stagedFor (Candidate bin symbols) = do
  GuestAddress entry <- candidateEntry symbols
  let load = fromIntegral entry - 0x10000
  stagedRecord (B.concat
    [ B.replicate 0x20 0, encodeLeWord (fromIntegral entry), B.replicate (load - 0x24) 0, bin
    , B.replicate (0x4c80 - load - B.length bin) 0 ])

derive :: Candidate -> Either String OwnedFacts
derive candidate@(Candidate bin symbols) = stagedFor candidate >>= deriveOwnedFacts bin symbols

replaceWord :: Int -> Word32 -> B.ByteString -> B.ByteString
replaceWord offset word bytes = B.concat [B.take offset bytes, encodeLeWord word, B.drop (offset + 4) bytes]

-- * JSON editing

set :: String -> Value -> Value -> Value
set name replacement (Object entries) = Object (KM.insert (K.fromString name) replacement entries)
set _ _ value = value

change :: String -> (Value -> Value) -> Value -> Value
change name f original@(Object entries) = case KM.lookup (K.fromString name) entries of
  Just value -> Object (KM.insert (K.fromString name) (f value) entries)
  Nothing -> original
change _ _ value = value

at :: Int -> (Value -> Value) -> Value -> Value
at index f (Array entries) = Array (entries V.// [(index, f (entries V.! index))])
at _ _ value = value

each :: (Value -> Value) -> Value -> Value
each f (Array entries) = Array (V.map f entries)
each _ value = value

vector :: (V.Vector Value -> V.Vector Value) -> Value -> Value
vector f (Array entries) = Array (f entries)
vector _ value = value

number :: Word64 -> Value
number = Number . fromIntegral

-- | Applies to the n-th stop (from 0) carrying the label.
labelled :: String -> Int -> (Value -> Value) -> Value -> Value
labelled name wanted f = change "stops" relabel
  where
    relabel (Array entries) = Array (V.fromList (go 0 (V.toList entries)))
    relabel value = value
    go _ [] = []
    go seen (stop : rest)
      | isLabel stop = (if seen == wanted then f stop else stop) : go (seen + 1 :: Int) rest
      | otherwise = stop : go seen rest
    isLabel (Object entries) = KM.lookup "label" entries == Just (String (fromStringValue name))
    isLabel _ = False
    fromStringValue = K.toText . K.fromString

insertBeforeLast :: Value -> Value -> Value
insertBeforeLast item (Array entries) = let (front, back) = V.splitAt (V.length entries - 1) entries in Array (front <> V.singleton item <> back)
insertBeforeLast _ value = value

lastStop :: Value -> Value
lastStop (Object entries) = case KM.lookup "stops" entries of
  Just (Array stops) | not (V.null stops) -> V.last stops
  _ -> Null
lastStop _ = Null

-- * Synthetic receipts

data Knobs = Knobs
  { knobSkew :: Word64
  , knobExtraRows :: [(Word64, Value)]
  , knobCompanion :: B.ByteString
  -- | (threshold, delta): every stop and row at or after the threshold moves
  -- later by delta core-0 instructions.
  , knobShifts :: [(Word64, Word64)]
  , knobPollValue :: Word32
  -- | Core-1 ATCM register at held-release and startup-entry.
  , knobHeldAtcm1 :: Word32
  -- | Segment table guest root and the physical address its core-1 mapping gives.
  , knobTable :: (Word32, Word32)
  -- | Whether core 1 reads the id-1 record's id and header words through the table mapping.
  , knobTableReads :: Bool
  }

-- | Core-1 ATCM 0xffff8011 at held-release and the segment root 0x4061c040 at physical 0x61c040 are the values the retained natural reset showed.
plain :: Knobs
plain = Knobs 0 [] (B.replicate 0x548 0x5a) [] 0 0xffff8011 (0x4061c040, 0x61c040) True

hexText :: B.ByteString -> String
hexText = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

leBytes :: Word32 -> [Word8]
leBytes = B.unpack . encodeLeWord

cpuRow :: String -> Int -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word64 -> Value
cpuRow source cpu pc guest physical value before time = object
  [ "source" .= source, "core" .= cpu, "pc" .= pc, "address" .= guest, "physical" .= physical
  , "size" .= (4 :: Int), "value" .= value, "before" .= leBytes before, "byte_order" .= ("little" :: String)
  , "instructions" .= time, "tick" .= time ]

transferRow :: String -> Word32 -> Value
transferRow source physical = sizedTransferRow source physical 4

sizedTransferRow :: String -> Word32 -> Word32 -> Value
sizedTransferRow source physical size = object
  [ "source" .= source, "address" .= physical, "physical" .= physical, "size" .= size
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null ]

-- | A model-bus write with the bus-alias address the classified watch records.
busWrite :: Word32 -> Word32 -> Value
busWrite physical size = object
  [ "source" .= ("model-bus-write" :: String), "address" .= (physical + 0x40000000), "physical" .= physical, "size" .= size
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null ]

-- | The two 0x800-byte boot transfers the retained natural reset recorded in the reset epoch.
bootBulkWrites :: [Value]
bootBulkWrites = [busWrite 0x14800 0x800, busWrite 0x20000 0x800]

peerState :: Word32 -> Word32 -> Word32 -> Word32 -> Value
peerState first second dest arg = object ["cell0" .= first, "cell1" .= second, "completion" .= (0 :: Int), "target" .= dest, "argument" .= arg]

mergeRanges :: [(Word64, Word64)] -> [(Word64, Word64)]
mergeRanges = reverse . foldl step [] . sortOn fst
  where
    step ((low, high) : merged) (nextLow, nextHigh) | nextLow <= high = (low, max high nextHigh) : merged
    step merged next = next : merged

intervalPair :: Interval -> (Word64, Word64)
intervalPair (Interval low high) = (low, high)

retainedSiteWords :: [(Word32, Word32)]
retainedSiteWords =
  [ (0x11800, 0xeb000013), (0x11804, 0xeb000000), (0x11f74, 0xe5913000), (0x11f4c, 0xe5810000), (0x11f98, 0xe5810000)
  , (0x11fb0, 0xe5810000), (0x11844, 0xe5933000), (0x1184c, 0xe5900000), (0x11850, 0xe1a0f003) ]

ownedSiteWords :: B.ByteString -> OwnedFacts -> Either String [(Word32, Word32)]
ownedSiteWords bin facts = mapM (\location -> (,) location <$> leWordAt bin (fromIntegral (location - entry)))
  [ entry, entry + 4, entry + 8, site sitePeerLoad, site sitePeerLoaded - 4, site sitePeerLoad + 4, site siteClearStore, site siteTargetStore
  , site siteArgumentStore, site siteTargetLoad, site siteArgumentLoad, site siteHandoff ]
  where
    sites = ownedSites facts
    site select = addressOf (select sites)
    entry = site siteEntry

heldGateWords, delayedGateWords, veneerWords :: [Word32]
heldGateWords = [0xe3003000, 0xe344300a, 0xe3a02000, 0xe58320c4, 0xe3003000, 0xeafffffe, 0xe5832000, 0xf57ff04f]
delayedGateWords = [0xe3003000, 0xe344300a, 0xe3a02000, 0xe58320c4, 0xe3003000, 0xeafff841, 0xe5832000, 0xf57ff04f]
veneerWords =
  [ 3912060943, 3775856640, 3808563200, 3812896770, 3808477739, 3851624452, 3813801984, 167772170
  , 3780247564, 436207621, 3818922084, 3810586624, 3797032961, 452984828, 3796963329, 452984820
  , 3777556480, 3904720911, 3942645758, 3808497664, 3796963329, 452984829, 3777556480, 3904720911
  , 3812896770, 3925870499 ]

-- ** Synthetic timeline, in aggregate instructions (core-0 count + core-1 count)

-- | Counts at the stop, value read at the stop, times of earlier waiting reads without a stop.
data Load = Load (Word64, Word64) Word32 [Word64]

countsTime :: (Word64, Word64) -> Word64
countsTime (first, second) = first + second

gateCounts, ackCounts :: (Word64, Word64)
gateCounts = (1050, 20)
ackCounts = (1105, 100)

gateTime :: Word64
gateTime = countsTime gateCounts

uniformCounts :: Word64 -> Word64 -> Int -> (Word64, Word64)
uniformCounts period skew index = (1100 + 100 * fromIntegral index, 100 + fromIntegral index * period + (if index >= 1 then skew else 0))

-- | After the fourth waiting stop; core 0 runs 3 * period further so the scheduler bound holds for any period.
preclearCounts :: Word64 -> (Word64, Word64)
preclearCounts period = (1450 + 3 * period, 110 + 3 * period)

preclearTime, readbackTime :: Word64 -> Word64
preclearTime period = countsTime (preclearCounts period)
readbackTime period = preclearTime period + 2

loadsFor :: Scenario -> Word64 -> Word64 -> [Load]
loadsFor scenario period skew = case scenario of
  HeldSentinel -> [Load (uniformCounts period skew index) 0x22b [] | index <- [0 .. 3]]
  DelayedRelease _ -> [Load (uniformCounts period skew index) 0x22b [] | index <- [0 .. 3]]
    ++ [Load (1500 + 3 * period, 100 + 4 * period) 0 [preclearTime period - 20]]
  Natural _ -> [Load (uniformCounts period skew index) (if index == 2 then 0 else 0x22b) [] | index <- [0 .. 2]]

exitCountsFor :: Scenario -> Word64 -> (Word64, Word64)
exitCountsFor scenario period = foldl (\_ (Load counts _ _) -> counts) (0, 0) (loadsFor scenario period 0)

heldFinalTime :: Word64 -> Word64
heldFinalTime period = countsTime (exitCountsFor HeldSentinel period)

-- | The last stop of a non-held run.
endpointTime :: Scenario -> Word64 -> Word64
endpointTime scenario period = countsTime (exitCountsFor scenario period) + 2 * (if scenario `elem` [Natural LoaderEntry, DelayedRelease LoaderEntry] then 60 else 50)

receiptFor :: Scenario -> Program -> [(Word32, Word32)] -> Knobs -> Value
receiptFor scenario program siteWords knobs = object
  [ "schema" .= (3 :: Int), "scenario" .= scenarioValue scenario, "program" .= programName program
  , "result" .= ("passed" :: String), "error" .= Null, "machine_failure" .= Null
  , "instructions" .= total, "cp14_identities" .= ([0x4003, 0x5003] :: [Word32])
  , "epochs" .= ([epochValue "reset" 0 resetClasses "observe", epochValue "released" releasedStart releasedClasses "fixed"]
      ++ [epochValue "published" publishedStart publishedClasses "fixed" | not held])
  , "stops" .= map finishStop stops
  , "bindings" .= object
      [ "decoder_choices" .= ([1] :: [Int])
      , "maps" .= [object ["core" .= (1 :: Int), "address" .= guest, "physical" .= physical, "size" .= size]
                  | (guest, physical, size) <- [(0x10000, 0x10000, 0x4ca0), (0x40020000, 0x20000, 8), (0xd0000, 0xd0000, 0xdaa0), (0x61c000, 0x61c000, 4), (0x61c40c, 0x61c40c, 4)] :: [(Word32, Word32, Word32)]]
      , "tcm" .= heldTcm ]
  , "gate" .= (if control then gateValue else Null)
  , "clear_readback" .= (if delayed then readbackValue else Null)
  , "final_primary" .= (if held then object
      [ "address" .= (0x1ff4 :: Word32), "registers" .= gateRegisters, "sp" .= (0x700007f0 :: Word32)
      , "lr" .= (0x1234 :: Word32), "cpsr" .= (0xd3 :: Word32), "instructions" .= total ] else Null)
  , "segment" .= (if held then Null else object
      [ "root" .= tableGuestRoot, "index" .= (1 :: Int), "record" .= (tableGuestRoot + 36), "header" .= (0x700000 :: Word32)
      , "entry" .= (0xd0020 :: Word32), "argument" .= argumentWord, "loader_sha256" .= loaderDigest, "loader_entry" .= (0xd0020 :: Word32)
      , "table" .= object ["core" .= (1 :: Int), "address" .= tableGuestRoot, "physical" .= tablePhysicalRoot, "size" .= (72 :: Word32)] ])
  , "record" .= object ["sha256" .= programRecordHex program, "site_words" .= [[location, word] | (location, word) <- siteWords]]
  ]
  where
    loaderDigest = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec" :: String
    held = scenario == HeldSentinel
    delayed = case scenario of
      DelayedRelease _ -> True
      _ -> False
    control = held || delayed
    loaderEndpoint = scenario `elem` [Natural LoaderEntry, DelayedRelease LoaderEntry]
    (sites, register, period, initPc, fetchSet, readSet) = case program of
      Retained -> (retainedSites, 3, 304, 0x11818, [], [])
      Owned facts -> let PollPeriod value = ownedPollPeriod facts in
        (ownedSites facts, registerIndexValue (ownedLoadedRegister facts), value, addressOf (siteMain (ownedSites facts)), ownedForbidFetch facts, ownedForbidRead facts)
    site select = addressOf (select sites)
    loads = loadsFor scenario period (knobSkew knobs)
    exitCounts = foldl (\_ (Load counts _ _) -> counts) (0, 0) loads
    exitTime = countsTime exitCounts
    shift time = sum [delta | (threshold, delta) <- knobShifts knobs, time >= threshold]
    bump time = time + shift time
    bumpCounts counts@(first, second) = (first + shift (countsTime counts), second)
    releasedStart = bump 1000
    publishedStart = bump exitTime
    zeros = replicate 13 (0 :: Word32)
    mainRegisters = [0, 0, 0, 0, 0, 0, 6, 7, 8, 9, 10, 11, 0] :: [Word32]
    endpointRegisters = [argumentWord, 0, 0, 0xd0020, 0, 0, 6, 7, 8, 9, 10, 11, 0] :: [Word32]
    gateRegisters = 0x1234 : replicate 12 (0 :: Word32)
    preclearRegisters = [0x1234, 0, 0, 0x40020000] ++ replicate 9 (0 :: Word32)
    -- The retained peer load is ldr r3,[r1]; the owned one loads through its destination.
    loadRegisters value = [if slot == register then value else if slot == 1 && program == Retained then 0x40020000 else 0 | slot <- [0 .. 12]]
    (tableGuestRoot, tablePhysicalRoot) = knobTable knobs
    heldTcm = [[0x70000011, 0], [knobHeldAtcm1 knobs, 0]] :: [[Word32]]
    mainTcm = [[0x70000011, 0], [0x70000011, 0]] :: [[Word32]]
    globals = hexText (B.replicate 0x40 0x11)
    endpointState = Just (object ["globals" .= globals, "companion" .= hexText (knobCompanion knobs)])
    peerRead value = cpuRow "cpu-read" 1 (site sitePeerLoad) 0x40020000 0x20000 value value
    primaryClearTime
      | delayed = readbackTime period
      | otherwise = countsTime (uniformCounts period (knobSkew knobs) 1) + 10
    poll = knobPollValue knobs
    rawRows =
      [(5, const row) | row <- bootBulkWrites]
      ++
      [ (10, const (transferRow "model-bus-write" 0xa00c4))
      , (20, cpuRow "cpu-write" 0 0x1e64 0x40020000 0x20000 0x22b 0x22b)
      , (21, cpuRow "cpu-write" 0 0x1e68 0x40020004 0x20004 0x22b 0x22b)
      , (30, cpuRow "cpu-write" 0 0x1fec 0x400a00c4 0xa00c4 0 0)
      , (40, const (transferRow "host-read" 0x20000))
      , (1030, cpuRow "cpu-write" 1 initPc 0x14c80 0x14c80 0 0xffffffff)
      , (1031, cpuRow "cpu-write" 1 initPc 0x14c84 0x14c84 0 0xffffffff)
      , (1040, cpuRow "cpu-write" 1 (site siteClearStore) 0x40020004 0x20004 0 0x22b) ]
      ++ concat [(countsTime counts - 5, peerRead value) : [(time, peerRead 0x22b) | time <- earlier] | Load counts value earlier <- loads]
      ++ [(primaryClearTime, cpuRow "cpu-write" 0 0x1ff8 0x40020000 0x20000 0 0x22b) | not held]
      ++ [(time, cpuRow "cpu-read" 0 0x2020 0x40020004 0x20004 0x22b 0x22b) | not control, time <- [1035, 1036]]
      ++ (if delayed then
           [(1100, cpuRow "cpu-read" 0 0x114 0x40020004 0x20004 poll poll)]
           ++ [(preclearTime period + 1, const (transferRow "host-read" physical)) | physical <- [0x20000, 0x20000, 0x20000, 0x20004, 0xa00c4, 0x14c80, 0x14c84]]
           ++ [(readbackTime period + 10, cpuRow "cpu-read" 0 0x2020 0x40020004 0x20004 0 0)]
         else [])
      ++ (if held then [] else
           [ (exitTime + 10, cpuRow "cpu-write" 1 (site siteTargetStore) 0x14c80 0x14c80 0xd0020 0)
           , (exitTime + 20, cpuRow "cpu-write" 1 (site siteArgumentStore) 0x14c84 0x14c84 argumentWord 0)
           , (exitTime + 30, cpuRow "cpu-read" 1 (site siteTargetLoad) 0x14c80 0x14c80 0xd0020 0xd0020)
           , (exitTime + 100, cpuRow "cpu-read" 1 (site siteArgumentLoad) 0x14c84 0x14c84 argumentWord argumentWord) ]
           ++ [ (exitTime + offset, cpuRow "cpu-read" 1 pc (tableGuestRoot + tableOffset) (tablePhysicalRoot + tableOffset) value value)
              | knobTableReads knobs, (offset, pc, tableOffset, value) <- [(3, 0x13588, 0, 7), (5, 0x135b4, 36, 1), (7, 0x11f90, 40, 0x700000)] ]
           ++ concat [ [(time, const (sizedTransferRow "host-read" 0x14c60 0x40)), (time, const (sizedTransferRow "host-read" 0xa0000 0x548))]
                     | time <- exitTime + 100 : [exitTime + 120 | loaderEndpoint] ])
      ++ [(time, const value) | (time, value) <- knobExtraRows knobs]
    rows = [(bump time, build (bump time)) | (time, build) <- sortOn fst rawRows]
    epochOf time
      | time <= releasedStart = "reset"
      | held || time <= publishedStart = "released"
      | otherwise = "published" :: String
    releasedCursor time = length [() | (rowTime, _) <- rows, epochOf rowTime == "released", rowTime <= time]
    stop name location cpu counts regs stackPointer state tcm programState =
      let (first, second) = bumpCounts counts
       in (first + second, object
            [ "label" .= (name :: String), "address" .= (location :: Word32), "core" .= (cpu :: Int), "instructions" .= (first + second)
            , "counts" .= [first, second], "registers" .= (regs :: [Word32]), "sp" .= (stackPointer :: Word32), "lr" .= (0x1234 :: Word32)
            , "cpsr" .= (0xd3 :: Word32), "state" .= state, "tcm" .= (tcm :: [[Word32]]), "program_state" .= (programState :: Maybe Value) ])
    idle = peerState 0x22b 0x22b 0 0
    endpointCounts offset = (fst exitCounts + offset, snd exitCounts + offset)
    stops = sortOn fst (
      [ stop "held-release" 0x3fff0000 1 (1000, 0) zeros 0x70000800 idle heldTcm Nothing
      , stop "startup-entry" (site siteEntry) 1 (1004, 2) zeros 0x70000800 idle heldTcm Nothing
      , stop "startup-main" (site siteMain) 1 (1010, 10) mainRegisters 0x70000800 idle mainTcm Nothing ]
      ++ [stop "primary-gate" 0x1ff4 0 gateCounts gateRegisters 0x700007f0 idle mainTcm Nothing | control]
      ++ (if delayed then
           [ stop "peer-acknowledged" 0x14c 0 ackCounts gateRegisters 0x700007f0 idle mainTcm Nothing
           , stop "primary-clear" 0x1ff8 0 (preclearCounts period) preclearRegisters 0x700007f0 idle mainTcm Nothing ]
         else [])
      ++ [stop "peer-loaded" (site sitePeerLoaded) 1 counts (loadRegisters value) 0x70000800 idle mainTcm Nothing | Load counts value _ <- loads]
      ++ (if held then [] else
           [ stop "startup-handoff" (site siteHandoff) 1 (endpointCounts 50) endpointRegisters 0x70000800 (peerState 0 0 0xd0020 argumentWord) mainTcm endpointState ]
           ++ [ stop "loader-b-entry" 0xd0020 1 (endpointCounts 60) endpointRegisters 0x70000800 (peerState 0 0 0xd0020 argumentWord) mainTcm endpointState | loaderEndpoint ]))
    total = maybe 0 fst (foldl (\_ item -> Just item) Nothing stops)
    finishStop (time, value) =
      let epoch = epochOf time
          cursor = length [() | (rowTime, _) <- rows, epochOf rowTime == epoch, rowTime <= time]
       in set "epoch" (String (K.toText (K.fromString epoch))) (set "watch_records" (number (fromIntegral cursor)) value)
    (precleared0, precleared1) = preclearCounts period
    microsteps = [((precleared0, precleared1), (precleared0, precleared1 + 1), 0x22b), ((precleared0, precleared1 + 1), (precleared0 + 1, precleared1 + 1), 0 :: Word32)]
    countsValue counts = let (first, second) = bumpCounts counts in [first, second]
    readbackValue = object
      [ "instructions" .= bump (readbackTime period), "primary_address" .= (0x1ffc :: Word32), "primary_cpsr" .= (0xd3 :: Word32)
      , "primary_instruction" .= ([0, 32, 131, 229] :: [Word8]), "state" .= peerState 0 0 0 0
      , "watch_records" .= releasedCursor (bump (readbackTime period))
      , "microsteps" .= [object ["before" .= countsValue before, "after" .= countsValue after, "cell0" .= scalar] | (before, after, scalar) <- microsteps] ]
    allKinds = ["cpu-read", "cpu-write", "cpu-fetch", "host-read", "host-write", "model-bus-read", "model-bus-write"] :: [String]
    writes = ["cpu-write", "host-write", "model-bus-write"] :: [String]
    resetClasses = [(extent, allKinds) | extent <- [(0x14c80, 0x14c88), (0x20000, 0x20008), (0xa00c4, 0xa00c8)]]
    releasedClasses = resetClasses
      ++ [(extent, writes) | extent <- [(0x10000, 0x14c60), (0x14c6c, 0x14c80)]]
      ++ [(intervalPair extent, "cpu-read" : "cpu-fetch" : "model-bus-read" : writes) | extent <- fetchSet ++ readSet]
      ++ [(extent, ["host-write", "model-bus-write"]) | extent <- [(0x14c60, 0x14c6c), (0x14c88, 0x14ca0), (0xa0000, 0xa00c4), (0xa00c8, 0xa0548)]]
    publishedClasses = releasedClasses
      ++ [(extent, writes) | extent <- [(0xd0000, 0xddaa0), (0x61c40c, 0x61c410), (0x61c000, 0x61c004)]]
      ++ [((fromIntegral tablePhysicalRoot, fromIntegral tablePhysicalRoot + 72), "cpu-read" : writes)]
    epochValue name start classes views = object
      [ "epoch" .= (name :: String), "instructions" .= (start :: Word64)
      , "memory_watch" .= object
          [ "enabled" .= True, "violation" .= Null, "views" .= (views :: String)
          , "ranges" .= [[low, high] | (low, high) <- mergeRanges (map fst classes)]
          , "records" .= [row | (time, row) <- rows, epochOf time == name]
          , "classes" .= [object ["range" .= [low, high], "kinds" .= kinds] | ((low, high), kinds) <- classes] ] ]
    (veneer, gateWords) = if held then (replicate 104 0xff, heldGateWords) else (concatMap leBytes veneerWords, delayedGateWords)
    gateValue = object
      [ "core" .= (0 :: Int), "instructions" .= bump gateTime, "decoder_choices" .= ([1] :: [Int])
      , "low_ram" .= object ["address" .= (0 :: Int), "physical" .= (0 :: Int), "size" .= (16384 :: Int), "permissions" .= (7 :: Int)]
      , "atcm" .= (0x70000011 :: Word32), "btcm" .= (0 :: Int), "stack_address" .= (0x700007d8 :: Word32)
      , "stack" .= replicate 40 (0 :: Word8)
      , "code" .= [ object ["address" .= (0x100 :: Word32), "virtual" .= (veneer :: [Word8]), "physical" .= veneer]
                  , let bytes = concatMap leBytes gateWords in object ["address" .= (0x1fe0 :: Word32), "virtual" .= bytes, "physical" .= bytes] ] ]

startup :: Scenario -> Program -> Value -> Either String ValidatedStartupReceipt
startup scenario program value = parseStartupReceipt (BL.toStrict (encode value)) >>= validateStartupReceipt scenario program

-- * Poll-period fixtures

blFrom :: Word32 -> Word32 -> Word32
blFrom location target = 0xeb000000 .|. (((target - location - 8) `shiftR` 2) .&. 0xffffff)

beqFrom :: Word32 -> Word32 -> Word32
beqFrom location target = 0x0a000000 .|. (((target - location - 8) `shiftR` 2) .&. 0xffffff)

-- | A rendezvous loop polling the peer cell through @cmp@; the fixture's period is 8 entered instructions.
pollFixture :: [C.ByteString] -> Word32 -> Candidate
pollFixture forbidLines comparison = Candidate (B.concat (map encodeLeWord codeWords)) (C.unlines (symbolLines ++ forbidLines))
  where
    (entry, mainAt, cell, rendezvous, publish, handoff) = (0x10400, 0x10410, 0x10414, 0x10424, 0x1043c, 0x10448) :: (Word32, Word32, Word32, Word32, Word32, Word32)
    codeWords =
      [ blFrom entry 0x11854, blFrom (entry + 4) 0x118b8, blFrom (entry + 8) mainAt, 0xeafffffe
      , 0xeafffffe
      , 0xe3001000, 0xe3441002, 0xe5910000, 0xe12fff1e
      , blFrom rendezvous cell, 0xe300222b, comparison, beqFrom (rendezvous + 12) rendezvous, 0xe5810004, 0xe12fff1e
      , 0xe5810000, 0xe5810004, 0xe12fff1e
      , 0xe59f300c, 0xe5933000, 0xe59f0008, 0xe5900000, 0xe1a0f003, 0x14c80, 0x14c84 ]
    symbolLines = [C.pack (showHex8 location ++ " " ++ showHex8 size ++ " T " ++ name) | (location, size, name) <-
      [ (entry, 0x10, "xm1_secondary_entry"), (mainAt, 4, "xm1_secondary_main"), (cell, 0x10, "xm1_secondary_peer_cell")
      , (rendezvous, 0x18, "xm1_secondary_rendezvous"), (publish, 0xc, "xm1_secondary_publish"), (handoff, 0x14, "xm1_secondary_handoff")
      , (handoff + 16, 4, "xm1_secondary_handoff_vector") ] :: [(Word32, Word32, String)]]
    showHex8 :: Word32 -> String
    showHex8 value = let digits = showHex value "" in replicate (8 - length digits) '0' ++ digits

-- | Applies the same CPSR to every endpoint stop.
endpointCpsr :: Word32 -> Value -> Value
endpointCpsr value = labelled "startup-handoff" 0 (set "cpsr" (number (fromIntegral value))) . labelled "loader-b-entry" 0 (set "cpsr" (number (fromIntegral value)))

increment, decrement :: Value -> Value
increment (Number value) = Number (value + 1)
increment value = value
decrement (Number value) = Number (value - 1)
decrement value = value

main :: IO ()
main = do
  arguments <- getArgs
  paths <- case arguments of
    [a, b, c, d, e, f] -> pure [(a, b), (c, d), (e, f)]
    _ -> error "usage: peer-startup-witness-tests IMAGE.bin IMAGE.symbols PUBLICATION_FAULT.bin PUBLICATION_FAULT.symbols STACK_FAULT.bin STACK_FAULT.symbols"
  candidates <- mapM (\(bin, symbols) -> Candidate <$> B.readFile bin <*> B.readFile symbols) paths
  (correct, publicationFault, stackFault) <- case candidates of
    [first, second, third] -> pure (first, second, third)
    _ -> error "three candidates expected"
  let Candidate correctBin correctSymbols = correct

  -- Owned facts from the linked image.
  facts <- assertRight "derive correct image" (derive correct)
  mainSymbol <- symbolAddress correctSymbols "xm1_secondary_main"
  let sites = ownedSites facts
      entry = addressOf (siteEntry sites)
      handoff = addressOf (siteHandoff sites)
      PollPeriod period = ownedPollPeriod facts
  unless (entry == 0x10400) (error "entry differs from 0x10400")
  unless (addressOf (siteMain sites) == mainSymbol) (error "main differs from xm1_secondary_main")
  unless (registerIndexValue (ownedLoadedRegister facts) == 0) (error "loaded register differs from r0")
  unless (ownedImage facts == Interval 0x10400 (0x10400 + fromIntegral (B.length correctBin))) (error "image extent differs")
  unless (leWordAt correctBin (fromIntegral (handoff - entry)) == Right 0xe1a0f003) (error "handoff site word is not MOV PC, R3")
  unless (ownedForbidFetch facts == forbidFetchIntervals && ownedForbidRead facts == forbidReadIntervals) (error "forbid sets differ")
  assertLeft "corrupted handoff word" (derive (Candidate (replaceWord (fromIntegral (handoff - entry)) 0xe1a0f004 correctBin) correctSymbols))
  forM_ [("publication fault", publicationFault), ("stack fault", stackFault)] $ \(label, candidate) -> do
    controlFacts <- assertRight ("derive " ++ label) (derive candidate)
    unless (ownedPollPeriod controlFacts == ownedPollPeriod facts) (error (label ++ " poll period differs from the correct image"))
  let Candidate stackBin stackSymbols = stackFault
  stackFacts <- assertRight "derive stack fault" (derive (Candidate stackBin stackSymbols))
  stackVector <- symbolAddress stackSymbols "xm1_secondary_handoff_vector"
  unless (addressOf (siteHandoff (ownedSites stackFacts)) == stackVector) (error "stack-fault handoff differs from its vector symbol")

  -- Poll-period derivation on synthetic loops.
  let forbidLines = filter ("xm1_forbid_" `B.isInfixOf`) (C.lines correctSymbols)
  fixtureFacts <- assertRight "deterministic synthetic poll loop" (derive (pollFixture forbidLines 0xe1500002))
  unless (ownedPollPeriod fixtureFacts == PollPeriod 8) (error ("synthetic poll period differs: " ++ show (ownedPollPeriod fixtureFacts)))
  assertLeft "poll comparison against an incoming register" (derive (pollFixture forbidLines 0xe1500005))
  assertLeft "poll branch on incoming flags" (derive (pollFixture forbidLines 0xe1a02002))

  -- Contract round trip and refusals.
  let owned = Owned facts
      scenarios = [Natural Handoff, Natural LoaderEntry, HeldSentinel, DelayedRelease Handoff, DelayedRelease LoaderEntry]
      contractBytes scenario program = BL.toStrict (encode (contractValue scenario program))
  forM_ scenarios $ \scenario -> forM_ [Retained, owned] $ \program -> do
    parsed <- assertRight "contract round trip" (parseStartupContract (contractBytes scenario program))
    unless (parsed == (scenario, program)) (error "contract round trip changed the scenario or program")
  let ownedContract = contractValue (Natural Handoff) owned
      shiftFirst (Array entries) = Array (entries V.// [(0, Array (V.fromList [number 0x11804, number 0x11854]))])
      shiftFirst value = value
      reparse = parseStartupContract . BL.toStrict . encode
  assertLeft "shifted forbid_fetch interval" (reparse (change "program" (change "forbid_fetch" shiftFirst) ownedContract))
  assertLeft "omitted forbid_read interval" (reparse (change "program" (change "forbid_read" (vector (V.drop 1))) ownedContract))
  assertLeft "natural budget at control value" (reparse (set "budget" (number 10000000) ownedContract))
  assertLeft "unknown contract field" (reparse (set "extra" Null ownedContract))

  -- Synthetic passing receipts.
  ownedWords <- assertRight "owned site words" (ownedSiteWords correctBin facts)
  let receipt scenario program knobs = receiptFor scenario program (if program == Retained then retainedSiteWords else ownedWords) knobs
      retainedNatural = receipt (Natural LoaderEntry) Retained plain
      ownedNatural = receipt (Natural LoaderEntry) owned plain
      ownedHandoff = receipt (Natural Handoff) owned plain
      ownedHeld = receipt HeldSentinel owned plain
      delayed endpoint = receipt (DelayedRelease endpoint) owned
  forM_ scenarios $ \scenario -> forM_ [Retained, owned] $ \program ->
    assertRight ("synthetic " ++ show scenario ++ " " ++ programName program) (startup scenario program (receipt scenario program plain))
  assertLeft "owned receipt against the retained contract" (startup (Natural LoaderEntry) Retained ownedNatural)
  assertLeft "loader-entry receipt against the handoff contract" (startup (Natural Handoff) owned ownedNatural)
  assertLeft "delayed receipt against the natural contract" (startup (Natural Handoff) owned (delayed Handoff plain))

  let register = registerIndexValue (ownedLoadedRegister facts)
      exitTime scenario = let (first, second) = exitCountsFor scenario period in first + second
      forbiddenFetch = set "label" (String "forbidden-fetch") (set "address" (number 0x11800) (lastStop ownedNatural))
      outsideProtocol = cpuRow "cpu-write" 1 (addressOf (siteMain sites)) 0x10000 0x10000 0 0 1050
      allHeldTcm = change "stops" (each (set "tcm" (Array (V.fromList [Array (V.fromList [number 0x70000011, number 0]), Array (V.fromList [number 0, number 0])]))))
      hostWrite = change "epochs" (at 1 (change "memory_watch" (change "records" (vector (`V.snoc` transferRow "host-write" 0x14c80)))))
      globalsRead = sizedTransferRow "host-read" 0x14c60 0x40
      refusals =
        [ ("wrong core on startup-entry", labelled "startup-entry" 0 (set "core" (number 0)) ownedNatural)
        , ("forbidden-fetch stop", change "stops" (insertBeforeLast forbiddenFetch) ownedNatural)
        , ("r3 0xdeadbe00 at handoff", labelled "startup-handoff" 0 (change "registers" (at 3 (const (number 0xdeadbe00)))) ownedNatural)
        , ("SP 0x700007f8 at handoff", labelled "startup-handoff" 0 (set "sp" (number 0x700007f8)) ownedNatural)
        , ("Jazelle bit at handoff", labelled "startup-handoff" 0 (set "cpsr" (number 0x010000d3)) ownedNatural)
        , ("pair register differs from the read value", labelled "peer-loaded" 0 (change "registers" (at register (const (number 0x22c)))) ownedNatural)
        , ("waiting-period mismatch", receipt (Natural LoaderEntry) owned plain {knobSkew = 1})
        , ("row outside P in released", receipt (Natural LoaderEntry) owned plain {knobExtraRows = [(1050, outsideProtocol)]})
        , ("globals snapshot read in released", receipt (Natural LoaderEntry) owned plain {knobExtraRows = [(1050, globalsRead)]})
        , ("straddling host read in published", receipt (Natural LoaderEntry) owned plain
            {knobExtraRows = [(exitTime (Natural LoaderEntry) + 50, sizedTransferRow "host-read" 0x14c7c 8)]})
        , ("per-core counts that wrap to the stop total", labelled "startup-main" 0 (set "counts" (Array (V.fromList [number (maxBound - 999), number 2020]))) ownedNatural)
        , ("host write", hostWrite ownedNatural)
        , ("missing startup-main TCM transition", allHeldTcm ownedNatural)
        , ("record digest differs", change "record" (set "sha256" (String "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4")) ownedNatural)
        , ("loader digest differs", change "segment" (set "loader_sha256" (String (K.toText (K.fromString (replicate 64 '0'))))) ownedNatural)
        , ("failed result", set "result" (String "failed") ownedNatural)
        , ("unknown receipt field", set "extra" Null ownedNatural)
        ]
  forM_ refusals $ \(label, value) -> assertLeft label (startup (Natural LoaderEntry) owned value)
  assertLeft "snapshot reads beyond the endpoint stops" (startup (Natural Handoff) owned
    (receipt (Natural Handoff) owned plain {knobExtraRows = [(exitTime (Natural Handoff) + 50, globalsRead)]}))
  assertLeft "held gate binding count differs" (startup HeldSentinel owned (change "gate" (set "instructions" (number 1071)) ownedHeld))

  -- Reset-epoch boot transfers: every synthetic receipt above carries the two 0x800-byte boot writes.
  let resetRefusals =
        [ ("boot-sized bus write after the 0x1e68 sentinel", 25, busWrite 0x20000 0x800)
        , ("boot-sized bus write in the released epoch", 1050, busWrite 0x14800 0x800)
        , ("straddling host read in the reset epoch", 15, sizedTransferRow "host-read" 0x1fffc 8)
        , ("straddling model-bus read in the reset epoch", 15, sizedTransferRow "model-bus-read" 0x20000 0x800)
        , ("straddling CPU write in the reset epoch", 15, cpuRow "cpu-write" 0 0x1e64 0x40014c86 0x14c86 0 0 15)
        ]
  forM_ resetRefusals $ \(label, time, row) ->
    assertLeft label (startup (Natural LoaderEntry) owned (receipt (Natural LoaderEntry) owned plain {knobExtraRows = [(time, row)]}))
  forM_ [ ("primary peer-clear poll reads zero before the core-1 clear", cpuRow "cpu-read" 0 0x2020 0x40020004 0x20004 0 0 1037, 1037)
        , ("primary peer-clear poll reads the sentinel after the core-1 clear", cpuRow "cpu-read" 0 0x2020 0x40020004 0x20004 0x22b 0x22b 1045, 1045) ] $
    \(label, row, time) -> assertLeft label (startup (Natural LoaderEntry) owned (receipt (Natural LoaderEntry) owned plain {knobExtraRows = [(time, row)]}))

  -- Held-release TCM: enabled windows are accepted only when disjoint from every guarded view.
  forM_ [("disabled", 0), ("enabled at 0x70000000", 0x70000011)] $ \(label, atcm) ->
    assertRight ("held-release core-1 ATCM " ++ label) (startup (Natural LoaderEntry) owned (receipt (Natural LoaderEntry) owned plain {knobHeldAtcm1 = atcm}))
  forM_ [ ("over record 2", 0x00010011), ("over the peer-cell alias", 0x40020011), ("with a reserved size", 0xffff803d)
        , ("misaligned", 0x70001011) ] $ \(label, atcm) ->
    assertLeft ("held-release core-1 ATCM " ++ label) (startup (Natural LoaderEntry) owned (receipt (Natural LoaderEntry) owned plain {knobHeldAtcm1 = atcm}))

  -- Segment table: the aliased root above, an identity root, and refusals of a mapping the same-run reads do not bear out.
  _ <- assertRight "identity segment table root" (startup (Natural LoaderEntry) owned (receipt (Natural LoaderEntry) owned plain {knobTable = (0x620000, 0x620000)}))
  let tableField name value = change "segment" (change "table" (set name (number value)))
      exitNatural = exitTime (Natural LoaderEntry)
      tableRefusals =
        [ ("table mapping guest differs from the root", tableField "address" 0x4061c044 ownedNatural)
        , ("table mapping size differs from root to record end", tableField "size" 36 ownedNatural)
        , ("table mapping on core 0", tableField "core" 0 ownedNatural)
        , ("table physical differs from the reads", tableField "physical" 0x62c040 ownedNatural)
        , ("no id or header reads through the table mapping", receipt (Natural LoaderEntry) owned plain {knobTableReads = False})
        , ("table read whose guest address the mapping does not give", receipt (Natural LoaderEntry) owned plain
            {knobExtraRows = [(exitNatural + 4, cpuRow "cpu-read" 1 0x135b4 0x4061c048 0x61c04c 0 0 (exitNatural + 4))]})
        , ("table write", receipt (Natural LoaderEntry) owned plain
            {knobExtraRows = [(exitNatural + 4, cpuRow "cpu-write" 1 0x135b4 0x4061c048 0x61c048 0 0 (exitNatural + 4))]})
        ]
  forM_ tableRefusals $ \(label, value) -> assertLeft label (startup (Natural LoaderEntry) owned value)

  -- Peer load bound to its stop by base register and decoded return.
  assertLeft "retained base register differs from the peer-cell address"
    (startup (Natural LoaderEntry) Retained (labelled "peer-loaded" 0 (change "registers" (at 1 (const (number 0x40020004)))) retainedNatural))
  let afterLoad = addressOf (sitePeerLoad sites) + 4
      reloadingReturn = [(location, if location == afterLoad then 0xe8bd0801 else word) | (location, word) <- ownedWords]
  assertLeft "owned return reloads the loaded register" (startup (Natural LoaderEntry) owned (receiptFor (Natural LoaderEntry) owned reloadingReturn plain))

  -- Phase bounds: accepted at each bound, refused one instruction past it.
  let heldGap = heldFinalTime period - gateTime
      clearGap = readbackTime period - gateTime
      releaseGap endpoint = endpointTime (DelayedRelease endpoint) period - readbackTime period
      boundary label scenario threshold room = do
        _ <- assertRight (label ++ " at its bound") (startup scenario owned (receipt scenario owned plain {knobShifts = [(threshold, room)]}))
        assertLeft (label ++ " past its bound") (startup scenario owned (receipt scenario owned plain {knobShifts = [(threshold, room + 1)]}))
  boundary "held prefix-to-gate" HeldSentinel gateTime (4000000 - gateTime)
  boundary "delayed prefix-to-gate" (DelayedRelease Handoff) gateTime (4000000 - gateTime)
  boundary "held gate-to-fourth-load" HeldSentinel (heldFinalTime period) (4000000 - heldGap)
  boundary "delayed gate-to-committed-clear" (DelayedRelease Handoff) (preclearTime period) (5100000 - clearGap)
  forM_ [Handoff, LoaderEntry] $ \endpoint ->
    boundary ("committed-clear-to-" ++ show endpoint) (DelayedRelease endpoint) (endpointTime (DelayedRelease endpoint) period) (20000 - releaseGap endpoint)

  -- Delayed clear evidence.
  let delayedRefusals =
        [ ("microstep skips an instruction", change "clear_readback" (change "microsteps" (at 0 (change "after" (at 1 increment)))) (delayed Handoff plain))
        , ("readback cell not cleared", change "clear_readback" (set "state" (peerState 0x22b 0 0 0)) (delayed Handoff plain))
        , ("readback address is the store itself", change "clear_readback" (set "primary_address" (number 0x1ff8)) (delayed Handoff plain))
        , ("readback cursor precedes the original clear", change "clear_readback" (change "watch_records" decrement) (delayed Handoff plain))
        , ("repeated physical readback", delayed Handoff plain {knobExtraRows = [(preclearTime period + 1, transferRow "host-read" 0x20004)]})
        , ("no zero veneer poll before the acknowledgement", delayed Handoff plain {knobPollValue = 0x22b})
        , ("acknowledgement before the peer clear", labelled "peer-acknowledged" 0 (set "instructions" (number 1030) . set "counts" (Array (V.fromList [number 1010, number 20]))) (delayed Handoff plain))
        ]
  forM_ delayedRefusals $ \(label, value) -> assertLeft label (startup (DelayedRelease Handoff) owned value)

  -- Retained versus owned comparison of validated receipts.
  let compared scenario retainedValue ownedValue = do
        retained <- startup scenario Retained retainedValue
        ownedReceipt <- startup scenario owned ownedValue
        compareStartupReceipts retained ownedReceipt
      companionWord offset = plain {knobCompanion = replaceWord offset 0x12345678 (knobCompanion plain)}
      guardless = change "epochs" (at 1 (change "memory_watch" (set "classes" (Array V.empty))))
  assertRight "comparison ignores timer words" (compared (Natural LoaderEntry) retainedNatural (receipt (Natural LoaderEntry) owned (companionWord 0x40)))
  assertLeft "comparison refuses a differing companion word" (compared (Natural LoaderEntry) retainedNatural (receipt (Natural LoaderEntry) owned (companionWord 0x44)))
  forM_ [Handoff, LoaderEntry] $ \endpoint ->
    assertRight ("delayed " ++ show endpoint ++ " comparison") (compared (DelayedRelease endpoint) (receipt (DelayedRelease endpoint) Retained plain) (delayed endpoint plain))
  assertRight "comparison ignores NZCV" (compared (Natural LoaderEntry) retainedNatural (endpointCpsr 0xf00000d3 ownedNatural))
  forM_ [("Q", 0x080000d3), ("GE", 0x000f00d3), ("A", 0x000001d3), ("E", 0x000002d3), ("I", 0x00000053), ("mode", 0x000000df)] $ \(bit, value) ->
    assertLeft ("comparison refuses a CPSR " ++ bit ++ " difference") (compared (Natural LoaderEntry) retainedNatural (endpointCpsr value ownedNatural))
  assertLeft "comparison refuses identical receipts without guard evidence" (compared (Natural LoaderEntry) (guardless retainedNatural) (guardless ownedNatural))
  assertLeft "comparison refuses an owned receipt as the retained one" (compared (Natural LoaderEntry) ownedNatural ownedNatural)
  retainedValidated <- assertRight "validated retained" (startup (Natural LoaderEntry) Retained retainedNatural)
  ownedValidated <- assertRight "validated owned" (startup (Natural LoaderEntry) owned ownedNatural)
  heldValidated <- assertRight "validated held" (startup HeldSentinel owned ownedHeld)
  assertLeft "comparison refuses swapped programs" (compareStartupReceipts ownedValidated retainedValidated)
  assertLeft "comparison refuses differing scenarios" (compareStartupReceipts retainedValidated heldValidated)
  _ <- assertRight "owned handoff receipt" (startup (Natural Handoff) owned ownedHandoff)
  putStrLn "peer-startup witness tests: pass"
