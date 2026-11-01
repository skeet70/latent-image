module Replacement.TaskSubmission.Admission
  ( AdmissionError (..), EntryPoint (..), FixedService (..), CallTarget (..)
  , CodeAddress (..), StackOffset (..), CodeExtent (..), NativeLayout (..)
  , ArgumentProvenance (..), ParameterWord (..), FrameProof, CallBoundary (..)
  , scratchLow, callBoundaries, executionPcs, globalWrites, deriveFrameProof
  , StackInterval (..), StackContext (..), CalleeDemand (..), JoinedStack (..), joinStack
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits ((.&.), (.|.), complement, rotateR, shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
newtype StackOffset = StackOffset Integer deriving (Eq, Ord, Show)
data EntryPoint = Submission | Release deriving (Eq, Ord, Show)
data FixedService = CriticalEnter | CriticalExit | Reservation | Fill | Cache | Log | ErrorCallback | TryLock | Unlock deriving (Eq, Ord, Show, Enum, Bounded)
data CallTarget = FixedService FixedService | AuxiliaryService | CreationService | ActivationService | CurrentCoreService | SaveMaskService | RestoreMaskService deriving (Eq, Ord, Show)
data CodeExtent = CodeExtent CodeAddress CodeAddress deriving (Eq, Show)
data NativeLayout = NativeLayout { codeExtents :: [CodeExtent] } deriving (Eq, Show)
data ArgumentProvenance = ScalarArgument | ConstantArgument Word32 | StackArgument StackOffset
  | NonstackArgument | ServiceArgument CallTarget | SavedRegisterArgument A.Reg | ReturnCapabilityArgument
  deriving (Eq, Ord, Show)
data ParameterWord = ParameterWord { parameterOffset :: StackOffset, parameterProvenance :: ArgumentProvenance }
  deriving (Eq, Ord, Show)
data CallBoundary = CallBoundary
  { callEntry :: EntryPoint, callInstruction :: CodeAddress, callContinuation :: CodeAddress
  , callTarget :: CallTarget, callTargetRegister :: Maybe A.Reg
  , callR0 :: ArgumentProvenance, callR1 :: ArgumentProvenance, callParameters :: [ParameterWord]
  , callStack :: StackOffset, callLiveWords :: [StackOffset]
  } deriving (Eq, Ord, Show)
data FrameProof = FrameProof (M.Map EntryPoint StackOffset) [CallBoundary] [CodeAddress] [CodeAddress]
  deriving (Eq, Show)
data AdmissionError = InvalidCarrier | EscapingControlFlow | UnsupportedInstruction | UnsupportedCondition
  | StackProvenanceLost | StackOutOfRange | UnallocatedStackAccess | UninitializedStackRead
  | StackAliasConflict | ArithmeticProvenanceLost | UnsupportedShift | ReadProvenanceRejected
  | WriteProvenanceRejected | LiteralExecuted | BadReturn | PreservedRegisterDiffers
  | CallStackUnaligned | ExternalCallRejected | CallContractRejected | WorklistBoundExceeded
  | IncompatibleFrames | MissingReturn | MissingCallBoundary | InvalidStackContext | MissingCalleeDemand
  | NestedStackOverlap | StackBackingExceeded | StackAddressOverflow
  deriving (Eq, Show)

data Guest = Descriptor | ClassTable | Wrapper | State | Roots | Root | ErrorWord | Lock deriving (Eq, Ord, Show)
data Scalar = Unknown | Constant Word32 | Incoming A.Reg | Stack Integer | ReturnAddress
  | InternalReturn Word32 | Guest Guest (Maybe Integer) | Service CallTarget
  deriving (Eq, Ord, Show)

provenance :: Scalar -> ArgumentProvenance
provenance value = case value of
  Unknown -> ScalarArgument
  Constant word -> ConstantArgument word
  Incoming register -> SavedRegisterArgument register
  Stack offset -> StackArgument (StackOffset offset)
  Guest _ _ -> NonstackArgument
  Service target -> ServiceArgument target
  _ -> ReturnCapabilityArgument

data ReturnFrame = ReturnFrame Word32 Integer [Scalar] deriving (Eq, Ord, Show)
data Machine = Machine { regs :: M.Map A.Reg Scalar, wordsAt :: M.Map Integer Scalar, returns :: [ReturnFrame] }
  deriving (Eq, Show)
data Walk = Walk
  { seen :: M.Map (Word32,[ReturnFrame]) Machine, pending :: [(Word32,Machine)]
  , fetched :: S.Set CodeAddress, literals :: S.Set CodeAddress, calls :: S.Set CallBoundary, writes :: S.Set CodeAddress
  , lowest :: Integer, returned :: Bool, iterations :: Int }

require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise failure = unless premise (Left failure)

scratchLow :: FrameProof -> EntryPoint -> StackOffset
scratchLow (FrameProof lows _ _ _) entry = M.findWithDefault (StackOffset 0) entry lows
callBoundaries :: FrameProof -> [CallBoundary]
callBoundaries (FrameProof _ boundaries _ _) = boundaries
executionPcs :: FrameProof -> [CodeAddress]
executionPcs (FrameProof _ _ pcs _) = pcs
globalWrites :: FrameProof -> [CodeAddress]
globalWrites (FrameProof _ _ _ pcs) = pcs

origin, release, end, lockLiteral :: Word32
origin = 0x725dd8
release = 0x726098
end = 0x72612c
lockLiteral = 0x726128

fixed :: Word32 -> Maybe CallTarget
fixed address = FixedService <$> lookup address
  [(0x7235d8,CriticalEnter),(0x7236b0,CriticalExit),(0x725c6c,Reservation)
  ,(0x7221c8,Fill),(0x42634,Cache),(0x41a34,Log),(0x723398,ErrorCallback)
  ,(0x427d4,TryLock),(0x427fc,Unlock)]

constant :: Word32 -> Scalar
constant address = case lookup address [(0x7cad9c,ClassTable),(0x40a5b204,Wrapper),(0x40a0fc44,State),(0x96bd20,Roots),(0x40a0f82c,ErrorWord),(0x409c280c,Lock)] of
  Just root -> Guest root (Just 0)
  Nothing -> Constant address

get :: Word32 -> Machine -> A.Reg -> Scalar
get pc machine register
  | register == A.pc = Constant (pc+8)
  | otherwise = M.findWithDefault Unknown register (regs machine)

spAt :: Machine -> Either AdmissionError Integer
spAt machine = case M.lookup A.sp (regs machine) of
  Just (Stack offset) -> Right offset
  _ -> Left StackProvenanceLost

put :: A.Reg -> Scalar -> Machine -> Either AdmissionError Machine
put register value machine = do
  require (register /= A.pc) EscapingControlFlow
  case (register,value) of
    (A.Reg 13,Stack offset) -> do
      require (offset <= 0 && offset >= -852 && offset `mod` 4 == 0) StackOutOfRange
      pure machine { regs = M.insert register value (regs machine), wordsAt = M.filterWithKey (\slot _ -> slot >= offset) (wordsAt machine) }
    (A.Reg 13,_) -> Left StackProvenanceLost
    _ -> pure machine { regs = M.insert register value (regs machine) }

allocated :: Machine -> Integer -> Integer -> Either AdmissionError ()
allocated machine slot size = do
  bottom <- spAt machine
  require (slot >= bottom && slot+size <= 0 && slot `mod` size == 0) UnallocatedStackAccess

readStack :: Machine -> Integer -> Either AdmissionError Scalar
readStack machine slot = do
  allocated machine slot 4
  maybe (Left UninitializedStackRead) Right (M.lookup slot (wordsAt machine))

writeStack :: Machine -> Integer -> Scalar -> Either AdmissionError Machine
writeStack machine slot value = do
  allocated machine slot 4
  pure machine { wordsAt = M.insert slot value (wordsAt machine) }

capability :: Scalar -> Bool
capability value = case value of
  Stack _ -> True
  ReturnAddress -> True
  InternalReturn _ -> True
  Guest _ _ -> True
  Service _ -> True
  _ -> False

shifted :: A.Shift -> Int -> Scalar -> Either AdmissionError Scalar
shifted A.LSL 0 value = Right value
shifted kind count value = case value of
  Constant word -> case kind of
    A.LSL -> Right (constant (word `shiftL` count))
    A.LSR -> Right (constant (word `shiftR` (if count == 0 then 32 else count)))
    A.ASR -> Right (constant (fromIntegral ((fromIntegral word :: Int32) `shiftR` (if count == 0 then 32 else count))))
    A.ROR | count /= 0 -> Right (constant (rotateR word count))
    _ -> Left UnsupportedShift
  _ -> require (not (capability value)) ArithmeticProvenanceLost >> pure Unknown

operand :: Word32 -> Machine -> A.Operand -> Either AdmissionError Scalar
operand _ _ (A.Immediate value) = Right (constant value)
operand pc machine (A.ShiftedImmediate register kind count) = shifted kind count (get pc machine register)
operand _ _ _ = Left UnsupportedShift

add :: Scalar -> Scalar -> Either AdmissionError Scalar
add (Stack slot) (Constant delta) = Right (Stack (slot+toInteger delta))
add (Constant delta) value@(Stack _) = add value (Constant delta)
add (Guest root offset) (Constant delta) = Right (Guest root ((+toInteger delta) <$> offset))
add (Constant delta) value@(Guest _ _) = add value (Constant delta)
add (Guest root _) Unknown = Right (Guest root Nothing)
add Unknown value@(Guest _ _) = add value Unknown
add (Constant left) (Constant right) = Right (constant (left+right))
add left right = require (not (capability left || capability right)) ArithmeticProvenanceLost >> pure Unknown

binary :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
binary A.MOV _ right = Right right
binary A.ADD left right = add left right
binary A.SUB (Stack slot) (Constant delta) = Right (Stack (slot-toInteger delta))
binary A.SUB (Guest root offset) (Constant delta) = Right (Guest root ((subtract (toInteger delta)) <$> offset))
binary A.SUB (Constant left) (Constant right) = Right (constant (left-right))
binary A.RSB left right = binary A.SUB right left
binary A.MVN _ (Constant right) = Right (constant (complement right))
binary operation left right = do
  require (not (capability left || capability right)) ArithmeticProvenanceLost
  case (left,right) of
    (Constant a,Constant b) -> case operation of
      A.AND -> Right (constant (a .&. b))
      A.EOR -> Right (constant (a `xor` b))
      A.ORR -> Right (constant (a .|. b))
      A.BIC -> Right (constant (a .&. complement b))
      _ -> scalar
    _ -> scalar
  where scalar = require (operation `elem` [A.SUB,A.AND,A.EOR,A.ORR,A.BIC,A.MVN]) UnsupportedInstruction >> pure Unknown

address :: Word32 -> Machine -> A.Reg -> A.Offset -> Either AdmissionError Scalar
address pc machine base displacement = case displacement of
  A.OffsetImmediate delta | delta < 0 -> binary A.SUB (get pc machine base) (Constant (fromIntegral (negate (toInteger delta))))
  A.OffsetImmediate delta -> add (get pc machine base) (Constant (fromIntegral delta))
  A.OffsetRegister positive register kind count -> do
    offset <- shifted kind count (get pc machine register)
    (if positive then add else binary A.SUB) (get pc machine base) offset

widthBytes :: A.Width -> Either AdmissionError Integer
widthBytes A.Byte = Right 1
widthBytes A.Half = Right 2
widthBytes A.Word = Right 4
widthBytes _ = Left UnsupportedInstruction

load :: B.ByteString -> A.Width -> Scalar -> Machine -> Either AdmissionError (Scalar,Machine,Maybe CodeAddress)
load bytes width location machine = do
  size <- widthBytes width
  case location of
    Stack slot -> do
      require (size == 4) StackAliasConflict
      value <- readStack machine slot
      pure (value,machine,Nothing)
    Constant at -> do
      require (width == A.Word && at >= origin && toInteger at+4 <= toInteger end && at .&. 3 == 0) ReadProvenanceRejected
      pure (constant (U.word32At bytes (fromIntegral (at-origin))),machine,Just (CodeAddress at))
    Guest Roots offset -> do
      require (width == A.Word && maybe True (\n -> n == 0 || n == 4) offset) ReadProvenanceRejected
      pure (Guest Root (Just 0),machine,Nothing)
    Guest Root offset -> do
      target <- case offset of
        Just 0x1618 -> Right CurrentCoreService
        Just 0x164 -> Right SaveMaskService
        Just 0x168 -> Right RestoreMaskService
        Just 0x80 -> Right AuxiliaryService
        Just 0 -> Right CreationService
        Just 0xc -> Right ActivationService
        _ -> Left ReadProvenanceRejected
      require (width == A.Word) ReadProvenanceRejected
      pure (Service target,machine,Nothing)
    Guest Descriptor offset -> do
      require (maybe False (\n -> n >= 0 && n+size <= 28 && n `mod` size == 0) offset) ReadProvenanceRejected
      pure (Unknown,machine,Nothing)
    Guest root _ -> do
      require (root `elem` [ClassTable,Wrapper,State]) ReadProvenanceRejected
      pure (Unknown,machine,Nothing)
    _ -> Left ReadProvenanceRejected

store :: A.Width -> Scalar -> Scalar -> Machine -> Either AdmissionError Machine
store width location value machine = do
  size <- widthBytes width
  case location of
    Stack slot -> require (size == 4) StackAliasConflict >> writeStack machine slot value
    Guest root offset -> do
      require (root `elem` [Wrapper,State,ErrorWord] && not (capability value)
        && (root /= ErrorWord || offset == Just 0) && size == 4) WriteProvenanceRejected
      pure machine
    _ -> Left WriteProvenanceRejected

mergeScalar :: Scalar -> Scalar -> Either AdmissionError Scalar
mergeScalar left right
  | left == right = Right left
mergeScalar (Guest root _) (Guest other _) | root == other = Right (Guest root Nothing)
mergeScalar left right = do
  require (not (capability left || capability right)) IncompatibleFrames
  pure Unknown

mergeMachine :: Machine -> Machine -> Either AdmissionError Machine
mergeMachine old new = do
  require (returns old == returns new) IncompatibleFrames
  registers <- M.traverseWithKey (\register value -> mergeScalar value (M.findWithDefault Unknown register (regs new))) (regs old)
  stack <- M.traverseWithKey (\slot value -> mergeScalar value (wordsAt new M.! slot)) (M.intersection (wordsAt old) (wordsAt new))
  pure (Machine registers stack (returns old))

initial :: EntryPoint -> Machine
initial entry = Machine (M.fromList ([(A.Reg n,Incoming (A.Reg n)) | n <- [0..12]] ++ [(A.Reg 0,if entry == Submission then Guest Descriptor (Just 0) else Unknown),(A.sp,Stack 0),(A.lr,ReturnAddress)])) M.empty []

inside :: NativeLayout -> Word32 -> Bool
inside layout pc = pc .&. 3 == 0 && pc /= lockLiteral && any contains (codeExtents layout)
  where contains (CodeExtent (CodeAddress start) (CodeAddress stop)) = pc >= start && toInteger pc+4 <= toInteger stop

deriveFrameProof :: NativeLayout -> B.ByteString -> Either AdmissionError FrameProof
deriveFrameProof layout bytes = do
  require (B.length bytes == 852 && U.word32At bytes 848 == 0x409c280c) InvalidCarrier
  require (not (null (codeExtents layout)) && all (\(CodeExtent (CodeAddress start) (CodeAddress stop)) ->
    start >= origin && start < stop && start .&. 3 == 0 && stop .&. 3 == 0
    && (stop <= release || (start == release && stop == release+12))) (codeExtents layout)) InvalidCarrier
  results <- mapM run [Submission,Release]
  let executed = S.unions [fetched result | result <- results]
      readLiterals = S.unions [literals result | result <- results]
      boundaries = S.unions [calls result | result <- results]
      writeSites = S.unions [writes result | result <- results]
  require (S.null (S.intersection executed readLiterals)) LiteralExecuted
  require (all (\target -> any ((== target) . callTarget) (S.toList boundaries))
    [AuxiliaryService,CreationService,ActivationService,CurrentCoreService,SaveMaskService,RestoreMaskService]) MissingCallBoundary
  let signatures = M.fromListWith S.union
        [(callInstruction boundary,S.singleton (callTarget boundary,callTargetRegister boundary,callContinuation boundary)) | boundary <- S.toList boundaries]
  require (all ((== 1) . S.size) (M.elems signatures)) CallContractRejected
  pure (FrameProof (M.fromList (zip [Submission,Release] (map (StackOffset . lowest) results))) (S.toAscList boundaries) (S.toAscList executed) (S.toAscList writeSites))
  where
    run entry = do
      let start = if entry == Submission then origin else release
      result <- walk entry (Walk M.empty [(start,initial entry)] S.empty S.empty S.empty S.empty 0 False 0)
      require (returned result) MissingReturn
      pure result
    walk _ work | null (pending work) = Right work
    walk entry work = do
      require (iterations work < 20000) WorklistBoundExceeded
      let ((pc,incoming):rest) = pending work
          key = (pc,returns incoming)
      machine <- maybe (Right incoming) (`mergeMachine` incoming) (M.lookup key (seen work))
      if M.lookup key (seen work) == Just machine then walk entry work {pending = rest}
      else do
        require (inside layout pc) EscapingControlFlow
        bottom <- spAt machine
        let advanced = work {pending = rest,seen = M.insert key machine (seen work),fetched = S.insert (CodeAddress pc) (fetched work),lowest = min bottom (lowest work),iterations = iterations work+1}
        (condition,action) <- step entry pc machine advanced (A.decode pc (U.word32At bytes (fromIntegral (pc-origin))))
        require (condition <= 14) UnsupportedCondition
        let withSkipped = if condition == 14 then action else action {pending = (pc+4,machine):pending action}
        walk entry withSkipped
    enqueue pc machine work = work {pending = (pc,machine):pending work}
    assign pc register value machine work = do
      require (register /= A.pc) EscapingControlFlow
      changed <- put register value machine
      pure (enqueue (pc+4) changed work)
    transfer entry destination machine work = case destination of
      ReturnAddress -> do
        bottom <- spAt machine
        require (bottom == 0 && null (returns machine)) BadReturn
        require (all (\n -> get 0 machine (A.Reg n) == Incoming (A.Reg n)) ([4..11] ++ if entry == Release then [3] else [])) PreservedRegisterDiffers
        pure work {returned = True}
      InternalReturn target -> case returns machine of
        ReturnFrame expected savedSp saved:outer | expected == target -> do
          bottom <- spAt machine
          require (bottom == savedSp) BadReturn
          require (map (get 0 machine . A.Reg) [4..11] == saved) PreservedRegisterDiffers
          pure (enqueue target machine {returns = outer} work)
        _ -> Left BadReturn
      _ -> Left BadReturn
    external entry pc target targetRegister machine work = do
      bottom <- spAt machine
      require (bottom `mod` 8 == 0) CallStackUnaligned
      mapM_ (\value -> case value of
        Stack slot -> do
          require (slot >= bottom && slot < 0 && slot `mod` 4 == 0) StackAliasConflict
          _ <- readStack machine slot
          pure ()
        _ -> pure ()) [get pc machine (A.Reg n) | n <- [0..3]]
      let parameters count = do
            slot <- case get pc machine (A.Reg 1) of Stack value -> Right value; _ -> Left CallContractRejected
            values <- mapM (readStack machine) (take count [slot,slot+4..])
            pure (zipWith (\offset value -> ParameterWord (StackOffset offset) (provenance value)) [slot,slot+4..] values)
      arguments <- case target of
        FixedService _ -> pure []
        _ -> do
          require (maybe False (\register -> get pc machine register == Service target) targetRegister) ExternalCallRejected
          require (target `elem` [CurrentCoreService,SaveMaskService] || not (capability (get pc machine (A.Reg 0)))) CallContractRejected
          case target of
            AuxiliaryService -> parameters 2
            CreationService -> parameters 6
            _ -> pure []
      let boundary = CallBoundary entry (CodeAddress pc) (CodeAddress (pc+4)) target targetRegister
            (provenance (get pc machine (A.Reg 0))) (provenance (get pc machine (A.Reg 1))) arguments
            (StackOffset bottom) (map (StackOffset . fst) (M.toAscList (wordsAt machine)))
          changed = machine {regs = foldr (\register -> M.insert register Unknown) (regs machine) (map A.Reg [0,1,2,3,12,14])}
      pure (enqueue (pc+4) changed work {calls = S.insert boundary (calls work)})
    branch entry pc linked targetRegister target machine work
      | inside layout target = do
          require (length (returns machine) < 16) WorklistBoundExceeded
          bottom <- spAt machine
          changed <- if linked then put A.lr (InternalReturn (pc+4)) machine else Right machine
          let saved = ReturnFrame (pc+4) bottom (map (get pc machine . A.Reg) [4..11])
          pure (enqueue target (if linked then changed {returns = saved:returns machine} else changed) work)
      | otherwise = do
          require linked EscapingControlFlow
          targetName <- maybe (Left ExternalCallRejected) Right (fixed target)
          external entry pc targetName targetRegister machine work
    step entry pc machine work instruction = case instruction of
      A.MoveWide (A.Cond cond) destination value -> (,) cond <$> assign pc destination (constant value) machine work
      A.MoveTop (A.Cond cond) destination value -> do
        old <- case get pc machine destination of Constant word -> Right word; _ -> Left ArithmeticProvenanceLost
        (,) cond <$> assign pc destination (constant ((old .&. 0xffff) .|. (value `shiftL` 16))) machine work
      A.DataProcessing (A.Cond cond) operation updates destination source value -> do
        right <- operand pc machine value
        if operation `elem` [A.CMP,A.CMN,A.TST,A.TEQ] then do
          require updates UnsupportedInstruction
          pure (cond,enqueue (pc+4) machine work)
        else do
          result <- binary operation (get pc machine source) right
          (,) cond <$> assign pc destination result machine work
      A.ZeroExtend (A.Cond cond) destination source rotation bits -> do
        require (rotation == 0 && bits `elem` [8,16]) UnsupportedInstruction
        value <- case get pc machine source of
          Constant word -> Right (constant (word .&. ((1 `shiftL` bits)-1)))
          other -> require (not (capability other)) ArithmeticProvenanceLost >> pure Unknown
        (,) cond <$> assign pc destination value machine work
      A.Multiply (A.Cond cond) destination left right accumulator -> do
        require (all (not . capability . get pc machine) ([left,right] ++ maybe [] (:[]) accumulator)) ArithmeticProvenanceLost
        (,) cond <$> assign pc destination Unknown machine work
      A.Load (A.Cond cond) width signed destination base displacement indexing -> do
        require (not signed && destination /= A.pc && (indexing == A.OffsetOnly || base /= destination)) UnsupportedInstruction
        location <- address pc machine base displacement
        let actual = if indexing == A.PostIndexed then get pc machine base else location
        allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base location machine else Right machine
        (value,loaded,literal) <- load bytes width actual allocatedMachine
        updated <- if indexing == A.OffsetOnly then Right loaded else put base location loaded
        next <- assign pc destination value updated work {literals = maybe (literals work) (`S.insert` literals work) literal}
        pure (cond,next)
      A.Store (A.Cond cond) width source base displacement indexing -> do
        require (indexing == A.OffsetOnly || source /= base) UnsupportedInstruction
        location <- address pc machine base displacement
        let actual = if indexing == A.PostIndexed then get pc machine base else location
        allocatedMachine <- if base == A.sp && indexing == A.PreIndexed then put base location machine else Right machine
        stored <- store width actual (get pc machine source) allocatedMachine
        updated <- if indexing == A.OffsetOnly then Right stored else put base location stored
        let observed = case actual of Guest _ _ -> work {writes=S.insert (CodeAddress pc) (writes work)}; _ -> work
        pure (cond,enqueue (pc+4) updated observed)
      A.StoreMultiple (A.Cond cond) base A.DecrementBefore True saved -> do
        require (base == A.sp && not (null saved) && base `notElem` saved && A.pc `notElem` saved) UnsupportedInstruction
        bottom <- spAt machine
        let allocatedBottom = bottom-4*toInteger (length saved)
        allocatedMachine <- put A.sp (Stack allocatedBottom) machine
        stored <- foldM (\state (slot,register) -> writeStack state slot (get pc machine register)) allocatedMachine (zip [allocatedBottom,allocatedBottom+4..] saved)
        pure (cond,enqueue (pc+4) stored work {lowest = min allocatedBottom (lowest work)})
      A.LoadMultiple (A.Cond cond) base A.IncrementAfter True restored -> do
        require (base == A.sp && not (null restored) && base `notElem` restored) UnsupportedInstruction
        bottom <- spAt machine
        values <- mapM (readStack machine) (take (length restored) [bottom,bottom+4..])
        loaded <- foldM (\state (register,value) -> if register == A.pc then Right state else put register value state) machine (zip restored values)
        changed <- put A.sp (Stack (bottom+4*toInteger (length restored))) loaded
        next <- case lookup A.pc (zip restored values) of
          Nothing -> Right (enqueue (pc+4) changed work)
          Just destination -> transfer entry destination changed work
        pure (cond,next)
      A.Branch (A.Cond cond) linked target -> (,) cond <$> branch entry pc linked Nothing target machine work
      A.BranchExchange (A.Cond cond) linked register -> do
        next <- if not linked then transfer entry (get pc machine register) machine work else case get pc machine register of
          Service target -> external entry pc target (Just register) machine work
          Constant target -> branch entry pc True (Just register) target machine work
          _ -> Left ExternalCallRejected
        pure (cond,next)
      _ -> Left UnsupportedInstruction

data StackInterval = StackInterval StackOffset StackOffset deriving (Eq, Show)
data StackContext = StackContext
  { incomingStackAddress :: Word32, writableStack :: StackInterval, liveObjects :: [StackInterval] }
  deriving (Eq, Show)
data CalleeDemand = CalleeDemand CallTarget StackOffset deriving (Eq, Show)
data JoinedStack = JoinedStack { joinedLow :: StackOffset, joinedBoundaries :: [CallBoundary] } deriving (Eq, Show)

joinStack :: FrameProof -> EntryPoint -> StackContext -> [CalleeDemand] -> Either AdmissionError JoinedStack
joinStack proof entry context demands = do
  let StackOffset nativeLow = scratchLow proof entry
      boundaries = filter ((== entry) . callEntry) (callBoundaries proof)
      StackInterval (StackOffset writableLow) (StackOffset writableHigh) = writableStack context
      absolute = toInteger (incomingStackAddress context)
      validInterval (StackInterval (StackOffset low) (StackOffset high)) = low < high && low `mod` 4 == 0 && high `mod` 4 == 0
      overlaps low high (StackInterval (StackOffset otherLow) (StackOffset otherHigh)) = low < high && low < otherHigh && otherLow < high
      checkRange low high = do
        require (absolute+low >= 0 && absolute+high <= 0x100000000) StackAddressOverflow
        require (low >= writableLow && high <= writableHigh) StackBackingExceeded
        require (not (any (overlaps low high) (liveObjects context))) NestedStackOverlap
  require (incomingStackAddress context .&. 7 == 0 && validInterval (writableStack context) && all validInterval (liveObjects context)) InvalidStackContext
  require (length (nub [target | CalleeDemand target _ <- demands]) == length demands) MissingCalleeDemand
  checkRange nativeLow 0
  lows <- mapM (\boundary -> do
    let StackOffset actual = callStack boundary
        live = [slot | StackOffset slot <- callLiveWords boundary]
    require ((absolute+actual) `mod` 8 == 0) CallStackUnaligned
    StackOffset demand <- maybe (Left MissingCalleeDemand) Right (lookup (callTarget boundary) [(target,low) | CalleeDemand target low <- demands])
    require (demand <= 0 && demand `mod` 4 == 0) InvalidStackContext
    let low = actual+demand
    checkRange low actual
    require (all (\slot -> slot >= actual && slot+4 <= 0 && slot `mod` 4 == 0 && not (slot < actual && low < slot+4)) live) NestedStackOverlap
    pure low) boundaries
  pure (JoinedStack (StackOffset (minimum (nativeLow:lows))) (sort boundaries))
