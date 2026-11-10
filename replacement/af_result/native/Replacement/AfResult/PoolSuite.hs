{-# LANGUAGE OverloadedStrings #-}
module Replacement.AfResult.PoolSuite
  ( Scope, Run (..), Mode (..), readScope, runSuite, compareObservations, sourceIdentity, consumerSourceIdentity ) where

import qualified ArmElf as E
import Control.DeepSeq (NFData (..), force)
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit, toLower)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import System.FilePath ((</>))

data Mode = Qualification | Smoke deriving (Eq, Show)
data Role = Stock | Owned deriving (Eq, Show)
data Operation = Select | Rotate | Reserve | Release | Pair deriving (Eq, Show)
data Pool = Ordinary0 | Ordinary1 | Alternate4 deriving (Eq, Show)
data SlotState = Free | Reserved | Marked | Current | State0d | Late deriving (Eq, Show, Enum, Bounded)
newtype RequestedSlot = RequestedSlot Word32
newtype Counter = Counter Word32
newtype UnknownWord = UnknownWord Word32

data Case = Case
  { name :: String, operation :: Operation, pool :: Pool, selector :: Word32
  , request :: RequestedSlot, states :: [SlotState], peerStates :: [SlotState]
  , counters :: (Counter, Counter), mark :: Word32, policy :: Word32, unknown :: UnknownWord
  , alternateState :: Word32, rotateCount :: Maybe Word32
  }

data Scope = Scope B.ByteString E.Elf E.Elf

data Run = Run
  { runName :: String, runDifferences :: [String], runControls :: [(String, Bool)] }

instance NFData Run where
  rnf (Run ident differences controls) = rnf (ident,differences,controls)

sourceIdentity :: String
sourceIdentity = "x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; image5 SHA256 3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a base00bf1a00; decomp/firmwares/xm1-150/app/capture/pipeline/tu_0103b1e8.c:2218-2805 NONMATCHING guide; tu_0102cfc4.c:1195 retained caller0102f1b0; image BL0102f1c4 and0102f1d4 to0103fb50"

consumerSourceIdentity :: String
consumerSourceIdentity = sourceIdentity ++ "; secondary0103fd88 extentSHAf8373c3f9d3adfd4c67dc78848f7e23427da987d4cb3ce9ee33498ebe09f8738; decomp/firmwares/xm1-150/app/capture/pipeline/tu_0103b1e8.c:2807-2924 secondary consumer"

ranges :: [(Word32, Word32)]
ranges = [(0x103f190, 460), (0x103fb50, 568)]

ownedRanges :: [(Word32, Word32)]
ownedRanges = ranges ++ [(0x103fd88, 740)]

entries :: [(String, Word32)]
entries = [("xm1_af_select", 0x103f190), ("xm1_af_rotate", 0x103f1fc), ("xm1_af_reserve", 0x103fb50), ("xm1_af_release", 0x103fd38), ("xm1_af_secondary_consumer", 0x103fd88)]

insideOld :: Word32 -> Word32 -> Bool
insideOld address size = any (\(base, width) -> address >= base && toInteger address + toInteger size <= toInteger base + toInteger width) ranges

readScope :: FilePath -> FilePath -> FilePath -> IO Scope
readScope imagePath oldElfPath elfPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  oldElf <- either fail pure . E.parseElf =<< B.readFile oldElfPath
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  validateElf "old owned" (take 4 entries) ranges oldElf
  validateElf "owned" entries ownedRanges elf
  case legacyPayloadMismatch oldElf elf of
    Just address -> fail ("legacy payload differs from old ELF at 0x" ++ showHex address)
    Nothing -> pure ()
  let wordsAt :: Word32 -> Word32 -> [Word32]
      wordsAt address count = [readWordAt image (fromIntegral (address - 0xbf1a00 + 4 * i)) | i <- [0 .. count - 1]]
  unless (wordsAt 0x102f1b0 11 == [0xe92d48f0,0xe28db014,0xe24dde12,0xe30f0fff,0xe3a01000,0xeb004261,0xe1a06000,0xe30f0fff,0xe3a01001,0xeb00425d,0xe1a04000]) (fail "retained pair caller bytes differ")
  pure (Scope image oldElf elf)

validateElf :: String -> [(String, Word32)] -> [(Word32, Word32)] -> E.Elf -> IO ()
validateElf label expectedEntries expectedRanges elf = do
  forM_ expectedEntries $ \(symbol, address) -> case filter ((== symbol) . E.symbolName) (E.elfSymbols elf) of
    [entry] | E.symbolValue entry == address, E.symbolKind entry == E.FunctionSymbol, E.symbolSize entry > 0, any (\(base, width) -> address >= base && toInteger address + toInteger (E.symbolSize entry) <= toInteger base + toInteger width) expectedRanges, E.InSection index <- E.symbolSection entry, index < length (E.elfSections elf), index >= 0 -> do
      let section = E.elfSections elf !! index
      unless (E.sectionAllocated section && E.sectionExecutable section && not (E.sectionWritable section)) (fail (label ++ " " ++ symbol ++ " is not executable read-only allocated code"))
    _ -> fail ("missing or misbound " ++ label ++ " function " ++ symbol)
  forM_ (filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)) $ \section ->
    unless (E.sectionType section == 1 && not (E.sectionWritable section) && any (\(base, width) -> E.sectionAddress section >= base && toInteger (E.sectionAddress section) + toInteger (E.sectionSize section) <= toInteger base + toInteger width) expectedRanges) (fail (label ++ " allocated section escapes sparse ranges"))
  forM_ expectedRanges $ \(base, size) -> unless (all (\a -> E.byteAt elf a /= Nothing) [base .. base + size - 1]) (fail (label ++ " sparse range has missing bytes"))

legacyPayloadMismatch :: E.Elf -> E.Elf -> Maybe Word32
legacyPayloadMismatch oldElf elf = case [base + offset | (base,size) <- ranges, offset <- [0 .. size - 1], E.byteAt oldElf (base + offset) /= E.byteAt elf (base + offset)] of
  [] -> Nothing
  address : _ -> Just address

poolBase :: Pool -> Word32
poolBase Ordinary0 = 0x16fedd0
poolBase Ordinary1 = 0x16fed28
poolBase Alternate4 = 0x1702f84

poolCount :: Pool -> Int
poolCount Ordinary0 = 5
poolCount Ordinary1 = 7
poolCount Alternate4 = 4

stateWord :: SlotState -> Word32
stateWord Free = 0
stateWord Reserved = 1
stateWord Marked = 2
stateWord Current = 3
stateWord State0d = 0xd
stateWord Late = 0x1b

baseCase :: Pool -> Case
baseCase p = Case "" Reserve p (if p == Ordinary1 then 1 else 0) (RequestedSlot 0xffff) (replicate (poolCount p) Free) (replicate 7 Free) (Counter 10, Counter 10) 0 0 (UnknownWord 0x89abcdef) 5 Nothing

