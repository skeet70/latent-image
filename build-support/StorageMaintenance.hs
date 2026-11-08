{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Applicative ((<|>))
import Control.Exception (IOException, bracket, catch, onException)
import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (FromJSON (parseJSON), ToJSON (toJSON), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, isDigit, isHexDigit)
import Data.List (isPrefixOf, nub, sort, sortOn, stripPrefix)
import qualified Data.Map.Strict as Map
import Data.Maybe (catMaybes)
import qualified Data.Set as Set
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.Time.Clock.POSIX (getPOSIXTime)
import Foreign.C.Types (CInt (..))
import Foreign.C.Error (eAGAIN, eWOULDBLOCK, getErrno)
import GHC.Generics (Generic)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesPathExist, listDirectory, makeAbsolute, removeFile, renameFile)
import qualified GHC.Foreign as GHC
import GHC.IO.Encoding (getFileSystemEncoding)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die)
import System.FilePath ((</>), isAbsolute, makeRelative, normalise, splitDirectories, takeDirectory, takeExtension, takeFileName)
import System.IO (IOMode (ReadMode), hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (deviceID, fileID, fileGroup, fileOwner, fileSize, getFileStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, linkCount, modificationTimeHiRes, readSymbolicLink, setFileMode)
import System.Posix.IO (OpenFileFlags (cloexec, nofollow), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (getProcessID)
import System.Posix.Types (Fd (..))
import System.Posix.User (getEffectiveGroupID, getEffectiveUserID, getGroups, setEffectiveGroupID, setEffectiveUserID, setGroups)
import System.Process (readProcessWithExitCode)

newtype Root = Root FilePath deriving stock (Eq, Ord, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype RelativePath = RelativePath T.Text deriving stock (Eq, Ord, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype Bytes = Bytes Integer deriving stock (Eq, Ord, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype FileTimeNanoseconds = FileTimeNanoseconds Integer deriving stock (Eq, Ord, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype AgeSeconds = AgeSeconds Integer deriving stock (Eq, Ord, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype Digest = Digest T.Text deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype ProcessId = ProcessId String deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype OwnerUID = OwnerUID Integer deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
data InspectionIssue = InspectionIssue { inspectionPid :: ProcessId, inspectionFailure :: String } deriving (Generic, ToJSON)
data TargetUse = TargetContext | FileReference RelativePath
data Activity = Activity Root [(ProcessId, [TargetUse])] [InspectionIssue]
data ProcessScope = OwnerProcesses | AllProcesses deriving Eq
data Disposition = Retain | CompilerArtifact | FingerprintSuccess deriving (Eq, Show, Generic, ToJSON, FromJSON)
data EntryType = RegularFile | Directory | SymbolicLink deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Entry = Entry { path :: !RelativePath, device :: !Integer, inode :: !Integer, ownerUID :: !OwnerUID, size :: !Bytes, digest :: !(Maybe Digest), disposition :: !Disposition, entryType :: !EntryType, mtimeNanoseconds :: !FileTimeNanoseconds } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Binding = Binding { root :: Root, entries :: [Entry] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data CoherentUnit = CoherentUnit { unitRoot :: Root, unitFingerprint :: RelativePath, unitArtifacts :: [RelativePath], unitControl :: RelativePath } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data RetentionContext = RetentionContext { authorizedPolicySha256 :: Digest, authorizedRootPolicy :: RetentionRootPolicy, authorizedArtifactClasses :: [RetentionClass] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Plan = Plan { schemaVersion :: Int, bindings :: [Binding], coherentUnits :: [CoherentUnit], retentionAuthorization :: Maybe RetentionContext } deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype FileCount = FileCount Integer deriving stock (Generic) deriving anyclass (ToJSON)
data RootSummary = RootSummary { canonicalCargoTarget :: Root, recognizedFileCount :: FileCount, recognizedBytes :: Bytes, retainedFileCount :: FileCount, retainedBytes :: Bytes } deriving (Generic, ToJSON)
data Inventory = Inventory { rootSummaries :: [RootSummary], availableBytes :: [(Root, Bytes)], activeUsers :: [(Root, [ProcessId])], uninspectableUsers :: [(Root, [InspectionIssue])] } deriving (Generic, ToJSON)
data ScanMode = MetadataOnly | HashContents | RetentionMetadata deriving Eq
data AuditPhase = Applying | Complete | Interrupted deriving (Generic, ToJSON)
data SkipReason = Missing | Changed | InUse | OwnerUnavailable | NoSelectedArtifact | UnitUnavailable deriving (Generic, ToJSON)
data Skipped = Skipped Root RelativePath SkipReason deriving (Generic, ToJSON)
data Audit = Audit { auditSchemaVersion :: Int, auditPhase :: AuditPhase, appliedPlan :: Plan, removed :: [(Root, RelativePath)], skipped :: [Skipped], beforeAvailable :: [(Root, Bytes)], afterAvailable :: [(Root, Bytes)], actualAvailableDelta :: [(Root, Bytes)], deletionScopeVerified :: Bool } deriving (Generic, ToJSON)

newtype CrateName = CrateName String deriving stock (Eq, Ord)
newtype UnitHash = UnitHash String
data CargoUnit = LibraryUnit CrateName | BinaryUnit CrateName | TestUnit CrateName | BuildScriptUnit
data ValidatedFingerprint = ValidatedFingerprint RelativePath CargoUnit UnitHash

data CargoFingerprint = CargoFingerprint Integer String Integer Integer Integer [(Integer, String, Bool, Integer)]
instance FromJSON CargoFingerprint where
  parseJSON = withObject "Cargo fingerprint" $ \o -> CargoFingerprint <$> o .: "rustc" <*> o .: "features" <*> o .: "target" <*> o .: "profile" <*> o .: "path" <*> o .: "deps"

newtype RustMetadata = RustMetadata Integer
instance FromJSON RustMetadata where
  parseJSON = withObject "rustc metadata" $ \o -> RustMetadata <$> o .: "rustc_fingerprint"

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

ensure :: Bool -> String -> IO ()
ensure ok message = unless ok (ioError (userError message))

relativePath :: FilePath -> RelativePath
relativePath = RelativePath . T.pack

relativeFilePath :: RelativePath -> FilePath
relativeFilePath (RelativePath text) = T.unpack text

hashFile :: Bytes -> FilePath -> IO Digest
hashFile (Bytes inputBytes) file = withBinaryFile file ReadMode (loop SHA.init)
 where
  chunkBytes = fromInteger (max 1 (min inputBytes (1024 * 1024)))
  loop !context handle = do
    chunk <- B.hGetSome handle chunkBytes
    if B.null chunk then do
      let !value = T.pack (concatMap hex (B.unpack (SHA.finalize context)))
      pure (Digest value)
    else loop (SHA.update context chunk) handle
  hex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

inside :: FilePath -> FilePath -> Bool
inside parent child = splitDirectories parent `isPrefixOf` splitDirectories child

safeRoot :: FilePath -> IO Root
safeRoot input = do
  absolute <- makeAbsolute input
  canonical <- canonicalizePath absolute
  ensure (canonical == absolute && canonical /= "/") ("root must be an absolute, canonical, non-symlink directory; canonical input: " ++ canonical)
  ensure (not (any (`elem` ["private-data", ".git"]) (splitDirectories canonical)) && not (inside "/nix/store" canonical)) "protected root"
  let ancestors p = p : if p == "/" then [] else ancestors (takeDirectory p)
  forM_ (ancestors canonical) $ \p -> do
    status <- getSymbolicLinkStatus p
    ensure (isDirectory status && not (isSymbolicLink status)) "root traverses a symlink or non-directory"
  tag <- doesPathExist (canonical </> "CACHEDIR.TAG")
  rust <- doesPathExist (canonical </> ".rustc_info.json")
  ensure (tag || rust) "missing Cargo provenance marker"
  when tag $ do
    status <- getSymbolicLinkStatus (canonical </> "CACHEDIR.TAG")
    ensure (isRegularFile status) "invalid cache marker"
    prefix <- withBinaryFile (canonical </> "CACHEDIR.TAG") ReadMode (`B.hGet` 43)
    ensure (BC.pack "Signature: 8a477f597d28d172789f06886806bc55" `B.isPrefixOf` prefix) "invalid cache marker signature"
  when rust $ do
    status <- getSymbolicLinkStatus (canonical </> ".rustc_info.json")
    ensure (isRegularFile status) "invalid rustc metadata"
    bytes <- B.readFile (canonical </> ".rustc_info.json")
    ensure (case eitherDecodeStrict' bytes :: Either String RustMetadata of Right _ -> True; Left _ -> False) "invalid rustc metadata"
  pure (Root canonical)

walk :: ScanMode -> FilePath -> IO [RelativePath]
walk mode base = go ""
 where
  go relative = visit relative `catch` \(err :: IOException) ->
    if not (null relative) && isDoesNotExistError err then pure [] else ioError err
  visit relative = do
    ensure (all (\c -> c < '\xD800' || c > '\xDFFF') relative) "Cargo path cannot be represented losslessly in a JSON plan"
    let !packed = relativePath relative
    status <- getSymbolicLinkStatus (base </> relative)
    ensure (mode == RetentionMetadata || not (isSymbolicLink status)) ("symlinks in Cargo roots are not admitted: " ++ (base </> relative))
    if isSymbolicLink status then pure [packed]
    else if isDirectory status && any (`elem` ["source", ".git", "private-data"]) (splitDirectories relative) then pure [packed]
    else if isDirectory status then do
      children <- sort <$> listDirectory (base </> relative)
      descendants <- concat <$> mapM (go . (relative </>)) children
      pure (packed : descendants)
    else do
      ensure (isRegularFile status) "special file in Cargo root"
      pure [packed]

parseCargoUnit :: FilePath -> Maybe CargoUnit
parseCargoUnit marker
  | marker == "build-script-build-script-build" = Just BuildScriptUnit
  | otherwise =
      named "test-integration-test-" TestUnit
      <|> named "test-bin-" TestUnit
      <|> named "test-lib-" TestUnit
      <|> named "lib-" LibraryUnit
      <|> named "bin-" BinaryUnit
 where
  named prefix constructor = do
    name <- stripPrefix prefix marker
    if not (null name) && all (\c -> isAlphaNum c || c == '_' || c == '-') name
      then Just (constructor (CrateName [if c == '-' then '_' else c | c <- name]))
      else Nothing

validFingerprints :: Root -> [RelativePath] -> IO [ValidatedFingerprint]
validFingerprints (Root base) files = fmap catMaybes $ forM files $ \relative ->
  let p = relativeFilePath relative
  in (if any (`elem` ["private-data", ".git"]) (splitDirectories p) then pure Nothing else
    case parseCargoUnit (takeFileName p) of
      Nothing -> pure Nothing
      Just unit -> valid relative unit) `catch` \(err :: IOException) ->
        if isDoesNotExistError err then pure Nothing else ioError err
 where
  valid relative unit = do
    let p = relativeFilePath relative
        components = splitDirectories p
        directory = takeFileName (takeDirectory p)
        suffix = reverse (takeWhile (/= '-') (reverse directory))
    if length components < 4 || takeFileName (takeDirectory (takeDirectory p)) /= ".fingerprint" || length suffix /= 16 || not (all isHexDigit suffix) then pure Nothing else do
      status <- getSymbolicLinkStatus (base </> p)
      if not (isRegularFile status) || linkCount status /= 1 || fileSize status /= 16 then pure Nothing else do
        marker <- BC.unpack <$> B.readFile (base </> p)
        let json = base </> (p ++ ".json")
        exists <- doesPathExist json
        if not (all isHexDigit marker && exists) then pure Nothing else do
          jsonStatus <- getSymbolicLinkStatus json
          if not (isRegularFile jsonStatus) then pure Nothing else do
            bytes <- B.readFile json
            pure $ case eitherDecodeStrict' bytes :: Either String CargoFingerprint of
              Right _ -> Just (ValidatedFingerprint relative unit (UnitHash suffix))
              Left _ -> Nothing

artifactOwners :: [ValidatedFingerprint] -> RelativePath -> [RelativePath]
artifactOwners fingerprints = lookupOwners
 where
  units = [(fp, takeDirectory (takeDirectory (takeDirectory relative)), takeFileName (takeDirectory relative), unit, suffix) | ValidatedFingerprint fp unit (UnitHash suffix) <- fingerprints, let relative = relativeFilePath fp]
  exact = Map.fromListWith (++) $ concatMap ownedFiles units
  ownedFiles (fp, profile, directory, unit, suffix) =
    [(relativePath file, [fp]) | file <- case unit of
      LibraryUnit (CrateName crate) -> [profile </> "deps" </> ("lib" ++ crate ++ "-" ++ suffix ++ extension) | extension <- [".rlib", ".rmeta", ".so"]]
      TestUnit (CrateName crate) -> [profile </> "deps" </> (crate ++ "-" ++ suffix)]
      BinaryUnit _ -> []
      BuildScriptUnit -> [profile </> "build" </> directory </> "build-script-build"]]
  crateName unit = case unit of
    LibraryUnit crate -> Just crate
    BinaryUnit crate -> Just crate
    TestUnit crate -> Just crate
    BuildScriptUnit -> Nothing
  incremental = Map.fromListWith (++) [((relativePath profile, crate), [fp]) | (fp, profile, _, unit, _) <- units, Just crate <- [crateName unit]]
  lookupOwners relative =
    let components = splitDirectories (relativeFilePath relative)
        (prefix, remainder) = break (== "incremental") components
    in Map.findWithDefault [] relative exact ++ case remainder of
      ["incremental", unit, session, artifact] | "s-" `isPrefixOf` session && artifact `elem` ["dep-graph.bin", "query-cache.bin", "work-products.bin"] ->
        Map.findWithDefault [] (relativePath (foldl (</>) "" prefix), CrateName (takeWhile (/= '-') unit)) incremental
      _ -> []

snapshot :: Root -> IO Binding
snapshot = scan HashContents


scan :: ScanMode -> Root -> IO Binding
scan mode binding@(Root base) = do
  when (mode == HashContents) $ hPutStrLn stderr ("Hashing Cargo target contents: " ++ base)
  _ <- safeRoot base
  files <- walk mode base
  let fileSet = Set.fromList files
  let profiles = Set.fromList [relativePath (takeDirectory relative) | packed <- files, let relative = relativeFilePath packed, takeFileName relative == ".cargo-lock", Set.member (relativePath (takeDirectory relative </> "deps")) fileSet, Set.member (relativePath (takeDirectory relative </> ".fingerprint")) fileSet]
  ensure (not (Set.null profiles)) "no verified Cargo profile structures"
  forM_ (Set.toList profiles) $ \profile -> forM_ [".cargo-lock", "deps", ".fingerprint"] $ \component -> do
    status <- getSymbolicLinkStatus (base </> relativeFilePath profile </> component)
    ensure (if component == ".cargo-lock" then isRegularFile status else isDirectory status) "invalid Cargo profile structure"
  fingerprints <- validFingerprints binding files
  forM_ fingerprints $ \(ValidatedFingerprint fp _ _) -> ensure (Set.member (relativePath (takeDirectory (takeDirectory (takeDirectory (relativeFilePath fp))))) profiles) "Cargo fingerprint has no verified profile"
  let owners = artifactOwners fingerprints
  records <- fmap catMaybes $ forM files $ \packedPath ->
    (do
      let relative = relativeFilePath packedPath
      status <- getSymbolicLinkStatus (base </> relative)
      ensure ((mode == RetentionMetadata && isSymbolicLink status) || (not (isSymbolicLink status) && (isDirectory status || isRegularFile status))) "special file or symlink in Cargo root"
      let bytes = Bytes (if isRegularFile status then fromIntegral (fileSize status) else 0)
          proposed = if isRegularFile status && linkCount status == 1 && not (null (owners packedPath)) then CompilerArtifact else Retain
      recognized <- if proposed == CompilerArtifact then compilerHeader (base </> relative) else pure True
      let selected = recognized && proposed == CompilerArtifact
      hash <- if mode == HashContents && selected then Just <$> hashFile bytes (base </> relative) else pure Nothing
      let modified = FileTimeNanoseconds (floor (toRational (modificationTimeHiRes status) * 1_000_000_000))
      pure (Just (Entry packedPath (fromIntegral (deviceID status)) (fromIntegral (fileID status)) (OwnerUID (fromIntegral (fileOwner status))) bytes hash (if selected then CompilerArtifact else Retain) (if isSymbolicLink status then SymbolicLink else if isRegularFile status then RegularFile else Directory) modified)))
      `catch` \(err :: IOException) -> if not (null (relativeFilePath packedPath)) && isDoesNotExistError err then pure Nothing else ioError err
  let selectedOwners = Set.fromList [owner | entry <- records, disposition entry == CompilerArtifact, owner <- owners (path entry)]
  selected <- forM records $ \entry ->
    if Set.member (path entry) selectedOwners then do
      hash <- if mode == HashContents then Just <$> hashFile (size entry) (base </> relativeFilePath (path entry)) else pure Nothing
      pure entry { disposition = FingerprintSuccess, digest = hash }
    else pure entry
  pure (Binding binding selected)

summarize :: Binding -> RootSummary
summarize (Binding binding records) =
  let regular = filter ((== RegularFile) . entryType) records
      selected = filter ((/= Retain) . disposition) regular
      kept = filter ((== Retain) . disposition) regular
      count = FileCount . fromIntegral . length
      total group = Bytes (sum [n | entry <- group, let Bytes n = size entry])
  in RootSummary binding (count selected) (total selected) (count kept) (total kept)

compilerHeader :: FilePath -> IO Bool
compilerHeader file = withBinaryFile file ReadMode $ \handle -> do
  prefix <- B.hGet handle 8
  pure $ case takeExtension file of
    ".rlib" -> prefix == BC.pack "!<arch>\n"
    ".rmeta" -> BC.pack "rust" `B.isPrefixOf` prefix
    ".so" -> B.pack [127,69,76,70] `B.isPrefixOf` prefix
    ".bin" -> any (`B.isPrefixOf` prefix) [BC.pack "rust", BC.pack "RSIC"]
    _ -> B.pack [127,69,76,70] `B.isPrefixOf` prefix

space :: Root -> IO (Root, Bytes)
space r@(Root p) = do
  (code, out, _) <- readProcessWithExitCode "df" ["-B1", "--output=avail", "--", p] ""
  case (code, words out) of
    (ExitSuccess, [_, n]) | [(count, "")] <- reads n -> pure (r, Bytes count)
    _ -> ioError (userError "cannot measure available disk space")

activity :: Root -> IO Activity
activity r@(Root base) = do
  uid <- fileOwner <$> getSymbolicLinkStatus base
  caller <- getEffectiveUserID
  let scope = if caller == 0 then AllProcesses else OwnerProcesses
  encoding <- getFileSystemEncoding
  encodedPrefix <- GHC.withCStringLen encoding (base ++ "/") B.packCStringLen
  let mappedPrefix = if B.elem 10 encodedPrefix then B.intercalate "\\012" (B.split 10 encodedPrefix) else encodedPrefix
  self <- show <$> getProcessID
  pids <- filter (\p -> not (null p) && all isDigit p && p /= self) <$> listDirectory "/proc"
  inspected <- forM pids $ \pid -> (,) (ProcessId pid) <$> inspect scope uid encoding mappedPrefix pid
  pure (Activity r [(pid, uses) | (pid, Right uses) <- inspected, not (null uses)] [issue | (_, Left issue) <- inspected])
 where
  inspect scope uid encoding mappedPrefix pid = do
    let proc = "/proc" </> pid
    let check = do
          status <- getFileStatus proc
          if scope == OwnerProcesses && fileOwner status /= uid then pure [] else do
            processStatus <- BC.readFile (proc </> "status")
            if any (\line -> any (`B.isPrefixOf` line) ["State:\tZ", "Kthread:\t1"]) (BC.lines processStatus) then pure [] else do
              cwd <- readSymbolicLink (proc </> "cwd")
              exe <- readSymbolicLink (proc </> "exe")
              environment <- B.readFile (proc </> "environ")
              targets <- forM [B.drop 17 field | field <- B.split 0 environment, "CARGO_TARGET_DIR=" `B.isPrefixOf` field] $ \raw -> do
                target <- B.useAsCStringLen raw (GHC.peekCStringLen encoding)
                canonicalizePath (if isAbsolute target then target else cwd </> target)
              fds <- listDirectory (proc </> "fd")
              links <- forM fds $ \fd -> readSymbolicLink (proc </> "fd" </> fd) `catch` \(err :: IOException) -> if isDoesNotExistError err then pure "" else ioError err
              mappings <- BC.lines <$> B.readFile (proc </> "maps")
              mapped <- forM [mappingPath line | line <- mappings, mappedPrefix `B.isPrefixOf` mappingPath line] $ \raw ->
                B.useAsCStringLen raw (GHC.peekCStringLen encoding)
              let mappedNames = concatMap (\file -> [file, T.unpack (T.replace "\\012" "\n" (T.pack file))]) mapped
                  references = [FileReference (relativePath (makeRelative base file)) | file <- exe : links ++ mappedNames, inside base file]
                  context = [TargetContext | any (inside base) (cwd : targets)]
              pure (context ++ references)
    (Right <$> check) `catch` \(err :: IOException) -> do
      exists <- doesDirectoryExist proc
      if not exists && isDoesNotExistError err then pure (Right [])
        else pure (Left (InspectionIssue (ProcessId pid) (show err)))
  mappingPath = skip 5
    where
      fieldSpace byte = byte == 32 || byte == 9
      skip :: Int -> B.ByteString -> B.ByteString
      skip 0 line = line
      skip count line = skip (count - 1) (B.dropWhile fieldSpace (B.dropWhile (not . fieldSpace) line))

data LockMode = WaitForLocks | DeferBusyLocks deriving Eq
data LockResult a = LocksBusy | LocksAcquired a

withLocks :: LockMode -> [Binding] -> IO a -> IO (LockResult a)
withLocks mode roots action = acquire lockPaths
 where
  lockPaths = sort (nub [base </> relative | Binding (Root base) records <- roots, entry <- records, let relative = relativeFilePath (path entry), takeFileName relative `elem` [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"], entryType entry == RegularFile])
  acquire [] = LocksAcquired <$> action
  acquire (p:ps) = bracket (openFd p ReadWrite defaultFileFlags { nofollow = True, cloexec = True }) closeFd $ \(Fd fd) -> do
    result <- c_flock fd (if mode == WaitForLocks then 2 else 6)
    if result == 0 then acquire ps else do
      failure <- getErrno
      if mode == DeferBusyLocks && (failure == eAGAIN || failure == eWOULDBLOCK) then pure LocksBusy
      else ioError (userError "cannot acquire Cargo lock")

writeJson :: ToJSON a => FilePath -> a -> IO ()
writeJson p = BL.writeFile p . encode

readPlan :: FilePath -> IO Plan
readPlan file = do
  bytes <- B.readFile file
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

validateBindings :: Plan -> IO [Root]
validateBindings plan = do
  ensure (schemaVersion plan == 3 && not (null (bindings plan))) "unsupported or empty plan"
  let roots = map root (bindings plan)
  ensure (length roots == length (nub roots)) "duplicate root"
  forM_ [(a,b) | a@(Root pa) <- roots, b@(Root pb) <- roots, a /= b, inside pa pb] $ \_ -> ensure False "overlapping roots"
  forM_ (bindings plan) $ \(Binding (Root base) records) -> do
    _ <- safeRoot base
    let paths = map path records
    ensure (length paths == Set.size (Set.fromList paths)) "duplicate planned path"
    forM_ records $ \entry -> do
      let relative = relativeFilePath (path entry)
      ensure (not (isAbsolute relative) && '\0' `notElem` relative && not (any (`elem` [".", ".."]) (splitDirectories relative)) && (null relative || normalise relative == relative)) "invalid planned relative path"
      when (disposition entry /= Retain) $
        ensure (entryType entry == RegularFile && case digest entry of Just (Digest text) -> T.length text == 64 && T.all isHexDigit text; Nothing -> False) "selected file lacks a content binding"
  let selected = Set.fromList [(r, path entry) | Binding r records <- bindings plan, entry <- records, disposition entry /= Retain]
      grouped = [(unitRoot unit, file) | unit <- coherentUnits plan, file <- unitFingerprint unit : unitArtifacts unit]
  when (not (null (coherentUnits plan))) $ do
    ensure (length grouped == Set.size (Set.fromList grouped) && Set.fromList grouped == selected) "coherent units do not partition selected artifacts"
    forM_ (coherentUnits plan) $ \unit -> do
      ensure (not (null (unitArtifacts unit))) "empty coherent unit"
      ensure (unitControl unit == relativePath (relativeFilePath (unitFingerprint unit) ++ ".json")) "invalid unit control path"
      ensure (any (\(Binding r records) -> r == unitRoot unit && any ((== unitControl unit) . path) records) (bindings plan)) "missing unit control binding"
  pure roots


verifyRemoval :: Root -> Map.Map RelativePath Entry -> Entry -> IO ()
verifyRemoval (Root base) identities entry = do
  let relative = relativeFilePath (path entry)
      ancestors p = p : if null p || p == "." then [] else ancestors (takeDirectory p)
  forM_ (ancestors (takeDirectory relative)) $ \directory -> do
    let key = if directory == "." then "" else directory
    expected <- maybe (ioError (userError "unbound ancestor")) pure (Map.lookup (relativePath key) identities)
    status <- getSymbolicLinkStatus (base </> key)
    ensure (isDirectory status && fromIntegral (deviceID status) == device expected && fromIntegral (fileID status) == inode expected && OwnerUID (fromIntegral (fileOwner status)) == ownerUID expected) "compiler output ancestor changed"
  status <- getSymbolicLinkStatus (base </> relative)
  ensure (isRegularFile status && linkCount status == 1 && fromIntegral (deviceID status) == device entry && fromIntegral (fileID status) == inode entry && OwnerUID (fromIntegral (fileOwner status)) == ownerUID entry && Bytes (fromIntegral (fileSize status)) == size entry) "compiler output identity changed"

busyArtifacts :: [Binding] -> IO (Set.Set (Root, RelativePath))
busyArtifacts records = do
  uses <- mapM (activity . root) records
  ensure (all (\(Activity _ _ unavailable) -> null unavailable) uses)
    "active-use inspection is incomplete; no files removed. Obtain an explicitly authorized privileged host scan instead of bypassing process checks"
  pure (Set.fromList [(r, file) | Activity r users _ <- uses, (_, references) <- users, FileReference file <- references])

apply :: FilePath -> IO ()
apply file = do
  plan <- readPlan file
  _ <- validateBindings plan
  hPutStrLn stderr "Waiting for Cargo locks; agents and applications may remain running"
  result <- withLocks WaitForLocks (bindings plan) $ do
    current <- mapM (scan MetadataOnly . root) (bindings plan)
    applyLocked file plan current
  case result of
    LocksAcquired audit -> BL.putStr (encode audit)
    LocksBusy -> ioError (userError "cannot acquire Cargo locks")

applyLocked :: FilePath -> Plan -> [Binding] -> IO Audit
applyLocked planFile plan current = do
    let auditPath = planFile ++ ".audit.json"
    auditAbsolute <- makeAbsolute auditPath
    forM_ (bindings plan) $ \(Binding (Root base) _) -> ensure (not (inside base auditAbsolute)) "audit must be outside managed roots"
    exists <- doesPathExist auditPath
    ensure (not exists) "audit already exists"
    busyBefore <- busyArtifacts current
    let identity entry = (device entry, inode entry, ownerUID entry, entryType entry)
    candidates <- forM (zip (bindings plan) current) $ \(Binding r planned, Binding _ actual) -> do
      let original = Map.fromList [(path entry, entry) | entry <- planned]
          latest = Map.fromList [(path entry, entry) | entry <- actual]
          rootEntry records = Map.lookup (relativePath "") records
          sameRoot = case (rootEntry original, rootEntry latest) of
            (Just old, Just new) -> identity old == identity new
            _ -> False
          sameAncestors entry =
            let parents p = p : if null p || p == "." then [] else parents (takeDirectory p)
            in all (\p -> let key = relativePath (if p == "." then "" else p)
                          in case (Map.lookup key original, Map.lookup key latest) of
                            (Just old, Just new) -> identity old == identity new
                            _ -> False) (parents (takeDirectory (relativeFilePath (path entry))))
      ensure sameRoot "managed root identity changed"
      fingerprints <- validFingerprints r (map path actual)
      let freshUnits = Map.fromList [(unitFingerprint group, (group, safe)) | UnitMetadata group _ _ safe <- ownedUnits [LibraryUnits, CargoTestUnits, BuildScriptUnits] (Binding r actual) fingerprints]
          groups = filter ((== r) . unitRoot) (coherentUnits plan)
          version records file = (\entry -> entry {digest = Nothing}) <$> Map.lookup file records
          groupMatches group = case Map.lookup (unitFingerprint group) freshUnits of
            Just (fresh, True) -> fresh == group && all (\file -> version original file == version latest file) (unitControl group : unitFingerprint group : unitArtifacts group)
            _ -> False
          unavailable = Set.fromList [file | group <- groups,
            not (groupMatches group) || any (\member -> Set.member (r, member) busyBefore) (unitFingerprint group : unitArtifacts group),
            file <- unitFingerprint group : unitArtifacts group]
      checked <- forM [entry | entry <- planned, disposition entry /= Retain] $ \entry -> do
        let reject reason = pure (entry, Just reason)
        case Map.lookup (path entry) latest of
          Nothing -> reject Missing
          Just now
            | entry {digest = Nothing} /= now {digest = Nothing} || not (sameAncestors entry) -> reject Changed
            | Set.member (path entry) unavailable -> reject UnitUnavailable
            | Set.member (r, path entry) busyBefore -> reject InUse
            | otherwise -> do
                actualHash <- hashFile (size entry) (let Root base = r in base </> relativeFilePath (path entry))
                pure (entry, if Just actualHash == digest entry then Nothing else Just Changed)
      let unlockedProfiles = Set.fromList [relativePath (takeDirectory (relativeFilePath (path entry))) |
            entry <- actual, takeFileName (relativeFilePath (path entry)) `elem` [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"],
            case Map.lookup (path entry) original of Just old -> identity old /= identity entry; Nothing -> True]
      pure (r, original, artifactOwners fingerprints, unlockedProfiles, checked, groups)
    busyAfter <- busyArtifacts current
    let selections = [choose r original owners unlockedProfiles checked groups | (r, original, owners, unlockedProfiles, checked, groups) <- candidates]
        choose r original owners unlockedProfiles checked groups =
          let usable = Map.fromList [(path entry, entry) | (entry, Nothing) <- checked, not (Set.member (r, path entry) busyAfter)]
              unavailableGroups = Set.fromList [file | group <- groups,
                not (all (`Map.member` usable) (unitFingerprint group : unitArtifacts group)),
                file <- unitFingerprint group : unitArtifacts group]
              available = Map.withoutKeys usable unavailableGroups
              usableOwner marker = Map.member marker available &&
                not (Set.member (relativePath (takeDirectory (takeDirectory (takeDirectory (relativeFilePath marker))))) unlockedProfiles)
              outputs = [entry | entry <- Map.elems available, disposition entry == CompilerArtifact,
                          not (null (owners (path entry))), all usableOwner (owners (path entry))]
              markers = Set.fromList (concatMap (owners . path) outputs)
              chosen = outputs ++ [entry | entry <- Map.elems available, disposition entry == FingerprintSuccess, Set.member (path entry) markers]
              selected = Set.fromList (map path chosen)
              omitted = [Skipped r (path entry) (case reason of
                            Just why -> why
                            Nothing | Set.member (r, path entry) busyAfter -> InUse
                                    | Set.member (path entry) unavailableGroups -> UnitUnavailable
                                    | disposition entry == CompilerArtifact -> OwnerUnavailable
                                    | otherwise -> NoSelectedArtifact)
                        | (entry, reason) <- checked, not (Set.member (path entry) selected)]
          in (r, original, chosen, omitted)
        omissions = concat [items | (_, _, _, items) <- selections]
    forM_ selections $ \(r, original, chosen, _) -> mapM_ (verifyRemoval r original) chosen
    before <- mapM (space . root) current
    deleted <- newIORef []
    let persist ok = do
          after <- mapM (space . root) current
          let deltas = [(r, Bytes (a-b)) | ((r,Bytes b),(_,Bytes a)) <- zip before after]
          actualRemoved <- reverse <$> readIORef deleted
          let audit = Audit 3 (if ok then Complete else Interrupted) plan actualRemoved omissions before after deltas ok
          writeJson auditPath audit
          pure audit
    writeJson auditPath (Audit 3 Applying plan [] omissions before before [(r,Bytes 0) | (r,_) <- before] False)
    (do
      hPutStrLn stderr "Removing only unchanged, unused, approved compiler artifacts"
      forM_ [FingerprintSuccess, CompilerArtifact] $ \kind -> forM_ selections $ \(r@(Root base), original, chosen, _) ->
        withDeletionCredentials r $ forM_ chosen $ \entry -> when (disposition entry == kind) $ do
          verifyRemoval r original entry
          removeFile (base </> relativeFilePath (path entry))
          modifyIORef' deleted ((r, path entry):)
      persist True) `onException` persist False

withDeletionCredentials :: Root -> IO a -> IO a
withDeletionCredentials (Root base) action = do
  caller <- getEffectiveUserID
  target <- getSymbolicLinkStatus base
  if caller /= 0 || fileOwner target == 0 then action else do
    originalGroup <- getEffectiveGroupID
    originalGroups <- getGroups
    let restore _ = do
          setEffectiveUserID caller
          setEffectiveGroupID originalGroup
          setGroups originalGroups
    bracket (pure ()) restore $ \_ -> do
      setGroups []
      setEffectiveGroupID (fileGroup target)
      setEffectiveUserID (fileOwner target)
      action

data RetentionClass = LibraryUnits | CargoTestUnits | BuildScriptUnits deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype AuditDirectory = AuditDirectory FilePath deriving stock (Generic) deriving anyclass (FromJSON)
data RetentionRootPolicy = RetentionRootPolicy Root Bytes Bytes AgeSeconds deriving (Eq, Show)
instance FromJSON RetentionRootPolicy where
  parseJSON = withObject "retention root" $ \o -> RetentionRootPolicy <$> o .: "cargoTarget" <*> o .: "budgetBytes" <*> o .: "targetBytes" <*> o .: "minimumAgeSeconds"
instance ToJSON RetentionRootPolicy where
  toJSON (RetentionRootPolicy r high low age) = object
    ["cargoTarget" .= r, "budgetBytes" .= high, "targetBytes" .= low, "minimumAgeSeconds" .= age]
data RetentionPolicy = RetentionPolicy Int [RetentionRootPolicy] [RetentionClass] AuditDirectory
instance FromJSON RetentionPolicy where
  parseJSON = withObject "retention policy" $ \o -> RetentionPolicy <$> o .: "policyVersion" <*> o .: "roots" <*> o .: "artifactClasses" <*> o .: "auditDirectory"
data RetentionMode = PreviewRetention | ApplyRetention Digest
data RetentionStatus = BelowBudget | PreviewSelection | RetentionComplete | TargetUnmet | DeferredBusy
instance ToJSON RetentionStatus where
  toJSON result = toJSON (case result of
    BelowBudget -> "BelowBudget"
    PreviewSelection -> "Preview"
    RetentionComplete -> "Complete"
    TargetUnmet -> "TargetUnmet"
    DeferredBusy -> "DeferredBusy" :: T.Text)
data UnitMetadata = UnitMetadata CoherentUnit [Entry] FileTimeNanoseconds Bool
data RetentionRootReport = RetentionRootReport Root RetentionStatus Bytes Bytes Bytes Bytes [UnitMetadata] FileCount FileCount FileCount Bool (Maybe FilePath) (Maybe Bool) [RelativePath]
instance ToJSON RetentionRootReport where
  toJSON (RetentionRootReport r result before after high low units removedCount recentCount blockedCount met audit inspected removedMarkers) = object
    [ "cargoTarget" .= r, "status" .= result, "beforeBytes" .= before, "afterBytes" .= after
    , "budgetBytes" .= high, "targetBytes" .= low, "selectedUnitCount" .= countItems units
    , "removedUnitCount" .= removedCount, "selectedBytes" .= totalUnitBytes units
    , "removedUnitFingerprints" .= removedMarkers
    , "recentUnitCount" .= recentCount, "blockedUnitCount" .= blockedCount
    , "targetMet" .= met, "auditPath" .= audit, "inspectionComplete" .= inspected
    , "units" .= [object ["fingerprint" .= unitFingerprint group, "members" .= (unitFingerprint group : unitArtifacts group),
                         "bytes" .= totalUnitBytes [unit], "newestMtimeNanoseconds" .= newest]
                | unit@(UnitMetadata group _ newest _) <- units]]

countItems :: [a] -> FileCount
countItems = FileCount . fromIntegral . length

sumBytes :: [Bytes] -> Bytes
sumBytes values = Bytes (sum [value | Bytes value <- values])

totalUnitBytes :: [UnitMetadata] -> Bytes
totalUnitBytes units = sumBytes [size entry | UnitMetadata group records _ _ <- units, entry <- records,
  path entry `elem` (unitFingerprint group : unitArtifacts group)]

unitClass :: CargoUnit -> Maybe RetentionClass
unitClass unit = case unit of
  LibraryUnit _ -> Just LibraryUnits
  TestUnit _ -> Just CargoTestUnits
  BuildScriptUnit -> Just BuildScriptUnits
  BinaryUnit _ -> Nothing

ownedUnits :: [RetentionClass] -> Binding -> [ValidatedFingerprint] -> [UnitMetadata]
ownedUnits classes (Binding r records) fingerprints =
  [makeUnit marker outputs | ValidatedFingerprint marker unit _ <- fingerprints,
    maybe False (`elem` classes) (unitClass unit), Just outputs <- [Map.lookup marker byOwner]]
 where
  identities = Map.fromList [(path entry, entry) | entry <- records]
  owners = artifactOwners fingerprints
  byOwner = Map.fromListWith (++) [(marker, [entry]) | entry <- records,
    "incremental" `notElem` splitDirectories (relativeFilePath (path entry)), [marker] <- [owners (path entry)]]
  makeUnit marker outputs =
    let control = relativePath (relativeFilePath marker ++ ".json")
        controls = catMaybes [Map.lookup marker identities, Map.lookup control identities]
        group = CoherentUnit r marker (sort (map path outputs)) control
        unitEntries = outputs ++ controls
        newest = maximum (map mtimeNanoseconds unitEntries)
        safe = length controls == 2 && all ((== CompilerArtifact) . disposition) outputs
               && maybe False ((== FingerprintSuccess) . disposition) (Map.lookup marker identities)
               && all ((== RegularFile) . entryType) controls
    in UnitMetadata group unitEntries newest safe

rootApparentBytes :: Root -> IO Bytes
rootApparentBytes (Root base) = do
  (code, out, err) <- readProcessWithExitCode "du" ["--apparent-size", "--summarize", "--block-size=1", "--", base] ""
  case (code, words out) of
    (ExitSuccess, value:_) | [(count, "")] <- reads value -> pure (Bytes count)
    _ -> ioError (userError ("cannot measure Cargo root metadata: " ++ err))

digestBytes :: B.ByteString -> Digest
digestBytes bytes = Digest (T.pack (concatMap hex (B.unpack (SHA.hash bytes))))
 where
  hex byte = let value = showHex byte "" in replicate (2 - length value) '0' ++ value

digestText :: Digest -> FilePath
digestText (Digest value) = T.unpack value

guardIdentities :: Binding -> [(RelativePath, Integer, Integer, OwnerUID)]
guardIdentities (Binding _ records) =
  [(path entry, device entry, inode entry, ownerUID entry) | entry <- records,
    takeFileName (relativeFilePath (path entry)) `elem` [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"]]

validateRetentionPolicy :: RetentionPolicy -> IO ()
validateRetentionPolicy (RetentionPolicy version policies classes (AuditDirectory directory)) = do
  ensure (version == 1 && not (null policies) && not (null classes)) "unsupported or empty retention policy"
  ensure (length classes == length (nub classes)) "duplicate retention artifact class"
  let roots = [r | RetentionRootPolicy r _ _ _ <- policies]
  ensure (length roots == length (nub roots)) "duplicate retention root"
  forM_ [(a,b) | a@(Root pa) <- roots, b@(Root pb) <- roots, a /= b, inside pa pb] $ \_ ->
    ensure False "overlapping retention roots"
  forM_ policies $ \(RetentionRootPolicy (Root base) (Bytes high) (Bytes low) (AgeSeconds age)) -> do
    _ <- safeRoot base
    ensure (high > 0 && low >= 0 && low < high && age > 0) "invalid retention budget, lower target or minimum age"
  absolute <- makeAbsolute directory
  canonical <- canonicalizePath directory
  ensure (isAbsolute directory && directory == absolute && directory == canonical) "audit directory must be canonical and absolute"
  forM_ roots $ \(Root base) -> ensure (not (inside base directory)) "retention audit directory is inside a managed root"

inspectionReferences :: [Binding] -> IO (Set.Set (Root, RelativePath), Bool)
inspectionReferences records = do
  uses <- mapM (activity . root) records
  pure (Set.fromList [(r, file) | Activity r users _ <- uses, (_, references) <- users, FileReference file <- references],
        all (\(Activity _ _ issues) -> null issues) uses)

retention :: FilePath -> RetentionMode -> IO ()
retention policyFile mode = do
  raw <- B.readFile policyFile
  policy@(RetentionPolicy _ policies classes directory) <- either (ioError . userError) pure (eitherDecodeStrict' raw)
  validateRetentionPolicy policy
  let policyHash = digestBytes raw
  case mode of
    PreviewRetention -> pure ()
    ApplyRetention approved -> ensure (approved == policyHash) "standing approval does not match policy SHA-256"
  results <- mapM (retainRoot mode classes directory) policies
  let modeName = case mode of PreviewRetention -> "Preview"; ApplyRetention _ -> "Apply" :: T.Text
  BL.putStr (encode (object ["policySha256" .= policyHash, "runMode" .= modeName, "rootReports" .= results]))

retainRoot :: RetentionMode -> [RetentionClass] -> AuditDirectory -> RetentionRootPolicy -> IO RetentionRootReport
retainRoot mode classes directory policy@(RetentionRootPolicy r high low _) = do
  measured <- rootApparentBytes r
  let report result after units removedCount recentCount blockedCount met audit inspected =
        RetentionRootReport r result measured after high low units removedCount recentCount blockedCount met audit inspected []
  if measured <= high then pure (report BelowBudget measured [] (FileCount 0) (FileCount 0) (FileCount 0) True Nothing Nothing)
  else do
    metadata <- scan RetentionMetadata r
    locked <- withLocks DeferBusyLocks [metadata] $ do
      current <- scan RetentionMetadata r
      if guardIdentities current /= guardIdentities metadata then
        pure (report DeferredBusy measured [] (FileCount 0) (FileCount 0) (FileCount 0) False Nothing Nothing)
      else retainLocked mode classes directory policy current
    case locked of
      LocksBusy -> pure (report DeferredBusy measured [] (FileCount 0) (FileCount 0) (FileCount 0) False Nothing Nothing)
      LocksAcquired result -> pure result

retainLocked :: RetentionMode -> [RetentionClass] -> AuditDirectory -> RetentionRootPolicy -> Binding -> IO RetentionRootReport
retainLocked mode classes directory (RetentionRootPolicy r high low@(Bytes target) (AgeSeconds age)) current = do
  before@(Bytes initial) <- rootApparentBytes r
  let report result after units removedCount recentCount blockedCount met audit inspected =
        RetentionRootReport r result before after high low units removedCount recentCount blockedCount met audit inspected []
  if before <= high then pure (report BelowBudget before [] (FileCount 0) (FileCount 0) (FileCount 0) True Nothing Nothing)
  else do
    fingerprints <- validFingerprints r (map path (entries current))
    now <- floor . (* 1_000_000_000) . toRational <$> getPOSIXTime
    (busy, inspected) <- inspectionReferences [current]
    case mode of
      ApplyRetention _ -> ensure inspected "active-use inspection is incomplete; retention removed no files"
      PreviewRetention -> pure ()
    let units = ownedUnits classes current fingerprints
        recent (UnitMetadata _ _ (FileTimeNanoseconds newest) _) = newest > now - age * 1_000_000_000
        blocked (UnitMetadata group _ _ safe) = not safe || any (\member -> Set.member (r, member) busy) (unitFingerprint group : unitArtifacts group)
        eligible = sortOn (\(UnitMetadata group _ newest _) -> (newest, unitFingerprint group)) [unit | unit <- units, not (recent unit), not (blocked unit)]
        select _ [] = []
        select remaining (unit:rest)
          | remaining <= target = []
          | otherwise = unit : select (remaining - let Bytes bytes = totalUnitBytes [unit] in bytes) rest
        selectedUnits = select initial eligible
        recentCount = countItems (filter recent units)
        blockedCount = countItems (filter blocked units)
        Bytes selectedTotal = totalUnitBytes selectedUnits
        projectedMet = initial - selectedTotal <= target
    case mode of
      PreviewRetention -> pure (report PreviewSelection before selectedUnits (FileCount 0) recentCount blockedCount projectedMet Nothing (Just inspected))
      ApplyRetention approved
        | null selectedUnits -> pure (report TargetUnmet before [] (FileCount 0) recentCount blockedCount False Nothing (Just inspected))
        | otherwise -> do
            (planFile, finalAudit) <- retentionPaths directory r
            let groups = [group | UnitMetadata group _ _ _ <- selectedUnits]
                selectedPaths = Set.fromList [member | group <- groups, member <- unitFingerprint group : unitArtifacts group]
            records <- forM (entries current) $ \entry ->
              if Set.member (path entry) selectedPaths then do
                hash <- hashFile (size entry) (let Root base = r in base </> relativeFilePath (path entry))
                pure entry {digest = Just hash}
              else pure entry {disposition = Retain, digest = Nothing}
            let plan = Plan 3 [Binding r records] groups (Just (RetentionContext approved (RetentionRootPolicy r high low (AgeSeconds age)) classes))
            _ <- validateBindings plan
            writeJson planFile plan
            latest <- scan RetentionMetadata r
            audit <- applyLocked planFile plan [latest]
            let removedPaths = Set.fromList (removed audit)
                completedUnits = [group | group <- groups, all (\member -> Set.member (r, member) removedPaths) (unitFingerprint group : unitArtifacts group)]
            renameFile (planFile ++ ".audit.json") finalAudit
            removeFile planFile
            after <- rootApparentBytes r
            pure (RetentionRootReport r (if after <= low then RetentionComplete else TargetUnmet) before after high low selectedUnits (countItems completedUnits) recentCount blockedCount (after <= low) (Just finalAudit) (Just inspected) (map unitFingerprint completedUnits))

retentionPaths :: AuditDirectory -> Root -> IO (FilePath, FilePath)
retentionPaths (AuditDirectory directory) (Root base) = do
  createDirectoryIfMissing True directory
  canonical <- canonicalizePath directory
  ensure (canonical == directory) "retention audit directory changed"
  auditStatus <- getSymbolicLinkStatus directory
  caller <- getEffectiveUserID
  ensure (isDirectory auditStatus && not (isSymbolicLink auditStatus) && fileOwner auditStatus == caller) "retention audit directory must be owned by the invoking account"
  setFileMode directory 0o700
  let bucket = directory </> digestText (digestBytes (TE.encodeUtf8 (T.pack base)))
  createDirectoryIfMissing False bucket
  bucketStatus <- getSymbolicLinkStatus bucket
  ensure (isDirectory bucketStatus && not (isSymbolicLink bucketStatus) && fileOwner bucketStatus == caller) "invalid retention audit bucket"
  setFileMode bucket 0o700
  let planFile = bucket </> "pending.plan.json"
  forM_ [planFile, planFile ++ ".audit.json"] $ \checkpoint -> do
    exists <- doesPathExist checkpoint
    ensure (not exists) "unfinished retention checkpoint requires inspection before another automatic deletion"
  pure (planFile, bucket </> "last-complete.audit.json")

parseRoots :: [String] -> IO [Root]
parseRoots arguments = do
  paths <- go arguments
  ensure (not (null paths)) "at least one --cargo-target is required"
  roots <- mapM (makeAbsolute >=> safeRoot) paths
  ensure (length roots == length (nub roots)) "duplicate root"
  pure roots
 where
  go [] = pure []
  go ("--cargo-target":p:rest) = (p:) <$> go rest
  go _ = ioError (userError "expected repeated --cargo-target DIR")
  (>=>) f g value = f value >>= g

main :: IO ()
main = (getArgs >>= run) `catch` \(err :: IOException) -> die (show err)
 where
  run ("inventory":arguments) = do
    roots <- parseRoots arguments
    records <- mapM (scan MetadataOnly) roots
    free <- mapM space roots
    users <- mapM activity roots
    BL.putStr (encode (Inventory (map summarize records) free [(r, map fst active) | Activity r active _ <- users] [(r, unavailable) | Activity r _ unavailable <- users]))
  run ("plan":arguments) = case reverse arguments of
    output:"--output":rest -> do
      roots <- parseRoots (reverse rest)
      absolute <- makeAbsolute output
      forM_ roots $ \(Root base) -> ensure (not (inside base absolute)) "plan must be outside managed roots"
      let requireNewOutput = do
            exists <- doesPathExist output
            ensure (not exists) "plan output already exists"
      requireNewOutput
      hPutStrLn stderr "Inspecting target metadata before acquiring Cargo locks"
      metadata <- mapM (scan MetadataOnly) roots
      ensure (not (null metadata)) "empty target list"
      hPutStrLn stderr "Waiting for Cargo locks before planning compiler artifacts"
      result <- withLocks WaitForLocks metadata $ do
        records <- mapM snapshot roots
        requireNewOutput
        hPutStrLn stderr ("Writing exact maintenance plan: " ++ output)
        writeJson output (Plan 3 records [] Nothing)
      case result of
        LocksAcquired () -> pure ()
        LocksBusy -> ioError (userError "cannot acquire Cargo locks")
    _ -> usage
  run ["apply", "--plan", file] = apply file
  run ["retain", "--policy", file] = retention file PreviewRetention
  run ["retain", "--policy", file, "--apply", "--approved-policy-sha256", approved] = do
    ensure (length approved == 64 && all isHexDigit approved) "invalid standing-policy SHA-256"
    retention file (ApplyRetention (Digest (T.pack approved)))
  run _ = usage
  usage = die "usage: storage-maintenance inventory --cargo-target DIR ... | plan --cargo-target DIR ... --output PLAN | apply --plan PLAN | retain --policy FILE [--apply --approved-policy-sha256 HEX]"
