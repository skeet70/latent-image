{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Exception (IOException, bracket, bracketOnError, catch, finally, throwIO)
import Control.Monad (forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, isAscii, isDigit, isSpace)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (dropWhileEnd, isPrefixOf, stripPrefix)
import qualified Data.Map.Strict as Map
import Data.Maybe (fromMaybe, mapMaybe)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import Data.Text.Encoding (decodeUtf8')
import Data.Time.Calendar (Day)
import Data.Time.Clock (UTCTime (..))
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime, posixSecondsToUTCTime, utcTimeToPOSIXSeconds)
import Data.Time.Format (defaultTimeLocale, formatTime, parseTimeM)
import Data.Time.Format.ISO8601 (iso8601ParseM)
import Foreign.C.Types (CInt (..))
import System.Directory (canonicalizePath, doesPathExist, removeDirectory, removeFile, renameFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, normalise, splitDirectories, takeDirectory, (</>))
import System.IO (hClose, hPutStrLn, openTempFile, stderr)
import System.IO.Error (isDoesNotExistError, isPermissionError)
import System.Posix.Directory (closeDirStream, openDirStream, readDirStreamMaybe)
import System.Posix.Files (FileStatus, deviceID, fileMode, fileOwner, fileSize, getSymbolicLinkStatus, intersectFileModes, isDirectory, isRegularFile, isSymbolicLink, modificationTimeHiRes, nullFileMode, ownerWriteMode, setFileMode, statusChangeTimeHiRes, unionFileModes)
import System.Posix.IO (OpenMode (ReadOnly), closeFd, defaultFileFlags, openFd)
import System.Posix.Types (DeviceID, Fd (..))
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), StdStream (CreatePipe), createProcess, proc, readProcessWithExitCode, waitForProcess)
import Text.Read (readMaybe)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

lockExclusiveNonBlocking :: CInt
lockExclusiveNonBlocking = 2 + 4

newtype WorkspaceRoot = WorkspaceRoot FilePath
newtype RelativePath = RelativePath FilePath deriving (Eq, Ord)
newtype ByteCount = ByteCount Integer
newtype FileCount = FileCount Integer
newtype DirectoryCount = DirectoryCount Integer
newtype RunCount = RunCount Integer
newtype FailureCount = FailureCount Integer
newtype AgeDays = AgeDays Integer
newtype AgeHours = AgeHours Integer
newtype EpochSeconds = EpochSeconds Integer

data Pins = Pins (Set.Set RelativePath) (Set.Set RelativePath) (Set.Set RelativePath)

data Counts = Counts
  { countExpiredFiles :: !FileCount
  , countExpiredBytes :: !ByteCount
  , countExpiredWorkRuns :: !RunCount
  , countClosedAbandonedRuns :: !RunCount
  , countPinnedFiles :: !FileCount
  , countPinnedBytes :: !ByteCount
  , countRetainedFiles :: !FileCount
  , countRetainedBytes :: !ByteCount
  , countOwnedDirectories :: !DirectoryCount
  , countRemovedDirectories :: !DirectoryCount
  , countFailures :: !FailureCount
  }

emptyCounts :: Counts
emptyCounts = Counts (FileCount 0) (ByteCount 0) (RunCount 0) (RunCount 0) (FileCount 0) (ByteCount 0) (FileCount 0) (ByteCount 0) (DirectoryCount 0) (DirectoryCount 0) (FailureCount 0)

data Sweep = Sweep
  { sweepNow :: POSIXTime
  , sweepMaxAge :: POSIXTime
  , sweepWorkAge :: POSIXTime
  , sweepApply :: Bool
  , sweepPins :: Pins
  , sweepBranches :: Map.Map String Integer
  , sweepCounts :: IORef Counts
  }

data Options = Options
  { optionWorkspace :: Maybe FilePath
  , optionMaxAgeDays :: Maybe AgeDays
  , optionWorkAgeHours :: Maybe AgeHours
  , optionApply :: Bool
  , optionNow :: Maybe EpochSeconds
  }

data Bucket = ExpiredBucket | PinnedBucket | RetainedBucket

data Ownership = Owned | Unowned deriving (Eq)

data BranchInfo = BranchInfo
  { branchRefName :: String
  , branchShortName :: String
  , branchCommittedAt :: Integer
  }

data Hold = Hold Integer Day

data RunOutcome = OutcomeOpen | OutcomePassed | OutcomeFailed | OutcomeUnjudged

data RunRecord = RunRecord
  { recordOutcome :: RunOutcome
  , recordClosedAt :: Maybe POSIXTime
  }

data EvidenceRecord = EvidenceRecord
  { evidencePaths :: [Text]
  }

instance FromJSON RunRecord where
  parseJSON = withObject "run record" $ \recordObject -> do
    schema <- recordObject .: "schema_version" :: Parser Int
    unless (schema == 1) (fail "unsupported run schema")
    openerText <- recordObject .: "opener" :: Parser Text
    unless (openerText == "exec" || openerText == "slot") (fail "unknown run opener")
    outcomeText <- recordObject .: "outcome" :: Parser Text
    outcome <- case outcomeText of
      "open" -> pure OutcomeOpen
      "passed" -> pure OutcomePassed
      "failed" -> pure OutcomeFailed
      "unjudged" -> pure OutcomeUnjudged
      _ -> fail "unknown run outcome"
    closedText <- recordObject .:? "closed_at" :: Parser (Maybe Text)
    let closed = closedText >>= fmap utcTimeToPOSIXSeconds . iso8601ParseM . T.unpack
    pure (RunRecord outcome closed)

