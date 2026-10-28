{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Packaged qualification for the retained row-availability service.
module Replacement.Service.Packaged
  ( PackagedInputs (..)
  , PackagedStatus (..)
  , PackagedReport (..)
  , PackagedRow (..)
  , OwnedSequence (..)
  , rowQualified
  , qualifiedRowCount
  , qualifyPackaged
  , packagedStatusLabel
  , packagedReportValue
  ) where

import Control.Exception (IOException, try)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', withObject, withText, (.:), (.:?), (.!=))
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (find, findIndex, nub)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import Emulation.Symbols (SymbolTable, parseSymbolTable, resolveSymbol)
import Emulation.SegmentReceipt (SegmentName, segmentName, segmentNameText)
import Emulation.WatchObservation (WatchJudgement (..))
import FujiAF.Host (arr, b, n, obj, s)
import FujiAF.RamPush (RamPushManifest (..), readRamPushManifest)
import Numeric (readHex, showHex)
import Replacement.Service.Qualify
  ( ServiceRow (..)
  , serviceContractSha256
  , serviceContractValue
  , serviceRowExpected
  , serviceRowProperty
  , serviceRowToken
  , serviceRows
  , serviceStockTarget
  , rowStateWord
  )
import System.Directory (doesFileExist)
import System.FilePath (isAbsolute, (</>), takeFileName)

newtype Digest = Digest { digestText :: String } deriving (Eq, Ord, Show)

instance FromJSON Digest where
  parseJSON = withText "digest" (either fail pure . parseDigest . T.unpack)

parseDigest :: String -> Either String Digest
parseDigest text
  | length text == 64 && all (`elem` ("0123456789abcdef" :: String)) text = Right (Digest text)
  | otherwise = Left "digest must be 64 lowercase hex digits"

trustedDigest :: String -> Digest
trustedDigest = Digest

data TraceSiteLabel = SvcRustEntry | SvcStockTrampoline | SvcStockReturn | StockRowProbe
  deriving (Bounded, Enum, Eq, Ord, Show)

traceSiteLabelText :: TraceSiteLabel -> String
traceSiteLabelText SvcRustEntry = "svc-rust-entry"
traceSiteLabelText SvcStockTrampoline = "svc-stock-trampoline"
traceSiteLabelText SvcStockReturn = "svc-stock-return"
traceSiteLabelText StockRowProbe = "stock-row-probe"

instance FromJSON TraceSiteLabel where
  parseJSON = withText "trace label" $ \label -> case T.unpack label of
    "svc-rust-entry" -> pure SvcRustEntry
    "svc-stock-trampoline" -> pure SvcStockTrampoline
    "svc-stock-return" -> pure SvcStockReturn
    "stock-row-probe" -> pure StockRowProbe
    other -> fail ("unknown service trace label: " ++ other)

data CoreId = Core0 | Core1 deriving (Eq, Ord, Show)

coreIdValue :: CoreId -> Int
coreIdValue Core0 = 0
coreIdValue Core1 = 1

instance FromJSON CoreId where
  parseJSON value = do
    core <- parseJSON value :: Parser Int
    case core of
      0 -> pure Core0
      1 -> pure Core1
      _ -> fail "trace core must be 0 or 1"

newtype InstructionCount = InstructionCount { instructionCountValue :: Word64 } deriving (Eq, Ord, Show)

instance FromJSON InstructionCount where
  parseJSON = fmap InstructionCount . parseJSON

newtype TraceAddress = TraceAddress { traceAddressWord :: Word32 } deriving (Eq, Ord, Show)

instance FromJSON TraceAddress where
  parseJSON = withText "trace address" parseAddress8

data DeclaredTraceSite = DeclaredTraceSite
  { declaredTraceLabel :: TraceSiteLabel
  , declaredTraceAddress :: TraceAddress
  } deriving (Eq, Show)

instance FromJSON DeclaredTraceSite where
  parseJSON = withObject "declared trace site" $ \site ->
    DeclaredTraceSite
      <$> site .: "label"
      <*> site .: "address"

data TraceHit = TraceHit
  { hitSite :: TraceSiteLabel
  , hitCore :: CoreId
  , hitInstructions :: InstructionCount
  , hitR0, hitR1, hitR2, hitR3 :: Word32
  } deriving (Eq, Show)

instance FromJSON TraceHit where
  parseJSON = withObject "trace hit" $ \hit ->
    TraceHit
      <$> hit .: "site"
      <*> hit .: "core"
      <*> hit .: "instructions"
      <*> (hit .: "r0" >>= parseRegister)
      <*> (hit .: "r1" >>= parseRegister)
      <*> (hit .: "r2" >>= parseRegister)
      <*> (hit .: "r3" >>= parseRegister)

data Observation = Observation
  { observationPhase :: SegmentName
  , observationHits :: [TraceHit]
  } deriving (Eq, Show)

instance FromJSON Observation where
  parseJSON = withObject "observation" $ \observation ->
    Observation
      <$> observation .: "phase"
      <*> observation .:? "trace_hits" .!= []

data RelinkMode = RelinkReviewed | RelinkUnreviewed deriving (Eq, Show)

instance FromJSON RelinkMode where
  parseJSON = withText "relink mode" $ \mode -> case mode of
    "reviewed" -> pure RelinkReviewed
    "unreviewed" -> pure RelinkUnreviewed
    other -> fail ("unknown relink mode: " ++ T.unpack other)

data PhaseDisposition = PhaseMatched deriving (Eq, Show)

instance FromJSON PhaseDisposition where
  parseJSON = withText "phase disposition" $ \value -> case value of
    "matched" -> pure PhaseMatched
    other -> fail ("unknown phase disposition: " ++ T.unpack other)

data TrajectoryResult = TrajectoryResult
  { trajectoryName :: SegmentName
  , trajectoryResult :: PhaseDisposition
  } deriving (Eq, Show)

instance FromJSON TrajectoryResult where
  parseJSON = withObject "trajectory result" $ \result ->
    TrajectoryResult
      <$> result .: "name"
      <*> result .: "result"

newtype PublicationCount = PublicationCount { publicationCountValue :: Int } deriving (Eq, Ord, Show)

instance FromJSON PublicationCount where
  parseJSON value = do
    count <- parseJSON value :: Parser Int
    if count >= 0 then pure (PublicationCount count) else fail "publication count must be nonnegative"

newtype ByteSize = ByteSize { byteSizeValue :: Int } deriving (Eq, Ord, Show)

instance FromJSON ByteSize where
  parseJSON value = do
    size <- parseJSON value :: Parser Int
    if size >= 0 then pure (ByteSize size) else fail "runtime size must be nonnegative"

newtype OperationStatus = OperationStatus { operationStatusValue :: Int } deriving (Eq, Show)

instance FromJSON OperationStatus where
  parseJSON value = do
    status <- parseJSON value :: Parser Int
    if status >= 0 then pure (OperationStatus status) else fail "operation status must be nonnegative"

data Operation = InstallOperation | RollbackOperation deriving (Eq, Show)

instance FromJSON Operation where
  parseJSON value = do
    operation <- parseJSON value :: Parser Int
    case operation of
      1 -> pure InstallOperation
      2 -> pure RollbackOperation
      _ -> fail "mailbox operation must be 1 or 2"

data RuntimeFacts = RuntimeFacts
  { runtimeByteSize :: ByteSize
  , runtimePublicationCount :: PublicationCount
  , runtimeSymbolsSha :: Digest
  } deriving (Eq, Show)

instance FromJSON RuntimeFacts where
  parseJSON = withObject "runtime" $ \runtime ->
    RuntimeFacts
      <$> runtime .: "bytes"
      <*> runtime .: "publications"
      <*> runtime .: "symbols_sha256"

data RelinkFacts = RelinkFacts
  { relinkMode :: RelinkMode
  , relinkSourceIdentity :: M.Map String Digest
  } deriving (Eq, Show)

instance FromJSON RelinkFacts where
  parseJSON = withObject "relink" $ \relink ->
    RelinkFacts
      <$> relink .: "mode"
      <*> relink .: "source_identity"

data StartFacts = StartFacts
  { startInstalledDat :: Digest
  , startCardPresent :: Maybe Bool
  } deriving (Eq, Show)

instance FromJSON StartFacts where
  parseJSON = withObject "emulator start" $ \start -> do
    inputs <- start .: "inputs"
    withObject "emulator inputs" parseFiles inputs
    where
      parseFiles inputs = do
        files <- inputs .: "inputs"
        withObject "emulator input files" (\value ->
          StartFacts
            <$> value .: "installed_dat"
            <*> value .:? "card_present"
          ) files

data MailboxOp = MailboxOp
  { mailboxOperation :: Operation
  , mailboxStatus :: OperationStatus
  , mailboxPeerStatus :: OperationStatus
  , mailboxValidated :: Maybe PublicationCount
  , mailboxRestored :: Maybe PublicationCount
  } deriving (Eq, Show)

instance FromJSON MailboxOp where
  parseJSON = withObject "mailbox operation" $ \mailbox ->
    MailboxOp
      <$> mailbox .: "operation"
      <*> mailbox .: "status"
      <*> mailbox .: "peer_status"
      <*> mailbox .:? "validated"
      <*> mailbox .:? "restored"

data Mailbox = Mailbox
  { mailboxInstall :: MailboxOp
  , mailboxRollback :: MailboxOp
  } deriving (Eq, Show)

instance FromJSON Mailbox where
  parseJSON = withObject "mailbox" $ \mailbox ->
    Mailbox
      <$> mailbox .: "install"
      <*> mailbox .: "rollback"

data E2E = E2E
  { e2eSchema :: Int
  , e2eOutcome :: WatchJudgement
  , e2eRuntime :: RuntimeFacts
  , e2eRelink :: RelinkFacts
  , e2eStart :: StartFacts
  , e2eObservations :: [Observation]
  , e2eTrajectories :: [TrajectoryResult]
  , e2eUnload :: Maybe String
  , e2eMailbox :: Mailbox
  , e2eDeclaredSites :: [DeclaredTraceSite]
  } deriving (Eq, Show)

instance FromJSON E2E where
  parseJSON = withObject "e2e result" $ \result ->
    E2E
      <$> result .: "schema"
      <*> result .: "outcome"
      <*> result .: "runtime"
      <*> result .: "relink"
      <*> result .: "emulator_start"
      <*> result .: "observations"
      <*> result .: "trajectories"
      <*> result .:? "unload"
      <*> result .: "mailbox"
      <*> result .: "declared_trace_sites"

data PackageSchema = RecipeExecutablePackage deriving (Eq, Show)

instance FromJSON PackageSchema where
  parseJSON = withText "package schema" $ \schema -> case schema of
    "recipe-executable-package-1" -> pure RecipeExecutablePackage
    other -> fail ("unknown package schema: " ++ T.unpack other)

data PackageInput = PackageInput
  { packageInputPath :: String
  , packageInputSha :: Digest
  } deriving (Eq, Show)

instance FromJSON PackageInput where
  parseJSON = withObject "package input" $ \input ->
    PackageInput
      <$> input .: "path"
      <*> input .: "sha256"

data PackageManifest = PackageManifest
  { packageSchema :: PackageSchema
  , packageFirmwareSha :: Digest
  , packageExecutableSha :: Digest
  , packageExecutableStart :: TraceAddress
  , packageInputs :: [PackageInput]
  , packageInstallable :: Bool
  } deriving (Eq, Show)

instance FromJSON PackageManifest where
  parseJSON = withObject "package manifest" $ \manifest ->
    PackageManifest
      <$> manifest .: "schema"
      <*> manifest .: "firmware_sha256"
      <*> manifest .: "executable_sha256"
      <*> (manifest .: "executable_start" >>= parseAddressText)
      <*> manifest .: "inputs"
      <*> manifest .: "installable"

data PackagedInputs = PackagedInputs
  { packagedReceiptDir :: FilePath
  , packagedPackageDir :: FilePath
  } deriving (Eq, Show)

data PackagedStatus = PackagedQualified | PackagedRefused deriving (Eq, Show)

data PackagedRow = PackagedRow
  { packagedRowToken :: Word32
  , packagedRowExpected :: Word32
  , packagedRowEntries :: Int
  , packagedRowReturned :: [Word32]
  , packagedRowNested :: Int
  , packagedRowPhases :: [SegmentName]
  } deriving (Eq, Show)

data OwnedSequence = OwnedSequence
  { ownedPhase :: SegmentName
  , ownedRow :: Word32
  , ownedCore :: CoreId
  , ownedEntry :: TraceHit
  , ownedTrampoline :: TraceHit
  , ownedStock :: TraceHit
  , ownedNested :: [TraceHit]
  , ownedReturn :: TraceHit
  } deriving (Eq, Show)

data PackagedIdentities = PackagedIdentities
  { packagedDatSha :: Digest
  , packagedArchiveSha :: Digest
  , packagedFactsSha :: Digest
  , packagedExecutableSha :: Digest
  , packagedInstalledDat :: Digest
  , packagedResultSha :: Digest
  , packagedManifestSha :: Digest
  , packagedSymbolsSha :: Digest
  , packagedRuntimeSha :: Digest
  , packagedRelinkMode :: RelinkMode
  , packagedOutcome :: WatchJudgement
  } deriving (Eq, Show)

data PackagedReport = PackagedReport
  { packagedStatus :: PackagedStatus
  , packagedRows :: [PackagedRow]
  , packagedSequences :: [OwnedSequence]
  , packagedRetainedHits :: [TraceHit]
  , packagedProblems :: [String]
  , packagedIdentities :: PackagedIdentities
  , packagedPhases :: [SegmentName]
  , packagedUnload :: Maybe String
  } deriving (Eq, Show)

packagedStatusLabel :: PackagedStatus -> String
packagedStatusLabel PackagedQualified = "qualified"
packagedStatusLabel PackagedRefused = "refused"

qualifyPackaged :: PackagedInputs -> IO (Either String PackagedReport)
qualifyPackaged (PackagedInputs receipt packageDirectory) = do
  control <- doesFileExist (receipt </> "e2e-control.json")
  if control
    then pure (Left "receipt contains e2e-control.json alongside a candidate result")
    else do
      resultEither <- readJson (receipt </> "e2e-result.json")
      case resultEither of
        Left problem -> pure (Left problem)
        Right result -> do
          manifestEither <- try (readRamPushManifest (receipt </> "relinked" </> "ram-push-manifest.json"))
            :: IO (Either IOException RamPushManifest)
          case manifestEither of
            Left problem -> pure (Left ("relinked manifest: " ++ show problem))
            Right manifest -> do
              packageEither <- readJson (packageDirectory </> "recipe-complete.json")
              case packageEither of
                Left problem -> pure (Left problem)
                Right package ->
                  case (uniqueInput "bank.a" package, uniqueInput "build-facts.txt" package) of
                    (Just archive, Just facts) -> do
                      symbolsBytes <- B.readFile (receipt </> "relinked" </> "kernel.syms")
                      actualDat <- digestFile (packageDirectory </> "recipe-complete.DAT")
                      actualKernel <- digestFile (packageDirectory </> "kernel" </> "kernel.bin")
                      actualArchive <- digestFile (packagePath packageDirectory archive)
                      actualFacts <- digestFile (packagePath packageDirectory facts)
                      resultDigest <- digestFile (receipt </> "e2e-result.json")
                      let table = parseSymbolTable symbolsBytes
                          symbolsDigest = trustedDigest (sha256Hex (sha256 symbolsBytes))
                          identities = PackagedIdentities
                            { packagedDatSha = actualDat
                            , packagedArchiveSha = actualArchive
                            , packagedFactsSha = actualFacts
                            , packagedExecutableSha = packageExecutableSha package
                            , packagedInstalledDat = startInstalledDat (e2eStart result)
                            , packagedResultSha = resultDigest
                            , packagedManifestSha = trustedDigest (manifestDigest manifest)
                            , packagedSymbolsSha = symbolsDigest
                            , packagedRuntimeSha = trustedDigest (runtimeDigest manifest)
                            , packagedRelinkMode = relinkMode (e2eRelink result)
                            , packagedOutcome = e2eOutcome result
                            }
                          (sequences, retained, traceProblems) = analyzeObservations result
                          rowResults = rowEvidence sequences
                          problems = nub
                            (schemaProblems result
                              ++ modeProblems result
                              ++ identityProblems package manifest result actualDat actualKernel actualArchive actualFacts symbolsDigest
                              ++ validateDeclaredSites table (e2eDeclaredSites result)
                              ++ traceProblems
                              ++ rowProblemsFor rowResults
                              ++ lifecycleChecks result
                              ++ crossSequenceProblems sequences)
                      pure (Right PackagedReport
                        { packagedStatus = if null problems then PackagedQualified else PackagedRefused
                        , packagedRows = rowResults
                        , packagedSequences = sequences
                        , packagedRetainedHits = retained
                        , packagedProblems = problems
                        , packagedIdentities = identities
                        , packagedPhases = nub (map observationPhase (e2eObservations result) ++ map trajectoryName (e2eTrajectories result))
                        , packagedUnload = e2eUnload result
                        })
                    _ -> pure (Left "package requires exactly one bank.a and one build-facts.txt input")
  where
    uniqueInput name manifest = case inputsNamed name manifest of
      [input] -> Just input
      _ -> Nothing
    packagePath packageDir input =
      let path = packageInputPath input
       in if isAbsolute path then path else packageDir </> path

readJson :: FromJSON a => FilePath -> IO (Either String a)
readJson path = do
  bytes <- B.readFile path
  pure (either (Left . ((path ++ ": ") ++)) Right (eitherDecodeStrict' bytes))

digestFile :: FilePath -> IO Digest
digestFile path = trustedDigest . sha256Hex <$> sha256File path

inputsNamed :: String -> PackageManifest -> [PackageInput]
inputsNamed name manifest =
  [ input | input <- packageInputs manifest, takeFileName (packageInputPath input) == name ]

selectedRows :: [Replacement.Service.Qualify.ServiceRow]
selectedRows = filter ((== 0x110) . serviceRowProperty) serviceRows

selectedRow :: Word32 -> Maybe Replacement.Service.Qualify.ServiceRow
selectedRow token = find ((== token) . serviceRowToken) selectedRows

schemaProblems :: E2E -> [String]
schemaProblems result = [ "E2E schema is not 1" | e2eSchema result /= 1 ]

modeProblems :: E2E -> [String]
modeProblems result =
  [ "packaged run outcome is not candidate-qualified" | e2eOutcome result /= CandidateQualified ]

data SourceIdentityKey = BankArchiveKey | ToolchainFactsKey deriving (Eq, Show)

sourceIdentityKeyText :: SourceIdentityKey -> String
sourceIdentityKeyText BankArchiveKey = "rust_object_replacement_bank"
sourceIdentityKeyText ToolchainFactsKey = "rust_toolchain_replacement_bank"

identityProblems :: PackageManifest -> RamPushManifest -> E2E -> Digest -> Digest -> Digest -> Digest -> Digest -> [String]
identityProblems package manifest result actualDat actualKernel actualArchive actualFacts symbolsDigest =
  [ "package DAT digest differs from recipe-complete.DAT" | actualDat /= packageFirmwareSha package ]
    ++ [ "package kernel digest differs from kernel/kernel.bin" | actualKernel /= packageExecutableSha package ]
    ++ [ "package firmware digest differs from the emulator start input" | packageFirmwareSha package /= startInstalledDat (e2eStart result) ]
    ++ [ "emulator start omits card_present" | startCardPresent (e2eStart result) == Nothing ]
    ++ [ "emulator start is not the cardless route" | startCardPresent (e2eStart result) /= Just False ]
    ++ [ "package has no unique bank.a input" | length (inputsNamed "bank.a" package) /= 1 ]
    ++ [ "package has no unique build-facts.txt input" | length (inputsNamed "build-facts.txt" package) /= 1 ]
    ++ [ "bank.a bytes differ from the package input digest"
       | Just input <- [uniqueInput "bank.a" package], actualArchive /= packageInputSha input ]
    ++ [ "build-facts.txt bytes differ from the package input digest"
       | Just input <- [uniqueInput "build-facts.txt" package], actualFacts /= packageInputSha input ]
    ++ [ "runtime symbols digest differs from relinked/kernel.syms" | runtimeSymbolsSha (e2eRuntime result) /= symbolsDigest ]
    ++ [ "E2E runtime byte count differs from the relink manifest" | runtimeByteSize (e2eRuntime result) /= ByteSize (allocationSize manifest) ]
    ++ [ "E2E runtime publication count differs from the relink manifest" | runtimePublicationCount (e2eRuntime result) /= PublicationCount (length (manifestPublications manifest)) ]
    ++ [ "relinked manifest digest is missing" | null (manifestDigest manifest) ]
    ++ [ "relinked runtime digest is missing" | null (runtimeDigest manifest) ]
    ++ [ "E2E relink source identity differs from the relink manifest" | relinkSourceIdentity (e2eRelink result) /= manifestIdentity manifest ]
    ++ [ "relink source identity omits an archive identity" | M.lookup (sourceIdentityKeyText BankArchiveKey) manifestIdentityValue == Nothing ]
    ++ [ "relink source identity omits a toolchain identity" | M.lookup (sourceIdentityKeyText ToolchainFactsKey) manifestIdentityValue == Nothing ]
    ++ [ "E2E source identity omits an archive identity" | M.lookup (sourceIdentityKeyText BankArchiveKey) (relinkSourceIdentity (e2eRelink result)) == Nothing ]
    ++ [ "E2E source identity omits a toolchain identity" | M.lookup (sourceIdentityKeyText ToolchainFactsKey) (relinkSourceIdentity (e2eRelink result)) == Nothing ]
    ++ [ "relinked archive identity differs from the actual bank.a bytes"
       | Just value <- [M.lookup (sourceIdentityKeyText BankArchiveKey) manifestIdentityValue]
       , value /= actualArchive ]
    ++ [ "relinked toolchain identity differs from the actual build-facts.txt bytes"
       | Just value <- [M.lookup (sourceIdentityKeyText ToolchainFactsKey) manifestIdentityValue]
       , value /= actualFacts ]
    ++ [ "relinked archive identity differs from the package archive"
       | Just archive <- [uniqueInput "bank.a" package]
       , Just value <- [M.lookup (sourceIdentityKeyText BankArchiveKey) manifestIdentityValue]
       , value /= packageInputSha archive ]
    ++ [ "relinked toolchain identity differs from the package facts"
       | Just facts <- [uniqueInput "build-facts.txt" package]
       , Just value <- [M.lookup (sourceIdentityKeyText ToolchainFactsKey) manifestIdentityValue]
       , value /= packageInputSha facts ]
  where
    uniqueInput name value = case inputsNamed name value of
      [input] -> Just input
      _ -> Nothing
    manifestIdentityValue = manifestIdentity manifest
    manifestIdentity value = M.fromList [(name, trustedDigest digest) | (name, digest) <- sourceIdentity value]

validateDeclaredSites :: SymbolTable -> [DeclaredTraceSite] -> [String]
validateDeclaredSites table sites =
  let addresses =
        [ (SvcRustEntry, symbol "rb_service_field_row_state")
        , (SvcStockTrampoline, symbol "rb_call_stock_probe")
        , (SvcStockReturn, symbolPlus "rb_call_stock_probe" 0x10)
        , (StockRowProbe, Right (TraceAddress serviceStockTarget))
        ]
      declared = [(declaredTraceLabel site, declaredTraceAddress site) | site <- sites]
      duplicateLabels = duplicates (map fst declared)
      duplicateAddresses = duplicates (map snd declared)
      checks = concat
        [ case resolved of
            Left problem -> [problem]
            Right expected ->
              case lookup label declared of
                Nothing -> ["declared trace site is missing: " ++ traceSiteLabelText label]
                Just actual | actual == expected -> []
                Just actual -> [ "declared trace address for " ++ traceSiteLabelText label ++ " is "
                                   ++ traceAddressText actual ++ ", expected " ++ traceAddressText expected ]
        | (label, resolved) <- addresses
        ]
   in checks
        ++ [ "declared trace labels are not unique: " ++ show duplicateLabels | not (null duplicateLabels) ]
        ++ [ "declared trace addresses are not unique: " ++ show [ traceAddressText value | value <- duplicateAddresses ]
           | not (null duplicateAddresses) ]
  where
    symbol name = case resolveSymbol table name of
      Left problem -> Left problem
      Right address -> Right (TraceAddress address)
    symbolPlus name offset = case resolveSymbol table name of
      Left problem -> Left problem
      Right address
        | address > maxBound - offset -> Left ("runtime symbol " ++ name ++ " overflows the return offset")
        | otherwise -> Right (TraceAddress (address + offset))
    duplicates values = [ value | value <- nub values, length (filter (== value) values) > 1 ]

data PositionedHit = PositionedHit
  { positionedObservation :: Int
  , positionedPhase :: SegmentName
  , positionedHit :: TraceHit
  }

analyzeObservations :: E2E -> ([OwnedSequence], [TraceHit], [String])
analyzeObservations result =
  let observations = e2eObservations result
      boundary = findIndex ((== serviceAfterUnloadPhase) . observationPhase) observations
      positions = concat
        [ [PositionedHit observationIndex (observationPhase observation) hit
          | hit <- observationHits observation]
        | (observationIndex, observation) <- zip [0 :: Int ..] observations
        ]
      (sequences, retained, problems) = analyzeHits boundary positions
      orderProblems =
        [ "trace instruction counts regress across retained or owned events"
        | not (nondecreasing (map (instructionCountValue . hitInstructions . positionedHit) positions)) ]
   in (sequences, retained, problems ++ orderProblems)

analyzeHits :: Maybe Int -> [PositionedHit] -> ([OwnedSequence], [TraceHit], [String])
analyzeHits boundary = go [] [] []
  where
    go sequences retained problems [] = (reverse sequences, reverse retained, reverse problems)
    go sequences retained problems (position : rest)
      | hitSite hit == entryLabel =
          if maybe False (positionedObservation position >=) boundary
            then (reverse sequences, reverse retained, reverse ("svc-rust-entry occurs after service-after-unload" : problems))
            else case parseSequence (position : rest) of
              Left problem -> (reverse sequences, reverse retained, reverse (problem : problems))
              Right (ownedSequence, remaining) -> go (ownedSequence : sequences) retained problems remaining
      | hitSite hit == stockLabel = go sequences (hit : retained) problems rest
      | otherwise = (reverse sequences, reverse retained, reverse (("unexpected trace event outside an owned interval: " ++ traceSiteLabelText (hitSite hit)) : problems))
      where
        hit = positionedHit position

parseSequence :: [PositionedHit] -> Either String (OwnedSequence, [PositionedHit])
parseSequence (entryPosition : rest) =
  let entry = positionedHit entryPosition
   in case selectedRow (hitR1 entry) of
        Just row | hitR0 entry == 0x110 ->
          case expect trampolineLabel rest of
            Left problem -> Left problem
            Right (trampoline, rest1) ->
              case expect stockLabel rest1 of
                Left problem -> Left problem
                Right (stock, rest2) ->
                  -- private-data/analysis/x-m1-1.50/filmsim/fn_d52684.c:2539-2547:
                  -- case 0x110 calls FUN_00d52684(5,0x1a); the second call is conditional.
                  let (nestedPositions, rest3) = span isNested rest2
                      nested = map positionedHit nestedPositions
                      ordered = entry : trampoline : stock : nested
                      expected = rowStateWord (serviceRowExpected row)
                   in case expect returnLabel rest3 of
                        Left problem -> Left problem
                        Right (returned, rest4) ->
                          let problems =
                                [ "svc-stock-trampoline arguments differ from svc-rust-entry"
                                | argumentPair trampoline /= argumentPair entry ]
                                  ++ [ "stock-row-probe outer arguments differ from svc-rust-entry"
                                     | argumentPair stock /= argumentPair entry ]
                                  ++ [ "0x110 owned call must contain one or two FUN_00d52684(5,0x1a) nested calls"
                                     | length nested < 1 || length nested > 2 ]
                                  ++ [ "owned service trace crosses cores"
                                     | length (nub (map hitCore (returned : ordered))) /= 1 ]
                                  ++ [ "owned service trace instruction counts are not strictly increasing"
                                     | not (strictlyIncreasing (map (instructionCountValue . hitInstructions) (ordered ++ [returned]))) ]
                                  ++ [ "svc-stock-return returned " ++ show (hitR0 returned) ++ ", expected " ++ show expected
                                     | hitR0 returned /= expected ]
                           in case problems of
                                [] -> Right
                                  ( OwnedSequence
                                      { ownedPhase = positionedPhase entryPosition
                                      , ownedRow = serviceRowToken row
                                      , ownedCore = hitCore entry
                                      , ownedEntry = entry
                                      , ownedTrampoline = trampoline
                                      , ownedStock = stock
                                      , ownedNested = nested
                                      , ownedReturn = returned
                                      }
                                  , rest4
                                  )
                                problem : _ -> Left problem
        _ -> Left "svc-rust-entry arguments are outside the five selected 0x110 rows"
parseSequence _ = Left "svc-rust-entry is missing"

expect :: TraceSiteLabel -> [PositionedHit] -> Either String (TraceHit, [PositionedHit])
expect label (position : rest)
  | hitSite (positionedHit position) == label = Right (positionedHit position, rest)
  | otherwise = Left ("expected " ++ traceSiteLabelText label ++ ", observed " ++ traceSiteLabelText (hitSite (positionedHit position)))
expect label [] = Left ("missing " ++ traceSiteLabelText label)

isNested :: PositionedHit -> Bool
isNested position =
  let hit = positionedHit position
   in hitSite hit == stockLabel && argumentPair hit == (5, 0x1a)

argumentPair :: TraceHit -> (Word32, Word32)
argumentPair hit = (hitR0 hit, hitR1 hit)

strictlyIncreasing :: [Word64] -> Bool
strictlyIncreasing values = and (zipWith (<) values (drop 1 values))

nondecreasing :: [Word64] -> Bool
nondecreasing values = and (zipWith (<=) values (drop 1 values))

crossSequenceProblems :: [OwnedSequence] -> [String]
crossSequenceProblems sequences =
  [ "owned service traces cross cores" | length (nub (map ownedCore sequences)) > 1 ]

rowQualified :: PackagedRow -> Bool
rowQualified row =
  packagedRowEntries row >= 2
    && serviceFieldListPhase `elem` packagedRowPhases row
    && serviceFieldReopenPhase `elem` packagedRowPhases row
    && all (== packagedRowExpected row) (packagedRowReturned row)

qualifiedRowCount :: [PackagedRow] -> Int
qualifiedRowCount = length . filter rowQualified

rowProblemsFor :: [PackagedRow] -> [String]
rowProblemsFor rows = concat
  [ [ "selected row " ++ show (packagedRowToken row) ++ " was entered " ++ show (packagedRowEntries row) ++ " times; repeat entry requires at least two"
    | packagedRowEntries row < 2 ]
    ++ [ "selected row " ++ show (packagedRowToken row) ++ " lacks a service-field-list owned sequence"
       | serviceFieldListPhase `notElem` packagedRowPhases row ]
    ++ [ "selected row " ++ show (packagedRowToken row) ++ " lacks a service-field-reopen owned sequence"
       | serviceFieldReopenPhase `notElem` packagedRowPhases row ]
    ++ [ "selected row " ++ show (packagedRowToken row) ++ " returned "
           ++ show (packagedRowReturned row) ++ ", expected " ++ show (packagedRowExpected row)
       | any (/= packagedRowExpected row) (packagedRowReturned row) ]
  | row <- rows
  ]
    ++ [ "required selected row count is " ++ show (length selectedRows) ++ ", observed " ++ show (length rows)
       | length rows /= length selectedRows
       ]

rowEvidence :: [OwnedSequence] -> [PackagedRow]
rowEvidence sequences =
  [ PackagedRow
      { packagedRowToken = serviceRowToken row
      , packagedRowExpected = rowStateWord (serviceRowExpected row)
      , packagedRowEntries = length owned
      , packagedRowReturned = map (hitR0 . ownedReturn) owned
      , packagedRowNested = sum (map (length . ownedNested) owned)
      , packagedRowPhases = nub (map ownedPhase owned)
      }
  | row <- selectedRows
  , let owned = [ownedSequence | ownedSequence <- sequences, ownedRow ownedSequence == serviceRowToken row]
  ]

lifecycleChecks :: E2E -> [String]
lifecycleChecks result =
  [ "packaged trajectory did not match service-page-cancel" | not (trajectoryMatched servicePageCancelPhase) ]
    ++ [ "packaged trajectory did not match service-field-reopen" | not (trajectoryMatched serviceFieldReopenPhase) ]
    ++ [ "packaged trajectory has no service-after-unload observation"
       | not (any ((== serviceAfterUnloadPhase) . observationPhase) (e2eObservations result)) ]
    ++ [ "packaged trajectory has no post-unload stock observation"
       | not (any ((== serviceAfterUnloadStockPhase) . observationPhase) (e2eObservations result)) ]
    ++ [ "post-unload stock observation has no stock probe activity"
       | not (any (\observation -> observationPhase observation == serviceAfterUnloadStockPhase
                     && any ((== stockLabel) . hitSite) (observationHits observation))
              (e2eObservations result)) ]
    ++ [ "packaged run has no unload outcome" | maybe True (null) (e2eUnload result) ]
    ++ mailboxProblems
  where
    trajectoryMatched name =
      any (\trajectory -> trajectoryName trajectory == name && trajectoryResult trajectory == PhaseMatched)
        (e2eTrajectories result)
    mailbox = e2eMailbox result
    publications = runtimePublicationCount (e2eRuntime result)
    install = mailboxInstall mailbox
    rollback = mailboxRollback mailbox
    mailboxProblems =
      [ "INSTALL mailbox is not a successful publication of every manifest word"
      | mailboxOperation install /= InstallOperation
          || mailboxStatus install /= OperationStatus 0
          || mailboxPeerStatus install /= OperationStatus 0
          || mailboxValidated install /= Just publications
      ]
      ++ [ "rollback mailbox is not a successful restoration of every publication"
         | mailboxOperation rollback /= RollbackOperation
             || mailboxStatus rollback /= OperationStatus 0
             || mailboxPeerStatus rollback /= OperationStatus 0
             || mailboxRestored rollback /= Just publications
         ]

entryLabel, trampolineLabel, returnLabel, stockLabel :: TraceSiteLabel
entryLabel = SvcRustEntry
trampolineLabel = SvcStockTrampoline
returnLabel = SvcStockReturn
stockLabel = StockRowProbe

serviceFieldListPhase, servicePageCancelPhase, serviceFieldReopenPhase, serviceAfterUnloadPhase, serviceAfterUnloadStockPhase :: SegmentName
serviceFieldListPhase = phaseName "service-field-list"
servicePageCancelPhase = phaseName "service-page-cancel"
serviceFieldReopenPhase = phaseName "service-field-reopen"
serviceAfterUnloadPhase = phaseName "service-after-unload"
serviceAfterUnloadStockPhase = phaseName "service-after-unload-stock"

phaseName :: Text -> SegmentName
phaseName text = either (\problem -> error ("invalid service phase: " ++ T.unpack text ++ ": " ++ problem)) id (segmentName text)

parseAddress8 :: Text -> Parser TraceAddress
parseAddress8 text
  | T.length text == 8
  , all isHexDigit (T.unpack text)
  , [(value, "")] <- readHex (T.unpack text) = pure (TraceAddress value)
  | otherwise = fail ("trace address must be eight hex digits without 0x: " ++ T.unpack text)

parseAddressText :: Text -> Parser TraceAddress
parseAddressText text = case T.stripPrefix "0x" text of
  Just digits
    | not (T.null digits), all isHexDigit (T.unpack digits), length (T.unpack digits) <= 8
    , [(value, "")] <- readHex (T.unpack digits) -> pure (TraceAddress value)
  _ -> fail ("hex address requires 0x: " ++ T.unpack text)

parseRegister :: Value -> Parser Word32
parseRegister (String text) = do
  address <- case T.stripPrefix "0x" text of
    Just _ -> parseAddressText text
    Nothing -> parseAddress8 text
  pure (traceAddressWord address)
parseRegister _ = fail "register value must be a hex string"

traceAddressText :: TraceAddress -> String
traceAddressText (TraceAddress value) = pad (showHex value "")
  where
    pad text = replicate (8 - length text) '0' ++ text

packagedReportValue :: PackagedReport -> Value
packagedReportValue report =
  obj
    [ ("schema_version", n 1)
    , ("profile", s "service-packaged")
    , ("contract_sha256", s serviceContractSha256)
    , ("contract", serviceContractValue)
    , ("status", s (packagedStatusLabel (packagedStatus report)))
    , ("capability", b (packagedStatus report == PackagedQualified))
    , ("required_rows", n (fromIntegral (length selectedRows)))
    , ("qualified_rows", n (fromIntegral (qualifiedRowCount (packagedRows report))))
    , ("rows", arr (map rowValue (packagedRows report)))
    , ("sequences", arr (map sequenceValue (packagedSequences report)))
    , ("retained_stock_hits", arr (map hitValue (packagedRetainedHits report)))
    , ("identities", identitiesValue (packagedIdentities report))
    , ("phases", arr (map (s . T.unpack . segmentNameText) (packagedPhases report)))
    , ("unload", maybe Null s (packagedUnload report))
    , ("problems", arr (map s (packagedProblems report)))
    , ("evidence_limits", arr (map s evidenceLimits))
    ]
  where
    rowValue row = obj
      [ ("property", n 272)
      , ("row", n (fromIntegral (packagedRowToken row)))
      , ("expected", n (fromIntegral (packagedRowExpected row)))
      , ("entries", n (fromIntegral (packagedRowEntries row)))
      , ("returned", arr (map (n . fromIntegral) (packagedRowReturned row)))
      , ("nested_calls", n (fromIntegral (packagedRowNested row)))
      , ("phases", arr (map (s . T.unpack . segmentNameText) (packagedRowPhases row)))
      ]
    sequenceValue ownedSequence = obj
      [ ("row", n (fromIntegral (ownedRow ownedSequence)))
      , ("phase", s (T.unpack (segmentNameText (ownedPhase ownedSequence))))
      , ("core", n (fromIntegral (coreIdValue (ownedCore ownedSequence))))
      , ("entry_instructions", n (fromIntegral (instructionCountValue (hitInstructions (ownedEntry ownedSequence)))))
      , ("return_instructions", n (fromIntegral (instructionCountValue (hitInstructions (ownedReturn ownedSequence)))))
      , ("nested_calls", n (fromIntegral (length (ownedNested ownedSequence))))
      , ("returned", n (fromIntegral (hitR0 (ownedReturn ownedSequence))))
      ]
    hitValue hit = obj
      [ ("site", s (traceSiteLabelText (hitSite hit)))
      , ("core", n (fromIntegral (coreIdValue (hitCore hit))))
      , ("instructions", n (fromIntegral (instructionCountValue (hitInstructions hit))))
      , ("r0", s (traceAddressText (TraceAddress (hitR0 hit))))
      , ("r1", s (traceAddressText (TraceAddress (hitR1 hit))))
      ]
    identitiesValue identities = obj
      [ ("package_dat_sha256", s (digestText (packagedDatSha identities)))
      , ("package_archive_sha256", s (digestText (packagedArchiveSha identities)))
      , ("package_facts_sha256", s (digestText (packagedFactsSha identities)))
      , ("package_executable_sha256", s (digestText (packagedExecutableSha identities)))
      , ("installed_dat_sha256", s (digestText (packagedInstalledDat identities)))
      , ("e2e_result_sha256", s (digestText (packagedResultSha identities)))
      , ("relinked_manifest_sha256", s (digestText (packagedManifestSha identities)))
      , ("relinked_symbols_sha256", s (digestText (packagedSymbolsSha identities)))
      , ("runtime_sha256", s (digestText (packagedRuntimeSha identities)))
      , ("relink_mode", s (relinkModeText (packagedRelinkMode identities)))
      , ("outcome", s (outcomeText (packagedOutcome identities)))
      ]
    evidenceLimits =
      [ "Only the five selected 0x110 rows are packaged qualification rows; stock probe calls outside each owned entry/return are retained for review."
      , "The 0x110 body's grounded FUN_00d52684(5,0x1a) nested call is allowed once or twice; any other stock event inside the owned interval refuses."
      , "rb_service_field_row_state tail-branches; rb_call_stock_probe+0x10 is the retained-call return."
      , "Page-visit cancellation is evidenced by service-page-cancel and service-field-reopen; cancellation inside the synchronous call and handback remain not applicable."
      , "No camera, hardware, visual or final-byte review is claimed."
      ]

outcomeText :: WatchJudgement -> String
outcomeText CandidateQualified = "candidate-qualified"
outcomeText ControlRefusedProtectedWrite = "control-refused"

relinkModeText :: RelinkMode -> String
relinkModeText RelinkReviewed = "reviewed"
relinkModeText RelinkUnreviewed = "unreviewed"
