{-# LANGUAGE OverloadedStrings #-}
module Emulation.StartupTaskWitness
  ( SourceFiles(..), ObservedFiles(..), Inputs, QualifiedReceipt, WitnessError(..)
  , admitInputs, admitReceipt, qualificationValue
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
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Wire as W

data SourceFiles = SourceFiles
  {sourceDat :: B.ByteString, sourceRun :: B.ByteString, sourceRom :: B.ByteString
  ,sourceA :: B.ByteString, sourceB :: B.ByteString, sourceImage :: B.ByteString, sourceObserver :: B.ByteString}
data ObservedFiles = ObservedFiles {observedReceipt :: B.ByteString, observedWatch :: B.ByteString, observedReset :: B.ByteString}
data Inputs = Inputs SourceFiles Identity
data QualifiedReceipt = QualifiedReceipt Receipt W.Digest Int
data WitnessError = SourceRejected | ReceiptDecodeRejected | StatusRejected | IdentityRejected | ResetRejected
  | WatchRejected | ClockRejected | FetchRejected | WriteRejected | PreimageRejected | ForeignWriterRejected
  | SnapshotRejected | AliasRejected | ContextRejected | CodeRejected | KernelRejected | TaskRejected
  | SubmissionRejected | RunnableRejected | FirstTaskRejected | PrologueRejected | RawWatchRejected
  deriving (Eq, Show)

require :: Bool -> WitnessError -> Either WitnessError ()
require condition problem = unless condition (Left problem)
required :: WitnessError -> Maybe a -> Either WitnessError a
required problem = maybe (Left problem) Right
address :: W.Address -> Word32
address (W.Address value) = value
value :: W.WordValue -> Word32
value (W.WordValue number) = number
count :: W.Count -> Word64
count (W.Count number) = number
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
    ,(image,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    ,(observer,"bc6e8ab207703901286a85bffc994d74543b3d213e3d54ab89a7bff8b5b6609d")]
  require (U.word32At (sourceA sources) 0xdabc == 120 && U.word32At (sourceB sources) 0xda80 == 120) SourceRejected
  pure (Inputs sources (Identity run dat rom observer
    [SourceIdentity LoaderA (W.Address 0x40000) (B.length (sourceA sources)) loaderA
    ,SourceIdentity LoaderB (W.Address 0xd0000) (B.length (sourceB sources)) loaderB
    ,SourceIdentity Image4 (W.Address 0x61c400) (B.length (sourceImage sources)) image]))

codeSources :: Inputs -> [(SourceKind,Word32,B.ByteString)]
codeSources (Inputs sources _) = [(LoaderA,0x40000,sourceA sources),(LoaderB,0xd0000,sourceB sources),(Image4,0x61c400,sourceImage sources)]
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

expectedSites :: M.Map W.Address Boundary
expectedSites = M.fromList [(W.Address pc,boundary) | (boundary,pcs) <-
  [(SubmissionEntry,[0x725dd8]),(AuxiliaryReturn,[0x725f0c]),(CreationCall,[0x725fa8]),(CreationReturn,[0x725fac])
  ,(ActivationCall,[0x726024]),(ActivationReturn,[0x726028]),(WrapperReturn,[0x69ce8c,0x69cee8,0x69cf38,0x69cf70])
  ,(AutoReadyEntry,[0x43f9c,0xd3f58]),(AutoReadyReturn,[0x40a74,0xd0a74]),(Prepare,[0x43f70,0xd3f2c])
  ,(Prepared,[0x43f98,0xd3f54]),(Scheduler,[0x43688,0xd3688]),(Selected,[0x436bc,0xd36bc])
  ,(StackTransfer,[0x436e8,0xd36e8]),(Trampoline,[0x43728,0xd3728]),(EntryTransfer,[0x43758,0xd3758])
  ,(TaskEntry,[0x69d160,0x69cbf0,0x69cd58]),(EnteredInstruction,[0x69d164,0x69cbf4,0x69cd5c])],pc <- pcs]

expectedClasses :: Inputs -> [W.WatchClass]
expectedClasses inputs = [W.WatchClass [start,end] writes | (start,end) <-
  [(0x4dae0,0x4f10c),(0xddaa0,0xdf0cc),(0x545a4,0x63460),(0xe4564,0xf3410),(0xc0800,0xc0900)
  ,(0x96bd20,0x96bd28),(0xa0fc44,0xa101e4),(0xa5b204,0xa5b5c4),(0x89ea94,0x89ea9c),(0x9c280c,0x9c2810),(0xa10200,0xa5b200)]]
  ++ [W.WatchClass [fromIntegral base,fromIntegral base+fromIntegral (B.length contents)] [W.CpuWrite,W.CpuFetch,W.HostWrite,W.ModelBusWrite] | (_,base,contents) <- codeSources inputs]
  where writes = [W.CpuWrite,W.HostWrite,W.ModelBusWrite]
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
  mapM_ validate (allViews sample)
  foldM mergeView memory (allViews sample)
  where
    state = contextState (sampleContext sample)
    validate view = do
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

checkLocalShape :: Receipt -> Sample -> Memory -> Either WitnessError ()
checkLocalShape receipt sample memory = do
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
  mapM_ (\(table,offset,expected) -> readWord SnapshotRejected memory (table+offset) >>= \actualWord -> require (actualWord == expected) SnapshotRejected)
    [(0x4dae0,0,0x4d094),(0x4dae0,0xc,0x44f2c),(0x4dae0,0x80,0x4d39c),(0x4dae0,0x1618,0x412bc)
    ,(0x4dae0,0x164,0x41140),(0x4dae0,0x168,0x41164),(0xddaa0,0,0xdd054),(0xddaa0,0xc,0xd4eec)
    ,(0xddaa0,0x80,0xdd360),(0xddaa0,0x164,0xd1140),(0xddaa0,0x168,0xd1164)]
  where core = contextCore (sampleContext sample); position = sampleHit sample


checkSample :: Inputs -> Receipt -> Sample -> Memory -> Either WitnessError Memory
checkSample inputs receipt sample previous = do
  let context = sampleContext sample; state = contextState context
  hit <- at ContextRejected (receiptHits receipt) (index (sampleHit sample))
  require (length (W.contextRegisters state) == 13 && length (W.contextCounts state) == 2 && length (W.contextTcm state) == 2
    && W.contextIdentities state == map W.WordValue [0x4003,0x5003] && contextCore context == hitCore hit
    && W.contextPc state == hitAddress hit && sampleBoundary sample == hitBoundary hit
    && M.lookup (W.contextPc state) expectedSites == Just (sampleBoundary sample)
    && sum (map count (W.contextCounts state)) == count (hitInstructions hit)
    && W.contextRegisters state == hitRegisters hit && W.contextSp state == hitSp hit && W.contextLr state == hitLr hit && W.contextCpsr state == hitCpsr hit) ContextRejected
  instruction <- sourceWord inputs (address (W.contextPc state))
  require (sampleCodeWord sample == W.WordValue instruction && map kernelCore (sampleKernels sample) == [PrimaryKernel,SecondaryKernel]) CodeRejected
  memory <- validateViews sample previous
  mapM_ (checkKernel memory) (sampleKernels sample)
  checkLocalShape receipt sample memory
  readWord SnapshotRejected memory 0x96bd20 >>= \root -> require (root == 0x4004dae0) SnapshotRejected
  readWord SnapshotRejected memory 0x96bd24 >>= \root -> require (root == 0x400ddaa0) SnapshotRejected
  pure memory

data Progress = Progress {progressCounts :: [W.Count], progressFetches :: M.Map W.Core (W.Address,W.Count), progressMemory :: Memory}
clockTotal :: Progress -> Word64
clockTotal = sum . map count . progressCounts
advance :: Inputs -> Progress -> W.MemoryEvent -> Either WitnessError Progress
advance inputs progress event = do
  require (W.eventSource event /= W.HostWrite && W.eventSource event /= W.ModelBusWrite) ForeignWriterRejected
  core <- required FetchRejected (W.eventCore event); pc <- required FetchRejected (W.eventPc event)
  clock <- required ClockRejected (W.eventInstructions event); target <- required WriteRejected (W.eventPhysical event)
  require (W.eventByteOrder event == Just W.Little && W.eventAddress event <= 0xffffffff
    && target <= 0xffffffff && physical (fromIntegral (W.eventAddress event)) == fromIntegral target) WriteRejected
  require (any (\classification -> case W.classRange classification of
      [start,end] -> target >= start && toInteger target+toInteger (W.eventSize event) <= toInteger end
        && W.eventSource event `elem` W.classKinds classification
      _ -> False) (expectedClasses inputs)) WatchRejected
  _ <- sourceWord inputs (address pc)
  case W.eventSource event of
    W.CpuFetch -> do
      require (W.eventAddress event == fromIntegral (address pc) && target == fromIntegral (address pc)
        && W.eventSize event == 4 && W.eventValue event == Nothing && W.eventBefore event == Nothing) FetchRejected
      require (count clock == clockTotal progress+1) ClockRejected
      let counts = zipWith (\position current -> if position == (case core of W.Primary -> 0; W.Secondary -> 1) then W.Count (count current+1) else current) [0 :: Int ..] (progressCounts progress)
      pure progress {progressCounts=counts,progressFetches=M.insert core (pc,clock) (progressFetches progress)}
    W.CpuWrite -> do
      require (M.lookup core (progressFetches progress) == Just (pc,clock)) ClockRejected
      require (W.eventSize event `elem` [1,2,4,8] && not (immutable inputs (fromIntegral target) (W.eventSize event))) WriteRejected
      old <- bytes <$> required PreimageRejected (W.eventBefore event); new <- required WriteRejected (W.eventValue event)
      let size = fromIntegral (W.eventSize event); Memory contents = progressMemory progress
      require (B.length old == size && (size == 8 || new < 1 `shiftL` (8*size))) WriteRejected
      mapM_ (\(offset,byte) -> require (maybe True (==byte) (M.lookup (fromIntegral target+offset) contents)) PreimageRejected) (zip [0..] (B.unpack old))
      let replacement = [(fromIntegral target+fromIntegral offset,fromIntegral (new `shiftR` (8*offset))) | offset <- [0..size-1]]
      pure progress {progressMemory=Memory (foldr (uncurry M.insert) contents replacement)}
    W.HostWrite -> Left ForeignWriterRejected
    W.ModelBusWrite -> Left ForeignWriterRejected
    _ -> Left WatchRejected

replay :: Inputs -> Receipt -> W.Watch -> Either WitnessError Memory
replay inputs receipt watch = do
  first <- at SnapshotRejected (receiptSamples receipt) 0
  require (sampleCursor first == Cursor 0) SnapshotRejected
  let Inputs sources _ = inputs; offset = 0x89ea8c-0x61c400
      initialTail = View Control (W.Address 0x89ea8c) (W.Bytes (B.drop offset (sourceImage sources))) []
  initial <- mergeView (Memory M.empty) initialTail >>= checkSample inputs receipt first
  checkInitial first initial
  final <- foldM step (Progress (W.contextCounts (contextState (sampleContext first))) M.empty initial,sampleCursor first) (drop 1 (receiptSamples receipt))
  require (cursor (snd final) == length (W.watchRecords watch) && Just (progressCounts (fst final)) == receiptStartupCounts receipt) ClockRejected
  pure (progressMemory (fst final))
  where
    step (progress,previousCursor) sample = do
      let end = cursor (sampleCursor sample); start = cursor previousCursor; events = take (end-start) (drop start (W.watchRecords watch))
          context = sampleContext sample; state = contextState context; expectedClock = W.Count (sum (map count (W.contextCounts state)))
          -- Instruction points precede watch hooks; stopped fetch clocks equal the settled count.
          stopped event = W.eventSource event == W.CpuFetch && W.eventCore event == Just (watchCore (contextCore context))
            && W.eventPc event == Just (W.contextPc state) && W.eventInstructions event == Just expectedClock
      require (end > start && end <= length (W.watchRecords watch)) SnapshotRejected
      executed <- case reverse events of event:rest | stopped event -> Right (reverse rest); _ -> Left FetchRejected
      next <- foldM (advance inputs) progress executed
      require (progressCounts next == W.contextCounts state) ClockRejected
      memory <- checkSample inputs receipt sample (progressMemory next)
      pure (next {progressMemory=memory},sampleCursor sample)

submissionSpec :: TaskId -> Either WitnessError (KernelCore,Word32,Word32,Word32,Word32,Word32,Word32)
submissionSpec (TaskId ident) = case ident of
  104 -> Right (PrimaryKernel,0x40063424,0x40063420,0x69ce8c,0x781960,0,0x69d160)
  102 -> Right (PrimaryKernel,0x40063424,0x40063420,0x69cee8,0x781968,14,0x69cbf0)
  103 -> Right (SecondaryKernel,0x400f33e4,0x400f33e0,0x69cf38,0x781970,14,0x69cbf0)
  93 -> Right (SecondaryKernel,0x400f33e4,0x400f33e0,0x69cf70,0x781978,12,0x69cd58)
  _ -> Left SubmissionRejected
checkSubmission :: Receipt -> Submission -> Either WitnessError ()
checkSubmission receipt submitted = do
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
  let interval = filter (\sample -> sampleHit sample >= start && sampleHit sample <= finish && contextCore (sampleContext sample) == core) (receiptSamples receipt)
  mapM_ (\kind -> require (length (filter ((==kind) . sampleBoundary) interval) == 1) SubmissionRejected)
    [SubmissionEntry,AuxiliaryReturn,CreationCall,CreationReturn,ActivationCall,ActivationReturn,WrapperReturn]
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
  require (pool >= 0xa10200 && toInteger pool+1024 <= 0xa5b200 && size == 1024) SubmissionRejected
  let classNumber = case core of PrimaryKernel -> 1; SecondaryKernel -> 2
      encoded = classNumber `shiftL` 16 .|. fromIntegral number
      constructor = case core of PrimaryKernel -> 0x4d094; SecondaryKernel -> 0xdd054
      activation = case core of PrimaryKernel -> 0x44f2c; SecondaryKernel -> 0xd4eec
      creationState = contextState (sampleContext creationCall); activationState = contextState (sampleContext activationCall)
  contextRegister 0 creationCall >>= \actual -> require (actual == encoded) SubmissionRejected
  contextRegister 1 creationCall >>= \actual -> require (actual == sp-48) SubmissionRejected
  contextRegister 3 creationCall >>= \actual -> require (actual == constructor) SubmissionRejected
  contextRegister 0 activationCall >>= \actual -> require (actual == encoded) SubmissionRejected
  contextRegister 3 activationCall >>= \actual -> require (actual == activation) SubmissionRejected
  require (W.contextSp creationState == W.Address (sp-56) && W.contextSp activationState == W.Address (sp-56)
    && testBit (value (W.contextCpsr activationState)) 7) SubmissionRejected
  callMemory <- localMemory creationCall
  callParameters <- mapM (\offset -> readWord SubmissionRejected callMemory (physical sp-48+offset)) [0,4..20]
  require (callParameters == [2,0,entry,priority+1,1024,pool]) SubmissionRejected
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
  require (count trampolineCount == count stackCount+1 && count entryCount == count transferCount+1) FirstTaskRejected
  let entryPc = address (firstEntry first)
  registers <- case entryPc of 0x69d160 -> Right [4,5,11,14]; 0x69cbf0 -> Right [11]; 0x69cd58 -> Right [4,5,6,11,14]; _ -> Left PrologueRejected
  let frameBytes = 4*fromIntegral (length registers); expectedSp = address (firstStackTop first)-frameBytes
      corePosition = coreIndex core
  require (W.contextPc after == W.Address (entryPc+4) && W.contextSp after == W.Address expectedSp && W.contextRegisters after == W.contextRegisters state
    && W.contextLr after == W.contextLr state && W.contextCpsr after == W.contextCpsr state) PrologueRejected
  beforeCount <- at PrologueRejected (W.contextCounts state) corePosition
  afterCount <- at PrologueRejected (W.contextCounts after) corePosition
  require (count afterCount == count beforeCount+1) PrologueRejected
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
admitReceipt inputs@(Inputs _ expectedIdentity) observed = do
  require (B.length (observedReceipt observed) <= 128*1024*1024 && B.length (observedWatch observed) <= 32*1024*1024) ReceiptDecodeRejected
  receipt <- decode ReceiptDecodeRejected (observedReceipt observed)
  watch <- decode RawWatchRejected (observedWatch observed)
  require (receiptSchema receipt == 1 && receiptScope receipt == StockStartupScope
    && receiptOutcome receipt == ObservedMenu && receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing
    && receiptWatch receipt == WatchOk True Nothing && receiptWatchEndError receipt == Nothing && receiptMenu receipt == MenuOk (Just [1,5,2])
    && receiptInstructions receipt == W.Count 382000000) StatusRejected
  require (receiptIdentity receipt == expectedIdentity) IdentityRejected
  require (receiptCheckpointReused receipt && receiptCheckpoint receipt == hash (observedReset observed) && not (B.null (observedReset observed))) ResetRejected
  require (receiptWatchDigest receipt == hash (observedWatch observed)) RawWatchRejected
  require (W.watchEnabled watch && W.watchViolation watch == Nothing && W.watchViews watch == "observe"
    && W.watchClasses watch == expectedClasses inputs && W.watchRanges watch == unionRanges (map W.classRange (expectedClasses inputs))
    && not (null (W.watchRecords watch)) && length (W.watchRecords watch) < 1048576) WatchRejected
  let clocks = mapMaybe W.eventInstructions (W.watchRecords watch); ticks = mapMaybe W.eventTick (W.watchRecords watch)
  require (length clocks == length (W.watchRecords watch) && length ticks == length clocks && clocks == sort clocks && ticks == sort ticks) ClockRejected
  require (receiptSites receipt == expectedSites && not (null (receiptSamples receipt)) && length (receiptHits receipt) < 4096) SnapshotRejected
  first <- at SnapshotRejected (receiptSamples receipt) 0
  require (map (index . sampleHit) (receiptSamples receipt) == [index (sampleHit first)..length (receiptHits receipt)-1]) SnapshotRejected
  _ <- replay inputs receipt watch
  require (sort (map submissionId (receiptSubmissions receipt)) == map TaskId [93,102,103,104]) SubmissionRejected
  mapM_ (checkSubmission receipt) (receiptSubmissions receipt)
  mapM_ (checkRunnable receipt) [PrimaryKernel,SecondaryKernel]
  require (sort (map firstCore (receiptFirstTasks receipt)) == [PrimaryKernel,SecondaryKernel]) FirstTaskRejected
  mapM_ (checkFirst receipt watch) (receiptFirstTasks receipt)
  pure (QualifiedReceipt receipt (hash (observedReceipt observed)) (length (W.watchRecords watch)))

qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt receipt (W.Digest digest) rows) = J.object
  ["schema" J..= (1 :: Int),"qualification" J..= ("stock-startup-task-observation" :: String),"receipt_sha256" J..= digest
  ,"watch_rows" J..= rows,"snapshots" J..= length (receiptSamples receipt),"submissions" J..= length (receiptSubmissions receipt)
  ,"first_tasks" J..= [J.object ["core" J..= show (firstCore first),"id" J..= (case firstId first of TaskId ident -> ident)
      ,"entry" J..= address (firstEntry first),"stack_top" J..= address (firstStackTop first)] | first <- receiptFirstTasks receipt]
  ,"scope" J..= ("Canonical stock startup task objects, auto-ready queues and both first prologues; no owned task implementation or post-entry lifetime acceptance" :: String)]
