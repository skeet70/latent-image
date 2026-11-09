{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (myThreadId, throwTo)
import Control.Exception (IOException, catch, displayException, try)
import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (ToJSON, eitherDecodeStrict', encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (toUpper)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as Map
import Data.Maybe (catMaybes, listToMaybe, mapMaybe)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Text.Encoding.Error (lenientDecode)
import Data.Time.Clock.POSIX (getPOSIXTime)
import qualified GHC.Foreign as GHC
import GHC.Generics (Generic)
import GHC.IO.Encoding (TextEncoding, getFileSystemEncoding)
import Numeric (showHex)
import StorageAccountingPolicy (checkAccounting, evaluateBudgets, validateProducerBudgets)
import StorageAccountingRuntime (CommandOutcome (..))
import qualified StorageAccountingRuntime as Runtime
import StorageAccountingTypes
import System.Directory (canonicalizePath, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die, exitFailure)
import System.FilePath ((</>), isAbsolute, normalise, splitDirectories, takeDirectory)
import System.IO (IOMode (ReadMode), hFlush, hPutStrLn, stderr, stdout, withBinaryFile)
import System.IO.Error (isDoesNotExistError, isPermissionError)
import System.Posix.Files (FileStatus, deviceID, getFileStatus, getSymbolicLinkStatus, isDirectory, isSymbolicLink)
import System.Posix.Signals (Handler (CatchOnce), installHandler, sigTERM)
import System.Posix.Types (DeviceID)
import System.Posix.Process (executeFile)


schemaVersion :: Int
schemaVersion = 1

maxScanDepth :: Int
maxScanDepth = 8

maxConfigBytes, maxStateBytes, maxDfOutput, maxStatOutput, maxBtrfsOutput, maxDuOutput, maxIssuesPerProbe, maxDirectoryRecords, issueDetailLimit :: Int
maxConfigBytes = 4 * 1024 * 1024
maxStateBytes = 256 * 1024 * 1024
maxDfOutput = 1024 * 1024
maxStatOutput = 1024 * 1024
maxBtrfsOutput = 16 * 1024 * 1024
maxDuOutput = 256 * 1024 * 1024
maxIssuesPerProbe = 1000
maxDirectoryRecords = 500000
issueDetailLimit = 500

usageText :: String
usageText = "usage: storage-accounting sample --config FILE --state DIR [--filesystem-only] | check --config FILE --state DIR | guard --config FILE --state DIR -- COMMAND ARGS..."


data ProbeResult = ProbeResult { probeMeasurements :: [FilesystemMeasurement], probeIssues :: [CoverageIssue] }

data IssueCount = IssueCount { issueCountKind :: CoverageKind, issueCountValue :: Int } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)
data BudgetSummary = BudgetSummary
  { budgetSummaryProducer :: ProducerId
  , budgetSummaryObservedBytes :: ByteCount
  , budgetSummaryObservedCopies :: CopyCount
  , budgetSummaryVerdict :: BudgetVerdict
  , budgetSummaryProblemCount :: Int
  } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)

data SampleReport = SampleReport
  { sampleStatus :: SampleStatus
  , sampleStatePath :: StoragePath
  , sampleConfigurationDigest :: ConfigurationDigest
  , sampleSnapshotStartedAt :: EpochSeconds
  , sampleSnapshotCompletedAt :: EpochSeconds
  , sampleFilesystemOnly :: Bool
  , sampleDirectoryCount :: Int
  , sampleFilesystemCount :: Int
  , sampleBtrfsCount :: Int
  , sampleIssueCount :: Int
  , sampleIssueKinds :: [IssueCount]
  , sampleLowSpace :: Bool
  , sampleFilesystems :: [FilesystemMeasurement]
  , sampleGrowthCompatible :: Bool
  , sampleFilesystemGrowth :: [FilesystemGrowth]
  , sampleDirectoryGrowthCount :: Int
  , sampleBudgets :: [BudgetSummary]
  } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)

data CheckReport = CheckReport
  { checkStatus :: CheckStatus
  , checkConfigurationDigest :: ConfigurationDigest
  , checkSnapshotCompletedAt :: EpochSeconds
  , checkSnapshotAgeSeconds :: Seconds
  , checkSnapshotFilesystemOnly :: Bool
  , checkCoverageIssueCount :: Int
  , checkFirstCoverageIssue :: Maybe CoverageIssue
  , checkLiveFilesystems :: [FilesystemMeasurement]
  , checkReserveBytes :: ByteCount
  , checkLiveLowSpace :: Bool
  , checkGrowthCompatible :: Bool
  , checkFilesystemGrowth :: [FilesystemGrowth]
  , checkBudgets :: [BudgetSummary]
  , checkProblems :: [Text]
  } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)

data Evaluation = Evaluation { evaluationProblems :: [String], evaluationReport :: CheckReport }

main :: IO ()
main = do
  thread <- myThreadId
  _ <- installHandler sigTERM (CatchOnce (throwTo thread (ExitFailure 143))) Nothing
  (getArgs >>= dispatch) `catch` \(err :: IOException) -> die (displayException err)

dispatch :: [String] -> IO ()
dispatch ("sample" : arguments) = do
  (configFile, stateArgument, remaining, filesystemOnly) <- parseOptions arguments
  ensure (null remaining) ("unexpected sample argument: " ++ unwords remaining)
  sampleCommand configFile stateArgument filesystemOnly
dispatch ("check" : arguments) = do
  (configFile, stateArgument, remaining, filesystemOnly) <- parseOptions arguments
  ensure (not filesystemOnly) "--filesystem-only is a sample option"
  ensure (null remaining) ("unexpected check argument: " ++ unwords remaining)
  checkCommand configFile stateArgument
dispatch ("guard" : arguments) = do
  (configFile, stateArgument, remaining, filesystemOnly) <- parseOptions arguments
  ensure (not filesystemOnly) "--filesystem-only is a sample option"
  command <- case remaining of
    ("--" : commandArguments) | not (null commandArguments) -> pure commandArguments
    _ -> die "guard requires -- COMMAND ARGS..."
  guardCommand configFile stateArgument command
dispatch _ = die usageText

