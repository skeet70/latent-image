{-# LANGUAGE OverloadedStrings #-}
module Emulation.IdleTaskWitness
  ( SourceFiles(..), ObservedFiles(..), Inputs, QualifiedReceipt, WitnessError(..)
  , admitInputs, admitReceipt, compareReceipts, qualificationValue
  ) where

import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import Data.Aeson ((.:))
import Data.Aeson.Types (parseEither)
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (sort, find)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Maybe (isJust)
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import Emulation.IdleTaskWire
import Emulation.TaskMemoryWire
import Emulation.TaskMemoryWitness (Memory(..), CoreOrdinals, CaptureCoverage(..))
import qualified Emulation.TaskMemoryWitness as Memory
import qualified Emulation.LoaderCode as Loader
import qualified Emulation.StartupTaskWire as S
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.IdleTask.Admission as A
import qualified Replacement.IdleTask.Artifact as I
import qualified Replacement.KernelInstall.Wire as W
import StockInventory (FunctionDeclaration (..), FunctionSize (..), FunctionWindow (..), GuestAddress (..), deriveFunction, derivedDeclaration)

data Inputs = Inputs SourceFiles Role Identity I.LinkedArtifact B.ByteString
data QualifiedReceipt = QualifiedReceipt Receipt [Lifecycle]
data WitnessError = SourceRejected | ArtifactRejected | ContractRejected | CandidateRejected | ReceiptDecodeRejected | StatusRejected
  | IdentityRejected | ResetRejected | RawWatchRejected | WatchRejected | ClockRejected | FetchRejected | WriteRejected
  | ForeignWriterRejected | PreimageRejected | SnapshotRejected | AliasRejected | ContextRejected | CodeRejected | TaskRejected
  | EntryRejected | WaitRejected | IrqRejected | FrameRejected | ReturnRejected | RepetitionRejected | ComparisonRejected
  deriving (Eq, Show)

coreOrdinals :: [W.Count] -> Either WitnessError CoreOrdinals
coreOrdinals = memoryResult . Memory.coreOrdinals
coreOrdinal :: W.Core -> CoreOrdinals -> W.Count
coreOrdinal = Memory.coreOrdinal
data Lifecycle = Lifecycle S.KernelCore S.TaskId Word32 Word32 Int deriving (Eq, Show)
data Pending = Pending Sample Sample (Maybe Sample) (Maybe Sample)
data Track = Track {trackEntry :: Maybe Sample, trackWait :: Maybe Sample, trackPending :: Maybe Pending, trackCycles :: Int, trackRepeated :: Maybe Sample, trackResume :: Maybe Sample}

require :: Bool -> WitnessError -> Either WitnessError ()
require condition problem = unless condition (Left problem)
need :: WitnessError -> Maybe a -> Either WitnessError a
need problem = maybe (Left problem) Right
address :: W.Address -> Word32
address (W.Address number) = number
word :: W.WordValue -> Word32
word (W.WordValue number) = number
count :: W.Count -> Word64
count (W.Count number) = number
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes body) = body
physical :: Word32 -> Word32
physical number = number .&. 0x3fffffff
hash :: B.ByteString -> W.Digest
hash = W.Digest . SHA.sha256Hex . SHA.sha256
parse :: J.FromJSON a => WitnessError -> B.ByteString -> Either WitnessError a
parse problem raw = do
  value <- either (const (Left problem)) Right (decodeStrictIntegers raw)
  case J.fromJSON value of J.Success decoded -> Right decoded; J.Error _ -> Left problem
budget :: Word64
budget = 80000000
counts :: W.Context -> Either WitnessError Word64
counts state = do
  let values = map (toInteger . count) (W.contextCounts state); total = sum values
  require (length values == 2 && total <= toInteger budget) ClockRejected
  pure (fromInteger total)
sourceBodies :: Inputs -> [(Word32,B.ByteString)]
sourceBodies (Inputs source _ _ _ image) = [(0x40000,sourceA source),(0xd0000,sourceB source),(0x61c400,image)]
sourceWord :: Inputs -> Word32 -> Either WitnessError Word32
sourceWord inputs pc = do
  (base,body) <- need CodeRejected (find (\(base,body) -> pc >= base && toInteger pc+4 <= toInteger base+toInteger (B.length body)) (sourceBodies inputs))
  require (pc `mod` 4 == 0 && (base /= 0x61c400 || pc < 0x89ea8c)) CodeRejected
  pure (U.word32At body (fromIntegral (pc-base)))