cases :: Mode -> [Case]
cases mode = zipWith identify [0 :: Int ..] (if mode == Smoke then smokeCases else smokeCases ++ selections ++ reserves ++ rotations ++ pairs)
  where
    identify index one = one { name = "af-" ++ show index ++ "-" ++ take 48 (map sanitize (name one)) }
    sanitize character = let lower = toLower character in if isAsciiLower lower || isDigit lower then lower else '-'
    label p suffix = show p ++ "-" ++ suffix
    smokeCases = [ (baseCase p) { name = label p (show op), operation = op } | p <- [Ordinary0, Ordinary1, Alternate4], op <- [Select, Rotate, Reserve, Release] ] ++ [ (baseCase Ordinary0) { name = "pair-first-failure-second-acquires", operation = Pair, states = replicate 5 State0d, mark = 1 }, (baseCase Alternate4) { name = "pair-second-failure-first-acquires", operation = Pair, peerStates = replicate 7 State0d } ] ++ [ (baseCase p) { name = label p "pair-success", operation = Pair, mark = 1 } | p <- [Ordinary0,Alternate4] ]
    selections = [ (baseCase p) { name = label p ("selector-" ++ show raw ++ "-state-" ++ show st), operation = Select, selector = raw, alternateState = st } | p <- [Ordinary0, Alternate4], raw <- [0,1,2,0xffffffff], st <- [0,4,5,6,7,0xffffffff] ]
    reserves = [ (baseCase p) { name = label p ("reserve-" ++ show s ++ "-slot-" ++ show slot ++ "-mark-" ++ show m ++ "-counter-" ++ show (a,b)), states = replicate slot State0d ++ [s] ++ replicate (poolCount p - slot - 1) State0d, mark = m, counters = (Counter a, Counter b) } | p <- [Ordinary0, Ordinary1, Alternate4], s <- [minBound .. maxBound], slot <- [0 .. poolCount p - 1], m <- [0,1,2], (a,b) <- [(10,10),(12,10),(11,10),(0,0xffffffff),(0xffffffff,0xffffffff)] ]
      ++ [ (baseCase p) { name = label p ("requested-" ++ show slot ++ "-" ++ show s), request = RequestedSlot (fromIntegral slot), states = replicate slot Free ++ [s] ++ replicate (poolCount p - slot - 1) Free, mark = 1 } | p <- [Ordinary0, Ordinary1, Alternate4], slot <- [0 .. poolCount p - 1], s <- [minBound .. maxBound] ]
      ++ [ (baseCase p) { name = label p ("reserve-mixed-" ++ show pos ++ "-mark-" ++ show m ++ "-counter-" ++ show (a,b)), states = take (poolCount p) (drop pos (cycle [Current,Reserved,Current,Free,Reserved,Marked,Late])), mark = m, counters = (Counter a,Counter b) } | p <- [Ordinary0,Ordinary1,Alternate4], pos <- [0..6], m <- [0,1], (a,b) <- [(10,10),(11,10),(12,10),(0,0xffffffff)] ]
      ++ [ (baseCase Alternate4) { name = "alternate-state6-" ++ show op, operation = op, alternateState = 6 } | op <- [Reserve,Release] ]
    rotations = [ (baseCase p) { name = label p (show op ++ "-" ++ show pos ++ "-policy-" ++ show pol ++ "-unknown-" ++ show u ++ "-counter-" ++ show (a,b)), operation = op, states = take (poolCount p) (drop pos (cycle [Current,Marked,Reserved,Late,State0d,Free])), policy = pol, unknown = UnknownWord u, counters = (Counter a,Counter b) } | p <- [Ordinary0, Ordinary1, Alternate4], op <- [Rotate,Release], pos <- [0..5], pol <- [0,1,2], u <- [0,0x89abcdef], (a,b) <- [(10,10),(11,10),(0,0xffffffff),(0xffffffff,0)] ]
      ++ [ (baseCase p) { name = label p "rotate-empty", operation = Rotate, rotateCount = Just 0 } | p <- [Ordinary0, Ordinary1, Alternate4] ]
      ++ [ (baseCase p) { name = label p (show op ++ "-uniform-" ++ show s ++ "-policy-" ++ show pol), operation = op, states = replicate (poolCount p) s, policy = pol } | p <- [Ordinary0,Ordinary1,Alternate4], op <- [Rotate,Release], s <- [minBound .. maxBound], pol <- [0,1] ]
    pairs = [ (baseCase p) { name = label p ("pair-" ++ show first ++ "-" ++ show second ++ "-mark-" ++ show m), operation = Pair, states = replicate (poolCount p) first, peerStates = replicate 7 second, mark = m } | p <- [Ordinary0,Alternate4], first <- [Free,State0d], second <- [Free,State0d], m <- [0,1,2] ]

poke :: B.ByteString -> Word32 -> Word32 -> B.ByteString
poke bytes offset value = B.take (fromIntegral offset) bytes <> word32le value <> B.drop (fromIntegral offset + 4) bytes

seed :: Register -> Word32
seed r = 0xa5a50000 + fromIntegral (fromEnum r)

guest :: Word32 -> GuestAddress
guest = GuestAddress . fromIntegral

