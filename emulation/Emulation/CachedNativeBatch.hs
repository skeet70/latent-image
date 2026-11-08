{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | Reuse raw native-runner observations keyed by the ordered case wire and the canonical Nix
-- runner path. A committed entry binds raw runner JSON, its recorded exit and the run's provenance;
-- the current case expectation is re-judged on every read.
module Emulation.CachedNativeBatch
  ( BatchReuse (..)
  , CacheEscape
  , cachedObservedBatch
  ) where

import Control.Exception (Exception, IOException, catch, onException, throwIO, try)
import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isDigit)
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (isPrefixOf, nub)
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
import Emulation.Sha256 (sha256, sha256Hex)
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
import FujiAF.JSON (encodeCanonical)
import System.Directory
  ( canonicalizePath
  , createDirectory
  , createDirectoryIfMissing
  , doesDirectoryExist
  , removeDirectoryRecursive
  , renameDirectory
  )
import System.Exit (ExitCode (..))
import System.FilePath (addTrailingPathSeparator, takeDirectory, (</>))
import System.IO.Error (isDoesNotExistError, tryIOError)
import System.Posix.Files (getSymbolicLinkStatus, isDirectory, isSymbolicLink)
import System.Posix.Process (getProcessID)

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
  ensure
    (withinRoot root resolved)
    ("cache path resolves outside its root: " ++ resolved)
  pure target

canonicalRoot :: FilePath -> IO CacheRoot
canonicalRoot path = do
  createDirectoryIfMissing True path
  CacheRoot <$> canonicalizePath path

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

data CacheRead = CacheAbsent | CacheInvalid String | CacheHit [(CaseId, NativeObservation)]

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

cachedObservedBatch :: FilePath -> FilePath -> FilePath -> IO (BatchReuse, [(CaseId, NativeObservation)])
cachedObservedBatch cacheRoot runner batchPath = do
  root <- canonicalRoot cacheRoot
  (batchSha256, cases) <- prepareCurrent batchPath
  identity <- runnerIdentity runner
  let key = cacheKey identity cases
      executed = do
        temporary <-
          if identityImmutable identity
            then Just <$> createAttempt root key
            else pure Nothing
        let cleanup = maybe (pure ()) removeAttempt temporary
        (observed, manifests) <-
          executeValidated identity batchSha256 batchPath cases temporary `onException` cleanup
        if identityImmutable identity
          then case temporary of
            Just directory -> do
              published <-
                publishAttempt root identity key batchSha256 directory manifests
                  `onException` cleanup
              if published then pure () else cleanup
            Nothing -> pure ()
          else pure ()
        pure (BatchExecuted, observed)
  if not (identityImmutable identity)
    then executed
    else do
      cached <- readCached root identity key cases batchPath
      case cached of
        CacheHit observed -> pure (BatchReused, observed)
        CacheAbsent -> executed
        CacheInvalid _reason -> do
          quarantine root key
          executed

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

executeValidated :: RunnerIdentity -> DigestHex -> FilePath -> [CurrentCase] -> Maybe OwnedPath -> IO ([(CaseId, NativeObservation)], [ManifestCase])
executeValidated identity batchSha256 batchPath cases temporary = do
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
            writeResult batchPath current judged
            modifyIORef' observations ((currentId current, observation) :)
            case temporary of
              Nothing -> pure ()
              Just directory -> do
                caseDirectory <- ownedDirectory directory ["cases", ident]
                let provenance = provenanceValue identity batchSha256 current exit
                    provenanceBytes = encodeCanonical provenance
                B.writeFile (ownedPath caseDirectory </> "runner.json") rawBytes
                B.writeFile (ownedPath caseDirectory </> "exit") (exitBytes exit)
                B.writeFile (ownedPath caseDirectory </> "provenance.json") provenanceBytes
                modifyIORef' manifests (manifestCase current exit rawBytes provenanceBytes :)
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

