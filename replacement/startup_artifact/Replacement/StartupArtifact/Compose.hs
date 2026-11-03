{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupArtifact.Compose
  ( Canonical, canonical, canonicalDat, canonicalParsed, canonicalTargets
  , StagedOutput (..), PieceDeclaration (..), derivePiece
  , JoinName (..), Join (..), JoinBasis (..), HostCheck (..), JoinEvidence (..)
  , Composition, compositionPieces, compositionJoins, compositionTargets, compose, attachGuard
  , Qualification (..), compositionQualification
  , preTableDat, verifyComposed
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Text (Text)
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types

-- | The canonical DAT, its parse and its six decoded targets.
data Canonical = Canonical B.ByteString U.Parsed TargetBytes

canonicalDat :: Canonical -> B.ByteString
canonicalDat (Canonical dat _ _) = dat

canonicalParsed :: Canonical -> U.Parsed
canonicalParsed (Canonical _ parsed _) = parsed

canonicalTargets :: Canonical -> TargetBytes
canonicalTargets (Canonical _ _ targets) = targets

-- | Refuses a DAT that does not rebuild byte for byte from its parts.
canonical :: B.ByteString -> B.ByteString -> B.ByteString -> Either Refusal Canonical
canonical dat image4 image5 = do
  parsed <- either (const (Left CanonicalRefused)) Right (U.parseContainer dat)
  let parts = U.parsedParts parsed
  unless (U.rebuildContainer parsed parts == Right dat) (Left CanonicalRefused)
  pre <- extractPreTable parts
  extent4 <- imageExtent parts Image4
  extent5 <- imageExtent parts Image5
  unless (B.length image4 == extent4 && B.length image5 == extent5) (Left CanonicalRefused)
  targets <- maybe (Left CanonicalRefused) Right (targetBytes (M.insert Image4 image4 (M.insert Image5 image5 pre)))
  pure (Canonical dat parsed targets)

-- | An adapter's staged DAT and the decoded images it changed; 'Nothing'
-- requires that image's record to stay canonical.
data StagedOutput = StagedOutput
  { stagedDat :: B.ByteString
  , stagedImage4 :: Maybe B.ByteString
  , stagedImage5 :: Maybe B.ByteString
  }

data PieceDeclaration = PieceDeclaration
  { declaredKind :: PieceKind
  , declaredAreas :: Set.Set Area
  , declaredCarriers :: [Span]
  , declaredFootprint :: [Footprint]
  , declaredPremises :: [Premise]
  , declaredCatalog :: Catalog
  , declaredFacts :: PieceFacts
  }

derivePiece :: Canonical -> PieceDeclaration -> StagedOutput -> Either Refusal Piece
derivePiece source declaration staged = do
  let kind = declaredKind declaration
      carriers = normalizeSpans (declaredCarriers declaration)
      outside = Left (PieceChangesOutsideTargets kind)
      canonicalParts = U.parsedParts (canonicalParsed source)
  parsed <- either (const outside) Right (U.parseContainer (stagedDat staged))
  let parts = U.parsedParts parsed
  restored <- maybe outside Right (restoreTargets canonicalParts parts)
  unless (U.rebuildContainer (canonicalParsed source) restored == Right (canonicalDat source)) outside
  pre <- extractPreTable parts
  let image target selected record = case selected of
        Just bytes -> do
          unless (B.length bytes == B.length (targetOf (canonicalTargets source) target)) (Left (TargetGeometryRefused target))
          pure bytes
        Nothing -> do
          unless (all (\part -> part canonicalParts == part parts) record) (Left (PieceChangesUndecodedImage kind target))
          pure (targetOf (canonicalTargets source) target)
  four <- image Image4 (stagedImage4 staged) [U.serializeRecord . U.partRecord4, U.partGap45]
  five <- image Image5 (stagedImage5 staged) [U.serializeRecord . U.partRecord5, U.partPostHead]
  let stagedTargets = M.insert Image4 four (M.insert Image5 five pre)
  edits <- fmap concat $ forM [minBound .. maxBound] $ \target -> do
    let before = targetOf (canonicalTargets source) target
        after = M.findWithDefault B.empty target stagedTargets
    forM (diffSpans target before after) $ \range -> do
      unless (coveredBy range carriers) (Left (EditOutsideCarriers kind range))
      let offset = fromIntegral (let Guest start = spanStart range in start - targetBase target)
      maybe (Left (TargetGeometryRefused target)) Right (edit range (B.take (spanLength range) (B.drop offset after)))
  let footprint = [Footprint remaining basis citation
        | Footprint range basis citation <- declaredFootprint declaration, remaining <- subtractSpans range carriers]
  pure (Piece kind (declaredAreas declaration) carriers edits footprint (declaredPremises declaration) (declaredCatalog declaration)
    (declaredFacts declaration))

class JoinName k where
  joinName :: k -> Text

data JoinBasis
  -- | Qualified by its host checks alone; the text is the contract reason.
  = HostProof Text
  -- | Qualified by a composed natural comparison; the contract is pending.
  | ObservationQualified Text

-- | What a join's host checks see at composition.
data JoinEvidence = JoinEvidence
  { evidenceCanonical :: TargetBytes
  , evidenceComposed :: TargetBytes
  -- | Canonical parts with every pre-table edit applied.
  , evidenceParts :: U.Parts
  , evidencePieces :: [Piece]
  }

-- | A named check run at composition; failure refuses the composition.
data HostCheck = HostCheck Text (JoinEvidence -> Bool)

data Join k = Join
  { joinKind :: k
  , joinMembers :: Set.Set PieceKind
  , joinSpans :: [Span]
  , joinBasis :: JoinBasis
  , joinChecks :: [HostCheck]
  }

data Composition k = Composition [Piece] [Join k] TargetBytes

compositionPieces :: Composition k -> [Piece]
compositionPieces (Composition pieces _ _) = pieces

-- | The registered joins whose members are all present.
compositionJoins :: Composition k -> [Join k]
compositionJoins (Composition _ joins _) = joins

compositionTargets :: Composition k -> TargetBytes
compositionTargets (Composition _ _ targets) = targets

data Qualification = HostComposed | Pending [Text] deriving (Eq, Show)

compositionQualification :: JoinName k => Composition k -> Qualification
compositionQualification composition =
  case [joinName (joinKind join) | join <- compositionJoins composition, ObservationQualified _ <- [joinBasis join]] of
    [] -> HostComposed
    names -> Pending names

pairs :: [a] -> [(a, a)]
pairs items = [(left, right) | (index, left) <- zip [0 :: Int ..] items, right <- drop (index + 1) items]

compose :: JoinName k => [Join k] -> Canonical -> [Piece] -> Either Refusal (Composition k)
compose registry source pieces = do
  let kinds = map pieceKind pieces
  forM_ (sort kinds `zip` drop 1 (sort kinds)) $ \(left, right) ->
    when (left == right) (Left (DuplicatePieceKind left))
  forM_ (pairs pieces) $ \(left, right) ->
    forM_ (Set.toList (Set.intersection (pieceAreas left) (pieceAreas right))) $ \area ->
      Left (SharedArea (pieceKind left) (pieceKind right) area)
  forM_ pieces $ \piece -> forM_ (pieceEdits piece) $ \change ->
    unless (coveredBy (editSpan change) (pieceCarriers piece)) (Left (EditOutsideCarriers (pieceKind piece) (editSpan change)))
  forM_ (pairs pieces) $ \(left, right) ->
    forM_ [overlap | a <- pieceCarriers left, b <- pieceCarriers right, Just overlap <- [intersectSpan a b]] $ \overlap ->
      Left (CarrierOverlap (pieceKind left) (pieceKind right) overlap)
  let present = Set.fromList kinds
      applicable = [join | join <- registry, joinMembers join `Set.isSubsetOf` present]
      conflicts = [ (pieceKind owner, pieceKind carrier, overlap)
                  | owner <- pieces, carrier <- pieces, pieceKind owner /= pieceKind carrier
                  , Footprint range _ _ <- pieceFootprint owner, held <- pieceCarriers carrier
                  , Just overlap <- [intersectSpan range held] ]
  forM_ applicable $ \join -> forM_ (joinSpans join) $ \range -> do
    let among = [overlap | (owner, carrier, overlap) <- conflicts, Set.member owner (joinMembers join), Set.member carrier (joinMembers join)]
    unless (coveredBy range among) (Left (JoinSpanNotConflicting (joinName (joinKind join)) range))
  forM_ conflicts $ \(owner, carrier, overlap) -> do
    let joined = concat [joinSpans join | join <- applicable, Set.member owner (joinMembers join), Set.member carrier (joinMembers join)]
    case subtractSpans overlap joined of
      [] -> pure ()
      uncovered : _ -> Left (FootprintOverlap owner carrier uncovered)
  when (null (concatMap pieceEdits pieces)) (Left CompositionUnchanged)
  validateCatalogs pieces
  targets <- applyTargetEdits (concatMap pieceEdits pieces) (canonicalTargets source)
  parts <- applyPreTableEdits (filter ((`elem` preTableTargets) . spanTarget . editSpan) (concatMap pieceEdits pieces))
    (U.parsedParts (canonicalParsed source))
  let evidence = JoinEvidence (canonicalTargets source) targets parts pieces
  forM_ applicable $ \join -> forM_ (joinChecks join) $ \(HostCheck name check) ->
    unless (check evidence) (Left (HostProofFailed (joinName (joinKind join)) name))
  pure (Composition pieces applicable targets)

validateCatalogs :: [Piece] -> Either Refusal ()
validateCatalogs pieces = do
  let labelled = [(pieceKind piece, boundary) | piece <- pieces, boundary <- catalogBoundaries (pieceCatalog piece)]
      readLabels = [(pieceKind piece, readLabel row) | piece <- pieces, row <- catalogReads (pieceCatalog piece)]
      refuse kind problem = Left (CatalogRefused kind problem)
  forM_ (repeated ([(kind, boundaryLabel boundary) | (kind, boundary) <- labelled] ++ readLabels)) $ \(kind, name) ->
    refuse kind (DuplicateLabel name)
  forM_ (repeated [(kind, boundaryComposedPc boundary) | (kind, boundary) <- labelled]) $ \(kind, pc) ->
    refuse kind (DuplicateComposedPc pc)
  forM_ (repeated [(kind, pc) | (kind, boundary) <- labelled, Just pc <- [boundaryRetainedPc boundary]]) $ \(kind, pc) ->
    refuse kind (DuplicateRetainedPc pc)
  forM_ (pairs pieces) $ \(left, right) ->
    forM_ [() | CodeSpan start end <- map physicalCode (catalogOwnedCode (pieceCatalog left))
              , CodeSpan otherStart otherEnd <- map physicalCode (catalogOwnedCode (pieceCatalog right))
              , start < otherEnd && otherStart < end] $ \_ ->
      refuse (pieceKind right) (OwnedCodeOverlap (pieceKind left))
  forM_ [minBound .. maxBound] $ \core -> do
    let composed = [kind | (kind, boundary) <- labelled, core `elem` coreList (boundaryCores boundary)]
        retained = [kind | (kind, boundary) <- labelled, core `elem` coreList (boundaryCores boundary), boundaryRetainedPc boundary /= Nothing]
    forM_ (take 1 (drop maxBoundariesPerCore composed) ++ take 1 (drop maxBoundariesPerCore retained)) $ \kind ->
      refuse kind (TooManyBoundaries core)
  forM_ pieces $ \piece -> do
    let kind = pieceKind piece
        catalog = pieceCatalog piece
        boundaries = catalogBoundaries catalog
        inside (Pc pc) = any (\(CodeSpan start end) -> start <= pc && pc < end) (catalogOwnedCode catalog)
        boundaryCoreSet = nub (concatMap (coreList . boundaryCores) boundaries)
    forM_ boundaries $ \boundary ->
      when (boundaryKind boundary /= ReturnBoundary && not (inside (boundaryComposedPc boundary)))
        (refuse kind (OwnedBoundaryOutsideOwnedCode (boundaryLabel boundary)))
    forM_ (catalogReads catalog) $ \row -> case [boundary | boundary <- boundaries, boundaryLabel boundary == readBoundary row] of
      [boundary] -> do
        unless (boundaryStop boundary) (refuse kind (ReadBoundaryDoesNotStop (readBoundary row)))
        when (boundaryRetainedPc boundary == Nothing && readDiffers row == Nothing) (refuse kind (ReadNeedsDiffers (readLabel row)))
      _ -> refuse kind (ReadBoundaryUnknown (readBoundary row))
    let tcmCores = case catalogWrites catalog of
          UnenumeratedWrites _ -> []
          EnumeratedWrites ranges _ -> [core | WriteRange (TcmTarget core _) _ _ <- ranges]
    forM_ tcmCores $ \core -> unless (core `elem` boundaryCoreSet) (refuse kind (TcmCoreWithoutBoundary core))

maxBoundariesPerCore :: Int
maxBoundariesPerCore = 4096

physicalCode :: CodeSpan -> CodeSpan
physicalCode (CodeSpan start end) =
  CodeSpan (fromIntegral (physicalAddress (fromIntegral start))) (fromIntegral (physicalAddress (fromIntegral end - 1)) + 1)

-- | Marks the named boundary as the stopping guard attach point; it must
-- have a retained PC and exactly one core.
attachGuard :: Label -> [Piece] -> Either Refusal [Piece]
attachGuard attach pieces =
  case [boundary | piece <- pieces, boundary <- catalogBoundaries (pieceCatalog piece), boundaryLabel boundary == attach] of
    [] -> Left (ManifestRefused (GuardAttachUnknown (labelText attach)))
    found : _
      | boundaryRetainedPc found == Nothing || length (coreList (boundaryCores found)) /= 1 ->
          Left (ManifestRefused (GuardAttachUnsuitable (labelText attach)))
      | otherwise ->
          Right [piece {pieceCatalog = (pieceCatalog piece) {catalogBoundaries = map stop (catalogBoundaries (pieceCatalog piece))}} | piece <- pieces]
  where
    stop boundary
      | boundaryLabel boundary == attach = boundary {boundaryStop = True}
      | otherwise = boundary

-- | The first repeated key, with the piece that repeats it.
repeated :: Eq key => [(PieceKind, key)] -> Maybe (PieceKind, key)
repeated = go []
  where
    go _ [] = Nothing
    go seen ((kind, key) : rest)
      | key `elem` seen = Just (kind, key)
      | otherwise = go (key : seen) rest

preTableEditsOf :: Composition k -> [Edit]
preTableEditsOf composition = [change | piece <- compositionPieces composition, change <- pieceEdits piece
  , spanTarget (editSpan change) `elem` preTableTargets]

-- | The canonical DAT with every pre-table edit applied, rebuilt once.
preTableDat :: Canonical -> Composition k -> Either Refusal B.ByteString
preTableDat source composition = do
  parts <- applyPreTableEdits (preTableEditsOf composition) (U.parsedParts (canonicalParsed source))
  either (const (Left ComposedUnparsed)) Right (U.rebuildContainer (canonicalParsed source) parts)

-- | Postconditions over the composed DAT and its decoded images.
verifyComposed :: Canonical -> Composition k -> B.ByteString -> B.ByteString -> B.ByteString -> Either Refusal TargetBytes
verifyComposed source composition dat image4 image5 = do
  parsed <- either (const (Left ComposedUnparsed)) Right (U.parseContainer dat)
  let parts = U.parsedParts parsed
  pre <- either (const (Left ComposedUnparsed)) Right (extractPreTable parts)
  observed <- maybe (Left ComposedUnparsed) Right (targetBytes (M.insert Image4 image4 (M.insert Image5 image5 pre)))
  forM_ [minBound .. maxBound] $ \target -> do
    let before = targetOf (canonicalTargets source) target
        after = targetOf observed target
        declared = normalizeSpans [editSpan change | piece <- compositionPieces composition, change <- pieceEdits piece
          , spanTarget (editSpan change) == target]
    unless (B.length before == B.length after && normalizeSpans (diffSpans target before after) == declared
      && after == targetOf (compositionTargets composition) target) (Left (ComposedDiffMismatch target))
  restored <- maybe (Left ComposedRestoreMismatch) Right (restoreTargets (U.parsedParts (canonicalParsed source)) parts)
  unless (U.rebuildContainer (canonicalParsed source) restored == Right (canonicalDat source)) (Left ComposedRestoreMismatch)
  pure observed
