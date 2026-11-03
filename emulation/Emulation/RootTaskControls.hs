{-# LANGUAGE OverloadedStrings #-}
module Emulation.RootTaskControls (ControlError(..), refusalControls) where

import qualified A32 as A
import Control.Monad (foldM, unless, zipWithM)
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), (.|.), xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import qualified Data.IntSet as IntSet
import Data.List (findIndex)
import Data.Word (Word32)
import Emulation.EvidenceMutation (Part(..), field)
import qualified Emulation.EvidenceMutation as Mutation
import Emulation.RootTaskWitness
import Emulation.RootTaskWire
import Emulation.TaskMemoryWire
import qualified Emulation.RootTaskPlan as Plan
import qualified Emulation.Sha256 as SHA
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.KernelInstall.Wire as W
import FujiAF.JSON (decodeStrictIntegers)

data Control = WrongSource | WrongArtifact | WrongCandidate | WrongContract | DuplicateReceiptKey | RefusedOutcome
  | WrongObserver | WrongReset | WrongRawSidecar | DisabledWatch | WrongWatchClasses | WrongFetchAttribution | WrongFetchRanges
  | MissingWriterFetch | WrongWriterFetchCore | WrongWriterFetchPc | WrongWriterFetchClock | WrongWriterFetchOrdinal
  | WrongWriterFetchTick | MissingCoreOrdinal | WrongCoreOrdinal | WrongStoppedOrdinal | RecursiveWriterFetch
  | WrongWriterCore | OtherWriterCore | ForeignWriter | CodeWrite | SyncCallbackWrite | WrongKnownPreimage | MalformedStoppedFetch | WrongStoppedClock
  | MissingSample | MissingPool | MissingSyncRecord | WrongAlias | WrongCodeWord | WrappedCounts | WrongEntryContext | WrongEntryStack
  | WrongResourceArgument | MissingResourceCall | WrongResourceIdentity | WrongCheckedStatus | MissingSubmission | WrongDescriptorAddress
  | WrongFlagIdentity | WrongFlagTarget | WrongFlagResult | WrongConstructorTarget | WrongConstructorResult | WrongWrapperRegister | WrongCleanupSelector | WrongCleanupMask
  | WrongCallbackArgument | WrongCallbackResult | WrongCallbackFrame | WrongCallbackRestoration | MissingPostCleanupCallback
  | WrongIrqStatus | WrongIrqFrame | WrongIrqReturn | WrongIrqRestoration deriving (Eq, Show, Enum, Bounded)
data ControlError = BaselineRejected WitnessError | MutationRejected Control | WrongRefusal Control WitnessError (Maybe WitnessError)
  | ProducerSummaryChangedQualification deriving (Eq, Show)
pathAt :: Control -> [Part] -> J.Value -> Either ControlError J.Value
pathAt control = Mutation.pathAt (MutationRejected control)
modifyAt :: Control -> [Part] -> (J.Value -> Either ControlError J.Value) -> J.Value -> Either ControlError J.Value
modifyAt control = Mutation.modifyAt (MutationRejected control)
set :: J.ToJSON value => Control -> [Part] -> value -> J.Value -> Either ControlError J.Value
set control path value = modifyAt control path (const (Right (J.toJSON value)))
parse :: J.FromJSON value => Control -> J.Value -> Either ControlError value
parse control value = case J.fromJSON value of J.Success decoded -> Right decoded; J.Error _ -> Left (MutationRejected control)
encode :: J.Value -> B.ByteString
encode = L.toStrict . J.encode
hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256
zeroDigest :: String
zeroDigest = replicate 64 '0'
word :: W.WordValue -> Word32
word (W.WordValue value) = value
address :: W.Address -> Word32
address (W.Address value) = value
state :: Sample -> W.Context
state = contextState . sampleContext
pc :: Sample -> Word32
pc = Plan.physical . address . W.contextPc . state
register :: Sample -> Int -> Maybe Word32
register sample index = case drop index (W.contextRegisters (state sample)) of value:_ -> Just (word value); [] -> Nothing
currentRoot :: Sample -> Bool
currentRoot sample = any (\view -> viewObject view == Current && viewCore view == S.PrimaryKernel
  && let W.Bytes body = viewBytes view in B.length body == 4 && decodeWord body == 0x40061594) (sampleViews sample)
