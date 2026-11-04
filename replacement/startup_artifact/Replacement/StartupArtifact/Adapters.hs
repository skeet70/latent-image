{-# LANGUAGE OverloadedStrings #-}
-- | One composition adapter per piece kind: each runs its area's own admission
-- and staging on the canonical inputs and declares carriers and catalog from
-- the admitted artifact.
module Replacement.StartupArtifact.Adapters
  ( LinkedImage (..), ImageBytes (..), SymbolBytes (..), PieceImages (..)
  , deriveFirstStage, deriveRecord2Link, derivePrimaryPublisher, deriveLoaderOrchestrationB, deriveLoaderStartup, derivePieceOf
  ) where

import Control.Monad (unless)
import Data.Bifunctor (first)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import qualified Emulation.CriticalPair as C
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.PrimaryInitializerArtifact as PA
import qualified Emulation.PrimaryInitializerConsumerSource as PC
import qualified Emulation.PrimaryInitializerSource as PS
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import qualified Replacement.KernelInstall.Admission as KA
import qualified Replacement.KernelInstall.Artifact as KI
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Compose
import qualified Replacement.StartupComposition.Artifact as SC
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupDispatch.Admission as DA
import qualified Replacement.StartupDispatch.Artifact as SD
import qualified Replacement.StartupArtifact.Footprint.FirstStage as FirstStageFootprint
import qualified Replacement.StartupArtifact.Footprint.PrimaryPublisher as PublisherFootprint
import qualified Replacement.StartupArtifact.Footprint.Record2Link as Record2Footprint
import qualified Replacement.StartupArtifact.Footprint.LoaderOrchestration as OrchestrationFootprint
import qualified Replacement.StartupArtifact.Footprint.LoaderStartup as StartupFootprint
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as Stage

newtype ImageBytes = ImageBytes B.ByteString
newtype SymbolBytes = SymbolBytes B.ByteString

-- | A linked flat image and its llvm-nm symbols.
data LinkedImage = LinkedImage ImageBytes SymbolBytes

-- | A manifest piece's loaded inputs.
data PieceImages
  = SingleImage PieceKind LinkedImage
  -- | Loader-A and loader-B orchestration, dispatcher and installer.
  | LoaderStartupImages LinkedImage LinkedImage LinkedImage LinkedImage

derivePieceOf :: Canonical -> PieceImages -> Either Refusal Piece
derivePieceOf source images = case images of
  LoaderStartupImages a b dispatcher installer -> deriveLoaderStartup source a b dispatcher installer
  SingleImage kind image -> case kind of
    FirstStage -> deriveFirstStage source image
    Record2Link -> deriveRecord2Link source image
    PrimaryPublisher -> derivePrimaryPublisher source image
    LoaderOrchestrationB -> deriveLoaderOrchestrationB source image
    LoaderStartup -> Left (AdapterRefused kind "loader-startup takes four linked images")

admitted :: PieceKind -> Either String a -> Either Refusal a
admitted kind = first (AdapterRefused kind . T.pack)

refuseUnless :: PieceKind -> Bool -> String -> Either Refusal ()
refuseUnless kind condition message = unless condition (Left (AdapterRefused kind (T.pack message)))

spanOf :: PieceKind -> Target -> Word32 -> Word32 -> Either Refusal Span
spanOf kind target start end = maybe (Left (AdapterRefused kind "empty carrier")) Right (span' target start end)

codeOf :: PieceKind -> Word32 -> Word32 -> Either Refusal CodeSpan
codeOf kind start end = maybe (Left (AdapterRefused kind "empty owned code span")) Right (codeSpan start end)

limit :: PieceKind -> Int -> Either Refusal RecordLimit
limit kind count = maybe (Left (AdapterRefused kind "record limit below one")) Right (recordLimit count)

hexName :: Word32 -> T.Text
hexName value = T.pack (showHex value "")

coreSet :: PieceKind -> [Core] -> Either Refusal Cores
coreSet kind listed = maybe (Left (AdapterRefused kind "boundary without cores")) Right (cores listed)

-- | Physical spelling of a guest address.
physical :: Word32 -> Word64
physical = physicalAddress . fromIntegral

-- | The 0x40000000 RAM-alias spelling of a low address.
alias :: Word32 -> Word32
alias = (+ 0x40000000)

-- | The record-1 path of host/firmware/BuildComposedStartup.hs.
deriveFirstStage :: Canonical -> LinkedImage -> Either Refusal Piece
deriveFirstStage source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = FirstStage
      admit = admitted kind
  original <- admit (Stage.parseOriginalStartupDat (canonicalDat source))
  artifact <- admit (Stage.parseFirstStageArtifact (U.MediaImage bin) symbolBytes)
  let rows = Stage.firstStageArtifactSymbols artifact
      plan = Stage.firstStageArtifactPlan artifact
      parsed = Stage.originalStartupParsed original
  refuseUnless kind (all ((/= "xm1_first_stage_interleaved_tail_fault") . F.symbolName) rows)
    "interleaved-tail fault image is not a composition piece"
  parts <- admit (F.replaceRecordSlot plan (U.parsedParts parsed))
  staged <- admit (U.rebuildContainer parsed parts)
  final <- admit (Stage.parseStagedStartupDat staged)
  admit (F.verifyStagedRecord plan [] parsed (Stage.stagedStartupParsed final))
  patches <- admit (F.planPatches plan)
  let base = fromIntegral (F.recordGuestBase (F.planRecord plan))
      load = base + fromIntegral (F.slotOffsetBytes (F.planLoadOffset plan))
  carriers <- mapM (\(offset, bytes) -> spanOf kind Record1 (base + fromIntegral offset) (base + fromIntegral (offset + B.length bytes))) patches
  handoff <- admit (F.symbolAddress <$> F.lookupSymbol rows "xm1_first_stage_handoff")
  entryLabel <- label kind "entry"
  handoffLabel <- label kind "handoff"
  twice <- limit kind 2
  primaryCore <- coreSet kind [Core0]
  code <- codeOf kind load (load + fromIntegral (B.length bin))
  _ <- either (Left . AdapterRefused kind) Right (FirstStageFootprint.tcmRegionWrite (LinkedBytes bin symbolBytes))
  let entry = F.armEntryAddress (F.planEntry plan)
      boundaries =
        -- Stock record-1 entry 0x1800: emulation/Emulation/LoaderPrefixWitness.hs:212-213.
        [ Boundary entryLabel EntryBoundary primaryCore (Pc entry) (Just (Pc 0x1800)) False CompareSequence twice Nothing
        -- Stock handoff 0x183c: replacement/first_stage/entry.S:86.
        , Boundary handoffLabel SiteBoundary primaryCore (Pc handoff) (Just (Pc 0x183c)) False CompareSequence twice Nothing
        ]
      catalog = Catalog boundaries [code] (EnumeratedWrites FirstStageFootprint.writes (Premised FirstStageFootprint.writesPremise)) []
      declaration = PieceDeclaration kind (Set.singleton FirstStageArea) carriers FirstStageFootprint.footprint
        FirstStageFootprint.premises catalog (FirstStageFacts (Pc entry) (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput staged Nothing Nothing)

-- | Emulation.CriticalPair over //replacement/critical_pair:critical-pair-image.
deriveRecord2Link :: Canonical -> LinkedImage -> Either Refusal Piece
deriveRecord2Link source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = Record2Link
      admit = admitted kind
      targets = canonicalTargets source
  original <- admit (C.deriveOriginal (canonicalDat source) (targetOf targets Image4))
  artifact <- admit (C.deriveLinkedArtifact bin symbolBytes)
  staging <- admit (C.stageStartup original artifact)
  rows <- admit (F.parseSymbols symbolBytes)
  declared <- admit (I.declarations rows)
  prefixDeclared <- admit (Stage.loaderPrefixDeclarations rows)
  let at name = admit (F.symbolAddress <$> F.lookupSymbol rows name)
  start <- at "__secondary_stage_start"
  secondaryEntry <- at "xm1_secondary_entry"
  prefixEntry <- at "xm1_loader_prefix_entry"
  initializerEntry <- at "xm1_interface_initializer_entry"
  stockRecord2 <- admit (F.recordSlotBytes F.SecondRecord (U.parsedParts (canonicalParsed source)))
  (publication, initializerPublication) <- case (declaredLoaderPatches prefixDeclared, filter (`notElem` declaredLoaderPatches prefixDeclared) (declaredLoaderPatches declared)) of
    ([header], [published]) -> pure (header, published)
    _ -> Left (AdapterRefused kind "record-2 link lacks its unique loader-B header and initializer publications")
  contract <- admit (I.deriveContract I.Owned (canonicalDat source) (C.startupDat staging) bin symbolBytes)
  let stockInitializer = F.loaderPatchAddress initializerPublication
      end = start + fromIntegral (B.length bin)
      loaderPatches = declaredLoaderPatches declared
      layout = C.witnessLayout artifact
      C.Guest enter = C.witnessEntry layout
      C.Guest leave = C.witnessExit layout
      C.FrameBytes entryFrame = C.witnessEntryFrame layout
      C.FrameBytes exitFrame = C.witnessExitFrame layout
      retained = [(0x10000 + fromIntegral low, 0x10000 + fromIntegral high) | extent <- declaredRetained declared
        , let (low, high) = F.retainedExtentRange extent]
      stockSecondary = U.mediaWordAt stockRecord2 F.resetVectorWordOffset
  resetVector <- spanOf kind Record2 (0x10000 + fromIntegral F.resetVectorWordOffset) (0x10000 + fromIntegral F.resetVectorWordOffset + 4)
  image <- spanOf kind Record2 start end
  loaderWords <- mapM (\patch -> spanOf kind LoaderB (F.loaderPatchAddress patch) (F.loaderPatchAddress patch + 4)) loaderPatches
  -- Owned image4 spans: emulation/Emulation/CriticalPair.hs:709.
  pair <- mapM (\(low, high) -> spanOf kind Image4 low high) [(0x7235d8, 0x7236b0), (0x7236b0, 0x723790)]
  let lowCode = [(start, end), (stockInitializer, stockInitializer + 4), (0x7235d8, 0x7235dc), (0x7236b0, 0x7236b4)]
  code <- mapM (\(low, high) -> codeOf kind low high) (lowCode ++ [(alias low, alias high) | (low, high) <- lowCode])
  few <- limit kind 4
  twice <- limit kind 2
  both <- coreSet kind [Core0, Core1]
  -- Record 2 runs on the second core (replacement/first_stage/src.rs:162-163).
  second <- coreSet kind [Core1]
  let boundary name kind' coreSet' composed retainedPc stop compare' limit' yields = do
        named <- label kind name
        pure (Boundary named kind' coreSet' (Pc composed) (Pc <$> retainedPc) stop compare' limit' yields)
      -- Stock loader-B return of the core-1 initializer call (loader-d0000.dis:662-663).
      stockReturn = 0xd0ab0
  boundaries <- sequence
    [ boundary "secondary-entry" EntryBoundary second secondaryEntry (Just stockSecondary) False CompareSequence twice Nothing
    -- A site: r1-r3 at the prefix handoff differ by construction.
    , boundary "prefix-entry" SiteBoundary second prefixEntry (Just (F.loaderPatchExpected publication)) False CompareSequence twice Nothing
    -- A site: r0-r3 at the initializer are caller leftovers.
    , boundary "initializer-entry" SiteBoundary second initializerEntry (Just stockInitializer) False CompareSequence twice Nothing
    , boundary "initializer-return" ReturnBoundary second stockReturn (Just stockReturn) True CompareSequence twice (Just LoaderOrchestrationBArea)
    , boundary "critical-enter" EntryBoundary both enter (Just 0x7235d8) False ComparePresence few Nothing
    , boundary "critical-exit" EntryBoundary both leave (Just 0x7236b0) False ComparePresence few Nothing
    ]
  returnLabel <- label kind "initializer-return"
  tableLabel <- label kind "table-b"
  let catalog = Catalog boundaries code (EnumeratedWrites record2Destinations (Premised (Premise record2Premise)))
        [CatalogRead tableLabel returnLabel tableB tableBBytes Nothing]
      declaration = PieceDeclaration kind
        (Set.fromList [SecondaryStageArea, LoaderPrefixArea, InterfaceInitializerArea, CriticalPairArea])
        ([resetVector, image] ++ loaderWords ++ pair)
        (Record2Footprint.footprint (targetOf targets LoaderB) retained) (map (Premise . T.pack) C.stagingPremises) catalog
        (Record2LinkFacts (Pc secondaryEntry) [ServiceBinding offset target | (offset, target) <- C.primaryBindings original]
          (LinkedBytes bin symbolBytes) (ScratchBytes (I.contractScratchBytes contract)) (StackDepth (fromIntegral (max entryFrame exitFrame))))
  derivePiece source declaration (StagedOutput (C.startupDat staging) (Just (C.patchedImage4 staging)) Nothing)

-- | Interface table B, which the core-1 initializer fills (emulation/README.md:222).
tableB, tableBBytes :: Word32
tableB = 0x400ddaa0
tableBBytes = 0x162c

-- | Destinations of record-2 code: the core-1 secondary, prefix and its
-- workspace clear before the attach-state boundary, then the interface
-- initializer and the critical-pair bodies.
record2Destinations :: [WriteRange]
record2Destinations =
  [ WriteRange PhysicalTarget 0xa0f824 0xa0f828
  , WriteRange PhysicalTarget 0xa10200 0xa5b200
  , WriteRange (CorePhysicalTarget Core0) 0x62c54 0x63460
  , WriteRange (CorePhysicalTarget Core1) 0xddaa0 0xf3420
  , WriteRange (TcmTarget Core0 InstructionBank) 0 0x2000
  , WriteRange (TcmTarget Core1 InstructionBank) 0 0x2000
  ] ++ map (\(start, end) -> WriteRange (CorePhysicalTarget Core1) start end)
  [ (0x14c60, 0x14c64), (0x14c68, 0x14c6c), (0x14c80, 0x14ca0)
  , (0x20004, 0x20008)
  , (0xa0080, 0xa00c4), (0xa0140, 0xa0148)
  , (0x3fff0024, 0x3fff0040)
  ] ++ [ WriteRange (CorePhysicalTarget Core1) register (register + 4)
       | register <- [ 0xfff8900c, 0xfff89080, 0xfff89084, 0xfff89090, 0xfff89344, 0xfff8934c, 0xfff89350, 0xfff89358
                     , 0xfffb3060, 0xfffb3100, 0xfffb3120, 0xfffb3140, 0xfffb3180, 0xfffb343c, 0xfffb383c ] ]

record2Premise :: T.Text
record2Premise = T.intercalate "; "
  [ "counter [0xa0f824,0xa0f828): the critical pair's only absolute store (emulation/Emulation/CriticalPair.hs:412)"
  , "task reservation pool [0xa10200,0xa5b200) (reports/replacement-inventory-baseline.md:963), a reservation pool and not proven stack space (:887); one task's critical pair may write another task's stack"
  , "core-0 A boot stack [0x62c54,0x63460) (baseline.md:1206); critical-pair frames up to 128 bytes on entry and 112 on exit below the caller SP (emulation/Emulation/CriticalPair.hs:643-644)"
  , "core-1 [0xddaa0,0xf3420): the prefix workspace clear [0xddaa0,0xf2c14) (replacement/loader_prefix/src.rs xm1_loader_prefix_clear_workspace), table B [0xddaa0,0xdf0cc) (emulation/Emulation/InterfaceInitializer.hs:768) and the B boot stack [0xf2c14,0xf3420) (baseline.md:1206)"
  , "initializer scratch [0xf33f0,0xf3410) lies inside the B boot stack (InterfaceInitializer.hs:768)"
  , "ATCM [0,0x2000) on each core: IRQ-context callers and core-1 prefix frames (baseline.md:927,1688)"
  , "every caller of the critical pair runs on one of these stacks [INFERENCE]; stock callees run under the default policy"
  , "before the boundary on core 1: log pointer 0x40014c60 and core-1 index 0x40014c68, publication globals [0x14c80,0x14ca0) (replacement/secondary_stage/src.rs:78-80,115-117,133,245-246; adapter.rs:10-11,18-20)"
  , "secondary peer cell 0x40020004 (src.rs:230-231)"
  , "core-1 log slots [0x400a0080,0x400a00c4): canonical index 0x10 (record-2 word 0x14c68) advancing by 2, slot above 31 returns (src.rs:119-134); its last word 0x400a00c0 is also the companion sample (src.rs:202-205, adapter.rs:28), as in stock; the window stops below the completion word 0x400a00c4"
  , "companion power and status [0x400a0140,0x400a0148) (src.rs:211-216; adapter.rs:33-34)"
  , "loader-B vector slots 0xffff0024..0xffff003f through the core-1 view of secondary SRAM, physical [0x3fff0024,0x3fff0040) (replacement/loader_prefix/src.rs:44-49,69-82; adapter.rs:9-15)"
  , "companion power registers 0xfff8900c, 0xfff89080, 0xfff89084, 0xfff89090, 0xfff89344, 0xfff8934c, 0xfff89350, 0xfff89358 and 0xfffb3060, 0xfffb3100, 0xfffb3120, 0xfffb3140, 0xfffb3180, 0xfffb343c, 0xfffb383c (secondary_stage/src.rs:164-195)"
  ]

-- | Emulation.PrimaryInitializerArtifact over //replacement/primary_initializer:primary-initializer-image.
derivePrimaryPublisher :: Canonical -> LinkedImage -> Either Refusal Piece
derivePrimaryPublisher source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = PrimaryPublisher
      admit = admitted kind
      targets = canonicalTargets source
  primary <- admit (PS.parseSource (canonicalDat source))
  _ <- admit (PC.bindConsumerSource primary (targetOf targets Image4))
  artifact <- admit (PA.deriveArtifact primary bin symbolBytes)
  staged <- admit (PA.stageArtifact primary artifact)
  rows <- admit (F.parseSymbols symbolBytes)
  textEnd <- admit (F.symbolAddress <$> F.lookupSymbol rows "__primary_initializer_text_end")
  let metadata = PA.artifactMetadata artifact
      placement = PA.metadataPlacement metadata
      scratch = PA.metadataScratch metadata
      PS.Guest ownedStart = PA.placementStart placement
      PS.Guest ownedEnd = PA.placementEnd placement
      PS.Guest scratchLow = PA.scratchLow scratch
      PS.Guest scratchHigh = PA.scratchHigh scratch
      PS.Guest root = PS.primaryRoot (PS.sourceRoots primary)
      PS.Guest callerSp = PS.callerSp (PS.sourceCaller primary)
      PA.PublicationWords publicationWords = PA.metadataPublicationWords metadata
      tableBytes = publicationWords * 4
      returns = [site + 4 | PS.Guest site <- PA.metadataCallerSites metadata]
  carrier <- spanOf kind LoaderA ownedStart ownedEnd
  code <- codeOf kind ownedStart textEnd
  entryLabel <- label kind "entry-core0"
  once <- limit kind 1
  -- The alternate caller 0x40aac is unobserved; the entry may recur.
  entryLimit <- limit kind 4
  primaryCore <- coreSet kind [Core0]
  -- loader-startup's a-initializer-return and table read replace these when composed.
  returnRows <- mapM (\pc -> do
    returnLabel <- label kind ("return-" <> hexName pc)
    tableLabel <- label kind ("table-a-" <> hexName pc)
    pure ( Boundary returnLabel ReturnBoundary primaryCore (Pc pc) (Just (Pc pc)) True CompareSequence once (Just LoaderOrchestrationAArea)
         , CatalogRead tableLabel returnLabel root tableBytes Nothing )) returns
  let boundaries = Boundary entryLabel EntryBoundary primaryCore (Pc ownedStart) (Just (Pc ownedStart)) False CompareSequence entryLimit Nothing
        : map fst returnRows
      writes = EnumeratedWrites
        [ WriteRange PhysicalTarget (physical root) (physical root + fromIntegral tableBytes)
        , WriteRange PhysicalTarget (physical scratchLow) (physical scratchHigh) ]
        (Premised (Premise "publisher scratch is the boot stack window below the caller frame (emulation/Emulation/PrimaryInitializerArtifact.hs:499-501)"))
      catalog = Catalog boundaries [code] writes (map snd returnRows)
      declaration = PieceDeclaration kind (Set.singleton PrimaryInitializerArea) [carrier]
        (PublisherFootprint.footprint (targetOf targets LoaderA))
        (map (Premise . T.pack) (map PA.obligationName (PA.metadataObligations metadata) ++ PC.runtimePremiseNames)) catalog
        (PublisherFacts (PublisherFrame (Pc ownedStart) (Guest callerSp))
          [Publication (Guest guest) value | PA.EmittedPublication (PS.Guest guest) value _ _ <- PA.artifactPublications artifact])
  derivePiece source declaration (StagedOutput (PA.stagedBytes staged) Nothing Nothing)

-- | How a dispatcher continuation is observed: a site when the retained role
-- runs the stock dispatcher, a return (r0 and SP compared) when both roles run
-- an admitted one.
data ContinuationKind = SiteContinuations | ReturnContinuations

codeAddress :: LI.CodeAddress -> Word32
codeAddress (LI.CodeAddress value) = value

uniqueOf :: PieceKind -> T.Text -> [Word32] -> Either Refusal Word32
uniqueOf kind what values = case nub values of
  [value] -> Right value
  _ -> Left (AdapterRefused kind ("loader orchestration lacks a unique " <> what))

-- | One orchestration side's boundaries and reads, labelled with a prefix.
-- Retained PCs are stock loader A 0x409ac-0x40af0 (private-data/analysis/x-m1-1.50/decode/loader-40000.dis:598-679)
-- and the same offsets in loader B (loader-d0000.dis:598-679).
orchestrationCatalog :: PieceKind -> T.Text -> ContinuationKind -> L.LinkedArtifact -> Either Refusal ([Boundary], [CatalogRead])
orchestrationCatalog kind prefix continuations artifact = do
  let side = L.linkedSide artifact
      proof = L.linkedProof artifact
      base = LI.loaderBase side
      start = codeAddress (LI.carrierStart side)
      continuation phase = uniqueOf kind ("continuation of the " <> T.pack (show phase) <> " dispatch")
        [codeAddress (LA.boundaryContinuation edge) | edge <- LA.dispatcherBoundaries proof, LA.boundaryPhase edge == phase]
      continuationKind = case continuations of
        SiteContinuations -> SiteBoundary
        ReturnContinuations -> ReturnBoundary
  sites <- first (AdapterRefused kind . T.pack . show) (LS.deriveSites side artifact)
  schedule <- uniqueOf kind "scheduler call" [codeAddress pc | (pc, LA.Retained LI.Schedule) <- LA.callBoundaries proof]
  twice <- limit kind 2
  polls <- limit kind 4
  core <- coreSet kind [case side of LI.LoaderA -> Core0; LI.LoaderB -> Core1]
  let boundary name kind' composed retainedOffset stop compare' limit' = do
        named <- label kind (prefix <> name)
        pure (Boundary named kind' core (Pc composed) (Just (Pc (base + retainedOffset))) stop compare' limit' Nothing)
      read' name stopping guest bytes = do
        named <- label kind (prefix <> name)
        at <- label kind (prefix <> stopping)
        pure (CatalogRead named at guest bytes Nothing)
      initializerCall = codeAddress (LS.initializerCall sites)
      initializerReturn = codeAddress (LS.initializerReturn sites)
  case side of
    LI.LoaderA -> do
      setup <- continuation LI.Setup
      root <- continuation LI.Root
      idle <- continuation LI.Idle0
      diagnostic <- continuation LI.Diagnostic
      boundaries <- sequence
        [ boundary "entry" SiteBoundary start 0x9ac False CompareSequence twice
        , boundary "initializer-call" SiteBoundary initializerCall 0x9dc True CompareSequence twice
        , boundary "initializer-return" ReturnBoundary initializerReturn 0x9e0 True CompareSequence twice
        , boundary "completion-store" SiteBoundary (codeAddress (LS.completionStore sites)) 0xa08 False CompareSequence twice
        , boundary "setup-return" continuationKind setup 0xa00 False CompareSequence twice
        , boundary "root-return" continuationKind root 0xa38 False CompareSequence twice
        , boundary "idle0-return" continuationKind idle 0xa40 False CompareSequence twice
        , boundary "diagnostic-return" continuationKind diagnostic 0xa6c False CompareSequence twice
        , boundary "schedule-call" SiteBoundary schedule 0xa94 True CompareSequence twice
        ]
      rows <- sequence
        -- The saved r11 at 0x40063458 is the first-stage handoff r11, which differs by role.
        [ read' "frame-r4-r5" "initializer-call" 0x40063450 8
        , read' "frame-lr" "initializer-call" 0x4006345c 4
        , read' "table" "initializer-return" tableA interfaceTableBytes
        , read' "ready" "schedule-call" 0x4005927c 4
        , read' "completion" "schedule-call" 0x400a00c4 4
        ]
      pure (boundaries, rows)
    LI.LoaderB -> do
      peerTasks <- continuation LI.PeerTasks
      diagnostic <- continuation LI.Diagnostic
      boundaries <- sequence
        [ boundary "entry" SiteBoundary start 0x9ac True CompareSequence twice
        , boundary "peer-poll" SiteBoundary (codeAddress (LS.peerPoll sites)) 0xa44 False ComparePresence polls
        , boundary "initializer-call" SiteBoundary initializerCall 0xaac True CompareSequence twice
        , boundary "initializer-return" ReturnBoundary initializerReturn 0xab0 True CompareSequence twice
        , boundary "peer-tasks-return" continuationKind peerTasks 0xaf0 False CompareSequence twice
        , boundary "diagnostic-return" continuationKind diagnostic 0xa6c False CompareSequence twice
        , boundary "schedule-call" SiteBoundary schedule 0xa94 True CompareSequence twice
        ]
      rows <- sequence
        [ read' "workspace" "entry" 0x400ddaa0 0x15174
        , read' "vectors" "entry" 0xffff0024 28
        , read' "frame" "initializer-call" 0x400f3410 16
        , read' "completion" "initializer-call" 0x400a00c4 4
        , read' "table-b" "initializer-return" tableB interfaceTableBytes
        , read' "ready" "schedule-call" 0x400e923c 4
        ]
      pure (boundaries, rows)

-- | Interface table A, which the core-0 publisher fills (emulation/Emulation/PrimaryInitializerSource.hs:122-123).
tableA, interfaceTableBytes :: Word32
tableA = 0x4004dae0
interfaceTableBytes = 0x162c

ownedCode :: PieceKind -> Word32 -> Word32 -> Either Refusal [CodeSpan]
ownedCode kind start end = mapM (\(low, high) -> codeOf kind low high) [(start, end), (alias start, alias end)]

-- | Replacement.LoaderOrchestration.Artifact over
-- //replacement/loader_orchestration:loader-b-orchestration-image, staged alone.
deriveLoaderOrchestrationB :: Canonical -> LinkedImage -> Either Refusal Piece
deriveLoaderOrchestrationB source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = LoaderOrchestrationB
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
  orchestration <- shown (L.canonicalSource (canonicalDat source) (targetOf targets Image4))
  artifact <- shown (L.deriveArtifact LI.LoaderB bin symbolBytes)
  staged <- shown (L.stageSide orchestration artifact)
  let start = codeAddress (LI.carrierStart LI.LoaderB)
  carrier <- spanOf kind LoaderB start (start + 340)
  code <- ownedCode kind start (codeAddress (L.linkedEnd artifact))
  (boundaries, rows) <- orchestrationCatalog kind "" SiteContinuations artifact
  let writes = EnumeratedWrites
        [ WriteRange (CorePhysicalTarget Core1) 0xe923c 0xe9240
        , WriteRange (CorePhysicalTarget Core1) 0xf3410 0xf3420 ]
        (Premised (Premise "the ready word is the only store the admission accepts on the peer path (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:322-326); the frame lies below the entry SP 0x400f3420 the record-2 prefix exit installs (join prefix-orchestration-b)"))
      catalog = Catalog boundaries code writes rows
      declaration = PieceDeclaration kind (Set.singleton LoaderOrchestrationBArea) [carrier]
        (OrchestrationFootprint.footprint orchestration artifact)
        (map Premise
          [ "retained loader-B callees behave as stock from their entries (replacement/loader_orchestration/Replacement/LoaderOrchestration/Interface.hs:34-48)"
          , "the canonical image-4 dispatcher keeps its frame within 48 bytes below the service SP (replacement/loader_orchestration/Replacement/LoaderOrchestration/Artifact.hs:51-52)" ])
        catalog (LoaderOrchestrationFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (L.stagedSideBytes staged) Nothing Nothing)

-- | Replacement.StartupComposition.LoaderStartup over the loader-A and loader-B
-- orchestration, //replacement/startup_dispatch:startup-dispatch-image and
-- //replacement/kernel_install:kernel-install-image, joined and staged as one unit.
deriveLoaderStartup :: Canonical -> LinkedImage -> LinkedImage -> LinkedImage -> LinkedImage -> Either Refusal Piece
deriveLoaderStartup source a b dispatcher installer = do
  let kind = LoaderStartup
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      native (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = SC.NativeFiles bin symbolBytes
      linked (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = LinkedBytes bin symbolBytes
  loaderSource <- shown (LS.canonicalSource
    (SC.SourceFiles (canonicalDat source) (targetOf targets Image4) (targetOf targets LoaderA) (targetOf targets LoaderB)))
  joined <- shown (LS.deriveArtifact (native a) (native b) (native dispatcher) (native installer))
  (dat, image4) <- shown (LS.stagedTargets loaderSource joined)
  orchestration <- shown (L.canonicalSource (canonicalDat source) (targetOf targets Image4))
  let sideA = LS.loaderArtifact joined LI.LoaderA
      sideB = LS.loaderArtifact joined LI.LoaderB
      composition = LS.dispatcherInstallerArtifact joined
      dispatcherArtifact = SC.dispatcherArtifact composition
      dispatcherEnd = 0x69cdf8 + fromIntegral (B.length (SD.linkedBytes dispatcherArtifact))
      installerEnd = 0x725a04 + fromIntegral (B.length (KI.linkedBytes (SC.installerArtifact composition)))
      calls = DA.callBoundaries (SD.frameProof dispatcherArtifact)
      instruction = DA.instructionAddressValue . DA.callInstruction
      proof = LS.joinedProof joined
      -- Core 0 runs loader A as the primary, core 1 runs loader B as the peer (emulation/README.md:222).
      naturalRole side = case side of LI.LoaderA -> LI.Primary; LI.LoaderB -> LI.Peer
      ownedLow side = minimum (LS.stackOffsetValue (LS.loaderFrameLow proof)
        : [ LS.stackOffsetValue (LS.boundaryOwnedLow boundary) | boundary <- LS.loaderBoundaries proof side
          , let LI.Scenario role _ _ = LS.boundaryScenario boundary, role == naturalRole side ])
      window top side = WriteRange (CorePhysicalTarget (sideCore side)) (top - fromIntegral (negate (ownedLow side))) top
      sideCore side = case side of LI.LoaderA -> Core0; LI.LoaderB -> Core1
      L.CodeWindow (LI.CodeAddress dispatcherLow) (LI.CodeAddress dispatcherHigh) = StartupFootprint.dispatcherCarrier
      L.CodeWindow (LI.CodeAddress installerLow) (LI.CodeAddress installerHigh) = StartupFootprint.installerCarrier
  modeCall <- uniqueOf kind "mode getter call" [instruction call | call <- calls, DA.callEffect call == DA.ModeRead]
  installReturn <- uniqueOf kind "installer continuation"
    [DA.instructionAddressValue continuation | call <- calls, DA.callEffect call == DA.Install, Just continuation <- [DA.callContinuation call]]
  submits <- mapM (\call -> do
    core <- case DA.callPhase call of
      DA.Root -> Right Core0
      DA.Idle0 -> Right Core0
      DA.Idle1AndStandby -> Right Core1
      phase -> Left (AdapterRefused kind ("task submission in phase " <> T.pack (show phase)))
    pure (SubmitCall core (StackDepth (fromIntegral (negate (DA.stackOffsetValue (DA.callStack call)))))))
    [call | call <- calls, case DA.callEffect call of DA.Submit _ -> True; _ -> False]
  carriers <- sequence
    [ spanOf kind LoaderA 0x409ac 0x40b00, spanOf kind LoaderB 0xd09ac 0xd0b00
    , spanOf kind Image4 dispatcherLow dispatcherHigh, spanOf kind Image4 installerLow installerHigh ]
  code <- concat <$> sequence
    [ ownedCode kind (codeAddress (LI.carrierStart LI.LoaderA)) (codeAddress (L.linkedEnd sideA))
    , ownedCode kind (codeAddress (LI.carrierStart LI.LoaderB)) (codeAddress (L.linkedEnd sideB))
    , ownedCode kind dispatcherLow dispatcherEnd, ownedCode kind installerLow installerEnd ]
  (boundariesA, rowsA) <- orchestrationCatalog kind "a-" ReturnContinuations sideA
  (boundariesB, rowsB) <- orchestrationCatalog kind "b-" ReturnContinuations sideB
  twice <- limit kind 2
  dispatches <- limit kind 8
  primaryCore <- coreSet kind [Core0]
  bothCores <- coreSet kind [Core0, Core1]
  let boundary name kind' onCores composed retainedPc stop limit' = do
        named <- label kind name
        pure (Boundary named kind' onCores (Pc composed) (Just (Pc retainedPc)) stop CompareSequence limit' Nothing)
      read' name stopping guest bytes = do
        named <- label kind name
        at <- label kind stopping
        pure (CatalogRead named at guest bytes Nothing)
  -- Retained dispatcher PCs: stock 0x69ce0c (ModeRead call) and 0x69ce38 (Install continuation),
  -- private-data/analysis/x-m1-1.50/lz/id4.dis.
  boundaries <- sequence
    [ boundary "dispatcher-entry" SiteBoundary bothCores dispatcherLow dispatcherLow False dispatches
    , boundary "mode-call" SiteBoundary primaryCore modeCall 0x69ce0c True twice
    , boundary "installer-entry" SiteBoundary primaryCore installerLow installerLow False twice
    -- A site: the Install call SP is S-48 in the stock dispatcher and lower in the owned one.
    , boundary "installer-return" SiteBoundary primaryCore installReturn 0x69ce38 True twice
    ]
  rows <- sequence
    ( read' "mode" "mode-call" 0x0061c008 4
    : read' "roots" "installer-return" 0x0096bd20 8
    : [ read' ("kernel-data-" <> T.pack (show index)) "installer-return" low (high - low)
      | (index, (low, high)) <- zip [0 :: Int ..] KA.dataWriteRanges ] )
  let writes = EnumeratedWrites
        ( [ WriteRange (CorePhysicalTarget Core0) 0xa00c4 0xa00c8
          , WriteRange (CorePhysicalTarget Core0) 0x5927c 0x59280
          , window 0x63460 LI.LoaderA
          , WriteRange (CorePhysicalTarget Core0) 0x96bd20 0x96bd28 ]
          ++ [ WriteRange (CorePhysicalTarget Core0) (physical low) (physical high) | (low, high) <- KA.dataWriteRanges ]
          ++ [ WriteRange (CorePhysicalTarget Core1) 0xe923c 0xe9240
             , window 0xf3420 LI.LoaderB ] )
        (Premised (Premise (T.intercalate "; "
          [ "completion and ready words are the only stores the orchestration admission accepts (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:322-326)"
          , "each side's window runs from its entry SP (core 0 0x40063460, the stock 0x400a8 literal; core 1 0x400f3420, join prefix-orchestration-b) down to the lowest owned frame of the joined loader, dispatcher and installer proof (replacement/startup_composition/Replacement/StartupComposition/LoaderStartup.hs:132-139), which holds the dispatcher descriptors"
          , "the installer stores the root pair and its data words (replacement/kernel_install/Replacement/KernelInstall/Admission.hs:126-141,172-176)" ])))
      catalog = Catalog (boundariesA ++ boundariesB ++ boundaries) code writes (rowsA ++ rowsB ++ rows)
      declaration = PieceDeclaration kind
        (Set.fromList [LoaderOrchestrationAArea, LoaderOrchestrationBArea, PhaseDispatcherArea, KernelInstallArea])
        carriers
        (StartupFootprint.footprint (targetOf targets LoaderA) (targetOf targets Image4) orchestration sideA sideB)
        (map Premise
          [ "retained loader callees behave as stock from their entries (replacement/loader_orchestration/Replacement/LoaderOrchestration/Interface.hs:34-48)"
          , "retained mode getter 0x657184, clear 0x7221a0 and task submission 0x725dd8 behave as stock (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:226-236)"
          , "retained cache service 0x42634 returns under its pinned contract (replacement/kernel_install/Replacement/KernelInstall/Admission.hs:184-189)"
          , "owned-component stack lows exclude transitive retained-service depth and interrupt frames (replacement/startup_composition/Replacement/StartupComposition/LoaderStartup.hs contractValue open_premises)" ])
        catalog (LoaderStartupFacts (linked a) (linked b) submits)
  derivePiece source declaration (StagedOutput dat (Just image4) Nothing)
