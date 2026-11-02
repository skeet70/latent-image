module Replacement.StartupConsumers.Source
  ( SourceModel, SourceFiles (..), SourceError (..), Store (..), AllocationKind (..)
  , AllocationInput (..), AllocationResult (..), Reservation (..), ReservationState, reservationState, reservationWord
  , ByteCount (..), TaskId, taskId, taskIndex
  , deriveSource, primaryStores, secondaryStores, sourceFiles, reserve
  , familyEntryAddress, familyEpilogueAddress, familySourceSites, globalSourceSites, commonSourceSites, atomicSourceSites
  , CodeSpan (..), ownedCodeSpans
  , MessageAccessKind (..), MessageAccess (..), messageAccesses
  , CopyPart (..), CopyAccess (..), copyAccess
  ) where

import Data.Bits ((.&.), complement)
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.LoaderPrefixWitness as P
import qualified Emulation.PrimaryInitializerSource as A
import qualified Replacement.KernelInstall.Artifact as K
import Replacement.KernelInstall.Wire (Address (..), WordValue (..))
import Replacement.StartupConsumers.Wire (Family (..))

data SourceFiles = SourceFiles
  { sourceDat :: B.ByteString, sourceImage4 :: B.ByteString
  , sourceLoaderA :: B.ByteString, sourceLoaderB :: B.ByteString
  } deriving (Eq, Show)
data SourceError = CanonicalSourceRejected | PrimarySourceRejected | SecondarySourceRejected deriving (Eq, Show)
data Store = Store { storeAddress :: Address, storeValue :: WordValue } deriving (Eq, Show)
data SourceModel = SourceModel SourceFiles [Store] [Store]

sourceFiles :: SourceModel -> SourceFiles
sourceFiles (SourceModel files _ _) = files
primaryStores :: SourceModel -> [Store]
primaryStores (SourceModel _ stores _) = stores
secondaryStores :: SourceModel -> [Store]
secondaryStores (SourceModel _ _ stores) = stores

deriveSource :: SourceFiles -> Either SourceError SourceModel
deriveSource files = do
  _ <- either (const (Left CanonicalSourceRejected)) Right
    (K.canonicalSource (sourceDat files) (sourceImage4 files) (sourceLoaderA files) (sourceLoaderB files))
  (primary,_) <- either (const (Left PrimarySourceRejected)) Right (A.publications (sourceLoaderA files))
  secondary <- either (const (Left SecondarySourceRejected)) Right (I.originalStores (sourceLoaderB files))
  pure (SourceModel files
    [Store (Address address) (WordValue (A.publicationValue publication)) | publication <- primary, let A.Guest address = A.publicationGuest publication]
    [Store (Address address) (WordValue value) | I.ExpectedStore (P.Guest address) value <- secondary])

newtype TaskId = TaskId Word32 deriving (Eq, Ord, Show)
taskId :: Word32 -> Maybe TaskId
taskId value | value >= 1 && value <= 120 = Just (TaskId value)
             | otherwise = Nothing
taskIndex :: TaskId -> Word32
taskIndex (TaskId value) = value-1

data AllocationKind = TaskAllocation | StorageAllocation deriving (Eq, Ord, Show)
newtype ByteCount = ByteCount Word32 deriving (Eq, Ord, Show)
data ReservationState = Available | Occupied WordValue deriving (Eq, Show)
reservationState :: WordValue -> ReservationState
reservationState (WordValue 0) = Available
reservationState value = Occupied value
reservationWord :: ReservationState -> WordValue
reservationWord Available = WordValue 0
reservationWord (Occupied value) = value
data Reservation = Reservation { reservationBase :: Address, reservationSize :: ByteCount, reservationBusy :: ReservationState } deriving (Eq, Show)
data AllocationInput = AllocationInput
  { allocationKind :: AllocationKind, allocationRequested :: ByteCount
  , allocationCursor :: Address, allocationRecord :: Reservation
  } deriving (Eq, Show)
data AllocationResult
  = Allocated { allocatedPointer :: Address, allocatedCursor :: Address, allocatedRecord :: Reservation }
  | AllocationSizeRejected | AllocationBusyRejected
  deriving (Eq, Show)

reserve :: AllocationInput -> AllocationResult
reserve input
  | reservationBase record /= Address 0 =
      if reservationSize record /= allocationRequested input then AllocationSizeRejected
      else if reservationBusy record /= Available then AllocationBusyRejected
      else Allocated (reservationBase record) (allocationCursor input) (record {reservationBusy = Occupied (WordValue 1)})
  | cursor-pool+requested > 0x4b000 = AllocationSizeRejected
  | otherwise = Allocated (Address pointer) (Address (cursor + (requested .&. complement 3)))
      (Reservation (Address pointer) (allocationRequested input) (Occupied (WordValue 1)))
  where
    record = allocationRecord input
    ByteCount requested = allocationRequested input
    Address cursor = allocationCursor input
    pool = case allocationKind input of TaskAllocation -> 0x40a10200; StorageAllocation -> 0x409c4460
    pointer = case allocationKind input of TaskAllocation -> cursor .&. complement 0x40000000; StorageAllocation -> cursor

