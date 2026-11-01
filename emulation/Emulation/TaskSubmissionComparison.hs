{-# LANGUAGE OverloadedStrings #-}
module Emulation.TaskSubmissionComparison
  ( ComparisonError(..), ComparisonProof, compareReceipts, comparisonValue ) where

import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Emulation.StartupTaskWire
import Emulation.StartupTaskWitness (QualifiedReceipt, qualifiedReceiptData, qualifiedProbeData)
import Emulation.TaskSubmissionControlWire
import qualified Replacement.KernelInstall.Wire as W

data ComparisonError = ComparisonScopeRejected | ComparisonCaseRejected | ComparisonCheckpointRejected
  | ComparisonSnapshotRejected | ComparisonStackRejected | ComparisonTaskRejected TaskId Boundary
  | ComparisonReadyRejected KernelCore | ComparisonFirstEntryRejected KernelCore
  | ComparisonControlRejected ProbeCase | ComparisonStorageRejected TaskId
  deriving (Eq, Show)
data ComparisonProof = ComparisonProof (Maybe ProbeCase) deriving (Eq, Show)
data StackAlias = PhysicalStack | CachedStack deriving (Eq, Show)
data Pointer = Absolute W.Address | OwnStack TaskId StackAlias Word32 deriving (Eq, Show)
data TaskRecord = TaskRecord TaskId B.ByteString Pointer B.ByteString B.ByteString Pointer B.ByteString deriving (Eq, Show)
data StorageRecord = StorageRecord Pointer Word32 Pointer Word32 Word32 deriving (Eq, Show)
data FirstEntry = FirstEntry TaskId W.Address W.Address W.Address Pointer [W.WordValue] W.Address Word32 Pointer W.Address deriving (Eq, Show)
data ReadySet = ReadySet [TaskRecord] [(ObjectKind,W.Address,W.Bytes)] W.Address W.Address W.Address deriving (Eq, Show)
type Memory = M.Map Word32 Word8

ensure :: ComparisonError -> Bool -> Either ComparisonError ()
ensure problem condition = unless condition (Left problem)
need :: ComparisonError -> Maybe a -> Either ComparisonError a
need problem = maybe (Left problem) Right
unique :: ComparisonError -> [a] -> Either ComparisonError a
unique _ [item] = Right item
unique problem _ = Left problem
addr :: W.Address -> Word32
addr (W.Address value) = value
word :: W.WordValue -> Word32
word (W.WordValue value) = value
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes value) = value
state :: Sample -> W.Context
state = contextState . sampleContext
memoryAt :: Sample -> Either ComparisonError Memory
memoryAt sample = foldM merge M.empty (sampleLocal sample ++ concatMap kernelViews (sampleKernels sample))
  where merge memory view = foldM insert memory (zip [addr (viewPhysical view)..] (B.unpack (bytes (viewBytes view))))
        insert memory (address,byte) = do
          ensure ComparisonSnapshotRejected (maybe True (==byte) (M.lookup address memory))
          pure (M.insert address byte memory)
readBytes :: Memory -> Word32 -> Int -> Either ComparisonError B.ByteString
readBytes memory start size = B.pack <$> mapM (\offset -> need ComparisonSnapshotRejected (M.lookup (physical start+fromIntegral offset) memory)) [0..size-1]
readWord :: Memory -> Word32 -> Either ComparisonError Word32
readWord memory start = do
  raw <- readBytes memory start 4
  pure (sum [fromIntegral value `shiftL` (8*index) | (index,value) <- zip [0..] (B.unpack raw)])
sampleAt :: Receipt -> HitIndex -> Either ComparisonError Sample
sampleAt receipt index = unique ComparisonSnapshotRejected (filter ((==index) . sampleHit) (receiptSamples receipt))
kernelAt :: Sample -> KernelCore -> Either ComparisonError Kernel
kernelAt sample core = unique ComparisonSnapshotRejected (filter ((==core) . kernelCore) (sampleKernels sample))
oneBoundary :: Receipt -> KernelCore -> Boundary -> Either ComparisonError Sample
oneBoundary receipt core kind = unique ComparisonCheckpointRejected
  (filter (\sample -> sampleBoundary sample == kind && sampleCallTaken sample /= Just False
    && contextCore (sampleContext sample) == core) (receiptSamples receipt))
withinSubmission :: Receipt -> Submission -> Boundary -> Either ComparisonError Sample
withinSubmission receipt submission kind = do
  end <- need ComparisonCheckpointRejected (submissionReturnHit submission)
  unique ComparisonCheckpointRejected (filter (\sample -> sampleBoundary sample == kind && sampleCallTaken sample /= Just False
    && contextCore (sampleContext sample) == submissionCore submission && sampleHit sample >= submissionEntryHit submission
    && sampleHit sample <= end) (receiptSamples receipt))

