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
  , WriteBasis (..), WritesPolicy (..), ReadExpectation (..), CatalogRead (..), Catalog (..)
  , TcmRegister (..), DecoderIndex (..), DecoderChoice (..), DeclaredMutation (..)
  , Piece (..), PieceFacts (..), ServiceBinding (..), Publication (..), LinkedBytes (..), ScratchBytes (..)
  , PublisherFrame (..), SubmitCall (..), StackDepth (..), TaskNumber (..), StackBytes (..), TaskStart (..)
  , AllocationPosition (..), allocationPositionName, AllocationWords (..), AllocationOwner (..), PoolSpan (..), PoolAllocation (..)
  , ComposedWrites (..), composedWritesName, CarrierWritePolicy (..), OwnedFrame (..), HeapFacts (..)
  , Refusal (..), CatalogProblem (..), ManifestProblem (..), ApplicationProblem (..)
  ) where

import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word16, Word32, Word64)

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

data PieceKind = FirstStage | Record2Link | PrimaryPublisher | LoaderOrchestrationB | LoaderStartup | TaskSubmission | TaskStorage
  | IdleTask | RootTask | Image5Producer | PlainProducer | GeneralHeap | ApplicationPump | Task1Actor | Image5Initializer | TaskConstructors
  deriving (Eq, Ord, Show, Enum, Bounded)

pieceKindName :: PieceKind -> Text
pieceKindName kind = case kind of
  FirstStage -> "first-stage"
  Record2Link -> "record2-link"
  PrimaryPublisher -> "primary-publisher"
  LoaderOrchestrationB -> "loader-orchestration-b"
  LoaderStartup -> "loader-startup"
  TaskSubmission -> "task-submission"
  TaskStorage -> "task-storage"
  IdleTask -> "idle-task"
  RootTask -> "root-task"
  Image5Producer -> "image5-producer"
  PlainProducer -> "plain-producer"
  GeneralHeap -> "general-heap"
  ApplicationPump -> "application-pump"
  Task1Actor -> "task1-actor"
  Image5Initializer -> "image5-initializer"
  TaskConstructors -> "task-constructors"

data Area = FirstStageArea | SecondaryStageArea | LoaderPrefixArea | InterfaceInitializerArea | CriticalPairArea | PrimaryInitializerArea
  | LoaderOrchestrationAArea | LoaderOrchestrationBArea | PhaseDispatcherArea | KernelInstallArea | TaskSubmissionArea | TaskStorageArea
  | IdleTaskArea | RootTaskArea | Image5ProducerArea | PlainProducerArea | GeneralHeapArea | ApplicationPumpArea | Task1ActorArea | Image5InitializerArea | TaskConstructorsArea
  deriving (Eq, Ord, Show, Enum, Bounded)

areaName :: Area -> Text
areaName area = case area of
  FirstStageArea -> "first-stage"
  SecondaryStageArea -> "secondary-stage"
  LoaderPrefixArea -> "loader-prefix"
  InterfaceInitializerArea -> "interface-initializer"
  CriticalPairArea -> "critical-pair"
  PrimaryInitializerArea -> "primary-initializer"
  LoaderOrchestrationAArea -> "loader-orchestration-a"
  LoaderOrchestrationBArea -> "loader-orchestration-b"
  PhaseDispatcherArea -> "phase-dispatcher"
  KernelInstallArea -> "kernel-install"
  TaskSubmissionArea -> "task-submission"
  TaskStorageArea -> "task-storage"
  IdleTaskArea -> "idle-task"
  RootTaskArea -> "root-task"
  Image5ProducerArea -> "image5-producer"
  PlainProducerArea -> "plain-producer"
  GeneralHeapArea -> "general-heap"
  ApplicationPumpArea -> "application-pump"
  Task1ActorArea -> "task1-actor"
  Image5InitializerArea -> "image5-initializer"
  TaskConstructorsArea -> "task-constructors"

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

data WritesPolicy = EnumeratedWrites [WriteRange] WriteBasis
  deriving (Eq, Show)

-- | A TCM region register value as the CPU stores it after normalization.
newtype TcmRegister = TcmRegister Word32 deriving (Eq, Show)

newtype DecoderIndex = DecoderIndex Word16 deriving (Eq, Show)

newtype DecoderChoice = DecoderChoice Word16 deriving (Eq, Show)

-- | A mutation the composite guard admits once in a role: a guest TCM region
-- register write (core, bank, previous, next, writer) or a memory decoder
-- commit (decoder, previous, next, writer core, writer).
data DeclaredMutation
  = TcmRegionWrite Core TcmBank TcmRegister TcmRegister Pc Citation
  | DecoderCommit DecoderIndex DecoderChoice DecoderChoice Core Pc Citation
  deriving (Eq, Show)

-- | The digest a read must have in each role, resolved by the contract from the role's own images:
-- a decoded target, or the NAND bytes of the record-5 media window.
data ReadExpectation = ExpectTarget Target | ExpectRecord5Media deriving (Eq, Show)

