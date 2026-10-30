{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
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
  }

plain :: Knobs
plain = Knobs 0 [] (B.replicate 0x548 0x5a)

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
transferRow source physical = object
  [ "source" .= source, "address" .= physical, "physical" .= physical, "size" .= (4 :: Int)
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null ]

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
  [ entry, entry + 4, entry + 8, site sitePeerLoad, site sitePeerLoaded - 4, site siteClearStore, site siteTargetStore
  , site siteArgumentStore, site siteTargetLoad, site siteArgumentLoad, site siteHandoff ]
  where
    sites = ownedSites facts
    site select = addressOf (select sites)
    entry = site siteEntry

heldGateWords :: [Word32]
heldGateWords = [0xe3003000, 0xe344300a, 0xe3a02000, 0xe58320c4, 0xe3003000, 0xeafffffe, 0xe5832000, 0xf57ff04f]

receiptFor :: Scenario -> Program -> [(Word32, Word32)] -> Knobs -> Value
receiptFor scenario program siteWords knobs = object
  [ "schema" .= (3 :: Int), "scenario" .= scenarioValue scenario, "program" .= programName program
  , "result" .= ("passed" :: String), "error" .= Null, "machine_failure" .= Null
  , "instructions" .= total, "cp14_identities" .= ([0x4003, 0x5003] :: [Word32])
  , "epochs" .= ([epochValue "reset" 0 resetClasses "observe", epochValue "released" 1000 releasedClasses "fixed"]
      ++ [epochValue "published" exitTime publishedClasses "fixed" | not held])
  , "stops" .= map finishStop stops
  , "bindings" .= object
      [ "decoder_choices" .= ([1] :: [Int])
      , "maps" .= [object ["core" .= (1 :: Int), "address" .= guest, "physical" .= physical, "size" .= size]
                  | (guest, physical, size) <- [(0x10000, 0x10000, 0x4ca0), (0x40020000, 0x20000, 8), (0xd0000, 0xd0000, 0xdaa0), (0x61c000, 0x61c000, 4), (0x61c40c, 0x61c40c, 4)] :: [(Word32, Word32, Word32)]]
      , "tcm" .= heldTcm ]
  , "gate" .= (if held then gateValue else Null)
  , "clear_readback" .= Null
  , "final_primary" .= (if held then object
      [ "address" .= (0x1ff4 :: Word32), "registers" .= gateRegisters, "sp" .= (0x700007f0 :: Word32)
      , "lr" .= (0x1234 :: Word32), "cpsr" .= (0xd3 :: Word32), "instructions" .= total ] else Null)
  , "segment" .= (if held then Null else object
      [ "root" .= (0x620000 :: Word32), "index" .= (1 :: Int), "record" .= (0x620024 :: Word32), "header" .= (0x700000 :: Word32)
      , "entry" .= (0xd0020 :: Word32), "argument" .= argumentWord, "loader_sha256" .= loaderDigest, "loader_entry" .= (0xd0020 :: Word32) ])
  , "record" .= object ["sha256" .= programRecordHex program, "site_words" .= [[location, word] | (location, word) <- siteWords]]
  ]
  where
    loaderDigest = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec" :: String
    held = scenario == HeldSentinel
    loaderEndpoint = scenario `elem` [Natural LoaderEntry, DelayedRelease LoaderEntry]
    (sites, register, period, initPc, fetchSet, readSet) = case program of
      Retained -> (retainedSites, 3, 304, 0x11818, [], [])
      Owned facts -> let PollPeriod value = ownedPollPeriod facts in
        (ownedSites facts, registerIndexValue (ownedLoadedRegister facts), value, addressOf (siteMain (ownedSites facts)), ownedForbidFetch facts, ownedForbidRead facts)
    site select = addressOf (select sites)
    loadCount = if held then 4 else 3 :: Int
    skewAt index = if index >= 1 then knobSkew knobs else 0
    loadCounts index = (1100 + 100 * fromIntegral index, 100 + fromIntegral index * period + skewAt index) :: (Word64, Word64)
    loadTime index = let (first, second) = loadCounts index in first + second
    loadValue index = if not held && index == loadCount - 1 then 0 else 0x22b :: Word32
    exitCounts = loadCounts (loadCount - 1)
    exitTime = loadTime (loadCount - 1)
    zeros = replicate 13 (0 :: Word32)
    mainRegisters = [0, 0, 0, 0, 0, 0, 6, 7, 8, 9, 10, 11, 0] :: [Word32]
    endpointRegisters = [argumentWord, 0, 0, 0xd0020, 0, 0, 6, 7, 8, 9, 10, 11, 0] :: [Word32]
    gateRegisters = 0x1234 : replicate 12 (0 :: Word32)
    loadRegisters index = [if slot == register then loadValue index else 0 | slot <- [0 .. 12]]
    heldTcm = [[0x70000011, 0], [0, 0]] :: [[Word32]]
    mainTcm = [[0x70000011, 0], [0x70000011, 0]] :: [[Word32]]
    globals = hexText (B.replicate 0x40 0x11)
    endpointState = Just (object ["globals" .= globals, "companion" .= hexText (knobCompanion knobs)])
    rows = sortOn fst (
      [ (10, transferRow "model-bus-write" 0xa00c4)
      , (20, cpuRow "cpu-write" 0 0x1e64 0x40020000 0x20000 0x22b 0x22b 20)
      , (21, cpuRow "cpu-write" 0 0x1e68 0x40020004 0x20004 0x22b 0x22b 21)
      , (30, cpuRow "cpu-write" 0 0x1fec 0x400a00c4 0xa00c4 0 0 30)
      , (40, transferRow "host-read" 0x20000)
      , (1030, cpuRow "cpu-write" 1 initPc 0x14c80 0x14c80 0 0xffffffff 1030)
      , (1031, cpuRow "cpu-write" 1 initPc 0x14c84 0x14c84 0 0xffffffff 1031)
      , (1040, cpuRow "cpu-write" 1 (site siteClearStore) 0x40020004 0x20004 0 0x22b 1040) ]
      ++ [ (loadTime index - 5, cpuRow "cpu-read" 1 (site sitePeerLoad) 0x40020000 0x20000 (loadValue index) (loadValue index) (loadTime index - 5))
         | index <- [0 .. loadCount - 1] ]
      ++ (if held then [] else
         [ (loadTime 1 + 10, cpuRow "cpu-write" 0 0x1ff8 0x40020000 0x20000 0 0x22b (loadTime 1 + 10))
         , (exitTime + 10, cpuRow "cpu-write" 1 (site siteTargetStore) 0x14c80 0x14c80 0xd0020 0 (exitTime + 10))
         , (exitTime + 20, cpuRow "cpu-write" 1 (site siteArgumentStore) 0x14c84 0x14c84 argumentWord 0 (exitTime + 20))
         , (exitTime + 30, cpuRow "cpu-read" 1 (site siteTargetLoad) 0x14c80 0x14c80 0xd0020 0xd0020 (exitTime + 30))
         , (exitTime + 40, cpuRow "cpu-read" 1 (site siteArgumentLoad) 0x14c84 0x14c84 argumentWord argumentWord (exitTime + 40)) ])
      ++ knobExtraRows knobs)
    epochOf time
      | time <= 1000 = "reset"
      | held || time <= exitTime = "released"
      | otherwise = "published" :: String
    stop name location cpu (first, second) regs stackPointer state tcm programState =
      (first + second, object
        [ "label" .= (name :: String), "address" .= (location :: Word32), "core" .= (cpu :: Int), "instructions" .= (first + second)
        , "counts" .= [first, second], "registers" .= (regs :: [Word32]), "sp" .= (stackPointer :: Word32), "lr" .= (0x1234 :: Word32)
        , "cpsr" .= (0xd3 :: Word32), "state" .= state, "tcm" .= (tcm :: [[Word32]]), "program_state" .= (programState :: Maybe Value) ])
    idle = peerState 0x22b 0x22b 0 0
    stops =
      [ stop "held-release" 0x3fff0000 1 (1000, 0) zeros 0x70000800 idle heldTcm Nothing
      , stop "startup-entry" (site siteEntry) 1 (1004, 2) zeros 0x70000800 idle heldTcm Nothing
      , stop "startup-main" (site siteMain) 1 (1010, 10) mainRegisters 0x70000800 idle mainTcm Nothing ]
      ++ [stop "primary-gate" 0x1ff4 0 (1050, 20) gateRegisters 0x700007f0 idle mainTcm Nothing | held]
      ++ [stop "peer-loaded" (site sitePeerLoaded) 1 (loadCounts index) (loadRegisters index) 0x70000800 idle mainTcm Nothing | index <- [0 .. loadCount - 1]]
      ++ (if held then [] else
         [ stop "startup-handoff" (site siteHandoff) 1 (fst exitCounts + 50, snd exitCounts + 50) endpointRegisters 0x70000800 (peerState 0 0 0xd0020 argumentWord) mainTcm endpointState ]
         ++ [ stop "loader-b-entry" 0xd0020 1 (fst exitCounts + 60, snd exitCounts + 60) endpointRegisters 0x70000800 (peerState 0 0 0xd0020 argumentWord) mainTcm endpointState | loaderEndpoint ])
    total = maybe 0 fst (foldl (\_ item -> Just item) Nothing stops)
    finishStop (time, value) =
      let epoch = epochOf time
          cursor = length [() | (rowTime, _) <- rows, epochOf rowTime == epoch, rowTime <= time]
       in set "epoch" (String (K.toText (K.fromString epoch))) (set "watch_records" (number (fromIntegral cursor)) value)
    allKinds = ["cpu-read", "cpu-write", "cpu-fetch", "host-read", "host-write", "model-bus-read", "model-bus-write"] :: [String]
    writes = ["cpu-write", "host-write", "model-bus-write"] :: [String]
    resetClasses = [(extent, allKinds) | extent <- [(0x14c80, 0x14c88), (0x20000, 0x20008), (0xa00c4, 0xa00c8)]]
    releasedClasses = resetClasses
      ++ [(extent, writes) | extent <- [(0x10000, 0x14c60), (0x14c6c, 0x14c80)]]
      ++ [(intervalPair extent, "cpu-read" : "cpu-fetch" : "model-bus-read" : writes) | extent <- fetchSet ++ readSet]
      ++ [(extent, ["host-write", "model-bus-write"]) | extent <- [(0x14c60, 0x14c6c), (0x14c88, 0x14ca0), (0xa0000, 0xa00c4), (0xa00c8, 0xa0548)]]
    publishedClasses = releasedClasses
      ++ [(extent, writes) | extent <- [(0xd0000, 0xddaa0), (0x61c40c, 0x61c410), (0x61c000, 0x61c004), (0x620000, 0x620000 + 36 * 2)]]
    epochValue name start classes views = object
      [ "epoch" .= (name :: String), "instructions" .= (start :: Word64)
      , "memory_watch" .= object
          [ "enabled" .= True, "violation" .= Null, "views" .= (views :: String)
          , "ranges" .= [[low, high] | (low, high) <- mergeRanges (map fst classes)]
          , "records" .= [row | (time, row) <- rows, epochOf time == name]
          , "classes" .= [object ["range" .= [low, high], "kinds" .= kinds] | ((low, high), kinds) <- classes] ] ]
    gateValue = object
      [ "core" .= (0 :: Int), "instructions" .= (1070 :: Word64), "decoder_choices" .= ([1] :: [Int])
      , "low_ram" .= object ["address" .= (0 :: Int), "physical" .= (0 :: Int), "size" .= (16384 :: Int), "permissions" .= (7 :: Int)]
      , "atcm" .= (0x70000011 :: Word32), "btcm" .= (0 :: Int), "stack_address" .= (0x700007d8 :: Word32)
      , "stack" .= replicate 40 (0 :: Word8)
      , "code" .= [ object ["address" .= (0x100 :: Word32), "virtual" .= replicate 104 (0xff :: Word8), "physical" .= replicate 104 (0xff :: Word8)]
                  , let bytes = concatMap leBytes heldGateWords in object ["address" .= (0x1fe0 :: Word32), "virtual" .= bytes, "physical" .= bytes] ] ]

