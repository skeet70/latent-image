module Replacement.StartupConsumers.Source
  ( SourceModel, SourceFiles (..), SourceError (..), Store (..), AllocationKind (..)
  , AllocationInput (..), AllocationResult (..), Reservation (..), ReservationState, reservationState, reservationWord
  , ByteCount (..), TaskId, taskId, taskIndex
  , deriveSource, primaryStores, secondaryStores, sourceFiles, reserve
  ) where

import Data.Bits ((.&.), complement)
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.LoaderPrefixWitness as P
import qualified Emulation.PrimaryInitializerSource as A
import qualified Replacement.KernelInstall.Artifact as K
import Replacement.KernelInstall.Wire (Address (..), WordValue (..))

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
