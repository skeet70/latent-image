{-# LANGUAGE OverloadedStrings #-}
-- | One composition adapter per piece kind: each runs its area's own admission
-- and staging on the canonical inputs and declares carriers and catalog from
-- the admitted artifact.
module Replacement.StartupArtifact.Adapters
  ( LinkedImage (..), ImageBytes (..), SymbolBytes (..), PieceImages (..)
  , deriveFirstStage, deriveRecord2Link, derivePrimaryPublisher, deriveLoaderOrchestrationB, deriveLoaderStartup, derivePieceOf
  , deriveTaskSubmission, deriveTaskStorage, deriveIdleTask, deriveRootTask, deriveImage5Producer, derivePlainProducer, deriveGeneralHeap
  , deriveApplicationPump, deriveTask1Actor, deriveImage5Initializer, deriveTaskConstructors
  , ProducerCall (..), producerCallBoundaries, callerReturn
  , RegionsBytes (..), deriveKernelSlotBodies, slotCallerProblems
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bifunctor (first)
import qualified Data.ByteString as B
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
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
import qualified Replacement.ApplicationPump.Admission as APA
import qualified Replacement.ApplicationPump.Artifact as AP
import qualified Replacement.Task1Actor.Artifact as T1
import qualified Replacement.Image5Init.Admission as IIA
import qualified Replacement.Image5Init.Artifact as II
import qualified Replacement.StartupArtifact.Footprint.Image5Init as InitFootprint
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
import qualified Replacement.StartupArtifact.Footprint.IdleTask as IdleFootprint
import qualified Replacement.StartupArtifact.Footprint.RootTask as RootFootprint
import qualified Replacement.StartupArtifact.Footprint.TaskStorage as TaskStorageFootprint
import qualified Replacement.StartupArtifact.Footprint.TaskSubmission as TaskSubmissionFootprint
import qualified Replacement.StartupArtifact.StackBounds as SB
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as Stage
import qualified Replacement.IdleTask.Admission as IA
import qualified Replacement.IdleTask.Artifact as IT
import qualified Replacement.RootTask.Admission as RA
import qualified Replacement.RootTask.Artifact as RT
import Replacement.RootTask.Services (serviceSymbols)
import qualified Replacement.Image5Load.Admission as I5A
import qualified Replacement.Image5Load.Artifact as I5
import qualified Replacement.PlainLoad.Admission as PLA
import qualified Replacement.PlainLoad.Artifact as PL
import qualified Replacement.StartupArtifact.Footprint.ApplicationPump as PumpFootprint
import qualified Replacement.StartupArtifact.Footprint.Producers as ProducersFootprint
import qualified Replacement.StartupArtifact.Footprint.Task1Actor as Task1Footprint
import qualified Replacement.StartupArtifact.Footprint.GeneralHeap as HeapFootprint
import Replacement.StartupArtifact.Application (PlainImage (..), plainImage)
import qualified Replacement.GeneralHeap.Admission as HA
import qualified Replacement.GeneralHeap.Artifact as HT
import qualified Replacement.TaskStorage.Artifact as TSt
import qualified Replacement.TaskSubmission.Admission as WA
import qualified Replacement.TaskSubmission.Artifact as W
import qualified Replacement.TaskConstructors.Artifact as TC
import qualified Replacement.TaskConstructors.Admission as TCA
import Control.Monad (forM)
import Data.List (find, intercalate)
import Data.Maybe (listToMaybe)
import qualified Emulation.Sha256 as SHA
import qualified Replacement.RtosOwnFirst.Artifact as OF
import qualified Replacement.RtosOwnFirst.Frame as OFrame
import qualified Replacement.RtosOwnFirst.Pins as OP
import qualified Replacement.RtosOwnFirst.Regions as OR
import qualified Replacement.StartupArtifact.KernelSlots as KS

newtype ImageBytes = ImageBytes B.ByteString
newtype SymbolBytes = SymbolBytes B.ByteString

-- | A linked flat image and its llvm-nm symbols.
data LinkedImage = LinkedImage ImageBytes SymbolBytes

-- | A manifest piece's loaded inputs.
data PieceImages
  = SingleImage PieceKind LinkedImage
  -- | Loader-A and loader-B orchestration, dispatcher and installer.
  | LoaderStartupImages LinkedImage LinkedImage LinkedImage LinkedImage
  -- | Loader-A and loader-B kernel slot images and regions.json.
  | KernelSlotImages LinkedImage LinkedImage RegionsBytes

-- | The own-first crate's regions.json.
newtype RegionsBytes = RegionsBytes B.ByteString

derivePieceOf :: Canonical -> PieceImages -> Either Refusal Piece
derivePieceOf source images = case images of
  LoaderStartupImages a b dispatcher installer -> deriveLoaderStartup source a b dispatcher installer
  KernelSlotImages a b regions -> deriveKernelSlotBodies source a b regions
  SingleImage kind image -> case kind of
    FirstStage -> deriveFirstStage source image
    Record2Link -> deriveRecord2Link source image
    PrimaryPublisher -> derivePrimaryPublisher source image
    LoaderOrchestrationB -> deriveLoaderOrchestrationB source image
    LoaderStartup -> Left (AdapterRefused kind "loader-startup takes four linked images")
    TaskSubmission -> deriveTaskSubmission source image
    TaskStorage -> deriveTaskStorage source image
    IdleTask -> deriveIdleTask source image
    RootTask -> deriveRootTask source image
    Image5Producer -> deriveImage5Producer source image
    PlainProducer -> derivePlainProducer source image
    GeneralHeap -> deriveGeneralHeap source image
    ApplicationPump -> deriveApplicationPump source image
    Task1Actor -> deriveTask1Actor source image
    Image5Initializer -> deriveImage5Initializer source image
    TaskConstructors -> deriveTaskConstructors source image
    KernelSlotBodies -> Left (AdapterRefused kind "kernel-slot-bodies takes two linked loaders and regions.json")

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
        [CatalogRead tableLabel returnLabel tableB tableBBytes Nothing Nothing]
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
         , CatalogRead tableLabel returnLabel root tableBytes Nothing Nothing )) returns
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
        pure (CatalogRead named at guest bytes Nothing Nothing)
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
  let starts = [ TaskStart (TaskNumber task) (Pc entry) (StackBytes bytes)
               | call <- calls, DA.Submit (DA.Descriptor (DA.TaskId task) _ _ (DA.StackBytes bytes) (DA.EntryAddress entry) _) <- [DA.callEffect call] ]
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
        pure (CatalogRead named at guest bytes Nothing Nothing)
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
        catalog (LoaderStartupFacts (linked a) (linked b) submits starts)
  derivePiece source declaration (StagedOutput dat (Just image4) Nothing)

-- | Task-stack windows: the A and B boot stacks (reports/replacement-inventory-baseline.md:1206) and the
-- task reservation pool the wrapper's later callers' stacks come from (:963).
taskStackWindows :: [WriteRange]
taskStackWindows =
  [ WriteRange (CorePhysicalTarget Core0) 0x62c54 0x63460
  , WriteRange (CorePhysicalTarget Core1) 0xf2c14 0xf3420
  , WriteRange PhysicalTarget 0xa10200 0xa5b200 ]

-- | Wrapper records 0x40a5b204+8i and storage records 0x40a0fc44+12i for indices 0..119
-- (reports/replacement-inventory-baseline.md:963,1088).
wrapperRecords, storageRecords :: WriteRange
wrapperRecords = WriteRange PhysicalTarget 0xa5b204 0xa5b5c4
storageRecords = WriteRange PhysicalTarget 0xa0fc44 0xa101e4

taskWindowPremise :: T.Text
taskWindowPremise = "frames of the four dispatcher submissions lie in the boot-stack windows (join dispatcher-task-composition, submission-joined-stack-in-boot-window); the stock callers are the 63 call sites join task-composition-owned-calls pins, plus the dispatcher's admitted calls (wrapper-callers-closed); no caller runs in IRQ context [INFERENCE]; later callers run on pool-reserved task stacks [INFERENCE]"

-- | Kebab name of a fixed wrapper service.
serviceName :: WA.FixedService -> T.Text
serviceName service = case service of
  WA.CriticalEnter -> "critical-enter"
  WA.CriticalExit -> "critical-exit"
  WA.Reservation -> "reservation"
  WA.Fill -> "fill"
  WA.Cache -> "cache"
  WA.Log -> "log"
  WA.ErrorCallback -> "error-callback"
  WA.TryLock -> "try-lock"
  WA.Unlock -> "unlock"

