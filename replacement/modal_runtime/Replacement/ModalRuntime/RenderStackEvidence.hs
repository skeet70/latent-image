{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.ModalRuntime.RenderStackEvidence
  ( StackRole(..), StackKind(..), InstructionCounter(..), StackBoundary(..)
  , RenderStackEvidence, AllocationVerdict, EntryWitness(..)
  , StackAddress(..), CodeAddress(..), FrameBytes(..)
  , parseRenderStackEvidence, allocationVerdict, checkEntryWitnesses, evidenceInterval
  , checkLifetimeGuard, checkOwnedSources, allocationVerdictJson
  ) where

import qualified ArmElf as ELF
import Control.Monad (foldM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Foldable as F
import Data.List (find, nub, sort, sortOn)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Numeric (showHex)

data StackRole = StockStack | OwnedStack deriving (Eq, Show)
data StackKind = TreeStack | PickerStack | PaneStack deriving (Eq, Ord, Show)
newtype InstructionCounter = InstructionCounter Word64 deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype GuestTick = GuestTick Word64 deriving newtype (J.FromJSON, Eq, Ord, Show)
data StackBoundary = StackBoundary InstructionCounter GuestTick deriving (Eq, Show)
newtype StackAddress = StackAddress Word32 deriving newtype (J.FromJSON, J.ToJSON, Eq, Ord, Show)
newtype CodeAddress = CodeAddress Word32 deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype FrameBytes = FrameBytes Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype InvocationId = InvocationId Word64 deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype ObservationOrdinal = ObservationOrdinal Word64 deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype TaskId = TaskId Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype NodeAddress = NodeAddress Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype CreationAddress = CreationAddress Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype KernelPriority = KernelPriority Word32 deriving (Eq, Show)
newtype StackExtent = StackExtent Word32 deriving (Eq, Show)
newtype CreationState = CreationState Word32 deriving (Eq, Show)
data CpuCore = Core0 | Core1 deriving (Eq, Show)
data SupervisorMode = SupervisorMode deriving (Eq, Show)
newtype RawCpsr = RawCpsr Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype ContextDepth = ContextDepth Word32 deriving newtype (J.FromJSON, Eq, Show)
newtype InvocationCount = InvocationCount Int deriving newtype (J.ToJSON, Eq, Show)
data Descriptor = Descriptor Word32 Word32 CodeAddress KernelPriority StackExtent StackAddress Word32 Word32 TaskId CreationState deriving (Eq, Show)
data Kernel = Primary | Secondary deriving (Eq, Show)
data Identity = Identity Kernel TaskId NodeAddress CreationAddress Descriptor deriving (Eq, Show)
data Terminal = OwnedReturn | FallbackHandoff | StockReturn deriving (Eq, Show)
data Coordinate = EntryBoundary StackBoundary | ObservationCoordinate ObservationOrdinal deriving (Eq, Show)
data ModeSource = ModeSource CodeAddress Word32 deriving (Eq, Show)
data MinimumContext = MinimumContext Coordinate CpuCore RawCpsr ContextDepth Identity (Maybe Identity) (Maybe Bool) (Maybe ModeSource) deriving (Eq, Show)
data SourceKind = OwnedSource | RetainedSource deriving (Eq, Ord, Show)
newtype SourcePath = SourcePath FilePath deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype SourceDigest = SourceDigest T.Text deriving (Eq, Ord, Show)
data SourceBinding = SourceBinding SourceKind CodeAddress SourcePath SourceDigest deriving (Eq, Ord, Show)
newtype PhysicalAddress = PhysicalAddress Word32 deriving newtype (J.FromJSON, Eq, Ord, Show)
newtype GuardBaseline = GuardBaseline B.ByteString deriving (Eq)
data GuardAccess = GuardWrites | GuardFetches deriving (Eq, Show)
data GuardClass = GuardClass PhysicalAddress PhysicalAddress GuardAccess
data TransitionContext = TransitionContext CodeAddress RawCpsr ContextDepth NodeAddress (Maybe NodeAddress) Bool (Maybe ModeSource) deriving (Eq, Show)
data TransitionSample = TransitionSample ObservationOrdinal TransitionContext deriving (Eq, Show)
data TransitionRun = TransitionSingle TransitionSample | TransitionPair TransitionSample TransitionSample Word64 Word64 Bool deriving (Eq, Show)
data TransitionSpan = TransitionSpan ObservationOrdinal ObservationOrdinal deriving (Eq, Show)
data Invocation = Invocation
  { invocationId :: InvocationId
  , invocationParent :: Maybe InvocationId
  , invocationKind :: StackKind
  , invocationCore :: CpuCore
  , invocationIdentity :: Identity
  , invocationEntryPc :: CodeAddress
  , invocationEntrySp :: StackAddress
  , invocationEntryLr :: CodeAddress
  , invocationStart :: StackBoundary
  , invocationEnd :: StackBoundary
  , invocationExitSp :: StackAddress
  , invocationMinimum :: StackAddress
  , invocationDeepestPc :: CodeAddress
  , invocationMinimumContext :: MinimumContext
  , invocationTerminal :: Terminal
  , invocationFrame :: FrameBytes
  , invocationAllowance :: FrameBytes
  , invocationEstimatePassed :: Bool
  , invocationTransitionSpan :: TransitionSpan
  , invocationTransitionCount :: Word64
  , invocationTransitions :: [TransitionRun]
  } deriving (Eq, Show)
newtype ActiveObservations = ActiveObservations Word64 deriving newtype (J.FromJSON, Eq, Show)
data RenderStackEvidence = RenderStackEvidence StackRole StackBoundary StackBoundary ActiveObservations [SourceBinding] [Invocation] AllocationVerdict deriving (Eq, Show)
data EntryWitness = EntryWitness StackKind InstructionCounter StackAddress CodeAddress deriving (Eq, Show)

-- | The diagnostic interval a report covers; entries outside it are post-unload activity.
evidenceInterval :: RenderStackEvidence -> (InstructionCounter, InstructionCounter)
evidenceInterval (RenderStackEvidence _ (StackBoundary start _) (StackBoundary end _) _ _ _ _) = (start, end)
data AllocationVerdict = AllocationVerdict StackRole InvocationCount InvocationCount InvocationCount (Maybe StackAddress) deriving (Eq, Show)

instance J.FromJSON CpuCore where
  parseJSON value = do
    core <- J.parseJSON value :: J.Parser Word32
    case core of 0 -> pure Core0; 1 -> pure Core1; _ -> fail "unknown render CPU core"
instance J.FromJSON SupervisorMode where
  parseJSON value = do
    mode <- J.parseJSON value :: J.Parser Word32
    unless (mode == 0x13) (fail "render entry is not SVC")
    pure SupervisorMode

instance J.FromJSON StackRole where
  parseJSON = J.withText "render stack role" $ \tag -> case tag of
    "stock" -> pure StockStack
    "owned" -> pure OwnedStack
    _ -> fail "unknown render stack role"
instance J.FromJSON StackKind where
  parseJSON = J.withText "render stack kind" $ \tag -> case tag of
    "tree" -> pure TreeStack
    "picker" -> pure PickerStack
    "pane" -> pure PaneStack
    _ -> fail "unknown render stack kind"
instance J.FromJSON StackBoundary where
  parseJSON = J.withObject "render stack boundary" $ \fields -> StackBoundary <$> fields J..: "instructions" <*> fields J..: "tick"
instance J.FromJSON Identity where
  parseJSON = J.withObject "render stack task identity" $ \fields -> do
    kernel <- fields J..: "kernel" >>= \tag -> case (tag :: T.Text) of
      "primary" -> pure Primary
      "secondary" -> pure Secondary
      _ -> fail "unknown render stack kernel"
    descriptor <- fields J..: "descriptor" >>= \words_ -> case words_ of
      [a,b,entry,priority,size,base,c,d,task,created] -> pure
        (Descriptor a b (CodeAddress entry) (KernelPriority priority) (StackExtent size) (StackAddress base) c d (TaskId task) (CreationState created))
      _ -> fail "render stack descriptor is not ten words"
    Identity kernel <$> fields J..: "task" <*> fields J..: "node" <*> fields J..: "creation" <*> pure descriptor
instance J.FromJSON Terminal where
  parseJSON = J.withText "render stack terminal" $ \tag -> case tag of
    "owned-return" -> pure OwnedReturn
    "fallback-handoff" -> pure FallbackHandoff
    "stock-return" -> pure StockReturn
    _ -> fail "unknown render stack terminal"
instance J.FromJSON Coordinate where
  parseJSON = J.withObject "render minimum coordinate" $ \fields -> do
    tag <- fields J..: "kind" :: J.Parser T.Text
    case tag of
      "entry-boundary" -> EntryBoundary <$> fields J..: "boundary"
      "observation" -> ObservationCoordinate <$> fields J..: "ordinal"
      _ -> fail "unknown render minimum coordinate"
instance J.FromJSON ModeSource where
  parseJSON = J.withObject "render minimum mode source" $ \fields -> ModeSource <$> fields J..: "pc" <*> fields J..: "word"
instance J.FromJSON MinimumContext where
  parseJSON = J.withObject "render minimum context" $ \fields -> MinimumContext
    <$> fields J..: "coordinate" <*> fields J..: "core" <*> fields J..: "cpsr" <*> fields J..: "context_depth"
    <*> fields J..: "current" <*> fields J..: "owner" <*> fields J..: "scheduler" <*> fields J..: "mode_source"
instance J.FromJSON SourceBinding where
  parseJSON = J.withObject "render source binding" $ \fields -> do
    kind <- fields J..: "kind" >>= \tag -> case (tag :: T.Text) of
      "owned" -> pure OwnedSource
      "retained" -> pure RetainedSource
      _ -> fail "unknown render source kind"
    digest <- fields J..: "sha256"
    unless (T.length digest == 64 && T.all (\c -> c >= '0' && c <= '9' || c >= 'a' && c <= 'f') digest)
      (fail "render source digest is not lowercase SHA256")
    SourceBinding kind <$> fields J..: "start" <*> fields J..: "path" <*> pure (SourceDigest digest)
instance J.FromJSON TransitionContext where
  parseJSON = J.withObject "render transition context" $ \fields -> TransitionContext
    <$> fields J..: "pc" <*> fields J..: "cpsr" <*> fields J..: "context_depth"
    <*> fields J..: "current_node" <*> fields J..: "owner_node" <*> fields J..: "scheduler" <*> fields J..: "mode_source"
instance J.FromJSON TransitionSample where
  parseJSON = J.withObject "render transition sample" $ \fields ->
    TransitionSample <$> fields J..: "ordinal" <*> fields J..: "context"
instance J.FromJSON TransitionRun where
  parseJSON value = J.withObject "render transition run" (\fields -> do
    kind <- fields J..: "kind" :: J.Parser T.Text
    case kind of
      "sample" -> TransitionSingle <$> J.parseJSON value
      "pair" -> TransitionPair <$> fields J..: "first" <*> fields J..: "second"
        <*> fields J..: "stride" <*> fields J..: "pairs" <*> fields J..: "trailing_first"
      _ -> fail "unknown render transition encoding entry") value
instance J.FromJSON TransitionSpan where
  parseJSON = J.withObject "render transition span" $ \fields ->
    TransitionSpan <$> fields J..: "first" <*> fields J..: "last"

boundedArray :: J.FromJSON a => Int -> J.Value -> J.Parser [a]
boundedArray limit = J.withArray "bounded render evidence array" $ \values -> do
  unless (F.length values <= limit) (fail "render evidence storage bound exceeded")
  mapM J.parseJSON (F.toList values)

transitionExtent :: TransitionRun -> Either String (Integer,Integer,Integer)
transitionExtent (TransitionSingle (TransitionSample (ObservationOrdinal ordinal) _)) =
  pure (toInteger ordinal,toInteger ordinal,1)
transitionExtent (TransitionPair (TransitionSample (ObservationOrdinal first) _) (TransitionSample (ObservationOrdinal second) _) stride pairs trailing) = do
  let a = toInteger first
      b = toInteger second
      step = toInteger stride
      n = toInteger pairs
      count = 2*n + if trailing then 1 else 0
      last_ = if trailing then a+n*step else b+(n-1)*step
  unless (n >= 1 && count >= 3 && a < b && b < a+step
    && last_ <= toInteger (maxBound :: Word64) && count <= toInteger (maxBound :: Word64))
    (Left "render transition pair has invalid order, count or overflow")
  pure (a,last_,count)

validateTransitions :: ActiveObservations -> Invocation -> Either String Integer
validateTransitions (ActiveObservations observations) invocation = do
  let TransitionSpan (ObservationOrdinal first) (ObservationOrdinal last_) = invocationTransitionSpan invocation
      StackBoundary (InstructionCounter entry) _ = invocationStart invocation
      StackBoundary (InstructionCounter terminal) _ = invocationEnd invocation
  unless (first > 0 && first <= last_ && last_ <= observations
    && toInteger last_ - toInteger first <= toInteger terminal - toInteger entry)
    (Left "render transition span leaves observed invocation interval")
  (_,count) <- foldM (\(previous,total) run -> do
    (a,b,n) <- transitionExtent run
    unless (toInteger first <= a && previous < a && b <= toInteger last_)
      (Left "render transition positions are unordered or outside invocation span")
    pure (b,total+n)) (0,0) (invocationTransitions invocation)
  unless (count == toInteger (invocationTransitionCount invocation)
    && count <= toInteger terminal - toInteger entry + 1)
    (Left "render logical transition count differs or exceeds instruction span")
  unless (count > 0) (Left "render invocation retains no transition")
  pure count

invocationSpan :: Invocation -> (Word64, Word64)
invocationSpan invocation =
  let TransitionSpan (ObservationOrdinal first) (ObservationOrdinal last_) = invocationTransitionSpan invocation
  in (first, last_)

validateInvocationSpans :: [Invocation] -> Either String ()
validateInvocationSpans invocations = do
  forM_ invocations $ \invocation -> case invocationParent invocation of
    Nothing -> pure ()
    Just parentId -> case find ((== parentId) . invocationId) invocations of
      Nothing -> Left "render invocation parent is absent"
      Just parent -> do
        let (parentFirst, parentLast) = invocationSpan parent
            (first, last_) = invocationSpan invocation
        unless (invocationCore parent == invocationCore invocation
          && parentFirst <= first && last_ <= parentLast)
          (Left "render nested invocation span leaves its parent")
  forM_ [Core0, Core1] $ \core -> do
    let roots = sortOn (fst . invocationSpan) [invocation | invocation <- invocations,
          invocationParent invocation == Nothing, invocationCore invocation == core]
    forM_ (zip roots (drop 1 roots)) $ \(earlier, later) -> do
      let (_, earlierLast) = invocationSpan earlier
          (laterFirst, _) = invocationSpan later
      unless (earlierLast < laterFirst) (Left "render root invocation spans overlap or are unordered")
instance J.FromJSON Invocation where
  parseJSON = J.withObject "render invocation" $ \fields -> do
    _ <- fields J..: "entry_mode" :: J.Parser SupervisorMode
    _ <- fields J..: "deepest_lr" :: J.Parser CodeAddress
    Invocation
      <$> fields J..: "id" <*> fields J..: "parent" <*> fields J..: "kind" <*> fields J..: "core"
      <*> fields J..: "task_identity" <*> fields J..: "entry_pc" <*> fields J..: "entry_sp" <*> fields J..: "entry_lr"
      <*> fields J..: "entry_boundary" <*> fields J..: "terminal_boundary"
      <*> fields J..: "exit_sp" <*> fields J..: "min_sp" <*> fields J..: "deepest_pc" <*> fields J..: "minimum_context"
      <*> fields J..: "terminal" <*> fields J..: "frame_depth" <*> fields J..: "allowance" <*> fields J..: "within_bound"
      <*> fields J..: "transition_span" <*> fields J..: "transition_count"
      <*> (fields J..: "transitions" >>= boundedArray 4096)

canonicalIdentity :: Identity
canonicalIdentity = Identity Primary (TaskId 33) (NodeAddress 0x4006060c) (CreationAddress 0x4005c3ac)
  (Descriptor 2 0 (CodeAddress 0x1109130) (KernelPriority 8) (StackExtent 4096) (StackAddress 0xa4ce00) 0 0 (TaskId 33) (CreationState 1))

ownerAllocation :: Identity -> (StackAddress, StackExtent)
ownerAllocation (Identity _ _ _ _ (Descriptor _ _ _ _ size base _ _ _ _)) = (base,size)

boundaryBetween :: StackBoundary -> StackBoundary -> StackBoundary -> Bool
boundaryBetween (StackBoundary first firstTick) (StackBoundary last_ lastTick) (StackBoundary at tick) =
  first <= at && at <= last_ && firstTick <= tick && tick <= lastTick

parseRenderStackEvidence :: StackRole -> J.Value -> J.Value -> Either String RenderStackEvidence
parseRenderStackEvidence expectedRole request value = J.parseEither parse value
 where
  parse = J.withObject "render stack evidence" $ \fields -> do
    version <- fields J..: "version" :: J.Parser Word32
    diagnosticOnly <- fields J..: "diagnostic_only"
    scope <- fields J..: "scope" :: J.Parser T.Text
    wholeRoute <- fields J..: "whole_route_stack_minimum_proven"
    coverage <- fields J..: "coverage" :: J.Parser T.Text
    failure <- fields J..: "failure" :: J.Parser (Maybe T.Text)
    active <- fields J..: "active_invocations" :: J.Parser Word32
    incomplete <- fields J..: "incomplete_intervals"
    finished <- fields J..: "finished"
    role <- fields J..: "role"
    start <- fields J..: "start"
    end <- fields J..: "end"
    unless (version == 2 && diagnosticOnly && scope == "installed-before-unload" && not wholeRoute
      && coverage == "continuous" && failure == Nothing && active == 0 && not incomplete && finished
      && role == expectedRole && boundaryBetween start end start)
      (fail "render stack evidence is refused, unfinished, mismatched or overclaimed")
    sources <- fields J..: "sources"
    expectedSources <- J.withObject "render stack request" (J..: "sources") request
    unless (not (null sources) && length sources <= 128 && length (nub sources) == length sources
      && sort sources == sort expectedSources) (fail "render stack source bindings differ from admitted request")
    (requestRole,entries,returns) <- J.withObject "render stack request" (\r -> do
      requestRole <- r J..: "role"
      entries <- r J..: "entries" >>= mapM (J.withObject "render entry specification" (\entry ->
        (,) <$> entry J..: "kind" <*> entry J..: "frame_depth"))
      returns <- r J..: "returns"
      pure (requestRole,entries,returns)) request
    unless (requestRole == role && sort (map fst entries) == [TreeStack,PickerStack,PaneStack]
      && not (null returns) && length (nub returns) == length returns)
      (fail "render stack request role, entries or return sites differ")
    partial <- fields J..: "retained_service_coverage" :: J.Parser T.Text
    informational <- fields J..: "retained_service_spans_informational"
    unless (partial == "partial-observed-source-transitions" && informational) (fail "render retained-service coverage is overclaimed")
    encoding <- fields J..: "transition_encoding" :: J.Parser T.Text
    unless (encoding == "pair-stride-v1") (fail "unsupported render transition encoding")
    rawInvocations <- fields J..: "invocations" >>= boundedArray 4096 :: J.Parser [J.Value]
    sizes <- mapM (J.withObject "render invocation storage" (\i ->
      i J..: "transitions" >>= J.withArray "render transition storage" (\rows -> do
        unless (F.length rows <= 4096) (fail "render per-invocation transition storage exceeded")
        pure (toInteger (F.length rows))))) rawInvocations
    let retained = sum sizes
    unless (retained <= 65536) (fail "render aggregate transition storage exceeded")
    invocations <- mapM J.parseJSON rawInvocations
    either fail pure (validateInvocationSpans invocations)
    (observations,count,hooks) <- fields J..: "performance" >>= J.withObject "render diagnostic performance" (\p ->
      (,,) <$> p J..: "active_instructions" <*> p J..: "invocation_count" <*> p J..: "hook_transitions")
    let roots = length (filter ((==Nothing) . invocationParent) invocations)
    unless (length invocations <= 4096 && count == length invocations && hooks == 2 * roots)
      (fail "render invocation coverage counters differ")
    let ActiveObservations activeInstructions = observations
        StackBoundary (InstructionCounter startInstructions) _ = start
        StackBoundary (InstructionCounter endInstructions) _ = end
    unless (toInteger activeInstructions <= toInteger endInstructions - toInteger startInstructions + 1)
      (fail "render active observations exceed observed guest instruction span")
    logical <- sum <$> mapM (either fail pure . validateTransitions observations) invocations
    unless (logical <= toInteger (maxBound :: Word64) && logical <= toInteger activeInstructions * 32)
      (fail "render logical transitions exceed observed active nesting")
    fields J..: "transition_storage" >>= J.withObject "render transition storage counters" (\storage -> do
      perInvocation <- storage J..: "per_invocation_limit" :: J.Parser Integer
      aggregate <- storage J..: "aggregate_limit" :: J.Parser Integer
      retainedCount <- storage J..: "retained_entries" :: J.Parser Integer
      allocatedCount <- storage J..: "allocated_entries" :: J.Parser Integer
      logicalCount <- storage J..: "logical_transitions" :: J.Parser Integer
      unless (perInvocation == 4096 && aggregate == 65536 && retainedCount == retained && logicalCount == logical
        && retainedCount <= allocatedCount && allocatedCount <= 65536)
        (fail "render transition storage limits or counters differ"))
    forM_ invocations $ \invocation -> do
      unless (lookup (invocationKind invocation) entries == Just (invocationFrame invocation))
        (fail "render invocation frame differs from admitted request")
      unless (invocationTerminal invocation == FallbackHandoff || invocationEntryLr invocation `elem` returns)
        (fail "render invocation return leaves admitted return sites")
    verdict <- either fail pure (validateAllocationEvidence role start end observations invocations)
    pure (RenderStackEvidence role start end observations sources invocations verdict)

physical :: StackAddress -> Word32
physical (StackAddress address) = address .&. 0xbfffffff

entryAddress :: StackKind -> CodeAddress
entryAddress TreeStack = CodeAddress 0x1164e1c
entryAddress PickerStack = CodeAddress 0x1163f2c
entryAddress PaneStack = CodeAddress 0x11662a0

allocationVerdict :: RenderStackEvidence -> AllocationVerdict
allocationVerdict (RenderStackEvidence _ _ _ _ _ _ verdict) = verdict

validateAllocationEvidence :: StackRole -> StackBoundary -> StackBoundary -> ActiveObservations -> [Invocation] -> Either String AllocationVerdict
validateAllocationEvidence role start end (ActiveObservations observations) invocations = do
  unless (map invocationId invocations == map InvocationId [1 .. fromIntegral (length invocations)])
    (Left "render invocation identifiers are missing or repeated")
  _ <- foldM (\open invocation -> do
    let entry = physical (invocationEntrySp invocation)
        minimum_ = physical (invocationMinimum invocation)
        FrameBytes frame = invocationFrame invocation
        FrameBytes allowance = invocationAllowance invocation
        (baseAddress,StackExtent size) = ownerAllocation (invocationIdentity invocation)
        base = physical baseAddress
        active = dropWhile (boundaryBetween start (invocationStart invocation) . invocationEnd) open
        estimatePassed = toInteger minimum_ >= toInteger entry - toInteger frame - toInteger allowance
        validReturn = case (role,invocationTerminal invocation) of
          (StockStack,StockReturn) -> physical (invocationExitSp invocation) == entry
          (OwnedStack,OwnedReturn) -> physical (invocationExitSp invocation) == entry
          (OwnedStack,FallbackHandoff) -> toInteger (physical (invocationExitSp invocation)) == toInteger entry - 36
          _ -> False
    unless (invocationIdentity invocation == canonicalIdentity && invocationCore invocation == Core0
      && invocationEntryPc invocation == entryAddress (invocationKind invocation))
      (Left "render invocation owner or entry identity differs")
    unless (all ((==0) . (`mod` 4)) [entry,minimum_,physical (invocationExitSp invocation)]
      && toInteger base + 64 <= toInteger minimum_ && minimum_ <= entry
      && toInteger entry <= toInteger base + toInteger size)
      (Left "render invocation minimum crosses validated allocation or existing bottom guard")
    unless (frame > 0 && frame <= 4096 && allowance == 1024 && estimatePassed == invocationEstimatePassed invocation)
      (Left "render invocation emitted frame or historical estimate receipt differs")
    unless validReturn
      (Left "render invocation return or fallback frame is not restored")
    unless (boundaryBetween start end (invocationStart invocation)
      && boundaryBetween (invocationStart invocation) end (invocationEnd invocation))
      (Left "render invocation interval leaves diagnostic coverage")
    case (invocationParent invocation,active) of
      (Nothing,[]) -> pure ()
      (Just parent,enclosing:_)
        | parent == invocationId enclosing && length active < 32
        , boundaryBetween (invocationStart enclosing) (invocationEnd enclosing) (invocationStart invocation)
        , boundaryBetween (invocationStart enclosing) (invocationEnd enclosing) (invocationEnd invocation)
        , physical (invocationMinimum enclosing) <= minimum_ -> pure ()
      _ -> Left "render nested invocation has missing or ambiguous enclosing minimum coverage"
    let MinimumContext coordinate core (RawCpsr cpsr) _ current owner scheduler _ = invocationMinimumContext invocation
    unless (core == Core0 && cpsr .&. 31 == 0x13) (Left "render minimum lacks attributed primary SVC context")
    case coordinate of
      EntryBoundary boundary -> unless (boundary == invocationStart invocation && minimum_ == entry
        && invocationDeepestPc invocation == invocationEntryPc invocation && current == canonicalIdentity
        && owner == Nothing && scheduler == Nothing) (Left "render initial minimum context differs from entry boundary")
      ObservationCoordinate (ObservationOrdinal ordinal) -> unless (ordinal > 0 && ordinal <= observations
        && owner == Just canonicalIdentity && scheduler /= Nothing)
        (Left "render minimum lacks its validated owner or observation coordinate")
    pure (invocation:active)) [] invocations
  let failures = length (filter (not . invocationEstimatePassed) invocations)
      fallbacks = length (filter ((==FallbackHandoff) . invocationTerminal) invocations)
      minimum_ = case invocations of [] -> Nothing; _ -> Just (StackAddress (minimum (map (physical . invocationMinimum) invocations)))
  pure (AllocationVerdict role (InvocationCount (length invocations)) (InvocationCount fallbacks) (InvocationCount failures) minimum_)

checkEntryWitnesses :: [(StackKind,FrameBytes)] -> [EntryWitness] -> RenderStackEvidence -> Either String ()
checkEntryWitnesses frames witnesses (RenderStackEvidence _ _ _ _ _ invocations _) = do
  let observed = [EntryWitness (invocationKind invocation) instructions (StackAddress (physical (invocationEntrySp invocation))) (invocationEntryLr invocation)
        | invocation <- invocations, let StackBoundary instructions _ = invocationStart invocation]
  unless (witnesses == observed) (Left "render invocation coverage does not match captured entry boundaries")
  forM_ invocations $ \invocation -> do
    unless (lookup (invocationKind invocation) frames == Just (invocationFrame invocation))
      (Left "render invocation frame differs from linked owned ELF")

checkLifetimeGuard :: J.Value -> Either String ()
checkLifetimeGuard = J.parseEither $ J.withObject "render lifetime evidence" $ \fields -> do
  complete <- fields J..: "whole_installed_lifetime_coverage"
  failures <- fields J..: "failures" :: J.Parser [J.Value]
  (violation,classes) <- fields J..: "watch_final_summary" >>= J.withObject "lifetime watch" (\watch -> do
    enabled <- watch J..: "enabled"
    views <- watch J..: "views" :: J.Parser T.Text
    unless (enabled && views == "fixed") (fail "render lifetime watch is not enabled on fixed backing")
    (,) <$> watch J..: "violation" <*> (watch J..: "classes" >>= mapM parseGuardClass))
  unless (complete && null failures && violation == (Nothing :: Maybe J.Value))
    (fail "render lifetime identity or attempted-write coverage refused")
  let ranges = sort [(start,end) | GuardClass start end GuardWrites <- classes]
  forM_ [(0x60614,0x60618),(0x5c3ac,0x5c3d4),(0xa4ce00,0xa4ce40)] $ \(start,end) ->
    unless (covers (PhysicalAddress start) (PhysicalAddress end) ranges)
      (fail "render lifetime watch omits descriptor, backlink or attempted bottom writes")
  forM_ ["observation_error","permit_end_error","watch_final_error"] $ \key -> do
    problem <- fields J..: key :: J.Parser (Maybe T.Text)
    unless (problem == Nothing) (fail "render completed lifetime contains an observation error")
  requested <- fields J..: "request" >>= J.withObject "modal observation request" (\request ->
    request J..: "lifetime" >>= J.withObject "modal lifetime" (\lifetime -> lifetime J..: "task33_stack_guard" >>= parseGuard))
  completed <- fields J..: "permit_end" >>= J.withObject "completed modal permit" (\permit -> do
    enabled <- permit J..: "enabled"
    physicalCoverage <- permit J..: "physical_fetch_coverage"
    failure <- permit J..: "failure" :: J.Parser (Maybe T.Text)
    unless (not enabled && physicalCoverage && failure == Nothing) (fail "render completed permit lacks physical-fetch coverage")
    permit J..: "lifetime" >>= J.withObject "completed task lifetime" (\lifetime -> do
      phase <- lifetime J..: "phase" :: J.Parser T.Text
      problem <- lifetime J..: "failure" :: J.Parser (Maybe T.Text)
      unless (phase == "ended" && problem == Nothing) (fail "render Task33 guard lifetime is incomplete or refused")
      lifetime J..: "task33_stack_guard" >>= J.withObject "completed Task33 guard" (\guard -> do
        scope <- guard J..: "scope" :: J.Parser T.Text
        priority <- guard J..: "priority" :: J.Parser Word32
        task <- guard J..: "task_id" :: J.Parser Word32
        created <- guard J..: "created" :: J.Parser Word32
        unless (scope == "diagnostic_only_bottom_guard_write_attempts" && priority == 8 && task == 33 && created == 1)
          (fail "render completed Task33 guard owner differs")
        parseGuard (J.Object guard))))
  unless (requested == completed) (fail "render completed Task33 guard differs from requested baseline")
 where
  parseGuard = J.withObject "Task33 bottom guard" $ \guard -> do
    node <- guard J..: "node" :: J.Parser Word32
    creation <- guard J..: "creation" :: J.Parser Word32
    entry <- guard J..: "entry" :: J.Parser Word32
    base <- guard J..: "base" :: J.Parser Word32
    size <- guard J..: "size" :: J.Parser Word32
    extent <- guard J..: "guard_bytes" :: J.Parser Word32
    baseline <- guard J..: "baseline" :: J.Parser [Word32]
    unless (node == 0x4006060c && creation == 0x4005c3ac && entry == 0x1109130 && base == 0xa4ce00
      && size == 4096 && extent == 64 && length baseline == 64 && all (<=255) baseline)
      (fail "render lifetime Task33 descriptor or existing bottom guard differs")
    pure (GuardBaseline (B.pack (map fromIntegral baseline)))
  parseGuardClass = J.withObject "render lifetime watch class" $ \fields -> do
    access <- fields J..: "kinds" >>= \kinds -> case (kinds :: [T.Text]) of
      ["cpu-write","host-write","model-bus-write"] -> pure GuardWrites
      ["cpu-fetch"] -> pure GuardFetches
      _ -> fail "render lifetime watch class access sources differ"
    fields J..: "range" >>= \range -> case range of
      [start,end] | start < end -> pure (GuardClass start end access)
      _ -> fail "render lifetime watch range is invalid"
  covers cursor end _ | cursor >= end = True
  covers _ _ [] = False
  covers cursor end ((start,last_):rest)
    | start > cursor = False
    | otherwise = covers (max cursor last_) end rest

checkOwnedSources :: ELF.Elf -> RenderStackEvidence -> Either String ()
checkOwnedSources elf (RenderStackEvidence _ _ _ _ sources _ _) = do
  let expected = sort [(CodeAddress (ELF.sectionAddress section),digest (B.take (fromIntegral (ELF.sectionSize section))
        (B.drop (fromIntegral (ELF.sectionOffset section)) (ELF.elfBytes elf))))
        | section <- ELF.elfSections elf, ELF.sectionExecutable section, ELF.sectionSize section > 0]
      actual = sort [(start,sha) | SourceBinding OwnedSource start _ sha <- sources]
  unless (actual == expected) (Left "render executable source identities differ from linked role ELF")
 where
  digest = SourceDigest . T.pack . concatMap hexByte . B.unpack . SHA256.hash
  hexByte byte = let text = showHex byte "" in replicate (2-length text) '0' ++ text

allocationVerdictJson :: AllocationVerdict -> J.Value
allocationVerdictJson (AllocationVerdict role count fallbacks (InvocationCount estimateFailures) minimum_) = J.object
  [ "contract" J..= ("finite-owner-allocation-r1-revised" :: T.Text)
  , "role" J..= (case role of StockStack -> "stock"; OwnedStack -> "owned" :: T.Text)
  , "allocation_verdict" J..= ("pass" :: T.Text), "floor" J..= (0xa4ce40 :: Word32)
  , "stack_base" J..= (0xa4ce00 :: Word32), "stack_size" J..= (4096 :: Word32), "existing_bottom_guard_bytes" J..= (64 :: Word32)
  , "invocations" J..= count, "fallback_prefixes" J..= fallbacks, "minimum_sp" J..= minimum_
  , "elf_plus_1024_estimate_violations" J..= estimateFailures
  , "elf_plus_1024_estimate_verdict" J..= (if estimateFailures == 0 then "not-falsified-on-observed-route" else "falsified" :: T.Text)
  , "whole_task_overflow_freedom_proven" J..= False, "static_retained_callee_bound_proven" J..= False ]
