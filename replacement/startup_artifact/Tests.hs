{-# LANGUAGE OverloadedStrings #-}
-- | Synthetic stager tests: refusals and postconditions of the composed
-- startup stager over a synthetic container.
module Main where

import Control.Monad (unless)
import qualified Data.Aeson as J
import Data.Aeson ((.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Vector as V
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Fixtures
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.AttachState
import Replacement.StartupArtifact.Contract
import Replacement.StartupArtifact.Manifest
import Replacement.StartupArtifact.Application (RoleImages (..), application, applicationValue, plainImage)
import Replacement.StartupArtifact.Joins (JoinKind, registeredJoins)
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.StartupArtifact.Footprint.FirstStage as FirstStageFootprint
import System.Environment (getArgs)

data TestJoin = TestJoin deriving (Eq, Show)

instance JoinName TestJoin where
  joinName TestJoin = "test-join"

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (fail ("failed: " ++ name))
  modifyIORef' counter (+ 1)

refusal :: Either Refusal a -> Maybe Refusal
refusal = either Just (const Nothing)

refusal' :: Either Text a -> Bool
refusal' = either (const True) (const False)

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

slice :: Int -> Int -> B.ByteString -> B.ByteString
slice offset count = B.take count . B.drop offset

-- | An edit whose every byte differs from the canonical target.
flip' :: TargetBytes -> Span -> Edit
flip' targets range@(Span target (Guest start) _) =
  let offset = fromIntegral (start - targetBase target)
      bytes = B.map complement (slice offset (spanLength range) (targetOf targets target))
   in maybe (error "flip' builds an exact-length edit") id (edit range bytes)

emptyCatalog :: Catalog
emptyCatalog = Catalog [] [] (EnumeratedWrites [] Proved) []

piece :: PieceKind -> [Area] -> [Span] -> [Edit] -> [Footprint] -> Piece
piece kind areas carriers edits footprint = Piece kind (Set.fromList areas) carriers edits footprint [] emptyCatalog NoFacts

-- | An edit writing one little-endian word.
wordEdit :: Target -> Word32 -> Word32 -> Edit
wordEdit target address value = maybe (error "wordEdit builds an exact-length edit") id (edit (at target address (address + 4)) (U.putWord32 value))

reads' :: Footprint
reads' = Footprint (at LoaderA 0x40400 0x40500) AdmissionRead (Citation "synthetic")

rebuilt :: Canonical -> U.Parts -> IO B.ByteString
rebuilt source parts = either fail pure (U.rebuildContainer (canonicalParsed source) parts)

declaration :: PieceKind -> [Span] -> [Footprint] -> PieceDeclaration
declaration kind carriers footprint = PieceDeclaration kind (Set.singleton FirstStageArea) carriers footprint [] emptyCatalog NoFacts

keyPaths :: J.Value -> Set.Set [Text]
keyPaths value = case value of
  J.Object fields -> Set.unions [Set.insert [Key.toText key] (Set.map (Key.toText key :) (keyPaths inner)) | (key, inner) <- KeyMap.toList fields]
  J.Array items -> Set.unions (map keyPaths (V.toList items))
  _ -> Set.empty

boundary :: Label -> BoundaryKind -> Core -> Word32 -> Bool -> Boundary
boundary name kind core pc stop = boundaryOn name kind [core] pc stop

boundaryOn :: Label -> BoundaryKind -> [Core] -> Word32 -> Bool -> Boundary
boundaryOn name kind listed pc stop = Boundary name kind set (Pc pc) (Just (Pc (pc + 0x100000))) stop CompareSequence once Nothing
  where
    once = maybe (error "positive record limit") id (recordLimit 1)
    set = maybe (error "nonempty cores") id (cores listed)

labelled :: PieceKind -> Text -> IO Label
labelled kind name = either (fail . show) pure (label kind name)

-- | A schema-4 manifest with the given guard and pieces JSON.
manifestWith :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString
manifestWith attach rules pieces =
  "{\"schema\":4,\"guard\":{\"attach\":\"" <> attach <> "\",\"attach_state\":" <> rules <> "},\"pieces\":" <> pieces <> "}"

emptyRules :: B.ByteString
emptyRules = "{\"peers\":[],\"carriers\":[],\"allowances\":[],\"registers\":[],\"board\":[],\"relations\":{}}"

onePiece :: B.ByteString
onePiece = "[{\"kind\":\"first-stage\",\"inputs\":{\"image\":\"a.bin\",\"symbols\":\"a.symbols\"}}]"

rulesFrom :: J.Value -> Either Refusal AttachRules
rulesFrom = parseAttachRules

digestText :: B.ByteString -> Text
digestText = T.pack . SHA.sha256Hex . SHA.sha256

member :: Text -> J.Value -> J.Value
member key value = case value of
  J.Object fields -> maybe J.Null id (KeyMap.lookup (Key.fromText key) fields)
  _ -> J.Null

element :: Int -> J.Value -> J.Value
element index value = case value of
  J.Array items | index < V.length items -> items V.! index
  _ -> J.Null

main :: IO ()
main = do
  arguments <- getArgs
  fixturePath <- case arguments of
    [path] -> pure path
    _ -> fail "usage: startup-artifact-tests COMPOSED_CONTRACT_FIXTURE"
  counter <- newIORef 0
  let ok = check counter
  source <- either (fail . show) pure (canonical fixtureDat fixtureImage4 fixtureImage5)
  let parts = U.parsedParts (canonicalParsed source)
      targets = canonicalTargets source
      pre = U.partPreTable parts

  -- Targets are the uncomplemented pre-table bytes at their slots and segments.
  ok "record1 is the complemented slot" (targetOf targets Record1 == B.map complement (slice 0x40000 0x10000 pre))
  ok "record2 is the complemented slot" (targetOf targets Record2 == B.map complement (slice 0x50000 0x10000 pre))
  ok "loader A is the complemented segment" (targetOf targets LoaderA == B.map complement (slice 0x60000 0xdae0 pre))
  ok "loader B is the complemented segment" (targetOf targets LoaderB == B.map complement (slice 0x70000 0xdaa0 pre))
  ok "image targets are the decoded images" (targetOf targets Image4 == fixtureImage4 && targetOf targets Image5 == fixtureImage5)
  let moved = parts {U.partSegments = [if U.segId s == 0 then s {U.segLoad = 0x41000} else s | s <- U.partSegments parts]}
  ok "loader A at another load address refuses" (refusal (extractPreTable moved) == Just (TargetGeometryRefused LoaderA))
  ok "canonical with a short image refuses" (refusal (canonical fixtureDat (B.drop 1 fixtureImage4) fixtureImage5) == Just CanonicalRefused)

  -- Complement round trip.
  let e1 = flip' targets (at Record1 0x6000 0x6010)
      e2 = flip' targets (at LoaderB 0xd0100 0xd0108)
      e3 = flip' targets (at LoaderA 0x40430 0x40440)
  edited <- either (fail . show) pure (applyPreTableEdits [e1, e2, e3] parts)
  expected <- either (fail . show) pure (applyTargetEdits [e1, e2, e3] targets)
  extracted <- either (fail . show) pure (extractPreTable edited)
  ok "extracting applied edits yields the edited targets"
    (and [M.lookup target extracted == Just (targetOf expected target) | target <- preTableTargets])
  ok "applied edits are stored complemented"
    (slice (0x40000 + 0x6000) 16 (U.partPreTable edited) == B.map complement (editBytes e1)
      && slice (0x70000 + 0x100) 8 (U.partPreTable edited) == B.map complement (editBytes e2))
  let undo = [flip' expected (editSpan change) | change <- [e1, e2, e3]]
  ok "re-applying canonical bytes restores the parts" (applyPreTableEdits undo edited == Right parts)
  editedDat <- rebuilt source edited
  ok "edited parts rebuild and reparse to the edited targets"
    ((U.parseContainer editedDat >>= either (Left . show) Right . extractPreTable . U.parsedParts) == Right extracted)
  ok "image edits are not pre-table edits"
    (refusal (applyPreTableEdits [flip' targets (at Image4 0x61c400 0x61c404)] parts) == Just (TargetGeometryRefused Image4))

  -- Piece derivation.
  let stagedOf edits = either (fail . show) pure (applyPreTableEdits edits parts) >>= rebuilt source
  stagedE1 <- stagedOf [e1]
  let derived carriers = derivePiece source (declaration FirstStage carriers []) (StagedOutput stagedE1 Nothing Nothing)
  ok "a change inside its carrier derives that edit" (fmap pieceEdits (derived [at Record1 0x6000 0x7000]) == Right [e1])
  ok "an edit may span abutting carriers"
    (fmap pieceEdits (derived [at Record1 0x6000 0x6008, at Record1 0x6008 0x6010]) == Right [e1])
  ok "a change outside its carriers refuses"
    (refusal (derived [at Record1 0x6000 0x6008]) == Just (EditOutsideCarriers FirstStage (at Record1 0x6000 0x6010)))
  outsideDat <- rebuilt source parts {U.partPreTable = B.take 0x100 pre <> B.singleton (complement (B.index pre 0x100)) <> B.drop 0x101 pre}
  ok "a change outside every target refuses"
    (refusal (derivePiece source (declaration FirstStage [at Record1 0 0x10000] []) (StagedOutput outsideDat Nothing Nothing))
      == Just (PieceChangesOutsideTargets FirstStage))
  ok "an unparsable staged DAT refuses"
    (refusal (derivePiece source (declaration FirstStage [] []) (StagedOutput (B.take 100 fixtureDat) Nothing Nothing))
      == Just (PieceChangesOutsideTargets FirstStage))
  let record4 = U.partRecord4 parts
      stream = U.recStream record4
  record4Dat <- rebuilt source parts {U.partRecord4 = record4 {U.recStream = B.map complement stream}}
  ok "an undeclared image record change refuses"
    (refusal (derivePiece source (declaration FirstStage [at Image4 0x61c400 0x89ec00] []) (StagedOutput record4Dat Nothing Nothing))
      == Just (PieceChangesUndecodedImage FirstStage Image4))
  let image4Edit = flip' targets (at Image4 0x700000 0x700008)
  image4After <- either (fail . show) pure (applyTargetEdits [image4Edit] targets)
  ok "a declared image change derives image edits"
    (fmap pieceEdits (derivePiece source (declaration Record2Link [at Image4 0x700000 0x700010] [])
      (StagedOutput fixtureDat (Just (targetOf image4After Image4)) Nothing)) == Right [image4Edit])
  ok "own carriers are subtracted from the footprint"
    (fmap pieceFootprint (derivePiece source (declaration FirstStage [at Record1 0x6000 0x7000] [Footprint (at Record1 0 0x10000) AdmissionRead (Citation "c")])
      (StagedOutput stagedE1 Nothing Nothing))
      == Right [Footprint (at Record1 0 0x6000) AdmissionRead (Citation "c"), Footprint (at Record1 0x7000 0x10000) AdmissionRead (Citation "c")])

  -- Composition rules.
  let composeWith :: [Join TestJoin] -> [Piece] -> Either Refusal (Composition TestJoin)
      composeWith joins = compose joins source
      first = piece FirstStage [FirstStageArea] [at Record1 0x6000 0x6010] [e1] [reads']
      publisher = piece PrimaryPublisher [PrimaryInitializerArea] [at LoaderA 0x40430 0x409ac] [e3] []
      overlap = at LoaderA 0x40430 0x40500
      join spans members basis = Join TestJoin (Set.fromList members) spans basis []
      proved name predicate = Join TestJoin (Set.fromList [FirstStage, PrimaryPublisher]) [overlap] (HostProof "synthetic") [HostCheck name predicate]
  ok "duplicate piece kinds refuse" (refusal (composeWith [] [first, first]) == Just (DuplicatePieceKind FirstStage))
  ok "shared areas refuse"
    (refusal (composeWith [] [first, piece PrimaryPublisher [FirstStageArea] [] [] []]) == Just (SharedArea FirstStage PrimaryPublisher FirstStageArea))
  ok "overlapping carriers refuse"
    (refusal (composeWith [] [first {pieceFootprint = []}, piece Record2Link [CriticalPairArea] [at Record1 0x600c 0x6020] [] []])
      == Just (CarrierOverlap FirstStage Record2Link (at Record1 0x600c 0x6010)))
  ok "an edit outside its carriers refuses at composition"
    (refusal (composeWith [] [piece FirstStage [FirstStageArea] [at Record1 0x6000 0x6008] [e1] []])
      == Just (EditOutsideCarriers FirstStage (at Record1 0x6000 0x6010)))
  ok "a footprint over another piece's carrier refuses without a join"
    (refusal (composeWith [] [first, publisher]) == Just (FootprintOverlap FirstStage PrimaryPublisher overlap))
  let observed = join [overlap] [FirstStage, PrimaryPublisher] (ObservationQualified "synthetic")
  ok "a registered join covering the conflict admits it as pending"
    (fmap compositionQualification (composeWith [observed] [first, publisher]) == Right (Pending ["test-join"]))
  ok "a join covering part of the conflict refuses the rest"
    (refusal (composeWith [join [at LoaderA 0x40430 0x40480] [FirstStage, PrimaryPublisher] (ObservationQualified "partial")] [first, publisher])
      == Just (FootprintOverlap FirstStage PrimaryPublisher (at LoaderA 0x40480 0x40500)))
  ok "a join with an absent member does not apply"
    (refusal (composeWith [join [overlap] [FirstStage, PrimaryPublisher, Record2Link] (ObservationQualified "absent")] [first, publisher])
      == Just (FootprintOverlap FirstStage PrimaryPublisher overlap))
  ok "a join span that is not a conflict refuses"
    (refusal (composeWith [join [overlap, at LoaderA 0x41000 0x41004] [FirstStage, PrimaryPublisher] (ObservationQualified "stale")] [first, publisher])
      == Just (JoinSpanNotConflicting "test-join" (at LoaderA 0x41000 0x41004)))
  ok "a failing host check refuses"
    (refusal (composeWith [proved "never" (const False)] [first, publisher]) == Just (HostProofFailed "test-join" "never"))
  ok "a passing host proof composes host-qualified"
    (fmap compositionQualification (composeWith [proved "always" (const True)] [first, publisher]) == Right HostComposed)
  ok "a join's spans in a composition are the member conflicts inside its bounds"
    (fmap (map appliedSpans . compositionJoins) (composeWith [join [at LoaderA 0x40000 0x4dae0] [FirstStage, PrimaryPublisher] (ObservationQualified "bounded")] [first, publisher])
      == Right [[overlap]])

  -- Registered J1 joins over synthetic pieces.
  let composeRegistered :: [Piece] -> Either Refusal (Composition JoinKind)
      composeRegistered = compose registeredJoins source
      firstEntry = 0x6000
      secondaryEntry = 0x10400
      starter entry = (piece FirstStage [FirstStageArea] [at Record1 0x20 0x24, at Record1 0x6000 0x64ac]
          [wordEdit Record1 0x20 firstEntry, flip' targets (at Record1 0x6000 0x6004)]
          [Footprint (at Record2 0x10000 0x20000) RuntimePremise (Citation "synthetic")])
        {pieceFacts = FirstStageFacts (Pc entry) (LinkedBytes B.empty "")}
      link entry bindings' footprint = (piece Record2Link [CriticalPairArea] [at Record2 0x10020 0x10024, at Record2 0x10400 0x110e0]
          [wordEdit Record2 0x10020 secondaryEntry, flip' targets (at Record2 0x10400 0x10404)] footprint)
        {pieceFacts = Record2LinkFacts (Pc entry) bindings' (LinkedBytes (B.replicate 0x40 0) "") (ScratchBytes 32) (StackDepth 128)}
      onRecord1 = [Footprint (at Record1 0 0x10000) RuntimePremise (Citation "synthetic")]
      rendezvousName = "first-stage-secondary-rendezvous"
  ok "the rendezvous join refuses a record-2 image whose prefix facts do not derive on the composed images"
    (refusal (composeRegistered [starter firstEntry, link secondaryEntry [] onRecord1])
      == Just (HostProofFailed rendezvousName "prefix-owned-facts-on-composed-images"))
  ok "the rendezvous join refuses a first-stage entry that is not the record-1 reset literal"
    (refusal (composeRegistered [starter 0x6004, link secondaryEntry [] onRecord1])
      == Just (HostProofFailed rendezvousName "record1-reset-literal-is-first-stage-entry"))
  ok "the rendezvous join refuses a secondary entry that is not the record-2 reset literal"
    (refusal (composeRegistered [starter firstEntry, link 0x10404 [] onRecord1])
      == Just (HostProofFailed rendezvousName "record2-reset-literal-is-secondary-entry"))
  let root = U.word32At (targetOf targets LoaderA) 0x10
      bound = [ServiceBinding 0x164 0x41140, ServiceBinding 0x168 0x41164]
      onLoaderA = [Footprint (at LoaderA 0x40430 0x409ac) AdmissionRead (Citation "synthetic")]
      emitter rows extra = (piece PrimaryPublisher [PrimaryInitializerArea] ([at LoaderA 0x40430 0x409ac] ++ map editSpan extra)
          (flip' targets (at LoaderA 0x40430 0x40434) : extra) [])
        {pieceFacts = PublisherFacts (PublisherFrame (Pc 0x40430) (Guest 0x40063450)) rows}
      emitted = [Publication (Guest (root + offset)) target | ServiceBinding offset target <- bound]
      bindingsName = "critical-pair-publisher-bindings"
  ok "the binding join admits the record-2 link and publisher host-qualified"
    (fmap compositionQualification (composeRegistered [link secondaryEntry bound onLoaderA, emitter emitted []]) == Right HostComposed)
  ok "the binding join refuses a publication with another target"
    (refusal (composeRegistered [link secondaryEntry bound onLoaderA, emitter (Publication (Guest (root + 0x164)) 0x41144 : drop 1 emitted) []])
      == Just (HostProofFailed bindingsName "core0-bindings-published-once"))
  ok "the binding join refuses a binding published twice"
    (refusal (composeRegistered [link secondaryEntry bound onLoaderA, emitter (emitted ++ take 1 emitted) []])
      == Just (HostProofFailed bindingsName "core0-bindings-published-once"))
  ok "the binding join refuses publications at another root"
    (refusal (composeRegistered [link secondaryEntry bound onLoaderA, emitter [Publication (Guest (root + 4 + offset)) target | ServiceBinding offset target <- bound] []])
      == Just (HostProofFailed bindingsName "core0-bindings-published-once"))
  ok "the binding join refuses a changed loader-A root header"
    (refusal (composeRegistered [link secondaryEntry bound onLoaderA, emitter emitted [wordEdit LoaderA 0x40010 (complement root)]])
      == Just (HostProofFailed bindingsName "loader-a-root-unchanged"))
  ok "registered joins whose members are absent are ignored"
    (fmap (\composition -> (compositionQualification composition, length (compositionJoins composition))) (composeRegistered [link secondaryEntry bound onRecord1])
      == Right (HostComposed, 0))

  -- Catalog validation.
  entryLabel <- labelled FirstStage "entry"
  readLabel' <- labelled FirstStage "cells"
  ok "labels outside kebab case refuse" (refusal (label FirstStage "Entry") == Just (CatalogRefused FirstStage (InvalidLabel "Entry")))
  let catalogOf boundaries code rows = Catalog boundaries code (EnumeratedWrites [] (Premised (Premise "p"))) rows
      withCatalog p catalog = p {pieceCatalog = catalog}
      firstCatalog = catalogOf [boundary entryLabel EntryBoundary Core0 0x6000 False] [CodeSpan 0x6000 0x6010] []
  ok "an entry outside owned code refuses"
    (refusal (composeWith [] [withCatalog first (catalogOf [boundary entryLabel EntryBoundary Core0 0x5000 False] [CodeSpan 0x6000 0x6010] [])] )
      == Just (CatalogRefused FirstStage (OwnedBoundaryOutsideOwnedCode entryLabel)))
  ok "a read at a boundary that does not stop refuses"
    (refusal (composeWith [] [withCatalog first (catalogOf [boundary entryLabel EntryBoundary Core0 0x6000 False] [CodeSpan 0x6000 0x6010]
      [CatalogRead readLabel' entryLabel 0x40020000 8 Nothing])])
      == Just (CatalogRefused FirstStage (ReadBoundaryDoesNotStop entryLabel)))
  ok "TCM writes on a core without boundaries refuse"
    (refusal (composeWith [] [withCatalog first (Catalog [boundary entryLabel EntryBoundary Core0 0x6000 False] [CodeSpan 0x6000 0x6010]
      (EnumeratedWrites [WriteRange (TcmTarget Core1 DataBank) 0 0x100] Proved) [])])
      == Just (CatalogRefused FirstStage (TcmCoreWithoutBoundary Core1)))
  publisherEntry <- labelled PrimaryPublisher "entry"
  ok "a composed PC shared by two pieces refuses"
    (refusal (composeWith [observed] [withCatalog first firstCatalog
      , withCatalog publisher (catalogOf [boundary publisherEntry ReturnBoundary Core0 0x6000 False] [] [])])
      == Just (CatalogRefused PrimaryPublisher (DuplicateComposedPc (Pc 0x6000))))
  ok "owned code shared by two pieces refuses, alias spelling included"
    (refusal (composeWith [observed] [withCatalog first firstCatalog
      , withCatalog publisher (catalogOf [] [CodeSpan 0x4000600c 0x40006020] [])])
      == Just (CatalogRefused PrimaryPublisher (OwnedCodeOverlap FirstStage)))
  let unretained = (boundary entryLabel EntryBoundary Core0 0x6000 True) {boundaryRetainedPc = Nothing}
  ok "a read at a boundary without a retained PC needs a difference"
    (refusal (composeWith [] [withCatalog first (catalogOf [unretained] [CodeSpan 0x6000 0x6010] [CatalogRead readLabel' entryLabel 0x40020000 8 Nothing])])
      == Just (CatalogRefused FirstStage (ReadNeedsDiffers readLabel')))
  ok "a read at a boundary without a retained PC with a stated difference composes"
    (either (const False) (const True)
      (composeWith [] [withCatalog first (catalogOf [unretained] [CodeSpan 0x6000 0x6010] [CatalogRead readLabel' entryLabel 0x40020000 8 (Just "owned only")])]))
  let yielding = (boundary entryLabel EntryBoundary Core0 0x6000 True) {boundaryYieldsTo = Just PrimaryInitializerArea}
      yieldingFirst = withCatalog first (catalogOf [yielding] [CodeSpan 0x6000 0x6010] [CatalogRead readLabel' entryLabel 0x40020000 8 Nothing])
      kept composition = [ (map boundaryLabel (catalogBoundaries (pieceCatalog p)), map readLabel (catalogReads (pieceCatalog p)))
                         | p <- compositionPieces composition, pieceKind p == FirstStage ]
  ok "a boundary yielding to an area another piece claims is dropped with its reads"
    (fmap kept (composeWith [observed] [yieldingFirst, publisher]) == Right [([], [])])
  ok "a boundary yielding to an unclaimed area stays with its reads"
    (fmap kept (composeWith [] [yieldingFirst]) == Right [([entryLabel], [readLabel'])])
  ok "a composition without edits refuses"
    (refusal (composeWith [] [piece FirstStage [FirstStageArea] [at Record1 0x6000 0x6010] [] []]) == Just CompositionUnchanged)
  ok "the guard attach boundary stops"
    (fmap (map boundaryStop . concatMap (catalogBoundaries . pieceCatalog)) (attachGuard entryLabel [withCatalog first firstCatalog]) == Right [True])
  ok "an unknown guard attach boundary refuses"
    (refusal (attachGuard publisherEntry [withCatalog first firstCatalog]) == Just (ManifestRefused (GuardAttachUnknown "primary-publisher/entry")))

  -- Postconditions, with abutting edits from two pieces.
  let left = flip' targets (at Record1 0x6000 0x6004)
      right = flip' targets (at Record1 0x6004 0x6008)
      pieceLeft = piece FirstStage [FirstStageArea] [at Record1 0x6000 0x6004] [left] []
      pieceRight = piece PrimaryPublisher [PrimaryInitializerArea] [at Record1 0x6004 0x6008, at Image4 0x700000 0x700010] [right, image4Edit] []
  abutting <- either (fail . show) pure (composeWith [] [pieceLeft, pieceRight])
  composedDat <- either (fail . show) pure (preTableDat source abutting)
  let composedImage4 = targetOf (compositionTargets abutting) Image4
  ok "abutting edits from two pieces compose and verify"
    (fmap (`targetOf` Record1) (verifyComposed source abutting composedDat composedImage4 fixtureImage5)
      == Right (targetOf (compositionTargets abutting) Record1))
  ok "a composed image missing its edit refuses"
    (refusal (verifyComposed source abutting composedDat fixtureImage4 fixtureImage5) == Just (ComposedDiffMismatch Image4))
  ok "a composed image with an extra change refuses"
    (refusal (verifyComposed source abutting composedDat composedImage4 (B.map complement fixtureImage5)) == Just (ComposedDiffMismatch Image5))
  composedParts <- either fail (pure . U.parsedParts) (U.parseContainer composedDat)
  extraDat <- either (fail . show) pure (applyPreTableEdits [flip' targets (at Record1 0x6100 0x6101)] composedParts) >>= rebuilt source
  ok "a composed target change outside the edits refuses"
    (refusal (verifyComposed source abutting extraDat composedImage4 fixtureImage5) == Just (ComposedDiffMismatch Record1))
  let composedPre = U.partPreTable composedParts
  strayDat <- rebuilt source composedParts {U.partPreTable = B.take 0x100 composedPre <> B.singleton (complement (B.index composedPre 0x100)) <> B.drop 0x101 composedPre}
  ok "a composed change outside every target refuses"
    (refusal (verifyComposed source abutting strayDat composedImage4 fixtureImage5) == Just ComposedRestoreMismatch)
  ok "an unparsable composed DAT refuses"
    (refusal (verifyComposed source abutting (B.take 100 composedDat) composedImage4 fixtureImage5) == Just ComposedUnparsed)

  -- Manifest.
  let manifestText = manifestWith "first-stage/entry" emptyRules onePiece
  ok "a manifest parses"
    (parseManifest manifestText == Right (Manifest entryLabel emptyAttachRules [SingleImagePiece FirstStage (LinkedPaths "a.bin" "a.symbols")] WithoutApplication))
  let withApplication section = B.init manifestText <> ",\"application\":" <> section <> "}"
  ok "an empty application member asks for the application section"
    (fmap manifestApplication (parseManifest (withApplication "{}")) == Right WithApplication)
  ok "an application member with a key refuses"
    (refusal (parseManifest (withApplication "{\"settle\":1}")) == Just (ManifestRefused (ManifestUnknownKey "application.settle")))
  ok "an application member that is not an object refuses"
    (refusal (parseManifest (withApplication "true")) == Just (ManifestRefused (ManifestMalformed "application is not an object")))
  let linkedJson name = "{\"image\":\"" <> name <> ".bin\",\"symbols\":\"" <> name <> ".symbols\"}"
      startupInputs = "{\"loader_a\":" <> linkedJson "a" <> ",\"loader_b\":" <> linkedJson "b" <> ",\"dispatcher\":"
        <> linkedJson "d" <> ",\"installer\":" <> linkedJson "i" <> "}"
      linkedPaths name = LinkedPaths (name <> ".bin") (name <> ".symbols")
  ok "a loader-startup piece parses its four linked images"
    (fmap manifestPieces (parseManifest (manifestWith "first-stage/entry" emptyRules ("[{\"kind\":\"loader-startup\",\"inputs\":" <> startupInputs <> "}]")))
      == Right [LoaderStartupPiece (LoaderStartupPaths (linkedPaths "a") (linkedPaths "b") (linkedPaths "d") (linkedPaths "i"))])
  let unknownKey parsed = case refusal parsed of
        Just (ManifestRefused (ManifestUnknownKey _)) -> True
        _ -> False
  ok "a loader-startup piece with one linked image refuses"
    (unknownKey (parseManifest (manifestWith "first-stage/entry" emptyRules "[{\"kind\":\"loader-startup\",\"inputs\":{\"image\":\"a.bin\",\"symbols\":\"a.symbols\"}}]")))
  ok "a single-image piece with four linked images refuses"
    (unknownKey (parseManifest (manifestWith "first-stage/entry" emptyRules ("[{\"kind\":\"loader-orchestration-b\",\"inputs\":" <> startupInputs <> "}]"))))
  ok "an unknown top-level key refuses"
    (refusal (parseManifest "{\"schema\":4,\"guard\":{\"attach\":\"first-stage/entry\",\"attach_state\":{}},\"pieces\":[],\"extra\":1}")
      == Just (ManifestRefused (ManifestUnknownKey "manifest.extra")))
  ok "an unknown input key refuses"
    (refusal (parseManifest (manifestWith "first-stage/entry" emptyRules "[{\"kind\":\"first-stage\",\"inputs\":{\"image\":\"a\",\"symbols\":\"b\",\"dat\":\"c\"}}]"))
      == Just (ManifestRefused (ManifestUnknownKey "piece.inputs.dat")))
  ok "an unknown kind refuses"
    (refusal (parseManifest (manifestWith "first-stage/entry" emptyRules "[{\"kind\":\"unregistered-piece\",\"inputs\":{\"image\":\"a\",\"symbols\":\"b\"}}]"))
      == Just (ManifestRefused (ManifestUnknownKind "unregistered-piece")))
  ok "another schema refuses"
    (refusal (parseManifest "{\"schema\":2,\"guard\":{\"attach\":\"first-stage/entry\"},\"pieces\":[]}") == Just (ManifestRefused ManifestSchemaUnsupported))
  ok "a manifest without attach_state refuses"
    (refusal (parseManifest "{\"schema\":4,\"guard\":{\"attach\":\"first-stage/entry\"},\"pieces\":[]}")
      == Just (ManifestRefused (ManifestMalformed "guard lacks attach_state")))
  ok "a schema-3 manifest refuses"
    (refusal (parseManifest "{\"schema\":3,\"guard\":{\"attach\":\"first-stage/entry\",\"attach_state\":{}},\"pieces\":[]}")
      == Just (ManifestRefused ManifestSchemaUnsupported))
  ok "duplicate manifest kinds refuse"
    (refusal (parseManifest (manifestWith "first-stage/entry" emptyRules "[{\"kind\":\"first-stage\",\"inputs\":{\"image\":\"a\",\"symbols\":\"b\"}},{\"kind\":\"first-stage\",\"inputs\":{\"image\":\"c\",\"symbols\":\"d\"}}]"))
      == Just (DuplicatePieceKind FirstStage))
  ok "an attach that is not a label refuses"
    (refusal (parseManifest (manifestWith "nowhere" emptyRules onePiece)) == Just (ManifestRefused (GuardAttachUnknown "nowhere")))

  -- Footprint normalization and write resolution.
  ok "overlapping footprints normalize to disjoint spans with merged citations"
    (normalizeFootprint [Footprint (at LoaderA 0x40000 0x40010) AdmissionRead (Citation "a"), Footprint (at LoaderA 0x40008 0x40020) RuntimePremise (Citation "b")]
      == [ (at LoaderA 0x40000 0x40008, AdmissionRead, [Citation "a"])
         , (at LoaderA 0x40008 0x40010, AdmissionRead, [Citation "a", Citation "b"])
         , (at LoaderA 0x40010 0x40020, RuntimePremise, [Citation "b"]) ])
  ok "the first stage's writes are enumerated on core 0 and premised"
    (case allowRanges [at Record1 0x6000 0x64d8] (EnumeratedWrites FirstStageFootprint.writes (Premised FirstStageFootprint.writesPremise)) of
      EmittedWrites ranges "premised" (Just _) -> ranges == sort FirstStageFootprint.writes
        && all (\(WriteRange target _ _) -> target `elem` [CorePhysicalTarget Core0, TcmTarget Core0 InstructionBank]) ranges
      _ -> False)
  ok "premised allow ranges are physical and exclude every owned carrier"
    (allowRanges [at LoaderA 0x40430 0x409ac]
      (EnumeratedWrites [WriteRange PhysicalTarget 0x40040000 0x40041000, WriteRange (TcmTarget Core0 DataBank) 0 0x100] (Premised (Premise "p")))
      == EmittedWrites [WriteRange PhysicalTarget 0x40000 0x40430, WriteRange PhysicalTarget 0x409ac 0x41000, WriteRange (TcmTarget Core0 DataBank) 0 0x100] "premised" (Just "p"))
  ok "core-bound physical ranges are translated and cut like shared ones"
    (allowRanges [at LoaderA 0x40430 0x409ac]
      (EnumeratedWrites [WriteRange (CorePhysicalTarget Core1) 0x40040400 0x40040500, WriteRange PhysicalTarget 0x10 0x20] (Premised (Premise "p")))
      == EmittedWrites [WriteRange PhysicalTarget 0x10 0x20, WriteRange (CorePhysicalTarget Core1) 0x40400 0x40430] "premised" (Just "p"))
  ok "the alias window ends at 0x60000000"
    (allowRanges [] (EnumeratedWrites [WriteRange PhysicalTarget 0x5fff0000 0x60000000, WriteRange PhysicalTarget 0x60000000 0x60001000] Proved)
      == EmittedWrites [WriteRange PhysicalTarget 0x1fff0000 0x20000000, WriteRange PhysicalTarget 0x60000000 0x60001000] "proved" Nothing)
  ok "a core-bound physical range on a core without boundaries refuses"
    (refusal (composeWith [] [withCatalog first (Catalog [boundary entryLabel EntryBoundary Core0 0x6000 False] [CodeSpan 0x6000 0x6010]
      (EnumeratedWrites [WriteRange (CorePhysicalTarget Core1) 0x100 0x200] Proved) [])])
      == Just (CatalogRefused FirstStage (TcmCoreWithoutBoundary Core1)))
  ok "a multi-core attach boundary refuses"
    (refusal (attachGuard entryLabel [withCatalog first (catalogOf [boundaryOn entryLabel EntryBoundary [Core0, Core1] 0x6000 False] [CodeSpan 0x6000 0x6010] [])])
      == Just (ManifestRefused (GuardAttachUnsuitable "first-stage/entry")))
  ok "an attach boundary without a retained PC refuses"
    (refusal (attachGuard entryLabel [withCatalog first (catalogOf [unretained] [CodeSpan 0x6000 0x6010] [])])
      == Just (ManifestRefused (GuardAttachUnsuitable "first-stage/entry")))

  -- Contract shape against the shared fixture.
  fixture <- B.readFile fixturePath >>= maybe (fail "fixture is not JSON") pure . J.decodeStrict
  handoffLabel <- labelled FirstStage "handoff"
  returnLabel <- labelled PrimaryPublisher "return"
  tableLabel <- labelled PrimaryPublisher "table-a"
  let contractFirst = withCatalog first (Catalog
        [boundary entryLabel EntryBoundary Core0 0x6000 False, boundary handoffLabel SiteBoundary Core0 0x6008 True]
        [CodeSpan 0x6000 0x6010] (EnumeratedWrites [WriteRange (CorePhysicalTarget Core0) 0x4300 0x5280] (Premised (Premise "first-stage stores")))
        [CatalogRead readLabel' handoffLabel 0x40020000 8 Nothing])
      contractPublisher = withCatalog publisher (Catalog
        [boundary publisherEntry EntryBoundary Core0 0x40430 False, boundary returnLabel ReturnBoundary Core0 0x409e0 True]
        [CodeSpan 0x40430 0x40580]
        (EnumeratedWrites [WriteRange PhysicalTarget 0x4dae0 0x4f10c] (Premised (Premise "scratch")))
        [CatalogRead tableLabel returnLabel 0x4004dae0 0x162c Nothing])
  contracted <- either (fail . show) pure (composeWith [observed] [contractFirst, contractPublisher])
  contractDat <- either (fail . show) pure (preTableDat source contracted)
  contractTargets <- either (fail . show) pure (verifyComposed source contracted contractDat fixtureImage4 fixtureImage5)
  let sources = AttachSources (canonicalDat source) contractDat (canonicalTargets source) contractTargets [] (compositionPieces contracted) entryLabel
      carriersOf = contractCarriers contracted
      absentPart (_, Span target (Guest start) (Guest end)) =
        J.object ["target" .= targetName target, "start" .= start, "end" .= end, "residency" .= ("absent" :: Text), "reason" .= ("illustrative" :: Text)]
      peer = J.object ["core" .= (1 :: Int), "retained_pc" .= [J.object ["start" .= (0xd0000 :: Int), "end" .= (0xdc000 :: Int)]]
        , "composed_pc" .= [J.object ["start" .= (0x10400 :: Int), "end" .= (0x11000 :: Int)]], "reason" .= ("waiting" :: Text)]
      rulesJson :: [J.Value] -> [J.Value] -> J.Value
      rulesJson residency allowances = J.object ["peers" .= [peer], "carriers" .= residency, "allowances" .= allowances, "registers" .= ([] :: [J.Value]), "board" .= ([] :: [J.Value]), "relations" .= J.object []]
  absentRules <- either (fail . show) pure (rulesFrom (rulesJson (map absentPart carriersOf) ([] :: [J.Value])))
  attachJson <- either (fail . show) pure (attachStateValue sources absentRules carriersOf)
  let contractJson = contractValue source manifestText contracted contractDat contractTargets entryLabel attachJson Nothing
  ok "the contract has exactly the fixture's keys" (keyPaths contractJson == keyPaths fixture)
  ok "a pending contract names its joins"
    (case contractJson of
      J.Object fields -> KeyMap.lookup "qualification" fields == Just (J.String "pending")
        && KeyMap.lookup "pending_joins" fields == Just (J.toJSON ["test-join" :: Text])
      _ -> False)

  -- Declared mutations.
  let linked words' = LinkedBytes (B.concat (map U.putWord32 words')) "00006000 00000000 T __first_stage_start\n"
      atcmWrite = [0xe59f0004, 0xee090f31, 0xeafffffe, 0x70000011]
      ownedAtcm = TcmRegionWrite Core0 InstructionBank (TcmRegister 0xffff8011) (TcmRegister 0x70000011) (Pc 0x6004)
      writerOf mutation = case mutation of
        TcmRegionWrite core bank previous next writer _ -> Just (core, bank, previous, next, writer)
        DecoderCommit {} -> Nothing
  ok "the first stage's TCM write is the unique ATCM MCR after its literal load"
    (fmap writerOf (FirstStageFootprint.tcmRegionWrite (linked atcmWrite)) == Right (writerOf (ownedAtcm (Citation "c"))))
  ok "a second TCM region write refuses"
    (refusal' (FirstStageFootprint.tcmRegionWrite (linked (atcmWrite ++ [0xee090f11]))))
  ok "a conditional MCR refuses"
    (refusal' (FirstStageFootprint.tcmRegionWrite (linked [0xe59f0004, 0x0e090f31, 0xeafffffe, 0x70000011])))
  ok "a literal other than 0x70000011 refuses"
    (refusal' (FirstStageFootprint.tcmRegionWrite (linked [0xe59f0004, 0xee090f31, 0xeafffffe, 0x70000013])))
  ok "an image without a TCM region write refuses"
    (refusal' (FirstStageFootprint.tcmRegionWrite (linked [0xe59f0004, 0xe1a00000, 0xeafffffe, 0x70000011])))
  let (retainedDeclared, composedDeclared) = declaredMutations contracted
      writers = map (\mutation -> case mutation of
        TcmRegionWrite core _ _ _ (Pc writer) _ -> (coreIndex core, writer)
        DecoderCommit _ _ _ core (Pc writer) _ -> (coreIndex core, writer))
  ok "the retained role declares the ROM commit and both stock ATCM writes"
    (writers retainedDeclared == [(0, 0x800022a8), (0, 0x18c4), (1, 0x118c4)])
  ok "a first stage without its TCM write declares none in the composed role"
    (writers composedDeclared == [(0, 0x800022a8), (1, 0x118c4)])
  withOwned <- either (fail . show) pure
    (composeWith [observed] [contractFirst {pieceFacts = FirstStageFacts (Pc 0x6000) (linked atcmWrite)}, contractPublisher])
  ok "the composed role takes the first stage's ATCM write in place of stock record 1's"
    (writers (snd (declaredMutations withOwned)) == [(0, 0x800022a8), (0, 0x6004), (1, 0x118c4)])
  ok "a composition without a first stage keeps stock record 1's ATCM write"
    (either (const False) (\composition -> writers (snd (declaredMutations composition)) == [(0, 0x800022a8), (0, 0x18c4), (1, 0x118c4)])
      (composeWith [] [contractPublisher]))

  -- Attach-state rules and their contract form.
  let firstCarrier = at Record1 0x6000 0x6010
      part range residency extra = case range of
        Span target (Guest start) (Guest end) -> J.object
          (["target" .= targetName target, "start" .= start, "end" .= end, "residency" .= (residency :: Text)] ++ extra)
      publisherAbsent = part (at LoaderA 0x40430 0x409ac) "absent" ["reason" .= ("loaded later" :: Text)]
      residentFirst = [part firstCarrier "resident" [], publisherAbsent]
      attachOf residency allowances = rulesFrom (rulesJson residency allowances) >>= \rules -> attachStateValue sources rules carriersOf
      firstPart = element 0 . member "parts" . element 0 . member "carriers"
  case attachOf residentFirst [] of
    Right value -> ok "a resident part carries both roles' digests over its target bytes"
      (member "canonical_sha256" (firstPart value) == J.toJSON (digestText (slice 0x6000 0x10 (targetOf (canonicalTargets source) Record1)))
        && member "composed_sha256" (firstPart value) == J.toJSON (digestText (slice 0x6000 0x10 (targetOf contractTargets Record1)))
        && member "canonical_sha256" (firstPart value) /= member "composed_sha256" (firstPart value))
    Left problem -> fail (show problem)
  let split = [part (at Record1 0x6000 0x6008) "resident" [], part (at Record1 0x6008 0x6010) "absent" ["reason" .= ("r" :: Text)], publisherAbsent]
  ok "a carrier may be tiled by resident and absent parts"
    (either (const False) ((== 2) . V.length . (\v -> case v of J.Array items -> items; _ -> V.empty) . member "parts" . element 0 . member "carriers") (attachOf split []))
  let tilingRefused residency = case attachOf residency [] of
        Left (ManifestRefused (AttachStateRefused _)) -> True
        _ -> False
  ok "a gap in a carrier's parts refuses" (tilingRefused [part (at Record1 0x6000 0x6008) "resident" [], publisherAbsent])
  ok "a carrier without parts refuses" (tilingRefused [part firstCarrier "resident" []])
  ok "a part outside every carrier refuses" (tilingRefused (residentFirst ++ [part (at Record2 0x10400 0x10404) "resident" []]))
  ok "a resident part with a reason refuses" (tilingRefused [part firstCarrier "resident" ["reason" .= ("r" :: Text)], publisherAbsent])
  let copy transform = J.object ["label" .= ("staged-record" :: Text), "kind" .= ("copy" :: Text), "space" .= J.object ["target" .= ("physical" :: Text)]
        , "start" .= (0x50000 :: Int), "end" .= (0x50010 :: Int), "reason" .= ("r" :: Text)
        , "source" .= J.object ["dat" .= J.object ["offset" .= (0x40000 :: Int), "transform" .= (transform :: Text)]]]
      copyDigest which = member which . element 0 . member "allowances"
  case (attachOf residentFirst [copy "complement"], attachOf residentFirst [copy "identity"]) of
    (Right complemented, Right identical) -> do
      ok "a DAT copy carries both DATs' digests under its transform"
        (copyDigest "canonical_sha256" complemented == J.toJSON (digestText (B.map complement (slice 0x40000 0x10 (canonicalDat source))))
          && copyDigest "composed_sha256" identical == J.toJSON (digestText (slice 0x40000 0x10 contractDat)))
      ok "an allowance carries every allowance key"
        (case element 0 (member "allowances" complemented) of
          J.Object fields -> length (KeyMap.keys fields) == 14 && member "citation" (J.Object fields) == J.Null
          _ -> False)
    _ -> fail "copy allowances did not emit"
  let declared size = J.object ["label" .= ("word" :: Text), "kind" .= ("declared" :: Text), "space" .= J.object ["target" .= ("physical" :: Text)]
        , "start" .= (0 :: Int), "end" .= (size :: Int), "reason" .= ("r" :: Text), "citation" .= ("c" :: Text)]
      refusedRules value = case rulesFrom value of
        Left (ManifestRefused (AttachStateRefused _)) -> True
        _ -> False
  ok "a declared allowance within 64 KiB parses" (either (const False) (const True) (rulesFrom (rulesJson [] [declared 0x10000])))
  ok "a declared allowance above 64 KiB refuses" (refusedRules (rulesJson [] [declared 0x10001]))
  ok "an allowance with another kind's key refuses"
    (refusedRules (rulesJson [] [case declared 4 of J.Object fields -> J.Object (KeyMap.insert "core" (J.toJSON (0 :: Int)) fields); other -> other]))
  ok "a repeated allowance label refuses" (refusedRules (rulesJson [] [declared 4, declared 8]))
  ok "an unknown attach_state key refuses"
    (refusedRules (case rulesJson [] [] of J.Object fields -> J.Object (KeyMap.insert "extra" J.Null fields); other -> other))
  let withPeerSpans spans = case rulesJson [] [] of
        J.Object fields -> J.Object (KeyMap.insert "peers" (J.toJSON [J.object ["core" .= (1 :: Int), "retained_pc" .= spans
          , "composed_pc" .= [J.object ["start" .= (0x10400 :: Int), "end" .= (0x11000 :: Int)]], "reason" .= ("r" :: Text)]]) fields)
        other -> other
      interval' start end = J.object ["start" .= (start :: Int), "end" .= (end :: Int)]
  ok "overlapping peer PC spans refuse" (refusedRules (withPeerSpans [interval' 0xd0000 0xd1800, interval' 0xd1000 0xd2000]))
  ok "unsorted peer PC spans refuse" (refusedRules (withPeerSpans [interval' 0xd1000 0xd2000, interval' 0xd0000 0xd0800]))
  let exactNull = J.object ["pointer" .= ("/memory_decoders/pending/0" :: Text), "kind" .= ("exact" :: Text)
        , "retained" .= J.Null, "composed" .= (3 :: Int), "reason" .= ("r" :: Text)]
      withBoard entry = case rulesJson [] [] of
        J.Object fields -> J.Object (KeyMap.insert "board" (J.toJSON [entry]) fields)
        other -> other
  case rulesFrom (withBoard exactNull) >>= \rules -> attachStateValue sources rules [] of
    Right value -> ok "an exact board entry keeps a null value"
      (member "retained" (element 0 (member "board" value)) == J.Null
        && case element 0 (member "board" value) of J.Object fields -> KeyMap.member "retained" fields; _ -> False)
    Left problem -> fail (show problem)

  -- LZ submitted word: each role's record-4 length comes from its own DAT.
  let shortened = U.partRecord4 parts
      header4 = U.recHeader shortened
      streamKept = B.length (U.recStream shortened) - 4
      shorterParts = parts
        { U.partRecord4 = shortened { U.recHeader = header4 { U.recTotalLen = U.recTotalLen header4 - 4 }, U.recStream = B.take streamKept (U.recStream shortened) }
        , U.partGap45 = B.drop streamKept (U.recStream shortened) <> U.partGap45 parts }
  shorterDat <- rebuilt source shorterParts
  let withLz submitted allowances = case rulesJson [] allowances of
        J.Object fields -> J.Object (KeyMap.insert "relations" (J.object ["lz" .= J.object ["citation" .= ("lz" :: Text), "submitted" .= submitted]]) fields)
        other -> other
      submittedAt address = J.object ["address" .= (address :: Int), "citation" .= ("c" :: Text)]
  case rulesFrom (withLz (submittedAt 0x42a4) []) >>= \rules -> attachStateValue sources { composedDatBytes = shorterDat } rules [] of
    Right value -> ok "the submitted word carries each role's record-4 length from its own DAT"
      (member "record_length" (member "submitted" (member "lz" (member "relations" value)))
        == J.object ["retained" .= U.recTotalLen header4, "composed" .= (U.recTotalLen header4 - 4)])
    Left problem -> fail (show problem)
  ok "a submitted word inside an allowance refuses" (refusedRules (withLz (submittedAt 0) [declared 4]))
  ok "an unaligned submitted word refuses" (refusedRules (withLz (submittedAt 0x42a6) []))

  -- Log sites: an owned site resolves from its piece's symbols, and both sites are word-checked in their role's image.
  let wordBytes :: Word32 -> B.ByteString
      wordBytes word = B.pack [fromIntegral (word `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]
      withWords target offset patch images = maybe (error "six targets") id (targetBytes (M.fromList
        [ (name, if name == target then B.take offset bytes <> B.concat (map wordBytes patch) <> B.drop (offset + 4 * length patch) bytes else bytes)
        | (name, bytes) <- targetList images ]))
      loadStore = [0xe5912000, 0xe5802000]
      siteSources = sources
        { canonicalTargetBytes = withWords Record1 0x40f4 loadStore (canonicalTargetBytes sources)
        , composedTargetBytes = withWords Record1 0x64a8 loadStore (composedTargetBytes sources)
        , ownedSymbols = [(FirstStage, "000064a8 00000000 T xm1_first_stage_log_read\n000064ac 00000000 T xm1_first_stage_log_store\n")]
        }
      site name role coreNumber from = J.object
        ["label" .= (name :: Text), "role" .= (role :: Text), "core" .= (coreNumber :: Int), "source" .= from, "citation" .= ("c" :: Text)]
      stock pc = J.object ["stock" .= (pc :: Int)]
      owned = J.object ["owned" .= ("first-stage" :: Text)]
      withSites :: [J.Value] -> J.Value
      withSites sites = case rulesJson [] [] of
        J.Object fields -> J.Object (KeyMap.insert "relations" (J.object
          [ "timer" .= J.object ["citation" .= ("t" :: Text)]
          , "log" .= J.object ["span" .= J.object ["start" .= (0xa0040 :: Int), "end" .= (0xa00c8 :: Int)], "sites" .= sites] ]) fields)
        other -> other
      sitesOf rules = rulesFrom rules >>= \parsed -> attachStateValue siteSources parsed []
      resolved value = [ (member "read_pc" row, member "store_pc" row)
                       | J.Array rows <- [member "sites" (member "log" (member "relations" value))], row <- V.toList rows ]
  case sitesOf (withSites [site "stock-record1" "retained" 0 (stock 0x40f4), site "owned-first-stage" "composed" 1 owned]) of
    Right value -> do
      ok "log sites resolve to their read and store PCs"
        (resolved value == [(J.toJSON (0x40f4 :: Int), J.toJSON (0x40f8 :: Int)), (J.toJSON (0x64a8 :: Int), J.toJSON (0x64ac :: Int))])
      ok "an absent relation member is null" (member "lz" (member "relations" value) == J.Null)
    Left problem -> fail (show problem)
  ok "a stock site whose words are not a load and store refuses"
    (either (const True) (const False) (sitesOf (withSites [site "stock-record1" "composed" 0 (stock 0x40f4)])))
  ok "an owned site of a piece without log symbols refuses"
    (either (const True) (const False) (sitesOf (withSites [site "owned" "composed" 0 (J.object ["owned" .= ("primary-publisher" :: Text)])])))
  ok "two sites with one role, core and read PC refuse"
    (either (const True) (const False) (sitesOf (withSites [site "a" "retained" 0 (stock 0x40f4), site "b" "retained" 0 (stock 0x40f4)])))
  ok "the same read PC on two cores parses"
    (either (const False) (const True) (sitesOf (withSites [site "a" "retained" 0 (stock 0x40f4), site "b" "retained" 1 (stock 0x40f4)])))
  ok "the A32 word checks tell word loads from word stores, media and unconditional space"
    (not (a32WordLoad 0xe5802000) && not (a32WordStore 0xe5912000) && a32WordLoad 0xe5111fff && a32WordStore 0xe7831102
      && not (a32WordLoad 0xe6111f10) && not (a32WordStore 0xe6011f10) && not (a32WordLoad 0xf5912000))
  ok "an owned site of a piece the manifest lacks refuses"
    (either (const True) (const False) (sitesOf (withSites [site "owned" "composed" 0 (J.object ["owned" .= ("record2-link" :: Text)])])))
  ok "an owned site whose store does not follow its read refuses"
    (either (const True) (const False) (rulesFrom (withSites [site "owned" "composed" 0 owned]) >>= \parsed -> attachStateValue siteSources
      { ownedSymbols = [(FirstStage, "000064a8 00000000 T xm1_first_stage_log_read\n000064b0 00000000 T xm1_first_stage_log_store\n")] } parsed []))
  ok "an owned site declared for the retained role is checked in the canonical image and refuses"
    (either (const True) (const False) (sitesOf (withSites [site "owned" "retained" 0 owned])))
  ok "a log span overlapping a physical allowance refuses"
    (refusedRules (case withSites [] of
      J.Object fields -> J.Object (KeyMap.insert "allowances" (J.toJSON [J.object ["label" .= ("word" :: Text), "kind" .= ("declared" :: Text)
        , "space" .= J.object ["target" .= ("physical" :: Text)], "start" .= (0xa0048 :: Int), "end" .= (0xa004c :: Int)
        , "reason" .= ("r" :: Text), "citation" .= ("c" :: Text)]]) fields)
      other -> other))
  ok "an unaligned log span refuses"
    (refusedRules (case withSites [] of
      J.Object fields -> J.Object (KeyMap.insert "relations" (J.object ["log" .= J.object
        ["span" .= J.object ["start" .= (0xa0042 :: Int), "end" .= (0xa00c8 :: Int)], "sites" .= ([] :: [J.Value])]]) fields)
      other -> other))

  -- Register sources: a word or row passes through; call_return and entry_state resolve against the composition.
  let registerRule :: J.Value -> J.Value -> J.Value
      registerRule retained composed = J.object ["core" .= (0 :: Int), "register" .= ("r5" :: Text), "mask" .= (0xffffffff :: Word32)
        , "retained" .= retained, "composed" .= composed, "reason" .= ("r" :: Text)]
      withRegisters entries = case rulesJson [] [] of
        J.Object fields -> J.Object (KeyMap.insert "registers" (J.toJSON entries) fields)
        other -> other
      registerOut sources' retained composed =
        element 0 . member "registers" <$> (rulesFrom (withRegisters [registerRule retained composed]) >>= \rules -> attachStateValue sources' rules [])
      refusesWith sources' retained composed = either (const True) (const False) (registerOut sources' retained composed)
      word :: Int -> J.Value
      word = J.toJSON
      rowOf name = J.object ["row" .= (name :: Text)]
      callReturn name = J.object ["call_return" .= (name :: Text)]
      entryState = J.object ["entry_state" .= J.object ["side" .= ("loader-a" :: Text), "service" .= ("private-initialize" :: Text)]]
      blToEntry = withWords Record1 0x6008 [0xebfffffc] (composedTargetBytes sources)
  ok "word register values render unchanged"
    (fmap (\value -> (member "retained" value, member "composed" value, member "reason" value)) (registerOut sources (word 1) (word 2))
      == Right (word 1, word 2, J.String "r"))
  ok "a row register value renders as its label"
    (fmap (member "composed") (registerOut sources (word 0) (rowOf "first-stage/handoff")) == Right (rowOf "first-stage/handoff"))
  ok "a row register value naming no composed boundary refuses" (refusesWith sources (word 0) (rowOf "first-stage/absent"))
  ok "an unknown register source refuses" (refusedRules (withRegisters [registerRule (word 0) (J.object ["bogus" .= (1 :: Int)])]))
  ok "a call_return site that is not a BL to the attach PC refuses" (refusesWith sources (word 0) (callReturn "first-stage/handoff"))
  ok "a call_return site that BLs to the attach PC resolves to its PC plus 4"
    (fmap (member "composed") (registerOut sources {composedTargetBytes = blToEntry} (word 0) (callReturn "first-stage/handoff")) == Right (word 0x600c))
  ok "a retained entry_state refuses" (refusesWith sources entryState (word 0))
  ok "an entry_state without a loader-A orchestration piece refuses" (refusesWith sources (word 0) entryState)

  -- Application section.
  plain <- either (fail . show) pure (plainImage parts)
  U.MediaImage plainBytes <- either fail pure (U.extractPlain parts)
  let patched base changes = either (error . show) id (applyTargetEdits [wordEdit target address value | (target, address, value) <- changes] base)
      stockImages = patched targets [(Image5, 0xbfbd20, 0xe92d4010), (Image4, 0x725214, 0xe59f0004)]
      role targets' = RoleImages targets' plain
      appOf carriers' composedTargets = application carriers' (role stockImages) (role composedTargets)
      rowsOf key name value = [row | J.Array rows <- [member key value], row <- V.toList rows, member "label" row == J.String name]
      sectionOf carriers' composedTargets = applicationValue <$> appOf carriers' composedTargets
      image5Slice :: TargetBytes -> Word32 -> Word32 -> B.ByteString
      image5Slice images start end = slice (fromIntegral (start - 0xbf1a00)) (fromIntegral (end - start)) (targetOf images Image5)
      number :: Word32 -> J.Value
      number = J.toJSON
  case sectionOf [] stockImages of
    Right value -> do
      ok "a site's words are the image5, image4 or plain-window word at its PC in each role"
        (map (\row -> (member "kind" row, member "retained_pc" row, member "composed_pc" row, member "retained_word" row, member "composed_word" row))
            (rowsOf "sites" "app-bfbd20" value)
            == [(J.String "pump", number 0xbfbd20, number 0xbfbd20, number 0xe92d4010, number 0xe92d4010)]
          && map (member "composed_word") (rowsOf "sites" "app-725214" value) == [number 0xe59f0004]
          && map (member "retained_word") (rowsOf "sites" "app-1109130" value) == [number (U.word32At plainBytes 0x9130)]
          && map (member "word") (rowsOf "queue_sites" "queue-725318" value) == [number 0x5a5a5a5a])
      ok "the section lists every pump site and the publication and initializer sites"
        (case member "sites" value of
          J.Array rows -> V.length rows == 56 && all (\name -> length (rowsOf "sites" name value) == 1) ["app-73b4b0", "app-73b558", "app-d985d8", "app-69d2e0", "app-110f9d8"]
          _ -> False)
    Left problem -> fail (show problem)
  ok "a site inside a piece's carrier refuses"
    (refusal (appOf [(LoaderStartup, at Image5 0xbfbd00 0xbfbd24)] stockImages) == Just (ApplicationRefused (SiteInCarrier LoaderStartup (Pc 0xbfbd20))))
  ok "a carrier ending at a site does not refuse"
    (either (const False) (const True) (appOf [(LoaderStartup, at Image5 0xbfbd00 0xbfbd20)] stockImages))
  let ownedPump = patched stockImages [(Image5, 0xbfbd20, 0xe92d4c10)]
      pumpCarrier = at Image5 0xbfbd20 0xbfbd40
  case sectionOf [(ApplicationPump, pumpCarrier)] ownedPump of
    Right value -> ok "a pump site inside an application-pump carrier carries each role's word"
      (map (\row -> (member "retained_word" row, member "composed_word" row)) (rowsOf "sites" "app-bfbd20" value)
        == [(number 0xe92d4010, number 0xe92d4c10)])
    Left problem -> fail (show problem)
  ok "a pump site inside another piece's carrier refuses"
    (refusal (appOf [(Image5Producer, pumpCarrier)] ownedPump) == Just (ApplicationRefused (SiteInCarrier Image5Producer (Pc 0xbfbd20))))
  ok "a non-pump site inside an application-pump carrier refuses"
    (refusal (appOf [(ApplicationPump, at Image5 0xbfb1b8 0xbfb1bc)] stockImages) == Just (ApplicationRefused (SiteInCarrier ApplicationPump (Pc 0xbfb1b8))))
  ok "a changed pump word outside every carrier refuses"
    (refusal (appOf [] ownedPump) == Just (ApplicationRefused (SiteWordDiffers (Pc 0xbfbd20))))
  ok "a composed site word other than the canonical word refuses"
    (refusal (appOf [] (patched stockImages [(Image4, 0x725214, 0)])) == Just (ApplicationRefused (SiteWordDiffers (Pc 0x725214))))
  let changed = patched stockImages [(Image5, 0xc22f78, 0)]
  case sectionOf [(LoaderStartup, at Image5 0xc22f78 0xc22f7c)] changed of
    Right value -> ok "code digests are each role's image5 bytes over the region"
      (map (\row -> (member "start" row, member "end" row, member "retained_sha256" row, member "composed_sha256" row)) (rowsOf "code" "task1-body" value)
          == [(number 0xc22f60, number 0xc2327c, J.toJSON (digestText (image5Slice stockImages 0xc22f60 0xc2327c)), J.toJSON (digestText (image5Slice changed 0xc22f60 0xc2327c)))]
        && image5Slice stockImages 0xc22f60 0xc2327c /= image5Slice changed 0xc22f60 0xc2327c
        && map (\row -> (member "retained_sha256" row, member "composed_sha256" row)) (rowsOf "code" "pump" value)
          == [(J.toJSON (digestText (image5Slice stockImages 0xbfbd20 0xbfbd40)), J.toJSON (digestText (image5Slice stockImages 0xbfbd20 0xbfbd40)))])
    Left problem -> fail (show problem)
  sectioned <- either (fail . show) pure (appOf [] stockImages)
  let withSection = contractValue source manifestText contracted contractDat contractTargets entryLabel attachJson (Just sectioned)
      without key value = case value of
        J.Object fields -> J.Object (KeyMap.delete key fields)
        other -> other
  ok "a contract without the section is schema 4 and has no application member"
    (member "schema" contractJson == J.toJSON (4 :: Int) && member "application" contractJson == J.Null)
  ok "a contract with the section is schema 5 and otherwise unchanged"
    (member "schema" withSection == J.toJSON (5 :: Int) && member "application" withSection == applicationValue sectioned
      && without "schema" (without "application" withSection) == without "schema" contractJson)

  -- Image5 producer reads under an image5 edit.
  producerReturn <- labelled Image5Producer "caller-return"
  destinationLabel <- labelled Image5Producer "destination"
  stagingLabel <- labelled Image5Producer "staging"
  let producerPiece = (piece Image5Producer [Image5ProducerArea] [] [] [])
        { pieceCatalog = Catalog [boundary producerReturn ReturnBoundary Core0 0x73b4b0 True] [] (EnumeratedWrites [] Proved)
            [ CatalogRead destinationLabel producerReturn 0xbf1a00 0x4f1e60 Nothing
            , CatalogRead stagingLabel producerReturn 0x43e40000 0x2e0000 Nothing ] }
      pumpPiece = piece ApplicationPump [ApplicationPumpArea] [at Image5 0xbfbd20 0xbfbd40] [flip' targets (at Image5 0xbfbd20 0xbfbd22)] []
      producerReads composition = [ (labelText (readLabel row), readAddress row, readLength row, readDiffers row /= Nothing)
                                  | owner <- compositionPieces composition, pieceKind owner == Image5Producer, row <- catalogReads (pieceCatalog owner) ]
  ok "an image5 edit splits the producer's destination read around the editing piece's carrier and marks staging as differing"
    (fmap producerReads (composeWith [] [producerPiece, pumpPiece]) == Right
      [ ("image5-producer/destination-bf1a00", 0xbf1a00, 0xbfbd20 - 0xbf1a00, False)
      , ("image5-producer/destination-bfbd40", 0xbfbd40, 0x10e3860 - 0xbfbd40, False)
      , ("image5-producer/staging", 0x43e40000, 0x2e0000, True) ])
  ok "without an image5 edit the producer's reads are unchanged"
    (fmap producerReads (composeWith [] [producerPiece, first {pieceFootprint = []}]) == Right
      [ ("image5-producer/destination", 0xbf1a00, 0x4f1e60, False), ("image5-producer/staging", 0x43e40000, 0x2e0000, False) ])

  passed <- readIORef counter
  putStrLn ("startup-artifact-tests: " ++ show passed ++ " checks passed")
