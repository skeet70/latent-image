{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | A census of a stock image set: one graph over the declared function starts
-- with three edge kinds, roots from every task-creation path, and the reach of
-- each root.
--
-- The inputs are the disassembly listings of the images (the firmware) and a
-- table of function starts. Without both the census reports @not-run@; it never
-- reports an empty reach as a result.
--
-- Edge kinds:
--
-- * 'Direct': a @b@, @bl@ or @blx@ whose target is a function start.
-- * 'Table': a word in a data run that equals a function start, attributed to
--   the functions that build the table's base (the indexers). A table whose
--   words are themselves table starts is followed into those tables.
-- * 'CodeWord': an address a function builds with a literal load or a
--   @movw@/@movt@ pair and then calls, stores or passes.
--
-- The listing supplies every word of each image, so a table word is read from
-- the image and an extent digest is computed over the same bytes a runner
-- reports.
module Emulation.Census
  ( -- * Inputs
    Listing (..)
  , parseListing
  , Function (..)
  , parseFunctions
  , assignImages
    -- * Graph
  , EdgeKind (..)
  , edgeKindLabel
  , EdgeSelection (..)
  , allEdgeKinds
  , Edge (..)
  , RootKind (..)
  , rootKindLabel
  , Root (..)
  , Table (..)
  , Census (..)
  , buildCensus
    -- * Reach
  , reachFrom
  , reachByFunction
  , reachableFunctions
    -- * Output
  , censusJson
  , censusTable
  , extentsJson
  , extentDigest
  , hexAddress
  ) where

import Data.Aeson (object, (.=))
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits (rotateR, shiftL, shiftR, testBit, (.&.), (.|.))
import Data.Int (Int32)
import qualified Data.IntMap.Strict as IM
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isNothing, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)

-- * Inputs

-- | One disassembly listing. @listingWords@ is every word the listing prints,
-- keyed by its address; data words are printed as instructions and are kept,
-- because a table lives in them.
data Listing = Listing
  { listingName :: String
  , listingBase :: Word32
  , listingEnd :: Word32
  , listingWords :: M.Map Word32 Word32
  } deriving (Eq, Show)

