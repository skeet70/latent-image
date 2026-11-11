{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE ForeignFunctionInterface #-}
-- | Reuse raw native-runner observations keyed by the ordered case wire and the canonical Nix
-- runner path. A committed entry binds raw runner JSON, its recorded exit and the run's provenance;
-- the current case expectation is re-judged on every read.
module Emulation.CachedNativeBatch
  ( defaultNativeCacheRoot
  , BatchReuse (..)
  , CacheEscape
  , ResultPolicy (..)
  , CacheBinding
  , cacheBindingEntry
  , cacheBindingManifestSha256
  , CacheInspection (..)
  , CacheVerdict (..)
  , inspectCachedBatch
  , readCachedPayload
  , CacheOnlyRunner
  , cacheOnlyRunner
  , cachedObservedBatchOnly
  , MaintenanceInterval (..)
  , cachedObservedBatch
  , cachedObservedBatchWith
  , cachedObservedBatchWithSince
  , cachedObservedBatchWithBinding
  ) where

import qualified Codec.Compression.GZip as GZip
import qualified Codec.Compression.Zlib.Internal as Z
import Control.Concurrent (threadDelay)
import Control.Exception (Exception, IOException, bracket, catch, evaluate, onException, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isDigit)
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (isPrefixOf, nub, sortOn)
import Data.Maybe (catMaybes)
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime)
import Foreign.C.Error (eAGAIN, eWOULDBLOCK, getErrno, throwErrno)
import Foreign.C.Types (CInt (..))
import GHC.Clock (getMonotonicTimeNSec)
import Emulation.NativeCase
  ( Admitted (..)
  , BatchStatus (..)
  , CaseId (..)
  , CaseReport (..)
  , CaseStatus (..)
  , DigestHex (..)
  , RegionPlan
  , batchCaseId
  , batchCaseStatus
  , batchStatusLabel
  , judgeRunner
  , memoryProfile
  , profileOutputBytes
  , MemoryProfile (Resource8)
  , materialize
  , prepareBatch
  , readBatch
  , runBatch
  , runnerDigest
  , summaryCases
  , summaryFailedCase
  , summaryReason
  , summaryStatus
  , validId
  )
import Emulation.NativeObservation (NativeObservation, observeNative, renderObservationError, reportRunnerExit)
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import FujiAF.Host
  ( arr
  , ensure
  , failH
  , fields
  , get
  , integer
  , jsonRead
  , list
  , n
  , obj
  , readRegularWith
  , s
  , set
  , str
  )
import FujiAF.StorageBudget (StorageBytes (..), checkStorageBudget, parseByteCount)
import FujiAF.JSON (encodeCanonical)
import System.Directory
  ( canonicalizePath
  , createDirectory
  , createDirectoryIfMissing
  , listDirectory
  , removeDirectoryRecursive
  , renameDirectory
  )
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath (addTrailingPathSeparator, takeDirectory, (</>))
import System.IO.Error (isAlreadyExistsError, isDoesNotExistError, tryIOError)
import System.Posix.Files (fileSize, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, modificationTimeHiRes, setFileTimesHiRes)
import System.Posix.Process (getProcessID)
import System.Posix.IO (FdOption (CloseOnExec), OpenFileFlags (creat), OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd, setFdOption)
import System.Posix.Types (Fd (..))

foreign import ccall unsafe "flock" flock :: CInt -> CInt -> IO CInt

-- | Relative to the workspace root.
defaultNativeCacheRoot :: FilePath
defaultNativeCacheRoot = "private-data/cache/native"

withCacheFlight :: CacheRoot -> CacheKey -> IO a -> IO a
withCacheFlight root key action = do
  directory <- ownedDirectory (rootPath root) ["flights"]
  let path = ownedPath directory </> cacheKeyText key
      acquire = do
        status <- tryIOError (getSymbolicLinkStatus path)
        case status of
          Left problem | isDoesNotExistError problem -> pure ()
          Left problem -> throwIO problem
          Right entry -> when (isSymbolicLink entry) (refuseEscape ("cache lock is a symbolic link: " ++ path))
        fd <- openFd path ReadWrite defaultFileFlags {creat = Just 0o600}
        setFdOption fd CloseOnExec True `onException` closeFd fd
        pure fd
      wait fd@(Fd descriptor) = do
        result <- flock descriptor 6
        if result == 0 then pure () else do
          problem <- getErrno
          if problem == eAGAIN || problem == eWOULDBLOCK
            then threadDelay 10000 >> wait fd
            else throwErrno "native cache flock"
  bracket acquire closeFd $ \fd -> wait fd >> action

data BatchReuse = BatchExecuted | BatchReused
  deriving (Eq, Show)

newtype CacheKey = CacheKey DigestHex
  deriving (Eq, Show)

cacheKeyText :: CacheKey -> String
cacheKeyText (CacheKey key) = digestHex key

-- | The caller-selected cache root, resolved once so an intentional alias (a
-- symlinked cache directory) keeps working while everything below it is owned.
newtype CacheRoot = CacheRoot FilePath

-- | A cache-owned path carrying its canonical root and the relative components
-- already admitted as non-symbolic directories.
data OwnedPath = OwnedPath CacheRoot [FilePath]

ownedPath :: OwnedPath -> FilePath
ownedPath (OwnedPath (CacheRoot base) components) = foldl (</>) base components

ownedCacheRoot :: OwnedPath -> CacheRoot
ownedCacheRoot (OwnedPath root _) = root

ownedRelative :: OwnedPath -> [FilePath]
ownedRelative (OwnedPath _ components) = components

ownedChild :: OwnedPath -> [FilePath] -> OwnedPath
ownedChild (OwnedPath root components) extra = OwnedPath root (components ++ extra)

rootPath :: CacheRoot -> OwnedPath
rootPath root = OwnedPath root []

-- | A cache-owned path that would resolve outside the canonical root. The call
-- refuses rather than following the link, so no outside path is read, written,
-- renamed or quarantined.
newtype CacheEscape = CacheEscape String
  deriving (Show)

instance Exception CacheEscape

refuseEscape :: String -> IO a
refuseEscape = throwIO . CacheEscape

withinRoot :: CacheRoot -> FilePath -> Bool
withinRoot (CacheRoot base) resolved =
  resolved == base || addTrailingPathSeparator base `isPrefixOf` resolved

