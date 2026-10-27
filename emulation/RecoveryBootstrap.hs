{-# LANGUAGE GeneralizedNewtypeDeriving, OverloadedStrings, ScopedTypeVariables #-}
module Main where

import BootCard (BootFile (BootPayload, RecoveryStage), mountRom, readRomFile)
import Control.Exception (IOException, displayException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (..), ToJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser, parseEither)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isHexDigit, isLower, isDigit, isSpace)
import Data.List (isPrefixOf)
import Data.Word (Word32, Word64)
import qualified FujiAF.Host as H
import qualified FujiAF.JSON as J
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>), isAbsolute, splitDirectories, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)
import System.IO.Error (isDoesNotExistError, tryIOError)
import System.Posix.Files (getSymbolicLinkStatus, isDirectory, isSymbolicLink, setFileMode)

newtype InputPath = InputPath FilePath deriving (Eq, Show, ToJSON)
newtype ToolPath = ToolPath FilePath deriving (Eq, Show, ToJSON)
newtype OutputPath = OutputPath FilePath deriving (Eq, Show, ToJSON)
newtype Digest = Digest String deriving (Eq, Show, ToJSON)
newtype Address = Address Word32 deriving (Eq, Show, ToJSON)
newtype MachineWord = MachineWord Word32 deriving (Eq, Show, ToJSON)
newtype ByteCount = ByteCount Word64 deriving (Eq, Show, ToJSON)
newtype InstructionCount = InstructionCount Word64 deriving (Eq, Ord, Show, ToJSON)
newtype EventCount = EventCount Word64 deriving (Eq, Show, ToJSON)
newtype WallSeconds = WallSeconds Int deriving (Eq, Show, ToJSON)
newtype MediaOffset = MediaOffset Word32 deriving (Eq, Show, ToJSON)

data ModelProfile = XM1PrimaryV1 deriving (Eq, Show)
newtype PoisonValue = PoisonValue Word32 deriving (Eq, Show, ToJSON)

data Variant = Stock | RC0 | Recorder deriving (Eq, Show)
data Route = Media | FatSd | FatSdStaged deriving (Eq, Show)
data RomMode = RomMode0 | RomMode1 | RomMode2 | RomMode3 | RomMode4 | RomMode5 | RomMode6 | RomMode7 deriving (Eq, Show, Enum, Bounded)
instance FromJSON RomMode where
  parseJSON value = do
    mode <- parseJSON value :: Parser Integer
    if mode >= 0 && mode <= toInteger (fromEnum (maxBound :: RomMode))
      then pure (toEnum (fromInteger mode))
      else fail "ROM mode outside eight-mode dispatcher"
instance ToJSON RomMode where
  toJSON = toJSON . fromEnum
romModeForRoute :: Route -> RomMode
romModeForRoute Media = RomMode4
romModeForRoute FatSd = RomMode1
romModeForRoute FatSdStaged = RomMode1
engineRoute :: Route -> Route
engineRoute FatSdStaged = FatSd
engineRoute route = route
data Fault = NoFault | TapFault | MarkerFault | MediaFault | RouteFault deriving (Eq, Show)
data Status = Complete | Unsupported | NativeHalt | Budget | EngineError | RouteMismatch deriving (Eq, Show)
data Phase = Reset | RomLoad | NativeReset | NativeLoad | Rollback | Close deriving (Eq, Show)
data WriterOrigin = CpuAttempt | MediaDma | SdDma | Fill | Lz deriving (Eq, Show)
data LoadStage = BeforeLoad | PreDecode | ImageDecode | AfterLoad deriving (Eq, Ord, Show)
data Verdict = ModelMatched | UnacceptedTerminal | SnapshotMismatch | UnexpectedNegativeCompletion | InvocationFailure | InvalidReport deriving (Eq, Show)
data SnapshotName = Staging | Kernel0 | Kernel1 | Id4Before | Id4Bss | Id4After | Segments deriving (Eq, Show)
data ArtifactName = DatArtifact | Decode4Artifact | Decode5Artifact | RomArtifact | CardArtifact | DecoderArtifact | EngineArtifact | SourceMediaArtifact | ModeledMediaArtifact deriving (Eq, Show)
data SegmentId = Segment0 | Segment1 | Segment4 deriving (Eq, Show)
data DecodeId = Decode4 | Decode5 deriving (Eq, Show)

variants :: [(String, Variant)]
variants = [("stock", Stock), ("rc0", RC0), ("recorder", Recorder)]
routes :: [(String, Route)]
routes = [("media", Media), ("fat-sd", FatSd), ("fat-sd-staged", FatSdStaged)]
faults :: [(String, Fault)]
faults = [("none", NoFault), ("tap", TapFault), ("marker", MarkerFault), ("media", MediaFault), ("route", RouteFault)]
statuses :: [(String, Status)]
statuses = [("complete", Complete), ("unsupported", Unsupported), ("native_halt", NativeHalt), ("budget", Budget), ("engine_error", EngineError), ("route_mismatch", RouteMismatch)]
phases :: [(String, Phase)]
phases = [("reset", Reset), ("rom_load", RomLoad), ("native_reset", NativeReset), ("native_load", NativeLoad), ("rollback", Rollback), ("close", Close)]
profiles :: [(String, ModelProfile)]
profiles = [("x-m1-primary-v1", XM1PrimaryV1)]
writerOrigins :: [(String, WriterOrigin)]
writerOrigins = [("cpu_attempt", CpuAttempt), ("media_dma", MediaDma), ("sd_dma", SdDma), ("fill", Fill), ("lz", Lz)]
loadStages :: [(String, LoadStage)]
loadStages = [("before_load", BeforeLoad), ("pre_decode", PreDecode), ("image_decode", ImageDecode), ("after_load", AfterLoad)]

label :: Eq a => [(String, a)] -> a -> String
label choices value = case [name | (name, item) <- choices, item == value] of
  [name] -> name
  _ -> error "closed enumeration has no label"

parseChoice :: [(String, a)] -> Value -> Parser a
parseChoice choices value = do
  name <- parseJSON value
  maybe (fail "unsupported enumeration value") pure (lookup name choices)

