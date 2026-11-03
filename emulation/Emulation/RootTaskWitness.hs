{-# LANGUAGE OverloadedStrings #-}
module Emulation.RootTaskWitness
  ( SourceFiles(..), ObservedFiles(..), Inputs, QualifiedReceipt, WitnessError(..)
  , admitInputs, admitReceipt, compareReceipts, qualificationValue
  ) where

import qualified A32 as A
import Control.Monad (filterM, foldM, unless)
import qualified Data.Aeson as J
import Data.Aeson ((.:))
import Data.Aeson.Types (parseEither)
import Data.Bits ((.&.), shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, listToMaybe)
import qualified Data.Set as Set
import Data.Word (Word16, Word32, Word64)
import Emulation.RootTaskWire
import Emulation.TaskMemoryWire
import Emulation.TaskMemoryWitness (Memory(..), CoreOrdinals, CaptureCoverage(..))
import qualified Emulation.TaskMemoryWitness as Memory
import qualified Emulation.RootTaskPlan as Plan
import qualified Emulation.Sha256 as SHA
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.RootTask.Admission as Root
import qualified Replacement.RootTask.Artifact as Artifact
import qualified Replacement.KernelInstall.Wire as W
import qualified FujiAF.UpdateContainer as U
import FujiAF.JSON (decodeStrictIntegers)

data Inputs = Inputs
  { inputSource :: SourceFiles, inputRole :: Role, inputIdentity :: Identity
  , inputImage :: B.ByteString, inputProof :: Root.RootProof }
data QualifiedReceipt = QualifiedReceipt Receipt RootSummary

data WitnessError = SourceRejected | ArtifactRejected | ContractRejected | CandidateRejected | ReceiptDecodeRejected | StatusRejected
  | IdentityRejected | ResetRejected | RawWatchRejected | WatchRejected | ClockRejected | FetchRejected | WriteRejected
  | ForeignWriterRejected | PreimageRejected | SnapshotRejected | AliasRejected | ContextRejected | CodeRejected | TaskRejected
  | EntryRejected | ActionOrderRejected | CallRejected | ResourceRejected | SubmissionRejected | FlagRejected | CreationRejected
  | CallbackRejected | CleanupRejected | ReturnRejected | IrqRejected | FrameRejected | CompletionRejected | ComparisonRejected
  | PlanRejected Plan.PlanError deriving (Eq, Show)
newtype ResourceId = ResourceId Word16 deriving (Eq, Ord, Show)
newtype TaskName = TaskName String deriving (Eq, Show)
data FlagAttribute = KeepBits | ClearOnWait deriving (Eq, Show)
data Service = SharedSetup | DiagnosticSetup | Marker | ErrorCallback | ResourceCreate | Workers | StateSetup | PlatformSetup
  | DeviceSetup | ConfigurationSetup | ConfigurationEnable | CheckedA | CheckedB | Adjustment | HardwareSetup | DiagnosticReset
  | DiagnosticCallback | FinalSetup | ConfigurationWrite | ModeQuery | ConfigurationQuery | ConfigurationFinish | ConfigurationCommit
  | Shutdown | LaterTasks | PeriodicRegister | Cleanup deriving (Eq, Show)
data Action = Action Service [W.WordValue] deriving (Eq, Show)
newtype RetainedResult = RetainedResult W.WordValue deriving (Eq, Show)
newtype StackBytes = StackBytes Word32 deriving (Eq, Show)
newtype IrqCycles = IrqCycles Int deriving (Eq, Show)
data ResourceResult = ResourceResult ResourceId RetainedResult deriving (Eq, Show)
data Publication = Publication
  { publicationId :: S.TaskId, publicationName :: TaskName, publicationEntry :: W.Address, publicationArgument :: W.WordValue
  , publicationPriority :: W.WordValue, publicationStackBytes :: StackBytes, publicationCore :: S.KernelCore
  , publicationFlag :: FlagAttribute, publicationStatus :: RetainedResult } deriving (Eq, Show)
data Completion = Terminated | ReturnedMinus63 deriving (Eq, Show)
data CallbackCycle = CallbackCycle W.WordValue W.WordValue deriving (Eq, Show)
data RootSummary = RootSummary
  { summaryActions :: [Action], summaryResources :: [ResourceResult], summaryPublications :: [Publication]
  , summaryCompletion :: Completion, summarySp :: W.Address, summaryLr :: W.Address
  , summaryIrqs :: IrqCycles, summaryCallbacks :: [CallbackCycle] } deriving (Eq, Show)

require :: Bool -> WitnessError -> Either WitnessError ()
require condition refusal = unless condition (Left refusal)
need :: WitnessError -> Maybe a -> Either WitnessError a
need refusal = maybe (Left refusal) Right
address :: W.Address -> Word32
address (W.Address value) = value
word :: W.WordValue -> Word32
word (W.WordValue value) = value
count :: W.Count -> Word64
count (W.Count value) = value
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes value) = value
physical :: Word32 -> Word32
physical = Plan.physical
hash :: B.ByteString -> W.Digest
hash = W.Digest . SHA.sha256Hex . SHA.sha256
parse :: J.FromJSON a => WitnessError -> B.ByteString -> Either WitnessError a
parse refusal raw = do
  value <- either (const (Left refusal)) Right (decodeStrictIntegers raw)
  case J.fromJSON value of J.Success decoded -> Right decoded; J.Error _ -> Left refusal
stateOf :: Sample -> W.Context
stateOf = contextState . sampleContext
pcOf :: Sample -> Word32
pcOf = physical . address . W.contextPc . stateOf
coreOf :: Sample -> S.KernelCore
coreOf = contextCore . sampleContext
clock :: Sample -> Either WitnessError Word64
clock sample = do
  let values = map (toInteger . count) (W.contextCounts (stateOf sample)); total = sum values
  require (length values == 2 && total <= 80000000) ClockRejected
  pure (fromInteger total)
register :: Sample -> Int -> Either WitnessError Word32
register sample index = case index of
  13 -> pure (address (W.contextSp (stateOf sample)))
  14 -> pure (address (W.contextLr (stateOf sample)))
  15 -> pure (address (W.contextPc (stateOf sample))+8)
  _ | index >= 0 && index < 13 -> word <$> need ContextRejected (case drop index (W.contextRegisters (stateOf sample)) of value:_ -> Just value; [] -> Nothing)
  _ -> Left ContextRejected
coreCount :: Sample -> Either WitnessError W.Count
coreCount sample = case (coreOf sample,W.contextCounts (stateOf sample)) of
  (S.PrimaryKernel,[primary,_]) -> Right primary
  (S.SecondaryKernel,[_,secondary]) -> Right secondary
  _ -> Left ClockRejected
memoryResult :: Either Memory.ReplayError a -> Either WitnessError a
memoryResult = either (Left . convert) Right
  where
    convert Memory.WatchRejected = WatchRejected
    convert Memory.ClockRejected = ClockRejected
    convert Memory.FetchRejected = FetchRejected
    convert Memory.WriteRejected = WriteRejected
    convert Memory.ForeignWriterRejected = ForeignWriterRejected
    convert Memory.PreimageRejected = PreimageRejected
    convert Memory.CodeRejected = CodeRejected