-- | Check every existing component for a symbolic link. A missing component
-- ends the walk because nothing below it can exist.
admitComponents :: FilePath -> [FilePath] -> IO ()
admitComponents parent = go parent
  where
    go _ [] = pure ()
    go base (component : rest) = do
      let path = base </> component
      status <- tryIOError (getSymbolicLinkStatus path)
      case status of
        Left problem | isDoesNotExistError problem -> pure ()
        Left problem -> throwIO problem
        Right entry -> do
          when (isSymbolicLink entry) (refuseEscape ("cache path is a symbolic link: " ++ path))
          ensure (isDirectory entry) ("cache path is not a directory: " ++ path)
          go path rest

-- | Admit an existing directory below an owned parent. Missing components report
-- absence; a symbolic link refuses the call.
admitDirectory :: OwnedPath -> [FilePath] -> IO (Maybe OwnedPath)
admitDirectory parent components = do
  present <- go (ownedPath (rootPath (ownedCacheRoot parent))) (ownedRelative parent ++ components)
  pure (if present then Just (ownedChild parent components) else Nothing)
  where
    go _ [] = pure True
    go base (component : rest) = do
      let path = base </> component
      status <- tryIOError (getSymbolicLinkStatus path)
      case status of
        Left problem | isDoesNotExistError problem -> pure False
        Left problem -> throwIO problem
        Right entry -> do
          when (isSymbolicLink entry) (refuseEscape ("cache path is a symbolic link: " ++ path))
          ensure (isDirectory entry) ("cache path is not a directory: " ++ path)
          go path rest

-- | Admit or create a directory below an owned parent. Symbolic links are
-- refused before and after creation, and the resolved path must stay under the
-- canonical root.
ownedDirectory :: OwnedPath -> [FilePath] -> IO OwnedPath
ownedDirectory parent components = do
  let root = ownedCacheRoot parent
      full = ownedRelative parent ++ components
  admitComponents (ownedPath (rootPath root)) full
  let target = ownedChild parent components
  createDirectoryIfMissing True (ownedPath target)
  admitComponents (ownedPath (rootPath root)) full
  resolved <- canonicalizePath (ownedPath target)
  when (not (withinRoot root resolved))
    (refuseEscape ("cache path resolves outside its root: " ++ resolved))
  pure target

canonicalRoot :: FilePath -> IO CacheRoot
canonicalRoot path = do
  createDirectoryIfMissing True path
  CacheRoot <$> canonicalizePath path

requireDirectory :: OwnedPath -> [FilePath] -> IO OwnedPath
requireDirectory parent components =
  admitDirectory parent components >>= maybe (failH "cache directory is missing") pure

ownedFilePresent :: OwnedPath -> FilePath -> IO Bool
ownedFilePresent directory name = do
  admitted <- requireDirectory (rootPath (ownedCacheRoot directory)) (ownedRelative directory)
  let path = ownedPath admitted </> name
  status <- tryIOError (getSymbolicLinkStatus path)
  case status of
    Left problem | isDoesNotExistError problem -> pure False
    Left problem -> throwIO problem
    Right entry -> do
      when (isSymbolicLink entry) (refuseEscape ("cache file is a symbolic link: " ++ path))
      pure True

readOwnedRegular :: OwnedPath -> FilePath -> Int -> IO B.ByteString
readOwnedRegular directory name limit = do
  _ <- ownedFilePresent directory name
  readRegularWith (pure ()) True limit (ownedPath directory </> name)

data RunnerIdentity = RunnerIdentity
  { identityPath :: FilePath
  , identitySha256 :: DigestHex
  , identityImmutable :: Bool
  }

data RunnerRecord = RunnerRecord FilePath DigestHex
  deriving (Eq, Show)

data CurrentCase = CurrentCase
  { currentId :: CaseId
  , currentValue :: Value
  , currentRegionPlans :: [RegionPlan]
  , currentWireSha256 :: DigestHex
  , currentCaseSha256 :: DigestHex
  , currentReport :: Value
  }

data CacheRead = CacheAbsent | CacheInvalid String | CacheHit Sha256 [(CaseId, NativeObservation, CacheVerdict)]

data ManifestCase = ManifestCase
  { manifestCaseId :: CaseId
  , manifestCaseWireSha256 :: DigestHex
  , manifestCaseCaseSha256 :: DigestHex
  , manifestCaseRunnerExit :: ExitCode
  , manifestCaseRawSha256 :: DigestHex
  , manifestCaseRawBytes :: Int
  , manifestCaseExitSha256 :: DigestHex
  , manifestCaseProvenanceSha256 :: DigestHex
  }
  deriving (Show)

data CacheManifest = CacheManifest
  { manifestKey :: CacheKey
  , manifestBatchSha256 :: DigestHex
  , manifestRunner :: RunnerRecord
  , manifestCases :: [ManifestCase]
  }
  deriving (Show)

data ProvenanceRecord = ProvenanceRecord
  { provenanceCaseId :: CaseId
  , provenanceCaseSha256 :: DigestHex
  , provenanceWireSha256 :: DigestHex
  , provenanceRunner :: RunnerRecord
  , provenanceBatchSha256 :: DigestHex
  , provenanceRunnerExit :: ExitCode
  }
  deriving (Show)

cacheVersion :: String
cacheVersion = "cached-native-batch-v1"

manifestLimit :: Int
manifestLimit = 64 * 1024 * 1024

payloadLimit :: Int
payloadLimit = 128 * 1024 * 1024

digestBytes :: B.ByteString -> DigestHex
digestBytes bytes = DigestHex (sha256Hex (sha256 bytes))

isDigestText :: String -> Bool
isDigestText text = length text == 64 && all (\c -> isDigit c || c >= 'a' && c <= 'f') text

digestValue :: String -> Value -> IO DigestHex
digestValue label value =
  let text = str value
   in if isDigestText text
        then pure (DigestHex text)
        else failH (label ++ ": SHA256 invalid")

caseIdValue :: String -> Value -> IO CaseId
caseIdValue label value =
  let text = str value
   in if validId (s text)
        then pure (CaseId text)
        else failH (label ++ ": case id invalid")

exitValue :: String -> Value -> IO ExitCode
exitValue label value = do
  number <- integer value 0 255 label
  pure (if number == 0 then ExitSuccess else ExitFailure (fromIntegral number))

exitNumber :: ExitCode -> Integer
exitNumber ExitSuccess = 0
exitNumber (ExitFailure code) = fromIntegral code

exitBytes :: ExitCode -> B.ByteString
exitBytes exit = BC.pack (show (exitNumber exit))

parseExit :: B.ByteString -> Either String ExitCode
parseExit bytes =
  let text = BC.unpack bytes
   in if null text || length text > 3 || any (not . isDigit) text
        then Left "cached native batch: invalid recorded runner exit"
        else case reads text :: [(Int, String)] of
          [(value, "")] | value >= 0 && value <= 255 ->
            Right (if value == 0 then ExitSuccess else ExitFailure value)
          _ -> Left "cached native batch: invalid recorded runner exit"