-- | Replacement.TaskSubmission.Artifact over //replacement/task_submission:task-submission-image.
deriveTaskSubmission :: Canonical -> LinkedImage -> Either Refusal Piece
deriveTaskSubmission source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = TaskSubmission
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      image4 = targetOf targets Image4
  wrapperSource <- shown (W.canonicalSource (canonicalDat source) image4 (targetOf targets LoaderA) (targetOf targets LoaderB))
  artifact <- shown (W.deriveArtifact bin symbolBytes)
  carrier <- spanOf kind Image4 0x725dd8 0x72612c
  code <- ownedCode kind 0x725dd8 0x72612c
  both <- coreSet kind [Core0, Core1]
  primaryCore <- coreSet kind [Core0]
  secondCore <- coreSet kind [Core1]
  few <- limit kind 4
  once <- limit kind 1
  let wordAt site = U.word32At image4 (fromIntegral (site - targetBase Image4))
      -- Stock submission BLs to fixed services, decoded from canonical image 4 [0x725dd8,0x726098).
      stockSites = [ (site, target) | site <- [0x725dd8, 0x725ddc .. 0x726094]
                   , A.Branch _ True destination <- [A.decode site (wordAt site)], Just target <- [WA.fixed destination] ]
      ownedSites = [ (address, WA.callTarget call) | call <- WA.callBoundaries (W.frameProof artifact)
                   , WA.callEntry call == WA.Submission, let WA.CodeAddress address = WA.callInstruction call ]
      unique target sites = case [site | (site, found) <- sites, found == target] of
        [site] -> Just site
        _ -> Nothing
      paired = [ (service, owned, stock) | service <- [minBound .. maxBound]
               , Just owned <- [unique (WA.FixedService service) ownedSites], Just stock <- [unique (WA.FixedService service) stockSites] ]
      boundary name kind' onCores composed retainedPc stop compare' limit' = do
        named <- label kind name
        pure (Boundary named kind' onCores (Pc composed) (Just (Pc retainedPc)) stop compare' limit' Nothing)
      read' name stopping guest bytes = do
        named <- label kind name
        at <- label kind stopping
        pure (CatalogRead named at guest bytes Nothing Nothing)
  -- Presence only: wrapper hits after the first task entry interleave by preemption timing.
  calls <- mapM (\(service, owned, stock) -> boundary ("submit-" <> serviceName service <> "-call") SiteBoundary both owned stock False ComparePresence few) paired
  boundaries <- sequence
    [ boundary "submit-entry" SiteBoundary both 0x725dd8 0x725dd8 False ComparePresence few
    -- First selected tasks after auto-ready: root104 on core 0 and standby93 on core 1, entered by the
    -- trampoline with r0 = argument 0 and SP = stack top T+24 (reports/replacement-inventory-baseline.md:1124,1130,1146-1153).
    , boundary "a104-first-entry" ReturnBoundary primaryCore 0x69d160 0x69d160 True CompareSequence once
    , boundary "b93-first-entry" ReturnBoundary secondCore 0x69cd58 0x69cd58 True CompareSequence once
    ]
  -- Object bases and strides: reports/replacement-inventory-baseline.md:1108-1116; A objects for j 101..103, B for j 92..102.
  rows <- sequence
    [ read' "wrapper-records" "a104-first-entry" 0x40a5b204 832
    , read' "storage-records" "a104-first-entry" 0x40a0fc44 1248
    , read' "cursor" "a104-first-entry" 0x4089ea98 4
    , read' "error-word" "a104-first-entry" 0x40a0f82c 4
    , read' "a-creation" "a104-first-entry" 0x4005ce74 120
    , read' "a-nodes" "a104-first-entry" 0x40061524 168
    , read' "a-auxiliary" "a104-first-entry" 0x4005d634 36
    , read' "a-auxiliary-nodes" "a104-first-entry" 0x40062b04 48
    , read' "a-id-map" "a104-first-entry" 0x40062468 12
    , read' "a-domain" "a104-first-entry" 0x400545cc 132
    , read' "a-current" "a104-first-entry" 0x40054760 12
    , read' "b-creation" "b93-first-entry" 0x400ecccc 440
    , read' "b-nodes" "b93-first-entry" 0x400f12ec 616
    , read' "b-auxiliary" "b93-first-entry" 0x400ed588 132
    , read' "b-auxiliary-nodes" "b93-first-entry" 0x400f2a34 176
    , read' "b-id-map" "b93-first-entry" 0x400f2404 44
    , read' "b-domain" "b93-first-entry" 0x400e4610 132
    , read' "b-current" "b93-first-entry" 0x400e4720 12
    ]
  let labelled = label kind
  records <- labelled "storage-records"
  cursorRead <- labelled "cursor"
  wrappers <- labelled "wrapper-records"
  creationA <- labelled "a-creation"
  creationB <- labelled "b-creation"
  nodesA <- labelled "a-nodes"
  nodesB <- labelled "b-nodes"
  entryA <- labelled "a104-first-entry"
  entryB <- labelled "b93-first-entry"
  -- Base words: storage record +0, wrapper record +0 and creation record +20 (C's base); top words: node +24 (T's
  -- stack top) and the first-entry SP (reports/replacement-inventory-baseline.md:1088,1120-1124,1130).
  let pool = PoolAllocation (PoolSpan 0xa10200 0xa5b200) records cursorRead
        [ AllocationWords records 0 12 AllocationBase, AllocationWords wrappers 0 8 AllocationBase
        , AllocationWords creationA 20 40 AllocationBase, AllocationWords creationB 20 40 AllocationBase
        , AllocationWords nodesA 24 56 AllocationTop, AllocationWords nodesB 24 56 AllocationTop ]
        [entryA, entryB]
        -- Core 0 creates tasks 102 and 104 in kernel A, core 1 tasks 93 and 103 in kernel B (reports/replacement-inventory-baseline.md:1063-1068,1108-1116).
        [AllocationOwner Core0 creationA 20 40, AllocationOwner Core1 creationB 20 40]
        (Citation "pool [0x40a10200,0x40a5b200) and record layout {base, size, reserved}: replacement/task_storage/src.rs; reservation order across cores is a race, within a core program order (reports/replacement-inventory-baseline.md:1134,1621-1624)")
  let writes = EnumeratedWrites
        (taskStackWindows ++ [wrapperRecords, WriteRange PhysicalTarget 0xa0f82c 0xa0f830, storageRecords])
        (Premised (Premise (T.intercalate "; "
          [ taskWindowPremise
          , "size word W+4 of wrapper record W = 0x40a5b204+8i, error word 0x40a0f82c and release's reservation clear 0x40a0fc44+12i+8 are the admission's guest roots (replacement/task_submission/Replacement/TaskSubmission/Admission.hs:123); i < 120 for the dispatcher's ids 104, 102, 103, 93 (replacement/startup_dispatch/Replacement/StartupDispatch/Admission.hs:206-211) and for every stock call site with an immediate id (wrapper-caller-ids); premised for the sites 0x6b0010, 0x6b07f0, 0xe9231c and 0x11e00ec, whose id the scan does not fix" ])))
      catalog = Catalog (calls ++ boundaries) code writes rows
      declaration = PieceDeclaration kind (Set.singleton TaskSubmissionArea) [carrier]
        (TaskSubmissionFootprint.footprint (targetOf targets LoaderA) (targetOf targets LoaderB) image4)
        (map Premise
          [ "retained callees behave as stock from their entries (replacement/task_submission/Replacement/TaskSubmission/Artifact.hs:73-77)"
          , "the error callback word 0x4089ea8c holds 0x723330 or 0x72835c (error-callback-writers); stores through computed pointers are premised"
          , "the log trace callback is 0x6470fc when set (trace-callback-writers); its initial value 0 and stores through computed pointers are premised"
          , "exit receives enter's token is a source property (replacement/task_submission/src.rs:89-90,202,212,232,258), not an emitted-code proof"
          , "stack bounds exclude exception frames; an IRQ that preempts the interrupted task leaves at most 88 bytes on its stack (irq-entry-frame); retained demands are StockStack depths, " <> T.pack (SB.conditionsPhrase SB.demandConditions) ])
        catalog (TaskSubmissionFacts (LinkedBytes bin symbolBytes) pool)
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (W.stagedBytes (W.stageImage4 wrapperSource artifact))) Nothing)

-- | Replacement.TaskStorage.Artifact over //replacement/task_storage:task-storage-image.
deriveTaskStorage :: Canonical -> LinkedImage -> Either Refusal Piece
deriveTaskStorage source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = TaskStorage
      admit = admitted kind
      targets = canonicalTargets source
  storageSource <- admit (TSt.canonicalSource (canonicalDat source) (targetOf targets Image4) (targetOf targets LoaderA) (targetOf targets LoaderB))
  artifact <- admit (TSt.deriveArtifact bin symbolBytes)
  carrier <- spanOf kind Image4 0x725cc4 0x725da8
  -- The NOP padding after the linked bytes is never executed.
  code <- ownedCode kind 0x725cc4 (0x725cc4 + fromIntegral (B.length (TSt.linkedBytes artifact)))
  both <- coreSet kind [Core0, Core1]
  few <- limit kind 4
  entryLabel <- label kind "transition-entry"
  let boundaries = [Boundary entryLabel SiteBoundary both (Pc 0x725cc4) (Just (Pc 0x725cc4)) False ComparePresence few Nothing]
      writes = EnumeratedWrites
        (taskStackWindows ++ [storageRecords, wrapperRecords, WriteRange PhysicalTarget 0x89ea98 0x89ea9c])
        (Premised (Premise (T.intercalate "; "
          [ taskWindowPremise
          , "record words +0/+4/+8 of 0x40a0fc44+12i and the cursor 0x4089ea98 are the proof's guest roots (replacement/task_storage/Replacement/TaskStorage/FrameProof.hs:120-122,236); index i and the output word W = 0x40a5b204+8i (incoming r2) are the wrapper's: its reservation call is the prelude's only caller (storage-prelude-sole-caller)" ])))
      catalog = Catalog boundaries code writes []
      declaration = PieceDeclaration kind (Set.singleton TaskStorageArea) [carrier]
        (TaskStorageFootprint.footprint (targetOf targets LoaderA) (targetOf targets LoaderB))
        (map Premise
          [ "the retained prelude [0x725c6c,0x725cc4) and epilogue [0x725da8,0x725dd8) behave as stock (replacement/task_storage/Replacement/TaskStorage/Artifact.hs:202)"
          , "the wrapper is the prelude's only caller (storage-prelude-sole-caller) and passes index u32[D]-1 and the wrapper record W; i < 120 is task-submission's premise" ])
        catalog (TaskStorageFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (TSt.stagedBytes (TSt.stageImage4 storageSource artifact))) Nothing)

