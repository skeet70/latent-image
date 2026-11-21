-- | A static stack-depth bound over flat stock code: the depth of an entry is the deepest SP
-- offset any path from it reaches, a call adding the callee's depth at the call's offset. The
-- walk follows control flow (no function extents), tracks the offset per instruction and refuses
-- where two paths reach one instruction at different offsets. Owned code is not walked: a supplied
-- entry is taken at its depth wherever a path reaches it, and entering owned code elsewhere refuses.
--
-- Values a path stores in its own frame are reloaded from it until a frame address is copied
-- (a pointer formed below the entry SP addresses only this frame) or a callee may write the slot:
-- each closure records how far above its entry SP it may write, unbounded once it stores through
-- or derives a pointer from its caller's r11, reaches supplied code or forms a pointer at or above
-- its entry SP.
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
  , SupervisorEntry (..)
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
import Data.Bits (complement, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Unsafe as BU
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import qualified Data.Set as S
import Data.Word (Word32)
import StackWalk (Effect (..), Refused (SupervisorCall, UnaccountedSpWrite), Strictness (Strict), classify, framePointer, writes)

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
  | -- | A word loaded from a constant address holds its image value or a constant one of its scanned
    -- direct stores writes; no computed-pointer store writes it (S6).
    LoadedWordsClosed
  | -- | Each kernel's exception-reporter hook word holds only the targets its writer check names (S7).
    ReporterHookClosed
  | -- | A nonzero SVC starts a fatal report after which the task does not resume normal operation;
    -- a bound under it covers the paths before the report (S8).
    FatalReportExcluded
  | -- | A declared register-branch target set holds: the words it reads are written only by the
    -- direct stores its check scans, and words in image 5's BSS hold 0 before their first store (S9).
    DeclaredTargetsClosed
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

-- | How one kernel takes an SVC with a nonzero comment from task code in SVC mode: its handler
-- stores `supervisorFrame` bytes on the caller's stack and calls the reporter, whose instruction at
-- `supervisorResume` restores SP to that frame for the exception return to the instruction after the SVC.
data SupervisorEntry = SupervisorEntry
  { supervisorKernel :: CodeRange
  , supervisorFrame :: Word32
  , supervisorReporter :: Word32
  , supervisorResume :: Word32
  }
  deriving (Eq, Show)

data IndirectTargets = IndirectTargets
  { -- | Guest address of the interface root table (0x96bd20).
    rootTable :: Word32
  , -- | Slot offset to the services of table A and table B.
    rootSlots :: M.Map SlotOffset [Word32]
  , -- | Register-branch PCs with closed target sets.
    callbackSites :: M.Map Word32 CallbackSite
  , contextLocks :: [ContextLocks]
  , -- | The values a word at a constant address can hold, when its writers close it (S6).
    loadedWords :: Word32 -> Maybe [Word32]
  , -- | An SVC in a kernel's range enters that kernel's handler; one outside every range may enter any.
    supervisorEntries :: [SupervisorEntry]
  , -- | Whether a nonzero SVC ends the path under S8 instead of entering its handler.
    excludeFatalReports :: Bool
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
  | -- | A word loaded from a constant address.
    WordAt Word32
  | -- | r11 as the entry received it: the caller's frame pointer.
    CallerFrame
  deriving (Eq, Ord, Show)

type Registers = M.Map A32.Reg Value

-- | Words this activation stored in its frame, by depth below the entry SP, or `Escaped` once a
-- frame address has been copied.
data Slots = Slots (M.Map Int Value) | Escaped deriving (Eq, Ord, Show)

-- | Tracked registers, the condition outcomes a path assumes (condition code to taken), frame slots,
-- and the register the last flag-setting instruction compared with zero.
data Machine = Machine Registers (M.Map Word32 Bool) Slots (Maybe A32.Reg) deriving (Eq, Ord, Show)

-- | Bytes above its entry SP a closure may write.
data Reach = Above Int | Anywhere deriving (Eq, Show)

widest :: Reach -> Reach -> Reach
widest a b = case (a, b) of
  (Above x, Above y) -> Above (max x y)
  _ -> Anywhere

-- | A callee's reach from a call at the offset, above the caller's entry SP.
shifted :: Int -> Reach -> Reach
shifted offset reach = case reach of
  Above bytes -> Above (max 0 (bytes - offset))
  Anywhere -> Anywhere

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

-- | Whether some path of a walk returns to its caller.
data Returning = Returns | NeverReturns deriving (Eq, Show)

eitherReturns :: Returning -> Returning -> Returning
eitherReturns a b = if a == Returns || b == Returns then Returns else NeverReturns

data Result = Result StackDepth [Word32] [Word32] (S.Set Condition) (S.Set Word32) Reach Returning

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
    report (Result depth reached chain conditions entered _ _) = StackReport depth (S.toList (S.fromList reached)) chain conditions (S.toList entered)

    resumes = S.fromList (map supervisorResume (supervisorEntries indirect))

    depthOf :: Memo -> [Word32] -> Word32 -> (Either StackRefusal Result, Memo)
    depthOf memo stack entry
      | Just depth <- M.lookup entry supplied = (Right (Result depth [entry] [entry] S.empty S.empty Anywhere Returns), memo)
      | Just known <- M.lookup entry memo = (known, memo)
      | entry `elem` stack = (Left (Recursion (reverse (entry : stack))), memo)
      | owned code entry = (Left (OwnedCode entry), memo)
      | otherwise =
          let (result, memo') = walk memo (entry : stack) entry
           in (result, M.insert entry result memo')

    -- One state per path: address, SP offset below entry, r11's offset if known, and the
    -- tracked registers with the condition outcomes the path has assumed since flags were last set.
    walk :: Memo -> [Word32] -> Word32 -> (Either StackRefusal Result, Memo)
    walk memo0 stack entry = loop memo0 [(entry, 0, Nothing, Machine entryRegisters M.empty (Slots M.empty) Nothing)] M.empty S.empty
      (Result (StackDepth 0) [] [entry] S.empty (S.singleton entry) (Above 0) NeverReturns)
      where
        entryRegisters = M.fromList [(A32.lr, EntryLink), (framePointer, CallerFrame)]
        loop :: Memo -> [(Word32, Int, Maybe Int, Machine)] -> M.Map Word32 (Int, Maybe Int) -> S.Set (Word32, Machine) -> Result
          -> (Either StackRefusal Result, Memo)
        loop memo [] _ _ best = (Right best, memo)
        loop memo ((address, offset, frame, machine) : rest) offsets seen best
          | S.member (address, machine) seen && M.lookup address offsets == Just (offset, frame) = loop memo rest offsets seen best
          | Just (known, knownFrame) <- M.lookup address offsets, (known, knownFrame) /= (offset, frame) =
              (Left (if known /= offset then OffsetMismatch address else FrameUnknown address), memo)
          | S.member address (switchEnds switches) || S.member address resumes = loop memo rest offsets' seen' (returned best')
          | Just (StackDepth depth) <- M.lookup address supplied =
              loop memo rest offsets' seen'
                (deeper best (Result (StackDepth (fromIntegral offset + depth)) [address] [entry, address] S.empty S.empty Anywhere Returns))
          | owned code address = (Left (OwnedCode address), memo)
          | otherwise = case codeWord code address of
              Nothing -> (Left (NoCode address), memo)
              Just word -> step word
          where
            Machine registers flags slots tested = machine
            offsets' = M.insert address (offset, frame) offsets
            seen' = S.insert (address, machine) seen
            best' = deeper best (atOffset offset)
            next = address + 4
            -- A state's slots hold only words at or above its SP.
            continueWith memo' best'' states =
              loop memo' ([(at, o, f, aboveSp o m) | (at, o, f, m) <- states] ++ rest) offsets' seen' best''
            aboveSp o (Machine r c s z) = Machine r c (case s of
              Slots held -> Slots (M.filterWithKey (\position _ -> position <= o) held)
              Escaped -> Escaped) z
            resumed = case M.lookup address (resumeEdges switches) of
              Just pc -> [(pc, offset, frame, Machine M.empty M.empty (Slots M.empty) Nothing)]
              Nothing -> []
            instruction word = A32.decode address word
            -- The machines after a non-branch instruction: one, or two when its condition is undecided.
            evolve word = evolveMachine address word offset frame machine
            -- Outcomes of a conditional transfer: (taken, machine) pairs the assumptions allow.
            outcomes cond = case decided cond flags of
              Just taken -> [(taken, machine)]
              Nothing -> [(True, Machine registers (assume cond True flags) slots tested), (False, Machine registers (assume cond False flags) slots tested)]
            -- Each (call offset, target) walked; the continuation receives the deepest result, the widest callee
            -- reach and whether some target returns.
            callTo memo' targets k = do
              let visit (acc, m) (at, target) = case acc of
                    Left refusal -> (Left refusal, m)
                    Right (deepest, reached, conditions, reach, returns) ->
                      let (result, m') = depthOf m stack target
                       in case result of
                            Left refusal -> (Left refusal, m')
                            Right (Result (StackDepth depth) more chain extra entered calleeReach calleeReturns) ->
                              let candidate = Result (StackDepth (fromIntegral at + depth)) [] (entry : chain) S.empty entered (shifted at calleeReach) NeverReturns
                               in (Right (deeper deepest candidate, reached ++ more, S.union conditions extra, widest reach (shifted (at - offset) calleeReach), eitherReturns returns calleeReturns), m')
                  (outcome, memo'') = foldl visit (Right (best', [], S.empty, Above 0, NeverReturns), memo') targets
               in case outcome of
                    Left refusal -> (Left refusal, memo'')
                    Right (deepest, reached, conditions, reach, returns) -> k memo'' (addReached reached (addConditions conditions deepest)) reach returns
            -- The path continues after a call only when some target returns.
            afterReturn returns states = if returns == Returns then states else []
            -- The caller-saved registers are clobbered; slots in [SP, SP + reach) may be overwritten.
            afterCall reach = Machine (clobber registers) M.empty (exposed reach) Nothing
            exposed reach = case (slots, reach) of
              (Escaped, _) -> Escaped
              (Slots _, Anywhere) -> Slots M.empty
              (Slots held, Above bytes) -> Slots (M.filterWithKey (\position _ -> position <= offset - bytes) held)
            step word = case classify Strict address word of
              Reserve amount -> continueWith memo (deeper here (atOffset (offset + fromIntegral amount)))
                [(next, offset + fromIntegral amount, frameAfter word, m) | m <- evolve word]
              Release amount
                | offset - fromIntegral amount < 0 -> (Left (Refusal address UnaccountedSpWrite), memo)
                | otherwise -> continueWith memo here [(next, offset - fromIntegral amount, frameAfter word, m) | m <- evolve word]
              FromFrame relative -> case frame of
                Just fp -> continueWith memo here [(next, fp - fromIntegral relative, frame, m) | m <- evolve word]
                Nothing -> (Left (FrameUnknown address), memo)
              SetFrame amount -> continueWith memo here [(next, offset, Just (offset - fromIntegral amount), m) | m <- evolve word]
              Neutral -> continueWith memo here ([(next, offset, frameAfter word, m) | m <- evolve word] ++ resumed)
              Call target -> callTo memo [(offset, target)] $ \memo' best'' reach returns ->
                continueWith memo' best'' (afterReturn returns [(next, offset, frame, afterCall reach)])
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
                Just (targets, conditions) -> callTo memo [(offset, target) | target <- targets] $ \memo' best'' reach returns ->
                  continueWith memo' (addConditions conditions best'') (afterReturn returns [(next, offset, frame, afterCall reach)])
              RegisterJump cond register -> registerJump cond register
              JumpTable cond index -> case tableBound cond index of
                Nothing -> (Left (TableUnbounded address), memo)
                Just count -> case mapM (\slot -> codeWord code (address + 8 + 4 * slot)) [0 .. count - 1] of
                  Nothing -> (Left (TableUnbounded address), memo)
                  Just targets -> continueWith memo best'
                    ([(target, offset, frame, m) | (True, m) <- outcomes cond, target <- targets] ++ [(next, offset, frame, m) | (False, m) <- outcomes cond])
              ModeWrite source
                | modeOf source == Just kernelMode -> continueWith memo here [(next, offset, frame, Machine registers M.empty slots Nothing)]
                | otherwise -> (Left (ModeChange address), memo)
              Refuse SupervisorCall
                | excludeFatalReports indirect, Just _ <- supervisorTargets word ->
                    continueWith memo (addConditions (S.singleton FatalReportExcluded) best') []
                | Just targets <- supervisorTargets word -> callTo memo targets $ \memo' best'' reach returns ->
                    continueWith memo' best'' (afterReturn returns [(next, offset, frame, Machine M.empty M.empty (exposed reach) Nothing)])
              -- @ldm<c> sp!, {..., lr}@ then @bx<c> lr@: a return when the condition holds, which restores
              -- the link the entry's frame saved; otherwise the path continues past both.
              Refuse UnaccountedSpWrite
                | A32.LoadMultiple cond base A32.IncrementAfter True regs <- instruction word
                , base == A32.sp && cond /= A32.Cond 14 && A32.pc `notElem` regs && A32.sp `notElem` regs && A32.lr `elem` regs
                , Just (A32.BranchExchange cond' False link) <- A32.decode next <$> codeWord code next
                , cond' == cond && link == A32.lr ->
                    let restored = case slots of
                          Slots held -> M.lookup (offset - 4 * (length regs - 1)) held == Just EntryLink
                          Escaped -> False
                     in if any fst (outcomes cond) && (offset /= 4 * length regs || not restored) then (Left (UnbalancedReturn address), memo)
                        else continueWith memo (if any fst (outcomes cond) then returned best' else best') [(next + 4, offset, frame, m) | (False, m) <- outcomes cond]
              Refuse reason -> (Left (Refusal address reason), memo)
              Stop -> continueWith memo best' []
              where
                frameAfter w = if framePointer `elem` writes address w then Nothing else frame
                here = addReach (instructionReach (instruction word) word offset frame registers) best'
            -- A return releases its bytes back to the entry's SP.
            returning cond bytes
              | any fst (outcomes cond) && offset /= fromIntegral bytes = (Left (UnbalancedReturn address), memo)
              | otherwise = continueWith memo (if any fst (outcomes cond) then returned best' else best') [(next, offset, frame, m) | (False, m) <- outcomes cond]
            registerJump cond register
              | M.lookup register registers == Just EntryLink = returning cond (0 :: Word32)
              | otherwise = case resolve register of
                  Nothing -> (Left (UnresolvedBranch address), memo)
                  Just (targets, conditions) -> callTo memo [(offset, target) | target <- targets] $ \memo' best'' _ returns ->
                    let tail' = if any fst (outcomes cond) && returns == Returns then returned best'' else best''
                     in continueWith memo' (addConditions conditions tail') [(next, offset, frame, m) | (False, m) <- outcomes cond]
            -- A declared or premised site's set; a zero in it is not called on a path that assumes the
            -- register compared unequal to zero.
            resolve register = case M.lookup address (callbackSites indirect) of
              Just site
                | 0 `notElem` callbackTargets site || nonzero register -> Just (filter (/= 0) (callbackTargets site), S.singleton (callbackCondition site))
                | otherwise -> Nothing
              Nothing -> case M.lookup register registers of
                Just (SlotPointer slot) -> case M.lookup (SlotOffset slot) (rootSlots indirect) of
                  Just targets@(_ : _) -> Just (targets, S.singleton SlotWordsFixed)
                  _ -> Nothing
                -- lock.method(&lock): the method's offset matches the lock r0 addresses.
                Just (LockMethod method) | M.lookup (A32.Reg 0) registers == Just (LockAddress (lockOf method)) ->
                  case [methods | ContextLocks kernel methods <- contextLocks indirect, inRange kernel address] of
                    [LockMethods acquire release] -> Just (if method `elem` [0xc, 0x18] then acquire else release, S.singleton ContextLocksFixed)
                    _ -> Nothing
                Just (WordAt word) -> case loadedWords indirect word of
                  Just values | 0 `notElem` values || nonzero register ->
                    Just (filter (/= 0) values, S.singleton LoadedWordsClosed)
                  _ -> Nothing
                _ -> Nothing
            nonzero register = tested == Just register && decided (A32.Cond 0) flags == Just False
            -- An SVC with a nonzero comment calls its kernel's reporter below the handler's frame.
            supervisorTargets word
              | word .&. 0x0f000000 /= 0x0f000000 || word .&. 0x00ffffff == 0 = Nothing
              | otherwise = case [e | e <- supervisorEntries indirect, inRange (supervisorKernel e) address] of
                  [] | null (supervisorEntries indirect) -> Nothing
                     | otherwise -> Just (map frameCall (supervisorEntries indirect))
                  local -> Just (map frameCall local)
              where frameCall e = (offset + fromIntegral (supervisorFrame e), supervisorReporter e)
            modeOf source = case source of
              Right value -> Just (value .&. 0x1f)
              Left register -> case M.lookup register registers of
                Just (Const value) -> Just (value .&. 0x1f)
                Just (ModeBits mode) -> Just mode
                _ -> Nothing
            -- The bound of an @ldrls pc, [pc, rN, lsl #2]@ table: the nearest earlier flag-setting
            -- instruction or write of rN, within four and with no branch between, is @cmp rN, #k@, or
            -- @cmp rN, rM@ with the nearest earlier write of rM within four more @mov@/@movw rM, #k@.
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
                        A32.DataProcessing (A32.Cond 14) A32.CMP True _ rn (A32.ShiftedImmediate rm A32.LSL 0)
                          | rn == index && rm /= index -> (+ 1) <$> constantAt rm (back + 1) (back + 4)
                        candidate
                          | setsFlags candidate || index `elem` writes (address - 4 * back) w || isBranch candidate -> Nothing
                          | otherwise -> guardAt (back + 1)
                constantAt :: A32.Reg -> Word32 -> Word32 -> Maybe Word32
                constantAt register back limit
                  | back > limit = Nothing
                  | otherwise = case codeWord code (address - 4 * back) of
                      Nothing -> Nothing
                      Just w -> case A32.decode (address - 4 * back) w of
                        A32.MoveWide (A32.Cond 14) rd k | rd == register -> Just k
                        A32.DataProcessing (A32.Cond 14) A32.MOV _ rd _ (A32.Immediate k) | rd == register -> Just k
                        candidate
                          | register `elem` writes (address - 4 * back) w || isBranch candidate -> Nothing
                          | otherwise -> constantAt register (back + 1) limit
            atOffset value = Result (StackDepth (fromIntegral value)) [] [entry] S.empty S.empty (Above 0) NeverReturns

    clobber registers = foldr M.delete registers [A32.Reg n | n <- [0 .. 3] ++ [12, 14]]

    -- A conditional instruction the assumptions do not decide splits the path when executing it
    -- would change the tracked registers or slots.
    evolveMachine :: Word32 -> Word32 -> Int -> Maybe Int -> Machine -> [Machine]
    evolveMachine address word offset frame (Machine before flags slots tested) = case conditional instruction of
      Nothing -> case decided (conditionOf instruction) flags of
        Just False -> [Machine before flags slots tested]
        Just True -> [Machine (track instruction before) (reset flags) (stored True instruction) (compared instruction)]
        Nothing -> [Machine (track instruction before) (reset flags) (stored False instruction) (compared instruction)]
      Just (cond, unconditional) -> case decided cond flags of
        Just True -> [Machine (track unconditional before) (reset flags) (stored True unconditional) (compared unconditional)]
        Just False -> [Machine before flags slots tested]
        Nothing
          | executed == before && executedSlots == slots && executedTest == tested -> [Machine before (reset flags) slots tested]
          | otherwise -> [Machine executed (reset (assume cond True flags)) executedSlots executedTest, Machine before (assume cond False flags) slots tested]
          where
            executed = track unconditional before
            executedSlots = stored True unconditional
            executedTest = compared unconditional
      where
        instruction = A32.decode address word
        -- The register a flag-setting instruction compares with zero; another write of it forgets it.
        compared decoded
          | setsFlags decoded = case decoded of
              A32.DataProcessing _ A32.CMP True _ rn (A32.Immediate 0) -> Just rn
              _ -> Nothing
          | otherwise = case tested of
              Just register | register `elem` writes address word ++ writeback decoded -> Nothing
              other -> other
        reset current = if setsFlags instruction then M.empty else current
        ownFrame register
          | register == A32.sp = Just offset
          | register == framePointer = frame
          | otherwise = Nothing
        slotValue position = case slots of
          Slots held -> M.lookup position held
          Escaped -> Nothing
        restore reg value registers = maybe (M.delete reg registers) (\v -> M.insert reg v registers) value
        -- The slots after a store the path executes (certain) or may execute.
        stored certain decoded = case slots of
          Escaped -> Escaped
          Slots held
            | formsFrameAddress decoded word frame -> Escaped
            | otherwise -> case frameWrites decoded word offset frame of
                Nothing -> Slots M.empty
                Just written' -> Slots (foldl (\held' (position, width, source) ->
                  let cleared = M.filterWithKey (\key _ -> key <= position - width || key >= position + 4) held'
                   in case source of
                        Just register | certain, width == 4, Just value <- M.lookup register before, register /= A32.sp -> M.insert position value cleared
                        _ -> cleared) held written')
        -- Register values the resolution, the mode check and the return check use.
        track decoded registers = case decoded of
          A32.LoadMultiple (A32.Cond 14) base mode updated regs
            | Just start <- ownFrame base, mode == A32.IncrementAfter ->
                let restored = foldl (\r (index, reg) -> restore reg (slotValue (start - 4 * index)) r) registers (zip [0 ..] regs)
                 in if base == A32.sp && updated && A32.lr `elem` regs then M.insert A32.lr EntryLink restored
                    else if updated && base /= A32.sp then M.delete base restored else restored
          A32.LoadMultiple (A32.Cond 14) base _ True regs | base == A32.sp && A32.lr `elem` regs ->
            M.insert A32.lr EntryLink (foldr M.delete registers regs)
          A32.Load (A32.Cond 14) A32.Word False rt base _ indexing
            | rt == A32.lr && base == A32.sp && indexing /= A32.OffsetOnly -> M.insert A32.lr EntryLink registers
          A32.Load (A32.Cond 14) A32.Word False rt base (A32.OffsetImmediate imm) A32.OffsetOnly
            | Just start <- ownFrame base -> restore rt (slotValue (start - fromIntegral imm)) registers
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
          A32.Load (A32.Cond 14) A32.Word False rt base (A32.OffsetImmediate offset') A32.OffsetOnly ->
            set rt (loaded (M.lookup base registers) (Just (Const (fromIntegral (offset' :: Int32)))) False)
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
    -- A load from the root table gives a root; a load from a root at a known offset gives a slot;
    -- any other load from a constant address names its word.
    loaded base offset scaled = case (base, offset, scaled) of
      (Just (Const table), Just (Const 0), False) | isTable table -> Just RootEntry
      (Just (Const table), _, True) | isTable table -> Just RootEntry
      (Just RootEntry, Just (Const slot), False) -> Just (SlotPointer slot)
      (_, Just (Const method), False) | method `elem` lockMethodOffsets -> Just (LockMethod method)
      (Just (Const address), Just (Const displacement), False) -> Just (WordAt (address + displacement))
      _ -> Nothing
    isTable value = value .&. 0x3fffffff == rootTable indirect .&. 0x3fffffff
    writeback instruction = case instruction of
      A32.Load _ _ _ _ base _ indexing | indexing /= A32.OffsetOnly -> [base]
      A32.Store _ _ _ base _ indexing | indexing /= A32.OffsetOnly -> [base]
      A32.LoadMultiple _ base _ True _ -> [base]
      A32.StoreMultiple _ base _ True _ -> [base]
      _ -> []

    -- Bytes above the entry SP the instruction may write: its own stores, and any store through,
    -- arithmetic on or store of the caller's frame pointer, or pointer formed at or above the entry SP.
    instructionReach :: A32.Instruction -> Word32 -> Int -> Maybe Int -> Registers -> Reach
    instructionReach decoded word offset frame registers = case decoded of
      A32.Store _ width rt base displacement indexing
        | Just start <- own base -> case displacement of
            A32.OffsetImmediate imm ->
              let low = (if indexing == A32.PostIndexed then 0 else fromIntegral imm) - start
               in widest (Above (max 0 (low + widthBytes width))) (storedPointer rt)
            A32.OffsetRegister {} -> Anywhere
        | callerFrame base || callerFrame rt -> Anywhere
        | otherwise -> storedPointer rt
      A32.StoreMultiple _ base mode _ regs
        | Just start <- own base -> foldr (widest . storedPointer) (Above (max 0 (maximum (map (+ 4) (blockOffsets mode (length regs))) - start))) regs
        | callerFrame base || any callerFrame regs -> Anywhere
        | otherwise -> foldr (widest . storedPointer) (Above 0) regs
      A32.DataProcessing _ op _ rd rn operand
        | op `elem` [A32.TST, A32.TEQ, A32.CMP, A32.CMN] || rd `elem` [A32.sp, framePointer] -> Above 0
        | op == A32.MOV, A32.ShiftedImmediate source A32.LSL 0 <- operand, callerFrame source -> Above 0
        | any callerFrame sources -> Anywhere
        | any (isJust . own) sources -> case (op, operand) of
            (A32.MOV, A32.ShiftedImmediate source A32.LSL 0) -> pointer source 0
            (A32.ADD, A32.Immediate imm) -> pointer rn (fromIntegral imm)
            (A32.SUB, A32.Immediate imm) -> pointer rn (negate (fromIntegral imm))
            _ -> Anywhere
        | otherwise -> Above 0
        where sources = [rn | op `notElem` [A32.MOV, A32.MVN]] ++ operandRegisters operand
      A32.Multiply _ _ a b c | any framedOrCaller (a : b : maybe [] pure c) -> Anywhere
      A32.Compute _ _ | any framedOrCaller (A32.computeSources word) -> Anywhere
      A32.Rejected _
        | word .&. 0x0f900ff0 == 0x01800f90 || word .&. 0x0fb00ff0 == 0x01000090
        , framedOrCaller (A32.Reg (fromIntegral ((word `shiftR` 16) .&. 0xf))) -> Anywhere
      _ -> Above 0
      where
        own register
          | register == A32.sp = Just offset
          | register == framePointer = frame
          | otherwise = Nothing
        callerFrame register = register /= A32.sp && M.lookup register registers == Just CallerFrame && (register /= framePointer || frame == Nothing)
        framedOrCaller register = callerFrame register || isJust (own register)
        -- A frame address stored as data, or a pointer formed from one.
        storedPointer register = case own register of
          Just start -> pointerAt (negate start)
          Nothing -> Above 0
        pointer register displacement = case own register of
          Just start -> pointerAt (displacement - start)
          Nothing -> Above 0
        pointerAt relative = if relative >= 0 then Anywhere else Above 0

    -- The own-frame words a store writes, by depth below the entry SP with width and source
    -- register, or Nothing when the address within the frame is unknown.
    frameWrites :: A32.Instruction -> Word32 -> Int -> Maybe Int -> Maybe [(Int, Int, Maybe A32.Reg)]
    frameWrites decoded word offset frame = case decoded of
      A32.Store _ width rt base displacement indexing | Just start <- own base -> case displacement of
        A32.OffsetImmediate imm -> Just [(start - (if indexing == A32.PostIndexed then 0 else fromIntegral imm), widthBytes width, if width == A32.Word then Just rt else Nothing)]
        A32.OffsetRegister {} -> Nothing
      A32.StoreMultiple _ base mode _ regs | Just start <- own base ->
        Just [(start - delta, 4, Just reg) | (delta, reg) <- zip (blockOffsets mode (length regs)) regs]
      A32.Rejected _
        | word .&. 0x0f900ff0 == 0x01800f90 || word .&. 0x0fb00ff0 == 0x01000090
        , isJust (own (A32.Reg (fromIntegral ((word `shiftR` 16) .&. 0xf)))) -> Nothing
      _ -> Just []
      where
        own register
          | register == A32.sp = Just offset
          | register == framePointer = frame
          | otherwise = Nothing

    -- Whether the instruction copies an address in this frame (SP, or r11 holding the frame base).
    formsFrameAddress :: A32.Instruction -> Word32 -> Maybe Int -> Bool
    formsFrameAddress decoded word frame = case decoded of
      A32.DataProcessing _ op _ rd rn operand
        | op `elem` [A32.TST, A32.TEQ, A32.CMP, A32.CMN] || rd `elem` [A32.sp, framePointer] -> False
        | otherwise -> any framed ([rn | op `notElem` [A32.MOV, A32.MVN]] ++ operandRegisters operand)
      A32.Store _ _ rt _ _ _ -> framed rt
      A32.StoreMultiple _ _ _ _ regs -> any framed regs
      A32.Compute _ _ -> any framed (A32.computeSources word)
      A32.Multiply _ _ a b c -> any framed (a : b : maybe [] pure c)
      _ -> False
      where framed register = register == A32.sp || (register == framePointer && isJust frame)

    deeper a@(Result (StackDepth x) reachedA _ condA enteredA reachA returnsA) b@(Result (StackDepth y) reachedB _ condB enteredB reachB returnsB) =
      let pick = if y > x then b else a
          Result depth _ chain _ _ _ _ = pick
       in Result depth (reachedA ++ reachedB) chain (S.union condA condB) (S.union enteredA enteredB) (widest reachA reachB) (eitherReturns returnsA returnsB)
    addConditions extra (Result depth reached chain conditions entered reach returns) = Result depth reached chain (S.union conditions extra) entered reach returns
    addReached more (Result depth reached chain conditions entered reach returns) = Result depth (reached ++ more) chain conditions entered reach returns
    addReach extra (Result depth reached chain conditions entered reach returns) = Result depth reached chain conditions entered (widest reach extra) returns
    returned (Result depth reached chain conditions entered reach _) = Result depth reached chain conditions entered reach Returns

-- | The condition of a store, the instructions whose frame effect `evolveMachine` applies directly.
conditionOf :: A32.Instruction -> A32.Cond
conditionOf instruction = case instruction of
  A32.Store cond _ _ _ _ _ -> cond
  A32.StoreMultiple cond _ _ _ _ -> cond
  _ -> A32.Cond 14

-- | Byte offsets from the base of each register a block transfer of the mode moves, lowest first.
blockOffsets :: A32.BlockMode -> Int -> [Int]
blockOffsets mode count = [first + 4 * index | index <- [0 .. count - 1]]
  where
    first = case mode of
      A32.IncrementAfter -> 0
      A32.IncrementBefore -> 4
      A32.DecrementAfter -> 4 - 4 * count
      A32.DecrementBefore -> negate (4 * count)

widthBytes :: A32.Width -> Int
widthBytes width = case width of
  A32.Byte -> 1
  A32.Half -> 2
  A32.Word -> 4
  A32.Dual -> 8

operandRegisters :: A32.Operand -> [A32.Reg]
operandRegisters operand = case operand of
  A32.Immediate _ -> []
  A32.ShiftedImmediate register _ _ -> [register]
  A32.ShiftedRegister register _ amount -> [register, amount]

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
