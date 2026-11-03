{-# LANGUAGE OverloadedStrings #-}
-- | The registered J1 joins.
module Replacement.StartupArtifact.Joins
  ( JoinKind (..), registeredJoins
  ) where

import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.LoaderPrefixWitness as P
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types

data JoinKind = FirstStageSecondaryRendezvous | CriticalPairPublisherBindings
  deriving (Eq, Ord, Show, Enum, Bounded)

instance JoinName JoinKind where
  joinName FirstStageSecondaryRendezvous = "first-stage-secondary-rendezvous"
  joinName CriticalPairPublisherBindings = "critical-pair-publisher-bindings"

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

-- | Little-endian word of a decoded target at a guest address.
targetWord :: TargetBytes -> Target -> Word32 -> Maybe Word32
targetWord targets target address
  | offset >= 0 && offset + 4 <= B.length bytes = Just (U.word32At bytes offset)
  | otherwise = Nothing
  where
    bytes = targetOf targets target
    offset = fromIntegral address - fromIntegral (targetBase target)

firstStageEntry :: JoinEvidence -> Maybe Word32
firstStageEntry evidence = case [entry | FirstStageFacts (Pc entry) <- map pieceFacts (evidencePieces evidence)] of
  [entry] -> Just entry
  _ -> Nothing

secondaryFacts :: JoinEvidence -> Maybe (Word32, [ServiceBinding], LinkedBytes)
secondaryFacts evidence = case [(entry, consumed, linked) | Record2LinkFacts (Pc entry) consumed linked <- map pieceFacts (evidencePieces evidence)] of
  [facts] -> Just facts
  _ -> Nothing

publications :: JoinEvidence -> Maybe [Publication]
publications evidence = case [rows | PublisherFacts rows <- map pieceFacts (evidencePieces evidence)] of
  [rows] -> Just rows
  _ -> Nothing

resetLiteral :: Target -> JoinEvidence -> Maybe Word32
resetLiteral target evidence = targetWord (evidenceComposed evidence) target (targetBase target + fromIntegral F.resetVectorWordOffset)

rendezvous :: Join JoinKind
rendezvous = Join FirstStageSecondaryRendezvous (Set.fromList [FirstStage, Record2Link])
  [at Record2 0x10020 0x10024, at Record2 0x10400 0x110e0, at Record1 0x20 0x24, at Record1 0x6000 0x64ac]
  (ObservationQualified "first-stage peer release and record-2 rendezvous for the current record-2 link are qualified by the J1 composed natural comparison; E-COMPOSED-NATURAL-POSITIVE-150 covers the earlier [0x10400,0x10a6c) link")
  [ HostCheck "record1-reset-literal-is-first-stage-entry" $ \evidence ->
      maybe False (\entry -> resetLiteral Record1 evidence == Just entry) (firstStageEntry evidence)
  , HostCheck "record2-reset-literal-is-secondary-entry" $ \evidence ->
      maybe False (\(entry, _, _) -> resetLiteral Record2 evidence == Just entry) (secondaryFacts evidence)
  -- The composed-contract derivation: emulation/Emulation/LoaderPrefixWitness.hs:671-672.
  , HostCheck "prefix-owned-facts-on-composed-images" $ \evidence ->
      case (firstStageEntry evidence, secondaryFacts evidence) of
        (Just entry, Just (_, _, LinkedBytes image symbols)) -> either (const False) (const True)
          (P.stagedImagesFromParts (P.Guest entry) (evidenceParts evidence) >>= P.deriveOwnedFacts image symbols)
        _ -> False
  ]

-- | Loader A's header word at 0x40010 names its published interface root.
rootHeader :: Word32
rootHeader = 0x40010

bindings :: Join JoinKind
bindings = Join CriticalPairPublisherBindings (Set.fromList [Record2Link, PrimaryPublisher])
  [at LoaderA 0x40430 0x409ac]
  (HostProof "the owned publisher emits every core-0 interface binding the critical pair consumes at the unchanged loader-A root; its proof requires every store before its return (emulation/Emulation/PrimaryInitializerArtifact.hs:300,337), and core 1 reads the core-0 table only after the core-0 completion store at 0x40a08, which follows the return at 0x409e0")
  [ HostCheck "loader-a-root-unchanged" $ \evidence ->
      targetWord (evidenceComposed evidence) LoaderA rootHeader == targetWord (evidenceCanonical evidence) LoaderA rootHeader
        && targetWord (evidenceCanonical evidence) LoaderA rootHeader /= Nothing
  , HostCheck "core0-bindings-published-once" $ \evidence ->
      case (targetWord (evidenceCanonical evidence) LoaderA rootHeader, secondaryFacts evidence, publications evidence) of
        (Just root, Just (_, consumed, _), Just emitted) -> not (null consumed) && all (published root emitted) consumed
        _ -> False
  ]
  where
    physical (Guest address) = physicalAddress (fromIntegral address)
    published root emitted (ServiceBinding offset target) =
      mapMaybe (\row -> if physical (publicationGuest row) == physical (Guest (root + offset)) then Just (publicationValue row) else Nothing) emitted
        == [target]

registeredJoins :: [Join JoinKind]
registeredJoins = [rendezvous, bindings]
