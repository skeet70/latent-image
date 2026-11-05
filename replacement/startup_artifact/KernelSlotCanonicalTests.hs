{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input composition of the kernel slots with J6, J2 and the general heap.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits (complement, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (find, nub)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Application (PlainImage (..), plainImage)
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins (JoinKind (..), depthsIn, registeredJoins, suppliedOf)
import qualified Replacement.StartupArtifact.KernelSlots as KS
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.StartupArtifact.Footprint.GeneralHeap as Heap
import qualified Replacement.StartupArtifact.StackBounds as SB
import qualified StockStack as SS
import qualified Replacement.RtosOwnFirst.Artifact as OF
import qualified Replacement.RtosOwnFirst.Pins as OP
import qualified Replacement.RtosOwnFirst.Regions as OR
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (H.failH ("failed: " ++ name))
  modifyIORef' counter (+ 1)

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

-- | An A32 BL from a site to a target.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `div` 4) .&. 0xffffff)

-- | @movw r4,#0xbd20; movt r4,#0x96; ldr r2,[r4]; ldr r3,[r2,#0x168]; blx r3; mov r0,r1@: a root-slot call to
-- +0x168 whose caller reads r1.
rootCaller :: [Word32]
rootCaller = [0xe30b4d20, 0xe3404096, 0xe5942000, 0xe5923168, 0xe12fff33, 0xe1a00001]

