{-# LANGUAGE OverloadedStrings #-}
-- | The registered joins.
module Replacement.StartupArtifact.Joins
  ( JoinKind (..), registeredJoins, joinedTasks, submissionStack, submissionStackWith
  , kernelSlotSupplied
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
import qualified Replacement.StartupArtifact.Footprint.Producers as Producers
import qualified Replacement.StartupArtifact.Footprint.RootTask as RootFootprint
import qualified Replacement.TaskSubmission.Admission as WA
import qualified Replacement.TaskSubmission.Artifact as W
import qualified StartupStageContract as Stage
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import qualified Data.Text as T
import qualified Emulation.PrimaryInitializerSource as PS
import qualified Replacement.RtosOwnFirst.Pins as OP
import qualified Replacement.RtosOwnFirst.Regions as OR
import qualified Replacement.StartupArtifact.KernelSlots as KS

data JoinKind = FirstStageSecondaryRendezvous | CriticalPairPublisherBindings | PrefixOrchestrationB | FirstStageCompletionZero
  | LoaderStartupPublisher | PublisherInstallerRoots | CriticalPairInstallerRoots | DispatcherCriticalPair
  | TaskCompositionOwnedCalls | DispatcherTaskComposition | PublisherTaskServices
  | DispatcherRootEntry | DispatcherIdleEntry | WrapperIdleErrorCallback | RootTaskServices
  | Image5ProducerServices | PlainProducerServices
  | KernelSlotPrimaryPublisher | KernelSlotLoaderStartup | KernelSlotLoaderOrchestrationB | KernelSlotRecord2Link
  | KernelSlotTaskSubmission | KernelSlotTaskStorage | KernelSlotRootTask | KernelSlotImage5Producer | KernelSlotPlainProducer
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
  joinName KernelSlotPrimaryPublisher = "kernel-slot-primary-publisher"
  joinName KernelSlotLoaderStartup = "kernel-slot-loader-startup"
  joinName KernelSlotLoaderOrchestrationB = "kernel-slot-loader-orchestration-b"
  joinName KernelSlotRecord2Link = "kernel-slot-record2-link"
  joinName KernelSlotTaskSubmission = "kernel-slot-task-submission"
  joinName KernelSlotTaskStorage = "kernel-slot-task-storage"
  joinName KernelSlotRootTask = "kernel-slot-root-task"
  joinName KernelSlotImage5Producer = "kernel-slot-image5-producer"
  joinName KernelSlotPlainProducer = "kernel-slot-plain-producer"

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
  , image5ProducerServices, plainProducerServices ] ++ kernelSlotJoins

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

-- * kernel-slot-bodies

kernelSlotFacts :: JoinEvidence -> Maybe ([KernelSlot], [KernelSlotBody])
kernelSlotFacts evidence = case [(slots, bodies) | KernelSlotFacts slots bodies <- map pieceFacts (evidencePieces evidence)] of
  [facts] -> Just facts
  _ -> Nothing

pieceOf :: PieceKind -> JoinEvidence -> Maybe Piece
pieceOf kind evidence = find ((== kind) . pieceKind) (evidencePieces evidence)

-- | A loader's slot extents as the own-first pins name them, merged.
slotCarriers :: Target -> [Span]
slotCarriers target = normalizeSpans
  [ at target start end | domain <- OR.domains, loaderTarget domain == target, let pinned = OP.stockPins domain
  , (_, OR.Extent (OR.GuestAddress start) (OR.GuestAddress end)) <- OP.pinRows pinned ++ [OP.pinTail pinned] ]
  where
    loaderTarget OR.DomainA = LoaderA
    loaderTarget OR.DomainB = LoaderB

-- | A loader's +0x164 and +0x168 extents and its +0x1618 entry extent.
serviceSlots :: Target -> [Span]
serviceSlots target = [at target (base + 0x1140) (base + 0x118c), at target (base + 0x12bc) (base + 0x12cc)]
  where base = targetBase target

slotEntries :: [KernelSlot] -> [Word32]
slotEntries slots = [entry | KernelSlot {kernelSlotEntry = Pc entry} <- slots]

-- | Direct branches into the slot extents: from the piece's footprint in the canonical targets, and from its
-- carriers in the composed targets.
enteringCalls :: PieceKind -> JoinEvidence -> Maybe ([KS.DirectCall], [KS.DirectCall])
enteringCalls kind evidence = do
  piece <- pieceOf kind evidence
  (slots, _) <- kernelSlotFacts evidence
  let extents = map kernelSlotExtent slots
  pure ( KS.directCalls (targetOf (evidenceCanonical evidence)) [range | Footprint range _ _ <- pieceFootprint piece] extents
       , KS.directCalls (targetOf (evidenceComposed evidence)) (pieceCarriers piece) extents )

callsEnterAtEntries :: PieceKind -> HostCheck
callsEnterAtEntries kind = HostCheck (pieceKindName kind <> "-calls-enter-slot-entries") $ \evidence ->
  case (enteringCalls kind evidence, kernelSlotFacts evidence) of
    (Just (stock, owned), Just (slots, _)) -> all ((`elem` slotEntries slots) . KS.directTarget) (stock ++ owned)
    _ -> False

-- | The word at a guest address of whichever canonical target holds it.
canonicalWord :: JoinEvidence -> Word32 -> Maybe Word32
canonicalWord evidence address = listToMaybe [word | target <- [minBound .. maxBound], Just word <- [targetWord (evidenceCanonical evidence) target address]]

-- | Each direct call and root-slot call in the piece's footprint, from its return site, reads none of the
-- caller-saved state the entered bodies may change before overwriting it. A root-slot call may enter either
-- loader's body of its slot.
callersScratchDead :: PieceKind -> HostCheck
callersScratchDead kind = HostCheck (pieceKindName kind <> "-slot-callers-leave-clobbers-dead") $ \evidence ->
  case (pieceOf kind evidence, enteringCalls kind evidence, kernelSlotFacts evidence) of
    (Just piece, Just (stock, _), Just (slots, _)) ->
      let word = canonicalWord evidence
          clobbersAt entry = concat [kernelSlotClobbers slot | slot <- slots, kernelSlotEntry slot == Pc entry]
          clobbersOf offset = nub (concat [kernelSlotClobbers slot | slot <- slots, kernelSlotOffset slot == offset])
          offsets = map kernelSlotOffset slots
          spans = [range | Footprint range _ _ <- pieceFootprint piece]
          direct = [KS.deadAfter word (slotEntries slots) offsets (clobbersAt (KS.directTarget call)) (KS.directSite call + 4) | call <- stock, KS.directLinked call]
          indirect = [ KS.deadAfter word (slotEntries slots) offsets (clobbersOf offset) (site + 4)
                     | KS.SlotCall site offset <- KS.slotCalls (targetOf (evidenceCanonical evidence)) spans, offset `elem` offsets ]
      in and (direct ++ indirect)
    _ -> False

-- | Every table-A word the canonical or owned publisher stores and every table-B word the canonical initializer
-- stores that lies in a slot extent names an entry, and the +0x1618, +0x164 and +0x168 words of both roots do.
publishedSlotEntries :: HostCheck
publishedSlotEntries = HostCheck "published-slot-words-name-entries" $ \evidence ->
  case ( kernelSlotFacts evidence, PS.publications (targetOf (evidenceCanonical evidence) LoaderA)
       , I.originalStores (targetOf (evidenceCanonical evidence) LoaderB)
       , targetWord (evidenceCanonical evidence) LoaderA rootHeader, targetWord (evidenceCanonical evidence) LoaderB 0xd0010 ) of
    (Just (slots, _), Right (stock, _), Right stores, Just rootA, Just rootB) ->
      let physicalOf address = physicalAddress (fromIntegral address)
          tableA = [(physicalOf guest, value) | PS.Publication (PS.Guest guest) value _ <- stock]
            ++ [(physicalOf guest, value) | Publication (Guest guest) value <- maybe [] id (publications evidence)]
          tableB = [(physicalOf guest, value) | I.ExpectedStore (P.Guest guest) value <- stores]
          entries = slotEntries slots
          inSlot value = any (\(Span _ (Guest low) (Guest high)) -> value >= low && value < high) (map kernelSlotExtent slots)
          named root table offset = case [value | (guest, value) <- table, guest == physicalOf (root + offset)] of
            values@(_ : _) -> all (`elem` entries) values
            [] -> False
      in all (\(_, value) -> not (inSlot value) || value `elem` entries) (tableA ++ tableB)
           && and [named root table offset | (root, table) <- [(rootA, tableA), (rootB, tableB)], offset <- [0x1618, 0x164, 0x168]]
    _ -> False

-- | The composed target differs from canonical only inside the composition's carriers.
differsOnlyInCarriers :: Target -> HostCheck
differsOnlyInCarriers target = HostCheck (targetName target <> "-differs-only-in-carriers") $ \evidence ->
  all (`coveredBy` concatMap pieceCarriers (evidencePieces evidence))
    (diffSpans target (targetOf (evidenceCanonical evidence) target) (targetOf (evidenceComposed evidence) target))

slotFrames :: [KernelSlot] -> Word32 -> [Word32]
slotFrames slots offset = [depth | KernelSlot {kernelSlotOffset = RootSlot slot, kernelSlotFrame = StackDepth depth} <- slots, slot == offset]

-- | Bytes the record-2 critical pair leaves a retained slot: its bridge charges 24 below its call SP and pushes
-- 16 of them (emulation/Emulation/CriticalPair.hs:532; replacement/critical_pair/entry.S:60).
criticalSlotBytes :: Word32
criticalSlotBytes = 8

-- | Both loaders' +0x1618, +0x164 and +0x168 bodies fit the critical pair's slot budget, and no +0x164 body
-- changes r3, which the entry adapter holds the +0x164 target in across the call.
criticalSlotsFit :: HostCheck
criticalSlotsFit = HostCheck "critical-pair-slot-frames-and-r3" $ \evidence ->
  case kernelSlotFacts evidence of
    Just (slots, _) ->
      all (\offset -> let frames = slotFrames slots offset in length frames == 2 && all (<= criticalSlotBytes) frames) [0x1618, 0x164, 0x168]
        && not (any (\slot -> kernelSlotOffset slot == RootSlot 0x164 && ClobberR3 `elem` kernelSlotClobbers slot) slots)
    Nothing -> False

-- | The retained storage prelude's push at 0x725c6c, whose depth its +0x1618, +0x164 and epilogue +0x168 calls run at.
storagePreludeBytes :: JoinEvidence -> Maybe Word32
storagePreludeBytes evidence = case A.decode 0x725c6c <$> targetWord (evidenceCanonical evidence) Image4 0x725c6c of
  Just (A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers) | base == A.sp -> Just (4 * fromIntegral (length registers))
  _ -> Nothing

-- | The prelude depth plus each owned slot frame fits the reservation demand the wrapper charges for the storage call.
storageSlotsFit :: HostCheck
storageSlotsFit = HostCheck "storage-prelude-slot-frames-within-reservation-demand" $ \evidence ->
  case (kernelSlotFacts evidence, storagePreludeBytes evidence, WA.stockCalleeDemand (WA.FixedService WA.Reservation)) of
    (Just (slots, _), Just prelude, WA.CalleeDemand _ (WA.StackOffset low)) ->
      all (\offset -> let frames = slotFrames slots offset in length frames == 2 && all (\frame -> toInteger (prelude + frame) <= negate low) frames)
        [0x1618, 0x164, 0x168]
    _ -> False

-- | Retained wrapper demands with each root-slot service charged the larger of its stock demand and its owned bodies' frames.
ownedSlotDemand :: [KernelSlot] -> WA.CallTarget -> WA.CalleeDemand
ownedSlotDemand slots target = case lookup target [(WA.CurrentCoreService, 0x1618), (WA.SaveMaskService, 0x164), (WA.RestoreMaskService, 0x168)] of
  Just offset ->
    let WA.CalleeDemand _ (WA.StackOffset stock) = WA.stockCalleeDemand target
    in WA.CalleeDemand target (WA.StackOffset (negate (maximum (negate stock : map toInteger (slotFrames slots offset)))))
  Nothing -> WA.stockCalleeDemand target

slotDemandSubmissionStack :: HostCheck
slotDemandSubmissionStack = HostCheck "submission-stack-with-owned-slot-frames" $ \evidence ->
  case (submissions evidence, joinedTasks evidence, kernelSlotFacts evidence) of
    (Just calls@(_ : _), Just joined, Just (slots, _)) ->
      all (either (const False) (const True) . submissionStackWith (ownedSlotDemand slots) joined) calls
    _ -> False

-- | Each owned slot entry and +0x1618 body with its admitted frame, for a stack walk that does not enter owned code.
kernelSlotSupplied :: JoinEvidence -> M.Map Word32 StackDepth
kernelSlotSupplied evidence = case kernelSlotFacts evidence of
  Just (slots, bodies) -> M.fromList
    ([(entry, frame) | KernelSlot {kernelSlotEntry = Pc entry, kernelSlotFrame = frame} <- slots]
     ++ [(start, frame) | KernelSlotBody _ (Span _ (Guest start) _) frame <- bodies])
  Nothing -> M.empty

slotsText :: T.Text
slotsText = "the owned slot bodies are linked at the stock entries inside the stock extents (replacement/rtos_own_first/stage/Replacement/RtosOwnFirst/Artifact.hs:321-322) and equal stock per slot on the native-case grid and the natural route (E-RTOS-OWN-FIRST-ORACLE-150, E-RTOS-OWN-FIRST-NATURAL-150); each body restores r4-r11 and SP and returns to its entry LR, and its facts list the caller-saved state it may change (replacement/startup_artifact/Replacement/StartupArtifact/KernelSlots.hs walkAbi); published-slot-words-name-entries checks that the canonical and owned tables name entries"

rebaseText :: T.Text
rebaseText = "a whole-loader hash taken at admission is over the canonical loader and re-bases onto the composed loader as its canonical bytes outside the composition's carriers, which loader_a-differs-only-in-carriers and loader_b-differs-only-in-carriers check, plus each carrier's edit, whose digest the contract carries"

reason :: T.Text -> JoinBasis
reason text = ObservationQualified (slotsText <> "; " <> text <> "; qualified by a composed natural comparison")

kernelSlotPublisher :: Join JoinKind
kernelSlotPublisher = Join KernelSlotPrimaryPublisher (Set.fromList [KernelSlotBodies, PrimaryPublisher])
  (slotCarriers LoaderA ++ [at LoaderA 0x40430 0x409ac])
  (reason ("the publisher hashes the canonical loader A (emulation/Emulation/PrimaryInitializerSource.hs:110-113) and its consumer source pins the stock +0x1618 words at 0x412bc (emulation/Emulation/PrimaryInitializerConsumerSource.hs:41); the kernel-slot admission hashes loader A over the publisher carrier; " <> rebaseText
    <> "; the descriptor consumer 0x725b74 calls +0x1618 through root slot 0x1618, and primary-publisher-slot-callers-leave-clobbers-dead checks that it and every direct caller in the publisher's footprint read no state the entered bodies may change"))
  [publishedSlotEntries, callsEnterAtEntries PrimaryPublisher, callersScratchDead PrimaryPublisher, differsOnlyInCarriers LoaderA]

kernelSlotLoaderStartup :: Join JoinKind
kernelSlotLoaderStartup = Join KernelSlotLoaderStartup (Set.fromList [KernelSlotBodies, LoaderStartup])
  (slotCarriers LoaderA ++ slotCarriers LoaderB ++ [at LoaderA loaderAStart 0x40b00, at LoaderB loaderStartup 0xd0b00])
  (reason ("loader-startup hashes both canonical loaders (replacement/startup_dispatch/Replacement/StartupDispatch/Artifact.hs:40-46) and the kernel-slot admission hashes both over the orchestration carriers; " <> rebaseText
    <> "; the orchestration admissions treat retained callees as stock from their entries (replacement/loader_orchestration/Replacement/LoaderOrchestration/Interface.hs:34-48), and loader-startup-calls-enter-slot-entries and loader-startup-slot-callers-leave-clobbers-dead check every direct and root-slot caller in both loaders and the owned orchestration code"))
  [publishedSlotEntries, callsEnterAtEntries LoaderStartup, callersScratchDead LoaderStartup, differsOnlyInCarriers LoaderA, differsOnlyInCarriers LoaderB]

kernelSlotOrchestrationB :: Join JoinKind
kernelSlotOrchestrationB = Join KernelSlotLoaderOrchestrationB (Set.fromList [KernelSlotBodies, LoaderOrchestrationB])
  [at LoaderB loaderStartup 0xd0b00]
  (HostProof ("the kernel-slot admission hashes the canonical loader B over the loader-B orchestration carrier (replacement/rtos_own_first/stage/Replacement/RtosOwnFirst/Artifact.hs:198-203,318); " <> rebaseText
    <> "; loader-orchestration-b-calls-enter-slot-entries checks that the owned orchestration and its retained callee closures enter no slot extent except at an entry"))
  [callsEnterAtEntries LoaderOrchestrationB, callersScratchDead LoaderOrchestrationB, differsOnlyInCarriers LoaderB]

kernelSlotRecord2Link :: Join JoinKind
kernelSlotRecord2Link = Join KernelSlotRecord2Link (Set.fromList [KernelSlotBodies, Record2Link])
  (slotCarriers LoaderB ++ serviceSlots LoaderA ++ [at LoaderB 0xd000c 0xd0010, at LoaderB initializerSite (initializerSite + 4)])
  (reason ("the record-2 admission pins the stock +0x164, +0x168 and +0x1618 words of both canonical loaders (emulation/Emulation/CriticalPair.hs:106-110) and hashes loader B into the prefix facts, retained against the original (emulation/Emulation/LoaderPrefixWitness.hs:446) and owned over the loader it is given (:524); the kernel-slot admission hashes loader B over the header and initializer words; " <> rebaseText
    <> "; the critical-pair entry adapter calls +0x1618 and +0x164 32 bytes into its 64-byte frame and holds the +0x164 target in r3 across the call (replacement/critical_pair/entry.S:7-18; emulation/Emulation/CriticalPair.hs:484,640), its bridge leaves a retained slot 8 bytes (criticalSlotBytes) and its model keeps only the published r0 of a slot call (CriticalPair.hs bridgeSummary), which critical-pair-slot-frames-and-r3 checks against the owned frames and clobbers"))
  [publishedSlotEntries, callsEnterAtEntries Record2Link, callersScratchDead Record2Link, criticalSlotsFit, differsOnlyInCarriers LoaderB]

kernelSlotTaskSubmission :: Join JoinKind
kernelSlotTaskSubmission = Join KernelSlotTaskSubmission (Set.fromList [KernelSlotBodies, TaskSubmission])
  (serviceSlots LoaderA ++ serviceSlots LoaderB)
  (reason "the wrapper calls +0x1618, +0x164 and +0x168 through the root slots (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:103-106) and its admission forgets r0-r3, r12 and lr at every external call (:382), so the owned clobbers meet its model; its stock demand table charges 8, 4 and 4 bytes (:120) where the owned +0x164 and +0x168 frames are 8, and submission-stack-with-owned-slot-frames joins every dispatcher submission with the larger of the two inside its boot-stack window; the retained fixed services' closures reach the +0x164 and +0x168 bodies by direct call, and task-submission-slot-callers-leave-clobbers-dead checks those callers; later callers run on pool-reserved task stacks [INFERENCE]")
  [publishedSlotEntries, callsEnterAtEntries TaskSubmission, callersScratchDead TaskSubmission, slotDemandSubmissionStack]

kernelSlotTaskStorage :: Join JoinKind
kernelSlotTaskStorage = Join KernelSlotTaskStorage (Set.fromList [KernelSlotBodies, TaskStorage])
  (serviceSlots LoaderA ++ serviceSlots LoaderB)
  (reason "the retained storage prelude 0x725c6c calls +0x1618 and +0x164 and its epilogue +0x168 through the root slots (replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207-208) at the prelude's push depth, which storage-prelude-slot-frames-within-reservation-demand adds to each owned frame against the 40-byte reservation demand the wrapper charges (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:117); task-storage-slot-callers-leave-clobbers-dead checks the three call sites")
  [publishedSlotEntries, callsEnterAtEntries TaskStorage, callersScratchDead TaskStorage, storageSlotsFit]

-- | A piece whose retained closure reaches the slot bodies only through loader-A callers.
closureJoin :: JoinKind -> PieceKind -> Join JoinKind
closureJoin kind piece = Join kind (Set.fromList [KernelSlotBodies, piece])
  [at LoaderA 0x41140 0x4118c]
  (reason ("the piece's retained closure reaches the +0x164 and +0x168 bodies by direct calls from loader A, which " <> pieceKindName piece <> "-calls-enter-slot-entries and " <> pieceKindName piece <> "-slot-callers-leave-clobbers-dead check; the owned frames are 8 bytes where stock takes 4, and retained callee depth on the piece's stack is that piece's premise"))
  [publishedSlotEntries, callsEnterAtEntries piece, callersScratchDead piece]

kernelSlotJoins :: [Join JoinKind]
kernelSlotJoins =
  [ kernelSlotPublisher, kernelSlotLoaderStartup, kernelSlotOrchestrationB, kernelSlotRecord2Link, kernelSlotTaskSubmission, kernelSlotTaskStorage
  , closureJoin KernelSlotRootTask RootTask, closureJoin KernelSlotImage5Producer Image5Producer, closureJoin KernelSlotPlainProducer PlainProducer ]