rootTerminated :: Sample -> Bool
rootTerminated sample = any (\view -> viewObject view == CreationRecord && viewCore view == S.PrimaryKernel
  && let W.Bytes body = viewBytes view in B.length body == 40 && decodeWord (B.drop 8 body) == Plan.rootLow
    && decodeWord (B.drop 36 body) == 0) (sampleViews sample)
decodeWord :: B.ByteString -> Word32
decodeWord body = sum [fromIntegral (B.index body index)*256^index | index <- [0..3 :: Int]]
expected :: Control -> WitnessError
expected control = case control of
  WrongSource -> SourceRejected; WrongArtifact -> ArtifactRejected; WrongCandidate -> CandidateRejected; WrongContract -> ContractRejected
  DuplicateReceiptKey -> ReceiptDecodeRejected; RefusedOutcome -> StatusRejected; WrongObserver -> IdentityRejected
  WrongReset -> ResetRejected; WrongRawSidecar -> RawWatchRejected; DisabledWatch -> WatchRejected; WrongWatchClasses -> WatchRejected
  WrongFetchAttribution -> WatchRejected; WrongFetchRanges -> WatchRejected; MissingWriterFetch -> WriteRejected
  WrongWriterFetchCore -> WriteRejected; WrongWriterFetchPc -> WriteRejected; WrongWriterFetchClock -> WriteRejected
  WrongWriterFetchOrdinal -> WriteRejected; WrongWriterFetchTick -> WriteRejected; MissingCoreOrdinal -> WriteRejected
  WrongCoreOrdinal -> WriteRejected; WrongStoppedOrdinal -> FetchRejected; RecursiveWriterFetch -> RawWatchRejected
  WrongWriterCore -> FetchRejected; OtherWriterCore -> WriteRejected; ForeignWriter -> ForeignWriterRejected; CodeWrite -> WriteRejected
  SyncCallbackWrite -> WriteRejected; WrongKnownPreimage -> PreimageRejected; MalformedStoppedFetch -> FetchRejected; WrongStoppedClock -> FetchRejected
  MissingSample -> SnapshotRejected; MissingPool -> SnapshotRejected; MissingSyncRecord -> SnapshotRejected; WrongAlias -> AliasRejected; WrongCodeWord -> CodeRejected
  WrappedCounts -> ClockRejected; WrongEntryContext -> ContextRejected; WrongEntryStack -> EntryRejected
  WrongResourceArgument -> CallRejected; MissingResourceCall -> CallRejected; WrongResourceIdentity -> ResourceRejected
  WrongCheckedStatus -> CallRejected; MissingSubmission -> SubmissionRejected; WrongDescriptorAddress -> SnapshotRejected
  WrongFlagIdentity -> SubmissionRejected; WrongFlagTarget -> FlagRejected; WrongFlagResult -> FlagRejected
  WrongConstructorTarget -> CreationRejected; WrongConstructorResult -> CreationRejected
  WrongWrapperRegister -> SubmissionRejected; WrongCleanupSelector -> CleanupRejected; WrongCleanupMask -> ContextRejected
  WrongCallbackArgument -> CallbackRejected; WrongCallbackResult -> CallbackRejected; WrongCallbackFrame -> FrameRejected
  WrongCallbackRestoration -> ReturnRejected; MissingPostCleanupCallback -> CompletionRejected
  WrongIrqStatus -> IrqRejected; WrongIrqFrame -> FrameRejected; WrongIrqReturn -> ReturnRejected; WrongIrqRestoration -> ReturnRejected
withHash :: Control -> ObservedFiles -> J.Value -> J.Value -> Either ControlError ObservedFiles
withHash control original receipt watch = do
  changed <- set control [field "memory_watch_sha256"] (hash (encode watch)) receipt
  pure original {observedReceipt=encode changed,observedWatch=encode watch}
hexadecimal :: Word32 -> String
hexadecimal number = "0x" ++ digits number
  where digits value = let alphabet = "0123456789abcdef" in if value < 16 then [alphabet !! fromIntegral value]
          else digits (value `div` 16) ++ [alphabet !! fromIntegral (value `mod` 16)]
changeContext :: Control -> Int -> String -> Word32 -> J.Value -> Either ControlError J.Value
changeContext control index name value receipt = do
  changed <- set control [field "samples",Element index,field "context",field name] value receipt
  if name == "spsr" then pure changed else set control [field "hits",Element index,field name]
    (if name == "cpsr" then J.toJSON value else J.toJSON (hexadecimal value)) changed
