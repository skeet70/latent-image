{-# LANGUAGE GHC2021 #-}

module ReturnProvenance
  ( GuestAddress (..)
  , SavedStackArray (..)
  , ProgramCounter (..)
  , Step (..)
  , CodeNode (..)
  , LiteralWord (..)
  , PremiseUse (..)
  , ReturnPremise (..)
  , requiredReturnPremises
  , returnPremiseLabel
  , verifyReturns
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits (rotateR, shiftL, shiftR, (.&.), (.|.))
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import Data.Maybe (isNothing)
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (showHex)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
-- | `savedStackCount` records of `savedStackStride` bytes from `savedStackStart`,
-- each holding a saved stack pointer at `savedStackField`.
data SavedStackArray = SavedStackArray
  { savedStackStart :: GuestAddress
  , savedStackStride :: Word32
  , savedStackCount :: Word32
  , savedStackField :: Word32
  } deriving (Eq, Show)
newtype ProgramCounter = ProgramCounter Word32 deriving (Eq, Ord, Show)
newtype FrameOffset = FrameOffset Int32 deriving (Eq, Ord, Show)
-- | WritesStatus carries an MSR that `A.decode` rejects as SystemRegister;
-- SignExtends carries an SXTB or SXTH that `A.decode` returns as Compute.
data Step = Executes A.Instruction | WritesStatus A.StatusWrite | SignExtends A.SignExtension
data CodeNode = CodeNode ProgramCounter Step [ProgramCounter] A.Cond
newtype LiteralWord = LiteralWord Word32 deriving (Eq, Show)
data PremiseUse = PremiseUse ProgramCounter ReturnPremise deriving (Eq, Ord, Show)

-- Callees write exposed local objects, not saved-register slots; unknown incoming
-- and callee-returned pointers do not alias unexposed frame words.
-- Absolute and indexed global-object addresses stay inside their objects and
-- do not alias allocated task or boot stacks.
-- A word loaded through an address not derived from the frame, while the frame
-- is unexposed, does not address its unexposed words. Loads that may read a
-- declared saved stack pointer give unknown instead; a loaded pointer is taken
-- to address the start of its record and a register index to select whole
-- records, so a pointer into a record's interior defeats that exclusion. An
-- indexed global's constant base plus its constant displacement lies in its
-- array or at most one record before it.
data ReturnPremise = UnexposedFrameIsolation | GlobalObjectStackSeparation | LoadedObjectFrameIsolation deriving (Eq, Ord, Show)

requiredReturnPremises :: [ReturnPremise]
requiredReturnPremises = [UnexposedFrameIsolation, GlobalObjectStackSeparation, LoadedObjectFrameIsolation]

returnPremiseLabel :: ReturnPremise -> String
returnPremiseLabel UnexposedFrameIsolation = "unexposed-caller-frame-isolation"
returnPremiseLabel GlobalObjectStackSeparation = "global-object-stack-separation"
returnPremiseLabel LoadedObjectFrameIsolation = "loaded-object-frame-isolation"

-- StatusWord: CPSR bits `executionState` equal the current CPSR's.
-- StatusNeutral: bits `executionState` are zero.
-- LoadedObject: a word loaded from outside the frame while it was private, with
-- its constant displacement (if known) and the bases of global objects it was
-- joined with.
-- GlobalObject: the possible constant bases of an absolute or indexed global.
data Value = IncomingReturn | FrameAddress FrameOffset | FrameDerived | ReturnedObject | GlobalObject (S.Set Word32) | ScaledIndex | Constant Word32
  | StatusWord | StatusNeutral | LoadedObject (Maybe Int32) (S.Set Word32) deriving (Eq, Show)
data FrameExposure = PrivateFrame | ExposedFrame deriving (Eq, Ord, Show)
data Accumulation = ProductOnly | Addend (Maybe Value)
data FrameState = FrameState
  { registers :: M.Map A.Reg Value
  , frameWords :: M.Map FrameOffset Value
  , frameExposure :: FrameExposure
  , savedRegisters :: S.Set FrameOffset
  , instructionPremises :: S.Set ReturnPremise
  } deriving (Eq, Show)

verifyReturns :: [SavedStackArray] -> M.Map ProgramCounter LiteralWord -> ProgramCounter -> [CodeNode] -> Either String [PremiseUse]
verifyReturns arrays literals entry nodes = walk [(entry, initial)] M.empty S.empty
  where
    graph = M.fromList [(address, node) | node@(CodeNode address _ _ _) <- nodes]
    initial = FrameState (M.fromList [(A.sp, FrameAddress (FrameOffset 0)), (A.lr, IncomingReturn)]) M.empty PrivateFrame S.empty S.empty
    walk [] _ dependencies = Right (S.toList dependencies)
    walk ((address, incoming):pending) states dependencies = do
      let joined = maybe incoming (`joinState` incoming) (M.lookup address states)
      if M.lookup address states == Just joined
        then walk pending states dependencies
        else do
          CodeNode _ step successors condition <- maybe (Left "return proof leaves the classified code") Right (M.lookup address graph)
          executed <- case executeStep arrays literals address step joined of
            Left problem -> Left (problem ++ "; premise uses: " ++ show
              [showHex at "" ++ "=" ++ returnPremiseLabel premise | PremiseUse (ProgramCounter at) premise <- S.toList dependencies])
            Right result -> Right result
          let nextDependencies = S.union dependencies (S.map (PremiseUse address) (instructionPremises executed))
              propagated = executed {instructionPremises = S.empty}
              actual = [(next, propagated) | next <- successors, not (stepReturns step)]
              skipped = case (address, condition) of
                (ProgramCounter pc, A.Cond c) -> [(ProgramCounter (pc+4), joined) | c /= 14]
          walk (actual ++ skipped ++ pending) (M.insert address joined states) nextDependencies

joinState :: FrameState -> FrameState -> FrameState
joinState left right = FrameState
  (common (registers left) (registers right))
  (common (frameWords left) (frameWords right))
  (max (frameExposure left) (frameExposure right))
  (S.intersection (savedRegisters left) (savedRegisters right))
  S.empty
  where
    common leftMap rightMap = M.mergeWithKey
      (\_ a b -> case (a, b) of
        (LoadedObject x bases, LoadedObject y others) -> Just (LoadedObject (if x == y then x else Nothing) (S.union bases others))
        (LoadedObject x bases, GlobalObject others) -> Just (LoadedObject x (S.union bases others))
        (GlobalObject bases, LoadedObject y others) -> Just (LoadedObject y (S.union bases others))
        (GlobalObject bases, GlobalObject others) -> Just (GlobalObject (S.union bases others))
        _ | a == b -> Just a
        _ -> derivedFrom [Just a, Just b])
      (M.mapMaybe (derivedFrom . (:[]) . Just))
      (M.mapMaybe (derivedFrom . (:[]) . Just))
      leftMap rightMap

derivedFrom :: [Maybe Value] -> Maybe Value
derivedFrom values
  | any isFrameValue values = Just FrameDerived
  | otherwise = Nothing

isFrameValue :: Maybe Value -> Bool
isFrameValue (Just (FrameAddress _)) = True
isFrameValue (Just FrameDerived) = True
isFrameValue _ = False

exposeFrame :: Maybe Value -> FrameState -> FrameState
exposeFrame value state
  | isFrameValue value = state {frameExposure = ExposedFrame}
  | otherwise = state

usePremise :: ReturnPremise -> FrameState -> FrameState
usePremise premise state = state {instructionPremises = S.insert premise (instructionPremises state)}

savedWords :: FrameState -> M.Map FrameOffset Value
savedWords state = M.restrictKeys (frameWords state) (savedRegisters state)

returns :: A.Instruction -> Bool
returns instruction = case instruction of
  A.BranchExchange _ False register -> register == A.lr
  A.LoadMultiple _ _ _ _ listed -> A.pc `elem` listed
  A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) -> destination == A.pc && source == A.lr
  _ -> False

stepReturns :: Step -> Bool
stepReturns (Executes instruction) = returns instruction
stepReturns _ = False

setRegister :: A.Reg -> Maybe Value -> FrameState -> FrameState
setRegister register value state = state
  { registers = maybe (M.delete register) (M.insert register) value (registers state)
  , savedRegisters = if register /= A.sp then savedRegisters state else case value of
      Just (FrameAddress boundary) -> S.filter (>= boundary) (savedRegisters state)
      _ -> S.empty
  }

add :: Maybe Value -> Int32 -> Maybe Value
add (Just (FrameAddress (FrameOffset offset))) delta = Just (FrameAddress (FrameOffset (offset+delta)))
add (Just (Constant value)) delta = Just (Constant (value + fromIntegral delta))
add (Just ReturnedObject) _ = Just ReturnedObject
add (Just (LoadedObject displacement bases)) delta = Just (LoadedObject ((+ delta) <$> displacement) (S.map (+ fromIntegral delta) bases))
add (Just FrameDerived) _ = Just FrameDerived
add (Just (GlobalObject bases)) delta = Just (GlobalObject (S.map (+ fromIntegral delta) bases))
add (Just ScaledIndex) delta = Just (if delta == 0 then ScaledIndex else GlobalObject (S.singleton (fromIntegral delta)))
add value 0 = value
add _ _ = Nothing

indexedAddress :: FrameExposure -> Maybe Value -> Maybe Value -> Maybe Value
indexedAddress exposure base index = case derivedFrom [base, index] of
  Just FrameDerived -> Just FrameDerived
  _ -> case base of
    -- After exposure an unknown operand can be a reloaded frame address.
    Just (Constant base') | exposure == PrivateFrame -> Just (GlobalObject (S.singleton base'))
    Just (GlobalObject bases) | exposure == PrivateFrame -> Just (GlobalObject bases)
    Just ReturnedObject -> Just ReturnedObject
    Just loaded@(LoadedObject _ _) | exposure == PrivateFrame -> Just loaded
    _ -> Nothing

