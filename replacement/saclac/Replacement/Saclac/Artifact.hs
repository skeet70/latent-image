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
  , loadingEntries
  , Retained (..)
  , retainedExternals
  , retainedData
  , unitImports
  , retainedRegions
  , taskStackRuntime
  , taskStackBytes
  , taskCreateSite
  , taskCreateWord
  , trapWord
  , trapBytes
  , Section (..)
  , parseSections
  , PlacementError (..)
  , WalkReason (..)
  , Admitted (..)
  , bankStackEntries
  , admit
  , wordAt
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
import Data.Bits ((.&.), (.|.))
import Data.List (isPrefixOf, nub, sortOn)
import Data.Maybe (catMaybes, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import Numeric (readHex)
import qualified Replacement.Record5Staging as Record5
import qualified Replacement.DisplayScanout.Artifact as Scanout
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.FixedImage.Layout as Fixed
import qualified Replacement.FixedImage.Record as Fixed
import qualified StackWalk as W
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

decoderEntries :: [(Word32, String)]
decoderEntries =
  [ (0x6ae360, "xm1_decoder_init")
  , (0x6ae500, "xm1_decoder_feed")
  , (0x6ae1dc, "xm1_decoder_progress")
  , (0x6ae310, "xm1_decoder_complete")
  , (0x6ae3f4, "xm1_decoder_finish")
  ]

-- | The stock task60 constructor, request and cancel wrappers.
loadingEntries :: [(Word32, String)]
loadingEntries =
  [ (0x73ac74, "xm1_loading_startup")
  , (0x73ac94, "xm1_loading_legacy_request")
  , (0x73aca4, "xm1_loading_legacy_cancel")
  ]

-- | The receiver task's stock-created stack (`0x725dd8` descriptor word 3).
loadingTaskStackBytes :: Int
loadingTaskStackBytes = 0xc00

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
  , Retained "__saclac_runtime_malloc" 0x89cda4
  , Retained "__saclac_runtime_create_task" 0x725dd8
  , Retained "xm1_decoder_current_task" 0x723cb8
  , Retained "xm1_decoder_take" 0x724b68
  , Retained "xm1_decoder_give" 0x724c00
  , Retained "xm1_decoder_route" 0x6499d0
  , Retained "xm1_decoder_clock_take" 0x649c4c
  , Retained "xm1_decoder_clock_give" 0x649ddc
  , Retained "xm1_decoder_irq_attach" 0x64e918
  , Retained "xm1_decoder_irq_detach" 0x64eb20
  , Retained "xm1_decoder_marker" 0x42188
  , Retained "xm1_decoder_progress_entry" 0x6ae1dc
  , Retained "xm1_decoder_complete_entry" 0x6ae310
  ] ++ loadingRetained

-- | Source-inspected X-M1 1.50 leaves the task60 owner and its NAND port call.
loadingRetained :: [Retained]
loadingRetained =
  [ Retained "xm1_loading_create_channel" 0x724cdc
  , Retained "xm1_loading_create_task" 0x725dd8
  , Retained "xm1_loading_send_message" 0x72503c
  , Retained "xm1_loading_receive_message" 0x725318
  , Retained "xm1_loading_current_task" 0x723cb8
  , Retained "xm1_loading_wait_reserved" 0x724b68
  , Retained "xm1_loading_signal_reserved" 0x724c00
  , Retained "xm1_loading_segment" 0x657114
  , Retained "xm1_loading_extents" 0x6571a0
  , Retained "xm1_loading_mode" 0x657184
  , Retained "xm1_loading_plain_progress" 0x73b924
  , Retained "xm1_loading_take" 0x724b68
  , Retained "xm1_loading_give" 0x724c00
  , Retained "xm1_loading_delay" 0x723ce8
  , Retained "xm1_loading_cache" 0x42634
  , Retained "xm1_loading_action1" 0x6aa5b4
  , Retained "xm1_loading_action2" 0x6aa5e4
  , Retained "xm1_loading_section3" 0x6aa6f8
  , Retained "xm1_loading_selected" 0x6aa63c
  , Retained "xm1_loading_region" 0x6aa750
  , Retained "xm1_loading_initialize_storage" 0x69f920
  , Retained "xm1_loading_register_completion" 0x6c1444
  , Retained "xm1_loading_auxiliary" 0x6aa838
  , Retained "xm1_loading_fill_request" 0x66609c
  , Retained "xm1_loading_fill_done" 0x6a8bec
  , Retained "xm1_loading_fill_flag" 0x89f8ac
  , Retained "xm1_lower_read_clean_invalidate" 0x42634
  , Retained "xm1_lower_read_clock_mask" 0x42560
  , Retained "xm1_lower_read_clock_restore" 0x42568
  ]