mov :: Word32 -> Word32 -> [Word32]
mov rd value = [0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (rd `shiftL` 12) .|. (value .&. 0xfff), 0xe3400000 .|. (((value `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (rd `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)]

receiptBase, outputBase, stackBase, stackPointer, exitAddress :: Word32
receiptBase = 0x200000
outputBase = 0x201000
stackBase = 0x300000
stackPointer = stackBase + 0x800
exitAddress = 0x202000

-- These critical interfaces record calls and a token; they do not model physical locking.
stubs :: [(Word32, [Word32])]
stubs =
  [ (0x7235d8, mov 2 receiptBase ++ [0xe5923000,0xe2833001,0xe5823000] ++ mov 0 0x5a5a ++ [0xe12fff1e])
  , (0x7236b0, mov 2 receiptBase ++ [0xe5820008,0xe5923004,0xe2833001,0xe5823004,0xe12fff1e])
  , (0xed8b10, mov 12 0x203000 ++ [0xe12fff1c])
  , (0x203000, mov 2 480 ++ [0xe580202c] ++ mov 2 640 ++ [0xe5802030] ++ mov 2 240 ++ [0xe5802034] ++ mov 2 320 ++ [0xe5802038,0xe12fff1e])
  , (0x1005f60, mov 2 0x12345678 ++ [0xe5812000,0xe12fff1e])
  ] ++ [(a, [0xe3a00000,0xe12fff1e]) | a <- [0x1029f8c,0x102df4c,0xed8af4,0x76d9c8,0x76da84,0xed8b2c,0xedb51c,0xed8b6c,0x103faf4,0x75e704,0xed9378,0xed95b4,0xedb590,0x1029430]]

packedStubs :: [(Word32, B.ByteString)]
packedStubs = foldr combine [] [(address, assemble body) | (address,body) <- sortOn fst stubs]
  where
    combine (address,bytes) ((next,rest):others)
      | next - address - fromIntegral (B.length bytes) <= 0x400 =
          let padding = fromIntegral (next - address) - B.length bytes
          in (address,bytes <> B.concat (replicate (padding `div` 4) (word32le 0xe7f000f0)) <> rest):others
    combine item others = item:others

stateBanks :: [(RegionId, Word32, Word32)]
stateBanks = [(RegionId "state",0x16fec00,0x248),(RegionId "late-state",0x1702f84,0x884)]

stateRegion :: Case -> B.ByteString
stateRegion one = foldl (\bytes (address,value) -> poke bytes (address - base) value) initial words'
  where
    base = 0x16fec00
    initial = B.concat [word32le (0xcafe0000 + fromIntegral i) | i <- [0 :: Int .. 0x4fff `div` 4]]
    (Counter produced, Counter consumed) = counters one
    UnknownWord u = unknown one
    poolWords p ss = [(poolBase p + fromIntegral i * 24 + offset, value) | (i,s) <- zip [0 :: Int ..] ss, (offset,value) <- [(0,0x90000000 + fromIntegral i * 0x1000),(4,stateWord s),(8,0xfeed0000 + fromIntegral i),(12,0xbeef0000 + fromIntegral i),(16,u),(20,u `xor` 0x55555555)]]
    selected = pool one
    primary0 = if selected == Ordinary0 then states one else replicate 5 State0d
    primary1 = if selected == Ordinary1 then states one else peerStates one
    alternate = if selected == Alternate4 then states one else replicate 4 State0d
    words' = [(0x16fec4c,if selected == Alternate4 then alternateState one else 0),(0x170306c,mark one),(0x1703080,produced),(0x1703084,consumed),(0x170319c,policy one),(0x1703110,0x10203040)] ++ poolWords Ordinary0 primary0 ++ poolWords Ordinary1 primary1 ++ poolWords Alternate4 alternate

caseFor :: Role -> Scope -> Case -> CaseSpec
caseFor role (Scope image oldElf _) one = CaseSpec
  { caseId = name one
  , caseProvenance = sourceIdentity
  , caseExclusion = "Explicit synthetic pool records, unknown words, counter/mark/policy globals and external services. No captured globals, concurrency, physical critical locking, timing, scheduler or stall qualification. Full retained0102f1b0 caller bytes; all non-owned calls are synthetic."
  , caseEntry = guest entry
  , caseExit = guest exitAddress
  , caseRegions = codeRegions ++ [localRegion "retained-pair-caller" 0x102f1b0 ReadExecute (slice 0x102f1b0 0x1d0)] ++ [localRegion ("synthetic-interface-" ++ show a) a ReadExecute bytes | (a,bytes) <- packedStubs] ++ [localRegion (regionName ident) base ReadWrite (B.take (fromIntegral size) (B.drop (fromIntegral (base - 0x16fec00)) stateBytes)) | (ident,base,size) <- stateBanks] ++ [localRegion "output" outputBase ReadWrite (B.replicate 16 0xa5),localRegion "receipts" receiptBase ReadWrite (B.replicate 16 0),localRegion "stack" stackBase ReadWrite (B.replicate 0x1000 0xa5),localRegion "exit" exitAddress ReadExecute (word32le 0xe7f000f0)]
  , caseFunctions = [(guest a,z) | (a,z) <- functions ++ [(0x102f1b0,0x1d0)] ++ [(a,fromIntegral (length body * 4)) | (a,body) <- stubs]]
  , caseInitial = [(r,maybe (seed r) id (lookup r args)) | r <- [R0 .. R12]] ++ [(Sp,stackPointer),(Lr,exitAddress),(Cpsr,0x10)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(r,seed r) | r <- [R4 .. R11]] ++ [(Sp,stackPointer)]
  , caseExpectedMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    localRegion ident address perm bytes = mkRegion (RegionId ident) (guest address) (B.length bytes) perm bytes
    slice :: Word32 -> Word32 -> B.ByteString
    slice address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - 0xbf1a00)) image)
    stateBytes = stateRegion one
    codeRegions = [localRegion ("owned-range-" ++ show a) a ReadExecute bytes | (a,z) <- ranges, let bytes = if role == Stock then slice a z else B.pack [maybe (error "missing checked ELF byte") id (E.byteAt oldElf x) | x <- [a .. a+z-1]]]
    functions = if role == Stock then [(0x103f190,108),(0x103f1fc,352),(0x103fb50,488),(0x103fd38,80)] else [(E.symbolValue s,E.symbolSize s) | s <- E.elfSymbols oldElf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0, insideOld (E.symbolValue s) (E.symbolSize s)]
    RequestedSlot requested = request one
    count = maybe (fromIntegral (poolCount (pool one))) id (rotateCount one)
    (entry,args) = case operation one of
      Select -> (0x103f190,[(R0,selector one),(R1,outputBase),(R2,outputBase+4)])
      Rotate -> (0x103f1fc,[(R0,count),(R1,poolBase (pool one))])
      Reserve -> (0x103fb50,[(R0,requested),(R1,selector one)])
      Release -> (0x103fd38,[(R0,selector one)])
      Pair -> (0x102f1b0,[])

observedRegions :: [RegionId]
observedRegions = map RegionId ["state","late-state","output","receipts"]

compareObservations :: Bool -> NativeObservation -> NativeObservation -> [String]
compareObservations resultIsDefined reference subject =
  ["stock status" | nativeStatus reference /= Returned] ++ ["owned status" | nativeStatus subject /= Returned]
  ++ ["region " ++ regionName ident | ident <- observedRegions, regionBytesOf reference ident /= regionBytesOf subject ident]
  ++ ["register " ++ registerLabel r | r <- (if resultIsDefined then [R0] else []) ++ [R4 .. R11] ++ [Sp], registerValue reference r /= registerValue subject r]
  ++ ["callee-saved " ++ registerLabel r | r <- [R4 .. R11], registerValue subject r /= seed r]
  ++ ["caller stack" | registerValue subject Sp /= stackPointer]
  ++ ["stock caller stack guard" | not (stackGuards reference)]
  ++ ["owned caller stack guard" | not (stackGuards subject)]

stackGuards :: NativeObservation -> Bool
stackGuards observation =
  let bytes = regionBytesOf observation (RegionId "stack")
  in B.length bytes == 0x1000 && B.all (== 0xa5) (B.take 0x400 bytes) && B.all (== 0xa5) (B.drop 0x800 bytes)

runSuite :: Mode -> FilePath -> FilePath -> FilePath -> Scope -> IO [Run]
runSuite mode runner cache directory scope = do
  existing <- fmap concat $ forM (zip [0 :: Int ..] (chunks (cases mode))) $ \(index,selected) ->
    runCases runner cache (directory </> ("batch-" ++ show index)) scope selected >>= evaluate . force
  consumers <- runConsumerCases mode runner cache (directory </> "consumer") scope
  corrections <- runCorrectionCases mode runner cache (directory </> "correction") scope
  callers <- runCallerCases mode runner cache (directory </> "callers") scope
  pure (existing ++ consumers ++ corrections ++ callers)
  where
    chunks [] = []
    chunks remaining = let (selected,rest) = splitAt 64 remaining in selected : chunks rest

runCases :: FilePath -> FilePath -> FilePath -> Scope -> [Case] -> IO [Run]
runCases runner cache directory scope selected = do
  batches <- forM [Stock,Owned] $ \role -> do
    batch <- writeBatch (directory </> show role) (map (caseFor role scope) selected)
    (_,observed) <- cachedObservedBatch cache runner batch
    pure (M.fromList [(caseIdText ident,o) | (ident,o) <- observed])
  case batches of
    [reference,subject] -> forM selected $ \one -> case (M.lookup (name one) reference,M.lookup (name one) subject) of
      (Just stock,Just owned) -> do
        let defined = operation one == Reserve
            differences = compareObservations defined stock owned ++ semanticChecks one stock ++ semanticChecks one owned
            controls = observationControls defined stock
        pure (Run (name one) differences controls)
      _ -> pure (Run (name one) ["missing stock or owned observation"] [])
    _ -> fail "internal role count"

readStateWord :: NativeObservation -> Word32 -> Word32
readStateWord observation address = case [(ident,base) | (ident,base,size) <- stateBanks, address >= base, address + 4 <= base + size] of
  [(ident,base)] -> readWordAt (regionBytesOf observation ident) (fromIntegral (address - base))
  _ -> error "state address outside synthetic banks"

