module Replacement.StartupConsumers.Evidence where

import Control.Monad (foldM, forM, forM_, unless)
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (sort, sortOn, nub)
import qualified Data.Map.Strict as M
import Data.Ord (Down (..))
import qualified Data.Set as Set
import Data.Word (Word8, Word32, Word64)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Bytes (..), Core (..), MemoryEvent (..), Watch (..), WatchClass (..), Source (..), ByteOrder (..))
import qualified Replacement.KernelInstall.Wire as K
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

executedFetchesAt :: [Address] -> Receipt -> Either WitnessError [ExecutedFetch]
executedFetchesAt targets receipt = concat <$> mapM collect (receiptSegments receipt)
  where
    collect segment = do
      let rows = watchRecords (segmentWatch segment)
      require (unique (map stopCore (segmentStops segment))) FetchRejected
      stoppedRows <- forM (segmentStops segment) $ \stopped -> do
        require (stopInstructions stopped == Count (sum (map count (segmentEnd segment)))
          && any (\context -> contextCore context == stopCore stopped
            && K.contextPc (contextMachine context) == stopPc stopped) (segmentEndContexts segment)) FetchRejected
        required FetchRejected (case reverse [index | (index,event) <- zip [0 :: Word64 ..] rows
          ,eventSource event == CpuFetch,eventCore event == Just (stopCore stopped)
          ,eventPc event == Just (stopPc stopped),eventInstructions event == Just (stopInstructions stopped)] of
            index:_ -> Just index
            [] -> Nothing)
      (primary,secondary) <- case segmentStart segment of
        [a,b] -> pure (a,b)
        _ -> Left FetchRejected
      (_,_,selected) <- foldM (advance (Set.fromList stoppedRows)) (primary,secondary,[]) (zip [0 :: Word64 ..] rows)
      pure (reverse selected)
    advance stopped state@(primary,secondary,selected) (index,event)
      | eventSource event /= CpuFetch || Set.member index stopped = pure state
      | otherwise = do
          owner <- required FetchRejected (eventCore event)
          instruction <- required FetchRejected (eventPc event)
          let before = if owner == Primary then primary else secondary
              next = Count (count before+1)
              found = if instruction `elem` targets then ExecutedFetch event before:selected else selected
          pure (if owner == Primary then (next,secondary,found) else (primary,next,found))

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
      let intervals = [segment | segment <- receiptSegments receipt
            ,sum (map count (segmentEnd segment)) > clock (resourceAdmitted resource)
            ,sum (map count (segmentStart segment)) < clock end]
          low = fromIntegral (address (resourcePhysical resource)); high = low+count (resourceSize resource)
          covered watch kind = any (\classification -> kind `elem` classKinds classification && case classRange classification of [a,b] -> a <= low && b >= high; _ -> False) (watchClasses watch)
      require (not (null intervals) && all (\segment -> all (covered (segmentWatch segment))
        (if resourceKind resource == Carrier then [CpuWrite,HostWrite,ModelBusWrite] else [CpuRead,CpuWrite,HostRead,HostWrite,ModelBusRead,ModelBusWrite])) intervals) WatchRejected
      let tcms = concat (K.contextTcm (contextMachine (resourceAdmitted resource)))
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

checkSegments :: Receipt -> Either WitnessError ()
checkSegments receipt = do
  outer <- probeAt receipt OuterEntry
  let segments = receiptSegments receipt
  initial <- required SegmentRejected (case segments of value:_ -> Just value; [] -> Nothing)
  terminalSegment <- required SegmentRejected (case reverse segments of value:_ -> Just value; [] -> Nothing)
  require (unique (map segmentId segments)
    && segmentStart initial == K.contextCounts (machine outer)
    && and (zipWith (\a b -> segmentEnd a == segmentStart b && segmentEndContexts a == segmentStartContexts b) segments (drop 1 segments))) SegmentRejected
  mapM_ (check (K.contextTcm (machine outer))) segments
  let events = allEvents receipt
  clocks <- mapM eventClock [event | event <- events,eventSource event `elem` [CpuRead,CpuWrite,CpuFetch]]
  require (clocks == sort clocks) EventOrderRejected
  let terminal = sum (map count (segmentEnd terminalSegment))
  require (count (receiptInstructions receipt) == terminal
    && all (\attempt -> fetchSegment attempt `elem` map segmentId segments) (receiptFetches receipt)) SegmentRejected
  checkStops receipt outer
  where
    check decoder segment = do
      let start = segmentStart segment; end = segmentEnd segment; watch = segmentWatch segment
      require (length start == 2 && length end == 2 && and (zipWith (<=) start end)
        && sum (map count end)-sum (map count start) <= count (limitSegmentInstructions (receiptLimits receipt))) SegmentRejected
      let startContexts = segmentStartContexts segment; endContexts = segmentEndContexts segment
      require (map contextCore startContexts == [Primary,Secondary] && map contextCore endContexts == [Primary,Secondary]
        && all ((==start) . K.contextCounts . contextMachine) startContexts
        && all ((==end) . K.contextCounts . contextMachine) endContexts) SegmentRejected
      mapM_ checkContext (startContexts ++ endContexts)
      require (all ((==decoder) . K.contextTcm . contextMachine) (startContexts ++ endContexts)) ContextRejected
      require (watchEnabled watch && watchViews watch == "fixed" && watchViolation watch == Nothing
        && segmentError segment == Nothing && not (null (watchClasses watch))
        && fromIntegral (length (watchRecords watch)) <= count (limitRecords (receiptLimits receipt))) WatchRejected
      let covered kind low high = any (\classification -> kind `elem` classKinds classification
            && case classRange classification of [a,b] -> a <= low && b >= high; _ -> False) (watchClasses watch)
      require (all (\(low,high) -> covered CpuFetch low high) backedRanges
        && all (\range -> case range of [a,b] -> a < b && b <= 0x100000000; _ -> False) (watchRanges watch)
        && all (\classification -> not (null (classKinds classification)) && case classRange classification of
          [a,b] -> a < b && any (\range -> case range of [low,high] -> low <= a && high >= b; _ -> False) (watchRanges watch)
          _ -> False) (watchClasses watch)) WatchRejected
      mapM_ (\event -> do
        backing <- eventPhysical32 event
        require (covered (eventSource event) (fromIntegral backing) (fromIntegral backing+eventSize event)) WatchRejected
        if eventSource event `elem` [CpuRead,CpuWrite,CpuFetch] then do
          at <- eventClock event
          require (at >= sum (map count start) && at <= sum (map count end)+1 && eventSize event <= 8) EventOrderRejected
        else require (eventCore event == Nothing && eventPc event == Nothing && eventInstructions event == Nothing && eventTick event == Nothing) EventRejected) (watchRecords watch)
      checkFetches receipt segment