instance ToJSON Variant where toJSON = toJSON . label variants
instance ToJSON Route where toJSON = toJSON . label routes
instance ToJSON Fault where toJSON = toJSON . label faults
instance ToJSON Status where toJSON = toJSON . label statuses
instance ToJSON Phase where toJSON = toJSON . label phases
instance FromJSON Route where parseJSON = parseChoice routes
instance FromJSON Fault where parseJSON = parseChoice faults
instance FromJSON Status where parseJSON = parseChoice statuses
instance FromJSON Phase where parseJSON = parseChoice phases
instance ToJSON ModelProfile where toJSON = toJSON . label profiles
instance FromJSON ModelProfile where parseJSON = parseChoice profiles
instance ToJSON WriterOrigin where toJSON = toJSON . label writerOrigins
instance FromJSON WriterOrigin where parseJSON = parseChoice writerOrigins
instance ToJSON LoadStage where toJSON = toJSON . label loadStages
instance FromJSON LoadStage where parseJSON = parseChoice loadStages
instance ToJSON Verdict where
  toJSON value = toJSON (case value of
    ModelMatched -> "model_matched"; UnacceptedTerminal -> "unaccepted_terminal"
    SnapshotMismatch -> "snapshot_mismatch"; UnexpectedNegativeCompletion -> "unexpected_negative_completion"
    InvocationFailure -> "invocation_failure"; InvalidReport -> "invalid_report" :: String)
instance ToJSON SnapshotName where toJSON = toJSON . snapshotFile
instance FromJSON SnapshotName where
  parseJSON = parseChoice [(snapshotFile name, name) | name <- [Staging, Kernel0, Kernel1, Id4Before, Id4Bss, Id4After, Segments]]
instance ToJSON ArtifactName where
  toJSON name = toJSON (case name of
    DatArtifact -> "dat"; Decode4Artifact -> "decode_id4"; Decode5Artifact -> "decode_id5"
    RomArtifact -> "rom"; CardArtifact -> "card"; DecoderArtifact -> "decoder"; EngineArtifact -> "engine"
    SourceMediaArtifact -> "source_media"; ModeledMediaArtifact -> "modeled_media" :: String)

bounded :: Integer -> Value -> Parser Integer
bounded high value = do
  number <- parseJSON value
  unless (number >= 0 && number <= high) (fail "unsigned integer out of range")
  pure number
instance FromJSON Address where parseJSON value = Address . fromInteger <$> bounded 0xffffffff value
instance FromJSON MachineWord where parseJSON value = MachineWord . fromInteger <$> bounded 0xffffffff value
instance FromJSON ByteCount where parseJSON value = ByteCount . fromInteger <$> bounded 0xffffffffffffffff value
instance FromJSON InstructionCount where parseJSON value = InstructionCount . fromInteger <$> bounded 0xffffffffffffffff value
instance FromJSON EventCount where parseJSON value = EventCount . fromInteger <$> bounded 0xffffffffffffffff value
instance FromJSON PoisonValue where parseJSON value = PoisonValue . fromInteger <$> bounded 255 value

exact :: Object -> [String] -> Parser ()
exact fields names = unless (KM.size fields == length names && all (\name -> KM.member (K.fromString name) fields) names) (fail "exact report fields required")

data Access = Access
  { accessAddress :: Address, accessWidth :: ByteCount, accessWrite :: Bool
  , accessValue :: MachineWord, accessPc :: Address, accessOpcode :: MachineWord, accessCallerPc :: Address
  } deriving (Eq, Show)
instance FromJSON Access where
  parseJSON = withObject "access" $ \fields -> do
    exact fields ["address", "width", "write", "value", "pc", "opcode", "caller_pc"]
    Access <$> fields .: "address" <*> fields .: "width" <*> fields .: "write" <*> fields .: "value" <*> fields .: "pc" <*> fields .: "opcode" <*> fields .: "caller_pc"
instance ToJSON Access where
  toJSON access = object ["address" .= accessAddress access, "width" .= accessWidth access, "write" .= accessWrite access, "value" .= accessValue access, "pc" .= accessPc access, "opcode" .= accessOpcode access, "caller_pc" .= accessCallerPc access]

data Statistics = Statistics
  { mediaReadBytes :: ByteCount, sdReadBytes :: ByteCount, fillBytes :: ByteCount
  , decodedBytes :: ByteCount, transactions :: EventCount, peerReleaseRequests :: EventCount
  } deriving (Eq, Show)
instance FromJSON Statistics where
  parseJSON = withObject "statistics" $ \fields -> do
    exact fields ["media_read_bytes", "sd_read_bytes", "fill_bytes", "decoded_bytes", "transactions", "peer_release_requests"]
    Statistics <$> fields .: "media_read_bytes" <*> fields .: "sd_read_bytes" <*> fields .: "fill_bytes" <*> fields .: "decoded_bytes" <*> fields .: "transactions" <*> fields .: "peer_release_requests"
instance ToJSON Statistics where
  toJSON stats = object ["media_read_bytes" .= mediaReadBytes stats, "sd_read_bytes" .= sdReadBytes stats, "fill_bytes" .= fillBytes stats, "decoded_bytes" .= decodedBytes stats, "transactions" .= transactions stats, "peer_release_requests" .= peerReleaseRequests stats]

data TapTransition = TapTransition
  { tapBefore :: MachineWord, tapAfter :: MachineWord
  } deriving (Eq, Show)
instance FromJSON TapTransition where
  parseJSON = withObject "tap transition" $ \fields -> do
    exact fields ["before", "after"]
    TapTransition <$> fields .: "before" <*> fields .: "after"
instance ToJSON TapTransition where
  toJSON tap = object ["before" .= tapBefore tap, "after" .= tapAfter tap]

data BssPoison = BssPoison Address ByteCount PoisonValue deriving (Eq, Show)
instance FromJSON BssPoison where
  parseJSON = withObject "BSS poison" $ \fields -> do
    exact fields ["address", "bytes", "value"]
    BssPoison <$> fields .: "address" <*> fields .: "bytes" <*> fields .: "value"
instance ToJSON BssPoison where
  toJSON (BssPoison address size value) = object ["address" .= address, "bytes" .= size, "value" .= value]

