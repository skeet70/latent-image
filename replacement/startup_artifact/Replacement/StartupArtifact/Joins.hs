{-# LANGUAGE OverloadedStrings #-}
-- | The registered joins.
module Replacement.StartupArtifact.Joins
  ( JoinKind (..), registeredJoins, joinedTasks, submissionStack, submissionStackWith
  ) where

import qualified A32 as A
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Maybe (isJust, mapMaybe)
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.LoaderPrefixWitness as P
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.StartupComposition.Artifact as SC
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupComposition.TaskSubmission as TS
import qualified Replacement.IdleTask.Admission as IA
import qualified Replacement.IdleTask.Artifact as IT
import qualified Replacement.RootTask.Admission as RA
import qualified Replacement.RootTask.Artifact as RT
import qualified Replacement.StartupArtifact.Footprint.ApplicationPump as Pump
import qualified Replacement.Task1Actor.Artifact as T1
import qualified Replacement.StartupArtifact.Footprint.Producers as Producers
import qualified Replacement.StartupArtifact.Footprint.RootTask as RootFootprint
import qualified Replacement.TaskSubmission.Admission as WA
import qualified Replacement.TaskSubmission.Artifact as W
import qualified StartupStageContract as Stage

data JoinKind = FirstStageSecondaryRendezvous | CriticalPairPublisherBindings | PrefixOrchestrationB | FirstStageCompletionZero
  | LoaderStartupPublisher | PublisherInstallerRoots | CriticalPairInstallerRoots | DispatcherCriticalPair
  | TaskCompositionOwnedCalls | DispatcherTaskComposition | PublisherTaskServices
  | DispatcherRootEntry | DispatcherIdleEntry | WrapperIdleErrorCallback | RootTaskServices
  | Image5ProducerServices | PlainProducerServices | ApplicationPumpServices | Task1ActorProducer | Task1ActorPump
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
  joinName TaskCompositionOwnedCalls = "task-composition-owned-calls"
  joinName DispatcherTaskComposition = "dispatcher-task-composition"
  joinName PublisherTaskServices = "publisher-task-services"
  joinName DispatcherRootEntry = "dispatcher-root-entry"
  joinName DispatcherIdleEntry = "dispatcher-idle-entry"
  joinName WrapperIdleErrorCallback = "wrapper-idle-error-callback"
  joinName RootTaskServices = "root-task-services"
  joinName Image5ProducerServices = "image5-producer-services"
  joinName PlainProducerServices = "plain-producer-services"
  joinName ApplicationPumpServices = "application-pump-services"
  joinName Task1ActorProducer = "task1-actor-producer"
  joinName Task1ActorPump = "task1-actor-pump"

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
secondaryFacts evidence = case [(entry, consumed, linked, scratch) | Record2LinkFacts (Pc entry) consumed linked scratch _ <- map pieceFacts (evidencePieces evidence)] of
  [facts] -> Just facts
  _ -> Nothing

criticalFrame :: JoinEvidence -> Maybe StackDepth
criticalFrame evidence = case [frame | Record2LinkFacts _ _ _ _ frame <- map pieceFacts (evidencePieces evidence)] of
  [frame] -> Just frame
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
      (LI.LoaderA, LoaderStartupFacts linked _ _ _) -> [linked]
      (LI.LoaderB, LoaderStartupFacts _ linked _ _) -> [linked]
      _ -> []

orchestrationArtifact :: LI.LoaderSide -> JoinEvidence -> Maybe L.LinkedArtifact
orchestrationArtifact side evidence = do
  LinkedBytes image symbols <- orchestrationImage side evidence
  either (const Nothing) Just (L.deriveArtifact side image symbols)

submissions :: JoinEvidence -> Maybe [SubmitCall]
submissions evidence = case [calls | LoaderStartupFacts _ _ calls _ <- map pieceFacts (evidencePieces evidence)] of
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

-- | Registered once per piece that owns loader-B orchestration.
prefixOrchestration :: PieceKind -> Join JoinKind
prefixOrchestration owner = Join PrefixOrchestrationB (Set.fromList [Record2Link, owner])
  [at LoaderB 0xd000c 0xd0010, at LoaderB initializerSite (initializerSite + 4), at LoaderB loaderStartup 0xd0b00]
  (ObservationQualified "the record-2 prefix enters loader-B orchestration with the stock 0xd00c4 state and the orchestration calls the record-2 initializer through 0xd0430 after the completion wait; the wait-before-initializer order is loader-B orchestration's own admission (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:219-240,261); qualified by a composed natural comparison")
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
  -- The frame push itself is loader-A orchestration's admission (Admission.hs requireFrame).
  , HostCheck "loader-a-entry-stack-unchanged" $ \evidence ->
      loaderAStackLiteral (evidenceComposed evidence) == loaderAStackLiteral (evidenceCanonical evidence)
        && loaderAStackLiteral (evidenceCanonical evidence) /= Nothing
        && all (\address -> targetWord (evidenceComposed evidence) LoaderA address == targetWord (evidenceCanonical evidence) LoaderA address)
             [loaderAStackLoad, loaderAStackLoad + 4]
  , HostCheck "publisher-caller-sp-is-orchestration-frame" $ \evidence ->
      case (publisherFrame evidence, loaderAStackLiteral (evidenceComposed evidence), orchestrationArtifact LI.LoaderA evidence) of
        (Just (PublisherFrame _ (Guest callerSp)), Just (_, top), Just artifact) ->
          let LA.StackOffset low = LA.frameLow (L.linkedProof artifact)
          in toInteger callerSp == toInteger top + toInteger low
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

-- | Stock task-submission wrapper frame, which the record-2 admission pins (emulation/Emulation/CriticalPair.hs:133).
wrapperFrame :: Integer
wrapperFrame = 56

dispatcherCriticalPair :: Join JoinKind
dispatcherCriticalPair = Join DispatcherCriticalPair (Set.fromList [Record2Link, LoaderStartup])
  [at Image4 0x7235d8 0x723790, at LoaderA loaderAStart 0x40b00]
  (ObservationQualified "the owned dispatcher submits tasks through the task wrapper at 0x725dd8, stock or the owned task-submission piece, which enters the owned critical pair; submission-depth-in-stack-window bounds each submission with the stock wrapper's 56-byte frame (emulation/Emulation/CriticalPair.hs:133) and the deeper critical-pair frame, and with task-submission composed dispatcher-task-composition bounds the owned wrapper's joined stack; loader-A orchestration's admission orders its call of the initializer at 0x40430, which fills the core-0 table the pair reads, before its first dispatch (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs expected); qualified by a composed natural comparison")
  [ HostCheck "submission-depth-in-stack-window" $ \evidence ->
      case (submissions evidence, criticalFrame evidence) of
        (Just calls@(_ : _), Just (StackDepth pairFrame)) -> all (\(SubmitCall core (StackDepth depth)) ->
          toInteger (serviceSp core) - toInteger depth - wrapperFrame - toInteger pairFrame >= toInteger (stackFloor core)) calls
        _ -> False
  ]

registeredJoins :: [Join JoinKind]
registeredJoins =
  [ rendezvous, bindings
  , prefixOrchestration LoaderOrchestrationB, prefixOrchestration LoaderStartup
  , completionZero LoaderOrchestrationB, completionZero LoaderStartup
  , loaderStartupPublisher, publisherInstallerRoots, criticalPairInstallerRoots, dispatcherCriticalPair
  , taskCompositionOwnedCalls, dispatcherTaskComposition, publisherTaskServices
  , dispatcherRootEntry, dispatcherIdleEntry, wrapperIdleErrorCallback, rootTaskServices
  , image5ProducerServices, plainProducerServices, applicationPumpServices, task1ActorProducer, task1ActorPump ]

-- | The task wrapper, the record-2 critical pair and the task-storage transition joined as in
-- replacement/startup_composition/Replacement/StartupComposition/TaskSubmission.hs:63-97.
joinedTasks :: JoinEvidence -> Maybe TS.JoinedArtifact
joinedTasks evidence = do
  LinkedBytes wrapper wrapperSymbols <- single [linked | TaskSubmissionFacts linked _ <- facts]
  LinkedBytes storage storageSymbols <- single [linked | TaskStorageFacts linked <- facts]
  (_, _, LinkedBytes critical criticalSymbols, _) <- secondaryFacts evidence
  either (const Nothing) Just (TS.deriveArtifact (SC.NativeFiles wrapper wrapperSymbols)
    (SC.NativeFiles critical criticalSymbols) (SC.NativeFiles storage storageSymbols))
  where
    facts = map pieceFacts (evidencePieces evidence)
    single [linked] = Just linked
    single _ = Nothing

-- | Root-1 table slots the canonical loader-B initializer stores (emulation/Emulation/InterfaceInitializer.hs:510-534); the
-- owned record-2 initializer's stores are checked against them by receipt validation (:785-795) and the record2-link/table-b read.
tableBSlotsStored :: JoinEvidence -> Bool
tableBSlotsStored evidence =
  case (targetWord (evidenceCanonical evidence) LoaderB 0xd0010, I.originalStores (targetOf (evidenceCanonical evidence) LoaderB)) of
    (Just root, Right stores) -> all (\(WA.RootOffset offset, _) ->
      length [() | I.ExpectedStore (P.Guest guest) _ <- stores, guest == root + offset] == 1) WA.rootServiceOffsets
    _ -> False

taskCompositionOwnedCalls :: Join JoinKind
taskCompositionOwnedCalls = Join TaskCompositionOwnedCalls (Set.fromList [Record2Link, TaskSubmission, TaskStorage])
  [at Image4 0x7235d8 0x723790, at Image4 0x725cc4 0x725da8, at Image4 0x725dd8 0x72612c, at LoaderB initializerSite (initializerSite + 4)]
  (ObservationQualified "the owned wrapper calls the record-2 critical pair through the entry words at 0x7235d8 and 0x7236b0 and the storage transition through the retained prelude at 0x725c6c; its call targets are wrapper-admission constants (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:93-97), call-SP alignment, live words and continuations are checked by that admission's external calls, and the storage adapter ABI by the storage admission (replacement/task_storage/Replacement/TaskStorage/Artifact.hs:118-125); owned-calls-join derives each owned callee's frame low at the wrapper's actual call SPs, which submission-joined-stack-in-boot-window bounds, and with admitted pieces fails only on a missing owned call; on core 1 the wrapper and the storage prelude load root-1 slots that the record-2 initializer publishes; exit receiving enter's token and the reservation arguments are source properties qualified by a composed natural comparison")
  [ HostCheck "owned-calls-join" (isJust . joinedTasks)
  , HostCheck "table-b-service-slots-stored" tableBSlotsStored ]

-- | The dispatcher passes descriptor D = S-44 (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:234),
-- of which the wrapper reads 28 bytes (replacement/task_submission/Replacement/TaskSubmission/Admission.hs Guest Descriptor).
descriptorOffset, descriptorBytes :: WA.StackOffset
descriptorOffset = WA.StackOffset 44
descriptorBytes = WA.StackOffset 28

-- | The owned wrapper's joined stack at a dispatcher submission: entered at the boot stack's service SP
-- less the call depth, written only inside that core's boot-stack window, disjoint from the descriptor.
submissionStack :: TS.JoinedArtifact -> SubmitCall -> Either TS.JoinError WA.JoinedStack
submissionStack = submissionStackWith WA.stockCalleeDemand

-- | 'submissionStack' with the retained callees' demands from the given table.
submissionStackWith :: (WA.CallTarget -> WA.CalleeDemand) -> TS.JoinedArtifact -> SubmitCall -> Either TS.JoinError WA.JoinedStack
submissionStackWith demand joined (SubmitCall core (StackDepth depth)) = TS.joinStack joined WA.Submission context retained
  where
    wrapperSp = toInteger (serviceSp core) - toInteger depth
    WA.StackOffset offset = descriptorOffset
    WA.StackOffset bytes = descriptorBytes
    descriptor = toInteger depth - offset
    context = WA.StackContext (fromInteger wrapperSp)
      (WA.StackInterval (WA.StackOffset (toInteger (stackFloor core) - wrapperSp)) (WA.StackOffset 0))
      [WA.StackInterval (WA.StackOffset descriptor) (WA.StackOffset (descriptor + bytes))]
    owned = map WA.FixedService [WA.CriticalEnter, WA.CriticalExit, WA.Reservation]
    retained = [ demand target
               | target <- nub (map WA.callTarget (WA.callBoundaries (W.frameProof (TS.wrapperArtifact joined)))), target `notElem` owned ]

dispatcherTaskComposition :: Join JoinKind
dispatcherTaskComposition = Join DispatcherTaskComposition (Set.fromList [LoaderStartup, Record2Link, TaskSubmission, TaskStorage])
  [ at Image4 0x69cdf8 0x69cf84, at Image4 0x725a04 0x725b54, at LoaderA loaderAStart 0x40b00, at LoaderB loaderStartup 0xd0b00
  , at Image4 0x725cc4 0x725da8, at Image4 0x725dd8 0x72612c ]
  (ObservationQualified "the owned dispatcher hands D = S-44 to the owned wrapper at S-64 on each core's boot stack; the descriptor contents, r0 and the descriptor stores before each call are the dispatcher's admission (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206-211,225,233-237), tables A and B are filled before the first dispatch by the orchestration admission (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:219-240,261), and the class roots are the installer's stores of the loader headers (replacement/kernel_install/Replacement/KernelInstall/Admission.hs RootWrite); qualified by a composed natural comparison")
  [ HostCheck "submission-joined-stack-in-boot-window" $ \evidence ->
      case (submissions evidence, joinedTasks evidence) of
        (Just calls@(_ : _), Just joined) -> all (\call -> either (const False) (const True) (submissionStack joined call)) calls
        _ -> False
  , loaderHeadersUnchanged ]

publisherTaskServices :: Join JoinKind
publisherTaskServices = Join PublisherTaskServices (Set.fromList [PrimaryPublisher, TaskSubmission, TaskStorage])
  [at LoaderA 0x40430 0x409ac]
  (HostProof "the owned publisher emits its complete ordered stock source publication (emulation/Emulation/PrimaryInitializerArtifact.hs:297-302,470), so each slot it publishes holds the stock service; the wrapper and the storage prelude load service pointers from the root slots replacement/task_submission/Replacement/TaskSubmission/Admission.hs rootServiceOffsets names")
  [ HostCheck "task-service-slots-published-once" $ \evidence ->
      case (targetWord (evidenceCanonical evidence) LoaderA rootHeader, publications evidence) of
        (Just root, Just emitted) -> all (\(WA.RootOffset offset, _) ->
          length [() | row <- emitted, physicalOf (publicationGuest row) == physicalOf (Guest (root + offset))] == 1) WA.rootServiceOffsets
        _ -> False ]
  where
    physicalOf (Guest address) = physicalAddress (fromIntegral address)

taskStarts :: JoinEvidence -> Maybe [TaskStart]
taskStarts evidence = case [starts | LoaderStartupFacts _ _ _ starts <- map pieceFacts (evidencePieces evidence)] of
  [starts] -> Just starts
  _ -> Nothing

-- | The root image's entry symbol and admitted frame low.
rootFrame :: JoinEvidence -> Maybe (Word32, Int)
rootFrame evidence = case [linked | RootTaskFacts linked _ <- map pieceFacts (evidencePieces evidence)] of
  [LinkedBytes image symbols] -> do
    artifact <- either (const Nothing) Just (RT.deriveArtifact image symbols)
    entry <- symbolAt symbols "xm1_root_task"
    pure (entry, RA.scratchLow (RT.artifactProof artifact))
  _ -> Nothing

-- | The idle image's entry symbol and admitted frame low.
idleFrame :: JoinEvidence -> Maybe (Word32, Int)
idleFrame evidence = case [linked | IdleTaskFacts linked _ <- map pieceFacts (evidencePieces evidence)] of
  [LinkedBytes image symbols] -> do
    artifact <- either (const Nothing) Just (IT.deriveArtifact image symbols)
    entry <- symbolAt symbols "xm1_idle_task"
    pure (entry, IA.scratchLow (IT.artifactProof artifact))
  _ -> Nothing

-- | The dispatcher starts the task exactly once, at this entry.
dispatchedAt :: Word32 -> Word32 -> [TaskStart] -> Bool
dispatchedAt task entry starts = [address | TaskStart (TaskNumber number) (Pc address) _ <- starts, number == task] == [entry]

-- | Bytes of the source IRQ frame a task stack must hold below the task's own frame
-- (replacement/idle_task/Replacement/IdleTask/Artifact.hs:131).
irqFrameBytes :: Integer
irqFrameBytes = 32

-- | The task's one requested stack holds its admitted frame and the IRQ frame.
stackCovers :: Word32 -> Int -> [TaskStart] -> Bool
stackCovers task low starts = case [bytes | TaskStart (TaskNumber number) _ (StackBytes bytes) <- starts, number == task] of
  [bytes] -> toInteger bytes >= toInteger (negate low) + irqFrameBytes
  _ -> False

dispatcherRootEntry :: Join JoinKind
dispatcherRootEntry = Join DispatcherRootEntry (Set.fromList [LoaderStartup, RootTask])
  [at Image4 0x69d160 0x69d164]
  (ObservationQualified "the owned dispatcher submits task 104 with entry 0x69d160 and 0x400 stack bytes (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206), the root image links its entry there (replacement/root_task/Replacement/RootTask/Artifact.hs:69-71), and the retained trampoline enters it with r0 the argument 0 and SP the stack top; qualified by the composed comparison at task-submission/a104-first-entry")
  [ HostCheck "root-entry-dispatched" $ \evidence ->
      case (taskStarts evidence, rootFrame evidence) of
        (Just starts, Just (entry, _)) -> dispatchedAt 104 entry starts
        _ -> False
  , HostCheck "root-stack-request-covers-frame" $ \evidence ->
      case (taskStarts evidence, rootFrame evidence) of
        (Just starts, Just (_, low)) -> stackCovers 104 low starts
        _ -> False ]

dispatcherIdleEntry :: Join JoinKind
dispatcherIdleEntry = Join DispatcherIdleEntry (Set.fromList [LoaderStartup, IdleTask])
  [at Image4 0x69cbf0 0x69cbf4]
  (ObservationQualified "the owned dispatcher submits tasks 102 and 103 with entry 0x69cbf0 and 0x400 stack bytes (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:208,210), the idle image links its entry there (replacement/idle_task/Replacement/IdleTask/Artifact.hs:63-65), and the retained trampolines enter it with r0 the argument 0 and SP the stack top; qualified by the composed comparison at idle-task/first-entry")
  [ HostCheck "idle-entry-dispatched" $ \evidence ->
      case (taskStarts evidence, idleFrame evidence) of
        (Just starts, Just (entry, _)) -> all (\task -> dispatchedAt task entry starts) [102, 103]
        _ -> False
  , HostCheck "idle-stack-request-covers-frame" $ \evidence ->
      case (taskStarts evidence, idleFrame evidence) of
        (Just starts, Just (_, low)) -> all (\task -> stackCovers task low starts) [102, 103]
        _ -> False ]

-- | The wrapper's retained demands with error dispatch into a callback of the given frame low.
idleCallbackDemand :: Int -> WA.CallTarget -> WA.CalleeDemand
idleCallbackDemand low target
  | target == WA.FixedService WA.ErrorCallback = WA.errorCallbackDemand (WA.StackOffset (toInteger low))
  | otherwise = WA.stockCalleeDemand target

wrapperIdleErrorCallback :: Join JoinKind
wrapperIdleErrorCallback = Join WrapperIdleErrorCallback (Set.fromList [TaskSubmission, IdleTask])
  [at Image4 0x69cbf0 0x69cc00]
  (HostProof "the wrapper's error dispatch 0x723398 pushes 8 bytes and calls the callback (replacement/task_submission/Replacement/TaskSubmission/Admission.hs errorCallbackDemand); with callback 0x69cbf0 the demand is 8 plus the idle admission's frame (replacement/idle_task/Replacement/IdleTask/Admission.hs:157-171), and every dispatcher submission's joined stack with that demand fits its boot-stack window; dispatcher-task-composition keeps the stock 12 bytes for callback 0x723330, so the checked bound is the larger of the two; the natural callback word holds 0x723330 or 0x72835c, so this bounds a non-natural callback")
  [ HostCheck "error-callback-idle-demand" $ \evidence ->
      case (submissions evidence, joinedTasks evidence, idleFrame evidence) of
        (Just calls@(_ : _), Just joined, Just (_, low)) ->
          all (either (const False) (const True) . submissionStackWith (idleCallbackDemand low) joined) calls
        _ -> False ]

-- | Owned carriers a root or producer closure may enter, with the entries their pieces admit: the wrapper and release
-- (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:87-91), the critical-pair branch words
-- (record-2 staging) and the storage transition, which only the retained prelude enters.
admittedEntries :: [((Word32, Word32), [Word32])]
admittedEntries =
  [ ((0x725dd8, 0x72612c), [0x725dd8, 0x726098])
  , ((0x7235d8, 0x723790), [0x7235d8, 0x7236b0])
  , ((0x725cc4, 0x725da8), []) ]

-- | Direct calls from a closure into an owned carrier of 'admittedEntries', from outside those carriers, each with
-- the entries its piece admits.
entering :: [(Word32, Word32)] -> [(Word32, [Word32])]
entering calls = [ (target, entries) | (site, target) <- calls, not (any (within site . fst) admittedEntries)
                 , (range, entries) <- admittedEntries, within target range ]
  where within address (low, high) = address >= low && address < high

closureEntersAtEntries :: JoinEvidence -> Bool
closureEntersAtEntries evidence = any ((== 0x725dd8) . fst) calls && all (uncurry elem) calls
  where calls = entering (RootFootprint.closureCalls (targetOf (evidenceCanonical evidence) Image4))

rootTaskServices :: Join JoinKind
rootTaskServices = Join RootTaskServices (Set.fromList [RootTask, Record2Link, TaskSubmission, TaskStorage])
  [at Image4 0x7235d8 0x723790, at Image4 0x725cc4 0x725da8, at Image4 0x725dd8 0x72612c]
  (ObservationQualified "root's retained submitters (0x69ca24 through 0x6b0764, and the constructors under 0x69ca44) call the owned wrapper at 0x725dd8, its cleanup 0x72612c can call owned release 0x726098, and the stock wrapper body and callback 0x69cc18 reach the critical pair and, through the retained prelude 0x725c6c, the storage transition; root-closure-enters-carriers-at-entries checks that every direct call from root's closure into these carriers targets an entry the owning piece admits; the wrapper's owned calls are task-composition-owned-calls; qualified by root's call sequence and the committed submission records in the composed comparison")
  [HostCheck "root-closure-enters-carriers-at-entries" closureEntersAtEntries]

-- | The producer's closure calls the critical pair, and every call into an owned carrier targets an admitted entry.
producerEntersAtEntries :: Producers.Producer -> JoinEvidence -> Bool
producerEntersAtEntries producer evidence = any (\(target, _) -> target >= 0x7235d8 && target < 0x723790) calls && all (uncurry elem) calls
  where calls = entering (Producers.closureCalls producer (targetOf (evidenceCanonical evidence) Image4))

producerServices :: JoinKind -> PieceKind -> Producers.Producer -> Join JoinKind
producerServices kind piece producer = Join kind (Set.fromList [piece, Record2Link])
  [at Image4 0x7235d8 0x723790]
  (ObservationQualified ("the producer's retained services reach the critical-pair entry words 0x7235d8 and 0x7236b0 of the record-2 link; " <> joinName kind <> "-closure-enters-carriers-at-entries checks that every direct call from the producer's closure into the wrapper, critical-pair and storage carriers targets an entry the owning piece admits (critical-pair words: replacement/startup_artifact/Replacement/StartupArtifact/Adapters.hs:199-200); qualified by the producer's call sequence and reads in the composed comparison"))
  [HostCheck (joinName kind <> "-closure-enters-carriers-at-entries") (producerEntersAtEntries producer)]

image5ProducerServices, plainProducerServices :: Join JoinKind
image5ProducerServices = producerServices Image5ProducerServices Image5Producer Producers.Image5Load
plainProducerServices = producerServices PlainProducerServices PlainProducer Producers.PlainLoad

-- | The image5 producer decodes the record carrying the owned pump; task1 calls the pump, which calls the stock hooks.
applicationPumpServices :: Join JoinKind
applicationPumpServices = Join ApplicationPumpServices (Set.fromList [ApplicationPump, Image5Producer])
  [at Image5 start end]
  (ObservationQualified "the image5 producer decodes record 5, re-encoded with the owned pump in [0xbfbd20,0xbfbd40), and publishes it before task1 first runs; task1 calls the pump only at 0xc23274 and the pump calls the retained hooks at their stock entries; the call word at 0xc23274 and the hook entries are the pump's footprint, so FootprintOverlap refuses any piece that edits them, and the pump adapter's canonical source hash fixes their stock words; after staging, verifyComposed requires the decoded composed image5 to equal the composition's image5 target, whose carrier is the linked image with NOP padding (ComposedDiffMismatch); qualified by the application phase's pump sequences in the composed comparison")
  []
  where
    (start, end) = Pump.pumpCarrier

task1Artifact :: JoinEvidence -> Maybe T1.Artifact
task1Artifact evidence = case [linked | Task1ActorFacts linked <- map pieceFacts (evidencePieces evidence)] of
  [LinkedBytes bytes symbols] -> either (const Nothing) Just (T1.deriveArtifact bytes symbols)
  _ -> Nothing

task1CarriersMatch :: JoinEvidence -> Bool
task1CarriersMatch evidence = case task1Artifact evidence of
  Just artifact -> all (\(T1.Carrier (T1.CodeAddress low) (T1.CodeAddress high),bytes) ->
    B.take (fromIntegral (high-low)) (B.drop (fromIntegral (low-targetBase Image5)) (targetOf (evidenceComposed evidence) Image5)) == bytes)
    (T1.artifactCarriers artifact)
  Nothing -> False

task1ActorProducer :: Join JoinKind
task1ActorProducer = Join Task1ActorProducer (Set.fromList [Task1Actor,Image5Producer])
  [at Image5 low high | T1.Carrier (T1.CodeAddress low) (T1.CodeAddress high) <- T1.carriers]
  (HostProof "record5 staging publishes the admitted once-duty and actor carriers; composed decoding checks the complete image5")
  [HostCheck "task1-carriers-match-linked-artifact" task1CarriersMatch]

task1ActorPump :: Join JoinKind
task1ActorPump = Join Task1ActorPump (Set.fromList [Task1Actor,ApplicationPump])
  [at Image5 0xbfbd20 0xbfbd24, at Image5 0xc23274 0xc23278]
  (HostProof "the actor calls the separately admitted J8 pump at its fixed entry; its return is the actor's emitted handback site")
  [ HostCheck "task1-carriers-match-linked-artifact" task1CarriersMatch
  , HostCheck "task1-pump-call-and-handback" $ \evidence -> case task1Artifact evidence of
      Just artifact -> case [site | site <- T1.semanticMap artifact, T1.callService site == T1.Pump] of
        [T1.CallSite (T1.CodeAddress pc) _ (T1.CodeAddress handback)] ->
          targetWord (evidenceComposed evidence) Image5 pc == Just (0xeb000000 + ((0xbfbd20-pc-8) `div` 4 `mod` 0x1000000))
            && lookup 0xc23278 (T1.semanticSites artifact) == Just handback
        _ -> False
      Nothing -> False ]
