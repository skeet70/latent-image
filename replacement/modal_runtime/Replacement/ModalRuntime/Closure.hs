{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The closure FUN_00bfb028 reaches across every controller that calls it,
-- computed from the installed images, the package publications and the linked
-- bank. A global entry hook is admissible only when the proof carries neither
-- an obligation nor a refusal.
module Replacement.ModalRuntime.Closure
  ( RegionKind (..)
  , Region (..)
  , Publication (..)
  , BankFunction (..)
  , Inputs (..)
  , Proof (..)
  , buildProof
  ) where

import A32
  ( BlockMode (..)
  , Cond (..)
  , DataOp (..)
  , Forbidden (..)
  , Indexing (..)
  , Instruction (..)
  , Offset (..)
  , Operand (..)
  , Reg (..)
  , Shift (..)
  , Width (..)
  , decode
  , lr
  , pc
  , regName
  , sp
  )
import Data.Aeson (Value, object, (.=))
import Data.Bits (bit, complement, rotateR, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int16, Int32)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word16, Word32, Word8)
import Numeric (showHex)

-- * Pinned X-M1 1.50 layout

dispatcherEntry, dispatcherEnd :: Word32
dispatcherEntry = 0x00bfb028
dispatcherEnd = 0x00bfb124

-- | The prehook and its callee keep the stock bytes the owned dispatcher calls.
retainedServices :: [(String, Word32, Word32)]
retainedServices = [("prehook", 0x00d0cc68, 0x00d0cc9c), ("prehook-callee", 0x00d3cde8, 0x00d3cdfc)]

cellPointers, actionPointers, transitionPointers, widthCells, stateCells, eventCells :: Word32
cellPointers = 0x00cf58b8
actionPointers = 0x00cfe2a8
transitionPointers = 0x00d0013c
widthCells = 0x00cd5e78
stateCells = 0x01467588
eventCells = 0x014676e4

-- | The state and event halfword arrays are adjacent and share the index.
controllerCount :: Word32
controllerCount = (eventCells - stateCells) `div` 2

arraysEnd :: Word32
arraysEnd = eventCells + 2 * controllerCount

-- | End of the plain image load; constants from here up name bss objects.
bssStart :: Word32
bssStart = 0x01460000

ownedDispatcherName :: String
ownedDispatcherName = "rb_modal_dispatch"

-- * Typed forms

newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)

newtype ActionSlot = ActionSlot Word16 deriving (Eq, Ord, Show)

newtype CellIndex = CellIndex Word16 deriving (Eq, Ord, Show)

-- | A published word the bank owns inside a dispatcher table.
data Interposition
  = ActionEntry ControllerId ActionSlot String
  | CellAction ControllerId CellIndex ActionSlot

-- | The bank's published modal-75 words: action slots 1, 37 and 66, and the
-- SET, editor-confirm and two leaving cells routed to slot 1.
existingInterpositions :: [Interposition]
existingInterpositions =
  [ ActionEntry (ControllerId 75) (ActionSlot 1) "rb_bank_interpose"
  , ActionEntry (ControllerId 75) (ActionSlot 37) "rb_bank_key_left"
  , ActionEntry (ControllerId 75) (ActionSlot 66) "rb_bank_key_right"
  , CellAction (ControllerId 75) (CellIndex 368) (ActionSlot 1)
  , CellAction (ControllerId 75) (CellIndex 398) (ActionSlot 1)
  , CellAction (ControllerId 75) (CellIndex 598) (ActionSlot 1)
  , CellAction (ControllerId 75) (CellIndex 628) (ActionSlot 1)
  ]

-- * Inputs

data RegionKind = StockImage | BankText | BankData deriving (Eq, Show)

data Region = Region
  { regionName :: String
  , regionKind :: RegionKind
  , regionBase :: Word32
  , regionBytes :: B.ByteString
  }

data Publication = Publication
  { publicationAddress :: Word32
  , publicationOld :: Word32
  , publicationNew :: Word32
  }

data BankFunction = BankFunction
  { bankFunctionName :: String
  , bankFunctionAddress :: Word32
  , bankFunctionSize :: Word32
  }

data Inputs = Inputs
  { inputRegions :: [Region]
  , inputPublications :: [Publication]
  , inputBankFunctions :: [BankFunction]
  }

data Proof = Proof
  { proofAdmitted :: Bool
  , proofValue :: Value
  }

-- * Memory

data Memory = Memory
  { memoryRegions :: M.Map Word32 Region
  , memoryPatches :: M.Map Word32 Word32
  }

regionEnd :: Region -> Integer
regionEnd region = toInteger (regionBase region) + toInteger (B.length (regionBytes region))

regionAt :: Memory -> Word32 -> Maybe Region
regionAt memory address = case M.lookupLE address (memoryRegions memory) of
  Just (_, region) | toInteger address < regionEnd region -> Just region
  _ -> Nothing

littleWord :: B.ByteString -> Int -> Word32
littleWord bytes offset = foldr (\k acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + k))) 0 [0 .. 3]

originalWord :: Memory -> Word32 -> Maybe Word32
originalWord memory address
  | address .&. 3 /= 0 = Nothing
  | otherwise = do
      region <- regionAt memory address
      let offset = fromIntegral (address - regionBase region)
      if offset + 4 <= B.length (regionBytes region) then Just (littleWord (regionBytes region) offset) else Nothing

memoryWord :: Memory -> Word32 -> Maybe Word32
memoryWord memory address = case M.lookup address (memoryPatches memory) of
  Just patched -> Just patched
  Nothing -> originalWord memory address

memoryHalf :: Memory -> Word32 -> Maybe Word16
memoryHalf memory address = do
  value <- memoryWord memory (address .&. complement 3)
  pure (fromIntegral (value `shiftR` (8 * fromIntegral (address .&. 2))))

memoryWords :: Memory -> [(Word32, Word32)]
memoryWords memory =
  [ (address, M.findWithDefault (littleWord bytes offset) address (memoryPatches memory))
  | region <- M.elems (memoryRegions memory)
  , let bytes = regionBytes region
        skew = fromIntegral ((4 - regionBase region .&. 3) .&. 3) :: Int
  , offset <- [skew, skew + 4 .. B.length bytes - 4]
  , let address = regionBase region + fromIntegral offset
  ]

bitsOf :: Int -> Int -> Word32 -> Word32
bitsOf high low word = (word `shiftR` low) .&. ((1 `shiftL` (high - low + 1)) - 1)

-- * Abstract registers

-- | `Derived` is a value computed from a tracked pointer whose exact value
-- was widened away.
data Val = Known !Word32 | Unknown | Derived
  deriving (Eq, Ord, Show)

newtype Regs = Regs [Val] deriving (Eq, Ord)

newtype Flags = Flags Word8 deriving (Eq, Ord)

type World = (Regs, Flags)

regAt :: Regs -> Reg -> Val
regAt (Regs values) (Reg n) = values !! n

regValues :: Regs -> [Val]
regValues (Regs values) = values

setReg :: Reg -> Val -> Regs -> Regs
setReg (Reg n) value (Regs values) = Regs (take n values ++ value : drop (n + 1) values)

setRegs :: [(Reg, Val)] -> Regs -> Regs
setRegs updates regs = foldl' (\acc (register, value) -> setReg register value acc) regs updates

unknownRegs :: Regs
unknownRegs = Regs (replicate 16 Unknown)

unknownWorlds :: S.Set World
unknownWorlds = S.fromList [(unknownRegs, Flags f) | f <- [0 .. 15]]

isKnown :: Val -> Bool
isKnown (Known _) = True
isKnown _ = False

knownValue :: Val -> Maybe Word32
knownValue (Known value) = Just value
knownValue _ = Nothing

mapKnown :: (Word32 -> Word32) -> Val -> Val
mapKnown f (Known value) = Known (f value)
mapKnown _ other = other

taint :: [Val] -> Val
taint values = if Derived `elem` values then Derived else Unknown

combine :: (Word32 -> Word32 -> Word32) -> Val -> Val -> Val
combine f (Known x) (Known y) = Known (f x y)
combine _ x y = taint [x, y]

condPass :: Cond -> Flags -> Bool
condPass (Cond c) (Flags f) = case c of
  0 -> zero
  1 -> not zero
  2 -> carry
  3 -> not carry
  4 -> negative
  5 -> not negative
  6 -> overflow
  7 -> not overflow
  8 -> carry && not zero
  9 -> not carry || zero
  10 -> negative == overflow
  11 -> negative /= overflow
  12 -> not zero && negative == overflow
  13 -> zero || negative /= overflow
  _ -> True
 where
  negative = testBit f 3
  zero = testBit f 2
  carry = testBit f 1
  overflow = testBit f 0

instructionCond :: Instruction -> Cond
instructionCond instruction = case instruction of
  DataProcessing c _ _ _ _ _ -> c
  MoveWide c _ _ -> c
  MoveTop c _ _ -> c
  ZeroExtend c _ _ _ _ -> c
  Multiply c _ _ _ _ -> c
  Compute c _ -> c
  Load c _ _ _ _ _ _ -> c
  Store c _ _ _ _ _ -> c
  LoadMultiple c _ _ _ _ -> c
  StoreMultiple c _ _ _ _ -> c
  Branch c _ _ -> c
  BranchExchange c _ _ -> c
  Hint c -> c
  Rejected _ -> Cond 14

-- * One instruction

data Flow = Continue | Jump Word32 | Call (Maybe Word32) | Return | JumpIndirect | Switch [Word32]

data Access = Access
  { accessStore :: Bool
  , accessAddress :: Maybe Word32
  , accessSize :: Word32
  , accessValues :: [Val]
  , accessBase :: Val
  }

data Effect = Effect
  { effectRegs :: Regs
  , effectSetsFlags :: Bool
  , effectRefinement :: Maybe (Reg, Word32)
  , effectAccesses :: [Access]
  , effectFlow :: Flow
  , effectModeled :: Bool
  }

plain :: Regs -> Effect
plain regs = Effect regs False Nothing [] Continue True

-- | An unmodeled callee may write any register other than pc.
callClobber :: [(Reg, Val)]
callClobber = [(Reg n, Unknown) | n <- [0 .. 14]]

readReg :: Word32 -> Regs -> Reg -> Val
readReg address regs register
  | register == pc = Known (address + 8)
  | otherwise = regAt regs register

nextReg :: Reg -> Reg
nextReg (Reg n) = Reg (n + 1)

widthBytes :: Width -> Word32
widthBytes width = case width of
  Byte -> 1
  Half -> 2
  Word -> 4
  Dual -> 8

arithmeticShift :: Word32 -> Int -> Word32
arithmeticShift value amount = fromIntegral ((fromIntegral value :: Int32) `shiftR` min amount 31)

shiftByImmediate :: Shift -> Int -> Val -> Val
shiftByImmediate kind amount value = case value of
  Known v -> case kind of
    LSL -> Known (v `shiftL` amount)
    LSR -> Known (if amount == 0 then 0 else v `shiftR` amount)
    ASR -> Known (arithmeticShift v (if amount == 0 then 32 else amount))
    ROR -> if amount == 0 then Unknown else Known (rotateR v amount)
  other -> other

operandValue :: Word32 -> Regs -> Operand -> Val
operandValue address regs operand = case operand of
  Immediate value -> Known value
  ShiftedImmediate rm kind amount -> shiftByImmediate kind amount (readReg address regs rm)
  ShiftedRegister rm kind rs -> case (readReg address regs rm, readReg address regs rs) of
    (Known value, Known count)
      | amount == 0 -> Known value
      | amount >= 32 -> Unknown
      | otherwise -> shiftByImmediate kind amount (Known value)
     where
      amount = fromIntegral (count .&. 0xff) :: Int
    (x, y) -> taint [x, y]

offsetValue :: Word32 -> Regs -> Offset -> Val
offsetValue _ _ (OffsetImmediate n) = Known (fromIntegral n)
offsetValue address regs (OffsetRegister add rm kind amount) =
  let shifted = shiftByImmediate kind amount (readReg address regs rm)
   in if add then shifted else mapKnown negate shifted

