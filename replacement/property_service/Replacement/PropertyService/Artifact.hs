{-# LANGUAGE GHC2021 #-}

module Replacement.PropertyService.Artifact
  ( CanonicalSource
  , LinkedArtifact
  , ArtifactInput (..)
  , ArtifactPurpose (..)
  , StagedImage4
  , StagedCandidate
  , StagedPlacement (..)
  , ArtifactError (..)
  , EntrySide (..)
  , StorageWidth (..)
  , DefaultImage (..)
  , EncodingPart (..)
  , canonicalSource
  , sourceProperties
  , sourceHull
  , deriveArtifact
  , artifactCarrier
  , artifactEntries
  , artifactTable
  , artifactSlotTable
  , artifactSpecialTable
  , artifactDefaultWindows
  , artifactFunctions
  , artifactProof
  , linkedBytes
  , storageStoreName
  , storageStoreWords
  , storageOutputName
  , storageOutputWords
  , storageStatusWords
  , mirrorBoundaries
  , stagePlacement
  , stageImage4
  , validateStagedImage4
  , stagedImage4Bytes
  , stagedBytes
  , stageCandidate
  , candidateBytes
  , candidateContractValue
  ) where

import Replacement.PropertyService.Artifact.Internal
