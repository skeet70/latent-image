{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | Reuse raw native-runner observations keyed by the ordered case wire and the canonical Nix
-- runner path. A committed entry binds raw runner JSON, its recorded exit and the run's provenance;
-- the current case expectation is re-judged on every read.
module Emulation.CachedNativeBatch
  ( BatchReuse (..)
  , cachedObservedBatch
  ) where

import Control.Exception (IOException, catch, finally, throwIO, try)
import Control.Monad (forM, forM_)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isDigit)
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (isPrefixOf)
import GHC.Clock (getMonotonicTimeNSec)
import Emulation.NativeCase
  ( BatchEntry (..)
  , BatchStatus (..)
  , CaseId (..)
  , CaseReport (..)
  , CaseStatus (..)
  , LoadedCase (..)
  , Region
  , batchCaseId
  , batchCaseStatus
  , batchStatusLabel
  , caseReport
  , judgeRunner
  , loadCaseWith
  , readArtifact
  , readBatch
  , reportRunnerExit
  , runBatch
  , runnerDigest
  , summaryCases
  , summaryFailedCase
  , summaryReason
  , summaryStatus
  )
import Emulation.NativeObservation (NativeObservation, observeNative, renderObservationError)
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
  , num
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
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.Posix.Process (getProcessID)

data BatchReuse = BatchExecuted | BatchReused
  deriving (Eq, Show)

data RunnerIdentity = RunnerIdentity
  { identityPath :: FilePath
  , identitySha256 :: String
  , identityImmutable :: Bool
  }

data CurrentCase = CurrentCase
  { currentId :: CaseId
  , currentValue :: Value
  , currentRegions :: [Region]
  , currentWireSha256 :: String
  , currentIdentity :: [(String, Value)]
  , currentCaseSha256 :: String
  }

data Payload = Payload
  { payloadCase :: CurrentCase
  , payloadRaw :: B.ByteString
  , payloadExit :: ExitCode
  , payloadProvenance :: B.ByteString
  , payloadObservation :: NativeObservation
  , payloadJudged :: Value
  }

data CacheRead = CacheAbsent | CacheInvalid String | CacheHit [Payload]

cacheVersion :: String
cacheVersion = "cached-native-batch-v1"

manifestLimit :: Int
manifestLimit = 64 * 1024 * 1024

payloadLimit :: Int
payloadLimit = 128 * 1024 * 1024

digest :: B.ByteString -> String
digest = sha256Hex . sha256

isDigest :: String -> Bool
isDigest text = length text == 64 && all (\c -> isDigit c || c >= 'a' && c <= 'f') text

exitNumber :: ExitCode -> Integer
exitNumber ExitSuccess = 0
exitNumber (ExitFailure code) = fromIntegral code

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
  createDirectoryIfMissing True cacheRoot
  (batchSha256, cases) <- prepareCurrent batchPath
  identity <- runnerIdentity runner
  let key = cacheKey identity cases
      entry = cacheRoot </> "v1" </> key
      executed = do
        payloads <- executeValidated identity batchSha256 batchPath cases
        if identityImmutable identity
          then do
            _ <- publish cacheRoot identity key batchSha256 payloads
            pure ()
          else pure ()
        writeResults batchPath payloads
        pure (BatchExecuted, observations payloads)
  if not (identityImmutable identity)
    then executed
    else do
      cached <- readCached identity key cases entry
      case cached of
        CacheHit payloads -> do
          writeResults batchPath payloads
          pure (BatchReused, observations payloads)
        CacheAbsent -> executed
        CacheInvalid _reason -> do
          quarantine cacheRoot entry
          executed
  where
    observations payloads = [(currentId (payloadCase payload), payloadObservation payload) | payload <- payloads]

prepareCurrent :: FilePath -> IO (String, [CurrentCase])
prepareCurrent batchPath = do
  (rawBatch, entries) <- readBatch batchPath
  cases <- forM entries $ \entry -> do
    LoadedCase
      { loadedValue = value
      , loadedRegions = regions
      , loadedWire = wire
      , loadedIdentity = identity
      } <- loadCaseWith readArtifact (entryPath entry)
    ensure (get value "id" == s (caseIdText (entryId entry))) "case id differs from its batch entry"
    let wireSha256 = digest wire
        identityValue = obj identity
        caseSha256 = str (get identityValue "case_sha256")
        identityWireSha256 = str (get identityValue "wire_sha256")
    ensure (identityWireSha256 == wireSha256) "case wire digest differs from its identity"
    ensure (isDigest caseSha256) "case SHA256 invalid"
    pure
      CurrentCase
        { currentId = entryId entry
        , currentValue = value
        , currentRegions = map fst regions
        , currentWireSha256 = wireSha256
        , currentIdentity = identity
        , currentCaseSha256 = caseSha256
        }
  pure (digest rawBatch, cases)

