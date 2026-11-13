{-# LANGUAGE OverloadedStrings #-}

-- | Entry veneers stay in image 5; the body is loaded separately before handoff.
module Replacement.Saclac.Artifact
  ( Extent (..)
  , extents
  , extentBytes
  , extentContains
  , fixedExtent
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
  , StackBudget
  , stackBudget
  , checkStackArtifact
  , admit
  , ownedSpans
  , installExtent
  , fixedBody
  , installedImage
  , admitInstalled
  , Staged (..)
  , stageSaclac
  , contractValue
  , publicationsValue
  , digest
  ) where

import qualified A32 as A
import qualified ArmElf as ELF
import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (Value, object, toJSON, (.=))
import Data.Bifunctor (first)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (sortOn)
import Data.Maybe (catMaybes, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import Numeric (readHex)
import qualified Replacement.Record5Staging as Record5
import qualified Replacement.FixedImage.Layout as Fixed
import qualified Replacement.FixedImage.Record as Fixed
import qualified Replacement.Toolchain.Stack as Stack
import qualified FujiAF.UpdateContainer as U

data Extent = Extent
  { extentLow :: Word32
  , extentHigh :: Word32
  } deriving (Eq, Show)

extents :: [Extent]
extents = [Extent 0xefca6c 0xefcb88, Extent 0xefcb88 0xefd2fc, Extent 0xf01bc8 0xf01c48]

fixedExtent :: Extent
fixedExtent = Extent (Fixed.imageBase + Fixed.headerBytes) Fixed.imageLimit

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
  , Entry 0xf01bc8 "xm1_saclac_driver_entry" "xm1_saclac_driver"
  ]

-- | A stock word the owned image reaches outside its extents.
data Retained = Retained
  { retainedName :: String
  , retainedAddress :: Word32
  } deriving (Eq, Show)

retainedExternals :: [Retained]
retainedExternals =
  [ Retained "trace" 0xf46c38
  , Retained "fault" 0xf46c20
  , Retained "xm1_kernel_console9" 0x41a34
  , Retained "xm1_kernel_critical_enter" 0x7235d8
  , Retained "xm1_kernel_critical_exit" 0x7236b0
  ]

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
  | BranchTargetNotAllowed Word32
  | BranchToDataWord
  | FallThroughIntoDataWord
  | FallThroughOutsideOwned
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
  | SafetyMarkerInvalid
  | ElfRejected String
  | ReferenceOutside Word32 Word32
  | KernelBoundaryInvalid
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


-- | Direct targets stay in owned code or enter a declared retained entry.
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
    _ -> Right ()
  when (mayFallThrough instruction && S.member (address + 4) dataWords) $
    Left (WalkRefused (address + 4) FallThroughIntoDataWord)
  when (mayFallThrough instruction && not (insideExtents (address + 4))) $
    Left (WalkRefused (address + 4) FallThroughOutsideOwned)

data Admitted = Admitted
  { admittedElf :: B.ByteString
  , admittedBin :: B.ByteString
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
insideExtents address = any (`extentContains` address) (fixedExtent : extents)

binBase :: Admitted -> Word32
binBase admitted = minimum [sectionLow section | section <- admittedSections admitted, sectionAllocated section, sectionBytes section > 0]

wordAt :: Admitted -> Word32 -> Word32
wordAt admitted address =
  let offset = fromIntegral (address - binBase admitted)
      byte index = fromIntegral (B.index (admittedBin admitted) (offset + index)) :: Word32
  in if offset >= 0 && offset + 4 <= B.length (admittedBin admitted)
       then byte 0 + byte 1 * 0x100 + byte 2 * 0x10000 + byte 3 * 0x1000000
       else 0

symbolKinds :: B.ByteString -> [(Word32, String)]
symbolKinds bytes =
  [ (address, C.unpack kind)
  | line <- C.lines bytes
  , [addressText, _, kind, _] <- [C.words line]
  , Just address <- [hexWord32 (C.unpack addressText)]
  ]

-- | Task 43's stack is 0x1000 bytes: the creator at 0xef8c90 submits entry
-- 0xf01bc8 with @StackBytes(0x1000)@ (replacement/task_constructors/src.rs and
-- reports/replacement-inventory-baseline.md). The owned chain may use 1,024 of
-- those bytes; the stock actor, the row functions and the action bodies share
-- the rest of the same stack.
ownedStackBudget :: Int
ownedStackBudget = 1024

type StackBudget = Stack.StackBytes

stackBudget :: Int -> Maybe StackBudget
stackBudget bytes = if bytes > 0 then Stack.stackBytes bytes else Nothing

checkStackArtifact :: StackBudget -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PlacementError ()
checkStackArtifact budget bin symbolsBytes disasm sectionsBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolsBytes)
  sections <- parseSections sectionsBytes
  instructions <- instructionAddresses disasm
  require (any (\section -> sectionAllocated section && sectionBytes section > 0) sections) SectionsRejected
  let artifact = Admitted B.empty bin symbolsBytes disasm sectionsBytes sections symbols []
      begin = binBase artifact
  forM_ (S.toList instructions) $ \address ->
    require (address >= begin && toInteger address + 4 <= toInteger begin + toInteger (B.length bin))
      (ExtentImageMismatch address 4)
  checkStack budget artifact instructions

checkStack :: StackBudget -> Admitted -> S.Set Word32 -> Either PlacementError ()
checkStack budget admitted instructions = first failure (Stack.check budget image)
  where
    image = Stack.Image (Stack.Guest (binBase admitted)) (admittedBin admitted)
      (admittedSymbols admitted) (S.map Stack.Guest instructions)
    failure problem = case problem of
      Stack.OutsideImage (Stack.Guest address) -> ExtentImageMismatch address 4
      Stack.InvalidFunction (Stack.Guest address) -> WalkRefused address FrameNotDerivable
      Stack.FrameNotDerivable (Stack.Guest address) -> WalkRefused address FrameNotDerivable
      Stack.RecursiveCall (Stack.Guest address) (Stack.Guest target) -> WalkRefused address (RecursiveCall target)
      Stack.BudgetExceeded (Stack.Guest address) bytes -> WalkRefused address (StackBudgetExceeded (Stack.stackByteCount bytes))

-- | Admit layout, direct control-flow targets and the owned stack budget.
admit :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PlacementError Admitted
admit elfBytes bin symbolsBytes disasm sectionsBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolsBytes)
  sections <- parseSections sectionsBytes
  instructions <- instructionAddresses disasm
  ownedArrays <- forM
    [OwnedCellCounts, OwnedActionCounts, OwnedSecondCounts, OwnedRowCounts, OwnedRowMap]
    $ \array -> do
      symbol <- either
        (const (Left (OwnedArrayMissing (ownedArraySymbol array))))
        Right
        (F.lookupSymbol symbols (ownedArraySymbol array))
      pure (array, F.symbolAddress symbol, F.symbolSize symbol)
  let admitted = Admitted elfBytes bin symbolsBytes disasm sectionsBytes sections symbols ownedArrays
      ownedWords = S.fromList
        [ address
        | (low, high) <- ownedSpans admitted
        , address <- [low, low + 4 .. high - 4]
        ]
      dataWords = ownedWords `S.difference` instructions
      dataKinds = ["d", "D", "b", "B"]
  forM_ (symbolKinds symbolsBytes) $ \(address, kind) ->
    when (kind `elem` dataKinds && not (extentContains fixedExtent address)) (Left (DataSymbol kind))
  forM_ [section | section <- sections, sectionAllocated section, sectionBytes section > 0] $ \section -> do
    let inFixed = extentContains fixedExtent (sectionLow section)
          && extentContains fixedExtent (sectionLow section + fromIntegral (sectionBytes section) - 1)
    require (not (sectionWritable section) || inFixed) (WritableSection (sectionName section))
    require (not (sectionNoBits section) || (inFixed && sectionName section == ".bss")) (WritableSection (sectionName section))
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
    require (extentContains fixedExtent (F.symbolAddress body)) (EntryNotOwned (entryBody entry) (F.symbolAddress body))
    require (A.decode (entryAddress entry) (wordAt admitted (entryAddress entry))
             == A.Branch (A.Cond 14) False (F.symbolAddress body))
      (EntryBranchMismatch (entryVeneer entry) (entryAddress entry) (F.symbolAddress body))
  marker <- either (const (Left SafetyMarkerInvalid)) Right (F.lookupSymbol symbols "XM1_REQUIRES_LENS_SAFETY_CONTRACT")
  require (F.symbolSize marker == 4 && wordAt admitted (F.symbolAddress marker) == 1) SafetyMarkerInvalid
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
  budget <- maybe (Left SectionsRejected) Right (stackBudget ownedStackBudget)
  checkStack budget admitted instructions
  checkReferences elfBytes admitted
  checkControl admitted instructions dataWords
  pure admitted

