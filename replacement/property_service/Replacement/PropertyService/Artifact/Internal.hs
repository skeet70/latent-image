{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Admission of the linked owned property service and its staging into image4.
module Replacement.PropertyService.Artifact.Internal
  ( CanonicalSource
  , LinkedArtifact
  , ArtifactInput (..)
  , ArtifactPurpose (..)
  , StagedImage4
  , NativeImage4View
  , StagedCandidate
  , StagedPlacement (..)
  , ArtifactError (..)
  , EntrySide (..)
  , StorageWidth (..)
  , DefaultImage (..)
  , EncodingPart (..)
  , PublicEntryShape (..)
  , canonicalSource
  , sourceProperties
  , sourceHull
  , sourceImage4
  , sourceScanned
  , sourceRetainedFieldEvidence
  , deriveArtifact
  , derivePublicArtifact
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
  , storageStoreWord
  , storageOutputName
  , storageOutputWords
  , storageStatusWords
  , mirrorBoundaries
  , kernelBoundaries
  , stagePlacement
  , stageNativePlacement
  , validateLocalImage4
  , nativeImage4View
  , nativeViewBytes
  , nativeViewIdentityValue
  , validateNativeImage4View
  , nativeRetiredReferences
  , stageNativeImage4
  , stageImage4
  , validateStagedImage4
  , stagedImage4Bytes
  , stagedBytes
  , stageCandidate
  , candidateBytes
  , candidateContractValue
  , publicCandidateContractValue
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
import Replacement.PropertyService.PlacementNative (RetainedFieldEvidence, retainedFieldEvidence, retiredReferences)
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

-- | The checked whole-write windows in storage-boundary.S. Each window's last
-- store starts at @high - width@, the bound the assembler emits.
storageStoreWords :: StorageWidth -> [Word32]
storageStoreWords width = concat [window index low high | (index, (low, high)) <- zip [0 ..] storageWindows] ++ [0xe12fff1e]
  where
    bytes = case width of StorageByte -> 1; StorageHalf -> 2
    store = storageStoreWord width
    constant register address =
      [ 0xe3000000 .|. (register `shiftL` 12) .|. ((address .&. 0xf000) `shiftL` 4) .|. (address .&. 0xfff)
      , 0xe3400000 .|. (register `shiftL` 12) .|. (((address `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. ((address `shiftR` 16) .&. 0xfff)
      ]
    branch site target = 0x8a000000 .|. (fromIntegral (target - site - 2 :: Int) .&. 0xffffff)
    window index low high =
      constant (12 :: Word32) low
        ++ [0xe040200c]
        ++ constant (3 :: Word32) (high - bytes - low)
        ++ [0xe1520003, branch (index * 10 + 6) ((index + 1) * 10), 0xe082200c, store, 0xe12fff1e]

-- | The store opcode of the checked-store encoding, at its one width per window.
storageStoreWord :: StorageWidth -> Word32
storageStoreWord StorageByte = 0xe5c21000
storageStoreWord StorageHalf = 0xe1c210b0

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
  [ ("xm1_storage_diagnostic", "xm1_backup_diagnostic726c74", 0x726c74)
  , ("xm1_storage_write", "xm1_backup_write6a59b8", 0x6a59b8)
  , ("xm1_storage_report", "xm1_backup_report633594", 0x633594)
  ]

kernelBoundaries :: [(String, Word32)]
kernelBoundaries =
  [ ("xm1_kernel_console9", 0x41a34)
  , ("xm1_kernel_critical_enter", 0x7235d8)
  , ("xm1_kernel_critical_exit", 0x7236b0)
  ]

-- | Own-frame pushes at retained kernel entries; their callee depths are not proved.
kernelBoundaryStack :: [(GuestAddress, StackOffset)]
kernelBoundaryStack =
  [ (GuestAddress 0x41a34, StackOffset (-32))
  , (GuestAddress 0x7235d8, StackOffset (-32))
  , (GuestAddress 0x7236b0, StackOffset (-24))
  ]

-- | The table the kernel crate exports as its declared boundary.
kernelBoundaryName :: String
kernelBoundaryName = "XM1_KERNEL_BOUNDARY"

kernelBoundaryMagic :: Word32
kernelBoundaryMagic = 0x314e524b

newtype KernelBoundary = KernelBoundary { kernelWindows :: [AddressSpan] }

readKernelBoundary :: Word32 -> B.ByteString -> F.Symbol -> Either AdmissionError KernelBoundary
readKernelBoundary start bytes entry = do
  let address = F.symbolAddress entry
      word index = U.word32At bytes (fromIntegral (address - start) + 4 * index)
      windows = fromIntegral (word 1) :: Int
      entries = fromIntegral (word 2) :: Int
  unless (address .&. 3 == 0 && word 0 == kernelBoundaryMagic && windows <= 16 && entries <= 32
    && F.symbolSize entry == fromIntegral (4 * (3 + 2 * windows + entries)))
    (Left (AdmissionError (GuestAddress address) BoundaryInvalid))
  pure (KernelBoundary [AddressSpan (GuestAddress (word (3 + 2 * index))) (GuestAddress (word (4 + 2 * index))) | index <- [0 .. windows - 1]])

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

-- | Retained regions each purpose may add for relocated data references.
purposeRetained :: ArtifactPurpose -> [AddressSpan]
purposeRetained ProductionArtifact = []
purposeRetained CloseAppendArea3Control = [area3HandleCell]

-- | The linked artifact admitted at `carrier`, which staging derives from the hull scan.
deriveArtifact :: ArtifactPurpose -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveArtifact purpose = deriveArtifactUsing CanonicalEntry purpose Nothing

-- | The linked artifact whose public roots are the owned getter and setter wrappers.
derivePublicArtifact :: ArtifactPurpose -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
derivePublicArtifact purpose = deriveArtifactUsing PublicWrapperEntry purpose Nothing

deriveRecipeArtifactInternal :: RecipeDependency -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveRecipeArtifactInternal dependency = deriveArtifactUsing CanonicalEntry ProductionArtifact (Just dependency)

-- | Which owned bodies the image places at the carrier start.
data PublicEntryShape = CanonicalEntry | PublicWrapperEntry deriving (Eq, Show)

deriveArtifactUsing :: PublicEntryShape -> ArtifactPurpose -> Maybe RecipeDependency -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError LinkedArtifact
deriveArtifactUsing shape purpose dependency properties carrier@(Carrier (GuestAddress carrierLow) (GuestAddress carrierHigh)) (ArtifactInput bytes symbolBytes elfBytes) = do
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
  require (sortOn fst [(F.symbolName entry, F.symbolAddress entry) | entry <- symbols, kindIs [AbsoluteSymbol] entry] == sortOn fst (stockLeaves ++ kernelBoundaries ++ [(external, address) | (_, external, address) <- mirrorBoundaries] ++ callbackSymbols)) SymbolRejected
  forM_ symbols $ \entry -> unless (kindIs [AbsoluteSymbol] entry) $
    require (kindIs [GlobalText, LocalText, ReadOnlyData] entry && F.symbolAddress entry >= start
      && toInteger (F.symbolAddress entry) + toInteger (F.symbolSize entry) <= toInteger end) ExtentRejected
  let (getterEntryName, getterEntryBytes) = case shape of
        CanonicalEntry -> ("xm1_property_get_entry", 4)
        PublicWrapperEntry -> ("xm1_public_get", 12)
      (setterEntryName, setterEntryBytes) = case shape of
        CanonicalEntry -> ("xm1_property_set_entry", 4)
        PublicWrapperEntry -> ("xm1_public_set", 12)
      getterEntryAddress = start
      setterEntryAddress = start + getterEntryBytes
  placed getterEntryName getterEntryAddress getterEntryBytes
  placed setterEntryName setterEntryAddress setterEntryBytes
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
  boundarySymbol <- symbol kernelBoundaryName
  boundary <- either (Left . CodeRejected) Right (readKernelBoundary start bytes boundarySymbol)
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
          | entry <- symbols, F.symbolSize entry > 0, entry `notElem` ([tableSymbol, slotSymbol, specialSymbol, boundarySymbol] ++ defaultSymbols), kindIs [GlobalText, LocalText] entry
          ]
      disjoint = and (zipWith (\(FunctionExtent _ _ a) (FunctionExtent _ b _) -> a <= b) functions (drop 1 functions))
  require (disjoint && all (\(FunctionExtent _ (GuestAddress low) (GuestAddress high)) -> low .&. 3 == 0 && high .&. 3 == 0) functions) ExtentRejected
  require (all (`elem` map functionStart functions) (map GuestAddress rustEntries)) SymbolRejected
  let word address = U.word32At bytes (fromIntegral (address - start))
      wordsAt address count = map word (take count [address, address + 4 ..])
  getterTarget <- at getterEntryTarget
  setterTarget <- at setterEntryTarget
  case shape of
    CanonicalEntry -> do
      require (A.decode getterEntryAddress (word getterEntryAddress) == A.Branch always False getterTarget) (EncodingRejected (EntryEncoding GetterEntry))
      require (A.decode setterEntryAddress (word setterEntryAddress) == A.Branch always False setterTarget) (EncodingRejected (EntryEncoding SetterEntry))
    PublicWrapperEntry -> do
      require (wordsAt getterEntryAddress 1 == [0xe92d4830]
        && A.decode (getterEntryAddress + 4) (word (getterEntryAddress + 4)) == A.Branch always True getterTarget
        && word (getterEntryAddress + 8) == 0xe8bd8830) (EncodingRejected (EntryEncoding GetterEntry))
      require (wordsAt setterEntryAddress 1 == [0xe92d49f8]
        && A.decode (setterEntryAddress + 4) (word (setterEntryAddress + 4)) == A.Branch always True setterTarget
        && word (setterEntryAddress + 8) == 0xe8bd89f8) (EncodingRejected (EntryEncoding SetterEntry))
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
  mappings <- artifactMappings start end functions bytes symbolBytes elfBytes
  elf <- either (const (Left ElfRejected)) Right (ELF.parseElf elfBytes)
  references <- either (const (Left ElfRejected)) Right (ELF.addressRelocations elf)
  require (not (null references)) ElfRejected
  let layout = CodeLayout
        { layoutBase = GuestAddress start
        , layoutFunctions = functions
        , layoutMappings = mappings
        , layoutData = [(GuestAddress tableAddress, GuestAddress (tableAddress + 2580)), (GuestAddress slotAddress, GuestAddress (slotAddress + slotBytes)), (GuestAddress specialAddress, GuestAddress (specialAddress + specialBytes))]
            ++ [(GuestAddress (F.symbolAddress entry), GuestAddress (F.symbolAddress entry + F.symbolSize entry)) | entry <- defaultSymbols]
            ++ [(low, high) | DataSpan (AddressSpan low high) <- mappings
                , not (any (\(FunctionExtent _ begin finish) -> begin < high && low < finish) functions)]
        , layoutBoundaryWindows = kernelWindows boundary
            ++ purposeRetained purpose
            ++ [AddressSpan (GuestAddress low) (GuestAddress high) | (low, high) <- storageWindows]
            ++ [AddressSpan (GuestAddress configBase) (GuestAddress (configBase + configBytes))]
        , layoutStockLeaves = [(GuestAddress address, stockLeafStack) | (_, address) <- stockLeaves]
            ++ kernelBoundaryStack ++ [(GuestAddress target, StackOffset 0) | (_, _, target) <- mirrorAddresses]
        , layoutReferences = references
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


artifactMappings :: Word32 -> Word32 -> [FunctionExtent] -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError [MappedSpan]
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
  pure mappings

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

-- | The local image4 placement of an artifact whose public roots are owned wrappers.
data NativeImage4View = NativeImage4View CanonicalSource LinkedArtifact StagedPlacement

sourceRetainedFieldEvidence :: CanonicalSource -> [ScanHit] -> [RetainedFieldEvidence]
sourceRetainedFieldEvidence source = retainedFieldEvidence (sourceScanned source)

nativeRetiredReferences :: CanonicalSource -> [ScanHit]
nativeRetiredReferences = retiredReferences . sourceScanned

-- | Place only the fixed carrier and the two public root branches; no retirement is admitted.
stageNativePlacement :: [Property] -> Image -> LinkedArtifact -> Either ArtifactError StagedPlacement
stageNativePlacement properties (Image _ _ base original) artifact = do
  require (artifactCarrier artifact == stage4Carrier)
    (PlacementRejected (CarrierNotCandidate (carrierStart (artifactCarrier artifact))))
  require (length properties == 645 && all (\property -> case propertyClass property of Owned _ -> True; Delegated _ -> False) properties) TableRejected
  require (base == 0x61c400
    && U.word32At original (fromIntegral (0x61e110 - base)) == 0xe92d4830
    && U.word32At original (fromIntegral (0x624694 - base)) == 0xe92d49f8)
    (PlacementRejected PublicEntryDisagrees)
  let EntryAddresses getter setter = artifactEntries artifact
      Carrier (GuestAddress low) _ = artifactCarrier artifact
      offset address = fromIntegral (address - base)
      carried = linkedBytes artifact <> B.concat (replicate ((carrierBytes stage4Carrier - B.length (linkedBytes artifact)) `div` 4) (U.putWord32 fillWord))
      replace at replacement bytes = B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes
      withCarrier = replace (offset low) carried original
      staged = foldl (\bytes (site@(GuestAddress address), target) -> replace (offset address) (U.putWord32 (adapterWord site target)) bytes)
        withCarrier [(getterStockEntry, getter), (setterStockEntry, setter)]
  validateLocalImage4 artifact original staged
  pure (StagedPlacement (HullScan retiredHull [] [] (Just stage4Carrier)) staged)

-- | Check a locally placed image4: only the carrier and the two root branches may differ.
validateLocalImage4 :: LinkedArtifact -> B.ByteString -> B.ByteString -> Either ArtifactError ()
validateLocalImage4 artifact original staged = do
  let EntryAddresses getter setter = artifactEntries artifact
      entryStarts = [GuestAddress low | FunctionExtent _ (GuestAddress low) _ <- artifactFunctions artifact]
  require (B.length original == B.length staged && B.length original `mod` 4 == 0
    && artifactCarrier artifact == stage4Carrier
    -- The canonical entry is a 4-byte branch and the public wrapper 12 bytes, so the
    -- setter starts at the second function the artifact declares.
    && [getter, setter] == take 2 entryStarts
    && B.length (linkedBytes artifact) <= carrierBytes stage4Carrier
    && B.length (linkedBytes artifact) `mod` 4 == 0) StagingRejected
  let Carrier (GuestAddress codeLow) _ = artifactCarrier artifact
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
          | otherwise = word original address
    require (word staged address == expected) StagingRejected

nativeImage4View :: CanonicalSource -> LinkedArtifact -> Either ArtifactError NativeImage4View
nativeImage4View source artifact =
  NativeImage4View source artifact <$> stageNativePlacement (sourceProperties source) (sourceImage4 source) artifact

nativeViewBytes :: NativeImage4View -> B.ByteString
nativeViewBytes (NativeImage4View _ _ placement) = placementBytes placement

validateNativeImage4View :: NativeImage4View -> B.ByteString -> Either ArtifactError ()
validateNativeImage4View (NativeImage4View source artifact _) =
  validateLocalImage4 artifact (imageBytes (sourceImage4 source))

nativeViewIdentityValue :: NativeImage4View -> Value
nativeViewIdentityValue view@(NativeImage4View source artifact _) = object
  [ "schema" .= (1 :: Int)
  , "qualification_scope" .= ("native-only" :: String)
  , "dat_retirement_admitted" .= False
  , "source_sha256" .= M.fromList [(roleLabel role, identityText digest) | (role, digest) <- sourceIdentities source]
  , "linked_sha256" .= identityText (linkedIdentity artifact)
  , "symbols_sha256" .= identityText (symbolIdentity artifact)
  , "elf_sha256" .= identityText (elfIdentity artifact)
  , "linked_bytes" .= B.length (linkedBytes artifact)
  , "native_image4_sha256" .= identityText (identity (nativeViewBytes view))
  , "carrier" .= object ["start" .= low, "end" .= high, "bytes" .= carrierBytes carrier, "fill_word" .= fillWord]
  , "public_wrappers" .= object ["getter" .= getter, "setter" .= setter]
  , "public_entry_words" .=
      [object ["address" .= address, "original" .= U.word32At original (fromIntegral (address - 0x61c400)), "patched" .= adapterWord (GuestAddress address) target]
      | (address, target) <- [(0x61e110, getterEntry (artifactEntries artifact)), (0x624694, setterEntry (artifactEntries artifact))]]
  , "owned_instruction_count" .= S.size (proofInstructions (artifactProof artifact))
  , "recipe_edit_package_sha256" .= fmap (\(RecipePackageIdentity digest) -> SHA.sha256Hex digest) (artifactRecipeIdentity artifact)
  ]
  where
    carrier@(Carrier (GuestAddress low) (GuestAddress high)) = artifactCarrier artifact
    EntryAddresses (GuestAddress getter) (GuestAddress setter) = artifactEntries artifact
    original = imageBytes (sourceImage4 source)

stageImage4 :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImage4
stageImage4 source artifact = StagedImage4 source artifact <$> stagePlacement (sourceProperties source) (sourceImage4 source) (sourceHull source) artifact

-- | The locally placed image4, refusing while any retired reference is unresolved.
stageNativeImage4 :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImage4
stageNativeImage4 source artifact = do
  let hits = retiredReferences (sourceScanned source)
  require (null hits) (PlacementRejected (RetiredReferenced hits))
  StagedImage4 source artifact <$> stageNativePlacement (sourceProperties source) (sourceImage4 source) artifact

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
candidateContractValue = candidateContractValueWith canonicalHullRule canonicalOpenPremises

-- | The candidate contract of the locally placed public-entry image.
publicCandidateContractValue :: StagedCandidate -> Value
publicCandidateContractValue = candidateContractValueWith publicHullRule publicOpenPremises

canonicalHullRule :: String
canonicalHullRule = "fixed carrier; full hull retired except entry adapters, wrappers and external data reservations"

publicHullRule :: String
publicHullRule = "only two public root branches and the fixed carrier change; no retirement admitted with unknown references"

canonicalOpenPremises :: [String]
canonicalOpenPremises =
  [ "loads are not confined: owned code may read any address; the native oracle confines reads"
  , "stack depths are bytes below the owned callable entry sp"
  , "the setter owns its 32-byte frame and calls retained brackets at incoming sp minus 32; bracket chain depth and register use are not proved"
  , "owned memory safety rests on the compiler, bounded types, source boundaries and subsystem comparisons"
  , "the reference scan reads image4, image5, plain, loader A and loader B as instructions and words, and image2 and image3 (the FONT_ICON.FDL window extracted from the DAT) as words; it does not cover computed addresses"
  ]

publicOpenPremises :: [String]
publicOpenPremises =
  [ "loads are not confined: owned code may read any address; the native oracle confines reads"
  , "stack depths are bytes below the owned callable entry sp"
  , "public wrappers preserve r4-r11 and the setter's original r3; retained bracket chain depth and register use are not proved"
  , "owned memory safety rests on the compiler, bounded types, source boundaries and subsystem comparisons"
  , "the reference scan reads image4, image5, plain, loader A and loader B as instructions and words, and image2 and image3 (the FONT_ICON.FDL window extracted from the DAT) as words; it does not cover computed addresses"
  ]

candidateContractValueWith :: String -> [String] -> StagedCandidate -> Value
candidateContractValueWith hullRule premises (StagedCandidate staged@(StagedImage4 source artifact (StagedPlacement scan _)) candidate) = object $
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
      , "rule" .= hullRule
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
  , "mapped_data_words" .= [address | GuestAddress address <- S.toList (proofLiterals proof)]
  , "address_relocations" .= [object ["site" .= ELF.addressSite reference, "target" .= ELF.addressTarget reference, "kind" .= show (ELF.addressKind reference)] | reference <- proofReferences proof]
  , "reference_contract" .= ("ELF address relocations and direct branches name owned bytes or declared stock entries and regions. Unrelocated literals and formed constants, indirect targets and memory safety are outside binary admission. Owned memory safety rests on the compiler, bounded types, source boundaries and subsystem comparisons." :: String)
  , "stack_scope" .= ("Direct-call composition of static owned frame allocations; assumes allocations do not repeat on a path and no unrecognized dynamic stack adjustment. Stock leaf frames are charged 4 bytes; kernel entries 0x41a34 and 0x7235d8 are charged 32 bytes and 0x7236b0 24 bytes. Mirror stock entries 0x633594, 0x6a59b8 and 0x726c74 are charged zero. Their own frames (48, 16 and 16 bytes), their direct callees and the direct callees of 0x41a34 are not proved by this budget. Callback and indirect-call stack use is not proved." :: String)
  , "staged_image4_sha256" .= identityText (identity (stagedBytes staged))
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  , "open_premises" .=
      (premises ++
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
          [ object ["site" .= site, "target" .= target, "sp_offset" .= at, "tail" .= (kind `elem` [TailCall, StockLeafTailCall, CallbackTailCall]), "stock" .= (kind `elem` [StockLeafCall, StockLeafTailCall]), "callback" .= (kind `elem` [CallbackCall, CallbackTailCall])]
          | CallEdge (GuestAddress site) (GuestAddress target) (StackOffset at) kind <- calls
          ]
      ]
    reasonText reason = case reason of
      HighEntropyData -> "high-entropy-data" :: String
