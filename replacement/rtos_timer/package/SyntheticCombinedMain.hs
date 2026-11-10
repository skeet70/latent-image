{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as ELF
import Control.Exception (IOException, try)
import Control.Monad (foldM, forM, forM_)
import Data.Bits (xor)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Either (isLeft)
import qualified Emulation.ModalObservation as Modal
import qualified Emulation.SdkContextAdmission as Sdk
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.RamPush as Ram
import qualified FujiAF.UpdateContainer as U
import qualified RamPushLayout as Layout
import qualified Replacement.PropertyService.Artifact as Property
import qualified Replacement.PropertyService.Classify as Classify
import qualified Replacement.PropertyService.Placement as P
import qualified Replacement.PropertyService.RecipeArtifact as Recipe
import qualified Replacement.RtosTimer.BankPackage as Bank
import qualified Replacement.RtosTimer.CombinedPublication as Combined
import ServiceInventory (Image (..))
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)
import System.IO.Error (ioeSetFileName, modifyIOError)

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

input :: FilePath -> IO B.ByteString
input path = modifyIOError (\problem -> ioeSetFileName problem path) (H.readRegular (64 * 1024 * 1024) path)

checked :: Show e => Either e a -> IO a
checked = either (fail . show) pure

label :: Value -> Value
label value = object
  [ "synthetic_host_source_validation" .= True, "candidate_admitted" .= False
  , "installed_lifetime_admitted" .= False, "total" .= False, "evidence" .= value ]

data SyntheticImages = SyntheticImages B.ByteString B.ByteString B.ByteString B.ByteString deriving Eq

applyTransaction :: SyntheticImages -> [Ram.Publication] -> Combined.PublicationDirection -> Either String SyntheticImages
applyTransaction images rows direction = foldM apply images rows
  where
    apply (SyntheticImages four two five plain) row
      | within 0x61c200 four = (\bytes -> SyntheticImages bytes two five plain) <$> change 0x61c200 four
      | within 0x130000 two = (\bytes -> SyntheticImages four bytes five plain) <$> change 0x130000 two
      | within 0xbf1a00 five = (\bytes -> SyntheticImages four two bytes plain) <$> change 0xbf1a00 five
      | within 0x1100000 plain = SyntheticImages four two five <$> change 0x1100000 plain
      | otherwise = Left "combined publication is outside the actual synthetic source images"
      where
        site = Ram.unAddress (Ram.publicationAddress row)
        within base bytes = site >= base && toInteger site + 4 <= toInteger base + toInteger (B.length bytes)
        change base bytes = Combined.applyCarrierPublication (P.GuestAddress base) bytes [row] direction

newOutput :: FilePath -> IO ()
newOutput path = do
  exists <- doesPathExist path
  H.ensure (not exists) "synthetic output must be a new directory"
  createDirectory path

sourcesAt :: [FilePath] -> IO Bank.RecipeEditSources
sourcesAt paths = case paths of
  [edit, header, storage, arm, fields, ui] -> Bank.RecipeEditSources
    <$> input edit <*> input header <*> input storage <*> input arm <*> input fields <*> input ui
  _ -> fail "six recipe source paths required"

