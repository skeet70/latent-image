module Replacement.StartupConsumers.Evidence where

import Control.Monad (foldM, forM, forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (sort, sortOn, nub)
import qualified Data.Map.Strict as M
import Data.Ord (Down (..))
import qualified Data.Set as Set
import Data.Word (Word8, Word32, Word64)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Bytes (..), Core (..), MemoryEvent (..), Watch (..), WatchClass (..), Source (..), ByteOrder (..))
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.Source as S
import qualified Replacement.StartupConsumers.ObservationPolicy as Policy
import Replacement.StartupConsumers.Wire

data WitnessError
  = SourceRejected | ArtifactRejected | ContractRejected | IdentityRejected | DecodeRejected | StatusRejected
  | CheckpointRejected | LimitsRejected | ContextRejected | ProbeRejected | SourceWordRejected | SegmentRejected
  | WatchRejected | EventRejected | EventOrderRejected | FetchRejected | ViewsRejected | AdmissionRejected
  | PreimageRejected | EffectRejected | LifetimeRejected | FrameRejected | CompletionRejected | InitializerRejected
  | FirstConsumerRejected | WriterRejected | CensusRejected | MissingFamily Family | UnclosedFamily Family
  | InvocationRejected | CallRejected | AllocationRejected | TaskRejected | ConstructRejected | SendRejected
  | ReceiveRejected | PayloadRejected | ComparisonRejected
  deriving (Eq, Show)

require :: Bool -> WitnessError -> Either WitnessError ()
require predicate failure = unless predicate (Left failure)
required :: WitnessError -> Maybe a -> Either WitnessError a
required failure = maybe (Left failure) Right
unique :: Eq a => [a] -> Bool
unique values = length values == length (nub values)
one :: WitnessError -> [a] -> Either WitnessError a
one _ [value] = Right value
one failure _ = Left failure
address :: Address -> Word32
address (Address value) = value
number :: WordValue -> Word32
number (WordValue value) = value
count :: Count -> Word64
count (Count value) = value
machine :: Probe -> K.Context
machine = contextMachine . probeContext
pc :: Probe -> Word32
pc = address . K.contextPc . machine
sp :: Probe -> Word32
sp = address . K.contextSp . machine
lr :: Probe -> Word32
lr = address . K.contextLr . machine
core :: Probe -> Core
core = contextCore . probeContext
data ExecutionMode = UserMode | FiqMode | IrqMode | SupervisorMode | MonitorMode | AbortMode | HypervisorMode | UndefinedMode | SystemMode deriving (Eq)
data ExecutionFrame = ExecutionFrame Core ExecutionMode K.Address K.Address deriving (Eq)

executionFrame :: Probe -> Either WitnessError ExecutionFrame
executionFrame probe = do
  mode <- case number (K.contextCpsr (machine probe)) .&. 0x1f of
    0x10 -> pure UserMode
    0x11 -> pure FiqMode
    0x12 -> pure IrqMode
    0x13 -> pure SupervisorMode
    0x16 -> pure MonitorMode
    0x17 -> pure AbortMode
    0x1a -> pure HypervisorMode
    0x1b -> pure UndefinedMode
    0x1f -> pure SystemMode
    _ -> Left InvocationRejected
  fp <- K.Address <$> register probe 11
  pure (ExecutionFrame (core probe) mode fp (K.contextSp (machine probe)))

