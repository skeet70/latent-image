{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.WitnessControls (checkRefusals,checkLoaderPairRefusals) where

import Control.Monad (foldM, forM, forM_)
import qualified Data.Aeson as J
import Data.Bits (shiftL, xor)
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Word (Word8, Word32)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Digest (..))
import qualified Replacement.KernelInstall.Wire as K
import Replacement.StartupConsumers.Evidence (address, bytesWord, clock, count, number, physical, probeClock, wordBytes)
import Replacement.StartupConsumers.Wire
import Replacement.StartupConsumers.Witness
import qualified Replacement.StartupConsumers.ObservationPolicy as Policy
import qualified Replacement.StartupConsumers.Source as S

newtype ControlName = ControlName String
data Control = Control ControlName WitnessError (Receipt -> Receipt)
data CoherentCase = DescriptorTailCase | PayloadByteCase | CallerByteCase deriving (Eq, Show)
data PayloadScope = SelectedPayloadEmpty | SelectedCopyAbsent deriving (Eq, Show)
data CoherentChange = ChangedReceipt Receipt | UnobservedPayload PayloadScope

checkRefusals :: Inputs -> ObservedFiles -> ObservedFiles -> ObservedFiles -> Either String J.Value
checkRefusals inputs retained installer joined = do
  qualifiedRetained <- liftAdmission (admitReceipt inputs retained)
  qualifiedInstaller <- liftAdmission (admitReceipt inputs installer)
  qualifiedJoined <- liftAdmission (admitReceipt inputs joined)
  liftAdmission (compareReceipts qualifiedRetained qualifiedInstaller qualifiedJoined)
  comparisons <- sequence
    [expectComparison (ControlName "duplicate retained role") (compareReceipts qualifiedRetained qualifiedRetained qualifiedJoined)
    ,expectComparison (ControlName "swapped installer and joined roles") (compareReceipts qualifiedRetained qualifiedJoined qualifiedInstaller)
    ,expectComparison (ControlName "missing joined role") (compareReceipts qualifiedRetained qualifiedInstaller qualifiedInstaller)]
  receipts <- mapM (either (const (Left "control receipt decode failed")) Right . J.eitherDecodeStrict' . observedReceipt) [retained,installer,joined]
  rows <- forM (zip receipts [qualifiedRetained,qualifiedInstaller,qualifiedJoined]) $ \(receipt,qualified) ->
    checkReceiptControls compareSemanticProjections (validateReceipt inputs) [] receipt (qualifiedProjection qualified)
  forM_ [retained,installer,joined] $ \files ->
    case admitReceipt inputs (files {observedCheckpoint = observedCheckpoint files <> B.singleton 0}) of
      Left CheckpointRejected -> Right ()
      Left actual -> Left ("checkpoint mutation: expected CheckpointRejected, got " ++ show actual)
      Right _ -> Left "checkpoint mutation accepted"
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("exact-startup-consumer-refusals" :: String)
    ,"roles" J..= rows,"comparison_controls" J..= comparisons,"checkpoint_controls" J..= (3 :: Int)])
  where
    liftAdmission :: Either WitnessError value -> Either String value
    liftAdmission = either (Left . show) Right

checkLoaderPairRefusals :: Policy.ObservationPolicy -> Policy.ObservationPolicy -> Inputs -> Receipt -> Receipt -> Either String J.Value
checkLoaderPairRefusals retainedPolicy joinedPolicy inputs retained joined = do
  if receiptRole retained == Retained && receiptRole joined == Joined then pure () else Left "loader consumer control roles differ"
  before <- either (Left . show) Right (validateReceiptWithPolicy retainedPolicy inputs retained)
  after <- either (Left . show) Right (validateReceiptWithPolicy joinedPolicy inputs joined)
  either (Left . show) Right (compareLoaderSemanticProjections before after)
  retainedExtra <- policyControls retainedPolicy retained
  joinedExtra <- policyControls joinedPolicy joined
  rows <- sequence [checkReceiptControls compareLoaderSemanticProjections (validateReceiptWithPolicy retainedPolicy inputs) retainedExtra retained before
    ,checkReceiptControls compareLoaderSemanticProjections (validateReceiptWithPolicy joinedPolicy inputs) joinedExtra joined after]
  pure (J.object ["schema" J..= (1::Int),"qualification" J..= ("exact-loader-joined-consumer-refusals"::String),"roles" J..= rows])

checkReceiptControls :: (Projection -> Projection -> Either WitnessError ()) -> (Receipt -> Either WitnessError Projection) -> [Control] -> Receipt -> Projection -> Either String J.Value
checkReceiptControls compareProjection validate extra receipt baseline = do
  controls <- forM (mutations ++ extra) $ \(Control (ControlName name) expected mutate) ->
    case validate (mutate receipt) of
      Left actual | actual == expected -> Right (J.object ["control" J..= name,"rejection" J..= show expected])
      Left actual -> Left (name ++ ": expected " ++ show expected ++ ", got " ++ show actual)
      Right _ -> Left (name ++ ": mutation was accepted")
  coherent <- forM [DescriptorTailCase,PayloadByteCase,CallerByteCase] $ \which -> do
    changed <- prepareCoherent which receipt
    case changed of
      UnobservedPayload reason -> pure (J.object ["control" J..= show which,"observation" J..= ("not-observed"::String),"reason" J..= show reason])
      ChangedReceipt altered -> do
        projected <- either (\failure -> Left (show which ++ ": individual admission rejected with " ++ show failure)) Right (validate altered)
        case compareProjection baseline projected of
          Left ComparisonRejected -> pure (J.object ["control" J..= show which,"typed_receipt_admitted" J..= True,"rejection" J..= show ComparisonRejected])
          Left failure -> Left (show which ++ ": expected ComparisonRejected, got " ++ show failure)
          Right () -> Left (show which ++ ": semantic difference was accepted")
  pure (J.object ["role" J..= show (receiptRole receipt),"controls" J..= controls,"coherent_comparison_controls" J..= coherent])