data CloseEvidence = CloseEvidence
  { closeEntered :: Bool, closeCallbackEntered :: Bool, closeRestoreEntered :: Bool, closeReturned :: Bool
  , closeOpenBefore :: Maybe MachineWord, closeOpenAfter :: Maybe MachineWord
  , closeSpBefore :: Maybe Address, closeSpAfter :: Maybe Address, closeReturnR0 :: Maybe MachineWord
  } deriving (Eq, Show)
instance FromJSON CloseEvidence where
  parseJSON = withObject "close evidence" $ \fields -> do
    exact fields ["entered", "callback_entered", "restore_entered", "returned", "open_before", "open_after", "sp_before", "sp_after", "return_r0"]
    CloseEvidence <$> fields .: "entered" <*> fields .: "callback_entered" <*> fields .: "restore_entered" <*> fields .: "returned" <*> fields .: "open_before" <*> fields .: "open_after" <*> fields .: "sp_before" <*> fields .: "sp_after" <*> fields .: "return_r0"
instance ToJSON CloseEvidence where
  toJSON evidence = object
    [ "entered" .= closeEntered evidence, "callback_entered" .= closeCallbackEntered evidence
    , "restore_entered" .= closeRestoreEntered evidence, "returned" .= closeReturned evidence
    , "open_before" .= closeOpenBefore evidence, "open_after" .= closeOpenAfter evidence
    , "sp_before" .= closeSpBefore evidence, "sp_after" .= closeSpAfter evidence, "return_r0" .= closeReturnR0 evidence
    ]

data PrefixWrite = PrefixWrite
  { writeOrigin :: WriterOrigin, writePhase :: Phase, writeStage :: LoadStage
  , writePc :: Address, writeAddress :: Address, writeBytes :: ByteCount
  , writeOverlapAddress :: Address, writeOverlapBytes :: ByteCount, writeOverlapUnchanged :: Bool
  } deriving (Eq, Show)
instance FromJSON PrefixWrite where
  parseJSON = withObject "prefix write" $ \fields -> do
    exact fields ["origin", "phase", "stage", "pc", "address", "bytes", "overlap_address", "overlap_bytes", "overlap_unchanged"]
    event <- PrefixWrite <$> fields .: "origin" <*> fields .: "phase" <*> fields .: "stage" <*> fields .: "pc" <*> fields .: "address" <*> fields .: "bytes" <*> fields .: "overlap_address" <*> fields .: "overlap_bytes" <*> fields .: "overlap_unchanged"
    let Address address = writeAddress event
        ByteCount size = writeBytes event
        base
          | toInteger address + toInteger size <= 0x03660000 = Just (toInteger address)
          | address >= 0x40000000 && toInteger address + toInteger size <= 0x43660000 = Just (toInteger address - 0x40000000)
          | otherwise = Nothing
        Address overlapAddress = writeOverlapAddress event
        ByteCount overlapBytes = writeOverlapBytes event
    case base of
      Nothing -> fail "prefix write outside mapped RAM"
      Just start -> do
        let low = max 0x61c1fc start
            high = min 0x61c400 (start + toInteger size)
        unless (size > 0 && high > low && toInteger overlapAddress == low && toInteger overlapBytes == high - low) (fail "prefix write overlap differs from mapped interval")
    unless (writeOrigin event /= CpuAttempt || size <= 8) (fail "CPU prefix write exceeds hook value")
    unless (case writeStage event of
      BeforeLoad -> writePhase event `elem` [Reset, RomLoad, NativeReset]
      PreDecode -> writePhase event == NativeLoad
      ImageDecode -> writePhase event == NativeLoad
      AfterLoad -> writePhase event `elem` [Rollback, Close]) (fail "prefix write stage disagrees with phase")
    pure event
instance ToJSON PrefixWrite where
  toJSON event = object
    [ "origin" .= writeOrigin event, "phase" .= writePhase event, "stage" .= writeStage event
    , "pc" .= writePc event, "address" .= writeAddress event, "bytes" .= writeBytes event
    , "overlap_address" .= writeOverlapAddress event, "overlap_bytes" .= writeOverlapBytes event, "overlap_unchanged" .= writeOverlapUnchanged event
    ]

data PrefixWrites = PrefixWrites
  { prefixAddress :: Address, prefixBytes :: ByteCount, prefixComplete :: Bool
  , prefixLoadEntered :: Bool, prefixDecodeEntered :: Bool, prefixLoadReturned :: Bool
  , prefixCpuAttempts :: EventCount, prefixDeviceCommits :: EventCount, prefixEvents :: [PrefixWrite]
  } deriving (Eq, Show)
instance FromJSON PrefixWrites where
  parseJSON = withObject "prefix write evidence" $ \fields -> do
    exact fields ["address", "bytes", "complete", "load_entered", "decode_entered", "load_returned", "cpu_attempts", "device_commits", "events"]
    audit <- PrefixWrites <$> fields .: "address" <*> fields .: "bytes" <*> fields .: "complete" <*> fields .: "load_entered" <*> fields .: "decode_entered" <*> fields .: "load_returned" <*> fields .: "cpu_attempts" <*> fields .: "device_commits" <*> fields .: "events"
    let events = prefixEvents audit
        cpu = length (filter ((== CpuAttempt) . writeOrigin) events)
        devices = length events - cpu
        EventCount attempts = prefixCpuAttempts audit
    unless (prefixAddress audit == Address 0x61c1fc && prefixBytes audit == ByteCount 516 && length events <= 512) (fail "prefix audit extent or event bound differs")
    unless (prefixDeviceCommits audit == EventCount (fromIntegral devices) &&
      (if prefixComplete audit then attempts == fromIntegral cpu else attempts >= fromIntegral cpu)) (fail "prefix event counters disagree")
    unless (and (zipWith (<=) (map writeStage events) (drop 1 (map writeStage events)))) (fail "prefix event stages move backwards")
    unless ((not (prefixDecodeEntered audit) || prefixLoadEntered audit) &&
      (not (prefixLoadReturned audit) || prefixDecodeEntered audit)) (fail "prefix load markers disagree")
    unless (all (\event -> case writeStage event of
      BeforeLoad -> True
      PreDecode -> prefixLoadEntered audit
      ImageDecode -> prefixDecodeEntered audit
      AfterLoad -> prefixLoadReturned audit) events) (fail "prefix event lacks its load marker")
    pure audit
