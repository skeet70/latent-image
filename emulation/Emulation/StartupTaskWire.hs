{-# LANGUAGE OverloadedStrings #-}
module Emulation.StartupTaskWire where

import Control.Monad (unless)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word16, Word32)
import Numeric (readHex)
import qualified Replacement.KernelInstall.Wire as W

newtype TaskId = TaskId Word16 deriving (Eq, Ord, Show)
newtype HitIndex = HitIndex Int deriving (Eq, Ord, Show)
newtype Cursor = Cursor Int deriving (Eq, Ord, Show)
data KernelCore = PrimaryKernel | SecondaryKernel deriving (Eq, Ord, Show)
data SourceKind = LoaderA | LoaderB | Record2 | Image4 deriving (Eq, Ord, Show)
data Boundary = SubmissionEntry | AuxiliaryCall | AuxiliaryReturn | CreationCall | CreationReturn | ActivationCall | ActivationReturn
  | WrapperReturn | AutoReadyEntry | AutoReadyReturn | Prepare | Prepared | Scheduler | Selected | StackTransfer
  | Trampoline | EntryTransfer | TaskEntry | EnteredInstruction
  | CriticalEnterCall | CriticalEnterReturn | CriticalExitCall | CriticalExitReturn | ReservationCall | ReservationReturn
  | FillCall | FillReturn | CacheCall | CacheReturn | LogCall | LogReturn | ErrorCallbackCall | ErrorCallbackReturn
  | IdentityCall | IdentityReturn | SaveMaskCall | SaveMaskReturn | RestoreMaskCall | RestoreMaskReturn
  | TryLockCall | TryLockReturn | UnlockCall | UnlockReturn | ReleaseEntry | ReleaseReturn | DeletionEntry | NativeWrite deriving (Eq, Ord, Show)
data ObjectKind = CreationRecords | TaskNodes | IdMap | AuxiliaryRecords | AuxiliaryNodes | Domains | Control
  | QueueHeads | Bitmap | SelectedObject | Current | Descriptor | Wrapper | Storage | Stack | EntryCode
  | SharedDomains | InterfaceRoots | InterfaceTable | Parameters | ErrorState | Callback | CursorObject | LockObject
  | ClassTable | ControlPatch | LogCallback | LogMode | LogEnable | CriticalCounter | CriticalLock deriving (Eq, Ord, Show)
data Outcome = ObservedMenu | ObservedProbe | Refused deriving (Eq, Show)
data Scope = StockStartupScope | OwnedStartupScope | StockProbeScope | OwnedProbeScope
  | JoinedStartupScope | JoinedProbeScope | StockJoinedStartupScope | StockJoinedProbeScope deriving (Eq, Show)
data ReceiptProtocol = LegacyStockProtocol | TaskSubmissionProtocol | JoinedTaskProtocol deriving (Eq, Show)
data JoinedEvidence = JoinedEvidence
  { joinedContractDigest :: W.Digest, joinedWrapperDigest :: W.Digest, joinedWrapperSymbolsDigest :: W.Digest
  , joinedCriticalDigest :: W.Digest, joinedCriticalSymbolsDigest :: W.Digest
  , joinedStorageDigest :: W.Digest, joinedStorageSymbolsDigest :: W.Digest
  } deriving (Eq, Show)
data ProgramEvidence = LegacyStockProgram | StockProgram | OwnedProgram W.Digest W.Digest W.Digest | JoinedProgram JoinedEvidence deriving (Eq, Show)
data WatchResult = WatchOk Bool (Maybe String) | WatchError String deriving (Eq, Show)
data MenuResult = MenuOk (Maybe [Word32]) | MenuError String deriving (Eq, Show)
data SourceIdentity = SourceIdentity SourceKind W.Address Int W.Digest deriving (Eq, Show)
data Identity = Identity W.Digest W.Digest W.Digest W.Digest [SourceIdentity] deriving (Eq, Show)
data Context = Context {contextCore :: KernelCore, contextState :: W.Context} deriving (Eq, Show)
data View = View {viewObject :: ObjectKind, viewPhysical :: W.Address, viewBytes :: W.Bytes, viewAliases :: [[W.Digest]]} deriving (Eq, Show)
data Task = Task
  {taskId :: TaskId, taskCreation :: W.Address, taskCreationPointer :: W.Address, taskCreationId :: W.WordValue, taskNode :: W.Address
  ,taskFlags :: W.WordValue, taskEntry :: W.Address, taskArgument :: W.WordValue, taskPriority :: W.WordValue
  ,taskStackBytes :: W.WordValue, taskStackBase :: W.Address, taskCreated :: W.WordValue
  ,taskState :: Word8, taskEffectivePriority :: Word8, taskPending :: Word8, taskSavedSp :: W.Address, taskResumePc :: W.Address
  } deriving (Eq, Show)
