{-# LANGUAGE OverloadedStrings #-}

-- | Admission of the linked image over the two retired image-5 extents, and
-- record-5 staging of the admitted bytes into a candidate DAT.
--
-- Admission checks owned symbols and sections, entry words, literal words,
-- direct branches, stores, trap fill, function frames and direct-call depth
-- against task 43's stack. Register branches are admitted only within the
-- symbol extents of 'boundarySymbols'. Their targets come from the typed
-- stock-code-word values at the source boundary.
--
-- Memory safety of the rest rests on the compiler and the bounded types: the
-- crate root denies @unsafe_code@, @engine@ and @machine0@ forbid it, every
-- index is a bounded type parsed at entry. The comparison suites and the census
-- fetch traps cover the retained callers' actual entries, and the @unsafe@
-- blocks of @src/stock.rs@ are listed with their invariants in
-- @replacement/README.md@.
module Replacement.Saclac.Artifact
  ( Extent (..)
  , extents
  , extentBytes
  , extentContains
  , Entry (..)
  , entries
  , Retained (..)
  , retainedExternals
  , retainedData
  , trapWord
  , trapBytes
  , Section (..)
  , parseSections
  , PlacementError (..)
  , WalkReason (..)
  , Admitted (..)
  , admit
  , ownedSpans
  , installExtent
  , installedImage
  , admitInstalled
  , Staged (..)
  , stageSaclac
  , contractValue
  , digest
  ) where

import qualified A32 as A
import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (catMaybes, listToMaybe, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import Numeric (readHex)
import qualified Replacement.Record5Staging as Record5

-- | One retired stock extent of the decoded image 5.
data Extent = Extent
  { extentLow :: Word32
  , extentHigh :: Word32
  } deriving (Eq, Show)

-- | The dispatcher's extent and machine 0's router extent, in address order.
extents :: [Extent]
extents = [Extent 0xefca6c 0xefcb88, Extent 0xefcb88 0xefd2fc]

extentBytes :: Extent -> Int
extentBytes (Extent low high) = fromIntegral (high - low)

extentContains :: Extent -> Word32 -> Bool
extentContains (Extent low high) address = low <= address && address < high

-- | The entry word each retained stock caller enters, its veneer symbol and the
-- owned body the veneer branches to.
data Entry = Entry
  { entryAddress :: Word32
  , entryVeneer :: String
  , entryBody :: String
  } deriving (Eq, Show)

entries :: [Entry]
entries =
  [ Entry 0xefca6c "xm1_saclac_dispatch_entry" "xm1_saclac_dispatch"
  , Entry 0xefcb88 "xm1_saclac_router_entry" "xm1_saclac_router"
  ]

-- | A stock word the owned image reaches outside its extents.
data Retained = Retained
  { retainedName :: String
  , retainedAddress :: Word32
  } deriving (Eq, Show)

-- | Code entries the owned image calls: the dispatcher's trace call. The shim
-- register lists it as the saclac trace row.
retainedExternals :: [Retained]
retainedExternals = [Retained "trace" 0xf46c38]

externalAddresses :: [Word32]
externalAddresses = map retainedAddress retainedExternals

-- | The stock tables and arrays the boundary module reads and writes.
data StockArray
  = CellTables
  | ActionTables
  | SecondTables
  | StateCounts
  | Rows
  | States
  deriving (Eq, Show)

-- | Each declared array's address and byte extent. The rows and the states are
-- two 74-byte arrays; the four table pointer arrays are 148 bytes each.
stockArrays :: [(StockArray, String, Word32, Word32)]
stockArrays =
  [ (CellTables, "cell-tables", 0xf12f70, 148)
  , (ActionTables, "action-tables", 0xf133ec, 148)
  , (SecondTables, "second-tables", 0xf12074, 148)
  , (StateCounts, "state-counts", 0xf1158c, 74)
  , (Rows, "rows", 0x169d468, 74)
  , (States, "states", 0x169d4b4, 74)
  ]

-- | Data words the owned image reads: the four per-machine tables and the two
-- halfword arrays the boundary module supplies.
retainedData :: [Retained]
retainedData = [Retained name address | (_, name, address, _) <- stockArrays]

stockArrayAddresses :: [Word32]
stockArrayAddresses = [address | (_, _, address, _) <- stockArrays]

-- | The platform stager's trap word for unused extent bytes.
trapWord :: Word32
trapWord = 0xe7f000f0

trapBytes :: B.ByteString
trapBytes = B.pack [0xf0, 0x00, 0xf0, 0xe7]

-- | One allocated section of the linked ELF, from @llvm-readelf -S@.
data Section = Section
  { sectionName :: String
  , sectionLow :: Word32
  , sectionBytes :: Int
  , sectionAllocated :: Bool
  , sectionWritable :: Bool
  , sectionNoBits :: Bool
  } deriving (Eq, Show)

-- | Why admission refused an instruction. Each constructor is one rule.
data WalkReason
  = DecoderRejected A.Forbidden
  | PcRelativeTransferWriteback
  | PcRelativeLiteralOutside
  | BranchTargetNotAllowed Word32
  | BranchToDataWord
  | FallThroughIntoDataWord
  | FallThroughOutsideOwned
  | LiteralOutside Word32
  | StoreToCode
  | IndirectOutsideBoundary String
  | FrameNotDerivable
  | RecursiveCall Word32
  | StackBudgetExceeded Int
  deriving (Eq, Show)

data PlacementError
  = SymbolsRejected
  | SectionsRejected
  | DataSymbol String
  | SymbolOutside String Word32 Word32
  | SectionOutside String Word32 Int
  | WritableSection String
  | EntryMissing String
  | EntryMoved String Word32 Word32
  | EntryVeneerSize String Word32
  | EntryNotOwned String Word32
  | EntryBranchMismatch String Word32 Word32
  | WalkRefused Word32 WalkReason
  | ExtentImageMismatch Word32 Int
  | FillDiffers Word32
  | OwnedArrayMissing String
  | TableLengthMismatch String Int Word32 Word32
  deriving (Eq, Show)

require :: Bool -> PlacementError -> Either PlacementError ()
require condition failure = unless condition (Left failure)

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256

hexWord32 :: String -> Maybe Word32
hexWord32 text0 =
  let text = case text0 of
        '0' : 'x' : rest -> rest
        '0' : 'X' : rest -> rest
        _ -> text0
  in case readHex text of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> Just (fromInteger value)
    _ -> Nothing

-- | Parse the section table of @llvm-readelf -S@ output.
parseSections :: B.ByteString -> Either PlacementError [Section]
parseSections bytes = fmap catMaybes (mapM parseRow (mapMaybe sectionRow (C.lines bytes)))
  where
    sectionRow line = case dropWhile (not . C.isPrefixOf ".") (C.words line) of
      row@(_ : _ : _ : _ : _ : _ : _ : _) -> Just row
      _ -> Nothing
    parseRow (name : kind : address : _offset : size : _entry : flags : _)
      | kind `elem` ["PROGBITS", "NOBITS"] = do
          low <- maybe (Left SectionsRejected) Right (hexWord32 (C.unpack address))
          count <- maybe (Left SectionsRejected) (Right . fromIntegral) (hexWord32 (C.unpack size))
          pure (Just (Section (C.unpack name) low count ('A' `C.elem` flags) ('W' `C.elem` flags) (kind == "NOBITS")))
    parseRow _ = Right Nothing

-- | The addresses the disassembly prints as instructions. Every other word in
-- the owned spans is data.
instructionAddresses :: B.ByteString -> Either PlacementError (S.Set Word32)
instructionAddresses bytes = fmap (S.fromList . catMaybes) (mapM row (C.lines bytes))
  where
    row line =
      let (addressText, rest) = C.break (== ':') line
          after = C.dropWhile (== ' ') (C.drop 1 rest)
       in case hexWord32 (C.unpack (C.dropWhile (== ' ') addressText)) of
            Just address | not (C.null after), C.head after == '\t' -> Right (Just address)
            _ -> Right Nothing

-- | The words the disassembly prints as literal pool entries.
literalAddresses :: B.ByteString -> Either PlacementError (S.Set Word32)
literalAddresses bytes = fmap (S.fromList . concat) (mapM row (C.lines bytes))
  where
    row line =
      let (addressText, rest) = C.break (== ':') line
       in case hexWord32 (C.unpack (C.dropWhile (== ' ') addressText)) of
            Just address | ".word" `elem` C.words (C.drop 1 rest) -> Right [address]
            _ -> Right []

-- | Each machine's cell count, action-table word count, second-table word count
-- and row count, from image 5's pointer arrays and table bytes. A table ends at
-- the next table or at the next declared array, whichever is nearer, so
-- machine 3's and 14's second tables are 2 words and machine 20's and 21's
-- action tables are 8 and 7 words.
data MachineLengths = MachineLengths
  { machineCellCount :: Word32
  , machineActionWords :: Word32
  , machineSecondWords :: Word32
  , machineRowCount :: Word32
  } deriving (Eq, Show)

machineLengths :: [MachineLengths]
machineLengths =
  [ MachineLengths 528 46 2 48
  , MachineLengths 280 26 2 20
  , MachineLengths 54 13 2 9
  , MachineLengths 150 35 2 15
  , MachineLengths 15 8 2 5
  , MachineLengths 24 12 2 4
  , MachineLengths 24 11 2 8
  , MachineLengths 63 15 2 7
  , MachineLengths 84 45 2 28
  , MachineLengths 72 24 2 12
  , MachineLengths 42 12 2 6
  , MachineLengths 651 57 2 21
  , MachineLengths 1275 95 2 51
  , MachineLengths 120 25 2 20
  , MachineLengths 24 13 2 6
  , MachineLengths 16 9 2 4
  , MachineLengths 21 14 2 7
  , MachineLengths 504 49 2 28
  , MachineLengths 48 17 2 8
  , MachineLengths 49 13 2 7
  , MachineLengths 9 8 2 3
  , MachineLengths 8 7 2 4
  , MachineLengths 104 33 2 13
  , MachineLengths 180 43 2 9
  , MachineLengths 40 16 2 8
  , MachineLengths 8 8 2 2
  , MachineLengths 676 55 2 52
  , MachineLengths 99 24 2 9
  , MachineLengths 252 33 2 14
  , MachineLengths 24 13 2 6
  , MachineLengths 24 13 2 6
  , MachineLengths 66 14 2 6
  , MachineLengths 56 19 2 8
  , MachineLengths 28 16 2 4
  , MachineLengths 144 27 2 9
  , MachineLengths 12 11 2 6
  , MachineLengths 20 8 2 5
  ]

-- | The image's own arrays: the four per-machine counts the engine checks
-- against, and machine 0's event-to-row table.
data OwnedArray
  = OwnedCellCounts
  | OwnedActionCounts
  | OwnedSecondCounts
  | OwnedRowCounts
  | OwnedRowMap
  deriving (Eq, Show)

ownedArraySymbol :: OwnedArray -> String
ownedArraySymbol OwnedCellCounts = "SACLAC_CELL_COUNTS"
ownedArraySymbol OwnedActionCounts = "SACLAC_ACTION_COUNTS"
ownedArraySymbol OwnedSecondCounts = "SACLAC_SECOND_COUNTS"
ownedArraySymbol OwnedRowCounts = "SACLAC_ROW_COUNTS"
ownedArraySymbol OwnedRowMap = "SACLAC_ROW_MAP"

-- | The functions the compiler emits for @src/stock.rs@. The two veneers are its
-- @#[no_mangle]@ entries; the third is @engine::dispatch@ instantiated at
-- @StockTables@ and @StockPorts@, into which the module's @call3@ and
-- @enter_word@ inline their @unsafe@ code-pointer calls.
boundarySymbols :: [String]
boundarySymbols =
  [ "xm1_saclac_dispatch"
  , "xm1_saclac_router"
  , "_RINvNtCs5dX5h91vudr_10xm1_saclac6engine8dispatchNtNtB4_5stock11StockTablesNtBM_10StockPortsEB4_"
  ]

-- | The set of instructions from a function's entry that set up its frame: the
-- push of lr and any sp or frame-pointer arithmetic that follows it.
prologueWords :: Admitted -> Word32 -> Word32 -> S.Set Word32
prologueWords admitted start end = go start True
  where
    go address first
      | address >= end = S.empty
      | isPrologueInstruction first (A.decode address (wordAt admitted address)) =
          S.insert address (go (address + 4) False)
      | otherwise = S.empty

isPrologueInstruction :: Bool -> A.Instruction -> Bool
isPrologueInstruction first instruction = case instruction of
  A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers
    | first -> base == A.sp && A.lr `elem` registers
  A.DataProcessing (A.Cond 14) _ _ destination base (A.Immediate _) ->
    destination == A.sp || base == A.sp
  A.DataProcessing (A.Cond 14) A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) ->
    destination == A.sp || source == A.sp
  _ -> False

-- | The registers a function loads from a pc-relative literal whose word names
-- an owned address.
codeLiteralRegisters :: Admitted -> S.Set Word32 -> F.Symbol -> S.Set A.Reg
codeLiteralRegisters admitted instructions symbol = S.fromList
  [ destination
  | address <- [F.symbolAddress symbol, F.symbolAddress symbol + 4 .. F.symbolAddress symbol + F.symbolSize symbol - 4]
  , S.member address instructions
  , A.Load _ _ _ destination base (A.OffsetImmediate immediate) A.OffsetOnly <-
      [A.decode address (wordAt admitted address)]
  , base == A.pc
  , insideExtents (wordAt admitted (address + 8 + fromIntegral immediate))
  ]


checkBoundary :: Admitted -> Word32 -> Either PlacementError ()
checkBoundary admitted address =
  case containingSymbol admitted address of
    Just symbol | F.symbolName symbol `elem` boundarySymbols -> Right ()
    symbol -> Left (WalkRefused address (IndirectOutsideBoundary (maybe "" F.symbolName symbol)))

unconditionalPcWrite :: A.Instruction -> Bool
unconditionalPcWrite instruction = case instruction of
  A.Load (A.Cond 14) _ _ destination _ _ _ -> destination == A.pc
  A.LoadMultiple (A.Cond 14) _ _ _ registers -> A.pc `elem` registers
  A.DataProcessing (A.Cond 14) _ _ destination _ _ -> destination == A.pc
  A.ReadStatus (A.Cond 14) destination -> destination == A.pc
  A.ZeroExtend (A.Cond 14) destination _ _ _ -> destination == A.pc
  A.Multiply (A.Cond 14) destination _ _ _ -> destination == A.pc
  A.Compute (A.Cond 14) destinations -> A.pc `elem` destinations
  _ -> False

mayFallThrough :: A.Instruction -> Bool
mayFallThrough instruction = case instruction of
  A.Branch (A.Cond 14) _ _ -> False
  A.BranchExchange (A.Cond 14) _ _ -> False
  _ -> not (unconditionalPcWrite instruction)

-- | A pc-relative load or store with writeback is unpredictable.
isPcRelativeWriteback :: A.Instruction -> Bool
isPcRelativeWriteback instruction = case instruction of
  A.Load _ _ _ _ base _ indexing -> base == A.pc && indexing /= A.OffsetOnly
  A.Store _ _ _ base _ indexing -> base == A.pc && indexing /= A.OffsetOnly
  _ -> False

isPcRelativeLoad :: A.Instruction -> Maybe Word32
isPcRelativeLoad instruction = case instruction of
  A.Load _ _ _ _ base (A.OffsetImmediate immediate) A.OffsetOnly
    | base == A.pc -> Just (fromIntegral immediate)
  _ -> Nothing

ownedAddress :: Admitted -> Word32 -> Bool
ownedAddress admitted address =
  any (\(low, high) -> low <= address && address + 4 <= high) (ownedSpans admitted)

-- | Every literal pool entry names an owned address, a declared external or a
-- declared stock table or array.
checkLiterals :: Admitted -> S.Set Word32 -> Either PlacementError ()
checkLiterals admitted literals = forM_ (S.toList literals) $ \address -> do
  let value = wordAt admitted address
  unless (insideExtents value || value `elem` externalAddresses || value `elem` stockArrayAddresses) $
    Left (WalkRefused address (LiteralOutside value))

-- | No store uses a base that a pc-relative literal naming the extents loaded in
-- the same function, and no store addresses memory through pc.
checkStores :: Admitted -> S.Set Word32 -> Either PlacementError ()
checkStores admitted instructions =
  forM_ [symbol | symbol <- admittedSymbols admitted, F.symbolSize symbol > 0] $ \symbol -> do
    let codeRegisters = codeLiteralRegisters admitted instructions symbol
        start = F.symbolAddress symbol
        end = start + F.symbolSize symbol
    forM_ [start, start + 4 .. end - 4] $ \address ->
      when (S.member address instructions) $ case A.decode address (wordAt admitted address) of
        A.Store _ _ _ base _ _ | S.member base codeRegisters ->
          Left (WalkRefused address StoreToCode)
        A.Store _ _ _ base _ _ | base == A.pc -> Left (WalkRefused address StoreToCode)
        A.StoreMultiple _ base _ _ _ | S.member base codeRegisters ->
          Left (WalkRefused address StoreToCode)
        A.StoreMultiple _ base _ _ _ | base == A.pc -> Left (WalkRefused address StoreToCode)
        _ -> Right ()

-- | Instructions decode, direct targets name admitted code, and no branch or
-- fall-through enters a data word.
checkControl :: Admitted -> S.Set Word32 -> S.Set Word32 -> Either PlacementError ()
checkControl admitted instructions dataWords = forM_ (S.toList instructions) $ \address -> do
  let instruction = A.decode address (wordAt admitted address)
  case instruction of
    A.Rejected forbidden -> Left (WalkRefused address (DecoderRejected forbidden))
    A.Branch _ _ target -> do
      unless (insideExtents target || target `elem` externalAddresses) $
        Left (WalkRefused address (BranchTargetNotAllowed target))
      when (insideExtents target && S.member target dataWords) $
        Left (WalkRefused address BranchToDataWord)
      unless (S.member target instructions || target `elem` externalAddresses) $
        Left (WalkRefused address (BranchTargetNotAllowed target))
    A.BranchExchange _ _ _ -> checkBoundary admitted address
    _ -> do
      when (isPcRelativeWriteback instruction) $
        Left (WalkRefused address PcRelativeTransferWriteback)
      case isPcRelativeLoad instruction of
        Just immediate
          | not (ownedAddress admitted (address + 8 + immediate)) ->
              Left (WalkRefused address PcRelativeLiteralOutside)
        _ -> Right ()
  when (mayFallThrough instruction && S.member (address + 4) dataWords) $
    Left (WalkRefused (address + 4) FallThroughIntoDataWord)
  when (mayFallThrough instruction && not (insideExtents (address + 4))) $
    Left (WalkRefused (address + 4) FallThroughOutsideOwned)

containingSymbol :: Admitted -> Word32 -> Maybe F.Symbol
containingSymbol admitted at =
  listToMaybe
    [ symbol
    | symbol <- admittedSymbols admitted
    , F.symbolSize symbol > 0
    , F.symbolAddress symbol <= at
    , at < F.symbolAddress symbol + F.symbolSize symbol
    ]

data Admitted = Admitted
  { admittedBin :: B.ByteString
  , admittedSymbolsBytes :: B.ByteString
  , admittedDisasm :: B.ByteString
  , admittedSectionsBytes :: B.ByteString
  , admittedSections :: [Section]
  , admittedSymbols :: [F.Symbol]
  , admittedOwnedArrays :: [(OwnedArray, Word32, Word32)]
  } deriving (Eq, Show)

-- | The byte spans the linked sections occupy, in ascending order.
ownedSpans :: Admitted -> [(Word32, Word32)]
ownedSpans admitted =
  sortOn fst [(sectionLow section, sectionLow section + fromIntegral (sectionBytes section))
             | section <- admittedSections admitted, sectionAllocated section, sectionBytes section > 0]

insideExtents :: Word32 -> Bool
insideExtents address = any (`extentContains` address) extents

binBase :: Admitted -> Word32
binBase admitted = minimum [sectionLow section | section <- admittedSections admitted, sectionAllocated section, sectionBytes section > 0]

wordAt :: Admitted -> Word32 -> Word32
wordAt admitted address =
  let offset = fromIntegral (address - binBase admitted)
      byte index = fromIntegral (B.index (admittedBin admitted) (offset + index)) :: Word32
  in if offset >= 0 && offset + 4 <= B.length (admittedBin admitted)
       then byte 0 + byte 1 * 0x100 + byte 2 * 0x10000 + byte 3 * 0x1000000
       else 0

symbolKinds :: B.ByteString -> [String]
symbolKinds bytes =
  [ C.unpack kind
  | line <- C.lines bytes
  , [_, _, kind, _] <- [C.words line]
  ]

-- | Task 43's stack is 0x1000 bytes: the creator at 0xef8c90 submits entry
-- 0xf01bc8 with @StackBytes(0x1000)@ (replacement/task_constructors/src.rs and
-- reports/replacement-inventory-baseline.md). The owned chain may use 1,024 of
-- those bytes; the stock actor, the row functions and the action bodies share
-- the rest of the same stack.
ownedStackBudget :: Int
ownedStackBudget = 1024

spWrite :: A.Instruction -> Bool
spWrite instruction = case instruction of
  A.DataProcessing _ _ _ destination _ _ -> destination == A.sp
  A.LoadMultiple _ base _ writeback _ -> base == A.sp && writeback
  A.StoreMultiple _ base _ writeback _ -> base == A.sp && writeback
  _ -> False

-- | A function's frame bytes from its prologue push and immediate sp decrements.
functionFrame :: Admitted -> F.Symbol -> Either PlacementError Int
functionFrame admitted symbol = do
  let start = F.symbolAddress symbol
      end = start + F.symbolSize symbol
      prologue = prologueWords admitted start end
      pushed = case A.decode start (wordAt admitted start) of
        A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers
          | base == A.sp -> 4 * length registers
        _ -> 0
      adjustments = sum
        [ fromIntegral immediate
        | address <- S.toList prologue
        , A.DataProcessing (A.Cond 14) A.SUB False destination base (A.Immediate immediate) <-
            [A.decode address (wordAt admitted address)]
        , destination == A.sp
        , base == A.sp
        ]
  if S.null prologue
    then if any (\address -> spWrite (A.decode address (wordAt admitted address)))
             [start, start + 4 .. end - 4]
           then Left (WalkRefused start FrameNotDerivable)
           else Right 0
    else Right (pushed + adjustments)

entrySymbol :: Admitted -> S.Set Word32 -> Word32 -> Maybe F.Symbol
entrySymbol admitted instructions target =
  listToMaybe
    [ symbol
    | symbol <- admittedSymbols admitted
    , F.symbolSize symbol > 0
    , F.symbolAddress symbol == target
    , S.member target instructions
    ]

data Frame = Frame
  { frameAddress :: Word32
  , frameBytes :: Int
  , frameCallees :: [(Word32, Word32)]
  }

functionFrames :: Admitted -> S.Set Word32 -> Either PlacementError [Frame]
functionFrames admitted instructions = forM codeSymbols $ \symbol -> do
  bytes <- functionFrame admitted symbol
  let start = F.symbolAddress symbol
      end = start + F.symbolSize symbol
      callees =
        [ (address, target)
        | address <- [start, start + 4 .. end - 4]
        , S.member address instructions
        , A.Branch _ _ target <- [A.decode address (wordAt admitted address)]
        , Just _ <- [entrySymbol admitted instructions target]
        ]
  pure (Frame start bytes callees)
  where
    codeSymbols =
      [ symbol
      | symbol <- admittedSymbols admitted
      , F.symbolSize symbol > 0
      , S.member (F.symbolAddress symbol) instructions
      ]

-- | Every owned function's frame from its prologue, no cycle in the direct call
-- graph, and every chain inside 'ownedStackBudget'.
checkStack :: Admitted -> S.Set Word32 -> Either PlacementError ()
checkStack admitted instructions = do
  frames <- functionFrames admitted instructions
  let byAddress = M.fromList [(frameAddress frame, frame) | frame <- frames]
      visit path address = case M.lookup address byAddress of
        Nothing -> Right ()
        Just frame ->
          forM_ (frameCallees frame) $ \(branchAddress, target) ->
            if target `elem` path
              then Left (WalkRefused branchAddress (RecursiveCall target))
              else visit (target : path) target
      chain path frame
        | frameAddress frame `elem` path = 0
        | otherwise =
            frameBytes frame
              + maximum
                (0 :
                  [ chain (frameAddress frame : path) callee
                  | (_, target) <- frameCallees frame
                  , Just callee <- [M.lookup target byAddress]
                  ])
  forM_ frames $ \frame -> visit [frameAddress frame] (frameAddress frame)
  forM_ frames $ \frame ->
    when (chain [] frame > ownedStackBudget) $
      Left (WalkRefused (frameAddress frame) (StackBudgetExceeded (chain [] frame)))

-- | Admit the linked image over the two declared extents.
admit :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PlacementError Admitted
admit bin symbolsBytes disasm sectionsBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolsBytes)
  sections <- parseSections sectionsBytes
  instructions <- instructionAddresses disasm
  literals <- literalAddresses disasm
  ownedArrays <- forM
    [OwnedCellCounts, OwnedActionCounts, OwnedSecondCounts, OwnedRowCounts, OwnedRowMap]
    $ \array -> do
      symbol <- either
        (const (Left (OwnedArrayMissing (ownedArraySymbol array))))
        Right
        (F.lookupSymbol symbols (ownedArraySymbol array))
      pure (array, F.symbolAddress symbol, F.symbolSize symbol)
  let admitted = Admitted bin symbolsBytes disasm sectionsBytes sections symbols ownedArrays
      ownedWords = S.fromList
        [ address
        | (low, high) <- ownedSpans admitted
        , address <- [low, low + 4 .. high - 4]
        ]
      dataWords = ownedWords `S.difference` instructions
      dataKinds = ["d", "D", "b", "B"]
  forM_ (symbolKinds symbolsBytes) $ \kind -> when (kind `elem` dataKinds) (Left (DataSymbol kind))
  forM_ [section | section <- sections, sectionAllocated section, sectionBytes section > 0] $ \section -> do
    require (not (sectionWritable section)) (WritableSection (sectionName section))
    require (not (sectionNoBits section)) (WritableSection (sectionName section))
    unless (insideExtents (sectionLow section) && insideExtents (sectionLow section + fromIntegral (sectionBytes section) - 1)) $
      Left (SectionOutside (sectionName section) (sectionLow section) (sectionBytes section))
  forM_ [symbol | symbol <- symbols, F.symbolSize symbol > 0] $ \symbol ->
    unless (insideExtents (F.symbolAddress symbol)
            && insideExtents (F.symbolAddress symbol + F.symbolSize symbol - 1)) $
      Left (SymbolOutside (F.symbolName symbol) (F.symbolAddress symbol) (F.symbolSize symbol))
  forM_ entries $ \entry -> do
    veneer <- either (const (Left (EntryMissing (entryVeneer entry)))) Right (F.lookupSymbol symbols (entryVeneer entry))
    body <- either (const (Left (EntryMissing (entryBody entry)))) Right (F.lookupSymbol symbols (entryBody entry))
    require (F.symbolAddress veneer == entryAddress entry) (EntryMoved (entryVeneer entry) (entryAddress entry) (F.symbolAddress veneer))
    require (F.symbolSize veneer == 4) (EntryVeneerSize (entryVeneer entry) (F.symbolSize veneer))
    require (insideExtents (F.symbolAddress body)) (EntryNotOwned (entryBody entry) (F.symbolAddress body))
    require (A.decode (entryAddress entry) (wordAt admitted (entryAddress entry))
             == A.Branch (A.Cond 14) False (F.symbolAddress body))
      (EntryBranchMismatch (entryVeneer entry) (entryAddress entry) (F.symbolAddress body))
  forM_ (zip [0 :: Int ..] machineLengths) $ \(index, lengths) -> do
    let wordOf array =
          case [address | (other, address, _) <- ownedArrays, other == array] of
            (address : _) -> wordAt admitted (address + 4 * fromIntegral index)
            [] -> 0
    require (wordOf OwnedCellCounts == machineCellCount lengths)
      (TableLengthMismatch "SACLAC_CELL_COUNTS" index (machineCellCount lengths) (wordOf OwnedCellCounts))
    require (wordOf OwnedActionCounts == machineActionWords lengths)
      (TableLengthMismatch "SACLAC_ACTION_COUNTS" index (machineActionWords lengths) (wordOf OwnedActionCounts))
    require (wordOf OwnedSecondCounts == machineSecondWords lengths)
      (TableLengthMismatch "SACLAC_SECOND_COUNTS" index (machineSecondWords lengths) (wordOf OwnedSecondCounts))
    require (wordOf OwnedRowCounts == machineRowCount lengths)
      (TableLengthMismatch "SACLAC_ROW_COUNTS" index (machineRowCount lengths) (wordOf OwnedRowCounts))
  checkLiterals admitted literals
  checkStores admitted instructions
  checkStack admitted instructions
  checkControl admitted instructions dataWords
  pure admitted