-- | Parse an @objdump@-style listing: @address: word mnemonic operands@.
-- Label lines, section headers and continuation lines carry no word and are
-- skipped. A @...@ continuation elides a run of zero words, which the listing
-- does not print; those addresses are materialized as zeros, because a table's
-- leading words are what make its start.
parseListing :: String -> B.ByteString -> Either String Listing
parseListing name bytes
  | M.null wordsByAddress = Left (name ++ ": listing holds no instruction word")
  | otherwise = Right Listing
      { listingName = name
      , listingBase = fst (M.findMin wordsByAddress)
      , listingEnd = fst (M.findMax wordsByAddress) + 4
      , listingWords = wordsByAddress
      }
  where
    (wordsByAddress, _, _) = foldl' step (M.empty, Nothing, False) (BC.lines bytes)
    step (accumulated, lastAddress, pending) line = case row line of
      Just (address, word) ->
        let elided = case lastAddress of
              Just previous | pending && address > previous + 4 -> [(zero, 0) | zero <- [previous + 4, previous + 8 .. address - 4]]
              _ -> []
         in (foldl' (\table (zero, value) -> M.insert zero value table) (M.insert address word accumulated) elided, Just address, False)
      Nothing -> (accumulated, lastAddress, pending || BC.strip line == "...")
    row line = do
      let trimmed = BC.dropWhile (== ' ') line
      (addressText, rest) <- splitColon trimmed
      address <- hexWord addressText
      word <- hexWord (BC.take 8 (BC.dropWhile (== ' ') rest))
      pure (address, word)
    splitColon text = case BC.elemIndex ':' text of
      Nothing -> Nothing
      Just index -> Just (BC.take index text, BC.drop (index + 1) text)

hexWord :: BC.ByteString -> Maybe Word32
hexWord text
  | BC.null text || BC.length text > 8 = Nothing
  | not (BC.all isHexDigit text) = Nothing
  | otherwise = case readHex (BC.unpack text) of
      [(value, "")] -> Just (fromIntegral (value :: Integer))
      _ -> Nothing
  where
    isHexDigit c = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')

-- | One declared function extent.
data Function = Function
  { functionEntry :: Word32
  , functionSize :: Word32
  , functionName :: String
  , functionImage :: String
  } deriving (Eq, Show)

-- | Parse a function table: the decomp index @range@ rows
-- (@range image section address offset size digest name@) or a plain
-- @address<TAB>size<TAB>name@ table. Addresses may be @0x@-prefixed or decimal;
-- sizes are bytes.
parseFunctions :: B.ByteString -> Either String [Function]
parseFunctions bytes = do
  rows <- mapM parseRow (filter (not . BC.null) (map (BC.dropWhile (== ' ')) (BC.lines bytes)))
  pure (sortOn functionEntry (M.elems (M.fromList [(functionEntry row, row) | row <- rows, functionSize row > 0])))
  where
    parseRow line = do
      let fields = BC.split '\t' line
      case fields of
        ("range" : _image : _section : address : _offset : size : _digest : name : _) ->
          Function <$> addressField address <*> sizeField size <*> pure (BC.unpack name) <*> pure ""
        (address : size : rest) ->
          Function <$> addressField address <*> sizeField size <*> pure (BC.unpack (BC.intercalate "\t" rest)) <*> pure ""
        _ -> Left ("function row has neither range nor address/size columns: " ++ BC.unpack line)
    addressField text = maybe (Left ("function address is not hexadecimal: " ++ BC.unpack text)) Right (hexWord (stripPrefix text))
    sizeField text = case reads (BC.unpack (stripPrefix text)) of
      [(value, "")] | value >= 0 && value <= toInteger (maxBound :: Word32) -> Right (fromIntegral (value :: Integer))
      _ -> Left ("function size is not a byte count: " ++ BC.unpack text)
    stripPrefix text = fromMaybe text (BC.stripPrefix "0x" text)

-- | Attach each function to the listing whose address window holds it. A
-- function that no listing holds, or that crosses two listings, is refused.
assignImages :: [Listing] -> [Function] -> Either String [Function]
assignImages listings = mapM assign
  where
    assign function = case [listing | listing <- listings, listingBase listing <= entry, entry + functionSize function <= listingEnd listing] of
      [listing] -> Right function { functionImage = listingName listing }
      [] -> Left ("function " ++ hexAddress entry ++ " " ++ functionName function ++ " lies in no declared listing")
      many -> Left ("function " ++ hexAddress entry ++ " " ++ functionName function ++ " spans " ++ show (length many) ++ " listings")
      where entry = functionEntry function

-- * Instruction decode

data Alu = Add | Sub | Rsb | And | Orr | Eor | Mov | Mvn | OtherAlu
  deriving (Eq, Show)

data Operand2 = Immediate Word32 | Shifted Int Int Int
  -- ^ register, shift kind (0 lsl, 1 lsr, 2 asr, 3 ror), amount
  deriving (Eq, Show)

data Op
  = Branch Int Bool Word32
  | BranchExchange Bool Int
  | MoveWide Bool Int Word32
  | DataOperation Alu Int Int Operand2
  | Load Int Int (Maybe Int32) (Maybe Operand2) Bool Bool
  | Store Int Int (Maybe Int32) (Maybe Operand2) Bool
  | Compare Int Operand2
  | Other
  deriving (Eq, Show)

-- | Decode the instruction forms this census reads. Everything else is
-- 'Other'; the executor ignores it.
decodeWord :: Word32 -> Word32 -> Op
decodeWord address word
  | condition == 0xf = Other
  | (word .&. 0x0ffffff0) == 0x012fff10 = BranchExchange False (fromIntegral (word .&. 0xf))
  | (word .&. 0x0ffffff0) == 0x012fff30 = BranchExchange True (fromIntegral (word .&. 0xf))
  | (word `shiftR` 24) .&. 0xf == 3
  , wide <- (word `shiftR` 20) .&. 0xf
  , wide == 0 || wide == 4 =
      MoveWide (wide == 4) (fromIntegral ((word `shiftR` 12) .&. 0xf))
        ((((word `shiftR` 16) .&. 0xf) `shiftL` 12) .|. (word .&. 0xfff))
  | (word `shiftR` 25) .&. 0x7 == 0x5 =
      Branch (fromIntegral (word `shiftR` 28)) (testBit word 24) (address + 8 + (signed 24 (word .&. 0xffffff) `shiftL` 2))
  | (word `shiftR` 26) .&. 0x3 == 0x1 =
      let register = fromIntegral ((word `shiftR` 12) .&. 0xf)
          base = fromIntegral ((word `shiftR` 16) .&. 0xf)
          load = testBit word 20
          pre = testBit word 24
          up = testBit word 23
          writeback = not pre || testBit word 21
          registerOffset = testBit word 25
          operand = Shifted (fromIntegral (word .&. 0xf)) (fromIntegral ((word `shiftR` 5) .&. 0x3)) (fromIntegral ((word `shiftR` 7) .&. 0x1f))
          magnitude = fromIntegral (word .&. 0xfff) :: Int32
          offset = if up then magnitude else negate magnitude
       in if load
            then Load register base (if registerOffset then Nothing else Just offset) (if registerOffset then Just operand else Nothing) (base == 15) writeback
            else Store register base (if registerOffset then Nothing else Just offset) (if registerOffset then Just operand else Nothing) writeback
  | (word `shiftR` 26) .&. 0x3 == 0x0 =
      let immediate = testBit word 25
          opcode = (word `shiftR` 21) .&. 0xf
          register = fromIntegral ((word `shiftR` 12) .&. 0xf)
          base = fromIntegral ((word `shiftR` 16) .&. 0xf)
          operand = if immediate
            then Immediate (rotateImmediate (word .&. 0xff) ((word `shiftR` 8) .&. 0xf))
            else Shifted (fromIntegral (word .&. 0xf)) (fromIntegral ((word `shiftR` 5) .&. 0x3)) (fromIntegral ((word `shiftR` 7) .&. 0x1f))
       in case opcode of
            8 -> Compare base operand
            9 -> Compare base operand
            10 -> Compare base operand
            13 -> DataOperation Mov register 0 operand
            15 -> DataOperation Mvn register 0 operand
            0 -> DataOperation And register base operand
            1 -> DataOperation Eor register base operand
            2 -> DataOperation Sub register base operand
            3 -> DataOperation Rsb register base operand
            4 -> DataOperation Add register base operand
            12 -> DataOperation Orr register base operand
            _ -> Other
  | otherwise = Other
  where
    condition = word `shiftR` 28

rotateImmediate :: Word32 -> Word32 -> Word32
rotateImmediate value amount = rotateR (value .&. 0xff) (fromIntegral (2 * amount))

signed :: Int -> Word32 -> Word32
signed bits value = fromIntegral ((fromIntegral (value `shiftL` (32 - bits)) :: Int32) `shiftR` (32 - bits))

-- * Function facts

data BuiltUse = CalledIndirect | StoredWord | PassedArgument | LoadedOnly
  deriving (Eq, Ord, Show)

edgeUseLabel :: BuiltUse -> String
edgeUseLabel CalledIndirect = "called"
edgeUseLabel StoredWord = "stored"
edgeUseLabel PassedArgument = "passed"
edgeUseLabel LoadedOnly = "loaded"

data CallSite = CallSite
  { callSiteAddress :: Word32
  , callSiteTarget :: Word32
  , callSiteArguments :: [Word32]
  } deriving (Eq, Show)

data TaskSite = TaskSite
  { taskSiteAddress :: Word32
  , taskSiteId :: Maybe Word32
  , taskSiteEntry :: Maybe Word32
  , taskSiteName :: Maybe Word32
  , taskSitePriority :: Maybe Word32
  } deriving (Eq, Show)

data InterruptSite = InterruptSite
  { interruptSiteAddress :: Word32
  , interruptSiteService :: Word32
  , interruptSiteHandler :: Word32
  } deriving (Eq, Show)

data FunctionFacts = FunctionFacts
  { factsDirect :: [Word32]
  , factsBuilt :: [(Word32, BuiltUse)]
  , factsCalls :: [CallSite]
  , factsTasks :: [TaskSite]
  , factsInterrupts :: [InterruptSite]
  , factsIndexed :: [Word32]
  -- ^ Base addresses of tables the function reads or writes by a scaled index.
  } deriving (Eq, Show)

emptyFacts :: FunctionFacts
emptyFacts = FunctionFacts [] [] [] [] [] []

-- * Values and cells

data Anchor = FramePointer | StackPointer deriving (Eq, Ord, Show)

data Value = Constant Word32 | Frame Anchor Int32 | Element Word32 | Unknown deriving (Eq, Ord, Show)
-- ^ 'Element' is the word selected from the table at the given base by an index
-- the walk does not know.

data Cell = CellAbsolute Word32 | CellFrame Anchor Int32 deriving (Eq, Ord, Show)

data State = State
  { stateRegisters :: IM.IntMap Value
  , stateMemory :: M.Map Cell Value
  , stateMoveWide :: IM.IntMap Word32
  , stateSnapshots :: IM.IntMap (IM.IntMap Value, M.Map Cell Value)
  , stateFacts :: FunctionFacts
  }

initialState :: State
initialState = State
  { stateRegisters = IM.fromList [(13, Frame StackPointer 0), (11, Frame FramePointer 0)]
  , stateMemory = M.empty
  , stateMoveWide = IM.empty
  , stateSnapshots = IM.empty
  , stateFacts = emptyFacts
  }

-- | The guest alias of a word in the decoded images.
physicalAddress :: Word32 -> Word32
physicalAddress address
  | address >= 0x40000000 && address < 0x80000000 = address .&. 0x3fffffff
  | otherwise = address

-- | How many loop iterations one replay walks for facts. A loop over a record
-- array is short; a copy loop's later iterations repeat the same cells.
replayLimit :: Int
replayLimit = 32

taskCreateWrapper, interruptAttach, timerStart :: Word32
taskCreateWrapper = 0x00725dd8
interruptAttach = 0x0064e918
timerStart = 0x0069b328

-- * Executor

data Env = Env
  { envWords :: M.Map Word32 Word32
  , envStarts :: M.Map Word32 ()
  , envInstructions :: [(Word32, Word32)]
  }

-- | Walk one function once. Calls clobber the caller-saved registers; a
-- backward conditional branch whose counter has a constant start, step and
-- bound is replayed for each counter value.
execFunction :: Env -> Function -> FunctionFacts
execFunction env function = finalize (go 0 initialState)
  where
    instructions = envInstructions env
    count = length instructions

    go index state
      | index >= count = state
      | otherwise =
          let (address, word) = instructions !! index
              decoded = decodeWord address word
              state' = step address decoded (snapshot index state)
           in case decoded of
                Branch _ False _ -> case replayLoop index state' of
                  Just replayed -> go (index + 1) replayed
                  Nothing -> go (index + 1) state'
                Branch _ True _ -> go (index + 1) (clobber state')
                BranchExchange _ _ -> go (index + 1) (clobber state')
                _ -> go (index + 1) state'

    snapshot index state
      | IM.member index (stateSnapshots state) = state
      | otherwise = state { stateSnapshots = IM.insert index (stateRegisters state, stateMemory state) (stateSnapshots state) }

    clobber state = state
      { stateRegisters = foldl' (\registers register -> IM.insert register Unknown registers)
          (stateRegisters state) [0, 1, 2, 3, 12, 14]
      , stateMoveWide = IM.empty
      }

    indexOfAddress address = search 0
      where
        search index
          | index >= count = Nothing
          | fst (instructions !! index) == address = Just index
          | otherwise = search (index + 1)

    replayLoop index state = case instructions !! index of
      (address, word) -> case decodeWord address word of
        Branch condition _ target
          | condition /= 14
          , target < address
          , Just headIndex <- indexOfAddress target
          , Just (snapshotRegisters, snapshotMemory) <- IM.lookup headIndex (stateSnapshots state)
          , Just (loopRegister, bound) <- precedingCompare headIndex (index - 1) snapshotRegisters
          , Just (stepKind, stepAmount) <- induction headIndex index loopRegister
          , Just start <- constantOf loopRegister snapshotRegisters ->
              let next value = if stepKind == Add then value + stepAmount else value - stepAmount
                  values = take 4096 (takeWhile (\value -> conditionHolds condition value bound) (iterate next start))
                  failing = next (last values)
                  run previous value = runBody headIndex index previous { stateRegisters = IM.insert loopRegister (Constant value) snapshotRegisters }
                  final = foldl' run (state { stateMemory = snapshotMemory }) (take replayLimit values)
               in if null values then Nothing else Just (final { stateRegisters = IM.insert loopRegister (Constant failing) (stateRegisters final) })
        _ -> Nothing

    -- Execute a loop body once, without replaying a loop inside it.
    runBody headIndex endIndex state = body headIndex state
      where
        body index current
          | index >= endIndex = current
          | otherwise =
              let (address, word) = instructions !! index
                  decoded = decodeWord address word
                  state' = step address decoded current
               in case decoded of
                    Branch _ True _ -> body (index + 1) (clobber state')
                    BranchExchange _ _ -> body (index + 1) (clobber state')
                    _ -> body (index + 1) state'

    -- The compare that bounds a loop is an immediate or a register whose value
    -- the snapshot holds, so a loop over a record array with a computed end is
    -- replayed like one with an immediate end.
    precedingCompare headIndex index registers
      | index < headIndex = Nothing
      | otherwise = case decodeWord (fst (instructions !! index)) (snd (instructions !! index)) of
          Compare register (Immediate bound) -> Just (register, bound)
          Compare register (Shifted source 0 0) -> case constantOf source registers of
            Just bound -> Just (register, bound)
            Nothing -> Nothing
          Compare _ _ -> Nothing
          _ -> precedingCompare headIndex (index - 1) registers

    induction headIndex endIndex register = search headIndex
      where
        search index
          | index >= endIndex = Nothing
          | otherwise = case decodeWord (fst (instructions !! index)) (snd (instructions !! index)) of
              DataOperation Add destination source (Immediate amount)
                | destination == register && source == register -> Just (Add, amount)
              DataOperation Sub destination source (Immediate amount)
                | destination == register && source == register -> Just (Sub, amount)
              _ -> search (index + 1)

    constantOf register registers = case IM.lookup register registers of
      Just (Constant value) -> Just value
      _ -> Nothing

    conditionHolds condition left right = case condition of
      0 -> left == right
      1 -> left /= right
      2 -> left >= right
      3 -> left < right
      8 -> left > right
      9 -> left <= right
      10 -> asSigned left >= asSigned right
      11 -> asSigned left < asSigned right
      12 -> asSigned left > asSigned right
      13 -> asSigned left <= asSigned right
      _ -> False
      where asSigned value = fromIntegral (fromIntegral value :: Int32) :: Integer

    step address decoded state = case decoded of
      Branch _ link target -> callAddress address link target state
      BranchExchange link register -> case IM.lookup register (stateRegisters state) of
        Just (Constant value) ->
          let facts = stateFacts state
              facts' = if imageWord value then facts { factsBuilt = (value, CalledIndirect) : factsBuilt facts } else facts
           in if link
                then clobber state { stateFacts = facts' { factsCalls = CallSite address (physicalAddress value) (argumentValues state) : factsCalls facts' } }
                else state { stateFacts = facts' }
        _ -> state
      MoveWide top register immediate
        | top -> case IM.lookup register (stateMoveWide state) of
            Just low ->
              let value = (immediate `shiftL` 16) .|. low
                  facts = stateFacts state
               in state
                    { stateRegisters = IM.insert register (Constant value) (stateRegisters state)
                    , stateMoveWide = IM.delete register (stateMoveWide state)
                    , stateFacts = if imageWord value then facts { factsBuilt = (value, LoadedOnly) : factsBuilt facts } else facts
                    }
            Nothing -> state
              { stateRegisters = IM.insert register (Constant (immediate `shiftL` 16)) (stateRegisters state)
              , stateMoveWide = IM.delete register (stateMoveWide state)
              }
        | otherwise -> state
              { stateRegisters = IM.insert register (Constant immediate) (stateRegisters state)
              , stateMoveWide = IM.insert register immediate (stateMoveWide state)
              }
      DataOperation operation destination source operand ->
        state { stateRegisters = IM.insert destination (alu operation (registerValue state source) (operandValue state operand)) (stateRegisters state) }
      Load destination base offset registerOffset pcRelative writeback ->
        let cellAddress = addressOf state base offset registerOffset
            baseValue = registerValue state base
            value
              | pcRelative, Just immediate <- offset =
                  maybe Unknown Constant (M.lookup (physicalAddress (address + 8 + fromIntegral immediate)) (envWords env))
              | Just cell <- cellAddress = readCell state cell
              | otherwise = case baseValue of
                  Element indexed -> Element indexed
                  Constant address' | Just _ <- registerOffset -> Element (physicalAddress address')
                  _ -> Unknown
            state' = state
              { stateRegisters = IM.insert destination value (stateRegisters state)
              , stateFacts = indexedUse value (stateFacts state)
              }
         in if writeback then writeBack cellAddress base state' else state'
      Store source base offset registerOffset writeback ->
        let cellAddress = addressOf state base offset registerOffset
            value = registerValue state source
            state' = case cellAddress of
              Just cell ->
                let facts = indexedUse (registerValue state base) (stateFacts state)
                    facts' = case value of
                      Constant word | imageWord word -> facts { factsBuilt = (word, StoredWord) : factsBuilt facts }
                      _ -> facts
                 in state { stateMemory = M.insert cell value (stateMemory state), stateFacts = facts' }
              Nothing -> state { stateFacts = indexedUse (registerValue state base) (stateFacts state) }
         in if writeback then writeBack cellAddress base state' else state'
      Compare _ _ -> state
      Other -> state
      where
        registerValue current register = fromMaybe Unknown (IM.lookup register (stateRegisters current))

    indexedUse value facts = case value of
      Element base -> facts { factsIndexed = base : factsIndexed facts }
      _ -> facts

    imageWord value = M.member (physicalAddress value) (envWords env)

    writeBack address base state = case address of
      Just cell -> case cellDelta cell base of
        Just (anchor, delta) -> state { stateRegisters = IM.insert base (Frame anchor delta) (stateRegisters state) }
        Nothing -> state
      Nothing -> state
      where
        cellDelta cell register = case (cell, IM.lookup register (stateRegisters state)) of
          (CellFrame anchor delta, Just (Frame _ _)) -> Just (anchor, delta)
          _ -> Nothing

    callAddress address link target state
      | not link =
          let facts = stateFacts state
           in if M.member (physicalAddress target) (envStarts env)
                then state { stateFacts = facts { factsDirect = physicalAddress target : factsDirect facts } }
                else state
      | otherwise =
          let arguments = argumentValues state
              facts = stateFacts state
              facts' = facts
                { factsCalls = CallSite address target arguments : factsCalls facts
                , factsDirect = if M.member (physicalAddress target) (envStarts env)
                    then physicalAddress target : factsDirect facts
                    else factsDirect facts
                }
              facts'' = if target == taskCreateWrapper
                then recordTask address state facts'
                else if target == interruptAttach
                  then recordInterrupt address interruptAttach 1 state facts'
                  else if target == timerStart
                    then recordInterrupt address timerStart 2 state facts'
                    else facts'
           in state { stateFacts = facts'' }

    argumentValues state = [value | register <- [0, 1, 2, 3], Just (Constant value) <- [IM.lookup register (stateRegisters state)]]

    recordTask address state facts = case IM.lookup 0 (stateRegisters state) >>= cellOf of
      Nothing -> facts { factsTasks = TaskSite address Nothing Nothing Nothing Nothing : factsTasks facts }
      Just cell ->
        let field offset = readCell state (cellOffset cell offset)
            constant (Constant word) = Just (physicalAddress word)
            constant _ = Nothing
         in facts { factsTasks = TaskSite address (constant (field 0)) (constant (field 16)) (constant (field 4)) (constant (field 8)) : factsTasks facts }

    recordInterrupt address service register state facts = case IM.lookup register (stateRegisters state) of
      Just (Constant handler) | M.member (physicalAddress handler) (envStarts env) ->
        facts { factsInterrupts = InterruptSite address service (physicalAddress handler) : factsInterrupts facts }
      _ -> facts

    cellOf value = case value of
      Constant address -> Just (CellAbsolute (physicalAddress address))
      Frame anchor offset -> Just (CellFrame anchor offset)
      _ -> Nothing

    cellOffset (CellAbsolute address) offset = CellAbsolute (address + offset)
    cellOffset (CellFrame anchor offset) delta = CellFrame anchor (offset + fromIntegral delta)

    addressOf state base offset registerOffset =
      let baseValue = fromMaybe Unknown (IM.lookup base (stateRegisters state))
          offsetValue = case offset of
            Just immediate -> Constant (fromIntegral immediate)
            Nothing -> case registerOffset of
              Just operand -> operandValue state operand
              Nothing -> Constant 0
       in case alu Add baseValue offsetValue of
            Constant address -> Just (CellAbsolute (physicalAddress address))
            Frame anchor delta -> Just (CellFrame anchor delta)
            _ -> Nothing

    readCell state cell = case M.lookup cell (stateMemory state) of
      Just value -> value
      Nothing -> case cell of
        CellAbsolute address -> maybe Unknown Constant (M.lookup address (envWords env))
        CellFrame _ _ -> Unknown

    -- Mov and Mvn read the operand only: the register field the decoder records
    -- as their source is not an input.
    alu operation left right = case operation of
      Mov -> right
      Mvn -> complementValue right
      Add -> addValues left right
      Sub -> subtractValues left right
      Rsb -> subtractValues right left
      And -> bitwise (.&.) left right
      Orr -> bitwise (.|.) left right
      Eor -> bitwise xorWord left right
      OtherAlu -> left

    complementValue (Constant value) = Constant (0xffffffff - value)
    complementValue _ = Unknown

    bitwise combine (Constant a) (Constant b) = Constant (combine a b)
    bitwise _ _ _ = Unknown

    addValues (Constant a) (Constant b) = Constant (a + b)
    addValues (Frame anchor offset) (Constant b) = Frame anchor (offset + fromIntegral b)
    addValues (Constant a) (Frame anchor offset) = Frame anchor (offset + fromIntegral a)
    addValues _ _ = Unknown

    subtractValues (Constant a) (Constant b) = Constant (a - b)
    subtractValues (Frame anchor offset) (Constant b) = Frame anchor (offset - fromIntegral b)
    subtractValues _ _ = Unknown

    operandValue state operand = case operand of
      Immediate value -> Constant value
      Shifted register kind amount -> case IM.lookup register (stateRegisters state) of
        Just (Constant value) -> Constant (shiftValue kind amount value)
        _ -> Unknown

    shiftValue kind amount value = case kind of
      0 -> value `shiftL` amount
      1 -> value `shiftR` amount
      2 -> fromIntegral ((fromIntegral value :: Int32) `shiftR` amount)
      _ -> rotateR value amount

    finalize state = (stateFacts state)
      { factsDirect = reverse (factsDirect (stateFacts state))
      , factsBuilt = reverse (factsBuilt (stateFacts state))
      , factsCalls = reverse (factsCalls (stateFacts state))
      , factsTasks = reverse (factsTasks (stateFacts state))
      , factsInterrupts = reverse (factsInterrupts (stateFacts state))
      , factsIndexed = reverse (factsIndexed (stateFacts state))
      }
    _unusedFunction = function

xorWord :: Word32 -> Word32 -> Word32
xorWord a b = (a .|. b) .&. (0xffffffff - (a .&. b))

-- * Graph

data EdgeKind = Direct | TableEdge | CodeWord deriving (Eq, Ord, Show, Enum, Bounded)

edgeKindLabel :: EdgeKind -> String
edgeKindLabel Direct = "direct"
edgeKindLabel TableEdge = "table"
edgeKindLabel CodeWord = "code-word"

data EdgeSelection = EdgeSelection
  { selectDirect :: Bool
  , selectTable :: Bool
  , selectCodeWord :: Bool
  } deriving (Eq, Show)

allEdgeKinds :: EdgeSelection
allEdgeKinds = EdgeSelection True True True

selected :: EdgeSelection -> EdgeKind -> Bool
selected selection kind = case kind of
  Direct -> selectDirect selection
  TableEdge -> selectTable selection
  CodeWord -> selectCodeWord selection

data Edge = Edge
  { edgeFrom :: Word32
  , edgeTo :: Word32
  , edgeKind :: EdgeKind
  , edgeDetail :: String
  } deriving (Eq, Show)

data RootKind = TaskRoot Word32 | InterruptRoot | CallbackRoot deriving (Eq, Ord, Show)

rootKindLabel :: RootKind -> String
rootKindLabel (TaskRoot taskId) = "task-" ++ show taskId
rootKindLabel InterruptRoot = "interrupt"
rootKindLabel CallbackRoot = "callback"

data Root = Root
  { rootKind :: RootKind
  , rootEntry :: Word32
  , rootProvenance :: [String]
  } deriving (Eq, Show)

data Table = Table
  { tableStart :: Word32
  , tableEnd :: Word32
  , tableTargets :: [Word32]
  , tableNested :: [Word32]
  , tableIndexers :: [Word32]
  } deriving (Eq, Show)

data Census = Census
  { censusFunctions :: M.Map Word32 Function
  , censusEdges :: [Edge]
  , censusRoots :: [Root]
  , censusTables :: [Table]
  , censusUnresolvedTasks :: [TaskSite]
  , censusBuiltFunctions :: S.Set Word32
  , censusUnindexedTables :: Int
  } deriving (Eq, Show)

-- | Build the graph. @functions@ must already carry their image assignment.
buildCensus :: EdgeSelection -> [Listing] -> [Function] -> Census
buildCensus selection listings declaredFunctions =
  let wordsByImage = M.fromList [(listingName listing, listingWords listing) | listing <- listings]
      functions = joinSplitPrologues wordsByImage declaredFunctions
      starts = M.fromList [(functionEntry function, ()) | function <- functions]
      wordMapFor function = M.findWithDefault M.empty (functionImage function) wordsByImage
      literalSets = M.fromList
        [ (functionEntry function, S.fromList (literalTargets (envFor function) function))
        | function <- functions
        ]
      envFor function = Env
        { envWords = wordMapFor function
        , envStarts = starts
        , envInstructions = instructionsIn (wordMapFor function) function
        }
      envForFinal function = (envFor function)
        { envInstructions = [instruction | instruction@(address, _) <- envInstructions (envFor function), not (S.member address (literalSets M.! functionEntry function))]
        }
      facts = M.fromList [(functionEntry function, execFunction (envForFinal function) function) | function <- functions]
      taskSites = concatMap factsTasks (M.elems facts)
      creators = M.fromList
        [ (functionEntry function, site)
        | function <- functions
        , site <- factsTasks (facts M.! functionEntry function)
        , isNothing (taskSiteId site)
        , not (isNothing (taskSiteEntry site))
        ]
      immediateRoots =
        [ Root (TaskRoot taskId) entry [siteLabel site]
        | site <- taskSites
        , Just taskId <- [taskSiteId site]
        , Just entry <- [taskSiteEntry site]
        ]
      creatorRoots =
        [ Root (TaskRoot taskId) entry ["task site " ++ hexAddress (callSiteAddress callSite) ++ " via creator " ++ hexAddress creatorEntry]
        | (creatorEntry, creatorSite) <- M.toList creators
        , Just entry <- [taskSiteEntry creatorSite]
        , caller <- M.elems facts
        , callSite <- factsCalls caller
        , callSiteTarget callSite == creatorEntry
        , taskId : _ <- [callSiteArguments callSite]
        ]
      unresolvedCreators =
        [ creatorSite
        | (creatorEntry, creatorSite) <- M.toList creators
        , not (any (\caller -> any (\callSite -> callSiteTarget callSite == creatorEntry && not (null (callSiteArguments callSite))) (factsCalls caller)) (M.elems facts))
        ]
      interruptRoots =
        [ Root InterruptRoot handler ["interrupt attach " ++ hexAddress (interruptSiteAddress site) ++ " service " ++ hexAddress (interruptSiteService site)]
        | function <- functions
        , site <- factsInterrupts (facts M.! functionEntry function)
        , let handler = interruptSiteHandler site
        ]
      callbackRoots =
        [ Root CallbackRoot target [hexAddress (functionEntry function) ++ " " ++ edgeUseLabel use ++ " " ++ hexAddress target]
        | function <- functions
        , (target, use) <- factsBuilt (facts M.! functionEntry function)
        , let target' = physicalAddress target
        , M.member target' starts
        , use == StoredWord || use == PassedArgument
        ]
      roots = dedupeRoots (immediateRoots ++ creatorRoots ++ interruptRoots ++ callbackRoots)
      directEdges =
        [ Edge (functionEntry function) target Direct (hexAddress (functionEntry function))
        | function <- functions
        , target <- factsDirect (facts M.! functionEntry function)
        ]
      codeEdges =
        [ Edge (functionEntry function) target CodeWord (edgeUseLabel use)
        | function <- functions
        , (target, use) <- factsBuilt (facts M.! functionEntry function)
        , use /= LoadedOnly
        , let target' = physicalAddress target
        , M.member target' starts
        ]
      builtByFunction = M.fromList
        [ ( functionEntry function
          , S.fromList (map (physicalAddress . fst) (factsBuilt facts') ++ map physicalAddress (factsIndexed facts'))
          )
        | function <- functions
        , let facts' = facts M.! functionEntry function
        ]
      indexedBases = S.fromList [base | facts' <- M.elems facts, base <- factsIndexed facts']
      extents = [(functionEntry function, functionEntry function + functionSize function) | function <- functions]
      (tables, unindexed) = discoverTables wordsByImage extents literalSets builtByFunction indexedBases
      tableEdges = concatMap tableOut tables
      tableOut table = [Edge indexer target TableEdge (hexAddress (tableStart table)) | indexer <- tableIndexers table, target <- tableTargets table]
      edges = filter (selected selection . edgeKind) (dedupeEdges (directEdges ++ codeEdges ++ tableEdges))
   in Census
        { censusFunctions = M.fromList [(functionEntry function, function) | function <- functions]
        , censusEdges = edges
        , censusRoots = roots
        , censusTables = tables
        , censusUnresolvedTasks = unresolvedCreators
        , censusBuiltFunctions = S.fromList [target | function <- functions, (target, _) <- factsBuilt (facts M.! functionEntry function), M.member (physicalAddress target) starts]
        , censusUnindexedTables = unindexed
        }

siteLabel :: TaskSite -> String
siteLabel site = "task site " ++ hexAddress (taskSiteAddress site)

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ pad (showHex value "")
  where pad text = replicate (8 - length text) '0' ++ text

instructionsIn :: M.Map Word32 Word32 -> Function -> [(Word32, Word32)]
instructionsIn wordsByAddress function =
  let (_, found, above) = M.splitLookup (functionEntry function) wordsByAddress
      fromEntry = maybe [] (\word -> [(functionEntry function, word)]) found ++ M.toAscList above
   in takeWhile (\(address, _) -> address < functionEntry function + functionSize function) fromEntry

-- | The function table starts a function at the @push@ that saves the link
-- register, so @sub sp, sp, #N@ before one is declared as its own function.
joinSplitPrologues :: M.Map String (M.Map Word32 Word32) -> [Function] -> [Function]
joinSplitPrologues wordsByImage = go . sortOn functionEntry
  where
    go (function : next : rest)
      | Just joined <- join function next = go (joined : rest)
    go (function : rest) = function : go rest
    go [] = []

    join function next
      | functionSize function /= 4 = Nothing
      | functionImage function /= functionImage next = Nothing
      | functionEntry function + 4 /= functionEntry next = Nothing
      | Just word <- wordAt function (functionEntry function)
      , DataOperation Sub 13 13 (Immediate _) <- decodeWord (functionEntry function) word
      , Just registers <- wordAt next (functionEntry next) >>= pushedRegisters
      , testBit registers 14
      = Just function { functionSize = functionSize next + 4 }
      | otherwise = Nothing

    wordAt function address = M.lookup address (M.findWithDefault M.empty (functionImage function) wordsByImage)

-- | The register list of a @push@ (@stmdb sp!@), which the single-word decode
-- reads as 'Other': a pre-indexed, writeback block store to sp.
pushedRegisters :: Word32 -> Maybe Word32
pushedRegisters word
  | word `shiftR` 28 == 14
  , (word `shiftR` 25) .&. 0x7 == 0x4
  , testBit word 24
  , not (testBit word 23)
  , testBit word 21
  , not (testBit word 20)
  , (word `shiftR` 16) .&. 0xf == 13
  = Just (word .&. 0xffff)
  | otherwise = Nothing

-- | Addresses a function loads through @pc@: its literal pools.
literalTargets :: Env -> Function -> [Word32]
literalTargets env _ = mapMaybe literal (envInstructions env)
  where
    literal (address, word) = case decodeWord address word of
      Load _ 15 (Just offset) Nothing True _ -> Just (physicalAddress (address + 8 + fromIntegral offset))
      _ -> Nothing

-- * Tables

-- | Data runs whose words name function starts, or after a pass, table starts.
-- A run may begin with up to two zero words and bridge up to two zeros between
-- pointers: the shape stock's action tables have.
--
-- A base the code reads by a scaled index is a table too, whatever its words
-- are: when those words are themselves table starts it is a table of table
-- addresses, and the indexer reaches every target of every table it names.
discoverTables
  :: M.Map String (M.Map Word32 Word32)
  -> [(Word32, Word32)]
  -> M.Map Word32 (S.Set Word32)
  -> M.Map Word32 (S.Set Word32)
  -> S.Set Word32
  -> ([Table], Int)
discoverTables wordsByImage extents literalSets builtByFunction seeds =
  iterateTables candidates (1 :: Int)
  where
    allWords = M.fromList (concatMap M.toAscList (M.elems wordsByImage))
    code = S.fromList (concat [ [entry, entry + 4 .. end - 4] | (entry, end) <- extents ])
    dataAddresses = S.fromList
      ([address | address <- M.keys allWords, not (S.member address code)]
        ++ concatMap S.toList (M.elems literalSets))
    imageLow = fst (M.findMin allWords)
    imageHigh = fst (M.findMax allWords) + 4
    pointer word = word /= 0 && word .&. 3 == 0 && imageLow <= word && word < imageHigh
    -- The words from an address while each points into an image.
    pointerRun address = go address []
      where
        go current found = case M.lookup current allWords of
          Just word | pointer word -> go (current + 4) (word : found)
          _ -> (current, reverse found)
    -- An address whose words are up to two zeros and then a pointer run.
    nestedTable address = case M.lookup address allWords of
      Just 0 -> case M.lookup (address + 4) allWords of
        Just 0 -> shape (address + 8) address
        _ -> shape (address + 4) address
      _ -> shape address address
      where
        shape start named = case pointerRun start of
          (end, targets@(_ : _)) -> Just (named, end, targets)
          _ -> Nothing
    -- A base whose words each name a nested table, up to the first that does not.
    tableOfTables base = go base []
      where
        go address found = case M.lookup address allWords >>= (nestedTable . physicalAddress) of
          Just (named, _, _) -> go (address + 4) (named : found)
          Nothing -> if null found then Nothing else Just (address, reverse found)
    seedTables =
      [ Table base end [] nested [] | base <- S.toList seeds, Just (end, nested) <- [tableOfTables base] ]
        ++ [ Table named end targets [] [] | base <- S.toList seeds, Just (_, nested) <- [tableOfTables base], named <- nested, Just (_, end, raw) <- [nestedTable named], let targets = [physicalAddress word | word <- raw, M.member (physicalAddress word) starts] ]
    candidates = S.fromList (M.keys starts)
    starts = M.fromList [(entry, ()) | (entry, _) <- extents]

    iterateTables current iteration =
      let runs = groupRuns current
          pointed = concatMap (pointedTables current) runs
          known = S.union current (S.fromList (map tableStart pointed))
          tables = dedupeTables (map (makeTable known) runs ++ pointed)
          starts' = S.fromList (map tableStart tables)
          next = S.union current starts'
       in if next == current || iteration > 8
            then attribute (dedupeTables (seedTables ++ tables))
            else iterateTables next (iteration + 1)

    -- A table word that is neither a function start nor a known table start can
    -- still name a table: the words at that address are then its targets.
    pointedTables candidateSet (start, lastPointer) =
      [ Table target end targets [] []
      | address <- [start, start + 4 .. lastPointer]
      , Just value <- [M.lookup address allWords]
      , let target = physicalAddress value
      , target /= 0
      , S.member target dataAddresses
      , not (M.member target starts)
      , not (S.member target candidateSet)
      , Just (end, targets) <- [runAt target]
      ]
      where
        runAt from = search from [] (0 :: Int)
          where
            search address found zeros
              | zeros > 3 = stop address found
              | otherwise = case M.lookup address allWords of
                  Just word
                    | M.member (physicalAddress word) starts -> search (address + 4) (physicalAddress word : found) 0
                    | word == 0, zeros < 3 -> search (address + 4) found (zeros + 1)
                  _ -> stop address found
            stop address found = if null found then Nothing else Just (address, reverse found)

    dedupeTables tables = M.elems (M.fromListWith preferRun [(tableStart table, table) | table <- tables])
      where
        preferRun left right
          | length (tableNested left) > length (tableNested right) = left
          | length (tableNested right) > length (tableNested left) = right
          | length (tableTargets left) > length (tableTargets right) = left
          | length (tableTargets right) > length (tableTargets left) = right
          | tableEnd left >= tableEnd right = left
          | otherwise = right

    groupRuns candidateSet = reverse (finish (foldl' extend (Nothing, []) dataWords))
      where
        dataWords = [(address, word) | (address, word) <- M.toAscList allWords, S.member address dataAddresses]
        candidateWord word = S.member (physicalAddress word) candidateSet
        extend (Nothing, runs) (address, word)
          | candidateWord word = (Just (tableStartOf address, address), runs)
          | otherwise = (Nothing, runs)
        extend (Just (start, lastAddress), runs) (address, word)
          | address /= lastAddress + 4 = extend (Nothing, (start, lastAddress) : runs) (address, word)
          | candidateWord word = (Just (start, address), runs)
          | word == 0 && address - lastAddress <= 12 = (Just (start, lastAddress), runs)
          | otherwise = (Nothing, (start, lastAddress) : runs)
        finish (Nothing, runs) = runs
        finish (Just (start, lastAddress), runs) = (start, lastAddress) : runs
        tableStartOf address
          | Just 0 <- M.lookup (address - 8) allWords
          , Just 0 <- M.lookup (address - 4) allWords
          , S.member (address - 8) dataAddresses = address - 8
          | otherwise = address

    makeTable candidateSet (start, lastPointer) =
      let addresses = [start, start + 4 .. lastPointer]
          values = [fromMaybe 0 (M.lookup address allWords) | address <- addresses]
          targets = [physicalAddress value | value <- values, M.member (physicalAddress value) starts]
          nested = [physicalAddress value | value <- values, S.member (physicalAddress value) candidateSet, physicalAddress value /= 0, not (M.member (physicalAddress value) starts)]
       in Table start (lastPointer + 4) targets nested []

    -- A table's targets include the targets of every table it names, and its
    -- indexers are the functions that build a base inside its extent.
    attribute tables =
      let sorted = sortOn tableStart tables
          byIndex = IM.fromList (zip [0 ..] sorted)
          byStart = M.fromList [(tableStart table, table) | table <- sorted]
          startIndex = M.fromList [(tableStart table, index) | (index, table) <- IM.toList byIndex]
          closure seen table
            | S.member (tableStart table) seen = []
            | otherwise =
                [ nested
                | start <- tableNested table
                , Just nested <- [M.lookup start byStart]
                , not (S.member (tableStart nested) seen)
                ] ++ concat
                [ closure (S.insert (tableStart table) seen) nested
                | start <- tableNested table
                , Just nested <- [M.lookup start byStart]
                ]
          targetsOf table = S.toList (S.fromList (tableTargets table ++ concatMap tableTargets (closure S.empty table)))
          addIndexer accumulated (entry, values) = S.foldl' (add entry) accumulated values
          add entry acc value = case M.lookupLE value startIndex of
            Just (_, index) | value < tableEnd (byIndex IM.! index) -> IM.adjust (S.insert entry) index acc
            _ -> acc
          indexers = foldl' addIndexer (IM.fromList [(index, S.empty) | index <- [0 .. length sorted - 1]]) (M.toList builtByFunction)
          attributed =
            [ table { tableTargets = targetsOf table, tableIndexers = S.toList (IM.findWithDefault S.empty index indexers) }
            | (index, table) <- IM.toList byIndex
            ]
       in (attributed, length [table | table <- attributed, null (tableIndexers table)])

-- * Reach

-- | Reach over the selected edges from one entry, as a set of function starts.
reachFrom :: Census -> [Word32] -> S.Set Word32
reachFrom census entries = S.fromList (map physicalAddress entries) <> closure
  where
    adjacency = adjacencyOf census
    closure = go (S.fromList (map physicalAddress entries)) (map physicalAddress entries)
    go seen [] = seen
    go seen (address : rest) =
      let next = [target | (target, _) <- M.findWithDefault [] address adjacency, not (S.member target seen)]
       in go (foldl' (flip S.insert) seen next) (next ++ rest)

adjacencyOf :: Census -> M.Map Word32 [(Word32, EdgeKind)]
adjacencyOf census = M.fromListWith (++)
  [(physicalAddress (edgeFrom edge), [(physicalAddress (edgeTo edge), edgeKind edge)]) | edge <- censusEdges census]

-- | Per function, the roots that reach it and the edge kinds on the way. The
-- entry itself is reached by its root with no edge.
reachByFunction :: Census -> M.Map Word32 [(RootKind, S.Set EdgeKind)]
reachByFunction census = M.fromListWith (++)
  [ (address, [(rootKind root, kinds)])
  | root <- censusRoots census
  , (address, kinds) <- M.toList (walk root)
  ]
  where
    adjacency = adjacencyOf census
    walk root = go (M.singleton (physicalAddress (rootEntry root)) S.empty) [physicalAddress (rootEntry root)]
      where
        go reached [] = reached
        go reached (address : rest) =
          let next = [(target, kind) | (target, kind) <- M.findWithDefault [] address adjacency, not (S.member kind (M.findWithDefault S.empty target reached))]
              reached' = foldl' (\accumulated (target, kind) -> M.insertWith S.union target (S.singleton kind) accumulated) reached next
           in go reached' (rest ++ map fst next)

-- | The functions the given root kinds reach, with the edge kinds.
reachableFunctions :: Census -> (RootKind -> Bool) -> M.Map Word32 (S.Set EdgeKind)
reachableFunctions census accepts = M.fromListWith S.union
  [ (address, kinds)
  | root <- censusRoots census
  , accepts (rootKind root)
  , (address, kinds) <- M.toList (walk root)
  ]
  where
    adjacency = adjacencyOf census
    walk root = go (M.singleton (physicalAddress (rootEntry root)) S.empty) [physicalAddress (rootEntry root)]
      where
        go reached [] = reached
        go reached (address : rest) =
          let next = [(target, kind) | (target, kind) <- M.findWithDefault [] address adjacency, not (S.member kind (M.findWithDefault S.empty target reached))]
              reached' = foldl' (\accumulated (target, kind) -> M.insertWith S.union target (S.singleton kind) accumulated) reached next
           in go reached' (rest ++ map fst next)

-- * Output

dedupeEdges :: [Edge] -> [Edge]
dedupeEdges = M.elems . M.fromList . map (\edge -> ((edgeFrom edge, edgeTo edge, edgeKind edge), edge))

dedupeRoots :: [Root] -> [Root]
dedupeRoots = M.elems . M.fromList . map (\root -> ((rootKind root, physicalAddress (rootEntry root)), root))

censusJson :: Census -> A.Value
censusJson census = object
  [ "schema" .= (1 :: Int)
  , "status" .= ("complete" :: String)
  , "functions" .= object
      [ "count" .= M.size (censusFunctions census)
      , "bytes" .= sum [fromIntegral (functionSize function) :: Integer | function <- M.elems (censusFunctions census)]
      , "built" .= S.size (censusBuiltFunctions census)
      ]
  , "edges" .= [object ["from" .= hexAddress (edgeFrom edge), "to" .= hexAddress (edgeTo edge), "kind" .= edgeKindLabel (edgeKind edge), "detail" .= edgeDetail edge] | edge <- censusEdges census]
  , "roots" .= [object ["kind" .= rootKindLabel (rootKind root), "entry" .= hexAddress (rootEntry root), "provenance" .= rootProvenance root] | root <- censusRoots census]
  , "tables" .= [object
      [ "start" .= hexAddress (tableStart table)
      , "bytes" .= (fromIntegral (tableEnd table - tableStart table) :: Integer)
      , "targets" .= map hexAddress (tableTargets table)
      , "nested" .= map hexAddress (tableNested table)
      , "indexers" .= map hexAddress (tableIndexers table)
      ] | table <- censusTables census]
  , "unindexed_tables" .= censusUnindexedTables census
  , "unresolved_task_sites" .= [object ["site" .= hexAddress (taskSiteAddress site), "entry" .= maybe "" hexAddress (taskSiteEntry site)] | site <- censusUnresolvedTasks census]
  , "reach" .= [ object
      [ "entry" .= hexAddress entry
      , "name" .= functionName function
      , "image" .= functionImage function
      , "size" .= (fromIntegral (functionSize function) :: Integer)
      , "roots" .= [object ["kind" .= rootKindLabel kind, "edges" .= map edgeKindLabel (S.toList kinds)] | (kind, kinds) <- roots]
      ] | (entry, roots) <- M.toAscList (reachByFunction census), Just function <- [M.lookup entry (censusFunctions census)] ]
  ]

censusTable :: Census -> String
censusTable census = unlines (header ++ rootRows ++ tableRows ++ summaryRows)
  where
    header =
      [ "functions " ++ show (M.size (censusFunctions census))
      , "edges " ++ show (length (censusEdges census)) ++ " (" ++ counts ++ ")"
      , "roots " ++ show (length (censusRoots census))
      ]
    counts = joinWith ", " [edgeKindLabel kind ++ " " ++ show (length (filter ((== kind) . edgeKind) (censusEdges census))) | kind <- [minBound .. maxBound]]
    rootRows = ["root " ++ rootKindLabel (rootKind root) ++ " " ++ hexAddress (rootEntry root) ++ " reaches " ++ show (S.size (reachFrom census [rootEntry root])) | root <- censusRoots census]
    tableRows = ["table " ++ hexAddress (tableStart table) ++ ".." ++ hexAddress (tableEnd table) ++ " targets " ++ show (length (tableTargets table)) ++ " indexers " ++ show (map hexAddress (tableIndexers table)) | table <- censusTables census, not (null (tableIndexers table))]
    summaryRows =
      [ "unindexed tables " ++ show (censusUnindexedTables census)
      , "unresolved task sites " ++ show (length (censusUnresolvedTasks census))
      ]
    joinWith separator = foldr1 (\left right -> left ++ separator ++ right)

-- | The extent rows @//emulation:coverage@ reads for the functions the given
-- entries reach, with the digest of each extent's stock bytes.
extentsJson :: Census -> [Listing] -> [Word32] -> Either String A.Value
extentsJson census listings entries = do
  rows <- mapM row (S.toList (reachFrom census entries))
  pure (object ["entries" .= rows])
  where
    row entry = case M.lookup entry (censusFunctions census) of
      Nothing -> Left ("reached address " ++ hexAddress entry ++ " is not a declared function start")
      Just function -> do
        digest <- extentDigest listings function
        pure (object ["entry" .= hexAddress entry, "extent" .= (fromIntegral (functionSize function) :: Integer), "stock" .= object ["sha256" .= digest]])

extentDigest :: [Listing] -> Function -> Either String String
extentDigest listings function =
  let wordsByAddress = case [listingWords listing | listing <- listings, listingName listing == functionImage function] of
        [] -> M.empty
        (wordMap : _) -> wordMap
      addresses = [functionEntry function, functionEntry function + 4 .. functionEntry function + functionSize function - 4]
   in if any (`M.notMember` wordsByAddress) addresses
        then Left ("extent " ++ hexAddress (functionEntry function) ++ " is not wholly in its listing")
        else Right (sha256Hex (sha256 (B.pack (concatMap (bytesOf wordsByAddress) addresses))))
  where
    bytesOf wordsByAddress address = case M.lookup address wordsByAddress of
      Just word -> [fromIntegral (word .&. 0xff), fromIntegral ((word `shiftR` 8) .&. 0xff), fromIntegral ((word `shiftR` 16) .&. 0xff), fromIntegral (word `shiftR` 24)]
      Nothing -> []
