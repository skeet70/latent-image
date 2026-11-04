{-# LANGUAGE OverloadedStrings #-}
-- | composed-contract.json, schema 4, in the shape of
-- emulation/fujimod-xm1/tests/data/composed-contract-v4.json: every list is
-- sorted and non-overlapping, and every object carries all its keys.
module Replacement.StartupArtifact.Contract
  ( contractValue, normalizeFootprint, allowRanges, EmittedWrites (..), contractCarriers, declaredMutations
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
import Data.Word (Word16, Word32, Word64)
import qualified Replacement.StartupArtifact.Footprint.FirstStage as FirstStageFootprint
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

-- | The boot-ROM selection store commits decoder 0 to choice 1 (low window to
-- RAM) in both roles.
romSelection :: DeclaredMutation
romSelection = DecoderCommit (DecoderIndex 0) (DecoderChoice 0) (DecoderChoice 1) Core0 (Pc 0x800022a8)
  (Citation "boot ROM str r1,[r0,#8] at 0x800022a8, r0 = 0xfff98000 from 0x800022c4 (reports/bootrom-window.md:51); selection words 1 and 2 are choice 1 (emulation/fujimod-xm1/data/xm1-150.json:776-790)")

-- | The stock ATCM move from the catalog reset value: record 1 at 0x18c4 on
-- core 0, record 2 at 0x118c4 on core 1, each @orr #0x11@ over literal
-- 0x70000000 (decode/boot-40048.dis and boot-50048.dis 1584-1587, 1717).
stockAtcm :: Core -> Word32 -> DeclaredMutation
stockAtcm core writer = TcmRegionWrite core InstructionBank (TcmRegister 0xffff8011) (TcmRegister 0x70000011) (Pc writer)
  (Citation "stock mcr p15,0,r1,c9,c1,1 in service 0x18b8 of its record (reports/firmware-extraction.md:371); reset value catalog/soc/exr2/soc.json:24-27,51-54")

-- | Per role the mutations the guard admits before the attach-state boundary.
-- The composed role takes the first stage's own ATCM write in place of stock
-- record 1's; a first stage whose image lacks it declares none, so its MCR
-- refuses.
declaredMutations :: Composition k -> ([DeclaredMutation], [DeclaredMutation])
declaredMutations composition = (retained, composed)
  where
    retained = [romSelection, stockAtcm Core0 0x18c4, stockAtcm Core1 0x118c4]
    owned = [facts | FirstStageFacts _ linked <- map pieceFacts (compositionPieces composition), facts <- either (const []) pure (FirstStageFootprint.tcmRegionWrite linked)]
    firstStage = any ((== FirstStage) . pieceKind) (compositionPieces composition)
    composed = [romSelection] ++ (if firstStage then owned else [stockAtcm Core0 0x18c4]) ++ [stockAtcm Core1 0x118c4]

declarationValue :: DeclaredMutation -> Value
declarationValue declaration = case declaration of
  TcmRegionWrite core bank (TcmRegister previous) (TcmRegister next) (Pc writer) (Citation citation) -> object
    [ "kind" .= ("tcm-region" :: Text), "core" .= coreIndex core, "bank" .= tcmBankName bank
    , "previous" .= previous, "next" .= next, "writer" .= writer, "citation" .= citation ]
  DecoderCommit (DecoderIndex decoder) (DecoderChoice previous) (DecoderChoice next) core (Pc writer) (Citation citation) -> object
    [ "kind" .= ("decoder-commit" :: Text), "decoder" .= (decoder :: Word16), "previous" .= previous, "next" .= next
    , "core" .= coreIndex core, "writer" .= writer, "citation" .= citation ]

contractValue :: JoinName k => Canonical -> B.ByteString -> Composition k -> B.ByteString -> TargetBytes -> Label -> Value -> Value
contractValue source manifest composition composedDat composedTargets attach attachState = object
  [ "schema" .= (4 :: Int)
  , "kind" .= ("composed-startup-contract" :: Text)
  , "qualification" .= (case qualification of HostComposed -> "host-composed" :: Text; Pending _ -> "pending")
  , "pending_joins" .= (case qualification of HostComposed -> [] :: [Text]; Pending names -> names)
  , "manifest_sha256" .= sha256 manifest
  , "canonical" .= identityValue (canonicalDat source) (canonicalTargets source)
  , "composed" .= identityValue composedDat composedTargets
  , "guard" .= object
      [ "attach" .= labelText attach, "attach_state" .= attachState
      , "declared" .= object ["retained" .= map declarationValue retained, "composed" .= map declarationValue composed] ]
  , "joins" .= map joinValue (compositionJoins composition)
  , "pieces" .= map (pieceValue carriers) (sortOn pieceKind (compositionPieces composition))
  ]
  where
    qualification = compositionQualification composition
    carriers = concatMap pieceCarriers (compositionPieces composition)
    (retained, composed) = declaredMutations composition
