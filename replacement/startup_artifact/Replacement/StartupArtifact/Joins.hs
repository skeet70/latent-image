{-# LANGUAGE OverloadedStrings #-}
-- | The registered joins.
module Replacement.StartupArtifact.Joins
  ( JoinKind (..), registeredJoins, joinedTasks, submissionStack, submissionStackWith
  , stockDemand, suppliedOf, depthsIn, indirectOf, stockCodeOfEvidence, composedSegments, ownedOf
  , rootTargets, wrapperCallSites, rootUnbounded, image5Unbounded, plainUnbounded
  ) where

import qualified A32 as A
import qualified Data.ByteString as B
import Data.List (nub, sort, sortOn)
import qualified Data.Map.Strict as M
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
import Replacement.StartupArtifact.Application (PlainImage (..), plainImage)
import qualified Replacement.StartupArtifact.StackBounds as SB
import qualified Replacement.TaskSubmission.Admission as WA
import qualified Replacement.TaskSubmission.Artifact as W
import qualified Replacement.Image5Load.Admission as I5A
import qualified Replacement.Image5Load.Artifact as I5
import qualified Replacement.PlainLoad.Admission as PLA
import qualified Replacement.PlainLoad.Artifact as PL
import qualified StackWalk as W
import qualified StartupStageContract as Stage
import qualified StockStack as SS

data JoinKind = FirstStageSecondaryRendezvous | CriticalPairPublisherBindings | PrefixOrchestrationB | FirstStageCompletionZero
  | LoaderStartupPublisher | PublisherInstallerRoots | CriticalPairInstallerRoots | DispatcherCriticalPair
  | TaskCompositionOwnedCalls | DispatcherTaskComposition | PublisherTaskServices
  | DispatcherRootEntry | DispatcherIdleEntry | WrapperIdleErrorCallback | RootTaskServices
  | Image5ProducerServices | PlainProducerServices
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
  , image5ProducerServices, plainProducerServices ]

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
  (ObservationQualified "the owned wrapper calls the record-2 critical pair through the entry words at 0x7235d8 and 0x7236b0 and the storage transition through the retained prelude at 0x725c6c; its call targets are wrapper-admission constants (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:93-97), call-SP alignment, live words and continuations are checked by that admission's external calls, and the storage adapter ABI by the storage admission (replacement/task_storage/Replacement/TaskStorage/Artifact.hs:118-125); owned-calls-join derives each owned callee's frame low at the wrapper's actual call SPs, which submission-joined-stack-in-boot-window bounds, and with admitted pieces fails only on a missing owned call; on core 1 the wrapper and the storage prelude load root-1 slots that the record-2 initializer publishes; exit receiving enter's token and the reservation arguments are source properties qualified by a composed natural comparison; over the composed images outside owned code, wrapper-callers-closed pins the stock calls of 0x725dd8 and finds no formed address of the wrapper, prelude, release or cleanup, storage-prelude-sole-caller finds no other entry into [0x725c6c,0x725dd8), wrapper-caller-ids finds every stock descriptor id an immediate in 1..120 except the data-driven sites 0x6b07f0 and 0xe9231c, and error-callback-writers finds the only store to the error callback word 0x4089ea8c at 0x725b64 in dispatch_set_error_callback, which no stock code calls")
  [ HostCheck "owned-calls-join" (isJust . joinedTasks)
  , HostCheck "table-b-service-slots-stored" tableBSlotsStored
  , HostCheck "wrapper-callers-closed" wrapperCallersClosed
  , HostCheck "storage-prelude-sole-caller" storagePreludeSoleCaller
  , HostCheck "wrapper-caller-ids" wrapperCallerIds
  , HostCheck "error-callback-writers" errorCallbackWriters ]

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
  (ObservationQualified "the owned dispatcher hands D = S-44 to the owned wrapper at S-64 on each core's boot stack; the descriptor contents, r0 and the descriptor stores before each call are the dispatcher's admission (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206-211,225,233-237), tables A and B are filled before the first dispatch by the orchestration admission (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:219-240,261), and the class roots are the installer's stores of the loader headers (replacement/kernel_install/Replacement/KernelInstall/Admission.hs RootWrite); the wrapper's retained callee demands are StockStack depths (host/firmware/StockStack.hs), error dispatch at the deeper of callbacks 0x723330 and 0x72835c, conditional on fixed published slot words, the error-callback set, fixed context lock methods and no task-exception routine; qualified by a composed natural comparison")
  [ HostCheck "submission-joined-stack-in-boot-window" $ \evidence ->
      case (submissions evidence, joinedTasks evidence, stockDemand evidence) of
        (Just calls@(_ : _), Just joined, Just demand) -> all (\call -> either (const False) (const True) (submissionStackWith demand joined call)) calls
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

-- | Bytes an IRQ that preempts a task leaves below the task's own frame: the entry frame, the return
-- dispatch's saved context and the dispatch to the switch (StackBounds.irqPreemptionPeak).
irqFrameBytes :: Integer
irqFrameBytes = toInteger SB.irqPreemptionBytes

-- | The task's one requested stack holds its admitted frame and the IRQ preemption peak.
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
  (HostProof "the wrapper's error dispatch 0x723398 pushes 8 bytes and calls the callback (replacement/task_submission/Replacement/TaskSubmission/Admission.hs errorCallbackDemand); with callback 0x69cbf0 the demand is 8 plus the idle admission's frame (replacement/idle_task/Replacement/IdleTask/Admission.hs:157-171), and every dispatcher submission's joined stack with that demand fits its boot-stack window; dispatcher-task-composition charges error dispatch at the deeper of callbacks 0x723330 and 0x72835c; this check adds the idle body 0x69cbf0 as a callback, which error-callback-writers excludes on the natural route")
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
  (ObservationQualified "root's retained submitters (0x69ca24 through 0x6b0764, and the constructors under 0x69ca44) call the owned wrapper at 0x725dd8, its cleanup 0x72612c can call owned release 0x726098, and the stock wrapper body and callback 0x69cc18 reach the critical pair and, through the retained prelude 0x725c6c, the storage transition; root-closure-enters-carriers-at-entries checks that every direct call from root's closure into these carriers targets an entry the owning piece admits; the wrapper's owned calls are task-composition-owned-calls; root-cleanup-skips-release shows root's cleanup path has no call of release 0x726098 (its slot-target closures conditional on fixed slot words); root-retained-depth bounds retained callee demand below root's frame with StockStack (host/firmware/StockStack.hs), with the 88-byte IRQ preemption peak, conditional on fixed slot words, the error and trace callback sets, fixed context lock methods and no task-exception routine, except the chains it pins; qualified by root's call sequence and the committed submission records in the composed comparison")
  [ HostCheck "root-closure-enters-carriers-at-entries" closureEntersAtEntries
  , HostCheck "root-cleanup-skips-release" rootCleanupSkipsRelease
  , HostCheck "root-retained-depth" rootRetainedDepth ]

-- | The producer's closure calls the critical pair, and every call into an owned carrier targets an admitted entry.
producerEntersAtEntries :: Producers.Producer -> JoinEvidence -> Bool
producerEntersAtEntries producer evidence = any (\(target, _) -> target >= 0x7235d8 && target < 0x723790) calls && all (uncurry elem) calls
  where calls = entering (Producers.closureCalls producer (targetOf (evidenceCanonical evidence) Image4))

producerServices :: JoinKind -> PieceKind -> Producers.Producer -> Join JoinKind
producerServices kind piece producer = Join kind (Set.fromList [piece, Record2Link])
  [at Image4 0x7235d8 0x723790]
  (ObservationQualified ("the producer's retained services reach the critical-pair entry words 0x7235d8 and 0x7236b0 of the record-2 link; " <> joinName kind <> "-closure-enters-carriers-at-entries checks that every direct call from the producer's closure into the wrapper, critical-pair and storage carriers targets an entry the owning piece admits (critical-pair words: replacement/startup_artifact/Replacement/StartupArtifact/Adapters.hs:199-200); producer-retained-depth bounds retained callee demand below the owned frame with StockStack (host/firmware/StockStack.hs), with the 88-byte IRQ preemption peak, conditional as the piece premise states, except the chains it pins" <> fillText <> "; qualified by the producer's call sequence and reads in the composed comparison"))
  ([ HostCheck (joinName kind <> "-closure-enters-carriers-at-entries") (producerEntersAtEntries producer)
   , HostCheck "producer-retained-depth" (producerRetainedDepth producer) ]
   ++ [HostCheck "fill-flag-writers" fillFlagWriters | producer == Producers.Image5Load])
  where
    fillText = case producer of
      Producers.Image5Load -> "; fill-flag-writers finds the only stock store to 0x89f8ac at 0x6a8c00 in storage_request_refresh 0x6a8bec, whose address only the producer forms"
      Producers.PlainLoad -> ""

image5ProducerServices, plainProducerServices :: Join JoinKind
image5ProducerServices = producerServices Image5ProducerServices Image5Producer Producers.Image5Load
plainProducerServices = producerServices PlainProducerServices PlainProducer Producers.PlainLoad

-- J7: stock stack bounds and caller and writer scans over the composed images.

-- | The composed loaders, images 4 and 5 and the plain window, and every owned carrier's range.
composedSegments :: JoinEvidence -> [SB.Segment]
composedSegments evidence = SB.segmentsOf (evidenceComposed evidence) plain
  where plain = either (const Nothing) (\(PlainImage (Guest load) bytes) -> Just (load, bytes)) (plainImage (evidenceParts evidence))

ownedOf :: JoinEvidence -> [(Word32, Word32)]
ownedOf evidence = SB.ownedRanges (concatMap pieceCarriers (evidencePieces evidence))

stockCodeOfEvidence :: JoinEvidence -> SS.StockCode
stockCodeOfEvidence evidence = SB.stockCodeOf (composedSegments evidence) (concatMap pieceCarriers (evidencePieces evidence))

-- | Table A as the owned publisher emits it, table B as the canonical loader-B initializer stores it,
-- each at its loader header's root.
indirectOf :: JoinEvidence -> Maybe SS.IndirectTargets
indirectOf evidence = do
  rootA <- targetWord (evidenceCanonical evidence) LoaderA rootHeader
  rootB <- targetWord (evidenceCanonical evidence) LoaderB 0xd0010
  emitted <- publications evidence
  stores <- either (const Nothing) Just (I.originalStores (targetOf (evidenceCanonical evidence) LoaderB))
  let tableA = [(guest, publicationValue row) | row <- emitted, let Guest guest = publicationGuest row]
      tableB = [(guest, value) | I.ExpectedStore (P.Guest guest) value <- stores]
  either (const Nothing) Just (SB.indirectTargetsOf (stockCodeOfEvidence evidence) (SB.rootSlotTargets rootA tableA rootB tableB))

-- | Owned entries with proved depths, before the wrapper: the record-2 critical pair, idle and root.
baseSupplied :: JoinEvidence -> M.Map Word32 SS.StackDepth
baseSupplied evidence = M.fromList
  ([ (entry, SS.StackDepth (fromIntegral depth)) | Just (StackDepth depth) <- [criticalFrame evidence], entry <- [0x7235d8, 0x7236b0] ]
   ++ [ (entry, SS.StackDepth (fromIntegral (negate low))) | Just (entry, low) <- [idleFrame evidence, rootFrame evidence] ])

-- | The StockStack depths of retained entries, sharing callee results, conditions included.
depthsIn :: JoinEvidence -> M.Map Word32 SS.StackDepth -> [Word32] -> Maybe [(Word32, Either SS.StackRefusal SS.StackReport)]
depthsIn evidence supplied entries = do
  indirect <- indirectOf evidence
  pure (SS.stockDepths (stockCodeOfEvidence evidence) (SS.Supplied supplied) indirect SB.switchModel entries)

-- | Retained fixed services of the wrapper by address (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:93-97).
retainedFixed :: [(WA.FixedService, Word32)]
retainedFixed = [ (service, address) | address <- [0x7221c8, 0x42634, 0x41a34, 0x723398, 0x427d4, 0x427fc]
                , Just (WA.FixedService service) <- [WA.fixed address] ]

-- | The wrapper's retained callee demands as StockStack depths: fixed services at their addresses,
-- root-slot services at the deeper of their table-A and table-B targets.
stockDemand :: JoinEvidence -> Maybe (WA.CallTarget -> WA.CalleeDemand)
stockDemand evidence = do
  indirect <- indirectOf evidence
  let slotServices = [ (target, M.findWithDefault [] (SS.SlotOffset offset) (SS.rootSlots indirect)) | (WA.RootOffset offset, target) <- WA.rootServiceOffsets ]
  results <- depthsIn evidence (baseSupplied evidence) (map snd retainedFixed ++ concatMap snd slotServices)
  let depth address = case lookup address results of
        Just (Right report) -> let SS.StackDepth bytes = SS.reportDepth report in Just bytes
        _ -> Nothing
  fixedDepths <- mapM (\(service, address) -> (,) (WA.FixedService service) <$> depth address) retainedFixed
  slotDepths <- mapM (\(target, services) -> do
      depths <- mapM depth services
      if null depths then Nothing else pure (target, maximum depths)) slotServices
  let table = fixedDepths ++ slotDepths
  pure (\target -> maybe (WA.stockCalleeDemand target) (WA.CalleeDemand target . WA.StackOffset . negate . toInteger) (lookup target table))

-- | The owned wrapper's or release's depth below its entry SP with the given demands.
wrapperDepth :: TS.JoinedArtifact -> (WA.CallTarget -> WA.CalleeDemand) -> WA.EntryPoint -> Maybe Word32
wrapperDepth joined demand entry = either (const Nothing) (Just . depthOfLow . WA.joinedLow) (TS.joinStack joined entry context retained)
  where
    context = WA.StackContext 0x10000000 (WA.StackInterval (WA.StackOffset (-0x10000)) (WA.StackOffset 0)) []
    owned = map WA.FixedService [WA.CriticalEnter, WA.CriticalExit, WA.Reservation]
    retained = [ demand target | target <- nub (map WA.callTarget (WA.callBoundaries (W.frameProof (TS.wrapperArtifact joined)))), target `notElem` owned ]
    depthOfLow (WA.StackOffset low) = fromIntegral (negate low)

-- | Every owned entry a retained closure may reach, with its proved depth.
suppliedOf :: JoinEvidence -> Maybe (M.Map Word32 SS.StackDepth)
suppliedOf evidence = do
  demand <- stockDemand evidence
  let base = baseSupplied evidence
  wrapper <- case joinedTasks evidence of
    Nothing -> Just []
    Just joined -> do
      submission <- wrapperDepth joined demand WA.Submission
      release <- wrapperDepth joined demand WA.Release
      pure [(SB.wrapperEntry, SS.StackDepth submission), (SB.releaseEntry, SS.StackDepth release)]
  pure (M.union base (M.fromList wrapper))

-- | Entries whose bound fits the budget below a task's entry SP: the owned frame, the deepest
-- retained callee and the IRQ preemption peak. Refused entries and their refusals must be exactly the pinned list.
retainedFits :: JoinEvidence -> Integer -> Integer -> [(Word32, SS.StackRefusal)] -> [Word32] -> Bool
retainedFits evidence frame budget unbounded entries = case suppliedOf evidence >>= \supplied -> depthsIn evidence supplied (nub entries) of
  Nothing -> False
  Just results ->
    let refused = sortOn fst [(entry, refusal) | (entry, Left refusal) <- results]
        depths = [toInteger bytes | (_, Right report) <- results, let SS.StackDepth bytes = SS.reportDepth report]
     in refused == sortOn fst unbounded && all (\depth -> frame + depth + toInteger SB.irqPreemptionBytes <= budget) depths

-- | Root's call targets from its admitted proof.
rootTargets :: JoinEvidence -> Maybe [Word32]
rootTargets evidence = case [linked | RootTaskFacts linked _ <- map pieceFacts (evidencePieces evidence)] of
  [LinkedBytes image symbols] -> do
    artifact <- either (const Nothing) Just (RT.deriveArtifact image symbols)
    pure (nub [RA.callTarget site | (site, _) <- RA.callPositions (RT.artifactProof artifact)])
  _ -> Nothing

-- | Task 104's requested stack (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206).
rootStackBytes :: Integer
rootStackBytes = 0x400

-- | Root targets whose closures StockStack refuses: the `svc #0x1010` at 0x4360c under 0x6490dc, 0x69cfa4,
-- 0x69f204, 0x6a5920, 0x6a59b8 and 0x6a5c08, and the argument callback 0x772d58 under 0x69ca44.
rootUnbounded :: [(Word32, SS.StackRefusal)]
rootUnbounded = [(0x69ca44, SS.UnresolvedBranch 0x772d58)] ++ [(entry, svc) | entry <- [0x6490dc, 0x69cfa4, 0x69f204, 0x6a5920, 0x6a59b8, 0x6a5c08]]

svc :: SS.StackRefusal
svc = SS.Refusal 0x4360c W.SupervisorCall

rootRetainedDepth :: JoinEvidence -> Bool
rootRetainedDepth evidence = case (rootFrame evidence, rootTargets evidence) of
  (Just (_, low), Just targets) -> retainedFits evidence (toInteger (negate low)) rootStackBytes rootUnbounded targets
  _ -> False

-- | The r0 = 0 path of 0x72612c: @cmp r6, #0; cmpne r6, r0; bne 0x7261d0@, no @bl@ in [0x72616c, 0x7261d0),
-- and the closures of its four root-slot calls (+0x84, +0x164, +0x4, +0x14) do not reach release.
rootCleanupSkipsRelease :: JoinEvidence -> Bool
rootCleanupSkipsRelease evidence = unchanged && guarded && noCall && closuresAvoid
  where
    composed = evidenceComposed evidence
    word = targetWord composed Image4
    unchanged = all (\address -> word address == targetWord (evidenceCanonical evidence) Image4 address && isJust (word address)) [0x72612c, 0x726130 .. 0x7261cc]
    decoded address = A.decode address <$> word address
    guarded = case (decoded 0x726160, decoded 0x726164, decoded 0x726168) of
      (Just (A.DataProcessing (A.Cond 14) A.CMP True _ r6 (A.Immediate 0)), Just (A.DataProcessing (A.Cond 1) A.CMP True _ r6' (A.ShiftedImmediate r0 A.LSL 0)), Just (A.Branch (A.Cond 1) False 0x7261d0)) ->
        r6 == A.Reg 6 && r6' == A.Reg 6 && r0 == A.Reg 0
      _ -> False
    noCall = and [case decoded address of Just (A.Branch _ True _) -> False; Just _ -> True; Nothing -> False | address <- [0x72616c, 0x726170 .. 0x7261cc]]
    closuresAvoid = case (indirectOf evidence, suppliedOf evidence) of
      (Just indirect, Just supplied) ->
        let marked = M.insert SB.releaseEntry (SS.StackDepth 0) supplied
            targets = concat [M.findWithDefault [] (SS.SlotOffset slot) (SS.rootSlots indirect) | slot <- [0x84, 0x164, 0x4, 0x14]]
         in not (null targets) && all (\target -> case SS.stockDepth (stockCodeOfEvidence evidence) (SS.Supplied marked) indirect SB.switchModel target of
              Right report -> SB.releaseEntry `notElem` SS.reportSupplied report
              Left _ -> False) targets
      _ -> False

-- | task60's allocation below the producers' entry SP: 0xa1c1e0 - 0xa1b600 (the premised base, Adapters.hs producerWritesPremise).
task60Budget :: Integer
task60Budget = 0xa1c1e0 - 0xa1b600

-- | Producer services whose closures StockStack refuses: the argument callback 0x6f16e0 under storage read
-- 0x6a8f1c, the message-trace callback word under progress 0x73b924, and the `svc` at 0x4360c under 0x69b9c4.
image5Unbounded, plainUnbounded :: [(Word32, SS.StackRefusal)]
image5Unbounded = [(0x6a8f1c, SS.UnresolvedBranch 0x6f16e0)]
plainUnbounded = [(0x69b9c4, svc), (0x6a8f1c, SS.UnresolvedBranch 0x6f16e0), (0x73b924, SS.UnresolvedBranch 0x7250cc)]

producerFrame :: Producers.Producer -> JoinEvidence -> Maybe Int
producerFrame producer evidence = case (producer, [facts | facts <- map pieceFacts (evidencePieces evidence), matches facts]) of
  (Producers.Image5Load, [Image5ProducerFacts (LinkedBytes image symbols)]) -> I5A.scratchLow . I5.artifactProof <$> either (const Nothing) Just (I5.deriveArtifact image symbols)
  (Producers.PlainLoad, [PlainProducerFacts (LinkedBytes image symbols)]) -> PLA.scratchLow . PL.artifactProof <$> either (const Nothing) Just (PL.deriveArtifact image symbols)
  _ -> Nothing
  where
    matches facts = case (producer, facts) of
      (Producers.Image5Load, Image5ProducerFacts _) -> True
      (Producers.PlainLoad, PlainProducerFacts _) -> True
      _ -> False

producerRetainedDepth :: Producers.Producer -> JoinEvidence -> Bool
producerRetainedDepth producer evidence = case producerFrame producer evidence of
  Just low -> retainedFits evidence (toInteger (negate low)) task60Budget unbounded services
  Nothing -> False
  where
    (services, unbounded) = case producer of
      Producers.Image5Load -> (I5A.retainedTargets, image5Unbounded)
      Producers.PlainLoad -> (PLA.retainedTargets, plainUnbounded)

-- | The stock calls of the wrapper in the canonical images (63 outside the dispatcher carrier; the
-- dispatcher's four are its admitted submissions).
wrapperCallSites :: [Word32]
wrapperCallSites =
  [ 0x69c3e0, 0x69c6dc, 0x69ce88, 0x69cee4, 0x69cf34, 0x69cf6c, 0x69f068, 0x6b0010, 0x6b07f0, 0x6b15d0, 0x6b6954, 0x6b790c
  , 0x6bda48, 0x6c0930, 0x6c5994, 0x6d22a0, 0x6d2924, 0x6d48b0, 0x6d7ff8, 0x6e2f7c, 0x6e2fe8, 0x6e3054, 0x6ea07c, 0x6eaf28
  , 0x71fdb0, 0x7218b0, 0x73b0a4, 0x73bfd4, 0x73c144, 0x76fe04, 0x8923a8, 0x8923f4, 0xd0a2a4, 0xd80db4, 0xdb644c, 0xdca688
  , 0xe9231c, 0xec53bc, 0xed1120, 0xed2b6c, 0xef8d1c, 0xf494d4, 0xf49be4, 0xf49e24, 0xfe9094, 0x1039564, 0x1050f6c, 0x1051f98
  , 0x105437c, 0x1055d78, 0x10620f8, 0x10662e8, 0x1072764, 0x110f9cc, 0x11e00ec, 0x125a254, 0x125f160, 0x127461c, 0x1279d2c
  , 0x127ae58, 0x127c060, 0x127ce0c, 0x127dcb8, 0x127fa1c, 0x1281554, 0x13e4d80, 0x13ed5a0 ]

-- | Wrapper call sites whose descriptor id is data (cpu_create_workers' table, 0xe922bc's argument).
dataIdSites :: [Word32]
dataIdSites = [0x6b07f0, 0xe9231c]

outsideOwned :: JoinEvidence -> [Word32] -> [Word32]
outsideOwned evidence = filter (\site -> not (any (\(low, high) -> low <= site && site < high) (ownedOf evidence)))

wrapperCallersClosed :: JoinEvidence -> Bool
wrapperCallersClosed evidence =
  sort (map fst (SB.branchSites segments owned (== SB.wrapperEntry))) == sort (outsideOwned evidence wrapperCallSites)
    && null (SB.addressFormations segments owned (`elem` [SB.wrapperEntry, SB.storagePrelude, SB.releaseEntry, SB.cleanupEntry]))
  where
    segments = composedSegments evidence
    owned = ownedOf evidence

storagePreludeSoleCaller :: JoinEvidence -> Bool
storagePreludeSoleCaller evidence =
  null [() | (site, _) <- SB.branchSites segments owned inPrelude, not (site >= SB.storagePrelude && site < SB.cleanupEntry)]
    && null (SB.addressFormations segments owned inPrelude)
    && maybe False (\joined -> WA.FixedService WA.Reservation `elem` map WA.callTarget (WA.callBoundaries (W.frameProof (TS.wrapperArtifact joined)))) (joinedTasks evidence)
  where
    segments = composedSegments evidence
    owned = ownedOf evidence
    inPrelude address = address >= SB.storagePrelude && address < SB.wrapperEntry

wrapperCallerIds :: JoinEvidence -> Bool
wrapperCallerIds evidence =
  all (\(_, identity) -> case identity of SB.ImmediateId value -> value >= 1 && value <= 120; SB.DataId -> True) ids
    && sort [site | (site, SB.DataId) <- ids] == sort (outsideOwned evidence dataIdSites)
  where ids = SB.descriptorIds (composedSegments evidence) (ownedOf evidence)

errorCallbackWriters :: JoinEvidence -> Bool
errorCallbackWriters evidence =
  SB.storesReaching segments owned SB.errorCallbackWord == outsideOwned evidence [0x725b64]
    && null (SB.branchSites segments owned (== SB.errorCallbackSetter))
    && null (SB.addressFormations segments owned (== SB.errorCallbackSetter))
  where
    segments = composedSegments evidence
    owned = ownedOf evidence

fillFlagWriters :: JoinEvidence -> Bool
fillFlagWriters evidence =
  SB.storesReaching segments owned SB.fillFlag == outsideOwned evidence [0x6a8c00]
    && null (SB.branchSites segments owned (== SB.fillDoneCallback))
    && null (SB.addressFormations segments owned (== SB.fillDoneCallback))
  where
    segments = composedSegments evidence
    owned = ownedOf evidence
