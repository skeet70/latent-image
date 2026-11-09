{-# LANGUAGE OverloadedStrings #-}
-- | Shared extents are not deducted from allocated-footprint budgets.
module StorageAccountingPolicy
  ( checkAccounting
  , evaluateBudgets
  , validateProducerBudgets
  ) where

import Data.List (intercalate, isPrefixOf)
import qualified Data.Map.Strict as Map
import Data.Maybe (isJust, isNothing)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import System.FilePath (isAbsolute, joinPath, normalise, splitDirectories, takeFileName)
import StorageAccountingTypes

byteCountValue :: ByteCount -> Integer
byteCountValue (ByteCount value) = value

copyCountValue :: CopyCount -> Integer
copyCountValue (CopyCount value) = value

epochValue :: EpochSeconds -> Integer
epochValue (EpochSeconds value) = value

secondsValue :: Seconds -> Integer
secondsValue (Seconds value) = value

producerIdValue :: ProducerId -> Text
producerIdValue (ProducerId value) = value

rootIdValue :: RootId -> Text
rootIdValue (RootId value) = value

filesystemIdValue :: FilesystemId -> String
filesystemIdValue (FilesystemId value) = T.unpack value

copyPrefixValue :: CopyPrefix -> Text
copyPrefixValue (CopyPrefix value) = value

storagePathValue :: StoragePath -> FilePath
storagePathValue (StoragePath path) = path

canonicalPathProblem :: StoragePath -> Maybe String
canonicalPathProblem (StoragePath raw)
  | null raw = Just "path is empty"
  | not (isAbsolute raw) = Just ("path is not absolute: " ++ raw)
  | normalise raw /= raw = Just ("path is not lexically canonical: " ++ raw)
  | any (`elem` [".", ".."]) (splitDirectories raw) || '\0' `elem` raw = Just ("invalid path component: " ++ raw)
  | otherwise = Nothing

pathProblem :: String -> StoragePath -> [String]
pathProblem label path =
  case canonicalPathProblem path of
    Nothing -> []
    Just problem -> [label ++ ": " ++ problem]

pathComponents :: FilePath -> [FilePath]
pathComponents = splitDirectories . normalise

containsFilePath :: FilePath -> FilePath -> Bool
containsFilePath parent child = pathComponents parent `isPrefixOf` pathComponents child

containsStoragePath :: StoragePath -> StoragePath -> Bool
containsStoragePath (StoragePath parent) (StoragePath child) = containsFilePath parent child

isImmediateChild :: StoragePath -> StoragePath -> Bool
isImmediateChild (StoragePath parent) (StoragePath child) =
  let parentComponents = pathComponents parent
      childComponents = pathComponents child
  in length childComponents == length parentComponents + 1
       && parentComponents `isPrefixOf` childComponents

properAncestors :: FilePath -> [FilePath]
properAncestors path =
  let components = pathComponents path
  in [joinPath (take count components) | count <- [1 .. length components - 1]]

childName :: FilePath -> Text
childName = T.pack . takeFileName . normalise

prefixMatches :: CopyPrefix -> FilePath -> Bool
prefixMatches (CopyPrefix prefix) path = prefix `T.isPrefixOf` childName path

data RowIndex = RowIndex
  { rowIndexBytes :: Map.Map FilePath ByteCount
  , rowIndexDuplicates :: [FilePath]
  }

indexRows :: [DirectoryMeasurement] -> RowIndex
indexRows measurements =
  let grouped = Map.fromListWith (++) [(normalise path, [bytes]) | DirectoryMeasurement (StoragePath path) bytes <- measurements]
      bytesByPath = Map.map minimum grouped
      duplicates = [path | (path, values) <- Map.toList grouped, length values > 1]
  in RowIndex bytesByPath duplicates

duplicateProducerProblems :: [ProducerBudget] -> [String]
duplicateProducerProblems budgets =
  [ "producer id is duplicated: " ++ T.unpack pid
  | (pid, count) <- Map.toList (Map.fromListWith (+) [(producerIdValue (producerId budget), 1 :: Int) | budget <- budgets])
  , count > 1
  ]

duplicatePathProblemsFor :: String -> [StoragePath] -> [String]
duplicatePathProblemsFor label paths =
  [ label ++ " is duplicated: " ++ path
  | (path, count) <- Map.toList (Map.fromListWith (+) [(normalise (storagePathValue path), 1 :: Int) | path <- paths])
  , count > 1
  ]

overlapPathProblemsFor :: String -> [StoragePath] -> [String]
overlapPathProblemsFor label paths =
  [ label ++ " overlaps " ++ storagePathValue second ++ ": " ++ storagePathValue first
  | (firstIndex, first) <- zip [0 :: Int ..] paths
  , (secondIndex, second) <- zip [0 :: Int ..] paths
  , firstIndex < secondIndex
  , normalise (storagePathValue first) /= normalise (storagePathValue second)
  , containsStoragePath first second || containsStoragePath second first
  ]

validateCopyScope :: String -> CopyScope -> [String]
validateCopyScope label (CopyScope parent prefix) =
  pathProblem (label ++ "copy scope parent") parent
    ++ [ label ++ "copy name prefix must not contain '/': " ++ T.unpack (copyPrefixValue prefix)
       | T.any (== '/') (copyPrefixValue prefix)
       ]

validateProducer :: ProducerBudget -> [String]
validateProducer budget =
  let pid = producerIdValue (producerId budget)
      label = "producer " ++ show pid ++ ": "
      idProblems = [label ++ "producer id is empty" | T.null (T.strip pid)]
      paths = producerPaths budget
      pathProblems = concatMap (pathProblem (label ++ "producer path")) paths
      duplicatePathProblems = duplicatePathProblemsFor (label ++ "producer path") paths
      overlapPathProblems = overlapPathProblemsFor (label ++ "producer path") paths
      byteProblems = case producerByteBudget budget of
        Just (ByteCount limit) | limit <= 0 -> [label ++ "byte budget must be positive"]
        _ -> []
      copyProblems = case producerCopyBudget budget of
        Just (CopyCount limit) | limit <= 0 -> [label ++ "copy budget must be positive"]
        _ -> []
      byteBudgetScopeProblems =
        [ label ++ "byte budget requires at least one producer path or copy scope"
        | isJust (producerByteBudget budget) && null paths && null (producerCopyScopes budget)
        ]
      scopes = producerCopyScopes budget
      copyBudgetScopeProblems =
        [ label ++ "copy budget requires at least one copy scope"
        | isJust (producerCopyBudget budget) && null scopes
        ]
      scopeProblems = concatMap (validateCopyScope label) scopes
  in concat
       [ idProblems
       , pathProblems
       , duplicatePathProblems
       , overlapPathProblems
       , byteProblems
       , copyProblems
       , byteBudgetScopeProblems
       , copyBudgetScopeProblems
       , scopeProblems
       ]

validateProducerBudgets :: AccountingConfig -> Either String ()
validateProducerBudgets config =
  case duplicateProducerProblems budgets ++ concatMap validateProducer budgets of
    [] -> Right ()
    problems -> Left (intercalate "; " problems)
 where
  budgets = accountingBudgets config

selectTopmost :: [(FilePath, ByteCount)] -> [(FilePath, ByteCount)]
selectTopmost rows =
  let measuredPaths = Set.fromList (map fst rows)
      topmost (path, _) = not (any (`Set.member` measuredPaths) (properAncestors path))
  in filter topmost rows

issuesUnder :: StoragePath -> [CoverageIssue] -> [CoverageIssue]
issuesUnder scope =
  filter (\(CoverageIssue path _ _) -> containsStoragePath scope path || containsStoragePath path scope)

describeIssue :: CoverageIssue -> String
describeIssue (CoverageIssue (StoragePath path) kind detail) =
  path ++ " (" ++ show kind ++ "): " ++ T.unpack detail

hasExactRow :: RowIndex -> StoragePath -> Bool
hasExactRow index scope =
  Map.member (normalise (storagePathValue scope)) (rowIndexBytes index)

hasAnyRowUnder :: RowIndex -> StoragePath -> Bool
hasAnyRowUnder index scope =
  any (\(path, _) -> containsStoragePath scope (StoragePath path)) (Map.toList (rowIndexBytes index))

copyDepthCovered :: AccountingConfig -> StoragePath -> Bool
copyDepthCovered config parent =
  any covered (accountingScanRoots config)
 where
  covered (ScanRoot _ root depth) =
    containsStoragePath root parent
      && length (pathComponents (storagePathValue parent)) - length (pathComponents (storagePathValue root)) + 1 <= depth

byteScopeCovered :: RowIndex -> [CoverageIssue] -> StoragePath -> Bool
byteScopeCovered index issues scope =
  hasExactRow index scope && null (issuesUnder scope issues)

byteScopeProblems :: RowIndex -> [CoverageIssue] -> StoragePath -> [String]
byteScopeProblems index issues scope
  | hasExactRow index scope = issueProblems
  | hasAnyRowUnder index scope =
      [ "producer path " ++ storagePathValue scope ++ " has no directory measurement; byte usage is a partial lower bound"
      ] ++ issueProblems
  | otherwise =
      [ "no directory measurement covers producer path " ++ storagePathValue scope ++ "; byte usage is not attributed"
      ] ++ issueProblems
 where
  issueProblems =
    [ "coverage issue under producer path " ++ storagePathValue scope ++ ": " ++ describeIssue issue
    | issue <- issuesUnder scope issues
    ]

copyScopeCovered :: AccountingConfig -> RowIndex -> [CoverageIssue] -> CopyScope -> Bool
copyScopeCovered config index issues scope =
  hasExactRow index (copyParent scope)
    && copyDepthCovered config (copyParent scope)
    && null (issuesUnder (copyParent scope) issues)

copyScopeProblems :: AccountingConfig -> RowIndex -> [CoverageIssue] -> CopyScope -> [String]
copyScopeProblems config index issues scope =
  missingParentProblems
    ++ missingChildrenProblems
    ++ [ "coverage issue under copy scope parent " ++ storagePathValue (copyParent scope) ++ ": " ++ describeIssue issue
       | issue <- issuesUnder (copyParent scope) issues
       ]
 where
  parent = copyParent scope
  missingParentProblems =
    [ "copy scope parent " ++ storagePathValue parent ++ " has no directory measurement; managed copy count is incomplete"
    | not (hasExactRow index parent)
    ]
  missingChildrenProblems =
    [ "copy scope parent " ++ storagePathValue parent ++ " is not scanned deeply enough to enumerate immediate children"
    | not (copyDepthCovered config parent)
    ]

scopeChild :: CopyScope -> FilePath -> Bool
scopeChild (CopyScope parent prefix) path =
  isImmediateChild parent (StoragePath path) && prefixMatches prefix path

copyScopeOverlapProblems :: [CopyScope] -> [String]
copyScopeOverlapProblems scopes =
  [ "copy scopes overlap: parent " ++ parent ++ ", prefixes " ++ show firstValue ++ " and " ++ show secondValue
      ++ "; overlapping copies are counted once"
  | (firstIndex, CopyScope firstParent firstPrefix) <- zip [0 :: Int ..] scopes
  , (secondIndex, CopyScope secondParent secondPrefix) <- zip [0 :: Int ..] scopes
  , firstIndex < secondIndex
  , normalise (storagePathValue firstParent) == normalise (storagePathValue secondParent)
  , let firstValue = copyPrefixValue firstPrefix
  , let secondValue = copyPrefixValue secondPrefix
  , firstValue `T.isPrefixOf` secondValue || secondValue `T.isPrefixOf` firstValue
  , let parent = storagePathValue firstParent
  ]

overBudgetProblems :: Maybe ByteCount -> Maybe CopyCount -> ByteCount -> CopyCount -> Bool -> Bool -> [String]
overBudgetProblems byteBudget copyBudget observedBytes observedCopies byteCovered copyCovered =
  byteProblems ++ copyProblems
 where
  byteProblems = case byteBudget of
    Just (ByteCount limit)
      | byteCountValue observedBytes > limit ->
          [ "allocated bytes " ++ show (byteCountValue observedBytes)
              ++ " exceed budget " ++ show limit
              ++ lowerBoundNote byteCovered
          ]
    _ -> []
  copyProblems = case copyBudget of
    Just (CopyCount limit)
      | copyCountValue observedCopies > limit ->
          [ "managed copies " ++ show (copyCountValue observedCopies)
              ++ " exceed budget " ++ show limit
              ++ lowerBoundNote copyCovered
          ]
    _ -> []
  lowerBoundNote covered =
    if covered then "" else "; observed value is a lower bound because coverage is incomplete"

producerOverlapProblems :: ProducerBudget -> [String]
producerOverlapProblems budget =
  [ "producer paths overlap: " ++ storagePathValue first ++ " and " ++ storagePathValue second
      ++ "; overlapping directory rows are counted once"
  | (firstIndex, first) <- zip [0 :: Int ..] (producerPaths budget)
  , (secondIndex, second) <- zip [0 :: Int ..] (producerPaths budget)
  , firstIndex < secondIndex
  , normalise (storagePathValue first) /= normalise (storagePathValue second)
  , containsStoragePath first second || containsStoragePath second first
  ]

rowRelevant :: ProducerBudget -> FilePath -> Bool
rowRelevant budget path =
  any (\scope -> containsStoragePath scope (StoragePath path)) (producerPaths budget)
    || any (\scope -> scopeChild scope path) (producerCopyScopes budget)

evaluateProducer :: AccountingConfig -> Bool -> [String] -> ProducerBudget -> RowIndex -> [CoverageIssue] -> BudgetResult
evaluateProducer config filesystemOnly globalProblems budget index issues =
  let pid = producerId budget
      paths = producerPaths budget
      scopes = producerCopyScopes budget
      byteBudget = producerByteBudget budget
      copyBudget = producerCopyBudget budget
      bytesByPath = rowIndexBytes index
      matchingRows =
        [ (path, bytes)
        | (path, bytes) <- Map.toList bytesByPath
        , any (\scope -> containsStoragePath scope (StoragePath path)) paths
            || any (\scope -> scopeChild scope path) scopes
        ]
      topmostRows = selectTopmost matchingRows
      observedBytes = ByteCount (sum [byteCountValue bytes | (_, bytes) <- topmostRows])
      producerPathsCovered =
        all (byteScopeCovered index issues) paths
          && (not (null paths) || not (null scopes) || isNothing byteBudget)
      copyScopesCovered =
        all (copyScopeCovered config index issues) scopes
          && (not (null scopes) || isNothing copyBudget)
      byteCovered = producerPathsCovered && copyScopesCovered
      copyCovered = copyScopesCovered
      copyPaths =
        Set.fromList
          [ path
          | scope <- scopes
          , (path, _) <- Map.toList bytesByPath
          , scopeChild scope path
          ]
      observedCopies = CopyCount (fromIntegral (Set.size copyPaths))
      bothThresholdsAbsent = isNothing byteBudget && isNothing copyBudget
      coverageProblems =
        concatMap (byteScopeProblems index issues) paths
          ++ concatMap (copyScopeProblems config index issues) scopes
      relevantCoverage =
        filesystemOnly
          || (isJust byteBudget && not byteCovered)
          || (isJust copyBudget && not copyCovered)
          || (bothThresholdsAbsent && not (byteCovered && copyCovered))
          || not (null duplicateProblems)
      overProblems = overBudgetProblems byteBudget copyBudget observedBytes observedCopies byteCovered copyCovered
      overlapProblems = producerOverlapProblems budget ++ copyScopeOverlapProblems scopes
      duplicateProblems =
        [ "duplicate directory measurement for " ++ path ++ "; counted once"
        | path <- rowIndexDuplicates index
        , rowRelevant budget path
        ]
      noBudgetProblems = [ "no byte or copy budget configured" | bothThresholdsAbsent ]
      problems =
        globalProblems ++ overlapProblems ++ duplicateProblems ++ coverageProblems ++ overProblems ++ noBudgetProblems
      verdict
        | not (null overProblems) = OverBudget
        | relevantCoverage = CoverageIncomplete
        | bothThresholdsAbsent = NoBudget
        | otherwise = WithinBudget
  in BudgetResult
       { budgetProducer = pid
       , budgetObservedBytes = observedBytes
       , budgetObservedCopies = observedCopies
       , budgetVerdict = verdict
       , budgetProblems = map T.pack problems
       }

evaluateBudgets :: AccountingConfig -> AccountingSnapshot -> [BudgetResult]
evaluateBudgets config snapshot =
  [ evaluateProducer config filesystemOnly globalProblems budget index (snapshotCoverageIssues snapshot)
  | budget <- accountingBudgets config
  ]
 where
  filesystemOnly = snapshotFilesystemOnly snapshot
  index = if filesystemOnly then RowIndex Map.empty [] else indexRows (snapshotDirectories snapshot)
  globalProblems =
    [ "filesystem-only snapshot has no directory measurements; producer budgets cannot be evaluated"
    | filesystemOnly
    ]

validateScanRoot :: ScanRoot -> [String]
validateScanRoot (ScanRoot rootId rootPath depth) =
  [ "scan root id is empty" | T.null (T.strip (rootIdValue rootId)) ]
    ++ pathProblem "scan root path" rootPath
    ++ [ "scan root depth must not be negative: " ++ show depth | depth < 0 ]

duplicateTextProblems :: String -> [Text] -> [String]
duplicateTextProblems label values =
  [ label ++ " is duplicated: " ++ T.unpack value
  | (value, count) <- Map.toList (Map.fromListWith (+) [(value, 1 :: Int) | value <- values])
  , count > 1
  ]

validateAccountingConfig :: AccountingConfig -> [String]
validateAccountingConfig config =
  concat
    [ [ "config schema version must be positive" | accountingSchemaVersion config < 1 ]
    , [ "no scan roots configured" | null (accountingScanRoots config) ]
    , concatMap validateScanRoot (accountingScanRoots config)
    , duplicateTextProblems "scan root id" [rootIdValue rootId | ScanRoot rootId _ _ <- accountingScanRoots config]
    , duplicatePathProblemsFor "scan root path" [rootPath | ScanRoot _ rootPath _ <- accountingScanRoots config]
    , [ "no filesystem paths configured" | null (accountingFilesystemPaths config) ]
    , concatMap (pathProblem "config filesystem path") (accountingFilesystemPaths config)
    , duplicatePathProblemsFor "config filesystem path" (accountingFilesystemPaths config)
    , [ "reserve bytes must not be negative" | byteCountValue (accountingReserveBytes config) < 0 ]
    , [ "command timeout must be positive" | secondsValue (accountingCommandTimeout config) <= 0 ]
    , [ "maximum snapshot age must be positive" | secondsValue (accountingMaximumAge config) <= 0 ]
    ]

snapshotTimeProblems :: AccountingConfig -> EpochSeconds -> String -> AccountingSnapshot -> [String]
snapshotTimeProblems config now label snapshot =
  [ label ++ " starts after it completes"
  | snapshotStartedAt snapshot > snapshotCompletedAt snapshot
  ]
    ++ [ label ++ " completed in the future: " ++ show (epochValue (snapshotCompletedAt snapshot))
           ++ " after check time " ++ show (epochValue now)
       | snapshotCompletedAt snapshot > now
       ]
    ++ [ label ++ " is stale: completed at " ++ show (epochValue (snapshotCompletedAt snapshot))
           ++ ", maximum age " ++ show (secondsValue (accountingMaximumAge config)) ++ " seconds"
       | snapshotCompletedAt snapshot <= now
       , epochValue now - epochValue (snapshotCompletedAt snapshot) > secondsValue (accountingMaximumAge config)
       ]

directoryProblems :: AccountingSnapshot -> [String]
directoryProblems snapshot =
  concatMap (pathProblem "directory measurement path" . directoryPath) (snapshotDirectories snapshot)
    ++ [ "directory measurement has negative allocated bytes: " ++ storagePathValue path
       | DirectoryMeasurement path bytes <- snapshotDirectories snapshot
       , byteCountValue bytes < 0
       ]
    ++ duplicatePathProblemsFor "directory measurement" (map directoryPath (snapshotDirectories snapshot))

filesystemMeasurementProblems :: AccountingConfig -> EpochSeconds -> EpochSeconds -> FilesystemMeasurement -> [String]
filesystemMeasurementProblems config now completed measurement =
  let identifier = filesystemIdValue (filesystemId measurement)
      probe = storagePathValue (filesystemProbePath measurement)
      total = byteCountValue (filesystemTotalBytes measurement)
      used = byteCountValue (filesystemUsedBytes measurement)
      available = byteCountValue (filesystemAvailableBytes measurement)
      measured = epochValue (filesystemMeasuredAt measurement)
      maximumAge = secondsValue (accountingMaximumAge config)
  in [ "filesystem " ++ identifier ++ " has non-positive total bytes" | total <= 0 ]
       ++ [ "filesystem " ++ identifier ++ " has negative used bytes" | used < 0 ]
       ++ [ "filesystem " ++ identifier ++ " has negative available bytes" | available < 0 ]
       ++ [ "filesystem " ++ identifier ++ " used bytes exceed total bytes" | used > total ]
       ++ [ "filesystem " ++ identifier ++ " available bytes exceed total bytes" | available > total ]
       ++ [ "filesystem " ++ identifier ++ " used and available bytes exceed total bytes" | used + available > total ]
       ++ [ "filesystem " ++ identifier ++ " probe path is not absolute: " ++ probe | not (isAbsolute probe) ]
       ++ [ "filesystem " ++ identifier ++ " measurement is in the future" | measured > epochValue now ]
       ++ [ "filesystem " ++ identifier ++ " measurement is after snapshot completion" | measured > epochValue completed ]
       ++ [ "filesystem " ++ identifier ++ " measurement is older than maximum age"
          | measured <= epochValue now
          , epochValue now - measured > maximumAge
          ]

filesystemProblems :: AccountingConfig -> EpochSeconds -> AccountingSnapshot -> [String]
filesystemProblems config now snapshot =
  [ "snapshot has no filesystem measurement; free space cannot be verified"
  | null (snapshotFilesystems snapshot)
  ]
    ++ concatMap (filesystemMeasurementProblems config now (snapshotCompletedAt snapshot)) (snapshotFilesystems snapshot)
    ++ duplicateTextProblems "filesystem id" [value | measurement <- snapshotFilesystems snapshot, let FilesystemId value = filesystemId measurement]

validateAccountingState :: AccountingConfig -> ConfigurationDigest -> EpochSeconds -> AccountingState -> [String]
validateAccountingState config expected now state =
  concat
    [ [ "state schema version " ++ show (stateSchemaVersion state)
          ++ " does not match config schema version " ++ show (accountingSchemaVersion config)
      | stateSchemaVersion state /= accountingSchemaVersion config
      ]
    , validateCurrentSnapshot
    , validatePreviousSnapshot
    ]
 where
  current = stateCurrent state
  validateCurrentSnapshot =
    concat
      [ [ "current snapshot schema version " ++ show (snapshotSchemaVersion current)
            ++ " does not match config schema version " ++ show (accountingSchemaVersion config)
        | snapshotSchemaVersion current /= accountingSchemaVersion config
        ]
      , [ "current snapshot configuration digest " ++ show (snapshotConfigurationDigest current)
            ++ " does not match expected digest " ++ show expected
        | snapshotConfigurationDigest current /= expected
        ]
      , [ "current snapshot is filesystem-only; directory coverage is required"
        | snapshotFilesystemOnly current
        ]
      , snapshotTimeProblems config now "current snapshot" current
      , directoryProblems current
      , filesystemProblems config now current
      ]
  validatePreviousSnapshot = case statePrevious state of
    Nothing -> []
    Just previous ->
      concat
        [ [ "previous snapshot schema version " ++ show (snapshotSchemaVersion previous)
              ++ " does not match config schema version " ++ show (accountingSchemaVersion config)
          | snapshotSchemaVersion previous /= accountingSchemaVersion config
          ]
        , [ "previous snapshot configuration digest " ++ show (snapshotConfigurationDigest previous)
              ++ " does not match expected digest " ++ show expected
              ++ " while the growth baseline is marked compatible"
          | snapshotConfigurationDigest previous /= expected
          , growthCompatibleBaseline (stateGrowth state)
          ]
        , [ "previous snapshot completed at " ++ show (epochValue (snapshotCompletedAt previous))
              ++ " after current snapshot started at " ++ show (epochValue (snapshotStartedAt current))
          | snapshotCompletedAt previous > snapshotStartedAt current
          ]
        , [ "previous snapshot completed in the future"
          | snapshotCompletedAt previous > now
          ]
        , [ "previous snapshot starts after it completes"
          | snapshotStartedAt previous > snapshotCompletedAt previous
          ]
        ]

lowSpaceProblems :: AccountingConfig -> AccountingSnapshot -> [String]
lowSpaceProblems config snapshot =
  [ "filesystem " ++ filesystemIdValue (filesystemId measurement)
      ++ " at " ++ storagePathValue (filesystemProbePath measurement)
      ++ " has " ++ show (byteCountValue (filesystemAvailableBytes measurement))
      ++ " available bytes, below reserve " ++ show (byteCountValue (accountingReserveBytes config))
  | measurement <- snapshotFilesystems snapshot
  , byteCountValue (filesystemAvailableBytes measurement) < byteCountValue (accountingReserveBytes config)
  ]

checkBudgetPolicy :: AccountingConfig -> AccountingState -> [String]
checkBudgetPolicy config state =
  [ "no producer budgets configured; accounting policy is unbudgeted"
  | null (accountingBudgets config)
  ]
    ++ concatMap resultProblem (evaluateBudgets config (stateCurrent state))
 where
  resultProblem result =
    case budgetVerdict result of
      WithinBudget -> []
      NoBudget -> [ "producer " ++ show (producerIdValue (budgetProducer result)) ++ ": no byte or copy budget configured" ]
      OverBudget -> [ "producer " ++ show (producerIdValue (budgetProducer result)) ++ ": over budget: " ++ joinProblems result ]
      CoverageIncomplete -> [ "producer " ++ show (producerIdValue (budgetProducer result)) ++ ": coverage incomplete: " ++ joinProblems result ]
  joinProblems result =
    let problems = map T.unpack (budgetProblems result)
    in if null problems then "no detail" else intercalate "; " problems

checkAccounting :: AccountingConfig -> ConfigurationDigest -> EpochSeconds -> AccountingState -> Either [String] ()
checkAccounting config expected now state =
  let budgetConfigProblems =
        case validateProducerBudgets config of
          Left problem -> [problem]
          Right () -> []
      problems =
        validateAccountingConfig config
          ++ budgetConfigProblems
          ++ validateAccountingState config expected now state
          ++ lowSpaceProblems config (stateCurrent state)
          ++ checkBudgetPolicy config state
  in if null problems then Right () else Left problems