{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import Emulation.SchedulerPairPlan (planValue)
import qualified Emulation.Image4Staging as Tools
import qualified Emulation.ModalObservation as Modal
import qualified Emulation.SdkContextAdmission as Sdk
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.RamPush as Ram
import qualified Replacement.PropertyService.Artifact as Property
import qualified Replacement.PropertyService.Placement as Placement
import qualified Replacement.PropertyService.RecipeArtifact as Recipe
import qualified Replacement.RecordStore.Carrier as Record
import qualified Replacement.RecordStore.References as References
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import qualified Replacement.RtosPlatform.Artifact as Platform
import qualified Replacement.RtosPlatform.Union as Union
import qualified Replacement.RtosTaskRecords.System as TaskRecords
import Replacement.RtosTimer.Candidate
import Replacement.RtosTimer.RootPackage
import qualified Replacement.RtosTimer.BankPackage as Bank
import qualified Replacement.RtosTimer.CombinedPublication as Combined
import qualified Replacement.StartupArtifact.Compose as Compose
import qualified Replacement.StartupArtifact.Manifest as Manifest
import qualified Replacement.StartupArtifact.Stage as Startup
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as S
import ServiceInventory (Image (..))
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.Environment (getArgs, getExecutablePath)
import System.FilePath ((</>), takeDirectory, takeFileName)

data InputRole = Composer | Encoder | Rebuilder | Decoder | StartupManifest | PlatformUnion
  | PlatformA | PlatformASymbols | PlatformADisassembly | PlatformAPlacements
  | PlatformB | PlatformBSymbols | PlatformBDisassembly | PlatformBPlacements
  | PropertyImage | PropertySymbols | PropertyElf | RecordImage | RecordSymbols | PlainImage | Image2 | FunctionIndex
  deriving Show

data CompletedRuntime = CompletedRuntime
  { completedPlan :: Combined.CombinedPublicationPlan
  , completedManifest :: Ram.RamPushManifest
  , completedManifestBytes :: B.ByteString
  , completedSdk :: Sdk.ContextSdk
  }

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

shown :: Show e => Either e a -> IO a
shown = either (H.failH . show) pure

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "bind-recipe-edit" : rest -> bindRecipeEdit rest
    _ -> case drop 17 arguments of
      "bind-recipe-edit" : rest -> bindRecipeEdit rest
      _ -> stageCandidate arguments

bindRecipeEdit :: [String] -> IO ()
bindRecipeEdit arguments = case arguments of
  [prepublication, edit, header, storage, arm, fields, ui, output] -> do
    let input path = canonicalizePath path >>= H.readRegular (64 * 1024 * 1024)
    sources <- Bank.RecipeEditSources <$> input edit <*> input header <*> input storage
      <*> input arm <*> input fields <*> input ui
    package <- Bank.loadRecipeEditPrepublication prepublication sources
    scratch <- Tools.validateScratchDirectory (takeDirectory output)
    let name = takeFileName output
        directory = Tools.scratchDirectoryPath scratch </> name
    exists <- doesPathExist directory
    H.ensure (not exists && name `notElem` ["", ".", ".."]) "recipe binding output already exists or is not a new directory"
    createDirectory directory
    B.writeFile (directory </> "recipe-edit-getter.ld") (Bank.recipeEditLinkerInclude package)
    L.writeFile (directory </> "recipe-edit-package.json") (encode (Bank.packageIdentityValue package))
  _ -> H.failH "usage: stage-owned-candidate bind-recipe-edit PREPUBLICATION EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H NEW_OUTPUT"

stageCandidate :: [String] -> IO ()
stageCandidate arguments = do
  let (candidateArguments, recipeArguments) = splitAt 24 arguments
  unless (null recipeArguments || length recipeArguments == 11)
    (H.failH "recipe publication requires PREPUBLICATION RECIPE_BIN RECIPE_SYMBOLS RECIPE_ELF EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H SDK_QUALIFICATION")
  case candidateArguments of
    [encoder, rebuild, decoder, carriersPath, binA, symbolsA, disasmA, placementsA,
     binB, symbolsB, disasmB, placementsB, propertyBin, propertySymbols, propertyElf, recordBin, recordSymbols,
     canonicalPath, manifestPath, plainPath, image2Path, indexPath, scratchPath, output] -> do
      executable <- getExecutablePath
      snapshot <- forM ([executable, encoder, rebuild, decoder, carriersPath, binA, symbolsA, disasmA, placementsA,
        binB, symbolsB, disasmB, placementsB, propertyBin, propertySymbols, propertyElf, recordBin, recordSymbols,
        manifestPath, plainPath, image2Path, indexPath] ++ take 10 recipeArguments) $ \path -> do
          resolved <- canonicalizePath path
          bytes <- H.readRegular (64 * 1024 * 1024) resolved
          pure (path, bytes)
      let input path = maybe (H.failH "candidate input missing from immutable snapshot") pure (lookup path snapshot)
      tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
      scratch <- Tools.validateScratchDirectory scratchPath
      source <- Startup.loadCanonical tools scratch canonicalPath
      manifestBytes <- input manifestPath
      manifest <- shown (Manifest.parseManifest manifestBytes)
      (startupDat, observed, startupContract, composition) <- Startup.stageManifest tools scratch source manifestBytes manifest
      let installedKinds = map S.pieceKind (Compose.compositionPieces composition)
      forM_ [S.FirstStage, S.Record2Link, S.PrimaryPublisher, S.LoaderStartup, S.TaskSubmission,
        S.TaskStorage, S.IdleTask, S.RootTask, S.Image5Producer, S.PlainProducer, S.GeneralHeap,
        S.ApplicationPump, S.Task1Actor, S.Image5Initializer, S.TaskConstructors, S.AppCallback] $ \kind ->
          H.ensure (kind `elem` installedKinds) ("candidate manifest omits implemented startup subsystem: " ++ show kind)
      union <- shown . Union.parseUnion =<< input carriersPath
      forM_ R.domains $ \domain -> do
        let entries = Union.mapEntries (Union.domainMap union domain)
        H.ensure (length (filter ((== Union.Timer) . Union.entryGroup) entries) == 4 &&
          length (filter ((== Union.Cyclic) . Union.entryGroup) entries) == 8)
          "candidate requires timer and cyclic installation in both loader domains"
      aSpans <- placements =<< input placementsA
      bSpans <- placements =<< input placementsB
      linked <- Platform.Linked <$> input binA <*> input symbolsA <*> input disasmA <*> pure aSpans
        <*> input binB <*> input symbolsB <*> input disasmB <*> pure bSpans
      package <- shown (joinRootPlatform source composition startupDat observed union linked)
      let original = Compose.canonicalTargets source
          four = Targets.targetOf original S.Image4
          five = Targets.targetOf original S.Image5
          dat = Compose.canonicalDat source
          target = Targets.targetOf original
      plain <- input plainPath
      image2 <- input image2Path
      propertyBytes <- input propertyBin
      propertySymbolBytes <- input propertySymbols
      propertyElfBytes <- input propertyElf
      propertySource <- shown (Property.canonicalSource dat four (target S.LoaderA) (target S.LoaderB) five plain image2)
      carrier <- maybe (H.failH "property service has no admitted carrier") pure (Placement.hullCarrier (Property.sourceHull propertySource))
      propertyArtifact <- shown (Property.deriveArtifact Property.ProductionArtifact (Property.sourceProperties propertySource) carrier (Property.ArtifactInput propertyBytes propertySymbolBytes propertyElfBytes))
      propertyStage <- shown (Property.stageImage4 propertySource propertyArtifact)
      propertyCandidate <- Property.stageCandidate tools scratch propertyStage
      index <- input indexPath
      H.ensure (hash index == "c1b0cdbfb41d4241cd5250a133d90badece759860e138693d5154e78bda9b3c3") "record function index identity mismatch"
      H.ensure (hash plain == "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d") "record plain source identity mismatch"
      ranges <- shown (References.parseRanges index)
      let images = [(Placement.ScanImage4, Image "image4" canonicalPath 0x61c400 four),
            (Placement.ScanImage5, Image "image5" canonicalPath 0xbf1a00 five),
            (Placement.ScanPlain, Image "plain" plainPath 0x1100000 plain)]
      unless (null (References.resolvedReferences ranges images)) (H.failH "record carrier has retained references")
      unless (null (Record.scanObsoleteGap images)) (H.failH "record obsolete cache gap has retained references")
      let packageTarget = Targets.targetOf (packageTargets package)
          installedImages =
            [ (Placement.ScanImage4, Image "image4" canonicalPath 0x61c400 (packageTarget S.Image4))
            , (Placement.ScanImage5, Image "image5" canonicalPath 0xbf1a00 (packageTarget S.Image5))
            , (Placement.ScanLoaderA, Image "loader-a" canonicalPath 0x40000 (packageTarget S.LoaderA))
            , (Placement.ScanLoaderB, Image "loader-b" canonicalPath 0xd0000 (packageTarget S.LoaderB))
            , (Placement.ScanPlain, Image "plain" plainPath 0x1100000 plain)
            , (Placement.ScanImage2, Image "image2" image2Path 0x130000 image2)
            ]
      packageHull <- shown (Placement.scanHull (Image "image4" canonicalPath 0x61c400 (packageTarget S.Image4))
        installedImages (Property.sourceProperties propertySource))
      unless (null (Placement.referencesInto carrier (Placement.hullHits packageHull)))
        (H.failH "composed startup/platform references retired property carrier")
      unless (null (References.resolvedReferences ranges installedImages) && null (Record.scanObsoleteGap installedImages))
        (H.failH "composed startup/platform references retired record carrier or cache")
      recordBytes <- input recordBin
      recordSymbolBytes <- input recordSymbols
      recordImage <- shown (Record.stageImage four recordBytes recordSymbolBytes)
      propertyChanges <- shown (changedExtents PropertyService four (Property.stagedBytes propertyStage))
      recordChanges <- shown (changedExtents RecordStore four recordImage)
      let reserved = concatMap S.pieceCarriers (Compose.compositionPieces composition)
          changes = propertyChanges ++ recordChanges
          before = Targets.targetOf (packageTargets package) S.Image4
          Placement.Carrier (Placement.GuestAddress propertyLow) (Placement.GuestAddress propertyHigh) = carrier
          propertyCarrier = S.Span S.Image4 (S.Guest propertyLow) (S.Guest propertyHigh)
          recordCarriers = [S.Span S.Image4 (S.Guest low) (S.Guest high) |
            Placement.Carrier (Placement.GuestAddress low) (Placement.GuestAddress high) <- Record.retiredRegions]
      shown (rejectCarrierOverlap reserved (propertyCarrier : recordCarriers))
      shown (rejectCarrierOverlap [propertyCarrier] recordCarriers)
      merged <- shown (mergeImage4 reserved before changes)
      candidatePackage <- shown (admitCandidatePackage package propertyArtifact reserved changes merged)
      combined <- case recipeArguments of
        [] -> pure Nothing
        [prepublication, recipeBin, recipeSymbols, recipeElf, edit, header, storage, arm, fields, ui, sdkPacket] -> do
          sources <- Bank.RecipeEditSources <$> input edit <*> input header <*> input storage
            <*> input arm <*> input fields <*> input ui
          bank <- Bank.loadRecipeEditPrepublication prepublication sources
          recipeBytes <- input recipeBin
          recipeSymbolBytes <- input recipeSymbols
          recipeElfBytes <- input recipeElf
          artifact <- shown (Recipe.deriveRecipeArtifact bank (Property.sourceProperties propertySource)
            carrier (Property.ArtifactInput recipeBytes recipeSymbolBytes recipeElfBytes))
          variant <- shown (Recipe.derivePropertyVariantPublication propertyArtifact artifact)
          plan <- shown (Combined.combinePublicationPlan candidatePackage bank variant)
          let completedBytes = L.toStrict (encode (Combined.combinedManifestValue plan))
          completed <- Ram.parseRamPushManifest (output </> "ram-push-manifest.json")
            completedBytes (Combined.combinedRuntimeBytes plan)
          bound <- shown (Bank.bindRecipeEditPackage completed (Combined.combinedBankElfBytes plan) sources)
          H.ensure (Bank.getterEntry (Bank.getterBinding bound) == Bank.getterEntry (Bank.getterBinding bank)
            && Bank.getterCodeDigest (Bank.getterBinding bound) == Bank.getterCodeDigest (Bank.getterBinding bank)
            && Bank.packageAllocation bound == Bank.packageAllocation bank)
            "completed publication changed its source-bound getter or allocation layout"
          sdk <- Sdk.bindCompilerContextSdk sdkPacket
          pure (Just (CompletedRuntime plan completed completedBytes sdk))
        _ -> H.failH "recipe publication inputs differ from admitted argument shape"
      final <- Tools.runImage4Replacement tools scratch (packageDat package) (packageDat package) (SHA.sha256 before) merged
      finalTargets <- maybe (H.failH "candidate lost a decoded target") pure
        (Targets.targetBytes (M.insert S.Image4 merged (M.fromList (Targets.targetList (packageTargets package)))))
      taskSource <- shown (Platform.canonicalSource dat (target S.LoaderA) (target S.LoaderB))
      (stockTaskPlan, ownedTaskPlan) <- shown
        (TaskRecords.systemPlansForPackaged taskSource (packagePlatform package) (Tools.candidateDat final))
      lifetimeSourceBytes <- forM combined $ \runtime -> do
        lifetimeSource <- shown (Modal.bindModalLifetimeSourceForPackage package (Tools.candidateDat final)
          (completedManifest runtime) (Combined.combinedBankElfBytes (completedPlan runtime)))
        pure (L.toStrict (encode (object
          ["schema" .= (1 :: Int), "lifetime" .= Modal.modalLifetimeSourceJson lifetimeSource
          , "sdk" .= Sdk.contextSdkValue (completedSdk runtime), "installed_lifetime_admitted" .= False, "total" .= False])))
      let stockTaskPlanBytes = L.toStrict (encode (planValue stockTaskPlan))
          ownedTaskPlanBytes = L.toStrict (encode (planValue ownedTaskPlan))
          stockTaskPlanName = "stock-task-record-plan.json"
          ownedTaskPlanName = "owned-task-record-plan.json"
      identities <- forM
        [ (Composer, executable), (Encoder, encoder), (Rebuilder, rebuild), (Decoder, decoder)
        , (StartupManifest, manifestPath), (PlatformUnion, carriersPath)
        , (PlatformA, binA), (PlatformASymbols, symbolsA), (PlatformADisassembly, disasmA), (PlatformAPlacements, placementsA)
        , (PlatformB, binB), (PlatformBSymbols, symbolsB), (PlatformBDisassembly, disasmB), (PlatformBPlacements, placementsB)
        , (PropertyImage, propertyBin), (PropertySymbols, propertySymbols), (PropertyElf, propertyElf), (RecordImage, recordBin), (RecordSymbols, recordSymbols)
        , (PlainImage, plainPath), (Image2, image2Path), (FunctionIndex, indexPath)
        ] $ \(role, path) -> do
          bytes <- input path
          pure (object ["role" .= show role, "sha256" .= hash bytes, "bytes" .= B.length bytes])
      let contract = object
            [ "schema" .= (1 :: Int), "kind" .= ("owned-candidate-composition" :: String)
            , "total" .= False, "source_sha256" .= hash dat
            , "candidate_sha256" .= Tools.candidateSha256 final
            , "candidate_image4_sha256" .= hash (candidateImage4 candidatePackage)
            , "inputs" .= identities
            , "startup_and_platform_before_settings" .= packageContract startupContract startupDat package
            , "property_before_composition" .= Property.candidateContractValue propertyCandidate
            , "record_reference_scan" .= References.referenceSummary ranges images
            , "record_owned_cache" .= Record.cacheContract
            , "combined_publication" .= fmap (Combined.combinedProofValue . completedPlan) combined
            , "modal_lifetime_source" .= fmap (\bytes -> object
                ["file" .= ("modal-lifetime-source.json" :: String), "sha256" .= hash bytes]) lifetimeSourceBytes
            , "settings_changed_extents" .= map changeValue changes
            , "capability_manifest" .= capabilityManifest (map S.pieceKind (Compose.compositionPieces composition))
            , "task_record_plans" .= object
                [ "stock" .= object ["file" .= stockTaskPlanName, "sha256" .= hash stockTaskPlanBytes]
                , "owned" .= object ["file" .= ownedTaskPlanName, "sha256" .= hash ownedTaskPlanBytes]
                ]
            , "runtime_executed" .= False
            , "runtime_requalification_required" .= True
            , "resource_preservation" .= ("Image4 replacement verifies its carried decode and restores the exact input DAT record and padding. All non-image4 container resources remain byte-identical to the admitted startup/platform package." :: String)
            ]
      Startup.writeOutputs output (Tools.candidateDat final) finalTargets contract
      B.writeFile (output </> stockTaskPlanName) stockTaskPlanBytes
      B.writeFile (output </> ownedTaskPlanName) ownedTaskPlanBytes
      forM_ lifetimeSourceBytes (B.writeFile (output </> "modal-lifetime-source.json"))
      forM_ combined $ \runtime -> do
        B.writeFile (output </> "runtime.bin") (Combined.combinedRuntimeBytes (completedPlan runtime))
        B.writeFile (output </> "ram-push-manifest.json") (completedManifestBytes runtime)
        B.writeFile (output </> "kernel.elf") (Combined.combinedBankElfBytes (completedPlan runtime))
    _ -> H.failH "usage: stage-owned-candidate (Bazel supplies tools/platform/property/record inputs) CANONICAL_DAT STARTUP_MANIFEST PLAIN IMAGE2 FUNCTION_INDEX SCRATCH_DIR NEW_OUTPUT [PREPUBLICATION RECIPE_BIN RECIPE_SYMBOLS RECIPE_ELF EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H SDK_QUALIFICATION]"
  where
    placements bytes = do
      rows <- shown (Aggregate.parsePlacements bytes)
      pure [(Aggregate.placementAddress row, fromIntegral (Aggregate.placementBytes row)) | row <- rows]
