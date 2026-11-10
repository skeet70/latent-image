{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosTimer.CombinedPublication
  ( CombinedPublicationPlan, combinePublicationPlan, combinedPublications
  , combinedTableBytes, combinedCarrierBeforeDigest, combinedCarrierAfterDigest
  , verifyCombinedReadback, combinedRollbackPublications
  , combinedRuntimeBytes, combinedCounts
  , composePublicationVector
  , combinedManifestValue, combinedProofValue, combinedBankElfBytes
  , PublicationDirection (..), applyCarrierPublication
  , combineSyntheticPublicationPlan
  ) where

import Control.Monad (unless, forM_)
import Data.Aeson (Value, object, (.=))
import Data.String (fromString)
import Numeric (showHex)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Emulation.Sha256 as SHA
import Data.Word (Word32)
import qualified FujiAF.RamPush as Ram
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.PropertyService.Artifact as Property
import qualified Replacement.PropertyService.Placement as P
import qualified Replacement.PropertyService.RecipeArtifact as Recipe
import Replacement.RtosTimer.BankPackage
import Replacement.RtosTimer.Candidate (CandidatePackage, candidateImage4, candidateBaseImage4, candidatePropertyArtifact)
import qualified RamPushLayout as Layout

-- The vector is consumed in order by the parked installer.
data CombinedPublicationPlan = CombinedPublicationPlan
  { combinedPublications :: [Ram.Publication]
  , combinedTableBytes :: B.ByteString
  , combinedCarrierBeforeDigest :: SHA.Sha256
  , combinedCarrierAfterDigest :: SHA.Sha256
  , carrierExtent :: P.Carrier
  , combinedRuntimeBytes :: B.ByteString
  , combinedCounts :: Layout.PublicationCounts
  , planPackage :: RecipeEditPackage
  , planLayout :: Layout.PropertyPublicationLayout
  , planEngineStart :: BankAddress
  , planPropertyVariant :: Recipe.PropertyVariantPublication
  }

combinePublicationPlan :: CandidatePackage -> RecipeEditPackage -> Recipe.PropertyVariantPublication -> Either String CombinedPublicationPlan
combinePublicationPlan candidate = combineImages (candidateBaseImage4 candidate)
  (candidateImage4 candidate) (candidatePropertyArtifact candidate)

combineSyntheticPublicationPlan :: B.ByteString -> Property.LinkedArtifact -> RecipeEditPackage -> Recipe.PropertyVariantPublication -> Either String (CombinedPublicationPlan, B.ByteString)
combineSyntheticPublicationPlan source canonical package variant = do
  let P.Carrier (P.GuestAddress low) (P.GuestAddress high) = Property.artifactCarrier canonical
      offset = fromIntegral (low - 0x61c400)
      size = fromIntegral (high - low)
      linked = Property.linkedBytes canonical
      fill = linked <> B.concat (replicate ((size - B.length linked) `div` 4) (U.putWord32 0xe7f000f0))
  unless (low >= 0x61c400 && high > low && offset + size <= B.length source
    && B.length linked <= size && B.length linked `mod` 4 == 0)
    (Left "synthetic canonical carrier does not fit the source image")
  let installed = B.take offset source <> fill <> B.drop (offset + size) source
  plan <- combineImages source installed canonical package variant
  pure (plan, installed)

combineImages :: B.ByteString -> B.ByteString -> Property.LinkedArtifact -> RecipeEditPackage -> Recipe.PropertyVariantPublication -> Either String CombinedPublicationPlan
combineImages source installed canonical package variant = do
  selected <- maybe (Left "combined publication requires render bank variant") Right (packageBankVariant package)
  layout <- Layout.propertyPublicationLayout selected
  counts <- Layout.propertyPublicationCounts layout (length (Recipe.publicationWords variant))
  unless (packageIdentityValue package == packageIdentityValue (Recipe.publicationPackage variant))
    (Left "property variant uses another source-bound bank package")
  let carrier@(P.Carrier (P.GuestAddress low) (P.GuestAddress high)) = Recipe.publicationCarrier variant
      before = slice installed low high
      original = packagePublications package
      delta = [Ram.Publication 0 Ram.Setup (Ram.Address wordAddress)
        (Ram.WordValue (Recipe.publicationCanonicalWord word))
        (Ram.WordValue (Recipe.publicationRecipeWord word)) Ram.Both
        | word <- Recipe.publicationWords variant, let P.GuestAddress wordAddress = Recipe.publicationWordAddress word]
      allocation = packageAllocation package
      BankAddress allocationLow = extentStart allocation
      BankAddress allocationHigh = extentEnd allocation
  rows <- composePublicationVector (packageEditArms package) original delta
  let addresses = map (Ram.unAddress . Ram.publicationAddress) rows
  forM_ [row | row <- original, Ram.publicationAddress row `notElem` map Ram.publicationAddress rows] $ \row -> do
    let site = Ram.unAddress (Ram.publicationAddress row)
        offset = fromIntegral (site - 0x61c400)
    unless (site >= 0x61c400 && offset + 4 <= B.length source
      && Ram.publicationOld row == Ram.WordValue (U.word32At source offset))
      (Left "obsolete property table preimage differs from admitted candidate source")
  unless (low == 0x62c120 && high == 0x62fbfc && Property.artifactCarrier canonical == carrier
    && SHA.sha256 (Property.linkedBytes canonical) == Recipe.publicationCanonicalDigest variant
    && SHA.sha256 before == Recipe.publicationCanonicalCarrierDigest variant)
    (Left "installed candidate canonical property carrier differs")
  unless (length addresses == length (nub addresses) && all ((== 0) . (`mod` 4)) addresses
    && all (\site -> site < allocationLow || site >= allocationHigh) addresses
    && all (\row -> let site = Ram.unAddress (Ram.publicationAddress row) in site < low || site >= high)
      (filter ((`notElem` map Ram.publicationAddress delta) . Ram.publicationAddress) rows))
    (Left "combined publication ranges collide or are unaligned")
  after <- applyCarrierPublication (P.GuestAddress low) before delta InstallPublication
  unless (SHA.sha256 after == Recipe.publicationRecipeCarrierDigest variant)
    (Left "property delta does not recover admitted recipe carrier")
  restored <- applyCarrierPublication (P.GuestAddress low) after delta RollbackPublication
  unless (restored == before) (Left "property inverse does not recover installed carrier")
  config <- Layout.encodePropertyPublicationConfig layout counts
  let table = B.concat (map encode rows)
      BankAddress base = extentStart allocation
      installerOffset = fromIntegral (Layout.runtimeInstallerBase base - base)
      configOffset = installerOffset + fromIntegral Layout.propertyPublicationConfigOffset
      tableOffset = installerOffset + fromIntegral Layout.propertyPublicationTableOffset
      runtime = packageRuntimeBytes package
      capacityBytes = Layout.propertyPublicationCapacity layout * 16
  expectedSize <- Layout.propertyAllocationSize layout base
  engineStart <- Layout.propertyMenuEngineRegionStart layout base
  unless (B.length runtime == fromIntegral expectedSize
    && tableOffset + capacityBytes <= B.length runtime
    && B.take 16 (B.drop configOffset runtime) == B.replicate 16 0
    && B.take capacityBytes (B.drop tableOffset runtime) == B.replicate capacityBytes 0)
    (Left "bank is not an unconsumed property prepublication runtime")
  let replace offset bytes image = B.take offset image <> bytes <> B.drop (offset + B.length bytes) image
      completed = replace tableOffset table (replace configOffset config runtime)
  pure (CombinedPublicationPlan rows table
    (SHA.sha256 before) (SHA.sha256 after) carrier completed counts package layout (BankAddress engineStart) variant)
  where
    encode row = B.concat (map U.putWord32
      [Ram.unAddress (Ram.publicationAddress row), Ram.unWordValue (Ram.publicationOld row)
      , Ram.unWordValue (Ram.publicationNew row), if Ram.publicationPhase row == Ram.Setup then 0 else 1])

composePublicationVector :: (BankAddress, BankAddress) -> [Ram.Publication] -> [Ram.Publication] -> Either String [Ram.Publication]
composePublicationVector (BankAddress getArm, BankAddress setArm) original delta = do
  unless (length original == 228 && sort (map Ram.publicationIndex original) == [0..227]
    && map Ram.publicationPhase original == replicate 131 Ram.Setup ++ replicate 97 Ram.Activation)
    (Left "render bank publication order differs")
  let obsolete = [(site, getArm) | site <- [0x61eac4,0x61eac8 .. 0x61eae4]]
        ++ [(site, setArm) | site <- [0x625064,0x625068 .. 0x625084]]
      address = Ram.unAddress . Ram.publicationAddress
  forM_ obsolete $ \(site, target) -> case filter ((== site) . address) original of
    [row] -> unless (Ram.publicationPhase row == Ram.Setup && Ram.publicationCore row == Ram.UnknownCore
      && Ram.publicationNew row == Ram.WordValue target)
      (Left "obsolete property table semantic proof differs")
    _ -> Left "obsolete property table word missing or duplicated"
  let keep = filter ((`notElem` map fst obsolete) . address) original
      (setups, activations) = span ((== Ram.Setup) . Ram.publicationPhase) keep
      rows = zipWith (\index row -> row {Ram.publicationIndex = index}) [0..] (setups ++ delta ++ activations)
      addresses = map address rows
  unless (length setups == 113 && length activations == 97
    && all ((== Ram.Activation) . Ram.publicationPhase) activations
    && all (\row -> Ram.publicationPhase row == Ram.Setup && Ram.publicationCore row == Ram.Both
      && Ram.publicationOld row /= Ram.publicationNew row) delta)
    (Left "combined publication phases differ")
  unless (length addresses == length (nub addresses) && all ((== 0) . (`mod` 4)) addresses)
    (Left "combined publication ranges collide or are unaligned")
  pure rows

slice :: B.ByteString -> WordAddress -> WordAddress -> B.ByteString
slice bytes low high = B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - 0x61c400)) bytes)

