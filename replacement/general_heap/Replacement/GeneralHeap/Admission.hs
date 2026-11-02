{-# LANGUAGE OverloadedStrings #-}
module Replacement.GeneralHeap.Admission
  ( AdmissionError (..), Carrier (..), Image (..), FrameProof, carriers, entrySymbols, retainedCallees
  , deriveFrames, frameValue, instructionPCs, literalWords, validateStockHeader, validateFetch
  , BackendGeometry, ArenaLayout, decodeGeometry, geometryValue, arenaValue
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError = CarrierRejected | SymbolRejected | InstructionRejected Word32 | StackRejected Word32 | HeaderRejected | FetchRejected | GeometryRejected deriving (Eq, Show)
data Image = Image4 | Image5 deriving (Eq, Show)
data Carrier = Carrier Image Word32 Word32 deriving (Eq, Show)
data CallBoundary = CallBoundary Word32 Word32 Int deriving (Eq, Ord, Show)
data FrameProof = FrameProof Word32 Int [CallBoundary] [Word32] [Word32] [(Word32,Word32)]
data BackendGeometry = Rlsf023U32U16F17S16 deriving (Eq, Show)
data ArenaLayout = ArenaLayout Word32 Word32 Word32 Word32 deriving (Eq, Show)

decodeGeometry :: [Word32] -> Either AdmissionError (BackendGeometry,ArenaLayout)
decodeGeometry [magic,version,base,end,state,pool,guard,first,second,granularity,prefix,alignment,firstBits,secondBits] = do
  unless ([magic,version,base,guard,first,second,granularity,prefix,alignment,firstBits,secondBits]
    == [0x48454150,1,0xa71a00,32,17,16,16,16,8,32,16]
    && end `elem` [0xbf1a00,0xbe1a00]
    && state >= 17*16*4 && state < end-base-guard
    && toInteger pool == ((toInteger base + toInteger state + 15) `div` 16)*16
    && pool < end-guard) (Left GeometryRejected)
  pure (Rlsf023U32U16F17S16,ArenaLayout base end state pool)
decodeGeometry _ = Left GeometryRejected

geometryValue :: BackendGeometry -> Value
geometryValue Rlsf023U32U16F17S16 = object
  ["first_level_bins" .= (17::Int),"second_level_bins" .= (16::Int),"granularity" .= (16::Int)
  ,"stock_prefix_bytes" .= (16::Int),"stock_alignment" .= (8::Int)
  ,"first_level_bitmap_bits" .= (32::Int),"second_level_bitmap_bits" .= (16::Int)]

arenaValue :: ArenaLayout -> Value
arenaValue (ArenaLayout base end state pool) = object
  ["base" .= base,"end" .= end,"guard_bytes" .= (32::Int),"state_bytes" .= state
  ,"state_start" .= base,"state_end" .= (base+state),"pool_start" .= pool,"pool_end" .= (end-32)]

carriers :: [Carrier]
carriers = [Carrier Image4 0x89cda4 0x89d4f8, Carrier Image4 0x89d4fc 0x89d908, Carrier Image4 0x89da3c 0x89dfc0, Carrier Image4 0x89e464 0x89e7c4, Carrier Image5 0xbf1c1c 0xbf1cd4]

entrySymbols :: [(String, Word32)]
entrySymbols = [("xm1_heap_malloc_entry",0x89cda4),("xm1_heap_free_entry",0x89cdc4),("xm1_heap_malloc_r_entry",0x89cde4),("xm1_heap_realloc_entry",0x89da3c),("xm1_heap_realloc_r_entry",0x89da64),("xm1_heap_trim_r_entry",0x89e464),("xm1_heap_free_r_entry",0x89e560),("xm1_heap_calloc_entry",0xbf1c1c),("xm1_heap_calloc_r_entry",0xbf1c44)]

-- Stock recursive lock, unlock and memmove.
retainedCallees :: [Word32]
retainedCallees = [0x72b164,0x72b1cc,0x89d908]

instructionPCs :: [FrameProof] -> [Word32]
instructionPCs proofs = S.toAscList (S.fromList [pc | FrameProof _ _ _ _ pcs _ <- proofs, pc <- pcs])
literalWords :: [FrameProof] -> [(Word32,Word32)]
literalWords proofs = M.toAscList (M.fromList [word | FrameProof _ _ _ _ _ words' <- proofs, word <- words'])

validateStockHeader :: Word32 -> Word32 -> Either AdmissionError Word32
validateStockHeader available tag = do
  let chunk = tag .&. 0xfffffffc
  unless (tag .&. 3 == 1 && chunk >= 4 && chunk - 4 <= available) (Left HeaderRejected)
  pure (chunk - 4)

validateFetch :: [FrameProof] -> Word32 -> Word32 -> Word32 -> Either AdmissionError ()
validateFetch proofs pc expected actual = unless (pc `elem` instructionPCs proofs && expected == actual) (Left FetchRejected)

frameValue :: FrameProof -> Value
frameValue (FrameProof entry bottom calls returns pcs literals) = object
  ["entry" .= entry, "stack_low" .= bottom, "returns" .= returns, "instruction_pcs" .= pcs
  ,"call_boundaries" .= [object ["pc" .= pc,"target" .= target,"sp_offset" .= offset] | CallBoundary pc target offset <- calls]
  ,"literal_words" .= [object ["address" .= address,"value" .= value] | (address,value) <- literals]]

-- Calls retain only their boundary metadata; callee register effects are a conditional premise.
deriveFrames :: B.ByteString -> [Word32] -> Either AdmissionError [FrameProof]
deriveFrames bytes entries = mapM derive entries
  where
    base = 0x89cda4
    inside pc = pc .&. 3 == 0 && pc /= 0x89d4f8 && any (\(Carrier _ low high) -> low <= pc && pc < high) carriers
    word pc = if pc >= base && toInteger pc + 4 <= toInteger base + toInteger (B.length bytes)
      then Right (U.word32At bytes (fromIntegral (pc-base))) else Left CarrierRejected
    derive entry = walk entry S.empty [(entry,0,Nothing)] 0 [] [] [] []
    walk entry _ [] bottom calls returns pcs literals = Right (FrameProof entry bottom (S.toList (S.fromList calls)) (S.toList (S.fromList returns)) (S.toList (S.fromList pcs)) (M.toList (M.fromList literals)))
    walk entry seen ((pc,sp,fp):pending) bottom calls returns pcs literals
      | S.member (pc,sp,fp) seen = walk entry seen pending bottom calls returns pcs literals
      | S.size seen > 100000 || not (inside pc) = Left (InstructionRejected pc)
      | otherwise = do
          encoded <- word pc
          let instruction = A.decode pc encoded
              seen' = S.insert (pc,sp,fp) seen
              next offset frame = [(pc+4,offset,frame)]
              finish paths low calls' returns' literals' = walk entry seen' (paths ++ pending) (min bottom low) (calls' ++ calls) (returns' ++ returns) (pc:pcs) (literals' ++ literals)
              conditional (A.Cond condition) paths = paths ++ [state | condition /= 14, state <- next sp fp]
              stack offset = unless (offset <= 0 && offset >= -65536 && offset .&. 3 == 0) (Left (StackRejected pc))
              ret condition offset = do
                unless (offset == 0) (Left (StackRejected pc))
                finish (conditional condition []) offset [] [pc] []
              call condition target = do
                unless (pc >= 0xbf1a00 || target < 0xbf1a00) (Left (InstructionRejected pc))
                unless (sp .&. 7 == 0 && (inside target || target `elem` retainedCallees)) (Left (StackRejected pc))
                finish (conditional condition (next sp fp)) sp [CallBoundary pc target sp] [] []
          case instruction of
            A.Rejected _ -> Left (InstructionRejected pc)
            A.ReadStatus _ _ -> Left (InstructionRejected pc)
            A.WriteApplicationFlags _ _ -> Left (InstructionRejected pc)
            A.Branch condition True target -> call condition target
            A.Branch condition False target
              | target `elem` retainedCallees -> do
                  unless (sp == 0) (Left (StackRejected pc))
                  finish (conditional condition []) sp [CallBoundary pc target sp] [pc] []
              | pc < 0xbf1a00 && target >= 0xbf1a00 -> Left (InstructionRejected pc)
              | otherwise -> finish (conditional condition [(target,sp,fp)]) sp [] [] []
            A.BranchExchange condition False register | register == A.lr -> ret condition sp
            A.BranchExchange {} -> Left (InstructionRejected pc)
            A.StoreMultiple condition register A.DecrementBefore True registers | register == A.sp -> do
              let offset = sp - 4 * length registers
              stack offset
              finish (conditional condition (next offset fp)) offset [] [] []
            A.LoadMultiple condition register A.IncrementAfter True registers | register == A.sp -> do
              let offset = sp + 4 * length registers
              stack offset
              if A.pc `elem` registers then ret condition offset else finish (conditional condition (next offset (if A.Reg 11 `elem` registers then Nothing else fp))) offset [] [] []
            A.DataProcessing condition operation _ destination source (A.Immediate amount) | destination == A.sp && source == A.sp && operation `elem` [A.ADD,A.SUB] -> do
              let offset = sp + (if operation == A.ADD then 1 else -1) * fromIntegral amount
              stack offset
              finish (conditional condition (next offset fp)) offset [] [] []
            A.DataProcessing condition A.ADD _ (A.Reg 11) source (A.Immediate amount) | source == A.sp -> finish (conditional condition (next sp (Just (sp + fromIntegral amount)))) sp [] [] []
            A.DataProcessing condition A.SUB _ destination (A.Reg 11) (A.Immediate amount) | destination == A.sp -> case fp of
              Nothing -> Left (StackRejected pc)
              Just saved -> do
                let offset = saved - fromIntegral amount
                stack offset
                finish (conditional condition (next offset fp)) offset [] [] []
            A.DataProcessing condition A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) | destination == A.pc && source == A.lr -> ret condition sp
            A.DataProcessing _ _ _ destination _ _ | destination `elem` [A.pc,A.sp] -> Left (StackRejected pc)
            A.Store condition A.Word _ register (A.OffsetImmediate amount) indexing | register == A.sp && indexing /= A.OffsetOnly -> do
              let offset = sp + fromIntegral amount
              stack offset
              finish (conditional condition (next offset fp)) offset [] [] []
            A.Load condition A.Word _ destination register (A.OffsetImmediate amount) indexing | register == A.sp && indexing /= A.OffsetOnly -> do
              let offset = sp + fromIntegral amount
              stack offset
              if destination == A.pc then ret condition offset else do
                unless (destination /= A.sp) (Left (StackRejected pc))
                finish (conditional condition (next offset (if destination == A.Reg 11 then Nothing else fp))) offset [] [] []
            A.Load condition width _ _ register (A.OffsetImmediate offset) A.OffsetOnly | register == A.pc -> do
              let destinations = writtenRegisters instruction
              unless (all (`notElem` destinations) [A.pc,A.sp] && width `elem` [A.Word,A.Dual]) (Left (InstructionRejected pc))
              let address = fromInteger (toInteger pc + 8 + toInteger offset)
                  addresses = if width == A.Dual then [address,address+4] else [address]
              unless (all inside addresses && (pc >= 0xbf1a00 || all (< 0xbf1a00) addresses)) (Left (InstructionRejected pc))
              values <- mapM word addresses
              finish (conditional condition (next sp (if A.Reg 11 `elem` destinations then Nothing else fp))) sp [] [] (zip addresses values)
            A.Load _ _ _ destination _ _ _ | destination `elem` [A.pc,A.sp] -> Left (StackRejected pc)
            A.LoadMultiple _ _ _ _ registers | any (`elem` registers) [A.pc,A.sp] -> Left (StackRejected pc)
            _ -> do
              let destinations = writtenRegisters instruction
              unless (all (`notElem` destinations) [A.pc,A.sp]) (Left (StackRejected pc))
              finish (next sp (if A.Reg 11 `elem` destinations then Nothing else fp)) sp [] [] []

writtenRegisters :: A.Instruction -> [A.Reg]
writtenRegisters instruction = case instruction of
  A.DataProcessing _ operation _ destination _ _ -> [destination | operation `notElem` [A.CMP,A.CMN,A.TST,A.TEQ]]
  A.MoveWide _ destination _ -> [destination]
  A.MoveTop _ destination _ -> [destination]
  A.ZeroExtend _ destination _ _ _ -> [destination]
  A.Multiply _ destination _ _ _ -> [destination]
  A.Compute _ destinations -> destinations
  A.Load _ width _ destination@(A.Reg number) base _ indexing ->
    destination : [A.Reg (number+1) | width == A.Dual] ++ [base | indexing /= A.OffsetOnly]
  A.Store _ _ _ base _ indexing -> [base | indexing /= A.OffsetOnly]
  A.LoadMultiple _ base _ writeback destinations -> destinations ++ [base | writeback]
  A.StoreMultiple _ base _ writeback _ -> [base | writeback]
  _ -> []
