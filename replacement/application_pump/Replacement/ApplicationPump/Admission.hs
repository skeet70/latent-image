module Replacement.ApplicationPump.Admission
  ( AdmissionError (..), PcWriteForm (..), RetainedCall (..), FrameProof
  , deriveFrameProof, retainedCalls, scratchLow, proofCallSites, returnInstruction
  , CallSite (..), CodeAddress (..), StackOffset (..)
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data PcWriteForm = DataDestination | LoadDestination | MultipleDestination | WritebackDestination deriving (Eq, Show)
data AdmissionError
  = CarrierRejected | UnmodelledPcWrite PcWriteForm | ConditionalInstruction
  | ConditionalTransfer | LoopRejected | UnexpectedTransfer | ForbiddenStockFallback
  | UnexpectedCall | CallOrderRejected | StackRejected | MemoryEffectRejected
  | UnsupportedInstruction | ReturnRejected | ResultRejected | CalleeSavedRejected
  | TrailingCodeRejected
  deriving (Eq, Show)
data RetainedCall = Prehook | Normalizer | Posthook deriving (Eq, Show)
newtype StackOffset = StackOffset Int deriving (Eq, Show)
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Show)
data CallSite = CallSite
  { callPhase :: RetainedCall, callInstruction :: CodeAddress, callTarget :: CodeAddress
  , callReturn :: CodeAddress, callStack :: StackOffset
  } deriving (Eq, Show)
data Scalar = Incoming A.Reg | Stack StackOffset | CallerReturn | NormalizerResult | Volatile RetainedCall | Constant Word32 deriving (Eq, Show)
data Machine = Machine
  { registers :: M.Map A.Reg Scalar
  , memory :: M.Map Int Scalar
  , calls :: [RetainedCall]
  , sites :: [CallSite]
  , lowest :: Int
  }
data FrameProof = FrameProof Int [CallSite] CodeAddress

scratchLow :: FrameProof -> Int
scratchLow (FrameProof low _ _) = low

retainedCalls :: FrameProof -> [RetainedCall]
retainedCalls = map callPhase . proofCallSites

proofCallSites :: FrameProof -> [CallSite]
proofCallSites (FrameProof _ ordered _) = ordered

returnInstruction :: FrameProof -> CodeAddress
returnInstruction (FrameProof _ _ address) = address

require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise refusal = unless premise (Left refusal)

get :: Machine -> A.Reg -> Scalar
get machine register = M.findWithDefault (Incoming register) register (registers machine)

put :: A.Reg -> Scalar -> Machine -> Either AdmissionError Machine
put register value machine = do
  require (register /= A.pc) (UnmodelledPcWrite DataDestination)
  case (register,value) of
    (A.Reg 13,Stack (StackOffset offset)) -> do
      require (offset >= -64 && offset <= 0 && offset .&. 3 == 0) StackRejected
      pure machine
        { registers = M.insert register value (registers machine)
        , memory = M.filterWithKey (\address _ -> address >= offset) (memory machine)
        , lowest = min (lowest machine) offset
        }
    (A.Reg 13,_) -> Left StackRejected
    _ -> pure machine {registers = M.insert register value (registers machine)}

stackPointer :: Machine -> Either AdmissionError Int
stackPointer machine = case get machine A.sp of
  Stack (StackOffset offset) -> Right offset
  _ -> Left StackRejected

allocated :: Machine -> Int -> Either AdmissionError ()
allocated machine address = do
  root <- stackPointer machine
  require (address >= root && address < 0 && address .&. 3 == 0) MemoryEffectRejected

loadStack :: Machine -> Int -> Either AdmissionError Scalar
loadStack machine address = do
  allocated machine address
  maybe (Left MemoryEffectRejected) Right (M.lookup address (memory machine))

storeStack :: Machine -> (Int,Scalar) -> Either AdmissionError Machine
storeStack machine (address,value) = do
  allocated machine address
  pure machine {memory = M.insert address value (memory machine)}

operand :: Machine -> A.Operand -> Either AdmissionError Scalar
operand _ (A.Immediate word) = Right (Constant word)
operand machine (A.ShiftedImmediate register A.LSL 0) = do
  require (register /= A.pc) UnexpectedTransfer
  pure (get machine register)
operand _ _ = Left UnsupportedInstruction

arithmetic :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
arithmetic A.MOV _ right = Right right
arithmetic A.ADD (Stack (StackOffset base)) (Constant delta) | delta <= 64 = Right (Stack (StackOffset (base + fromIntegral delta)))
arithmetic A.SUB (Stack (StackOffset base)) (Constant delta) | delta <= 64 = Right (Stack (StackOffset (base - fromIntegral delta)))
arithmetic _ _ _ = Left UnsupportedInstruction

entry :: Word32
entry = 0xbfbd20

