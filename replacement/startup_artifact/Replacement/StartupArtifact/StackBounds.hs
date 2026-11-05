-- | Stock code views, register-branch targets and byte scans for the J7 stack and caller checks.
module Replacement.StartupArtifact.StackBounds
  ( Segment (..), segmentsOf, stockCodeOf, ownedRanges
  , rootTable, rootSlotTargets, indirectTargetsOf, switchModel
  , irqEntryFrame, irqPreemptionPeak, irqPreemptionBytes
  , branchSites, addressFormations, storesReaching, fallsInto
  , DescriptorId (..), descriptorIds
  , wrapperEntry, storagePrelude, releaseEntry, cleanupEntry
  , errorCallbackWord, errorCallbackSetter, errorDispatchSite, errorCallbacks
  , TraceCallback (..), traceCallbackKernels, traceCallbacks
  , fillFlag, fillDoneCallback
  , conditionsPhrase, demandConditions, rootDepthConditions, image5DepthConditions, plainDepthConditions, cleanupConditions, heapConditions
  ) where

import qualified A32
import Control.Monad (unless)
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Unsafe as BU
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Replacement.StartupArtifact.Targets (TargetBytes, targetBase, targetOf)
import Replacement.StartupArtifact.Types (Guest (..), Span (..), Target (..))
import StackWalk (Effect (..), Strictness (Strict), classify, writes, written)
import StockStack hiding (rootTable)

-- | Code bytes at a guest base.
data Segment = Segment Word32 B.ByteString

-- | Loader A, loader B, image 4 and image 5 of a target set, and the plain window.
segmentsOf :: TargetBytes -> Maybe (Word32, B.ByteString) -> [Segment]
segmentsOf targets plain =
  [Segment (targetBase target) (targetOf targets target) | target <- [LoaderA, LoaderB, Image4, Image5]]
  ++ [Segment load bytes | Just (load, bytes) <- [plain]]

-- | The walkable stock code: the segments, with every owned carrier excluded.
stockCodeOf :: [Segment] -> [Span] -> StockCode
stockCodeOf segments carriers = stockCode [(base, bytes) | Segment base bytes <- segments] (ownedRanges carriers)

ownedRanges :: [Span] -> [(Word32, Word32)]
ownedRanges carriers = [(start, end) | Span _ (Guest start) (Guest end) <- carriers]

-- | The interface root table the OSAL loads service pointers through.
rootTable :: Word32
rootTable = 0x96bd20

-- | Slot offset to services: table A's published words and table B's stored words, each at its root.
rootSlotTargets :: Word32 -> [(Word32, Word32)] -> Word32 -> [(Word32, Word32)] -> M.Map SlotOffset [Word32]
rootSlotTargets rootA tableA rootB tableB = M.map (S.toList . S.fromList) (M.fromListWith (++)
  ([ (SlotOffset (physical guest - physical rootA), [value]) | (guest, value) <- tableA, physical guest >= physical rootA ]
   ++ [ (SlotOffset (physical guest - physical rootB), [value]) | (guest, value) <- tableB, physical guest >= physical rootB ]))

physical :: Word32 -> Word32
physical address
  | address >= 0x40000000 && address < 0x60000000 = address - 0x40000000
  | otherwise = address

-- | Each kernel's code range and its context initializer @kernel_initialize_owner_contexts@
-- (x70view 55436fc85c function-ranges: kernel0 0x43ac0 and kernel1 0xd3a7c, 392 bytes each; exact).
kernelInitializers :: [(CodeRange, CodeRange)]
kernelInitializers = [(CodeRange 0x40000 0x4dae0, CodeRange 0x43ac0 0x43c48), (CodeRange 0xd0000 0xddaa0, CodeRange 0xd3a7c 0xd3c04)]

-- | Error dispatch @dispatch_error@ calls the word at 0x4089ea8c (x70view 55436fc85c
-- app/platform/osal/interface_ring_operations.c:42-45, exact; id4.dis 0x7233a0-0x7233ac).
errorCallbackWord, errorCallbackSetter, errorDispatchSite :: Word32
errorCallbackWord = 0x4089ea8c
errorCallbackSetter = 0x725b54
errorDispatchSite = 0x7233ac

-- | The canonical data word 0x723330 and root's argument 0x72835c (RootTask/Admission.hs:153-154).
errorCallbacks :: [Word32]
errorCallbacks = [0x723330, 0x72835c]

