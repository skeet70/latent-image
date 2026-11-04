-- | Stock code views, register-branch targets and byte scans for the J7 stack and caller checks.
module Replacement.StartupArtifact.StackBounds
  ( Segment (..), segmentsOf, stockCodeOf, ownedRanges
  , rootTable, rootSlotTargets, indirectTargetsOf, switchModel
  , irqEntryFrame, irqPreemptionPeak, irqPreemptionBytes
  , branchSites, addressFormations, storesReaching
  , DescriptorId (..), descriptorIds
  , wrapperEntry, storagePrelude, releaseEntry, cleanupEntry
  , errorCallbackWord, errorCallbackSetter, errorDispatchSite, errorCallbacks
  , traceCallbackWord, traceCallbackSetter, traceDispatchSites, traceCallbacks
  , fillFlag, fillDoneCallback
  ) where

import qualified A32
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Unsafe as BU
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Replacement.StartupArtifact.Targets (TargetBytes, targetBase, targetOf)
import Replacement.StartupArtifact.Types (Guest (..), Span (..), Target (..))
import StackWalk (written)
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
kernelInitializers :: [((Word32, Word32), (Word32, Word32))]
kernelInitializers = [((0x40000, 0x4dae0), (0x43ac0, 0x43c48)), ((0xd0000, 0xddaa0), (0xd3a7c, 0xd3c04))]

-- | Error dispatch @dispatch_error@ calls the word at 0x4089ea8c (x70view 55436fc85c
-- app/platform/osal/interface_ring_operations.c:42-45, exact; id4.dis 0x7233a0-0x7233ac).
errorCallbackWord, errorCallbackSetter, errorDispatchSite :: Word32
errorCallbackWord = 0x4089ea8c
errorCallbackSetter = 0x725b54
errorDispatchSite = 0x7233ac

-- | The canonical data word 0x723330 and root's argument 0x72835c (RootTask/Admission.hs:153-154).
errorCallbacks :: [Word32]
errorCallbacks = [0x723330, 0x72835c]

-- | The log path calls the word at 0x4004f15c when nonzero; 0x41548 sets it (kernel0 native_globals.c:57-63, exact).
traceCallbackWord, traceCallbackSetter :: Word32
traceCallbackWord = 0x4004f15c
traceCallbackSetter = 0x41548

traceDispatchSites, traceCallbacks :: [Word32]
traceDispatchSites = [0x41598, 0xd1598]
traceCallbacks = [0x6470fc]

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
  locks <- mapM (\(range, initializer) -> (,) range <$> contextLockMethods code initializer) kernelInitializers
  pure (IndirectTargets rootTable slots callbacks locks)
  where
    callbacks = M.fromList ((errorDispatchSite, (errorCallbacks, ErrorCallbackClosed))
      : [(site, (traceCallbacks, TraceCallbackClosed)) | site <- traceDispatchSites])

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
-- stack at loader base + 0x250: the reservations of [base + 0x204, base + 0x250).
irqEntryFrame :: StockCode -> Word32 -> Either String Word32
irqEntryFrame code base = do
  let at offset = maybe (Left ("no code at " ++ show (base + offset))) (Right . A32.decode (base + offset)) (codeWord code (base + offset))
  instructions <- mapM at [0x204, 0x208 .. 0x24c]
  switch <- at 0x250
  case switch of
    A32.Load (A32.Cond 14) A32.Word False rt rn _ A32.OffsetOnly | rt == A32.sp && rn == A32.pc -> pure ()
    _ -> Left "the IRQ entry does not load the IRQ stack at +0x250"
  pure (sum [reserved instruction | instruction <- instructions])
  where
    reserved instruction = case instruction of
      A32.StoreMultiple _ base' A32.DecrementBefore True regs | base' == A32.sp -> 4 * fromIntegral (length regs)
      A32.Store _ _ _ base' (A32.OffsetImmediate offset) A32.PreIndexed | base' == A32.sp && offset < 0 -> fromIntegral (negate offset :: Int32)
      _ -> 0

