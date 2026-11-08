{-# LANGUAGE OverloadedStrings #-}
module Emulation.StartupTaskWitness
  ( SourceFiles(..), ObservedFiles(..), Inputs, QualifiedReceipt, WitnessError(..)
  , admitInputs, admitOwnedInputs, admitJoinedInputs, admitReceipt, qualificationValue, qualifiedReceiptData, qualifiedProbeData
  ) where

import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.), (.|.), shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.Sha256 as SHA
import Emulation.StartupTaskWire
import qualified Emulation.TaskSubmissionProgram as P
import qualified Emulation.TaskSubmissionControlWire as C
import qualified Emulation.TaskSubmissionControlWitness as C
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Wire as W
import qualified Replacement.TaskSubmission.Artifact as A
import qualified Replacement.TaskSubmission.Admission as F
import qualified Replacement.StartupComposition.TaskSubmission as Joined

data SourceFiles = SourceFiles
  {sourceDat :: B.ByteString, sourceRun :: B.ByteString, sourceRom :: B.ByteString
  ,sourceA :: B.ByteString, sourceB :: B.ByteString, sourceImage :: B.ByteString, sourceObserver :: B.ByteString}
data ObservedFiles = ObservedFiles {observedReceipt :: B.ByteString, observedWatch :: B.ByteString, observedReset :: B.ByteString}
data Inputs = Inputs SourceFiles Identity P.Program
data QualifiedReceipt = QualifiedReceipt Receipt W.Digest Int (Maybe C.ProbeRecord) (Maybe C.ControlProof)
data WitnessError = SourceRejected | ReceiptDecodeRejected | StatusRejected | IdentityRejected | ResetRejected
  | WatchRejected | ClockRejected | FetchRejected | ReadRejected | WriteRejected | PreimageRejected | ForeignWriterRejected
  | SnapshotRejected | AliasRejected | ContextRejected | CodeRejected | KernelRejected | TaskRejected
  | SubmissionRejected | RunnableRejected | FirstTaskRejected | PrologueRejected | RawWatchRejected | ProgramRejected | ControlRejected
  | ProgramSampleRejected HitIndex P.ProgramError
  | ProgramAdmissionRejected P.ProgramError
  | ControlReasonRejected C.ControlError
  deriving (Eq, Show)

require :: Bool -> WitnessError -> Either WitnessError ()
require condition problem = unless condition (Left problem)
required :: WitnessError -> Maybe a -> Either WitnessError a
required problem = maybe (Left problem) Right
address :: W.Address -> Word32
address (W.Address location) = location
value :: W.WordValue -> Word32
value (W.WordValue number) = number
count :: W.Count -> Word64
count (W.Count number) = number
startupBudget :: W.Count
startupBudget = W.Count 80000000
checkedCounts :: [W.Count] -> Either WitnessError W.Count
checkedCounts counts = do
  let total = sum (map (toInteger . count) counts)
  require (length counts == 2 && total <= toInteger (count startupBudget)) ClockRejected
  pure (W.Count (fromInteger total))
checkedIncrement :: W.Count -> Either WitnessError W.Count
checkedIncrement current = do
  require (current < startupBudget) ClockRejected
  pure (W.Count (count current+1))
index :: HitIndex -> Int
index (HitIndex number) = number
cursor :: Cursor -> Int
cursor (Cursor number) = number
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes contents) = contents
hash :: B.ByteString -> W.Digest
hash = W.Digest . SHA.sha256Hex . SHA.sha256
physical :: Word32 -> Word32
physical number = number .&. 0x3fffffff
at :: WitnessError -> [a] -> Int -> Either WitnessError a
at problem items position = required problem (if position < 0 then Nothing else case drop position items of item:_ -> Just item; [] -> Nothing)
decode :: J.FromJSON a => WitnessError -> B.ByteString -> Either WitnessError a
decode problem raw = do
  document <- either (const (Left problem)) Right (decodeStrictIntegers raw)
  case J.fromJSON document of J.Success result -> Right result; J.Error _ -> Left problem