instance FromJSON EvidenceRecord where
  parseJSON = withObject "evidence record" $ \recordObject -> do
    status <- recordObject .:? "status" :: Parser (Maybe Text)
    label <- recordObject .:? "label" :: Parser (Maybe Text)
    artifacts <- recordObject .:? "artifacts" :: Parser (Maybe [Value])
    let qualifying = status == Just "open" || label == Just "hardware-observed"
    pure (EvidenceRecord (if qualifying then mapMaybe artifactPath (fromMaybe [] artifacts) else []))

artifactPath :: Value -> Maybe Text
artifactPath (Object artifact) = case KM.lookup "path" artifact of
  Just (String path) -> Just path
  _ -> Nothing
artifactPath _ = Nothing

citationPattern :: String
citationPattern = "private-data/[A-Za-z0-9._@+/=-]+"

citationPrefix :: String
citationPrefix = "private-data/"

keepListPath :: FilePath
keepListPath = "build-support/scratch-keep.txt"

evidencePath :: FilePath
evidencePath = "docs/evidence.jsonl"

originMainRef :: String
originMainRef = "refs/remotes/origin/main"

citationSizeLimit :: Integer
citationSizeLimit = 16 * 1024 * 1024

relativePathText :: RelativePath -> FilePath
relativePathText (RelativePath value) = value

byteCount :: ByteCount -> Integer
byteCount (ByteCount value) = value

fileCount :: FileCount -> Integer
fileCount (FileCount value) = value

directoryCount :: DirectoryCount -> Integer
directoryCount (DirectoryCount value) = value

runCount :: RunCount -> Integer
runCount (RunCount value) = value

failureCount :: FailureCount -> Integer
failureCount (FailureCount value) = value

ageDays :: AgeDays -> Integer
ageDays (AgeDays value) = value

ageHours :: AgeHours -> Integer
ageHours (AgeHours value) = value

epochSeconds :: EpochSeconds -> Integer
epochSeconds (EpochSeconds value) = value

ageDaysSeconds :: AgeDays -> Integer
ageDaysSeconds (AgeDays value) = value * 86400

ageHoursSeconds :: AgeHours -> Integer
ageHoursSeconds (AgeHours value) = value * 3600

main :: IO ()
main = (do
  arguments <- getArgs
  options <- parseOptions arguments
  case options of
    Options (Just workspaceInput) (Just days) hours applied nowOverride -> runSweep workspaceInput days hours applied nowOverride
    _ -> usage)
  `catch` \(problem :: IOException) -> failWith (firstLine (show problem))

usage :: IO a
usage = do
  hPutStrLn stderr "usage: scratch-expiry --workspace ROOT --max-age-days N [--work-age-hours H] [--apply] [--now EPOCH_SECONDS]"
  hPutStrLn stderr "  --work-age-hours defaults to 48; --now sets the reference time in epoch seconds and is for tests."
  exitWith (ExitFailure 2)

failWith :: String -> IO a
failWith message = do
  hPutStrLn stderr ("scratch-expiry: " ++ message)
  exitWith (ExitFailure 1)

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing False Nothing)
 where
  go options [] = case (optionWorkspace options, optionMaxAgeDays options) of
    (Just _, Just _) -> pure options
    _ -> usage
  go options ("--workspace" : value : rest) = case optionWorkspace options of
    Just _ -> usage
    Nothing -> go options {optionWorkspace = Just value} rest
  go options ("--max-age-days" : value : rest) = case optionMaxAgeDays options of
    Just _ -> usage
    Nothing -> case reads value :: [(Integer, String)] of
      [(days, "")] | days >= 0 -> go options {optionMaxAgeDays = Just (AgeDays days)} rest
      _ -> usage
  go options ("--work-age-hours" : value : rest) = case optionWorkAgeHours options of
    Just _ -> usage
    Nothing -> case reads value :: [(Integer, String)] of
      [(hours, "")] | hours >= 0 -> go options {optionWorkAgeHours = Just (AgeHours hours)} rest
      _ -> usage
  go options ("--now" : value : rest) = case optionNow options of
    Just _ -> usage
    Nothing -> case reads value :: [(Integer, String)] of
      [(seconds, "")] | seconds >= 0 -> go options {optionNow = Just (EpochSeconds seconds)} rest
      _ -> usage
  go options ("--apply" : rest) = go options {optionApply = True} rest
  go _ _ = usage

runSweep :: FilePath -> AgeDays -> Maybe AgeHours -> Bool -> Maybe EpochSeconds -> IO ()
runSweep workspaceInput days hoursOption applied nowOverride = do
  workspace@(WorkspaceRoot root) <- validateWorkspace workspaceInput
  stopped <- stopFilePresent root
  let hours = fromMaybe (AgeHours 48) hoursOption
  if stopped
    then printSummary applied True days hours emptyCounts
    else do
      reference <- case nowOverride of
        Just seconds -> pure (fromIntegral (epochSeconds seconds))
        Nothing -> getPOSIXTime
      pins <- collectPins workspace reference days
      branches <- collectBranchDates workspace
      counts <- newIORef emptyCounts
      let sweep = Sweep reference (fromIntegral (ageDaysSeconds days)) (fromIntegral (ageHoursSeconds hours)) applied pins branches counts
      sweepScratchRoot sweep workspace
      sweepVerifyRoot sweep workspace
      final <- readIORef counts
      printSummary applied False days hours final
      unless (failureCount (countFailures final) == 0) (exitWith (ExitFailure 1))