-- | Allocation tops of the startup tasks under the reservation order J4 observed in both roles
-- (B103, B93, A104, A102 from 0xa10200, 0x400 bytes each; E-STARTUP-J4-COMPOSED-NATURAL-150).
idleTopA, idleTopB, rootTop :: Word64
idleTopA = 0xa11200
idleTopB = 0xa10600
rootTop = 0xa10e00

orderPremise :: T.Text
orderPremise = "the base is the task's allocation top under the reservation order B103, B93, A104, A102 that J4 observed in both roles (E-STARTUP-J4-COMPOSED-NATURAL-150); another order faults the first push"

-- | Replacement.IdleTask.Artifact over //replacement/idle_task:idle-task-image.
deriveIdleTask :: Canonical -> LinkedImage -> Either Refusal Piece
deriveIdleTask source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = IdleTask
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
  refuseUnless kind (IdleFootprint.trampolinesMatch (targetOf targets LoaderA) (targetOf targets LoaderB))
    "loader-B first-entry trampoline differs from loader A's beyond the documented literal pairs"
  idleSource <- shown (IT.canonicalSource (canonicalDat source) (targetOf targets Image4))
  artifact <- shown (IT.deriveArtifact bin symbolBytes)
  let proof = IT.artifactProof artifact
      frame = fromIntegral (negate (IA.scratchLow proof)) :: Word64
  refuseUnless kind (frame > 0) "idle frame is empty"
  carrier <- spanOf kind Image4 0x69cbf0 0x69cc00
  code <- ownedCode kind 0x69cbf0 (0x69cbf0 + fromIntegral (B.length bin))
  both <- coreSet kind [Core0, Core1]
  few <- limit kind 4
  twice <- limit kind 2
  let boundary name kind' composed retainedPc compare' limit' = do
        named <- label kind name
        pure (Boundary named kind' both (Pc composed) (Just (Pc retainedPc)) False compare' limit' Nothing)
  -- Stock WFI 0x69cbf8 and backedge 0x69cbfc (replacement/idle_task/Replacement/IdleTask/Artifact.hs:52).
  boundaries <- sequence
    [ boundary "first-entry" ReturnBoundary 0x69cbf0 0x69cbf0 CompareSequence twice
    , boundary "wait" SiteBoundary (IA.wfiPc proof) 0x69cbf8 ComparePresence few
    , boundary "resume" SiteBoundary (IA.resumePc proof) 0x69cbfc ComparePresence few ]
  firstEntry <- label kind "first-entry"
  let writes = EnumeratedWrites
        [ WriteRange (CorePhysicalTarget Core0) (idleTopA - frame) idleTopA
        , WriteRange (CorePhysicalTarget Core1) (idleTopB - frame) idleTopB ]
        (Premised (Premise (T.intercalate "; "
          [ "the width is proved: the idle body stores only its entry frame below the entry SP (replacement/idle_task/Replacement/IdleTask/Admission.hs:67-78,112-118)"
          , "A102 on core 0 and B103 on core 1 enter at their allocation tops 0xa11200 and 0xa10600; " <> orderPremise ])))
      catalog = Catalog boundaries code writes []
      declaration = PieceDeclaration kind (Set.singleton IdleTaskArea) [carrier] IdleFootprint.footprint
        (map Premise
          [ "entry is A32 SVC on an aligned allocated live stack at least 88 bytes deeper than the admitted frame, the IRQ preemption peak (irq-entry-frame)"
          , "wakeups come from the retained IRQ entry and exit, which save and restore the interrupted context on the interrupted stack (reports/replacement-inventory-baseline.md:1688)" ])
        catalog (IdleTaskFacts (LinkedBytes bin symbolBytes) [firstEntry])
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (IT.stagedBytes (IT.stageImage4 idleSource artifact))) Nothing)

-- | Kebab name of a root call target (replacement/root_task/Replacement/RootTask/Services.hs).
rootServiceName :: Word32 -> Maybe T.Text
rootServiceName target = case [name | (name, address) <- serviceSymbols, address == target] of
  [name] -> T.replace "_" "-" <$> T.stripPrefix "root_" (T.pack name)
  _ -> Nothing

-- | Owned and stock call sites paired by the effect positions they serve; a site serving no common set refuses.
pairRootSites :: [(RA.CallSite, [RA.EffectPosition])] -> [(RA.CallSite, [RA.EffectPosition])]
  -> Maybe [(RA.CallSite, RA.CallSite, [RA.EffectPosition])]
pairRootSites owned stock
  | M.keys ownedSites /= M.keys stockSites = Nothing
  | otherwise = sequence
      [ case (ownedSites M.! positions, stockSites M.! positions) of
          ([o], [s]) | RA.callTarget o == RA.callTarget s -> Just (o, s, positions)
          _ -> Nothing
      | positions <- M.keys ownedSites ]
  where
    bySet sites = M.fromListWith (++) [(positions, [site]) | (site, positions) <- sites]
    ownedSites = bySet owned
    stockSites = bySet stock

-- | Replacement.RootTask.Artifact over //replacement/root_task:root-task-image.
deriveRootTask :: Canonical -> LinkedImage -> Either Refusal Piece
deriveRootTask source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = RootTask
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      image4 = targetOf targets Image4
  rootSource <- shown (RT.canonicalSource (canonicalDat source) image4)
  artifact <- shown (RT.deriveArtifact bin symbolBytes)
  let proof = RT.artifactProof artifact
      frame = fromIntegral (negate (RA.scratchLow proof)) :: Word64
  refuseUnless kind (frame > 0) "root frame is empty"
  paired <- maybe (Left (AdapterRefused kind "owned and stock root call sites do not pair by the effect positions they serve")) Right
    (pairRootSites (RA.callPositions proof) (RA.callPositions (RT.canonicalProof rootSource)))
  carrier <- spanOf kind Image4 0x69d160 0x69d2b4
  -- The NOP padding after the linked bytes is never executed.
  code <- ownedCode kind 0x69d160 (0x69d160 + fromIntegral (B.length bin))
  primary <- coreSet kind [Core0]
  once <- limit kind 1
  let byTarget = M.fromListWith (flip (++)) [(RA.callTarget owned, [(owned, stock, positions)]) | (owned, stock, positions) <- paired]
      numbered = concat
        [ [ (owned, stock, positions, if length group == 1 then Nothing else Just index)
          | (index, (owned, stock, positions)) <- zip [1 :: Int ..] (sortOn (\(_, _, served) -> take 1 served) group) ]
        | group <- M.elems byTarget ]
      -- Executions of a site in its busiest scenario.
      busiest positions = maximum (M.elems (M.fromListWith (+) [(scenario, 1 :: Int) | RA.EffectPosition scenario _ <- positions]))
      reading = ["create-workers-call", "init-6a5634-call", "register-periodic-call", "mark-2-call"]
  calls <- mapM (\(owned, stock, positions, index) -> do
      service <- maybe (Left (AdapterRefused kind ("root call target outside its service list: " <> hexName (RA.callTarget owned)))) Right
        (rootServiceName (RA.callTarget owned))
      let name = service <> maybe "" (\n -> "-" <> T.pack (show n)) index <> "-call"
      named <- label kind name
      count <- limit kind (busiest positions)
      pure (Boundary named SiteBoundary primary (Pc (RA.callPc owned)) (Just (Pc (RA.callPc stock))) (name `elem` reading) CompareSequence count Nothing))
    numbered
  (laterOwned, laterStock) <- case [(owned, stock) | (owned, stock, _) <- paired, RA.callTarget owned == 0x69ca44] of
    [pair] -> Right pair
    _ -> Left (AdapterRefused kind "root lacks a unique 0x69ca44 call")
  laterLabel <- label kind "later-returned"
  callbackLabel <- label kind "periodic-callback"
  -- Stock root calls 0x69ca44 at 0x69d27c (private-data/analysis/x-m1-1.50/lz/id4.dis:132004-132005); callback
  -- 0x69cc18 recurs every 30 ticks, so it compares by presence.
  let presence =
        [ Boundary laterLabel SiteBoundary primary (Pc (RA.callPc laterOwned + 4)) (Just (Pc (RA.callPc laterStock + 4))) False ComparePresence once Nothing
        , Boundary callbackLabel ReturnBoundary primary (Pc 0x69cc18) (Just (Pc 0x69cc18)) True ComparePresence once Nothing ]
      read' name stopping guest bytes = do
        named <- label kind name
        at <- label kind stopping
        pure (CatalogRead named at guest bytes Nothing Nothing)
  -- Resources: 0x7247ac stores the kernel index at 0x93e6f4+4*(id-1) (private-data/analysis/x-m1-1.50/lz/id4.dis:270633-270675)
  -- and creates each through root-0 slot +0x64, whose creation records are 16 bytes from 0x4005dfd8
  -- (replacement/media_permit/RtosManifestMain.hs:66,252,271). Task creation records are 40 bytes from 0x4005beac (A) and
  -- 0x400ebe6c (B) indexed by id-1 (emulation/fujimod-xm1/src/idle_tasks.rs:606-607; emulation/Emulation/RootTaskPlan.hs:56).
  rows <- sequence
    [ read' "error-callback" "create-workers-call" 0x0089ea8c 4
    , read' "resource-kernels" "create-workers-call" 0x0093e6f4 388
    , read' "resource-records" "create-workers-call" 0x4005dfd8 1552
    , read' "message-callback" "init-6a5634-call" 0x0089ea94 4
    , read' "submission-storage-records" "register-periodic-call" 0x40a0fc44 1248
    , read' "submission-wrapper-records" "register-periodic-call" 0x40a5b204 832
    , read' "submission-cursor" "register-periodic-call" 0x4089ea98 4
    , read' "submission-error-word" "register-periodic-call" 0x40a0f82c 4
    , read' "a-creation-all" "register-periodic-call" 0x4005beac 4160
    , read' "b-creation-all" "register-periodic-call" 0x400ebe6c 4160
    , read' "periodic-table" "mark-2-call" 0x409c3500 0x404
    , read' "periodic-context" "mark-2-call" 0x0096ca4c 4
    , read' "callback-sequence" "periodic-callback" 0x00828094 4
    , read' "irq-nesting" "periodic-callback" 0x400545a4 4 ]
  recordsRead <- label kind "submission-storage-records"
  wrapperRead <- label kind "submission-wrapper-records"
  creationA <- label kind "a-creation-all"
  creationB <- label kind "b-creation-all"
  let pool =
        [ AllocationWords recordsRead 0 12 AllocationBase, AllocationWords wrapperRead 0 8 AllocationBase
        , AllocationWords creationA 20 40 AllocationBase, AllocationWords creationB 20 40 AllocationBase ]
      writes = EnumeratedWrites [WriteRange (CorePhysicalTarget Core0) (rootTop - frame) rootTop]
        (Premised (Premise (T.intercalate "; "
          [ "the width is proved: root code stores only to its own stack, at offsets from the admitted frame low to the entry SP (replacement/root_task/Replacement/RootTask/Admission.hs:71,93-104,242-245)"
          , "root enters at its allocation top 0xa10e00; " <> orderPremise ])))
      catalog = Catalog (calls ++ presence) code writes rows
      declaration = PieceDeclaration kind (Set.singleton RootTaskArea) [carrier]
        (RootFootprint.footprint (targetOf targets LoaderA) (targetOf targets LoaderB) image4)
        (map Premise
          [ "retained callees behave as stock from their entries, including error dispatch from 0x725b74 in IRQ context (replacement/root_task/Replacement/RootTask/Artifact.hs:145; reports/replacement-inventory-baseline.md:1686-1688)"
          , "root is entered once on core 0 by the first-entry trampoline with r0 = 0 and SP = the A104 allocation top (reports/replacement-inventory-baseline.md:1130,1621,1628)"
          , "an IRQ that preempts root leaves at most 88 bytes below the interrupted frame (irq-entry-frame); retained callee demand on root's stack is bounded by root-retained-depth, " <> T.pack (SB.conditionsPhrase SB.rootDepthConditions) <> ", except the chains it names" ])
        catalog (RootTaskFacts (LinkedBytes bin symbolBytes) pool)
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (RT.stagedBytes (RT.stageImage4 rootSource artifact))) Nothing)