semanticChecks :: Case -> NativeObservation -> [String]
semanticChecks one observation =
  ["synthetic critical enter/leave/token" | let bytes = regionBytesOf observation (RegionId "receipts"), let count = if operation one == Pair then 2 else if operation one `elem` [Reserve,Release] then 1 else 0, readWordAt bytes 0 /= count || readWordAt bytes 4 /= count || (count /= 0 && readWordAt bytes 8 /= 0x5a5a)]
  ++ ["selector outputs" | operation one == Select, let bytes = regionBytesOf observation (RegionId "output"), let chosen = if selector one == 1 then Ordinary1 else if pool one == Alternate4 && alternateState one `elem` [5,6] then Alternate4 else Ordinary0, B.take 2 bytes /= B.take 2 (word32le (fromIntegral (poolCount chosen))) || B.take 2 (B.drop 2 bytes) /= B.pack [0xa5,0xa5] || readWordAt bytes 4 /= poolBase chosen]
  ++ ["pair failed first must still reserve second" | operation one == Pair, all (== State0d) (states one), all (== Free) (peerStates one), readStateWord observation (poolBase Ordinary1 + 4) /= (if mark one == 1 then 2 else 1)]
  ++ ["pair failed second retains first reservation" | operation one == Pair, all (== Free) (states one), all (== State0d) (peerStates one), readStateWord observation (poolBase (pool one) + 4) /= (if mark one == 1 then 2 else 1)]
  ++ ["pair clears only special mark1" | operation one == Pair, readStateWord observation 0x170306c /= (if mark one == 1 then 0 else mark one)]

observationControls :: Bool -> NativeObservation -> [(String,Bool)]
observationControls defined observation =
  [ ("status", not (null (compareObservations defined observation observation {nativeStatus = MemoryGuard})))
  , ("caller-frame", not (null (compareObservations defined observation observation {nativeRegisters = M.insert Sp (stackPointer-4) (nativeRegisters observation)})))
  , ("selection", not (null (compareObservations defined observation (mutate (RegionId "output")))))
  , ("mutation", not (null (compareObservations defined observation (mutate (RegionId "state")))))
  , ("caller-stack-guard", not (null (compareObservations defined observation (mutate (RegionId "stack")))))
  ]
  where
    mutate ident = observation {nativeMemory = [(key,if key == ident then B.cons (B.head bytes `xor` 1) (B.tail bytes) else bytes) | (key,bytes) <- nativeMemory observation]}

data ConsumerMode = NormalMode | TaggedMode deriving (Eq, Show)
data Route = RouteMarked | RouteCurrent deriving (Eq, Show)

consumerModeRaw :: ConsumerMode -> Word32
consumerModeRaw NormalMode = 0
consumerModeRaw TaggedMode = 1

routeRaw :: Route -> Word32
routeRaw RouteMarked = 1
routeRaw RouteCurrent = 0

routeLabel :: Route -> String
routeLabel RouteMarked = "marked"
routeLabel RouteCurrent = "current"

data ConsumerCase = ConsumerCase
  { consumerName :: String
  , consumerMode :: ConsumerMode
  , consumerRoute :: Route
  , consumerStates :: [Word32]
  , consumerFlags :: [Word32]
  }

data CorrectionCase = CorrectionCase
  { correctionName :: String
  , correctionRoute :: Route
  , correctionSlot :: Int
  , correctionGuard :: Bool
  , correctionStates :: [Word32]
  , correctionFlags :: [Word32]
  }

data CallerCase = CallerCase
  { callerName :: String
  , callerEntry :: Word32
  , callerSize :: Word32
  , callerSelector :: Word32
  , callerConsumer :: Bool
  , callerResult :: Word32
  , callerStates :: [Word32]
  , callerFlags :: [Word32]
  , callerMode :: ConsumerMode
  , callerRoute :: Route
  }

stateFree, stateMarked, stateCurrent, stateRetired, stateOpaque :: Word32
stateFree = 0
stateMarked = 2
stateCurrent = 3
stateRetired = 6
stateOpaque = 0xd

routeAddress, escapedWrite :: Word32
routeAddress = 0x170327c
escapedWrite = 0x16fed2c + 0xffff * 24

secondaryOffset, routeOffset :: Int
secondaryOffset = 0x128
routeOffset = 0x2f8

receiptsSize, logOffset, counterOffset :: Int
receiptsSize = 0x300
logOffset = 0x100
counterOffset = 0x200

setAt :: Int -> a -> [a] -> [a]
setAt index value values = take index values ++ [value] ++ drop (index + 1) values

seven :: String -> [a] -> [a]
seven label values = if length values == 7 then values else error (label ++ " needs seven records")

stateBankBytes :: [Word32] -> [Word32] -> B.ByteString
stateBankBytes states flags = foldl setFlag withStates (zip [0 :: Int ..] flags)
  where
    base = B.replicate 0x248 0xa5
    withStates = foldl (\bytes (slot,state) -> poke bytes (fromIntegral (secondaryOffset + slot * 24 + 4)) state) base (zip [0 :: Int ..] states)
    setFlag bytes (slot,flag) = poke bytes (fromIntegral (secondaryOffset + slot * 24 + 16)) flag

lateBankBytes :: Word32 -> B.ByteString
lateBankBytes route = poke (B.replicate 0x884 0xa5) (fromIntegral routeOffset) route

modeStub :: ConsumerMode -> B.ByteString
modeStub mode = assemble [0xe3a00000 .|. consumerModeRaw mode, 0xe12fff1e]

criticalEnterStub :: B.ByteString
criticalEnterStub = assemble (mov 2 receiptBase ++ [0xe5923000,0xe2833001,0xe5823000] ++ mov 0 0x5a5a ++ [0xe12fff1e])

criticalLeaveStub :: B.ByteString
criticalLeaveStub = assemble (mov 2 receiptBase ++ [0xe5820008,0xe5923004,0xe2833001,0xe5823004,0xe12fff1e])

recordingStub :: Word32 -> B.ByteString
recordingStub ident = assemble
  (mov 2 (receiptBase + fromIntegral counterOffset)
    ++ [0xe5923000]
    ++ mov 1 ident
    ++ mov 0 (receiptBase + fromIntegral logOffset)
    ++ [0xe0800103,0xe5801000,0xe2833001,0xe5823000,0xe12fff1e])

recordingReturnStub :: Word32 -> Word32 -> B.ByteString
recordingReturnStub ident value = assemble
  (mov 2 (receiptBase + fromIntegral counterOffset)
    ++ [0xe5923000]
    ++ mov 1 ident
    ++ mov 0 (receiptBase + fromIntegral logOffset)
    ++ [0xe0800103,0xe5801000,0xe2833001,0xe5823000]
    ++ mov 0 value
    ++ [0xe12fff1e])

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region ident address permission bytes = mkRegion (RegionId ident) (guest address) (B.length bytes) permission bytes

imageSlice :: Scope -> Word32 -> Word32 -> B.ByteString
imageSlice (Scope image _ _) address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - 0xbf1a00)) image)

ownedSlice :: Scope -> Word32 -> Word32 -> B.ByteString
ownedSlice (Scope _ _ elf) address size = B.pack [maybe (error "missing checked ELF byte") id (E.byteAt elf x) | x <- [address .. address + size - 1]]

ownedFunctions :: Scope -> Word32 -> Word32 -> [(GuestAddress, Word32)]
ownedFunctions (Scope _ _ elf) start end =
  [(guest (E.symbolValue symbol),E.symbolSize symbol) | symbol <- E.elfSymbols elf, E.symbolKind symbol == E.FunctionSymbol, E.symbolSize symbol > 0, E.symbolValue symbol >= start, E.symbolValue symbol + E.symbolSize symbol <= end]

initialRegisters :: [(Register, Word32)]
initialRegisters = [(r,seed r) | r <- [R0 .. R12]] ++ [(Sp,stackPointer),(Lr,exitAddress),(Cpsr,0x10)]

consumerCases :: Mode -> [ConsumerCase]
consumerCases Smoke = take 8 fullConsumerCases
consumerCases Qualification = fullConsumerCases

