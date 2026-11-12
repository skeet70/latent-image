{-# LANGUAGE GHC2021 #-}

-- | Decoded owned-code policy, caller-frame arguments and bounded stack composition.
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
  , FrameSlot (..)
  , CallArgument (..)
  , EntryFrame (..)
  , CodeProof (..)
  , Violation (..)
  , AdmissionError (..)
  , admitCode
  , admitCodeWithCallbacks
  ) where

import qualified A32 as A
import Control.Monad (forM_, foldM, unless, when)
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Int (Int32)
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
  , -- | Extents declared as data without an ELF data mapping.
    layoutDeclaredData :: [(GuestAddress, GuestAddress)]
  , -- | Windows only the boundary entries may name.
    layoutBoundaryWindows :: [AddressSpan]
  , -- | Functions allowed to name those windows and to call through a slot word.
    layoutBoundaryEntries :: [GuestAddress]
  , -- | Exact instruction sites admitted by the fixed backing-boundary policy.
    layoutStoreSites :: [GuestAddress]
  , -- | Stock leaf functions any owned function may call, with the lowest stack offset each reaches.
    layoutStockLeaves :: [(GuestAddress, StackOffset)]
  , -- | Retained boundary call sites, callees and required offsets from owned entry sp.
    layoutStockSites :: [(GuestAddress, GuestAddress, StackOffset)]
  }

data CallKind = Call | TailCall | StockLeafCall | StockLeafTailCall | StockSiteCall | StockSiteTailCall | CallbackCall | CallbackTailCall deriving (Eq, Ord, Show)

data CallEdge = CallEdge
  { edgeSite :: GuestAddress
  , edgeTarget :: GuestAddress
  , edgeStack :: StackOffset
  , edgeKind :: CallKind
  }
  deriving (Eq, Ord, Show)

-- | Where a callee receives a pointer into its caller's frame.
data FrameSlot = ArgumentRegister A.Reg | ArgumentStackWord Int deriving (Eq, Ord, Show)

-- | A pointer a call site passes into a frame-argument slot, and the bytes the caller's
-- frame holds from it.
data CallArgument = CallArgument
  { callArgumentCallee :: GuestAddress
  , callArgumentSlot :: FrameSlot
  , callArgumentBytes :: Maybe Int
  }
  deriving (Eq, Show)

