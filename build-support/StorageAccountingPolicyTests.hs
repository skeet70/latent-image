{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Text (Text)
import StorageAccountingPolicy
import StorageAccountingTypes

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

checkEq :: (Eq a, Show a) => String -> a -> a -> IO ()
checkEq label expected actual =
  check (expected == actual) (label ++ ": expected " ++ show expected ++ ", got " ++ show actual)

checkRight :: Show problem => String -> Either problem () -> IO ()
checkRight label result =
  case result of
    Right () -> pure ()
    Left problem -> ioError (userError (label ++ ": expected success, got " ++ show problem))

checkLeft :: Show problem => String -> Either problem () -> IO ()
checkLeft label result =
  case result of
    Left _ -> pure ()
    Right () -> ioError (userError (label ++ ": expected rejection"))

onlyResult :: [BudgetResult] -> BudgetResult
onlyResult [result] = result
onlyResult results = error ("expected exactly one budget result, got " ++ show (length results))

verdictOf :: [BudgetResult] -> BudgetVerdict
verdictOf = budgetVerdict . onlyResult

observedBytesOf :: [BudgetResult] -> ByteCount
observedBytesOf = budgetObservedBytes . onlyResult

observedCopiesOf :: [BudgetResult] -> CopyCount
observedCopiesOf = budgetObservedCopies . onlyResult

nowValue :: Integer
nowValue = 1000000

now :: EpochSeconds
now = EpochSeconds nowValue

staleTime :: EpochSeconds
staleTime = EpochSeconds (nowValue - 3601)

staleBoundaryTime :: EpochSeconds
staleBoundaryTime = EpochSeconds (nowValue - 3600)

futureTime :: EpochSeconds
futureTime = EpochSeconds (nowValue + 1)

baseConfig :: AccountingConfig
baseConfig =
  AccountingConfig
    { accountingSchemaVersion = 1
    , accountingScanRoots = [ScanRoot (RootId "data") (StoragePath "/") 8]
    , accountingFilesystemPaths = [StoragePath "/data"]
    , accountingReserveBytes = ByteCount 1000
    , accountingCommandTimeout = Seconds 30
    , accountingMaximumAge = Seconds 3600
    , accountingBudgets = []
    }

withBudgets :: [ProducerBudget] -> AccountingConfig
withBudgets budgets = baseConfig { accountingBudgets = budgets }

producerBudget :: Text -> [FilePath] -> Maybe Integer -> [(FilePath, Text)] -> Maybe Integer -> ProducerBudget
producerBudget pid paths bytes scopes copies =
  ProducerBudget
    { producerId = ProducerId pid
    , producerPaths = map StoragePath paths
    , producerByteBudget = fmap ByteCount bytes
    , producerCopyScopes = [CopyScope (StoragePath parent) (CopyPrefix prefix) | (parent, prefix) <- scopes]
    , producerCopyBudget = fmap CopyCount copies
    }

directory :: FilePath -> Integer -> DirectoryMeasurement
directory path bytes = DirectoryMeasurement (StoragePath path) (ByteCount bytes)

denied :: FilePath -> CoverageIssue
denied path = CoverageIssue (StoragePath path) PermissionDenied "permission denied"

healthyFilesystem :: FilesystemMeasurement
healthyFilesystem =
  FilesystemMeasurement
    { filesystemId = FilesystemId "fs-data"
    , filesystemProbePath = StoragePath "/data"
    , filesystemMeasuredAt = now
    , filesystemTotalBytes = ByteCount 100000
    , filesystemUsedBytes = ByteCount 40000
    , filesystemAvailableBytes = ByteCount 60000
    }

baseSnapshot :: AccountingSnapshot
baseSnapshot =
  AccountingSnapshot
    { snapshotSchemaVersion = 1
    , snapshotConfigurationDigest = ConfigurationDigest "config-digest"
    , snapshotStartedAt = now
    , snapshotCompletedAt = now
    , snapshotDirectories = []
    , snapshotFilesystems = [healthyFilesystem]
    , snapshotBtrfsCapacity = []
    , snapshotCoverageIssues = []
    , snapshotFilesystemOnly = False
    }

snapshotAt :: EpochSeconds -> AccountingSnapshot
snapshotAt instant =
  baseSnapshot
    { snapshotStartedAt = instant
    , snapshotCompletedAt = instant
    , snapshotFilesystems = [healthyFilesystem { filesystemMeasuredAt = instant }]
    }

withDirectories :: [DirectoryMeasurement] -> AccountingSnapshot
withDirectories directories = baseSnapshot { snapshotDirectories = directories }

withDirectoriesAndIssues :: [DirectoryMeasurement] -> [CoverageIssue] -> AccountingSnapshot
withDirectoriesAndIssues directories issues =
  baseSnapshot { snapshotDirectories = directories, snapshotCoverageIssues = issues }

stateWith :: AccountingSnapshot -> AccountingState
stateWith snapshot =
  AccountingState
    { stateSchemaVersion = 1
    , stateCurrent = snapshot
    , statePrevious = Nothing
    , stateGrowth = GrowthReport { growthCompatibleBaseline = False, growthFilesystems = [], growthDirectories = [] }
    , stateBudgets = []
    , stateLowSpace = False
    }

stateWithVerdicts :: AccountingSnapshot -> [BudgetResult] -> Bool -> AccountingState
stateWithVerdicts snapshot verdicts lowSpace =
  (stateWith snapshot) { stateBudgets = verdicts, stateLowSpace = lowSpace }

budgetResult :: Text -> Integer -> Integer -> BudgetVerdict -> BudgetResult
budgetResult pid bytes copies verdict =
  BudgetResult
    { budgetProducer = ProducerId pid
    , budgetObservedBytes = ByteCount bytes
    , budgetObservedCopies = CopyCount copies
    , budgetVerdict = verdict
    , budgetProblems = []
    }

testValidation :: IO ()
testValidation = do
  checkRight "valid budget"
    (validateProducerBudgets (withBudgets [producerBudget "cache" ["/data/cache"] (Just 100) [("/data/copies", "img-")] (Just 3)]))
  checkLeft "empty producer id"
    (validateProducerBudgets (withBudgets [producerBudget "" ["/data/a"] Nothing [] Nothing]))
  checkLeft "whitespace producer id"
    (validateProducerBudgets (withBudgets [producerBudget "   " ["/data/a"] Nothing [] Nothing]))
  checkLeft "duplicate producer id"
    (validateProducerBudgets (withBudgets [producerBudget "same" ["/data/a"] Nothing [] Nothing, producerBudget "same" ["/data/b"] Nothing [] Nothing]))
  checkLeft "relative producer path"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["relative"] Nothing [] Nothing]))
  checkLeft "non-canonical producer path"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/../a"] Nothing [] Nothing]))
  checkLeft "duplicate producer paths"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a", "/data/a"] Nothing [] Nothing]))
  checkLeft "nested producer paths"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a", "/data/a/b"] Nothing [] Nothing]))
  checkLeft "zero byte budget"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] (Just 0) [] Nothing]))
  checkLeft "negative copy budget"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [] (Just (-1))]))
  checkLeft "byte budget without paths"
    (validateProducerBudgets (withBudgets [producerBudget "x" [] (Just 1) [] Nothing]))
  checkLeft "copy budget without scopes"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [] (Just 1)]))
  checkLeft "relative copy parent"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [("copies", "img-")] (Just 1)]))
  checkLeft "copy prefix with slash"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [("/copies", "a/b")] (Just 1)]))
  checkRight "overlapping copy scopes allowed"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [("/copies", ""), ("/copies", "img-")] (Just 2)]))
  checkRight "unbudgeted producer validates"
    (validateProducerBudgets (withBudgets [producerBudget "x" ["/data/a"] Nothing [] Nothing]))
  checkRight "copy scopes can own the whole byte budget"
    (validateProducerBudgets (withBudgets [producerBudget "copies" [] (Just 100) [("/copies", "img-")] Nothing]))

