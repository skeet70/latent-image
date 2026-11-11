{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.Int (Int32)
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Application
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract (contractCarriers)
import Replacement.StartupArtifact.Joins (registeredJoins)
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

wordAt :: TargetBytes -> Target -> Word32 -> Word32
wordAt targets target address = U.word32At (targetOf targets target) (fromIntegral (address - targetBase target))

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 34 ->
      pure (encoder, rebuild, decoder, take 32 rest, rest !! 32, rest !! 33)
    _ -> H.failH "usage: startup-artifact-j9-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){14} PUMP_BIN PUMP_SYMBOLS INIT_BIN INIT_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , _appBin, _appSymbols, image5Bin, image5Symbols, plainBin, plainSymbols, pumpBin, pumpSymbols, initBin, initSymbols] <- pure resolved
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
  initializer <- single Image5Initializer initBin initSymbols
  let j3Pieces = [first, pair, publisher, startup]
      j6Pieces = j3Pieces ++ [submission, storage, idle, root, image5, plain]
      j8Pieces = j6Pieces ++ [pump]
      j9Pieces = j8Pieces ++ [initializer]
      carrier = at Image5 0xd9856c 0xd986b0

  ok "initializer owns exactly its carrier" (pieceCarriers initializer == [carrier])
  ok "initializer has entry, calls and return" (length (catalogBoundaries (pieceCatalog initializer)) == 23)
  ok "all initializer boundaries compare presence" (all ((== ComparePresence) . boundaryCompare) (catalogBoundaries (pieceCatalog initializer)))
  j9 <- derived (compose registeredJoins source j9Pieces)
  (dat, observed) <- stageComposition tools scratch source j9
  ok "both image5 carriers survive staging" (normalizeSpans (diffSpans Image5 (targetOf targets Image5) (targetOf observed Image5)) == normalizeSpans (map editSpan (pieceEdits pump ++ pieceEdits initializer)))
  let calls = [(before,after) | row <- catalogBoundaries (pieceCatalog initializer), boundaryKind row == SiteBoundary
        , Just before <- [boundaryRetainedPc row], before /= Pc 0xd985a4, let after = boundaryComposedPc row]
      branchTarget image pc = let instruction = wordAt image Image5 pc
        in pc+8+fromIntegral ((fromIntegral (instruction .&. 0xffffff) `shiftL` 8 :: Int32) `shiftR` 6)
      paired (Pc before,Pc after) = let old = branchTarget targets before; new = branchTarget observed after
        in if old == 0xd9856c then new >= 0xd985a4 && new < 0xd986b0 else old == new
  ok "catalog call sites pair the stock and owned calls by target" (length calls == 21 && all paired calls)
  ok "the caller return is the sole stopping boundary"
    ([ (boundaryKind row,boundaryComposedPc row,boundaryRetainedPc row) | row <- catalogBoundaries (pieceCatalog initializer), boundaryStop row]
      == [(ReturnBoundary,Pc 0x69d2e4,Just (Pc 0x69d2e4))])
  parts <- either H.failH (pure . U.parsedParts) (U.parseContainer dat)
  section <- derived $ do
    before <- RoleImages targets <$> plainImage (U.parsedParts (canonicalParsed source))
    after <- RoleImages observed <$> plainImage parts
    mapping <- applicationSiteMap (compositionPieces j9)
    application mapping (contractCarriers j9) before after
  let initSites = [row | row <- applicationSites section, siteKind row == InitializerSite
        , let Pc pc = sitePc row, pc >= 0xd9856c && pc < 0xd986b0]
  ok "initializer sites use owned code words" (all (\row -> let Pc pc = siteComposedPc row in siteComposedWord row == CodeWord (wordAt observed Image5 pc)) initSites)
  let follows target row = let Pc pc = siteComposedPc row; call = pc-4
        in wordAt observed Image5 call == 0xeb000000 .|. (((target-call-8) `div` 4) .&. 0xffffff)
  ok "readiness sites follow the owned retained-service calls"
    (and [follows target row | (stock,target) <- [(Pc 0xd985c8,0xed4acc),(Pc 0xd985d8,0xed4afc)], row <- initSites, sitePc row == stock])
  passed <- readIORef counter
  putStrLn ("startup-artifact-j9-canonical-tests: " ++ show passed ++ " checks passed")