-- | Install one extent: section bytes stay, every other byte is the trap word.
installExtent :: Admitted -> Extent -> Either PlacementError B.ByteString
installExtent admitted extent@(Extent low _) = B.pack <$> mapM byteAt [0 .. extentBytes extent - 1]
  where
    spans = ownedSpans admitted
    covered address = any (\(spanLow, spanHigh) -> spanLow <= address && address < spanHigh) spans
    byteAt offset =
      let address = low + fromIntegral offset
      in if covered address
           then let binOffset = fromIntegral (address - binBase admitted)
                in if binOffset < B.length (admittedBin admitted)
                     then Right (B.index (admittedBin admitted) binOffset)
                     else Left (ExtentImageMismatch address offset)
           else Right (B.index trapBytes (offset `mod` 4))

-- | The canonical decoded image 5 with both extents installed.
installedImage :: B.ByteString -> Admitted -> Either PlacementError B.ByteString
installedImage image5 admitted = foldM put image5 extents
  where
    put current extent = do
      installed <- installExtent admitted extent
      let offset = (fromIntegral (extentLow extent) :: Int) - fromIntegral (0xbf1a00 :: Word32)
          count = extentBytes extent
      require (offset >= 0 && offset + count <= B.length current) (ExtentImageMismatch (extentLow extent) count)
      pure (B.take offset current <> installed <> B.drop (offset + count) current)

