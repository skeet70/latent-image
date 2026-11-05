{-# LANGUAGE OverloadedStrings #-}
-- | The kernel-slot-bodies footprint and the A32 facts its joins check: the
-- slot bodies' calling-convention walk, the direct and root-slot calls that
-- enter the slot extents, and caller-saved liveness after those calls.
module Replacement.StartupArtifact.KernelSlots
  ( footprint, CodeImage (..), codeWord
  , Exits, walkAbi, changedState, differingState
  , DirectCall (..), directCalls, SlotCall (..), slotCalls, rootSlotAt
  , directImageCalls, slotImageCalls
  , RootTargets, rootTargets, publishedRows, rootRows, Entered (..), deadAfter, liveAfter
  ) where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.LoaderPrefixWitness as P
import qualified Emulation.PrimaryInitializerSource as PS
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.StartupArtifact.Targets (targetBase)
import Replacement.StartupArtifact.Types

-- | The canonical loader pins the leaf admission reads.
footprint :: [Footprint]
footprint =
  [ Footprint (Span LoaderA (Guest 0x40000) (Guest 0x4dae0)) AdmissionRead (Citation (citation "A"))
  , Footprint (Span LoaderB (Guest 0xd0000) (Guest 0xddaa0)) AdmissionRead (Citation (citation "B")) ]
  where
    citation side = "replacement/rtos_own_first/stage/Replacement/RtosOwnFirst/Artifact.hs:198-203,318 (whole loader-" <> side
      <> " sha256 pin and contained-loader equality)"

-- | Bytes at their guest base.
data CodeImage = CodeImage Word32 B.ByteString

codeWord :: CodeImage -> Word32 -> Maybe Word32
codeWord (CodeImage base bytes) address
  | address >= base && address .&. 3 == 0 && offset + 4 <= B.length bytes = Just (U.word32At bytes offset)
  | otherwise = Nothing
  where offset = fromIntegral (address - base)

hex :: Word32 -> Text
hex value = "0x" <> T.pack (showHex value "")

-- | A register value: the caller's register at entry, an offset from the entry SP, a constant, or anything.
data Value = Original Int | Frame Int | Constant Word32 | Unknown deriving (Eq, Ord, Show)

data State = State { statePc :: Word32, stateRegisters :: M.Map Int Value, stateStack :: M.Map Int Value, stateFlags :: Bool }
  deriving (Eq, Ord, Show)

registerOf :: State -> A.Reg -> Value
registerOf state (A.Reg n) = M.findWithDefault Unknown n (stateRegisters state)

setRegister :: A.Reg -> Value -> State -> State
setRegister (A.Reg n) value state = state {stateRegisters = M.insert n value (stateRegisters state)}

numbered :: A.Reg -> Int
numbered (A.Reg n) = n

-- | MRC p14/p15 destination, or Nothing for any other coprocessor word.
mrcDestination :: Word32 -> Maybe A.Reg
mrcDestination word
  | word .&. 0x0f100010 == 0x0e100010 = Just (A.Reg (fromIntegral ((word `shiftR` 12) .&. 0xf)))
  | otherwise = Nothing

scratch :: [(Int, Clobber)]
scratch = [(1, ClobberR1), (2, ClobberR2), (3, ClobberR3), (12, ClobberR12)]

-- | The r1, r2, r3 and r12 values at one return, and whether the flags were written on its path.
data Exit = Exit (M.Map Int Value) Bool deriving (Eq, Ord, Show)

-- | Every return a walked body can reach.
newtype Exits = Exits (S.Set Exit) deriving (Eq, Show)

-- | Caller-saved state some return leaves different from its entry value.
changedState :: Exits -> [Clobber]
changedState (Exits exits) =
  [clobber | (n, clobber) <- scratch, any (\(Exit values _) -> M.lookup n values /= Just (Original n)) (S.toList exits)]
    ++ [ClobberFlags | any (\(Exit _ flags) -> flags) (S.toList exits)]

-- | Caller-saved state whose value at return can differ between two bodies of one slot: a register unless both
-- leave the same entry value or constant at every return, and the flags if either writes them.
differingState :: Exits -> Exits -> [Clobber]
differingState (Exits left) (Exits right) =
  [clobber | (n, clobber) <- scratch, not (settled n)] ++ [ClobberFlags | any written (S.toList left ++ S.toList right)]
  where
    settled n = case S.toList (S.fromList [M.lookup n values | Exit values _ <- S.toList left ++ S.toList right]) of
      [Just value] -> value /= Unknown
      _ -> False
    written (Exit _ flags) = flags

-- | Returns a body can reach, and the (argument register, byte offset) words it stores through its r0-r3 arguments.
data Walk = Walk (S.Set Exit) (S.Set (Int, Int))

instance Semigroup Walk where
  Walk a b <> Walk c d = Walk (S.union a c) (S.union b d)

-- | Walks a slot body from its entry over the words of its loader and returns its exits. Refuses unless every
-- return restores r4-r11 and SP and returns to the entry LR. Calls must target an address the predicate admits,
-- which is walked in turn; branches must stay inside the given extents.
walkAbi :: CodeImage -> [(Word32, Word32)] -> (Word32 -> Bool) -> Word32 -> Either Text Exits
walkAbi code extents callable entry = (\(Walk exits _) -> Exits exits) <$> go (4 :: Int) entry
  where
    owned address = any (\(low, high) -> address >= low && address < high) extents
    go depth start = do
      when (depth == 0) (Left ("slot call depth exceeded at " <> hex start))
      unless (owned start) (Left ("walk entry outside the slot extents: " <> hex start))
      let initial = State start (M.fromList ((13, Frame 0) : [(n, Original n) | n <- [0 .. 12] ++ [14]])) M.empty False
      explore depth S.empty [initial] (Walk S.empty S.empty)
    explore _ _ [] found = Right found
    explore depth seen (state : pending) found
      | S.member state seen = explore depth seen pending found
      | S.size seen > 20000 = Left "slot walk state bound exceeded"
      | otherwise = do
          let pc = statePc state
          unless (owned pc) (Left ("slot flow leaves the extents at " <> hex pc))
          word <- maybe (Left ("slot word unreadable at " <> hex pc)) Right (codeWord code pc)
          let instruction = A.decode pc word
              skipped = [advance state | maybe False (/= A.Cond 14) (condOf instruction)]
          (next, returned, stores) <- execute depth pc word instruction state
          exits <- mapM (returnExit pc) returned
          explore depth (S.insert state seen) (next ++ skipped ++ pending) (found <> Walk (S.fromList exits) (S.fromList stores))
    returnExit pc state = do
      unless (registerOf state A.sp == Frame 0) (Left ("slot return changes SP at " <> hex pc))
      unless (all (\n -> registerOf state (A.Reg n) == Original n) [4 .. 11]) (Left ("slot return changes r4-r11 at " <> hex pc))
      pure (Exit (M.fromList [(n, registerOf state (A.Reg n)) | (n, _) <- scratch]) (stateFlags state))
    advance state = state {statePc = statePc state + 4}
    continue state = Right ([advance state], [], [])
    finish pc state value
      | value == Original 14 = Right ([], [state], [])
      | otherwise = Left ("slot return target is not the entry LR at " <> hex pc)
    execute depth pc word instruction state = case instruction of
      A.DataProcessing _ op setsFlags destination source operand -> do
        let flagged = state {stateFlags = stateFlags state || setsFlags || op `elem` [A.CMP, A.CMN, A.TST, A.TEQ]}
            value = case (op, operand) of
              (A.MOV, A.ShiftedImmediate register A.LSL 0) -> registerOf state register
              (A.MOV, A.Immediate k) -> Constant k
              (A.MVN, A.Immediate k) -> Constant (complement k)
              (A.ADD, A.Immediate k) | Frame o <- registerOf state source -> Frame (o + fromIntegral k)
              (A.SUB, A.Immediate k) | Frame o <- registerOf state source -> Frame (o - fromIntegral k)
              _ -> Unknown
        if op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] then continue flagged else do
          when (destination == A.pc) (Left ("slot data-processing write to pc at " <> hex pc))
          when (destination == A.sp && not (isFrame value)) (Left ("slot SP leaves its frame provenance at " <> hex pc))
          continue (setRegister destination value flagged)
      A.MoveWide _ destination k -> do
        when (destination == A.sp) (Left ("slot write to sp at " <> hex pc))
        continue (setRegister destination (Constant k) state)
      A.MoveTop _ destination k -> do
        when (destination == A.sp) (Left ("slot write to sp at " <> hex pc))
        continue (setRegister destination (case registerOf state destination of
          Constant low -> Constant ((low .&. 0xffff) .|. (k `shiftL` 16))
          _ -> Unknown) state)
      A.ReadStatus _ destination -> written [destination]
      A.ZeroExtend _ destination _ _ _ -> written [destination]
      A.Multiply _ destination _ _ _ -> written [destination]
      A.Compute _ destinations -> written destinations
      A.WriteApplicationFlags _ _ -> continue state {stateFlags = True}
      A.Hint _ -> continue state
      A.Load _ width _ destination base offset indexing -> do
        when (destination `elem` [A.pc, A.sp]) (Left ("slot load to sp or pc at " <> hex pc))
        when (indexing /= A.OffsetOnly && base == destination) (Left ("slot load writes back its destination at " <> hex pc))
        let baseValue = registerOf state base
            displacement = case offset of
              A.OffsetImmediate k -> Just (fromIntegral k)
              _ -> Nothing
            loaded = case (baseValue, displacement) of
              (Frame o, Just k) | width == A.Word -> M.findWithDefault Unknown (if indexing == A.PostIndexed then o else o + k) (stateStack state)
              _ -> Unknown
            cleared = foldl' (\s r -> setRegister r Unknown s) state (destination : [A.Reg (numbered destination + 1) | width == A.Dual])
            updated = setRegister destination loaded cleared
        if indexing == A.OffsetOnly then continue updated else case (baseValue, displacement) of
          (Frame o, Just k) -> continue (setRegister base (Frame (o + k)) updated)
          _ | base == A.sp -> Left ("slot SP writeback leaves its frame provenance at " <> hex pc)
            | otherwise -> continue (setRegister base Unknown updated)
      A.Store _ width source base offset indexing -> do
        let baseValue = registerOf state base
        k <- case offset of
          A.OffsetImmediate k -> Right (fromIntegral k)
          _ | isFrame baseValue || isArgument baseValue -> Left ("slot stack or argument store with a register offset at " <> hex pc)
            | otherwise -> Right 0
        let at' = if indexing == A.PostIndexed then 0 else k
            wordsAt slot = slot : [slot + 4 | width == A.Dual]
            moved s
              | indexing == A.OffsetOnly = Right s
              | Frame o <- baseValue = Right (setRegister base (Frame (o + k)) s)
              | base == A.sp || isArgument baseValue = Left ("slot store writeback form outside the walked forms at " <> hex pc)
              | otherwise = Right (setRegister base Unknown s)
        case baseValue of
          Frame o -> do
            let values = zip (wordsAt (o + at')) (if width `elem` [A.Word, A.Dual] then map (registerOf state) (source : [A.Reg (numbered source + 1) | width == A.Dual]) else [Unknown])
            stored <- moved state {stateStack = foldl' (\m (slot, v) -> M.insert slot v m) (stateStack state) values}
            continue stored
          Original n | n <= 3 -> do
            stored <- moved state
            Right ([advance stored], [], [(n, slot) | slot <- wordsAt at'])
          _ -> moved state >>= continue
      A.StoreMultiple _ base mode writeback registers
        | base == A.sp, Frame o <- registerOf state A.sp, mode == A.DecrementBefore -> do
            let low = o - 4 * length registers
                stack = foldl' (\m (k, r) -> M.insert k (registerOf state r) m) (stateStack state) (zip [low, low + 4 ..] registers)
                updated = state {stateStack = stack}
            continue (if writeback then setRegister A.sp (Frame low) updated else updated)
        | isFrame (registerOf state base) || isArgument (registerOf state base) -> Left ("slot store-multiple form outside the walked forms at " <> hex pc)
        | otherwise -> continue state
      A.LoadMultiple _ base mode writeback registers
        | base == A.sp, Frame o <- registerOf state A.sp, mode == A.IncrementAfter -> do
            let values = zip registers [M.findWithDefault Unknown k (stateStack state) | k <- [o, o + 4 ..]]
                top = o + 4 * length registers
                loaded = foldl' (\s (r, v) -> if r == A.pc then s else setRegister r v s) state values
                updated = if writeback then setRegister A.sp (Frame top) loaded else loaded
            case lookup A.pc values of
              Just value -> finish pc updated value
              Nothing -> continue updated
        | A.pc `elem` registers -> Left ("slot return form outside the walked forms at " <> hex pc)
        | otherwise -> written registers
      A.Branch _ False target
        | owned target -> Right ([state {statePc = target}], [], [])
        | otherwise -> Left ("slot branch leaves the extents at " <> hex pc)
      A.Branch _ True target -> do
        unless (callable target) (Left ("slot call to an unadmitted target " <> hex target <> " at " <> hex pc))
        Walk callee stores <- go (depth - 1) target
        let through = [(registerOf state (A.Reg n), k) | (n, k) <- S.toList stores]
            stack = foldl' (\m (o, k) -> M.insert (o + k) Unknown m) (stateStack state) [(o, k) | (Frame o, k) <- through]
            returned = foldl' (\s n -> setRegister (A.Reg n) Unknown s) state {stateStack = stack} [0, 1, 2, 3, 12, 14]
            calleeFlags = any (\(Exit _ flags) -> flags) (S.toList callee)
        Right ( [advance returned {stateFlags = stateFlags state || calleeFlags}], []
              , [(n, k) | (Original n, k) <- through, n <= 3] )
      A.BranchExchange _ False register -> finish pc state (registerOf state register)
      A.BranchExchange _ True _ -> Left ("slot indirect call at " <> hex pc)
      A.Rejected _
        | Just destination <- mrcDestination word -> do
            when (destination == A.pc) (Left ("slot MRC to the flags at " <> hex pc))
            written [destination]
        | Just (A.StatusWrite _ A.CurrentStatus fields _) <- A.statusWrite word ->
            continue state {stateFlags = stateFlags state || A.FlagBits `elem` fields}
        | otherwise -> Left ("slot instruction outside the walked forms at " <> hex pc)
      where
        written registers = do
          when (any (`elem` [A.sp, A.pc]) registers) (Left ("slot write to sp or pc at " <> hex pc))
          continue (foldl' (\s r -> setRegister r Unknown s) state registers)
    isFrame value = case value of
      Frame _ -> True
      _ -> False
    isArgument value = case value of
      Original n -> n <= 3
      _ -> False

condOf :: A.Instruction -> Maybe A.Cond
condOf instruction = case instruction of
  A.DataProcessing c _ _ _ _ _ -> Just c
  A.MoveWide c _ _ -> Just c
  A.MoveTop c _ _ -> Just c
  A.ReadStatus c _ -> Just c
  A.WriteApplicationFlags c _ -> Just c
  A.ZeroExtend c _ _ _ _ -> Just c
  A.Multiply c _ _ _ _ -> Just c
  A.Compute c _ -> Just c
  A.Load c _ _ _ _ _ _ -> Just c
  A.Store c _ _ _ _ _ -> Just c
  A.LoadMultiple c _ _ _ _ -> Just c
  A.StoreMultiple c _ _ _ _ -> Just c
  A.Branch c _ _ -> Just c
  A.BranchExchange c _ _ -> Just c
  A.Hint c -> Just c
  A.Rejected _ -> Nothing

-- | A direct branch or call into a slot carrier from outside every slot carrier.
data DirectCall = DirectCall { directSite :: Word32, directTarget :: Word32, directLinked :: Bool } deriving (Eq, Show)

-- | Every aligned word of the spans, read from the targets, that decodes to a B or BL into the carriers from outside them.
directCalls :: (Target -> B.ByteString) -> [Span] -> [Span] -> [DirectCall]
directCalls bytesOf spans = directImageCalls
  [CodeImage start (B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - targetBase owner)) (bytesOf owner)))
  | Span owner (Guest start) (Guest end) <- spans]

directImageCalls :: [CodeImage] -> [Span] -> [DirectCall]
directImageCalls images carriers =
  [ DirectCall site target linked
  | image@(CodeImage start bytes) <- images
  , B.length bytes >= 4
  , let end = start + fromIntegral (B.length bytes)
  , site <- [alignUp start, alignUp start + 4 .. end - 4]
  , not (inside site)
  , Just word <- [codeWord image site]
  , A.Branch _ linked target <- [A.decode site word], inside target ]
  where
    inside address = any (\(Span _ (Guest low) (Guest high)) -> address >= low && address < high) carriers

alignUp :: Word32 -> Word32
alignUp address = (address + 3) .&. complement 3

-- | A @blx@ whose register was just loaded from an interface-root slot.
data SlotCall = SlotCall { slotSite :: Word32, slotOffset :: RootSlot } deriving (Eq, Show)

-- | Root-slot calls in the spans: @ldr rN,[rM,#k]@ or @movw rK,#k; ldr rN,[rM,rK]@ within the six words before @blx rN@.
slotCalls :: (Target -> B.ByteString) -> [Span] -> [SlotCall]
slotCalls bytesOf spans = slotImageCalls
  [CodeImage start (B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - targetBase target)) (bytesOf target)))
  | Span target (Guest start) (Guest end) <- spans]

slotImageCalls :: [CodeImage] -> [SlotCall]
slotImageCalls images =
  [ SlotCall site offset
  | image@(CodeImage start bytes) <- images
  , B.length bytes >= 4
  , let word = codeWord image
        end = start + fromIntegral (B.length bytes)
  , site <- [alignUp start, alignUp start + 4 .. end - 4], Just offset <- [slotOffsetAt word (alignUp start) site] ]

-- | The root-slot offset a @blx@ at the site loads its target from, reading no word below the floor.
slotOffsetAt :: (Word32 -> Maybe Word32) -> Word32 -> Word32 -> Maybe RootSlot
slotOffsetAt word floor' site = case decoded site of
  Just (A.BranchExchange _ True register) -> RootSlot <$> offsetOf register
  _ -> Nothing
  where
    decoded address = A.decode address <$> word address
    before = [(earlier, instruction) | earlier <- [site - 4, site - 8 .. site - 24], earlier >= floor', earlier < site, Just instruction <- [decoded earlier]]
    writesTo register instruction = case instruction of
      A.Load _ _ _ destination _ _ _ -> destination == register
      A.MoveWide _ destination _ -> destination == register
      A.MoveTop _ destination _ -> destination == register
      A.DataProcessing _ _ _ destination _ _ -> destination == register
      _ -> False
    lastWrite register earlier = take 1 [instruction | (_, instruction) <- earlier, writesTo register instruction]
    offsetOf register = case lastWrite register before of
      [A.Load _ A.Word _ _ base (A.OffsetImmediate k) A.OffsetOnly] | k >= 0, base /= A.pc -> Just (fromIntegral k)
      [A.Load _ A.Word _ _ base (A.OffsetRegister True index A.LSL 0) A.OffsetOnly] | base /= A.pc ->
        case lastWrite index (drop 1 (dropWhile (not . writesTo register . snd) before)) of
          [A.MoveWide _ _ k] -> Just k
          _ -> Nothing
      _ -> Nothing

-- | The interface-root array of root-object pointers, indexed by core.
rootArray :: Word32
rootArray = 0x96bd20

-- | The root slot a @blx@ calls when the straight-line code before it, back to the previous branch or 16 words,
-- loads the target from offset k of a root object whose pointer it loaded from 'rootArray' at index 0, 1 or a
-- register scaled by 4. Each register's value comes from its last unconditional writer.
rootSlotAt :: (Word32 -> Maybe Word32) -> Word32 -> Maybe RootSlot
rootSlotAt word site = case decoded site of
  Just (A.BranchExchange (A.Cond 14) True register) -> do
    (offset, base, earlier) <- slotLoad register (window (site - 4) (16 :: Int))
    ((_, _, pointerLoad), rest) <- writer base earlier
    case pointerLoad of
      A.Load _ A.Word _ _ array index A.OffsetOnly | indexed index -> do
        value <- constant array rest
        if value == rootArray then Just (RootSlot offset) else Nothing
      _ -> Nothing
  _ -> Nothing
  where
    decoded address = A.decode address <$> word address
    window _ 0 = []
    window address n = case (word address, decoded address) of
      (Just raw, Just instruction)
        | breaks instruction -> []
        | otherwise -> (address, raw, instruction) : window (address - 4) (n - 1)
      _ -> []
    breaks instruction = case instruction of
      A.Branch _ False _ -> True
      A.BranchExchange _ False _ -> True
      A.LoadMultiple _ _ _ _ listed -> A.pc `elem` listed
      _ -> False
    writesTo register (_, raw, instruction) =
      let Writes _ targets _ _ = instructionWrites raw instruction
          calls = case instruction of
            A.Branch _ True _ -> True
            A.BranchExchange _ True _ -> True
            _ -> False
      in S.member (numbered register) targets || (calls && numbered register `elem` [0, 1, 2, 3, 12, 14])
    -- The last writer of a register and the words before it, or Nothing when that writer is conditional or absent.
    writer register earlier = case dropWhile (not . writesTo register) earlier of
      found@(_, _, instruction) : rest | condOf instruction == Just (A.Cond 14) -> Just (found, rest)
      _ -> Nothing
    indexed index = case index of
      A.OffsetImmediate k -> k == 0 || k == 4
      A.OffsetRegister True _ A.LSL 2 -> True
      _ -> False
    slotLoad register earlier = do
      ((_, _, instruction), rest) <- writer register earlier
      case instruction of
        A.Load _ A.Word _ _ base (A.OffsetImmediate k) A.OffsetOnly | k >= 0, base /= A.pc -> Just (fromIntegral k, base, rest)
        A.Load _ A.Word _ _ base (A.OffsetRegister True index A.LSL 0) A.OffsetOnly | base /= A.pc -> do
          k <- constant index rest
          Just (k, base, rest)
        _ -> Nothing
    constant register earlier = do
      ((address, _, instruction), rest) <- writer register earlier
      case instruction of
        A.MoveWide _ _ k -> Just k
        A.MoveTop _ _ high -> do
          low <- constant register rest
          Just ((high `shiftL` 16) .|. (low .&. 0xffff))
        A.Load _ A.Word _ _ base (A.OffsetImmediate k) A.OffsetOnly | base == A.pc -> word (address + 8 + fromIntegral k)
        _ -> Nothing

-- | An APSR condition flag.
data FlagBit = FlagN | FlagZ | FlagC | FlagV deriving (Eq, Ord, Show, Enum, Bounded)

allFlags :: S.Set FlagBit
allFlags = S.fromList [minBound .. maxBound]

-- | The flags a condition tests.
conditionFlags :: A.Cond -> S.Set FlagBit
conditionFlags (A.Cond c) = S.fromList $ case c `shiftR` 1 of
  0 -> [FlagZ]
  1 -> [FlagC]
  2 -> [FlagN]
  3 -> [FlagV]
  4 -> [FlagC, FlagZ]
  5 -> [FlagN, FlagV]
  6 -> [FlagN, FlagZ, FlagV]
  _ -> []

-- | Registers and flags an instruction reads, as a superset where the decoder does not name its sources.
instructionReads :: Word32 -> A.Instruction -> (S.Set Int, S.Set FlagBit)
instructionReads word instruction = (S.fromList (map numbered registers), flags)
  where
    conditional = maybe allFlags conditionFlags (condOf instruction)
    carry = S.insert FlagC conditional
    operandRegisters operand = case operand of
      A.Immediate _ -> []
      A.ShiftedImmediate register _ _ -> [register]
      A.ShiftedRegister register _ shift -> [register, shift]
    offsetRegisters offset = case offset of
      A.OffsetImmediate _ -> []
      A.OffsetRegister _ register _ _ -> [register]
    raw = map (\shift -> A.Reg (fromIntegral ((word `shiftR` shift) .&. 0xf))) [0, 8, 12, 16]
    (registers, flags) = case instruction of
      A.DataProcessing _ op _ _ source operand ->
        ( (if op `elem` [A.MOV, A.MVN] then [] else [source]) ++ operandRegisters operand
        , if op `elem` [A.ADC, A.SBC, A.RSC] || shiftsThroughCarry operand then carry else conditional )
      A.MoveWide _ _ _ -> ([], conditional)
      A.MoveTop _ destination _ -> ([destination], conditional)
      A.ReadStatus _ _ -> ([], allFlags)
      A.WriteApplicationFlags _ source -> ([source], conditional)
      A.ZeroExtend _ _ source _ _ -> ([source], conditional)
      A.Multiply _ _ left right accumulator -> ([left, right] ++ maybe [] pure accumulator, conditional)
      A.Compute _ _ -> (raw, allFlags)
      A.Load _ _ _ _ base offset _ -> (base : offsetRegisters offset, conditional)
      A.Store _ width source base offset _ -> ([source] ++ [A.Reg (numbered source + 1) | width == A.Dual] ++ base : offsetRegisters offset, conditional)
      A.LoadMultiple _ base _ _ _ -> ([base], conditional)
      A.StoreMultiple _ base _ _ listed -> (base : listed, conditional)
      A.Branch _ _ _ -> ([], conditional)
      A.BranchExchange _ _ register -> ([register], conditional)
      A.Hint _ -> ([], conditional)
      A.Rejected _
        | admittedSystem word, Just _ <- mrcDestination word -> ([], conditionFlags (A.Cond (word `shiftR` 28)))
        | admittedSystem word, Just (A.StatusWrite c _ _ (A.StatusFromRegister source)) <- A.statusWrite word -> ([source], conditionFlags c)
        | otherwise -> (raw, allFlags)
    -- RRX (ROR #0) reads the carry.
    shiftsThroughCarry operand = case operand of
      A.ShiftedImmediate _ A.ROR 0 -> True
      _ -> False

-- | An instruction's condition, the registers it writes when it executes, the flags it always writes when it
-- executes, and whether it may change any flag.
data Writes = Writes A.Cond (S.Set Int) (S.Set FlagBit) Bool

-- | Logical data-processing with S writes N and Z; C only for some shifter operands and V never.
instructionWrites :: Word32 -> A.Instruction -> Writes
instructionWrites word instruction = case instruction of
  A.DataProcessing c op setsFlags destination _ _ ->
    let tests = op `elem` [A.CMP, A.CMN, A.TST, A.TEQ]
        arithmetic = op `elem` [A.SUB, A.RSB, A.ADD, A.ADC, A.SBC, A.RSC, A.CMP, A.CMN]
        flags
          | not (setsFlags || tests) = S.empty
          | arithmetic = allFlags
          | otherwise = S.fromList [FlagN, FlagZ]
    in Writes c (registers [destination | not tests]) flags (setsFlags || tests)
  A.MoveWide c destination _ -> Writes c (registers [destination]) S.empty False
  A.MoveTop c destination _ -> Writes c (registers [destination]) S.empty False
  A.ReadStatus c destination -> Writes c (registers [destination]) S.empty False
  A.WriteApplicationFlags c _ -> Writes c S.empty allFlags True
  A.ZeroExtend c destination _ _ _ -> Writes c (registers [destination]) S.empty False
  A.Multiply c destination _ _ _ -> Writes c (registers [destination]) S.empty False
  A.Compute c destinations -> Writes c (registers destinations) S.empty False
  A.Load c width _ destination _ _ _ -> Writes c (registers (destination : [A.Reg (numbered destination + 1) | width == A.Dual])) S.empty False
  A.LoadMultiple c _ _ _ listed -> Writes c (registers listed) S.empty False
  A.Rejected _
    | Just destination <- mrcDestination word -> Writes (A.Cond (word `shiftR` 28)) (registers [destination]) S.empty False
    | Just (A.StatusWrite c A.CurrentStatus fields _) <- A.statusWrite word ->
        if A.FlagBits `elem` fields then Writes c S.empty allFlags True else Writes c S.empty S.empty False
  _ -> Writes (maybe (A.Cond 14) id (condOf instruction)) S.empty S.empty False
  where registers = S.fromList . map numbered

-- | An MRC to a general register, or an MSR to the CPSR from a register.
admittedSystem :: Word32 -> Bool
admittedSystem word = case (mrcDestination word, A.statusWrite word) of
  (Just destination, _) -> destination /= A.pc
  (Nothing, Just (A.StatusWrite _ A.CurrentStatus _ (A.StatusFromRegister _))) -> True
  _ -> False

clobberRegister :: Clobber -> Maybe Int
clobberRegister clobber = case clobber of
  ClobberR1 -> Just 1
  ClobberR2 -> Just 2
  ClobberR3 -> Just 3
  ClobberR12 -> Just 12
  ClobberFlags -> Nothing

-- | Caller-saved state on one path that may still hold the value the entered body left: registers, the flags,
-- and the condition of each such register's last conditional write since the flags last changed.
data Pending = Pending (S.Set Int) (S.Set FlagBit) (M.Map Int A.Cond) deriving (Eq, Ord)

settledPending :: Pending -> Bool
settledPending (Pending registers flags _) = S.null registers && S.null flags

-- | Pending state after an instruction that writes as given. An unconditional write kills a register. A write
-- under the complement of the register's recorded condition kills it too, since no flag write lies between them.
afterWrites :: Writes -> Pending -> Pending
afterWrites (Writes condition targets setsFlags mayChangeFlags) (Pending registers flags halves)
  | condition == A.Cond 14 =
      Pending (registers `S.difference` targets) (flags `S.difference` setsFlags)
        (if mayChangeFlags then M.empty else halves `M.withoutKeys` targets)
  | otherwise =
      let completed = S.filter (\n -> M.lookup n halves == Just (A.negateCond condition)) targets
          registers' = registers `S.difference` completed
          recorded = M.union (M.fromSet (const condition) (S.intersection registers' targets)) (halves `M.withoutKeys` targets)
      in Pending registers' flags (if mayChangeFlags then M.empty else recorded)

-- | The published words each root slot may hold, over both root objects.
newtype RootTargets = RootTargets (M.Map RootSlot (S.Set Word32)) deriving (Eq, Show)

rootTargets :: [(RootSlot, Word32)] -> RootTargets
rootTargets rows = RootTargets (M.fromListWith S.union [(slot, S.singleton target) | (slot, target) <- rows])

-- | Bytes of a root object.
rootObjectBytes :: Word32
rootObjectBytes = 0x162c

-- | The canonical publisher's stores into loader A's root object and the canonical initializer's into loader B's,
-- by slot. Each loader's header word at +0x10 names its root object.
publishedRows :: B.ByteString -> B.ByteString -> Either Text [(RootSlot, Word32)]
publishedRows loaderA loaderB = do
  (storesA, _) <- either (Left . T.pack) Right (PS.publications loaderA)
  storesB <- either (Left . T.pack) Right (I.originalStores loaderB)
  rootA <- header loaderA
  rootB <- header loaderB
  pure ( rootRows rootA [(guest, value) | PS.Publication (PS.Guest guest) value _ <- storesA]
      ++ rootRows rootB [(guest, value) | I.ExpectedStore (P.Guest guest) value <- storesB] )
  where
    header loader
      | B.length loader >= 0x14 = Right (physicalWord (U.word32At loader 0x10))
      | otherwise = Left "loader header word is absent"

-- | Stores that land in the root object at the root, by slot.
rootRows :: Word32 -> [(Word32, Word32)] -> [(RootSlot, Word32)]
rootRows root stores =
  [ (RootSlot (physicalWord guest - root), value) | (guest, value) <- stores
  , physicalWord guest >= root, physicalWord guest - root < rootObjectBytes ]

physicalWord :: Word32 -> Word32
physicalWord address
  | address >= 0x40000000 && address < 0x60000000 = address - 0x40000000
  | otherwise = address

-- | How the analysed call entered the slot.
data Entered = EnteredDirectly | EnteredThroughRoot deriving (Eq, Show)

-- | Whether the caller-saved state the entered body may change is dead from the return site. Every path is
-- followed through branches, including jump tables bounded by their compare, until the state is overwritten, a
-- call, or a return. A register or flag is live when some path reads it first. A pair of writes under
-- complementary conditions with no flag write between them counts as a write; so does one such write on the
-- side of a conditional branch, or for a read, under the same condition.
--
-- After a root-slot call the next call ends the path: the caller's code cannot know an indirect callee's
-- clobbers, so under AAPCS it passes the next callee no caller-saved state from before that call. After a direct
-- call, a later call to one of the slot entries, directly or through one of the owned root slots ('rootSlotAt'),
-- reads only r0 (replacement/rtos_own_first/src/entries.rs:51-174); any other direct call, and a call through
-- any other root slot into each of its published words, is followed into its callee, to depth 3, and ends the
-- path; any other indirect call is live. A return ends the path with r1 dead, which holds for results of at most
-- 32 bits. An indirect branch, a load to pc outside a jump table, an unadmitted word or a path longer than 128
-- instructions is live.
deadAfter :: (Word32 -> Maybe Word32) -> [Word32] -> [RootSlot] -> RootTargets -> Entered -> [Clobber] -> Word32 -> Bool
deadAfter word entries offsets published entered clobbers start = liveAfter word entries offsets published entered clobbers start == Nothing

-- | The first word 'deadAfter' finds the state live at, and why.
liveAfter :: (Word32 -> Maybe Word32) -> [Word32] -> [RootSlot] -> RootTargets -> Entered -> [Clobber] -> Word32 -> Maybe Text
liveAfter word entries offsets (RootTargets published) entered clobbers =
  dead (3 :: Int) (Pending (S.fromList (mapMaybe clobberRegister clobbers)) (if ClobberFlags `elem` clobbers then allFlags else S.empty) M.empty)
  where
    dead depth initial start = go S.empty [(start, initial, 0 :: Int)]
      where
        go _ [] = Nothing
        go seen ((pc, pending@(Pending registers flags halves), count) : rest)
          | settledPending pending = go seen rest
          | S.member (pc, pending) seen = go seen rest
          | count >= 128 = live "path bound"
          | otherwise = case word pc of
              Nothing -> live "unreadable word"
              Just raw ->
                let instruction = A.decode pc raw
                    condition = maybe (A.Cond 14) id (condOf instruction)
                    conditional = condition /= A.Cond 14
                    (readRegisters, readFlags) = instructionReads raw instruction
                    -- A register written under this instruction's condition holds a fresh value whenever it executes.
                    readPending = S.filter (\n -> not (conditional && M.lookup n halves == Just condition)) (S.intersection registers readRegisters)
                    next paths = go (S.insert (pc, pending) seen) (paths ++ rest)
                    fallthrough = [(pc + 4, resolved (A.negateCond condition), count + 1) | conditional]
                    -- Pending state on the side where the condition holds.
                    resolved holds = Pending (S.filter (\n -> M.lookup n halves /= Just holds) registers) flags halves
                    taken target = (target, if conditional then resolved condition else pending, count + 1)
                    ends = next fallthrough
                    callee targets = case mapMaybe (dead (depth - 1) pending) targets of
                      reason : _ -> Just reason
                      [] -> ends
                in if not (S.null readPending) || not (S.null (S.intersection flags readFlags)) then live "read"
                   else case instruction of
                     A.Branch _ True target
                       | entered == EnteredThroughRoot || target `elem` entries -> ends
                       | depth == 0 -> live "call depth"
                       | otherwise -> callee [target]
                     A.BranchExchange _ True _
                       | entered == EnteredThroughRoot -> ends
                       | otherwise -> case rootSlotAt word pc of
                           Just slot
                             | slot `elem` offsets -> ends
                             | depth == 0 -> live "call depth"
                             | Just targets <- M.lookup slot published, not (S.null targets) -> callee (S.toList targets)
                           _ -> live "indirect call"
                     A.Branch _ False target -> next (taken target : fallthrough)
                     A.BranchExchange _ False register
                       | register == A.lr -> ends
                       | otherwise -> live "indirect branch"
                     A.LoadMultiple _ base _ _ listed
                       | A.pc `elem` listed -> if base == A.sp then ends else live "load-multiple to pc"
                     A.DataProcessing _ _ _ destination _ _ | destination == A.pc -> live "write to pc"
                     A.Load _ _ _ destination _ _ _
                       | destination == A.pc -> case jumpTable word pc instruction of
                           Just targets -> next (map taken targets ++ fallthrough)
                           Nothing -> live "load to pc"
                     A.Rejected _ | not (admittedSystem raw) -> live "unadmitted word"
                     _ -> next [(pc + 4, afterWrites (instructionWrites raw instruction) pending, count + 1)]
          where live reason = Just (reason <> " at " <> hex pc)

-- | The targets of @ldrls pc,[pc,rI,lsl #2]@ or @ldrlo@ right after @cmp rI,#n@: the n+1 or n words after the
-- next one.
jumpTable :: (Word32 -> Maybe Word32) -> Word32 -> A.Instruction -> Maybe [Word32]
jumpTable word pc instruction = case instruction of
  A.Load (A.Cond c) A.Word _ _ base (A.OffsetRegister True index A.LSL 2) A.OffsetOnly
    | base == A.pc, c `elem` [3, 9] -> do
        compare' <- A.decode (pc - 4) <$> word (pc - 4)
        bound <- case compare' of
          A.DataProcessing (A.Cond 14) A.CMP _ _ register (A.Immediate n) | register == index -> Just n
          _ -> Nothing
        let count = if c == 9 then bound + 1 else bound
        mapM (\k -> word (pc + 8 + 4 * k)) [0 .. count - 1]
  _ -> Nothing