familyEntryAddress :: Family -> Address
familyEntryAddress family = Address $ case family of
  TaskWrapper -> 0x725dd8; TaskReserve -> 0x725c6c; StorageReserve -> 0x723470
  MessageConstruct -> 0x724cdc; MessageSend -> 0x72503c; MessageReceive -> 0x725318

familyEpilogueAddress :: Family -> Address
familyEpilogueAddress family = Address $ case family of
  TaskWrapper -> 0x726094; TaskReserve -> 0x725dcc; StorageReserve -> 0x7235cc
  MessageConstruct -> 0x724f04; MessageSend -> 0x725210; MessageReceive -> 0x725420

familySourceSites :: Family -> [Address]
familySourceSites family = map Address $ case family of
  TaskWrapper -> [0x725df8,0x725e1c,0x725e34,0x725e50,0x725e74,0x725e78
    ,0x725f08,0x725f0c,0x725f68,0x725f6c,0x725f78,0x725f7c,0x725fa8,0x725fac,0x725fb4,0x725fb8
    ,0x726000,0x726004,0x726024,0x726028,0x726094]
  TaskReserve -> [0x725c94,0x725c98,0x725ca4,0x725ca8,0x725cc0,0x725cc4
    ,0x725cd4,0x725cf0,0x725d0c,0x725d1c,0x725d30,0x725d44,0x725d54,0x725d64,0x725d7c
    ,0x725d94,0x725d98,0x725da0,0x725dac,0x725db0,0x725dc4,0x725dc8,0x725dcc]
  StorageReserve -> [0x723498,0x72349c,0x7234a8,0x7234ac,0x7234c4,0x7234c8
    ,0x7234d8,0x7234f4,0x723510,0x723520,0x723534,0x723548,0x723558,0x723564,0x72357c
    ,0x723594,0x723598,0x7235a0,0x7235ac,0x7235b0,0x7235c4,0x7235c8,0x7235cc]
  MessageConstruct -> [0x724d00,0x724d20,0x724d24,0x724d70,0x724d98,0x724d9c,0x724de4,0x724dec
    ,0x724e04,0x724e20,0x724e34,0x724e38,0x724ea4,0x724ea8,0x724ee4,0x724ee8,0x724f04]
  MessageSend -> [0x725064,0x725068,0x725078,0x725084,0x7250c4,0x7250cc,0x7250d0,0x725100,0x725104
    ,0x72514c,0x725154,0x72515c,0x725164,0x72517c,0x725180,0x725190,0x725194
    ,0x7251b4,0x7251b8,0x7251f0,0x7251f4,0x725210]
  MessageReceive -> [0x725344,0x725348,0x725378,0x72537c,0x7253c0,0x7253c8,0x7253d0,0x7253d8
    ,0x7253e0,0x7253f8,0x7253fc,0x725414,0x725418,0x725420]

globalSourceSites :: [Address]
globalSourceSites = map Address [0x69cdf8,0x725a04,0x42634,0x40a08,0x40a0c,0x40a10
  ,0xd0a44,0xd0a48,0xd0a4c,0xd0aac,0xd0430,0xd0ab0
  ,0x7235d8,0x7235f4,0x7235f8,0x7235fc,0x723600,0x723604,0x723608,0x412c4,0x412c8,0x427b8]

commonSourceSites :: [Address]
commonSourceSites = map Address [0x7233ac,0x7233b0]

atomicSourceSites :: [Address]
atomicSourceSites = map Address [0x427d4,0x427e4,0x42800,0x42810,0xd27d4,0xd27e4,0xd2800,0xd2810]

data MessageAccessKind = MessageRead | MessageWrite deriving (Eq, Show)
data MessageAccess = MessageAccess {messageAccessInstruction :: Address,messageAccessKind :: MessageAccessKind}
messageAccesses :: [MessageAccess]
messageAccesses =
  [MessageAccess (Address site) MessageRead | site <- [0x4b430,0xdb3f0,0x48fb8,0xd8f78]]
  ++ [MessageAccess (Address site) MessageWrite | site <- [0x48f04,0xd8ec4,0x48f58,0xd8f18]]

data CodeSpan = CodeSpan {codeSpanStart :: Address,codeSpanSize :: ByteCount}
ownedCodeSpans :: [CodeSpan]
ownedCodeSpans = [CodeSpan (Address 0x69cdf8) (ByteCount 0x18c),CodeSpan (Address 0x725a04) (ByteCount 0x150)]

data CopyPart = CopyPrefix | CopyWords | CopyLargeTail | CopyHalfWords | CopyAlignedTail | CopyBytes deriving (Eq, Ord, Enum, Bounded, Show)
data CopyAccess = CopyAccess {copyWidth :: ByteCount,copyLoad :: Address,copyStore :: Address}
copyAccess :: CopyPart -> CopyAccess
copyAccess part = case part of
  CopyPrefix -> access 1 0x722080 0x722084
  CopyWords -> access 4 0x7220c4 0x7220c8
  CopyLargeTail -> access 1 0x7220e8 0x7220ec
  CopyHalfWords -> access 2 0x722130 0x722134
  CopyAlignedTail -> access 1 0x722150 0x722154
  CopyBytes -> access 1 0x72215c 0x722160
  where access width load store = CopyAccess (ByteCount width) (Address load) (Address store)
