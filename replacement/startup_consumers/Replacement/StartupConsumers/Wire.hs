{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.Wire where

import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Text as T
import Data.Word (Word64)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Digest (..), Bytes (..), Core (..), Watch (..))
import qualified Replacement.KernelInstall.Wire as K

data Role = Retained | Installer | Joined deriving (Eq, Ord, Show, Enum, Bounded)
data Family = TaskWrapper | TaskReserve | StorageReserve | MessageConstruct | MessageSend | MessageReceive deriving (Eq, Ord, Show, Enum, Bounded)
data ProbeKind = OuterEntry | OuterReturn | InstallerEntry | InstallerReturn | CacheEntry | CacheReturn
  | CompletionStore | CompletionBeforeDsb | CompletionAfterDsb | PeerPoll | PeerPostload | PeerBranch
  | InitializerCall | InitializerEntry | InitializerReturn | CriticalEntry | CriticalIdentityCall
  | CriticalIdentityReturn | CriticalRootLoad | CriticalSlotLoad | CriticalCall | CriticalReturn
  | FamilyEntry | SourceProbe | DynamicTarget | DynamicReturn | FamilyReturn deriving (Eq, Ord, Show)
data ResourceKind = Roots | RootObject | ServiceStorage | TaskStorage | Stack | Code | Completion
  | Descriptor | Record | Cursor | Output | Auxiliary | Payload | Buffer | CallbackCell | ErrorState | Header | Carrier deriving (Eq, Ord, Show)
data Lifetime = ObservationLifetime | InvocationLifetime | AllocationLifetime | ReceiveReleaseLifetime deriving (Eq, Ord, Show)
data Status = Returned | NotObserved | Unclosed deriving (Eq, Show)
data Outcome = Observed | NonObservation | Refused deriving (Eq, Show)
data ComponentKind = InstallerComponent | DispatcherComponent deriving (Eq, Ord, Show)
data FetchClassification = BoundaryAttempt deriving (Eq, Show)
newtype ProbeId = ProbeId Word64 deriving (Eq, Ord, Show)
newtype InvocationId = InvocationId Word64 deriving (Eq, Ord, Show)
newtype ResourceId = ResourceId Word64 deriving (Eq, Ord, Show)
newtype SegmentId = SegmentId Word64 deriving (Eq, Ord, Show)
data Context = Context {contextCore :: Core, contextMachine :: K.Context} deriving (Eq, Show)
data Component = Component {componentKind :: ComponentKind, componentImage :: Digest, componentSymbols :: Digest, componentContract :: Digest} deriving (Eq, Show)
data Identity = Identity
  {identityRun :: Digest, identityUpdate :: Digest, identityCanonical :: Digest, identityImage4 :: Digest
  ,identityLoaderA :: Digest, identityLoaderB :: Digest, identityObserver :: Digest, identityComponents :: [Component]
  ,identityRom :: Digest, identityJoinedContract :: Digest} deriving (Eq, Show)
data Limits = Limits
  {limitInstructions :: Count, limitSegmentInstructions :: Count, limitRecords :: Count, limitProbes :: Count
  ,limitResources :: Count, limitResourceBytes :: Count, limitInvocations :: Count, limitTotalRecords :: Count
  ,limitSnapshotBytes :: Count, limitSegments :: Count, limitSnapshots :: Count} deriving (Eq, Show)
data View = View {viewCore :: Core, viewGuest :: Address, viewPhysical :: Address, viewBytes :: Bytes} deriving (Eq, Show)
data Snapshot = Snapshot {snapshotResource :: ResourceId, snapshotViews :: [View]} deriving (Eq, Show)
data Probe = Probe
  {probeId :: ProbeId, probeKind :: ProbeKind, probeFamily :: Maybe Family, probeInvocation :: Maybe InvocationId
  ,probeContext :: Context, probeSourceWord :: WordValue, probeSnapshots :: [Snapshot]} deriving (Eq, Show)
data Invocation = Invocation
  {invocationId :: InvocationId, invocationFamily :: Family, invocationCore :: Core
  ,invocationEntry :: Maybe ProbeId, invocationReturn :: Maybe ProbeId, invocationParent :: Maybe InvocationId
  ,invocationStatus :: Status, invocationLr :: Address, invocationSp :: Address} deriving (Eq, Show)
data Census = Census {censusFamily :: Family, censusCore :: Core, censusEntries :: Count, censusFirst :: Maybe ProbeId} deriving (Eq, Show)
data Resource = Resource
  {resourceId :: ResourceId, resourceKind :: ResourceKind, resourceLifetime :: Lifetime, resourceOwner :: Maybe InvocationId
  ,resourcePhysical :: Address, resourceSize :: Count, resourceAdmitted :: Context, resourceInitial :: [View]
  ,resourceClosed :: Maybe Context, resourceFinal :: [View]} deriving (Eq, Show)
