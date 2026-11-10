{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (forkFinally, newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, bracket, catch, onException, throwIO)
import Control.Monad (forM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (FromJSON (parseJSON), ToJSON (toJSON), eitherDecodeStrict', encode, withObject, withText, (.:))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isDigit, isHexDigit, toLower)
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import qualified Data.Text as T
import GHC.Generics (Generic)
import Foreign.C.Types (CUInt (..), CULong (..))
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, findExecutable, makeAbsolute, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die)
import System.FilePath ((</>), isAbsolute, normalise, splitDirectories, takeDirectory, takeExtension)
import System.IO (Handle, IOMode (ReadMode), hClose, hFlush, stderr, hPutStrLn, withBinaryFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (FileStatus, deviceID, fileGroup, fileID, fileMode, fileOwner, fileSize, getFileStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, linkCount, modificationTimeHiRes, statusChangeTimeHiRes)
import System.Posix.IO (OpenFileFlags (exclusive, nofollow, cloexec, creat), OpenMode (WriteOnly), closeFd, defaultFileFlags, fdToHandle, openFd)
import System.Posix.User (getEffectiveUserID, setGroupID, setGroups, setUserID)
import System.Process (CreateProcess (std_in, std_out, std_err), StdStream (CreatePipe, NoStream), proc, waitForProcess, withCreateProcess)

newtype AbsolutePath = AbsolutePath FilePath deriving stock (Eq, Ord, Show, Generic) deriving anyclass ToJSON
newtype RelativePath = RelativePath FilePath deriving stock (Eq, Ord, Show, Generic) deriving anyclass ToJSON
newtype Digest = Digest T.Text deriving stock (Eq, Show, Generic) deriving anyclass ToJSON
newtype ByteCount = ByteCount Integer deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype Owner = Owner Integer deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype Timestamp = Timestamp Integer deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype DeviceId = DeviceId Integer deriving stock (Eq, Ord, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype InodeId = InodeId Integer deriving stock (Eq, Ord, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype LinkCount = LinkCount Integer deriving stock (Eq, Show, Generic) deriving anyclass (FromJSON, ToJSON)
newtype ProcessId = ProcessId Integer deriving (Eq, Ord, Show)
newtype DeviceCoordinates = DeviceCoordinates (CUInt, CUInt) deriving (Eq, Ord)
data Inspector = DefaultInspector | PinnedInspector AbsolutePath
data InspectionView = ProcessFiles | TaskFiles
data FileKind = FilesystemFile | KernelObject | UnavailableFile deriving Eq
data Descriptor = AccessibleDescriptor | MappedDescriptor | InaccessibleDescriptors deriving Eq
data OpenFile = OpenFile Descriptor (Maybe DeviceId) (Maybe InodeId) FileKind (Maybe AbsolutePath)
data BusyFiles = BusyFiles !(Set.Set (DeviceId, InodeId)) !(Set.Set InodeId)
data Reason = SupersededSuccessDiagnostic | ReproducibleAttemptState | ObsoleteReuseCache deriving (Eq, Show)

foreign import ccall unsafe "gnu_dev_major" c_deviceMajor :: CULong -> IO CUInt
foreign import ccall unsafe "gnu_dev_minor" c_deviceMinor :: CULong -> IO CUInt

nonCandidateFuseMounts :: Set.Set DeviceId -> IO [FilePath]
nonCandidateFuseMounts devices = do
  coordinates <- Set.fromList <$> mapM coordinate (Set.toList devices)
  rows <- BC.lines <$> B.readFile "/proc/self/mountinfo"
  fmap concat $ forM rows $ \row -> case BC.words row of
    _:_:deviceText:_:mount:remaining -> case dropWhile (/= "-") remaining of
      "-":filesystem:_
        | "fuse." `B.isPrefixOf` filesystem && not (B.elem 92 mount) ->
            case BC.split ':' deviceText of
              [major, minor] -> case (reads (BC.unpack major), reads (BC.unpack minor)) of
                ([(a, "")], [(b, "")]) -> pure [BC.unpack mount | Set.notMember (DeviceCoordinates (a, b)) coordinates]
                _ -> ioError (userError "invalid mounted device identity")
              _ -> ioError (userError "invalid mounted device identity")
      _ -> pure []
    _ -> ioError (userError "invalid mountinfo record")
 where
  coordinate (DeviceId value) = DeviceCoordinates <$> ((,) <$> c_deviceMajor (fromInteger value) <*> c_deviceMinor (fromInteger value))

ensure :: Bool -> String -> IO ()
ensure ok message = unless ok (ioError (userError message))

instance FromJSON AbsolutePath where
  parseJSON = withText "absolute path" $ \text -> do
    let path = T.unpack text
    unless (isAbsolute path && normalise path == path && '\0' `notElem` path && ".." `notElem` splitDirectories path) (fail "expected normalized absolute path")
    pure (AbsolutePath path)
instance FromJSON RelativePath where
  parseJSON = withText "relative file" $ \text -> do
    let path = T.unpack text
    unless (not (null path) && not (isAbsolute path) && normalise path == path && '\0' `notElem` path && all (`notElem` [".", ".."]) (splitDirectories path)) (fail "expected normalized relative file path")
    pure (RelativePath path)
instance FromJSON Digest where
  parseJSON = withText "sha256" $ \text -> do
    unless (T.length text == 64 && T.all isHexDigit text && T.toLower text == text) (fail "invalid SHA-256")
    pure (Digest text)
instance FromJSON Reason where
  parseJSON = withText "compaction reason" $ \text -> case text of
    "superseded-success-diagnostic" -> pure SupersededSuccessDiagnostic
    "reproducible-attempt-state" -> pure ReproducibleAttemptState
    "obsolete-reuse-cache" -> pure ObsoleteReuseCache
    _ -> fail "unknown compaction reason"
instance ToJSON Reason where
  toJSON reason = toJSON (case reason of
    SupersededSuccessDiagnostic -> "superseded-success-diagnostic"
    ReproducibleAttemptState -> "reproducible-attempt-state"
    ObsoleteReuseCache -> "obsolete-reuse-cache" :: T.Text)

data Candidate = Candidate RelativePath Reason T.Text deriving (Generic, ToJSON)
instance FromJSON Candidate where
  parseJSON = withObject "candidate" $ \o -> Candidate <$> o .: "path" <*> o .: "reason" <*> o .: "basis"
data SelectionRoot = SelectionRoot AbsolutePath [Candidate]
instance FromJSON SelectionRoot where
  parseJSON = withObject "selection root" $ \o -> SelectionRoot <$> o .: "path" <*> o .: "files"
data Selection = Selection Int [SelectionRoot] [AbsolutePath] [T.Text]
instance FromJSON Selection where
  parseJSON = withObject "selection" $ \o -> Selection <$> o .: "schema" <*> o .: "roots" <*> o .: "protected" <*> o .: "limitations"

data Identity = Identity
  { device :: DeviceId, inode :: InodeId, owner :: Owner, bytes :: ByteCount
  , links :: LinkCount, modified :: Timestamp, changed :: Timestamp
  } deriving (Eq, Show, Generic, FromJSON, ToJSON)
data Ancestor = Ancestor AbsolutePath DeviceId InodeId Owner deriving (Eq, Show, Generic, FromJSON, ToJSON)
data BoundFile = BoundFile
  { filePath :: AbsolutePath, fileIdentity :: Identity, fileDigest :: Digest
  , fileReason :: Reason, fileBasis :: T.Text
  } deriving (Generic, FromJSON, ToJSON)
data Protection = ProtectedFile AbsolutePath Identity Digest | ProtectedDirectory Ancestor deriving (Generic, FromJSON, ToJSON)
data Plan = Plan
  { planSchema :: Int, workspace :: AbsolutePath, selectionDigest :: Digest
  , plannedFiles :: [BoundFile], directories :: [Ancestor], protections :: [Protection], limitations :: [T.Text]
  } deriving (Generic, FromJSON, ToJSON)
data SkipReason = Missing | Changed | InUse deriving (Generic, ToJSON)
data Event = Started Digest | Removed AbsolutePath | Skipped AbsolutePath SkipReason | Finished Integer Integer | Interrupted deriving (Generic, ToJSON)

absolute :: AbsolutePath -> FilePath
absolute (AbsolutePath path) = path
inside :: FilePath -> FilePath -> Bool
inside parent child = splitDirectories parent `isPrefixOf` splitDirectories child

identity :: FileStatus -> Identity
identity status = Identity (DeviceId (fromIntegral (deviceID status))) (InodeId (fromIntegral (fileID status))) (Owner (fromIntegral (fileOwner status)))
  (ByteCount (fromIntegral (fileSize status))) (LinkCount (fromIntegral (linkCount status)))
  (Timestamp (floor (toRational (modificationTimeHiRes status) * 1000000000)))
  (Timestamp (floor (toRational (statusChangeTimeHiRes status) * 1000000000)))
ancestor :: AbsolutePath -> FileStatus -> Ancestor
ancestor path status = Ancestor path (DeviceId (fromIntegral (deviceID status))) (InodeId (fromIntegral (fileID status))) (Owner (fromIntegral (fileOwner status)))

hashFile :: FilePath -> IO Digest
hashFile path = withBinaryFile path ReadMode (go SHA.init)
 where
  go !context handle = do
    chunk <- B.hGetSome handle (1024 * 1024)
    if B.null chunk then pure (Digest (T.pack (concatMap hex (B.unpack (SHA.finalize context)))))
      else go (SHA.update context chunk) handle
  hex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text


newOutput :: FilePath -> (Handle -> IO a) -> IO a
newOutput path action = bracket acquire hClose action
 where
  acquire = do
    fd <- openFd path WriteOnly defaultFileFlags { creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True }
    fdToHandle fd `onException` closeFd fd

writeNewJson :: ToJSON a => FilePath -> a -> IO ()
writeNewJson path value = newOutput path $ \handle -> BL.hPut handle (encode value <> "\n") >> hFlush handle

canonical :: AbsolutePath -> IO ()
canonical path = do
  resolved <- canonicalizePath (absolute path)
  ensure (resolved == absolute path) ("path traverses a symlink: " ++ absolute path)

parents :: FilePath -> [FilePath]
parents path = path : if path == "/" then [] else parents (takeDirectory path)

bindAncestor :: FilePath -> IO Ancestor
bindAncestor path = do
  status <- getSymbolicLinkStatus path
  ensure (isDirectory status && not (isSymbolicLink status)) ("non-directory ancestor: " ++ path)
  pure (ancestor (AbsolutePath path) status)

validateWorkspace :: AbsolutePath -> IO FilePath
validateWorkspace root = do
  canonical root
  let scratch = absolute root </> "private-data/scratch"
  canonical (AbsolutePath scratch)
  status <- getSymbolicLinkStatus scratch
  ensure (isDirectory status) "scratch directory is missing"
  pure scratch

validateTarget :: FilePath -> AbsolutePath -> IO ()
validateTarget scratch path = do
  let value = absolute path
      components = map (map toLower) (splitDirectories value)
  ensure (inside scratch value && value /= scratch && not (inside (scratch </> "reclaim") value)) "candidate must be below a named scratch subtree, outside reclaim"
  ensure (not (any (`elem` ["analysis", "firmware", "provenance", "sessions", "photo-sessions", "read-plans", ".git"]) components)) "protected artifact category"
  ensure (map toLower (takeExtension value) `notElem` [".dat", ".nand", ".img", ".dump"]) "firmware, media and dump images are protected"

bindFile :: AbsolutePath -> Reason -> T.Text -> IO BoundFile
bindFile path reason basis = do
  ensure (not (T.null (T.strip basis))) "candidate needs a semantic retirement basis"
  canonical path
  before <- getSymbolicLinkStatus (absolute path)
  caller <- getEffectiveUserID
  ensure (isRegularFile before && linkCount before == 1 && fileOwner before == caller) "candidate must be a single-link caller-owned regular file"
  digest <- hashFile (absolute path)
  after <- getSymbolicLinkStatus (absolute path)
  ensure (isRegularFile after && linkCount after == 1 && identity before == identity after) "candidate changed while hashing"
  pure (BoundFile path (identity before) digest reason basis)

bindProtection :: AbsolutePath -> IO Protection
bindProtection path = do
  canonical path
  before <- getSymbolicLinkStatus (absolute path)
  if isDirectory before then pure (ProtectedDirectory (ancestor path before)) else do
    ensure (isRegularFile before) "protected path must be a regular file or directory"
    digest <- hashFile (absolute path)
    after <- getSymbolicLinkStatus (absolute path)
    ensure (identity before == identity after) "protected file changed while hashing"
    pure (ProtectedFile path (identity before) digest)

protectionPath :: Protection -> AbsolutePath
protectionPath (ProtectedFile path _ _) = path
protectionPath (ProtectedDirectory (Ancestor path _ _ _)) = path

verifyAncestor :: Ancestor -> IO ()
verifyAncestor expected@(Ancestor path _ _ _) = do
  status <- getSymbolicLinkStatus (absolute path)
  ensure (isDirectory status && ancestor path status == expected) "directory identity changed"

verifyProtection :: Protection -> IO ()
verifyProtection (ProtectedDirectory value) = verifyAncestor value
verifyProtection (ProtectedFile path expected digest) = do
  canonical path
  status <- getSymbolicLinkStatus (absolute path)
  ensure (isRegularFile status && identity status == expected) "protected file changed"
  actual <- hashFile (absolute path)
  ensure (actual == digest) "protected file digest changed"

verifyFile :: Map.Map AbsolutePath Ancestor -> BoundFile -> IO Bool
verifyFile bound file = (do
  forM_ (parents (takeDirectory (absolute (filePath file)))) $ \path -> do
    expected <- maybe (ioError (userError "unbound ancestor")) pure (Map.lookup (AbsolutePath path) bound)
    verifyAncestor expected
  status <- getSymbolicLinkStatus (absolute (filePath file))
  pure (isRegularFile status && linkCount status == 1 && identity status == fileIdentity file)) `catch` \(_ :: IOException) -> pure False

plan :: FilePath -> FilePath -> FilePath -> IO ()
plan workspacePath selectionPath output = do
  root <- readPath workspacePath
  scratch <- validateWorkspace root
  selectedBytes <- B.readFile selectionPath
  Selection schema roots protected notes <- either (ioError . userError) pure (eitherDecodeStrict' selectedBytes)
  ensure (schema == 1 && not (null roots)) "unsupported or empty selection"
  selections <- fmap concat $ forM roots $ \(SelectionRoot selectedRoot candidates) -> do
    canonical selectedRoot
    validateTarget scratch selectedRoot
    forM candidates $ \(Candidate (RelativePath relative) reason basis) -> do
      let path = AbsolutePath (absolute selectedRoot </> relative)
      validateTarget scratch path
      pure (path, reason, basis)
  let paths = [path | (path, _, _) <- selections]
  ensure (not (null paths) && Set.size (Set.fromList paths) == length paths) "empty or duplicate candidate paths"
  forM_ paths $ \path -> ensure (not (any (\keep -> inside (absolute keep) (absolute path)) protected)) "candidate overlaps protected path"
  hPutStrLn stderr ("Binding " ++ show (length paths) ++ " selected files")
  ancestors <- mapM bindAncestor (Set.toList (Set.fromList (concatMap (parents . takeDirectory . absolute) paths)))
  files <- forM selections $ \(path, reason, basis) -> bindFile path reason basis
  guards <- mapM bindProtection protected
  mapM_ verifyAncestor ancestors
  let digest = digestBytes selectedBytes
  writeNewJson output (Plan 1 root digest files ancestors guards notes)
  actual <- hashFile output
  let total = sum [n | BoundFile _ ident _ _ _ <- files, let ByteCount n = bytes ident]
  putStrLn (show (length files) ++ " selected files, " ++ show total ++ " apparent bytes; plan SHA-256 " ++ digestText actual)

readPath :: FilePath -> IO AbsolutePath
readPath input = do
  value <- makeAbsolute input
  ensure (normalise value == value && '\0' `notElem` value) "expected normalized path"
  pure (AbsolutePath value)

digestText :: Digest -> String
digestText (Digest value) = T.unpack value

digestBytes :: B.ByteString -> Digest
digestBytes value = Digest (T.pack (concatMap hex (B.unpack (SHA.hash value))))
 where
  hex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

resolveInspector :: Inspector -> IO FilePath
resolveInspector choice = do
  caller <- getEffectiveUserID
  input <- case choice of
    DefaultInspector -> do
      ensure (caller /= 0) "root inspection requires --inspector with an immutable Nix executable"
      findExecutable "lsof" >>= maybe (ioError (userError "lsof is missing from the environment")) pure
    PinnedInspector path -> pure (absolute path)
  path <- canonicalizePath input
  if caller == 0 then do
    ensure (inside "/nix/store" path) "privileged inspector must be in the Nix store"
    status <- getSymbolicLinkStatus path
    ensure (isRegularFile status && fileOwner status == 0 && fileMode status .&. 0o022 == 0) "privileged inspector is not immutable root-owned code"
  else pure ()
  pure path

readInspection :: FilePath -> [String] -> IO (ExitCode, B.ByteString, B.ByteString)
readInspection executable arguments =
  withCreateProcess (proc executable arguments) {std_in = NoStream, std_out = CreatePipe, std_err = CreatePipe} $ \_ output errors process ->
    case (output, errors) of
      (Just outputHandle, Just errorHandle) -> do
        receiver <- newEmptyMVar
        _ <- forkFinally (B.hGetContents errorHandle) (putMVar receiver)
        contents <- B.hGetContents outputHandle
        warnings <- takeMVar receiver >>= either throwIO pure
        status <- waitForProcess process
        pure (status, contents, warnings)
      _ -> ioError (userError "lsof inspection pipes unavailable")

referencesFile :: BusyFiles -> Identity -> Bool
referencesFile (BusyFiles exact mapped) ident =
  Set.member (device ident, inode ident) exact || Set.member (inode ident) mapped

busyFiles :: FilePath -> Owner -> Set.Set DeviceId -> IO BusyFiles
busyFiles inspector (Owner dataOwner) devices = do
  caller <- getEffectiveUserID
  exemptions <- nonCandidateFuseMounts devices
  forM_ exemptions $ \path -> hPutStrLn stderr ("Inspecting processes without statting noncandidate FUSE mount " ++ path)
  let ownerFilter = if caller == 0 then [] else ["-a", "-u", show dataOwner]
  let filesystemOptions = concatMap (\path -> ["-e", path]) exemptions
  let scan view (busy, mapped, unavailable, probes) = do
        let taskOptions = case view of ProcessFiles -> []; TaskFiles -> ["-K"]
        (status, output, warnings) <- readInspection inspector (["+w", "-nP"] ++ taskOptions ++ filesystemOptions ++ ownerFilter ++ ["-F0pfiDtn"])
        ensure (status `elem` [ExitSuccess, ExitFailure 1] && B.null warnings) ("lsof inspection incomplete; no deletion permitted (" ++ show status ++ "): " ++ BC.unpack (B.take 2048 warnings))
        either (ioError . userError) pure (go Nothing Nothing busy mapped unavailable probes (map (BC.dropWhile (== '\n')) (BC.split '\0' output)))
  -- Task-only lsof output omits single-threaded processes.
  processes <- scan ProcessFiles (Set.empty, Set.empty, Set.empty, Set.empty)
  (busy, mapped, unavailable, probes) <- scan TaskFiles processes
  reconciled <- forM (Set.toList probes) $ \path ->
    (do
      statusNow <- getSymbolicLinkStatus (takeDirectory (absolute path))
      ensure (isDirectory statusNow) "descriptor table is not a directory"
      statusNowFile <- getFileStatus (absolute path)
      pure (Just (DeviceId (fromIntegral (deviceID statusNowFile)), InodeId (fromIntegral (fileID statusNowFile)))))
    `catch` \(problem :: IOException) -> if isDoesNotExistError problem then pure Nothing else ioError problem
  forM_ (Set.toList unavailable) $ \(ProcessId pid) ->
    (do
      statusNow <- B.readFile ("/proc/" ++ show pid ++ "/status")
      let kernelThread = any ((== ["Kthread:", "1"]) . BC.words) (BC.lines statusNow)
      ensure kernelThread ("lsof could not inspect live userspace process " ++ show pid ++ "; obtain privileged inspection"))
    `catch` \(problem :: IOException) -> if isDoesNotExistError problem then pure () else ioError problem
  pure (BusyFiles (Set.union busy (Set.fromList [value | Just value <- reconciled])) mapped)
 where
  finish _ Nothing busy mapped unavailable probes = Right (busy, mapped, unavailable, probes)
  finish pid (Just (OpenFile descriptor dev ident kind probe)) busy mapped unavailable probes
    | descriptor == InaccessibleDescriptors = unknown pid busy mapped unavailable probes
    | descriptor == MappedDescriptor, Just i <- ident = Right (busy, Set.insert i mapped, unavailable, probes)
    | Just d <- dev, Just i <- ident = Right (Set.insert (d, i) busy, mapped, unavailable, probes)
    | kind == KernelObject = Right (busy, mapped, unavailable, probes)
    | Just path <- probe = Right (busy, mapped, unavailable, Set.insert path probes)
    | otherwise = unknown pid busy mapped unavailable probes
  unknown Nothing _ _ _ _ = Left "lsof file record has no process identity"
  unknown (Just pid) busy mapped unavailable probes = Right (busy, mapped, Set.insert pid unavailable, probes)
  go pid current !busy !mapped !unavailable !probes [] = finish pid current busy mapped unavailable probes
  go pid current !busy !mapped !unavailable !probes (field:rest) = case BC.uncons field of
    Just ('p', value) -> do
      (nextBusy, nextMapped, nextUnavailable, nextProbes) <- finish pid current busy mapped unavailable probes
      process <- case BC.readInteger value of
        Just (number, remaining) | number > 0 && B.null remaining -> Right (ProcessId number)
        _ -> Left "invalid lsof process identity"
      go (Just process) Nothing nextBusy nextMapped nextUnavailable nextProbes rest
    Just ('f', value) -> do
      (nextBusy, nextMapped, nextUnavailable, nextProbes) <- finish pid current busy mapped unavailable probes
      let descriptor = case value of
            "NOFD" -> InaccessibleDescriptors
            "mem" -> MappedDescriptor
            "DEL" -> MappedDescriptor
            "txt" -> MappedDescriptor
            _ -> AccessibleDescriptor
      go pid (Just (OpenFile descriptor Nothing Nothing UnavailableFile Nothing)) nextBusy nextMapped nextUnavailable nextProbes rest
    Just ('D', value) -> do
      number <- maybe (Left "invalid lsof device identity") Right (BC.stripPrefix "0x" value >>= parseHex)
      update (\(OpenFile descriptor _ ident kind probe) -> OpenFile descriptor (Just (DeviceId number)) ident kind probe)
    Just ('i', value) -> case BC.readInteger value of
      Just (number, remaining) | B.null remaining -> update (\(OpenFile descriptor dev _ kind probe) -> OpenFile descriptor dev (Just (InodeId number)) kind probe)
      _ -> Left "invalid lsof inode identity"
    Just ('t', value) -> update (\(OpenFile descriptor dev ident _ probe) -> OpenFile descriptor dev ident (fileKind value) probe)
    Just ('n', value) -> update (\record@(OpenFile descriptor dev ident kind _) -> case (dev, ident, kind) of
      (Just _, Just _, _) -> record
      (_, _, KernelObject) -> record
      _ -> OpenFile descriptor dev ident kind (descriptorPath pid value))
    _ -> go pid current busy mapped unavailable probes rest
   where
    update change = case current of
      Nothing -> Left "lsof metadata has no file record"
      Just record -> go pid (Just (change record)) busy mapped unavailable probes rest
  descriptorPath Nothing _ = Nothing
  descriptorPath (Just (ProcessId pid)) value =
    let (pathBytes, suffix) = BC.break (== ' ') value
        path = BC.unpack pathBytes
        decimal text = not (null text) && all isDigit text
        valid = case splitDirectories path of
          ["/", "proc", process, "fd", descriptor] -> process == show pid && decimal descriptor
          ["/", "proc", process, "task", task, "fd", descriptor] -> process == show pid && decimal task && decimal descriptor
          _ -> False
    in if valid && " (readlink:" `BC.isPrefixOf` suffix then Just (AbsolutePath path) else Nothing
  parseHex value = case readHex (BC.unpack value) of [(number, "")] -> Just number; _ -> Nothing
  fileKind value = case value of
    "REG" -> FilesystemFile
    "DIR" -> FilesystemFile
    "DEL" -> FilesystemFile
    "LINK" -> FilesystemFile
    "unix" -> KernelObject
    "IPv4" -> KernelObject
    "IPv6" -> KernelObject
    "raw4" -> KernelObject
    "raw6" -> KernelObject
    "pack" -> KernelObject
    "sock" -> KernelObject
    "netlink" -> KernelObject
    "a_inode" -> KernelObject
    "FIFO" -> KernelObject
    "CHR" -> KernelObject
    "BLK" -> KernelObject
    _ -> UnavailableFile

emit :: Handle -> Event -> IO ()
emit handle event = BL.hPut handle (encode event <> "\n")

apply :: FilePath -> Digest -> Inspector -> IO ()
apply planPath approved inspectorChoice = do
  caller <- getEffectiveUserID
  inspector <- resolveInspector inspectorChoice
  encoded <- B.readFile planPath
  ensure (digestBytes encoded == approved) "approved plan digest differs"
  document <- either (ioError . userError) pure (eitherDecodeStrict' encoded)
  ensure (planSchema document == 1) "unsupported plan schema"
  scratch <- validateWorkspace (workspace document)
  workspaceStatus <- getSymbolicLinkStatus (absolute (workspace document))
  let dataOwner = Owner (fromIntegral (fileOwner workspaceStatus))
  ensure (dataOwner /= Owner 0 && (caller == 0 || dataOwner == Owner (fromIntegral caller))) "workspace must belong to the invoking non-root data owner"
  let files = plannedFiles document
      paths = map filePath files
      bound = Map.fromList [(path, value) | value@(Ancestor path _ _ _) <- directories document]
  ensure (not (null files) && Set.size (Set.fromList paths) == length paths) "empty or duplicate planned files"
  forM_ files $ \file -> do
    validateTarget scratch (filePath file)
    ensure (owner (fileIdentity file) == dataOwner && links (fileIdentity file) == LinkCount 1) "candidate is not a single-link data-owner file"
    ensure (not (any (\keep -> inside (absolute (protectionPath keep)) (absolute (filePath file))) (protections document))) "planned file overlaps protection"
  forM_ (protections document) $ \keep -> ensure (inside (absolute (workspace document)) (absolute (protectionPath keep))) "protected path is outside workspace"
  mapM_ verifyProtection (protections document)
  let selectedDevices = Set.fromList (map (device . fileIdentity) files)
  beforeBusy <- busyFiles inspector dataOwner selectedDevices
  hPutStrLn stderr "Revalidating selected contents; changed and in-use files will be skipped"
  checked <- forM files $ \file -> do
    same <- verifyFile bound file
    if not same || referencesFile beforeBusy (fileIdentity file) then pure (file, False) else do
      digest <- hashFile (absolute (filePath file))
      pure (file, digest == fileDigest file)
  afterBusy <- busyFiles inspector dataOwner selectedDevices
  if caller == 0 then do
    setGroups []
    setGroupID (fileGroup workspaceStatus)
    setUserID (fileOwner workspaceStatus)
  else pure ()
  removed <- newIORef (0 :: Integer)
  skipped <- newIORef (0 :: Integer)
  auditPath <- makeAbsolute (planPath ++ ".audit.jsonl")
  ensure (all (\file -> absolute (filePath file) /= auditPath) files) "audit collides with selection"
  newOutput auditPath $ \audit -> do
    emit audit (Started approved)
    hFlush audit
    (do
      forM_ checked $ \(file, contentsMatch) -> do
        let path = filePath file
            ident = fileIdentity file
            inUse = referencesFile beforeBusy ident || referencesFile afterBusy ident
        same <- verifyFile bound file
        if inUse || not contentsMatch || not same then do
          present <- (getSymbolicLinkStatus (absolute path) >> pure True) `catch` \(problem :: IOException) -> if isDoesNotExistError problem then pure False else ioError problem
          emit audit (Skipped path (if inUse then InUse else if present then Changed else Missing))
          modifyIORef' skipped (+1)
        else do
          removeFile (absolute path)
          emit audit (Removed path)
          modifyIORef' removed (+1)
      mapM_ verifyProtection (protections document)
      count <- readIORef removed
      omissions <- readIORef skipped
      emit audit (Finished count omissions)
      hFlush audit
      putStrLn (show count ++ " removed; " ++ show omissions ++ " skipped; protected paths verified; audit " ++ auditPath))
      `onException` (emit audit Interrupted >> hFlush audit)

main :: IO ()
main = (do
  arguments <- getArgs
  case arguments of
    ["plan", "--workspace", root, "--selection", selection, "--out", output] -> plan root selection output
    ["apply", "--plan", path, "--sha256", digest] -> do
      approved <- either (ioError . userError) pure (eitherDecodeStrict' (BL.toStrict (encode digest)))
      apply path approved DefaultInspector
    ["apply", "--plan", path, "--sha256", digest, "--inspector", executable] -> do
      approved <- either (ioError . userError) pure (eitherDecodeStrict' (BL.toStrict (encode digest)))
      pinned <- readPath executable
      apply path approved (PinnedInspector pinned)
    _ -> die "usage: research-compaction plan --workspace ROOT --selection FILE --out PLAN | apply --plan PLAN --sha256 SHA256 [--inspector NIX_LSOF]")
  `catch` \(problem :: IOException) -> die (show problem)