changeRegister :: Control -> Int -> Int -> Word32 -> J.Value -> Either ControlError J.Value
changeRegister control index index' value receipt = do
  changed <- set control [field "samples",Element index,field "context",field "registers",Element index'] value receipt
  set control [field "hits",Element index,field "registers",field ("r" ++ show index')] (hexadecimal value) changed
position :: Control -> (Sample -> Bool) -> Receipt -> Either ControlError Int
position control predicate receipt = maybe (Left (MutationRejected control)) Right (findIndex predicate (receiptSamples receipt))
after :: Control -> Receipt -> Int -> (Sample -> Bool) -> Either ControlError Int
after control receipt index predicate = maybe (Left (MutationRejected control)) (Right . (+ (index+1)))
  (findIndex predicate (drop (index+1) (receiptSamples receipt)))
dropSample :: Control -> Int -> J.Value -> Either ControlError J.Value
dropSample control index = dropSamples control (IntSet.singleton index)
dropSamples :: Control -> IntSet.IntSet -> J.Value -> Either ControlError J.Value
dropSamples control excluded receipt = do
  samples <- pathAt control [field "samples"] receipt >>= parse control :: Either ControlError [J.Value]
  hits <- pathAt control [field "hits"] receipt >>= parse control :: Either ControlError [J.Value]
  let remove values = [value | (index,value) <- zip [0..] values,not (IntSet.member index excluded)]
  changedSamples <- zipWithM (\number value -> set control [field "hit"] number value) [0 :: Int ..] (remove samples)
  changed <- set control [field "samples"] changedSamples receipt
  set control [field "hits"] (remove hits) changed

refusalControls :: Role -> SourceFiles -> ObservedFiles -> Either ControlError J.Value
refusalControls role sources original = do
  inputs <- either (Left . BaselineRejected) Right (admitInputs role sources)
  baseline <- either (Left . BaselineRejected) Right (admitReceipt inputs original)
  receiptValue <- either (const (Left (MutationRejected MissingSample))) Right (decodeStrictIntegers (observedReceipt original))
  watchValue <- either (const (Left (MutationRejected WrongRawSidecar))) Right (decodeStrictIntegers (observedWatch original))
  receipt <- parse MissingSample receiptValue
  watch <- parse WrongRawSidecar watchValue
  results <- mapM (run inputs receiptValue watchValue receipt watch) [minBound..maxBound]
  withoutSummaries <- modifyAt MissingSample [field "samples"] (\raw -> do
    values <- parse MissingSample raw :: Either ControlError [J.Value]
    J.toJSON <$> mapM (set MissingSample [field "kernels"] J.Null) values) receiptValue
  independent <- either (Left . BaselineRejected) Right (admitReceipt inputs original {observedReceipt=encode withoutSummaries})
  unless (qualificationValue baseline == qualificationValue independent) (Left ProducerSummaryChangedQualification)
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("serialized-root-task-refusals" :: String)
    ,"baseline_admitted" J..= True,"role" J..= role,"controls" J..= results,"producer_summaries_not_used" J..= True])
  where
    run inputs receiptValue watchValue receipt watch control = do
      (changedSource,changedObserved) <- mutate receiptValue watchValue receipt watch control
      let result = case changedSource of
            Just modified -> admitInputs role modified >> pure ()
            Nothing -> admitReceipt inputs changedObserved >> pure ()
          actual = either Just (const Nothing) result
      unless (actual == Just (expected control)) (Left (WrongRefusal control (expected control) actual))
      pure (J.object ["control" J..= show control,"refusal" J..= show (expected control),"passed" J..= True])
    mutate receiptValue watchValue receipt watch control = do
      let plain value = Right (Nothing,original {observedReceipt=encode value})
          source value = Right (Just value,original)
          changedWatch value = do observed <- withHash control original receiptValue value; pure (Nothing,observed)
          samples = receiptSamples receipt
          sampleAt index = samples !! index
          primary sample = contextCore (sampleContext sample) == S.PrimaryKernel
          at location = position control (\sample -> primary sample && currentRoot sample && pc sample == location) receipt
          firstEntry = position control (\sample -> primary sample && pc sample == Plan.rootLow) receipt
          callTo target sample = primary sample && Plan.rootLow <= pc sample && pc sample < Plan.rootHigh
            && case A.decode (pc sample) (word (sampleCodeWord sample)) of A.Branch _ True destination -> destination == target; _ -> False
          resourceCall = position control (\sample -> callTo 0x7247ac sample && register sample 0 == Just 42) receipt
          submission = position control (\sample -> primary sample && currentRoot sample && sampleBoundary sample == TaskSubmit) receipt
          callback location = position control (\sample -> rootTerminated sample && pc sample == location) receipt
          irq = position control (\sample -> primary sample && currentRoot sample && sampleBoundary sample == IrqEntry
            && Plan.physical (address (W.contextLr (state sample)))-4 `notElem` [0x725dd8,0x724924,0x725f08,0x725fa8]) receipt
          irqAfter boundary = do index <- irq; after control receipt index (\sample -> primary sample && currentRoot sample && sampleBoundary sample == boundary)
          firstWrite = maybe (Left (MutationRejected control)) Right (findIndex ((==W.CpuWrite) . W.eventSource . eventRecord) (captureEvents watch))
          stoppedRow = do
            index <- position control (\sample -> sampleCursor sample > 0 && case drop (sampleCursor sample-1) (captureEvents watch) of
              value:_ -> W.eventSource (eventRecord value) == W.CpuFetch && W.eventPc (eventRecord value) == Just (W.contextPc (state sample)); [] -> False) receipt
            pure (sampleCursor (sampleAt index)-1)
          alterRegister selection number transform = do
            index <- selection
            old <- maybe (Left (MutationRejected control)) Right (register (sampleAt index) number)
            changeRegister control index number (transform old) receiptValue >>= plain
          alterContext selection name transform = do
            index <- selection
            old <- pathAt control [field "samples",Element index,field "context",field name] receiptValue >>= parse control
            changeContext control index name (transform old) receiptValue >>= plain
          increment path = do
            old <- pathAt control path watchValue >>= parse control :: Either ControlError Integer
            set control path (old+1) watchValue >>= changedWatch
      case control of
        WrongSource -> source sources {sourceDat=B.snoc (sourceDat sources) 0}
        WrongArtifact -> source sources {sourceLinked=B.replicate (B.length (sourceLinked sources)) 0}
        WrongCandidate -> source sources {runDecoded=B.snoc (runDecoded sources) 0}
        WrongContract -> do
          contract <- either (const (Left (MutationRejected control))) Right (decodeStrictIntegers (sourceContract sources))
          changed <- set control [field "candidate_image4_sha256"] zeroDigest contract
          source sources {sourceContract=encode changed}
        DuplicateReceiptKey -> Right (Nothing,original {observedReceipt=C.pack "{\"schema\":1," <> B.drop 1 (observedReceipt original)})
        RefusedOutcome -> set control [field "outcome"] ("refused" :: String) receiptValue >>= plain
        WrongObserver -> set control [field "identity",field "observer"] zeroDigest receiptValue >>= plain
        WrongReset -> Right (Nothing,original {observedReset=B.snoc (observedReset original) 0})
        WrongRawSidecar -> Right (Nothing,original {observedWatch=B.snoc (observedWatch original) 32})
        DisabledWatch -> set control [field "enabled"] False watchValue >>= changedWatch
        WrongWatchClasses -> set control [field "classes"] ([] :: [J.Value]) watchValue >>= changedWatch
        WrongFetchAttribution -> set control [field "fetch_attribution",field "sources"] ([] :: [J.Value]) watchValue >>= changedWatch
        WrongFetchRanges -> set control [field "fetch_attribution",field "ranges"] ([] :: [J.Value]) watchValue >>= changedWatch
        MissingWriterFetch -> do index <- firstWrite; set control [field "records",Element index,field "latest_fetch"] J.Null watchValue >>= changedWatch
        WrongWriterFetchCore -> do
          index <- firstWrite
          let path = [field "records",Element index,field "latest_fetch",field "core"]
          old <- pathAt control path watchValue >>= parse control :: Either ControlError Int
          set control path (1-old) watchValue >>= changedWatch
        WrongWriterFetchPc -> do
          index <- firstWrite
          let path = [field "records",Element index,field "latest_fetch"]
          old <- pathAt control (path++[field "pc"]) watchValue >>= parse control :: Either ControlError Word32
          changed <- foldM (\value name -> set control (path++[field name]) (old+4) value) watchValue ["pc","address","physical"]
          changedWatch changed
        WrongWriterFetchClock -> do index <- firstWrite; increment [field "records",Element index,field "latest_fetch",field "instructions"]
        WrongWriterFetchOrdinal -> do index <- firstWrite; increment [field "records",Element index,field "latest_fetch",field "core_instructions"]
        WrongWriterFetchTick -> do index <- firstWrite; increment [field "records",Element index,field "latest_fetch",field "tick"]
        MissingCoreOrdinal -> do index <- firstWrite; set control [field "records",Element index,field "core_instructions"] J.Null watchValue >>= changedWatch
        WrongCoreOrdinal -> do index <- firstWrite; increment [field "records",Element index,field "core_instructions"]
        WrongStoppedOrdinal -> do index <- stoppedRow; increment [field "records",Element index,field "core_instructions"]
        RecursiveWriterFetch -> do
          index <- firstWrite
          changed <- modifyAt control [field "records",Element index,field "latest_fetch"] (\value -> case value of
            J.Object object -> pure (J.Object (KM.insert "latest_fetch" J.Null object)); _ -> Left (MutationRejected control)) watchValue
          changedWatch changed
        WrongWriterCore -> do index <- firstWrite; set control [field "records",Element index,field "core"] J.Null watchValue >>= changedWatch
        OtherWriterCore -> do
          index <- firstWrite
          old <- pathAt control [field "records",Element index,field "core"] watchValue >>= parse control :: Either ControlError Int
          set control [field "records",Element index,field "core"] (1-old) watchValue >>= changedWatch
        ForeignWriter -> do index <- firstWrite; set control [field "records",Element index,field "source"] ("host-write" :: String) watchValue >>= changedWatch
        CodeWrite -> do
          index <- firstWrite
          changed <- set control [field "records",Element index,field "physical"] Plan.rootLow watchValue
          set control [field "records",Element index,field "address"] Plan.rootLow changed >>= changedWatch
        SyncCallbackWrite -> do
          index <- firstWrite
          changed <- set control [field "records",Element index,field "physical"] (0x545d8 :: Word32) watchValue
          set control [field "records",Element index,field "address"] (0x400545d8 :: Word32) changed >>= changedWatch
        WrongKnownPreimage -> do
          index <- firstWrite
          old <- pathAt control [field "records",Element index,field "before",Element 0] watchValue >>= parse control :: Either ControlError Word32
          set control [field "records",Element index,field "before",Element 0] (old `xor` 1) watchValue >>= changedWatch
        MalformedStoppedFetch -> do index <- stoppedRow; set control [field "records",Element index,field "size"] (2 :: Int) watchValue >>= changedWatch
        WrongStoppedClock -> do index <- stoppedRow; increment [field "records",Element index,field "instructions"]
        MissingSample -> set control [field "samples"] ([] :: [J.Value]) receiptValue >>= plain
        MissingPool -> do
          initial <- maybe (Left (MutationRejected control)) Right (case samples of value:_ -> Just value; [] -> Nothing)
          index <- maybe (Left (MutationRejected control)) Right (findIndex (\view -> viewObject view == Pool
            && address (viewPhysical view) == Plan.stackLow) (sampleViews initial))
          modifyAt control [field "samples",Element 0,field "views"] (\raw -> do
            values <- parse control raw :: Either ControlError [J.Value]
            pure (J.toJSON (take index values++drop (index+1) values))) receiptValue >>= plain
        MissingSyncRecord -> do
          initial <- maybe (Left (MutationRejected control)) Right (case samples of value:_ -> Just value; [] -> Nothing)
          index <- maybe (Left (MutationRejected control)) Right (findIndex (\view ->
            viewObject view == ResourceState && address (viewPhysical view) == 0x545cc) (sampleViews initial))
          modifyAt control [field "samples",Element 0,field "views"] (\raw -> do
            values <- parse control raw :: Either ControlError [J.Value]
            pure (J.toJSON (take index values++drop (index+1) values))) receiptValue >>= plain
        WrongAlias -> set control [field "samples",Element 0,field "views",Element 0,field "aliases",Element 0,Element 0] zeroDigest receiptValue >>= plain
        WrongCodeWord -> set control [field "samples",Element 0,field "code_word"] (0 :: Word32) receiptValue >>= plain
        WrappedCounts -> do
          old <- pathAt control [field "samples",Element 0,field "context",field "counts",Element 0] receiptValue >>= parse control :: Either ControlError Integer
          set control [field "samples",Element 0,field "context",field "counts",Element 0] (old+4294967296) receiptValue >>= plain
        WrongEntryContext -> alterContext firstEntry "cpsr" (`xor` 0x20)
        WrongEntryStack -> alterContext firstEntry "sp" (+8)
        WrongResourceArgument -> alterRegister resourceCall 0 (+1)
        MissingResourceCall -> do index <- resourceCall; dropSample control index receiptValue >>= plain
        WrongResourceIdentity -> alterRegister (position control (\sample -> primary sample && currentRoot sample && pc sample == 0x724924 && register sample 0 == Just 0x1002a) receipt) 0 (`xor` 1)
        WrongCheckedStatus -> do
          call <- position control (callTo 0x6a9354) receipt
          returned <- after control receipt call (\sample -> primary sample && pc sample == pc (sampleAt call)+4)
          changeRegister control returned 0 1 receiptValue >>= plain
        MissingSubmission -> do index <- submission; dropSample control index receiptValue >>= plain
        WrongDescriptorAddress -> alterRegister submission 0 (+4)
        WrongFlagIdentity -> alterRegister (at 0x725f08) 0 (`xor` 1)
        WrongFlagTarget -> do
          constructor <- at 0x725fa8
          destination <- maybe (Left (MutationRejected control)) Right (register (sampleAt constructor) 3)
          alterRegister (at 0x725f08) 3 (const destination)
        WrongFlagResult -> alterRegister (at 0x725f0c) 0 (const 0xffffffff)
        WrongConstructorTarget -> do
          flag <- at 0x725f08
          destination <- maybe (Left (MutationRejected control)) Right (register (sampleAt flag) 3)
          alterRegister (at 0x725fa8) 3 (const destination)
        WrongConstructorResult -> alterRegister (at 0x725fac) 0 (const 1)
        WrongWrapperRegister -> do
          entered <- submission
          returned <- after control receipt entered (\sample -> primary sample && pc sample == Plan.physical (address (W.contextLr (state (sampleAt entered)))))
          alterRegister (Right returned) 4 (`xor` 1)
        WrongCleanupSelector -> alterRegister (at 0x726150) 0 (const 1)
        WrongCleanupMask -> alterContext (at 0x7261a8) "cpsr" (.&. 0xffffff7f)
        WrongCallbackArgument -> alterRegister (callback 0x69cc2c) 0 (+1)
        WrongCallbackResult -> alterRegister (callback 0x69cc30) 0 (const 0xffffffc0)
        WrongCallbackFrame -> alterContext (callback 0x69cc1c) "sp" (+4)
        WrongCallbackRestoration -> do
          entered <- callback 0x69cc18
          ending <- after control receipt entered (\sample -> pc sample == 0x69cc68 && contextCore (sampleContext sample) == contextCore (sampleContext (sampleAt entered)))
          returned <- after control receipt ending (\sample -> pc sample == Plan.physical (address (W.contextLr (state (sampleAt entered))))
            && contextCore (sampleContext sample) == contextCore (sampleContext (sampleAt entered)))
          alterRegister (Right returned) 4 (`xor` 1)
        MissingPostCleanupCallback -> do
          let excluded = IntSet.fromList [index | (index,sample) <- zip [0..] samples,rootTerminated sample && pc sample == 0x69cc18]
          unless (not (IntSet.null excluded)) (Left (MutationRejected control))
          dropSamples control excluded receiptValue >>= plain
        WrongIrqStatus -> alterContext irq "spsr" (\value -> value .&. 0xffffffe0 .|. 0x12)
        WrongIrqFrame -> alterContext (irqAfter IrqFrame) "sp" (+4)
        WrongIrqReturn -> alterContext (irqAfter IrqReturn) "spsr" (`xor` 0x20000000)
        WrongIrqRestoration -> do
          entered <- irq
          returned <- irqAfter IrqReturn
          resumed <- after control receipt returned (\sample -> primary sample && pc sample == Plan.physical (address (W.contextLr (state (sampleAt entered))))-4)
          alterRegister (Right resumed) 0 (`xor` 1)