-- | The retained services the file service and the display scanout call.
unitImports :: [Retained]
unitImports =
  [ Retained "file_service_semaphore_wait" 0x724b68
  , Retained "file_service_semaphore_signal" 0x724c00
  , Retained "file_service_cache_claim" 0x71e960
  , Retained "file_service_current_task" 0x723cb8
  , Retained "file_service_local_record" 0x7111c4
  , Retained "file_service_local_position" 0x7111e0
  , Retained "file_service_local_open" 0x711200
  , Retained "file_service_device_for_letter" 0x70ec84
  , Retained "file_service_copy_string" 0x70e748
  , Retained "file_service_copy_bytes" 0x722054
  , Retained "file_service_media_report" 0x726c74
  , Retained "file_service_send" 0x72503c
  , Retained "file_service_receive_request" 0x71fe2c
  , Retained "file_service_sleep" 0x7240c8
  , Retained "file_service_wake" 0x724144
  , Retained "file_service_ticks" 0x7257a8
  , Retained "file_service_cache_release" 0x71e9ec
  , Retained "file_service_parse_path" 0x7160b8
  , Retained "file_service_directory_slot" 0x715f0c
  , Retained "file_service_split_path" 0x71543c
  , Retained "file_service_directory_number" 0x712dac
  , Retained "file_service_make_directory" 0x716184
  , Retained "file_service_fs_open" 0x701634
  , Retained "file_service_register_written" 0x71565c
  , Retained "file_service_fs_write" 0x6ff000
  , Retained "file_service_fs_seek" 0x6ff138
  , Retained "file_service_fs_close" 0x6ffff4
  , Retained "file_service_fs_close_named" 0x6ffdcc
  , Retained "file_service_fs_close_entry" 0x6ffb10
  , Retained "file_service_fs_release" 0x6ffc68
  , Retained "file_service_media_refresh" 0x71d66c
  , Retained "file_service_after_written_close" 0x71141c
  , Retained "file_service_rescan" 0x71985c
  , Retained "scanout_enter_critical" 0x7235d8
  , Retained "scanout_leave_critical" 0x7236b0
  , Retained "scanout_yield" 0x723ce8
  , Retained "scanout_irq_configure" 0x64e918
  , Retained "scanout_irq_release" 0x64eb20
  ]

externalAddresses :: [Word32]
externalAddresses = map retainedAddress (retainedExternals ++ unitImports)

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
retainedData = [Retained name address | (_, name, address, _) <- stockArrays] ++ [taskStackRuntime]

-- | The stock data the owned image reads and writes: name, address, bytes.
retainedRegions :: [(String, Word32, Word32)]
retainedRegions = [(name, address, bytes) | (_, name, address, bytes) <- stockArrays]
  ++ [(retainedName taskStackRuntime, retainedAddress taskStackRuntime, 8)]

taskStackRuntime :: Retained
taskStackRuntime = Retained "__saclac_runtime_stack_words" 0x40a5b354

taskStackBytes, taskCreateSite, taskCreateWord :: Word32
taskStackBytes = 0xb000
taskCreateSite = 0xef8d1c
taskCreateWord = 0xebe0b42d

constructorStackBytes, constructorOwnedStackBudget :: Int
constructorStackBytes = 0xc00
constructorOwnedStackBudget = constructorStackBytes - 0x60 - 72 - max 128 104 - 88


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
  deriving (Eq, Show)