startup :: Scenario -> Program -> Value -> Either String ()
startup scenario program value = parseStartupReceipt (BL.toStrict (encode value)) >>= validateStartupReceipt scenario program

parseValue :: Value -> Either String StartupReceipt
parseValue = parseStartupReceipt . BL.toStrict . encode

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
  unless (entry == 0x10400) (error "entry differs from 0x10400")
  unless (addressOf (siteMain sites) == mainSymbol) (error "main differs from xm1_secondary_main")
  unless (ownedPollPeriod facts == PollPeriod 512) (error ("poll period differs: " ++ show (ownedPollPeriod facts)))
  unless (registerIndexValue (ownedLoadedRegister facts) == 0) (error "loaded register differs from r0")
  unless (ownedImage facts == Interval 0x10400 (0x10400 + fromIntegral (B.length correctBin))) (error "image extent differs")
  unless (leWordAt correctBin (fromIntegral (handoff - entry)) == Right 0xe1a0f003) (error "handoff site word is not MOV PC, R3")
  unless (ownedForbidFetch facts == forbidFetchIntervals && ownedForbidRead facts == forbidReadIntervals) (error "forbid sets differ")
  assertLeft "corrupted handoff word" (derive (Candidate (replaceWord (fromIntegral (handoff - entry)) 0xe1a0f004 correctBin) correctSymbols))
  forM_ [("publication fault", publicationFault), ("stack fault", stackFault)] $ \(label, candidate) -> do
    controlFacts <- assertRight ("derive " ++ label) (derive candidate)
    unless (ownedPollPeriod controlFacts == PollPeriod 512) (error (label ++ " poll period differs"))
  let Candidate stackBin stackSymbols = stackFault
  stackFacts <- assertRight "derive stack fault" (derive (Candidate stackBin stackSymbols))
  stackVector <- symbolAddress stackSymbols "xm1_secondary_handoff_vector"
  unless (addressOf (siteHandoff (ownedSites stackFacts)) == stackVector) (error "stack-fault handoff differs from its vector symbol")

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
  let retainedNatural = receiptFor (Natural LoaderEntry) Retained retainedSiteWords plain
      ownedNatural = receiptFor (Natural LoaderEntry) owned ownedWords plain
      ownedHeld = receiptFor HeldSentinel owned ownedWords plain
  _ <- assertRight "retained natural loader-entry" (startup (Natural LoaderEntry) Retained retainedNatural)
  _ <- assertRight "owned natural loader-entry" (startup (Natural LoaderEntry) owned ownedNatural)
  _ <- assertRight "owned held sentinel" (startup HeldSentinel owned ownedHeld)
  _ <- assertRight "owned natural handoff" (startup (Natural Handoff) owned (receiptFor (Natural Handoff) owned ownedWords plain))
  assertLeft "owned receipt against the retained contract" (startup (Natural LoaderEntry) Retained ownedNatural)
  assertLeft "loader-entry receipt against the handoff contract" (startup (Natural Handoff) owned ownedNatural)

  let register = registerIndexValue (ownedLoadedRegister facts)
      forbiddenFetch = set "label" (String "forbidden-fetch") (set "address" (number 0x11800) (lastStop ownedNatural))
      outsideProtocol = cpuRow "cpu-write" 1 (addressOf (siteMain sites)) 0x10000 0x10000 0 0 1050
      allHeldTcm = change "stops" (each (set "tcm" (Array (V.fromList [Array (V.fromList [number 0x70000011, number 0]), Array (V.fromList [number 0, number 0])]))))
      hostWrite = change "epochs" (at 1 (change "memory_watch" (change "records" (vector (`V.snoc` transferRow "host-write" 0x14c80)))))
      refusals =
        [ ("wrong core on startup-entry", labelled "startup-entry" 0 (set "core" (number 0)) ownedNatural)
        , ("forbidden-fetch stop", change "stops" (insertBeforeLast forbiddenFetch) ownedNatural)
        , ("r3 0xdeadbe00 at handoff", labelled "startup-handoff" 0 (change "registers" (at 3 (const (number 0xdeadbe00)))) ownedNatural)
        , ("SP 0x700007f8 at handoff", labelled "startup-handoff" 0 (set "sp" (number 0x700007f8)) ownedNatural)
        , ("pair register differs from the read value", labelled "peer-loaded" 0 (change "registers" (at register (const (number 0x22c)))) ownedNatural)
        , ("waiting-period mismatch", receiptFor (Natural LoaderEntry) owned ownedWords plain {knobSkew = 1})
        , ("row outside P in released", receiptFor (Natural LoaderEntry) owned ownedWords plain {knobExtraRows = [(1050, outsideProtocol)]})
        , ("host write", hostWrite ownedNatural)
        , ("missing startup-main TCM transition", allHeldTcm ownedNatural)
        , ("record digest differs", change "record" (set "sha256" (String "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4")) ownedNatural)
        , ("loader digest differs", change "segment" (set "loader_sha256" (String (K.toText (K.fromString (replicate 64 '0'))))) ownedNatural)
        , ("failed result", set "result" (String "failed") ownedNatural)
        , ("unknown receipt field", set "extra" Null ownedNatural)
        ]
  forM_ refusals $ \(label, value) -> assertLeft label (startup (Natural LoaderEntry) owned value)
  assertLeft "held gate binding count differs" (startup HeldSentinel owned (change "gate" (set "instructions" (number 1071)) ownedHeld))

  -- Retained versus owned comparison.
  retainedReceipt <- assertRight "parse retained receipt" (parseValue retainedNatural)
  let timerDiffers = receiptFor (Natural LoaderEntry) owned ownedWords plain {knobCompanion = replaceWord 0x40 0x12345678 (knobCompanion plain)}
      eventDiffers = receiptFor (Natural LoaderEntry) owned ownedWords plain {knobCompanion = replaceWord 0x44 0x12345678 (knobCompanion plain)}
  ownedTimer <- assertRight "parse owned receipt with differing timer" (parseValue timerDiffers)
  _ <- assertRight "owned timer receipt still passes" (startup (Natural LoaderEntry) owned timerDiffers)
  _ <- assertRight "comparison ignores timer words" (compareStartupReceipts retainedReceipt ownedTimer)
  ownedEvent <- assertRight "parse owned receipt with differing companion" (parseValue eventDiffers)
  assertLeft "comparison refuses a differing companion word" (compareStartupReceipts retainedReceipt ownedEvent)
  assertLeft "comparison refuses swapped programs" (compareStartupReceipts ownedTimer retainedReceipt)
  heldReceipt <- assertRight "parse held receipt" (parseValue ownedHeld)
  assertLeft "comparison refuses differing scenarios" (compareStartupReceipts retainedReceipt heldReceipt)
  putStrLn "peer-startup witness tests: pass"
