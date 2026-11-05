{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.Witness
  ( NativeFiles (..), ProducerFiles (..), LoaderProducerFiles (..), ObservedFiles (..), Inputs, Projection, QualifiedReceipt
  , WitnessError (..), admitInputs, admitLoaderJoinedInputs, joinedLoaderProvider, admitReceipt, validateReceipt, validateReceiptWithPolicy, compareReceipts, qualificationValue
  , qualifiedProjection, compareSemanticProjections, compareLoaderSemanticProjections
  ) where

import Control.Monad (forM, forM_, foldM)
import qualified A32 as A
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (sort, sortOn)
import qualified Data.Set as Set
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Replacement.KernelInstall.Wire (Address (..), WordValue (..), Count (..), Digest (..), Core (..), MemoryEvent (..), Source (..))
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.KernelInstall.Artifact as I
import qualified Replacement.KernelInstall.Admission as IA
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.StartupDispatch.Artifact as D
import Replacement.StartupConsumers.Evidence
import Replacement.StartupConsumers.Effects (ConsumerProjection, checkConsumers, consumerProjectionValue)
import qualified Replacement.StartupConsumers.ObservationPolicy as Policy
import qualified Replacement.StartupConsumers.Source as S
import Replacement.StartupConsumers.Wire

data NativeFiles = NativeFiles {nativeImage :: B.ByteString, nativeSymbols :: B.ByteString, nativeContract :: B.ByteString}
data ProducerFiles = ProducerFiles
  {producerExecutable :: B.ByteString, producerRun :: B.ByteString, producerRom :: B.ByteString
  ,producerInstallerDat :: B.ByteString, producerJoinedDat :: B.ByteString, producerJoinedContract :: B.ByteString}
data LoaderProducerFiles = LoaderProducerFiles
  {loaderProducerExecutable :: B.ByteString,loaderProducerRun :: B.ByteString,loaderProducerRom :: B.ByteString
  ,loaderProducerDat :: B.ByteString,loaderProducerContract :: B.ByteString}
data BoundProducer = StandaloneProducer ProducerFiles | LoaderJoinedProducer LoaderProducerFiles
data LoaderProvider = CanonicalLoaderProvider | JoinedLoaderProvider LS.LoaderJoinedArtifact LS.LoaderStagedCandidate
data ObservedFiles = ObservedFiles {observedReceipt :: B.ByteString, observedCheckpoint :: B.ByteString}
data Inputs = Inputs
  {inputSource :: S.SourceModel, inputInstaller :: NativeFiles, inputDispatcher :: NativeFiles, inputProducer :: BoundProducer
  ,inputInstallerImage :: B.ByteString, inputJoinedImage :: B.ByteString, inputJoinedProof :: C.JoinedProof
  ,inputInstallerProof :: IA.InstallationProof,inputLoaders :: LoaderProvider}
data CallerState = CallerState Address [WordValue] WordValue K.Bytes deriving (Eq)
data BoundaryMemory = BoundaryMemory ResourceKind Address Count K.Bytes K.Bytes deriving (Eq)
data OuterProjection = OuterProjection CallerState CallerState K.Bytes [BoundaryMemory] deriving (Eq)
data Projection = Projection OuterProjection ConsumerProjection deriving (Eq)
data CoverageState = SelectedInvocationReturned | CensusEntryOnly | NoEntryObserved deriving (Show)
data ObservationCoverage = ObservationCoverage Census CoverageState
data QualifiedReceipt = QualifiedReceipt Role Identity Digest Digest [ObservationCoverage] Projection

hash :: B.ByteString -> Digest
hash = Digest . SHA.sha256Hex . SHA.sha256

admitInputs :: S.SourceFiles -> NativeFiles -> NativeFiles -> ProducerFiles -> I.StagedCandidate -> D.StagedCandidate -> C.StagedCandidate -> Either WitnessError Inputs
admitInputs sources installer dispatcher producer installerCandidate dispatcherCandidate joinedCandidate = do
  model <- either (const (Left SourceRejected)) Right (S.deriveSource sources)
  canonical <- either (const (Left SourceRejected)) Right
    (C.canonicalSource (C.SourceFiles (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources)))
  installationSource <- either (const (Left SourceRejected)) Right
    (I.canonicalSource (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources))
  dispatchSource <- either (const (Left SourceRejected)) Right
    (D.canonicalSource (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources))
  joined <- either (const (Left ArtifactRejected)) Right
    (C.deriveArtifact (C.NativeFiles (nativeImage dispatcher) (nativeSymbols dispatcher)) (C.NativeFiles (nativeImage installer) (nativeSymbols installer)))
  let installation = I.stageImage4 installationSource (C.installerArtifact joined)
      dispatch = D.stageImage4 dispatchSource (C.dispatcherArtifact joined)
      composition = C.stageImage4 canonical joined
      inner name (J.Object fields) = required ContractRejected (KM.lookup name fields)
      inner _ _ = Left ContractRejected
  installerMetadata <- inner "kernel_install" (I.candidateContractValue installerCandidate)
  dispatcherMetadata <- inner "startup_dispatch" (D.candidateContractValue dispatcherCandidate)
  joinedMetadata <- inner "composition" (C.candidateContractValue joinedCandidate)
  require (installerMetadata == I.contractValue installation && dispatcherMetadata == D.contractValue dispatch
    && joinedMetadata == C.contractValue composition) ContractRejected
  require (hash (producerRun producer) == Digest "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
    && hash (producerRom producer) == Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
    && not (B.null (producerExecutable producer))) IdentityRejected
  require (nativeContract installer == L.toStrict (J.encode (I.candidateContractValue installerCandidate))
    && nativeContract dispatcher == L.toStrict (J.encode (D.candidateContractValue dispatcherCandidate))
    && producerJoinedContract producer == L.toStrict (J.encode (C.candidateContractValue joinedCandidate))
    && producerInstallerDat producer == I.candidateBytes installerCandidate
    && producerJoinedDat producer == C.candidateBytes joinedCandidate) ContractRejected
  pure (Inputs model installer dispatcher (StandaloneProducer producer)
    (I.stagedBytes (I.stageImage4 installationSource (C.installerArtifact joined)))
    (C.stagedBytes (C.stageImage4 canonical joined)) (C.joinedProof joined) (I.installationProof (C.installerArtifact joined)) CanonicalLoaderProvider)

admitLoaderJoinedInputs :: S.SourceFiles -> C.NativeFiles -> C.NativeFiles -> C.NativeFiles -> C.NativeFiles
  -> LoaderProducerFiles -> LS.LoaderStagedCandidate -> Either WitnessError Inputs
admitLoaderJoinedInputs sources loaderA loaderB dispatcher installer producer candidate = do
  model <- either (const (Left SourceRejected)) Right (S.deriveSource sources)
  canonical <- either (const (Left SourceRejected)) Right
    (LS.canonicalSource (C.SourceFiles (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources)))
  artifact <- either (const (Left ArtifactRejected)) Right (LS.deriveArtifact loaderA loaderB dispatcher installer)
  installationSource <- either (const (Left SourceRejected)) Right
    (I.canonicalSource (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources))
  let manifest = loaderProducerContract producer
      composition = LS.dispatcherInstallerArtifact artifact
      native files = NativeFiles (C.nativeBytes files) (C.nativeSymbols files) manifest
      inner (J.Object fields) = required ContractRejected (KM.lookup "composition" fields)
      inner _ = Left ContractRejected
  recorded <- inner (LS.candidateContractValue candidate)
  require (recorded == LS.contractValue canonical artifact
    && manifest == L.toStrict (J.encode (LS.candidateContractValue candidate))
    && loaderProducerDat producer == LS.candidateBytes candidate) ContractRejected
  require (hash (loaderProducerRun producer) == Digest "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
    && hash (loaderProducerRom producer) == Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
    && not (B.null (loaderProducerExecutable producer))) IdentityRejected
  pure (Inputs model (native installer) (native dispatcher) (LoaderJoinedProducer producer)
    (I.stagedBytes (I.stageImage4 installationSource (C.installerArtifact composition)))
    (LS.candidateImage4 candidate) (C.joinedProof composition) (I.installationProof (C.installerArtifact composition))
    (JoinedLoaderProvider artifact candidate))

boundExecutable :: BoundProducer -> B.ByteString
boundExecutable (StandaloneProducer producer) = producerExecutable producer
boundExecutable (LoaderJoinedProducer producer) = loaderProducerExecutable producer
boundRun :: BoundProducer -> B.ByteString
boundRun (StandaloneProducer producer) = producerRun producer
boundRun (LoaderJoinedProducer producer) = loaderProducerRun producer
boundRom :: BoundProducer -> B.ByteString
boundRom (StandaloneProducer producer) = producerRom producer
boundRom (LoaderJoinedProducer producer) = loaderProducerRom producer
boundContract :: BoundProducer -> B.ByteString
boundContract (StandaloneProducer producer) = producerJoinedContract producer
boundContract (LoaderJoinedProducer producer) = loaderProducerContract producer

roleUpdate :: Inputs -> Role -> Either WitnessError B.ByteString
roleUpdate inputs Retained = Right (S.sourceDat (S.sourceFiles (inputSource inputs)))
roleUpdate inputs role = case (inputProducer inputs,role) of
  (StandaloneProducer producer,Installer) -> Right (producerInstallerDat producer)
  (StandaloneProducer producer,Joined) -> Right (producerJoinedDat producer)
  (LoaderJoinedProducer producer,Joined) -> Right (loaderProducerDat producer)
  _ -> Left IdentityRejected

joinedLoaderProvider :: Inputs -> Maybe LS.LoaderJoinedArtifact
joinedLoaderProvider inputs = case inputLoaders inputs of
  CanonicalLoaderProvider -> Nothing
  JoinedLoaderProvider artifact _ -> Just artifact

admitReceipt :: Inputs -> ObservedFiles -> Either WitnessError QualifiedReceipt
admitReceipt inputs files = do
  receipt <- either (const (Left DecodeRejected)) Right (J.eitherDecodeStrict' (observedReceipt files))
  require (hash (observedCheckpoint files) == receiptCheckpoint receipt && not (B.null (observedCheckpoint files))
    && not (receiptCheckpointReused receipt)) CheckpointRejected
  projection <- validateReceipt inputs receipt
  let coverage = [ObservationCoverage item (state item) | item <- receiptCensus receipt]
      state item
        | any (\invocation -> invocationFamily invocation == censusFamily item && invocationCore invocation == censusCore item
            && invocationStatus invocation == Returned) (receiptInvocations receipt) = SelectedInvocationReturned
        | censusEntries item == Count 0 = NoEntryObserved
        | otherwise = CensusEntryOnly
  pure (QualifiedReceipt (receiptRole receipt) (receiptIdentity receipt) (hash (observedReceipt files)) (receiptCheckpoint receipt) coverage projection)

validateReceipt :: Inputs -> Receipt -> Either WitnessError Projection
validateReceipt = validateReceiptWithPolicy Policy.standalonePolicy

validateReceiptWithPolicy :: Policy.ObservationPolicy -> Inputs -> Receipt -> Either WitnessError Projection
validateReceiptWithPolicy policy inputs receipt = do
  require (receiptSchema receipt == 1) DecodeRejected
  checkIdentity inputs receipt
  checkWatchFailures receipt
  checkCensus receipt
  require (receiptOutcome receipt == Observed && receiptError receipt == Nothing && receiptFailure receipt == Nothing) StatusRejected
  checkLimits receipt
  checkProbes inputs receipt
  checkSegmentsWithPolicy policy receipt
  checkSourcePublications inputs receipt
  checkResources receipt
  case inputProducer inputs of
    StandaloneProducer _ -> pure ()
    LoaderJoinedProducer _ -> checkJoinedPreimages inputs receipt
  outer <- checkOuter inputs receipt
  checkInstallation inputs receipt
  checkCompletion inputs receipt
  checkFetchPoliciesWithPolicy policy receipt
  consumers <- checkConsumers (inputSource inputs) receipt
  pure (Projection outer consumers)

checkJoinedPreimages :: Inputs -> Receipt -> Either WitnessError ()
checkJoinedPreimages inputs receipt = do
  outer <- probeAt receipt OuterEntry
  forM_ [(0x61c008,4),(0x7caf3c,4),(0x40000,32),(0xd0000,32)] $ \(location,size) -> do
    resource <- one EffectRejected [value | value <- receiptResources receipt,resourceKind value == Header
      ,resourcePhysical value == Address location,resourceSize value == Count size]
    require (clock (resourceAdmitted resource) == probeClock outer && resourceLifetime resource == ObservationLifetime) EffectRejected
    checkViews resource (resourceInitial resource) >> pure ()
  committed <- memoryAt receipt outer
  forM_ [(0x61c008,0),(0x7caf3c,3)] $ \(location,expected) ->
    wordAt EffectRejected committed location >>= \actual -> require (actual == expected) EffectRejected
  let sources = S.sourceFiles (inputSource inputs)
  forM_ [(0x40000,S.sourceLoaderA sources),(0xd0000,S.sourceLoaderB sources)] $ \(base,bytes) ->
    forM_ [0,4..28] $ \offset ->
      wordAt EffectRejected committed (base+fromIntegral offset) >>= \actual ->
        require (actual == U.word32At bytes offset) EffectRejected

checkIdentity :: Inputs -> Receipt -> Either WitnessError ()
checkIdentity inputs receipt = do
  let identity = receiptIdentity receipt
      sources = S.sourceFiles (inputSource inputs)
      producer = inputProducer inputs
      native kind files = Component kind (hash (nativeImage files)) (hash (nativeSymbols files)) (hash (nativeContract files))
  update <- roleUpdate inputs (receiptRole receipt)
  require (identityRun identity == hash (boundRun producer) && identityRom identity == hash (boundRom producer)
    && identityUpdate identity == hash update && identityCanonical identity == hash (S.sourceDat sources)
    && identityImage4 identity == hash (S.sourceImage4 sources) && identityLoaderA identity == hash (S.sourceLoaderA sources)
    && identityLoaderB identity == hash (S.sourceLoaderB sources) && identityObserver identity == hash (boundExecutable producer)
    && identityJoinedContract identity == hash (boundContract producer)
    && sortOn componentKind (identityComponents identity) == sortOn componentKind [native InstallerComponent (inputInstaller inputs),native DispatcherComponent (inputDispatcher inputs)]) IdentityRejected

checkLimits :: Receipt -> Either WitnessError ()
checkLimits receipt = do
  let limits = receiptLimits receipt
      policy = Limits (Count 80000000) (Count 256) (Count 4096) (Count 65536) (Count 131072) (Count 512) (Count 4194304)
        (Count 64) (Count 1048576) (Count 134217728) (Count 32768) (Count 65536)
      views = concatMap (\resource -> resourceInitial resource ++ resourceFinal resource) (receiptResources receipt)
        ++ concatMap (concatMap snapshotViews . probeSnapshots) (receiptProbes receipt)
      snapshotBytes = sum [toInteger (B.length bytes) | view <- views,let K.Bytes bytes = viewBytes view]
      snapshotSets = 2*length (receiptResources receipt)+sum (map (length . probeSnapshots) (receiptProbes receipt))
  require (limits == policy && count (receiptInstructions receipt) <= count (limitInstructions limits)
    && sum (map (toInteger . length . K.watchRecords . segmentWatch) (receiptSegments receipt)) <= toInteger (count (limitTotalRecords limits))
    && snapshotBytes <= toInteger (count (limitSnapshotBytes limits))
    && toInteger snapshotSets <= toInteger (count (limitSnapshots limits))
    && fromIntegral (length (receiptSegments receipt)) <= count (limitSegments limits)
    && fromIntegral (length (receiptProbes receipt)) <= count (limitProbes limits)
    && fromIntegral (length (receiptResources receipt)) <= count (limitResources limits)
    && all ((<= count (limitResourceBytes limits)) . count . resourceSize) (receiptResources receipt)
    && fromIntegral (length (receiptInvocations receipt)) <= count (limitInvocations limits)) LimitsRejected

checkCensus :: Receipt -> Either WitnessError ()
checkCensus receipt = do
  let census = receiptCensus receipt
      invocations = receiptInvocations receipt
      families = [minBound..maxBound]
  outer <- probeAt receipt OuterEntry
  executed <- executedFetchesAt (map S.familyEntryAddress families) receipt
  require (sort [(censusFamily item,censusCore item) | item <- census] == [(family,which) | family <- families,which <- [Primary,Secondary]]) CensusRejected
  forM_ families $ \family -> do
    let entered = [invocation | invocation <- invocations,invocationFamily invocation == family,invocationStatus invocation /= NotObserved]
    require (not (null entered)) (MissingFamily family)
    require (all (\invocation -> invocationStatus invocation == Returned && invocationReturn invocation /= Nothing) entered) (UnclosedFamily family)
    let occurrences = [occurrence | occurrence <- executed,eventPc (executedRow occurrence) == Just (S.familyEntryAddress family)]
        claimed = [probe | probe <- receiptProbes receipt,probeKind probe == FamilyEntry,probeFamily probe == Just family]
        matches probe occurrence =
          let event = executedRow occurrence
              before = lookup (core probe) (zip [Primary,Secondary] (K.contextCounts (machine probe)))
          in eventCore event == Just (core probe) && eventPc event == Just (K.contextPc (machine probe))
            && before == Just (executedCoreBefore occurrence)
            && probeClock probe >= probeClock outer
            && maybe False ((>probeClock probe) . count) (eventInstructions event)
    require (length claimed == length occurrences) CensusRejected
    entries <- forM occurrences $ \occurrence -> one CensusRejected [probe | probe <- claimed,matches probe occurrence]
    earliestEntered <- required CensusRejected (case entries of probe:_ -> Just probe; [] -> Nothing)
    require (any ((==Just (probeId earliestEntered)) . invocationEntry) entered) CensusRejected
    forM_ [Primary,Secondary] $ \which -> do
      item <- one CensusRejected [value | value <- census,censusFamily value == family,censusCore value == which]
      let onCore = filter ((==which) . core) entries
      require (count (censusEntries item) == fromIntegral (length onCore)) CensusRejected
      require (censusFirst item == case onCore of probe:_ -> Just (probeId probe); [] -> Nothing) CensusRejected

sourceWord :: Inputs -> Role -> Word32 -> Either WitnessError Word32
sourceWord inputs role guest = do
  let site = physical guest
      sources = S.sourceFiles (inputSource inputs)
      image = case role of Retained -> S.sourceImage4 sources; Installer -> inputInstallerImage inputs; Joined -> inputJoinedImage inputs
      loader side = case (inputLoaders inputs,role) of
        (JoinedLoaderProvider _ candidate,Joined) -> LS.candidateLoader candidate side
        _ -> case side of LS.LoaderA -> S.sourceLoaderA sources; LS.LoaderB -> S.sourceLoaderB sources
      candidates = [(0x40000,loader LS.LoaderA),(0xd0000,loader LS.LoaderB),(0x61c400,image)]
  (base,bytes) <- one SourceWordRejected [(base,bytes) | (base,bytes) <- candidates,site >= base,toInteger site+4 <= toInteger base+toInteger (B.length bytes)]
  pure (U.word32At bytes (fromIntegral (site-base)))

checkProbes :: Inputs -> Receipt -> Either WitnessError ()
checkProbes inputs receipt = do
  let probes = receiptProbes receipt
  require (not (null probes) && unique (map probeId probes) && map probeId probes == sort (map probeId probes)
    && map probeClock probes == sort (map probeClock probes)) ProbeRejected
  forM_ probes $ \probe -> do
    checkContext (probeContext probe)
    expected <- sourceWord inputs (receiptRole receipt) (pc probe)
    require (probeSourceWord probe == WordValue expected) SourceWordRejected
  initial <- required ProbeRejected (case probes of value:_ -> Just value; [] -> Nothing)
  require (all (\probe -> K.contextTcm (machine initial) == K.contextTcm (machine probe)
    && K.contextIdentities (machine initial) == K.contextIdentities (machine probe)) probes) ContextRejected
  forM_ [resource | resource <- receiptResources receipt,resourceKind resource == Code] $ \resource -> do
    let base = address (resourcePhysical resource); size = count (resourceSize resource)
    require (base `mod` 4 == 0 && size `mod` 4 == 0) SourceWordRejected
    actual <- checkViews resource (resourceInitial resource)
    forM_ [0,4..size-4] $ \offset -> do
      expected <- sourceWord inputs (receiptRole receipt) (base+fromIntegral offset)
      observed <- wordAt SourceWordRejected actual (base+fromIntegral offset)
      require (observed == expected) SourceWordRejected
  forM_ [OuterEntry,OuterReturn,InstallerEntry,InstallerReturn,CacheEntry,CacheReturn,CompletionStore,CompletionBeforeDsb,CompletionAfterDsb
    ,PeerPoll,PeerPostload,PeerBranch,InitializerCall,InitializerEntry,InitializerReturn,CriticalEntry,CriticalIdentityCall,CriticalIdentityReturn
    ,CriticalRootLoad,CriticalSlotLoad,CriticalCall,CriticalReturn] $ \kind -> probeAt receipt kind >> pure ()
  checkFetchedCode inputs receipt

checkFetchedCode :: Inputs -> Receipt -> Either WitnessError ()
checkFetchedCode inputs receipt = do
  outer <- probeAt receipt OuterEntry
  let sites = Set.fromList [physical (address site) | event <- allFetchedEvents receipt,Just site <- [eventPc event]]
      resources = [resource | resource <- receiptResources receipt,resourceKind resource == Carrier]
      sources = S.sourceFiles (inputSource inputs)
      expectedShapes = [(base+fromIntegral offset,fromIntegral (min 1048576 (size-offset)))
        | (base,size) <- [(0x40000,B.length (S.sourceLoaderA sources)),(0xd0000,B.length (S.sourceLoaderB sources)),(0x61c400,B.length (S.sourceImage4 sources))]
        ,offset <- [0,1048576..size-1]]
  require (sort [(address (resourcePhysical resource),count (resourceSize resource)) | resource <- resources] == sort expectedShapes) SourceWordRejected
  carriers <- mapM (\resource -> checkViews resource (resourceInitial resource) >>= \memory -> pure (resource,memory)) resources
  forM_ (Set.toList sites) $ \site -> do
    require (site `mod` 4 == 0) FetchRejected
    expected <- sourceWord inputs (receiptRole receipt) site
    (carrier,initial) <- one SourceWordRejected [(resource,memory) | (resource,memory) <- carriers,address (resourcePhysical resource) <= site
      ,toInteger site+4 <= toInteger (address (resourcePhysical resource))+toInteger (count (resourceSize resource))]
    require (clock (resourceAdmitted carrier) == probeClock outer && resourceLifetime carrier == ObservationLifetime
      && resourceOwner carrier == Nothing) AdmissionRejected
    observed <- wordAt SourceWordRejected initial site
    require (observed == expected) SourceWordRejected
  forM_ [event | event <- allEvents receipt,isWrite event] $ \event -> do
    backing <- eventPhysical32 event
    require (case Set.lookupGE (backing .&. 0xfffffffc) sites of
      Just site -> fromIntegral site >= fromIntegral backing+eventSize event
      Nothing -> True) WriterRejected

loaderAddress :: LI.CodeAddress -> Address
loaderAddress (LI.CodeAddress value) = Address value

outerContinuation :: Inputs -> Role -> Either WitnessError Address
outerContinuation inputs role = case (inputLoaders inputs,role) of
  (JoinedLoaderProvider artifact _,Joined) ->
    one FrameRejected [loaderAddress (LS.boundaryContinuation boundary)
      | boundary <- LS.loaderBoundaries (LS.joinedProof artifact) LS.LoaderA
      ,LS.boundaryScenario boundary == LI.Scenario LI.Primary LI.InitiallyPublished LI.SetupReturns
      ,LS.boundaryPhase boundary == LI.Setup]
  _ -> Right (Address 0x40a00)

data InstallationFrame = InstallationFrame Address Address
installationFrame :: Inputs -> Role -> Address -> Address -> Either WitnessError InstallationFrame
installationFrame inputs Joined (Address incoming) (Address observedLink) = do
  boundary <- one FrameRejected [value | value <- C.installationBoundaries (inputJoinedProof inputs)
    ,C.codeAddressValue (C.boundaryContinuation value) == observedLink
    ,C.codeAddressValue (C.boundaryInstruction value)+4 == observedLink]
  pure (InstallationFrame (Address (incoming+fromIntegral (C.stackOffsetValue (C.boundaryStack boundary))))
    (Address (C.codeAddressValue (C.boundaryContinuation boundary))))
installationFrame _ _ (Address incoming) _ = Right (InstallationFrame (Address (incoming-48)) (Address 0x69ce38))

data CompletionSites = CompletionSites
  {siteCompletionStore :: Address,siteCompletionBeforeDsb :: Address,siteCompletionAfterDsb :: Address
  ,sitePeerPoll :: Address,sitePeerPostload :: Address,sitePeerBranch :: Address,sitePeerDestination :: A.Reg
  ,siteInitializerCall :: Address,siteInitializerEntry :: Address,siteInitializerReturn :: Address}

completionSites :: Inputs -> Role -> CompletionSites
completionSites inputs role = case (inputLoaders inputs,role) of
  (JoinedLoaderProvider artifact _,Joined) ->
    let primary = LS.loaderSites artifact LS.LoaderA
        secondary = LS.loaderSites artifact LS.LoaderB
    in CompletionSites (loaderAddress (LS.completionStore primary)) (loaderAddress (LS.completionBeforeDsb primary))
      (loaderAddress (LS.completionAfterDsb primary)) (loaderAddress (LS.peerPoll secondary))
      (loaderAddress (LS.peerPollPostload secondary)) (loaderAddress (LS.peerPollBranch secondary))
      (LS.peerPollDestination secondary) (loaderAddress (LS.initializerCall secondary))
      (loaderAddress (LS.initializerEntry secondary)) (loaderAddress (LS.initializerReturn secondary))
  _ -> CompletionSites (Address 0x40a08) (Address 0x40a0c) (Address 0x40a10)
    (Address 0xd0a44) (Address 0xd0a48) (Address 0xd0a4c) (A.Reg 3)
    (Address 0xd0aac) (Address 0xd0430) (Address 0xd0ab0)

checkOuter :: Inputs -> Receipt -> Either WitnessError OuterProjection
checkOuter inputs receipt = do
  entry <- probeAt receipt OuterEntry
  returned <- probeAt receipt OuterReturn
  installation <- probeAt receipt InstallerEntry
  installed <- probeAt receipt InstallerReturn
  cache <- probeAt receipt CacheEntry
  cached <- probeAt receipt CacheReturn
  phase <- register entry 0
  encodedReturn <- outerContinuation inputs (receiptRole receipt)
  require (core entry == Primary && pc entry == 0x69cdf8 && phase == 0 && sp entry == 0x40063450 && lr entry == address encodedReturn
    && pc returned == address encodedReturn && sp returned == sp entry && core returned == Primary
    && take 8 (drop 4 (K.contextRegisters (machine returned))) == take 8 (drop 4 (K.contextRegisters (machine entry)))
    && probeId entry < probeId installation && probeId installed < probeId returned) FrameRejected
  InstallationFrame expectedStack continuation <- installationFrame inputs (receiptRole receipt) (K.contextSp (machine entry)) (K.contextLr (machine installation))
  let minimumStack = case receiptRole receipt of
        Joined -> fromIntegral (C.stackOffsetValue (C.joinedLow (inputJoinedProof inputs)))
        Retained -> -56
        Installer -> -48 + fromIntegral (IA.scratchLow (inputInstallerProof inputs))
  require (core installation == Primary && pc installation == 0x725a04 && sp installation == address expectedStack
    && lr installation == address continuation && pc installed == address continuation && sp installed == address expectedStack) FrameRejected
  before <- memoryAt receipt entry
  after <- memoryAt receipt returned
  callerBefore <- bytesAt FrameRejected before (physical (sp entry)) 16
  callerAfter <- bytesAt FrameRejected after (physical (sp entry)) 16
  savedMemory <- memoryAt receipt installation
  savedFrame <- bytesAt FrameRejected savedMemory (physical (sp entry-16)) 16
  afterFrame <- bytesAt FrameRejected after (physical (sp entry-16)) 16
  savedRegisters <- mapM (register entry) [4,5,11]
  require (callerBefore == callerAfter && savedFrame == B.concat (map wordBytes (savedRegisters ++ [lr entry]))
    && afterFrame == savedFrame) FrameRejected
  stack <- one FrameRejected [resource | resource <- receiptResources receipt,resourceKind resource == Stack
    ,clock (resourceAdmitted resource) <= probeClock entry
    ,fromIntegral (address (resourcePhysical resource)) <= (fromIntegral (physical (sp entry)) + minimumStack :: Integer)
    ,fromIntegral (address (resourcePhysical resource)) + toInteger (count (resourceSize resource)) >= toInteger (physical (sp entry)+16)]
  require (resourceLifetime stack == ObservationLifetime) FrameRejected
  forM_ (filter isWrite (allEvents receipt)) $ \event -> do
    at <- eventClock event
    backing <- eventPhysical32 event
    require (not (at > probeClock installation && at <= probeClock installed
      && backing < physical (sp entry)
      && fromIntegral backing+eventSize event > fromIntegral (physical (sp entry-16)))) FrameRejected
  require (pc cache == 0x42634 && lr cache == pc cached && pc cached == lr cache && sp cache == sp cached
    && probeId installation < probeId cache && probeId cache < probeId cached && probeId cached < probeId installed) CallRejected
  cacheArgs <- mapM (register cache) [0,1]
  require (cacheArgs == [0x96bd20,32]) CallRejected
  outerResult <- register returned 0
  require (outerResult == 0) FrameRejected
  let caller probe frame = CallerState (K.contextSp (machine probe)) (take 8 (drop 4 (K.contextRegisters (machine probe))))
        (WordValue (number (K.contextCpsr (machine probe)) .&. 0x0fffffff)) (K.Bytes frame)
  boundary <- forM [resource | resource <- receiptResources receipt,resourceOwner resource == Nothing
    ,clock (resourceAdmitted resource) == probeClock entry,resourceKind resource `notElem` [Stack,Code,Carrier]] $ \resource -> do
      initial <- bytesAt EffectRejected before (address (resourcePhysical resource)) (count (resourceSize resource))
      committed <- bytesAt EffectRejected after (address (resourcePhysical resource)) (count (resourceSize resource))
      pure (BoundaryMemory (resourceKind resource) (resourcePhysical resource) (resourceSize resource) (K.Bytes initial) (K.Bytes committed))
  pure (OuterProjection (caller entry callerBefore) (caller returned callerAfter) (K.Bytes savedFrame) boundary)

installationStores :: [S.Store]
installationStores = map (\(whereAt,value) -> S.Store (Address whereAt) (WordValue value))
  ([(0x96bd20,0x4004dae0),(0x96bd24,0x400ddaa0)]
    ++ [(0x40a0f870+4*index,0) | index <- [0..119]]
    ++ [(0x40a0fc30,120),(0x40a0fc34,120),(0x40a0f86c,250),(0x40a0fc38,120),(0x40a0fc3c,1),(0x40a0fc40,122),(0x409c3500,0x7531)]
    ++ [(base+4*index,0) | index <- [1..64],base <- [0x409c3600,0x409c3800]]
    ++ [(base+4*index,0) | (base,wordsCount) <- [(0x40a0fc44,360),(0x409c3904,720),(0x40a0fa50,120)],index <- [0..wordsCount-1]])

data NormalStartupMode = NormalStartupMode

directCallTarget :: Address -> WordValue -> Maybe Address
directCallTarget (Address site) (WordValue instruction)
  | instruction .&. 0xff000000 /= 0xeb000000 = Nothing
  | otherwise =
      let offset = (instruction .&. 0x00ffffff) `shiftL` 2
          displacement = if offset .&. 0x02000000 /= 0 then offset .|. 0xfc000000 else offset
      in Just (Address (site+8+displacement))

observedStartupMode :: Inputs -> Receipt -> Probe -> Either WitnessError NormalStartupMode
observedStartupMode inputs receipt installation = do
  outer <- probeAt receipt OuterEntry
  beginOwn <- coreCounter Primary (K.contextCounts (machine outer))
  endOwn <- coreCounter Primary (K.contextCounts (machine installation))
  let getterEntry = Address 0x657184
      loadSite = Address 0x657194
      moveSite = Address 0x657198
      returnSite = Address 0x65719c
      modeCell = Address 0x61c008
      prefix = [segment | segment <- receiptSegments receipt,segmentFetchScope segment == CompleteFetch]
      within event = eventCore event == Just Primary && maybe False
        (\at -> count at > probeClock outer && count at <= probeClock installation) (eventInstructions event)
      own fetched = count (executedCoreBefore fetched)+1
      site fetched = eventPc (executedRow fetched)
  forM_ [event | segment <- prefix,event <- K.watchRecords (segmentWatch segment),isWrite event] $ \event -> do
    backing <- eventPhysical32 event
    require (backing >= address modeCell+4
      || fromIntegral backing+eventSize event <= fromIntegral (address modeCell)) WriterRejected
  (segment,row,readMode) <- one EffectRejected [(segment,Count index,event)
    | segment <- prefix,(index,event) <- zip [0..] (K.watchRecords (segmentWatch segment))
    ,eventSource event == CpuRead,eventPc event == Just loadSite,within event]
  evidence <- one EffectRejected [item | item <- segmentCpuEvidence segment,evidenceRow item == row]
  latest <- required EffectRejected (evidenceLatestFetch evidence)
  fetched <- executedFetchesAt ([Address location | location <- [0x69cdf8,0x69cdfc..0x69cf80]]
    ++ [getterEntry,loadSite,moveSite,returnSite]) receipt
  let executed = filter (within . executedRow) fetched
      at destination = one EffectRejected [item | item <- executed,site item == Just destination]
  candidates <- forM [item | item <- executed,Just destination <- [site item]
    ,destination >= Address 0x69cdf8,destination < Address 0x69cf84] $ \item -> do
      destination <- required EffectRejected (site item)
      instruction <- sourceWord inputs (receiptRole receipt) (address destination)
      pure [item | directCallTarget destination (WordValue instruction) == Just getterEntry]
  called <- one EffectRejected (concat candidates)
  getter <- at getterEntry
  loaded <- at loadSite
  moved <- at moveSite
  returned <- at returnSite
  callSite <- required EffectRejected (site called)
  continued <- at (Address (address callSite+4))
  installed <- at (Address (lr installation-4))
  installInstruction <- sourceWord inputs (receiptRole receipt) (lr installation-4)
  loadInstruction <- sourceWord inputs (receiptRole receipt) (address loadSite)
  moveInstruction <- sourceWord inputs (receiptRole receipt) (address moveSite)
  returnInstruction <- sourceWord inputs (receiptRole receipt) (address returnSite)
  require ([loadInstruction,moveInstruction,returnInstruction] == [0xe5933008,0xe1a00003,0xe8bd8800]) SourceWordRejected
  argument <- register installation 0
  require (eventAddress readMode == fromIntegral (address modeCell)
    && eventPhysical readMode == Just (fromIntegral (address modeCell)) && eventSize readMode == 4
    && eventByteOrder readMode == Just K.Little && eventBefore readMode == Just (K.Bytes (wordBytes 0))
    && eventValue readMode == Just 0 && argument == 0
    && count (evidenceCoreInstructions evidence) > count beginOwn
    && count (evidenceCoreInstructions evidence) < count endOwn
    && eventInstructions readMode == eventInstructions (executedRow loaded)
    && eventTick readMode == eventTick (executedRow loaded)
    && attributedFetchEvent latest == executedRow loaded
    && evidenceCoreInstructions evidence == attributedFetchCoreInstructions latest
    && count (evidenceCoreInstructions evidence) == own loaded
    && own getter == own called+1 && own getter < own loaded
    && own moved == own loaded+1 && own returned == own moved+1
    && own continued == own returned+1 && own continued < own installed
    && own installed == count endOwn
    && directCallTarget (Address (lr installation-4)) (WordValue installInstruction) == Just (Address (pc installation))) EffectRejected
  pure NormalStartupMode

checkInstallation :: Inputs -> Receipt -> Either WitnessError ()
checkInstallation inputs receipt = do
  entry <- probeAt receipt InstallerEntry
  returned <- probeAt receipt InstallerReturn
  cache <- probeAt receipt CacheEntry
  cached <- probeAt receipt CacheReturn
  before <- memoryAt receipt entry
  _ <- observedStartupMode inputs receipt entry
  forM_ [(0x40010,0x4004dae0),(0xd0010,0x400ddaa0),(0x7caf3c,3)] $ \(location,value) ->
    wordAt EffectRejected before location >>= \actual -> require (actual == value) EffectRejected
  let isRetained = receiptRole receipt == Retained
      bottom = physical (sp entry) + fromIntegral (if isRetained then -8 else IA.scratchLow (inputInstallerProof inputs))
      top = physical (sp entry)
      native = nativeImage (inputInstaller inputs)
      cacheReturns = [site+4 | site <- IA.executionPcs (inputInstallerProof inputs),site >= 0x725a04
        ,toInteger site+4 <= 0x725a04+toInteger (B.length native)
        ,let instruction = U.word32At native (fromIntegral (site-0x725a04))
        ,directCallTarget (Address site) (WordValue instruction) == Just (Address 0x42634)]
  cacheReturn <- if isRetained then Right 0x725a40 else one CallRejected cacheReturns
  require (lr cache == cacheReturn && pc cached == cacheReturn && lr returned == 0x725a40
    && take 4 (K.contextRegisters (machine returned)) == map WordValue [0,0x40a0fc2c,0,0x40a0fc2c]
    && drop 4 (K.contextRegisters (machine returned)) == drop 4 (K.contextRegisters (machine entry))
    && number (K.contextCpsr (machine returned)) `shiftR` 28 == 6
    && number (K.contextCpsr (machine returned)) .&. 0x0fffffff == number (K.contextCpsr (machine entry)) .&. 0x0fffffff) EffectRejected
  require (take 4 (K.contextRegisters (machine cached)) == map WordValue [0x96bd40,0x96bd40,1,0]
    && drop 4 (K.contextRegisters (machine cached)) == drop 4 (K.contextRegisters (machine cache))
    && number (K.contextCpsr (machine cached)) `shiftR` 28 == 6) CallRejected
  stores <- foldM (\found event -> do
    at <- eventClock event
    backing <- eventPhysical32 event
    pure (if at > probeClock entry && at <= probeClock returned && (backing < bottom || backing >= top)
      then event:found else found)) [] [event | event <- allEvents receipt,eventSource event == CpuWrite,eventCore event == Just Primary]
  require ([(eventAddress event,eventSize event,eventValue event) | event <- reverse stores]
    == [(fromIntegral (address (S.storeAddress store)),4,Just (fromIntegral (number (S.storeValue store)))) | store <- installationStores]) EffectRejected
  committed <- memoryAt receipt returned
  forM_ installationStores $ \store -> wordAt EffectRejected committed (address (S.storeAddress store)) >>= \actual ->
    require (actual == number (S.storeValue store)) EffectRejected

checkCompletion :: Inputs -> Receipt -> Either WitnessError ()
checkCompletion inputs receipt = do
  installed <- probeAt receipt InstallerReturn
  outer <- probeAt receipt OuterReturn
  stored <- probeAt receipt CompletionStore
  beforeDsb <- probeAt receipt CompletionBeforeDsb
  afterDsb <- probeAt receipt CompletionAfterDsb
  poll <- probeAt receipt PeerPoll
  loaded <- probeAt receipt PeerPostload
  branch <- probeAt receipt PeerBranch
  called <- probeAt receipt InitializerCall
  initializer <- probeAt receipt InitializerEntry
  initialized <- probeAt receipt InitializerReturn
  let sites = completionSites inputs (receiptRole receipt)
  require (map pc [stored,beforeDsb,afterDsb,poll,loaded,branch,called,initializer,initialized]
    == map address [siteCompletionStore sites,siteCompletionBeforeDsb sites,siteCompletionAfterDsb sites
      ,sitePeerPoll sites,sitePeerPostload sites,sitePeerBranch sites,siteInitializerCall sites,siteInitializerEntry sites,siteInitializerReturn sites]
    && all ((== Primary) . core) [stored,beforeDsb,afterDsb]
    && all ((== Secondary) . core) [poll,loaded,branch,called,initializer,initialized]
    && probeId installed < probeId outer && probeId outer < probeId stored && probeId stored < probeId beforeDsb && probeId beforeDsb < probeId afterDsb
    && probeId poll < probeId loaded && probeId loaded < probeId branch && probeId branch < probeId called
    && probeId called < probeId initializer && probeId initializer < probeId initialized) CompletionRejected
  let A.Reg pollRegister = sitePeerDestination sites
      initializerLink = address (siteInitializerReturn sites)
  nonzero <- register loaded (fromIntegral pollRegister)
  require (nonzero /= 0 && sp initializer == 0x400f3410 && lr initializer == initializerLink
    && sp initialized == sp initializer && lr initialized == initializerLink
    && take 8 (drop 4 (K.contextRegisters (machine initializer))) == take 8 (drop 4 (K.contextRegisters (machine initialized)))) InitializerRejected
  forM_ [beforeDsb,afterDsb] $ \probe -> do
    contents <- memoryAt receipt probe
    value <- wordAt CompletionRejected contents 0x400a00c4
    require (value == 1) CompletionRejected
  require (any (\event -> eventSource event == CpuWrite && eventCore event == Just Primary && eventPc event == Just (siteCompletionStore sites)
    && eventAddress event == 0x400a00c4 && eventSize event == 4 && eventValue event == Just 1) (allEvents receipt)) CompletionRejected
  require (any (\event -> eventSource event == CpuRead && eventCore event == Just Secondary && eventPc event == Just (sitePeerPoll sites)
    && eventAddress event == 0x400a00c4 && eventValue event == Just (fromIntegral nonzero)) (allEvents receipt)) CompletionRejected
  require (take 4 (K.contextRegisters (machine initialized)) == map WordValue [0x400ebe6c,0x400ed120,0x400f18f0,0x400def40]
    && drop 12 (K.contextRegisters (machine initialized)) == [WordValue 120]
    && number (K.contextCpsr (machine initialized)) == (number (K.contextCpsr (machine initializer)) .&. 0x0fffffff) .|. 0x60000000
    && K.contextTcm (machine initialized) == K.contextTcm (machine initializer)) InitializerRejected
  let model = inputSource inputs
  primaryEntry <- probeAt receipt OuterEntry
  primaryMemory <- memoryAt receipt primaryEntry
  forM_ (S.primaryStores model) $ \store -> wordAt InitializerRejected primaryMemory (address (S.storeAddress store)) >>= \actual ->
    require (actual == number (S.storeValue store)) InitializerRejected
  initializedMemory <- memoryAt receipt initialized
  forM_ (S.secondaryStores model) $ \store -> wordAt InitializerRejected initializedMemory (address (S.storeAddress store)) >>= \actual ->
    require (actual == number (S.storeValue store)) InitializerRejected
  before <- memoryAt receipt initializer
  forM_ [(0x96bd20,0x4004dae0),(0x96bd24,0x400ddaa0)] $ \(whereAt,value) -> do
    actual <- wordAt InitializerRejected before whereAt
    require (actual == value) InitializerRejected
  checkFirstConsumer model receipt initialized

data PublicationSpan = PublicationSpan !Address !S.ByteCount

publicationSpan :: [S.Store] -> Either WitnessError PublicationSpan
publicationSpan [] = Left SourceRejected
publicationSpan (first:rest) = Right (foldl' extend initial rest)
  where
    initial = PublicationSpan (Address (physical (address (S.storeAddress first)))) (S.ByteCount 4)
    extend (PublicationSpan (Address low) (S.ByteCount size)) store =
      let target = physical (address (S.storeAddress store))
          lower = min low target
          upper = max (low+size) (target+4)
      in PublicationSpan (Address lower) (S.ByteCount (upper-lower))

checkSourcePublications :: Inputs -> Receipt -> Either WitnessError ()
checkSourcePublications inputs receipt = do
  outer <- probeAt receipt OuterEntry
  initializer <- probeAt receipt InitializerEntry
  initialized <- probeAt receipt InitializerReturn
  let model = inputSource inputs
  forM_ [S.primaryStores model,S.secondaryStores model] $ \stores -> do
    PublicationSpan low (S.ByteCount size) <- publicationSpan stores
    resource <- one InitializerRejected [item | item <- receiptResources receipt,resourceKind item == RootObject
      ,resourcePhysical item == low,resourceSize item == Count (fromIntegral size)]
    end <- resourceEnd receipt resource
    let fullExtent views = length views == 4 && all
          (\view -> case viewBytes view of K.Bytes bytes -> B.length bytes == fromIntegral size) views
    require (resourceLifetime resource == ObservationLifetime && resourceOwner resource == Nothing
      && resourceAdmitted resource == probeContext outer && clock end == count (receiptInstructions receipt)
      && fullExtent (resourceInitial resource) && fullExtent (resourceFinal resource)) InitializerRejected
    checkResourceCoverage InitializerRejected receipt resource
  checkStores receipt Secondary (probeClock initializer) (probeClock initialized) (S.secondaryStores model)

checkStores :: Receipt -> Core -> Word64 -> Word64 -> [S.Store] -> Either WitnessError ()
checkStores receipt which begin end stores = do
  let addresses = Set.fromList (map (physical . address . S.storeAddress) stores)
  selected <- foldM (\found event -> do
    at <- eventClock event
    backing <- eventPhysical32 event
    pure (if at > begin && at <= end && eventSource event == CpuWrite && eventCore event == Just which && Set.member backing addresses
      then event:found else found)) [] [event | event <- allEvents receipt,eventSource event == CpuWrite]
  require ([(eventAddress event,eventSize event,eventValue event) | event <- reverse selected]
    == [(fromIntegral (address (S.storeAddress store)),4,Just (fromIntegral (number (S.storeValue store)))) | store <- stores]) InitializerRejected

selectedOffsets :: [Word32]
selectedOffsets = [0,0xc,0x80,0xa0,0xa4,0xac,0xb0,0xc8,0xe4,0xf8,0x148,0x164,0x168,0x170,0x1618,0x161c,0x1620,0x1624]

checkGlobalCall :: Receipt -> Probe -> Probe -> Address -> Either WitnessError (Probe,Probe)
checkGlobalCall receipt entry returned target = do
  selected <- one FirstConsumerRejected [probe | probe <- receiptProbes receipt,probeKind probe == DynamicTarget
    ,probeInvocation probe == Nothing,probeFamily probe == Nothing,core probe == core entry
    ,K.contextPc (machine probe) == target,lr probe == pc returned,sp probe == sp entry
    ,probeId entry < probeId probe,probeId probe < probeId returned]
  exited <- one FirstConsumerRejected [probe | probe <- receiptProbes receipt,probeKind probe == DynamicReturn
    ,probeInvocation probe == Nothing,probeFamily probe == Nothing,core probe == core entry
    ,pc probe == pc returned,sp probe == sp entry,probeId selected < probeId probe,probeId probe <= probeId returned]
  require (K.contextRegisters (machine selected) == K.contextRegisters (machine entry)
    && machine exited == machine returned
    && take 8 (drop 4 (K.contextRegisters (machine exited))) == take 8 (drop 4 (K.contextRegisters (machine selected)))) FirstConsumerRejected
  pure (selected,exited)


checkFirstConsumer :: S.SourceModel -> Receipt -> Probe -> Either WitnessError ()
checkFirstConsumer model receipt initialized = do
  initializer <- probeAt receipt InitializerEntry
  outer <- probeAt receipt OuterEntry
  installed <- probeAt receipt InstallerReturn
  let expectedSlots = [(address (S.storeAddress store),number (S.storeValue store)) | store <- S.secondaryStores model,address (S.storeAddress store)-0x400ddaa0 `elem` selectedOffsets]
      primarySlots = [physical (address (S.storeAddress store)) | store <- S.primaryStores model,address (S.storeAddress store)-0x4004dae0 `elem` selectedOffsets]
      protected = [(location,probeClock installed) | location <- [0x96bd20,0x96bd24]]
        ++ [(location,probeClock outer) | location <- primarySlots]
        ++ [(physical location,probeClock initialized) | (location,_) <- expectedSlots]
      rootReads = [event | event <- allEvents receipt,eventSource event `elem` [CpuRead,ModelBusRead]
        ,Just backing <- [eventPhysical event],backing < 0x96bd28,backing+eventSize event > 0x96bd24
        ,not (eventCore event == Just Secondary
          && maybe False (\site -> address site >= 0xd0430 && address site < 0xd09ac) (eventPc event)
          && maybe False (\(Count at) -> at > probeClock initializer && at <= probeClock initialized) (eventInstructions event))]
  firstRead <- required FirstConsumerRejected (case rootReads of event:_ -> Just event; [] -> Nothing)
  require (eventCore firstRead == Just Secondary && eventPc firstRead == Just (Address 0x7235fc)
    && eventValue firstRead == Just 0x400ddaa0) FirstConsumerRejected
  readClock <- eventClock firstRead
  require (readClock > probeClock initialized) FirstConsumerRejected
  identityCall <- probeAt receipt CriticalIdentityCall
  identityReturn <- probeAt receipt CriticalIdentityReturn
  rootLoad <- probeAt receipt CriticalRootLoad
  slotLoad <- probeAt receipt CriticalSlotLoad
  service <- probeAt receipt CriticalCall
  returned <- probeAt receipt CriticalReturn
  require (map pc [identityCall,identityReturn,rootLoad,slotLoad,service,returned]
    == [0x7235f4,0x7235f8,0x7235fc,0x723600,0x723604,0x723608]
    && probeId identityCall < probeId identityReturn && probeId identityReturn < probeId rootLoad
    && probeId rootLoad < probeId slotLoad && probeId slotLoad < probeId service && probeId service < probeId returned
    && all ((==Secondary) . core) [identityCall,identityReturn,rootLoad,slotLoad,service,returned]) FirstConsumerRejected
  identity <- register identityReturn 0
  target <- register service 3
  require (identity == 1 && target == 0xd1140 && readClock > probeClock rootLoad && readClock <= probeClock slotLoad) FirstConsumerRejected
  identityTarget <- register identityCall 3
  require (identityTarget == 0x412bc
    && number (K.contextCpsr (machine identityReturn)) .&. 0x07f0ffff == number (K.contextCpsr (machine identityCall)) .&. 0x07f0ffff) FirstConsumerRejected
  _ <- checkGlobalCall receipt identityCall identityReturn (Address 0x412bc)
  (_,irqReturn) <- checkGlobalCall receipt service returned (Address 0xd1140)
  priorMask <- register irqReturn 0
  require (priorMask == number (K.contextCpsr (machine service)) .&. 0x80
    && number (K.contextCpsr (machine irqReturn)) .&. 0x07f0ffff == (number (K.contextCpsr (machine service)) .|. 0x80) .&. 0x07f0ffff) FirstConsumerRejected
  atConsumer <- memoryAt receipt service
  forM_ expectedSlots $ \(location,value) -> wordAt FirstConsumerRejected atConsumer location >>= \actual -> require (actual == value) FirstConsumerRejected
  forM_ (filter isWrite (allEvents receipt)) $ \event -> do
    at <- eventClock event
    backing <- eventPhysical32 event
    require (not (any (\(start,begin) -> at > begin && backing < start+4
      && fromIntegral backing+eventSize event > fromIntegral start) protected)) WriterRejected

compareReceipts :: QualifiedReceipt -> QualifiedReceipt -> QualifiedReceipt -> Either WitnessError ()
compareReceipts (QualifiedReceipt retained retainedIdentity _ _ _ r) (QualifiedReceipt installer installerIdentity _ _ _ i) (QualifiedReceipt joined joinedIdentity _ _ _ j) = do
  let cohort identity = identity {identityUpdate = identityUpdate retainedIdentity,identityComponents = sortOn componentKind (identityComponents identity)}
  require ([retained,installer,joined] == [Retained,Installer,Joined]
    && cohort retainedIdentity == cohort installerIdentity && cohort retainedIdentity == cohort joinedIdentity) ComparisonRejected
  compareSemanticProjections r i
  compareSemanticProjections i j

qualifiedProjection :: QualifiedReceipt -> Projection
qualifiedProjection (QualifiedReceipt _ _ _ _ _ projection) = projection

compareSemanticProjections :: Projection -> Projection -> Either WitnessError ()
compareSemanticProjections before after = require (before == after) ComparisonRejected

compareLoaderSemanticProjections :: Projection -> Projection -> Either WitnessError ()
compareLoaderSemanticProjections
  (Projection (OuterProjection (CallerState beforeSp _ beforeControl beforeFrame) _ _ before) consumersBefore)
  (Projection (OuterProjection (CallerState afterSp _ afterControl afterFrame) _ _ after) consumersAfter) =
    require (beforeSp == afterSp && beforeControl == afterControl && beforeFrame == afterFrame
      && before == after && consumersBefore == consumersAfter) ComparisonRejected

qualificationValue :: QualifiedReceipt -> J.Value
qualificationValue (QualifiedReceipt role identity (Digest digest) (Digest checkpoint) coverage
  (Projection (OuterProjection entry returned (K.Bytes savedFrame) boundary) consumers)) = J.object
  ["schema" J..= (1 :: Int),"qualification" J..= ("source-admitted-startup-consumers" :: String)
  ,"role" J..= show role,"receipt_sha256" J..= digest,"checkpoint_sha256" J..= checkpoint
  ,"observer_sha256" J..= digestText (identityObserver identity)
  ,"outer_entry" J..= callerValue entry,"outer_return" J..= callerValue returned
  ,"outer_saved_frame" J..= B.unpack savedFrame
  ,"outer_memory" J..= [J.object ["kind" J..= show kind,"physical" J..= address location,"bytes" J..= count size
      ,"before_bytes" J..= B.length before,"committed_bytes" J..= B.length after]
      | BoundaryMemory kind location size (K.Bytes before) (K.Bytes after) <- boundary]
  ,"coverage" J..= [J.object ["family" J..= show (censusFamily item),"core" J..= show (censusCore item)
      ,"entries" J..= count (censusEntries item),"first_probe" J..= fmap (\(ProbeId ident) -> ident) (censusFirst item)
      ,"observation" J..= show state] | ObservationCoverage item state <- coverage]
  ,"comparison_scope" J..= ("outer-return-and-selected-operation-boundaries" :: String)
  ,"post_return_scheduler_equality" J..= False
  ,"consumers" J..= consumerProjectionValue consumers]
  where
    digestText (Digest value) = value
    callerValue (CallerState stack registers control (K.Bytes frame)) = J.object
      ["sp" J..= address stack,"callee_saved" J..= map number registers,"cpsr_control" J..= number control,"caller_frame" J..= B.unpack frame]
