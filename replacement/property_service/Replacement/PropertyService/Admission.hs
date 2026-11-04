{-# LANGUAGE GHC2021 #-}

-- | Code policy over the linked owned text: every word is a walked A32 instruction or a literal it
-- loads, control stays in owned functions apart from the stub tails, the only non-stack stores are
-- the store accessors', and each entry's stack is bounded and composes over an acyclic call graph.
module Replacement.PropertyService.Admission
  ( FunctionName (..)
  , FunctionExtent (..)
  , CodeLayout (..)
  , StackOffset (..)
  , CallKind (..)
  , CallEdge (..)
  , EntryFrame (..)
  , CodeProof (..)
  , Violation (..)
  , AdmissionError (..)
  , admitCode
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless, when)
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Placement (GuestAddress (..))

newtype FunctionName = FunctionName String deriving (Eq, Ord, Show)

data FunctionExtent = FunctionExtent
  { functionName :: FunctionName
  , functionStart :: GuestAddress
  , functionEnd :: GuestAddress
  }
  deriving (Eq, Show)

-- | Bytes relative to the stack pointer at a function's entry.
newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)

data CodeLayout = CodeLayout
  { layoutBase :: GuestAddress
  , layoutFunctions :: [FunctionExtent]
  , -- | Extents holding no code.
    layoutData :: [(GuestAddress, GuestAddress)]
  , -- | Entries of the functions whose stores are not confined to the stack.
    layoutStoreAccessors :: [GuestAddress]
  , -- | Entries no owned code calls.
    layoutRoots :: [GuestAddress]
  , -- | B sites and their permitted targets outside owned text.
    layoutTails :: [(GuestAddress, GuestAddress)]
  }

data CallKind = Call | TailCall deriving (Eq, Ord, Show)

data CallEdge = CallEdge
  { edgeSite :: GuestAddress
  , edgeTarget :: GuestAddress
  , edgeStack :: StackOffset
  , edgeKind :: CallKind
  }
  deriving (Eq, Ord, Show)

data EntryFrame = EntryFrame
  { frameEntry :: GuestAddress
  , -- | The lowest stack pointer the function itself reaches.
    frameLow :: StackOffset
  , frameCalls :: [CallEdge]
  }
  deriving (Eq, Show)

data CodeProof = CodeProof
  { proofFrames :: [EntryFrame]
  , -- | The lowest stack pointer each entry reaches including its callees.
    proofComposed :: M.Map GuestAddress StackOffset
  , proofInstructions :: S.Set GuestAddress
  , proofLiterals :: S.Set GuestAddress
  }
  deriving (Eq, Show)

data Violation
  = ForbiddenWord A.Forbidden
  | StatusAccess
  | CallOutsideOwned GuestAddress
  | BranchOutsideOwned GuestAddress
  | PcWrite
  | NonStackStore
  | StackUnbounded
  | StackMisaligned
  | StackUnbalanced
  | FlowLeavesFunction
  | LiteralOutsideFunction
  | Recursion
  | UnwalkedWord
  | ReturnAddressLost
  | FramePointerLost
  deriving (Eq, Show)

data AdmissionError = AdmissionError GuestAddress Violation deriving (Eq, Show)

stackLimit :: Int
stackLimit = 4096

-- | Where the return address is: in lr, in the stack slot the function saved it to, or nowhere.
data LinkState = LinkLive | LinkSaved Int | LinkLost deriving (Eq, Ord, Show)

-- | Whether r11 holds the caller's value, holds another value with the caller's in a stack slot,
-- or holds another value with no copy of the caller's.
data FrameRegister = R11Entry | R11Saved Int | R11Changed deriving (Eq, Ord, Show)

-- | A path state: pc, stack offset, the stack offset r11 holds when known, lr and r11 integrity.
data PathState = PathState
  { statePc :: Word32
  , stateSp :: Int
  , stateFp :: Maybe Int
  , stateLink :: LinkState
  , stateR11 :: FrameRegister
  }

data Outcome = Outcome [PathState] Int [CallEdge] [Word32]

