{-# LANGUAGE OverloadedStrings #-}
module Emulation.IdleTaskControls (ControlError(..), refusalControls) where

import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (xor, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.List (findIndex)
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Emulation.IdleTaskWitness
import Emulation.IdleTaskWire
import qualified Emulation.StartupTaskWire as S
import FujiAF.JSON (decodeStrictIntegers)
import qualified Replacement.IdleTask.Admission as A
import qualified Replacement.IdleTask.Artifact as I
import qualified Replacement.KernelInstall.Wire as W

data Control = WrongSource | WrongArtifact | WrongCandidate | WrongContract | DuplicateReceiptKey | RefusedOutcome | WrongObserver
  | WrongReset | WrongRawSidecar | DisabledWatch | WrongWatchClasses | MissingSample | WrongAlias | WrongCodeWord
  | WrongWriterCore | ForeignWriter | CodeWrite | WrongKnownPreimage | MalformedStoppedFetch | WrongStoppedClock
  | WrappedCounts | WrongTask | WrongEntryContext | WrongIrqStatus | WrongIrqFrame | WrongIrqReturn | WrongRestoredRegister
  | MissingRepeatedWait deriving (Eq, Show, Enum, Bounded)
data ControlError = BaselineRejected WitnessError | MutationRejected Control | WrongRefusal Control WitnessError (Maybe WitnessError) deriving (Eq, Show)
data Part = Field K.Key | Element Int
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
encode :: J.Value -> B.ByteString
encode = L.toStrict . J.encode
hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256
zeroDigest :: String
zeroDigest = replicate 64 '0'
expected :: Control -> WitnessError
expected control = case control of
  WrongSource -> SourceRejected; WrongArtifact -> ArtifactRejected; WrongCandidate -> CandidateRejected; WrongContract -> ContractRejected
  DuplicateReceiptKey -> ReceiptDecodeRejected; RefusedOutcome -> StatusRejected; WrongObserver -> IdentityRejected; WrongReset -> ResetRejected
  WrongRawSidecar -> RawWatchRejected; DisabledWatch -> WatchRejected; WrongWatchClasses -> WatchRejected; MissingSample -> SnapshotRejected
  WrongAlias -> AliasRejected; WrongCodeWord -> CodeRejected; WrongWriterCore -> FetchRejected; ForeignWriter -> ForeignWriterRejected
  CodeWrite -> WriteRejected; WrongKnownPreimage -> PreimageRejected; MalformedStoppedFetch -> FetchRejected; WrongStoppedClock -> FetchRejected
  WrappedCounts -> ClockRejected; WrongTask -> TaskRejected; WrongEntryContext -> EntryRejected; WrongIrqStatus -> IrqRejected
  WrongIrqFrame -> FrameRejected; WrongIrqReturn -> ReturnRejected; WrongRestoredRegister -> ReturnRejected; MissingRepeatedWait -> RepetitionRejected

withHash :: Control -> ObservedFiles -> J.Value -> J.Value -> Either ControlError ObservedFiles
withHash control original receipt watch = do
  changed <- set control [field "memory_watch_sha256"] (hash (encode watch)) receipt
  pure original {observedReceipt=encode changed,observedWatch=encode watch}
hexadecimal :: Word32 -> String
hexadecimal number = "0x" ++ digits number
  where digits value = let alphabet = "0123456789abcdef" in if value < 16 then [alphabet !! fromIntegral value] else digits (value `div` 16) ++ [alphabet !! fromIntegral (value `mod` 16)]
changeContext :: Control -> Int -> String -> Word32 -> J.Value -> Either ControlError J.Value
changeContext control index name number receipt = do
  changed <- set control [field "samples",Element index,field "context",field name] number receipt
  if name == "spsr" then pure changed else set control [field "hits",Element index,field name] (if name == "cpsr" then J.toJSON number else J.toJSON (hexadecimal number)) changed
changeRegister :: Control -> Int -> Int -> Word32 -> J.Value -> Either ControlError J.Value
changeRegister control index register number receipt = do
  changed <- set control [field "samples",Element index,field "context",field "registers",Element register] number receipt
  set control [field "hits",Element index,field "registers",field ("r" ++ show register)] (hexadecimal number) changed
findPosition :: Control -> (Sample -> Bool) -> Receipt -> Either ControlError Int
findPosition control predicate receipt = maybe (Left (MutationRejected control)) Right (findIndex predicate (receiptSamples receipt))
word :: W.WordValue -> Word32
word (W.WordValue number) = number
address :: W.Address -> Word32
address (W.Address number) = number
count :: W.Count -> Integer
count (W.Count number) = toInteger number
state :: Sample -> W.Context
state = contextState . sampleContext
ownerNode :: S.KernelCore -> Word32
ownerNode S.PrimaryKernel = 0x40061524
ownerNode S.SecondaryKernel = 0x400f151c
isCurrent :: Sample -> Bool
isCurrent sample = any (\view -> viewObject view == Current && viewCore view == contextCore (sampleContext sample)
  && let W.Bytes contents = viewBytes view in B.length contents == 4 && word32 contents == ownerNode (viewCore view)) (sampleViews sample)
  where word32 contents = sum [fromIntegral (B.index contents index) * 256^index | index <- [0..3 :: Int]]

refusalControls :: Role -> SourceFiles -> ObservedFiles -> Either ControlError J.Value
refusalControls role sources original = do
  inputs <- either (Left . BaselineRejected) Right (admitInputs role sources)
  _ <- either (Left . BaselineRejected) Right (admitReceipt inputs original)
  receiptValue <- either (const (Left (MutationRejected MissingSample))) Right (decodeStrictIntegers (observedReceipt original))
  watchValue <- either (const (Left (MutationRejected WrongRawSidecar))) Right (decodeStrictIntegers (observedWatch original))
  receipt <- parse MissingSample receiptValue
  watch <- parse WrongRawSidecar watchValue
  artifact <- either (const (Left (MutationRejected WrongArtifact))) Right (I.deriveArtifact (sourceLinked sources) (sourceSymbols sources))
  proof <- case role of
    Owned -> Right (I.artifactProof artifact)
    Retained -> either (const (Left (MutationRejected WrongArtifact))) Right (A.deriveIdleProof (B.take 16 (B.drop (0x69cbf0-0x61c400) (sourceImage sources))))
  results <- mapM (run inputs receiptValue watchValue receipt watch proof) [minBound..maxBound]
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("serialized-idle-task-refusals" :: String),"baseline_admitted" J..= True,"role" J..= role,"controls" J..= results])
  where
    run inputs receiptValue watchValue receipt watch proof control = do
      (changedSource,changedObserved) <- mutate receiptValue watchValue receipt watch proof control
      let result = case changedSource of
            Just modified -> admitInputs role modified >> pure ()
            Nothing -> admitReceipt inputs changedObserved >> pure ()
          actual = either Just (const Nothing) result
      unless (actual == Just (expected control)) (Left (WrongRefusal control (expected control) actual))
      pure (J.object ["control" J..= show control,"refusal" J..= show (expected control),"passed" J..= True])
    mutate receiptValue watchValue receipt watch proof control = do
      let plain value = Right (Nothing,original {observedReceipt=encode value})
          source value = Right (Just value,original)
          changedWatch value = (Nothing,) <$> withHash control original receiptValue value
          firstEntry = findPosition control ((==IdleEntry) . sampleBoundary) receipt
          irq = findPosition control (\sample -> sampleBoundary sample == IrqEntry && isCurrent sample
            && word (contextSpsr (sampleContext sample)) .&. 0xff == 0x53 && address (W.contextLr (state sample)) == A.resumePc proof+4) receipt
          after index kind = maybe (Left (MutationRejected control)) (Right . (+index+1)) (findIndex (\sample -> sampleBoundary sample == kind
            && contextCore (sampleContext sample) == contextCore (sampleContext (receiptSamples receipt !! index))) (drop (index+1) (receiptSamples receipt)))
          matchingReturn entered = do
            let core = contextCore (sampleContext (receiptSamples receipt !! entered))
                waits = [sample | sample <- take entered (receiptSamples receipt),contextCore (sampleContext sample) == core
                  ,address (W.contextPc (state sample)) == A.wfiPc proof]
            waiting <- maybe (Left (MutationRejected control)) Right (case reverse waits of item:_ -> Just item; [] -> Nothing)
            maybe (Left (MutationRejected control)) (Right . (+entered+1)) (findIndex (\sample -> contextCore (sampleContext sample) == core
              && sampleBoundary sample == IrqReturn && isCurrent sample
              && address (W.contextSp (state sample))+28 == address (W.contextSp (state waiting))
              && contextSpsr (sampleContext sample) == W.contextCpsr (state waiting)) (drop (entered+1) (receiptSamples receipt)))
          firstWrite = maybe (Left (MutationRejected control)) Right (findIndex ((==W.CpuWrite) . W.eventSource) (W.watchRecords watch))
          stoppedRow = case drop 1 (receiptSamples receipt) of sample:_ -> Right (sampleCursor sample-1); [] -> Left (MutationRejected control)
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
        MissingSample -> modifyAt control [field "samples"] (\value -> do values <- parse control value :: Either ControlError [J.Value]; pure (J.toJSON (drop 1 values))) receiptValue >>= plain
        WrongAlias -> set control [field "samples",Element 0,field "views",Element 0,field "aliases",Element 0,Element 0] zeroDigest receiptValue >>= plain
        WrongCodeWord -> set control [field "samples",Element 0,field "code_word"] (0 :: Word32) receiptValue >>= plain
        WrongWriterCore -> do index <- firstWrite; set control [field "records",Element index,field "core"] J.Null watchValue >>= changedWatch
        ForeignWriter -> do index <- firstWrite; set control [field "records",Element index,field "source"] ("host-write" :: String) watchValue >>= changedWatch
        CodeWrite -> do
          index <- firstWrite
          changed <- set control [field "records",Element index,field "physical"] (0x69cbf0 :: Word32) watchValue
          set control [field "records",Element index,field "address"] (0x69cbf0 :: Word32) changed >>= changedWatch
        WrongKnownPreimage -> do
          index <- firstWrite
          old <- pathAt control [field "records",Element index,field "before",Element 0] watchValue >>= parse control :: Either ControlError Word32
          set control [field "records",Element index,field "before",Element 0] (old `xor` 1) watchValue >>= changedWatch
        MalformedStoppedFetch -> do index <- stoppedRow; set control [field "records",Element index,field "size"] (2 :: Int) watchValue >>= changedWatch
        WrongStoppedClock -> do
          index <- stoppedRow
          old <- pathAt control [field "records",Element index,field "instructions"] watchValue >>= parse control :: Either ControlError Integer
          set control [field "records",Element index,field "instructions"] (old+1) watchValue >>= changedWatch
        WrappedCounts -> do
          old <- pathAt control [field "samples",Element 0,field "context",field "counts",Element 0] receiptValue >>= parse control :: Either ControlError Integer
          set control [field "samples",Element 0,field "context",field "counts",Element 0] (old+4294967296) receiptValue >>= plain
        WrongTask -> coherentTask control receiptValue watchValue >>= \(r,w) -> (Nothing,) <$> withHash control original r w
        WrongEntryContext -> do index <- firstEntry; changeContext control index "cpsr" 0xd3 receiptValue >>= plain
        WrongIrqStatus -> do
          index <- irq
          let old = word (contextSpsr (sampleContext (receiptSamples receipt !! index)))
          changeContext control index "spsr" (old `xor` 0x20000000) receiptValue >>= plain
        WrongIrqFrame -> do
          entered <- irq; index <- after entered IrqFrame
          changeContext control index "sp" (address (W.contextSp (state (receiptSamples receipt !! index)))+4) receiptValue >>= plain
        WrongIrqReturn -> do
          entered <- irq; index <- matchingReturn entered
          let old = word (contextSpsr (sampleContext (receiptSamples receipt !! index)))
          changeContext control index "spsr" (old `xor` 0x20000000) receiptValue >>= plain
        WrongRestoredRegister -> do
          entered <- irq; returned <- matchingReturn entered
          let core = contextCore (sampleContext (receiptSamples receipt !! returned))
          index <- maybe (Left (MutationRejected control)) (Right . (+returned+1)) (findIndex (\sample -> contextCore (sampleContext sample) == core
            && address (W.contextPc (state sample)) == A.resumePc proof) (drop (returned+1) (receiptSamples receipt)))
          let old = word (head (W.contextRegisters (state (receiptSamples receipt !! index))))
          changeRegister control index 0 (old `xor` 1) receiptValue >>= plain
        MissingRepeatedWait -> removeLastRepeated control receiptValue watchValue receipt watch proof >>= \(r,w) -> (Nothing,) <$> withHash control original r w

