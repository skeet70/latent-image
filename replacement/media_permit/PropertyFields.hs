{-# LANGUAGE GHC2021 #-}

-- | Per-property storage fields of the X-M1 1.50 property getter and setter,
-- read from their jump-table arms by a bounded path-forking interpreter.
module PropertyFields
  ( PropertyId (..)
  , ArgumentIndex (..)
  , PropertyTable (..)
  , Loc (..)
  , Extract (..)
  , Source (..)
  , ArmResult (..)
  , StoreRecord (..)
  , Outcome (..)
  , Guard (..)
  , GuardSubject (..)
  , GuardCondition (..)
  , PathEnd (..)
  , CutReason (..)
  , PathStore (..)
  , ExitRecord (..)
  , PathRecord (..)
  , Arm (..)
  , tableArms
  , getterTable
  , setterTable
  , getterArm
  , setterArm
  , getterContinuation
  , setterContinuation
  , maxPaths
  , maxSteps
  ) where

import qualified A32 as A
import Data.Bits (popCount, shiftL, shiftR, (.&.))
import Data.Int (Int32)
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import ServiceInventory (Image, imageWord)

newtype PropertyId = PropertyId Word32 deriving (Eq, Ord, Show)

-- | Position of an incoming function argument (0 for r0 at the function's entry).
newtype ArgumentIndex = ArgumentIndex Int deriving (Eq, Ord, Show)

-- | A dispatching function whose `ldrls pc, [pc, rN, lsl #2]` selects an arm by property - 1.
data PropertyTable = PropertyTable
  { tableFunction :: Word32
  , tableDispatch :: Word32
  , tableEntries :: Int
  -- | Shared continuation the arms branch to; a path reaching it ends there.
  , tableExit :: Maybe Word32
  }

-- | The property getter 0x61e110(property, bank, defaults flag, slot) and setter 0x624694(property, value, bank, slot).
getterTable, setterTable :: PropertyTable
getterTable = PropertyTable 0x61e110 0x61e124 645 Nothing
setterTable = PropertyTable 0x624694 0x6246c4 645 (Just 0x630200)

data Arm = Arm { armProperty :: PropertyId, armTarget :: Word32 } deriving (Eq, Show)

-- | The fallback target and the arms after the dispatch, or why the dispatch is not the expected form.
tableArms :: Image -> PropertyTable -> Either String (Word32, [Arm])
tableArms image table = do
  let dispatch = tableDispatch table
      word what address = maybe (Left (what ++ " outside image")) Right (imageWord image address)
  raw <- word "dispatch word" dispatch
  compareWord <- word "bound word" (dispatch - 4)
  defaultWord <- word "default branch" (dispatch + 4)
  case (A.decode (dispatch - 4) compareWord, A.decode dispatch raw, A.decode (dispatch + 4) defaultWord) of
    (A.DataProcessing (A.Cond 14) A.CMP _ _ _ (A.Immediate bound), A.Load (A.Cond 9) A.Word False destination base (A.OffsetRegister True _ A.LSL 2) A.OffsetOnly, A.Branch (A.Cond 14) False fallback)
      | destination == A.pc && base == A.pc && fromIntegral bound + 1 == tableEntries table -> do
          targets <- mapM (\index -> word "table word" (dispatch + 8 + 4 * fromIntegral index)) [0 .. tableEntries table - 1]
          pure (fallback, [Arm (PropertyId (fromIntegral index + 1)) target | (index, target) <- zip [0 :: Int ..] targets])
    _ -> Left "dispatch is not cmp #bound; ldrls pc, [pc, rN, lsl #2]; b default"

-- | Memory a field lives in: a fixed base plus, for each term, a stride times an incoming argument,
-- with the terms in argument order, one per argument and no zero stride.
data Loc = Loc { locBase :: Word32, locTerms :: [(ArgumentIndex, Word32)], locOffset :: Int32, locBytes :: Int } deriving (Eq, Ord, Show)

-- | Bit range of a loaded or inserted value.
data Extract = Extract { extractLsb :: Int, extractBits :: Int, extractSigned :: Bool } deriving (Eq, Ord, Show)

data Source = FromArgument ArgumentIndex | FromField Loc Extract | FromConstant Word32 | FromOther deriving (Eq, Ord, Show)

-- | Linear holds at least one term and is never a single unit term over base 0, which is Arg.
data Val
  = Const Word32
  | Arg ArgumentIndex
  | Linear Word32 [(ArgumentIndex, Word32)]
  | Field Loc [Step]
  | Inserted Loc [Step] Source Int Int
  | DefaultRequested
  | Incoming
  | Other
  deriving (Eq, Show)

-- | A value's base and terms when it is a constant plus strides times arguments.
linearOf :: Val -> Maybe (Word32, [(ArgumentIndex, Word32)])
linearOf value = case value of
  Const c -> Just (c, [])
  Arg n -> Just (0, [(n, 1)])
  Linear base terms -> Just (base, terms)
  _ -> Nothing

-- | The canonical value of base plus the terms, strides summed mod 2^32 per argument.
linear :: Word32 -> [(ArgumentIndex, Word32)] -> Val
linear base terms = case M.toAscList (M.filter (/= 0) (M.fromListWith (+) terms)) of
  [] -> Const base
  [(n, 1)] | base == 0 -> Arg n
  summed -> Linear base summed

plus :: Val -> Val -> Val
plus left right = case (linearOf left, linearOf right) of
  (Just (leftBase, leftTerms), Just (rightBase, rightTerms)) -> linear (leftBase + rightBase) (leftTerms ++ rightTerms)
  _ -> Other

scaled :: Word32 -> Val -> Val
scaled factor value = case linearOf value of
  Just (base, terms) -> linear (factor * base) [(n, factor * stride) | (n, stride) <- terms]
  Nothing -> Other

-- | A product with a constant factor.
times :: Val -> Val -> Val
times left right = case (left, right) of
  (Const factor, _) -> scaled factor right
  (_, Const factor) -> scaled factor left
  _ -> Other

data Step = Ubfx Int Int | Sbfx Int Int | Lsr Int | Mask Word32 deriving (Eq, Show)

-- | The bit range a sequence of steps selects from a field, or Nothing for an unrecognized sequence.
extractOf :: Loc -> [Step] -> Maybe Extract
extractOf loc = go (Extract 0 (8 * locBytes loc) False)
  where
    go current [] = Just current
    go (Extract lsb bits signed) (next : rest) = case next of
      Ubfx l w | l + w <= bits -> go (Extract (lsb + l) w False) rest
      Sbfx l w | l + w <= bits -> go (Extract (lsb + l) w True) rest
      Lsr n | n < bits, not signed -> go (Extract (lsb + n) (bits - n) False) rest
      Mask m | m /= 0, m .&. (m + 1) == 0, not signed -> go (Extract lsb (min bits (popCount m)) False) rest
      Mask m | m /= 0, m .&. (m + 1) == 0, popCount m <= bits -> go (Extract lsb (popCount m) False) rest
      _ -> Nothing

-- | ReturnsIncoming is r0 unchanged from the value a shared exit was entered with.
data ArmResult = ReturnsField Loc Extract | ReturnsConstant Word32 | ReturnsArgument ArgumentIndex | ReturnsIncoming | ReturnsOther deriving (Eq, Ord, Show)

data StoreRecord = StoreRecord { storeLoc :: Loc, storeExtract :: Extract, storeSource :: Source } deriving (Eq, Ord, Show)

-- | What a forked conditional compared: an incoming argument, the setter's default-request flag
-- (r7 = 1 exactly when argument 1 is 0x1affffff), or a value the interpreter does not model.
data GuardSubject = GuardArgument ArgumentIndex | GuardDefaultRequested | GuardUnmodelled deriving (Eq, Ord, Show)

-- | Condition code of the forking instruction; the A32 codes the interpreter does not decide are kept raw.
data GuardCondition = CondEqual | CondNotEqual | CondHigherSame | CondLower | CondHigher | CondLowerSame | CondRaw Int deriving (Eq, Ord, Show)

-- | One fork taken by a path: the comparison's subject and constant, the condition, and whether it held.
-- The constant is absent when the flags did not come from a comparison with a constant.
data Guard = Guard { guardSubject :: GuardSubject, guardConstant :: Maybe Word32, guardCondition :: GuardCondition, guardHolds :: Bool } deriving (Eq, Ord, Show)

data CutReason = StepLimit | OutsideImage | Unrecognized deriving (Eq, Ord, Show)

-- | How a path ended: a return, reaching the table's shared exit with r0's value, or cut short.
data PathEnd = EndReturn ArmResult | EndExit ArmResult | EndCut CutReason deriving (Eq, Ord, Show)

-- | A store on a path: a modelled location and value, or the address of a store instruction whose
-- location the interpreter does not model.
data PathStore = Modelled StoreRecord | Unmodelled Word32 deriving (Eq, Ord, Show)

-- | The setter's shared exit run from one arm path's registers; its paths start with that arm path's guards.
data ExitRecord = ExitRecord { exitPaths :: [PathRecord], exitComplete :: Bool } deriving (Eq, Ord, Show)

-- | One feasible explored path: its forks, stores and calls in execution order, its end, and for a
-- setter path that reached the shared exit, the exit's continuation.
data PathRecord = PathRecord { pathGuards :: [Guard], pathStores :: [PathStore], pathCalls :: [Word32], pathEnd :: PathEnd, pathExit :: Maybe ExitRecord } deriving (Eq, Ord, Show)

-- | Results over every explored path, stores, called targets, and whether exploration was cut short.
-- outcomeReturns, outcomeStores and outcomeCalls include paths whose guards contradict; outcomePaths
-- holds only the feasible paths that ended, and paths dropped at the path limit leave outcomeComplete False.
data Outcome = Outcome { outcomeReturns :: [ArmResult], outcomeStores :: [StoreRecord], outcomeComplete :: Bool, outcomeCalls :: [Word32], outcomePaths :: [PathRecord] } deriving (Show)

type Regs = M.Map A.Reg Val

valueOf :: Regs -> A.Reg -> Val
valueOf regs register = M.findWithDefault Other register regs

seed :: [(Int, Val)] -> Regs
seed pairs = M.fromList [(A.Reg n, value) | (n, value) <- pairs]

-- | Getter arms run after the prologue keeps the property in r12, its index in r0 and arguments 1..3 in r1..r3.
getterArm :: Image -> PropertyTable -> Arm -> Outcome
getterArm image table (Arm (PropertyId property) target) =
  fst (explore image (tableExit table) DefaultUnlinked [] (seed [(12, Const property), (0, Const (property - 1)), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2)), (3, Arg (ArgumentIndex 3))]) target)