image4CodeWindows :: [(Word32,Word32)]
image4CodeWindows = [(0x7221a0,0x7221c8),(0x7221c8,0x722218)]
immutableRanges :: [(Word64,Word64)]
immutableRanges = Loader.loaderSpans 0x40000 0xdae0 ++ Loader.loaderSpans 0xd0000 0xdaa0
  ++ [(0x69cbf0,0x69cc00),(0x96bd20,0x96bd28),(0x4dae0,0x4f10c),(0xddaa0,0xdf0cc)]
  ++ [(fromIntegral low,fromIntegral high) | (low,high) <- image4CodeWindows]
layout :: S.KernelCore -> (Word32,Word32,Word32,Word32,Word32,Word32,S.TaskId)
layout S.PrimaryKernel = (0x4005beac+40*101,0x4005ff0c+56*101,0x40054760,0x40054768,0xa10e00,0xa11200,S.TaskId 102)
layout S.SecondaryKernel = (0x400ebe6c+40*102,0x400efecc+56*102,0x400e4720,0x400e4728,0xa10200,0xa10600,S.TaskId 103)
sites :: M.Map W.Address Boundary
sites = M.fromList [(W.Address pc,boundary) | (boundary,pcs) <-
  [(Start,[0x725dd8]),(IdleEntry,[0x69cbf0]),(IdleInstruction,[0x69cbf4,0x69cbf8,0x69cbfc])
  ,(IrqEntry,[0x40204,0xd0204]),(IrqFrame,[0x40234,0xd0234]),(IrqReturn,[0x40364,0xd0364,0x437d8,0xd37d8])],pc <- pcs]
fetchRanges :: [(Word64,Word64)]
fetchRanges = [(0x40000,0x4dae0),(0xd0000,0xddaa0),(0x61c400,0x89ea8c)]
watchClasses :: [W.WatchClass]
watchClasses = [W.WatchClass [start,end] writes | (start,end) <- immutableRanges ++ dynamic]
  ++ [W.WatchClass [fromIntegral pc,fromIntegral pc+4] [W.CpuFetch] | W.Address pc <- M.keys sites]
  where
    writes = [W.CpuWrite,W.HostWrite,W.ModelBusWrite]
    dynamic = [(fromIntegral (physical base),fromIntegral (physical base)+size) | core <- [S.PrimaryKernel,S.SecondaryKernel]
      ,let (c,t,current,selected,low,high,_) = layout core,(base,size) <- [(c,40),(t,56),(current,4),(selected,4),(low,fromIntegral (high-low))]]