fullConsumerCases :: [ConsumerCase]
fullConsumerCases = normalMatrix ++ normalDuplicates ++ normalOpaque ++ taggedMatrix ++ taggedNoTag ++ taggedPriority ++ taggedDuplicates ++ taggedOpaque
  where
    flags = [0x10,0x0004_0000,0x8000_0000,1,2,4,8]
    normalMatrix =
      [ ConsumerCase
          { consumerName = "consumer-normal-" ++ routeLabel route ++ "-slot-" ++ show slot
          , consumerMode = NormalMode
          , consumerRoute = route
          , consumerStates = seven "normal matrix" (setAt release stateRetired (setAt prior 5 (setAt slot candidate (replicate 7 stateFree))))
          , consumerFlags = flags
          }
      | route <- [RouteCurrent, RouteMarked]
      , slot <- [0 :: Int .. 6]
      , let candidate = if route == RouteCurrent then stateCurrent else stateMarked
      , let prior = (slot + 1) `mod` 7
      , let release = (slot + 2) `mod` 7
      ]
    normalDuplicates =
      [ ConsumerCase ("consumer-normal-duplicate-six-five-" ++ routeLabel route) NormalMode route (seven "duplicate" [6,6,5,5,3,stateOpaque,0x1b]) flags
      | route <- [RouteCurrent, RouteMarked]
      ]
    normalOpaque =
      [ ConsumerCase "consumer-normal-opaque" NormalMode RouteCurrent (seven "opaque" [stateOpaque,0x1b,0x77,0x83,0xffffffff,5,3]) flags
      , ConsumerCase "consumer-normal-opaque-marked" NormalMode RouteMarked (seven "opaque" [stateOpaque,0x1b,0x77,0x83,0xffffffff,5,2]) flags
      ]
    taggedMatrix =
      [ ConsumerCase
          { consumerName = "consumer-tagged-slot-" ++ show slot
          , consumerMode = TaggedMode
          , consumerRoute = RouteCurrent
          , consumerStates = seven "tagged matrix" (setAt oldTwo 5 (setAt oldOne 5 (setAt release stateRetired (setAt slot stateFree (replicate 7 stateFree)))))
          , consumerFlags = setAt slot 0x0004_0000 (setAt release 0x10 [0x8000_0000,1,2,4,8,0x20,0x40])
          }
      | slot <- [0 :: Int .. 6]
      , let release = (slot + 1) `mod` 7
      , let oldOne = (slot + 2) `mod` 7
      , let oldTwo = (slot + 3) `mod` 7
      ]
    taggedNoTag =
      [ ConsumerCase "consumer-tagged-no-tag" TaggedMode RouteCurrent (seven "no tag" [6,5,5,stateOpaque,0x1b,0,0]) [0x10,0,0,0x8000_0000,1,2,4]
      ]
    taggedPriority =
      [ ConsumerCase "consumer-tagged-slot-zero-priority" TaggedMode RouteCurrent (seven "priority" [5,5,5,0,0,0,0]) [0x0004_0000,0x0004_0000,0,0,0,0,0]
      ]
    taggedDuplicates =
      [ ConsumerCase "consumer-tagged-duplicate-six-five" TaggedMode RouteCurrent (seven "tagged duplicate" [6,6,5,5,5,0,0]) [0x10,0x10,0x0004_0000,0,0,0,0]
      ]
    taggedOpaque =
      [ ConsumerCase "consumer-tagged-opaque" TaggedMode RouteCurrent (seven "tagged opaque" [stateOpaque,0x1b,0x77,0x83,0xffffffff,5,5]) [0,0,0,0,0,0x0004_0000,0x10]
      ]

correctionCases :: Mode -> [CorrectionCase]
correctionCases _ = [correctionCase route slot guard | route <- [RouteCurrent, RouteMarked], slot <- [0 :: Int .. 6], guard <- [False,True]]

correctionCase :: Route -> Int -> Bool -> CorrectionCase
correctionCase route slot guard = CorrectionCase
  { correctionName = "correction-" ++ routeLabel route ++ "-slot-" ++ show slot ++ if guard then "-guard" else "-witness"
  , correctionRoute = route
  , correctionSlot = slot
  , correctionGuard = guard
  , correctionStates = seven "correction" (setAt slot candidate (replicate 7 stateFree))
  , correctionFlags = [0x10,0x0004_0000,0x8000_0000,1,2,4,8]
  }
  where
    candidate = if route == RouteCurrent then stateCurrent else stateMarked

correctionExpectedStates :: CorrectionCase -> [Word32]
correctionExpectedStates one = setAt (correctionSlot one) 5 (correctionStates one)

callerCases :: Mode -> [CallerCase]
callerCases Smoke = take 3 fullCallerCases
callerCases Qualification = fullCallerCases

fullCallerCases :: [CallerCase]
fullCallerCases = callerOne ++ callerTwo ++ callerThree
  where
    states = seven "caller" [0,5,3,0,0,0,0]
    flags = replicate 7 0
    callerOne =
      [ CallerCase
          { callerName = "caller-01031c54-selector-" ++ show selector
          , callerEntry = 0x1031c54
          , callerSize = 84
          , callerSelector = selector
          , callerConsumer = selector `elem` [3,4]
          , callerResult = 0
          , callerStates = states
          , callerFlags = flags
          , callerMode = NormalMode
          , callerRoute = RouteCurrent
          }
      | selector <- [3,4,0xe,0]
      ]
    callerTwo =
      [ CallerCase
          { callerName = "caller-01032b40-selector-" ++ show selector
          , callerEntry = 0x1032b40
          , callerSize = 84
          , callerSelector = selector
          , callerConsumer = selector `elem` [5,6]
          , callerResult = 0
          , callerStates = states
          , callerFlags = flags
          , callerMode = NormalMode
          , callerRoute = RouteCurrent
          }
      | selector <- [5,6,0xe,0]
      ]
    callerThree =
      [ CallerCase
          { callerName = "caller-01033344-selector-" ++ show selector ++ "-result-" ++ show result
          , callerEntry = 0x01033344
          , callerSize = 568
          , callerSelector = selector
          , callerConsumer = selector `elem` [7,8] && result == 0
          , callerResult = result
          , callerStates = states
          , callerFlags = flags
          , callerMode = NormalMode
          , callerRoute = RouteCurrent
          }
      | selector <- [7,8,0xe]
      , result <- if selector == 0xe then [0] else [0,1]
      ]

consumerSpec :: Role -> Scope -> ConsumerCase -> CaseSpec
consumerSpec role scope one = base
  { caseId = consumerName one
  , caseEntry = guest 0x103fd88
  , caseRegions =
      [ region "consumer-code" 0x103fd88 ReadExecute (if role == Stock then imageSlice scope 0x103fd88 740 else ownedSlice scope 0x103fd88 740)
      , region "synthetic-mode" 0x103e988 ReadExecute (modeStub (consumerMode one))
      , region "critical-enter" 0x7235d8 ReadExecute criticalEnterStub
      , region "critical-leave" 0x7236b0 ReadExecute criticalLeaveStub
      , region "state" 0x16fec00 ReadWrite (stateBankBytes (seven "consumer states" (consumerStates one)) (seven "consumer flags" (consumerFlags one)))
      , region "late-state" 0x1702f84 ReadWrite (lateBankBytes (routeRaw (consumerRoute one)))
      , region "output" outputBase ReadWrite (B.replicate 16 0)
      , region "receipts" receiptBase ReadWrite (B.replicate receiptsSize 0)
      , region "stack" stackBase ReadWrite (B.replicate 0x1000 0xa5)
      , region "exit" exitAddress ReadExecute (word32le 0xe7f000f0)
      ]
  , caseFunctions = consumerFunctions role scope
  }
  where
    base = CaseSpec
      { caseId = ""
      , caseProvenance = consumerSourceIdentity
      , caseExclusion = "Synthetic secondary-bank consumer; retained mode, critical enter/leave and caller stubs record only. No physical locking, scheduler, timing, IRQ or stall claim."
      , caseEntry = guest 0x103fd88
      , caseExit = guest exitAddress
      , caseRegions = []
      , caseFunctions = []
      , caseInitial = initialRegisters
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = [(R6,seed R6)]
      , caseExpectedMemory = []
      , caseCp14Identity = Nothing
      , caseKernel = Nothing
      }