backedRanges :: [(Word64,Word64)]
backedRanges = [(0,0x20000000),(0x3fff0000,0x3fff1000),(0x80000000,0x80004000),(0x90000000,0x90020000),(0xfff00000,0xfff01000)]

checkStops :: Receipt -> Probe -> Either WitnessError ()
checkStops receipt outer = forM_ [probe | probe <- receiptProbes receipt,probeClock probe >= probeClock outer,probeKind probe /= OuterEntry] $ \probe -> do
  observed <- required ProbeRejected (M.lookup (K.contextCounts (machine probe),core probe,K.contextPc (machine probe)) stops)
  require (observed == machine probe) ProbeRejected
  where
    stops = M.fromList [((segmentEnd segment,stopCore stopped,stopPc stopped),contextMachine context)
      | segment <- receiptSegments receipt,stopped <- segmentStops segment
      ,context <- segmentEndContexts segment,contextCore context == stopCore stopped]

checkFetches :: Receipt -> Segment -> Either WitnessError ()
checkFetches receipt segment = do
  let attempts = [attempt | attempt <- receiptFetches receipt,fetchSegment attempt == segmentId segment]
      rows = watchRecords (segmentWatch segment)
      boundaries = Set.fromList (map (count . fetchRow) attempts)
      stopped = segmentStops segment
  require (unique (map stopCore stopped)) FetchRejected
  expectedRows <- forM stopped $ \point -> do
    require (stopInstructions point == Count (sum (map count (segmentEnd segment)))
      && any (\context -> contextCore context == stopCore point && K.contextPc (contextMachine context) == stopPc point) (segmentEndContexts segment)) FetchRejected
    let matching = [index | (index,event) <- zip [0..] rows,eventSource event == CpuFetch
          ,eventCore event == Just (stopCore point),eventPc event == Just (stopPc point),eventInstructions event == Just (stopInstructions point)]
    required FetchRejected (case reverse matching of index:_ -> Just index; [] -> Nothing)
  require (sort expectedRows == sort (map (count . fetchRow) attempts)) FetchRejected
  require (unique (map fetchRow attempts) && all ((==BoundaryAttempt) . fetchClassification) attempts
    && all (\index -> index < fromIntegral (length rows)) (Set.toList boundaries)
    && all (\(index,event) -> Set.notMember index boundaries || eventSource event == CpuFetch) (zip [0..] rows)) FetchRejected
  require (unique [eventCore event | (index,event) <- zip [0..] rows,Set.member index boundaries]) FetchRejected
  (final,_,_) <- foldM (advance boundaries) (segmentStart segment,M.empty,M.empty) (zip [0..] rows)
  require (final == segmentEnd segment) FetchRejected
  where
    advance boundaries state@(progress,lastFetch,lastTick) (index,event)
      | eventSource event `notElem` [CpuFetch,CpuRead,CpuWrite] = pure state
      | otherwise = do
          which <- required FetchRejected (eventCore event)
          site <- required FetchRejected (eventPc event)
          at <- required FetchRejected (eventInstructions event)
          tick <- required FetchRejected (eventTick event)
          require (maybe True (<=tick) (M.lookup which lastTick)) EventOrderRejected
          let ticks = M.insert which tick lastTick
          case eventSource event of
            CpuFetch -> do
              require (eventAddress event == fromIntegral (address site)
                && eventSize event == 4 && eventValue event == Nothing && eventBefore event == Nothing
                && eventByteOrder event == Just Little) FetchRejected
              if Set.member index boundaries then do
                require (count at == sum (map count progress) && progress == segmentEnd segment && any (\context -> contextCore context == which
                  && K.contextPc (contextMachine context) == site && K.contextCounts (contextMachine context) == progress) (segmentEndContexts segment)) FetchRejected
                pure (progress,lastFetch,ticks)
              else do
                require (count at == sum (map count progress)+1) FetchRejected
                next <- case (which,progress) of
                  (Primary,[a,b]) -> pure [Count (count a+1),b]
                  (Secondary,[a,b]) -> pure [a,Count (count b+1)]
                  _ -> Left FetchRejected
                pure (next,M.insert which (site,at) lastFetch,ticks)
            _ -> do
              require (M.lookup which lastFetch == Just (site,at)) FetchRejected
              pure (progress,lastFetch,ticks)
