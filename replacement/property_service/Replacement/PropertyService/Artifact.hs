{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Admission of the linked owned property service and its staging into image4.
module Replacement.PropertyService.Artifact
  ( CanonicalSource
  , LinkedArtifact
  , StagedImage4
  , StagedCandidate
  , StagedPlacement (..)
  , ArtifactError (..)
  , Stub (..)
  , AccessorImage (..)
  , AccessorOperation (..)
  , Accessor (..)
  , EncodingPart (..)
  , canonicalSource
  , sourceProperties
  , deriveArtifact
  , artifactCarrier
  , artifactStubs
  , artifactTable
  , artifactSlotTable
  , artifactFunctions
  , artifactProof
  , linkedBytes
  , accessorName
  , accessors
  , stagePlacement
  , stageImage4
  , stagedImage4Bytes
  , stagedBytes
  , stageCandidate
  , candidateBytes
  , candidateContractValue
  ) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isSuffixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.FontWindow as FW
import qualified FujiAF.UpdateContainer as U
import PropertyFields (PropertyId (..))
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

data CanonicalSource = CanonicalSource
  { sourceDat :: B.ByteString
  , sourceImage4 :: Image
  , sourceScanned :: [(ScanImage, Image)]
  , sourceIdentities :: [(SourceRole, SourceIdentity)]
  , sourceProperties :: [Property]
  }

data LinkedArtifact = LinkedArtifact
  { linkedBytes :: B.ByteString
  , linkedIdentity :: SourceIdentity
  , symbolIdentity :: SourceIdentity
  , artifactCarrier :: Carrier
  , artifactStubs :: StubAddresses
  , artifactFunctions :: [FunctionExtent]
  , artifactTable :: GuestAddress
  , artifactSlotTable :: GuestAddress
  , artifactProof :: CodeProof
  }

-- | The placement checks' results and the staged image4 they admit.
data StagedPlacement = StagedPlacement
  { placementPreceding :: Word32
  , placementPatch :: TablePatch
  , placementExempted :: [ExemptedHit]
  , placementRefused :: [ScanHit]
  , placementPassedOver :: [PassedOver]
  , placementBytes :: B.ByteString
  }

data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact StagedPlacement

data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

data Stub = GetterStub | SetterStub deriving (Eq, Show)

-- | The bank images 0x96cc64, the previous-value image 0x89ec00, the slot-defaults records
-- 0x7cb804 and the defaults table 0x81b074.
data AccessorImage = BankImage | SaveImage | SlotDefaultsImage | TailDefaultsImage deriving (Eq, Show, Enum, Bounded)

data AccessorOperation = Load8 | Load16 | Store8 | Store16 deriving (Eq, Show, Enum, Bounded)

data Accessor = Accessor AccessorImage AccessorOperation deriving (Eq, Show)

data EncodingPart = StubEncoding Stub | AccessorEncoding Accessor deriving (Eq, Show)

-- | An llvm-nm symbol type the artifact may carry.
data SymbolKind = GlobalText | LocalText | AbsoluteSymbol | OtherSection deriving (Eq, Show)

data ArtifactError
  = SourceMismatch SourceRole
  | ClassificationRejected String
  | PlacementRejected PlacementError
  | ExtentRejected
  | SymbolRejected
  | EncodingRejected EncodingPart
  | TableRejected
  | SlotTableRejected
  | CodeRejected AdmissionError
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
  pure (CanonicalSource dat image4 scanned identities properties)

-- | Loads and stores of the bank and previous-value images, and loads of the two defaults images.
accessors :: [Accessor]
accessors =
  [Accessor image operation | image <- [BankImage, SaveImage], operation <- [minBound .. maxBound]]
    ++ [Accessor SlotDefaultsImage Load8, Accessor SlotDefaultsImage Load16, Accessor TailDefaultsImage Load8]

accessorName :: Accessor -> String
accessorName (Accessor image operation) = "xm1_" ++ imagePart ++ "_" ++ operationPart
  where
    imagePart = case image of
      BankImage -> "bank"
      SaveImage -> "save"
      SlotDefaultsImage -> "slot_defaults"
      TailDefaultsImage -> "tail_defaults"
    operationPart = case operation of
      Load8 -> "load8"
      Load16 -> "load16"
      Store8 -> "store8"
      Store16 -> "store16"

storing :: Accessor -> Bool
storing (Accessor _ operation) = operation `elem` [Store8, Store16]

accessorCode :: Accessor -> [A.Instruction]
accessorCode (Accessor image operation) =
  [ A.MoveWide always r12 (base .&. 0xffff)
  , A.MoveTop always r12 (base `shiftR` 16)
  , transfer
  , A.BranchExchange always False A.lr
  ]
  where
    base = case image of
      BankImage -> bankBase
      SaveImage -> saveBase
      SlotDefaultsImage -> slotDefaultsBase
      TailDefaultsImage -> tailDefaultsBase
    indexed = A.OffsetRegister True (A.Reg 0) A.LSL 0
    transfer = case operation of
      Load8 -> A.Load always A.Byte False (A.Reg 0) r12 indexed A.OffsetOnly
      Load16 -> A.Load always A.Half False (A.Reg 0) r12 indexed A.OffsetOnly
      Store8 -> A.Store always A.Byte (A.Reg 1) r12 indexed A.OffsetOnly
      Store16 -> A.Store always A.Half (A.Reg 1) r12 indexed A.OffsetOnly

-- | The assembled accessor words: movw/movt r12 with the base, the transfer, `bx lr`.
accessorWords :: Accessor -> [Word32]
accessorWords (Accessor image operation) = base ++ [transfer, 0xe12fff1e]
  where
    base = case image of
      BankImage -> [0xe30ccc64, 0xe340c096]
      SaveImage -> [0xe30ecc00, 0xe340c089]
      SlotDefaultsImage -> [0xe30bc804, 0xe340c07c]
      TailDefaultsImage -> [0xe30bc074, 0xe340c081]
    transfer = case operation of
      Load8 -> 0xe7dc0000
      Load16 -> 0xe19c00b0
      Store8 -> 0xe7cc1000
      Store16 -> 0xe18c10b0

always :: A.Cond
always = A.Cond 14

r12 :: A.Reg
r12 = A.Reg 12

move :: Int -> Int -> A.Instruction
move destination source = A.DataProcessing always A.MOV False (A.Reg destination) (A.Reg 0) (A.ShiftedImmediate (A.Reg source) A.LSL 0)

propertiesSuffix, slotsSuffix :: String
propertiesSuffix = "14property_table10PROPERTIES"
slotsSuffix = "14property_table5SLOTS"

symbolKind :: C.ByteString -> SymbolKind
symbolKind text = case C.unpack text of
  "T" -> GlobalText
  "t" -> LocalText
  "A" -> AbsoluteSymbol
  _ -> OtherSection

deriveArtifact :: [Property] -> B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact properties bytes symbolBytes = do
  let candidates = carrierCandidates properties
  require (not (null candidates)) (PlacementRejected NoOwnedRun)
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
  carrier@(Carrier _ (GuestAddress carrierHigh)) <- case [c | c@(Carrier (GuestAddress low) _) <- candidates, low == start] of
    [c] -> Right c
    _ -> Left (PlacementRejected (CarrierNotCandidate (GuestAddress start)))
  require (end >= start && end <= carrierHigh && end .&. 3 == 0
    && toInteger (B.length bytes) == toInteger (end - start)) ExtentRejected
  let absolutes = sortOn fst [(F.symbolName entry, F.symbolAddress entry) | entry <- symbols, kindIs [AbsoluteSymbol] entry]
      GuestAddress getterExitAddress = getterExit
      GuestAddress setterExitAddress = setterExit
  require (absolutes == [("xm1_getter_exit", getterExitAddress), ("xm1_setter_exit", setterExitAddress)]) SymbolRejected
  forM_ symbols $ \entry -> unless (kindIs [AbsoluteSymbol] entry) $
    require (kindIs [GlobalText, LocalText] entry && F.symbolAddress entry >= start
      && toInteger (F.symbolAddress entry) + toInteger (F.symbolSize entry) <= toInteger end) ExtentRejected
  let getterStubAddress = start
      setterStubAddress = start + 0x14
      accessorAddresses = zip accessors [start + 0x2c + 0x10 * index | index <- [0 ..]]
  placed "xm1_getter_stub" getterStubAddress 0x14
  placed "xm1_setter_stub" setterStubAddress 0x18
  forM_ accessorAddresses $ \(accessor, address) -> placed (accessorName accessor) address 0x10
  getAddress <- at "xm1_property_get"
  setAddress <- at "xm1_property_set"
  let tableNamed suffix = case [entry | entry <- symbols, suffix `isSuffixOf` F.symbolName entry] of
        [entry] -> Right entry
        _ -> Left SymbolRejected
  tableSymbol <- tableNamed propertiesSuffix
  slotSymbol <- tableNamed slotsSuffix
  let functions =
        sortOn functionStart
          [ FunctionExtent (FunctionName (F.symbolName entry)) (GuestAddress (F.symbolAddress entry)) (GuestAddress (F.symbolAddress entry + F.symbolSize entry))
          | entry <- symbols, F.symbolSize entry > 0, entry `notElem` [tableSymbol, slotSymbol], kindIs [GlobalText, LocalText] entry
          ]
      disjoint = and (zipWith (\(FunctionExtent _ _ a) (FunctionExtent _ b _) -> a <= b) functions (drop 1 functions))
  require (disjoint && all (\(FunctionExtent _ (GuestAddress low) (GuestAddress high)) -> low .&. 3 == 0 && high .&. 3 == 0) functions) ExtentRejected
  require (all (`elem` map functionStart functions) [GuestAddress getAddress, GuestAddress setAddress]) SymbolRejected
  let word address = U.word32At bytes (fromIntegral (address - start))
      wordsAt address count = map word (take count [address, address + 4 ..])
      code address count = [A.decode a (word a) | a <- take count [address, address + 4 ..]]
  require (code getterStubAddress 5 == [move 4 12, move 0 12, A.Branch always True getAddress, move 12 4, A.Branch always False getterExitAddress])
    (EncodingRejected (StubEncoding GetterStub))
  require (code setterStubAddress 6 == [move 0 4, move 1 5, move 2 6, move 3 8, A.Branch always True setAddress, A.Branch always False setterExitAddress])
    (EncodingRejected (StubEncoding SetterStub))
  forM_ accessorAddresses $ \(accessor, address) ->
    require (code address 4 == accessorCode accessor && wordsAt address 4 == accessorWords accessor) (EncodingRejected (AccessorEncoding accessor))
  EncodedTables entries slots <- either (const (Left SlotTableRejected)) Right (encodeTables (map propertyClass properties))
  let tableAddress = F.symbolAddress tableSymbol
      slotAddress = F.symbolAddress slotSymbol
      slotBytes = 4 * fromIntegral (length slots)
  require (F.symbolSize tableSymbol == 2580 && tableAddress .&. 3 == 0 && length properties == 645) TableRejected
  require ([EncodedEntry (word (tableAddress + 4 * index)) | index <- [0 .. 644]] == entries) TableRejected
  require (F.symbolSize slotSymbol == slotBytes && slotAddress .&. 3 == 0) SlotTableRejected
  require ([EncodedSlot (word address) | address <- take (length slots) [slotAddress, slotAddress + 4 ..]] == slots) SlotTableRejected
  let layout = CodeLayout
        { layoutBase = GuestAddress start
        , layoutFunctions = functions
        , layoutData = [(GuestAddress tableAddress, GuestAddress (tableAddress + 2580)), (GuestAddress slotAddress, GuestAddress (slotAddress + slotBytes))]
        , layoutStoreAccessors = [GuestAddress address | (accessor, address) <- accessorAddresses, storing accessor]
        , layoutRoots = [GuestAddress getterStubAddress, GuestAddress setterStubAddress]
        , layoutTails = [(GuestAddress (getterStubAddress + 0x10), getterExit), (GuestAddress (setterStubAddress + 0x14), setterExit)]
        }
  proof <- either (Left . CodeRejected) Right (admitCode layout bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) carrier (StubAddresses (GuestAddress getterStubAddress) (GuestAddress setterStubAddress)) functions (GuestAddress tableAddress) (GuestAddress slotAddress) proof)

