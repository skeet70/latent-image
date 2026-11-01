module Emulation.TaskSubmissionControlWitness
  ( ControlError(..), ControlProof, ControlEndpoint(..), qualifyControl, controlCase, controlEndpoint ) where

import Control.Monad (foldM, unless)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import qualified Emulation.Sha256 as SHA
import Emulation.StartupTaskWire
import Emulation.TaskSubmissionControlWire
import qualified Replacement.KernelInstall.Wire as W

data ControlError = ControlScopeRejected | ControlPhaseRejected | ControlContextRejected
  | ControlRegisterMutationRejected | ControlPatchRejected | ControlHostWriteRejected
  | ControlSnapshotRejected | ControlAliasRejected | ControlReplayRejected | ControlSequenceRejected
  | ControlArgumentsRejected | ControlStatusRejected | ControlIrqRejected | ControlDescriptorRejected
  | ControlStorageRejected | ControlCreationRejected | ControlAuxiliaryRejected | ControlRunnableRejected
  | ControlErrorEffectRejected | ControlReleaseRejected | ControlCallbackRejected
  deriving (Eq, Show)
data ControlEndpoint = ReturnedEndpoint | ReleaseReturnedEndpoint | CallbackPrologueEndpoint deriving (Eq, Show)
data ControlProof = ControlProof ProbeCase ControlEndpoint deriving (Eq, Show)
controlCase :: ControlProof -> ProbeCase
controlCase (ControlProof kind _) = kind
controlEndpoint :: ControlProof -> ControlEndpoint
controlEndpoint (ControlProof _ endpoint) = endpoint