instance ToJSON PrefixWrites where
  toJSON audit = object
    [ "address" .= prefixAddress audit, "bytes" .= prefixBytes audit, "complete" .= prefixComplete audit
    , "load_entered" .= prefixLoadEntered audit, "decode_entered" .= prefixDecodeEntered audit, "load_returned" .= prefixLoadReturned audit
    , "cpu_attempts" .= prefixCpuAttempts audit, "device_commits" .= prefixDeviceCommits audit, "events" .= prefixEvents audit
    ]

data Report = Report
  { reportStatus :: Status, reportPhase :: Phase, reportRequestedRoute :: Route, reportSelectedRomMode :: Maybe RomMode, reportFault :: Fault
  , reportPc :: Address, reportInstructions :: InstructionCount, reportAccess :: Maybe Access
  , reportDetail :: String, reportStatistics :: Statistics, reportTap :: TapTransition, reportAssumptions :: [String]
  , reportProfile :: ModelProfile, reportSnapshots :: [Extent], reportBssPoison :: Maybe BssPoison, reportClose :: CloseEvidence
  , reportPrefixWrites :: PrefixWrites
  } deriving (Eq, Show)
instance FromJSON Report where
  parseJSON = withObject "engine report" $ \fields -> do
    exact fields ["schema_version", "status", "phase", "requested_route", "selected_rom_mode", "fault", "pc", "instructions", "access", "detail", "statistics", "tap", "assumptions", "profile", "snapshots", "bss_poison", "close", "prefix_writes"]
    version <- fields .: "schema_version" :: Parser Integer
    unless (version == 2) (fail "unsupported engine report schema")
    report <- Report <$> fields .: "status" <*> fields .: "phase" <*> fields .: "requested_route" <*> fields .: "selected_rom_mode" <*> fields .: "fault" <*> fields .: "pc" <*> fields .: "instructions" <*> fields .: "access" <*> fields .: "detail" <*> fields .: "statistics" <*> fields .: "tap" <*> fields .: "assumptions" <*> fields .: "profile" <*> fields .: "snapshots" <*> fields .: "bss_poison" <*> fields .: "close" <*> fields .: "prefix_writes"
    let names = [name | Extent name _ _ <- reportSnapshots report]
    unless (length names == length [name | name <- [Staging, Kernel0, Kernel1, Id4Before, Id4Bss, Id4After, Segments], name `elem` names]) (fail "duplicate snapshot filename")
    pure report
instance ToJSON Report where
  toJSON report = object
    [ "schema_version" .= (2 :: Int), "status" .= reportStatus report, "phase" .= reportPhase report
    , "requested_route" .= reportRequestedRoute report, "selected_rom_mode" .= reportSelectedRomMode report, "fault" .= reportFault report, "pc" .= reportPc report
    , "instructions" .= reportInstructions report, "access" .= reportAccess report, "detail" .= reportDetail report
    , "statistics" .= reportStatistics report, "tap" .= reportTap report, "assumptions" .= reportAssumptions report
    , "profile" .= reportProfile report, "snapshots" .= reportSnapshots report, "bss_poison" .= reportBssPoison report, "close" .= reportClose report
    , "prefix_writes" .= reportPrefixWrites report
    ]

data Artifact = Artifact ArtifactName InputPath ByteCount Digest
instance ToJSON Artifact where
  toJSON (Artifact name path size digest) = object
    [ "name" .= name, "path" .= path, "bytes" .= size, "sha256" .= digest
    , "derived_complemented_tail" .= (name == SourceMediaArtifact)
    ]
data Mutation = Mutation MediaOffset ByteCount MachineWord MachineWord
instance ToJSON Mutation where
  toJSON (Mutation offset width before after) = object ["media_offset" .= offset, "width" .= width, "before" .= before, "after" .= after]
data Extent = Extent SnapshotName Address ByteCount deriving (Eq, Show)
instance FromJSON Extent where
  parseJSON = withObject "snapshot extent" $ \fields -> do
    exact fields ["file", "address", "bytes"]
    Extent <$> fields .: "file" <*> fields .: "address" <*> fields .: "bytes"
instance ToJSON Extent where
  toJSON (Extent name address size) = object ["file" .= name, "address" .= address, "bytes" .= size]
data Provenance = Provenance Variant Route Fault [Artifact] [Mutation] [Extent] InstructionCount WallSeconds
instance ToJSON Provenance where
  toJSON (Provenance variant route fault artifacts mutations extents instructions seconds) = object
    [ "schema_version" .= (1 :: Int), "target" .= ("X-M1 1.50" :: String), "variant" .= variant, "route" .= route, "fault" .= fault
    , "artifacts" .= artifacts, "modeled_mutations" .= mutations, "snapshot_extents" .= extents
    , "native_flash_DAT_offset" .= ByteCount (fromIntegral U.stagingLength)
    , "instruction_limit" .= instructions, "wall_seconds" .= seconds, "model_limits" .= modelLimits
    , "pin_sources" .= (["docs/evidence.jsonl:E-FILMSIM-PREFIX-150", "private-data/analysis/x-m1-1.50/af-recorder/rev-2/emit-results.md", "private-data/analysis/x-m1-1.50/update/natw-nativewriter-report.md", "reports/full-system-emulation.md:11.4"] :: [String])
    ]

data SnapshotComparison = SnapshotComparison SnapshotName Address (Maybe Address) ByteCount (Maybe ByteCount) Digest (Maybe Digest) Bool String
instance ToJSON SnapshotComparison where
  toJSON (SnapshotComparison name expectedAddress actualAddress wanted got expected actual matches detail) = object
    [ "file" .= name, "expected_address" .= expectedAddress, "actual_address" .= actualAddress, "expected_bytes" .= wanted, "actual_bytes" .= got
    , "expected_sha256" .= expected, "actual_sha256" .= actual, "matches" .= matches, "detail" .= detail
    ]