data TraceStop = TraceStop {stopCore :: Core, stopPc :: Address, stopInstructions :: Count} deriving (Eq, Show)
data Segment = Segment
  {segmentId :: SegmentId, segmentStart :: [Count], segmentEnd :: [Count], segmentStartContexts :: [Context]
  ,segmentEndContexts :: [Context], segmentStops :: [TraceStop], segmentWatch :: Watch, segmentError :: Maybe String} deriving (Eq, Show)
data FetchAttempt = FetchAttempt
  {fetchSegment :: SegmentId, fetchRow :: Count, fetchClassification :: FetchClassification} deriving (Eq, Show)
data PayloadState = EmptyPayload | AdmittedPayload deriving (Eq, Show)
data PayloadAdmission = PayloadAdmission
  {payloadOwner :: InvocationId, payloadContext :: Context, payloadPointer :: Address, payloadLength :: Count
  ,payloadState :: PayloadState, payloadResource :: Maybe ResourceId} deriving (Eq, Show)
data Receipt = Receipt
  {receiptSchema :: Int, receiptRole :: Role, receiptOutcome :: Outcome, receiptError :: Maybe String, receiptFailure :: Maybe String
  ,receiptIdentity :: Identity, receiptCheckpoint :: Digest, receiptCheckpointReused :: Bool, receiptInstructions :: Count
  ,receiptLimits :: Limits, receiptProbes :: [Probe], receiptInvocations :: [Invocation], receiptResources :: [Resource]
  ,receiptSegments :: [Segment], receiptCensus :: [Census], receiptFetches :: [FetchAttempt], receiptPayloads :: [PayloadAdmission]} deriving (Eq, Show)

tagged :: String -> [(T.Text,a)] -> Value -> Parser a
tagged name choices = withText name $ \tag -> maybe (fail ("invalid " ++ name)) pure (lookup tag choices)
instance FromJSON Role where parseJSON = tagged "role" [("retained",Retained),("installer",Installer),("joined",Joined)]
instance FromJSON Family where parseJSON = tagged "family" [("task-wrapper",TaskWrapper),("task-reserve",TaskReserve),("storage-reserve",StorageReserve),("message-construct",MessageConstruct),("message-send",MessageSend),("message-receive",MessageReceive)]
instance FromJSON ProbeKind where
  parseJSON = tagged "probe kind"
    [("outer-entry",OuterEntry),("outer-return",OuterReturn),("installer-entry",InstallerEntry),("installer-return",InstallerReturn)
    ,("cache-entry",CacheEntry),("cache-return",CacheReturn),("completion-store",CompletionStore),("completion-before-dsb",CompletionBeforeDsb)
    ,("completion-after-dsb",CompletionAfterDsb),("peer-poll",PeerPoll),("peer-postload",PeerPostload),("peer-branch",PeerBranch)
    ,("initializer-call",InitializerCall),("initializer-entry",InitializerEntry),("initializer-return",InitializerReturn)
    ,("critical-entry",CriticalEntry),("critical-identity-call",CriticalIdentityCall),("critical-identity-return",CriticalIdentityReturn)
    ,("critical-root-load",CriticalRootLoad),("critical-slot-load",CriticalSlotLoad),("critical-call",CriticalCall),("critical-return",CriticalReturn)
    ,("family-entry",FamilyEntry),("source",SourceProbe),("dynamic-target",DynamicTarget),("dynamic-return",DynamicReturn),("family-return",FamilyReturn)]
instance FromJSON ResourceKind where
  parseJSON = tagged "resource kind" [("roots",Roots),("root-object",RootObject),("service-storage",ServiceStorage),("task-storage",TaskStorage)
    ,("stack",Stack),("code",Code),("completion",Completion),("descriptor",Descriptor),("record",Record),("cursor",Cursor),("output",Output)
    ,("auxiliary",Auxiliary),("payload",Payload),("buffer",Buffer),("callback-cell",CallbackCell),("error-state",ErrorState),("header",Header),("carrier",Carrier)]