data PlacementError
  = SymbolsRejected
  | SectionsRejected
  | SectionsDifferFromElf
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
  | TaskStackContractInvalid
  | TaskCreatePreimage Word32
  | PublishedWordInvalid Word32 Word32
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
compilerTrapWord :: Word32
compilerTrapWord = 0xe7ffdefe


-- | Direct targets stay in owned code or enter a declared retained or loader entry.
checkControl :: S.Set Word32 -> Admitted -> S.Set Word32 -> S.Set Word32 -> Either PlacementError ()
checkControl loaderEntries admitted instructions dataWords = forM_ (S.toList instructions) $ \address -> do
  let word = wordAt admitted address
      instruction = A.decode address word
      external target = target `elem` externalAddresses || S.member target loaderEntries
  case instruction of
    A.Rejected A.UndefinedEncoding | word == compilerTrapWord -> Right ()
    A.Rejected forbidden -> case W.classify W.Strict address word of
      W.Refuse _ -> Left (WalkRefused address (DecoderRejected forbidden))
      W.Stop -> Left (WalkRefused address (DecoderRejected forbidden))
      _ -> Right ()
    A.Branch _ _ target -> do
      unless (insideExtents target || external target) $
        Left (WalkRefused address (BranchTargetNotAllowed target))
      when (insideExtents target && S.member target dataWords) $
        Left (WalkRefused address BranchToDataWord)
      unless (S.member target instructions || external target) $
        Left (WalkRefused address (BranchTargetNotAllowed target))
    _ -> Right ()
  when (word /= compilerTrapWord && mayFallThrough instruction && S.member (address + 4) dataWords) $
    Left (WalkRefused (address + 4) FallThroughIntoDataWord)
  when (word /= compilerTrapWord && mayFallThrough instruction && not (insideExtents (address + 4))) $
    Left (WalkRefused (address + 4) FallThroughOutsideOwned)

-- | The stock entries of both loaders whose bodies stay in the loader carriers: the fixed-bank
-- family bodies call their own domain's services there.
loaderEntryAddresses :: Agg.CarrierUnion -> S.Set Word32
loaderEntryAddresses union = S.fromList
  [ Agg.carrierEntryAddress entry
  | entry <- Agg.carrierEntriesA union ++ Agg.carrierEntriesB union
  , Agg.carrierEntryBody entry == Agg.LoaderBody
  ]

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

-- | Image 4's load range.
image4Extent :: Extent
image4Extent = Extent 0x61c400 (0x61c400 + 0x282800)

-- | A read-only section inside image 4 publishes over retained stock code: a bank unit's entry
-- branches into owned fixed code and trap fill elsewhere.
publishedSection :: Section -> Bool
publishedSection section = sectionAllocated section && sectionBytes section > 0
  && not (sectionWritable section) && not (sectionNoBits section)
  && extentContains image4Extent (sectionLow section)
  && toInteger (sectionLow section) + toInteger (sectionBytes section) <= toInteger (extentHigh image4Extent)

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

-- | Task43 receives the linked stack contract before its retained creation call.
-- The retained reserve is separate from owned demand, not a callee-depth proof.
ownedStackBudget :: Word32
ownedStackBudget = taskStackBytes - 4096

-- | Owned stack bytes a scanout or file-service entry may use.
unitStackBudget :: Word32
unitStackBudget = 2048