multiplyValue :: FrameExposure -> Maybe Value -> Maybe Value -> Accumulation -> Maybe Value
multiplyValue exposure left right accumulator = case derivedFrom [left, right, case accumulator of ProductOnly -> Nothing; Addend value -> value] of
  Just FrameDerived -> Just FrameDerived
  _ -> case (left, right, accumulator) of
    (Just (Constant a), Just (Constant b), Addend (Just (Constant c))) -> Just (Constant (a*b+c))
    (Just (Constant a), Just (Constant b), ProductOnly) -> Just (Constant (a*b))
    -- After exposure an unknown operand can be a reloaded frame address.
    _ | exposure == ExposedFrame -> Nothing
    (_, _, Addend (Just (Constant base))) -> Just (GlobalObject (S.singleton base))
    (_, _, Addend (Just (GlobalObject bases))) -> Just (GlobalObject bases)
    (Just (Constant _), _, _) -> Just ScaledIndex
    (_, Just (Constant _), _) -> Just ScaledIndex
    _ -> Nothing

-- CPSR mode M[4:0], T [5] and E [9] bits.
executionState :: Word32
executionState = 0x23f

statusNeutral :: Maybe Value -> Bool
statusNeutral (Just StatusNeutral) = True
statusNeutral (Just (Constant value)) = value .&. executionState == 0
statusNeutral _ = False