ensure :: ControlError -> Bool -> Either ControlError ()
ensure problem condition = unless condition (Left problem)
need :: ControlError -> Maybe a -> Either ControlError a
need problem = maybe (Left problem) Right
unique :: ControlError -> [a] -> Either ControlError a
unique _ [item] = Right item
unique problem _ = Left problem
addr :: W.Address -> Word32
addr (W.Address value) = value
word :: W.WordValue -> Word32
word (W.WordValue value) = value
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff
contents :: W.Bytes -> B.ByteString
contents (W.Bytes value) = value
hitNumber :: HitIndex -> Int
hitNumber (HitIndex value) = value
cursorNumber :: Cursor -> Int
cursorNumber (Cursor value) = value
state :: Sample -> W.Context
state = contextState . sampleContext
reg :: Int -> W.Context -> Either ControlError Word32
reg index context = word <$> need ControlContextRejected (case drop index (W.contextRegisters context) of value:_ | index >= 0 -> Just value; _ -> Nothing)
sampleAt :: Receipt -> HitIndex -> Either ControlError Sample
sampleAt receipt index = unique ControlSnapshotRejected (filter ((==index) . sampleHit) (receiptSamples receipt))
kernelAt :: Sample -> KernelCore -> Either ControlError Kernel
kernelAt sample core = unique ControlSnapshotRejected (filter ((==core) . kernelCore) (sampleKernels sample))
views :: Sample -> [View]
views sample = sampleLocal sample ++ concatMap kernelViews (sampleKernels sample)
type Memory = M.Map Word32 Word8
readBytes :: ControlError -> Memory -> Word32 -> Int -> Either ControlError B.ByteString
readBytes problem memory address size = B.pack <$> mapM (\offset -> need problem (M.lookup (physical address+fromIntegral offset) memory)) [0..size-1]
readWord :: ControlError -> Memory -> Word32 -> Either ControlError Word32
readWord problem memory address = do
  raw <- readBytes problem memory address 4
  pure (sum [fromIntegral byte `shiftL` (8*offset) | (offset,byte) <- zip [0..] (B.unpack raw)])
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` amount) | amount <- [0,8,16,24]]
putBytes :: Word32 -> B.ByteString -> Memory -> Memory
putBytes start raw memory = foldr (uncurry M.insert) memory (zip [physical start..] (B.unpack raw))
putWord :: Word32 -> Word32 -> Memory -> Memory
putWord start value = putBytes start (wordBytes value)
merge :: Memory -> View -> Either ControlError Memory
merge memory view = do
  let start = addr (viewPhysical view); raw = contents (viewBytes view)
      digest = W.Digest (SHA.sha256Hex (SHA.sha256 raw))
  ensure ControlAliasRejected (not (B.null raw) && start < 0x20000000
    && toInteger start+toInteger (B.length raw) <= 0x20000000 && viewAliases view == replicate 2 (replicate 2 digest))
  mapM_ (\(address,byte) -> ensure ControlSnapshotRejected (maybe True (==byte) (M.lookup address memory))) (zip [start..] (B.unpack raw))
  pure (putBytes start raw memory)
memoryAt :: Sample -> Either ControlError Memory
memoryAt = foldM merge M.empty . views
sameRegion :: ControlError -> Memory -> Memory -> Word32 -> Int -> Either ControlError ()
sameRegion problem before after start size = do
  left <- readBytes problem before start size
  right <- readBytes problem after start size
  ensure problem (left == right)
wordsAt :: ControlError -> Memory -> Word32 -> Int -> Either ControlError [Word32]
wordsAt problem memory start count = mapM (readWord problem memory . (start+) . (4*) . fromIntegral) [0..count-1]

isRelease :: ProbeCase -> Bool
isRelease kind = kind `elem` [ReleaseUnlocked,ReleasePeerLock,ReleaseUnlockRefusal]
isFresh :: ProbeCase -> Bool
isFresh kind = kind `elem` [FreshEnabled,FreshMasked,SentinelFresh]
isRefusal :: ProbeCase -> Bool
isRefusal kind = kind `elem` [ReservedRefusal,CallbackUnreturned]
expectedStatus :: ProbeCase -> Word32
expectedStatus kind = case kind of
  FreshMasked -> 0xffffffe7
  ReusedRecord -> 0xffffffd7
  PartialPair -> 0xffffffd7
  ReservedRefusal -> 0xffffffee
  CallbackUnreturned -> 0xffffffee
  _ -> 0

checkProtocol :: Receipt -> Either ControlError ()
checkProtocol receipt = ensure ControlScopeRejected (receiptSchema receipt == TaskSubmissionProtocol
  && case (receiptScope receipt,receiptProgram receipt) of
    (StockStartupScope,StockProgram) -> True
    (StockProbeScope,StockProgram) -> True
    (OwnedStartupScope,OwnedProgram _ _ _) -> True
    (OwnedProbeScope,OwnedProgram _ _ _) -> True
    _ -> False)

checkPhase :: Receipt -> ProbeRecord -> Either ControlError Sample
checkPhase receipt probe = do
  initial <- sampleAt receipt (probeInitialHit probe)
  ensure ControlScopeRejected (receiptScope receipt `elem` [StockProbeScope,OwnedProbeScope])
  ensure ControlPhaseRejected (sort (map submissionId (receiptSubmissions receipt)) == map TaskId [93,102,103,104]
    && all (maybe False (<probeInitialHit probe) . submissionReturnHit) (receiptSubmissions receipt)
    && length (receiptFirstTasks receipt) == 2
    && all (maybe False (<=probeInitialHit probe) . firstEnteredHit) (receiptFirstTasks receipt))
  let before = contextState (probeBefore probe)
  root <- unique ControlPhaseRejected (filter ((==PrimaryKernel) . firstCore) (receiptFirstTasks receipt))
  ensure ControlPhaseRejected (firstId root == TaskId 104 && contextCore (probeBefore probe) == PrimaryKernel
    && sampleContext initial == probeBefore probe && W.contextPc before == W.Address 0x69d164
    && addr (firstStackTop root) >= 16 && W.contextSp before == W.Address (addr (firstStackTop root)-16)
    && W.contextCpsr before == W.WordValue 0x53
    && probeDescriptor probe == W.Address 0x40063424 && probeReturnPc probe == W.Address 0x69d164
    && probeTarget probe == (if isRelease (probeCase probe) then DeletionTarget else SubmissionTarget)
    && probeTask probe == TaskId (if isFresh (probeCase probe) then 1 else 102))
  pure initial

checkRegisters :: ProbeRecord -> Either ControlError ()
checkRegisters probe = do
  let before = contextState (probeBefore probe); after = contextState (probeConfigured probe)
      targetR0 = if isRelease (probeCase probe) then 102 else addr (probeDescriptor probe)
      targetPc = if isRelease (probeCase probe) then 0x72612c else 0x725dd8
      targetCpsr = (word (W.contextCpsr before) .&. complement 0x80) .|. (if probeCase probe == FreshMasked then 0x80 else 0)
  oldR0 <- reg 0 before
  let edits = [(ArgumentRegister,oldR0,targetR0),(LinkRegister,addr (W.contextLr before),addr (probeReturnPc probe))
        ,(ProgramCounter,addr (W.contextPc before),targetPc),(StatusRegister,word (W.contextCpsr before),targetCpsr)]
      expected = [RegisterChange name (W.WordValue old) (W.WordValue new) | (name,old,new) <- edits,old /= new]
      configured = before { W.contextRegisters = W.WordValue targetR0 : drop 1 (W.contextRegisters before)
        ,W.contextLr = probeReturnPc probe,W.contextPc = W.Address targetPc,W.contextCpsr = W.WordValue targetCpsr }
  ensure ControlRegisterMutationRejected (length (W.contextRegisters before) == 13 && probeRegisterChanges probe == expected
    && contextCore (probeConfigured probe) == PrimaryKernel && after == configured)

patchStage :: Receipt -> ProbeRecord -> ControlPatch -> Either ControlError ProbeStage
patchStage receipt probe patch
  | patchAfterHit patch == probeInitialHit probe = Right InitialStage
  | Just (patchAfterHit patch) == probeReleaseEntryHit probe = Right ReleaseEntryStage
  | otherwise = do
      sample <- sampleAt receipt (patchAfterHit patch)
      ensure ControlPatchRejected (sampleBoundary sample == UnlockCall && sampleCallTaken sample /= Just False && isRelease (probeCase probe))
      pure UnlockCallStage

checkPatches :: Receipt -> W.Watch -> ProbeRecord -> Sample -> Either ControlError ()
checkPatches receipt watch probe initial = do
  memory <- memoryAt initial
  let kind = probeCase probe; initialHit = probeInitialHit probe
      descriptor = physical (addr (probeDescriptor probe))
      fresh = zipWith (\offset value -> (InitialStage,descriptor+offset,value)) [0,4..24]
        [1,0x781968,15,1024,0x69cbf0,0,if kind == SentinelFresh then 0xa5f0a5f0 else 1]
      initialEdits = (if isFresh kind then fresh else [])
        ++ (if isRefusal kind then [(InitialStage,0xa5b204+8*101,0),(InitialStage,0xa5b208+8*101,0)] else [])
        ++ (if kind == PartialPair then [(InitialStage,0xa5b208+8*101,0)] else [])
        ++ (if kind == CallbackUnreturned then [(InitialStage,0x89ea8c,0x69cbf0)] else [])
      laterEdits = case kind of ReleasePeerLock -> [(ReleaseEntryStage,0x9c280c,2)]
                               ReleaseUnlockRefusal -> [(UnlockCallStage,0x9c280c,0)]
                               _ -> []
  (_,expectedInitial) <- foldM (\(current,edits) edit@(_,address,value) -> do
    old <- readWord ControlPatchRejected current address
    pure (putWord address value current,edits ++ [edit | old /= value])) (memory,[]) initialEdits
  actual <- mapM (\patch -> do
    stage <- patchStage receipt probe patch
    let before = patchBefore patch; after = patchAfter patch; address = physical (addr (patchAddress patch))
    ensure ControlPatchRejected (addr (patchAddress patch) .&. 3 == 0 && viewObject before == ControlPatch && viewObject after == ControlPatch
      && viewPhysical before == W.Address address && viewPhysical after == W.Address address
      && B.length (contents (viewBytes before)) == 4 && B.length (contents (viewBytes after)) == 4 && viewBytes before /= viewBytes after)
    _ <- merge M.empty before; afterMemory <- merge M.empty after
    value <- readWord ControlPatchRejected afterMemory address
    anchor <- sampleAt receipt (patchAfterHit patch)
    ensure ControlPatchRejected (patchWatchCursor patch >= sampleCursor anchor && (stage /= InitialStage || patchAfterHit patch == initialHit))
    let slot = cursorNumber (patchWatchCursor patch)
    event <- need ControlHostWriteRejected (case drop slot (W.watchRecords watch) of row:_ | slot >= 0 -> Just row; _ -> Nothing)
    ensure ControlHostWriteRejected (W.eventSource event == W.HostWrite && W.eventSize event == 4
      && physical (fromIntegral (W.eventAddress event)) == address && W.eventAddress event == fromIntegral (addr (patchAddress patch))
      && W.eventPhysical event == Just (fromIntegral address)
      && maybe True (==fromIntegral value) (W.eventValue event)
      && maybe True (==viewBytes before) (W.eventBefore event)
      && maybe True (==W.Little) (W.eventByteOrder event))
    ensure ControlPatchRejected (all ((==W.HostWrite) . W.eventSource)
      (take (slot-cursorNumber (sampleCursor anchor)) (drop (cursorNumber (sampleCursor anchor)) (W.watchRecords watch))))
    pure (stage,address,value)) (probePatches probe)
  ensure ControlPatchRejected (actual == expectedInitial ++ laterEdits
    && strictlyIncreasing (map (cursorNumber . patchWatchCursor) (probePatches probe)))
  let declared = map (cursorNumber . patchWatchCursor) (probePatches probe)
      suffix = drop (cursorNumber (sampleCursor initial)) (zip [0..] (W.watchRecords watch))
  ensure ControlHostWriteRejected ([index | (index,event) <- suffix,W.eventSource event == W.HostWrite] == declared
    && all ((/=W.ModelBusWrite) . W.eventSource . snd) suffix)
  where strictlyIncreasing values = and (zipWith (<) values (drop 1 values))

replaySuffix :: Receipt -> W.Watch -> ProbeRecord -> Sample -> Either ControlError ()
replaySuffix receipt watch probe initial = do
  memory <- memoryAt initial
  let samples = filter ((>probeInitialHit probe) . sampleHit) (receiptSamples receipt)
  ensure ControlSnapshotRejected (not (null samples) && map sampleHit samples == sort (map sampleHit samples)
    && length (map sampleHit samples) == M.size (M.fromList [(sampleHit sample,()) | sample <- samples]))
  (_,lastCursor) <- foldM step (memory,sampleCursor initial) samples
  ensure ControlReplayRejected (cursorNumber lastCursor == length (W.watchRecords watch))
  where
    step (memory,previous) sample = do
      let start = cursorNumber previous; end = cursorNumber (sampleCursor sample)
      ensure ControlReplayRejected (end >= start && end <= length (W.watchRecords watch))
      next <- foldM apply memory (zip [start..] (take (end-start) (drop start (W.watchRecords watch))))
      merged <- foldM merge next (views sample)
      pure (merged,sampleCursor sample)
    apply memory (index,event) = case W.eventSource event of
      W.HostWrite -> do
        patch <- unique ControlHostWriteRejected (filter ((==Cursor index) . patchWatchCursor) (probePatches probe))
        let start = physical (addr (patchAddress patch))
        old <- readBytes ControlReplayRejected memory start 4
        ensure ControlReplayRejected (old == contents (viewBytes (patchBefore patch)))
        pure (putBytes start (contents (viewBytes (patchAfter patch))) memory)
      W.CpuWrite -> do
        target <- need ControlReplayRejected (W.eventPhysical event)
        before <- contents <$> need ControlReplayRejected (W.eventBefore event)
        value <- need ControlReplayRejected (W.eventValue event)
        let size = fromIntegral (W.eventSize event)
        ensure ControlReplayRejected (W.eventCore event `elem` [Just W.Primary,Just W.Secondary] && W.eventSize event `elem` [1,2,4,8]
          && target < 0x20000000 && B.length before == size && W.eventByteOrder event == Just W.Little
          && physical (fromIntegral (W.eventAddress event)) == fromIntegral target
          && (size == 8 || toInteger value < 1 `shiftL` (8*size)))
        mapM_ (\(offset,byte) -> ensure ControlReplayRejected (maybe True (==byte) (M.lookup (fromIntegral target+offset) memory))) (zip [0..] (B.unpack before))
        pure (putBytes (fromIntegral target) (B.pack [fromIntegral (value `shiftR` (8*offset)) | offset <- [0..size-1]]) memory)
      W.ModelBusWrite -> Left ControlHostWriteRejected
      W.CpuFetch -> pure memory
      _ -> Left ControlReplayRejected

one :: Boundary -> [Sample] -> Either ControlError Sample
one kind = unique ControlSequenceRejected . filter ((==kind) . sampleBoundary)
arguments :: Sample -> [Word32] -> Either ControlError ()
arguments sample expected = do
  actual <- mapM (\index -> reg index (state sample)) [0..length expected-1]
  ensure ControlArgumentsRejected (actual == expected)
parametersAt :: Sample -> Word32 -> Int -> Either ControlError [Word32]
parametersAt sample pointer count = do
  _ <- unique ControlArgumentsRejected (filter (\view -> viewObject view == Parameters
    && viewPhysical view == W.Address (physical pointer) && B.length (contents (viewBytes view)) == 4*count) (sampleLocal sample))
  memory <- memoryAt sample
  wordsAt ControlArgumentsRejected memory pointer count
statusAt :: Sample -> Word32 -> Either ControlError ()
statusAt sample expected = reg 0 (state sample) >>= ensure ControlStatusRejected . (==expected)
irq :: W.Context -> Word32
irq context = word (W.contextCpsr context) .&. 0x80
restored :: Bool -> W.Context -> W.Context -> Either ControlError ()
restored preserveR3 before after = ensure ControlContextRejected
  (W.contextSp after == W.contextSp before && irq after == irq before
    && take 8 (drop 4 (W.contextRegisters after)) == take 8 (drop 4 (W.contextRegisters before))
    && (not preserveR3 || take 1 (drop 3 (W.contextRegisters after)) == take 1 (drop 3 (W.contextRegisters before))))
configuredEntry :: Context -> Context -> Bool
configuredEntry configured actual = contextCore configured == PrimaryKernel && contextCore actual == PrimaryKernel
  && case (W.contextCounts before,W.contextCounts after) of
    ([a,b],[c,d]) -> a == c && b <= d && after == before {W.contextCounts=W.contextCounts after}
    _ -> False
  where before = contextState configured; after = contextState actual

submissionTrace :: ProbeRecord -> [Sample] -> Either ControlError ()
submissionTrace probe samples = do
  let kind = probeCase probe
      common = [SubmissionEntry,CriticalEnterCall,CriticalEnterReturn]
      allocation = if isFresh kind || isRefusal kind then [ReservationCall,ReservationReturn] else []
      creation = if isRefusal kind then [] else [AuxiliaryCall,AuxiliaryReturn,FillCall,FillReturn,CacheCall,CacheReturn,CreationCall,CreationReturn]
      activate = if isFresh kind then [IdentityCall,IdentityReturn,ActivationCall,ActivationReturn] else []
      errors = if expectedStatus kind /= 0 then [LogCall,LogReturn,ErrorCallbackCall]
        ++ (if kind == CallbackUnreturned then [] else [ErrorCallbackReturn]) else []
      ending = if kind == CallbackUnreturned then [] else [EnteredInstruction]
      expected = common ++ allocation ++ creation ++ [CriticalExitCall,CriticalExitReturn] ++ activate ++ errors ++ ending
      outer = outerCalls (filter (\sample -> Just (sampleHit sample) /= probeCallbackEntryHit probe
        && Just (sampleHit sample) /= probeCallbackEnteredHit probe && sampleBoundary sample /= NativeWrite) samples)
  ensure ControlSequenceRejected (map sampleBoundary outer == expected)
  where
    pairs = [(CriticalEnterCall,CriticalEnterReturn),(CriticalExitCall,CriticalExitReturn)
      ,(ReservationCall,ReservationReturn),(AuxiliaryCall,AuxiliaryReturn),(FillCall,FillReturn)
      ,(CacheCall,CacheReturn),(CreationCall,CreationReturn),(ActivationCall,ActivationReturn)
      ,(LogCall,LogReturn),(ErrorCallbackCall,ErrorCallbackReturn)]
    outerCalls [] = []
    outerCalls (sample:rest) = sample : case lookup (sampleBoundary sample) pairs of
      Nothing -> outerCalls rest
      Just returned -> case break ((==returned) . sampleBoundary) rest of
        (_,finish:later) -> finish : outerCalls later
        _ -> outerCalls rest

checkSubmission :: Receipt -> W.Watch -> ProbeRecord -> Sample -> [Sample] -> Either ControlError ()
checkSubmission receipt watch probe initial samples = do
  submissionTrace probe samples
  entryIndex <- need ControlSequenceRejected (probeEntryHit probe)
  incoming <- sampleAt receipt entryIndex
  ensure ControlContextRejected (sampleBoundary incoming == SubmissionEntry && configuredEntry (probeConfigured probe) (sampleContext incoming)
    && probeDeleteEntryHit probe == Nothing && probeReleaseEntryHit probe == Nothing && probeReleaseReturnHit probe == Nothing)
  before <- memoryAt incoming
  natural <- memoryAt initial
  let kind = probeCase probe; TaskId number = probeTask probe; i = fromIntegral number-1
      wrapper = 0xa5b204+8*i; storage = 0xa0fc44+12*i
  descriptor <- wordsAt ControlDescriptorRejected before 0x63424 7
  if isFresh kind then ensure ControlDescriptorRejected (descriptor == [1,0x781968,15,1024,0x69cbf0,0,if kind == SentinelFresh then 0xa5f0a5f0 else 1])
    else do original <- wordsAt ControlDescriptorRejected natural 0x63424 7
            ensure ControlDescriptorRejected (descriptor == original && take 6 descriptor == [102,0x781968,15,1024,0x69cbf0,0])
  end <- if kind == CallbackUnreturned then need ControlCallbackRejected (probeCallbackEnteredHit probe) >>= sampleAt receipt
    else need ControlSequenceRejected (probeReturnHit probe) >>= sampleAt receipt
  after <- memoryAt end
  sameRegion ControlDescriptorRejected before after 0x63424 28
  sameRegion ControlStorageRejected before after 0x89ea8c 4
  ensure ControlSequenceRejected (sampleHit incoming > sampleHit initial && sampleHit end == sampleHit (last samples))
  if kind == CallbackUnreturned then checkCallback receipt watch probe samples
    else do
      ensure ControlContextRejected (W.contextPc (state end) == probeReturnPc probe && sampleBoundary end == EnteredInstruction
        && probeCallbackEntryHit probe == Nothing && probeCallbackEnteredHit probe == Nothing)
      restored False (state incoming) (state end)
      statusAt end (expectedStatus kind)
  let startCursor = cursorNumber (sampleCursor incoming); endCursor = cursorNumber (sampleCursor end)
      writes = filter (\event -> W.eventSource event == W.CpuWrite && W.eventCore event == Just W.Primary)
        (take (endCursor-startCursor) (drop startCursor (W.watchRecords watch)))
      allowedSlot event = case W.eventPhysical event of
        Just target
          | target >= 0xa5b204 && target < 0xa5b5c4 -> target >= fromIntegral wrapper
              && target+W.eventSize event <= fromIntegral wrapper+8
          | target >= 0xa0fc44 && target < 0xa101e4 -> target >= fromIntegral storage
              && target+W.eventSize event <= fromIntegral storage+12
        _ -> True
  ensure ControlStorageRejected (all allowedSlot writes)
  sameRegion ControlStorageRejected before after 0x9c280c 4
  mapM_ (\kernel -> mapM_ (\view -> sameRegion ControlRunnableRejected before after
    (addr (viewPhysical view)) (B.length (contents (viewBytes view))))
      (filter (\view -> viewObject view `elem` [Domains,Control,QueueHeads,Bitmap,SelectedObject,Current]) (kernelViews kernel)))
    (filter (\kernel -> kernelCore kernel == PrimaryKernel && kind `notElem` [FreshEnabled,SentinelFresh]) (sampleKernels incoming))
  enter <- one CriticalEnterCall samples; entered <- one CriticalEnterReturn samples
  exit <- one CriticalExitCall samples; exited <- one CriticalExitReturn samples
  token <- reg 0 (state entered)
  arguments exit [token]
  ensure ControlIrqRejected (irq (state enter) == irq (state incoming) && irq (state entered) == 0x80
    && irq (state exit) == 0x80 && irq (state exited) == irq (state incoming))
  if isFresh kind || isRefusal kind then do
    call <- one ReservationCall samples; returned <- one ReservationReturn samples
    arguments call [i,1024,wrapper .|. 0x40000000]
    statusAt returned (if isRefusal kind then 0xffffffee else 0)
    callMemory <- memoryAt call
    readWord ControlStorageRejected callMemory (wrapper+4) >>= ensure ControlStorageRejected . (==1024)
    ensure ControlIrqRejected (irq (state call) == 0x80 && irq (state returned) == 0x80)
    else pure ()
  if isRefusal kind then do
    sameRegion ControlStorageRejected before after 0x89ea98 4
    unchangedKernelRecords before after
    oldStorage <- wordsAt ControlStorageRejected before storage 3
    newStorage <- wordsAt ControlStorageRejected after storage 3
    ensure ControlStorageRejected (case oldStorage of
      [base,1024,1] -> base >= 0xa10200 && toInteger base+1024 <= 0xa5b200 && oldStorage == newStorage
      _ -> False)
    pair <- wordsAt ControlStorageRejected after wrapper 2
    ensure ControlStorageRejected (pair == [0,1024])
    else checkCreated probe before after samples
  checkErrorEffects probe before after samples
  where
    unchangedKernelRecords before after = mapM_ (\(start,size) -> sameRegion ControlCreationRejected before after start size)
      [(0x5beac,4800),(0x5ff0c,6720),(0x5d178,1440),(0x624b4,1920)]

checkCreated :: ProbeRecord -> Memory -> Memory -> [Sample] -> Either ControlError ()
checkCreated probe before after samples = do
  let kind = probeCase probe; TaskId number = probeTask probe; i = fromIntegral number-1
      wrapper = 0xa5b204+8*i; storage = 0xa0fc44+12*i
      c = 0x5beac+40*i; t = 0x5ff0c+56*i; aux = 0x5d178+12*i; auxNode = 0x624b4+16*i
      encoded = 0x10000 .|. fromIntegral number
  readWord ControlArgumentsRejected before (0x7cad9c+4*i) >>= ensure ControlArgumentsRejected . (==1)
  createdBefore <- readWord ControlCreationRejected before (c+36)
  auxiliaryBefore <- readWord ControlAuxiliaryRejected before (aux+8)
  ensure ControlCreationRejected (createdBefore == (if isFresh kind then 0 else 1))
  ensure ControlAuxiliaryRejected (auxiliaryBefore == (if isFresh kind then 0 else 1))
  pair <- wordsAt ControlStorageRejected after wrapper 2
  (base,size) <- case pair of [base,size] -> pure (base,size); _ -> Left ControlStorageRejected
  ensure ControlStorageRejected (size == (if kind == PartialPair then 0 else 1024) && base >= 0xa10200 && toInteger base+1024 <= 0xa5b200)
  auxiliaryCall <- one AuxiliaryCall samples; auxiliaryReturn <- one AuxiliaryReturn samples
  arguments auxiliaryCall [encoded]
  auxiliaryPointer <- reg 1 (state auxiliaryCall)
  auxiliaryParameters <- parametersAt auxiliaryCall auxiliaryPointer 2
  ensure ControlArgumentsRejected (auxiliaryParameters == [if kind == SentinelFresh then 0 else 4,0])
  statusAt auxiliaryReturn (if isFresh kind then 0 else 0xffffffd7)
  creationCall <- one CreationCall samples; creationReturn <- one CreationReturn samples
  fill <- one FillCall samples; filled <- one FillReturn samples; cache <- one CacheCall samples
  arguments fill [base,fromIntegral number .&. 0xff,size]
  arguments cache [physical base,size]
  arguments creationCall [encoded]
  pointer <- reg 1 (state creationCall)
  parameters <- parametersAt creationCall pointer 6
  ensure ControlArgumentsRejected (parameters == [2,0,0x69cbf0,15,size,base])
  statusAt creationReturn (if isFresh kind then 0 else 0xffffffd7)
  filledMemory <- memoryAt filled
  if size == 0 then sameRegion ControlStorageRejected before after base 1024
    else readBytes ControlStorageRejected filledMemory base 1024 >>= ensure ControlStorageRejected . (==B.replicate 1024 (fromIntegral number))
  if isFresh kind then do
    oldPair <- wordsAt ControlStorageRejected before wrapper 2
    oldStorage <- wordsAt ControlStorageRejected before storage 3
    cursor <- readWord ControlStorageRejected before 0x89ea98
    laterCursor <- readWord ControlStorageRejected after 0x89ea98
    reservation <- wordsAt ControlStorageRejected after storage 3
    ensure ControlStorageRejected (oldPair == [0,0] && oldStorage == [0,0,0] && base == physical cursor
      && laterCursor == cursor+1024 && reservation == [base,1024,1])
    createdMemory <- memoryAt creationReturn
    record <- wordsAt ControlCreationRejected createdMemory c 10
    ensure ControlCreationRejected (record == [2,0,0x69cbf0,14,1024,base,0,0,fromIntegral number,1])
    node <- readBytes ControlCreationRejected before t 56
    readWord ControlAuxiliaryRejected before (auxNode+8) >>= ensure ControlAuxiliaryRejected . (==aux .|. 0x40000000)
    let creationWords = [2,0,0x69cbf0,14,1024,base,0,0,fromIntegral number,1]
        expectedCreation = foldr (uncurry putWord) before (zip [c,c+4..] creationWords)
        expectedNode = foldr (uncurry putWord) expectedCreation
          [(t+16,0),(t+32,1),(t+36,0),(t+40,0),(t+44,0),(t+48,0)]
        expectedNode' = putBytes (t+12) (B.pack [0,14,B.index node 14 .&. 0xf8]) expectedNode
        auxiliaryFlags = if kind == SentinelFresh then 0 else 4
        expectedAuxiliary = foldr (uncurry putWord) expectedNode'
          [(aux,auxiliaryFlags),(aux+4,0),(aux+8,1),(auxNode,auxNode .|. 0x40000000)
          ,(auxNode+4,auxNode .|. 0x40000000),(auxNode+12,0)]
    sameRegion ControlCreationRejected expectedCreation createdMemory 0x5beac 4800
    sameRegion ControlCreationRejected expectedCreation after 0x5beac 4800
    sameRegion ControlCreationRejected expectedNode' createdMemory 0x5ff0c 6720
    sameRegion ControlAuxiliaryRejected expectedAuxiliary after 0x5d178 1440
    sameRegion ControlAuxiliaryRejected expectedAuxiliary after 0x624b4 1920
    checkActivation kind before after samples
    else do
      mapM_ (\(start,length) -> sameRegion ControlCreationRejected before after start length)
        [(0x5beac,4800),(0x5ff0c,6720),(0x5d178,1440),(0x624b4,1920),(0x89ea98,4),(storage,12),(wrapper,8)]

checkActivation :: ProbeCase -> Memory -> Memory -> [Sample] -> Either ControlError ()
checkActivation kind before after samples = do
  call <- one ActivationCall samples; returned <- one ActivationReturn samples
  arguments call [0x10001]
  statusAt returned (if kind == FreshMasked then 0xffffffe7 else 0)
  ensure ControlIrqRejected (irq (state call) == (if kind == FreshMasked then 0x80 else 0) && irq (state returned) == irq (state call))
  callMemory <- memoryAt call
  returnedMemory <- memoryAt returned
  mapM_ (\view -> sameRegion ControlRunnableRejected returnedMemory after (addr (viewPhysical view))
    (B.length (contents (viewBytes view))))
    (filter (\view -> viewObject view `elem` [TaskNodes,Domains,QueueHeads,Bitmap,SelectedObject,Current])
      (concatMap kernelViews (filter ((==PrimaryKernel) . kernelCore) (sampleKernels returned))))
  identity <- one IdentityReturn samples
  statusAt identity 0
  if kind == FreshMasked then mapM_ (\(start,length) -> sameRegion ControlRunnableRejected callMemory returnedMemory start length)
    [(0x5ff0c,6720),(0x545cc,264)]
    else do
      kernelBefore <- kernelAt call PrimaryKernel; kernelAfter <- kernelAt returned PrimaryKernel
      ensure ControlRunnableRejected (kernelSelected kernelBefore == kernelSelected kernelAfter
        && kernelCurrent kernelBefore == kernelCurrent kernelAfter && kernelCurrent kernelAfter == W.Address (0x4005ff0c+56*103)
        && map taskId (filter ((==1) . taskState) (kernelTasks kernelAfter))
          == sort (TaskId 1 : map taskId (filter ((==1) . taskState) (kernelTasks kernelBefore))))
      let domain = 0x545cc
      heads <- readWord ControlRunnableRejected callMemory (domain+0x20)
      bitmap <- readWord ControlRunnableRejected callMemory (domain+0x24)
      resume <- readWord ControlRunnableRejected callMemory (domain+0x7c)
      base <- readWord ControlRunnableRejected after 0xa5b204
      let headAddress = physical heads+8*14; node = 0x4005ff0c
      tailNode <- readWord ControlRunnableRejected callMemory (headAddress+4)
      ensure ControlRunnableRejected (tailNode == 0x4005ff0c+56*101)
      let expected = foldr (uncurry putWord) callMemory
            [(physical tailNode,node),(headAddress+4,node),(physical node,heads+8*14),(physical node+4,tailNode)
            ,(physical node+24,base+1024),(physical node+28,resume)]
          expected' = putBytes (physical node+12) (B.singleton 1) expected
      sameRegion ControlRunnableRejected expected' returnedMemory 0x5ff0c 6720
      sameRegion ControlRunnableRejected expected' returnedMemory (physical heads) 128
      sameRegion ControlRunnableRejected callMemory returnedMemory (physical bitmap) 4
  readWord ControlRunnableRejected before 0x54760 >>= \current -> readWord ControlRunnableRejected after 0x54760 >>= ensure ControlRunnableRejected . (==current)

checkErrorEffects :: ProbeRecord -> Memory -> Memory -> [Sample] -> Either ControlError ()
checkErrorEffects probe before after samples = do
  let kind = probeCase probe; status = expectedStatus kind
  if status == 0 then sameRegion ControlErrorEffectRejected before after 0xa0f82c 4
    else do
      logged <- one LogCall samples
      arguments logged [if isRefusal kind then 37 else 3,if isFresh kind then 1 else 102,status]
      callback <- one ErrorCallbackCall samples
      arguments callback [status]
      callbackMemory <- memoryAt callback
      readWord ControlErrorEffectRejected callbackMemory 0xa0f82c >>= ensure ControlErrorEffectRejected . (==status)
      readWord ControlErrorEffectRejected after 0xa0f82c >>= ensure ControlErrorEffectRejected . (==status)

checkCallback :: Receipt -> W.Watch -> ProbeRecord -> [Sample] -> Either ControlError ()
checkCallback receipt watch probe samples = do
  entryIndex <- need ControlCallbackRejected (probeCallbackEntryHit probe)
  enteredIndex <- need ControlCallbackRejected (probeCallbackEnteredHit probe)
  entry <- sampleAt receipt entryIndex; entered <- sampleAt receipt enteredIndex
  let before = state entry; after = state entered
  ensure ControlCallbackRejected (probeReturnHit probe == Nothing && entryIndex < enteredIndex
    && W.contextPc before == W.Address 0x69cbf0 && W.contextPc after == W.Address 0x69cbf4
    && sampleCodeWord entry == W.WordValue 0xe52db004
    && W.contextSp after == W.Address (addr (W.contextSp before)-4)
    && W.contextRegisters before == W.contextRegisters after && W.contextLr before == W.contextLr after
    && W.contextCpsr before == W.contextCpsr after && not (any ((==ErrorCallbackReturn) . sampleBoundary) samples))
  arguments entry [0xffffffee]
  let counts = W.contextCounts before; laterCounts = W.contextCounts after
  ensure ControlCallbackRejected (case (counts,laterCounts) of
    ([W.Count a,b],[W.Count c,d]) -> toInteger c == toInteger a+1 && b <= d
    _ -> False)
  saved <- reg 11 before
  memory <- memoryAt entered
  readWord ControlCallbackRejected memory (addr (W.contextSp after)) >>= ensure ControlCallbackRejected . (==saved)
  let writes = filter (\event -> W.eventSource event == W.CpuWrite && W.eventCore event == Just W.Primary)
        (take (cursorNumber (sampleCursor entered)-cursorNumber (sampleCursor entry)) (drop (cursorNumber (sampleCursor entry)) (W.watchRecords watch)))
  ensure ControlCallbackRejected (map (\event -> (W.eventCore event,W.eventAddress event,W.eventSize event,W.eventValue event)) writes
    == [(Just W.Primary,fromIntegral (addr (W.contextSp after)),4,Just (fromIntegral saved))])

checkRelease :: Receipt -> W.Watch -> ProbeRecord -> [Sample] -> Either ControlError ()
checkRelease receipt watch probe samples = do
  deleteIndex <- need ControlReleaseRejected (probeDeleteEntryHit probe)
  entryIndex <- need ControlReleaseRejected (probeReleaseEntryHit probe)
  returnIndex <- need ControlReleaseRejected (probeReleaseReturnHit probe)
  outerIndex <- need ControlReleaseRejected (probeReturnHit probe)
  deleted <- sampleAt receipt deleteIndex; entry <- sampleAt receipt entryIndex
  returned <- sampleAt receipt returnIndex; outer <- sampleAt receipt outerIndex
  ensure ControlReleaseRejected (probeEntryHit probe == Nothing && probeCallbackEntryHit probe == Nothing && probeCallbackEnteredHit probe == Nothing
    && sampleBoundary deleted == DeletionEntry && configuredEntry (probeConfigured probe) (sampleContext deleted)
    && deleteIndex < entryIndex && entryIndex < returnIndex && returnIndex < outerIndex
    && sampleBoundary entry == ReleaseEntry && W.contextPc (state entry) == W.Address 0x726098
    && W.contextLr (state entry) == W.Address 0x726240 && W.contextPc (state returned) == W.Address 0x726240
    && sampleBoundary returned == ReleaseReturn && W.contextPc (state outer) == probeReturnPc probe
    && sampleHit outer == sampleHit (last samples))
  let callerEvents = take (cursorNumber (sampleCursor entry)-cursorNumber (sampleCursor deleted))
        (drop (cursorNumber (sampleCursor deleted)) (W.watchRecords watch))
  ensure ControlReleaseRejected (length (filter (\event -> W.eventSource event == W.CpuFetch
    && W.eventCore event == Just W.Primary && W.eventPc event == Just (W.Address 0x72623c)
    && W.eventAddress event == 0x72623c && W.eventSize event == 4) callerEvents) == 1)
  arguments entry [101]
  statusAt returned 0; statusAt outer 0
  restored True (state entry) (state returned)
  restored False (state deleted) (state outer)
  let interval = filter (\sample -> sampleHit sample >= entryIndex && sampleHit sample <= returnIndex) samples
  ensure ControlSequenceRejected (map sampleBoundary interval == [ReleaseEntry,IdentityCall,IdentityReturn,SaveMaskCall,SaveMaskReturn
    ,TryLockCall,TryLockReturn,UnlockCall,UnlockReturn,RestoreMaskCall,RestoreMaskReturn,ReleaseReturn])
  identity <- one IdentityReturn interval; statusAt identity 0
  saved <- one SaveMaskReturn interval; savedMask <- reg 0 (state saved)
  ensure ControlIrqRejected (savedMask == irq (state entry) && irq (state saved) == 0x80)
  restoredCall <- one RestoreMaskCall interval; arguments restoredCall [savedMask]
  try <- one TryLockCall interval; tried <- one TryLockReturn interval
  unlock <- one UnlockCall interval; unlocked <- one UnlockReturn interval
  arguments try [0x409c280c,1]; arguments unlock [0x409c280c]
  let kind = probeCase probe
  statusAt tried (if kind == ReleasePeerLock then 0xffffffff else 0)
  statusAt unlocked (if kind == ReleaseUnlockRefusal then 0xffffffff else 0)
  before <- memoryAt entry; after <- memoryAt returned
  initialLock <- readWord ControlReleaseRejected before 0x9c280c
  ensure ControlReleaseRejected (initialLock == 0)
  reserved <- readWord ControlReleaseRejected before (0xa0fc44+12*101+8)
  ensure ControlReleaseRejected (reserved == 1)
  let expected = putWord (0xa0fc44+12*101+8) 0 (putWord 0x9c280c 0 before)
      observable view = viewObject view `notElem` [Stack,Parameters,SharedDomains,InterfaceRoots,InterfaceTable]
      primaryViews = sampleLocal entry ++ concatMap kernelViews (filter ((==PrimaryKernel) . kernelCore) (sampleKernels entry))
  mapM_ (\view -> sameRegion ControlReleaseRejected expected after (addr (viewPhysical view)) (B.length (contents (viewBytes view)))) (filter observable primaryViews)
  pool <- readWord ControlReleaseRejected before (0xa5b204+8*101)
  poolSize <- readWord ControlReleaseRejected before (0xa5b208+8*101)
  ensure ControlReleaseRejected (pool >= 0xa10200 && poolSize == 1024)
  sameRegion ControlReleaseRejected before after pool 1024
  triedMemory <- memoryAt tried; unlockMemory <- memoryAt unlock
  readWord ControlReleaseRejected triedMemory 0x9c280c >>= \value -> ensure ControlReleaseRejected (value == (if kind == ReleasePeerLock then 2 else 1))
  readWord ControlReleaseRejected unlockMemory (0xa0fc44+12*101+8) >>= ensure ControlReleaseRejected . (==0)
  readWord ControlReleaseRejected unlockMemory 0x9c280c >>= \value -> ensure ControlReleaseRejected (value == (if kind == ReleasePeerLock then 2 else 1))
  let start = cursorNumber (sampleCursor entry); end = cursorNumber (sampleCursor returned)
      events = take (end-start) (drop start (W.watchRecords watch))
      minimumSp = minimum (map (physical . addr . W.contextSp . state) interval)
      upperSp = physical (addr (W.contextSp (state entry)))
      allowed event = case W.eventPhysical event of
        Just target -> ((target == 0x9c280c || target == 0xa0fc44+12*101+8) && W.eventSize event == 4)
          || (minimumSp >= 8 && target >= fromIntegral minimumSp-8 && target+W.eventSize event <= fromIntegral upperSp)
        _ -> False
  ensure ControlReleaseRejected (all allowed (filter (\event -> W.eventSource event == W.CpuWrite && W.eventCore event == Just W.Primary) events))

qualifyControl :: Receipt -> W.Watch -> Maybe ProbeRecord -> Either ControlError (Maybe ControlProof)
qualifyControl receipt _ Nothing = do
  checkProtocol receipt
  ensure ControlScopeRejected (receiptScope receipt `elem` [StockStartupScope,OwnedStartupScope])
  pure Nothing
qualifyControl receipt watch (Just probe) = do
  checkProtocol receipt
  initial <- checkPhase receipt probe
  ensure ControlScopeRejected (W.watchEnabled watch && W.watchViolation watch == Nothing)
  checkRegisters probe
  checkPatches receipt watch probe initial
  replaySuffix receipt watch probe initial
  let rawSamples = filter ((>probeInitialHit probe) . sampleHit) (receiptSamples receipt)
      samples = filter ((/=Just False) . sampleCallTaken) rawSamples
  ensure ControlContextRejected (not (null samples) && all ((==PrimaryKernel) . contextCore . sampleContext) rawSamples)
  ensure ControlSequenceRejected (sampleHit (last rawSamples) == sampleHit (last samples))
  if isRelease (probeCase probe) then checkRelease receipt watch probe samples
    else checkSubmission receipt watch probe initial samples
  pure (Just (ControlProof (probeCase probe) (if isRelease (probeCase probe) then ReleaseReturnedEndpoint
    else if probeCase probe == CallbackUnreturned then CallbackPrologueEndpoint else ReturnedEndpoint)))
