{-# LANGUAGE OverloadedStrings #-}
-- | Stock-against-owned comparison of the Power record helpers that are leaves
-- over ordinary memory.
--
-- A case stages the memory the stock body reads from its own disassembly and
-- the decomp C definition, enters the stock extent and the owned export with
-- the same registers, and compares the declared result, every staged byte, the
-- caller frame, both write sets and the executed instruction count. The
-- classification that selected the rows is in the table module.
module Replacement.PowerLifecycle.RecordHelperSuite
  ( runSuite
  , sourceIdentity
  , controlIdentities
  , planProblems
  , ControlResult (..)
  ) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Bits (shiftL, shiftR, testBit, xor, (.&.), (.|.))
import Data.Char (isAsciiLower, isDigit)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.PowerLifecycle.RecordHelperTable
import System.FilePath ((</>))

sourceIdentity :: String
sourceIdentity = helperTableIdentity ++ "; image4 SHA256 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

data Role = Stock | Owned deriving (Eq, Show)

-- The admitted layout. The owned bank is [0x00100000,0x00180000); every staged
-- region is outside it.
stopAddress, stackBase, stackLimit, stackPointer :: Word32
stopAddress = 0x00080000
stackBase = 0x00090000
stackLimit = 0x000a0000
stackPointer = 0x00098000

dataSize, configBase, configSize :: Word32
dataSize = 0x00040000
configBase = 0x00240000
configSize = 0x00100000

argBase, argStride, argSize :: Word32
argBase = 0x00340000
argStride = 0x00010000
argSize = 0x00010000

imageBase :: Image -> Word32
imageBase Image4 = 0x0061c400
imageBase Image5 = 0x00bf1a00

imageBytes :: B.ByteString -> B.ByteString -> Image -> B.ByteString
imageBytes image4 image5 image = if image == Image4 then image4 else image5

imageOf :: Word32 -> Image
imageOf address = if address >= 0x0061c400 && address < 0x0089ec00 then Image4 else Image5

-- | Deterministic staged content. Pattern 3 is the address-derived ramp, so a
-- split of one page keeps every word's value.
wordAt :: Int -> Word32 -> Word32
wordAt 0 _ = 0
wordAt 1 _ = 0xffffffff
wordAt 2 address = (address * 0x9e3779b9 + 0x5a5a5a5a) `xor` (address `shiftR` 7)
wordAt 3 address = address `shiftR` 2
wordAt _ _ = 0

fill :: Int -> Word32 -> Word32 -> B.ByteString
fill pattern base size =
  B.concat [word32le (wordAt pattern (base + 4 * index)) | index <- [0 .. size `div` 4 - 1]]

-- | The stock image is the initial content of a staged in-image word; RAM is
-- the case pattern.
stagedBytes :: B.ByteString -> B.ByteString -> Image -> Int -> Word32 -> Word32 -> B.ByteString
stagedBytes image4 image5 image pattern base size =
  B.concat [wordAtAddress address | address <- [base, base + 4 .. base + size - 4]]
  where
    bytes = imageBytes image4 image5 image
    imageEnd = imageBase image + fromIntegral (B.length bytes)
    wordAtAddress address
      | address >= imageBase image && address + 4 <= imageEnd = B.take 4 (B.drop (fromIntegral (address - imageBase image)) bytes)
      | otherwise = word32le (wordAt pattern address)

data Plan = Plan {planRegions :: [(RegionId, Word32, B.ByteString)], planStack :: B.ByteString}

-- | Cuts @[base,base+size)@ into the intervals that no mapped stock extent
-- covers. Code and data cannot share one region because the runner has no
-- read-write-execute permission.
splitAround :: [(Word32, Word32)] -> (Word32, Word32) -> [(Word32, Word32)]
splitAround extents (base, size) =
  [ (start, end - start)
  | (start, end) <- foldl cut [(base, base + size)] extents
  , end > start
  ]
  where
    cut intervals (extentBase, extentSize) = concatMap (\interval -> cutOut interval (extentBase, extentBase + extentSize)) intervals
    cutOut (intervalStart, intervalEnd) (cutStart, cutEnd)
      | cutEnd <= intervalStart || cutStart >= intervalEnd = [(intervalStart, intervalEnd)]
      | otherwise = [(intervalStart, cutStart) | intervalStart < cutStart] ++ [(cutEnd, intervalEnd) | cutEnd < intervalEnd]

-- | The staged layout: every page split around the mapped stock extents, each
-- pointer target, each pointer argument buffer, and the caller frame.
planLayout :: Helper -> [(String, Word32, Word32)]
planLayout helper =
  [ ("data-" ++ hexWord32 start, start, length')
  | (base, size) <- helperDataPages helper
  , (start, length') <- splitAround (helperStockExtents helper) (base, size)
  ]
  ++ [ (targetName target, target, targetSize target)
     | target <- nub (map snd (helperPointerSlots helper))
     ]
  ++ [ ("arg-" ++ registerLabel register, bufferBase register, argSize)
     | register <- helperPointerArguments helper
     ]
  ++ [("stack", stackBase, stackLimit - stackBase)]

planFor :: B.ByteString -> B.ByteString -> Helper -> (Int, [Word32]) -> Plan
planFor image4 image5 helper (pattern, _) = Plan regions stack
  where
    regions =
      [ (RegionId ident, base, poked base (content ident base size))
      | (ident, base, size) <- planLayout helper
      , ident /= "stack"
      ]
    stack = poked stackBase (B.replicate (fromIntegral (stackLimit - stackBase)) 0xa5)
    content ident base size
      | ident == targetName configBase = fill pattern base size
      | "arg-" `prefixOf` ident = fill pattern base size
      | otherwise = stagedBytes image4 image5 (helperImage helper) pattern base size
    poked base bytes = foldl (\current (slot, value) -> if slot >= base && slot + 4 <= base + fromIntegral (B.length current) then poke (fromIntegral (slot - base)) value current else current) bytes (helperPointerSlots helper ++ helperStagedWords helper)

targetName :: Word32 -> String
targetName target = if target == configBase then "slot-config" else "slot-data"

targetSize :: Word32 -> Word32
targetSize target = if target == configBase then configSize else dataSize

prefixOf :: String -> String -> Bool
prefixOf prefix value = take (length prefix) value == prefix

poke :: Int -> Word32 -> B.ByteString -> B.ByteString
poke offset value bytes = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

bufferBase :: Register -> Word32
bufferBase register = argBase + fromIntegral (fromEnum register) * argStride

-- | One function extent of the owned image.
type Span = (Word32, Word32)

-- | Every function extent the owned image declares, deduplicated where the
-- linker folded two exports onto one body.
ownedSpans :: E.Elf -> [Span]
ownedSpans elf = nub
  [ (E.symbolValue symbol, E.symbolSize symbol)
  | symbol <- E.elfSymbols elf
  , E.symbolKind symbol == E.FunctionSymbol
  , E.symbolSize symbol > 0
  ]

spanHolding :: [Span] -> Word32 -> Maybe Span
spanHolding spans address = case [extent | extent@(base, size) <- spans, base <= address, address - base < size] of
  (one : _) -> Just one
  [] -> Nothing

-- | A branch or branch-with-link word in the ARM instruction set, including the
-- unconditional BLX immediate form.
isBranch :: Word32 -> Bool
isBranch word = (word `shiftR` 25) .&. 7 == 5

branchDisplacement :: Word32 -> Word32
branchDisplacement word = (signExtend24 (word .&. 0xffffff) `shiftL` 2) + halfword
  where
    halfword = if word `shiftR` 28 == 0xf && testBit word 24 then 2 else 0

signExtend24 :: Word32 -> Word32
signExtend24 value = if testBit value 23 then value .|. 0xff000000 else value

-- | Direct branch edges between owned spans. An owned export is a wrapper
-- around a monomorphised context call, so the runner has to admit the spans the
-- body reaches, not only the export's own extent; an indirect call leaves no
-- edge and the runner reports it as an unknown call.
ownedCalls :: E.Elf -> [Span] -> [(Span, Span)]
ownedCalls elf spans =
  [ (source, destination)
  | section <- E.elfSections elf
  , E.sectionAllocated section
  , E.sectionExecutable section
  , E.sectionSize section >= 4
  , offset <- [0, 4 .. E.sectionSize section - 4]
  , let here = E.sectionAddress section + offset
  , Just word <- [E.wordAt elf here]
  , isBranch word
  , let target = here + 8 + branchDisplacement word
  , Just source <- [spanHolding spans here]
  , Just destination <- [spanHolding spans target]
  ]

-- | The direct-branch closure of every owned span, keyed by its base address.
ownedClosures :: E.Elf -> [Span] -> M.Map Word32 [Span]
ownedClosures elf spans = M.fromList [(base, walk [start] []) | (base, _) <- spans, start <- [spanAt base]]
  where
    spanAt base = case [extent | extent@(spanBase, _) <- spans, spanBase == base] of
      (one : _) -> one
      [] -> (base, 0)
    successors = M.fromListWith (++) [(source, [destination]) | (source, destination) <- ownedCalls elf spans]
    walk [] seen = reverse seen
    walk (extent : rest) seen
      | extent `elem` seen = walk rest seen
      | otherwise = walk (M.findWithDefault [] extent successors ++ rest) (extent : seen)

-- | Two extents share a byte.
overlapping :: [Span] -> Bool
overlapping [] = False
overlapping ((base, size) : rest) =
  any (\(other, otherSize) -> base < other + otherSize && other < base + size) rest || overlapping rest

symbolAddressOf :: E.Elf -> String -> Word32
symbolAddressOf elf name = case [E.symbolValue symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol, E.symbolSize symbol > 0] of
  [value] -> value
  _ -> error ("admitted symbol disappeared: " ++ name)

-- | The owned export address of one compared helper.
ownedEntry :: E.Elf -> Helper -> Word32
ownedEntry elf helper = symbolAddressOf elf ("xm1_power_record_helper_" ++ helperAddress helper)

codeRegions :: Role -> B.ByteString -> B.ByteString -> E.Elf -> Helper -> [CaseRegion]
codeRegions role image4 image5 elf helper =
  case role of
    Stock ->
      [ mkRegion (RegionId ("stock-" ++ hexWord32 address)) (GuestAddress (fromIntegral address)) (fromIntegral size) ReadExecute (slice (imageOf address) address size)
      | (address, size) <- helperStockExtents helper
      ]
    Owned ->
      [ mkRegion (RegionId ("owned-" ++ hexWord32 (E.sectionAddress section))) (GuestAddress (fromIntegral (E.sectionAddress section))) (fromIntegral (E.sectionSize section)) (if E.sectionWritable section then ReadWrite else ReadExecute) (ownedBytes section)
      | section <- E.elfSections elf
      , E.sectionAllocated section
      , E.sectionSize section > 0
      ]
  where
    slice image address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image4 image5 image))
    ownedBytes section
      | E.sectionType section == 8 = B.replicate (fromIntegral (E.sectionSize section)) 0
      | otherwise = B.pack [maybe (error "admitted ELF hole") id (E.byteAt elf address) | address <- [E.sectionAddress section .. E.sectionAddress section + E.sectionSize section - 1]]

seed :: Register -> Word32
seed register = 0xa5a50000 + fromIntegral (fromEnum register)

-- | The runner admits a case id that starts with a lowercase letter and holds
-- only lowercase letters, digits and dashes.
caseIdFor :: Helper -> Int -> Role -> String
caseIdFor helper index role = "h" ++ helperAddress helper ++ "-" ++ show index ++ "-" ++ roleName
  where
    roleName = if role == Stock then "stock" else "owned"

runnerCaseId :: String -> Bool
runnerCaseId value = case value of
  first : rest -> first >= 'a' && first <= 'z' && length rest <= 63 && all (\character -> isAsciiLower character || isDigit character || character == '-') rest
  [] -> False

caseFor :: Role -> B.ByteString -> B.ByteString -> E.Elf -> M.Map Word32 [Span] -> Helper -> Int -> (Int, [Word32]) -> CaseSpec
caseFor role image4 image5 elf closures helper index one@(_, scalars) = CaseSpec
  { caseId = caseIdFor helper index role
  , caseProvenance = sourceIdentity
  , caseExclusion = "Leaf stock body over declared ordinary memory only: no MMIO, no coprocessor, no undefined callee, no interrupt, timing or physical device. Pointer arguments, pointer words and incoming stack arguments point at staged pages; in-image pages carry the stock bytes."
  , caseEntry = GuestAddress (fromIntegral (if role == Stock then helperEntry helper else ownedEntry elf helper))
  , caseExit = GuestAddress (fromIntegral stopAddress)
  , caseRegions = codeRegions role image4 image5 elf helper
      ++ [mkRegion ident (GuestAddress (fromIntegral base)) (B.length bytes) ReadWrite bytes | (ident, base, bytes) <- planRegions plan]
      ++ [mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (B.length (planStack plan)) ReadWrite (planStack plan)
         , mkRegion (RegionId "return") (GuestAddress (fromIntegral stopAddress)) 4 ReadExecute (word32le 0xe7f000f0)]
  , caseFunctions = case role of
      Stock -> [(GuestAddress (fromIntegral address), size) | (address, size) <- helperStockExtents helper]
      Owned -> [(GuestAddress (fromIntegral base), size) | (base, size) <- M.findWithDefault [] (ownedEntry elf helper) closures]
  , caseInitial = [(register, maybe (seed register) id (lookup register argumentValues)) | register <- [R0 .. R12]]
      ++ [(Sp, stackPointer), (Lr, stopAddress), (Cpsr, 0x10)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(Sp, stackPointer), (Lr, stopAddress)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    plan = planFor image4 image5 helper one
    argumentValues =
      zip (helperPointerArguments helper) (map bufferBase (helperPointerArguments helper))
      ++ zip (helperScalarArguments helper) scalars

-- | Two roles of one case compare outcome, declared result, every staged byte,
-- the caller frame, both write sets and the stock instruction count.
compareCase :: Helper -> Plan -> NativeObservation -> NativeObservation -> [String]
compareCase helper plan stock owned =
  ["stock status " ++ runnerStatusLabel (nativeStatus stock) | nativeStatus stock /= Returned]
  ++ ["owned status " ++ runnerStatusLabel (nativeStatus owned) | nativeStatus owned /= Returned]
  ++ ["return " ++ registerLabel register | register <- helperReturnRegisters helper, registerValue stock register /= registerValue owned register]
  ++ ["stack pointer" | registerValue stock Sp /= registerValue owned Sp]
  ++ ["region " ++ regionName ident | (ident, _, _) <- planRegions plan, regionBytesOf stock ident /= regionBytesOf owned ident]
  ++ ["stock caller frame" | not (callerFrameIntact plan stock)]
  ++ ["owned caller frame" | not (callerFrameIntact plan owned)]
  ++ ["stock entered " ++ show (instructionCount (nativeInstructions stock)) | instructionCount (nativeInstructions stock) < fromIntegral (helperMinimumInstructions helper)]
  ++ ["owned entered" | instructionCount (nativeInstructions owned) == 0]
  ++ ["stock write outside extent" | not (writesContained plan stock)]
  ++ ["owned write outside extent" | not (writesContained plan owned)]

-- | Every staged frame byte above the entry stack pointer is unchanged.
callerFrameIntact :: Plan -> NativeObservation -> Bool
callerFrameIntact plan observation =
  let staged = planStack plan
      bytes = regionBytesOf observation (RegionId "stack")
      offset = fromIntegral (stackPointer - stackBase)
  in B.length bytes == B.length staged && B.drop offset bytes == B.drop offset staged

writesContained :: Plan -> NativeObservation -> Bool
writesContained plan observation = case provenanceWrites (nativeProvenance observation) of
  Nothing -> False
  Just writes -> all (within plan) writes
  where
    within plan' (Write (GuestAddress address) (WriteBytes length')) =
      any (\(_, base, bytes) -> address >= fromIntegral base && address + fromIntegral length' <= fromIntegral base + fromIntegral (B.length bytes)) (planRegions plan')
      || (address >= fromIntegral stackBase && address + fromIntegral length' <= fromIntegral stackPointer)

data ControlResult = ControlResult
  { controlHelper :: String
  , controlModule :: String
  , controlVariant :: String
  , controlDetected :: Bool
  }

-- | Six deliberately wrong owned observations in six modules. Each is compared
-- with the stock observation through the same comparator the cases use.
controlIdentities :: [String]
controlIdentities = map fst controlSpecs

controlSpecs :: [(String, String)]
controlSpecs =
  [ ("69f8fc", "return register")
  , ("75a440", "staged byte")
  , ("740888", "staged byte")
  , ("756e90", "staged byte")
  , ("ef9bc0", "return register")
  , ("741fa0", "return register")
  ]

controlFor :: Helper -> Plan -> NativeObservation -> NativeObservation -> ControlResult
controlFor helper plan stock owned = ControlResult (helperAddress helper) (helperModule helper) variant (not (null (compareCase helper plan stock wrong)))
  where
    (variant, wrong) = case lookup (helperAddress helper) controlSpecs of
      Just "return register" -> ("return register", owned {nativeRegisters = M.adjust (`xor` 1) R0 (nativeRegisters owned)})
      _ -> ("staged byte", flipFirst)
    flipFirst = owned {nativeMemory = [(key, if key == RegionId name then flipByte bytes else bytes) | (key, bytes) <- nativeMemory owned]}
    name = firstRegion plan
    flipByte bytes = case B.uncons bytes of
      Just (byte, rest) -> B.cons (byte `xor` 1) rest
      Nothing -> bytes

firstCase :: Helper -> (Int, [Word32])
firstCase helper = case helperCases helper of
  (one : _) -> one
  [] -> error ("helper without cases: " ++ helperAddress helper)

firstRegion :: Plan -> String
firstRegion plan = case planRegions plan of
  ((ident, _, _) : _) -> regionName ident
  [] -> "stack"

-- | Pure layout invariants, checked before a run so a staging defect is a
-- host failure rather than a native case.
planProblems :: [String]
planProblems = concatMap helperProblems allHelpers
  where
    helperProblems helper =
      [ "no cases: " ++ helperAddress helper | null (helperCases helper) ]
      ++ [ "case id rejected by the runner: " ++ ident
         | (index, _) <- zip [0 ..] (helperCases helper)
         , role <- [Stock, Owned]
         , let ident = caseIdFor helper index role
         , not (runnerCaseId ident) ]
      ++ [ "pointer argument outside r0-r3: " ++ helperAddress helper ++ " " ++ registerLabel register
         | register <- helperPointerArguments helper, register > R3 ]
      ++ [ "pointer and scalar argument overlap: " ++ helperAddress helper
         | not (null (filter (`elem` helperScalarArguments helper) (helperPointerArguments helper))) ]
      ++ [ "pointer slot outside staged pages: " ++ helperAddress helper
         | (slot, _) <- helperPointerSlots helper
         , not (any (\(_, base, size) -> slot >= base && slot + 4 <= base + size) (planLayout helper)) ]
      ++ [ "overlapping staged regions: " ++ helperAddress helper
         | overlapping (map (\(_, base, size) -> (base, size)) (planLayout helper)) ]
      ++ [ "staged region overlaps stock extent: " ++ helperAddress helper
         | (_, base, size) <- planLayout helper
         , (extentBase, extentSize) <- helperStockExtents helper
         , base < extentBase + extentSize && extentBase < base + size ]
      ++ [ "staged region inside owned bank: " ++ helperAddress helper
         | (_, base, size) <- planLayout helper
         , base < 0x180000 && 0x100000 < base + size ]
      ++ [ "stock extent outside image: " ++ helperAddress helper
         | (address, size) <- helperStockExtents helper
         , let (start, end) = case imageOf address of
                 Image4 -> (0x0061c400, 0x0089ec00)
                 Image5 -> (0x00bf1a00, 0x010e3860)
         , address < start || address + size > end ]
      ++ [ "entry extent missing: " ++ helperAddress helper
         | not (any (\(address, size) -> address == helperEntry helper && size == helperSize helper) (helperStockExtents helper)) ]

-- | @only@ restricts the compared rows to those stock addresses; empty compares
-- every row. The six controls run either way.
runSuite :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> [String] -> IO ([String], [ControlResult])
runSuite runner cache directory image4Path image5Path elfPath only = do
  unless (null planProblems) (fail ("staging plan problems: " ++ show (take 20 planProblems)))
  let compared = case only of
        [] -> allHelpers
        addresses -> [helper | helper <- allHelpers, helperAddress helper `elem` addresses]
      unknown = [address | address <- only, not (any (\helper -> helperAddress helper == address) allHelpers)]
  unless (null unknown) (fail ("rows not in the table: " ++ show unknown))
  image4 <- B.readFile image4Path
  image5 <- B.readFile image5Path
  unless (sha256Hex (sha256 image4) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  let closures = ownedClosures elf (ownedSpans elf)
      spec role helper index one = caseFor role image4 image5 elf closures helper index one
  forM_ allHelpers $ \helper -> case [symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == "xm1_power_record_helper_" ++ helperAddress helper, E.symbolKind symbol == E.FunctionSymbol, E.symbolSize symbol > 0] of
    [_] -> pure ()
    _ -> fail ("missing owned function: " ++ helperAddress helper)
  forM_ allHelpers $ \helper -> case M.lookup (ownedEntry elf helper) closures of
    Nothing -> fail ("missing owned closure: " ++ helperAddress helper)
    Just closure
      | null closure -> fail ("empty owned closure: " ++ helperAddress helper)
      | not (any (\(base, _) -> base == ownedEntry elf helper) closure) -> fail ("owned entry does not start a function: " ++ helperAddress helper)
      | length closure > 512 -> fail ("owned closure exceeds the runner function limit: " ++ helperAddress helper)
      | overlapping closure -> fail ("owned closure overlaps: " ++ helperAddress helper)
      | otherwise -> pure ()
  forM_ (filter (\section -> E.sectionAllocated section && E.sectionSize section > 0) (E.elfSections elf)) $ \section ->
    unless ((E.sectionType section == 1 || E.sectionType section == 8) && E.sectionAddress section >= 0x100000 && toInteger (E.sectionAddress section) + toInteger (E.sectionSize section) <= 0x180000) (fail "owned allocated section escapes admitted bank")
  failures <- fmap concat $ forM (chunks compared) $ \selected -> do
    observations <- forM [Stock, Owned] $ \role -> do
      batch <- case selected of
        [] -> fail "empty chunk"
        (first : _) -> writeBatchWithProfile Resource8 (directory </> roleName role </> show (helperEntry first)) (concat [map (uncurry (spec role helper)) (zip [0 ..] (helperCases helper)) | helper <- selected])
      (_, results) <- cachedObservedBatch cache runner batch
      pure (M.fromList [(caseIdText key, value) | (key, value) <- results])
    case observations of
      [stock, owned] -> pure
        [ (helperAddress helper ++ " case " ++ show index ++ ": " ++ problem)
        | helper <- selected
        , (index, one) <- zip [0 ..] (helperCases helper)
        , problem <- case (M.lookup (caseId (spec Stock helper index one)) stock, M.lookup (caseId (spec Owned helper index one)) owned) of
            (Just stockObservation, Just ownedObservation) -> compareCase helper (planFor image4 image5 helper one) stockObservation ownedObservation
            _ -> ["missing observation"]
        ]
      _ -> fail "role count"
  controls <- fmap concat $ forM (chunks [helper | helper <- allHelpers, helperAddress helper `elem` controlIdentities]) $ \selected -> do
    observations <- forM [Stock, Owned] $ \role -> do
      batch <- case selected of
        [] -> fail "empty control chunk"
        (first : _) -> writeBatchWithProfile Resource8 (directory </> ("controls-" ++ roleName role) </> show (helperEntry first)) (map (\helper -> spec role helper 0 (firstCase helper)) selected)
      (_, results) <- cachedObservedBatch cache runner batch
      pure (M.fromList [(caseIdText key, value) | (key, value) <- results])
    case observations of
      [stock, owned] -> pure
        [ controlFor helper (planFor image4 image5 helper (firstCase helper)) stockObservation ownedObservation
        | helper <- selected
        , Just stockObservation <- [M.lookup (caseId (spec Stock helper 0 (firstCase helper))) stock]
        , Just ownedObservation <- [M.lookup (caseId (spec Owned helper 0 (firstCase helper))) owned]
        ]
      _ -> fail "control role count"
  pure (failures, controls)
  where
    roleName Stock = "stock"
    roleName Owned = "owned"
    chunks [] = []
    chunks rest = let (batch, remaining) = splitAt 4 rest in batch : chunks remaining
