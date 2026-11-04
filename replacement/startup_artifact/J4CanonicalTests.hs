{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J4 composition: the task-submission and
-- task-storage pieces with the J3 pieces, their joins, host-check controls
-- and composed staging.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.Vector as V
import Data.Scientific (toBoundedInteger)
import Replacement.StartupArtifact.Contract (poolAllocationValue)
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins (JoinKind (..), joinedTasks, registeredJoins, submissionStack)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.StartupComposition.TaskSubmission as TS
import qualified Replacement.TaskSubmission.Admission as WA
import qualified Replacement.TaskSubmission.Artifact as W
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (H.failH ("failed: " ++ name))
  modifyIORef' counter (+ 1)

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

editTargets :: Piece -> Set.Set Target
editTargets piece = Set.fromList [spanTarget (editSpan change) | change <- pieceEdits piece]

editsInsideCarriers :: Piece -> Bool
editsInsideCarriers piece = all (\change -> coveredBy (editSpan change) (pieceCarriers piece)) (pieceEdits piece)

footprintOutsideCarriers :: Piece -> Bool
footprintOutsideCarriers piece =
  null [() | Footprint range _ _ <- pieceFootprint piece, carrier <- pieceCarriers piece, Just _ <- [intersectSpan range carrier]]

editSpansOf :: Target -> [Piece] -> [Span]
editSpansOf target pieces = normalizeSpans [editSpan change | piece <- pieces, change <- pieceEdits piece, spanTarget (editSpan change) == target]

-- | An A32 BL from a site to a target.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `div` 4) .&. 0xffffff)

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    [encoder, rebuild, decoder, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, dat, scratch] ->
      pure (encoder, rebuild, decoder, [a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18], dat, scratch)
    _ -> H.failH "usage: startup-artifact-j4-canonical-tests ENCODER REBUILD DECODER FIRST_BIN FIRST_SYMBOLS PAIR_BIN PAIR_SYMBOLS PUBLISHER_BIN PUBLISHER_SYMBOLS ORCHESTRATION_A_BIN ORCHESTRATION_A_SYMBOLS ORCHESTRATION_B_BIN ORCHESTRATION_B_SYMBOLS DISPATCHER_BIN DISPATCHER_SYMBOLS INSTALLER_BIN INSTALLER_SYMBOLS SUBMISSION_BIN SUBMISSION_SYMBOLS STORAGE_BIN STORAGE_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
  first <- linked firstBin firstSymbols >>= derived . deriveFirstStage source
  pair <- linked pairBin pairSymbols >>= derived . deriveRecord2Link source
  publisher <- linked publisherBin publisherSymbols >>= derived . derivePrimaryPublisher source
  startupImages <- LoaderStartupImages <$> linked orchestrationABin orchestrationASymbols <*> linked orchestrationBBin orchestrationBSymbols
    <*> linked dispatcherBin dispatcherSymbols <*> linked installerBin installerSymbols
  startup <- derived (derivePieceOf source startupImages)
  submissionImage <- linked submissionBin submissionSymbols
  submission <- derived (derivePieceOf source (SingleImage TaskSubmission submissionImage))
  storage <- linked storageBin storageSymbols >>= derived . derivePieceOf source . SingleImage TaskStorage

  -- Pieces.
  ok "task-submission edits only image 4 inside [0x725dd8,0x72612c)"
    (editTargets submission == Set.fromList [Image4] && pieceCarriers submission == [at Image4 0x725dd8 0x72612c]
      && editsInsideCarriers submission && footprintOutsideCarriers submission && not (null (pieceEdits submission)))
  ok "task-storage edits only image 4 inside [0x725cc4,0x725da8)"
    (editTargets storage == Set.fromList [Image4] && pieceCarriers storage == [at Image4 0x725cc4 0x725da8]
      && editsInsideCarriers storage && footprintOutsideCarriers storage && not (null (pieceEdits storage)))
  let labelsOf piece = map (labelText . boundaryLabel) (catalogBoundaries (pieceCatalog piece))
  putStrLn ("task-submission boundaries: " ++ show (labelsOf submission))
  putStrLn ("task-submission footprint: " ++ show [range | Footprint range _ _ <- pieceFootprint submission])
  ok "task-submission catalogs the shared entry, its first-entry boundaries and at least the critical-enter and reservation call sites"
    (all (`elem` labelsOf submission)
      [ "task-submission/submit-entry", "task-submission/a104-first-entry", "task-submission/b93-first-entry"
      , "task-submission/submit-critical-enter-call", "task-submission/submit-reservation-call" ])
  ok "only the first-entry boundaries compare as a sequence"
    (and [ (boundaryCompare boundary == CompareSequence) == (labelText (boundaryLabel boundary) `elem` ["task-submission/a104-first-entry", "task-submission/b93-first-entry"])
         | boundary <- catalogBoundaries (pieceCatalog submission) ++ catalogBoundaries (pieceCatalog storage) ])
  ok "task-storage catalogs its transition entry" (labelsOf storage == ["task-storage/transition-entry"])

  -- The pool-allocation relation as the contract emits it.
  let keysOf value = case value of
        J.Object fields -> Set.fromList (map Key.toText (KeyMap.keys fields))
        _ -> Set.empty
      member name value = case value of
        J.Object fields -> KeyMap.lookup (Key.fromText name) fields
        _ -> Nothing
      items name value = case member name value of
        Just (J.Array rows) -> V.toList rows
        _ -> []
      text name value = case member name value of
        Just (J.String content) -> Just content
        _ -> Nothing
      catalog = pieceCatalog submission
      readLengths = [(labelText (readLabel row), readLength row) | row <- catalogReads catalog]
      returns = [labelText (boundaryLabel boundary) | boundary <- catalogBoundaries catalog, boundaryKind boundary == ReturnBoundary]
      number name value = case member name value of
        Just (J.Number content) -> toBoundedInteger content :: Maybe Word32
        _ -> Nothing
      layout row = case (text "read" row >>= (`lookup` readLengths), number "offset" row, number "stride" row) of
        (Just bytes, Just offset, Just stride) -> stride > 0 && offset + 4 <= stride && bytes `mod` stride == 0
        _ -> False
  relation <- maybe (H.failH "J4 pieces emit no pool-allocation relation") pure (poolAllocationValue [first, pair, publisher, startup, submission, storage])
  ok "the relation carries exactly the consumer's keys"
    (keysOf relation == Set.fromList ["pool", "records", "cursor", "words", "stack_tops", "owners", "citation"]
      && maybe False ((== Set.fromList ["start", "end"]) . keysOf) (member "pool" relation)
      && all ((== Set.fromList ["read", "offset", "stride", "position"]) . keysOf) (items "words" relation)
      && all ((== Set.fromList ["core", "read", "offset", "stride"]) . keysOf) (items "owners" relation))
  ok "the relation names the piece's reads with whole records, a one-word cursor and words inside their strides"
    ((text "records" relation >>= (`lookup` readLengths)) == Just 1248 && (text "cursor" relation >>= (`lookup` readLengths)) == Just 4
      && length (items "words" relation) == 6 && all layout (items "words" relation)
      && length (items "owners" relation) == 2 && all layout (items "owners" relation)
      && all (\row -> text "position" row `elem` [Just "base", Just "top"]) (items "words" relation))
  ok "the relation's stack tops are the piece's first-entry return boundaries"
    ([content | J.String content <- items "stack_tops" relation] == ["task-submission/a104-first-entry", "task-submission/b93-first-entry"]
      && all (`elem` returns) ["task-submission/a104-first-entry", "task-submission/b93-first-entry"])
  ok "no relation is emitted without task storage, or without the task pieces"
    (poolAllocationValue [first, pair, publisher, startup, submission] == Nothing
      && poolAllocationValue [first, pair, publisher, startup] == Nothing)

  -- Composition.
  let pieces = [first, pair, publisher, startup, submission, storage]
      newJoins = [TaskCompositionOwnedCalls, DispatcherTaskComposition, PublisherTaskServices]
  j4 <- either (H.failH . show) pure (compose registeredJoins source pieces)
  let applied name = [spans | AppliedJoin join spans <- compositionJoins j4, joinName (joinKind join) == name]
  forM_ newJoins $ \kind -> putStrLn ("J4 " ++ T.unpack (joinName kind) ++ ": " ++ show (applied (joinName kind)))
  ok "J4 composes pending the J3 observation joins and the two observation-qualified task joins"
    (compositionQualification j4 == Pending
      [ "first-stage-secondary-rendezvous", "prefix-orchestration-b", "first-stage-completion-zero"
      , "loader-startup-publisher", "dispatcher-critical-pair", "task-composition-owned-calls", "dispatcher-task-composition" ])
  ok "every J4 join applies once" (all (\kind -> length (applied (joinName kind)) == 1) newJoins)
  forM_ newJoins $ \kind ->
    let without = [join | join <- registeredJoins, joinKind join /= kind]
        bounds = concat [joinBounds join | join <- registeredJoins, joinKind join == kind]
    in ok ("without " ++ T.unpack (joinName kind) ++ ", J4 refuses on a span inside its bounds")
      (case compose without source pieces of
        Left (FootprintOverlap _ _ range) -> coveredBy range bounds
        _ -> False)
  ok "without the task pieces' joins, task-submission and task-storage refuse"
    (case compose ([] :: [Join JoinKind]) source [submission, storage] of
      Left (FootprintOverlap _ _ _) -> True
      _ -> False)

  -- Host checks, each in isolation.
  let j4Targets = compositionTargets j4
      j4Pieces = compositionPieces j4
      evidence pieces' = JoinEvidence targets j4Targets (U.parsedParts (canonicalParsed source)) pieces'
      named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      holdsIn name pieces' = not (null (named name)) && all ($ evidence pieces') (named name)
      withFacts change = [piece {pieceFacts = change (pieceFacts piece)} | piece <- j4Pieces]
      j4Checks = ["owned-calls-join", "table-b-service-slots-stored", "submission-joined-stack-in-boot-window", "loader-headers-unchanged", "task-service-slots-published-once"]
      deeper extra = [SubmitCall core (StackDepth (depth + extra)) | LoaderStartupFacts _ _ calls <- map pieceFacts j4Pieces, SubmitCall core (StackDepth depth) <- calls]
  ok "every J4 host check holds on the J4 composition" (all (`holdsIn` j4Pieces) j4Checks)
  joined <- maybe (H.failH "J4 task pieces do not join") pure (joinedTasks (evidence j4Pieces))
  ok "every dispatcher submission 0x600 bytes deeper still fits its boot-stack window"
    (not (null (deeper 0x600)) && all (either (const False) (const True) . submissionStack joined) (deeper 0x600))
  ok "every dispatcher submission 0x700 bytes deeper exceeds its window through the owned callees"
    (not (null (deeper 0x700)) && all (\call -> case submissionStack joined call of
      Left (TS.CallRejected WA.StackBackingExceeded) -> True
      _ -> False) (deeper 0x700))
  ok "a dispatcher submission at a misaligned depth fails the joined-stack check"
    (not (holdsIn "submission-joined-stack-in-boot-window" (withFacts (\facts -> case facts of
      LoaderStartupFacts a b calls -> LoaderStartupFacts a b [SubmitCall core (StackDepth (depth + 4)) | SubmitCall core (StackDepth depth) <- calls]
      other -> other))))
  let ImageBytes submissionBytes = case submissionImage of LinkedImage image _ -> image
      SymbolBytes submissionSymbolBytes = case submissionImage of LinkedImage _ symbols -> symbols
  wrapperArtifact <- either (H.failH . show) pure (W.deriveArtifact submissionBytes submissionSymbolBytes)
  reservationCall <- case [address | call <- WA.callBoundaries (W.frameProof wrapperArtifact)
                                   , WA.callTarget call == WA.FixedService WA.Reservation, let WA.CodeAddress address = WA.callInstruction call] of
    site : _ -> pure site
    [] -> H.failH "owned wrapper lacks a reservation call"
  let offset = fromIntegral (reservationCall - 0x725dd8)
      redirected = B.take offset submissionBytes <> U.putWord32 (branchLink reservationCall 0x7221c8) <> B.drop (offset + 4) submissionBytes
  ok "a wrapper whose reservation call is redirected to fill fails the owned-calls join"
    (not (holdsIn "owned-calls-join" (withFacts (\facts -> case facts of
      TaskSubmissionFacts (LinkedBytes _ symbols) pool -> TaskSubmissionFacts (LinkedBytes redirected symbols) pool
      other -> other))))
  let headerMoved = either (const targets) id
        (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] targets) (edit (at LoaderB 0xd0010 0xd0014) (U.putWord32 0x400e0000)))
  ok "a loader-B root naming no initializer store fails the table-B slot check"
    (not (all ($ JoinEvidence headerMoved j4Targets (U.parsedParts (canonicalParsed source)) j4Pieces) (named "table-b-service-slots-stored")))
  ok "a publisher without the root-0 auxiliary slot fails the service-slot check"
    (not (holdsIn "task-service-slots-published-once" (withFacts (\facts -> case facts of
      PublisherFacts frame rows -> PublisherFacts frame
        [row | row <- rows, let Guest guest = publicationGuest row, physicalAddress (fromIntegral guest) /= 0x4db60]
      other -> other))))

  -- Staging.
  (_, observed) <- stageComposition tools scratch source j4
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4] $ \target ->
    ok ("J4 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf observed target)) == editSpansOf target pieces)

  passed <- readIORef counter
  putStrLn ("startup-artifact-j4-canonical-tests: " ++ show passed ++ " checks passed")
