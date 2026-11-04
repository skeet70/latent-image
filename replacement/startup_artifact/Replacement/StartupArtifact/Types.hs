{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupArtifact.Types
  ( Target (..), targetName, Guest (..), Span (..), span', spanLength
  , Edit, edit, editSpan, editBytes
  , PieceKind (..), pieceKindName, Area (..), areaName
  , FootprintBasis (..), footprintBasisName, Citation (..), Footprint (..), Premise (..)
  , Core (..), coreIndex, BoundaryKind (..), boundaryKindName, Compare (..), compareName
  , Label, label, labelText, Pc (..), RecordLimit, recordLimit, recordLimitCount
  , Cores, cores, coreList, physicalAddress
  , Boundary (..), CodeSpan (..), codeSpan, TcmBank (..), tcmBankName, WriteTarget (..), WriteRange (..)
  , WriteBasis (..), WritesPolicy (..), CatalogRead (..), Catalog (..)
  , Piece (..), PieceFacts (..), ServiceBinding (..), Publication (..), LinkedBytes (..), ScratchBytes (..)
  , Refusal (..), CatalogProblem (..), ManifestProblem (..)
  ) where

import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32, Word64)

data Target = Record1 | Record2 | LoaderA | LoaderB | Image4 | Image5
  deriving (Eq, Ord, Show, Enum, Bounded)

targetName :: Target -> Text
targetName target = case target of
  Record1 -> "record1"
  Record2 -> "record2"
  LoaderA -> "loader_a"
  LoaderB -> "loader_b"
  Image4 -> "image4"
  Image5 -> "image5"

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)

-- | Half-open guest range of one target.
data Span = Span { spanTarget :: Target, spanStart :: Guest, spanEnd :: Guest }
  deriving (Eq, Ord, Show)

span' :: Target -> Word32 -> Word32 -> Maybe Span
span' target start end
  | start < end = Just (Span target (Guest start) (Guest end))
  | otherwise = Nothing

spanLength :: Span -> Int
spanLength (Span _ (Guest start) (Guest end)) = fromIntegral (end - start)

-- | Uncomplemented replacement bytes for exactly the span.
data Edit = Edit Span B.ByteString deriving (Eq, Show)

edit :: Span -> B.ByteString -> Maybe Edit
edit range bytes
  | B.length bytes == spanLength range = Just (Edit range bytes)
  | otherwise = Nothing

editSpan :: Edit -> Span
editSpan (Edit range _) = range

editBytes :: Edit -> B.ByteString
editBytes (Edit _ bytes) = bytes

data PieceKind = FirstStage | Record2Link | PrimaryPublisher | LoaderOrchestrationB
  deriving (Eq, Ord, Show, Enum, Bounded)

pieceKindName :: PieceKind -> Text
pieceKindName kind = case kind of
  FirstStage -> "first-stage"
  Record2Link -> "record2-link"
  PrimaryPublisher -> "primary-publisher"
  LoaderOrchestrationB -> "loader-orchestration-b"

data Area = FirstStageArea | SecondaryStageArea | LoaderPrefixArea | InterfaceInitializerArea | CriticalPairArea | PrimaryInitializerArea
  | LoaderOrchestrationBArea
  deriving (Eq, Ord, Show, Enum, Bounded)

areaName :: Area -> Text
areaName area = case area of
  FirstStageArea -> "first-stage"
  SecondaryStageArea -> "secondary-stage"
  LoaderPrefixArea -> "loader-prefix"
  InterfaceInitializerArea -> "interface-initializer"
  CriticalPairArea -> "critical-pair"
  PrimaryInitializerArea -> "primary-initializer"
  LoaderOrchestrationBArea -> "loader-orchestration-b"

data FootprintBasis = AdmissionRead | RuntimePremise deriving (Eq, Ord, Show)

footprintBasisName :: FootprintBasis -> Text
footprintBasisName AdmissionRead = "admission-read"
footprintBasisName RuntimePremise = "runtime-premise"

-- | @path:line@ or @path:first-last@ of the source the footprint is read from.
newtype Citation = Citation Text deriving (Eq, Ord, Show)

data Footprint = Footprint Span FootprintBasis Citation deriving (Eq, Show)

