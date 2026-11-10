{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import qualified Replacement.RtosPlatform.Artifact as Platform
import qualified Replacement.RtosPlatform.Union as Union
import Replacement.RtosTimer.RootPackage
import qualified Replacement.StartupArtifact.Manifest as Manifest
import qualified Replacement.StartupArtifact.Stage as Startup
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as Startup
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    encoder : rebuild : decoder : rest -> case splitAt 24 rest of
      (imagePaths, [carriersPath, binA, symbolsA, disasmA, placementsA, binB, symbolsB, disasmB, placementsB, canonicalPath, guardPath, scratchPath, output]) -> do
        paths <- mapM canonicalizePath imagePaths
        guardBytes <- input guardPath
        initialBytes <- either H.failH pure (j5Manifest guardBytes [] paths)
        initialManifest <- either (H.failH . show) pure (Manifest.parseManifest initialBytes)
        tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
        scratch <- Tools.validateScratchDirectory scratchPath
        source <- Startup.loadCanonical tools scratch canonicalPath
        derived <- Startup.loadPieces source initialManifest
        let appCarriers = concat [Startup.pieceCarriers piece | piece <- derived, Startup.pieceKind piece == Startup.AppCallback]
        manifestBytes <- either H.failH pure (j5Manifest guardBytes appCarriers paths)
        manifest <- either (H.failH . show) pure (Manifest.parseManifest manifestBytes)
        (startupDat, observed, startupContract, composition) <- Startup.stageLoadedManifest tools scratch source manifestBytes manifest derived
        union <- either (H.failH . show) pure . Union.parseUnion =<< input carriersPath
        forM_ R.domains $ \domain -> do
          let entries = Union.mapEntries (Union.domainMap union domain)
          H.ensure (length (filter ((== Union.Timer) . Union.entryGroup) entries) == 4 &&
            length (filter ((== Union.Cyclic) . Union.entryGroup) entries) == 8)
            "root platform requires all timer and cyclic ABI entries in both domains"
        aSpans <- placements placementsA
        bSpans <- placements placementsB
        linked <- Platform.Linked <$> input binA <*> input symbolsA <*> input disasmA <*> pure aSpans
          <*> input binB <*> input symbolsB <*> input disasmB <*> pure bSpans
        package <- either H.failH pure (joinRootPlatform source composition startupDat observed union linked)
        effective <- either H.failH pure (packageCarriers package)
        Startup.writeOutputs output (packageDat package) (packageTargets package) (packageContract startupContract startupDat package)
        B.writeFile (output </> "startup-loader-a.bin") (Targets.targetOf (packageStartupTargets package) Startup.LoaderA)
        B.writeFile (output </> "startup-loader-b.bin") (Targets.targetOf (packageStartupTargets package) Startup.LoaderB)
        forM_ effective $ \(domain, bytes) ->
          B.writeFile (output </> ("effective-carrier-" ++ R.domainLabel domain ++ ".bin")) bytes
      _ -> usage
    _ -> usage
  where
    usage = H.failH "usage: stage-root-platform (Bazel supplies tools and linked J5/platform inputs) CANONICAL_DAT J5_GUARD_MANIFEST SCRATCH_DIR NEW_OUTPUT"
    input path = H.readRegular (64 * 1024 * 1024) path
    placements path = do
      rows <- either H.failH pure . Aggregate.parsePlacements =<< input path
      pure [(Aggregate.placementAddress row, fromIntegral (Aggregate.placementBytes row)) | row <- rows]