-- | Refuse an installed extent whose unused bytes are not the trap fill.
admitInstalled :: B.ByteString -> Admitted -> Either PlacementError ()
admitInstalled image5 admitted = forM_ extents $ \extent@(Extent low _) -> do
  let spans = ownedSpans admitted
      covered address = any (\(spanLow, spanHigh) -> spanLow <= address && address < spanHigh) spans
      offset = (fromIntegral low :: Int) - fromIntegral (0xbf1a00 :: Word32)
  require (offset >= 0 && offset + extentBytes extent <= B.length image5) (ExtentImageMismatch low (extentBytes extent))
  forM_ [0 .. extentBytes extent - 1] $ \index -> do
    let address = low + fromIntegral index
    unless (covered address) $
      require (B.index image5 (offset + index) == B.index trapBytes (index `mod` 4)) (FillDiffers address)

data Staged = Staged
  { stagedAdmitted :: Admitted
  , stagedImage5 :: B.ByteString
  , stagedDat :: B.ByteString
  , stagedCanonicalDat :: B.ByteString
  , stagedCanonicalImage5 :: B.ByteString
  } deriving (Eq, Show)

-- | Staging the canonical image reproduces the canonical DAT: the inverse check.
stageSaclac
  :: Tools.Tools -> FilePath -> B.ByteString -> B.ByteString
  -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString
  -> IO Staged
