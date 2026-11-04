{-# LANGUAGE OverloadedStrings #-}

-- | Native cases of the 17 own-first RTOS slots (#156): the case grid, the per-domain memory layout,
-- the bodies under test, the batch runs and the per-case comparison.
module Replacement.RtosOwnFirst.Oracle
  ( Slot (..)
  , slots
  , slotOffset
  , slotTag
  , DataWords (..)
  , dataWords
  , EntryMode (..)
  , EntryState (..)
  , entryCpsr
  , entryStates
  , TaskChoice (..)
  , taskChoices
  , Row (..)
  , caseGrid
  , xorshift
  , contextValues
  , dispatchValues
  , flagBytes
  , argumentValues
  , identityValues
  , bufferPointer
  , stackPointer
  , Body (..)
  , OwnedDomain (..)
  , ownedDomain
  , stockBody
  , ownedBody
  , replaceEntry
  , slotEntry
  , caseSpec
  , WriteEvent (..)
  , Run (..)
  , Difference (..)
  , differenceText
  , compareRuns
  , comparedRegions
  , controlMask
  , runOf
  , reportWrites
  , CaseKey (..)
  , runSlot
  , Outcome (..)
  , outcomeOf
  , statusCounts
  , allReturned
  ) where

import Control.Exception (evaluate)
import Control.Monad (forM, unless)
import Data.Aeson (Value, eitherDecodeStrict')
import Data.Bits (shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (isNothing, mapMaybe)
import Data.Word (Word32, Word64, Word8)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Register (..), RegionId (..), RunnerStatus (..))
import Emulation.OwnedCase (CaseId (..), CaseRegion, CaseSpec (..), Permission (..), loadObservedBatch, mkRegion, word32le, writeBatch)
import FujiAF.Host (get, list, num, str)
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Artifact (LinkedDomain, StageError, ownedExtents)
import Replacement.RtosOwnFirst.Regions
import System.FilePath ((</>))

data Slot = S60 | Sfc | S100 | S128 | S12c | S130 | S134 | S138 | S13c | S148 | S14c | S150 | S154 | S164 | S168 | S16c | S1618
  deriving (Eq, Ord, Show, Enum, Bounded)

slots :: [Slot]
slots = [minBound .. maxBound]

slotOffset :: Slot -> SlotOffset
slotOffset slot = SlotOffset $ case slot of
  S60 -> 0x60
  Sfc -> 0xfc
  S100 -> 0x100
  S128 -> 0x128
  S12c -> 0x12c
  S130 -> 0x130
  S134 -> 0x134
  S138 -> 0x138
  S13c -> 0x13c
  S148 -> 0x148
  S14c -> 0x14c
  S150 -> 0x150
  S154 -> 0x154
  S164 -> 0x164
  S168 -> 0x168
  S16c -> 0x16c
  S1618 -> 0x1618

-- | As @s16c@.
slotTag :: Slot -> String
slotTag slot = case slotOffset slot of SlotOffset offset -> 's' : showHex offset ""

-- | Stock data words of one loader, at their stock addresses.
data DataWords = DataWords
  { wordContext :: Word32
  , wordCurrentTask :: Word32
  , wordDispatch :: Word32
  , wordTimerBase :: Word32
  , wordTimeOffset :: Word32
  , -- | Task record 0; records are 56 bytes.
    wordTaskTable :: Word32
  }
  deriving (Eq, Show)

dataWords :: Domain -> DataWords
dataWords domain = case domain of
  DomainA -> DataWords 0x400545a4 0x40054760 0x40054764 0x4005476c 0x40054774 0x4005ff0c
  DomainB -> DataWords 0x400e4564 0x400e4720 0x400e4724 0x400e472c 0x400e4734 0x400efecc

-- | The stock +0x1618 helper.
helperExtent :: Domain -> Extent
helperExtent domain = case domain of
  DomainA -> Extent (GuestAddress 0x427b8) (GuestAddress 0x427d4)
  DomainB -> Extent (GuestAddress 0xd27b8) (GuestAddress 0xd27d4)

data EntryMode = Supervisor | System deriving (Eq, Ord, Show, Enum, Bounded)

data EntryState = EntryState
  { entryMode :: EntryMode
  , entryIrqMasked :: Bool
  , entryFiqMasked :: Bool
  , -- | NZCVQ and GE bits.
    entryFlags :: Word32
  }
  deriving (Eq, Show)

entryCpsr :: EntryState -> Word32
entryCpsr (EntryState mode irq fiq flags) =
  (flags .&. flagMask) .|. (if irq then 0x80 else 0) .|. (if fiq then 0x40 else 0) .|. case mode of
    Supervisor -> 0x13
    System -> 0x1f

flagMask :: Word32
flagMask = 0xf80f0000

flagPatterns :: [Word32]
flagPatterns = [0, 0xf80f0000, 0x60050000]

entryStates :: [EntryState]
entryStates = [EntryState mode irq fiq flags | mode <- [minBound .. maxBound], irq <- [False, True], fiq <- [False, True], flags <- flagPatterns]

-- | The current-task word: null, or a byte offset from task record 0.
data TaskChoice = NullTask | TaskAt Int deriving (Eq, Show)

recordBytes :: Int
recordBytes = 56

-- | T[0], T[59], T[119], base+4, base+8, base-56 and base+24, whose (T-base)>>3 is 3.
taskChoices :: [TaskChoice]
taskChoices = NullTask : map TaskAt [0, 59 * recordBytes, 119 * recordBytes, 4, 8, -recordBytes, 24]

taskAddress :: DataWords -> TaskChoice -> Word32
taskAddress words' choice = case choice of
  NullTask -> 0
  TaskAt offset -> fromIntegral (toInteger (wordTaskTable words') + toInteger offset)

contextValues, dispatchValues :: [Word32]
contextValues = [0, 1, 2, 0x80000000, 0xffffffff]
dispatchValues = contextValues

flagBytes :: [Word8]
flagBytes = [0, 4, 0xfb, 0xff]

-- | +0x168's r0, the last drawn from the xorshift stream.
argumentValues :: [Word32]
argumentValues = [0, 0x80, 0x7f, 0xffffffff, xorshift 0x1f123bb5]

-- | +0x1618's CP14 c2 value, the last drawn from the xorshift stream.
identityValues :: [Word32]
identityValues = [0x4003, 0x4002, 0x14003, 0, xorshift 0x2b8f6e01]

data Row = Row
  { rowEntry :: EntryState
  , rowContext :: Word32
  , rowDispatch :: Word32
  , rowTask :: TaskChoice
  , rowFlagByte :: Word8
  , rowTimerBase :: Word32
  , rowTimeOffset :: Word32
  , rowR0 :: Word32
  , rowIdentity :: Maybe Word32
  }
  deriving (Eq, Show)

xorshift :: Word32 -> Word32
xorshift x0 =
  let x1 = x0 `xor` (x0 `shiftL` 13)
      x2 = x1 `xor` (x1 `shiftR` 17)
   in x2 `xor` (x2 `shiftL` 5)

stream :: Word32 -> [Word32]
stream seed = drop 1 (iterate xorshift seed)

randomRowsPerSlot :: Int
randomRowsPerSlot = 64

-- | The dimensions a slot's structured rows sweep; the rest hold their first value.
data Sweep = Sweep
  { sweepContext :: [Word32]
  , sweepDispatch :: [Word32]
  , sweepTask :: [TaskChoice]
  , sweepByte :: [Word8]
  , sweepR0 :: [Word32]
  , sweepIdentity :: [Maybe Word32]
  }

sweep :: Slot -> Sweep
sweep slot = case slot of
  S60 -> base {sweepTask = taskChoices, sweepByte = flagBytes}
  Sfc -> contexts
  S100 -> contexts
  S128 -> contextsAndTasks
  S12c -> contextsAndTasks
  S16c -> contextsAndTasks
  S130 -> contexts
  S134 -> contexts
  S138 -> contexts
  S13c -> contexts
  S148 -> contexts
  S14c -> base
  S150 -> base {sweepDispatch = dispatchValues}
  S154 -> contexts {sweepDispatch = dispatchValues}
  S164 -> base
  S168 -> base {sweepR0 = argumentValues}
  S1618 -> base {sweepIdentity = map Just identityValues}
  where
    base = Sweep [0] [1] [TaskAt 0] [0] [bufferPointer] [Nothing]
    contexts = base {sweepContext = contextValues}
    contextsAndTasks = contexts {sweepTask = taskChoices}

pick :: [a] -> Word32 -> a
pick values word = values !! fromIntegral (word `mod` fromIntegral (length values))

-- | Every entry state crossed with the slot's sweep, then 64 xorshift rows; timer base and time
-- offset come from a per-slot xorshift stream.
caseGrid :: Slot -> [Row]
caseGrid slot = zipWith timed (pairs (stream (0x2545f491 + index))) (structured ++ drawn)
  where
    index = fromIntegral (fromEnum slot)
    Sweep contexts dispatches tasks bytes arguments identities = sweep slot
    structured =
      [ Row entry context dispatch task byte 0 0 argument identity
      | entry <- entryStates
      , context <- contexts
      , dispatch <- dispatches
      , task <- tasks
      , byte <- bytes
      , argument <- arguments
      , identity <- identities
      ]
    drawn = take randomRowsPerSlot (map draw (chunks 10 (stream (0x6c8e9cf5 `xor` (index `shiftL` 8)))))
    draw ws = case ws of
      [w0, w1, w2, w3, w4, w5, w6, w7, w8, w9] ->
        Row
          (EntryState (pick [minBound .. maxBound] w0) (testBit w1 0) (testBit w1 1) (w2 .&. flagMask))
          (pick (contextValues ++ [w3]) w4)
          (pick (dispatchValues ++ [w5]) w6)
          (pick taskChoices w7)
          (fromIntegral w8)
          0
          0
          (if slot == S168 then w9 else bufferPointer)
          (if slot == S1618 then Just (pick (identityValues ++ [w9]) w8) else Nothing)
      _ -> error "unreachable chunk"
    timed (timerBase, timeOffset) row = row {rowTimerBase = timerBase, rowTimeOffset = timeOffset}
    pairs (a : b : rest) = (a, b) : pairs rest
    pairs _ = []

chunks :: Int -> [a] -> [[a]]
chunks size items = case splitAt size items of
  (chunk, rest) | length chunk == size -> chunk : chunks size rest
  _ -> []

bufferBase, bufferBytes, bufferPointer, bufferSentinel :: Word32
bufferBase = 0x0096b400
bufferBytes = 16
bufferPointer = bufferBase + 4
bufferSentinel = 0xc0dec0de

stackBase, stackPointer, stackTop, exitAddress :: Word32
stackBase = 0x009596f0
stackPointer = 0x009597f0
stackTop = 0x00959800
exitAddress = 0x00958f00

-- | Words between the current-task word and the time-offset word that no slot names.
unnamedWord :: Word32
unnamedWord = 0x0badf00d

wordsBytes :: Int
wordsBytes = 0x18

tasksBytes :: Int
tasksBytes = 121 * recordBytes

-- | The task-table windows the grid's T values reach, as byte offsets from record 0: records -1 to
-- 1, record 59 and record 119.
taskWindows :: [(RegionId, Int, Int)]
taskWindows =
  [ (RegionId "tasks-low", -recordBytes, 2 * recordBytes)
  , (RegionId "tasks-59", 59 * recordBytes, recordBytes)
  , (RegionId "tasks-119", 119 * recordBytes, recordBytes)
  ]

windowBase :: DataWords -> Int -> Word32
windowBase words' offset = fromIntegral (toInteger (wordTaskTable words') + toInteger offset)

contextId, wordsId, bufferId, stackId, callerFrameId, exitId :: RegionId
contextId = RegionId "context"
wordsId = RegionId "words"
bufferId = RegionId "buffer"
stackId = RegionId "stack"
callerFrameId = RegionId "caller-frame"
exitId = RegionId "exit"

-- | The data regions and the caller frame; the stack below entry sp is scratch.
comparedRegions :: Domain -> [(RegionId, Word32, Int)]
comparedRegions domain =
  [ (contextId, wordContext words', 4)
  , (wordsId, wordCurrentTask words', wordsBytes)
  ]
    ++ [(ident, windowBase words' offset, size) | (ident, offset, size) <- taskWindows]
    ++ [ (bufferId, bufferBase, fromIntegral bufferBytes)
       , (callerFrameId, stackPointer, fromIntegral (stackTop - stackPointer))
       ]
  where
    words' = dataWords domain

wordsImage :: DataWords -> Row -> B.ByteString
wordsImage words' row = B.concat [word32le (valueAt (wordCurrentTask words' + offset)) | offset <- [0, 4 .. fromIntegral wordsBytes - 4]]
  where
    valueAt address
      | address == wordCurrentTask words' = taskAddress words' (rowTask row)
      | address == wordDispatch words' = rowDispatch row
      | address == wordTimerBase words' = rowTimerBase row
      | address == wordTimeOffset words' = rowTimeOffset row
      | otherwise = unnamedWord

-- | One task window's bytes: 'tasksBackground', with the row's T+0xe byte when it lies in the window.
taskWindowImage :: Row -> Int -> Int -> B.ByteString
taskWindowImage row offset size = case rowTask row of
  TaskAt task
    | at >= 0 && at < size -> B.take at window <> B.singleton (rowFlagByte row) <> B.drop (at + 1) window
    where
      at = task + 0xe - offset
  _ -> window
  where
    window = B.take size (B.drop (offset + recordBytes) tasksBackground)

-- | xorshift bytes with no zero byte, from record -1.
tasksBackground :: B.ByteString
tasksBackground = B.pack (take tasksBytes [if byte == 0 then 0x5a else byte | word <- stream 0x9e3779b9, let byte = fromIntegral (word `shiftR` 24) :: Word8])

callerFrameImage :: B.ByteString
callerFrameImage = B.concat [word32le (0xca11e500 + n) | n <- [0 .. (stackTop - stackPointer) `div` 4 - 1]]

-- | Executable regions, as (base, bytes), and declared functions, as (start, size).
data Body = Body
  { bodyEntry :: Word32
  , bodyCode :: [(Word32, B.ByteString)]
  , bodyFunctions :: [(Word32, Word32)]
  }
  deriving (Eq, Show)

-- | One domain's region table and installed owned extents.
data OwnedDomain = OwnedDomain
  { ownedDomainId :: Domain
  , ownedTable :: DomainTable
  , ownedFill :: Word32
  , ownedRegions :: [(Region, B.ByteString)]
  }

ownedDomain :: Word32 -> Domain -> DomainTable -> LinkedDomain -> Either StageError OwnedDomain
ownedDomain fill domain table linked = OwnedDomain domain table fill <$> ownedExtents fill domain table linked

regionOf :: DomainTable -> Slot -> Either String Region
regionOf table slot = maybe (Left ("no region for " ++ slotTag slot)) Right (find ((== slotOffset slot) . regionSlot) (domainExtents table))

slotEntry :: DomainTable -> Slot -> Either String Word32
slotEntry table slot = (\region -> case regionEntry region of GuestAddress address -> address) <$> regionOf table slot

-- | The slot's own region, then the regions it reaches: +0x16c calls +0x148 and +0x128, and the
-- owned +0x1618 entry branches to its body in +0x12c's region.
ownedReach :: DomainTable -> Slot -> [SlotOffset]
ownedReach table slot = case slot of
  S16c -> [slotOffset S16c, slotOffset S148, slotOffset S128]
  S1618 -> [slotOffset S1618, bodyHost (tableBody table)]
  _ -> [slotOffset slot]

extentBounds :: Extent -> (Word32, Word32)
extentBounds (Extent (GuestAddress start) (GuestAddress end)) = (start, end)

stockBody :: Domain -> DomainTable -> B.ByteString -> Slot -> Either String Body
stockBody domain table loader slot = do
  own <- regionOf table slot
  extras <- case slot of
    S16c -> mapM (fmap regionExtent . regionOf table) [S148, S128]
    S1618 -> Right [helperExtent domain]
    _ -> Right []
  let extents = regionExtent own : extras
      GuestAddress base = domainLoaderBase domain
      slice (start, end) = B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - base)) loader)
      bounds = map extentBounds extents
  unless (B.length loader == domainLoaderSize domain) (Left "loader size")
  pure (Body (fst (extentBounds (regionExtent own))) [(start, slice (start, end)) | (start, end) <- bounds] [(start, end - start) | (start, end) <- bounds])

ownedBody :: OwnedDomain -> Slot -> Either String Body
ownedBody owned slot = do
  let table = ownedTable owned
      body = tableBody table
      (bodyStart, bodyEnd) = extentBounds (bodyExtent body)
  entry <- slotEntry table slot
  mapped <- forM (ownedReach table slot) $ \offset ->
    maybe (Left "owned region missing") Right (find ((== offset) . regionSlot . fst) (ownedRegions owned))
  let functions (region, _)
        | regionSlot region == bodyHost body = [(start, bodyStart - start), (bodyStart, bodyEnd - bodyStart)]
        | otherwise = [(start, end - start)]
        where
          (start, end) = extentBounds (regionExtent region)
  pure (Body entry [(fst (extentBounds (regionExtent region)), bytes) | (region, bytes) <- mapped] (concatMap functions mapped))

-- | The body with the function at its entry rewritten as @code@ followed by fill words.
replaceEntry :: Word32 -> [Word32] -> Body -> Either String Body
replaceEntry fill code body = do
  let entry = bodyEntry body
  size <- maybe (Left "entry is not a declared function") Right (lookup entry (bodyFunctions body))
  unless (4 * length code <= fromIntegral size) (Left ("code of " ++ show (length code) ++ " words exceeds the " ++ show size ++ "-byte function"))
  let replacement = B.concat (map word32le (code ++ replicate (fromIntegral size `div` 4 - length code) fill))
      rewrite (base, bytes)
        | base <= entry && entry < base + fromIntegral (B.length bytes) =
            let at = fromIntegral (entry - base) in (base, B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes)
        | otherwise = (base, bytes)
  pure body {bodyCode = map rewrite (bodyCode body)}

sentinel :: Register -> Word32
sentinel register = 0xa5a50000 + fromIntegral (fromEnum register)

placeRegion :: RegionId -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
placeRegion ident address = mkRegion ident (N.GuestAddress (fromIntegral address))

caseSpec :: Domain -> Body -> String -> Slot -> Row -> CaseSpec
caseSpec domain body label slot row =
  CaseSpec
    { caseId = label
    , caseProvenance = "X-M1 1.50 loader " ++ domainLabel domain ++ " RTOS slot +0x" ++ drop 1 (slotTag slot) ++ " over the own-first oracle data layout"
    , caseExclusion = "one privileged call of the slot body; reads or calls outside the mapped regions refuse"
    , caseEntry = N.GuestAddress (fromIntegral (bodyEntry body))
    , caseExit = N.GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [placeRegion (RegionId ("code-" ++ show n)) base (B.length bytes) ReadExecute bytes | (n, (base, bytes)) <- zip [0 :: Int ..] (bodyCode body)]
          ++ [ placeRegion contextId (wordContext words') 4 ReadWrite (word32le (rowContext row))
             , placeRegion wordsId (wordCurrentTask words') wordsBytes ReadWrite (wordsImage words' row)
             ]
          ++ [placeRegion ident (windowBase words' offset) size ReadWrite (taskWindowImage row offset size) | (ident, offset, size) <- taskWindows]
          ++ [ placeRegion bufferId bufferBase (fromIntegral bufferBytes) ReadWrite (B.concat (replicate (fromIntegral bufferBytes `div` 4) (word32le bufferSentinel)))
             , placeRegion stackId stackBase (fromIntegral (stackPointer - stackBase)) ReadWrite (B.replicate (fromIntegral (stackPointer - stackBase)) 0)
             , placeRegion callerFrameId stackPointer (B.length callerFrameImage) ReadWrite callerFrameImage
             , placeRegion exitId exitAddress 4 ReadExecute (word32le 0xe12fff1e)
             ]
    , caseFunctions = [(N.GuestAddress (fromIntegral start), size) | (start, size) <- bodyFunctions body]
    , caseInitial =
        (R0, rowR0 row)
          : [(register, sentinel register) | register <- [R1 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, entryCpsr (rowEntry row))]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = [(R4, sentinel R4)]
    , caseExpectedMemory = []
    , caseCp14Identity = rowIdentity row
    }
  where
    words' = dataWords domain

data WriteEvent = WriteEvent
  { eventAddress :: !Word32
  , eventWidth :: !Int
  , eventValue :: !Word64
  }
  deriving (Eq, Show)

-- | The compared part of one observation; 'runWrites' is Nothing when the provenance log was truncated.
data Run = Run
  { runStatus :: !RunnerStatus
  , runRegisters :: ![(Register, Word32)]
  , runControl :: !Word32
  , runMemory :: ![(RegionId, B.ByteString)]
  , runWrites :: !(Maybe [WriteEvent])
  }
  deriving (Eq, Show)

comparedRegisters :: [Register]
comparedRegisters = [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp]

-- | CPSR mode, T, F, I and A.
controlMask :: Word32
controlMask = 0x1ff

data Difference
  = StatusDiffers RunnerStatus RunnerStatus
  | RegisterDiffers Register Word32 Word32
  | ControlDiffers Word32 Word32
  | MemoryDiffers RegionId
  | WritesDiffer
  | ReferenceTruncated
  | SubjectTruncated
  deriving (Eq, Show)

hex32 :: Word32 -> String
hex32 = N.hexWord32

differenceText :: Difference -> String
differenceText difference = case difference of
  StatusDiffers expected actual -> "status " ++ N.runnerStatusLabel actual ++ ", expected " ++ N.runnerStatusLabel expected
  RegisterDiffers register expected actual -> N.registerLabel register ++ " " ++ hex32 actual ++ ", expected " ++ hex32 expected
  ControlDiffers expected actual -> "cpsr mode/T/F/I/A " ++ hex32 actual ++ ", expected " ++ hex32 expected
  MemoryDiffers ident -> "final bytes of " ++ regionName ident
  WritesDiffer -> "ordered write log"
  ReferenceTruncated -> "reference provenance truncated"
  SubjectTruncated -> "subject provenance truncated"

compareRuns :: Run -> Run -> [Difference]
compareRuns reference subject =
  [StatusDiffers (runStatus reference) (runStatus subject) | runStatus reference /= runStatus subject]
    ++ [ RegisterDiffers register expected actual
       | (register, expected) <- runRegisters reference
       , Just actual <- [lookup register (runRegisters subject)]
       , actual /= expected
       ]
    ++ [ControlDiffers (runControl reference) (runControl subject) | runControl reference /= runControl subject]
    ++ [MemoryDiffers ident | (ident, bytes) <- runMemory reference, lookup ident (runMemory subject) /= Just bytes]
    ++ [ReferenceTruncated | isNothing (runWrites reference)]
    ++ [SubjectTruncated | isNothing (runWrites subject)]
    ++ [WritesDiffer | Just expected <- [runWrites reference], Just actual <- [runWrites subject], expected /= actual]

-- | The write events of a case report's @runner.memory_provenance@, in order.
reportWrites :: Value -> Either String [WriteEvent]
reportWrites report = do
  entries <- mapM entry (list (get (get report "runner") "memory_provenance"))
  pure [event | Just event <- entries]
  where
    entry value = case str (get value "kind") of
      "read" -> Right Nothing
      "write" -> do
        address <- hexField value "address"
        width <- case num (get value "size") of
          size | size >= 1 && size <= 8 -> Right (fromInteger size)
          _ -> Left "provenance size"
        written <- hexField value "value"
        Right (Just (WriteEvent (fromIntegral address) width written))
      other -> Left ("provenance kind " ++ other)
    hexField :: Value -> String -> Either String Word64
    hexField value key = case str (get value key) of
      '0' : 'x' : digits | not (null digits), length digits <= 16, [(n, "")] <- readHex digits -> Right n
      text -> Left ("provenance " ++ key ++ " " ++ text)

runOf :: Domain -> N.NativeObservation -> [WriteEvent] -> Either String Run
runOf domain observation writes = do
  registers <- mapM (\register -> maybe (Left ("register " ++ N.registerLabel register)) (Right . (,) register) (N.lookupRegister register observation)) comparedRegisters
  cpsr <- maybe (Left "register cpsr") Right (N.lookupRegister Cpsr observation)
  memory <- mapM (\(ident, _, _) -> maybe (Left ("region " ++ regionName ident)) (Right . (,) ident . B.copy) (lookup ident (N.nativeMemory observation))) regions
  retained <- case N.nativeProvenance observation of
    N.Truncated _ -> Right Nothing
    N.Complete _ logged -> do
      unless (map (\(N.Write (N.GuestAddress a) (N.WriteBytes w)) -> (fromIntegral a, fromIntegral w)) logged == [(eventAddress e, eventWidth e) | e <- writes])
        (Left "report writes differ from the parsed provenance")
      Right (Just [event | event <- writes, compared (eventAddress event)])
  pure (Run (N.nativeStatus observation) registers (cpsr .&. controlMask) memory retained)
  where
    regions = comparedRegions domain
    compared address = or [base <= address && address < base + fromIntegral size | (_, base, size) <- regions]

-- | A case: its row's index in the slot's grid.
newtype CaseKey = CaseKey Int deriving (Eq, Ord, Show)

caseLabel :: String -> Domain -> Slot -> CaseKey -> String
caseLabel role domain slot (CaseKey index) = role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot ++ "-" ++ pad (show index)
  where
    pad text = replicate (4 - length text) '0' ++ text

batchLimit :: Int
batchLimit = 4096

-- | Runs the slot's grid against @body@ in @directory@, one runner session per 4096 cases.
runSlot :: FilePath -> FilePath -> String -> Domain -> Slot -> Body -> IO (M.Map CaseKey Run)
runSlot runner directory role domain slot body = do
  let keyed = zip (map CaseKey [0 ..]) (caseGrid slot)
  results <- forM (zip [0 :: Int ..] (chunks' batchLimit keyed)) $ \(batch, rows) -> do
    let batchDirectory = directory </> ("batch-" ++ show batch)
        labelled = [(caseLabel role domain slot key, key) | (key, _) <- rows]
    batchPath <- writeBatch batchDirectory [caseSpec domain body (caseLabel role domain slot key) slot row | (key, row) <- rows]
    observed <- loadObservedBatch runner batchPath
    unless (length observed == length rows) (fail (batchPath ++ ": observation count differs from the case count"))
    forM observed $ \(CaseId label, observation) -> do
      let reportPath = batchDirectory </> label ++ ".native-result.json"
          failWith problem = fail (reportPath ++ ": " ++ problem)
      key <- maybe (failWith "unknown case") pure (lookup label labelled)
      reportBytes <- B.readFile reportPath
      report <- either failWith pure (eitherDecodeStrict' reportBytes :: Either String Value)
      writes <- either failWith pure (reportWrites report)
      run <- either failWith pure (runOf domain observation writes)
      forced <- evaluate (forceRun run)
      pure (key, forced)
  pure (M.fromList (concat results))
  where
    chunks' size items = case splitAt size items of
      (chunk, []) -> [chunk | not (null chunk)]
      (chunk, rest) -> chunk : chunks' size rest

forceRun :: Run -> Run
forceRun run =
  sum (map snd (runRegisters run)) `seq` sum (map (B.length . snd) (runMemory run)) `seq` maybe 0 length (runWrites run) `seq` run

data Outcome = Outcome
  { outcomeCases :: Int
  , outcomeMatches :: Int
  , outcomeFirst :: Maybe (CaseKey, [Difference])
  , -- | Differing cases whose subject run returned.
    outcomeDifferingReturned :: Int
  , -- | Differing cases whose subject run stopped, or is missing.
    outcomeDifferingStopped :: Int
  , outcomeStatuses :: M.Map String Int
  }
  deriving (Eq, Show)

-- | Every subject run against the reference run of the same case; a case missing on either side differs.
outcomeOf :: M.Map CaseKey Run -> M.Map CaseKey Run -> Outcome
outcomeOf reference subject =
  Outcome
    { outcomeCases = length rows
    , outcomeMatches = length (filter (null . snd) rows)
    , outcomeFirst = find (not . null . snd) rows
    , outcomeDifferingReturned = length (filter returned differing)
    , outcomeDifferingStopped = length (filter (not . returned) differing)
    , outcomeStatuses = statusCounts subject
    }
  where
    keys = M.keys (M.union reference subject)
    rows = mapMaybe row keys
    differing = [key | (key, differences) <- rows, not (null differences)]
    returned key = fmap runStatus (M.lookup key subject) == Just Returned
    row key = case (M.lookup key reference, M.lookup key subject) of
      (Just expected, Just actual) -> Just (key, compareRuns expected actual)
      (Just expected, Nothing) -> Just (key, [StatusDiffers (runStatus expected) (OtherStatus "missing")])
      (Nothing, Just actual) -> Just (key, [StatusDiffers (OtherStatus "missing") (runStatus actual)])
      (Nothing, Nothing) -> Nothing

-- | Runs per runner status label.
statusCounts :: M.Map CaseKey Run -> M.Map String Int
statusCounts runs = M.fromListWith (+) [(N.runnerStatusLabel (runStatus run), 1) | run <- M.elems runs]

allReturned :: M.Map CaseKey Run -> Bool
allReturned runs = not (M.null runs) && all ((== Returned) . runStatus) (M.elems runs)