pointer :: TaskId -> W.Address -> Word32 -> W.Address -> Either ComparisonError Pointer
pointer ident base size actual
  | actual == W.Address 0 = Right (Absolute actual)
  | physical (addr base) < 0xa10200 || physical (addr base) >= 0xa5b200 = Right (Absolute actual)
  | physical (addr actual) >= physical (addr base) && toInteger (physical (addr actual)) <= toInteger (physical (addr base))+toInteger size = do
      ensure ComparisonStackRejected (size > 0 && physical (addr base) >= 0xa10200
        && toInteger (physical (addr base))+toInteger size <= 0xa5b200
        && addr actual .&. 0x80000000 == 0 && addr base .&. 0x80000000 == 0)
      pure (OwnStack ident (if addr actual .&. 0x40000000 == 0 then PhysicalStack else CachedStack)
        (physical (addr actual)-physical (addr base)))
  | otherwise = Right (Absolute actual)

taskRecord :: Sample -> KernelCore -> TaskId -> Either ComparisonError TaskRecord
taskRecord sample core ident = do
  kernel <- kernelAt sample core
  task <- unique ComparisonSnapshotRejected (filter ((==ident) . taskId) (kernelTasks kernel))
  memory <- memoryAt sample
  creation <- readBytes memory (addr (taskCreation task)) 40
  node <- readBytes memory (addr (taskNode task)) 56
  let base = taskStackBase task; size = word (taskStackBytes task)
  creationBase <- readWord memory (addr (taskCreation task)+20)
  savedSp <- readWord memory (addr (taskNode task)+24)
  ensure ComparisonSnapshotRejected (creationBase == addr base && savedSp == addr (taskSavedSp task))
  normalizedBase <- pointer ident base size (W.Address creationBase)
  normalizedSp <- pointer ident base size (W.Address savedSp)
  pure (TaskRecord ident (B.take 20 creation) normalizedBase (B.drop 24 creation)
    (B.take 24 node) normalizedSp (B.drop 28 node))

storageRecord :: Sample -> TaskId -> Either ComparisonError StorageRecord
storageRecord sample ident@(TaskId number) = do
  memory <- memoryAt sample
  let index = fromIntegral number-1; wrapper = 0xa5b204+8*index; storage = 0xa0fc44+12*index
  base <- readWord memory wrapper; size <- readWord memory (wrapper+4)
  reservedBase <- readWord memory storage; reservedSize <- readWord memory (storage+4); reserved <- readWord memory (storage+8)
  let allocationBase = if reservedBase /= 0 then reservedBase else base
      allocationSize = if reservedSize /= 0 then reservedSize else size
  normalizedBase <- pointer ident (W.Address allocationBase) allocationSize (W.Address base)
  normalizedReservation <- pointer ident (W.Address allocationBase) allocationSize (W.Address reservedBase)
  pure (StorageRecord normalizedBase size normalizedReservation reservedSize reserved)

compareTaskCheckpoint :: Receipt -> Receipt -> Submission -> Submission -> Boundary -> Either ComparisonError ()
compareTaskCheckpoint retained owned left right kind = do
  before <- withinSubmission retained left kind; after <- withinSubmission owned right kind
  let ident = submissionId left; core = submissionCore left
  leftRecord <- taskRecord before core ident; rightRecord <- taskRecord after core ident
  ensure (ComparisonTaskRejected ident kind) (leftRecord == rightRecord)
  leftStorage <- storageRecord before ident; rightStorage <- storageRecord after ident
  ensure (ComparisonStorageRejected ident) (leftStorage == rightStorage)
  leftMemory <- memoryAt before; rightMemory <- memoryAt after
  leftDescriptor <- readBytes leftMemory (addr (submissionDescriptor left)) 28
  rightDescriptor <- readBytes rightMemory (addr (submissionDescriptor right)) 28
  ensure (ComparisonTaskRejected ident kind) (leftDescriptor == rightDescriptor)
  let TaskId number = ident
      auxBase = case core of PrimaryKernel -> 0x5d178; SecondaryKernel -> 0xed138
      nodeBase = case core of PrimaryKernel -> 0x624b4; SecondaryKernel -> 0xf2474
  mapM_ (\(start,size) -> do
    leftBytes <- readBytes leftMemory start size; rightBytes <- readBytes rightMemory start size
    ensure (ComparisonTaskRejected ident kind) (leftBytes == rightBytes))
    [(auxBase+12*(fromIntegral number-1),12),(nodeBase+16*(fromIntegral number-1),16)]

