{-# LANGUAGE OverloadedStrings #-}
-- | The manifest's @guard.attach_state@ and its contract form. The stager tiles
-- each contract carrier with the declared residency parts and computes every
-- digest; emulation/fujimod-xm1/src/attach_state.rs checks the rest of the rules.
module Replacement.StartupArtifact.AttachState
  ( AttachRules, emptyAttachRules, parseAttachRules, AttachSources (..), attachStateValue
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.Aeson as J
import Data.Aeson ((.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.List (sort, sortOn)
import Data.Maybe (fromMaybe)
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types

data Residency = Resident | Absent Text deriving (Eq, Show)

-- | A carrier sub-span in target coordinates.
data CarrierPart = CarrierPart Span Residency deriving (Eq, Show)

data Space = PhysicalSpace | TcmSpace Core TcmBank deriving (Eq, Show)

data Window = Window Space Word64 Word64 Text deriving (Eq, Show)

data Transform = IdentityTransform | ComplementTransform deriving (Eq, Show, Enum, Bounded)

data CopySource = DatSource Word64 Transform | TargetSource Target Word32 deriving (Eq, Show)

data ArmMode = Usr | Fiq | Irq | Svc | Abt | Und | Sys deriving (Eq, Show, Enum, Bounded)

data AllowanceKind
  = CopyAllowance CopySource
  | DeadStack Core ArmMode Window
  | DeadStackAtBoundary Core Text Window
  | PeerLiveStack Core Window
  | DeclaredAllowance Text
  deriving (Eq, Show)

data Allowance = Allowance
  { allowanceLabel :: Text
  , allowanceSpace :: Space
  , allowanceStart :: Word64
  , allowanceEnd :: Word64
  , allowanceReason :: Text
  , allowanceKind :: AllowanceKind
  } deriving (Eq, Show)

data Peer = Peer Core [(Word64, Word64)] [(Word64, Word64)] Text deriving (Eq, Show)

data ContextRegister
  = R0 | R1 | R2 | R3 | R4 | R5 | R6 | R7 | R8 | R9 | R10 | R11 | R12
  | Sp | Lr | PcRegister | Cpsr | Spsr | Cpacr | Fpexc | Fpscr | Sctlr | TcmData | TcmInstruction
  deriving (Eq, Ord, Show, Enum, Bounded)

data RegisterEntry = RegisterEntry Core ContextRegister Word32 Word32 Word32 Text deriving (Eq, Show)

data BoardKind = BoardExact J.Value J.Value | BoardDeclared Text deriving (Eq, Show)

data BoardEntry = BoardEntry Text BoardKind Text deriving (Eq, Show)

data AttachRules = AttachRules [Peer] [CarrierPart] [Allowance] [RegisterEntry] [BoardEntry] deriving (Eq, Show)

-- | No peers, residency parts or allowances; only valid for compositions without peers or carriers.
emptyAttachRules :: AttachRules
emptyAttachRules = AttachRules [] [] [] [] []

refuse :: Text -> Either Refusal a
refuse = Left . ManifestRefused . AttachStateRefused

object :: Text -> [Text] -> [Text] -> J.Value -> Either Refusal (KeyMap.KeyMap J.Value)
object context required optional value = case value of
  J.Object fields -> do
    forM_ (KeyMap.keys fields) $ \key ->
      unless (Key.toText key `elem` required ++ optional) (refuse (context <> "." <> Key.toText key <> " is unknown"))
    forM_ required $ \key ->
      unless (KeyMap.member (Key.fromText key) fields) (refuse (context <> " lacks " <> key))
    pure fields
  _ -> refuse (context <> " is not an object")

field :: KeyMap.KeyMap J.Value -> Text -> J.Value
field fields key = fromMaybe J.Null (KeyMap.lookup (Key.fromText key) fields)

present :: KeyMap.KeyMap J.Value -> Text -> Bool
present fields key = field fields key /= J.Null

text :: Text -> J.Value -> Either Refusal Text
text _ (J.String value) | not (T.null (T.strip value)) = pure value
text context _ = refuse (context <> " is not a nonempty string")

integral :: (Integral a, Bounded a) => Text -> J.Value -> Either Refusal a
integral context value = case value of
  J.Number number | Just parsed <- toBoundedInteger number -> pure parsed
  _ -> refuse (context <> " is not an integer in range")

array :: Text -> J.Value -> Either Refusal [J.Value]
array _ (J.Array items) = pure (V.toList items)
array context _ = refuse (context <> " is not an array")

named :: (Enum a, Bounded a) => Text -> (a -> Text) -> J.Value -> Either Refusal a
named context name value = do
  given <- text context value
  case [option | option <- [minBound .. maxBound], name option == given] of
    [option] -> pure option
    _ -> refuse (context <> " names no known value")

core :: Text -> J.Value -> Either Refusal Core
core context value = do
  index <- integral context value :: Either Refusal Int
  case index of
    0 -> pure Core0
    1 -> pure Core1
    _ -> refuse (context <> " is not core 0 or 1")

interval :: Text -> KeyMap.KeyMap J.Value -> Either Refusal (Word64, Word64)
interval context fields = do
  start <- integral (context <> ".start") (field fields "start")
  end <- integral (context <> ".end") (field fields "end")
  unless (start < end && end <= 0x100000000) (refuse (context <> " is empty or exceeds 32 bits"))
  pure (start, end)

modeName :: ArmMode -> Text
modeName mode = case mode of
  Usr -> "usr"; Fiq -> "fiq"; Irq -> "irq"; Svc -> "svc"; Abt -> "abt"; Und -> "und"; Sys -> "sys"

registerName :: ContextRegister -> Text
registerName register = case register of
  PcRegister -> "pc"
  TcmData -> "tcm-data"
  TcmInstruction -> "tcm-instruction"
  other -> T.toLower (T.pack (show other))

transformName :: Transform -> Text
transformName IdentityTransform = "identity"
transformName ComplementTransform = "complement"

space :: Text -> J.Value -> Either Refusal Space
space context value = do
  fields <- object context ["target"] ["core", "bank"] value
  target <- text (context <> ".target") (field fields "target")
  case target of
    "physical" | not (present fields "core" || present fields "bank") -> pure PhysicalSpace
    "tcm" -> TcmSpace <$> core (context <> ".core") (field fields "core") <*> named (context <> ".bank") tcmBankName (field fields "bank")
    _ -> refuse (context <> " is not a physical or TCM space")

window :: Text -> J.Value -> Either Refusal Window
window context value = do
  fields <- object context ["space", "start", "end", "citation"] [] value
  (start, end) <- interval context fields
  Window <$> space (context <> ".space") (field fields "space") <*> pure start <*> pure end <*> text (context <> ".citation") (field fields "citation")

kebab :: Text -> Bool
kebab name = not (T.null name) && T.all (\c -> isAsciiLower c || isDigit c || c == '-') name && T.head name /= '-' && T.last name /= '-'

-- | Keys each allowance kind takes beyond label, kind, space, start, end and reason.
kindKeys :: Text -> Maybe [Text]
kindKeys kind = case kind of
  "copy" -> Just ["source"]
  "dead-stack" -> Just ["core", "mode", "window"]
  "dead-stack-at-boundary" -> Just ["core", "boundary", "window"]
  "peer-live-stack" -> Just ["core", "window"]
  "declared" -> Just ["citation"]
  _ -> Nothing

allowance :: J.Value -> Either Refusal Allowance
allowance value = do
  base <- object "allowance" ["label", "kind", "space", "start", "end", "reason"] ["source", "core", "mode", "boundary", "window", "citation"] value
  name <- text "allowance.label" (field base "label")
  unless (kebab name) (refuse ("allowance label " <> name <> " is not kebab-case"))
  kind <- text "allowance.kind" (field base "kind")
  keys <- maybe (refuse ("allowance " <> name <> " has an unknown kind")) pure (kindKeys kind)
  fields <- object ("allowance " <> name) (["label", "kind", "space", "start", "end", "reason"] ++ keys) [] value
  (start, end) <- interval ("allowance " <> name) fields
  where' <- space ("allowance " <> name <> ".space") (field fields "space")
  reason <- text ("allowance " <> name <> ".reason") (field fields "reason")
  let at key = field fields key
      context key = "allowance " <> name <> "." <> key
  parsed <- case kind of
    "copy" -> CopyAllowance <$> copySource (context "source") (at "source")
    "dead-stack" -> DeadStack <$> core (context "core") (at "core") <*> named (context "mode") modeName (at "mode") <*> window (context "window") (at "window")
    "dead-stack-at-boundary" -> DeadStackAtBoundary <$> core (context "core") (at "core") <*> text (context "boundary") (at "boundary") <*> window (context "window") (at "window")
    "peer-live-stack" -> PeerLiveStack <$> core (context "core") (at "core") <*> window (context "window") (at "window")
    _ -> DeclaredAllowance <$> text (context "citation") (at "citation")
  case parsed of
    DeclaredAllowance _ -> when (end - start > 0x10000) (refuse ("allowance " <> name <> " declares more than 64 KiB"))
    _ -> pure ()
  pure (Allowance name where' start end reason parsed)

copySource :: Text -> J.Value -> Either Refusal CopySource
copySource context value = case value of
  J.Object fields | [key] <- KeyMap.keys fields -> case Key.toText key of
    "dat" -> do
      dat <- object (context <> ".dat") ["offset", "transform"] [] (field fields "dat")
      DatSource <$> integral (context <> ".offset") (field dat "offset") <*> named (context <> ".transform") transformName (field dat "transform")
    "target" -> do
      inner <- object (context <> ".target") ["target", "start"] [] (field fields "target")
      TargetSource <$> named (context <> ".target") targetName (field inner "target") <*> integral (context <> ".start") (field inner "start")
    _ -> refuse (context <> " is neither a DAT nor a target source")
  _ -> refuse (context <> " is not one source")

peer :: J.Value -> Either Refusal Peer
peer value = do
  fields <- object "peer" ["core", "retained_pc", "composed_pc", "reason"] [] value
  let spans key = do
        rows <- array ("peer." <> key) (field fields key)
        when (null rows) (refuse ("peer." <> key <> " is empty"))
        forM rows (\row -> object ("peer." <> key) ["start", "end"] [] row >>= interval ("peer." <> key))
  Peer <$> core "peer.core" (field fields "core") <*> spans "retained_pc" <*> spans "composed_pc" <*> text "peer.reason" (field fields "reason")

carrierPart :: J.Value -> Either Refusal CarrierPart
carrierPart value = do
  fields <- object "carrier" ["target", "start", "end", "residency"] ["reason"] value
  target <- named "carrier.target" targetName (field fields "target")
  (start, end) <- interval "carrier" fields
  residency <- text "carrier.residency" (field fields "residency")
  parsed <- case (residency, present fields "reason") of
    ("resident", False) -> pure Resident
    ("absent", True) -> Absent <$> text "carrier.reason" (field fields "reason")
    _ -> refuse "carrier residency is resident without a reason or absent with one"
  range <- maybe (refuse "carrier part is empty") pure (span' target (fromIntegral start) (fromIntegral end))
  pure (CarrierPart range parsed)

registerEntry :: J.Value -> Either Refusal RegisterEntry
registerEntry value = do
  fields <- object "register" ["core", "register", "mask", "retained", "composed", "reason"] [] value
  RegisterEntry <$> core "register.core" (field fields "core") <*> named "register.register" registerName (field fields "register")
    <*> integral "register.mask" (field fields "mask") <*> integral "register.retained" (field fields "retained")
    <*> integral "register.composed" (field fields "composed") <*> text "register.reason" (field fields "reason")

boardEntry :: J.Value -> Either Refusal BoardEntry
boardEntry value = do
  fields <- object "board" ["pointer", "kind", "reason"] ["retained", "composed", "citation"] value
  pointer <- text "board.pointer" (field fields "pointer")
  unless ("/" `T.isPrefixOf` pointer) (refuse ("board pointer " <> pointer <> " is not a JSON pointer"))
  kind <- text "board.kind" (field fields "kind")
  parsed <- case (kind, KeyMap.lookup "retained" fields, KeyMap.lookup "composed" fields, present fields "citation") of
    ("exact", Just retained, Just composed, False) -> pure (BoardExact retained composed)
    ("declared", Nothing, Nothing, True) -> BoardDeclared <$> text "board.citation" (field fields "citation")
    _ -> refuse ("board entry " <> pointer <> " has fields of another kind")
  BoardEntry pointer parsed <$> text "board.reason" (field fields "reason")

-- | @{"peers":[...],"carriers":[...],"allowances":[...],"registers":[...],"board":[...]}@.
parseAttachRules :: J.Value -> Either Refusal AttachRules
parseAttachRules value = do
  fields <- object "attach_state" ["peers", "carriers", "allowances", "registers", "board"] [] value
  let rows key = array ("attach_state." <> key) (field fields key)
  allowances <- rows "allowances" >>= mapM allowance
  let labels = sort (map allowanceLabel allowances)
  forM_ (zip labels (drop 1 labels)) $ \(left, right) -> when (left == right) (refuse ("allowance label " <> left <> " repeats"))
  AttachRules <$> (rows "peers" >>= mapM peer) <*> (rows "carriers" >>= mapM carrierPart) <*> pure allowances
    <*> (rows "registers" >>= mapM registerEntry) <*> (rows "board" >>= mapM boardEntry)

-- | Both roles' DATs and targets, for the digests the contract carries.
data AttachSources = AttachSources
  { canonicalDatBytes :: B.ByteString
  , composedDatBytes :: B.ByteString
  , canonicalTargetBytes :: TargetBytes
  , composedTargetBytes :: TargetBytes
  }

sha256 :: B.ByteString -> Text
sha256 = T.pack . SHA.sha256Hex . SHA.sha256

slice :: Int -> Int -> B.ByteString -> Maybe B.ByteString
slice offset count bytes
  | offset >= 0 && count >= 0 && offset + count <= B.length bytes = Just (B.take count (B.drop offset bytes))
  | otherwise = Nothing

targetSlice :: TargetBytes -> Span -> Maybe B.ByteString
targetSlice targets range@(Span target (Guest start) _) =
  slice (fromIntegral (start - targetBase target)) (spanLength range) (targetOf targets target)

spaceValue :: Space -> J.Value
spaceValue PhysicalSpace = J.object ["target" .= ("physical" :: Text)]
spaceValue (TcmSpace owner bank) = J.object ["target" .= ("tcm" :: Text), "core" .= coreIndex owner, "bank" .= tcmBankName bank]

intervalValue :: (Word64, Word64) -> J.Value
intervalValue (start, end) = J.object ["start" .= start, "end" .= end]

windowValue :: Window -> J.Value
windowValue (Window where' start end citation) = J.object ["space" .= spaceValue where', "start" .= start, "end" .= end, "citation" .= citation]

-- | Contract @guard.attach_state@ for the composition's carriers, in contract order.
attachStateValue :: AttachSources -> AttachRules -> [(PieceKind, Span)] -> Either Refusal J.Value
attachStateValue sources (AttachRules peers parts allowances registers board) carriers = do
  let inside (CarrierPart part _) (_, carrier) = spanTarget part == spanTarget carrier
        && spanStart carrier <= spanStart part && spanEnd part <= spanEnd carrier
  forM_ parts $ \part@(CarrierPart range _) ->
    unless (any (inside part) carriers) (refuse ("residency part " <> T.pack (show range) <> " lies in no carrier"))
  carrierValues <- forM carriers $ \entry@(owner, carrier) -> do
    let mine = sortOn (\(CarrierPart range _) -> range) (filter (`inside` entry) parts)
        ends = [spanStart carrier] ++ [spanEnd range | CarrierPart range _ <- mine]
        starts = [spanStart range | CarrierPart range _ <- mine] ++ [spanEnd carrier]
    unless (not (null mine) && ends == starts) (refuse ("residency parts do not tile the " <> pieceKindName owner <> " carrier " <> T.pack (show carrier)))
    partValues <- forM mine $ \(CarrierPart range residency) -> case residency of
      Resident -> do
        before <- maybe (refuse "resident part lies outside its target") pure (targetSlice (canonicalTargetBytes sources) range)
        after <- maybe (refuse "resident part lies outside its target") pure (targetSlice (composedTargetBytes sources) range)
        pure (partValue range "resident" J.Null (J.toJSON (sha256 before)) (J.toJSON (sha256 after)))
      Absent reason -> pure (partValue range "absent" (J.toJSON reason) J.Null J.Null)
    let Span target (Guest start) (Guest end) = carrier
    pure (J.object ["piece" .= pieceKindName owner, "target" .= targetName target, "start" .= start, "end" .= end, "parts" .= partValues])
  allowanceValues <- mapM (allowanceValue sources) allowances
  pure $ J.object
    [ "peers" .= [ J.object ["core" .= coreIndex owner, "retained_pc" .= map intervalValue retained, "composed_pc" .= map intervalValue composed, "reason" .= reason]
                 | Peer owner retained composed reason <- peers ]
    , "carriers" .= carrierValues
    , "allowances" .= allowanceValues
    , "registers" .= [ J.object ["core" .= coreIndex owner, "register" .= registerName register, "mask" .= mask, "retained" .= retained, "composed" .= composed, "reason" .= reason]
                     | RegisterEntry owner register mask retained composed reason <- registers ]
    , "board" .= [ J.object (["pointer" .= pointer, "reason" .= reason] ++ case kind of
                     BoardExact retained composed -> ["kind" .= ("exact" :: Text), "retained" .= retained, "composed" .= composed]
                     BoardDeclared citation -> ["kind" .= ("declared" :: Text), "citation" .= citation])
                 | BoardEntry pointer kind reason <- board ]
    ]
  where
    partValue (Span _ (Guest start) (Guest end)) residency reason canonicalDigest composedDigest = J.object
      ["start" .= start, "end" .= end, "residency" .= (residency :: Text), "reason" .= reason
      , "canonical_sha256" .= canonicalDigest, "composed_sha256" .= composedDigest]

allowanceValue :: AttachSources -> Allowance -> Either Refusal J.Value
allowanceValue sources (Allowance name where' start end reason kind) = do
  let blank = Set.fromList ["citation", "source", "canonical_sha256", "composed_sha256", "core", "mode", "boundary", "window"] :: Set.Set Text
      common = ["label" .= name, "space" .= spaceValue where', "start" .= start, "end" .= end, "reason" .= reason]
      count = fromIntegral (end - start)
  (kindName, specific) <- case kind of
    CopyAllowance source -> do
      (before, after, sourceValue) <- case source of
        DatSource offset transform -> do
          let grab dat = maybe (refuse ("copy " <> name <> " lies outside the DAT")) pure (slice (fromIntegral offset) count dat)
              apply = if transform == ComplementTransform then B.map complement else id
          before <- apply <$> grab (canonicalDatBytes sources)
          after <- apply <$> grab (composedDatBytes sources)
          pure (before, after, J.object ["dat" .= J.object ["offset" .= offset, "transform" .= transformName transform]])
        TargetSource target targetStart -> do
          range <- maybe (refuse ("copy " <> name <> " is empty")) pure (span' target targetStart (targetStart + fromIntegral count))
          let grab targets = maybe (refuse ("copy " <> name <> " lies outside its target")) pure (targetSlice targets range)
          before <- grab (canonicalTargetBytes sources)
          after <- grab (composedTargetBytes sources)
          pure (before, after, J.object ["target" .= J.object ["target" .= targetName target, "start" .= targetStart]])
      pure ("copy", ["source" .= sourceValue, "canonical_sha256" .= sha256 before, "composed_sha256" .= sha256 after])
    DeadStack owner mode stack -> pure ("dead-stack", ["core" .= coreIndex owner, "mode" .= modeName mode, "window" .= windowValue stack])
    DeadStackAtBoundary owner boundary stack ->
      pure ("dead-stack-at-boundary", ["core" .= coreIndex owner, "boundary" .= boundary, "window" .= windowValue stack])
    PeerLiveStack owner stack -> pure ("peer-live-stack", ["core" .= coreIndex owner, "window" .= windowValue stack])
    DeclaredAllowance citation -> pure ("declared", ["citation" .= citation])
  let given = Set.fromList [Key.toText key | (key, _) <- specific]
      nulls = [Key.fromText key .= J.Null | key <- Set.toList (Set.difference blank given)]
  pure (J.object (common ++ ["kind" .= (kindName :: Text)] ++ specific ++ nulls))