expectComparison :: ControlName -> Either WitnessError () -> Either String J.Value
expectComparison (ControlName name) result = case result of
  Left ComparisonRejected -> Right (J.object ["control" J..= name,"rejection" J..= show ComparisonRejected])
  Left actual -> Left (name ++ ": expected ComparisonRejected, got " ++ show actual)
  Right () -> Left (name ++ ": role mutation was accepted")

-- Mutations of the shared observation union, selected from the policy issued for this receipt.
policyControls :: Policy.ObservationPolicy -> Receipt -> Either String [Control]
policyControls policy receipt = do
  let segments = receiptSegments receipt
      external segment = either (Left . show) Right (Policy.policyExternalBefore policy (segmentId segment))
      consumerStatic = Set.fromList (Policy.policyGlobalSites policy ++ map S.familyEntryAddress [minBound..maxBound])
  externals <- mapM external segments
  (omittedSegment,omitted) <- selectFirst "external-only armed site"
    [(segmentId segment,site) | (segment,sites) <- zip segments externals,site <- sites
      ,not (Set.member site consumerStatic),site `elem` segmentArmedSites segment,site `notElem` map stopPc (segmentStops segment)]
  (retainedSegment,released) <- selectFirst "released external site"
    [(segmentId later,site) | ((_,sites),(later,laterSites)) <- zip (zip segments externals) (drop 1 (zip segments externals))
      ,site <- sites,site `notElem` laterSites,site `notElem` segmentArmedSites later]
  let onSegment ident modify changed = changed
        {receiptSegments = [if segmentId segment == ident then modify segment else segment | segment <- receiptSegments changed]}
  pure
    [Control (ControlName "external observation site omitted") WatchRejected $ onSegment omittedSegment $ \segment -> segment
      {segmentArmedSites = filter (/= omitted) (segmentArmedSites segment)}
    ,Control (ControlName "released external site retained") WatchRejected $ onSegment retainedSegment $ \segment -> segment
      {segmentArmedSites = sort (released:segmentArmedSites segment)}
    ,Control (ControlName "shared loader carrier fetch guard omitted") WatchRejected omitLoaderCarrierFetch]
  where
    selectFirst label values = case values of
      value:_ -> Right value
      [] -> Left ("policy control cannot identify " ++ label)

omitLoaderCarrierFetch :: Receipt -> Receipt
omitLoaderCarrierFetch = mapScoped $ \segment -> segment
  {segmentFetchRanges = [range | range@(FetchRange (PhysicalBoundary low) (PhysicalBoundary high)) <- segmentFetchRanges segment,not (covers low high)]
  ,segmentWatch = (segmentWatch segment) {K.watchClasses = concatMap remove (K.watchClasses (segmentWatch segment))}}
  where
    covers low high = low <= 0x409ac && 0x409ac < high
    remove classification
      | K.CpuFetch `elem` K.classKinds classification
      , [low,high] <- K.classRange classification
      , covers low high =
          [classification {K.classKinds = remaining} | let remaining = filter (/= K.CpuFetch) (K.classKinds classification),not (null remaining)]
      | otherwise = [classification]

