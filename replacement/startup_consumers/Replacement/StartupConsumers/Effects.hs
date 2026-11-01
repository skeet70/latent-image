{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.Effects (ConsumerProjection, familyEntryAddress, checkConsumers, consumerProjection, consumerProjectionValue) where

import Control.Monad (foldM, forM_, when)
import qualified Data.Aeson as J
import Data.Bits ((.&.), (.|.), shiftL, xor)
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Set as Set
import Data.Word (Word32, Word64)
import Replacement.KernelInstall.Wire (Core (..), Bytes (..), MemoryEvent (..))
import qualified Replacement.KernelInstall.Wire as K
import Replacement.StartupConsumers.Evidence
import qualified Replacement.StartupConsumers.Source as S
import Replacement.StartupConsumers.Wire

data Call = Call {callEntry :: Probe, callExit :: Probe}
data ConsumerProjection = ConsumerProjection [InvocationProjection] [ResourceProjection] [(Family,Core,K.Count)] deriving (Eq)
data InvocationProjection = InvocationProjection Family Core InputProjection OutputProjection [CallProjection] deriving (Eq)
data InputProjection = TaskInput [K.WordValue] | ReserveInput K.WordValue K.WordValue PointerLocation
  | ConstructInput K.WordValue [K.WordValue] | MessageInput K.WordValue MessageProjection (Maybe K.WordValue) deriving (Eq)
data OutputProjection = ScalarOutput K.WordValue | ConstructOutput K.WordValue [K.WordValue]
  | MessageOutput K.WordValue MessageProjection deriving (Eq)
data MessageProjection = MessageProjection K.WordValue K.WordValue PointerLocation deriving (Eq)
data Argument = ScalarArgument K.WordValue | PointerArgument PointerLocation deriving (Eq)
data CallProjection = CallProjection K.Address K.Address [Argument] [Argument] (Maybe K.WordValue) deriving (Eq)
data PointerLocation = AbsoluteLocation K.Address | LocalLocation Family ResourceKind Int K.Count | FrameLocation Family Integer deriving (Eq)
data ResourceProjection = ResourceProjection ResourceKind Lifetime PointerLocation Bytes Bytes [ComparableWrite] deriving (Eq)
data WritePhase = InstallationWrites | ConsumerWrites deriving (Eq)
data ComparableWrite = ComparableWrite WritePhase K.Source (Maybe Core) (Maybe K.Address) PointerLocation K.Count (Maybe Bytes) Bytes deriving (Eq)
data RootSelector = PrimaryRoot | SecondaryRoot deriving (Eq)
rootIndex :: RootSelector -> Word32
rootIndex PrimaryRoot = 0
rootIndex SecondaryRoot = 1

familyEntryAddress :: Family -> K.Address
familyEntryAddress family = K.Address $ case family of
  TaskWrapper -> 0x725dd8; TaskReserve -> 0x725c6c; StorageReserve -> 0x723470
  MessageConstruct -> 0x724cdc; MessageSend -> 0x72503c; MessageReceive -> 0x725318
familyEpilogue :: Family -> Word32
familyEpilogue family = case family of
  TaskWrapper -> 0x726094; TaskReserve -> 0x725dcc; StorageReserve -> 0x7235cc
  MessageConstruct -> 0x724f04; MessageSend -> 0x725210; MessageReceive -> 0x725420

checkConsumers :: S.SourceModel -> Receipt -> Either WitnessError ()
checkConsumers source receipt = do
  checkKernelBaselines receipt
  let invocations = [invocation | invocation <- receiptInvocations receipt,invocationStatus invocation /= NotObserved]
  require (unique (map invocationId invocations)) InvocationRejected
  forM_ invocations $ \invocation -> do
    entry <- entryOf receipt invocation
    returned <- returnOf receipt invocation
    epilogue <- sourceAt receipt invocation (familyEpilogue (invocationFamily invocation))
    require (probeKind entry == FamilyEntry && pc entry == address (familyEntryAddress (invocationFamily invocation))
      && core entry == invocationCore invocation && lr entry == address (invocationLr invocation)
      && sp entry == address (invocationSp invocation) && probeFamily entry == Just (invocationFamily invocation)
      && probeKind returned == FamilyReturn && pc returned == lr entry && sp returned == sp entry
      && probeId entry < probeId epilogue && probeId epilogue < probeId returned
      && core returned == core entry && probeFamily returned == probeFamily entry
      && take 8 (drop 4 (K.contextRegisters (machine returned))) == take 8 (drop 4 (K.contextRegisters (machine entry)))
      && K.contextTcm (machine returned) == K.contextTcm (machine entry)) InvocationRejected
    case invocationParent invocation of
      Nothing -> pure ()
      Just parentId -> do
        parent <- one InvocationRejected [candidate | candidate <- invocations,invocationId candidate == parentId]
        parentEntry <- entryOf receipt parent
        parentReturn <- returnOf receipt parent
        require (core entry == core parentEntry && probeId parentEntry < probeId entry && probeId returned < probeId parentReturn) InvocationRejected
        target <- one InvocationRejected [probe | probe <- invocationProbes receipt parent,probeKind probe == DynamicTarget
          ,pc probe == pc entry,lr probe == lr entry,sp probe == sp entry,probeClock probe == probeClock entry]
        require (machine target == machine entry) InvocationRejected
    forM_ (invocationProbes receipt invocation) $ \probe -> require (core probe == core entry && probeFamily probe == Just (invocationFamily invocation)
      && probeId entry <= probeId probe && probeId probe <= probeId returned) InvocationRejected
  forM_ invocations $ \invocation ->
    case invocationFamily invocation of
      TaskReserve -> checkAllocation source receipt invocation S.TaskAllocation
      StorageReserve -> checkAllocation source receipt invocation S.StorageAllocation
      TaskWrapper -> checkTask source receipt invocation
      MessageConstruct -> checkConstruct source receipt invocation
      MessageSend -> checkSend source receipt invocation
      MessageReceive -> checkReceive source receipt invocation

atWord :: Receipt -> Probe -> Word32 -> Either WitnessError Word32
atWord receipt probe location = memoryAt receipt probe >>= \contents -> wordAt EffectRejected contents location
atBytes :: Receipt -> Probe -> Word32 -> Word64 -> Either WitnessError B.ByteString
atBytes receipt probe location size = memoryAt receipt probe >>= \contents -> bytesAt EffectRejected contents (physical location) size
args :: Probe -> Int -> Either WitnessError [Word32]
args probe size = mapM (register probe) [0..size-1]
args2 :: Probe -> Either WitnessError (Word32,Word32)
args2 probe = (,) <$> register probe 0 <*> register probe 1
args3 :: Probe -> Either WitnessError (Word32,Word32,Word32)
args3 probe = (,,) <$> register probe 0 <*> register probe 1 <*> register probe 2
status :: Call -> Either WitnessError Word32
status = (`register` 0) . callExit

call :: Receipt -> Invocation -> Word32 -> Word32 -> Either WitnessError Call
call receipt invocation site target = do
  entry <- sourceAt receipt invocation site
  exited <- sourceAt receipt invocation (site+4)
  selected <- one CallRejected [probe | probe <- invocationProbes receipt invocation,probeKind probe == DynamicTarget
    ,pc probe == target,lr probe == site+4,sp probe == sp entry,probeId entry < probeId probe,probeId probe < probeId exited]
  returned <- one CallRejected [probe | probe <- invocationProbes receipt invocation,probeKind probe == DynamicReturn
    ,pc probe == site+4,sp probe == sp entry,probeId selected < probeId probe,probeId probe <= probeId exited]
  require (core selected == core entry && core returned == core entry
    && K.contextRegisters (machine selected) == K.contextRegisters (machine entry)
    && machine returned == machine exited
    && take 8 (drop 4 (K.contextRegisters (machine returned))) == take 8 (drop 4 (K.contextRegisters (machine selected)))) CallRejected
  pure (Call entry exited)

service :: S.SourceModel -> Receipt -> Invocation -> Word32 -> RootSelector -> Word32 -> Either WitnessError Call
service source receipt invocation site selected offset = do
  entry <- sourceAt receipt invocation site
  let root = if selected == PrimaryRoot then 0x4004dae0 else 0x400ddaa0
      stores = if selected == PrimaryRoot then S.primaryStores source else S.secondaryStores source
  expected <- one CallRejected [number (S.storeValue store) | store <- stores,address (S.storeAddress store) == root+offset]
  actualRoot <- atWord receipt entry (0x96bd20+4*rootIndex selected)
  actualTarget <- atWord receipt entry (root+offset)
  targetRegister <- register entry 3
  require (actualRoot == root && actualTarget == expected && targetRegister == expected) CallRejected
  when (offset `elem` [0x170,0xac,0xb0,0xc8,0xf8]) $ checkSchedulerRanges receipt invocation entry selected
  observed <- call receipt invocation site expected
  when (offset `elem` [0x170,0xac,0xb0,0xc8]) $ checkRetainedPrehistory receipt observed
  pure observed

checkRetainedPrehistory :: Receipt -> Call -> Either WitnessError ()
checkRetainedPrehistory receipt observed = do
  before <- memoryAt receipt (callEntry observed)
  let boundaries = Set.fromList [(fetchSegment attempt,count (fetchRow attempt)) | attempt <- receiptFetches receipt]
      accesses = [(K.Address location,kind) | (location,kind) <-
        [(0x4b430,K.CpuRead),(0xdb3f0,K.CpuRead),(0x48fb8,K.CpuRead),(0xd8f78,K.CpuRead)
        ,(0x48f04,K.CpuWrite),(0xd8ec4,K.CpuWrite),(0x48f58,K.CpuWrite),(0xd8f18,K.CpuWrite)]]
      executed = [event | segment <- receiptSegments receipt
        ,(index,event) <- zip [0 :: Word64 ..] (K.watchRecords (segmentWatch segment))
        ,Set.notMember (segmentId segment,index) boundaries,eventSource event == K.CpuFetch
        ,eventCore event == Just (core (callEntry observed)),Just instruction <- [eventPc event],instruction `elem` map fst accesses
        ,Just at <- [eventInstructions event],count at > probeClock (callEntry observed),count at <= probeClock (callExit observed)]
  forM_ executed $ \fetched -> do
    instruction <- required EffectRejected (eventPc fetched)
    kind <- required EffectRejected (lookup instruction accesses)
    event <- one EffectRejected [candidate | candidate <- allEvents receipt,eventSource candidate == kind
      ,eventCore candidate == eventCore fetched,eventPc candidate == Just instruction,eventInstructions candidate == eventInstructions fetched]
    backing <- eventPhysical32 event
    at <- eventClock event
    require (eventSize event == 4) EffectRejected
    let covering = [resource | resource <- receiptResources receipt,resourceKind resource `notElem` [Code,Carrier]
          ,clock (resourceAdmitted resource) <= probeClock (callEntry observed)
          ,address (resourcePhysical resource) <= backing
          ,toInteger backing+4 <= toInteger (address (resourcePhysical resource))+toInteger (count (resourceSize resource))]
    ends <- mapM (resourceEnd receipt) covering
    require (any ((>= at) . clock) ends) LifetimeRejected
    _ <- bytesAt PreimageRejected before backing 4
    pure ()

identity :: S.SourceModel -> Receipt -> Invocation -> Word32 -> Either WitnessError RootSelector
identity source receipt invocation site = do
  observed <- service source receipt invocation site PrimaryRoot 0x1618
  result <- status observed
  require (result == if invocationCore invocation == Primary then 0 else 1) CallRejected
  pure (if invocationCore invocation == Primary then PrimaryRoot else SecondaryRoot)

classEncoding :: Receipt -> Probe -> Word32 -> Either WitnessError (RootSelector,Word32)
classEncoding receipt probe ident = do
  _ <- required InvocationRejected (S.taskId ident)
  classWord <- atWord receipt probe (0x7cad9c+4*(ident-1))
  selected <- case classWord of 1 -> pure PrimaryRoot; 2 -> pure SecondaryRoot; _ -> Left InvocationRejected
  pure (selected,ident .|. (classWord `shiftL` 16))

ensureArgs :: WitnessError -> Probe -> [Word32] -> Either WitnessError ()
ensureArgs failure probe expected = args probe (length expected) >>= \actual -> require (actual == expected) failure
ensureResult :: WitnessError -> Receipt -> Invocation -> Word32 -> Either WitnessError ()
ensureResult failure receipt invocation expected = returnOf receipt invocation >>= \probe -> register probe 0 >>= \actual -> require (actual == expected) failure

absentSites :: Receipt -> Invocation -> [Word32] -> Either WitnessError ()
absentSites receipt invocation sites = require (all (\probe -> pc probe `notElem` sites) (invocationProbes receipt invocation)) InvocationRejected

requireLifetime :: Receipt -> Invocation -> ResourceKind -> Lifetime -> Probe -> Probe -> Word32 -> Word64 -> Either WitnessError ()
requireLifetime receipt invocation kind lifetime begin end pointer size = do
  resource <- one LifetimeRejected [item | item <- receiptResources receipt,resourceKind item == kind
    ,resourceOwner item == Just (invocationId invocation),resourceLifetime item == lifetime
    ,address (resourcePhysical item) == physical pointer,count (resourceSize item) == size]
  terminal <- resourceEnd receipt resource
  require (clock (resourceAdmitted resource) <= probeClock begin && clock terminal >= probeClock end) LifetimeRejected
  when (lifetime == ReceiveReleaseLifetime) $ require (clock terminal == probeClock end && resourceClosed resource /= Nothing) LifetimeRejected

data LockOperation = AcquireLock K.WordValue | ReleaseLock

checkLockAccesses :: Receipt -> Call -> LockOperation -> Either WitnessError ()
checkLockAccesses receipt observed operation = do
  fetched <- executedFetchesAt [K.Address loadPc,K.Address storePc] receipt
  let executed = [executedRow occurrence | occurrence <- fetched
        ,eventCore (executedRow occurrence) == Just (core (callEntry observed))
        ,Just at <- [eventInstructions (executedRow occurrence)]
        ,count at > probeClock (callEntry observed),count at <= probeClock (callExit observed)]
  walk executed
  where
    (loadPc,storePc,written) = case operation of
      AcquireLock owner -> (0x427d4,0x427e4,fromIntegral (number owner))
      ReleaseLock -> (0x42800,0x42810,0)
    matching kind fetched = [event | event <- allEvents receipt,eventSource event == kind
      ,eventCore event == eventCore fetched,eventPc event == eventPc fetched,eventInstructions event == eventInstructions fetched]
    checkRow event = require (eventAddress event == 0x409c280c && eventPhysical event == Just 0x9c280c
      && eventSize event == 4 && eventByteOrder event == Just K.Little) AllocationRejected
    terminal expected remaining = do
      actual <- status observed
      require (null remaining && actual == expected) AllocationRejected
    walk [] = Left AllocationRejected
    walk (loaded:remaining) = do
      require (eventPc loaded == Just (K.Address loadPc)) AllocationRejected
      readEvent <- one AllocationRejected (matching K.CpuRead loaded)
      checkRow readEvent
      value <- required AllocationRejected (eventValue readEvent)
      let unavailable = case operation of AcquireLock _ -> value /= 0; ReleaseLock -> value == 0
      if unavailable then terminal (negate 1) remaining else case remaining of
        stored:rest -> do
          require (eventPc stored == Just (K.Address storePc)) AllocationRejected
          case matching K.CpuWrite stored of
            [] -> walk rest
            [writeEvent] -> do
              checkRow writeEvent
              require (eventValue writeEvent == Just written) AllocationRejected
              terminal 0 rest
            _ -> Left AllocationRejected
        [] -> Left AllocationRejected

checkAllocation :: S.SourceModel -> Receipt -> Invocation -> S.AllocationKind -> Either WitnessError ()
checkAllocation source receipt invocation kind = do
  entry <- entryOf receipt invocation
  returned <- returnOf receipt invocation
  outer <- probeAt receipt OuterEntry
  lock <- one AdmissionRejected [resource | resource <- receiptResources receipt,resourceKind resource == Record
    ,resourceLifetime resource == ObservationLifetime,resourceOwner resource == Nothing
    ,address (resourcePhysical resource) == 0x9c280c,count (resourceSize resource) == 4]
  require (clock (resourceAdmitted lock) == probeClock outer) AdmissionRejected
  (index,requested,out) <- args3 entry
  require (index < if kind == S.TaskAllocation then 120 else 240) AllocationRejected
  let isTask = kind == S.TaskAllocation
      record = (if isTask then 0x40a0fc44 else 0x409c3904)+12*index
      cursorCell = if isTask then 0x4089ea98 else 0x4089ea90
      site storage task = if isTask then task else storage
  selected <- identity source receipt invocation (site 0x723498 0x725c94)
  saved <- service source receipt invocation (site 0x7234a8 0x725ca4) selected 0x164
  mask <- status saved
  require (mask == 0 || mask == 0x80) AllocationRejected
  locked <- call receipt invocation (site 0x7234c4 0x725cc0) 0x427d4
  ensureArgs AllocationRejected (callEntry locked) [0x409c280c,rootIndex selected+1]
  checkLockAccesses receipt locked (AcquireLock (K.WordValue (rootIndex selected+1)))
  decision <- sourceAt receipt invocation (site 0x7234d8 0x725cd4)
  let operand at location = number <$> readOperand AllocationRejected receipt invocation (K.Address at) (K.Address location)
      stores = map (\(location,value) -> S.Store (K.Address location) (K.WordValue value))
      writeSites = map K.Address [site 0x723520 0x725d1c,site 0x723534 0x725d30,site 0x723564 0x725d64
        ,site 0x72357c 0x725d7c,site 0x723594 0x725d94,site 0x723598 0x725d98,site 0x7235a0 0x725da0]
  base <- operand (site 0x7234d8 0x725cd4) record
  size <- if base == 0 then atWord receipt decision (record+4) else operand (site 0x7234f4 0x725cf0) (record+4)
  busy <- if base /= 0 && size == requested then operand (site 0x723510 0x725d0c) (record+8) else atWord receipt decision (record+8)
  cursor <- if base == 0 then operand (site 0x723548 0x725d44) cursorCell else atWord receipt decision cursorCell
  let input = S.AllocationInput kind (S.ByteCount requested) (K.Address cursor)
        (S.Reservation (K.Address base) (S.ByteCount size) (S.reservationState (K.WordValue busy)))
      expected = S.reserve input
  unlocked <- call receipt invocation (site 0x7235ac 0x725dac) 0x427fc
  ensureArgs AllocationRejected (callEntry unlocked) [0x409c280c]
  checkLockAccesses receipt unlocked ReleaseLock
  restored <- service source receipt invocation (site 0x7235c4 0x725dc4) selected 0x168
  ensureArgs AllocationRejected (callEntry restored) [mask]
  require (probeId (callExit locked) < probeId decision && probeId decision < probeId (callEntry unlocked)
    && probeId (callExit unlocked) < probeId (callEntry restored)) AllocationRejected
  case expected of
    S.AllocationSizeRejected -> do
      ensureResult AllocationRejected receipt invocation (negate 33)
      checkAllocationWrites receipt invocation writeSites []
      absentSites receipt invocation [site 0x723520 0x725d1c,site 0x723564 0x725d64]
    S.AllocationBusyRejected -> do
      ensureResult AllocationRejected receipt invocation (negate 18)
      checkAllocationWrites receipt invocation writeSites []
      absentSites receipt invocation [site 0x723520 0x725d1c,site 0x723564 0x725d64]
    S.Allocated pointer _ _ -> do
      ensureResult AllocationRejected receipt invocation 0
      if base == 0 then do
        cursorReload <- operand (site 0x723574 0x725d74) cursorCell
        outputReload <- operand (site 0x723580 0x725d80) out
        checkAllocationWrites receipt invocation writeSites (stores
          [(out,address pointer),(cursorCell,cursorReload+(requested .&. 0xfffffffc)),(record,outputReload),(record+4,requested),(record+8,1)])
        mapM_ (sourceAt receipt invocation) [site 0x723548 0x725d44,site 0x723558 0x725d54,site 0x723564 0x725d64
          ,site 0x72357c 0x725d7c,site 0x723594 0x725d94,site 0x723598 0x725d98,site 0x7235a0 0x725da0]
        absentSites receipt invocation [site 0x723520 0x725d1c]
      else do
        checkAllocationWrites receipt invocation writeSites (stores [(out,base),(record+8,1)])
        mapM_ (sourceAt receipt invocation) [site 0x7234f4 0x725cf0,site 0x723510 0x725d0c,site 0x723520 0x725d1c,site 0x723534 0x725d30]
        absentSites receipt invocation [site 0x723564 0x725d64]
      when (requested > 0) $ do
        actualPointer <- atWord receipt returned out
        requireLifetime receipt invocation Buffer AllocationLifetime returned returned actualPointer (fromIntegral requested)
  require (probeId entry < probeId returned) AllocationRejected

readOperand :: WitnessError -> Receipt -> Invocation -> K.Address -> K.Address -> Either WitnessError K.WordValue
readOperand failure receipt invocation site location = readScalar failure receipt invocation site location (S.ByteCount 4)

readScalar :: WitnessError -> Receipt -> Invocation -> K.Address -> K.Address -> S.ByteCount -> Either WitnessError K.WordValue
readScalar failure receipt invocation site location (S.ByteCount width) = do
  require (width `elem` [1,2,4]) failure
  entry <- entryOf receipt invocation
  returned <- returnOf receipt invocation
  event <- one failure [event | event <- allEvents receipt,K.eventSource event == K.CpuRead
    ,K.eventCore event == Just (invocationCore invocation),K.eventPc event == Just site
    ,K.eventAddress event == fromIntegral (address location),K.eventSize event == fromIntegral width
    ,Just at <- [K.eventInstructions event],count at > probeClock entry,count at <= probeClock returned]
  value <- required failure (K.eventValue event)
  require (value < (1 `shiftL` fromIntegral (width*8))) failure
  pure (K.WordValue (fromIntegral value))

checkAllocationWrites :: Receipt -> Invocation -> [K.Address] -> [S.Store] -> Either WitnessError ()
checkAllocationWrites receipt invocation sites expected = checkWrites AllocationRejected receipt invocation sites
  [(S.storeAddress store,S.ByteCount 4,S.storeValue store) | store <- expected]

checkWrites :: WitnessError -> Receipt -> Invocation -> [K.Address] -> [(K.Address,S.ByteCount,K.WordValue)] -> Either WitnessError ()
checkWrites failure receipt invocation sites expected = do
  entry <- entryOf receipt invocation
  returned <- returnOf receipt invocation
  let selected = [event | event <- allEvents receipt,K.eventSource event == K.CpuWrite
        ,K.eventCore event == Just (invocationCore invocation),K.eventPc event `elem` map Just sites
        ,Just at <- [K.eventInstructions event],count at > probeClock entry,count at <= probeClock returned]
  require ([(K.eventAddress event,K.eventSize event,K.eventValue event) | event <- selected]
    == [(fromIntegral (address location),fromIntegral size,Just (fromIntegral (number value))) | (location,S.ByteCount size,value) <- expected]) failure

errorPath :: Receipt -> Invocation -> Word32 -> Either WitnessError ()
errorPath receipt invocation errorCode = do
  returned <- returnOf receipt invocation
  actual <- atWord receipt returned 0x40a0f82c
  require (actual == errorCode) EffectRejected
  callbackSite <- one CallRejected [probe | probe <- invocationProbes receipt invocation,probeKind probe == SourceProbe,pc probe == 0x7233ac]
  target <- atWord receipt callbackSite 0x4089ea8c
  ensureArgs CallRejected callbackSite [errorCode]
  _ <- call receipt invocation 0x7233ac target
  ensureResult EffectRejected receipt invocation errorCode

data KernelLayout = KernelLayout
  {kernelCreation :: Word32, kernelNodes :: Word32, kernelIds :: Word32, kernelAuxiliary :: Word32
  ,kernelAuxiliaryNodes :: Word32, kernelControl :: Word32, kernelDomains :: Word32
  ,kernelSelected :: Word32, kernelCurrent :: Word32, kernelDomainPointer :: Word32}

kernelLayout :: RootSelector -> KernelLayout
kernelLayout selected = case selected of
  PrimaryRoot -> KernelLayout 0x4005beac 0x4005ff0c 0x400622d4 0x4005d178 0x400624b4 0x400545a4 0x400545cc 0x40054768 0x40054760 0x400545c8
  SecondaryRoot -> KernelLayout 0x400ebe6c 0x400efecc 0x400f2294 0x400ed138 0x400f2474 0x400e4564 0x400e458c 0x400e4728 0x400e4720 0x400e4588

data MessageLayout = MessageLayout
  {messageCreation :: Word32,messageNodes :: Word32,messagePoolCreation :: Word32,messagePoolNodes :: Word32
  ,messageFallback :: Word32,messagePoolFallback :: Word32,messageCreateCode :: Word32,messagePoolCode :: Word32}
data MessageCreation = QueueCreation | PoolCreation deriving (Eq)

messageLayout :: RootSelector -> MessageLayout
messageLayout selected = case selected of
  PrimaryRoot -> MessageLayout 0x4005988c 0x4005a00c 0x4006194c 0x4005f584 0x40059284 0x40062c4c 0x4d49c 0x4d6c4
  SecondaryRoot -> MessageLayout 0x400e984c 0x400e9fcc 0x400f190c 0x400ef544 0x400e9244 0x400f2c0c 0xdd460 0xdd688

checkKernelBaselines :: Receipt -> Either WitnessError ()
checkKernelBaselines receipt = do
  outer <- probeAt receipt OuterEntry
  require (any (\resource -> resourceKind resource == Record && resourceOwner resource == Nothing
    && resourceLifetime resource == ObservationLifetime && resourcePhysical resource == K.Address 0xa0f464
    && count (resourceSize resource) == 960 && clock (resourceAdmitted resource) == probeClock outer) (receiptResources receipt)) LifetimeRejected
  forM_ [PrimaryRoot,SecondaryRoot] $ \selected -> do
    let layout = kernelLayout selected
        messages = messageLayout selected
        records = [(kernelCreation layout,4800),(kernelNodes layout,6720),(kernelIds layout,480),(kernelControl layout,456)
          ,(messageCreation messages,1920),(messageNodes messages,3840),(messagePoolCreation messages,2400),(messagePoolNodes messages,2400)]
        ranges = [(Record,pointer,size) | (pointer,size) <- records]
          ++ [(Auxiliary,kernelAuxiliary layout,1440),(Auxiliary,kernelAuxiliaryNodes layout,1920)
             ,(Buffer,messageFallback messages,4),(Buffer,messagePoolFallback messages,8)]
    forM_ ranges $ \(kind,pointer,size) ->
      require (any (\resource -> resourceKind resource == kind && resourceOwner resource == Nothing
        && resourceLifetime resource == ObservationLifetime && address (resourcePhysical resource) == physical pointer
        && count (resourceSize resource) == size && clock (resourceAdmitted resource) == probeClock outer) (receiptResources receipt)) LifetimeRejected

checkMessageCreation :: Receipt -> Invocation -> RootSelector -> Word32 -> Call -> MessageCreation -> Either WitnessError ()
checkMessageCreation receipt invocation selected index observed creation = do
  let layout = messageLayout selected
      pool = creation == PoolCreation
      base = if pool then messagePoolCode layout else messageCreateCode layout
      record = (if pool then messagePoolCreation layout else messageCreation layout)+(if pool then 20 else 16)*index
      node = (if pool then messagePoolNodes layout else messageNodes layout)+(if pool then 20 else 32)*index
      readAt offset pointer = number <$> readOperand ConstructRejected receipt invocation (K.Address (base+offset)) (K.Address pointer)
      write offset pointer value = (K.Address (base+offset),(K.Address pointer,S.ByteCount 4,K.WordValue value))
  busy <- readAt (if pool then 0x2c else 0x28) (record+(if pool then 16 else 12))
  actualStatus <- status observed
  require (actualStatus == if busy == 0 then 0 else negate 41) ConstructRejected
  let offsets = if pool then [0x54,0x64,0x6c,0x80,0x88,0xa4,0xa8,0xb4,0xbc]
        else [0x44,0x50,0x58,0x60,0x74,0x78,0x80,0x84,0x8c,0x90,0x94]
  writes <- if busy /= 0 then pure [] else do
    parameters <- register (callEntry observed) 1
    if pool then do
      flags <- readAt 0x3c parameters
      stride <- readAt 0x58 (parameters+8)
      pointer <- readAt 0x68 (parameters+12)
      elements <- readAt 0x70 (parameters+4)
      rawStride <- readAt 0x74 (parameters+8)
      rawPointer <- readAt 0x78 (parameters+12)
      nodeRecord <- readAt 0xac (node+8)
      nodeBuffer <- readAt 0xb0 (nodeRecord+8)
      pure [write 0x54 record flags,write 0x64 (record+4) ((stride+3) .&. 0xfffffffc)
        ,write 0x6c (record+8) pointer,write 0x80 (record+12) (rawPointer+elements*rawStride)
        ,write 0x88 (record+16) 1,write 0xa4 node node,write 0xa8 (node+4) node
        ,write 0xb4 (node+12) nodeBuffer,write 0xbc (node+16) 0]
    else do
      flags <- readAt 0x38 parameters
      elements <- readAt 0x48 (parameters+4)
      pointer <- readAt 0x54 (parameters+8)
      pure [write 0x44 record flags,write 0x50 (record+4) elements,write 0x58 (record+8) pointer
        ,write 0x60 (record+12) 1,write 0x74 node node,write 0x78 (node+4) node
        ,write 0x80 (node+12) (node+12),write 0x84 (node+16) (node+12)
        ,write 0x8c (node+20) 0,write 0x90 (node+24) 0,write 0x94 (node+28) 0]
  checkWrites ConstructRejected receipt invocation (map (K.Address . (base+)) offsets) (map snd writes)

observeRange :: Receipt -> Invocation -> Probe -> Word32 -> Word64 -> Either WitnessError ()
observeRange receipt invocation probe pointer size = require (any covers (receiptResources receipt)) LifetimeRejected
  where
    covers resource = resourceOwner resource == Just (invocationId invocation) && resourceKind resource `elem` [Record,Auxiliary]
      && resourceLifetime resource == AllocationLifetime && address (resourcePhysical resource) <= physical pointer
      && toInteger (physical pointer)+toInteger size <= toInteger (address (resourcePhysical resource))+toInteger (count (resourceSize resource))
      && clock (resourceAdmitted resource) <= probeClock probe

checkKernelInitial :: Receipt -> Invocation -> Probe -> Either WitnessError ()
checkKernelInitial receipt invocation entry = do
  memory <- memoryAt receipt entry
  forM_ [PrimaryRoot,SecondaryRoot] $ \selected -> do
    let layout = kernelLayout selected
    forM_ [(kernelCreation layout,4800),(kernelNodes layout,6720),(kernelIds layout,480),(kernelAuxiliary layout,1440)
      ,(kernelAuxiliaryNodes layout,1920),(kernelControl layout,456)] $ \(pointer,size) -> observeRange receipt invocation entry pointer size
    forM_ [0..119] $ \index -> do
      let creation = kernelCreation layout+40*index
          node = kernelNodes layout+56*index
          defaultStack = if selected == PrimaryRoot then 0x40059280 else 0x400e9240
      values <- mapM (wordAt TaskRejected memory . (creation+)) [0,4..36]
      require (values == [0,index,0,15,4,defaultStack,0,0,index+1,0]) TaskRejected
      ident <- wordAt TaskRejected memory (kernelIds layout+4*index)
      auxiliary <- bytesAt TaskRejected memory (physical (kernelAuxiliary layout+12*index)) 12
      created <- wordAt TaskRejected memory (node+8)
      flags <- bytesAt TaskRejected memory (physical (node+12)) 3
      require (ident == index+1 && auxiliary == B.replicate 12 0 && created == creation
        && B.index flags 0 == 0 && B.index flags 1 == 15 && B.index flags 2 .&. 7 == 0) TaskRejected
    forM_ [kernelCurrent layout,kernelSelected layout] $ \location -> wordAt TaskRejected memory location >>= \value -> require (value == 0) TaskRejected
    domain <- wordAt TaskRejected memory (kernelDomainPointer layout)
    require (domain == kernelDomains layout+132*rootIndex selected) TaskRejected

checkSchedulerRanges :: Receipt -> Invocation -> Probe -> RootSelector -> Either WitnessError ()
checkSchedulerRanges receipt invocation probe selected = do
  let layout = kernelLayout selected
  forM_ [kernelDomains layout,kernelDomains layout+132] $ \domain ->
    forM_ [(0x20,128),(0x24,4),(0x28,4),(0x2c,4)] $ \(field,size) -> do
      pointer <- atWord receipt probe (domain+field)
      observeRange receipt invocation probe pointer size

checkAuxiliary :: Receipt -> Invocation -> RootSelector -> Word32 -> Call -> [Word32] -> Either WitnessError ()
checkAuxiliary receipt invocation selected index observed parameters = do
  let layout = kernelLayout selected
  checkSchedulerRanges receipt invocation (callEntry observed) selected
  let record = kernelAuxiliary layout+12*index; node = kernelAuxiliaryNodes layout+16*index
  busy <- atWord receipt (callEntry observed) (record+8)
  actualStatus <- status observed
  require (actualStatus == if busy == 0 then 0 else negate 41) TaskRejected
  if busy /= 0 then do
    before <- atBytes receipt (callEntry observed) record 12
    after <- atBytes receipt (callExit observed) record 12
    require (before == after) TaskRejected
  else do
    actual <- mapM (atWord receipt (callExit observed) . (record+)) [0,4,8]
    links <- mapM (atWord receipt (callExit observed) . (node+)) [0,4,8,12]
    require (actual == parameters ++ [1] && links == [node,node,record,0]) TaskRejected

checkCreation :: Receipt -> Invocation -> RootSelector -> Word32 -> Call -> [Word32] -> Either WitnessError ()
checkCreation receipt invocation selected index observed parameters = do
  let layout = kernelLayout selected
  checkSchedulerRanges receipt invocation (callEntry observed) selected
  let creation = kernelCreation layout+40*index; node = kernelNodes layout+56*index
  busy <- atWord receipt (callEntry observed) (creation+36)
  actualStatus <- status observed
  require (actualStatus == if busy == 0 then 0 else negate 41) TaskRejected
  if busy /= 0 then do
    before <- atBytes receipt (callEntry observed) creation 40
    after <- atBytes receipt (callExit observed) creation 40
    require (before == after) TaskRejected
  else case parameters of
    [flags,argument,entry,priority,size,base] -> do
      actual <- mapM (atWord receipt (callExit observed) . (creation+)) [0,4..36]
      ident <- atWord receipt (callExit observed) (kernelIds layout+4*index)
      beforeNode <- atBytes receipt (callEntry observed) node 56
      afterNode <- atBytes receipt (callExit observed) node 56
      let expectedByte offset
            | offset == 12 = 0
            | offset == 13 = fromIntegral (priority-1)
            | offset == 14 = B.index beforeNode 14 .&. 0xf8
            | offset >= 16 && offset < 20 || offset >= 36 && offset < 52 = 0
            | offset >= 32 && offset < 36 = B.index (wordBytes (rootIndex selected+1)) (offset-32)
            | otherwise = B.index beforeNode offset
      require (actual == [flags,argument,entry,priority-1,(size+3) .&. 0xfffffffc,base,0,0,index+1,1]
        && ident == index+1 && afterNode == B.pack (map expectedByte [0..55])) TaskRejected
    _ -> Left TaskRejected

requirePayload :: Receipt -> Invocation -> Probe -> Probe -> Word32 -> Word32 -> Either WitnessError ()
requirePayload receipt invocation begin use pointer lengthWord = require (any matches (receiptPayloads receipt)) PayloadRejected
  where
    matches admission = payloadOwner admission == invocationId invocation && address (payloadPointer admission) == pointer
      && count (payloadLength admission) == fromIntegral lengthWord && clock (payloadContext admission) >= probeClock begin
      && clock (payloadContext admission) <= probeClock use
      && if lengthWord == 0 then payloadState admission == EmptyPayload && payloadResource admission == Nothing
        else payloadState admission == AdmittedPayload && payloadResource admission /= Nothing

checkTask :: S.SourceModel -> Receipt -> Invocation -> Either WitnessError ()
checkTask source receipt invocation = do
  entry <- entryOf receipt invocation
  returned <- returnOf receipt invocation
  descriptor <- register entry 0
  fields <- mapM (atWord receipt entry . (descriptor+)) [0,4,8,12,16,20,24]
  ident <- one TaskRejected (take 1 fields)
  identType <- required TaskRejected (S.taskId ident)
  let index = S.taskIndex identType; pair = 0x40a5b204+8*index
  (selected,encoded) <- classEncoding receipt entry ident
  checkKernelInitial receipt invocation entry
  classProbe <- sourceAt receipt invocation 0x725df8
  _ <- classEncoding receipt classProbe ident
  sizeProbe <- sourceAt receipt invocation 0x725e1c
  wordSize <- atWord receipt sizeProbe (pair+4)
  needsReservation <- if wordSize /= 0 then absentSites receipt invocation [0x725e34] >> pure False else do
    baseProbe <- sourceAt receipt invocation 0x725e34
    base <- atWord receipt baseProbe pair
    pure (base == 0)
  requireLifetime receipt invocation Descriptor InvocationLifetime entry returned descriptor 28
  forM_ [event | event <- allEvents receipt,eventSource event == K.CpuWrite] $ \event -> do
    at <- eventClock event
    backing <- eventPhysical32 event
    require (not (at > probeClock entry && at <= probeClock returned && backing < physical descriptor+28
      && fromIntegral backing+eventSize event > fromIntegral (physical descriptor))) WriterRejected
  let field offset = atWord receipt entry (descriptor+offset)
  reserveStatus <- if needsReservation then do
    size <- field 12
    reservation <- call receipt invocation 0x725e74 0x725c6c
    ensureArgs TaskRejected (callEntry reservation) [index,size,pair]
    published <- atWord receipt (callEntry reservation) (pair+4)
    require (published == size) TaskRejected
    status reservation
    else absentSites receipt invocation [0x725e74] >> pure 0
  if reserveStatus /= 0 then errorPath receipt invocation reserveStatus else do
    auxiliary <- service source receipt invocation 0x725f08 selected 0x80
    ensureArgs TaskRejected (callEntry auxiliary) [encoded]
    params <- register (callEntry auxiliary) 1
    tailWord <- field 24
    actualAux <- mapM (atWord receipt (callEntry auxiliary) . (params+)) [0,4]
    require (actualAux == [if tailWord == 0xa5f0a5f0 then 0 else 4,0]) TaskRejected
    auxStatus <- status auxiliary
    checkAuxiliary receipt invocation selected index auxiliary actualAux
    if auxStatus /= 0 && auxStatus /= negate 41 then do
      absentSites receipt invocation [0x725f68,0x725fa8,0x726024]
      errorPath receipt invocation auxStatus
    else do
      fill <- call receipt invocation 0x725f68 0x7221c8
      storedBase <- atWord receipt (callEntry fill) pair
      storedSize <- atWord receipt (callEntry fill) (pair+4)
      ensureArgs TaskRejected (callEntry fill) [storedBase,ident .&. 0xff,storedSize]
      filled <- atBytes receipt (callExit fill) storedBase (fromIntegral storedSize)
      require (filled == B.replicate (fromIntegral storedSize) (fromIntegral ident)) TaskRejected
      cache <- call receipt invocation 0x725f78 0x42634
      ensureArgs TaskRejected (callEntry cache) [physical storedBase,storedSize]
      create <- service source receipt invocation 0x725fa8 selected 0
      ensureArgs TaskRejected (callEntry create) [encoded]
      parameters <- register (callEntry create) 1
      argument <- field 20
      taskEntry <- field 16
      priority <- field 8
      actual <- mapM (atWord receipt (callEntry create) . (parameters+)) [0,4,8,12,16,20]
      require (actual == [2,argument,taskEntry,priority,storedSize,storedBase]) TaskRejected
      checkCreation receipt invocation selected index create actual
      exitCritical <- call receipt invocation 0x725fb4 0x7236b0
      require (probeId (callExit create) < probeId (callEntry exitCritical)) TaskRejected
      createStatus <- status create
      if createStatus /= 0 then absentSites receipt invocation [0x726024] >> errorPath receipt invocation createStatus else do
        current <- identity source receipt invocation 0x726000
        activated <- service source receipt invocation 0x726024 current 0xc
        ensureArgs TaskRejected (callEntry activated) [encoded]
        activatedStatus <- status activated
        checkSchedulerRanges receipt invocation (callEntry activated) current
        let layout = kernelLayout current
        activeContext <- atWord receipt (callEntry activated) (kernelControl layout)
        when (activeContext /= 0 || number (K.contextCpsr (machine (callEntry activated))) .&. 0x80 /= 0) $
          require (activatedStatus == negate 25) TaskRejected
        if activatedStatus == 0 then ensureResult TaskRejected receipt invocation 0 else errorPath receipt invocation activatedStatus
      when (storedSize > 0) $ requireLifetime receipt invocation Buffer AllocationLifetime (callEntry fill) returned storedBase (fromIntegral storedSize)
  finalDescriptor <- atBytes receipt returned descriptor 28
  initialDescriptor <- atBytes receipt entry descriptor 28
  require (finalDescriptor == initialDescriptor) TaskRejected

checkConstruct :: S.SourceModel -> Receipt -> Invocation -> Either WitnessError ()
checkConstruct source receipt invocation = do
  entry <- entryOf receipt invocation
  (ident,descriptor) <- args2 entry
  returned <- returnOf receipt invocation
  requireLifetime receipt invocation Descriptor InvocationLifetime entry returned descriptor 8
  _ <- required ConstructRejected (S.taskId ident)
  (selected,encoded) <- classEncoding receipt entry ident
  first <- call receipt invocation 0x724d20 0x723470
  countFirst <- number <$> readOperand ConstructRejected receipt invocation (K.Address 0x724d0c) (K.Address descriptor)
  firstOut <- register (callEntry first) 2
  ensureArgs ConstructRejected (callEntry first) [ident+119,4*countFirst,firstOut]
  firstStatus <- status first
  if firstStatus /= 0 then do
    absentSites receipt invocation [0x724d98,0x724e34,0x724ea4]
    errorPath receipt invocation firstStatus
  else do
    firstPointer <- atWord receipt (callExit first) firstOut
    firstService <- service source receipt invocation 0x724d98 selected 0xa0
    published <- atWord receipt (callEntry firstService) (0x40a0f870+4*(ident-1))
    ensureArgs ConstructRejected (callEntry firstService) [encoded]
    parameters <- register (callEntry firstService) 1
    values <- mapM (atWord receipt (callEntry firstService) . (parameters+)) [0,4,8]
    require (published == firstPointer && values == [0,countFirst,firstPointer]) ConstructRejected
    firstServiceStatus <- status firstService
    checkMessageCreation receipt invocation selected (ident-1) firstService QueueCreation
    if firstServiceStatus /= 0 then do
      absentSites receipt invocation [0x724e34,0x724ea4]
      errorPath receipt invocation firstServiceStatus
    else do
      second <- call receipt invocation 0x724e34 0x723470
      let pair = 0x40a0f464+8*(ident-1)
      requested <- atWord receipt (callEntry second) (pair+4)
      _ <- sourceAt receipt invocation 0x724de4
      _ <- sourceAt receipt invocation 0x724dec
      countReload <- number <$> readOperand ConstructRejected receipt invocation (K.Address 0x724de4) (K.Address descriptor)
      sizeReload <- number <$> readOperand ConstructRejected receipt invocation (K.Address 0x724dec) (K.Address (descriptor+4))
      ensureArgs ConstructRejected (callEntry second) [ident-1,countReload*(sizeReload+4),pair]
      publishedSize <- atWord receipt (callEntry second) (0x40a0fa50+4*(ident-1))
      require (requested == countReload*(sizeReload+4) && publishedSize == sizeReload) ConstructRejected
      secondStatus <- status second
      if secondStatus /= 0 then absentSites receipt invocation [0x724ea4] >> errorPath receipt invocation secondStatus else do
        secondPointer <- atWord receipt (callExit second) pair
        secondService <- service source receipt invocation 0x724ea4 selected 0xe4
        ensureArgs ConstructRejected (callEntry secondService) [encoded]
        secondParameters <- register (callEntry secondService) 1
        secondValues <- mapM (atWord receipt (callEntry secondService) . (secondParameters+)) [0,4,8,12]
        require (secondValues == [0,countReload,sizeReload+4,secondPointer]) ConstructRejected
        secondServiceStatus <- status secondService
        checkMessageCreation receipt invocation selected (ident-1) secondService PoolCreation
        if secondServiceStatus == 0 then do
          absentSites receipt invocation [0x724ee4]
          ensureResult ConstructRejected receipt invocation 0
        else do
          cleanup <- service source receipt invocation 0x724ee4 selected 0xa4
          ensureArgs ConstructRejected (callEntry cleanup) [encoded]
          errorPath receipt invocation secondServiceStatus

checkSend :: S.SourceModel -> Receipt -> Invocation -> Either WitnessError ()
checkSend source receipt invocation = do
  entry <- entryOf receipt invocation
  (ident,message) <- args2 entry
  completed <- returnOf receipt invocation
  requireLifetime receipt invocation Descriptor InvocationLifetime entry completed message 12
  _ <- required SendRejected (S.taskId ident)
  selected <- identity source receipt invocation 0x725064
  sizeProbe <- sourceAt receipt invocation 0x725078
  size <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725078) (K.Address (0x40a0fa50+4*(ident-1)))
  comparison <- sourceAt receipt invocation 0x725084
  lengthBefore <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725080) (K.Address (message+4))
  comparedLength <- register comparison 2
  comparedCapacity <- register comparison 3
  require (probeId sizeProbe < probeId comparison && comparedLength == lengthBefore && comparedCapacity == size-8) SendRejected
  if lengthBefore > size-8 then do
    absentSites receipt invocation [0x7250cc,0x725100,0x7251b4]
    errorPath receipt invocation (negate 60)
  else do
    callbackProbe <- sourceAt receipt invocation 0x7250cc
    callback <- atWord receipt callbackProbe 0x4089ea94
    observedCallback <- call receipt invocation 0x7250cc callback
    ensureArgs SendRejected (callEntry observedCallback) [message]
    beforePointer <- atWord receipt callbackProbe (message+8)
    beforeLength <- atWord receipt callbackProbe (message+4)
    requirePayload receipt invocation comparison callbackProbe beforePointer beforeLength
    afterPointer <- atWord receipt (callExit observedCallback) (message+8)
    afterLength <- atWord receipt (callExit observedCallback) (message+4)
    requirePayload receipt invocation (callExit observedCallback) (callExit observedCallback) afterPointer afterLength
    (_,encoded) <- classEncoding receipt (callExit observedCallback) ident
    acquired <- service source receipt invocation 0x725100 selected 0x170
    out <- register (callEntry acquired) 1
    ensureArgs SendRejected (callEntry acquired) [encoded,out]
    acquisitionStatus <- status acquired
    if acquisitionStatus /= 0 then do
      absentSites receipt invocation [0x72514c,0x72517c,0x7251b4,0x7251f0]
      errorPath receipt invocation (if acquisitionStatus == negate 50 then negate 62 else acquisitionStatus)
    else do
      buffer <- atWord receipt (callExit acquired) out
      header <- sourceAt receipt invocation 0x72514c
      loadedBuffer <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725144) (K.Address out)
      kind <- number <$> readScalar SendRejected receipt invocation (K.Address 0x725148) (K.Address message) (S.ByteCount 2)
      field2 <- number <$> readScalar SendRejected receipt invocation (K.Address 0x725150) (K.Address (message+2)) (S.ByteCount 1)
      field3 <- number <$> readScalar SendRejected receipt invocation (K.Address 0x725158) (K.Address (message+3)) (S.ByteCount 1)
      headerLength <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725160) (K.Address (message+4))
      copyLength <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725168) (K.Address (message+4))
      require (loadedBuffer == buffer && probeId (callExit observedCallback) < probeId (callEntry acquired)
        && probeId (callExit acquired) < probeId header) SendRejected
      checkWrites SendRejected receipt invocation (map K.Address [0x72514c,0x725154,0x72515c,0x725164])
        [(K.Address (buffer+4),S.ByteCount 2,K.WordValue kind),(K.Address (buffer+6),S.ByteCount 1,K.WordValue field2)
        ,(K.Address (buffer+7),S.ByteCount 1,K.WordValue field3),(K.Address (buffer+8),S.ByteCount 4,K.WordValue headerLength)]
      let expectedHeader = B.concat [B.take 2 (wordBytes kind),B.pack [fromIntegral field2,fromIntegral field3],wordBytes headerLength]
      contextQuery <- service source receipt invocation 0x725190 selected 0x148
      returned <- returnOf receipt invocation
      when (copyLength == 0) $ absentSites receipt invocation [0x72517c]
      when (copyLength /= 0) $ do
        copy <- call receipt invocation 0x72517c 0x722054
        payload <- number <$> readOperand SendRejected receipt invocation (K.Address 0x725178) (K.Address (message+8))
        ensureArgs SendRejected (callEntry copy) [buffer+12,payload,copyLength]
        requirePayload receipt invocation (callExit observedCallback) (callEntry copy) payload copyLength
        _ <- checkCopy SendRejected receipt copy (buffer+12) payload copyLength
        requireLifetime receipt invocation Payload InvocationLifetime (callEntry copy) returned payload (fromIntegral copyLength)
        requireLifetime receipt invocation Buffer InvocationLifetime (callEntry copy) returned (buffer+12) (fromIntegral copyLength)
      query <- status contextQuery
      submitted <- service source receipt invocation 0x7251b4 selected (if query == 1 then 0xb0 else 0xac)
      ensureArgs SendRejected (callEntry submitted) [encoded,buffer]
      submissionStatus <- status submitted
      observedHeader <- atBytes receipt (callEntry submitted) (buffer+4) 8
      initialPrefix <- atBytes receipt (callExit acquired) buffer 4
      prefix <- atBytes receipt (callEntry submitted) buffer 4
      require (observedHeader == expectedHeader && prefix == initialPrefix) SendRejected
      requireLifetime receipt invocation Buffer InvocationLifetime header returned buffer 12
      if submissionStatus == 0 then absentSites receipt invocation [0x7251f0] >> ensureResult SendRejected receipt invocation 0 else do
        released <- service source receipt invocation 0x7251f0 selected 0xf8
        ensureArgs SendRejected (callEntry released) [encoded,buffer]
        errorPath receipt invocation submissionStatus