checkReferences :: B.ByteString -> Admitted -> Either PlacementError ()
checkReferences elfBytes admitted = do
  elf <- first ElfRejected (ELF.parseElf elfBytes)
  references <- first ElfRejected (ELF.addressRelocations elf)
  boundary <- either (const (Left KernelBoundaryInvalid)) Right
    (F.lookupSymbol (admittedSymbols admitted) "XM1_KERNEL_BOUNDARY")
  let begin = F.symbolAddress boundary
      at index = wordAt admitted (begin + 4 * index)
      windows = at 1
      entryCount = at 2
  require (at 0 == 0x314e524b && windows <= 16 && entryCount <= 32
    && F.symbolSize boundary == 4 * (3 + 2 * windows + entryCount)) KernelBoundaryInvalid
  let ranges = [(at (3 + 2 * index), at (4 + 2 * index)) | index <- take (fromIntegral windows) [0 ..]]
      retained address = address `elem` externalAddresses
        || any (\(_, _, low, bytes) -> low <= address && toInteger address < toInteger low + toInteger bytes) stockArrays
        || any (\(low, high) -> low <= address && address < high) ranges
  forM_ references $ \reference -> do
    let site = ELF.addressSite reference
        target = ELF.addressTarget reference
        owned address = any (\(low, high) -> low <= address && address < high) (ownedSpans admitted)
    require (wordAt admitted site == maybe 0 id (ELF.wordAt elf site)) (ExtentImageMismatch site 4)
    require (owned target || retained target) (ReferenceOutside site target)

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

