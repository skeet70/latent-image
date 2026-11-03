{-# LANGUAGE OverloadedStrings #-}
module Replacement.TaskStorage.FrameProof
  ( Admission (..), Scratch (..), ByteCount (..), FrameOffset (..)
  , InstructionAddress (..), DataOffset (..), DataRoot (..), Location (..)
  , Direction (..), Access (..), Premise (..), ReturnPaths (..)
  , proveFrame, admissionValue
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), (.|.), complement, rotateR, shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

newtype ByteCount = ByteCount Word32 deriving (Eq, Show)
newtype FrameOffset = FrameOffset Integer deriving (Eq, Ord, Show)
newtype InstructionAddress = InstructionAddress Word32 deriving (Eq, Ord, Show)
newtype DataOffset = DataOffset Word32 deriving (Eq, Ord, Show)
newtype ReturnPaths = ReturnPaths Word32 deriving (Eq, Show)
data DataRoot = IndexedState | Cursor | Output deriving (Eq, Ord, Show)
data Location
  = FrameWord FrameOffset
  | GuestWord DataRoot DataOffset
  | ImageWord InstructionAddress
  | RetainedPoolWord
  deriving (Eq, Ord, Show)
data Direction = ReadWord | WriteWord deriving (Eq, Ord, Show)
data Access = Access
  { accessInstruction :: InstructionAddress
  , accessDirection :: Direction
  , accessLocation :: Location
  } deriving (Eq, Ord, Show)
data Premise
  = IncomingAapcsStack
  | OwnFrameStableStorage
  | ReachedGuestWordSemantics
  | GuestWritesDisjointImageAndAllocatedFrame
  | RuntimeAliasAndTcmResolution
  | RetainedLiteralAndImageLifetime
  deriving (Eq, Show)
data Scratch = Scratch
  { scratchBytes :: ByteCount
  , scratchLowFromRustSp :: FrameOffset
  , scratchHighFromRustSp :: FrameOffset
  } deriving (Eq, Show)
data Admission = Admission
  { admissionScratch :: Scratch
  , admissionAccesses :: [Access]
  , admissionReturnPaths :: ReturnPaths
  , admissionPremises :: [Premise]
  , admissionExecutionPcs :: [InstructionAddress]
  } deriving (Eq, Show)

data WordValue
  = UnknownScalar
  | Constant Word32
  | IndexAffine Word32 Word32
  | CallerRegister A.Reg
  | ReturnAddress
  | FrameAddress Integer
  | StateBase
  | GuestAddress DataRoot Word32
  | CodeAddress Word32
  deriving (Eq, Show)
data Frame = Frame
  { registers :: M.Map A.Reg WordValue
  , frameWords :: M.Map Integer WordValue
  , deepest :: Integer
  , accesses :: Set.Set Access
  }
data Environment = Environment InstructionAddress B.ByteString
newtype InstructionEncoding = InstructionEncoding Word32

data Transfer = Continue InstructionAddress Frame | Return WordValue Frame

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

addressNumber :: InstructionAddress -> Word32
addressNumber (InstructionAddress address) = address

following :: InstructionAddress -> InstructionAddress
following (InstructionAddress address) = InstructionAddress (address + 4)

preserved :: [A.Reg]
preserved = map A.Reg [4..11]

initial :: Frame
initial = Frame
  (M.fromList ([(A.Reg n,CallerRegister (A.Reg n)) | n <- [4..11]] ++
    [(A.Reg 0,IndexAffine 1 0),(A.Reg 1,UnknownScalar),(A.Reg 2,GuestAddress Output 0)
    ,(A.sp,FrameAddress 0),(A.lr,ReturnAddress)])) M.empty 0 Set.empty

register :: InstructionAddress -> Frame -> A.Reg -> WordValue
register pc frame selected
  | selected == A.pc = CodeAddress (addressNumber pc + 8)
  | otherwise = M.findWithDefault UnknownScalar selected (registers frame)

setRegister :: A.Reg -> WordValue -> Frame -> Either String Frame
setRegister selected value frame = do
  require (selected /= A.pc) "task-storage proof has an unsupported PC write"
  case (selected,value) of
    (A.Reg 13,FrameAddress offset) -> do
      require (offset <= 0 && offset >= -0x400 && offset `mod` 4 == 0)
        "task-storage emitted SP leaves its bounded own-frame policy"
      pure frame
        { registers = M.insert selected value (registers frame)
        , frameWords = M.filterWithKey (\wordOffset _ -> wordOffset >= offset && wordOffset < 0) (frameWords frame)
        , deepest = max (deepest frame) (negate offset)
        }
    (A.Reg 13,_) -> Left "task-storage emitted SP loses incoming-stack provenance"
    _ -> pure frame {registers = M.insert selected value (registers frame)}

constant :: Word32 -> WordValue
constant value
  | value == 0x40a0fc44 = StateBase
  | value == 0x4089ea98 = GuestAddress Cursor 0
  | otherwise = Constant value

shifted :: A.Shift -> Int -> WordValue -> Either String WordValue
shifted A.LSL 0 value = Right value
shifted kind count value = case value of
  Constant word -> case kind of
    A.LSL -> pure (constant (word `shiftL` count))
    A.LSR -> pure (constant (word `shiftR` (if count == 0 then 32 else count)))
    A.ASR -> pure (constant (fromIntegral ((fromIntegral word :: Int32) `shiftR` (if count == 0 then 32 else count))))
    A.ROR | count /= 0 -> pure (constant (rotateR word count))
    _ -> Left "task-storage proof does not admit carry-dependent shifts"
  IndexAffine coefficient offset | kind == A.LSL ->
    pure (IndexAffine (coefficient `shiftL` count) (offset `shiftL` count))
  UnknownScalar -> pure UnknownScalar
  CallerRegister _ -> pure UnknownScalar
  _ -> Left "task-storage shift has unsupported address or return provenance"

operand :: InstructionAddress -> Frame -> A.Operand -> Either String WordValue
operand _ _ (A.Immediate value) = pure (constant value)
operand pc frame (A.ShiftedImmediate selected kind count) = do
  require (selected /= A.pc) "task-storage arithmetic cannot use PC as a scalar operand"
  shifted kind count (register pc frame selected)
operand _ _ (A.ShiftedRegister _ _ _) = Left "task-storage proof does not admit register-controlled shifts"

addressLike :: WordValue -> Bool
addressLike value = case value of
  FrameAddress _ -> True
  StateBase -> True
  GuestAddress _ _ -> True
  CodeAddress _ -> True
  ReturnAddress -> True
  _ -> False

add :: WordValue -> WordValue -> Either String WordValue
add left right = case (left,right) of
  (FrameAddress offset,Constant bytes) -> pure (FrameAddress (offset + toInteger bytes))
  (Constant bytes,FrameAddress offset) -> pure (FrameAddress (offset + toInteger bytes))
  (CodeAddress base,Constant bytes) -> pure (CodeAddress (base + bytes))
  (GuestAddress root offset,Constant bytes) -> pure (GuestAddress root (offset + bytes))
  (Constant bytes,GuestAddress root offset) -> pure (GuestAddress root (offset + bytes))
  (StateBase,IndexAffine 12 offset) -> pure (GuestAddress IndexedState offset)
  (IndexAffine 12 offset,StateBase) -> pure (GuestAddress IndexedState offset)
  (IndexAffine coefficient offset,Constant bytes) -> pure (IndexAffine coefficient (offset + bytes))
  (Constant bytes,IndexAffine coefficient offset) -> pure (IndexAffine coefficient (offset + bytes))
  (IndexAffine a b,IndexAffine c d) -> pure (IndexAffine (a + c) (b + d))
  (Constant a,Constant b) -> pure (constant (a + b))
  _ -> scalarOperation left right

subtractValue :: WordValue -> WordValue -> Either String WordValue
subtractValue left right = case (left,right) of
  (FrameAddress offset,Constant bytes) -> pure (FrameAddress (offset - toInteger bytes))
  (CodeAddress base,Constant bytes) -> pure (CodeAddress (base - bytes))
  (GuestAddress root offset,Constant bytes) -> pure (GuestAddress root (offset - bytes))
  (IndexAffine coefficient offset,Constant bytes) -> pure (IndexAffine coefficient (offset - bytes))
  (IndexAffine a b,IndexAffine c d) -> pure (IndexAffine (a - c) (b - d))
  (Constant a,Constant b) -> pure (constant (a - b))
  _ -> scalarOperation left right

scalarOperation :: WordValue -> WordValue -> Either String WordValue
scalarOperation left right = do
  require (not (addressLike left || addressLike right)) "task-storage arithmetic loses an address or return capability"
  pure UnknownScalar

binary :: A.DataOp -> WordValue -> WordValue -> Either String WordValue
binary operation left right = case operation of
  A.ADD -> add left right
  A.SUB -> subtractValue left right
  A.RSB -> subtractValue right left
  _ -> do
    require (not (addressLike left || addressLike right)) "task-storage bit operation loses an address or return capability"
    case (left,right) of
      (Constant a,Constant b) -> case operation of
        A.AND -> pure (constant (a .&. b))
        A.EOR -> pure (constant (a `xor` b))
        A.ORR -> pure (constant (a .|. b))
        A.BIC -> pure (constant (a .&. complement b))
        _ -> Left "task-storage proof has an unsupported scalar operation"
      _ | operation `elem` [A.AND,A.EOR,A.ORR,A.BIC] -> pure UnknownScalar
      _ -> Left "task-storage proof has an unsupported scalar operation"

offsetValue :: InstructionAddress -> Frame -> A.Offset -> Either String (Bool,WordValue)
offsetValue _ _ (A.OffsetImmediate bytes)
  | bytes < 0 = pure (False,Constant (fromIntegral (negate (toInteger bytes))))
  | otherwise = pure (True,Constant (fromIntegral bytes))
offsetValue pc frame (A.OffsetRegister positive selected kind count) = do
  require (selected /= A.pc) "task-storage memory index cannot use PC"
  value <- shifted kind count (register pc frame selected)
  pure (positive,value)

addressPlan :: InstructionAddress -> Frame -> A.Reg -> A.Offset -> A.Indexing -> Either String (WordValue,Maybe WordValue)
addressPlan pc frame base offset mode = do
  case (base,offset) of
    (A.Reg 15,A.OffsetImmediate _) -> pure ()
    (A.Reg 15,_) -> Left "task-storage PC-relative load requires an immediate offset"
    _ -> pure ()
  (positive,bytes) <- offsetValue pc frame offset
  let original = register pc frame base
  updated <- (if positive then add else subtractValue) original bytes
  pure $ case mode of
    A.OffsetOnly -> (updated,Nothing)
    A.PreIndexed -> (updated,Just updated)
    A.PostIndexed -> (original,Just updated)

location :: Environment -> Frame -> WordValue -> Either String Location
location (Environment (InstructionAddress origin) image) frame address = case address of
  FrameAddress offset -> do
    sp <- case M.lookup A.sp (registers frame) of
      Just (FrameAddress value) -> pure value
      _ -> Left "task-storage proof lost allocated-frame SP"
    require (offset >= sp && offset + 4 <= 0 && offset `mod` 4 == 0)
      "task-storage frame access leaves allocated own-frame words"
    pure (FrameWord (FrameOffset offset))
  GuestAddress root offset -> do
    require (case root of IndexedState -> offset `elem` [0,4,8]; Cursor -> offset == 0; Output -> offset == 0)
      "task-storage guest access leaves its source-derived word footprint"
    pure (GuestWord root (DataOffset offset))
  CodeAddress addressWord
    | addressWord == 0x725dd4 -> pure RetainedPoolWord
    | otherwise -> do
        let offset = toInteger addressWord - toInteger origin
        require (offset >= 0 && offset + 4 <= toInteger (B.length image) && addressWord .&. 3 == 0)
          "task-storage literal load leaves its admitted image words"
        pure (ImageWord (InstructionAddress addressWord))
  _ -> Left "task-storage memory address has no source-derived capability"

recordAccess :: InstructionAddress -> Direction -> Location -> Frame -> Frame
recordAccess pc direction address frame = frame
  {accesses = Set.insert (Access pc direction address) (accesses frame)}

load :: Environment -> InstructionAddress -> Frame -> WordValue -> Either String (WordValue,Frame)
load environment@(Environment (InstructionAddress origin) image) pc frame address = do
  selected <- location environment frame address
  value <- case selected of
    FrameWord (FrameOffset offset) -> maybe (Left "task-storage frame load has no initialized word provenance") Right (M.lookup offset (frameWords frame))
    GuestWord _ _ -> pure UnknownScalar
    ImageWord (InstructionAddress literal) -> pure (constant (U.word32At image (fromIntegral (literal - origin))))
    RetainedPoolWord -> pure (Constant 0x40a10200)
  pure (value,recordAccess pc ReadWord selected frame)

store :: Environment -> InstructionAddress -> Frame -> WordValue -> WordValue -> Either String Frame
store environment pc frame address value = do
  selected <- location environment frame address
  changed <- case selected of
    FrameWord (FrameOffset offset) -> pure frame {frameWords = M.insert offset value (frameWords frame)}
    GuestWord _ _ -> do
      require (case value of Constant _ -> True; UnknownScalar -> True; IndexAffine _ _ -> True; _ -> False)
        "task-storage guest store cannot escape frame, caller or address provenance"
      pure frame
    _ -> Left "task-storage store cannot modify image or retained literals"
  pure (recordAccess pc WriteWord selected changed)

applyWriteback :: A.Reg -> Maybe WordValue -> Frame -> Either String Frame
applyWriteback _ Nothing frame = pure frame
applyWriteback base (Just value) frame = setRegister base value frame

prepareFrameAllocation :: A.Reg -> A.Indexing -> Maybe WordValue -> Frame -> Either String Frame
prepareFrameAllocation base A.PreIndexed update frame | base == A.sp = applyWriteback base update frame
prepareFrameAllocation _ _ _ frame = pure frame

single :: Environment -> InstructionAddress -> Bool -> A.Reg -> A.Reg -> A.Offset -> A.Indexing -> Frame -> Either String [Transfer]
single environment pc loading selected base offset mode frame = do
  require (base /= A.pc || loading && mode == A.OffsetOnly) "task-storage PC base cannot use store or writeback"
  require (loading || selected /= A.pc) "task-storage single store cannot publish an implementation-defined PC value"
  require (mode == A.OffsetOnly || base /= selected) "task-storage transfer writeback overlaps its source or destination"
  (address,update) <- addressPlan pc frame base offset mode
  allocated <- prepareFrameAllocation base mode update frame
  if loading
    then do
      (value,readFrame) <- load environment pc allocated address
      updated <- applyWriteback base update readFrame
      if selected == A.pc
        then pure [Return value updated]
        else do
          changed <- setRegister selected value updated
          pure [Continue (following pc) changed]
    else do
      let value = register pc frame selected
      changed <- store environment pc allocated address value
      updated <- applyWriteback base update changed
      pure [Continue (following pc) updated]

block :: Environment -> InstructionAddress -> Bool -> A.Reg -> A.BlockMode -> Bool -> [A.Reg] -> Frame -> Either String [Transfer]
block environment pc loading base mode writeback selected frame = do
  require (base /= A.pc && not (null selected) && A.sp `notElem` selected)
    "task-storage block transfer has an unsupported base or register list"
  require (not writeback || base `notElem` selected) "task-storage block writeback overlaps its register list"
  require (loading || A.pc `notElem` selected) "task-storage block store cannot publish PC"
  baseOffset <- case register pc frame base of
    FrameAddress offset -> pure offset
    _ -> Left "task-storage block transfer requires an own-frame address"
  let bytes = 4 * toInteger (length selected)
      (first,delta) = case mode of
        A.IncrementAfter -> (0,bytes)
        A.IncrementBefore -> (4,bytes)
        A.DecrementAfter -> (4 - bytes,negate bytes)
        A.DecrementBefore -> (negate bytes,negate bytes)
      update = if writeback then Just (FrameAddress (baseOffset + delta)) else Nothing
      addresses = [FrameAddress (baseOffset + first + 4 * toInteger index) | index <- [0 .. length selected - 1]]
  allocated <- if base == A.sp && delta < 0 then applyWriteback base update frame else pure frame
  if loading
    then do
      (values,readFrame) <- foldM (\(values,current) address -> do
        (value,changed) <- load environment pc current address
        pure (values ++ [value],changed)) ([],allocated) addresses
      let writes = zip selected values
      changed <- foldM (\current (destination,value) ->
        if destination == A.pc then pure current else setRegister destination value current) readFrame writes
      updated <- applyWriteback base update changed
      case lookup A.pc writes of
        Just value -> pure [Return value updated]
        Nothing -> pure [Continue (following pc) updated]
    else do
      changed <- foldM (\current (source,address) -> store environment pc current address (register pc frame source)) allocated (zip selected addresses)
      updated <- applyWriteback base update changed
      pure [Continue (following pc) updated]

execute :: Environment -> InstructionAddress -> InstructionEncoding -> Frame -> A.Instruction -> Either String [Transfer]
execute environment pc (InstructionEncoding encoded) frame instruction = case instruction of
  A.Branch (A.Cond condition) False target -> do
    require (condition <= 14) "task-storage branch condition is unsupported"
    pure (Continue (InstructionAddress target) frame : [Continue (following pc) frame | condition /= 14])
  A.BranchExchange (A.Cond 14) False source -> pure [Return (register pc frame source) frame]
  A.DataProcessing (A.Cond 14) operation flags destination source value -> do
    require (operation `notElem` [A.MOV,A.MVN] || source == A.Reg 0)
      "task-storage move has a nonzero reserved source field"
    require (operation `notElem` [A.TST,A.TEQ,A.CMP,A.CMN] || destination == A.Reg 0)
      "task-storage comparison has a nonzero reserved destination field"
    right <- operand pc frame value
    let left = register pc frame source
    if operation `elem` [A.TST,A.TEQ,A.CMP,A.CMN]
      then pure [Continue (following pc) frame]
      else do
        result <- case operation of
          A.MOV -> pure right
          A.MVN -> case right of
            Constant word -> pure (constant (complement word))
            _ -> do
              require (not (addressLike right)) "task-storage complement loses address or return provenance"
              pure UnknownScalar
          _ -> binary operation left right
        if destination == A.pc
          then do
            require (operation == A.MOV && not flags) "task-storage PC write is not an ordinary return"
            pure [Return result frame]
          else do
            require (destination /= A.sp || not flags) "task-storage SP arithmetic cannot update flags"
            changed <- setRegister destination result frame
            pure [Continue (following pc) changed]
  A.MoveWide (A.Cond 14) destination value -> do
    changed <- setRegister destination (constant value) frame
    pure [Continue (following pc) changed]
  A.MoveTop (A.Cond 14) destination value -> do
    low <- case register pc frame destination of
      Constant word -> pure word
      _ -> Left "task-storage MOVT requires a known scalar low word"
    changed <- setRegister destination (constant ((low .&. 0xffff) .|. (value `shiftL` 16))) frame
    pure [Continue (following pc) changed]
  A.Load (A.Cond 14) A.Word False selected base offset mode -> single environment pc True selected base offset mode frame
  A.Store (A.Cond 14) A.Word selected base offset mode -> single environment pc False selected base offset mode frame
  A.LoadMultiple (A.Cond 14) base mode writeback selected -> block environment pc True base mode writeback selected frame
  A.StoreMultiple (A.Cond 14) base mode writeback selected -> block environment pc False base mode writeback selected frame
  A.Hint (A.Cond 14) | encoded == 0xe320f000 -> pure [Continue (following pc) frame]
  _ -> Left "task-storage emitted instruction is outside frame/capability admission"

returned :: WordValue -> Frame -> Either String ()
returned target frame = do
  require (target == ReturnAddress) "task-storage return target loses incoming-return provenance"
  require (M.lookup A.sp (registers frame) == Just (FrameAddress 0)) "task-storage return does not restore incoming SP"
  require (all (\selected -> M.lookup selected (registers frame) == Just (CallerRegister selected)) preserved)
    "task-storage return does not preserve r4..r11"

proveFrame :: InstructionAddress -> B.ByteString -> InstructionAddress -> InstructionAddress -> Either String Admission
proveFrame origin image start end = do
  require (addressNumber start .&. 3 == 0 && addressNumber end .&. 3 == 0 && start < end) "task-storage Rust extent is not complete A32 words"
  walk 0 [(start,Set.empty,initial)] 0 Set.empty 0 Set.empty
  where
    environment = Environment origin image
    walk steps pending maximumDepth observed returns executed = do
      require (steps <= (4096 :: Int)) "task-storage frame proof exceeds its finite context policy"
      case pending of
        [] -> do
          require (returns > 0) "task-storage frame proof has no returning path"
          pure (Admission
            (Scratch (ByteCount (fromInteger maximumDepth)) (FrameOffset (negate maximumDepth)) (FrameOffset 0))
            (Set.toAscList observed) (ReturnPaths returns)
            [IncomingAapcsStack,OwnFrameStableStorage,ReachedGuestWordSemantics,GuestWritesDisjointImageAndAllocatedFrame
            ,RuntimeAliasAndTcmResolution,RetainedLiteralAndImageLifetime] (Set.toAscList executed))
        (pc,seen,frame):rest -> do
          require (pc >= start && toInteger (addressNumber pc) + 4 <= toInteger (addressNumber end) && addressNumber pc .&. 3 == 0) "task-storage control leaves its Rust body"
          require (Set.notMember pc seen) "task-storage frame proof does not admit a control cycle"
          let offset = toInteger (addressNumber pc) - toInteger (addressNumber origin)
          require (offset >= 0 && offset + 4 <= toInteger (B.length image)) "task-storage instruction leaves its linked image"
          let encoded = U.word32At image (fromInteger offset)
          successors <- execute environment pc (InstructionEncoding encoded) frame (A.decode (addressNumber pc) encoded)
          (queued,depth,allAccesses,count) <- foldM (\(queued,depth,allAccesses,count) transfer -> case transfer of
            Continue next changed -> pure
              ((next,Set.insert pc seen,changed):queued,max depth (deepest changed),Set.union allAccesses (accesses changed),count)
            Return value changed -> do
              returned value changed
              pure (queued,max depth (deepest changed),Set.union allAccesses (accesses changed),count + 1))
            (rest,maximumDepth,observed,returns) successors
          walk (steps + 1) queued depth allAccesses count (Set.insert pc executed)

admissionValue :: Admission -> Value
admissionValue admission =
  let Scratch (ByteCount bytes) (FrameOffset low) (FrameOffset high) = admissionScratch admission
      ReturnPaths paths = admissionReturnPaths admission
  in object
    [ "status" .= ("conditional" :: String)
    , "scratch" .= object ["rust_bytes" .= bytes,"low_from_rust_sp" .= low,"high_from_rust_sp" .= high]
    , "preserves" .= (["r4","r5","r6","r7","r8","r9","r10","r11","incoming-sp","incoming-return"] :: [String])
    , "return_paths" .= paths
    , "accesses" .= map accessValue (admissionAccesses admission)
    , "premises" .= map premiseText (admissionPremises admission)
    ]

accessValue :: Access -> Value
accessValue (Access (InstructionAddress pc) direction selected) = object
  [ "pc" .= pc, "direction" .= (case direction of ReadWord -> "read"; WriteWord -> "write" :: String)
  , "word_bytes" .= (4 :: Word32), "location" .= locationValue selected
  ]

locationValue :: Location -> Value
locationValue selected = case selected of
  FrameWord (FrameOffset offset) -> object ["root" .= ("allocated-own-frame" :: String),"offset_from_rust_sp" .= offset]
  GuestWord root (DataOffset offset) -> object
    (["root" .= rootText root,"offset" .= offset,"wrapping_address_arithmetic" .= True] ++ case root of
      IndexedState -> ["base" .= (0x40a0fc44 :: Word32),"stride" .= (12 :: Word32),"index" .= ("incoming-r0" :: String)]
      Cursor -> ["base" .= (0x4089ea98 :: Word32)]
      Output -> ["argument" .= ("incoming-r2" :: String)])
  ImageWord (InstructionAddress address) -> object ["root" .= ("linked-image-literal" :: String),"guest" .= address]
  RetainedPoolWord -> object ["root" .= ("retained-pool-base-literal" :: String),"guest" .= (0x725dd4 :: Word32)]

rootText :: DataRoot -> String
rootText root = case root of IndexedState -> "indexed-state"; Cursor -> "cursor"; Output -> "output-word"

premiseText :: Premise -> String
premiseText premise = case premise of
  IncomingAapcsStack -> "incoming Rust SP is eight-byte aligned and the derived own frame has mapped word semantics"
  OwnFrameStableStorage -> "the derived allocated frame is live ordinary word storage and no outside agent modifies it before deallocation"
  ReachedGuestWordSemantics -> "each reached guest word has the target's mapped word semantics, without new guest alignment or bounds checks"
  GuestWritesDisjointImageAndAllocatedFrame -> "all reached guest writes are disjoint from the entire linked image, adapter, retained pool-base literal and derived allocated Rust frame"
  RuntimeAliasAndTcmResolution -> "runtime mappings, aliases, TCM selection and surrounding-memory confinement remain unresolved"
  RetainedLiteralAndImageLifetime -> "the linked image and retained pool-base literal stay mapped and byte-stable throughout the transition"
