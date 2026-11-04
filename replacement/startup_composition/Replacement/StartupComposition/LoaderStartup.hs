{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupComposition.LoaderStartup
  ( LoaderSource, LoaderJoinedArtifact, LoaderJoinedProof, LoaderBoundary, LoaderStagedCandidate
  , LoaderSide(..), LoaderJoinError(..), StackOffset, stackOffsetValue
  , canonicalSource, deriveArtifact, dispatcherInstallerArtifact, loaderArtifact, joinedProof
  , loaderBoundaries, boundarySide, boundaryScenario, boundaryPhase, boundaryDispatcherPhase
  , boundaryInstruction, boundaryContinuation, boundaryStack, boundaryFramePointer, boundaryLiveWords, boundaryOwnedLow
  , loaderFrameLow, dispatcherLow, installerLow, joinedLow, loaderCompilerEntry, loaderBodyEntry
  , LoaderSites, deriveSites, loaderSites, completionStore, completionBeforeDsb, completionAfterDsb
  , peerPoll, peerPollDestination, peerPollPostload, peerPollBranch, initializerCall, initializerEntry, initializerReturn
  , stageCandidate, candidateBytes, candidateImage4, candidateLoader, contractValue, candidateContractValue
  ) where

import qualified A32 as A
import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import Replacement.LoaderOrchestration.Interface (LoaderSide(..))
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupDispatch.Admission as DA
import qualified Replacement.StartupDispatch.Artifact as D

newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)
data LoaderSource = LoaderSource C.SourceFiles C.CompositionSource L.CanonicalSource
data LoaderNative = LoaderNative L.LinkedArtifact C.NativeFiles LI.CodeAddress LoaderSites
data LoaderJoinedArtifact = LoaderJoinedArtifact LoaderNative LoaderNative C.JoinedArtifact C.NativeFiles C.NativeFiles LoaderJoinedProof
data LoaderJoinedProof = LoaderJoinedProof C.JoinedProof [LoaderBoundary] [LoaderBoundary]
data LoaderBoundary = LoaderBoundary LA.DispatcherBoundary DA.Phase StackOffset deriving (Eq, Show)
data LoaderStagedCandidate = LoaderStagedCandidate LoaderSource LoaderJoinedArtifact L.StagedCandidate B.ByteString Image4.StagedCandidate
data LoaderSites = LoaderSites LI.CodeAddress LI.CodeAddress LI.CodeAddress A.Reg LI.CodeAddress LI.CodeAddress LI.CodeAddress
data LoaderJoinError
  = CompositionRejected C.JoinError | LoaderSourceRejected L.ArtifactError
  | LoaderRejected LoaderSide L.ArtifactError | LoaderSitesRejected LoaderSide
  | LoaderBoundaryMissing LoaderSide | LoaderPhaseRejected LoaderSide
  | LoaderContinuationRejected LoaderSide | LoaderFrameRejected LoaderSide | OwnedStackRejected
  deriving (Eq, Show)

require :: Bool -> LoaderJoinError -> Either LoaderJoinError ()
require premise failure = unless premise (Left failure)
stackOffsetValue :: StackOffset -> Int
stackOffsetValue (StackOffset value) = value
addressValue :: LI.CodeAddress -> Word32
addressValue (LI.CodeAddress value) = value
addAddress :: LI.CodeAddress -> Word32 -> LI.CodeAddress
addAddress (LI.CodeAddress value) offset = LI.CodeAddress (value+offset)
loaderOffset :: LA.StackOffset -> Int
loaderOffset (LA.StackOffset value) = value

canonicalSource :: C.SourceFiles -> Either LoaderJoinError LoaderSource
canonicalSource files = do
  composition <- either (Left . CompositionRejected) Right (C.canonicalSource files)
  loaders <- either (Left . LoaderSourceRejected) Right
    (L.canonicalSource (C.sourceDat files) (C.sourceImage4 files))
  pure (LoaderSource files composition loaders)