-- | Every published bank entry and task body with the owned stack bytes check-fixed-image allows
-- it. Root's task entries and every other published Root or family entry take task43's owned
-- budget; the constructor and the loading task take their stock-created stacks' owned budgets.
bankStackEntries :: Agg.CarrierUnion -> Admitted -> Either PlacementError [(String, Word32)]
bankStackEntries union admitted = do
  fileService <- forM [veneer | veneer <- symbols, "xm1_file_service_entry_" `isPrefixOf` F.symbolName veneer] $ \veneer -> do
    let at = F.symbolAddress veneer
    case A.decode at (wordAt admitted at) of
      A.Branch (A.Cond 14) False target
        | [body] <- nub [F.symbolName symbol | symbol <- symbols, F.symbolAddress symbol == target, F.symbolSize symbol > 0] ->
            Right (body, unitStackBudget)
      _ -> Left (EntryNotOwned (F.symbolName veneer) at)
  pure $
    [(entryBody entry, ownedStackBudget) | entry <- entries]
    ++ [(name, ownedStackBudget) | (_, name) <- decoderEntries ++ loadingEntries]
    ++ [ (Agg.carrierEntrySymbol entry, ownedStackBudget)
       | entry <- Agg.carrierEntriesA union ++ Agg.carrierEntriesB union, Agg.carrierEntryBody entry == Agg.BankBody ]
    ++ [("xm1_saclac_create_task", fromIntegral constructorOwnedStackBudget), ("xm1_loading_task", fromIntegral loadingTaskStackBytes)]
    ++ [(name, unitStackBudget) | Scanout.Entry name _ <- Scanout.entries]
    ++ fileService
  where
    symbols = admittedSymbols admitted

-- | Admit layout and direct control-flow targets.
admit :: Agg.CarrierUnion -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PlacementError Admitted
admit union elfBytes bin symbolsBytes disasm sectionsBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolsBytes)
  sections <- parseSections sectionsBytes
  linkedInstructions <- instructionAddresses disasm
  ownedArrays <- forM
    [OwnedCellCounts, OwnedActionCounts, OwnedSecondCounts, OwnedRowCounts, OwnedRowMap]
    $ \array -> do
      symbol <- either
        (const (Left (OwnedArrayMissing (ownedArraySymbol array))))
        Right
        (F.lookupSymbol symbols (ownedArraySymbol array))
      pure (array, F.symbolAddress symbol, F.symbolSize symbol)
  let admitted = Admitted elfBytes bin symbolsBytes disasm sectionsBytes sections symbols ownedArrays
      published = filter publishedSection sections
      inPublished address = any (\section -> sectionLow section <= address
        && toInteger address < toInteger (sectionLow section) + toInteger (sectionBytes section)) published
      instructions = S.filter (not . inPublished) linkedInstructions
      ownedWords = S.fromList
        [ address
        | (low, high) <- ownedSpans admitted
        , address <- [low, low + 4 .. high - 4]
        ]
      dataWords = ownedWords `S.difference` instructions
      dataKinds = ["d", "D", "b", "B"]
  forM_ (symbolKinds symbolsBytes) $ \(address, kind) ->
    when (kind `elem` dataKinds && not (extentContains fixedExtent address)) (Left (DataSymbol kind))
  forM_ [section | section <- sections, sectionAllocated section, sectionBytes section > 0, not (publishedSection section)] $ \section -> do
    let inFixed = extentContains fixedExtent (sectionLow section)
          && extentContains fixedExtent (sectionLow section + fromIntegral (sectionBytes section) - 1)
    require (not (sectionWritable section) || inFixed) (WritableSection (sectionName section))
    require (not (sectionNoBits section) || (inFixed && sectionName section == ".bss")) (WritableSection (sectionName section))
    unless (insideExtents (sectionLow section) && insideExtents (sectionLow section + fromIntegral (sectionBytes section) - 1)) $
      Left (SectionOutside (sectionName section) (sectionLow section) (sectionBytes section))
  elf <- first ElfRejected (ELF.parseElf elfBytes)
  require (sections == [ Section (ELF.sectionName section) (ELF.sectionAddress section) (fromIntegral (ELF.sectionSize section))
                           (ELF.sectionAllocated section) (ELF.sectionWritable section) (ELF.sectionType section == 8)
                       | section <- ELF.elfSections elf, ELF.sectionType section `elem` [1, 8] ])
    SectionsDifferFromElf
  forM_ published $ \section -> do
    let low = sectionLow section
    require (low `mod` 4 == 0 && sectionBytes section `mod` 4 == 0) (SectionOutside (sectionName section) low (sectionBytes section))
    forM_ [low, low + 4 .. low + fromIntegral (sectionBytes section) - 4] $ \address -> do
      let word = wordAt admitted address
      unless (word == trapWord) $ case A.decode address word of
        A.Branch (A.Cond 14) False target | extentContains fixedExtent target && S.member target instructions -> pure ()
        _ -> Left (PublishedWordInvalid address word)
  forM_ [symbol | symbol <- symbols, F.symbolSize symbol > 0] $ \symbol ->
    unless ((insideExtents (F.symbolAddress symbol)
             && insideExtents (F.symbolAddress symbol + F.symbolSize symbol - 1))
            || (inPublished (F.symbolAddress symbol) && inPublished (F.symbolAddress symbol + F.symbolSize symbol - 1))) $
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
  checkTaskStackContract admitted
  checkReferences (loaderEntryAddresses union) elfBytes admitted
  checkControl (loaderEntryAddresses union) admitted instructions dataWords
  pure admitted

