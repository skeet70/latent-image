{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (forkIO, newEmptyMVar, putMVar, takeMVar, threadDelay)
import Control.Exception (IOException, bracket, catch, displayException, finally, try)
import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (ToJSON, eitherDecodeStrict', encode)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (toUpper)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as Map
import Data.Maybe (catMaybes, isJust, listToMaybe, mapMaybe)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Text.Encoding.Error (lenientDecode)
import Data.Time.Clock.POSIX (getPOSIXTime)
import Foreign.C.Error (eAGAIN, eWOULDBLOCK, getErrno)
import Foreign.C.Types (CInt (..))
import qualified GHC.Foreign as GHC
import GHC.Generics (Generic)
import GHC.IO.Encoding (TextEncoding, getFileSystemEncoding)
import Numeric (showHex)
import StorageAccountingPolicy (checkAccounting, evaluateBudgets, validateProducerBudgets)
import StorageAccountingTypes
import System.Directory (canonicalizePath, createDirectory, getHomeDirectory, makeAbsolute, removeFile, renameFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die, exitFailure)
import System.FilePath ((</>), isAbsolute, makeRelative, normalise, splitDirectories, takeDirectory)
import System.IO (Handle, IOMode (ReadMode), hClose, hFlush, hPutStrLn, stderr, stdout, withBinaryFile)
import System.IO.Error (isAlreadyExistsError, isDoesNotExistError, isPermissionError)
import System.Posix.Files (FileStatus, deviceID, fileMode, fileOwner, getFdStatus, getFileStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, setFileMode)
import System.Posix.IO (OpenFileFlags (..), OpenMode (..), closeFd, defaultFileFlags, fdToHandle, openFd)
import System.Posix.Process (executeFile)
import System.Posix.Types (Fd (..))
import System.Posix.User (getEffectiveUserID, getUserEntryForID, homeDirectory)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (..), createProcess, getProcessExitCode, proc, terminateProcess)
import System.Timeout (timeout)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

schemaVersion :: Int
schemaVersion = 1

maxScanDepth :: Int
maxScanDepth = 8

maxConfigBytes, maxStateBytes, maxDfOutput, maxStatOutput, maxBtrfsOutput, maxDuOutput, maxCommandStderr, maxIssuesPerProbe, maxDirectoryRecords, issueDetailLimit :: Int
maxConfigBytes = 4 * 1024 * 1024
maxStateBytes = 256 * 1024 * 1024
maxDfOutput = 1024 * 1024
maxStatOutput = 1024 * 1024
maxBtrfsOutput = 16 * 1024 * 1024
maxDuOutput = 256 * 1024 * 1024
maxCommandStderr = 16 * 1024
maxIssuesPerProbe = 1000
maxDirectoryRecords = 500000
issueDetailLimit = 500

usageText :: String
usageText = "usage: storage-accounting sample --config FILE --state DIR [--filesystem-only] | check --config FILE --state DIR | guard --config FILE --state DIR -- COMMAND ARGS..."

data CommandOutcome
  = CommandCompleted ExitCode B.ByteString B.ByteString
  | CommandTimedOut B.ByteString B.ByteString
  | CommandOutputExceeded B.ByteString B.ByteString
  | CommandSpawnFailed String

data ProbeResult = ProbeResult { probeMeasurements :: [FilesystemMeasurement], probeIssues :: [CoverageIssue] }

data IssueCount = IssueCount { issueCountKind :: CoverageKind, issueCountValue :: Int } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)
data BudgetSummary = BudgetSummary
  { budgetSummaryProducer :: ProducerId
  , budgetSummaryObservedBytes :: ByteCount
  , budgetSummaryObservedCopies :: CopyCount
  , budgetSummaryVerdict :: BudgetVerdict
  , budgetSummaryProblemCount :: Int
  } deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)

data SampleStatus = SampleComplete | SampleIncomplete deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)
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