coherentTask :: Control -> J.Value -> J.Value -> Either ControlError (J.Value,J.Value)
coherentTask control receipt watch = do
  let target = 0x54760 :: Word32; oldNode = 0x40061524 :: Word32; newNode = 0x4006155c :: Word32
      replaceBytes raw = do
        W.Bytes body <- parse control raw
        if B.length body == 4 && decodeWord body == oldNode then pure (J.toJSON (B.unpack (putWord newNode))) else pure raw
      replaceView value = do
        location <- pathAt control [field "physical"] value >>= parse control :: Either ControlError Word32
        if location /= target then pure value else do
          changed <- modifyAt control [field "bytes"] replaceBytes value
          W.Bytes body <- pathAt control [field "bytes"] changed >>= parse control
          set control [field "aliases"] (replicate 2 (replicate 2 (hash body))) changed
      replaceSample = modifyAt control [field "views"] (mapArray replaceView)
      replaceRow value = do
        targetValue <- pathAt control [field "physical"] value >>= parse control :: Either ControlError Word32
        if targetValue /= target then pure value else do
          source <- pathAt control [field "source"] value >>= parse control :: Either ControlError String
          if source /= "cpu-write" then pure value else do
            changed <- modifyAt control [field "before"] replaceBytes value
            stored <- pathAt control [field "value"] changed >>= parse control :: Either ControlError Word32
            if stored == oldNode then set control [field "value"] newNode changed else pure changed
  r <- modifyAt control [field "samples"] (mapArray replaceSample) receipt
  w <- modifyAt control [field "records"] (mapArray replaceRow) watch
  pure (r,w)
  where
    mapArray f value = J.toJSON <$> ((parse control value :: Either ControlError [J.Value]) >>= mapM f)
    decodeWord body = sum [fromIntegral (B.index body index)*256^index | index <- [0..3 :: Int]] :: Word32
    putWord number = B.pack [fromIntegral (number `div` 256^index) | index <- [0..3 :: Int]]