-- Every accepted path advances by one instruction; the only terminal transfer restores CallerReturn.
deriveFrameProof :: B.ByteString -> Either AdmissionError FrameProof
deriveFrameProof bytes = do
  require (not (B.null bytes) && B.length bytes <= 32 && B.length bytes `mod` 4 == 0) CarrierRejected
  walk entry (Machine (M.fromList [(A.sp,Stack (StackOffset 0)),(A.lr,CallerReturn)]) M.empty [] [] 0)
  where
    end = entry + fromIntegral (B.length bytes)
    finish pc target machine = do
      require (pc + 4 == end) TrailingCodeRejected
      require (target == CallerReturn) ReturnRejected
      require (calls machine == [Prehook,Normalizer,Posthook]) CallOrderRejected
      require (get machine (A.Reg 0) == NormalizerResult) ResultRejected
      require (get machine A.sp == Stack (StackOffset 0)) StackRejected
      require (all (\n -> get machine (A.Reg n) == Incoming (A.Reg n)) [4..11]) CalleeSavedRejected
      pure (FrameProof (lowest machine) (sites machine) (CodeAddress pc))
    retained pc target machine = do
      phase <- case target of
        0xbfb1b8 -> Right Prehook
        0xbfb20c -> Right Normalizer
        0xbfb1e0 -> Right Posthook
        _ | target >= entry && target < entry + 32 -> Left ForbiddenStockFallback
          | otherwise -> Left UnexpectedCall
      require (calls machine ++ [phase] == take (length (calls machine) + 1) [Prehook,Normalizer,Posthook]
        && length (calls machine) < 3) CallOrderRejected
      root <- stackPointer machine
      require (root < 0 && root .&. 7 == 0) StackRejected
      clobbered <- foldM (\state register -> put register (Volatile phase) state) machine (map A.Reg [0,1,2,3,12,14])
      changed <- put (A.Reg 0) (if phase == Normalizer then NormalizerResult else Volatile phase) clobbered
      pure changed {calls = calls machine ++ [phase]
        , sites = sites machine ++ [CallSite phase (CodeAddress pc) (CodeAddress target) (CodeAddress (pc+4)) (StackOffset root)]}
    address machine base displacement = case (get machine base,displacement) of
      (Stack (StackOffset root),A.OffsetImmediate delta) -> Right (root + fromIntegral delta)
      _ -> Left MemoryEffectRejected
    walk pc machine = do
      require (pc >= entry && pc < end && pc .&. 3 == 0) ReturnRejected
      let word = U.word32At bytes (fromIntegral (pc-entry))
          decoded = A.decode pc word
          next = walk (pc+4)
          ordinary destination = require (destination /= A.pc) (UnmodelledPcWrite DataDestination)
          writeback base indexing = require (base /= A.pc || indexing == A.OffsetOnly) (UnmodelledPcWrite WritebackDestination)
      case decoded of
        A.Rejected _ -> Left UnsupportedInstruction
        A.DataProcessing _ _ _ destination _ _ -> ordinary destination
        A.MoveWide _ destination _ -> ordinary destination
        A.MoveTop _ destination _ -> ordinary destination
        A.ZeroExtend _ destination _ _ _ -> ordinary destination
        A.ReadStatus _ destination -> ordinary destination
        A.Multiply _ destination _ _ _ -> ordinary destination
        A.Compute _ destinations -> mapM_ ordinary destinations
        A.Load _ _ _ destination base _ indexing -> do
          require (destination /= A.pc) (UnmodelledPcWrite LoadDestination)
          writeback base indexing
        A.Store _ _ _ base _ indexing -> writeback base indexing
        A.StoreMultiple _ base _ updates saved -> do
          require (not updates || base /= A.pc) (UnmodelledPcWrite WritebackDestination)
          require (A.pc `notElem` saved) UnexpectedTransfer
        A.LoadMultiple _ base mode updates restored -> do
          require (not updates || base /= A.pc) (UnmodelledPcWrite WritebackDestination)
          require (A.pc `notElem` restored || (base == A.sp && mode == A.IncrementAfter && updates
            && A.sp `notElem` restored && A.lr `notElem` restored)) (UnmodelledPcWrite MultipleDestination)
        _ -> pure ()
      require (word `shiftR` 28 == 14) (case decoded of
        A.Branch {} -> ConditionalTransfer
        A.BranchExchange {} -> ConditionalTransfer
        A.LoadMultiple _ _ _ _ restored | A.pc `elem` restored -> ConditionalTransfer
        _ -> ConditionalInstruction)
      case decoded of
        A.DataProcessing _ operation False destination source value -> do
          require (source /= A.pc) UnexpectedTransfer
          right <- operand machine value
          result <- arithmetic operation (get machine source) right
          put destination result machine >>= next
        A.StoreMultiple _ base A.DecrementBefore True saved -> do
          require (base == A.sp && not (null saved) && A.sp `notElem` saved) MemoryEffectRejected
          root <- stackPointer machine
          let bottom = root - 4 * length saved
          allocatedMachine <- put A.sp (Stack (StackOffset bottom)) machine
          foldM storeStack allocatedMachine (zip [bottom,bottom+4..] (map (get machine) saved)) >>= next
        A.LoadMultiple _ base A.IncrementAfter True restored -> do
          require (base == A.sp && not (null restored) && A.sp `notElem` restored) MemoryEffectRejected
          root <- stackPointer machine
          values <- mapM (loadStack machine) (take (length restored) [root,root+4..])
          loaded <- foldM (\state (register,value) -> if register == A.pc then pure state else put register value state)
            machine (zip restored values)
          changed <- put A.sp (Stack (StackOffset (root + 4 * length restored))) loaded
          case lookup A.pc (zip restored values) of
            Just target -> finish pc target changed
            Nothing -> next changed
        A.Store _ A.Word source base displacement A.OffsetOnly -> do
          require (source /= A.pc) UnexpectedTransfer
          slot <- address machine base displacement
          storeStack machine (slot,get machine source) >>= next
        A.Load _ A.Word False destination base displacement A.OffsetOnly -> do
          slot <- address machine base displacement
          value <- loadStack machine slot
          put destination value machine >>= next
        A.Branch _ True target -> retained pc target machine >>= next
        A.Branch _ False target
          | target <= pc -> Left LoopRejected
          | target >= entry && target < entry + 32 -> Left UnexpectedTransfer
          | otherwise -> Left ForbiddenStockFallback
        A.BranchExchange _ False register -> finish pc (get machine register) machine
        A.BranchExchange {} -> Left UnexpectedTransfer
        _ -> Left UnsupportedInstruction