data Kernel = Kernel
  {kernelCore :: KernelCore, kernelViews :: [View], kernelTasks :: [Task], kernelDomain :: W.Address
  ,kernelSelected :: W.Address, kernelCurrent :: W.Address} deriving (Eq, Show)
data Sample = Sample
  {sampleHit :: HitIndex, sampleBoundary :: Boundary, sampleContext :: Context, sampleCodeWord :: W.WordValue
  ,sampleCursor :: Cursor, sampleKernels :: [Kernel], sampleLocal :: [View], sampleCallTaken :: Maybe Bool} deriving (Eq, Show)
data Submission = Submission
  {submissionId :: TaskId, submissionCore :: KernelCore, submissionDescriptor :: W.Address, submissionSp :: W.Address
  ,submissionReturnPc :: W.Address, submissionEntryHit :: HitIndex, submissionReturnHit :: Maybe HitIndex, submissionStatus :: Maybe W.WordValue
  } deriving (Eq, Show)
data FirstTask = FirstTask
  {firstCore :: KernelCore, firstId :: TaskId, firstNode :: W.Address, firstCreation :: W.Address, firstStackTop :: W.Address
  ,firstEntry :: W.Address, firstSelectedHit :: HitIndex, firstTransferHit :: Maybe HitIndex, firstEntryHit :: Maybe HitIndex
  ,firstEntryCounts :: Maybe [W.Count], firstEnteredHit :: Maybe HitIndex} deriving (Eq, Show)
data Hit = Hit
  {hitBoundary :: Boundary, hitAddress :: W.Address, hitCore :: KernelCore, hitInstructions :: W.Count
  ,hitRegisters :: [W.WordValue], hitSp :: W.Address, hitLr :: W.Address, hitCpsr :: W.WordValue} deriving (Eq, Show)
data Receipt = Receipt
  {receiptSchema :: ReceiptProtocol, receiptScope :: Scope, receiptOutcome :: Outcome, receiptError :: Maybe String
  ,receiptMachineFailure :: Maybe String, receiptIdentity :: Identity, receiptCheckpoint :: W.Digest
  ,receiptCheckpointReused :: Bool, receiptInstructions :: W.Count, receiptStartupCounts :: Maybe [W.Count]
  ,receiptWatchDigest :: W.Digest, receiptWatch :: WatchResult, receiptWatchEndError :: Maybe String
  ,receiptSites :: M.Map W.Address Boundary, receiptHits :: [Hit], receiptSubmissions :: [Submission]
  ,receiptFirstTasks :: [FirstTask], receiptSamples :: [Sample], receiptMenu :: MenuResult, receiptProgram :: ProgramEvidence} deriving (Eq, Show)

coreIndex :: KernelCore -> Int
coreIndex PrimaryKernel = 0
coreIndex SecondaryKernel = 1
watchCore :: KernelCore -> W.Core
watchCore PrimaryKernel = W.Primary
watchCore SecondaryKernel = W.Secondary
hexWord :: String -> Parser Word32
hexWord ('0':'x':digits) = case readHex digits :: [(Integer,String)] of
  [(number,"")] | number >= 0 && number <= 0xffffffff -> pure (fromInteger number)
  _ -> fail "invalid hexadecimal word"