data ProcessEvidence = ProcessEvidence Bool Int
instance ToJSON ProcessEvidence where
  toJSON (ProcessEvidence timed code) = object ["host_bound_exceeded" .= timed, "exit_code" .= code, "stdout_file" .= ("engine.stdout" :: String), "stderr_file" .= ("engine.stderr" :: String)]
data Result = Result Verdict String Provenance ProcessEvidence (Maybe Report) [SnapshotComparison]
instance ToJSON Result where
  toJSON (Result verdict detail provenance process report comparisons) = object
    [ "schema_version" .= (1 :: Int), "verdict" .= verdict, "detail" .= detail, "provenance" .= provenance
    , "process" .= process, "engine_report" .= report, "comparisons" .= comparisons
    , "model_accepted" .= (verdict == ModelMatched), "hardware_recovery_proven" .= False, "gate_advance" .= False
    ]

modelLimits :: [String]
modelLimits =
  [ "Logical complemented DAT media, not a physical post-flash clone; no ECC, spare, remap history, per-unit settings or failed-install residue."
  , "Captured 16 KiB X-M1 ROM only; Cortex-R5F ISA proxy, CP14 discriminator and reset context are declared model assumptions."
  , "Sibling-derived controller completion, clean media, initial RAM and timing laws are model assumptions, not X-M1 silicon observations."
  , "CPU1 execution is held; release requests and vector copies do not establish peer acknowledgement."
  , "Execution stops before native 0x1ff8; no kernel, application, recorder task, display, USB, sensor or lens execution."
  , "No physical camera, card-reader or USB I/O; no persistent-media erase or program is permitted."
  , "A matched single case is not the paired acceptance suite, physical SD entry, hardware recovery or authorization for another live attempt."
  ]

data Command = Command Variant Route Fault InputPath InputPath InputPath InputPath InputPath ToolPath ToolPath OutputPath

datPin :: Variant -> Digest
datPin variant = Digest (case variant of
  Stock -> "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
  RC0 -> "acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8"
  Recorder -> "72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8")
id4Pin :: Variant -> Digest
id4Pin variant = Digest (case variant of
  Stock -> "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  RC0 -> "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551"
  Recorder -> "9447d0873d1926b333987888097fc35d6234ae1d5a7607f32cf026b6bf11d23a")
id5Pin, romPin :: Digest
id5Pin = Digest "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
romPin = Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
cardPin :: Route -> Digest
cardPin FatSdStaged = Digest "c62c4085ad4cbe402300e8171a4d52791b5fdbb6f0b8df7eba0418520daee19e"
cardPin _ = Digest "38271a89abae8e750ad5cc84b842989e9f3cfeee68b2e76ac8aeda766ddd3f6f"

hashBytes :: B.ByteString -> IO Digest
hashBytes bytes = do
  digest <- H.sha256 bytes
  H.ensure (length digest == 64 && all (\c -> isHexDigit c && (isDigit c || isLower c)) digest) "SHA256 helper returned invalid digest"
  pure (Digest digest)

workspaceRoot :: IO FilePath
workspaceRoot = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
resolve :: FilePath -> IO FilePath
resolve path = do
  H.ensure (not (null path)) "path must be nonempty"
  root <- workspaceRoot
  pure (if isAbsolute path then path else root </> path)

parseCommand :: [String] -> IO Command
parseCommand arguments = case arguments of
  [variant, route, fault, dat, image4, image5, rom, card, decoder, engine, output] -> do
    v <- choice "variant" variants variant
    r <- choice "route" routes route
    f <- choice "fault" faults fault
    Command v r f <$> input dat <*> input image4 <*> input image5 <*> input rom <*> input card <*> tool decoder <*> tool engine <*> (OutputPath <$> resolve output)
  _ -> H.failH "usage: recovery-bootstrap VARIANT ROUTE FAULT DAT DECODE_ID4 DECODE_ID5 ROM CARD FFLZD ENGINE NEW_PRIVATE_OUTPUT_DIRECTORY"
  where
    choice name choices value = maybe (H.failH ("unsupported " ++ name ++ ": " ++ value)) pure (lookup value choices)
    input path = InputPath <$> resolve path
    tool path = ToolPath <$> resolve path

readPinned :: ArtifactName -> InputPath -> Int -> Digest -> IO (B.ByteString, Artifact)
readPinned name path@(InputPath file) cap pin = do
  bytes <- H.readRegular cap file
  artifact@(Artifact _ _ _ digest) <- describe name path bytes
  H.ensure (digest == pin) ("unsupported artifact digest: " ++ show name)
  pure (bytes, artifact)
describe :: ArtifactName -> InputPath -> B.ByteString -> IO Artifact
describe name path bytes = Artifact name path (ByteCount (fromIntegral (B.length bytes))) <$> hashBytes bytes

eitherChecked :: Either String a -> IO a
eitherChecked = either H.failH pure

newPrivateDirectory :: OutputPath -> IO FilePath
newPrivateDirectory (OutputPath given) = do
  H.ensure (takeFileName given `notElem` ["", ".", ".."]) "output must name a fresh directory"
  root <- workspaceRoot
  private <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory given)
  H.ensure (splitDirectories private `isPrefixOf` splitDirectories parent) "output must be beneath private-data"
  forM_ (scanl (</>) "/" (filter (/= "/") (splitDirectories (takeDirectory given)))) $ \ancestor -> do
    status <- getSymbolicLinkStatus ancestor
    H.ensure (isDirectory status && not (isSymbolicLink status)) "output ancestor must be a nonsymlink directory"
  let output = parent </> takeFileName given
  status <- tryIOError (getSymbolicLinkStatus output)
  case status of
    Left err | isDoesNotExistError err -> pure ()
             | otherwise -> ioError err
    Right _ -> H.failH "output directory already exists"
  createPrivate output
  pure output
createPrivate :: FilePath -> IO ()
createPrivate path = createDirectory path >> setFileMode path 0o700

spanBytes :: B.ByteString -> MediaOffset -> ByteCount -> IO B.ByteString
spanBytes bytes (MediaOffset offset) (ByteCount size) = do
  H.ensure (toInteger offset + toInteger size <= toInteger (B.length bytes)) "source extent exceeds modeled medium"
  pure (B.take (fromIntegral size) (B.drop (fromIntegral offset) bytes))
