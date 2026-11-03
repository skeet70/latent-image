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
  , Arm (..)
  , tableArms
  , getterArm
  , setterArm
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

-- | Memory a field lives in: a fixed base, optionally plus a stride times an incoming argument.
data Loc = Loc { locBase :: Word32, locIndex :: Maybe (ArgumentIndex, Word32), locOffset :: Int32, locBytes :: Int } deriving (Eq, Ord, Show)

-- | Bit range of a loaded or inserted value.
data Extract = Extract { extractLsb :: Int, extractBits :: Int, extractSigned :: Bool } deriving (Eq, Ord, Show)

data Source = FromArgument ArgumentIndex | FromField Loc Extract | FromConstant Word32 | FromOther deriving (Eq, Ord, Show)

data Val
  = Const Word32
  | Arg ArgumentIndex
  | Indexed Word32 ArgumentIndex Word32
  | Field Loc [Step]
  | Inserted Loc [Step] Source Int Int
  | Other
  deriving (Eq, Show)

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

data ArmResult = ReturnsField Loc Extract | ReturnsConstant Word32 | ReturnsArgument ArgumentIndex | ReturnsOther deriving (Eq, Ord, Show)

data StoreRecord = StoreRecord { storeLoc :: Loc, storeExtract :: Extract, storeSource :: Source } deriving (Eq, Ord, Show)

-- | Results over every explored path, stores, called targets, and whether exploration was cut short.
data Outcome = Outcome { outcomeReturns :: [ArmResult], outcomeStores :: [StoreRecord], outcomeComplete :: Bool, outcomeCalls :: [Word32] } deriving (Show)

type Regs = M.Map A.Reg Val

valueOf :: Regs -> A.Reg -> Val
valueOf regs register = M.findWithDefault Other register regs

seed :: [(Int, Val)] -> Regs
seed pairs = M.fromList [(A.Reg n, value) | (n, value) <- pairs]

-- | Getter arms run after the prologue keeps the property in r12, its index in r0 and arguments 1..3 in r1..r3.
getterArm :: Image -> PropertyTable -> Arm -> Outcome
getterArm image table (Arm (PropertyId property) target) =
  explore image (tableExit table) (seed [(12, Const property), (0, Const (property - 1)), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2)), (3, Arg (ArgumentIndex 3))]) target

-- | Setter arms run after the prologue copies the property and arguments 1..3 to r4, r5, r6 and r8,
-- keeps r1 and r2, and leaves the index in r3.
setterArm :: Image -> PropertyTable -> Arm -> Outcome
setterArm image table (Arm (PropertyId property) target) =
  explore image (tableExit table) (seed [(4, Const property), (3, Const (property - 1)), (5, Arg (ArgumentIndex 1)), (6, Arg (ArgumentIndex 2)), (8, Arg (ArgumentIndex 3)), (1, Arg (ArgumentIndex 1)), (2, Arg (ArgumentIndex 2))]) target

maxSteps, maxPaths :: Int
maxSteps = 48
maxPaths = 64

data Condition = Always | Equal | NotEqual | HigherSame | Lower | Higher | LowerSame | Undecided deriving (Eq)

condition :: A.Cond -> Condition
condition (A.Cond code) = case code of
  0 -> Equal
  1 -> NotEqual
  2 -> HigherSame
  3 -> Lower
  8 -> Higher
  9 -> LowerSame
  14 -> Always
  _ -> Undecided

-- | The last comparison of a value with a constant, and whether equality holds on this path once assumed.
data Flags = Flags Val Word32 (Maybe Bool) | NoFlags

-- | A condition the path's comparison decides: from two constants, or from an earlier equality assumption.
decided :: Flags -> Condition -> Maybe Bool
decided flags wanted = case (flags, wanted) of
  (Flags (Const value) constant _, _) -> case wanted of
    Equal -> Just (value == constant)
    NotEqual -> Just (value /= constant)
    HigherSame -> Just (value >= constant)
    Lower -> Just (value < constant)
    Higher -> Just (value > constant)
    LowerSame -> Just (value <= constant)
    _ -> Nothing
  (Flags _ _ (Just equal), Equal) -> Just equal
  (Flags _ _ (Just equal), NotEqual) -> Just (not equal)
  _ -> Nothing

assume :: Flags -> Condition -> Bool -> Flags
assume flags wanted holds = case (flags, wanted) of
  (Flags value constant Nothing, Equal) -> Flags value constant (Just holds)
  (Flags value constant Nothing, NotEqual) -> Flags value constant (Just (not holds))
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

data Path = Path Word32 Regs Flags Int

data Effect = Continue Word32 Regs | Return ArmResult | StoreThen StoreRecord Word32 Regs | CallThen Word32 Word32 Regs | Abandon

