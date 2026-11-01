module Replacement.KernelInstall.Admission
  ( AdmissionError (..), InstallationProof, scratchLow, refusalPc, executionPcs
  , deriveInstallationProof
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError = OperandNotConstant | StackOutOfRange | StackProvenanceLost | UnsupportedCondition
  | UnprovedCondition | UnsupportedShift | UnsupportedOperation | ArithmeticProvenanceLost
  | MissingStackPointer | UnallocatedStackAccess | UninitializedStackRead | InvalidCarrier
  | OrderedEffectsDiffer | MarkerOutcomeDiffer | ReturnStackDiffers | PreservedRegisterDiffers
  | ReturnRegistersDiffer | ReturnFlagsOrLrDiffer | ReadProvenanceRejected | UnalignedWrite
  | WriteProvenanceRejected | AddressProvenanceRejected | TransferOffsetRejected | CacheArgumentsDiffer
  | CacheStackUnaligned | ExecutionBoundExceeded | UnprovedTransfer | MarkerComparisonRejected
  | UnsupportedFlagUpdate | StoreOverlap | LoadOverlap | PcWriteback | PushBaseRejected
  | PushRegistersRejected | PopBaseRejected | PopRegistersRejected | CallStackUnaligned
  | ExternalCallRejected | UnsupportedInstruction deriving (Eq, Show)
data MarkerClass = ValidMarker | InvalidMarker deriving (Eq, Show)
data Root = PrimaryRoot | SecondaryRoot deriving (Eq, Show)
newtype WordAddress = WordAddress Word32 deriving (Eq, Show)
data Scalar = Constant Word32 | Incoming A.Reg | Stack Int | ReturnAddress | InternalReturn Word32 | HeaderRoot Root | Marker deriving (Eq, Show)
data Effect = HeaderRead Root | RootWrite Root | CacheCall | MarkerRead | DataWrite WordAddress Word32 deriving (Eq, Show)
data Flags = Flags (Maybe Bool) (Maybe Bool) (Maybe Bool) (Maybe Bool) deriving (Eq, Show)
data Machine = Machine
  { registers :: M.Map A.Reg Scalar, stack :: M.Map Int Scalar, flags :: Flags
  , effects :: [Effect], low :: Int, steps :: Int, fetched :: S.Set Word32
  }
data InstallationProof = InstallationProof Int Word32 [Word32]

scratchLow :: InstallationProof -> Int
scratchLow (InstallationProof bottom _ _) = bottom
refusalPc :: InstallationProof -> Word32
refusalPc (InstallationProof _ at _) = at
executionPcs :: InstallationProof -> [Word32]
executionPcs (InstallationProof _ _ pcs) = pcs

origin :: Word32
origin = 0x725a04

require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise failure = unless premise (Left failure)

get :: Machine -> A.Reg -> Scalar
get machine register = M.findWithDefault (Incoming register) register (registers machine)

known :: Scalar -> Either AdmissionError Word32
known (Constant word) = Right word
known _ = Left OperandNotConstant

put :: A.Reg -> Scalar -> Machine -> Either AdmissionError Machine
put register value machine = case (register,value) of
  (A.Reg 13,Stack offset) -> do
    require (offset >= -4096 && offset <= 0 && offset `mod` 4 == 0) StackOutOfRange
    pure machine {registers = M.insert register value (registers machine), stack = M.filterWithKey (\address _ -> address >= offset) (stack machine), low = min offset (low machine)}
  (A.Reg 13,_) -> Left StackProvenanceLost
  _ -> Right machine {registers = M.insert register value (registers machine)}

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

operand :: Machine -> A.Operand -> Either AdmissionError Scalar
operand _ (A.Immediate word) = Right (Constant word)
operand machine (A.ShiftedImmediate register A.LSL 0) = Right (get machine register)
operand _ _ = Left UnsupportedShift

arithmetic :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
arithmetic A.MOV _ right = Right right
arithmetic A.ADD (Stack offset) (Constant word) = Right (Stack (offset+fromIntegral word))
arithmetic A.SUB (Stack offset) (Constant word) = Right (Stack (offset-fromIntegral word))
arithmetic operation (Constant left) (Constant right) = case operation of
  A.ADD -> Right (Constant (left+right))
  A.SUB -> Right (Constant (left-right))
  _ -> Left UnsupportedOperation
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

append :: Effect -> Machine -> Machine
append effect machine = machine {effects = effect : effects machine}

expected :: MarkerClass -> [Effect]
expected marker = [HeaderRead PrimaryRoot,RootWrite PrimaryRoot,HeaderRead SecondaryRoot,RootWrite SecondaryRoot,CacheCall,MarkerRead]
  ++ if marker == InvalidMarker then [] else
    [DataWrite (WordAddress (0x40a0f870+4*i)) 0 | i <- [0..119]]
    ++ [DataWrite (WordAddress address) value | (address,value) <- [(0x40a0fc30,120),(0x40a0fc34,120),(0x40a0f86c,250),(0x40a0fc38,120),(0x40a0fc3c,1),(0x40a0fc40,122),(0x409c3500,0x7531)]]
    ++ concat [[DataWrite (WordAddress (0x409c3600+4*i)) 0,DataWrite (WordAddress (0x409c3800+4*i)) 0] | i <- [1..64]]
    ++ [DataWrite (WordAddress (0x40a0fc44+4*i)) 0 | i <- [0..359]]
    ++ [DataWrite (WordAddress (0x409c3904+4*i)) 0 | i <- [0..719]]
    ++ [DataWrite (WordAddress (0x40a0fa50+4*i)) 0 | i <- [0..119]]

deriveInstallationProof :: B.ByteString -> Either AdmissionError InstallationProof
deriveInstallationProof bytes = do
  require (not (B.null bytes) && B.length bytes <= 336 && B.length bytes `mod` 4 == 0) InvalidCarrier
  positive <- run ValidMarker
  negative <- run InvalidMarker
  let (positiveLow,_,positivePcs) = positive
      (negativeLow,negativePc,negativePcs) = negative
  pure (InstallationProof (min positiveLow negativeLow) negativePc (S.toAscList (S.union positivePcs negativePcs)))
  where
    inside address = address >= origin && toInteger address+4 <= toInteger origin+toInteger (B.length bytes) && address .&. 3 == 0
    run marker = walk marker origin (Machine (M.fromList ([(A.Reg n,Incoming (A.Reg n)) | n <- [0..12]] ++ [(A.sp,Stack 0),(A.lr,ReturnAddress)])) M.empty (Flags Nothing Nothing Nothing Nothing) [] 0 0 S.empty)
    finish marker spins pc machine = do
      require (spins == (marker == InvalidMarker)) MarkerOutcomeDiffer
      require (reverse (effects machine) == expected marker) OrderedEffectsDiffer
      unless spins $ do
        require (get machine A.sp == Stack 0) ReturnStackDiffers
        require (all (\n -> get machine (A.Reg n) == Incoming (A.Reg n)) [4..12]) PreservedRegisterDiffers
        require (map (get machine . A.Reg) [0..3] == map Constant [0,0x40a0fc2c,0,0x40a0fc2c]) ReturnRegistersDiffer
        require (get machine A.lr == Constant 0x725a40 && flags machine == Flags (Just False) (Just True) (Just True) (Just False)) ReturnFlagsOrLrDiffer
      pure (low machine,pc,fetched machine)
    load machine address = case address of
      Stack slot -> readStack machine slot >>= \value -> pure (value,machine)
      Constant 0x40010 -> pure (HeaderRoot PrimaryRoot,append (HeaderRead PrimaryRoot) machine)
      Constant 0xd0010 -> pure (HeaderRoot SecondaryRoot,append (HeaderRead SecondaryRoot) machine)
      Constant 0x7caf3c -> pure (Marker,append MarkerRead machine)
      Constant literal | inside literal -> pure (Constant (U.word32At bytes (fromIntegral (literal-origin))),machine)
      _ -> Left ReadProvenanceRejected
    store machine address value = case (address,value) of
      (Stack slot,_) -> writeStack machine slot value
      (Constant 0x96bd20,HeaderRoot PrimaryRoot) -> Right (append (RootWrite PrimaryRoot) machine)
      (Constant 0x96bd24,HeaderRoot SecondaryRoot) -> Right (append (RootWrite SecondaryRoot) machine)
      (Constant target,Constant word) -> do
        require (target .&. 3 == 0) UnalignedWrite
        pure (append (DataWrite (WordAddress target) word) machine)
      _ -> Left WriteProvenanceRejected
    offset machine base displacement = case displacement of
      A.OffsetImmediate delta -> case get machine base of
        Stack root -> Right (Stack (root+fromIntegral delta))
        Constant root -> Right (Constant (root+fromIntegral delta))
        _ -> Left AddressProvenanceRejected
      _ -> Left TransferOffsetRejected
    retained machine = do
      require (map (get machine . A.Reg) [0,1] == map Constant [0x96bd20,32]) CacheArgumentsDiffer
      require (case get machine A.sp of Stack bottom -> bottom <= -8 && bottom `mod` 8 == 0; _ -> False) CacheStackUnaligned
      changed <- foldM (\state (register,value) -> put register (Constant value) state) machine (zip (map A.Reg [0..3]) [0x96bd40,0x96bd40,1,0])
      pure (append CacheCall changed) {flags = Flags (Just False) (Just True) (Just True) (Just False)}
    walk marker pc machine = do
      require (inside pc && steps machine < 12000) ExecutionBoundExceeded
      let instruction = A.decode pc (U.word32At bytes (fromIntegral (pc-origin)))
          advanced = machine {steps = steps machine+1,fetched = S.insert pc (fetched machine)}
          next = walk marker (pc+4)
          transfer ReturnAddress state = finish marker False pc state
          transfer (InternalReturn target) state = walk marker target state
          transfer (Constant target) state = walk marker target state
          transfer _ _ = Left UnprovedTransfer
          assign register value state = if register == A.pc then transfer value state else put register value state >>= next
          execute cond action = condition (flags machine) cond >>= \taken -> if taken then action else next advanced
      case instruction of
        A.MoveWide cond destination word -> execute cond (assign destination (Constant word) advanced)
        A.MoveTop cond destination word -> execute cond $ do
          previous <- known (get machine destination)
          assign destination (Constant ((previous .&. 0xffff) .|. (word `shiftL` 16))) advanced
        A.DataProcessing cond operation updates destination source value -> execute cond $ do
          right <- operand machine value
          if operation == A.CMP && updates then do
            comparison <- case (get machine source,right) of
              (Marker,Constant 3) -> Right (Flags Nothing (Just (marker == ValidMarker)) Nothing Nothing)
              (Constant left,Constant other) -> Right (subtractFlags left other)
              _ -> Left MarkerComparisonRejected
            next advanced {flags = comparison}
          else do
            result <- arithmetic operation (get machine source) right
            changed <- put destination result advanced
            if updates then do
              require (operation == A.SUB) UnsupportedFlagUpdate
              left <- known (get machine source)
              other <- known right
              next changed {flags = subtractFlags left other}
            else if destination == A.pc then transfer result changed else next changed
        A.Store cond A.Word source base displacement indexing -> execute cond $ do
          require (indexing == A.OffsetOnly || base /= source) StoreOverlap
          address <- offset machine base displacement
          let actual = if indexing == A.PostIndexed then get machine base else address
          allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base address advanced else pure advanced
          changed <- store allocatedMachine actual (get machine source)
          updated <- if indexing == A.OffsetOnly then pure changed else put base address changed
          next updated
        A.Load cond A.Word False destination base displacement indexing -> execute cond $ do
          require (indexing == A.OffsetOnly || base /= destination) LoadOverlap
          require (base /= A.pc || indexing == A.OffsetOnly) PcWriteback
          baseMachine <- if base == A.pc then put base (Constant (pc+8)) machine else pure machine
          address <- offset baseMachine base displacement
          let actual = if indexing == A.PostIndexed then get machine base else address
          allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base address advanced else pure advanced
          (value,changed) <- load allocatedMachine actual
          updated <- if indexing == A.OffsetOnly then pure changed else put base address changed
          assign destination value updated
        A.StoreMultiple cond base A.DecrementBefore True saved -> execute cond $ do
          root <- case get machine base of Stack address -> Right address; _ -> Left PushBaseRejected
          require (base == A.sp && not (null saved) && base `notElem` saved && A.pc `notElem` saved) PushRegistersRejected
          let bottom = root-4*length saved
          allocatedMachine <- put A.sp (Stack bottom) advanced
          changed <- foldM (\state (address,register) -> writeStack state address (get machine register)) allocatedMachine (zip [bottom,bottom+4..] saved)
          next changed
        A.LoadMultiple cond base A.IncrementAfter True restored -> execute cond $ do
          root <- case get machine base of Stack address -> Right address; _ -> Left PopBaseRejected
          require (base == A.sp && not (null restored) && base `notElem` restored) PopRegistersRejected
          values <- mapM (readStack machine) (take (length restored) [root,root+4..])
          loaded <- foldM (\state (register,value) -> put register value state) advanced (zip restored values)
          changed <- put A.sp (Stack (root+4*length restored)) loaded
          if A.pc `elem` restored then transfer (get changed A.pc) changed else next changed
        A.Branch cond link target -> execute cond $ do
          if link then do
            require (case get machine A.sp of Stack bottom -> bottom `mod` 8 == 0; _ -> False) CallStackUnaligned
            called <- put A.lr (InternalReturn (pc+4)) advanced
            if inside target then walk marker target called else do
              require (target == 0x42634) ExternalCallRejected
              retained called >>= next
          else if target == pc then finish marker True pc advanced
          else walk marker target advanced
        A.BranchExchange cond False register -> execute cond (transfer (get machine register) advanced)
        A.Hint cond | U.word32At bytes (fromIntegral (pc-origin)) == 0xe320f000 -> execute cond (next advanced)
        _ -> Left UnsupportedInstruction
