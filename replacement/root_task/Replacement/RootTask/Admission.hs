module Replacement.RootTask.Admission
  ( AdmissionError (..), RootProof, CallKind (..), CallSite (..), Scenario (..), EffectPosition (..), deriveRootProof
  , scratchLow, executionPcs, callSites, haltPcs, returnPcs, scenarioCount, callPositions
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError = InvalidCarrier | UnsupportedInstruction | UnprovedCondition
  | PcOperandRejected | PcWriteRejected | NumericProvenanceRejected | StackProvenanceRejected
  | StackEscape | UnallocatedStackAccess | UninitializedStackRead | ExternalMemoryAccess
  | CallStackUnaligned | OrderedEffectsDiffer | CallArgumentsDiffer | ReturnStatusDiffer
  | ReturnStackDiffers | PreservedRegisterDiffers | ReturnProvenanceRejected
  | ExecutionOutOfRange | ExecutionBoundExceeded | UnexpectedCycle | LoopStateDrift
  | UnreachableCode | LiteralCodeOverlap
  deriving (Eq, Show)
data CallKind = Call | TailCall deriving (Eq, Ord, Show)
data CallSite = CallSite {callPc :: Word32, callTarget :: Word32, callKind :: CallKind}
  deriving (Eq, Ord, Show)
-- | A corpus scenario and the index of one effect in its ordered list.
data EffectPosition = EffectPosition Scenario Int deriving (Eq, Ord, Show)
data RootProof = RootProof Int [Word32] [CallSite] [Word32] [Word32] Int [(CallSite,[EffectPosition])] deriving (Eq, Show)
scratchLow :: RootProof -> Int
scratchLow (RootProof n _ _ _ _ _ _) = n
executionPcs :: RootProof -> [Word32]
executionPcs (RootProof _ n _ _ _ _ _) = n
callSites :: RootProof -> [CallSite]
callSites (RootProof _ _ n _ _ _ _) = n
haltPcs :: RootProof -> [Word32]
haltPcs (RootProof _ _ _ n _ _ _) = n
returnPcs :: RootProof -> [Word32]
returnPcs (RootProof _ _ _ _ n _ _) = n
scenarioCount :: RootProof -> Int
scenarioCount (RootProof _ _ _ _ _ n _) = n
-- | Each call site with the effect positions it executes across the corpus, positions ascending.
callPositions :: RootProof -> [(CallSite,[EffectPosition])]
callPositions (RootProof _ _ _ _ _ _ n) = n

data ResultSource = CheckedA | CheckedB | Mode | Query deriving (Eq, Show)
data Value = Number Word32 | Result ResultSource Word32 | CleanupResult | Incoming A.Reg
  | Stack Int | IncomingLr | Unknown deriving (Eq, Show)
data Effect = Effect Word32 [Word32] deriving (Eq, Show)
data Outcome = Halt | Return deriving (Eq, Show)
-- | Checked results of 0x6a9354, 0x6ac2b4, the mode 0x69cfa4 and the query 0x6a5920.
data Scenario = Scenario Word32 Word32 Word32 Word32 deriving (Eq, Ord, Show)
data Machine = Machine
  { registers :: M.Map A.Reg Value, stack :: M.Map Int Value, zero :: Maybe Bool
  , remaining :: [Effect], low :: Int, steps :: Int, fetched :: S.Set Word32
  , literals :: S.Set Word32, sites :: S.Set CallSite, served :: S.Set (CallSite,EffectPosition)
  } deriving (Eq, Show)
data Path = Path Machine Word32 Outcome

origin :: Word32
origin = 0x69d160
require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise refusal = unless premise (Left refusal)
get :: Machine -> A.Reg -> Value
get machine register = M.findWithDefault Unknown register (registers machine)
put :: A.Reg -> Value -> Machine -> Either AdmissionError Machine
put register value machine
  | register == A.pc = Left PcWriteRejected
  | register == A.sp = case value of
      Stack offset -> do
        require (offset >= -992 && offset <= 0 && offset `mod` 4 == 0) StackEscape
        pure machine {registers = M.insert register value (registers machine), low = min offset (low machine)
          , stack = M.filterWithKey (\address _ -> address >= offset) (stack machine)}
      _ -> Left StackProvenanceRejected
  | otherwise = pure machine {registers = M.insert register value (registers machine)}
spOffset :: Machine -> Either AdmissionError Int
spOffset machine = case get machine A.sp of Stack offset -> Right offset; _ -> Left StackProvenanceRejected
numeric :: Value -> Either AdmissionError Word32
numeric (Number value) = Right value
numeric (Result _ value) = Right value
numeric _ = Left NumericProvenanceRejected
operand :: Machine -> A.Operand -> Either AdmissionError Value
operand _ (A.Immediate value) = Right (Number value)
operand machine (A.ShiftedImmediate register A.LSL 0) = do
  require (register /= A.pc) PcOperandRejected
  pure (get machine register)
operand _ _ = Left UnsupportedInstruction
condition :: Machine -> A.Cond -> Either AdmissionError Bool
condition _ (A.Cond 14) = Right True
condition machine (A.Cond 0) = maybe (Left UnprovedCondition) Right (zero machine)
condition machine (A.Cond 1) = maybe (Left UnprovedCondition) (Right . not) (zero machine)
condition _ _ = Left UnsupportedInstruction
allocated :: Machine -> Int -> Either AdmissionError ()
allocated machine address = do
  bottom <- spOffset machine
  require (address >= bottom && address < 0 && address `mod` 4 == 0) UnallocatedStackAccess
readStack :: Machine -> Int -> Either AdmissionError Value
readStack machine address = do
  allocated machine address
  maybe (Left UninitializedStackRead) Right (M.lookup address (stack machine))
writeStack :: Machine -> Int -> Value -> Either AdmissionError Machine
writeStack machine address value = do
  allocated machine address
  pure machine {stack = M.insert address value (stack machine)}
arithmetic :: A.DataOp -> Value -> Value -> Either AdmissionError Value
arithmetic A.MOV _ right = Right right
arithmetic A.ADD (Stack offset) (Number amount) = Right (Stack (offset+fromIntegral amount))
arithmetic A.SUB (Stack offset) (Number amount) = Right (Stack (offset-fromIntegral amount))
arithmetic A.ADD (Number a) (Number b) = Right (Number (a+b))
arithmetic A.SUB (Number a) (Number b) = Right (Number (a-b))
arithmetic A.ORR (Number a) (Number b) = Right (Number (a .|. b))
arithmetic _ _ _ = Left NumericProvenanceRejected

validate :: A.Instruction -> Either AdmissionError ()
validate instruction = case instruction of
  A.MoveWide _ destination _ -> noPc destination
  A.MoveTop _ destination _ -> noPc destination
  A.DataProcessing _ operation updates destination source value -> do
    require (operation `elem` [A.MOV,A.ADD,A.SUB,A.CMP,A.ORR] && updates == (operation == A.CMP)) UnsupportedInstruction
    case value of
      A.Immediate _ -> pure ()
      A.ShiftedImmediate register A.LSL 0 -> require (register /= A.pc) PcOperandRejected
      _ -> Left UnsupportedInstruction
    require (operation /= A.ORR || case value of A.Immediate _ -> True; _ -> False) UnsupportedInstruction
    let relative = source == A.pc && operation `elem` [A.ADD,A.SUB]
          && case value of A.Immediate _ -> True; _ -> False
        lrReturn = operation == A.MOV && value == A.ShiftedImmediate A.lr A.LSL 0
    require (source /= A.pc || operation == A.MOV || relative) PcOperandRejected
    require (destination /= A.pc || lrReturn) PcWriteRejected
  A.Store _ A.Word source base (A.OffsetImmediate _) _ -> noPc source >> noPc base
  A.Load _ A.Word False destination base (A.OffsetImmediate _) indexing ->
    require (if base == A.pc then destination /= A.pc && indexing == A.OffsetOnly
      else destination /= A.pc || base == A.sp) PcWriteRejected
  A.StoreMultiple _ base A.DecrementBefore True saved ->
    require (base == A.sp && A.pc `notElem` saved) PcOperandRejected
  A.LoadMultiple _ base A.IncrementAfter True _ -> require (base == A.sp) PcOperandRejected
  A.Branch {} -> pure ()
  A.BranchExchange _ False register -> require (register == A.lr) PcOperandRejected
  A.Hint {} -> pure ()
  _ -> Left UnsupportedInstruction
  where noPc register = require (register /= A.pc) PcOperandRejected

-- Checked results are finite representatives. Ignored results, volatile registers and NZCV are unknown.
-- CleanupResult is opaque and must reach the caller without numeric reconstruction.
corpus :: [Scenario]
corpus = [Scenario a b mode query | a <- values, b <- values, mode <- values, query <- values]
  where values = [0,1,0xffffffff,0x80000000]
sourceEffects :: Scenario -> ([Effect],Outcome)
sourceEffects (Scenario a b mode query) =
  (prefix ++ if a /= 0 then [] else [effect 0x6ac2b4 []] ++ if b /= 0 then [] else suffix, outcome)
  where
    effect = Effect
    prefix = [effect 0x649624 [],effect 0x7284ec [],effect 0x72872c [0x01010000]
      ,effect 0x725b54 [0x72835c]]
      ++ [effect 0x7247ac [identifier,1] | identifier <- [1..97]]
      ++ map (`effect` []) [0x69ca24,0x676054,0x662e48,0x665224,0x649070]
      ++ [effect 0x6490dc [1],effect 0x6a9354 []]
    suffix = map (`effect` []) [0x69f204,0x656788,0x7262cc]
      ++ [effect 0x724cbc [0x726478],effect 0x6a5634 [],effect 0x6a59b8 [34,0,0],effect 0x69cfa4 []]
      ++ if mode == 0 then [effect 0x6a5920 [34]]
           ++ (if query == 0 then [] else map (effect 0x6a59b8) [[34,0,0],[31,60,0],[32,0,0],[34,1,0]])
           ++ map (`effect` []) [0x6a564c,0x6a5c08,0x6568ec]
         else [effect 0x69ca44 [],effect 0x725424 [30,0,0x69cc18,0x96ca4c]
              ,effect 0x72872c [0x11010000],effect 0x72612c [0]]
    outcome = if a == 0 && b == 0 && mode /= 0 then Return else Halt

handback :: Machine -> Either AdmissionError ()
handback machine = do
  require (get machine A.sp == Stack 0) ReturnStackDiffers
  require (all (\n -> get machine (A.Reg n) == Incoming (A.Reg n)) [4..11]) PreservedRegisterDiffers

-- Only documented immediate PC-relative address/literal forms and provenance-proven LR returns are admitted.
deriveRootProof :: B.ByteString -> Either AdmissionError RootProof
deriveRootProof bytes = do
  require (not (B.null bytes) && B.length bytes <= 340 && B.length bytes `mod` 4 == 0) InvalidCarrier
  paths <- mapM run corpus
  let states = [machine | Path machine _ _ <- paths]
      pcs = S.unions (map fetched states)
      pools = S.unions (map literals states)
      allWords = S.fromList [origin,origin+4..origin+fromIntegral (B.length bytes)-4]
  require (S.null (S.intersection pcs pools)) LiteralCodeOverlap
  require (S.union pcs pools == allWords) UnreachableCode
  pure (RootProof (minimum (map low states)) (S.toAscList pcs)
    (S.toAscList (S.unions (map sites states)))
    (S.toAscList (S.fromList [pc | Path _ pc Halt <- paths]))
    (S.toAscList (S.fromList [pc | Path _ pc Return <- paths])) (length corpus)
    (M.toAscList (M.map S.toAscList (M.fromListWith S.union
      [(site,S.singleton position) | machine <- states, (site,position) <- S.toList (served machine)]))))
  where
    inside pc = pc >= origin && toInteger pc+4 <= toInteger origin+toInteger (B.length bytes) && pc .&. 3 == 0
    run scenario = let (effects,_) = sourceEffects scenario in walk scenario origin
      (Machine (M.fromList ([(A.Reg n,Incoming (A.Reg n)) | n <- [0..12]] ++ [(A.sp,Stack 0),(A.lr,IncomingLr)]))
        M.empty Nothing effects 0 0 S.empty S.empty S.empty S.empty) M.empty
    finish scenario outcome pc machine = do
      require (null (remaining machine) && snd (sourceEffects scenario) == outcome) OrderedEffectsDiffer
      if outcome == Return then do
        handback machine
        require (get machine (A.Reg 0) == CleanupResult) ReturnStatusDiffer
      else pure ()
      pure (Path machine pc outcome)
    retained scenario pc target kind machine = do
      offset <- spOffset machine
      require (offset `mod` 8 == 0) CallStackUnaligned
      rest <- case remaining machine of
        Effect wanted arguments:rest -> do
          require (target == wanted) OrderedEffectsDiffer
          require (map (get machine . A.Reg) [0..length arguments-1] == map Number arguments) CallArgumentsDiffer
          pure rest
        [] -> Left OrderedEffectsDiffer
      clobbered <- foldM (\state register -> put (A.Reg register) Unknown state) machine [0,1,2,3,12,14]
      let Scenario a b mode query = scenario
          result = case target of
            0x6a9354 -> Result CheckedA a
            0x6ac2b4 -> Result CheckedB b
            0x69cfa4 -> Result Mode mode
            0x6a5920 -> Result Query query
            0x72612c -> CleanupResult
            _ -> Unknown
          position = EffectPosition scenario (length (fst (sourceEffects scenario)) - length (remaining machine))
          site = CallSite pc target kind
      changed <- put (A.Reg 0) result clobbered
      pure changed {remaining = rest, zero = Nothing, sites = S.insert site (sites machine)
        , served = S.insert (site,position) (served machine)}
    walk scenario pc machine seen = do
      require (inside pc) ExecutionOutOfRange
      require (steps machine < 8192) ExecutionBoundExceeded
      let signature = (registers machine,stack machine,zero machine,remaining machine)
      case M.lookup pc seen of
        Just previous | null (remaining machine) -> do
          require (signature == previous) LoopStateDrift
          finish scenario Halt pc machine
        _ -> step scenario pc machine (M.insert pc signature seen)
    step scenario pc machine seen = do
      let word = U.word32At bytes (fromIntegral (pc-origin))
          advanced = machine {steps = steps machine+1,fetched = S.insert pc (fetched machine)}
          next changed = walk scenario (pc+4) changed seen
          execute cond action = condition machine cond >>= \taken -> if taken then action else next advanced
          assign register value = put register value advanced >>= next
          returned value changed = do
            require (value == IncomingLr) ReturnProvenanceRejected
            finish scenario Return pc changed
          address :: A.Reg -> Int32 -> Either AdmissionError Int
          address base delta = case get machine base of
            Stack offset -> Right (offset+fromIntegral delta)
            _ -> Left ExternalMemoryAccess
      let instruction = A.decode pc word
      validate instruction
      case instruction of
        A.MoveWide cond destination value -> execute cond (assign destination (Number value))
        A.MoveTop cond destination value -> execute cond $ do
          require (destination /= A.pc) PcWriteRejected
          previous <- numeric (get machine destination)
          assign destination (Number ((previous .&. 0xffff) .|. (value `shiftL` 16)))
        A.DataProcessing cond operation updates destination source value -> execute cond $ do
          right <- operand machine value
          if destination == A.pc && operation == A.MOV && not updates then
            case value of
              A.ShiftedImmediate register A.LSL 0 | register == A.lr -> returned right advanced
              _ -> Left PcWriteRejected
          else if operation == A.CMP && updates then do
            require (source /= A.pc) PcOperandRejected
            left <- numeric (get machine source)
            other <- numeric right
            next advanced {zero = Just (left == other)}
          else do
            require (destination /= A.pc && not updates) PcWriteRejected
            let relative = source == A.pc && operation `elem` [A.ADD,A.SUB]
                  && case value of A.Immediate _ -> True; _ -> False
            require (source /= A.pc || operation == A.MOV || relative) PcOperandRejected
            result <- arithmetic operation (if relative then Number (pc+8) else get machine source) right
            assign destination result
        A.StoreMultiple cond base A.DecrementBefore True saved -> execute cond $ do
          require (base == A.sp && not (null saved) && base `notElem` saved && A.pc `notElem` saved) PcOperandRejected
          root <- spOffset machine
          let bottom = root-4*length saved
          allocatedMachine <- put A.sp (Stack bottom) advanced
          changed <- foldM (\state (slot,register) -> writeStack state slot (get machine register)) allocatedMachine (zip [bottom,bottom+4..] saved)
          next changed
        A.LoadMultiple cond base A.IncrementAfter True restored -> execute cond $ do
          require (base == A.sp && not (null restored) && base `notElem` restored) StackProvenanceRejected
          root <- spOffset machine
          values <- mapM (readStack machine) (take (length restored) [root,root+4..])
          changed <- foldM (\state (register,value) -> if register == A.pc then pure state else put register value state) advanced (zip restored values)
          balanced <- put A.sp (Stack (root+4*length restored)) changed
          case lookup A.pc (zip restored values) of
            Just value -> returned value balanced
            Nothing -> next balanced
        A.Store cond A.Word source base (A.OffsetImmediate delta) indexing -> execute cond $ do
          require (source /= A.pc && base /= A.pc && (indexing == A.OffsetOnly || source /= base)) PcOperandRejected
          slot <- address base delta
          actual <- if indexing == A.PostIndexed then address base 0 else pure slot
          allocatedMachine <- if indexing == A.PreIndexed then put base (Stack slot) advanced else pure advanced
          changed <- writeStack allocatedMachine actual (get machine source)
          updated <- if indexing == A.PostIndexed then put base (Stack slot) changed else pure changed
          next updated
        A.Load cond A.Word False destination base (A.OffsetImmediate delta) indexing -> execute cond $ do
          require (indexing == A.OffsetOnly || destination /= base) PcWriteRejected
          if base == A.pc then do
            require (indexing == A.OffsetOnly) PcOperandRejected
            let literal = fromIntegral (toInteger pc+8+toInteger delta)
            require (inside literal) ExternalMemoryAccess
            changed <- put destination (Number (U.word32At bytes (fromIntegral (literal-origin)))) advanced
            next changed {literals = S.insert literal (literals changed)}
          else do
            slot <- address base delta
            actual <- if indexing == A.PostIndexed then address base 0 else pure slot
            value <- readStack machine actual
            changed <- if destination == A.pc then pure advanced else put destination value advanced
            updated <- if indexing == A.OffsetOnly then pure changed else put base (Stack slot) changed
            if destination == A.pc then returned value updated else next updated
        A.Branch cond link target -> execute cond $ do
          if inside target then do
            require (not link) UnexpectedCycle
            walk scenario target advanced seen
          else if link then retained scenario pc target Call advanced >>= next
          else do
            handback advanced
            require (get advanced A.lr == IncomingLr) ReturnProvenanceRejected
            changed <- retained scenario pc target TailCall advanced
            finish scenario Return pc changed
        A.BranchExchange cond False register -> execute cond $ do
          require (register == A.lr) PcOperandRejected
          returned (get machine register) advanced
        A.Hint cond | word == 0xe320f000 -> execute cond (next advanced)
        _ -> Left UnsupportedInstruction