readySet :: Receipt -> KernelCore -> Either ComparisonError ReadySet
readySet receipt core = do
  sample <- oneBoundary receipt core AutoReadyReturn
  kernel <- kernelAt sample core
  records <- mapM (taskRecord sample core . taskId) (sortOn taskId (filter ((==1) . taskState) (kernelTasks kernel)))
  let relevant = filter (\view -> viewObject view `elem` [QueueHeads,Bitmap,SelectedObject,Current]) (kernelViews kernel)
  ensure (ComparisonReadyRejected core) (length relevant == 8)
  pure (ReadySet records (sortOn (\(kind,address,_) -> (kind,address))
    [(viewObject view,viewPhysical view,viewBytes view) | view <- relevant]) (kernelDomain kernel) (kernelSelected kernel) (kernelCurrent kernel))

firstProjection :: Receipt -> FirstTask -> Either ComparisonError FirstEntry
firstProjection receipt first = do
  selected <- sampleAt receipt (firstSelectedHit first)
  kernel <- kernelAt selected (firstCore first)
  task <- unique ComparisonSnapshotRejected (filter ((==firstId first) . taskId) (kernelTasks kernel))
  entryIndex <- need ComparisonCheckpointRejected (firstEntryHit first)
  enteredIndex <- need ComparisonCheckpointRejected (firstEnteredHit first)
  entry <- sampleAt receipt entryIndex; entered <- sampleAt receipt enteredIndex
  let initial = state entry; after = state entered; base = taskStackBase task; size = word (taskStackBytes task)
  stack <- pointer (firstId first) base size (W.contextSp initial)
  committedStack <- pointer (firstId first) base size (W.contextSp after)
  ensure (ComparisonFirstEntryRejected (firstCore first)) (length (W.contextRegisters initial) == 13
    && take 3 (W.contextRegisters after) == take 3 (W.contextRegisters initial))
  pure (FirstEntry (firstId first) (firstNode first) (firstCreation first) (firstEntry first) stack
    (take 3 (W.contextRegisters initial)) (W.contextLr initial) (word (W.contextCpsr initial) .&. 0xff0000ff)
    committedStack (W.contextPc after))

compareNatural :: Receipt -> Receipt -> Either ComparisonError ()
compareNatural retained owned = do
  let left = sortOn submissionId (receiptSubmissions retained); right = sortOn submissionId (receiptSubmissions owned)
  ensure ComparisonCheckpointRejected (map submissionId left == map TaskId [93,102,103,104] && map submissionId right == map submissionId left)
  mapM_ (\(before,after) -> do
    ensure ComparisonCheckpointRejected (submissionCore before == submissionCore after && submissionDescriptor before == submissionDescriptor after
      && submissionSp before == submissionSp after && submissionReturnPc before == submissionReturnPc after
      && submissionStatus before == submissionStatus after)
    mapM_ (compareTaskCheckpoint retained owned before after) [CreationReturn,WrapperReturn]) (zip left right)
  mapM_ (\core -> do
    before <- readySet retained core; after <- readySet owned core
    ensure (ComparisonReadyRejected core) (before == after)
    leftFirst <- unique ComparisonCheckpointRejected (filter ((==core) . firstCore) (receiptFirstTasks retained))
    rightFirst <- unique ComparisonCheckpointRejected (filter ((==core) . firstCore) (receiptFirstTasks owned))
    leftProjection <- firstProjection retained leftFirst; rightProjection <- firstProjection owned rightFirst
    ensure (ComparisonFirstEntryRejected core) (leftProjection == rightProjection)) [PrimaryKernel,SecondaryKernel]

probeSample :: Receipt -> ProbeRecord -> Either ComparisonError Sample
probeSample receipt probe = need ComparisonCheckpointRejected
  (if probeCase probe == CallbackUnreturned then probeCallbackEnteredHit probe else probeReturnHit probe) >>= sampleAt receipt