deriveArtifact :: C.NativeFiles -> C.NativeFiles -> C.NativeFiles -> C.NativeFiles -> Either LoaderJoinError LoaderJoinedArtifact
deriveArtifact a b dispatcher installer = do
  ownedA <- admitLoader LoaderA a
  ownedB <- admitLoader LoaderB b
  composition <- either (Left . CompositionRejected) Right (C.deriveArtifact dispatcher installer)
  let proof = C.joinedProof composition
  require (C.stackOffsetValue (C.dispatcherLow proof) < 0 && C.stackOffsetValue (C.installerLow proof) < 0
    && C.stackOffsetValue (C.joinedLow proof) <= C.stackOffsetValue (C.dispatcherLow proof)) OwnedStackRejected
  boundariesA <- joinLoader LoaderA ownedA composition
  boundariesB <- joinLoader LoaderB ownedB composition
  pure (LoaderJoinedArtifact ownedA ownedB composition dispatcher installer (LoaderJoinedProof proof boundariesA boundariesB))

admitLoader :: LoaderSide -> C.NativeFiles -> Either LoaderJoinError LoaderNative
admitLoader side files = do
  linked <- either (Left . LoaderRejected side) Right (L.deriveArtifact side (C.nativeBytes files) (C.nativeSymbols files))
  symbols <- either (const (Left (LoaderSitesRejected side))) Right (F.parseSymbols (C.nativeSymbols files))
  compiler <- either (const (Left (LoaderSitesRejected side))) (Right . LI.CodeAddress . F.symbolAddress)
    (F.lookupSymbol symbols "xm1_loader_orchestration")
  sites <- deriveSites side linked
  pure (LoaderNative linked files compiler sites)

deriveSites :: LoaderSide -> L.LinkedArtifact -> Either LoaderJoinError LoaderSites
deriveSites side linked = do
  let proof = L.linkedProof linked
      first = addressValue (LI.carrierStart side)
      word pc = U.word32At (L.linkedBytes linked) (fromIntegral (addressValue pc-first))
      instructions = [(pc,A.decode (addressValue pc) (word pc)) | pc <- LA.executionPcs proof]
      unique values = case values of [value] -> Right value; _ -> Left (LoaderSitesRejected side)
      consistent predicate applies = do
        sites <- forM LI.scenarios $ \scenario -> do
          let selected = nub [LA.effectInstruction located | located <- LA.effectSites proof
                ,LA.effectScenario located==scenario,predicate (LA.effectKind located)]
          if applies scenario then (:[]) <$> unique selected
            else require (null selected) (LoaderSitesRejected side) >> pure []
        unique (nub (concat sites))
      publishes (LI.Scenario role _ outcome) = role==LI.Primary && outcome==LI.SetupReturns
      polls (LI.Scenario role _ _) = role==LI.Peer
      initializes (LI.Scenario role completion _) = role/=LI.Peer || completion/=LI.NeverPublished
  store <- consistent (==LA.CompletionPublication) publishes
  barrier <- consistent (==LA.StoreBarrier) publishes
  poll <- consistent (\effect -> case effect of LA.CompletionRead _ -> True; _ -> False) polls
  destination <- unique [register | (pc,A.Load _ A.Word False register _ _ A.OffsetOnly) <- instructions,pc==poll]
  branch <- unique [pc | (pc,A.Branch (A.Cond condition) False _) <- instructions
    ,addressValue pc>addressValue poll,addressValue pc<=addressValue poll+8,condition==0||condition==1]
  initializer <- consistent (\effect -> case effect of LA.ServiceCall LI.PrivateInitialize _ -> True; _ -> False) initializes
  pure (LoaderSites store barrier poll destination branch initializer (LI.serviceAddress side LI.PrivateInitialize))