stageSaclac tools scratch dat image5 bin symbolsBytes disasm sectionsBytes = do
  admitted <- either (H.failH . show) pure (admit bin symbolsBytes disasm sectionsBytes)
  installed <- either (H.failH . show) pure (installedImage image5 admitted)
  either (H.failH . show) pure (admitInstalled installed admitted)
  scratchDirectory <- Tools.validateScratchDirectory scratch
  staged <- Record5.stageRecord5 tools scratchDirectory dat image5 installed
  inverse <- Record5.stageRecord5 tools scratchDirectory dat image5 image5
  H.ensure (Record5.stagedRecord5Dat inverse == dat) "record-5 staging is not inverse to the canonical DAT"
  H.ensure (Record5.stagedRecord5Image staged == installed) "record-5 staging carried an image other than the installed one"
  pure Staged
    { stagedAdmitted = admitted
    , stagedImage5 = installed
    , stagedDat = Record5.stagedRecord5Dat staged
    , stagedCanonicalDat = dat
    , stagedCanonicalImage5 = image5
    }

contractValue :: FilePath -> FilePath -> Staged -> Value
contractValue imagePath datPath staged = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("saclac-placement" :: String)
  , "canonical_dat_sha256" .= digest (stagedCanonicalDat staged)
  , "canonical_image5_sha256" .= digest (stagedCanonicalImage5 staged)
  , "linked_bin_sha256" .= digest (admittedBin admitted)
  , "linked_symbols_sha256" .= digest (admittedSymbolsBytes admitted)
  , "linked_disasm_sha256" .= digest (admittedDisasm admitted)
  , "linked_sections_sha256" .= digest (admittedSectionsBytes admitted)
  , "staged_image5" .= object ["path" .= imagePath, "sha256" .= digest (stagedImage5 staged)]
  , "candidate_dat" .= object ["path" .= datPath, "sha256" .= digest (stagedDat staged)]
  , "extents" .= [object
      [ "low" .= low
      , "high" .= high
      , "bytes" .= extentBytes extent
      , "owned" .= [object ["low" .= spanLow, "high" .= spanHigh] | (spanLow, spanHigh) <- ownedSpans admitted, spanLow >= low, spanHigh <= high]
      ] | extent@(Extent low high) <- extents]
  , "trap_word" .= trapWord
  , "entries" .= [object ["address" .= entryAddress entry, "veneer" .= entryVeneer entry, "body" .= entryBody entry] | entry <- entries]
  , "retained_externals" .= [object ["name" .= retainedName word, "address" .= retainedAddress word] | word <- retainedExternals]
  , "retained_data" .= [object ["name" .= retainedName word, "address" .= retainedAddress word] | word <- retainedData]
  , "owned_bytes" .= sum [sectionBytes section | section <- admittedSections admitted, sectionAllocated section]
  , "extent_bytes" .= sum (map extentBytes extents)
  ]
  where
    admitted = stagedAdmitted staged
