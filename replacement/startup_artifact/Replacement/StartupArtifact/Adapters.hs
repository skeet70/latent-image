{-# LANGUAGE OverloadedStrings #-}
-- | One composition adapter per piece kind: each runs its area's own admission
-- and staging on the canonical inputs and declares carriers and catalog from
-- the admitted artifact.
module Replacement.StartupArtifact.Adapters
  ( LinkedImage (..), ImageBytes (..), SymbolBytes (..)
  , deriveFirstStage, deriveRecord2Link, derivePrimaryPublisher, derivePieceOf
  ) where

import Control.Monad (unless)
import Data.Bifunctor (first)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
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
import Replacement.StartupArtifact.Compose
import qualified Replacement.StartupArtifact.Footprint.FirstStage as FirstStageFootprint
import qualified Replacement.StartupArtifact.Footprint.PrimaryPublisher as PublisherFootprint
import qualified Replacement.StartupArtifact.Footprint.Record2Link as Record2Footprint
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

-- | Target of an A32 BL word at a site.
branchTarget :: Word32 -> Word32 -> Maybe Word32
branchTarget site word
  | word .&. 0x0f000000 == 0x0b000000 =
      let offset = word .&. 0x00ffffff
          signed = if offset .&. 0x00800000 /= 0 then offset .|. 0xff000000 else offset
       in Just (site + 8 + signed * 4)
  | otherwise = Nothing

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
  let entry = F.armEntryAddress (F.planEntry plan)
      boundaries =
        -- Stock record-1 entry 0x1800: emulation/Emulation/LoaderPrefixWitness.hs:212-213.
        [ Boundary entryLabel EntryBoundary primaryCore (Pc entry) (Just (Pc 0x1800)) False CompareSequence twice
        -- Stock handoff 0x183c: replacement/first_stage/entry.S:86.
        , Boundary handoffLabel SiteBoundary primaryCore (Pc handoff) (Just (Pc 0x183c)) False CompareSequence twice
        ]
      catalog = Catalog boundaries [code] (UnenumeratedWrites (Premise "first-stage stores are not enumerated")) []
      declaration = PieceDeclaration kind (Set.singleton FirstStageArea) carriers FirstStageFootprint.footprint
        FirstStageFootprint.premises catalog (FirstStageFacts (Pc entry))
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
  (publication, initializerCall) <- case (declaredLoaderPatches prefixDeclared, filter (`notElem` declaredLoaderPatches prefixDeclared) (declaredLoaderPatches declared)) of
    ([header], [call]) -> pure (header, call)
    _ -> Left (AdapterRefused kind "record-2 link lacks its unique loader-B publication and initializer call")
  let callSite = F.loaderPatchAddress initializerCall
  stockInitializer <- maybe (Left (AdapterRefused kind "initializer call preimage is not a BL")) Right
    (branchTarget callSite (F.loaderPatchExpected initializerCall))
  let end = start + fromIntegral (B.length bin)
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
  let lowCode = [(start, end), (callSite, callSite + 4), (0x7235d8, 0x7235dc), (0x7236b0, 0x7236b4)]
  code <- mapM (\(low, high) -> codeOf kind low high) (lowCode ++ [(alias low, alias high) | (low, high) <- lowCode])
  few <- limit kind 4
  twice <- limit kind 2
  both <- coreSet kind [Core0, Core1]
  -- Record 2 runs on the second core (replacement/first_stage/src.rs:162-163); the
  -- initializer is its core-1 exit (replacement/README.md, loader_prefix entry).
  second <- coreSet kind [Core1]
  let boundary name kind' coreSet' composed retainedPc compare' limit' = do
        named <- label kind name
        pure (Boundary named kind' coreSet' (Pc composed) (Pc <$> retainedPc) False compare' limit')
  boundaries <- sequence
    [ boundary "secondary-entry" EntryBoundary second secondaryEntry (Just stockSecondary) CompareSequence twice
    -- A site: r1-r3 at the prefix handoff differ by construction.
    , boundary "prefix-entry" SiteBoundary second prefixEntry (Just (F.loaderPatchExpected publication)) CompareSequence twice
    , boundary "initializer-call" SiteBoundary second callSite (Just callSite) CompareSequence twice
    -- A site: r0-r3 at the initializer are prefix leftovers.
    , boundary "initializer-entry" SiteBoundary second initializerEntry (Just stockInitializer) CompareSequence twice
    , boundary "initializer-return" ReturnBoundary second (callSite + 4) (Just (callSite + 4)) CompareSequence twice
    , boundary "critical-enter" EntryBoundary both enter (Just 0x7235d8) ComparePresence few
    , boundary "critical-exit" EntryBoundary both leave (Just 0x7236b0) ComparePresence few
    ]
  let catalog = Catalog boundaries code
        (UnenumeratedWrites (Premise "record-2 secondary, prefix and initializer stores are not enumerated; critical-pair frames use the caller stack (emulation/Emulation/CriticalPair.hs:166-167)"))
        []
      declaration = PieceDeclaration kind
        (Set.fromList [SecondaryStageArea, LoaderPrefixArea, InterfaceInitializerArea, CriticalPairArea])
        ([resetVector, image] ++ loaderWords ++ pair)
        (Record2Footprint.footprint (targetOf targets LoaderB) retained) (map (Premise . T.pack) C.stagingPremises) catalog
        (Record2LinkFacts (Pc secondaryEntry) [ServiceBinding offset target | (offset, target) <- C.primaryBindings original]
          (LinkedBytes bin symbolBytes))
  derivePiece source declaration (StagedOutput (C.startupDat staging) (Just (C.patchedImage4 staging)) Nothing)

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
    pure ( Boundary returnLabel ReturnBoundary primaryCore (Pc pc) (Just (Pc pc)) True CompareSequence once
         , CatalogRead tableLabel returnLabel root tableBytes Nothing )) returns
  let boundaries = Boundary entryLabel EntryBoundary primaryCore (Pc ownedStart) (Just (Pc ownedStart)) False CompareSequence entryLimit
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