-- | Entry SP of both producers in task60 on the qualified leaf routes (reports/replacement-inventory-baseline.md:1450,1477).
newtype ProducerEntrySp = ProducerEntrySp Word64

producerEntrySp :: ProducerEntrySp
producerEntrySp = ProducerEntrySp 0xa1c1e0

-- | A retained target the producer must call from exactly one owned and one stock site, the boundary name
-- and kind at that site, and whether it stops.
data ProducerCall = ProducerCall Word32 T.Text BoundaryKind Bool

-- | One sequence boundary per listed call, at the unique owned site and the unique stock BL in the canonical carrier.
producerCallBoundaries :: PieceKind -> B.ByteString -> (Word32, Word32) -> [(Word32, Word32)] -> [ProducerCall] -> Either Refusal [Boundary]
producerCallBoundaries kind image4 (start, end) owned calls = do
  primary <- coreSet kind [Core0]
  once <- limit kind 1
  let wordAt site = U.word32At image4 (fromIntegral (site - targetBase Image4))
      stock = [(site, target) | site <- [start, start + 4 .. end - 4], A.Branch _ True target <- [A.decode site (wordAt site)]]
      sitesOf target sites = [site | (site, found) <- sites, found == target]
  mapM (\(ProducerCall target name kind' stops) -> case (sitesOf target owned, sitesOf target stock) of
      ([ownedSite], [stockSite]) -> do
        named <- label kind (name <> "-call")
        pure (Boundary named kind' primary (Pc ownedSite) (Just (Pc stockSite)) stops CompareSequence once Nothing)
      _ -> Left (AdapterRefused kind ("owned and stock calls of " <> hexName target <> " do not pair uniquely"))) calls

-- | The canonical flashloader site that calls the producer entry, and its continuation.
callerReturn :: PieceKind -> B.ByteString -> Word32 -> Word32 -> Either Refusal Word32
callerReturn kind image4 site entryPc = case A.decode site (U.word32At image4 (fromIntegral (site - targetBase Image4))) of
  A.Branch _ True target | target == entryPc -> Right (site + 4)
  _ -> Left (AdapterRefused kind ("canonical image 4 does not call " <> hexName entryPc <> " at " <> hexName site))

producerWritesPremise :: T.Text -> T.Text
producerWritesPremise proof = T.intercalate "; "
  [ proof
  , "the base is the producer entry SP 0xa1c1e0 in task60 on core 0 that the leaf routes recorded (reports/replacement-inventory-baseline.md:1450,1477); task60's allocation [0xa1b600,0xa1c200) comes from root's submission, compared by root-task/a-creation-all and submission-storage-records; another entry SP faults an entry push" ]

-- | Replacement.Image5Load.Artifact over //replacement/image5_load:image5-load-image.
deriveImage5Producer :: Canonical -> LinkedImage -> Either Refusal Piece
deriveImage5Producer source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = Image5Producer
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      image4 = targetOf targets Image4
      (start, end) = ProducersFootprint.producerCarrier ProducersFootprint.Image5Load
      ProducerEntrySp sp = producerEntrySp
  producerSource <- shown (I5.canonicalSource (canonicalDat source) image4)
  artifact <- shown (I5.deriveArtifact bin symbolBytes)
  let proof = I5.artifactProof artifact
      frame = fromIntegral (negate (I5A.scratchLow proof)) :: Word64
  refuseUnless kind (frame > 0) "image5 producer frame is empty"
  returned <- callerReturn kind image4 (ProducersFootprint.callerSite ProducersFootprint.Image5Load) start
  carrier <- spanOf kind Image4 start end
  code <- ownedCode kind start (start + fromIntegral (B.length bin))
  primary <- coreSet kind [Core0]
  once <- limit kind 1
  -- Stock BLs: private-data/analysis/x-m1-1.50/lz/id4.dis:144569-144658. The six markers and the two storage reads
  -- have several stock sites and get no boundary. Fill request has arity 4, so r0-r3 are all arguments.
  calls <- producerCallBoundaries kind image4 (start, end) (I5A.callSites proof)
    [ ProducerCall (I5A.serviceAddress I5A.SegmentLookup) "segment-lookup" SiteBoundary False
    , ProducerCall (I5A.serviceAddress I5A.FillRequest) "fill-request" EntryBoundary True
    , ProducerCall (I5A.serviceAddress I5A.ModeGetter) "mode" SiteBoundary False
    , ProducerCall (I5A.serviceAddress I5A.DecoderInit) "decoder-init" SiteBoundary False
    , ProducerCall (I5A.serviceAddress I5A.DecoderFinish) "decoder-finish" SiteBoundary False
    , ProducerCall (I5A.serviceAddress I5A.CpuFill) "cpu-fill" SiteBoundary False
    , ProducerCall (I5A.serviceAddress I5A.Cache) "cache" SiteBoundary False ]
  entryLabel <- label kind "entry"
  returnLabel <- label kind "caller-return"
  image5Entry <- label kind "image5-entry"
  fillLabel <- label kind "fill-request-call"
  let read' name at guest bytes = (\named -> CatalogRead named at guest bytes Nothing Nothing) <$> label kind name
      expected name at guest bytes expectation = (\named -> CatalogRead named at guest bytes Nothing (Just expectation)) <$> label kind name
      boundaries =
        [ Boundary entryLabel SiteBoundary primary (Pc start) (Just (Pc start)) False CompareSequence once Nothing ]
        ++ calls ++
        [ Boundary returnLabel ReturnBoundary primary (Pc returned) (Just (Pc returned)) True CompareSequence once Nothing
        -- Image5 entry runs in another task after the done store and callback (reports/replacement-inventory-baseline.md:1352).
        , Boundary image5Entry ReturnBoundary primary (Pc 0xd985a4) (Just (Pc 0xd985a4)) False ComparePresence once (Just Image5InitializerArea) ]
  -- Destination, BSS and staging: reports/replacement-inventory-baseline.md:1321; the flag: Admission.hs:81-87.
  rows <- sequence
    [ read' "fill-flag-cleared" fillLabel I5A.fillFlag 4
    , read' "fill-flag-set" returnLabel I5A.fillFlag 4
    , expected "destination" returnLabel 0x00bf1a00 0x4f1e60 (ExpectTarget Image5)
    , read' "bss" returnLabel 0x01460000 0x2bbe40
    , expected "staging" returnLabel 0x43e40000 0x2e0000 ExpectRecord5Media ]
  let flag = fromIntegral I5A.fillFlag :: Word64
      writes = EnumeratedWrites
        [ WriteRange (CorePhysicalTarget Core0) (sp - frame) sp, WriteRange (CorePhysicalTarget Core0) flag (flag + 4) ]
        (Premised (Premise (producerWritesPremise
          "the width is proved: the producer stores only inside its allocated stack below the entry SP or to the fill flag 0x89f8ac (replacement/image5_load/Replacement/Image5Load/Admission.hs:320-328,401-404)")))
      catalog = Catalog boundaries code writes rows
      declaration = PieceDeclaration kind (Set.singleton Image5ProducerArea) [carrier]
        (ProducersFootprint.footprint ProducersFootprint.Image5Load (targetOf targets LoaderA) (targetOf targets LoaderB) image4)
        (map Premise
          [ "the segment-5 record is tagged and nonaliasing (replacement/image5_load/Replacement/Image5Load/Admission.hs:64-65,345-352)"
          , "0x89f8ac is stored only by the producer and by storage_request_refresh 0x6a8bec, whose address only the producer forms (fill-flag-writers); stores through computed pointers are premised"
          , "the producer runs in task60 on core 0 with an 8-byte aligned entry SP (reports/replacement-inventory-baseline.md:1477; replacement/image5_load/Replacement/Image5Load/Admission.hs:414)"
          , "retained callees, the decoder engine, IRQ98 completion and resource 0x48 behave as stock (replacement/image5_load/Replacement/Image5Load/Artifact.hs:140); their stack demand below the owned frame is bounded by producer-retained-depth, " <> T.pack (SB.conditionsPhrase SB.image5DepthConditions) <> ", except the chains it names" ])
        catalog (Image5ProducerFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (I5.stagedBytes (I5.stageImage4 producerSource artifact))) Nothing)

-- | Replacement.PlainLoad.Artifact over //replacement/plain_load:plain-load-image.
derivePlainProducer :: Canonical -> LinkedImage -> Either Refusal Piece
derivePlainProducer source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = PlainProducer
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      image4 = targetOf targets Image4
      (start, end) = ProducersFootprint.producerCarrier ProducersFootprint.PlainLoad
      ProducerEntrySp sp = producerEntrySp
  producerSource <- shown (PL.canonicalSource (canonicalDat source) image4)
  artifact <- shown (PL.deriveArtifact bin symbolBytes)
  let proof = PL.artifactProof artifact
      frame = fromIntegral (negate (PLA.scratchLow proof)) :: Word64
  refuseUnless kind (frame > 0) "plain producer frame is empty"
  returned <- callerReturn kind image4 (ProducersFootprint.callerSite ProducersFootprint.PlainLoad) start
  carrier <- spanOf kind Image4 start end
  code <- ownedCode kind start (start + fromIntegral (B.length bin))
  primary <- coreSet kind [Core0]
  once <- limit kind 1
  -- Stock BLs: private-data/analysis/x-m1-1.50/lz/id4.dis:144666-144719. Takes, gives and progress have two stock
  -- sites each. The storage read has arity 4 plus a stacked fifth argument.
  calls <- producerCallBoundaries kind image4 (start, end) (PLA.callSites proof)
    [ ProducerCall (PLA.serviceAddress PLA.SegmentLookup) "segment-lookup" SiteBoundary False
    , ProducerCall (PLA.serviceAddress PLA.ExtentGetter) "extent" SiteBoundary False
    , ProducerCall (PLA.serviceAddress PLA.ModeGetter) "mode" SiteBoundary False
    , ProducerCall (PLA.serviceAddress PLA.StorageRead) "storage-read" EntryBoundary False
    , ProducerCall (PLA.serviceAddress PLA.Delay) "delay" SiteBoundary False ]
  entryLabel <- label kind "entry"
  returnLabel <- label kind "caller-return"
  extent0Hook <- label kind "extent0-first-hook"
  extent3Hook <- label kind "extent3-first-hook"
  let read' name at guest bytes = (\named -> CatalogRead named at guest bytes Nothing Nothing) <$> label kind name
      boundaries =
        [ Boundary entryLabel SiteBoundary primary (Pc start) (Just (Pc start)) False CompareSequence once Nothing ]
        ++ calls ++
        [ Boundary returnLabel ReturnBoundary primary (Pc returned) (Just (Pc returned)) False CompareSequence once Nothing
        -- First hooks of extents 0 and 3, run by the worker before any code of their extent
        -- (reports/replacement-inventory-baseline.md:1374-1384).
        , Boundary extent0Hook ReturnBoundary primary (Pc 0x110f9d8) (Just (Pc 0x110f9d8)) True ComparePresence once Nothing
        , Boundary extent3Hook ReturnBoundary primary (Pc 0x11e00f8) (Just (Pc 0x11e00f8)) True ComparePresence once Nothing ]
  rows <- sequence
    [ read' "extent0" extent0Hook 0x01100000 0xac5a0
    , read' "extent3" extent3Hook 0x011e0000 0x26a5e0 ]
  let writes = EnumeratedWrites [WriteRange (CorePhysicalTarget Core0) (sp - frame) sp]
        (Premised (Premise (producerWritesPremise
          "the width is proved: the producer stores only inside its allocated stack below the entry SP (replacement/plain_load/Replacement/PlainLoad/Admission.hs:214-222,299-301)")))
      catalog = Catalog boundaries code writes rows
      declaration = PieceDeclaration kind (Set.singleton PlainProducerArea) [carrier]
        (ProducersFootprint.footprint ProducersFootprint.PlainLoad (targetOf targets LoaderA) (targetOf targets LoaderB) image4)
        (map Premise
          [ "the segment-6 record and the extent descriptor at 0x61c1a4 are tagged and nonaliasing (replacement/plain_load/Replacement/PlainLoad/Admission.hs:51-54; reports/replacement-inventory-baseline.md:1370)"
          , "the producer runs in task60 on core 0 with an 8-byte aligned entry SP (reports/replacement-inventory-baseline.md:1450)"
          , "the raw reader, progress 0x73b924, worker 0x73bc54 and the extent hooks behave as stock (reports/replacement-inventory-baseline.md:1378-1384); their stack demand below the owned frame is bounded by producer-retained-depth, " <> T.pack (SB.conditionsPhrase SB.plainDepthConditions) <> ", except the chains it names" ])
        catalog (PlainProducerFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just (PL.stagedBytes (PL.stageImage4 producerSource artifact))) Nothing)

-- | The canonical images and the plain window, as (base, bytes).
canonicalSegments :: Canonical -> [(Word32, B.ByteString)]
canonicalSegments source =
  [(targetBase target, bytes) | (target, bytes) <- targetList (canonicalTargets source)]
  ++ [(load, bytes) | Right (PlainImage (Guest load) bytes) <- [plainImage (U.parsedParts (canonicalParsed source))]]

-- | The task reservation pool that every heap caller's stack comes from (reports/replacement-inventory-baseline.md:963).
taskPool :: (Word64, Word64)
taskPool = (0xa10200, 0xa5b200)

heapWritesPremise :: T.Text
heapWritesPremise = T.intercalate "; "
  [ "SP-relative stores are proved per entry by the heap frame fixpoint (replacement/general_heap/Replacement/GeneralHeap/Admission.hs deriveFrames); rlsf's stores through pointers are premised to the arena [base, end - 32) its decoded geometry fixes (Admission.hs:29-36)"
  , "the sentinel 0x89d4f8 is the only absolute store (replacement/general_heap/src.rs acquire); errno is stored at the context word, which every public entry loads from 0x89c970 (canonical value 0x89c978)"
  , "every heap caller on the route runs on a task stack in the reservation pool [0xa10200,0xa5b200), as record2-link's pool range; an owned non-SP store into another live frame of the pool is not excluded"
  , "core 1 has no admitted heap write" ]

-- | Replacement.GeneralHeap.Artifact over //replacement/general_heap:general-heap-image.
deriveGeneralHeap :: Canonical -> LinkedImage -> Either Refusal Piece
deriveGeneralHeap source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = GeneralHeap
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      image4 = targetOf targets Image4
  heapSource <- shown (HT.canonicalSource (canonicalDat source) image4 (targetOf targets Image5))
  artifact <- shown (HT.deriveArtifact bin symbolBytes)
  staged <- shown (HT.stageImages heapSource artifact)
  let frames = HT.artifactFrames artifact
      (arenaBase, arenaEnd, guard) = HA.arenaBounds (HT.artifactArena artifact)
      entries = map snd HA.entrySymbols
      staged4 = HT.image4Bytes staged
      carrierSpans = HeapFootprint.carriers
  symbols <- shown (F.parseSymbols symbolBytes)
  -- The metadata words XM1_HEAP_METADATA (56 bytes; replacement/general_heap/src.rs:214-224).
  metadataStart <- shown (F.symbolAddress <$> F.lookupSymbol symbols "XM1_HEAP_METADATA")
  let metadata = (metadataStart, metadataStart + 56)
  refuseUnless kind (HeapFootprint.graphicsTableForwards image4) "graphics-table-forwards: the canonical table or adapters differ"
  let surface = HeapFootprint.entrySurface entries (canonicalSegments source)
  refuseUnless kind (null surface) ("entry-surface: canonical references name heap carrier interiors: " ++ show (take 4 surface))
  refuseUnless kind (HeapFootprint.ownedSurface (HA.instructionPCs frames) entries metadata staged4) "entry-surface: an owned reference names an unadmitted carrier address"
  refuseUnless kind (HeapFootprint.retiredStateUnreferenced (canonicalSegments source)) "retired-state-unreferenced: a canonical reference names retired heap state"
  refuseUnless kind (HeapFootprint.ownedNamesNoRetired staged4) "retired-state-unreferenced: the heap code names retired stock state"
  code <- concat <$> mapM (\(Span _ (Guest low) (Guest high)) -> ownedCode kind low high) carrierSpans
  primary <- coreSet kind [Core0]
  calls <- limit kind 1024
  once <- limit kind 1
  let boundaryAt name boundaryKind pc compare' records stop = do
        named <- label kind name
        pure (Boundary named boundaryKind primary (Pc pc) (Just (Pc pc)) stop compare' records Nothing)
  -- Heap calls come from tasks 72, 73 and 33, whose order across tasks is timing-dependent; entries the
  -- route does not reach are sequence boundaries that must stay empty.
  boundaries <- sequence
    [ boundaryAt "malloc-entry" SiteBoundary 0x89cda4 ComparePresence calls False
    , boundaryAt "free-entry" SiteBoundary 0x89cdc4 ComparePresence calls False
    , boundaryAt "malloc-core-entry" SiteBoundary 0x89cde4 ComparePresence calls False
    , boundaryAt "free-core-entry" SiteBoundary 0x89e560 ComparePresence calls False
    , boundaryAt "realloc-entry" SiteBoundary 0x89da3c CompareSequence once False
    , boundaryAt "realloc-core-entry" SiteBoundary 0x89da64 CompareSequence once False
    , boundaryAt "trim-entry" SiteBoundary 0x89e464 CompareSequence once False
    , boundaryAt "calloc-entry" SiteBoundary 0xbf1c1c CompareSequence once False
    , boundaryAt "calloc-core-entry" SiteBoundary 0xbf1c44 CompareSequence once False
    -- Task 33's entry (reports/replacement-inventory-baseline.md:1762): once, after the first heap call.
    , boundaryAt "uilib-first-entry" ReturnBoundary 0x1109130 ComparePresence once True ]
  uilib <- label kind "uilib-first-entry"
  sentinelRead <- label kind "sentinel"
  tableRead <- label kind "graphics-table"
  storageRead <- label kind "storage-records"
  let heapReads =
        [ CatalogRead sentinelRead uilib HeapFootprint.sentinel 4
            (Just "retained holds stock malloc_av_[0]; composed holds the owned sentinel 0x48454150 after first use") Nothing
        , CatalogRead tableRead uilib 0x890f08 16 Nothing Nothing
        , CatalogRead storageRead uilib 0x40a0fc44 1248
            (Just "consume compares size and third words; bases are normalized per role to the root allocation base, or for task 33 to its first-entry SP minus size, with a zero offset required; all other records must equal the role's root storage records") Nothing ]
      core0 low high = WriteRange (CorePhysicalTarget Core0) low high
      errno = fromIntegral HeapFootprint.errnoWord
      sentinelWord = fromIntegral HeapFootprint.sentinel
      writes = EnumeratedWrites
        [ core0 (fromIntegral arenaBase) (fromIntegral (arenaEnd - guard)), core0 sentinelWord (sentinelWord + 4)
        , core0 errno (errno + 4), uncurry core0 taskPool ]
        (Premised (Premise heapWritesPremise))
      catalog = Catalog boundaries code writes heapReads
      policies =
        [ CarrierWritePolicy carrier WritesNone (if carrier == HeapFootprint.retiredBins then HeapFootprint.image4Carriers else [])
        | carrier <- HeapFootprint.image4Carriers ]
        ++ [CarrierWritePolicy HeapFootprint.image5Carrier WritesModelBusOnly []]
      facts = HeapFacts
        { heapLinked = LinkedBytes bin symbolBytes, heapEntries = entries
        , heapFrames = [ OwnedFrame (HA.frameEntry frame) (HA.frameLow frame) (HA.frameCalls frame) | frame <- frames ]
        , heapArena = (arenaBase, arenaEnd), heapCarrierWrites = policies }
      declaration = PieceDeclaration kind (Set.singleton GeneralHeapArea) carrierSpans
        (HeapFootprint.footprint (targetOf targets LoaderA) (targetOf targets LoaderB) image4)
        (map Premise
          [ "unknown computed entries into the carriers are excluded: entry-surface covers direct branches, literal words and movw/movt pairs only; the two pinned cross-image conditional branch words are excluded (replacement/general_heap/Replacement/GeneralHeap/Artifact.hs:165)"
          , "no metadata reader consumes beyond payload-4; the stock boundary tag decodes to an extent within the allocation (Artifact.hs:166)"
          , "retired bin state has no reader or writer outside the heap: retired-state-unreferenced covers direct references, and the composed-role carrier watch admits no write after attach"
          , "the sentinel is 0 in the decoded image 4 and the phase-0 clear, which runs only in mode 1 and before any task, cannot reach it (general-heap-arena-clear)"
          , "the arena and pool have no external writer or backing consumer: the backing helper 0x72b110 is called only from retired code; writers through computed pointers are premised"
          , "the retained recursive lock 0x72b164/0x72b1cc serializes every mutation and first-use setup; heap calls on the route are core 0 only, and cross-core contention is premised"
          , "retained lock, unlock and memmove behave as stock; their stack demand is a StockStack depth under the switch model, " <> T.pack (SB.conditionsPhrase SB.heapConditions) <> " (StackBounds.switchModel)"
          , "the image-5 calloc entry and thunk are published by the image-5 producer's decode of record 5 before any code that names them runs (general-heap-image5-publication)"
          , "cold boot only; no warm-checkpoint allocator ownership migration is admitted"
          , "finite host checks do not qualify silicon contention, deadlines or capture, development and storage workloads" ])
        catalog (GeneralHeapFacts facts)
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just staged4) (Just (HT.image5Bytes staged)))
-- | Task1's SP at its pump call 0xc23274 on core 0, recorded in both roles of J3 v12 and in J6 v1 retained.
task1CallSp :: Word32
task1CallSp = 0xa4cdd0