joinLoader :: LoaderSide -> LoaderNative -> C.JoinedArtifact -> Either LoaderJoinError [LoaderBoundary]
joinLoader side (LoaderNative linked _ _ _) composition = do
  let proof = L.linkedProof linked
      boundaries = LA.dispatcherBoundaries proof
      dispatcher = D.frameProof (C.dispatcherArtifact composition)
      joined = C.joinedProof composition
      saved = M.fromList [(LA.StackOffset (-16),LA.Incoming (A.Reg 4)),(LA.StackOffset (-12),LA.Incoming (A.Reg 5))
        ,(LA.StackOffset (-8),LA.Incoming (A.Reg 11)),(LA.StackOffset (-4),LA.IncomingLink)]
      mapped phase = case phase of
        LI.Setup -> DA.Setup; LI.Root -> DA.Root; LI.Diagnostic -> DA.Diagnostic; LI.Idle0 -> DA.Idle0; LI.PeerTasks -> DA.Idle1AndStandby
      hasPhase phase = any (\(DA.PathProof selected _ _) -> selected==phase) (DA.proofPaths dispatcher)
      first = addressValue (LI.carrierStart side)
      end = first+fromIntegral (B.length (L.linkedBytes linked))
  require (not (null boundaries)) (LoaderBoundaryMissing side)
  forM boundaries $ \boundary -> do
    let selected = mapped (LA.boundaryPhase boundary)
        stack = loaderOffset (LA.boundaryStack boundary)
        continuation = addressValue (LA.boundaryContinuation boundary)
        instruction = addressValue (LA.boundaryInstruction boundary)
        low = if selected==DA.Setup then C.joinedLow joined else C.dispatcherLow joined
    require (hasPhase selected && LA.boundarySide boundary==side) (LoaderPhaseRejected side)
    require (continuation==instruction+4 && instruction>=first && continuation<end
      && LA.boundaryContinuation boundary `elem` LA.executionPcs proof) (LoaderContinuationRejected side)
    require (LA.boundaryLiveWords boundary==saved && stack==(-16)
      && loaderOffset (LA.boundaryFramePointer boundary)==(-4)
      && all ((>= stack) . loaderOffset) (M.keys (LA.boundaryLiveWords boundary))) (LoaderFrameRejected side)
    pure (LoaderBoundary boundary selected (StackOffset (stack+C.stackOffsetValue low)))

