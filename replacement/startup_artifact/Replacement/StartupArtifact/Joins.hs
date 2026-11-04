{-# LANGUAGE OverloadedStrings #-}
-- | The registered joins.
module Replacement.StartupArtifact.Joins
  ( JoinKind (..), registeredJoins
  ) where

import qualified A32 as A
import qualified Data.ByteString as B
import Data.List (findIndex)
import Data.Maybe (mapMaybe)
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.LoaderPrefixWitness as P
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified StartupStageContract as Stage

data JoinKind = FirstStageSecondaryRendezvous | CriticalPairPublisherBindings | PrefixOrchestrationB | FirstStageCompletionZero
  | LoaderStartupPublisher | PublisherInstallerRoots | CriticalPairInstallerRoots | DispatcherCriticalPair
  deriving (Eq, Ord, Show, Enum, Bounded)

instance JoinName JoinKind where
  joinName FirstStageSecondaryRendezvous = "first-stage-secondary-rendezvous"
  joinName CriticalPairPublisherBindings = "critical-pair-publisher-bindings"
  joinName PrefixOrchestrationB = "prefix-orchestration-b"
  joinName FirstStageCompletionZero = "first-stage-completion-zero"
  joinName LoaderStartupPublisher = "loader-startup-publisher"
  joinName PublisherInstallerRoots = "publisher-installer-roots"
  joinName CriticalPairInstallerRoots = "critical-pair-installer-roots"
  joinName DispatcherCriticalPair = "dispatcher-critical-pair"

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

-- | Little-endian word of a decoded target at a guest address.
targetWord :: TargetBytes -> Target -> Word32 -> Maybe Word32
targetWord targets target address
  | offset >= 0 && offset + 4 <= B.length bytes = Just (U.word32At bytes offset)
  | otherwise = Nothing
  where
    bytes = targetOf targets target
    offset = fromIntegral address - fromIntegral (targetBase target)

firstStageFacts :: JoinEvidence -> Maybe (Word32, LinkedBytes)
firstStageFacts evidence = case [(entry, linked) | FirstStageFacts (Pc entry) linked <- map pieceFacts (evidencePieces evidence)] of
  [facts] -> Just facts
  _ -> Nothing

firstStageEntry :: JoinEvidence -> Maybe Word32
firstStageEntry = fmap fst . firstStageFacts

secondaryFacts :: JoinEvidence -> Maybe (Word32, [ServiceBinding], LinkedBytes, ScratchBytes)
secondaryFacts evidence = case [(entry, consumed, linked, scratch) | Record2LinkFacts (Pc entry) consumed linked scratch <- map pieceFacts (evidencePieces evidence)] of
  [facts] -> Just facts
  _ -> Nothing

publications :: JoinEvidence -> Maybe [Publication]
publications evidence = case [rows | PublisherFacts _ rows <- map pieceFacts (evidencePieces evidence)] of
  [rows] -> Just rows
  _ -> Nothing

publisherFrame :: JoinEvidence -> Maybe PublisherFrame
publisherFrame evidence = case [frame | PublisherFacts frame _ <- map pieceFacts (evidencePieces evidence)] of
  [frame] -> Just frame
  _ -> Nothing

-- | The one linked orchestration image of a loader side.
orchestrationImage :: LI.LoaderSide -> JoinEvidence -> Maybe LinkedBytes
orchestrationImage side evidence = case concatMap images (map pieceFacts (evidencePieces evidence)) of
  [linked] -> Just linked
  _ -> Nothing
  where
    images facts = case (side, facts) of
      (LI.LoaderB, LoaderOrchestrationFacts linked) -> [linked]
      (LI.LoaderA, LoaderStartupFacts linked _ _) -> [linked]
      (LI.LoaderB, LoaderStartupFacts _ linked _) -> [linked]
      _ -> []

orchestrationArtifact :: LI.LoaderSide -> JoinEvidence -> Maybe L.LinkedArtifact
orchestrationArtifact side evidence = do
  LinkedBytes image symbols <- orchestrationImage side evidence
  either (const Nothing) Just (L.deriveArtifact side image symbols)

submissions :: JoinEvidence -> Maybe [SubmitCall]
submissions evidence = case [calls | LoaderStartupFacts _ _ calls <- map pieceFacts (evidencePieces evidence)] of
  [calls] -> Just calls
  _ -> Nothing

resetLiteral :: Target -> JoinEvidence -> Maybe Word32
resetLiteral target evidence = targetWord (evidenceComposed evidence) target (targetBase target + fromIntegral F.resetVectorWordOffset)

-- | Record 2 and loader B as the composed DAT loads them.
composedImages :: JoinEvidence -> P.StagedImages
composedImages evidence = P.StagedImages (targetOf (evidenceComposed evidence) Record2) (targetOf (evidenceComposed evidence) LoaderB)

rendezvous :: Join JoinKind
rendezvous = Join FirstStageSecondaryRendezvous (Set.fromList [FirstStage, Record2Link])
  [at Record2 0x10000 0x20000, at Record1 0x0 0x10000]
  (ObservationQualified "first-stage peer release and record-2 rendezvous for the current record-2 link are qualified by a composed natural comparison; E-COMPOSED-NATURAL-POSITIVE-150 covers the earlier [0x10400,0x10a6c) link")
  [ HostCheck "record1-reset-literal-is-first-stage-entry" $ \evidence ->
      maybe False (\entry -> resetLiteral Record1 evidence == Just entry) (firstStageEntry evidence)
  , HostCheck "record2-reset-literal-is-secondary-entry" $ \evidence ->
      maybe False (\(entry, _, _, _) -> resetLiteral Record2 evidence == Just entry) (secondaryFacts evidence)
  -- The composed-contract derivation: emulation/Emulation/LoaderPrefixWitness.hs:476-509.
  , HostCheck "prefix-owned-facts-on-composed-images" $ \evidence ->
      case (firstStageEntry evidence, secondaryFacts evidence) of
        (Just entry, Just (_, _, LinkedBytes image symbols, _)) -> either (const False) (const True)
          (P.stagedImagesFromParts (P.Guest entry) (evidenceParts evidence) >>= P.deriveOwnedEntryFacts image symbols)
        _ -> False
  ]

-- | Loader A's header word at 0x40010 names its published interface root.
rootHeader :: Word32
rootHeader = 0x40010

bindings :: Join JoinKind
bindings = Join CriticalPairPublisherBindings (Set.fromList [Record2Link, PrimaryPublisher])
  [at LoaderA 0x40430 0x409ac]
  (HostProof "the owned publisher emits every core-0 interface binding the critical pair consumes at the unchanged loader-A root; its proof requires every store before its return (emulation/Emulation/PrimaryInitializerArtifact.hs:300,337), and core 1 reads the core-0 table only after the loader-B wait observes the core-0 completion store, which follows the publisher's return: stock 0x40a08 after the return at 0x409e0, or loader-A orchestration's, which its admission orders after the initializer call (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs expected); the wait is the stock one or, with loader-B orchestration composed, the one join prefix-orchestration-b checks")
  [ HostCheck "loader-a-root-unchanged" $ \evidence ->
      targetWord (evidenceComposed evidence) LoaderA rootHeader == targetWord (evidenceCanonical evidence) LoaderA rootHeader
        && targetWord (evidenceCanonical evidence) LoaderA rootHeader /= Nothing
  , HostCheck "core0-bindings-published-once" $ \evidence ->
      case (targetWord (evidenceCanonical evidence) LoaderA rootHeader, secondaryFacts evidence, publications evidence) of
        (Just root, Just (_, consumed, _, _), Just emitted) -> not (null consumed) && all (published root emitted) consumed
        _ -> False
  ]
  where
    physical (Guest address) = physicalAddress (fromIntegral address)
    published root emitted (ServiceBinding offset target) =
      mapMaybe (\row -> if physical (publicationGuest row) == physical (Guest (root + offset)) then Just (publicationValue row) else Nothing) emitted
        == [target]

-- | Stock loader-B values the prefix exit reproduces.
loaderStartup, stockTop, entrySp, frameReturn, initializerSite, stackWindowLow :: Word32
loaderStartup = 0xd09ac
stockTop = 0x400f3420
entrySp = 0x400f3410
frameReturn = 0xd00c0
initializerSite = 0xd0430
stackWindowLow = 0x400f2c14

-- | Word of a linked flat image at a guest address.
imageWord :: Word32 -> B.ByteString -> Word32 -> Maybe Word32
imageWord base image address
  | address >= base && offset + 4 <= B.length image && address `mod` 4 == 0 = Just (U.word32At image offset)
  | otherwise = Nothing
  where
    offset = fromIntegral (address - base)

symbolAt :: B.ByteString -> String -> Maybe Word32
symbolAt symbols name = either (const Nothing) Just (F.parseSymbols symbols >>= fmap F.symbolAddress . flip F.lookupSymbol name)

-- | The record-2 prefix exits with @mov r0,r5; ldr lr,=0xd00c0; b 0xd09ac@,
-- its entry having installed @sp = r4 = 0x400f3420@.
prefixExitContract :: LinkedBytes -> Bool
prefixExitContract (LinkedBytes image symbols) = case (symbolAt symbols "__secondary_stage_start", symbolAt symbols "xm1_loader_prefix_entry", symbolAt symbols "xm1_loader_prefix_exit") of
  (Just base, Just entry, Just exit) ->
    let decoded site = A.decode site <$> imageWord base image site
        literalOf site = case decoded site of
          Just (A.Load (A.Cond 14) A.Word False destination source (A.OffsetImmediate offset) A.OffsetOnly)
            | source == A.pc -> (,) destination <$> imageWord base image (site + 8 + fromIntegral offset)
          _ -> Nothing
        isBranch = case decoded exit of
          Just (A.Branch (A.Cond 14) False target) -> target == loaderStartup
          _ -> False
        movesArgument = case decoded (exit - 8) of
          Just (A.DataProcessing (A.Cond 14) A.MOV False destination _ (A.ShiftedImmediate source A.LSL 0)) -> destination == A.Reg 0 && source == A.Reg 5
          _ -> False
        stackInstalls = [site | site <- [entry, entry + 4 .. exit - 4], literalOf site == Just (A.Reg 4, stockTop)
          , case decoded (site + 4) of
              Just (A.DataProcessing (A.Cond 14) A.MOV False destination _ (A.ShiftedImmediate source A.LSL 0)) -> destination == A.sp && source == A.Reg 4
              _ -> False]
    in isBranch && movesArgument && literalOf (exit - 4) == Just (A.lr, frameReturn) && not (null stackInstalls)
  _ -> False

-- | Every peer path reads a published completion before calling the
-- initializer, delays 100 NOPs after every pending read, and a path whose
-- completion is never published waits without calling it.
peerWaitsBeforeInitializer :: L.LinkedArtifact -> Bool
peerWaitsBeforeInitializer artifact = all admissible (LA.proofPaths (L.linkedProof artifact))
  where
    initializes effect = case effect of LA.ServiceCall LI.PrivateInitialize _ -> True; _ -> False
    admissible (LA.PathProof (LI.Scenario role completion _) effects terminal)
      | role /= LI.Peer = True
      | completion == LI.NeverPublished = terminal == LA.WaitingForCompletion && not (any initializes effects)
      | otherwise = case (findIndex (== LA.CompletionRead LA.Published) effects, findIndex initializes effects) of
          (Just published, Just called) -> published < called && delayed effects
          _ -> False
    delayed (LA.CompletionRead LA.Pending : rest) = take 1 rest == [LA.DelayNops (LA.NopCount 100)] && delayed rest
    delayed (_ : rest) = delayed rest
    delayed [] = True

-- | Registered once per piece that owns loader-B orchestration.
prefixOrchestration :: PieceKind -> Join JoinKind
prefixOrchestration owner = Join PrefixOrchestrationB (Set.fromList [Record2Link, owner])
  [at LoaderB 0xd000c 0xd0010, at LoaderB initializerSite (initializerSite + 4), at LoaderB loaderStartup 0xd0b00]
  (ObservationQualified "the record-2 prefix enters loader-B orchestration with the stock 0xd00c4 state and the orchestration calls the record-2 initializer through 0xd0430 after the completion wait; qualified by a composed natural comparison")
  [ HostCheck "prefix-exit-enters-orchestration" $ \evidence ->
      case secondaryFacts evidence of
        Just (_, _, linked@(LinkedBytes image symbols), _) -> prefixExitContract linked
          && either (const False) (const True) (P.deriveOwnedEntryFacts image symbols (composedImages evidence))
        Nothing -> False
  , HostCheck "orchestration-entry-pushes-the-caller-frame" $ \evidence ->
      pushesCallerFrame (targetWord (evidenceComposed evidence) LoaderB) loaderStartup
  , HostCheck "published-initializer-entry" $ \evidence ->
      case (secondaryFacts evidence, targetWord (evidenceComposed evidence) LoaderB initializerSite) of
        (Just (_, _, LinkedBytes _ symbols, _), Just word) -> case (symbolAt symbols "xm1_interface_initializer_entry", A.decode initializerSite word) of
          (Just entry, A.Branch (A.Cond 14) False target) -> target == entry
          _ -> False
        _ -> False
  , HostCheck "peer-waits-before-initializer" $ \evidence ->
      maybe False peerWaitsBeforeInitializer (orchestrationArtifact LI.LoaderB evidence)
  , HostCheck "initializer-scratch-in-window" $ \evidence ->
      case secondaryFacts evidence of
        Just (_, _, _, ScratchBytes scratch) -> toInteger entrySp - toInteger scratch >= toInteger stackWindowLow
        Nothing -> False
  ]

-- | The stock loader frame push at a carrier start: @push {r4,r5,r11,lr}; add r11,sp,#12@.
pushesCallerFrame :: (Word32 -> Maybe Word32) -> Word32 -> Bool
pushesCallerFrame word start =
  fmap (A.decode start) (word start)
    == Just (A.StoreMultiple (A.Cond 14) A.sp A.DecrementBefore True [A.Reg 4, A.Reg 5, A.Reg 11, A.lr])
  && fmap (A.decode (start + 4)) (word (start + 4))
    == Just (A.DataProcessing (A.Cond 14) A.ADD False (A.Reg 11) A.sp (A.Immediate 12))

-- | Registered once per piece that owns loader-B orchestration.
completionZero :: PieceKind -> Join JoinKind
completionZero owner = Join FirstStageCompletionZero (Set.fromList [FirstStage, owner])
  [at Record1 0x0 0x10000]
  (ObservationQualified "the first stage zeroes 0x400a00c4 before the peer rendezvous that precedes the loader-B wait; the order is qualified by a composed natural comparison")
  [ HostCheck "first-stage-zeroes-the-completion-cell" $ \evidence ->
      case firstStageFacts evidence of
        Just (_, LinkedBytes image symbols) -> case Stage.parseFirstStageArtifact (U.MediaImage image) symbols of
          Right artifact ->
            let plan = Stage.firstStageArtifactPlan artifact
                load = fromIntegral (F.recordGuestBase F.FirstRecord + F.slotOffsetBytes (F.planLoadOffset plan)) - fromIntegral (targetBase Record1)
                record = targetOf (evidenceComposed evidence) Record1
            in B.take (B.length image) (B.drop load record) == image
                 && either (const False) (const True) (P.deriveCompletionZero artifact)
          Left _ -> False
        Nothing -> False
  ]

-- | Loader-A orchestration starts at the stock 0x409ac; stock entry code reaches it with the SP its 0x400a8 literal names.
loaderAStart, loaderAStackLoad :: Word32
loaderAStart = 0x409ac
loaderAStackLoad = 0x400a8

-- | The address and value of the word the @ldr r4,[pc,#imm]@ at 0x400a8 loads, in a target set.
loaderAStackLiteral :: TargetBytes -> Maybe (Word32, Word32)
loaderAStackLiteral targets = do
  load <- targetWord targets LoaderA loaderAStackLoad
  address <- case A.decode loaderAStackLoad load of
    A.Load (A.Cond 14) A.Word False (A.Reg 4) base (A.OffsetImmediate offset) A.OffsetOnly
      | base == A.pc -> Just (fromIntegral (toInteger loaderAStackLoad + 8 + toInteger offset))
    _ -> Nothing
  value <- targetWord targets LoaderA address
  pure (address, value)

-- | The composed word at loader-A orchestration's initializer call, decoded.
initializerCallA :: JoinEvidence -> Maybe A.Instruction
initializerCallA evidence = do
  artifact <- orchestrationArtifact LI.LoaderA evidence
  sites <- either (const Nothing) Just (LS.deriveSites LI.LoaderA artifact)
  let LI.CodeAddress site = LS.initializerCall sites
  A.decode site <$> targetWord (evidenceComposed evidence) LoaderA site

loaderStartupPublisher :: Join JoinKind
loaderStartupPublisher = Join LoaderStartupPublisher (Set.fromList [LoaderStartup, PrimaryPublisher])
  [at LoaderA 0x40430 loaderAStart, at LoaderA loaderAStart 0x40b00]
  (ObservationQualified "loader-A orchestration calls the owned publisher at 0x40430 from its admitted frame, after the identity and shared clear and before its first dispatch, and publishes completion after it returns (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs expected); the publisher's caller-return premises at 0x409e4 and 0x40ab4 are replaced by that admission, which discards the result and requires Log with r0 = 0 next; qualified by a composed natural comparison")
  [ HostCheck "orchestration-a-calls-publisher-entry" $ \evidence ->
      case (initializerCallA evidence, publisherFrame evidence) of
        (Just (A.Branch (A.Cond 14) True target), Just (PublisherFrame (Pc entry) _)) -> target == entry
        _ -> False
  , HostCheck "orchestration-a-entry-frame" $ \evidence ->
      pushesCallerFrame (targetWord (evidenceComposed evidence) LoaderA) loaderAStart
        && loaderAStackLiteral (evidenceComposed evidence) == loaderAStackLiteral (evidenceCanonical evidence)
        && loaderAStackLiteral (evidenceCanonical evidence) /= Nothing
        && all (\address -> targetWord (evidenceComposed evidence) LoaderA address == targetWord (evidenceCanonical evidence) LoaderA address)
             [loaderAStackLoad, loaderAStackLoad + 4]
  -- Every loader-A service call runs at the entry SP minus the 16-byte frame
  -- (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs requireFrame).
  , HostCheck "publisher-caller-sp-is-orchestration-frame" $ \evidence ->
      case (publisherFrame evidence, loaderAStackLiteral (evidenceComposed evidence)) of
        (Just (PublisherFrame _ (Guest callerSp)), Just (_, top)) -> toInteger callerSp == toInteger top - 16
        _ -> False
  ]

-- | Loader headers whose roots the installer stores at 0x96bd20 and 0x96bd24.
loaderHeadersUnchanged :: HostCheck
loaderHeadersUnchanged = HostCheck "loader-headers-unchanged" $ \evidence ->
  all (\(target, address) -> targetWord (evidenceComposed evidence) target address == targetWord (evidenceCanonical evidence) target address
        && targetWord (evidenceCanonical evidence) target address /= Nothing)
    [(LoaderA, 0x40010), (LoaderB, 0xd0010)]

installerRootsBasis :: JoinBasis
installerRootsBasis = HostProof "the owned installer stores loader A's header root at 0x96bd20 and loader B's at 0x96bd24 (replacement/kernel_install/Replacement/KernelInstall/Admission.hs RootWrite), as the stock words at 0x725a0c..0x725a34 do; the dispatcher's Setup path ends in Install (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:205) and Setup is loader-A orchestration's first dispatch (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs expected)"

publisherInstallerRoots :: Join JoinKind
publisherInstallerRoots = Join PublisherInstallerRoots (Set.fromList [PrimaryPublisher, LoaderStartup])
  [at Image4 0x725a04 0x725a34] installerRootsBasis [loaderHeadersUnchanged]

criticalPairInstallerRoots :: Join JoinKind
criticalPairInstallerRoots = Join CriticalPairInstallerRoots (Set.fromList [Record2Link, LoaderStartup])
  [at Image4 0x725a0c 0x725a34] installerRootsBasis [loaderHeadersUnchanged]

-- | Core-0 and core-1 service SPs of the loader orchestration, and the low ends of their boot-stack windows
-- (replacement/startup_artifact/Replacement/StartupArtifact/Adapters.hs record2Premise).
serviceSp, stackFloor :: Core -> Word32
serviceSp Core0 = 0x40063450
serviceSp Core1 = 0x400f3410
stackFloor Core0 = 0x40062c54
stackFloor Core1 = 0x400f2c14

-- | Stock task-submission wrapper frame (emulation/Emulation/CriticalPair.hs:137) and critical-pair entry frame
-- (emulation/Emulation/CriticalPair.hs:643-644).
wrapperFrame, criticalEntryFrame :: Integer
wrapperFrame = 56
criticalEntryFrame = 128

dispatcherCriticalPair :: Join JoinKind
dispatcherCriticalPair = Join DispatcherCriticalPair (Set.fromList [Record2Link, LoaderStartup])
  [at Image4 0x7235d8 0x723790, at LoaderA loaderAStart 0x40b00]
  (ObservationQualified "the owned dispatcher submits tasks through the stock wrapper 0x725dd8, which enters the owned critical pair; loader-A orchestration fills the core-0 table the pair reads before it dispatches; qualified by a composed natural comparison")
  [ HostCheck "orchestration-a-initializes-before-dispatch" $ \evidence ->
      initializerCallA evidence == Just (A.Branch (A.Cond 14) True 0x40430)
        && maybe False initializesBeforeDispatch (orchestrationArtifact LI.LoaderA evidence)
  , HostCheck "submission-depth-in-stack-window" $ \evidence ->
      case submissions evidence of
        Just calls@(_ : _) -> all (\(SubmitCall core (StackDepth depth)) ->
          toInteger (serviceSp core) - toInteger depth - wrapperFrame - criticalEntryFrame >= toInteger (stackFloor core)) calls
        _ -> False
  ]
  where
    initializesBeforeDispatch artifact = all ordered (LA.proofPaths (L.linkedProof artifact))
    initializes effect = case effect of LA.ServiceCall LI.PrivateInitialize _ -> True; _ -> False
    dispatches effect = case effect of LA.DispatchCall _ -> True; _ -> False
    ordered (LA.PathProof (LI.Scenario role _ _) effects _)
      | role /= LI.Primary = True
      | otherwise = case (findIndex initializes effects, findIndex dispatches effects) of
          (Just called, Just dispatched) -> called < dispatched
          (_, Nothing) -> True
          _ -> False

registeredJoins :: [Join JoinKind]
registeredJoins =
  [ rendezvous, bindings
  , prefixOrchestration LoaderOrchestrationB, prefixOrchestration LoaderStartup
  , completionZero LoaderOrchestrationB, completionZero LoaderStartup
  , loaderStartupPublisher, publisherInstallerRoots, criticalPairInstallerRoots, dispatcherCriticalPair ]
