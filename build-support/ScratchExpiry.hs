{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, throwIO)
import Control.Monad (forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (stripPrefix)
import qualified Data.Set as Set
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime)
import System.Directory (canonicalizePath, doesPathExist, removeDirectory, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (isAbsolute, joinPath, normalise, splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (hPutStrLn, stderr)
import System.IO.Error (isDoesNotExistError, isPermissionError)
import System.Posix.Directory (closeDirStream, openDirStream, readDirStreamMaybe)
import System.Posix.Files (FileStatus, deviceID, fileMode, fileOwner, fileSize, getSymbolicLinkStatus, intersectFileModes, isDirectory, isSymbolicLink, modificationTimeHiRes, nullFileMode, ownerWriteMode, setFileMode, statusChangeTimeHiRes, unionFileModes)
import System.Posix.Types (DeviceID)
import System.Posix.User (getEffectiveUserID)
import System.Process (readProcessWithExitCode)

newtype WorkspaceRoot = WorkspaceRoot FilePath
newtype RelativePath = RelativePath FilePath deriving (Eq, Ord)
newtype OwnerName = OwnerName String deriving (Eq, Ord)
newtype ByteCount = ByteCount Integer
newtype FileCount = FileCount Integer
newtype DirectoryCount = DirectoryCount Integer
newtype FailureCount = FailureCount Integer
newtype AgeDays = AgeDays Integer
newtype EpochSeconds = EpochSeconds Integer

data Pins = Pins (Set.Set RelativePath) (Set.Set RelativePath)

data Counts = Counts
  { countExpiredFiles :: !FileCount
  , countExpiredBytes :: !ByteCount
  , countPinnedFiles :: !FileCount
  , countPinnedBytes :: !ByteCount
  , countRetainedFiles :: !FileCount
  , countRetainedBytes :: !ByteCount
  , countOwnedDirectories :: !DirectoryCount
  , countRemovedDirectories :: !DirectoryCount
  , countFailures :: !FailureCount
  }

data Sweep = Sweep
  { sweepNow :: POSIXTime
  , sweepMaxAge :: POSIXTime
  , sweepApply :: Bool
  , sweepPins :: Pins
  , sweepOwners :: Set.Set OwnerName
  , sweepCounts :: IORef Counts
  }

data Options = Options
  { optionWorkspace :: Maybe FilePath
  , optionMaxAgeDays :: Maybe AgeDays
  , optionApply :: Bool
  , optionNow :: Maybe EpochSeconds
  }

data Bucket = ExpiredBucket | PinnedBucket | RetainedBucket

citationPattern :: String
citationPattern = "private-data/[A-Za-z0-9._@+/=-]+"

keepListPath :: FilePath
keepListPath = "build-support/scratch-keep.txt"

baselinesPath :: FilePath
baselinesPath = "private-data/verify-candidate/baselines"

relativePathText :: RelativePath -> FilePath
relativePathText (RelativePath value) = value

byteCount :: ByteCount -> Integer
byteCount (ByteCount value) = value

fileCount :: FileCount -> Integer
fileCount (FileCount value) = value

directoryCount :: DirectoryCount -> Integer
directoryCount (DirectoryCount value) = value

failureCount :: FailureCount -> Integer
failureCount (FailureCount value) = value

ageDays :: AgeDays -> Integer
ageDays (AgeDays value) = value

epochSeconds :: EpochSeconds -> Integer
epochSeconds (EpochSeconds value) = value

main :: IO ()
main = (do
  arguments <- getArgs
  options <- parseOptions arguments
  case options of
    Options (Just workspaceInput) (Just days) applied nowOverride -> runSweep workspaceInput days applied nowOverride
    _ -> usage)
  `catch` \(problem :: IOException) -> failWith (firstLine (show problem))

usage :: IO a
usage = do
  hPutStrLn stderr "usage: scratch-expiry --workspace ROOT --max-age-days N [--apply] [--now EPOCH_SECONDS]"
  hPutStrLn stderr "  --now sets the reference time in epoch seconds; tests only."
  exitWith (ExitFailure 2)

failWith :: String -> IO a
failWith message = do
  hPutStrLn stderr ("scratch-expiry: " ++ message)
  exitWith (ExitFailure 1)

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing False Nothing)
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
  go options ("--now" : value : rest) = case optionNow options of
    Just _ -> usage
    Nothing -> case reads value :: [(Integer, String)] of
      [(seconds, "")] | seconds >= 0 -> go options {optionNow = Just (EpochSeconds seconds)} rest
      _ -> usage
  go options ("--apply" : rest) = go options {optionApply = True} rest
  go _ _ = usage

runSweep :: FilePath -> AgeDays -> Bool -> Maybe EpochSeconds -> IO ()
runSweep workspaceInput days applied nowOverride = do
  workspace@(WorkspaceRoot root) <- validateWorkspace workspaceInput
  pins <- collectPins workspace
  owners <- collectOwners workspace
  reference <- case nowOverride of
    Just seconds -> pure (fromIntegral (epochSeconds seconds))
    Nothing -> getPOSIXTime
  counts <- newIORef (Counts (FileCount 0) (ByteCount 0) (FileCount 0) (ByteCount 0) (FileCount 0) (ByteCount 0) (DirectoryCount 0) (DirectoryCount 0) (FailureCount 0))
  let sweep = Sweep reference (fromIntegral (ageDays days * 86400)) applied pins owners counts
  forM_
    [ ("private-data/scratch", RelativePath "private-data/scratch")
    , ("private-data/verify-candidate", RelativePath "private-data/verify-candidate")
    ]
    $ \(suffix, relative) -> sweepRoot sweep (root </> suffix) relative
  final <- readIORef counts
  BL.putStr
    ( encode
        ( object
            [ "applied" .= applied
            , "max_age_days" .= ageDays days
            , "expired_files" .= fileCount (countExpiredFiles final)
            , "expired_bytes" .= byteCount (countExpiredBytes final)
            , "pinned_files" .= fileCount (countPinnedFiles final)
            , "pinned_bytes" .= byteCount (countPinnedBytes final)
            , "retained_files" .= fileCount (countRetainedFiles final)
            , "retained_bytes" .= byteCount (countRetainedBytes final)
            , "owned_directories" .= directoryCount (countOwnedDirectories final)
            , "removed_directories" .= directoryCount (countRemovedDirectories final)
            , "failures" .= failureCount (countFailures final)
            ]
        )
        <> "\n"
    )
  unless (failureCount (countFailures final) == 0) (exitWith (ExitFailure 1))

validateWorkspace :: FilePath -> IO WorkspaceRoot
validateWorkspace input = do
  let normalized = normalise input
  unless (isAbsolute normalized && normalized == input) (failWith "workspace must be a normalized absolute path")
  present <- doesPathExist normalized
  unless present (failWith "workspace does not exist")
  resolved <- canonicalizePath normalized
  unless (resolved == normalized) (failWith "workspace path must be canonical")
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

collectPins :: WorkspaceRoot -> IO Pins
collectPins (WorkspaceRoot root) = do
  headPresent <- hasHead root
  if not headPresent
    then pure (Pins Set.empty Set.empty)
    else do
      references <- unmergedBranches root
      exactHead <- grepMatches root ["HEAD"] []
      exactBranches <- grepBatches root references []
      keepHead <- grepMatches root ["HEAD"] [keepListPath]
      keepBranches <- grepBatches root references [keepListPath]
      pure (Pins (Set.unions (exactHead : exactBranches)) (Set.unions (keepHead : keepBranches)))

hasHead :: FilePath -> IO Bool
hasHead root = do
  (status, _, _) <- readProcessWithExitCode "git" ["-C", root, "rev-parse", "--verify", "--quiet", "HEAD"] ""
  pure (status == ExitSuccess)

unmergedBranches :: FilePath -> IO [String]
unmergedBranches root = do
  (status, output, errors) <- readProcessWithExitCode "git" ["-C", root, "for-each-ref", "--no-merged", "HEAD", "--format=%(refname)", "refs/heads"] ""
  case status of
    ExitSuccess -> pure (lines output)
    _ -> failWith ("git for-each-ref: " ++ firstLine errors)

grepBatches :: FilePath -> [String] -> [FilePath] -> IO [Set.Set RelativePath]
grepBatches root references pathspec = mapM (\batch -> grepMatches root batch pathspec) (chunksOf 40 references)

grepMatches :: FilePath -> [String] -> [FilePath] -> IO (Set.Set RelativePath)
grepMatches root references pathspec = do
  (status, output, errors) <-
    readProcessWithExitCode
      "git"
      (["-C", root, "grep", "-h", "-o", "-I", "-E", "-e", citationPattern] ++ references ++ ["--"] ++ pathspec)
      ""
  case status of
    ExitSuccess -> pure (Set.fromList (map (RelativePath . stripTrailing) (lines output)))
    ExitFailure 1 -> pure Set.empty
    _ -> failWith ("git grep: " ++ firstLine errors)

collectOwners :: WorkspaceRoot -> IO (Set.Set OwnerName)
collectOwners (WorkspaceRoot root) = do
  branches <- branchOwnerNames root
  worktrees <- worktreeOwnerNames root
  pure (Set.fromList (map OwnerName (filter (not . null) (branches ++ worktrees))))

branchOwnerNames :: FilePath -> IO [String]
branchOwnerNames root = do
  (status, output, errors) <- readProcessWithExitCode "git" ["-C", root, "for-each-ref", "--format=%(refname)", "refs/heads"] ""
  case status of
    ExitSuccess -> pure [takeFileName reference | reference <- lines output, not (null reference)]
    _ -> failWith ("git for-each-ref: " ++ firstLine errors)

worktreeOwnerNames :: FilePath -> IO [String]
worktreeOwnerNames root = do
  (status, output, errors) <- readProcessWithExitCode "git" ["-C", root, "worktree", "list", "--porcelain"] ""
  case status of
    ExitSuccess ->
      pure
        [ takeFileName (normalise path)
        | line <- lines output
        , Just path <- [stripPrefix "worktree " line]
        , normalise path /= normalise root
        ]
    _ -> failWith ("git worktree list: " ++ firstLine errors)

sweepRoot :: Sweep -> FilePath -> RelativePath -> IO ()
sweepRoot sweep directory relative = do
  result <- (Right <$> getSymbolicLinkStatus directory) `catch` \(problem :: IOException) -> pure (Left problem)
  case result of
    Left problem
      | isDoesNotExistError problem -> pure ()
      | otherwise -> throwIO problem
    Right status
      | isDirectory status && not (isSymbolicLink status) -> do
          _ <- walkDirectory sweep (deviceID status) directory relative True
          pure ()
      | otherwise -> pure ()

walkDirectory :: Sweep -> DeviceID -> FilePath -> RelativePath -> Bool -> IO Bool
walkDirectory sweep rootDevice directory relative isRoot = do
  allGone <-
    bracket (openDirStream directory) closeDirStream (loop directory relative)
      `catch` \(problem :: IOException) ->
        if isDoesNotExistError problem
          then pure True
          else do
            recordFailure (sweepCounts sweep)
            pure False
  if isRoot || not (sweepApply sweep) || not allGone
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
            gone <- visitEntry sweep rootDevice child childRelative
            if gone then go allGone else go False

visitEntry :: Sweep -> DeviceID -> FilePath -> RelativePath -> IO Bool
visitEntry sweep rootDevice absolutePath relative = do
  result <- (Right <$> getSymbolicLinkStatus absolutePath) `catch` \(problem :: IOException) -> pure (Left problem)
  case result of
    Left problem
      | isDoesNotExistError problem -> pure True
      | otherwise -> do
          recordFailure (sweepCounts sweep)
          pure False
    Right status
      | relativePathText relative == baselinesPath -> pure False
      | deviceID status /= rootDevice -> pure False
      | isDirectory status && not (isSymbolicLink status) ->
          if isOwnedTaskDirectory (sweepOwners sweep) relative
            then do
              bumpOwnedDirectory (sweepCounts sweep)
              pure False
            else walkDirectory sweep rootDevice absolutePath relative False
      | otherwise -> do
          let bytes = ByteCount (fromIntegral (fileSize status))
          if isPinned (sweepPins sweep) relative
            then do
              bump (sweepCounts sweep) PinnedBucket bytes
              pure False
            else
              if isExpired sweep status
                then do
                  bump (sweepCounts sweep) ExpiredBucket bytes
                  if sweepApply sweep then removeEntry sweep absolutePath else pure False
                else do
                  bump (sweepCounts sweep) RetainedBucket bytes
                  pure False

isPinned :: Pins -> RelativePath -> Bool
isPinned (Pins exact keep) relative@(RelativePath value) =
  Set.member relative exact || any (\candidate -> Set.member candidate keep) (prefixes value)
 where
  prefixes text = [RelativePath (joinPath (take count components)) | count <- [1 .. length components]]
  components = splitDirectories text

isExpired :: Sweep -> FileStatus -> Bool
isExpired sweep status =
  sweepNow sweep - max (modificationTimeHiRes status) (statusChangeTimeHiRes status) > sweepMaxAge sweep

isOwnedTaskDirectory :: Set.Set OwnerName -> RelativePath -> Bool
isOwnedTaskDirectory owners relative =
  let path = relativePathText relative
  in takeDirectory path == "private-data/scratch" && ownsName owners (takeFileName path)

ownsName :: Set.Set OwnerName -> String -> Bool
ownsName owners name =
  Set.member (OwnerName name) owners
    || any (\candidate -> Set.member (OwnerName candidate) owners) [take index name | (index, character) <- zip [0 ..] name, character == '-']

bumpOwnedDirectory :: IORef Counts -> IO ()
bumpOwnedDirectory counts =
  modifyIORef' counts
    (\current -> current {countOwnedDirectories = DirectoryCount (directoryCount (countOwnedDirectories current) + 1)})

removeEntry :: Sweep -> FilePath -> IO Bool
removeEntry sweep path = do
  removed <- retryWithParentWrite path (removeFile path)
  unless removed (recordFailure (sweepCounts sweep))
  pure removed

removeDirectoryEntry :: Sweep -> FilePath -> IO Bool
removeDirectoryEntry sweep path = do
  removed <- retryWithParentWrite path (removeDirectory path)
  unless removed (recordFailure (sweepCounts sweep))
  when removed (modifyIORef' (sweepCounts sweep) (\counts -> counts {countRemovedDirectories = DirectoryCount (directoryCount (countRemovedDirectories counts) + 1)}))
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

chunksOf :: Int -> [a] -> [[a]]
chunksOf size values
  | null values = []
  | otherwise = let (prefix, suffix) = splitAt size values in prefix : chunksOf size suffix

stripTrailing :: String -> String
stripTrailing = reverse . dropWhile (`elem` ".,)/") . reverse

firstLine :: String -> String
firstLine = takeWhile (/= '\n')