dispatcherInstallerArtifact :: LoaderJoinedArtifact -> C.JoinedArtifact
dispatcherInstallerArtifact (LoaderJoinedArtifact _ _ artifact _ _ _) = artifact
nativeLoader :: LoaderJoinedArtifact -> LoaderSide -> LoaderNative
nativeLoader (LoaderJoinedArtifact a b _ _ _ _) side = case side of LoaderA -> a; LoaderB -> b
loaderArtifact :: LoaderJoinedArtifact -> LoaderSide -> L.LinkedArtifact
loaderArtifact artifact side = case nativeLoader artifact side of LoaderNative linked _ _ _ -> linked
loaderCompilerEntry :: LoaderJoinedArtifact -> LoaderSide -> LI.CodeAddress
loaderCompilerEntry artifact side = case nativeLoader artifact side of LoaderNative _ _ compiler _ -> compiler
loaderBodyEntry :: LoaderJoinedArtifact -> LoaderSide -> LI.CodeAddress
loaderBodyEntry artifact side = addAddress (loaderCompilerEntry artifact side) 8
loaderSites :: LoaderJoinedArtifact -> LoaderSide -> LoaderSites
loaderSites artifact side = case nativeLoader artifact side of LoaderNative _ _ _ sites -> sites
joinedProof :: LoaderJoinedArtifact -> LoaderJoinedProof
joinedProof (LoaderJoinedArtifact _ _ _ _ _ proof) = proof
loaderBoundaries :: LoaderJoinedProof -> LoaderSide -> [LoaderBoundary]
loaderBoundaries (LoaderJoinedProof _ a b) side = case side of LoaderA -> a; LoaderB -> b
loaderFrameLow :: LoaderJoinedProof -> StackOffset
loaderFrameLow _ = StackOffset (-16)
dispatcherLow :: LoaderJoinedProof -> StackOffset
dispatcherLow (LoaderJoinedProof proof _ _) = StackOffset (C.stackOffsetValue (C.dispatcherLow proof))
installerLow :: LoaderJoinedProof -> StackOffset
installerLow (LoaderJoinedProof proof _ _) = StackOffset (C.stackOffsetValue (C.installerLow proof))
joinedLow :: LoaderJoinedProof -> StackOffset
joinedLow (LoaderJoinedProof proof _ _) = StackOffset (C.stackOffsetValue (C.joinedLow proof))
boundarySide :: LoaderBoundary -> LoaderSide
boundarySide (LoaderBoundary boundary _ _) = LA.boundarySide boundary
boundaryScenario :: LoaderBoundary -> LI.Scenario
boundaryScenario (LoaderBoundary boundary _ _) = LA.boundaryScenario boundary
boundaryPhase :: LoaderBoundary -> LI.Phase
boundaryPhase (LoaderBoundary boundary _ _) = LA.boundaryPhase boundary
boundaryDispatcherPhase :: LoaderBoundary -> DA.Phase
boundaryDispatcherPhase (LoaderBoundary _ phase _) = phase
boundaryInstruction :: LoaderBoundary -> LI.CodeAddress
boundaryInstruction (LoaderBoundary boundary _ _) = LA.boundaryInstruction boundary
boundaryContinuation :: LoaderBoundary -> LI.CodeAddress
boundaryContinuation (LoaderBoundary boundary _ _) = LA.boundaryContinuation boundary
boundaryStack :: LoaderBoundary -> StackOffset
boundaryStack (LoaderBoundary boundary _ _) = StackOffset (loaderOffset (LA.boundaryStack boundary))
boundaryFramePointer :: LoaderBoundary -> StackOffset
boundaryFramePointer (LoaderBoundary boundary _ _) = StackOffset (loaderOffset (LA.boundaryFramePointer boundary))
boundaryLiveWords :: LoaderBoundary -> M.Map LA.StackOffset LA.Scalar
boundaryLiveWords (LoaderBoundary boundary _ _) = LA.boundaryLiveWords boundary
boundaryOwnedLow :: LoaderBoundary -> StackOffset
boundaryOwnedLow (LoaderBoundary _ _ low) = low
completionStore :: LoaderSites -> LI.CodeAddress
completionStore (LoaderSites pc _ _ _ _ _ _) = pc
completionBeforeDsb :: LoaderSites -> LI.CodeAddress
completionBeforeDsb (LoaderSites _ pc _ _ _ _ _) = pc
completionAfterDsb :: LoaderSites -> LI.CodeAddress
completionAfterDsb sites = addAddress (completionBeforeDsb sites) 4
peerPoll :: LoaderSites -> LI.CodeAddress
peerPoll (LoaderSites _ _ pc _ _ _ _) = pc
peerPollDestination :: LoaderSites -> A.Reg
peerPollDestination (LoaderSites _ _ _ register _ _ _) = register
peerPollPostload :: LoaderSites -> LI.CodeAddress
peerPollPostload sites = addAddress (peerPoll sites) 4
peerPollBranch :: LoaderSites -> LI.CodeAddress
peerPollBranch (LoaderSites _ _ _ _ pc _ _) = pc
initializerCall :: LoaderSites -> LI.CodeAddress
initializerCall (LoaderSites _ _ _ _ _ pc _) = pc
initializerEntry :: LoaderSites -> LI.CodeAddress
initializerEntry (LoaderSites _ _ _ _ _ _ pc) = pc
initializerReturn :: LoaderSites -> LI.CodeAddress
initializerReturn sites = addAddress (initializerCall sites) 4

stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> LoaderSource -> LoaderJoinedArtifact -> IO LoaderStagedCandidate
stageCandidate tools scratch source@(LoaderSource files composition loaders) artifact = do
  patched <- either (fail . show) pure (L.stageCandidate loaders (loaderArtifact artifact LoaderA) (loaderArtifact artifact LoaderB))
  let image = C.stagedBytes (C.stageImage4 composition (dispatcherInstallerArtifact artifact))
  candidate <- Image4.runImage4Staging tools scratch (C.sourceDat files) (L.candidateBytes patched) image
  pure (LoaderStagedCandidate source artifact patched image candidate)