runnerIdentity :: FilePath -> IO RunnerIdentity
runnerIdentity runner = do
  path <- canonicalizePath runner
  sha256Value <- runnerDigest path
  ensure (isDigest sha256Value) "native runner digest invalid"
  pure
    RunnerIdentity
      { identityPath = path
      , identitySha256 = sha256Value
      , identityImmutable = "/nix/store/" `isPrefixOf` path
      }

-- Ordered case wires plus the canonical runner path and its bytes. The expected values and
-- checker are intentionally outside the key.
cacheKey :: RunnerIdentity -> [CurrentCase] -> String
cacheKey identity cases =
  digest . BC.pack . unlines $
    [cacheVersion, identityPath identity, identitySha256 identity]
      ++ map currentWireSha256 cases

executeValidated :: RunnerIdentity -> String -> FilePath -> [CurrentCase] -> IO [Payload]
executeValidated identity batchSha256 batchPath cases = do
  collected <- newIORef []
  summary <- runBatch (identityPath identity) batchPath $ \report -> modifyIORef' collected (report :)
  reports <- reverse <$> readIORef collected
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
  ensure
    (length reports == length cases)
    "native batch emitted a different number of reports than admitted cases"
  forM (zip cases reports) $ \(current, report) -> do
    ensure
      (reportCase report == currentId current)
      (caseIdText (currentId current) ++ ": native batch report is out of order")
    let value = reportValue report
        ident = caseIdText (currentId current)
    ensure (get value "runner" /= Null) (ident ++ ": native batch report has no runner observation")
    exit <- case reportRunnerExit value of
      Left problem -> failH (ident ++ ": " ++ renderObservationError problem)
      Right code -> pure code
    let rawBytes = encodeCanonical (get value "runner")
    (observation, judged) <- judgeRaw identity current exit (get value "runner") rawBytes
    pure (makePayload identity batchSha256 current exit rawBytes observation judged)

