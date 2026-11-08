{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Host admission for one completed render-pair receipt and for the inputs of a role that has
-- not run yet. A receipt is admitted only against the exact inputs it names.
module Emulation.RenderPairAdmission
  ( Role (..)
  , roleText
  , Digest
  , digestText
  , digestBytes
  , digestFile
  , parseDigest
  , CommitId (..)
  , commitText
  , ModelId (..)
  , ProcedureId (..)
  , CheckpointId (..)
  , checkpointText
  , SourceKind (..)
  , sourceKindText
  , Expected (..)
  , InputPins (..)
  , Provenance (..)
  , ReceiptIdentity (..)
  , DiagnosticSource (..)
  , DiagnosticEntry (..)
  , DiagnosticConfig (..)
  , verifyInputs
  , verifyReceipt
  , bindReceipt
  , importReceipt
  , writeOwnedConfig
  , ownedConfigFromElf
  , readDiagnosticConfig
  , renderSourceIdentityPaths
  ) where

import qualified ArmElf as ELF
import Control.Applicative ((<|>))
import Control.Monad (forM, unless)
import BankAllowlist (Report, allowlistAnalysisIdentity, violations)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (isDigit)
import Data.List (isInfixOf, isPrefixOf, nub, sort, sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import Data.String (IsString (..))
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import qualified Emulation.ObservationStream as OS
import Numeric (readHex, showHex)
import qualified RenderStack as R
import qualified Replacement.ModalRuntime.RenderStackEvidence as E
import System.Directory
  ( canonicalizePath
  , copyFileWithMetadata
  , createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , listDirectory
  , makeAbsolute
  , pathIsSymbolicLink
  , removeFile
  , renameFile
  )
import System.FilePath ((</>), splitDirectories, takeDirectory, takeFileName)
import System.IO.Error (catchIOError, isDoesNotExistError)

data Role = StockRole | OwnedRole deriving (Eq, Show)

data ReceiptQualification = CandidateReceipt | ScopedLifetimeReceipt deriving (Eq)

parseQualification :: J.Value -> J.Parser ReceiptQualification
parseQualification = J.withText "receipt qualification" $ \value -> case value of
  "candidate-qualified" -> pure CandidateReceipt
  "qualified-scoped-lifetime-diagnostic" -> pure ScopedLifetimeReceipt
  _ -> fail "receipt has no successful qualification"

roleText :: Role -> T.Text
roleText StockRole = "stock"
roleText OwnedRole = "owned"

parseRole :: T.Text -> Either String Role
parseRole "stock" = Right StockRole
parseRole "owned" = Right OwnedRole
parseRole other = Left ("unknown render role " ++ T.unpack other)

newtype Digest = Digest Sha256

instance Eq Digest where
  Digest left == Digest right = left == right

instance Ord Digest where
  compare (Digest left) (Digest right) = compare (sha256Hex left) (sha256Hex right)

instance Show Digest where
  show (Digest sha) = sha256Hex sha

digestText :: Digest -> T.Text
digestText (Digest sha) = T.pack (sha256Hex sha)

parseDigest :: T.Text -> Either String Digest
parseDigest text = Digest <$> parseSha256Hex (T.unpack text)

digestFile :: FilePath -> IO Digest
digestFile path = Digest <$> sha256File path

digestBytes :: B.ByteString -> Digest
digestBytes = Digest . sha256

newtype CommitId = CommitId T.Text deriving (Eq, Ord, Show)

commitText :: CommitId -> T.Text
commitText (CommitId text) = text

instance IsString CommitId where
  fromString = CommitId . T.pack

newtype ModelId = ModelId T.Text deriving (Eq, Ord, Show)

newtype ProcedureId = ProcedureId T.Text deriving (Eq, Ord, Show)

newtype CheckpointId = CheckpointId T.Text deriving (Eq, Ord, Show)

checkpointText :: CheckpointId -> T.Text
checkpointText (CheckpointId text) = text

data SourceKind = OwnedSource | RetainedSource deriving (Eq, Ord, Show)

sourceKindText :: SourceKind -> T.Text
sourceKindText OwnedSource = "owned"
sourceKindText RetainedSource = "retained"

parseSourceKind :: T.Text -> Either String SourceKind
parseSourceKind "owned" = Right OwnedSource
parseSourceKind "retained" = Right RetainedSource
parseSourceKind other = Left ("unknown diagnostic source kind " ++ T.unpack other)

stackKindText :: E.StackKind -> T.Text
stackKindText E.TreeStack = "tree"
stackKindText E.PickerStack = "picker"
stackKindText E.PaneStack = "pane"

parseStackKind :: T.Text -> Either String E.StackKind
parseStackKind "tree" = Right E.TreeStack
parseStackKind "picker" = Right E.PickerStack
parseStackKind "pane" = Right E.PaneStack
parseStackKind other = Left ("unknown diagnostic entry kind " ++ T.unpack other)

-- | The input identities one role's invocation must use. 'expectedPins' carries the guest inputs
-- a previously admitted role of the same cohort recorded, so both roles bind the same inputs.
data Expected = Expected
  { expectedName :: T.Text
  , expectedRole :: Role
  , expectedSourceCommit :: CommitId
  , expectedHeadCommit :: CommitId
  , expectedSdk :: FilePath
  , expectedFujiModPaths :: FilePath
  , expectedTrajectory :: FilePath
  , expectedTrajectorySha :: Digest
  , expectedFirmware :: FilePath
  , expectedNand :: FilePath
  , expectedBootRom :: FilePath
  , expectedRunDocument :: FilePath
  , expectedConfig :: FilePath
  , expectedConfigSha :: Digest
  , expectedArchive :: Maybe FilePath
  , expectedArchiveSha :: Maybe Digest
  , expectedSourceIdentity :: Maybe FilePath
  , expectedChecker :: Maybe FilePath
  , expectedPrelinked :: Maybe FilePath
  , expectedPrelinkedBase :: Maybe Word32
  , expectedPrelinkedSize :: Maybe Word32
  , expectedPins :: Maybe InputPins
  }

data InputPins = InputPins
  { pinsInstalledDat :: Digest
  , pinsAdjustmentNand :: Digest
  , pinsBootRom :: Digest
  , pinsRunDocument :: Digest
  , pinsModel :: ModelId
  , pinsProcedure :: ProcedureId
  } deriving (Eq, Show)

data Provenance = GuestRun | FromReviewPacket FilePath Digest deriving (Eq, Show)

-- | What a verified receipt records. Every digest is recomputed at verification time.
data ReceiptIdentity = ReceiptIdentity
  { identityName :: T.Text
  , identityRole :: Role
  , identityPhases :: [T.Text]
  , identityObservationCount :: Int
  , identityPins :: InputPins
  , identityCheckpoint :: CheckpointId
  , identityKernelElf :: Digest
  , identityResult :: Digest
  , identitySession :: Digest
  , identityModal :: Digest
  , identityDiagnostic :: Digest
  , identityRequest :: Digest
  , identityWatchFinal :: Digest
  , identityManifest :: Digest
  , identityRuntime :: Digest
  , identityTaskIdentityVerified :: Bool
  , identityTaskEntries :: Int
  , identityBankVariant :: T.Text
  , identityPublications :: Int
  } deriving (Eq, Show)

data DiagnosticSource = DiagnosticSource
  { sourceKind :: SourceKind
  , sourceStart :: E.StackAddress
  , sourceSha :: Digest
  , sourcePath :: FilePath
  } deriving (Eq, Ord, Show)

data DiagnosticEntry = DiagnosticEntry
  { entryKind :: E.StackKind
  , entryAddress :: E.StackAddress
  , entryDepth :: E.FrameBytes
  } deriving (Eq, Show)

entryKey :: DiagnosticEntry -> (E.StackKind, E.StackAddress)
entryKey entry = (entryKind entry, entryAddress entry)

data DiagnosticConfig = DiagnosticConfig
  { configRole :: Role
  , configSources :: [DiagnosticSource]
  , configEntries :: [DiagnosticEntry]
  , configReturns :: [Word32]
  , configRetained :: [Word32]
  } deriving (Eq, Show)

-- JSON ---------------------------------------------------------------------------------------

instance J.FromJSON Digest where
  parseJSON = J.withText "SHA-256" (either fail pure . parseDigest)

instance J.ToJSON Digest where
  toJSON = J.String . digestText

instance J.FromJSON CommitId where
  parseJSON = J.withText "commit" (pure . CommitId)

instance J.ToJSON CommitId where
  toJSON = J.String . commitText

instance J.FromJSON ModelId where
  parseJSON = J.withText "model" (pure . ModelId)

instance J.ToJSON ModelId where
  toJSON (ModelId text) = J.String text

instance J.FromJSON ProcedureId where
  parseJSON = J.withText "procedure" (pure . ProcedureId)

instance J.ToJSON ProcedureId where
  toJSON (ProcedureId text) = J.String text

instance J.FromJSON CheckpointId where
  parseJSON = J.withText "checkpoint" (pure . CheckpointId)

instance J.ToJSON CheckpointId where
  toJSON = J.String . checkpointText

instance J.FromJSON Role where
  parseJSON = J.withText "render role" (either fail pure . parseRole)

instance J.ToJSON Role where
  toJSON = J.String . roleText

sourceValue :: DiagnosticSource -> J.Value
sourceValue (DiagnosticSource kind start sha path) = J.object
  [ "kind" J..= sourceKindText kind, "start" J..= start, "sha256" J..= sha, "path" J..= path ]

entryValue :: DiagnosticEntry -> J.Value
entryValue (DiagnosticEntry kind address depth) = J.object
  [ "kind" J..= stackKindText kind, "address" J..= address, "frame_depth" J..= frameDepth depth ]
 where
  frameDepth (E.FrameBytes bytes) = bytes

configValue :: DiagnosticConfig -> J.Value
configValue (DiagnosticConfig role sources entries returns retained) = J.object
  [ "version" J..= (1 :: Word32)
  , "role" J..= roleText role
  , "sources" J..= map sourceValue sources
  , "entries" J..= map entryValue entries
  , "returns" J..= returns
  , "retained_services" J..= retained
  ]

instance J.FromJSON DiagnosticSource where
  parseJSON = J.withObject "diagnostic source" $ \fields -> do
    kind <- fields J..: "kind" >>= either fail pure . parseSourceKind
    DiagnosticSource kind <$> fields J..: "start" <*> fields J..: "sha256" <*> fields J..: "path"

instance J.FromJSON DiagnosticEntry where
  parseJSON = J.withObject "diagnostic entry" $ \fields -> do
    kind <- fields J..: "kind" >>= either fail pure . parseStackKind
    DiagnosticEntry kind <$> fields J..: "address" <*> fields J..: "frame_depth"

instance J.FromJSON DiagnosticConfig where
  parseJSON = J.withObject "diagnostic configuration" $ \fields -> do
    version <- fields J..: "version" :: J.Parser Word32
    unless (version == 1) (fail "unsupported render diagnostic configuration version")
    DiagnosticConfig
      <$> fields J..: "role" <*> fields J..: "sources" <*> fields J..: "entries"
      <*> fields J..: "returns" <*> fields J..: "retained_services"

readDiagnosticConfig :: FilePath -> IO (Either String DiagnosticConfig)
readDiagnosticConfig path = do
  bytes <- B.readFile path
  pure (J.eitherDecodeStrict' bytes)

-- Input verification -------------------------------------------------------------------------

-- | Runs before any guest launch.
verifyInputs :: Expected -> IO (Either String ())
verifyInputs expected = runChecks
  [ checkDigest "trajectory" (expectedTrajectory expected) (expectedTrajectorySha expected)
  , checkDigest "diagnostic configuration" (expectedConfig expected) (expectedConfigSha expected)
  , checkFujiModPaths expected
  , checkSourceIdentity expected
  , checkOwnedInputs expected
  , checkPins expected
  ]

runChecks :: [IO (Either String ())] -> IO (Either String ())
runChecks checks = do
  results <- sequence checks
  pure (case [problem | Left problem <- results] of [] -> Right (); problem : _ -> Left problem)

checkDigest :: String -> FilePath -> Digest -> IO (Either String ())
checkDigest label path expected = do
  present <- doesFileExist path
  if not present then pure (Left ("missing " ++ label ++ ": " ++ path)) else do
    actual <- digestFile path
    pure (if actual == expected then Right () else Left
      (label ++ " SHA-256 " ++ T.unpack (digestText actual) ++ " differs from " ++ T.unpack (digestText expected)))

checkFujiModPaths :: Expected -> IO (Either String ())
checkFujiModPaths expected = do
  present <- doesFileExist (expectedFujiModPaths expected)
  if not present then pure (Left ("missing generated FujiModPaths: " ++ expectedFujiModPaths expected)) else do
    text <- readFile (expectedFujiModPaths expected)
    let sdk = expectedSdk expected
        wanted =
          [ "fujimodRamPushBridge = \"" ++ sdk ++ "/bin/fujimod-xm1-ram-push\""
          , "fujimodRun = \"" ++ sdk ++ "/share/fujimod-xm1/xm1-150.json\""
          ]
    pure (if all (`isInfixOf` text) wanted then Right () else Left
      ("built viewer does not name SDK " ++ sdk ++ " for its bridge and run document"))

-- | The renderer sources a frozen owned build binds, in the order host/firmware/BuildRecipeBank.hs
-- emits them.
renderSourceIdentityPaths :: [FilePath]
renderSourceIdentityPaths =
  [ "replacement/render_adapter/mod.rs"
  , "replacement/render_adapter/entry.S"
  , "replacement/render_adapter/class.rs"
  , "replacement/render_adapter/calls.rs"
  , "replacement/render_adapter/picker.rs"
  , "replacement/render_adapter/tree.rs"
  , "replacement/render_adapter/pane.rs"
  , "replacement/render_adapter/build.sh"
  , "replacement/render_commands/src/lib.rs"
  , "replacement/menu_widgets/src/lib.rs"
  , "replacement/menu_widgets/src/value_picker.rs"
  , "replacement/menu_widgets/src/tree_list.rs"
  , "replacement/menu_widgets/src/pane_drawer.rs"
  , "host/firmware/RenderStack.hs"
  ]

checkSourceIdentity :: Expected -> IO (Either String ())
checkSourceIdentity expected = case (expectedRole expected, expectedSourceIdentity expected) of
  (OwnedRole, Nothing) -> pure (Left "owned frozen admission needs the archive's source identity")
  (_, Nothing) -> pure (Right ())
  (_, Just path) -> do
    present <- doesFileExist path
    if not present then pure (Left ("source identity is missing: " ++ path)) else do
      parsed <- jsonFile path
      case parsed of
        Left problem -> pure (Left ("source identity: " ++ problem))
        Right (J.Object fields) -> case traverse parseBinding (KM.toList fields) of
          Left problem -> pure (Left ("source identity: " ++ problem))
          Right bindings | M.null (M.fromList bindings) -> pure (Left "source identity is empty")
          Right bindings
            | expectedRole expected == OwnedRole
            , sort (map fst bindings) /= sort renderSourceIdentityPaths ->
                pure (Left "source identity does not name the renderer sources")
            | otherwise -> go (M.toList (M.fromList bindings))
        Right _ -> pure (Left "source identity is not an object")
 where
  parseBinding (key, J.String text) = do
    digest <- either Left Right (parseDigest text)
    pure (T.unpack (K.toText key), digest)
  parseBinding (key, _) = Left ("source identity value is not a string: " ++ T.unpack (K.toText key))
  go [] = pure (Right ())
  go ((relative, expectedDigest) : rest) = do
    present <- doesFileExist relative
    if not present then pure (Left ("source identity names missing file " ++ relative)) else do
      actual <- digestFile relative
      if actual /= expectedDigest then pure (Left
        ("source identity of " ++ relative ++ " is " ++ T.unpack (digestText actual)
          ++ ", the frozen build used " ++ T.unpack (digestText expectedDigest))) else go rest

checkOwnedInputs :: Expected -> IO (Either String ())
checkOwnedInputs expected = case expectedRole expected of
  StockRole -> pure (Right ())
  OwnedRole -> case (expectedArchive expected, expectedArchiveSha expected) of
    (Nothing, _) -> pure (Left "owned role needs its archive")
    (_, Nothing) -> pure (Left "owned role needs the frozen archive SHA-256")
    (Just archive, Just sha) -> do
      digest <- checkDigest "owned archive" archive sha
      config <- readDiagnosticConfig (expectedConfig expected)
      case (digest, config) of
        (Left problem, _) -> pure (Left problem)
        (_, Left problem) -> pure (Left ("owned diagnostic configuration: " ++ problem))
        (_, Right parsed) -> do
          sources <- forM (configSources parsed) $ \source -> do
            present <- doesFileExist (sourcePath source)
            if not present then pure (Left ("diagnostic source is missing: " ++ sourcePath source)) else do
              actual <- digestFile (sourcePath source)
              pure (if actual == sourceSha source then Right () else Left
                ("diagnostic source " ++ sourcePath source ++ " differs from its recorded bytes"))
          prelinkedCheck <- case (expectedPrelinked expected, expectedPrelinkedBase expected, expectedPrelinkedSize expected, expectedArchiveSha expected) of
            (Nothing, _, _, _) -> pure (Right ())
            (Just directory, Just base, Just size, Just archiveSha) -> checkPrelinked directory base size archiveSha (expectedChecker expected) (expectedSourceIdentity expected)
            _ -> pure (Left "prelinked admission needs --prelinked-base, --prelinked-size and --archive-sha256")
          pure $ do
            unless (configRole parsed == OwnedRole) (Left "owned role was given a non-owned diagnostic configuration")
            let kinds = sort (map entryKind (configEntries parsed))
            unless (kinds == [E.TreeStack, E.PickerStack, E.PaneStack])
              (Left "owned diagnostic configuration lacks the three drawer entries")
            unless (any ((== RetainedSource) . sourceKind) (configSources parsed)
              && any ((== OwnedSource) . sourceKind) (configSources parsed))
              (Left "owned diagnostic configuration lacks retained or owned sources")
            case [problem | Left problem <- sources] of
              [] -> Right ()
              problem : _ -> Left problem
            prelinkedCheck

data PrelinkedProof = PrelinkedProof
  { admissionProofAnalysis :: String
  , admissionProofChecker :: String
  , admissionProofVariant :: String
  , admissionProofPolicy :: String
  , admissionProofManifest :: String
  , admissionProofRuntime :: String
  , admissionProofElf :: String
  , admissionProofArchive :: String
  , admissionProofInput :: String
  , admissionProofReport :: String
  }

instance J.FromJSON PrelinkedProof where
  parseJSON = J.withObject "prelinked allowlist proof" $ \fields ->
    PrelinkedProof <$> fields J..: "analysis" <*> fields J..: "checker_sha256" <*> fields J..: "bank_variant"
      <*> fields J..: "policy" <*> fields J..: "manifest_sha256" <*> fields J..: "runtime_sha256"
      <*> fields J..: "kernel_elf_sha256" <*> fields J..: "archive_sha256" <*> fields J..: "allowlist_input_sha256"
      <*> fields J..: "report_sha256"

checkPrelinked :: FilePath -> Word32 -> Word32 -> Digest -> Maybe FilePath -> Maybe FilePath -> IO (Either String ())
checkPrelinked directory base size archiveSha checkerPath sourceIdentityPath = do
  manifest <- jsonFile (directory </> "ram-push-manifest.json")
  runtime <- digestFile (directory </> "runtime.bin")
  kernel <- doesFileExist (directory </> "kernel.elf")
  scope <- doesFileExist (directory </> "modal-scope.json")
  checker <- case checkerPath of
    Nothing -> pure (Left "prelinked admission needs --checker")
    Just path -> do
      present <- doesFileExist path
      if present then Right <$> digestFile path else pure (Left ("prelinked checker is missing: " ++ path))
  proofBytes <- B.readFile (directory </> "bank-allowlist-proof.json")
  reportBytes <- B.readFile (directory </> "bank-allowlist-report.json")
  inputBytes <- B.readFile (directory </> "bank-allowlist-input.json")
  elfBytes <- B.readFile (directory </> "kernel.elf")
  runtimeBytes <- B.readFile (directory </> "runtime.bin")
  manifestBytes <- B.readFile (directory </> "ram-push-manifest.json")
  let proof = J.eitherDecodeStrict' proofBytes :: Either String PrelinkedProof
      report = J.eitherDecodeStrict' reportBytes :: Either String Report
      reportResult = case J.eitherDecodeStrict' reportBytes of
        Right value -> J.parseEither (J.withObject "allowlist report" (\fields -> fields J..: "result" :: J.Parser T.Text)) value
        Left problem -> Left problem
  case (manifest, proof, report, reportResult, checker) of
    (Left problem, _, _, _, _) -> pure (Left ("prelinked manifest: " ++ problem))
    (_, Left problem, _, _, _) -> pure (Left ("prelinked allowlist proof: " ++ problem))
    (_, _, Left problem, _, _) -> pure (Left ("prelinked allowlist report: " ++ problem))
    (_, _, _, Left problem, _) -> pure (Left ("prelinked allowlist report: " ++ problem))
    (_, _, _, _, Left problem) -> pure (Left ("prelinked checker: " ++ problem))
    (Right value, Right checked, Right parsedReport, Right result, Right checkerDigest) -> case J.parseEither (J.withObject "prelinked manifest" (\fields -> do
        baseText <- fields J..: "base" :: J.Parser T.Text
        baseValue <- either fail pure (hexWord baseText)
        sizeValue <- fields J..: "size" :: J.Parser Word32
        runtimeSha <- fields J..: "runtime_sha256" :: J.Parser Digest
        variantText <- fields J..: "bank_variant" :: J.Parser T.Text
        identity <- fields J..: "source_identity" >>= J.withObject "source identity" (\source -> source J..: "rust_object_replacement_bank" :: J.Parser Digest)
        sources <- fields J..: "source_identity" >>= J.withObject "source identity" (\source -> source J..: "render_sources_sha256" :: J.Parser Digest)
        pure (baseValue, sizeValue, runtimeSha, variantText, identity, sources))) value of
      Left problem -> pure (Left ("prelinked manifest: " ++ problem))
      Right (baseValue, sizeValue, runtimeSha, variantText, identity, sources) -> do
        sourceSidecar <- case sourceIdentityPath of
          Nothing -> pure (Left "prelinked admission needs the render source identity sidecar")
          Just path -> do
            present <- doesFileExist path
            if present then Right <$> digestFile path else pure (Left ("render source identity is missing: " ++ path))
        case sourceSidecar of
          Left problem -> pure (Left problem)
          Right sourceDigest -> pure $ do
            unless (sourceDigest == sources) (Left "render source identity differs from the prelinked manifest")
            unless (baseValue == base) (Left "prelinked manifest base differs from the admitted base")
            unless (sizeValue == size) (Left "prelinked manifest size differs from the admitted size")
            unless (runtime == runtimeSha) (Left "prelinked runtime differs from its manifest")
            unless (identity == archiveSha) (Left "prelinked manifest archive differs from the admitted archive")
            unless kernel (Left "prelinked kernel.elf is missing")
            unless scope (Left "prelinked modal-scope.json is missing")
            unless (admissionProofAnalysis checked == allowlistAnalysisIdentity) (Left "prelinked proof analysis differs")
            unless (admissionProofChecker checked == T.unpack (digestText checkerDigest))
              (Left "prelinked proof checker identity differs from the admitted checker")
            unless (admissionProofVariant checked == T.unpack variantText) (Left "prelinked proof bank variant differs")
            unless (admissionProofPolicy checked == "enforce") (Left "prelinked proof policy differs")
            unless (admissionProofManifest checked == T.unpack (digestText (digestBytes manifestBytes)))
              (Left "prelinked proof manifest identity differs")
            unless (admissionProofRuntime checked == T.unpack (digestText (digestBytes runtimeBytes)))
              (Left "prelinked proof runtime identity differs")
            unless (admissionProofElf checked == T.unpack (digestText (digestBytes elfBytes))) (Left "prelinked proof kernel ELF identity differs")
            unless (admissionProofArchive checked == T.unpack (digestText archiveSha)) (Left "prelinked proof archive identity differs")
            unless (admissionProofInput checked == T.unpack (digestText (digestBytes inputBytes))) (Left "prelinked proof allowlist input identity differs")
            unless (admissionProofReport checked == T.unpack (digestText (digestBytes reportBytes))) (Left "prelinked proof report identity differs")
            unless (result == "pass" && null (violations parsedReport)) (Left "prelinked allowlist report is not a pass")
 where
  hexWord text = case stripPrefix "0x" (T.unpack text) of
    Just digits | [(value, "")] <- readHex digits -> Right (value :: Word32)
    _ -> Left "prelinked manifest base is not a hex word"

comparePrelinked :: FilePath -> FilePath -> IO (Either String ())
comparePrelinked prelinked receipt = runChecks
  [ compareBytes "kernel.elf" (prelinked </> "kernel.elf") (receipt </> "relinked/kernel.elf")
  , compareBytes "runtime.bin" (prelinked </> "runtime.bin") (receipt </> "relinked/runtime.bin")
  , compareBytes "ram-push-manifest.json" (prelinked </> "ram-push-manifest.json") (receipt </> "relinked/ram-push-manifest.json")
  , compareBytes "bank-allowlist.json" (prelinked </> "bank-allowlist-report.json") (receipt </> "relinked/bank-allowlist.json")
  , compareBytes "bank-allowlist-proof.json" (prelinked </> "bank-allowlist-proof.json") (receipt </> "relinked/bank-allowlist-proof.json")
  ]
 where
  compareBytes label expected actual = do
    expectedBytes <- B.readFile expected
    actualBytes <- B.readFile actual
    pure (if expectedBytes == actualBytes then Right () else Left ("receipt " ++ label ++ " differs from the prelinked build"))

checkPins :: Expected -> IO (Either String ())
checkPins expected = case expectedPins expected of
  Nothing -> pure (Right ())
  Just pins -> checkPinsValue expected pins

checkPinsValue :: Expected -> InputPins -> IO (Either String ())
checkPinsValue expected pins = runChecks
  [ checkDigest "firmware" (expectedFirmware expected) (pinsInstalledDat pins)
  , checkDigest "NAND" (expectedNand expected) (pinsAdjustmentNand pins)
  , checkDigest "boot ROM" (expectedBootRom expected) (pinsBootRom pins)
  , checkDigest "run document" (expectedRunDocument expected) (pinsRunDocument pins)
  ]

-- Receipt verification -----------------------------------------------------------------------

-- | Re-verifies an admitted receipt. It refuses when the admission is missing, when any bound
-- file changed, or when the receipt no longer has the admitted role's shape.
verifyReceipt :: Expected -> FilePath -> IO (Either String ReceiptIdentity)
verifyReceipt expected directory = do
  inputs <- verifyInputs expected
  case inputs of
    Left problem -> pure (Left problem)
    Right () -> do
      admission <- readAdmission directory
      case admission of
        Left problem -> pure (Left problem)
        Right value -> case J.fromJSON value of
          J.Error problem -> pure (Left ("admission record: " ++ problem))
          J.Success (record :: AdmissionRecord) -> case admissionMismatch expected record of
            Just problem -> pure (Left problem)
            Nothing -> do
              pins <- checkPinsValue expected (recordPins record)
              case pins of
                Left problem -> pure (Left problem)
                Right () -> do
                  sourceIdentity <- traverse digestFile (expectedSourceIdentity expected)
                  case (recordSourceIdentity record, sourceIdentity) of
                    (Just recorded, Just actual)
                      | recorded == actual -> verifyAfter record
                      | otherwise -> pure (Left "source identity file differs from the admission")
                    (Nothing, Nothing) -> verifyAfter record
                    _ -> pure (Left "source identity binding differs from the admission")
 where
  verifyAfter record = do
    contents <- verifyContents expected directory
    pure $ case contents of
      Left problem -> Left problem
      Right identity -> case receiptMismatch record identity of
        Just problem -> Left problem
        Nothing -> Right identity

-- | Verifies a receipt that has no admission record yet and writes one.
bindReceipt :: Expected -> FilePath -> Provenance -> IO (Either String ReceiptIdentity)
bindReceipt expected directory provenance = do
  inputs <- verifyInputs expected
  case inputs of
    Left problem -> pure (Left problem)
    Right () -> do
      contents <- verifyContents expected directory
      case contents of
        Left problem -> pure (Left problem)
        Right identity -> do
          sourceIdentity <- traverse digestFile (expectedSourceIdentity expected)
          let record = admissionRecord expected identity sourceIdentity provenance
          exists <- doesFileExist (directory </> "admission.json")
          if exists
            then pure (Left "receipt already has an admission record; verify or import it instead")
            else do
              writeAtomic (directory </> "admission.json") (BL.toStrict (J.encode record) <> "\n")
              pure (Right identity)

-- | Imports a completed stock receipt named by its review packet: verifies the packet's hashes,
-- the receipt's internal inputs and shape, copies the receipt, and writes the admission record.
importReceipt :: Expected -> FilePath -> FilePath -> FilePath -> IO (Either String ReceiptIdentity)
importReceipt expected packet source destination = do
  inputs <- verifyInputs expected
  case inputs of
    Left problem -> pure (Left problem)
    Right () -> do
      packetValue <- jsonFile packet
      case packetValue of
        Left problem -> pure (Left ("review packet: " ++ problem))
        Right value -> case J.fromJSON value of
          J.Error problem -> pure (Left ("review packet: " ++ problem))
          J.Success (parsed :: ReviewPacket) -> do
            packetDigest <- digestFile packet
            case packetProblems expected parsed of
              problem : _ -> pure (Left problem)
              [] -> do
                contents <- verifyContents expected source
                case contents of
                  Left problem -> pure (Left problem)
                  Right identity -> do
                    evidence <- verifyPacketEvidence parsed source
                    case evidence of
                      Left problem -> pure (Left problem)
                      Right () -> do
                        prepared <- prepareDestination source destination
                        case prepared of
                          Left problem -> pure (Left problem)
                          Right () -> do
                            copied <- copyTree source destination
                            case copied of
                              Left problem -> pure (Left problem)
                              Right () -> do
                                sourceIdentity <- traverse digestFile (expectedSourceIdentity expected)
                                let record = admissionRecord expected identity sourceIdentity (FromReviewPacket packet packetDigest)
                                writeAtomic (destination </> "admission.json") (BL.toStrict (J.encode record) <> "\n")
                                verifyReceipt expected destination

prepareDestination :: FilePath -> FilePath -> IO (Either String ())
prepareDestination source destination = do
  sourcePath <- canonicalizePath source
  target <- canonicalizeTarget destination
  if within sourcePath target || within target sourcePath
    then pure (Left "import source and destination overlap")
    else do
      unsafe <- treeSymlinks sourcePath
      case unsafe of
        Left problem -> pure (Left problem)
        Right () -> do
          present <- doesDirectoryExist destination
          contents <- if present then listDirectory destination else pure []
          if not (null contents) then pure (Left ("import destination is not empty: " ++ destination)) else do
            createDirectoryIfMissing True destination
            pure (Right ())
 where
  within parent child = splitDirectories parent `isPrefixOf` splitDirectories child

canonicalizeTarget :: FilePath -> IO FilePath
canonicalizeTarget path = do
  let parent = takeDirectory path
      name = takeFileName path
  present <- doesDirectoryExist parent
  if present
    then (</> name) <$> canonicalizePath parent
    else do
      resolved <- canonicalizeTarget parent
      pure (resolved </> name)

treeSymlinks :: FilePath -> IO (Either String ())
treeSymlinks path = do
  entries <- listDirectory path
  results <- forM entries $ \name -> do
    let entry = path </> name
    link <- pathIsSymbolicLink entry
    directory <- doesDirectoryExist entry
    if link && directory
      then pure (Left ("import source contains a directory symlink: " ++ entry))
      else if directory then treeSymlinks entry else pure (Right ())
  pure (case [problem | Left problem <- results] of [] -> Right (); problem : _ -> Left problem)

copyTree :: FilePath -> FilePath -> IO (Either String ())
copyTree source destination = do
  entries <- listDirectory source
  results <- forM entries $ \name -> do
    let from = source </> name
        to = destination </> name
    link <- pathIsSymbolicLink from
    directory <- doesDirectoryExist from
    if link && directory
      then pure (Left ("import source contains a directory symlink: " ++ from))
      else if directory
        then do
          createDirectoryIfMissing True to
          copyTree from to
        else do
          copyFileWithMetadata from to
          pure (Right ())
  pure (case [problem | Left problem <- results] of [] -> Right (); problem : _ -> Left problem)

-- Admission record ---------------------------------------------------------------------------

data AdmissionRecord = AdmissionRecord
  { recordSchema :: Word32
  , recordContract :: T.Text
  , recordName :: T.Text
  , recordRole :: Role
  , recordSourceCommit :: CommitId
  , recordHeadCommit :: CommitId
  , recordSdk :: FilePath
  , recordTrajectory :: FilePath
  , recordTrajectorySha :: Digest
  , recordConfig :: FilePath
  , recordConfigSha :: Digest
  , recordArchive :: Maybe FilePath
  , recordArchiveSha :: Maybe Digest
  , recordSourceIdentity :: Maybe Digest
  , recordPins :: InputPins
  , recordCheckpoint :: CheckpointId
  , recordKernelElf :: Digest
  , recordResult :: Digest
  , recordSession :: Digest
  , recordModal :: Digest
  , recordDiagnostic :: Digest
  , recordRequest :: Digest
  , recordWatchFinal :: Digest
  , recordManifest :: Digest
  , recordRuntime :: Digest
  , recordPhases :: [T.Text]
  , recordObservationCount :: Int
  , recordProvenance :: Provenance
  }

instance J.ToJSON AdmissionRecord where
  toJSON record = J.object
    [ "schema" J..= recordSchema record
    , "contract" J..= recordContract record
    , "name" J..= recordName record
    , "role" J..= roleText (recordRole record)
    , "source_commit" J..= recordSourceCommit record
    , "head_commit" J..= recordHeadCommit record
    , "sdk_package" J..= recordSdk record
    , "trajectory" J..= J.object ["path" J..= recordTrajectory record, "sha256" J..= recordTrajectorySha record]
    , "diagnostic_config" J..= J.object ["path" J..= recordConfig record, "sha256" J..= recordConfigSha record]
    , "archive" J..= fmap (\path -> J.object ["path" J..= path, "sha256" J..= recordArchiveSha record]) (recordArchive record)
    , "source_identity" J..= recordSourceIdentity record
    , "inputs" J..= pinsValue (recordPins record)
    , "checkpoint" J..= recordCheckpoint record
    , "receipt" J..= J.object
        [ "kernel_elf_sha256" J..= recordKernelElf record
        , "e2e_result_sha256" J..= recordResult record
        , "emulator_session_sha256" J..= recordSession record
        , "e2e_modal_observation_sha256" J..= recordModal record
        , "e2e_render_stack_diagnostic_sha256" J..= recordDiagnostic record
        , "e2e_render_stack_diagnostic_request_sha256" J..= recordRequest record
        , "e2e_modal_watch_final_sha256" J..= recordWatchFinal record
        , "ram_push_manifest_sha256" J..= recordManifest record
        , "runtime_sha256" J..= recordRuntime record
        , "phases" J..= recordPhases record
        , "observation_count" J..= recordObservationCount record
        ]
    , "provenance" J..= provenanceValue (recordProvenance record)
    ]

instance J.FromJSON AdmissionRecord where
  parseJSON = J.withObject "admission record" $ \fields -> do
    schema <- fields J..: "schema"
    unless (schema == (1 :: Word32)) (fail "unsupported admission record version")
    contract <- fields J..: "contract"
    unless (contract == ("render-pair-receipt-admission" :: T.Text)) (fail "not a render pair admission record")
    receipt <- fields J..: "receipt"
    trajectory <- fields J..: "trajectory"
    config <- fields J..: "diagnostic_config"
    archive <- fields J..:? "archive"
    AdmissionRecord schema contract
      <$> fields J..: "name" <*> fields J..: "role" <*> fields J..: "source_commit" <*> fields J..: "head_commit"
      <*> fields J..: "sdk_package"
      <*> J.withObject "trajectory" (\t -> t J..: "path") trajectory
      <*> J.withObject "trajectory" (\t -> t J..: "sha256") trajectory
      <*> J.withObject "diagnostic_config" (\c -> c J..: "path") config
      <*> J.withObject "diagnostic_config" (\c -> c J..: "sha256") config
      <*> traverse (J.withObject "archive" (\a -> a J..: "path")) archive
      <*> traverse (J.withObject "archive" (\a -> a J..: "sha256")) archive
      <*> fields J..:? "source_identity"
      <*> fields J..: "inputs"
      <*> fields J..: "checkpoint"
      <*> J.withObject "receipt" (\r -> r J..: "kernel_elf_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "e2e_result_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "emulator_session_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "e2e_modal_observation_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "e2e_render_stack_diagnostic_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "e2e_render_stack_diagnostic_request_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "e2e_modal_watch_final_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "ram_push_manifest_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "runtime_sha256") receipt
      <*> J.withObject "receipt" (\r -> r J..: "phases") receipt
      <*> J.withObject "receipt" (\r -> r J..: "observation_count") receipt
      <*> fields J..: "provenance"

pinsValue :: InputPins -> J.Value
pinsValue pins = J.object
  [ "installed_dat" J..= pinsInstalledDat pins
  , "adjustment_nand" J..= pinsAdjustmentNand pins
  , "boot_rom" J..= pinsBootRom pins
  , "run_document" J..= pinsRunDocument pins
  , "model" J..= pinsModel pins
  , "procedure" J..= pinsProcedure pins
  ]

instance J.FromJSON InputPins where
  parseJSON = J.withObject "admission inputs" $ \fields ->
    InputPins <$> fields J..: "installed_dat" <*> fields J..: "adjustment_nand" <*> fields J..: "boot_rom"
      <*> fields J..: "run_document" <*> fields J..: "model" <*> fields J..: "procedure"

provenanceValue :: Provenance -> J.Value
provenanceValue GuestRun = J.object ["kind" J..= ("guest-run" :: T.Text)]
provenanceValue (FromReviewPacket path digest) = J.object
  ["kind" J..= ("review-packet" :: T.Text), "path" J..= path, "sha256" J..= digest]

instance J.FromJSON Provenance where
  parseJSON = J.withObject "admission provenance" $ \fields -> do
    kind <- fields J..: "kind" :: J.Parser T.Text
    case kind of
      "guest-run" -> pure GuestRun
      "review-packet" -> FromReviewPacket <$> fields J..: "path" <*> fields J..: "sha256"
      _ -> fail "unknown admission provenance"

admissionRecord :: Expected -> ReceiptIdentity -> Maybe Digest -> Provenance -> AdmissionRecord
admissionRecord expected identity sourceIdentity provenance = AdmissionRecord
  { recordSchema = 1
  , recordContract = "render-pair-receipt-admission"
  , recordName = expectedName expected
  , recordRole = expectedRole expected
  , recordSourceCommit = expectedSourceCommit expected
  , recordHeadCommit = expectedHeadCommit expected
  , recordSdk = expectedSdk expected
  , recordTrajectory = expectedTrajectory expected
  , recordTrajectorySha = expectedTrajectorySha expected
  , recordConfig = expectedConfig expected
  , recordConfigSha = expectedConfigSha expected
  , recordArchive = expectedArchive expected
  , recordArchiveSha = expectedArchiveSha expected
  , recordSourceIdentity = sourceIdentity
  , recordPins = identityPins identity
  , recordCheckpoint = identityCheckpoint identity
  , recordKernelElf = identityKernelElf identity
  , recordResult = identityResult identity
  , recordSession = identitySession identity
  , recordModal = identityModal identity
  , recordDiagnostic = identityDiagnostic identity
  , recordRequest = identityRequest identity
  , recordWatchFinal = identityWatchFinal identity
  , recordManifest = identityManifest identity
  , recordRuntime = identityRuntime identity
  , recordPhases = identityPhases identity
  , recordObservationCount = identityObservationCount identity
  , recordProvenance = provenance
  }

admissionMismatch :: Expected -> AdmissionRecord -> Maybe String
admissionMismatch expected record
  | recordName record /= expectedName expected = Just "admission names a different case"
  | recordRole record /= expectedRole expected = Just "admission names a different role"
  | not (commitMatches (recordSourceCommit record) (expectedSourceCommit expected)) = Just "admission names a different source commit"
  | recordSdk record /= expectedSdk expected = Just "admission names a different SDK package"
  | recordTrajectorySha record /= expectedTrajectorySha expected = Just "admission names a different trajectory"
  | recordConfigSha record /= expectedConfigSha expected = Just "admission names a different diagnostic configuration"
  | recordArchiveSha record /= expectedArchiveSha expected = Just "admission names a different owned archive"
  | otherwise = Nothing
 where
  commitMatches (CommitId recorded) (CommitId wanted) = recorded == wanted
    || (T.length recorded >= 8 && recorded `T.isPrefixOf` wanted)
    || (T.length wanted >= 8 && wanted `T.isPrefixOf` recorded)

receiptMismatch :: AdmissionRecord -> ReceiptIdentity -> Maybe String
receiptMismatch record identity
  | recordPins record /= identityPins identity = Just "receipt guest inputs differ from the admission"
  | recordCheckpoint record /= identityCheckpoint identity = Just "receipt checkpoint differs from the admission"
  | recordKernelElf record /= identityKernelElf identity = Just "receipt kernel ELF differs from the admission"
  | recordResult record /= identityResult identity = Just "receipt result differs from the admission"
  | recordSession record /= identitySession identity = Just "receipt session differs from the admission"
  | recordModal record /= identityModal identity = Just "receipt modal observation differs from the admission"
  | recordDiagnostic record /= identityDiagnostic identity = Just "receipt render diagnostic differs from the admission"
  | recordRequest record /= identityRequest identity = Just "receipt diagnostic request differs from the admission"
  | recordWatchFinal record /= identityWatchFinal identity = Just "receipt lifetime watch differs from the admission"
  | recordManifest record /= identityManifest identity = Just "receipt manifest differs from the admission"
  | recordRuntime record /= identityRuntime identity = Just "receipt runtime differs from the admission"
  | recordPhases record /= identityPhases identity = Just "receipt phases differ from the admission"
  | recordObservationCount record /= identityObservationCount identity = Just "receipt observation count differs from the admission"
  | otherwise = Nothing

readAdmission :: FilePath -> IO (Either String J.Value)
readAdmission directory = do
  present <- doesFileExist (directory </> "admission.json")
  if not present then pure (Left ("no admission record in " ++ directory ++ "; a nonempty receipt is not reusable without one"))
    else jsonFile (directory </> "admission.json")

jsonFile :: FilePath -> IO (Either String J.Value)
jsonFile path = do
  bytes <- B.readFile path
  pure (J.eitherDecodeStrict' bytes)

-- Receipt contents ---------------------------------------------------------------------------

verifyContents :: Expected -> FilePath -> IO (Either String ReceiptIdentity)
verifyContents expected directory = do
  let required =
        [ "e2e-result.json", "emulator-session.json", "e2e-modal-observation.json"
        , "e2e-render-stack-diagnostic.json", "e2e-render-stack-diagnostic-request.json"
        , "e2e-modal-watch-final.json", "relinked/kernel.elf", "relinked/ram-push-manifest.json"
        , "relinked/runtime.bin" ]
  missing <- fmap concat $ forM required $ \name -> do
    file <- doesFileExist (directory </> name)
    pure [name | not file]
  case missing of
    name : _ -> pure (Left ("receipt is partial; missing " ++ name))
    [] -> do
      resultValue <- jsonFile (directory </> "e2e-result.json")
      sessionValue <- jsonFile (directory </> "emulator-session.json")
      modalValue <- jsonFile (directory </> "e2e-modal-observation.json")
      diagnosticValue <- jsonFile (directory </> "e2e-render-stack-diagnostic.json")
      requestValue <- jsonFile (directory </> "e2e-render-stack-diagnostic-request.json")
      config <- readDiagnosticConfig (expectedConfig expected)
      observations <- observationFiles directory
      elfBytes <- B.readFile (directory </> "relinked/kernel.elf")
      case (config, ELF.parseElf elfBytes) of
        (Left problem, _) -> pure (Left problem)
        (_, Left problem) -> pure (Left ("relinked ELF: " ++ problem))
        (Right configValue', Right elf) -> do
          retained <- checkRetainedSources configValue'
          case retained of
            Left problem -> pure (Left problem)
            Right () -> case (resultValue, sessionValue, modalValue, diagnosticValue, requestValue, observations) of
              (Left problem, _, _, _, _, _) -> pure (Left problem)
              (_, Left problem, _, _, _, _) -> pure (Left problem)
              (_, _, Left problem, _, _, _) -> pure (Left problem)
              (_, _, _, Left problem, _, _) -> pure (Left problem)
              (_, _, _, _, Left problem, _) -> pure (Left problem)
              (_, _, _, _, _, Left problem) -> pure (Left problem)
              (Right result, Right session, Right modal, Right diagnostic, Right request, Right indices) ->
                case decodeReceipt expected configValue' elf result session modal diagnostic request indices of
                  Left problem -> pure (Left problem)
                  Right identity -> do
                    bound <- OS.loadReceiptObservationValues directory result
                    case bound of
                      Left problem -> pure (Left problem)
                      Right verified
                        | map (takeFileName . fst) verified
                            /= ["e2e-observation-" ++ show index ++ ".json" | index <- indices] ->
                            pure (Left "observation references do not match the receipt's numbered documents")
                        | otherwise -> do
                            hashes <- receiptHashes directory
                            case hashes of
                              Left problem -> pure (Left problem)
                              Right (kernel, resultSha, sessionSha, modalSha, diagnosticSha, requestSha, watchSha, manifestSha, runtimeSha) -> do
                                prelinked <- case (expectedRole expected, expectedPrelinked expected) of
                                  (OwnedRole, Just prelinkedDirectory) -> comparePrelinked prelinkedDirectory directory
                                  _ -> pure (Right ())
                                pure $ do
                                  prelinked
                                  pure identity
                                    { identityKernelElf = kernel
                                    , identityResult = resultSha
                                    , identitySession = sessionSha
                                    , identityModal = modalSha
                                    , identityDiagnostic = diagnosticSha
                                    , identityRequest = requestSha
                                    , identityWatchFinal = watchSha
                                    , identityManifest = manifestSha
                                    , identityRuntime = runtimeSha
                                    }

observationFiles :: FilePath -> IO (Either String [Int])
observationFiles directory = do
  names <- listDirectory directory
  let indices = [read digits :: Int
                | name <- names
                , Just rest <- [stripPrefix "e2e-observation-" name]
                , let digits = takeWhile isDigit rest
                , not (null digits)
                , drop (length digits) rest == ".json"]
      ordered = sort indices
  pure (if ordered == [0 .. length ordered - 1] && not (null ordered)
    then Right ordered
    else Left ("observation files are not one per segment: " ++ show ordered))

receiptHashes :: FilePath -> IO (Either String (Digest, Digest, Digest, Digest, Digest, Digest, Digest, Digest, Digest))
receiptHashes directory = do
  kernel <- digestFile (directory </> "relinked/kernel.elf")
  result <- digestFile (directory </> "e2e-result.json")
  session <- digestFile (directory </> "emulator-session.json")
  modal <- digestFile (directory </> "e2e-modal-observation.json")
  diagnostic <- digestFile (directory </> "e2e-render-stack-diagnostic.json")
  request <- digestFile (directory </> "e2e-render-stack-diagnostic-request.json")
  watch <- digestFile (directory </> "e2e-modal-watch-final.json")
  manifest <- digestFile (directory </> "relinked/ram-push-manifest.json")
  runtime <- digestFile (directory </> "relinked/runtime.bin")
  pure (Right (kernel, result, session, modal, diagnostic, request, watch, manifest, runtime))

decodeReceipt
  :: Expected -> DiagnosticConfig -> ELF.Elf -> J.Value -> J.Value -> J.Value -> J.Value -> J.Value -> [Int]
  -> Either String ReceiptIdentity
decodeReceipt expected config elf result session modal diagnostic request indices =
  J.parseEither (\_ -> do
    (outcome, trajectoryFailure, forbidden) <- J.withObject "e2e result" (\fields ->
      (,,) <$> (fields J..: "outcome" >>= parseQualification)
        <*> (fields J..:? "trajectory_failure" :: J.Parser (Maybe J.Value))
        <*> (fields J..:? "forbidden_execution" :: J.Parser (Maybe J.Value))) result
    let qualification = case expectedRole expected of
          StockRole -> CandidateReceipt
          OwnedRole -> ScopedLifetimeReceipt
    unless (outcome == qualification && trajectoryFailure == Nothing && forbidden == Nothing)
      (fail "receipt qualification does not match its admitted role")
    phases <- J.withObject "e2e result" (\fields ->
      fields J..: "phases" >>= mapM (J.withObject "phase" (J..: "name"))) result
    let segments = [T.drop 8 name | name <- phases, "segment " `T.isPrefixOf` name]
    unless (not (null segments) && length (nub segments) == length segments && length segments == length indices)
      (fail "receipt segment phases do not match its observation files")
    checkpoint <- J.withObject "emulator session" (\fields ->
      fields J..: "checkpoint" >>= J.withObject "checkpoint" (\c -> c J..: "directory")) session
    unless (not (null checkpoint)) (fail "receipt has no checkpoint identity")
    pins <- J.withObject "emulator session" (\fields ->
      fields J..: "inputs" >>= J.withObject "session inputs" (\inputs ->
        InputPins <$> inputs J..: "installed_dat" <*> inputs J..: "adjustment_nand" <*> inputs J..: "boot_rom"
          <*> inputs J..: "run_document" <*> inputs J..: "model" <*> inputs J..: "procedure")) session
    (verified, coverage, entries, variant, publications) <- J.withObject "modal observation" (\fields -> do
      verified <- fields J..: "task_identity_verified"
      coverage <- fields J..: "whole_installed_lifetime_coverage"
      permit <- fields J..: "permit_end"
      entries <- J.withObject "permit end" (\p -> p J..: "task_entries") permit :: J.Parser [J.Value]
      requestValue <- fields J..: "request"
      requestLifetime <- J.withObject "request" (\r -> r J..: "lifetime") requestValue
      variant <- J.withObject "lifetime" (\l -> l J..: "bank_variant") requestLifetime
      requested <- J.withObject "lifetime" (\l -> l J..: "publications") requestLifetime :: J.Parser [J.Value]
      completedLifetime <- J.withObject "permit end" (\p -> p J..: "lifetime") permit
      completed <- J.withObject "lifetime" (\l -> l J..: "publication_count") completedLifetime :: J.Parser Word32
      unless (fromIntegral (length requested) == completed) (fail "completed publication count differs from the request")
      pure (verified, coverage, length entries, variant, length requested)) modal
    unless coverage (fail "receipt lacks whole installed lifetime coverage")
    case expectedRole expected of
      StockRole -> do
        unless (not verified && entries == 0) (fail "stock receipt claims verified task identity")
        unless (variant == ("standard" :: T.Text) && publications == 223) (fail "stock receipt publication vector differs")
      OwnedRole -> do
        unless (verified && entries > 0) (fail "owned receipt lacks verified task identity")
        unless (variant == ("modal75,menu-engine,render" :: T.Text) && publications == 228)
          (fail "owned receipt publication vector differs")
    (scope, diagnosticOnly, wholeRoute, coverageText, finished, failure, active, incomplete, diagnosticRole) <-
      J.withObject "render diagnostic" (\fields -> do
        scope <- fields J..: "scope"
        diagnosticOnly <- fields J..: "diagnostic_only"
        wholeRoute <- fields J..: "whole_route_stack_minimum_proven"
        coverageText <- fields J..: "coverage"
        finished <- fields J..: "finished"
        failure <- fields J..: "failure" :: J.Parser (Maybe T.Text)
        active <- fields J..: "active_invocations" :: J.Parser Word32
        incomplete <- fields J..: "incomplete_intervals"
        role <- fields J..: "role"
        pure (scope, diagnosticOnly, wholeRoute, coverageText, finished, failure, active, incomplete, role)) diagnostic
    unless (scope == ("installed-before-unload" :: T.Text) && diagnosticOnly && not wholeRoute
      && coverageText == ("continuous" :: T.Text) && finished && failure == Nothing && active == 0 && not incomplete)
      (fail "render diagnostic is refused, unfinished or overclaimed")
    let expectedRoleText = roleText (expectedRole expected)
    unless (diagnosticRole == expectedRoleText) (fail "render diagnostic role differs")
    requestRole <- J.withObject "diagnostic request" (\fields -> fields J..: "role") request
    unless (requestRole == expectedRoleText) (fail "diagnostic request role differs")
    requestConfig <- J.parseJSON request
    unless (sameConfig (config {configRole = expectedRole expected}) (requestConfig :: DiagnosticConfig))
      (fail "diagnostic request differs from the admitted configuration")
    checkConfigSources config elf
    pure (ReceiptIdentity
      { identityName = expectedName expected
      , identityRole = expectedRole expected
      , identityPhases = segments
      , identityObservationCount = length indices
      , identityPins = pins
      , identityCheckpoint = CheckpointId (T.pack (takeFileName checkpoint))
      , identityKernelElf = digestBytes B.empty
      , identityResult = digestBytes B.empty
      , identitySession = digestBytes B.empty
      , identityModal = digestBytes B.empty
      , identityDiagnostic = digestBytes B.empty
      , identityRequest = digestBytes B.empty
      , identityWatchFinal = digestBytes B.empty
      , identityManifest = digestBytes B.empty
      , identityRuntime = digestBytes B.empty
      , identityTaskIdentityVerified = verified
      , identityTaskEntries = entries
      , identityBankVariant = variant
      , identityPublications = publications
      })) J.Null

sameConfig :: DiagnosticConfig -> DiagnosticConfig -> Bool
sameConfig left right =
  configRole left == configRole right
    && sort (configSources left) == sort (configSources right)
    && sortOn entryKey (configEntries left) == sortOn entryKey (configEntries right)
    && configReturns left == configReturns right
    && configRetained left == configRetained right

-- | Source paths are presentation; only kind, start and bytes are compared.
sameAdmittedConfig :: DiagnosticConfig -> DiagnosticConfig -> Bool
sameAdmittedConfig left right =
  configRole left == configRole right
    && sort [(sourceKind s, sourceStart s, sourceSha s) | s <- configSources left]
      == sort [(sourceKind s, sourceStart s, sourceSha s) | s <- configSources right]
    && sortOn entryKey (configEntries left) == sortOn entryKey (configEntries right)
    && configReturns left == configReturns right
    && configRetained left == configRetained right

-- | Retained source files must still hold their recorded bytes.
checkRetainedSources :: DiagnosticConfig -> IO (Either String ())
checkRetainedSources config = runChecks
  [ do
      present <- doesFileExist (sourcePath source)
      if not present then pure (Left ("retained diagnostic source is missing: " ++ sourcePath source)) else do
        actual <- digestFile (sourcePath source)
        pure (if actual == sourceSha source then Right () else Left
          ("retained diagnostic source " ++ sourcePath source ++ " differs from its recorded bytes"))
  | source <- configSources config, sourceKind source == RetainedSource ]

-- | The owned sources must be exactly the role ELF's executable sections, as
-- 'Replacement.ModalRuntime.RenderStackEvidence.checkOwnedSources' requires.
checkConfigSources :: DiagnosticConfig -> ELF.Elf -> J.Parser ()
checkConfigSources config elf = do
  let expected = sort [(E.StackAddress (ELF.sectionAddress section), digestBytes (sectionBytes section))
                      | section <- ELF.elfSections elf, ELF.sectionExecutable section, ELF.sectionSize section > 0]
      actual = sort [(sourceStart source, sourceSha source) | source <- configSources config, sourceKind source == OwnedSource]
  unless (actual == expected) (fail "diagnostic owned sources differ from the linked role ELF")
 where
  sectionBytes section = B.take (fromIntegral (ELF.sectionSize section))
    (B.drop (fromIntegral (ELF.sectionOffset section)) (ELF.elfBytes elf))

-- Review packet ------------------------------------------------------------------------------

data ReviewPacket = ReviewPacket
  { packetPassed :: Bool
  , packetSourceCommit :: CommitId
  , packetSdk :: FilePath
  , packetTrajectory :: FilePath
  , packetTrajectorySha :: Digest
  , packetConfig :: FilePath
  , packetConfigSha :: Digest
  , packetTerminal :: FilePath
  , packetTerminalSha :: Digest
  , packetEvidence :: [(FilePath, Digest)]
  }

instance J.FromJSON ReviewPacket where
  parseJSON = J.withObject "review packet" $ \fields -> do
    passed <- fields J..: "passed"
    source <- fields J..: "source_commit"
    sdk <- fields J..: "sdk_package"
    trajectory <- fields J..: "trajectory" >>= J.withObject "trajectory" (\t -> (,) <$> t J..: "path" <*> t J..: "sha256")
    config <- fields J..: "config" >>= J.withObject "config" (\c -> (,) <$> c J..: "path" <*> c J..: "sha256")
    terminal <- fields J..: "terminal" >>= J.withObject "terminal" (\t -> (,) <$> t J..: "path" <*> t J..: "sha256")
    evidence <- fields J..: "evidence" >>= J.withObject "evidence" (\e -> forM ["result", "diagnostic", "session"] $ \key ->
      e J..: key >>= J.withObject "evidence file" (\file -> (,) <$> file J..: "path" <*> file J..: "sha256"))
    pure (ReviewPacket passed source sdk (fst trajectory) (snd trajectory) (fst config) (snd config)
      (fst terminal) (snd terminal) evidence)

packetProblems :: Expected -> ReviewPacket -> [String]
packetProblems expected packet =
  [ "review packet did not pass" | not (packetPassed packet) ]
    ++ [ "review packet source commit differs" | not (commitMatches (packetSourceCommit packet) (expectedSourceCommit expected)) ]
    ++ [ "review packet SDK differs" | packetSdk packet /= expectedSdk expected ]
    ++ [ "review packet trajectory differs" | packetTrajectorySha packet /= expectedTrajectorySha expected ]
    ++ [ "review packet configuration differs" | packetConfigSha packet /= expectedConfigSha expected ]
 where
  commitMatches (CommitId recorded) (CommitId wanted) = recorded == wanted
    || (T.length recorded >= 8 && recorded `T.isPrefixOf` wanted)
    || (T.length wanted >= 8 && wanted `T.isPrefixOf` recorded)

verifyPacketEvidence :: ReviewPacket -> FilePath -> IO (Either String ())
verifyPacketEvidence packet receipt = runChecks
  ( checkDigest "packet trajectory" (packetTrajectory packet) (packetTrajectorySha packet)
  : checkDigest "packet configuration" (packetConfig packet) (packetConfigSha packet)
  : checkDigest "packet terminal" (packetTerminal packet) (packetTerminalSha packet)
  : [ verifyEvidence path sha | (path, sha) <- packetEvidence packet ]
  )
 where
  verifyEvidence path sha = do
    let name = takeFileName path
        actual = receipt </> name
    present <- doesFileExist actual
    if not present then pure (Left ("packet evidence file is not in the receipt: " ++ name))
      else do
        digest <- digestFile actual
        pure (if digest == sha then Right () else Left ("packet evidence " ++ name ++ " differs from the receipt"))

-- Owned configuration ------------------------------------------------------------------------

-- | Frame depths and owned sources come from the relinked owned ELF; returns and retained services
-- come from the stock configuration. Retained images come from the owned retained configuration,
-- whose hook-patched images carry the owned entry branches.
writeOwnedConfig :: FilePath -> FilePath -> Word32 -> FilePath -> FilePath -> FilePath -> IO (Either String ())
writeOwnedConfig elfPath manifestPath base stockConfigPath retainedConfigPath outPath = do
  bytes <- B.readFile elfPath
  manifest <- jsonFile manifestPath
  stock <- readDiagnosticConfig stockConfigPath
  retainedConfig <- readDiagnosticConfig retainedConfigPath
  case (ELF.parseElf bytes, manifest, stock, retainedConfig) of
    (Left problem, _, _, _) -> pure (Left ("owned ELF: " ++ problem))
    (_, Left problem, _, _) -> pure (Left ("owned manifest: " ++ problem))
    (_, _, Left problem, _) -> pure (Left ("stock configuration: " ++ problem))
    (_, _, _, Left problem) -> pure (Left ("retained configuration: " ++ problem))
    (Right elf, Right manifestValue, Right stockConfig, Right retained) ->
      case J.parseEither (J.withObject "manifest" (\fields -> (fields J..: "size" :: J.Parser Word32) <|> fields J..: "bytes")) manifestValue of
        Left problem -> pure (Left ("owned manifest: " ++ problem))
        Right allocationSize -> case ownedConfigFromElf elf allocationSize base stockConfig retained of
          Left problem -> pure (Left problem)
          Right (owned, sections) -> do
            retainedCheck <- checkRetainedSources owned
            case retainedCheck of
              Left problem -> pure (Left problem)
              Right () -> do
                let retainedSources = [source | source <- configSources owned, sourceKind source == RetainedSource]
                exists <- doesFileExist outPath
                if exists
                  then do
                    previous <- readDiagnosticConfig outPath
                    pure $ case previous of
                      Right prior | sameAdmittedConfig prior owned -> Right ()
                      _ -> Left ("refusing to replace the existing diagnostic configuration " ++ outPath)
                  else do
                    createDirectoryIfMissing True (takeDirectory outPath)
                    sourceDirectory <- makeAbsolute (takeDirectory outPath </> "owned-sources")
                    createDirectoryIfMissing True sourceDirectory
                    ownedSources <- forM sections $ \(address, sectionBytes) -> do
                      let path = sourceDirectory </> ("section-" ++ showHexWord address ++ ".bin")
                      B.writeFile path sectionBytes
                      pure (DiagnosticSource OwnedSource (E.StackAddress address) (digestBytes sectionBytes) path)
                    let final = owned { configSources = retainedSources ++ ownedSources }
                    writeAtomic outPath (BL.toStrict (J.encode (configValue final)) <> "\n")
                    pure (Right ())

-- | Depths and owned sources come from the ELF; returns and services from the stock configuration;
-- retained images from the retained configuration. The executable section bytes are returned for
-- the caller to write.
ownedConfigFromElf :: ELF.Elf -> Word32 -> Word32 -> DiagnosticConfig -> DiagnosticConfig -> Either String (DiagnosticConfig, [(Word32, B.ByteString)])
ownedConfigFromElf elf allocationSize base stock retainedConfig = do
  let inRuntime address = address >= base && toInteger address < toInteger base + toInteger allocationSize
  report <- R.checkRenderStack elf inRuntime
  let depths = [ (E.TreeStack, R.entryDepth (R.reportTree report))
               , (E.PickerStack, R.entryDepth (R.reportPicker report))
               , (E.PaneStack, R.entryDepth (R.reportPane report)) ]
      declared = [(entryKind entry, depth) | entry <- configEntries stock, E.FrameBytes depth <- [entryDepth entry]]
  unless (sort depths == sort declared) (Left
    ("owned ELF depths " ++ show depths ++ " differ from the frozen stock receipt's " ++ show declared))
  let sections = [(ELF.sectionAddress section, sectionBytes section)
                 | section <- ELF.elfSections elf, ELF.sectionExecutable section, ELF.sectionSize section > 0]
      ownedSources = [DiagnosticSource OwnedSource (E.StackAddress address) (digestBytes bytes) "" | (address, bytes) <- sections]
  unless (not (null ownedSources)) (Left "owned ELF has no executable section")
  let retained = [source | source <- configSources retainedConfig, sourceKind source == RetainedSource]
  pure ( stock
    { configRole = OwnedRole
    , configSources = retained ++ ownedSources
    , configEntries = [entry { entryDepth = E.FrameBytes depth } | entry <- configEntries stock, Just depth <- [lookup (entryKind entry) depths]]
    }
    , sections )
 where
  sectionBytes section = B.take (fromIntegral (ELF.sectionSize section))
    (B.drop (fromIntegral (ELF.sectionOffset section)) (ELF.elfBytes elf))

showHexWord :: Word32 -> String
showHexWord value = let text = showHex value "" in replicate (8 - length text) '0' ++ text

-- Utility ------------------------------------------------------------------------------------

writeAtomic :: FilePath -> B.ByteString -> IO ()
writeAtomic path bytes = do
  let temporary = path ++ ".tmp"
  removeIfPresent temporary
  B.writeFile temporary bytes
  renameFile temporary path

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = removeFile path `catchIOError` \problem ->
  unless (isDoesNotExistError problem) (ioError problem)