coversExecutionState :: Maybe Value -> Bool
coversExecutionState (Just (Constant mask)) = mask .&. executionState == executionState
coversExecutionState _ = False

-- AND, EOR, ORR and BIC; other operations give derivedFrom.
bitwiseValue :: A.DataOp -> Maybe Value -> Maybe Value -> Maybe Value
bitwiseValue operation left right = case derivedFrom [left, right] of
  Just FrameDerived -> Just FrameDerived
  unknown -> case (operation, left, right) of
    (A.AND, _, _) | statusNeutral left || statusNeutral right -> Just StatusNeutral
    (A.AND, Just StatusWord, _) | coversExecutionState right -> Just StatusWord
    (A.AND, _, Just StatusWord) | coversExecutionState left -> Just StatusWord
    (A.BIC, _, _) | statusNeutral left || coversExecutionState right -> Just StatusNeutral
    (A.BIC, Just StatusWord, _) | statusNeutral right -> Just StatusWord
    _ | operation `elem` [A.ORR, A.EOR] -> case (left, right) of
      _ | statusNeutral left && statusNeutral right -> Just StatusNeutral
      (Just StatusWord, _) | statusNeutral right -> Just StatusWord
      (_, Just StatusWord) | statusNeutral left -> Just StatusWord
      _ -> unknown
    _ -> unknown

shiftConstant :: A.Shift -> Int -> Word32 -> Maybe Word32
shiftConstant shift amount value = case shift of
  A.LSL -> Just (value `shiftL` amount)
  A.LSR -> Just (if amount == 0 then 0 else value `shiftR` amount)
  A.ASR -> Just (fromIntegral ((fromIntegral value :: Int32) `shiftR` (if amount == 0 then 32 else amount)))
  A.ROR -> if amount == 0 then Nothing else Just (rotateR value amount)

