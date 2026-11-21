{-# LANGUAGE GHC2021 #-}

-- | The reference frontier of a locally placed image4: the retired public bodies and the retained
-- fields no production admission owns.
module Replacement.PropertyService.PlacementNative
  ( retiredRegions
  , isRetiredFetch
  , retiredReferences
  , retainedDependencies
  , keptWords
  , RetainedFieldEvidence (..)
  , retainedFieldEvidence
  ) where

import qualified Data.Set as S
import Data.List.NonEmpty (NonEmpty)
import Replacement.PropertyService.Placement
import Replacement.PropertyService.RetainedClosure (ClosedSite, RetainedDependency (..), accountsHit, closesHit)
import qualified Replacement.PropertyService.RetainedGvff as Gvff
import Replacement.PropertyService.RetainedDfShaper (dfShaperClosures)
import Replacement.PropertyService.RetainedFdl (fdlClosures)
import Replacement.PropertyService.RetainedImageProcessor (imageProcessorDependencies)
import Replacement.PropertyService.RetainedImage4EarlyTables (image4EarlyTableClosures)
import Replacement.PropertyService.RetainedImage4Pcm (pcmClosures)
import Replacement.PropertyService.RetainedImage5Blocks (image5BlockClosures)
import Replacement.PropertyService.RetainedImage5Cfg (image5CfgClosures)
import Replacement.PropertyService.RetainedImage5Tables (image5TableClosures)
import Replacement.PropertyService.RetainedImagingConfig (imagingConfigClosures, imagingConfigDependencies)
import Replacement.PropertyService.RetainedLogTable (logTableClosures, logTableDependencies)
import Replacement.PropertyService.RetainedPlainBlocks (plainBlockClosures)
import Replacement.PropertyService.RetainedPlainCfg (plainCfgClosures)
import Replacement.PropertyService.RetainedPlainTables (plainTableClosures)
import Replacement.PropertyService.RetainedScalar (scalarClosures)
import Replacement.PropertyService.RetainedStrings (stringClosures)
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
  , not (any (`closesHit` hit) closures)
  , not (any (`accountsHit` hit) dependencies)
  , not (hitImage hit == ScanImage4 && any (\retired -> inCarrier retired (hitSite hit)) retiredRegions)
  ]
  where
    closures = retainedClosures images
    dependencies = retainedDependencies images

-- | Dependencies whose kept words are aligned words of a retired body outside the carrier.
retainedDependencies :: [(ScanImage, Image)] -> [RetainedDependency]
retainedDependencies images =
  [ dependency
  | dependency <- concatMap ($ images) [imagingConfigDependencies, logTableDependencies, imageProcessorDependencies]
  , not (null (dependencyKept dependency))
  , all keepable (dependencyKept dependency)
  ]
  where
    keepable address@(GuestAddress word) =
      word `mod` 4 == 0 && any (`inCarrier` address) retiredRegions && not (inCarrier stage4Carrier address)

-- | The retired words staging leaves stock.
keptWords :: [RetainedDependency] -> S.Set GuestAddress
keptWords dependencies = S.fromList (concatMap dependencyKept dependencies)

retainedClosures :: [(ScanImage, Image)] -> [ClosedSite]
retainedClosures images = concatMap ($ images)
  [ scalarClosures
  , plainCfgClosures
  , image5CfgClosures
  , image4EarlyTableClosures
  , plainBlockClosures
  , stringClosures
  , image5TableClosures
  , image5BlockClosures
  , plainTableClosures
  , fdlClosures
  , pcmClosures
  , Gvff.gvffClosures
  , imagingConfigClosures
  , logTableClosures
  , dfShaperClosures
  ]

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