mutations :: [Control]
mutations =
  [Control (ControlName "producer canonical source") IdentityRejected $ \receipt -> receipt
    {receiptIdentity = (receiptIdentity receipt) {identityCanonical = Digest (replicate 64 '0')}}
  ,Control (ControlName "refused is not observation") StatusRejected $ \receipt -> receipt {receiptOutcome = Refused}
  ,Control (ControlName "bounded instruction ceiling") LimitsRejected $ \receipt -> receipt
    {receiptLimits = (receiptLimits receipt) {limitInstructions = Count 0}}
  ,Control (ControlName "source instruction substitution") SourceWordRejected $ mapProbe OuterEntry $ \probe -> probe
    {probeSourceWord = WordValue (number (probeSourceWord probe) `xor` 1)}
  ,Control (ControlName "outer dispatcher SP") FrameRejected $ mapProbe OuterEntry $ \probe -> probe
    {probeContext = (probeContext probe) {contextMachine = (contextMachine (probeContext probe)) {K.contextSp = Address 0x40063458}}}
  ,Control (ControlName "secondary accepts only observed nonzero") InitializerRejected $ mapProbe PeerPostload $ \probe -> probe
    {probeContext = (probeContext probe) {contextMachine = (contextMachine (probeContext probe)) {K.contextRegisters = replicate 13 (WordValue 0)}}}
  ,Control (ControlName "initializer return ABI") InitializerRejected $ mapProbe InitializerReturn $ changeRegister 0 0
  ,Control (ControlName "installer private return LR differs") EffectRejected $ mapProbe InstallerReturn $ \probe -> probe
    {probeContext = (probeContext probe) {contextMachine = (contextMachine (probeContext probe)) {K.contextLr = Address 0x725a84}}}
  ,Control (ControlName "first root1 target") FirstConsumerRejected $ mapProbe CriticalCall $ changeRegister 3 0xd1164
  ,Control (ControlName "watch teardown error") WatchRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentError = Just "control teardown failure"} | segment <- receiptSegments receipt]}
  ,Control (ControlName "access outside matching source classes") WatchRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentWatch = (segmentWatch segment) {K.watchRecords = map outsideSourceClasses (K.watchRecords (segmentWatch segment))}} | segment <- receiptSegments receipt]}
  ,Control (ControlName "stopped fetch counted as execution") FetchRejected $ \receipt -> receipt {receiptFetches = []}
  ,Control (ControlName "committed readback differs") EffectRejected alterFinal
  ,Control (ControlName "writer preimage differs") PreimageRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentWatch = (segmentWatch segment) {K.watchRecords = map alterPreimage (K.watchRecords (segmentWatch segment))}} | segment <- receiptSegments receipt]}
  ,Control (ControlName "reservation lock interval unclosed") LifetimeRejected $ \receipt -> receipt
    {receiptResources = [if resourceKind resource == Record && resourcePhysical resource == Address 0x9c280c
      then resource {resourceClosed = Nothing} else resource | resource <- receiptResources receipt]}
  ,Control (ControlName "zero payload claims backed admission") PayloadRejected $ \receipt -> receipt
    {receiptPayloads = receiptPayloads receipt ++ [PayloadAdmission (invocationId invocation) (probeContext probe)
      (Address 0) (Count 0) AdmittedPayload Nothing
      | invocation <- receiptInvocations receipt,invocationFamily invocation == MessageSend
      ,probe <- receiptProbes receipt,Just (probeId probe) == invocationEntry invocation]}
  ,Control (ControlName "message kernel baseline required") LifetimeRejected $ \receipt -> receipt
    {receiptResources = [if resourceKind resource == Record && resourcePhysical resource == Address 0x5988c
      then resource {resourceKind = Auxiliary} else resource | resource <- receiptResources receipt]}
  ,Control (ControlName "executed entry omitted from producer census") CensusRejected omitCensusEntry
  ,Control (ControlName "later census count fabricated") CensusRejected $ \receipt -> receipt
    {receiptCensus = [item {censusEntries = Count (count (censusEntries item)+1)} | item <- receiptCensus receipt]}
  ,Control (ControlName "root publications truncated to 1418 words") InitializerRejected truncatePublications
  ,Control (ControlName "required initializer extension store omitted") InitializerRejected $
    omitCpuAccesses (\event -> K.eventSource event == K.CpuWrite && K.eventCore event == Just K.Secondary && K.eventPc event == Just (Address 0xd08f4))
  ,Control (ControlName "acquire LDREX access omitted") AllocationRejected (omitInstructionRead (Address 0x427d4))
  ,Control (ControlName "release LDREX access omitted") AllocationRejected (omitInstructionRead (Address 0x42800))
  ,Control (ControlName "consumed startup mode read omitted") EffectRejected (omitInstructionRead (Address 0x657194))
  ,Control (ControlName "consumed startup mode differs") EffectRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentWatch = (segmentWatch segment) {K.watchRecords = map alterStartupMode (K.watchRecords (segmentWatch segment))}} | segment <- receiptSegments receipt]}
  ,Control (ControlName "receive source and call probes belong to another task frame") InvocationRejected $
    mapSelectedProbe (\probe -> probeFamily probe == Just MessageReceive && probeInvocation probe /= Nothing
      && probeKind probe `elem` [SourceProbe,DynamicTarget,DynamicReturn]) otherTaskFrame
  ,Control (ControlName "scoped pause cadence bound") LimitsRejected $ \receipt -> receipt
    {receiptLimits = (receiptLimits receipt) {limitScopedSegmentInstructions = Count 256}}
  ,Control (ControlName "scoped fetch widened to whole image4") WatchRejected expandScopedFetch
  ,Control (ControlName "mandatory message prehistory fetch word omitted") WatchRejected omitMessageFetch
  ,Control (ControlName "scoped fetch falsely declared complete") WatchRejected $ mapScoped $ \segment -> segment {segmentFetchScope = CompleteFetch}
  ,Control (ControlName "armed source sites omitted") WatchRejected $ mapScoped $ \segment -> segment {segmentArmedSites = []}
  ,Control (ControlName "SDK attribution scope omitted") WatchRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentFetchAttribution = (segmentFetchAttribution segment) {attributionRanges = []}} | segment <- receiptSegments receipt]}
  ,Control (ControlName "actual complete-fetch ordinal altered") FetchRejected alterCompleteOrdinal
  ,Control (ControlName "CPU access latest fetch omitted") FetchRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentCpuEvidence = [item {evidenceLatestFetch = Nothing} | item <- segmentCpuEvidence segment]} | segment <- receiptSegments receipt]}
  ] ++ concat
    [[Control (ControlName (show family ++ " absent")) (MissingFamily family) $ \receipt -> receipt
        {receiptInvocations = [if invocationFamily invocation == family then invocation {invocationStatus = NotObserved,invocationEntry = Nothing,invocationReturn = Nothing} else invocation | invocation <- receiptInvocations receipt]}
      ,Control (ControlName (show family ++ " unclosed")) (UnclosedFamily family) $ \receipt -> receipt
        {receiptInvocations = [if invocationFamily invocation == family then invocation {invocationStatus = Unclosed,invocationReturn = Nothing} else invocation | invocation <- receiptInvocations receipt]}
      ,Control (ControlName (show family ++ " caller register preservation")) InvocationRejected $
        mapSelectedProbe (\probe -> probeKind probe == FamilyReturn && probeFamily probe == Just family) corruptSavedRegister]
    | family <- [minBound..maxBound]]