operandValue :: FrameState -> A.Operand -> Maybe Value
operandValue _ (A.Immediate value) = Just (Constant value)
operandValue state (A.ShiftedImmediate register A.LSL 0) = M.lookup register (registers state)
operandValue state (A.ShiftedImmediate register shift amount) = case (shift, M.lookup register (registers state)) of
  (_, Just (Constant value)) -> Constant <$> shiftConstant shift amount value
  (A.LSL, value) -> multiplyValue (frameExposure state) value (Just (Constant (1 `shiftL` amount))) ProductOnly
  (_, value) -> derivedFrom [value]
operandValue state (A.ShiftedRegister register shift amountRegister) = case M.lookup amountRegister (registers state) of
  Just (Constant amount)
    | amount .&. 0xff == 0 -> operandValue state (A.ShiftedImmediate register A.LSL 0)
    | amount .&. 0xff < 32 -> operandValue state (A.ShiftedImmediate register shift (fromIntegral (amount .&. 0xff)))
  amountValue -> derivedFrom [M.lookup register (registers state), amountValue]

accessAddress :: FrameState -> A.Reg -> A.Offset -> A.Indexing -> (Maybe Value, FrameState)
accessAddress state base offset indexing = (if indexing == A.PostIndexed then original else adjusted, updated)
  where
    original = M.lookup base (registers state)
    delta = case offset of
      A.OffsetImmediate value -> Just value
      A.OffsetRegister positive register shift amount -> case M.lookup register (registers state) of
        Just (Constant value) -> do
          shifted <- fromIntegral <$> shiftConstant shift amount value
          pure (if positive then shifted else negate shifted)
        _ -> Nothing
    adjusted = case delta of
      Just value -> add original value
      Nothing -> indexedAddress (frameExposure state) original (case offset of
        A.OffsetImmediate _ -> Nothing
        A.OffsetRegister _ register _ _ -> M.lookup register (registers state))
    updated = if indexing == A.OffsetOnly then state else setRegister base adjusted state

widthBytes :: A.Width -> Word32
widthBytes width = case width of
  A.Byte -> 1
  A.Half -> 2
  A.Word -> 4
  A.Dual -> 8

readFrame :: Maybe Value -> FrameState -> Maybe Value
readFrame (Just (FrameAddress offset@(FrameOffset raw))) state
  | raw .&. 3 == 0 = M.lookup offset (frameWords state)
readFrame _ _ = Nothing

writeFrame :: Maybe Value -> Word32 -> Maybe Value -> FrameState -> FrameState
writeFrame (Just (FrameAddress offset@(FrameOffset raw))) size value state = (exposeFrame value state) {frameWords = stored}
  where
    untouched = M.filterWithKey (\(FrameOffset other) _ ->
      let forward = fromIntegral (other-raw) :: Word32
          backward = fromIntegral (raw-other) :: Word32
       in forward >= size && backward >= 4) (frameWords state)
    stored = case value of
      Just known | size == 4 && raw .&. 3 == 0 -> M.insert offset known untouched
      _ -> untouched
writeFrame (Just (Constant _)) _ value state = usePremise GlobalObjectStackSeparation (exposeFrame value state)
writeFrame (Just (GlobalObject _)) _ value state = usePremise GlobalObjectStackSeparation (exposeFrame value state)
writeFrame (Just ReturnedObject) _ value state =
  usePremise UnexposedFrameIsolation ((exposeFrame value state)
    {frameWords = if frameExposure state == PrivateFrame then frameWords state else savedWords state})
-- An empty frame map has nothing for the store to clobber.
writeFrame (Just (LoadedObject _ bases)) _ value state
  | M.null (frameWords state) = globalBases (exposeFrame value state)
  | otherwise = globalBases (usePremise LoadedObjectFrameIsolation ((exposeFrame value state)
      {frameWords = if frameExposure state == PrivateFrame then frameWords state else savedWords state}))
  where
    globalBases = if S.null bases then id else usePremise GlobalObjectStackSeparation
writeFrame _ _ value state = (exposeFrame value state) {frameWords = M.empty}

-- Whether a word `displacement` bytes from a record start overlaps the saved
-- stack pointer of that record or of the next one.
overlapsSavedStack :: SavedStackArray -> Integer -> Bool
overlapsSavedStack array displacement = stride > 0 && (within field || within (field + stride))
  where
    stride = toInteger (savedStackStride array)
    field = toInteger (savedStackField array)
    offset = displacement `mod` stride
    within start = offset < start + 4 && offset + 4 > start