signExtend :: Int -> Word32 -> Word32
signExtend width value = if testBit value (width - 1) then value .|. complement (bit width - 1) else value

-- | SXTH, SXTB, UBFX and BFI/BFC, which the decoder reports as untracked.
mediaExact :: Word32 -> Word32 -> Regs -> Maybe (Reg, Val)
mediaExact address word regs
  | word .&. 0x0fff03f0 == 0x06bf0070 = Just (destination, mapKnown (signExtend 16 . (.&. 0xffff) . rotated) source)
  | word .&. 0x0fff03f0 == 0x06af0070 = Just (destination, mapKnown (signExtend 8 . (.&. 0xff) . rotated) source)
  | word .&. 0x0fe00070 == 0x07e00050 = Just (destination, mapKnown (\v -> (v `shiftR` lsb) .&. (bit (msb + 1) - 1)) source)
  | word .&. 0x0fe00070 == 0x07c00010 =
      Just (destination, combine (\old new -> (old .&. complement mask) .|. ((new `shiftL` lsb) .&. mask)) (regAt regs destination) inserted)
  | otherwise = Nothing
 where
  destination = Reg (fromIntegral (bitsOf 15 12 word))
  rm = Reg (fromIntegral (bitsOf 3 0 word))
  source = readReg address regs rm
  inserted = if rm == pc then Known 0 else source
  rotated v = rotateR v (8 * fromIntegral (bitsOf 11 10 word))
  lsb = fromIntegral (bitsOf 11 7 word) :: Int
  msb = fromIntegral (bitsOf 20 16 word) :: Int
  mask = if msb >= lsb then (bit (msb - lsb + 1) - 1) `shiftL` lsb else 0 :: Word32

-- | Targets of `ldrls pc, [pc, rX, lsl #2]` and `addls pc, pc, rX, lsl #2`
-- when the word before it is `cmp rX, #n` and no branch or data word enters
-- the switch itself.
switchTargets :: Analysis -> Word32 -> Word32 -> Maybe [Word32]
switchTargets analysis address word
  | bitsOf 31 28 word /= 9 = Nothing
  | M.member address (analysisPredecessors analysis) || M.member address (analysisDataEntries analysis) = Nothing
  | word .&. 0x0ffffff0 == 0x079ff100 = bound >>= \n -> mapM (\i -> memoryWord memory (address + 8 + 4 * i)) [0 .. n]
  | word .&. 0x0ffffff0 == 0x008ff100 = bound >>= \n -> Just [address + 8 + 4 * i | i <- [0 .. n]]
  | otherwise = Nothing
 where
  memory = analysisMemory analysis
  index = bitsOf 3 0 word
  bound = do
    previous <- memoryWord memory (address - 4)
    if previous .&. 0x0ff0f000 == 0x03500000 && bitsOf 19 16 previous == index && index /= 15 && bitsOf 31 28 previous == 14
      then Just (rotateR (previous .&. 0xff) (2 * fromIntegral (bitsOf 11 8 previous)))
      else Nothing

execute :: Analysis -> Word32 -> Word32 -> Instruction -> Regs -> Effect
execute analysis address word instruction regs = case instruction of
  DataProcessing _ op setsFlags rd rn operand ->
    let operandVal = operandValue address regs operand
        firstVal = readReg address regs rn
        tests = op `elem` [TST, TEQ, CMP, CMN]
        result = case op of
          MOV -> operandVal
          MVN -> mapKnown complement operandVal
          AND -> combine (.&.) firstVal operandVal
          EOR -> combine xor firstVal operandVal
          SUB -> combine (-) firstVal operandVal
          RSB -> combine (flip (-)) firstVal operandVal
          ADD -> combine (+) firstVal operandVal
          ORR -> combine (.|.) firstVal operandVal
          BIC -> combine (\x y -> x .&. complement y) firstVal operandVal
          _ -> taint [firstVal, operandVal]
        refinement
          | op /= CMP || rn == pc = Nothing
          | Known value <- operandVal, not (isKnown firstVal) = Just (rn, value)
          | Known value <- firstVal, ShiftedImmediate rm LSL 0 <- operand, rm /= pc, not (isKnown operandVal) = Just (rm, value)
          | otherwise = Nothing
        programCounterFlow = case switchTargets analysis address word of
          Just targets -> Switch targets
          Nothing -> case operand of
            ShiftedImmediate source LSL 0 | op == MOV && source == lr -> Return
            _ -> JumpIndirect
     in if tests
          then (plain regs) {effectSetsFlags = True, effectRefinement = refinement}
          else
            if rd == pc
              then (plain regs) {effectSetsFlags = setsFlags, effectFlow = programCounterFlow}
              else (plain (setReg rd result regs)) {effectSetsFlags = setsFlags}
  MoveWide _ rd value -> plain (setReg rd (Known value) regs)
  MoveTop _ rd value -> plain (setReg rd (mapKnown (\old -> (value `shiftL` 16) .|. (old .&. 0xffff)) (regAt regs rd)) regs)
  ZeroExtend _ rd rm rotation width ->
    plain (setReg rd (mapKnown (\v -> rotateR v rotation .&. (bit width - 1)) (readReg address regs rm)) regs)
  Multiply _ rd rm rs accumulator ->
    let productVal = combine (*) (readReg address regs rm) (readReg address regs rs)
        total = maybe productVal (combine (+) productVal . readReg address regs) accumulator
     in plain (setReg rd total regs)
  Compute _ destinations -> case mediaExact address word regs of
    Just (rd, value) -> plain (setReg rd value regs)
    Nothing -> plain (setRegs [(register, Unknown) | register <- destinations] regs)
  Load _ width _ rt rn offset indexing ->
    let base = readReg address regs rn
        updated = combine (+) base (offsetValue address regs offset)
        effective = knownValue (if indexing == PostIndexed then base else updated)
        literal
          | rn == pc, indexing == OffsetOnly, width == Word, Just target <- effective = maybe Unknown Known (memoryWord (analysisMemory analysis) target)
          | otherwise = Unknown
        loaded = case width of
          Dual -> [(rt, Unknown), (nextReg rt, Unknown)]
          _ -> [(rt, literal) | rt /= pc]
        writeback = [(rn, updated) | indexing /= OffsetOnly, rn /= rt, rn /= pc]
        flow
          | rt /= pc = Continue
          | rn == sp && indexing == PostIndexed = Return
          | Just targets <- switchTargets analysis address word = Switch targets
          | otherwise = JumpIndirect
     in (plain (setRegs (loaded ++ writeback) regs)) {effectAccesses = [Access False effective (widthBytes width) [] base], effectFlow = flow}
  Store _ width rt rn offset indexing ->
    let base = readReg address regs rn
        updated = combine (+) base (offsetValue address regs offset)
        effective = knownValue (if indexing == PostIndexed then base else updated)
        source = readReg address regs rt
        values = case width of
          Byte -> [mapKnown (.&. 0xff) source]
          Half -> [mapKnown (.&. 0xffff) source]
          Word -> [source]
          Dual -> [source, readReg address regs (nextReg rt)]
        writeback = [(rn, updated) | indexing /= OffsetOnly, rn /= pc]
     in (plain (setRegs writeback regs)) {effectAccesses = [Access True effective (widthBytes width) values base]}
  LoadMultiple _ rn mode writeback list ->
    let (after, accesses) = blockAccesses False address regs rn mode list
        flow
          | pc `notElem` list = Continue
          | rn == sp = Return
          | otherwise = JumpIndirect
     in (plain (setRegs ([(register, Unknown) | register <- list, register /= pc] ++ [(rn, after) | writeback, rn `notElem` list]) regs))
          {effectAccesses = accesses, effectFlow = flow}
  StoreMultiple _ rn mode writeback list ->
    let (after, accesses) = blockAccesses True address regs rn mode list
     in (plain (setRegs [(rn, after) | writeback] regs)) {effectAccesses = accesses}
  Branch _ link target
    | link -> (plain (setRegs callClobber regs)) {effectSetsFlags = True, effectFlow = Call (Just target)}
    | otherwise -> (plain regs) {effectFlow = Jump target}
  BranchExchange _ link rm
    | link -> (plain (setRegs callClobber regs)) {effectSetsFlags = True, effectFlow = Call Nothing}
    | rm == lr -> (plain regs) {effectFlow = Return}
    | otherwise -> (plain regs) {effectFlow = JumpIndirect}
  Hint _ -> plain regs
  Rejected reason -> rejectedEffect address word reason regs

blockAccesses :: Bool -> Word32 -> Regs -> Reg -> BlockMode -> [Reg] -> (Val, [Access])
blockAccesses store address regs rn mode list =
  ( mapKnown (\b -> if ascending then b + 4 * count else b - 4 * count) base
  , [ Access store (fmap (+ 4 * k) start) 4 [readReg address regs register | store] base
    | (k, register) <- zip [0 ..] list
    ]
  )
 where
  base = regAt regs rn
  count = fromIntegral (length list) :: Word32
  ascending = mode == IncrementAfter || mode == IncrementBefore
  start = case knownValue base of
    Just b -> Just (case mode of
      IncrementAfter -> b
      IncrementBefore -> b + 4
      DecrementAfter -> b - 4 * count + 4
      DecrementBefore -> b - 4 * count)
    Nothing -> Nothing

rejectedEffect :: Word32 -> Word32 -> Forbidden -> Regs -> Effect
rejectedEffect address word reason regs = case reason of
  Coprocessor _
    | word .&. 0x0f100010 == 0x0e100010 ->
        if registerAt 12 == pc then (plain regs) {effectSetsFlags = True} else plain (setReg (registerAt 12) Unknown regs)
    | word .&. 0x0fe00000 == 0x0c400000 ->
        if testBit word 20 then plain (setRegs [(registerAt 12, Unknown), (registerAt 16, Unknown)] regs) else plain regs
    | bitsOf 27 25 word == 6 -> coprocessorTransfer
    | otherwise -> plain regs
  SupervisorCall -> (plain (setRegs callClobber regs)) {effectSetsFlags = True, effectFlow = Call Nothing}
  SystemRegister
    | word .&. 0x0fbf0fff == 0x010f0000 -> plain (setReg (registerAt 12) Unknown regs)
    | otherwise -> (plain regs) {effectSetsFlags = testBit word 19}
  Divide -> plain (setReg (registerAt 16) Unknown regs)
  ThumbBranch -> (plain (setRegs callClobber regs)) {effectSetsFlags = True, effectFlow = Call Nothing}
  Barrier -> plain regs
  WaitForEvent -> plain regs
  InterruptMask -> plain regs
  Breakpoint -> plain regs
  _ -> (plain unknownRegs) {effectSetsFlags = True, effectModeled = False}
 where
  registerAt low = Reg (fromIntegral (bitsOf (low + 3) low word))
  coprocessorTransfer =
    let rn = registerAt 16
        base = readReg address regs rn
        immediate = (word .&. 0xff) * 4
        moved = mapKnown (\b -> if testBit word 23 then b + immediate else b - immediate) base
        preIndexed = testBit word 24
        writeback = testBit word 21
        size
          | writeback || not preIndexed = immediate
          | testBit word 8 = 8
          | otherwise = 4
     in (plain (setRegs [(rn, moved) | writeback] regs))
          {effectAccesses = [Access (not (testBit word 20)) (knownValue (if preIndexed then moved else base)) size [] base]}

nextWorlds :: Effect -> Flags -> [World]
nextWorlds effect flags
  | effectSetsFlags effect = [(refined f, Flags f) | f <- [0 .. 15]]
  | otherwise = [(effectRegs effect, flags)]
 where
  refined f = case effectRefinement effect of
    Just (register, value) | testBit f 2 -> setReg register (Known value) (effectRegs effect)
    _ -> effectRegs effect

-- * Register use, for callee argument liveness

data UseKind = UsePlain | UseCall Word32 | UseCallIndirect | UseJump Word32 | UseReturn | UseIndirect | UseSupervisor | UseProgramCounter