-- | A read compares byte for byte across roles unless it states a difference or an expectation.
data CatalogRead = CatalogRead
  { readLabel :: Label
  , readBoundary :: Label
  , readAddress :: Word32
  , readLength :: Word32
  , readDiffers :: Maybe Text
  , readExpect :: Maybe ReadExpectation
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

data PublisherFrame = PublisherFrame { publisherEntry :: Pc, publisherCallerSp :: Guest } deriving (Eq, Show)

-- | Bytes below an incoming SP.
newtype StackDepth = StackDepth Word32 deriving (Eq, Show)

-- | One task-submission tail call of the owned dispatcher: its core and stack depth at the branch.
data SubmitCall = SubmitCall { submitCore :: Core, submitStack :: StackDepth } deriving (Eq, Show)

newtype TaskNumber = TaskNumber Word32 deriving (Eq, Show)

newtype StackBytes = StackBytes Word32 deriving (Eq, Show)

-- | A task the owned dispatcher submits: its id, entry and requested stack.
data TaskStart = TaskStart { startTask :: TaskNumber, startEntry :: Pc, startStack :: StackBytes } deriving (Eq, Show)

-- | Where a normalized word points in an allocation: from its base (inclusive) or up to its end (inclusive).
data AllocationPosition = AllocationBase | AllocationTop deriving (Eq, Show)

allocationPositionName :: AllocationPosition -> Text
allocationPositionName AllocationBase = "base"
allocationPositionName AllocationTop = "top"

-- | Words of a read that hold pool addresses: every stride bytes from the offset.
data AllocationWords = AllocationWords
  { allocationRead :: Label, allocationOffset :: Word32, allocationStride :: Word32, allocationPosition :: AllocationPosition }
  deriving (Eq, Show)

-- | A read whose base word, every stride bytes from the offset, names an allocation the core reserved.
data AllocationOwner = AllocationOwner { ownerCore :: Core, ownerRead :: Label, ownerOffset :: Word32, ownerStride :: Word32 }
  deriving (Eq, Show)

-- | Physical [start, end) of the task reservation pool.
data PoolSpan = PoolSpan Word32 Word32 deriving (Eq, Show)

-- | Each role's allocations come from the storage-record and cursor reads; the listed words and the SP of
-- each stack-top return boundary compare as (task, offset) within that role's own allocations, and each
-- core's tasks, ordered by base, must match between the roles.
data PoolAllocation = PoolAllocation
  { poolSpan :: PoolSpan, poolRecords :: Label, poolCursor :: Label
  , poolWords :: [AllocationWords], poolStackTops :: [Label], poolOwners :: [AllocationOwner], poolCitation :: Citation }
  deriving (Eq, Show)

-- | What the composed role may write into a carrier after attach.
data ComposedWrites = WritesCompared | WritesNone | WritesModelBusOnly deriving (Eq, Show)

composedWritesName :: ComposedWrites -> Text
composedWritesName WritesCompared = "compared"
composedWritesName WritesNone = "none"
composedWritesName WritesModelBusOnly = "model-bus"

-- | A carrier's post-attach write rule: the composed role's admitted kinds, and the code spans whose
-- retained-role CPU writes into it are the stock code the piece replaces.
data CarrierWritePolicy = CarrierWritePolicy { policySpan :: Span, policyComposed :: ComposedWrites, policyRetainedExcluded :: [Span] }
  deriving (Eq, Show)

-- | One owned function's frame: entry, low SP offset and its calls as (target, SP offset at the call).
data OwnedFrame = OwnedFrame { ownedEntry :: Word32, ownedLow :: Int, ownedCalls :: [(Word32, Int)] } deriving (Eq, Show)

-- | The heap's linked image, public and core entries, frames, arena [base, end) and carrier write rules.
data HeapFacts = HeapFacts
  { heapLinked :: LinkedBytes, heapEntries :: [Word32], heapFrames :: [OwnedFrame], heapArena :: (Word32, Word32)
  , heapCarrierWrites :: [CarrierWritePolicy] }
  deriving (Eq, Show)

-- | Artifact values that registered joins check across pieces.
data PieceFacts
  -- | First-stage entry and the linked record-1 image.
  = FirstStageFacts Pc LinkedBytes
  -- | Secondary entry, the core-0 bindings the critical pair consumes, the
  -- linked record-2 image, the owned initializer's scratch and the deeper of
  -- the critical pair's entry and exit frames below its caller's SP.
  | Record2LinkFacts Pc [ServiceBinding] LinkedBytes ScratchBytes StackDepth
  -- | The publisher's entry and the SP its caller frame provides, and its emitted stores.
  | PublisherFacts PublisherFrame [Publication]
  -- | The linked loader-B orchestration image.
  | LoaderOrchestrationFacts LinkedBytes
  -- | The linked loader-A and loader-B orchestration images, the dispatcher's task submissions and the tasks they start.
  | LoaderStartupFacts LinkedBytes LinkedBytes [SubmitCall] [TaskStart]
  | NoFacts
  | TaskConstructorsFacts LinkedBytes
  -- | The linked task wrapper/release image and the pool-allocation relation over its reads.
  | TaskSubmissionFacts LinkedBytes PoolAllocation
  -- | The linked task-storage transition image.
  | TaskStorageFacts LinkedBytes
  -- | The linked idle image and the stack-top return boundaries it adds to the pool-allocation relation.
  | IdleTaskFacts LinkedBytes [Label]
  -- | The linked root image and the pool-address words of its reads.
  | RootTaskFacts LinkedBytes [AllocationWords]
  -- | The linked image5 producer image.
  | Image5ProducerFacts LinkedBytes
  -- | The linked plain producer image.
  | PlainProducerFacts LinkedBytes
  -- | The general heap.
  | GeneralHeapFacts HeapFacts
  -- | The linked application pump image.
  | ApplicationPumpFacts LinkedBytes
  | Task1ActorFacts LinkedBytes
  | Image5InitializerFacts LinkedBytes
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

data ApplicationProblem
  -- | The site's map and carrier ownership disagree.
  = SiteInCarrier PieceKind Pc
  -- | Code at the PC lies in neither image4, image5 nor the plain window.
  | OutsideImages Pc
  -- | The composed word at a site outside every carrier differs from the canonical word.
  | SiteWordDiffers Pc
  | PlainWindowUnreadable
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
  | ApplicationRefused ApplicationProblem
  deriving (Eq, Show)
