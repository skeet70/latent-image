{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.WitnessControls (checkRefusals) where

import Control.Monad (forM, forM_)
import qualified Data.Aeson as J
import Data.Bits (xor)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Digest (..))
import qualified Replacement.KernelInstall.Wire as K
import Replacement.StartupConsumers.Evidence (number)
import Replacement.StartupConsumers.Wire
import Replacement.StartupConsumers.Witness

newtype ControlName = ControlName String
data Control = Control ControlName WitnessError (Receipt -> Receipt)

checkRefusals :: Inputs -> ObservedFiles -> ObservedFiles -> ObservedFiles -> Either String J.Value
checkRefusals inputs retained installer joined = do
  qualifiedRetained <- liftAdmission (admitReceipt inputs retained)
  qualifiedInstaller <- liftAdmission (admitReceipt inputs installer)
  qualifiedJoined <- liftAdmission (admitReceipt inputs joined)
  liftAdmission (compareReceipts qualifiedRetained qualifiedInstaller qualifiedJoined)
  comparisons <- sequence
    [expectComparison (ControlName "duplicate retained role") (compareReceipts qualifiedRetained qualifiedRetained qualifiedJoined)
    ,expectComparison (ControlName "swapped installer and joined roles") (compareReceipts qualifiedRetained qualifiedJoined qualifiedInstaller)
    ,expectComparison (ControlName "missing joined role") (compareReceipts qualifiedRetained qualifiedInstaller qualifiedInstaller)]
  receipts <- mapM (either (const (Left "control receipt decode failed")) Right . J.eitherDecodeStrict' . observedReceipt) [retained,installer,joined]
  rows <- forM receipts $ \receipt -> do
    controls <- forM mutations $ \(Control (ControlName name) expected mutate) ->
      case validateReceipt inputs (mutate receipt) of
        Left actual | actual == expected -> Right (J.object ["control" J..= name,"rejection" J..= show expected])
        Left actual -> Left (name ++ ": expected " ++ show expected ++ ", got " ++ show actual)
        Right _ -> Left (name ++ ": mutation was accepted")
    pure (J.object ["role" J..= show (receiptRole receipt),"controls" J..= controls])
  forM_ [retained,installer,joined] $ \files ->
    case admitReceipt inputs (files {observedCheckpoint = observedCheckpoint files <> B.singleton 0}) of
      Left CheckpointRejected -> Right ()
      Left actual -> Left ("checkpoint mutation: expected CheckpointRejected, got " ++ show actual)
      Right _ -> Left "checkpoint mutation accepted"
  pure (J.object ["schema" J..= (1 :: Int),"qualification" J..= ("exact-startup-consumer-refusals" :: String)
    ,"roles" J..= rows,"comparison_controls" J..= comparisons,"checkpoint_controls" J..= (3 :: Int)])
  where
    liftAdmission :: Either WitnessError value -> Either String value
    liftAdmission = either (Left . show) Right

expectComparison :: ControlName -> Either WitnessError () -> Either String J.Value
expectComparison (ControlName name) result = case result of
  Left ComparisonRejected -> Right (J.object ["control" J..= name,"rejection" J..= show ComparisonRejected])
  Left actual -> Left (name ++ ": expected ComparisonRejected, got " ++ show actual)
  Right () -> Left (name ++ ": role mutation was accepted")

mutations :: [Control]
mutations =
  [Control (ControlName "producer canonical source") IdentityRejected $ \receipt -> receipt
    {receiptIdentity = (receiptIdentity receipt) {identityCanonical = Digest (replicate 64 '0')}}
  ,Control (ControlName "refused is not observation") StatusRejected $ \receipt -> receipt {receiptOutcome = Refused}
  ,Control (ControlName "bounded instruction ceiling") LimitsRejected $ \receipt -> receipt
    {receiptLimits = (receiptLimits receipt) {limitInstructions = Count 0}}
  ,Control (ControlName "source instruction substitution") SourceWordRejected $ mapProbe OuterEntry $ \probe -> probe
    {probeSourceWord = WordValue (number (probeSourceWord probe) `xor` 1)}
  ,Control (ControlName "outer dispatcher SP") FrameRejected $ mapProbe OuterEntry $ \probe -> probe
    {probeContext = (probeContext probe) {contextMachine = (contextMachine (probeContext probe)) {K.contextSp = Address 0x40063458}}}
  ,Control (ControlName "secondary accepts only observed nonzero") InitializerRejected $ mapProbe PeerPostload $ changeRegister 3 0
  ,Control (ControlName "initializer return ABI") InitializerRejected $ mapProbe InitializerReturn $ changeRegister 0 0
  ,Control (ControlName "first root1 target") FirstConsumerRejected $ mapProbe CriticalCall $ changeRegister 3 0xd1164
  ,Control (ControlName "watch teardown error") WatchRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentError = Just "control teardown failure"} | segment <- receiptSegments receipt]}
  ,Control (ControlName "stopped fetch counted as execution") FetchRejected $ \receipt -> receipt {receiptFetches = []}
  ,Control (ControlName "committed readback differs") EffectRejected alterFinal
  ,Control (ControlName "writer preimage differs") PreimageRejected $ \receipt -> receipt
    {receiptSegments = [segment {segmentWatch = (segmentWatch segment) {K.watchRecords = map alterPreimage (K.watchRecords (segmentWatch segment))}} | segment <- receiptSegments receipt]}
  ,Control (ControlName "reservation lock interval unclosed") LifetimeRejected $ \receipt -> receipt
    {receiptResources = [if resourceKind resource == Record && resourcePhysical resource == Address 0x9c280c
      then resource {resourceClosed = Nothing} else resource | resource <- receiptResources receipt]}
  ,Control (ControlName "zero payload claims backed admission") PayloadRejected $ \receipt -> receipt
    {receiptPayloads = receiptPayloads receipt ++ [PayloadAdmission (invocationId invocation) (probeContext probe)
      (Address 0) (Count 0) AdmittedPayload Nothing
      | invocation <- receiptInvocations receipt,invocationFamily invocation == MessageSend
      ,probe <- receiptProbes receipt,Just (probeId probe) == invocationEntry invocation]}
  ,Control (ControlName "message kernel baseline required") LifetimeRejected $ \receipt -> receipt
    {receiptResources = [if resourceKind resource == Record && resourcePhysical resource == Address 0x5988c
      then resource {resourceKind = Auxiliary} else resource | resource <- receiptResources receipt]}
  ,Control (ControlName "executed entry omitted from producer census") CensusRejected omitCensusEntry
  ,Control (ControlName "acquire LDREX access omitted") AllocationRejected (omitLockRead (Address 0x427d4))
  ,Control (ControlName "release LDREX access omitted") AllocationRejected (omitLockRead (Address 0x42800))
  ,Control (ControlName "receive source and call probes belong to another task frame") InvocationRejected $
    mapSelectedProbe (\probe -> probeFamily probe == Just MessageReceive && probeInvocation probe /= Nothing
      && probeKind probe `elem` [SourceProbe,DynamicTarget,DynamicReturn]) otherTaskFrame
  ] ++ concat
    [[Control (ControlName (show family ++ " absent")) (MissingFamily family) $ \receipt -> receipt
        {receiptInvocations = [if invocationFamily invocation == family then invocation {invocationStatus = NotObserved,invocationEntry = Nothing,invocationReturn = Nothing} else invocation | invocation <- receiptInvocations receipt]}
      ,Control (ControlName (show family ++ " unclosed")) (UnclosedFamily family) $ \receipt -> receipt
        {receiptInvocations = [if invocationFamily invocation == family then invocation {invocationStatus = Unclosed,invocationReturn = Nothing} else invocation | invocation <- receiptInvocations receipt]}
      ,Control (ControlName (show family ++ " caller register preservation")) InvocationRejected $
        mapSelectedProbe (\probe -> probeKind probe == FamilyReturn && probeFamily probe == Just family) corruptSavedRegister]
    | family <- [minBound..maxBound]]

