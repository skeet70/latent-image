{-# LANGUAGE OverloadedStrings #-}
module Replacement.AfResult.PoolSuite
  ( Scope, Run (..), Mode (..), readScope, runSuite, compareObservations, sourceIdentity ) where

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

data Scope = Scope B.ByteString E.Elf

data Run = Run
  { runName :: String, runDifferences :: [String], runControls :: [(String, Bool)] }

instance NFData Run where
  rnf (Run ident differences controls) = rnf (ident,differences,controls)

sourceIdentity :: String
sourceIdentity = "x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; image5 SHA256 3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a base00bf1a00; decomp/firmwares/xm1-150/app/capture/pipeline/tu_0103b1e8.c:2218-2805 NONMATCHING guide; tu_0102cfc4.c:1195 retained caller0102f1b0; image BL0102f1c4 and0102f1d4 to0103fb50"

ranges :: [(Word32, Word32)]
ranges = [(0x103f190, 460), (0x103fb50, 568)]

entries :: [(String, Word32)]
entries = [("xm1_af_select", 0x103f190), ("xm1_af_rotate", 0x103f1fc), ("xm1_af_reserve", 0x103fb50), ("xm1_af_release", 0x103fd38)]

inside :: Word32 -> Word32 -> Bool
inside address size = any (\(base, width) -> address >= base && toInteger address + toInteger size <= toInteger base + toInteger width) ranges

readScope :: FilePath -> FilePath -> IO Scope
readScope imagePath elfPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  forM_ entries $ \(symbol, address) -> case filter ((== symbol) . E.symbolName) (E.elfSymbols elf) of
    [entry] | E.symbolValue entry == address, E.symbolKind entry == E.FunctionSymbol, E.symbolSize entry > 0, inside address (E.symbolSize entry), E.InSection index <- E.symbolSection entry, index < length (E.elfSections elf), index >= 0 -> do
      let section = E.elfSections elf !! index
      unless (E.sectionAllocated section && E.sectionExecutable section && not (E.sectionWritable section)) (fail (symbol ++ " is not executable read-only allocated code"))
    _ -> fail ("missing or misbound owned function " ++ symbol)
  forM_ (filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)) $ \section ->
    unless (E.sectionType section == 1 && not (E.sectionWritable section) && inside (E.sectionAddress section) (E.sectionSize section)) (fail "owned allocated section escapes sparse ranges")
  forM_ ranges $ \(base, size) -> unless (all (\a -> E.byteAt elf a /= Nothing) [base .. base + size - 1]) (fail "owned sparse range has missing bytes")
  let wordsAt :: Word32 -> Word32 -> [Word32]
      wordsAt address count = [readWordAt image (fromIntegral (address - 0xbf1a00 + 4 * i)) | i <- [0 .. count - 1]]
  unless (wordsAt 0x102f1b0 11 == [0xe92d48f0,0xe28db014,0xe24dde12,0xe30f0fff,0xe3a01000,0xeb004261,0xe1a06000,0xe30f0fff,0xe3a01001,0xeb00425d,0xe1a04000]) (fail "retained pair caller bytes differ")
  pure (Scope image elf)

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
caseFor role (Scope image elf) one = CaseSpec
  { caseId = name one
  , caseProvenance = sourceIdentity
  , caseExclusion = "Explicit synthetic pool records, unknown words, counter/mark/policy globals and external services. No captured globals, concurrency, physical critical locking, timing, scheduler or stall qualification. Full retained0102f1b0 caller bytes; all non-owned calls are synthetic."
  , caseEntry = guest entry
  , caseExit = guest exitAddress
  , caseRegions = codeRegions ++ [region "retained-pair-caller" 0x102f1b0 ReadExecute (slice 0x102f1b0 0x1d0)] ++ [region ("synthetic-interface-" ++ show a) a ReadExecute bytes | (a,bytes) <- packedStubs] ++ [region (regionName ident) base ReadWrite (B.take (fromIntegral size) (B.drop (fromIntegral (base - 0x16fec00)) stateBytes)) | (ident,base,size) <- stateBanks] ++ [region "output" outputBase ReadWrite (B.replicate 16 0xa5),region "receipts" receiptBase ReadWrite (B.replicate 16 0),region "stack" stackBase ReadWrite (B.replicate 0x1000 0xa5),region "exit" exitAddress ReadExecute (word32le 0xe7f000f0)]
  , caseFunctions = [(guest a,z) | (a,z) <- functions ++ [(0x102f1b0,0x1d0)] ++ [(a,fromIntegral (length body * 4)) | (a,body) <- stubs]]
  , caseInitial = [(r,maybe (seed r) id (lookup r args)) | r <- [R0 .. R12]] ++ [(Sp,stackPointer),(Lr,exitAddress),(Cpsr,0x10)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(r,seed r) | r <- [R4 .. R11]] ++ [(Sp,stackPointer)]
  , caseExpectedMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    region ident address perm bytes = mkRegion (RegionId ident) (guest address) (B.length bytes) perm bytes
    slice :: Word32 -> Word32 -> B.ByteString
    slice address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - 0xbf1a00)) image)
    stateBytes = stateRegion one
    codeRegions = [region ("owned-range-" ++ show a) a ReadExecute bytes | (a,z) <- ranges, let bytes = if role == Stock then slice a z else B.pack [maybe (error "missing checked ELF byte") id (E.byteAt elf x) | x <- [a .. a+z-1]]]
    functions = if role == Stock then [(0x103f190,108),(0x103f1fc,352),(0x103fb50,488),(0x103fd38,80)] else [(E.symbolValue s,E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0, inside (E.symbolValue s) (E.symbolSize s)]
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
runSuite mode runner cache directory scope =
  fmap concat $ forM (zip [0 :: Int ..] (chunks (cases mode))) $ \(index,selected) ->
    runCases runner cache (directory </> ("batch-" ++ show index)) scope selected >>= evaluate . force
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
