{-# LANGUAGE OverloadedStrings #-}
module Emulation.StartupTaskControls (ControlError(..), refusalControls, refusalControlsFor) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Bits ((.|.), xor, shiftR)
import Data.List (findIndex)
import qualified Data.Vector as V
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.Sha256 as SHA
import Emulation.StartupTaskWitness
import qualified Emulation.StartupTaskWire as Wire
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Wire as W

data Control = WrongSource | DuplicateReceiptKey | OverflowingHitAddress | RefusedOutcome | WrongProducer | WrongReset
  | WrongRawSidecar | WrongEmbeddedWatchHash | DisabledWatch | MissingSnapshot | WrongAlias | WrongPoolAlias
  | WrongDerivedTask | WrongInitialTask | WrongDescriptorClass | WrongActivationContext | WrongCodeWord
  | MissingWriterCore | ForeignWriter | MissingExecutedFetch | DuplicateExecutedFetch | MissingStoppedFetch
  | WrongStoppedClock | WrongWatchClass | WrongAuxiliaryFlag | ExtraRunnableBit | WrongFirstNode
  | WrongPrologueRegister | WrongPrologueStack | WrongPrologueStore
  | CodeWrite | WrongKnownPreimage | WrongReturnStatus | MissingFirstTransfer
  | MissingStoppedPhysical | MalformedStoppedAddress | MalformedStoppedSize | WrappedCounts
  | WrongReadValue | WrongReadPreimage | WrongReadClock | MissingReadPhysical | MalformedReadSize
  | RetiredCriticalRead | WrongJoinedSource | MissingCriticalCounter | WrongCounterAlias
  | MissingReadTick | RegressingReadTick
  deriving (Bounded, Enum, Eq, Ord, Show)
data ControlError = BaselineRejected WitnessError | MutationRejected Control
  | WrongRefusal Control WitnessError (Maybe WitnessError) deriving (Eq, Show)
data Part = Field K.Key | Element Int

data Effect = Replace Word32 | AddBits Word32
applyEffect :: Effect -> Word32 -> Word32
applyEffect (Replace word) _ = word
applyEffect (AddBits mask) word = word .|. mask
field :: String -> Part
field = Field . K.fromString
pathAt :: Control -> [Part] -> J.Value -> Either ControlError J.Value
pathAt _ [] value = Right value
pathAt control (Field key:rest) (J.Object object) = maybe (Left (MutationRejected control)) (pathAt control rest) (KM.lookup key object)
pathAt control (Element index:rest) (J.Array values) = maybe (Left (MutationRejected control)) (pathAt control rest) (values V.!? index)
pathAt control _ _ = Left (MutationRejected control)
modifyAt :: Control -> [Part] -> (J.Value -> Either ControlError J.Value) -> J.Value -> Either ControlError J.Value
modifyAt _ [] change value = change value
modifyAt control (Field key:rest) change (J.Object object) = do
  old <- maybe (Left (MutationRejected control)) Right (KM.lookup key object)
  new <- modifyAt control rest change old
  pure (J.Object (KM.insert key new object))
modifyAt control (Element index:rest) change (J.Array values) = do
  old <- maybe (Left (MutationRejected control)) Right (values V.!? index)
  new <- modifyAt control rest change old
  pure (J.Array (values V.// [(index,new)]))
modifyAt control _ _ _ = Left (MutationRejected control)
set :: J.ToJSON a => Control -> [Part] -> a -> J.Value -> Either ControlError J.Value
set control path value = modifyAt control path (const (Right (J.toJSON value)))
parse :: J.FromJSON a => Control -> J.Value -> Either ControlError a
parse control value = case J.fromJSON value of J.Success decoded -> Right decoded; J.Error _ -> Left (MutationRejected control)
numberAt :: Control -> [Part] -> J.Value -> Either ControlError Word64
numberAt control path value = pathAt control path value >>= parse control
arrayAt :: Control -> [Part] -> J.Value -> Either ControlError [J.Value]
arrayAt control path value = pathAt control path value >>= parse control
hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256
encode :: J.Value -> B.ByteString
encode = L.toStrict . J.encode

expected :: Control -> WitnessError
expected control = case control of
  WrongSource -> SourceRejected; DuplicateReceiptKey -> ReceiptDecodeRejected; OverflowingHitAddress -> ReceiptDecodeRejected
  RefusedOutcome -> StatusRejected; WrongProducer -> IdentityRejected; WrongReset -> ResetRejected
  WrongRawSidecar -> RawWatchRejected; WrongEmbeddedWatchHash -> RawWatchRejected; DisabledWatch -> StatusRejected
  MissingSnapshot -> SnapshotRejected; WrongAlias -> AliasRejected; WrongPoolAlias -> AliasRejected
  WrongDerivedTask -> TaskRejected; WrongInitialTask -> KernelRejected; WrongDescriptorClass -> SubmissionRejected
  WrongActivationContext -> SubmissionRejected; WrongCodeWord -> CodeRejected; MissingWriterCore -> FetchRejected
  ForeignWriter -> ForeignWriterRejected; MissingExecutedFetch -> ClockRejected; DuplicateExecutedFetch -> ClockRejected
  MissingStoppedFetch -> FetchRejected; WrongStoppedClock -> FetchRejected; WrongWatchClass -> WatchRejected
  WrongAuxiliaryFlag -> TaskRejected; ExtraRunnableBit -> RunnableRejected; WrongFirstNode -> FirstTaskRejected
  WrongPrologueRegister -> PrologueRejected; WrongPrologueStack -> PrologueRejected; WrongPrologueStore -> PrologueRejected
  CodeWrite -> WriteRejected; WrongKnownPreimage -> PreimageRejected; WrongReturnStatus -> SubmissionRejected
  MissingFirstTransfer -> FirstTaskRejected
  MissingStoppedPhysical -> FetchRejected; MalformedStoppedAddress -> FetchRejected; MalformedStoppedSize -> FetchRejected
  WrappedCounts -> ClockRejected
  WrongReadValue -> ReadRejected; WrongReadPreimage -> ReadRejected; WrongReadClock -> ClockRejected
  MissingReadPhysical -> ReadRejected; MalformedReadSize -> ReadRejected; RetiredCriticalRead -> ReadRejected
  WrongJoinedSource -> IdentityRejected; MissingCriticalCounter -> SnapshotRejected; WrongCounterAlias -> AliasRejected
  MissingReadTick -> ClockRejected; RegressingReadTick -> ClockRejected

withHash :: Control -> ObservedFiles -> J.Value -> J.Value -> Either ControlError ObservedFiles
withHash control original receipt watch = do
  updated <- set control [field "memory_watch_sha256"] (hash (encode watch)) receipt
  pure original {observedReceipt=encode updated,observedWatch=encode watch}
findRow :: Control -> (W.MemoryEvent -> Bool) -> J.Value -> Either ControlError Int
findRow control predicate watch = do
  raw <- arrayAt control [field "records"] watch
  rows <- mapM (parse control) raw
  maybe (Left (MutationRejected control)) Right (findIndex predicate rows)
writeRow :: W.MemoryEvent -> Bool
writeRow row = W.eventSource row == W.CpuWrite && W.eventSize row == 4
fetchRow :: W.MemoryEvent -> Bool
fetchRow row = W.eventSource row == W.CpuFetch
findSample :: Control -> String -> J.Value -> Either ControlError Int
findSample control label receipt = do
  samples <- arrayAt control [field "samples"] receipt
  labels <- mapM (\sample -> pathAt control [field "boundary"] sample >>= parse control) samples
  maybe (Left (MutationRejected control)) Right (findIndex (==label) (labels :: [String]))
modifyContextWord :: Control -> Int -> Int -> Word32 -> J.Value -> Either ControlError J.Value
modifyContextWord control sampleIndex register word receipt = do
  hit <- numberAt control [field "samples",Element sampleIndex,field "hit"] receipt
  changed <- set control [field "samples",Element sampleIndex,field "context",field "registers",Element register] word receipt
  set control [field "hits",Element (fromIntegral hit),field "registers",field ("r" ++ show register)] ("0x" ++ hexadecimal word) changed
  where hexadecimal number = let digits = "0123456789abcdef" in if number < 16 then [digits !! fromIntegral number] else hexadecimal (number `div` 16) ++ [digits !! fromIntegral (number `mod` 16)]

reAlias :: Control -> J.Value -> Either ControlError J.Value
reAlias control view = do
  raw <- pathAt control [field "bytes"] view >>= parse control :: Either ControlError W.Bytes
  let W.Bytes contents = raw
  set control [field "aliases"] (replicate 2 (replicate 2 (hash contents))) view
changeViewWord :: Control -> Word32 -> Effect -> J.Value -> Either ControlError J.Value
changeViewWord control target effect view = do
  start <- numberAt control [field "physical"] view
  W.Bytes contents <- pathAt control [field "bytes"] view >>= parse control
  if fromIntegral target < start || fromIntegral target+4 > start+fromIntegral (B.length contents) then pure view else do
    let offset = fromIntegral (fromIntegral target-start :: Word64)
        word = applyEffect effect (U.word32At contents offset)
        replacement = B.pack [fromIntegral (word `shiftR` amount) | amount <- [0,8,16,24]]
        updated = B.take offset contents <> replacement <> B.drop (offset+4) contents
    changed <- set control [field "bytes"] (B.unpack updated) view
    reAlias control changed
changeViews :: Control -> Word32 -> Effect -> J.Value -> Either ControlError J.Value
changeViews control target effect sample = do
  changed <- modifyAt control [field "local"] (changeArray (changeViewWord control target effect)) sample
  modifyAt control [field "kernels"] (changeArray (modifyAt control [field "views"] (changeArray (changeViewWord control target effect)))) changed
  where changeArray function (J.Array values) = J.Array . V.fromList <$> mapM function (V.toList values)
        changeArray _ _ = Left (MutationRejected control)

recordedEffect :: Control -> Word32 -> Effect -> J.Value -> J.Value -> Either ControlError (J.Value,J.Value)
recordedEffect control target effect = recordedEffectFor control target effect (const True)

recordedEffectFor :: Control -> Word32 -> Effect -> (W.MemoryEvent -> Bool) -> J.Value -> J.Value -> Either ControlError (J.Value,J.Value)
recordedEffectFor control target effect selected receipt watch = do
  start <- findRow control (\row -> writeRow row && W.eventPhysical row == Just (fromIntegral target) && selected row) watch
  rows <- arrayAt control [field "records"] watch
  changedRows <- mapM (changeRow start) (zip [0..] rows)
  changedWatch <- set control [field "records"] changedRows watch
  samples <- arrayAt control [field "samples"] receipt
  changedSamples <- mapM (changeSample start) samples
  changedReceipt <- set control [field "samples"] changedSamples receipt
  pure (changedReceipt,changedWatch)
  where
    changeRow start (index,row) = do
      decoded <- parse control row
      if index < start || not (writeRow decoded) || W.eventPhysical decoded /= Just (fromIntegral target) then pure row else do
        original <- maybe (Left (MutationRejected control)) Right (W.eventValue decoded)
        changed <- set control [field "value"] (applyEffect effect (fromIntegral original)) row
        if index == start then pure changed else case W.eventBefore decoded of
          Just (W.Bytes before) | B.length before == 4 -> set control [field "before"]
            ([fromIntegral (applyEffect effect (U.word32At before 0) `shiftR` amount) | amount <- [0,8,16,24]] :: [Word8]) changed
          _ -> Left (MutationRejected control)
    changeSample start sample = do
      position <- numberAt control [field "watch_cursor"] sample
      if position > fromIntegral start then changeViews control target effect sample else pure sample

journalEdit :: Control -> Int -> (J.Value -> [J.Value]) -> Int -> J.Value -> J.Value -> Either ControlError (J.Value,J.Value)
journalEdit control position replacement delta receipt watch = do
  rows <- arrayAt control [field "records"] watch
  row <- maybe (Left (MutationRejected control)) Right (case drop position rows of item:_ -> Just item; [] -> Nothing)
  changedWatch <- set control [field "records"] (take position rows ++ replacement row ++ drop (position+1) rows) watch
  samples <- arrayAt control [field "samples"] receipt
  changedSamples <- mapM (\sample -> do
    old <- numberAt control [field "watch_cursor"] sample
    if old > fromIntegral position then set control [field "watch_cursor"] (fromIntegral old+delta :: Int) sample else pure sample) samples
  changedReceipt <- set control [field "samples"] changedSamples receipt
  pure (changedReceipt,changedWatch)

wrappedCounts :: Control -> J.Value -> Either ControlError J.Value
wrappedCounts control receipt = do
  samples <- arrayAt control [field "samples"] receipt
  changedSamples <- mapM (modifyAt control [field "context",field "counts"] shiftPair) samples
  withSamples <- set control [field "samples"] changedSamples receipt
  firstTasks <- arrayAt control [field "first_tasks"] withSamples
  changedTasks <- mapM (modifyAt control [field "entry_counts"] shiftPair) firstTasks
  withTasks <- set control [field "first_tasks"] changedTasks withSamples
  modifyAt control [field "startup_end_counts"] shiftPair withTasks
  where
    shiftPair raw = do
      pair <- parse control raw
      case pair of
        [W.Count primary,W.Count secondary] -> pure (J.toJSON
          [fromInteger ((toInteger primary-4000000) `mod` 0x10000000000000000),secondary+4000000])
        _ -> Left (MutationRejected control)

mutate :: Control -> SourceFiles -> ObservedFiles -> Either ControlError (SourceFiles,ObservedFiles)
mutate control sources observed = do
  receipt <- either (const (Left (MutationRejected control))) Right (decodeStrictIntegers (observedReceipt observed))
  watch <- either (const (Left (MutationRejected control))) Right (decodeStrictIntegers (observedWatch observed))
  let receiptOnly change = do changed <- change receipt; pure (sources,observed {observedReceipt=encode changed})
      journal change = do (changedReceipt,changedWatch) <- change receipt watch; updated <- withHash control observed changedReceipt changedWatch; pure (sources,updated)
      stoppedField name value = journal (\raw watches -> do
        rows <- arrayAt control [field "records"] watches
        changed <- set control [field "records",Element (length rows-1),field name] value watches
        pure (raw,changed))
      readField name change = journal (\raw watches -> do
        position <- findRow control (\row -> W.eventSource row == W.CpuRead) watches
        changed <- modifyAt control [field "records",Element position,field name] change watches
        pure (raw,changed))
      counterView raw sample = do
        views <- arrayAt control [field "samples",Element sample,field "local"] raw
        decoded <- mapM (parse control) views
        maybe (Left (MutationRejected control)) Right (findIndex ((==Wire.CriticalCounter) . Wire.viewObject) decoded)
  case control of
    WrongSource -> pure (sources {sourceImage=B.cons (B.head (sourceImage sources) `xor` 1) (B.tail (sourceImage sources))},observed)
    DuplicateReceiptKey -> pure (sources,observed {observedReceipt=C.pack "{\"schema\":1," <> B.drop 1 (observedReceipt observed)})
    OverflowingHitAddress -> receiptOnly (set control [field "hits",Element 0,field "address"] ("0x100725dd8" :: String))
    RefusedOutcome -> receiptOnly (set control [field "outcome"] ("refused" :: String))
    WrongProducer -> receiptOnly (set control [field "identity",field "observer"] (replicate 64 '0'))
    WrongReset -> pure (sources,observed {observedReset=observedReset observed <> B.singleton 0})
    WrongRawSidecar -> pure (sources,observed {observedWatch=observedWatch observed <> C.pack " "})
    WrongEmbeddedWatchHash -> receiptOnly (set control [field "memory_watch_sha256"] (replicate 64 '0'))
    DisabledWatch -> receiptOnly (set control [field "watch",field "Ok",field "enabled"] False)
    MissingSnapshot -> receiptOnly (modifyAt control [field "samples"] (\value -> do values <- parse control value; pure (J.toJSON (drop 1 (values :: [J.Value])))))
    WrongAlias -> receiptOnly (set control [field "samples",Element 0,field "kernels",Element 0,field "views",Element 0,field "aliases",Element 0,Element 0] (replicate 64 '0'))
    WrongPoolAlias -> receiptOnly (\raw -> do
      sample <- findSample control "auxiliary-return" raw
      views <- arrayAt control [field "samples",Element sample,field "local"] raw
      pools <- mapM (\view -> do
        decoded <- parse control view :: Either ControlError Wire.View
        let W.Bytes contents = Wire.viewBytes decoded
        pure (Wire.viewObject decoded == Wire.Stack && B.length contents == 1024)) views
      position <- maybe (Left (MutationRejected control)) Right (findIndex id pools)
      physical <- numberAt control [field "samples",Element sample,field "local",Element position,field "physical"] raw
      set control [field "samples",Element sample,field "local",Element position,field "physical"] (physical .|. 0x40000000) raw)
    WrongDerivedTask -> receiptOnly (set control [field "samples",Element 0,field "kernels",Element 0,field "tasks",Element 0,field "state"] (1 :: Int))
    WrongInitialTask -> receiptOnly (\raw -> do
      changedViews <- modifyAt control [field "samples",Element 0] (changeViews control 0x5beb8 (Replace 14)) raw
      set control [field "samples",Element 0,field "kernels",Element 0,field "tasks",Element 0,field "priority"] (14 :: Word32) changedViews)
    WrongDescriptorClass -> receiptOnly (\raw -> do sample <- findSample control "creation-call" raw; modifyContextWord control sample 0 0x10067 raw)
    WrongActivationContext -> receiptOnly (\raw -> do
      sample <- findSample control "activation-call" raw
      hit <- numberAt control [field "samples",Element sample,field "hit"] raw
      cpsr <- numberAt control [field "samples",Element sample,field "context",field "cpsr"] raw
      changed <- set control [field "samples",Element sample,field "context",field "cpsr"] (cpsr `xor` 0x80) raw
      set control [field "hits",Element (fromIntegral hit),field "cpsr"] (cpsr `xor` 0x80) changed)
    WrongCodeWord -> receiptOnly (\raw -> do word <- numberAt control [field "samples",Element 0,field "code_word"] raw; set control [field "samples",Element 0,field "code_word"] (word `xor` 1) raw)
    CodeWrite -> journal (\raw watches -> do
      position <- findRow control writeRow watches
      pc <- numberAt control [field "records",Element position,field "pc"] watches
      changed <- set control [field "records",Element position,field "address"] pc watches
        >>= set control [field "records",Element position,field "physical"] pc
      pure (raw,changed))
    WrongKnownPreimage -> journal (\raw watches -> do
      position <- findRow control (\row -> writeRow row && maybe False (\target -> target >= 0xebe6c && target < 0xed12c) (W.eventPhysical row)) watches
      before <- arrayAt control [field "records",Element position,field "before"] watches
      first <- maybe (Left (MutationRejected control)) Right (case before of item:_ -> Just item; [] -> Nothing)
      byte <- parse control first :: Either ControlError Word8
      changed <- set control [field "records",Element position,field "before"] (J.toJSON (byte `xor` 1):drop 1 before) watches
      pure (raw,changed))
    WrongReturnStatus -> receiptOnly (\raw -> do
      sample <- findSample control "wrapper-return" raw
      changed <- modifyContextWord control sample 0 0 raw
      set control [field "submissions",Element 0,field "status"] (0 :: Word32) changed)
    MissingFirstTransfer -> receiptOnly (set control [field "first_tasks",Element 0,field "transfer_hit"] J.Null)
    MissingWriterCore -> journal (\raw watches -> do position <- findRow control writeRow watches; changed <- set control [field "records",Element position,field "core"] J.Null watches; pure (raw,changed))
    ForeignWriter -> journal (\raw watches -> do position <- findRow control writeRow watches; changed <- set control [field "records",Element position,field "source"] ("host-write" :: String) watches; pure (raw,changed))
    MissingExecutedFetch -> journal (\raw watches -> do position <- findRow control fetchRow watches; journalEdit control position (const []) (-1) raw watches)
    DuplicateExecutedFetch -> journal (\raw watches -> do position <- findRow control fetchRow watches; journalEdit control position (\row -> [row,row]) 1 raw watches)
    MissingStoppedFetch -> journal (\raw watches -> do rows <- arrayAt control [field "records"] watches; journalEdit control (length rows-1) (const []) (-1) raw watches)
    WrongStoppedClock -> journal (\raw watches -> do
      rows <- arrayAt control [field "records"] watches
      let position = length rows-1
      old <- numberAt control [field "records",Element position,field "instructions"] watches
      changed <- set control [field "records",Element position,field "instructions"] (old+1) watches
      pure (raw,changed))
    MissingStoppedPhysical -> stoppedField "physical" J.Null
    MalformedStoppedAddress -> stoppedField "address" (J.toJSON (0 :: Word32))
    MalformedStoppedSize -> stoppedField "size" (J.toJSON (8 :: Int))
    WrappedCounts -> receiptOnly (wrappedCounts control)
    WrongReadValue -> readField "value" (\raw -> do value <- parse control raw :: Either ControlError Word64; pure (J.toJSON (value `xor` 1)))
    WrongReadPreimage -> readField "before" (\raw -> do
      values <- parse control raw :: Either ControlError [Word8]
      case values of first:rest -> pure (J.toJSON ((first `xor` 1):rest)); [] -> Left (MutationRejected control))
    WrongReadClock -> readField "instructions" (\raw -> do value <- parse control raw :: Either ControlError Word64; pure (J.toJSON (value+1)))
    MissingReadPhysical -> readField "physical" (const (Right J.Null))
    MalformedReadSize -> readField "size" (const (Right (J.toJSON (0 :: Int))))
    MissingReadTick -> readField "tick" (const (Right J.Null))
    RegressingReadTick -> journal (\raw watches -> do
      let positiveTick row = maybe False (>W.Count 0) (W.eventTick row)
      position <- findRow control (\row -> W.eventSource row == W.CpuRead && positiveTick row) watches
      rows <- arrayAt control [field "records"] watches
      prior <- mapM (parse control) (take position rows)
      unless (any (\row -> W.eventSource row `elem` [W.CpuFetch,W.CpuRead,W.CpuWrite] && positiveTick row) prior)
        (Left (MutationRejected control))
      changed <- set control [field "records",Element position,field "tick"] (0 :: Word64) watches
      pure (raw,changed))
    RetiredCriticalRead -> journal (\raw watches -> do
      position <- findRow control (\row -> W.eventSource row == W.CpuRead) watches
      changed <- set control [field "records",Element position,field "address"] (0x7235dc :: Word32) watches
        >>= set control [field "records",Element position,field "physical"] (0x7235dc :: Word32)
      pure (raw,changed))
    WrongJoinedSource -> receiptOnly (\raw -> do
      identities <- arrayAt control [field "identity",field "sources"] raw
      names <- mapM (\identity -> pathAt control [field "name"] identity >>= parse control) identities
      position <- maybe (Left (MutationRejected control)) Right (findIndex (==("record2/critical-pair.bin" :: String)) names)
      set control [field "identity",field "sources",Element position,field "sha256"] (replicate 64 '0') raw)
    MissingCriticalCounter -> receiptOnly (\raw -> do
      samples <- arrayAt control [field "samples"] raw
      let sample = length samples-1
      position <- counterView raw sample
      modifyAt control [field "samples",Element sample,field "local"] (\value -> do
        views <- parse control value :: Either ControlError [J.Value]
        pure (J.toJSON (take position views ++ drop (position+1) views))) raw)
    WrongCounterAlias -> receiptOnly (\raw -> do
      position <- counterView raw 0
      set control [field "samples",Element 0,field "local",Element position,field "aliases",Element 0,Element 0] (replicate 64 '0') raw)
    WrongWatchClass -> journal (\raw watches -> do changed <- modifyAt control [field "classes"] (\value -> do values <- parse control value; pure (J.toJSON (drop 1 (values :: [J.Value])))) watches; pure (raw,changed))
    WrongAuxiliaryFlag -> journal (recordedEffect control 0xed600 (Replace 0))
    ExtraRunnableBit -> journal (recordedEffect control 0xe471c (AddBits 32))
    WrongFirstNode -> receiptOnly (set control [field "first_tasks",Element 0,field "node"] (0x400f1380 :: Word32))
    WrongPrologueRegister -> receiptOnly (\raw -> do sample <- findSample control "task-entry" raw; old <- numberAt control [field "samples",Element sample,field "context",field "registers",Element 4] raw; modifyContextWord control sample 4 (fromIntegral old `xor` 1) raw)
    WrongPrologueStack -> receiptOnly (\raw -> do
      sample <- findSample control "entered-instruction" raw
      hit <- numberAt control [field "samples",Element sample,field "hit"] raw
      sp <- numberAt control [field "samples",Element sample,field "context",field "sp"] raw
      changed <- set control [field "samples",Element sample,field "context",field "sp"] (sp+4) raw
      set control [field "hits",Element (fromIntegral hit),field "sp"] ("0x" ++ showHexWord (fromIntegral sp+4)) changed)
    WrongPrologueStore -> journal (\raw watches -> do
      firstTasks <- arrayAt control [field "first_tasks"] raw
      first <- maybe (Left (MutationRejected control)) Right (case firstTasks of item:_ -> Just item; [] -> Nothing)
      top <- numberAt control [field "stack_top"] first
      entry <- numberAt control [field "entry"] first
      let frame = if entry == 0x69cd58 then 20 else if entry == 0x69d160 then 16 else 4
      recordedEffectFor control (fromIntegral top-frame) (Replace 0) (\row -> W.eventPc row == Just (W.Address (fromIntegral entry))) raw watches)
  where
    showHexWord :: Word32 -> String
    showHexWord number = let digits = "0123456789abcdef" in if number < 16 then [digits !! fromIntegral number] else showHexWord (number `div` 16) ++ [digits !! fromIntegral (number `mod` 16)]

refusalControls :: SourceFiles -> ObservedFiles -> Either ControlError J.Value
refusalControls sources observed = do
  inputs <- either (Left . BaselineRejected) Right (admitInputs sources)
  refusalControlsFor sources inputs observed

refusalControlsFor :: SourceFiles -> Inputs -> ObservedFiles -> Either ControlError J.Value
refusalControlsFor sources inputs observed = do
  baseline <- either (Left . BaselineRejected) Right (admitReceipt inputs observed)
  let receipt = qualifiedReceiptData baseline
      selected control
        | control `elem` [RetiredCriticalRead,WrongJoinedSource] = case Wire.receiptProgram receipt of Wire.JoinedProgram _ -> True; _ -> False
        | control >= WrongReadValue = Wire.receiptSchema receipt == Wire.JoinedTaskProtocol
        | otherwise = True
  results <- mapM (run inputs) (filter selected [minBound..maxBound])
  pure (J.object ["schema" J..= (1 :: Int),"baseline" J..= qualificationValue baseline,"refusals" J..= length results,"cases" J..= results])
  where
    run admittedInputs control = do
      (changedSources,changedObserved) <- mutate control sources observed
      let result = (if control == WrongSource then admitInputs changedSources else Right admittedInputs) >>= \selectedInputs -> admitReceipt selectedInputs changedObserved
          actual = case result of Left problem -> Just problem; Right _ -> Nothing
      unless (actual == Just (expected control)) (Left (WrongRefusal control (expected control) actual))
      pure (J.object ["control" J..= show control,"expected" J..= show (expected control),"actual" J..= fmap show actual])