data CheckStatus = CheckHealthy | CheckUnhealthy deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON)
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
main = (getArgs >>= dispatch) `catch` \(err :: IOException) -> die (displayException err)

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
  go config state filesystemOnly ("--config" : value : rest) = go (Just value) state filesystemOnly rest
  go config state filesystemOnly ("--state" : value : rest) = go config (Just value) filesystemOnly rest
  go config state _ ("--filesystem-only" : rest) = go config state True rest
  go config state filesystemOnly rest = do
    configFile <- maybe (die "missing --config FILE") pure config
    stateArgument <- maybe (die "missing --state DIR") pure state
    pure (configFile, stateArgument, rest, filesystemOnly)

sampleCommand :: FilePath -> FilePath -> Bool -> IO ()
sampleCommand configFile stateArgument filesystemOnly = do
  (config, digest) <- loadConfig configFile
  withStateLock stateArgument $ \directory -> do
    previousState <- readStateMaybe directory
    started <- nowEpoch
    filesystemProbe <- probeFilesystems config
    (capacities, capacityIssues) <- probeBtrfs config (probeMeasurements filesystemProbe)
    (directories, directoryIssues) <- if filesystemOnly then pure ([], []) else scanDirectories config
    completed <- nowEpoch
    let issues = sortOn issueOrder (probeIssues filesystemProbe ++ capacityIssues ++ directoryIssues)
        snapshot = AccountingSnapshot
          { snapshotSchemaVersion = schemaVersion
          , snapshotConfigurationDigest = digest
          , snapshotStartedAt = started
          , snapshotCompletedAt = completed
          , snapshotDirectories = sortOn directoryPath directories
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
      , sampleStatePath = StoragePath (stateFilePath directory)
      , sampleConfigurationDigest = digest
      , sampleSnapshotStartedAt = started
      , sampleSnapshotCompletedAt = completed
      , sampleFilesystemOnly = filesystemOnly
      , sampleDirectoryCount = length directories
      , sampleFilesystemCount = length (probeMeasurements filesystemProbe)
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
    forM_ (evaluationProblems evaluation) $ \problem -> hPutStrLn stderr ("storage accounting check: " ++ problem)
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
      forM_ (evaluationProblems evaluation) $ \problem -> hPutStrLn stderr ("storage accounting guard: " ++ problem)
      hFlush stderr
      exitFailure

evaluateCheck :: AccountingConfig -> ConfigurationDigest -> AccountingState -> EpochSeconds -> IO Evaluation
evaluateCheck config digest state now = do
  live <- probeFilesystems config
  let current = stateCurrent state
      age = Seconds (max 0 (unEpoch now - unEpoch (snapshotCompletedAt current)))
      accountingProblems = case checkAccounting config digest now state of
        Left problems -> problems
        Right () -> []
      coverageProblems =
        let issues = snapshotCoverageIssues current
        in if null issues
             then []
             else ["coverage incomplete: " ++ show (length issues) ++ " issue(s); first: " ++ renderIssue (head issues)]
      liveProblems =
        [ "live filesystem probe failed: " ++ renderIssue coverageIssue | coverageIssue <- probeIssues live ]
        ++ [ "no live filesystem measurement" | null (probeMeasurements live) ]
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
        , checkGrowthCompatible = growthCompatibleBaseline (stateGrowth state)
        , checkFilesystemGrowth = growthFilesystems (stateGrowth state)
        , checkBudgets = budgetSummaries (stateBudgets state)
        , checkProblems = map T.pack problems
        }
  pure (Evaluation problems report)

