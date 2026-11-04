{-# LANGUAGE OverloadedStrings #-}
-- | Canonical decode, composition staging and the output directory.
module Replacement.StartupArtifact.Stage
  ( loadCanonical, loadPieces, stageComposition, writeOutputs, runStager
  ) where

import Control.Monad (forM, forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (tails)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.Record5Staging (decodeRecord, stageRecord5, stagedRecord5Dat)
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.AttachState (AttachSources (..), attachStateValue)
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Contract
import Replacement.StartupArtifact.Joins (registeredJoins)
import Replacement.StartupArtifact.Manifest
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))

refused :: Either Refusal a -> IO a
refused = either (H.failH . show) pure

canonicalDatSha256 :: Either String F.Sha256
canonicalDatSha256 = F.parseSha256 "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

decodeImages :: Tools.Tools -> Tools.ScratchDirectory -> U.Parts -> IO (B.ByteString, B.ByteString)
decodeImages tools scratch parts = H.withTemporaryAt (Tools.scratchDirectoryPath scratch) "startup-decode-" $ \root ->
  (,) <$> decodeRecord tools root "image4" (U.partRecord4 parts) <*> decodeRecord tools root "image5" (U.partRecord5 parts)

-- | The canonical X-M1 1.50 DAT with its records decoded.
loadCanonical :: Tools.Tools -> Tools.ScratchDirectory -> FilePath -> IO Canonical
loadCanonical tools scratch path = do
  dat <- H.readRegular (64*1024*1024) path
  expected <- either H.failH pure canonicalDatSha256
  observed <- either H.failH pure (F.parseSha256 (SHA.sha256Hex (SHA.sha256 dat)))
  unless (observed == expected) (H.failH (show CanonicalRefused))
  parsed <- refused (either (const (Left CanonicalRefused)) Right (U.parseContainer dat))
  (image4, image5) <- decodeImages tools scratch (U.parsedParts parsed)
  refused (canonical dat image4 image5)

loadPieces :: Canonical -> Manifest -> IO [Piece]
loadPieces source manifest = forM (manifestPieces manifest) $ \piece -> do
  let linked (LinkedPaths imagePath symbolsPath) =
        LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 imagePath) <*> (SymbolBytes <$> H.readRegular 0x10000 symbolsPath)
  images <- case piece of
    SingleImagePiece kind paths -> SingleImage kind <$> linked paths
    LoaderStartupPiece (LoaderStartupPaths a b dispatcher installer) ->
      LoaderStartupImages <$> linked a <*> linked b <*> linked dispatcher <*> linked installer
  refused (derivePieceOf source images)

-- | Rule 4 application, then the rule 5 postconditions on the result.
stageComposition :: Tools.Tools -> Tools.ScratchDirectory -> Canonical -> Composition k -> IO (B.ByteString, TargetBytes)
stageComposition tools scratch source composition = do
  base <- refused (preTableDat source composition)
  let composed = compositionTargets composition
      original = canonicalTargets source
      changed target = targetOf composed target /= targetOf original target
  withImage4 <- if changed Image4
    then Tools.candidateDat <$> Tools.runImage4Staging tools scratch (canonicalDat source) base (targetOf composed Image4)
    else pure base
  final <- if changed Image5
    then stagedRecord5Dat <$> stageRecord5 tools scratch withImage4 (targetOf original Image5) (targetOf composed Image5)
    else pure withImage4
  parsed <- refused (either (const (Left ComposedUnparsed)) Right (U.parseContainer final))
  (image4, image5) <- decodeImages tools scratch (U.parsedParts parsed)
  observed <- refused (verifyComposed source composition final image4 image5)
  pure (final, observed)

-- | A fresh directory under private-data/scratch holding the composed DAT, the
-- six decoded targets and the contract.
writeOutputs :: FilePath -> B.ByteString -> TargetBytes -> J.Value -> IO ()
writeOutputs output dat targets contract = do
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (any ((== ["private-data","scratch"]) . take 2) (tails (splitDirectories parent)))
    "startup artifact output must be under private-data/scratch"
  let directory = parent </> takeFileName output
  exists <- doesPathExist directory
  H.ensure (not exists && takeFileName output `notElem` ["", ".", ".."]) ("refusing existing output: " ++ directory)
  createDirectory directory
  B.writeFile (directory </> "composed.DAT") dat
  forM_ (targetList targets) $ \(target, bytes) -> B.writeFile (directory </> fileName target) bytes
  L.writeFile (directory </> "composed-contract.json") (J.encode contract)
  where
    fileName target = case target of
      Record1 -> "record1.bin"
      Record2 -> "record2.bin"
      LoaderA -> "loader-a.bin"
      LoaderB -> "loader-b.bin"
      Image4 -> "image4.bin"
      Image5 -> "image5.bin"

runStager :: Tools.Tools -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runStager tools canonicalPath manifestPath scratchPath output = do
  scratch <- Tools.validateScratchDirectory scratchPath
  manifestBytes <- H.readRegular (1024*1024) manifestPath
  manifest <- refused (parseManifest manifestBytes)
  source <- loadCanonical tools scratch canonicalPath
  derived <- loadPieces source manifest
  let attach = manifestAttach manifest
  pieces <- refused (attachGuard attach derived)
  composition <- refused (compose registeredJoins source pieces)
  (dat, observed) <- stageComposition tools scratch source composition
  -- Log sites resolve symbols of single-image pieces only.
  ownedSymbols <- concat <$> forM (manifestPieces manifest) (\piece -> case piece of
    SingleImagePiece kind (LinkedPaths _ symbolsPath) -> (\symbols -> [(kind, symbols)]) <$> H.readRegular 0x10000 symbolsPath
    LoaderStartupPiece _ -> pure [])
  let sources = AttachSources (canonicalDat source) dat (canonicalTargets source) observed ownedSymbols
  attachState <- refused (attachStateValue sources (manifestAttachRules manifest) (contractCarriers composition))
  writeOutputs output dat observed (contractValue source manifestBytes composition dat observed attach attachState)