data EntryFrame = EntryFrame
  { frameEntry :: GuestAddress
  , -- | The lowest stack pointer the function itself reaches.
    frameLow :: StackOffset
  , frameCalls :: [CallEdge]
  , frameArguments :: [CallArgument]
  , -- | The incoming stack words, by byte offset from entry SP, the function reads.
    frameIncoming :: S.Set Int
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
  | BoundaryInvalid
  | LiteralPointerOutsideOwned
  | -- | A call site passes something other than a frame pointer into a frame-argument slot.
    FrameArgumentNotFrame
  | -- | A store, or the bytes a call site provides, run past the caller frame a slot names.
    FrameArgumentExceeded
  | -- | A register offset has no provable range.
    FrameOffsetUnbounded
  deriving (Eq, Show)

data AdmissionError = AdmissionError GuestAddress Violation deriving (Eq, Show)

stackLimit :: Int
stackLimit = 4096

-- | Where the return address is: in lr, in the stack slot the function saved it to, or nowhere.
data LinkState = LinkLive | LinkSaved Int | LinkLost deriving (Eq, Ord, Show)

-- | Whether r11 holds the caller's value, holds another value with the caller's in a stack slot,
-- or holds another value with no copy of the caller's.
data FrameRegister = R11Entry | R11Saved Int | R11Changed deriving (Eq, Ord, Show)

-- | The values congruent to `intervalLow` modulo `intervalStride` between the bounds.
-- A zero stride is the single value `intervalLow`.
data Interval = Interval
  { intervalLow :: Int
  , intervalHigh :: Int
  , intervalStride :: Int
  }
  deriving (Eq, Ord, Show)

-- | A register's value: a location in this function's frame, or one in its caller's frame
-- reached through a frame argument.
data Alias = LocalAlias StackOffset | CallerAlias FrameSlot Interval deriving (Eq, Ord, Show)

-- | Whether an interval survives a comparison on the path it selects.
data Refined = Kept Interval | Empty

-- | The frame arguments a walk may rely on, and the frame-argument slots each callee takes.
data FramePolicy = FramePolicy
  { -- | This function's own frame arguments and the bytes its callers' frames hold.
    policyOwn :: M.Map FrameSlot (Maybe Int)
  , -- | Every declared frame-argument slot, by callee.
    policyDeclared :: M.Map GuestAddress (S.Set FrameSlot)
  , -- | Whether an argument with no known bytes is refused rather than deferred.
    policyStrict :: Bool
  }

data PathState = PathState
  { statePc :: Word32
  , stateSp :: Int
  , stateFp :: Maybe Int
  , stateLink :: LinkState
  , stateR11 :: FrameRegister
  , -- | Aliases lie between current SP and entry SP, or in the caller's frame.
    stateAliases :: M.Map A.Reg Alias
  , -- | Frame words holding an alias, keyed by offset from entry SP.
    stateSlots :: M.Map Int Alias
  , stateCounters :: M.Map A.Reg Interval
  , -- | The lowest offset this function's own saved registers occupy.
    stateReserved :: Int
  }

data Outcome = Outcome [PathState] Int [CallEdge] [Word32] [CallArgument] [Int]

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

intervalValue :: Int -> Interval
intervalValue value = Interval value value 0

intervalShift :: Int -> Interval -> Interval
intervalShift amount (Interval low high stride) = Interval (low + amount) (high + amount) stride

intervalAdd :: Interval -> Interval -> Interval
intervalAdd (Interval lowA highA strideA) (Interval lowB highB strideB) =
  Interval (lowA + lowB) (highA + highB) (gcd strideA strideB)

intervalScale :: Int -> Interval -> Interval
intervalScale amount (Interval low high stride) =
  Interval (low `shiftL` amount) (high `shiftL` amount) (stride `shiftL` amount)

intervalNegate :: Interval -> Interval
intervalNegate (Interval low high stride) = Interval (negate high) (negate low) stride

intervalHolds :: Int -> Interval -> Bool
intervalHolds value (Interval low high stride) =
  low <= value && value <= high && (stride == 0 || (value - low) `mod` stride == 0)

intervalExclude :: Int -> Interval -> Maybe Interval
intervalExclude value interval@(Interval low high stride)
  | not (intervalHolds value interval) = Just interval
  | low == high = Nothing
  | value == low = Just (Interval (low + step) high stride)
  | value == high = Just (Interval low (high - step) stride)
  | otherwise = Just interval
  where
    step = max 1 stride

intervalAtLeast :: Int -> Interval -> Maybe Interval
intervalAtLeast bound (Interval low high stride)
  | low >= bound = Just (Interval low high stride)
  | high < bound = Nothing
  | otherwise = Just (Interval (low + ((bound - low + stride - 1) `div` stride) * stride) high stride)

intervalAtMost :: Int -> Interval -> Maybe Interval
intervalAtMost bound (Interval low high stride)
  | high <= bound = Just (Interval low high stride)
  | low > bound = Nothing
  | otherwise = Just (Interval low (high - ((high - bound + stride - 1) `div` stride) * stride) stride)

-- | The interval a register holds on a path whose condition holds.
refineCondition :: A.Cond -> Int -> Interval -> Refined
refineCondition (A.Cond code) bound interval = case code of
  0 -> if intervalHolds bound interval then Kept (intervalValue bound) else Empty
  1 -> maybe Empty Kept (intervalExclude bound interval)
  2 -> atLeast bound
  3 -> atMost (bound - 1)
  8 -> atLeast (bound + 1)
  9 -> atMost bound
  10 -> signedAtLeast bound
  11 -> signedAtMost (bound - 1)
  12 -> signedAtLeast (bound + 1)
  13 -> signedAtMost bound
  _ -> Kept interval
  where
    atLeast value = maybe Empty Kept (intervalAtLeast value interval)
    atMost value = maybe Empty Kept (intervalAtMost value interval)
    -- Signed and unsigned comparisons agree while the register cannot be negative.
    signedAtLeast value = if intervalLow interval >= 0 then atLeast value else Kept interval
    signedAtMost value = if intervalLow interval >= 0 then atMost value else Kept interval

immediate :: Word32 -> Int
immediate = fromIntegral . (fromIntegral :: Word32 -> Int32)

-- | The lenient policy a function is walked under while the frame-argument slots are
-- being discovered: every declared slot of the callee is a frame pointer with no bound yet.
ownPolicy :: M.Map GuestAddress (S.Set FrameSlot) -> FunctionExtent -> FramePolicy
ownPolicy declared (FunctionExtent _ (GuestAddress start) _) =
  FramePolicy (M.fromList [(slot, Nothing) | slot <- S.toList (M.findWithDefault S.empty (GuestAddress start) declared)]) declared False

-- | Summaries require a successful instruction, flow and frame walk.
nonreturningEntries :: [FunctionExtent] -> CodeLayout -> B.ByteString -> (FunctionExtent -> FramePolicy) -> S.Set Word32
nonreturningEntries callbacks layout bytes policyFor = close S.empty
  where
    GuestAddress base = layoutBase layout
    close known =
      let next = S.union known (S.fromList
            [ start
            | function@(FunctionExtent _ (GuestAddress start) _) <- layoutFunctions layout
            , not (S.member start known)
            , Right (frame, instructions, _) <- [walkFunction callbacks layout bytes known (policyFor function) function]
            , all (not . returns) (S.toList instructions)
            , all (\edge -> case edgeKind edge of
                StockLeafTailCall -> False
                StockSiteTailCall -> False
                CallbackTailCall -> False
                TailCall -> let GuestAddress target = edgeTarget edge in S.member target known
                _ -> True) (frameCalls frame)
            ])
      in if next == known then known else close next
    returns address = case A.decode address (U.word32At bytes (fromIntegral (address - base))) of
      A.BranchExchange _ False _ -> True
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
  unless (all (\(GuestAddress begin, GuestAddress finish) -> begin .&. 3 == 0 && finish .&. 3 == 0 && begin <= finish)
      (layoutDeclaredData layout)
    && all (\(AddressSpan (GuestAddress begin) (GuestAddress finish)) -> begin .&. 3 == 0 && finish .&. 3 == 0 && begin < finish)
      (layoutBoundaryWindows layout)
    && all (\(GuestAddress entry) -> any (\(FunctionExtent _ (GuestAddress begin) _) -> begin == entry) (layoutFunctions layout))
      (layoutBoundaryEntries layout))
    (Left (AdmissionError (layoutBase layout) BoundaryInvalid))
  declared <- discoverArguments callbacks layout bytes
  let declaredMap = M.fromListWith S.union [(callee, S.singleton slot) | (callee, slot) <- S.toList declared]
      nonreturning = nonreturningEntries callbacks layout bytes (ownPolicy declaredMap)
  lenient <- mapM (\function -> walkFunction callbacks layout bytes nonreturning (ownPolicy declaredMap function) function)
    (layoutFunctions layout)
  order <- callOrder layout (map (\(frame, _, _) -> frame) lenient)
  (_, walked) <- foldM (admitInOrder callbacks layout bytes nonreturning declaredMap) (M.empty, []) order
  let frames = map (\(frame, _, _) -> frame) (reverse walked)
      instructions = S.unions [pcs | (_, pcs, _) <- walked]
      literals = S.unions [lits | (_, _, lits) <- walked]
      GuestAddress base = layoutBase layout
      inData address = any (\(GuestAddress low, GuestAddress high) -> low <= address && address < high) (layoutData layout ++ layoutDeclaredData layout)
      required = [address | address <- [base, base + 4 .. base + fromIntegral (B.length bytes) - 4], not (inData address)]
  case [address | address <- required, S.member address instructions == S.member address literals] of
    address : _ -> Left (AdmissionError (GuestAddress address) UnwalkedWord)
    [] -> pure ()
  composed <- foldM (compose (M.fromList [(frameEntry frame, frame) | frame <- frames]) (M.fromList (layoutStockLeaves layout)) S.empty) M.empty (map frameEntry frames)
  pure (CodeProof frames composed (S.map GuestAddress instructions) (S.map GuestAddress literals) (S.map GuestAddress nonreturning))

-- | The frame-argument slots each callee receives a caller frame pointer in, derived from
-- the call sites that pass one.
discoverArguments :: [FunctionExtent] -> CodeLayout -> B.ByteString -> Either AdmissionError (S.Set (GuestAddress, FrameSlot))
discoverArguments callbacks layout bytes = close S.empty
  where
    close declared =
      let declaredMap = M.fromListWith S.union [(callee, S.singleton slot) | (callee, slot) <- S.toList declared]
          nonreturning = nonreturningEntries callbacks layout bytes (ownPolicy declaredMap)
          frames = [frame | function <- layoutFunctions layout
                   , Right (frame, _, _) <- [walkFunction callbacks layout bytes nonreturning (ownPolicy declaredMap function) function]]
          incomingReads = M.fromList [(frameEntry frame, frameIncoming frame) | frame <- frames]
          -- A stack word is a frame argument only if the callee reads it.
          isReadSlot _ (ArgumentRegister _) = True
          isReadSlot callee (ArgumentStackWord offset) = maybe False (S.member offset) (M.lookup callee incomingReads)
          found = S.fromList
            [ (callArgumentCallee argument, callArgumentSlot argument)
            | frame <- frames
            , argument <- frameArguments frame
            , isReadSlot (callArgumentCallee argument) (callArgumentSlot argument)
            ]
          next = S.union declared found
      in if next == declared then Right declared else close next

-- | The owned functions, every caller before the functions it calls.
callOrder :: CodeLayout -> [EntryFrame] -> Either AdmissionError [GuestAddress]
callOrder layout frames = go entries (S.fromList [(frameEntry frame, edgeTarget edge)
      | frame <- frames, edge <- frameCalls frame, edgeKind edge `elem` [Call, TailCall]]) []
  where
    entries = S.fromList [GuestAddress entry | FunctionExtent _ (GuestAddress entry) _ <- layoutFunctions layout]
    go remaining edges done
      | S.null remaining = Right done
      | not (null ready) = go (remaining `S.difference` S.fromList ready) edges (done ++ ready)
      | otherwise = case S.toList remaining of
          first : _ -> Left (AdmissionError first Recursion)
          [] -> Right done
      where
        ready = [entry | entry <- S.toList remaining
          , not (any (\(source, target) -> target == entry && S.member source remaining) (S.toList edges))]

-- | Walk one function with the bytes its callers provide, and pass those bytes on.
admitInOrder :: [FunctionExtent] -> CodeLayout -> B.ByteString -> S.Set Word32 -> M.Map GuestAddress (S.Set FrameSlot)
  -> (M.Map GuestAddress (M.Map FrameSlot (Maybe Int)), [(EntryFrame, S.Set Word32, S.Set Word32)])
  -> GuestAddress
  -> Either AdmissionError (M.Map GuestAddress (M.Map FrameSlot (Maybe Int)), [(EntryFrame, S.Set Word32, S.Set Word32)])
admitInOrder callbacks layout bytes nonreturning declared (bounds, walked) entry = do
  function <- case [item | item@(FunctionExtent _ (GuestAddress start) _) <- layoutFunctions layout, start == target] of
    [item] -> Right item
    _ -> Left (AdmissionError entry FlowLeavesFunction)
  let policy = FramePolicy (M.findWithDefault M.empty entry bounds) declared True
  (frame, instructions, literals) <- walkFunction callbacks layout bytes nonreturning policy function
  let provided = M.fromListWith (M.unionWith min)
        [(callArgumentCallee argument, M.singleton (callArgumentSlot argument) (callArgumentBytes argument))
        | argument <- frameArguments frame]
  pure (M.unionWith (M.unionWith min) bounds provided, (frame, instructions, literals) : walked)
  where
    GuestAddress target = entry

-- | Owned edges compose through their callee's depth and stock leaf edges through the leaf's;
-- stock site edges add nothing.
compose :: M.Map GuestAddress EntryFrame -> M.Map GuestAddress StackOffset -> S.Set GuestAddress -> M.Map GuestAddress StackOffset -> GuestAddress -> Either AdmissionError (M.Map GuestAddress StackOffset)
compose frames leaves visiting done entry
  | M.member entry done = Right done
  | S.member entry visiting = Left (AdmissionError entry Recursion)
  | otherwise = case M.lookup entry frames of
      Nothing -> Left (AdmissionError entry FlowLeavesFunction)
      Just (EntryFrame _ (StackOffset own) calls _ _) -> do
        done' <- foldM (compose frames leaves (S.insert entry visiting)) done [edgeTarget edge | edge <- calls, edgeKind edge `elem` [Call, TailCall]]
        let through (CallEdge _ target (StackOffset at) kind) = case kind of
              StockLeafCall -> let StackOffset leafLow = leaves M.! target in Just (at + leafLow)
              StockLeafTailCall -> let StackOffset leafLow = leaves M.! target in Just (at + leafLow)
              StockSiteCall -> Nothing
              StockSiteTailCall -> Nothing
              CallbackCall -> Nothing
              CallbackTailCall -> Nothing
              _ -> let StackOffset calleeLow = done' M.! target in Just (at + calleeLow)
            low = minimum (own : [depth | Just depth <- map through calls])
        when (low < negate stackLimit) (Left (AdmissionError entry StackUnbounded))
        pure (M.insert entry (StackOffset low) done')

-- | Every return restores incoming sp, r11 and the return address.
walkFunction :: [FunctionExtent] -> CodeLayout -> B.ByteString -> S.Set Word32 -> FramePolicy -> FunctionExtent -> Either AdmissionError (EntryFrame, S.Set Word32, S.Set Word32)
walkFunction callbacks layout bytes nonreturning policy (FunctionExtent _ (GuestAddress start) (GuestAddress end)) =
  go M.empty [entry] 0 [] S.empty [] S.empty
  where
    GuestAddress base = layoutBase layout
    callable = S.fromList [entryPoint | FunctionExtent _ (GuestAddress entryPoint) _ <- layoutFunctions layout]
    callbackEntries = S.fromList [entryPoint | FunctionExtent _ (GuestAddress entryPoint) _ <- callbacks]
    storeSites = S.fromList [site | GuestAddress site <- layoutStoreSites layout]
    leaves = S.fromList [target | (GuestAddress target, _) <- layoutStockLeaves layout]
    stockSites = M.fromList [((site, target), offset) | (GuestAddress site, GuestAddress target, StackOffset offset) <- layoutStockSites layout]
    boundaryExtents = [(low, high) | FunctionExtent _ (GuestAddress low) (GuestAddress high) <- layoutFunctions layout
      , GuestAddress low `elem` layoutBoundaryEntries layout]
    inBoundary address = any (\(low, high) -> low <= address && address < high) boundaryExtents
    decoded = [(address, word, A.decode address word) | address <- [start, start + 4 .. end - 4]
      , let word = U.word32At bytes (fromIntegral (address - base))]
    -- A conditional instruction's flags come from the comparison immediately before it, so a
    -- branch target cannot rely on the preceding word.
    branchTargets = S.fromList [target | (_, _, A.Branch _ _ target) <- decoded]
    offsetRegisters = S.fromList [register | (_, _, A.Store _ _ _ _ (A.OffsetRegister _ register _ _) _) <- decoded]
    compareGuard pc
      | pc - 4 < start || S.member pc branchTargets = Nothing
      | otherwise = case A.decode (pc - 4) (U.word32At bytes (fromIntegral (pc - 4 - base))) of
          A.DataProcessing (A.Cond 14) operation True source _ (A.Immediate amount)
            | operation == A.CMP -> Just (source, immediate amount)
            | operation == A.CMN -> Just (source, negate (immediate amount))
          _ -> Nothing
    entry = PathState
      { statePc = start
      , stateSp = 0
      , stateFp = Nothing
      , stateLink = LinkLive
      , stateR11 = R11Entry
      , stateAliases = M.fromList [(register, CallerAlias slot (intervalValue 0))
          | slot@(ArgumentRegister register) <- M.keys (policyOwn policy)]
      , stateSlots = M.empty
      , stateCounters = M.empty
      , stateReserved = 0
      }
    admittedPointer pc address =
      let value = toInteger (U.word32At bytes (fromIntegral (address - base)))
      in if any (containsBytes value) (layoutBoundaryWindows layout)
        then inBoundary pc
        else any (containsBytes value . pointerBounds) (layoutLiteralPointers layout)
    go seen [] low calls literals arguments incoming =
      Right ( EntryFrame (GuestAddress start) (StackOffset low) (S.toList (S.fromList calls)) arguments incoming
            , M.keysSet seen, literals )
    go seen (state : pending) low calls literals arguments incoming =
      let pc = statePc state
          key = ( stateSp state, stateFp state, stateLink state, stateR11 state, stateAliases state
                , stateSlots state, M.filterWithKey (\register _ -> S.member register offsetRegisters) (stateCounters state)
                , stateReserved state )
      in case M.lookup pc seen of
        Just keys | S.member key keys -> go seen pending low calls literals arguments incoming
        Just keys | any (\(sp, _, _, _, _, _, _, _) -> sp /= stateSp state) (S.toList keys) ->
          Left (AdmissionError (GuestAddress pc) StackUnbalanced)
        _
          | pc < start || pc >= end || pc .&. 3 /= 0 -> Left (AdmissionError (GuestAddress pc) FlowLeavesFunction)
          | not (mappedCode layout pc) -> Left (AdmissionError (GuestAddress pc) DataExecuted)
          | otherwise -> do
              let word = U.word32At bytes (fromIntegral (pc - base))
              Outcome next touched edges literalAddresses passed readOffsets <- either (Left . AdmissionError (GuestAddress pc)) Right (step state word)
              go (M.insertWith S.union pc (S.singleton key) seen) (next ++ pending) (min low touched)
                (edges ++ calls) (S.union literals (S.fromList literalAddresses)) (passed ++ arguments) (S.union incoming (S.fromList readOffsets))
    -- The value a frame-argument slot holds at a call site. A stack word is named by its
    -- byte offset from the callee's entry SP.
    argumentValue current slot = case slot of
      ArgumentRegister register -> M.lookup register (stateAliases current)
      ArgumentStackWord offset -> lookup offset
        [ (address - stateSp current, alias)
        | (address, alias) <- M.toList (stateSlots current)
        , address >= stateSp current
        , (address - stateSp current) `mod` 4 == 0
        ]
    outgoingSlots current = [ArgumentRegister register | register <- map A.Reg [0 .. 3]]
      ++ [ArgumentStackWord (address - stateSp current)
         | (address, _) <- M.toList (stateSlots current)
         , address >= stateSp current
         , (address - stateSp current) `mod` 4 == 0
         ]
    -- Whether an alias is a proven pointer into a caller's frame.
    frameAlias (LocalAlias _) = True
    frameAlias (CallerAlias slot _) = M.member slot (policyOwn policy)
    -- The bytes a caller's frame holds from an alias it passes on.
    argumentBytes current alias = case alias of
      LocalAlias (StackOffset offset) -> Just (stateReserved current - offset)
      CallerAlias slot offset -> case M.lookup slot (policyOwn policy) of
        Just (Just limit) -> Just (limit - intervalHigh offset)
        _ -> Nothing
    callArguments target current = do
      when (policyStrict policy) $ forM_ (S.toList (M.findWithDefault S.empty target (policyDeclared policy))) $ \slot ->
        case argumentValue current slot of
          Nothing -> Left FrameArgumentNotFrame
          Just alias -> unless (frameAlias alias) (Left FrameArgumentNotFrame)
      pure [ CallArgument target slot (argumentBytes current alias)
           | slot <- outgoingSlots current
           , Just alias <- [argumentValue current slot]
           , frameAlias alias
           ]
    step state@(PathState pc sp fp link frame aliases slots counters reserved) word = do
      let instruction = A.decode pc word
          conditional = word `shiftR` 28 < 14
          condition = A.Cond (word `shiftR` 28)
          guard = compareGuard pc
          restrict selected current = case guard of
            Nothing -> Just current
            Just (register, comparison) -> case M.lookup register (stateCounters current) of
              Nothing -> Just current
              Just interval -> case refineCondition (if selected then condition else A.negateCond condition) comparison interval of
                Empty -> Nothing
                Kept narrowed -> Just current {stateCounters = M.insert register narrowed (stateCounters current)}
          skip paths = paths ++ [falsePath {statePc = pc + 4} | conditional, Just falsePath <- [restrict False state]]
          taken next = [refined | Just refined <- [restrict True next]]
          advance next = Right (Outcome (skip [next {statePc = pc + 4}]) (min sp (stateSp next)) [] [] [] [])
          terminal = Right (Outcome (skip []) sp [] [] [] [])
          baseAlias register
            | register == A.sp = Just (LocalAlias (StackOffset sp))
            | register == r11 = LocalAlias . StackOffset <$> fp
            | otherwise = M.lookup register aliases
          -- The offset from entry SP a local alias and an immediate name.
          localAddress (LocalAlias (StackOffset offset)) amount = Just (offset + fromIntegral amount)
          localAddress _ _ = Nothing
          affine operation source operand = do
            source' <- baseAlias source
            let shifted amount = case source' of
                  LocalAlias (StackOffset offset)
                    | let moved = offset + amount, sp <= moved && moved <= 0 -> Just (LocalAlias (StackOffset moved))
                    | otherwise -> Nothing
                  CallerAlias slot offset -> Just (CallerAlias slot (intervalShift amount offset))
            case (operation, operand) of
              (A.MOV, A.ShiftedImmediate moved A.LSL 0) | moved == source -> Just source'
              (A.ADD, A.Immediate amount) -> shifted (immediate amount)
              (A.SUB, A.Immediate amount) -> shifted (negate (immediate amount))
              _ -> Nothing
          counter operation source operand = case (operation, operand) of
            (A.MOV, A.Immediate amount) -> Just (intervalValue (immediate amount))
            (A.MOV, A.ShiftedImmediate moved A.LSL 0) -> M.lookup moved counters
            (A.ADD, A.Immediate amount) -> intervalShift (immediate amount) <$> M.lookup source counters
            (A.SUB, A.Immediate amount) -> intervalShift (negate (immediate amount)) <$> M.lookup source counters
            _ -> Nothing
          remember register known current = case known of
            Nothing -> current
            Just value@(LocalAlias (StackOffset offset))
              | register == r11 -> current {stateFp=Just offset}
              | otherwise -> current {stateAliases=M.insert register value (stateAliases current)}
            Just value -> current {stateAliases=M.insert register value (stateAliases current)}
          savedSlots = [slot | LinkSaved slot <- [link]] ++ [slot | R11Saved slot <- [frame]]
          clear low high = unless (all (\slot -> slot + 4 <= low || slot >= high) savedSlots) (Left NonStackStore)
          bound offset = do
            unless (offset .&. 3 == 0) (Left StackMisaligned)
            unless (offset <= 0) (Left StackUnbalanced)
            unless (offset >= negate stackLimit) (Left StackUnbounded)
          inFrame offset (LocalAlias (StackOffset address)) = offset <= address && address <= 0
          inFrame _ CallerAlias {} = True
          moveSp offset current = current
            { stateSp = offset
            , stateAliases = M.filter (inFrame offset) (stateAliases current)
            , stateSlots = M.filterWithKey (\address _ -> offset <= address && address <= 0) (stateSlots current)
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
            , stateCounters = foldr M.delete (stateCounters current) registers
            }
          restored (register, address) current
            | register == A.lr = (written [register] current) {stateLink = if stateLink current == LinkSaved address then LinkLive else LinkLost}
            | register == r11 && stateR11 current == R11Saved address = (written [register] current) {stateFp = Nothing, stateR11 = R11Entry}
            | otherwise = written [register] current
          -- A frame word holds an alias only while a word store put one there.
          recordSlot address width register current
            | width == A.Word, address .&. 3 == 0, Just alias <- M.lookup register (stateAliases current) =
                current {stateSlots = M.insert address alias (stateSlots current)}
            | otherwise = current
              { stateSlots = M.filterWithKey (\offset _ -> offset + 4 <= address || address + widthBytes width <= offset) (stateSlots current) }
          placeAlias writtenBack register slot offset current
            | not writtenBack = current
            | otherwise = current {stateAliases = M.insert register (CallerAlias slot offset) (stateAliases current)}
          leaving current = do
            unless (stateSp current == 0) (Left StackUnbalanced)
            unless (stateR11 current == R11Entry) (Left FramePointerLost)
          returning current = do
            leaving current
            unless (stateLink current == LinkLive) (Left ReturnAddressLost)
          -- A store through a frame argument stays inside the bytes the caller's frame holds.
          frameStore slot address width = case M.lookup slot (policyOwn policy) of
            Nothing -> when (policyStrict policy) (Left FrameArgumentNotFrame)
            Just Nothing -> when (policyStrict policy) (Left FrameArgumentExceeded)
            Just (Just limit) ->
              unless (intervalLow address >= 0 && intervalHigh address + width <= limit) (Left FrameArgumentExceeded)
          heldSlot width destination address current
            | width == A.Word, Just value <- M.lookup address (stateSlots current) =
                current {stateAliases = M.insert destination value (stateAliases current)}
            | otherwise = current
          held destination value current =
            (written (loaded destination A.Word) current) {stateAliases = M.insert destination value (stateAliases current)}
          loaded destination@(A.Reg number) width = destination : [A.Reg (number + 1) | width == A.Dual]
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
              passed <- callArguments (GuestAddress target) state
              let next = state {statePc = pc + 4, stateLink = if link == LinkLive then LinkLost else link, stateAliases=M.empty, stateSlots=M.empty}
              Right (Outcome (skip (if S.member target nonreturning then [] else taken next)) sp
                [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) kind] [] passed [])
        A.Branch _ False target
          | target >= start && target < end -> Right (Outcome (skip [state {statePc = target}]) sp [] [] [] [])
          | S.member target callable -> tailTo TailCall
          | S.member target leaves -> tailTo StockLeafTailCall
          | S.member target callbackEntries -> tailTo CallbackTailCall
          | otherwise -> Left (BranchOutsideOwned (GuestAddress target))
          where
            tailTo kind = do
              returning state
              passed <- callArguments (GuestAddress target) state
              Right (Outcome (skip []) sp [CallEdge (GuestAddress pc) (GuestAddress target) (StackOffset sp) kind] [] passed [])
        A.BranchExchange _ linked register
          | register == A.lr -> if linked then Left PcWrite else returning state >> terminal
          | inBoundary pc -> indirect linked
          | otherwise -> Left PcWrite
          where
            indirect True = do
              unless (sp `mod` 8 == 0) (Left StackMisaligned)
              let next = state {statePc = pc + 4, stateLink = if link == LinkLive then LinkLost else link, stateAliases = M.empty, stateSlots = M.empty}
              Right (Outcome (skip (taken next)) sp [CallEdge (GuestAddress pc) (GuestAddress pc) (StackOffset sp) StockSiteCall] [] [] [])
            indirect False = do
              returning state
              Right (Outcome (skip []) sp [CallEdge (GuestAddress pc) (GuestAddress pc) (StackOffset sp) StockSiteTailCall] [] [] [])
        A.StoreMultiple _ register mode writeback registers
          | register == A.sp && mode == A.DecrementBefore && writeback -> do
              let after = sp - 4 * length registers
                  addressed = zip registers [after, after + 4 ..]
                  savedAt wanted = lookup wanted addressed
              bound after
              clear after sp
              let moved = (moveSp after state)
                    { stateLink = case (link, savedAt A.lr) of
                        (LinkLive, Just slot) -> LinkSaved slot
                        _ -> link
                    , stateR11 = case (frame, savedAt r11) of
                        (R11Entry, Just slot) -> R11Saved slot
                        _ -> frame
                    }
              advance (foldl' (\current (address, register') -> recordSlot address A.Word register' current)
                moved {stateReserved = min reserved after} (zip [after, after + 4 ..] registers))
          | Just (CallerAlias slot offset) <- baseAlias register, not writeback -> do
              let count = 4 * length registers
                  address = case mode of
                    A.IncrementAfter -> offset
                    A.IncrementBefore -> intervalShift 4 offset
                    A.DecrementAfter -> intervalShift (4 - count) offset
                    A.DecrementBefore -> intervalShift (negate count) offset
              frameStore slot address count
              advance state
          | not writeback, Just (LocalAlias (StackOffset offset)) <- baseAlias register -> do
              let count = 4 * length registers
                  lowest = case mode of
                    A.IncrementAfter -> offset
                    A.IncrementBefore -> offset + 4
                    A.DecrementAfter -> offset - count + 4
                    A.DecrementBefore -> offset - count
              unless (lowest >= sp && lowest + count <= 0) (Left NonStackStore)
              clear lowest (lowest + count)
              advance (foldl' (\current (address, register') -> recordSlot address A.Word register' current)
                state (zip [lowest, lowest + 4 ..] registers))
          | writeback, Just (LocalAlias (StackOffset offset)) <- baseAlias register -> do
              let count = 4 * length registers
                  lowest = case mode of
                    A.IncrementAfter -> offset
                    A.IncrementBefore -> offset + 4
                    A.DecrementAfter -> offset - count + 4
                    A.DecrementBefore -> offset - count
                  after = case mode of
                    A.IncrementAfter -> offset + count
                    A.IncrementBefore -> offset + 4
                    A.DecrementAfter -> offset - count + 4
                    A.DecrementBefore -> offset - count
              unless (lowest >= sp && lowest + count <= 0) (Left NonStackStore)
              clear lowest (lowest + count)
              let recorded = foldl' (\current (address, register') -> recordSlot address A.Word register' current)
                    state (zip [lowest, lowest + 4 ..] registers)
              advance (if sp <= after && after <= 0
                         then remember register (Just (LocalAlias (StackOffset after))) recorded
                         else written [register] recorded)
          | otherwise -> Left NonStackStore
        A.LoadMultiple _ register mode writeback registers
          | A.sp `elem` registers -> Left StackUnbounded
          | register == A.sp && mode == A.IncrementAfter && writeback -> do
              let after = sp + 4 * length registers
                  addressed = zip registers [sp, sp + 4 ..]
                  afterLoads = foldl (flip restored) state [item | item@(loadedRegister, _) <- addressed, loadedRegister /= A.pc]
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
          | otherwise -> advance (counted operation source operand
              (remember destination (affine operation source operand)
                (written [destination | operation `notElem` [A.CMP, A.CMN, A.TST, A.TEQ]] state)))
          where
            counted op source' operand' current
              | op `elem` [A.CMP, A.CMN, A.TST, A.TEQ] = current
              | Just interval <- counter op source' operand' = current {stateCounters = M.insert destination interval (stateCounters current)}
              | otherwise = current
        A.Store _ width value register offset indexing
          | register == A.sp -> case offset of
              A.OffsetImmediate amount -> do
                let moved = sp + fromIntegral amount
                    address = if indexing == A.PostIndexed then sp else moved
                    after = if indexing == A.OffsetOnly then sp else moved
                unless (address >= after && address + widthBytes width <= 0) (Left NonStackStore)
                clear address (address + widthBytes width)
                bound after
                advance ((recordSlot address width value (moveSp after state))
                  {stateReserved = if indexing == A.OffsetOnly then reserved else min reserved after})
              A.OffsetRegister {} -> Left NonStackStore
          | Just (CallerAlias slot base') <- baseAlias register -> case offset of
              A.OffsetImmediate amount -> do
                let shifted = intervalShift (fromIntegral amount) base'
                    address = if indexing == A.PostIndexed then base' else shifted
                frameStore slot address (widthBytes width)
                advance (placeAlias (indexing /= A.OffsetOnly) register slot shifted state)
              A.OffsetRegister addend moved shift amount -> do
                interval <- maybe (Left FrameOffsetUnbounded) Right (M.lookup moved counters)
                scaled <- case (shift, amount) of
                  (A.LSL, 0) -> Right interval
                  (A.LSL, _) -> Right (intervalScale amount interval)
                  _ -> Left FrameOffsetUnbounded
                let shifted = if addend then scaled else intervalNegate scaled
                    moved' = intervalAdd base' shifted
                    address = if indexing == A.PostIndexed then base' else moved'
                frameStore slot address (widthBytes width)
                advance (placeAlias (indexing /= A.OffsetOnly) register slot moved' state)
          | indexing == A.OffsetOnly, Just alias <- baseAlias register, A.OffsetImmediate amount <- offset
          , Just address <- localAddress alias amount -> do
              unless (address >= sp && address + widthBytes width <= 0) (Left NonStackStore)
              clear address (address + widthBytes width)
              advance (recordSlot address width value state)
          | S.member pc storeSites && indexing == A.OffsetOnly -> advance state
          | inBoundary pc -> advance (written [register | indexing /= A.OffsetOnly] state)
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
                unless (all (admittedPointer pc) addresses) (Left LiteralPointerOutsideOwned)
                Right (Outcome (skip [(written (loaded destination width) state) {statePc = pc + 4}]) sp [] addresses [] [])
              A.OffsetRegister {} -> Left LiteralOutsideFunction
          | register == A.sp && indexing /= A.OffsetOnly -> case offset of
              A.OffsetImmediate amount -> do
                let moved = sp + fromIntegral amount
                    address = if indexing == A.PostIndexed then sp else moved
                    after = moved
                bound after
                let current = if width == A.Word then restored (destination, address) state else written (loaded destination width) state
                advance (moveSp after (heldSlot width destination address current))
              A.OffsetRegister {} -> Left StackUnbounded
          | indexing == A.OffsetOnly, Just alias <- baseAlias register, A.OffsetImmediate amount <- offset
          , Just address <- localAddress alias amount, width == A.Word, Just value <- M.lookup address slots ->
              advance (held destination value state)
          | indexing == A.OffsetOnly, Just alias <- baseAlias register, A.OffsetImmediate amount <- offset
          , Just address <- localAddress alias amount, address >= 0, address .&. 3 == 0 ->
              Right (Outcome (skip [(held destination (CallerAlias (ArgumentStackWord address) (intervalValue 0)) state) {statePc = pc + 4}]) sp [] [] [] [address])
          | otherwise -> advance (written (loaded destination width ++ [register | indexing /= A.OffsetOnly]) state)
        _ -> do
          let registers = writtenRegisters instruction
          when (A.pc `elem` registers) (Left PcWrite)
          when (A.sp `elem` registers) (Left StackUnbounded)
          advance (written registers state)

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