admitInputs :: Role -> SourceFiles -> Either WitnessError Inputs
admitInputs role source = do
  canonical <- either (const (Left SourceRejected)) Right (Artifact.canonicalSource (sourceDat source) (sourceImage source))
  let sourcePins = [(sourceA source,"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
        ,(sourceB source,"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
        ,(sourceRun source,"efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef")
        ,(sourceRom source,"4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be")]
  mapM_ (\(body,expected) -> require (hash body == W.Digest expected) SourceRejected) sourcePins
  artifact <- either (const (Left ArtifactRejected)) Right (Artifact.deriveArtifact (sourceLinked source) (sourceSymbols source))
  let staged = Artifact.stageImage4 canonical artifact; expectedImage = Artifact.stagedBytes staged
  contract <- either (const (Left ContractRejected)) Right (decodeStrictIntegers (sourceContract source))
  (inner,datIdentity,imageIdentity) <- either (const (Left ContractRejected)) Right (parseEither contractParser contract)
  require (inner == Artifact.contractValue staged && imageIdentity == hash expectedImage) ContractRejected
  (linkedIdentity,symbolIdentity) <- either (const (Left ContractRejected)) Right
    (parseEither (J.withObject "root artifact identities" (\o -> (,) <$> o .: "linked_sha256" <*> o .: "symbols_sha256")) inner)
  let runtimeImage = case role of Retained -> sourceImage source; Owned -> expectedImage
  require (runDecoded source == runtimeImage) CandidateRejected
  updateIdentity <- case role of
    Retained -> do
      require (runDat source == sourceDat source) CandidateRejected
      pure (W.Digest "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    Owned -> do
      require (hash (runDat source) == datIdentity) CandidateRejected
      original <- either (const (Left CandidateRejected)) Right (U.parseContainer (sourceDat source))
      candidate <- either (const (Left CandidateRejected)) Right (U.parseContainer (runDat source))
      let old = U.parsedParts original; new = U.parsedParts candidate
          restored = new {U.partRecord4=U.partRecord4 old,U.partGap45=U.partGap45 old}
      restoredDat <- either (const (Left CandidateRejected)) Right (U.rebuildContainer candidate restored)
      require (restoredDat == sourceDat source && U.recDecodedSize (U.recHeader (U.partRecord4 new)) == fromIntegral (B.length runtimeImage)) CandidateRejected
      pure datIdentity
  proof <- case role of
    Owned -> pure (Artifact.artifactProof artifact)
    Retained -> pure (Artifact.canonicalProof canonical)
  let identity = Identity (W.Digest "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef") updateIdentity
        (W.Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be") (hash (sourceObserver source))
        linkedIdentity symbolIdentity (hash (sourceContract source))
        [S.SourceIdentity kind (W.Address base) (B.length body) (W.Digest digest) | (kind,base,body,digest) <-
          [(S.LoaderA,0x40000,sourceA source,"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
          ,(S.LoaderB,0xd0000,sourceB source,"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
          ,(S.Image4,0x61c400,sourceImage source,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")]]
  pure (Inputs source role identity runtimeImage proof)
  where
    contractParser = J.withObject "root staging contract" $ \o -> do
      schema <- o .: "schema"; qualification <- o .: "qualification"
      unless (schema == (1 :: Int) && qualification == ("host-admitted-root-task" :: String)) (fail "invalid root staging contract")
      (,,) <$> o .: "root_task" <*> o .: "candidate_dat_sha256" <*> o .: "candidate_image4_sha256"

readBytes :: WitnessError -> Memory -> Word32 -> Int -> Either WitnessError B.ByteString
readBytes refusal (Memory memory) base size = B.pack <$> mapM (\offset -> need refusal (M.lookup (base+fromIntegral offset) memory)) [0..size-1]
readWord :: WitnessError -> Memory -> Word32 -> Either WitnessError Word32
readWord refusal memory base = (`U.word32At` 0) <$> readBytes refusal memory base 4
mergeView :: Memory -> View -> Either WitnessError Memory
mergeView (Memory memory) view = Memory <$> foldM insert memory (zip [address (viewPhysical view)..] (B.unpack (bytes (viewBytes view))))
  where
    insert old (target,value) = case M.lookup target old of
      Just previous -> require (previous == value) SnapshotRejected >> pure old
      Nothing -> pure (M.insert target value old)
checkAliases :: Sample -> View -> Either WitnessError ()
checkAliases sample view = do
  let body = bytes (viewBytes view); low = address (viewPhysical view); expected = hash body
  require (not (B.null body) && toInteger low+toInteger (B.length body) <= 0x20000000
    && viewAliases view == replicate 2 (replicate 2 expected)) AliasRejected
  mapM_ (\banks -> do
    require (length banks == 2) ContextRejected
    mapM_ (\(W.WordValue bank) -> if bank .&. 1 == 0 || (bank `shiftR` 2) .&. 31 == 0 then pure () else do
      let sizeLog2 = (bank `shiftR` 2) .&. 31; start = fromIntegral (bank .&. 0xfffff000) :: Word64; size = 512 `shiftL` fromIntegral sizeLog2
      require (sizeLog2 >= 3 && sizeLog2 <= 14 && start `mod` size == 0 && start+size <= 0x100000000) ContextRejected
      mapM_ (\alias -> let begin = fromIntegral low+alias; end = begin+fromIntegral (B.length body)
        in require (end <= start || begin >= start+size) ContextRejected) [0,0x40000000]) banks) (W.contextTcm (stateOf sample))
viewShape :: View -> (Object,S.KernelCore,Word32,Int)
viewShape view = (viewObject view,viewCore view,address (viewPhysical view),B.length (bytes (viewBytes view)))
recordPools :: [(S.KernelCore,Word32,Word32,Word32)]
recordPools = [(S.PrimaryKernel,0x5beac,0x5ff0c,0x5d178),(S.SecondaryKernel,0xebe6c,0xefecc,0xed138)]
allocatedTask :: Memory -> S.KernelCore -> Word32 -> Either WitnessError (Word32,Word32,Word32,Word32)
allocatedTask memory core node = do
  (_,creations,nodes,_) <- need TaskRejected (find (\(owner,_,_,_) -> owner == core) recordPools)
  let selected = physical node
  require (selected >= nodes && (selected-nodes) `mod` 56 == 0 && (selected-nodes) `div` 56 < 120) TaskRejected
  let index = (selected-nodes) `div` 56; creation = creations+40*index
  pointer <- readWord TaskRejected memory (selected+8)
  ident <- readWord TaskRejected memory (creation+32)
  created <- readWord TaskRejected memory (creation+36)
  low <- physical <$> readWord TaskRejected memory (creation+20)
  size <- readWord TaskRejected memory (creation+16)
  cursor <- physical <$> readWord TaskRejected memory 0x89ea98
  require (pointer == creation+0x40000000 && ident == index+1 && created == 1 && size /= 0 && size `mod` 4 == 0
    && low >= Plan.stackLow && toInteger low+toInteger size <= toInteger cursor && cursor <= Plan.stackHigh) TaskRejected
  pure (creation,selected,low,size)
data DescriptorScope = RootDescriptorFrame | OtherDescriptorFrame deriving (Eq)
checkViews :: Inputs -> Plan.RootPlan -> Sample -> Memory -> Either WitnessError Memory
checkViews inputs plan sample previous = do
  mapM_ (checkAliases sample) (sampleViews sample)
  descriptorScope <- if sampleBoundary sample == Start || coreOf sample /= S.PrimaryKernel then pure OtherDescriptorFrame
    else do current <- rootCurrent previous; pure (if current then RootDescriptorFrame else OtherDescriptorFrame)
  let retained view = viewObject view /= LoadedCode
        && (viewObject view `notElem` [TaskDescriptor,ResourceDescriptor] || descriptorScope == RootDescriptorFrame)
  memory <- foldM mergeView previous (filter retained (sampleViews sample))
  mapM_ (checkShape descriptorScope memory) (sampleViews sample)
  let shapes = map viewShape (sampleViews sample); core = coreOf sample; pc = pcOf sample
      fixed = [(Code,S.PrimaryKernel,Plan.rootLow,340),(Code,core,pc,4)
        ,(InterfaceRoots,S.PrimaryKernel,0x96bd20,8),(InterfaceTable,S.PrimaryKernel,0x4dae0,0x162c)
        ,(InterfaceTable,S.SecondaryKernel,0xddaa0,0x162c),(TaskClasses,S.PrimaryKernel,0x7cad9c,416)
        ,(ResourceState,S.PrimaryKernel,0x93e6f4,416),(ResourceState,S.PrimaryKernel,0x89ea98,4)]
        ++ [(CallbackResource,S.PrimaryKernel,base,size) | (base,size) <- Plan.callbackResources]
        ++ [(ResourceState,owner,base,size) | (owner,base,size) <- synchronizationShapes]
  require (all (`elem` shapes) fixed) SnapshotRejected
  mapM_ (\owner -> do
    let (creation,node,current,selected,low,high,_) = Plan.taskLayout owner
        expected = [(CreationRecord,owner,physical creation,40),(TaskNode,owner,physical node,56)
          ,(Current,owner,physical current,4),(Selected,owner,physical selected,4),(Pool,owner,low,fromIntegral (high-low))]
    require (all (`elem` shapes) expected) SnapshotRejected) [S.PrimaryKernel,S.SecondaryKernel]
  require (readWord CodeRejected memory 0x96bd20 == Right 0x4004dae0
    && readWord CodeRejected memory 0x96bd24 == Right 0x400ddaa0) CodeRejected
  a <- readBytes CodeRejected memory 0x4dae0 0x162c
  b <- readBytes CodeRejected memory 0xddaa0 0x162c
  require (a == Plan.tableA plan && b == Plan.tableB plan) CodeRejected
  mapM_ (\(Plan.SyncCallback cell target) ->
    require (readWord CodeRejected memory (physical (address cell)) == Right (address target)) CodeRejected)
    (concatMap Plan.syncCallbacks (Plan.syncRecords plan))
  pure memory
  where
    source = Plan.sourceBodies (inputSource inputs) (inputImage inputs)
    synchronizationShapes = [(Plan.syncCore record,physical (address (Plan.syncBase record)),264) | record <- Plan.syncRecords plan]
    checkShape descriptorScope memory view = do
      let (object,core,base,size) = viewShape view
          initial = sampleBoundary sample == Start
          codeBytes = bytes (viewBytes view)
          within :: Word32 -> Word32 -> Bool
          within low high = base >= low && toInteger base+toInteger size <= toInteger high
      case object of
        LoadedCode -> do
          require initial SnapshotRejected
          (start,body) <- need CodeRejected (find (\(start,_) -> start == base) source)
          require (base == start && codeBytes == body) CodeRejected
        ServiceCode -> do
          require (initial && base `mod` 4 == 0 && size `mod` 4 == 0) CodeRejected
          mapM_ (\pc -> do
            expected <- either (const (Left CodeRejected)) Right (Plan.sourceWord source pc)
            require (Set.member pc (Plan.serviceWords plan) && U.word32At codeBytes (fromIntegral (pc-base)) == expected) CodeRejected) [base,base+4..base+fromIntegral size-4]
        Code -> do
          require (size `mod` 4 == 0 && (base == Plan.rootLow && size == 340 || base == pcOf sample && size == 4)) SnapshotRejected
          mapM_ (\pc -> do
            expected <- either (const (Left CodeRejected)) Right (Plan.sourceWord source pc)
            require (U.word32At codeBytes (fromIntegral (pc-base)) == expected) CodeRejected) [base,base+4..base+fromIntegral size-4]
        InterfaceRoots -> require (core == S.PrimaryKernel && base == 0x96bd20 && size == 8) SnapshotRejected
        InterfaceTable -> require ((core,base,size) `elem` [(S.PrimaryKernel,0x4dae0,0x162c),(S.SecondaryKernel,0xddaa0,0x162c)]) SnapshotRejected
        CallbackResource -> require (core == S.PrimaryKernel && (base,size) `elem` Plan.callbackResources) SnapshotRejected
        TaskClasses -> do
          require (core == S.PrimaryKernel && base == 0x7cad9c && size == 416) SnapshotRejected
          require (codeBytes == B.take size (B.drop (fromIntegral (base-0x61c400)) (inputImage inputs))) CodeRejected
        ResourceState -> require ((base,size) `elem` [(0x93e6f4,416),(0x89ea98,4)]
          || (core,base,size) `elem` synchronizationShapes
          || any (\(owner,_,_,flags) -> owner == core && within flags (flags+1440) && size == 12 && (base-flags) `mod` 12 == 0) recordPools) SnapshotRejected
        Pool -> require ((base == Plan.stackLow && size == fromIntegral (Plan.stackHigh-Plan.stackLow) && initial)
          || any (\(owner,c,n,f) -> owner == core && initial && (base,size) `elem` [(c,4800),(n,6720),(f,1440)]) recordPools
          || any (\owner -> let (_,_,_,_,low,high,_) = Plan.taskLayout owner in owner == core && base == low && size == fromIntegral (high-low)) [S.PrimaryKernel,S.SecondaryKernel]) SnapshotRejected
        CreationRecord -> require (any (\(owner,c,_,_) -> owner == core && within c (c+4800) && size == 40 && (base-c) `mod` 40 == 0) recordPools) SnapshotRejected
        TaskNode -> require (any (\(owner,_,n,_) -> owner == core && within n (n+6720) && size == 56 && (base-n) `mod` 56 == 0) recordPools) SnapshotRejected
        Current -> require ((core,base,size) `elem` [(S.PrimaryKernel,0x54760,4),(S.SecondaryKernel,0xe4720,4)]) SnapshotRejected
        Selected -> require ((core,base,size) `elem` [(S.PrimaryKernel,0x54768,4),(S.SecondaryKernel,0xe4728,4)]) SnapshotRejected
        Stack -> do
          require (within Plan.stackLow Plan.stackHigh && size `mod` 4 == 0) SnapshotRejected
          let matches = [() | (owner,c,_,_) <- recordPools,owner == core,index <- [0..119]
                ,readWord TaskRejected memory (c+40*index+36) == Right 1
                ,fmap physical (readWord TaskRejected memory (c+40*index+20)) == Right base
                ,readWord TaskRejected memory (c+40*index+16) == Right (fromIntegral size)]
          require (not (null matches)) TaskRejected
        TaskDescriptor -> do
          descriptor <- physical <$> register sample 0
          require (sampleBoundary sample == TaskSubmit && base == descriptor && size == 28
            && (descriptorScope /= RootDescriptorFrame || within 0xa10a00 0xa10e00)) SnapshotRejected
        ResourceDescriptor -> do
          descriptor <- physical <$> register sample 1
          require (base == descriptor && (pcOf sample,size) `elem` [(0x724924,12),(0x725f08,8),(0x725fa8,24)]
            && (descriptorScope /= RootDescriptorFrame || within 0xa10a00 0xa10e00)) SnapshotRejected

checkSample :: Inputs -> Plan.RootPlan -> Receipt -> Hit -> Int -> Sample -> Memory -> Either WitnessError Memory
checkSample inputs plan receipt hit index sample previous = do
  let state = stateOf sample; context = sampleContext sample; pc = pcOf sample
  total <- clock sample
  require (sampleHit sample == index && length (W.contextRegisters state) == 13 && length (W.contextTcm state) == 2
    && W.contextIdentities state == map W.WordValue [0x4003,0x5003]
    && Plan.declaredBoundary plan sources (address (W.contextPc state)) (sampleBoundary sample)) ContextRejected
  require (index == 0 && sampleBoundary sample == Start
    || M.member (W.contextPc state) (receiptSites receipt)) ContextRejected
  let recorded = hitState hit
  require (hitBoundary hit == sampleBoundary sample && hitCore hit == contextCore context && count (hitCount hit) == total
    && W.contextPc recorded == W.contextPc state && W.contextRegisters recorded == W.contextRegisters state
    && W.contextSp recorded == W.contextSp state && W.contextLr recorded == W.contextLr state
    && W.contextCpsr recorded == W.contextCpsr state) ContextRejected
  expected <- either (const (Left CodeRejected)) Right (Plan.sourceWord sources pc)
  require (sampleCodeWord sample == W.WordValue expected) CodeRejected
  memory <- checkViews inputs plan sample previous
  if index == 0 then do
    let shapes = map viewShape (sampleViews sample)
        pools = (Pool,S.PrimaryKernel,Plan.stackLow,fromIntegral (Plan.stackHigh-Plan.stackLow))
          : concat [[(Pool,core,c,4800),(Pool,core,n,6720),(Pool,core,f,1440)] | (core,c,n,f) <- recordPools]
        loaded = [(LoadedCode,if base == 0xd0000 then S.SecondaryKernel else S.PrimaryKernel,base,B.length body) | (base,body) <- sources]
    require (all (`elem` shapes) (pools++loaded)) SnapshotRejected
    require (physical (address (W.contextLr state)) `elem` (case coreOf sample of
      S.PrimaryKernel -> [0x69ce8c,0x69cee8]; S.SecondaryKernel -> [0x69cf38,0x69cf70])) EntryRejected
  else pure ()
  pure memory
  where sources = Plan.sourceBodies (inputSource inputs) (inputImage inputs)

newtype Executions = Executions (Set.Set (W.Core,Word32,W.Count))
executed :: Executions -> Sample -> Word32 -> Either WitnessError Bool
executed (Executions events) sample pc = do
  ordinal <- coreCount sample
  require (count ordinal /= maxBound) ClockRejected
  pure (Set.member (S.watchCore (coreOf sample),physical pc,W.Count (count ordinal+1)) events)
isFetchSite :: Plan.RootPlan -> Sample -> Bool
isFetchSite plan sample = any (\classification -> W.CpuFetch `elem` W.classKinds classification
  && case W.classRange classification of
    [low,high] -> fromIntegral (pcOf sample) >= low && fromIntegral (pcOf sample)+4 <= high
    _ -> False) (Memory.planClasses (Plan.memoryPlan plan))

-- The initial loaded views seed unmodified source bytes; every later source write is observed.
seedSource :: Plan.RootPlan -> Memory -> CapturedEvent -> Either WitnessError Memory
seedSource plan (Memory old) captured
  | W.eventSource event /= W.CpuWrite = Right (Memory old)
  | otherwise = do
      target <- need WriteRejected (W.eventPhysical event)
      let size = W.eventSize event
      require (target <= 0xffffffff && size `elem` [1,2,4,8]) WriteRejected
      seeded <- foldM (\memory offset -> do
        let location = fromIntegral target+offset
        case M.lookup location memory of
          Just _ -> pure memory
          Nothing -> do
            let contains source =
                  let base = address (Memory.imageBase source)
                  in location >= base && toInteger location < toInteger base+toInteger (B.length (bytes (Memory.imageBytes source)))
            image <- need PreimageRejected (find contains (Memory.planCode (Plan.memoryPlan plan)))
            let value = B.index (bytes (Memory.imageBytes image)) (fromIntegral (location-address (Memory.imageBase image)))
            pure (M.insert location value memory)) old [0..fromIntegral size-1]
      pure (Memory seeded)
  where event = eventRecord captured
advanceRoot :: Inputs -> Plan.RootPlan -> CoreOrdinals -> CoreOrdinals -> Word64 -> Word64
  -> (Memory,Word64,CoreOrdinals,Set.Set (W.Core,Word32,W.Count)) -> CapturedEvent
  -> Either WitnessError (Memory,Word64,CoreOrdinals,Set.Set (W.Core,Word32,W.Count))
advanceRoot inputs plan previous next lower upper (memory,oldClock,ordinals,executions) captured = do
  let event = eventRecord captured
  pc <- address <$> need FetchRejected (W.eventPc event)
  core <- need FetchRejected (W.eventCore event)
  if physical pc >= Plan.rootLow && physical pc < Plan.rootHigh then do
    require (core == W.Primary && physical pc `elem` Root.executionPcs (inputProof inputs)) FetchRejected
    if W.eventSource event == W.CpuWrite then do
      target <- need WriteRejected (W.eventPhysical event)
      let low = toInteger (0xa10e00 :: Word32)+toInteger (Root.scratchLow (inputProof inputs))
      require (toInteger target >= low && toInteger target+toInteger (W.eventSize event) <= 0xa10e00) WriteRejected
    else pure ()
  else pure ()
  seeded <- seedSource plan memory captured
  (updated,newClock,newOrdinals) <- memoryResult
    (Memory.advance (Plan.memoryPlan plan) previous next lower upper (seeded,oldClock,ordinals) captured)
  actual <- if W.eventSource event == W.CpuFetch then do
    ordinal <- need ClockRejected (eventCoreInstructions captured)
    pure (Set.insert (core,physical pc,ordinal) executions)
  else pure executions
  pure (updated,newClock,newOrdinals,actual)

replay :: Inputs -> Plan.RootPlan -> Receipt -> CapturedWatch -> Either WitnessError ([Memory],Executions)
replay inputs plan receipt watch = do
  let samples = receiptSamples receipt; hits = receiptHits receipt; rows = captureEvents watch
  require (not (null samples) && length samples == length hits && length samples <= 16384) SnapshotRejected
  first <- need SnapshotRejected (case samples of sample:_ -> Just sample; [] -> Nothing)
  hit <- need ContextRejected (case hits of item:_ -> Just item; [] -> Nothing)
  require (sampleBoundary first == Start && sampleCursor first == 0 && sampleHit first == 0) SnapshotRejected
  initial <- checkSample inputs plan receipt hit 0 first (Memory M.empty)
  (_,cursor,remaining,_,memories,events) <- foldM next (first,0,rows,initial,[initial],Set.empty)
    (zip3 [1..] (drop 1 samples) (drop 1 hits))
  require (cursor == length rows && null remaining) WatchRejected
  pure (reverse memories,Executions events)
  where
    next (previous,cursor,remaining,memory,memories,executions) (index,sample,hit) = do
      lower <- clock previous; upper <- clock sample
      let end = sampleCursor sample; oldCounts = W.contextCounts (stateOf previous); newCounts = W.contextCounts (stateOf sample)
      require (and (zipWith (<=) oldCounts newCounts) && upper >= lower && end >= cursor && end <= length (captureEvents watch)) ClockRejected
      oldOrdinals <- memoryResult (Memory.coreOrdinals oldCounts)
      newOrdinals <- memoryResult (Memory.coreOrdinals newCounts)
      let (segment,rest) = splitAt (end-cursor) remaining
      entered <- if isFetchSite plan sample then do
        stopped <- need FetchRejected (case reverse segment of item:_ -> Just item; [] -> Nothing)
        memoryResult (Memory.checkFetch (Plan.memoryPlan plan) ClassifiedAccess (eventRecord stopped))
        let event = eventRecord stopped
        ordinal <- coreCount sample
        require (W.eventCore event == Just (S.watchCore (coreOf sample)) && W.eventPc event == Just (W.contextPc (stateOf sample))
          && W.eventInstructions event == Just (W.Count upper) && eventCoreInstructions stopped == Just ordinal
          && eventLatestFetch stopped == Nothing) FetchRejected
        pure (init segment)
      else pure segment
      (updated,_,_,actual) <- foldM (advanceRoot inputs plan oldOrdinals newOrdinals lower upper)
        (memory,lower,oldOrdinals,executions) entered
      checked <- checkSample inputs plan receipt hit index sample updated
      pure (sample,end,rest,checked,checked:memories,actual)

action :: Service -> [Word32] -> Action
action service = Action service . map W.WordValue
serviceTarget :: Service -> Word32
serviceTarget service = case service of
  SharedSetup -> 0x649624; DiagnosticSetup -> 0x7284ec; Marker -> 0x72872c; ErrorCallback -> 0x725b54
  ResourceCreate -> 0x7247ac; Workers -> 0x69ca24; StateSetup -> 0x676054; PlatformSetup -> 0x662e48
  DeviceSetup -> 0x665224; ConfigurationSetup -> 0x649070; ConfigurationEnable -> 0x6490dc
  CheckedA -> 0x6a9354; CheckedB -> 0x6ac2b4; Adjustment -> 0x69f204; HardwareSetup -> 0x656788
  DiagnosticReset -> 0x7262cc; DiagnosticCallback -> 0x724cbc; FinalSetup -> 0x6a5634
  ConfigurationWrite -> 0x6a59b8; ModeQuery -> 0x69cfa4; ConfigurationQuery -> 0x6a5920
  ConfigurationFinish -> 0x6a564c; ConfigurationCommit -> 0x6a5c08; Shutdown -> 0x6568ec
  LaterTasks -> 0x69ca44; PeriodicRegister -> 0x725424; Cleanup -> 0x72612c
expectedActions :: [Action]
expectedActions = [action SharedSetup [],action DiagnosticSetup [],action Marker [0x01010000],action ErrorCallback [0x72835c]]
  ++ [action ResourceCreate [identifier,1] | identifier <- [1..97]]
  ++ map (`action` []) [Workers,StateSetup,PlatformSetup,DeviceSetup,ConfigurationSetup]
  ++ [action ConfigurationEnable [1],action CheckedA [],action CheckedB []]
  ++ map (`action` []) [Adjustment,HardwareSetup,DiagnosticReset]
  ++ [action DiagnosticCallback [0x726478],action FinalSetup [],action ConfigurationWrite [34,0,0],action ModeQuery []
     ,action LaterTasks [],action PeriodicRegister [30,0,0x69cc18,0x96ca4c],action Marker [0x11010000],action Cleanup [0]]
data CallEpisode = CallEpisode
  { episodeAction :: Action, episodeSite :: Root.CallSite, episodeCaller :: Sample, episodeEntry :: Maybe Sample
  , episodeReturn :: Maybe Sample }
data ActionTrack = ActionTrack
  { actionEntry :: Maybe Sample, actionPending :: Maybe CallEpisode, actionCompleted :: [CallEpisode]
  , actionResources :: [ResourceResult] }
rootCurrent :: Memory -> Either WitnessError Bool
rootCurrent memory = do
  let (_,node,current,_,_,_,_) = Plan.taskLayout S.PrimaryKernel
  actual <- readWord TaskRejected memory (physical current)
  pure (actual == node)
rootHeader :: Memory -> Bool -> Either WitnessError ()
rootHeader memory live = do
  let (creation,node,current,selected,low,high,_) = Plan.taskLayout S.PrimaryKernel
  record <- mapM (readWord TaskRejected memory . (physical creation+)) [0,4..36]
  require (take 6 record == [2,0,Plan.rootLow,0,high-low,low] && record !! 8 == 104 && record !! 9 <= 1) TaskRejected
  require (readWord TaskRejected memory (physical node+8) == Right creation) TaskRejected
  if live then do
    require (record !! 9 == 1 && readWord TaskRejected memory (physical current) == Right node
      && readWord TaskRejected memory (physical selected) == Right node) TaskRejected
    flags <- readBytes TaskRejected memory (physical node+12) 2
    require (flags == B.pack [1,0]) TaskRejected
  else pure ()
checkRootCaller :: Sample -> Memory -> Either WitnessError ()
checkRootCaller sample memory = do
  require (coreOf sample == S.PrimaryKernel && word (W.contextCpsr (stateOf sample)) .&. 0x23f == 0x13) ContextRejected
  current <- rootCurrent memory
  require current TaskRejected
  rootHeader memory False
preserved :: Sample -> Sample -> Bool
preserved before after = W.contextSp (stateOf before) == W.contextSp (stateOf after)
  && take 8 (drop 4 (W.contextRegisters (stateOf before))) == take 8 (drop 4 (W.contextRegisters (stateOf after)))
checkCallInstruction :: Inputs -> Sample -> Root.CallSite -> Either WitnessError Bool
checkCallInstruction inputs sample site = do
  raw <- either (const (Left CodeRejected)) Right
    (Plan.sourceWord (Plan.sourceBodies (inputSource inputs) (inputImage inputs)) (pcOf sample))
  let taken = Plan.conditionHolds (A.Cond (raw `shiftR` 28)) (W.contextCpsr (stateOf sample))
  case A.decode (pcOf sample) raw of
    A.Branch _ link destination -> require (destination == Root.callTarget site
      && link == (Root.callKind site == Root.Call)) CallRejected
    A.BranchExchange _ True (A.Reg index) -> do
      destination <- register sample index
      require (physical destination == Root.callTarget site && destination .&. 3 == 0 && Root.callKind site == Root.Call) CallRejected
    _ -> Left CallRejected
  pure taken
checkCalleeEntry :: Executions -> [IrqInterval] -> CallEpisode -> Sample -> Either WitnessError ()
checkCalleeEntry executions irqs episode entered = do
  let caller = episodeCaller episode; site = episodeSite episode
  ran <- executed executions caller (Root.callPc site)
  before <- coreCount caller; after <- coreCount entered
  let expectedLr = case Root.callKind site of Root.Call -> Root.callPc site+4; Root.TailCall -> address (W.contextLr (stateOf caller))
      interrupted = sum [count high-count low | IrqInterval low high resume <- irqs
        ,low > before,high <= after,address resume == Root.callTarget site]
  require (ran && coreOf entered == coreOf caller && pcOf entered == Root.callTarget site && count after == count before+1+interrupted
    && W.contextRegisters (stateOf entered) == W.contextRegisters (stateOf caller)
    && W.contextSp (stateOf entered) == W.contextSp (stateOf caller)
    && W.contextCpsr (stateOf entered) == W.contextCpsr (stateOf caller)
    && physical (address (W.contextLr (stateOf entered))) == physical expectedLr) CallRejected
checkActions :: Inputs -> Plan.RootPlan -> Executions -> [IrqInterval] -> [(Sample,Memory)]
  -> Either WitnessError (Sample,[CallEpisode],[ResourceResult])
checkActions inputs plan executions irqs pairs = do
  tracked <- foldM step (ActionTrack Nothing Nothing [] []) pairs
  entry <- need EntryRejected (actionEntry tracked)
  final <- need CompletionRejected (actionPending tracked)
  require (case episodeAction final of Action Cleanup _ -> isJust (episodeEntry final); _ -> False) CompletionRejected
  let completed = reverse (actionCompleted tracked)++[final]
  require (map episodeAction completed == expectedActions && length (actionResources tracked) == 97) ActionOrderRejected
  pure (entry,completed,reverse (actionResources tracked))
  where
    step tracked (sample,memory) = do
      trackedEntry <- if pcOf sample == Plan.rootLow && coreOf sample == S.PrimaryKernel then do
        require (coreOf sample == S.PrimaryKernel && register sample 0 == Right 0
          && W.contextSp (stateOf sample) == W.Address 0xa10e00
          && W.contextLr (stateOf sample) == W.Address 0x453f0) EntryRejected
        rootHeader memory True
        case actionEntry tracked of
          Nothing -> pure tracked {actionEntry=Just sample}
          Just previous -> do
            ran <- executed executions previous Plan.rootLow
            require (not ran && preserved previous sample && W.contextRegisters (stateOf previous) == W.contextRegisters (stateOf sample)) EntryRejected
            pure tracked {actionEntry=Just sample}
      else pure tracked
      if not (isJust (actionEntry trackedEntry)) then pure trackedEntry else do
        entered <- case actionPending trackedEntry of
          Just episode | episodeEntry episode == Nothing && pcOf sample == Root.callTarget (episodeSite episode)
            && coreOf sample == S.PrimaryKernel -> do
              checkCalleeEntry executions irqs episode sample
              pure trackedEntry {actionPending=Just episode {episodeEntry=Just sample}}
          _ -> pure trackedEntry
        returned <- case actionPending entered of
          Just episode | Root.callKind (episodeSite episode) == Root.Call && isJust (episodeEntry episode)
            && pcOf sample == Root.callPc (episodeSite episode)+4 && coreOf sample == S.PrimaryKernel -> do
              checkRootCaller sample memory
              require (preserved (episodeCaller episode) sample) ReturnRejected
              result <- register sample 0
              case episodeAction episode of
                Action CheckedA _ -> require (result == 0) CallRejected
                Action CheckedB _ -> require (result == 0) CallRejected
                Action ModeQuery _ -> require (result /= 0) CallRejected
                Action ResourceCreate [W.WordValue identifier,_] -> require (result == 0 && identifier >= 1 && identifier <= 97) ResourceRejected
                Action Cleanup _ -> require (result == 0xffffffc1) ReturnRejected
                Action ErrorCallback _ -> require (readWord CallRejected memory 0x89ea8c == Right 0x72835c) CallRejected
                Action DiagnosticCallback _ -> require (readWord CallRejected memory 0x89ea94 == Right 0x726478) CallRejected
                Action Workers _ -> require (readWord CallRejected memory 0x8e1f28 == Right 0x69c9f8) CallRejected
                _ -> pure ()
              let resource = case episodeAction episode of
                    Action ResourceCreate [W.WordValue identifier,_] -> [ResourceResult (ResourceId (fromIntegral identifier)) (RetainedResult (W.WordValue result))]
                    _ -> []
                  finished = episode {episodeReturn=Just sample}
              case episodeAction episode of
                Action Cleanup _ -> pure entered {actionPending=Just finished}
                _ -> pure entered {actionPending=Nothing,actionCompleted=finished:actionCompleted entered,actionResources=resource++actionResources entered}
          _ -> pure entered
        case M.lookup (pcOf sample) (Plan.rootCallMap plan) of
          Nothing -> pure returned
          Just site -> do
            checkRootCaller sample memory
            taken <- checkCallInstruction inputs sample site
            if not taken then pure returned else case actionPending returned of
              Just previous | Root.callPc (episodeSite previous) == pcOf sample && episodeEntry previous == Nothing -> do
                ran <- executed executions (episodeCaller previous) (Root.callPc site)
                require (not ran && preserved (episodeCaller previous) sample) CallRejected
                pure returned {actionPending=Just previous {episodeCaller=sample}}
              Just _ -> Left ActionOrderRejected
              Nothing -> do
                expected <- need ActionOrderRejected (case drop (length (actionCompleted returned)) expectedActions of value:_ -> Just value; [] -> Nothing)
                let Action service arguments = expected
                observed <- mapM (register sample) [0..length arguments-1]
                require (Root.callTarget site == serviceTarget service && map W.WordValue observed == arguments) CallRejected
                let callerSp = toInteger (physical (address (W.contextSp (stateOf sample))))
                require (callerSp <= 0xa10e00 && callerSp >= 0xa10e00+toInteger (Root.scratchLow (inputProof inputs))
                  && callerSp `mod` 8 == 0) FrameRejected
                pure returned {actionPending=Just (CallEpisode expected site sample Nothing Nothing)}

data SubmissionStage = Submitted | FlagsEntered | FlagsReady FlagAttribute | ConstructorEntered FlagAttribute W.Address | Constructed FlagAttribute
data SubmissionEpisode = SubmissionEpisode
  { submissionCaller :: Sample, submissionValue :: Publication, submissionStage :: SubmissionStage }
data SourceSubmission = SourceSubmission
  { sourceService :: Service, sourceReturn :: Word32, sourceTask :: S.TaskId, sourceName :: TaskName, sourceNamePointer :: Word32
  , sourceEntry :: Word32, sourceArgument :: Word32, sourcePriority :: Word32, sourceStack :: Word32 }
-- | Every task submission reachable from root services 0x69ca24 and 0x69ca44 in source call order.
-- Worker ids come from the runtime table at 0x4089e9f8.
expectedPublications :: [SourceSubmission]
expectedPublications = [SourceSubmission Workers 0x6b07f4 (S.TaskId identifier) (TaskName "execcpu") 0x781a3c 0x6c6b5c 0x8e1f28 2 4096 | identifier <- [88..92]]
  ++ [SourceSubmission LaterTasks returnPc (S.TaskId identifier) (TaskName name) namePointer entry argument priority size
     | (returnPc,identifier,name,namePointer,priority,size,entry,argument) <-
       [(0x69c6e0,9,"timer",0x781940,3,3072,0x69c5d4,0x89f770),(0x69c3e4,6,"delay",0x781938,3,3072,0x69c2b0,0x89f76c)
       ,(0x6b15d4,8,"calendar",0x781a54,4,3072,0x6b14e4,0x8e1f2c),(0x6b7910,37,"keytimer",0x781d40,4,3072,0x6b7d7c,0)
       ,(0x71fdb4,28,"media",0x782000,10,3072,0x71fe7c,0),(0x7218b4,101,"mediamsg",0x782008,2,3072,0x721744,0x9114e0)
       ,(0x73bfd8,95,"elfloader",0x782078,1,3072,0x73bc54,0),(0x73b0a8,60,"flashloader",0x782064,4,3072,0x73b3a0,0x91482c)
       ,(0x6d48b4,69,"hdmi",0x781e40,7,3072,0x6d6c9c,0x8e2090),(0x6bda4c,98,"lenscom",0x781d88,4,3072,0x6b9c68,0)
       ,(0x6c5998,99,"lswatch",0x781e10,4,3072,0x6c28f4,0),(0x6c0934,100,"lensdata",0x781e04,5,3072,0x6c12d8,0)
       ,(0x76fe08,50,"lensdc",0x78215c,3,3072,0x75fe54,0),(0x8923ac,71,"smaphp",0x892430,5,3072,0x892184,0)
       ,(0x8923f8,72,"smaphk",0x892438,5,3072,0x892110,0),(0x6d2928,34,"lcdevf",0x781e38,6,3072,0x6d35e0,0x8e208c)
       ,(0x6d22a4,40,"graphic",0x781e30,4,3072,0x6d2700,0),(0x6d7ffc,73,"vglib",0x781e48,7,4096,0x6d7540,0)
       ,(0x6b0014,83,"pos",0x781a38,4,3072,0x6b0434,0x8e1f24),(0x73c148,59,"initcont",0x78209c,6,3072,0x73e048,0)
       ,(0x6e2f80,30,"soundio",0x781e50,3,3072,0x6e2b70,0),(0x6e2fec,29,"voicecont",0x781e58,6,3072,0x6e2b70,0)
       ,(0x6e3058,27,"comp",0x781e64,9,3072,0x6e2b70,0)]]
flagValue :: FlagAttribute -> Word32
flagValue KeepBits = 0
flagValue ClearOnWait = 4
flagAttribute :: Word32 -> Either WitnessError FlagAttribute
flagAttribute 0 = Right KeepBits
flagAttribute 4 = Right ClearOnWait
flagAttribute _ = Left FlagRejected
taskDomain :: Memory -> S.TaskId -> Either WitnessError S.KernelCore
taskDomain memory (S.TaskId ident) = do
  require (ident >= 1 && ident <= 104) SubmissionRejected
  value <- readWord SubmissionRejected memory (0x7cad9c+4*(fromIntegral ident-1))
  case value of 1 -> Right S.PrimaryKernel; 2 -> Right S.SecondaryKernel; _ -> Left SubmissionRejected
taskLocations :: S.KernelCore -> S.TaskId -> Either WitnessError (Word32,Word32,Word32)
taskLocations core (S.TaskId ident) = do
  require (ident >= 1 && ident <= 120) TaskRejected
  (_,c,n,f) <- need TaskRejected (find (\(owner,_,_,_) -> owner == core) recordPools)
  let index = fromIntegral ident-1
  pure (c+40*index,n+56*index,f+12*index)
wordsAt :: WitnessError -> Memory -> Word32 -> Int -> Either WitnessError [Word32]
wordsAt refusal memory base size = mapM (readWord refusal memory . (base+)) [0,4..fromIntegral size-4]
insideEpisode :: Sample -> CallEpisode -> Either WitnessError Bool
insideEpisode sample episode = do
  let Action service _ = episodeAction episode
  if service `notElem` [Workers,LaterTasks] || coreOf sample /= S.PrimaryKernel then pure False else do
    beginning <- need SubmissionRejected (episodeEntry episode)
    ending <- need SubmissionRejected (episodeReturn episode)
    time <- clock sample; low <- clock beginning; high <- clock ending
    pure (time >= low && time <= high)
descriptorView :: Object -> Sample -> Either WitnessError View
descriptorView object sample = need SnapshotRejected
  (find (\view -> viewObject view == object && viewCore view == coreOf sample) (sampleViews sample))
checkSubmissions :: [CallEpisode] -> Executions -> [(Sample,Memory)] -> Either WitnessError [Publication]
checkSubmissions episodes executions pairs = do
  (pending,completed) <- foldM step (Nothing,[]) pairs
  require (not (isJust pending) && map publicationId (reverse completed) == map sourceTask expectedPublications) SubmissionRejected
  pure (reverse completed)
  where
    step (pending,completed) (sample,memory) = do
      services <- map (\episode -> let Action service _ = episodeAction episode in service) <$> filterM (insideEpisode sample) episodes
      began <- if not (null services) && sampleBoundary sample == TaskSubmit && coreOf sample == S.PrimaryKernel then do
        current <- rootCurrent memory
        if not current then pure pending else do
          require (not (isJust pending)) SubmissionRejected
          source <- need SubmissionRejected
            (case drop (length completed) expectedPublications of value:_ -> Just value; [] -> Nothing)
          let S.TaskId identifier = sourceTask source
          require (sourceService source `elem` services
            && physical (address (W.contextLr (stateOf sample))) == sourceReturn source) SubmissionRejected
          descriptor <- descriptorView TaskDescriptor sample
          fields <- wordsAt SubmissionRejected memory (address (viewPhysical descriptor)) 28
          require (take 6 fields == [fromIntegral identifier,sourceNamePointer source,sourcePriority source,sourceStack source
            ,sourceEntry source,sourceArgument source]) SubmissionRejected
          domain <- taskDomain memory (sourceTask source)
          let flags = if fields !! 6 == 0xa5f0a5f0 then KeepBits else ClearOnWait
              value = Publication (sourceTask source) (sourceName source) (W.Address (sourceEntry source))
                (W.WordValue (sourceArgument source)) (W.WordValue (sourcePriority source))
                (StackBytes (sourceStack source)) domain flags (RetainedResult (W.WordValue 0))
          pure (Just (SubmissionEpisode sample value Submitted))
      else pure pending
      case began of
        Nothing -> pure (Nothing,completed)
        Just episode -> do
          let caller = submissionCaller episode; publication = submissionValue episode; pc = pcOf sample
              core = publicationCore publication; S.TaskId identifier = publicationId publication
              encoded = fromIntegral identifier + (case core of S.PrimaryKernel -> 0x10000; S.SecondaryKernel -> 0x20000)
              W.WordValue priority = publicationPriority publication
              W.WordValue argument = publicationArgument publication
              StackBytes size = publicationStackBytes publication
          if coreOf sample /= coreOf caller then pure (Just episode,completed) else do
            wrapperRan <- executed executions caller 0x725dd8
            if pc `elem` [0x725f08,0x725f0c,0x725fa8,0x725fac] then do
              require (wrapperRan && register sample 4 == register caller 0
                && toInteger (address (W.contextSp (stateOf sample)))+56 == toInteger (address (W.contextSp (stateOf caller)))) SubmissionRejected
              case pc of
                0x725f08 -> do
                  require (case submissionStage episode of Submitted -> True; _ -> False) SubmissionRejected
                  require (register sample 0 == Right encoded) SubmissionRejected
                  let table = case core of S.PrimaryKernel -> 0x4dae0; S.SecondaryKernel -> 0xddaa0
                  require (register sample 3 == readWord FlagRejected memory (table+0x80)) FlagRejected
                  descriptor <- descriptorView ResourceDescriptor sample
                  fields <- wordsAt FlagRejected memory (address (viewPhysical descriptor)) 8
                  require (fields == [flagValue (publicationFlag publication),0]) FlagRejected
                  pure (Just episode {submissionStage=FlagsEntered},completed)
                0x725f0c -> do
                  require (case submissionStage episode of FlagsEntered -> True; _ -> False) SubmissionRejected
                  result <- register sample 0
                  require (result `elem` [0,0xffffffd7]) FlagRejected
                  (_,_,flag) <- taskLocations core (publicationId publication)
                  state <- wordsAt FlagRejected memory flag 12
                  actual <- case state of [attributes,_,1] -> flagAttribute attributes; _ -> Left FlagRejected
                  require (result /= 0 || actual == publicationFlag publication) FlagRejected
                  pure (Just episode {submissionStage=FlagsReady actual},completed)
                0x725fa8 -> do
                  actual <- case submissionStage episode of FlagsReady value -> Right value; _ -> Left SubmissionRejected
                  require (register sample 0 == Right encoded) CreationRejected
                  let table = case core of S.PrimaryKernel -> 0x4dae0; S.SecondaryKernel -> 0xddaa0
                  require (register sample 3 == readWord CreationRejected memory table) CreationRejected
                  descriptor <- descriptorView ResourceDescriptor sample
                  fields <- wordsAt CreationRejected memory (address (viewPhysical descriptor)) 24
                  require (take 5 fields == [2,argument,address (publicationEntry publication),priority,size]) CreationRejected
                  pure (Just episode {submissionStage=ConstructorEntered actual (W.Address (last fields))},completed)
                _ -> do
                  (actual,stack) <- case submissionStage episode of ConstructorEntered value pointer -> Right (value,pointer); _ -> Left SubmissionRejected
                  require (register sample 0 == Right 0) CreationRejected
                  (creation,node,_) <- taskLocations core (publicationId publication)
                  record <- wordsAt CreationRejected memory creation 40
                  require (take 6 record == [2,argument,address (publicationEntry publication),priority-1,size,address stack]
                    && drop 8 record == [fromIntegral identifier,1]
                    && readWord CreationRejected memory (node+8) == Right (creation+0x40000000)) CreationRejected
                  livePriority <- readBytes CreationRejected memory (node+13) 1
                  require (B.head livePriority == fromIntegral (priority-1)) CreationRejected
                  (_,_,low,allocated) <- allocatedTask memory core (node+0x40000000)
                  require (low == physical (address stack) && allocated == size) CreationRejected
                  pure (Just episode {submissionStage=Constructed actual},completed)
            else if pc == physical (address (W.contextLr (stateOf caller))) && pc /= 0x725dd8 then do
              actual <- case submissionStage episode of Constructed value -> Right value; _ -> Left SubmissionRejected
              require (wrapperRan && register sample 0 == Right 0 && preserved caller sample) SubmissionRejected
              pure (Nothing,publication {publicationFlag=actual}:completed)
            else pure (Just episode,completed)

data CompletionWitness = CompletionWitness Completion Sample
checkCleanup :: Inputs -> Executions -> Sample -> [CallEpisode] -> [(Sample,Memory)] -> Either WitnessError CompletionWitness
checkCleanup inputs executions entry episodes pairs = do
  cleanup <- need CleanupRejected (find (\episode -> case episodeAction episode of Action Cleanup _ -> True; _ -> False) episodes)
  entered <- need CleanupRejected (episodeEntry cleanup)
  beginning <- clock entered
  scoped <- filterMRoot beginning pairs
  let lowerPcs = [0x72614c,0x726150,0x72615c,0x726160,0x726198,0x72619c,0x7261a4,0x7261a8,0x7261b8,0x7261bc,0x7261c4]
      lower = filter (\(sample,_) -> pcOf sample `elem` lowerPcs) scoped
      collapsed = foldl (\old pair -> case old of
        previous:_ | pcOf (fst previous) == pcOf (fst pair) -> pair:drop 1 old
        _ -> pair:old) [] lower
  require (map (pcOf . fst) (reverse collapsed) == lowerPcs) CleanupRejected
  mapM_ checkLower (reverse collapsed)
  returning <- filterMReturn cleanup pairs
  case returning of
    sample:_ -> pure (CompletionWitness ReturnedMinus63 sample)
    [] -> do
      selections <- mapM (selectedTransfer beginning) pairs
      completed <- need CompletionRejected (find isJust selections >>= id)
      pure (CompletionWitness Terminated completed)
  where
    filterMRoot beginning = foldM (\kept pair@(sample,memory) -> do
      time <- clock sample
      current <- rootCurrent memory
      pure (if time >= beginning && coreOf sample == S.PrimaryKernel && current then kept++[pair] else kept)) []
    checkLower (sample,memory) = do
      checkRootCaller sample memory
      let pc = pcOf sample
          called = [(0x72614c,0x412bc),(0x72615c,0x4118c),(0x726198,0x4d434)
            ,(0x7261a4,0x41140),(0x7261b8,0x4d1a4),(0x7261c4,0x453f0)]
      case lookup pc called of
        Just function -> require (fmap physical (register sample 3) == Right function) CleanupRejected
        Nothing -> pure ()
      case pc of
        0x726150 -> require (register sample 0 == Right 0) CleanupRejected
        0x726160 -> require (register sample 0 == Right 104) CleanupRejected
        0x726198 -> require (register sample 0 == Right 0x10068) CleanupRejected
        0x7261a8 -> require (testBit (word (W.contextCpsr (stateOf sample))) 7) ContextRejected
        0x7261b8 -> require (register sample 0 == Right 0x10068 && register sample 1 == Right 0) CleanupRejected
        0x7261bc -> do
          let (creation,_,_,_,_,_,_) = Plan.taskLayout S.PrimaryKernel
          require (readWord CleanupRejected memory (physical creation+36) == Right 0) CleanupRejected
        _ -> pure ()
    retired memory = do
      let (creation,node,_,_,_,_,_) = Plan.taskLayout S.PrimaryKernel
      rootHeader memory False
      created <- readWord CleanupRejected memory (physical creation+36)
      state <- readBytes CleanupRejected memory (physical node+12) 1
      pure (created == 0 && B.head state == 0)
    filterMReturn cleanup = foldM (\kept (sample,_) -> do
      let restored = coreOf sample == S.PrimaryKernel && pcOf sample == physical (address (W.contextLr (stateOf entry)))
            && register sample 0 == Right 0xffffffc1 && preserved entry sample
      if not restored then pure kept else do
        returned <- need ReturnRejected (case Root.callKind (episodeSite cleanup) of Root.Call -> episodeReturn cleanup; Root.TailCall -> episodeEntry cleanup)
        low <- clock returned; high <- clock sample
        let relevant = [candidate | (candidate,_) <- pairs,coreOf candidate == S.PrimaryKernel
              ,pcOf candidate `elem` Root.returnPcs (inputProof inputs)]
        terminal <- or <$> mapM (\candidate -> do
          time <- clock candidate; ran <- executed executions candidate (pcOf candidate)
          pure (ran && time >= low && time < high)) relevant
        wrapperReturned <- or <$> mapM (\(candidate,_) -> do
          time <- clock candidate; ran <- executed executions candidate 0x7261cc
          pure (pcOf candidate == 0x7261cc && ran && time < high)) pairs
        require (high > low && (terminal || Root.callKind (episodeSite cleanup) == Root.TailCall && wrapperReturned)) ReturnRejected
        pure (kept++[sample])) []
    selectedTransfer beginning (sample,memory)
      | coreOf sample /= S.PrimaryKernel || pcOf sample /= 0x436e8 = pure Nothing
      | otherwise = do
          time <- clock sample
          dead <- retired memory
          if time < beginning || not dead then pure Nothing else do
            selected <- physical <$> register sample 1
            let (_,root,current,selection,_,_,_) = Plan.taskLayout S.PrimaryKernel
            require (selected /= physical root && readWord CleanupRejected memory (physical current) == Right (selected+0x40000000)
              && readWord CleanupRejected memory (physical selection) == Right (selected+0x40000000)) CleanupRejected
            saved <- physical <$> readWord CleanupRejected memory (selected+28)
            require (saved `elem` [0x43668,0x43728,0x437a4]) CleanupRejected
            savedSp <- physical <$> readWord CleanupRejected memory (selected+24)
            require (physical (address (W.contextSp (stateOf sample))) == savedSp) FrameRejected
            loaded <- executed executions sample 0x436e8
            require loaded CompletionRejected
            candidates <- foldM (\found (candidate,after) -> do
              later <- clock candidate
              if coreOf candidate /= S.PrimaryKernel || pcOf candidate /= saved || later <= time then pure found else do
                restored <- executed executions candidate saved
                isDead <- retired after
                require (restored && isDead) CompletionRejected
                pure (found++[candidate])) [] pairs
            first <- need CompletionRejected (case candidates of value:_ -> Just value; [] -> Nothing)
            firstTime <- clock first
            continuations <- foldM (\found (candidate,after) -> do
              later <- clock candidate
              if coreOf candidate /= S.PrimaryKernel || pcOf candidate /= saved+4 || later <= firstTime then pure found else do
                isDead <- retired after
                require isDead CompletionRejected
                (_,_,stack,size) <- allocatedTask after S.PrimaryKernel (selected+0x40000000)
                let sp = physical (address (W.contextSp (stateOf candidate)))
                require (sp >= stack && toInteger sp <= toInteger stack+toInteger size) FrameRejected
                pure (found++[candidate])) [] pairs
            Just <$> need CompletionRejected (case continuations of value:_ -> Just value; [] -> Nothing)

data CallbackTrack = CallbackTrack
  { callbackEntry :: Sample, callbackCounter :: W.WordValue, callbackFrameSeen :: Bool
  , callbackCall :: Maybe Sample, callbackServiceEntered :: Bool, callbackIncrementSeen :: Bool, callbackEpilogue :: Maybe Sample }
currentNode :: Memory -> S.KernelCore -> Either WitnessError Word32
currentNode memory core = do
  let (_,_,current,_,_,_,_) = Plan.taskLayout core
  readWord TaskRejected memory (physical current)
-- | Two words at a guest address inside the sample's core0 ATCM interrupt-stack bytes.
interruptWords :: Sample -> Word32 -> Either WitnessError [Word32]
interruptWords sample location = do
  InterruptStack (W.Address low) (W.Bytes body) <- need FrameRejected (sampleInterruptStack sample)
  let sp = address (W.contextSp (stateOf sample)); (bottom,top) = Plan.irqStack
  require (coreOf sample == S.PrimaryKernel && sp >= bottom+8 && low == sp-8
    && toInteger low+toInteger (B.length body) == toInteger top
    && location >= low && toInteger location+8 <= toInteger top) FrameRejected
  let offset = fromIntegral (location-low)
  pure [U.word32At body offset,U.word32At body (offset+4)]
-- | F is inherited from the interrupted context; I is cleared at 0x4bef8 before the 0x4bf08 dispatch.
interruptDispatch :: Sample -> Memory -> Either WitnessError ()
interruptDispatch sample memory = do
  let state = stateOf sample; sp = address (W.contextSp state); (bottom,top) = Plan.irqStack
  nest <- readWord IrqRejected memory Plan.irqNest
  require (nest /= 0 && coreOf sample == S.PrimaryKernel
    && take 1 (W.contextTcm state) == [[W.WordValue 0,W.WordValue Plan.irqAtcm]]
    && word (W.contextCpsr state) .&. 0x07f0ffbf == 0x113 && sp `mod` 4 == 0
    && sp >= bottom+Plan.irqCallbackDepth && toInteger sp+8 <= toInteger top) IrqRejected
  dispatcher <- interruptWords sample sp
  require (drop 1 dispatcher == [Plan.loaderDispatchReturn]) FrameRejected
checkCallbackFrame :: CallbackTrack -> Sample -> Either WitnessError ()
checkCallbackFrame tracked sample = do
  let caller = callbackEntry tracked; sp = address (W.contextSp (stateOf caller))
  require (sp >= 8 && coreOf caller == coreOf sample
    && address (W.contextSp (stateOf sample)) == sp-8) FrameRejected
  frame <- interruptWords sample (sp-8)
  incomingFp <- register caller 11
  require (frame == [incomingFp,address (W.contextLr (stateOf caller))]) FrameRejected
checkCallbacks :: Executions -> CompletionWitness -> [(Sample,Memory)] -> Either WitnessError [CallbackCycle]
checkCallbacks executions (CompletionWitness _ completed) pairs = do
  beginning <- clock completed
  (pending,cycles) <- foldM (step beginning) (Nothing,[]) pairs
  require (not (isJust pending) && not (null cycles)) CompletionRejected
  pure (reverse cycles)
  where
    step beginning (pending,cycles) (sample,memory) = do
      time <- clock sample
      if time < beginning then pure (pending,cycles) else case pending of
        Nothing | pcOf sample == 0x69cc18 -> do
          slot <- register sample 0
          require (slot >= 1 && slot <= 64 && W.contextLr (stateOf sample) == W.Address 0x723394) CallbackRejected
          handle <- readWord CallbackRejected memory 0x96ca4c
          require (readWord CallbackRejected memory (0x9c3500+4*slot) == Right 0x69cc18
            && readWord CallbackRejected memory (0x9c3600+4*slot) == Right handle
            && readWord CallbackRejected memory (0x9c3700+4*slot) == Right 30
            && readWord CallbackRejected memory (0x9c3800+4*slot) == Right 1) CallbackRejected
          let (creation,node,_,_,_,_,_) = Plan.taskLayout S.PrimaryKernel
          require (readWord CallbackRejected memory (physical creation+36) == Right 0) CallbackRejected
          dead <- readBytes CallbackRejected memory (physical node+12) 1
          require (B.head dead == 0) CallbackRejected
          interruptDispatch sample memory
          counter <- readWord CallbackRejected memory 0x828094
          require (counter >= 1 && counter <= 104) CallbackRejected
          pure (Just (CallbackTrack sample (W.WordValue counter) False Nothing False False Nothing),cycles)
        Nothing -> pure (Nothing,cycles)
        Just tracked | coreOf sample /= coreOf (callbackEntry tracked) -> pure (pending,cycles)
        Just tracked -> do
          let caller = callbackEntry tracked; before = word (callbackCounter tracked); pc = pcOf sample
              expected = if before == 104 then 1 else before+1
          case pc of
            0x69cc18 -> do
              ran <- executed executions caller 0x69cc18
              require (not ran && preserved caller sample && W.contextRegisters (stateOf caller) == W.contextRegisters (stateOf sample)) CallbackRejected
              interruptDispatch sample memory
              pure (Just tracked {callbackEntry=sample},cycles)
            0x69cc1c -> do
              ran <- executed executions caller 0x69cc18
              require ran CallbackRejected
              checkCallbackFrame tracked sample
              pure (Just tracked {callbackFrameSeen=True},cycles)
            0x69cc2c -> do
              require (callbackFrameSeen tracked && register sample 0 == Right before) CallbackRejected
              checkCallbackFrame tracked sample
              pure (Just tracked {callbackCall=Just sample},cycles)
            0x725b74 -> do
              call <- need CallbackRejected (callbackCall tracked)
              ran <- executed executions call 0x69cc2c
              require (ran && register sample 0 == Right before && W.contextLr (stateOf sample) == W.Address 0x69cc30) CallbackRejected
              pure (Just tracked {callbackServiceEntered=True},cycles)
            0x69cc30 -> do
              require (callbackServiceEntered tracked && register sample 0 /= Right 0xffffffc0) CallbackRejected
              checkCallbackFrame tracked sample
              pure (pending,cycles)
            0x69cc54 -> do
              require (callbackServiceEntered tracked && readWord CallbackRejected memory 0x828094 == Right (before+1)) CallbackRejected
              pure (Just tracked {callbackIncrementSeen=True},cycles)
            0x69cc68 -> do
              require (callbackFrameSeen tracked && callbackIncrementSeen tracked
                && readWord CallbackRejected memory 0x828094 == Right expected) CallbackRejected
              checkCallbackFrame tracked sample
              pure (Just tracked {callbackEpilogue=Just sample},cycles)
            _ | pc == physical (address (W.contextLr (stateOf caller))) -> do
              epilogue <- need CallbackRejected (callbackEpilogue tracked)
              ran <- executed executions epilogue 0x69cc68
              require (ran && preserved caller sample
                && word (W.contextCpsr (stateOf sample)) .&. 0x3ff == word (W.contextCpsr (stateOf caller)) .&. 0x3ff
                && readWord CallbackRejected memory 0x828094 == Right expected) ReturnRejected
              interruptDispatch sample memory
              pure (Nothing,CallbackCycle (callbackCounter tracked) (W.WordValue expected):cycles)
            _ -> pure (pending,cycles)

data IrqInterval = IrqInterval W.Count W.Count W.Address
data IrqTrack = IrqTrack
  { irqEntered :: Sample, irqSaved :: Maybe (W.Address,[Word32]), irqReturn :: Maybe Sample }
checkIrqs :: Executions -> [(Sample,Memory)] -> Either WitnessError (IrqCycles,[IrqInterval])
checkIrqs executions pairs = do
  (pending,intervals) <- foldM step (Nothing,[]) pairs
  require (not (isJust pending) && not (null intervals)) IrqRejected
  pure (IrqCycles (length intervals),reverse intervals)
  where
    step (pending,intervals) (sample,memory) = do
      current <- rootCurrent memory
      if coreOf sample /= S.PrimaryKernel then pure (pending,intervals) else case (sampleBoundary sample,pending) of
        (IrqEntry,Nothing) | current -> do
          require (word (W.contextCpsr (stateOf sample)) .&. 0xbf == 0x92
            && word (contextSpsr (sampleContext sample)) .&. 0x23f == 0x13
            && physical (address (W.contextLr (stateOf sample))) >= 4) IrqRejected
          pure (Just (IrqTrack sample Nothing Nothing),intervals)
        (IrqEntry,Just _) | current -> Left IrqRejected
        (IrqFrame,Just tracked) | current -> do
          ran <- executed executions (irqEntered tracked) 0x40204
          require (ran && word (W.contextCpsr (stateOf sample)) .&. 0xbf == 0x93) FrameRejected
          let sp = physical (address (W.contextSp (stateOf sample)))
          require (sp >= 0xa10a00 && toInteger sp+32 <= 0xa10e00) FrameRejected
          payload <- wordsAt FrameRejected memory sp 32
          volatile <- mapM (register (irqEntered tracked)) [0,1,2,3,12]
          saved <- mapM (register (irqEntered tracked)) [4..11]
          present <- mapM (register sample) [4..11]
          status <- need FrameRejected (listToMaybe payload)
          require (status == word (contextSpsr (sampleContext (irqEntered tracked)))
            && take 5 (drop 1 payload) == volatile && last payload == physical (address (W.contextLr (stateOf (irqEntered tracked))))-4
            && saved == present) FrameRejected
          pure (Just tracked {irqSaved=Just (W.Address sp,payload)},intervals)
        (IrqReturn,Just tracked) | current -> do
          (W.Address savedSp,payload) <- need FrameRejected (irqSaved tracked)
          status <- need FrameRejected (listToMaybe payload)
          require (physical (address (W.contextSp (stateOf sample))) == savedSp+4
            && contextSpsr (sampleContext sample) == W.WordValue status
            && word (W.contextCpsr (stateOf sample)) .&. 0xbf == 0x93) ReturnRejected
          remaining <- wordsAt FrameRejected memory (savedSp+4) 28
          require (remaining == drop 1 payload) FrameRejected
          pure (Just tracked {irqReturn=Just sample},intervals)
        (_,Just tracked) | current -> case (irqSaved tracked,irqReturn tracked) of
          (Just (W.Address savedSp,payload),Just returning) | pcOf sample == last payload -> do
            ran <- executed executions returning (pcOf returning)
            restored <- mapM (register sample) [0,1,2,3,12]
            before <- mapM (register (irqEntered tracked)) [4..11]
            after <- mapM (register sample) [4..11]
            status <- need FrameRejected (listToMaybe payload)
            require (ran && restored == take 5 (drop 1 payload) && before == after
              && W.contextCpsr (stateOf sample) == W.WordValue status
              && W.contextLr (stateOf sample) == W.Address (payload !! 6)
              && physical (address (W.contextSp (stateOf sample))) == savedSp+32) ReturnRejected
            low <- coreCount (irqEntered tracked); high <- coreCount sample
            require (high > low) ClockRejected
            pure (Nothing,IrqInterval low high (W.Address (last payload)):intervals)
          _ -> pure (pending,intervals)
        _ -> pure (pending,intervals)

checkResources :: [CallEpisode] -> [(Sample,Memory)] -> Either WitnessError ()
checkResources episodes pairs = mapM_ check [episode | episode <- episodes,case episodeAction episode of Action ResourceCreate _ -> True; _ -> False]
  where
    check episode = do
      let Action _ arguments = episodeAction episode
      identifier <- case arguments of W.WordValue value:_ -> Right value; _ -> Left ResourceRejected
      entered <- need ResourceRejected (episodeEntry episode)
      returned <- need ResourceRejected (episodeReturn episode)
      low <- clock entered; high <- clock returned
      descriptors <- foldM (\found (sample,memory) -> do
        time <- clock sample
        if coreOf sample /= S.PrimaryKernel || pcOf sample /= 0x724924 || time < low || time > high then pure found else do
          descriptor <- descriptorView ResourceDescriptor sample
          fields <- wordsAt ResourceRejected memory (address (viewPhysical descriptor)) 12
          require (fields == [0,1,1] && register sample 0 == Right (0x10000+identifier)
            && register sample 3 == readWord ResourceRejected memory (0x4dae0+0x64)) ResourceRejected
          pure (sample:found)) [] pairs
      require (not (null descriptors)) ResourceRejected
      memory <- need ResourceRejected (lookup returned pairs)
      require (readWord ResourceRejected memory (0x93e6f4+4*(identifier-1)) == Right 1) ResourceRejected

data CallbackTransfer = CallbackTransfer
  { transferCore :: S.KernelCore, transferNode :: W.Address, transferReturn :: W.Address }
checkTransfers :: Inputs -> Plan.RootPlan -> [(Sample,Memory)] -> Either WitnessError ()
checkTransfers inputs plan pairs = do
  _ <- foldM step Nothing pairs
  pure ()
  where
    sources = Plan.sourceBodies (inputSource inputs) (inputImage inputs)
    step pending (sample,memory) = do
      node <- W.Address <$> currentNode memory (coreOf sample)
      root <- rootCurrent memory
      let (creation,_,_,_,_,_,_) = Plan.taskLayout S.PrimaryKernel
          dead = readWord TaskRejected memory (physical creation+36) == Right 0
            && readWord TaskRejected memory (physical creation+8) == Right Plan.rootLow
          returning scope = transferCore scope == coreOf sample && transferNode scope == node
            && transferReturn scope == W.contextPc (stateOf sample)
          continued = case pending of Just scope | returning scope -> Nothing; _ -> pending
          selected = case continued of
            Nothing | pcOf sample == 0x69cc18 && dead -> Just (CallbackTransfer (coreOf sample) node (W.contextLr (stateOf sample)))
            _ -> continued
          callback = case selected of Just scope -> transferCore scope == coreOf sample && transferNode scope == node; Nothing -> False
      if coreOf sample == S.PrimaryKernel && root || callback then do
        let pc = pcOf sample
        raw <- either (const (Left CodeRejected)) Right (Plan.sourceWord sources pc)
        case A.decode pc raw of
          A.BranchExchange condition _ (A.Reg index) | Plan.conditionHolds condition (W.contextCpsr (stateOf sample)) ->
            register sample index >>= admitted
          A.DataProcessing condition A.MOV _ destination _ (A.ShiftedImmediate (A.Reg index) A.LSL 0)
            | destination == A.pc && Plan.conditionHolds condition (W.contextCpsr (stateOf sample)) ->
                register sample index >>= admitted
          _ -> pure ()
      else pure ()
      pure selected
    admitted destination = require (destination .&. 3 == 0
      && Set.member (physical destination) (Plan.serviceInstructions plan)) CodeRejected

checkRootInterval :: Executions -> Sample -> [CallEpisode] -> CompletionWitness -> CapturedWatch -> Either WitnessError ()
checkRootInterval _ entry episodes (CompletionWitness completion ending) watch = do
  first <- coreCount entry
  cleanup <- need CleanupRejected (find (\episode -> case episodeAction episode of Action Cleanup _ -> True; _ -> False) episodes)
  final <- case completion of
    Terminated -> coreCount =<< need CleanupRejected (episodeEntry cleanup)
    ReturnedMinus63 -> coreCount ending
  mapM_ (\captured -> do
    let event = eventRecord captured
    case W.eventPc event of
      Just (W.Address pc) | W.eventSource event == W.CpuFetch && physical pc >= Plan.rootLow && physical pc < Plan.rootHigh -> do
        ordinal <- need ClockRejected (eventCoreInstructions captured)
        require (W.eventCore event == Just W.Primary && ordinal >= first && ordinal <= final) FetchRejected
      _ -> pure ()) (captureEvents watch)

admitReceipt :: Inputs -> ObservedFiles -> Either WitnessError QualifiedReceipt
admitReceipt inputs observed = do
  value <- either (const (Left ReceiptDecodeRejected)) Right (decodeStrictIntegers (observedReceipt observed))
  receipt <- either (const (Left ReceiptDecodeRejected)) Right (parseEither (receiptWith (observedBytes observed)) value)
  require (receiptSchema receipt == 1 && receiptRole receipt == inputRole inputs && receiptOutcome receipt == S.ObservedMenu
    && receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing && receiptWatchEndError receipt == Nothing
    && receiptWatch receipt == S.WatchOk True Nothing && receiptMenu receipt == S.MenuOk (Just [1,5,2])) StatusRejected
  require (receiptIdentity receipt == inputIdentity inputs) IdentityRejected
  require (receiptCheckpointReused receipt && hash (observedReset observed) == receiptCheckpoint receipt) ResetRejected
  require (hash (observedWatch observed) == receiptWatchDigest receipt) RawWatchRejected
  watch <- parse RawWatchRejected (observedWatch observed)
  plan <- either (Left . PlanRejected) Right (Plan.derivePlan (inputSource inputs) (inputProof inputs) (inputImage inputs))
  memoryResult (Memory.checkWatch (Plan.memoryPlan plan) watch)
  require (Plan.requiredSites plan `Set.isSubsetOf` Set.map (physical . address) (M.keysSet (receiptSites receipt))) ContextRejected
  let sources = Plan.sourceBodies (inputSource inputs) (inputImage inputs)
  mapM_ (\(pc,boundary) -> require (Plan.declaredBoundary plan sources (address pc) boundary) ContextRejected) (M.toList (receiptSites receipt))
  (memories,executions) <- replay inputs plan receipt watch
  let pairs = zip (receiptSamples receipt) memories
  checkTransfers inputs plan pairs
  (irqs,intervals) <- checkIrqs executions pairs
  (entry,episodes,resources) <- checkActions inputs plan executions intervals pairs
  checkResources episodes pairs
  publications <- checkSubmissions episodes executions pairs
  completed@(CompletionWitness completion _) <- checkCleanup inputs executions entry episodes pairs
  callbacks <- checkCallbacks executions completed pairs
  checkRootInterval executions entry episodes completed watch
  final <- need SnapshotRejected (case reverse (receiptSamples receipt) of sample:_ -> Just sample; [] -> Nothing)
  let endingCounts = W.contextCounts (stateOf final)
  require (receiptEndCounts receipt == Just endingCounts && count (receiptInstructions receipt) >= sum (map count endingCounts)
    && count (receiptInstructions receipt) <= 500000000) ClockRejected
  pure (QualifiedReceipt receipt (RootSummary (map episodeAction episodes) resources publications completion
    (W.contextSp (stateOf entry)) (W.contextLr (stateOf entry)) irqs callbacks))

serviceName :: Service -> String
serviceName = show
taskName :: TaskName -> String
taskName (TaskName name) = name
completionName :: Completion -> String
completionName Terminated = "terminated"
completionName ReturnedMinus63 = "returned-minus63"
publicationValue :: Publication -> J.Value
publicationValue publication = J.object
  [ "id" J..= identifier,"name" J..= taskName (publicationName publication),"entry" J..= address (publicationEntry publication)
  , "argument" J..= word (publicationArgument publication),"priority" J..= word (publicationPriority publication)
  , "stack_bytes" J..= size,"core" J..= (case publicationCore publication of S.PrimaryKernel -> ("primary" :: String); S.SecondaryKernel -> "secondary")
  , "flag_attribute" J..= flagValue (publicationFlag publication),"status" J..= word status ]
  where S.TaskId identifier = publicationId publication; StackBytes size = publicationStackBytes publication
        RetainedResult status = publicationStatus publication
qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt receipt summary) = J.object
  [ "schema" J..= (1 :: Int),"qualification" J..= ("natural-root-task-lifecycle" :: String),"role" J..= receiptRole receipt
  , "snapshots" J..= length (receiptSamples receipt),"task" J..= (104 :: Int),"stack" J..= ([0xa10a00,0xa10e00] :: [Word32])
  , "actions" J..= [J.object ["service" J..= serviceName service,"arguments" J..= map word arguments] | Action service arguments <- summaryActions summary]
  , "resources" J..= [J.object ["id" J..= identifier,"status" J..= word status] | ResourceResult (ResourceId identifier) (RetainedResult status) <- summaryResources summary]
  , "publications" J..= map publicationValue (summaryPublications summary),"completion" J..= completionName (summaryCompletion summary)
  , "irq_cycles" J..= cycles,"post_cleanup_callbacks" J..= [J.object ["before" J..= word before,"after" J..= word after] | CallbackCycle before after <- summaryCallbacks summary]
  , "callback_interrupt_stack" J..= [fst Plan.irqStack,snd Plan.irqStack]
  , "watched_end_counts" J..= fmap (map count) (receiptEndCounts receipt)
  , "scope" J..= ("Root104 caller orchestration, resource/constructor effects, conditional cleanup and a surviving periodic consumer; retained services and later application ownership remain separate" :: String)]
  where IrqCycles cycles = summaryIrqs summary
compareReceipts :: QualifiedReceipt -> QualifiedReceipt -> Either WitnessError J.Value
compareReceipts (QualifiedReceipt retained left) (QualifiedReceipt owned right) = do
  require (receiptRole retained == Retained && receiptRole owned == Owned) ComparisonRejected
  let first = receiptIdentity retained; second = receiptIdentity owned
      semantic summary = (summaryActions summary,summaryResources summary,summaryPublications summary,summaryCompletion summary
        ,summarySp summary,summaryLr summary,summaryCallbacks summary)
  require (semantic left == semantic right && identityRun first == identityRun second && identityRom first == identityRom second
    && identityLinked first == identityLinked second && identitySymbols first == identitySymbols second
    && identityContract first == identityContract second && identitySources first == identitySources second
    && identityObserver first == identityObserver second) ComparisonRejected
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("retained-owned-root-task-comparison" :: String)
    ,"task" J..= (104 :: Int),"compared" J..= (["ordered retained calls and consumed arguments","97 successful resource creations"
      ,"worker and later task constructors with consumed flag attributes","within-run task/IRQ state and conditional handback"
      ,"executed post-cleanup periodic callback and surviving resources"] :: [String])])