parseOptions :: [String] -> IO (FilePath, FilePath, [String], Bool)
parseOptions = go Nothing Nothing False
 where
  go _ state filesystemOnly ("--config" : value : rest) = go (Just value) state filesystemOnly rest
  go config _ filesystemOnly ("--state" : value : rest) = go config (Just value) filesystemOnly rest
  go config state _ ("--filesystem-only" : rest) = go config state True rest
  go config state filesystemOnly rest = do
    configFile <- maybe (die "missing --config FILE") pure config
    stateArgument <- maybe (die "missing --state DIR") pure state
    pure (configFile, stateArgument, rest, filesystemOnly)

sampleCommand :: FilePath -> FilePath -> Bool -> IO ()
sampleCommand configFile stateArgument filesystemOnly = do
  (config, digest) <- loadConfig configFile
  Runtime.withStateLock stateArgument $ \directory -> do
    previousState <- readStateMaybe directory
    started <- nowEpoch
    filesystemProbe <- probeFilesystems config
    ensure (not (null (probeMeasurements filesystemProbe)))
      ("no filesystem measurements succeeded; previous state preserved\n" ++ unlines (reportedProblems (map renderIssue (probeIssues filesystemProbe))))
    (capacities, capacityIssues) <- probeBtrfs config (probeMeasurements filesystemProbe)
    (directories, directoryIssues) <- if filesystemOnly then pure ([], []) else scanDirectories config
    allocations <- if filesystemOnly then pure [] else probeProducerAllocations config directories
    completed <- nowEpoch
    let issues = sortOn issueOrder (probeIssues filesystemProbe ++ capacityIssues ++ directoryIssues ++ concatMap allocationCoverageIssues allocations)
        snapshot = AccountingSnapshot
          { snapshotSchemaVersion = schemaVersion
          , snapshotConfigurationDigest = digest
          , snapshotStartedAt = started
          , snapshotCompletedAt = completed
          , snapshotDirectories = sortOn directoryPath directories
          , snapshotProducerAllocations = allocations
          , snapshotFilesystems = probeMeasurements filesystemProbe
          , snapshotBtrfsCapacity = capacities
          , snapshotCoverageIssues = issues
          , snapshotFilesystemOnly = filesystemOnly
          }
        growth = computeGrowth config (stateCurrent <$> previousState) snapshot
        budgets = evaluateBudgets config snapshot
        lowSpace = any (\measurement -> filesystemAvailableBytes measurement < accountingReserveBytes config) (probeMeasurements filesystemProbe)
        state = AccountingState
          { stateSchemaVersion = schemaVersion
          , stateCurrent = snapshot
          , statePrevious = stateCurrent <$> previousState
          , stateGrowth = growth
          , stateBudgets = budgets
          , stateLowSpace = lowSpace
          }
    writeStateFile directory state
    BL.putStr (encode (SampleReport
      { sampleStatus = if null issues then SampleComplete else SampleIncomplete
      , sampleStatePath = StoragePath (Runtime.stateDisplayPath directory </> "accounting.json")
      , sampleConfigurationDigest = digest
      , sampleSnapshotStartedAt = started
      , sampleSnapshotCompletedAt = completed
      , sampleFilesystemOnly = filesystemOnly
      , sampleDirectoryCount = length directories
      , sampleFilesystemCount = length (dedupeMeasurements (probeMeasurements filesystemProbe))
      , sampleBtrfsCount = length capacities
      , sampleIssueCount = length issues
      , sampleIssueKinds = issueCounts issues
      , sampleLowSpace = lowSpace
      , sampleFilesystems = probeMeasurements filesystemProbe
      , sampleGrowthCompatible = growthCompatibleBaseline growth
      , sampleFilesystemGrowth = growthFilesystems growth
      , sampleDirectoryGrowthCount = length (growthDirectories growth)
      , sampleBudgets = budgetSummaries budgets
      }))
    hFlush stdout
    when (not (null issues)) $
      hPutStrLn stderr ("storage accounting sample: incomplete coverage: " ++ show (length issues) ++ " issue(s)")

checkCommand :: FilePath -> FilePath -> IO ()
checkCommand configFile stateArgument = do
  (config, digest) <- loadConfig configFile
  state <- loadState stateArgument
  now <- nowEpoch
  evaluation <- evaluateCheck config digest state now
  BL.putStr (encode (evaluationReport evaluation))
  hFlush stdout
  unless (null (evaluationProblems evaluation)) $ do
    forM_ (reportedProblems (evaluationProblems evaluation)) $ \problem -> hPutStrLn stderr ("storage accounting check: " ++ problem)
    exitFailure

guardCommand :: FilePath -> FilePath -> [String] -> IO ()
guardCommand configFile stateArgument command = do
  (config, digest) <- loadConfig configFile
  state <- loadState stateArgument
  now <- nowEpoch
  evaluation <- evaluateCheck config digest state now
  if null (evaluationProblems evaluation)
    then do
      hPutStrLn stderr "storage accounting guard: healthy; executing command"
      hFlush stderr
      hFlush stdout
      case command of
        executable : arguments -> executeFile executable True arguments Nothing
        [] -> die "storage accounting guard: empty command"
    else do
      forM_ (reportedProblems (evaluationProblems evaluation)) $ \problem -> hPutStrLn stderr ("storage accounting guard: " ++ problem)
      hFlush stderr
      exitFailure