makePayload :: RunnerIdentity -> String -> CurrentCase -> ExitCode -> B.ByteString -> NativeObservation -> Value -> Payload
makePayload identity batchSha256 current exit rawBytes observation judged =
  Payload
    { payloadCase = current
    , payloadRaw = rawBytes
    , payloadExit = exit
    , payloadProvenance = encodeCanonical provenance
    , payloadObservation = observation
    , payloadJudged = judged
    }
  where
    currentValue' = currentValue current
    identityValue = obj (currentIdentity current)
    provenance =
      obj
        [ ("schema_version", n 1)
        , ("case_id", s (caseIdText (currentId current)))
        , ("case_sha256", s (currentCaseSha256 current))
        , ("wire_sha256", s (currentWireSha256 current))
        , ("origin", get currentValue' "origin")
        , ("provenance", get currentValue' "provenance")
        , ("exclusions", get currentValue' "exclusions")
        , ("artifacts", get identityValue "artifacts")
        , ("runner", obj [("path", s (identityPath identity)), ("sha256", s (identitySha256 identity))])
        , ("batch_sha256", s batchSha256)
        , ("runner_exit", n (exitNumber exit))
        ]

judgeRaw :: RunnerIdentity -> CurrentCase -> ExitCode -> Value -> B.ByteString -> IO (NativeObservation, Value)
judgeRaw identity current exit raw rawBytes = do
  let ident = caseIdText (currentId current)
  observation <- case observeNative exit (currentRegions current) raw of
    Left problem -> failH (ident ++ ": " ++ renderObservationError problem)
    Right observed -> pure observed
  let base =
        set
          "runner_sha256"
          (s (identitySha256 identity))
          (caseReport (currentValue current) (currentIdentity current))
  (_, judged) <- judgeRunner (currentValue current) (currentRegions current) base exit rawBytes B.empty
  let status = str (get judged "status")
      reason = maybe "" ((": " ++) . id) (nonEmpty (str (get judged "reason")))
  ensure
    (status == "passed" || status == "failed")
    (ident ++ ": native runner output is not a complete report" ++ reason)
  pure (observation, judged)
  where
    nonEmpty "" = Nothing
    nonEmpty text = Just text

writeResults :: FilePath -> [Payload] -> IO ()
writeResults batchPath payloads =
  forM_ payloads $ \payload ->
    B.writeFile
      (takeDirectory batchPath </> (caseIdText (currentId (payloadCase payload)) ++ ".native-result.json"))
      (encodeCanonical (payloadJudged payload))

readCached :: RunnerIdentity -> String -> [CurrentCase] -> FilePath -> IO CacheRead
readCached identity key cases entry = do
  exists <- doesDirectoryExist entry
  if not exists
    then pure CacheAbsent
    else do
      result <- try (validateCached identity key cases entry)
      pure $ case result of
        Left (problem :: IOException) -> CacheInvalid (show problem)
        Right payloads -> CacheHit payloads

validateCached :: RunnerIdentity -> String -> [CurrentCase] -> FilePath -> IO [Payload]
validateCached identity key cases entry = do
  manifestBytes <- readRegularWith (pure ()) False manifestLimit (entry </> "manifest.json")
  manifest <- jsonRead manifestBytes
  fields manifest "schema_version kind key batch_sha256 runner cases" "cache manifest"
  voidInteger (get manifest "schema_version") 1 "cache manifest schema version"
  ensure (str (get manifest "kind") == cacheVersion) "cache manifest kind differs"
  ensure (str (get manifest "key") == key) "cache manifest key differs"
  ensure (isDigest (str (get manifest "batch_sha256"))) "cache manifest batch identity invalid"
  let runnerValue = get manifest "runner"
  fields runnerValue "path sha256" "cache manifest runner"
  ensure (str (get runnerValue "path") == identityPath identity) "cache manifest runner path differs"
  ensure (str (get runnerValue "sha256") == identitySha256 identity) "cache manifest runner digest differs"
  let items = list (get manifest "cases")
  ensure (length items == length cases) "cache manifest case count differs"
  forM (zip cases items) $ \(current, item) -> do
    fields
      item
      "id wire_sha256 case_sha256 runner_exit raw_sha256 raw_bytes exit_sha256 provenance_sha256"
      "cache manifest case"
    let ident = caseIdText (currentId current)
    ensure (str (get item "id") == ident) "cache manifest case id differs"
    ensure (str (get item "wire_sha256") == currentWireSha256 current) "cache manifest wire differs"
    ensure (isDigest (str (get item "case_sha256"))) "cache manifest case identity invalid"
    ensure (isDigest (str (get item "raw_sha256"))) "cache manifest raw identity invalid"
    ensure (isDigest (str (get item "exit_sha256"))) "cache manifest exit identity invalid"
    ensure (isDigest (str (get item "provenance_sha256"))) "cache manifest provenance identity invalid"
    let caseDirectory = entry </> "cases" </> ident
    exitBytes <- readRegularWith (pure ()) False 16 (caseDirectory </> "exit")
    ensure (digest exitBytes == str (get item "exit_sha256")) "cache recorded runner exit digest differs"
    exit <- either failH pure (parseExit exitBytes)
    ensure
      (exitNumber exit == num (get item "runner_exit"))
      "cache recorded runner exit differs from its manifest"
    rawBytes <- readRegularWith (pure ()) False payloadLimit (caseDirectory </> "runner.json")
    ensure
      (fromIntegral (B.length rawBytes) == num (get item "raw_bytes"))
      "cache raw runner observation length differs"
    ensure (digest rawBytes == str (get item "raw_sha256")) "cache raw runner observation digest differs"
    raw <- jsonRead rawBytes
    provenanceBytes <- readRegularWith (pure ()) False payloadLimit (caseDirectory </> "provenance.json")
    ensure
      (digest provenanceBytes == str (get item "provenance_sha256"))
      "cache provenance digest differs"
    provenance <- jsonRead provenanceBytes
    validateProvenance identity current exit provenance
    (observation, judged) <- judgeRaw identity current exit raw rawBytes
    pure (makePayloadFromCache current exit rawBytes provenance observation judged)

makePayloadFromCache :: CurrentCase -> ExitCode -> B.ByteString -> Value -> NativeObservation -> Value -> Payload
makePayloadFromCache current exit rawBytes provenance observation judged =
  Payload
    { payloadCase = current
    , payloadRaw = rawBytes
    , payloadExit = exit
    , payloadProvenance = encodeCanonical provenance
    , payloadObservation = observation
    , payloadJudged = judged
    }

validateProvenance :: RunnerIdentity -> CurrentCase -> ExitCode -> Value -> IO ()
validateProvenance identity current exit provenance = do
  fields
    provenance
    "schema_version case_id case_sha256 wire_sha256 origin provenance exclusions artifacts runner batch_sha256 runner_exit"
    "cache provenance"
  voidInteger (get provenance "schema_version") 1 "cache provenance schema version"
  ensure (str (get provenance "case_id") == caseIdText (currentId current)) "cache provenance case id differs"
  ensure (str (get provenance "wire_sha256") == currentWireSha256 current) "cache provenance wire differs"
  ensure (isDigest (str (get provenance "case_sha256"))) "cache provenance case identity invalid"
  ensure (isDigest (str (get provenance "batch_sha256"))) "cache provenance batch identity invalid"
  let runnerValue = get provenance "runner"
  fields runnerValue "path sha256" "cache provenance runner"
  ensure (str (get runnerValue "path") == identityPath identity) "cache provenance runner path differs"
  ensure (str (get runnerValue "sha256") == identitySha256 identity) "cache provenance runner digest differs"
  ensure
    (exitNumber exit == num (get provenance "runner_exit"))
    "cache provenance runner exit differs"

voidInteger :: Value -> Integer -> String -> IO ()
voidInteger value expected label = do
  actual <- integer value expected expected label
  ensure (actual == expected) (label ++ ": unexpected value")

publish :: FilePath -> RunnerIdentity -> String -> String -> [Payload] -> IO Bool
publish cacheRoot identity key batchSha256 payloads = do
  let versionDirectory = cacheRoot </> "v1"
      final = versionDirectory </> key
      temporaryBase = versionDirectory </> "tmp"
  createDirectoryIfMissing True temporaryBase
  temporary <- uniqueDirectory temporaryBase key
  let cleanup = removeDirectoryRecursive temporary `catch` \(_ :: IOException) -> pure ()
  ( do
      forM_ payloads $ \payload -> do
        let caseDirectory = temporary </> "cases" </> caseIdText (currentId (payloadCase payload))
            exitBytes = BC.pack (show (exitNumber (payloadExit payload)))
        createDirectoryIfMissing True caseDirectory
        B.writeFile (caseDirectory </> "runner.json") (payloadRaw payload)
        B.writeFile (caseDirectory </> "exit") exitBytes
        B.writeFile (caseDirectory </> "provenance.json") (payloadProvenance payload)
      B.writeFile (temporary </> "manifest.json") (manifestBytes identity key batchSha256 payloads)
      result <- try (renameDirectory temporary final)
      case result of
        Right () -> pure True
        Left (problem :: IOException) -> do
          existing <- doesDirectoryExist final
          if existing then pure False else throwIO problem
    )
    `finally` cleanup

manifestBytes :: RunnerIdentity -> String -> String -> [Payload] -> B.ByteString
manifestBytes identity key batchSha256 payloads =
  encodeCanonical $
    obj
      [ ("schema_version", n 1)
      , ("kind", s cacheVersion)
      , ("key", s key)
      , ("batch_sha256", s batchSha256)
      , ("runner", obj [("path", s (identityPath identity)), ("sha256", s (identitySha256 identity))])
      , ("cases", arr (map caseEntry payloads))
      ]
  where
    caseEntry payload =
      let current = payloadCase payload
          exitBytes = BC.pack (show (exitNumber (payloadExit payload)))
       in obj
            [ ("id", s (caseIdText (currentId current)))
            , ("wire_sha256", s (currentWireSha256 current))
            , ("case_sha256", s (currentCaseSha256 current))
            , ("runner_exit", n (exitNumber (payloadExit payload)))
            , ("raw_sha256", s (digest (payloadRaw payload)))
            , ("raw_bytes", n (fromIntegral (B.length (payloadRaw payload))))
            , ("exit_sha256", s (digest exitBytes))
            , ("provenance_sha256", s (digest (payloadProvenance payload)))
            ]

uniqueDirectory :: FilePath -> String -> IO FilePath
uniqueDirectory base label = go (0 :: Int)
  where
    go attempt = do
      pid <- getProcessID
      stamp <- getMonotonicTimeNSec
      let path =
            base
              </> ( ".tmp-"
                      ++ take 16 label
                      ++ "-"
                      ++ show pid
                      ++ "-"
                      ++ show stamp
                      ++ "-"
                      ++ show attempt
                  )
      result <- try (createDirectory path)
      case result of
        Right () -> pure path
        Left (problem :: IOException)
          | attempt < 100 -> go (attempt + 1)
          | otherwise -> throwIO problem

quarantine :: FilePath -> FilePath -> IO ()
quarantine cacheRoot entry = do
  let root = cacheRoot </> "v1" </> "quarantine"
  createDirectoryIfMissing True root
  pid <- getProcessID
  stamp <- getMonotonicTimeNSec
  let destination = root </> (takeFileName entry ++ "-" ++ show pid ++ "-" ++ show stamp)
  renameDirectory entry destination `catch` \(_ :: IOException) -> pure ()
