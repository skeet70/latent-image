{-# LANGUAGE OverloadedStrings #-}
-- | One composition adapter per piece kind: each runs its area's own admission
-- and staging on the canonical inputs and declares carriers and catalog from
-- the admitted artifact.
module Replacement.StartupArtifact.Adapters
  ( LinkedImage (..), ImageBytes (..), SymbolBytes (..)
  , deriveFirstStage, deriveRecord2Link, derivePrimaryPublisher, deriveLoaderOrchestrationB, derivePieceOf
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
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import Replacement.StartupArtifact.Compose
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupArtifact.Footprint.FirstStage as FirstStageFootprint
import qualified Replacement.StartupArtifact.Footprint.PrimaryPublisher as PublisherFootprint
import qualified Replacement.StartupArtifact.Footprint.Record2Link as Record2Footprint
import qualified Replacement.StartupArtifact.Footprint.LoaderOrchestration as OrchestrationFootprint
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as Stage

newtype ImageBytes = ImageBytes B.ByteString
newtype SymbolBytes = SymbolBytes B.ByteString

-- | A linked flat image and its llvm-nm symbols.
data LinkedImage = LinkedImage ImageBytes SymbolBytes

derivePieceOf :: PieceKind -> Canonical -> LinkedImage -> Either Refusal Piece
derivePieceOf kind = case kind of
  FirstStage -> deriveFirstStage
  Record2Link -> deriveRecord2Link
  PrimaryPublisher -> derivePrimaryPublisher
  LoaderOrchestrationB -> deriveLoaderOrchestrationB

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
          (LinkedBytes bin symbolBytes) (ScratchBytes (I.contractScratchBytes contract)))
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
  returnRows <- mapM (\pc -> do
    returnLabel <- label kind ("return-" <> hexName pc)
    tableLabel <- label kind ("table-a-" <> hexName pc)
    pure ( Boundary returnLabel ReturnBoundary primaryCore (Pc pc) (Just (Pc pc)) True CompareSequence once Nothing
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
        (PublisherFacts [Publication (Guest guest) value | PA.EmittedPublication (PS.Guest guest) value _ _ <- PA.artifactPublications artifact])
  derivePiece source declaration (StagedOutput (PA.stagedBytes staged) Nothing Nothing)

-- | Replacement.LoaderOrchestration.Artifact over
-- //replacement/loader_orchestration:loader-b-orchestration-image, staged alone.
deriveLoaderOrchestrationB :: Canonical -> LinkedImage -> Either Refusal Piece
deriveLoaderOrchestrationB source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = LoaderOrchestrationB
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      address (LI.CodeAddress value) = value
      unique what values = case nub values of
        [value] -> Right value
        _ -> Left (AdapterRefused kind ("loader-B orchestration lacks a unique " <> what))
  orchestration <- shown (L.canonicalSource (canonicalDat source) (targetOf targets Image4))
  artifact <- shown (L.deriveArtifact LI.LoaderB bin symbolBytes)
  staged <- shown (L.stageSide orchestration artifact)
  sites <- shown (LS.deriveSites LI.LoaderB artifact)
  let proof = L.linkedProof artifact
      start = address (LI.carrierStart LI.LoaderB)
      emitted = address (L.linkedEnd artifact)
      continuation phase = unique ("continuation of the " <> T.pack (show phase) <> " dispatch")
        [address (LA.boundaryContinuation edge) | edge <- LA.dispatcherBoundaries proof, LA.boundaryPhase edge == phase]
  peerTasks <- continuation LI.PeerTasks
  diagnostic <- continuation LI.Diagnostic
  schedule <- unique "scheduler call" [address pc | (pc, LA.Retained LI.Schedule) <- LA.callBoundaries proof]
  carrier <- spanOf kind LoaderB start (start + 340)
  code <- mapM (\(low, high) -> codeOf kind low high) [(start, emitted), (alias start, alias emitted)]
  twice <- limit kind 2
  polls <- limit kind 4
  second <- coreSet kind [Core1]
  let boundary name kind' composed retainedPc stop compare' limit' = do
        named <- label kind name
        pure (Boundary named kind' second (Pc composed) (Just (Pc retainedPc)) stop compare' limit' Nothing)
      read' name stopping guest bytes = do
        named <- label kind name
        at <- label kind stopping
        pure (CatalogRead named at guest bytes Nothing)
  -- Retained PCs: stock loader-B 0xd09ac-0xd0af0 (private-data/analysis/x-m1-1.50/decode/loader-d0000.dis:598-679).
  boundaries <- sequence
    [ boundary "entry" SiteBoundary start 0xd09ac True CompareSequence twice
    , boundary "peer-poll" SiteBoundary (address (LS.peerPoll sites)) 0xd0a44 False ComparePresence polls
    , boundary "initializer-call" SiteBoundary (address (LS.initializerCall sites)) 0xd0aac True CompareSequence twice
    , boundary "initializer-return" ReturnBoundary (address (LS.initializerReturn sites)) 0xd0ab0 True CompareSequence twice
    , boundary "peer-tasks-return" SiteBoundary peerTasks 0xd0af0 False CompareSequence twice
    , boundary "diagnostic-return" SiteBoundary diagnostic 0xd0a6c False CompareSequence twice
    , boundary "schedule-call" SiteBoundary schedule 0xd0a94 True CompareSequence twice
    ]
  catalogRows <- sequence
    [ read' "workspace" "entry" 0x400ddaa0 0x15174
    , read' "vectors" "entry" 0xffff0024 28
    , read' "frame" "initializer-call" 0x400f3410 16
    , read' "completion" "initializer-call" 0x400a00c4 4
    , read' "table-b" "initializer-return" tableB tableBBytes
    , read' "ready" "schedule-call" 0x400e923c 4
    ]
  let writes = EnumeratedWrites
        [ WriteRange (CorePhysicalTarget Core1) 0xe923c 0xe9240
        , WriteRange (CorePhysicalTarget Core1) 0xf3410 0xf3420 ]
        (Premised (Premise "the ready word is the only store the admission accepts on the peer path (replacement/loader_orchestration/Replacement/LoaderOrchestration/Admission.hs:322-326); the frame lies below the entry SP 0x400f3420 the record-2 prefix exit installs (join prefix-orchestration-b)"))
      catalog = Catalog boundaries code writes catalogRows
      declaration = PieceDeclaration kind (Set.singleton LoaderOrchestrationBArea) [carrier]
        (OrchestrationFootprint.footprint orchestration artifact)
        (map Premise
          [ "retained loader-B callees behave as stock from their entries (replacement/loader_orchestration/Replacement/LoaderOrchestration/Interface.hs:34-48)"
          , "the canonical image-4 dispatcher keeps its frame within 48 bytes below the service SP (replacement/loader_orchestration/Replacement/LoaderOrchestration/Artifact.hs:51-52)" ])
        catalog (LoaderOrchestrationFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (L.stagedSideBytes staged) Nothing Nothing)