-- An indexed global's constant base reaches an array from one record before it.
reachesSavedStacks :: [SavedStackArray] -> Word32 -> Bool
reachesSavedStacks arrays base = or
  [ toInteger base >= start - stride && toInteger base < start + stride * toInteger (savedStackCount array)
  | array@(SavedStackArray (GuestAddress address) _ _ _) <- arrays
  , let start = toInteger address
        stride = toInteger (savedStackStride array) ]

readsSavedStack :: [SavedStackArray] -> Word32 -> Bool
readsSavedStack arrays address = or
  [ offset > -4 && offset < toInteger (savedStackStride array * savedStackCount array) && overlapsSavedStack array offset
  | array@(SavedStackArray (GuestAddress start) _ _ _) <- arrays, let offset = toInteger address - toInteger start ]

blockStart :: Maybe Value -> A.BlockMode -> Int -> Maybe Value
blockStart base mode count = add base (case mode of
  A.IncrementAfter -> 0
  A.IncrementBefore -> 4
  A.DecrementAfter -> negate (4 * fromIntegral (count-1))
  A.DecrementBefore -> negate (4 * fromIntegral count))

blockWriteback :: A.Reg -> A.BlockMode -> Bool -> Int -> Maybe Value -> FrameState -> FrameState
blockWriteback base mode enabled count original state
  | enabled = setRegister base (add original (direction * 4 * fromIntegral count)) state
  | otherwise = state
  where
    direction = if mode `elem` [A.IncrementAfter,A.IncrementBefore] then 1 else -1

executeStep :: [SavedStackArray] -> M.Map ProgramCounter LiteralWord -> ProgramCounter -> Step -> FrameState -> Either String FrameState
executeStep arrays literals address (Executes instruction) state = execute arrays literals address instruction state
executeStep _ _ (ProgramCounter pc) (WritesStatus (A.StatusWrite _ target fields source)) state
  | target == A.SavedStatus = Left ("unadmitted SPSR write at " ++ showHex pc "")
  -- The status and flags fields hold no mode, T or E bit.
  | all (`elem` [A.StatusBits, A.FlagBits]) fields = pure state
  | value == Just StatusWord = pure state
  | otherwise = Left ("CPSR write may change the mode or endianness at " ++ showHex pc "")
  where
    value = case source of
      A.StatusFromRegister register -> M.lookup register (registers state)
      A.StatusFromImmediate _ -> Nothing
executeStep _ _ _ (SignExtends (A.SignExtension _ destination source _ _)) state =
  pure (setRegister destination (derivedFrom [M.lookup source (registers state)]) state)

