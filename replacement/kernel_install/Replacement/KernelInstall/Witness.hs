{-# LANGUAGE OverloadedStrings #-}
module Replacement.KernelInstall.Witness
  ( SourceFiles (..), NativeFiles (..), ProducerFiles (..), ObservedFiles (..), Inputs, QualifiedReceipt
  , WitnessError (..), admitInputs, admitReceipt, compareReceipts, qualificationValue
  ) where

import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Replacement.KernelInstall.Admission
import qualified Replacement.KernelInstall.Artifact as A
import Replacement.KernelInstall.Wire


data SourceFiles = SourceFiles {sourceDat :: B.ByteString, sourceImage :: B.ByteString, sourceA :: B.ByteString, sourceB :: B.ByteString}
data NativeFiles = NativeFiles {nativeImage :: B.ByteString, nativeSymbols :: B.ByteString}
data ProducerFiles = ProducerFiles {producerExecutable :: B.ByteString, producerContract :: B.ByteString, producerCandidate :: B.ByteString}
data ObservedFiles = ObservedFiles {observedReceipt :: B.ByteString, observedCheckpoint :: B.ByteString, observedWatch :: B.ByteString}
data Inputs = Inputs
  { inputSource :: SourceFiles, inputNative :: NativeFiles, inputProducer :: ProducerFiles
  , inputProof :: InstallationProof, inputOwned :: B.ByteString
  }
data QualifiedReceipt = QualifiedReceipt Receipt Digest

data WitnessError
  = SourceRejected | ArtifactRejected | CandidateRejected | ContractRejected | ReceiptDecodeRejected
  | ProducerIdentityRejected | ReceiptStatusRejected | ContextRejected | CacheRejected | TerminalRejected
  | MutationRejected | ViewsRejected | CodeRejected | WatchRejected | EventRejected | EventOrderRejected
  | FetchRejected | ReadRejected | WriteRejected | PreimageRejected | StoreOrderRejected | ReadbackRejected
  | FrameRejected | ComparisonRejected | CheckpointRejected | RawWatchRejected
  deriving (Eq, Show)

require :: Bool -> WitnessError -> Either WitnessError ()
require condition failure = unless condition (Left failure)
required :: WitnessError -> Maybe value -> Either WitnessError value
required failure = maybe (Left failure) Right
hash :: B.ByteString -> Digest
hash = Digest . SHA.sha256Hex . SHA.sha256

admitInputs :: SourceFiles -> NativeFiles -> A.StagedCandidate -> ProducerFiles -> Either WitnessError Inputs
admitInputs sources native expected producer = do
  canonical <- either (const (Left SourceRejected)) Right
    (A.canonicalSource (sourceDat sources) (sourceImage sources) (sourceA sources) (sourceB sources))
  artifact <- either (const (Left ArtifactRejected)) Right (A.deriveArtifact (nativeImage native) (nativeSymbols native))
  proof <- either (const (Left ArtifactRejected)) Right (deriveInstallationProof (nativeImage native))
  require (producerCandidate producer == A.candidateBytes expected) CandidateRejected
  require (producerContract producer == L.toStrict (J.encode (A.candidateContractValue expected))) ContractRejected
  pure (Inputs sources native producer proof (A.stagedBytes (A.stageImage4 canonical artifact)))

word :: Word32 -> [Word8]
word value = [fromIntegral (value `shiftR` amount) | amount <- [0,8,16,24]]
address :: Address -> Word32
address (Address value) = value
number :: WordValue -> Word32
number (WordValue value) = value
count :: Count -> Word64
count (Count value) = value
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff

layout :: Context -> [(ObjectKind,Word32,Int)]
layout entry = [(Roots,0x96bd10,64),(ServiceStorage,0x9c34f0,0xf64),(TaskStorage,0xa0f85c,0x998)
  ,(Stack,physical (address (contextSp entry)-64),128),(Code,0x725a04,336),(Cache,0x42634,0x74)
  ,(Header0,0x40010,4),(Header1,0xd0010,4),(Marker,0x7caf3c,4),(Mode,0x61c008,4)
  ,(PeerCode,0xd0a44,32),(Completion,0xa00c4,4)]

type Memory = M.Map Word32 Word8
memory :: [View] -> Memory
memory views = M.fromList [(address (viewPhysical view)+fromIntegral offset,byte)
  | view <- views, viewCore view == Primary, viewGuest view == viewPhysical view
  , let Bytes bytes = viewBytes view, (offset,byte) <- zip [0 :: Int ..] (B.unpack bytes)]

bytesAt :: Memory -> Word32 -> Int -> Either WitnessError B.ByteString
bytesAt contents start size = B.pack <$> mapM (\offset -> required ViewsRejected (M.lookup (start+fromIntegral offset) contents)) [0..size-1]
wordAt :: Memory -> Word32 -> Either WitnessError Word32
wordAt contents start = do
  bytes <- bytesAt contents start 4
  pure (U.word32At bytes 0)

checkViews :: Context -> [View] -> Either WitnessError Memory
checkViews entry views = do
  let expected = [(object,core,base+alias,base,size) | (object,base,size) <- layout entry, core <- [Primary,Secondary], alias <- [0,0x40000000]]
      actual = [(viewObject view,viewCore view,address (viewGuest view),address (viewPhysical view),B.length bytes) | view <- views, let Bytes bytes = viewBytes view]
  require (sort actual == sort expected) ViewsRejected
  let contents = memory views
  mapM_ (\view -> do
    expectedBytes <- bytesAt contents (address (viewPhysical view)) (let Bytes bytes = viewBytes view in B.length bytes)
    require (viewBytes view == Bytes expectedBytes) ViewsRejected) views
  pure contents

checkCode :: Inputs -> Program -> Memory -> Either WitnessError ()
checkCode inputs program contents = do
  let sources = inputSource inputs
      image = if program == Owned then inputOwned inputs else sourceImage sources
  mapM_ (\(base,expected) -> bytesAt contents base (B.length expected) >>= \actual -> require (actual == expected) CodeRejected)
    [(0x725a04,B.take 336 (B.drop (0x725a04-0x61c400) image))
    ,(0x42634,B.take 0x74 (B.drop 0x2634 (sourceA sources)))
    ,(0xd0a44,B.take 32 (B.drop 0xa44 (sourceB sources)))]

checkContext :: Context -> Either WitnessError ()
checkContext context = require
  (length (contextRegisters context) == 13 && length (contextCounts context) == 2
    && length (contextTcm context) == 2 && all ((==2) . length) (contextTcm context)
    && contextIdentities context `elem` recordedCoreIdentities
    && number (contextCpsr context) .&. 0x07f0ffff == 0xd3) ContextRejected

sameControl :: Context -> Context -> Bool
sameControl before after = contextTcm before == contextTcm after && contextIdentities before == contextIdentities after
  && number (contextCpsr before) .&. 0x0fffffff == number (contextCpsr after) .&. 0x0fffffff

checkTcm :: Context -> Either WitnessError ()
checkTcm context = mapM_ (\register -> do
  let raw = number register
      sizeCode = (raw `shiftR` 2) .&. 31
      low = fromIntegral (raw .&. 0xfffff000) :: Word64
      high = low + (512 `shiftL` fromIntegral sizeCode)
  if raw .&. 1 == 0 then pure () else do
    require (sizeCode >= 3 && sizeCode <= 14) ContextRejected
    mapM_ (\(_,base,size) -> mapM_ (\alias ->
      let start = fromIntegral base+alias; end = start+fromIntegral size
      in require (end <= low || start >= high) ContextRejected) [0,0x40000000]) (layout context)) (concat (contextTcm context))

checkCache :: Context -> Context -> Either WitnessError ()
checkCache entry returned = do
  let input = contextRegisters entry; output = contextRegisters returned
  require (contextPc entry == Address 0x42634 && take 2 input == map WordValue [0x96bd20,32]
    && contextPc returned == contextLr entry && contextLr returned == contextLr entry
    && contextSp returned == contextSp entry && address (contextSp entry) `mod` 8 == 0
    && take 4 output == map WordValue [0x96bd40,0x96bd40,1,0]
    && drop 4 output == drop 4 input && sameControl entry returned
    && number (contextCpsr returned) `shiftR` 28 == 6) CacheRejected

expectedWrites :: Scenario -> [(Word32,Word32)]
expectedWrites scenario = [(0x96bd20,0x4004dae0),(0x96bd24,0x400ddaa0)] ++ if scenario == InvalidMarker then [] else
  [(start+4*index,0) | (start,wordsCount) <- [(0x40a0f870,120)], index <- [0..wordsCount-1]]
  ++ [(0x40a0fc30,120),(0x40a0fc34,120),(0x40a0f86c,250),(0x40a0fc38,120),(0x40a0fc3c,1),(0x40a0fc40,122),(0x409c3500,0x7531)]
  ++ [(base+4*index,0) | index <- [1..64], base <- [0x409c3600,0x409c3800]]
  ++ [(start+4*index,0) | (start,wordsCount) <- [(0x40a0fc44,360),(0x409c3904,720),(0x40a0fa50,120)], index <- [0..wordsCount-1]]

allowedFetch :: Inputs -> Program -> Core -> Word32 -> Bool
allowedFetch inputs program core pc = case core of
  Secondary -> pc >= 0xd0a44 && pc <= 0xd0a60 && pc `mod` 4 == 0
  Primary -> pc == 0x69ce38 || pc `elem` cachePcs || case program of
    Owned -> pc `elem` executionPcs (inputProof inputs)
    Retained -> pc >= 0x725a04 && pc < 0x725b44 && pc `mod` 4 == 0
  where cachePcs = [0x42634,0x42638] ++ [0x4264c,0x42650..0x4266c] ++ [0x4268c,0x42690..0x426a4]

data FetchStamp = FetchStamp {fetchPc :: Address, fetchClock :: Count}
data FetchProgress = FetchProgress
  { primaryProgress :: Count, secondaryProgress :: Count, lastFetches :: M.Map Core FetchStamp }

checkContinuity :: Observation -> [MemoryEvent] -> Either WitnessError ()
checkContinuity observation events = do
  entry <- required FetchRejected (observationEntry observation)
  cache <- required FetchRejected (observationCache observation)
  cacheReturn <- required FetchRejected (observationCacheReturn observation)
  after <- required FetchRejected (observationAfter observation)
  require (case reverse [event | event <- events,eventSource event == CpuFetch,eventCore event == Just Primary] of
    terminal:_ -> stoppedFetch after terminal
    [] -> False) FetchRejected
  -- The pinned exact scheduler runs one core slice at a time.
  -- Exact stops roll back fetch attempts; resumed entries are fetched again.
  executed <- foldM removeStopped events [cache,cacheReturn,after]
  initial <- case contextCounts entry of
    [primary,secondary] -> Right (FetchProgress primary secondary M.empty)
    _ -> Left FetchRejected
  progress <- foldM advance initial executed
  require ([primaryProgress progress,secondaryProgress progress] == contextCounts after) FetchRejected
  where
    stoppedFetch boundary event = eventSource event == CpuFetch && eventCore event == Just Primary
      && eventPc event == Just (contextPc boundary)
      && eventInstructions event == Just (Count (sum (map count (contextCounts boundary))+1))
    removeStopped remaining boundary = case break (stoppedFetch boundary) remaining of
      (prefix,_:suffix) -> Right (prefix ++ suffix)
      _ -> Left FetchRejected
    advance progress event = do
      core <- required FetchRejected (eventCore event)
      pc <- required FetchRejected (eventPc event)
      clock <- required FetchRejected (eventInstructions event)
      case eventSource event of
        CpuFetch -> do
          require (count clock == count (primaryProgress progress)+count (secondaryProgress progress)+1) FetchRejected
          pure (case core of
            Primary -> progress {primaryProgress = Count (count (primaryProgress progress)+1),lastFetches = M.insert core (FetchStamp pc clock) (lastFetches progress)}
            Secondary -> progress {secondaryProgress = Count (count (secondaryProgress progress)+1),lastFetches = M.insert core (FetchStamp pc clock) (lastFetches progress)})
        CpuRead -> dataAccess progress core pc clock
        CpuWrite -> dataAccess progress core pc clock
        _ -> Left FetchRejected
    dataAccess progress core pc clock = do
      preceding <- required FetchRejected (M.lookup core (lastFetches progress))
      require (fetchPc preceding == pc && fetchClock preceding == clock) FetchRejected
      pure progress

checkEvents :: Inputs -> Receipt -> Context -> Context -> Memory -> Watch -> Either WitnessError Memory
checkEvents inputs receipt entry after initial watch = do
  require (watchEnabled watch && watchRanges watch `elem` map backedRanges [minBound..maxBound]
    && watchClasses watch == [WatchClass range [CpuRead,CpuWrite,CpuFetch,HostWrite,ModelBusWrite] | range <- watchRanges watch]
    && watchViews watch == "observe" && watchViolation watch == Nothing
    && not (null events) && length events < 32768) WatchRejected
  let clocks = [clock | event <- events, Just clock <- [eventInstructions event]]
      ticks = [tick | event <- events, Just tick <- [eventTick event]]
      beginCount = sum (map count (contextCounts entry)); endCount = sum (map count (contextCounts after))
  require (clocks == sort clocks && ticks == sort ticks && length clocks == length events && length ticks == length events
    && all (\clock -> count clock >= beginCount && count clock <= endCount+1) clocks) EventOrderRejected
  final <- foldM apply initial events
  let stores = [(fromIntegral (eventAddress event),fromIntegral value) | event <- events,eventSource event == CpuWrite
        ,Just value <- [eventValue event],not (inStack (fromIntegral (eventAddress event)))]
  require (stores == expectedWrites (receiptScenario receipt)) StoreOrderRejected
  checkContinuity (receiptObservation receipt) events
  pure final
  where
    events = watchRecords watch
    stack = physical (address (contextSp entry))
    low = if receiptProgram receipt == Retained then stack-8 else fromIntegral (toInteger stack+toInteger (scratchLow (inputProof inputs)))
    inStack guest = guest >= 0x40000000+low && guest < 0x40000000+stack
    inCode target = target >= 0x725a04 && target < 0x725b54
    apply contents event = do
      core <- required EventRejected (eventCore event)
      pc <- address <$> required EventRejected (eventPc event)
      target64 <- required EventRejected (eventPhysical event)
      require (eventSource event `elem` [CpuRead,CpuWrite,CpuFetch] && eventAddress event <= 0xfffffffc
        && target64 == fromIntegral (physical (fromIntegral (eventAddress event)))
        && eventSize event == 4 && eventAddress event `mod` 4 == 0
        && allowedFetch inputs (receiptProgram receipt) core pc) EventRejected
      let guest = fromIntegral (eventAddress event); target = fromIntegral target64
      case eventSource event of
        CpuFetch -> do
          require (eventAddress event == fromIntegral pc && eventValue event == Nothing && eventBefore event == Nothing
            && eventByteOrder event == Just Little && allowedFetch inputs (receiptProgram receipt) core pc) FetchRejected
          pure contents
        CpuRead -> do
          require (case core of Secondary -> target == 0xa00c4
                                ; Primary -> target `elem` [0x40010,0xd0010,0x7caf3c] || inCode target || inStack guest) ReadRejected
          expected <- bytesAt contents target 4
          require (eventValue event == Just (fromIntegral (U.word32At expected 0)) && eventBefore event == Just (Bytes expected)
            && eventByteOrder event == Just Little) ReadRejected
          pure contents
        CpuWrite -> do
          require (core == Primary && (inStack guest || guest `elem` map fst (expectedWrites (receiptScenario receipt)))) WriteRejected
          expected <- bytesAt contents target 4
          require (eventBefore event == Just (Bytes expected) && eventByteOrder event == Just Little) PreimageRejected
          value <- required EventRejected (eventValue event)
          require (value <= 0xffffffff) EventRejected
          pure (foldr (uncurry M.insert) contents (zip [target..target+3] (word (fromIntegral value))))
        _ -> Left EventRejected

admitReceipt :: Inputs -> ObservedFiles -> Either WitnessError QualifiedReceipt
admitReceipt inputs observed = do
  let raw = observedReceipt observed
  receipt <- either (const (Left ReceiptDecodeRejected)) Right (J.eitherDecodeStrict' raw)
  rawWatch <- either (const (Left RawWatchRejected)) Right (J.eitherDecodeStrict' (observedWatch observed))
  let observation = receiptObservation receipt; identity = receiptIdentity receipt
      native = inputNative inputs; producer = inputProducer inputs
      update = if receiptProgram receipt == Owned then producerCandidate producer else sourceDat (inputSource inputs)
  require (identity == Identity
    (Digest "6e6ac441c0661470b5b26abe7628f19ec118ac568967938378c88dea448835cb") (hash update)
    (Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be")
    (hash (nativeImage native)) (hash (nativeSymbols native)) (hash (producerContract producer)) (hash (producerExecutable producer))) ProducerIdentityRejected
  require (receiptSchema receipt == 1 && receiptError receipt == Nothing && receiptMachineFailure receipt == Nothing
    && receiptCheckpointReused receipt && observationWatchEndError observation == Nothing
    && receiptOutcome receipt == (if receiptScenario receipt == Natural then Returned else Nonreturning)) ReceiptStatusRejected
  require (not (B.null (observedCheckpoint observed)) && receiptCheckpoint receipt == hash (observedCheckpoint observed)) CheckpointRejected
  require (observationWatch observation == Just rawWatch) RawWatchRejected
  entry <- required ContextRejected (observationEntry observation)
  cache <- required CacheRejected (observationCache observation)
  cacheReturn <- required CacheRejected (observationCacheReturn observation)
  after <- required TerminalRejected (observationAfter observation)
  mapM_ checkContext [entry,cache,cacheReturn,after]
  require (oneRecordedModel (map contextIdentities [entry,cache,cacheReturn,after])) ContextRejected
  require (all (\context -> all ((<=8300000) . count) (contextCounts context)
    && sum (map count (contextCounts context)) <= 8300000) [entry,cache,cacheReturn,after]
    && sum (map count (contextCounts entry)) <= 8000000) ContextRejected
  checkTcm entry
  require (contextPc entry == Address 0x725a04 && contextSp entry == Address 0x40063420 && contextLr entry == Address 0x69ce38
    && take 1 (contextRegisters entry) == [WordValue 0] && all (sameControl entry) [cache,cacheReturn,after]
    && and (zipWith (\before later -> and (zipWith (<=) (contextCounts before) (contextCounts later)))
      [entry,cache,cacheReturn] [cache,cacheReturn,after]) && receiptInstructions receipt == Count (sum (map count (contextCounts after)))) ContextRejected
  checkCache cache cacheReturn
  initial <- checkViews entry (observationBefore observation)
  committed <- checkViews entry (observationCommitted observation)
  checkCode inputs (receiptProgram receipt) initial
  checkCode inputs (receiptProgram receipt) committed
  mapM_ (\(target,expected) -> wordAt initial target >>= \value -> require (value == expected) ViewsRejected)
    [(0x40010,0x4004dae0),(0xd0010,0x400ddaa0),(0x61c008,0),(0xa00c4,0)]
  let marker = if receiptScenario receipt == Natural then 3 else 0
  wordAt initial 0x7caf3c >>= \value -> require (value == marker) MutationRejected
  require (observationMutation observation == (if receiptScenario receipt == Natural then Nothing else Just (Mutation (Address 0x7caf3c) (WordValue 3) (WordValue 0)))) MutationRejected
  let sp = physical (address (contextSp entry)); registers = contextRegisters after
  wordAt committed (sp-8) >>= \value -> require (value == number (contextRegisters entry !! 11)) FrameRejected
  wordAt committed (sp-4) >>= \value -> require (value == address (contextLr entry)) FrameRejected
  case receiptScenario receipt of
    Natural -> require (contextPc after == Address 0x69ce38 && contextSp after == contextSp entry
      && drop 4 registers == drop 4 (contextRegisters entry) && take 4 registers == map WordValue [0,0x40a0fc2c,0,0x40a0fc2c]
      && contextLr after == Address 0x725a40 && number (contextCpsr after) `shiftR` 28 == 6) TerminalRejected
    InvalidMarker -> do
      let target = if receiptProgram receipt == Retained then 0x725a54 else refusalPc (inputProof inputs)
      require (contextPc after == Address target) TerminalRejected
      wordAt committed target >>= \value -> require (value == 0xeafffffe) TerminalRejected
  watch <- required WatchRejected (observationWatch observation)
  final <- checkEvents inputs receipt entry after initial watch
  require (final == committed) ReadbackRejected
  pure (QualifiedReceipt receipt (hash raw))

compareReceipts :: QualifiedReceipt -> QualifiedReceipt -> Either WitnessError ()
compareReceipts (QualifiedReceipt retained _) (QualifiedReceipt owned _) = do
  require (receiptProgram retained == Retained && receiptProgram owned == Owned && receiptScenario retained == receiptScenario owned) ComparisonRejected
  beforeRetained <- required ComparisonRejected (observationEntry (receiptObservation retained))
  beforeOwned <- required ComparisonRejected (observationEntry (receiptObservation owned))
  require (beforeRetained {contextCounts = []} == beforeOwned {contextCounts = []}) ComparisonRejected
  let common views = sort [(viewObject view,viewCore view,viewGuest view,viewPhysical view,viewBytes view)
        | view <- views,viewObject view `notElem` [Code,Stack]]
      observationR = receiptObservation retained; observationO = receiptObservation owned
  require (common (observationBefore observationR) == common (observationBefore observationO)
    && common (observationCommitted observationR) == common (observationCommitted observationO)) ComparisonRejected
  let frame views = bytesAt (memory views) (physical (address (contextSp beforeRetained))+32) 16
  frameBeforeR <- frame (observationBefore observationR)
  frameBeforeO <- frame (observationBefore observationO)
  frameAfterR <- frame (observationCommitted observationR)
  frameAfterO <- frame (observationCommitted observationO)
  require (frameBeforeR == frameBeforeO && frameAfterR == frameAfterO && frameBeforeR == frameAfterR) ComparisonRejected

qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt receipt (Digest receiptHash)) = J.object
  ["schema" J..= (1 :: Int),"qualification" J..= ("bounded-natural-installation" :: String)
  ,"program" J..= show (receiptProgram receipt),"scenario" J..= show (receiptScenario receipt)
  ,"word_stores" J..= length (expectedWrites (receiptScenario receipt))
  ,"receipt_sha256" J..= receiptHash
  ,"limitations" J..= (["Finite natural phase0 interval only; no later consumer lifetime or menu acceptance.","The invalid-marker case mutates marker3 to0 before the watched interval.","Private compiler/adapter stack differs below the original saved frame.","Retained cache behavior is an emulator observation, not silicon coherence ownership.","The optional BSS clear, table producers and later task/service bodies remain retained."] :: [String])]