-- | One kernel's log trace callback: the word its log path calls when nonzero, the setter that
-- stores it, and the call site (kernel0 native_globals.c:57-63, exact; loader-d0000.dis:1341-1348 for kernel1).
data TraceCallback = TraceCallback {traceWord :: Word32, traceSetter :: Word32, traceDispatch :: Word32} deriving (Eq, Show)

traceCallbackKernels :: [TraceCallback]
traceCallbackKernels = [TraceCallback 0x4004f15c 0x41548 0x41598, TraceCallback 0x400df11c 0xd1548 0xd1598]

traceCallbacks :: [Word32]
traceCallbacks = [0x6470fc]

-- | The premises each StockStack bound may use; its check fails when the walk uses another.
demandConditions, rootDepthConditions, image5DepthConditions, plainDepthConditions, cleanupConditions, heapConditions :: [Condition]
demandConditions = [minBound .. maxBound]
rootDepthConditions = [minBound .. maxBound]
image5DepthConditions = [minBound .. maxBound]
plainDepthConditions = [minBound .. maxBound]
cleanupConditions = [SlotWordsFixed, ContextLocksFixed]
heapConditions = [SlotWordsFixed, NoTaskException, ContextLocksFixed]

conditionsPhrase :: [Condition] -> String
conditionsPhrase conditions = case map phrase conditions of
  [] -> "unconditional"
  [one] -> "conditional on " ++ one
  several -> "conditional on " ++ concatMap (++ ", ") (init several) ++ "and " ++ last several
  where
    phrase condition = case condition of
      SlotWordsFixed -> "published root-slot words staying fixed (S1)"
      ErrorCallbackClosed -> "the error-callback set (S2)"
      TraceCallbackClosed -> "the trace-callback set (S3)"
      NoTaskException -> "no task-exception routine being defined (S4)"
      ContextLocksFixed -> "fixed context lock methods called on one context (S5)"

-- | The image5 producer's fill flag and the fill-done callback that sets it.
fillFlag, fillDoneCallback :: Word32
fillFlag = 0x89f8ac
fillDoneCallback = 0x6a8bec

wrapperEntry, storagePrelude, releaseEntry, cleanupEntry :: Word32
wrapperEntry = 0x725dd8
storagePrelude = 0x725c6c
releaseEntry = 0x726098
cleanupEntry = 0x72612c

indirectTargetsOf :: StockCode -> M.Map SlotOffset [Word32] -> Either String IndirectTargets
indirectTargetsOf code slots = do
  locks <- mapM (\(kernel, initializer) -> ContextLocks kernel <$> contextLockMethods code initializer) kernelInitializers
  pure (IndirectTargets rootTable slots callbacks locks)
  where
    callbacks = M.fromList ((errorDispatchSite, CallbackSite errorCallbacks ErrorCallbackClosed)
      : [(traceDispatch kernel, CallbackSite traceCallbacks TraceCallbackClosed) | kernel <- traceCallbackKernels])

-- | The kernels' context switch (loader-40000.dis:3446-3503 and the loader-B twin): SP loads from the
-- incoming task's control block at 0x436e4, or moves to the idle stack at 0x436f4; 0x4364c saves the
-- outgoing SP and 0x43654 its resume PC 0x43668; 0x44288 runs task-exception routines.
switchModel :: SwitchModel
switchModel = SwitchModel
  (S.fromList [0x436e4, 0x436f4, 0xd36e4, 0xd36f4])
  (M.fromList [(0x4364c, 0x43668), (0xd364c, 0xd3668)])
  (S.fromList [0x44288, 0xd4248])

-- | The peak an IRQ that preempts a task leaves on that task's stack (irq-entry-frame derives it).
irqPreemptionBytes :: Word32
irqPreemptionBytes = 88