type WordAddress = Word32

data PublicationDirection = InstallPublication | RollbackPublication deriving (Eq, Show)

applyCarrierPublication :: P.GuestAddress -> B.ByteString -> [Ram.Publication] -> PublicationDirection -> Either String B.ByteString
applyCarrierPublication (P.GuestAddress base) = foldWords
  where
    foldWords bytes [] _ = Right bytes
    foldWords bytes (row:rest) inverse = do
      let site = Ram.unAddress (Ram.publicationAddress row)
          offset = fromIntegral (site - base)
          old = Ram.unWordValue (if inverse == RollbackPublication then Ram.publicationNew row else Ram.publicationOld row)
          new = Ram.unWordValue (if inverse == RollbackPublication then Ram.publicationOld row else Ram.publicationNew row)
      unless (site >= base && site `mod` 4 == 0 && offset + 4 <= B.length bytes
        && U.word32At bytes offset == old) (Left "property carrier word preimage differs")
      foldWords (B.take offset bytes <> U.putWord32 new <> B.drop (offset + 4) bytes) rest inverse

verifyCombinedReadback :: CombinedPublicationPlan -> PublicationDirection -> B.ByteString -> Either String ()
verifyCombinedReadback plan inverse image = do
  let P.Carrier (P.GuestAddress low) (P.GuestAddress high) = carrierExtent plan
      expected = if inverse == RollbackPublication then combinedCarrierBeforeDigest plan else combinedCarrierAfterDigest plan
  unless (SHA.sha256 (slice image low high) == expected) (Left "combined full-carrier readback differs")