-- | Undefined-instruction fill for the carrier past the linked bytes.
fillWord :: Word32
fillWord = 0xe7f000f0

-- | The staged image4 for the classified properties: the preceding-word check, the table patch,
-- the reference scan over `scanned` with the reviewed data sites, the refusal of every larger
-- candidate run by those checks, the carrier write and the check that every other byte of image4 is unchanged.
stagePlacement :: [Property] -> Image -> [(ScanImage, Image)] -> LinkedArtifact -> Either ArtifactError StagedPlacement
stagePlacement properties image4@(Image _ _ base original) scanned artifact = do
  let carrier@(Carrier (GuestAddress low) _) = artifactCarrier artifact
      linked = linkedBytes artifact
  preceding <- placementStep (checkPreceding image4 carrier)
  patch@(TablePatch patched) <- placementStep (derivePatch image4 properties carrier (artifactStubs artifact))
  let exempt = S.fromList [wordAddress entry | entry <- patched, wordSide entry == SetterTable]
      scan run = partitionHits scanned dataSites (scanReferences run scanned exempt)
      (refused, exempted) = scan carrier
      admits run = do
        _ <- checkPreceding image4 run
        case fst (scan run) of
          [] -> Right ()
          hits -> Left (CarrierReferenced hits)
  unless (null refused) (Left (PlacementRejected (CarrierReferenced refused)))
  passedOver <- mapM (\run -> either (Right . PassedOver run) (const (Left (PlacementRejected (LargerCarrierAdmissible run)))) (admits run))
    (takeWhile (/= carrier) (carrierCandidates properties))
  let offset address = fromIntegral (address - base)
      carrierOffset = offset low
      fillCount = (carrierBytes carrier - B.length linked) `div` 4
      carried = linked <> B.concat (replicate fillCount (U.putWord32 fillWord))
      replace at replacement bytes = B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes
      withCarrier = replace carrierOffset carried original
      staged = foldl (\bytes (TableWord _ _ (GuestAddress address) _ (GuestAddress target)) -> replace (offset address) (U.putWord32 target) bytes) withCarrier patched
      restored = foldl (\bytes (TableWord _ _ (GuestAddress address) _ _) -> replace (offset address) (B.take 4 (B.drop (offset address) original)) bytes)
        (replace carrierOffset (B.take (carrierBytes carrier) (B.drop carrierOffset original)) staged) patched
  require (B.length linked `mod` 4 == 0 && B.length carried == carrierBytes carrier && B.length staged == B.length original && restored == original
    && B.take (carrierBytes carrier) (B.drop carrierOffset staged) == carried
    && all (\(TableWord _ _ (GuestAddress address) _ (GuestAddress target)) -> U.word32At staged (offset address) == target) patched) StagingRejected
  pure (StagedPlacement preceding patch exempted refused passedOver staged)
  where
    placementStep = either (Left . PlacementRejected) Right