findEntry :: U.Parts -> SegmentId -> IO U.SegmentEntry
findEntry parts ident = case filter ((== number) . U.segId) (U.partSegments parts) of
  [entry] -> pure entry
  _ -> H.failH "required unique segment missing"
  where number = case ident of Segment0 -> 0; Segment1 -> 1; Segment4 -> 4

snapshotFile :: SnapshotName -> String
snapshotFile name = case name of
  Staging -> "staging.bin"; Kernel0 -> "kernel0.bin"; Kernel1 -> "kernel1.bin"
  Id4Before -> "id4-before.bin"; Id4Bss -> "id4-bss.bin"; Id4After -> "id4-after.bin"
  Segments -> "segments.bin"

decodeRecord :: FilePath -> ToolPath -> DecodeId -> U.CompressedRecord -> B.ByteString -> IO ()
decodeRecord output (ToolPath decoder) ident record expected = do
  let name = case ident of Decode4 -> "id4"; Decode5 -> "id5"
      source = output </> (name ++ ".record")
      destination = output </> (name ++ ".decoded")
      expectedSize = U.recDecodedSize (U.recHeader record)
  H.ensure (toInteger (B.length expected) == toInteger expectedSize) "retained decode length differs from record"
  B.writeFile source (B.map complement (U.serializeRecord record))
  (timed, code, stdoutBytes, stderrBytes) <- H.runBounded 20000000 8192 decoder [source, "0", destination, show expectedSize] B.empty
  B.writeFile (destination ++ ".stdout") stdoutBytes
  B.writeFile (destination ++ ".stderr") stderrBytes
  H.ensure (not timed && code == ExitSuccess) "project decoder failed or exceeded its bound"
  decoded <- H.readRegular (16 * 1024 * 1024) destination
  H.ensure (decoded == expected) "retained decoded image does not match actual project DAT decode"

prepareMarker :: Fault -> U.Parsed -> B.ByteString -> IO (B.ByteString, [Mutation])
prepareMarker fault parsed media = case fault of
  MarkerFault -> do
    let header = U.parsedHeader parsed
        offsetInteger = toInteger (U.hMainOffset header) + toInteger (U.hMainLength header) - 2
    H.ensure (offsetInteger >= 0 && offsetInteger + 2 <= toInteger (B.length media)) "completion marker outside modeled media"
    let offset = fromInteger offsetInteger
    H.ensure (U.word16At media offset == 0xaaaa) "documented completion marker differs from selected DAT"
    pure (B.take offset media <> U.putWord16 0 <> B.drop (offset + 2) media, [Mutation (MediaOffset (fromIntegral offset)) (ByteCount 2) (MachineWord 0xaaaa) (MachineWord 0)])
  _ -> pure (media, [])

data ExpectedSnapshot = ExpectedSnapshot SnapshotName Address B.ByteString
snapshotExtent :: ExpectedSnapshot -> Extent
snapshotExtent (ExpectedSnapshot name address bytes) = Extent name address (ByteCount (fromIntegral (B.length bytes)))

expectedSnapshots :: Variant -> Route -> U.Parts -> B.ByteString -> B.ByteString -> B.ByteString -> IO ([ExpectedSnapshot], TapTransition)
expectedSnapshots variant route parts media card image4 = do
  staging <- case route of
    Media -> spanBytes media (MediaOffset 0x40000) (ByteCount 0x40000)
    FatSd -> do
      volume <- eitherChecked (mountRom card)
      payload <- eitherChecked (readRomFile card volume BootPayload)
      H.ensure (B.length payload == 0x40000) "FAT FFBOOT must hold exact full 256 KiB staging"
      pure payload
    FatSdStaged -> do
      volume <- eitherChecked (mountRom card)
      payload <- eitherChecked (readRomFile card volume RecoveryStage)
      H.ensure (B.length payload == 0x40000) "FAT FFRECOV must hold exact full 256 KiB staging"
      pure payload
  entry0 <- findEntry parts Segment0
  entry1 <- findEntry parts Segment1
  entry4 <- findEntry parts Segment4
  kernel0 <- spanBytes media (MediaOffset (U.segFlash entry0)) (ByteCount (fromIntegral (U.segSize entry0)))
  kernel1 <- spanBytes media (MediaOffset (U.segFlash entry1)) (ByteCount (fromIntegral (U.segSize entry1)))
  segments <- spanBytes media (MediaOffset (fromIntegral (U.segmentTableOffset (U.partTableLayout parts)))) (ByteCount (fromIntegral (U.segmentCount * U.segmentEntryBytes + 4)))
  let offsetInteger = toInteger (0x71feb4 :: Word32) - toInteger (U.segLoad entry4)
  H.ensure (offsetInteger >= 0 && offsetInteger + 4 <= toInteger (B.length image4)) "tap outside decoded id4 extent"
  let offset = fromInteger offsetInteger
      tap = U.word32At image4 offset
      wantedTap = if variant == Recorder then 0xeb34a9c9 else 0xebffffdc
  H.ensure (tap == wantedTap) "retained decode tap does not match selected variant"
  let after = if variant == Recorder && route /= Media
        then B.take offset image4 <> U.putWord32 0xebffffdc <> B.drop (offset + 4) image4
        else image4
  pure
    ( [ ExpectedSnapshot Staging (Address 0) staging
    , ExpectedSnapshot Kernel0 (Address (U.segLoad entry0)) kernel0
    , ExpectedSnapshot Kernel1 (Address (U.segLoad entry1)) kernel1
    , ExpectedSnapshot Id4Before (Address (U.segLoad entry4)) image4
    , ExpectedSnapshot Id4Bss (Address (U.segBss entry4)) (B.replicate (fromIntegral (U.segBssLen entry4)) 0)
    , ExpectedSnapshot Id4After (Address (U.segLoad entry4)) after
    , ExpectedSnapshot Segments (Address (0x61c000 + fromIntegral (U.segmentTableOffset (U.partTableLayout parts) - 0x140000))) segments
      ]
    , TapTransition (MachineWord tap) (MachineWord (U.word32At after offset))
    )

