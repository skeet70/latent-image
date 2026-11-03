{-# LANGUAGE OverloadedStrings #-}
-- | composed-contract.json, schema 2, in the shape of
-- emulation/fujimod-xm1/tests/data/composed-contract-v2.json: every list is
-- sorted and non-overlapping, and every object carries all its keys.
module Replacement.StartupArtifact.Contract
  ( contractValue, normalizeFootprint, allowRanges, EmittedWrites (..), contractCarriers
  ) where

import qualified Data.Aeson as J
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as Key
import Data.Aeson.Types (Pair)
import qualified Data.ByteString as B
import Data.List (nub, sort, sortOn)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types

sha256 :: B.ByteString -> Text
sha256 = T.pack . SHA.sha256Hex . SHA.sha256

spanFields :: Span -> [Pair]
spanFields (Span target (Guest start) (Guest end)) = ["target" .= targetName target, "start" .= start, "end" .= end]

identityValue :: B.ByteString -> TargetBytes -> Value
identityValue dat targets = object
  [ "dat_sha256" .= sha256 dat
  , "targets" .= object [Key.fromText (targetName target) .= sha256 bytes | (target, bytes) <- targetList targets]
  ]

-- | Disjoint sorted spans; each carries every citation that covers it and is an
-- admission read when any covering entry is.
normalizeFootprint :: [Footprint] -> [(Span, FootprintBasis, [Citation])]
normalizeFootprint footprints = concatMap perTarget [minBound .. maxBound]
  where
    perTarget target =
      let items = [(start, end, basis, citation) | Footprint (Span own (Guest start) (Guest end)) basis citation <- footprints, own == target]
          points = sort (nub (concat [[start, end] | (start, end, _, _) <- items]))
          labelled = [ ((low, high), (if AdmissionRead `elem` map fst covering then AdmissionRead else RuntimePremise, sort (nub (map snd covering))))
                     | (low, high) <- zip points (drop 1 points)
                     , let covering = [(basis, citation) | (start, end, basis, citation) <- items, start <= low, high <= end]
                     , not (null covering) ]
      in [(Span target (Guest low) (Guest high), basis, citations) | ((low, high), (basis, citations)) <- merge labelled]
    merge (((low, middle), tag) : ((next, high), other) : rest)
      | middle == next && tag == other = merge (((low, high), tag) : rest)
    merge (item : rest) = item : merge rest
    merge [] = []

data EmittedWrites = EmittedWrites [WriteRange] Text (Maybe Text) deriving (Eq, Show)

-- | Allow ranges in physical spelling; premised ranges exclude every owned
-- carrier of the composition.
allowRanges :: [Span] -> WritesPolicy -> EmittedWrites
allowRanges carriers policy = case policy of
  UnenumeratedWrites (Premise premise) -> EmittedWrites [] "unenumerated" (Just premise)
  EnumeratedWrites ranges Proved -> EmittedWrites (sort (map physicalRange ranges)) "proved" Nothing
  EnumeratedWrites ranges (Premised (Premise premise)) ->
    EmittedWrites (sort (concatMap (excludeCarriers . physicalRange) ranges)) "premised" (Just premise)
  where
    physicalRange (WriteRange target start end) | physicalTarget target = WriteRange target (physicalAddress start) (physicalAddress (end - 1) + 1)
    physicalRange range = range
    held = [(fromIntegral start, fromIntegral end) | Span _ (Guest start) (Guest end) <- carriers] :: [(Word64, Word64)]
    excludeCarriers range@(WriteRange target _ _) | physicalTarget target = foldl cut [range] held
    excludeCarriers range = [range]
    cut ranges (low, high) = concat
      [ [WriteRange target start (min end low) | start < min end low] ++ [WriteRange target (max start high) end | max start high < end]
      | WriteRange target start end <- ranges ]

physicalTarget :: WriteTarget -> Bool
physicalTarget target = case target of
  PhysicalTarget -> True
  CorePhysicalTarget _ -> True
  TcmTarget _ _ -> False

writeRangeValue :: WriteRange -> Value
writeRangeValue (WriteRange target start end) = object $ case target of
  PhysicalTarget -> ["target" .= ("physical" :: Text), "core" .= J.Null, "bank" .= J.Null, "start" .= start, "end" .= end]
  CorePhysicalTarget core -> ["target" .= ("physical" :: Text), "core" .= coreIndex core, "bank" .= J.Null, "start" .= start, "end" .= end]
  TcmTarget core bank -> ["target" .= ("tcm" :: Text), "core" .= coreIndex core, "bank" .= tcmBankName bank, "start" .= start, "end" .= end]

pcValue :: Pc -> Word32
pcValue (Pc value) = value

catalogValue :: [Span] -> Catalog -> Value
catalogValue carriers catalog = object
  [ "boundaries" .= map boundaryValue (catalogBoundaries catalog)
  , "owned_code" .= [object ["start" .= start, "end" .= end] | CodeSpan start end <- sort (catalogOwnedCode catalog)]
  , "writes" .= object ["allow" .= map writeRangeValue allow, "basis" .= basis, "premise" .= premise]
  , "reads" .= [ object [ "label" .= labelText (readLabel row), "boundary" .= labelText (readBoundary row)
                        , "address" .= readAddress row, "length" .= readLength row, "differs" .= readDiffers row ]
               | row <- catalogReads catalog ]
  ]
  where
    EmittedWrites allow basis premise = allowRanges carriers (catalogWrites catalog)
    boundaryValue boundary = object
      [ "label" .= labelText (boundaryLabel boundary), "kind" .= boundaryKindName (boundaryKind boundary)
      , "cores" .= map coreIndex (coreList (boundaryCores boundary)), "composed_pc" .= pcValue (boundaryComposedPc boundary)
      , "retained_pc" .= fmap pcValue (boundaryRetainedPc boundary), "stop" .= boundaryStop boundary
      , "compare" .= compareName (boundaryCompare boundary), "record_limit" .= recordLimitCount (boundaryRecordLimit boundary)
      ]

pieceValue :: [Span] -> Piece -> Value
pieceValue carriers piece = object
  [ "kind" .= pieceKindName (pieceKind piece)
  , "areas" .= map areaName (Set.toAscList (pieceAreas piece))
  , "carriers" .= [object (spanFields range) | range <- normalizeSpans (pieceCarriers piece)]
  , "edits" .= [object (spanFields (editSpan change) ++ ["sha256" .= sha256 (editBytes change)]) | change <- sortOn editSpan (pieceEdits piece)]
  , "footprint" .= [ object (spanFields range ++ ["basis" .= footprintBasisName basis, "citation" .= T.intercalate "; " [text | Citation text <- citations]])
                   | (range, basis, citations) <- normalizeFootprint (pieceFootprint piece) ]
  , "premises" .= [text | Premise text <- piecePremises piece]
  , "catalog" .= catalogValue carriers (pieceCatalog piece)
  ]

joinValue :: JoinName k => AppliedJoin k -> Value
joinValue (AppliedJoin join spans) = object
  [ "kind" .= joinName (joinKind join)
  , "members" .= map pieceKindName (Set.toAscList (joinMembers join))
  , "spans" .= [object (spanFields range) | range <- spans]
  , "basis" .= (case joinBasis join of HostProof _ -> "host-proof" :: Text; ObservationQualified _ -> "observation-qualified")
  , "reason" .= (case joinBasis join of HostProof reason -> reason; ObservationQualified reason -> reason)
  ]

-- | Every piece's carriers in contract order: pieces by kind, each piece's carriers normalized.
contractCarriers :: Composition k -> [(PieceKind, Span)]
contractCarriers composition =
  [(pieceKind piece, range) | piece <- sortOn pieceKind (compositionPieces composition), range <- normalizeSpans (pieceCarriers piece)]

contractValue :: JoinName k => Canonical -> B.ByteString -> Composition k -> B.ByteString -> TargetBytes -> Label -> Value -> Value
contractValue source manifest composition composedDat composedTargets attach attachState = object
  [ "schema" .= (2 :: Int)
  , "kind" .= ("composed-startup-contract" :: Text)
  , "qualification" .= (case qualification of HostComposed -> "host-composed" :: Text; Pending _ -> "pending")
  , "pending_joins" .= (case qualification of HostComposed -> [] :: [Text]; Pending names -> names)
  , "manifest_sha256" .= sha256 manifest
  , "canonical" .= identityValue (canonicalDat source) (canonicalTargets source)
  , "composed" .= identityValue composedDat composedTargets
  , "guard" .= object ["attach" .= labelText attach, "attach_state" .= attachState]
  , "joins" .= map joinValue (compositionJoins composition)
  , "pieces" .= map (pieceValue carriers) (sortOn pieceKind (compositionPieces composition))
  ]
  where
    qualification = compositionQualification composition
    carriers = concatMap pieceCarriers (compositionPieces composition)