stageImage4 :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImage4
stageImage4 source artifact = StagedImage4 source artifact <$> stagePlacement (sourceProperties source) (sourceImage4 source) (sourceScanned source) artifact

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
candidateContractValue (StagedCandidate staged@(StagedImage4 source artifact (StagedPlacement preceding patch@(TablePatch patched) exempted refused passedOver image)) candidate) = object
  [ "schema" .= (1 :: Int)
  , "qualification" .= ("conditional-owned-property-service-stage-2" :: String)
  , "source_sha256" .= M.fromList [(roleLabel role, identityText digest) | (role, digest) <- sourceIdentities source]
  , "linked_sha256" .= identityText (linkedIdentity artifact)
  , "symbols_sha256" .= identityText (symbolIdentity artifact)
  , "linked_bytes" .= B.length (linkedBytes artifact)
  , "carrier" .= object ["start" .= low, "end" .= high, "bytes" .= carrierBytes carrier, "fill_words" .= ((carrierBytes carrier - B.length (linkedBytes artifact)) `div` 4), "fill_word" .= fillWord]
  , "preceding" .= object ["address" .= (low - 4), "word" .= preceding, "instruction" .= show (A.decode (low - 4) preceding)]
  , "passed_over_carriers" .=
      [ object
          [ "start" .= runLow, "end" .= runHigh, "bytes" .= carrierBytes run
          , "refusal" .= case refusal of
              PrecedingNotTransfer word -> object ["kind" .= ("preceding-not-transfer" :: String), "word" .= word, "instruction" .= show (A.decode (runLow - 4) word)]
              PrecedingMissing -> object ["kind" .= ("preceding-missing" :: String)]
              CarrierReferenced hits -> object ["kind" .= ("referenced" :: String), "hits" .= [object ["image" .= scanLabel label, "site" .= site, "word" .= word, "reference" .= show reference, "listing" .= hitListing hit] | hit@(ScanHit label (GuestAddress site) word reference) <- hits]]
              other -> object ["kind" .= show other]
          ]
      | PassedOver run@(Carrier (GuestAddress runLow) (GuestAddress runHigh)) refusal <- passedOver
      ]
  , "stubs" .= object ["getter" .= getterAddress, "setter" .= setterAddress, "getter_exit" .= getterExitAddress, "setter_exit" .= setterExitAddress]
  , "patch" .= object
      [ "getter_table" .= getterTableAddress, "setter_table" .= setterTableAddress
      , "getter_words" .= patchCount GetterTable patch, "setter_words" .= patchCount SetterTable patch
      , "table_words_unchanged" .= length [() | address <- tableWords, wordAt image4Bytes address == wordAt image address]
      , "words" .=
          [ object ["table" .= sideLabel side, "property" .= identifier, "address" .= address, "original" .= original, "patched" .= target]
          | TableWord side (PropertyId identifier) (GuestAddress address) (GuestAddress original) (GuestAddress target) <- patched
          ]
      ]
  , "scan" .= object
      [ "images" .=
          [ object ["image" .= scanLabel label, "base" .= imageBase scannedImage, "bytes" .= B.length (imageBytes scannedImage), "sha256" .= identityText (identity (imageBytes scannedImage)), "instructions" .= (scanDepth label == InstructionsAndWords)]
          | (label, scannedImage) <- sourceScanned source
          ]
      , "references" .= (["branch", "word", "movw-movt", "pc-relative"] :: [String])
      , "refused_hits" .= length refused
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
          | ExemptedHit site hit@(ScanHit _ _ _ reference) rejected <- exempted
          , let GuestAddress siteAt = siteAddress site
          ]
      ]
  , "classification" .= object ["summary" .= summaryValue (sourceProperties source), "encoding" .= encodingDescription, "properties" .= map classValue (sourceProperties source)]
  , "stack" .= object
      [ "entries" .= [frameValue frame | frame <- proofFrames proof]
      , "stub_max_depth_bytes" .= object ["getter" .= depth getterAddress, "setter" .= depth setterAddress]
      ]
  , "owned_instruction_count" .= S.size (proofInstructions proof)
  , "literal_words" .= [address | GuestAddress address <- S.toList (proofLiterals proof)]
  , "staged_image4_sha256" .= identityText (identity (stagedBytes staged))
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  , "open_premises" .=
      ([ "loads are not confined: owned code may read any address; the native oracle confines reads"
       , "stack depths are bytes below the stock getter or setter frame's sp at the arm; the stock frames supply the depth above it"
       , "the stock getter prologue saved r4 and r5, and the stock setter holds property, value and bank in r4-r6 and the slot in r8 at the arm"
       , "the code policy checks sp, r11 and the return address at every exit; other callee-saved registers follow AAPCS unproved"
       , "the reference scan reads image4, image5, plain, loader A and loader B as instructions and words, and image2 and image3 (the FONT_ICON.FDL window extracted from the DAT) as words; it does not cover computed addresses"
       ] :: [String])
  ]
  where
    carrier@(Carrier (GuestAddress low) (GuestAddress high)) = artifactCarrier artifact
    StubAddresses (GuestAddress getterAddress) (GuestAddress setterAddress) = artifactStubs artifact
    GuestAddress getterExitAddress = getterExit
    GuestAddress setterExitAddress = setterExit
    GuestAddress getterTableAddress = getterTableBase
    GuestAddress setterTableAddress = setterTableBase
    Image _ _ imageBase4 image4Bytes = sourceImage4 source
    tableWords = [table + 4 * index | table <- [getterTableAddress, setterTableAddress], index <- [0 .. 644]]
    wordAt bytes address = U.word32At bytes (fromIntegral (address - imageBase4))
    proof = artifactProof artifact
    names = M.fromList [(entry, name) | FunctionExtent (FunctionName name) entry _ <- artifactFunctions artifact]
    depth address = maybe 0 (\(StackOffset offset) -> negate offset) (M.lookup (GuestAddress address) (proofComposed proof))
    frameValue (EntryFrame entry@(GuestAddress address) (StackOffset own) calls) = object
      [ "entry" .= address
      , "name" .= M.lookup entry names
      , "own_depth_bytes" .= negate own
      , "max_depth_bytes" .= depth address
      , "calls" .= [object ["site" .= site, "target" .= target, "sp_offset" .= at, "tail" .= (kind == TailCall)] | CallEdge (GuestAddress site) (GuestAddress target) (StackOffset at) kind <- calls]
      ]
    sideLabel side = case side of
      GetterTable -> "getter" :: String
      SetterTable -> "setter"
    reasonText reason = case reason of
      HighEntropyData -> "high-entropy-data" :: String