compareProbe :: Receipt -> Receipt -> ProbeRecord -> ProbeRecord -> Either ComparisonError ()
compareProbe retained owned left right = do
  let kind = probeCase left; ident = probeTask left; problem = ComparisonControlRejected kind
  ensure ComparisonCaseRejected (probeCase right == kind && probeTarget right == probeTarget left
    && probeTask right == ident && probeDescriptor right == probeDescriptor left && probeReturnPc right == probeReturnPc left)
  before <- probeSample retained left; after <- probeSample owned right
  leftRecord <- taskRecord before PrimaryKernel ident; rightRecord <- taskRecord after PrimaryKernel ident
  ensure problem (leftRecord == rightRecord)
  leftStorage <- storageRecord before ident; rightStorage <- storageRecord after ident
  ensure (ComparisonStorageRejected ident) (leftStorage == rightStorage)
  let leftState = state before; rightState = state after
  ensure problem (take 1 (W.contextRegisters leftState) == take 1 (W.contextRegisters rightState)
    && word (W.contextCpsr leftState) .&. 0x80 == word (W.contextCpsr rightState) .&. 0x80
    && W.contextPc leftState == W.contextPc rightState)
  leftMemory <- memoryAt before; rightMemory <- memoryAt after
  let TaskId number = ident
  mapM_ (\(start,size) -> do
    original <- readBytes leftMemory start size; replacement <- readBytes rightMemory start size
    ensure problem (original == replacement))
    [(0xa0f82c,4),(0x89ea8c,4),(0x89ea98,4),(0x9c280c,4),(0x63424,28)
    ,(0x5d178+12*(fromIntegral number-1),12),(0x624b4+16*(fromIntegral number-1),16)]
  leftKernel <- kernelAt before PrimaryKernel; rightKernel <- kernelAt after PrimaryKernel
  let queueViews kernel = sortOn (\(object,address,_) -> (object,address))
        [(viewObject view,viewPhysical view,viewBytes view) | view <- kernelViews kernel
        ,viewObject view `elem` [QueueHeads,Bitmap,SelectedObject,Current]]
  ensure problem (queueViews leftKernel == queueViews rightKernel
    && kernelSelected leftKernel == kernelSelected rightKernel && kernelCurrent leftKernel == kernelCurrent rightKernel
    && map taskId (filter ((==1) . taskState) (kernelTasks leftKernel)) == map taskId (filter ((==1) . taskState) (kernelTasks rightKernel)))
  if kind `elem` [ReleaseUnlocked,ReleasePeerLock,ReleaseUnlockRefusal] then do
    leftEntry <- need ComparisonCheckpointRejected (probeReleaseEntryHit left) >>= sampleAt retained
    rightEntry <- need ComparisonCheckpointRejected (probeReleaseEntryHit right) >>= sampleAt owned
    leftRelease <- need ComparisonCheckpointRejected (probeReleaseReturnHit left) >>= sampleAt retained
    rightRelease <- need ComparisonCheckpointRejected (probeReleaseReturnHit right) >>= sampleAt owned
    mapM_ (\(a,b) -> do
      leftTask <- taskRecord a PrimaryKernel ident; rightTask <- taskRecord b PrimaryKernel ident
      leftStorage' <- storageRecord a ident; rightStorage' <- storageRecord b ident
      ensure problem (leftTask == rightTask && leftStorage' == rightStorage')) [(leftEntry,rightEntry),(leftRelease,rightRelease)]
    else pure ()

sameResetEnvironment :: Receipt -> Receipt -> Bool
sameResetEnvironment retained owned =
  let Identity leftRun _ leftRom leftObserver leftSources = receiptIdentity retained
      Identity rightRun _ rightRom rightObserver rightSources = receiptIdentity owned
      loaders = filter (\(SourceIdentity kind _ _ _) -> kind `elem` [LoaderA,LoaderB])
  in leftRun == rightRun && leftRom == rightRom && leftObserver == rightObserver && loaders leftSources == loaders rightSources

compareReceipts :: QualifiedReceipt -> QualifiedReceipt -> Either ComparisonError ComparisonProof
compareReceipts retainedInput ownedInput = do
  let retained = qualifiedReceiptData retainedInput; owned = qualifiedReceiptData ownedInput
      retainedProbe = qualifiedProbeData retainedInput; ownedProbe = qualifiedProbeData ownedInput
  ensure ComparisonScopeRejected (receiptSchema retained == TaskSubmissionProtocol && receiptSchema owned == TaskSubmissionProtocol
    && case (receiptProgram retained,receiptProgram owned) of (StockProgram,OwnedProgram _ _ _) -> True; _ -> False)
  ensure ComparisonScopeRejected (case (receiptScope retained,receiptScope owned,retainedProbe,ownedProbe) of
    (StockStartupScope,OwnedStartupScope,Nothing,Nothing) -> True
    (StockProbeScope,OwnedProbeScope,Just _,Just _) -> True
    _ -> False)
  ensure ComparisonCheckpointRejected (receiptCheckpointReused retained && receiptCheckpointReused owned && sameResetEnvironment retained owned)
  compareNatural retained owned
  case (retainedProbe,ownedProbe) of
    (Nothing,Nothing) -> pure (ComparisonProof Nothing)
    (Just left,Just right) -> compareProbe retained owned left right >> pure (ComparisonProof (Just (probeCase left)))
    _ -> Left ComparisonCaseRejected

comparisonValue :: ComparisonProof -> J.Value
comparisonValue (ComparisonProof probe) = J.object
  ["schema" J..= (1 :: Int),"comparison" J..= ("retained-owned-task-submission" :: String)
  ,"probe" J..= fmap show probe
  ,"scope" J..= ("Four committed task creation/return records, complete per-core auto-ready queues, defined first-entry ABI and qualified per-receipt prologues; controlled source-service effects at the declared bounded endpoint" :: String)]
