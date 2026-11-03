{-# LANGUAGE OverloadedStrings #-}
module Replacement.GeneralHeap.Admission
  ( AdmissionError (..), Carrier (..), Image (..), FrameProof, carriers, entrySymbols, retainedCallees
  , deriveFrames, frameValue, instructionPCs, literalWords, validateStockHeader, validateFetch
  , BackendGeometry, ArenaLayout, decodeGeometry, geometryValue, arenaValue
  , RedesignItem (..), redesignId, redesignValue
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftR, xor, (.&.))
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

-- Approved intentional differences from the stock allocator.
data RedesignItem = CheckedCallocProduct | OversizedResizeRejectedBeforeLock | ZeroRequestMinimumAllocation | TrimReturnsZero
  deriving (Eq, Ord, Show, Enum, Bounded)

redesignId :: RedesignItem -> String
redesignId item = case item of
  CheckedCallocProduct -> "checked-calloc-product"
  OversizedResizeRejectedBeforeLock -> "oversized-resize-rejected-before-lock"
  ZeroRequestMinimumAllocation -> "zero-request-minimum-allocation"
  TrimReturnsZero -> "trim-returns-zero"

redesignValue :: RedesignItem -> Value
redesignValue item = object ["id" .= redesignId item,"stock" .= stock,"owned" .= owned,"why" .= why,"dependence_signal" .= signal]
  where
    (stock,owned,why,signal) = case item of
      CheckedCallocProduct ->
        ("calloc 0xbf1c44 computes count*size with mul, modulo 2^32, and allocates the wrapped size" :: String
        ,"a product above 2^32-1 returns null with errno 12 before locking" :: String
        ,"a wrapped product hands the caller fewer bytes than count*size" :: String
        ,"a calloc whose 64-bit request exceeds 0xffffffff succeeds in stock and returns null in owned" :: String)
      OversizedResizeRejectedBeforeLock ->
        ("resize of a live allocation to [0x7ffffff5,0xffffffff] writes errno 12, returns null and leaks one recursive lock depth (#147)"
        ,"the same requests return null with errno 12 without acquiring the lock"
        ,"the leaked depth keeps the heap lock owned by the caller's task"
        ,"a realloc request above 0x7ffffff4; stock returns with lock depth one higher than at entry")
      ZeroRequestMinimumAllocation ->
        ("zero-byte malloc takes the minimum 16-byte chunk (12 usable bytes, tag 0x11); realloc(p, 0) at 0x89da80..0x89dad0 rounds to the same minimum, does not free, and shrinks p in place, returning p"
        ,"zero-byte malloc returns the same 12-byte usable extent and tag from rlsf behind a 16-byte owned prefix; realloc(p, 0) returns a live 12-byte allocation that rlsf may move"
        ,"the stock tag and usable extent stay readable at payload-4 for retained header readers"
        ,"a realloc(p, 0) caller relying on the result equalling p, or a zero-byte call whose success or usable extent differs")
      TrimReturnsZero ->
        ("trim may release page-rounded top backing through 0x72b110 and returns 1 when it does"
        ,"trim takes and releases the lock and returns 0 without releasing backing"
        ,"the owned pool is the fixed charged arena with no backing break to lower"
        ,"a trim that returns 1 in stock and 0 in owned, or a later backing request expecting released memory")

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
-- Each path carries the last evaluated condition and its outcome until an instruction may write
-- the flags, so a predicated epilogue is followed only on the paths where it executes.
-- A call to an entry without a reachable return has no fallthrough. Nonreturning entries are the
-- greatest fixpoint: every entry starts nonreturning, and explored paths only grow as entries leave the set.
deriveFrames :: B.ByteString -> [Word32] -> Either AdmissionError [FrameProof]
deriveFrames bytes entries = settle (S.fromList entries)
  where
    settle noreturn = do
      frames <- mapM (derive noreturn) entries
      let remaining = S.fromList [entry | FrameProof entry _ _ returns _ _ <- frames, null returns]
      if remaining == noreturn then pure frames else settle remaining
    base = 0x89cda4
    inside pc = pc .&. 3 == 0 && pc /= 0x89d4f8 && any (\(Carrier _ low high) -> low <= pc && pc < high) carriers
    word pc = if pc >= base && toInteger pc + 4 <= toInteger base + toInteger (B.length bytes)
      then Right (U.word32At bytes (fromIntegral (pc-base))) else Left CarrierRejected
    derive noreturn entry = walk noreturn entry S.empty [(entry,0,Nothing,Nothing)] 0 [] [] [] []
    walk _ entry _ [] bottom calls returns pcs literals = Right (FrameProof entry bottom (S.toList (S.fromList calls)) (S.toList (S.fromList returns)) (S.toList (S.fromList pcs)) (M.toList (M.fromList literals)))
    walk noreturn entry seen (state@(pc,sp,fp,known):pending) bottom calls returns pcs literals
      | S.member state seen = walk noreturn entry seen pending bottom calls returns pcs literals
      | S.size seen > 100000 || not (inside pc) = Left (InstructionRejected pc)
      | otherwise = do
          encoded <- word pc
          let instruction = A.decode pc encoded
              code = min 14 (encoded `shiftR` 28)
              outcome = case known of
                _ | code == 14 -> Just True
                Just (previous,held) | previous == code -> Just held
                                     | previous `xor` 1 == code -> Just (not held)
                _ -> Nothing
              executedKnown = if writesFlags instruction then Nothing else maybe (Just (code,True)) (const known) outcome
              skippedKnown = maybe (Just (code,False)) (const known) outcome
              seen' = S.insert state seen
              next offset frame = [(pc+4,offset,frame,executedKnown)]
              finish paths low calls' returns' literals' = walk noreturn entry seen' (paths ++ pending) (min bottom low) (calls' ++ calls) (returns' ++ returns) (pc:pcs) (literals' ++ literals)
              conditional paths = paths ++ [(pc+4,sp,fp,skippedKnown) | outcome == Nothing]
              stack offset = unless (offset <= 0 && offset >= -65536 && offset .&. 3 == 0) (Left (StackRejected pc))
              ret offset = do
                unless (offset == 0) (Left (StackRejected pc))
                finish (conditional []) offset [] [pc] []
              call target = do
                unless (pc >= 0xbf1a00 || target < 0xbf1a00) (Left (InstructionRejected pc))
                unless (sp .&. 7 == 0 && ((inside target && target `elem` entries) || target `elem` retainedCallees)) (Left (StackRejected pc))
                finish (conditional (if S.member target noreturn then [] else next sp fp)) sp [CallBoundary pc target sp] [] []
          case instruction of
            A.Rejected _ -> Left (InstructionRejected pc)
            A.ReadStatus _ _ -> Left (InstructionRejected pc)
            A.WriteApplicationFlags _ _ -> Left (InstructionRejected pc)
            _ | outcome == Just False -> finish [(pc+4,sp,fp,known)] sp [] [] []
            A.Branch _ True target -> call target
            A.Branch _ False target
              | target `elem` retainedCallees -> do
                  unless (sp == 0) (Left (StackRejected pc))
                  finish (conditional []) sp [CallBoundary pc target sp] [pc] []
              | pc < 0xbf1a00 && target >= 0xbf1a00 -> Left (InstructionRejected pc)
              | otherwise -> finish (conditional [(target,sp,fp,executedKnown)]) sp [] [] []
            A.BranchExchange _ False register | register == A.lr -> ret sp
            A.BranchExchange {} -> Left (InstructionRejected pc)
            A.StoreMultiple _ register A.DecrementBefore True registers | register == A.sp -> do
              let offset = sp - 4 * length registers
              stack offset
              finish (conditional (next offset fp)) offset [] [] []
            A.LoadMultiple _ register A.IncrementAfter True registers | register == A.sp -> do
              let offset = sp + 4 * length registers
              stack offset
              if A.pc `elem` registers then ret offset else finish (conditional (next offset (if A.Reg 11 `elem` registers then Nothing else fp))) offset [] [] []
            A.DataProcessing _ operation _ destination source (A.Immediate amount) | destination == A.sp && source == A.sp && operation `elem` [A.ADD,A.SUB] -> do
              let offset = sp + (if operation == A.ADD then 1 else -1) * fromIntegral amount
              stack offset
              finish (conditional (next offset fp)) offset [] [] []
            A.DataProcessing _ A.ADD _ (A.Reg 11) source (A.Immediate amount) | source == A.sp -> finish (conditional (next sp (Just (sp + fromIntegral amount)))) sp [] [] []
            A.DataProcessing _ A.SUB _ destination (A.Reg 11) (A.Immediate amount) | destination == A.sp -> case fp of
              Nothing -> Left (StackRejected pc)
              Just saved -> do
                let offset = saved - fromIntegral amount
                stack offset
                finish (conditional (next offset fp)) offset [] [] []
            A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate (A.Reg 11) A.LSL 0) | destination == A.sp -> case fp of
              Nothing -> Left (StackRejected pc)
              Just saved -> do
                stack saved
                finish (conditional (next saved fp)) saved [] [] []
            A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) | destination == A.pc && source == A.lr -> ret sp
            A.DataProcessing _ _ _ destination _ _ | destination `elem` [A.pc,A.sp] -> Left (StackRejected pc)
            A.Store _ A.Word _ register (A.OffsetImmediate amount) indexing | register == A.sp && indexing /= A.OffsetOnly -> do
              let offset = sp + fromIntegral amount
              stack offset
              finish (conditional (next offset fp)) offset [] [] []
            A.Load _ A.Word _ destination register (A.OffsetImmediate amount) indexing | register == A.sp && indexing /= A.OffsetOnly -> do
              let offset = sp + fromIntegral amount
              stack offset
              if destination == A.pc then ret offset else do
                unless (destination /= A.sp) (Left (StackRejected pc))
                finish (conditional (next offset (if destination == A.Reg 11 then Nothing else fp))) offset [] [] []
            A.Load _ width _ _ register (A.OffsetImmediate offset) A.OffsetOnly | register == A.pc -> do
              let destinations = writtenRegisters instruction
              unless (all (`notElem` destinations) [A.pc,A.sp] && width `elem` [A.Word,A.Dual]) (Left (InstructionRejected pc))
              let address = fromInteger (toInteger pc + 8 + toInteger offset)
                  addresses = if width == A.Dual then [address,address+4] else [address]
              unless (all inside addresses && (pc >= 0xbf1a00 || all (< 0xbf1a00) addresses)) (Left (InstructionRejected pc))
              values <- mapM word addresses
              finish (conditional (next sp (if A.Reg 11 `elem` destinations then Nothing else fp))) sp [] [] (zip addresses values)
            A.Load _ _ _ destination _ _ _ | destination `elem` [A.pc,A.sp] -> Left (StackRejected pc)
            A.LoadMultiple _ _ _ _ registers | any (`elem` registers) [A.pc,A.sp] -> Left (StackRejected pc)
            _ -> do
              let destinations = writtenRegisters instruction
              unless (all (`notElem` destinations) [A.pc,A.sp]) (Left (StackRejected pc))
              finish (conditional (next sp (if A.Reg 11 `elem` destinations then Nothing else fp))) sp [] [] []

writesFlags :: A.Instruction -> Bool
writesFlags instruction = case instruction of
  A.DataProcessing _ _ setsFlags _ _ _ -> setsFlags
  A.Branch _ True _ -> True
  A.BranchExchange _ True _ -> True
  A.WriteApplicationFlags _ _ -> True
  _ -> False

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
