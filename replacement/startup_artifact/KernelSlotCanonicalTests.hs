{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the kernel-slot-bodies piece composed with the J6 pieces and with the J2
-- loader-B orchestration: carriers, composition, each join's necessity, each host check against a violating
-- fixture, staging and the own-first restore.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits (complement, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (find, nub)
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins (JoinKind (..), registeredJoins)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
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
    encoder : rebuild : decoder : rest | length rest == 33 ->
      pure (encoder, rebuild, decoder, take 31 rest, rest !! 31, rest !! 32)
    _ -> H.failH "usage: startup-artifact-kernel-slot-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){13} SLOT_A_BIN SLOT_A_SYMBOLS SLOT_B_BIN SLOT_B_SYMBOLS REGIONS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols, slotABin, slotASymbols, slotBBin, slotBSymbols, regionsPath] <- pure resolved
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
  let appliedIn composition = [joinKind join | AppliedJoin join _ <- compositionJoins composition]
      kernelJoins = [KernelSlotPrimaryPublisher ..]
      j7Joins = filter (`elem` appliedIn j7) kernelJoins
      j2Joins = filter (`elem` appliedIn j2) kernelJoins
  putStrLn ("kernel-slot joins in the J6 composition: " ++ show j7Joins)
  putStrLn ("kernel-slot joins in the J2 composition: " ++ show j2Joins)
  ok "the J6 pieces compose with kernel-slot-bodies under every kernel-slot join but the loader-B orchestration one"
    (j7Joins == filter (/= KernelSlotLoaderOrchestrationB) kernelJoins)
  ok "the J2 pieces compose with kernel-slot-bodies under the publisher, record-2 and loader-B orchestration joins"
    (j2Joins == [KernelSlotPrimaryPublisher, KernelSlotLoaderOrchestrationB, KernelSlotRecord2Link])
  forM_ ([(kind, j6Pieces ++ [slots]) | kind <- j7Joins] ++ [(kind, j2Pieces ++ [slots]) | kind <- j2Joins]) $ \(kind, pieces) -> do
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
                               , target `elem` [LoaderA, LoaderB, Image4], end - start >= 8
                               , not (any (\carrier -> intersectSpan carrier (at target start end) /= Nothing) (pieceCarriers slots)) ]
      targetOfAddress address = case [target | target <- [LoaderA, LoaderB, Image4], targetBase target <= address
                                       , address - targetBase target < fromIntegral (B.length (targetOf targets target))] of
        target : _ -> target
        [] -> Image4
      compositionFor kind = if kind == LoaderOrchestrationB then j2 else j7
  ok "loader A's +0x168 bodies may leave r1 different from stock" (r1Entry /= 0)
  putStrLn ("kernel-slot host checks: " ++ show checkNames)
  forM_ checkNames $ \name -> ok (T.unpack name ++ " holds on the J6 or J2 composition")
    (holds name (evidenceOf targets (compositionTargets j7) (compositionPieces j7))
      || holds name (evidenceOf targets (compositionTargets j2) (compositionPieces j2)))
  forM_ [PrimaryPublisher, LoaderStartup, LoaderOrchestrationB, Record2Link, TaskSubmission, TaskStorage, RootTask, Image5Producer, PlainProducer] $ \kind -> do
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
        ([pieceKindName kind <> suffix | kind <- [PrimaryPublisher, LoaderStartup, LoaderOrchestrationB, Record2Link, TaskSubmission, TaskStorage, RootTask, Image5Producer, PlainProducer]
                                         , suffix <- ["-slot-callers-leave-clobbers-dead", "-calls-enter-slot-entries"]]
         ++ [ "published-slot-words-name-entries", "critical-pair-slot-frames-and-r3", "storage-prelude-slot-frames-within-reservation-demand"
            , "submission-stack-with-owned-slot-frames", "loader_a-differs-only-in-carriers", "loader_b-differs-only-in-carriers" ])
  ok "every kernel-slot host check has a violating fixture" (Set.fromList checkNames == fixtured)

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