-- | The targets with the words written from a guest address.
patched :: TargetBytes -> Target -> Word32 -> [Word32] -> TargetBytes
patched targets target address words' =
  case edit (at target address (address + 4 * fromIntegral (length words'))) (B.concat (map U.putWord32 words')) of
    Just change -> either (const targets) id (applyTargetEdits [change] targets)
    Nothing -> targets

editSpansOf :: Target -> [Piece] -> [Span]
editSpansOf target pieces = normalizeSpans [editSpan change | piece <- pieces, change <- pieceEdits piece, spanTarget (editSpan change) == target]

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 35 ->
      pure (encoder, rebuild, decoder, take 33 rest, rest !! 33, rest !! 34)
    _ -> H.failH "usage: startup-artifact-kernel-slot-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){13} SLOT_A_BIN SLOT_A_SYMBOLS SLOT_B_BIN SLOT_B_SYMBOLS REGIONS HEAP_BIN HEAP_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols, slotABin, slotASymbols, slotBBin, slotBSymbols, regionsPath, heapBin, heapSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      readLinked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
      single kind bin symbols = readLinked bin symbols >>= derived . derivePieceOf source . SingleImage kind
  first <- single FirstStage firstBin firstSymbols
  pair <- single Record2Link pairBin pairSymbols
  publisher <- single PrimaryPublisher publisherBin publisherSymbols
  startupImages <- LoaderStartupImages <$> readLinked orchestrationABin orchestrationASymbols <*> readLinked orchestrationBBin orchestrationBSymbols
    <*> readLinked dispatcherBin dispatcherSymbols <*> readLinked installerBin installerSymbols
  startup <- derived (derivePieceOf source startupImages)
  orchestrationB <- single LoaderOrchestrationB orchestrationBBin orchestrationBSymbols
  submission <- single TaskSubmission submissionBin submissionSymbols
  storage <- single TaskStorage storageBin storageSymbols
  idle <- single IdleTask idleBin idleSymbols
  root <- single RootTask rootBin rootSymbols
  image5 <- single Image5Producer image5Bin image5Symbols
  plain <- single PlainProducer plainBin plainSymbols
  slotA <- readLinked slotABin slotASymbols
  slotB <- readLinked slotBBin slotBSymbols
  regions <- H.readRegular 0x10000 regionsPath
  slots <- derived (derivePieceOf source (KernelSlotImages slotA slotB (RegionsBytes regions)))
  heapBytes <- H.readRegular (imageLimit GeneralHeap) heapBin
  heapSymbolBytes <- H.readRegular 0x10000 heapSymbols
  heap <- derived (derivePieceOf source (SingleImage GeneralHeap (LinkedImage (ImageBytes heapBytes) (SymbolBytes heapSymbolBytes))))
  let j6Pieces = [first, pair, publisher, startup, submission, storage, idle, root, image5, plain]
      j2Pieces = [first, pair, publisher, orchestrationB]
      everyOther = j6Pieces ++ [orchestrationB]
      (slotRows, slotBodies) = case pieceFacts slots of
        KernelSlotFacts rows bodies -> (rows, bodies)
        _ -> ([], [])

  -- Carriers.
  let pinned = normalizeSpans
        [ at (loaderOf domain) low high | domain <- OR.domains, let pins = OP.stockPins domain
        , (_, OR.Extent (OR.GuestAddress low) (OR.GuestAddress high)) <- OP.pinRows pins ++ [OP.pinTail pins] ]
      loaderOf domain = if domain == OR.DomainA then LoaderA else LoaderB
  putStrLn ("kernel-slot-bodies carriers: " ++ show (length (pieceCarriers slots)) ++ ", slots " ++ show (length slotRows))
  ok "the carriers are the pinned stock extents of the 34 slot rows"
    (length slotRows == 34 && normalizeSpans (pieceCarriers slots) == pinned
      && normalizeSpans (map kernelSlotExtent slotRows) == pinned)
  ok "every edit lies in a carrier and the +0x1618 bodies lie in their host extents"
    (all (\change -> coveredBy (editSpan change) (pieceCarriers slots)) (pieceEdits slots)
      && all (\(KernelSlotBody _ range _) -> coveredBy range (pieceCarriers slots)) slotBodies && length slotBodies == 2)
  let claimed =
        [ at LoaderA 0x40430 0x409ac, at LoaderA 0x409ac 0x40b00, at LoaderB 0xd09ac 0xd0b00, at LoaderB 0xd0430 0xd0434 ]
        ++ [range | range <- pieceCarriers pair, spanTarget range == LoaderB]
      overlapping = [ (other, overlap) | (other, held) <- [("claimed", range) | range <- claimed] ++ [(show (pieceKind piece), range) | piece <- everyOther, range <- pieceCarriers piece]
                    , carrier <- pieceCarriers slots, Just overlap <- [intersectSpan carrier held] ]
  forM_ overlapping $ \overlap -> putStrLn ("carrier overlap: " ++ show overlap)
  ok "no carrier meets the publisher, either orchestration, the record-2 loader-B prefix, 0xd0430, record 1 or another piece's carrier"
    (null overlapping && all ((`elem` [LoaderA, LoaderB]) . spanTarget) (pieceCarriers slots))

  -- Composition.
  j7 <- either (H.failH . show) pure (compose registeredJoins source (j6Pieces ++ [slots]))
  j2 <- either (H.failH . show) pure (compose registeredJoins source (j2Pieces ++ [slots]))
  let heapOverlaps = normalizeSpans [overlap | Footprint held _ _ <- pieceFootprint heap, carrier <- pieceCarriers slots, Just overlap <- [intersectSpan carrier held]]
  putStrLn ("general-heap kernel-slot overlaps: " ++ show heapOverlaps)
  heapPair <- either (H.failH . show) pure (compose registeredJoins source [heap, slots])
  heapFull <- either (H.failH . show) pure (compose registeredJoins source (j6Pieces ++ [heap, slots]))
  let appliedIn composition = [joinKind join | AppliedJoin join _ <- compositionJoins composition]
      kernelJoins = [KernelSlotPrimaryPublisher .. KernelSlotGeneralHeap]
      j7Joins = filter (`elem` appliedIn j7) kernelJoins
      j2Joins = filter (`elem` appliedIn j2) kernelJoins
      heapJoins = filter (`elem` appliedIn heapFull) kernelJoins
  putStrLn ("kernel-slot joins in the J6 composition: " ++ show j7Joins)
  putStrLn ("kernel-slot joins in the J2 composition: " ++ show j2Joins)
  ok "the J6 pieces compose with the kernel slots under their member joins"
    (j7Joins == filter (`notElem` [KernelSlotLoaderOrchestrationB, KernelSlotGeneralHeap]) kernelJoins)
  ok "the J2 pieces compose with kernel-slot-bodies under the publisher, record-2 and loader-B orchestration joins"
    (j2Joins == [KernelSlotPrimaryPublisher, KernelSlotLoaderOrchestrationB, KernelSlotRecord2Link])
  ok "the heap composes with the kernel slots under exactly their footprint conflicts"
    ([normalizeSpans spans | AppliedJoin join spans <- compositionJoins heapPair, joinKind join == KernelSlotGeneralHeap] == [heapOverlaps])
  forM_ (compositionJoins heapPair) $ \(AppliedJoin join spans) ->
    ok "the heap-slot join bounds equal its conflict spans" (normalizeSpans (joinBounds join) == normalizeSpans spans)
  let heapEvidence = JoinEvidence targets (compositionTargets heapFull) (U.parsedParts (canonicalParsed source)) (compositionPieces heapFull)
  supplies <- maybe (H.failH "no heap supplies with owned kernel slots") pure (suppliedOf heapEvidence)
  retained <- maybe (H.failH "no retained heap depths with owned kernel slots") pure (depthsIn heapEvidence supplies Heap.retainedCallees)
  let heapEntries' = case pieceFacts heap of GeneralHeapFacts facts -> heapEntries facts; _ -> []
      heapDemands = [(entry, M.lookup entry supplies) | entry <- heapEntries']
      conditions = Set.unions [SS.reportConditions report | (_, Right report) <- retained]
  putStrLn ("heap entry demands with owned slots: " ++ show heapDemands)
  putStrLn ("heap retained depths with owned slots: " ++ show retained)
  ok "owned-slot heap depths use only the published S1-S5 conditions"
    (all (either (const False) (const True) . snd) retained && conditions `Set.isSubsetOf` Set.fromList SB.heapConditions)
  ok "every heap entry has an owned-slot demand" (all (\(_, depth) -> depth /= Nothing) heapDemands)
  forM_ ([(kind, j6Pieces ++ [slots]) | kind <- j7Joins] ++ [(kind, j2Pieces ++ [slots]) | kind <- j2Joins] ++ [(KernelSlotGeneralHeap, [heap, slots]), (KernelSlotGeneralHeap, j6Pieces ++ [heap, slots])]) $ \(kind, pieces) -> do
    let without = [join | join <- registeredJoins, joinKind join /= kind]
        refusal = either Just (const Nothing) (compose without source pieces)
    putStrLn ("without " ++ show kind ++ ": " ++ show refusal)
    ok ("without " ++ show kind ++ " the composition refuses on a kernel-slot overlap")
      (case refusal of
        Just (FootprintOverlap owner carrier _) -> KernelSlotBodies `elem` [owner, carrier]
        Just (CarrierOverlap owner carrier _) -> KernelSlotBodies `elem` [owner, carrier]
        _ -> False)

  -- Host checks.
  let named name = [predicate | join <- registeredJoins, joinKind join `elem` kernelJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      checkNames = nub [name | join <- registeredJoins, joinKind join `elem` kernelJoins, HostCheck name _ <- joinChecks join]
      evidenceOf canonical' composed' pieces = JoinEvidence canonical' composed' (U.parsedParts (canonicalParsed source)) pieces
      holds name evidence = not (null (named name)) && all ($ evidence) (named name)
      refuses name evidence = not (null (named name)) && not (all ($ evidence) (named name))
      withFacts composition change =
        [if pieceKind piece == KernelSlotBodies then piece {pieceFacts = KernelSlotFacts (map change slotRows) slotBodies} else piece | piece <- compositionPieces composition]
      offsetIs offset slot = kernelSlotOffset slot == RootSlot offset
      -- An entry whose bodies may leave r1 different from stock: loader A's +0x168.
      r1Entry = case find (\slot -> offsetIs 0x168 slot && spanTarget (kernelSlotExtent slot) == LoaderA) slotRows of
        Just KernelSlot {kernelSlotEntry = Pc pc, kernelSlotDiffers = differs} | ClobberR1 `elem` differs -> pc
        _ -> 0
      firstCode piece = take 1 [ start | Footprint (Span target (Guest start) (Guest end)) _ _ <- pieceFootprint piece
                               , target `elem` [LoaderA, LoaderB, Image4], end - start >= 24
                               , not (any (\carrier -> intersectSpan carrier (at target start end) /= Nothing) (pieceCarriers slots)) ]
      targetOfAddress address = case [target | target <- [LoaderA, LoaderB, Image4], targetBase target <= address
                                       , address - targetBase target < fromIntegral (B.length (targetOf targets target))] of
        target : _ -> target
        [] -> Image4
      compositionFor kind
        | kind == LoaderOrchestrationB = j2
        | kind == GeneralHeap = heapFull
        | otherwise = j7
  ok "loader A's +0x168 bodies may leave r1 different from stock" (r1Entry /= 0)
  putStrLn ("kernel-slot host checks: " ++ show checkNames)
  forM_ checkNames $ \name -> ok (T.unpack name ++ " holds on a composition with its members")
    (any (\composition -> holds name (evidenceOf targets (compositionTargets composition) (compositionPieces composition))) [j7, j2, heapFull])
  ok "the general-heap kernel-slot join is applied with the J6 pieces" (KernelSlotGeneralHeap `elem` heapJoins)
  forM_ [PrimaryPublisher, LoaderStartup, LoaderOrchestrationB, Record2Link, TaskSubmission, TaskStorage, RootTask, Image5Producer, PlainProducer, GeneralHeap] $ \kind -> do
    let composition = compositionFor kind
        pieces = compositionPieces composition
        name suffix = pieceKindName kind <> suffix
    case find ((== kind) . pieceKind) pieces of
      Nothing -> H.failH ("missing piece " ++ show kind)
      Just piece -> do
        case firstCode piece of
          site : _ -> do
            let target = targetOfAddress site
                reading = patched targets target site [branchLink site r1Entry, 0xe1a00001]
            ok (T.unpack (name "-slot-callers-leave-clobbers-dead") ++ " refuses a footprint caller that reads r1 after +0x168")
              (refuses (name "-slot-callers-leave-clobbers-dead") (evidenceOf reading (compositionTargets composition) pieces))
            ok (T.unpack (name "-slot-callers-leave-clobbers-dead") ++ " refuses a footprint root-slot caller that reads r1 after +0x168")
              (refuses (name "-slot-callers-leave-clobbers-dead") (evidenceOf (patched targets target site rootCaller) (compositionTargets composition) pieces))
            let offEntry = patched targets target site [branchLink site (r1Entry + 4)]
            ok (T.unpack (name "-calls-enter-slot-entries") ++ " refuses a footprint call into an extent off its entry")
              (refuses (name "-calls-enter-slot-entries") (evidenceOf offEntry (compositionTargets composition) pieces))
          [] -> H.failH ("no footprint code for " ++ show kind)
  let violating name evidence' = ok (T.unpack name ++ " refuses its violating fixture") (refuses name evidence')
      shifted slot = let Pc pc = kernelSlotEntry slot in slot {kernelSlotEntry = Pc (pc + 4)}
  violating "published-slot-words-name-entries" (evidenceOf targets (compositionTargets j7) (withFacts j7 shifted))
  violating "critical-pair-slot-frames-and-r3"
    (evidenceOf targets (compositionTargets j7) (withFacts j7 (\slot -> if offsetIs 0x164 slot then slot {kernelSlotClobbers = ClobberR3 : kernelSlotClobbers slot} else slot)))
  ok "critical-pair-slot-frames-and-r3 refuses a 16-byte +0x1618 frame"
    (refuses "critical-pair-slot-frames-and-r3" (evidenceOf targets (compositionTargets j7) (withFacts j7 (\slot -> if offsetIs 0x1618 slot then slot {kernelSlotFrame = StackDepth 16} else slot))))
  violating "storage-prelude-slot-frames-within-reservation-demand"
    (evidenceOf targets (compositionTargets j7) (withFacts j7 (\slot -> if offsetIs 0x164 slot then slot {kernelSlotFrame = StackDepth 64} else slot)))
  violating "submission-stack-with-owned-slot-frames"
    (evidenceOf targets (compositionTargets j7) (withFacts j7 (\slot -> if offsetIs 0x1618 slot then slot {kernelSlotFrame = StackDepth 0x10000} else slot)))
  let unboundedHeap = [case pieceFacts piece of
        GeneralHeapFacts facts -> piece {pieceFacts = GeneralHeapFacts facts {heapFrames = [OwnedFrame entry (-8) [(0xdeadbeec, 0)] | entry <- heapEntries facts]}}
        _ -> piece | piece <- compositionPieces heapFull]
  violating "heap-slot-retained-depth" (evidenceOf targets (compositionTargets heapFull) unboundedHeap)
  forM_ [(LoaderA, "loader_a-differs-only-in-carriers"), (LoaderB, "loader_b-differs-only-in-carriers")] $ \(target, name) -> do
    let carriers = concatMap pieceCarriers (compositionPieces j7)
        base = targetBase target
        outside = case [address | address <- [base, base + 4 ..], not (coveredBy (at target address (address + 4)) carriers)] of
          address : _ -> address
          [] -> base
        composed = compositionTargets j7
        current = U.word32At (targetOf composed target) (fromIntegral (outside - base))
    violating name (evidenceOf targets (patched composed target outside [complement current]) (compositionPieces j7))

  let fixtured = Set.fromList
        ([pieceKindName kind <> suffix | kind <- [PrimaryPublisher, LoaderStartup, LoaderOrchestrationB, Record2Link, TaskSubmission, TaskStorage, RootTask, Image5Producer, PlainProducer, GeneralHeap]
                                         , suffix <- ["-slot-callers-leave-clobbers-dead", "-calls-enter-slot-entries"]]
         ++ [ "published-slot-words-name-entries", "critical-pair-slot-frames-and-r3", "storage-prelude-slot-frames-within-reservation-demand"
            , "submission-stack-with-owned-slot-frames", "loader_a-differs-only-in-carriers", "loader_b-differs-only-in-carriers", "heap-slot-retained-depth" ])
  ok "every kernel-slot host check has a violating fixture" (Set.fromList checkNames == fixtured)

  -- The adapter's caller scan over canonical bytes.
  published <- either (H.failH . show) (pure . KS.rootTargets) (KS.publishedRows (targetOf targets LoaderA) (targetOf targets LoaderB))
  plainCanonical@(PlainImage (Guest plainBase) plainBytes) <- derived (plainImage (U.parsedParts (canonicalParsed source)))
  let scan targets' plain' = slotCallerProblems targets' plain' slotRows published
      scanImages = [(show target, KS.CodeImage (targetBase target) (targetOf targets target)) | target <- [LoaderA, LoaderB, Image4, Image5]]
        ++ [("Plain", KS.CodeImage plainBase plainBytes)]
      offsets = map kernelSlotOffset slotRows
  ok "the adapter scan finds no problem in the canonical targets" (null (scan targets plainCanonical))
  forM_ scanImages $ \(name, image) -> do
    let direct = KS.directImageCalls [image] (map kernelSlotExtent slotRows)
        rooted = [call' | call' <- KS.slotImageCalls [image], KS.slotOffset call' `elem` offsets]
    putStrLn ("kernel-slot callers " ++ name ++ ": direct=" ++ show (length direct) ++ ", root-slot=" ++ show (length rooted))
  forM_ ([Just target | target <- [LoaderA, LoaderB, Image4, Image5]] ++ [Nothing]) $ \owner -> do
    let site = maybe plainBase targetBase owner
        inject words' = case owner of
          Just target -> scan (patched targets target site words') plainCanonical
          Nothing -> scan targets (PlainImage (Guest plainBase) (B.concat (map U.putWord32 words') <> B.drop (4 * length words') plainBytes))
    forM_ [ ("a direct caller that reads r1 after +0x168", [branchLink site r1Entry, 0xe1a00001])
          , ("a root-slot caller that reads r1 after +0x168", rootCaller)
          , ("a branch into +0x168 off its entry", [branchLink site (r1Entry + 4)]) ] $ \(fixture, words') ->
      ok ("the adapter scan refuses " ++ fixture ++ " in " ++ maybe "Plain" show owner) (not (null (inject words')))

  -- Staging and restore.
  (stagedJ7, observed) <- stageComposition tools scratch source j7
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4, Image5] $ \target ->
    ok ("composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf observed target)) == editSpansOf target (compositionPieces j7))
  j6 <- either (H.failH . show) pure (compose registeredJoins source j6Pieces)
  (stagedJ6, _) <- stageComposition tools scratch source j6
  ownSource <- either (H.failH . show) pure (OF.canonicalSource (canonicalDat source) (targetOf targets LoaderA) (targetOf targets LoaderB))
  admitted <- either (H.failH . show) pure (OF.admit OP.stockPins ownSource regions (linkedDomain slotA) (linkedDomain slotB))
  ok "restoring the slot extents of the composed DAT gives the composition without kernel-slot-bodies"
    (OF.restore ownSource admitted stagedJ7 == Right stagedJ6)
  passed <- readIORef counter
  putStrLn ("startup-artifact-kernel-slot-canonical-tests: " ++ show passed ++ " checks passed")
  where
    linkedDomain (LinkedImage (ImageBytes bin) (SymbolBytes symbols)) = OF.LinkedDomain bin symbols