hexWord _ = fail "word lacks hexadecimal prefix"
instance FromJSON Scope where
  parseJSON = withText "scope" $ \scope -> case scope of
    "stock-natural-reset-startup-task-objects-through-both-first-prologue-instructions;unwatched-continuation-to-menu" -> pure StockStartupScope
    "owned-task-submission-reset-startup-task-objects-through-both-first-prologue-instructions;unwatched-continuation-to-menu" -> pure OwnedStartupScope
    "stock-controlled-task-submission-after-both-first-prologue-instructions" -> pure StockProbeScope
    "owned-controlled-task-submission-after-both-first-prologue-instructions" -> pure OwnedProbeScope
    "owned-task-composition-reset-startup-task-objects-through-both-first-prologue-instructions;unwatched-continuation-to-menu" -> pure JoinedStartupScope
    "owned-composed-controlled-task-submission-after-both-first-prologue-instructions" -> pure JoinedProbeScope
    "retained-task-composition-reset-startup-task-objects-through-both-first-prologue-instructions;unwatched-continuation-to-menu" -> pure StockJoinedStartupScope
    "retained-composed-controlled-task-submission-after-both-first-prologue-instructions" -> pure StockJoinedProbeScope
    _ -> fail "unknown task observation scope"
instance FromJSON ReceiptProtocol where
  parseJSON value = do
    protocol <- parseJSON value :: Parser Int
    case protocol of 1 -> pure LegacyStockProtocol; 2 -> pure TaskSubmissionProtocol; 3 -> pure JoinedTaskProtocol; _ -> fail "unsupported task receipt protocol"
instance FromJSON ProgramEvidence where
  parseJSON = withObject "task program" $ \o -> do
    kind <- o .: "kind" :: Parser String
    case kind of
      "stock" -> do unless (KM.size o == 1) (fail "unexpected stock program evidence"); pure StockProgram
      "owned" -> do
        unless (KM.size o == 4) (fail "incomplete owned program evidence")
        OwnedProgram <$> o .: "contract_sha256" <*> o .: "linked_sha256" <*> o .: "symbols_sha256"
      "joined" -> do
        unless (KM.size o == 8) (fail "incomplete joined program evidence")
        JoinedProgram <$> (JoinedEvidence <$> o .: "contract_sha256" <*> o .: "linked_sha256" <*> o .: "symbols_sha256"
          <*> o .: "critical_linked_sha256" <*> o .: "critical_symbols_sha256"
          <*> o .: "storage_linked_sha256" <*> o .: "storage_symbols_sha256")
      _ -> fail "unknown task program"
parseBoundary :: String -> Parser Boundary
parseBoundary value = case value of
  "submission" -> pure SubmissionEntry; "auxiliary-return" -> pure AuxiliaryReturn; "creation-call" -> pure CreationCall
  "creation-return" -> pure CreationReturn; "activation-call" -> pure ActivationCall; "activation-return" -> pure ActivationReturn
  "wrapper-return" -> pure WrapperReturn; "auto-ready-entry" -> pure AutoReadyEntry; "auto-ready-return" -> pure AutoReadyReturn
  "prepare" -> pure Prepare; "prepared" -> pure Prepared; "scheduler" -> pure Scheduler; "selected" -> pure Selected
  "stack-transfer" -> pure StackTransfer; "trampoline" -> pure Trampoline; "entry-transfer" -> pure EntryTransfer
  "task-entry" -> pure TaskEntry; "entered-instruction" -> pure EnteredInstruction
  "auxiliary-call" -> pure AuxiliaryCall
  "critical-enter-call" -> pure CriticalEnterCall; "critical-enter-return" -> pure CriticalEnterReturn
  "critical-exit-call" -> pure CriticalExitCall; "critical-exit-return" -> pure CriticalExitReturn
  "reservation-call" -> pure ReservationCall; "reservation-return" -> pure ReservationReturn
  "fill-call" -> pure FillCall; "fill-return" -> pure FillReturn; "cache-call" -> pure CacheCall; "cache-return" -> pure CacheReturn
  "log-call" -> pure LogCall; "log-return" -> pure LogReturn; "error-callback-call" -> pure ErrorCallbackCall; "error-callback-return" -> pure ErrorCallbackReturn
  "identity-call" -> pure IdentityCall; "identity-return" -> pure IdentityReturn
  "save-mask-call" -> pure SaveMaskCall; "save-mask-return" -> pure SaveMaskReturn; "restore-mask-call" -> pure RestoreMaskCall; "restore-mask-return" -> pure RestoreMaskReturn
  "try-lock-call" -> pure TryLockCall; "try-lock-return" -> pure TryLockReturn; "unlock-call" -> pure UnlockCall; "unlock-return" -> pure UnlockReturn
  "release-entry" -> pure ReleaseEntry; "release-return" -> pure ReleaseReturn; "deletion-entry" -> pure DeletionEntry
  "native-write" -> pure NativeWrite
  _ -> fail "unknown startup task boundary"