printSummary :: Bool -> Bool -> AgeDays -> AgeHours -> Counts -> IO ()
printSummary applied stopped days hours counts =
  BL.putStr
    ( encode
        ( object
            [ "applied" .= applied
            , "stopped" .= stopped
            , "max_age_days" .= ageDays days
            , "work_age_hours" .= ageHours hours
            , "expired_files" .= fileCount (countExpiredFiles counts)
            , "expired_bytes" .= byteCount (countExpiredBytes counts)
            , "expired_work_runs" .= runCount (countExpiredWorkRuns counts)
            , "closed_abandoned_runs" .= runCount (countClosedAbandonedRuns counts)
            , "pinned_files" .= fileCount (countPinnedFiles counts)
            , "pinned_bytes" .= byteCount (countPinnedBytes counts)
            , "retained_files" .= fileCount (countRetainedFiles counts)
            , "retained_bytes" .= byteCount (countRetainedBytes counts)
            , "owned_directories" .= directoryCount (countOwnedDirectories counts)
            , "removed_directories" .= directoryCount (countRemovedDirectories counts)
            , "failures" .= failureCount (countFailures counts)
            ]
        )
        <> "\n"
    )

validateWorkspace :: FilePath -> IO WorkspaceRoot
validateWorkspace input = do
  let normalized = normalise input
  unless (isAbsolute normalized) (failWith "workspace must be an absolute path")
  present <- doesPathExist normalized
  unless present (failWith "workspace does not exist")
  resolved <- canonicalizePath normalized
  status <- getSymbolicLinkStatus resolved
  unless (isDirectory status && not (isSymbolicLink status)) (failWith "workspace must be a directory")
  repository <- doesPathExist (resolved </> ".git")
  unless repository (failWith "workspace must contain .git")
  dataStatus <- getSymbolicLinkStatus (resolved </> "private-data")
    `catch` \(problem :: IOException) ->
      if isDoesNotExistError problem
        then failWith "workspace private-data must be a real directory"
        else throwIO problem
  unless (isDirectory dataStatus && not (isSymbolicLink dataStatus)) (failWith "workspace private-data must be a real directory, not a symlink")
  pure (WorkspaceRoot resolved)

stopFilePresent :: FilePath -> IO Bool
stopFilePresent root = do
  result <- (Right <$> getSymbolicLinkStatus (root </> "private-data/scratch/NO-EXPIRY")) `catch` \(problem :: IOException) -> pure (Left problem)
  case result of
    Right _ -> pure True
    Left problem
      | isDoesNotExistError problem -> pure False
      | otherwise -> throwIO problem

collectPins :: WorkspaceRoot -> POSIXTime -> AgeDays -> IO Pins
collectPins (WorkspaceRoot root) reference days = do
  requireHead root
  originPresent <- refExists root originMainRef
  grace <- graceCommit root reference days
  live <- liveBranches root reference days
  let historyRefs = uniqueRefs (["HEAD"] ++ [originMainRef | originPresent] ++ maybe [] pure grace)
      keepRefs = uniqueRefs (historyRefs ++ map branchRefName live)
  keepEntries <- fmap Set.unions (mapM (keepListAt root) keepRefs)
  historyCitations <- fmap Set.unions (mapM (citationsAt root) historyRefs)
  historyStrong <- fmap Set.unions (mapM (strongCitationsAt root) historyRefs)
  branchPins <- mapM (liveBranchPins root) live
  pure
    ( Pins
        (Set.union historyCitations (Set.unions (map fst branchPins)))
        (Set.union historyStrong (Set.unions (map snd branchPins)))
        keepEntries
    )

uniqueRefs :: [String] -> [String]
uniqueRefs = Set.toList . Set.fromList

forceSet :: Set.Set RelativePath -> Set.Set RelativePath
forceSet values = Set.foldl' force () values `seq` values
 where
  force () (RelativePath path) = length path `seq` ()

requireHead :: FilePath -> IO ()
requireHead root = do
  (status, _, errors) <- git root ["rev-parse", "--verify", "HEAD"]
  case status of
    ExitSuccess -> pure ()
    _ -> failWith ("git rev-parse HEAD: " ++ firstLine errors)

refExists :: FilePath -> String -> IO Bool
refExists root ref = do
  (status, _, errors) <- git root ["show-ref", "--verify", "--quiet", ref]
  case status of
    ExitSuccess -> pure True
    ExitFailure 1 -> pure False
    _ -> failWith ("git show-ref " ++ ref ++ ": " ++ firstLine errors)

git :: FilePath -> [String] -> IO (ExitCode, String, String)
git root arguments = readProcessWithExitCode "git" (["-C", root] ++ arguments) ""

gitBytes :: FilePath -> [String] -> IO (ExitCode, B.ByteString, String)
gitBytes root arguments = do
  (_, Just outputHandle, Just errorHandle, processHandle) <-
    createProcess (proc "git" (["-C", root] ++ arguments)) {std_out = CreatePipe, std_err = CreatePipe}
  output <- B.hGetContents outputHandle
  errors <- B.hGetContents errorHandle
  status <- waitForProcess processHandle
  pure (status, output, BC.unpack errors)

collectBranchDates :: WorkspaceRoot -> IO (Map.Map String Integer)
collectBranchDates (WorkspaceRoot root) = do
  (status, output, errors) <- git root ["for-each-ref", "--format=%(refname)%09%(committerdate:unix)", "refs/heads"]
  case status of
    ExitSuccess -> pure (Map.fromList [(branchShortName branch, branchCommittedAt branch) | branch <- mapMaybe parseBranch (lines output)])
    _ -> failWith ("git for-each-ref: " ++ firstLine errors)