omitCensusEntry :: Receipt -> Receipt
omitCensusEntry receipt = receipt
  {receiptProbes = retained,receiptCensus = map recount (receiptCensus receipt)
  ,receiptInvocations = [item {invocationEntry = shiftReference (invocationEntry item),invocationReturn = shiftReference (invocationReturn item)} | item <- receiptInvocations receipt]
  ,receiptSegments = [segment {segmentProbeRange = case segmentProbeRange segment of
      ProbeRange begin end -> ProbeRange (shift begin) (shift end)} | segment <- receiptSegments receipt]}
  where
    entries = filter ((==FamilyEntry) . probeKind) (receiptProbes receipt)
    undetailed = [probe | probe <- entries,Just (probeId probe) `notElem` map invocationEntry (receiptInvocations receipt)]
    omitted = map probeId (take 1 (if null undetailed then entries else undetailed))
    shift (ProbeId original) = ProbeId (original-fromIntegral (length [ident | ident <- omitted,ident < ProbeId original]))
    shiftReference reference = reference >>= \ident -> if ident `elem` omitted then Nothing else Just (shift ident)
    retained = [probe {probeId = shift (probeId probe)} | probe <- receiptProbes receipt,probeId probe `notElem` omitted]
    recount item =
      let remaining = [probe | probe <- retained,probeKind probe == FamilyEntry
            ,probeFamily probe == Just (censusFamily item),contextCore (probeContext probe) == censusCore item]
      in item {censusEntries = Count (fromIntegral (length remaining))
        ,censusFirst = case remaining of probe:_ -> Just (probeId probe); [] -> Nothing}

omitInstructionRead :: Address -> Receipt -> Receipt
omitInstructionRead instruction = omitCpuAccesses
  (\event -> K.eventSource event == K.CpuRead && K.eventPc event == Just instruction)

omitCpuAccesses :: (K.MemoryEvent -> Bool) -> Receipt -> Receipt
omitCpuAccesses omitted receipt = receipt
  {receiptSegments = map transform (receiptSegments receipt)
  ,receiptFetches = [attempt {fetchRow = shift indices (fetchRow attempt)}
    | attempt <- receiptFetches receipt,Just indices <- [M.lookup (fetchSegment attempt) removed]]}
  where
    removed = M.fromList [(segmentId segment,Set.fromList
      [Count index | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment)),omitted event])
      | segment <- receiptSegments receipt]
    shift indices (Count original) = Count (original-fromIntegral (Set.size (fst (Set.split (Count original) indices))))
    transform segment = case M.lookup (segmentId segment) removed of
      Nothing -> segment {segmentError = Just "control segment index absent"}
      Just indices -> segment
        {segmentWatch = (segmentWatch segment) {K.watchRecords =
          [event | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment)),Set.notMember (Count index) indices]}
        ,segmentCpuEvidence = [item {evidenceRow = shift indices (evidenceRow item)}
          | item <- segmentCpuEvidence segment,Set.notMember (evidenceRow item) indices]}

truncatePublications :: Receipt -> Receipt
truncatePublications receipt = receipt
  {receiptResources = [if Set.member (resourceId resource) roots then resource
    {resourceSize = Count 0x1628,resourceInitial = map clip (resourceInitial resource),resourceFinal = map clip (resourceFinal resource)}
    else resource | resource <- receiptResources receipt]
  ,receiptProbes = [probe {probeSnapshots = [if Set.member (snapshotResource snapshot) roots
    then snapshot {snapshotViews = map clip (snapshotViews snapshot)} else snapshot | snapshot <- probeSnapshots probe]}
    | probe <- receiptProbes receipt]}
  where
    roots = Set.fromList [resourceId resource | resource <- receiptResources receipt,resourceKind resource == RootObject]
    clip view = view {viewBytes = case viewBytes view of K.Bytes bytes -> K.Bytes (B.take 0x1628 bytes)}

mapScoped :: (Segment -> Segment) -> Receipt -> Receipt
mapScoped modify receipt = receipt
  {receiptSegments = [if segmentFetchScope segment == ScopedFetch then modify segment else segment | segment <- receiptSegments receipt]}