checkTaskStackContract :: Admitted -> Either PlacementError ()
checkTaskStackContract admitted = do
  metadata <- either (const (Left TaskStackContractInvalid)) Right
    (F.lookupSymbol (admittedSymbols admitted) "XM1_SACLAC_TASK_STACK")
  constructor <- either (const (Left TaskStackContractInvalid)) Right
    (F.lookupSymbol (admittedSymbols admitted) "XM1_SACLAC_CONSTRUCTOR_STACK")
  body <- either (const (Left TaskStackContractInvalid)) Right
    (F.lookupSymbol (admittedSymbols admitted) "xm1_saclac_create_task")
  let begin = F.symbolAddress metadata
      constructorBegin = F.symbolAddress constructor
      allocatedReadOnly :: Word32 -> Word32 -> Bool
      allocatedReadOnly low bytes = any (\section -> sectionAllocated section
        && not (sectionWritable section) && not (sectionNoBits section)
        && sectionLow section <= low
        && toInteger low + toInteger bytes <= toInteger (sectionLow section) + toInteger (sectionBytes section))
        (admittedSections admitted)
  require (F.symbolSize metadata == 24 && begin `mod` 4 == 0
    && allocatedReadOnly begin 24
    && map (wordAt admitted . (begin +)) [0, 4 .. 20]
       == [0x31545354, 43, taskStackBytes, retainedAddress taskStackRuntime, 0xa71a00, Fixed.imageBase]
    && F.symbolSize constructor == 56 && constructorBegin `mod` 4 == 0
    && allocatedReadOnly constructorBegin 56
    && map (wordAt admitted . (constructorBegin +)) [0, 4 .. 52]
       == [0x31545343, 59, fromIntegral constructorStackBytes, 0xa24a00, 0xa255a0,
           0x60, 72, 128, 104, 88, fromIntegral constructorOwnedStackBudget,
           0xd985a4, 0xef8c90, taskCreateSite]
    && F.symbolSize body > 0 && F.symbolAddress body `mod` 4 == 0
    && allocatedReadOnly (F.symbolAddress body) (F.symbolSize body)
    && any (\(address, kind) -> address == F.symbolAddress body && kind `elem` ["T", "t"])
       (symbolKinds (admittedSymbolsBytes admitted))) TaskStackContractInvalid

taskCreateBranch :: Admitted -> Either PlacementError Word32
taskCreateBranch admitted = do
  body <- either (const (Left TaskStackContractInvalid)) Right
    (F.lookupSymbol (admittedSymbols admitted) "xm1_saclac_create_task")
  let delta = toInteger (F.symbolAddress body) - toInteger taskCreateSite - 8
  require (delta `mod` 4 == 0 && delta >= -0x2000000 && delta < 0x2000000) TaskStackContractInvalid
  pure (0xeb000000 .|. (fromIntegral (delta `div` 4) .&. 0xffffff))