liveBranches :: FilePath -> POSIXTime -> AgeDays -> IO [BranchInfo]
liveBranches root reference days = do
  (status, output, errors) <- git root ["for-each-ref", "--no-merged", "HEAD", "--format=%(refname)%09%(committerdate:unix)", "refs/heads"]
  case status of
    ExitSuccess ->
      pure
        [ branch
        | branch <- mapMaybe parseBranch (lines output)
        , reference - fromIntegral (branchCommittedAt branch) <= fromIntegral (ageDaysSeconds days)
        ]
    _ -> failWith ("git for-each-ref: " ++ firstLine errors)

parseBranch :: String -> Maybe BranchInfo
parseBranch line =
  let (referenceName, rest) = break (== '\t') line
      dateText = drop 1 rest
  in do
       name <- stripPrefix "refs/heads/" referenceName
       date <- readMaybe dateText :: Maybe Integer
       pure (BranchInfo referenceName name date)

graceCommit :: FilePath -> POSIXTime -> AgeDays -> IO (Maybe String)
graceCommit root reference days = do
  let before = floor reference - ageDaysSeconds days
  (status, output, errors) <- git root ["rev-list", "-1", "--first-parent", "--before=" ++ show before, "HEAD"]
  case status of
    ExitSuccess -> pure (case lines output of [] -> Nothing; first : _ -> Just first)
    _ -> failWith ("git rev-list: " ++ firstLine errors)

keepListAt :: FilePath -> String -> IO (Set.Set RelativePath)
keepListAt root ref = do
  present <- pathInTree root ref keepListPath
  if not present
    then pure Set.empty
    else do
      content <- gitShow root ref keepListPath
      case decodeUtf8' content of
        Left _ -> failWith ("invalid UTF-8 in " ++ keepListPath ++ " at " ++ ref)
        Right text -> pure (forceSet (Set.fromList (mapMaybe keepListEntry (T.lines text))))

keepListEntry :: Text -> Maybe RelativePath
keepListEntry line =
  let trimmed = trimSpace (T.unpack line)
  in if null trimmed || "#" `isPrefixOf` trimmed
       then Nothing
       else Just (RelativePath (normaliseEntry trimmed))

normaliseEntry :: FilePath -> FilePath
normaliseEntry = dropWhileEnd (== '/') . normalise

pathInTree :: FilePath -> String -> FilePath -> IO Bool
pathInTree root ref path = do
  (status, output, errors) <- git root ["ls-tree", ref, "--", path]
  case status of
    ExitSuccess -> pure (not (null (lines output)))
    _ -> failWith ("git ls-tree: " ++ firstLine errors)

gitShow :: FilePath -> String -> FilePath -> IO B.ByteString
gitShow root ref path = do
  (status, output, errors) <- gitBytes root ["show", ref ++ ":" ++ path]
  case status of
    ExitSuccess -> pure output
    _ -> failWith ("git show " ++ ref ++ ":" ++ path ++ ": " ++ firstLine errors)