-- | Setter arms run after the prologue copies the property and arguments 1..3 to r4, r5, r6 and r8,
-- keeps r1 and r2, leaves the index in r3 and sets r7 to whether argument 1 is 0x1affffff. A path that
-- reaches the shared exit continues there from its own registers with r0 marked as the arm's result.
setterArm :: Image -> PropertyTable -> Arm -> Outcome
setterArm image table (Arm (PropertyId property) target) =
  let (outcome, exits) = explore image (tableExit table) DefaultLinked [] (seed [(4, Const property), (3, Const (property - 1)), (5, Arg (ArgumentIndex 1)), (6, Arg (ArgumentIndex 2)), (8, Arg (ArgumentIndex 3)), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2)), (7, DefaultRequested)]) target
      continued (path, regs) = case (tableExit table, regs) of
        (Just exit, Just atExit) ->
          let (exitOutcome, _) = explore image Nothing DefaultLinked (pathGuards path) (M.insert (A.Reg 0) Incoming atExit) exit
          in path { pathExit = Just (ExitRecord (outcomePaths exitOutcome) (outcomeComplete exitOutcome)) }
        _ -> path
  in outcome { outcomePaths = map continued exits }

-- | The getter code at `address` run from the getter arm registers with r0 marked incoming.
getterContinuation :: Image -> PropertyId -> Word32 -> Outcome
getterContinuation image (PropertyId property) address =
  fst (explore image Nothing DefaultUnlinked [] (seed [(12, Const property), (0, Incoming), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2)), (3, Arg (ArgumentIndex 3))]) address)