checkReceive :: S.SourceModel -> Receipt -> Invocation -> Either WitnessError ()
checkReceive source receipt invocation = do
  entry <- entryOf receipt invocation
  (ident,message,timeout) <- args3 entry
  completed <- returnOf receipt invocation
  requireLifetime receipt invocation Descriptor InvocationLifetime entry completed message 12
  _ <- required ReceiveRejected (S.taskId ident)
  selected <- identity source receipt invocation 0x725344
  (_,encoded) <- classEncoding receipt entry ident
  received <- service source receipt invocation 0x725378 selected 0xc8
  out <- register (callEntry received) 1
  ensureArgs ReceiveRejected (callEntry received) [encoded,out,timeout]
  receivedStatus <- status received
  if receivedStatus /= 0 then do
    absentSites receipt invocation [0x7253c8,0x7253f8,0x725414]
    if receivedStatus == negate 50 then ensureResult ReceiveRejected receipt invocation receivedStatus else errorPath receipt invocation receivedStatus
  else do
    buffer <- atWord receipt (callExit received) out
    pointer <- sourceAt receipt invocation 0x7253c0
    release <- service source receipt invocation 0x725414 selected 0xf8
    ensureArgs ReceiveRejected (callEntry release) [encoded,buffer]
    loadedBuffer <- number <$> readOperand ReceiveRejected receipt invocation (K.Address 0x7253c0) (K.Address out)
    kind <- number <$> readScalar ReceiveRejected receipt invocation (K.Address 0x7253c4) (K.Address (buffer+4)) (S.ByteCount 2)
    field2 <- number <$> readScalar ReceiveRejected receipt invocation (K.Address 0x7253cc) (K.Address (buffer+6)) (S.ByteCount 1)
    field3 <- number <$> readScalar ReceiveRejected receipt invocation (K.Address 0x7253d4) (K.Address (buffer+7)) (S.ByteCount 1)
    headerLength <- number <$> readOperand ReceiveRejected receipt invocation (K.Address 0x7253dc) (K.Address (buffer+8))
    lengthWord <- number <$> readOperand ReceiveRejected receipt invocation (K.Address 0x7253e4) (K.Address (buffer+8))
    require (loadedBuffer == buffer) ReceiveRejected
    checkWrites ReceiveRejected receipt invocation (map K.Address [0x7253c8,0x7253d0,0x7253d8,0x7253e0])
      [(K.Address message,S.ByteCount 2,K.WordValue kind),(K.Address (message+2),S.ByteCount 1,K.WordValue field2)
      ,(K.Address (message+3),S.ByteCount 1,K.WordValue field3),(K.Address (message+4),S.ByteCount 4,K.WordValue headerLength)]
    let expectedHeader = B.concat [B.take 2 (wordBytes kind),B.pack [fromIntegral field2,fromIntegral field3],wordBytes headerLength]
    initialHeader <- memoryAt receipt pointer
    let committedHeader = writeMemory initialHeader (physical message) expectedHeader
    finalHeader <- if lengthWord == 0 then do
      destination <- atWord receipt pointer (message+8)
      requirePayload receipt invocation (callExit received) pointer destination 0
      absentSites receipt invocation [0x7253f8]
      pure committedHeader
    else do
      copy <- call receipt invocation 0x7253f8 0x722054
      destination <- number <$> readOperand ReceiveRejected receipt invocation (K.Address 0x7253f0) (K.Address (message+8))
      ensureArgs ReceiveRejected (callEntry copy) [destination,buffer+12,lengthWord]
      requirePayload receipt invocation (callExit received) (callEntry copy) destination lengthWord
      writes <- checkCopy ReceiveRejected receipt copy destination (buffer+12) lengthWord
      requireLifetime receipt invocation Payload ReceiveReleaseLifetime (callEntry copy) (callExit release) destination (fromIntegral lengthWord)
      requireLifetime receipt invocation Buffer ReceiveReleaseLifetime (callEntry copy) (callExit release) (buffer+12) (fromIntegral lengthWord)
      foldM (\contents event -> do
        location <- eventPhysical32 event
        value <- eventBytes event
        pure (writeMemory contents location value)) committedHeader writes
    expectedCommitted <- bytesAt ReceiveRejected finalHeader (physical message) 8
    actualHeader <- atBytes receipt (callEntry release) message 8
    require (expectedCommitted == actualHeader) ReceiveRejected
    requireLifetime receipt invocation Buffer ReceiveReleaseLifetime pointer (callExit release) buffer 12
    ensureResult ReceiveRejected receipt invocation 0

