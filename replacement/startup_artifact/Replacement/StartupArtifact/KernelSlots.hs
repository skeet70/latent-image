{-# LANGUAGE OverloadedStrings #-}
-- | The kernel-slot-bodies footprint and the A32 facts its joins check: the
-- owned bodies' calling-convention walk, the direct and root-slot calls that
-- enter the slot extents, and caller-saved liveness after those calls.
module Replacement.StartupArtifact.KernelSlots
  ( footprint, CodeImage (..), codeWord, walkAbi
  , DirectCall (..), directCalls, SlotCall (..), slotCalls, deadAfter
  ) where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Bits (complement, shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32)
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

-- | A register value: the caller's register at entry, an offset from the entry SP, or anything.
data Value = Original Int | Frame Int | Unknown deriving (Eq, Ord, Show)

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

-- | Caller-saved state a body may change, and the (argument register, byte offset) words it stores through
-- its r0-r3 arguments.
data Walk = Walk (S.Set Clobber) (S.Set (Int, Int))

instance Semigroup Walk where
  Walk a b <> Walk c d = Walk (S.union a c) (S.union b d)

-- | Walks an owned body from its entry over the words of its loader and returns the caller-saved state it may
-- change. Refuses unless every return restores r4-r11 and SP and returns to the entry LR. Calls must target one
-- of the given entries, which are walked in turn; branches must stay inside the owned extents.
walkAbi :: CodeImage -> [(Word32, Word32)] -> [Word32] -> Word32 -> Either Text [Clobber]
walkAbi code extents entries entry = (\(Walk clobbers _) -> S.toAscList clobbers) <$> go (4 :: Int) entry
  where
    owned address = any (\(low, high) -> address >= low && address < high) extents
    go depth start = do
      when (depth == 0) (Left ("owned call depth exceeded at " <> hex start))
      unless (owned start) (Left ("walk entry outside the owned extents: " <> hex start))
      let initial = State start (M.fromList ((13, Frame 0) : [(n, Original n) | n <- [0 .. 12] ++ [14]])) M.empty False
      explore depth S.empty [initial] (Walk S.empty S.empty)
    explore _ _ [] found = Right found
    explore depth seen (state : pending) found
      | S.member state seen = explore depth seen pending found
      | S.size seen > 20000 = Left "owned walk state bound exceeded"
      | otherwise = do
          let pc = statePc state
          unless (owned pc) (Left ("owned flow leaves the extents at " <> hex pc))
          word <- maybe (Left ("owned word unreadable at " <> hex pc)) Right (codeWord code pc)
          let instruction = A.decode pc word
              skipped = [advance state | maybe False (/= A.Cond 14) (condOf instruction)]
          (next, returned, stores) <- execute depth pc word instruction state
          clobbers <- mapM (returnClobbers pc) returned
          explore depth (S.insert state seen) (next ++ skipped ++ pending) (found <> Walk (S.unions clobbers) (S.fromList stores))
    returnClobbers pc state = do
      unless (registerOf state A.sp == Frame 0) (Left ("owned return changes SP at " <> hex pc))
      unless (all (\n -> registerOf state (A.Reg n) == Original n) [4 .. 11]) (Left ("owned return changes r4-r11 at " <> hex pc))
      pure (S.fromList ([clobber | (n, clobber) <- scratch, registerOf state (A.Reg n) /= Original n] ++ [ClobberFlags | stateFlags state]))
    scratch = [(1, ClobberR1), (2, ClobberR2), (3, ClobberR3), (12, ClobberR12)]
    advance state = state {statePc = statePc state + 4}
    continue state = Right ([advance state], [], [])
    finish pc state value
      | value == Original 14 = Right ([], [state], [])
      | otherwise = Left ("owned return target is not the entry LR at " <> hex pc)
    execute depth pc word instruction state = case instruction of
      A.DataProcessing _ op setsFlags destination source operand -> do
        let flagged = state {stateFlags = stateFlags state || setsFlags || op `elem` [A.CMP, A.CMN, A.TST, A.TEQ]}
            value = case (op, operand) of
              (A.MOV, A.ShiftedImmediate register A.LSL 0) -> registerOf state register
              (A.ADD, A.Immediate k) | Frame o <- registerOf state source -> Frame (o + fromIntegral k)
              (A.SUB, A.Immediate k) | Frame o <- registerOf state source -> Frame (o - fromIntegral k)
              _ -> Unknown
        if op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] then continue flagged else do
          when (destination == A.pc) (Left ("owned data-processing write to pc at " <> hex pc))
          when (destination == A.sp && not (isFrame value)) (Left ("owned SP leaves its frame provenance at " <> hex pc))
          continue (setRegister destination value flagged)
      A.MoveWide _ destination _ -> written [destination]
      A.MoveTop _ destination _ -> written [destination]
      A.ReadStatus _ destination -> written [destination]
      A.ZeroExtend _ destination _ _ _ -> written [destination]
      A.Multiply _ destination _ _ _ -> written [destination]
      A.Compute _ destinations -> written destinations
      A.WriteApplicationFlags _ _ -> continue state {stateFlags = True}
      A.Hint _ -> continue state
      A.Load _ width _ destination base offset indexing -> do
        when (destination == A.pc || indexing /= A.OffsetOnly) (Left ("owned load form outside the walked forms at " <> hex pc))
        let loaded = case (registerOf state base, offset) of
              (Frame o, A.OffsetImmediate k) | width == A.Word -> M.findWithDefault Unknown (o + fromIntegral k) (stateStack state)
              _ -> Unknown
            cleared = foldl' (\s r -> setRegister r Unknown s) state (destination : [A.Reg (numbered destination + 1) | width == A.Dual])
        continue (setRegister destination loaded cleared)
      A.Store _ width source base offset indexing -> do
        unless (indexing == A.OffsetOnly) (Left ("owned store with writeback at " <> hex pc))
        k <- case offset of
          A.OffsetImmediate k -> Right (fromIntegral k)
          _ | isFrame (registerOf state base) || isArgument (registerOf state base) -> Left ("owned stack or argument store with a register offset at " <> hex pc)
            | otherwise -> Right 0
        let wordsAt slot = slot : [slot + 4 | width == A.Dual]
        case registerOf state base of
          Frame o -> do
            let values = zip (wordsAt (o + k)) (if width `elem` [A.Word, A.Dual] then map (registerOf state) (source : [A.Reg (numbered source + 1) | width == A.Dual]) else [Unknown])
            continue state {stateStack = foldl' (\m (slot, v) -> M.insert slot v m) (stateStack state) values}
          Original n | n <= 3 -> Right ([advance state], [], [(n, slot) | slot <- wordsAt k])
          _ -> continue state
      A.StoreMultiple _ base mode writeback registers
        | base == A.sp, Frame o <- registerOf state A.sp, mode == A.DecrementBefore -> do
            let low = o - 4 * length registers
                stack = foldl' (\m (k, r) -> M.insert k (registerOf state r) m) (stateStack state) (zip [low, low + 4 ..] registers)
                updated = state {stateStack = stack}
            continue (if writeback then setRegister A.sp (Frame low) updated else updated)
        | isFrame (registerOf state base) || isArgument (registerOf state base) -> Left ("owned store-multiple form outside the walked forms at " <> hex pc)
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
        | A.pc `elem` registers -> Left ("owned return form outside the walked forms at " <> hex pc)
        | otherwise -> written registers
      A.Branch _ False target
        | owned target -> Right ([state {statePc = target}], [], [])
        | otherwise -> Left ("owned branch leaves the extents at " <> hex pc)
      A.Branch _ True target -> do
        unless (target `elem` entries) (Left ("owned call to a non-entry " <> hex target <> " at " <> hex pc))
        Walk callee stores <- go (depth - 1) target
        let through = [(registerOf state (A.Reg n), k) | (n, k) <- S.toList stores]
            stack = foldl' (\m (o, k) -> M.insert (o + k) Unknown m) (stateStack state) [(o, k) | (Frame o, k) <- through]
            returned = foldl' (\s n -> setRegister (A.Reg n) Unknown s) state {stateStack = stack} [0, 1, 2, 3, 12, 14]
        Right ( [advance returned {stateFlags = stateFlags state || S.member ClobberFlags callee}], []
              , [(n, k) | (Original n, k) <- through, n <= 3] )
      A.BranchExchange _ False register -> finish pc state (registerOf state register)
      A.BranchExchange _ True _ -> Left ("owned indirect call at " <> hex pc)
      A.Rejected _
        | Just destination <- mrcDestination word -> do
            when (destination == A.pc) (Left ("owned MRC to the flags at " <> hex pc))
            written [destination]
        | Just (A.StatusWrite _ A.CurrentStatus fields _) <- A.statusWrite word ->
            continue state {stateFlags = stateFlags state || A.FlagBits `elem` fields}
        | otherwise -> Left ("owned instruction outside the walked forms at " <> hex pc)
      where
        written registers = do
          when (any (`elem` [A.sp, A.pc]) registers) (Left ("owned write to sp or pc at " <> hex pc))
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
directCalls bytesOf spans carriers =
  [ DirectCall site target linked
  | Span owner (Guest start) (Guest end) <- spans, site <- [alignUp start, alignUp start + 4 .. end - 4]
  , not (inside site)
  , Just word <- [codeWord (CodeImage (targetBase owner) (bytesOf owner)) site]
  , A.Branch _ linked target <- [A.decode site word], inside target ]
  where
    inside address = any (\(Span _ (Guest low) (Guest high)) -> address >= low && address < high) carriers

alignUp :: Word32 -> Word32
alignUp address = (address + 3) .&. complement 3

-- | A @blx@ whose register was just loaded from an interface-root slot.
data SlotCall = SlotCall { slotSite :: Word32, slotOffset :: RootSlot } deriving (Eq, Show)

-- | Root-slot calls in the spans: @ldr rN,[rM,#k]@ or @movw rK,#k; ldr rN,[rM,rK]@ within the six words before @blx rN@.
slotCalls :: (Target -> B.ByteString) -> [Span] -> [SlotCall]
slotCalls bytesOf spans =
  [ SlotCall site offset
  | Span target (Guest start) (Guest end) <- spans
  , let word = codeWord (CodeImage (targetBase target) (bytesOf target))
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
      [A.Load _ A.Word _ _ _ (A.OffsetImmediate k) A.OffsetOnly] | k >= 0 -> Just (fromIntegral k)
      [A.Load _ A.Word _ _ _ (A.OffsetRegister True index A.LSL 0) A.OffsetOnly] ->
        case lastWrite index (drop 1 (dropWhile (not . writesTo register . snd) before)) of
          [A.MoveWide _ _ k] -> Just k
          _ -> Nothing
      _ -> Nothing

-- | Registers and flags an instruction reads, as a superset where the decoder does not name its sources.
instructionReads :: Word32 -> A.Instruction -> (S.Set Int, Bool)
instructionReads word instruction = (S.fromList (map numbered registers), flags)
  where
    conditional = maybe True (/= A.Cond 14) (condOf instruction)
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
        , conditional || op `elem` [A.ADC, A.SBC, A.RSC] )
      A.MoveWide _ _ _ -> ([], conditional)
      A.MoveTop _ destination _ -> ([destination], conditional)
      A.ReadStatus _ _ -> ([], True)
      A.WriteApplicationFlags _ source -> ([source], conditional)
      A.ZeroExtend _ _ source _ _ -> ([source], conditional)
      A.Multiply _ _ left right accumulator -> ([left, right] ++ maybe [] pure accumulator, conditional)
      A.Compute _ _ -> (raw, True)
      A.Load _ _ _ _ base offset _ -> (base : offsetRegisters offset, conditional)
      A.Store _ width source base offset _ -> ([source] ++ [A.Reg (numbered source + 1) | width == A.Dual] ++ base : offsetRegisters offset, conditional)
      A.LoadMultiple _ base _ _ _ -> ([base], conditional)
      A.StoreMultiple _ base _ _ listed -> (base : listed, conditional)
      A.Branch _ _ _ -> ([], conditional)
      A.BranchExchange _ _ register -> ([register], conditional)
      A.Hint _ -> ([], conditional)
      A.Rejected _
        | admittedSystem word, Just _ <- mrcDestination word -> ([], word `shiftR` 28 /= 14)
        | admittedSystem word, Just (A.StatusWrite _ _ _ (A.StatusFromRegister source)) <- A.statusWrite word -> ([source], word `shiftR` 28 /= 14)
        | otherwise -> (raw, True)

-- | Registers an unconditional instruction writes, and whether it sets the flags.
instructionWrites :: Word32 -> A.Instruction -> (S.Set Int, Bool)
instructionWrites word instruction = case instruction of
  A.DataProcessing c op setsFlags destination _ _ -> always c ([destination | op `notElem` [A.CMP, A.CMN, A.TST, A.TEQ]], setsFlags)
  A.MoveWide c destination _ -> always c ([destination], False)
  A.MoveTop c destination _ -> always c ([destination], False)
  A.ReadStatus c destination -> always c ([destination], False)
  A.WriteApplicationFlags c _ -> always c ([], True)
  A.ZeroExtend c destination _ _ _ -> always c ([destination], False)
  A.Multiply c destination _ _ _ -> always c ([destination], False)
  A.Compute c destinations -> always c (destinations, False)
  A.Load c width _ destination _ _ _ -> always c (destination : [A.Reg (numbered destination + 1) | width == A.Dual], False)
  A.LoadMultiple c _ _ _ listed -> always c (listed, False)
  A.Rejected _
    | Just destination <- mrcDestination word, (word `shiftR` 28) == 14 -> (S.singleton (numbered destination), False)
    | Just (A.StatusWrite c A.CurrentStatus fields _) <- A.statusWrite word -> always c ([], A.FlagBits `elem` fields)
  _ -> (S.empty, False)
  where
    always c (registers, flags)
      | c == A.Cond 14 = (S.fromList (map numbered registers), flags)
      | otherwise = (S.empty, False)

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

-- | Whether stock code from the return site reads none of the clobbered state before overwriting it. A call to
-- one of the given slot entries, directly or through one of the given root slots, reads only r0
-- (replacement/rtos_own_first/src/entries.rs:51-173) and leaves the state pending; any other direct call ends
-- liveness once its callee, walked the same way to depth 3, reads none of the pending r1-r3; any other indirect
-- call or branch with pending r1-r3 refuses; a return ends liveness; any other unadmitted word with pending state
-- refuses. Paths are bounded to 128 instructions.
deadAfter :: (Word32 -> Maybe Word32) -> [Word32] -> [RootSlot] -> [Clobber] -> Word32 -> Bool
deadAfter word entries offsets clobbers = live (3 :: Int) (S.fromList (mapMaybe clobberRegister clobbers), ClobberFlags `elem` clobbers)
  where
    live depth initial start = go S.empty [(start, initial, 0 :: Int)]
      where
        go _ [] = True
        go seen ((pc, pending@(registers, flags), count) : rest)
          | S.null registers && not flags = go seen rest
          | S.member (pc, pending) seen = go seen rest
          | count >= 128 = False
          | otherwise = case word pc of
              Nothing -> False
              Just raw ->
                let instruction = A.decode pc raw
                    (readRegisters, readFlags) = instructionReads raw instruction
                    (writtenRegisters, writtenFlags) = instructionWrites raw instruction
                    conditional = maybe True (/= A.Cond 14) (condOf instruction)
                    after = (registers `S.difference` writtenRegisters, flags && not writtenFlags)
                    seen' = S.insert (pc, pending) seen
                    argumentRegisters = S.intersection registers (S.fromList [1, 2, 3])
                    arguments = not (S.null argumentRegisters)
                    fallthrough = [(pc + 4, pending, count + 1) | conditional]
                in if not (S.null (S.intersection registers readRegisters)) || (flags && readFlags) then False
                   else case instruction of
                     A.Branch _ True target
                       | target `elem` entries -> go seen' ((pc + 4, pending, count + 1) : rest)
                       | arguments && (depth == 0 || not (live (depth - 1) (argumentRegisters, False) target)) -> False
                       | otherwise -> go seen' (fallthrough ++ rest)
                     A.BranchExchange _ True _
                       | maybe False (`elem` offsets) (slotOffsetAt word 0 pc) -> go seen' ((pc + 4, pending, count + 1) : rest)
                       | otherwise -> not arguments && go seen' (fallthrough ++ rest)
                     A.Branch _ False target -> go seen' ((target, pending, count + 1) : fallthrough ++ rest)
                     A.BranchExchange _ False register
                       | register == A.lr -> go seen' (fallthrough ++ rest)
                       | otherwise -> not arguments && go seen' (fallthrough ++ rest)
                     A.LoadMultiple _ _ _ _ listed | A.pc `elem` listed -> go seen' (fallthrough ++ rest)
                     A.DataProcessing _ _ _ destination _ _ | destination == A.pc -> False
                     A.Load _ _ _ destination _ _ _ | destination == A.pc -> False
                     A.Rejected _ | not (admittedSystem raw) -> False
                     _ -> go seen' ((pc + 4, after, count + 1) : rest)