evaluateCheck :: AccountingConfig -> ConfigurationDigest -> AccountingState -> EpochSeconds -> IO Evaluation
evaluateCheck config digest state now = do
  live <- probeFilesystems config
  let current = stateCurrent state
      age = Seconds (max 0 (unEpoch now - unEpoch (snapshotCompletedAt current)))
      currentGrowth = computeGrowth config (statePrevious state) current
      accountingProblems = case checkAccounting config digest now state of
        Left rejected -> rejected
        Right () -> []
      coverageProblems = case snapshotCoverageIssues current of
        [] -> []
        firstIssue : remaining -> ["coverage incomplete: " ++ show (1 + length remaining) ++ " issue(s); first: " ++ renderIssue firstIssue]
      liveProblems =
        [ "live filesystem probe failed: " ++ renderIssue coverageIssue | coverageIssue <- probeIssues live ]
        ++ [ "no live filesystem measurement" | null (probeMeasurements live) ]
        ++ [ "filesystem identity changed at a configured probe path; run sample again"
           | Set.fromList [(filesystemProbePath row, filesystemId row) | row <- probeMeasurements live]
             /= Set.fromList [(filesystemProbePath row, filesystemId row) | row <- snapshotFilesystems current] ]
        ++ [ "live free space below reserve: " ++ T.unpack (filesystemIdText (filesystemId measurement))
               ++ " available " ++ show (unByteCount (filesystemAvailableBytes measurement))
               ++ " reserve " ++ show (unByteCount (accountingReserveBytes config))
           | measurement <- probeMeasurements live
           , filesystemAvailableBytes measurement < accountingReserveBytes config
           ]
      modeProblems = [ "detailed inventory snapshot required; current snapshot is filesystem-only" | snapshotFilesystemOnly current ]
      problems = accountingProblems ++ modeProblems ++ coverageProblems ++ liveProblems
      lowSpace = any (\measurement -> filesystemAvailableBytes measurement < accountingReserveBytes config) (probeMeasurements live)
      report = CheckReport
        { checkStatus = if null problems then CheckHealthy else CheckUnhealthy
        , checkConfigurationDigest = digest
        , checkSnapshotCompletedAt = snapshotCompletedAt current
        , checkSnapshotAgeSeconds = age
        , checkSnapshotFilesystemOnly = snapshotFilesystemOnly current
        , checkCoverageIssueCount = length (snapshotCoverageIssues current)
        , checkFirstCoverageIssue = listToMaybe (snapshotCoverageIssues current)
        , checkLiveFilesystems = probeMeasurements live
        , checkReserveBytes = accountingReserveBytes config
        , checkLiveLowSpace = lowSpace
        , checkGrowthCompatible = growthCompatibleBaseline currentGrowth
        , checkFilesystemGrowth = growthFilesystems currentGrowth
        , checkBudgets = budgetSummaries (evaluateBudgets config current)
        , checkProblems = map T.pack (reportedProblems problems)
        }
  pure (Evaluation problems report)

reportedProblems :: [String] -> [String]
reportedProblems = map (take 2000) . take 32