instance FromJSON Boundary where parseJSON value = parseJSON value >>= parseBoundary
instance FromJSON KernelCore where
  parseJSON = withText "kernel core" $ \value -> case value of "primary" -> pure PrimaryKernel; "secondary" -> pure SecondaryKernel; _ -> fail "invalid kernel core"
instance FromJSON TaskId where
  parseJSON value = do
    number <- parseJSON value
    unless (number >= 1 && number <= 120) (fail "task id outside canonical table")
    pure (TaskId number)
instance FromJSON HitIndex where parseJSON value = do number <- parseJSON value; unless (number >= 0 && number < 4096) (fail "hit index exceeds bound"); pure (HitIndex number)
instance FromJSON Cursor where parseJSON value = do number <- parseJSON value; unless (number >= 0 && number <= 1048576) (fail "watch cursor exceeds bound"); pure (Cursor number)
instance FromJSON SourceKind where
  parseJSON = withText "source name" $ \value -> case value of
    "decode/loader-40000.bin" -> pure LoaderA; "decode/loader-d0000.bin" -> pure LoaderB; "lz/id4-0061c400.bin" -> pure Image4
    "record2/critical-pair.bin" -> pure Record2
    _ -> fail "unknown canonical source"
instance FromJSON SourceIdentity where
  parseJSON = withObject "source identity" $ \o -> SourceIdentity <$> o .: "name" <*> o .: "base" <*> o .: "bytes" <*> o .: "sha256"
instance FromJSON Identity where
  parseJSON = withObject "identity" $ \o -> Identity <$> o .: "run" <*> o .: "update" <*> o .: "boot_rom" <*> o .: "observer" <*> o .: "sources"
instance FromJSON ObjectKind where
  parseJSON = withText "object" $ \value -> case value of
    "creation-records" -> pure CreationRecords; "task-nodes" -> pure TaskNodes; "id-map" -> pure IdMap
    "auxiliary-records" -> pure AuxiliaryRecords; "auxiliary-nodes" -> pure AuxiliaryNodes; "domains" -> pure Domains
    "control" -> pure Control; "queue-heads" -> pure QueueHeads; "bitmap" -> pure Bitmap; "selected" -> pure SelectedObject
    "current" -> pure Current; "descriptor" -> pure Descriptor; "wrapper" -> pure Wrapper; "storage" -> pure Storage
    "stack" -> pure Stack; "entry-code" -> pure EntryCode; "shared-domains" -> pure SharedDomains
    "interface-roots" -> pure InterfaceRoots; "interface-table" -> pure InterfaceTable
    "parameters" -> pure Parameters; "error-state" -> pure ErrorState; "callback" -> pure Callback; "cursor" -> pure CursorObject; "lock" -> pure LockObject
    "class-table" -> pure ClassTable; "control-patch" -> pure ControlPatch; "log-callback" -> pure LogCallback; "log-mode" -> pure LogMode; "log-enable" -> pure LogEnable
    "critical-counter" -> pure CriticalCounter; "critical-lock" -> pure CriticalLock
    _ -> fail "unknown task object"
instance FromJSON Context where
  parseJSON value = withObject "context" (\o -> Context <$> o .: "core" <*> parseJSON value) value
instance FromJSON View where
  parseJSON = withObject "view" $ \o -> View <$> o .: "object" <*> o .: "physical" <*> o .: "bytes" <*> o .: "aliases"
instance FromJSON Task where
  parseJSON = withObject "task" $ \o -> Task <$> o .: "id" <*> o .: "creation" <*> o .: "creation_pointer" <*> o .: "creation_id" <*> o .: "node"
    <*> o .: "flags" <*> o .: "entry" <*> o .: "argument" <*> o .: "priority" <*> o .: "stack_bytes" <*> o .: "stack_base"
    <*> o .: "created" <*> o .: "state" <*> o .: "effective_priority" <*> o .: "pending" <*> o .: "saved_sp" <*> o .: "resume_pc"
instance FromJSON Kernel where
  parseJSON = withObject "kernel" $ \o -> Kernel <$> o .: "core" <*> o .: "views" <*> o .: "tasks" <*> o .: "domain" <*> o .: "selected" <*> o .: "current"