omitMessageFetch :: Receipt -> Receipt
omitMessageFetch = mapScoped $ \segment -> segment
  {segmentFetchRanges = [range | range@(FetchRange (PhysicalBoundary low) (PhysicalBoundary high)) <- segmentFetchRanges segment,not (covers low high)]
  ,segmentWatch = (segmentWatch segment) {K.watchClasses = concatMap remove (K.watchClasses (segmentWatch segment))}}
  where
    covers low high = low <= 0x4b430 && 0x4b430 < high
    remove classification
      | K.CpuFetch `elem` K.classKinds classification
      , [low,high] <- K.classRange classification
      , covers low high =
          [classification {K.classKinds = remaining} | let remaining = filter (/= K.CpuFetch) (K.classKinds classification),not (null remaining)]
      | otherwise = [classification]

expandScopedFetch :: Receipt -> Receipt
expandScopedFetch receipt = mapScoped expand receipt
  where
    PhysicalBoundary low = PhysicalBoundary 0x61c400
    high = foldr max low [fromIntegral (address (resourcePhysical resource))+count (resourceSize resource)
      | resource <- receiptResources receipt,resourceKind resource == Carrier,fromIntegral (address (resourcePhysical resource)) >= low]
    whole = FetchRange (PhysicalBoundary low) (PhysicalBoundary high)
    outside (FetchRange (PhysicalBoundary begin) (PhysicalBoundary end)) = end <= low || begin >= high
    expand segment = segment
      {segmentFetchRanges = sort (whole:filter outside (segmentFetchRanges segment))
      ,segmentWatch = (segmentWatch segment)
        {K.watchClasses = K.WatchClass [low,high] [K.CpuFetch]:K.watchClasses (segmentWatch segment)}}

alterCompleteOrdinal :: Receipt -> Receipt
alterCompleteOrdinal receipt = receipt {receiptSegments = map alter (receiptSegments receipt)}
  where
    alter segment
      | segmentFetchScope segment /= CompleteFetch = segment
      | otherwise =
          let fetched = Set.fromList [Count index | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment)),K.eventSource event == K.CpuFetch]
          in segment {segmentCpuEvidence = [if Set.member (evidenceRow item) fetched
            then item {evidenceCoreInstructions = Count (count (evidenceCoreInstructions item)+1)} else item | item <- segmentCpuEvidence segment]}

mapProbe :: ProbeKind -> (Probe -> Probe) -> Receipt -> Receipt
mapProbe kind = mapSelectedProbe ((== kind) . probeKind)

mapSelectedProbe :: (Probe -> Bool) -> (Probe -> Probe) -> Receipt -> Receipt
mapSelectedProbe selected modify receipt = receipt
  {receiptProbes = [probe {probeContext = replaceContext (probeContext probe)} | probe <- changed]
  ,receiptSegments = [segment {segmentStartContexts = map replaceContext (segmentStartContexts segment)
    ,segmentEndContexts = map replaceContext (segmentEndContexts segment)} | segment <- receiptSegments receipt]
  ,receiptResources = [resource {resourceAdmitted = replaceContext (resourceAdmitted resource)
    ,resourceClosed = fmap replaceContext (resourceClosed resource)} | resource <- receiptResources receipt]
  ,receiptPayloads = [payload {payloadContext = replaceContext (payloadContext payload)} | payload <- receiptPayloads receipt]}
  where
    changed = [if selected probe then modify probe else probe | probe <- receiptProbes receipt]
    replacements = [(probeContext before,probeContext after) | (before,after) <- zip (receiptProbes receipt) changed,selected before]
    replaceContext context = maybe context id (lookup context replacements)