-- | The IRQ preemption peak on one loader: the entry frame, the return dispatch's
-- @push {r4-r11}@ at base + 0x3790 (it stores SP to the control block at base + 0x3794), and the
-- dispatch from base + 0x36ac to its switch.
irqPreemptionPeak :: StockCode -> IndirectTargets -> Word32 -> Either String Word32
irqPreemptionPeak code indirect base = do
  entry <- irqEntryFrame code base
  saved <- case (A32.decode (base + 0x3790) <$> codeWord code (base + 0x3790), A32.decode (base + 0x3794) <$> codeWord code (base + 0x3794)) of
    (Just (A32.StoreMultiple (A32.Cond 14) b A32.DecrementBefore True regs), Just (A32.Store (A32.Cond 14) A32.Word rt _ (A32.OffsetImmediate 0x18) A32.OffsetOnly))
      | b == A32.sp && rt == A32.sp -> pure (4 * fromIntegral (length regs))
    _ -> Left "the return dispatch does not save the task context at +0x3790"
  StackDepth dispatch <- either (Left . show) (Right . reportDepth) (stockDepth code (Supplied M.empty) indirect switchModel (base + 0x36ac))
  pure (entry + saved + dispatch)

-- | Every @b@/@bl@ outside owned code whose target the predicate selects: (site, target).
branchSites :: [Segment] -> [(Word32, Word32)] -> (Word32 -> Bool) -> [(Word32, Word32)]
branchSites segments owned selected =
  [ (site, target) | (site, word) <- codeWords segments, not (inOwned owned site)
  , word `shiftR` 28 /= 15, (word `shiftR` 25) .&. 7 == 5
  , let target = site + 8 + signExtended word, selected target ]
  where
    signExtended word = let imm = word .&. 0xffffff in (if testBit imm 23 then imm .|. 0xff000000 else imm) `shiftL` 2