consumerFunctions :: Role -> Scope -> [(GuestAddress, Word32)]
consumerFunctions role scope =
  [(guest 0x103e988,fromIntegral (B.length (modeStub NormalMode))),(guest 0x7235d8,fromIntegral (B.length criticalEnterStub)),(guest 0x7236b0,fromIntegral (B.length criticalLeaveStub))]
    ++ if role == Stock then [(guest 0x103fd88,740)] else ownedFunctions scope 0x103fd88 0x104006c

correctionSpec :: Role -> Scope -> CorrectionCase -> CaseSpec
correctionSpec role scope one = (consumerSpec role scope consumer)
  { caseId = correctionName one
  , caseEntry = guest 0x103fd88
  , caseRegions = correctionRegions
  , caseFunctions = consumerFunctions role scope
  , caseExpectedStatus = if role == Stock && correctionGuard one then MemoryGuard else Returned
  , caseExclusion = "Original normal path writes through the 0xffff prior-five sentinel when no prior five exists. The stock guard case preserves the refused write; the owned case selects the candidate and skips retiring an absent prior five. Synthetic critical and mode stubs only; no physical locking, scheduler, timing, IRQ or stall claim."
  }
  where
    consumer = ConsumerCase
      { consumerName = correctionName one
      , consumerMode = NormalMode
      , consumerRoute = correctionRoute one
      , consumerStates = correctionStates one
      , consumerFlags = correctionFlags one
      }
    correctionRegions =
      [ region "consumer-code" 0x103fd88 ReadExecute (if role == Stock then imageSlice scope 0x103fd88 740 else ownedSlice scope 0x103fd88 740)
      , region "synthetic-mode" 0x103e988 ReadExecute (modeStub NormalMode)
      , region "critical-enter" 0x7235d8 ReadExecute criticalEnterStub
      , region "critical-leave" 0x7236b0 ReadExecute criticalLeaveStub
      , region "state" 0x16fec00 ReadWrite (stateBankBytes (correctionStates one) (correctionFlags one))
      , region "late-state" 0x1702f84 ReadWrite (lateBankBytes (routeRaw (correctionRoute one)))
      , region "output" outputBase ReadWrite (B.replicate 16 0)
      , region "receipts" receiptBase ReadWrite (B.replicate receiptsSize 0)
      , region "stack" stackBase ReadWrite (B.replicate 0x1000 0xa5)
      , region "exit" exitAddress ReadExecute (word32le 0xe7f000f0)
      ] ++ [region "escaped-write-witness" escapedWrite ReadWrite (word32le 0xa5a5a5a5) | role == Owned || not (correctionGuard one)]

callerSpec :: Role -> Scope -> CallerCase -> CaseSpec
callerSpec role scope one = base
  { caseId = callerName one
  , caseEntry = guest (callerEntry one)
  , caseRegions =
      [ region "caller-code" (callerEntry one) ReadExecute (imageSlice scope (callerEntry one) (callerSize one))
      , region "consumer-code" 0x103fd88 ReadExecute (if role == Stock then imageSlice scope 0x103fd88 740 else ownedSlice scope 0x103fd88 740)
      , region "synthetic-mode" 0x103e988 ReadExecute (modeStub (callerMode one))
      , region "critical-enter" 0x7235d8 ReadExecute criticalEnterStub
      , region "critical-leave" 0x7236b0 ReadExecute criticalLeaveStub
      , region "state" 0x16fec00 ReadWrite (callerStateBytes one)
      , region "late-state" 0x1702f84 ReadWrite (lateBankBytes (routeRaw (callerRoute one)))
      , region "output" outputBase ReadWrite (B.replicate 16 0)
      , region "receipts" receiptBase ReadWrite (B.replicate receiptsSize 0)
      , region "stack" stackBase ReadWrite (B.replicate 0x1000 0xa5)
      , region "exit" exitAddress ReadExecute (word32le 0xe7f000f0)
      ] ++ callerStubRegions one
  , caseFunctions = callerFunctions role scope one
  }
  where
    base = CaseSpec
      { caseId = ""
      , caseProvenance = consumerSourceIdentity
      , caseExclusion = "Direct retained callback entry points 0x01031c54, 0x01032b40 and 0x01033344 are invoked directly; queue posting and deferred delivery, selector changes between posting and callback, scheduler, downstream commands and result publication are outside this bounded case and are not claimed. Downstream calls are recording stubs."
      , caseEntry = guest (callerEntry one)
      , caseExit = guest exitAddress
      , caseRegions = []
      , caseFunctions = []
      , caseInitial = initialRegisters
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = [(R6,seed R6)]
      , caseExpectedMemory = []
      , caseCp14Identity = Nothing
      , caseKernel = Nothing
      }

callerStateBytes :: CallerCase -> B.ByteString
callerStateBytes one = poke (poke (stateBankBytes (seven "caller states" (callerStates one)) (seven "caller flags" (callerFlags one))) 0x44 0) 0x4c (callerSelector one)

callerStubRegions :: CallerCase -> [CaseRegion]
callerStubRegions one =
  [ region "caller-downstream-0102fd28" 0x102fd28 ReadExecute (recordingStub 0x102fd28)
  , region "caller-result" (callerResultAddress one) ReadExecute (recordingStub (callerResultAddress one))
  ] ++ if callerEntry one == 0x01033344
    then
      [ region "caller-downstream-00edb590" 0xedb590 ReadExecute (recordingReturnStub 0xedb590 0x12345678)
      , region "caller-downstream-00ef773c" 0xef773c ReadExecute (recordingReturnStub 0xef773c (callerResult one))
      , region "caller-downstream-01041d60" 0x1041d60 ReadExecute (recordingStub 0x1041d60)
      ]
    else []

callerFunctions :: Role -> Scope -> CallerCase -> [(GuestAddress, Word32)]
callerFunctions role scope one =
  [(guest (callerEntry one),callerSize one)]
    ++ consumerFunctions role scope
    ++ [(guest 0x102fd28,fromIntegral (B.length (recordingStub 0x102fd28))),(guest (callerResultAddress one),fromIntegral (B.length (recordingStub (callerResultAddress one))))]
    ++ if callerEntry one == 0x01033344
      then
        [ (guest 0xedb590,fromIntegral (B.length (recordingReturnStub 0xedb590 0x12345678)))
        , (guest 0xef773c,fromIntegral (B.length (recordingReturnStub 0xef773c (callerResult one))))
        , (guest 0x1041d60,fromIntegral (B.length (recordingStub 0x1041d60)))
        ]
      else []

callerResultAddress :: CallerCase -> Word32
callerResultAddress one = case callerEntry one of
  0x1031c54 -> 0x103a824
  0x1032b40 -> 0x103a9d4
  _ -> 0x103a8fc

callerExpectedLog :: CallerCase -> [Word32]
callerExpectedLog one
  | callerEntry one `elem` [0x1031c54,0x1032b40] =
      if callerConsumer one then [0x102fd28,callerResultAddress one]
      else if callerSelector one == 0xe then [callerResultAddress one]
      else []
  | otherwise =
      [0xedb590,0xef773c]
        ++ (if callerConsumer one then [0x102fd28] else if callerSelector one `elem` [7,8] then [0x1041d60] else [])
        ++ [0x103a8fc]

chunksOf :: Int -> [a] -> [[a]]
chunksOf _ [] = []
chunksOf size values = let (selected,rest) = splitAt size values in selected : chunksOf size rest

