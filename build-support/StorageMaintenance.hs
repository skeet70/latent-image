{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, onException)
import Control.Monad (filterM, forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (FromJSON (parseJSON), ToJSON, eitherDecodeStrict', encode, withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, isDigit, isHexDigit)
import Data.List (isPrefixOf, nub, sort)
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.IORef (modifyIORef', newIORef, readIORef)
import Foreign.C.Types (CInt (..))
import GHC.Generics (Generic)
import Numeric (showHex)
import System.Directory (canonicalizePath, doesDirectoryExist, doesPathExist, listDirectory, makeAbsolute, removeFile)
import qualified GHC.Foreign as GHC
import GHC.IO.Encoding (getFileSystemEncoding)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die)
import System.FilePath ((</>), isAbsolute, splitDirectories, takeDirectory, takeExtension, takeFileName)
import System.IO (IOMode (ReadMode), hPutStrLn, stderr, withBinaryFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (deviceID, fileID, fileOwner, fileSize, getFileStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, linkCount, readSymbolicLink)
import System.Posix.IO (OpenFileFlags (cloexec, nofollow), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (getProcessID)
import System.Posix.Types (Fd (..))
import System.Posix.User (getEffectiveUserID)
import System.Process (readProcessWithExitCode)

newtype Root = Root FilePath deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype RelativePath = RelativePath FilePath deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype Bytes = Bytes Integer deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype Digest = Digest String deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype ProcessId = ProcessId String deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
newtype OwnerUID = OwnerUID Integer deriving stock (Eq, Show, Generic) deriving anyclass (ToJSON, FromJSON)
data InspectionIssue = InspectionIssue { inspectionPid :: ProcessId, inspectionFailure :: String } deriving (Generic, ToJSON)
data Activity = Activity Root [ProcessId] [InspectionIssue]
data ProcessScope = OwnerProcesses | AllProcesses deriving Eq
data Disposition = Retain | CompilerArtifact | FingerprintSuccess deriving (Eq, Show, Generic, ToJSON, FromJSON)
data EntryType = RegularFile | Directory deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Entry = Entry { path :: RelativePath, device :: Integer, inode :: Integer, ownerUID :: OwnerUID, size :: Bytes, digest :: Maybe Digest, disposition :: Disposition, entryType :: EntryType } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Binding = Binding { root :: Root, entries :: [Entry] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Plan = Plan { schemaVersion :: Int, bindings :: [Binding] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype FileCount = FileCount Integer deriving stock (Generic) deriving anyclass (ToJSON)
data RootSummary = RootSummary { canonicalCargoTarget :: Root, recognizedFileCount :: FileCount, recognizedBytes :: Bytes, retainedFileCount :: FileCount, retainedBytes :: Bytes } deriving (Generic, ToJSON)
data Inventory = Inventory { rootSummaries :: [RootSummary], availableBytes :: [(Root, Bytes)], activeUsers :: [(Root, [ProcessId])], uninspectableUsers :: [(Root, [InspectionIssue])] } deriving (Generic, ToJSON)
data ScanMode = MetadataOnly | HashContents deriving Eq
data AuditPhase = Applying | Complete | Interrupted deriving (Generic, ToJSON)
data Audit = Audit { auditSchemaVersion :: Int, auditPhase :: AuditPhase, appliedPlan :: Plan, removed :: [(Root, RelativePath)], beforeAvailable :: [(Root, Bytes)], afterAvailable :: [(Root, Bytes)], actualAvailableDelta :: [(Root, Bytes)], protectedFilesVerified :: Bool } deriving (Generic, ToJSON)

data CargoFingerprint = CargoFingerprint Integer String Integer Integer Integer [(Integer, String, Bool, Integer)]
instance FromJSON CargoFingerprint where
  parseJSON = withObject "Cargo fingerprint" $ \o -> CargoFingerprint <$> o .: "rustc" <*> o .: "features" <*> o .: "target" <*> o .: "profile" <*> o .: "path" <*> o .: "deps"

newtype RustMetadata = RustMetadata Integer
instance FromJSON RustMetadata where
  parseJSON = withObject "rustc metadata" $ \o -> RustMetadata <$> o .: "rustc_fingerprint"

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

ensure :: Bool -> String -> IO ()
ensure ok message = unless ok (ioError (userError message))

hashFile :: FilePath -> IO Digest
hashFile file = withBinaryFile file ReadMode (loop SHA.init)
 where
  loop context handle = do
    chunk <- B.hGetSome handle (1024 * 1024)
    if B.null chunk then pure (Digest (concatMap hex (B.unpack (SHA.finalize context)))) else loop (SHA.update context chunk) handle
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

walk :: FilePath -> IO [FilePath]
walk base = go ""
 where
  go relative = do
    status <- getSymbolicLinkStatus (base </> relative)
    ensure (not (isSymbolicLink status)) "symlinks in Cargo roots are not admitted"
    if isDirectory status then do
      children <- sort <$> listDirectory (base </> relative)
      descendants <- concat <$> mapM (go . (relative </>)) children
      pure (relative : descendants)
    else do
      ensure (isRegularFile status) "special file in Cargo root"
      pure [relative]

-- Cargo's success fingerprint is invalidated together with its compiler outputs.
validFingerprints :: FilePath -> [FilePath] -> IO [FilePath]
validFingerprints base files = filterM valid [p | p <- files, not (any (`elem` ["private-data", ".git"]) (splitDirectories p)), ".fingerprint" `elem` splitDirectories p, any (`isPrefixOf` takeFileName p) ["lib-", "bin-"] || takeFileName p == "build-script-build-script-build"]
 where
  valid p = do
    let components = splitDirectories p
        directory = takeFileName (takeDirectory p)
        suffix = reverse (takeWhile (/= '-') (reverse directory))
    if length components < 4 || takeFileName (takeDirectory (takeDirectory p)) /= ".fingerprint" || length suffix /= 16 || not (all isHexDigit suffix) then pure False else do
      status <- getSymbolicLinkStatus (base </> p)
      if not (isRegularFile status) || linkCount status /= 1 || fileSize status /= 16 then pure False else do
        marker <- BC.unpack <$> B.readFile (base </> p)
        let json = base </> (p ++ ".json")
        exists <- doesPathExist json
        if not (all isHexDigit marker && exists) then pure False else do
          bytes <- B.readFile json
          pure (case eitherDecodeStrict' bytes :: Either String CargoFingerprint of Right _ -> True; Left _ -> False)

artifactOwners :: [FilePath] -> FilePath -> [FilePath]
artifactOwners fingerprints = lookupOwners
 where
  units = [(fp, takeDirectory (takeDirectory (takeDirectory fp)), takeFileName (takeDirectory fp), takeFileName fp) | fp <- fingerprints]
  validCrate crate = not (null crate) && all (\c -> isAlphaNum c || c == '_') crate
  exact = Map.fromListWith (++) $
    [(profile </> "build" </> directory </> "build-script-build", [fp]) | (fp,profile,directory,marker) <- units, marker == "build-script-build-script-build"] ++
    [(profile </> "deps" </> ("lib" ++ crate ++ "-" ++ suffix ++ extension), [fp]) | (fp,profile,directory,marker) <- units, marker /= "build-script-build-script-build", let crate = drop 4 marker, validCrate crate, let suffix = reverse (takeWhile (/= '-') (reverse directory)), extension <- [".rlib", ".rmeta", ".so"]]
  incremental = Map.fromListWith (++) [((profile,crate),[fp]) | (fp,profile,_,marker) <- units, marker /= "build-script-build-script-build", let crate = drop 4 marker, validCrate crate]
  lookupOwners relative =
    let components = splitDirectories relative
        (prefix, remainder) = break (== "incremental") components
    in Map.findWithDefault [] relative exact ++ case remainder of
      ["incremental", unit, session, artifact] | "s-" `isPrefixOf` session && artifact `elem` ["dep-graph.bin", "query-cache.bin", "work-products.bin"] ->
        Map.findWithDefault [] (foldl (</>) "" prefix, takeWhile (/= '-') unit) incremental
      _ -> []

snapshot :: Root -> IO Binding
snapshot = scan HashContents

scan :: ScanMode -> Root -> IO Binding
scan mode binding@(Root base) = do
  when (mode == HashContents) $ hPutStrLn stderr ("Hashing Cargo target contents: " ++ base)
  _ <- safeRoot base
  files <- walk base
  let fileSet = Set.fromList files
  let profiles = Set.fromList [takeDirectory relative | relative <- files, takeFileName relative == ".cargo-lock", Set.member (takeDirectory relative </> "deps") fileSet, Set.member (takeDirectory relative </> ".fingerprint") fileSet]
  ensure (not (Set.null profiles)) "no verified Cargo profile structures"
  forM_ (Set.toList profiles) $ \profile -> forM_ [".cargo-lock", "deps", ".fingerprint"] $ \component -> do
    status <- getSymbolicLinkStatus (base </> profile </> component)
    ensure (if component == ".cargo-lock" then isRegularFile status else isDirectory status) "invalid Cargo profile structure"
  fingerprints <- validFingerprints base files
  forM_ fingerprints $ \fp -> ensure (Set.member (takeDirectory (takeDirectory (takeDirectory fp))) profiles) "Cargo fingerprint has no verified profile"
  let owners = artifactOwners fingerprints
  records <- forM files $ \relative -> do
    status <- getSymbolicLinkStatus (base </> relative)
    hash <- if mode == HashContents && isRegularFile status then Just <$> hashFile (base </> relative) else pure Nothing
    let proposed = if isRegularFile status && linkCount status == 1 && not (null (owners relative)) then CompilerArtifact else Retain
    recognized <- if proposed == CompilerArtifact then compilerHeader (base </> relative) else pure True
    pure (Entry (RelativePath relative) (fromIntegral (deviceID status)) (fromIntegral (fileID status)) (OwnerUID (fromIntegral (fileOwner status))) (Bytes (if isRegularFile status then fromIntegral (fileSize status) else 0)) hash (if recognized then proposed else Retain) (if isRegularFile status then RegularFile else Directory))
  let selectedOwners = Set.fromList [owner | entry <- records, disposition entry == CompilerArtifact, let RelativePath relative = path entry, owner <- owners relative]
      selectFingerprint entry = let RelativePath relative = path entry in if Set.member relative selectedOwners then entry { disposition = FingerprintSuccess } else entry
  pure (Binding binding (map selectFingerprint records))

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
  pure (Activity r [pid | (pid, Right True) <- inspected] [issue | (_, Left issue) <- inspected])
 where
  inspect scope uid encoding mappedPrefix pid = do
    let proc = "/proc" </> pid
    let check = do
          status <- getFileStatus proc
          if scope == OwnerProcesses && fileOwner status /= uid then pure False else do
            processStatus <- BC.readFile (proc </> "status")
            if any (\line -> any (`B.isPrefixOf` line) ["State:\tZ", "Kthread:\t1"]) (BC.lines processStatus) then pure False else do
              cwd <- readSymbolicLink (proc </> "cwd")
              exe <- readSymbolicLink (proc </> "exe")
              environment <- B.readFile (proc </> "environ")
              targets <- forM [B.drop 17 field | field <- B.split 0 environment, "CARGO_TARGET_DIR=" `B.isPrefixOf` field] $ \raw -> do
                target <- B.useAsCStringLen raw (GHC.peekCStringLen encoding)
                canonicalizePath (if isAbsolute target then target else cwd </> target)
              fds <- listDirectory (proc </> "fd")
              links <- forM fds $ \fd -> readSymbolicLink (proc </> "fd" </> fd) `catch` \(err :: IOException) -> if isDoesNotExistError err then pure "" else ioError err
              mappings <- BC.lines <$> B.readFile (proc </> "maps")
              let mapped = any (\line -> mappedPrefix `B.isInfixOf` line && mappedPrefix `B.isPrefixOf` mappingPath line) mappings
              pure (mapped || any (inside base) (cwd : exe : targets ++ links))
    (Right <$> check) `catch` \(err :: IOException) -> do
      exists <- doesDirectoryExist proc
      if not exists && isDoesNotExistError err then pure (Right False)
        else pure (Left (InspectionIssue (ProcessId pid) (show err)))
  mappingPath = skip 5
    where
      fieldSpace byte = byte == 32 || byte == 9
      skip :: Int -> B.ByteString -> B.ByteString
      skip 0 line = line
      skip count line = skip (count - 1) (B.dropWhile fieldSpace (B.dropWhile (not . fieldSpace) line))

withLocks :: [Binding] -> IO a -> IO a
withLocks roots action = acquire lockPaths
 where
  lockPaths = nub [base </> relative | Binding (Root base) records <- roots, entry <- records, let RelativePath relative = path entry, takeFileName relative == ".cargo-lock", entryType entry == RegularFile]
  acquire [] = action
  acquire (p:ps) = bracket (openFd p ReadWrite defaultFileFlags { nofollow = True, cloexec = True }) closeFd $ \(Fd fd) -> do
    result <- c_flock fd 6
    ensure (result == 0) "Cargo lock is held"
    acquire ps

writeJson :: ToJSON a => FilePath -> a -> IO ()
writeJson p = BL.writeFile p . encode

readPlan :: FilePath -> IO Plan
readPlan file = do
  bytes <- B.readFile file
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

validateBindings :: Plan -> IO [Root]
validateBindings plan = do
  ensure (schemaVersion plan == 1 && not (null (bindings plan))) "unsupported or empty plan"
  let roots = map root (bindings plan)
  ensure (length roots == length (nub roots)) "duplicate root"
  forM_ [(a,b) | a@(Root pa) <- roots, b@(Root pb) <- roots, a /= b, inside pa pb] $ \_ -> ensure False "overlapping roots"
  forM_ roots $ \(Root base) -> safeRoot base >> pure ()
  pure roots

validatePlan :: Plan -> IO [Binding]
validatePlan plan = do
  roots <- validateBindings plan
  current <- mapM snapshot roots
  ensure (current == bindings plan) "plan inputs changed or plan was tampered with"
  pure current

verifyRemoval :: Root -> Map.Map FilePath Entry -> Entry -> IO ()
verifyRemoval (Root base) identities entry = do
  let RelativePath relative = path entry
      ancestors p = p : if null p || p == "." then [] else ancestors (takeDirectory p)
  forM_ (ancestors (takeDirectory relative)) $ \directory -> do
    let key = if directory == "." then "" else directory
    expected <- maybe (ioError (userError "unbound ancestor")) pure (Map.lookup key identities)
    status <- getSymbolicLinkStatus (base </> key)
    ensure (isDirectory status && fromIntegral (deviceID status) == device expected && fromIntegral (fileID status) == inode expected && OwnerUID (fromIntegral (fileOwner status)) == ownerUID expected) "compiler output ancestor changed"
  status <- getSymbolicLinkStatus (base </> relative)
  ensure (isRegularFile status && linkCount status == 1 && fromIntegral (deviceID status) == device entry && fromIntegral (fileID status) == inode entry && OwnerUID (fromIntegral (fileOwner status)) == ownerUID entry && Bytes (fromIntegral (fileSize status)) == size entry) "compiler output identity changed"

apply :: FilePath -> IO ()
apply file = do
  plan <- readPlan file
  roots <- validateBindings plan
  hPutStrLn stderr "Inspecting target metadata before acquiring Cargo locks"
  metadata <- mapM (scan MetadataOnly) roots
  let withoutHashes binding = binding { entries = [entry { digest = Nothing } | entry <- entries binding] }
  ensure (metadata == map withoutHashes (bindings plan)) "plan metadata changed or plan was tampered with"
  let auditPath = file ++ ".audit.json"
  auditAbsolute <- makeAbsolute auditPath
  forM_ roots $ \(Root base) -> ensure (not (inside base auditAbsolute)) "audit must be outside managed roots"
  exists <- doesPathExist auditPath
  ensure (not exists) "audit already exists"
  hPutStrLn stderr "Acquiring Cargo locks and revalidating planned contents"
  withLocks metadata $ do
    current <- validatePlan plan
    hPutStrLn stderr "Checking active target users"
    users <- mapM (activity . root) current
    ensure (all (\(Activity _ _ unavailable) -> null unavailable) users)
      "active-use inspection is incomplete; no files removed. Obtain an explicitly authorized privileged host scan instead of bypassing process checks"
    ensure (all (\(Activity _ active _) -> null active) users) "active users of Cargo target"
    before <- mapM (space . root) current
    deleted <- newIORef []
    let removalBindings = [(r, records, Map.fromList [(relative, entry) | entry <- records, let RelativePath relative = path entry]) | Binding r records <- current]
        verifyProtected = forM_ current $ \(Binding (Root base) records) -> do
          hPutStrLn stderr ("Verifying protected target contents: " ++ base)
          forM_ records $ \entry -> when (disposition entry == Retain && digest entry /= Nothing) $ do
            let RelativePath relative = path entry
            status <- getSymbolicLinkStatus (base </> relative)
            ensure (isRegularFile status && fromIntegral (deviceID status) == device entry && fromIntegral (fileID status) == inode entry && OwnerUID (fromIntegral (fileOwner status)) == ownerUID entry && Bytes (fromIntegral (fileSize status)) == size entry) "protected file identity changed"
            actual <- hashFile (base </> relative)
            ensure (Just actual == digest entry) "protected file changed"
        persist ok = do
          after <- mapM (space . root) current
          let deltas = [(r, Bytes (a-b)) | ((r,Bytes b),(_,Bytes a)) <- zip before after]
          actualRemoved <- reverse <$> readIORef deleted
          writeJson auditPath (Audit 1 (if ok then Complete else Interrupted) plan actualRemoved before after deltas ok)
    writeJson auditPath (Audit 1 Applying plan [] before before [(r,Bytes 0) | (r,_) <- before] False)
    (do
      hPutStrLn stderr "Removing individually selected compiler artifacts"
      -- Invalidate success markers before removing compiler outputs.
      forM_ [FingerprintSuccess, CompilerArtifact] $ \kind -> forM_ removalBindings $ \(r@(Root base), records, identities) -> forM_ records $ \entry -> when (disposition entry == kind) $ do
        verifyRemoval r identities entry
        let RelativePath relative = path entry
        removeFile (base </> relative)
        modifyIORef' deleted ((r, path entry):)
      verifyProtected
      persist True) `onException` persist False
    B.readFile auditPath >>= B.putStr

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
    BL.putStr (encode (Inventory (map summarize records) free [(r, active) | Activity r active _ <- users] [(r, unavailable) | Activity r _ unavailable <- users]))
  run ("plan":arguments) = case reverse arguments of
    output:"--output":rest -> do
      roots <- parseRoots (reverse rest)
      records <- mapM snapshot roots
      let plan = Plan 1 records
      _ <- validateBindings plan
      absolute <- makeAbsolute output
      forM_ roots $ \(Root base) -> ensure (not (inside base absolute)) "plan must be outside managed roots"
      exists <- doesPathExist output
      ensure (not exists) "plan output already exists"
      hPutStrLn stderr ("Writing exact maintenance plan: " ++ output)
      writeJson output plan
    _ -> usage
  run ["apply", "--plan", file] = apply file
  run _ = usage
  usage = die "usage: storage-maintenance inventory --cargo-target DIR ... | plan --cargo-target DIR ... --output PLAN | apply --plan PLAN"