newtype Premise = Premise Text deriving (Eq, Show)

data Core = Core0 | Core1 deriving (Eq, Ord, Show, Enum, Bounded)

coreIndex :: Core -> Int
coreIndex Core0 = 0
coreIndex Core1 = 1

data BoundaryKind = EntryBoundary | ReturnBoundary | SiteBoundary deriving (Eq, Show)

boundaryKindName :: BoundaryKind -> Text
boundaryKindName EntryBoundary = "entry"
boundaryKindName ReturnBoundary = "return"
boundaryKindName SiteBoundary = "site"

data Compare = CompareSequence | ComparePresence deriving (Eq, Show)

compareName :: Compare -> Text
compareName CompareSequence = "sequence"
compareName ComparePresence = "presence"

-- | @<piece kind>/<name>@ with a lowercase kebab name.
data Label = Label PieceKind Text deriving (Eq, Ord, Show)

label :: PieceKind -> Text -> Either Refusal Label
label kind name
  | not (T.null name) && T.all (\c -> isAsciiLower c || isDigit c || c == '-') name
      && T.head name /= '-' && T.last name /= '-' = Right (Label kind name)
  | otherwise = Left (CatalogRefused kind (InvalidLabel name))

labelText :: Label -> Text
labelText (Label kind name) = pieceKindName kind <> "/" <> name

newtype Pc = Pc Word32 deriving (Eq, Ord, Show)

newtype RecordLimit = RecordLimit Int deriving (Eq, Show)

recordLimit :: Int -> Maybe RecordLimit
recordLimit count
  | count >= 1 = Just (RecordLimit count)
  | otherwise = Nothing

recordLimitCount :: RecordLimit -> Int
recordLimitCount (RecordLimit count) = count

-- | Nonempty set of cores a boundary may hit on.
newtype Cores = Cores (Set.Set Core) deriving (Eq, Show)

cores :: [Core] -> Maybe Cores
cores [] = Nothing
cores listed = Just (Cores (Set.fromList listed))

coreList :: Cores -> [Core]
coreList (Cores set) = Set.toAscList set

data Boundary = Boundary
  { boundaryLabel :: Label
  , boundaryKind :: BoundaryKind
  , boundaryCores :: Cores
  , boundaryComposedPc :: Pc
  , boundaryRetainedPc :: Maybe Pc
  , boundaryStop :: Bool
  , boundaryCompare :: Compare
  , boundaryRecordLimit :: RecordLimit
  -- | Dropped, with the reads stopping at it, when a manifest piece claims this area.
  , boundaryYieldsTo :: Maybe Area
  } deriving (Eq, Show)

-- | Owned instructions as fetched, [start, end), in the low or the 0x40000000 alias spelling.
data CodeSpan = CodeSpan Word32 Word32 deriving (Eq, Ord, Show)

codeSpan :: Word32 -> Word32 -> Maybe CodeSpan
codeSpan start end
  | start < end = Just (CodeSpan start end)
  | otherwise = Nothing

-- | The physical spelling: [0x40000000,0x60000000) clears 0x40000000.
physicalAddress :: Word64 -> Word64
physicalAddress address
  | address >= 0x40000000 && address < 0x60000000 = address - 0x40000000
  | otherwise = address

data TcmBank = DataBank | InstructionBank deriving (Eq, Ord, Show, Enum, Bounded)

tcmBankName :: TcmBank -> Text
tcmBankName DataBank = "data"
tcmBankName InstructionBank = "instruction"

-- | 'PhysicalTarget' binds both cores' partitions; 'CorePhysicalTarget' one core's.
data WriteTarget = PhysicalTarget | CorePhysicalTarget Core | TcmTarget Core TcmBank deriving (Eq, Ord, Show)

-- | Physical addresses or TCM bank offsets, [start, end).
data WriteRange = WriteRange WriteTarget Word64 Word64 deriving (Eq, Ord, Show)

data WriteBasis = Proved | Premised Premise deriving (Eq, Show)

data WritesPolicy
  = EnumeratedWrites [WriteRange] WriteBasis
  -- | Destinations are not enumerated; the observer derives the policy.
  | UnenumeratedWrites Premise
  deriving (Eq, Show)