candidateBytes :: LoaderStagedCandidate -> B.ByteString
candidateBytes (LoaderStagedCandidate _ _ _ _ candidate) = Image4.candidateDat candidate
candidateImage4 :: LoaderStagedCandidate -> B.ByteString
candidateImage4 (LoaderStagedCandidate _ _ _ image _) = image
candidateLoader :: LoaderStagedCandidate -> LoaderSide -> B.ByteString
candidateLoader (LoaderStagedCandidate _ _ patched _ _) = L.candidateLoader patched

identity :: B.ByteString -> String
identity = SHA.sha256Hex . SHA.sha256
nativeValue :: C.NativeFiles -> Value
nativeValue files = object ["image_sha256" .= identity (C.nativeBytes files),"symbols_sha256" .= identity (C.nativeSymbols files)]
contractValue :: LoaderSource -> LoaderJoinedArtifact -> Value
contractValue (LoaderSource files composition _) artifact@(LoaderJoinedArtifact a b joined dispatch install proof) = object
  [ "schema" .= (1::Int),"qualification" .= ("conditional-loader-startup-composition"::String)
  , "source" .= object ["dat_sha256" .= identity (C.sourceDat files),"image4_sha256" .= identity (C.sourceImage4 files)
      ,"loader_a_sha256" .= identity (C.sourceLoaderA files),"loader_b_sha256" .= identity (C.sourceLoaderB files)]
  , "native_inputs" .= object ["loader_a" .= loaderInput a,"loader_b" .= loaderInput b,"dispatcher" .= nativeValue dispatch,"installer" .= nativeValue install]
  , "dispatcher_installer" .= C.contractValue (C.stageImage4 composition joined)
  , "dispatcher_provider" .= object ["role" .= ("OwnedJoined"::String),"entry" .= (0x69cdf8::Word32)]
  , "dispatcher_calls" .= map dispatcherCallValue (DA.callBoundaries (D.frameProof (C.dispatcherArtifact joined)))
  , "loaders" .= map loaderValue [LoaderA,LoaderB]
  , "owned_stack" .= object ["loader_frame_low" .= stackOffsetValue (loaderFrameLow proof)
      ,"dispatcher_low" .= stackOffsetValue (dispatcherLow proof),"installer_low_from_entry" .= stackOffsetValue (installerLow proof)
      ,"combined_low_from_dispatcher_entry" .= stackOffsetValue (joinedLow proof)]
  , "staging" .= object
      [ "order" .= (["loader-a","loader-b","image4"]::[String])
      , "loader_checksum_exception" .= object ["offset" .= (0x20c::Int),"bytes" .= (4::Int)]
      , "image4_patches" .= ([[0x69cdf8,0x69cf84],[0x725a04,0x725b54]]::[[Word32]])
      , "reconstruction" .= ("Loader patches rebuild the pre-table container and checksum; image4 is recompressed into record4, its gap and container checksum reconstructed. Restoring canonical record4 and gap to the final parsed container must reproduce the loader-patched DAT. Raw DAT changes are not confined to four executable windows."::String)]
  , "open_premises" .=
      (["Owned-component stack lows exclude transitive retained-service depth and interrupt frames."
       ,"Natural A phases0/1/3/2 require loader-entry low L-120; natural B phases4/2 require conservative L-88, not L-120."
       ,"Actual stack backing, aliases, TCM, live-frame separation, retained resources and continuous shared observation remain independent obligations."
       ,"No whole-application execution independence, runtime retirement or camera qualification."]::[String])
  ]
  where
    loaderInput (LoaderNative _ native _ _) = nativeValue native
    dispatcherCallValue boundary =
      let instruction = DA.instructionAddressValue (DA.callInstruction boundary)
          bytes = D.linkedBytes (C.dispatcherArtifact joined)
          target = case A.decode instruction (U.word32At bytes (fromIntegral (instruction-0x69cdf8))) of
            A.Branch _ _ address -> Just address
            _ -> Nothing
      in object ["phase" .= show (DA.callPhase boundary),"effect" .= show (DA.callEffect boundary)
        ,"instruction" .= instruction,"continuation" .= fmap DA.instructionAddressValue (DA.callContinuation boundary)
        ,"target" .= target,"stack_from_dispatcher_entry" .= DA.stackOffsetValue (DA.callStack boundary)
        ,"caller_live_words" .= map DA.stackOffsetValue (DA.callLiveWords boundary)]
    loaderValue side =
      let linked = loaderArtifact artifact side
          frame = L.linkedProof linked
          first = addressValue (LI.carrierStart side)
          call (pc,target) = case target of
            LA.Dispatcher -> object ["pc" .= addressValue pc,"kind" .= ("Dispatcher"::String),"target" .= (0x69cdf8::Word32)]
            LA.Retained service -> object ["pc" .= addressValue pc,"kind" .= ("Retained"::String),"service" .= show service,"target" .= addressValue (LI.serviceAddress side service)]
          path (LA.PathProof (LI.Scenario role completion setup) effects terminal) = object
            ["role" .= show role,"completion" .= show completion,"setup" .= show setup,"terminal" .= show terminal
            ,"phases" .= [show phase | LA.DispatchCall phase <- effects]]
      in object ["side" .= show side,"linked_bytes" .= B.length (L.linkedBytes linked),"patch" .= [first,first+340]
        ,"compiler_entry" .= addressValue (loaderCompilerEntry artifact side),"body_entry" .= addressValue (loaderBodyEntry artifact side)
        ,"execution_pcs" .= map addressValue (LA.executionPcs frame),"calls" .= map call (LA.callBoundaries frame)
        ,"paths" .= map path (LA.proofPaths frame),"boundaries" .= map boundaryValue (loaderBoundaries proof side)
        ,"sites" .= sitesValue (loaderSites artifact side)]

