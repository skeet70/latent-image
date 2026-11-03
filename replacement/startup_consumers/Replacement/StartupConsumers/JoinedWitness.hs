{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.JoinedWitness
  ( NativeInputs(..), CapturedArtifact(..), CapturedWatch(..), ObservedFiles(..), Inputs, QualifiedCapture
  , JoinedError(..), admitInputs, consumerInputs, capturePolicy, admitCapture, checkMutation, compareCaptures, qualificationValue
  ) where

import Control.Monad (foldM, forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.List (sort, sortOn)
import qualified Data.Set as Set
import Data.Word (Word64)
import qualified Emulation.Sha256 as SHA
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupConsumers.Evidence as E
import qualified Replacement.StartupConsumers.JoinedHost as Host
import qualified Replacement.StartupConsumers.JoinedWire as S
import qualified Replacement.StartupConsumers.ObservationPolicy as Policy
import qualified Replacement.StartupConsumers.Source as Source
import qualified Replacement.StartupConsumers.Wire as W
import qualified Replacement.StartupConsumers.Witness as Witness

data NativeInputs = NativeInputs
  {nativeLoaderA :: C.NativeFiles,nativeLoaderB :: C.NativeFiles,nativeDispatcher :: C.NativeFiles,nativeInstaller :: C.NativeFiles}
data CapturedArtifact = CapturedArtifact S.ArtifactRole B.ByteString
data CapturedWatch = CapturedWatch S.RelativeFilename B.ByteString
data ObservedFiles = ObservedFiles
  {observedCapture :: B.ByteString,observedCheckpoint :: B.ByteString,observedTrace :: B.ByteString,observedTimers :: B.ByteString,observedUpdate :: B.ByteString
  ,observedArtifacts :: [CapturedArtifact],observedWatches :: [CapturedWatch]}
data Inputs = Inputs Source.SourceFiles NativeInputs Witness.LoaderProducerFiles LS.LoaderStagedCandidate Witness.Inputs
data Cohort = Cohort K.Digest K.Digest K.Digest K.Digest deriving (Eq)
-- Fields: role, cohort, capture digest, policy digest, host checker digest, projection.
data QualifiedCapture = QualifiedCapture S.CaptureRole Cohort K.Digest K.Digest K.Digest Witness.Projection

data JoinedError = ComponentInputsRejected Witness.WitnessError | ConsumerRejected Witness.WitnessError
  | SharedDecodeRejected | SharedIdentityRejected | SharedArtifactRejected | SharedStatusRejected
  | SharedCheckpointRejected | SharedSubobservationRejected | SharedGuardRejected | SharedComparisonRejected
  | SharedPolicyRejected Policy.PolicyError
  deriving (Eq, Show)

require :: Bool -> JoinedError -> Either JoinedError ()
require condition failure = unless condition (Left failure)
hash :: B.ByteString -> K.Digest
hash = K.Digest . SHA.sha256Hex . SHA.sha256
clock :: K.Count -> Word64
clock (K.Count value) = value
counts :: S.CoreCounts -> [K.Count]
counts (S.CoreCounts primary secondary) = [primary,secondary]
total :: S.CoreCounts -> Integer
total = sum . map (toInteger . clock) . counts

consumerInputs :: Inputs -> Witness.Inputs
consumerInputs (Inputs _ _ _ _ checked) = checked

admitInputs :: Source.SourceFiles -> NativeInputs -> Witness.LoaderProducerFiles -> LS.LoaderStagedCandidate -> Either JoinedError Inputs
admitInputs sources native producer candidate = do
  checked <- either (Left . ComponentInputsRejected) Right
    (Witness.admitLoaderJoinedInputs sources (nativeLoaderA native) (nativeLoaderB native)
      (nativeDispatcher native) (nativeInstaller native) producer candidate)
  pure (Inputs sources native producer candidate checked)

artifactBytes :: Inputs -> S.ArtifactRole -> B.ByteString
artifactBytes (Inputs sources native producer candidate _) role = case role of
  S.CanonicalDat -> Source.sourceDat sources; S.CanonicalImage4 -> Source.sourceImage4 sources
  S.CanonicalLoaderA -> Source.sourceLoaderA sources; S.CanonicalLoaderB -> Source.sourceLoaderB sources
  S.CandidateDat -> LS.candidateBytes candidate; S.CandidateImage4 -> LS.candidateImage4 candidate
  S.CandidateLoaderA -> LS.candidateLoader candidate LS.LoaderA; S.CandidateLoaderB -> LS.candidateLoader candidate LS.LoaderB
  S.NativeLoaderA -> C.nativeBytes (nativeLoaderA native); S.SymbolsLoaderA -> C.nativeSymbols (nativeLoaderA native)
  S.NativeLoaderB -> C.nativeBytes (nativeLoaderB native); S.SymbolsLoaderB -> C.nativeSymbols (nativeLoaderB native)
  S.NativeDispatcher -> C.nativeBytes (nativeDispatcher native); S.SymbolsDispatcher -> C.nativeSymbols (nativeDispatcher native)
  S.NativeInstaller -> C.nativeBytes (nativeInstaller native); S.SymbolsInstaller -> C.nativeSymbols (nativeInstaller native)
  S.JoinedContract -> Witness.loaderProducerContract producer

-- The policy must have been issued by the host check for these capture bytes.
capturePolicy :: Inputs -> Host.HostPolicyBytes -> ObservedFiles -> Either JoinedError Policy.ObservationPolicy
capturePolicy (Inputs _ _ _ _ checked) admitted files = do
  require (Host.policyCaptureDigest admitted == hash (observedCapture files)) (SharedPolicyRejected Policy.PolicyBindingRejected)
  capture <- either (const (Left SharedDecodeRejected)) Right (J.eitherDecodeStrict' (observedCapture files))
  trace <- either (const (Left SharedDecodeRejected)) Right (J.eitherDecodeStrict' (observedTrace files))
  artifact <- maybe (Left SharedIdentityRejected) Right (Witness.joinedLoaderProvider checked)
  either (Left . SharedPolicyRejected) Right (Policy.sharedPolicy admitted artifact capture trace)

admitCapture :: Inputs -> Host.HostPolicyBytes -> ObservedFiles -> Either JoinedError QualifiedCapture
admitCapture inputs admitted files = do
  policy <- capturePolicy inputs admitted files
  (role,cohort,projection) <- admitWithPolicy inputs policy files
  pure (QualifiedCapture role cohort (hash (observedCapture files)) (hash (Host.policyBytes admitted)) (Host.policyCheckerDigest admitted) projection)

-- Checks mutated files against the policy issued for the unmutated capture; an admitted mutation yields Right ().
checkMutation :: Inputs -> Host.HostPolicyBytes -> ObservedFiles -> ObservedFiles -> Either JoinedError ()
checkMutation inputs admitted original mutated = do
  policy <- capturePolicy inputs admitted original
  _ <- admitWithPolicy inputs policy original
  _ <- admitWithPolicy inputs policy mutated
  pure ()

admitWithPolicy :: Inputs -> Policy.ObservationPolicy -> ObservedFiles -> Either JoinedError (S.CaptureRole,Cohort,Witness.Projection)
admitWithPolicy inputs@(Inputs _ _ producer _ checked) policy files = do
  capture <- either (const (Left SharedDecodeRejected)) Right (J.eitherDecodeStrict' (observedCapture files))
  trace <- either (const (Left SharedDecodeRejected)) Right (J.eitherDecodeStrict' (observedTrace files))
  let consumer = S.captureConsumer capture
      window = S.consumerEvidence consumer
      role = S.captureRole capture
      identity = S.captureIdentity capture
  require (S.captureSchema capture == 1 && S.captureError capture == Nothing && S.captureFailure capture == Nothing
    && S.captureInstructions capture == K.Count 382000000 && S.captureMenu capture == S.Observed (Just (S.MenuCursor 1 5 2))) SharedStatusRejected
  actualCounts <- case S.captureCounts capture of S.Observed value -> Right value; _ -> Left SharedStatusRejected
  require (total actualCounts == 382000000) SharedStatusRejected
  checkArtifacts inputs files capture
  checkIdentity inputs capture
  require (hash (observedUpdate files) == S.identityUpdate identity) SharedIdentityRejected
  require (not (B.null (observedCheckpoint files)) && hash (observedCheckpoint files) == S.captureCheckpoint capture
    && W.receiptCheckpoint window == S.captureCheckpoint capture && not (W.receiptCheckpointReused window)) SharedCheckpointRejected
  require (S.captureTrace capture == S.FileBinding S.traceFilename (hash (observedTrace files))
    && S.captureTimers capture == S.FileBinding S.timerFilename (hash (observedTimers files))) SharedIdentityRejected
  require (W.receiptRole window == case role of S.RetainedCapture -> W.Retained; S.JoinedCapture -> W.Joined) SharedIdentityRejected
  checkSubobservation capture trace
  checkEpochs files capture actualCounts
  projection <- either (Left . ConsumerRejected) Right (Witness.validateReceiptWithPolicy policy checked window)
  let cohort = Cohort (S.identityRun identity) (S.identityRom identity) (S.identityExecutable identity) (hash (Witness.loaderProducerContract producer))
  pure (role,cohort,projection)

checkArtifacts :: Inputs -> ObservedFiles -> S.SharedCapture -> Either JoinedError ()
checkArtifacts inputs files capture = do
  let roles = [minBound..maxBound]
      expected = [S.FileBinding (S.artifactFilename role) (hash (artifactBytes inputs role)) | role <- roles]
      key = S.relativeName . S.bindingPath
      captured = observedArtifacts files
  require (sort [role | CapturedArtifact role _ <- captured] == roles
    && sortOn key (S.captureArtifacts capture) == sortOn key expected) SharedArtifactRejected
  forM_ captured $ \(CapturedArtifact role bytes) -> require (bytes == artifactBytes inputs role) SharedArtifactRejected
  require (case (S.captureRole capture,S.captureProgram capture) of
    (S.RetainedCapture,S.RetainedProgram) -> True
    (S.JoinedCapture,S.JoinedProgram bindings) -> sortOn key bindings == sortOn key expected
    _ -> False) SharedArtifactRejected

checkIdentity :: Inputs -> S.SharedCapture -> Either JoinedError ()
checkIdentity (Inputs sources _ producer candidate _) capture = do
  let identity = S.captureIdentity capture
      update = case S.captureRole capture of S.RetainedCapture -> Source.sourceDat sources; S.JoinedCapture -> LS.candidateBytes candidate
      image = case S.captureRole capture of S.RetainedCapture -> Source.sourceImage4 sources; S.JoinedCapture -> LS.candidateImage4 candidate
      loader side = case S.captureRole capture of
        S.JoinedCapture -> LS.candidateLoader candidate side
        S.RetainedCapture -> case side of LS.LoaderA -> Source.sourceLoaderA sources; LS.LoaderB -> Source.sourceLoaderB sources
      catalog = S.identitySources identity
      expected = [(S.LoaderA,0x40000,loader LS.LoaderA),(S.LoaderB,0xd0000,loader LS.LoaderB),(S.Image4,0x61c400,image)]
  require (S.identityRun identity == hash (Witness.loaderProducerRun producer)
    && S.identityRom identity == hash (Witness.loaderProducerRom producer)
    && S.identityExecutable identity == hash (Witness.loaderProducerExecutable producer)
    && S.identityUpdate identity == hash update
    && S.identityUpdateFile identity == S.FileBinding S.updateFilename (hash update)
    && sort (map S.sourceKind catalog) == [minBound..maxBound]) SharedIdentityRejected
  forM_ expected $ \(kind,base,bytes) -> require
    ([binding | binding <- catalog,S.sourceKind binding==kind] == [S.SourceBinding kind (K.Address base) (K.Count (fromIntegral (B.length bytes))) (hash bytes)]) SharedIdentityRejected

checkSubobservation :: S.SharedCapture -> [S.TraceHit] -> Either JoinedError ()
checkSubobservation capture trace = do
  let consumer = S.captureConsumer capture
      window = S.consumerEvidence consumer
      segments = W.receiptSegments window
      probes = W.receiptProbes window
  begin <- maybe (Left SharedSubobservationRejected) Right (S.consumerBegin consumer)
  end <- maybe (Left SharedSubobservationRejected) Right (S.consumerEnd consumer)
  require (S.consumerTermination consumer == Just S.FirstFamiliesAndSecondaryRoot1
    && S.endpointInstructions begin < S.endpointInstructions end && S.endpointInstructions end <= S.captureInstructions capture
    && total (S.endpointCounts begin) == toInteger (clock (S.endpointInstructions begin))
    && total (S.endpointCounts end) == toInteger (clock (S.endpointInstructions end))
    && W.receiptInstructions window == S.endpointInstructions end) SharedSubobservationRejected
  (first,lastSegment) <- case segments of [] -> Left SharedSubobservationRejected; initial:rest -> Right (initial,last (initial:rest))
  require (W.segmentStart first == counts (S.endpointCounts begin) && W.segmentEnd lastSegment == counts (S.endpointCounts end)) SharedSubobservationRejected
  let W.ProbeRange terminalBegin terminalEnd = W.segmentProbeRange lastSegment
  require (any (\probe -> W.probeId probe >= terminalBegin && W.probeId probe < terminalEnd
    && E.probeClock probe == clock (S.endpointInstructions end)
    && W.probeKind probe `elem` [W.FamilyReturn,W.CriticalReturn]) probes) SharedSubobservationRejected
  reconcileSharedContexts trace begin end window
  forM_ [resource | resource <- W.receiptResources window,W.resourceLifetime resource == W.ObservationLifetime] $ \resource ->
    require (maybe False ((==clock (S.endpointInstructions end)) . E.clock) (W.resourceClosed resource)) SharedSubobservationRejected

data SharedTraceCursor = SharedTraceCursor [S.TraceHit] [W.Probe] W.ProbeId

matchesContext :: S.TraceHit -> W.Context -> Bool
matchesContext hit context =
  let machine = W.contextMachine context
  in S.hitCore hit == W.contextCore context && S.hitPc hit == K.contextPc machine
    && S.registerWords (S.hitRegisters hit) == K.contextRegisters machine
    && S.hitSp hit == K.contextSp machine && S.hitLr hit == K.contextLr machine && S.hitCpsr hit == K.contextCpsr machine
    && toInteger (clock (S.hitInstructions hit)) == sum (map (toInteger . clock) (K.contextCounts machine))

reconcileSharedContexts :: [S.TraceHit] -> S.Endpoint -> S.Endpoint -> W.Receipt -> Either JoinedError ()
reconcileSharedContexts trace begin end window = do
  let S.TraceIndex beginIndex = S.endpointTraceIndex begin
      S.TraceIndex endIndex = S.endpointTraceIndex end
      clocks = map S.hitInstructions trace
      probes = W.receiptProbes window
  require (beginIndex > 0 && beginIndex <= endIndex && toInteger endIndex <= toInteger (length trace)
    && and (zipWith (<=) clocks (drop 1 clocks))) SharedSubobservationRejected
  outer <- either (const (Left SharedSubobservationRejected)) Right (E.probeAt window W.OuterEntry)
  let prefix = take (fromIntegral beginIndex) trace
      entryHits = [hit | hit <- prefix,S.hitInstructions hit == S.endpointInstructions begin,matchesContext hit (W.probeContext outer)]
  require (length entryHits == 1 && K.contextCounts (E.machine outer) == counts (S.endpointCounts begin)
    && S.hitInstructions (last prefix) == S.endpointInstructions begin) SharedSubobservationRejected
  SharedTraceCursor remaining pending consumed <- foldM consume
    (SharedTraceCursor (drop (fromIntegral beginIndex) trace) probes (W.ProbeId 0)) (W.receiptSegments window)
  require (length remaining == length trace-fromIntegral endIndex && null pending
    && consumed == W.ProbeId (fromIntegral (length probes))) SharedSubobservationRejected
  where
    consume (SharedTraceCursor remaining pending expected) segment = do
      let W.ProbeRange first@(W.ProbeId low) after@(W.ProbeId high) = W.segmentProbeRange segment
          starts = W.segmentStartContexts segment
          ends = W.segmentEndContexts segment
      require (first == expected && low <= high && toInteger (high-low) <= toInteger (length pending)
        && map W.contextCore starts == [K.Primary,K.Secondary] && map W.contextCore ends == [K.Primary,K.Secondary]
        && all ((==W.segmentStart segment) . K.contextCounts . W.contextMachine) starts
        && all ((==W.segmentEnd segment) . K.contextCounts . W.contextMachine) ends) SharedSubobservationRejected
      rest <- foldM (consumeStop ends) remaining (W.segmentStops segment)
      let (selected,next) = splitAt (fromIntegral (high-low)) pending
      require (map W.probeId selected == map W.ProbeId (take (length selected) [low..])) SharedSubobservationRejected
      forM_ selected $ \probe ->
        if W.probeKind probe == W.OuterEntry && W.probeId probe == W.ProbeId 0 then
          require (W.probeContext probe `elem` starts) SharedSubobservationRejected
        else require (W.probeContext probe `elem` ends && any (matchesStop probe) (W.segmentStops segment)) SharedSubobservationRejected
      pure (SharedTraceCursor rest next after)
    consumeStop ends remaining stopped = case remaining of
      [] -> Left SharedSubobservationRejected
      hit:rest -> do
        context <- case [value | value <- ends,W.contextCore value == W.stopCore stopped] of
          [value] -> Right value
          _ -> Left SharedSubobservationRejected
        require (S.hitCore hit == W.stopCore stopped && S.hitPc hit == W.stopPc stopped
          && S.hitInstructions hit == W.stopInstructions stopped && matchesContext hit context) SharedSubobservationRejected
        pure rest
    matchesStop probe stopped = W.stopCore stopped == E.core probe && W.stopPc stopped == K.contextPc (E.machine probe)
      && W.stopInstructions stopped == K.Count (E.probeClock probe)

checkEpochs :: ObservedFiles -> S.SharedCapture -> S.CoreCounts -> Either JoinedError ()
checkEpochs files capture actualCounts = do
  let epochs = S.captureEpochs capture
      consumer = S.captureConsumer capture
      segments = W.receiptSegments (S.consumerEvidence consumer)
      references = [index | epoch <- epochs,S.ConsumerSegment index <- [S.epochEvidence epoch]]
      watchBindings = [binding | epoch <- epochs,S.WatchFile binding _ <- [S.epochEvidence epoch]]
  (first,final) <- case epochs of [] -> Left SharedGuardRejected; initial:rest -> Right (initial,last (initial:rest))
  require (S.epochStart first == K.Count 0 && S.epochStartCounts first == S.CoreCounts (K.Count 0) (K.Count 0)
    && S.epochEnd final == S.captureInstructions capture && S.epochEndCounts final == actualCounts
    && references == map S.SegmentIndex (take (length segments) [0..])
    && any ((==K.Count 8000000) . S.epochEnd) epochs) SharedGuardRejected
  forM_ (zip epochs (drop 1 epochs)) $ \(before,after) ->
    require (S.epochEnd before == S.epochStart after && S.epochEndCounts before == S.epochStartCounts after) SharedGuardRejected
  forM_ (zip [0::Int ..] epochs) $ \(ordinal,epoch) -> do
    let begin = S.epochStart epoch; end = S.epochEnd epoch
        stage = if ordinal==0 then S.Bootstrap else if begin < K.Count 8000000 then S.Startup else S.Tail
    require (begin <= end && S.epochStage epoch == stage && not (begin < K.Count 8000000 && end > K.Count 8000000)
      && total (S.epochStartCounts epoch) == toInteger (clock begin) && total (S.epochEndCounts epoch) == toInteger (clock end)
      && and (zipWith (<=) (counts (S.epochStartCounts epoch)) (counts (S.epochEndCounts epoch)))) SharedGuardRejected
    case S.epochEvidence epoch of
      S.ConsumerSegment (S.SegmentIndex index) -> do
        segment <- case drop (fromIntegral index) segments of value:_ -> Right value; _ -> Left SharedGuardRejected
        require (W.segmentStart segment == counts (S.epochStartCounts epoch) && W.segmentEnd segment == counts (S.epochEndCounts epoch)) SharedGuardRejected
        require (begin < end || (not (null (W.segmentStops segment))
          && any ((==W.segmentId segment) . W.fetchSegment) (W.receiptFetches (S.consumerEvidence consumer)))) SharedGuardRejected
      S.WatchFile binding teardownError -> do
        require (begin < end && teardownError == Nothing) SharedGuardRejected
        bytes <- case [value | CapturedWatch name value <- observedWatches files,name == S.bindingPath binding] of [value] -> Right value; _ -> Left SharedGuardRejected
        watch <- either (const (Left SharedGuardRejected)) Right (J.eitherDecodeStrict' bytes)
        require (hash bytes == S.bindingDigest binding && K.watchEnabled watch && K.watchViolation watch == Nothing
          && K.watchViews watch == (if stage == S.Bootstrap then "observe" else "fixed")) SharedGuardRejected
  require (sort [name | CapturedWatch name _ <- observedWatches files] == sort (map S.bindingPath watchBindings)
    && length watchBindings == Set.size (Set.fromList (map S.bindingPath watchBindings))) SharedGuardRejected

compareCaptures :: QualifiedCapture -> QualifiedCapture -> Either JoinedError ()
compareCaptures (QualifiedCapture before beforeCohort _ _ beforeChecker beforeProjection) (QualifiedCapture after afterCohort _ _ afterChecker afterProjection) = do
  require (before == S.RetainedCapture && after == S.JoinedCapture && beforeCohort == afterCohort && beforeChecker == afterChecker) SharedComparisonRejected
  either (const (Left SharedComparisonRejected)) Right (Witness.compareLoaderSemanticProjections beforeProjection afterProjection)

qualificationValue :: QualifiedCapture -> J.Value
qualificationValue (QualifiedCapture role _ (K.Digest receipt) (K.Digest policy) (K.Digest checker) _) = J.object
  ["schema" J..= (1::Int),"qualification" J..= ("source-admitted-shared-startup-consumers"::String)
  ,"role" J..= show role,"receipt_sha256" J..= receipt,"observation_policy_sha256" J..= policy,"host_checker_sha256" J..= checker
  ,"comparison_scope" J..= ("source-proved-loader-caller-and-exact-outer-public-effects-and-selected-operation-boundaries"::String)
  ,"post_return_scheduler_equality" J..= False]