changeRegister :: Int -> Word32 -> Probe -> Probe
changeRegister index value probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe)) {K.contextRegisters = [if offset == index then WordValue value else original | (offset,original) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}
otherTaskFrame :: Probe -> Probe
otherTaskFrame probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe))
    {K.contextSp = case K.contextSp (contextMachine (probeContext probe)) of Address incoming -> Address (incoming+0x1000)
    ,K.contextRegisters = [if index == (11 :: Int) then WordValue (number value+0x1000) else value
      | (index,value) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}

corruptSavedRegister :: Probe -> Probe
corruptSavedRegister probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe)) {K.contextRegisters =
    [if index == (4 :: Int) then WordValue (number value `xor` 1) else value
    | (index,value) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}

alterFinal :: Receipt -> Receipt
alterFinal receipt = case receiptResources receipt of
  resource:rest -> receipt {receiptResources = resource {resourceFinal = map alterView (resourceFinal resource)}:rest}
  [] -> receipt
  where
    alterView view = view {viewBytes = case viewBytes view of K.Bytes bytes -> K.Bytes (flipFirst bytes)}
flipFirst :: B.ByteString -> B.ByteString
flipFirst bytes = case B.uncons bytes of Just (byte,rest) -> B.cons (byte `xor` 1) rest; Nothing -> bytes
alterPreimage :: K.MemoryEvent -> K.MemoryEvent
alterPreimage event = if K.eventSource event == K.CpuWrite then
  event {K.eventBefore = fmap (\(K.Bytes bytes) -> K.Bytes (flipFirst bytes)) (K.eventBefore event)}
  else event

outsideSourceClasses :: K.MemoryEvent -> K.MemoryEvent
outsideSourceClasses event = if K.eventSource event == K.HostRead then
  event {K.eventAddress = 0x60000000,K.eventPhysical = Just 0x60000000}
  else event

alterStartupMode :: K.MemoryEvent -> K.MemoryEvent
alterStartupMode event = if K.eventSource event == K.CpuRead && K.eventPc event == Just (Address 0x657194) then
  event {K.eventValue = Just 1,K.eventBefore = Just (K.Bytes (wordBytes 1))}
  else event

data EventPosition = EventPosition SegmentId Count deriving (Eq, Ord)
data LocatedEvent = LocatedEvent {locatedPosition :: EventPosition,locatedOwn :: Count,locatedEvent :: K.MemoryEvent}
data MutationStart = InitialCell Address | WrittenCell EventPosition Address
data CopyDependency = CopyDependency EventPosition Address
data PendingCopy = PendingCopy Address Count B.ByteString
data PatchState = PatchState
  {patchActive :: M.Map Address Word8,patchHistory :: M.Map Address [(Count,Word8)]
  ,patchPending :: M.Map K.Core PendingCopy,patchRows :: M.Map EventPosition K.MemoryEvent}

selectOne :: String -> [value] -> Either String value
selectOne _ [value] = Right value
selectOne label _ = Left ("coherent control cannot identify " ++ label)

controlEntry :: Receipt -> Family -> Either String (Invocation,Probe)
controlEntry receipt family = do
  invocation <- selectOne "selected family" [item | item <- receiptInvocations receipt,invocationFamily item == family,invocationStatus item == Returned]
  probe <- selectOne "selected entry" [item | item <- receiptProbes receipt,Just (probeId item) == invocationEntry invocation]
  pure (invocation,probe)

controlRegister :: Probe -> Int -> Either String Word32
controlRegister probe index = case drop index (K.contextRegisters (contextMachine (probeContext probe))) of
  value:_ -> Right (number value)
  [] -> Left "coherent control register is missing"

prepareCoherent :: CoherentCase -> Receipt -> Either String CoherentChange
prepareCoherent which receipt = case which of
  CallerByteCase -> do
    outer <- selectOne "outer entry" [probe | probe <- receiptProbes receipt,probeKind probe == OuterEntry]
    let caller = K.contextSp (contextMachine (probeContext outer))
    ChangedReceipt <$> coherentPatch (InitialCell (Address (physical (address caller)+8))) receipt
  DescriptorTailCase -> do
    (invocation,entry) <- controlEntry receipt TaskWrapper
    descriptor <- controlRegister entry 0
    resource <- selectOne "task descriptor" [item | item <- receiptResources receipt,resourceKind item == Descriptor
      ,resourceOwner item == Just (invocationId invocation),resourcePhysical item == Address (physical descriptor),resourceSize item == Count 28]
    view <- selectOne "descriptor low view" [item | item <- resourceInitial resource,viewCore item == K.Primary,viewGuest item == viewPhysical item]
    let K.Bytes bytes = viewBytes view
        tailWord = bytesWord (B.take 4 (B.drop 24 bytes))
    if tailWord == 0xa5f0a5f0 || tailWord `xor` 1 == 0xa5f0a5f0 then
      Left "descriptor-tail control requires a non-sentinel mutation"
    else do
      events <- locatedCpuEvents receipt
      let cell = Address (physical descriptor+24)
          prior = [item | item <- events,K.eventSource (locatedEvent item) == K.CpuWrite,eventTouches cell (locatedEvent item)
            ,Just at <- [K.eventInstructions (locatedEvent item)],count at <= probeClock entry]
          start = case reverse prior of item:_ -> WrittenCell (locatedPosition item) cell; [] -> InitialCell cell
      ChangedReceipt <$> coherentPatchWithEvents start receipt events
  PayloadByteCase -> do
    (invocation,_) <- controlEntry receipt MessageSend
    let copies = [probe | probe <- receiptProbes receipt,probeInvocation probe == Just (invocationId invocation)
          ,probeKind probe == SourceProbe,K.contextPc (contextMachine (probeContext probe)) == Address 0x72517c]
    case copies of
      [] -> pure (UnobservedPayload (if any (\admission -> payloadOwner admission == invocationId invocation
        && payloadState admission == EmptyPayload) (receiptPayloads receipt) then SelectedPayloadEmpty else SelectedCopyAbsent))
      [copied] -> do
        size <- controlRegister copied 2
        if size == 0 then pure (UnobservedPayload SelectedPayloadEmpty) else do
          source <- controlRegister copied 1
          ended <- selectOne "selected copy return" [probe | probe <- receiptProbes receipt
            ,probeInvocation probe == Just (invocationId invocation),probeKind probe == SourceProbe
            ,K.contextPc (contextMachine (probeContext probe)) == Address 0x725180]
          events <- locatedCpuEvents receipt
          let sourceCell = Address (physical source)
          readEvent <- selectOne "selected payload read" [item | item <- events
            ,let event = locatedEvent item,K.eventSource event == K.CpuRead,eventTouches sourceCell event
            ,K.eventCore event == Just (invocationCore invocation),Just at <- [K.eventInstructions event]
            ,count at > probeClock copied,count at <= probeClock ended,copyLoadFor event /= Nothing]
          links <- copyDependencies events
          start <- payloadOrigin receipt events links sourceCell (locatedPosition readEvent)
          ChangedReceipt <$> coherentPatchWithEvents start receipt events
      _ -> Left "coherent control has multiple selected copies"

locatedCpuEvents :: Receipt -> Either String [LocatedEvent]
locatedCpuEvents receipt = concat <$> mapM collect (receiptSegments receipt)
  where
    collect segment = do
      let metadata = M.fromList [(evidenceRow item,evidenceCoreInstructions item) | item <- segmentCpuEvidence segment]
      forM [(Count index,event) | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment))
        ,K.eventSource event `elem` [K.CpuRead,K.CpuWrite]] $ \(index,event) -> do
          own <- maybe (Left "coherent control CPU ordinal is missing") Right (M.lookup index metadata)
          pure (LocatedEvent (EventPosition (segmentId segment) index) own event)