instance FromJSON Lifetime where parseJSON = tagged "lifetime" [("observation",ObservationLifetime),("invocation",InvocationLifetime),("allocation",AllocationLifetime),("receive-release",ReceiveReleaseLifetime)]
instance FromJSON Status where parseJSON = tagged "status" [("returned",Returned),("not-observed",NotObserved),("unclosed",Unclosed)]
instance FromJSON Outcome where parseJSON = tagged "outcome" [("observed",Observed),("non-observation",NonObservation),("refused",Refused)]
instance FromJSON ComponentKind where parseJSON = tagged "component kind" [("installer",InstallerComponent),("dispatcher",DispatcherComponent)]
instance FromJSON FetchClassification where parseJSON = tagged "fetch classification" [("boundary-attempt",BoundaryAttempt)]
instance FromJSON PayloadState where parseJSON = tagged "payload state" [("empty",EmptyPayload),("admitted",AdmittedPayload)]
instance FromJSON PayloadAdmission where
  parseJSON = withObject "payload admission" $ \o -> PayloadAdmission <$> o .: "owner" <*> o .: "context"
    <*> o .: "pointer" <*> o .: "length" <*> o .: "state" <*> o .: "resource"
instance FromJSON ProbeId where parseJSON value = ProbeId <$> parseJSON value
instance FromJSON InvocationId where parseJSON value = InvocationId <$> parseJSON value
instance FromJSON ResourceId where parseJSON value = ResourceId <$> parseJSON value
instance FromJSON SegmentId where parseJSON value = SegmentId <$> parseJSON value
instance FromJSON Context where parseJSON value = withObject "context" (\o -> Context <$> o .: "core" <*> parseJSON value) value
instance FromJSON Component where parseJSON = withObject "component" $ \o -> Component <$> o .: "kind" <*> o .: "image" <*> o .: "symbols" <*> o .: "contract"
instance FromJSON Identity where
  parseJSON = withObject "identity" $ \o -> Identity <$> o .: "run" <*> o .: "update" <*> o .: "canonical_dat" <*> o .: "image4"
    <*> o .: "loader_a" <*> o .: "loader_b" <*> o .: "observer" <*> o .: "components" <*> o .: "boot_rom" <*> o .: "joined_contract"
instance FromJSON Limits where
  parseJSON = withObject "limits" $ \o -> Limits <$> o .: "instructions" <*> o .: "segment_instructions" <*> o .: "watch_records"
    <*> o .: "probes" <*> o .: "resources" <*> o .: "resource_bytes" <*> o .: "invocations"
    <*> o .: "records" <*> o .: "snapshot_bytes" <*> o .: "segments" <*> o .: "snapshots"
instance FromJSON View where parseJSON = withObject "view" $ \o -> View <$> o .: "core" <*> o .: "guest" <*> o .: "physical" <*> o .: "bytes"
instance FromJSON Snapshot where parseJSON = withObject "snapshot" $ \o -> Snapshot <$> o .: "resource" <*> o .: "views"
instance FromJSON Probe where
  parseJSON = withObject "probe" $ \o -> Probe <$> o .: "sequence" <*> o .: "kind" <*> o .: "family" <*> o .: "invocation"
    <*> o .: "context" <*> o .: "source_word" <*> o .: "snapshots"
instance FromJSON Invocation where
  parseJSON = withObject "invocation" $ \o -> Invocation <$> o .: "id" <*> o .: "family" <*> o .: "core" <*> o .: "entry_probe"
    <*> o .: "return_probe" <*> o .: "parent" <*> o .: "status" <*> o .: "entry_lr" <*> o .: "entry_sp"
instance FromJSON Census where parseJSON = withObject "census" $ \o -> Census <$> o .: "family" <*> o .: "core" <*> o .: "entries" <*> o .: "first_probe"
instance FromJSON Resource where
  parseJSON = withObject "resource" $ \o -> Resource <$> o .: "id" <*> o .: "kind" <*> o .: "lifetime" <*> o .: "owner" <*> o .: "physical"
    <*> o .: "size" <*> o .: "admitted" <*> o .: "initial" <*> o .: "closed" <*> o .: "final_views"
instance FromJSON TraceStop where
  parseJSON = withObject "trace stop" $ \o -> TraceStop <$> o .: "core" <*> o .: "pc" <*> o .: "instructions"
instance FromJSON Segment where
  parseJSON = withObject "segment" $ \o -> Segment <$> o .: "id" <*> o .: "start_counts" <*> o .: "end_counts"
    <*> o .: "start_contexts" <*> o .: "end_contexts" <*> o .: "stops" <*> o .: "watch" <*> o .: "end_error"
instance FromJSON FetchAttempt where
  parseJSON = withObject "fetch attempt" $ \o -> FetchAttempt <$> o .: "segment" <*> o .: "row" <*> o .: "classification"
instance FromJSON Receipt where
  parseJSON = withObject "receipt" $ \o -> Receipt <$> o .: "schema" <*> o .: "role" <*> o .: "outcome" <*> o .: "error" <*> o .: "machine_failure"
    <*> o .: "identity" <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions" <*> o .: "limits"
    <*> o .: "probes" <*> o .: "invocations" <*> o .: "resources" <*> o .: "segments" <*> o .: "census" <*> o .: "fetch_attempts" <*> o .: "payloads"