-- | Bytes the first-level IRQ entry stores on the interrupted stack before it moves to the IRQ
-- stack: the reservations of [base + 0x204, base + 0x24c), then @mov r2, sp@, the IRQ stack load at
-- base + 0x250 and @stmdb sp!, {r2}@, which saves the interrupted SP on the IRQ stack.
irqEntryFrame :: StockCode -> Word32 -> Either String Word32
irqEntryFrame code base = do
  instructions <- mapM (decodedIn code base) [0x204, 0x208 .. 0x248]
  saving <- mapM (decodedIn code base) [0x24c, 0x250, 0x254]
  case saving of
    [ A32.DataProcessing (A32.Cond 14) A32.MOV False r2 _ (A32.ShiftedImmediate from A32.LSL 0)
      , A32.Load (A32.Cond 14) A32.Word False rt rn _ A32.OffsetOnly
      , A32.StoreMultiple (A32.Cond 14) b A32.DecrementBefore True [saved] ]
      | r2 == A32.Reg 2 && from == A32.sp && rt == A32.sp && rn == A32.pc && b == A32.sp && saved == A32.Reg 2 -> pure ()
    _ -> Left "the IRQ entry does not save the interrupted SP on the IRQ stack at +0x24c"
  pure (sum [reserved instruction | instruction <- instructions])
  where
    reserved instruction = case instruction of
      A32.StoreMultiple _ base' A32.DecrementBefore True regs | base' == A32.sp -> 4 * fromIntegral (length regs)
      A32.Store _ _ _ base' (A32.OffsetImmediate offset) A32.PreIndexed | base' == A32.sp && offset < 0 -> fromIntegral (negate offset :: Int32)
      _ -> 0

decodedIn :: StockCode -> Word32 -> Word32 -> Either String A32.Instruction
decodedIn code base offset = maybe (Left ("no code at " ++ show (base + offset))) (Right . A32.decode (base + offset)) (codeWord code (base + offset))

-- | The IRQ preemption peak on one loader. The first-level exit restores the saved SP
-- (@ldm sp!, {r0}; mov sp, r0@ at base + 0x328) and branches from base + 0x348 to the return
-- dispatch, which leaves SP unchanged through [base + 0x375c, base + 0x3790). The peak is the entry
-- frame, the dispatch's @push {r4-r11}@ at base + 0x3790 (it stores SP to the control block at
-- base + 0x3794), and the dispatch from base + 0x36ac to its switch.
irqPreemptionPeak :: StockCode -> IndirectTargets -> Word32 -> Either String Word32
irqPreemptionPeak code indirect base = do
  entry <- irqEntryFrame code base
  exit <- mapM (decodedIn code base) [0x328, 0x32c, 0x348]
  case exit of
    [ A32.LoadMultiple (A32.Cond 14) b A32.IncrementAfter True [r0]
      , A32.DataProcessing (A32.Cond 14) A32.MOV False rd _ (A32.ShiftedImmediate from A32.LSL 0)
      , A32.Branch (A32.Cond 14) False target ]
      | b == A32.sp && r0 == A32.Reg 0 && rd == A32.sp && from == A32.Reg 0 && target == base + 0x375c -> pure ()
    _ -> Left "the IRQ exit does not restore the saved SP and branch to the return dispatch at +0x328"
  dispatchWords <- mapM (\offset -> maybe (Left ("no code at " ++ show (base + offset))) (Right . (,) (base + offset)) (codeWord code (base + offset))) [0x375c, 0x3760 .. 0x378c]
  unless (all (\(address, word) -> stackNeutral (classify Strict address word)) dispatchWords)
    (Left "the return dispatch changes SP before +0x3790")
  saved <- case (A32.decode (base + 0x3790) <$> codeWord code (base + 0x3790), A32.decode (base + 0x3794) <$> codeWord code (base + 0x3794)) of
    (Just (A32.StoreMultiple (A32.Cond 14) b A32.DecrementBefore True regs), Just (A32.Store (A32.Cond 14) A32.Word rt _ (A32.OffsetImmediate 0x18) A32.OffsetOnly))
      | b == A32.sp && rt == A32.sp -> pure (4 * fromIntegral (length regs))
    _ -> Left "the return dispatch does not save the task context at +0x3790"
  StackDepth dispatch <- either (Left . show) (Right . reportDepth) (stockDepth code (Supplied M.empty) indirect switchModel (base + 0x36ac))
  pure (entry + saved + dispatch)
  where
    stackNeutral effect = case effect of
      Neutral -> True
      Jump _ _ -> True
      _ -> False