eventTouches :: Address -> K.MemoryEvent -> Bool
eventTouches cell event = case K.eventPhysical event of
  Just low -> low <= fromIntegral (address cell) && fromIntegral (address cell) < low+K.eventSize event
  Nothing -> False

copyAccesses :: [S.CopyAccess]
copyAccesses = map S.copyAccess [minBound..maxBound]

copyLoadFor :: K.MemoryEvent -> Maybe Address
copyLoadFor event = case [S.copyStore access | access <- copyAccesses,K.eventPc event == Just (S.copyLoad access)
  ,let S.ByteCount width = S.copyWidth access,K.eventSize event == fromIntegral width] of
  [destination] -> Just destination
  _ -> Nothing

isCopyStore :: K.MemoryEvent -> Bool
isCopyStore event = any (\access -> K.eventPc event == Just (S.copyStore access)
  && case S.copyWidth access of S.ByteCount width -> K.eventSize event == fromIntegral width) copyAccesses

copyDependencies :: [LocatedEvent] -> Either String (M.Map EventPosition CopyDependency)
copyDependencies events = snd <$> foldM step (M.empty,M.empty) events
  where
    step (pending,links) item = do
      let event = locatedEvent item
      owner <- maybe (Left "coherent copy core is missing") Right (K.eventCore event)
      case (K.eventSource event,copyLoadFor event) of
        (K.CpuRead,Just destination) -> pure (M.insert owner (destination,item) pending,links)
        (K.CpuWrite,_) | isCopyStore event -> case M.lookup owner pending of
          Just (destination,readItem)
            | K.eventPc event == Just destination && count (locatedOwn item) == count (locatedOwn readItem)+1
            && K.eventSize event == K.eventSize (locatedEvent readItem)
            && K.eventValue event == K.eventValue (locatedEvent readItem) -> do
                source <- maybe (Left "coherent copy source is missing") (Right . Address . fromIntegral) (K.eventPhysical (locatedEvent readItem))
                pure (M.delete owner pending,M.insert (locatedPosition item) (CopyDependency (locatedPosition readItem) source) links)
          _ -> pure (M.delete owner pending,links)
        _ -> pure (pending,links)

payloadOrigin :: Receipt -> [LocatedEvent] -> M.Map EventPosition CopyDependency -> Address -> EventPosition -> Either String MutationStart
payloadOrigin receipt events links cell before = do
  written <- case reverse [item | item <- takeWhile ((/=before) . locatedPosition) events,K.eventSource (locatedEvent item) == K.CpuWrite,eventTouches cell (locatedEvent item)] of
    item:_ -> Right item
    [] -> Left "selected payload has no observed writable origin"
  let event = locatedEvent written
  if isCopyStore event then do
    CopyDependency readPosition source <- maybe (Left "payload origin has no observed copy dependence") Right (M.lookup (locatedPosition written) links)
    low <- maybe (Left "payload copy destination is missing") Right (K.eventPhysical event)
    payloadOrigin receipt events links (Address (address source+address cell-fromIntegral low)) readPosition
  else do
    outer <- selectOne "outer return" [probe | probe <- receiptProbes receipt,probeKind probe == OuterReturn]
    at <- maybe (Left "payload origin clock is missing") Right (K.eventInstructions event)
    if count at <= probeClock outer then Left "payload origin precedes the finite outer boundary"
      else pure (WrittenCell (locatedPosition written) cell)

coherentPatch :: MutationStart -> Receipt -> Either String Receipt
coherentPatch start receipt = locatedCpuEvents receipt >>= coherentPatchWithEvents start receipt

coherentPatchWithEvents :: MutationStart -> Receipt -> [LocatedEvent] -> Either String Receipt
coherentPatchWithEvents start receipt events = do
  let initial = case start of
        InitialCell cell -> PatchState (M.singleton cell 1) (M.singleton cell [(Count 0,1)]) M.empty M.empty
        WrittenCell _ _ -> PatchState M.empty M.empty M.empty M.empty
  finished <- foldM (patchEvent start) initial events
  if not (M.null (patchPending finished)) then Left "coherent payload copy has no observed store" else do
    resources <- mapM (patchResource (patchHistory finished)) (receiptResources receipt)
    pure receipt
      {receiptResources = resources
      ,receiptSegments = [segment {segmentWatch = (segmentWatch segment) {K.watchRecords =
          [M.findWithDefault event (EventPosition (segmentId segment) (Count index)) (patchRows finished)
          | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment))]}} | segment <- receiptSegments receipt]
      ,receiptProbes = [probe {probeSnapshots = [snapshot {snapshotViews = map (patchView (patchHistory finished) (Count (probeClock probe))) (snapshotViews snapshot)}
        | snapshot <- probeSnapshots probe]} | probe <- receiptProbes receipt]}