data CatalogRead = CatalogRead
  { readLabel :: Label
  , readBoundary :: Label
  , readAddress :: Word32
  , readLength :: Word32
  , readDiffers :: Maybe Text
  } deriving (Eq, Show)

data Catalog = Catalog
  { catalogBoundaries :: [Boundary]
  , catalogOwnedCode :: [CodeSpan]
  , catalogWrites :: WritesPolicy
  , catalogReads :: [CatalogRead]
  } deriving (Eq, Show)

-- | A core-0 interface word the record-2 link consumes: root offset and the retained code it must name.
data ServiceBinding = ServiceBinding { bindingOffset :: Word32, bindingTarget :: Word32 } deriving (Eq, Show)

-- | One emitted store of the owned publisher.
data Publication = Publication { publicationGuest :: Guest, publicationValue :: Word32 } deriving (Eq, Show)

-- | A linked flat image and its llvm-nm symbol table.
data LinkedBytes = LinkedBytes B.ByteString B.ByteString deriving (Eq, Show)

-- | Stack bytes a piece's owned callee uses below its caller's SP.
newtype ScratchBytes = ScratchBytes Word32 deriving (Eq, Show)

-- | Artifact values that registered joins check across pieces.
data PieceFacts
  -- | First-stage entry and the linked record-1 image.
  = FirstStageFacts Pc LinkedBytes
  -- | Secondary entry, the core-0 bindings the critical pair consumes, the
  -- linked record-2 image and the owned initializer's scratch.
  | Record2LinkFacts Pc [ServiceBinding] LinkedBytes ScratchBytes
  | PublisherFacts [Publication]
  -- | The linked loader-B orchestration image.
  | LoaderOrchestrationFacts LinkedBytes
  | NoFacts
  deriving (Eq, Show)

data Piece = Piece
  { pieceKind :: PieceKind
  , pieceAreas :: Set.Set Area
  , pieceCarriers :: [Span]
  , pieceEdits :: [Edit]
  , pieceFootprint :: [Footprint]
  , piecePremises :: [Premise]
  , pieceCatalog :: Catalog
  , pieceFacts :: PieceFacts
  } deriving (Eq, Show)

data CatalogProblem
  = InvalidLabel Text
  | DuplicateLabel Label
  | DuplicateComposedPc Pc
  | DuplicateRetainedPc Pc
  | OwnedBoundaryOutsideOwnedCode Label
  | ReadBoundaryUnknown Label
  | ReadBoundaryDoesNotStop Label
  | TcmCoreWithoutBoundary Core
  -- | Owned code shared with the named piece.
  | OwnedCodeOverlap PieceKind
  -- | A read at a boundary without a retained PC must state how it differs.
  | ReadNeedsDiffers Label
  | TooManyBoundaries Core
  deriving (Eq, Show)

data ManifestProblem
  = ManifestMalformed Text
  | ManifestSchemaUnsupported
  | ManifestUnknownKey Text
  | ManifestUnknownKind Text
  | GuardAttachUnknown Text
  -- | The attach boundary lacks a retained PC or names more than one core.
  | GuardAttachUnsuitable Text
  -- | The named @guard.attach_state@ entry is malformed or inconsistent with the composition.
  | AttachStateRefused Text
  deriving (Eq, Show)

-- | 'FootprintOverlap' names the footprint owner, then the carrier owner.
data Refusal
  = ManifestRefused ManifestProblem
  | AdapterRefused PieceKind Text
  | CanonicalRefused
  | TargetGeometryRefused Target
  | DuplicatePieceKind PieceKind
  | SharedArea PieceKind PieceKind Area
  | CarrierOverlap PieceKind PieceKind Span
  | FootprintOverlap PieceKind PieceKind Span
  | JoinSpanNotConflicting Text Span
  -- | Join name, then the failing host check.
  | HostProofFailed Text Text
  | EditOutsideCarriers PieceKind Span
  | PieceChangesOutsideTargets PieceKind
  | PieceChangesUndecodedImage PieceKind Target
  | CompositionUnchanged
  | CatalogRefused PieceKind CatalogProblem
  | ComposedUnparsed
  | ComposedDiffMismatch Target
  | ComposedRestoreMismatch
  deriving (Eq, Show)
