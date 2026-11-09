{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE GeneralizedNewtypeDeriving #-}
module StorageAccountingTypes where

import Data.Aeson (FromJSON, ToJSON)
import Data.Text (Text)
import GHC.Generics (Generic)

newtype StoragePath = StoragePath FilePath deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype RootId = RootId Text deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype ProducerId = ProducerId Text deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype ByteCount = ByteCount Integer deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype ByteDelta = ByteDelta Integer deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype EpochSeconds = EpochSeconds Integer deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype Seconds = Seconds Integer deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype FilesystemId = FilesystemId Text deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype ConfigurationDigest = ConfigurationDigest Text deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype CopyCount = CopyCount Integer deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)
newtype CopyPrefix = CopyPrefix Text deriving stock (Eq, Ord, Show, Generic) deriving newtype (FromJSON, ToJSON)

data ScanRoot = ScanRoot { scanRootId :: RootId, scanRootPath :: StoragePath, scanDepth :: Int } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data CopyScope = CopyScope { copyParent :: StoragePath, copyNamePrefix :: CopyPrefix } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data ProducerBudget = ProducerBudget
  { producerId :: ProducerId
  , producerPaths :: [StoragePath]
  , producerByteBudget :: Maybe ByteCount
  , producerCopyScopes :: [CopyScope]
  , producerCopyBudget :: Maybe CopyCount
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data AccountingConfig = AccountingConfig
  { accountingSchemaVersion :: Int
  , accountingScanRoots :: [ScanRoot]
  , accountingFilesystemPaths :: [StoragePath]
  , accountingReserveBytes :: ByteCount
  , accountingCommandTimeout :: Seconds
  , accountingMaximumAge :: Seconds
  , accountingBudgets :: [ProducerBudget]
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)

data CoverageKind = PermissionDenied | MissingPath | ToolFailure | TimedOut | InvalidOutput | ScopeChanged deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data CoverageIssue = CoverageIssue { issuePath :: StoragePath, issueKind :: CoverageKind, issueDetail :: Text } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data DirectoryMeasurement = DirectoryMeasurement { directoryPath :: StoragePath, directoryAllocatedBytes :: ByteCount } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data FilesystemMeasurement = FilesystemMeasurement
  { filesystemId :: FilesystemId
  , filesystemProbePath :: StoragePath
  , filesystemMeasuredAt :: EpochSeconds
  , filesystemTotalBytes :: ByteCount
  , filesystemUsedBytes :: ByteCount
  , filesystemAvailableBytes :: ByteCount
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data BtrfsCapacity = BtrfsCapacity
  { btrfsProbePath :: StoragePath
  , btrfsDataAllocatedBytes :: ByteCount
  , btrfsDataUsedBytes :: ByteCount
  , btrfsMetadataAllocatedPhysicalBytes :: ByteCount
  , btrfsMetadataUsedPhysicalBytes :: ByteCount
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data AccountingSnapshot = AccountingSnapshot
  { snapshotSchemaVersion :: Int
  , snapshotConfigurationDigest :: ConfigurationDigest
  , snapshotStartedAt :: EpochSeconds
  , snapshotCompletedAt :: EpochSeconds
  , snapshotDirectories :: [DirectoryMeasurement]
  , snapshotFilesystems :: [FilesystemMeasurement]
  , snapshotBtrfsCapacity :: [BtrfsCapacity]
  , snapshotCoverageIssues :: [CoverageIssue]
  , snapshotFilesystemOnly :: Bool
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)

data FilesystemGrowth = FilesystemGrowth { growthFilesystem :: FilesystemId, growthElapsed :: Seconds, growthUsedBytes :: ByteDelta, growthBytesPerDay :: ByteDelta, growthSecondsToReserve :: Maybe Seconds } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data DirectoryGrowth = DirectoryGrowth { growthDirectory :: StoragePath, growthAllocatedBytes :: ByteDelta } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data GrowthReport = GrowthReport { growthCompatibleBaseline :: Bool, growthFilesystems :: [FilesystemGrowth], growthDirectories :: [DirectoryGrowth] } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data BudgetVerdict = WithinBudget | OverBudget | CoverageIncomplete | NoBudget deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data BudgetResult = BudgetResult
  { budgetProducer :: ProducerId
  , budgetObservedBytes :: ByteCount
  , budgetObservedCopies :: CopyCount
  , budgetVerdict :: BudgetVerdict
  , budgetProblems :: [Text]
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
data AccountingState = AccountingState
  { stateSchemaVersion :: Int
  , stateCurrent :: AccountingSnapshot
  , statePrevious :: Maybe AccountingSnapshot
  , stateGrowth :: GrowthReport
  , stateBudgets :: [BudgetResult]
  , stateLowSpace :: Bool
  } deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