loadConfig :: FilePath -> IO (AccountingConfig, ConfigurationDigest)
loadConfig path = do
  bytes <- readBoundedFile maxConfigBytes path
  config <- either (ioError . userError . ("invalid accounting config: " ++)) pure (eitherDecodeStrict' bytes)
  validateConfig config
  pure (config, digestBytes bytes)

loadState :: FilePath -> IO AccountingState
loadState argument = stateDirectory False argument >>= readStateRequired

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
            pure $ case outcome of
              CommandCompleted ExitSuccess output _ ->
                either (Left . issue InvalidOutput path) Right (parseDfOutput storagePath measuredAt output)
              CommandCompleted (ExitFailure _) _ err -> Left (commandFailureIssue path err)
              CommandTimedOut _ err -> Left (issue TimedOut path (appendDetail "df timed out" err))
              CommandOutputExceeded _ err -> Left (issue ToolFailure path (appendDetail "df output exceeded bound" err))
              CommandSpawnFailed message -> Left (issue ToolFailure path (T.pack message))
  let issues = [coverageIssue | Left coverageIssue <- results]
      measurements = dedupeMeasurements [measurement | Right measurement <- results]
  pure (ProbeResult (sortOn filesystemId measurements) (sortOn issueOrder issues))

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

dedupeMeasurements :: [FilesystemMeasurement] -> [FilesystemMeasurement]
dedupeMeasurements = go Set.empty
 where
  go _ [] = []
  go seen (measurement : rest)
    | Set.member (filesystemId measurement) seen = go seen rest
    | otherwise = measurement : go (Set.insert (filesystemId measurement) seen) rest

probeBtrfs :: AccountingConfig -> [FilesystemMeasurement] -> IO ([BtrfsCapacity], [CoverageIssue])
probeBtrfs config measurements = do
  results <- forM measurements $ \measurement -> do
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
    -- One du invocation per device deduplicates hardlinks across every root on that device; hardlinks cannot cross devices.
    let ordered = sortOn (storagePathText . scanRootPath) roots
        maxDepth = maximum (map scanDepth ordered)
        arguments = ["-x", "-B1", "--null", "--max-depth=" ++ show maxDepth, "--"] ++ map (storagePathText . scanRootPath) ordered
    outcome <- runCommand (accountingCommandTimeout config) maxDuOutput "du" arguments
    (parsed, malformed) <- case outcome of
      CommandCompleted _ output _ -> parseDuRecords output
      _ -> pure ([], 0)
    let (boundedRecords, truncated) = if length parsed > maxDirectoryRecords then (take maxDirectoryRecords parsed, True) else (parsed, False)
        rootPaths = map (storagePathText . scanRootPath) ordered
        firstRoot = storagePathText (scanRootPath (head ordered))
        measurements = recordsForGroup ordered boundedRecords
        recordIssues =
          [issue InvalidOutput firstRoot "du produced a malformed record" | malformed > 0]
          ++ [issue ToolFailure firstRoot "directory record bound exceeded; measurements are a lower bound" | truncated]
          ++ [issue InvalidOutput path "du produced no record for the scan root" | path <- rootPaths, not (any ((== path) . snd) boundedRecords)]
    outcomeIssues <- duOutcomeIssues ordered outcome
    pure (measurements, recordIssues ++ outcomeIssues)
  let directories = concatMap fst outcomes
      issues = setupIssues ++ concatMap snd outcomes
  pure (sortOn directoryPath (dedupeDirectories directories), sortOn issueOrder issues)

recordsForGroup :: [ScanRoot] -> [(ByteCount, FilePath)] -> [DirectoryMeasurement]
recordsForGroup roots = mapMaybe record
 where
  record (size, path)
    | any (\root -> maybe False (\depth -> depth <= scanDepth root) (relativeDepth (storagePathText (scanRootPath root)) path)) roots =
        Just (DirectoryMeasurement (StoragePath path) size)
    | otherwise = Nothing

dedupeDirectories :: [DirectoryMeasurement] -> [DirectoryMeasurement]
dedupeDirectories = Map.elems . Map.fromList . map (\measurement -> (directoryPath measurement, measurement))

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
          Right path -> (Just (issue (classifyMessage line) path (issueText line)), line)
          Left _ -> (Nothing, line)
  let specific = take maxIssuesPerProbe [coverageIssue | (Just coverageIssue, _) <- mapped]
      genericLines = take 1 [line | (Nothing, line) <- mapped]
      generic = [issue (classifyMessage line) (storagePathText (scanRootPath root)) (issueText line) | root <- roots, line <- genericLines]
      fallback = [issue ToolFailure (storagePathText (scanRootPath root)) "du failed without diagnostics" | root <- roots, null rawLines]
  pure (specific ++ generic ++ fallback)

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
    | otherwise -> GrowthReport True (filesystemGrowth config current prior) (directoryGrowth current prior)

filesystemGrowth :: AccountingConfig -> AccountingSnapshot -> AccountingSnapshot -> [FilesystemGrowth]
filesystemGrowth config current previous =
  [ makeGrowth measurement prior
  | measurement <- snapshotFilesystems current
  , Just prior <- [Map.lookup (filesystemId measurement) priorById]
  , filesystemMeasuredAt measurement > filesystemMeasuredAt prior
  ]
 where
  priorById = Map.fromList [(filesystemId measurement, measurement) | measurement <- snapshotFilesystems previous]
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

directoryGrowth :: AccountingSnapshot -> AccountingSnapshot -> [DirectoryGrowth]
directoryGrowth current previous
  | snapshotCompletedAt current <= snapshotCompletedAt previous = []
  | otherwise = sortOn growthDirectory
      [ DirectoryGrowth (directoryPath measurement) (ByteDelta (unByteCount (directoryAllocatedBytes measurement) - unByteCount priorBytes))
      | measurement <- snapshotDirectories current
      , Just priorBytes <- [Map.lookup (directoryPath measurement) priorByPath]
      , directoryAllocatedBytes measurement /= priorBytes
      ]
 where
  priorByPath = Map.fromList [(directoryPath measurement, directoryAllocatedBytes measurement) | measurement <- snapshotDirectories previous]

nowEpoch :: IO EpochSeconds
nowEpoch = EpochSeconds . floor <$> getPOSIXTime

digestBytes :: B.ByteString -> ConfigurationDigest
digestBytes bytes = ConfigurationDigest (T.pack (concatMap hex (B.unpack (SHA.hash bytes))))
 where
  hex byte = let value = showHex byte "" in replicate (2 - length value) '0' ++ value

runCommand :: Seconds -> Int -> FilePath -> [String] -> IO CommandOutcome
runCommand timeoutValue outputLimit program arguments =
  (do
    result <- createProcess (proc program arguments) { std_in = NoStream, std_out = CreatePipe, std_err = CreatePipe }
    case result of
      (_, Just outHandle, Just errHandle, processHandle) -> do
        outRef <- newIORef []
        errRef <- newIORef []
        overflowRef <- newIORef False
        outDone <- newEmptyMVar
        errDone <- newEmptyMVar
        _ <- forkIO (readStream outHandle outputLimit outRef overflowRef processHandle True `finally` putMVar outDone ())
        _ <- forkIO (readStream errHandle maxCommandStderr errRef overflowRef processHandle False `finally` putMVar errDone ())
        let drain = do
              outFinished <- timeout 2000000 (takeMVar outDone)
              errFinished <- timeout 2000000 (takeMVar errDone)
              pure (isJust outFinished && isJust errFinished)
            collect = do
              out <- B.concat . reverse <$> readIORef outRef
              err <- B.concat . reverse <$> readIORef errRef
              overflow <- readIORef overflowRef
              pure (out, err, overflow)
            closeBoth = closeQuietly outHandle >> closeQuietly errHandle
        exited <- waitForProcessUntil processHandle (unSeconds timeoutValue * 1000000)
        case exited of
          Just exitCode -> do
            finished <- drain
            when finished closeBoth
            (out, err, overflow) <- collect
            pure (if overflow then CommandOutputExceeded out err else CommandCompleted exitCode out err)
          Nothing -> do
            terminateProcess processHandle `catch` \(_ :: IOException) -> pure ()
            _ <- waitForProcessUntil processHandle 2000000
            finished <- drain
            when finished closeBoth
            (out, err, _) <- collect
            pure (CommandTimedOut out err)
      (_, _, _, processHandle) -> do
        terminateProcess processHandle `catch` \(_ :: IOException) -> pure ()
        pure (CommandSpawnFailed "cannot create process pipes")
  ) `catch` \(err :: IOException) -> pure (CommandSpawnFailed (displayException err))

readStream :: Handle -> Int -> IORef [B.ByteString] -> IORef Bool -> ProcessHandle -> Bool -> IO ()
readStream handle limit chunksRef overflowRef processHandle stopOnOverflow = loop 0 `catch` \(_ :: IOException) -> pure ()
 where
  loop total = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then pure ()
      else do
        let remaining = limit - total
        if remaining <= 0 || B.length chunk > remaining
          then do
            when (remaining > 0) $ modifyIORef' chunksRef (B.take remaining chunk :)
            if stopOnOverflow
              then do
                writeIORef overflowRef True
                terminateProcess processHandle `catch` \(_ :: IOException) -> pure ()
                closeQuietly handle
              else loop (total + B.length chunk)
          else do
            modifyIORef' chunksRef (chunk :)
            loop (total + B.length chunk)

waitForProcessUntil :: ProcessHandle -> Integer -> IO (Maybe ExitCode)
waitForProcessUntil processHandle microseconds = do
  start <- getPOSIXTime
  let loop = do
        exitCode <- getProcessExitCode processHandle
        case exitCode of
          Just code -> pure (Just code)
          Nothing -> do
            current <- getPOSIXTime
            let elapsedMicroseconds = floor ((current - start) * 1000000) :: Integer
            if elapsedMicroseconds >= microseconds
              then pure Nothing
              else threadDelay 10000 >> loop
  loop

closeQuietly :: Handle -> IO ()
closeQuietly handle = hClose handle `catch` \(_ :: IOException) -> pure ()

stateDirectory :: Bool -> FilePath -> IO FilePath
stateDirectory create input = do
  absoluteResult <- try (normalise <$> makeAbsolute input) :: IO (Either IOException FilePath)
  absolute <- either (ioError . userError . ("state directory cannot be made absolute: " ++) . displayException) pure absoluteResult
  statusResult <- try (getSymbolicLinkStatus absolute) :: IO (Either IOException FileStatus)
  status <- case statusResult of
    Right existing -> pure existing
    Left err
      | isDoesNotExistError err && create -> do
          let parent = takeDirectory absolute
          _ <- requireTrustedStatePath parent
          created <- try (createDirectory absolute) :: IO (Either IOException ())
          case created of
            Left createError | not (isAlreadyExistsError createError) -> ioError createError
            _ -> pure ()
          getSymbolicLinkStatus absolute
      | isDoesNotExistError err -> ioError (userError ("state directory does not exist: " ++ input))
      | otherwise -> ioError err
  ensure (isDirectory status && not (isSymbolicLink status)) "state directory must be a directory, not a symlink or file"
  canonical <- requireTrustedStatePath absolute
  caller <- getEffectiveUserID
  ensure (fileOwner status == caller) "state directory must be owned by the invoking account"
  if create
    then setFileMode canonical 0o700
    else ensure ((fileMode status .&. 0o077) == 0) "state directory must be owner-only"
  pure canonical

requireTrustedStatePath :: FilePath -> IO FilePath
requireTrustedStatePath absolute = do
  canonicalResult <- try (canonicalizePath absolute) :: IO (Either IOException FilePath)
  canonical <- either (ioError . userError . ("state path cannot be canonicalized: " ++) . displayException) pure canonicalResult
  environmentHome <- try (getHomeDirectory) :: IO (Either IOException FilePath)
  passwdHome <- try (getEffectiveUserID >>= getUserEntryForID >>= \entry -> pure (homeDirectory entry)) :: IO (Either IOException FilePath)
  let homePaths = nub (either (const []) (: []) environmentHome ++ either (const []) (: []) passwdHome)
  homePairs <- fmap catMaybes $ forM homePaths $ \home -> do
    homeCanonicalResult <- try (canonicalizePath home) :: IO (Either IOException FilePath)
    pure $ case homeCanonicalResult of
      Right homeCanonical -> Just (home, homeCanonical)
      Left _ -> Nothing
  let expected = case
        [ normalise (homeCanonical </> makeRelative prefix absolute)
        | (home, homeCanonical) <- homePairs
        , prefix <- [home, homeCanonical]
        , inside prefix absolute
        ] of
        candidate : _ -> candidate
        [] -> absolute
  ensure (canonical == expected) "state path must not redirect through a symlink"
  pure canonical

withStateLock :: FilePath -> (FilePath -> IO a) -> IO a
withStateLock input action = do
  directory <- stateDirectory True input
  bracket (acquire directory) closeFd (\_ -> action directory)
 where
  acquire directory = do
    fd <- openFd (lockFilePath directory) ReadWrite defaultFileFlags { creat = Just 0o600, nofollow = True, cloexec = True }
    status <- getFdStatus fd
    caller <- getEffectiveUserID
    if not (isRegularFile status && fileOwner status == caller)
      then do
        closeFd fd
        ioError (userError "state lock must be a regular file owned by the invoking account")
      else case fd of
        Fd descriptor -> do
          result <- c_flock descriptor 6
          if result == 0
            then pure fd
            else do
              errno <- getErrno
              closeFd fd
              if errno == eAGAIN || errno == eWOULDBLOCK
                then ioError (userError "another storage accounting run holds the state lock")
                else ioError (userError "cannot acquire storage accounting state lock")

stateFilePath :: FilePath -> FilePath
stateFilePath directory = directory </> "accounting.json"

lockFilePath :: FilePath -> FilePath
lockFilePath directory = directory </> "accounting.lock"

tempFilePath :: FilePath -> FilePath
tempFilePath directory = directory </> "accounting.json.tmp"

readStateMaybe :: FilePath -> IO (Maybe AccountingState)
readStateMaybe directory = do
  let path = stateFilePath directory
  statusResult <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
  case statusResult of
    Left err
      | isDoesNotExistError err -> pure Nothing
      | otherwise -> ioError err
    Right status -> do
      caller <- getEffectiveUserID
      ensure (isRegularFile status && not (isSymbolicLink status)) "accounting state must be a regular file"
      ensure (fileOwner status == caller) "accounting state must be owned by the invoking account"
      ensure ((fileMode status .&. 0o077) == 0) "accounting state must not be accessible by other accounts"
      bytes <- readBoundedFile maxStateBytes path
      state <- either (ioError . userError . ("malformed accounting state: " ++)) pure (eitherDecodeStrict' bytes)
      ensure (stateSchemaVersion state == schemaVersion) "unsupported accounting state schema"
      ensure (snapshotSchemaVersion (stateCurrent state) == schemaVersion) "unsupported accounting snapshot schema"
      pure (Just state)

readStateRequired :: FilePath -> IO AccountingState
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

writeStateFile :: FilePath -> AccountingState -> IO ()
writeStateFile directory state = do
  let target = stateFilePath directory
      temporary = tempFilePath directory
      payload = encode state
  ensure (BL.length payload <= fromIntegral maxStateBytes) "accounting state exceeds the configured bound"
  removeIfExists temporary
  fd <- openFd temporary WriteOnly defaultFileFlags { creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True }
  handle <- fdToHandle fd
  (BL.hPut handle payload >> hFlush handle) `finally` hClose handle
  setFileMode temporary 0o600
  canonicalResult <- try (canonicalizePath directory) :: IO (Either IOException FilePath)
  canonical <- either (ioError . userError . ("state directory changed during write: " ++) . displayException) pure canonicalResult
  ensure (canonical == directory) "state directory changed during write"
  renameFile temporary target

removeIfExists :: FilePath -> IO ()
removeIfExists path = do
  result <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
  case result of
    Left err
      | isDoesNotExistError err -> pure ()
      | otherwise -> ioError err
    Right status -> do
      ensure (not (isDirectory status)) "state temporary path is a directory"
      removeFile path

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