admitCode :: CodeLayout -> B.ByteString -> Either AdmissionError CodeProof
admitCode layout bytes = do
  walked <- mapM (walkFunction layout bytes) (layoutFunctions layout)
  let instructions = S.unions [pcs | (_, pcs, _) <- walked]
      literals = S.unions [lits | (_, _, lits) <- walked]
      GuestAddress base = layoutBase layout
      inData address = any (\(GuestAddress low, GuestAddress high) -> low <= address && address < high) (layoutData layout)
      required = [address | address <- [base, base + 4 .. base + fromIntegral (B.length bytes) - 4], not (inData address)]
  case [address | address <- required, S.member address instructions == S.member address literals] of
    address : _ -> Left (AdmissionError (GuestAddress address) UnwalkedWord)
    [] -> pure ()
  let frames = [frame | (frame, _, _) <- walked]
  composed <- foldM (compose (M.fromList [(frameEntry frame, frame) | frame <- frames]) S.empty) M.empty (map frameEntry frames)
  pure (CodeProof frames composed (S.map GuestAddress instructions) (S.map GuestAddress literals))

compose :: M.Map GuestAddress EntryFrame -> S.Set GuestAddress -> M.Map GuestAddress StackOffset -> GuestAddress -> Either AdmissionError (M.Map GuestAddress StackOffset)
compose frames visiting done entry
  | M.member entry done = Right done
  | S.member entry visiting = Left (AdmissionError entry Recursion)
  | otherwise = case M.lookup entry frames of
      Nothing -> Left (AdmissionError entry FlowLeavesFunction)
      Just (EntryFrame _ (StackOffset own) calls) -> do
        done' <- foldM (compose frames (S.insert entry visiting)) done (map edgeTarget calls)
        let through (CallEdge _ target (StackOffset at) _) = let StackOffset calleeLow = done' M.! target in at + calleeLow
            low = minimum (own : map through calls)
        when (low < negate stackLimit) (Left (AdmissionError entry StackUnbounded))
        pure (M.insert entry (StackOffset low) done')

-- | Every function returns with sp, r11 and the return address it entered with; stubs (roots) enter
-- with no return address in lr and leave only through their tails.
walkFunction :: CodeLayout -> B.ByteString -> FunctionExtent -> Either AdmissionError (EntryFrame, S.Set Word32, S.Set Word32)
walkFunction layout bytes (FunctionExtent _ (GuestAddress start) (GuestAddress end)) =
  go M.empty [PathState start 0 Nothing entryLink R11Entry] 0 [] S.empty
  where
    GuestAddress base = layoutBase layout
    entryLink = if GuestAddress start `elem` layoutRoots layout then LinkLost else LinkLive
    callable = S.fromList [entry | FunctionExtent _ (GuestAddress entry) _ <- layoutFunctions layout, GuestAddress entry `notElem` layoutRoots layout]
    accessor = GuestAddress start `elem` layoutStoreAccessors layout
    tails = S.fromList [(site, target) | (GuestAddress site, GuestAddress target) <- layoutTails layout]
    go seen [] low calls literals =
      Right (EntryFrame (GuestAddress start) (StackOffset low) (S.toList (S.fromList calls)), M.keysSet seen, literals)
    go seen (state : pending) low calls literals =
      let pc = statePc state
          key = (stateSp state, stateFp state, stateLink state, stateR11 state)
      in case M.lookup pc seen of
        Just keys | S.member key keys -> go seen pending low calls literals
        Just keys | any (\(sp, _, _, _) -> sp /= stateSp state) (S.toList keys) -> Left (AdmissionError (GuestAddress pc) StackUnbalanced)
        _
          | pc < start || pc >= end || pc .&. 3 /= 0 -> Left (AdmissionError (GuestAddress pc) FlowLeavesFunction)
          | otherwise -> do
              let word = U.word32At bytes (fromIntegral (pc - base))
              Outcome next touched edges literalAddresses <- either (Left . AdmissionError (GuestAddress pc)) Right (step state word)
              go (M.insertWith S.union pc (S.singleton key) seen) (next ++ pending) (min low touched) (edges ++ calls) (S.union literals (S.fromList literalAddresses))
    step state@(PathState pc sp fp link frame) word = do
      let instruction = A.decode pc word
          conditional = word `shiftR` 28 < 14
          skip paths = paths ++ [state {statePc = pc + 4} | conditional]
          advance next = Right (Outcome (skip [next {statePc = pc + 4}]) (min sp (stateSp next)) [] [])
          terminal = Right (Outcome (skip []) sp [] [])
          slots = [slot | LinkSaved slot <- [link]] ++ [slot | R11Saved slot <- [frame]]
          clear low high = unless (all (\slot -> slot + 4 <= low || slot >= high) slots) (Left NonStackStore)
          bound offset = do
            unless (offset .&. 3 == 0) (Left StackMisaligned)
            unless (offset <= 0) (Left StackUnbalanced)
            unless (offset >= negate stackLimit) (Left StackUnbounded)
          moveSp offset current = current
            { stateSp = offset
            , stateLink = case stateLink current of
                LinkSaved slot | slot < offset -> LinkLost
                other -> other
            , stateR11 = case stateR11 current of
                R11Saved slot | slot < offset -> R11Changed
                other -> other
            }
          adjust offset = bound offset >> advance (moveSp offset state)
          written registers current = current
            { stateFp = if r11 `elem` registers then Nothing else stateFp current
            , stateLink = if A.lr `elem` registers then LinkLost else stateLink current
            , stateR11 = if r11 `elem` registers && stateR11 current == R11Entry then R11Changed else stateR11 current
            }
          restored (register, address) current
            | register == A.lr = current {stateLink = if stateLink current == LinkSaved address then LinkLive else LinkLost}
            | register == r11 && stateR11 current == R11Saved address = current {stateFp = Nothing, stateR11 = R11Entry}
            | otherwise = written [register] current
          leaving current = do
            unless (stateSp current == 0) (Left StackUnbalanced)
            unless (stateR11 current == R11Entry) (Left FramePointerLost)
          returning current = do
            leaving current
            unless (stateLink current == LinkLive) (Left ReturnAddressLost)
      case instruction of
        A.Rejected reason -> Left (ForbiddenWord reason)
        A.ReadStatus {} -> Left StatusAccess
        A.WriteApplicationFlags {} -> Left StatusAccess
        A.Branch _ True target
          | S.member target callable -> do
              unless (sp `mod` 8 == 0) (Left StackMisaligned)
              let next = state {statePc = pc + 4, stateLink = if link == LinkLive then LinkLost else link}
              Right (Outcome (skip [next]) sp [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) Call] [])
          | otherwise -> Left (CallOutsideOwned (GuestAddress target))
        A.Branch _ False target
          | S.member (pc, target) tails -> leaving state >> terminal
          | target >= start && target < end -> Right (Outcome (skip [state {statePc = target}]) sp [] [])
          | S.member target callable -> do
              returning state
              Right (Outcome (skip []) sp [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) TailCall] [])
          | otherwise -> Left (BranchOutsideOwned (GuestAddress target))
        A.BranchExchange _ False register | register == A.lr -> returning state >> terminal
        A.BranchExchange {} -> Left PcWrite
        A.StoreMultiple _ register mode writeback registers
          | register == A.sp && mode == A.DecrementBefore && writeback -> do
              let after = sp - 4 * length registers
                  addressed = zip registers [after, after + 4 ..]
                  savedAt wanted = lookup wanted addressed
              bound after
              clear after sp
              advance (moveSp after state)
                { stateLink = case (link, savedAt A.lr) of
                    (LinkLive, Just slot) -> LinkSaved slot
                    _ -> link
                , stateR11 = case (frame, savedAt r11) of
                    (R11Entry, Just slot) -> R11Saved slot
                    _ -> frame
                }
          | register == A.sp && not writeback -> do
              let count = 4 * length registers
                  lowest = case mode of
                    A.IncrementAfter -> sp
                    A.IncrementBefore -> sp + 4
                    A.DecrementAfter -> sp - count + 4
                    A.DecrementBefore -> sp - count
              unless (lowest >= sp && lowest + count <= 0) (Left NonStackStore)
              clear lowest (lowest + count)
              advance state
          | accessor && not writeback -> advance state
          | otherwise -> Left NonStackStore
        A.LoadMultiple _ register mode writeback registers
          | A.sp `elem` registers -> Left StackUnbounded
          | register == A.sp && mode == A.IncrementAfter && writeback -> do
              let after = sp + 4 * length registers
                  addressed = zip registers [sp, sp + 4 ..]
                  afterLoads = foldl (flip restored) state [entry | entry@(loadedRegister, _) <- addressed, loadedRegister /= A.pc]
              bound after
              case lookup A.pc addressed of
                Just address -> do
                  unless (link == LinkSaved address) (Left PcWrite)
                  leaving afterLoads {stateSp = after}
                  terminal
                Nothing -> advance (moveSp after afterLoads)
          | A.pc `elem` registers -> Left PcWrite
          | register == A.sp && writeback -> Left StackUnbounded
          | otherwise -> advance (written (registers ++ [register | writeback]) state)
        A.DataProcessing _ operation _ destination source operand
          | destination == A.pc -> Left PcWrite
          | destination == A.sp -> case (operation, operand) of
              (A.ADD, A.Immediate amount) | source == A.sp -> adjust (sp + fromIntegral amount)
              (A.SUB, A.Immediate amount) | source == A.sp -> adjust (sp - fromIntegral amount)
              (A.SUB, A.Immediate amount) | source == r11, Just saved <- fp -> adjust (saved - fromIntegral amount)
              (A.MOV, A.ShiftedImmediate moved A.LSL 0) | moved == r11, Just saved <- fp -> adjust saved
              _ -> Left StackUnbounded
          | destination == r11, operation == A.ADD, source == A.sp, A.Immediate amount <- operand ->
              advance (written [r11] state) {stateFp = Just (sp + fromIntegral amount)}
          | destination == r11, operation == A.MOV, A.ShiftedImmediate moved A.LSL 0 <- operand, moved == A.sp ->
              advance (written [r11] state) {stateFp = Just sp}
          | otherwise -> advance (written [destination | operation `notElem` [A.CMP, A.CMN, A.TST, A.TEQ]] state)
        A.Store _ width _ register offset indexing
          | register == A.sp -> case offset of
              A.OffsetImmediate amount -> do
                let moved = sp + fromIntegral amount
                    address = if indexing == A.PostIndexed then sp else moved
                    after = if indexing == A.OffsetOnly then sp else moved
                unless (address >= after && address + widthBytes width <= 0) (Left NonStackStore)
                clear address (address + widthBytes width)
                bound after
                advance (moveSp after state)
              A.OffsetRegister {} -> Left NonStackStore
          | accessor && indexing == A.OffsetOnly -> advance state
          | otherwise -> Left NonStackStore
        A.Load _ width _ destination@(A.Reg number) register offset indexing
          | destination == A.pc || (width == A.Dual && number == 14) -> Left PcWrite
          | destination == A.sp || (width == A.Dual && number == 12) -> Left StackUnbounded
          | register == A.pc && indexing /= A.OffsetOnly -> Left PcWrite
          | register == A.pc -> case offset of
              A.OffsetImmediate amount -> do
                let address = fromIntegral (toInteger pc + 8 + toInteger amount)
                    addresses = if width == A.Dual then [address, address + 4] else [address]
                unless (width `elem` [A.Word, A.Dual] && all (\a -> a .&. 3 == 0 && a >= start && a < end) addresses) (Left LiteralOutsideFunction)
                Right (Outcome (skip [(written (loaded destination width) state) {statePc = pc + 4}]) sp [] addresses)
              A.OffsetRegister {} -> advance (written (loaded destination width) state)
          | register == A.sp && indexing /= A.OffsetOnly -> case offset of
              A.OffsetImmediate amount -> do
                let moved = sp + fromIntegral amount
                    address = if indexing == A.PostIndexed then sp else moved
                    after = moved
                bound after
                let current = if width == A.Word then restored (destination, address) state else written (loaded destination width) state
                advance (moveSp after current)
              A.OffsetRegister {} -> Left StackUnbounded
          | otherwise -> advance (written (loaded destination width ++ [register | indexing /= A.OffsetOnly]) state)
        _ -> do
          let registers = writtenRegisters instruction
          when (A.pc `elem` registers) (Left PcWrite)
          when (A.sp `elem` registers) (Left StackUnbounded)
          advance (written registers state)
    loaded destination@(A.Reg number) width = destination : [A.Reg (number + 1) | width == A.Dual]

r11 :: A.Reg
r11 = A.Reg 11

widthBytes :: A.Width -> Int
widthBytes width = case width of
  A.Byte -> 1
  A.Half -> 2
  A.Word -> 4
  A.Dual -> 8

writtenRegisters :: A.Instruction -> [A.Reg]
writtenRegisters instruction = case instruction of
  A.MoveWide _ destination _ -> [destination]
  A.MoveTop _ destination _ -> [destination]
  A.ZeroExtend _ destination _ _ _ -> [destination]
  A.Multiply _ destination _ _ _ -> [destination]
  A.Compute _ destinations -> destinations
  _ -> []
