{-# LANGUAGE OverloadedStrings #-}
module Coverage (coverageReport) where

import Data.Aeson (Value, object, (.=))
import ShimCatalog

coverageReport :: [Unit] -> Manifest -> Either Refusal [ImageOutcome] -> Value
coverageReport units (Manifest images references) result = object
  [ "result" .= report result
  , "coverage" .= map unitCoverage units
  , "references" .= map referenceCoverage references
  , "image_count" .= length images
  , "row_count" .= sum (map (length . unitRows) units)
  ]
  where
    unitCoverage unit = object
      [ "unit" .= unitName (unitId unit)
      , "classification" .= show (unitClass unit)
      , "rows" .= map rowCoverage (unitRows unit)
      ]
    rowCoverage row = object
      [ "id" .= shimName (rowId row)
      , "images" .= [imageName target | evidence <- rowEvidence row, Just target <- [evidenceImage evidence]]
      , "reference_only" .= all ((== Nothing) . evidenceImage) (rowEvidence row)
      , "has_reference_obligations" .= any ((== Reference) . evidenceKind) (rowEvidence row)
      , "consumers" .= map unitName (rowConsumers row)
      , "removal" .= truthName (removalTruth (rowPredicates row))
      ]
    referenceCoverage (ReferenceArtifact target owners (ContractPath contract) reason) = object
      [ "image" .= imageName target
      , "units" .= map unitName owners
      , "contract" .= contract
      , "reason" .= reason
      ]
    unitName (UnitId name) = name
    imageName (ImageId name) = name
    shimName (ShimId name) = name
