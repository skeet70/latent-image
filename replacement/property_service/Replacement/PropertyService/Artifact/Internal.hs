{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Admission of the linked owned property service and its staging into image4.
module Replacement.PropertyService.Artifact.Internal
  ( CanonicalSource
  , LinkedArtifact
  , ArtifactInput (..)
  , ArtifactPurpose (..)
  , StagedImage4
  , StagedCandidate
  , StagedPlacement (..)
  , ArtifactError (..)
  , EntrySide (..)
  , StorageWidth (..)
  , DefaultImage (..)
  , EncodingPart (..)
  , canonicalSource
  , sourceProperties
  , sourceHull
  , deriveArtifact
  , deriveRecipeArtifactInternal
  , RecipeDependency (..)
  , RecipePackageIdentity (..)
  , artifactRecipeIdentity
  , artifactCarrier
  , artifactEntries
  , artifactTable
  , artifactSlotTable
  , artifactSpecialTable
  , artifactDefaultWindows
  , artifactFunctions
  , artifactProof
  , linkedBytes
  , storageStoreName
  , storageStoreWords
  , storageOutputName
  , storageOutputWords
  , storageStatusWords
  , mirrorBoundaries
  , stagePlacement
  , stageImage4
  , validateStagedImage4
  , stagedImage4Bytes
  , stagedBytes
  , stageCandidate
  , candidateBytes
  , candidateContractValue
  ) where

import qualified A32 as A
import qualified ArmElf as ELF
import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isPrefixOf, isSuffixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.FontWindow as FW
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Admission
import Replacement.PropertyService.ClassRecord (classValue, summaryValue)
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding
import Replacement.PropertyService.Placement
import ServiceInventory (Image (..))

newtype SourceIdentity = SourceIdentity SHA.Sha256

-- | The inputs, then image3, which `canonicalSource` extracts from the DAT.
data SourceRole = SourceDat | SourceImage4 | SourceLoaderA | SourceLoaderB | SourceImage5 | SourcePlain | SourceImage2 | SourceImage3
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | The closed admission authority of an artifact derivation.
data ArtifactPurpose = ProductionArtifact | CloseAppendArea3Control
  deriving (Eq, Show)

data CanonicalSource = CanonicalSource
  { sourceDat :: B.ByteString
  , sourceImage4 :: Image
  , sourceScanned :: [(ScanImage, Image)]
  , sourceIdentities :: [(SourceRole, SourceIdentity)]
  , sourceProperties :: [Property]
  , sourceHull :: HullScan
  }

data ArtifactInput = ArtifactInput
  { inputBytes :: B.ByteString
  , inputSymbols :: B.ByteString
  , inputElf :: B.ByteString
  }

data LinkedArtifact = LinkedArtifact
  { linkedBytes :: B.ByteString
  , linkedIdentity :: SourceIdentity
  , symbolIdentity :: SourceIdentity
  , elfIdentity :: SourceIdentity
  , artifactCarrier :: Carrier
  , artifactEntries :: EntryAddresses
  , artifactFunctions :: [FunctionExtent]
  , artifactTable :: GuestAddress
  , artifactSlotTable :: GuestAddress
  , artifactSpecialTable :: GuestAddress
  , artifactDefaultWindows :: [(DefaultImage, GuestAddress)]
  , artifactProof :: CodeProof
  , artifactRecipeIdentity :: Maybe RecipePackageIdentity
  }

newtype RecipePackageIdentity = RecipePackageIdentity SHA.Sha256 deriving Eq
data RecipeDependency = RecipeDependency FunctionExtent RecipePackageIdentity

-- | The placement checks' results and the staged image4 they admit.
data StagedPlacement = StagedPlacement
  { placementScan :: HullScan
  , placementBytes :: B.ByteString
  }

data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact StagedPlacement

data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

data EntrySide = GetterEntry | SetterEntry deriving (Eq, Show)

data DefaultImage = SlotDefaultBytes | TailDefaultBytes deriving (Eq, Show)
data StorageWidth = StorageByte | StorageHalf deriving (Eq, Show, Enum, Bounded)

data EncodingPart = EntryEncoding EntrySide | StorageEncoding StorageWidth | OutputEncoding StorageWidth | StatusEncoding | MirrorEncoding FunctionName deriving (Eq, Show)

-- | An llvm-nm symbol type the artifact may carry.
data SymbolKind = GlobalText | LocalText | ReadOnlyData | AbsoluteSymbol | OtherSection deriving (Eq, Show)

data ArtifactError
  = SourceMismatch SourceRole
  | ClassificationRejected String
  | PlacementRejected PlacementError
  | ExtentRejected
  | SymbolRejected
  | EncodingRejected EncodingPart
  | TableRejected
  | SlotTableRejected
  | SpecialTableRejected
  | DefaultTableRejected FunctionName
  | CodeRejected AdmissionError
  | CallbackRejected
  | ElfRejected
  | MappingRejected
  | StagingRejected
  deriving (Eq, Show)

require :: Bool -> ArtifactError -> Either ArtifactError ()
require premise failure = unless premise (Left failure)

identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256

identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

pins :: [(SourceRole, String)]
pins =
  [ (SourceDat, "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
  , (SourceImage4, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  , (SourceLoaderA, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
  , (SourceLoaderB, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
  , (SourceImage5, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
  , (SourcePlain, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  , (SourceImage2, "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5")
  , (SourceImage3, "a52dfa0aaea0c3eecb106aed809e99fed71cb9a754dc7cd412ff488b98542dac")
  ]

roleLabel :: SourceRole -> String
roleLabel role = case role of
  SourceDat -> "dat"
  SourceImage4 -> "image4"
  SourceLoaderA -> "loader_a"
  SourceLoaderB -> "loader_b"
  SourceImage5 -> "image5"
  SourcePlain -> "plain"
  SourceImage2 -> "image2"
  SourceImage3 -> "image3"

scanLabel :: ScanImage -> String
scanLabel label = case label of
  ScanImage4 -> "image4"
  ScanImage5 -> "image5"
  ScanPlain -> "plain"
  ScanLoaderA -> "loader_a"
  ScanLoaderB -> "loader_b"
  ScanImage2 -> "image2"
  ScanImage3 -> "image3"

-- | The canonical DAT, image4, loader A, loader B, image5, plain and image2 bytes, in that order. Image3
-- is the DAT's FONT_ICON.FDL window, which the loader copies raw to 0x3bc000.
canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat four loaderA loaderB five plain two = do
  let given = zip [minBound .. pred maxBound] (map identity [dat, four, loaderA, loaderB, five, plain, two])
  forM_ (zip given pins) $ \((role, actual), (_, expected)) -> require (identityText actual == expected) (SourceMismatch role)
  three <- either (const (Left (SourceMismatch SourceImage3))) (Right . U.mediaImageBytes) (U.parseContainer dat >>= FW.extractFont . U.parsedParts)
  require (Just (identityText (identity three)) == lookup SourceImage3 pins) (SourceMismatch SourceImage3)
  let identities = given ++ [(SourceImage3, identity three)]
  let image4 = Image "image4" "" 0x61c400 four
      scanned =
        [ (ScanImage4, image4)
        , (ScanImage5, Image "image5" "" 0xbf1a00 five)
        , (ScanPlain, Image "plain" "" 0x1100000 plain)
        , (ScanLoaderA, Image "loader_a" "" 0x40000 loaderA)
        , (ScanLoaderB, Image "loader_b" "" 0xd0000 loaderB)
        , (ScanImage2, Image "image2" "" 0x130000 two)
        , (ScanImage3, Image "image3" "" 0x3bc000 three)
        ]
  properties <- either (Left . ClassificationRejected) Right (classify image4)
  hull <- either (Left . PlacementRejected) Right (scanHull image4 scanned properties)
  pure (CanonicalSource dat image4 scanned identities properties hull)

storageStoreName :: StorageWidth -> String
storageStoreName width = case width of
  StorageByte -> "xm1_storage_store8"
  StorageHalf -> "xm1_storage_store16"

storageWindows :: [(Word32, Word32)]
storageWindows = [(0x96cc64, 0x96d1aa), (0x89ec00, 0x89ede0), (0x409c281c, 0x409c29b6), (0x409c29b8, 0x409c29c8)]

-- | The checked whole-write windows in storage-boundary.S.
storageStoreWords :: StorageWidth -> [Word32]
storageStoreWords width = concat [window index low high | (index, (low, high)) <- zip [0 ..] storageWindows] ++ [0xe12fff1e, store, 0xe12fff1e]
  where
    bytes = case width of StorageByte -> 1; StorageHalf -> 2
    store = case width of StorageByte -> 0xe5c01000; StorageHalf -> 0xe1c010b0
    constant address = [0xe300c000 .|. ((address .&. 0xf000) `shiftL` 4) .|. (address .&. 0xfff), 0xe340c000 .|. (((address `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. ((address `shiftR` 16) .&. 0xfff)]
    branch site target = 0x3a000000 .|. (fromIntegral (target - site - 2 :: Int) .&. 0xffffff)
    window index low high = constant low ++ [0xe150000c, branch (index * 8 + 3) ((index + 1) * 8)]
      ++ constant (high - bytes + 1) ++ [0xe150000c, branch (index * 8 + 7) 33]

storageOutputName :: StorageWidth -> String
storageOutputName width = case width of
  StorageByte -> "xm1_storage_output8"
  StorageHalf -> "xm1_storage_output16"

storageOutputWords :: StorageWidth -> [Word32]
storageOutputWords width = case width of
  StorageByte -> [0xe300c19a,0xe151000c,0x812fff1e,0xe090c001,0x212fff1e,0xe1520001,0x212fff1e,0xe7c03002,0xe12fff1e]
  StorageHalf -> [0xe300c19a,0xe151000c,0x812fff1e,0xe090c001,0x212fff1e,0xe292c002,0x212fff1e,0xe15c0001,0x812fff1e,0xe080c002,0xe31c0001,0x112fff1e,0xe1cc30b0,0xe12fff1e]

storageStatusWords :: [Word32]
storageStatusWords = [0xe30ecdc4,0xe340c089,0xe58c0000,0xe12fff1e]

mirrorBoundaries :: [(String, String, Word32)]
mirrorBoundaries =
  [ ("xm1_storage_context", "xm1_backup_context723cb8", 0x723cb8)
  , ("xm1_storage_acquire", "xm1_backup_acquire724b68", 0x724b68)
  , ("xm1_storage_release", "xm1_backup_release724c00", 0x724c00)
  , ("xm1_storage_diagnostic", "xm1_backup_diagnostic726c74", 0x726c74)
  , ("xm1_storage_write", "xm1_backup_write6a59b8", 0x6a59b8)
  , ("xm1_storage_report", "xm1_backup_report633594", 0x633594)
  ]

always :: A.Cond
always = A.Cond 14


propertiesSuffix, slotsSuffix, specialsSuffix :: String
propertiesSuffix = "14property_table10PROPERTIES"
slotsSuffix = "14property_table5SLOTS"
specialsSuffix = "14property_table8SPECIALS"

-- | The stock leaves owned code may call, each bound by an absolute linker symbol.
stockLeaves :: [(String, Word32)]
stockLeaves =
  [ ("xm1_stock_61df44", 0x61df44)
  , ("xm1_stock_61d8a4", 0x61d8a4)
  , ("xm1_stock_61d59c", 0x61d59c)
  , ("xm1_stock_61d618", 0x61d618)
  , ("xm1_stock_61d5f0", 0x61d5f0)
  , ("xm1_stock_61d65c", 0x61d65c)
  ]

-- | The lowest stack offset each stock leaf reaches: `str r11, [sp, #-4]!` at entry.
stockLeafStack :: StackOffset
stockLeafStack = StackOffset (-4)

symbolKind :: C.ByteString -> SymbolKind
symbolKind text = case C.unpack text of
  "T" -> GlobalText
  "t" -> LocalText
  "R" -> ReadOnlyData
  "r" -> ReadOnlyData
  "A" -> AbsoluteSymbol
  _ -> OtherSection

getterEntryTarget, setterEntryTarget :: String
getterEntryTarget = "xm1_property_get"
setterEntryTarget = "xm1_property_set"

-- | The area-3 handle cell, 0x89f86c..0x89f874 in the settings service contract.
area3HandleCell :: AddressSpan
area3HandleCell = AddressSpan (GuestAddress 0x89f86c) (GuestAddress 0x89f874)

-- | The only retained pointer span each purpose may add to literal admission.
purposeRetained :: ArtifactPurpose -> [AddressSpan]
purposeRetained ProductionArtifact = []
purposeRetained CloseAppendArea3Control = [area3HandleCell]

-- | The linked artifact admitted at `carrier`, which staging derives from the hull scan.
deriveArtifact :: ArtifactPurpose -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveArtifact purpose = deriveArtifactUsing purpose Nothing

deriveRecipeArtifactInternal :: RecipeDependency -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveRecipeArtifactInternal dependency = deriveArtifactUsing ProductionArtifact (Just dependency)

deriveArtifactUsing :: ArtifactPurpose -> Maybe RecipeDependency -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveArtifactUsing purpose dependency properties carrier@(Carrier (GuestAddress carrierLow) (GuestAddress carrierHigh)) (ArtifactInput bytes symbolBytes elfBytes) = do
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let rows = [(F.symbolName symbol, symbolKind kind) | (symbol, row) <- zip symbols (filter (not . C.null) (C.lines symbolBytes)), [_, _, kind, _] <- [C.words row]]
      kindOf = M.fromList rows
      kindIs kinds entry = maybe False (`elem` kinds) (M.lookup (F.symbolName entry) kindOf)
  require (length rows == length symbols && length (nub (map F.symbolName symbols)) == length symbols) SymbolRejected
  let symbol name = either (const (Left SymbolRejected)) Right (F.lookupSymbol symbols name)
      at name = F.symbolAddress <$> symbol name
      placed name address size = do
        entry <- symbol name
        require (F.symbolAddress entry == address && F.symbolSize entry == size && kindIs [GlobalText] entry) SymbolRejected
  start <- at "__property_service_start"
  end <- at "__property_service_end"
  require (start == carrierLow) (PlacementRejected (CarrierNotCandidate (GuestAddress start)))
  require (end >= start && end <= carrierHigh && end .&. 3 == 0
    && toInteger (B.length bytes) == toInteger (end - start)) ExtentRejected
  let callbacks = maybe [] (\(RecipeDependency callback _) -> [callback]) dependency
      callbackSymbols = [(name, low) | FunctionExtent (FunctionName name) (GuestAddress low) _ <- callbacks]
  forM_ callbacks $ \(FunctionExtent (FunctionName name) (GuestAddress low) (GuestAddress high)) -> do
    require (name == "xm1_recipe_edit_get" && low .&. 3 == 0 && high .&. 3 == 0 && low < high
      && (high <= carrierLow || low >= carrierHigh)) CallbackRejected
    entry <- symbol name
    require (F.symbolAddress entry == low && F.symbolSize entry == 0 && kindIs [AbsoluteSymbol] entry) CallbackRejected
  require (sortOn fst [(F.symbolName entry, F.symbolAddress entry) | entry <- symbols, kindIs [AbsoluteSymbol] entry] == sortOn fst (stockLeaves ++ [(external, address) | (_, external, address) <- mirrorBoundaries] ++ callbackSymbols)) SymbolRejected
  forM_ symbols $ \entry -> unless (kindIs [AbsoluteSymbol] entry) $
    require (kindIs [GlobalText, LocalText, ReadOnlyData] entry && F.symbolAddress entry >= start
      && toInteger (F.symbolAddress entry) + toInteger (F.symbolSize entry) <= toInteger end) ExtentRejected
  let getterEntryAddress = start
      getterEntryBytes = 4
      setterEntryAddress = start + getterEntryBytes
      setterEntryBytes = 4
  placed "xm1_property_get_entry" getterEntryAddress getterEntryBytes
  placed "xm1_property_set_entry" setterEntryAddress setterEntryBytes
  storeAddresses <- mapM (\width -> do
    address <- at (storageStoreName width)
    placed (storageStoreName width) address (4 * fromIntegral (length (storageStoreWords width)))
    pure (width, address)) [minBound .. maxBound]
  outputAddresses <- mapM (\width -> do
    address <- at (storageOutputName width)
    placed (storageOutputName width) address (4 * fromIntegral (length (storageOutputWords width)))
    pure (width,address)) [minBound .. maxBound]
  statusAddress <- at "xm1_storage_status32"
  placed "xm1_storage_status32" statusAddress (4 * fromIntegral (length storageStatusWords))
  mirrorAddresses <- mapM (\(name, _, target) -> do
    address <- at name
    placed name address 16
    pure (name, address, target)) mirrorBoundaries
  rustEntries <- mapM at [getterEntryTarget, setterEntryTarget]
  let tableNamed suffix = case [entry | entry <- symbols, suffix `isSuffixOf` F.symbolName entry] of
        [entry] -> Right entry
        _ -> Left SymbolRejected
  tableSymbol <- tableNamed propertiesSuffix
  slotSymbol <- tableNamed slotsSuffix
  specialSymbol <- tableNamed specialsSuffix
  defaultSymbols <- mapM (\(name, size, digest) -> do
    entry <- symbol name
    let address = F.symbolAddress entry
        content = B.take size (B.drop (fromIntegral (address-start)) bytes)
    require (F.symbolSize entry == fromIntegral size && address .&. 3 == 0 && identityText (identity content) == digest) (DefaultTableRejected (FunctionName name))
    pure entry)
    [ ("SLOT_BYTES",200,"f556b0f2f3cf1232809b5bedf6e4b7354d6f4e3ff62b740269573d1a1f705bb6")
    , ("TAIL_BYTES",40,"7e6a0e9ab8e0bfdea806e3f11a5a1f567c22c4275e4e5382f33d23baccac58c9")
    ]
  let functions =
        sortOn functionStart
          [ FunctionExtent (FunctionName (F.symbolName entry)) (GuestAddress (F.symbolAddress entry)) (GuestAddress (F.symbolAddress entry + F.symbolSize entry))
          | entry <- symbols, F.symbolSize entry > 0, entry `notElem` ([tableSymbol, slotSymbol, specialSymbol] ++ defaultSymbols), kindIs [GlobalText, LocalText] entry
          ]
      disjoint = and (zipWith (\(FunctionExtent _ _ a) (FunctionExtent _ b _) -> a <= b) functions (drop 1 functions))
  require (disjoint && all (\(FunctionExtent _ (GuestAddress low) (GuestAddress high)) -> low .&. 3 == 0 && high .&. 3 == 0) functions) ExtentRejected
  require (all (`elem` map functionStart functions) (map GuestAddress rustEntries)) SymbolRejected
  let word address = U.word32At bytes (fromIntegral (address - start))
      wordsAt address count = map word (take count [address, address + 4 ..])
  getterTarget <- at getterEntryTarget
  setterTarget <- at setterEntryTarget
  require (A.decode getterEntryAddress (word getterEntryAddress) == A.Branch always False getterTarget) (EncodingRejected (EntryEncoding GetterEntry))
  require (A.decode setterEntryAddress (word setterEntryAddress) == A.Branch always False setterTarget) (EncodingRejected (EntryEncoding SetterEntry))
  forM_ storeAddresses $ \(width, address) ->
    require (wordsAt address (length (storageStoreWords width)) == storageStoreWords width) (EncodingRejected (StorageEncoding width))
  forM_ outputAddresses $ \(width,address) ->
    require (wordsAt address (length (storageOutputWords width)) == storageOutputWords width) (EncodingRejected (OutputEncoding width))
  require (wordsAt statusAddress (length storageStatusWords) == storageStatusWords) (EncodingRejected StatusEncoding)
  forM_ mirrorAddresses $ \(name, address, target) ->
    require (wordsAt address 2 == [0xe92d4800, 0xe1a0b00d]
      && A.decode (address + 8) (word (address + 8)) == A.Branch always True target
      && word (address + 12) == 0xe8bd8800) (EncodingRejected (MirrorEncoding (FunctionName name)))
  EncodedTables entries slots specials <- either (Left . encodingRejected) Right (encodeTables (map propertyClass properties))
  let tableAddress = F.symbolAddress tableSymbol
      slotAddress = F.symbolAddress slotSymbol
      slotBytes = 4 * fromIntegral (length slots)
      specialAddress = F.symbolAddress specialSymbol
      specialBytes = 8 * fromIntegral (length specials)
  require (F.symbolSize tableSymbol == 2580 && tableAddress .&. 3 == 0 && length properties == 645) TableRejected
  require ([EncodedEntry (word (tableAddress + 4 * index)) | index <- [0 .. 644]] == entries) TableRejected
  require (F.symbolSize slotSymbol == slotBytes && slotAddress .&. 3 == 0) SlotTableRejected
  require ([EncodedSlot (word address) | address <- take (length slots) [slotAddress, slotAddress + 4 ..]] == slots) SlotTableRejected
  require (F.symbolSize specialSymbol == specialBytes && specialAddress .&. 3 == 0) SpecialTableRejected
  require ([EncodedSpecial (word address) (word (address + 4)) | address <- take (length specials) [specialAddress, specialAddress + 8 ..]] == specials) SpecialTableRejected
  (mappings, readonly) <- artifactMappings start end functions bytes symbolBytes elfBytes
  let layout = CodeLayout
        { layoutBase = GuestAddress start
        , layoutFunctions = functions
        , layoutMappings = mappings
        , layoutLiteralPointers = map OwnedReadonly readonly
            ++ [RetainedStorage retainedSpan | retainedSpan <- purposeRetained purpose]
            ++ [RetainedStorage (AddressSpan (GuestAddress low) (GuestAddress high)) | (low, high) <- storageWindows]
            ++ [RetainedConfiguration (AddressSpan (GuestAddress configBase) (GuestAddress (configBase + configBytes)))]
        , layoutData = [(GuestAddress tableAddress, GuestAddress (tableAddress + 2580)), (GuestAddress slotAddress, GuestAddress (slotAddress + slotBytes)), (GuestAddress specialAddress, GuestAddress (specialAddress + specialBytes))]
            ++ [(GuestAddress (F.symbolAddress entry), GuestAddress (F.symbolAddress entry + F.symbolSize entry)) | entry <- defaultSymbols]
            ++ [(low, high) | DataSpan (AddressSpan low high) <- mappings
                , not (any (\(FunctionExtent _ begin finish) -> begin < high && low < finish) functions)]
        , layoutStoreSites = [GuestAddress (address + case width of StorageByte -> 28; StorageHalf -> 48) | (width,address) <- outputAddresses]
            ++ [GuestAddress (statusAddress + 8)] ++ [GuestAddress (address + 132) | (_, address) <- storeAddresses]
        , layoutStockLeaves = [(GuestAddress address, stockLeafStack) | (_, address) <- stockLeaves]
        , layoutStockSites = [(GuestAddress (address + 8), GuestAddress target, StackOffset (-8)) | (_, address, target) <- mirrorAddresses]
        }
  proof <- either (Left . CodeRejected) Right (admitCodeWithCallbacks callbacks layout bytes)
  forM_ callbacks $ \callback ->
    require (any (\edge -> edgeTarget edge == functionStart callback && edgeKind edge `elem` [CallbackCall, CallbackTailCall])
      (concatMap frameCalls (proofFrames proof))) CallbackRejected
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) (identity elfBytes) carrier (EntryAddresses (GuestAddress getterEntryAddress) (GuestAddress setterEntryAddress)) functions (GuestAddress tableAddress) (GuestAddress slotAddress) (GuestAddress specialAddress) (zip [SlotDefaultBytes,TailDefaultBytes] [GuestAddress (F.symbolAddress entry) | entry <- defaultSymbols]) proof ((\(RecipeDependency _ packageIdentity) -> packageIdentity) <$> dependency))
  where
    encodingRejected failure = case failure of
      DelegatedProperty -> TableRejected
      SlotTableFull -> SlotTableRejected
      SpecialTableFull -> SpecialTableRejected
      SpecialDefaultUnencodable -> SpecialTableRejected


artifactMappings :: Word32 -> Word32 -> [FunctionExtent] -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError ([MappedSpan], [AddressSpan])
artifactMappings start end functions bytes symbolBytes elfBytes = do
  elf <- either (const (Left ElfRejected)) Right (ELF.parseElf elfBytes)
  (sectionIndex, section) <- case [(index, item) | (index, item) <- zip [0 ..] (ELF.elfSections elf), ELF.sectionAllocated item] of
    [item] -> Right item
    _ -> Left ElfRejected
  require (ELF.elfEntry elf == start && ELF.sectionType section == 1 && ELF.sectionExecutable section && not (ELF.sectionWritable section)
    && ELF.sectionAddress section == start && toInteger (ELF.sectionSize section) == toInteger end - toInteger start
    && B.take (B.length bytes) (B.drop (fromIntegral (ELF.sectionOffset section)) elfBytes) == bytes) ElfRejected
  require (all (\segment -> not (ELF.loadSegment segment) || not (ELF.segmentWritable segment)) (ELF.elfSegments elf)) ElfRejected
  require ([(ELF.segmentAddress segment, ELF.segmentOffset segment, ELF.segmentFileSize segment, ELF.segmentMemorySize segment)
      | segment <- ELF.elfSegments elf, ELF.loadSegment segment, ELF.segmentExecutable segment]
    == [(start, ELF.sectionOffset section, ELF.sectionSize section, ELF.sectionSize section)]) ElfRejected
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let elfSymbols = ELF.elfSymbols elf
      matching entry = [symbol | symbol <- elfSymbols, ELF.symbolName symbol == F.symbolName entry]
  forM_ symbols $ \entry -> case matching entry of
    [symbol] -> require (ELF.symbolValue symbol == F.symbolAddress entry && ELF.symbolSize symbol == F.symbolSize entry
      && ELF.symbolSection symbol `elem` [ELF.Absolute, ELF.InSection sectionIndex]) ElfRejected
    _ -> Left ElfRejected
  require (all (\symbol -> ELF.symbolKind symbol `notElem` [ELF.FunctionSymbol, ELF.ObjectSymbol]
    || any (\entry -> F.symbolName entry == ELF.symbolName symbol) symbols) elfSymbols) ElfRejected
  require (sortOn functionStart functions == sortOn functionStart
    [FunctionExtent (FunctionName (ELF.symbolName symbol)) (GuestAddress (ELF.symbolValue symbol))
      (GuestAddress (ELF.symbolValue symbol + ELF.symbolSize symbol))
    | symbol <- elfSymbols, ELF.symbolKind symbol == ELF.FunctionSymbol, ELF.symbolSize symbol > 0]) ElfRejected
  let mappingName name = name `elem` ["$a", "$d", "$t"] || any (`isPrefixOf` name) ["$a.", "$d.", "$t."]
      markers = [symbol | symbol <- elfSymbols, mappingName (ELF.symbolName symbol)]
  require (not (null markers) && all (\symbol -> ELF.symbolSection symbol == ELF.InSection sectionIndex
    && ELF.symbolSize symbol == 0 && ELF.symbolValue symbol >= start && ELF.symbolValue symbol < end
    && ELF.symbolValue symbol .&. 3 == 0 && take 2 (ELF.symbolName symbol) /= "$t") markers) MappingRejected
  let points = sortOn fst (nub [(ELF.symbolValue symbol, take 2 (ELF.symbolName symbol) == "$a") | symbol <- markers])
  case points of
    (first, _) : _ -> require (first == start && length (nub (map fst points)) == length points) MappingRejected
    [] -> Left MappingRejected
  let mappings = [ (if code then ArmSpan else DataSpan) (AddressSpan (GuestAddress low) (GuestAddress high))
        | ((low, code), high) <- zip points (map fst (drop 1 points) ++ [end]) ]
      readonly = [bounds | DataSpan bounds <- mappings]
  pure (mappings, readonly)

-- | Undefined-instruction fill for the carrier past the linked bytes.
fillWord :: Word32
fillWord = 0xe7f000f0

-- | Stage the owned text and adapters, retaining only admitted islands.
stagePlacement :: [Property] -> Image -> HullScan -> LinkedArtifact -> Either ArtifactError StagedPlacement
stagePlacement _ (Image _ _ base original) scan artifact = do
  require (artifactCarrier artifact == stage4Carrier && hullCarrier scan == Just stage4Carrier)
    (PlacementRejected (CarrierNotCandidate (carrierStart (artifactCarrier artifact))))
  let external = referencesInto retiredHull (hullHits scan)
  forM_ external $ \hit -> case hitReference hit of
    BranchInto target | target `notElem` [getterStockEntry, setterStockEntry] && not (inCarrier retainedWrappers target) ->
      Left (PlacementRejected (ExecutableReference hit))
    _ -> Right ()
  let regions = retainedRegions scan
      EntryAddresses getter setter = artifactEntries artifact
      wordAt address
        | address == getterStockEntry = adapterWord address getter
        | address == setterStockEntry = adapterWord address setter
        | inCarrier stage4Carrier address = let GuestAddress codeLow = carrierStart stage4Carrier; GuestAddress at = address
                                           in if fromIntegral (at - codeLow) < B.length (linkedBytes artifact)
                                              then U.word32At (linkedBytes artifact) (fromIntegral (at - codeLow)) else fillWord
        | any (`inCarrier` address) regions = let GuestAddress at = address in U.word32At original (fromIntegral (at - base))
        | otherwise = fillWord
      Carrier (GuestAddress low) (GuestAddress high) = retiredHull
      offset = fromIntegral (low - base)
      hull = B.concat [U.putWord32 (wordAt (GuestAddress address)) | address <- [low, low + 4 .. high - 4]]
      staged = B.take offset original <> hull <> B.drop (offset + B.length hull) original
  validateWithRegions artifact regions original staged
  pure (StagedPlacement scan staged)

adapterWord :: GuestAddress -> GuestAddress -> Word32
adapterWord (GuestAddress site) (GuestAddress target) = 0xea000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)

validateStagedImage4 :: HullScan -> LinkedArtifact -> B.ByteString -> B.ByteString -> Either ArtifactError ()
validateStagedImage4 scan artifact = validateWithRegions artifact (retainedRegions scan)

validateWithRegions :: LinkedArtifact -> [Carrier] -> B.ByteString -> B.ByteString -> Either ArtifactError ()
validateWithRegions artifact regions original staged = do
  require (B.length original == B.length staged) StagingRejected
  let EntryAddresses getter setter = artifactEntries artifact
      Carrier (GuestAddress low) (GuestAddress high) = retiredHull
      Carrier (GuestAddress codeLow) _ = artifactCarrier artifact
      word bytes address = U.word32At bytes (fromIntegral (address - 0x61c400))
  forM_ [(getterStockEntry, getter), (setterStockEntry, setter)] $ \(site@(GuestAddress address), target@(GuestAddress destination)) ->
    require (A.decode address (adapterWord site target) == A.Branch always False destination) StagingRejected
  forM_ [0x61c400, 0x61c404 .. 0x61c400 + fromIntegral (B.length original) - 4] $ \address -> do
    let guest = GuestAddress address
        expected
          | guest == getterStockEntry = adapterWord guest getter
          | guest == setterStockEntry = adapterWord guest setter
          | inCarrier (artifactCarrier artifact) guest = if fromIntegral (address - codeLow) < B.length (linkedBytes artifact)
              then U.word32At (linkedBytes artifact) (fromIntegral (address - codeLow)) else fillWord
          | address < low || address >= high || any (`inCarrier` guest) regions = word original address
          | otherwise = fillWord
    require (word staged address == expected) StagingRejected

stageImage4 :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImage4
stageImage4 source artifact = StagedImage4 source artifact <$> stagePlacement (sourceProperties source) (sourceImage4 source) (sourceHull source) artifact

-- | The staged image4 for an admitted artifact, or the refusal `stageImage4` gives.
stagedImage4Bytes :: CanonicalSource -> LinkedArtifact -> Either ArtifactError B.ByteString
stagedImage4Bytes source artifact = stagedBytes <$> stageImage4 source artifact

stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ placement) = placementBytes placement

stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 source _ placement) = do
  candidate <- Image4.runImage4Staging tools scratch (sourceDat source) (sourceDat source) (placementBytes placement)
  pure (StagedCandidate staged candidate)

candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged@(StagedImage4 source artifact (StagedPlacement scan _)) candidate) = object $
  [ "schema" .= (2 :: Int)
  , "qualification" .= ("conditional-owned-property-service-stage-4" :: String)
  , "source_sha256" .= M.fromList [(roleLabel role, identityText digest) | (role, digest) <- sourceIdentities source]
  , "linked_sha256" .= identityText (linkedIdentity artifact)
  , "symbols_sha256" .= identityText (symbolIdentity artifact)
  , "elf_sha256" .= identityText (elfIdentity artifact)
  , "linked_bytes" .= B.length (linkedBytes artifact)
  , "carrier" .= object ["start" .= low, "end" .= high, "bytes" .= carrierBytes carrier, "fill_words" .= ((carrierBytes carrier - B.length (linkedBytes artifact)) `div` 4), "fill_word" .= fillWord]
  , "hull" .= object
      [ "start" .= hullLow, "end" .= hullHigh, "bytes" .= carrierBytes (hullRegion scan)
      , "rule" .= ("fixed carrier; full hull retired except entry adapters, wrappers and external data reservations" :: String)
      , "hits" .= [object ["image" .= scanLabel label, "site" .= site, "word" .= word, "target" .= target, "reference" .= show reference, "listing" .= hitListing hit] | hit@(ScanHit label (GuestAddress site) word reference) <- hullHits scan, let GuestAddress target = hitTarget hit]
      ]
  , "entries" .= object ["getter" .= getterAddress, "setter" .= setterAddress]
  , "adapters" .=
      [ object ["address" .= address, "word" .= adapterWord (GuestAddress address) target]
      | (address, target) <- [(0x61e110, getterEntry (artifactEntries artifact)), (0x624694, setterEntry (artifactEntries artifact))]
      ]
  , "retained_regions" .= [object ["start" .= a, "end" .= b] | Carrier (GuestAddress a) (GuestAddress b) <- retainedRegions scan]
  , "scan" .= object
      [ "images" .=
          [ object ["image" .= scanLabel label, "base" .= imageBase scannedImage, "bytes" .= B.length (imageBytes scannedImage), "sha256" .= identityText (identity (imageBytes scannedImage)), "instructions" .= (scanDepth label == InstructionsAndWords)]
          | (label, scannedImage) <- sourceScanned source
          ]
      , "references" .= (["branch", "word", "movw-movt", "pc-relative"] :: [String])
      , "hull_hits" .= length (hullHits scan)
      , "exemptions" .=
          [ object
              [ "image" .= scanLabel (siteImage site), "address" .= siteAt, "word" .= siteWord site
              , "window_start" .= (siteAt - 32), "window_bytes" .= (64 :: Int)
              , "window_sha256" .= SHA.sha256Hex (siteWindowSha256 site)
              , "reason" .= reasonText (siteReason site)
              , "window_rejected_decodes" .= rejected
              , "reference" .= show reference
              , "listing" .= hitListing hit
              ]
          | ExemptedHit site hit@(ScanHit _ _ _ reference) rejected <- hullExempted scan
          , let GuestAddress siteAt = siteAddress site
          ]
      ]
  , "classification" .= object ["summary" .= summaryValue (sourceProperties source), "encoding" .= encodingDescription, "properties" .= map classValue (sourceProperties source)]
  , "stack" .= object
      [ "entries" .= [frameValue frame | frame <- proofFrames proof]
      , "entry_max_depth_bytes" .= object ["getter" .= depth getterAddress, "setter" .= depth setterAddress]
      ]
  , "owned_instruction_count" .= S.size (proofInstructions proof)
  , "literal_words" .= [address | GuestAddress address <- S.toList (proofLiterals proof)]
  , "nonreturning_entries" .= [address | GuestAddress address <- S.toList (proofNonreturning proof)]
  , "literal_pointer_contract" .= ("Each literal word supplies a pointer whose first four bytes lie in ELF-mapped owned readonly data, a fixed storage window or the classified retained configuration window. This does not establish an object size or confine downstream loads." :: String)
  , "staged_image4_sha256" .= identityText (identity (stagedBytes staged))
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  , "open_premises" .=
      ([ "loads are not confined: owned code may read any address; the native oracle confines reads"
       , "stack depths are bytes below the owned callable entry sp"
       , "the setter owns its 32-byte frame and calls retained brackets at incoming sp minus 32; bracket chain depth and register use are not proved"
       , "the code policy checks sp, r11 and the return address at every exit; other callee-saved registers follow AAPCS unproved"
       , "the reference scan reads image4, image5, plain, loader A and loader B as instructions and words, and image2 and image3 (the FONT_ICON.FDL window extracted from the DAT) as words; it does not cover computed addresses"
       ] ++
       [ "the recipe callback AAPCS behavior, stack depth, publication and installed lifetime remain unproved"
       | Just _ <- [artifactRecipeIdentity artifact]
       ] :: [String])
  ] ++ ["recipe_edit_package_sha256" .= SHA.sha256Hex packageDigest
       | Just (RecipePackageIdentity packageDigest) <- [artifactRecipeIdentity artifact]]
  where
    carrier@(Carrier (GuestAddress low) (GuestAddress high)) = artifactCarrier artifact
    EntryAddresses (GuestAddress getterAddress) (GuestAddress setterAddress) = artifactEntries artifact
    Carrier (GuestAddress hullLow) (GuestAddress hullHigh) = hullRegion scan
    proof = artifactProof artifact
    names = M.fromList [(entry, name) | FunctionExtent (FunctionName name) entry _ <- artifactFunctions artifact]
    depth address = maybe 0 (\(StackOffset offset) -> negate offset) (M.lookup (GuestAddress address) (proofComposed proof))
    frameValue (EntryFrame entry@(GuestAddress address) (StackOffset own) calls) = object
      [ "entry" .= address
      , "name" .= M.lookup entry names
      , "own_depth_bytes" .= negate own
      , "max_depth_bytes" .= depth address
      , "calls" .=
          [ object ["site" .= site, "target" .= target, "sp_offset" .= at, "tail" .= (kind `elem` [TailCall, StockLeafTailCall, CallbackTailCall]), "stock" .= (kind `elem` [StockLeafCall, StockLeafTailCall, StockSiteCall]), "callback" .= (kind `elem` [CallbackCall, CallbackTailCall])]
          | CallEdge (GuestAddress site) (GuestAddress target) (StackOffset at) kind <- calls
          ]
      ]
    reasonText reason = case reason of
      HighEntropyData -> "high-entropy-data" :: String
