{-# LANGUAGE GHC2021 #-}

-- | The reference frontier of a locally placed image4: the retired public bodies and the retained
-- fields no production admission owns.
module Replacement.PropertyService.PlacementNative
  ( retiredRegions
  , isRetiredFetch
  , retiredReferences
  , RetainedFieldEvidence (..)
  , retainedFieldEvidence
  ) where

import qualified Data.Set as S
import Data.List.NonEmpty (NonEmpty)
import Replacement.PropertyService.Placement
import qualified Replacement.PropertyService.RetainedGvff as Gvff
import qualified Replacement.PropertyService.RetainedScalar as Scalar
import ServiceInventory (Image)

retiredRegions :: [Carrier]
retiredRegions =
  [ Carrier (GuestAddress 0x61e114) (GuestAddress 0x6245f8)
  , Carrier (GuestAddress 0x624698) (GuestAddress 0x6309c4)
  ]

isRetiredFetch :: Carrier -> GuestAddress -> Bool
isRetiredFetch payload address = any (`inCarrier` address) retiredRegions && not (inCarrier payload address)

retiredReferences :: [(ScanImage, Image)] -> [ScanHit]
retiredReferences images =
  [ hit
  | region <- retiredRegions
  , hit <- scanReferences region images S.empty
  , not (provedInstruction hit)
  , not (hitImage hit == ScanImage4 && any (\retired -> inCarrier retired (hitSite hit)) retiredRegions)
  ]
  where
    proofs = [(label, proof) | (label, image) <- images, Just proof <- [Scalar.deriveInstructionProof image]]
    provedInstruction hit =
      hitReference hit == WordInto
        && maybe False (\proof -> Scalar.scalarProvesNonPointer proof (hitSite hit) (hitWord hit)) (lookup (hitImage hit) proofs)

data RetainedFieldEvidence = RetainedGvffField
  { retainedFieldHit :: ScanHit
  , retainedFieldRole :: Gvff.GvffFieldRole
  , retainedFieldUnclosedPremises :: NonEmpty Gvff.GvffUnclosedPremise
  }
  deriving (Eq, Show)

retainedFieldEvidence :: [(ScanImage, Image)] -> [ScanHit] -> [RetainedFieldEvidence]
retainedFieldEvidence images hits = case fields of
  Nothing -> []
  Just fieldMap ->
    [ RetainedGvffField hit (Gvff.gvffFieldRole field) (Gvff.gvffUnclosedPremises fieldMap)
    | hit <- hits
    , hitImage hit == ScanImage2
    , hitReference hit == WordInto
    , Just field <- [Gvff.gvffFieldAt fieldMap (hitSite hit)]
    , Gvff.gvffFieldWord field == hitWord hit
    ]
  where
    fields = do
      image2 <- lookup ScanImage2 images
      image4 <- lookup ScanImage4 images
      plain <- lookup ScanPlain images
      loaderA <- lookup ScanLoaderA images
      Gvff.deriveGvffFieldMap image2 image4 plain loaderA