-- | Replacement.ApplicationPump.Artifact over //replacement/application_pump:application-pump-image.
deriveApplicationPump :: Canonical -> LinkedImage -> Either Refusal Piece
deriveApplicationPump source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = ApplicationPump
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      (start, end) = PumpFootprint.pumpCarrier
      address (APA.CodeAddress value) = value
  artifact <- shown (AP.deriveArtifact bin symbolBytes)
  pumpSource <- shown (AP.canonicalSource (canonicalDat source) (targetOf targets Image5))
  staged <- shown (AP.stageImage5 pumpSource artifact)
  let proof = AP.artifactProof artifact
      frame = fromIntegral (negate (APA.scratchLow proof)) :: Word64
      sp = fromIntegral task1CallSp :: Word64
  refuseUnless kind ([(address (APA.callInstruction site), address (APA.callTarget site)) | site <- APA.proofCallSites proof]
      == zip [0xbfbd28, 0xbfbd2c, 0xbfbd34] PumpFootprint.hookEntries)
    "the pump's hook calls are not at the stock sites 0xbfbd28, 0xbfbd2c and 0xbfbd34"
  refuseUnless kind (address (APA.returnInstruction proof) == 0xbfbd3c) "the pump's return is not at the stock 0xbfbd3c"
  refuseUnless kind (frame > 0) "the pump frame is empty"
  carrier <- spanOf kind Image5 start end
  code <- ownedCode kind start end
  let writes = EnumeratedWrites [WriteRange PhysicalTarget (sp - frame) sp]
        (Premised (Premise (T.intercalate "; "
          [ "the width is proved: the pump stores only its frame below the entry SP (replacement/application_pump/Replacement/ApplicationPump/Admission.hs:179-198)"
          , "the base is task1's SP 0xa4cdd0 at the call 0xc23274 on core 0 (J3 v12 both roles, J6 v1 retained); task1's allocation comes from root's submission, compared by the pool relation; another SP faults the entry push" ])))
      declaration = PieceDeclaration kind (Set.singleton ApplicationPumpArea) [carrier] PumpFootprint.footprint
        (map Premise
          [ "the retained hooks 0xbfb1b8, 0xbfb20c and 0xbfb1e0 return in A32 preserving r4-r11 and SP (replacement/application_pump/Replacement/ApplicationPump/Artifact.hs:126)"
          , "task1 calls the pump only from 0xc23274 with LR 0xc23278 (private-data/analysis/x-m1-1.50/lz/id5.dis:50679-50680)"
          , "retained callee stack demand below the owned frame is not proved" ])
        (Catalog [] code writes []) (ApplicationPumpFacts (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (canonicalDat source) Nothing (Just (AP.stagedBytes staged)))

deriveTask1Actor :: Canonical -> LinkedImage -> Either Refusal Piece
deriveTask1Actor source (LinkedImage (ImageBytes bin) (SymbolBytes symbols)) = do
  let kind = Task1Actor
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
  artifact <- shown (T1.deriveArtifact bin symbols)
  original <- shown (T1.canonicalSource (canonicalDat source) (targetOf (canonicalTargets source) Image5))
  image <- shown (T1.stageImage5 original artifact)
  ranges <- mapM (\(T1.Carrier (T1.CodeAddress low) (T1.CodeAddress high)) -> spanOf kind Image5 low high) T1.carriers
  code <- concat <$> mapM (\(T1.Carrier (T1.CodeAddress low) _,bytes) -> ownedCode kind low (low + fromIntegral (B.length bytes))) (T1.emittedCarriers artifact)
  let writes = EnumeratedWrites
        ([WriteRange PhysicalTarget low high | (low,high) <- [(0x944580,0x94458a),(0x94f5e0,0x94f5e4),(0x950148,0x95014c),(0x95200c,0x952010),(0x954d68,0x954d6c),(0x972e84,0x972e88)]]
          ++ [WriteRange PhysicalTarget 0xa4cdc0 0xa4ce00])
        (Premised (Premise "task1 entry SP 0xa4ce00, 32-byte actor and once-duty frames; retained service effects are not proved by staging"))
      declaration = PieceDeclaration kind (Set.singleton Task1ActorArea) ranges
        (Task1Footprint.footprint artifact)
        [Premise "host composition authenticates carriers and direct service boundaries; application observer qualification is separate"]
        (Catalog [] code writes []) (Task1ActorFacts (LinkedBytes bin symbols))
  derivePiece source declaration (StagedOutput (canonicalDat source) Nothing (Just image))
deriveImage5Initializer :: Canonical -> LinkedImage -> Either Refusal Piece
deriveImage5Initializer source (LinkedImage (ImageBytes bin) (SymbolBytes symbols)) = do
  let kind = Image5Initializer
      shown :: Show e => Either e a -> Either Refusal a
      shown = first (AdapterRefused kind . T.pack . show)
  artifact <- shown (II.deriveArtifact bin symbols)
  staged <- shown (II.stageImage5 (targetOf (canonicalTargets source) Image5) artifact)
  carrier <- spanOf kind Image5 0xd9856c 0xd986b0
  code <- ownedCode kind 0xd9856c 0xd986b0
  primary <- coreSet kind [Core0]
  once <- limit kind 1
  let stock = [0xd985b4,0xd985b8,0xd985c0,0xd985c4,0xd985d4,0xd985e0,0xd985e4,0xd985e8,0xd985ec,0xd98600,0xd98614,0xd98618,0xd98628,0xd98638,0xd98648,0xd9865c,0xd98660,0xd98670,0xd98680,0xd98690,0xd986a8]
      boundary name before after stop = do
        named <- label kind name
        pure (Boundary named SiteBoundary primary (Pc after) (Just (Pc before)) stop ComparePresence once Nothing)
  calls <- sequence [boundary ("call-" <> (if n < (10 :: Int) then "0" else "") <> T.pack (show n)) before (IIA.callInstruction owned) False | (n,before,owned) <- zip3 [1..] stock (IIA.proofCallSites (II.artifactProof artifact))]
  entry <- boundary "entry" 0xd985a4 0xd985a4 False
  returned <- boundary "return" 0x69d2e4 0x69d2e4 True
  let writes = EnumeratedWrites [WriteRange PhysicalTarget 0xa25588 0xa255a0]
        (Premised (Premise "entry SP 0xa255a0 in both roles of J8 v1; private-data/scratch/j9/design.md section 3"))
      declaration = PieceDeclaration kind (Set.singleton Image5InitializerArea) [carrier] InitFootprint.footprint
        [ Premise "retained services preserve SP and r4-r11 and return in A32; private-data/scratch/j9/spec.md"
        , Premise "0x725dd8 observes unwritten descriptor D+24 only by equality with 0xa5f0a5f0; the admitted frame preserves that classification, not the residue bytes; private-data/scratch/j9/design.md section 2 and spec.md sections 2.1 and 3.3"
        , Premise "retained callee stack demand below the owned frame is not proved" ]
        (Catalog (entry:calls ++ [returned {boundaryKind = ReturnBoundary}]) code writes []) (Image5InitializerFacts (LinkedBytes bin symbols))
  derivePiece source declaration (StagedOutput (canonicalDat source) Nothing (Just staged))
deriveTaskConstructors :: Canonical -> LinkedImage -> Either Refusal Piece
deriveTaskConstructors source (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = do
  let kind = TaskConstructors
      admit = admitted kind
      targets = canonicalTargets source
  original <- admit (TC.canonicalSource (canonicalDat source) (targetOf targets Image4) (targetOf targets Image5))
  artifact <- admit (TC.deriveArtifact original bin symbolBytes)
  carriers <- mapM (\(TCA.Constructor (TCA.CodeAddress low) (TCA.CodeAddress high)) ->
    spanOf kind (if low < targetBase Image5 then Image4 else Image5) low high) TC.constructors
  code <- concat <$> mapM (\(TCA.Constructor (TCA.CodeAddress low) (TCA.CodeAddress high)) -> ownedCode kind low high) TC.constructors
  both <- coreSet kind [Core0, Core1]
  few <- limit kind 4
  boundaries <- mapM (\(TCA.Constructor (TCA.CodeAddress low) _) -> do
    named <- label kind ("entry-" <> hexName low)
    pure (Boundary named SiteBoundary both (Pc low) (Just (Pc low)) False ComparePresence few Nothing)) TC.constructors
  let serviceWords = nub [address | constructor <- TC.constructors, TCA.CodeAddress address <- TC.retainedCalls original constructor]
      footprint = [Footprint (Span (if address < targetBase Image5 then Image4 else Image5) (Guest address) (Guest (address+4))) RuntimePremise
        (Citation "replacement/task_constructors/Replacement/TaskConstructors/Artifact.hs:33-39")
        | address <- serviceWords, address >= targetBase Image4]
        ++ [Footprint (Span Image5 (Guest 0xec56a0) (Guest 0xec56a4)) RuntimePremise
          (Citation "replacement/task_constructors/Replacement/TaskConstructors/Artifact.hs:22-30")]
      premises = map Premise
        [ "retained mailbox creation and task submission preserve their stock entry ABI and effects"
        , "layout/control-flow admission does not prove descriptor effects, status propagation or callee-saved registers"
        , "constructor callers provide mapped task-stack storage; descriptor D+24 retains its caller-stack classification word"
        , "callback provenance from incoming r0 into r8 and its result argument at 0xef8da8 require native qualification"
        , "retained callees preserve r4-r11 and do not modify the constructor descriptor tail" ]
      writes = EnumeratedWrites (taskStackWindows ++ [wrapperRecords, storageRecords
        , WriteRange PhysicalTarget 0x89ea98 0x89ea9c, WriteRange PhysicalTarget 0xa0f82c 0xa0f830
        , WriteRange PhysicalTarget 0x1526a7c 0x1526a84, WriteRange PhysicalTarget 0x169d370 0x169d376
        , WriteRange PhysicalTarget 0x1469cd0 0x146a184])
        (Premised (Premise "constructor and retained-service effects require native and composed qualification"))
      catalog = Catalog boundaries code writes []
      declaration = PieceDeclaration kind (Set.singleton TaskConstructorsArea) carriers footprint premises catalog
        (TaskConstructorsFacts (LinkedBytes bin symbolBytes))
      (image4,image5) = TC.stageImages original artifact
  derivePiece source declaration (StagedOutput (canonicalDat source) (Just image4) (Just image5))
-- | Replacement.RtosOwnFirst.Artifact over //replacement/rtos_own_first:own-first-a and own-first-b and
-- //replacement/rtos_own_first:regions.json, staged through the pre-table loader path.
deriveKernelSlotBodies :: Canonical -> LinkedImage -> LinkedImage -> RegionsBytes -> Either Refusal Piece
deriveKernelSlotBodies source a b (RegionsBytes regions) = do
  let kind = KernelSlotBodies
      shown :: Show e => Either e x -> Either Refusal x
      shown = first (AdapterRefused kind . T.pack . show)
      targets = canonicalTargets source
      linked (LinkedImage (ImageBytes bin) (SymbolBytes symbolBytes)) = OF.LinkedDomain bin symbolBytes
      linkedOf domain = case domain of
        OR.DomainA -> linked a
        OR.DomainB -> linked b
      loaderOf domain = case domain of
        OR.DomainA -> LoaderA
        OR.DomainB -> LoaderB
      digest = ExtentDigest . T.pack . SHA.sha256Hex . SHA.sha256
      bounds region = let OR.Extent (OR.GuestAddress low) (OR.GuestAddress high) = OR.regionExtent region in (low, high)
  ownSource <- shown (OF.canonicalSource (canonicalDat source) (targetOf targets LoaderA) (targetOf targets LoaderB))
  shown (OF.requirePinned ownSource)
  slotsAdmitted <- shown (OF.admit OP.stockPins ownSource regions (linked a) (linked b))
  staged <- shown (OF.stage ownSource slotsAdmitted)
  let fill = OR.tableFill (OF.admittedTable slotsAdmitted)
  domains <- forM OR.domains $ \domain -> do
    let table = OR.domainTable (OF.admittedTable slotsAdmitted) domain
        target = loaderOf domain
        OR.GuestAddress base = OR.domainLoaderBase domain
        rows = OR.domainExtents table
        code = KS.CodeImage base (OF.admittedLoader slotsAdmitted domain)
        stockCode = KS.CodeImage base (targetOf targets target)
        entries = [entry | row <- rows, let OR.GuestAddress entry = OR.regionEntry row]
        canonicalLoader = (base, base + fromIntegral (B.length (targetOf targets target)))
        stockBytes (low, high) = B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base)) (targetOf targets target))
        frameAt address = case OF.admittedFrame slotsAdmitted domain address of
          Just frame -> let OR.FrameBytes bytes = OFrame.frameComposed frame in Right (StackDepth (fromIntegral bytes))
          Nothing -> Left (AdapterRefused kind "admitted slot frame missing")
    owned <- shown (OF.ownedExtents fill domain table (linkedOf domain))
    slots <- forM (OF.entryTable slotsAdmitted domain) $ \(OF.EntryRow slot stock (OR.SymbolName name) ownedEntry) -> do
      refuseUnless kind (ownedEntry == stock) "owned symbol is not linked at its stock entry"
      (region, bytes) <- maybe (Left (AdapterRefused kind "entry row without an installed extent")) Right
        (find ((== slot) . OR.regionSlot . fst) owned)
      let OR.SlotOffset offset = slot
          OR.GuestAddress entry = stock
          (low, high) = bounds region
      extent <- spanOf kind target low high
      frame <- frameAt stock
      ownedExits <- first (AdapterRefused kind . ("owned " <>)) (KS.walkAbi code (map bounds rows) (`elem` entries) entry)
      stockExits <- first (AdapterRefused kind . ("stock " <>)) (KS.walkAbi stockCode [canonicalLoader] (const True) entry)
      pure (KernelSlot (RootSlot offset) (Pc entry) (OwnedSymbol (T.pack name)) extent frame (KS.changedState ownedExits)
        (KS.differingState stockExits ownedExits) (digest (stockBytes (low, high))) (digest bytes))
    let body = OR.tableBody table
        OR.Extent bodyStart@(OR.GuestAddress bodyLow) (OR.GuestAddress bodyHigh) = OR.bodyExtent body
        OR.SlotOffset host = OR.bodyHost body
    bodySpan <- spanOf kind target bodyLow bodyHigh
    bodyFrame <- frameAt bodyStart
    carriers <- mapM (\region -> uncurry (spanOf kind target) (bounds region)) rows
    pure (carriers, slots, KernelSlotBody (RootSlot host) bodySpan bodyFrame)
  published <- first (AdapterRefused kind) (KS.rootTargets <$> KS.publishedRows (targetOf targets LoaderA) (targetOf targets LoaderB))
  let carriers = concat [spans | (spans, _, _) <- domains]
      slots = concat [rows | (_, rows, _) <- domains]
      bodies = [body | (_, _, body) <- domains]
  plain <- plainImage (U.parsedParts (canonicalParsed source))
  case slotCallerProblems targets plain slots published of
    [] -> pure ()
    problems -> Left (AdapterRefused kind (T.pack (intercalate "; " problems)))
  code <- concat <$> mapM (\(Span _ (Guest low) (Guest high)) -> ownedCode kind low high) (normalizeSpans carriers)
  few <- limit kind 4
  let entryOf target offset = case [pc | KernelSlot {kernelSlotOffset = RootSlot slot, kernelSlotEntry = Pc pc, kernelSlotExtent = Span owner _ _} <- slots, slot == offset, owner == target] of
        [pc] -> Right pc
        _ -> Left (AdapterRefused kind "kernel slot table lacks a unique entry")
      boundary name target offset onCores = do
        pc <- entryOf target offset
        named <- label kind name
        reached <- coreSet kind onCores
        pure (Boundary named EntryBoundary reached (Pc pc) (Just (Pc pc)) False ComparePresence few Nothing)
  boundaries <- sequence
    [ boundary "a-1618-entry" LoaderA 0x1618 [Core0, Core1]
    , boundary "a-164-entry" LoaderA 0x164 [Core0, Core1]
    , boundary "a-168-entry" LoaderA 0x168 [Core0, Core1]
    , boundary "b-164-entry" LoaderB 0x164 [Core1]
    , boundary "b-168-entry" LoaderB 0x168 [Core1] ]
  let writes = EnumeratedWrites
        [ WriteRange PhysicalTarget 0 0x20000000
        , WriteRange (TcmTarget Core0 InstructionBank) 0 0x2000
        , WriteRange (TcmTarget Core1 InstructionBank) 0 0x2000 ]
        (Premised (Premise (T.intercalate "; "
          [ "the bodies store on the caller's stack within their admitted frames (8 bytes, 24 for +0x16c; replacement/rtos_own_first/stage/Replacement/RtosOwnFirst/Artifact.hs:351-356), through the caller's r0 (+0x100, +0x128, +0x12c) and to their loader's time-offset word (+0xfc) (replacement/rtos_own_first/src/lib.rs:168-180,186-200)"
          , "callers run on task, boot and IRQ stacks, including each core's ATCM, and pass any r0, so the range is all of physical RAM and both ATCMs [INFERENCE]" ])))
      catalog = Catalog boundaries code writes []
      declaration = PieceDeclaration kind (Set.singleton KernelSlotBodiesArea) carriers KS.footprint
        (map Premise
          [ "callers enter each slot in A32 in a privileged mode, SVC or SYS, the modes the native-case oracle qualified (E-RTOS-OWN-FIRST-ORACLE-150)"
          , "loader-A service boundaries admit both cores: image4 wrappers 0x72afd8 and 0x72b034 call root[0]+0x1618 at 0x72aff4 and 0x72b050 before selecting root[core], and their error paths call A +0x16c at 0x72b00c and 0x72b068 and log 0x41a34 at 0x72b01c and 0x72b078; that log calls A +0x164 at 0x41a5c and A +0x168 at 0x41b84 (canonical X-M1 1.50 image4 and loader A)"
          , "each body preserves r4-r11 and SP, returns to its entry LR and changes only the caller-saved state its facts list (replacement/startup_artifact/Replacement/StartupArtifact/KernelSlots.hs walkAbi)"
          , "every canonical direct and root-slot caller in loader A, loader B, image4, image5 and the plain image leaves dead the caller-saved state the owned body may leave different from stock (KernelSlots.hs deadAfter); the rule takes it that callers rely on no callee's preservation of r1-r3, r12 or the flags except a slot body's, and return results of at most 32 bits, that the root array index is a core number, and that each other root slot holds a word the canonical publisher or initializer stores"
          , "the slot semantics equal stock per slot on the oracle grid (E-RTOS-OWN-FIRST-ORACLE-150) and on the natural route (E-RTOS-OWN-FIRST-NATURAL-150)"
          , "publishers and root words are unchanged, so every published slot word names the stock entry where its owned body is linked (replacement/rtos_own_first/stage/Replacement/RtosOwnFirst/Artifact.hs:321-322)" ])
        catalog (KernelSlotFacts slots bodies)
  derivePiece source declaration (StagedOutput (OF.stagedDat staged) Nothing Nothing)