-- | The setter code at `address` run from the setter arm registers with r0 marked incoming.
setterContinuation :: Image -> PropertyId -> Word32 -> Outcome
setterContinuation image (PropertyId property) address =
  fst (explore image Nothing DefaultLinked [] (seed [(4, Const property), (0, Incoming), (3, Const (property - 1)), (5, Arg (ArgumentIndex 1)), (6, Arg (ArgumentIndex 2)), (8, Arg (ArgumentIndex 3)), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2)), (7, DefaultRequested)]) address)

maxSteps, maxPaths :: Int
maxSteps = 48
maxPaths = 64

data Condition = Always | Decides GuardCondition deriving (Eq)

condition :: A.Cond -> Condition
condition (A.Cond code) = case code of
  0 -> Decides CondEqual
  1 -> Decides CondNotEqual
  2 -> Decides CondHigherSame
  3 -> Decides CondLower
  8 -> Decides CondHigher
  9 -> Decides CondLowerSame
  14 -> Always
  _ -> Decides (CondRaw (fromIntegral code))

-- | The last comparison of a value with a constant, and whether equality holds on this path once assumed.
data Flags = Flags Val Word32 (Maybe Bool) | NoFlags

-- | A condition the path's comparison decides: from two constants, or from an earlier equality assumption.
decided :: Flags -> Condition -> Maybe Bool
decided flags wanted = case (flags, wanted) of
  (Flags (Const value) constant _, Decides k) -> case k of
    CondEqual -> Just (value == constant)
    CondNotEqual -> Just (value /= constant)
    CondHigherSame -> Just (value >= constant)
    CondLower -> Just (value < constant)
    CondHigher -> Just (value > constant)
    CondLowerSame -> Just (value <= constant)
    CondRaw _ -> Nothing
  (Flags _ _ (Just equal), Decides CondEqual) -> Just equal
  (Flags _ _ (Just equal), Decides CondNotEqual) -> Just (not equal)
  _ -> Nothing