combinedRollbackPublications :: CombinedPublicationPlan -> [Ram.Publication]
combinedRollbackPublications = map swap . reverse . combinedPublications
  where swap row = row {Ram.publicationOld = Ram.publicationNew row, Ram.publicationNew = Ram.publicationOld row}

combinedManifestValue :: CombinedPublicationPlan -> Value
combinedManifestValue plan = object
  [ "schema" .= (1 :: Int)
  , "base" .= hexadecimal base
  , "size" .= B.length (combinedRuntimeBytes plan)
  , "runtime_sha256" .= SHA.sha256Hex (SHA.sha256 (combinedRuntimeBytes plan))
  , "source_identity" .= object [fromString path .= digest | (path, digest) <- packageSourceIdentity package]
  , "entry" .= object
      [ "install" .= entry Ram.installEntry, "rollback" .= entry Ram.rollbackEntry, "status" .= entry Ram.statusEntry ]
  , "bank_variant" .= Layout.bankVariantTag Layout.modalRenderBankVariant
  , "publications" .= map publication (combinedPublications plan)
  , "property_extension" .= object
      [ "capacity" .= Layout.propertyPublicationCapacity (planLayout plan)
      , "carrier_word_bound" .= Layout.propertyCarrierWordBound
      , "config_offset" .= hexadecimal Layout.propertyPublicationConfigOffset
      , "table_offset" .= hexadecimal Layout.propertyPublicationTableOffset
      , "installer_extent" .= Layout.propertyInstallerExtent (planLayout plan)
      , "engine_start" .= hexadecimal (bankAddressValue (planEngineStart plan))
      , "engine_extent" .= Layout.propertyMenuEngineRegionExtent (planLayout plan)
      , "delta_words" .= (Layout.publicationActualSetupCount counts - 113)
      , "count" .= Layout.publicationActualCount counts
      , "setup_count" .= Layout.publicationActualSetupCount counts
      , "activation_count" .= Layout.publicationActualActivationCount counts
      ]
  ]
  where
    package = planPackage plan
    BankAddress base = extentStart (packageAllocation package)
    counts = combinedCounts plan
    entry select = hexadecimal (Ram.unAddress (select (packageEntries package)))
    hexadecimal :: Word32 -> String
    hexadecimal word = "0x" ++ replicate (8 - length digits) '0' ++ digits
      where digits = showHex word ""
    publication row = object
      [ "index" .= Ram.publicationIndex row
      , "phase" .= (if Ram.publicationPhase row == Ram.Setup then "setup" else "activation" :: String)
      , "address" .= hexadecimal (Ram.unAddress (Ram.publicationAddress row))
      , "old" .= hexadecimal (Ram.unWordValue (Ram.publicationOld row))
      , "new" .= hexadecimal (Ram.unWordValue (Ram.publicationNew row))
      , "core" .= core (Ram.publicationCore row)
      ]
    core :: Ram.CoreScope -> String
    core Ram.Cpu0 = "cpu0"
    core Ram.Cpu1 = "cpu1"
    core Ram.Both = "both"
    core Ram.UnknownCore = "unknown"

combinedBankElfBytes :: CombinedPublicationPlan -> B.ByteString
combinedBankElfBytes = packageElfBytes . planPackage

combinedProofValue :: CombinedPublicationPlan -> Value
combinedProofValue plan = object
  [ "bank_package" .= packageIdentityValue (planPackage plan)
  , "canonical_linked_sha256" .= digest Recipe.publicationCanonicalDigest
  , "recipe_linked_sha256" .= digest Recipe.publicationRecipeDigest
  , "canonical_carrier_readback_sha256" .= SHA.sha256Hex (combinedCarrierBeforeDigest plan)
  , "recipe_carrier_readback_sha256" .= SHA.sha256Hex (combinedCarrierAfterDigest plan)
  , "publication_table_sha256" .= SHA.sha256Hex (SHA.sha256 (combinedTableBytes plan))
  , "completed_runtime_sha256" .= SHA.sha256Hex (SHA.sha256 (combinedRuntimeBytes plan))
  , "installed_lifetime_admitted" .= False
  ]
  where digest select = SHA.sha256Hex (select (planPropertyVariant plan))
