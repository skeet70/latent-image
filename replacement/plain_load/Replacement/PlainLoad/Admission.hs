module Replacement.PlainLoad.Admission
  ( AdmissionError (..), ProducerProof, deriveProducerProof
  , scratchLow, refusalPc, executionPcs, callSites, returnPcs
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits (complement, rotateR, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError
  = InvalidCarrier | UnsupportedInstruction | UnsupportedCondition | UnprovedCondition
  | UnsupportedShift | UnsupportedOperation | UnsupportedFlagUpdate | OperandNotNumeric
  | ArithmeticProvenanceLost | StackProvenanceLost | StackOutOfRange | MissingStackPointer
  | UnallocatedStackAccess | UninitializedStackRead | AddressProvenanceRejected
  | ReadProvenanceRejected | WriteProvenanceRejected | TransferOffsetRejected
  | StoreOverlap | LoadOverlap | PcWriteback | PushRegistersRejected | PopRegistersRejected
  | CallStackUnaligned | ExternalCallRejected | CallArgumentsDiffer | OrderedEffectsDiffer
  | FifthArgumentDiffer | OutcomeDiffer | ReturnStatusDiffer | ReturnStackDiffers
  | PreservedRegisterDiffers | ReturnLrDiffers | UnprovedTransfer | ExecutionOutOfRange
  | ExecutionBoundExceeded | RefusalPcDiffers
  deriving (Eq, Show)

data Service = SegmentLookup | ExtentGetter | ModeGetter | StorageRead | Progress | Take | Give | Delay
  deriving (Eq, Ord, Show)
data Field = FlashOffset | InitialLength | FinalLength | Destination deriving (Eq, Ord, Show)
data Source = FieldSource Field | ModeSource | ReadStatusSource | IgnoredResult Int deriving (Eq, Ord, Show)
data Root = SegmentRoot | ExtentRoot deriving (Eq, Show)
data Scalar = Number Word32 (S.Set Source) | Pointer Root Int | Stack Int | Incoming A.Reg
  | ReturnAddress | InternalReturn Word32 | Unknown deriving (Eq, Show)
data Effect = FieldRead Field | Retained Service [Scalar] deriving (Eq, Show)
data Flags = Flags (Maybe Bool) (Maybe Bool) (Maybe Bool) (Maybe Bool) deriving (Eq, Show)
data IgnoredResults = FixedStatus Word32 | VaryingStatus
data Scenario = Scenario
  { nullSegment :: Bool, modeWord :: Word32, flashOffset :: Word32, initialLength :: Word32
  , finalLength :: Word32, destinationWord :: Word32, readStatus :: Word32, ignoredResults :: IgnoredResults
  }
data Machine = Machine
  { registers :: M.Map A.Reg Scalar, stack :: M.Map Int Scalar, flags :: Flags
  , remaining :: [Effect], lengthReads :: Int, serviceCount :: Int
  , low :: Int, steps :: Int, fetched :: S.Set Word32, calls :: S.Set (Word32,Word32)
  }
data Path = Path Int Word32 (S.Set Word32) (S.Set (Word32,Word32)) Bool

-- Conditional finite host admission, not natural qualification or a universal kernel ABI proof.
-- Premises: tagged nonaliasing segment/extent words, retained AAPCS callee saves, and aligned incoming SP.
-- Each field read and service result supplies its corpus value. Other volatile registers/flags are unknown.
-- Number sources track dependencies, not algebraic equality. Unsampled values and unreachable code are not quantified.
data ProducerProof = ProducerProof Int Word32 [Word32] [(Word32,Word32)] [Word32]

scratchLow :: ProducerProof -> Int
scratchLow (ProducerProof bottom _ _ _ _) = bottom
refusalPc :: ProducerProof -> Word32
refusalPc (ProducerProof _ pc _ _ _) = pc
executionPcs :: ProducerProof -> [Word32]
executionPcs (ProducerProof _ _ pcs _ _) = pcs
callSites :: ProducerProof -> [(Word32,Word32)]
callSites (ProducerProof _ _ _ sites _) = sites
returnPcs :: ProducerProof -> [Word32]
returnPcs (ProducerProof _ _ _ _ pcs) = pcs

origin :: Word32
origin = 0x6a9844

require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise failure = unless premise (Left failure)

constant :: Word32 -> Scalar
constant word = Number word S.empty
sourced :: Source -> Word32 -> Scalar
sourced source word = Number word (S.singleton source)

-- The oracle is the ordered source contract, independent of instruction selection.
sourceEffects :: Scenario -> [Effect]
sourceEffects scenario = [Retained SegmentLookup [constant 6],Retained ExtentGetter []]
  ++ if nullSegment scenario then [] else [Retained ModeGetter []] ++ body
  where
    field name value = sourced (FieldSource name) value
    count = (initialLength scenario + 0x1ffff) `shiftR` 17
    body
      | modeWord scenario == 0 =
          [ FieldRead FlashOffset,FieldRead InitialLength,FieldRead Destination
          , Retained StorageRead
              [ field FlashOffset (flashOffset scenario `shiftR` 17),field InitialLength count
              , field Destination (destinationWord scenario),constant 0x73b924,constant 1]
          ] ++ final
      | otherwise = [FieldRead InitialLength,Retained Take [constant 0x24,constant maxBound]
          ,Retained Take [constant 0x34,constant maxBound]]
          ++ concat [[Retained Delay [constant 14],Retained Progress [constant (index `shiftL` 17)]] | index <- [1..count]]
          ++ [Retained Give [constant 0x34],Retained Give [constant 0x24]] ++ final
    final = [FieldRead FinalLength,Retained Progress [field FinalLength (finalLength scenario)]]

-- Finite field/status discrimination includes canonical 27 blocks and wrapping addition.
-- Nonzero modes are representatives, not an interval proof over all nonzero words.
corpus :: [Scenario]
corpus = [base,base {modeWord = 1},base {nullSegment = True}]
  ++ [base {modeWord = mode,initialLength = len,finalLength = len `xor` 0x13579bdf}
     | mode <- [0,1,2,0x80000000,maxBound]
     , len <- [0,1,0x1ffff,0x20000,0x20001,0x40000,0x352e00,0xfffe0001,0xffffffff]]
  ++ [base {modeWord = mode,flashOffset = offset,destinationWord = dest,readStatus = status
           ,initialLength = len,finalLength = final,ignoredResults = ignored}
     | mode <- [0,7,0x40000000]
     , (offset,dest,len,final) <- [(0x12345678,0x81234560,0x40001,0x87654321),(0xfffeffff,0x10203040,0,0x20001)]
     , status <- [0,0xffffff01,0xffffffe2,0xfffffff6,0xffffffd8,1,0xffffffff,0x80000005,0x13572468]
     , ignored <- [FixedStatus 0,FixedStatus 0xffffffe7,FixedStatus 0xffffffce,FixedStatus 0xffffffff,VaryingStatus]]
  where base = Scenario False 0 0x5a0000 0x360000 0x360000 0x1100000 0 (FixedStatus 0)

ignoredResult :: Scenario -> Int -> Word32
ignoredResult scenario index = case ignoredResults scenario of
  FixedStatus status -> status
  VaryingStatus -> 0x80000009 `xor` (fromIntegral (index+1)*0x9e3779b9)

get :: Machine -> A.Reg -> Scalar
get machine register = M.findWithDefault Unknown register (registers machine)

put :: A.Reg -> Scalar -> Machine -> Either AdmissionError Machine
put register value machine = case (register,value) of
  (A.Reg 13,Stack offset) -> do
    require (offset >= -4096 && offset <= 0 && offset `mod` 4 == 0) StackOutOfRange
    pure machine {registers = M.insert register value (registers machine)
      ,stack = M.filterWithKey (\address _ -> address >= offset) (stack machine),low = min offset (low machine)}
  (A.Reg 13,_) -> Left StackProvenanceLost
  _ -> pure machine {registers = M.insert register value (registers machine)}

numeric :: Scalar -> Either AdmissionError (Word32,S.Set Source)
numeric (Number word sources) = Right (word,sources)
numeric _ = Left OperandNotNumeric

condition :: Flags -> A.Cond -> Either AdmissionError Bool
condition (Flags n z c v) (A.Cond code) = case code of
  14 -> Right True
  0 -> need z
  1 -> not <$> need z
  2 -> need c
  3 -> not <$> need c
  4 -> need n
  5 -> not <$> need n
  6 -> need v
  7 -> not <$> need v
  8 -> (&&) <$> need c <*> (not <$> need z)
  9 -> (||) <$> (not <$> need c) <*> need z
  10 -> (==) <$> need n <*> need v
  11 -> (/=) <$> need n <*> need v
  12 -> (&&) <$> (not <$> need z) <*> ((==) <$> need n <*> need v)
  13 -> (||) <$> need z <*> ((/=) <$> need n <*> need v)
  _ -> Left UnsupportedCondition
  where need = maybe (Left UnprovedCondition) Right

subtractFlags :: Word32 -> Word32 -> Flags
subtractFlags left right =
  let result = left-right
      negative word = word .&. 0x80000000 /= 0
  in Flags (Just (negative result)) (Just (result == 0)) (Just (left >= right))
       (Just (negative left /= negative right && negative result /= negative left))

addFlags :: Word32 -> Word32 -> Flags
addFlags left right =
  let result = left+right
      negative word = word .&. 0x80000000 /= 0
  in Flags (Just (negative result)) (Just (result == 0)) (Just (toInteger left+toInteger right > 0xffffffff))
       (Just (negative left == negative right && negative result /= negative left))

compareFlags :: Scalar -> Scalar -> Either AdmissionError Flags
compareFlags (Pointer _ 0) (Number 0 _) = Right (Flags Nothing (Just False) Nothing Nothing)
compareFlags left right = do
  (a,_) <- numeric left
  (b,_) <- numeric right
  pure (subtractFlags a b)

operand :: Machine -> A.Operand -> Either AdmissionError Scalar
operand _ (A.Immediate word) = Right (constant word)
operand machine (A.ShiftedImmediate register A.LSL 0) = do
  require (register /= A.pc) UnprovedTransfer
  pure (get machine register)
operand machine (A.ShiftedImmediate register shift amount) = do
  require (register /= A.pc && amount > 0 && amount < 32) UnsupportedShift
  (word,sources) <- numeric (get machine register)
  result <- case shift of
    A.LSL -> Right (word `shiftL` amount)
    A.LSR -> Right (word `shiftR` amount)
    A.ASR -> Right (fromIntegral ((fromIntegral word :: Int32) `shiftR` amount))
    A.ROR -> Right (rotateR word amount)
  pure (Number result sources)
operand _ _ = Left UnsupportedShift

arithmetic :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
arithmetic A.MOV _ right = Right right
arithmetic A.MVN _ right = do
  (word,sources) <- numeric right
  pure (Number (complement word) sources)
arithmetic A.ADD (Stack offset) (Number word sources) | S.null sources = Right (Stack (offset+fromIntegral (fromIntegral word :: Int32)))
arithmetic A.SUB (Stack offset) (Number word sources) | S.null sources = Right (Stack (offset-fromIntegral (fromIntegral word :: Int32)))
arithmetic A.ADD (Pointer root offset) (Number word sources) | S.null sources = Right (Pointer root (offset+fromIntegral (fromIntegral word :: Int32)))
arithmetic A.SUB (Pointer root offset) (Number word sources) | S.null sources = Right (Pointer root (offset-fromIntegral (fromIntegral word :: Int32)))
arithmetic operation (Number left a) (Number right b) = do
  result <- case operation of
    A.ADD -> Right (left+right)
    A.SUB -> Right (left-right)
    A.RSB -> Right (right-left)
    A.AND -> Right (left .&. right)
    A.ORR -> Right (left .|. right)
    A.EOR -> Right (left `xor` right)
    A.BIC -> Right (left .&. complement right)
    _ -> Left UnsupportedOperation
  pure (Number result (S.union a b))
arithmetic _ _ _ = Left ArithmeticProvenanceLost

allocated :: Machine -> Int -> Either AdmissionError ()
allocated machine address = do
  bottom <- case get machine A.sp of Stack offset -> Right offset; _ -> Left MissingStackPointer
  require (address >= bottom && address < 0 && address `mod` 4 == 0) UnallocatedStackAccess

writeStack :: Machine -> Int -> Scalar -> Either AdmissionError Machine
writeStack machine address value = do
  allocated machine address
  pure machine {stack = M.insert address value (stack machine)}

readStack :: Machine -> Int -> Either AdmissionError Scalar
readStack machine address = do
  allocated machine address
  maybe (Left UninitializedStackRead) Right (M.lookup address (stack machine))

consume :: Effect -> Machine -> Either AdmissionError Machine
consume effect machine = case remaining machine of
  wanted:rest -> do
    require (effect == wanted) $ case (effect,wanted) of
      (Retained actual _,Retained expected _) | actual == expected -> CallArgumentsDiffer
      _ -> OrderedEffectsDiffer
    pure machine {remaining = rest}
  [] -> Left OrderedEffectsDiffer

serviceAt :: Word32 -> Either AdmissionError Service
serviceAt target = case target of
  0x657114 -> Right SegmentLookup
  0x6571a0 -> Right ExtentGetter
  0x657184 -> Right ModeGetter
  0x6a8f1c -> Right StorageRead
  0x73b924 -> Right Progress
  0x724b68 -> Right Take
  0x724c00 -> Right Give
  0x69b9c4 -> Right Delay
  _ -> Left ExternalCallRejected

deriveProducerProof :: B.ByteString -> Either AdmissionError ProducerProof
deriveProducerProof bytes = do
  require (not (B.null bytes) && B.length bytes <= 244 && B.length bytes `mod` 4 == 0) InvalidCarrier
  paths <- mapM run corpus
  let bottoms = [bottom | Path bottom _ _ _ _ <- paths]
      refusals = S.fromList [pc | Path _ pc _ _ True <- paths]
      pcs = S.unions [visited | Path _ _ visited _ _ <- paths]
      sites = S.unions [visited | Path _ _ _ visited _ <- paths]
      returns = S.fromList [pc | Path _ pc _ _ False <- paths]
  require (S.size refusals == 1) RefusalPcDiffers
  pure (ProducerProof (minimum bottoms) (S.findMin refusals) (S.toAscList pcs) (S.toAscList sites) (S.toAscList returns))
  where
    inside pc = pc >= origin && toInteger pc+4 <= toInteger origin+toInteger (B.length bytes) && pc .&. 3 == 0
    run scenario = walk scenario origin (Machine
      (M.fromList ([(A.Reg n,Incoming (A.Reg n)) | n <- [0..12]] ++ [(A.sp,Stack 0),(A.lr,ReturnAddress)]))
      M.empty (Flags Nothing Nothing Nothing Nothing) (sourceEffects scenario) 0 0 0 0 S.empty S.empty)
    finish scenario spins pc machine = do
      require (null (remaining machine)) OrderedEffectsDiffer
      require (spins == nullSegment scenario) OutcomeDiffer
      unless spins $ do
        require (get machine A.sp == Stack 0) ReturnStackDiffers
        require (all (\n -> get machine (A.Reg n) == Incoming (A.Reg n)) [4..11]) PreservedRegisterDiffers
        require (get machine (A.Reg 0) == if modeWord scenario == 0 then sourced ReadStatusSource (readStatus scenario) else constant 0) ReturnStatusDiffer
        require (get machine A.lr == constant 0x6a992c) ReturnLrDiffers
      pure (Path (low machine) pc (fetched machine) (calls machine) spins)
    load scenario machine address = case address of
      Stack slot -> do
        value <- readStack machine slot
        pure (value,machine)
      Pointer SegmentRoot 20 -> field FlashOffset (flashOffset scenario)
      Pointer SegmentRoot 24 -> do
        let name = if lengthReads machine == 0 then InitialLength else FinalLength
            value = if lengthReads machine == 0 then initialLength scenario else finalLength scenario
        (result,changed) <- field name value
        pure (result,changed {lengthReads = lengthReads machine+1})
      Pointer ExtentRoot 4 -> field Destination (destinationWord scenario)
      Number literal sources | S.null sources && inside literal ->
        pure (constant (U.word32At bytes (fromIntegral (literal-origin))),machine)
      _ -> Left ReadProvenanceRejected
      where field name value = do
              changed <- consume (FieldRead name) machine
              pure (sourced (FieldSource name) value,changed)
    store machine address value = case address of
      Stack slot -> writeStack machine slot value
      _ -> Left WriteProvenanceRejected
    offset machine base displacement = case displacement of
      A.OffsetImmediate delta -> case get machine base of
        Stack root -> Right (Stack (root+fromIntegral delta))
        Pointer root position -> Right (Pointer root (position+fromIntegral delta))
        Number root sources | S.null sources -> Right (constant (root+fromIntegral delta))
        _ -> Left AddressProvenanceRejected
      _ -> Left TransferOffsetRejected
    retained scenario pc target machine = do
      service <- serviceAt target
      require (case get machine A.sp of Stack bottom -> bottom <= 0 && bottom `mod` 8 == 0; _ -> False) CallStackUnaligned
      fifth <- if service == StorageRead then do
        bottom <- case get machine A.sp of Stack value -> Right value; _ -> Left MissingStackPointer
        value <- readStack machine bottom
        require (value == constant 1) FifthArgumentDiffer
        pure [value]
        else pure []
      let arity = case service of
            SegmentLookup -> 1; ExtentGetter -> 0; ModeGetter -> 0; StorageRead -> 4
            Progress -> 1; Take -> 2; Give -> 1; Delay -> 1
          arguments = map (get machine . A.Reg) [0..arity-1] ++ fifth
      consumed <- consume (Retained service arguments) machine
      clobbered <- foldM (\state register -> put register Unknown state) consumed (map A.Reg [0,1,2,3,12,14])
      let result = case service of
            SegmentLookup -> if nullSegment scenario then constant 0 else Pointer SegmentRoot 0
            ExtentGetter -> Pointer ExtentRoot 0
            ModeGetter -> sourced ModeSource (modeWord scenario)
            StorageRead -> sourced ReadStatusSource (readStatus scenario)
            _ -> sourced (IgnoredResult (serviceCount machine)) (ignoredResult scenario (serviceCount machine))
      changed <- put (A.Reg 0) result clobbered
      pure changed {flags = Flags Nothing Nothing Nothing Nothing,serviceCount = serviceCount machine+1
        ,calls = S.insert (pc,target) (calls machine)}
    walk scenario pc machine = do
      require (inside pc) ExecutionOutOfRange
      require (steps machine < 8192) ExecutionBoundExceeded
      let word = U.word32At bytes (fromIntegral (pc-origin))
          instruction = A.decode pc word
          advanced = machine {steps = steps machine+1,fetched = S.insert pc (fetched machine)}
          next = walk scenario (pc+4)
          transfer ReturnAddress state = finish scenario False pc state
          transfer (InternalReturn target) state = walk scenario target state
          transfer (Number target sources) state | S.null sources = walk scenario target state
          transfer _ _ = Left UnprovedTransfer
          assign register value state = if register == A.pc then transfer value state else put register value state >>= next
          execute cond action = condition (flags machine) cond >>= \taken -> if taken then action else next advanced
      case instruction of
        A.MoveWide cond destination word16 -> execute cond (assign destination (constant word16) advanced)
        A.MoveTop cond destination word16 -> execute cond $ do
          (previous,sources) <- numeric (get machine destination)
          assign destination (Number ((previous .&. 0xffff) .|. (word16 `shiftL` 16)) sources) advanced
        A.DataProcessing cond operation updates destination source value -> execute cond $ do
          require (source /= A.pc || operation `elem` [A.MOV,A.MVN]) UnprovedTransfer
          right <- operand machine value
          if operation == A.CMP && updates then do
            compared <- compareFlags (get machine source) right
            next advanced {flags = compared}
          else if operation `elem` [A.TST,A.TEQ] && updates then do
            (left,_) <- numeric (get machine source)
            (other,_) <- numeric right
            let result = if operation == A.TST then left .&. other else left `xor` other
            next advanced {flags = Flags (Just (result .&. 0x80000000 /= 0)) (Just (result == 0)) Nothing Nothing}
          else do
            result <- arithmetic operation (get machine source) right
            if updates then do
              require (destination /= A.pc && operation `elem` [A.ADD,A.SUB,A.RSB]) UnsupportedFlagUpdate
              (left,_) <- numeric (get machine source)
              (other,_) <- numeric right
              let updated = case operation of
                    A.ADD -> addFlags left other
                    A.RSB -> subtractFlags other left
                    _ -> subtractFlags left other
              changed <- put destination result advanced
              next changed {flags = updated}
            else assign destination result advanced
        A.Store cond A.Word source base displacement indexing -> execute cond $ do
          require (indexing == A.OffsetOnly || base /= source) StoreOverlap
          require (base /= A.pc && source /= A.pc) PcWriteback
          address <- offset machine base displacement
          let actual = if indexing == A.PostIndexed then get machine base else address
          allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base address advanced else pure advanced
          changed <- store allocatedMachine actual (get machine source)
          updated <- if indexing == A.OffsetOnly then pure changed else put base address changed
          next updated
        A.Load cond A.Word False destination base displacement indexing -> execute cond $ do
          require (indexing == A.OffsetOnly || base /= destination) LoadOverlap
          require (base /= A.pc || indexing == A.OffsetOnly) PcWriteback
          baseMachine <- if base == A.pc then put base (constant (pc+8)) machine else pure machine
          address <- offset baseMachine base displacement
          let actual = if indexing == A.PostIndexed then get machine base else address
          allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base address advanced else pure advanced
          (value,changed) <- load scenario allocatedMachine actual
          updated <- if indexing == A.OffsetOnly then pure changed else put base address changed
          assign destination value updated
        A.StoreMultiple cond base A.DecrementBefore True saved -> execute cond $ do
          require (base == A.sp && not (null saved) && base `notElem` saved && A.pc `notElem` saved) PushRegistersRejected
          root <- case get machine base of Stack address -> Right address; _ -> Left MissingStackPointer
          let bottom = root-4*length saved
          allocatedMachine <- put A.sp (Stack bottom) advanced
          changed <- foldM (\state (address,register) -> writeStack state address (get machine register)) allocatedMachine (zip [bottom,bottom+4..] saved)
          next changed
        A.LoadMultiple cond base A.IncrementAfter True restored -> execute cond $ do
          require (base == A.sp && not (null restored) && base `notElem` restored) PopRegistersRejected
          root <- case get machine base of Stack address -> Right address; _ -> Left MissingStackPointer
          values <- mapM (readStack machine) (take (length restored) [root,root+4..])
          loaded <- foldM (\state (register,value) -> put register value state) advanced (zip restored values)
          changed <- put A.sp (Stack (root+4*length restored)) loaded
          if A.pc `elem` restored then transfer (get changed A.pc) changed else next changed
        A.Branch cond link target -> execute cond $ do
          if link then do
            require (case get machine A.sp of Stack bottom -> bottom `mod` 8 == 0; _ -> False) CallStackUnaligned
            called <- put A.lr (InternalReturn (pc+4)) advanced
            if inside target then walk scenario target called else retained scenario pc target called >>= next
          else if target == pc then finish scenario True pc advanced
          else walk scenario target advanced
        A.BranchExchange cond False register -> execute cond (transfer (get machine register) advanced)
        A.Hint cond | word == 0xe320f000 -> execute cond (next advanced)
        _ -> Left UnsupportedInstruction