checkCopy :: WitnessError -> Receipt -> Call -> Word32 -> Word32 -> Word32 -> Either WitnessError [MemoryEvent]
checkCopy failure receipt copied destination source size = walk 0
  [event | event <- allEvents receipt,eventCore event == Just (core (callEntry copied))
    ,Just instruction <- [eventPc event],address instruction > 0x722054,address instruction < 0x722170
    ,Just at <- [eventInstructions event],count at > probeClock (callEntry copied),count at <= probeClock (callExit copied)
    ,eventSource event `elem` [K.CpuRead,K.CpuWrite]]
  where
    largeAligned = size > 1024 && (source `xor` destination) .&. 3 == 0
    prefix = 4-(destination .&. 3)
    wordsEnd = prefix+((size-prefix) .&. 0xfffffffc)
    step offset
      | largeAligned && offset < prefix = (1,0x722080,0x722084)
      | largeAligned && offset < wordsEnd = (4,0x7220c4,0x7220c8)
      | largeAligned = (1,0x7220e8,0x7220ec)
      | (source .|. destination) .&. 1 == 0 && offset < size .&. 0xfffffffe = (2,0x722130,0x722134)
      | (source .|. destination) .&. 1 == 0 = (1,0x722150,0x722154)
      | otherwise = (1,0x72215c,0x722160)
    walk offset [] = require (offset == size) failure >> pure []
    walk offset (readEvent:writeEvent:rest) = do
      let (width,loadPc,storePc) = step offset
      require (offset < size && width <= size-offset
        && eventSource readEvent == K.CpuRead && eventSource writeEvent == K.CpuWrite
        && eventPc readEvent == Just (K.Address loadPc) && eventPc writeEvent == Just (K.Address storePc)
        && eventAddress readEvent == fromIntegral (source+offset)
        && eventAddress writeEvent == fromIntegral (destination+offset)
        && eventSize readEvent == fromIntegral width && eventSize writeEvent == fromIntegral width
        && eventValue readEvent == eventValue writeEvent) failure
      remaining <- walk (offset+width) rest
      pure (writeEvent:remaining)
    walk _ _ = Left failure