execute :: [SavedStackArray] -> M.Map ProgramCounter LiteralWord -> ProgramCounter -> A.Instruction -> FrameState -> Either String FrameState
execute arrays literals (ProgramCounter pc) instruction state = case instruction of
  A.MoveWide _ destination value -> pure (setRegister destination (Just (Constant value)) state)
  A.MoveTop _ destination value -> pure (setRegister destination (case reg destination of
    Just (Constant low) -> Just (Constant ((low .&. 0xffff) .|. (value `shiftL` 16)))
    value' -> derivedFrom [value']) state)
  A.DataProcessing _ operation _ destination source operand
    | operation `elem` [A.TST,A.TEQ,A.CMP,A.CMN] -> pure state
    | destination == A.pc && returns instruction -> requireReturn (reg A.lr) >> pure state
    | otherwise -> pure (setRegister destination (case operation of
        A.MOV -> operandValue state operand
        A.ADD -> case operandValue state operand of
          Just (Constant value) -> add (reg source) (fromIntegral value)
          _ -> indexedAddress (frameExposure state) (reg source) (operandValue state operand)
        A.SUB -> case operandValue state operand of
          Just (Constant value) -> add (reg source) (negate (fromIntegral value))
          _ -> indexedAddress (frameExposure state) (reg source) (operandValue state operand)
        _ -> bitwiseValue operation (reg source) (operandValue state operand)) state)
  A.ReadStatus _ destination -> pure (setRegister destination (Just StatusWord) state)
  A.ZeroExtend _ destination source _ _ -> pure (setRegister destination (derivedFrom [reg source]) state)
  A.Multiply _ destination left right accumulator ->
    pure (setRegister destination (multiplyValue (frameExposure state) (reg left) (reg right) (maybe ProductOnly (Addend . reg) accumulator)) state)
  -- Compute exposes destinations without source registers.
  A.Compute _ destinations -> pure (foldr (\destination -> setRegister destination (Just FrameDerived)) state destinations)
  A.Load _ width _ destination base offset indexing -> do
    let (address, updated) = accessAddress state base offset indexing
        loadWord pointer = case pointer of
          Just (Constant absolute) -> case M.lookup (ProgramCounter absolute) literals of
            Just (LiteralWord value) -> Just (Constant value)
            Nothing -> Nothing
          _ -> readFrame pointer state
        actualAddress = case (base == A.pc, offset, indexing) of
          (True, A.OffsetImmediate delta, A.OffsetOnly) -> add (Just (Constant (pc+8))) delta
          (True, _, _) -> Nothing
          _ -> address
        readable pointer = case pointer of
          Just (Constant absolute) -> not (readsSavedStack arrays absolute)
          Just (GlobalObject bases) -> not (any (reachesSavedStacks arrays) bases)
          Just (LoadedObject (Just displacement) bases) ->
            not (any (`overlapsSavedStack` toInteger displacement) arrays || any (reachesSavedStacks arrays) bases)
          _ -> False
        loadedObject pointer value
          | isNothing value && frameExposure state == PrivateFrame && readable pointer = Just (LoadedObject (Just 0) S.empty)
          | otherwise = value
        loaded = if width `elem` [A.Word,A.Dual] then loadedObject actualAddress (loadWord actualAddress) else Nothing
        first = setRegister destination loaded updated
        second = add actualAddress 4
    pure (if width == A.Dual then setRegister (nextRegister destination) (loadedObject second (loadWord second)) first else first)
  A.Store _ width source base offset indexing -> do
    let (address, updated) = accessAddress state base offset indexing
    pure (if width == A.Dual
      then writeFrame (add address 4) 4 (reg (nextRegister source)) (writeFrame address 4 (reg source) updated)
      else writeFrame address (widthBytes width) (reg source) updated)
  A.StoreMultiple _ base mode writeback listed -> do
    let original = reg base
        start = blockStart original mode (length listed)
        stored = foldl' (\current (index, source) -> writeFrame (add start (4*index)) 4 (reg source) current) state (zip [0..] listed)
        protected = case start of
          Just (FrameAddress first) | base == A.sp && mode == A.DecrementBefore && writeback && reg A.lr == Just IncomingReturn && A.lr `elem` listed ->
            S.fromList [offset | (index, A.Reg number) <- zip [0..] listed, number `elem` ([4..11] ++ [14]), Just (FrameAddress offset) <- [add (Just (FrameAddress first)) (4*index)]]
          _ -> S.empty
    pure (blockWriteback base mode writeback (length listed) original
      (stored {savedRegisters = S.union (savedRegisters stored) protected}))
  A.LoadMultiple _ base mode writeback listed -> do
    let original = reg base
        start = blockStart original mode (length listed)
    loaded <- foldM (\current (index, destination) -> do
      let value = readFrame (add start (4*index)) state
      if destination == A.pc then requireReturn value >> pure current else pure (setRegister destination value current)) state (zip [0..] listed)
    pure (blockWriteback base mode writeback (length listed) original loaded)
  A.Branch _ True _ -> pure (callee state)
  A.BranchExchange _ True _ -> pure (callee state)
  A.BranchExchange _ False register | register == A.lr -> requireReturn (reg A.lr) >> pure state
  _ -> pure state
  where
    reg register = M.lookup register (registers state)
    nextRegister (A.Reg number) = A.Reg (number+1)
    requireReturn value = unless (value == Just IncomingReturn) (Left ("PC load is not proven to be the incoming return at " ++ showHex pc ""))
    -- Opaque calls assume the admitted AAPCS preserves SP and r4-r11, not the CPSR.
    callee current = usePremise UnexposedFrameIsolation (current
      { registers = M.insert (A.Reg 0) ReturnedObject (M.filter (/= StatusWord) (foldr M.delete (registers current) (A.lr : A.Reg 12 : map A.Reg [0..3])))
      , frameWords = case M.lookup A.sp (registers current) of
          Just (FrameAddress boundary) -> M.filterWithKey (\offset value -> offset >= boundary && value /= StatusWord)
            (if exposure == PrivateFrame then frameWords current else savedWords current)
          _ -> M.empty
      , frameExposure = exposure
      })
      where
        exposure
          | frameExposure current == ExposedFrame || any (\register -> isFrameValue (M.lookup register (registers current))) (map A.Reg [0..3]) = ExposedFrame
          | otherwise = PrivateFrame