-- | Every literal word, @movw/movt@ pair and @add rd, pc, #imm@ outside owned code whose value (either
-- alias) the predicate selects: (site, value). A pair is a @movt rd@ within 16 words after a @movw rd@.
addressFormations :: [Segment] -> [(Word32, Word32)] -> (Word32 -> Bool) -> [(Word32, Word32)]
addressFormations segments owned selected = concatMap scan segments
  where
    scan segment = go M.empty (segmentWords segment)
    go _ [] = []
    go pending ((site, word) : rest)
      | inOwned owned site = go pending rest
      | otherwise =
          let literal = [(site, word) | selected (physical word)]
              instruction = A32.decode site word
              (pending', formed) = case instruction of
                A32.MoveWide (A32.Cond 14) rd low -> (M.insert rd (site, low) pending, [])
                A32.MoveTop (A32.Cond 14) rd high -> case M.lookup rd pending of
                  Just (at, low) | site - at <= 64 ->
                    let value = (high `shiftL` 16) .|. low in (M.delete rd pending, [(site, value) | selected (physical value)])
                  _ -> (pending, [])
                A32.DataProcessing (A32.Cond 14) A32.ADD False _ rn (A32.Immediate imm) | rn == A32.pc ->
                  (pending, [(site, site + 8 + imm) | selected (physical (site + 8 + imm))])
                _ -> (pending, [])
           in literal ++ formed ++ go pending' rest

-- | Stores outside owned code that reach the word through a base formed by a @movw/movt@ pair or a
-- pc-relative literal within 4095 bytes of it: the forward 40 instructions after the base while the
-- base register keeps it, any @str@/@strb@/@strh@ whose immediate offset lands on the word.
storesReaching :: [Segment] -> [(Word32, Word32)] -> Word32 -> [Word32]
storesReaching segments owned target = concatMap scan segments
  where
    near value = physical value <= physical target + 4095 && physical value + 4095 >= physical target
    scan segment@(Segment base bytes) =
      let wordAt site = if site >= base && site - base + 4 <= fromIntegral (B.length bytes) then Just (word32 bytes (fromIntegral (site - base))) else Nothing
          bases = [ (site, rd, value) | (site, word) <- segmentWords segment, not (inOwned owned site)
                  , (rd, value) <- formed wordAt site (A32.decode site word), near value ]
       in concat [ forward wordAt rd value (site + 4) (40 :: Int) | (site, rd, value) <- bases ]
    formed wordAt site instruction = case instruction of
      A32.MoveTop (A32.Cond 14) rd high ->
        [ (rd, (high `shiftL` 16) .|. low) | back <- take 1 [b | b <- [1 .. 16 :: Word32], Just w <- [wordAt (site - 4 * b)], isMovw rd (A32.decode (site - 4 * b) w)]
        , Just w <- [wordAt (site - 4 * back)], A32.MoveWide _ _ low <- [A32.decode (site - 4 * back) w] ]
      A32.Load (A32.Cond 14) A32.Word False rd rn (A32.OffsetImmediate offset) A32.OffsetOnly | rn == A32.pc ->
        [ (rd, value) | Just value <- [wordAt (fromIntegral (toInteger site + 8 + toInteger offset))] ]
      _ -> []
    isMovw rd instruction = case instruction of
      A32.MoveWide _ rd' _ -> rd' == rd
      _ -> False
    forward _ _ _ _ 0 = []
    forward wordAt rd value site budget = case A32.decode site <$> wordAt site of
      Nothing -> []
      Just instruction ->
        let stores = case instruction of
              A32.Store _ _ _ rn (A32.OffsetImmediate offset) _ | rn == rd && physical (value + fromIntegral offset) == physical target -> [site]
              _ -> []
            stops = rd `elem` written instruction || isTransfer instruction
         in stores ++ (if stops then [] else forward wordAt rd value (site + 4) (budget - 1))
    isTransfer instruction = case instruction of
      A32.Branch _ False _ -> True
      A32.BranchExchange _ False _ -> True
      A32.LoadMultiple _ _ _ _ regs -> A32.pc `elem` regs
      _ -> False

-- | The task id a wrapper call site stores to its descriptor: an immediate, or the site when the id is data.
data DescriptorId = ImmediateId Word32 | DataId deriving (Eq, Show)

-- | For each call of the wrapper outside owned code: the descriptor D = r11 - n set by
-- @sub r0, r11, #n@ in the three instructions before the call, the nearest earlier store to
-- @[r11, #-n]@ (backwards until a return or a branch), and the instruction that last wrote its source.
descriptorIds :: [Segment] -> [(Word32, Word32)] -> [(Word32, DescriptorId)]
descriptorIds segments owned =
  [ (site, identify site) | (site, _) <- branchSites segments owned (== wrapperEntry) ]
  where
    wordAt site = case [word32 bytes (fromIntegral (site - base)) | Segment base bytes <- segments, site >= base, site - base + 4 <= fromIntegral (B.length bytes)] of
      word : _ -> Just word
      [] -> Nothing
    decodedAt site = A32.decode site <$> wordAt site
    identify site = case [ n | back <- [1 .. 3], Just (A32.DataProcessing (A32.Cond 14) A32.SUB False rd rn (A32.Immediate n)) <- [decodedAt (site - 4 * back)]
                             , rd == A32.Reg 0, rn == A32.Reg 11 ] of
      n : _ -> maybe DataId id (storeOf n (site - 4) (80 :: Int))
      [] -> DataId
    storeOf _ _ 0 = Nothing
    storeOf n site budget = case decodedAt site of
      Nothing -> Nothing
      Just instruction
        | boundary instruction -> Nothing
        | A32.Store (A32.Cond 14) A32.Word rt rn (A32.OffsetImmediate offset) A32.OffsetOnly <- instruction
        , rn == A32.Reg 11, toInteger offset == negate (toInteger n) -> Just (sourceOf rt (site - 4) (40 :: Int))
        | otherwise -> storeOf n (site - 4) (budget - 1)
    sourceOf _ _ 0 = DataId
    sourceOf register site budget = case decodedAt site of
      Nothing -> DataId
      Just instruction
        | boundary instruction -> DataId
        | register `elem` written instruction -> case instruction of
            A32.DataProcessing (A32.Cond 14) A32.MOV _ _ _ (A32.Immediate value) -> ImmediateId value
            A32.MoveWide (A32.Cond 14) _ value -> ImmediateId value
            _ -> DataId
        | otherwise -> sourceOf register (site - 4) (budget - 1)
    boundary instruction = case instruction of
      A32.Branch _ False _ -> True
      A32.BranchExchange _ False _ -> True
      A32.LoadMultiple _ _ _ _ regs -> A32.pc `elem` regs
      A32.StoreMultiple _ base A32.DecrementBefore True regs -> base == A32.sp && A32.lr `elem` regs
      _ -> False

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