-- | Every direct branch into a slot extent off its entry, and every direct or root-slot call into an owned slot
-- whose caller reads caller-saved state the owned body may leave different from stock, in loader A, loader B,
-- image4, image5 and the plain image.
slotCallerProblems :: TargetBytes -> PlainImage -> [KernelSlot] -> KS.RootTargets -> [String]
slotCallerProblems targets (PlainImage (Guest plainBase) plainBytes) slots published =
  [ "the canonical branch at 0x" ++ showHex site " enters a slot extent off its entry"
  | KS.DirectCall site target _ <- direct, target `notElem` entries ]
  ++ [ "the canonical caller at 0x" ++ showHex site (" reads caller-saved state the owned body may leave different from stock (" ++ T.unpack reason ++ ")")
     | (site, reason) <- liveDirect ++ liveSlot ]
  where
    entries = [pc | KernelSlot {kernelSlotEntry = Pc pc} <- slots]
    images = [KS.CodeImage (targetBase target) (targetOf targets target) | target <- [LoaderA, LoaderB, Image4, Image5]]
      ++ [KS.CodeImage plainBase plainBytes]
    word address = listToMaybe [found | image <- images, Just found <- [KS.codeWord image address]]
    differsAt entry = concat [kernelSlotDiffers slot | slot <- slots, kernelSlotEntry slot == Pc entry]
    differsOf offset = nub (concat [kernelSlotDiffers slot | slot <- slots, kernelSlotOffset slot == offset])
    offsets = map kernelSlotOffset slots
    direct = KS.directImageCalls images (map kernelSlotExtent slots)
    liveDirect = [ (site, reason) | KS.DirectCall site target True <- direct, target `elem` entries
                 , Just reason <- [KS.liveAfter word entries offsets published KS.EnteredDirectly (differsAt target) (site + 4)] ]
    liveSlot = [ (site, reason) | KS.SlotCall site offset <- KS.slotImageCalls images, offset `elem` offsets
               , Just reason <- [KS.liveAfter word entries offsets published KS.EnteredThroughRoot (differsOf offset) (site + 4)] ]

