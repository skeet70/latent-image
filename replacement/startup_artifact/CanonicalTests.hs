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
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins (JoinKind, registeredJoins)
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

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, fixturePath, datPath, scratchPath) <- case arguments of
    [encoder, rebuild, decoder, firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, fixture, dat, scratch] ->
      pure (encoder, rebuild, decoder, [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols], fixture, dat, scratch)
    _ -> H.failH "usage: startup-artifact-canonical-tests ENCODER REBUILD DECODER FIRST_BIN FIRST_SYMBOLS PAIR_BIN PAIR_SYMBOLS PUBLISHER_BIN PUBLISHER_SYMBOLS CONTRACT_FIXTURE CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols] <- pure resolved
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
  ok "record-2 link carries both loader-B words and the image-4 pair"
    (all (`coveredBy` pieceCarriers pair) [at LoaderB 0xd000c 0xd0010, at LoaderB 0xd0aac 0xd0ab0, at Image4 0x7235d8 0x723790])
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

  H.withTemporaryAt (Tools.scratchDirectoryPath scratch) "startup-canonical-" $ \root -> do
    -- {FirstStage, Record2Link, PrimaryPublisher} end to end through the stager.
    let manifest = J.object
          [ "schema" J..= (1 :: Int)
          , "guard" J..= J.object ["attach" J..= ("primary-publisher/entry-core0" :: Text)]
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
    ok "the contract has exactly the fixture's keys" (keyPaths contract == keyPaths fixture)
    ok "the contract names the composed DAT" ((field "composed" contract >>= field "dat_sha256") == Just (J.String composedDigest))
    let boundaries = case field "pieces" contract of
          Just (J.Array rows) -> [boundary | row <- V.toList rows, Just (J.Array items) <- [field "catalog" row >>= field "boundaries"], boundary <- V.toList items]
          _ -> []
    ok "the guard attach boundary stops"
      ((field "guard" contract >>= field "attach") == Just (J.String "primary-publisher/entry-core0")
        && [field "stop" boundary | boundary <- boundaries, field "label" boundary == Just (J.String "primary-publisher/entry-core0")] == [Just (J.Bool True)])

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
