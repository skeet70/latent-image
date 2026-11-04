{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J5 composition: the idle-task and root-task
-- pieces with the J4 pieces, the windowed root closure, the trampolines, the
-- J5 joins, host-check controls and composed staging.
module Main where

import qualified A32 as A
import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (nub)
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.Vector as V
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (poolAllocationValue)
import qualified Replacement.StartupArtifact.Footprint.IdleTask as IdleFootprint
import qualified Replacement.StartupArtifact.Footprint.RootTask as RootFootprint
import Replacement.StartupArtifact.Joins (JoinKind (..), joinedTasks, registeredJoins, submissionStackWith)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.TaskSubmission.Admission as WA
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

-- | A target with one word replaced.
patchWord :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patchWord base image address value =
  let offset = fromIntegral (address - base) in B.take offset image <> U.putWord32 value <> B.drop (offset + 4) image

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    [encoder, rebuild, decoder, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, dat, scratch] ->
      pure (encoder, rebuild, decoder, [a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22], dat, scratch)
    _ -> H.failH "usage: startup-artifact-j5-canonical-tests ENCODER REBUILD DECODER (J4 IMAGE SYMBOLS){9} IDLE_BIN IDLE_SYMBOLS ROOT_BIN ROOT_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      image4 = targetOf targets Image4
      loaderA = targetOf targets LoaderA
      loaderB = targetOf targets LoaderB
      linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
      single kind bin symbols = linked bin symbols >>= derived . derivePieceOf source . SingleImage kind
  first <- single FirstStage firstBin firstSymbols
  pair <- single Record2Link pairBin pairSymbols
  publisher <- single PrimaryPublisher publisherBin publisherSymbols
  startupImages <- LoaderStartupImages <$> linked orchestrationABin orchestrationASymbols <*> linked orchestrationBBin orchestrationBSymbols
    <*> linked dispatcherBin dispatcherSymbols <*> linked installerBin installerSymbols
  startup <- derived (derivePieceOf source startupImages)
  submission <- single TaskSubmission submissionBin submissionSymbols
  storage <- single TaskStorage storageBin storageSymbols
  idle <- single IdleTask idleBin idleSymbols
  root <- single RootTask rootBin rootSymbols

  -- Pieces.
  ok "idle-task edits only image 4 inside [0x69cbf0,0x69cc00)"
    (editTargets idle == Set.fromList [Image4] && pieceCarriers idle == [at Image4 0x69cbf0 0x69cc00]
      && editsInsideCarriers idle && footprintOutsideCarriers idle && not (null (pieceEdits idle)))
  ok "root-task edits only image 4 inside [0x69d160,0x69d2b4)"
    (editTargets root == Set.fromList [Image4] && pieceCarriers root == [at Image4 0x69d160 0x69d2b4]
      && editsInsideCarriers root && footprintOutsideCarriers root && not (null (pieceEdits root)))
  let boundariesOf piece = catalogBoundaries (pieceCatalog piece)
      labelsOf piece = map (labelText . boundaryLabel) (boundariesOf piece)
      boundaryNamed piece name = [boundary | boundary <- boundariesOf piece, labelText (boundaryLabel boundary) == name]
  putStrLn ("root-task boundaries: " ++ show [(labelText (boundaryLabel b), boundaryComposedPc b, boundaryRetainedPc b, recordLimitCount (boundaryRecordLimit b)) | b <- boundariesOf root])
  putStrLn ("root-task footprint: " ++ show [range | Footprint range _ _ <- pieceFootprint root])
  putStrLn ("idle-task boundaries: " ++ show (labelsOf idle))
  ok "root labels are unique and name the resource loop, the reading call sites and both presence boundaries"
    (length (nub (labelsOf root)) == length (labelsOf root)
      && all (`elem` labelsOf root) (map ("root-task/" <>)
        ["create-resource-call", "create-workers-call", "init-6a5634-call", "register-periodic-call", "mark-1-call", "mark-2-call", "cleanup-call", "later-returned", "periodic-callback"]))
  ok "the resource loop site records 97 rows"
    (map (recordLimitCount . boundaryRecordLimit) (boundaryNamed root "root-task/create-resource-call") == [97])
  ok "root call sites compare as a core-0 sequence; later-returned and the periodic callback by presence"
    (and [ coreList (boundaryCores boundary) == [Core0]
           && (boundaryCompare boundary == ComparePresence) == (labelText (boundaryLabel boundary) `elem` ["root-task/later-returned", "root-task/periodic-callback"])
         | boundary <- boundariesOf root ])
  ok "every root read stops at a root boundary"
    (and [ any boundaryStop (boundaryNamed root (labelText (readBoundary row))) | row <- catalogReads (pieceCatalog root) ])
  ok "idle first entry is a sequence return boundary on both cores with two rows; wait and resume by presence"
    (map (\b -> (boundaryKind b, coreList (boundaryCores b), boundaryCompare b, recordLimitCount (boundaryRecordLimit b))) (boundaryNamed idle "idle-task/first-entry")
        == [(ReturnBoundary, [Core0, Core1], CompareSequence, 2)]
      && all (\name -> map boundaryCompare (boundaryNamed idle name) == [ComparePresence]) ["idle-task/wait", "idle-task/resume"])

  -- The windowed root closure (review R1).
  let extents = RootFootprint.closureExtents image4
      overlaps (low, high) (start, end) = low < end && start < high
  putStrLn ("root closure extents: " ++ show (length extents))
  ok "the root closure stays out of the idle, dispatcher and installer carriers"
    (not (any (\extent -> any (overlaps extent) [(0x69cbf0, 0x69cc00), (0x69cdf8, 0x69cf84), (0x725a04, 0x725b54)]) extents))
  ok "the 0x69ca44 extent ends at the idle carrier"
    (any (\(low, high) -> low <= 0x69ca44 && high == 0x69cbf0) extents)
  ok "the root closure reaches the wrapper, storage and critical-pair carriers"
    (all (\carrier -> any (overlaps carrier) extents) [(0x725dd8, 0x72612c), (0x725cc4, 0x725da8), (0x7235d8, 0x723790)])

  -- Trampolines (review R3).
  let wordIn :: Word32 -> B.ByteString -> Word32 -> Word32
      wordIn base image address = U.word32At image (fromIntegral (address - base))
      decodedA = [A.decode (0x43728 + offset) (wordIn 0x40000 loaderA (0x43728 + offset)) | offset <- [0, 4 .. 0x30]]
      decodedB = [A.decode (0xd3728 + offset) (wordIn 0xd0000 loaderB (0xd3728 + offset)) | offset <- [0, 4 .. 0x30]]
  ok "loader-B [0xd3728,0xd375c) decodes instruction for instruction as loader A's trampoline, ending in ldr pc,[r2,#8]"
    (decodedB == decodedA && last decodedB == A.Load (A.Cond 14) A.Word False A.pc (A.Reg 2) (A.OffsetImmediate 8) A.OffsetOnly)
  ok "loader-B trampoline literals are the current-task cell 0x400e4720 and task-return service 0xd53b0"
    (map (wordIn 0xd0000 loaderB) [0xd37fc, 0xd3808] == [0x400e4720, 0xd53b0]
      && map (wordIn 0x40000 loaderA) [0x437fc, 0x43808] == [0x40054760, 0x453f0])
  ok "the trampoline check holds on the canonical loaders"
    (IdleFootprint.trampolinesMatch loaderA loaderB)
  ok "the trampoline check refuses a loader B carrying loader A's task-return literal"
    (not (IdleFootprint.trampolinesMatch loaderA (patchWord 0xd0000 loaderB 0xd3808 0x453f0)))
  ok "the trampoline check refuses a loader B with a changed instruction word"
    (not (IdleFootprint.trampolinesMatch loaderA (patchWord 0xd0000 loaderB 0xd3748 0xe3a02001)))

  -- Error-callback demand.
  ok "the error-callback demand of the stock 4-byte idle frame is the stock 12-byte demand"
    (WA.errorCallbackDemand (WA.StackOffset (-4)) == WA.stockCalleeDemand (WA.FixedService WA.ErrorCallback))

  -- The pool-allocation relation with the J5 additions.
  let pieces = [first, pair, publisher, startup, submission, storage, idle, root]
      items name value = case value of
        J.Object fields -> case KeyMap.lookup (Key.fromText name) fields of
          Just (J.Array rows) -> V.toList rows
          _ -> []
        _ -> []
      text name value = case value of
        J.Object fields -> case KeyMap.lookup (Key.fromText name) fields of
          Just (J.String content) -> Just content
          _ -> Nothing
        _ -> Nothing
  relation <- maybe (H.failH "J5 pieces emit no pool-allocation relation") pure (poolAllocationValue pieces)
  ok "the relation adds root's four base-word reads and idle's first-entry stack top"
    (length (items "words" relation) == 10
      && all (\name -> Just name `elem` map (text "read") (items "words" relation))
           ["root-task/submission-storage-records", "root-task/submission-wrapper-records", "root-task/a-creation-all", "root-task/b-creation-all"]
      && [content | J.String content <- items "stack_tops" relation]
           == ["task-submission/a104-first-entry", "task-submission/b93-first-entry", "idle-task/first-entry"])

  -- Composition.
  let newJoins = [DispatcherRootEntry, DispatcherIdleEntry, WrapperIdleErrorCallback, RootTaskServices]
  j5 <- either (H.failH . show) pure (compose registeredJoins source pieces)
  let applied name = [spans | AppliedJoin join spans <- compositionJoins j5, joinName (joinKind join) == name]
  forM_ newJoins $ \kind -> putStrLn ("J5 " ++ T.unpack (joinName kind) ++ ": " ++ show (applied (joinName kind)))
  ok "J5 composes pending the J4 observation joins and the three observation-qualified J5 joins"
    (compositionQualification j5 == Pending
      [ "first-stage-secondary-rendezvous", "prefix-orchestration-b", "first-stage-completion-zero"
      , "loader-startup-publisher", "dispatcher-critical-pair", "task-composition-owned-calls", "dispatcher-task-composition"
      , "dispatcher-root-entry", "dispatcher-idle-entry", "root-task-services" ])
  ok "every J5 join applies once" (all (\kind -> length (applied (joinName kind)) == 1) newJoins)
  forM_ newJoins $ \kind ->
    let without = [join | join <- registeredJoins, joinKind join /= kind]
        bounds = concat [joinBounds join | join <- registeredJoins, joinKind join == kind]
    in ok ("without " ++ T.unpack (joinName kind) ++ ", J5 refuses on a span inside its bounds")
      (case compose without source pieces of
        Left (FootprintOverlap _ _ range) -> coveredBy range bounds
        _ -> False)
  ok "J4 still composes without the J5 pieces"
    (either (const False) (const True) (compose registeredJoins source [first, pair, publisher, startup, submission, storage]))

  -- Host checks, each in isolation.
  let j5Targets = compositionTargets j5
      j5Pieces = compositionPieces j5
      evidenceWith canonical' pieces' = JoinEvidence canonical' j5Targets (U.parsedParts (canonicalParsed source)) pieces'
      evidence = evidenceWith targets
      named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      holdsIn name pieces' = not (null (named name)) && all ($ evidence pieces') (named name)
      withStarts change = [piece {pieceFacts = case pieceFacts piece of
                              LoaderStartupFacts a b calls starts -> LoaderStartupFacts a b calls (map change starts)
                              other -> other} | piece <- j5Pieces]
      forTask task change start@(TaskStart (TaskNumber number) _ _) = if number == task then change start else start
      moveEntry (TaskStart number (Pc entry) bytes) = TaskStart number (Pc (entry + 4)) bytes
      shrink bytes (TaskStart number entry _) = TaskStart number entry (StackBytes bytes)
      j5Checks = [ "root-entry-dispatched", "root-stack-request-covers-frame", "idle-entry-dispatched", "idle-stack-request-covers-frame"
                 , "error-callback-idle-demand", "root-closure-enters-carriers-at-entries" ]
  ok "every J5 host check holds on the J5 composition" (all (`holdsIn` j5Pieces) j5Checks)
  ok "task 104 dispatched at 0x69d164 fails root-entry-dispatched"
    (not (holdsIn "root-entry-dispatched" (withStarts (forTask 104 moveEntry))))
  ok "task 104 with a 0x20-byte stack fails root-stack-request-covers-frame"
    (not (holdsIn "root-stack-request-covers-frame" (withStarts (forTask 104 (shrink 0x20)))))
  ok "task 103 dispatched at 0x69cbf4 fails idle-entry-dispatched"
    (not (holdsIn "idle-entry-dispatched" (withStarts (forTask 103 moveEntry))))
  ok "task 102 with a 0x20-byte stack fails idle-stack-request-covers-frame"
    (not (holdsIn "idle-stack-request-covers-frame" (withStarts (forTask 102 (shrink 0x20)))))
  joined <- maybe (H.failH "J5 task pieces do not join") pure (joinedTasks (evidence j5Pieces))
  let calls = [call | LoaderStartupFacts _ _ submitted _ <- map pieceFacts j5Pieces, call <- submitted]
      demandWith low target
        | target == WA.FixedService WA.ErrorCallback = WA.errorCallbackDemand (WA.StackOffset low)
        | otherwise = WA.stockCalleeDemand target
  ok "an error callback with a 0x800-byte frame exceeds every dispatcher submission's boot-stack window"
    (not (null calls) && all (either (const True) (const False) . submissionStackWith (demandWith (-0x800)) joined) calls)
  let entering = [ site | (site, 0x725dd8) <- RootFootprint.closureCalls image4
                 , not (any (\(low, high) -> site >= low && site < high) [(0x725dd8, 0x72612c), (0x7235d8, 0x723790), (0x725cc4, 0x725da8)]) ]
  site <- case entering of
    found : _ -> pure found
    [] -> H.failH "root closure has no call into the wrapper entry"
  let retargeted = either (const targets) id
        (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] targets) (edit (at Image4 site (site + 4)) (U.putWord32 (branchLink site 0x725ddc))))
  ok "a closure call retargeted into the wrapper body fails root-closure-enters-carriers-at-entries"
    (not (all ($ evidenceWith retargeted j5Pieces) (named "root-closure-enters-carriers-at-entries")))

  -- Staging.
  (_, observed) <- stageComposition tools scratch source j5
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4] $ \target ->
    ok ("J5 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf observed target)) == editSpansOf target pieces)

  passed <- readIORef counter
  putStrLn ("startup-artifact-j5-canonical-tests: " ++ show passed ++ " checks passed")
