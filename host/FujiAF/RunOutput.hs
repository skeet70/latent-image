{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | Run directories under @private-data/scratch/NAME/runs/@ and the task output budget that
-- bounds them.
module FujiAF.RunOutput
  ( Task
  , Label
  , parseLabel
  , labelFromText
  , labelText
  , Opener (..)
  , Outcome (..)
  , CloseFailure (..)
  , CloseResult (..)
  , RunPaths (..)
  , RunHandle
  , runPaths
  , OpenRequest (..)
  , OpenResult (..)
  , Refusal (..)
  , Microseconds (..)
  , resolveTask
  , openRun
  , closeRun
  , makeRunLockInheritable
  , keepInput
  , describeRefusal
  , runEnvironment
  , awaitWithinWorkBudget
  , resolveWorkBudget
  , defaultTaskBudget
  , defaultWorkBudget
  , defaultPollMicroseconds
  , resultLimitBytes
  ) where

import Control.Concurrent (forkIO)
import Control.Concurrent.MVar (MVar, newEmptyMVar, putMVar, readMVar)
import Control.Exception (IOException, catch, finally, onException, throwIO, try)
import Control.Monad (filterM, foldM, forM, forM_, void, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (FromJSON (parseJSON), Object, ToJSON (toJSON), Value (Object, String), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (isDigit)
import Data.List (intercalate, sort, sortOn, stripPrefix)
import Data.Maybe (fromMaybe)
import qualified Data.Set as Set
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Time.Clock (UTCTime, getCurrentTime)
import Data.Time.Clock.POSIX (getPOSIXTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Foreign.C.Error (throwErrno)
import Foreign.C.Types (CInt (..))
import FujiAF.StorageBudget (StorageBytes (..), parseByteCount)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesPathExist, listDirectory, makeAbsolute, removePathForcibly, renameDirectory, renameFile)
import System.Environment (getEnvironment, lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath (isAbsolute, makeRelative, splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO.Error (isAlreadyExistsError, isDoesNotExistError)
import System.Posix.Files (FileStatus, deviceID, fileBlocks, fileID, fileSize, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink)
import System.Posix.IO (FdOption (CloseOnExec), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setFdOption)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (sigKILL, sigTERM, signalProcessGroup)
import System.Posix.Types (DeviceID, Fd (..), FileID, ProcessID)
import System.Process (ProcessHandle, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

lockExclusive, lockExclusiveNonBlocking :: CInt
lockExclusive = 2
lockExclusiveNonBlocking = 6

data Task = Task
  { taskName :: String
  , taskWorkspace :: FilePath
  }

newtype Label = Label String

parseLabel :: String -> Either String Label
parseLabel input
  | null input = Left "label must not be empty"
  | all labelCharacter input = Right (Label input)
  | otherwise = Left ("label must use only A-Z, a-z, 0-9, '.', '_' and '-': " ++ show input)

labelFromText :: String -> Label
labelFromText input =
  let cleaned = map (\character -> if labelCharacter character then character else '-') input
  in Label (if null cleaned then "run" else cleaned)

labelText :: Label -> String
labelText (Label value) = value

labelCharacter :: Char -> Bool
labelCharacter character =
  (character >= 'a' && character <= 'z')
    || (character >= 'A' && character <= 'Z')
    || isDigit character
    || character `elem` ("._-" :: String)

data Opener = Exec | Slot

openerText :: Opener -> String
openerText Exec = "exec"
openerText Slot = "slot"

data Outcome = OutcomeOpen | OutcomePassed | OutcomeFailed | OutcomeUnjudged

outcomeText :: Outcome -> String
outcomeText OutcomeOpen = "open"
outcomeText OutcomePassed = "passed"
outcomeText OutcomeFailed = "failed"
outcomeText OutcomeUnjudged = "unjudged"

data CloseFailure = ResultTooLarge | WorkBudget | Signal Int

failureText :: CloseFailure -> String
failureText ResultTooLarge = "result-too-large"
failureText WorkBudget = "work-budget"
failureText (Signal _) = "signal"

data RunPaths = RunPaths
  { runDirectory :: FilePath
  , runWork :: FilePath
  , runResult :: FilePath
  }

data CloseResult = CloseResult
  { closeOutcome :: Outcome
  , closeFailure :: Maybe CloseFailure
  , closePaths :: RunPaths
  }

data RunHandle = RunHandle
  { handleTask :: Task
  , handlePaths :: RunPaths
  , handleLabel :: Label
  , handleOpener :: Opener
  , handleCommand :: [String]
  , handleCommit :: Maybe String
  , handleOpenedAt :: UTCTime
  , handleWorkBudget :: StorageBytes
  , handleLock :: Fd
  }

runPaths :: RunHandle -> RunPaths
runPaths = handlePaths

data OpenRequest = OpenRequest
  { openTask :: Task
  , openLabel :: Label
  , openOpener :: Opener
  , openCommand :: [String]
  , openWorkBudget :: StorageBytes
  }

data OpenResult = RunOpened RunHandle | RunRefused Refusal

data Refusal = Refusal
  { refusalMeasured :: StorageBytes
  , refusalLimit :: StorageBytes
  , refusalChildren :: [(FilePath, StorageBytes)]
  , refusalDirectory :: FilePath
  }

newtype Microseconds = Microseconds Int

defaultTaskBudget, defaultWorkBudget :: StorageBytes
defaultTaskBudget = StorageBytes (10 * 1024 ^ (3 :: Int))
defaultWorkBudget = StorageBytes (20 * 1024 ^ (3 :: Int))

defaultPollMicroseconds :: Microseconds
defaultPollMicroseconds = Microseconds 60_000_000

resultLimitBytes :: Integer
resultLimitBytes = 16 * 1024 ^ (2 :: Int)

taskScratch :: Task -> FilePath
taskScratch task = taskWorkspace task </> "private-data" </> "scratch" </> taskName task

taskRuns :: Task -> FilePath
taskRuns = (</> "runs") . taskScratch

taskInputs :: Task -> FilePath
taskInputs = (</> "inputs") . taskScratch

resolveTask :: IO (Maybe Task)
resolveTask = do
  root <- gitOutput ["rev-parse", "--show-toplevel"]
  case root of
    Nothing -> pure Nothing
    Just workspace -> do
      present <- doesPathExist (workspace </> "private-data")
      if not present
        then pure Nothing
        else do
          branch <- gitOutput ["symbolic-ref", "--short", "HEAD"]
          pure (Just (Task (taskNameFromBranch branch) workspace))

taskNameFromBranch :: Maybe String -> String
taskNameFromBranch Nothing = "detached"
taskNameFromBranch (Just branch) =
  map (\character -> if character == '/' then '-' else character) (fromMaybe branch (stripPrefix "agent/" branch))

gitOutput :: [String] -> IO (Maybe String)
gitOutput arguments = do
  result <- try (readProcessWithExitCode "git" arguments "") :: IO (Either IOException (ExitCode, String, String))
  pure $ case result of
    Right (ExitSuccess, output, _) -> case lines output of
      line : _ | not (null line) -> Just line
      _ -> Nothing
    _ -> Nothing

gitCommit :: FilePath -> IO (Maybe String)
gitCommit workspace = gitOutput ["-C", workspace, "rev-parse", "HEAD"]

openRun :: OpenRequest -> IO OpenResult
openRun request = do
  let task = openTask request
  createDirectoryIfMissing True (taskScratch task)
  createDirectoryIfMissing True (taskRuns task)
  markAbandoned task
  limit <- taskBudget
  (measured, _) <- measureTaskBudget task limit
  if measured > budgetBytes limit
    then do
      _ <- removeOldestFailedWork task (measured - budgetBytes limit)
      (after, children) <- measureTaskFresh task
      now <- currentEpoch
      writeBudgetCache task after now
      if after > budgetBytes limit
        then pure (RunRefused (Refusal (StorageBytes after) limit (largestChildren children) (taskScratch task)))
        else createRun request
    else createRun request

createRun :: OpenRequest -> IO OpenResult
createRun request = do
  now <- getCurrentTime
  let task = openTask request
      label = openLabel request
      stamp = utcStamp now
  directory <- createRunDirectory (taskRuns task) (stamp ++ "-" ++ labelText label) 2
  let paths = RunPaths directory (directory </> "work") (directory </> "result")
  onException
    (do
      createDirectory (runWork paths)
      createDirectory (runResult paths)
      lock <- openLock (directory </> "lock")
      commit <- gitCommit (taskWorkspace task) `onException` closeFd lock
      let handle = RunHandle task paths label (openOpener request) (openCommand request) commit now (openWorkBudget request) lock
      writeRunJson directory (openRecord task label (openOpener request) (openCommand request) (openWorkBudget request) commit now)
        `onException` closeFd lock
      pure (RunOpened handle))
    (removePathForcibly directory)

createRunDirectory :: FilePath -> String -> Int -> IO FilePath
createRunDirectory runs base suffix = do
  let name = if suffix == 2 then base else base ++ "-" ++ show suffix
      path = runs </> name
  created <- try (createDirectory path) :: IO (Either IOException ())
  case created of
    Right () -> pure path
    Left problem
      | isAlreadyExistsError problem -> createRunDirectory runs base (suffix + 1)
      | otherwise -> throwIO problem

openLock :: FilePath -> IO Fd
openLock path = do
  fd@(Fd descriptor) <- openFd path ReadWrite defaultFileFlags { creat = Just 0o600 }
  setFdOption fd CloseOnExec True
  acquired <- c_flock descriptor lockExclusive
  if acquired == 0
    then pure fd
    else closeFd fd >> throwErrno "run-output lock"

-- | The lock descriptor survives @exec@, so the next opener sees the run as live.
makeRunLockInheritable :: RunHandle -> IO ()
makeRunLockInheritable handle = setFdOption (handleLock handle) CloseOnExec False

data LockState = LockAbsent | LockTaken Fd | LockBusy

probeLock :: FilePath -> IO LockState
probeLock path = do
  present <- entryExists path
  if not present
    then pure LockAbsent
    else do
      opened <- try (openFd path ReadWrite defaultFileFlags) :: IO (Either IOException Fd)
      case opened of
        Left problem | isDoesNotExistError problem -> pure LockAbsent
        Left _ -> pure LockBusy
        Right fd@(Fd descriptor) -> do
          acquired <- c_flock descriptor lockExclusiveNonBlocking
          if acquired == 0
            then pure (LockTaken fd)
            else closeFd fd >> pure LockBusy

releaseLock :: LockState -> IO ()
releaseLock (LockTaken fd) = closeFd fd
releaseLock _ = pure ()

lockHeld :: LockState -> Bool
lockHeld LockBusy = True
lockHeld _ = False

markAbandoned :: Task -> IO ()
markAbandoned task = do
  directories <- runDirectories task
  forM_ directories $ \directory -> do
    record <- readRunJson directory
    case record of
      Just object' | objectText object' "outcome" == Just "open" -> do
        state <- probeLock (directory </> "lock")
        case state of
          LockBusy -> releaseLock state
          _ -> do
            now <- getCurrentTime
            writeRunJson directory (markUnjudged object' (utcIso now)) `finally` releaseLock state
      _ -> pure ()

markUnjudged :: Object -> String -> Value
markUnjudged object' closedAt =
  Object (KM.insert "closed_at" (String (T.pack closedAt)) (KM.insert "outcome" (String "unjudged") object'))

runDirectories :: Task -> IO [FilePath]
runDirectories task = do
  let runs = taskRuns task
  present <- doesDirectoryExist runs
  if not present
    then pure []
    else do
      names <- listDirectory runs
      directories <- filterM (isRunDirectory . (runs </>)) (sort names)
      pure (map (runs </>) directories)
 where
  isRunDirectory path = do
    status <- getSymbolicLinkStatus path
    pure (isDirectory status && not (isSymbolicLink status))

measureTaskBudget :: Task -> StorageBytes -> IO (Integer, [(FilePath, Integer)])
measureTaskBudget task limit = do
  now <- currentEpoch
  lifetime <- measureLifetime
  cached <- readBudgetCache (taskRuns task </> ".budget")
  case cached of
    Just (BudgetCache bytes measuredAt)
      | bytes <= budgetBytes limit && now - measuredAt < lifetime -> pure (bytes, [])
    _ -> do
      (bytes, children) <- measureTaskFresh task
      writeBudgetCache task bytes now
      pure (bytes, children)

measureLifetime :: IO Integer
measureLifetime = do
  raw <- lookupEnv "XM1_TASK_MEASURE_SECONDS"
  case raw of
    Nothing -> pure budgetCacheSeconds
    Just text -> case readMaybe text :: Maybe Integer of
      Just seconds | seconds >= 0 -> pure seconds
      _ -> ioError (userError "XM1_TASK_MEASURE_SECONDS must be a non-negative integer")

measureTaskFresh :: Task -> IO (Integer, [(FilePath, Integer)])
measureTaskFresh task = do
  held <- heldWorkPaths task
  measureChildren (taskScratch task) held

heldWorkPaths :: Task -> IO (Set.Set FilePath)
heldWorkPaths task = do
  directories <- runDirectories task
  paths <- forM directories $ \directory -> do
    state <- probeLock (directory </> "lock")
    releaseLock state
    pure (if lockHeld state then Just (directory </> "work") else Nothing)
  pure (Set.fromList [path | Just path <- paths])

measureChildren :: FilePath -> Set.Set FilePath -> IO (Integer, [(FilePath, Integer)])
measureChildren root excluded = do
  names <- listDirectory root `catch` missingRoot
  (_, total, children) <- foldM step (Set.empty, 0, []) names
  pure (total, reverse children)
 where
  step (seen, total, children) name = do
    let path = root </> name
    status <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
    case status of
      Left _ -> pure (seen, total, children)
      Right entry -> do
        (next, bytes) <- measureEntry seen excluded path entry
        pure (next, total + bytes, (name, bytes) : children)

measureEntry :: Set.Set (DeviceID, FileID) -> Set.Set FilePath -> FilePath -> FileStatus -> IO (Set.Set (DeviceID, FileID), Integer)
measureEntry seen excluded path status
  | Set.member path excluded = pure (seen, 0)
  | otherwise = do
      let key = (deviceID status, fileID status)
          own = if Set.member key seen then 0 else allocatedBytes status
          next = Set.insert key seen
      if isDirectory status && not (isSymbolicLink status)
        then do
          names <- listDirectory path `catch` missingDirectory
          foldM (step path) (next, own) names
        else pure (next, own)
 where
  step directory (current, total) name = do
    let child = directory </> name
    status' <- try (getSymbolicLinkStatus child) :: IO (Either IOException FileStatus)
    case status' of
      Left _ -> pure (current, total)
      Right entry -> do
        (next, bytes) <- measureEntry current excluded child entry
        pure (next, total + bytes)
  missingDirectory problem = if isDoesNotExistError problem then pure [] else throwIO problem

missingRoot :: IOException -> IO [FilePath]
missingRoot problem = if isDoesNotExistError problem then pure [] else throwIO problem

allocatedBytes :: FileStatus -> Integer
allocatedBytes status = maybe 0 fromIntegral (fileBlocks status) * 512

measureAllocated :: FilePath -> IO Integer
measureAllocated path = do
  status <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
  case status of
    Left _ -> pure 0
    Right entry -> snd <$> measureEntry Set.empty Set.empty path entry

removeOldestFailedWork :: Task -> Integer -> IO Integer
removeOldestFailedWork task target = do
  directories <- runDirectories task
  go directories target 0
 where
  go [] _ removed = pure removed
  go (directory : rest) remaining removed
    | remaining <= 0 = pure removed
    | otherwise = do
        record <- readRunJson directory
        case record of
          Just object' | objectText object' "outcome" == Just "failed" -> do
            state <- probeLock (directory </> "lock")
            if lockHeld state
              then releaseLock state >> go rest remaining removed
              else do
                releaseLock state
                let work = directory </> "work"
                present <- entryExists work
                if not present
                  then go rest remaining removed
                  else do
                    bytes <- measureAllocated work
                    removePathForcibly work
                    go rest (remaining - bytes) (removed + bytes)
          _ -> go rest remaining removed

largestChildren :: [(FilePath, Integer)] -> [(FilePath, StorageBytes)]
largestChildren children = take 5 [(name, StorageBytes bytes) | (name, bytes) <- sortOn (negate . snd) children]

data BudgetCache = BudgetCache Integer Integer

instance FromJSON BudgetCache where
  parseJSON = withObject "budget cache" $ \object' ->
    BudgetCache <$> object' .: "bytes" <*> object' .: "measured_at"

instance ToJSON BudgetCache where
  toJSON (BudgetCache bytes measuredAt) = object ["bytes" .= bytes, "measured_at" .= measuredAt]

budgetCacheSeconds :: Integer
budgetCacheSeconds = 600

readBudgetCache :: FilePath -> IO (Maybe BudgetCache)
readBudgetCache path = do
  present <- doesPathExist path
  if not present
    then pure Nothing
    else do
      bytes <- try (B.readFile path) :: IO (Either IOException B.ByteString)
      pure $ case bytes of
        Left _ -> Nothing
        Right content -> either (const Nothing) Just (eitherDecodeStrict' content)

writeBudgetCache :: Task -> Integer -> Integer -> IO ()
writeBudgetCache task bytes now =
  BL.writeFile (taskRuns task </> ".budget") (encode (BudgetCache bytes now))

currentEpoch :: IO Integer
currentEpoch = floor <$> getPOSIXTime

taskBudget :: IO StorageBytes
taskBudget = do
  raw <- maybe "10G" id <$> lookupEnv "XM1_TASK_OUTPUT_MAX"
  either (ioError . userError) pure (parseByteCount raw)

-- | The per-run work limit: @XM1_RUN_WORK_MAX@, else 20 GiB.
resolveWorkBudget :: IO StorageBytes
resolveWorkBudget = do
  raw <- maybe "20G" id <$> lookupEnv "XM1_RUN_WORK_MAX"
  either (ioError . userError) pure (parseByteCount raw)

budgetBytes :: StorageBytes -> Integer
budgetBytes (StorageBytes value) = value

openRecord :: Task -> Label -> Opener -> [String] -> StorageBytes -> Maybe String -> UTCTime -> Value
openRecord task label opener command workBudget commit openedAt =
  object
    [ "schema_version" .= (1 :: Int)
    , "task" .= taskName task
    , "label" .= labelText label
    , "opener" .= openerText opener
    , "command" .= command
    , "commit" .= commit
    , "work_budget_bytes" .= budgetBytes workBudget
    , "opened_at" .= utcIso openedAt
    , "closed_at" .= (Nothing :: Maybe String)
    , "exit_status" .= (Nothing :: Maybe Int)
    , "outcome" .= outcomeText OutcomeOpen
    , "reason" .= (Nothing :: Maybe String)
    , "work_file_count" .= (0 :: Integer)
    , "work_bytes" .= (0 :: Integer)
    , "work_sha256" .= hexText (SHA.finalize SHA.init)
    , "work_files" .= ([] :: [WorkFile])
    ]

writeRunJson :: FilePath -> Value -> IO ()
writeRunJson directory value = do
  pid <- getProcessID
  let temporary = directory </> (".run.json." ++ show pid ++ ".tmp")
  BL.writeFile temporary (encode value)
  renameFile temporary (directory </> "run.json")

readRunJson :: FilePath -> IO (Maybe Object)
readRunJson directory = do
  content <- try (B.readFile (directory </> "run.json")) :: IO (Either IOException B.ByteString)
  pure $ case content of
    Left _ -> Nothing
    Right bytes -> case eitherDecodeStrict' bytes of
      Right (Object object') -> Just object'
      _ -> Nothing

objectText :: Object -> K.Key -> Maybe String
objectText object' key = case KM.lookup key object' of
  Just (String value) -> Just (T.unpack value)
  _ -> Nothing

closeRun :: RunHandle -> Maybe ExitCode -> Maybe CloseFailure -> IO CloseResult
closeRun handle status forced = do
  digest <- measureWork (runWork (handlePaths handle))
  resultBytes <- treeFileSize (runResult (handlePaths handle))
  now <- getCurrentTime
  let (outcome, failure) = decide resultBytes
      decide bytes = case forced of
        Just reason -> (OutcomeFailed, Just reason)
        Nothing
          | bytes > resultLimitBytes -> (OutcomeFailed, Just ResultTooLarge)
          | otherwise -> case status of
              Just ExitSuccess -> (OutcomePassed, Nothing)
              Just (ExitFailure _) -> (OutcomeFailed, Nothing)
              Nothing -> (OutcomeUnjudged, Nothing)
  case outcome of
    OutcomePassed -> removePathForcibly (runWork (handlePaths handle))
    OutcomeFailed -> removeOlderFailed (handleTask handle) (runDirectory (handlePaths handle)) (handleLabel handle)
    _ -> pure ()
  let record = closeRecord handle status outcome failure now digest
  (writeRunJson (runDirectory (handlePaths handle)) record >> pure (CloseResult outcome failure (handlePaths handle)))
    `finally` closeFd (handleLock handle)

closeRecord :: RunHandle -> Maybe ExitCode -> Outcome -> Maybe CloseFailure -> UTCTime -> WorkDigest -> Value
closeRecord handle status outcome failure now digest =
  object
    [ "schema_version" .= (1 :: Int)
    , "task" .= taskName (handleTask handle)
    , "label" .= labelText (handleLabel handle)
    , "opener" .= openerText (handleOpener handle)
    , "command" .= handleCommand handle
    , "commit" .= handleCommit handle
    , "work_budget_bytes" .= budgetBytes (handleWorkBudget handle)
    , "opened_at" .= utcIso (handleOpenedAt handle)
    , "closed_at" .= utcIso now
    , "exit_status" .= fmap exitStatusNumber status
    , "outcome" .= outcomeText outcome
    , "reason" .= fmap failureText failure
    , "work_file_count" .= workDigestCount digest
    , "work_bytes" .= workDigestBytes digest
    , "work_sha256" .= workDigestSha256 digest
    , "work_files" .= workDigestFiles digest
    ]

exitStatusNumber :: ExitCode -> Int
exitStatusNumber ExitSuccess = 0
exitStatusNumber (ExitFailure code) = code

removeOlderFailed :: Task -> FilePath -> Label -> IO ()
removeOlderFailed task currentDirectory label = do
  directories <- runDirectories task
  let current = takeFileName currentDirectory
  forM_ directories $ \directory -> when (takeFileName directory < current) $ do
    record <- readRunJson directory
    case record of
      Just object'
        | objectText object' "outcome" == Just "failed"
        , objectText object' "label" == Just (labelText label) -> do
            state <- probeLock (directory </> "lock")
            if lockHeld state
              then releaseLock state
              else do
                releaseLock state
                removePathForcibly (directory </> "work")
      _ -> pure ()

data WorkFile = WorkFile FilePath Integer String

instance ToJSON WorkFile where
  toJSON (WorkFile path bytes digest) =
    object ["path" .= path, "bytes" .= bytes, "sha256" .= digest]

data WorkDigest = WorkDigest
  { workDigestCount :: Integer
  , workDigestBytes :: Integer
  , workDigestSha256 :: String
  , workDigestFiles :: [WorkFile]
  }

measureWork :: FilePath -> IO WorkDigest
measureWork work = do
  present <- doesDirectoryExist work
  if not present
    then pure (WorkDigest 0 0 (hexText (SHA.finalize SHA.init)) [])
    else do
      relatives <- regularFiles work
      entries <- forM (sort relatives) $ \relative -> do
        let path = work </> relative
        status <- getSymbolicLinkStatus path
        digest <- SHA.hashlazy <$> BL.readFile path
        pure (relative, fromIntegral (fileSize status), hexText digest)
      let content =
            [ hex ++ "  " ++ relative ++ "\n"
            | (relative, _, hex) <- entries
            ]
          combined = SHA.finalize (foldl (\context line -> SHA.update context (TE.encodeUtf8 (T.pack line))) SHA.init content)
          files = [WorkFile relative bytes hex | (relative, bytes, hex) <- entries, bytes >= 1024 * 1024]
      pure
        ( WorkDigest
            (fromIntegral (length entries))
            (sum [bytes | (_, bytes, _) <- entries])
            (hexText combined)
            files
        )

regularFiles :: FilePath -> IO [FilePath]
regularFiles root = go ""
 where
  go relative = do
    let directory = if null relative then root else root </> relative
    names <- listDirectory directory
    fmap concat $ forM names $ \name -> do
      let childRelative = if null relative then name else relative </> name
      status <- getSymbolicLinkStatus (root </> childRelative)
      if isDirectory status && not (isSymbolicLink status)
        then go childRelative
        else pure [childRelative | isRegularFile status]

treeFileSize :: FilePath -> IO Integer
treeFileSize root = do
  present <- doesDirectoryExist root
  if not present
    then pure 0
    else do
      relatives <- regularFiles root
      sizes <- forM relatives $ \relative -> fromIntegral . fileSize <$> getSymbolicLinkStatus (root </> relative)
      pure (sum sizes)

hexText :: B.ByteString -> String
hexText = concatMap hexByte . B.unpack
 where
  hexByte byte = let value = showHex byte "" in replicate (2 - length value) '0' ++ value

keepInput :: Task -> FilePath -> FilePath -> IO (Either String FilePath)
keepInput task work path = do
  outcome <- try move :: IO (Either IOException (Either String FilePath))
  pure $ case outcome of
    Left problem -> Left (path ++ ": " ++ firstLine (show problem))
    Right result -> result
 where
  move = do
    workRoot <- canonicalizePath work
    absolute <- makeAbsolute path
    resolved <- canonicalizePath absolute
    if not (inside workRoot resolved)
      then pure (Left ("refusing a path outside " ++ workRoot ++ ": " ++ path))
      else do
        let relative = makeRelative workRoot resolved
            destination = taskInputs task </> relative
        exists <- entryExists destination
        if exists
          then pure (Left ("refusing to replace existing " ++ destination))
          else do
            createDirectoryIfMissing True (takeDirectory destination)
            directory <- doesDirectoryExist resolved
            if directory
              then renameDirectory resolved destination
              else renameFile resolved destination
            pure (Right destination)

inside :: FilePath -> FilePath -> Bool
inside root candidate =
  let relative = makeRelative root candidate
  in not (isAbsolute relative) && relative /= "." && ".." `notElem` splitDirectories relative

entryExists :: FilePath -> IO Bool
entryExists path =
  (getSymbolicLinkStatus path >> pure True) `catch` \problem ->
    if isDoesNotExistError problem then pure False else throwIO problem

describeRefusal :: Refusal -> String
describeRefusal refusal =
  intercalate
    "\n"
    ( [ "run-output: task directory " ++ refusalDirectory refusal ++ " is over XM1_TASK_OUTPUT_MAX="
          ++ show limit ++ ": " ++ show measured ++ " bytes allocated"
      ]
        ++ [ "run-output: largest child " ++ name ++ ": " ++ show bytes ++ " bytes"
           | (name, StorageBytes bytes) <- refusalChildren refusal
           ]
        ++ ["Keep verdicts and digests in result/, delete what no later run needs or move it with run-output keep, then retry."]
    )
 where
  StorageBytes measured = refusalMeasured refusal
  StorageBytes limit = refusalLimit refusal

runEnvironment :: RunPaths -> IO [(String, String)]
runEnvironment paths = do
  environment <- getEnvironment
  pure
    ( set "XM1_RUN_DIR" (runDirectory paths)
        (set "XM1_RUN_WORK" (runWork paths) (set "XM1_RUN_RESULT" (runResult paths) environment))
    )
 where
  set key value = ((key, value) :) . filter ((/= key) . fst)

awaitWithinWorkBudget :: FilePath -> StorageBytes -> Microseconds -> ProcessID -> ProcessHandle -> IO (ExitCode, Bool)
awaitWithinWorkBudget work limit (Microseconds interval) pid process = do
  finished <- newEmptyMVar
  exceeded <- newEmptyMVar
  _ <- forkIO $ do
    over <- monitor finished
    putMVar exceeded over
  status <- waitForProcess process
  putMVar finished ()
  over <- readMVar exceeded
  pure (status, over)
 where
  threshold = budgetBytes limit
  monitor :: MVar () -> IO Bool
  monitor finished = do
    slept <- timeout interval (readMVar finished)
    case slept of
      Just () -> pure False
      Nothing -> do
        bytes <- measureAllocated work
        if bytes > threshold
          then do
            signalQuietly sigTERM
            stopped <- timeout 10_000_000 (readMVar finished)
            case stopped of
              Just () -> pure ()
              Nothing -> signalQuietly sigKILL
            pure True
          else monitor finished
  signalQuietly signal = void (try (signalProcessGroup signal pid) :: IO (Either IOException ()))

utcStamp :: UTCTime -> String
utcStamp time = formatTime defaultTimeLocale "%Y%m%dT%H%M%S" time ++ "Z"

utcIso :: UTCTime -> String
utcIso time = formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%S" time ++ "Z"

firstLine :: String -> String
firstLine = takeWhile (/= '\n')