compareSnapshot :: FilePath -> [Extent] -> ExpectedSnapshot -> IO SnapshotComparison
compareSnapshot output extents wanted@(ExpectedSnapshot name address expected) = do
  digest <- hashBytes expected
  let metadata = [extent | extent@(Extent actualName _ _) <- extents, actualName == name]
      actualAddress = case metadata of [Extent _ observed _] -> Just observed; _ -> Nothing
      bound = metadata == [snapshotExtent wanted]
  captured <- try (H.readRegular (16 * 1024 * 1024) (output </> snapshotFile name))
  case captured of
    Left (err :: IOException) -> pure (SnapshotComparison name address actualAddress size Nothing digest Nothing False (displayException err))
    Right actual -> do
      actualDigest <- hashBytes actual
      let equal = bound && actual == expected
          detail | not bound = "snapshot metadata is missing or differs from selected source extent"
                 | actual /= expected = "snapshot differs from selected source"
                 | otherwise = "source-bound extent and full byte equality"
      pure (SnapshotComparison name address actualAddress size (Just (ByteCount (fromIntegral (B.length actual)))) digest (Just actualDigest) equal detail)
  where size = ByteCount (fromIntegral (B.length expected))
comparisonMatches :: SnapshotComparison -> Bool
comparisonMatches (SnapshotComparison _ _ _ _ _ _ _ matches _) = matches

expectedCloseReturn :: [ExpectedSnapshot] -> IO MachineWord
expectedCloseReturn expected = case [bytes | ExpectedSnapshot Staging _ bytes <- expected] of
  [staging] -> do
    H.ensure (B.length staging >= 0x2058) "native close wrapper outside staging source"
    let wordsAtWrapper = [U.word32At staging offset | offset <- [0x2044, 0x2048, 0x204c, 0x2050, 0x2054]]
    H.ensure (wordsAtWrapper == [0xe92d4800, 0xe28db004, 0xeb00041a, 0xe3a00000, 0xe8bd8800]) "native close wrapper differs from original source"
    pure (MachineWord (U.word32At staging 0x2050 `mod` 256))
  _ -> H.failH "required unique staging source missing"

prefixEvidenceMatches :: Address -> ByteCount -> PrefixWrites -> Bool
prefixEvidenceMatches (Address loadAddress) (ByteCount decodedSize) audit =
  prefixComplete audit && prefixLoadEntered audit && prefixDecodeEntered audit && prefixLoadReturned audit
    && prefixCpuAttempts audit == EventCount 0 && prefixDeviceCommits audit /= EventCount 0
    && length tableWrites + length decodedWrites == length events
    && covers 0x61c1fc 0x61c400 tableWrites
    && covers (max 0x61c1fc (toInteger loadAddress)) (min 0x61c400 (toInteger loadAddress + toInteger decodedSize)) decodedWrites
  where
    events = prefixEvents audit
    tableWrites = filter (\event -> writeStage event == PreDecode && writeOrigin event `elem` [MediaDma, SdDma]) events
    decodedWrites = filter (\event -> writeStage event == ImageDecode && writeOrigin event == Lz) events
    covers low high remaining
      | low >= high = null remaining
      | otherwise = case remaining of
          [] -> False
          event : rest ->
            let Address start = writeOverlapAddress event
                ByteCount size = writeOverlapBytes event
                end = toInteger start + toInteger size
            in toInteger start == low && end <= high && covers end high rest

completeEvidenceMatches :: ByteCount -> BssPoison -> MachineWord -> Report -> Bool
completeEvidenceMatches decodedSize poison@(BssPoison _ (ByteCount bssCount) _) returnR0 report =
  reportProfile report == XM1PrimaryV1
    && reportSelectedRomMode report == Just (romModeForRoute (reportRequestedRoute report))
    && not (null (reportAssumptions report)) && all (any (not . isSpace)) (reportAssumptions report)
    && reportInstructions report > InstructionCount 0
    && transactions stats /= EventCount 0 && mediaReadBytes stats /= ByteCount 0
    && peerReleaseRequests stats /= EventCount 0
    && decodedBytes stats == decodedSize
    && fillCount >= bssCount
    && reportBssPoison report == Just poison
    && closeEntered close && closeCallbackEntered close && closeRestoreEntered close && closeReturned close
    && closeOpenBefore close == Just (MachineWord 1) && closeOpenAfter close == Just (MachineWord 0)
    && closeSpBefore close /= Nothing && closeSpBefore close == closeSpAfter close
    && closeReturnR0 close == Just returnR0
  where
    stats = reportStatistics report
    close = reportClose report
    ByteCount fillCount = fillBytes stats

writeCanonical :: ToJSON a => FilePath -> a -> IO ()
writeCanonical path value = B.writeFile path (J.encodeCanonical (toJSON value) <> "\n")

instructionLimit :: InstructionCount
instructionLimit = InstructionCount 120000000
wallLimit :: WallSeconds
wallLimit = WallSeconds 120

invoke :: ToolPath -> FilePath -> FilePath -> Route -> Fault -> IO (ProcessEvidence, Maybe Report, String)
invoke (ToolPath engine) input output route fault = do
  let InstructionCount instructions = instructionLimit
      WallSeconds seconds = wallLimit
  result <- try (H.runBounded ((seconds + 5) * 1000000) (64 * 1024) engine [input, label routes (engineRoute route), label faults fault, output, show instructions, show seconds] B.empty)
  case result of
    Left (err :: IOException) -> do
      B.writeFile (takeDirectory output </> "engine.stdout") B.empty
      B.writeFile (takeDirectory output </> "engine.stderr") B.empty
      pure (ProcessEvidence False 1, Nothing, displayException err)
    Right (timed, code, stdoutBytes, stderrBytes) -> do
      B.writeFile (takeDirectory output </> "engine.stdout") stdoutBytes
      B.writeFile (takeDirectory output </> "engine.stderr") stderrBytes
      let number = case code of ExitSuccess -> 0; ExitFailure value -> value
      report <- try $ do
        bytes <- H.readRegular (1024 * 1024) (output </> "report.json")
        value <- eitherChecked (J.decodeStrictIntegers bytes)
        eitherChecked (parseEither parseJSON value)
      case report of
        Left (err :: IOException) -> pure (ProcessEvidence timed number, Nothing, displayException err)
        Right parsed -> pure (ProcessEvidence timed number, Just parsed, "")