register :: Probe -> Int -> Either WitnessError Word32
register probe index = required ContextRejected (lookup index (zip [0..] (map number (K.contextRegisters (machine probe)))))
clock :: Context -> Word64
clock = sum . map count . K.contextCounts . contextMachine
probeClock :: Probe -> Word64
probeClock = clock . probeContext
physical :: Word32 -> Word32
physical value = if value >= 0x40000000 && value < 0x60000000 then value .&. 0x3fffffff else value
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` amount) | amount <- [0,8,16,24]]
bytesWord :: B.ByteString -> Word32
bytesWord bytes = foldl (\value (offset,byte) -> value + (fromIntegral byte `shiftL` (8*offset))) 0 (zip [0..] (B.unpack bytes))

data Memory = Memory [(Word32,B.ByteString)] (M.Map Word32 Word8)
viewMemory :: [View] -> Memory
viewMemory views = Memory [(address (viewPhysical view),bytes)
  | view <- views,viewCore view == Primary,viewGuest view == viewPhysical view,let Bytes bytes = viewBytes view] M.empty
memoryByte :: Memory -> Word32 -> Maybe Word8
memoryByte (Memory spans changes) location = case M.lookup location changes of
  Just value -> Just value
  Nothing -> case [B.index bytes (fromIntegral (location-base)) | (base,bytes) <- spans,location >= base
    ,toInteger location < toInteger base+toInteger (B.length bytes)] of
      value:_ -> Just value
      [] -> Nothing
writeMemory :: Memory -> Word32 -> B.ByteString -> Memory
writeMemory (Memory spans changes) start bytes = Memory spans
  (foldl' (\contents (offset,byte) -> M.insert (start+offset) byte contents) changes (zip [0..] (B.unpack bytes)))
bytesAt :: WitnessError -> Memory -> Word32 -> Word64 -> Either WitnessError B.ByteString
bytesAt failure memory@(Memory spans changes) start size = do
  require (toInteger start + toInteger size <= 0x100000000 && size <= 0x400000) failure
  let unchanged = case M.lookupGE start changes of Just (location,_) -> toInteger location >= toInteger start+toInteger size; Nothing -> True
      slices = [B.take (fromIntegral size) (B.drop (fromIntegral (start-base)) bytes) | (base,bytes) <- spans,start >= base
        ,toInteger start+toInteger size <= toInteger base+toInteger (B.length bytes)]
  case (size,unchanged,slices) of
    (0,_,_) -> pure B.empty
    (_,True,value:_) -> pure value
    _ -> B.pack <$> mapM (\offset -> required failure (memoryByte memory (start+fromIntegral offset))) [0..fromIntegral size-1 :: Int]
resourceBytes :: Resource -> Memory -> Either WitnessError B.ByteString
resourceBytes resource memory = bytesAt ViewsRejected memory (address (resourcePhysical resource)) (count (resourceSize resource))
wordAt :: WitnessError -> Memory -> Word32 -> Either WitnessError Word32
wordAt failure memory guest = bytesWord <$> bytesAt failure memory (physical guest) 4

checkContext :: Context -> Either WitnessError ()
checkContext context = do
  let state = contextMachine context
  require (length (K.contextRegisters state) == 13 && length (K.contextCounts state) == 2
    && length (K.contextTcm state) == 2 && all ((==2) . length) (K.contextTcm state)
    && K.contextIdentities state == map WordValue [0x4003,0x5003]
    && all ((<= Count 0x20000000)) (K.contextCounts state) && sum (map count (K.contextCounts state)) <= 0x20000000
    && number (K.contextCpsr state) .&. 0x01000020 == 0) ContextRejected

allEvents :: Receipt -> [MemoryEvent]
allEvents = concatMap (watchRecords . segmentWatch) . receiptSegments

data ExecutedFetch = ExecutedFetch {executedRow :: MemoryEvent,executedCoreBefore :: Count}

actualStoppedRows :: Segment -> Either WitnessError (Set.Set Word64)
actualStoppedRows segment = do
  require (unique (map stopCore (segmentStops segment))) FetchRejected
  rows <- forM (segmentStops segment) $ \stopped -> do
    require (stopInstructions stopped == Count (sum (map count (segmentEnd segment)))
      && any (\context -> contextCore context == stopCore stopped
        && K.contextPc (contextMachine context) == stopPc stopped) (segmentEndContexts segment)) FetchRejected
    required FetchRejected (case reverse [index | (index,event) <- zip [0 :: Word64 ..] (watchRecords (segmentWatch segment))
      ,eventSource event == CpuFetch,eventCore event == Just (stopCore stopped)
      ,eventPc event == Just (stopPc stopped),eventInstructions event == Just (stopInstructions stopped)] of
        index:_ -> Just index
        [] -> Nothing)
  pure (Set.fromList rows)

executedFetchesAt :: [Address] -> Receipt -> Either WitnessError [ExecutedFetch]
executedFetchesAt targets receipt = concat <$> mapM collect (receiptSegments receipt)
  where
    collect segment = do
      stopped <- actualStoppedRows segment
      let evidence = M.fromList [(evidenceRow item,item) | item <- segmentCpuEvidence segment]
      forM [(index,event) | (index,event) <- zip [0 :: Word64 ..] (watchRecords (segmentWatch segment))
        ,eventSource event == CpuFetch,eventPc event `elem` map Just targets,Set.notMember index stopped] $ \(index,event) -> do
          item <- required FetchRejected (M.lookup (Count index) evidence)
          let own = count (evidenceCoreInstructions item)
          require (own > 0) FetchRejected
          pure (ExecutedFetch event (Count (own-1)))

mergeFetchRanges :: [FetchRange] -> [FetchRange]
mergeFetchRanges = reverse . foldl' merge [] . sort
  where
    merge [] next = [next]
    merge (previous:rest) next
      | fetchRangeLow next <= fetchRangeHigh previous =
          FetchRange (fetchRangeLow previous) (max (fetchRangeHigh previous) (fetchRangeHigh next)):rest
      | otherwise = next:previous:rest

codeFetchRange :: Address -> S.ByteCount -> FetchRange
codeFetchRange guest (S.ByteCount size) =
  let start = fromIntegral (physical (address guest))
  in FetchRange (PhysicalBoundary start) (PhysicalBoundary (start+fromIntegral size))

-- | Boot-ROM backing of the model a receipt was recorded on: the catalog's 16 KiB region, or the
-- X-M1 decode repeated through 0x80104000 (E-BOOT-ROM-DECODE-150).
data RomBacking = CatalogRom | RepeatedRom deriving (Bounded, Enum, Eq, Show)

backedFetchRanges :: RomBacking -> [FetchRange]
backedFetchRanges rom = [FetchRange (PhysicalBoundary low) (PhysicalBoundary high) | (low,high) <- backedRanges rom]

-- | The backing whose ranges the receipt's first segment attributes fetches over.
receiptRomBacking :: Receipt -> Either WitnessError RomBacking
receiptRomBacking receipt = case receiptSegments receipt of
  segment:_ -> one WatchRejected [rom | rom <- [minBound..maxBound], attributionRanges (segmentFetchAttribution segment) == backedFetchRanges rom]
  [] -> Left WatchRejected

rangeContains :: [FetchRange] -> Address -> S.ByteCount -> Bool
rangeContains ranges low (S.ByteCount size) = any covers ranges
  where
    start = fromIntegral (address low)
    covers (FetchRange (PhysicalBoundary begin) (PhysicalBoundary end)) = begin <= start && start+fromIntegral size <= end

allFetchedEvents :: Receipt -> [MemoryEvent]
allFetchedEvents receipt = [event | event <- allEvents receipt,eventSource event == CpuFetch]
  ++ [attributedFetchEvent fetched | segment <- receiptSegments receipt,item <- segmentCpuEvidence segment
      ,Just fetched <- [evidenceLatestFetch item]]

isWrite :: MemoryEvent -> Bool
isWrite event = eventSource event `elem` [CpuWrite,HostWrite,ModelBusWrite]
isRead :: MemoryEvent -> Bool
isRead event = eventSource event `elem` [CpuRead,HostRead,ModelBusRead]
eventClock :: MemoryEvent -> Either WitnessError Word64
eventClock event = count <$> required EventRejected (eventInstructions event)
eventPhysical32 :: MemoryEvent -> Either WitnessError Word32
eventPhysical32 event = do
  backing <- required EventRejected (eventPhysical event)
  require (backing < 0x100000000 && eventAddress event < 0x100000000 && eventSize event > 0
    && eventSize event <= 0x20000000 && backing + eventSize event <= 0x100000000) EventRejected
  require (physical (fromIntegral (eventAddress event)) == fromIntegral backing) EventRejected
  pure (fromIntegral backing)
eventBytes :: MemoryEvent -> Either WitnessError B.ByteString
eventBytes event = do
  value <- required EventRejected (eventValue event)
  require (eventByteOrder event == Just Little && eventSize event > 0 && eventSize event <= 8) EventRejected
  require (eventSize event == 8 || value < 1 `shiftL` fromIntegral (8*eventSize event)) EventRejected
  pure (B.pack [fromIntegral (value `shiftR` fromIntegral (8*offset)) | offset <- [0..eventSize event-1]])

checkViews :: Resource -> [View] -> Either WitnessError Memory
checkViews resource views = do
  let base = address (resourcePhysical resource)
      size = count (resourceSize resource)
      actual = [(viewCore view,address (viewGuest view),address (viewPhysical view),fromIntegral (B.length bytes)) | view <- views, let Bytes bytes = viewBytes view]
  require (base < 0x20000000 && size > 0 && size <= 0x400000 && fromIntegral base + size <= 0x20000000
    && sort actual == sort [(which,base+alias,base,size) | which <- [Primary,Secondary],alias <- [0,0x40000000]]) ViewsRejected
  let contents = viewMemory views
  expected <- bytesAt ViewsRejected contents base size
  require (all ((== Bytes expected) . viewBytes) views) ViewsRejected
  pure contents

resourceEnd :: Receipt -> Resource -> Either WitnessError Context
resourceEnd _ resource = required LifetimeRejected (resourceClosed resource)

checkResourceCoverage :: WitnessError -> Receipt -> Resource -> Either WitnessError ()
checkResourceCoverage failure receipt resource = do
  end <- resourceEnd receipt resource
  let intervals = [segment | segment <- receiptSegments receipt
        ,sum (map count (segmentEnd segment)) > clock (resourceAdmitted resource)
        ,sum (map count (segmentStart segment)) < clock end]
      low = fromIntegral (address (resourcePhysical resource)); high = low+count (resourceSize resource)
      covered watch kind = any (\classification -> kind `elem` classKinds classification
        && case classRange classification of [a,b] -> a <= low && b >= high; _ -> False) (watchClasses watch)
  require (not (null intervals) && all (\segment -> all (covered (segmentWatch segment))
    (if resourceKind resource == Carrier then [CpuWrite,HostWrite,ModelBusWrite]
      else [CpuRead,CpuWrite,HostRead,HostWrite,ModelBusRead,ModelBusWrite])) intervals) failure

resourceEvents :: Receipt -> Resource -> Either WitnessError [MemoryEvent]
resourceEvents receipt resource = do
  end <- resourceEnd receipt resource
  let beginClock = clock (resourceAdmitted resource); endClock = clock end
      start = fromIntegral (address (resourcePhysical resource)); finish = start + count (resourceSize resource)
  reverse <$> foldM (\found event -> do
    backing <- eventPhysical32 event
    let low = fromIntegral backing; high = low + eventSize event
    if low >= finish || high <= start then pure found else case eventSource event of
      CpuRead -> select endClock found event
      CpuWrite -> select endClock found event
      HostRead -> pure found
      ModelBusRead -> pure found
      _ -> Left WriterRejected) [] [event | segment <- receiptSegments receipt
        ,sum (map count (segmentEnd segment)) >= beginClock,sum (map count (segmentStart segment)) <= endClock
        ,event <- watchRecords (segmentWatch segment),eventSource event /= CpuFetch]
  where
    select endClock found event = do
      at <- eventClock event
      pure (if at > clock (resourceAdmitted resource) && at <= endClock then event:found else found)

replayResource :: Receipt -> Resource -> Word64 -> Either WitnessError Memory
replayResource receipt resource untilClock = do
  initial <- checkViews resource (resourceInitial resource)
  events <- resourceEvents receipt resource
  foldM advance initial events
  where
    advance memory event = do
      at <- eventClock event
      if at > untilClock then pure memory else do
        backing <- eventPhysical32 event
        let start = address (resourcePhysical resource); size = count (resourceSize resource)
            low = max backing start
            high = min (fromIntegral backing+eventSize event) (fromIntegral start+size)
            clippedSize = high-fromIntegral low
            clip bytes = B.take (fromIntegral clippedSize) (B.drop (fromIntegral (low-backing)) bytes)
        if backing >= start && fromIntegral backing+eventSize event <= fromIntegral start+size then pure () else
          require (all (\location -> any (\item -> clock (resourceAdmitted item) < at
            && maybe False ((>=at) . clock) (resourceClosed item)
            && location >= address (resourcePhysical item)
            && fromIntegral location < fromIntegral (address (resourcePhysical item))+count (resourceSize item)) (receiptResources receipt))
            [backing..backing+fromIntegral (eventSize event)-1]) AdmissionRejected
        before <- bytesAt PreimageRejected memory low clippedSize
        Bytes rawBefore <- required PreimageRejected (eventBefore event)
        require (fromIntegral (B.length rawBefore) == eventSize event && clip rawBefore == before) PreimageRejected
        value <- clip <$> eventBytes event
        if isWrite event then pure (writeMemory memory low value) else do
          require (isRead event && value == before) EffectRejected
          pure memory

memoryAt :: Receipt -> Probe -> Either WitnessError Memory
memoryAt receipt probe = do
  active <- mapM (\resource -> resourceEnd receipt resource >>= \end -> pure (resource,clock end)) (receiptResources receipt)
  let resources = sortOn (Down . clock . resourceAdmitted) [resource | (resource,end) <- active
        ,clock (resourceAdmitted resource) <= probeClock probe,end >= probeClock probe]
      initial = Memory [(address (resourcePhysical resource),bytes) | resource <- resources
        ,view <- resourceInitial resource,viewCore view == Primary,viewGuest view == viewPhysical view,let Bytes bytes = viewBytes view] M.empty
  foldM (advance resources) initial [event | event <- allEvents receipt,eventSource event == CpuWrite]
  where
    advance resources memory event = do
      at <- eventClock event
      if at > probeClock probe then pure memory else do
        backing <- eventPhysical32 event
        value <- eventBytes event
        pure (foldl' (\contents (offset,byte) ->
          let location = backing+offset
              admitted = [clock (resourceAdmitted resource) | resource <- resources,location >= address (resourcePhysical resource)
                ,fromIntegral location < fromIntegral (address (resourcePhysical resource))+count (resourceSize resource)]
          in case admitted of start:_ | at > start -> writeMemory contents location (B.singleton byte); _ -> contents)
          memory (zip [0..] (B.unpack value)))

probeAt :: Receipt -> ProbeKind -> Either WitnessError Probe
probeAt receipt kind = one ProbeRejected [probe | probe <- receiptProbes receipt, probeKind probe == kind]
probeById :: Receipt -> ProbeId -> Either WitnessError Probe
probeById receipt ident = one ProbeRejected [probe | probe <- receiptProbes receipt, probeId probe == ident]
sourceAt :: Receipt -> Invocation -> Word32 -> Either WitnessError Probe
sourceAt receipt invocation site = one ProbeRejected [probe | probe <- receiptProbes receipt, probeInvocation probe == Just (invocationId invocation),pc probe == site,probeKind probe == SourceProbe]
invocationProbes :: Receipt -> Invocation -> [Probe]
invocationProbes receipt invocation = [probe | probe <- receiptProbes receipt,probeInvocation probe == Just (invocationId invocation)]
entryOf :: Receipt -> Invocation -> Either WitnessError Probe
entryOf receipt invocation = required (MissingFamily (invocationFamily invocation)) (invocationEntry invocation) >>= probeById receipt
returnOf :: Receipt -> Invocation -> Either WitnessError Probe
returnOf receipt invocation = required (UnclosedFamily (invocationFamily invocation)) (invocationReturn invocation) >>= probeById receipt

checkResources :: Receipt -> Either WitnessError ()
checkResources receipt = do
  require (unique (map resourceId resources)) AdmissionRejected
  mapM_ checkPayload (receiptPayloads receipt)
  mapM_ check resources
  mapM_ checkOverlap resources
  mapM_ checkSnapshot (receiptProbes receipt)
  where
    resources = receiptResources receipt
    check resource = do
      checkContext (resourceAdmitted resource)
      end <- resourceEnd receipt resource
      checkContext end
      require (clock (resourceAdmitted resource) <= clock end) LifetimeRejected
      outer <- probeAt receipt OuterEntry
      require (clock (resourceAdmitted resource) >= probeClock outer) AdmissionRejected
      let decoder = K.contextTcm . contextMachine
          terminal = count (receiptInstructions receipt)
      require (decoder (resourceAdmitted resource) == decoder (probeContext outer) && decoder end == decoder (probeContext outer)) AdmissionRejected
      case resourceLifetime resource of
        ObservationLifetime -> require (clock end == terminal) LifetimeRejected
        AllocationLifetime -> require (clock end == terminal) LifetimeRejected
        InvocationLifetime -> do
          ownerId <- required LifetimeRejected (resourceOwner resource)
          owner <- one LifetimeRejected [invocation | invocation <- receiptInvocations receipt,invocationId invocation == ownerId]
          returned <- returnOf receipt owner
          require (clock end == probeClock returned) LifetimeRejected
        ReceiveReleaseLifetime -> do
          ownerId <- required LifetimeRejected (resourceOwner resource)
          owner <- one LifetimeRejected [invocation | invocation <- receiptInvocations receipt,invocationId invocation == ownerId]
          require (invocationFamily owner == MessageReceive) LifetimeRejected
          released <- sourceAt receipt owner 0x725418
          require (clock end == probeClock released) LifetimeRejected
      final <- checkViews resource (resourceFinal resource)
      replayed <- replayResource receipt resource (clock end)
      expectedBytes <- resourceBytes resource replayed
      finalBytes <- resourceBytes resource final
      require (expectedBytes == finalBytes) EffectRejected
      events <- resourceEvents receipt resource
      require (resourceKind resource /= Code || not (any isWrite events)) WriterRejected
      checkResourceCoverage WatchRejected receipt resource
      let low = fromIntegral (address (resourcePhysical resource)); high = low+count (resourceSize resource)
          tcms = concat (K.contextTcm (contextMachine (resourceAdmitted resource)))
      mapM_ (\(WordValue raw) -> if raw .&. 1 == 0 then pure () else do
        let sizeCode = (raw `shiftR` 2) .&. 31
            start = fromIntegral (raw .&. 0xfffff000) :: Word64
            finish = start + (512 `shiftL` fromIntegral sizeCode)
        require (sizeCode >= 3 && sizeCode <= 14 && all (\alias -> high+alias <= start || low+alias >= finish) [0,0x40000000]) AdmissionRejected) tcms
    checkOverlap resource = forM_ [other | other <- resources,resourceId other /= resourceId resource
      ,clock (resourceAdmitted other) < clock (resourceAdmitted resource)
        || clock (resourceAdmitted other) == clock (resourceAdmitted resource) && resourceId other < resourceId resource] $ \other -> do
      end <- resourceEnd receipt other
      let low = max (address (resourcePhysical resource)) (address (resourcePhysical other))
          high = min (fromIntegral (address (resourcePhysical resource))+count (resourceSize resource))
            (fromIntegral (address (resourcePhysical other))+count (resourceSize other))
          admitted = clock (resourceAdmitted resource)
      if fromIntegral low >= high || clock end < admitted then pure () else do
        old <- replayResource receipt other admitted
        new <- checkViews resource (resourceInitial resource)
        previous <- bytesAt AdmissionRejected old low (high-fromIntegral low)
        actual <- bytesAt AdmissionRejected new low (high-fromIntegral low)
        require (actual == previous) AdmissionRejected
    checkPayload admission = do
      checkContext (payloadContext admission)
      owner <- one PayloadRejected [invocation | invocation <- receiptInvocations receipt,invocationId invocation == payloadOwner admission]
      entry <- entryOf receipt owner
      returned <- returnOf receipt owner
      require (invocationFamily owner `elem` [MessageSend,MessageReceive] && contextCore (payloadContext admission) == invocationCore owner
        && probeClock entry <= clock (payloadContext admission) && clock (payloadContext admission) <= probeClock returned) PayloadRejected
      if count (payloadLength admission) == 0 then
        require (payloadState admission == EmptyPayload && payloadResource admission == Nothing) PayloadRejected
      else do
        require (payloadState admission == AdmittedPayload) PayloadRejected
        ident <- required PayloadRejected (payloadResource admission)
        resource <- one PayloadRejected [item | item <- resources,resourceId item == ident]
        require (resourceKind resource == Payload && resourceOwner resource == Just (payloadOwner admission)
          && physical (address (payloadPointer admission)) == address (resourcePhysical resource)
          && payloadLength admission == resourceSize resource
          && clock (resourceAdmitted resource) <= clock (payloadContext admission)) PayloadRejected
    checkSnapshot probe = do
      require (unique (map snapshotResource (probeSnapshots probe))) ViewsRejected
      mapM_ (\snapshot -> do
        resource <- one ViewsRejected [item | item <- resources,resourceId item == snapshotResource snapshot]
        end <- resourceEnd receipt resource
        require (clock (resourceAdmitted resource) <= probeClock probe && probeClock probe <= clock end) LifetimeRejected
        actual <- checkViews resource (snapshotViews snapshot)
        expected <- replayResource receipt resource (probeClock probe)
        actualBytes <- resourceBytes resource actual
        expectedBytes <- resourceBytes resource expected
        require (actualBytes == expectedBytes) EffectRejected) (probeSnapshots probe)

data SiteCallPhase = AwaitingTarget | AwaitingReturn deriving (Eq)
data SiteCall = SiteCall
  {siteCallOwner :: Maybe InvocationId,siteCallTarget :: Address,siteCallContinuation :: Address
  ,siteCallFrame :: ExecutionFrame,siteCallPhase :: SiteCallPhase} deriving (Eq)
data GlobalSiteReturn = GlobalSiteReturn Core Address ProbeKind deriving (Eq)
data SiteState = SiteState
  {siteAddresses :: Set.Set Address,siteFrames :: Set.Set InvocationId,siteCalls :: [SiteCall]
  ,siteReturns :: [GlobalSiteReturn],siteCompleted :: Set.Set ProbeKind}

scopedFetchRanges :: [Address] -> [FetchRange]
scopedFetchRanges sites = mergeFetchRanges
  ([codeFetchRange start size | S.CodeSpan start size <- S.ownedCodeSpans]
  ++ [codeFetchRange site (S.ByteCount 4) | site <- map S.familyEntryAddress [minBound..maxBound]
      ++ S.atomicSourceSites ++ map S.messageAccessInstruction S.messageAccesses ++ sites])

fetchRangesWithPolicy :: Policy.ObservationPolicy -> RomBacking -> Segment -> FetchScope -> [Address] -> [FetchRange]
fetchRangesWithPolicy policy rom segment scope sites = mergeFetchRanges
  ((if scope == CompleteFetch then backedFetchRanges rom else scopedFetchRanges sites)
    ++ Policy.policyExtraFetchRanges policy (Count (sum (map count (segmentStart segment)))))

checkDeclaredFetchPolicy :: Receipt -> Either WitnessError ()
checkDeclaredFetchPolicy = checkDeclaredFetchPolicyWithPolicy Policy.standalonePolicy

checkDeclaredFetchPolicyWithPolicy :: Policy.ObservationPolicy -> Receipt -> Either WitnessError ()
checkDeclaredFetchPolicyWithPolicy policy receipt = do
  returned <- probeAt receipt OuterReturn
  rom <- receiptRomBacking receipt
  forM_ (receiptSegments receipt) $ \segment -> do
    either (const (Left SegmentRejected)) Right (Policy.verifyPolicySegment policy segment)
    let ProbeRange begin _ = segmentProbeRange segment
        expectedScope = if begin <= probeId returned then CompleteFetch else ScopedFetch
        sites = segmentArmedSites segment
        expected = fetchRangesWithPolicy policy rom segment expectedScope sites
        actual = [FetchRange (PhysicalBoundary low) (PhysicalBoundary high) | classification <- watchClasses (segmentWatch segment)
          ,CpuFetch `elem` classKinds classification,[low,high] <- [classRange classification]]
    require (segmentFetchScope segment == expectedScope && sites == Set.toAscList (Set.fromList sites)
      && all (\site -> address site .&. 3 == 0) sites
      && segmentFetchRanges segment == expected && mergeFetchRanges actual == expected
      && attributionSources (segmentFetchAttribution segment) == [CpuRead,CpuWrite]
      && attributionRanges (segmentFetchAttribution segment) == backedFetchRanges rom) WatchRejected

checkFetchPolicies :: Receipt -> Either WitnessError ()
checkFetchPolicies = checkFetchPoliciesWithPolicy Policy.standalonePolicy

checkFetchPoliciesWithPolicy :: Policy.ObservationPolicy -> Receipt -> Either WitnessError ()
checkFetchPoliciesWithPolicy policy receipt = do
  outer <- probeAt receipt OuterEntry
  rom <- receiptRomBacking receipt
  let probes = receiptProbes receipt
      initial = SiteState (Set.fromList (Policy.policyGlobalSites policy ++ map S.familyEntryAddress [minBound..maxBound]
        ++ [K.contextLr (machine outer) | Policy.policyInitialOuterReturn policy])) Set.empty [] [] Set.empty
  require (map probeId probes == map (ProbeId . fromIntegral) [0..length probes-1]) ProbeRejected
  (consumed,final,remaining) <- foldM (checkSegment rom) (0,initial,probes) (receiptSegments receipt)
  require (consumed == fromIntegral (length probes) && null remaining) ProbeRejected
  require (Set.null (siteFrames final) && null (siteCalls final) && null (siteReturns final)) WatchRejected
  where
    invocation ident = one WatchRejected [item | item <- receiptInvocations receipt,invocationId item == ident]
    checkSegment rom (consumed,state,remaining) segment = do
      either (const (Left SegmentRejected)) Right (Policy.verifyPolicySegment policy segment)
      external <- either (const (Left WatchRejected)) Right (Policy.policyExternalBefore policy (segmentId segment))
      let ProbeRange (ProbeId begin) (ProbeId end) = segmentProbeRange segment
          expectedScope = if Set.member OuterReturn (siteCompleted state) then ScopedFetch else CompleteFetch
          sites = Set.toAscList (Set.union (siteAddresses state) (Set.fromList external))
          expected = fetchRangesWithPolicy policy rom segment expectedScope sites
          actual = [FetchRange (PhysicalBoundary low) (PhysicalBoundary high) | classification <- watchClasses (segmentWatch segment)
            ,CpuFetch `elem` classKinds classification,[low,high] <- [classRange classification]]
      require (begin == consumed && begin <= end && end <= fromIntegral (length (receiptProbes receipt))) ProbeRejected
      require (segmentFetchScope segment == expectedScope && segmentArmedSites segment == sites
        && segmentFetchRanges segment == expected && mergeFetchRanges actual == expected
        && attributionSources (segmentFetchAttribution segment) == [CpuRead,CpuWrite]
        && attributionRanges (segmentFetchAttribution segment) == backedFetchRanges rom) WatchRejected
      let (captured,rest) = splitAt (fromIntegral (end-begin)) remaining
      forM_ captured $ \probe ->
        if probeKind probe == OuterEntry && probeId probe == ProbeId 0 then
          require (K.contextCounts (machine probe) == segmentStart segment
            && any (\context -> contextCore context == core probe && contextMachine context == machine probe) (segmentStartContexts segment)) ProbeRejected
        else require (any (\stopped -> stopCore stopped == core probe && stopPc stopped == K.contextPc (machine probe)
          && stopInstructions stopped == Count (probeClock probe)) (segmentStops segment)
          && any (\context -> contextCore context == core probe && contextMachine context == machine probe) (segmentEndContexts segment)) ProbeRejected
      next <- foldM applyProbe state captured
      pure (end,next,rest)
    applyProbe state probe = do
      changed <- case probeKind probe of
        FamilyEntry -> case probeInvocation probe of
          Nothing -> pure state
          Just ident -> do
            entered <- invocation ident
            require (invocationEntry entered == Just (probeId probe)) WatchRejected
            pure state {siteFrames = Set.insert ident (siteFrames state)
              ,siteAddresses = Set.union (siteAddresses state) (Set.fromList
                (S.familySourceSites (invocationFamily entered) ++ S.commonSourceSites ++ [K.contextLr (machine probe)]))}
        FamilyReturn -> do
          ident <- required WatchRejected (probeInvocation probe)
          closed <- invocation ident
          require (Set.member ident (siteFrames state)) WatchRejected
          pure (removeSites (state {siteFrames = Set.delete ident (siteFrames state)})
            (S.familySourceSites (invocationFamily closed) ++ S.commonSourceSites))
        DynamicTarget -> do
          key <- executionFrame probe
          selected <- one WatchRejected [pending | pending <- siteCalls state,siteCallPhase pending == AwaitingTarget
            ,siteCallOwner pending == probeInvocation probe,siteCallTarget pending == K.contextPc (machine probe)
            ,siteCallContinuation pending == K.contextLr (machine probe),siteCallFrame pending == key]
          pure state {siteCalls = [if pending == selected then pending {siteCallPhase = AwaitingReturn} else pending | pending <- siteCalls state]
            ,siteAddresses = Set.insert (siteCallContinuation selected) (siteAddresses state)}
        DynamicReturn -> do
          key <- executionFrame probe
          selected <- one WatchRejected [pending | pending <- siteCalls state,siteCallPhase pending == AwaitingReturn
            ,siteCallOwner pending == probeInvocation probe,siteCallContinuation pending == K.contextPc (machine probe),siteCallFrame pending == key]
          pure (removeSites (state {siteCalls = filter (/=selected) (siteCalls state)})
            [siteCallTarget selected,siteCallContinuation selected])
        kind | kind `elem` [OuterReturn,InstallerReturn,CacheReturn,InitializerReturn] -> do
          selected <- one WatchRejected [pending | pending@(GlobalSiteReturn owner destination returned) <- siteReturns state
            ,owner == core probe,destination == K.contextPc (machine probe),returned == kind]
          pure state {siteReturns = filter (/=selected) (siteReturns state),siteCompleted = Set.insert kind (siteCompleted state)}
        kind -> case globalReturn kind state of
          Nothing -> pure state
          Just returned -> pure state {siteReturns = GlobalSiteReturn (core probe) (K.contextLr (machine probe)) returned:siteReturns state
            ,siteAddresses = Set.insert (K.contextLr (machine probe)) (siteAddresses state)}
      let withoutPoll = if probeKind probe == InitializerCall && Policy.policyRetiresPoll policy
            then changed {siteAddresses = foldr Set.delete (siteAddresses changed) (Policy.policyPollSites policy)}
            else changed
      if probeKind probe `elem` [SourceProbe,CriticalIdentityCall,CriticalCall] then do
        target <- sourceCallTarget probe
        case target of
          Nothing -> pure withoutPoll
          Just destination -> do
            key <- executionFrame probe
            let pending = SiteCall (probeInvocation probe) destination (Address (pc probe+4)) key AwaitingTarget
            pure withoutPoll {siteCalls = pending:siteCalls withoutPoll,siteAddresses = Set.insert destination (siteAddresses withoutPoll)}
      else pure withoutPoll
    globalReturn kind state = case kind of
      OuterEntry -> Just OuterReturn
      InstallerEntry -> Just InstallerReturn
      CacheEntry | Set.notMember InstallerReturn (siteCompleted state) -> Just CacheReturn
      InitializerEntry -> Just InitializerReturn
      _ -> Nothing
    removeSites state candidates = state {siteAddresses = foldr remove (siteAddresses state) candidates}
      where
        active = [item | item <- receiptInvocations receipt,Set.member (invocationId item) (siteFrames state)]
        permanent = Policy.policyGlobalSites policy ++ map S.familyEntryAddress [minBound..maxBound]
        used site = site `elem` permanent
          || any (\item -> site == invocationLr item || site `elem` S.familySourceSites (invocationFamily item) ++ S.commonSourceSites) active
          || any (\pending -> site == siteCallTarget pending || site == siteCallContinuation pending) (siteCalls state)
          || any (\(GlobalSiteReturn _ destination _) -> site == destination) (siteReturns state)
        remove site sites = if used site then sites else Set.delete site sites

sourceCallTarget :: Probe -> Either WitnessError (Maybe Address)
sourceCallTarget probe
  | instruction .&. 0x0f000000 == 0x0b000000 = do
      require (instruction `shiftR` 28 == 14) WatchRejected
      let offset = (instruction .&. 0x00ffffff) `shiftL` 2
          displacement = if offset .&. 0x02000000 /= 0 then offset .|. 0xfc000000 else offset
      aligned (pc probe+8+displacement)
  | instruction .&. 0x0ffffff0 == 0x012fff30 = do
      require (instruction `shiftR` 28 == 14) WatchRejected
      destination <- case instruction .&. 15 of
        13 -> pure (sp probe)
        14 -> pure (lr probe)
        15 -> pure (pc probe+8)
        index -> register probe (fromIntegral index)
      aligned destination
  | otherwise = pure Nothing
  where
    instruction = number (probeSourceWord probe)
    aligned destination = require (destination .&. 3 == 0) WatchRejected >> pure (Just (Address destination))

checkSegments :: Receipt -> Either WitnessError ()
checkSegments = checkSegmentsWithPolicy Policy.standalonePolicy

checkSegmentsWithPolicy :: Policy.ObservationPolicy -> Receipt -> Either WitnessError ()
checkSegmentsWithPolicy policy receipt = do
  outer <- probeAt receipt OuterEntry
  let segments = receiptSegments receipt
  initial <- required SegmentRejected (case segments of value:_ -> Just value; [] -> Nothing)
  terminalSegment <- required SegmentRejected (case reverse segments of value:_ -> Just value; [] -> Nothing)
  require (unique (map segmentId segments)
    && segmentStart initial == K.contextCounts (machine outer)
    && and (zipWith (\a b -> segmentEnd a == segmentStart b && segmentEndContexts a == segmentStartContexts b) segments (drop 1 segments))) SegmentRejected
  checkWatchFailures receipt
  checkDeclaredFetchPolicyWithPolicy policy receipt
  mapM_ (check (K.contextTcm (machine outer))) segments
  mapM_ (checkFetches receipt) segments
  let events = allEvents receipt
  clocks <- mapM eventClock [event | event <- events,eventSource event `elem` [CpuRead,CpuWrite,CpuFetch]]
  require (and (zipWith (<=) clocks (drop 1 clocks))) EventOrderRejected
  let terminal = sum (map count (segmentEnd terminalSegment))
  require (count (receiptInstructions receipt) == terminal
    && all (\attempt -> fetchSegment attempt `elem` map segmentId segments) (receiptFetches receipt)) SegmentRejected
  checkStops receipt outer
  where
    check decoder segment = do
      let start = segmentStart segment; end = segmentEnd segment; watch = segmentWatch segment
      require (length start == 2 && length end == 2 && and (zipWith (<=) start end)
        && sum (map count end)-sum (map count start) <= count (if segmentFetchScope segment == CompleteFetch
          then limitSegmentInstructions (receiptLimits receipt) else limitScopedSegmentInstructions (receiptLimits receipt))) SegmentRejected
      let startContexts = segmentStartContexts segment; endContexts = segmentEndContexts segment
      require (map contextCore startContexts == [Primary,Secondary] && map contextCore endContexts == [Primary,Secondary]
        && all ((==start) . K.contextCounts . contextMachine) startContexts
        && all ((==end) . K.contextCounts . contextMachine) endContexts) SegmentRejected
      mapM_ checkContext (startContexts ++ endContexts)
      require (all ((==decoder) . K.contextTcm . contextMachine) (startContexts ++ endContexts)) ContextRejected
      require (watchEnabled watch && watchViews watch == "fixed" && watchViolation watch == Nothing
        && segmentError segment == Nothing && not (null (watchClasses watch))
        && fromIntegral (length (watchRecords watch)) <= count (limitRecords (receiptLimits receipt))) WatchRejected
      let overlaps kind low size =
            let high = low+min size (maxBound-low)
            in any (\classification -> kind `elem` classKinds classification
              && case classRange classification of [a,b] -> low < b && a < high; _ -> False) (watchClasses watch)
      require (all (\range -> case range of [a,b] -> a < b && b <= 0x100000000; _ -> False) (watchRanges watch)
        && all (\classification -> not (null (classKinds classification)) && case classRange classification of
          [a,b] -> a < b && any (\range -> case range of [low,high] -> low <= a && high >= b; _ -> False) (watchRanges watch)
          _ -> False) (watchClasses watch)) WatchRejected
      mapM_ (\event -> do
        backing <- eventPhysical32 event
        require (overlaps (eventSource event) (fromIntegral backing) (eventSize event)) WatchRejected
        if eventSource event `elem` [CpuRead,CpuWrite,CpuFetch] then do
          at <- eventClock event
          require (at >= sum (map count start) && at <= sum (map count end) && eventSize event <= 8) EventOrderRejected
        else require (eventCore event == Nothing && eventPc event == Nothing && eventInstructions event == Nothing && eventTick event == Nothing) EventRejected) (watchRecords watch)

backedRanges :: RomBacking -> [(Word64,Word64)]
backedRanges rom = [(0,0x20000000),(0x3fff0000,0x3fff1000),(0x80000000,romEnd),(0x90000000,0x90020000),(0xfff00000,0xfff01000)]
  where romEnd = case rom of CatalogRom -> 0x80004000; RepeatedRom -> 0x80104000

checkStops :: Receipt -> Probe -> Either WitnessError ()
checkStops receipt outer = forM_ [probe | probe <- receiptProbes receipt,probeClock probe >= probeClock outer,probeKind probe /= OuterEntry] $ \probe -> do
  observed <- required ProbeRejected (M.lookup (K.contextCounts (machine probe),core probe,K.contextPc (machine probe)) stops)
  require (observed == machine probe) ProbeRejected
  where
    stops = M.fromList [((segmentEnd segment,stopCore stopped,stopPc stopped),contextMachine context)
      | segment <- receiptSegments receipt,stopped <- segmentStops segment
      ,context <- segmentEndContexts segment,contextCore context == stopCore stopped]

checkWatchFailures :: Receipt -> Either WitnessError ()
checkWatchFailures receipt = forM_ (receiptSegments receipt) $ \segment ->
  require (watchViolation (segmentWatch segment) == Nothing && segmentError segment == Nothing) WatchRejected

coreCounter :: Core -> [Count] -> Either WitnessError Count
coreCounter owner counts = case (owner,counts) of
  (Primary,[a,_]) -> pure a
  (Secondary,[_,b]) -> pure b
  _ -> Left FetchRejected

cpuRowCounts :: Segment -> MemoryEvent -> CpuRowEvidence -> Either WitnessError [Count]
cpuRowCounts segment event evidence = do
  owner <- required FetchRejected (eventCore event)
  global <- eventClock event
  let own = count (evidenceCoreInstructions evidence)
  require (global >= own) FetchRejected
  let actual = if owner == Primary then map Count [own,global-own] else map Count [global-own,own]
  require (and (zipWith (<=) (segmentStart segment) actual) && and (zipWith (<=) actual (segmentEnd segment))) FetchRejected
  pure actual

fetchKey :: MemoryEvent -> Count -> Either WitnessError (Core,Address,Count,Count,Count)
fetchKey event own = (,,,,) <$> required FetchRejected (eventCore event) <*> required FetchRejected (eventPc event)
  <*> required FetchRejected (eventInstructions event) <*> pure own <*> required FetchRejected (eventTick event)

checkCpuEvidence :: Segment -> Set.Set Word64 -> Either WitnessError (M.Map Count CpuRowEvidence)
checkCpuEvidence segment stopped = do
  let rows = watchRecords (segmentWatch segment)
      evidence = M.fromList [(evidenceRow item,item) | item <- segmentCpuEvidence segment]
      expected = [Count index | (index,event) <- zip [0 :: Word64 ..] rows,eventSource event `elem` [CpuFetch,CpuRead,CpuWrite]]
  require (M.size evidence == length (segmentCpuEvidence segment) && M.keys evidence == expected) FetchRejected
  captured <- forM [(index,event) | (index,event) <- zip [0 :: Word64 ..] rows,eventSource event == CpuFetch,Set.notMember index stopped] $ \(index,event) -> do
    item <- required FetchRejected (M.lookup (Count index) evidence)
    key <- fetchKey event (evidenceCoreInstructions item)
    pure (key,(index,event))
  let fetched = M.fromList captured
  require (M.size fetched == length captured) FetchRejected
  forM_ (zip [0 :: Word64 ..] rows) $ \(index,event) ->
    whenCpu event $ do
      item <- required FetchRejected (M.lookup (Count index) evidence)
      _ <- cpuRowCounts segment event item
      if eventSource event == CpuFetch then do
        require (evidenceLatestFetch item == Nothing) FetchRejected
        checkPoint event
      else do
        point <- required FetchRejected (evidenceLatestFetch item)
        let latest = attributedFetchEvent point
        checkPoint latest
        key <- fetchKey latest (attributedFetchCoreInstructions point)
        actual <- fetchKey event (evidenceCoreInstructions item)
        require (key == actual) FetchRejected
        backing <- eventPhysical32 latest
        require (rangeContains (attributionRanges (segmentFetchAttribution segment)) (Address backing) (S.ByteCount 4)) FetchRejected
        if rangeContains (segmentFetchRanges segment) (Address backing) (S.ByteCount 4) then do
          (before,raw) <- required FetchRejected (M.lookup key fetched)
          require (before < index && raw == latest) FetchRejected
        else pure ()
  pure evidence
  where
    whenCpu event action = if eventSource event `elem` [CpuFetch,CpuRead,CpuWrite] then action else pure ()
    checkPoint event = do
      site <- required FetchRejected (eventPc event)
      require (eventSource event == CpuFetch && eventAddress event == fromIntegral (address site)
        && eventPhysical event == Just (fromIntegral (physical (address site))) && eventSize event == 4
        && eventValue event == Nothing && eventBefore event == Nothing && eventByteOrder event == Just Little) FetchRejected

checkFetches :: Receipt -> Segment -> Either WitnessError ()
checkFetches receipt segment = do
  let attempts = [attempt | attempt <- receiptFetches receipt,fetchSegment attempt == segmentId segment]
      rows = watchRecords (segmentWatch segment)
      boundaries = Set.fromList (map (count . fetchRow) attempts)
  expectedRows <- actualStoppedRows segment
  require (expectedRows == boundaries && length attempts == Set.size boundaries
    && all ((==BoundaryAttempt) . fetchClassification) attempts) FetchRejected
  require (all (\stopped -> stopPc stopped `elem` segmentArmedSites segment) (segmentStops segment)) FetchRejected
  evidence <- checkCpuEvidence segment boundaries
  resumed <- forM (segmentStartContexts segment) $ \context -> do
    let owner = contextCore context
        site = physical (address (K.contextPc (contextMachine context)))
    begin <- coreCounter owner (segmentStart segment)
    end <- coreCounter owner (segmentEnd segment)
    pure (owner,count end > count begin && rangeContains (segmentFetchRanges segment) (Address site) (S.ByteCount 4))
  (final,pending,_,_) <- foldM (advance evidence boundaries) (segmentStart segment,M.fromList resumed,M.empty,M.empty) (zip [0 :: Word64 ..] rows)
  require (not (or (M.elems pending))
    && (segmentFetchScope segment /= CompleteFetch || final == segmentEnd segment)) FetchRejected
  where
    advance evidence boundaries state@(progress,resumes,lastFetch,lastTick) (index,event)
      | eventSource event `notElem` [CpuFetch,CpuRead,CpuWrite] = pure state
      | otherwise = do
          owner <- required FetchRejected (eventCore event)
          site <- required FetchRejected (eventPc event)
          at <- required FetchRejected (eventInstructions event)
          tick <- required FetchRejected (eventTick event)
          item <- required FetchRejected (M.lookup (Count index) evidence)
          actual <- cpuRowCounts segment event item
          previousOwn <- coreCounter owner progress
          let ordinal = evidenceCoreInstructions item
          require (and (zipWith (<=) progress actual) && maybe True (<=tick) (M.lookup owner lastTick)) EventOrderRejected
          let ticks = M.insert owner tick lastTick
          if eventSource event == CpuFetch then
            if Set.member index boundaries then do
              require (at == Count (sum (map count (segmentEnd segment))) && actual == segmentEnd segment
                && any (\context -> contextCore context == owner && K.contextPc (contextMachine context) == site
                  && K.contextCounts (contextMachine context) == actual) (segmentEndContexts segment)) FetchRejected
              pure (actual,resumes,lastFetch,ticks)
            else do
              require (count at > sum (map count progress) && ordinal > previousOwn) FetchRejected
              whenComplete $ require (count at == sum (map count progress)+1 && count ordinal == count previousOwn+1) FetchRejected
              if M.findWithDefault False owner resumes then do
                paused <- one FetchRejected [context | context <- segmentStartContexts segment,contextCore context == owner]
                begin <- coreCounter owner (segmentStart segment)
                require (site == K.contextPc (contextMachine paused) && count ordinal == count begin+1) FetchRejected
              else pure ()
              pure (actual,M.insert owner False resumes,M.insert owner (site,at) lastFetch,ticks)
          else do
            whenComplete $ require (actual == progress && M.lookup owner lastFetch == Just (site,at)) FetchRejected
            begin <- coreCounter owner (segmentStart segment)
            require (ordinal > begin && count at > sum (map count (segmentStart segment))) FetchRejected
            pure (actual,resumes,lastFetch,ticks)
    whenComplete action = if segmentFetchScope segment == CompleteFetch then action else pure ()