assume :: Flags -> Condition -> Bool -> Flags
assume flags wanted holds = case (flags, wanted) of
  (Flags value constant Nothing, Decides CondEqual) -> Flags value constant (Just holds)
  (Flags value constant Nothing, Decides CondNotEqual) -> Flags value constant (Just (not holds))
  _ -> flags

conditionOf :: Word32 -> A.Instruction -> A.Cond
conditionOf raw instruction = case instruction of
  A.Branch c _ _ -> c
  A.Load c _ _ _ _ _ _ -> c
  A.Store c _ _ _ _ _ -> c
  A.LoadMultiple c _ _ _ _ -> c
  A.StoreMultiple c _ _ _ _ -> c
  A.DataProcessing c _ _ _ _ _ -> c
  A.MoveWide c _ _ -> c
  A.MoveTop c _ _ -> c
  A.Multiply c _ _ _ _ -> c
  A.BranchExchange c _ _ -> c
  A.ZeroExtend c _ _ _ _ -> c
  A.Compute c _ -> c
  _ -> A.Cond (raw `shiftR` 28)

-- | Flags after an instruction that sets them, or Nothing when it leaves them.
flagsAfter :: Regs -> A.Instruction -> Maybe Flags
flagsAfter regs instruction = case instruction of
  A.DataProcessing _ A.CMP _ _ source (A.Immediate constant) -> Just (Flags (valueOf regs source) constant Nothing)
  A.DataProcessing _ A.CMP _ _ source (A.ShiftedImmediate operand A.LSL 0)
    | Const constant <- valueOf regs operand -> Just (Flags (valueOf regs source) constant Nothing)
  A.DataProcessing _ op _ _ _ _ | op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] -> Just NoFlags
  A.DataProcessing _ _ True _ _ _ -> Just NoFlags
  _ -> Nothing

-- | A path's forks, stores and calls, most recent first.
data Trace = Trace [Guard] [PathStore] [Word32]

data Path = Path Word32 Regs Flags Int Trace

data Effect = Continue Word32 Regs | Return ArmResult | StoreThen StoreRecord Word32 Regs | UnmodelledStoreThen Word32 Regs | CallThen Word32 Word32 Regs | Abandon

guardOf :: Flags -> GuardCondition -> Bool -> Guard
guardOf flags k = case flags of
  Flags value constant _ -> Guard (subject value) (Just constant) k
  NoFlags -> Guard GuardUnmodelled Nothing k
  where
    subject value = case value of
      Arg n -> GuardArgument n
      DefaultRequested -> GuardDefaultRequested
      _ -> GuardUnmodelled