admitInputs :: SourceFiles -> Either WitnessError Inputs
admitInputs sources = do
  let dat = hash (sourceDat sources); run = hash (sourceRun sources); rom = hash (sourceRom sources)
      loaderA = hash (sourceA sources); loaderB = hash (sourceB sources); image = hash (sourceImage sources)
      observer = hash (sourceObserver sources)
  mapM_ (\(actual,expected) -> require (actual == W.Digest expected) SourceRejected)
    [(dat,"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    ,(run,"efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef")
    ,(rom,"4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be")
    ,(loaderA,"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
    ,(loaderB,"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
    ,(image,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")]
  require (B.length (sourceObserver sources) >= 64 && B.take 4 (sourceObserver sources) == "\x7f\&ELF") SourceRejected
  require (U.word32At (sourceA sources) 0xdabc == 120 && U.word32At (sourceB sources) 0xda80 == 120) SourceRejected
  pure (Inputs sources (Identity run dat rom observer
    [SourceIdentity LoaderA (W.Address 0x40000) (B.length (sourceA sources)) loaderA
    ,SourceIdentity LoaderB (W.Address 0xd0000) (B.length (sourceB sources)) loaderB
    ,SourceIdentity Image4 (W.Address 0x61c400) (B.length (sourceImage sources)) image]) P.stockProgram)

admitOwnedInputs :: SourceFiles -> P.NativeInputs -> A.StagedCandidate -> Either WitnessError Inputs
admitOwnedInputs sources native candidate = do
  Inputs _ identity _ <- admitInputs sources
  program <- either (Left . ProgramAdmissionRejected) Right (P.ownedProgram
    (P.CanonicalImages (sourceDat sources) (sourceA sources) (sourceB sources) (sourceImage sources)) native candidate)
  pure (Inputs sources (programIdentity sources program identity) program)

admitJoinedInputs :: SourceFiles -> P.JoinedInputs -> Joined.StagedCandidate -> Either WitnessError Inputs
admitJoinedInputs sources native candidate = do
  Inputs _ identity _ <- admitInputs sources
  program <- either (Left . ProgramAdmissionRejected) Right (P.joinedProgram
    (P.CanonicalImages (sourceDat sources) (sourceA sources) (sourceB sources) (sourceImage sources)) native candidate)
  pure (Inputs sources (programIdentity sources program identity) program)

programIdentity :: SourceFiles -> P.Program -> Identity -> Identity
programIdentity sources program (Identity run dat rom observer _) =
  Identity run (P.programUpdateDigest program dat) rom observer
    [SourceIdentity kind (W.Address (fromIntegral base)) (B.length actual) (hash actual) | (kind,base,actual) <- programSourceBytes sources program]

programSourceBytes :: SourceFiles -> P.Program -> [(SourceKind,Word32,B.ByteString)]
programSourceBytes sources program = P.programSources program
  [(LoaderA,0x40000,sourceA sources),(LoaderB,0xd0000,sourceB sources),(Image4,0x61c400,sourceImage sources)]

codeSources :: Inputs -> [(SourceKind,Word32,B.ByteString)]
codeSources (Inputs sources _ program) = programSourceBytes sources program
sourceWord :: Inputs -> Word32 -> Either WitnessError Word32
sourceWord inputs pc = do
  (_,base,contents) <- required CodeRejected (case filter (\(_,start,body) -> pc >= start && toInteger pc+4 <= toInteger start+toInteger (B.length body)) (codeSources inputs) of [found] -> Just found; _ -> Nothing)
  require (pc `mod` 4 == 0 && (base /= 0x61c400 || pc < 0x89ea8c)) CodeRejected
  pure (U.word32At contents (fromIntegral (pc-base)))
immutable :: Inputs -> Word32 -> Word64 -> Bool
immutable inputs target size = any (\(_,base,contents) -> let end = if base == 0x61c400 then 0x89ea8c else toInteger base+toInteger (B.length contents)
  in toInteger target < end && toInteger base < toInteger target+toInteger size) (codeSources inputs)

layout :: KernelCore -> (Word32,Word32,Word32,Word32,Word32,Word32,Word32,Word32,Word32,Word32)
layout PrimaryKernel = (0x4005beac,0x4005ff0c,0x400622d4,0x4005d178,0x400624b4,0x400545cc,0x400545a4,0x40054768,0x40054760,0x400545c8)
layout SecondaryKernel = (0x400ebe6c,0x400efecc,0x400f2294,0x400ed138,0x400f2474,0x400e458c,0x400e4564,0x400e4728,0x400e4720,0x400e4588)

expectedSites :: Inputs -> ReceiptProtocol -> M.Map W.Address Boundary
expectedSites (Inputs _ _ program) = P.programSites program
expectedClasses :: Inputs -> ReceiptProtocol -> [W.WatchClass]
expectedClasses inputs@(Inputs _ _ program) protocol = P.programClasses program protocol (codeSources inputs)

unionRanges :: [[Word64]] -> [[Word64]]
unionRanges = reverse . foldl merge [] . sort
  where
    merge ([low,high]:rest) [start,end] | start <= high = [low,max high end]:rest
    merge accumulated item = item:accumulated

allViews :: Sample -> [View]
allViews sample = concatMap kernelViews (sampleKernels sample) ++ sampleLocal sample
newtype Memory = Memory (M.Map Word32 Word8)
readBytes :: WitnessError -> Memory -> Word32 -> Int -> Either WitnessError B.ByteString
readBytes problem (Memory contents) start size = B.pack <$> mapM (\offset -> required problem (M.lookup (start+fromIntegral offset) contents)) [0..size-1]
readWord :: WitnessError -> Memory -> Word32 -> Either WitnessError Word32
readWord problem memory start = (`U.word32At` 0) <$> readBytes problem memory start 4
mergeView :: Memory -> View -> Either WitnessError Memory
mergeView (Memory memory) view = Memory <$> foldM insert memory (zip [address (viewPhysical view)..] (B.unpack (bytes (viewBytes view))))
  where insert contents (target,byte) = do
          require (maybe True (==byte) (M.lookup target contents)) SnapshotRejected
          pure (M.insert target byte contents)
validateViews :: Sample -> Memory -> Either WitnessError Memory
validateViews sample memory = do
  mapM_ (validateView (contextState (sampleContext sample))) (allViews sample)
  foldM mergeView memory (allViews sample)
validateView :: W.Context -> View -> Either WitnessError ()
validateView state view = do
  let contents = bytes (viewBytes view); start = address (viewPhysical view); expected = hash contents
  require (not (B.null contents) && toInteger start+toInteger (B.length contents) <= 0x20000000
    && viewAliases view == replicate 2 (replicate 2 expected)) AliasRejected
  mapM_ (\banks -> do
    require (length banks == 2) ContextRejected
    mapM_ (\(W.WordValue bank) -> if bank .&. 1 == 0 || (bank `shiftR` 2) .&. 31 == 0 then pure () else do
      let code = (bank `shiftR` 2) .&. 31; low = fromIntegral (bank .&. 0xfffff000) :: Word64; size = 512 `shiftL` fromIntegral code
      require (code >= 3 && code <= 14 && low `mod` size == 0 && low+size <= 0x100000000) ContextRejected
      mapM_ (\alias -> let begin = fromIntegral start+alias; end = begin+fromIntegral (B.length contents)
        in require (end <= low || begin >= low+size) ContextRejected) [0,0x40000000]) banks) (W.contextTcm state)

checkTask :: Memory -> KernelCore -> Int -> Task -> Either WitnessError ()
checkTask memory core i task = do
  let (cbase,tbase,_,_,_,_,_,_,_,_) = layout core; c = cbase+40*fromIntegral i; t = tbase+56*fromIntegral i
  require (taskId task == TaskId (fromIntegral i+1) && taskCreation task == W.Address c && taskNode task == W.Address t) TaskRejected
  let expected = [(taskCreationPointer task,8),(taskSavedSp task,24),(taskResumePc task,28)]
  mapM_ (\(observed,offset) -> readWord TaskRejected memory (physical t+offset) >>= \raw -> require (raw == address observed) TaskRejected) expected
  mapM_ (\(observed,offset) -> readWord TaskRejected memory (physical c+offset) >>= \raw -> require (raw == value observed) TaskRejected)
    [(taskFlags task,0),(taskArgument task,4),(taskPriority task,12),(taskStackBytes task,16),(taskCreated task,36),(taskCreationId task,32)]
  mapM_ (\(observed,offset) -> readWord TaskRejected memory (physical c+offset) >>= \raw -> require (raw == address observed) TaskRejected) [(taskEntry task,8),(taskStackBase task,20)]
  raw <- readBytes TaskRejected memory (physical t+12) 3
  require (B.unpack raw == [taskState task,taskEffectivePriority task,taskPending task]) TaskRejected

checkKernel :: Memory -> Kernel -> Either WitnessError ()
checkKernel memory kernel = do
  let core = kernelCore kernel; (c,t,ids,aux,nodes,domains,control,selected,current,domainPointer) = layout core
      prefix = [(CreationRecords,physical c,4800),(TaskNodes,physical t,6720),(IdMap,physical ids,480)
        ,(AuxiliaryRecords,physical aux,1440),(AuxiliaryNodes,physical nodes,1920),(Domains,physical domains,264),(Control,physical control,456)]
      shape view = (viewObject view,address (viewPhysical view),B.length (bytes (viewBytes view)))
  tails <- mapM (\(domain,kind,field,size) -> do pointer <- readWord KernelRejected memory (physical domain+field); pure (kind,physical pointer,size))
    [(domains+offset,kind,field,size) | offset <- [0,132],(kind,field,size) <- [(QueueHeads,0x20,128),(Bitmap,0x24,4),(SelectedObject,0x2c,4),(Current,0x28,4)]]
  require (map shape (kernelViews kernel) == prefix ++ tails && length (kernelTasks kernel) == 120) KernelRejected
  mapM_ (uncurry (checkTask memory core)) (zip [0..] (kernelTasks kernel))
  let expectedDomain = case core of PrimaryKernel -> domains; SecondaryKernel -> domains+132
  mapM_ (\(cell,expected) -> readWord KernelRejected memory (physical cell) >>= \actual -> require (actual == address expected) KernelRejected)
    [(selected,kernelSelected kernel),(current,kernelCurrent kernel),(domainPointer,kernelDomain kernel)]
  require (kernelDomain kernel == W.Address expectedDomain) KernelRejected

sampleAt :: Receipt -> HitIndex -> Either WitnessError Sample
sampleAt receipt position = required SnapshotRejected (case filter ((==position) . sampleHit) (receiptSamples receipt) of [sample] -> Just sample; _ -> Nothing)
kernelAt :: Sample -> KernelCore -> Either WitnessError Kernel
kernelAt sample core = required KernelRejected (case filter ((==core) . kernelCore) (sampleKernels sample) of [kernel] -> Just kernel; _ -> Nothing)
localMemory :: Sample -> Either WitnessError Memory
localMemory sample = foldM mergeView (Memory M.empty) (allViews sample)
contextRegister :: Int -> Sample -> Either WitnessError Word32
contextRegister position sample = value <$> at ContextRejected (W.contextRegisters (contextState (sampleContext sample))) position
checkInitial :: Sample -> Memory -> Either WitnessError ()
checkInitial sample memory = mapM_ initialize [PrimaryKernel,SecondaryKernel]
  where
    initialize core = do
      kernel <- kernelAt sample core
      let (cbase,tbase,ids,aux,_,_,_,_,_,_) = layout core
          defaultStack = case core of PrimaryKernel -> 0x40059280; SecondaryKernel -> 0x400e9240
      require (kernelSelected kernel == W.Address 0 && kernelCurrent kernel == W.Address 0) KernelRejected
      mapM_ (\i -> do
        let c = physical (cbase+40*i); t = physical (tbase+56*i)
        actual <- mapM (readWord KernelRejected memory . (c+)) [0,4..36]
        require (actual == [0,i,0,15,4,defaultStack,0,0,i+1,0]) KernelRejected
        readWord KernelRejected memory (physical ids+4*i) >>= \ident -> require (ident == i+1) KernelRejected
        readBytes KernelRejected memory (physical aux+12*i) 12 >>= \contents -> require (contents == B.replicate 12 0) KernelRejected
        readWord KernelRejected memory (t+8) >>= \pointer -> require (pointer == cbase+40*i) KernelRejected
        status <- readBytes KernelRejected memory (t+12) 3
        require (B.index status 0 == 0 && B.index status 1 == 15 && B.index status 2 .&. 7 == 0) KernelRejected) [0..119]

legacyLocalShape :: Receipt -> Sample -> Memory -> Either WitnessError ()
legacyLocalShape receipt sample memory = do
  active <- case filter (\submitted -> submissionCore submitted == core && submissionEntryHit submitted <= position
    && maybe True (position <=) (submissionReturnHit submitted)) (receiptSubmissions receipt) of
      [] -> pure []
      [submitted] -> do
        let TaskId ident = submissionId submitted; i = fromIntegral ident-1
            sp = address (submissionSp submitted); descriptor = physical (address (submissionDescriptor submitted))
        require (sp >= 56) SnapshotRejected
        base <- readWord SnapshotRejected memory (0xa5b204+8*i)
        size <- readWord SnapshotRejected memory (0xa5b208+8*i)
        let pool = if base == 0 then [] else [(Stack,physical base,fromIntegral size)]
        pure ([(Descriptor,descriptor,28),(Stack,physical (sp-56),88),(Wrapper,0xa5b204+8*i,8),(Storage,0xa0fc44+12*i,12)]
          ++ [(Descriptor,descriptor,28) | sampleBoundary sample == SubmissionEntry] ++ pool)
      _ -> Left SnapshotRejected
  entries <- fmap concat $ mapM (\first -> if firstCore first == core && (firstEntryHit first == Just position || firstEnteredHit first == Just position)
    then do require (address (firstStackTop first) >= 64) SnapshotRejected
            pure [(Stack,physical (address (firstStackTop first)-64),96)]
    else pure []) (receiptFirstTasks receipt)
  let base = [(SharedDomains,0xc0800,256),(InterfaceRoots,0x96bd20,8),(InterfaceTable,0x4dae0,0x162c),(InterfaceTable,0xddaa0,0x162c)]
      actual = [(viewObject view,address (viewPhysical view),B.length (bytes (viewBytes view))) | view <- sampleLocal sample]
  require (sort actual == sort (base ++ active ++ entries)) SnapshotRejected
  where core = contextCore (sampleContext sample); position = sampleHit sample

data CallerBinding = CallerBinding {bindingTask :: TaskId, bindingDescriptor :: W.Address, bindingSp :: W.Address}
data CallerBacking = CallerBacking {backingLow :: W.Address, backingHigh :: W.Address}
data PublishedSlot = PublishedSlot W.Address W.Address
publishedSlots :: [PublishedSlot]
publishedSlots = [PublishedSlot (W.Address (table+offset)) (W.Address target) | (table,slots) <-
  [(0x4dae0,[(0,0x4d094),(0xc,0x44f2c),(0x80,0x4d39c),(0x1618,0x412bc),(0x161c,0x412cc),(0x1620,0x412e8),(0x1624,0x41304),(0x164,0x41140),(0x168,0x41164)])
  ,(0xddaa0,[(0,0xdd054),(0xc,0xd4eec),(0x80,0xdd360),(0x1618,0xd12bc),(0x161c,0xd12cc),(0x1620,0xd12e8),(0x1624,0xd1304),(0x164,0xd1140),(0x168,0xd1164)])],(offset,target) <- slots]
activeBinding :: Receipt -> Maybe C.ProbeRecord -> Sample -> Either WitnessError (Maybe CallerBinding)
activeBinding receipt probe sample = case probe of
  Just controlled | core == PrimaryKernel && sampleHit sample >= C.probeInitialHit controlled ->
    pure (Just (CallerBinding (C.probeTask controlled) (C.probeDescriptor controlled) (W.contextSp (contextState (C.probeConfigured controlled)))))
  _ -> case filter (\submitted -> submissionCore submitted == core && submissionEntryHit submitted <= sampleHit sample
    && maybe True (sampleHit sample <=) (submissionReturnHit submitted)) (receiptSubmissions receipt) of
      [] -> pure Nothing
      [submitted] -> pure (Just (CallerBinding (submissionId submitted) (submissionDescriptor submitted) (submissionSp submitted)))
      _ -> Left SnapshotRejected
  where core = contextCore (sampleContext sample)
callerBacking :: Sample -> CallerBinding -> Either WitnessError CallerBacking
callerBacking sample binding
  | bindingSp binding == W.Address bootSp = pure (CallerBacking (W.Address bootLow) (W.Address (physical bootSp+32)))
  | otherwise = do
      kernel <- kernelAt sample core
      current <- required SnapshotRejected (case filter ((==kernelCurrent kernel) . taskNode) (kernelTasks kernel) of [task] -> Just task; _ -> Nothing)
      let low = physical (address (taskStackBase current)); high = toInteger low+toInteger (value (taskStackBytes current))
      require (taskId current == TaskId 104 && taskStackBytes current == W.WordValue 1024
        && low >= 0xa10200 && high <= 0xa5a200 && toInteger (physical (address (bindingSp binding)))+16 == high) SnapshotRejected
      pure (CallerBacking (W.Address low) (W.Address (fromInteger high)))
  where
    core = contextCore (sampleContext sample)
    (bootSp,bootLow) = case core of PrimaryKernel -> (0x40063420,0x62c54); SecondaryKernel -> (0x400f33e0,0xf2c14)
currentLocalShape :: Inputs -> Receipt -> Maybe C.ProbeRecord -> Sample -> Memory -> Either WitnessError ()
currentLocalShape inputs@(Inputs _ _ program) receipt probe sample memory = do
  binding <- activeBinding receipt probe sample
  active <- case binding of
    Nothing -> pure []
    Just current -> do
      let TaskId ident = bindingTask current; i = fromIntegral ident-1
          descriptor = physical (address (bindingDescriptor current))
      pool <- readWord SnapshotRejected memory (0xa0fc44+12*i)
      size <- readWord SnapshotRejected memory (0xa0fc48+12*i)
      reserved <- readWord SnapshotRejected memory (0xa0fc4c+12*i)
      pending <- if sampleBoundary sample == NativeWrite && P.programStorageSizeStore program (W.contextPc (contextState (sampleContext sample)))
        then either (Left . ProgramSampleRejected (sampleHit sample)) Right (P.programGuestStore program sample)
        else pure Nothing
      let publishing = size == 0 && reserved == 0 && case pending of
            Just store -> physical (address (P.guestStoreAddress store)) == 0xa0fc48+12*i && P.guestStoreValue store == W.WordValue 1024
            Nothing -> False
      require (pool == 0 || ((size == 1024 || publishing) && physical pool >= 0xa10200 && toInteger (physical pool)+1024 <= 0xa5a200)) SnapshotRejected
      backing <- callerBacking sample current
      let low = address (backingLow backing); high = address (backingHigh backing)
          pools = if pool == 0 || publishing then [] else [(Stack,physical pool,1024)]
      parameters <- if sampleCallTaken sample /= Just False && sampleBoundary sample `elem` [AuxiliaryCall,CreationCall]
        then do
          pointer <- contextRegister 1 sample
          let sizeBytes = if sampleBoundary sample == AuxiliaryCall then 8 else 24
          require (pointer `mod` 4 == 0 && physical pointer >= low && toInteger (physical pointer)+toInteger sizeBytes <= toInteger high) SnapshotRejected
          pure [(Parameters,physical pointer,sizeBytes)]
        else pure []
      pure ([(Descriptor,descriptor,28),(Wrapper,0xa5b204+8*i,8),(Storage,0xa0fc44+12*i,12),(Stack,low,fromIntegral (high-low))] ++ pools ++ parameters)
  entries <- fmap concat $ mapM (\first -> if firstCore first == core && (firstEntryHit first == Just position || firstEnteredHit first == Just position)
    then do require (address (firstStackTop first) >= 64) SnapshotRejected
            pure [(Stack,physical (address (firstStackTop first)-64),96)]
    else pure []) (receiptFirstTasks receipt)
  let base = [(SharedDomains,0xc0800,256),(InterfaceRoots,0x96bd20,8),(InterfaceTable,0x4dae0,0x162c),(InterfaceTable,0xddaa0,0x162c)
        ,(ErrorState,0xa0f82c,4),(Callback,0x89ea8c,4),(CursorObject,0x89ea98,4),(LockObject,0x9c280c,4),(ClassTable,0x7cad9c,420)
        ,(LogCallback,0x4f15c,4),(LogMode,0x4f25c,4),(LogEnable,0x4f5e4,38)]
        ++ (if receiptSchema receipt == JoinedTaskProtocol then
          [(CriticalCounter,0xa0f824,4),(CriticalLock,0xc0804,4),(BootLogControl,0x14c60,12),(BootBss,0x14c80,32),(BootMode,0xa00c0,4)] else [])
      actual = [(viewObject view,address (viewPhysical view),B.length (bytes (viewBytes view))) | view <- sampleLocal sample]
  require (sort actual == sort (base ++ active ++ entries)) SnapshotRejected
  (_,imageBase,image) <- required CodeRejected (case filter (\(kind,_,_) -> kind == Image4) (codeSources inputs) of [found] -> Just found; _ -> Nothing)
  classTable <- readBytes SnapshotRejected memory 0x7cad9c 420
  require (classTable == B.take 420 (B.drop (fromIntegral (0x7cad9c-imageBase)) image)) SnapshotRejected
  where core = contextCore (sampleContext sample); position = sampleHit sample
checkLocalShape :: Inputs -> Receipt -> Maybe C.ProbeRecord -> Sample -> Memory -> Either WitnessError ()
checkLocalShape inputs receipt probe sample memory = do
  case receiptSchema receipt of
    LegacyStockProtocol -> legacyLocalShape receipt sample memory
    TaskSubmissionProtocol -> currentLocalShape inputs receipt probe sample memory
    JoinedTaskProtocol -> do
      currentLocalShape inputs receipt probe sample memory
      control <- mapM (readWord SnapshotRejected memory) [0x14c60,0x14c64,0x14c68]
      case control of
        [buffer,first,second] -> require (buffer == 0x400a0040 && first <= 16 && first `mod` 2 == 0
          && second >= 16 && second <= 32 && second `mod` 2 == 0) SnapshotRejected
        _ -> Left SnapshotRejected
      loader <- sourceWord inputs 0xd000c
      entry <- sourceWord inputs 0x61c40c
      mode <- readWord SnapshotRejected memory 0xa00c0
      bss <- mapM (readWord SnapshotRejected memory . (0x14c80+)) [0,4..28]
      require (bss == [loader,entry,0,0,mode,0,0,0]) SnapshotRejected
  mapM_ (\(PublishedSlot cell target) -> readWord SnapshotRejected memory (address cell) >>= \actual -> require (actual == address target) SnapshotRejected) publishedSlots


checkSample :: Inputs -> Receipt -> Maybe C.ProbeRecord -> Sample -> Memory -> Either WitnessError Memory
checkSample inputs@(Inputs _ _ program) receipt probe sample previous = do
  let context = sampleContext sample; state = contextState context
  total <- checkedCounts (W.contextCounts state)
  hit <- at ContextRejected (receiptHits receipt) (index (sampleHit sample))
  require (length (W.contextRegisters state) == 13 && length (W.contextTcm state) == 2
    && W.contextIdentities state `elem` W.recordedCoreIdentities && contextCore context == hitCore hit
    && W.contextPc state == hitAddress hit && sampleBoundary sample == hitBoundary hit
    && M.lookup (W.contextPc state) (expectedSites inputs (receiptSchema receipt)) == Just (sampleBoundary sample)
    && total == hitInstructions hit
    && W.contextRegisters state == hitRegisters hit && W.contextSp state == hitSp hit && W.contextLr state == hitLr hit && W.contextCpsr state == hitCpsr hit) ContextRejected
  instruction <- sourceWord inputs (address (W.contextPc state))
  require (sampleCodeWord sample == W.WordValue instruction && map kernelCore (sampleKernels sample) == [PrimaryKernel,SecondaryKernel]) CodeRejected
  memory <- validateViews sample previous
  mapM_ (checkKernel memory) (sampleKernels sample)
  checkLocalShape inputs receipt probe sample memory
  either (\problem -> Left (if receiptSchema receipt == LegacyStockProtocol then SubmissionRejected else ProgramSampleRejected (sampleHit sample) problem)) Right (P.checkProgramSample program receipt sample)
  readWord SnapshotRejected memory 0x96bd20 >>= \root -> require (root == 0x4004dae0) SnapshotRejected
  readWord SnapshotRejected memory 0x96bd24 >>= \root -> require (root == 0x400ddaa0) SnapshotRejected
  pure memory

data WrapperFailure = ActivationMasked | CreationDuplicate | ReservationHeld deriving (Eq, Show)
data Publication = NoPublication | PendingPublication | Published deriving (Eq, Show)
data FailurePhase = NoFailure | BeforeFailure WrapperFailure | FailureObserved WrapperFailure | FailureLogging WrapperFailure
  | FailureLogComplete WrapperFailure | FailureStored WrapperFailure | FailureReporting WrapperFailure | FailureReported WrapperFailure deriving (Eq, Show)
data CriticalToken = InterruptsEnabled | InterruptsMasked deriving (Eq, Show)
criticalWord :: CriticalToken -> Word32
criticalWord InterruptsEnabled = 0
criticalWord InterruptsMasked = 0x80
data CriticalPhase = CriticalNotNeeded | CriticalPending | CriticalEntering | CriticalHeld CriticalToken | CriticalExiting CriticalToken | CriticalReleased deriving (Eq, Show)
criticalHeld :: CriticalPhase -> Bool
criticalHeld (CriticalHeld _) = True
criticalHeld _ = False
data Invocation = Invocation
  {invocationKind :: F.EntryPoint, invocationSp :: W.Address, invocationLow :: W.Address, invocationTask :: TaskId
  ,invocationPublication :: Publication, invocationFailure :: FailurePhase, invocationCritical :: CriticalPhase, invocationInitialToken :: CriticalToken}
data StoppedPoint = StoppedPoint W.Address W.Count
data PendingStore = PendingStore W.Address P.GuestStore
data WriteAuthority = RetainedWriter | OwnedWriter P.StoreComponent deriving (Eq)
writeAuthority :: P.Program -> W.Address -> WriteAuthority
writeAuthority program location = maybe RetainedWriter OwnedWriter (P.programStoreComponent program location)
data Progress = Progress
  {progressCounts :: [W.Count], progressFetches :: M.Map W.Core (W.Address,W.Count), progressMemory :: Memory
  ,progressInvocations :: M.Map W.Core Invocation, progressStops :: M.Map W.Core StoppedPoint, progressStores :: M.Map W.Core PendingStore}
failureWord :: WrapperFailure -> Word32
failureWord ActivationMasked = 0xffffffe7
failureWord CreationDuplicate = 0xffffffd7
failureWord ReservationHeld = 0xffffffee
expectedFailure :: Receipt -> Maybe C.ProbeRecord -> Sample -> Maybe WrapperFailure
expectedFailure receipt probe sample
  | any ((==sampleHit sample) . submissionEntryHit) (receiptSubmissions receipt) = Just ActivationMasked
  | otherwise = case C.probeCase <$> probe of
      Just C.FreshMasked -> Just ActivationMasked
      Just C.ReusedRecord -> Just CreationDuplicate
      Just C.PartialPair -> Just CreationDuplicate
      Just C.ReservedRefusal -> Just ReservationHeld
      Just C.CallbackUnreturned -> Just ReservationHeld
      _ -> Nothing
ownedPoint :: Inputs -> ReceiptProtocol -> W.Address -> Bool
ownedPoint (Inputs _ _ program) protocol = P.programOwnedPoint program protocol
noteSnapshot :: Inputs -> Receipt -> Maybe C.ProbeRecord -> Sample -> Progress -> Either WitnessError Progress
noteSnapshot inputs@(Inputs _ _ program) receipt probe sample progress = do
  noted <- noteInvocation inputs receipt probe sample progress
  if receiptSchema receipt == LegacyStockProtocol then pure noted else do
    let core = watchCore (contextCore (sampleContext sample)); state = contextState (sampleContext sample)
    counter <- at ClockRejected (W.contextCounts state) (coreIndex (contextCore (sampleContext sample)))
    require (M.notMember core (progressStops noted)) ContextRejected
    require (M.notMember core (progressStores noted)) WriteRejected
    store <- either (Left . ProgramSampleRejected (sampleHit sample)) Right (P.programGuestStore program sample)
    case store of
      Nothing -> pure ()
      Just actual -> do
        invocation <- required WriteRejected (M.lookup core (progressInvocations noted))
        _ <- applyStore (writeAuthority program (W.contextPc state)) invocation actual
        pure ()
    let stops = if ownedPoint inputs (receiptSchema receipt) (W.contextPc state)
          then M.insert core (StoppedPoint (W.contextPc state) counter) (progressStops noted) else progressStops noted
    pure noted {progressStops=stops
      ,progressStores=maybe (progressStores noted) (\actual -> M.insert core (PendingStore (W.contextPc state) actual) (progressStores noted)) store}
noteInvocation :: Inputs -> Receipt -> Maybe C.ProbeRecord -> Sample -> Progress -> Either WitnessError Progress
noteInvocation (Inputs _ _ program) receipt probe sample progress
  | receiptSchema receipt == LegacyStockProtocol = pure progress
  | sampleBoundary sample `elem` [SubmissionEntry,ReleaseEntry] = do
      let kind = if sampleBoundary sample == SubmissionEntry then F.Submission else F.Release
          incomingSp = W.contextSp (contextState (sampleContext sample))
          F.StackOffset offset = P.programFrameLow program kind
          low = toInteger (physical (address incomingSp))+offset
      require (M.notMember core (progressInvocations progress) && low >= 0 && low <= toInteger (physical (address incomingSp))) SubmissionRejected
      memory <- localMemory sample
      argument <- contextRegister 0 sample
      (ident,publication,failure) <- if kind == F.Submission
        then do
          number <- readWord SubmissionRejected memory (physical argument)
          requested <- readWord SubmissionRejected memory (physical argument+12)
          require (number >= 1 && number <= 120 && requested == 1024) SubmissionRejected
          base <- readWord SubmissionRejected memory (0xa5b204+8*(number-1))
          size <- readWord SubmissionRejected memory (0xa5b208+8*(number-1))
          pure (TaskId (fromIntegral number),if base == 0 && size == 0 then PendingPublication else NoPublication,maybe NoFailure BeforeFailure (expectedFailure receipt probe sample))
        else do require (argument < 120) SubmissionRejected; pure (TaskId (fromIntegral argument+1),NoPublication,NoFailure)
      let token = if testBit (value (W.contextCpsr (contextState (sampleContext sample)))) 7 then InterruptsMasked else InterruptsEnabled
          invocation = Invocation kind incomingSp (W.Address (fromInteger low)) ident publication failure (if kind == F.Submission then CriticalPending else CriticalNotNeeded) token
      pure progress {progressInvocations=M.insert core invocation (progressInvocations progress)}
  | isOuterReturn = do
      case M.lookup core (progressInvocations progress) of
        Nothing -> require (sampleBoundary sample /= WrapperReturn) SubmissionRejected
        Just invocation -> do
          require (invocationPublication invocation /= PendingPublication && case invocationFailure invocation of NoFailure -> True; FailureReported _ -> True; _ -> False) SubmissionRejected
          require (W.contextSp (contextState (sampleContext sample)) == invocationSp invocation) SubmissionRejected
          require (invocationCritical invocation == (if invocationKind invocation == F.Submission then CriticalReleased else CriticalNotNeeded)) SubmissionRejected
      pure progress {progressInvocations=M.delete core (progressInvocations progress)}
  | sampleCallTaken sample == Just False = pure progress
  | otherwise = case M.lookup core (progressInvocations progress) of
      Nothing -> pure progress
      Just invocation -> do
        next <- case sampleBoundary sample of
          CriticalEnterCall -> do
            require (invocationCritical invocation == CriticalPending) SubmissionRejected
            pure invocation {invocationCritical=CriticalEntering}
          CriticalEnterReturn -> do
            require (invocationCritical invocation == CriticalEntering) SubmissionRejected
            token <- contextRegister 0 sample
            require (token == criticalWord (invocationInitialToken invocation) && testBit (value (W.contextCpsr (contextState (sampleContext sample)))) 7) SubmissionRejected
            pure invocation {invocationCritical=CriticalHeld (invocationInitialToken invocation)}
          CriticalExitCall -> case invocationCritical invocation of
            CriticalHeld token -> do
              actual <- contextRegister 0 sample
              require (actual == criticalWord token && testBit (value (W.contextCpsr (contextState (sampleContext sample)))) 7) SubmissionRejected
              pure invocation {invocationCritical=CriticalExiting token}
            _ -> Left SubmissionRejected
          CriticalExitReturn -> case invocationCritical invocation of
            CriticalExiting token -> do
              require (value (W.contextCpsr (contextState (sampleContext sample))) .&. 0x80 == criticalWord token) SubmissionRejected
              pure invocation {invocationCritical=CriticalReleased}
            _ -> Left SubmissionRejected
          ReservationCall -> do
            require (criticalHeld (invocationCritical invocation) && invocationPublication invocation == Published) SubmissionRejected
            pure invocation
          AuxiliaryCall -> held invocation
          FillCall -> held invocation
          CacheCall -> held invocation
          CreationCall -> held invocation
          ActivationCall -> do require (invocationCritical invocation == CriticalReleased) SubmissionRejected; pure invocation
          ReservationReturn -> serviceResult ReservationHeld invocation
          CreationReturn -> serviceResult CreationDuplicate invocation
          ActivationReturn -> serviceResult ActivationMasked invocation
          LogCall -> do
            require (invocationCritical invocation == CriticalReleased) SubmissionRejected
            case invocationFailure invocation of
              FailureObserved failure -> pure invocation {invocationFailure=FailureLogging failure}
              _ -> Left SubmissionRejected
          LogReturn -> case invocationFailure invocation of
            FailureLogging failure -> pure invocation {invocationFailure=FailureLogComplete failure}
            _ -> Left SubmissionRejected
          ErrorCallbackCall -> case invocationFailure invocation of
            FailureStored failure -> pure invocation {invocationFailure=FailureReporting failure}
            _ -> Left SubmissionRejected
          ErrorCallbackReturn -> case invocationFailure invocation of
            FailureReporting failure -> pure invocation {invocationFailure=FailureReported failure}
            _ -> Left SubmissionRejected
          _ -> pure invocation
        pure progress {progressInvocations=M.insert core next (progressInvocations progress)}
  where
    core = watchCore (contextCore (sampleContext sample))
    isOuterReturn = sampleBoundary sample `elem` [WrapperReturn,ReleaseReturn] || maybe False ((==Just (sampleHit sample)) . C.probeReturnHit) probe
    held invocation = do
      require (criticalHeld (invocationCritical invocation) && testBit (value (W.contextCpsr (contextState (sampleContext sample)))) 7) SubmissionRejected
      pure invocation
    serviceResult failure invocation = do
      status <- contextRegister 0 sample
      if status == 0 then pure invocation else do
        require (status == failureWord failure && invocationFailure invocation == BeforeFailure failure) SubmissionRejected
        pure invocation {invocationFailure=FailureObserved failure}
clockTotal :: Progress -> Either WitnessError W.Count
clockTotal = checkedCounts . progressCounts
checkFetchShape :: W.Address -> Word64 -> W.MemoryEvent -> Either WitnessError ()
checkFetchShape pc target event =
  require (W.eventSource event == W.CpuFetch && W.eventAddress event == fromIntegral (address pc)
    && target == fromIntegral (physical (address pc)) && W.eventSize event == 4
    && W.eventValue event == Nothing && W.eventBefore event == Nothing) FetchRejected
eventShape :: Inputs -> ReceiptProtocol -> W.MemoryEvent -> Either WitnessError (W.Core,W.Address,W.Count,Word64)
eventShape inputs@(Inputs _ _ program) protocol event = do
  let malformed = case W.eventSource event of W.CpuFetch -> FetchRejected; W.CpuRead -> ReadRejected; _ -> WriteRejected
  core <- required FetchRejected (W.eventCore event); pc <- required FetchRejected (W.eventPc event)
  clock <- required ClockRejected (W.eventInstructions event); target <- required malformed (W.eventPhysical event)
  require (clock <= startupBudget) ClockRejected
  require (W.eventByteOrder event == Just W.Little && W.eventAddress event <= 0xffffffff
    && target <= 0xffffffff && physical (fromIntegral (W.eventAddress event)) == fromIntegral target) malformed
  require (any (\classification -> case W.classRange classification of
      [start,end] -> target >= start && toInteger target+toInteger (W.eventSize event) <= toInteger end
        && W.eventSource event `elem` W.classKinds classification
      _ -> False) (expectedClasses inputs protocol)) WatchRejected
  _ <- sourceWord inputs (address pc)
  require (P.programAllowsPc program pc) CodeRejected
  pure (core,pc,clock,target)
readByte :: Inputs -> Memory -> Word32 -> Either WitnessError Word8
readByte inputs (Memory memory) location = case M.lookup location memory of
  Just actual -> pure actual
  Nothing -> case [B.index body (fromIntegral (location-base)) | (_,base,body) <- codeSources inputs
      , location >= base, toInteger location < toInteger base+toInteger (B.length body)] of
    [actual] -> pure actual
    _ -> Left ReadRejected

advance :: Inputs -> Receipt -> Progress -> W.MemoryEvent -> Either WitnessError Progress
advance inputs@(Inputs _ _ program) receipt progress event = do
  require (W.eventSource event /= W.HostWrite && W.eventSource event /= W.ModelBusWrite) ForeignWriterRejected
  (core,pc,clock,target) <- eventShape inputs (receiptSchema receipt) event
  case W.eventSource event of
    W.CpuFetch -> do
      checkFetchShape pc target event
      nextTotal <- clockTotal progress >>= checkedIncrement
      require (clock == nextTotal) ClockRejected
      let position = case core of W.Primary -> 0; W.Secondary -> 1
      current <- at ClockRejected (progressCounts progress) position
      if receiptSchema receipt == LegacyStockProtocol then pure () else do
        case M.lookup core (progressStops progress) of
          Nothing -> require (not (ownedPoint inputs (receiptSchema receipt) pc)) FetchRejected
          Just (StoppedPoint stoppedPc settled) -> require (pc == stoppedPc && current == settled) FetchRejected
        case M.lookup core (progressStores progress) of
          Nothing -> pure ()
          Just (PendingStore expectedPc _) -> require (pc == expectedPc) WriteRejected
      incremented <- checkedIncrement current
      let counts = zipWith (\slot currentCount -> if slot == position then incremented else currentCount) [0 :: Int ..] (progressCounts progress)
      pure progress {progressCounts=counts,progressFetches=M.insert core (pc,clock) (progressFetches progress),progressStops=M.delete core (progressStops progress)}
    W.CpuWrite -> do
      require (M.lookup core (progressFetches progress) == Just (pc,clock)) ClockRejected
      require (W.eventSize event `elem` [1,2,4,8] && not (immutable inputs (fromIntegral target) (W.eventSize event))) WriteRejected
      let end = toInteger target+toInteger (W.eventSize event)
          touches cell = toInteger target < toInteger cell+4 && toInteger cell < end
      require (not (any (\(PublishedSlot cell _) -> touches (address cell)) publishedSlots)
        && not (toInteger target < 0x96bd28 && 0x96bd20 < end)) WriteRejected
      effects <- wrapperWrite inputs receipt progress event
      old <- bytes <$> required PreimageRejected (W.eventBefore event); new <- required WriteRejected (W.eventValue event)
      let size = fromIntegral (W.eventSize event); Memory contents = progressMemory progress
      require (B.length old == size && (size == 8 || new < 1 `shiftL` (8*size))) WriteRejected
      mapM_ (\(offset,byte) -> require (maybe True (==byte) (M.lookup (fromIntegral target+offset) contents)) PreimageRejected) (zip [0..] (B.unpack old))
      let replacement = [(fromIntegral target+fromIntegral offset,fromIntegral (new `shiftR` (8*offset))) | offset <- [0..size-1]]
      pure effects {progressMemory=Memory (foldr (uncurry M.insert) contents replacement)}
    W.CpuRead -> do
      require (receiptSchema receipt == JoinedTaskProtocol
        && M.lookup core (progressFetches progress) == Just (pc,clock)) ClockRejected
      require (W.eventSize event `elem` [1,2,4,8] && P.programAllowsRead program event) ReadRejected
      old <- bytes <$> required ReadRejected (W.eventBefore event)
      payload <- required ReadRejected (W.eventValue event)
      let size = fromIntegral (W.eventSize event)
          actual = B.pack [fromIntegral (payload `shiftR` (8*offset)) | offset <- [0..size-1]]
      require (B.length old == size && (size == 8 || payload < 1 `shiftL` (8*size)) && old == actual) ReadRejected
      expected <- B.pack <$> mapM (readByte inputs (progressMemory progress) . (fromIntegral target+)) [0..fromIntegral size-1]
      require (actual == expected) ReadRejected
      pure progress
    W.HostWrite -> Left ForeignWriterRejected
    W.ModelBusWrite -> Left ForeignWriterRejected
    _ -> Left WatchRejected

applyStore :: WriteAuthority -> Invocation -> P.GuestStore -> Either WitnessError Invocation
applyStore authority invocation store = do
  let target = physical (address (P.guestStoreAddress store)); payload = value (P.guestStoreValue store)
      TaskId ident = invocationTask invocation; i = fromIntegral ident-1
      end = toInteger target+4
      scratch = target >= address (invocationLow invocation) && end <= toInteger (physical (address (invocationSp invocation)))
      publication = invocationKind invocation == F.Submission && target == 0xa5b208+8*i
      release = invocationKind invocation == F.Release && target == 0xa0fc4c+12*i && payload == 0
      wrapper = authority `elem` [RetainedWriter,OwnedWriter P.WrapperComponent]
      critical = authority == OwnedWriter P.CriticalComponent && target == 0xa0f824
        && invocationCritical invocation `elem` [CriticalEntering,CriticalExiting (invocationInitialToken invocation)]
      storage = authority == OwnedWriter P.StorageComponent && invocationKind invocation == F.Submission
        && criticalHeld (invocationCritical invocation) && invocationPublication invocation == Published
        && target `elem` [0xa0fc44+12*i,0xa0fc48+12*i,0xa0fc4c+12*i,0x89ea98,0xa5b204+8*i]
  require (target `mod` 4 == 0) WriteRejected
  if publication
    then do
      require (wrapper && payload == 1024 && invocationPublication invocation == PendingPublication && criticalHeld (invocationCritical invocation)) WriteRejected
      pure invocation {invocationPublication=Published}
    else if target == 0xa0f82c
      then case invocationFailure invocation of
        FailureLogComplete failure -> do
          require (wrapper && payload == failureWord failure) WriteRejected
          pure invocation {invocationFailure=FailureStored failure}
        _ -> Left WriteRejected
      else do require (authority == RetainedWriter || scratch || (wrapper && release) || critical || storage) WriteRejected; pure invocation
wrapperWrite :: Inputs -> Receipt -> Progress -> W.MemoryEvent -> Either WitnessError Progress
wrapperWrite (Inputs _ _ program) receipt progress event
  | receiptSchema receipt == LegacyStockProtocol = pure progress
  | maybe True (\pc -> not (P.programOwnsPc program pc) && case P.programFrameProof program of
      Nothing -> address pc < 0x725dd8 || address pc >= 0x72612c
      Just _ -> True) (W.eventPc event) = pure progress
  | otherwise = do
      core <- required WriteRejected (W.eventCore event)
      pc <- required WriteRejected (W.eventPc event)
      invocation <- required WriteRejected (M.lookup core (progressInvocations progress))
      payload <- required WriteRejected (W.eventValue event)
      require (W.eventSize event == 4 && payload <= 0xffffffff && W.eventAddress event <= 0xffffffff) WriteRejected
      let store = P.GuestStore (W.Address (fromIntegral (W.eventAddress event))) (W.WordValue (fromIntegral payload))
          authority = writeAuthority program pc
      if P.programHasStore program pc
        then do
          PendingStore expectedPc expected <- required WriteRejected (M.lookup core (progressStores progress))
          require (pc == expectedPc && store == expected) WriteRejected
        else pure ()
      next <- applyStore authority invocation store
      pure progress {progressInvocations=M.insert core next (progressInvocations progress),progressStores=M.delete core (progressStores progress)}
hostPatch :: Receipt -> M.Map Int C.ControlPatch -> Progress -> (Int,W.MemoryEvent) -> Either WitnessError Progress
hostPatch receipt patches progress (slot,event) = do
  patch <- required ForeignWriterRejected (M.lookup slot patches)
  anchor <- sampleAt receipt (C.patchAfterHit patch)
  let before = C.patchBefore patch; after = C.patchAfter patch
      virtual = address (C.patchAddress patch); target = physical virtual
      old = bytes (viewBytes before); new = bytes (viewBytes after)
      state = contextState (sampleContext anchor)
  require (W.eventSource event == W.HostWrite && W.eventAddress event == fromIntegral virtual
    && W.eventPhysical event == Just (fromIntegral target) && W.eventSize event == 4
    && viewObject before == ControlPatch && viewObject after == ControlPatch
    && viewPhysical before == W.Address target && viewPhysical after == W.Address target
    && B.length old == 4 && B.length new == 4 && old /= new
    && W.eventCore event == Nothing && W.eventPc event == Nothing && W.eventInstructions event == Nothing && W.eventTick event == Nothing
    && maybe True (==W.Little) (W.eventByteOrder event)
    && maybe True (==fromIntegral (U.word32At new 0)) (W.eventValue event)
    && maybe True (==viewBytes before) (W.eventBefore event)) ForeignWriterRejected
  validateView state before
  validateView state after
  actual <- readBytes PreimageRejected (progressMemory progress) target 4
  require (actual == old) PreimageRejected
  let Memory memory = progressMemory progress
      replacement = zip [target..] (B.unpack new)
  pure progress {progressMemory=Memory (foldr (uncurry M.insert) memory replacement)}

replay :: Inputs -> Receipt -> Maybe C.ProbeRecord -> W.Watch -> Either WitnessError Memory
replay inputs@(Inputs sources _ program) receipt probe watch = do
  first <- at SnapshotRejected (receiptSamples receipt) 0
  require (sampleCursor first == Cursor 0) SnapshotRejected
  require (W.oneRecordedModel [W.contextIdentities (contextState (sampleContext sample)) | sample <- receiptSamples receipt]) ContextRejected
  let offset = 0x89ea8c-0x61c400
      initialTail = View Control (W.Address 0x89ea8c) (W.Bytes (B.drop offset (P.programImage4 program (sourceImage sources)))) []
      declared = maybe [] C.probePatches probe
      patches = M.fromList [(cursor (C.patchWatchCursor patch),patch) | patch <- declared]
  require (M.size patches == length declared) ControlRejected
  initial <- mergeView (Memory M.empty) initialTail >>= checkSample inputs receipt probe first
  checkInitial first initial
  started <- noteSnapshot inputs receipt probe first (Progress (W.contextCounts (contextState (sampleContext first))) M.empty initial M.empty M.empty M.empty)
  final <- foldM (step patches) (started,sampleCursor first) (drop 1 (receiptSamples receipt))
  lastSample <- required SnapshotRejected (case reverse (receiptSamples receipt) of sample:_ -> Just sample; [] -> Nothing)
  let expectedCounts = case probe of Nothing -> receiptStartupCounts receipt; Just _ -> Just (W.contextCounts (contextState (sampleContext lastSample)))
      invocations = progressInvocations (fst final)
  require (cursor (snd final) == length (W.watchRecords watch) && Just (progressCounts (fst final)) == expectedCounts) ClockRejected
  require (M.null (progressStores (fst final))) WriteRejected
  case probe of
    Just controlled | C.probeCase controlled == C.CallbackUnreturned ->
      require (M.size invocations == 1 && maybe False ((==FailureReporting ReservationHeld) . invocationFailure) (M.lookup W.Primary invocations)) SubmissionRejected
    _ -> require (M.null invocations) SubmissionRejected
  pure (progressMemory (fst final))
  where
    step patches (progress,previousCursor) sample = do
      let end = cursor (sampleCursor sample); start = cursor previousCursor
          events = zip [start..] (take (end-start) (drop start (W.watchRecords watch)))
          context = sampleContext sample; state = contextState context
      expectedClock <- checkedCounts (W.contextCounts state)
      require (end > start && end <= length (W.watchRecords watch)) SnapshotRejected
      -- Instruction points precede watch hooks; stopped fetch clocks equal the settled count.
      executed <- case reverse events of
        (_,event):rest -> do
          (core,pc,clock,target) <- eventShape inputs (receiptSchema receipt) event
          checkFetchShape pc target event
          require (core == watchCore (contextCore context) && pc == W.contextPc state && clock == expectedClock) FetchRejected
          pure (reverse rest)
        _ -> Left FetchRejected
      next <- foldM (\current row@(_,event) -> if W.eventSource event == W.HostWrite
        then hostPatch receipt patches current row else advance inputs receipt current event) progress executed
      require (progressCounts next == W.contextCounts state) ClockRejected
      memory <- checkSample inputs receipt probe sample (progressMemory next)
      observed <- noteSnapshot inputs receipt probe sample (next {progressMemory=memory})
      pure (observed,sampleCursor sample)

submissionSpec :: TaskId -> Either WitnessError (KernelCore,Word32,Word32,Word32,Word32,Word32,Word32)
submissionSpec (TaskId ident) = case ident of
  104 -> Right (PrimaryKernel,0x40063424,0x40063420,0x69ce8c,0x781960,0,0x69d160)
  102 -> Right (PrimaryKernel,0x40063424,0x40063420,0x69cee8,0x781968,14,0x69cbf0)
  103 -> Right (SecondaryKernel,0x400f33e4,0x400f33e0,0x69cf38,0x781970,14,0x69cbf0)
  93 -> Right (SecondaryKernel,0x400f33e4,0x400f33e0,0x69cf70,0x781978,12,0x69cd58)
  _ -> Left SubmissionRejected
checkSubmission :: Inputs -> Receipt -> Submission -> Either WitnessError ()
checkSubmission (Inputs _ _ program) receipt submitted = do
  let ident@(TaskId number) = submissionId submitted; start = submissionEntryHit submitted
  (core,descriptor,sp,returnPc,name,priority,entry) <- submissionSpec ident
  finish <- required SubmissionRejected (submissionReturnHit submitted)
  require (submissionCore submitted == core && submissionDescriptor submitted == W.Address descriptor && submissionSp submitted == W.Address sp
    && submissionReturnPc submitted == W.Address returnPc && submissionStatus submitted == Just (W.WordValue 0xffffffe7) && start < finish) SubmissionRejected
  incoming <- sampleAt receipt start; outgoing <- sampleAt receipt finish
  let incomingState = contextState (sampleContext incoming); outgoingState = contextState (sampleContext outgoing)
  require (sampleBoundary incoming == SubmissionEntry && sampleBoundary outgoing == WrapperReturn
    && W.contextSp incomingState == W.Address sp && W.contextSp outgoingState == W.Address sp
    && W.contextLr incomingState == W.Address returnPc && W.contextPc outgoingState == W.Address returnPc
    && value (W.contextCpsr incomingState) .&. 0xff == 0xd3 && value (W.contextCpsr outgoingState) .&. 0xff == 0xd3
    && take 8 (drop 4 (W.contextRegisters incomingState)) == take 8 (drop 4 (W.contextRegisters outgoingState))
    && take 1 (W.contextRegisters outgoingState) == [W.WordValue 0xffffffe7]) SubmissionRejected
  before <- localMemory incoming; after <- localMemory outgoing
  descriptorWords <- mapM (readWord SubmissionRejected before . (physical descriptor+)) [0,4..24]
  require (take 6 descriptorWords == [fromIntegral number,name,priority+1,1024,entry,0]) SubmissionRejected
  later <- mapM (readWord SubmissionRejected after . (physical descriptor+)) [0,4..24]
  require (later == descriptorWords) SubmissionRejected
  let interval = filter (\sample -> sampleHit sample >= start && sampleHit sample <= finish && contextCore (sampleContext sample) == core
        && sampleCallTaken sample /= Just False) (receiptSamples receipt)
  mapM_ (\kind -> require (length (filter ((==kind) . sampleBoundary) interval) == 1) SubmissionRejected)
    [SubmissionEntry,AuxiliaryReturn,CreationCall,CreationReturn,ActivationCall,ActivationReturn,WrapperReturn]
  if receiptSchema receipt == LegacyStockProtocol then pure () else do
    let ordered = [CriticalEnterCall,CriticalEnterReturn,ReservationCall,ReservationReturn,AuxiliaryCall,AuxiliaryReturn
          ,FillCall,FillReturn,CacheCall,CacheReturn,CreationCall,CreationReturn,CriticalExitCall,CriticalExitReturn
          ,IdentityCall,IdentityReturn,ActivationCall,ActivationReturn,LogCall,LogReturn,ErrorCallbackCall,ErrorCallbackReturn]
    require ([sampleBoundary sample | sample <- interval,sampleCallTaken sample /= Nothing] == ordered) SubmissionRejected
    readWord SubmissionRejected after 0xa0f82c >>= \errorWord -> require (errorWord == 0xffffffe7) SubmissionRejected
  creationCall <- required SubmissionRejected (case filter ((==CreationCall) . sampleBoundary) interval of [sample] -> Just sample; _ -> Nothing)
  activationCall <- required SubmissionRejected (case filter ((==ActivationCall) . sampleBoundary) interval of [sample] -> Just sample; _ -> Nothing)
  auxiliary <- required SubmissionRejected (case filter ((==AuxiliaryReturn) . sampleBoundary) interval of [sample] -> Just sample; _ -> Nothing)
  created <- required SubmissionRejected (case filter ((==CreationReturn) . sampleBoundary) interval of [sample] -> Just sample; _ -> Nothing)
  activated <- required SubmissionRejected (case filter ((==ActivationReturn) . sampleBoundary) interval of [sample] -> Just sample; _ -> Nothing)
  contextRegister 0 auxiliary >>= \status -> require (status == 0) SubmissionRejected
  contextRegister 0 created >>= \status -> require (status == 0) SubmissionRejected
  contextRegister 0 activated >>= \status -> require (status == 0xffffffe7) SubmissionRejected
  let (cbase,tbase,_,auxBase,nodeBase,_,_,_,_,_) = layout core; i = fromIntegral number-1; c = physical (cbase+40*i); t = physical (tbase+56*i)
  pool <- readWord SubmissionRejected after (0xa5b204+8*i)
  size <- readWord SubmissionRejected after (0xa5b208+8*i)
  require (pool >= 0xa10200 && toInteger pool+1024 <= 0xa5a200 && size == 1024) SubmissionRejected
  let classNumber = case core of PrimaryKernel -> 1; SecondaryKernel -> 2
      encoded = classNumber `shiftL` 16 .|. fromIntegral number
      constructor = case core of PrimaryKernel -> 0x4d094; SecondaryKernel -> 0xdd054
      activation = case core of PrimaryKernel -> 0x44f2c; SecondaryKernel -> 0xd4eec
      creationState = contextState (sampleContext creationCall); activationState = contextState (sampleContext activationCall)
  contextRegister 0 creationCall >>= \actual -> require (actual == encoded) SubmissionRejected
  parameterPointer <- contextRegister 1 creationCall
  contextRegister 0 activationCall >>= \actual -> require (actual == encoded) SubmissionRejected
  require (testBit (value (W.contextCpsr activationState)) 7) SubmissionRejected
  case P.programFrameProof program of
    Just _ -> pure ()
    Nothing -> do
      require (parameterPointer == sp-48 && W.contextSp creationState == W.Address (sp-56) && W.contextSp activationState == W.Address (sp-56)) SubmissionRejected
      contextRegister 3 creationCall >>= \actual -> require (actual == constructor) SubmissionRejected
      contextRegister 3 activationCall >>= \actual -> require (actual == activation) SubmissionRejected
  callMemory <- localMemory creationCall
  callParameters <- mapM (\offset -> readWord SubmissionRejected callMemory (physical parameterPointer+offset)) [0,4..20]
  require (callParameters == [2,0,entry,priority+1,1024,pool]) SubmissionRejected
  case P.programFrameProof program of
    Just _ -> pure ()
    Nothing -> do
      saved <- mapM (\position -> if position == 14 then pure (address (W.contextLr incomingState)) else value <$> at SubmissionRejected (W.contextRegisters incomingState) position) [4,5,6,7,11,14]
      savedBytes <- mapM (\offset -> readWord SubmissionRejected after (physical sp-24+offset)) [0,4..20]
      require (savedBytes == saved) SubmissionRejected
  readWord SubmissionRejected before (physical sp) >>= \untouched -> readWord SubmissionRejected after (physical sp) >>= \actual -> require (actual == untouched) SubmissionRejected
  parameters <- mapM (readWord TaskRejected after . (c+)) [0,4..36]
  require (parameters == [2,0,entry,priority,1024,pool,0,0,fromIntegral number,1]) TaskRejected
  node <- readBytes TaskRejected after t 56
  require (U.word32At node 8 == cbase+40*i && B.index node 12 == 0 && fromIntegral (B.index node 13) == priority
    && B.index node 14 .&. 7 == 0 && U.word32At node 32 == (case core of PrimaryKernel -> 1; SecondaryKernel -> 2)
    && all ((==0) . U.word32At node) [16,36,40,44,48]) TaskRejected
  flags <- at SubmissionRejected descriptorWords 6
  auxiliaryWords <- mapM (readWord SubmissionRejected after . (physical (auxBase+12*i)+)) [0,4,8]
  require (auxiliaryWords == [if flags == 0xa5f0a5f0 then 0 else 4,0,1]) TaskRejected
  let auxNode = physical (nodeBase+16*i)
  readWord TaskRejected after auxNode >>= \pointer -> require (pointer == nodeBase+16*i) TaskRejected
  readWord TaskRejected after (auxNode+4) >>= \pointer -> require (pointer == nodeBase+16*i) TaskRejected
  readWord TaskRejected after (auxNode+8) >>= \pointer -> require (pointer == auxBase+12*i) TaskRejected
  readWord TaskRejected after (auxNode+12) >>= \copied -> require (copied == 0) TaskRejected
  reserved <- mapM (\offset -> readWord TaskRejected after (0xa0fc44+12*i+offset)) [0,4,8]
  require (reserved == [pool,1024,1]) TaskRejected
  filled <- readBytes TaskRejected after pool 1024
  require (filled == B.replicate 1024 (fromIntegral number)) TaskRejected

checkRunnable :: Receipt -> KernelCore -> Either WitnessError ()
checkRunnable receipt core = do
  before <- one AutoReadyEntry; after <- one AutoReadyReturn
  require (sampleHit before < sampleHit after) RunnableRejected
  memory <- localMemory after; kernel <- kernelAt after core
  let tasks = kernelTasks kernel; candidates = filter (\task -> testBit (value (taskFlags task)) 1) tasks
      runnable = filter ((==1) . taskState) tasks
      (cbase,tbase,ids,_,_,_,_,_,_,_) = layout core
  require (map taskId candidates == map taskId runnable && not (null runnable) && all ((==W.WordValue 1) . taskCreated) candidates) RunnableRejected
  idOrder <- mapM (readWord RunnableRejected memory . (physical ids+) . (4*)) [0..119]
  require (sort idOrder == [1..120]) RunnableRejected
  let rank task = required RunnableRejected (lookup (case taskId task of TaskId ident -> fromIntegral ident) (zip idOrder [0 :: Int ..]))
  ordered <- mapM (\task -> do order <- rank task; pure (value (taskPriority task),order,task)) candidates
  winner <- required RunnableRejected (case sortOn (\(priority,order,_) -> (priority,order)) ordered of (_,_,task):_ -> Just task; [] -> Nothing)
  require (kernelSelected kernel == taskNode winner && kernelCurrent kernel == W.Address 0) RunnableRejected
  domain <- pure (physical (address (kernelDomain kernel)))
  heads <- readWord RunnableRejected memory (domain+0x20)
  bitmapPointer <- readWord RunnableRejected memory (domain+0x24)
  observedBitmap <- readWord RunnableRejected memory (physical bitmapPointer)
  let expectedBitmap = foldl (.|.) 0 [1 `shiftL` fromIntegral (value (taskPriority task)) | task <- candidates]
  require (all ((<16) . value . taskPriority) candidates && observedBitmap == expectedBitmap) RunnableRejected
  mapM_ (\priority -> do
    let queue = sortOn (\(_,order,_) -> order) (filter (\(p,_,_) -> p == priority) ordered)
        headAddress = heads+8*priority; nodes = [address (taskNode task) | (_,_,task) <- queue]
        expected = headAddress:nodes ++ [headAddress]
    actualHead <- readWord RunnableRejected memory (physical headAddress); actualTail <- readWord RunnableRejected memory (physical headAddress+4)
    require (actualHead == (case nodes of [] -> headAddress; node:_ -> node)
      && actualTail == (case reverse nodes of [] -> headAddress; node:_ -> node)) RunnableRejected
    mapM_ (\(previous,node,next) -> do
      readWord RunnableRejected memory (physical node) >>= \actual -> require (actual == next) RunnableRejected
      readWord RunnableRejected memory (physical node+4) >>= \actual -> require (actual == previous) RunnableRejected)
      (zip3 expected (drop 1 expected) (drop 2 expected))) [0..15]
  mapM_ (\task -> do
    let TaskId ident = taskId task; c = cbase+40*(fromIntegral ident-1); t = tbase+56*(fromIntegral ident-1)
    resume <- readWord RunnableRejected memory (domain+0x7c)
    require (taskCreationPointer task == W.Address c && taskNode task == W.Address t && value (taskStackBytes task) == 1024
      && taskSavedSp task == W.Address (address (taskStackBase task)+1024) && taskResumePc task == W.Address resume
      && taskEffectivePriority task == fromIntegral (value (taskPriority task))) RunnableRejected) candidates
  where
    one kind = required RunnableRejected (case filter (\sample -> sampleBoundary sample == kind && contextCore (sampleContext sample) == core) (receiptSamples receipt) of [sample] -> Just sample; _ -> Nothing)

checkFirst :: Receipt -> W.Watch -> FirstTask -> Either WitnessError ()
checkFirst receipt watch first = do
  selected <- sampleAt receipt (firstSelectedHit first)
  transferIndex <- required FirstTaskRejected (firstTransferHit first); transfer <- sampleAt receipt transferIndex
  entryIndex <- required FirstTaskRejected (firstEntryHit first); enteredIndex <- required FirstTaskRejected (firstEnteredHit first)
  entry <- sampleAt receipt entryIndex; entered <- sampleAt receipt enteredIndex
  let core = firstCore first; state = contextState (sampleContext entry); after = contextState (sampleContext entered)
      expectedTrampoline = case core of PrimaryKernel -> 0x43728; SecondaryKernel -> 0xd3728
      expectedReturn = case core of PrimaryKernel -> 0x453f0; SecondaryKernel -> 0xd53b0
  require (map sampleBoundary [selected,transfer,entry,entered] == [Selected,EntryTransfer,TaskEntry,EnteredInstruction]
    && all ((==core) . contextCore . sampleContext) [selected,transfer,entry,entered]
    && firstSelectedHit first < transferIndex && transferIndex < entryIndex && entryIndex < enteredIndex) FirstTaskRejected
  memory <- localMemory selected; kernel <- kernelAt selected core
  task <- required FirstTaskRejected (case filter ((==firstId first) . taskId) (kernelTasks kernel) of [task] -> Just task; _ -> Nothing)
  require (kernelCurrent kernel == firstNode first && kernelSelected kernel == firstNode first && taskNode task == firstNode first
    && taskCreation task == firstCreation first && taskCreationPointer task == firstCreation first && taskResumePc task == W.Address expectedTrampoline
    && taskSavedSp task == firstStackTop first && taskEntry task == firstEntry first) FirstTaskRejected
  readWord FirstTaskRejected memory (physical (address (firstNode first))+8) >>= \actual -> require (actual == address (firstCreation first)) FirstTaskRejected
  require (W.contextSp state == firstStackTop first && W.contextPc state == firstEntry first && W.contextLr state == W.Address expectedReturn
    && take 3 (W.contextRegisters state) == map W.WordValue [0,address (firstNode first),address (firstCreation first)]
    && value (W.contextCpsr state) .&. 0xff0000ff == 0x53 && firstEntryCounts first == Just (W.contextCounts state)) FirstTaskRejected
  autoReady <- required FirstTaskRejected (case filter (\sample -> sampleBoundary sample == AutoReadyReturn && contextCore (sampleContext sample) == core) (receiptSamples receipt) of [sample] -> Just sample; _ -> Nothing)
  readyKernel <- kernelAt autoReady core
  require (kernelSelected readyKernel == firstNode first && sampleHit autoReady < firstSelectedHit first) FirstTaskRejected
  stackTransfer <- oneBetween StackTransfer
  trampoline <- oneBetween Trampoline
  let stackState = contextState (sampleContext stackTransfer); trampolineState = contextState (sampleContext trampoline)
      transferState = contextState (sampleContext transfer)
  require (W.contextSp stackState == firstStackTop first && W.contextSp trampolineState == firstStackTop first
    && W.contextPc trampolineState == W.Address expectedTrampoline && W.contextSp transferState == firstStackTop first
    && W.contextLr transferState == W.Address expectedReturn
    && take 3 (W.contextRegisters transferState) == map W.WordValue [0,address (firstNode first),address (firstCreation first)]) FirstTaskRejected
  stackCount <- at FirstTaskRejected (W.contextCounts stackState) (coreIndex core)
  trampolineCount <- at FirstTaskRejected (W.contextCounts trampolineState) (coreIndex core)
  transferCount <- at FirstTaskRejected (W.contextCounts transferState) (coreIndex core)
  entryCount <- at FirstTaskRejected (W.contextCounts state) (coreIndex core)
  require (toInteger (count trampolineCount) == toInteger (count stackCount)+1 && toInteger (count entryCount) == toInteger (count transferCount)+1) FirstTaskRejected
  let entryPc = address (firstEntry first)
  registers <- case entryPc of 0x69d160 -> Right [4,5,11,14]; 0x69cbf0 -> Right [11]; 0x69cd58 -> Right [4,5,6,11,14]; _ -> Left PrologueRejected
  let frameBytes = 4*fromIntegral (length registers); expectedSp = address (firstStackTop first)-frameBytes
      corePosition = coreIndex core
  require (W.contextPc after == W.Address (entryPc+4) && W.contextSp after == W.Address expectedSp && W.contextRegisters after == W.contextRegisters state
    && W.contextLr after == W.contextLr state && W.contextCpsr after == W.contextCpsr state) PrologueRejected
  beforeCount <- at PrologueRejected (W.contextCounts state) corePosition
  afterCount <- at PrologueRejected (W.contextCounts after) corePosition
  require (toInteger (count afterCount) == toInteger (count beforeCount)+1) PrologueRejected
  expectedValues <- mapM (\position -> if position == 14 then pure (address (W.contextLr state)) else value <$> at PrologueRejected (W.contextRegisters state) position) registers
  committed <- localMemory entered
  actualValues <- mapM (readWord PrologueRejected committed . (physical expectedSp+) . (4*)) [0..fromIntegral (length registers)-1]
  require (actualValues == expectedValues) PrologueRejected
  let stores = [(W.eventAddress event,W.eventValue event,W.eventSize event) | event <- take (cursor (sampleCursor entered)-cursor (sampleCursor entry)) (drop (cursor (sampleCursor entry)) (W.watchRecords watch)),W.eventSource event == W.CpuWrite,W.eventCore event == Just (watchCore core)]
  require (stores == [(fromIntegral expectedSp+4*fromIntegral offset,Just (fromIntegral saved),4) | (offset,saved) <- zip [0 :: Int ..] expectedValues]) PrologueRejected
  where
    oneBetween kind = required FirstTaskRejected (case filter (\sample -> sampleBoundary sample == kind
      && contextCore (sampleContext sample) == firstCore first && sampleHit sample > firstSelectedHit first
      && maybe False (sampleHit sample <) (firstTransferHit first)) (receiptSamples receipt) of [sample] -> Just sample; _ -> Nothing)

admitReceipt :: Inputs -> ObservedFiles -> Either WitnessError QualifiedReceipt
admitReceipt inputs@(Inputs _ expectedIdentity program) observed = do
  require (B.length (observedReceipt observed) <= 512*1024*1024 && B.length (observedWatch observed) <= 32*1024*1024) ReceiptDecodeRejected
  C.ControlEnvelope receipt probe <- decode ReceiptDecodeRejected (observedReceipt observed)
  watch <- decode RawWatchRejected (observedWatch observed)
  either (const (Left StatusRejected)) Right (P.checkProgramHeader program receipt)
  require (receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing
    && receiptWatch receipt == WatchOk True Nothing && receiptWatchEndError receipt == Nothing) StatusRejected
  case probe of
    Nothing -> require (receiptOutcome receipt == ObservedMenu && receiptMenu receipt == MenuOk (Just [1,5,2])
      && case receiptSchema receipt of
        JoinedTaskProtocol -> receiptInstructions receipt >= W.Count 330000000 && receiptInstructions receipt <= W.Count 450000000
        _ -> receiptInstructions receipt == W.Count 382000000) StatusRejected
    Just _ -> do
      endpoint <- required SnapshotRejected (case reverse (receiptSamples receipt) of sample:_ -> Just sample; [] -> Nothing)
      endpointCount <- checkedCounts (W.contextCounts (contextState (sampleContext endpoint)))
      require (receiptOutcome receipt == ObservedProbe && receiptInstructions receipt == endpointCount
        && case receiptMenu receipt of MenuOk _ -> True; MenuError _ -> False) StatusRejected
  require (receiptIdentity receipt == expectedIdentity) IdentityRejected
  require (receiptCheckpointReused receipt && receiptCheckpoint receipt == hash (observedReset observed) && not (B.null (observedReset observed))) ResetRejected
  require (receiptWatchDigest receipt == hash (observedWatch observed)) RawWatchRejected
  require (W.watchEnabled watch && W.watchViolation watch == Nothing && W.watchViews watch == "observe"
    && W.watchClasses watch == expectedClasses inputs (receiptSchema receipt)
    && W.watchRanges watch == unionRanges (map W.classRange (expectedClasses inputs (receiptSchema receipt)))
    && not (null (W.watchRecords watch)) && length (W.watchRecords watch) < 1048576) WatchRejected
  let cpuRows = filter (\event -> W.eventSource event `elem` [W.CpuFetch,W.CpuRead,W.CpuWrite]) (W.watchRecords watch)
      clocks = mapMaybe W.eventInstructions cpuRows; ticks = mapMaybe W.eventTick cpuRows
  require (length clocks == length cpuRows && length ticks == length clocks
    && all (<=startupBudget) clocks && clocks == sort clocks && ticks == sort ticks) ClockRejected
  require (receiptSites receipt == expectedSites inputs (receiptSchema receipt) && not (null (receiptSamples receipt)) && length (receiptHits receipt) < 4096) SnapshotRejected
  first <- at SnapshotRejected (receiptSamples receipt) 0
  require (map (index . sampleHit) (receiptSamples receipt) == [index (sampleHit first)..length (receiptHits receipt)-1]) SnapshotRejected
  control <- case receiptSchema receipt of
    LegacyStockProtocol -> do require (probe == Nothing) ControlRejected; pure Nothing
    TaskSubmissionProtocol -> either (Left . ControlReasonRejected) Right (C.qualifyControl receipt watch probe)
    JoinedTaskProtocol -> either (Left . ControlReasonRejected) Right (C.qualifyControl receipt watch probe)
  _ <- replay inputs receipt probe watch
  require (sort (map submissionId (receiptSubmissions receipt)) == map TaskId [93,102,103,104]) SubmissionRejected
  mapM_ (checkSubmission inputs receipt) (receiptSubmissions receipt)
  mapM_ (checkRunnable receipt) [PrimaryKernel,SecondaryKernel]
  require (sort (map firstCore (receiptFirstTasks receipt)) == [PrimaryKernel,SecondaryKernel]) FirstTaskRejected
  mapM_ (checkFirst receipt watch) (receiptFirstTasks receipt)
  pure (QualifiedReceipt receipt (hash (observedReceipt observed)) (length (W.watchRecords watch)) probe control)

qualifiedReceiptData :: QualifiedReceipt -> Receipt
qualifiedReceiptData (QualifiedReceipt receipt _ _ _ _) = receipt
qualifiedProbeData :: QualifiedReceipt -> Maybe C.ProbeRecord
qualifiedProbeData (QualifiedReceipt _ _ _ probe _) = probe
qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt receipt (W.Digest digest) rows _ control) = J.object
  ["schema" J..= (1 :: Int),"qualification" J..= label,"receipt_sha256" J..= digest
  ,"watch_rows" J..= rows,"snapshots" J..= length (receiptSamples receipt),"submissions" J..= length (receiptSubmissions receipt)
  ,"control" J..= fmap (\proof -> J.object ["case" J..= show (C.controlCase proof),"endpoint" J..= show (C.controlEndpoint proof)]) control
  ,"first_tasks" J..= [J.object ["core" J..= show (firstCore first),"id" J..= (case firstId first of TaskId ident -> ident)
      ,"entry" J..= address (firstEntry first),"stack_top" J..= address (firstStackTop first)] | first <- receiptFirstTasks receipt]
  ,"scope" J..= scope]
  where
    label, scope :: String
    (label,scope) = case receiptScope receipt of
      StockStartupScope -> ("stock-startup-task-observation","Canonical stock startup task objects, auto-ready queues and both first prologues; no owned task implementation or post-entry lifetime acceptance")
      OwnedStartupScope -> ("owned-task-submission-observation","In-place wrapper/release with named retained callees, four startup submissions and both first prologues; no global caller retirement or post-entry lifetime acceptance")
      StockProbeScope -> ("stock-task-submission-control","Retained wrapper/release through the declared bounded controlled endpoint")
      OwnedProbeScope -> ("owned-task-submission-control","In-place wrapper/release through the declared bounded controlled endpoint; named retained callee, mapping, lifetime and external ABI premises remain")
      StockJoinedStartupScope -> ("retained-task-composition-observation","Canonical retained task startup with critical counter and lock endpoints; no owned implementation acceptance")
      StockJoinedProbeScope -> ("retained-task-composition-control","Canonical retained wrapper/release and critical resources through the declared bounded controlled endpoint")
      JoinedStartupScope -> ("owned-task-composition-observation","Joined wrapper/release, critical pair and storage transition through four startup submissions and both first prologues; retained creation and scheduler paths remain")
      JoinedProbeScope -> ("owned-task-composition-control","Joined wrapper/release, critical pair and storage transition through the declared bounded controlled endpoint; retained external ABI and lifetime premises remain")