useDef :: Word32 -> Word32 -> Maybe ([Int], [Int], UseKind)
useDef address word
  | bitsOf 31 28 word == 15 = Nothing
  | word .&. 0x0ff00000 == 0x03000000 = Just ([], [rd], UsePlain)
  | word .&. 0x0ff00000 == 0x03400000 = Just ([rd], [rd], UsePlain)
  | word .&. 0x0ffffff0 == 0x012fff10 = Just ([rm], [], if rm == 14 then UseReturn else UseIndirect)
  | word .&. 0x0ffffff0 == 0x012fff30 = Just ([rm], callWrites, UseCallIndirect)
  | top <= 1 && not (top == 0 && testBit word 4 && testBit word 7) = dataProcessingUse
  | top == 0 = extraUse
  | top <= 3 = singleUse
  | top == 4 = blockUse
  | top == 5 = Just (if testBit word 24 then ([], callWrites, UseCall branchTarget) else ([], [], UseJump branchTarget))
  | top == 6 = coprocessorUse
  | word .&. 0x0f000000 == 0x0f000000 = Just ([0 .. 3], [0 .. 3], UseSupervisor)
  | word .&. 0x0f000010 == 0x0e000010 = Just (if testBit word 20 then ([], [rt | rt /= 15], UsePlain) else ([rt], [], UsePlain))
  | otherwise = Just ([], [], UsePlain)
 where
  top = bitsOf 27 25 word
  register low = fromIntegral (bitsOf (low + 3) low word) :: Int
  rd = register 12
  rt = register 12
  rn = register 16
  rm = register 0
  rs = register 8
  callWrites = [0, 1, 2, 3, 12, 14]
  branchTarget =
    let offset = bitsOf 23 0 word
        extended = if testBit offset 23 then offset .|. 0xff000000 else offset
     in address + 8 + (extended `shiftL` 2)
  finish sources writes
    | 15 `elem` writes = Just (sources, filter (/= 15) writes, UseProgramCounter)
    | otherwise = Just (sources, writes, UsePlain)
  dataProcessingUse
    | opcode `elem` [8, 9, 10, 11] && not (testBit word 20) =
        if word .&. 0x0fbf0fff == 0x010f0000
          then Just ([], [rd], UsePlain)
          else
            if word .&. 0x0ff00ff0 == 0x01600f10
              then Just ([rm], [rd], UsePlain)
              else
                if word .&. 0x0fb0f000 `elem` [0x0120f000, 0x0320f000]
                  then Just ([rm | top == 0], [], UsePlain)
                  else Nothing
    | opcode `elem` [8, 9, 10, 11] = Just (operandReads, [], UsePlain)
    | rd == 15 = Just (operandReads, [], UseProgramCounter)
    | otherwise = Just (operandReads, [rd], UsePlain)
   where
    opcode = bitsOf 24 21 word
    operandReads = [rn | opcode `notElem` [13, 15]] ++ (if top == 0 then rm : [rs | testBit word 4] else [])
  extraUse
    | kind == 0 && testBit word 24 = Nothing
    | kind == 0 && testBit word 23 = Just ([rm, rs] ++ (if testBit word 21 then [rt, rn] else []), [rt, rn], UsePlain)
    | kind == 0 = Just ([rm, rs] ++ [rt | testBit word 21], [rn], UsePlain)
    | otherwise = finish (rn : [rm | not (testBit word 22)] ++ stored) (loaded ++ [rn | not (testBit word 24) || testBit word 21])
   where
    kind = bitsOf 6 5 word
    load = testBit word 20
    stored
      | load || kind == 2 = []
      | kind == 1 = [rt]
      | otherwise = [rt, rt + 1]
    loaded
      | load = [rt]
      | kind == 2 = [rt, rt + 1]
      | otherwise = []
  singleUse
    | top == 3 && testBit word 4 =
        if word .&. 0x0fe00070 == 0x07c00010
          then Just (rd : [rm | rm /= 15], [rd], UsePlain)
          else
            if word .&. 0x0f800010 == 0x07000010
              then Just ([rm, rs, rd], [rn], UsePlain)
              else Just (rm : [rn | rn /= 15], [rd], UsePlain)
    | otherwise =
        finish
          (rn : [rm | top == 3] ++ [rt | not (testBit word 20)])
          ([rt | testBit word 20] ++ [rn | not (testBit word 24) || testBit word 21])
  blockUse =
    let list = [r | r <- [0 .. 15], testBit word r]
        load = testBit word 20
        sources = rn : (if load || rn == 13 then [] else list)
        writes = (if load then list else []) ++ [rn | testBit word 21]
     in Just (sources, filter (/= 15) writes, if 15 `elem` writes then UseReturn else UsePlain)
  coprocessorUse
    | word .&. 0x0fe00000 == 0x0c400000 = Just (if testBit word 20 then ([], [rt, rn], UsePlain) else ([rt, rn], [], UsePlain))
    | otherwise = Just ([rn], [rn | testBit word 21], UsePlain)

type LiveMemo = M.Map (Word32, Int) (S.Set Int)

inputRegisters :: S.Set Int
inputRegisters = S.fromList [0 .. 12]