main :: IO ()
main = do
  args <- getArgs
  case args of
    "bind" : preliminary : rest | length rest == 7 -> do
      let paths = take 6 rest
          output = last rest
      sources <- sourcesAt paths
      bank <- Bank.loadRecipeEditPrepublication preliminary sources
      H.ensure (Bank.extentStart (Bank.packageAllocation bank) == Bank.BankAddress 0x02000000)
        "fixture requires declared synthetic base 0x02000000"
      newOutput output
      B.writeFile (output </> "recipe-edit-getter.ld") (Bank.recipeEditLinkerInclude bank)
      L.writeFile (output </> "synthetic-binding.json") (encode (label (Bank.packageIdentityValue bank)))
    "complete" : preliminary : imagePath : image2Path : image5Path : plainPath : canonicalBin : canonicalSymbols : canonicalElf : recipeBin : recipeSymbols : recipeElf : sdkPacket : output : rest
      | length rest >= 6 -> do
      let paths = take 6 rest
          provenancePaths = drop 6 rest
      sources <- sourcesAt paths
      bank <- Bank.loadRecipeEditPrepublication preliminary sources
      H.ensure (Bank.extentStart (Bank.packageAllocation bank) == Bank.BankAddress 0x02000000)
        "fixture requires declared synthetic base 0x02000000"
      rawImage <- input imagePath
      H.ensure (hash rawImage == "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551")
        "synthetic source requires the RAM builder's exact header-bearing RC0 image4"
      let image = B.drop 0xc0 rawImage
      image5 <- input image5Path
      image2 <- input image2Path
      H.ensure (hash image2 == "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5")
        "synthetic source requires the exact RC0 image2"
      plain <- input plainPath
      properties <- checked (Classify.classify (Image "synthetic-source-image4" imagePath 0x61c400 image))
      H.ensure (hash image5 == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
        && hash plain == "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
        "synthetic image5/plain source identity differs"
      let carrier = P.Carrier (P.GuestAddress 0x62c120) (P.GuestAddress 0x62fbfc)
      canonicalBytes <- input canonicalBin
      canonicalNames <- input canonicalSymbols
      canonicalElfBytes <- input canonicalElf
      canonical <- checked (Property.deriveArtifact Property.ProductionArtifact properties carrier (Property.ArtifactInput canonicalBytes canonicalNames canonicalElfBytes))
      recipeBytes <- input recipeBin
      recipeNames <- input recipeSymbols
      recipeElfBytes <- input recipeElf
      recipe <- checked (Recipe.deriveRecipeArtifact bank properties carrier (Property.ArtifactInput recipeBytes recipeNames recipeElfBytes))
      variant <- checked (Recipe.derivePropertyVariantPublication canonical recipe)
      (plan, before) <- checked (Combined.combineSyntheticPublicationPlan image canonical bank variant)
      let rows = Combined.combinedPublications plan
          propertyRows = filter (\row -> let site = Ram.unAddress (Ram.publicationAddress row)
            in site >= 0x62c120 && site < 0x62fbfc) rows
      after <- checked (Combined.applyCarrierPublication (P.GuestAddress 0x61c400) before propertyRows Combined.InstallPublication)
      checked (Combined.verifyCombinedReadback plan Combined.InstallPublication after)
      restored <- checked (Combined.applyCarrierPublication (P.GuestAddress 0x61c400) after propertyRows Combined.RollbackPublication)
      checked (Combined.verifyCombinedReadback plan Combined.RollbackPublication restored)
      H.ensure (restored == before && not (null propertyRows)) "synthetic inverse or nonempty publication proof differs"
      let transactionBefore = SyntheticImages (B.replicate 0x140 0 <> B.take 0xc0 rawImage <> before) image2 image5 plain
      transactionAfter <- checked (applyTransaction transactionBefore rows Combined.InstallPublication)
      transactionRestored <- checked (applyTransaction transactionAfter (reverse rows) Combined.RollbackPublication)
      H.ensure (transactionRestored == transactionBefore) "combined transaction inverse differs from source images"
      let manifestBytes = L.toStrict (encode (Combined.combinedManifestValue plan))
      manifest <- Ram.parseRamPushManifest (output </> "synthetic-manifest.json")
        manifestBytes (Combined.combinedRuntimeBytes plan)
      _ <- checked (Bank.bindRecipeEditPackage manifest (Combined.combinedBankElfBytes plan) sources)
      runtimeSource <- checked (Modal.bindModalRuntimeSource manifest (Combined.combinedBankElfBytes plan))
      sdk <- Sdk.bindCompilerContextSdk sdkPacket
      let sdkInputs = Sdk.compilerContextInputs
      forM_ [ sdkInputs {Sdk.sdkDriverPath = recipeElf}
        , sdkInputs {Sdk.sdkRamPushPath = recipeElf}
        , sdkInputs {Sdk.sdkRunDocumentPath = recipeElf} ] $ \wrongSdk -> do
        result <- try (Sdk.bindContextSdk sdkPacket wrongSdk) :: IO (Either IOException Sdk.ContextSdk)
        H.ensure (isLeft result) "negative control accepted: SDK artifact mismatch"
      wrongQualification <- try (Sdk.bindCompilerContextSdk recipeSymbols) :: IO (Either IOException Sdk.ContextSdk)
      H.ensure (isLeft wrongQualification) "negative control accepted: SDK qualification packet mismatch"
      delta <- case propertyRows of
        first : _ -> pure first
        [] -> fail "property publication delta is empty"
      let reject name result = H.ensure (isLeft result) ("negative control accepted: " ++ name)
          original = Bank.packagePublications bank
          stale = B.take staleOffset before <> U.putWord32 (Ram.unWordValue (Ram.publicationNew delta)) <> B.drop (staleOffset + 4) before
          staleOffset = fromIntegral (Ram.unAddress (Ram.publicationAddress delta) - 0x61c400)
      reject "source mismatch" (Bank.bindRecipeEditPackage manifest (Combined.combinedBankElfBytes plan)
        sources {Bank.editSource = Bank.editSource sources <> B.singleton 0})
      reject "ELF mismatch" (Bank.bindRecipeEditPackage manifest B.empty sources)
      reject "recipe artifact mismatch" (Recipe.deriveRecipeArtifact bank properties carrier (Property.ArtifactInput (B.singleton (B.head recipeBytes `xor` 1) <> B.drop 1 recipeBytes) recipeNames recipeElfBytes))
      reject "publication collision" (Combined.composePublicationVector (Bank.packageEditArms bank) original (propertyRows ++ [delta]))
      reject "publication count" (Combined.composePublicationVector (Bank.packageEditArms bank) (drop 1 original) propertyRows)
      layout <- checked (Layout.propertyPublicationLayout Layout.modalRenderBankVariant)
      reject "capacity overflow" (Layout.propertyPublicationCounts layout (Layout.propertyCarrierWordBound + 1))
      reject "stale preimage" (Combined.applyCarrierPublication (P.GuestAddress 0x61c400) stale propertyRows Combined.InstallPublication)
      reject "inverse preimage" (Combined.applyCarrierPublication (P.GuestAddress 0x61c400) before propertyRows Combined.RollbackPublication)
      reject "readback mismatch" (Combined.verifyCombinedReadback plan Combined.InstallPublication before)
      provenance <- forM ([preliminary, takeDirectory preliminary </> "runtime.bin", takeDirectory preliminary </> "kernel.elf",
        imagePath, image2Path, image5Path, plainPath, canonicalBin, canonicalSymbols, canonicalElf, recipeBin, recipeSymbols, recipeElf] ++ paths ++ provenancePaths) $ \path -> do
          bytes <- input path
          pure (object ["path" .= path, "sha256" .= hash bytes, "bytes" .= B.length bytes])
      elf <- input recipeElf
      parsedElf <- checked (ELF.parseElf elf)
      forM_ [section | section <- ELF.elfSections parsedElf, ELF.sectionAllocated section,
          ELF.sectionType section /= 8, ELF.sectionSize section > 0] $ \section -> do
        let low = ELF.sectionAddress section
            offset = fromIntegral (low - 0x62c120)
            size = fromIntegral (ELF.sectionSize section)
            bytes = B.take size (B.drop (fromIntegral (ELF.sectionOffset section)) elf)
        H.ensure (low >= 0x62c120 && offset + size <= B.length recipeBytes
          && B.length bytes == size && B.take size (B.drop offset recipeBytes) == bytes)
          "recipe ELF differs from admitted linked recipe bytes"
      generatedSources <- forM ["bank.a", "menu-tree.rs", "menu-records.rs", "menu-labels.rs"] $ \name -> do
        bytes <- input (takeDirectory preliminary </> name)
        pure (name, bytes)
      let outputs = [("runtime.bin", Combined.combinedRuntimeBytes plan), ("kernel.elf", Combined.combinedBankElfBytes plan)
            , ("recipe.elf", elf), ("recipe.bin", recipeBytes), ("recipe.symbols", recipeNames)
            , ("publication-table.bin", Combined.combinedTableBytes plan), ("recipe-edit-getter.ld", Bank.recipeEditLinkerInclude bank)
            , ("synthetic-manifest.json", manifestBytes)
            , ("runtime-source.json", L.toStrict (encode (label (object
                ["runtime" .= Modal.modalRuntimeSourceJson runtimeSource, "sdk" .= Sdk.contextSdkValue sdk]))))] ++ generatedSources
      newOutput output
      mapM_ (\(name, bytes) -> B.writeFile (output </> name) bytes) outputs
      L.writeFile (output </> "synthetic-proof.json") (encode (label (object
        [ "kind" .= ("synthetic-host-source-validation" :: String), "declared_synthetic_base" .= ("0x02000000" :: String)
        , "ram_view_profile" .= ("canonical-rc0-padding" :: String)
        , "synthetic_zero_padding" .= object ["base" .= ("0x0061c200" :: String), "bytes" .= (0x140 :: Int)]
        , "inputs" .= provenance, "composition" .= Combined.combinedProofValue plan
        , "before_sha256" .= hash before, "after_sha256" .= hash after, "inverse_sha256" .= hash restored
        , "full_transaction_inverse_exact" .= (transactionRestored == transactionBefore)
        , "negative_controls_passed" .= (["source mismatch", "ELF mismatch", "recipe artifact mismatch", "publication collision", "publication count", "capacity overflow", "stale preimage", "inverse preimage", "readback mismatch", "SDK driver mismatch", "SDK bridge mismatch", "SDK run-document mismatch", "SDK qualification mismatch"] :: [String])
        , "outputs" .= [label (object ["path" .= name, "sha256" .= hash bytes, "bytes" .= B.length bytes]) | (name, bytes) <- outputs]
        , "dat_emitted" .= False, "heap_allocation_witness" .= False, "runtime_executed" .= False ])))
    _ -> fail "usage: synthetic-combined bind PREPUBLICATION EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H NEW_OUTPUT | complete PREPUBLICATION HEADER_BEARING_IMAGE4 IMAGE2 IMAGE5 PLAIN CANONICAL_BIN CANONICAL_SYMBOLS CANONICAL_ELF RECIPE_BIN RECIPE_SYMBOLS RECIPE_ELF SDK_QUALIFICATION NEW_OUTPUT EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H [PROVENANCE_INPUT...]"
