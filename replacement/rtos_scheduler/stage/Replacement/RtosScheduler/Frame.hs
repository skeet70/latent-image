-- | Stack demand of owned code, walked over A32 words. Flow stays inside the extent holding the
-- function's entry; a call or a branch to another owned extent composes with the callee's frame.
module Replacement.RtosScheduler.Frame
  ( OwnedCode (..)
  , CallKind (..)
  , CallEdge (..)
  , Frame (..)
  , Violation (..)
  , FrameError (..)
  , frames
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless, when)
import Data.Bits (shiftR, testBit, (.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions

-- | A loader image at its guest base and the extents of it that are owned code.
data OwnedCode = OwnedCode
  { codeBase :: GuestAddress
  , codeBytes :: B.ByteString
  , codeExtents :: [Extent]
  }

data CallKind = Call | TailCall deriving (Eq, Ord, Show)

data CallEdge = CallEdge
  { edgeSite :: GuestAddress
  , edgeTarget :: GuestAddress
  , -- | Bytes below entry sp at the call.
    edgeDepth :: Int
  , edgeKind :: CallKind
  }
  deriving (Eq, Ord, Show)

data Frame = Frame
  { frameEntry :: GuestAddress
  , frameOwn :: FrameBytes
  , frameComposed :: FrameBytes
  , frameCalls :: [CallEdge]
  }
  deriving (Eq, Show)

data Violation
  = UnwalkedWord Word32
  | FlowLeavesExtent
  | CallOutsideOwned GuestAddress
  | BranchOutsideOwned GuestAddress
  | PcWrite
  | StackUnbalanced
  | StackUnbounded
  | Recursion
  deriving (Eq, Show)

data FrameError = FrameError GuestAddress Violation deriving (Eq, Show)

stackLimit :: Int
stackLimit = 4096

r11 :: A.Reg
r11 = A.Reg 11

-- | sp and, when known, r11 as offsets from entry sp.
data State = State {statePc :: Word32, stateSp :: Int, stateFp :: Maybe Int}

data Walked = Walked Int [CallEdge]

-- | The frames of the roots and of all owned code they reach.
frames :: OwnedCode -> [GuestAddress] -> Either FrameError (M.Map GuestAddress Frame)
frames code roots = do
  walked <- reach M.empty roots
  composed <- foldM (compose walked S.empty) M.empty (M.keys walked)
  pure (M.mapWithKey (\entry (Walked low calls) -> Frame entry (FrameBytes (negate low)) (FrameBytes (negate (composed M.! entry))) calls) walked)
  where
    reach done [] = Right done
    reach done (entry : pending)
      | M.member entry done = reach done pending
      | otherwise = do
          result@(Walked _ calls) <- walk code entry
          reach (M.insert entry result done) (map edgeTarget calls ++ pending)

compose :: M.Map GuestAddress Walked -> S.Set GuestAddress -> M.Map GuestAddress Int -> GuestAddress -> Either FrameError (M.Map GuestAddress Int)
compose walked visiting done entry
  | M.member entry done = Right done
  | S.member entry visiting = Left (FrameError entry Recursion)
  | otherwise = do
      let Walked own calls = walked M.! entry
      done' <- foldM (compose walked (S.insert entry visiting)) done (map edgeTarget calls)
      let low = minimum (own : [negate depth + done' M.! target | CallEdge _ target depth _ <- calls])
      when (low < negate stackLimit) (Left (FrameError entry StackUnbounded))
      pure (M.insert entry low done')

walk :: OwnedCode -> GuestAddress -> Either FrameError Walked
walk code entry@(GuestAddress start) = case filter (`extentContains` entry) (codeExtents code) of
  [extent] -> go extent M.empty [State start 0 Nothing] 0 []
  _ -> Left (FrameError entry FlowLeavesExtent)
  where
    GuestAddress base = codeBase code
    owned target = any (`extentContains` GuestAddress target) (codeExtents code)
    go _ _ [] low calls = Right (Walked low (S.toList (S.fromList calls)))
    go extent seen (state : pending) low calls =
      let pc = statePc state
          key = (stateSp state, stateFp state)
       in case M.lookup pc seen of
            Just keys | S.member key keys -> go extent seen pending low calls
            Just keys | any ((/= stateSp state) . fst) (S.toList keys) -> Left (FrameError (GuestAddress pc) StackUnbalanced)
            _
              | not (extentContains extent (GuestAddress pc)) || pc .&. 3 /= 0 -> Left (FrameError (GuestAddress pc) FlowLeavesExtent)
              | otherwise -> do
                  (next, edges) <- either (Left . FrameError (GuestAddress pc)) Right (step extent state (U.word32At (codeBytes code) (fromIntegral (pc - base))))
                  go extent (M.insertWith S.union pc (S.singleton key) seen) (next ++ pending) (minimum (low : map stateSp next)) (edges ++ calls)
    step extent state@(State pc sp fp) word = do
      let conditional = word `shiftR` 28 < 14
          following current = current {statePc = pc + 4}
          orSkipped successors = successors ++ [following state | conditional]
          advance current = Right (orSkipped [following current], [])
          clobber registers current = if r11 `elem` registers then current {stateFp = Nothing} else current
          bound offset = do
            unless (offset .&. 3 == 0 && offset <= 0) (Left StackUnbalanced)
            unless (offset >= negate stackLimit) (Left StackUnbounded)
          moveSp offset current = bound offset >> advance current {stateSp = offset}
          returning offset = do
            unless (offset == 0) (Left StackUnbalanced)
            Right (orSkipped [], [])
          plain registers
            | A.pc `elem` registers = Left PcWrite
            | A.sp `elem` registers = Left StackUnbounded
            | otherwise = advance (clobber registers state)
      case A.decode pc word of
        A.Rejected (A.Coprocessor 14)
          | word .&. 0x0f100010 == 0x0e100010 -> plain [A.Reg (fromIntegral ((word `shiftR` 12) .&. 0xf))]
        A.Rejected A.SystemRegister
          | Just (A.StatusWrite _ A.CurrentStatus _ _) <- A.statusWrite word -> advance state
        A.Rejected A.InterruptMask
          | not (testBit word 17) -> advance state
        A.Rejected _
          | word `elem` [0xf57ff05f, 0xf57ff04f, 0xf57ff01f] -> advance state
          | word .&. 0x0ff00fff == 0x01900f9f -> plain [A.Reg (fromIntegral ((word `shiftR` 12) .&. 15))]
          | word .&. 0x0ff00ff0 == 0x01800f90 -> plain [A.Reg (fromIntegral ((word `shiftR` 12) .&. 15))]
          | otherwise -> Left (UnwalkedWord word)
        A.ReadStatus _ register -> plain [register]
        A.WriteApplicationFlags {} -> advance state
        A.Hint {} -> advance state
        A.Branch _ True target
          | owned target -> Right ([following state], [CallEdge (GuestAddress pc) (GuestAddress target) (negate sp) Call])
          | otherwise -> Left (CallOutsideOwned (GuestAddress target))
        A.Branch _ False target
          | extentContains extent (GuestAddress target) -> Right (orSkipped [state {statePc = target}], [])
          | owned target -> do
              (successors, _) <- returning sp
              Right (successors, [CallEdge (GuestAddress pc) (GuestAddress target) 0 TailCall])
          | otherwise -> Left (BranchOutsideOwned (GuestAddress target))
        A.BranchExchange _ False register | register == A.lr -> returning sp
        A.BranchExchange {} -> Left PcWrite
        A.StoreMultiple _ register mode writeback registers
          | register == A.sp && writeback -> if mode == A.DecrementBefore then moveSp (sp - 4 * length registers) state else Left StackUnbalanced
          | otherwise -> advance (clobber [register | writeback] state)
        A.LoadMultiple _ register mode writeback registers
          | A.sp `elem` registers -> Left StackUnbounded
          | register == A.sp && writeback && mode == A.IncrementAfter ->
              let after = sp + 4 * length registers
               in if A.pc `elem` registers then bound after >> returning after else moveSp after (clobber registers state)
          | register == A.sp && writeback -> Left StackUnbalanced
          | A.pc `elem` registers -> Left PcWrite
          | otherwise -> advance (clobber (registers ++ [register | writeback]) state)
        A.DataProcessing _ operation _ destination source operand
          | destination == A.pc -> Left PcWrite
          | destination == A.sp -> case (operation, operand) of
              (A.ADD, A.Immediate amount) | source == A.sp -> moveSp (sp + fromIntegral amount) state
              (A.SUB, A.Immediate amount) | source == A.sp -> moveSp (sp - fromIntegral amount) state
              (A.ADD, A.Immediate amount) | source == r11, Just saved <- fp -> moveSp (saved + fromIntegral amount) state
              (A.SUB, A.Immediate amount) | source == r11, Just saved <- fp -> moveSp (saved - fromIntegral amount) state
              (A.MOV, A.ShiftedImmediate moved A.LSL 0) | moved == r11, Just saved <- fp -> moveSp saved state
              _ -> Left StackUnbounded
          | operation `elem` [A.CMP, A.CMN, A.TST, A.TEQ] -> advance state
          | destination == r11, operation == A.ADD, source == A.sp, A.Immediate amount <- operand -> advance state {stateFp = Just (sp + fromIntegral amount)}
          | destination == r11, operation == A.MOV, A.ShiftedImmediate moved A.LSL 0 <- operand, moved == A.sp -> advance state {stateFp = Just sp}
          | otherwise -> advance (clobber [destination] state)
        A.MoveWide _ register _ -> plain [register]
        A.MoveTop _ register _ -> plain [register]
        A.Multiply _ register _ _ _ -> plain [register]
        A.ZeroExtend _ register _ _ _ -> plain [register]
        A.Compute _ registers -> plain registers
        A.Load _ width _ register@(A.Reg number) addressing offset indexing
          | register == A.pc -> case (width, offset, indexing) of
              (A.Word, A.OffsetImmediate 4, A.PostIndexed) | addressing == A.sp -> bound (sp + 4) >> returning (sp + 4)
              _ -> Left PcWrite
          | width == A.Dual && number >= 12 -> Left StackUnbounded
          | A.sp `elem` loaded -> Left StackUnbounded
          | addressing == A.sp && indexing /= A.OffsetOnly -> case offset of
              A.OffsetImmediate amount -> moveSp (sp + fromIntegral amount) (clobber loaded state)
              A.OffsetRegister {} -> Left StackUnbounded
          | otherwise -> advance (clobber (loaded ++ [addressing | indexing /= A.OffsetOnly]) state)
          where
            loaded = register : [A.Reg (number + 1) | width == A.Dual]
        A.Store _ _ _ addressing offset indexing
          | addressing == A.sp && indexing /= A.OffsetOnly -> case offset of
              A.OffsetImmediate amount -> moveSp (sp + fromIntegral amount) state
              A.OffsetRegister {} -> Left StackUnbounded
          | otherwise -> advance (clobber [addressing | indexing /= A.OffsetOnly] state)
