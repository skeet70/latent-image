{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the kernel-slot-bodies piece composed with the J6 pieces.
module Main where

import Control.Monad (forM_, unless)
import Data.Maybe (listToMaybe)
import Numeric (showHex)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (nub)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins (registeredJoins)
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import qualified Replacement.StartupArtifact.KernelSlots as KS
import Replacement.StartupArtifact.Types
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (H.failH ("failed: " ++ name))
  modifyIORef' counter (+ 1)

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
  let linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
      single kind bin symbols = linked bin symbols >>= derived . derivePieceOf source . SingleImage kind
  first <- single FirstStage firstBin firstSymbols
  pair <- single Record2Link pairBin pairSymbols
  publisher <- single PrimaryPublisher publisherBin publisherSymbols
  startupImages <- LoaderStartupImages <$> linked orchestrationABin orchestrationASymbols <*> linked orchestrationBBin orchestrationBSymbols
    <*> linked dispatcherBin dispatcherSymbols <*> linked installerBin installerSymbols
  startup <- derived (derivePieceOf source startupImages)
  orchestrationB <- single LoaderOrchestrationB orchestrationBBin orchestrationBSymbols
  submission <- single TaskSubmission submissionBin submissionSymbols
  storage <- single TaskStorage storageBin storageSymbols
  idle <- single IdleTask idleBin idleSymbols
  root <- single RootTask rootBin rootSymbols
  image5 <- single Image5Producer image5Bin image5Symbols
  plain <- single PlainProducer plainBin plainSymbols
  slotImages <- KernelSlotImages <$> linked slotABin slotASymbols <*> linked slotBBin slotBSymbols <*> (RegionsBytes <$> H.readRegular 0x10000 regionsPath)
  slots <- derived (derivePieceOf source slotImages)
  let j6Pieces = [first, pair, publisher, startup, submission, storage, idle, root, image5, plain]
      others = j6Pieces ++ [orchestrationB]
      conflicts = nub
        [ (pieceKind owner, pieceKind carrier, overlap)
        | (owner, carrier) <- [(other, slots) | other <- others] ++ [(slots, other) | other <- others]
        , Footprint range _ _ <- pieceFootprint owner, held <- pieceCarriers carrier, Just overlap <- [intersectSpan range held] ]
  forM_ conflicts $ \conflict -> putStrLn ("conflict: " ++ show conflict)
  ok "the kernel-slot-bodies carriers are the 34 extents" (length (pieceCarriers slots) > 0)
  let targets = canonicalTargets source
      slotRows = concat [s | KernelSlotFacts s _ <- [pieceFacts slots]]
      entries = [pc | KernelSlot {kernelSlotEntry = Pc pc} <- slotRows]
      offsets = map kernelSlotOffset slotRows
      word address = listToMaybe [w | t <- [minBound .. maxBound], Just w <- [KS.codeWord (KS.CodeImage (targetBase t) (targetOf targets t)) address]]
  forM_ others $ \other -> do
    let spans = [range | Footprint range _ _ <- pieceFootprint other]
        direct = KS.directCalls (targetOf targets) spans (map kernelSlotExtent slotRows)
        indirect = [call | call@(KS.SlotCall _ offset) <- KS.slotCalls (targetOf targets) spans, offset `elem` map kernelSlotOffset slotRows]
        clobbersAt entry = concat [kernelSlotClobbers s | s <- slotRows, kernelSlotEntry s == Pc entry]
        clobbersOf offset = nub (concat [kernelSlotClobbers s | s <- slotRows, kernelSlotOffset s == offset])
        badDirect = [KS.directSite c | c <- direct, KS.directLinked c, not (KS.deadAfter word entries offsets (clobbersAt (KS.directTarget c)) (KS.directSite c + 4))]
        badIndirect = [site | KS.SlotCall site offset <- indirect, not (KS.deadAfter word entries offsets (clobbersOf offset) (site + 4))]
    putStrLn (show (pieceKind other) ++ ": direct " ++ show (length direct) ++ ", slot calls " ++ show (length indirect)
      ++ ", refused " ++ show (map (`showHex` "") (badDirect ++ badIndirect)))
  forM_ slotRows $ \s -> putStrLn ("slot " ++ show (kernelSlotOffset s) ++ " " ++ show (kernelSlotEntry s) ++ " frame " ++ show (kernelSlotFrame s) ++ " clobbers " ++ show (kernelSlotClobbers s))
  _ <- either (H.failH . show) pure (compose registeredJoins source (j6Pieces ++ [slots]))
  passed <- readIORef counter
  putStrLn ("startup-artifact-kernel-slot-canonical-tests: " ++ show passed ++ " checks passed")