fixedBody :: Admitted -> Either PlacementError B.ByteString
fixedBody admitted = do
  let Extent begin _ = fixedExtent
      sections = [section | section <- admittedSections admitted, sectionAllocated section,
        not (sectionNoBits section), sectionBytes section > 0, extentContains fixedExtent (sectionLow section)]
      end = maximum (begin : [sectionLow section + fromIntegral (sectionBytes section) | section <- sections])
      count = fromIntegral (end - begin)
      offset = fromIntegral (begin - binBase admitted)
  require (count > 0 && offset + count <= B.length (admittedBin admitted)) (ExtentImageMismatch begin count)
  pure (B.take count (B.drop offset (admittedBin admitted)))

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
  , stagedFixedBody :: B.ByteString
  , stagedPublications :: [Fixed.Publication]
  } deriving (Eq, Show)

-- | Staging the canonical image reproduces the canonical DAT: the inverse check.
stageSaclac
  :: Tools.Tools -> FilePath -> B.ByteString -> B.ByteString
  -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString
  -> IO Staged
stageSaclac tools scratch dat image5 elf bin symbolsBytes disasm sectionsBytes = do
  admitted <- either (H.failH . show) pure (admit elf bin symbolsBytes disasm sectionsBytes)
  installed <- either (H.failH . show) pure (installedImage image5 admitted)
  either (H.failH . show) pure (admitInstalled installed admitted)
  body <- either (H.failH . show) pure (fixedBody admitted)
  publications <- fmap concat $ forM extents $ \(Extent low high) ->
    fmap catMaybes $ forM [low, low + 4 .. high - 4] $ \address -> do
      let offset = fromIntegral (address - 0xbf1a00)
          old = U.word32At image5 offset
          new = U.word32At installed offset
      if old == new then pure Nothing
        else Just <$> either H.failH pure (Fixed.publication address old new)
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
    , stagedFixedBody = body
    , stagedPublications = publications
    }

contractValue :: FilePath -> FilePath -> Staged -> Value
contractValue imagePath datPath staged = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("saclac-placement" :: String)
  , "canonical_dat_sha256" .= digest (stagedCanonicalDat staged)
  , "canonical_image5_sha256" .= digest (stagedCanonicalImage5 staged)
  , "linked_bin_sha256" .= digest (admittedBin admitted)
  , "linked_elf_sha256" .= digest (admittedElf admitted)
  , "linked_symbols_sha256" .= digest (admittedSymbolsBytes admitted)
  , "linked_disasm_sha256" .= digest (admittedDisasm admitted)
  , "linked_sections_sha256" .= digest (admittedSectionsBytes admitted)
  , "fixed_body" .= object ["address" .= (Fixed.imageBase + Fixed.headerBytes), "bytes" .= B.length (stagedFixedBody staged), "sha256" .= digest (stagedFixedBody staged)]
  , "fixed_body_loaded" .= False
  , "lens_safety_contract_required" .= True
  , "publications" .= publicationsValue staged
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

publicationsValue :: Staged -> Value
publicationsValue staged = toJSON
  [ object ["address" .= address, "old" .= old, "new" .= new]
  | Fixed.Publication _ address old new <- stagedPublications staged
  ]
