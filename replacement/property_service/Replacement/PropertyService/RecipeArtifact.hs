{-# LANGUAGE GHC2021 #-}

module Replacement.PropertyService.RecipeArtifact
  ( RecipeArtifact
  , deriveRecipeArtifact
  , recipeLinkedArtifact
  , recipePackage
  , recipeEditLinkerInclude
  , PropertyVariantPublication
  , PropertyPublicationWord
  , PropertyPublicationError (..)
  , derivePropertyVariantPublication
  , publicationCarrier
  , publicationWords
  , publicationWordAddress
  , publicationCanonicalWord
  , publicationRecipeWord
  , publicationCanonicalDigest
  , publicationRecipeDigest
  , publicationCanonicalCarrierDigest
  , publicationRecipeCarrierDigest
  , publicationPackage
  ) where

import qualified Data.ByteString as B
import Data.Aeson (encode)
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Admission (FunctionExtent (..), FunctionName (..))
import Replacement.PropertyService.Artifact (ArtifactError (..), ArtifactInput, LinkedArtifact, artifactCarrier, artifactEntries, linkedBytes)
import Replacement.PropertyService.Artifact.Internal (RecipeDependency (..), RecipePackageIdentity (..), artifactRecipeIdentity, deriveRecipeArtifactInternal)
import Replacement.PropertyService.Classify (Property)
import Replacement.PropertyService.Placement (Carrier (..), GuestAddress (..), carrierBytes)
import Replacement.RtosTimer.BankPackage
  ( RecipeEditPackage
  , bankAddressValue
  , extentEnd
  , extentStart
  , getterBinding
  , getterEntry
  , getterExtent
  , packageIdentityValue
  , packageAllocation
  , recipeEditLinkerInclude
  )

data RecipeArtifact = RecipeArtifact
  { recipeLinkedArtifact :: LinkedArtifact
  , recipePackage :: RecipeEditPackage
  }

-- | Admission binds the callback layout, not its installation or lifetime.
deriveRecipeArtifact :: RecipeEditPackage -> [Property] -> Carrier -> ArtifactInput -> Either ArtifactError RecipeArtifact
deriveRecipeArtifact package properties carrier input = do
  let binding = getterBinding package
      extent = getterExtent binding
      entry = bankAddressValue (getterEntry binding)
      low = bankAddressValue (extentStart extent)
      high = bankAddressValue (extentEnd extent)
      allocation = packageAllocation package
      allocationLow = bankAddressValue (extentStart allocation)
      allocationHigh = bankAddressValue (extentEnd allocation)
      Carrier (GuestAddress carrierLow) (GuestAddress carrierHigh) = carrier
  if allocationLow >= allocationHigh || not (allocationHigh <= carrierLow || allocationLow >= carrierHigh)
    then Left CallbackRejected else pure ()
  if entry /= low then Left CallbackRejected else do
    linked <- deriveRecipeArtifactInternal
      (RecipeDependency (FunctionExtent (FunctionName "xm1_recipe_edit_get") (GuestAddress low) (GuestAddress high))
        (RecipePackageIdentity (SHA.sha256 (L.toStrict (encode (packageIdentityValue package)))))) properties carrier input
    pure (RecipeArtifact linked package)

data PropertyPublicationError
  = CanonicalArtifactRequired
  | PublicationCarrierMismatch
  | PublicationEntriesMismatch
  | PublicationExtentRejected
  deriving (Eq, Show)

data PropertyPublicationWord = PropertyPublicationWord
  { publicationWordAddress :: GuestAddress
  , publicationCanonicalWord :: Word32
  , publicationRecipeWord :: Word32
  }
  deriving (Eq, Show)

data PropertyVariantPublication = PropertyVariantPublication
  { publicationCarrier :: Carrier
  , publicationWords :: [PropertyPublicationWord]
  , publicationCanonicalDigest :: SHA.Sha256
  , publicationRecipeDigest :: SHA.Sha256
  , publicationCanonicalCarrierDigest :: SHA.Sha256
  , publicationRecipeCarrierDigest :: SHA.Sha256
  , publicationPackage :: RecipeEditPackage
  }

-- | The delta requires parked consumers, live callback code, coherent instruction caches and rollback.
derivePropertyVariantPublication :: LinkedArtifact -> RecipeArtifact -> Either PropertyPublicationError PropertyVariantPublication
derivePropertyVariantPublication canonical recipe = do
  case artifactRecipeIdentity canonical of
    Nothing -> pure ()
    Just _ -> Left CanonicalArtifactRequired
  let replacement = recipeLinkedArtifact recipe
      carrier@(Carrier (GuestAddress low) _) = artifactCarrier canonical
      canonicalBytes = linkedBytes canonical
      recipeBytes = linkedBytes replacement
      size = carrierBytes carrier
      fits bytes = B.length bytes <= size && B.length bytes `mod` 4 == 0
  if carrier /= artifactCarrier replacement then Left PublicationCarrierMismatch else pure ()
  if artifactEntries canonical /= artifactEntries replacement then Left PublicationEntriesMismatch else pure ()
  if size <= 0 || size `mod` 4 /= 0 || not (fits canonicalBytes && fits recipeBytes)
    then Left PublicationExtentRejected else pure ()
  let fill bytes = bytes <> B.concat (replicate ((size - B.length bytes) `div` 4) (U.putWord32 0xe7f000f0))
      before = fill canonicalBytes
      after = fill recipeBytes
      differences =
        [ PropertyPublicationWord (GuestAddress (low + fromIntegral offset)) old new
        | offset <- [0, 4 .. size - 4]
        , let old = U.word32At before offset
        , let new = U.word32At after offset
        , old /= new
        ]
  pure (PropertyVariantPublication carrier differences
    (SHA.sha256 canonicalBytes) (SHA.sha256 recipeBytes)
    (SHA.sha256 before) (SHA.sha256 after) (recipePackage recipe))
