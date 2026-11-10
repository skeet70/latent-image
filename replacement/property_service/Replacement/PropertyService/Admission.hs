{-# LANGUAGE GHC2021 #-}

-- | Decoded owned-code policy and bounded stack composition.
module Replacement.PropertyService.Admission
  ( FunctionName (..)
  , FunctionExtent (..)
  , CodeLayout (..)
  , MappedSpan (..)
  , AddressSpan (..)
  , PointerSpan (..)
  , StackOffset (..)
  , CallKind (..)
  , CallEdge (..)
  , EntryFrame (..)
  , CodeProof (..)
  , Violation (..)
  , AdmissionError (..)
  , admitCode
  , admitCodeWithCallbacks
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

data AddressSpan = AddressSpan GuestAddress GuestAddress deriving (Eq, Show)

data MappedSpan = ArmSpan AddressSpan | DataSpan AddressSpan deriving (Eq, Show)

data PointerSpan = OwnedReadonly AddressSpan | RetainedStorage AddressSpan | RetainedConfiguration AddressSpan deriving (Eq, Show)

-- | Bytes relative to the stack pointer at a function's entry.
newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)

data CodeLayout = CodeLayout
  { layoutBase :: GuestAddress
  , layoutFunctions :: [FunctionExtent]
  , layoutMappings :: [MappedSpan]
  , layoutLiteralPointers :: [PointerSpan]
  , -- | Extents holding no code.
    layoutData :: [(GuestAddress, GuestAddress)]
  , -- | Exact instruction sites admitted by the fixed backing-boundary policy.
    layoutStoreSites :: [GuestAddress]
  , -- | Stock leaf functions any owned function may call, with the lowest stack offset each reaches.
    layoutStockLeaves :: [(GuestAddress, StackOffset)]
  , -- | Retained boundary call sites, callees and required offsets from owned entry sp.
    layoutStockSites :: [(GuestAddress, GuestAddress, StackOffset)]
  }

data CallKind = Call | TailCall | StockLeafCall | StockLeafTailCall | StockSiteCall | CallbackCall | CallbackTailCall deriving (Eq, Ord, Show)

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
  , proofNonreturning :: S.Set GuestAddress
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
  | StockCallBelowEntry
  | DataExecuted
  | LiteralNotData
  | LayoutInvalid
  | LiteralPointerOutsideOwned
  deriving (Eq, Show)

data AdmissionError = AdmissionError GuestAddress Violation deriving (Eq, Show)

stackLimit :: Int
stackLimit = 4096

-- | Where the return address is: in lr, in the stack slot the function saved it to, or nowhere.
data LinkState = LinkLive | LinkSaved Int | LinkLost deriving (Eq, Ord, Show)

-- | Whether r11 holds the caller's value, holds another value with the caller's in a stack slot,
-- or holds another value with no copy of the caller's.
data FrameRegister = R11Entry | R11Saved Int | R11Changed deriving (Eq, Ord, Show)

data PathState = PathState
  { statePc :: Word32
  , stateSp :: Int
  , stateFp :: Maybe Int
  , stateLink :: LinkState
  , stateR11 :: FrameRegister
  , -- | Local aliases lie between current SP and entry SP, inclusive.
    stateAliases :: M.Map A.Reg StackOffset
  }

data Outcome = Outcome [PathState] Int [CallEdge] [Word32]

containsBytes :: Integer -> AddressSpan -> Bool
containsBytes address (AddressSpan (GuestAddress low) (GuestAddress high)) =
  toInteger low <= address && address + 4 <= toInteger high

pointerBounds :: PointerSpan -> AddressSpan
pointerBounds pointer = case pointer of
  OwnedReadonly bounds -> bounds
  RetainedStorage bounds -> bounds
  RetainedConfiguration bounds -> bounds

mappedCode :: CodeLayout -> Word32 -> Bool
mappedCode layout address = any (\mapping -> case mapping of
  ArmSpan bounds -> containsBytes (toInteger address) bounds
  DataSpan _ -> False) (layoutMappings layout)

mappedData :: CodeLayout -> Word32 -> Bool
mappedData layout address = any (\mapping -> case mapping of
  DataSpan bounds -> containsBytes (toInteger address) bounds
  ArmSpan _ -> False) (layoutMappings layout)

-- | Summaries require a successful instruction, flow and frame walk.
nonreturningEntries :: [FunctionExtent] -> CodeLayout -> B.ByteString -> S.Set Word32
nonreturningEntries callbacks layout bytes = close S.empty
  where
    GuestAddress base = layoutBase layout
    close known =
      let next = S.union known (S.fromList
            [ start
            | function@(FunctionExtent _ (GuestAddress start) _) <- layoutFunctions layout
            , not (S.member start known)
            , Right (frame, instructions, _) <- [walkFunction callbacks layout bytes known function]
            , all (not . returns) (S.toList instructions)
            , all (\edge -> case edgeKind edge of
                StockLeafTailCall -> False
                CallbackTailCall -> False
                TailCall -> let GuestAddress target = edgeTarget edge in S.member target known
                _ -> True) (frameCalls frame)
            ])
      in if next == known then known else close next
    returns address = case A.decode address (U.word32At bytes (fromIntegral (address - base))) of
      A.BranchExchange _ False register -> register == A.lr
      A.LoadMultiple _ _ _ _ registers -> A.pc `elem` registers
      _ -> False

admitCode :: CodeLayout -> B.ByteString -> Either AdmissionError CodeProof
admitCode = admitCodeWithCallbacks []

-- | Callback bodies and their stack depths are outside this proof.
admitCodeWithCallbacks :: [FunctionExtent] -> CodeLayout -> B.ByteString -> Either AdmissionError CodeProof
admitCodeWithCallbacks callbacks layout bytes = do
  let GuestAddress baseWord = layoutBase layout
      endInteger = toInteger baseWord + toInteger (B.length bytes)
      validSpan (AddressSpan (GuestAddress begin) (GuestAddress finish)) =
        begin .&. 3 == 0 && finish .&. 3 == 0 && baseWord <= begin && begin < finish && toInteger finish <= endInteger
      bounds mapping = case mapping of ArmSpan extent -> extent; DataSpan extent -> extent
      wordsOwned = [baseWord, baseWord + 4 .. baseWord + fromIntegral (B.length bytes) - 4]
  unless (not (B.null bytes) && B.length bytes `mod` 4 == 0 && endInteger <= toInteger (maxBound :: Word32)
    && all (validSpan . bounds) (layoutMappings layout)
    && all (\(FunctionExtent _ begin finish) -> validSpan (AddressSpan begin finish)) (layoutFunctions layout)
    && all (\address -> length [() | mapping <- layoutMappings layout, containsBytes (toInteger address) (bounds mapping)] == 1) wordsOwned
    && all (\(GuestAddress begin, GuestAddress finish) -> begin .&. 3 == 0 && finish .&. 3 == 0 && begin <= finish
      && all (mappedData layout) [begin, begin + 4 .. finish - 4]) (filter (uncurry (/=)) (layoutData layout)))
    (Left (AdmissionError (layoutBase layout) LayoutInvalid))
  let nonreturning = nonreturningEntries callbacks layout bytes
  walked <- mapM (walkFunction callbacks layout bytes nonreturning) (layoutFunctions layout)
  let instructions = S.unions [pcs | (_, pcs, _) <- walked]
      literals = S.unions [lits | (_, _, lits) <- walked]
      GuestAddress base = layoutBase layout
      inData address = any (\(GuestAddress low, GuestAddress high) -> low <= address && address < high) (layoutData layout)
      required = [address | address <- [base, base + 4 .. base + fromIntegral (B.length bytes) - 4], not (inData address)]
  case [address | address <- required, S.member address instructions == S.member address literals] of
    address : _ -> Left (AdmissionError (GuestAddress address) UnwalkedWord)
    [] -> pure ()
  let frames = [frame | (frame, _, _) <- walked]
  composed <- foldM (compose (M.fromList [(frameEntry frame, frame) | frame <- frames]) (M.fromList (layoutStockLeaves layout)) S.empty) M.empty (map frameEntry frames)
  pure (CodeProof frames composed (S.map GuestAddress instructions) (S.map GuestAddress literals) (S.map GuestAddress nonreturning))

-- | Owned edges compose through their callee's depth and stock leaf edges through the leaf's;
-- stock site edges add nothing.
compose :: M.Map GuestAddress EntryFrame -> M.Map GuestAddress StackOffset -> S.Set GuestAddress -> M.Map GuestAddress StackOffset -> GuestAddress -> Either AdmissionError (M.Map GuestAddress StackOffset)
compose frames leaves visiting done entry
  | M.member entry done = Right done
  | S.member entry visiting = Left (AdmissionError entry Recursion)
  | otherwise = case M.lookup entry frames of
      Nothing -> Left (AdmissionError entry FlowLeavesFunction)
      Just (EntryFrame _ (StackOffset own) calls) -> do
        done' <- foldM (compose frames leaves (S.insert entry visiting)) done [edgeTarget edge | edge <- calls, edgeKind edge `elem` [Call, TailCall]]
        let through (CallEdge _ target (StackOffset at) kind) = case kind of
              StockLeafCall -> let StackOffset leafLow = leaves M.! target in Just (at + leafLow)
              StockLeafTailCall -> let StackOffset leafLow = leaves M.! target in Just (at + leafLow)
              StockSiteCall -> Nothing
              CallbackCall -> Nothing
              CallbackTailCall -> Nothing
              _ -> let StackOffset calleeLow = done' M.! target in Just (at + calleeLow)
            low = minimum (own : [depth | Just depth <- map through calls])
        when (low < negate stackLimit) (Left (AdmissionError entry StackUnbounded))
        pure (M.insert entry (StackOffset low) done')

-- | Every return restores incoming sp, r11 and the return address.
walkFunction :: [FunctionExtent] -> CodeLayout -> B.ByteString -> S.Set Word32 -> FunctionExtent -> Either AdmissionError (EntryFrame, S.Set Word32, S.Set Word32)
walkFunction callbacks layout bytes nonreturning (FunctionExtent _ (GuestAddress start) (GuestAddress end)) =
  go M.empty [PathState start 0 Nothing LinkLive R11Entry M.empty] 0 [] S.empty
  where
    GuestAddress base = layoutBase layout
    callable = S.fromList [entry | FunctionExtent _ (GuestAddress entry) _ <- layoutFunctions layout]
    callbackEntries = S.fromList [entry | FunctionExtent _ (GuestAddress entry) _ <- callbacks]
    storeSites = S.fromList [site | GuestAddress site <- layoutStoreSites layout]
    leaves = S.fromList [target | (GuestAddress target, _) <- layoutStockLeaves layout]
    stockSites = M.fromList [((site, target), offset) | (GuestAddress site, GuestAddress target, StackOffset offset) <- layoutStockSites layout]
    go seen [] low calls literals =
      Right (EntryFrame (GuestAddress start) (StackOffset low) (S.toList (S.fromList calls)), M.keysSet seen, literals)
    go seen (state : pending) low calls literals =
      let pc = statePc state
          key = (stateSp state, stateFp state, stateLink state, stateR11 state, stateAliases state)
      in case M.lookup pc seen of
        Just keys | S.member key keys -> go seen pending low calls literals
        Just keys | any (\(sp, _, _, _, _) -> sp /= stateSp state) (S.toList keys) -> Left (AdmissionError (GuestAddress pc) StackUnbalanced)
        _
          | pc < start || pc >= end || pc .&. 3 /= 0 -> Left (AdmissionError (GuestAddress pc) FlowLeavesFunction)
          | not (mappedCode layout pc) -> Left (AdmissionError (GuestAddress pc) DataExecuted)
          | otherwise -> do
              let word = U.word32At bytes (fromIntegral (pc - base))
              Outcome next touched edges literalAddresses <- either (Left . AdmissionError (GuestAddress pc)) Right (step state word)
              go (M.insertWith S.union pc (S.singleton key) seen) (next ++ pending) (min low touched) (edges ++ calls) (S.union literals (S.fromList literalAddresses))
    step state@(PathState pc sp fp link frame aliases) word = do
      let instruction = A.decode pc word
          conditional = word `shiftR` 28 < 14
          skip paths = paths ++ [state {statePc = pc + 4} | conditional]
          advance next = Right (Outcome (skip [next {statePc = pc + 4}]) (min sp (stateSp next)) [] [])
          terminal = Right (Outcome (skip []) sp [] [])
          baseOffset register
            | register == A.sp = Just sp
            | register == r11 = fp
            | otherwise = case M.lookup register aliases of
                Just (StackOffset offset) -> Just offset
                Nothing -> Nothing
          affine operation source operand = do
            offset <- case (operation,operand) of
              (A.MOV,A.ShiftedImmediate moved A.LSL 0) -> toInteger <$> baseOffset moved
              (A.ADD,A.Immediate amount) -> (\known -> toInteger known + toInteger amount) <$> baseOffset source
              (A.SUB,A.Immediate amount) -> (\known -> toInteger known - toInteger amount) <$> baseOffset source
              _ -> Nothing
            if toInteger sp <= offset && offset <= 0
              then Just (StackOffset (fromInteger offset))
              else Nothing
          remember register known current = case known of
            Nothing -> current
            Just value@(StackOffset offset)
              | register == r11 -> current {stateFp=Just offset}
              | otherwise -> current {stateAliases=M.insert register value (stateAliases current)}
          slots = [slot | LinkSaved slot <- [link]] ++ [slot | R11Saved slot <- [frame]]
          clear low high = unless (all (\slot -> slot + 4 <= low || slot >= high) slots) (Left NonStackStore)
          bound offset = do
            unless (offset .&. 3 == 0) (Left StackMisaligned)
            unless (offset <= 0) (Left StackUnbalanced)
            unless (offset >= negate stackLimit) (Left StackUnbounded)
          moveSp offset current = current
            { stateSp = offset
            , stateAliases = M.filter (\(StackOffset address) -> offset <= address && address <= 0) (stateAliases current)
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
            , stateLink = if A.lr `elem` registers && stateLink current == LinkLive then LinkLost else stateLink current
            , stateR11 = if r11 `elem` registers && stateR11 current == R11Entry then R11Changed else stateR11 current
            , stateAliases = foldr M.delete (stateAliases current) registers
            }
          restored (register, address) current
            | register == A.lr = (written [register] current) {stateLink = if stateLink current == LinkSaved address then LinkLive else LinkLost}
            | register == r11 && stateR11 current == R11Saved address = (written [register] current) {stateFp = Nothing, stateR11 = R11Entry}
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
          | S.member target callable -> call Call
          | S.member target leaves -> call StockLeafCall
          | S.member target callbackEntries -> call CallbackCall
          | Just required <- M.lookup (pc, target) stockSites -> do
              unless (sp == required) (Left StockCallBelowEntry)
              call StockSiteCall
          | otherwise -> Left (CallOutsideOwned (GuestAddress target))
          where
            call kind = do
              unless (sp `mod` 8 == 0) (Left StackMisaligned)
              let next = state {statePc = pc + 4, stateLink = if link == LinkLive then LinkLost else link, stateAliases=M.empty}
              Right (Outcome (skip [next | not (S.member target nonreturning)]) sp [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) kind] [])
        A.Branch _ False target
          | target >= start && target < end -> Right (Outcome (skip [state {statePc = target}]) sp [] [])
          | S.member target callable -> tailTo TailCall
          | S.member target leaves -> tailTo StockLeafTailCall
          | S.member target callbackEntries -> tailTo CallbackTailCall
          | otherwise -> Left (BranchOutsideOwned (GuestAddress target))
          where
            tailTo kind = do
              returning state
              Right (Outcome (skip []) sp [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) kind] [])
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
          | not writeback, Just offset <- baseOffset register -> do
              let count = 4 * length registers
                  lowest = case mode of
                    A.IncrementAfter -> offset
                    A.IncrementBefore -> offset + 4
                    A.DecrementAfter -> offset - count + 4
                    A.DecrementBefore -> offset - count
              unless (lowest >= sp && lowest + count <= 0) (Left NonStackStore)
              clear lowest (lowest + count)
              advance state
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
          | otherwise -> advance (remember destination (affine operation source operand)
              (written [destination | operation `notElem` [A.CMP, A.CMN, A.TST, A.TEQ]] state))
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
          | indexing == A.OffsetOnly, Just saved <- baseOffset register, A.OffsetImmediate amount <- offset -> do
              let address = saved + fromIntegral amount
              unless (address >= sp && address + widthBytes width <= 0) (Left NonStackStore)
              clear address (address + widthBytes width)
              advance state
          | S.member pc storeSites && indexing == A.OffsetOnly -> advance state
          | otherwise -> Left NonStackStore
        A.Load _ width _ destination@(A.Reg number) register offset indexing
          | destination == A.pc || (width == A.Dual && number == 14) -> Left PcWrite
          | destination == A.sp || (width == A.Dual && number == 12) -> Left StackUnbounded
          | register == A.pc && indexing /= A.OffsetOnly -> Left PcWrite
          | register == A.pc -> case offset of
              A.OffsetImmediate amount -> do
                let literalAddress = toInteger pc + 8 + toInteger amount
                    address = fromInteger literalAddress
                    addresses = if width == A.Dual then [address, address + 4] else [address]
                unless (width `elem` [A.Word, A.Dual]
                  && literalAddress >= toInteger start && literalAddress + toInteger (widthBytes width) <= toInteger end
                  && address .&. 3 == 0) (Left LiteralOutsideFunction)
                unless (all (mappedData layout) addresses) (Left LiteralNotData)
                unless (all (\a -> any (containsBytes (toInteger (U.word32At bytes (fromIntegral (a - base)))) . pointerBounds) (layoutLiteralPointers layout)) addresses) (Left LiteralPointerOutsideOwned)
                Right (Outcome (skip [(written (loaded destination width) state) {statePc = pc + 4}]) sp [] addresses)
              A.OffsetRegister {} -> Left LiteralOutsideFunction
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
