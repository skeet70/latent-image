{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosTimer.Candidate
  ( Owner (..), Change, changedExtents, mergeImage4, admitImage4Inverse, rejectCarrierOverlap, changeValue, capabilityManifest
  , CandidatePackage, admitCandidatePackage, candidateImage4, candidateBaseImage4, candidatePropertyArtifact
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Emulation.Sha256 as SHA
import qualified Replacement.StartupArtifact.Types as S
import qualified Replacement.PropertyService.Artifact as Property
import qualified Replacement.RtosTimer.RootPackage as Root
import qualified Replacement.StartupArtifact.Targets as Targets


data Owner = PropertyService | RecordStore deriving (Eq, Ord, Show)
data Change = Change Owner Int B.ByteString B.ByteString deriving (Eq, Show)
data Capability = Startup | Rtos | Heap | Properties | Records | Menu | Autofocus | ManualFocus | LiveView | Render | Capture | Develop | Save | Playback | Calibration | ExternalInterfaces | Recovery | Storage | Usb | Platform deriving (Eq, Show)
data CapabilityStatus = OwnedWithRetainedDependencies | RetainedOrAbsent deriving (Eq, Show)
data RetainedDependency = ApplicationRuntime | RtosRuntimePremises | PropertyEntryAndDefaults | RawNandAndSynchronization | DynamicBankInstallation | SensorAndLensServices | DisplayServices | LiveViewPipeline | CapturePipeline | DevelopPipeline | SavePipeline | PlaybackPipeline | CalibrationServices | ExternalProtocolServices | RecoveryServices | FileAndMediaServices | UsbProtocol | BoardServices deriving (Eq, Show)
data CandidatePackage = CandidatePackage
  { candidateImage4 :: B.ByteString
  , candidatePropertyArtifact :: Property.LinkedArtifact
  , candidateBaseImage4 :: B.ByteString
  }

admitCandidatePackage :: Root.RootPackage -> Property.LinkedArtifact -> [S.Span] -> [Change] -> B.ByteString -> Either String CandidatePackage
admitCandidatePackage root property reserved changes observed = do
  expected <- mergeImage4 reserved (Targets.targetOf (Root.packageTargets root) S.Image4) changes
  unless (expected == observed) (Left "final candidate differs from admitted composition")
  pure (CandidatePackage observed property (Targets.targetOf (Root.packageTargets root) S.Image4))


hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

changedExtents :: Owner -> B.ByteString -> B.ByteString -> Either String [Change]
changedExtents owner before after = do
  unless (B.length before == B.length after) (Left "carrier changes image4 extent")
  pure (walk 0)
  where
    walk offset
      | offset == B.length before = []
      | B.index before offset == B.index after offset = walk (offset + 1)
      | otherwise = let end = advance (offset + 1)
                     in Change owner offset (B.take (end-offset) (B.drop offset before)) (B.take (end-offset) (B.drop offset after)) : walk end
    advance offset
      | offset < B.length before && B.index before offset /= B.index after offset = advance (offset + 1)
      | otherwise = offset

rejectCarrierOverlap :: [S.Span] -> [S.Span] -> Either String ()
rejectCarrierOverlap left right = mapM_ check [(a, b) | a <- left, b <- right]
  where
    check (S.Span targetA (S.Guest lowA) (S.Guest highA), S.Span targetB (S.Guest lowB) (S.Guest highB)) =
      unless (targetA /= targetB || highA <= lowB || highB <= lowA)
        (Left "candidate allocated carrier collision")

mergeImage4 :: [S.Span] -> B.ByteString -> [Change] -> Either String B.ByteString
mergeImage4 reserved base changes = do
  let ordered = sortOn start changes
  mapM_ check (zip ordered (drop 1 ordered))
  mapM_ free ordered
  merged <- replaceAll base ordered
  admitImage4Inverse base merged ordered
  pure merged
  where
    start (Change _ offset _ _) = offset
    end (Change _ offset old _) = offset + B.length old
    check (left, right) = unless (end left <= start right) (Left "candidate carrier collision")
    free change = mapM_ (disjoint change) reserved
    disjoint change (S.Span target (S.Guest low) (S.Guest high)) =
      unless (target /= S.Image4 || fromIntegral (end change) + (0x61c400 :: Integer) <= fromIntegral low || fromIntegral high <= fromIntegral (start change) + (0x61c400 :: Integer))
        (Left "candidate carrier overlaps admitted startup carrier")

admitImage4Inverse :: B.ByteString -> B.ByteString -> [Change] -> Either String ()
admitImage4Inverse original candidate changes = do
  restored <- replaceAll candidate [Change owner offset new old | Change owner offset old new <- sortOn start changes]
  unless (restored == original) (Left "image4 inverse did not recover admitted input")
  where start (Change _ offset _ _) = offset

replaceAll :: B.ByteString -> [Change] -> Either String B.ByteString
replaceAll bytes = fmap B.concat . walk 0
  where
    walk cursor [] = Right [B.drop cursor bytes]
    walk cursor (Change _ offset old new : rest) = do
      unless (offset >= cursor && offset + B.length old <= B.length bytes && B.take (B.length old) (B.drop offset bytes) == old)
        (Left "candidate carrier preimage/readback mismatch")
      following <- walk (offset + B.length old) rest
      pure (B.take (offset - cursor) (B.drop cursor bytes) : new : following)

changeValue :: Change -> Value
changeValue (Change owner offset old new) = object
  [ "owner" .= show owner, "target" .= S.targetName S.Image4
  , "start" .= (0x61c400 + offset), "end" .= (0x61c400 + offset + B.length old)
  , "before_sha256" .= hash old, "after_sha256" .= hash new
  ]

capabilityManifest :: [S.PieceKind] -> Value
capabilityManifest pieces = object
  [ "total" .= False
  , "capabilities" .= map row capabilities
  , "bank_installation_prerequisite" .= ("A permanent boot producer must allocate a lifetime-owned bank, relink all base-sensitive code/data, install setup then activation publications with rendezvous/cache maintenance, and preserve original font/menu resources. RAM-push allocation addresses and sparse AF runtime payload addresses are not DAT carrier evidence." :: String)
  ]
  where
    capabilities :: [(Capability, Bool, RetainedDependency)]
    capabilities =
      [ (Startup, True, ApplicationRuntime), (Rtos, True, RtosRuntimePremises)
      , (Heap, S.GeneralHeap `elem` pieces, ApplicationRuntime)
      , (Properties, True, PropertyEntryAndDefaults), (Records, True, RawNandAndSynchronization)
      , (Menu, False, DynamicBankInstallation), (Autofocus, False, SensorAndLensServices)
      , (ManualFocus, False, SensorAndLensServices), (LiveView, False, LiveViewPipeline)
      , (Render, False, DisplayServices), (Capture, False, CapturePipeline)
      , (Develop, False, DevelopPipeline), (Playback, False, PlaybackPipeline)
      , (Calibration, False, CalibrationServices), (ExternalInterfaces, False, ExternalProtocolServices)
      , (Recovery, False, RecoveryServices)
      , (Save, False, SavePipeline), (Storage, False, FileAndMediaServices)
      , (Usb, False, UsbProtocol), (Platform, False, BoardServices)
      ]
    row (capability, installed, dependency) = object
      [ "capability" .= show capability, "owned_component_installed" .= installed
      , "status" .= show (if installed then OwnedWithRetainedDependencies else RetainedOrAbsent)
      , "retained_dependency" .= show dependency
      ]
