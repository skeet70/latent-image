-- | Bounded A32 interpretation of a linked polling loop: core instructions
-- entered from one arrival at the post-load site to the next while the polled
-- cell keeps its waiting value.
module Emulation.PollPeriod
  ( PollPeriod (..)
  , PollModel (..)
  , CodeImage (..)
  , imageWord
  , pollPeriod
  ) where

import A32 (BlockMode (..), Cond (..), DataOp (..), Forbidden (Barrier), Indexing (..), Instruction (..), Offset (..), Operand (..), Reg (..), Shift (..), Width (..))
import qualified A32
import Control.Monad (foldM, unless, when)
import Data.Bits (bit, complement, rotateR, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Emulation.FirstStageWitness (leWordAt)
import Numeric (showHex)

-- | Core instructions entered from one arrival at the post-load site to the next.
newtype PollPeriod = PollPeriod Word64 deriving (Eq, Show)

-- | The polled cell and its waiting value, the half-open stack window the
-- loop may spill into, and the registers known on entry.
data PollModel = PollModel
  { pollCell :: Word32
  , pollWaiting :: Word32
  , pollStack :: (Word32, Word32)
  , pollRegisters :: M.Map Int Word32
  }

-- | Linked bytes and the guest address of their first byte.
data CodeImage = CodeImage Word32 B.ByteString

imageWord :: CodeImage -> Word32 -> Maybe Word32
imageWord (CodeImage base bytes) location
  | location < base || location .&. 3 /= 0 = Nothing
  | otherwise = either (const Nothing) Just (leWordAt bytes (fromIntegral (location - base)))

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

-- | 'Nothing' is an incoming flag or one computed from an unknown value.
data Flags = Flags {flagN :: Maybe Bool, flagZ :: Maybe Bool, flagC :: Maybe Bool, flagV :: Maybe Bool}

-- | An absent register, or a 'Nothing' stack word, holds an incoming or derived unknown value.
data Machine = Machine
  { machinePc :: Word32
  , machineRegisters :: M.Map Int Word32
  , machineFlags :: Flags
  , machineStack :: M.Map Word32 (Maybe Word32)
  }

maxPollSteps :: Int
maxPollSteps = 100000

-- | Interprets from @start@ until the second arrival at @loaded@.
pollPeriod :: PollModel -> CodeImage -> Word32 -> Word32 -> Either String PollPeriod
pollPeriod model code start loaded = go 0 Nothing initial
  where
    initial = Machine start (pollRegisters model) (Flags Nothing Nothing Nothing Nothing) M.empty
    go steps counted machine = do
      let here = machinePc machine
      case counted of
        Just total | here == loaded -> Right (PollPeriod total)
        _ -> do
          require (steps < maxPollSteps) "poll-period interpretation exceeded 100000 steps"
          next <- execute model code machine
          let counting = case counted of
                Just total -> Just (total + 1)
                Nothing | here == loaded -> Just 1
                Nothing -> Nothing
          go (steps + 1 :: Int) counting next

slot :: Machine -> Int -> Maybe Word32
slot machine number = M.lookup number (machineRegisters machine)

setSlot :: Int -> Maybe Word32 -> Machine -> Machine
setSlot number value machine = machine {machineRegisters = M.alter (const value) number (machineRegisters machine)}

regNumber :: Reg -> Int
regNumber (Reg number) = number

unsupportedAt :: Word32 -> Either String a
unsupportedAt location = Left ("poll-period interpreter does not support the instruction at " ++ hex location)

known :: Word32 -> Maybe a -> Either String a
known location = maybe (Left ("poll-period control flow or address depends on an unknown incoming value at " ++ hex location)) Right

conditionOf :: Instruction -> Maybe Cond
conditionOf instruction = case instruction of
  DataProcessing condition _ _ _ _ _ -> Just condition
  MoveWide condition _ _ -> Just condition
  MoveTop condition _ _ -> Just condition
  Load condition _ _ _ _ _ _ -> Just condition
  Store condition _ _ _ _ _ -> Just condition
  LoadMultiple condition _ _ _ _ -> Just condition
  StoreMultiple condition _ _ _ _ -> Just condition
  Branch condition _ _ -> Just condition
  BranchExchange condition _ _ -> Just condition
  Hint condition -> Just condition
  _ -> Nothing

holds :: Flags -> Cond -> Maybe Bool
holds flags (Cond condition) = case condition of
  0 -> z
  1 -> not <$> z
  2 -> c
  3 -> not <$> c
  4 -> n
  5 -> not <$> n
  6 -> v
  7 -> not <$> v
  8 -> (&&) <$> c <*> (not <$> z)
  9 -> (||) <$> (not <$> c) <*> z
  10 -> (==) <$> n <*> v
  11 -> (/=) <$> n <*> v
  12 -> (&&) <$> (not <$> z) <*> ((==) <$> n <*> v)
  13 -> (||) <$> z <*> ((/=) <$> n <*> v)
  _ -> Just (condition == 14)
  where
    n = flagN flags
    z = flagZ flags
    c = flagC flags
    v = flagV flags

execute :: PollModel -> CodeImage -> Machine -> Either String Machine
execute model code machine = do
  let here = machinePc machine
  word <- maybe (Left ("poll-period fetch outside the candidate image at " ++ hex here)) Right (imageWord code here)
  let instruction = A32.decode here word
      advance state = state {machinePc = here + 4}
      readReg r = if r == A32.pc then Just (here + 8) else slot machine (regNumber r)
      carry = flagC (machineFlags machine)
      linked link = if link then setSlot 14 (Just (here + 4)) machine else machine
      branchTo destination state
        | destination .&. 3 /= 0 = unsupportedAt here
        | otherwise = Right state {machinePc = destination}
      transferAddress rn offset indexing = do
        displacement <- case offset of
          OffsetImmediate value -> Right (fromIntegral value)
          OffsetRegister up rm kind amount
            | rm == A32.pc -> unsupportedAt here
            | otherwise -> (\shifted -> if up then shifted else negate shifted) <$> known here (fst (shiftImmediateKnown kind amount (readReg rm) carry))
        base <- known here (readReg rn)
        let effective = base + displacement
        pure (if indexing == PostIndexed then base else effective, effective)
      blockAddresses base mode list =
        let size = 4 * fromIntegral (length list)
            lowest = case mode of
              IncrementAfter -> base
              IncrementBefore -> base + 4
              DecrementAfter -> base - size + 4
              DecrementBefore -> base - size
            final = if mode == IncrementAfter || mode == IncrementBefore then base + size else base - size
         in ([lowest + 4 * fromIntegral index | index <- [0 .. length list - 1]], final)
  case conditionOf instruction of
    Nothing -> case instruction of
      Rejected Barrier -> Right (advance machine)
      _ -> unsupportedAt here
    Just condition -> do
      taken <- known here (holds (machineFlags machine) condition)
      if not taken then Right (advance machine) else case instruction of
        DataProcessing _ op setFlags rd rn shifter -> do
          (operand2, shifterCarry) <- case shifter of
            Immediate value -> Right (Just value, if (word `shiftR` 8) .&. 0xf == 0 then carry else Just (testBit value 31))
            ShiftedImmediate rm kind amount -> Right (shiftImmediateKnown kind amount (readReg rm) carry)
            ShiftedRegister rm kind rs
              | rm == A32.pc || rs == A32.pc -> unsupportedAt here
              | otherwise -> Right (shiftRegisterKnown kind (readReg rs) (readReg rm) carry)
          let (result, flags) = alu op (readReg rn) operand2 shifterCarry (machineFlags machine)
              writes = op `notElem` [TST, TEQ, CMP, CMN]
          when (writes && rd == A32.pc) (unsupportedAt here)
          let written = if writes then setSlot (regNumber rd) result machine else machine
          Right (advance (if setFlags then written {machineFlags = flags} else written))
        MoveWide _ rd value
          | rd == A32.pc -> unsupportedAt here
          | otherwise -> Right (advance (setSlot (regNumber rd) (Just value) machine))
        MoveTop _ rd value
          | rd == A32.pc -> unsupportedAt here
          | otherwise -> Right (advance (setSlot (regNumber rd) ((\old -> (old .&. 0xffff) .|. (value `shiftL` 16)) <$> readReg rd) machine))
        Load _ Word False rt rn offset indexing -> do
          let writeback = indexing /= OffsetOnly
          when (writeback && (rn == A32.pc || rn == rt)) (unsupportedAt here)
          (location, effective) <- transferAddress rn offset indexing
          loaded <- loadWord model code machine (rn == A32.pc) location here
          let based = if writeback then setSlot (regNumber rn) (Just effective) machine else machine
          if rt == A32.pc then known here loaded >>= (`branchTo` based) else Right (advance (setSlot (regNumber rt) loaded based))
        Store _ Word rt rn offset indexing -> do
          let writeback = indexing /= OffsetOnly
          when (rt == A32.pc || (writeback && (rn == A32.pc || rn == rt))) (unsupportedAt here)
          (location, effective) <- transferAddress rn offset indexing
          stored <- storeWord model here location (readReg rt) machine
          Right (advance (if writeback then setSlot (regNumber rn) (Just effective) stored else stored))
        LoadMultiple _ rn mode writeback list -> do
          when (null list || rn == A32.pc || (writeback && rn `elem` list)) (unsupportedAt here)
          base <- known here (readReg rn)
          let (locations, final) = blockAddresses base mode list
          values <- mapM (\location -> loadWord model code machine False location here) locations
          let pairs = zip list values
              loadedState = foldl (\state (r, value) -> if r == A32.pc then state else setSlot (regNumber r) value state) machine pairs
              based = if writeback then setSlot (regNumber rn) (Just final) loadedState else loadedState
          case lookup A32.pc pairs of
            Just destination -> known here destination >>= (`branchTo` based)
            Nothing -> Right (advance based)
        StoreMultiple _ rn mode writeback list -> do
          when (null list || rn == A32.pc || A32.pc `elem` list || (writeback && rn `elem` list)) (unsupportedAt here)
          base <- known here (readReg rn)
          let (locations, final) = blockAddresses base mode list
          stored <- foldM (\state (location, r) -> storeWord model here location (readReg r) state) machine (zip locations list)
          Right (advance (if writeback then setSlot (regNumber rn) (Just final) stored else stored))
        Branch _ link destination -> branchTo destination (linked link)
        BranchExchange _ link rm
          | rm == A32.pc -> unsupportedAt here
          | otherwise -> known here (readReg rm) >>= (`branchTo` linked link)
        Hint _ -> Right (advance machine)
        _ -> unsupportedAt here

insideStack :: PollModel -> Word32 -> Bool
insideStack model location = let (low, high) = pollStack model in location >= low && location < high

loadWord :: PollModel -> CodeImage -> Machine -> Bool -> Word32 -> Word32 -> Either String (Maybe Word32)
loadWord model code machine literal location here
  | location .&. 3 /= 0 = unsupportedAt here
  | location == pollCell model = Right (Just (pollWaiting model))
  | insideStack model location = maybe (Left ("poll-period interpreter reads an unwritten stack word at " ++ hex here)) Right (M.lookup location (machineStack machine))
  | literal, Just value <- imageWord code location = Right (Just value)
  | otherwise = Left ("poll-period interpreter load outside the modelled memory at " ++ hex here)

storeWord :: PollModel -> Word32 -> Word32 -> Maybe Word32 -> Machine -> Either String Machine
storeWord model here location value machine
  | location .&. 3 /= 0 = unsupportedAt here
  | insideStack model location = Right machine {machineStack = M.insert location value (machineStack machine)}
  | otherwise = Right machine

shiftImmediateKnown :: Shift -> Int -> Maybe Word32 -> Maybe Bool -> (Maybe Word32, Maybe Bool)
shiftImmediateKnown kind amount value carry
  | kind == LSL && amount == 0 = (value, carry)
  | kind == ROR && amount == 0 = split (shiftByImmediate kind amount <$> value <*> carry)
  | otherwise = split ((\word -> shiftByImmediate kind amount word False) <$> value)
  where
    split shifted = (fst <$> shifted, snd <$> shifted)

shiftRegisterKnown :: Shift -> Maybe Word32 -> Maybe Word32 -> Maybe Bool -> (Maybe Word32, Maybe Bool)
shiftRegisterKnown kind amountRegister value carry = case (.&. 0xff) <$> amountRegister of
  Nothing -> (Nothing, Nothing)
  Just 0 -> (value, carry)
  Just amount ->
    let shifted = (\word -> shiftByRegister kind (fromIntegral amount) word False) <$> value
     in (fst <$> shifted, snd <$> shifted)

alu :: DataOp -> Maybe Word32 -> Maybe Word32 -> Maybe Bool -> Flags -> (Maybe Word32, Flags)
alu op left right shifterCarry flags = case op of
  AND -> logical ((.&.) <$> left <*> right)
  EOR -> logical (xor <$> left <*> right)
  SUB -> arithmetic left (complement <$> right) (Just True)
  RSB -> arithmetic (complement <$> left) right (Just True)
  ADD -> arithmetic left right (Just False)
  ADC -> arithmetic left right (flagC flags)
  SBC -> arithmetic left (complement <$> right) (flagC flags)
  RSC -> arithmetic (complement <$> left) right (flagC flags)
  TST -> logical ((.&.) <$> left <*> right)
  TEQ -> logical (xor <$> left <*> right)
  CMP -> arithmetic left (complement <$> right) (Just True)
  CMN -> arithmetic left right (Just False)
  ORR -> logical ((.|.) <$> left <*> right)
  MOV -> logical right
  BIC -> logical ((\a b -> a .&. complement b) <$> left <*> right)
  MVN -> logical (complement <$> right)
  where
    logical result = (result, flags {flagN = (`testBit` 31) <$> result, flagZ = (== 0) <$> result, flagC = shifterCarry})
    arithmetic a b carryIn =
      let outcome = addWithCarry <$> a <*> b <*> carryIn
          result = (\(value, _, _) -> value) <$> outcome
       in (result, Flags ((`testBit` 31) <$> result) ((== 0) <$> result) ((\(_, carryOut, _) -> carryOut) <$> outcome) ((\(_, _, overflow) -> overflow) <$> outcome))

addWithCarry :: Word32 -> Word32 -> Bool -> (Word32, Bool, Bool)
addWithCarry a b carryIn =
  let incoming = if carryIn then 1 else 0 :: Integer
      unsigned = toInteger a + toInteger b + incoming
      result = fromInteger unsigned :: Word32
      signed value = toInteger (fromIntegral value :: Int32)
   in (result, unsigned /= toInteger result, signed a + signed b + incoming /= signed result)

arithmeticShift :: Word32 -> Int -> Word32
arithmeticShift value amount = fromIntegral ((fromIntegral value :: Int32) `shiftR` amount)

shiftByImmediate :: Shift -> Int -> Word32 -> Bool -> (Word32, Bool)
shiftByImmediate kind amount value carry = case kind of
  LSL
    | amount == 0 -> (value, carry)
    | otherwise -> (value `shiftL` amount, testBit value (32 - amount))
  LSR
    | amount == 0 -> (0, testBit value 31)
    | otherwise -> (value `shiftR` amount, testBit value (amount - 1))
  ASR
    | amount == 0 -> (if testBit value 31 then maxBound else 0, testBit value 31)
    | otherwise -> (arithmeticShift value amount, testBit value (amount - 1))
  ROR
    | amount == 0 -> ((if carry then bit 31 else 0) .|. (value `shiftR` 1), testBit value 0)
    | otherwise -> (value `rotateR` amount, testBit value (amount - 1))

shiftByRegister :: Shift -> Int -> Word32 -> Bool -> (Word32, Bool)
shiftByRegister kind amount value carry
  | amount == 0 = (value, carry)
  | otherwise = case kind of
      LSL
        | amount < 32 -> (value `shiftL` amount, testBit value (32 - amount))
        | amount == 32 -> (0, testBit value 0)
        | otherwise -> (0, False)
      LSR
        | amount < 32 -> (value `shiftR` amount, testBit value (amount - 1))
        | amount == 32 -> (0, testBit value 31)
        | otherwise -> (0, False)
      ASR
        | amount < 32 -> (arithmeticShift value amount, testBit value (amount - 1))
        | otherwise -> (if testBit value 31 then maxBound else 0, testBit value 31)
      ROR ->
        let rotation = amount `mod` 32
         in if rotation == 0 then (value, testBit value 31) else (value `rotateR` rotation, testBit value (rotation - 1))

