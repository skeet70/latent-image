{-# LANGUAGE OverloadedStrings #-}
module Emulation.InterfaceInitializer
  ( Role (..), Contract, parseOriginalSource, deriveContract, contractValue, declarations, validateReceipt
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, withText, (.:), (.=))
import qualified Data.Aeson as J
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (sort)
import qualified Data.Set as Set
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.LoaderPrefixWitness as P
import Emulation.FirstStageWitness (Cpsr (..), leWordAt, sha256Hex)
import qualified Emulation.PeerStageWitness as W
import qualified Emulation.PollPeriod as K
import FujiAF.ClassicChrome (stockFirmwareSha256)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import FujiAF.JSON (decodeStrictIntegers)
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as S


data Role = Retained | Owned deriving (Eq, Show)

newtype StackBytes = StackBytes Word32
data ExpectedStore = ExpectedStore P.Guest Word32
data Contract = Contract Role P.Contract P.Guest P.Span StackBytes F.Sha256 (Maybe F.Sha256) F.Sha256 CallerFramePremise [ExpectedStore]

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

at :: [F.Symbol] -> String -> Either String Word32
at symbols name = F.symbolAddress <$> F.lookupSymbol symbols name

branchLink :: Word32 -> Word32 -> Either String Word32
branchLink site target = do
  let displacement = toInteger target - toInteger site - 8
  require (target .&. 3 == 0 && displacement >= -0x2000000 && displacement < 0x2000000)
    "initializer entry is outside the A32 BL range"
  pure (0xeb000000 .|. (fromInteger (displacement `div` 4) .&. 0xffffff))

declarations :: [F.Symbol] -> Either String StageDeclarations
declarations symbols = do
  prefix <- S.loaderPrefixDeclarations symbols
  target <- at symbols "xm1_interface_initializer_entry"
  start <- at symbols "__secondary_stage_start"
  end <- at symbols "__secondary_stage_end"
  require (start <= target && target < end) "initializer adapter leaves the staged image"
  replacement <- branchLink 0xd0aac target
  patch <- F.loaderWordPatch (F.LoaderId 1) 0xd0aac 0xebfffe5f replacement
  pure prefix { declaredLoaderPatches = declaredLoaderPatches prefix ++ [patch] }

adapterExtent :: B.ByteString -> [F.Symbol] -> Either String Word32
adapterExtent image symbols = do
  origin <- at symbols "__secondary_stage_start"
  adapter <- F.lookupSymbol symbols "xm1_interface_initializer_entry"
  target <- at symbols "xm1_interface_initializer_publish"
  let start = F.symbolAddress adapter
  call <- branchLink (start + 8) target
  let expected =
        [ 0xe92d4010, 0xe10f4000, call, 0xe3c4420f, 0xe3844206, 0xe128f004, 0xe8bd4010
        , 0xe30b0e6c, 0xe344000e, 0xe30d1120, 0xe344100e, 0xe30128f0, 0xe344200f
        , 0xe30e3f40, 0xe344300d, 0xe3a0c078, 0xe12fff1e
        ]
  observed <- mapM (leWordAt image . (\offset -> fromIntegral start - fromIntegral origin + offset)) [0, 4 .. 64]
  require (F.symbolSize adapter == 68 && observed == expected) "initializer adapter does not implement its admitted eight-byte frame and exact return ABI"
  pure 8

data CallerFramePremise = CallerFramePremise
  { callerStackTop :: P.Guest
  , callerEntrySp :: P.Guest
  , callerFramePointer :: P.Guest
  }

callerFramePremise :: B.ByteString -> Either String CallerFramePremise
callerFramePremise loader = do
  load <- A.decode 0xd00a8 <$> leWordAt loader 0xa8
  literal <- case load of
    A.Load (A.Cond 14) A.Word False (A.Reg 4) base (A.OffsetImmediate offset) A.OffsetOnly | base == A.pc ->
      pure (0xb0 + toInteger offset)
    _ -> Left "initializer caller stack literal load differs"
  require (literal >= 0 && literal + 4 <= toInteger (B.length loader)) "initializer caller stack literal leaves loader B"
  top <- leWordAt loader (fromInteger literal)
  move <- A.decode 0xd00ac <$> leWordAt loader 0xac
  push <- A.decode 0xd09ac <$> leWordAt loader 0x9ac
  frame <- A.decode 0xd09b0 <$> leWordAt loader 0x9b0
  require (case move of
    A.DataProcessing (A.Cond 14) A.MOV False destination _ (A.ShiftedImmediate source A.LSL 0) ->
      destination == A.sp && source == A.Reg 4
    _ -> False) "initializer caller does not install its stack literal"
  registers <- case push of
    A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers
      | base == A.sp && registers == [A.Reg 4,A.Reg 5,A.Reg 11,A.lr] -> pure registers
    _ -> Left "initializer caller frame differs"
  let bytes = 4 * fromIntegral (length registers)
      entry = top - bytes
  require (top >= 0x40000418 && top < 0x40100000 && top .&. 3 == 0) "initializer caller stack leaves its linear alias window"
  require (case frame of
    A.DataProcessing (A.Cond 14) A.ADD False (A.Reg 11) base (A.Immediate offset) ->
      base == A.sp && offset + 4 == bytes
    _ -> False) "initializer caller frame pointer differs"
  pure (CallerFramePremise (P.Guest top) (P.Guest entry) (P.Guest (entry + bytes - 4)))

physicalAlias :: Word32 -> Maybe Integer
physicalAlias address
  | address < 0x100000 = Just (toInteger address)
  | address >= 0x40000000 && address < 0x40100000 = Just (toInteger address - 0x40000000)
  | otherwise = Nothing

data KnownWord = UnknownWord | KnownWord Word32 deriving (Eq, Ord)

wordValue :: KnownWord -> Maybe Word32
wordValue UnknownWord = Nothing
wordValue (KnownWord value) = Just value

knownWord :: Maybe Word32 -> KnownWord
knownWord = maybe UnknownWord KnownWord

-- Stack and return-slot offsets are relative to the Rust function's incoming SP.
data StackState = StackState
  { stackAliases :: M.Map A.Reg Integer
  , incomingReturns :: Set.Set A.Reg
  , savedReturns :: Set.Set Integer
  , scalarWords :: M.Map A.Reg KnownWord
  , scalarFlags :: K.Flags
  } deriving (Eq)

data ControlContext = ControlContext P.Guest (M.Map A.Reg KnownWord) K.Flags deriving (Eq, Ord)
data BaseUpdate = BaseUpdate KnownWord (Maybe Integer)
data AddressWrap = UnwrappedAddress | WrappedAddress deriving (Eq)
data AddressPlan = AddressPlan KnownWord (Maybe Integer) (Maybe BaseUpdate) AddressWrap
data StoreTarget = FrameStore Integer | PublicationStore

operandRegisters :: A.Operand -> [A.Reg]
operandRegisters operand = case operand of
  A.Immediate _ -> []
  A.ShiftedImmediate register _ _ -> [register]
  A.ShiftedRegister register _ shift -> [register,shift]

offsetRegisters :: A.Offset -> [A.Reg]
offsetRegisters (A.OffsetImmediate _) = []
offsetRegisters (A.OffsetRegister _ register _ _) = [register]

stackExtent :: CallerFramePremise -> StackBytes -> [ExpectedStore] -> B.ByteString -> [F.Symbol] -> Either String Word32
stackExtent caller (StackBytes adapterBytes) stores image symbols = do
  origin <- at symbols "__secondary_stage_start"
  symbol <- F.lookupSymbol symbols "xm1_interface_initializer_publish"
  let P.Guest callerSp = callerEntrySp caller
      P.Guest stackTop = callerStackTop caller
      incomingSp = callerSp - adapterBytes
  incomingPhysical <- maybe (Left "initializer Rust stack lacks its physical alias") Right (physicalAlias incomingSp)
  topPhysical <- maybe (Left "initializer caller stack lacks its physical alias") Right (physicalAlias stackTop)
  publications <- Set.fromList <$> mapM (\(ExpectedStore (P.Guest guest) _) ->
    maybe (Left "initializer publication lacks its physical alias") Right (physicalAlias guest)) stores
  require (all (\address -> address .&. 3 == 0 && (address + 4 <= incomingPhysical - 0x400 || address >= topPhysical))
    (Set.toList publications)) "initializer publication overlaps the Rust, adapter or caller stack"
  let start = F.symbolAddress symbol
      end = toInteger start + toInteger (F.symbolSize symbol)
      code = K.CodeImage origin image
      unknownFlags = K.Flags Nothing Nothing Nothing Nothing
      initial = StackState (M.singleton A.sp 0) (Set.singleton A.lr) Set.empty
        (M.fromList [(A.Reg index, if index == 13 then KnownWord incomingSp else UnknownWord) | index <- [0..14]]) unknownFlags
      setWord register value state = state {scalarWords = M.insert register (knownWord value) (scalarWords state)}
      setSp offset state = setWord A.sp (Just (fromInteger (toInteger incomingSp + offset))) state
      walk [] _ _ _ deepest returns = do
        require (returns > (0 :: Int)) "initializer Rust function has no admitted return"
        require (deepest <= 0x400) "initializer compiler stack exceeds the admitted workspace scratch budget"
        pure (fromInteger deepest)
      walk ((pc, state):pending) visited aliasVisits steps deepest returns = do
        require (steps < (100000 :: Int)) "initializer compiler proof exceeded 100000 value contexts"
        require (pc >= start && toInteger pc < end && pc .&. 3 == 0) "initializer branch leaves its function"
        case M.lookup pc aliasVisits of
          Just previous -> require (previous == stackAliases state) "initializer compiler loop has inconsistent stack aliases"
          Nothing -> pure ()
        let context = ControlContext (P.Guest pc) (scalarWords state) (scalarFlags state)
            aliasesSeen = M.insert pc (stackAliases state) aliasVisits
        case M.lookup context visited of
          Nothing -> advance pc context state pending visited aliasesSeen steps deepest returns
          Just previous -> do
            let common = state
                  { incomingReturns = Set.intersection (incomingReturns previous) (incomingReturns state)
                  , savedReturns = Set.intersection (savedReturns previous) (savedReturns state)
                  }
            if common == previous
              then walk pending visited aliasesSeen (steps + 1) deepest returns
              else advance pc context common pending visited aliasesSeen steps deepest returns
      advance pc context state pending visited aliasVisits steps deepest returns = do
        sp <- maybe (Left "initializer compiler SP is not statically derived") Right (M.lookup A.sp (stackAliases state))
        require (sp <= 0 && sp >= -0x400) "initializer compiler SP leaves its admitted frame budget"
        require (sp `mod` 4 == 0) "initializer compiler SP is not word aligned"
        instructionWord <- maybe (Left "initializer instruction leaves its image") Right (K.imageWord code pc)
        let instruction = A.decode pc instructionWord
            aliases = stackAliases state
            returnRegisters = incomingReturns state
            slots = savedReturns state
            flags = scalarFlags state
            readWord register = if register == A.pc then Just (pc + 8) else M.lookup register (scalarWords state) >>= wordValue
            next updated = [(pc + 4, updated)]
            forget registers = state
              { stackAliases = foldr M.delete aliases registers
              , incomingReturns = Set.difference returnRegisters (Set.fromList registers)
              , scalarWords = foldr (\register -> M.insert register UnknownWord) (scalarWords state) registers
              }
            returning condition proven updated = do
              require proven "initializer compiler indirect target is not its incoming return address"
              require (M.lookup A.sp (stackAliases updated) == Just 0) "initializer compiler return does not restore SP"
              pure ([(pc + 4, updated) | condition /= A.Cond 14], updated, (0 :: Integer), True)
            regular condition registers = do
              require (A.sp `notElem` registers && A.pc `notElem` registers) "unadmitted compiler SP or PC write"
              require (condition == A.Cond 14 || all (`M.notMember` aliases) registers)
                "conditional compiler write leaves an unresolved stack alias"
              let updated = forget registers
              pure (next updated, updated, (0 :: Integer), False)
            clearSlots low bytes = Set.filter (\saved -> saved + 4 <= low || low + bytes <= saved)
            trimSlots newSp = Set.filter (>= newSp)
            valueIsReturn operation source operand = case (operation, operand) of
              (A.MOV, A.ShiftedImmediate register A.LSL 0) -> Set.member register returnRegisters
              (A.ADD, A.Immediate 0) -> Set.member source returnRegisters
              (A.SUB, A.Immediate 0) -> Set.member source returnRegisters
              _ -> False
            assignReturn condition destination known updated =
              let definite = known && (condition == A.Cond 14 || Set.member destination returnRegisters)
                  changed = if definite then Set.insert destination else Set.delete destination
               in updated {incomingReturns = changed (incomingReturns updated)}
            operandValue operand = case operand of
              A.Immediate value -> (Just value, if (instructionWord `shiftR` 8) .&. 0xf == 0 then K.flagC flags else Just (testBit value 31))
              A.ShiftedImmediate register shift amount -> K.shiftImmediateKnown shift amount (readWord register) (K.flagC flags)
              A.ShiftedRegister _ _ _ -> (Nothing, Nothing)
            addressPlan base offset indexing = do
              require (indexing == A.OffsetOnly || base /= A.pc) "initializer compiler writes back PC"
              require (A.pc `notElem` offsetRegisters offset) "initializer compiler address uses PC as its register index"
              require (all (`M.notMember` aliases) (offsetRegisters offset)) "initializer compiler address uses a stack-derived index"
              let displacement = case offset of
                    A.OffsetImmediate amount -> Just (toInteger amount)
                    A.OffsetRegister up register shift amount ->
                      (\value -> if up then toInteger value else negate (toInteger value))
                        <$> fst (K.shiftImmediateKnown shift amount (readWord register) (K.flagC flags))
                  raw = (+) <$> (toInteger <$> readWord base) <*> displacement
                  effective = fromInteger <$> raw
                  relative = (+) <$> M.lookup base aliases <*> displacement
                  target = if indexing == A.PostIndexed then readWord base else effective
                  location = if indexing == A.PostIndexed then M.lookup base aliases else relative
                  update = if indexing == A.OffsetOnly then Nothing else Just (BaseUpdate (knownWord effective) relative)
                  wrapped = if maybe False (\value -> value < 0 || value > 0xffffffff) raw then WrappedAddress else UnwrappedAddress
              require (M.notMember base aliases || displacement /= Nothing) "initializer compiler frame index is unresolved"
              pure (AddressPlan (knownWord target) location update wrapped)
            writeback condition base plan updated = case plan of
              AddressPlan _ _ Nothing _ -> pure updated
              AddressPlan _ _ (Just (BaseUpdate value relative)) _ -> do
                require (condition == A.Cond 14 || M.notMember base aliases) "initializer compiler conditional stack writeback"
                require (base /= A.sp || (condition == A.Cond 14 && maybe False (\offset -> offset >= -0x400 && offset <= 0 && offset `mod` 4 == 0) relative))
                  "initializer compiler SP writeback is unresolved or unbounded"
                let changedAliases = if condition == A.Cond 14
                      then maybe (M.delete base (stackAliases updated)) (\offset -> M.insert base offset (stackAliases updated)) relative
                      else M.delete base (stackAliases updated)
                    changed = updated
                      { stackAliases = changedAliases
                      , incomingReturns = Set.delete base (incomingReturns updated)
                      , scalarWords = M.insert base (if condition == A.Cond 14 then value else UnknownWord) (scalarWords updated)
                      }
                pure (if base == A.sp then changed {savedReturns = maybe Set.empty (\offset -> trimSlots offset (savedReturns changed)) relative} else changed)
            frameLocation allocation bytes (AddressPlan value relative _ _) = case relative of
              Just low -> do
                require (low >= allocation && low + bytes <= 0) "initializer compiler access leaves its allocated frame"
                pure (Just low)
              Nothing -> pure $ do
                physical <- wordValue value >>= physicalAlias
                let low = physical - incomingPhysical
                if low >= allocation && low + bytes <= 0 then Just low else Nothing
            storeTarget allocation plan@(AddressPlan value _ _ wrapped) = do
              require (wrapped == UnwrappedAddress) "initializer compiler store address wraps"
              location <- frameLocation allocation 4 plan
              case location of
                Just low -> pure (FrameStore low)
                Nothing -> do
                  physical <- maybe (Left "initializer compiler store address is unproved") Right (wordValue value >>= physicalAlias)
                  require (Set.member physical publications) "initializer compiler store leaves its Rust frame and publication words"
                  pure PublicationStore
        (successors, updated, used, returned) <- case instruction of
          A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers | base == A.sp -> do
            require (not (null registers) && A.sp `notElem` registers && A.pc `notElem` registers) "initializer compiler push has unadmitted registers"
            require (all (`M.notMember` aliases) registers) "initializer compiler push spills a stack alias"
            let bytes = 4 * toInteger (length registers)
                low = sp - bytes
                saved = Set.fromList [offset | (register,offset) <- zip registers [low,low + 4..], Set.member register returnRegisters]
                changed = setSp low state
                  { stackAliases = M.insert A.sp low aliases
                  , savedReturns = Set.union saved (clearSlots low bytes slots)
                  }
            require (low >= -0x400) "initializer compiler push exceeds its Rust frame"
            pure (next changed, changed, negate low, False)
          A.LoadMultiple (A.Cond 14) base A.IncrementAfter True registers | base == A.sp -> do
            require (not (null registers) && A.sp `notElem` registers) "initializer compiler pop has unadmitted registers"
            let newSp = sp + 4 * toInteger (length registers)
                restored = [(register,Set.member offset slots) | (register,offset) <- zip registers [sp,sp + 4..]]
                loaded = Set.fromList [register | (register,True) <- restored, register /= A.pc]
                forgotten = forget registers
                changed = setSp newSp forgotten
                  { stackAliases = M.insert A.sp newSp (stackAliases forgotten)
                  , incomingReturns = Set.union loaded (incomingReturns forgotten)
                  , savedReturns = trimSlots newSp slots
                  }
            require (newSp <= 0) "initializer compiler pop enters the caller frame"
            if A.pc `elem` registers
              then returning (A.Cond 14) (lookup A.pc restored == Just True) changed
              else pure (next changed, changed, 0, False)
          A.DataProcessing condition operation writesFlags destination source operand -> do
            let (operand2, carry) = operandValue operand
                (result, calculatedFlags) = K.alu operation (readWord source) operand2 carry flags
                updateFlags changed = if writesFlags
                  then changed {scalarFlags = if condition == A.Cond 14 then calculatedFlags else unknownFlags}
                  else changed
                updateScalar changed = updateFlags (setWord destination (if condition == A.Cond 14 then result else Nothing) changed)
            if operation `elem` [A.CMP, A.CMN, A.TST, A.TEQ]
              then let changed = updateFlags state in pure (next changed, changed, 0, False)
              else if destination == A.pc
                then do
                  require (not writesFlags) "initializer compiler return writes CPSR"
                  returning condition (valueIsReturn operation source operand) state
                else if condition == A.Cond 14
                  then do
                    let inputs = operandRegisters operand ++ [source | operation `notElem` [A.MOV, A.MVN]]
                        value = case (operation, operand) of
                          (A.MOV, A.ShiftedImmediate register A.LSL 0) -> M.lookup register aliases
                          (A.ADD, A.Immediate amount) -> (+ toInteger amount) <$> M.lookup source aliases
                          (A.SUB, A.Immediate amount) -> subtract (toInteger amount) <$> M.lookup source aliases
                          _ -> Nothing
                    require (all (`M.notMember` aliases) inputs || value /= Nothing) "unadmitted compiler stack-derived computation"
                    require (destination /= A.sp || maybe False (<= 0) value) "unadmitted compiler SP computation"
                    let changedAliases = maybe (M.delete destination aliases) (\offset -> M.insert destination offset aliases) value
                        changed = updateScalar (assignReturn condition destination (valueIsReturn operation source operand)
                          (state {stackAliases = changedAliases}))
                        bounded = case M.lookup A.sp changedAliases of
                          Just newSp -> changed {savedReturns = trimSlots newSp slots}
                          Nothing -> changed
                    pure (next bounded, bounded, 0, False)
                  else do
                    let inputs = operandRegisters operand ++ [source | operation `notElem` [A.MOV, A.MVN]]
                    require (all (`M.notMember` aliases) inputs) "conditional compiler computation propagates a stack alias"
                    (_, changed, _, _) <- regular condition [destination]
                    let restored = updateScalar (assignReturn condition destination (valueIsReturn operation source operand) changed)
                    pure (next restored, restored, 0, False)
          A.MoveWide condition register value -> do
            (_, changed, _, _) <- regular condition [register]
            let scalar = setWord register (if condition == A.Cond 14 then Just value else Nothing) changed
            pure (next scalar, scalar, 0, False)
          A.MoveTop condition register value -> do
            require (M.notMember register aliases) "initializer compiler MOVT transforms a stack alias"
            (_, changed, _, _) <- regular condition [register]
            let result = (\low -> (low .&. 0xffff) .|. (value `shiftL` 16)) <$> readWord register
                scalar = setWord register (if condition == A.Cond 14 then result else Nothing) changed
            pure (next scalar, scalar, 0, False)
          A.ZeroExtend condition register source _ _ -> do
            require (source /= A.pc) "initializer compiler extension reads PC"
            require (M.notMember source aliases) "initializer compiler extension transforms a stack alias"
            regular condition [register]
          A.Multiply condition register left right accumulator -> do
            require (A.pc `notElem` ([left,right] ++ maybe [] pure accumulator)) "initializer compiler multiply reads PC"
            require (all (`M.notMember` aliases) ([left,right] ++ maybe [] pure accumulator)) "initializer compiler multiply transforms a stack alias"
            (_, changed, _, _) <- regular condition [register]
            let productValue = (*) <$> readWord left <*> readWord right
                result = maybe productValue (\acc -> (+) <$> productValue <*> readWord acc) accumulator
                scalar = setWord register (if condition == A.Cond 14 then result else Nothing) changed
            pure (next scalar, scalar, 0, False)
          A.Load condition width signed destination base offset indexing -> do
            let bytes = case width of A.Byte -> 1; A.Half -> 2; A.Word -> 4; A.Dual -> 8
                registers = case (width, destination) of (A.Dual, A.Reg index) -> [destination,A.Reg (index + 1)]; _ -> [destination]
            require (case (width,destination) of
              (A.Dual,A.Reg index) -> even index && index <= 12
              _ -> True) "initializer compiler dual load has an unpredictable register pair"
            require (indexing == A.OffsetOnly || base `notElem` registers) "initializer compiler load writeback overlaps its destination"
            plan <- addressPlan base offset indexing
            changedBase <- writeback condition base plan state
            let allocation = if indexing == A.PreIndexed then maybe sp id (M.lookup A.sp (stackAliases changedBase)) else sp
            location <- frameLocation allocation bytes plan
            let loaded index = case location of
                  Just low | width `elem` [A.Word,A.Dual] -> Set.member (low + 4 * index) slots
                  _ -> False
            if A.pc `elem` registers
              then do
                require (width == A.Word && indexing == A.OffsetOnly) "initializer compiler load return has unadmitted width or writeback"
                returning condition (loaded 0) state
              else do
                (_, forgotten, _, _) <- regular condition registers
                let restored = foldl (\current (register,index) -> assignReturn condition register (loaded index) current)
                      forgotten (zip registers [0..])
                    literal = case plan of
                      AddressPlan value _ _ _ | base == A.pc && width == A.Word && not signed && indexing == A.OffsetOnly ->
                        wordValue value >>= K.imageWord code
                      _ -> Nothing
                    scalar = setWord destination (if condition == A.Cond 14 then literal else Nothing) restored
                final <- writeback condition base plan scalar
                pure (next final, final, maybe 0 negate location, False)
          A.Store condition A.Word source base offset indexing -> do
            require (M.notMember source aliases) "initializer compiler store spills a stack alias"
            require (indexing == A.OffsetOnly || source /= base) "initializer compiler store writeback overlaps its source"
            plan <- addressPlan base offset indexing
            changedBase <- writeback condition base plan state
            let allocation = if indexing == A.PreIndexed then maybe sp id (M.lookup A.sp (stackAliases changedBase)) else sp
            target <- storeTarget allocation plan
            let storeState = case target of
                  PublicationStore -> state
                  FrameStore low ->
                    let cleared = clearSlots low 4 slots
                        updatedSlots = if condition == A.Cond 14 && Set.member source returnRegisters && low `mod` 4 == 0
                          then Set.insert low cleared else cleared
                     in state {savedReturns = updatedSlots}
                usedFrame = case target of FrameStore low -> negate low; PublicationStore -> 0
            final <- writeback condition base plan storeState
            pure (next final, final, usedFrame, False)
          A.Branch condition False target ->
            let successors = case K.holds flags condition of
                  Just True -> [(target,state)]
                  Just False -> next state
                  Nothing -> (target,state) : next state
             in pure (successors, state, 0, False)
          A.BranchExchange condition False register -> returning condition (Set.member register returnRegisters) state
          A.Hint _ -> pure (next state, state, 0, False)
          _ -> Left "initializer Rust code has an unadmitted instruction, call or stack effect"
        let usedSp = maybe 0 negate (M.lookup A.sp (stackAliases updated))
        walk (successors ++ pending) (M.insert context state visited) aliasVisits (steps + 1)
          (maximum [deepest, used, usedSp]) (returns + if returned then 1 else 0)
  require (F.symbolSize symbol > 0 && end <= toInteger origin + toInteger (B.length image)) "initializer Rust symbol leaves the image"
  walk [(start, initial)] M.empty M.empty 0 0 0

parseOriginalSource :: B.ByteString -> Either String S.OriginalStartupDat
parseOriginalSource bytes = do
  expected <- F.parseSha256 stockFirmwareSha256
  observed <- F.parseSha256 (sha256Hex bytes)
  require (observed == expected) "initializer requires the canonical original X-M1 1.50 DAT identity"
  S.parseOriginalStartupDat bytes

deriveContract :: Role -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String Contract
deriveContract role reference staged image symbolBytes = do
  original <- parseOriginalSource reference
  parsed <- U.parseContainer staged
  let source = S.originalStartupParsed original
  images <- P.stagedImages staged
  let loader = P.stagedLoader images
  require (sha256Hex (B.take 0x57c (B.drop 0x430 loader)) == "60aeb6c0c68717940fef3771f3387540a2c87276d7c947a9f3d276dd51fe850d")
    "retained initializer body differs from the pinned DAT bytes"
  caller <- callerFramePremise loader
  stores <- originalStores loader
  (facts, entry, rustStart, rustEnd, scratch) <- case role of
    Retained -> do
      require (B.null image && B.null symbolBytes) "retained initializer refuses owned artifact inputs"
      require (reference == staged) "retained initializer requires the exact original DAT"
      facts <- P.retainedFacts images
      pure (facts, 0xd0430, 0xd0430, 0xd09ac, 12)
    Owned -> do
      symbols <- F.parseSymbols symbolBytes
      declarations' <- declarations symbols
      start <- at symbols "__secondary_stage_start"
      entry <- at symbols "xm1_secondary_entry" >>= F.armEntry
      offset <- F.slotOffset (fromIntegral start - 0x10000)
      plan <- F.stagePlan F.SecondRecord entry offset (U.MediaImage image) (declaredRetained declarations')
      F.verifyStagedRecord plan (declaredLoaderPatches declarations') source parsed
      facts <- P.deriveOwnedFacts image symbolBytes images
      adapter <- at symbols "xm1_interface_initializer_entry"
      rust <- F.lookupSymbol symbols "xm1_interface_initializer_publish"
      adapterStack <- adapterExtent image symbols
      compilerStack <- stackExtent caller (StackBytes adapterStack) stores image symbols
      pure (facts, adapter, F.symbolAddress rust, F.symbolAddress rust + F.symbolSize rust, compilerStack + adapterStack)
  imageDigest <- F.parseSha256 (sha256Hex (if role == Retained then B.take 0x57c (B.drop 0x430 loader) else image))
  symbolsDigest <- if role == Retained then pure Nothing else Just <$> F.parseSha256 (sha256Hex symbolBytes)
  pure (Contract role (P.Contract P.Natural (P.Program (if role == Owned then P.OwnedProgram else P.RetainedProgram) facts))
    (P.Guest entry) (P.Span (fromIntegral rustStart) (fromIntegral rustEnd)) (StackBytes scratch)
    imageDigest symbolsDigest (S.originalStartupDigest original) caller stores)

contractValue :: Contract -> Value
contractValue (Contract role prefix (P.Guest entry) (P.Span rustStart rustEnd) (StackBytes scratch) imageDigest symbolsDigest source _ _) = object
  [ "schema" .= (6 :: Int), "prefix" .= P.contractValue prefix
  , "initializer" .= object
      [ "role" .= (if role == Owned then "owned" else "retained" :: String)
      , "entry" .= entry, "rust" .= [rustStart, rustEnd], "scratch_bytes" .= scratch
      , "image_sha256" .= F.sha256Hex imageDigest, "symbols_sha256" .= fmap F.sha256Hex symbolsDigest
      , "source_sha256" .= F.sha256Hex source
      ]
  ]

originalStores :: B.ByteString -> Either String [ExpectedStore]
originalStores loader = do
  let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
      address = P.Guest . (0x400ddaa0 +)
      wide low high = (.|.) <$> (immediate <$> leWordAt loader low) <*> ((`shiftL` 16) . immediate <$> leWordAt loader high)
  services <- forM [0 .. 93] $ \index -> do
    let offset = 0x440 + 12 * index
    low <- leWordAt loader offset
    high <- leWordAt loader (offset + 4)
    store <- leWordAt loader (offset + 8)
    require (low .&. 0xfff0f000 == 0xe3002000 && high .&. 0xfff0f000 == 0xe3402000
      && store == 0xe5832000 + fromIntegral index * 4) "original initializer service encoding differs"
    pure (ExpectedStore (address (fromIntegral index * 4)) (immediate low .|. (immediate high `shiftL` 16)))
  extensionValues <- mapM (uncurry wide) [(0x8a8,0x8b0),(0x8b8,0x8c0),(0x8c8,0x8d0),(0x8d8,0x8e0),(0x8e8,0x8f0)]
  pool <- wide 0x8fc 0x900
  extended <- wide 0x904 0x908
  let extensions = zipWith (\index value -> ExpectedStore (address (0x1618 + index * 4)) value) [0..] extensionValues
      descriptor index =
        let offset = 0x178 + index * 44
         in ExpectedStore (address offset) (index + 1)
              : [ExpectedStore (address (offset + 4 + field * 4)) (pool + index * 40 + field * 4) | field <- [0..7]]
              ++ [ ExpectedStore (address (offset + 36)) (extended + index * 56 + 24)
                 , ExpectedStore (address (offset + 40)) (extended + index * 56 + 28)
                 ]
  pure (services ++ extensions ++ concatMap descriptor [0..119])

instance FromJSON Role where
  parseJSON = withText "initializer role" $ \value -> case value of
    "retained" -> pure Retained
    "owned" -> pure Owned
    _ -> fail "unknown initializer role"

data Context = Context Word32 [Word32] Word32 Word32 Word32 [Word64] [[Word32]] deriving (Eq)
instance FromJSON Context where
  parseJSON = withObject "initializer context" $ \row ->
    Context <$> row .: "pc" <*> row .: "registers" <*> row .: "sp" <*> row .: "lr" <*> row .: "cpsr" <*> row .: "counts" <*> row .: "tcm"

data Views = Views [Word8] [Word8] deriving (Eq)
instance FromJSON Views where
  parseJSON = withObject "initializer views" $ \row -> Views <$> row .: "virtual" <*> row .: "physical"

data Identity = Identity Role Word32 [Word32] Word32 F.Sha256 F.Sha256 (Maybe F.Sha256) F.Sha256
instance FromJSON Identity where
  parseJSON = withObject "initializer identity" $ \row ->
    let digest name = row .: name >>= either fail pure . F.parseSha256
        optionalDigest name = row .: name >>= traverse (either fail pure . F.parseSha256)
     in Identity <$> row .: "role" <*> row .: "entry" <*> row .: "rust" <*> row .: "scratch_bytes"
          <*> digest "source_sha256" <*> digest "image_sha256" <*> optionalDigest "symbols_sha256" <*> digest "body_sha256"

data Publication = Publication Word32 Word64 Word32 Word32 Word64
instance FromJSON Publication where
  parseJSON = withObject "initializer publication" $ \row ->
    Publication <$> row .: "guest" <*> row .: "physical" <*> row .: "value" <*> row .: "pc" <*> row .: "instructions"

data Source = CpuWrite | CpuRead | CpuFetch | HostWrite | HostRead | ModelBusWrite | ModelBusRead deriving (Eq, Ord)
instance FromJSON Source where
  parseJSON = withText "initializer event source" $ \value -> case value of
    "cpu-write" -> pure CpuWrite
    "cpu-read" -> pure CpuRead
    "cpu-fetch" -> pure CpuFetch
    "host-write" -> pure HostWrite
    "host-read" -> pure HostRead
    "model-bus-write" -> pure ModelBusWrite
    "model-bus-read" -> pure ModelBusRead
    _ -> fail "unknown initializer event source"

data ByteOrder = LittleEndian | BigEndian deriving (Eq)
instance FromJSON ByteOrder where
  parseJSON = withText "initializer byte order" $ \value -> case value of
    "little" -> pure LittleEndian
    "big" -> pure BigEndian
    _ -> fail "unknown initializer byte order"

data Event = Event Source (Maybe Int) Word64 (Maybe Word64) Word64 (Maybe Word64) (Maybe Word32) (Maybe Word64) (Maybe ByteOrder)
instance FromJSON Event where
  parseJSON = withObject "initializer event" $ \row ->
    Event <$> row .: "source" <*> row .: "core" <*> row .: "address" <*> row .: "physical" <*> row .: "size"
      <*> row .: "value" <*> row .: "pc" <*> row .: "instructions" <*> row .: "byte_order"

data Mode = Allow | Forbid deriving (Eq)
instance FromJSON Mode where
  parseJSON = withText "initializer confinement mode" $ \value -> case value of
    "allow" -> pure Allow
    "forbid" -> pure Forbid
    _ -> fail "unknown initializer confinement mode"

newtype PhysicalSpan = PhysicalSpan [Word64] deriving (Eq, Ord)
instance FromJSON PhysicalSpan where
  parseJSON = withObject "initializer physical span" $ \row -> PhysicalSpan <$> row .: "physical"

newtype Window = Window (Word64, Word64) deriving (Eq, Ord)
instance FromJSON Window where
  parseJSON value = Window <$> parseJSON value

data BankSnapshot = BankSnapshot Word32 (Maybe Window) deriving (Eq, Ord)
instance FromJSON BankSnapshot where
  parseJSON = withObject "initializer TCM bank snapshot" $ \row ->
    BankSnapshot <$> row .: "register" <*> row .: "window"

data TcmSnapshot = TcmSnapshot Int BankSnapshot BankSnapshot deriving (Eq, Ord)
instance FromJSON TcmSnapshot where
  parseJSON = withObject "initializer TCM snapshot" $ \row ->
    TcmSnapshot <$> row .: "core" <*> row .: "atcm" <*> row .: "btcm"

bankSnapshot :: Word32 -> Either String BankSnapshot
bankSnapshot register
  | register .&. 1 == 0 = Right (BankSnapshot register Nothing)
  | otherwise = do
      let size = (512 :: Word64) `shiftL` fromIntegral ((register `shiftR` 2) .&. 31)
          base = fromIntegral (register .&. 0xfffff000)
      require (size >= 0x1000 && size <= 0x800000 && base `mod` size == 0)
        "initializer TCM bank size or alignment is unsupported"
      pure (BankSnapshot register (Just (Window (base,base + size))))

data WatchClass = WatchClass Window [Source] deriving (Eq, Ord)
instance FromJSON WatchClass where
  parseJSON = withObject "initializer watch class" $ \row -> WatchClass <$> row .: "range" <*> row .: "kinds"

data FixedViews = FixedViews
instance FromJSON FixedViews where
  parseJSON = withText "initializer watch views" $ \value ->
    if value == "fixed" then pure FixedViews else fail "initializer watch views are not fixed"

data NoFault = NoFault
instance FromJSON NoFault where
  parseJSON J.Null = pure NoFault
  parseJSON _ = fail "initializer watch recorded a fault"

data Rule = Rule [Int] Source Mode [PhysicalSpan] [TcmSnapshot] deriving (Eq)
instance FromJSON Rule where
  parseJSON = withObject "initializer confinement rule" $ \row ->
    Rule <$> row .: "cores" <*> row .: "kind" <*> row .: "mode" <*> row .: "spans" <*> row .: "tcm"

data Watch = Watch Bool (Maybe String) [Event] [Rule] [WatchClass] FixedViews NoFault [TcmSnapshot]
instance FromJSON Watch where
  parseJSON = withObject "initializer scoped watch" $ \row ->
    Watch <$> row .: "enabled" <*> row .: "violation" <*> row .: "records" <*> row .: "confinement"
      <*> row .: "classes" <*> row .: "views" <*> row .: "fault" <*> row .: "tcm"

data Alias = Alias Int Word64 Word64 Word64 F.Sha256
instance FromJSON Alias where
  parseJSON = withObject "initializer alias binding" $ \row -> do
    digest <- row .: "sha256" >>= either fail pure . F.parseSha256
    Alias <$> row .: "core" <*> row .: "guest" <*> row .: "physical" <*> row .: "size" <*> pure digest

data Decoder = Decoder [Int] [Maybe Int]
instance FromJSON Decoder where
  parseJSON = withObject "initializer decoder" $ \row -> Decoder <$> row .: "choices" <*> row .: "pending"

data Observation = Observation Identity Context Context Context Views Views Views Views Views Views Views Views Decoder [Alias] [Publication] Watch
instance FromJSON Observation where
  parseJSON = withObject "initializer observation" $ \row ->
    Observation <$> row .: "contract" <*> row .: "before" <*> row .: "entered" <*> row .: "after"
      <*> row .: "frame_before" <*> row .: "frame_after" <*> row .: "table_before" <*> row .: "table_after"
      <*> row .: "tail_before" <*> row .: "tail_after" <*> row .: "pools_before" <*> row .: "pools_after"
      <*> row .: "decoder" <*> row .: "aliases" <*> row .: "publications" <*> row .: "watch"

data Result = Passed | Failed deriving (Eq)
instance FromJSON Result where
  parseJSON = withText "initializer result" $ \value -> case value of
    "passed" -> pure Passed
    "failed" -> pure Failed
    _ -> fail "unknown initializer result"

data Receipt = Receipt Word32 Result (Maybe String) (Maybe String) [Word32] Observation
instance FromJSON Receipt where
  parseJSON = withObject "initializer receipt" $ \row ->
    Receipt <$> row .: "schema" <*> row .: "result" <*> row .: "error" <*> row .: "machine_failure"
      <*> row .: "cp14_identities" <*> row .: "initializer"

validateReceipt :: Contract -> B.ByteString -> Either String ()
validateReceipt (Contract role prefix (P.Guest entry) (P.Span rustStart rustEnd) (StackBytes scratch) imageDigest symbolsDigest source caller stores) bytes = do
  require (B.length bytes <= 16 * 1024 * 1024) "initializer receipt exceeds 16 MiB"
  verifiedPrefix <- P.parseReceipt P.InitializerBoundary bytes >>= P.validateReceipt prefix
  P.PrefixExitContext prefixRegisters prefixSp prefixLr (Cpsr prefixCpsr) (prefixCount0,prefixCount1)
    ((prefixAtcm0,prefixBtcm0),(prefixAtcm1,prefixBtcm1)) (prefixVirtual,prefixPhysical) <- P.prefixExitContext verifiedPrefix
  document <- decodeStrictIntegers bytes
  Receipt schema result failure machineFailure identities observation <- case J.fromJSON document of
    J.Error message -> Left message
    J.Success receipt -> Right receipt
  require (schema == 6 && result == Passed && failure == Nothing && machineFailure == Nothing && identities == [0x4003,0x5003])
    "initializer SDK receipt did not pass its distinct schema-6 role"
  let Observation (Identity observedRole observedEntry rust observedScratch observedSource observedImage observedSymbols body)
        before entered after frameBefore frameAfter tableBefore tableAfter tailBefore tailAfter poolsBefore poolsAfter
        (Decoder choices pending) aliases publications (Watch enabled violation events rules classes FixedViews NoFault snapshots) = observation
      Context incomingPc incoming sp _ cpsr counts tcm = before
      Context enteringPc entering enteringSp enteringLr enteringCpsr enteringCounts enteringTcm = entered
      Context outgoingPc outgoing outgoingSp outgoingLr outgoingCpsr outgoingCounts outgoingTcm = after
      P.Guest expectedSp = callerEntrySp caller
      P.Guest expectedFrame = callerFramePointer caller
      P.Guest expectedTop = callerStackTop caller
      views (Views virtual physical) = do
        require (virtual == physical) "initializer committed virtual/physical readback differs"
        pure (B.pack physical)
  stackPhysical <- maybe (Left "initializer entry stack lacks its physical alias") (Right . fromInteger) (physicalAlias expectedSp)
  require (before == Context 0xd0aac (W.registerWords prefixRegisters) prefixSp prefixLr prefixCpsr [prefixCount0,prefixCount1]
    [[prefixAtcm0,prefixBtcm0],[prefixAtcm1,prefixBtcm1]]
    && frameBefore == Views (B.unpack prefixVirtual) (B.unpack prefixPhysical))
    "initializer does not continue the independently validated prefix endpoint"
  expectedBody <- F.parseSha256 "60aeb6c0c68717940fef3771f3387540a2c87276d7c947a9f3d276dd51fe850d"
  require (observedRole == role && observedEntry == entry && rust == map fromIntegral [rustStart,rustEnd]
    && observedScratch == scratch && observedSource == source && observedImage == imageDigest && observedSymbols == symbolsDigest && body == expectedBody)
    "initializer receipt differs from its source-bound contract"
  require (length incoming == 13 && length outgoing == 13 && incomingPc == 0xd0aac && sp == expectedSp && incoming !! 11 == expectedFrame
    && incoming !! 4 == 0x69cdf8 && cpsr .&. 0x010002ff == 0xd3) "initializer incoming frame or mode differs"
  require (enteringPc == entry && entering == incoming && enteringSp == sp && enteringLr == 0xd0ab0 && enteringCpsr == cpsr && enteringTcm == tcm)
    "initializer BL changed registers, SP, CPSR or TCM"
  case (counts, enteringCounts, outgoingCounts) of
    ([c0,c1],[e0,e1],[a0,a1]) -> require (e0 >= c0 && e1 == c1 + 1 && a0 >= e0 && a1 > e1) "initializer core counts differ"
    _ -> Left "initializer core count width differs"
  require (outgoingPc == 0xd0ab0 && outgoingSp == sp && outgoingLr == 0xd0ab0 && drop 4 (take 12 outgoing) == drop 4 (take 12 incoming)
    && take 4 outgoing == [0x400ebe6c,0x400ed120,0x400f18f0,0x400def40] && outgoing !! 12 == 120
    && outgoingCpsr == ((cpsr .&. 0x0fffffff) .|. 0x60000000) && outgoingTcm == tcm)
    "initializer exact return state differs"
  require (length tcm == 2 && all ((== 2) . length) tcm) "initializer TCM context is absent"
  frame <- views frameBefore
  _ <- views frameAfter
  require (frameBefore == frameAfter && B.length frame == 16 && leWordAt frame 0 == Right expectedTop
    && leWordAt frame 4 == Right 0x69cdf8 && leWordAt frame 12 == Right 0xd00c0) "initializer caller frame changed"
  prestate <- views tableBefore
  committed <- views tableAfter
  require (B.length prestate == 0x162c && B.length committed == 0x162c) "initializer committed table length differs"
  forM_ [tailBefore,tailAfter,poolsBefore,poolsAfter] $ \value -> views value >> pure ()
  require (tailBefore == tailAfter && poolsBefore == poolsAfter) "initializer guard or retained pool changed"
  require (choices == [1] && pending == [Nothing] && enabled && violation == Nothing) "initializer continuous guard or decoder differs"
  expectedTcm <- sequence
    [ TcmSnapshot 0 <$> bankSnapshot prefixAtcm0 <*> bankSnapshot prefixBtcm0
    , TcmSnapshot 1 <$> bankSnapshot prefixAtcm1 <*> bankSnapshot prefixBtcm1
    ]
  require (sort snapshots == sort expectedTcm) "initializer watch TCM scope differs from the validated prefix context"
  let linearWindows = [Window (alias + low,alias + high) | alias <- [0,0x40000000], (low,high) <- [(0x10000,0x14c80),(0xd0000,0xddaa0),(0xddaa0,0xf3420)]]
      disjoint (Window (low,high)) (Window (otherLow,otherHigh)) = high <= otherLow || otherHigh <= low
  if role == Owned
    then forM_ expectedTcm $ \(TcmSnapshot _ atcm btcm) ->
      forM_ [atcm,btcm] $ \(BankSnapshot _ window) ->
        forM_ window $ \active -> require (all (disjoint active) linearWindows)
          "initializer TCM remaps a code, publication or stack alias"
    else pure ()
  let physical low high = PhysicalSpan [low,high]
      writes = [CpuWrite,HostWrite,ModelBusWrite]
      expectedClasses =
        [ WatchClass (Window (0xddaa0,0xdf0cc)) writes
        , WatchClass (Window (0xddaa0,0xf3420)) [HostWrite,ModelBusWrite]
        , WatchClass (Window (0x10000,0x14c80)) writes
        , WatchClass (Window (0xd0000,0xddaa0)) writes
        ]
      normalizeClass (WatchClass range kinds) = WatchClass range (sort kinds)
      rule cores kind mode spans =
        Rule cores kind mode spans [snapshot | snapshot@(TcmSnapshot core _ _) <- expectedTcm, core `elem` cores]
      readSpans = [physical 0xddaa0 0xdf0cc] ++ if role == Owned then [physical 0xd0430 0xd09ac,physical 0xd00c8 0xd00fc] else []
      expectedRules =
        [ rule [1] CpuWrite Allow [physical 0xddaa0 0xdf0cc, physical (stackPhysical - fromIntegral scratch) stackPhysical]
        , rule [0] CpuWrite Forbid [physical 0xddaa0 0xf3420,physical 0x10000 0x14c80,physical 0xd0000 0xddaa0]
        ]
          ++ [rule [1] CpuFetch Forbid (map (uncurry physical) [(0xd0430,0xd09ac),(0xd0020,0xd00c8),(0xd09ac,0xd09d0),(0xd0a44,0xd0a64)]) | role == Owned]
          ++ [rule [1] CpuRead Forbid readSpans]
      normalize (Rule cores kind mode spans describedTcm) = Rule cores kind mode (sort spans) (sort describedTcm)
  require (sort (map normalizeClass classes) == sort (map normalizeClass expectedClasses))
    "initializer watch source classes differ from the publication and retained-resource scope"
  require (map normalize rules == map normalize expectedRules) "initializer continuous confinement roles differ"
  require (length events == 1419 && length publications == 1419 && length stores == 1419) "initializer publication coverage differs"
  let lower = sum enteringCounts
      upper = sum outgoingCounts
  forM_ (zip3 stores publications events) $ \(ExpectedStore (P.Guest guest) value, Publication observedGuest observedPhysical observedValue pc instructions,
      Event kind core address eventPhysical width eventValue eventPc eventInstructions order) -> do
    let physicalAddress = fromIntegral guest - 0x40000000
    require (observedGuest == guest && observedPhysical == physicalAddress && observedValue == value
      && fromIntegral pc >= rustStart && fromIntegral pc < rustEnd && instructions >= lower && instructions < upper
      && kind == CpuWrite && core == Just 1 && address == fromIntegral guest && eventPhysical == Just physicalAddress
      && width == 4 && eventValue == Just (fromIntegral value) && eventPc == Just pc && eventInstructions == Just instructions && order == Just LittleEndian)
      "initializer store order, attribution or value differs"
    require (leWordAt committed (fromIntegral guest - 0x400ddaa0) == Right value) "initializer committed word differs"
  let clocks = [clock | Publication _ _ _ _ clock <- publications]
  require (and (zipWith (<=) clocks (drop 1 clocks))) "initializer publication clocks decreased"
  let keys = [(core,guest,physicalAddress,size) | Alias core guest physicalAddress size _ <- aliases]
      expectedKeys = concat (replicate 2 [(core,alias + low,low,high-low) | core <- [0,1], (low,high) <- [(0x10000,0x14c80),(0xd0000,0xddaa0),(0xddaa0,0xf3420)], alias <- [0,0x40000000]])
  require (sort keys == sort expectedKeys) "initializer full-image/workspace alias bindings differ"
  let prefixFacts = case prefix of
        P.Contract _ (P.Program _ facts) -> facts
        P.ComposedContract _ facts _ -> facts
      digestFor resource bindings = [digest | Alias _ _ physicalAddress _ digest <- bindings, physicalAddress == resource]
      consistent [] = False
      consistent (first:rest) = all (== first) rest
  require (all (== P.factLoader prefixFacts) (digestFor 0xd0000 aliases)
    && consistent (digestFor 0x10000 aliases) && consistent (digestFor 0xddaa0 (take 12 aliases))
    && consistent (digestFor 0xddaa0 (drop 12 aliases))) "initializer full-image or workspace alias identity differs"
