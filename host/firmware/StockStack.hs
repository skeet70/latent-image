-- | A static stack-depth bound over flat stock code: the depth of an entry is the deepest SP
-- offset any path from it reaches, a call adding the callee's depth at the call's offset. The
-- walk follows control flow (no function extents), tracks the offset per instruction and refuses
-- where two paths reach one instruction at different offsets. Owned code is not walked: a supplied
-- entry is taken at its depth wherever a path reaches it, and entering owned code elsewhere refuses.
module StockStack
  ( StockCode
  , stockCode
  , codeWord
  , StackDepth (..)
  , Supplied (..)
  , SlotOffset (..)
  , Condition (..)
  , CodeRange (..)
  , CallbackSite (..)
  , LockMethods (..)
  , ContextLocks (..)
  , IndirectTargets (..)
  , SwitchModel (..)
  , noSwitches
  , StackReport (..)
  , StackRefusal (..)
  , stockDepth
  , stockDepths
  , kernelMode
  , contextLockMethods
  ) where

import qualified A32
import Data.Bits (complement, shiftL, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Unsafe as BU
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import StackWalk (Effect (..), Refused (UnaccountedSpWrite), Strictness (Strict), classify, framePointer, writes)

-- | Code segments at their guest bases, and the owned ranges the walk must not enter.
data StockCode = StockCode [(Word32, B.ByteString)] [(Word32, Word32)]

stockCode :: [(Word32, B.ByteString)] -> [(Word32, Word32)] -> StockCode
stockCode = StockCode

codeWord :: StockCode -> Word32 -> Maybe Word32
codeWord (StockCode segments _) address = case [(base, bytes) | (base, bytes) <- segments, base <= address, address - base + 4 <= fromIntegral (B.length bytes)] of
  (base, bytes) : _ | address .&. 3 == 0 ->
    let offset = fromIntegral (address - base)
        byte index = fromIntegral (BU.unsafeIndex bytes (offset + index)) :: Word32
     in Just (byte 0 .|. (byte 1 `shiftL` 8) .|. (byte 2 `shiftL` 16) .|. (byte 3 `shiftL` 24))
  _ -> Nothing

owned :: StockCode -> Word32 -> Bool
owned (StockCode _ ranges) address = any (\(low, high) -> low <= address && address < high) ranges

-- | Bytes below an entry's SP.
newtype StackDepth = StackDepth Word32 deriving (Eq, Ord, Show)

-- | Entries whose depth is proved elsewhere (owned code); the walk does not enter them.
newtype Supplied = Supplied (M.Map Word32 StackDepth) deriving (Eq, Show)

-- | Offset of a service pointer in a published interface root.
newtype SlotOffset = SlotOffset Word32 deriving (Eq, Ord, Show)

-- | A premise a result depends on.
data Condition
  = -- | Published root-slot words are not rewritten after publication (S1).
    SlotWordsFixed
  | -- | The error callback word holds only the targets its writer check names (S2).
    ErrorCallbackClosed
  | -- | The trace callback word holds only the targets its writer check names (S3).
    TraceCallbackClosed
  | -- | No task-exception routine is defined (S4).
    NoTaskException
  | -- | A kernel context's lock methods are the ones its initializer stores, and a call through a word
    -- loaded at a lock-method offset with r0 at that lock's offset addresses one context (S5).
    ContextLocksFixed
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | Code addresses [low, high).
data CodeRange = CodeRange Word32 Word32 deriving (Eq, Show)

inRange :: CodeRange -> Word32 -> Bool
inRange (CodeRange low high) address = low <= address && address < high

-- | A register branch's closed target set and the premise that closes it.
data CallbackSite = CallbackSite {callbackTargets :: [Word32], callbackCondition :: Condition} deriving (Eq, Show)

-- | The lock methods a kernel context initializer stores.
data LockMethods = LockMethods {acquireMethods :: [Word32], releaseMethods :: [Word32]} deriving (Eq, Show)

-- | One kernel's code range and its context lock methods.
data ContextLocks = ContextLocks {lockKernel :: CodeRange, lockMethods :: LockMethods} deriving (Eq, Show)

data IndirectTargets = IndirectTargets
  { -- | Guest address of the interface root table (0x96bd20).
    rootTable :: Word32
  , -- | Slot offset to the services of table A and table B.
    rootSlots :: M.Map SlotOffset [Word32]
  , -- | Register-branch PCs with closed target sets.
    callbackSites :: M.Map Word32 CallbackSite
  , contextLocks :: [ContextLocks]
  }

data SwitchModel = SwitchModel
  { -- | Instructions that load SP from a task control block or an idle stack: a path ends there.
    switchEnds :: S.Set Word32
  , -- | The instruction storing SP to the control block, to the resume PC entered at that offset.
    resumeEdges :: M.Map Word32 Word32
  , -- | Branch targets of the resume path that run task-exception routines; skipped under S4.
    exceptionPaths :: S.Set Word32
  }

noSwitches :: SwitchModel
noSwitches = SwitchModel S.empty M.empty S.empty

-- | Tasks run in SVC mode.
kernelMode :: Word32
kernelMode = 0x13

data StackReport = StackReport
  { reportDepth :: StackDepth
  , -- | Supplied entries the closure reached.
    reportSupplied :: [Word32]
  , -- | Entries on the deepest path, outermost first.
    reportChain :: [Word32]
  , reportConditions :: S.Set Condition
  , -- | Stock entries the closure walked.
    reportEntered :: [Word32]
  }
  deriving (Eq, Show)

data StackRefusal
  = Refusal Word32 Refused
  | Recursion [Word32]
  | UnresolvedBranch Word32
  | NoCode Word32
  | OwnedCode Word32
  | ModeChange Word32
  | OffsetMismatch Word32
  | FrameUnknown Word32
  | TableUnbounded Word32
  | -- | A return whose SP is not the entry's SP once it releases its bytes.
    UnbalancedReturn Word32
  deriving (Eq, Ord, Show)

-- | A tracked register value.
data Value = Const Word32 | ModeBits Word32 | RootEntry | SlotPointer Word32
  | -- | A context lock's address: a context pointer plus 8 or 0x14.
    LockAddress Word32
  | -- | A word loaded from a context at a lock-method offset.
    LockMethod Word32
  | -- | The return address the entry was called with.
    EntryLink
  deriving (Eq, Ord, Show)

type Registers = M.Map A32.Reg Value

-- | Tracked registers and the condition outcomes a path assumes (condition code to taken).
data Machine = Machine Registers (M.Map Word32 Bool) deriving (Eq, Ord, Show)

-- | Whether a condition holds under the assumptions, when they decide it.
decided :: A32.Cond -> M.Map Word32 Bool -> Maybe Bool
decided (A32.Cond c) flags
  | c >= 14 = Just True
  | otherwise = M.lookup c flags

-- | Assumes a condition's outcome, and the opposite outcome of its negation.
assume :: A32.Cond -> Bool -> M.Map Word32 Bool -> M.Map Word32 Bool
assume (A32.Cond c) taken = M.insert c taken . M.insert (c `xor` 1) (not taken)

-- | The condition of an instruction the register tracking reads, and the instruction as if it were unconditional.
conditional :: A32.Instruction -> Maybe (A32.Cond, A32.Instruction)
conditional instruction = case instruction of
  A32.DataProcessing cond op s rd rn operand | cond /= always' -> Just (cond, A32.DataProcessing always' op s rd rn operand)
  A32.MoveWide cond rd value | cond /= always' -> Just (cond, A32.MoveWide always' rd value)
  A32.MoveTop cond rd value | cond /= always' -> Just (cond, A32.MoveTop always' rd value)
  A32.ReadStatus cond rd | cond /= always' -> Just (cond, A32.ReadStatus always' rd)
  A32.Load cond width signed rt base offset indexing | cond /= always' -> Just (cond, A32.Load always' width signed rt base offset indexing)
  _ -> Nothing
  where always' = A32.Cond 14