boundaryValue :: LoaderBoundary -> Value
boundaryValue boundary =
  let LI.Scenario role completion setup = boundaryScenario boundary
  in object ["role" .= show role,"completion" .= show completion,"setup" .= show setup
    ,"phase" .= show (boundaryPhase boundary),"dispatcher_phase" .= show (boundaryDispatcherPhase boundary)
    ,"instruction" .= addressValue (boundaryInstruction boundary),"continuation" .= addressValue (boundaryContinuation boundary)
    ,"stack_from_loader_entry" .= stackOffsetValue (boundaryStack boundary),"fp_from_loader_entry" .= stackOffsetValue (boundaryFramePointer boundary)
    ,"live_words" .= [object ["offset" .= loaderOffset offset,"value" .= show value] | (offset,value) <- M.toAscList (boundaryLiveWords boundary)]
    ,"owned_low_from_loader_entry" .= stackOffsetValue (boundaryOwnedLow boundary)]
sitesValue :: LoaderSites -> Value
sitesValue sites = let A.Reg register = peerPollDestination sites in object
  ["completion_store" .= addressValue (completionStore sites),"completion_before_dsb" .= addressValue (completionBeforeDsb sites)
  ,"completion_after_dsb" .= addressValue (completionAfterDsb sites),"peer_poll" .= addressValue (peerPoll sites)
  ,"load_destination_register" .= register,"peer_poll_postload" .= addressValue (peerPollPostload sites)
  ,"peer_poll_branch" .= addressValue (peerPollBranch sites),"initializer_call" .= addressValue (initializerCall sites)
  ,"initializer_entry" .= addressValue (initializerEntry sites),"initializer_return" .= addressValue (initializerReturn sites)]
candidateContractValue :: LoaderStagedCandidate -> Value
candidateContractValue candidate@(LoaderStagedCandidate source artifact _ _ _) = object
  ["schema" .= (1::Int),"qualification" .= ("host-admitted-loader-startup-composition"::String)
  ,"composition" .= contractValue source artifact,"candidate_dat_sha256" .= identity (candidateBytes candidate)
  ,"candidate_image4_sha256" .= identity (candidateImage4 candidate),"candidate_loader_a_sha256" .= identity (candidateLoader candidate LoaderA)
  ,"candidate_loader_b_sha256" .= identity (candidateLoader candidate LoaderB)]