explore :: Image -> Maybe Word32 -> Regs -> Word32 -> Outcome
explore image exit initial start = go [Path start initial NoFlags 0] 1 [] [] True []
  where
    go [] _ returns stores complete calls = Outcome (nub returns) (nub stores) complete (nub calls)
    go (Path address regs flags steps : pending) started returns stores complete calls
      | Just address == exit = go pending started returns stores complete calls
      | steps >= maxSteps = go pending started (ReturnsOther : returns) stores False calls
      | otherwise = case imageWord image address of
          Nothing -> go pending started (ReturnsOther : returns) stores False calls
          Just raw ->
            let instruction = A.decode address raw
                wanted = condition (conditionOf raw instruction)
                executed = case flagsAfter regs instruction of
                  Just after -> after
                  Nothing -> if wanted == Always then flags else assume flags wanted True
                taken = (step raw instruction address regs, executed)
                skipped = (Continue (address + 4) regs, assume flags wanted False)
                forks
                  | wanted == Always = [taken]
                  | otherwise = case decided flags wanted of
                      Just True -> [taken]
                      Just False -> [skipped]
                      Nothing -> [taken, skipped]
                started' = started + length forks - 1
                collect (newPending, newReturns, newStores, newCalls, whole) (effect, flags') = case effect of
                  Continue next regs' -> (Path next regs' flags' (steps + 1) : newPending, newReturns, newStores, newCalls, whole)
                  Return result -> (newPending, result : newReturns, newStores, newCalls, whole)
                  StoreThen record next regs' -> (Path next regs' flags' (steps + 1) : newPending, newReturns, record : newStores, newCalls, whole)
                  CallThen target next regs' -> (Path next regs' NoFlags (steps + 1) : newPending, newReturns, newStores, target : newCalls, whole)
                  Abandon -> (newPending, ReturnsOther : newReturns, newStores, newCalls, False)
                (morePending, moreReturns, moreStores, moreCalls, stillComplete) = foldl collect ([], [], [], [], True) forks
            in if started' > maxPaths
                 then go [] started' returns stores False calls
                 else go (morePending ++ pending) started' (moreReturns ++ returns) (moreStores ++ stores) (complete && stillComplete) (moreCalls ++ calls)

resultOf :: Val -> ArmResult
resultOf value = case value of
  Const c -> ReturnsConstant c
  Arg n -> ReturnsArgument n
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

memory :: Regs -> A.Reg -> A.Offset -> A.Width -> Maybe Loc
memory regs base offset width = case (valueOf regs base, offset) of
  (Const c, A.OffsetImmediate o) -> Just (Loc c Nothing o bytes)
  (Indexed c n stride, A.OffsetImmediate o) -> Just (Loc c (Just (n, stride)) o bytes)
  _ -> Nothing
  where
    bytes = case width of
      A.Byte -> 1
      A.Half -> 2
      A.Word -> 4
      A.Dual -> 8

step :: Word32 -> A.Instruction -> Word32 -> Regs -> Effect
step raw instruction address regs
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
      A.DataProcessing _ op _ _ _ _ | op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] -> Continue next regs
      A.DataProcessing _ _ _ destination _ _ -> Continue next (M.insert destination Other regs)
      A.ZeroExtend _ destination source rotation bits ->
        let rotated
              | rotation == 0 = valueOf regs source
              | rotation + bits <= 32 = shifted (valueOf regs source) rotation
              | otherwise = Other
        in Continue next (M.insert destination (masked rotated ((1 `shiftL` bits) - 1)) regs)
      A.Multiply _ destination left right (Just accumulator) -> Continue next (M.insert destination (indexed (valueOf regs left) (valueOf regs right) (valueOf regs accumulator)) regs)
      A.Multiply _ destination _ _ Nothing -> Continue next (M.insert destination Other regs)
      A.Load _ width False destination base offset A.OffsetOnly
        | base /= A.pc, Just loc <- memory regs base offset width -> Continue next (M.insert destination (Field loc []) regs)
      A.Load _ _ _ destination _ _ _ -> if destination == A.pc then Abandon else Continue next (M.insert destination Other regs)
      A.Store _ width source base offset A.OffsetOnly
        | Just loc <- memory regs base offset width -> StoreThen (storeRecord loc (valueOf regs source)) next regs
      A.Store {} -> Continue next regs
      A.Compute _ written -> Continue next (foldr (\r -> M.insert r Other) regs written)
      A.LoadMultiple _ base _ _ registers
        | A.pc `elem` registers && base == A.sp -> Return (resultOf (valueOf regs (A.Reg 0)))
        | otherwise -> Continue next (foldr (\r -> M.insert r Other) regs registers)
      A.StoreMultiple {} -> Continue next regs
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
    indexed left right accumulator = case (left, right, accumulator) of
      (Const stride, Arg n, Const base) -> Indexed base n stride
      (Arg n, Const stride, Const base) -> Indexed base n stride
      _ -> Other
    storeRecord loc value = case value of
      Inserted original steps source lsb width | original == loc, null steps -> StoreRecord loc (Extract lsb width False) source
      _ -> StoreRecord loc (Extract 0 (8 * locBytes loc) False) (sourceOf value)