observeBoth :: FilePath -> FilePath -> FilePath -> Scope -> (Role -> a -> CaseSpec) -> [a] -> IO (M.Map String NativeObservation, M.Map String NativeObservation)
observeBoth runner cache directory _scope spec selected = do
  observations <- forM [Stock,Owned] $ \role -> do
    batches <- forM (zip [0 :: Int ..] (chunksOf 32 (map (spec role) selected))) $ \(index,chunk) -> do
      batch <- writeBatch (directory </> show role </> ("batch-" ++ show index)) chunk
      (_,observed) <- cachedObservedBatch cache runner batch
      pure observed
    pure (M.fromList [(caseIdText ident,o) | (ident,o) <- concat batches])
  case observations of
    [stock,owned] -> pure (stock,owned)
    _ -> fail "internal role count"

runConsumerCases :: Mode -> FilePath -> FilePath -> FilePath -> Scope -> IO [Run]
runConsumerCases mode runner cache directory scope = do
  let selected = consumerCases mode
  (stockBy,ownedBy) <- observeBoth runner cache directory scope (\role one -> consumerSpec role scope one) selected
  forM selected $ \one -> case (M.lookup (consumerName one) stockBy,M.lookup (consumerName one) ownedBy) of
    (Just stock,Just owned) -> do
      let differences = compareObservations False stock owned ++ consumerChecks one stock ++ consumerChecks one owned
      pure (Run (consumerName one) differences (consumerControls stock))
    _ -> pure (Run (consumerName one) ["missing stock or owned observation"] [])

runCorrectionCases :: Mode -> FilePath -> FilePath -> FilePath -> Scope -> IO [Run]
runCorrectionCases mode runner cache directory scope = do
  let selected = correctionCases mode
  (stockBy,ownedBy) <- observeBoth runner cache directory scope (\role one -> correctionSpec role scope one) selected
  forM selected $ \one -> case (M.lookup (correctionName one) stockBy,M.lookup (correctionName one) ownedBy) of
    (Just stock,Just owned) -> do
      let differences = correctionChecks one stock owned
      pure (Run (correctionName one) differences (correctionControls one stock owned))
    _ -> pure (Run (correctionName one) ["missing stock or owned observation"] [])

runCallerCases :: Mode -> FilePath -> FilePath -> FilePath -> Scope -> IO [Run]
runCallerCases mode runner cache directory scope = do
  let selected = callerCases mode
  (stockBy,ownedBy) <- observeBoth runner cache directory scope (\role one -> callerSpec role scope one) selected
  forM selected $ \one -> case (M.lookup (callerName one) stockBy,M.lookup (callerName one) ownedBy) of
    (Just stock,Just owned) -> do
      let differences = compareObservations False stock owned ++ callerChecks one stock ++ callerChecks one owned
      pure (Run (callerName one) differences (consumerControls stock))
    _ -> pure (Run (callerName one) ["missing stock or owned observation"] [])

consumerControls :: NativeObservation -> [(String,Bool)]
consumerControls observation =
  [ ("status", not (null (compareObservations False observation observation {nativeStatus = MemoryGuard})))
  , ("secondary", not (null (compareObservations False observation (mutateRegion (RegionId "state") observation))))
  , ("route", not (null (compareObservations False observation (mutateRegion (RegionId "late-state") observation))))
  , ("receipts", not (null (compareObservations False observation (mutateRegion (RegionId "receipts") observation))))
  , ("caller-frame", not (null (compareObservations False observation observation {nativeRegisters = M.insert Sp (stackPointer - 4) (nativeRegisters observation)})))
  , ("caller-stack-guard", not (null (compareObservations False observation (mutateRegion (RegionId "stack") observation))))
  ]

mutateRegion :: RegionId -> NativeObservation -> NativeObservation
mutateRegion ident observation = observation { nativeMemory = [(key,if key == ident then B.cons (B.head bytes `xor` 1) (B.tail bytes) else bytes) | (key,bytes) <- nativeMemory observation] }

stateWordAt :: NativeObservation -> Word32 -> Word32
stateWordAt observation address = readWordAt (regionBytesOf observation (RegionId "state")) (fromIntegral (address - 0x16fec00))

lateWordAt :: NativeObservation -> Word32 -> Word32
lateWordAt observation address = readWordAt (regionBytesOf observation (RegionId "late-state")) (fromIntegral (address - 0x1702f84))

receiptWordAt :: NativeObservation -> Int -> Word32
receiptWordAt observation offset = readWordAt (regionBytesOf observation (RegionId "receipts")) offset

secondaryState :: NativeObservation -> Int -> Word32
secondaryState observation slot = stateWordAt observation (0x16fed2c + fromIntegral slot * 24)

secondaryFlag :: NativeObservation -> Int -> Word32
secondaryFlag observation slot = stateWordAt observation (0x16fed38 + fromIntegral slot * 24)

secondaryBytesOf :: NativeObservation -> B.ByteString
secondaryBytesOf observation = B.take 168 (B.drop secondaryOffset (regionBytesOf observation (RegionId "state")))

routeValue :: NativeObservation -> Word32
routeValue observation = lateWordAt observation routeAddress

outsideSecondaryClean :: NativeObservation -> Bool
outsideSecondaryClean observation =
  let bytes = regionBytesOf observation (RegionId "state")
  in B.length bytes == 0x248 && B.all (== 0xa5) (B.take secondaryOffset bytes) && B.all (== 0xa5) (B.drop (secondaryOffset + 168) bytes)

lateStateClean :: NativeObservation -> Bool
lateStateClean observation =
  let bytes = regionBytesOf observation (RegionId "late-state")
  in B.length bytes == 0x884 && B.all (== 0xa5) (B.take routeOffset bytes) && B.all (== 0xa5) (B.drop (routeOffset + 4) bytes)

callerOutsideSecondary :: B.ByteString -> B.ByteString
callerOutsideSecondary bytes = B.take 0x44 bytes <> B.replicate 4 0 <> B.drop 0x48 (B.take secondaryOffset bytes) <> B.drop (secondaryOffset + 168) bytes

consumerChecks :: ConsumerCase -> NativeObservation -> [String]
consumerChecks one observation =
  ["status " ++ show (nativeStatus observation) | nativeStatus observation /= Returned]
  ++ ["critical enter/leave " ++ show (receiptWordAt observation 0,receiptWordAt observation 4,receiptWordAt observation 8) | receiptWordAt observation 0 /= 1 || receiptWordAt observation 4 /= 1 || receiptWordAt observation 8 /= 0x5a5a]
  ++ ["route changed to 0x" ++ showHex (routeValue observation) | routeValue observation /= routeRaw (consumerRoute one)]
  ++ ["normal mode changed flags: " ++ show [secondaryFlag observation slot | slot <- [0 .. 6]] | consumerMode one == NormalMode, any (\slot -> secondaryFlag observation slot /= consumerFlags one !! slot) [0 .. 6]]
  ++ ["wrote outside secondary: " ++ show (regionBytesOf observation (RegionId "state")) | not (outsideSecondaryClean observation)]
  ++ ["wrote outside route: " ++ show (regionBytesOf observation (RegionId "late-state")) | not (lateStateClean observation)]
  ++ ["callee-saved " ++ registerLabel r | r <- [R4 .. R11], registerValue observation r /= seed r]
  ++ ["caller stack" | registerValue observation Sp /= stackPointer]
  ++ ["caller stack guard" | not (stackGuards observation)]

