{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input stager tests: each adapter on the canonical X-M1 1.50 DAT
-- and its linked image, the composed staging routes and the loader-A refusal.
module Main where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Application
import Replacement.StartupArtifact.AttachState (AttachSources (..), attachStateValue, parseAttachRules)
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (contractCarriers)
import Replacement.StartupArtifact.Joins (JoinKind (..), registeredJoins)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import System.Directory (canonicalizePath)
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

editsInsideCarriers :: Piece -> Bool
editsInsideCarriers piece = all (\change -> coveredBy (editSpan change) (pieceCarriers piece)) (pieceEdits piece)

footprintOutsideCarriers :: Piece -> Bool
footprintOutsideCarriers piece =
  null [() | Footprint range _ _ <- pieceFootprint piece, carrier <- pieceCarriers piece, Just _ <- [intersectSpan range carrier]]

-- | Edit spans of one target, merged.
editSpansOf :: Target -> [Piece] -> [Span]
editSpansOf target pieces = normalizeSpans [editSpan change | piece <- pieces, change <- pieceEdits piece, spanTarget (editSpan change) == target]

sha256Text :: B.ByteString -> IO Text
sha256Text = pure . T.pack . SHA.sha256Hex . SHA.sha256

keyPaths :: J.Value -> Set.Set [Text]
keyPaths value = case value of
  J.Object fields -> Set.unions [Set.insert [Key.toText key] (Set.map (Key.toText key :) (keyPaths inner)) | (key, inner) <- KeyMap.toList fields]
  J.Array items -> Set.unions (map keyPaths (V.toList items))
  _ -> Set.empty

-- | Key paths outside @guard.attach_state.relations@, whose members the fixture leaves null.
contractKeys :: J.Value -> Set.Set [Text]
contractKeys = Set.filter (\path -> take 3 path /= ["guard", "attach_state", "relations"] || length path == 3) . keyPaths

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, fixturePath, datPath, scratchPath) <- case arguments of
    [encoder, rebuild, decoder, firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationBin, orchestrationSymbols
      , orchestrationABin, orchestrationASymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols, fixture, dat, scratch] ->
      pure (encoder, rebuild, decoder, [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationBin, orchestrationSymbols
        , orchestrationABin, orchestrationASymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols], fixture, dat, scratch)
    _ -> H.failH "usage: startup-artifact-canonical-tests ENCODER REBUILD DECODER FIRST_BIN FIRST_SYMBOLS PAIR_BIN PAIR_SYMBOLS PUBLISHER_BIN PUBLISHER_SYMBOLS ORCHESTRATION_B_BIN ORCHESTRATION_B_SYMBOLS ORCHESTRATION_A_BIN ORCHESTRATION_A_SYMBOLS DISPATCHER_BIN DISPATCHER_SYMBOLS INSTALLER_BIN INSTALLER_SYMBOLS CONTRACT_FIXTURE CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationBin, orchestrationSymbols
    , orchestrationABin, orchestrationASymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols] <- pure resolved
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
  targetDigests <- mapM (\target -> sha256Text (targetOf targets target)) [LoaderA, LoaderB, Image4, Image5]
  ok "canonical loaders and decoded images carry their pinned identities"
    (targetDigests ==
      [ "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
      , "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
      , "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
      , "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" ])

  firstImage <- linked firstBin firstSymbols
  pairImage <- linked pairBin pairSymbols
  publisherImage <- linked publisherBin publisherSymbols
  first <- either (H.failH . show) pure (deriveFirstStage source firstImage)
  pair <- either (H.failH . show) pure (deriveRecord2Link source pairImage)
  publisher <- either (H.failH . show) pure (derivePrimaryPublisher source publisherImage)
  forM_ [first, pair, publisher] $ \piece -> do
    let name = show (pieceKind piece)
    ok (name ++ " derives edits") (not (null (pieceEdits piece)))
    ok (name ++ " edits lie inside its carriers") (editsInsideCarriers piece)
    ok (name ++ " footprint excludes its carriers") (footprintOutsideCarriers piece)
  ok "first stage edits only record 1" (editTargets first == Set.fromList [Record1])
  ok "first stage carries the reset literal" (at Record1 0x20 0x24 `elem` pieceCarriers first)
  ok "record-2 link edits record 2, loader B and image 4" (editTargets pair == Set.fromList [Record2, LoaderB, Image4])
  ok "record-2 link carries both loader-B publications and the image-4 pair"
    (all (`coveredBy` pieceCarriers pair) [at LoaderB 0xd000c 0xd0010, at LoaderB 0xd0430 0xd0434, at Image4 0x7235d8 0x723790]
      && not (coveredBy (at LoaderB 0xd0aac 0xd0ab0) (pieceCarriers pair)))
  ok "publisher edits only its loader-A window" (editTargets publisher == Set.fromList [LoaderA] && pieceCarriers publisher == [at LoaderA 0x40430 0x409ac])

  let composeOf :: [Piece] -> Either Refusal (Composition JoinKind)
      composeOf = compose registeredJoins source
      composeUnjoined :: [Piece] -> Either Refusal (Composition JoinKind)
      composeUnjoined = compose [] source
  ok "without joins, record-2 link and publisher refuse on the loader-A footprint"
    (all (\pieces -> case composeUnjoined pieces of
        Left (FootprintOverlap Record2Link PrimaryPublisher range) -> coveredBy range [at LoaderA 0x40430 0x409ac]
        _ -> False) [[pair, publisher], [publisher, pair]])
  ok "without joins, first stage and record-2 link refuse on the record-2 rendezvous"
    (case composeUnjoined [first, pair] of
      Left (FootprintOverlap FirstStage Record2Link range) -> spanTarget range == Record2
      _ -> False)
  ok "the binding join admits record-2 link and publisher host-qualified"
    (fmap compositionQualification (composeOf [pair, publisher]) == Right HostComposed)
  ok "the rendezvous join admits first stage and record-2 link as pending"
    (fmap compositionQualification (composeOf [first, pair]) == Right (Pending ["first-stage-secondary-rendezvous"]))

  -- J2: loader-B orchestration with the record-2 link, publisher and first stage.
  orchestrationImage <- linked orchestrationBin orchestrationSymbols
  orchestration <- either (H.failH . show) pure (deriveLoaderOrchestrationB source orchestrationImage)
  ok "loader-B orchestration edits only its 340-byte carrier"
    (editTargets orchestration == Set.fromList [LoaderB] && pieceCarriers orchestration == [at LoaderB 0xd09ac 0xd0b00]
      && editsInsideCarriers orchestration && footprintOutsideCarriers orchestration)
  ok "without joins, record-2 link and loader-B orchestration refuse on loader B"
    (case composeUnjoined [pair, orchestration] of
      Left (FootprintOverlap _ _ range) -> spanTarget range == LoaderB
      _ -> False)
  ok "without joins, loader-B orchestration and the first stage refuse on record 1"
    (case composeUnjoined [first, orchestration] of
      Left (FootprintOverlap LoaderOrchestrationB FirstStage range) -> spanTarget range == Record1
      _ -> False)
  ok "a record-2 link still carrying the 0xd0aac call word refuses on the orchestration carrier"
    (case composeUnjoined [pair {pieceCarriers = pieceCarriers pair ++ [at LoaderB 0xd0aac 0xd0ab0]}, orchestration] of
      Left (CarrierOverlap Record2Link LoaderOrchestrationB range) -> range == at LoaderB 0xd0aac 0xd0ab0
      _ -> False)
  j2 <- either (H.failH . show) pure (composeOf [first, pair, publisher, orchestration])
  let appliedOf name = [spans | AppliedJoin join spans <- compositionJoins j2, joinName (joinKind join) == name]
      catalogOf kind = [pieceCatalog piece | piece <- compositionPieces j2, pieceKind piece == kind]
      labelsOf catalog = map (labelText . boundaryLabel) (catalogBoundaries catalog)
  ok "J2 composes pending the rendezvous, prefix-orchestration and completion-zero joins"
    (compositionQualification j2 == Pending ["first-stage-secondary-rendezvous", "prefix-orchestration-b", "first-stage-completion-zero"])
  ok "prefix-orchestration-b covers exactly the three loader-B conflicts"
    (appliedOf "prefix-orchestration-b" == [[at LoaderB 0xd000c 0xd0010, at LoaderB 0xd0430 0xd0434, at LoaderB 0xd09ac 0xd0b00]])
  ok "first-stage-completion-zero covers the first-stage carriers"
    (appliedOf "first-stage-completion-zero" == [normalizeSpans (pieceCarriers first)])
  ok "the record-2 initializer return and its table read yield to loader-B orchestration"
    (map labelsOf (catalogOf Record2Link) == [["record2-link/secondary-entry", "record2-link/prefix-entry", "record2-link/initializer-entry"
        , "record2-link/critical-enter", "record2-link/critical-exit"]]
      && map catalogReads (catalogOf Record2Link) == [[]]
      && map (map (labelText . readLabel) . catalogReads) (catalogOf LoaderOrchestrationB)
        == [["loader-orchestration-b/workspace", "loader-orchestration-b/vectors", "loader-orchestration-b/frame"
            , "loader-orchestration-b/completion", "loader-orchestration-b/table-b", "loader-orchestration-b/ready"]])
  ok "without loader-B orchestration the record-2 link keeps the stock initializer return"
    (either (const False) (\composition -> any (elem "record2-link/initializer-return" . labelsOf . pieceCatalog) (compositionPieces composition))
      (composeOf [first, pair, publisher]))
  let j2Targets = compositionTargets j2
      j2Pieces = compositionPieces j2
      evidence targets' pieces' = JoinEvidence targets targets' (U.parsedParts (canonicalParsed source)) pieces'
      named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      holds name targets' pieces' = not (null (named name)) && all ($ evidence targets' pieces') (named name)
      replaced target address word = either (const j2Targets) id
        (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] j2Targets) (edit (at target address (address + 4)) (U.putWord32 word)))
      withScratch scratch' = [piece {pieceFacts = case pieceFacts piece of
          Record2LinkFacts entry bindings' image _ frame -> Record2LinkFacts entry bindings' image (ScratchBytes scratch') frame
          other -> other} | piece <- j2Pieces]
      j2Checks = [ "prefix-exit-enters-orchestration", "orchestration-entry-pushes-the-caller-frame", "published-initializer-entry"
                 , "initializer-scratch-in-window", "first-stage-zeroes-the-completion-cell" ]
  ok "every J2 host check holds on the J2 composition" (all (\name -> holds name j2Targets j2Pieces) j2Checks)
  ok "a loader-B header naming the stock entry fails the prefix exit check"
    (not (holds "prefix-exit-enters-orchestration" (replaced LoaderB 0xd000c 0xd0020) j2Pieces))
  ok "a loader-B startup without the stock frame push fails the frame-push check"
    (not (holds "orchestration-entry-pushes-the-caller-frame" (replaced LoaderB 0xd09ac 0xe92d4810) j2Pieces))
  ok "a stock initializer entry word fails the published-initializer check"
    (not (holds "published-initializer-entry" (replaced LoaderB 0xd0430 0xe92d0830) j2Pieces))
  ok "initializer scratch reaching below the B stack window fails"
    (not (holds "initializer-scratch-in-window" j2Targets (withScratch 0x800)))
  ok "a record 1 that no longer holds the first stage fails the completion-zero check"
    (not (holds "first-stage-zeroes-the-completion-cell" (replaced Record1 0x6000 0) j2Pieces))
  (_, j2Observed) <- stageComposition tools scratch source j2
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4] $ \target ->
    ok ("J2 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf j2Observed target)) == editSpansOf target [first, pair, publisher, orchestration])

  -- J3: loader orchestration A and B, dispatcher and installer as one piece.
  loaderStartupImages <- LoaderStartupImages <$> linked orchestrationABin orchestrationASymbols <*> linked orchestrationBin orchestrationSymbols
    <*> linked dispatcherBin dispatcherSymbols <*> linked installerBin installerSymbols
  startup <- either (H.failH . show) pure (derivePieceOf source loaderStartupImages)
  ok "loader-startup edits loaders A and B and image 4 inside its four carriers"
    (editTargets startup == Set.fromList [LoaderA, LoaderB, Image4]
      && pieceCarriers startup == [at LoaderA 0x409ac 0x40b00, at LoaderB 0xd09ac 0xd0b00, at Image4 0x69cdf8 0x69cf84, at Image4 0x725a04 0x725b54]
      && editsInsideCarriers startup && footprintOutsideCarriers startup)
  ok "loader-startup and loader-B orchestration refuse on the shared area"
    (case composeOf [pair, startup, orchestration] of
      Left (SharedArea _ _ LoaderOrchestrationBArea) -> True
      _ -> False)
  ok "without joins, the publisher and loader-startup refuse on loader A"
    (case composeUnjoined [publisher, startup] of
      Left (FootprintOverlap _ _ range) -> spanTarget range == LoaderA
      _ -> False)
  ok "without joins, the record-2 link and loader-startup refuse"
    (case composeUnjoined [pair, startup] of
      Left (FootprintOverlap _ _ _) -> True
      _ -> False)
  j3 <- either (H.failH . show) pure (composeOf [first, pair, publisher, startup])
  let appliedJ3 name = [spans | AppliedJoin join spans <- compositionJoins j3, joinName (joinKind join) == name]
      j3Catalog kind = [pieceCatalog piece | piece <- compositionPieces j3, pieceKind piece == kind]
      j3Names = [ "first-stage-secondary-rendezvous", "critical-pair-publisher-bindings", "prefix-orchestration-b", "first-stage-completion-zero"
                , "loader-startup-publisher", "publisher-installer-roots", "critical-pair-installer-roots", "dispatcher-critical-pair" ]
  forM_ j3Names $ \name -> putStrLn ("J3 " ++ T.unpack name ++ ": " ++ show (appliedJ3 name))
  putStrLn ("J3 loader-startup footprint: " ++ show [range | Footprint range _ _ <- pieceFootprint startup])
  ok "J3 composes pending the observation-qualified joins"
    (compositionQualification j3 == Pending [ "first-stage-secondary-rendezvous", "prefix-orchestration-b", "first-stage-completion-zero"
                                            , "loader-startup-publisher", "dispatcher-critical-pair" ])
  ok "every J3 join applies once" (all (\name -> length (appliedJ3 name) == 1) j3Names)
  forM_ [LoaderStartupPublisher, PublisherInstallerRoots, CriticalPairInstallerRoots, DispatcherCriticalPair] $ \kind ->
    let without = [join | join <- registeredJoins, joinKind join /= kind]
        bounds = concat [joinBounds join | join <- registeredJoins, joinKind join == kind]
    in ok ("without " ++ T.unpack (joinName kind) ++ ", J3 refuses on a span inside its bounds")
      (case compose without source [first, pair, publisher, startup] of
        Left (FootprintOverlap _ _ range) -> coveredBy range bounds
        _ -> False)
  ok "the installer root joins cover the root-pair words"
    (appliedJ3 "publisher-installer-roots" == [[at Image4 0x725a04 0x725a34]]
      && appliedJ3 "critical-pair-installer-roots" == [[at Image4 0x725a0c 0x725a34]])
  ok "dispatcher-critical-pair covers the image-4 pair and loader-A orchestration"
    (case appliedJ3 "dispatcher-critical-pair" of
      [spans] -> any ((== Image4) . spanTarget) spans && any ((== LoaderA) . spanTarget) spans
        && all (\range -> coveredBy range [at Image4 0x7235d8 0x723790, at LoaderA 0x409ac 0x40b00]) spans
      _ -> False)
  ok "the publisher's caller returns and table reads yield to loader-startup"
    (map (map (labelText . boundaryLabel) . catalogBoundaries) (j3Catalog PrimaryPublisher) == [["primary-publisher/entry-core0"]]
      && map catalogReads (j3Catalog PrimaryPublisher) == [[]])
  let j3Targets = compositionTargets j3
      j3Pieces = compositionPieces j3
      holdsIn name targets' pieces' = not (null (named name)) && all ($ evidence targets' pieces') (named name)
      replacedIn base target address word = either (const base) id
        (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] base) (edit (at target address (address + 4)) (U.putWord32 word)))
      withFacts change = [piece {pieceFacts = change (pieceFacts piece)} | piece <- j3Pieces]
      j3Checks = j2Checks ++
        [ "orchestration-a-calls-publisher-entry", "loader-a-entry-stack-unchanged", "publisher-caller-sp-is-orchestration-frame"
        , "loader-headers-unchanged", "submission-depth-in-stack-window" ]
  initializerCallA <- case [composed | Boundary label' _ _ (Pc composed) _ _ _ _ _ <- concatMap catalogBoundaries (j3Catalog LoaderStartup)
                                     , labelText label' == "loader-startup/a-initializer-call"] of
    [site] -> pure site
    _ -> H.failH "loader-startup lacks its loader-A initializer call"
  ok "every J3 host check holds on the J3 composition" (all (\name -> holdsIn name j3Targets j3Pieces) j3Checks)
  ok "a loader-A initializer call to 0x40434 fails the publisher-entry check"
    (let moved = replacedIn j3Targets LoaderA initializerCallA (0xeb000000 + (((0x40434 - initializerCallA - 8) `div` 4) `mod` 0x1000000))
     in not (holdsIn "orchestration-a-calls-publisher-entry" moved j3Pieces))
  ok "a changed loader-A stack load fails the entry-stack and caller-SP checks"
    (let moved = replacedIn j3Targets LoaderA 0x400a8 0xe59f4000
     in not (holdsIn "loader-a-entry-stack-unchanged" moved j3Pieces) && not (holdsIn "publisher-caller-sp-is-orchestration-frame" moved j3Pieces))
  ok "a publisher caller SP other than the orchestration frame fails"
    (not (holdsIn "publisher-caller-sp-is-orchestration-frame" j3Targets (withFacts (\facts -> case facts of
      PublisherFacts (PublisherFrame entry _) rows -> PublisherFacts (PublisherFrame entry (Guest 0x40063440)) rows
      other -> other))))
  ok "a changed loader-A header fails the installer root check"
    (not (holdsIn "loader-headers-unchanged" (replacedIn j3Targets LoaderA 0x40010 0x4004dae4) j3Pieces))
  ok "a task submission below the core-1 window fails the submission-depth check"
    (not (holdsIn "submission-depth-in-stack-window" j3Targets (withFacts (\facts -> case facts of
      LoaderStartupFacts a b calls starts -> LoaderStartupFacts a b (SubmitCall Core1 (StackDepth 0x800) : calls) starts
      other -> other))))
  ok "a critical-pair frame reaching below the boot-stack window fails the submission-depth check"
    (not (holdsIn "submission-depth-in-stack-window" j3Targets (withFacts (\facts -> case facts of
      Record2LinkFacts entry bindings' image scratch' _ -> Record2LinkFacts entry bindings' image scratch' (StackDepth 0x800)
      other -> other))))
  attachLabel <- either (H.failH . show) pure (label PrimaryPublisher "entry-core0")
  let j3Sources targets' = AttachSources (canonicalDat source) (canonicalDat source) targets targets' [] j3Pieces attachLabel
      registerJson :: Text -> J.Value -> J.Value -> J.Value
      registerJson name retained composed = J.object ["core" J..= (0 :: Int), "register" J..= name, "mask" J..= (0xffffffff :: Word32)
        , "retained" J..= retained, "composed" J..= composed, "reason" J..= ("r" :: Text)]
      registersOf targets' entries = do
        rules <- parseAttachRules (J.object [ "peers" J..= ([] :: [J.Value]), "carriers" J..= ([] :: [J.Value]), "allowances" J..= ([] :: [J.Value])
                                            , "registers" J..= entries, "board" J..= ([] :: [J.Value]), "relations" J..= J.object [] ])
        value <- attachStateValue (j3Sources targets') rules []
        case value of
          J.Object fields | Just (J.Array rows) <- KeyMap.lookup "registers" fields ->
            pure [(KeyMap.lookup "retained" row, KeyMap.lookup "composed" row) | J.Object row <- V.toList rows]
          _ -> pure []
      word :: Word32 -> J.Value
      word = J.toJSON
      rowOf name = J.object ["row" J..= (name :: Text)]
      callReturn = J.object ["call_return" J..= ("loader-startup/a-initializer-call" :: Text)]
      entryState = J.object ["entry_state" J..= J.object ["side" J..= ("loader-a" :: Text), "service" J..= ("private-initialize" :: Text)]]
      derived = [ registerJson "r5" (rowOf "loader-startup/a-entry") entryState, registerJson "r6" (rowOf "loader-startup/a-entry") entryState
                , registerJson "lr" callReturn entryState ]
  ok "the J3 attach r5, r6 and lr derive from the a-entry row, the stock initializer call and loader-A's PrivateInitialize entry state"
    (registersOf j3Targets derived == Right
      [ (Just (rowOf "loader-startup/a-entry"), Just (word 0)), (Just (rowOf "loader-startup/a-entry"), Just (word 0x400a00c4))
      , (Just (word 0x409e0), Just (word 0x409ec)) ])
  ok "the composed initializer call's return agrees with the admitted lr"
    (registersOf j3Targets [registerJson "lr" callReturn callReturn] == Right [(Just (word 0x409e0), Just (word 0x409ec))])
  ok "a retained call_return at a site that does not BL to the attach PC refuses"
    (either (const True) (const False) (registersOf j3Targets [registerJson "lr" (J.object ["call_return" J..= ("loader-startup/a-entry" :: Text)]) entryState]))
  ok "an entry state whose service call no longer BLs to the attach PC refuses"
    (let moved = replacedIn j3Targets LoaderA initializerCallA (0xeb000000 + (((0x40434 - initializerCallA - 8) `div` 4) `mod` 0x1000000))
     in either (const True) (const False) (registersOf moved [registerJson "r6" (rowOf "loader-startup/a-entry") entryState]))
  (j3Dat, j3Observed) <- stageComposition tools scratch source j3
  forM_ [Record1, Record2, LoaderA, LoaderB, Image4] $ \target ->
    ok ("J3 composed " ++ show target ++ " differs from canonical exactly at the edits")
      (normalizeSpans (diffSpans target (targetOf targets target) (targetOf j3Observed target)) == editSpansOf target [first, pair, publisher, startup])
  j3Parts <- either H.failH (pure . U.parsedParts) (U.parseContainer j3Dat)
  j3Section <- either (H.failH . show) pure $ do
    retainedImages <- RoleImages targets <$> plainImage (U.parsedParts (canonicalParsed source))
    composedImages <- RoleImages j3Observed <$> plainImage j3Parts
    application (contractCarriers j3) retainedImages composedImages
  U.MediaImage canonicalPlain <- either H.failH pure (U.extractPlain (U.parsedParts (canonicalParsed source)))
  let canonicalWord (Pc pc)
        | pc >= 0x1100000 = U.word32At canonicalPlain (fromIntegral (pc - 0x1100000))
        | pc >= targetBase Image5 = U.word32At (targetOf targets Image5) (fromIntegral (pc - targetBase Image5))
        | otherwise = U.word32At (targetOf targets Image4) (fromIntegral (pc - targetBase Image4))
  ok "on J3 every application site and queue site holds the canonical word in both roles"
    (length (applicationSites j3Section) == 56 && length (applicationQueueSites j3Section) == 9
      && all (\site -> siteRetainedWord site == CodeWord (canonicalWord (sitePc site)) && siteComposedWord site == siteRetainedWord site)
           (applicationSites j3Section)
      && all (\site -> queueWord site == CodeWord (canonicalWord (queuePc site))) (applicationQueueSites j3Section))
  pumpDigest <- Sha256Hex <$> sha256Text (B.take 0x20 (B.drop (0xbfbd20 - 0xbf1a00) (targetOf targets Image5)))
  ok "on J3 the pump code digest is the canonical one in both roles"
    ([(digestRetained code, digestComposed code) | code <- applicationCode j3Section, digestRegion code == PumpCode] == [(pumpDigest, pumpDigest)])

  allThree <- either (H.failH . show) pure (composeOf [first, pair, publisher])
  H.withTemporaryAt (Tools.scratchDirectoryPath scratch) "startup-canonical-" $ \root -> do
    -- {FirstStage, Record2Link, PrimaryPublisher} end to end through the stager.
    let publisherCarrier = at LoaderA 0x40430 0x409ac
        part (_, range@(Span target (Guest start) (Guest end)))
          | range == publisherCarrier = J.object ["target" J..= targetName target, "start" J..= start, "end" J..= end, "residency" J..= ("resident" :: Text)]
          | otherwise = J.object ["target" J..= targetName target, "start" J..= start, "end" J..= end, "residency" J..= ("absent" :: Text), "reason" J..= ("not asserted by this test" :: Text)]
        interval start end = J.object ["start" J..= (start :: Int), "end" J..= (end :: Int)]
        attachState = J.object
          [ "peers" J..= [J.object ["core" J..= (1 :: Int), "retained_pc" J..= [interval 0xd0000 0xddaa0], "composed_pc" J..= [interval 0x10400 0x110e0]
                                   , "reason" J..= ("core 1 waits for the loader-A completion" :: Text)]]
          , "carriers" J..= map part (contractCarriers allThree)
          , "allowances" J..= ([] :: [J.Value]), "registers" J..= ([] :: [J.Value]), "board" J..= ([] :: [J.Value])
          , "relations" J..= J.object
              [ "timer" J..= J.object ["citation" J..= ("timer.rs" :: Text)]
              , "log" J..= J.object ["span" J..= interval 0xa0040 0xa00c8, "sites" J..= logSites]
              , "lz" J..= J.object ["citation" J..= ("lz.rs" :: Text)] ] ]
        logSite name role coreNumber from = J.object
          [ "label" J..= (name :: Text), "role" J..= (role :: Text), "core" J..= (coreNumber :: Int), "source" J..= from
          , "citation" J..= ("boot-40048.dis; entry.S" :: Text) ]
        stock pc = J.object ["stock" J..= (pc :: Int)]
        owned kind = J.object ["owned" J..= (kind :: Text)]
        logSites =
          [ logSite "stock-record1-channel0" "retained" 0 (stock 0x40f4), logSite "stock-record1-channel1" "retained" 1 (stock 0x413c)
          , logSite "stock-record2-channel0" "retained" 0 (stock 0x134f4), logSite "stock-record2-channel1" "retained" 1 (stock 0x1353c)
          , logSite "composed-stock-record1" "composed" 0 (stock 0x40f4)
          , logSite "first-stage-core0" "composed" 0 (owned "first-stage"), logSite "first-stage-core1" "composed" 1 (owned "first-stage")
          , logSite "record2-core0" "composed" 0 (owned "record2-link"), logSite "record2-core1" "composed" 1 (owned "record2-link") ]
        manifest = J.object
          [ "schema" J..= (4 :: Int)
          , "guard" J..= J.object ["attach" J..= ("primary-publisher/entry-core0" :: Text), "attach_state" J..= attachState]
          , "pieces" J..= [ J.object ["kind" J..= ("first-stage" :: Text), "inputs" J..= J.object ["image" J..= firstBin, "symbols" J..= firstSymbols]]
                          , J.object ["kind" J..= ("record2-link" :: Text), "inputs" J..= J.object ["image" J..= pairBin, "symbols" J..= pairSymbols]]
                          , J.object ["kind" J..= ("primary-publisher" :: Text), "inputs" J..= J.object ["image" J..= publisherBin, "symbols" J..= publisherSymbols]] ]
          ]
        output = root </> "composed"
    L.writeFile (root </> "manifest.json") (J.encode manifest)
    runStager tools datPath (root </> "manifest.json") (Tools.scratchDirectoryPath scratch) output
    composedDat <- H.readRegular (64*1024*1024) (output </> "composed.DAT")
    let reparsed = U.parseContainer composedDat
    ok "the composed DAT reparses with every container check passing"
      (either (const False) (\parsed -> all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed)) reparsed)
    files <- mapM (\(target, name) -> (,) target <$> H.readRegular (16*1024*1024) (output </> name))
      [(Record1, "record1.bin"), (Record2, "record2.bin"), (LoaderA, "loader-a.bin"), (LoaderB, "loader-b.bin"), (Image4, "image4.bin"), (Image5, "image5.bin")]
    composedParts <- either H.failH (pure . U.parsedParts) reparsed
    extracted <- either (H.failH . show) pure (extractPreTable composedParts)
    ok "target files are the composed DAT's uncomplemented targets"
      (and [M.lookup target extracted == Just bytes | (target, bytes) <- files, target `elem` preTableTargets])
    forM_ files $ \(target, bytes) ->
      ok ("composed " ++ show target ++ " differs from canonical exactly at the edits")
        (normalizeSpans (diffSpans target (targetOf targets target) bytes) == editSpansOf target [first, pair, publisher])
    ok "first-stage edit bytes are stored complemented in record slot 1"
      (and [ B.take (spanLength (editSpan change)) (B.drop (0x40000 + fromIntegral (let Guest s = spanStart (editSpan change) in s)) (U.partPreTable composedParts))
               == B.map complement (editBytes change) | change <- pieceEdits first ])
    contract <- B.readFile (output </> "composed-contract.json") >>= maybe (H.failH "contract is not JSON") pure . J.decodeStrict
    composedDigest <- sha256Text composedDat
    let field name value = case value of
          J.Object fields -> KeyMap.lookup name fields
          _ -> Nothing
        kinds = case field "pieces" contract of
          Just (J.Array rows) -> [kind | row <- V.toList rows, Just (J.String kind) <- [field "kind" row]]
          _ -> []
        joinRows = case field "joins" contract of
          Just (J.Array rows) -> [(kind, basis) | row <- V.toList rows, Just (J.String kind) <- [field "kind" row], Just (J.String basis) <- [field "basis" row]]
          _ -> []
    fixture <- B.readFile fixturePath >>= maybe (H.failH "fixture is not JSON") pure . J.decodeStrict
    ok "the contract is pending on the rendezvous over all three pieces"
      (field "qualification" contract == Just (J.String "pending")
        && field "pending_joins" contract == Just (J.toJSON ["first-stage-secondary-rendezvous" :: Text])
        && kinds == ["first-stage", "record2-link", "primary-publisher"])
    ok "the contract lists both joins with their bases"
      (joinRows == [("first-stage-secondary-rendezvous", "observation-qualified"), ("critical-pair-publisher-bindings", "host-proof")])
    ok "the contract has exactly the fixture's keys outside the relations" (contractKeys contract == contractKeys fixture)
    ok "the contract names the composed DAT" ((field "composed" contract >>= field "dat_sha256") == Just (J.String composedDigest))
    let boundaries = case field "pieces" contract of
          Just (J.Array rows) -> [boundary | row <- V.toList rows, Just (J.Array items) <- [field "catalog" row >>= field "boundaries"], boundary <- V.toList items]
          _ -> []
    ok "the guard attach boundary stops"
      ((field "guard" contract >>= field "attach") == Just (J.String "primary-publisher/entry-core0")
        && [field "stop" boundary | boundary <- boundaries, field "label" boundary == Just (J.String "primary-publisher/entry-core0")] == [Just (J.Bool True)])
    let pieceRow name = case field "pieces" contract of
          Just (J.Array rows) -> [row | row <- V.toList rows, field "kind" row == Just (J.String name)]
          _ -> []
        writesOf name = [writes | row <- pieceRow name, Just writes <- [field "catalog" row >>= field "writes"]]
        allowOf name = [(field "target" range, field "core" range, field "start" range, field "end" range)
                       | writes <- writesOf name, Just (J.Array ranges) <- [field "allow" writes], range <- V.toList ranges]
        number :: Int -> Maybe J.Value
        number = Just . J.toJSON
        physicalOn core start end = (Just (J.String "physical"), core, number start, number end)
    ok "the record-2 link enumerates its destinations per core, pre-boundary core-1 stores included"
      ([field "basis" writes | writes <- writesOf "record2-link"] == [Just (J.String "premised")]
        && all (`elem` allowOf "record2-link")
          [ physicalOn (Just J.Null) 0xa0f824 0xa0f828, physicalOn (Just J.Null) 0xa10200 0xa5b200
          , physicalOn (number 0) 0x62c54 0x63460, physicalOn (number 1) 0xddaa0 0xf3420
          , (Just (J.String "tcm"), number 0, number 0, number 0x2000), (Just (J.String "tcm"), number 1, number 0, number 0x2000)
          , physicalOn (number 1) 0x14c60 0x14c64, physicalOn (number 1) 0x14c80 0x14ca0, physicalOn (number 1) 0x20004 0x20008
          , physicalOn (number 1) 0xa0080 0xa00c4, physicalOn (number 1) 0x3fff0024 0x3fff0040, physicalOn (number 1) 0xfffb383c 0xfffb3840 ]
        && length (allowOf "record2-link") == 28)
    ok "the first stage's writes are premised and enumerated on core 0"
      ([field "basis" writes | writes <- writesOf "first-stage"] == [Just (J.String "premised")]
        && length (allowOf "first-stage") == 14
        && all (\(_, core, _, _) -> core == number 0) (allowOf "first-stage"))
    let declaredOf role = case field "guard" contract >>= field "declared" >>= field role of
          Just (J.Array rows) -> [(field "kind" row, field "core" row, field "writer" row) | row <- V.toList rows]
          _ -> []
        tcmAt core writer = (Just (J.String "tcm-region"), number core, number writer)
        romCommit = (Just (J.String "decoder-commit"), number 0, number 0x800022a8)
    ok "the retained role declares the ROM commit and both stock ATCM writes"
      (declaredOf "retained" == [romCommit, tcmAt 0 0x18c4, tcmAt 1 0x118c4])
    ok "the composed role declares the first stage's own ATCM write"
      (declaredOf "composed" == [romCommit, tcmAt 0 0x605c, tcmAt 1 0x118c4])
    let wordOf target address = U.word32At (targetOf targets target) (fromIntegral (address - targetBase target))
    ok "the stock ATCM declarations name each record's mcr p15,0,r1,c9,c1,1 after its literal 0x70000000"
      (and [ wordOf target writer == 0xee091f31 && wordOf target literal == 0x70000000
           | (target, writer, literal) <- [(Record1, 0x18c4, 0x1acc), (Record2, 0x118c4, 0x11acc)] ])
    composedLoaderA <- H.readRegular (16*1024*1024) (output </> "loader-a.bin")
    canonicalDigest <- sha256Text (B.take 0x57c (B.drop 0x430 (targetOf targets LoaderA)))
    composedCarrierDigest <- sha256Text (B.take 0x57c (B.drop 0x430 composedLoaderA))
    let carrierRows = case field "guard" contract >>= field "attach_state" >>= field "carriers" of
          Just (J.Array rows) -> V.toList rows
          _ -> []
        publisherParts = [part' | row <- carrierRows, field "piece" row == Just (J.String "primary-publisher")
                                , Just (J.Array parts) <- [field "parts" row], part' <- V.toList parts]
    ok "a resident carrier part carries the canonical and composed digests of its bytes"
      (map (field "canonical_sha256") publisherParts == [Just (J.String canonicalDigest)]
        && map (field "composed_sha256") publisherParts == [Just (J.String composedCarrierDigest)]
        && canonicalDigest /= composedCarrierDigest)
    let sites = case field "guard" contract >>= field "attach_state" >>= field "relations" >>= field "log" >>= field "sites" of
          Just (J.Array rows) -> [(field "label" row, field "read_pc" row, field "store_pc" row) | row <- V.toList rows]
          _ -> []
        pcOf (_, Just (J.Number read'), Just (J.Number store)) = Just (read', store)
        pcOf _ = Nothing
    ok "every log site resolves to a word load and the store after it in its role's image"
      (length sites == 9 && all (maybe False (\(read', store) -> store == read' + 4) . pcOf) sites
        && [pc | site@(Just (J.String "composed-stock-record1"), _, _) <- sites, Just pc <- [pcOf site]] == [(0x40f4, 0x40f8)])

    -- Record2Link alone stages through the shared image-4 route.
    pairOnly <- either (H.failH . show) pure (composeOf [pair])
    (_, pairObserved) <- stageComposition tools scratch source pairOnly
    ok "record-2 link composes alone through image-4 staging"
      (normalizeSpans (diffSpans Image4 (targetOf targets Image4) (targetOf pairObserved Image4)) == editSpansOf Image4 [pair])

    -- An image-5 edit stages through the shared record-5 route.
    let image5Span = at Image5 0xbfbd20 0xbfbd40
        image5Bytes = B.map complement (B.take 32 (B.drop (0xbfbd20 - 0xbf1a00) (targetOf targets Image5)))
    image5Edit <- maybe (H.failH "image-5 edit length") pure (edit image5Span image5Bytes)
    let image5Piece = Piece PrimaryPublisher (Set.singleton PrimaryInitializerArea) [image5Span] [image5Edit] [] []
          (Catalog [] [] (EnumeratedWrites [] Proved) []) NoFacts
    image5Only <- either (H.failH . show) pure (composeOf [image5Piece])
    (_, image5Observed) <- stageComposition tools scratch source image5Only
    ok "an image-5 edit composes through record-5 staging"
      (normalizeSpans (diffSpans Image5 (targetOf targets Image5) (targetOf image5Observed Image5)) == [image5Span])

  passed <- readIORef counter
  putStrLn ("startup-artifact-canonical-tests: " ++ show passed ++ " checks passed")