testEvaluation :: IO ()
testEvaluation = do
  let copyResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "a-"), ("/copies", "b-")] (Just 2)])
          (withDirectories [directory "/copies" 100, directory "/copies/a-1" 10, directory "/copies/a-2" 10, directory "/copies/b-1" 10])
  checkEq "aggregate copies verdict" OverBudget (verdictOf copyResults)
  checkEq "aggregate copies count" (CopyCount 3) (observedCopiesOf copyResults)

  let byteResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/one", "/data/two"] (Just 100) [] Nothing])
          (withDirectories [directory "/data/one" 60, directory "/data/two" 60])
  checkEq "aggregate bytes verdict" OverBudget (verdictOf byteResults)
  checkEq "aggregate bytes count" (ByteCount 120) (observedBytesOf byteResults)

  let topmostResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data"] (Just 150) [] Nothing])
          (withDirectories [directory "/data" 100, directory "/data/child" 60])
  checkEq "topmost parent verdict" WithinBudget (verdictOf topmostResults)
  checkEq "topmost parent bytes" (ByteCount 100) (observedBytesOf topmostResults)

  let boundaryResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/a"] (Just 150) [] Nothing])
          (withDirectories [directory "/data" 999, directory "/data/a" 100, directory "/data/a-other" 100])
  checkEq "segment boundary verdict" WithinBudget (verdictOf boundaryResults)
  checkEq "segment boundary bytes" (ByteCount 100) (observedBytesOf boundaryResults)

  let missingResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/missing"] (Just 100) [] Nothing])
          (withDirectories [])
  checkEq "missing coverage verdict" CoverageIncomplete (verdictOf missingResults)
  checkEq "missing coverage bytes" (ByteCount 0) (observedBytesOf missingResults)

  let lowerBoundResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/a", "/data/missing"] (Just 50) [] Nothing])
          (withDirectoriesAndIssues [directory "/data/a" 100] [denied "/data/missing"])
  checkEq "over budget lower bound verdict" OverBudget (verdictOf lowerBoundResults)
  checkEq "over budget lower bound bytes" (ByteCount 100) (observedBytesOf lowerBoundResults)

  let overlapResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/a", "/data/a/b"] (Just 120) [] Nothing])
          (withDirectories [directory "/data/a" 100, directory "/data/a/b" 60])
  checkEq "overlapping producer paths verdict" WithinBudget (verdictOf overlapResults)
  checkEq "overlapping producer paths bytes" (ByteCount 100) (observedBytesOf overlapResults)

  let duplicateResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data"] (Just 150) [] Nothing])
          (withDirectories [directory "/data" 100, directory "/data" 100])
  checkEq "duplicate rows verdict" CoverageIncomplete (verdictOf duplicateResults)
  checkEq "duplicate rows bytes" (ByteCount 100) (observedBytesOf duplicateResults)

  let immediateResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "")] (Just 1)])
          (withDirectories [directory "/copies" 100, directory "/copies/one" 10, directory "/copies/one/nested" 5, directory "/copies/two" 10])
  checkEq "copy immediate children verdict" OverBudget (verdictOf immediateResults)
  checkEq "copy immediate children count" (CopyCount 2) (observedCopiesOf immediateResults)

  let prefixResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
          (withDirectories [directory "/copies" 100, directory "/copies/img-1" 10, directory "/copies/img-2" 10, directory "/copies/other" 10])
  checkEq "copy prefix verdict" OverBudget (verdictOf prefixResults)
  checkEq "copy prefix count" (CopyCount 2) (observedCopiesOf prefixResults)

  let measuredZeroResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
          (withDirectories [directory "/copies" 100, directory "/copies/other" 10])
  checkEq "measured zero copy verdict" WithinBudget (verdictOf measuredZeroResults)
  checkEq "measured zero copy count" (CopyCount 0) (observedCopiesOf measuredZeroResults)

  let emptyScopeResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
          (withDirectories [directory "/copies" 100])
  checkEq "measured empty copy scope verdict" WithinBudget (verdictOf emptyScopeResults)
  checkEq "empty copy scope count" (CopyCount 0) (observedCopiesOf emptyScopeResults)

  let shallowConfig = (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
        { accountingScanRoots = [ScanRoot (RootId "data") (StoragePath "/") 1] }
      shallowResults = evaluateBudgets shallowConfig (withDirectories [directory "/copies" 100])
  checkEq "unobserved copy children are incomplete" CoverageIncomplete (verdictOf shallowResults)

  let ancestorIssueResults = evaluateBudgets
        (withBudgets [producerBudget "data" ["/data/cache"] (Just 100) [] Nothing])
        (withDirectoriesAndIssues [directory "/data/cache" 10] [denied "/data"])
  checkEq "ancestor scan failures invalidate byte coverage" CoverageIncomplete (verdictOf ancestorIssueResults)

  let unmeasuredScopeResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
          (withDirectories [])
  checkEq "unmeasured copy scope verdict" CoverageIncomplete (verdictOf unmeasuredScopeResults)
  checkEq "unmeasured copy scope count" (CopyCount 0) (observedCopiesOf unmeasuredScopeResults)

  let unmeasuredParentResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", "img-")] (Just 1)])
          (withDirectories [directory "/copies/img-1" 10, directory "/copies/img-2" 10])
  checkEq "unmeasured parent lower bound verdict" OverBudget (verdictOf unmeasuredParentResults)
  checkEq "unmeasured parent lower bound count" (CopyCount 2) (observedCopiesOf unmeasuredParentResults)

  let copyBytesResults =
        evaluateBudgets
          (withBudgets [producerBudget "looklab" ["/checkout"] (Just 150) [("/var/tmp", "looklab-")] Nothing])
          (withDirectories [directory "/checkout" 100, directory "/var/tmp" 500, directory "/var/tmp/looklab-1" 100, directory "/var/tmp/looklab-2" 100])
  checkEq "copy bytes join producer paths verdict" OverBudget (verdictOf copyBytesResults)
  checkEq "copy bytes join producer paths bytes" (ByteCount 300) (observedBytesOf copyBytesResults)

  let copyOnceResults =
        evaluateBudgets
          (withBudgets [producerBudget "omp" ["/home/u/.omp"] (Just 150) [("/home/u/.omp/wt", "copy-")] (Just 1)])
          (withDirectories [directory "/home/u/.omp" 100, directory "/home/u/.omp/wt" 60, directory "/home/u/.omp/wt/copy-1" 60])
  checkEq "copy under producer path verdict" WithinBudget (verdictOf copyOnceResults)
  checkEq "copy under producer path bytes" (ByteCount 100) (observedBytesOf copyOnceResults)
  checkEq "copy under producer path count" (CopyCount 1) (observedCopiesOf copyOnceResults)

  let noBudgetResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data"] Nothing [] Nothing])
          (withDirectories [directory "/data" 100])
  checkEq "no budget complete verdict" NoBudget (verdictOf noBudgetResults)

  let noBudgetIncompleteResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data/missing"] Nothing [] Nothing])
          (withDirectories [])
  checkEq "no budget incomplete verdict" CoverageIncomplete (verdictOf noBudgetIncompleteResults)

  let filesystemOnlyResults =
        evaluateBudgets
          (withBudgets [producerBudget "data" ["/data"] (Just 100) [] Nothing])
          ((withDirectories [directory "/data" 10]) { snapshotFilesystemOnly = True })
  checkEq "filesystem only verdict" CoverageIncomplete (verdictOf filesystemOnlyResults)
  checkEq "filesystem only bytes" (ByteCount 0) (observedBytesOf filesystemOnlyResults)

  let scopeOverlapResults =
        evaluateBudgets
          (withBudgets [producerBudget "copies" [] Nothing [("/copies", ""), ("/copies", "img-")] (Just 2)])
          (withDirectories [directory "/copies" 100, directory "/copies/img-1" 10, directory "/copies/img-2" 10, directory "/copies/other" 10])
  checkEq "overlapping copy scopes verdict" OverBudget (verdictOf scopeOverlapResults)
  checkEq "overlapping copy scopes count" (CopyCount 3) (observedCopiesOf scopeOverlapResults)

testCheck :: IO ()
testCheck = do
  let healthyBudget = producerBudget "data" ["/data"] (Just 200) [] Nothing
      healthyConfig = withBudgets [healthyBudget]
      healthySnapshot = withDirectories [directory "/data" 100]
      expected = ConfigurationDigest "config-digest"
  checkRight "healthy state" (checkAccounting healthyConfig expected now (stateWith healthySnapshot))
  checkLeft "over budget state" (checkAccounting healthyConfig expected now (stateWith (withDirectories [directory "/data" 300])))
  checkLeft "coverage incomplete state" (checkAccounting (withBudgets [producerBudget "data" ["/data/missing"] (Just 200) [] Nothing]) expected now (stateWith (withDirectories [])))
  checkLeft "unbudgeted producer state" (checkAccounting (withBudgets [producerBudget "data" ["/data"] Nothing [] Nothing]) expected now (stateWith healthySnapshot))
  checkLeft "empty budget state" (checkAccounting (withBudgets []) expected now (stateWith healthySnapshot))
  checkLeft "stale state" (checkAccounting healthyConfig expected now (stateWith ((snapshotAt staleTime) { snapshotDirectories = [directory "/data" 100] })))
  checkLeft "future state" (checkAccounting healthyConfig expected now (stateWith ((snapshotAt now) { snapshotCompletedAt = futureTime, snapshotDirectories = [directory "/data" 100] })))
  checkLeft "wrong digest state" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotConfigurationDigest = ConfigurationDigest "other" })))
  checkLeft "filesystem only state" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystemOnly = True })))
  checkLeft "no filesystem measurement" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [] })))
  checkLeft "low space state" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemAvailableBytes = ByteCount 500 }] })))
  checkLeft "malformed state schema" (checkAccounting healthyConfig expected now ((stateWith healthySnapshot) { stateSchemaVersion = 2 }))
  checkLeft "malformed snapshot schema" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotSchemaVersion = 2 })))
  checkLeft "cached healthy verdict forgery" (checkAccounting healthyConfig expected now (stateWithVerdicts (withDirectories [directory "/data" 300]) [budgetResult "data" 0 0 WithinBudget] False))
  checkLeft "cached low space forgery" (checkAccounting healthyConfig expected now (stateWithVerdicts ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemAvailableBytes = ByteCount 500 }] }) [] False))
  checkRight "persisted verdicts ignored" (checkAccounting healthyConfig expected now (stateWithVerdicts healthySnapshot [budgetResult "data" 0 0 OverBudget] True))
  checkLeft "duplicate directory rows" (checkAccounting healthyConfig expected now (stateWith (withDirectories [directory "/data" 100, directory "/data" 100])))
  checkLeft "non-absolute directory row" (checkAccounting healthyConfig expected now (stateWith (withDirectories [directory "relative" 100])))
  checkLeft "previous snapshot schema mismatch" (checkAccounting healthyConfig expected now ((stateWith healthySnapshot) { statePrevious = Just ((snapshotAt now) { snapshotSchemaVersion = 2 }) }))
  checkLeft "previous snapshot after current" (checkAccounting healthyConfig expected now ((stateWith healthySnapshot) { statePrevious = Just ((snapshotAt now) { snapshotCompletedAt = futureTime }) }))
  checkLeft "config validation error" (checkAccounting (withBudgets [producerBudget "same" ["/data"] (Just 200) [] Nothing, producerBudget "same" ["/data/other"] (Just 200) [] Nothing]) expected now (stateWith healthySnapshot))
  checkLeft "future filesystem measurement" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemMeasuredAt = futureTime }] })))
  checkLeft "stale filesystem measurement" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemMeasuredAt = staleTime }] })))
  checkLeft "invalid filesystem numbers" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemTotalBytes = ByteCount 0 }] })))
  checkLeft "inconsistent filesystem capacity" (checkAccounting healthyConfig expected now (stateWith (healthySnapshot { snapshotFilesystems = [healthyFilesystem { filesystemAvailableBytes = ByteCount 60001 }] })))
  checkLeft "duplicate filesystem identity" (checkAccounting healthyConfig expected now (stateWith (healthySnapshot { snapshotFilesystems = [healthyFilesystem, healthyFilesystem] })))
  checkRight "low space boundary" (checkAccounting healthyConfig expected now (stateWith ((withDirectories [directory "/data" 100]) { snapshotFilesystems = [healthyFilesystem { filesystemTotalBytes = ByteCount 100000, filesystemUsedBytes = ByteCount 99000, filesystemAvailableBytes = ByteCount 1000 }] })))
  checkRight "stale boundary" (checkAccounting healthyConfig expected now (stateWith ((snapshotAt staleBoundaryTime) { snapshotDirectories = [directory "/data" 100] })))

main :: IO ()
main = do
  testValidation
  testEvaluation
  testCheck
  putStrLn "storage-accounting-policy-tests passed"