writeResult :: FilePath -> CurrentCase -> Value -> IO ()
writeResult batchPath current judged =
  B.writeFile
    (takeDirectory batchPath </> (caseIdText (currentId current) ++ ".native-result.json"))
    (encodeCanonical judged)

-- The incomplete report is retained beside the batch in the existing CaseReport format; a failed
-- validation never reaches publishAttempt.
preserveFailureReport :: FilePath -> CaseId -> Value -> IO ()
preserveFailureReport batchPath ident value =
  B.writeFile
    (takeDirectory batchPath </> (caseIdText ident ++ ".native-result.json"))
    (encodeCanonical value)

readCached :: CacheRoot -> RunnerIdentity -> CacheKey -> [CurrentCase] -> FilePath -> IO CacheRead
readCached root identity key cases batchPath = do
  entry <- admitDirectory (rootPath root) ["v1", cacheKeyText key]
  case entry of
    Nothing -> pure CacheAbsent
    Just ownedEntry -> do
      result <- try (validateCached identity key cases ownedEntry batchPath)
      pure $ case result of
        Left (problem :: IOException) -> CacheInvalid (show problem)
        Right observed -> CacheHit observed

validateCached :: RunnerIdentity -> CacheKey -> [CurrentCase] -> OwnedPath -> FilePath -> IO [(CaseId, NativeObservation)]
validateCached identity key cases entry batchPath = do
  encodedManifest <- readRegularWith (pure ()) True manifestLimit (ownedPath entry </> "manifest.json")
  manifest <- parseManifest =<< jsonRead encodedManifest
  ensure (manifestKey manifest == key) "cache manifest key differs"
  ensure
    (manifestRunner manifest == RunnerRecord (identityPath identity) (identitySha256 identity))
    "cache manifest runner differs"
  let items = manifestCases manifest
  ensure (length items == length cases) "cache manifest case count differs"
  let originalIds = map manifestCaseId items
  ensure (length (nub originalIds) == length originalIds) "cache manifest case ids duplicate"
  caseDirectories <-
    forM items $ \item -> do
      admitted <- admitDirectory entry ["cases", caseIdText (manifestCaseId item)]
      maybe (failH "cache case directory is missing") pure admitted
  forM (zip3 cases items caseDirectories) $ \(current, item, caseDirectory) -> do
    ensure
      (manifestCaseWireSha256 item == currentWireSha256 current)
      "cache manifest wire differs"
    exitBytes' <- readRegularWith (pure ()) True 16 (ownedPath caseDirectory </> "exit")
    ensure
      (digestBytes exitBytes' == manifestCaseExitSha256 item)
      "cache recorded runner exit digest differs"
    exit <- either failH pure (parseExit exitBytes')
    ensure (exit == manifestCaseRunnerExit item) "cache recorded runner exit differs from its manifest"
    rawBytes <- readRegularWith (pure ()) True payloadLimit (ownedPath caseDirectory </> "runner.json")
    ensure
      (B.length rawBytes == manifestCaseRawBytes item)
      "cache raw runner observation length differs"
    ensure
      (digestBytes rawBytes == manifestCaseRawSha256 item)
      "cache raw runner observation digest differs"
    raw <- jsonRead rawBytes
    provenanceBytes <- readRegularWith (pure ()) True payloadLimit (ownedPath caseDirectory </> "provenance.json")
    ensure
      (digestBytes provenanceBytes == manifestCaseProvenanceSha256 item)
      "cache provenance digest differs"
    provenance <- parseProvenance =<< jsonRead provenanceBytes
    validateProvenance identity (manifestBatchSha256 manifest) item provenance
    (observation, judged) <- judgeRaw identity current exit raw rawBytes
    writeResult batchPath current judged
    pure (currentId current, observation)

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
  rawBytes <- integer (get value "raw_bytes") 0 (fromIntegral payloadLimit) "cache manifest raw bytes"
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
    `catch` (\(_ :: CacheEscape) -> pure ())

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
          present <- doesDirectoryExist final
          if present then pure False else throwIO problem

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