omitCensusEntry :: Receipt -> Receipt
omitCensusEntry receipt = receipt {receiptProbes = retained,receiptCensus = map recount (receiptCensus receipt)}
  where
    entries = filter ((==FamilyEntry) . probeKind) (receiptProbes receipt)
    undetailed = [probe | probe <- entries,Just (probeId probe) `notElem` map invocationEntry (receiptInvocations receipt)]
    omitted = map probeId (take 1 (if null undetailed then entries else undetailed))
    retained = [probe | probe <- receiptProbes receipt,probeId probe `notElem` omitted]
    recount item =
      let remaining = [probe | probe <- retained,probeKind probe == FamilyEntry
            ,probeFamily probe == Just (censusFamily item),contextCore (probeContext probe) == censusCore item]
      in item {censusEntries = Count (fromIntegral (length remaining))
        ,censusFirst = case remaining of probe:_ -> Just (probeId probe); [] -> Nothing}

omitLockRead :: Address -> Receipt -> Receipt
omitLockRead instruction receipt = receipt
  {receiptSegments = [segment {segmentWatch = (segmentWatch segment)
    {K.watchRecords = filter (not . omitted) (K.watchRecords (segmentWatch segment))}} | segment <- receiptSegments receipt]
  ,receiptFetches = [attempt {fetchRow = Count (original-fromIntegral removed)}
    | attempt <- receiptFetches receipt,segment <- receiptSegments receipt,segmentId segment == fetchSegment attempt
    ,let Count original = fetchRow attempt
    ,let removed = length [() | (index,event) <- zip [0..] (K.watchRecords (segmentWatch segment)),index < original,omitted event]]}
  where
    omitted event = K.eventSource event == K.CpuRead && K.eventPc event == Just instruction

