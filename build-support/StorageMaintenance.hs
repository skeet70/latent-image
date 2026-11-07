{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
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
import Data.IORef (modifyIORef', newIORef, readIORef)
import Foreign.C.Types (CInt (..))
import GHC.Generics (Generic)
import Numeric (showHex)
import System.Directory (canonicalizePath, doesDirectoryExist, doesPathExist, listDirectory, makeAbsolute, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), die)
import System.FilePath ((</>), isAbsolute, splitDirectories, takeDirectory, takeExtension, takeFileName)
import System.IO (IOMode (ReadMode), withBinaryFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (deviceID, fileID, fileOwner, fileSize, getFileStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, linkCount, readSymbolicLink)
import System.Posix.IO (OpenFileFlags (cloexec, nofollow), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (getProcessID)
import System.Posix.Types (Fd (..))
import System.Posix.User (getEffectiveUserID)
import System.Process (readProcessWithExitCode)

newtype Root = Root FilePath deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype RelativePath = RelativePath FilePath deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype Bytes = Bytes Integer deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype Digest = Digest String deriving (Eq, Show, Generic, ToJSON, FromJSON)
newtype ProcessId = ProcessId String deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Disposition = Retain | CompilerArtifact | FingerprintSuccess deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Entry = Entry { path :: RelativePath, device :: Integer, inode :: Integer, size :: Bytes, digest :: Maybe Digest, disposition :: Disposition } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Binding = Binding { root :: Root, entries :: [Entry] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Plan = Plan { schemaVersion :: Int, bindings :: [Binding] } deriving (Eq, Show, Generic, ToJSON, FromJSON)
data Inventory = Inventory { inventoryPlan :: Plan, availableBytes :: [(Root, Bytes)], activeUsers :: [(Root, [ProcessId])] } deriving (Generic, ToJSON)
data Audit = Audit { auditSchemaVersion :: Int, appliedPlan :: Plan, removed :: [(Root, RelativePath)], beforeAvailable :: [(Root, Bytes)], afterAvailable :: [(Root, Bytes)], actualAvailableDelta :: [(Root, Bytes)], protectedFilesVerified :: Bool } deriving (Generic, ToJSON)

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
  ensure (canonical == absolute && canonical /= "/") "root must be an absolute, canonical, non-symlink directory"
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
    ensure (not (any (`elem` ["private-data", ".git"]) (splitDirectories relative))) "protected directory within Cargo root"
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
validFingerprints base files = filterM valid [p | p <- files, ".fingerprint" `elem` splitDirectories p, any (`isPrefixOf` takeFileName p) ["lib-", "bin-"] || takeFileName p == "build-script-build-script-build"]
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

classify :: [FilePath] -> FilePath -> Disposition
classify fingerprints p
  | p `elem` fingerprints = FingerprintSuccess
  | otherwise = if any matches fingerprints then CompilerArtifact else Retain
 where
  components = splitDirectories p
  matches fp =
    let parts = splitDirectories fp
        prefix = take (length parts - 3) parts
        directory = takeFileName (takeDirectory fp)
        marker = takeFileName fp
        crate = drop 4 marker
        suffix = reverse (takeWhile (/= '-') (reverse directory))
        libBase = "lib" ++ crate ++ "-" ++ suffix
        validCrate = not (null crate) && all (\c -> isAlphaNum c || c == '_') crate
    in if marker == "build-script-build-script-build"
       then components == prefix ++ ["build", directory, "build-script-build"]
       else validCrate && (components `elem` [prefix ++ ["deps", libBase ++ ext] | ext <- [".rlib", ".rmeta", ".so"]]
            || case drop (length prefix) components of
                 ["incremental", unit, session, artifact] -> (crate ++ "-") `isPrefixOf` unit && "s-" `isPrefixOf` session && artifact `elem` ["dep-graph.bin", "query-cache.bin", "work-products.bin"]
                 _ -> False)

snapshot :: Root -> IO Binding
snapshot binding@(Root base) = do
  _ <- safeRoot base
  files <- walk base
  fingerprints <- validFingerprints base files
  ensure (not (null fingerprints)) "no validated Cargo profile fingerprints"
  forM_ fingerprints $ \fp -> do
    let profile = takeDirectory (takeDirectory (takeDirectory fp))
    ensure ((profile </> ".cargo-lock") `elem` files) "Cargo profile lock is missing"
    deps <- getSymbolicLinkStatus (base </> profile </> "deps")
    ensure (isDirectory deps) "Cargo profile deps directory is missing"
  records <- forM files $ \relative -> do
    status <- getSymbolicLinkStatus (base </> relative)
    hash <- if isRegularFile status then Just <$> hashFile (base </> relative) else pure Nothing
    let proposed = if isRegularFile status && linkCount status == 1 then classify fingerprints relative else Retain
    recognized <- if proposed == CompilerArtifact then compilerHeader (base </> relative) else pure True
    pure (Entry (RelativePath relative) (fromIntegral (deviceID status)) (fromIntegral (fileID status)) (Bytes (if isRegularFile status then fromIntegral (fileSize status) else 0)) hash (if recognized then proposed else Retain))
  pure (Binding binding records)

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

activity :: Root -> IO (Root, [ProcessId])
activity r@(Root base) = do
  uid <- getEffectiveUserID
  self <- show <$> getProcessID
  pids <- filter (\p -> not (null p) && all isDigit p && p /= self) <$> listDirectory "/proc"
  users <- filterM (inspect uid) pids
  pure (r, map ProcessId users)
 where
  inspect uid pid = do
    let proc = "/proc" </> pid
    let check = do
          status <- getFileStatus proc
          if fileOwner status /= uid then pure False else do
            processStatus <- BC.readFile (proc </> "status")
            if any (BC.pack "State:\tZ" `B.isPrefixOf`) (BC.lines processStatus) then pure False else do
              cwd <- readSymbolicLink (proc </> "cwd")
              exe <- readSymbolicLink (proc </> "exe")
              environment <- B.readFile (proc </> "environ")
              targets <- forM [BC.unpack (B.drop 17 field) | field <- B.split 0 environment, BC.pack "CARGO_TARGET_DIR=" `B.isPrefixOf` field] $ \target -> canonicalizePath (if isAbsolute target then target else cwd </> target)
              fds <- listDirectory (proc </> "fd")
              links <- forM fds $ \fd -> readSymbolicLink (proc </> "fd" </> fd) `catch` \(err :: IOException) -> if isDoesNotExistError err then pure "" else ioError err
              pure (any (inside base) (cwd : exe : targets ++ links))
    check `catch` \(err :: IOException) -> do
      exists <- doesDirectoryExist proc
      if not exists && isDoesNotExistError err then pure False else ioError err

withLocks :: [Binding] -> IO a -> IO a
withLocks roots action = acquire lockPaths
 where
  lockPaths = nub [base </> relative | Binding (Root base) records <- roots, Entry (RelativePath relative) _ _ _ _ _ <- records, takeFileName relative == ".cargo-lock"]
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

validatePlan :: Plan -> IO [Binding]
validatePlan plan = do
  ensure (schemaVersion plan == 1 && not (null (bindings plan))) "unsupported or empty plan"
  let roots = map root (bindings plan)
  ensure (length roots == length (nub roots)) "duplicate root"
  forM_ [(a,b) | a@(Root pa) <- roots, b@(Root pb) <- roots, a /= b, inside pa pb] $ \_ -> ensure False "overlapping roots"
  current <- mapM snapshot roots
  ensure (current == bindings plan) "plan inputs changed or plan was tampered with"
  pure current

apply :: FilePath -> IO ()
apply file = do
  plan <- readPlan file
  current <- validatePlan plan
  let auditPath = file ++ ".audit.json"
  auditAbsolute <- makeAbsolute auditPath
  forM_ current $ \(Binding (Root base) _) -> ensure (not (inside base auditAbsolute)) "audit must be outside managed roots"
  exists <- doesPathExist auditPath
  ensure (not exists) "audit already exists"
  withLocks current $ do
    _ <- validatePlan plan
    users <- mapM (activity . root) current
    ensure (all (null . snd) users) "active users of Cargo target"
    _ <- validatePlan plan
    before <- mapM (space . root) current
    deleted <- newIORef []
    let verifyProtected = forM_ current $ \(Binding (Root base) records) -> forM_ records $ \entry -> when (disposition entry == Retain && digest entry /= Nothing) $ do
          let RelativePath relative = path entry
          status <- getSymbolicLinkStatus (base </> relative)
          ensure (isRegularFile status && fromIntegral (deviceID status) == device entry && fromIntegral (fileID status) == inode entry && Bytes (fromIntegral (fileSize status)) == size entry) "protected file identity changed"
          actual <- hashFile (base </> relative)
          ensure (Just actual == digest entry) "protected file changed"
        persist ok = do
          after <- mapM (space . root) current
          let deltas = [(r, Bytes (a-b)) | ((r,Bytes b),(_,Bytes a)) <- zip before after]
          actualRemoved <- reverse <$> readIORef deleted
          writeJson auditPath (Audit 1 plan actualRemoved before after deltas ok)
    writeJson auditPath (Audit 1 plan [] before before [(r,Bytes 0) | (r,_) <- before] False)
    (do
      -- Invalidate success markers before removing compiler outputs.
      forM_ [FingerprintSuccess, CompilerArtifact] $ \kind -> forM_ current $ \(Binding r@(Root base) records) -> forM_ records $ \entry -> when (disposition entry == kind) $ do
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
    records <- mapM snapshot roots
    free <- mapM space roots
    users <- mapM activity roots
    BL.putStr (encode (Inventory (Plan 1 records) free users))
  run ("plan":arguments) = case reverse arguments of
    output:"--output":rest -> do
      roots <- parseRoots (reverse rest)
      records <- mapM snapshot roots
      let plan = Plan 1 records
      _ <- validatePlan plan
      absolute <- makeAbsolute output
      forM_ roots $ \(Root base) -> ensure (not (inside base absolute)) "plan must be outside managed roots"
      exists <- doesPathExist output
      ensure (not exists) "plan output already exists"
      writeJson output plan
    _ -> usage
  run ["apply", "--plan", file] = apply file
  run _ = usage
  usage = die "usage: storage-maintenance inventory --cargo-target DIR ... | plan --cargo-target DIR ... --output PLAN | apply --plan PLAN"