data ResultPolicy = WriteAllResults | WriteFailedResults deriving (Eq)

data CacheBinding = CacheBinding
  { bindingRoot :: CacheRoot
  , bindingKey :: CacheKey
  , cacheBindingManifestSha256 :: Sha256
  , bindingCurrentIds :: [CaseId]
  }

cacheBindingEntry :: CacheBinding -> FilePath
cacheBindingEntry binding = ownedPath (ownedChild (rootPath (bindingRoot binding)) ["v1", cacheKeyText (bindingKey binding)])

makeBinding :: CacheRoot -> CacheKey -> [CurrentCase] -> IO CacheBinding
makeBinding root key cases = do
  entry <- requireDirectory (rootPath root) ["v1", cacheKeyText key]
  encoded <- readOwnedRegular entry "manifest.json" manifestLimit
  pure (CacheBinding root key (sha256 encoded) (map currentId cases))

data CacheVerdict = CacheVerdict CaseStatus [String]

data CacheInspection
  = InspectionRawHit FilePath CacheBinding [(CaseId, CacheVerdict)]
  | InspectionAbsent FilePath
  | InspectionInvalid FilePath String
  | InspectionIneligible FilePath

newtype MaintenanceInterval = MaintenanceInterval POSIXTime

defaultMaintenanceInterval :: MaintenanceInterval
defaultMaintenanceInterval = MaintenanceInterval 600

cachedObservedBatch :: FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)])
cachedObservedBatch = cachedObservedBatchWith WriteFailedResults

cachedObservedBatchWith :: ResultPolicy -> FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)])
cachedObservedBatchWith = cachedObservedBatchWithSince defaultMaintenanceInterval

cachedObservedBatchWithSince :: MaintenanceInterval -> ResultPolicy -> FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)])
cachedObservedBatchWithSince interval policy cacheRoot runner batchPath = do
  (reuse, observed, _) <- cachedObservedBatchWithBindingSince interval policy cacheRoot runner batchPath
  pure (reuse, observed)

cachedObservedBatchWithBinding :: ResultPolicy -> FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)], Maybe CacheBinding)
cachedObservedBatchWithBinding = cachedObservedBatchWithBindingSince defaultMaintenanceInterval

cachedObservedBatchWithBindingSince :: MaintenanceInterval -> ResultPolicy -> FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)], Maybe CacheBinding)
cachedObservedBatchWithBindingSince interval policy cacheRoot runner batchPath = do
  root <- canonicalRoot cacheRoot
  (batchSha256, cases) <- prepareCurrent batchPath
  identity <- runnerIdentity runner
  let key = cacheKey identity cases
      executed = do
        checkStorageBudget cacheRoot
        checkStorageBudget (takeDirectory batchPath)
        createDirectoryIfMissing True cacheRoot
        limit <-
          if identityImmutable identity
            then Just <$> nativeCacheBudget
            else pure Nothing
        temporary <-
          if identityImmutable identity
            then Just <$> createAttempt root key
            else pure Nothing
        let cleanup = maybe (pure ()) removeAttempt temporary
        (observed, manifests) <-
          executeValidated policy identity batchSha256 batchPath cases temporary `onException` cleanup
        if identityImmutable identity
          then case (temporary, limit) of
            (Just directory, Just budget) -> do
              published <-
                publishAttempt root identity key batchSha256 directory manifests
                  `onException` cleanup
              if published
                then
                  (pruneCacheRoot root interval budget key `catch` \(_ :: IOException) -> pure ())
                    `catch` \(_ :: CacheEscape) -> pure ()
                else cleanup
            _ -> pure ()
          else pure ()
        pure (BatchExecuted, observed)
  if not (identityImmutable identity)
    then do
      (reuse, observed) <- executed
      pure (reuse, observed, Nothing)
    else withCacheFlight root key $ do
      cached <- readCache (PersistResults policy batchPath) root identity key cases
      (reuse, observed, binding) <- case cached of
        CacheHit manifestSha items ->
          pure
            ( BatchReused
            , [(ident, observation) | (ident, observation, _) <- items]
            , Just (CacheBinding root key manifestSha (map currentId cases))
            )
        CacheAbsent -> do
          (reuse, observed) <- executed
          binding <- makeBinding root key cases
          pure (reuse, observed, Just binding)
        CacheInvalid _reason -> do
          quarantine root key
          (reuse, observed) <- executed
          binding <- makeBinding root key cases
          pure (reuse, observed, Just binding)
      pure (reuse, observed, binding)

newtype CacheOnlyRunner = CacheOnlyRunner RunnerIdentity

cacheOnlyRunner :: FilePath -> Sha256 -> IO CacheOnlyRunner
cacheOnlyRunner path digest = do
  canonical <- canonicalizePath path
  ensure (canonical == path && "/nix/store/" `isPrefixOf` canonical) "cache-only runner requires its exact immutable path"
  pure (CacheOnlyRunner (RunnerIdentity canonical (DigestHex (sha256Hex digest)) True))

cachedObservedBatchOnly :: ResultPolicy -> FilePath -> CacheOnlyRunner -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)], Maybe CacheBinding)
cachedObservedBatchOnly policy cacheRoot (CacheOnlyRunner identity) batchPath = do
  root <- CacheRoot <$> canonicalizePath cacheRoot
  (_, cases) <- prepareCurrent batchPath
  let key = cacheKey identity cases
      entry = cacheRoot </> "v1" </> cacheKeyText key
  cached <- readCache (PersistResults policy batchPath) root identity key cases
  case cached of
    CacheHit manifestSha items ->
      pure
        ( BatchReused
        , [(ident, observation) | (ident, observation, _) <- items]
        , Just (CacheBinding root key manifestSha (map currentId cases))
        )
    CacheAbsent -> failH ("cache-only runner observation missing: " ++ entry)
    CacheInvalid reason -> failH ("cache-only runner observation invalid: " ++ reason)

inspectCachedBatch :: FilePath -> FilePath -> FilePath -> IO CacheInspection
inspectCachedBatch cacheRoot runner batchPath = do
  root <- CacheRoot <$> canonicalizePath cacheRoot
  (_, cases) <- prepareCurrent batchPath
  identity <- runnerIdentity runner
  let key = cacheKey identity cases
      entry = cacheRoot </> "v1" </> cacheKeyText key
  if not (identityImmutable identity)
    then pure (InspectionIneligible entry)
    else do
      readResult <- readCache SkipResults root identity key cases
      case readResult of
        CacheAbsent -> pure (InspectionAbsent entry)
        CacheInvalid reason -> pure (InspectionInvalid entry reason)
        CacheHit manifestSha items ->
          let binding = CacheBinding root key manifestSha (map currentId cases)
           in pure
                ( InspectionRawHit
                    (cacheBindingEntry binding)
                    binding
                    [(ident, verdict) | (ident, _, verdict) <- items]
                )