finish :: FilePath -> Provenance -> ProcessEvidence -> Maybe Report -> Verdict -> String -> [SnapshotComparison] -> IO ExitCode
finish output provenance process report verdict detail comparisons = do
  let result = Result verdict detail provenance process report comparisons
  writeCanonical (output </> "result.json") result
  H.jsonPrint (toJSON result)
  pure (if verdict == ModelMatched then ExitSuccess else ExitFailure 2)

run :: Command -> IO ExitCode
run (Command variant route fault datPath image4Path image5Path romPath cardPath decoder@(ToolPath decoderFile) engine@(ToolPath engineFile) outputPath) = do
  (dat, datArtifact) <- readPinned DatArtifact datPath (64 * 1024 * 1024) (datPin variant)
  parsed <- eitherChecked (U.parseContainer dat)
  let checks = U.checks parsed ++ U.carriedChecks parsed ++ if variant == Stock then U.stockChecks parsed else []
      parts = U.parsedParts parsed
  forM_ checks $ \check -> H.ensure (U.checkPassed check) ("container contract: " ++ U.checkField check)
  (image4, artifact4) <- readPinned Decode4Artifact image4Path (16 * 1024 * 1024) (id4Pin variant)
  (image5, artifact5) <- readPinned Decode5Artifact image5Path (16 * 1024 * 1024) id5Pin
  (rom, romArtifact) <- readPinned RomArtifact romPath 16384 romPin
  H.ensure (B.length rom == 16384) "ROM must be the exact captured 16 KiB block"
  (card, cardArtifact) <- readPinned CardArtifact cardPath (32 * 1024 * 1024) (cardPin route)
  decoderBytes <- H.readRegular (64 * 1024 * 1024) decoderFile
  engineBytes <- H.readRegular (64 * 1024 * 1024) engineFile
  decoderArtifact <- describe DecoderArtifact (InputPath decoderFile) decoderBytes
  engineArtifact <- describe EngineArtifact (InputPath engineFile) engineBytes
  let media = B.map complement (B.drop U.stagingLength dat)
  (modeled, mutations) <- prepareMarker fault parsed media
  (expected, expectedTap) <- expectedSnapshots variant route parts media card image4
  closeReturn <- expectedCloseReturn expected
  entry4 <- findEntry parts Segment4
  let poison = BssPoison (Address (U.segBss entry4)) (ByteCount (fromIntegral (U.segBssLen entry4))) (PoisonValue 165)
      decodedSize = ByteCount (fromIntegral (B.length image4))
  output <- newPrivateDirectory outputPath
  let input = output </> "input"
      decoding = output </> "decoder"
      engineOutput = output </> "engine"
  forM_ [input, decoding, engineOutput] createPrivate
  decodeRecord decoding decoder Decode4 (U.partRecord4 parts) image4
  decodeRecord decoding decoder Decode5 (U.partRecord5 parts) image5
  B.writeFile (input </> "rom.bin") rom
  B.writeFile (input </> "media.bin") modeled
  B.writeFile (input </> "card.img") card
  sourceArtifact <- describe SourceMediaArtifact datPath media
  modelArtifact <- describe ModeledMediaArtifact (InputPath (input </> "media.bin")) modeled
  let provenance = Provenance variant route fault [datArtifact, artifact4, artifact5, romArtifact, cardArtifact, decoderArtifact, engineArtifact, sourceArtifact, modelArtifact] mutations (map snapshotExtent expected) instructionLimit wallLimit
  writeCanonical (output </> "provenance.json") provenance
  (process@(ProcessEvidence timed code), report, parseDetail) <- invoke engine input engineOutput route fault
  case report of
    Nothing -> finish output provenance process Nothing (if timed || code == 1 then InvocationFailure else InvalidReport) parseDetail []
    Just actual
      | timed -> finish output provenance process report InvocationFailure "Host process bound exceeded; retained engine report is not accepted." []
      | reportRequestedRoute actual /= engineRoute route || reportFault actual /= fault -> finish output provenance process report InvalidReport "Engine report request/fault differs from requested profile." []
      | reportInstructions actual > instructionLimit -> finish output provenance process report InvalidReport "Engine report exceeds instruction budget." []
      | code /= (if reportStatus actual == Complete then 0 else 2) -> finish output provenance process report InvocationFailure "Engine exit code disagrees with terminal report." []
      | reportStatus actual /= Complete -> finish output provenance process report UnacceptedTerminal (reportDetail actual) []
      | fault /= NoFault -> finish output provenance process report UnexpectedNegativeCompletion "Injected negative profile completed; no recovery acceptance is granted." []
      | reportPc actual /= Address 0x1ff8 || reportPhase actual /= Close || reportAccess actual /= Nothing -> finish output provenance process report InvalidReport "Complete terminal does not match the declared pre-rendezvous stop." []
      | otherwise -> do
          comparisons <- forM expected (compareSnapshot engineOutput (reportSnapshots actual))
          let (verdict, detail)
                | not (all comparisonMatches comparisons) = (SnapshotMismatch, "One or more required snapshots are missing, unreadable or differ from selected sources.")
                | reportTap actual /= expectedTap = (InvalidReport, "Complete report tap transition differs from selected before/after decoded source words.")
                | not (completeEvidenceMatches decodedSize poison closeReturn actual) = (InvalidReport, "Complete report lacks source-bound BSS poison, close frame/state/return, executed work or explicit model assumptions.")
                | not (prefixEvidenceMatches (Address (U.segLoad entry4)) decodedSize (reportPrefixWrites actual)) = (InvalidReport, "Prefix write evidence differs from the bounded table-transfer and selected image-decode intervals.")
                | otherwise = (ModelMatched, "Bounded primary model snapshots and tap transition match selected sources; no hardware recovery or gate advance.")
          finish output provenance process report verdict detail comparisons

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run)
  case result of
    Left (err :: IOException) -> hPutStrLn stderr (displayException err) >> exitWith (ExitFailure 1)
    Right code -> exitWith code