correctionChecks :: CorrectionCase -> NativeObservation -> NativeObservation -> [String]
correctionChecks one stock owned =
  stockChecks ++ ownedChecks
  where
    witness ident observation = case lookup (RegionId ident) (nativeMemory observation) of
      Just bytes -> bytes
      Nothing -> B.empty
    stockChecks
      | correctionGuard one =
          [ "missing-prior stock fault differs: status=" ++ show (nativeStatus stock)
            ++ " pc=0x" ++ showHex (registerValue stock Pc)
            ++ " r3=0x" ++ showHex (registerValue stock R3)
            ++ " expected-pc=0x1040064 expected-r3=0x" ++ showHex (escapedWrite - 0xf8)
          | nativeStatus stock /= MemoryGuard || registerValue stock Pc /= 0x1040064 || registerValue stock R3 /= escapedWrite - 0xf8
          ]
          ++ ["stock candidate state " ++ show (secondaryState stock (correctionSlot one)) | secondaryState stock (correctionSlot one) /= 5]
          ++ ["stock flags changed" | any (\slot -> secondaryFlag stock slot /= correctionFlags one !! slot) [0 .. 6]]
      | otherwise =
          [ "stock witness status=" ++ show (nativeStatus stock) ++ " bytes=" ++ show (witness "escaped-write-witness" stock)
          | nativeStatus stock /= Returned || witness "escaped-write-witness" stock /= word32le 6
          ]
          ++ ["stock candidate state " ++ show (secondaryState stock (correctionSlot one)) | secondaryState stock (correctionSlot one) /= 5]
          ++ ["stock flags changed" | any (\slot -> secondaryFlag stock slot /= correctionFlags one !! slot) [0 .. 6]]
    ownedChecks =
      ["owned status " ++ show (nativeStatus owned) | nativeStatus owned /= Returned]
      ++ ["owned secondary bank " ++ show (secondaryBytesOf owned) | secondaryBytesOf owned /= expectedCorrectionBank one]
      ++ ["owned states " ++ show [secondaryState owned slot | slot <- [0 .. 6]] | any (\slot -> secondaryState owned slot /= correctionExpectedStates one !! slot) [0 .. 6]]
      ++ ["owned flags " ++ show [secondaryFlag owned slot | slot <- [0 .. 6]] | any (\slot -> secondaryFlag owned slot /= correctionFlags one !! slot) [0 .. 6]]
      ++ ["owned escaped write " ++ show (witness "escaped-write-witness" owned) | witness "escaped-write-witness" owned /= word32le 0xa5a5a5a5]
      ++ ["owned route changed" | routeValue owned /= routeRaw (correctionRoute one)]
      ++ ["owned critical enter/leave" | receiptWordAt owned 0 /= 1 || receiptWordAt owned 4 /= 1 || receiptWordAt owned 8 /= 0x5a5a]
      ++ ["owned wrote outside secondary" | not (outsideSecondaryClean owned)]
      ++ ["owned wrote outside route" | not (lateStateClean owned)]
      ++ ["owned callee-saved " ++ registerLabel r | r <- [R4 .. R11], registerValue owned r /= seed r]
      ++ ["owned caller stack" | registerValue owned Sp /= stackPointer]
      ++ ["owned stack guard" | not (stackGuards owned)]

expectedCorrectionBank :: CorrectionCase -> B.ByteString
expectedCorrectionBank one = B.concat
  [ word32le 0xa5a5a5a5
    <> word32le (if slot == correctionSlot one then 5 else correctionStates one !! slot)
    <> word32le 0xa5a5a5a5
    <> word32le 0xa5a5a5a5
    <> word32le (correctionFlags one !! slot)
    <> word32le 0xa5a5a5a5
  | slot <- [0 :: Int .. 6]
  ]

mutateStateWord :: NativeObservation -> Int -> Word32 -> NativeObservation
mutateStateWord observation offset value = observation { nativeMemory = [(key,if key == RegionId "state" then poke bytes (fromIntegral offset) value else bytes) | (key,bytes) <- nativeMemory observation] }

mutateLateWord :: NativeObservation -> Int -> Word32 -> NativeObservation
mutateLateWord observation offset value = observation { nativeMemory = [(key,if key == RegionId "late-state" then poke bytes (fromIntegral offset) value else bytes) | (key,bytes) <- nativeMemory observation] }

correctionControls :: CorrectionCase -> NativeObservation -> NativeObservation -> [(String,Bool)]
correctionControls one stock owned =
  [ ("status", not (null (correctionChecks one stock owned { nativeStatus = MemoryGuard })))
  , ("unknown-word-0", not (null (correctionChecks one stock (mutateStateWord owned secondaryOffset 0))))
  , ("unknown-word-8", not (null (correctionChecks one stock (mutateStateWord owned (secondaryOffset + 8) 0))))
  , ("unknown-word-12", not (null (correctionChecks one stock (mutateStateWord owned (secondaryOffset + 12) 0))))
  , ("unknown-word-20", not (null (correctionChecks one stock (mutateStateWord owned (secondaryOffset + 20) 0))))
  , ("candidate-state", not (null (correctionChecks one stock (mutateStateWord owned (secondaryOffset + correctionSlot one * 24 + 4) 0))))
  , ("candidate-flag", not (null (correctionChecks one stock (mutateStateWord owned (secondaryOffset + correctionSlot one * 24 + 16) 0))))
  , ("route", not (null (correctionChecks one stock (mutateLateWord owned routeOffset (routeRaw (correctionRoute one) `xor` 1)))))
  , ("witness", not (null (correctionChecks one stock (mutateRegion (RegionId "escaped-write-witness") owned))))
  , ("receipts", not (null (correctionChecks one stock (mutateRegion (RegionId "receipts") owned))))
  , ("stack", not (null (correctionChecks one stock (mutateRegion (RegionId "stack") owned))))
  ]

callerChecks :: CallerCase -> NativeObservation -> [String]
callerChecks one observation =
  ["status " ++ show (nativeStatus observation) | nativeStatus observation /= Returned]
  ++ ["call log counter " ++ show counter | counter /= length expected]
  ++ ["call log " ++ show logged | take counter logged /= expected]
  ++ ["critical enter/leave " ++ show (receiptWordAt observation 0,receiptWordAt observation 4,receiptWordAt observation 8) | (if callerConsumer one then any (/= 1) [receiptWordAt observation 0,receiptWordAt observation 4] || receiptWordAt observation 8 /= 0x5a5a else any (/= 0) [receiptWordAt observation 0,receiptWordAt observation 4,receiptWordAt observation 8])]
  ++ ["consumer path effects " ++ show (secondaryBytesOf observation) | (if callerConsumer one then secondaryBytesOf observation == initialSecondary else secondaryBytesOf observation /= initialSecondary)]
  ++ ["route changed to 0x" ++ showHex (routeValue observation) | routeValue observation /= routeRaw (callerRoute one)]
  ++ ["wrote outside secondary" | callerOutsideSecondary (regionBytesOf observation (RegionId "state")) /= callerOutsideSecondary (callerStateBytes one)]
  ++ ["wrote outside route" | not (lateStateClean observation)]
  ++ ["callee-saved " ++ registerLabel r | r <- [R4 .. R11], registerValue observation r /= seed r]
  ++ ["caller stack" | registerValue observation Sp /= stackPointer]
  ++ ["caller stack guard" | not (stackGuards observation)]
  where
    expected = callerExpectedLog one
    logged = [receiptWordAt observation (logOffset + 4 * index) | index <- [0 .. counter - 1]]
    counter = fromIntegral (receiptWordAt observation counterOffset)
    initialSecondary = B.take 168 (B.drop secondaryOffset (callerStateBytes one))

showHex :: Word32 -> String
showHex value = go (8 :: Int) value ""
  where
    digits = "0123456789abcdef"
    go 0 _ acc = acc
    go count current acc = go (count - 1) (current `shiftR` 4) (digits !! fromIntegral (current .&. 0xf) : acc)