-- | Whether an instruction can set the condition flags.
setsFlags :: A32.Instruction -> Bool
setsFlags instruction = case instruction of
  A32.DataProcessing _ _ s _ _ _ -> s
  A32.Compute {} -> True
  A32.Multiply {} -> True
  A32.WriteApplicationFlags {} -> True
  A32.Rejected _ -> True
  _ -> False

data Result = Result StackDepth [Word32] [Word32] (S.Set Condition) (S.Set Word32)

type Memo = M.Map Word32 (Either StackRefusal Result)

-- | The depth of one entry.
stockDepth :: StockCode -> Supplied -> IndirectTargets -> SwitchModel -> Word32 -> Either StackRefusal StackReport
stockDepth code supplied indirect switches entry = case stockDepths code supplied indirect switches [entry] of
  [(_, result)] -> result
  _ -> Left (NoCode entry)

-- | The depths of several entries, sharing callee results.
stockDepths :: StockCode -> Supplied -> IndirectTargets -> SwitchModel -> [Word32] -> [(Word32, Either StackRefusal StackReport)]
stockDepths code (Supplied supplied) indirect switches entries = go M.empty entries
  where
    go _ [] = []
    go memo (entry : rest) =
      let (result, memo') = depthOf memo [] entry
       in (entry, report <$> result) : go memo' rest
    report (Result depth reached chain conditions entered) = StackReport depth (S.toList (S.fromList reached)) chain conditions (S.toList entered)

    depthOf :: Memo -> [Word32] -> Word32 -> (Either StackRefusal Result, Memo)
    depthOf memo stack entry
      | Just depth <- M.lookup entry supplied = (Right (Result depth [entry] [entry] S.empty S.empty), memo)
      | Just known <- M.lookup entry memo = (known, memo)
      | entry `elem` stack = (Left (Recursion (reverse (entry : stack))), memo)
      | owned code entry = (Left (OwnedCode entry), memo)
      | otherwise =
          let (result, memo') = walk memo (entry : stack) entry
           in (result, M.insert entry result memo')

    -- One state per path: address, SP offset below entry, r11's offset if known, and the
    -- tracked registers with the condition outcomes the path has assumed since flags were last set.
    walk :: Memo -> [Word32] -> Word32 -> (Either StackRefusal Result, Memo)
    walk memo0 stack entry = loop memo0 [(entry, 0, Nothing, Machine (M.singleton A32.lr EntryLink) M.empty)] M.empty S.empty (Result (StackDepth 0) [] [entry] S.empty (S.singleton entry))
      where
        loop :: Memo -> [(Word32, Int, Maybe Int, Machine)] -> M.Map Word32 (Int, Maybe Int) -> S.Set (Word32, Machine) -> Result
          -> (Either StackRefusal Result, Memo)
        loop memo [] _ _ best = (Right best, memo)
        loop memo ((address, offset, frame, machine) : rest) offsets seen best
          | S.member (address, machine) seen && M.lookup address offsets == Just (offset, frame) = loop memo rest offsets seen best
          | Just (known, knownFrame) <- M.lookup address offsets, (known, knownFrame) /= (offset, frame) =
              (Left (if known /= offset then OffsetMismatch address else FrameUnknown address), memo)
          | S.member address (switchEnds switches) = loop memo rest offsets' seen' best'
          | Just (StackDepth depth) <- M.lookup address supplied =
              loop memo rest offsets' seen'
                (deeper best (Result (StackDepth (fromIntegral offset + depth)) [address] [entry, address] S.empty S.empty))
          | owned code address = (Left (OwnedCode address), memo)
          | otherwise = case codeWord code address of
              Nothing -> (Left (NoCode address), memo)
              Just word -> step word
          where
            Machine registers flags = machine
            offsets' = M.insert address (offset, frame) offsets
            seen' = S.insert (address, machine) seen
            best' = deeper best (atOffset offset)
            next = address + 4
            continueWith memo' best'' states = loop memo' (states ++ rest) offsets' seen' best''
            resumed = case M.lookup address (resumeEdges switches) of
              Just pc -> [(pc, offset, frame, Machine M.empty M.empty)]
              Nothing -> []
            instruction word = A32.decode address word
            -- The machines after a non-branch instruction: one, or two when its condition is undecided.
            evolve word = evolveMachine address word machine
            -- Outcomes of a conditional transfer: (taken, machine) pairs the assumptions allow.
            outcomes cond = case decided cond flags of
              Just taken -> [(taken, machine)]
              Nothing -> [(True, Machine registers (assume cond True flags)), (False, Machine registers (assume cond False flags))]
            callTo memo' targets k = do
              -- Each target's depth at this offset; the call clobbers the caller-saved registers.
              let visit (acc, m) target = case acc of
                    Left refusal -> (Left refusal, m)
                    Right (deepest, reached, conditions) ->
                      let (result, m') = depthOf m stack target
                       in case result of
                            Left refusal -> (Left refusal, m')
                            Right (Result (StackDepth depth) more chain extra entered) ->
                              let candidate = Result (StackDepth (fromIntegral offset + depth)) [] (entry : chain) S.empty entered
                               in (Right (deeper deepest candidate, reached ++ more, S.union conditions extra), m')
                  (outcome, memo'') = foldl visit (Right (best', [], S.empty), memo') targets
               in case outcome of
                    Left refusal -> (Left refusal, memo'')
                    Right (deepest, reached, conditions) -> k memo'' (addReached reached (addConditions conditions deepest))
            afterCall = Machine (clobber registers) M.empty
            step word = case classify Strict address word of
              Reserve amount -> continueWith memo (deeper best' (atOffset (offset + fromIntegral amount)))
                [(next, offset + fromIntegral amount, frameAfter word, m) | m <- evolve word]
              Release amount
                | offset - fromIntegral amount < 0 -> (Left (Refusal address UnaccountedSpWrite), memo)
                | otherwise -> continueWith memo best' [(next, offset - fromIntegral amount, frameAfter word, m) | m <- evolve word]
              FromFrame relative -> case frame of
                Just fp -> continueWith memo best' [(next, fp - fromIntegral relative, frame, m) | m <- evolve word]
                Nothing -> (Left (FrameUnknown address), memo)
              SetFrame amount -> continueWith memo best' [(next, offset, Just (offset - fromIntegral amount), m) | m <- evolve word]
              Neutral -> continueWith memo best' ([(next, offset, frameAfter word, m) | m <- evolve word] ++ resumed)
              Call target -> callTo memo [target] $ \memo' best'' ->
                continueWith memo' best'' [(next, offset, frame, afterCall)]
              Jump cond target
                | S.member target (exceptionPaths switches) ->
                    continueWith memo (addConditions (S.singleton NoTaskException) best')
                      [(next, offset, frame, m) | (False, m) <- outcomes cond]
                | otherwise -> continueWith memo best'
                    [(if taken then target else next, offset, frame, m) | (taken, m) <- outcomes cond]
              Return cond bytes
                | viaLink (instruction word) && M.lookup A32.lr registers /= Just EntryLink -> registerJump cond A32.lr
                | otherwise -> returning cond bytes
              RegisterCall register -> case resolve register of
                Nothing -> (Left (UnresolvedBranch address), memo)
                Just (targets, conditions) -> callTo memo targets $ \memo' best'' ->
                  continueWith memo' (addConditions conditions best'') [(next, offset, frame, afterCall)]
              RegisterJump cond register -> registerJump cond register
              JumpTable cond index -> case tableBound cond index of
                Nothing -> (Left (TableUnbounded address), memo)
                Just count -> case mapM (\slot -> codeWord code (address + 8 + 4 * slot)) [0 .. count - 1] of
                  Nothing -> (Left (TableUnbounded address), memo)
                  Just targets -> continueWith memo best'
                    ([(target, offset, frame, m) | (True, m) <- outcomes cond, target <- targets] ++ [(next, offset, frame, m) | (False, m) <- outcomes cond])
              ModeWrite source
                | modeOf source == Just kernelMode -> continueWith memo best' [(next, offset, frame, Machine registers M.empty)]
                | otherwise -> (Left (ModeChange address), memo)
              Refuse reason -> (Left (Refusal address reason), memo)
              Stop -> continueWith memo best' []
              where
                frameAfter w = if framePointer `elem` writes address w then Nothing else frame
            -- A return releases its bytes back to the entry's SP.
            returning cond bytes
              | any fst (outcomes cond) && offset /= fromIntegral bytes = (Left (UnbalancedReturn address), memo)
              | otherwise = continueWith memo best' [(next, offset, frame, m) | (False, m) <- outcomes cond]
            registerJump cond register
              | M.lookup register registers == Just EntryLink = returning cond (0 :: Word32)
              | otherwise = case resolve register of
                  Nothing -> (Left (UnresolvedBranch address), memo)
                  Just (targets, conditions) -> callTo memo targets $ \memo' best'' ->
                    continueWith memo' (addConditions conditions best'') [(next, offset, frame, m) | (False, m) <- outcomes cond]
            resolve register = case M.lookup address (callbackSites indirect) of
              Just site -> Just (callbackTargets site, S.singleton (callbackCondition site))
              Nothing -> case M.lookup register registers of
                Just (SlotPointer slot) -> case M.lookup (SlotOffset slot) (rootSlots indirect) of
                  Just targets@(_ : _) -> Just (targets, S.singleton SlotWordsFixed)
                  _ -> Nothing
                -- lock.method(&lock): the method's offset matches the lock r0 addresses.
                Just (LockMethod method) | M.lookup (A32.Reg 0) registers == Just (LockAddress (lockOf method)) ->
                  case [methods | ContextLocks kernel methods <- contextLocks indirect, inRange kernel address] of
                    [LockMethods acquire release] -> Just (if method `elem` [0xc, 0x18] then acquire else release, S.singleton ContextLocksFixed)
                    _ -> Nothing
                _ -> Nothing
            modeOf source = case source of
              Right value -> Just (value .&. 0x1f)
              Left register -> case M.lookup register registers of
                Just (Const value) -> Just (value .&. 0x1f)
                Just (ModeBits mode) -> Just mode
                _ -> Nothing
            -- The bound of an @ldrls pc, [pc, rN, lsl #2]@ table: the nearest earlier flag-setting
            -- instruction or write of rN, within four and with no branch between, is @cmp rN, #k@.
            tableBound cond index
              | cond /= A32.Cond 9 = Nothing
              | otherwise = guardAt 1
              where
                guardAt :: Word32 -> Maybe Word32
                guardAt back
                  | back > 4 = Nothing
                  | otherwise = case codeWord code (address - 4 * back) of
                      Nothing -> Nothing
                      Just w -> case A32.decode (address - 4 * back) w of
                        A32.DataProcessing (A32.Cond 14) A32.CMP True _ rn (A32.Immediate k) | rn == index -> Just (k + 1)
                        candidate
                          | setsFlags candidate || index `elem` writes (address - 4 * back) w || isBranch candidate -> Nothing
                          | otherwise -> guardAt (back + 1)
            atOffset value = Result (StackDepth (fromIntegral value)) [] [entry] S.empty S.empty

    clobber registers = foldr M.delete registers [A32.Reg n | n <- [0 .. 3] ++ [12, 14]]

    -- A conditional instruction the assumptions do not decide splits the path when executing it
    -- would change the tracked registers.
    evolveMachine :: Word32 -> Word32 -> Machine -> [Machine]
    evolveMachine address word (Machine before flags) = case conditional instruction of
      Nothing -> [Machine (track instruction before) (reset flags)]
      Just (cond, unconditional) -> case decided cond flags of
        Just True -> [Machine (track unconditional before) (reset flags)]
        Just False -> [Machine before flags]
        Nothing
          | executed == before -> [Machine before (reset flags)]
          | otherwise -> [Machine executed (reset (assume cond True flags)), Machine before (assume cond False flags)]
          where executed = track unconditional before
      where
        instruction = A32.decode address word
        reset current = if setsFlags instruction then M.empty else current
        -- Register values the resolution, the mode check and the return check use.
        track decoded registers = case decoded of
          A32.LoadMultiple (A32.Cond 14) base _ True regs | base == A32.sp && A32.lr `elem` regs ->
            M.insert A32.lr EntryLink (foldr M.delete registers regs)
          A32.Load (A32.Cond 14) A32.Word False rt base _ indexing
            | rt == A32.lr && base == A32.sp && indexing /= A32.OffsetOnly -> M.insert A32.lr EntryLink registers
          A32.MoveWide _ rd value -> M.insert rd (Const value) registers
          A32.MoveTop _ rd value -> case M.lookup rd registers of
            Just (Const low) -> M.insert rd (Const ((value `shiftL` 16) .|. (low .&. 0xffff))) registers
            _ -> M.delete rd registers
          A32.DataProcessing (A32.Cond 14) A32.MOV _ rd _ (A32.Immediate value) -> M.insert rd (Const value) registers
          A32.DataProcessing (A32.Cond 14) A32.MVN _ rd _ (A32.Immediate value) -> M.insert rd (Const (complement value)) registers
          A32.DataProcessing (A32.Cond 14) A32.MOV _ rd _ (A32.ShiftedImmediate source A32.LSL 0) ->
            maybe (M.delete rd registers) (\value -> M.insert rd value registers) (M.lookup source registers)
          A32.ReadStatus (A32.Cond 14) rd -> M.insert rd (ModeBits kernelMode) registers
          A32.DataProcessing (A32.Cond 14) op _ rd rn (A32.Immediate value) | op `elem` [A32.AND, A32.ORR, A32.BIC] ->
            set rd (combine op (M.lookup rn registers) (Just (Const value)))
          A32.DataProcessing (A32.Cond 14) op _ rd rn (A32.ShiftedImmediate rm A32.LSL 0) | op `elem` [A32.AND, A32.ORR, A32.BIC] ->
            set rd (combine op (M.lookup rn registers) (M.lookup rm registers))
          A32.Load (A32.Cond 14) A32.Word False rt base (A32.OffsetImmediate offset) A32.OffsetOnly ->
            set rt (loaded (M.lookup base registers) (Just (Const (fromIntegral (offset :: Int32)))) False)
          A32.DataProcessing (A32.Cond 14) A32.ADD False rd _ (A32.Immediate value) | value `elem` [8, 0x14] ->
            M.insert rd (LockAddress value) registers
          A32.Load (A32.Cond 14) A32.Word False rt base (A32.OffsetRegister True index A32.LSL 0) A32.OffsetOnly ->
            set rt (loaded (M.lookup base registers) (M.lookup index registers) False)
          A32.Load (A32.Cond 14) A32.Word False rt base (A32.OffsetRegister True _ A32.LSL 2) A32.OffsetOnly ->
            set rt (loaded (M.lookup base registers) Nothing True)
          _ -> foldr M.delete registers (writes address word ++ writeback decoded)
          where
            set rd value = maybe (M.delete rd registers) (\v -> M.insert rd v registers) value
    combine op left right = case (op, left, right) of
      (A32.AND, Just (Const a), Just (Const b)) -> Just (Const (a .&. b))
      (A32.ORR, Just (Const a), Just (Const b)) -> Just (Const (a .|. b))
      (A32.BIC, Just (Const a), Just (Const b)) -> Just (Const (a .&. complement b))
      (A32.AND, Just (ModeBits mode), Just (Const value)) -> Just (ModeBits (mode .&. value .&. 0x1f))
      -- An AND with a mask clear in bits 4:0 zeroes the mode field whatever the input.
      (A32.AND, _, Just (Const value)) | value .&. 0x1f == 0 -> Just (ModeBits 0)
      (A32.ORR, Just (ModeBits a), Just (ModeBits b)) -> Just (ModeBits ((a .|. b) .&. 0x1f))
      (A32.ORR, Just (ModeBits mode), Just (Const value)) -> Just (ModeBits ((mode .|. value) .&. 0x1f))
      (A32.ORR, Just (Const value), Just (ModeBits mode)) -> Just (ModeBits ((mode .|. value) .&. 0x1f))
      (A32.BIC, Just (ModeBits mode), Just (Const value)) -> Just (ModeBits (mode .&. complement value .&. 0x1f))
      _ -> Nothing
    -- A load from the root table gives a root; a load from a root at a known offset gives a slot.
    loaded base offset scaled = case (base, offset, scaled) of
      (Just (Const table), Just (Const 0), False) | isTable table -> Just RootEntry
      (Just (Const table), _, True) | isTable table -> Just RootEntry
      (Just RootEntry, Just (Const slot), False) -> Just (SlotPointer slot)
      (_, Just (Const method), False) | method `elem` lockMethodOffsets -> Just (LockMethod method)
      _ -> Nothing
    isTable value = value .&. 0x3fffffff == rootTable indirect .&. 0x3fffffff
    writeback instruction = case instruction of
      A32.Load _ _ _ _ base _ indexing | indexing /= A32.OffsetOnly -> [base]
      A32.Store _ _ _ base _ indexing | indexing /= A32.OffsetOnly -> [base]
      A32.LoadMultiple _ base _ True _ -> [base]
      A32.StoreMultiple _ base _ True _ -> [base]
      _ -> []

    deeper a@(Result (StackDepth x) reachedA _ condA enteredA) b@(Result (StackDepth y) reachedB _ condB enteredB) =
      let pick = if y > x then b else a
          Result depth _ chain _ _ = pick
       in Result depth (reachedA ++ reachedB) chain (S.union condA condB) (S.union enteredA enteredB)
    addConditions extra (Result depth reached chain conditions entered) = Result depth reached chain (S.union conditions extra) entered
    addReached more (Result depth reached chain conditions entered) = Result depth (reached ++ more) chain conditions entered

-- | Context offsets of the lock methods: acquire and release of the lock at +8, then of the lock at +0x14.
lockMethodOffsets :: [Word32]
lockMethodOffsets = [0xc, 0x10, 0x18, 0x1c]

-- | The lock a method at a context offset belongs to.
lockOf :: Word32 -> Word32
lockOf method = if method `elem` [0xc, 0x10] then 8 else 0x14

-- | The context lock methods (acquire, release) one kernel's context initializer
-- (@kernel_initialize_owner_contexts@) stores: each @str rX, [r3, #m]@ at a lock-method offset
-- of a register holding a @movw/movt@ constant, acquire at +0xc and +0x18, release at +0x10 and +0x1c.
contextLockMethods :: StockCode -> CodeRange -> Either String LockMethods
contextLockMethods code (CodeRange initializer initializerEnd) = do
  let instructions = [(at, word) | at <- [initializer, initializer + 4 .. initializerEnd - 4], Just word <- [codeWord code at]]
      stored = snd (foldl step (M.empty, []) instructions)
      methods offsets = S.toList (S.fromList [value | (offset, value) <- stored, offset `elem` offsets])
      acquire = methods [0xc, 0x18]
      release = methods [0x10, 0x1c]
  if null acquire || null release then Left "context lock methods not found in the initializer" else Right (LockMethods acquire release)
  where
    step :: (M.Map A32.Reg Word32, [(Word32, Word32)]) -> (Word32, Word32) -> (M.Map A32.Reg Word32, [(Word32, Word32)])
    step (constants, stores) (at, word) = case A32.decode at word of
      A32.MoveWide _ rd value -> (M.insert rd value constants, stores)
      A32.MoveTop _ rd value -> (M.adjust (\low -> (value `shiftL` 16) .|. (low .&. 0xffff)) rd constants, stores)
      A32.Store _ A32.Word rt _ (A32.OffsetImmediate offset) A32.OffsetOnly
        | Just value <- M.lookup rt constants, fromIntegral offset `elem` lockMethodOffsets -> (constants, (fromIntegral offset, value) : stores)
      _ -> (foldr M.delete constants (writes at word), stores)

-- | @bx lr@ or @mov pc, lr@.
viaLink :: A32.Instruction -> Bool
viaLink instruction = case instruction of
  A32.BranchExchange _ False register -> register == A32.lr
  A32.DataProcessing _ A32.MOV _ rd _ (A32.ShiftedImmediate source A32.LSL 0) -> rd == A32.pc && source == A32.lr
  _ -> False

isBranch :: A32.Instruction -> Bool
isBranch instruction = case instruction of
  A32.Branch {} -> True
  A32.BranchExchange {} -> True
  _ -> False