consumerProjection :: Receipt -> Either WitnessError ConsumerProjection
consumerProjection receipt = do
  let invocations = sortOn invocationId [invocation | invocation <- receiptInvocations receipt,invocationStatus invocation == Returned]
  projected <- mapM projectInvocation invocations
  resources <- mapM projectResource [resource | resource <- receiptResources receipt
    ,resourceKind resource `notElem` [Stack,Code,Header,Carrier,Descriptor]
    ,not (resourceKind resource `elem` [Output,Auxiliary] && privateResource resource)]
  pure (ConsumerProjection projected resources [(censusFamily item,censusCore item,censusEntries item) | item <- receiptCensus receipt])
  where
    contains resource location = location >= address (resourcePhysical resource)
      && toInteger location < toInteger (address (resourcePhysical resource))+toInteger (count (resourceSize resource))
    onStack location = any (\resource -> resourceKind resource == Stack && contains resource location) (receiptResources receipt)
    privateResource resource = onStack (address (resourcePhysical resource))
      && onStack (address (resourcePhysical resource)+fromIntegral (count (resourceSize resource))-1)
    pointer invocation value =
      let here = physical value
          locals = [resource | resource <- receiptResources receipt,resourceOwner resource == Just (invocationId invocation)
            ,resourceKind resource `elem` [Descriptor,Output,Auxiliary,Payload],contains resource here]
          rank resource = (if resourceKind resource == Descriptor then 0 :: Int else 1,resourceId resource)
          ordinal selected = length [resource | resource <- receiptResources receipt
            ,resourceOwner resource == resourceOwner selected,resourceKind resource == resourceKind selected
            ,resourceId resource < resourceId selected]
      in if onStack here then case sortOn rank locals of
        resource:_ -> LocalLocation (invocationFamily invocation) (resourceKind resource) (ordinal resource)
          (K.Count (fromIntegral (here-address (resourcePhysical resource))))
        [] -> FrameLocation (invocationFamily invocation) (toInteger here-toInteger (physical (address (invocationSp invocation))))
      else AbsoluteLocation (K.Address value)
    argument :: Invocation -> [Int] -> Int -> Word32 -> Argument
    argument invocation pointers index value = if index `elem` pointers then PointerArgument (pointer invocation value) else ScalarArgument (K.WordValue value)
    projectInvocation invocation = do
      entry <- entryOf receipt invocation
      returned <- returnOf receipt invocation
      input <- case invocationFamily invocation of
        TaskWrapper -> do
          descriptor <- register entry 0
          TaskInput . map K.WordValue <$> mapM (atWord receipt entry . (descriptor+)) [0,4,8,12,16,20,24]
        TaskReserve -> reserveInput invocation entry
        StorageReserve -> reserveInput invocation entry
        MessageConstruct -> do
          ident <- register entry 0
          descriptor <- register entry 1
          ConstructInput (K.WordValue ident) . map K.WordValue <$> mapM (atWord receipt entry . (descriptor+)) [0,4]
        MessageSend -> MessageInput <$> (K.WordValue <$> register entry 0) <*> projectMessage invocation entry <*> pure Nothing
        MessageReceive -> MessageInput <$> (K.WordValue <$> register entry 0) <*> projectMessage invocation entry <*> (Just . K.WordValue <$> register entry 2)
      result <- K.WordValue <$> register returned 0
      output <- case invocationFamily invocation of
        MessageReceive -> MessageOutput result <$> projectMessage invocation returned
        MessageSend -> MessageOutput result <$> projectMessage invocation returned
        MessageConstruct -> do
          descriptor <- register entry 1
          ConstructOutput result . map K.WordValue <$> mapM (atWord receipt returned . (descriptor+)) [0,4]
        _ -> pure (ScalarOutput result)
      calls <- mapM (projectCall invocation) [probe | probe <- invocationProbes receipt invocation,probeKind probe == SourceProbe,pc probe `elem` map (\(site,_,_) -> site) callSpecs]
      pure (InvocationProjection (invocationFamily invocation) (invocationCore invocation) input output calls)
    reserveInput invocation entry = do
      (index,size,out) <- args3 entry
      pure (ReserveInput (K.WordValue index) (K.WordValue size) (pointer invocation out))
    projectMessage invocation probe = do
      entry <- entryOf receipt invocation
      message <- register entry 1
      header <- K.WordValue <$> atWord receipt probe message
      size <- K.WordValue <$> atWord receipt probe (message+4)
      payload <- pointer invocation <$> atWord receipt probe (message+8)
      pure (MessageProjection header size payload)
    projectCall invocation probe = do
      (_,arity,parameterWords) <- one CallRejected [spec | spec@(site,_,_) <- callSpecs,site == pc probe]
      returned <- sourceAt receipt invocation (pc probe+4)
      arguments <- args probe arity
      parameters <- if parameterWords == 0 then pure [] else do
        parameterPointer <- register probe 1
        mapM (atWord receipt probe . (parameterPointer+)) [4*index | index <- [0..parameterWords-1]]
      result <- if pc probe `elem` [0x725f68,0x725f78,0x725fb4,0x7250cc,0x72517c,0x7253f8,0x7233ac]
        then pure Nothing else Just . K.WordValue <$> register returned 0
      target <- one CallRejected [targetProbe | targetProbe <- invocationProbes receipt invocation,probeKind targetProbe == DynamicTarget
        ,lr targetProbe == pc probe+4,probeId probe < probeId targetProbe,probeId targetProbe < probeId returned]
      let pointerArgs = case pc probe of
            0x725cc0 -> [0]; 0x7234c4 -> [0]; 0x725e74 -> [2]; 0x725f08 -> [1]
            0x725f68 -> [0]; 0x725f78 -> [0]; 0x725fa8 -> [1]
            0x724d20 -> [2]; 0x724d98 -> [1]; 0x724e34 -> [2]; 0x724ea4 -> [1]
            0x7250cc -> [0]; 0x725100 -> [1]; 0x72517c -> [0,1]; 0x7251b4 -> [1]; 0x7251f0 -> [1]
            0x725378 -> [1]; 0x7253f8 -> [0,1]; 0x725414 -> [1]; _ -> []
          pointerParams = case pc probe of 0x725fa8 -> [5]; 0x724d98 -> [2]; 0x724ea4 -> [3]; _ -> []
      pure (CallProjection (K.Address (pc probe)) (K.Address (pc target))
        (zipWith (argument invocation pointerArgs) [0..] arguments)
        (zipWith (argument invocation pointerParams) [0..] parameters) result)
    resourcePointer resource value = case resourceOwner resource of
      Nothing -> pure (AbsoluteLocation (K.Address value))
      Just owner -> do
        invocation <- one ComparisonRejected [candidate | candidate <- receiptInvocations receipt,invocationId candidate == owner]
        pure (pointer invocation value)
    projectResource resource = do
      final <- checkViews resource (resourceFinal resource)
      bytes <- bytesAt ViewsRejected final (address (resourcePhysical resource)) (count (resourceSize resource))
      initial <- checkViews resource (resourceInitial resource)
      before <- bytesAt ViewsRejected initial (address (resourcePhysical resource)) (count (resourceSize resource))
      events <- resourceEvents receipt resource
      installed <- probeAt receipt InstallerEntry
      installationReturn <- probeAt receipt InstallerReturn
      writes <- mapM (projectWrite resource (probeClock installed) (probeClock installationReturn)) (filter isWrite events)
      location <- resourcePointer resource (address (resourcePhysical resource))
      pure (ResourceProjection (resourceKind resource) (resourceLifetime resource) location (Bytes before) (Bytes bytes) writes)
    projectWrite resource installationBegin installationEnd event = do
      at <- eventClock event
      location <- resourcePointer resource (fromIntegral (eventAddress event))
      value <- Bytes <$> eventBytes event
      let installation = at > installationBegin && at <= installationEnd
          phase = if installation then InstallationWrites else ConsumerWrites
      pure (ComparableWrite phase (eventSource event) (eventCore event) (if installation then Nothing else eventPc event)
        location (K.Count (eventSize event)) (eventBefore event) value)

