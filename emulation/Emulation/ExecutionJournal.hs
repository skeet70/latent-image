{-# LANGUAGE OverloadedStrings #-}
-- | Persistent execution evidence for @verify-candidate@.
--
-- A checkpoint records one typed evidence row per selected route and the
-- identity document it was created under. A resume refuses a different
-- identity. The execution ledger is the preserved, external evidence for
-- routes that an earlier coordinator completed: each known exit carries the
-- journal or kernel receipt it came from, and an unknown exit stays unknown.
module Emulation.ExecutionJournal
  ( ExitProvenance (..)
  , JournalReceipt (..)
  , KernelReceipt (..)
  , VerifierReceipt (..)
  , JournalEvidence (..)
  , ExitStatus (..)
  , EvidenceSource (..)
  , JournalRoute (..)
  , Checkpoint (..)
  , CheckpointIdentity (..)
  , IdentityRoute (..)
  , SelectionSource (..)
  , SourceDrift (..)
  , FileDigest (..)
  , ExecutionLedger (..)
  , LedgerEntry (..)
  , StorePath (..)
  , CommitId (..)
  , Seconds (..)
  , UnknownReason (..)
  , storePath
  , commitId
  , readCheckpoint
  , writeCheckpoint
  , readLedger
  , manifestDigest
  , manifestDrift
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Object, ToJSON (..), Value (..), eitherDecodeStrict', encode, object, withObject, withText, (.:), (.:?), (.=))
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit)
import Data.List (isInfixOf, isPrefixOf, isSuffixOf, nub)
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import Emulation.CandidateRoute (Route, parseRoute, routeName)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import System.Directory (createDirectoryIfMissing, doesFileExist, renameFile)
import System.Exit (ExitCode (..))
import System.FilePath (isAbsolute, takeDirectory, (</>))
import Text.Read (readMaybe)

newtype StorePath = StorePath FilePath
  deriving (Eq, Show)

storePath :: FilePath -> Either String StorePath
storePath path
  | ("/nix/store/" :: String) `isPrefixOf` path && length path > length ("/nix/store/" :: String) = Right (StorePath path)
  | otherwise = Left ("store path must be beneath /nix/store/: " ++ path)

instance ToJSON StorePath where
  toJSON (StorePath path) = toJSON path

instance FromJSON StorePath where
  parseJSON = withText "store path" $ \text -> either fail pure (storePath (T.unpack text))

newtype CommitId = CommitId String
  deriving (Eq, Show)

commitId :: String -> Either String CommitId
commitId text
  | not (null text) && length text <= 64 && all isHexDigit text = Right (CommitId text)
  | otherwise = Left ("commit id must be 1..64 hexadecimal digits: " ++ text)

instance ToJSON CommitId where
  toJSON (CommitId text) = toJSON text

instance FromJSON CommitId where
  parseJSON = withText "commit id" $ \text -> either fail pure (commitId (T.unpack text))

newtype Seconds = Seconds Double
  deriving (Eq, Show)

seconds :: Double -> Either String Seconds
seconds value
  | isNaN value || isInfinite value || value < 0 = Left "seconds must be finite and non-negative"
  | otherwise = Right (Seconds value)

instance ToJSON Seconds where
  toJSON (Seconds value) = toJSON value

instance FromJSON Seconds where
  parseJSON value = do
    number <- parseJSON value :: Parser Double
    either fail pure (seconds number)

newtype UnknownReason = UnknownReason String
  deriving (Eq, Show)

instance ToJSON UnknownReason where
  toJSON (UnknownReason reason) = toJSON reason

instance FromJSON UnknownReason where
  parseJSON = withText "unknown exit reason" $ \text ->
    if T.null text then fail "an unknown exit requires a reason" else pure (UnknownReason (T.unpack text))

newtype InvocationId = InvocationId String
  deriving (Eq, Show)

invocationId :: String -> Either String InvocationId
invocationId text
  | length text == 32 && all isHexDigit text = Right (InvocationId text)
  | otherwise = Left ("invocation id must be 32 hexadecimal digits: " ++ text)

instance ToJSON InvocationId where
  toJSON (InvocationId text) = toJSON text

instance FromJSON InvocationId where
  parseJSON = withText "invocation id" $ \text -> either fail pure (invocationId (T.unpack text))

newtype BootId = BootId String
  deriving (Eq, Show)

bootId :: String -> Either String BootId
bootId text
  | length normalized == 32 && all isHexDigit normalized = Right (BootId normalized)
  | otherwise = Left ("boot id must be 32 hexadecimal digits: " ++ text)
  where normalized = filter (/= '-') text

instance ToJSON BootId where
  toJSON (BootId text) = toJSON text

instance FromJSON BootId where
  parseJSON = withText "boot id" $ \text -> either fail pure (bootId (T.unpack text))

newtype JournalCursor = JournalCursor String
  deriving (Eq, Show)

journalCursor :: String -> Either String JournalCursor
journalCursor text
  | null text = Left "journal cursor must not be empty"
  | otherwise = Right (JournalCursor text)

instance ToJSON JournalCursor where
  toJSON (JournalCursor text) = toJSON text

instance FromJSON JournalCursor where
  parseJSON = withText "journal cursor" $ \text -> either fail pure (journalCursor (T.unpack text))

newtype JournalTimestamp = JournalTimestamp String
  deriving (Eq, Show)

journalTimestamp :: String -> Either String JournalTimestamp
journalTimestamp text
  | null text = Left "journal timestamp must not be empty"
  | otherwise = Right (JournalTimestamp text)

instance ToJSON JournalTimestamp where
  toJSON (JournalTimestamp text) = toJSON text

instance FromJSON JournalTimestamp where
  parseJSON = withText "journal timestamp" $ \text -> either fail pure (journalTimestamp (T.unpack text))

newtype JournalUnit = JournalUnit String
  deriving (Eq, Show)

journalUnit :: String -> Either String JournalUnit
journalUnit text
  | null text = Left "journal unit must not be empty"
  | not (".service" `isSuffixOf` text) = Left ("journal unit must name a service: " ++ text)
  | otherwise = Right (JournalUnit text)

instance ToJSON JournalUnit where
  toJSON (JournalUnit text) = toJSON text

instance FromJSON JournalUnit where
  parseJSON = withText "journal unit" $ \text -> either fail pure (journalUnit (T.unpack text))

newtype JournalMessage = JournalMessage String
  deriving (Eq, Show)

journalMessage :: String -> Either String JournalMessage
journalMessage text
  | null text = Left "journal message must not be empty"
  | otherwise = Right (JournalMessage text)

instance ToJSON JournalMessage where
  toJSON (JournalMessage text) = toJSON text

instance FromJSON JournalMessage where
  parseJSON = withText "journal message" $ \text -> either fail pure (journalMessage (T.unpack text))

newtype ProcessId = ProcessId Integer
  deriving (Eq, Show)

processId :: Integer -> Either String ProcessId
processId value
  | value > 0 = Right (ProcessId value)
  | otherwise = Left "process id must be positive"

instance ToJSON ProcessId where
  toJSON (ProcessId value) = toJSON value

instance FromJSON ProcessId where
  parseJSON value = do
    number <- parseJSON value :: Parser Integer
    either fail pure (processId number)

newtype StartTicks = StartTicks Integer
  deriving (Eq, Show)

startTicks :: Integer -> Either String StartTicks
startTicks value
  | value > 0 = Right (StartTicks value)
  | otherwise = Left "start ticks must be positive"

instance ToJSON StartTicks where
  toJSON (StartTicks value) = toJSON value

instance FromJSON StartTicks where
  parseJSON value = do
    number <- parseJSON value :: Parser Integer
    either fail pure (startTicks number)

newtype RawWaitStatus = RawWaitStatus Int
  deriving (Eq, Show)

rawWaitStatus :: Int -> Either String RawWaitStatus
rawWaitStatus value
  | value >= 0 = Right (RawWaitStatus value)
  | otherwise = Left "raw wait status must not be negative"

instance ToJSON RawWaitStatus where
  toJSON (RawWaitStatus value) = toJSON value

instance FromJSON RawWaitStatus where
  parseJSON value = do
    number <- parseJSON value :: Parser Int
    either fail pure (rawWaitStatus number)

newtype RawStat = RawStat String
  deriving (Eq, Show)

rawStat :: String -> Either String RawStat
rawStat text
  | null text = Left "raw stat must not be empty"
  | otherwise = Right (RawStat text)

instance ToJSON RawStat where
  toJSON (RawStat text) = toJSON text

instance FromJSON RawStat where
  parseJSON = withText "raw stat" $ \text -> either fail pure (rawStat (T.unpack text))

newtype RawRecord = RawRecord Value
  deriving (Eq, Show)

instance ToJSON RawRecord where
  toJSON (RawRecord value) = value

instance FromJSON RawRecord where
  parseJSON value = pure (RawRecord value)

newtype ExitStatus = ExitStatus ExitCode
  deriving (Eq, Show)

exitStatus :: Int -> Either String ExitStatus
exitStatus value
  | value == 0 = Right (ExitStatus ExitSuccess)
  | value > 0 && value <= 255 = Right (ExitStatus (ExitFailure value))
  | otherwise = Left ("exit code out of range: " ++ show value)

exitStatusInt :: ExitStatus -> Int
exitStatusInt (ExitStatus ExitSuccess) = 0
exitStatusInt (ExitStatus (ExitFailure value)) = value

instance ToJSON ExitStatus where
  toJSON = toJSON . exitStatusInt

instance FromJSON ExitStatus where
  parseJSON value = do
    number <- parseJSON value :: Parser Int
    either fail pure (exitStatus number)

data JournalReceipt = JournalReceipt
  { journalUnitId :: JournalUnit
  , journalInvocation :: InvocationId
  , journalCursorId :: JournalCursor
  , journalBoot :: BootId
  , journalTimestampId :: JournalTimestamp
  , journalPid :: ProcessId
  , journalMessageBody :: JournalMessage
  , journalRaw :: RawRecord
  } deriving (Eq, Show)

data KernelReceipt = KernelReceipt
  { kernelBoot :: BootId
  , kernelUnit :: JournalUnit
  , kernelInvocation :: InvocationId
  , kernelPid :: ProcessId
  , kernelParent :: ProcessId
  , kernelStartTicks :: StartTicks
  , kernelRawWaitStatus :: RawWaitStatus
  , kernelRawStat :: RawStat
  , kernelRaw :: RawRecord
  } deriving (Eq, Show)

data VerifierReceipt = VerifierReceipt
  { verifierRun :: FilePath
  , verifierLog :: FilePath
  , verifierHostBuild :: Maybe String
  } deriving (Eq, Show)

data ExitProvenance
  = JournalProvenance JournalReceipt
  | KernelProvenance KernelReceipt
  | VerifierProvenance VerifierReceipt
  deriving (Eq, Show)

instance ToJSON ExitProvenance where
  toJSON (JournalProvenance receipt) = object
    [ "kind" .= ("journal" :: T.Text)
    , "unit" .= journalUnitId receipt
    , "invocation" .= journalInvocation receipt
    , "cursor" .= journalCursorId receipt
    , "boot_id" .= journalBoot receipt
    , "timestamp" .= journalTimestampId receipt
    , "pid" .= journalPid receipt
    , "message" .= journalMessageBody receipt
    , "raw" .= journalRaw receipt
    ]
  toJSON (KernelProvenance receipt) = object
    [ "kind" .= ("kernel" :: T.Text)
    , "boot_id" .= kernelBoot receipt
    , "unit" .= kernelUnit receipt
    , "invocation" .= kernelInvocation receipt
    , "pid" .= kernelPid receipt
    , "parent_pid" .= kernelParent receipt
    , "start_ticks" .= kernelStartTicks receipt
    , "raw_wait_status" .= kernelRawWaitStatus receipt
    , "raw_stat" .= kernelRawStat receipt
    , "raw" .= kernelRaw receipt
    ]
  toJSON (VerifierProvenance receipt) = object
    [ "kind" .= ("verifier" :: T.Text)
    , "run" .= verifierRun receipt
    , "log" .= verifierLog receipt
    , "host_build" .= verifierHostBuild receipt
    ]

instance FromJSON ExitProvenance where
  parseJSON = withObject "exit provenance" $ \value -> do
    kind <- value .: "kind"
    case (kind :: T.Text) of
      "journal" -> JournalProvenance <$> journalReceipt value
      "kernel" -> KernelProvenance <$> kernelReceipt value
      "verifier" -> VerifierProvenance <$> verifierReceipt value
      _ -> fail ("unknown exit provenance kind: " ++ T.unpack kind)

journalReceipt :: Object -> Parser JournalReceipt
journalReceipt value =
  JournalReceipt
    <$> value .: "unit"
    <*> value .: "invocation"
    <*> value .: "cursor"
    <*> value .: "boot_id"
    <*> value .: "timestamp"
    <*> value .: "pid"
    <*> value .: "message"
    <*> value .: "raw"

kernelReceipt :: Object -> Parser KernelReceipt
kernelReceipt value =
  KernelReceipt
    <$> value .: "boot_id"
    <*> value .: "unit"
    <*> value .: "invocation"
    <*> value .: "pid"
    <*> value .: "parent_pid"
    <*> value .: "start_ticks"
    <*> value .: "raw_wait_status"
    <*> value .: "raw_stat"
    <*> value .: "raw"

verifierReceipt :: Object -> Parser VerifierReceipt
verifierReceipt value = do
  run <- value .: "run"
  logPath <- value .: "log"
  when (null (run :: FilePath)) (fail "verifier run must not be empty")
  when (null (logPath :: FilePath)) (fail "verifier log must not be empty")
  VerifierReceipt run logPath <$> value .:? "host_build"

data JournalEvidence
  = EvidenceKnown ExitStatus (Maybe Seconds) (Maybe FilePath) (Maybe ExitProvenance)
  | EvidenceUnknown UnknownReason
  deriving (Eq, Show)

instance ToJSON JournalEvidence where
  toJSON (EvidenceKnown exit maybeSeconds maybeLog maybeProvenance) = object $
    [ "kind" .= ("known" :: T.Text), "exit_code" .= exit ]
      ++ maybe [] (\value -> ["seconds" .= value]) maybeSeconds
      ++ maybe [] (\value -> ["log" .= value]) maybeLog
      ++ maybe [] (\value -> ["provenance" .= value]) maybeProvenance
  toJSON (EvidenceUnknown reason) = object
    [ "kind" .= ("unknown" :: T.Text), "reason" .= reason ]

instance FromJSON JournalEvidence where
  parseJSON = withObject "execution evidence" $ \value -> do
    kind <- value .: "kind"
    case (kind :: T.Text) of
      "known" -> EvidenceKnown <$> value .: "exit_code" <*> value .:? "seconds" <*> value .:? "log" <*> value .:? "provenance"
      "unknown" -> EvidenceUnknown <$> value .: "reason"
      _ -> fail ("unknown execution evidence kind: " ++ T.unpack kind)

data SelectionSource = ExecuteSelection | ExistingSelection
  deriving (Eq, Show)

selectionSourceName :: SelectionSource -> String
selectionSourceName ExecuteSelection = "execute"
selectionSourceName ExistingSelection = "existing"

parseSelectionSource :: String -> Either String SelectionSource
parseSelectionSource "execute" = Right ExecuteSelection
parseSelectionSource "existing" = Right ExistingSelection
parseSelectionSource text = Left ("unknown route selection source: " ++ text)

instance ToJSON SelectionSource where
  toJSON = toJSON . selectionSourceName

instance FromJSON SelectionSource where
  parseJSON = withText "route selection source" $ \text -> either fail pure (parseSelectionSource (T.unpack text))

data EvidenceSource = ExistingEvidence | ExecutedEvidence
  deriving (Eq, Show)

evidenceSourceName :: EvidenceSource -> String
evidenceSourceName ExistingEvidence = "existing"
evidenceSourceName ExecutedEvidence = "executed"

parseEvidenceSource :: String -> Either String EvidenceSource
parseEvidenceSource "existing" = Right ExistingEvidence
parseEvidenceSource "executed" = Right ExecutedEvidence
parseEvidenceSource text = Left ("unknown route evidence source: " ++ text)

instance ToJSON EvidenceSource where
  toJSON = toJSON . evidenceSourceName

instance FromJSON EvidenceSource where
  parseJSON = withText "route evidence source" $ \text -> either fail pure (parseEvidenceSource (T.unpack text))

data IdentityRoute = IdentityRoute
  { identityRoute :: Route
  , identityRouteSource :: SelectionSource
  , identityRouteReceipt :: Maybe FilePath
  } deriving (Eq, Show)

instance ToJSON IdentityRoute where
  toJSON route = object
    [ "route" .= routeName (identityRoute route)
    , "source" .= identityRouteSource route
    , "receipt" .= identityRouteReceipt route
    ]

instance FromJSON IdentityRoute where
  parseJSON = withObject "identity route" $ \value -> do
    name <- value .: "route"
    route <- maybe (fail ("unknown identity route: " ++ T.unpack name)) pure (parseRoute (T.unpack name))
    IdentityRoute route <$> value .: "source" <*> value .:? "receipt"

data JournalRoute = JournalRoute
  { journalRoute :: Route
  , journalRouteSource :: EvidenceSource
  , journalRouteReceipt :: FilePath
  , journalRouteEvidence :: JournalEvidence
  } deriving (Eq, Show)

instance ToJSON JournalRoute where
  toJSON route = object
    [ "route" .= routeName (journalRoute route)
    , "source" .= journalRouteSource route
    , "receipt" .= journalRouteReceipt route
    , "evidence" .= journalRouteEvidence route
    ]

instance FromJSON JournalRoute where
  parseJSON = withObject "checkpoint route" $ \value -> do
    name <- value .: "route"
    route <- maybe (fail ("unknown checkpoint route: " ++ T.unpack name)) pure (parseRoute (T.unpack name))
    receipt <- value .: "receipt"
    JournalRoute route <$> value .: "source" <*> pure receipt <*> value .: "evidence"

data SourceDrift = SourceDrift
  { driftPath :: FilePath
  , driftExpected :: Sha256
  , driftCurrent :: Maybe Sha256
  } deriving (Eq)

instance ToJSON SourceDrift where
  toJSON drift = object
    [ "path" .= driftPath drift
    , "expected" .= sha256Hex (driftExpected drift)
    , "current" .= fmap sha256Hex (driftCurrent drift)
    ]

instance FromJSON SourceDrift where
  parseJSON = withObject "source drift" $ \value -> do
    path <- value .: "path"
    expectedText <- value .: "expected"
    expected <- either fail pure (digest expectedText)
    currentText <- value .:? "current"
    current <- case currentText of
      Nothing -> pure Nothing
      Just text -> Just <$> either fail pure (digest text)
    pure (SourceDrift path expected current)
    where digest text = parseSha256Hex (T.unpack (text :: T.Text))

data FileDigest = FileDigest
  { digestPath :: FilePath
  , digestSha256 :: Sha256
  } deriving (Eq)

instance ToJSON FileDigest where
  toJSON digest = object
    ["path" .= digestPath digest, "sha256" .= sha256Hex (digestSha256 digest)]

instance FromJSON FileDigest where
  parseJSON = withObject "file digest" $ \value -> do
    path <- value .: "path"
    text <- value .: "sha256"
    hash <- either fail pure (parseSha256Hex (T.unpack (text :: T.Text)))
    pure (FileDigest path hash)

data CheckpointIdentity = CheckpointIdentity
  { identityWorkspace :: FilePath
  , identityRun :: FilePath
  , identityHead :: Maybe CommitId
  , identitySdk :: StorePath
  , identitySdkSource :: FileDigest
  , identityEngine :: Maybe CommitId
  , identityMatrix :: Bool
  , identityBaselines :: FilePath
  , identityRoutes :: [IdentityRoute]
  , identitySourceManifest :: Maybe FileDigest
  , identitySourceDrift :: [SourceDrift]
  , identityRouteInputManifest :: Maybe FileDigest
  , identityRouteInputDrift :: [SourceDrift]
  , identityBaselineManifest :: Maybe FileDigest
  , identityBaselineDrift :: [SourceDrift]
  , identityExecutionLedger :: Maybe FileDigest
  , identityEarlierReport :: Maybe FileDigest
  , identityHostVerifierSources :: [FileDigest]
  } deriving (Eq)

instance ToJSON CheckpointIdentity where
  toJSON identity = object
    [ "workspace" .= identityWorkspace identity
    , "run" .= identityRun identity
    , "head" .= identityHead identity
    , "sdk" .= identitySdk identity
    , "sdk_source" .= identitySdkSource identity
    , "engine" .= identityEngine identity
    , "matrix" .= identityMatrix identity
    , "baselines" .= identityBaselines identity
    , "routes" .= identityRoutes identity
    , "source_manifest" .= identitySourceManifest identity
    , "source_drift" .= identitySourceDrift identity
    , "route_input_manifest" .= identityRouteInputManifest identity
    , "route_input_drift" .= identityRouteInputDrift identity
    , "baseline_manifest" .= identityBaselineManifest identity
    , "baseline_drift" .= identityBaselineDrift identity
    , "execution_ledger" .= identityExecutionLedger identity
    , "earlier_report" .= identityEarlierReport identity
    , "host_verifier_sources" .= identityHostVerifierSources identity
    ]

instance FromJSON CheckpointIdentity where
  parseJSON = withObject "checkpoint identity" $ \value ->
    CheckpointIdentity
      <$> value .: "workspace"
      <*> value .: "run"
      <*> value .:? "head"
      <*> value .: "sdk"
      <*> value .: "sdk_source"
      <*> value .:? "engine"
      <*> value .: "matrix"
      <*> value .: "baselines"
      <*> value .: "routes"
      <*> value .:? "source_manifest"
      <*> value .: "source_drift"
      <*> value .:? "route_input_manifest"
      <*> value .: "route_input_drift"
      <*> value .:? "baseline_manifest"
      <*> value .: "baseline_drift"
      <*> value .:? "execution_ledger"
      <*> value .:? "earlier_report"
      <*> value .: "host_verifier_sources"

data Checkpoint = Checkpoint
  { checkpointIdentity :: CheckpointIdentity
  , checkpointRoutes :: [JournalRoute]
  } deriving (Eq)

instance ToJSON Checkpoint where
  toJSON checkpoint = object
    [ "schema" .= (2 :: Int)
    , "kind" .= ("verify-candidate-checkpoint" :: T.Text)
    , "identity" .= checkpointIdentity checkpoint
    , "routes" .= checkpointRoutes checkpoint
    ]

instance FromJSON Checkpoint where
  parseJSON = withObject "verify-candidate checkpoint" $ \value -> do
    schema <- value .:? "schema"
    unless (maybe True (== (2 :: Int)) schema) (fail "unsupported checkpoint schema")
    kind <- value .:? "kind"
    unless (maybe True (== ("verify-candidate-checkpoint" :: T.Text)) kind) (fail "unexpected checkpoint kind")
    Checkpoint <$> value .: "identity" <*> value .: "routes"

data LedgerEntry = LedgerEntry
  { entryRoute :: Route
  , entryEvidence :: JournalEvidence
  } deriving (Eq, Show)

instance FromJSON LedgerEntry where
  parseJSON = withObject "execution ledger route" $ \value -> do
    name <- value .: "route"
    route <- maybe (fail ("unknown execution ledger route: " ++ T.unpack name)) pure (parseRoute (T.unpack name))
    LedgerEntry route <$> value .: "evidence"

data ExecutionLedger = ExecutionLedger
  { ledgerRun :: Maybe FilePath
  , ledgerEntries :: [LedgerEntry]
  } deriving (Eq, Show)

instance FromJSON ExecutionLedger where
  parseJSON = withObject "execution ledger" $ \value -> do
    schema <- value .:? "schema"
    unless (maybe True (\number -> number == (1 :: Int) || number == 2) schema) (fail "unsupported execution ledger schema")
    kind <- value .:? "kind"
    unless (maybe True (== ("verify-candidate-execution-ledger" :: T.Text)) kind) (fail "unexpected execution ledger kind")
    ExecutionLedger <$> value .:? "run" <*> value .: "routes"

readCheckpoint :: FilePath -> IO Checkpoint
readCheckpoint path = do
  present <- doesFileExist path
  unless present (ioError (userError ("checkpoint is missing: " ++ path)))
  bytes <- B.readFile path
  checkpoint <- either (ioError . userError . ((("unreadable checkpoint " ++ path ++ ": ") ++))) pure
    (eitherDecodeStrict' bytes >>= parseEither parseJSON)
  let names = map (routeName . journalRoute) (checkpointRoutes checkpoint)
  unless (length names == length (nub names)) (ioError (userError ("checkpoint repeats a route: " ++ path)))
  forM_ (checkpointRoutes checkpoint) $ \route ->
    either (ioError . userError) pure (validateEvidence (journalRoute route) (journalRouteEvidence route))
  pure checkpoint

writeCheckpoint :: FilePath -> Checkpoint -> IO ()
writeCheckpoint path checkpoint = do
  createDirectoryIfMissing True (takeDirectory path)
  let temporary = path ++ ".tmp"
  BL.writeFile temporary (encode checkpoint <> "\n")
  renameFile temporary path

readLedger :: FilePath -> IO ExecutionLedger
readLedger path = do
  present <- doesFileExist path
  unless present (ioError (userError ("execution ledger is missing: " ++ path)))
  bytes <- B.readFile path
  ledger <- either (ioError . userError . ((("unreadable execution ledger " ++ path ++ ": ") ++))) pure
    (eitherDecodeStrict' bytes >>= parseEither parseJSON)
  let names = map (routeName . entryRoute) (ledgerEntries ledger)
  unless (length names == length (nub names)) (ioError (userError ("execution ledger repeats a route: " ++ path)))
  forM_ (ledgerEntries ledger) $ \entry ->
    either (ioError . userError . ((("execution ledger " ++ path ++ ": ") ++))) pure
      (validateEvidence (entryRoute entry) (entryEvidence entry))
  pure ledger

-- | A known exit is only admitted when its provenance agrees with it and with
-- its preserved raw record. An unknown exit carries a reason and no exit.
validateEvidence :: Route -> JournalEvidence -> Either String ()
validateEvidence route evidence = case evidence of
  EvidenceKnown exit maybeSeconds maybeLog maybeProvenance -> do
    case maybeSeconds of
      Just (Seconds value) | isNaN value || isInfinite value || value < 0 -> Left "execution seconds must be finite and non-negative"
      _ -> pure ()
    case maybeLog of
      Just path | null path -> Left "execution log path must not be empty"
      _ -> pure ()
    maybe (Left "a known exit requires provenance") (validateProvenance route exit) maybeProvenance
  EvidenceUnknown _ -> pure ()

validateProvenance :: Route -> ExitStatus -> ExitProvenance -> Either String ()
validateProvenance route exit provenance = case provenance of
  JournalProvenance receipt -> validateJournal route exit receipt
  KernelProvenance receipt -> validateKernel route exit receipt
  VerifierProvenance receipt -> do
    when (null (verifierRun receipt)) (Left "verifier provenance requires a run directory")
    when (null (verifierLog receipt)) (Left "verifier provenance requires a log path")

newtype JournalRaw = JournalRaw (JournalMessage, InvocationId, BootId, JournalCursor, ProcessId, JournalTimestamp, String)

instance FromJSON JournalRaw where
  parseJSON = withObject "journal record" $ \value -> do
    messageText <- value .: "MESSAGE"
    invocationText <- value .: "_SYSTEMD_INVOCATION_ID"
    bootText <- value .: "_BOOT_ID"
    cursorText <- value .: "__CURSOR"
    pidValue <- value .: "_PID" >>= parseNumericText
    timestampText <- value .: "__REALTIME_TIMESTAMP"
    cgroup <- value .: "_SYSTEMD_CGROUP"
    message <- either fail pure (journalMessage (T.unpack messageText))
    invocation <- either fail pure (invocationId (T.unpack invocationText))
    boot <- either fail pure (bootId (T.unpack bootText))
    cursor <- either fail pure (journalCursor (T.unpack cursorText))
    pid <- either fail pure (processId pidValue)
    timestamp <- either fail pure (journalTimestamp (T.unpack timestampText))
    pure (JournalRaw (message, invocation, boot, cursor, pid, timestamp, cgroup))

parseNumericText :: Value -> Parser Integer
parseNumericText value = case value of
  Number _ -> parseJSON value
  String text -> maybe (fail ("expected a decimal number: " ++ T.unpack text)) pure (readMaybe (T.unpack text))
  _ -> fail "expected a decimal number"

validateJournal :: Route -> ExitStatus -> JournalReceipt -> Either String ()
validateJournal route exit receipt = do
  JournalRaw (message, invocation, boot, cursor, pid, timestamp, cgroup) <-
    parseEither parseJSON (let RawRecord value = journalRaw receipt in value)
  unless (message == journalMessageBody receipt) (Left "journal raw message differs from the parsed message")
  unless (invocation == journalInvocation receipt) (Left "journal raw invocation differs from the parsed invocation")
  unless (boot == journalBoot receipt) (Left "journal raw boot id differs from the parsed boot id")
  unless (cursor == journalCursorId receipt) (Left "journal raw cursor differs from the parsed cursor")
  unless (pid == journalPid receipt) (Left "journal raw pid differs from the parsed pid")
  unless (timestamp == journalTimestampId receipt) (Left "journal raw timestamp differs from the parsed timestamp")
  let JournalUnit unit = journalUnitId receipt
  unless (("/" ++ unit) `isSuffixOf` cgroup) (Left "journal cgroup does not name the parsed unit")
  unless (("end " ++ routeName route ++ ": exit " ++ show (exitStatusInt exit) ++ " after ") `isInfixOf` let JournalMessage text = journalMessageBody receipt in text)
    (Left ("journal line does not record " ++ routeName route ++ " exit " ++ show (exitStatusInt exit)))

newtype KernelRecovery = KernelRecovery (Route, ProcessId, ProcessId, StartTicks, BootId, JournalUnit, InvocationId, RawWaitStatus, ExitStatus, String, RawStat)

instance FromJSON KernelRecovery where
  parseJSON = withObject "kernel recovery" $ \value -> do
    routeNameText <- value .: "route"
    route <- maybe (fail ("unknown recovery route: " ++ T.unpack routeNameText)) pure (parseRoute (T.unpack routeNameText))
    pid <- value .: "pid"
    parent <- value .: "parent_pid"
    ticks <- value .: "start_ticks"
    boot <- value .: "boot_id"
    unit <- value .: "unit"
    invocation <- value .: "invocation"
    wait <- value .: "raw_wait_status"
    exit <- value .: "exit_code"
    state <- value .: "process_state"
    stat <- value .: "raw_stat"
    KernelRecovery <$> ((,,,,,,,,,,) <$> pure route <*> pure pid <*> pure parent <*> pure ticks <*> pure boot <*> pure unit
      <*> pure invocation <*> pure wait <*> pure exit <*> pure state <*> pure stat)

validateKernel :: Route -> ExitStatus -> KernelReceipt -> Either String ()
validateKernel route exit receipt = do
  KernelRecovery (recoveryRoute, pid, parent, ticks, boot, unit, invocation, wait, recoveryExit, state, stat) <-
    parseEither parseJSON (let RawRecord value = kernelRaw receipt in value)
  unless (recoveryRoute == route) (Left "kernel recovery route differs from the ledger route")
  unless (pid == kernelPid receipt) (Left "kernel raw pid differs from the parsed pid")
  unless (parent == kernelParent receipt) (Left "kernel raw parent pid differs from the parsed parent")
  unless (ticks == kernelStartTicks receipt) (Left "kernel raw start ticks differ from the parsed start ticks")
  unless (boot == kernelBoot receipt) (Left "kernel raw boot id differs from the parsed boot id")
  unless (unit == kernelUnit receipt) (Left "kernel raw unit differs from the parsed unit")
  unless (invocation == kernelInvocation receipt) (Left "kernel raw invocation differs from the parsed invocation")
  unless (wait == kernelRawWaitStatus receipt) (Left "kernel raw wait status differs from the parsed wait status")
  unless (recoveryExit == exit) (Left "kernel raw exit differs from the ledger exit")
  unless (stat == kernelRawStat receipt) (Left "kernel raw stat differs from the parsed raw stat")
  parsedStat <- parseRawStat (kernelRawStat receipt)
  let (statPid, statParent, statTicks, statState, statWait) = parsedStat
  unless (statPid == pid) (Left "kernel raw stat pid differs from the recovery pid")
  unless (statParent == parent) (Left "kernel raw stat parent differs from the recovery parent")
  unless (statTicks == ticks) (Left "kernel raw stat start ticks differ from the recovery start ticks")
  unless (statState == state) (Left "kernel raw stat state differs from the recovery state")
  unless (terminalKernelState state) (Left ("kernel recovery process state is not terminal: " ++ state))
  unless (terminalKernelState statState) (Left ("kernel raw stat process state is not terminal: " ++ statState))
  unless (statWait == wait) (Left "kernel raw stat wait status differs from the recovery wait status")
  unless (wait == RawWaitStatus (exitStatusInt exit * 256)) (Left "kernel wait status does not encode the ledger exit")

-- | A reaped child is a zombie or dead in /proc; running, sleeping, stopped and
-- other live states cannot establish an exit.
terminalKernelState :: String -> Bool
terminalKernelState state = state `elem` ["Z", "X", "x"]

parseRawStat :: RawStat -> Either String (ProcessId, ProcessId, StartTicks, String, RawWaitStatus)
parseRawStat (RawStat text) = do
  let (pidText, rest) = break (== '(') text
      afterComm = drop 1 (dropWhile (/= ')') rest)
      fields = words afterComm
  pidValue <- maybe (Left "raw stat pid is not a number") Right (readMaybe pidText :: Maybe Integer)
  pid <- processId pidValue
  state <- maybe (Left "raw stat has no state") Right (field fields 0)
  parentValue <- maybe (Left "raw stat parent is not a number") Right (readMaybe (fieldAt fields 1) :: Maybe Integer)
  parent <- processId parentValue
  ticksValue <- maybe (Left "raw stat start ticks are not a number") Right (readMaybe (fieldAt fields 19) :: Maybe Integer)
  ticks <- startTicks ticksValue
  waitValue <- maybe (Left "raw stat wait status is not a number") Right (readMaybe (fieldAt fields 49) :: Maybe Int)
  wait <- rawWaitStatus waitValue
  pure (pid, parent, ticks, state, wait)
  where
    field values index = if index < length values then Just (values !! index) else Nothing
    fieldAt values index = fromMaybe "" (field values index)

manifestDigest :: FilePath -> IO FileDigest
manifestDigest path = do
  bytes <- B.readFile path
  pure (FileDigest path (sha256 bytes))

manifestDrift :: FilePath -> FilePath -> IO [SourceDrift]
manifestDrift root manifest = do
  bytes <- B.readFile manifest
  fmap concat . forM (C.lines bytes) $ \line ->
    if C.null line || C.head line == '#'
      then pure []
      else do
        let (expectedText, rest) = C.break (== ' ') line
            path = C.unpack (C.dropWhile (== ' ') rest)
        expected <- either (ioError . userError . ((manifest ++ ": ") ++)) pure
          (parseSha256Hex (C.unpack expectedText))
        if null path
          then pure []
          else do
            let full = if isAbsolute path then path else root </> path
            present <- doesFileExist full
            current <- if present then Just <$> sha256File full else pure Nothing
            pure (if current == Just expected then [] else [SourceDrift path expected current])