removeLastRepeated :: Control -> J.Value -> J.Value -> Receipt -> W.Watch -> A.IdleProof -> Either ControlError (J.Value,J.Value)
removeLastRepeated control receiptValue watchValue receipt watch proof = do
  let samples = receiptSamples receipt
      firstAfter index predicate = maybe (Left (MutationRejected control)) (Right . (+index+1)) (findIndex predicate (drop (index+1) samples))
      repeatedFor core = do
        waiting <- firstAfter (-1) (\sample -> contextCore (sampleContext sample) == core && address (W.contextPc (state sample)) == A.wfiPc proof)
        let saved = state (samples !! waiting)
        returned <- firstAfter waiting (\sample -> contextCore (sampleContext sample) == core && sampleBoundary sample == IrqReturn
          && isCurrent sample && address (W.contextSp (state sample))+28 == address (W.contextSp saved) && contextSpsr (sampleContext sample) == W.contextCpsr saved)
        resumed <- firstAfter returned (\sample -> contextCore (sampleContext sample) == core && address (W.contextPc (state sample)) == A.resumePc proof)
        repeated <- firstAfter resumed (\sample -> contextCore (sampleContext sample) == core && address (W.contextPc (state sample)) == A.wfiPc proof)
        pure (repeated,samples !! repeated)
  candidates <- mapM repeatedFor [S.PrimaryKernel,S.SecondaryKernel]
  (_,repeated) <- case candidates of
    [left,right] -> Right (if fst left > fst right then left else right)
    _ -> Left (MutationRejected control)
  let lower = sum (map count (W.contextCounts (state repeated))); core = S.watchCore (contextCore (sampleContext repeated))
      stopped = map ((subtract 1) . sampleCursor) (drop 1 samples)
      deleted = [index | (index,event) <- zip [0..] (W.watchRecords watch),index `notElem` stopped,W.eventSource event == W.CpuFetch
        ,W.eventCore event == Just core,W.eventPc event == Just (W.Address (A.wfiPc proof)),maybe False ((>lower) . count) (W.eventInstructions event)]
  unless (not (null deleted)) (Left (MutationRejected control))
  rows <- pathAt control [field "records"] watchValue >>= parse control :: Either ControlError [J.Value]
  w <- set control [field "records"] [row | (index,row) <- zip [0..] rows,index `notElem` deleted] watchValue
  r <- foldM (\value (index,sample) -> set control [field "samples",Element index,field "watch_cursor"]
    (sampleCursor sample-length (filter (<sampleCursor sample) deleted)) value) receiptValue (zip [0..] samples)
  pure (r,w)