-- | Whether the default-request flag is the setter's r7, so that argument 1 equal to 0x1affffff is
-- the flag equal to 1.
data DefaultLink = DefaultUnlinked | DefaultLinked

-- | Whether a path's equality assumptions can all hold: no subject both equal and unequal to one
-- constant, no argument equal to two constants, and the default-request flag, which is 0 or 1, left a value.
feasibleGuards :: DefaultLink -> [Guard] -> Bool
feasibleGuards link guards = all possible subjects
  where
    equalities = concatMap linked [(s, c, if k == CondEqual then holds else not holds) | Guard s (Just c) k holds <- guards, k `elem` [CondEqual, CondNotEqual], s /= GuardUnmodelled]
    linked equality = case (link, equality) of
      (DefaultLinked, (GuardArgument (ArgumentIndex 1), 0x1affffff, truth)) -> [(GuardDefaultRequested, 1, truth)]
      (DefaultLinked, (GuardArgument (ArgumentIndex 1), _, True)) -> [equality, (GuardDefaultRequested, 0, True)]
      _ -> [equality]
    subjects = nub [s | (s, _, _) <- equalities]
    possible s =
      let equal = nub [c | (s', c, True) <- equalities, s' == s]
          unequal = nub [c | (s', c, False) <- equalities, s' == s]
          domain = if s == GuardDefaultRequested then Just [0, 1] else Nothing
      in case (equal, domain) of
        (_ : _ : _, _) -> False
        ([c], Just values) -> c `elem` values && c `notElem` unequal
        ([c], Nothing) -> c `notElem` unequal
        ([], Just values) -> any (`notElem` unequal) values
        ([], Nothing) -> True

-- | The outcome, with each feasible ended path and, for paths that reached the exit, their registers there.
explore :: Image -> Maybe Word32 -> DefaultLink -> [Guard] -> Regs -> Word32 -> (Outcome, [(PathRecord, Maybe Regs)])
explore image exit link prior initial start = go [Path start initial NoFlags 0 (Trace (reverse prior) [] [])] 1 [] [] True [] []
  where
    record (Trace guards stores calls) end atExit
      | feasibleGuards link guards = [(PathRecord (reverse guards) (reverse stores) (reverse calls) end Nothing, atExit)]
      | otherwise = []
    go [] _ returns stores complete calls paths =
      let ended = reverse paths
      in (Outcome (nub returns) (nub stores) complete (nub calls) (map fst ended), ended)
    go (Path address regs flags steps trace : pending) started returns stores complete calls paths
      | Just address == exit = go pending started returns stores complete calls (record trace (EndExit (resultOf (valueOf regs (A.Reg 0)))) (Just regs) ++ paths)
      | steps >= maxSteps = go pending started (ReturnsOther : returns) stores False calls (record trace (EndCut StepLimit) Nothing ++ paths)
      | otherwise = case imageWord image address of
          Nothing -> go pending started (ReturnsOther : returns) stores False calls (record trace (EndCut OutsideImage) Nothing ++ paths)
          Just raw ->
            let instruction = A.decode address raw
                wanted = condition (conditionOf raw instruction)
                executed = case flagsAfter regs instruction of
                  Just after -> after
                  Nothing -> if wanted == Always then flags else assume flags wanted True
                taken = (step image raw instruction address regs, executed, trace)
                skipped = (Continue (address + 4) regs, assume flags wanted False, trace)
                forked k holds (effect, flags', Trace guards s c) = (effect, flags', Trace (guardOf flags k holds : guards) s c)
                forks = case wanted of
                  Always -> [taken]
                  Decides k -> case decided flags wanted of
                    Just True -> [taken]
                    Just False -> [skipped]
                    Nothing -> [forked k True taken, forked k False skipped]
                started' = started + length forks - 1
                collect (newPending, newReturns, newStores, newCalls, whole, newPaths) (effect, flags', t@(Trace guards s c)) = case effect of
                  Continue next regs' -> (Path next regs' flags' (steps + 1) t : newPending, newReturns, newStores, newCalls, whole, newPaths)
                  Return result -> (newPending, result : newReturns, newStores, newCalls, whole, record t (EndReturn result) Nothing ++ newPaths)
                  StoreThen stored next regs' -> (Path next regs' flags' (steps + 1) (Trace guards (Modelled stored : s) c) : newPending, newReturns, stored : newStores, newCalls, whole, newPaths)
                  UnmodelledStoreThen next regs' -> (Path next regs' flags' (steps + 1) (Trace guards (Unmodelled address : s) c) : newPending, newReturns, newStores, newCalls, whole, newPaths)
                  CallThen target next regs' -> (Path next regs' NoFlags (steps + 1) (Trace guards s (target : c)) : newPending, newReturns, newStores, target : newCalls, whole, newPaths)
                  Abandon -> (newPending, ReturnsOther : newReturns, newStores, newCalls, False, record t (EndCut Unrecognized) Nothing ++ newPaths)
                (morePending, moreReturns, moreStores, moreCalls, stillComplete, morePaths) = foldl collect ([], [], [], [], True, []) forks
            in if started' > maxPaths
                 then go [] started' returns stores False calls paths
                 else go (morePending ++ pending) started' (moreReturns ++ returns) (moreStores ++ stores) (complete && stillComplete) (moreCalls ++ calls) (morePaths ++ paths)

resultOf :: Val -> ArmResult
resultOf value = case value of
  Const c -> ReturnsConstant c
  Arg n -> ReturnsArgument n
  Incoming -> ReturnsIncoming
  Field loc steps | Just e <- extractOf loc steps -> ReturnsField loc e
  _ -> ReturnsOther

sourceOf :: Val -> Source
sourceOf value = case value of
  Arg n -> FromArgument n
  Const c -> FromConstant c
  Field loc steps | Just e <- extractOf loc steps -> FromField loc e
  _ -> FromOther

-- | Signed form, destination, source, lsb and width.
data BitExtract = BitExtract Bool A.Reg A.Reg Int Int

-- | UBFX or SBFX, which A32.decode leaves as Compute.
bitExtract :: Word32 -> Maybe BitExtract
bitExtract raw
  | raw .&. 0x0fe00070 == 0x07e00050 = Just (form False)
  | raw .&. 0x0fe00070 == 0x07a00050 = Just (form True)
  | otherwise = Nothing
  where
    form signed = BitExtract signed (A.Reg (fromIntegral ((raw `shiftR` 12) .&. 15))) (A.Reg (fromIntegral (raw .&. 15))) (fromIntegral ((raw `shiftR` 7) .&. 31)) (fromIntegral ((raw `shiftR` 16) .&. 31) + 1)

-- | `[rN, #imm]`, or `[rN, rM, lsl #k]` at offset 0, over linear values.
memory :: Regs -> A.Reg -> A.Offset -> A.Width -> Maybe Loc
memory regs base offset width = case offset of
  A.OffsetImmediate o | base /= A.pc -> located (valueOf regs base) o
  A.OffsetRegister True index A.LSL k | base /= A.pc, index /= A.pc -> located (plus (valueOf regs base) (scaled (1 `shiftL` k) (valueOf regs index))) 0
  _ -> Nothing
  where
    located value o = (\(b, terms) -> Loc b terms o bytes) <$> linearOf value
    bytes = case width of
      A.Byte -> 1
      A.Half -> 2
      A.Word -> 4
      A.Dual -> 8

step :: Image -> Word32 -> A.Instruction -> Word32 -> Regs -> Effect
step image raw instruction address regs
  | Just (BitExtract signed destination source lsb width) <- bitExtract raw =
      let value = case valueOf regs source of
            Field loc steps -> Field loc (steps ++ [if signed then Sbfx lsb width else Ubfx lsb width])
            _ -> Other
      in Continue next (M.insert destination value regs)
  | Just (A.BitFieldInsert _ destination source lsb width) <- A.bitFieldInsert raw =
      let inserted = maybe (FromConstant 0) (sourceOf . valueOf regs) source
          value = case valueOf regs destination of
            Field loc steps -> Inserted loc steps inserted lsb width
            _ -> Other
      in Continue next (M.insert destination value regs)
  | otherwise = case instruction of
      A.MoveWide _ destination value -> Continue next (M.insert destination (Const value) regs)
      A.MoveTop _ destination high -> case valueOf regs destination of
        Const low -> Continue next (M.insert destination (Const ((high `shiftL` 16) + (low .&. 0xffff))) regs)
        _ -> Continue next (M.insert destination Other regs)
      A.DataProcessing _ A.MOV _ destination _ (A.Immediate value) -> Continue next (M.insert destination (Const value) regs)
      A.DataProcessing _ A.MVN _ destination _ (A.Immediate value) -> Continue next (M.insert destination (Const (0xffffffff - value)) regs)
      A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) -> Continue next (M.insert destination (valueOf regs source) regs)
      A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate source A.LSR n) ->
        Continue next (M.insert destination (shifted (valueOf regs source) n) regs)
      A.DataProcessing _ A.AND _ destination source (A.Immediate mask) ->
        Continue next (M.insert destination (masked (valueOf regs source) mask) regs)
      A.DataProcessing _ A.ADD _ destination source (A.Immediate value) | source /= A.pc ->
        Continue next (M.insert destination (plus (valueOf regs source) (Const value)) regs)
      A.DataProcessing _ A.SUB _ destination source (A.Immediate value) | source /= A.pc ->
        Continue next (M.insert destination (plus (valueOf regs source) (Const (negate value))) regs)
      A.DataProcessing _ A.ADD _ destination source (A.ShiftedImmediate operand A.LSL k) | source /= A.pc, operand /= A.pc ->
        Continue next (M.insert destination (plus (valueOf regs source) (scaled (1 `shiftL` k) (valueOf regs operand))) regs)
      A.DataProcessing _ op _ _ _ _ | op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] -> Continue next regs
      A.DataProcessing _ _ _ destination _ _ -> Continue next (M.insert destination Other regs)
      A.ZeroExtend _ destination source rotation bits ->
        let rotated
              | rotation == 0 = valueOf regs source
              | rotation + bits <= 32 = shifted (valueOf regs source) rotation
              | otherwise = Other
        in Continue next (M.insert destination (masked rotated ((1 `shiftL` bits) - 1)) regs)
      A.Multiply _ destination left right accumulator ->
        Continue next (M.insert destination (maybe id (plus . valueOf regs) accumulator (times (valueOf regs left) (valueOf regs right))) regs)
      A.Load _ A.Word False destination base (A.OffsetImmediate o) A.OffsetOnly
        | base == A.pc, destination /= A.pc -> Continue next (M.insert destination (maybe Other Const (imageWord image (address + 8 + fromIntegral o))) regs)
      A.Load _ width False destination base offset A.OffsetOnly
        | Just loc <- memory regs base offset width -> Continue next (M.insert destination (Field loc []) regs)
      A.Load _ _ _ destination _ _ _ -> if destination == A.pc then Abandon else Continue next (M.insert destination Other regs)
      A.Store _ width source base offset A.OffsetOnly
        | Just loc <- memory regs base offset width -> StoreThen (storeRecord loc (valueOf regs source)) next regs
      A.Store {} -> UnmodelledStoreThen next regs
      A.Compute _ written -> Continue next (foldr (\r -> M.insert r Other) regs written)
      A.LoadMultiple _ base _ _ registers
        | A.pc `elem` registers && base == A.sp -> Return (resultOf (valueOf regs (A.Reg 0)))
        | otherwise -> Continue next (foldr (\r -> M.insert r Other) regs registers)
      A.StoreMultiple {} -> UnmodelledStoreThen next regs
      A.BranchExchange _ False register | register == A.lr -> Return (resultOf (valueOf regs (A.Reg 0)))
      A.Branch _ True target -> CallThen target next (foldr (\r -> M.insert (A.Reg r) Other) regs [0, 1, 2, 3, 12])
      A.Branch _ False target -> Continue target regs
      A.Hint _ -> Continue next regs
      _ -> Abandon
  where
    next = address + 4
    shifted value n = case value of
      Field loc steps -> Field loc (steps ++ [Lsr n])
      Const c -> Const (c `shiftR` n)
      _ -> Other
    masked value mask = case value of
      Field loc steps -> Field loc (steps ++ [Mask mask])
      Const c -> Const (c .&. mask)
      _ -> Other
    storeRecord loc value = case value of
      Inserted original steps source lsb width | original == loc, null steps -> StoreRecord loc (Extract lsb width False) source
      _ -> StoreRecord loc (Extract 0 (8 * locBytes loc) False) (sourceOf value)