-- | Registers r0-r12 a callee may read before writing them.
liveIn :: Analysis -> LiveMemo -> Word32 -> Int -> (S.Set Int, LiveMemo)
liveIn analysis memo entry depth
  | Just cached <- M.lookup (entry, depth) memo = (cached, memo)
  | depth == 0 || not (isJust (memoryWord memory entry)) = (inputRegisters, memo)
  | otherwise =
      let (result, memo') = explore [(entry, S.empty)] S.empty (0 :: Int) S.empty (M.insert (entry, depth) inputRegisters memo)
       in (result, M.insert (entry, depth) result memo')
 where
  memory = analysisMemory analysis
  explore [] _ _ result m = (result, m)
  explore ((address, written) : rest) seen steps result m = walk address written rest seen steps result m
  walk address written rest seen steps result m
    | steps >= 20000 = (inputRegisters, m)
    | S.member (address, written) seen = explore rest seen (steps + 1) result m
    | otherwise =
        let seen' = S.insert (address, written) seen
            steps' = steps + 1
            open = inputRegisters `S.difference` written
         in case memoryWord memory address >>= \word -> fmap (\use -> (word, use)) (useDef address word) of
              Nothing -> explore rest seen' steps' (result `S.union` open) m
              Just (word, (sources, writes, kind)) ->
                let always = bitsOf 31 28 word == 14
                    read' = result `S.union` (S.fromList (filter (< 13) sources) `S.difference` written)
                 in case (kind, switchTargets analysis address word) of
                      (UseProgramCounter, Just targets) ->
                        walk (address + 4) written ([(target, written) | target <- targets] ++ rest) seen' steps' read' m
                      _ ->
                        let (calleeLive, m') = case kind of
                              UseCall target -> liveIn analysis m target (depth - 1)
                              _ -> (S.empty, m)
                            extra = case kind of
                              UseCall _ -> calleeLive `S.difference` written
                              UseCallIndirect -> open
                              UseIndirect -> open
                              UseSupervisor -> open
                              UseProgramCounter -> open
                              _ -> S.empty
                            result' = read' `S.union` extra
                            written' = case kind of
                              UseCall _ -> written
                              UseCallIndirect -> written
                              UseSupervisor -> written
                              _ | always -> written `S.union` S.fromList (filter (< 13) writes)
                                | otherwise -> written
                            pushed = case kind of
                              UseJump target -> [(target, written')]
                              _ -> []
                            stops = always && case kind of
                              UseJump _ -> True
                              UseReturn -> True
                              UseIndirect -> True
                              UseProgramCounter -> True
                              _ -> False
                         in if stops
                              then explore (pushed ++ rest) seen' steps' result' m'
                              else walk (address + 4) written' (pushed ++ rest) seen' steps' result' m'

-- * Windows and entries

data Analysis = Analysis
  { analysisMemory :: Memory
  , analysisPredecessors :: M.Map Word32 [(Word32, Instruction)]
  , analysisDataEntries :: M.Map Word32 [Word32]
  }

analyse :: Memory -> Analysis
analyse memory = Analysis memory predecessors dataEntries
 where
  everyWord = memoryWords memory
  predecessors =
    M.fromListWith
      (++)
      [ (target, [(site, instruction)])
      | (site, word) <- everyWord
      , bitsOf 27 25 word == 5
      , bitsOf 31 28 word /= 15
      , let instruction = decode site word
      , Branch _ _ target <- [instruction]
      ]
  dataEntries = M.fromListWith (++) [(word, [site]) | (site, word) <- everyWord, word .&. 3 == 0, isJust (regionAt memory word)]

fetch :: Analysis -> Word32 -> Maybe (Word32, Instruction)
fetch analysis address = do
  word <- memoryWord (analysisMemory analysis) address
  pure (word, decode address word)

unconditionalTransfer :: Instruction -> Bool
unconditionalTransfer instruction = case instruction of
  Branch (Cond 14) False _ -> True
  BranchExchange (Cond 14) False _ -> True
  LoadMultiple (Cond 14) _ _ _ list -> pc `elem` list
  Load (Cond 14) _ _ rt _ _ _ -> rt == pc
  DataProcessing (Cond 14) op _ rd _ _ -> rd == pc && op `notElem` [TST, TEQ, CMP, CMN]
  _ -> False

isBoundary :: Analysis -> Word32 -> Bool
isBoundary analysis address = M.member address (analysisPredecessors analysis) || M.member address (analysisDataEntries analysis)

-- | The first address of the straight-line window that ends at `address`.
windowStart :: Analysis -> Word32 -> Word32
windowStart analysis address = go address (256 :: Int)
 where
  go start remaining
    | remaining == 0 = start
    | isBoundary analysis start = start
    | otherwise = case fetch analysis (start - 4) of
        Just (_, previous) | not (unconditionalTransfer previous) -> go (start - 4) (remaining - 1)
        _ -> start

runWindow :: Analysis -> Word32 -> Word32 -> S.Set World -> S.Set World
runWindow analysis address end worlds
  | address >= end = worlds
  | S.size worlds > 8192 = unknownWorlds
  | otherwise = case fetch analysis address of
      Nothing -> unknownWorlds
      Just (word, instruction) ->
        let cond = instructionCond instruction
            advance (regs, flags)
              | not (condPass cond flags) = [(regs, flags)]
              | otherwise =
                  let effect = execute analysis address word instruction regs
                   in case effectFlow effect of
                        Continue -> nextWorlds effect flags
                        Call _ -> nextWorlds effect flags
                        _ -> []
         in runWindow analysis (address + 4) end (S.fromList (concatMap advance (S.toList worlds)))

-- | Why an entry state is not fully known.
data Entry
  = DataWordEntry Word32 [Word32]
  | CallEntry Word32 Word32
  | DepthLimit Word32
  | WindowLimit Word32
  | UnreachedEntry Word32
  deriving (Eq, Ord)

entryWorlds :: Analysis -> Word32 -> Int -> (S.Set World, S.Set Entry)
entryWorlds analysis start depth
  | depth == 0 = (unknownWorlds, S.singleton (DepthLimit start))
  | otherwise = (S.unions (map fst parts), S.unions (map snd parts))
 where
  fromData = M.member start (analysisDataEntries analysis)
  direct = M.member start (analysisPredecessors analysis)
  parts = dataPart ++ predecessorParts ++ fallthroughPart
  dataPart = [(unknownWorlds, S.singleton (DataWordEntry start (M.findWithDefault [] start (analysisDataEntries analysis)))) | fromData]
  predecessorParts =
    [ case instruction of
        Branch _ True _ -> (unknownWorlds, S.singleton (CallEntry start site))
        _ ->
          let (worlds, entries) = worldsAt analysis site (depth - 1)
           in (S.filter (\(_, flags) -> condPass (instructionCond instruction) flags) worlds, entries)
    | (site, instruction) <- M.findWithDefault [] start (analysisPredecessors analysis)
    ]
  fallthroughPart = case fetch analysis (start - 4) of
    Just (_, previous)
      | not (unconditionalTransfer previous) ->
          if direct || fromData
            then
              let (worlds, entries) = worldsAt analysis (start - 4) (depth - 1)
               in [(runWindow analysis (start - 4) start worlds, entries)]
            else [(unknownWorlds, S.singleton (WindowLimit start))]
    _
      | not direct && not fromData -> [(unknownWorlds, S.singleton (UnreachedEntry start))]
      | otherwise -> []

worldsAt :: Analysis -> Word32 -> Int -> (S.Set World, S.Set Entry)
worldsAt analysis address depth =
  let start = windowStart analysis address
      (initial, entries) = entryWorlds analysis start depth
   in (runWindow analysis start address initial, entries)

-- | The values `register` can hold at `site` when `cond` passes: from the
-- straight-line window first, then through up to `depth` predecessor joins.
resolveValues :: Analysis -> Word32 -> Reg -> Cond -> Int -> Either ([Word32], S.Set Entry) [Word32]
resolveValues analysis site register cond depth = case attempt 0 of
  Right values -> Right values
  Left _ -> attempt depth
 where
  attempt d =
    let (worlds, entries)
          | d == 0 = (runWindow analysis (windowStart analysis site) site unknownWorlds, S.empty)
          | otherwise = worldsAt analysis site d
        values = [regAt regs register | (regs, flags) <- S.toList worlds, condPass cond flags]
        knowns = S.toList (S.fromList (mapMaybe knownValue values))
     in if not (null values) && all isKnown values then Right knowns else Left (knowns, entries)

-- * Forward pointer census

data Tracking = Tracking
  { trackingTracked :: Word32 -> Bool
  , trackingPointer :: Word32 -> Bool
  , trackingHit :: Word32 -> Word32 -> Bool
  }

tracked :: Tracking -> Val -> Bool
tracked tracking value = case value of
  Known v -> trackingTracked tracking v
  Derived -> True
  Unknown -> False

pointer :: Tracking -> Val -> Bool
pointer tracking value = case value of
  Known v -> trackingPointer tracking v
  Derived -> True
  Unknown -> False

data Fact = Fact
  { factSite :: Word32
  , factStore :: Bool
  , factAddress :: Maybe Word32
  , factSize :: Word32
  , factValues :: [Val]
  , factWord :: Word32
  }
  deriving (Eq, Ord)

data Escape
  = EscapeArgument Word32 (Maybe Word32) Int
  | EscapeStored Word32
  | EscapeReturned Word32 Int
  | EscapeIndirect Word32
  deriving (Eq, Ord)

data Limit = StepLimit Word32 | WorldLimit Word32 | Unmodeled Word32 | LeavesMemory Word32 | PartialConstant Word32
  deriving (Eq, Ord)

data Walk = Walk
  { walkSeen :: S.Set (Word32, S.Set World)
  , walkVisits :: M.Map Word32 Int
  , walkHistory :: M.Map Word32 [S.Set Val]
  , walkFacts :: [Fact]
  , walkEscapes :: [Escape]
  , walkLimits :: [Limit]
  , walkSteps :: Int
  }

data Stepped = Stepped
  { steppedNext :: [World]
  , steppedJumps :: [(Word32, World)]
  , steppedFacts :: [Fact]
  , steppedEscapes :: [Escape]
  , steppedUnmodeled :: Bool
  }

stepLimit :: Int
stepLimit = 50000

-- | Follows every value derived from the constant materialized at `site`
-- through straight-line code, branches and switch tables until no tracked
-- value is live.
forward :: Analysis -> Tracking -> Word32 -> Walk
forward analysis tracking site = drive [(site, initial, True)] (Walk S.empty M.empty M.empty [] [] partial 0)
 where
  local = runWindow analysis (windowStart analysis site) site unknownWorlds
  incomplete worlds = case fetch analysis site of
    Just (_, MoveTop cond rd _) -> any (\(regs, flags) -> condPass cond flags && not (isKnown (regAt regs rd))) (S.toList worlds)
    _ -> False
  initial = if incomplete local then fst (worldsAt analysis site 3) else local
  partial = [PartialConstant site | incomplete initial]
  drive [] walk = walk
  drive ((address, worlds, first) : rest) walk
    | walkSteps walk > stepLimit = walk {walkLimits = StepLimit address : walkLimits walk}
    | otherwise =
        let (walk', pending) = follow address worlds first walk
         in drive (pending ++ rest) walk'
  liveTracked worlds = any (\(regs, _) -> any (tracked tracking) (take 15 (regValues regs))) (S.toList worlds)
  widenAll worlds = S.map (\(Regs values, flags) -> (Regs (map widenValue values), flags)) worlds
  widenValue value
    | pointer tracking value = Derived
    | tracked tracking value = Unknown
    | otherwise = value
  follow address worlds0 first walk0
    | walkSteps walk0 > stepLimit = (walk0 {walkLimits = StepLimit address : walkLimits walk0}, [])
    | not first && not (liveTracked worlds0) = (walk0, [])
    | S.member (address, worlds) (walkSeen walk0) = (walk1, [])
    | otherwise = case fetch analysis address of
        Nothing -> (walk1 {walkLimits = LeavesMemory address : walkLimits walk1}, [])
        Just (word, instruction) ->
          let cond = instructionCond instruction
              steps = map (stepOne address word instruction cond) (S.toList worlds)
              next0 = S.fromList (concatMap steppedNext steps)
              next = if S.size next0 > 4096 then widenAll next0 else next0
              jumps =
                [ (target, targetWorlds, False)
                | (target, targetWorlds) <- M.toList (M.fromListWith S.union [(target, S.singleton w) | s <- steps, (target, w) <- steppedJumps s])
                ]
              walk2 =
                walk1
                  { walkFacts = concatMap steppedFacts steps ++ walkFacts walk1
                  , walkEscapes = concatMap steppedEscapes steps ++ walkEscapes walk1
                  , walkLimits = [Unmodeled address | any steppedUnmodeled steps] ++ walkLimits walk1
                  }
           in if S.size next > 4096
                then (walk2 {walkLimits = WorldLimit address : walkLimits walk2}, jumps)
                else
                  if S.null next
                    then (walk2, jumps)
                    else
                      let (walk3, more) = follow (address + 4) next False walk2
                       in (walk3, jumps ++ more)
   where
    visits = M.findWithDefault 0 address (walkVisits walk0) + 1
    columns = [S.fromList [regAt regs (Reg i) | (regs, _) <- S.toList worlds0] | i <- [0 .. 15]]
    history = zipWith S.union (M.findWithDefault (replicate 16 S.empty) address (walkHistory walk0)) columns
    widen (Regs values, flags) =
      ( Regs
          [ if S.size seenValues > 1 then (if any (pointer tracking) (S.toList seenValues) then Derived else Unknown) else value
          | (value, seenValues) <- zip values history
          ]
      , flags
      )
    worlds = if visits > 3 then S.map widen worlds0 else worlds0
    walk1 =
      walk0
        { walkSteps = walkSteps walk0 + 1
        , walkVisits = M.insert address visits (walkVisits walk0)
        , walkHistory = M.insert address history (walkHistory walk0)
        , walkSeen = S.insert (address, worlds) (walkSeen walk0)
        }
  stepOne address word instruction cond (regs, flags)
    | not (condPass cond flags) = Stepped [(regs, flags)] [] [] [] False
    | otherwise =
        let effect0 = execute analysis address word instruction regs
            effect = effect0 {effectRegs = acrossCall (lineage (effectRegs effect0))}
            -- A tracked value an unmodeled callee may have kept stays tracked.
            acrossCall regsAfter = case effectFlow effect0 of
              Call _ -> setRegs [(Reg n, Derived) | n <- [0 .. 14], tracked tracking (regAt regs (Reg n))] regsAfter
              _ -> regsAfter
            after = effectRegs effect
            -- A register computed from a tracked pointer whose value is lost
            -- becomes Derived; registers loaded from memory hold data.
            lineage regsAfter = case useDef address word of
              Just (sources, writes, _)
                | any (\s -> s /= 15 && pointer tracking (regAt regs (Reg s))) sources ->
                    setRegs
                      [ (Reg w, Derived)
                      | w <- writes
                      , w < 15
                      , w `notElem` loaded
                      , regAt regsAfter (Reg w) == Unknown
                      ]
                      regsAfter
              _ -> regsAfter
            loaded = case instruction of
              Load _ Dual _ (Reg n) _ _ _ -> [n, n + 1]
              Load _ _ _ (Reg n) _ _ _ -> [n]
              LoadMultiple _ _ _ _ list -> [n | Reg n <- list]
              _ -> []
            facts = mapMaybe (factOf address word) (effectAccesses effect)
            stored =
              [ EscapeStored address
              | access <- effectAccesses effect
              , accessStore access
              , accessSize access >= 4
              , any (pointer tracking) (accessValues access)
              ]
            flowEscapes = case effectFlow effect of
              Call target -> [EscapeArgument address target n | n <- [0 .. 12], pointer tracking (regAt regs (Reg n))]
              Return -> [EscapeReturned address n | n <- [0 .. 12], pointer tracking (regAt regs (Reg n))]
              JumpIndirect -> [EscapeIndirect address | any (pointer tracking) (take 13 (regValues after))]
              _ -> []
            unmodeled = not (effectModeled effect) && any (pointer tracking) (regValues regs)
            (next, jumps) = case effectFlow effect of
              Continue -> (nextWorlds effect flags, [])
              Call _ -> (nextWorlds effect flags, [])
              Jump target -> ([], [(target, w) | w <- nextWorlds effect flags])
              Switch targets -> ([], [(target, (after, flags)) | target <- targets])
              _ -> ([], [])
         in Stepped next jumps facts (stored ++ flowEscapes) unmodeled
  factOf address word access = case accessAddress access of
    Nothing
      | pointer tracking (accessBase access) ->
          Just (Fact address (accessStore access) Nothing (accessSize access) (accessValues access) word)
    Just effective
      | trackingHit tracking effective (accessSize access) ->
          Just (Fact address (accessStore access) (Just effective) (accessSize access) (accessValues access) word)
    _ -> Nothing

-- | Sites whose instruction materializes a constant in [low, high): a MOVT
-- completing that upper halfword, or a PC-relative literal word load.
materializations :: Analysis -> Word32 -> Word32 -> [Word32]
materializations analysis low high = [site | (site, word) <- memoryWords memory, materializes site word]
 where
  memory = analysisMemory analysis
  materializes site word = case decode site word of
    MoveTop _ _ upper -> low `shiftR` 16 <= upper && upper <= high `shiftR` 16
    Load _ Word _ _ rn (OffsetImmediate n) OffsetOnly
      | rn == pc -> maybe False (\value -> low <= value && value < high) (memoryWord memory (site + 8 + fromIntegral n))
    _ -> False

literalSources :: Memory -> S.Set Word32
literalSources memory =
  S.fromList
    [ site + 8 + fromIntegral n
    | (site, word) <- memoryWords memory
    , Load _ Word _ _ rn (OffsetImmediate n) OffsetOnly <- [decode site word]
    , rn == pc
    ]

data Census = Census
  { censusFacts :: S.Set Fact
  , censusEscapes :: S.Set Escape
  , censusLimits :: S.Set Limit
  }

census :: Analysis -> Tracking -> (Word32 -> Bool) -> [Word32] -> Census
census analysis tracking excluded sites =
  Census
    (S.fromList [fact | walk <- walks, fact <- walkFacts walk, not (excluded (factSite fact))])
    (S.fromList [escape | walk <- walks, escape <- walkEscapes walk, not (excluded (escapeSite escape))])
    (S.fromList (concatMap walkLimits walks))
 where
  walks = map (forward analysis tracking) (filter (not . excluded) sites)

escapeSite :: Escape -> Word32
escapeSite escape = case escape of
  EscapeArgument site _ _ -> site
  EscapeStored site -> site
  EscapeReturned site _ -> site
  EscapeIndirect site -> site

-- * Obligations and refusals

data ArrayKind = StateArray | EventArray deriving (Eq, Ord)

data Obligation
  = CallerUnresolved Word32 [Word32] (S.Set Entry)
  | CallerOutsideTables Word32 Word32
  | DispatcherReference Word32 String
  | StoreValueUnresolved Word32 ArrayKind Word32 [Word32] (S.Set Entry)
  | IrregularArrayStore Word32 Word32 Word32
  | ComputedStore Word32 String
  | PointerEscape Word32 String
  | AnalysisLimit Word32 String
  | TableStore Word32 Word32
  | DataWordNames Word32 Word32 String
  | TableReadOutside Word32 Word32
  | TargetOutsideCode Word32 Word32
  | TargetWithoutEntryEvidence Word32 Word32
  | TargetInRetiredBody Word32 Word32
  | InitialArrayContents
  | UncoveredConstantForms

data Refusal
  = PublicationOldDiffers Word32 Word32 Word32
  | PublicationTouchesArrays Word32
  | UndeclaredTablePublication Word32 Word32
  | InterpositionUnpublished Word32
  | InterpositionValue Word32 Word32 Word32
  | BankFunctionMissing String
  | DispatcherBodyPublication Word32 Word32
  | RetainedServicePublication String Word32
  | InterpositionTableUnread Word32
  | RegionOverlap String String
  | OwnedDispatcherMissing

limitObligation :: Limit -> Obligation
limitObligation limit = case limit of
  StepLimit site -> AnalysisLimit site "step limit"
  WorldLimit site -> AnalysisLimit site "world limit"
  Unmodeled site -> AnalysisLimit site "unmodeled instruction with a tracked pointer live"
  LeavesMemory site -> AnalysisLimit site "control leaves the analysed regions with a tracked pointer live"
  PartialConstant site -> AnalysisLimit site "constant upper half reached with an unknown lower half"

escapeObligations :: Analysis -> [Escape] -> [Obligation]
escapeObligations analysis = go M.empty
 where
  go _ [] = []
  go memo (escape : rest) = case escape of
    EscapeArgument site (Just target) n ->
      let (live, memo') = liveIn analysis memo target 8
       in [PointerEscape site ("argument r" ++ show n ++ " to " ++ hex target) | S.member n live] ++ go memo' rest
    EscapeArgument site Nothing n -> PointerEscape site ("argument r" ++ show n ++ " to an indirect call") : go memo rest
    EscapeStored site -> PointerEscape site "stored to memory" : go memo rest
    EscapeReturned site n -> PointerEscape site ("live in r" ++ show n ++ " at a return") : go memo rest
    EscapeIndirect site -> PointerEscape site "live across an indirect jump" : go memo rest

-- * Declared objects and scalar consumers

-- | A data extent whose words source entry or table-name edges. Extent and
-- role are static-finding premises; the role predicate is checked against
-- the bytes.
data DeclaredObject = DeclaredObject
  { objectName :: String
  , objectStart :: Word32
  , objectEnd :: Word32
  , objectRole :: ObjectRole
  }

-- | (value, register) word pairs whose register words are 0xf0xxxxxx.
data ObjectRole = RegisterWriteStream

-- | The image-4 streams of E-MODAL-ENTRY-SOURCES-150.
declaredObjects :: [DeclaredObject]
declaredObjects =
  [ DeclaredObject "register-write-stream-a" 0x007a2430 0x007b0838 RegisterWriteStream
  , DeclaredObject "register-write-stream-b" 0x007b0840 0x007c2d18 RegisterWriteStream
  , DeclaredObject "register-write-stream-c" 0x0081c630 0x00820f10 RegisterWriteStream
  ]

-- | Membership under the `address & 0x3fffffff` alias classes.
objectNames :: DeclaredObject -> Word32 -> Bool
objectNames declared value = let address = value .&. 0x3fffffff in objectStart declared <= address && address < objectEnd declared

objectHolding :: Word32 -> Maybe DeclaredObject
objectHolding address = case [o | o <- declaredObjects, objectStart o <= address, address < objectEnd o] of
  declared : _ -> Just declared
  [] -> Nothing

data RoleCheck = RoleHolds | RoleFails Word32 | RoleUnread Word32

roleCheck :: Memory -> DeclaredObject -> RoleCheck
roleCheck memory declared = case objectRole declared of
  RegisterWriteStream
    | (objectEnd declared - objectStart declared) .&. 7 /= 0 || objectStart declared .&. 7 /= 0 -> RoleFails (objectStart declared)
    | otherwise -> foldr check RoleHolds [objectStart declared + 4, objectStart declared + 12 .. objectEnd declared - 4]
 where
  check address rest = case memoryWord memory address of
    Nothing -> RoleUnread address
    Just word
      | word `shiftR` 24 /= 0xf0 -> RoleFails address
      | otherwise -> rest

data ConsumerForm = WideConstant | LiteralLoad Word32 | AlignedWord

-- | A materialization of an address the object may hold, and its verdict.
data Consumer = Consumer
  { consumerSite :: Word32
  , consumerForm :: ConsumerForm
  , consumerValues :: Maybe [Word32]
  , consumerVerdict :: Verdict
  }

-- | Every path from the materialization reads the value only as a TST, TEQ,
-- CMP or CMN operand until it is overwritten or reaches a return in r2, r3
-- or r12.
data ScalarProof = ScalarProof
  { scalarCompares :: S.Set Word32
  , scalarOverwrites :: S.Set Word32
  , scalarReturns :: S.Set (Word32, Reg)
  , scalarStates :: Int
  }

-- | `ReturnConditional` holds only if no continuation of those returns reads
-- the register before writing it; no continuation is checked.
data Verdict = Discharged ScalarProof | ReturnConditional ScalarProof | Open Unproven

verdict :: Either Unproven ScalarProof -> Verdict
verdict result = case result of
  Left reason -> Open reason
  Right proof
    | S.null (scalarReturns proof) -> Discharged proof
    | otherwise -> ReturnConditional proof

data Unproven
  = AddressUse Word32
  | StoredUse Word32
  | ControlUse Word32
  | DerivedUse Word32
  | LiveAcrossCall Word32
  | LiveAtIndirectJump Word32
  | LiveAtReturn Word32 Reg
  | UnmodeledUse Word32
  | LeavesImages Word32
  | ScalarStepLimit Word32
  | HeldByObject

scalarStepLimit :: Int
scalarStepLimit = 50000

operandRegs :: Operand -> [Reg]
operandRegs operand = case operand of
  Immediate _ -> []
  ShiftedImmediate rm _ _ -> [rm]
  ShiftedRegister rm _ rs -> [rm, rs]

offsetRegs :: Offset -> [Reg]
offsetRegs offset = case offset of
  OffsetImmediate _ -> []
  OffsetRegister _ rm _ _ -> [rm]

-- | Registers an executed instruction reads; Nothing when the decoder does
-- not expose them.
instructionReads :: Instruction -> Maybe [Reg]
instructionReads instruction = case instruction of
  DataProcessing _ op _ _ rn operand -> Just ([rn | op `notElem` [MOV, MVN]] ++ operandRegs operand)
  MoveWide {} -> Just []
  MoveTop _ rd _ -> Just [rd]
  ZeroExtend _ _ rm _ _ -> Just [rm]
  Multiply _ _ rm rs accumulator -> Just ([rm, rs] ++ maybe [] pure accumulator)
  Load _ _ _ _ rn offset _ -> Just (rn : offsetRegs offset)
  Store _ width rt rn offset _ -> Just ([rt] ++ [nextReg rt | width == Dual] ++ rn : offsetRegs offset)
  LoadMultiple _ rn _ _ _ -> Just [rn]
  StoreMultiple _ rn _ _ list -> Just (rn : list)
  Branch {} -> Just []
  BranchExchange _ _ rm -> Just [rm]
  Hint _ -> Just []
  Compute _ _ -> Nothing
  Rejected _ -> Nothing

instructionWrites :: Instruction -> [Reg]
instructionWrites instruction = case instruction of
  DataProcessing _ op _ rd _ _ -> [rd | op `notElem` [TST, TEQ, CMP, CMN]]
  MoveWide _ rd _ -> [rd]
  MoveTop _ rd _ -> [rd]
  ZeroExtend _ rd _ _ _ -> [rd]
  Multiply _ rd _ _ _ -> [rd]
  Load _ width _ rt rn _ indexing -> [rt] ++ [nextReg rt | width == Dual] ++ [rn | indexing /= OffsetOnly]
  Store _ _ _ rn _ indexing -> [rn | indexing /= OffsetOnly]
  LoadMultiple _ rn _ writeback list -> list ++ [rn | writeback]
  StoreMultiple _ rn _ writeback _ -> [rn | writeback]
  _ -> []

-- | Why an executed instruction that reads `held` is not a scalar use.
nonScalarRead :: Word32 -> Instruction -> Reg -> Maybe Unproven
nonScalarRead address instruction held = case instruction of
  DataProcessing _ op _ rd _ _
    | op `elem` [TST, TEQ, CMP, CMN] -> Nothing
    | rd == pc -> Just (ControlUse address)
    | otherwise -> Just (DerivedUse address)
  Load {} -> Just (AddressUse address)
  LoadMultiple {} -> Just (AddressUse address)
  Store _ _ _ rn offset _
    | held `elem` (rn : offsetRegs offset) -> Just (AddressUse address)
    | otherwise -> Just (StoredUse address)
  StoreMultiple _ rn _ _ _
    | held == rn -> Just (AddressUse address)
    | otherwise -> Just (StoredUse address)
  BranchExchange {} -> Just (ControlUse address)
  _ -> Just (DerivedUse address)

-- | Follows the value `held` holds at `start` along every successor,
-- treating each conditional instruction as both executed and skipped.
scalarOnly :: Analysis -> Word32 -> Reg -> Either Unproven ScalarProof
scalarOnly analysis start register = go [(start, register)] S.empty (ScalarProof S.empty S.empty S.empty 0)
 where
  go [] _ proof = Right proof
  go ((address, held) : rest) seen proof
    | S.member (address, held) seen = go rest seen proof
    | scalarStates proof >= scalarStepLimit = Left (ScalarStepLimit address)
    | otherwise = case fetch analysis address of
        Nothing -> Left (LeavesImages address)
        Just (word, instruction) -> do
          (next, proof') <- step address word instruction held proof
          go (next ++ rest) (S.insert (address, held) seen) proof' {scalarStates = scalarStates proof' + 1}
  step address word instruction held proof = do
    readRegs <- maybe (Left (UnmodeledUse address)) Right (instructionReads instruction)
    compared <-
      if held `elem` readRegs
        then maybe (Right True) Left (nonScalarRead address instruction held)
        else Right False
    let skipped = [(address + 4, held) | instructionCond instruction /= Cond 14]
        killed = held `elem` instructionWrites instruction
        proof1 = if compared then proof {scalarCompares = S.insert address (scalarCompares proof)} else proof
        overwritten = Right (skipped, proof1 {scalarOverwrites = S.insert address (scalarOverwrites proof1)})
        continueAt targets
          | killed = overwritten
          | otherwise = Right (skipped ++ [(target, held) | target <- targets], proof1)
    case effectFlow (execute analysis address word instruction unknownRegs) of
      Continue -> continueAt [address + 4]
      Jump target -> continueAt [target]
      Switch targets -> continueAt targets
      Call _ -> Left (LiveAcrossCall address)
      Return
        | killed -> overwritten
        | held `elem` [Reg 2, Reg 3, Reg 12] -> Right (skipped, proof1 {scalarReturns = S.insert (address, held) (scalarReturns proof1)})
        | otherwise -> Left (LiveAtReturn address held)
      JumpIndirect
        | killed -> overwritten
        | otherwise -> Left (LiveAtIndirectJump address)

-- | Values `rd` holds after the MOVT at `site` executes, through up to three
-- predecessor joins; Nothing when a lower half stays unknown.
wideValues :: Analysis -> Word32 -> Cond -> Reg -> Word32 -> Maybe [Word32]
wideValues analysis site cond rd upper = case attempt (runWindow analysis (windowStart analysis site) site unknownWorlds) of
  Just values -> Just values
  Nothing -> attempt (fst (worldsAt analysis site 3))
 where
  attempt worlds =
    let values = [mapKnown (\old -> (upper `shiftL` 16) .|. (old .&. 0xffff)) (regAt regs rd) | (regs, flags) <- S.toList worlds, condPass cond flags]
     in if not (null values) && all isKnown values then Just (S.toList (S.fromList (mapMaybe knownValue values))) else Nothing

-- | MOVT and PC-relative literal materializations that may name the object,
-- each with its scalar verdict, and the other aligned words naming it.
objectConsumers :: Analysis -> S.Set Word32 -> DeclaredObject -> [Consumer]
objectConsumers analysis literals declared =
  [consumer | (site, word) <- memoryWords memory, consumer <- consumersAt site word]
 where
  memory = analysisMemory analysis
  lowUpper = objectStart declared `shiftR` 16
  highUpper = (objectEnd declared - 1) `shiftR` 16
  proveFrom site rd
    | rd == pc = Open (ControlUse site)
    | otherwise = verdict (scalarOnly analysis (site + 4) rd)
  consumersAt site word = case decode site word of
    MoveTop cond rd upper
      | lowUpper <= upper .&. 0x3fff && upper .&. 0x3fff <= highUpper ->
          let values = wideValues analysis site cond rd upper
           in [Consumer site WideConstant values (proveFrom site rd) | maybe True (any (objectNames declared)) values]
    Load _ Word _ rt rn (OffsetImmediate n) OffsetOnly
      | rn == pc
      , let literal = site + 8 + fromIntegral n
      , Just value <- memoryWord memory literal
      , objectNames declared value ->
          [Consumer site (LiteralLoad literal) (Just [value]) (proveFrom site rt)]
    _
      | objectNames declared word, not (S.member site literals) -> [Consumer site AlignedWord (Just [word]) (Open HeldByObject)]
      | otherwise -> []

-- * Callers

-- | Every reference to a dispatcher entry, or into the stock body after its
-- first word, from outside both dispatcher bodies.
dispatcherReferences :: Memory -> (Word32 -> Bool) -> [Word32] -> [Obligation]
dispatcherReferences memory inside entries =
  [DispatcherReference site (form "data word" word) | (site, word) <- outsideWords, named word]
    ++ [DispatcherReference site (form "movw/movt pair" value) | (site, value) <- widePairs, not (inside site), named value]
    ++ [ DispatcherReference site (form "pc-relative arithmetic" value)
       | (site, word) <- outsideWords
       , DataProcessing _ op _ _ rn (Immediate offset) <- [decode site word]
       , rn == pc
       , op == ADD || op == SUB
       , let value = if op == ADD then site + 8 + offset else site + 8 - offset
       , named value
       ]
    ++ [ DispatcherReference site (form "branch" target)
       | (site, word) <- outsideWords
       , bitsOf 27 25 word == 5
       , bitsOf 31 28 word /= 15
       , Branch _ _ target <- [decode site word]
       , interior target
       ]
    ++ [DispatcherReference site "thumb call" | site <- thumbCallers]
 where
  everyWord = memoryWords memory
  outsideWords = [(site, word) | (site, word) <- everyWord, not (inside site)]
  interior value = dispatcherEntry < value && value < dispatcherEnd
  named value = (value .&. complement 1) `elem` entries || interior (value .&. complement 1)
  form label value = label ++ (if interior (value .&. complement 1) then " into the retired body" else " to an entry")
  instructions = M.fromList [(site, decode site word) | (site, word) <- everyWord, isWide word]
  isWide word = word .&. 0x0ff00000 == 0x03000000 || word .&. 0x0ff00000 == 0x03400000
  widePairs =
    [ (site, (upper `shiftL` 16) .|. low)
    | (site, MoveWide _ rd low) <- M.toList instructions
    , Just upper <- [firstTop site rd]
    ]
  firstTop site rd =
    case [upper | k <- [1 .. 24], Just (MoveTop _ rd' upper) <- [M.lookup (site + 4 * k) instructions], rd' == rd] of
      upper : _ -> Just upper
      [] -> Nothing
  thumbCallers =
    [ address
    | region <- M.elems (memoryRegions memory)
    , offset <- [0, 2 .. B.length (regionBytes region) - 4]
    , let address = regionBase region + fromIntegral offset
    , Just firstHalf <- [memoryHalf memory address]
    , firstHalf `shiftR` 11 == 0x1e
    , Just secondHalf <- [memoryHalf memory (address + 2)]
    , Just target <- [thumbCallTarget address firstHalf secondHalf]
    , not (inside address)
    , named target
    ]

thumbCallTarget :: Word32 -> Word16 -> Word16 -> Maybe Word32
thumbCallTarget address firstHalf secondHalf
  | firstHalf `shiftR` 11 /= 0x1e || secondHalf `shiftR` 14 /= 3 = Nothing
  | testBit secondHalf 12 = Just (address + 4 + offset)
  | otherwise = Just (((address + 4) .&. complement 3) + (offset .&. complement 3))
 where
  s = testBit firstHalf 10
  i1 = testBit secondHalf 13 == s
  i2 = testBit secondHalf 11 == s
  raw =
    (if s then bit 24 else 0)
      .|. (if i1 then bit 23 else 0)
      .|. (if i2 then bit 22 else 0)
      .|. (fromIntegral (firstHalf .&. 0x3ff) `shiftL` 12)
      .|. (fromIntegral (secondHalf .&. 0x7ff) `shiftL` 1) :: Word32
  offset = if s then raw .|. 0xfe000000 else raw

-- * Controller closure

data Controller = Controller
  { controllerIndex :: Word32
  , controllerCellTable :: Word32
  , controllerActionTable :: Word32
  , controllerTransitionTable :: Word32
  , controllerWidth :: Word16
  , controllerEvents :: S.Set Word16
  , controllerStates :: S.Set Word16
  , controllerDerived :: S.Set Word16
  , controllerVisited :: S.Set Word16
  , controllerCells :: M.Map Word16 Word32
  , controllerActions :: M.Map Word16 Word32
  , controllerTransitions :: M.Map Word16 Word32
  , controllerIssues :: [Obligation]
  }

controllerClosure :: Memory -> Word32 -> S.Set Word16 -> S.Set Word16 -> Controller
controllerClosure memory index events states =
  case tables of
    Just (cells, actions, transitions, width)
      | cells == 0 -> base cells actions transitions width
      | otherwise -> grow (base cells actions transitions width)
    Nothing -> (base 0 0 0 0) {controllerIssues = [TableReadOutside index (cellPointers + 4 * index)]}
 where
  tables = do
    cells <- memoryWord memory (cellPointers + 4 * index)
    actions <- memoryWord memory (actionPointers + 4 * index)
    transitions <- memoryWord memory (transitionPointers + 4 * index)
    width <- memoryHalf memory (widthCells + 2 * index)
    pure (cells, actions, transitions, width)
  base cells actions transitions width =
    Controller index cells actions transitions width events states S.empty S.empty M.empty M.empty M.empty []
  grow controller
    | null fresh = controller
    | otherwise = grow (foldl' visit controller fresh)
   where
    fresh =
      S.toList
        ( S.fromList
            [ event * controllerWidth controller + state
            | event <- S.toList (controllerEvents controller)
            , state <- S.toList (controllerStates controller)
            ]
            `S.difference` controllerVisited controller
        )
  visit controller cellIndex =
    let address = controllerCellTable controller + 4 * fromIntegral cellIndex
        marked = controller {controllerVisited = S.insert cellIndex (controllerVisited controller)}
     in case memoryWord memory address of
          Nothing -> issue marked address
          Just cell ->
            let slot = fromIntegral cell :: Word16
                high = fromIntegral (cell `shiftR` 16) :: Int16
                withCell = marked {controllerCells = M.insert cellIndex cell (controllerCells marked)}
                withAction =
                  entry withCell (controllerActionTable controller) slot $ \c value ->
                    c {controllerActions = M.insert slot value (controllerActions c)}
             in if high > 0
                  then
                    let transitionSlot = fromIntegral high :: Word16
                     in entry withAction (controllerTransitionTable controller) transitionSlot $ \c value ->
                          c {controllerTransitions = M.insert transitionSlot value (controllerTransitions c)}
                  else
                    let next = fromIntegral (negate (fromIntegral high :: Int32)) :: Word16
                     in withAction
                          { controllerStates = S.insert next (controllerStates withAction)
                          , controllerDerived = S.insert next (controllerDerived withAction)
                          }
  entry controller table slot insert =
    let address = table + 4 * fromIntegral slot
     in maybe (issue controller address) (insert controller) (memoryWord memory address)
  issue controller address = controller {controllerIssues = TableReadOutside (controllerIndex controller) address : controllerIssues controller}

-- | Halfword addresses the dispatcher reads for these controllers, and the
-- halfword extents of every table those reads lie in.
data Protected = Protected
  { protectedEntries :: S.Set Word32
  , protectedObjects :: S.Set Word32
  }

halves :: Word32 -> Word32 -> [Word32]
halves start bytes = takeWhile (< start + bytes) [start, start + 2 ..]

protect :: [Controller] -> Protected
protect controllers = Protected (S.fromList (layout ++ entries)) (S.fromList (layout ++ objects))
 where
  count = controllerCount
  layout =
    halves cellPointers (4 * count)
      ++ halves actionPointers (4 * count)
      ++ halves transitionPointers (4 * count)
      ++ halves widthCells (2 * count)
  entries =
    concat
      [ concatMap (\slot -> halves (table + 4 * fromIntegral slot) 4) (M.keys slots)
      | controller <- controllers
      , (table, slots) <-
          [ (controllerCellTable controller, controllerCells controller)
          , (controllerActionTable controller, controllerActions controller)
          , (controllerTransitionTable controller, controllerTransitions controller)
          ]
      ]
  objects =
    concat
      [ halves table (4 * (fromIntegral (fst (M.findMax slots)) + 1))
      | controller <- controllers
      , (table, slots) <-
          [ (controllerCellTable controller, controllerCells controller)
          , (controllerActionTable controller, controllerActions controller)
          , (controllerTransitionTable controller, controllerTransitions controller)
          ]
      , not (M.null slots)
      ]

-- * Publications

data Overlay = Overlay
  { overlayMemory :: Memory
  , overlayRefusals :: [Refusal]
  , overlayHook :: Maybe Word32
  , overlayInterpositions :: [(Word32, Word32, Word32)]
  , overlayOutside :: Int
  }

overlay :: Memory -> M.Map String BankFunction -> Maybe BankFunction -> [Publication] -> Overlay
overlay stock functions owned publications =
  Overlay
    (stock {memoryPatches = M.fromList [(publicationAddress p, publicationNew p) | p <- inside]})
    (oldRefusals ++ arrayRefusals ++ bodyRefusals ++ retainedRefusals ++ interpositionRefusals)
    hookWord
    [(site, old, new) | Right (site, (old, new)) <- interpositionChecks]
    (length publications - length inside)
 where
  inside = [p | p <- publications, isJust (originalWord stock (publicationAddress p))]
  byAddress = M.fromList [(publicationAddress p, p) | p <- publications]
  oldRefusals =
    [ PublicationOldDiffers (publicationAddress p) (publicationOld p) old
    | p <- inside
    , Just old <- [originalWord stock (publicationAddress p)]
    , old /= publicationOld p
    ]
  arrayRefusals =
    [ PublicationTouchesArrays (publicationAddress p)
    | p <- publications
    , toInteger (publicationAddress p) + 4 > toInteger stateCells
    , publicationAddress p < arraysEnd
    ]
  hookWord = do
    function <- owned
    p <- M.lookup dispatcherEntry byAddress
    case decode dispatcherEntry (publicationNew p) of
      Branch (Cond 14) False target | target == bankFunctionAddress function -> Just (publicationNew p)
      _ -> Nothing
  bodyRefusals =
    [ DispatcherBodyPublication (publicationAddress p) (publicationNew p)
    | p <- publications
    , dispatcherEntry <= publicationAddress p
    , publicationAddress p < dispatcherEnd
    , not (publicationAddress p == dispatcherEntry && isJust hookWord)
    ]
  retainedRefusals =
    [ RetainedServicePublication name (publicationAddress p)
    | p <- publications
    , (name, start, end) <- retainedServices
    , start <= publicationAddress p
    , publicationAddress p < end
    ]
  interpositionChecks = map check existingInterpositions
  check interposition = case interpositionSite stock interposition of
    Nothing -> Left (InterpositionTableUnread (interpositionController interposition))
    Just site -> case (M.lookup site byAddress, expected interposition site) of
      (_, Left refusal) -> Left refusal
      (Nothing, _) -> Left (InterpositionUnpublished site)
      (Just p, Right want)
        | publicationNew p == want -> Right (site, (publicationOld p, want))
        | otherwise -> Left (InterpositionValue site want (publicationNew p))
  interpositionRefusals = [refusal | Left refusal <- interpositionChecks]
  expected interposition site = case interposition of
    ActionEntry _ _ name -> maybe (Left (BankFunctionMissing name)) (Right . bankFunctionAddress) (M.lookup name functions)
    CellAction _ _ (ActionSlot slot) -> case originalWord stock site of
      Just old -> Right ((old .&. 0xffff0000) .|. fromIntegral slot)
      Nothing -> Left (InterpositionUnpublished site)

interpositionController :: Interposition -> Word32
interpositionController interposition = case interposition of
  ActionEntry (ControllerId index) _ _ -> index
  CellAction (ControllerId index) _ _ -> index

interpositionSite :: Memory -> Interposition -> Maybe Word32
interpositionSite memory interposition = case interposition of
  ActionEntry (ControllerId index) (ActionSlot slot) _ -> do
    table <- originalWord memory (actionPointers + 4 * index)
    pure (table + 4 * fromIntegral slot)
  CellAction (ControllerId index) (CellIndex cell) _ -> do
    table <- originalWord memory (cellPointers + 4 * index)
    pure (table + 4 * fromIntegral cell)

-- * Proof

data ArrayWrite = ArrayWrite
  { arrayWriteSite :: Word32
  , arrayWriteKind :: ArrayKind
  , arrayWriteController :: Word32
  , arrayWriteValue :: Word16
  }

arraySlot :: Word32 -> Maybe (ArrayKind, Word32)
arraySlot address
  | stateCells <= address && address < eventCells = Just (StateArray, (address - stateCells) `div` 2)
  | eventCells <= address && address < arraysEnd = Just (EventArray, (address - eventCells) `div` 2)
  | otherwise = Nothing

arrayWrites :: Analysis -> Fact -> [Either Obligation ArrayWrite]
arrayWrites analysis fact = case (factAddress fact, factSize fact, factValues fact) of
  (Nothing, _, _) -> [Left (ComputedStore site "state/event arrays")]
  (Just address, 2, [value])
    | even address, Just (kind, controller) <- arraySlot address -> resolved kind controller value
  (Just address, 4, [Known value])
    | even address ->
        [ Right (ArrayWrite site kind controller (fromIntegral (value `shiftR` (16 * k))))
        | k <- [0, 1 :: Int]
        , Just (kind, controller) <- [arraySlot (address + 2 * fromIntegral k)]
        ]
  (Just address, size, _) -> [Left (IrregularArrayStore site address size)]
 where
  site = factSite fact
  resolved kind controller (Known value) = [Right (ArrayWrite site kind controller (fromIntegral value))]
  resolved kind controller _ =
    let register = Reg (fromIntegral (bitsOf 15 12 (factWord fact)))
     in case resolveValues analysis site register (instructionCond (decode site (factWord fact))) 4 of
          Right values -> [Right (ArrayWrite site kind controller (fromIntegral value)) | value <- values]
          Left (knowns, entries) -> [Left (StoreValueUnresolved site kind controller knowns entries)]

data Target = RetainedTarget Word32 | OwnedTarget String Word32 deriving (Eq, Ord)

buildProof :: Inputs -> Proof
buildProof inputs =
  Proof
    (null obligations && null refusals)
    ( object
        [ "schema" .= ("modal-dispatcher-closure-1" :: String)
        , "admitted" .= (null obligations && null refusals)
        , "dispatcher"
            .= object
              [ "stock_entry" .= hex dispatcherEntry
              , "stock_end" .= hex dispatcherEnd
              , "owned_entry" .= fmap (hex . bankFunctionAddress) owned
              , "owned_size" .= fmap bankFunctionSize owned
              , "hook" .= maybe ("absent" :: String) (const "published") (overlayHook published)
              , "controller_count" .= controllerCount
              ]
        , "domain"
            .= object
              [ "regions"
                  .= [ object
                        [ "name" .= regionName region
                        , "kind" .= show (regionKind region)
                        , "base" .= hex (regionBase region)
                        , "size" .= B.length (regionBytes region)
                        ]
                     | region <- inputRegions inputs
                     ]
              , "publications" .= length (inputPublications inputs)
              , "publications_outside_regions" .= overlayOutside published
              , "provenance"
                  .= ( "pointers into the state/event arrays derive from constants at or above "
                        ++ hex bssStart
                        ++ " or from aligned data words; pointers into dispatcher tables derive from constants within 4 KiB of a table or from aligned data words" ::
                        String
                     )
              ]
        , "callers"
            .= object
              [ "sites" .= length callers
              , "controllers" .= [object ["controller" .= controller, "sites" .= sites] | (controller, sites) <- M.toList siteCounts]
              ]
        , "census"
            .= object
              [ "array_materializations" .= length arraySites
              , "table_materializations" .= length tableSites
              , "state_writes" .= length [() | w <- writes, arrayWriteKind w == StateArray]
              , "event_writes" .= length [() | w <- writes, arrayWriteKind w == EventArray]
              , "writer_sites" .= S.size (S.fromList (map arrayWriteSite writes))
              ]
        , "controllers" .= map controllerValue closures
        , "admitted_targets"
            .= object
              [ "retained" .= [hex address | RetainedTarget address <- S.toList targets]
              , "owned" .= [object ["name" .= name, "address" .= hex address] | OwnedTarget name address <- S.toList targets]
              ]
        , "retained"
            .= object
              [ "executable"
                  .= [object ["name" .= name, "start" .= hex start, "end" .= hex end] | (name, start, end) <- retainedServices]
              , "reference_only"
                  .= [object ["name" .= ("stock-dispatcher-body" :: String), "start" .= hex dispatcherEntry, "end" .= hex dispatcherEnd]]
              , "data"
                  .= [ object ["name" .= name, "start" .= hex start, "bytes" .= bytes]
                     | (name, start, bytes) <-
                        [ ("cell-pointers" :: String, cellPointers, 4 * controllerCount)
                        , ("action-pointers", actionPointers, 4 * controllerCount)
                        , ("transition-pointers", transitionPointers, 4 * controllerCount)
                        , ("widths", widthCells, 2 * controllerCount)
                        , ("states", stateCells, 2 * controllerCount)
                        , ("events", eventCells, 2 * controllerCount)
                        ]
                     ]
              ]
        , "interpositions" .= [object ["site" .= hex site, "old" .= hex old, "new" .= hex new] | (site, old, new) <- overlayInterpositions published]
        , "declared_objects"
            .= object
              [ "rule"
                  .= ( "a consumer is discharged when every path from its materialization reads the value only as a TST, TEQ, CMP or CMN operand until it is overwritten. It is return-conditional when some path instead reaches a return with the value in r2, r3 or r12: it then rests on the unverified premise that no continuation of those returns reads the register before writing it, and is not a discharge. Any other read, a call, an indirect jump, an unmodeled instruction or leaving the images while the value is live, a return with it in any other register, and every aligned word naming the object, stays open. Consumers that index a base into the extent and constants the census does not trace are not enumerated" ::
                        String
                     )
              , "objects" .= [objectValue memory declared (objectConsumers analysis literals declared) | declared <- declaredObjects]
              ]
        , "obligations" .= map obligationValue obligations
        , "refusals" .= map refusalValue refusals
        ]
    )
 where
  stockMemory = Memory (M.fromList [(regionBase region, region) | region <- inputRegions inputs]) M.empty
  overlaps =
    [ RegionOverlap (regionName earlier) (regionName later)
    | (earlier, later) <- zip ordered (drop 1 ordered)
    , regionEnd earlier > toInteger (regionBase later)
    ]
   where
    ordered = sortOn regionBase (inputRegions inputs)
  functions = M.fromList [(bankFunctionName f, f) | f <- inputBankFunctions inputs]
  functionsByAddress = M.fromList [(bankFunctionAddress f, bankFunctionName f) | f <- inputBankFunctions inputs]
  owned = M.lookup ownedDispatcherName functions
  published = overlay stockMemory functions owned (inputPublications inputs)
  memory = overlayMemory published
  analysis = analyse memory
  insideDispatcher site =
    (dispatcherEntry <= site && site < dispatcherEnd)
      || maybe False (\f -> bankFunctionAddress f <= site && site < bankFunctionAddress f + bankFunctionSize f) owned
  entries = dispatcherEntry : maybe [] (pure . bankFunctionAddress) owned
  callers =
    [ (site, instruction)
    | (site, word) <- memoryWords memory
    , bitsOf 27 25 word == 5
    , bitsOf 31 28 word /= 15
    , let instruction = decode site word
    , Branch _ _ target <- [instruction]
    , target `elem` entries
    , not (insideDispatcher site)
    ]
  callerResults = [(site, resolveValues analysis site (Reg 0) (instructionCond instruction) 3) | (site, instruction) <- callers]
  callerObligations =
    concat
      [ case result of
          Right values -> [CallerOutsideTables site value | value <- values, value >= controllerCount]
          Left (knowns, why) -> [CallerUnresolved site knowns why]
      | (site, result) <- callerResults
      ]
  calledControllers = S.fromList [value | (_, Right values) <- callerResults, value <- values, value < controllerCount]
  siteCounts = M.fromListWith (+) [(value, 1 :: Int) | (_, Right values) <- callerResults, value <- values, value < controllerCount]
  arrayTracking =
    Tracking
      { trackingTracked = \value -> bssStart <= value && value < arraysEnd + 0x1000
      , trackingPointer = \value -> stateCells <= value && value < arraysEnd
      , trackingHit = \address size -> address < arraysEnd && toInteger address + toInteger size > toInteger stateCells
      }
  arraySites = materializations analysis bssStart (arraysEnd + 0x1000)
  arrayCensus = census analysis arrayTracking insideDispatcher arraySites
  arrayOutcomes = concatMap (arrayWrites analysis) [fact | fact <- S.toList (censusFacts arrayCensus), factStore fact]
  writes = [w | Right w <- arrayOutcomes]
  valuesOf kind controller = S.fromList [arrayWriteValue w | w <- writes, arrayWriteKind w == kind, arrayWriteController w == controller]
  closureOver mem = [controllerClosure mem controller (valuesOf EventArray controller) (valuesOf StateArray controller) | controller <- S.toList calledControllers]
  closures = closureOver memory
  stockClosures = closureOver stockMemory
  protectedSet = protect (closures ++ stockClosures)
  tableLow = S.findMin (protectedEntries protectedSet) - 0x1000
  tableHigh = S.findMax (protectedEntries protectedSet) + 0x1004
  tableTracking =
    Tracking
      { trackingTracked = \value -> tableLow <= value && value < tableHigh
      , trackingPointer = \value -> S.member (value .&. complement 1) (protectedObjects protectedSet)
      , trackingHit = \address size ->
          any (`S.member` protectedEntries protectedSet) (takeWhile (< address + size) [address .&. complement 1, (address .&. complement 1) + 2 ..])
      }
  tableSites = materializations analysis tableLow tableHigh
  tableCensus = census analysis tableTracking insideDispatcher tableSites
  literals = literalSources memory
  pointerTableWord site =
    any (\table -> table <= site && site < table + 4 * controllerCount) [cellPointers, actionPointers, transitionPointers]
  dataWords =
    [ DataWordNames site word label
    | (site, word) <- memoryWords memory
    , not (S.member site literals)
    , not (pointerTableWord site)
    , Just label <- [nameOf word]
    ]
  nameOf word
    | stateCells - 0x100 <= word && word < arraysEnd = Just "state/event arrays"
    | S.member (word .&. complement 1) (protectedObjects protectedSet) = Just "dispatcher table"
    | otherwise = Nothing
  tableObligations =
    [TableStore (factSite fact) address | fact <- S.toList (censusFacts tableCensus), factStore fact, Just address <- [factAddress fact]]
      ++ [ComputedStore (factSite fact) "dispatcher table" | fact <- S.toList (censusFacts tableCensus), factStore fact, factAddress fact == Nothing]
  classify controller target
    | target == 0 = Nothing
    | Just name <- M.lookup target functionsByAddress = Just (Right (OwnedTarget name target))
    | dispatcherEntry <= target && target < dispatcherEnd = Just (Left (TargetInRetiredBody controller target))
    | target .&. 3 == 0
    , Just region <- regionAt memory target
    , regionKind region == StockImage
    , Just (_, instruction) <- fetch analysis target
    , framePrologue instruction =
        Just (Right (RetainedTarget target))
    | target .&. 3 == 0, Just region <- regionAt memory target, regionKind region == StockImage = Just (Left (TargetWithoutEntryEvidence controller target))
    | otherwise = Just (Left (TargetOutsideCode controller target))
  classified =
    [ outcome
    | controller <- closures
    , target <- M.elems (controllerActions controller) ++ M.elems (controllerTransitions controller)
    , Just outcome <- [classify (controllerIndex controller) target]
    ]
  targets = S.fromList [target | Right target <- classified]
  tableRefusals =
    [ UndeclaredTablePublication (publicationAddress p) (publicationNew p)
    | p <- inputPublications inputs
    , any (`S.member` protectedObjects protectedSet) [publicationAddress p, publicationAddress p + 2]
    , publicationAddress p `notElem` [site | (site, _, _) <- overlayInterpositions published]
    ]
  refusals =
    overlaps
      ++ overlayRefusals published
      ++ tableRefusals
      ++ [OwnedDispatcherMissing | null owned]
  obligations =
    dispatcherReferences memory insideDispatcher entries
      ++ callerObligations
      ++ [o | Left o <- arrayOutcomes]
      ++ escapeObligations analysis (S.toList (censusEscapes arrayCensus))
      ++ map limitObligation (S.toList (censusLimits arrayCensus))
      ++ tableObligations
      ++ escapeObligations analysis (S.toList (censusEscapes tableCensus))
      ++ map limitObligation (S.toList (censusLimits tableCensus))
      ++ dataWords
      ++ concatMap controllerIssues closures
      ++ [o | Left o <- classified]
      ++ [InitialArrayContents, UncoveredConstantForms]

-- | The two stock function openings: a push that saves lr, or a single
-- register stored to the pre-decremented sp.
framePrologue :: Instruction -> Bool
framePrologue instruction = case instruction of
  StoreMultiple (Cond 14) rn DecrementBefore True list -> rn == sp && lr `elem` list
  Store (Cond 14) Word _ rn (OffsetImmediate (-4)) PreIndexed -> rn == sp
  _ -> False

controllerValue :: Controller -> Value
controllerValue controller =
  object
    [ "controller" .= controllerIndex controller
    , "cell_table" .= hex (controllerCellTable controller)
    , "action_table" .= hex (controllerActionTable controller)
    , "transition_table" .= hex (controllerTransitionTable controller)
    , "width" .= controllerWidth controller
    , "events" .= S.toList (controllerEvents controller)
    , "states" .= S.toList (controllerStates controller)
    , "derived_state_writes" .= S.toList (controllerDerived controller)
    , "cells" .= M.size (controllerCells controller)
    , "action_slots" .= [object ["slot" .= slot, "target" .= hex target] | (slot, target) <- M.toList (controllerActions controller)]
    , "transition_slots" .= [object ["slot" .= slot, "target" .= hex target] | (slot, target) <- M.toList (controllerTransitions controller)]
    ]

entryValue :: Entry -> Value
entryValue entry = case entry of
  DataWordEntry address origins ->
    object
      [ "data_word_entry" .= hex address
      , "origins" .= map hex origins
      , "declared_objects" .= S.toList (S.fromList [objectName o | Just o <- map objectHolding origins])
      ]
  CallEntry address site -> object ["call_entry" .= hex address, "site" .= hex site]
  DepthLimit address -> object ["depth_limit" .= hex address]
  WindowLimit address -> object ["window_limit" .= hex address]
  UnreachedEntry address -> object ["unreached_entry" .= hex address]

arrayName :: ArrayKind -> String
arrayName StateArray = "state"
arrayName EventArray = "event"

obligationValue :: Obligation -> Value
obligationValue obligation = case obligation of
  CallerUnresolved site knowns why ->
    object ["kind" .= ("caller-controller-unresolved" :: String), "site" .= hex site, "known" .= knowns, "entries" .= map entryValue (S.toList why)]
  CallerOutsideTables site value ->
    object ["kind" .= ("caller-controller-outside-tables" :: String), "site" .= hex site, "controller" .= value]
  DispatcherReference site form ->
    object ["kind" .= ("computed-dispatcher-reference" :: String), "site" .= hex site, "form" .= form]
  StoreValueUnresolved site kind controller knowns why ->
    object
      [ "kind" .= ("store-value-unresolved" :: String)
      , "site" .= hex site
      , "array" .= arrayName kind
      , "controller" .= controller
      , "known" .= knowns
      , "entries" .= map entryValue (S.toList why)
      ]
  IrregularArrayStore site address size ->
    object ["kind" .= ("irregular-array-store" :: String), "site" .= hex site, "address" .= hex address, "bytes" .= size]
  ComputedStore site into ->
    object ["kind" .= ("computed-index-store" :: String), "site" .= hex site, "into" .= into]
  PointerEscape site how ->
    object ["kind" .= ("pointer-escape" :: String), "site" .= hex site, "how" .= how]
  AnalysisLimit site why ->
    object ["kind" .= ("analysis-limit" :: String), "site" .= hex site, "reason" .= why]
  TableStore site address ->
    object ["kind" .= ("table-store" :: String), "site" .= hex site, "address" .= hex address]
  DataWordNames site word label ->
    object
      ( ["kind" .= ("data-word-reference" :: String), "location" .= hex site, "value" .= hex word, "names" .= label]
          ++ ["declared_object" .= objectName o | Just o <- [objectHolding site]]
      )
  TableReadOutside controller address ->
    object ["kind" .= ("table-read-outside-images" :: String), "controller" .= controller, "address" .= hex address]
  TargetOutsideCode controller target ->
    object ["kind" .= ("target-outside-code" :: String), "controller" .= controller, "target" .= hex target]
  TargetWithoutEntryEvidence controller target ->
    object ["kind" .= ("target-without-entry-evidence" :: String), "controller" .= controller, "target" .= hex target]
  TargetInRetiredBody controller target ->
    object ["kind" .= ("target-in-retired-body" :: String), "controller" .= controller, "target" .= hex target]
  UncoveredConstantForms ->
    object
      [ "kind" .= ("uncovered-constant-forms" :: String)
      , "detail" .= ("pointers into the arrays or tables are traced only from MOVT and PC-relative literal constants and aligned image words; MOV/MVN/ORR or rotated immediates, arithmetic construction and pointers loaded from RAM are not traced" :: String)
      ]
  InitialArrayContents ->
    object
      [ "kind" .= ("initial-array-contents" :: String)
      , "start" .= hex stateCells
      , "end" .= hex arraysEnd
      , "detail" .= ("no static writer or clear establishes the halfwords before the first dispatch; the closure covers only written values" :: String)
      ]

refusalValue :: Refusal -> Value
refusalValue refusal = case refusal of
  PublicationOldDiffers site declared actual ->
    object ["kind" .= ("publication-old-word-differs" :: String), "site" .= hex site, "declared" .= hex declared, "image" .= hex actual]
  PublicationTouchesArrays site ->
    object ["kind" .= ("publication-in-state-event-arrays" :: String), "site" .= hex site]
  UndeclaredTablePublication site new ->
    object ["kind" .= ("undeclared-table-publication" :: String), "site" .= hex site, "new" .= hex new]
  InterpositionUnpublished site ->
    object ["kind" .= ("interposition-unpublished" :: String), "site" .= hex site]
  InterpositionValue site want actual ->
    object ["kind" .= ("interposition-value" :: String), "site" .= hex site, "expected" .= hex want, "published" .= hex actual]
  BankFunctionMissing name ->
    object ["kind" .= ("bank-function-missing" :: String), "name" .= name]
  DispatcherBodyPublication site new ->
    object ["kind" .= ("dispatcher-body-publication" :: String), "site" .= hex site, "new" .= hex new]
  RetainedServicePublication name site ->
    object ["kind" .= ("retained-service-publication" :: String), "service" .= name, "site" .= hex site]
  InterpositionTableUnread controller ->
    object ["kind" .= ("interposition-table-unread" :: String), "controller" .= controller]
  RegionOverlap first second ->
    object ["kind" .= ("region-overlap" :: String), "first" .= first, "second" .= second]
  OwnedDispatcherMissing ->
    object ["kind" .= ("owned-dispatcher-missing" :: String), "symbol" .= ownedDispatcherName]

objectValue :: Memory -> DeclaredObject -> [Consumer] -> Value
objectValue memory declared consumers =
  object
    [ "name" .= objectName declared
    , "start" .= hex (objectStart declared)
    , "end" .= hex (objectEnd declared)
    , "evidence" .= ("conditional" :: String)
    , "role" .= case objectRole declared of
        RegisterWriteStream -> ("register-write-stream" :: String)
    , "role_check" .= case roleCheck memory declared of
        RoleHolds -> object ["status" .= ("holds" :: String)]
        RoleFails address -> object ["status" .= ("fails" :: String), "at" .= hex address]
        RoleUnread address -> object ["status" .= ("unread" :: String), "at" .= hex address]
    , "discharged_consumers" .= length [() | Consumer {consumerVerdict = Discharged _} <- consumers]
    , "return_conditional_consumers" .= length [() | Consumer {consumerVerdict = ReturnConditional _} <- consumers]
    , "open_consumers" .= length [() | Consumer {consumerVerdict = Open _} <- consumers]
    , "consumers" .= map consumerValue consumers
    ]

consumerValue :: Consumer -> Value
consumerValue consumer =
  object
    ( [ "site" .= hex (consumerSite consumer)
      , "form" .= case consumerForm consumer of
          WideConstant -> ("movt" :: String)
          LiteralLoad _ -> "literal-load"
          AlignedWord -> "aligned-word"
      , "values" .= maybe ["unresolved" :: String] (map hex) (consumerValues consumer)
      ]
        ++ ["literal" .= hex literal | LiteralLoad literal <- [consumerForm consumer]]
        ++ case consumerVerdict consumer of
          Discharged proof -> ["status" .= ("discharged" :: String), "proof" .= proofValueOf proof]
          ReturnConditional proof ->
            [ "status" .= ("return-conditional" :: String)
            , "premise"
                .= object
                  [ "kind" .= ("return-continuation-does-not-read" :: String)
                  , "returns" .= [object ["at" .= hex site, "register" .= regName register] | (site, register) <- S.toList (scalarReturns proof)]
                  ]
            , "proof" .= proofValueOf proof
            ]
          Open reason -> ["status" .= ("open" :: String), "reason" .= unprovenValue reason]
    )
 where
  proofValueOf proof =
    object
      [ "use" .= ("scalar-compare" :: String)
      , "compares" .= map hex (S.toList (scalarCompares proof))
      , "overwritten" .= map hex (S.toList (scalarOverwrites proof))
      , "states" .= scalarStates proof
      ]

unprovenValue :: Unproven -> Value
unprovenValue reason = case reason of
  AddressUse site -> at "address-use" site
  StoredUse site -> at "stored-use" site
  ControlUse site -> at "control-use" site
  DerivedUse site -> at "derived-use" site
  LiveAcrossCall site -> at "live-across-call" site
  LiveAtIndirectJump site -> at "live-at-indirect-jump" site
  LiveAtReturn site register -> object ["kind" .= ("live-at-return" :: String), "at" .= hex site, "register" .= regName register]
  UnmodeledUse site -> at "unmodeled-instruction" site
  LeavesImages site -> at "leaves-images" site
  ScalarStepLimit site -> at "step-limit" site
  HeldByObject -> object ["kind" .= ("held-by-object" :: String)]
 where
  at kind site = object ["kind" .= (kind :: String), "at" .= hex site]

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
