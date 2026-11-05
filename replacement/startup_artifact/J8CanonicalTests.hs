{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J8 composition: the application-pump piece
-- with the J6 pieces, its join with the image5 producer, the split image5
-- reads, composed record-5 staging and the application section's pump words.
module Main where

import Control.Monad (unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Application
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (contractCarriers)
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

-- | An A32 BL from a site to a target.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `div` 4) .&. 0xffffff)

wordAt :: TargetBytes -> Target -> Word32 -> Word32
wordAt targets target address = U.word32At (targetOf targets target) (fromIntegral (address - targetBase target))

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 30 ->
      pure (encoder, rebuild, decoder, take 28 rest, rest !! 28, rest !! 29)
    _ -> H.failH "usage: startup-artifact-j8-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){13} PUMP_BIN PUMP_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols, pumpBin, pumpSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
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
  pump <- single ApplicationPump pumpBin pumpSymbols
  let j3Pieces = [first, pair, publisher, startup]
      j6Pieces = j3Pieces ++ [submission, storage, idle, root, image5, plain]
      j8Pieces = j6Pieces ++ [pump]
      pumpCarrier = at Image5 0xbfbd20 0xbfbd40

  -- The piece.
  ok "application-pump edits only image5 inside [0xbfbd20,0xbfbd40)"
    (pieceCarriers pump == [pumpCarrier] && not (null (pieceEdits pump))
      && all (\change -> spanTarget (editSpan change) == Image5 && coveredBy (editSpan change) [pumpCarrier]) (pieceEdits pump))
  let pumpEdited = either (const targets) id (applyTargetEdits (pieceEdits pump) targets)
  ok "the owned pump differs from stock only in the words at 0xbfbd20, 0xbfbd24 and 0xbfbd3c"
    ([address | address <- [0xbfbd20, 0xbfbd24 .. 0xbfbd3c], wordAt pumpEdited Image5 address /= wordAt targets Image5 address]
      == [0xbfbd20, 0xbfbd24, 0xbfbd3c])
  ok "the pump's call sites keep the stock hook calls"
    (and [ wordAt pumpEdited Image5 site == branchLink site target && wordAt targets Image5 site == branchLink site target
         | (site, target) <- [(0xbfbd28, 0xbfb1b8), (0xbfbd2c, 0xbfb20c), (0xbfbd34, 0xbfb1e0)] ])
  ok "the pump's footprint is task1's call word and the three hook entries"
    (Set.fromList [range | Footprint range _ _ <- pieceFootprint pump]
      == Set.fromList [at Image5 address (address + 4) | address <- [0xc23274, 0xbfb1b8, 0xbfb20c, 0xbfb1e0]])
  let catalog = pieceCatalog pump
      EnumeratedWrites ranges _ = catalogWrites catalog
  ok "the pump's catalog has no boundaries or reads and allows only its frame below task1's SP 0xa4cdd0"
    (null (catalogBoundaries catalog) && null (catalogReads catalog)
      && CodeSpan 0xbfbd20 0xbfbd40 `elem` catalogOwnedCode catalog
      && ranges == [WriteRange PhysicalTarget 0xa4cdc0 0xa4cdd0])

  -- Adapter refusals.
  pumpBytes <- H.readRegular 0x10000 pumpBin
  pumpSymbolBytes <- H.readRegular 0x10000 pumpSymbols
  let pumpWords = [U.word32At pumpBytes offset | offset <- [0, 4 .. B.length pumpBytes - 4]]
      adapted bytes = deriveApplicationPump source (LinkedImage (ImageBytes bytes) (SymbolBytes pumpSymbolBytes))
      refused (Left (AdapterRefused ApplicationPump _)) = True
      refused _ = False
  ok "the linked pump has the stock layout the refusal variant reorders"
    (length pumpWords == 8 && pumpWords !! 2 == branchLink 0xbfbd28 0xbfb1b8)
  ok "a pump whose prehook call moves to 0xbfbd24 refuses"
    (refused (adapted (B.concat (map U.putWord32
      [pumpWords !! 0, branchLink 0xbfbd24 0xbfb1b8, pumpWords !! 1, pumpWords !! 3, pumpWords !! 4, pumpWords !! 5, pumpWords !! 6, pumpWords !! 7]))))
  ok "the stock pump bytes refuse"
    (refused (adapted (B.take 32 (B.drop (0xbfbd20 - 0xbf1a00) (targetOf targets Image5)))))

  -- Composition.
  j8 <- either (H.failH . show) pure (compose registeredJoins source j8Pieces)
  let applied name = [spans | AppliedJoin join spans <- compositionJoins j8, joinName (joinKind join) == name]
  ok "J8 composes pending the J6 joins and application-pump-services"
    (case compositionQualification j8 of
      Pending names -> take 1 (reverse names) == ["application-pump-services"] && length names == 13
      HostComposed -> False)
  ok "application-pump-services applies once over the pump carrier" (applied "application-pump-services" == [[pumpCarrier]])
  ok "without application-pump-services, J8 refuses on the image5 producer's footprint over the pump carrier"
    (case compose [join | join <- registeredJoins, joinKind join /= ApplicationPumpServices] source j8Pieces of
      Left (FootprintOverlap Image5Producer ApplicationPump range) -> coveredBy range [pumpCarrier]
      _ -> False)
  ok "the pump composes with J3 without the image5 producer"
    (either (const False) (const True) (compose registeredJoins source (j3Pieces ++ [pump])))
  ok "J8 contract carriers add the image5 pump carrier"
    (filter ((== ApplicationPump) . fst) (contractCarriers j8) == [(ApplicationPump, pumpCarrier)])
  j6 <- either (H.failH . show) pure (compose registeredJoins source j6Pieces)
  let readsIn composition = [ (labelText (readLabel row), readAddress row, readLength row, readDiffers row)
                            | piece <- compositionPieces composition, pieceKind piece == Image5Producer, row <- catalogReads (pieceCatalog piece) ]
      named name composition = [(address, len, differs) | (label', address, len, differs) <- readsIn composition, label' == name]
  ok "without an image5 edit the producer's destination read is whole and staging is compared"
    (named "image5-producer/destination" j6 == [(0xbf1a00, 0x4f1e60, Nothing)]
      && map (\(_, _, differs) -> differs) (named "image5-producer/staging" j6) == [Nothing])
  ok "with the pump the destination read is split around the carrier and staging differs"
    (named "image5-producer/destination" j8 == []
      && named "image5-producer/destination-bf1a00" j8 == [(0xbf1a00, 0xbfbd20 - 0xbf1a00, Nothing)]
      && named "image5-producer/destination-bfbd40" j8 == [(0xbfbd40, 0x10e3860 - 0xbfbd40, Nothing)]
      && map (\(_, len, _) -> len) (named "image5-producer/staging" j8) == [0x2e0000]
      && all (\(_, _, differs) -> differs /= Nothing) (named "image5-producer/staging" j8))

  -- The pump's footprint guards task1's call word.
  callEdit <- maybe (H.failH "call edit length") pure (edit (at Image5 0xc23274 0xc23278) (U.putWord32 (branchLink 0xc23274 0xbfbd24)))
  let callEditor = plain {pieceCarriers = [at Image5 0xc23274 0xc23278], pieceEdits = [callEdit], pieceFootprint = []}
      withoutPlainJoin = [join | join <- registeredJoins, joinKind join /= PlainProducerServices]
  ok "a piece editing task1's pump call word refuses on the pump's footprint"
    (case compose withoutPlainJoin source (j3Pieces ++ [pump, callEditor]) of
      Left (FootprintOverlap ApplicationPump PlainProducer range) -> range == at Image5 0xc23274 0xc23278
      _ -> False)

  -- Staging and the application section.
  (dat, observed) <- stageComposition tools scratch source j8
  ok "J8 composed image5 differs from canonical exactly at the pump edits"
    (normalizeSpans (diffSpans Image5 (targetOf targets Image5) (targetOf observed Image5)) == normalizeSpans (map editSpan (pieceEdits pump)))
  let changedPump = let offset = 0xbfbd38 - 0xbf1a00 :: Int; bytes = targetOf observed Image5
        in B.take offset bytes <> U.putWord32 0xe1a00000 <> B.drop (offset + 4) bytes
  ok "a decoded composed image5 whose pump carrier is not the linked image refuses after staging"
    (either (== ComposedDiffMismatch Image5) (const False) (verifyComposed source j8 dat (targetOf observed Image4) changedPump))
  composedParts <- either H.failH (pure . U.parsedParts) (U.parseContainer dat)
  section <- either (H.failH . show) pure $ do
    retainedImages <- RoleImages targets <$> plainImage (U.parsedParts (canonicalParsed source))
    composedImages <- RoleImages observed <$> plainImage composedParts
    application (contractCarriers j8) retainedImages composedImages
  let pumpWords' = [ (pc, before, after) | ApplicationSite _ PumpSite (Pc pc) (CodeWord before) (CodeWord after) <- applicationSites section ]
  ok "the section's pump sites carry the stock word retained and the owned word composed"
    (pumpWords' == [ (pc, wordAt targets Image5 pc, wordAt observed Image5 pc) | pc <- [0xbfbd20, 0xbfbd24 .. 0xbfbd3c] ]
      && length [() | (_, before, after) <- pumpWords', before /= after] == 3)
  ok "every other site keeps one word in both roles"
    (and [ before == after | ApplicationSite _ kind _ before after <- applicationSites section, kind /= PumpSite ])
  ok "the pump code digests differ between roles and the task1 digests do not"
    (and [ (digestRetained code == digestComposed code) == (digestRegion code /= PumpCode) | code <- applicationCode section ])

  passed <- readIORef counter
  putStrLn ("startup-artifact-j8-canonical-tests: " ++ show passed ++ " checks passed")