citationsAt :: FilePath -> String -> IO (Set.Set RelativePath)
citationsAt root ref = do
  (status, output, errors) <- gitBytes root ["grep", "-h", "-o", "-I", "-E", "-e", citationPattern, ref]
  case status of
    ExitSuccess -> pure (forceSet (foldl' add Set.empty (BC.lines output)))
    ExitFailure 1 -> pure Set.empty
    _ -> failWith ("git grep: " ++ firstLine errors)
 where
  add found line = Set.insert (RelativePath (BC.unpack line)) found


strongCitationsAt :: FilePath -> String -> IO (Set.Set RelativePath)
strongCitationsAt root ref = do
  present <- pathInTree root ref evidencePath
  if not present
    then pure Set.empty
    else do
      content <- gitShow root ref evidencePath
      pure (strongCitations content)

strongCitations :: B.ByteString -> Set.Set RelativePath
strongCitations content = forceSet (foldl' add Set.empty (BC.lines content))
 where
  add found line = case eitherDecodeStrict' line of
    Right record -> foldl' (\values path -> Set.insert (RelativePath (T.unpack path)) values) found (evidencePaths record)
    Left _ -> found

liveBranchPins :: FilePath -> BranchInfo -> IO (Set.Set RelativePath, Set.Set RelativePath)
liveBranchPins root branch = do
  base <- mergeBase root branch
  citations <- addedCitations root base branch
  strong <- addedStrongCitations root base branch
  pure (citations, strong)

mergeBase :: FilePath -> BranchInfo -> IO String
mergeBase root branch = do
  (status, output, errors) <- git root ["merge-base", "HEAD", branchRefName branch]
  case status of
    ExitSuccess -> case lines output of
      first : _ -> pure first
      [] -> failWith ("git merge-base " ++ branchRefName branch ++ ": no merge base")
    _ -> failWith ("git merge-base " ++ branchRefName branch ++ ": " ++ firstLine errors)

addedCitations :: FilePath -> String -> BranchInfo -> IO (Set.Set RelativePath)
addedCitations root base branch = do
  (status, output, errors) <- gitBytes root ["diff", "--unified=0", "--no-color", base, branchRefName branch]
  case status of
    ExitSuccess -> pure (forceSet (foldl' add Set.empty (B.split 10 output)))
    _ -> failWith ("git diff: " ++ firstLine errors)
 where
  add found line
    | isAddedLine line = foldl' (\values citation -> Set.insert (RelativePath citation) values) found (scanCitations (BC.unpack (B.drop 1 line)))
    | otherwise = found

addedStrongCitations :: FilePath -> String -> BranchInfo -> IO (Set.Set RelativePath)
addedStrongCitations root base branch = do
  (status, output, errors) <- gitBytes root ["diff", "--unified=0", "--no-color", base, branchRefName branch, "--", evidencePath]
  case status of
    ExitSuccess -> pure (forceSet (foldl' add Set.empty (B.split 10 output)))
    _ -> failWith ("git diff: " ++ firstLine errors)
 where
  add found line
    | isAddedLine line = case eitherDecodeStrict' (B.drop 1 line) of
        Right record -> foldl' (\values path -> Set.insert (RelativePath (T.unpack path)) values) found (evidencePaths record)
        Left _ -> found
    | otherwise = found

isAddedLine :: B.ByteString -> Bool
isAddedLine line = B.take 1 line == B.singleton 43 && B.take 3 line /= "+++"

scanCitations :: String -> [String]
scanCitations text =
  case findPrefix text of
    Nothing -> []
    Just rest ->
      let after = drop (length citationPrefix) rest
          (body, remainder) = span isCitationChar after
      in if null body
           then scanCitations (drop 1 rest)
           else (citationPrefix ++ body) : scanCitations remainder
 where
  findPrefix value
    | citationPrefix `isPrefixOf` value = Just value
    | null value = Nothing
    | otherwise = findPrefix (drop 1 value)

isCitationChar :: Char -> Bool
isCitationChar character = isAscii character && (isAlphaNum character || character `elem` ("._@+/=-" :: String))

sweepScratchRoot :: Sweep -> WorkspaceRoot -> IO ()
sweepScratchRoot sweep (WorkspaceRoot root) = do
  let scratch = root </> "private-data/scratch"
  status <- lstatMaybe (sweepCounts sweep) scratch
  case status of
    Just rootStatus | isDirectory rootStatus && not (isSymbolicLink rootStatus) -> do
      entries <- listEntries (sweepCounts sweep) scratch
      forM_ entries $ \name -> do
        let absolute = scratch </> name
            relative = RelativePath ("private-data/scratch" </> name)
        entryStatus <- lstatMaybe (sweepCounts sweep) absolute
        case entryStatus of
          Nothing -> pure ()
          Just fileStatus
            | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
                owned <- isOwnedTask sweep absolute name
                when owned (bumpOwnedDirectory (sweepCounts sweep))
                sweepRuns sweep (absolute </> "runs")
                unless owned $ do
                  _ <- walkDirectory sweep (deviceID rootStatus) absolute relative Unowned (not (isExpired sweep fileStatus))
                  pure ()
            | otherwise -> do
                _ <- visitEntry sweep (deviceID rootStatus) Unowned absolute relative
                pure ()
    _ -> pure ()

sweepVerifyRoot :: Sweep -> WorkspaceRoot -> IO ()
sweepVerifyRoot sweep (WorkspaceRoot root) = do
  let verify = root </> "private-data/verify-candidate"
  status <- lstatMaybe (sweepCounts sweep) verify
  case status of
    Just rootStatus | isDirectory rootStatus && not (isSymbolicLink rootStatus) -> do
      entries <- listEntries (sweepCounts sweep) verify
      forM_ entries $ \name ->
        unless ("baselines" `isPrefixOf` name) $ do
          let absolute = verify </> name
              relative = RelativePath ("private-data/verify-candidate" </> name)
          entryStatus <- lstatMaybe (sweepCounts sweep) absolute
          case entryStatus of
            Nothing -> pure ()
            Just fileStatus
              | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
                  _ <- walkDirectory sweep (deviceID rootStatus) absolute relative Unowned (not (isExpired sweep fileStatus))
                  pure ()
              | otherwise -> do
                  _ <- visitEntry sweep (deviceID rootStatus) Unowned absolute relative
                  pure ()
    _ -> pure ()

isOwnedTask :: Sweep -> FilePath -> String -> IO Bool
isOwnedTask sweep taskDirectory name = do
  let branchOwned =
        any
          (\branch -> maybe False (\date -> sweepNow sweep - fromIntegral date <= sweepMaxAge sweep) (Map.lookup branch (sweepBranches sweep)))
          ["agent/" ++ name, name]
  if branchOwned
    then pure True
    else do
      holdOwned <- holdKeeps sweep (taskDirectory </> "HOLD")
      if holdOwned
        then pure True
        else do
          status <- probeStatus taskDirectory
          case status of
            Left _ -> pure True
            Right fileStatus -> recentEntryInTree sweep taskDirectory (deviceID fileStatus)

recentEntryInTree :: Sweep -> FilePath -> DeviceID -> IO Bool
recentEntryInTree sweep directory rootDevice = do
  listing <- probeEntries directory
  case listing of
    Left _ -> pure True
    Right entries -> go entries
 where
  go [] = pure False
  go (name : rest) = do
    status <- probeStatus (directory </> name)
    case status of
      Left _ -> pure True
      Right fileStatus
        | deviceID fileStatus /= rootDevice -> go rest
        | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
            hit <- recentEntryInTree sweep (directory </> name) rootDevice
            if hit then pure True else go rest
        | otherwise ->
            if sweepNow sweep - max (modificationTimeHiRes fileStatus) (statusChangeTimeHiRes fileStatus) <= sweepMaxAge sweep
              then pure True
              else go rest

holdKeeps :: Sweep -> FilePath -> IO Bool
holdKeeps sweep path = do
  status <- lstatMaybe (sweepCounts sweep) path
  case status of
    Just fileStatus
      | isRegularFile fileStatus && not (isSymbolicLink fileStatus) -> do
          content <- readBytesMaybe (sweepCounts sweep) path
          pure (maybe False (holdActive sweep fileStatus) (content >>= parseHold))
    _ -> pure False

holdActive :: Sweep -> FileStatus -> Hold -> Bool
holdActive sweep fileStatus (Hold _ untilDay) =
  let untilEnd = utcTimeToPOSIXSeconds (UTCTime untilDay 0) + 86400
      holdMtime = modificationTimeHiRes fileStatus
      clampedEnd = min untilEnd (holdMtime + 30 * 86400)
  in sweepNow sweep < clampedEnd

parseHold :: B.ByteString -> Maybe Hold
parseHold content = do
  let entries = map (trimSpace . BC.unpack) (BC.lines content)
      issues = [trimSpace (dropPrefix "issue:" entry) | entry <- entries, "issue:" `isPrefixOf` entry]
      untils = [trimSpace (dropPrefix "until:" entry) | entry <- entries, "until:" `isPrefixOf` entry]
  issueText <- single issues
  untilText <- single untils
  if null issueText || not (all isDigit issueText) then Nothing else pure ()
  issue <- readMaybe issueText :: Maybe Integer
  day <- parseTimeM True defaultTimeLocale "%Y-%m-%d" untilText :: Maybe Day
  pure (Hold issue day)
 where
  single [value] = Just value
  single _ = Nothing
  dropPrefix :: String -> String -> String
  dropPrefix prefix value = drop (length prefix) value

walkDirectory :: Sweep -> DeviceID -> FilePath -> RelativePath -> Ownership -> Bool -> IO Bool
walkDirectory sweep rootDevice directory relative ownership keepDirectory = do
  allGone <-
    bracket (openDirStream directory) closeDirStream (loop directory relative)
      `catch` \(problem :: IOException) ->
        if isDoesNotExistError problem
          then pure True
          else do
            recordFailure (sweepCounts sweep)
            pure False
  if keepDirectory || not (sweepApply sweep) || not allGone
    then pure False
    else removeDirectoryEntry sweep directory
 where
  loop current currentRelative stream = go True
   where
    go allGone = readDirStreamMaybe stream >>= \entry -> case entry of
      Nothing -> pure allGone
      Just name
        | name == "." || name == ".." -> go allGone
        | otherwise -> do
            let child = current </> name
                childRelative = RelativePath (relativePathText currentRelative </> name)
            gone <- visitEntry sweep rootDevice ownership child childRelative
            if gone then go allGone else go False

visitEntry :: Sweep -> DeviceID -> Ownership -> FilePath -> RelativePath -> IO Bool
visitEntry sweep rootDevice ownership absolutePath relative = do
  status <- lstatMaybe (sweepCounts sweep) absolutePath
  case status of
    Nothing -> pure True
    Just fileStatus
      | deviceID fileStatus /= rootDevice -> pure False
      | isDirectory fileStatus && not (isSymbolicLink fileStatus) ->
          walkDirectory sweep rootDevice absolutePath relative ownership (ownership == Owned || not (isExpired sweep fileStatus))
      | ownership == Owned -> pure False
      | otherwise -> do
          let bytes = ByteCount (fromIntegral (fileSize fileStatus))
          if isPinned (sweepPins sweep) fileStatus relative
            then do
              bump (sweepCounts sweep) PinnedBucket bytes
              pure False
            else
              if isExpired sweep fileStatus
                then do
                  bump (sweepCounts sweep) ExpiredBucket bytes
                  if sweepApply sweep then removeEntry sweep absolutePath else pure False
                else do
                  bump (sweepCounts sweep) RetainedBucket bytes
                  pure False

isPinned :: Pins -> FileStatus -> RelativePath -> Bool
isPinned (Pins citations strong keep) fileStatus relative@(RelativePath value) =
  keepCovered || Set.member relative strong || (Set.member relative citations && fromIntegral (fileSize fileStatus) <= citationSizeLimit)
 where
  components = splitDirectories value
  keepCovered = any (`covers` components) (Set.toList keep)
  covers (RelativePath entry) path = take (length (splitDirectories entry)) path == splitDirectories entry

isExpired :: Sweep -> FileStatus -> Bool
isExpired sweep status =
  sweepNow sweep - max (modificationTimeHiRes status) (statusChangeTimeHiRes status) > sweepMaxAge sweep

sweepRuns :: Sweep -> FilePath -> IO ()
sweepRuns sweep runsDirectory = do
  status <- lstatMaybe (sweepCounts sweep) runsDirectory
  case status of
    Just fileStatus | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
      entries <- listEntries (sweepCounts sweep) runsDirectory
      forM_ entries $ \name -> do
        let runDirectory = runsDirectory </> name
        runStatus <- lstatMaybe (sweepCounts sweep) runDirectory
        case runStatus of
          Just directoryStatus | isDirectory directoryStatus && not (isSymbolicLink directoryStatus) -> do
            record <- readRunRecord (sweepCounts sweep) (runDirectory </> "run.json")
            case record of
              Nothing -> pure ()
              Just runRecord -> case recordOutcome runRecord of
                OutcomeOpen -> closeAbandonedRun sweep runDirectory
                _ -> do
                  expired <- shouldExpireRun sweep runRecord
                  when expired (expireWork sweep runDirectory)
          _ -> pure ()
    _ -> pure ()

shouldExpireRun :: Sweep -> RunRecord -> IO Bool
shouldExpireRun sweep runRecord = case recordOutcome runRecord of
  OutcomePassed -> pure True
  OutcomeFailed -> pure False
  OutcomeUnjudged -> case recordClosedAt runRecord of
    Nothing -> pure False
    Just closed -> pure (sweepNow sweep - closed > sweepWorkAge sweep)
  OutcomeOpen -> pure False

closeAbandonedRun :: Sweep -> FilePath -> IO ()
closeAbandonedRun sweep runDirectory = do
  closed <- withFreeLock (runDirectory </> "lock") $ if sweepApply sweep then rewriteUnjudged sweep runDirectory else pure True
  case closed of
    Just True -> bumpClosedAbandoned (sweepCounts sweep)
    _ -> pure ()

withFreeLock :: FilePath -> IO a -> IO (Maybe a)
withFreeLock path action = do
  opened <- (Just <$> openFd path ReadOnly defaultFileFlags) `catch` \(_ :: IOException) -> pure Nothing
  case opened of
    Nothing -> pure Nothing
    Just fileDescriptor -> do
      let Fd rawDescriptor = fileDescriptor
      acquired <- c_flock rawDescriptor lockExclusiveNonBlocking
      if acquired /= 0
        then closeFd fileDescriptor >> pure Nothing
        else do
          result <- action `finally` closeFd fileDescriptor
          pure (Just result)

rewriteUnjudged :: Sweep -> FilePath -> IO Bool
rewriteUnjudged sweep runDirectory =
  ((do
    content <- B.readFile (runDirectory </> "run.json")
    case eitherDecodeStrict' content of
      Right (Object object') -> do
        let closedAt = T.pack (formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" (posixSecondsToUTCTime (sweepNow sweep)))
            updated = Object (KM.insert "closed_at" (String closedAt) (KM.insert "outcome" (String "unjudged") object'))
        writeJsonAtomically runDirectory updated
        pure True
      _ -> pure False)
    `catch` \(_ :: IOException) -> recordFailure (sweepCounts sweep) >> pure False)

writeJsonAtomically :: FilePath -> Value -> IO ()
writeJsonAtomically directory value =
  bracketOnError (openTempFile directory "run.json.tmp") cleanup $ \(temporary, handle) -> do
    BL.hPut handle (encode value)
    hClose handle
    renameFile temporary (directory </> "run.json")
 where
  cleanup (temporary, handle) = do
    (hClose handle `catch` \(_ :: IOException) -> pure ())
    (removeFile temporary `catch` \(_ :: IOException) -> pure ())

readRunRecord :: IORef Counts -> FilePath -> IO (Maybe RunRecord)
readRunRecord counts path = do
  content <- readBytesMaybe counts path
  pure $ case content of
    Nothing -> Nothing
    Just bytes -> case eitherDecodeStrict' bytes of
      Right runRecord -> Just runRecord
      Left _ -> Nothing

expireWork :: Sweep -> FilePath -> IO ()
expireWork sweep runDirectory = do
  let workDirectory = runDirectory </> "work"
  status <- lstatMaybe (sweepCounts sweep) workDirectory
  case status of
    Nothing -> pure ()
    Just fileStatus -> do
      (bytes, removed) <-
        if isDirectory fileStatus && not (isSymbolicLink fileStatus)
          then
            if sweepApply sweep
              then removeTree sweep workDirectory
              else do
                measured <- measureTree sweep workDirectory
                pure (measured, True)
          else
            if sweepApply sweep
              then do
                ok <- removeEntry sweep workDirectory
                pure (if ok then ByteCount (fromIntegral (fileSize fileStatus)) else ByteCount 0, ok)
              else pure (ByteCount (fromIntegral (fileSize fileStatus)), True)
      addExpiredBytes (sweepCounts sweep) bytes
      when removed (bumpWorkRun (sweepCounts sweep))

measureTree :: Sweep -> FilePath -> IO ByteCount
measureTree sweep path = do
  status <- lstatMaybe (sweepCounts sweep) path
  case status of
    Nothing -> pure (ByteCount 0)
    Just fileStatus
      | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
          entries <- listEntries (sweepCounts sweep) path
          measured <- mapM (measureTree sweep . (path </>)) entries
          pure (ByteCount (sum (map byteCount measured)))
      | otherwise -> pure (ByteCount (fromIntegral (fileSize fileStatus)))

removeTree :: Sweep -> FilePath -> IO (ByteCount, Bool)
removeTree sweep path = do
  status <- lstatMaybe (sweepCounts sweep) path
  case status of
    Nothing -> pure (ByteCount 0, True)
    Just fileStatus
      | isDirectory fileStatus && not (isSymbolicLink fileStatus) -> do
          entries <- listEntries (sweepCounts sweep) path
          removed <- mapM (removeTree sweep . (path </>)) entries
          directoryRemoved <- removeDirectoryQuiet (sweepCounts sweep) path
          pure (ByteCount (sum (map (byteCount . fst) removed)), all snd removed && directoryRemoved)
      | otherwise -> do
          entryRemoved <- removeEntry sweep path
          pure (if entryRemoved then ByteCount (fromIntegral (fileSize fileStatus)) else ByteCount 0, entryRemoved)

removeEntry :: Sweep -> FilePath -> IO Bool
removeEntry sweep path = do
  removed <- retryWithParentWrite path (removeFile path)
  unless removed (recordFailure (sweepCounts sweep))
  pure removed

removeDirectoryEntry :: Sweep -> FilePath -> IO Bool
removeDirectoryEntry sweep path = do
  removed <- removeDirectoryQuiet (sweepCounts sweep) path
  when removed (modifyIORef' (sweepCounts sweep) (\counts -> counts {countRemovedDirectories = DirectoryCount (directoryCount (countRemovedDirectories counts) + 1)}))
  pure removed

removeDirectoryQuiet :: IORef Counts -> FilePath -> IO Bool
removeDirectoryQuiet counts path = do
  removed <- retryWithParentWrite path (removeDirectory path)
  unless removed (recordFailure counts)
  pure removed

retryWithParentWrite :: FilePath -> IO () -> IO Bool
retryWithParentWrite path action = attempt `catch` \(problem :: IOException) ->
  if isDoesNotExistError problem
    then pure True
    else
      if isPermissionError problem
        then do
          writable <- grantParentOwnerWrite path
          if writable then attempt `catch` \(_ :: IOException) -> pure False else pure False
        else pure False
 where
  attempt = action >> pure True

grantParentOwnerWrite :: FilePath -> IO Bool
grantParentOwnerWrite path =
  (do
    let parent = takeDirectory path
    status <- getSymbolicLinkStatus parent
    caller <- getEffectiveUserID
    if fileOwner status == caller && intersectFileModes (fileMode status) ownerWriteMode == nullFileMode
      then do
        setFileMode parent (unionFileModes (fileMode status) ownerWriteMode)
        pure True
      else pure False)
    `catch` \(_ :: IOException) -> pure False

lstatMaybe :: IORef Counts -> FilePath -> IO (Maybe FileStatus)
lstatMaybe counts path =
  (Just <$> getSymbolicLinkStatus path)
    `catch` \(problem :: IOException) -> do
      unless (isDoesNotExistError problem) (recordFailure counts)
      pure Nothing

readBytesMaybe :: IORef Counts -> FilePath -> IO (Maybe B.ByteString)
readBytesMaybe counts path =
  (Just <$> B.readFile path)
    `catch` \(problem :: IOException) -> do
      unless (isDoesNotExistError problem) (recordFailure counts)
      pure Nothing

listEntries :: IORef Counts -> FilePath -> IO [String]
listEntries counts directory =
  listRaw directory
    `catch` \(problem :: IOException) -> do
      unless (isDoesNotExistError problem) (recordFailure counts)
      pure []

listRaw :: FilePath -> IO [String]
listRaw directory =
  bracket (openDirStream directory) closeDirStream collect
 where
  collect stream = go []
   where
    go names = readDirStreamMaybe stream >>= \entry -> case entry of
      Nothing -> pure (reverse names)
      Just name
        | name == "." || name == ".." -> go names
        | otherwise -> go (name : names)

probeEntries :: FilePath -> IO (Either IOException [String])
probeEntries directory =
  (Right <$> listRaw directory) `catch` \(problem :: IOException) -> pure (Left problem)

probeStatus :: FilePath -> IO (Either IOException FileStatus)
probeStatus path =
  (Right <$> getSymbolicLinkStatus path) `catch` \(problem :: IOException) -> pure (Left problem)

bumpOwnedDirectory :: IORef Counts -> IO ()
bumpOwnedDirectory counts =
  modifyIORef' counts
    (\current -> current {countOwnedDirectories = DirectoryCount (directoryCount (countOwnedDirectories current) + 1)})

bumpWorkRun :: IORef Counts -> IO ()
bumpWorkRun counts =
  modifyIORef' counts
    (\current -> current {countExpiredWorkRuns = RunCount (runCount (countExpiredWorkRuns current) + 1)})

bumpClosedAbandoned :: IORef Counts -> IO ()
bumpClosedAbandoned counts =
  modifyIORef' counts
    (\current -> current {countClosedAbandonedRuns = RunCount (runCount (countClosedAbandonedRuns current) + 1)})

addExpiredBytes :: IORef Counts -> ByteCount -> IO ()
addExpiredBytes counts (ByteCount bytes) =
  modifyIORef' counts
    (\current -> current {countExpiredBytes = ByteCount (byteCount (countExpiredBytes current) + bytes)})

bump :: IORef Counts -> Bucket -> ByteCount -> IO ()
bump counts bucket (ByteCount bytes) = modifyIORef' counts $ \current -> case bucket of
  ExpiredBucket ->
    current
      { countExpiredFiles = FileCount (fileCount (countExpiredFiles current) + 1)
      , countExpiredBytes = ByteCount (byteCount (countExpiredBytes current) + bytes)
      }
  PinnedBucket ->
    current
      { countPinnedFiles = FileCount (fileCount (countPinnedFiles current) + 1)
      , countPinnedBytes = ByteCount (byteCount (countPinnedBytes current) + bytes)
      }
  RetainedBucket ->
    current
      { countRetainedFiles = FileCount (fileCount (countRetainedFiles current) + 1)
      , countRetainedBytes = ByteCount (byteCount (countRetainedBytes current) + bytes)
      }

recordFailure :: IORef Counts -> IO ()
recordFailure counts = modifyIORef' counts (\current -> current {countFailures = FailureCount (failureCount (countFailures current) + 1)})

trimSpace :: String -> String
trimSpace = dropWhileEnd isSpace . dropWhile isSpace

firstLine :: String -> String
firstLine = takeWhile (/= '\n')
