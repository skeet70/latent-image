{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosTimer.BankPackage
  ( RecipeEditPackage, RecipeEditSources (..), GetterBinding, BankAddress (..), BankExtent
  , bindRecipeEditPackage, getterBinding, getterEntry, getterExtent, getterCodeDigest
  , extentStart, extentEnd, recipeEditLinkerInclude, packageIdentityValue
  , packageElfBytes, packageRuntimeBytes, packageAllocation
  , loadRecipeEditPrepublication, packagePublications, packageBankVariant, packageEditArms
  , packageEntries, packageSourceIdentity
  ) where

import Control.Monad (forM_, unless)
import qualified ArmElf as ELF
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Data.ByteString.Char8 as C
import qualified Emulation.ModalObservation as Modal
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.RamPush as Ram
import qualified FujiAF.Host as H
import qualified RamPushLayout as Layout
import Numeric (showHex)

newtype BankAddress = BankAddress { bankAddressValue :: Word32 } deriving (Eq, Ord, Show)
data BankExtent = BankExtent { extentStart :: BankAddress, extentEnd :: BankAddress } deriving (Eq, Show)
data GetterBinding = GetterBinding
  { getterEntry :: BankAddress
  , getterExtent :: BankExtent
  , getterCodeDigest :: SHA.Sha256
  }
data RecipeEditSources = RecipeEditSources
  { editSource :: B.ByteString
  , editHeader :: B.ByteString
  , storageSource :: B.ByteString
  , armSource :: B.ByteString
  , fieldsSource :: B.ByteString
  , uiHeader :: B.ByteString
  }
data GetterImplementation = RetainedCRecipeEditGetter deriving (Show)
data RecipeEditPackage = RecipeEditPackage
  { getterBinding :: GetterBinding
  , runtimeBinding :: Maybe Modal.ModalRuntimeSource
  , packageElfDigest :: SHA.Sha256
  , packageRuntimeDigest :: SHA.Sha256
  , packageManifestDigest :: SHA.Sha256
  , packageSources :: [(FilePath, SHA.Sha256)]
  , packageElfBytes :: B.ByteString
  , packageRuntimeBytes :: B.ByteString
  , packageAllocation :: BankExtent
  , packagePublications :: [Ram.Publication]
  , packageBankVariant :: Maybe Layout.BankVariant
  , packageEditArms :: (BankAddress, BankAddress)
  , packageEntries :: Ram.RamPushEntries
  , packageSourceIdentity :: [(String, String)]
  }

bindRecipeEditPackage :: Ram.RamPushManifest -> B.ByteString -> RecipeEditSources -> Either String RecipeEditPackage
bindRecipeEditPackage manifest elfBytes sources = do
  bound <- Modal.bindModalRuntimeSource manifest elfBytes
  bindRecipeEditSource (Just bound) manifest elfBytes sources

loadRecipeEditPrepublication :: FilePath -> RecipeEditSources -> IO RecipeEditPackage
loadRecipeEditPrepublication path sources = do
  (manifest, elfBytes) <- Ram.readRamPushPrepublication path
  either H.failH pure $ do
    variant <- maybe (Left "recipe preliminary package has no bank variant") Right (Ram.manifestBankVariant manifest)
    layout <- Layout.propertyPublicationLayout variant
    elf <- ELF.parseElf elfBytes
    let base = Ram.unAddress (Ram.allocationBase manifest)
        installer = Layout.runtimeInstallerBase base
        runtime = Ram.runtimeBytes manifest
        zeroRange address count = address >= base
          && toInteger address + toInteger count <= toInteger base + toInteger (B.length runtime)
          && B.take count (B.drop (fromIntegral (address - base)) runtime) == B.replicate count 0
    config <- uniqueSymbol elf "ram_push_publication_config"
    table <- uniqueSymbol elf "ram_push_publications"
    unless (ELF.symbolValue config == installer + Layout.propertyPublicationConfigOffset
      && ELF.symbolSize config == 16 && zeroRange (ELF.symbolValue config) 16
      && ELF.symbolValue table == installer + Layout.propertyPublicationTableOffset
      && fromIntegral (ELF.symbolSize table) == Layout.propertyPublicationCapacity layout * 16
      && zeroRange (ELF.symbolValue table) (Layout.propertyPublicationCapacity layout * 16))
      (Left "recipe preliminary installer config/table differ from the zero capacity-sized ELF layout")
    bindRecipeEditSource Nothing manifest elfBytes sources

uniqueSymbol :: ELF.Elf -> String -> Either String ELF.Symbol
uniqueSymbol elf name = case filter ((== name) . ELF.symbolName) (ELF.elfSymbols elf) of
  [symbol] -> Right symbol
  _ -> Left ("recipe edit package requires one ELF symbol: " ++ name)

bindRecipeEditSource :: Maybe Modal.ModalRuntimeSource -> Ram.RamPushManifest -> B.ByteString -> RecipeEditSources -> Either String RecipeEditPackage
bindRecipeEditSource bound manifest elfBytes sources = do
  expectedManifest <- SHA.parseSha256Hex (Ram.manifestDigest manifest)
  let sourceRows =
        [ ("menu/recipe_bank_edit.c", editSource sources)
        , ("menu/recipe_bank_edit.h", editHeader sources)
        , ("menu/recipe_bank_storage.c", storageSource sources)
        , ("menu/recipe_bank_arm.S", armSource sources)
        , ("menu/recipe_bank_fields.inc", fieldsSource sources)
        , ("menu/recipe_bank_ui.h", uiHeader sources)
        ]
  forM_ sourceRows $ \(path, bytes) -> do
    expected <- case [value | (key, value) <- Ram.sourceIdentity manifest, key == path] of
      [value] -> SHA.parseSha256Hex value
      _ -> Left ("recipe edit package requires one source identity: " ++ path)
    unless (not (B.null bytes) && SHA.sha256 bytes == expected)
      (Left ("recipe edit package source identity differs: " ++ path))
  elf <- ELF.parseElf elfBytes
  helper <- case filter ((== "rb_stage_edit_get") . ELF.symbolName) (ELF.elfSymbols elf) of
    [symbol] -> pure symbol
    _ -> Left "recipe edit package requires one rb_stage_edit_get symbol"
  section <- case ELF.symbolSection helper of
    ELF.InSection index | index >= 0 && index < length (ELF.elfSections elf) -> pure (ELF.elfSections elf !! index)
    _ -> Left "recipe edit getter has no allocated section"
  let address = ELF.symbolValue helper
      bytes = ELF.symbolSize helper
      base = Ram.unAddress (Ram.allocationBase manifest)
      size = Ram.allocationSize manifest
      end = toInteger address + toInteger bytes
      sectionEnd = toInteger (ELF.sectionAddress section) + toInteger (ELF.sectionSize section)
  unless (ELF.symbolKind helper == ELF.FunctionSymbol && ELF.sectionAllocated section
    && ELF.sectionExecutable section && not (ELF.sectionWritable section)
    && bytes > 0 && address `mod` 4 == 0 && bytes `mod` 4 == 0
    && ELF.sectionAddress section <= address && end <= sectionEnd
    && base <= address && end <= toInteger base + toInteger size)
    (Left "recipe edit getter is not complete readonly A32 code inside the declared runtime")
  let carried = B.take (fromIntegral bytes) (B.drop (fromIntegral (address - base)) (Ram.runtimeBytes manifest))
  words_ <- mapM (\pc -> maybe (Left "recipe edit getter word is absent from ELF") Right (ELF.wordAt elf pc))
    [address,address+4 .. address+bytes-4]
  unless (carried == B.concat (map littleWord words_))
    (Left "recipe edit getter ELF bytes differ from the carried runtime")
  let start = BankAddress (fromIntegral address)
      binding = GetterBinding start (BankExtent start (BankAddress (fromInteger end))) (SHA.sha256 carried)
  getterArm <- uniqueSymbol elf "rb_stage_edit_get_arm"
  setterArm <- uniqueSymbol elf "rb_stage_edit_set_arm"
  forM_ [getterArm, setterArm] $ \arm -> do
    armSection <- case ELF.symbolSection arm of
      ELF.InSection index | index >= 0 && index < length (ELF.elfSections elf) -> Right (ELF.elfSections elf !! index)
      _ -> Left "recipe edit legacy arm has no allocated section"
    unless (ELF.symbolKind arm == ELF.FunctionSymbol && ELF.symbolSize arm == 8
      && ELF.sectionAllocated armSection && ELF.sectionExecutable armSection && not (ELF.sectionWritable armSection)
      && ELF.sectionAddress armSection <= ELF.symbolValue arm
      && toInteger (ELF.symbolValue arm) + 8 <= toInteger (ELF.sectionAddress armSection) + toInteger (ELF.sectionSize armSection)
      && base <= ELF.symbolValue arm && toInteger (ELF.symbolValue arm) + 8 <= toInteger base + toInteger size)
      (Left "recipe edit legacy arm differs from its complete readonly runtime extent")
  forM_ [sourceSection | sourceSection <- ELF.elfSections elf, ELF.sectionAllocated sourceSection,
      not (ELF.sectionWritable sourceSection), ELF.sectionSize sourceSection > 0] $ \sourceSection -> do
    let low = ELF.sectionAddress sourceSection
        count = ELF.sectionSize sourceSection
        expected = if ELF.sectionType sourceSection == 8 then B.replicate (fromIntegral count) 0
          else B.take (fromIntegral count) (B.drop (fromIntegral (ELF.sectionOffset sourceSection)) elfBytes)
    unless (base <= low && toInteger low + toInteger count <= toInteger base + toInteger size
      && B.length expected == fromIntegral count
      && B.take (fromIntegral count) (B.drop (fromIntegral (low - base)) (Ram.runtimeBytes manifest)) == expected)
      (Left "recipe edit allocated readonly ELF source differs from the full runtime")
  pure RecipeEditPackage
    { getterBinding = binding, runtimeBinding = bound
    , packageElfDigest = SHA.sha256 elfBytes, packageRuntimeDigest = SHA.sha256 (Ram.runtimeBytes manifest)
    , packageManifestDigest = expectedManifest
    , packageSources = [(path, SHA.sha256 bytes_) | (path, bytes_) <- sourceRows]
    , packageElfBytes = elfBytes, packageRuntimeBytes = Ram.runtimeBytes manifest
    , packageAllocation = BankExtent (BankAddress base) (BankAddress (base + fromIntegral size))
    , packagePublications = Ram.manifestPublications manifest, packageBankVariant = Ram.manifestBankVariant manifest
    , packageEditArms = (BankAddress (ELF.symbolValue getterArm), BankAddress (ELF.symbolValue setterArm))
    , packageEntries = Ram.manifestEntries manifest, packageSourceIdentity = Ram.sourceIdentity manifest
    }
  where
    littleWord value = B.pack [fromIntegral (value `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]

recipeEditLinkerInclude :: RecipeEditPackage -> B.ByteString
recipeEditLinkerInclude package = C.pack ("xm1_recipe_edit_get = 0x" ++ showHex address ";\n")
  where BankAddress address = getterEntry (getterBinding package)

packageIdentityValue :: RecipeEditPackage -> Value
packageIdentityValue package = object
  [ "kind" .= ("source-bound-recipe-edit-runtime" :: String)
  , "elf_sha256" .= SHA.sha256Hex (packageElfDigest package)
  , "runtime_sha256" .= SHA.sha256Hex (packageRuntimeDigest package)
  , "manifest_sha256" .= SHA.sha256Hex (packageManifestDigest package)
  , "getter" .= object
      [ "entry" .= entry, "start" .= low, "end" .= high
      , "code_sha256" .= SHA.sha256Hex (getterCodeDigest binding)
      , "implementation" .= show RetainedCRecipeEditGetter
      ]
  , "sources" .= [object ["path" .= path, "sha256" .= SHA.sha256Hex digest] | (path, digest) <- packageSources package]
  , "runtime_binding" .= fmap Modal.modalRuntimeSourceJson (runtimeBinding package)
  , "publication_phase" .= (case runtimeBinding package of Nothing -> "prepublication"; Just _ -> "bound-publications" :: String)
  , "installed_lifetime_admitted" .= False
  ]
  where
    binding = getterBinding package
    BankAddress entry = getterEntry binding
    BankExtent (BankAddress low) (BankAddress high) = getterExtent binding