consumerProjectionValue :: ConsumerProjection -> J.Value
consumerProjectionValue (ConsumerProjection invocations resources census) = J.object
  ["invocations" J..= map invocationValue invocations
  ,"census" J..= [J.object ["family" J..= show family,"core" J..= show owner,"entries" J..= count entries] | (family,owner,entries) <- census]
  ,"resources" J..= [J.object ["kind" J..= show kind,"lifetime" J..= show lifetime,"location" J..= pointerValue location
    ,"before_bytes" J..= B.length before,"after_bytes" J..= B.length after,"writes" J..= length writes]
    | ResourceProjection kind lifetime location (Bytes before) (Bytes after) writes <- resources]]
  where
    invocationValue (InvocationProjection family owner input output calls) = J.object
      ["family" J..= show family,"core" J..= show owner,"input" J..= inputValue input,"output" J..= outputValue output
      ,"calls" J..= [J.object ["site" J..= address site,"target" J..= address target
        ,"arguments" J..= map argumentValue arguments,"parameters" J..= map argumentValue parameters
        ,"result" J..= fmap number result] | CallProjection site target arguments parameters result <- calls]]
    inputValue input = case input of
      TaskInput values -> J.object ["descriptor" J..= map number values]
      ReserveInput index size out -> J.object ["index" J..= number index,"size" J..= number size,"out" J..= pointerValue out]
      ConstructInput ident values -> J.object ["id" J..= number ident,"descriptor" J..= map number values]
      MessageInput ident message timeout -> J.object ["id" J..= number ident,"message" J..= messageValue message,"timeout" J..= fmap number timeout]
    outputValue output = case output of
      ScalarOutput result -> J.toJSON (number result)
      ConstructOutput result values -> J.object ["result" J..= number result,"descriptor" J..= map number values]
      MessageOutput result message -> J.object ["result" J..= number result,"message" J..= messageValue message]
    messageValue (MessageProjection header size payload) = J.object ["header" J..= number header,"size" J..= number size,"payload" J..= pointerValue payload]
    argumentValue value = case value of ScalarArgument word -> J.toJSON (number word); PointerArgument location -> pointerValue location
    pointerValue location = case location of
      AbsoluteLocation value -> J.toJSON (address value)
      LocalLocation family kind ordinal offset -> J.object ["family" J..= show family,"local" J..= show kind,"ordinal" J..= ordinal,"offset" J..= count offset]
      FrameLocation family offset -> J.object ["family" J..= show family,"frame_offset" J..= offset]

callSpecs :: [(Word32,Int,Word32)]
callSpecs =
  [(0x725c94,0,0),(0x725ca4,0,0),(0x725cc0,2,0),(0x725dac,1,0),(0x725dc4,1,0)
  ,(0x723498,0,0),(0x7234a8,0,0),(0x7234c4,2,0),(0x7235ac,1,0),(0x7235c4,1,0)
  ,(0x725e74,3,0),(0x725f08,2,2),(0x725f68,3,0),(0x725f78,2,0),(0x725fa8,2,6),(0x725fb4,1,0),(0x726000,0,0),(0x726024,1,0)
  ,(0x724d20,3,0),(0x724d98,2,3),(0x724e34,3,0),(0x724ea4,2,4),(0x724ee4,1,0)
  ,(0x725064,0,0),(0x7250cc,1,0),(0x725100,2,0),(0x72517c,3,0),(0x725190,0,0),(0x7251b4,2,0),(0x7251f0,2,0)
  ,(0x725344,0,0),(0x725378,3,0),(0x7253f8,3,0),(0x725414,2,0),(0x7233ac,1,0)]