loadConfig :: FilePath -> IO (AccountingConfig, ConfigurationDigest)
loadConfig path = do
  bytes <- readBoundedFile maxConfigBytes path
  config <- either (ioError . userError . ("invalid accounting config: " ++)) pure (eitherDecodeStrict' bytes)
  validateConfig config
  pure (config, digestBytes bytes)

loadState :: FilePath -> IO AccountingState
loadState argument = Runtime.withStateRead argument readStateRequired

validateConfig :: AccountingConfig -> IO ()
validateConfig config = do
  ensure (accountingSchemaVersion config == schemaVersion) "unsupported accounting config schema"
  let roots = accountingScanRoots config
  ensure (length (map scanRootId roots) == Set.size (Set.fromList (map scanRootId roots))) "duplicate scan root id"
  ensure (all (not . T.null . rootIdText . scanRootId) roots) "empty scan root id"
  ensure (all (\root -> scanDepth root >= 0 && scanDepth root <= maxScanDepth) roots) "scan depth must be between 0 and 8"
  forM_ roots $ \root -> do
    _ <- requireCanonicalPath "scan root" (storagePathText (scanRootPath root))
    pure ()
  let rootPaths = map (storagePathText . scanRootPath) roots
  ensure (length rootPaths == Set.size (Set.fromList rootPaths)) "duplicate scan root path"
  forM_ [(first, second) | (firstIndex, first) <- zip [0 :: Int ..] roots, (secondIndex, second) <- zip [0 :: Int ..] roots, firstIndex < secondIndex] $ \(first, second) -> do
    let firstPath = storagePathText (scanRootPath first)
        secondPath = storagePathText (scanRootPath second)
    when (inside firstPath secondPath || inside secondPath firstPath) $ do
      same <- sameDeviceWhenKnown firstPath secondPath
      ensure (same /= Just True) "overlapping scan roots on the same filesystem would double-count"
  let filesystemPaths = map storagePathText (accountingFilesystemPaths config)
  forM_ filesystemPaths $ \path -> do
    _ <- requireCanonicalPath "filesystem probe path" path
    pure ()
  ensure (length filesystemPaths == Set.size (Set.fromList filesystemPaths)) "duplicate filesystem probe path"
  ensure (unByteCount (accountingReserveBytes config) >= 0) "reserve must be nonnegative"
  ensure (unSeconds (accountingCommandTimeout config) > 0) "command timeout must be positive"
  ensure (unSeconds (accountingMaximumAge config) > 0) "maximum snapshot age must be positive"
  case validateProducerBudgets config of
    Left message -> ioError (userError ("invalid producer budget: " ++ message))
    Right () -> pure ()

requireCanonicalPath :: String -> FilePath -> IO FilePath
requireCanonicalPath label input = do
  absoluteResult <- try (normalise <$> makeAbsolute input) :: IO (Either IOException FilePath)
  absolute <- either (ioError . userError . ((label ++ " cannot be made absolute: ") ++) . displayException) pure absoluteResult
  canonicalResult <- try (canonicalizePath absolute) :: IO (Either IOException FilePath)
  case canonicalResult of
    Left err
      | isDoesNotExistError err ->
          ensure (isAbsolute input && input == absolute) (label ++ " must be an absolute, canonical path: " ++ input)
      | otherwise -> ioError (userError (label ++ " cannot be canonicalized: " ++ displayException err))
    Right canonical ->
      ensure (isAbsolute input && input == absolute && absolute == canonical) (label ++ " must be an absolute, canonical, non-symlink path: " ++ input ++ " (canonical: " ++ canonical ++ ")")
  pure absolute

sameDeviceWhenKnown :: FilePath -> FilePath -> IO (Maybe Bool)
sameDeviceWhenKnown first second = do
  result <- try (do
    firstStatus <- getFileStatus first
    secondStatus <- getFileStatus second
    pure (isDirectory firstStatus && isDirectory secondStatus && deviceID firstStatus == deviceID secondStatus)) :: IO (Either IOException Bool)
  pure $ case result of
    Right same -> Just same
    Left _ -> Nothing

inside :: FilePath -> FilePath -> Bool
inside parent child = splitDirectories parent `isPrefixOfList` splitDirectories child
 where
  isPrefixOfList [] _ = True
  isPrefixOfList _ [] = False
  isPrefixOfList (x : xs) (y : ys) = x == y && isPrefixOfList xs ys

relativeDepth :: FilePath -> FilePath -> Maybe Int
relativeDepth parent child
  | parent == child = Just 0
  | inside parent child = Just (length (splitDirectories child) - length (splitDirectories parent))
  | otherwise = Nothing

probeFilesystems :: AccountingConfig -> IO ProbeResult
probeFilesystems config = do
  results <- forM (accountingFilesystemPaths config) $ \storagePath -> do
    let path = storagePathText storagePath
    canonicalResult <- try (canonicalizePath path) :: IO (Either IOException FilePath)
    case canonicalResult of
      Left err -> pure (Left (issue (classifyIOException err) path (T.pack (displayException err))))
      Right canonical
        | canonical /= path -> pure (Left (issue ScopeChanged path "filesystem probe path is no longer canonical"))
        | otherwise -> do
            measuredAt <- nowEpoch
            outcome <- runCommand (accountingCommandTimeout config) maxDfOutput "df" ["-B1", "--output=source,size,used,avail", "--", path]
            case outcome of
              CommandCompleted ExitSuccess output _ ->
                case parseDfOutput storagePath measuredAt output of
                  Left problem -> pure (Left (issue InvalidOutput path problem))
                  Right measurement -> identifyFilesystem config measurement
              CommandCompleted (ExitFailure _) _ err -> pure (Left (commandFailureIssue path err))
              CommandTimedOut _ err -> pure (Left (issue TimedOut path (appendDetail "df timed out" err)))
              CommandOutputExceeded _ err -> pure (Left (issue ToolFailure path (appendDetail "df output exceeded bound" err)))
              CommandSpawnFailed message -> pure (Left (issue ToolFailure path (T.pack message)))
  let issues = [coverageIssue | Left coverageIssue <- results]
      measurements = [measurement | Right measurement <- results]
  pure (ProbeResult (sortOn filesystemProbePath measurements) (sortOn issueOrder issues))

parseDfOutput :: StoragePath -> EpochSeconds -> B.ByteString -> Either Text FilesystemMeasurement
parseDfOutput storagePath measuredAt output =
  case filter (not . B.null) (BC.lines output) of
    _header : line : [] -> parseLine line
    _ -> Left "df output must contain a header and exactly one measurement"
 where
  parseLine line =
    case reverse (BC.words line) of
      availField : usedField : sizeField : sourceFields | not (null sourceFields) ->
        case (readIntegerStrict availField, readIntegerStrict usedField, readIntegerStrict sizeField) of
          (Just available, Just used, Just total)
            | available >= 0 && used >= 0 && total >= 0 ->
                let source = TE.decodeUtf8With lenientDecode (BC.unwords sourceFields)
                in if T.null source
                     then Left "empty filesystem source"
                     else Right (FilesystemMeasurement (FilesystemId source) storagePath measuredAt (ByteCount total) (ByteCount used) (ByteCount available))
          _ -> Left "df output contains non-numeric fields"
      _ -> Left "df output is missing fields"

identifyFilesystem :: AccountingConfig -> FilesystemMeasurement -> IO (Either CoverageIssue FilesystemMeasurement)
identifyFilesystem config measurement = do
  let path = storagePathText (filesystemProbePath measurement)
  outcome <- runCommand (accountingCommandTimeout config) maxStatOutput "stat" ["-f", "-c", "%T:%i", "--", path]
  case outcome of
    CommandCompleted ExitSuccess output _ -> case BC.lines output of
      [value] | "btrfs:" `B.isPrefixOf` value -> identifyBtrfsFilesystem measurement
      [value] | not (B.null value), BC.elem ':' value ->
        pure (Right measurement { filesystemId = FilesystemId ("filesystem:" <> TE.decodeUtf8With lenientDecode value) })
      _ -> pure (Left (issue InvalidOutput path "stat did not return a filesystem identity"))
    CommandCompleted (ExitFailure _) _ err -> pure (Left (commandFailureIssue path err))
    CommandTimedOut _ err -> pure (Left (issue TimedOut path (appendDetail "filesystem identity probe timed out" err)))
    CommandOutputExceeded _ err -> pure (Left (issue ToolFailure path (appendDetail "filesystem identity output exceeded bound" err)))
    CommandSpawnFailed problem -> pure (Left (issue ToolFailure path (T.pack problem)))

identifyBtrfsFilesystem :: FilesystemMeasurement -> IO (Either CoverageIssue FilesystemMeasurement)
identifyBtrfsFilesystem measurement = do
  result <- try (Runtime.btrfsFilesystemId (filesystemProbePath measurement)) :: IO (Either IOException FilesystemId)
  pure $ case result of
    Right identity -> Right measurement { filesystemId = identity }
    Left err -> Left (issue (classifyIOException err) (storagePathText (filesystemProbePath measurement)) (T.pack (displayException err)))

dedupeMeasurements :: [FilesystemMeasurement] -> [FilesystemMeasurement]
dedupeMeasurements = go Set.empty
 where
  go _ [] = []
  go seen (measurement : rest)
    | Set.member (filesystemId measurement) seen = go seen rest
    | otherwise = measurement : go (Set.insert (filesystemId measurement) seen) rest

probeBtrfs :: AccountingConfig -> [FilesystemMeasurement] -> IO ([BtrfsCapacity], [CoverageIssue])
probeBtrfs config measurements = do
  results <- forM (dedupeMeasurements measurements) $ \measurement -> do
    let path = storagePathText (filesystemProbePath measurement)
    outcome <- runCommand (accountingCommandTimeout config) maxStatOutput "stat" ["-f", "-c", "%T", "--", path]
    case outcome of
      CommandCompleted ExitSuccess output _
        | T.strip (TE.decodeUtf8With lenientDecode output) == "btrfs" -> btrfsProbe config measurement
        | otherwise -> pure (Right [])
      CommandCompleted (ExitFailure _) _ err -> pure (Left [issue (classifyMessage err) path (appendDetail "filesystem type probe failed" err)])
      CommandTimedOut _ err -> pure (Left [issue TimedOut path (appendDetail "filesystem type probe timed out" err)])
      CommandOutputExceeded _ err -> pure (Left [issue ToolFailure path (appendDetail "filesystem type output exceeded bound" err)])
      CommandSpawnFailed message -> pure (Left [issue ToolFailure path (T.pack message)])
  let capacities = concat [capacity | Right capacity <- results]
      issues = concat [coverageIssue | Left coverageIssue <- results]
  pure (sortOn btrfsProbePath capacities, sortOn issueOrder issues)

btrfsProbe :: AccountingConfig -> FilesystemMeasurement -> IO (Either [CoverageIssue] [BtrfsCapacity])
btrfsProbe config measurement = do
  let path = storagePathText (filesystemProbePath measurement)
  outcome <- runCommand (accountingCommandTimeout config) maxBtrfsOutput "btrfs" ["filesystem", "usage", "-b", "--", path]
  pure $ case outcome of
    CommandCompleted ExitSuccess output _ ->
      case parseBtrfs (filesystemProbePath measurement) output of
        Left detail -> Left [issue InvalidOutput path detail]
        Right capacity -> Right [capacity]
    CommandCompleted (ExitFailure _) _ err
      | "not a btrfs filesystem" `B.isInfixOf` err -> Right []
      | otherwise -> Left [issue (classifyMessage err) path (appendDetail "btrfs capacity probe failed" err)]
    CommandTimedOut _ err -> Left [issue TimedOut path (appendDetail "btrfs capacity probe timed out" err)]
    CommandOutputExceeded _ err -> Left [issue ToolFailure path (appendDetail "btrfs capacity output exceeded bound" err)]
    CommandSpawnFailed message -> Left [issue ToolFailure path (T.pack message)]

parseBtrfs :: StoragePath -> B.ByteString -> Either Text BtrfsCapacity
parseBtrfs probePath output =
  let dataLines = mapMaybe (parseProfileLine "Data,") (BC.lines output)
      metadataLines = mapMaybe (parseProfileLine "Metadata,") (BC.lines output)
  in if null dataLines || null metadataLines
       then Left "btrfs output lacks Data or Metadata profile lines"
       else Right (BtrfsCapacity
         { btrfsProbePath = probePath
         , btrfsDataAllocatedBytes = ByteCount (sum [size | (_, size, _) <- dataLines])
         , btrfsDataUsedBytes = ByteCount (sum [used | (_, _, used) <- dataLines])
         , btrfsMetadataAllocatedPhysicalBytes = ByteCount (sum [size * profileCopies profile | (profile, size, _) <- metadataLines])
         , btrfsMetadataUsedPhysicalBytes = ByteCount (sum [used * profileCopies profile | (profile, _, used) <- metadataLines])
         })

parseProfileLine :: B.ByteString -> B.ByteString -> Maybe (B.ByteString, Integer, Integer)
parseProfileLine prefix line = do
  afterPrefix <- stripPrefixBytes prefix line
  let (profile, afterProfile) = BC.break (== ':') afterPrefix
  fieldsText <- stripPrefixBytes ":" afterProfile
  let fields = BC.split ',' fieldsText
  size <- fieldValue "Size:" fields
  used <- fieldValue "Used:" fields
  pure (profile, size, used)

stripPrefixBytes :: B.ByteString -> B.ByteString -> Maybe B.ByteString
stripPrefixBytes prefix value
  | prefix `B.isPrefixOf` value = Just (B.drop (B.length prefix) value)
  | otherwise = Nothing

fieldValue :: B.ByteString -> [B.ByteString] -> Maybe Integer
fieldValue name fields = do
  field <- listToMaybe [BC.dropWhile (== ' ') candidate | candidate <- fields, name `B.isPrefixOf` BC.dropWhile (== ' ') candidate]
  readIntegerStrict (BC.takeWhile isAsciiDigit (BC.drop (B.length name) field))

profileCopies :: B.ByteString -> Integer
profileCopies profile
  | "RAID1C4" `B.isInfixOf` upper = 4
  | "RAID1C3" `B.isInfixOf` upper = 3
  | "RAID10" `B.isInfixOf` upper = 2
  | "RAID1" `B.isInfixOf` upper = 2
  | "DUP" `B.isInfixOf` upper = 2
  | otherwise = 1
 where
  upper = BC.map toUpper profile

scanDirectories :: AccountingConfig -> IO ([DirectoryMeasurement], [CoverageIssue])
scanDirectories config = do
  prepared <- forM (accountingScanRoots config) $ \root -> do
    let path = storagePathText (scanRootPath root)
    canonicalResult <- try (canonicalizePath path) :: IO (Either IOException FilePath)
    case canonicalResult of
      Left err -> pure (Left (issue (classifyIOException err) path (T.pack (displayException err))))
      Right canonical
        | canonical /= path -> pure (Left (issue ScopeChanged path "scan root is no longer canonical"))
        | otherwise -> do
            statusResult <- try (getFileStatus path) :: IO (Either IOException FileStatus)
            case statusResult of
              Left err -> pure (Left (issue (classifyIOException err) path (T.pack (displayException err))))
              Right status
                | not (isDirectory status) -> pure (Left (issue MissingPath path "scan root is not a directory"))
                | otherwise -> pure (Right (root, deviceID status))
  let preparedRoots = [(root, device) | Right (root, device) <- prepared]
      overlapIds = Set.fromList
        [ scanRootId descendant
        | (descendant, descendantDevice) <- preparedRoots
        , (ancestor, ancestorDevice) <- preparedRoots
        , scanRootId descendant /= scanRootId ancestor
        , descendantDevice == ancestorDevice
        , inside (storagePathText (scanRootPath ancestor)) (storagePathText (scanRootPath descendant))
        ]
      kept = [(root, device) | (root, device) <- preparedRoots, not (Set.member (scanRootId root) overlapIds)]
      overlapIssues =
        [ issue ScopeChanged (storagePathText (scanRootPath root)) "scan root overlaps another root on the same filesystem; excluded to avoid double-counting"
        | (root, _) <- preparedRoots
        , Set.member (scanRootId root) overlapIds
        ]
      groups = Map.toList (Map.fromListWith (++) [(device, [root]) | (root, device) <- kept])
      setupIssues = [coverageIssue | Left coverageIssue <- prepared] ++ overlapIssues
  outcomes <- forM groups $ \(_, roots) -> do
    let ordered = sortOn (storagePathText . scanRootPath) roots
        maxDepth = maximum (map scanDepth ordered)
        arguments = ["-x", "-B1", "--null", "--max-depth=" ++ show maxDepth, "--"] ++ map (storagePathText . scanRootPath) ordered
    outcome <- runCommand (accountingCommandTimeout config) maxDuOutput "du" arguments
    (parsed, malformed) <- case outcome of
      CommandCompleted _ output _ -> parseDuRecords output
      _ -> pure ([], 0)
    let (boundedRecords, truncated) = if length parsed > maxDirectoryRecords then (take maxDirectoryRecords parsed, True) else (parsed, False)
        rootPaths = map (storagePathText . scanRootPath) ordered
        measurements = recordsForGroup config ordered boundedRecords
        recordIssues =
          [issue InvalidOutput path "du produced a malformed record" | path <- rootPaths, malformed > 0]
          ++ [issue ToolFailure path "directory record bound exceeded; measurements are a lower bound" | path <- rootPaths, truncated]
          ++ [issue InvalidOutput path "du produced no record for the scan root" | path <- rootPaths, not (any ((== path) . snd) boundedRecords)]
    outcomeIssues <- duOutcomeIssues ordered outcome
    pure (measurements, recordIssues ++ outcomeIssues)
  let directories = concatMap fst outcomes
      issues = setupIssues ++ concatMap snd outcomes
  pure (sortOn directoryPath (dedupeDirectories directories), sortOn issueOrder issues)

recordsForGroup :: AccountingConfig -> [ScanRoot] -> [(ByteCount, FilePath)] -> [DirectoryMeasurement]
recordsForGroup config roots = mapMaybe record
 where
  origins = sortOn (negate . length . splitDirectories . storagePathText . scanRootPath) (accountingScanRoots config)
  accepted = Set.fromList (map scanRootId roots)
  record (size, path) = do
    origin <- listToMaybe [root | root <- origins, inside (storagePathText (scanRootPath root)) path]
    depth <- relativeDepth (storagePathText (scanRootPath origin)) path
    if Set.member (scanRootId origin) accepted && depth <= scanDepth origin
      then Just (DirectoryMeasurement (StoragePath path) size (scanRootId origin))
      else Nothing

dedupeDirectories :: [DirectoryMeasurement] -> [DirectoryMeasurement]
dedupeDirectories = Map.elems . Map.fromList . map (\measurement -> (directoryPath measurement, measurement))

probeProducerAllocations :: AccountingConfig -> [DirectoryMeasurement] -> IO [ProducerAllocation]
probeProducerAllocations config directories = forM [budget | budget <- accountingBudgets config, producerByteBudget budget /= Nothing] $ \budget -> do
  let copyPaths =
        [ directoryPath row
        | scope <- producerCopyScopes budget
        , row <- directories
        , let path = storagePathText (directoryPath row)
        , path /= storagePathText (copyParent scope)
        , takeDirectory path == storagePathText (copyParent scope)
        , let CopyPrefix prefix = copyNamePrefix scope
        , prefix `T.isPrefixOf` T.pack (last (splitDirectories path))
        ]
      owned = nub (producerPaths budget ++ copyPaths)
      nested = [scanRootPath root | root <- accountingScanRoots config,
        any (\parent -> inside (storagePathText parent) (storagePathText (scanRootPath root))) owned]
      candidates = nub (owned ++ nested)
  prepared <- mapM prepareScopePath candidates
  let paths = normalizeScopeRoots [entry | Right entry <- prepared]
      setupIssues = [problem | Left problem <- prepared]
      scopeRoots = [ScanRoot (RootId "producer-allocation") path 0 | path <- paths]
  if null paths
    then pure (ProducerAllocation (producerId budget) (ByteCount 0) candidates setupIssues)
    else do
      outcome <- runCommand (accountingCommandTimeout config) maxDuOutput "du"
        (["-x", "-B1", "--null", "--summarize", "--"] ++ map storagePathText paths)
      (records, malformed) <- case outcome of
        CommandCompleted _ output _ -> parseDuRecords output
        _ -> pure ([], 0)
      commandIssues <- duOutcomeIssues scopeRoots outcome
      let expected = Set.fromList (map storagePathText paths)
          observed = Map.fromList [(path, recordBytes) | (recordBytes, path) <- records, Set.member path expected]
          missing = [issue InvalidOutput path "producer allocation has no directory measurement"
            | path <- Set.toList expected, Map.notMember path observed]
          invalid = [issue InvalidOutput (storagePathText path) "producer allocation contains malformed or unexpected records"
            | path <- paths, malformed > 0 || length records /= Map.size observed]
          bytes = ByteCount (sum [unByteCount value | value <- Map.elems observed])
      pure (ProducerAllocation (producerId budget) bytes candidates (setupIssues ++ commandIssues ++ missing ++ invalid))

prepareScopePath :: StoragePath -> IO (Either CoverageIssue (StoragePath, DeviceID))
prepareScopePath scope = do
  let path = storagePathText scope
  result <- try (do
    canonical <- canonicalizePath path
    ensure (canonical == path) "producer scope is no longer canonical"
    status <- getSymbolicLinkStatus path
    ensure (isDirectory status && not (isSymbolicLink status)) "producer scope must be a directory"
    pure (scope, deviceID status)) :: IO (Either IOException (StoragePath, DeviceID))
  pure $ case result of
    Right value -> Right value
    Left problem -> Left (issue (classifyIOException problem) path (T.pack (displayException problem)))

normalizeScopeRoots :: [(StoragePath, DeviceID)] -> [StoragePath]
normalizeScopeRoots entries = map fst (foldl' add [] ordered)
 where
  ordered = sortOn (\(path, _) -> (length (splitDirectories (storagePathText path)), path)) entries
  add kept candidate@(path, device) =
    case sortOn (negate . length . splitDirectories . storagePathText . fst)
      [entry | entry@(ancestor, _) <- kept, inside (storagePathText ancestor) (storagePathText path)] of
      (_, ancestorDevice) : _ | ancestorDevice == device -> kept
      _ -> candidate : kept

parseDuRecords :: B.ByteString -> IO ([(ByteCount, FilePath)], Int)
parseDuRecords output = do
  encoding <- getFileSystemEncoding
  let terminated = not (B.null output) && B.last output == 0
      rawRecords = B.split 0 output
      records = filter (not . B.null) (if terminated then rawRecords else dropLast rawRecords)
  parsed <- forM records $ \record -> do
    let (digits, rest) = BC.span isAsciiDigit record
    case (readIntegerStrict digits, BC.uncons rest) of
      (Just value, Just (separator, pathBytes)) | separator == '\t' -> do
        decoded <- try (decodeFilesystemPathWith encoding pathBytes) :: IO (Either IOException FilePath)
        pure $ case decoded of
          Right path -> Just (ByteCount value, path)
          Left _ -> Nothing
      _ -> pure Nothing
  pure (catMaybes parsed, length [() | Nothing <- parsed])
 where
  dropLast [] = []
  dropLast values = init values

decodeFilesystemPath :: B.ByteString -> IO FilePath
decodeFilesystemPath bytes = getFileSystemEncoding >>= \encoding -> decodeFilesystemPathWith encoding bytes

decodeFilesystemPathWith :: TextEncoding -> B.ByteString -> IO FilePath
decodeFilesystemPathWith encoding bytes = B.useAsCStringLen bytes (GHC.peekCStringLen encoding)

duOutcomeIssues :: [ScanRoot] -> CommandOutcome -> IO [CoverageIssue]
duOutcomeIssues roots outcome = case outcome of
  CommandCompleted ExitSuccess _ _ -> pure []
  CommandCompleted (ExitFailure _) _ err -> stderrIssues roots err
  CommandTimedOut _ err -> pure (groupIssues TimedOut "du timed out" err)
  CommandOutputExceeded _ err -> pure (groupIssues ToolFailure "du output exceeded bound" err)
  CommandSpawnFailed message -> pure (groupIssues ToolFailure ("cannot execute du: " ++ message) B.empty)
 where
  groupIssues kind prefix err = [issue kind (storagePathText (scanRootPath root)) (appendDetail (T.pack prefix) err) | root <- roots]

stderrIssues :: [ScanRoot] -> B.ByteString -> IO [CoverageIssue]
stderrIssues roots err = do
  let rawLines = filter (not . B.null) (BC.lines err)
  mapped <- forM rawLines $ \line ->
    case extractQuoted line of
      Nothing -> pure (Nothing, line)
      Just pathBytes -> do
        decoded <- try (decodeFilesystemPath pathBytes) :: IO (Either IOException FilePath)
        pure $ case decoded of
          Right path | isAbsolute path, any (\root -> inside (storagePathText (scanRootPath root)) path) roots ->
            (Just (issue (classifyMessage line) path (issueText line)), line)
          _ -> (Nothing, line)
  let specific = take maxIssuesPerProbe [coverageIssue | (Just coverageIssue, _) <- mapped]
      genericLines = take 1 [line | (Nothing, line) <- mapped]
      generic = [issue (classifyMessage line) (storagePathText (scanRootPath root)) (issueText line) | root <- roots, line <- genericLines]
      fallback = [issue ToolFailure (storagePathText (scanRootPath root)) "du failed without diagnostics" | root <- roots, null rawLines]
      truncated = [issue ToolFailure (storagePathText (scanRootPath root)) "du diagnostics exceeded the captured bound"
        | root <- roots, B.length err >= 16 * 1024 || length mapped > maxIssuesPerProbe]
  pure (specific ++ generic ++ fallback ++ truncated)

extractQuoted :: B.ByteString -> Maybe B.ByteString
extractQuoted line = do
  let afterFirst = BC.drop 1 (BC.dropWhile (/= '\'') line)
  if B.null afterFirst
    then Nothing
    else
      let (path, _) = BC.break (== '\'') afterFirst
      in if B.null path then Nothing else Just path

classifyMessage :: B.ByteString -> CoverageKind
classifyMessage message
  | "Permission denied" `B.isInfixOf` message = PermissionDenied
  | "No such file or directory" `B.isInfixOf` message = MissingPath
  | "Not a directory" `B.isInfixOf` message = MissingPath
  | otherwise = ToolFailure

classifyIOException :: IOException -> CoverageKind
classifyIOException err
  | isDoesNotExistError err = MissingPath
  | isPermissionError err = PermissionDenied
  | otherwise = ToolFailure

commandFailureIssue :: FilePath -> B.ByteString -> CoverageIssue
commandFailureIssue path err = issue (classifyMessage err) path (if B.null err then "command failed" else issueText err)

issue :: CoverageKind -> FilePath -> Text -> CoverageIssue
issue kind path detail = CoverageIssue (StoragePath path) kind (boundedText detail)

boundedText :: Text -> Text
boundedText = T.take issueDetailLimit . T.unwords . T.words

issueText :: B.ByteString -> Text
issueText = boundedText . TE.decodeUtf8With lenientDecode

appendDetail :: Text -> B.ByteString -> Text
appendDetail prefix err
  | B.null err = prefix
  | otherwise = boundedText (prefix <> ": " <> issueText err)

issueOrder :: CoverageIssue -> (StoragePath, Text)
issueOrder coverageIssue = (issuePath coverageIssue, issueDetail coverageIssue)

renderIssue :: CoverageIssue -> String
renderIssue coverageIssue =
  storagePathText (issuePath coverageIssue) ++ ": " ++ show (issueKind coverageIssue) ++ ": " ++ T.unpack (issueDetail coverageIssue)

issueCounts :: [CoverageIssue] -> [IssueCount]
issueCounts issues =
  [ IssueCount kind (length [() | coverageIssue <- issues, issueKind coverageIssue == kind])
  | kind <- [PermissionDenied, MissingPath, ToolFailure, TimedOut, InvalidOutput, ScopeChanged]
  ]

budgetSummaries :: [BudgetResult] -> [BudgetSummary]
budgetSummaries = map (\result -> BudgetSummary
  (budgetProducer result)
  (budgetObservedBytes result)
  (budgetObservedCopies result)
  (budgetVerdict result)
  (length (budgetProblems result)))

computeGrowth :: AccountingConfig -> Maybe AccountingSnapshot -> AccountingSnapshot -> GrowthReport
computeGrowth config previous current = case previous of
  Nothing -> GrowthReport False [] []
  Just prior
    | snapshotConfigurationDigest prior /= snapshotConfigurationDigest current -> GrowthReport False [] []
    | snapshotFilesystemOnly prior /= snapshotFilesystemOnly current -> GrowthReport False [] []
    | otherwise -> GrowthReport True (filesystemGrowth config current prior) (directoryGrowth config current prior)

filesystemGrowth :: AccountingConfig -> AccountingSnapshot -> AccountingSnapshot -> [FilesystemGrowth]
filesystemGrowth config current previous =
  [ makeGrowth measurement prior
  | measurement <- dedupeMeasurements (snapshotFilesystems current)
  , Just prior <- [Map.lookup (filesystemId measurement) priorById]
  , filesystemMeasuredAt measurement > filesystemMeasuredAt prior
  ]
 where
  priorById = Map.fromList [(filesystemId measurement, measurement) | measurement <- dedupeMeasurements (snapshotFilesystems previous)]
  makeGrowth measurement prior =
    let elapsed = Seconds (unEpoch (filesystemMeasuredAt measurement) - unEpoch (filesystemMeasuredAt prior))
        delta = unByteCount (filesystemUsedBytes measurement) - unByteCount (filesystemUsedBytes prior)
        rate = delta * 86400 `quot` unSeconds elapsed
        seconds = if rate > 0 then Just (secondsToReserve config measurement rate) else Nothing
    in FilesystemGrowth (filesystemId measurement) elapsed (ByteDelta delta) (ByteDelta rate) seconds

secondsToReserve :: AccountingConfig -> FilesystemMeasurement -> Integer -> Seconds
secondsToReserve config measurement rate =
  let gap = unByteCount (filesystemAvailableBytes measurement) - unByteCount (accountingReserveBytes config)
  in Seconds (if gap <= 0 then 0 else (gap * 86400) `quot` rate)

directoryGrowth :: AccountingConfig -> AccountingSnapshot -> AccountingSnapshot -> [DirectoryGrowth]
directoryGrowth config current previous
  | snapshotCompletedAt current <= snapshotCompletedAt previous = []
  | otherwise =
      [ DirectoryGrowth path (ByteDelta (unByteCount after - unByteCount before))
      | path <- Set.toAscList (Map.keysSet currentByPath `Set.union` Map.keysSet previousByPath)
      , Just (before, after) <- [values path]
      , before /= after
      ]
 where
  currentByPath = Map.fromList [(directoryPath row, row) | row <- snapshotDirectories current]
  previousByPath = Map.fromList [(directoryPath row, row) | row <- snapshotDirectories previous]
  currentPaths = Map.keysSet currentByPath
  previousPaths = Map.keysSet previousByPath
  values path = case (Map.lookup path previousByPath, Map.lookup path currentByPath) of
    (Just before, Just after) | directoryScanRoot before == directoryScanRoot after ->
      Just (directoryAllocatedBytes before, directoryAllocatedBytes after)
    (Nothing, Just after) | absenceKnown previous previousPaths after -> Just (ByteCount 0, directoryAllocatedBytes after)
    (Just before, Nothing) | absenceKnown current currentPaths before -> Just (directoryAllocatedBytes before, ByteCount 0)
    _ -> Nothing
  absenceKnown snapshot knownPaths row = any covers (accountingScanRoots config)
   where
    target = storagePathText (directoryPath row)
    covers root =
      scanRootId root == directoryScanRoot row
        && maybe False (<= scanDepth root) (relativeDepth (storagePathText (scanRootPath root)) target)
        && Set.member (scanRootPath root) knownPaths
        && all (\problem -> let failed = storagePathText (issuePath problem)
            in not (inside failed target || inside target failed)) (snapshotCoverageIssues snapshot)

nowEpoch :: IO EpochSeconds
nowEpoch = EpochSeconds . floor <$> getPOSIXTime

digestBytes :: B.ByteString -> ConfigurationDigest
digestBytes bytes = ConfigurationDigest (T.pack (concatMap hex (B.unpack (SHA.hash bytes))))
 where
  hex byte = let value = showHex byte "" in replicate (2 - length value) '0' ++ value

runCommand :: Seconds -> Int -> FilePath -> [String] -> IO CommandOutcome
runCommand limit outputLimit = Runtime.runBoundedCommand limit (Runtime.OutputLimit outputLimit)

readStateMaybe :: Runtime.StateDirectory -> IO (Maybe AccountingState)
readStateMaybe directory = do
  bytes <- Runtime.readStateBytes (Runtime.OutputLimit maxStateBytes) directory
  case bytes of
    Nothing -> pure Nothing
    Just payload -> do
      state <- either (ioError . userError . ("malformed accounting state: " ++)) pure (eitherDecodeStrict' payload)
      ensure (stateSchemaVersion state == schemaVersion) "unsupported accounting state schema"
      ensure (snapshotSchemaVersion (stateCurrent state) == schemaVersion) "unsupported accounting snapshot schema"
      pure (Just state)

readStateRequired :: Runtime.StateDirectory -> IO AccountingState
readStateRequired directory = readStateMaybe directory >>= maybe (ioError (userError "no accounting state; run sample first")) pure

readBoundedFile :: Int -> FilePath -> IO B.ByteString
readBoundedFile limit path = withBinaryFile path ReadMode $ \handle -> go handle 0 []
 where
  go handle total chunks = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then pure (B.concat (reverse chunks))
      else if total + B.length chunk > limit
        then ioError (userError ("file exceeds accounting bound: " ++ path))
        else go handle (total + B.length chunk) (chunk : chunks)

writeStateFile :: Runtime.StateDirectory -> AccountingState -> IO ()
writeStateFile directory = Runtime.writeStateBytes (Runtime.OutputLimit maxStateBytes) directory . encode

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (ioError (userError message))

storagePathText :: StoragePath -> FilePath
storagePathText (StoragePath path) = path

rootIdText :: RootId -> Text
rootIdText (RootId value) = value

filesystemIdText :: FilesystemId -> Text
filesystemIdText (FilesystemId value) = value

unByteCount :: ByteCount -> Integer
unByteCount (ByteCount value) = value

unEpoch :: EpochSeconds -> Integer
unEpoch (EpochSeconds value) = value

unSeconds :: Seconds -> Integer
unSeconds (Seconds value) = value

isAsciiDigit :: Char -> Bool
isAsciiDigit character = character >= '0' && character <= '9'

readIntegerStrict :: B.ByteString -> Maybe Integer
readIntegerStrict bytes
  | B.null bytes = Nothing
  | BC.all isAsciiDigit bytes = BC.readInteger bytes >>= \(value, rest) -> if B.null rest then Just value else Nothing
  | otherwise = Nothing