data ValidationDestination
  = SkipResults
  | PersistResults ResultPolicy FilePath

readCache :: ValidationDestination -> CacheRoot -> RunnerIdentity -> CacheKey -> [CurrentCase] -> IO CacheRead
readCache destination root identity key cases = do
  entry <- admitDirectory (rootPath root) ["v1", cacheKeyText key]
  case entry of
    Nothing -> pure CacheAbsent
    Just ownedEntry -> do
      result <- try (validateCachedWith destination identity key cases ownedEntry)
      case result of
        Left (problem :: IOException) -> pure (CacheInvalid (show problem))
        Right (manifestSha, observed) -> do
          case destination of
            SkipResults -> pure ()
            PersistResults _ _ ->
              (touchManifest ownedEntry `catch` \(_ :: IOException) -> pure ())
                `catch` \(_ :: CacheEscape) -> pure ()
          pure (CacheHit manifestSha observed)

touchManifest :: OwnedPath -> IO ()
touchManifest entry = do
  let path = ownedPath entry </> "manifest.json"
  status <- getSymbolicLinkStatus path
  when (isSymbolicLink status) (refuseEscape ("cache file is a symbolic link: " ++ path))
  now <- getPOSIXTime
  setFileTimesHiRes path now now

validateCachedWith :: ValidationDestination -> RunnerIdentity -> CacheKey -> [CurrentCase] -> OwnedPath -> IO (Sha256, [(CaseId, NativeObservation, CacheVerdict)])
validateCachedWith destination identity key cases entry = do
  encodedManifest <- readOwnedRegular entry "manifest.json" manifestLimit
  manifest <- parseManifest =<< jsonRead encodedManifest
  ensure (manifestKey manifest == key) "cache manifest key differs"
  ensure
    (manifestRunner manifest == RunnerRecord (identityPath identity) (identitySha256 identity))
    "cache manifest runner differs"
  let items = manifestCases manifest
  ensure (length items == length cases) "cache manifest case count differs"
  let originalIds = map manifestCaseId items
  ensure (length (nub originalIds) == length originalIds) "cache manifest case ids duplicate"
  validated <- forM (zip cases items) $ \(current, item) -> do
    ensure
      (manifestCaseWireSha256 item == currentWireSha256 current)
      "cache manifest wire differs"
    caseDirectory <- requireDirectory entry ["cases", caseIdText (manifestCaseId item)]
    exitBytes' <- readOwnedRegular caseDirectory "exit" 16
    ensure
      (digestBytes exitBytes' == manifestCaseExitSha256 item)
      "cache recorded runner exit digest differs"
    exit <- either failH pure (parseExit exitBytes')
    ensure (exit == manifestCaseRunnerExit item) "cache recorded runner exit differs from its manifest"
    profile <- memoryProfile (currentValue current)
    ensure (manifestCaseRawBytes item <= profileOutputBytes profile) "cache payload exceeds case memory profile"
    rawBytes <- readRawPayload caseDirectory (manifestCaseRawBytes item)
    ensure
      (B.length rawBytes == manifestCaseRawBytes item)
      "cache raw runner observation length differs"
    ensure
      (digestBytes rawBytes == manifestCaseRawSha256 item)
      "cache raw runner observation digest differs"
    raw <- jsonRead rawBytes
    provenanceBytes <- readOwnedRegular caseDirectory "provenance.json" payloadLimit
    ensure
      (digestBytes provenanceBytes == manifestCaseProvenanceSha256 item)
      "cache provenance digest differs"
    provenance <- parseProvenance =<< jsonRead provenanceBytes
    validateProvenance identity (manifestBatchSha256 manifest) item provenance
    (observation, judged) <- judgeRaw identity current exit raw rawBytes
    case destination of
      SkipResults -> pure ()
      PersistResults policy batchPath -> writeResult policy batchPath current judged
    pure (currentId current, observation, verdictFromJudged judged)
  pure (sha256 encodedManifest, validated)

readCachedPayload :: CacheBinding -> CaseId -> IO B.ByteString
readCachedPayload binding ident = do
  entry <- requireDirectory (rootPath (bindingRoot binding)) ["v1", cacheKeyText (bindingKey binding)]
  encoded <- readOwnedRegular entry "manifest.json" manifestLimit
  ensure (sha256 encoded == cacheBindingManifestSha256 binding) "cache manifest changed"
  manifest <- parseManifest =<< jsonRead encoded
  ensure (manifestKey manifest == bindingKey binding) "cache manifest key differs"
  let items = manifestCases manifest
  ensure (length items == length (bindingCurrentIds binding)) "cache manifest case count differs"
  item <- case [stored | (current, stored) <- zip (bindingCurrentIds binding) items, current == ident] of
    [found] -> pure found
    _ -> failH "cache payload case identity is not unique"
  directory <- requireDirectory entry ["cases", caseIdText (manifestCaseId item)]
  bytes <- readRawPayload directory (manifestCaseRawBytes item)
  ensure (B.length bytes == manifestCaseRawBytes item) "cache raw runner observation length differs"
  ensure (digestBytes bytes == manifestCaseRawSha256 item) "cache raw runner observation digest differs"
  pure bytes

readRawPayload :: OwnedPath -> Int -> IO B.ByteString
readRawPayload directory declared = do
  compressed <- ownedFilePresent directory "runner.json.gz"
  if not compressed
    then readOwnedRegular directory "runner.json" declared
    else do
      legacy <- ownedFilePresent directory "runner.json"
      ensure (not legacy) "cache has ambiguous raw payload encodings"
      encoded <- readOwnedRegular directory "runner.json.gz" (declared + 1024 * 1024)
      decode encoded
  where
    decode encoded = go (Just encoded) 0 []
      (Z.decompressIO Z.gzipFormat Z.defaultDecompressParams
        {Z.decompressBufferSize = 32768, Z.decompressAllMembers = False})
    go pending total chunks stream = case stream of
      Z.DecompressInputRequired next -> case pending of
        Just bytes -> next bytes >>= go Nothing total chunks
        Nothing -> next B.empty >>= finishInput total chunks
      Z.DecompressOutputAvailable chunk next -> do
        ensure (B.length chunk <= declared - total) "cache decoded payload exceeds declared size"
        next >>= go pending (total + B.length chunk) (chunk : chunks)
      Z.DecompressStreamEnd trailing -> do
        ensure (B.null trailing && maybe True B.null pending) "cache compressed payload has trailing bytes"
        ensure (total == declared) "cache decoded payload length differs"
        pure (B.concat (reverse chunks))
      Z.DecompressStreamError problem -> failH ("cache compressed payload invalid: " ++ show problem)
    finishInput total chunks stream = case stream of
      Z.DecompressInputRequired _ -> failH "cache compressed payload truncated"
      _ -> go Nothing total chunks stream

verdictFromJudged :: Value -> CacheVerdict
verdictFromJudged judged = case str (get judged "status") of
  "passed" -> CacheVerdict CasePassed []
  _ -> CacheVerdict CaseFailed [str item | item <- list (get judged "mismatches")]
prepareCurrent :: FilePath -> IO (DigestHex, [CurrentCase])
prepareCurrent batchPath = do
  (rawBatch, entries) <- readBatch batchPath
  prepared <- prepareBatch rawBatch entries
  cases <- forM prepared $ \(ident, result) -> case result of
    Left report ->
      failH
        ( caseIdText ident
            ++ ": native case admission failed: "
            ++ str (get report "status")
            ++ maybe "" ((": " ++) . id) (nonEmpty (str (get report "reason")))
        )
    Right admitted -> do
      ensure (admittedId admitted == ident) "admitted case id differs from its batch entry"
      let report = admittedReport admitted
      wireSha256 <- digestValue "wire_sha256" (get report "wire_sha256")
      caseSha256 <- digestValue "case_sha256" (get report "case_sha256")
      pure
        CurrentCase
          { currentId = ident
          , currentValue = admittedValue admitted
          , currentRegionPlans = admittedRegions admitted
          , currentWireSha256 = wireSha256
          , currentCaseSha256 = caseSha256
          , currentReport = report
          }
  pure (digestBytes rawBatch, cases)
  where
    nonEmpty "" = Nothing
    nonEmpty text = Just text

runnerIdentity :: FilePath -> IO RunnerIdentity
runnerIdentity runner = do
  path <- canonicalizePath runner
  sha256Value <- runnerDigest path
  ensure (isDigestText sha256Value) "native runner digest invalid"
  pure
    RunnerIdentity
      { identityPath = path
      , identitySha256 = DigestHex sha256Value
      , identityImmutable = "/nix/store/" `isPrefixOf` path
      }

-- Ordered case wires plus the canonical runner path and its bytes. The expected values and
-- checker are intentionally outside the key.
cacheKey :: RunnerIdentity -> [CurrentCase] -> CacheKey
cacheKey identity cases =
  CacheKey . digestBytes . BC.pack . unlines $
    [cacheVersion, identityPath identity, digestHex (identitySha256 identity)]
      ++ map (digestHex . currentWireSha256) cases

executeValidated :: ResultPolicy -> RunnerIdentity -> DigestHex -> FilePath -> [CurrentCase] -> Maybe OwnedPath -> IO ([(CaseId, NativeObservation)], [ManifestCase])
executeValidated policy identity batchSha256 batchPath cases temporary = do
  observations <- newIORef []
  manifests <- newIORef []
  remaining <- newIORef cases
  summary <- runBatch (identityPath identity) batchPath $ \report -> do
    pending <- readIORef remaining
    case pending of
      [] -> failH "native batch emitted more reports than admitted cases"
      current : rest -> do
        ensure
          (reportCase report == currentId current)
          (caseIdText (currentId current) ++ ": native batch report is out of order")
        let value = reportValue report
            ident = caseIdText (currentId current)
            validate = do
              ensure
                (str (get value "wire_sha256") == digestHex (currentWireSha256 current))
                (ident ++ ": executed wire identity differs from the admitted case")
              ensure
                (str (get value "case_sha256") == digestHex (currentCaseSha256 current))
                (ident ++ ": executed case identity differs from the admitted case")
              ensure
                (str (get value "runner_sha256") == digestHex (identitySha256 identity))
                (ident ++ ": executed runner identity differs from the effective runner")
              ensure (get value "runner" /= Null) (ident ++ ": native batch report has no runner observation")
              exit <- case reportRunnerExit value of
                Left problem -> failH (ident ++ ": " ++ renderObservationError problem)
                Right code -> pure code
              let rawBytes = encodeCanonical (get value "runner")
              (observation, judged) <- judgeRaw identity current exit (get value "runner") rawBytes
              pure (observation, judged, exit, rawBytes)
        validated <- try validate
        case validated of
          Left (problem :: IOException) -> do
            preserveFailureReport batchPath (currentId current) value
              `catch` \(_ :: IOException) -> pure ()
            throwIO problem
          Right (observation, judged, exit, rawBytes) -> do
            checkStorageBudget (takeDirectory batchPath)
            writeResult policy batchPath current judged
            modifyIORef' observations ((currentId current, observation) :)
            case temporary of
              Nothing -> pure ()
              Just directory -> do
                caseDirectory <- ownedDirectory directory ["cases", ident]
                let provenance = provenanceValue identity batchSha256 current exit
                    provenanceBytes = encodeCanonical provenance
                checkStorageBudget (ownedPath caseDirectory)
                BL.writeFile (ownedPath caseDirectory </> "runner.json.gz")
                  (GZip.compressWith GZip.defaultCompressParams (BL.fromStrict rawBytes))
                B.writeFile (ownedPath caseDirectory </> "exit") (exitBytes exit)
                B.writeFile (ownedPath caseDirectory </> "provenance.json") provenanceBytes
                manifest <- evaluate (forceManifestCase (manifestCase current exit rawBytes provenanceBytes))
                modifyIORef' manifests (manifest :)
            writeIORef remaining rest
  observed <- reverse <$> readIORef observations
  manifested <- reverse <$> readIORef manifests
  unprocessed <- readIORef remaining
  ensure
    (summaryStatus summary == BatchComplete)
    ( "native batch did not complete: "
        ++ batchStatusLabel (summaryStatus summary)
        ++ maybe "" (" at " ++) (caseIdText <$> summaryFailedCase summary)
        ++ maybe "" ((": " ++) . id) (summaryReason summary)
    )
  ensure
    (map (caseIdText . batchCaseId) (summaryCases summary) == map (caseIdText . currentId) cases)
    "native batch summary does not cover the admitted cases in order"
  forM_ (zip cases (summaryCases summary)) $ \(current, batchCase) ->
    ensure
      (batchCaseStatus batchCase == CasePassed || batchCaseStatus batchCase == CaseFailed)
      (caseIdText (currentId current) ++ ": native batch case did not produce a complete report")
  ensure (null unprocessed) "native batch did not report every admitted case"
  ensure (length observed == length cases) "native batch did not observe every admitted case"
  pure (observed, manifested)

provenanceValue :: RunnerIdentity -> DigestHex -> CurrentCase -> ExitCode -> Value
provenanceValue identity batchSha256 current exit =
  obj
    [ ("schema_version", n 1)
    , ("case_id", s (caseIdText (currentId current)))
    , ("case_sha256", s (digestHex (currentCaseSha256 current)))
    , ("wire_sha256", s (digestHex (currentWireSha256 current)))
    , ("origin", get (currentValue current) "origin")
    , ("provenance", get (currentValue current) "provenance")
    , ("exclusions", get (currentValue current) "exclusions")
    , ("artifacts", get (currentReport current) "artifacts")
    , ("runner", obj [("path", s (identityPath identity)), ("sha256", s (digestHex (identitySha256 identity)))])
    , ("batch_sha256", s (digestHex batchSha256))
    , ("runner_exit", n (exitNumber exit))
    ]

manifestCase :: CurrentCase -> ExitCode -> B.ByteString -> B.ByteString -> ManifestCase
manifestCase current exit rawBytes provenanceBytes =
  ManifestCase
    { manifestCaseId = currentId current
    , manifestCaseWireSha256 = currentWireSha256 current
    , manifestCaseCaseSha256 = currentCaseSha256 current
    , manifestCaseRunnerExit = exit
    , manifestCaseRawSha256 = digestBytes rawBytes
    , manifestCaseRawBytes = B.length rawBytes
    , manifestCaseExitSha256 = digestBytes (exitBytes exit)
    , manifestCaseProvenanceSha256 = digestBytes provenanceBytes
    }

forceManifestCase :: ManifestCase -> ManifestCase
forceManifestCase item =
  forceText (caseIdText (manifestCaseId item)) `seq`
    forceDigest (manifestCaseWireSha256 item) `seq`
      forceDigest (manifestCaseCaseSha256 item) `seq`
        exitNumber (manifestCaseRunnerExit item) `seq`
          forceDigest (manifestCaseRawSha256 item) `seq`
            manifestCaseRawBytes item `seq`
              forceDigest (manifestCaseExitSha256 item) `seq`
                forceDigest (manifestCaseProvenanceSha256 item) `seq` item
  where
    forceText = foldr seq ()
    forceDigest = forceText . digestHex

judgeRaw :: RunnerIdentity -> CurrentCase -> ExitCode -> Value -> B.ByteString -> IO (NativeObservation, Value)
judgeRaw identity current exit raw rawBytes = do
  let ident = caseIdText (currentId current)
      regions = map materialize (currentRegionPlans current)
  observation <- case observeNative exit regions raw of
    Left problem -> failH (ident ++ ": " ++ renderObservationError problem)
    Right observed -> pure observed
  let base =
        set
          "runner_sha256"
          (s (digestHex (identitySha256 identity)))
          (currentReport current)
  (_, judged) <- judgeRunner (currentValue current) regions base exit rawBytes B.empty
  let status = str (get judged "status")
      reason = maybe "" ((": " ++) . id) (nonEmpty (str (get judged "reason")))
  ensure
    (status == "passed" || status == "failed")
    (ident ++ ": native runner output is not a complete report" ++ reason)
  pure (observation, judged)
  where
    nonEmpty "" = Nothing
    nonEmpty text = Just text

writeResult :: ResultPolicy -> FilePath -> CurrentCase -> Value -> IO ()
writeResult policy batchPath current judged =
  when
    (policy == WriteAllResults || str (get judged "status") == "failed")
    (B.writeFile
      (takeDirectory batchPath </> (caseIdText (currentId current) ++ ".native-result.json"))
      (encodeCanonical judged))
-- The incomplete report is retained beside the batch in the existing CaseReport format; a failed
-- validation never reaches publishAttempt.
preserveFailureReport :: FilePath -> CaseId -> Value -> IO ()
preserveFailureReport batchPath ident value =
  B.writeFile
    (takeDirectory batchPath </> (caseIdText ident ++ ".native-result.json"))
    (encodeCanonical value)


parseManifest :: Value -> IO CacheManifest
parseManifest value = do
  fields value "schema_version kind key batch_sha256 runner cases" "cache manifest"
  voidInteger (get value "schema_version") 1 "cache manifest schema version"
  ensure (str (get value "kind") == cacheVersion) "cache manifest kind differs"
  key <- CacheKey <$> digestValue "cache manifest key" (get value "key")
  batchSha256 <- digestValue "cache manifest batch" (get value "batch_sha256")
  runner <- parseRunnerRecord (get value "runner")
  let items = list (get value "cases")
  ensure (not (null items)) "cache manifest has no cases"
  cases <- mapM parseManifestCase items
  pure
    CacheManifest
      { manifestKey = key
      , manifestBatchSha256 = batchSha256
      , manifestRunner = runner
      , manifestCases = cases
      }

parseManifestCase :: Value -> IO ManifestCase
parseManifestCase value = do
  fields
    value
    "id wire_sha256 case_sha256 runner_exit raw_sha256 raw_bytes exit_sha256 provenance_sha256"
    "cache manifest case"
  ident <- caseIdValue "cache manifest case id" (get value "id")
  wireSha256 <- digestValue "cache manifest case wire" (get value "wire_sha256")
  caseSha256 <- digestValue "cache manifest case identity" (get value "case_sha256")
  runnerExit <- exitValue "cache manifest runner exit" (get value "runner_exit")
  rawSha256 <- digestValue "cache manifest raw" (get value "raw_sha256")
  rawBytes <- integer (get value "raw_bytes") 0 (fromIntegral (profileOutputBytes Resource8)) "cache manifest raw bytes"
  exitSha256 <- digestValue "cache manifest exit" (get value "exit_sha256")
  provenanceSha256 <- digestValue "cache manifest provenance" (get value "provenance_sha256")
  pure
    ManifestCase
      { manifestCaseId = ident
      , manifestCaseWireSha256 = wireSha256
      , manifestCaseCaseSha256 = caseSha256
      , manifestCaseRunnerExit = runnerExit
      , manifestCaseRawSha256 = rawSha256
      , manifestCaseRawBytes = fromIntegral rawBytes
      , manifestCaseExitSha256 = exitSha256
      , manifestCaseProvenanceSha256 = provenanceSha256
      }

parseRunnerRecord :: Value -> IO RunnerRecord
parseRunnerRecord value = do
  fields value "path sha256" "cache runner"
  let path = str (get value "path")
  ensure (not (null path)) "cache runner path invalid"
  sha256Value <- digestValue "cache runner" (get value "sha256")
  pure (RunnerRecord path sha256Value)

parseProvenance :: Value -> IO ProvenanceRecord
parseProvenance value = do
  fields
    value
    "schema_version case_id case_sha256 wire_sha256 origin provenance exclusions artifacts runner batch_sha256 runner_exit"
    "cache provenance"
  voidInteger (get value "schema_version") 1 "cache provenance schema version"
  ident <- caseIdValue "cache provenance case id" (get value "case_id")
  caseSha256 <- digestValue "cache provenance case hash" (get value "case_sha256")
  wireSha256 <- digestValue "cache provenance wire" (get value "wire_sha256")
  runner <- parseRunnerRecord (get value "runner")
  batchSha256 <- digestValue "cache provenance batch" (get value "batch_sha256")
  runnerExit <- exitValue "cache provenance runner exit" (get value "runner_exit")
  pure
    ProvenanceRecord
      { provenanceCaseId = ident
      , provenanceCaseSha256 = caseSha256
      , provenanceWireSha256 = wireSha256
      , provenanceRunner = runner
      , provenanceBatchSha256 = batchSha256
      , provenanceRunnerExit = runnerExit
      }

validateProvenance :: RunnerIdentity -> DigestHex -> ManifestCase -> ProvenanceRecord -> IO ()
validateProvenance identity batchSha256 item provenance = do
  ensure (provenanceCaseId provenance == manifestCaseId item) "cache provenance case id differs"
  ensure
    (provenanceCaseSha256 provenance == manifestCaseCaseSha256 item)
    "cache provenance case hash differs"
  ensure
    (provenanceWireSha256 provenance == manifestCaseWireSha256 item)
    "cache provenance wire differs"
  ensure
    (provenanceRunner provenance == RunnerRecord (identityPath identity) (identitySha256 identity))
    "cache provenance runner differs"
  ensure
    (provenanceBatchSha256 provenance == batchSha256)
    "cache provenance batch differs from its manifest"
  ensure
    (provenanceRunnerExit provenance == manifestCaseRunnerExit item)
    "cache provenance runner exit differs"

voidInteger :: Value -> Integer -> String -> IO ()
voidInteger value expected label = do
  actual <- integer value expected expected label
  ensure (actual == expected) (label ++ ": unexpected value")

createAttempt :: CacheRoot -> CacheKey -> IO OwnedPath
createAttempt root key = do
  base <- ownedDirectory (rootPath root) ["v1", "tmp"]
  uniqueDirectory base (cacheKeyText key)

removeAttempt :: OwnedPath -> IO ()
removeAttempt directory =
  ( do
      admitted <- admitDirectory (rootPath (ownedCacheRoot directory)) (ownedRelative directory)
      case admitted of
        Just owned -> removeDirectoryRecursive (ownedPath owned)
        Nothing -> pure ()
  )
    `catch` (\(_ :: IOException) -> pure ())

publishAttempt :: CacheRoot -> RunnerIdentity -> CacheKey -> DigestHex -> OwnedPath -> [ManifestCase] -> IO Bool
publishAttempt root identity key batchSha256 temporary manifests = do
  attempt <-
    admitDirectory (rootPath (ownedCacheRoot temporary)) (ownedRelative temporary)
      >>= maybe (failH "cache attempt directory is missing") pure
  B.writeFile (ownedPath attempt </> "manifest.json") (manifestBytes identity key batchSha256 manifests)
  existing <- admitDirectory (rootPath root) ["v1", cacheKeyText key]
  case existing of
    Just _ -> pure False
    Nothing -> do
      let final = ownedPath (rootPath root) </> "v1" </> cacheKeyText key
      result <- try (renameDirectory (ownedPath attempt) final)
      case result of
        Right () -> pure True
        Left (problem :: IOException) -> do
          present <- admitDirectory (rootPath root) ["v1", cacheKeyText key]
          case present of
            Just _ -> pure False
            Nothing -> throwIO problem

manifestBytes :: RunnerIdentity -> CacheKey -> DigestHex -> [ManifestCase] -> B.ByteString
manifestBytes identity key batchSha256 manifests =
  encodeCanonical $
    obj
      [ ("schema_version", n 1)
      , ("kind", s cacheVersion)
      , ("key", s (cacheKeyText key))
      , ("batch_sha256", s (digestHex batchSha256))
      , ("runner", obj [("path", s (identityPath identity)), ("sha256", s (digestHex (identitySha256 identity)))])
      , ("cases", arr (map caseEntry manifests))
      ]
  where
    caseEntry item =
      obj
        [ ("id", s (caseIdText (manifestCaseId item)))
        , ("wire_sha256", s (digestHex (manifestCaseWireSha256 item)))
        , ("case_sha256", s (digestHex (manifestCaseCaseSha256 item)))
        , ("runner_exit", n (exitNumber (manifestCaseRunnerExit item)))
        , ("raw_sha256", s (digestHex (manifestCaseRawSha256 item)))
        , ("raw_bytes", n (fromIntegral (manifestCaseRawBytes item)))
        , ("exit_sha256", s (digestHex (manifestCaseExitSha256 item)))
        , ("provenance_sha256", s (digestHex (manifestCaseProvenanceSha256 item)))
        ]

uniqueDirectory :: OwnedPath -> String -> IO OwnedPath
uniqueDirectory base label = go (0 :: Int)
  where
    go attempt = do
      pid <- getProcessID
      stamp <- getMonotonicTimeNSec
      let name =
            ".tmp-"
              ++ take 16 label
              ++ "-"
              ++ show pid
              ++ "-"
              ++ show stamp
              ++ "-"
              ++ show attempt
      result <- try (createDirectory (ownedPath base </> name))
      case result of
        Right () -> pure (ownedChild base [name])
        Left (problem :: IOException)
          | attempt < 100 -> go (attempt + 1)
          | otherwise -> throwIO problem

quarantine :: CacheRoot -> CacheKey -> IO ()
quarantine root key = do
  destinationRoot <- ownedDirectory (rootPath root) ["v1", "quarantine"]
  entry <- admitDirectory (rootPath root) ["v1", cacheKeyText key]
  case entry of
    Nothing -> pure ()
    Just ownedEntry -> do
      pid <- getProcessID
      stamp <- getMonotonicTimeNSec
      let destination =
            ownedPath destinationRoot
              </> (cacheKeyText key ++ "-" ++ show pid ++ "-" ++ show stamp)
      renameDirectory (ownedPath ownedEntry) destination `catch` \(_ :: IOException) -> pure ()

nativeCacheBudget :: IO StorageBytes
nativeCacheBudget = do
  raw <- maybe "20G" id <$> lookupEnv "XM1_NATIVE_CACHE_MAX"
  either
    (\input -> failH ("XM1_NATIVE_CACHE_MAX is not a positive byte count: " ++ show input))
    pure
    (parseByteCount raw)

data CacheEntry = CacheEntry
  { prunedName :: FilePath
  , prunedBytes :: StorageBytes
  , prunedModified :: POSIXTime
  }

zeroBytes :: StorageBytes
zeroBytes = StorageBytes 0

addBytes :: StorageBytes -> StorageBytes -> StorageBytes
addBytes (StorageBytes left) (StorageBytes right) = StorageBytes (left + right)

subtractBytes :: StorageBytes -> StorageBytes -> StorageBytes
subtractBytes (StorageBytes left) (StorageBytes right) = StorageBytes (left - right)

pruneCacheRoot :: CacheRoot -> MaintenanceInterval -> StorageBytes -> CacheKey -> IO ()
pruneCacheRoot root interval budget publishedKey = do
  version <- ownedDirectory (rootPath root) ["v1"]
  fd@(Fd descriptor) <- openPruneLock version
  bracket (pure fd) closeFd $ \_ -> do
    result <- flock descriptor 6
    if result /= 0
      then do
        problem <- getErrno
        if problem == eAGAIN || problem == eWOULDBLOCK
          then pure ()
          else throwErrno "native cache prune flock"
      else do
        recent <- stampRecent version interval
        unless recent $ do
          entries <- prunedEntries version
          evictEntries
            version
            budget
            (foldr (addBytes . prunedBytes) zeroBytes entries)
            [ item
            | item <- sortOn (\entry -> (prunedModified entry, prunedName entry)) entries
            , prunedName item /= cacheKeyText publishedKey
            ]
          writeStamp version

openPruneLock :: OwnedPath -> IO Fd
openPruneLock version = do
  let path = ownedPath version </> ".prune-lock"
  status <- tryIOError (getSymbolicLinkStatus path)
  case status of
    Left problem | isDoesNotExistError problem -> pure ()
    Left problem -> throwIO problem
    Right entry -> when (isSymbolicLink entry) (refuseEscape ("cache lock is a symbolic link: " ++ path))
  fd <- openFd path ReadWrite defaultFileFlags {creat = Just 0o600}
  setFdOption fd CloseOnExec True `onException` closeFd fd
  pure fd

stampRecent :: OwnedPath -> MaintenanceInterval -> IO Bool
stampRecent version (MaintenanceInterval interval)
  | interval <= 0 = pure False
  | otherwise = do
      let path = ownedPath version </> ".prune-stamp"
      now <- getPOSIXTime
      status <- tryIOError (getSymbolicLinkStatus path)
      case status of
        Left problem | isDoesNotExistError problem -> pure False
        Left problem -> throwIO problem
        Right entry -> do
          when (isSymbolicLink entry) (refuseEscape ("cache file is a symbolic link: " ++ path))
          pure (now - modificationTimeHiRes entry < interval)

writeStamp :: OwnedPath -> IO ()
writeStamp version = do
  let path = ownedPath version </> ".prune-stamp"
  status <- tryIOError (getSymbolicLinkStatus path)
  case status of
    Left problem | isDoesNotExistError problem -> pure ()
    Left problem -> throwIO problem
    Right entry -> when (isSymbolicLink entry) (refuseEscape ("cache file is a symbolic link: " ++ path))
  B.writeFile path B.empty
  now <- getPOSIXTime
  setFileTimesHiRes path now now

prunedEntries :: OwnedPath -> IO [CacheEntry]
prunedEntries version = do
  names <- listDirectory (ownedPath version)
  fmap catMaybes . forM names $ \name ->
    if name == "tmp" || name == "quarantine" || "." `isPrefixOf` name
      then pure Nothing
      else do
        let path = ownedPath version </> name
        status <- tryIOError (getSymbolicLinkStatus path)
        case status of
          Right entry | isDirectory entry -> do
            manifest <- tryIOError (getSymbolicLinkStatus (path </> "manifest.json"))
            case manifest of
              Right found | isRegularFile found && not (isSymbolicLink found) -> do
                bytes <- measureEntry path
                pure (Just (CacheEntry name bytes (modificationTimeHiRes found)))
              _ -> pure Nothing
          _ -> pure Nothing

measureEntry :: FilePath -> IO StorageBytes
measureEntry path = do
  status <- tryIOError (getSymbolicLinkStatus path)
  case status of
    Left _ -> pure zeroBytes
    Right entry
      | isDirectory entry -> do
          names <- tryIOError (listDirectory path)
          case names of
            Left _ -> pure zeroBytes
            Right children -> foldr addBytes zeroBytes <$> mapM (measureEntry . (path </>)) children
      | isRegularFile entry -> pure (StorageBytes (fromIntegral (fileSize entry)))
      | otherwise -> pure zeroBytes

evictEntries :: OwnedPath -> StorageBytes -> StorageBytes -> [CacheEntry] -> IO ()
evictEntries version budget total candidates
  | total <= budget = pure ()
  | otherwise = case candidates of
      [] -> pure ()
      candidate : rest -> do
        moved <- moveEntryToTmp version (prunedName candidate)
        case moved of
          Just destination -> removeDirectoryRecursive destination `catch` \(_ :: IOException) -> pure ()
          Nothing -> pure ()
        let remaining = case moved of
              Just _ -> subtractBytes total (prunedBytes candidate)
              Nothing -> total
        evictEntries version budget remaining rest

moveEntryToTmp :: OwnedPath -> FilePath -> IO (Maybe FilePath)
moveEntryToTmp version name = do
  temporary <- ownedDirectory version ["tmp"]
  pid <- getProcessID
  stamp <- getMonotonicTimeNSec
  let attempt index = do
        let destination =
              ownedPath temporary
                </> (name ++ "-" ++ show pid ++ "-" ++ show stamp ++ "-" ++ show index)
        result <- tryIOError (renameDirectory (ownedPath version </> name) destination)
        case result of
          Right () -> pure (Just destination)
          Left problem
            | isAlreadyExistsError problem && index < 100 -> attempt (index + 1)
            | otherwise -> pure Nothing
  attempt (0 :: Int)
