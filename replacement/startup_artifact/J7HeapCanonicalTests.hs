{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J7 heap cutover: the general-heap piece with the J6 pieces, its
-- closure, joins, adapter and join host checks against planted references, partition, reads, the
-- contract's per-role read expectations and the stager's image cap.
module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (nub)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.GeneralHeap.Admission as HA
import qualified Replacement.GeneralHeap.Artifact as HT
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Application (PlainImage (..), plainImage)
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (contractCarriers, contractValue, record5Media)
import qualified Replacement.StartupArtifact.Footprint.GeneralHeap as Heap
import Replacement.StartupArtifact.Joins (JoinKind (..), depthsIn, registeredJoins, suppliedOf)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified StockStack as SS
import System.Directory (canonicalizePath, removeFile)
import System.Environment (getArgs)
import System.FilePath ((</>))

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (H.failH ("failed: " ++ name))
  modifyIORef' counter (+ 1)

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

editTargets :: Piece -> Set.Set Target
editTargets piece = Set.fromList [spanTarget (editSpan change) | change <- pieceEdits piece]

-- | An A32 BL from a site to a target.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | @movw r0@ and @movt r0@ forming the value.
formed :: Word32 -> [Word32]
formed value = [encode 0xe3000000 (value .&. 0xffff), encode 0xe3400000 (value `shiftR` 16)]
  where encode opcode half = opcode .|. ((half .&. 0xf000) `shiftL` 4) .|. (half .&. 0xfff)

-- | Consecutive words from an address.
run :: Target -> Word32 -> [Word32] -> [(Target, Word32, Word32)]
run target address values = [(target, address + 4 * index, value) | (index, value) <- zip [0 ..] values]

-- | The targets with each (target, address, word) stored.
plant :: TargetBytes -> [(Target, Word32, Word32)] -> IO TargetBytes
plant targets stores = case mapM (\(target, address, value) -> edit (at target address (address + 4)) (U.putWord32 value)) stores of
  Just edits -> either (H.failH . show) pure (applyTargetEdits edits targets)
  Nothing -> H.failH "plant: empty edit"

segments :: U.Parts -> TargetBytes -> [(Word32, B.ByteString)]
segments parts targets = [(targetBase target, bytes) | (target, bytes) <- targetList targets]
  ++ [(load, bytes) | Right (PlainImage (Guest load) bytes) <- [plainImage parts]]

path :: [T.Text] -> J.Value -> Maybe J.Value
path keys value = case keys of
  [] -> Just value
  key : rest -> case value of
    J.Object members -> KM.lookup (Key.fromText key) members >>= path rest
    _ -> Nothing

sha256 :: B.ByteString -> J.Value
sha256 = J.String . T.pack . SHA.sha256Hex . SHA.sha256

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, heapPaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 30 ->
      pure (encoder, rebuild, decoder, take 26 rest, take 2 (drop 26 rest), rest !! 28, rest !! 29)
    _ -> H.failH "usage: startup-artifact-j7-heap-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){13} HEAP_BIN HEAP_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols] <- pure resolved
  [heapBin, heapSymbols] <- mapM canonicalizePath heapPaths
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      image4 = targetOf targets Image4
      parts = U.parsedParts (canonicalParsed source)
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
  heapBytes <- H.readRegular (imageLimit GeneralHeap) heapBin
  heapSymbolBytes <- H.readRegular 0x10000 heapSymbols
  heap <- derived (derivePieceOf source (SingleImage GeneralHeap (LinkedImage (ImageBytes heapBytes) (SymbolBytes heapSymbolBytes))))
  artifact <- either (H.failH . show) pure (HT.deriveArtifact heapBytes heapSymbolBytes)
  let j6Pieces = [first, pair, publisher, startup, submission, storage, idle, root, image5, plain]
      j7Pieces = j6Pieces ++ [heap]
      entries = map snd HA.entrySymbols

  -- Image cap.
  ok "the heap image is larger than every other piece's 64 KiB cap and loads under the general-heap cap"
    (B.length heapBytes > 0x10000 && B.length heapBytes <= imageLimit GeneralHeap)
  let oversize = scratchPath </> "j7-heap-oversize.bin"
  B.writeFile oversize (B.replicate (4 * 1024 * 1024 + 1) 0)
  refusedOversize <- try (H.readRegular (imageLimit GeneralHeap) oversize) :: IO (Either IOException B.ByteString)
  removeFile oversize
  ok "a 4 MiB + 1 heap image refuses" (either (const True) (const False) refusedOversize)

  -- Piece.
  ok "general-heap carries the five admission carriers and edits images 4 and 5 inside them"
    (normalizeSpans (pieceCarriers heap) == normalizeSpans Heap.carriers && editTargets heap == Set.fromList [Image4, Image5]
      && all (\change -> coveredBy (editSpan change) (pieceCarriers heap)) (pieceEdits heap))
  ok "no heap edit covers the sentinel word"
    (null [() | change <- pieceEdits heap, Just _ <- [intersectSpan (editSpan change) (at Image4 Heap.sentinel (Heap.sentinel + 4))]])
  ok "the heap footprint lies outside its carriers"
    (null [() | Footprint range _ _ <- pieceFootprint heap, carrier <- pieceCarriers heap, Just _ <- [intersectSpan range carrier]])
  let EnumeratedWrites heapWrites _ = catalogWrites (pieceCatalog heap)
      core0 = WriteRange (CorePhysicalTarget Core0)
  putStrLn ("general-heap writes: " ++ show heapWrites)
  ok "partition: arena [0xa71a00,0xbf19e0), sentinel, errno and the task pool on core 0, nothing on core 1"
    (heapWrites == [core0 0xa71a00 0xbf19e0, core0 0x89d4f8 0x89d4fc, core0 0x89c978 0x89c97c, core0 0xa10200 0xa5b200])
  let readsOf piece = [(labelText (readLabel row), readDiffers row /= Nothing, readExpect row) | row <- catalogReads (pieceCatalog piece)]
  ok "image5-producer destination and staging carry per-role expectations, its other reads none"
    (readsOf image5 ==
      [ ("image5-producer/fill-flag-cleared", False, Nothing), ("image5-producer/fill-flag-set", False, Nothing)
      , ("image5-producer/destination", False, Just (ExpectTarget Image5)), ("image5-producer/bss", False, Nothing)
      , ("image5-producer/staging", False, Just ExpectRecord5Media) ])
  ok "general-heap reads: the sentinel and storage records differ, the graphics table compares"
    (readsOf heap == [("general-heap/sentinel", True, Nothing), ("general-heap/graphics-table", False, Nothing), ("general-heap/storage-records", True, Nothing)])

  -- Closure windows (design 3.4).
  let windowSpans = [(0x6a96c0, 0x6a9844), (0x6a9844, 0x6a9938), (0x69cbf0, 0x69cc00), (0x69d160, 0x69d2b4), (0x69cdf8, 0x69cf84)]
        ++ [(low, high) | Span _ (Guest low) (Guest high) <- Heap.image4Carriers]
      within address (low, high) = address >= low && address < high
      overlaps (low, high) (start, end) = low < end && start < high
      extents = Heap.closureExtents image4
      calls = Heap.closureCalls image4
  putStrLn ("heap closure extents: " ++ show (length extents) ++ "; calls: " ++ show (length calls))
  ok "the lock, unlock and memmove closure is non-empty" (not (null extents))
  ok "no heap closure call targets a window" (not (any (\(_, target) -> any (within target) windowSpans) calls))
  ok "no heap closure extent overlaps a window" (not (any (\extent -> any (overlaps extent) windowSpans) extents))
  -- memmove is runtime_move_bytes [0x89d908,0x89da3c), exact (xm1-decomp lookup 0x89d908), ending in b 0x89d954.
  ok "the only heap closure extent ending at a window's low bound is memmove's own extent, ending in an unconditional b"
    ([(low, high) | (low, high) <- extents, high `elem` map fst windowSpans] == [(0x89d908, 0x89da3c)]
      && U.word32At image4 (fromIntegral (0x89da38 - 0x61c400 :: Word32)) .&. 0xff000000 == 0xea000000)

  -- Conflicts.
  let conflicts footprints held = nub [ (pieceKind other, overlap) | Footprint range _ _ <- footprints, other <- held
                                      , carrier <- pieceCarriers other, Just overlap <- [intersectSpan range carrier] ]
      heapConflicts = conflicts (pieceFootprint heap) j6Pieces
  putStrLn ("general-heap footprint conflicts: " ++ show heapConflicts)
  ok "the only J6 footprint meeting a heap carrier is image5-producer's, on the image5 carrier"
    (conflicts (concatMap pieceFootprint j6Pieces) [heap] == [(GeneralHeap, Heap.image5Carrier)]
      && null [() | Footprint range _ _ <- concatMap pieceFootprint (filter ((/= Image5Producer) . pieceKind) j6Pieces), carrier <- pieceCarriers heap, Just _ <- [intersectSpan range carrier]])
  ok "the heap footprint meets only loader-startup's carriers, inside the dispatcher window"
    (not (null heapConflicts) && all (\(kind, overlap) -> kind == LoaderStartup && coveredBy overlap [at Image4 0x69cdf8 0x69cf84]) heapConflicts)

  -- Composition.
  j7 <- either (H.failH . show) pure (compose registeredJoins source j7Pieces)
  let heapJoins = [GeneralHeapImage5Publication, GeneralHeapArenaClear]
      applied kind = [spans | AppliedJoin join spans <- compositionJoins j7, joinKind join == kind]
      boundsOf kind = concat [joinBounds join | join <- registeredJoins, joinKind join == kind]
  forM_ heapJoins $ \kind -> putStrLn ("J7 " ++ show (joinName kind) ++ ": " ++ show (applied kind))
  ok "general-heap-image5-publication applies once over the image5 carrier"
    (applied GeneralHeapImage5Publication == [[Heap.image5Carrier]])
  ok "general-heap-arena-clear applies once inside the dispatcher window"
    (case applied GeneralHeapArenaClear of
      [spans] -> not (null spans) && all (\range -> coveredBy range (boundsOf GeneralHeapArenaClear)) spans
      _ -> False)
  ok "J7 is pending general-heap-image5-publication and not the host-proved general-heap-arena-clear"
    (case compositionQualification j7 of
      Pending names -> "general-heap-image5-publication" `elem` names && "general-heap-arena-clear" `notElem` names
      HostComposed -> False)
  forM_ heapJoins $ \kind ->
    ok ("without " ++ show (joinName kind) ++ ", J7 refuses on a span inside its bounds")
      (case compose [join | join <- registeredJoins, joinKind join /= kind] source j7Pieces of
        Left (FootprintOverlap _ _ range) -> coveredBy range (boundsOf kind)
        _ -> False)
  ok "J7 contract carriers add the five heap carriers"
    (filter ((== GeneralHeap) . fst) (contractCarriers j7) == [(GeneralHeap, range) | range <- normalizeSpans Heap.carriers])
  ok "J6 still composes without the heap" (either (const False) (const True) (compose registeredJoins source j6Pieces))

  -- Join host checks.
  let composedTargets = compositionTargets j7
      evidence = JoinEvidence targets composedTargets parts (compositionPieces j7)
      named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      holds evidence' name = not (null (named name)) && all ($ evidence') (named name)
      withComposed stores = (\composed -> evidence {evidenceComposed = composed}) <$> plant composedTargets stores
      outside = 0x700000
  forM_ [ "segment5-publishes-heap-carrier", "image5-carrier-unnamed-before-publication", "composed-entry-surface"
        , "composed-retired-state-unreferenced", "arena-inside-mode1-clear", "sentinel-outside-clear-and-zero", "no-heap-call-in-phase-0" ] $ \name ->
    ok (T.unpack name ++ " holds on the J7 composition") (holds evidence name)
  let moved = parts {U.partSegments = [if U.segId segment == 5 then segment {U.segLoad = U.segLoad segment + 0x1000} else segment | segment <- U.partSegments parts]}
  ok "segment5-publishes-heap-carrier fails with segment 5 loaded 0x1000 higher"
    (not (holds evidence {evidenceParts = moved} "segment5-publishes-heap-carrier"))
  namedBefore <- withComposed [(Image4, outside, branchLink outside 0xbf1c1c)]
  ok "image5-carrier-unnamed-before-publication fails with a bl 0xbf1c1c outside the carriers"
    (not (holds namedBefore "image5-carrier-unnamed-before-publication"))
  ownedNaming <- withComposed (run Image4 0x89cda4 (formed 0xbf1c1c))
  ok "image5-carrier-unnamed-before-publication fails with an owned movw/movt of 0xbf1c1c in the heap's image-4 carrier"
    (not (holds ownedNaming "image5-carrier-unnamed-before-publication"))
  intoProducer <- withComposed [(Image4, 0x6a96c4, branchLink 0x6a96c4 0x89ce00)]
  ok "composed-entry-surface fails with a bl 0x89ce00 in image5-producer's carrier"
    (not (holds intoProducer "composed-entry-surface"))
  retiredFormed <- withComposed (run Image4 outside (formed 0x89d500))
  ok "composed-retired-state-unreferenced fails with a movw/movt of 0x89d500"
    (not (holds retiredFormed "composed-retired-state-unreferenced"))
  let widened = [ case pieceFacts piece of
                    GeneralHeapFacts facts -> piece {pieceFacts = GeneralHeapFacts facts {heapArena = (fst (heapArena facts), 0xbf2a00)}}
                    _ -> piece
                | piece <- compositionPieces j7 ]
  ok "arena-inside-mode1-clear fails for an arena ending at 0xbf2a00"
    (not (holds evidence {evidencePieces = widened} "arena-inside-mode1-clear"))
  sentinelSet <- withComposed [(Image4, Heap.sentinel, 1)]
  ok "sentinel-outside-clear-and-zero fails with the composed sentinel word 1"
    (not (holds sentinelSet "sentinel-outside-clear-and-zero"))
  loaderCall <- withComposed [(LoaderA, 0x40a00, branchLink 0x40a00 0x89cda4)]
  ok "no-heap-call-in-phase-0 fails with a loader-A bl to malloc"
    (not (holds loaderCall "no-heap-call-in-phase-0"))

  -- Adapter checks (design 4.3).
  ok "graphics-table-forwards holds on canonical image 4" (Heap.graphicsTableForwards image4)
  changedTable <- plant targets [(Image4, 0x890f0c, 0x890ecc)]
  ok "graphics-table-forwards fails with table word 0x890f0c changed"
    (not (Heap.graphicsTableForwards (targetOf changedTable Image4)))
  ok "entry-surface: no canonical reference names a heap carrier interior" (null (Heap.entrySurface entries (segments parts targets)))
  plantedCall <- plant targets [(Image4, outside, branchLink outside 0x89ce00)]
  ok "entry-surface fails with a planted bl 0x89ce00"
    (Heap.entrySurface entries (segments parts plantedCall) == [Heap.BranchReference outside 0x89ce00])
  changedDataWord <- plant targets [(Image5, 0x109ba48, branchLink 0x109ba48 0x89d26c)]
  ok "entry-surface fails when the image-5 data word 0x109ba48 holds any other branch into the carrier"
    (Heap.entrySurface entries (segments parts changedDataWord) == [Heap.BranchReference 0x109ba48 0x89d26c])
  symbols <- either (H.failH . show) pure (F.parseSymbols heapSymbolBytes)
  metadataStart <- either (H.failH . show) (pure . F.symbolAddress) (F.lookupSymbol symbols "XM1_HEAP_METADATA")
  let instructions = HA.instructionPCs (HT.artifactFrames artifact)
      metadata = (metadataStart, metadataStart + 56)
      staged4 = targetOf composedTargets Image4
      admitted address = address `elem` instructions || address `elem` entries || address == Heap.sentinel
        || (fst metadata <= address && address < snd metadata)
      unadmitted = take 1 [address | Span _ (Guest low) (Guest high) <- Heap.image4Carriers, address <- [low, low + 4 .. high - 4], not (admitted address)]
  ok "ownedSurface holds on the staged heap image" (Heap.ownedSurface instructions entries metadata staged4)
  case unadmitted of
    address : _ -> do
      ownedLiteral <- plant composedTargets [(Image4, 0x89cda4, address)]
      ok ("ownedSurface fails with a planted owned literal " ++ show address)
        (not (Heap.ownedSurface instructions entries metadata (targetOf ownedLiteral Image4)))
    [] -> H.failH "every heap carrier word is admitted"
  ok "retired-state-unreferenced holds on the canonical images" (Heap.retiredStateUnreferenced (segments parts targets))
  plantedRetired <- plant targets (run Image4 outside (formed 0x89d500))
  ok "retired-state-unreferenced fails with a planted movw/movt of 0x89d500"
    (not (Heap.retiredStateUnreferenced (segments parts plantedRetired)))
  ok "the staged heap code names no retired stock state" (Heap.ownedNamesNoRetired staged4)
  namesBreak <- plant composedTargets [(Image4, 0x89cda4, 0x829a08)]
  ok "ownedNamesNoRetired fails with a planted owned literal 0x829a08"
    (not (Heap.ownedNamesNoRetired (targetOf namesBreak Image4)))

  -- Stack: supplied depths with the heap (design 6.1).
  case suppliedOf evidence of
    Nothing -> H.failH "suppliedOf refused on the J7 composition"
    Just supplied -> do
      let depthOf entry = (\(SS.StackDepth bytes) -> toInteger bytes) <$> M.lookup entry supplied
          owned = [(0x89cda4, 240), (0x89cde4, 240), (0x89cdc4, 200), (0x89e560, 200), (0x89da3c, 280), (0x89da64, 280), (0x89e464, 160), (0xbf1c1c, 240), (0xbf1c44, 240)]
      putStrLn ("heap supplied depths: " ++ show [(entry, depthOf entry) | (entry, _) <- owned])
      ok "every heap entry is supplied, at least its owned cumulative depth"
        (all (\(entry, bytes) -> maybe False (>= bytes) (depthOf entry)) owned)
  -- The retained role's demands: stock malloc, free, _malloc_r and _free_r walked on canonical image 4, the
  -- J6 pieces' owned entries (critical pair, wrapper, idle, root) taking their proved depths (design 6.2).
  j6 <- either (H.failH . show) pure (compose registeredJoins source j6Pieces)
  let stockEvidence = JoinEvidence targets targets parts (compositionPieces j6)
  case suppliedOf stockEvidence >>= \supplied -> depthsIn stockEvidence supplied [0x89cda4, 0x89cdc4, 0x89cde4, 0x89e560] of
    Nothing -> H.failH "no supplied depths or indirect targets for the stock heap walk"
    Just results -> do
      putStrLn ("stock heap depths: " ++ show [(entry, fmap (\report -> let SS.StackDepth bytes = SS.reportDepth report in bytes) result) | (entry, result) <- results])
      ok "stock malloc, free, _malloc_r and _free_r have bounded depths on canonical code"
        (length results == 4 && all (either (const False) (const True) . snd) results)

  -- Staging and the contract.
  (dat, observed) <- stageComposition tools scratch source j7
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4, Image5] $ \target ->
    ok ("J7 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf observed target))
        == normalizeSpans [editSpan change | piece <- j7Pieces, change <- pieceEdits piece, spanTarget (editSpan change) == target])
  ok "the canonical record-5 media digest is J6 consume check 4's 817e3970"
    (fmap sha256 (record5Media (canonicalDat source)) == Just (J.String "817e3970b6c6a06290d4b89536f18cd37cda71a4e12c7ab05f7f78a7c99381d1"))
  attach <- case catalogBoundaries (pieceCatalog heap) of
    boundary : _ -> pure (boundaryLabel boundary)
    [] -> H.failH "general-heap has no boundary"
  let contract = contractValue source "" j7 dat observed attach J.Null Nothing
      producer = case path ["pieces"] contract of
        Just (J.Array pieces) -> [piece | piece <- foldr (:) [] pieces, path ["kind"] piece == Just (J.String "image5-producer")]
        _ -> []
      expectOf name = case producer of
        [piece] -> case path ["catalog", "reads"] piece of
          Just (J.Array rows) -> [row | row <- foldr (:) [] rows, path ["label"] row == Just (J.String name)] >>= \row -> foldr (:) [] (path ["expect"] row)
          _ -> []
        _ -> []
      digests name = [(path ["retained_sha256"] expectation, path ["composed_sha256"] expectation) | expectation <- expectOf name]
  ok "the destination expectation is the canonical image5 retained and the composed image5 composed, which differ"
    (digests "image5-producer/destination" == [(Just (sha256 (targetOf targets Image5)), Just (sha256 (targetOf observed Image5)))]
      && targetOf targets Image5 /= targetOf observed Image5)
  ok "the staging expectation is each DAT's record-5 media digest, which differ"
    (digests "image5-producer/staging" == [(sha256 <$> record5Media (canonicalDat source), sha256 <$> record5Media dat)]
      && record5Media (canonicalDat source) /= record5Media dat)
  ok "the contract identities carry both record-5 media digests"
    (path ["canonical", "record5_media_sha256"] contract == (sha256 <$> record5Media (canonicalDat source))
      && path ["composed", "record5_media_sha256"] contract == (sha256 <$> record5Media dat))

  passed <- readIORef counter
  putStrLn ("startup-artifact-j7-heap-canonical-tests: " ++ show passed ++ " checks passed")