mapProbe :: ProbeKind -> (Probe -> Probe) -> Receipt -> Receipt
mapProbe kind = mapSelectedProbe ((== kind) . probeKind)

mapSelectedProbe :: (Probe -> Bool) -> (Probe -> Probe) -> Receipt -> Receipt
mapSelectedProbe selected modify receipt = receipt
  {receiptProbes = [probe {probeContext = replaceContext (probeContext probe)} | probe <- changed]
  ,receiptSegments = [segment {segmentStartContexts = map replaceContext (segmentStartContexts segment)
    ,segmentEndContexts = map replaceContext (segmentEndContexts segment)} | segment <- receiptSegments receipt]
  ,receiptResources = [resource {resourceAdmitted = replaceContext (resourceAdmitted resource)
    ,resourceClosed = fmap replaceContext (resourceClosed resource)} | resource <- receiptResources receipt]
  ,receiptPayloads = [payload {payloadContext = replaceContext (payloadContext payload)} | payload <- receiptPayloads receipt]}
  where
    changed = [if selected probe then modify probe else probe | probe <- receiptProbes receipt]
    replacements = [(probeContext before,probeContext after) | (before,after) <- zip (receiptProbes receipt) changed,selected before]
    replaceContext context = maybe context id (lookup context replacements)
changeRegister :: Int -> Word32 -> Probe -> Probe
changeRegister index value probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe)) {K.contextRegisters = [if offset == index then WordValue value else original | (offset,original) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}
otherTaskFrame :: Probe -> Probe
otherTaskFrame probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe))
    {K.contextSp = case K.contextSp (contextMachine (probeContext probe)) of Address incoming -> Address (incoming+0x1000)
    ,K.contextRegisters = [if index == (11 :: Int) then WordValue (number value+0x1000) else value
      | (index,value) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}

corruptSavedRegister :: Probe -> Probe
corruptSavedRegister probe = probe {probeContext = (probeContext probe)
  {contextMachine = (contextMachine (probeContext probe)) {K.contextRegisters =
    [if index == (4 :: Int) then WordValue (number value `xor` 1) else value
    | (index,value) <- zip [0..] (K.contextRegisters (contextMachine (probeContext probe)))]}}}

alterFinal :: Receipt -> Receipt
alterFinal receipt = case receiptResources receipt of
  resource:rest -> receipt {receiptResources = resource {resourceFinal = map alterView (resourceFinal resource)}:rest}
  [] -> receipt
  where
    alterView view = view {viewBytes = case viewBytes view of K.Bytes bytes -> K.Bytes (flipFirst bytes)}
flipFirst :: B.ByteString -> B.ByteString
flipFirst bytes = case B.uncons bytes of Just (byte,rest) -> B.cons (byte `xor` 1) rest; Nothing -> bytes
alterPreimage :: K.MemoryEvent -> K.MemoryEvent
alterPreimage event = if K.eventSource event == K.CpuWrite then
  event {K.eventBefore = fmap (\(K.Bytes bytes) -> K.Bytes (flipFirst bytes)) (K.eventBefore event)}
  else event