-- | Every @b@/@bl@ outside owned code whose target the predicate selects: (site, target).
branchSites :: [Segment] -> [(Word32, Word32)] -> (Word32 -> Bool) -> [(Word32, Word32)]
branchSites segments owned selected =
  [ (site, target) | (site, word) <- codeWords segments, not (inOwned owned site)
  , word `shiftR` 28 /= 15, (word `shiftR` 25) .&. 7 == 5
  , let target = site + 8 + signExtended word, selected target ]
  where
    signExtended word = let imm = word .&. 0xffffff in (if testBit imm 23 then imm .|. 0xff000000 else imm) `shiftL` 2

-- | Every literal word, @movw/movt@ pair (either conditional) and @add@/@sub rd, pc, #imm@ outside owned
-- code whose value (either alias) the predicate selects: (site, value). A pair is a @movt rd@ within 16
-- words after a @movw rd@.
addressFormations :: [Segment] -> [(Word32, Word32)] -> (Word32 -> Bool) -> [(Word32, Word32)]
addressFormations segments owned selected = concatMap scan segments
  where
    scan segment = go M.empty (segmentWords segment)
    go _ [] = []
    go pending ((site, word) : rest)
      | inOwned owned site = go pending rest
      | otherwise =
          let literal = [(site, word) | selected (physical word)]
              (pending', formed) = case A32.decode site word of
                A32.MoveWide _ rd low -> (M.insert rd (site, low) pending, [])
                A32.MoveTop _ rd high -> case M.lookup rd pending of
                  Just (at, low) | site - at <= 64 ->
                    let value = (high `shiftL` 16) .|. low in (M.delete rd pending, [(site, value) | selected (physical value)])
                  _ -> (pending, [])
                A32.DataProcessing _ A32.ADD _ _ rn (A32.Immediate imm) | rn == A32.pc ->
                  (pending, [(site, site + 8 + imm) | selected (physical (site + 8 + imm))])
                A32.DataProcessing _ A32.SUB _ _ rn (A32.Immediate imm) | rn == A32.pc ->
                  (pending, [(site, site + 8 - imm) | selected (physical (site + 8 - imm))])
                _ -> (pending, [])
           in literal ++ formed ++ go pending' rest

-- | Stores outside owned code that may write any byte of the word through a base formed by a
-- @movw/movt@ pair or a pc-relative literal within 4095 bytes of it. Each path from the base is
-- followed for 40 instructions, across both edges of a conditional branch and to the target of an
-- unconditional one, while the base register holds a known value. A register offset counts when the
-- path sets it to a constant; one it does not is a computed pointer, outside the scan.
storesReaching :: [Segment] -> [(Word32, Word32)] -> Word32 -> [Word32]
storesReaching segments owned target = S.toList (S.fromList (concatMap scan segments))
  where
    near value = physical value <= physical target + 4095 && physical value + 4095 >= physical target
    wordAt = segmentWordAt segments
    scan segment = concat [ forward rd value (site + 4) | (site, word) <- segmentWords segment, not (inOwned owned site)
                          , (rd, value) <- formed site (A32.decode site word), near value ]
    formed site instruction = case instruction of
      A32.MoveTop _ rd high -> [ (rd, (high `shiftL` 16) .|. low) | low <- take 1 (movwBefore rd site) ]
      A32.Load _ A32.Word False rd rn (A32.OffsetImmediate offset) A32.OffsetOnly | rn == A32.pc ->
        [ (rd, value) | Just value <- [wordAt (fromIntegral (toInteger site + 8 + toInteger offset))] ]
      _ -> []
    movwBefore rd site = [ low | back <- [1 .. 16 :: Word32], Just w <- [wordAt (site - 4 * back)]
                         , A32.MoveWide _ rd' low <- [A32.decode (site - 4 * back) w], rd' == rd ]
    forward rd value start = go S.empty [(start, value, M.empty, 40 :: Int)]
      where
        go _ [] = []
        go visited ((site, base, known, budget) : rest)
          | budget == 0 || S.member (site, base, known) visited || inOwned owned site = go visited rest
          | otherwise = case wordAt site of
              Nothing -> go visited' rest
              Just word ->
                let instruction = A32.decode site word
                    unconditional = word `shiftR` 28 == 14
                    base' = rebased rd base instruction
                    overwritten = unconditional && rd `elem` writes site word && base' == Just base
                    known' = constantsAfter known site word unconditional instruction
                    successors = [(next, b, known', budget - 1) | not overwritten, Just b <- [base'], next <- following site unconditional instruction]
                 in [site | storesThrough rd base known word instruction] ++ go visited' (successors ++ rest)
          where visited' = S.insert (site, base, known) visited
    -- Registers the path set to constants.
    constantsAfter known site word unconditional instruction = case instruction of
      A32.MoveWide _ rd low | unconditional -> M.insert rd low cleared
      A32.MoveTop _ rd high | unconditional, Just low <- M.lookup rd known -> M.insert rd ((high `shiftL` 16) .|. (low .&. 0xffff)) cleared
      A32.DataProcessing (A32.Cond 14) A32.MOV _ rd _ (A32.Immediate constant) -> M.insert rd constant cleared
      _ -> cleared
      where cleared = foldr M.delete known (writes site word)
    storesThrough rd base known word instruction = case instruction of
      A32.Store _ width _ rn offset indexing | rn == rd -> case offset of
        A32.OffsetImmediate bytes -> overlaps (if indexing == A32.PostIndexed then base else base + fromIntegral bytes) (widthBytes width)
        A32.OffsetRegister add index A32.LSL amount | Just constant <- M.lookup index known ->
          let moved = (if add then (+) else (-)) base (constant `shiftL` amount)
           in overlaps (if indexing == A32.PostIndexed then base else moved) (widthBytes width)
        A32.OffsetRegister {} -> False
      A32.StoreMultiple _ rn mode _ regs | rn == rd ->
        let count = 4 * fromIntegral (length regs)
         in overlaps (case mode of
              A32.IncrementAfter -> base
              A32.IncrementBefore -> base + 4
              A32.DecrementAfter -> base + 4 - count
              A32.DecrementBefore -> base - count) (toInteger count)
      -- @strex@/@strexd@ and @swp@/@swpb@ through the base.
      A32.Rejected _ | (word `shiftR` 16) .&. 0xf == registerNumber rd
                     , word .&. 0x0f900ff0 == 0x01800f90 || word .&. 0x0fb00ff0 == 0x01000090 -> overlaps base 8
      _ -> False
    overlaps address size =
      let start = toInteger (physical address); word = toInteger (physical target)
       in start < word + 4 && word < start + size
    widthBytes width = case width of
      A32.Byte -> 1
      A32.Half -> 2
      A32.Word -> 4
      A32.Dual -> 8
    -- The base after the instruction's writeback of it, when the amount is known.
    rebased rd base instruction = case instruction of
      A32.Store _ _ _ rn offset indexing | rn == rd && indexing /= A32.OffsetOnly -> moved offset
      A32.Load _ _ _ rt rn offset indexing | rn == rd && rt /= rd && indexing /= A32.OffsetOnly -> moved offset
      A32.StoreMultiple _ rn mode True regs | rn == rd -> Just (stepped mode regs)
      A32.LoadMultiple _ rn mode True regs | rn == rd && rd `notElem` regs -> Just (stepped mode regs)
      _ -> Just base
      where
        moved offset = case offset of
          A32.OffsetImmediate bytes -> Just (base + fromIntegral bytes)
          A32.OffsetRegister {} -> Nothing
        stepped mode regs =
          let count = 4 * fromIntegral (length regs)
           in if mode `elem` [A32.IncrementAfter, A32.IncrementBefore] then base + count else base - count
    following site unconditional instruction
      | transfersAway instruction = if unconditional then [] else [site + 4]
      | otherwise = case instruction of
          A32.Branch _ False branchTarget -> if unconditional then [branchTarget] else [site + 4, branchTarget]
          _ -> [site + 4]

registerNumber :: A32.Reg -> Word32
registerNumber (A32.Reg n) = fromIntegral n

-- | Whether the instruction transfers control other than by @b@/@bl@.
transfersAway :: A32.Instruction -> Bool
transfersAway instruction = case instruction of
  A32.BranchExchange _ False _ -> True
  A32.LoadMultiple _ _ _ _ regs -> A32.pc `elem` regs
  A32.Load _ _ _ rt _ _ _ -> rt == A32.pc
  A32.DataProcessing _ _ _ rd _ _ -> rd == A32.pc
  _ -> False

-- | Whether execution can fall through into the address from the instruction before it.
fallsInto :: [Segment] -> Word32 -> Bool
fallsInto segments address = case segmentWordAt segments (address - 4) of
  Nothing -> False
  Just word -> word `shiftR` 28 /= 14 || not (case A32.decode (address - 4) word of
    A32.Branch _ False _ -> True
    instruction -> transfersAway instruction)

-- | The task id a wrapper call site stores to its descriptor: an immediate, or the site when the id is data.
data DescriptorId = ImmediateId Word32 | DataId deriving (Eq, Show)

-- | For each call of the wrapper outside owned code: the descriptor D = r11 - n set by
-- @sub r0, r11, #n@ in the three instructions before the call, the nearest earlier store to
-- @[r11, #-n]@ (backwards until a return or a branch), and the instruction that last wrote its source.
-- The id is data when the store or the source write is conditional, or a branch in the segments
-- targets an address between it and the call.
descriptorIds :: [Segment] -> [(Word32, Word32)] -> [(Word32, DescriptorId)]
descriptorIds segments owned =
  [ (site, identify site) | (site, _) <- branchSites segments owned (== wrapperEntry) ]
  where
    entered = S.fromList (map snd (branchSites segments [] (const True)))
    enteredBetween low high = case S.lookupGT low entered of
      Just address -> address <= high
      Nothing -> False
    decodedAt site = A32.decode site <$> segmentWordAt segments site
    identify site = case [ n | back <- [1 .. 3], Just (A32.DataProcessing (A32.Cond 14) A32.SUB False rd rn (A32.Immediate n)) <- [decodedAt (site - 4 * back)]
                             , rd == A32.Reg 0, rn == A32.Reg 11 ] of
      n : _ -> maybe DataId id (storeOf site n (site - 4) (80 :: Int))
      [] -> DataId
    storeOf _ _ _ 0 = Nothing
    storeOf call n site budget = case decodedAt site of
      Nothing -> Nothing
      Just instruction
        | boundary instruction -> Nothing
        | A32.Store cond _ rt rn (A32.OffsetImmediate offset) A32.OffsetOnly <- instruction
        , rn == A32.Reg 11, toInteger offset == negate (toInteger n) ->
            Just (if cond /= A32.Cond 14 || enteredBetween site call then DataId else sourceOf site rt (site - 4) (40 :: Int))
        | otherwise -> storeOf call n (site - 4) (budget - 1)
    sourceOf _ _ _ 0 = DataId
    sourceOf store register site budget = case decodedAt site of
      Nothing -> DataId
      Just instruction
        | boundary instruction -> DataId
        | register `elem` written instruction -> if enteredBetween site store then DataId else case instruction of
            A32.DataProcessing (A32.Cond 14) A32.MOV _ _ _ (A32.Immediate value) -> ImmediateId value
            A32.MoveWide (A32.Cond 14) _ value -> ImmediateId value
            _ -> DataId
        | otherwise -> sourceOf store register (site - 4) (budget - 1)
    boundary instruction = case instruction of
      A32.Branch _ False _ -> True
      A32.BranchExchange _ False _ -> True
      A32.LoadMultiple _ _ _ _ regs -> A32.pc `elem` regs
      A32.StoreMultiple _ base A32.DecrementBefore True regs -> base == A32.sp && A32.lr `elem` regs
      _ -> False

segmentWordAt :: [Segment] -> Word32 -> Maybe Word32
segmentWordAt segments site = case [word32 bytes (fromIntegral (site - base)) | Segment base bytes <- segments, site >= base, site - base + 4 <= fromIntegral (B.length bytes)] of
  word : _ -> Just word
  [] -> Nothing

codeWords :: [Segment] -> [(Word32, Word32)]
codeWords = concatMap segmentWords

segmentWords :: Segment -> [(Word32, Word32)]
segmentWords (Segment base bytes) = [(base + fromIntegral offset, word32 bytes offset) | offset <- [0, 4 .. B.length bytes - 4]]

word32 :: B.ByteString -> Int -> Word32
word32 bytes offset =
  let byte index = fromIntegral (BU.unsafeIndex bytes (offset + index)) :: Word32
   in byte 0 .|. (byte 1 `shiftL` 8) .|. (byte 2 `shiftL` 16) .|. (byte 3 `shiftL` 24)

inOwned :: [(Word32, Word32)] -> Word32 -> Bool
inOwned owned site = any (\(low, high) -> low <= site && site < high) owned
