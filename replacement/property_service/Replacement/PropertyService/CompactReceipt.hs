{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
module Replacement.PropertyService.CompactReceipt
  ( Role (..), ObservationIdentity (..), ArtifactIdentity (..), ArtifactId (..), ArtifactPath (..), ArtifactSize (..)
  , SDKStatus (..), SDKMismatch (..), DiagnosticText (..), ReportOrigin (..), ReportProfile (..)
  , SDKReport (..), ObservationReceipt (..), PairReceipt (..)
  , MismatchDiagnostic (..), DiagnosticClass (..), DiagnosticDisposition (..)
  , observationRequiresFull, observationDiagnosticClass
  , parseObservationReceipt, observationRun, observationPassed, observationSDKStatus
  , pairReceipt, pairPassed, encodePairReceipt
  ) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import Data.Aeson.Types (Pair, ToJSON)
import Data.Scientific (toBoundedInteger)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.List (nub, stripPrefix)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Numeric (readHex)
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import qualified Replacement.PropertyService.Oracle as O

data Role = StockRole | OwnedRole deriving (Eq, Show)
newtype ArtifactId = ArtifactId String deriving (Eq, Show)
newtype ArtifactPath = ArtifactPath FilePath deriving (Eq, Show)
newtype ArtifactSize = ArtifactSize Word64 deriving (Eq, Show)
newtype DiagnosticText = DiagnosticText String deriving (Eq, Show)
data ArtifactIdentity = ArtifactIdentity ArtifactId ArtifactPath ArtifactSize Sha256 deriving Eq
data ObservationIdentity = ObservationIdentity
  { identityCase :: C.CaseId, identityRunner :: Sha256, identityCaseSHA :: Sha256
  , identityWireSHA :: Sha256, identityArtifacts :: [ArtifactIdentity]
  } deriving Eq

data SDKStatus = SDKPassed | SDKFailed deriving (Eq, Show)
data SDKMismatch = SDKStatusMismatch | SDKRegisterMismatch N.Register | SDKMemoryMismatch N.RegionId
  | SDKStubEventsMismatch | SDKKernelMismatch N.SystemRegister | SDKReservedMismatch | SDKUnknownMismatch deriving (Eq, Show)
data ReportOrigin = StockOrigin | SyntheticOrigin deriving (Eq, Show)
data ReportProfile = StockBounded | SyntheticOnly deriving (Eq, Show)
data SDKReport = SDKReport
  { sdkStatus :: SDKStatus, sdkMismatches :: [SDKMismatch], sdkExactMismatches :: [MismatchDiagnostic], sdkStderr :: DiagnosticText
  , sdkRunnerExit :: Int, sdkOrigin :: ReportOrigin, sdkProfile :: ReportProfile
  , sdkProvenance :: DiagnosticText, sdkExclusions :: [DiagnosticText]
  } deriving (Eq, Show)
newtype MismatchDiagnostic = MismatchDiagnostic String deriving (Eq, Ord, Show)
data DiagnosticClass = CleanDiagnostic | UnsupportedSourceBounds | FullDiagnostic [MismatchDiagnostic] deriving (Eq, Ord, Show)
data DiagnosticDisposition = CleanDisposition | FullRawRequired deriving (Eq, Show)
data ObservationReceipt = ObservationReceipt
  { observationIdentity :: ObservationIdentity
  , observationRawRun :: O.Run
  , observationClaimsR3 :: Bool
  , observationNative :: N.NativeObservation
  , observationPhysicalWrites :: [O.WriteEvent]
  , observationSDK :: SDKReport
  , observationRawSHA :: Sha256
  , observationDisposition :: DiagnosticDisposition
  , observationMetadata :: RunnerMetadata
  } deriving Eq

data PairReceipt = PairReceipt
  { pairStock :: ObservationReceipt, pairOwned :: ObservationReceipt
  , pairModel :: Maybe O.Expectation
  , pairStockModelDifferences :: [O.Difference], pairOwnedModelDifferences :: [O.Difference]
  , pairDifferences :: [O.Difference]
  } deriving Eq

field :: String -> Value -> Either String Value
field key (Object fields) = maybe (Left ("missing " ++ key)) Right (KM.lookup (K.fromString key) fields)
field key _ = Left ("object required for " ++ key)
text :: Value -> Either String String
text (String value) = Right (T.unpack value)
text _ = Left "string required"
array :: Value -> Either String [Value]
array (Array values) = Right (foldr (:) [] values)
array _ = Left "array required"
stringField :: String -> Value -> Either String String
stringField key value = field key value >>= text
hashField :: String -> Value -> Either String Sha256
hashField key value = stringField key value >>= parseSha256Hex
validId :: String -> Bool
validId (first:rest) = isAsciiLower first && length rest <= 63 && all (\c -> isAsciiLower c || isDigit c || c == '-') rest
validId [] = False

parseObservationReceipt :: Bool -> Sha256 -> C.CaseId -> [(String, Value)] -> N.NativeObservation -> Value -> Either String ObservationReceipt
parseObservationReceipt claimR3 runner ident identity native report = do
  knownFields ["schema_version","profile","case_sha256","wire_sha256","artifacts","case_id","origin","provenance","exclusions","runner_sha256","status","runner","runner_exit","mismatches","stderr"] report
  unless (validId (C.caseIdText ident)) (Left "invalid case identifier")
  let identityValue = object [K.fromString key .= value | (key, value) <- identity]
  unless (length (nub (map fst identity)) == length identity) (Left "duplicate identity field")
  caseSHA <- hashField "case_sha256" identityValue
  wireSHA <- hashField "wire_sha256" identityValue
  artifacts <- field "artifacts" identityValue >>= array >>= mapM artifact
  unless (length (nub [name | ArtifactIdentity name _ _ _ <- artifacts]) == length artifacts) (Left "duplicate artifact identifier")
  reportCase <- stringField "case_id" report
  reportRunner <- hashField "runner_sha256" report
  reportCaseSHA <- hashField "case_sha256" report
  reportWireSHA <- hashField "wire_sha256" report
  reportArtifacts <- field "artifacts" report >>= array >>= mapM artifact
  unless (reportCase == C.caseIdText ident && reportRunner == runner && reportCaseSHA == caseSHA && reportWireSHA == wireSHA && reportArtifacts == artifacts) (Left "report identity differs from admitted case")
  sdk <- parseSDK report
  writes <- O.reportWrites report
  rawRun <- O.runOf native writes
  metadata <- field "runner" report >>= parseMetadata
  pure (ObservationReceipt (ObservationIdentity ident runner caseSHA wireSHA artifacts) rawRun claimR3 native writes sdk (sha256 (encodeCanonical report)) (if sdkStatus sdk == SDKPassed then CleanDisposition else FullRawRequired) metadata)
  where
    artifact value = do
      name <- stringField "id" value
      path <- stringField "path" value
      unless (validId name && not (null path) && '\0' `notElem` path) (Left "invalid artifact identity")
      sizeValue <- field "size" value
      size <- case sizeValue of
        Number n | Just count <- (toBoundedInteger n :: Maybe Word64), count > 0 -> Right count
        _ -> Left "invalid artifact size"
      digest <- hashField "sha256" value
      pure (ArtifactIdentity (ArtifactId name) (ArtifactPath path) (ArtifactSize size) digest)

parseSDK :: Value -> Either String SDKReport
parseSDK report = do
  status <- stringField "status" report >>= \case
    "passed" -> Right SDKPassed
    "failed" -> Right SDKFailed
    _ -> Left "SDK report has no native judgment"
  exact <- field "mismatches" report >>= array >>= mapM text
  let mismatches = map mismatch exact
  unless ((status == SDKPassed) == null mismatches) (Left "SDK status/mismatches inconsistent")
  stderr <- DiagnosticText <$> stringField "stderr" report
  exit <- field "runner_exit" report >>= \case
    Number n | Just code <- (toBoundedInteger n :: Maybe Int) -> Right code
    _ -> Left "invalid runner exit"
  origin <- stringField "origin" report >>= \case
    "stock" -> Right StockOrigin
    "synthetic" -> Right SyntheticOrigin
    _ -> Left "invalid report origin"
  profile <- stringField "profile" report >>= \case
    "stock-bounded" -> Right StockBounded
    "synthetic-only" -> Right SyntheticOnly
    _ -> Left "invalid report profile"
  unless ((origin == StockOrigin) == (profile == StockBounded)) (Left "report origin/profile inconsistent")
  provenance <- DiagnosticText <$> stringField "provenance" report
  exclusions <- field "exclusions" report >>= array >>= mapM (fmap DiagnosticText . text)
  pure (SDKReport status mismatches (map MismatchDiagnostic exact) stderr exit origin profile provenance exclusions)
  where
    mismatch "status differs" = SDKStatusMismatch
    mismatch "stub events differ" = SDKStubEventsMismatch
    mismatch message
      | Just rest <- stripPrefix "register " message, Just name <- stripSuffix ": differs" rest
      , Just register <- N.registerFromLabel name = SDKRegisterMismatch register
      | Just rest <- stripPrefix "memory " message, Just name <- stripSuffix ": differs from exact allowed final bytes" rest
      , validId name = SDKMemoryMismatch (N.RegionId name)
    mismatch "kernel register tpidrurw: differs" = SDKKernelMismatch N.Tpidrurw
    mismatch "kernel register fpexc: differs" = SDKKernelMismatch N.Fpexc
    mismatch "kernel register reserved: differs" = SDKReservedMismatch
    mismatch _ = SDKUnknownMismatch

stripSuffix :: Eq a => [a] -> [a] -> Maybe [a]
stripSuffix suffix value = reverse <$> stripPrefix (reverse suffix) (reverse value)

observationRequiresFull :: ObservationReceipt -> Bool
observationRequiresFull receipt = not (observationPassed receipt) || observationDisposition receipt == FullRawRequired

observationDiagnosticClass :: ObservationReceipt -> DiagnosticClass
observationDiagnosticClass receipt = case observationDisposition receipt of
  CleanDisposition -> CleanDiagnostic
  FullRawRequired
    | sdkExactMismatches (observationSDK receipt) == [MismatchDiagnostic "memory stack: differs from exact allowed final bytes"] -> UnsupportedSourceBounds
    | otherwise -> FullDiagnostic (sdkExactMismatches (observationSDK receipt))

observationRun :: ObservationReceipt -> O.Run
observationRun receipt = raw { O.runRegisters = filter (\(register, _) -> register /= N.R3 || observationClaimsR3 receipt) (O.runRegisters raw) }
  where raw = observationRawRun receipt

observationSDKStatus :: ObservationReceipt -> SDKStatus
observationSDKStatus = sdkStatus . observationSDK

observationPassed :: ObservationReceipt -> Bool
observationPassed receipt = N.nativeStatus native == N.Returned && N.nativeEngineError native == N.EngineError 0
  && sdkRunnerExit (observationSDK receipt) == 0 && case N.nativeProvenance native of
    N.Complete _ _ -> True
    N.Truncated _ -> False
  where native = observationNative receipt

pairReceipt :: Maybe O.Expectation -> ObservationReceipt -> ObservationReceipt -> PairReceipt
pairReceipt model stock owned = PairReceipt stock owned model (differences stock) (differences owned) (O.compareRuns (observationRun stock) (observationRun owned))
  where differences receipt = maybe [] (\expected -> O.expectationDifferences expected (observationRun receipt)) model

pairPassed :: PairReceipt -> Bool
pairPassed receipt = observationPassed (pairStock receipt) && observationPassed (pairOwned receipt)
  && null (pairStockModelDifferences receipt) && null (pairOwnedModelDifferences receipt) && null (pairDifferences receipt)

encodePairReceipt :: PairReceipt -> B.ByteString
encodePairReceipt receipt = encodeCanonical $ object
  [ "schema_version" .= (1 :: Int), "stock" .= observationJSON StockRole (pairStock receipt)
  , "owned" .= observationJSON OwnedRole (pairOwned receipt), "model" .= fmap expectationJSON (pairModel receipt)
  , "stock_model_differences" .= map differenceJSON (pairStockModelDifferences receipt)
  , "owned_model_differences" .= map differenceJSON (pairOwnedModelDifferences receipt)
  , "comparison_differences" .= map differenceJSON (pairDifferences receipt), "public_consumer_agreement" .= pairPassed receipt ]

observationJSON :: Role -> ObservationReceipt -> Value
observationJSON role receipt = object
  [ "role" .= show role, "identity" .= identityJSON (observationIdentity receipt)
  , "raw_report_sha256" .= sha256Hex (observationRawSHA receipt), "claims_r3" .= observationClaimsR3 receipt
  , "raw_run" .= runJSON (observationRawRun receipt), "comparison_run" .= runJSON (observationRun receipt)
  , "physical_writes" .= map writeJSON (observationPhysicalWrites receipt)
  , "native" .= nativeJSON (observationNative receipt), "sdk" .= sdkJSON (observationSDK receipt)
  , "runner_metadata" .= metadataJSON (observationMetadata receipt)
  , "diagnostic_disposition" .= dispositionJSON (observationDisposition receipt)
  , "diagnostic_explanation" .= show (observationDiagnosticClass receipt)
  , "native_observation_complete_and_returned" .= observationPassed receipt ]

identityJSON :: ObservationIdentity -> Value
identityJSON identity = object
  [ "case_id" .= C.caseIdText (identityCase identity), "runner_sha256" .= sha256Hex (identityRunner identity)
  , "case_sha256" .= sha256Hex (identityCaseSHA identity), "wire_sha256" .= sha256Hex (identityWireSHA identity)
  , "artifacts" .= [object ["id" .= name, "path" .= path, "size" .= size, "sha256" .= sha256Hex digest]
      | ArtifactIdentity (ArtifactId name) (ArtifactPath path) (ArtifactSize size) digest <- identityArtifacts identity] ]
registersJSON :: ToJSON a => [(N.Register, a)] -> [Value]
registersJSON = map (\(register, value) -> object ["register" .= N.registerLabel register, "value" .= value])
memoryJSON :: [(N.RegionId, B.ByteString)] -> [Value]
memoryJSON = map (\(N.RegionId name, bytes) -> object ["region" .= name, "bytes" .= B.unpack bytes])
runJSON :: O.Run -> Value
runJSON run = object ["status" .= N.runnerStatusLabel (O.runStatus run), "registers" .= registersJSON (O.runRegisters run), "memory" .= memoryJSON (O.runMemory run), "writes" .= fmap (map writeJSON) (O.runWrites run)]
expectationJSON :: O.Expectation -> Value
expectationJSON expected = object ["status" .= N.runnerStatusLabel (O.expectedStatus expected), "registers" .= registersJSON (O.expectedRegisters expected), "memory" .= memoryJSON (O.expectedMemory expected)]
writeJSON :: O.WriteEvent -> Value
writeJSON event = object ["address" .= O.eventAddress event, "width" .= O.eventWidth event, "value" .= O.eventValue event]

nativeJSON :: N.NativeObservation -> Value
nativeJSON native = object
  [ "status" .= N.runnerStatusLabel (N.nativeStatus native), "registers" .= registersJSON (M.toAscList (N.nativeRegisters native))
  , "memory" .= memoryJSON (N.nativeMemory native), "instructions" .= N.instructionCount (N.nativeInstructions native)
  , "engine_error" .= N.engineError (N.nativeEngineError native)
  , "provenance" .= case N.nativeProvenance native of
      N.Complete total writes -> object ["complete" .= True, "total" .= N.provenanceTotal total, "writes" .= map nativeWriteJSON writes]
      N.Truncated total -> object ["complete" .= False, "total" .= N.provenanceTotal total]
  , "kernel" .= fmap kernelJSON (N.nativeKernel native) ]
  where
    nativeWriteJSON (N.Write (N.GuestAddress address) (N.WriteBytes width)) = object ["address" .= address, "width" .= width]
    kernelJSON kernel = object
      [ "order" .= fmap (map orderedJSON) (N.observedOrder kernel), "tpidrurw" .= N.observedTpidrurw kernel
      , "fpexc" .= N.observedFpexc kernel, "reserved" .= fmap N.guestAddress (N.observedReserved kernel) ]
    orderedJSON event = case event of
      N.OrderedWrite write (N.WriteValue value) -> object ["kind" .= ("write" :: String), "write" .= nativeWriteJSON write, "value" .= value]
      N.OrderedBarrier barrier -> object ["kind" .= ("barrier" :: String), "barrier" .= show barrier]
      N.OrderedSystemWrite register value -> object ["kind" .= ("system_write" :: String), "register" .= show register, "value" .= value]

sdkJSON :: SDKReport -> Value
sdkJSON sdk = object
  [ "status" .= (if sdkStatus sdk == SDKPassed then ("passed" :: String) else "failed")
  , "mismatches" .= [message | MismatchDiagnostic message <- sdkExactMismatches sdk]
  , "classified_mismatches" .= map mismatchJSON (sdkMismatches sdk)
  , "stderr" .= diagnostic (sdkStderr sdk), "runner_exit" .= sdkRunnerExit sdk
  , "origin" .= show (sdkOrigin sdk), "profile" .= show (sdkProfile sdk)
  , "provenance" .= diagnostic (sdkProvenance sdk), "exclusions" .= map diagnostic (sdkExclusions sdk) ]
  where diagnostic (DiagnosticText value) = value

mismatchJSON :: SDKMismatch -> Value
mismatchJSON mismatch = case mismatch of
  SDKStatusMismatch -> tagged "status" []
  SDKRegisterMismatch register -> tagged "register" ["register" .= N.registerLabel register]
  SDKMemoryMismatch (N.RegionId name) -> tagged "memory" ["region" .= name]
  SDKStubEventsMismatch -> tagged "stub_events" []
  SDKKernelMismatch register -> tagged "kernel_register" ["register" .= show register]
  SDKReservedMismatch -> tagged "reserved" []
  SDKUnknownMismatch -> tagged "unknown" []

differenceJSON :: O.Difference -> Value
differenceJSON difference = case difference of
  O.StatusDiffers expected actual -> tagged "status" ["expected" .= N.runnerStatusLabel expected, "actual" .= N.runnerStatusLabel actual]
  O.RegisterDiffers register expected actual -> tagged "register" ["register" .= N.registerLabel register, "expected" .= expected, "actual" .= actual]
  O.MemoryDiffers (N.RegionId name) -> tagged "memory" ["region" .= name]
  O.WritesDiffer -> tagged "writes" []
  O.ReferenceTruncated -> tagged "reference_truncated" []
  O.SubjectTruncated -> tagged "subject_truncated" []
  O.ReferenceWriteProjectionFailed problem -> tagged "reference_projection" ["error" .= projectionJSON problem]
  O.SubjectWriteProjectionFailed problem -> tagged "subject_projection" ["error" .= projectionJSON problem]
  O.ModelOutsideFixture -> tagged "model_outside_fixture" []
  where
    projectionJSON problem = case problem of
      O.InvalidWriteWidth event -> tagged "invalid_width" ["event" .= writeJSON event]
      O.WriteAddressOverflow event -> tagged "address_overflow" ["event" .= writeJSON event]
      O.HeaderBoundaryCrossing header event -> tagged "header_crossing" ["header" .= show header, "event" .= writeJSON event]

tagged :: String -> [Pair] -> Value
tagged kind fields = object (("kind" .= kind) : fields)

dispositionJSON :: DiagnosticDisposition -> Value
dispositionJSON disposition = case disposition of
  CleanDisposition -> tagged "clean" []
  FullRawRequired -> tagged "full_raw_required" []

newtype NativeCount = NativeCount Word64 deriving (Eq, Show)
newtype NativeInstruction = NativeInstruction Word32 deriving (Eq, Show)
data InstructionSite = InstructionSite N.GuestAddress NativeInstruction deriving (Eq, Show)
data AccessKind = AccessRead | AccessWrite | ReadUnmapped | WriteUnmapped | FetchUnmapped | WriteProtected | ReadProtected | FetchProtected deriving (Eq, Show)
data MemoryAccess = MemoryAccess AccessKind N.GuestAddress Int Word64 deriving (Eq, Show)
data ProvenanceEvent = ProvenanceRead InstructionSite N.GuestAddress Int Word64 | ProvenanceWrite InstructionSite N.GuestAddress Int Word64
  | ProvenanceBarrier InstructionSite N.Barrier | ProvenanceSystem InstructionSite N.SystemRegister Word32 deriving (Eq, Show)
data StubEvent = StubEvent N.GuestAddress [(N.Register, Word32)] [(N.Register, Word32)] deriving (Eq, Show)
data RunnerMetadata = RunnerMetadata NativeCount NativeCount [InstructionSite] NativeCount [MemoryAccess] [ProvenanceEvent] [StubEvent] (Maybe DiagnosticText) [N.GuestAddress] deriving (Eq, Show)

knownFields :: [String] -> Value -> Either String ()
knownFields names (Object fields) = unless (all ((`elem` names) . K.toString) (KM.keys fields)) (Left "unknown native metadata field requires full raw")
knownFields _ _ = Left "native metadata object required"

unsigned :: Value -> Either String Word64
unsigned (Number n) | Just value <- toBoundedInteger n = Right value
unsigned (String value)
  | not (T.null value), T.all (\c -> c >= '0' && c <= '9') value
  , [(count, "")] <- reads (T.unpack value) = Right count
  | otherwise = Left "native count"
unsigned _ = Left "native count"

wide :: Value -> Either String Word64
wide value = do
  encoded <- text value
  case stripPrefix "0x" encoded of
    Just digits | not (null digits), length digits <= 16, [(number, "")] <- readHex digits -> Right number
    _ -> Left "native hexadecimal value"

word :: Value -> Either String Word32
word value = do
  parsed <- wide value
  if parsed <= fromIntegral (maxBound :: Word32) then Right (fromIntegral parsed) else Left "native word overflow"

parseMetadata :: Value -> Either String RunnerMetadata
parseMetadata runner = do
  knownFields ["wire_version","status","detail","unicorn_error","instructions","elapsed_ns","registers","memory","events","trace_total","trace","access_total","accesses","memory_provenance_total","memory_provenance","kernel"] runner
  version <- field "wire_version" runner >>= unsigned
  unless (version == 1) (Left "unsupported native wire version")
  case runner of
    Object fields -> case KM.lookup "kernel" fields of
      Nothing -> pure ()
      Just kernel -> knownFields ["tpidrurw","fpexc","reserved"] kernel
    _ -> Left "runner object required"
  elapsed <- count "elapsed_ns"
  traceTotal <- count "trace_total"
  traces <- rows "trace" trace
  accessTotal <- count "access_total"
  accesses <- rows "accesses" access
  provenance <- rows "memory_provenance" provenanceEvent
  events <- rows "events" event
  memories <- rows "memory" finalMemory
  detail <- case runner of
    Object fields -> traverse (fmap DiagnosticText . text) (KM.lookup "detail" fields)
    _ -> Left "runner object required"
  pure (RunnerMetadata elapsed traceTotal traces accessTotal accesses provenance events detail memories)
  where
    count name = NativeCount <$> (field name runner >>= unsigned)
    rows name parser = field name runner >>= array >>= mapM parser
    address name value = N.GuestAddress . fromIntegral <$> (field name value >>= word)
    size value = do
      n <- field "size" value >>= unsigned
      if n <= fromIntegral (maxBound :: Int) then Right (fromIntegral n) else Left "native access size"
    site value = InstructionSite <$> address "pc" value <*> (NativeInstruction <$> (field "instruction" value >>= word))
    trace value = knownFields ["pc","instruction"] value >> site value
    finalMemory value = knownFields ["address","bytes"] value >> address "address" value
    access value = do
      knownFields ["type","address","size","value"] value
      kind <- field "type" value >>= unsigned >>= \case
        16 -> Right AccessRead
        17 -> Right AccessWrite
        19 -> Right ReadUnmapped
        20 -> Right WriteUnmapped
        21 -> Right FetchUnmapped
        22 -> Right WriteProtected
        23 -> Right ReadProtected
        24 -> Right FetchProtected
        _ -> Left "unknown native access kind"
      MemoryAccess kind <$> address "address" value <*> size value <*> (field "value" value >>= wide)
    provenanceEvent value = do
      location <- site value
      kind <- stringField "kind" value
      case kind of
        "read" -> memory ProvenanceRead location value
        "write" -> memory ProvenanceWrite location value
        "barrier" -> do
          knownFields ["pc","instruction","kind","barrier"] value
          barrier <- stringField "barrier" value >>= \case
            "dmb" -> Right N.Dmb
            "dsb" -> Right N.Dsb
            "isb" -> Right N.Isb
            _ -> Left "unknown native barrier"
          pure (ProvenanceBarrier location barrier)
        "system" -> do
          knownFields ["pc","instruction","kind","register","value"] value
          register <- stringField "register" value >>= \case
            "tpidrurw" -> Right N.Tpidrurw
            "fpexc" -> Right N.Fpexc
            _ -> Left "unknown native system register"
          ProvenanceSystem location register <$> (field "value" value >>= word)
        _ -> Left "unknown native provenance kind"
    memory constructor location value = do
      knownFields ["pc","instruction","kind","address","size","value"] value
      constructor location <$> address "address" value <*> size value <*> (field "value" value >>= wide)
    event value = do
      knownFields ["address","inputs","outputs"] value
      StubEvent <$> address "address" value <*> (field "inputs" value >>= bindings) <*> (field "outputs" value >>= bindings)
    bindings (Object fields) = mapM binding (KM.toList fields)
    bindings _ = Left "native event bindings required"
    binding (key, value) = do
      register <- maybe (Left "unknown native event register") Right (N.registerFromLabel (K.toString key))
      (,) register <$> word value

metadataJSON :: RunnerMetadata -> Value
metadataJSON (RunnerMetadata elapsed traceTotal traces accessTotal accesses provenance events detail memories) = object
  [ "elapsed_ns" .= count elapsed, "trace_total" .= count traceTotal, "trace" .= map siteJSON traces
  , "access_total" .= count accessTotal, "accesses" .= map accessJSON accesses
  , "memory_provenance" .= map provenanceJSON provenance
  , "events" .= [object ["address" .= N.guestAddress address, "inputs" .= registersJSON inputs, "outputs" .= registersJSON outputs] | StubEvent address inputs outputs <- events]
  , "wire_version" .= (1 :: Int), "memory_addresses" .= map N.guestAddress memories
  , "detail" .= fmap (\(DiagnosticText value) -> value) detail ]
  where
    count (NativeCount n) = n
    siteJSON (InstructionSite pc (NativeInstruction instruction)) = object ["pc" .= N.guestAddress pc, "instruction" .= instruction]
    accessJSON (MemoryAccess kind address size value) = object ["kind" .= show kind, "address" .= N.guestAddress address, "size" .= size, "value" .= value]
    provenanceJSON entry = case entry of
      ProvenanceRead site address size value -> memoryJSON' "read" site address size value
      ProvenanceWrite site address size value -> memoryJSON' "write" site address size value
      ProvenanceBarrier site barrier -> tagged "barrier" ["site" .= siteJSON site, "barrier" .= show barrier]
      ProvenanceSystem site register value -> tagged "system" ["site" .= siteJSON site, "register" .= show register, "value" .= value]
    memoryJSON' kind site address size value = tagged kind ["site" .= siteJSON site, "address" .= N.guestAddress address, "size" .= size, "value" .= value]
