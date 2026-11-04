{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J6 composition: the image5-producer and
-- plain-producer pieces with the J5 pieces, their windowed closures, call-site
-- pairing, partitions, the image5 footprint and composed staging.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (nub, sort)
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (contractCarriers)
import qualified Replacement.StartupArtifact.Footprint.Producers as Producers
import Replacement.StartupArtifact.Joins (JoinKind (..), registeredJoins)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
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

patchWord :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patchWord base image address value =
  let offset = fromIntegral (address - base) in B.take offset image <> U.putWord32 value <> B.drop (offset + 4) image

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 28 ->
      pure (encoder, rebuild, decoder, take 26 rest, rest !! 26, rest !! 27)
    _ -> H.failH "usage: startup-artifact-j6-canonical-tests ENCODER REBUILD DECODER (J5 IMAGE SYMBOLS){11} IMAGE5_BIN IMAGE5_SYMBOLS PLAIN_BIN PLAIN_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      image4 = targetOf targets Image4
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
  image5 <- single Image5Producer image5Bin image5Symbols
  plain <- single PlainProducer plainBin plainSymbols
  let j5Pieces = [first, pair, publisher, startup, submission, storage, idle, root]
      j6Pieces = j5Pieces ++ [image5, plain]

  -- Pieces.
  ok "image5-producer edits only image 4 inside [0x6a96c0,0x6a9844)"
    (editTargets image5 == Set.fromList [Image4] && pieceCarriers image5 == [at Image4 0x6a96c0 0x6a9844]
      && editsInsideCarriers image5 && footprintOutsideCarriers image5 && not (null (pieceEdits image5)))
  ok "plain-producer edits only image 4 inside [0x6a9844,0x6a9938)"
    (editTargets plain == Set.fromList [Image4] && pieceCarriers plain == [at Image4 0x6a9844 0x6a9938]
      && editsInsideCarriers plain && footprintOutsideCarriers plain && not (null (pieceEdits plain)))
  let boundariesOf piece = catalogBoundaries (pieceCatalog piece)
      shape b = (labelText (boundaryLabel b), boundaryKind b, boundaryRetainedPc b, boundaryCompare b, boundaryStop b)
  putStrLn ("image5-producer boundaries: " ++ show [(labelText (boundaryLabel b), boundaryComposedPc b, boundaryRetainedPc b) | b <- boundariesOf image5])
  putStrLn ("plain-producer boundaries: " ++ show [(labelText (boundaryLabel b), boundaryComposedPc b, boundaryRetainedPc b) | b <- boundariesOf plain])
  ok "image5-producer boundaries pair with the stock sites (id4.dis:144569-144658)"
    (map shape (boundariesOf image5) ==
      [ ("image5-producer/entry", SiteBoundary, Just (Pc 0x6a96c0), CompareSequence, False)
      , ("image5-producer/segment-lookup-call", SiteBoundary, Just (Pc 0x6a96d0), CompareSequence, False)
      , ("image5-producer/fill-request-call", EntryBoundary, Just (Pc 0x6a9730), CompareSequence, True)
      , ("image5-producer/mode-call", SiteBoundary, Just (Pc 0x6a9750), CompareSequence, False)
      , ("image5-producer/decoder-init-call", SiteBoundary, Just (Pc 0x6a9778), CompareSequence, False)
      , ("image5-producer/decoder-finish-call", SiteBoundary, Just (Pc 0x6a97b8), CompareSequence, False)
      , ("image5-producer/cpu-fill-call", SiteBoundary, Just (Pc 0x6a981c), CompareSequence, False)
      , ("image5-producer/cache-call", SiteBoundary, Just (Pc 0x6a9828), CompareSequence, False)
      , ("image5-producer/caller-return", ReturnBoundary, Just (Pc 0x73b4b0), CompareSequence, True)
      , ("image5-producer/image5-entry", ReturnBoundary, Just (Pc 0xd985a4), ComparePresence, False) ])
  ok "plain-producer boundaries pair with the stock sites (id4.dis:144666-144719)"
    (map shape (boundariesOf plain) ==
      [ ("plain-producer/entry", SiteBoundary, Just (Pc 0x6a9844), CompareSequence, False)
      , ("plain-producer/segment-lookup-call", SiteBoundary, Just (Pc 0x6a9854), CompareSequence, False)
      , ("plain-producer/extent-call", SiteBoundary, Just (Pc 0x6a985c), CompareSequence, False)
      , ("plain-producer/mode-call", SiteBoundary, Just (Pc 0x6a9870), CompareSequence, False)
      , ("plain-producer/storage-read-call", EntryBoundary, Just (Pc 0x6a98ac), CompareSequence, False)
      , ("plain-producer/delay-call", SiteBoundary, Just (Pc 0x6a98f8), CompareSequence, False)
      , ("plain-producer/caller-return", ReturnBoundary, Just (Pc 0x73b558), CompareSequence, False)
      , ("plain-producer/extent0-first-hook", ReturnBoundary, Just (Pc 0x110f9d8), ComparePresence, True)
      , ("plain-producer/extent3-first-hook", ReturnBoundary, Just (Pc 0x11e00f8), ComparePresence, True) ])
  ok "every producer boundary is core 0 with one row; non-return boundaries lie in the piece's carrier"
    (and [ coreList (boundaryCores b) == [Core0] && recordLimitCount (boundaryRecordLimit b) == 1
           && (boundaryKind b == ReturnBoundary || let Pc pc = boundaryComposedPc b in pc >= low && pc < high)
         | (piece, low, high) <- [(image5, 0x6a96c0, 0x6a9844), (plain, 0x6a9844 :: Word32, 0x6a9938)], b <- boundariesOf piece ])
  let readsOf piece = [(labelText (readLabel row), labelText (readBoundary row), readAddress row, readLength row) | row <- catalogReads (pieceCatalog piece)]
  ok "image5-producer reads the flag twice, the destination, BSS and staging"
    (readsOf image5 ==
      [ ("image5-producer/fill-flag-cleared", "image5-producer/fill-request-call", 0x89f8ac, 4)
      , ("image5-producer/fill-flag-set", "image5-producer/caller-return", 0x89f8ac, 4)
      , ("image5-producer/destination", "image5-producer/caller-return", 0xbf1a00, 0x4f1e60)
      , ("image5-producer/bss", "image5-producer/caller-return", 0x1460000, 0x2bbe40)
      , ("image5-producer/staging", "image5-producer/caller-return", 0x43e40000, 0x2e0000) ])
  ok "plain-producer reads extent 0 and extent 3 only, each at its first hook (review R1)"
    (readsOf plain ==
      [ ("plain-producer/extent0", "plain-producer/extent0-first-hook", 0x1100000, 0xac5a0)
      , ("plain-producer/extent3", "plain-producer/extent3-first-hook", 0x11e0000, 0x26a5e0) ])
  ok "the producer reads total 14,305,320 bytes"
    (sum [fromIntegral len | (_, _, _, len) <- readsOf image5 ++ readsOf plain] == (14305320 :: Integer))
  let writesOf piece = let EnumeratedWrites ranges _ = catalogWrites (pieceCatalog piece) in ranges
  putStrLn ("image5-producer writes: " ++ show (writesOf image5))
  putStrLn ("plain-producer writes: " ++ show (writesOf plain))
  ok "image5-producer partition: its 48-byte frame below 0xa1c1e0 and the flag word, core 0"
    (writesOf image5 == [WriteRange (CorePhysicalTarget Core0) 0xa1c1b0 0xa1c1e0, WriteRange (CorePhysicalTarget Core0) 0x89f8ac 0x89f8b0])
  ok "plain-producer partition: its 40-byte frame below 0xa1c1e0, core 0"
    (writesOf plain == [WriteRange (CorePhysicalTarget Core0) 0xa1c1b8 0xa1c1e0])
  ok "image5-producer's footprint covers the whole decoded image5 target"
    (any (\(Footprint range _ _) -> range == at Image5 0xbf1a00 0x10e3860) (pieceFootprint image5))
  ok "image5-producer's footprint names the caller bl 0x6a96c0 and continuation; plain-producer's bl 0x6a9844"
    (any (\(Footprint range _ _) -> range == at Image4 0x73b4ac 0x73b4b4) (pieceFootprint image5)
      && any (\(Footprint range _ _) -> range == at Image4 0x73b554 0x73b55c) (pieceFootprint plain))

  -- Closures (design 2.3, review A2).
  let windowSpans = [(0x6a96c0, 0x6a9844), (0x6a9844, 0x6a9938), (0x69cbf0, 0x69cc00), (0x69d160, 0x69d2b4), (0x69cdf8, 0x69cf84)]
      within address (low, high) = address >= low && address < high
      overlaps (low, high) (start, end) = low < end && start < high
  forM_ [minBound .. maxBound] $ \producer -> do
    let extents = Producers.closureExtents producer image4
        calls = Producers.closureCalls producer image4
        ending = sort [(low, high) | (low, high) <- extents, high `elem` map fst windowSpans]
    putStrLn (show producer ++ " closure extents: " ++ show (length extents) ++ "; ending at a window bound: " ++ show ending)
    ok (show producer ++ ": no closure call targets a window")
      (not (any (\(_, target) -> any (within target) windowSpans) calls))
    ok (show producer ++ ": no closure extent overlaps a window")
      (not (any (\extent -> any (overlaps extent) windowSpans) extents))
    -- Review A2: an extent cut short by a window would be a window doing work; none is.
    ok (show producer ++ ": no closure extent ends at a window bound, so the windows bound nothing")
      (null ending)

  -- Conflicts with other carriers.
  let conflicts piece = nub [ (pieceKind other, overlap) | Footprint range _ _ <- pieceFootprint piece, other <- j6Pieces
                            , pieceKind other /= pieceKind piece, held <- pieceCarriers other, Just overlap <- [intersectSpan range held] ]
  putStrLn ("image5-producer conflicts: " ++ show (conflicts image5))
  putStrLn ("plain-producer conflicts: " ++ show (conflicts plain))
  ok "no J5 piece's footprint meets a producer carrier"
    (null [ () | piece <- j5Pieces, Footprint range _ _ <- pieceFootprint piece, held <- pieceCarriers image5 ++ pieceCarriers plain
               , Just _ <- [intersectSpan range held] ])

  -- Composition.
  j6 <- either (H.failH . show) pure (compose registeredJoins source j6Pieces)
  let newJoins = [Image5ProducerServices, PlainProducerServices]
      applied name = [spans | AppliedJoin join spans <- compositionJoins j6, joinName (joinKind join) == name]
  forM_ newJoins $ \kind -> putStrLn ("J6 " ++ show (joinName kind) ++ ": " ++ show (applied (joinName kind)))
  ok "J6 composes pending the J1-J5 observation joins and the two producer service joins"
    (compositionQualification j6 == Pending
      [ "first-stage-secondary-rendezvous", "prefix-orchestration-b", "first-stage-completion-zero"
      , "loader-startup-publisher", "dispatcher-critical-pair", "task-composition-owned-calls", "dispatcher-task-composition"
      , "dispatcher-root-entry", "dispatcher-idle-entry", "root-task-services"
      , "image5-producer-services", "plain-producer-services" ])
  ok "each producer service join applies once, over the critical-pair words [0x7235d8,0x723790)"
    (all (\kind -> applied (joinName kind) == [[at Image4 0x7235d8 0x723790]]) newJoins)
  forM_ newJoins $ \kind ->
    let without = [join | join <- registeredJoins, joinKind join /= kind]
        bounds = concat [joinBounds join | join <- registeredJoins, joinKind join == kind]
    in ok ("without " ++ show (joinName kind) ++ ", J6 refuses on a span inside its bounds")
      (case compose without source j6Pieces of
        Left (FootprintOverlap _ Record2Link range) -> coveredBy range bounds
        _ -> False)
  ok "J6 contract carriers add the two producer carriers"
    (filter ((`elem` [Image5Producer, PlainProducer]) . fst) (contractCarriers j6)
      == [(Image5Producer, at Image4 0x6a96c0 0x6a9844), (PlainProducer, at Image4 0x6a9844 0x6a9938)])
  ok "J5 still composes without the producers"
    (either (const False) (const True) (compose registeredJoins source j5Pieces))
  let imageCarrier = plain {pieceCarriers = [at Image5 0xbf1c1c 0xbf1cd4], pieceEdits = [], pieceFootprint = []}
      withoutPlainJoin = [join | join <- registeredJoins, joinKind join /= PlainProducerServices]
  ok "a piece with an image5 carrier conflicts with image5-producer's footprint (the J7 join)"
    (case compose withoutPlainJoin source (j5Pieces ++ [image5, imageCarrier]) of
      Left (FootprintOverlap Image5Producer PlainProducer range) -> range == at Image5 0xbf1c1c 0xbf1cd4
      _ -> False)

  -- Host checks.
  let named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      evidenceWith canonical' = JoinEvidence canonical' (compositionTargets j6) (U.parsedParts (canonicalParsed source)) (compositionPieces j6)
      holdsWith canonical' name = not (null (named name)) && all ($ evidenceWith canonical') (named name)
  forM_ [(Producers.Image5Load, Image5ProducerServices), (Producers.PlainLoad, PlainProducerServices)] $ \(producer, kind) -> do
    let checkName = joinName kind <> "-closure-enters-carriers-at-entries"
        intoPair = [ site | (site, target) <- Producers.closureCalls producer image4, target == 0x7235d8 || target == 0x7236b0
                   , not (site >= 0x7235d8 && site < 0x723790) ]
    putStrLn (show producer ++ " closure calls into the critical pair: " ++ show (length intoPair))
    ok (show checkName ++ " holds on canonical image 4") (holdsWith targets checkName)
    case intoPair of
      site : _ -> do
        let retargeted = either (const targets) id
              (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] targets) (edit (at Image4 site (site + 4)) (U.putWord32 (branchLink site 0x7235dc))))
        ok (show checkName ++ " fails with one closure call retargeted to 0x7235dc") (not (holdsWith retargeted checkName))
      [] -> H.failH (show producer ++ " closure has no call into the critical pair")

  -- Adapter refusals.
  ok "the caller check accepts the canonical flashloader calls"
    (callerReturn Image5Producer image4 0x73b4ac 0x6a96c0 == Right 0x73b4b0
      && callerReturn PlainProducer image4 0x73b554 0x6a9844 == Right 0x73b558)
  ok "the caller check refuses a flashloader site retargeted by 4"
    (either (const True) (const False)
      (callerReturn Image5Producer (patchWord 0x61c400 image4 0x73b4ac (branchLink 0x73b4ac 0x6a96c4)) 0x73b4ac 0x6a96c0))
  let fillRequest = ProducerCall 0x66609c "fill-request" EntryBoundary True
  ok "pairing accepts a unique owned fill-request site"
    (either (const False) (const True) (producerCallBoundaries Image5Producer image4 (0x6a96c0, 0x6a9844) [(0x6a9700, 0x66609c)] [fillRequest]))
  ok "pairing refuses two owned fill-request sites"
    (either (const True) (const False)
      (producerCallBoundaries Image5Producer image4 (0x6a96c0, 0x6a9844) [(0x6a9700, 0x66609c), (0x6a9710, 0x66609c)] [fillRequest]))
  ok "pairing refuses a target with two stock sites (the mode-0 and nonzero storage reads)"
    (either (const True) (const False)
      (producerCallBoundaries Image5Producer image4 (0x6a96c0, 0x6a9844) [(0x6a9700, 0x6a8f1c)] [ProducerCall 0x6a8f1c "storage-read" EntryBoundary False]))

  -- Staging.
  (_, observed) <- stageComposition tools scratch source j6
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4, Image5] $ \target ->
    ok ("J6 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf observed target)) == editSpansOf target j6Pieces)

  passed <- readIORef counter
  putStrLn ("startup-artifact-j6-canonical-tests: " ++ show passed ++ " checks passed")