admitInputs :: Role -> SourceFiles -> Either WitnessError Inputs
admitInputs role source = do
  mapM_ (\(body,digest) -> require (hash body == W.Digest digest) SourceRejected)
    [(sourceDat source,"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    ,(sourceImage source,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    ,(sourceA source,"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
    ,(sourceB source,"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
    ,(sourceRun source,"efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef")
    ,(sourceRom source,"4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be")]
  mapM_ (\(low,high) -> do
    derived <- either (const (Left CodeRejected)) Right
      (deriveFunction (GuestAddress 0x61c400) (sourceImage source) (FunctionWindow (GuestAddress low) (GuestAddress high)) [])
    require (derivedDeclaration derived == FunctionDeclaration (GuestAddress low) (FunctionSize (fromIntegral (high-low)))) CodeRejected) image4CodeWindows
  canonical <- either (const (Left ArtifactRejected)) Right (I.canonicalSource (sourceDat source) (sourceImage source))
  artifact <- either (const (Left ArtifactRejected)) Right (I.deriveArtifact (sourceLinked source) (sourceSymbols source))
  let staged = I.stageImage4 canonical artifact; expectedImage = I.stagedBytes staged
  contract <- either (const (Left ContractRejected)) Right (decodeStrictIntegers (sourceContract source))
  (inner,datIdentity,imageIdentity) <- case parseEither contractParser contract of
    Left _ -> Left ContractRejected; Right fields -> Right fields
  require (inner == I.contractValue staged && imageIdentity == hash expectedImage) ContractRejected
  let runtimeImage = case role of Retained -> sourceImage source; Owned -> expectedImage
  require (runDecoded source == runtimeImage) CandidateRejected
  case role of
    Retained -> require (runDat source == sourceDat source) CandidateRejected
    Owned -> do
      require (hash (runDat source) == datIdentity) CandidateRejected
      original <- either (const (Left CandidateRejected)) Right (U.parseContainer (sourceDat source))
      candidate <- either (const (Left CandidateRejected)) Right (U.parseContainer (runDat source))
      let old = U.parsedParts original; new = U.parsedParts candidate
          restored = new {U.partRecord4=U.partRecord4 old,U.partGap45=U.partGap45 old}
      restoredDat <- either (const (Left CandidateRejected)) Right (U.rebuildContainer candidate restored)
      require (restoredDat == sourceDat source && U.recDecodedSize (U.recHeader (U.partRecord4 new)) == fromIntegral (B.length runtimeImage)) CandidateRejected
  let identity = Identity (hash (sourceRun source)) (hash (runDat source)) (hash (sourceRom source)) (hash (sourceObserver source))
        (hash (sourceLinked source)) (hash (sourceSymbols source)) (hash (sourceContract source))
        [S.SourceIdentity kind (W.Address base) (B.length body) (hash body) | (kind,base,body) <-
          [(S.LoaderA,0x40000,sourceA source),(S.LoaderB,0xd0000,sourceB source),(S.Image4,0x61c400,sourceImage source)]]
  pure (Inputs source role identity artifact runtimeImage)
  where
    contractParser = J.withObject "idle staging contract" $ \o -> do
      schema <- o .: "schema"; qualification <- o .: "qualification"
      unless (schema == (1 :: Int) && qualification == ("host-admitted-idle-task" :: String)) (fail "invalid idle staging contract")
      (,,) <$> o .: "idle_task" <*> o .: "candidate_dat_sha256" <*> o .: "candidate_image4_sha256"

mergeView :: Memory -> View -> Either WitnessError Memory
mergeView (Memory memory) view = Memory <$> foldM insert memory (zip [address (viewPhysical view)..] (B.unpack (bytes (viewBytes view))))
  where insert contents (target,value) = case M.lookup target contents of
          Just previous -> require (previous == value) SnapshotRejected >> pure contents
          Nothing -> pure (M.insert target value contents)
readBytes :: WitnessError -> Memory -> Word32 -> Int -> Either WitnessError B.ByteString
readBytes problem (Memory memory) start size = B.pack <$> mapM (\offset -> need problem (M.lookup (start+fromIntegral offset) memory)) [0..size-1]
readWord :: WitnessError -> Memory -> Word32 -> Either WitnessError Word32
readWord problem memory target = (`U.word32At` 0) <$> readBytes problem memory target 4
viewShape :: View -> (Object,S.KernelCore,Word32,Int)
viewShape view = (viewObject view,viewCore view,address (viewPhysical view),B.length (bytes (viewBytes view)))
checkViews :: Sample -> Memory -> Either WitnessError Memory
checkViews sample previous = do
  mapM_ check (sampleViews sample)
  foldM mergeView previous (filter ((/=LoadedCode) . viewObject) (sampleViews sample))
  where
    state = contextState (sampleContext sample)
    check view = do
      let body = bytes (viewBytes view); start = address (viewPhysical view); expected = hash body
      require (not (B.null body) && toInteger start+toInteger (B.length body) <= 0x20000000 && viewAliases view == replicate 2 (replicate 2 expected)) AliasRejected
      mapM_ (\banks -> do
        require (length banks == 2) ContextRejected
        mapM_ (\(W.WordValue bank) -> if bank .&. 1 == 0 || (bank `shiftR` 2) .&. 31 == 0 then pure () else do
          let sizeLog2 = (bank `shiftR` 2) .&. 31; low = fromIntegral (bank .&. 0xfffff000) :: Word64; extent = 512 `shiftL` fromIntegral sizeLog2
          require (sizeLog2 >= 3 && sizeLog2 <= 14 && low `mod` extent == 0 && low+extent <= 0x100000000) ContextRejected
          mapM_ (\alias -> let begin = fromIntegral start+alias; end = begin+fromIntegral (B.length body)
            in require (end <= low || begin >= low+extent) ContextRejected) [0,0x40000000]) banks) (W.contextTcm state)

checkShape :: Inputs -> Sample -> Memory -> Either WitnessError ()
checkShape inputs sample memory = do
  let context = sampleContext sample; state = contextState context
      common = [(Code,S.PrimaryKernel,0x69cbf0,16),(Code,contextCore context,physical (address (W.contextPc state)),4)
        ,(InterfaceRoots,S.PrimaryKernel,0x96bd20,8),(InterfaceTable,S.PrimaryKernel,0x4dae0,0x162c),(InterfaceTable,S.SecondaryKernel,0xddaa0,0x162c)]
  perCore <- fmap concat $ mapM (\core -> do
    let (c,t,current,selected,low,high,_) = layout core
    created <- readWord SnapshotRejected memory (physical c+36)
    require (created <= 1) TaskRejected
    pure ([(CreationRecord,core,physical c,40),(TaskNode,core,physical t,56),(Current,core,physical current,4)
      ,(Selected,core,physical selected,4),(Pool,core,low,fromIntegral (high-low))]
      ++ [(Stack,core,low,fromIntegral (high-low)) | created == 1])) [S.PrimaryKernel,S.SecondaryKernel]
  let installed = [(LoadedCode,if base == 0xd0000 then S.SecondaryKernel else S.PrimaryKernel,base,B.length (prefix base body))
        | (base,body) <- sourceBodies inputs,sampleBoundary sample == Start]
      prefix base body = if base == 0x61c400 then B.take (0x89ea8c-0x61c400) body else body
  require (sort (map viewShape (sampleViews sample)) == sort (common ++ perCore ++ installed)) SnapshotRejected
  mapM_ (\view -> case find (\(base,_) -> base == address (viewPhysical view)) (sourceBodies inputs) of
    Just (base,body) -> require (bytes (viewBytes view) == prefix base body) CodeRejected
    Nothing -> Left CodeRejected) (filter ((==LoadedCode) . viewObject) (sampleViews sample))
  mapM_ (\(target,expected) -> readWord SnapshotRejected memory target >>= \actual -> require (actual == expected) SnapshotRejected)
    [(0x96bd20,0x4004dae0),(0x96bd24,0x400ddaa0)
    ,(0x4dae0+0x1618,0x412bc),(0x4dae0+0x164,0x41140),(0x4dae0+0x168,0x41164)
    ,(0xddaa0+0x164,0xd1140),(0xddaa0+0x168,0xd1164)]

checkIdleTask :: Sample -> Memory -> S.KernelCore -> Either WitnessError ()
checkIdleTask _ memory core = do
  let (c,t,current,selected,low,high,S.TaskId ident) = layout core
  record <- mapM (readWord TaskRejected memory . (physical c+)) [0,4..36]
  require (take 6 record == [2,0,0x69cbf0,14,high-low,low]
    && drop 8 record == [fromIntegral ident,1]) TaskRejected
  pointer <- readWord TaskRejected memory (physical t+8)
  state <- readBytes TaskRejected memory (physical t+12) 3
  require (pointer == c && B.index state 0 == 1 && B.index state 1 == 14) TaskRejected
  mapM_ (\cell -> readWord TaskRejected memory (physical cell) >>= \node -> require (node == t) TaskRejected) [current,selected]

checkSample :: Inputs -> Hit -> Int -> Sample -> Memory -> Either WitnessError Memory
checkSample inputs hit index sample previous = do
  let context = sampleContext sample; state = contextState context
  total <- counts state
  require (sampleHit sample == index && length (W.contextRegisters state) == 13 && length (W.contextTcm state) == 2
    && W.contextIdentities state == map W.WordValue [0x4003,0x5003]
    && M.lookup (W.contextPc state) sites == Just (sampleBoundary sample)) ContextRejected
  let recorded = hitState hit
  require (hitBoundary hit == sampleBoundary sample && hitCore hit == contextCore context && count (hitCount hit) == total
    && W.contextPc recorded == W.contextPc state && W.contextRegisters recorded == W.contextRegisters state
    && W.contextSp recorded == W.contextSp state && W.contextLr recorded == W.contextLr state && W.contextCpsr recorded == W.contextCpsr state) ContextRejected
  expected <- sourceWord inputs (address (W.contextPc state))
  require (sampleCodeWord sample == W.WordValue expected) CodeRejected
  memory <- checkViews sample previous
  checkShape inputs sample memory
  actualWord <- readWord CodeRejected memory (physical (address (W.contextPc state)))
  require (actualWord == expected) CodeRejected
  mapM_ (\pc -> do actual <- readWord CodeRejected memory pc; source <- sourceWord inputs pc; require (actual == source) CodeRejected) [0x69cbf0,0x69cbf4,0x69cbf8,0x69cbfc]
  case sampleBoundary sample of
    IdleEntry -> checkIdleTask sample memory (contextCore context)
    IdleInstruction -> checkIdleTask sample memory (contextCore context)
    _ -> pure ()
  pure memory

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
watchPlan :: Memory.CapturePlan
watchPlan = Memory.CapturePlan (W.Count budget) watchClasses (map spanOf fetchRanges)
  (map spanOf immutableRanges) irqCores []
  where
    spanOf (low,high) = Memory.PhysicalSpan (PhysicalBoundary low) (PhysicalBoundary high)
    irqCores = M.fromList [(pc,if address pc < 0xd0000 then W.Primary else W.Secondary)
      | (pc,boundary) <- M.toList sites,boundary `elem` [IrqEntry,IrqFrame,IrqReturn]]
planFor :: Inputs -> Memory.CapturePlan
planFor inputs = watchPlan {Memory.planCode =
  [Memory.CodeImage (W.Address base) (W.Bytes (if base == 0x61c400 then B.take (0x89ea8c-0x61c400) body else body))
  | (base,body) <- sourceBodies inputs]}
checkFetch :: CaptureCoverage -> W.MemoryEvent -> Either WitnessError ()
checkFetch coverage = memoryResult . Memory.checkFetch watchPlan coverage
advance :: Inputs -> CoreOrdinals -> CoreOrdinals -> Word64 -> Word64
  -> (Memory,Word64,CoreOrdinals) -> CapturedEvent -> Either WitnessError (Memory,Word64,CoreOrdinals)
advance inputs previousCounts nextCounts lower upper state observed =
  memoryResult (Memory.advance (planFor inputs) previousCounts nextCounts lower upper state observed)
checkWatch :: CapturedWatch -> Either WitnessError ()
checkWatch = memoryResult . Memory.checkWatch watchPlan

replay :: Inputs -> Receipt -> CapturedWatch -> Either WitnessError [Memory]
replay inputs receipt watch = do
  require (not (null samples) && length samples <= 16384 && length samples == length (receiptHits receipt)) SnapshotRejected
  first <- need SnapshotRejected (case samples of item:_ -> Just item; [] -> Nothing)
  require (sampleBoundary first == Start && sampleCursor first == 0 && sampleHit first == 0) SnapshotRejected
  firstHit <- need ContextRejected (case receiptHits receipt of item:_ -> Just item; [] -> Nothing)
  initial <- checkSample inputs firstHit 0 first (Memory M.empty)
  (_,_,_,_,memories) <- foldM next (first,0,rows,initial,[initial]) (zip3 [1..] (drop 1 samples) (drop 1 (receiptHits receipt)))
  require (sampleCursor (last samples) == rowCount) WatchRejected
  pure (reverse memories)
  where
    samples = receiptSamples receipt; rows = captureEvents watch; rowCount = length rows
    next (previous,cursor,remaining,memory,memories) (index,sample,hit) = do
      lower <- counts (contextState (sampleContext previous)); upper <- counts (contextState (sampleContext sample))
      let oldCounts = map count (W.contextCounts (contextState (sampleContext previous)))
          newCounts = map count (W.contextCounts (contextState (sampleContext sample)))
          end = sampleCursor sample
      require (and (zipWith (<=) oldCounts newCounts) && upper >= lower && end > cursor && end <= rowCount) ClockRejected
      let (segment,rest) = splitAt (end-cursor) remaining
      stopped <- need FetchRejected (case reverse segment of item:_ -> Just item; [] -> Nothing)
      checkFetch ClassifiedAccess (eventRecord stopped)
      let context = sampleContext sample; state = contextState context; source = eventRecord stopped
          core = S.watchCore (contextCore context)
      oldOrdinals <- coreOrdinals (W.contextCounts (contextState (sampleContext previous)))
      newOrdinals <- coreOrdinals (W.contextCounts state)
      require (W.eventCore source == Just core && W.eventPc source == Just (W.contextPc state)
        && W.eventInstructions source == Just (W.Count upper) && eventCoreInstructions stopped == Just (coreOrdinal core newOrdinals)
        && eventLatestFetch stopped == Nothing) FetchRejected
      (updated,_,_) <- foldM (advance inputs oldOrdinals newOrdinals lower upper) (memory,lower,oldOrdinals) (init segment)
      checked <- checkSample inputs hit index sample updated
      pure (sample,end,rest,checked,checked:memories)

proofFor :: Inputs -> Either WitnessError A.IdleProof
proofFor (Inputs source role _ artifact _) = case role of
  Owned -> Right (I.artifactProof artifact)
  Retained -> either (const (Left ArtifactRejected)) Right (A.deriveIdleProof (B.take 16 (B.drop (0x69cbf0-0x61c400) (sourceImage source))))
stateOf :: Sample -> W.Context
stateOf = contextState . sampleContext
sameRegisters :: Sample -> Sample -> Bool
sameRegisters before after = W.contextRegisters (stateOf before) == W.contextRegisters (stateOf after)
  && W.contextSp (stateOf before) == W.contextSp (stateOf after)
  && W.contextLr (stateOf before) == W.contextLr (stateOf after)
  && W.contextCpsr (stateOf before) == W.contextCpsr (stateOf after)
currentTask :: Memory -> S.KernelCore -> Either WitnessError Bool
currentTask memory core = do
  let (_,node,current,_,_,_,_) = layout core
  actual <- readWord TaskRejected memory (physical current)
  pure (actual == node)
frameWords :: Memory -> Sample -> Int -> Either WitnessError [Word32]
frameWords memory sample size = mapM (readWord FrameRejected memory . (address (W.contextSp (stateOf sample))+)) [0,4..4*fromIntegral size-4]
savedWords :: Sample -> Either WitnessError [Word32]
savedWords waiting = do
  let state = stateOf waiting; registers = map word (W.contextRegisters state)
  require (length registers == 13) ContextRejected
  pure ([registers !! i | i <- [0,1,2,3,12]] ++ [address (W.contextLr state),address (W.contextPc state)+4])
checkFrame :: A.IdleProof -> Sample -> Sample -> Memory -> Bool -> Either WitnessError ()
checkFrame proof waiting sample memory exiting = do
  let saved = stateOf waiting; actual = stateOf sample; displacement = if exiting then 28 else 32
      expectedSp = toInteger (address (W.contextSp saved))-displacement
  require (expectedSp >= 0 && toInteger (address (W.contextSp actual)) == expectedSp && word (W.contextCpsr actual) .&. 0xbf == 0x93) FrameRejected
  payload <- savedWords waiting
  let expected = if exiting then payload else word (W.contextCpsr saved):payload
  actualWords <- frameWords memory sample (length expected)
  require (actualWords == expected && last payload == A.resumePc proof) FrameRejected
  if exiting then require (contextSpsr (sampleContext sample) == W.contextCpsr saved) ReturnRejected else pure ()

recordWait :: A.IdleProof -> Sample -> Sample -> Track -> Either WitnessError Track
recordWait proof entry sample track = do
  let original = stateOf entry; state = stateOf sample
      (_,_,_,_,low,high,_) = layout (contextCore (sampleContext sample))
      unchanged = all (\index -> W.contextRegisters state !! index == W.contextRegisters original !! index) ([0..10] ++ [12])
  require (unchanged && W.contextLr state == W.contextLr original && W.contextCpsr state == W.contextCpsr original
    && address (W.contextSp state) >= low+32 && address (W.contextSp state) <= high
    && toInteger (address (W.contextSp state)) >= toInteger high+toInteger (A.scratchLow proof)) WaitRejected
  case trackResume track of
    Just resumed -> require (sameRegisters resumed sample) ReturnRejected
    Nothing -> pure ()
  let repeated = case (trackRepeated track,trackResume track) of
        (Nothing,Just _) | trackCycles track >= 1 -> Just sample
        (previous,_) -> previous
  pure track {trackWait=Just sample,trackRepeated=repeated,trackResume=Nothing}

checkLifecycles :: Inputs -> Receipt -> CapturedWatch -> [Memory] -> Either WitnessError [Lifecycle]
checkLifecycles inputs receipt watch memories = do
  proof <- proofFor inputs
  tracks <- foldM (step proof) (M.fromList [(core,emptyTrack) | core <- [S.PrimaryKernel,S.SecondaryKernel]]) (zip (receiptSamples receipt) memories)
  mapM (finish tracks) [S.PrimaryKernel,S.SecondaryKernel]
  where
    emptyTrack = Track Nothing Nothing Nothing 0 Nothing Nothing
    executed = go 0 (receiptSamples receipt) (captureEvents watch)
      where go _ [] _ = []
            go cursor (sample:rest) rows =
              let (segment,remaining) = splitAt (sampleCursor sample-cursor) rows
              in (if null segment then [] else init segment) ++ go (sampleCursor sample) rest remaining
    executedFetches = M.fromListWith Set.union
      [((core,pc),Set.singleton (count clock)) | observed <- executed,let event=eventRecord observed,W.eventSource event == W.CpuFetch
        ,Just core <- [W.eventCore event],Just pc <- [W.eventPc event],Just clock <- [W.eventInstructions event]]
    executedWait waiting upper =
      let key = (S.watchCore (contextCore (sampleContext waiting)),W.contextPc (stateOf waiting))
          lower = sum (map count (W.contextCounts (stateOf waiting)))
      in maybe False (<=upper) (Set.lookupGT lower (M.findWithDefault Set.empty key executedFetches))
    finish tracks core = do
      track <- need RepetitionRejected (M.lookup core tracks)
      repeated <- need RepetitionRejected (trackRepeated track)
      let ending = sum (map count (W.contextCounts (stateOf (last (receiptSamples receipt)))))
      require (isJust (trackEntry track) && trackCycles track >= 1 && executedWait repeated ending) RepetitionRejected
      let (_,_,_,_,low,high,ident) = layout core
      pure (Lifecycle core ident low high (trackCycles track))
    step proof tracks (sample,memory) = do
      let core = contextCore (sampleContext sample); state = stateOf sample; pc = address (W.contextPc state)
          (_,_,_,_,low,high,_) = layout core
      track <- need ContextRejected (M.lookup core tracks)
      actualIdle <- currentTask memory core
      next <- case sampleBoundary sample of
        Start -> pure track
        IdleEntry -> case trackEntry track of
          Nothing -> do
            argument <- need EntryRejected (case W.contextRegisters state of first:_ -> Just first; [] -> Nothing)
            require (address (W.contextSp state) == high && word argument == 0
              && word (W.contextCpsr state) .&. 0xff == 0x53
              && address (W.contextLr state) == case core of S.PrimaryKernel -> 0x453f0; S.SecondaryKernel -> 0xd53b0) EntryRejected
            let entered = track {trackEntry=Just sample}
            if pc == A.wfiPc proof then recordWait proof sample sample entered else pure entered
          Just entry -> do
            require (pc == A.wfiPc proof) EntryRejected
            recordWait proof entry sample track
        IdleInstruction -> do
          entry <- need EntryRejected (trackEntry track)
          require (pc `elem` A.executionPcs proof && word (W.contextCpsr state) .&. 0xff == 0x53
            && address (W.contextSp state) >= low+32 && address (W.contextSp state) <= high) WaitRejected
          if pc == A.wfiPc proof then recordWait proof entry sample track
          else if pc == A.resumePc proof then case trackPending track of
            Just (Pending waiting _ _ (Just _)) -> do
              require (sameRegisters waiting sample) ReturnRejected
              pure track {trackPending=Nothing,trackCycles=trackCycles track+1,trackResume=Just sample}
            Nothing -> do
              waiting <- need ReturnRejected (trackWait track)
              total <- counts state
              require (sameRegisters waiting sample && executedWait waiting total) ReturnRejected
              pure track {trackResume=Just sample}
            _ -> Left ReturnRejected
          else pure track
        IrqEntry -> case (trackWait track,trackPending track,trackResume track) of
          (Just waiting,Nothing,Nothing) | actualIdle && address (W.contextLr state) == A.resumePc proof+4 -> do
              total <- counts state
              require (executedWait waiting total) WaitRejected
              require (word (W.contextCpsr state) .&. 0xbf == 0x92
                && contextSpsr (sampleContext sample) == W.contextCpsr (stateOf waiting)
                && W.contextRegisters state == W.contextRegisters (stateOf waiting)) IrqRejected
              pure track {trackPending=Just (Pending waiting sample Nothing Nothing)}
          _ -> pure track
        IrqFrame -> case trackPending track of
          Just (Pending waiting entered Nothing Nothing) -> do
            checkFrame proof waiting sample memory False
            pure track {trackPending=Just (Pending waiting entered (Just sample) Nothing)}
          _ -> pure track
        IrqReturn -> case trackPending track of
          Just (Pending waiting entered (Just framed) Nothing)
            | actualIdle && address (W.contextSp state)+28 == address (W.contextSp (stateOf waiting)) -> do
                checkFrame proof waiting sample memory True
                pure track {trackPending=Just (Pending waiting entered (Just framed) (Just sample))}
          _ -> pure track
      pure (M.insert core next tracks)

admitReceipt :: Inputs -> ObservedFiles -> Either WitnessError QualifiedReceipt
admitReceipt inputs@(Inputs _ role identity _ _) observed = do
  receipt <- parse ReceiptDecodeRejected (observedReceipt observed)
  require (receiptSchema receipt == 1 && receiptRole receipt == role && receiptOutcome receipt == S.ObservedMenu
    && receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing && receiptWatchEndError receipt == Nothing
    && receiptWatch receipt == S.WatchOk True Nothing && receiptMenu receipt == S.MenuOk (Just [1,5,2])) StatusRejected
  require (receiptIdentity receipt == identity) IdentityRejected
  require (receiptCheckpointReused receipt && hash (observedReset observed) == receiptCheckpoint receipt) ResetRejected
  require (hash (observedWatch observed) == receiptWatchDigest receipt) RawWatchRejected
  watch <- parse RawWatchRejected (observedWatch observed)
  checkWatch watch
  require (receiptSites receipt == sites) ContextRejected
  memories <- replay inputs receipt watch
  let samples = receiptSamples receipt
  final <- need SnapshotRejected (case reverse samples of item:_ -> Just item; [] -> Nothing)
  require (receiptEndCounts receipt == Just (W.contextCounts (stateOf final)) && count (receiptInstructions receipt) >= sum (map count (W.contextCounts (stateOf final)))
    && count (receiptInstructions receipt) <= 500000000) ClockRejected
  lifecycles <- checkLifecycles inputs receipt watch memories
  pure (QualifiedReceipt receipt lifecycles)

coreValue :: S.KernelCore -> String
coreValue S.PrimaryKernel = "primary"
coreValue S.SecondaryKernel = "secondary"
qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt receipt lifecycles) = J.object
  [ "schema" J..= (1 :: Int), "qualification" J..= ("natural-idle-task-lifecycle" :: String), "role" J..= receiptRole receipt
  , "snapshots" J..= length (receiptSamples receipt)
  , "tasks" J..= [J.object ["core" J..= coreValue core,"id" J..= ident,"stack" J..= [low,high],"irq_cycles" J..= cycles]
      | Lifecycle core (S.TaskId ident) low high cycles <- lifecycles]
  , "immutable_ranges" J..= [[low,high] | (low,high) <- immutableRanges]
  , "stock_inventory_image4_code" J..= [[low,high] | (low,high) <- image4CodeWindows]
  , "scope" J..= ("Both initial idle bodies through real IRQ preemption/frame/return and repeated wait; no root/standby or general scheduler retirement" :: String)]
compareReceipts :: QualifiedReceipt -> QualifiedReceipt -> Either WitnessError J.Value
compareReceipts (QualifiedReceipt retained left) (QualifiedReceipt owned right) = do
  require (receiptRole retained == Retained && receiptRole owned == Owned) ComparisonRejected
  let semantic = map (\(Lifecycle core ident low high _) -> (core,ident,low,high))
      first = receiptIdentity retained; second = receiptIdentity owned
  require (semantic left == semantic right && identityRun first == identityRun second && identityRom first == identityRom second
    && identityLinked first == identityLinked second && identitySymbols first == identitySymbols second && identityContract first == identityContract second
    && identitySources first == identitySources second && identityObserver first == identityObserver second) ComparisonRejected
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("retained-owned-idle-task-comparison" :: String)
    ,"tasks" J..= ([102,103] :: [Int]),"compared" J..= (["natural task/core/stack identities","IRQ and register preservation within each run","actual IRQ frames and restored return","repeated WFI without forbidden effects"] :: [String])])