checkReferences :: S.Set Word32 -> B.ByteString -> Admitted -> Either PlacementError ()
checkReferences loaderEntries elfBytes admitted = do
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
      retained address = address `elem` externalAddresses || S.member address loaderEntries
        || any (\(_, _, low, bytes) -> low <= address && toInteger address < toInteger low + toInteger bytes) stockArrays
        || any (\(low, high) -> low <= address && address < high) ranges
      metadataBoundary reference symbol = ELF.addressKind reference == ELF.DataReference
        && ELF.symbolName symbol == "__power_cpu_end"
        && ELF.symbolSize symbol == 0
        && ELF.symbolKind symbol == ELF.OtherSymbol
        && case [section | section <- admittedSections admitted, sectionAllocated section,
                  sectionNoBits section, sectionName section == ".bss"] of
          [section] ->
            let end = toInteger (sectionLow section) + toInteger (sectionBytes section)
                matchingBss =
                  [index | (index, linked) <- zip [0 ..] (ELF.elfSections elf),
                    ELF.sectionName linked == ".bss", ELF.sectionAllocated linked,
                    ELF.sectionType linked == 8, ELF.sectionAddress linked == sectionLow section,
                    toInteger (ELF.sectionSize linked) == toInteger (sectionBytes section)]
            in sectionBytes section >= 0
              && extentContains fixedExtent (sectionLow section)
              && end <= toInteger (extentHigh fixedExtent)
              && toInteger (ELF.symbolValue symbol) == end
              && toInteger (ELF.addressTarget reference) == end
              && case matchingBss of
                [index] -> ELF.symbolSection symbol `elem` [ELF.Absolute, ELF.InSection index]
                _ -> False
          _ -> False
  forM_ references $ \reference -> do
    let symbol = ELF.addressSymbol reference
        site = ELF.addressSite reference
        target = ELF.addressTarget reference
        owned address = any (\(low, high) -> low <= address && address < high) (ownedSpans admitted)
    require (wordAt admitted site == maybe 0 id (ELF.wordAt elf site)) (ExtentImageMismatch site 4)
    let stackRuntime = ELF.addressKind reference == ELF.DataReference
          && retainedAddress taskStackRuntime <= target
          && toInteger target < toInteger (retainedAddress taskStackRuntime) + 8
    require (owned target || retained target || stackRuntime || metadataBoundary reference symbol) (ReferenceOutside site target)

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
installedImage image5 admitted = do
  let offset = fromIntegral (taskCreateSite - 0xbf1a00)
  require (offset + 4 <= B.length image5) (ExtentImageMismatch taskCreateSite 4)
  let previous = U.word32At image5 offset
  require (previous == taskCreateWord) (TaskCreatePreimage previous)
  branch <- taskCreateBranch admitted
  let prepared = B.take offset image5 <> U.putWord32 branch <> B.drop (offset + 4) image5
  foldM put prepared extents
  where
    put current extent = do
      installed <- installExtent admitted extent
      let offset = (fromIntegral (extentLow extent) :: Int) - fromIntegral (0xbf1a00 :: Word32)
          count = extentBytes extent
      require (offset >= 0 && offset + count <= B.length current) (ExtentImageMismatch (extentLow extent) count)
      pure (B.take offset current <> installed <> B.drop (offset + count) current)

-- | Refuse an installed extent whose unused bytes are not the trap fill.
admitInstalled :: B.ByteString -> Admitted -> Either PlacementError ()
admitInstalled image5 admitted = do
  branch <- taskCreateBranch admitted
  require (U.word32At image5 (fromIntegral (taskCreateSite - 0xbf1a00)) == branch) (ExtentImageMismatch taskCreateSite 4)
  forM_ extents $ \extent@(Extent low _) -> do
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
  , stagedCanonicalImage4 :: B.ByteString
  , stagedCanonicalImage5 :: B.ByteString
  , stagedFixedBody :: B.ByteString
  , stagedPublications :: [Fixed.Publication]
  } deriving (Eq, Show)