instance FromJSON Sample where
  parseJSON = withObject "sample" $ \o -> Sample <$> o .: "hit" <*> o .: "boundary" <*> o .: "context" <*> o .: "code_word" <*> o .: "watch_cursor" <*> o .: "kernels" <*> o .: "local" <*> o .:? "call_taken"
instance FromJSON Submission where
  parseJSON = withObject "submission" $ \o -> Submission <$> o .: "id" <*> o .: "core" <*> o .: "descriptor" <*> o .: "caller_sp" <*> o .: "return_pc" <*> o .: "entry_hit" <*> o .:? "return_hit" <*> o .:? "status"
instance FromJSON FirstTask where
  parseJSON = withObject "first task" $ \o -> FirstTask <$> o .: "core" <*> o .: "id" <*> o .: "node" <*> o .: "creation" <*> o .: "stack_top" <*> o .: "entry"
    <*> o .: "selected_hit" <*> o .:? "transfer_hit" <*> o .:? "entry_hit" <*> o .:? "entry_counts" <*> o .:? "entered_instruction_hit"
instance FromJSON Hit where
  parseJSON = withObject "hit" $ \o -> do
    label <- o .: "site" >>= parseBoundary
    address <- o .: "address" >>= hexWord
    core <- o .: "core" :: Parser Int
    kernel <- case core of 0 -> pure PrimaryKernel; 1 -> pure SecondaryKernel; _ -> fail "invalid hit core"
    instructions <- o .: "instructions"
    values <- o .: "registers" :: Parser (M.Map String String)
    let names = ["r" ++ show index | index <- [0 :: Int ..12]]
    unless (M.keysSet values == M.keysSet (M.fromList [(name,()) | name <- names])) (fail "incomplete full register context")
    registers <- mapM (\name -> maybe (fail "missing register") (fmap W.WordValue . hexWord) (M.lookup name values)) names
    sp <- o .: "sp" >>= hexWord; lr <- o .: "lr" >>= hexWord; cpsr <- o .: "cpsr"
    pure (Hit label (W.Address address) kernel instructions registers (W.Address sp) (W.Address lr) cpsr)
instance FromJSON Outcome where
  parseJSON = withText "outcome" $ \value -> case value of "observed-menu" -> pure ObservedMenu; "observed-probe" -> pure ObservedProbe; "refused" -> pure Refused; _ -> fail "invalid outcome"
instance FromJSON WatchResult where
  parseJSON = withObject "watch result" $ \o -> do
    unless (KM.size o == 1 && (KM.member "Ok" o || KM.member "Err" o)) (fail "ambiguous watch result")
    good <- o .:? "Ok"
    case good of
      Just value -> withObject "watch summary" (\summary -> WatchOk <$> summary .: "enabled" <*> summary .:? "violation") value
      Nothing -> WatchError <$> o .: "Err"
instance FromJSON MenuResult where
  parseJSON = withObject "menu result" $ \o -> do
    unless (KM.size o == 1 && (KM.member "Ok" o || KM.member "Err" o)) (fail "ambiguous menu result")
    good <- o .:? "Err"
    case good of Just problem -> pure (MenuError problem); Nothing -> MenuOk <$> o .: "Ok"
instance FromJSON Receipt where
  parseJSON = withObject "startup task receipt" $ \o -> do
    rawSites <- o .: "sites" :: Parser (M.Map String String)
    sites <- mapM (\(key,label) -> do
      number <- case reads key :: [(Integer,String)] of [(value,"")] | value >= 0 && value <= 0xffffffff -> pure (fromInteger value); _ -> fail "invalid site address"
      kind <- parseBoundary label; pure (W.Address number,kind)) (M.toList rawSites)
    Receipt <$> o .: "schema" <*> o .: "scope" <*> o .: "outcome" <*> o .:? "error" <*> o .:? "machine_failure"
      <*> o .: "identity" <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions"
      <*> o .:? "startup_end_counts" <*> o .: "memory_watch_sha256" <*> o .: "watch" <*> o .:? "watch_end_error"
      <*> pure (M.fromList sites) <*> o .: "hits" <*> o .: "submissions" <*> o .: "first_tasks" <*> o .: "samples" <*> o .: "final_menu" <*> o .:? "program" .!= LegacyStockProgram