patchEvent :: MutationStart -> PatchState -> LocatedEvent -> Either String PatchState
patchEvent start state item = do
  let event = locatedEvent item
  low <- maybe (Left "coherent event physical address is missing") Right (K.eventPhysical event)
  owner <- maybe (Left "coherent event core is missing") Right (K.eventCore event)
  at <- maybe (Left "coherent event clock is missing") Right (K.eventInstructions event)
  let seeded = case start of WrittenCell position _ -> position == locatedPosition item; InitialCell _ -> False
      touched = case M.lookupGE (Address (fromIntegral low)) (patchActive state) of
        Just (cell,_) -> fromIntegral (address cell) < low+K.eventSize event
        Nothing -> False
  if not seeded && not touched && not (M.member owner (patchPending state)) then pure state else do
    let cells = [Address (fromIntegral (low+offset)) | offset <- [0..K.eventSize event-1]]
        oldMasks = B.pack [M.findWithDefault 0 cell (patchActive state) | cell <- cells]
        patchValue masks value = foldl' (\word (index,mask) -> word `xor` (fromIntegral mask `shiftL` (8*index))) value (zip [0..] (B.unpack masks))
        valueWith masks = if B.any (/=0) masks then fmap (patchValue masks) (K.eventValue event) else K.eventValue event
        changedBefore = if B.any (/=0) oldMasks then event
          {K.eventBefore = fmap (\(K.Bytes bytes) -> K.Bytes (B.pack (B.zipWith xor bytes oldMasks))) (K.eventBefore event)}
          else event
        remember current changed = if changed == event then current else current {patchRows = M.insert (locatedPosition item) changed (patchRows current)}
    case K.eventSource event of
      K.CpuRead -> do
        pending <- case copyLoadFor event of
          Nothing -> pure (patchPending state)
          Just destination
            | M.member owner (patchPending state) -> Left "coherent copy dependence is interrupted"
            | B.any (/=0) oldMasks -> pure (M.insert owner (PendingCopy destination (locatedOwn item) oldMasks) (patchPending state))
            | otherwise -> pure (patchPending state)
        pure (remember (state {patchPending = pending}) (changedBefore {K.eventValue = valueWith oldMasks}))
      K.CpuWrite -> do
        let zeroMasks = B.replicate (fromIntegral (K.eventSize event)) 0
        (pending,masks) <- case start of
          WrittenCell position cell | position == locatedPosition item ->
            pure (patchPending state,B.pack [if here == cell then 1 else 0 | here <- cells])
          _ | isCopyStore event -> case M.lookup owner (patchPending state) of
            Just (PendingCopy destination ordinal copied)
              | K.eventPc event == Just destination && count (locatedOwn item) == count ordinal+1
              && B.length copied == fromIntegral (K.eventSize event) -> pure (M.delete owner (patchPending state),copied)
            Just _ -> Left "coherent copy store does not follow its actual load"
            Nothing -> pure (patchPending state,zeroMasks)
          _ -> pure (patchPending state,zeroMasks)
        let changed = changedBefore {K.eventValue = valueWith masks}
            updated = foldl' (recordMask at) (state {patchPending = pending}) (zip cells (B.unpack masks))
        pure (remember updated changed)
      _ -> Left "coherent patch received a non-CPU-data event"

recordMask :: Count -> PatchState -> (Address,Word8) -> PatchState
recordMask at state (cell,mask)
  | M.findWithDefault 0 cell (patchActive state) == mask = state
  | otherwise = state
      {patchActive = if mask == 0 then M.delete cell (patchActive state) else M.insert cell mask (patchActive state)
      ,patchHistory = M.insertWith (++) cell [(at,mask)] (patchHistory state)}

patchResource :: M.Map Address [(Count,Word8)] -> Resource -> Either String Resource
patchResource history resource = do
  end <- maybe (Left "coherent resource lifetime is unclosed") Right (resourceClosed resource)
  pure resource {resourceInitial = map (patchView history (Count (clock (resourceAdmitted resource)))) (resourceInitial resource)
    ,resourceFinal = map (patchView history (Count (clock end))) (resourceFinal resource)}

patchView :: M.Map Address [(Count,Word8)] -> Count -> View -> View
patchView history at view = view {viewBytes = K.Bytes (applyChanges bytes edits)}
  where
    K.Bytes bytes = viewBytes view
    low = address (viewPhysical view)
    edits = [(fromIntegral (address cell-low),mask) | (cell,changes) <- M.toAscList history
      ,address cell >= low,toInteger (address cell) < toInteger low+toInteger (B.length bytes)
      ,(_,mask):_ <- [dropWhile ((>at) . fst) changes],mask /= 0]
    applyChanges original [] = original
    applyChanges original changes = B.concat (walk 0 changes)
      where
        walk offset [] = [B.drop offset original]
        walk offset ((index,mask):rest) = B.take (index-offset) (B.drop offset original)
          : B.singleton (B.index original index `xor` mask) : walk (index+1) rest