decoderPublications :: B.ByteString -> Admitted -> IO [Fixed.Publication]
decoderPublications image4 admitted = do
  H.ensure (B.length image4 == 0x282800
    && digest image4 == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    "canonical image4 differs"
  forM (decoderEntries ++ loadingEntries) $ \(address, name) -> do
    target <- either H.failH pure (F.symbolAddress <$> F.lookupSymbol (admittedSymbols admitted) name)
    H.ensure (target `mod` 4 == 0 && extentContains fixedExtent target) ("image4 entry leaves fixed body: " ++ name)
    let delta = toInteger target - toInteger address - 8
    H.ensure (delta `mod` 4 == 0 && delta >= -0x2000000 && delta < 0x2000000) ("image4 entry leaves A32 branch range: " ++ name)
    let old = U.word32At image4 (fromIntegral (address - 0x61c400))
        new = 0xea000000 .|. (fromInteger (delta `div` 4) .&. 0xffffff)
    either H.failH pure (Fixed.publication address old new)

-- | The display scanout's stock-entry branches, when the bank links the scanout; a partial
-- scanout is refused.
scanoutEntries :: B.ByteString -> Admitted -> IO [Fixed.Publication]
scanoutEntries image4 admitted =
  case [name | Scanout.Entry name _ <- Scanout.entries, Left _ <- [F.lookupSymbol (admittedSymbols admitted) name]] of
    [] -> do
      rows <- either (H.failH . show) pure (Scanout.canonicalPublications image4 (admittedSymbolsBytes admitted))
      forM rows $ \(Scanout.Publication address old new) -> either H.failH pure (Fixed.publication address old new)
    missing | length missing == length Scanout.entries -> pure []
    missing -> H.failH ("partial display scanout: " ++ unwords missing)

-- | Staging the canonical image reproduces the canonical DAT: the inverse check.
stageSaclac
  :: Tools.Tools -> FilePath -> B.ByteString -> B.ByteString -> B.ByteString
  -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Agg.CarrierUnion
  -> IO Staged
stageSaclac tools scratch dat image5 image4 elf bin symbolsBytes disasm sectionsBytes union = do
  admitted <- either (H.failH . show) pure (admit union elf bin symbolsBytes disasm sectionsBytes)
  installed <- either (H.failH . show) pure (installedImage image5 admitted)
  either (H.failH . show) pure (admitInstalled installed admitted)
  body <- either (H.failH . show) pure (fixedBody admitted)
  let sites = taskCreateSite : concat [[low, low + 4 .. high - 4] | Extent low high <- extents]
  image5Publications <- fmap catMaybes $ forM sites $ \address -> do
    let offset = fromIntegral (address - 0xbf1a00)
        old = U.word32At image5 offset
        new = U.word32At installed offset
    if old == new then pure Nothing
      else Just <$> either H.failH pure (Fixed.publication address old new)
  image4Publications <- decoderPublications image4 admitted
  unitPublications <- fmap catMaybes $ forM
    [ address
    | section <- admittedSections admitted, publishedSection section
    , address <- [sectionLow section, sectionLow section + 4 .. sectionLow section + fromIntegral (sectionBytes section) - 4]
    ] $ \address -> do
      let old = U.word32At image4 (fromIntegral (address - extentLow image4Extent))
          new = wordAt admitted address
      if old == new then pure Nothing
        else Just <$> either H.failH pure (Fixed.publication address old new)
  scanoutPublications <- scanoutEntries image4 admitted
  let publications = image5Publications ++ image4Publications ++ unitPublications ++ scanoutPublications
      addresses = [address | Fixed.Publication _ address _ _ <- publications]
  H.ensure (S.size (S.fromList addresses) == length addresses) "two publications name one word"
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
    , stagedCanonicalImage4 = image4
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
  , "canonical_image4_sha256" .= digest (stagedCanonicalImage4 staged)
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
  , "decoder_entries" .= [object ["address" .= address, "symbol" .= name] | (address, name) <- decoderEntries]
  , "loading_entries" .= [object ["address" .= address, "symbol" .= name] | (address, name) <- loadingEntries]
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
