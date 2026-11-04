{-# LANGUAGE OverloadedStrings #-}
-- | The manifest's @guard.attach_state@ and its contract form. The stager tiles
-- each contract carrier with the declared residency parts and computes every
-- digest; emulation/fujimod-xm1/src/attach_state.rs checks the rest of the rules.
module Replacement.StartupArtifact.AttachState
  ( AttachRules, emptyAttachRules, parseAttachRules, AttachSources (..), attachStateValue
  , a32WordLoad, a32WordStore
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Bits (complement, shiftR, (.&.), (.|.), shiftL)
import qualified Data.Aeson as J
import Data.Aeson ((.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
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
import qualified A32 as A
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import qualified FujiAF.FirstStage as F
import Numeric (showHex)
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
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

-- | One role's expected register value. 'CallReturn' is the role's PC of a boundary plus 4, that boundary being a BL
-- to the attach PC; 'EntryState' is an owned loader orchestration's admitted register value as the named retained
-- service is entered (composed role only); 'RowValue' is resolved by the consumer from that role's earlier row.
data RegisterSource = SourceWord Word32 | CallReturn Label | EntryState LI.LoaderSide LI.Service | RowValue Label
  deriving (Eq, Show)

data RegisterEntry = RegisterEntry Core ContextRegister Word32 RegisterSource RegisterSource Text deriving (Eq, Show)

data BoardKind = BoardExact J.Value J.Value | BoardDeclared Text deriving (Eq, Show)

data BoardEntry = BoardEntry Text BoardKind Text deriving (Eq, Show)

data Role = RetainedRole | ComposedRole deriving (Eq, Ord, Show, Enum, Bounded)

-- | A stock site names its read PC; an owned site names the piece whose symbols carry its labelled read and store.
data SiteSource = StockSite Word32 | OwnedSite PieceKind deriving (Eq, Show)

data LogSite = LogSite
  { siteLabel :: Text
  , siteRole :: Role
  , siteCore :: Core
  , siteSource :: SiteSource
  , siteCitation :: Text
  } deriving (Eq, Show)

-- | The physical log span and its sites.
data LogRelation = LogRelation Word64 Word64 [LogSite] deriving (Eq, Show)

-- | Each present member carries its citation.
data Relations = Relations
  { timerRelation :: Maybe Text
  , logRelation :: Maybe LogRelation
  , lzRelation :: Maybe LzRelation
  } deriving (Eq, Show)

data LzRelation = LzRelation Text (Maybe SubmittedWord) deriving (Eq, Show)

-- | The physical record-1 word counting the record-4 stream bytes submitted to the LZ device.
data SubmittedWord = SubmittedWord
  { submittedAddress :: Word64
  , submittedCitation :: Text
  } deriving (Eq, Show)

data AttachRules = AttachRules [Peer] [CarrierPart] [Allowance] [RegisterEntry] [BoardEntry] Relations deriving (Eq, Show)

-- | No peers, residency parts, allowances or relations; only valid for compositions without peers or carriers.
emptyAttachRules :: AttachRules
emptyAttachRules = AttachRules [] [] [] [] [] (Relations Nothing Nothing Nothing)

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
        parsed <- forM rows (\row -> object ("peer." <> key) ["start", "end"] [] row >>= interval ("peer." <> key))
        when (or [next < end | ((_, end), (next, _)) <- zip parsed (drop 1 parsed)]) (refuse ("peer." <> key <> " is unsorted or overlapping"))
        pure parsed
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

-- | A piece-qualified boundary label.
labelOf :: Text -> Text -> Either Refusal Label
labelOf context value = case T.splitOn "/" value of
  [kindName, name] | [kind] <- [kind | kind <- [minBound .. maxBound], pieceKindName kind == kindName] ->
    either (const (refuse (context <> " is not a boundary label"))) pure (label kind name)
  _ -> refuse (context <> " is not a boundary label")

-- | Kebab spelling of the constructor name.
serviceName :: LI.Service -> Text
serviceName = T.pack . dropLeading . concatMap (\c -> if c >= 'A' && c <= 'Z' then ['-', toEnum (fromEnum c + 32)] else [c]) . show
  where
    dropLeading ('-' : rest) = rest
    dropLeading rest = rest

registerSource :: Text -> J.Value -> Either Refusal RegisterSource
registerSource context value = case value of
  J.Number _ -> SourceWord <$> integral context value
  J.Object fields | [key] <- KeyMap.keys fields -> case Key.toText key of
    "call_return" -> text (context <> ".call_return") (field fields "call_return") >>= fmap CallReturn . labelOf (context <> ".call_return")
    "row" -> text (context <> ".row") (field fields "row") >>= fmap RowValue . labelOf (context <> ".row")
    "entry_state" -> do
      inner <- object (context <> ".entry_state") ["side", "service"] [] (field fields "entry_state")
      sideName <- text (context <> ".entry_state.side") (field inner "side")
      side <- case sideName of
        "loader-a" -> pure LI.LoaderA
        "loader-b" -> pure LI.LoaderB
        _ -> refuse (context <> ".entry_state.side is not loader-a or loader-b")
      service <- named (context <> ".entry_state.service") serviceName (field inner "service")
      pure (EntryState side service)
    _ -> refuse (context <> " is not a register source")
  _ -> refuse (context <> " is not a register source")

registerEntry :: J.Value -> Either Refusal RegisterEntry
registerEntry value = do
  fields <- object "register" ["core", "register", "mask", "retained", "composed", "reason"] [] value
  RegisterEntry <$> core "register.core" (field fields "core") <*> named "register.register" registerName (field fields "register")
    <*> integral "register.mask" (field fields "mask") <*> registerSource "register.retained" (field fields "retained")
    <*> registerSource "register.composed" (field fields "composed") <*> text "register.reason" (field fields "reason")

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

roleName :: Role -> Text
roleName RetainedRole = "retained"
roleName ComposedRole = "composed"

citationMember :: Text -> KeyMap.KeyMap J.Value -> Text -> Either Refusal (Maybe Text)
citationMember context fields key
  | present fields key = do
      inner <- object (context <> "." <> key) ["citation"] [] (field fields key)
      Just <$> text (context <> "." <> key <> ".citation") (field inner "citation")
  | otherwise = pure Nothing

sourceOf :: Text -> J.Value -> Either Refusal SiteSource
sourceOf context value = case value of
  J.Object fields | [key] <- KeyMap.keys fields -> case Key.toText key of
    "stock" -> StockSite <$> integral (context <> ".stock") (field fields "stock")
    "owned" -> OwnedSite <$> named (context <> ".owned") pieceKindName (field fields "owned")
    _ -> refuse (context <> " is neither stock nor owned")
  _ -> refuse (context <> " is not one source")

logSite :: J.Value -> Either Refusal LogSite
logSite value = do
  fields <- object "log site" ["label", "role", "core", "source", "citation"] [] value
  name <- text "log site.label" (field fields "label")
  unless (kebab name) (refuse ("log site label " <> name <> " is not kebab-case"))
  let context key = "log site " <> name <> "." <> key
  LogSite name <$> named (context "role") roleName (field fields "role") <*> core (context "core") (field fields "core")
    <*> sourceOf (context "source") (field fields "source") <*> text (context "citation") (field fields "citation")

-- | @{"timer"?:{"citation"},"log"?:{"span":{start,end},"sites":[...]},"lz"?:{"citation","submitted"?:{"address","citation"}}}@.
relations :: J.Value -> Either Refusal Relations
relations value = do
  fields <- object "relations" [] ["timer", "log", "lz"] value
  timer <- citationMember "relations" fields "timer"
  lz <- if not (present fields "lz") then pure Nothing else do
    inner <- object "relations.lz" ["citation"] ["submitted"] (field fields "lz")
    citation <- text "relations.lz.citation" (field inner "citation")
    submitted <- if not (present inner "submitted") then pure Nothing else do
      word <- object "relations.lz.submitted" ["address", "citation"] [] (field inner "submitted")
      address <- integral "relations.lz.submitted.address" (field word "address")
      unless (address `mod` 4 == 0 && address + 4 <= 0x20000000) (refuse "relations.lz.submitted.address is unaligned or outside DRAM")
      Just . SubmittedWord address <$> text "relations.lz.submitted.citation" (field word "citation")
    pure (Just (LzRelation citation submitted))
  logged <- if not (present fields "log") then pure Nothing else do
    inner <- object "relations.log" ["span", "sites"] [] (field fields "log")
    (start, end) <- object "relations.log.span" ["start", "end"] [] (field inner "span") >>= interval "relations.log.span"
    unless (start `mod` 4 == 0 && end `mod` 4 == 0 && end - start <= 0x1000 && end <= 0x20000000)
      (refuse "relations.log.span is unaligned, above 4 KiB or outside DRAM")
    sites <- array "relations.log.sites" (field inner "sites") >>= mapM logSite
    when (length sites > 32) (refuse "relations.log has more than 32 sites")
    let labels = sort (map siteLabel sites)
    forM_ (zip labels (drop 1 labels)) $ \(left, right) -> when (left == right) (refuse ("log site label " <> left <> " repeats"))
    pure (Just (LogRelation start end sites))
  pure (Relations timer logged lz)

-- | @{"peers":[...],"carriers":[...],"allowances":[...],"registers":[...],"board":[...],"relations":{...}}@.
parseAttachRules :: J.Value -> Either Refusal AttachRules
parseAttachRules value = do
  fields <- object "attach_state" ["peers", "carriers", "allowances", "registers", "board", "relations"] [] value
  let rows key = array ("attach_state." <> key) (field fields key)
  allowances <- rows "allowances" >>= mapM allowance
  let labels = sort (map allowanceLabel allowances)
  forM_ (zip labels (drop 1 labels)) $ \(left, right) -> when (left == right) (refuse ("allowance label " <> left <> " repeats"))
  related <- relations (field fields "relations")
  forM_ (logRelation related) $ \(LogRelation start end _) ->
    forM_ allowances $ \entry -> when (allowanceSpace entry == PhysicalSpace && allowanceStart entry < end && start < allowanceEnd entry)
      (refuse ("allowance " <> allowanceLabel entry <> " overlaps the log span"))
  forM_ (lzRelation related) $ \(LzRelation _ submitted) -> forM_ submitted $ \(SubmittedWord address _) -> do
    let overlaps start end = start < address + 4 && address < end
    forM_ allowances $ \entry -> when (allowanceSpace entry == PhysicalSpace && overlaps (allowanceStart entry) (allowanceEnd entry))
      (refuse ("allowance " <> allowanceLabel entry <> " overlaps the LZ submitted word"))
    forM_ (logRelation related) $ \(LogRelation start end _) -> when (overlaps start end) (refuse "the LZ submitted word overlaps the log span")
  AttachRules <$> (rows "peers" >>= mapM peer) <*> (rows "carriers" >>= mapM carrierPart) <*> pure allowances
    <*> (rows "registers" >>= mapM registerEntry) <*> (rows "board" >>= mapM boardEntry) <*> pure related

-- | Both roles' DATs and targets, for the digests the contract carries, each owned piece's symbol table, the composed
-- pieces and the guard's attach boundary, for derived register values.
data AttachSources = AttachSources
  { canonicalDatBytes :: B.ByteString
  , composedDatBytes :: B.ByteString
  , canonicalTargetBytes :: TargetBytes
  , composedTargetBytes :: TargetBytes
  , ownedSymbols :: [(PieceKind, B.ByteString)]
  , composedPieces :: [Piece]
  , attachBoundary :: Label
  }

-- | A composed boundary's PC in one role.
rolePc :: Role -> Boundary -> Maybe Word32
rolePc role boundary = case role of
  RetainedRole -> (\(Pc pc) -> pc) <$> boundaryRetainedPc boundary
  ComposedRole -> let Pc pc = boundaryComposedPc boundary in Just pc

boundaryNamed :: AttachSources -> Label -> Maybe Boundary
boundaryNamed sources name = case [boundary | piece <- composedPieces sources, boundary <- catalogBoundaries (pieceCatalog piece), boundaryLabel boundary == name] of
  [boundary] -> Just boundary
  _ -> Nothing

registerReg :: ContextRegister -> Maybe A.Reg
registerReg register = lookup register ([(general, A.Reg index) | (general, index) <- zip [R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12] [0 ..]]
  ++ [(Sp, A.sp), (Lr, A.lr)])

-- | One role's expected value of a register entry, as the contract writes it, and the derivation it rests on.
resolveRegister :: AttachSources -> Core -> ContextRegister -> Role -> RegisterSource -> Either Refusal (J.Value, Maybe Text)
resolveRegister sources owner register role source = do
  attach <- maybe (refuse "the attach boundary is not in the composed catalog") pure (boundaryNamed sources (attachBoundary sources))
  attachPc <- maybe (refuse "the attach boundary has no PC in a role") pure (rolePc role attach)
  let image = case role of
        RetainedRole -> canonicalTargetBytes sources
        ComposedRole -> composedTargetBytes sources
      roleText = roleName role
  case source of
    SourceWord word -> pure (J.toJSON word, Nothing)
    RowValue name -> do
      boundary <- maybe (refuse ("register row " <> labelText name <> " is not a composed boundary")) pure (boundaryNamed sources name)
      unless (owner `elem` coreList (boundaryCores boundary) && rolePc role boundary /= Nothing)
        (refuse ("register row " <> labelText name <> " has no " <> roleText <> " PC on the register's core"))
      -- Only a sequence-compared boundary refuses when it fires past its record limit.
      unless (boundaryCompare boundary == CompareSequence)
        (refuse ("register row " <> labelText name <> " is not a sequence-compared boundary"))
      pure (J.object ["row" .= labelText name], Nothing)
    CallReturn name -> do
      boundary <- maybe (refuse ("register call_return " <> labelText name <> " is not a composed boundary")) pure (boundaryNamed sources name)
      unless (owner `elem` coreList (boundaryCores boundary))
        (refuse ("register call_return " <> labelText name <> " is not on the register's core"))
      pc <- maybe (refuse ("register call_return " <> labelText name <> " has no " <> roleText <> " PC")) pure (rolePc role boundary)
      case A.decode pc <$> targetWord image pc of
        Just (A.Branch (A.Cond 14) True target) | target == attachPc ->
          pure (J.toJSON (pc + 4), Just (roleText <> " " <> labelText name <> " BL 0x" <> hex pc <> " to the attach PC 0x" <> hex attachPc <> ", plus 4"))
        _ -> refuse ("register call_return " <> labelText name <> " is not a BL to the attach PC in the " <> roleText <> " image")
    EntryState side service -> do
      unless (role == ComposedRole) (refuse "register entry_state applies to the composed role only")
      reg <- maybe (refuse ("register " <> registerName register <> " has no admitted entry state")) pure (registerReg register)
      LinkedBytes bin symbols <- case [linked | piece <- composedPieces sources, linked <- orchestrationImages side (pieceFacts piece)] of
        [linked] -> pure linked
        _ -> refuse "register entry_state names a loader side no single composed piece owns"
      artifact <- either (const (refuse "register entry_state: the loader orchestration image is not admitted")) pure (L.deriveArtifact side bin symbols)
      let bootRole = case owner of Core0 -> LI.Primary; Core1 -> LI.Peer
          entries = [ entry | entry <- LA.serviceEntryStates (L.linkedProof artifact), LA.entryService entry == service
                    , let LI.Scenario scenarioRole _ _ = LA.entryScenario entry, scenarioRole == bootRole ]
          callsAttach entry = let LI.CodeAddress site = LA.entrySite entry in
            (A.decode site <$> targetWord image site) == Just (A.Branch (A.Cond 14) True attachPc)
      unless (not (null entries) && all callsAttach entries)
        (refuse ("register entry_state: the " <> serviceName service <> " entries are not BLs to the attach PC"))
      case map (`LA.entryRegister` reg) entries of
        LA.Word word : rest | all (== LA.Word word) rest ->
          pure (J.toJSON word, Just ("composed loader " <> sideName side <> " orchestration admitted " <> registerName register
            <> " at its " <> serviceName service <> " entry, a BL to the attach PC 0x" <> hex attachPc))
        _ -> refuse ("register entry_state " <> registerName register <> " is not one admitted word at the " <> serviceName service <> " entry")
  where
    hex value = T.pack (showHex value "")
    sideName side = case side of LI.LoaderA -> "A"; LI.LoaderB -> "B"
    orchestrationImages side facts = case (side, facts) of
      (LI.LoaderA, LoaderStartupFacts linked _ _) -> [linked]
      (LI.LoaderB, LoaderStartupFacts _ linked _) -> [linked]
      (LI.LoaderB, LoaderOrchestrationFacts linked) -> [linked]
      _ -> []

registerValue :: AttachSources -> RegisterEntry -> Either Refusal J.Value
registerValue sources (RegisterEntry owner register mask retained composed reason) = do
  (retainedValue, retainedDerivation) <- resolveRegister sources owner register RetainedRole retained
  (composedValue, composedDerivation) <- resolveRegister sources owner register ComposedRole composed
  let derivations = [text' | Just text' <- [retainedDerivation, composedDerivation]]
      reason' = if null derivations then reason else reason <> " (derived: " <> T.intercalate "; " derivations <> ")"
  pure (J.object ["core" .= coreIndex owner, "register" .= registerName register, "mask" .= mask
                 , "retained" .= retainedValue, "composed" .= composedValue, "reason" .= reason'])

-- | A32 @LDR Rt,...@ of a word, any addressing mode: not unconditional-space, not media-space (register form with bit 4 set).
a32WordLoad :: Word32 -> Bool
a32WordLoad word = word .&. 0x0c500000 == 0x04100000 && a32SingleTransfer word

-- | A32 @STR Rt,...@ of a word, any addressing mode: not unconditional-space, not media-space (register form with bit 4 set).
a32WordStore :: Word32 -> Bool
a32WordStore word = word .&. 0x0c500000 == 0x04000000 && a32SingleTransfer word

a32SingleTransfer :: Word32 -> Bool
a32SingleTransfer word = word `shiftR` 28 /= 0xf && word .&. 0x02000010 /= 0x02000010

-- | Labelled read and store of an owned piece's timestamp routine.
logSymbols :: PieceKind -> Maybe (String, String)
logSymbols kind = case kind of
  FirstStage -> Just ("xm1_first_stage_log_read", "xm1_first_stage_log_store")
  Record2Link -> Just ("xm1_secondary_log_read", "xm1_secondary_log_store")
  _ -> Nothing

targetWord :: TargetBytes -> Word32 -> Maybe Word32
targetWord targets address = case [ (target, bytes) | (target, bytes) <- targetList targets
                                  , targetBase target <= address
                                  , fromIntegral (address - targetBase target) + 4 <= B.length bytes ] of
  [(target, bytes)] -> let at = fromIntegral (address - targetBase target)
                       in Just (foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes (at + index))) 0 [0 .. 3])
  _ -> Nothing

-- | The site's read and store PCs, checked as an adjacent word load and store of one register in its role's image.
resolveSite :: AttachSources -> LogSite -> Either Refusal (Word32, Word32)
resolveSite sources site = do
  let name = siteLabel site
  (readPc, storePc) <- case siteSource site of
    StockSite pc -> pure (pc, pc + 4)
    OwnedSite kind -> do
      (readName, storeName) <- maybe (refuse ("log site " <> name <> " names a piece without log symbols")) pure (logSymbols kind)
      symbols <- maybe (refuse ("log site " <> name <> " names a piece the manifest lacks")) pure (lookup kind (ownedSymbols sources))
      rows <- either (const (refuse ("log site " <> name <> ": symbols do not parse"))) pure (F.parseSymbols symbols)
      let at symbol = either (const (refuse ("log site " <> name <> " lacks " <> T.pack symbol))) (pure . F.symbolAddress) (F.lookupSymbol rows symbol)
      (,) <$> at readName <*> at storeName
  unless (storePc == readPc + 4) (refuse ("log site " <> name <> " store is not the instruction after its read"))
  let image = case siteRole site of
        RetainedRole -> canonicalTargetBytes sources
        ComposedRole -> composedTargetBytes sources
  case (targetWord image readPc, targetWord image storePc) of
    (Just load, Just store) | a32WordLoad load && a32WordStore store && (load `shiftR` 12) .&. 0xf == (store `shiftR` 12) .&. 0xf ->
      pure (readPc, storePc)
    _ -> refuse ("log site " <> name <> " is not a word load and store of one register in its role's image")

relationsValue :: AttachSources -> Relations -> Either Refusal J.Value
relationsValue sources (Relations timer logged lz) = do
  logValue <- forM logged $ \(LogRelation start end sites) -> do
    resolved <- forM sites $ \site -> (,) site <$> resolveSite sources site
    let keys = sort [(siteRole site, siteCore site, readPc) | (site, (readPc, _)) <- resolved]
    forM_ (zip keys (drop 1 keys)) $ \(left, right) -> when (left == right) (refuse "two log sites share role, core and read PC")
    pure $ J.object
      [ "span" .= intervalValue (start, end)
      , "sites" .= [ J.object [ "label" .= siteLabel site, "role" .= roleName (siteRole site), "core" .= coreIndex (siteCore site)
                              , "read_pc" .= readPc, "store_pc" .= storePc, "citation" .= siteCitation site ]
                   | (site, (readPc, storePc)) <- resolved ]
      ]
  let citationValue = fmap (\citation -> J.object ["citation" .= citation])
  lzValue <- forM lz $ \(LzRelation citation submitted) -> do
    submittedValue <- forM submitted $ \(SubmittedWord address wordCitation) -> do
      retained <- record4Length (canonicalDatBytes sources)
      composed <- record4Length (composedDatBytes sources)
      pure (J.object ["address" .= address, "citation" .= wordCitation, "record_length" .= J.object ["retained" .= retained, "composed" .= composed]])
    pure (J.object (("citation" .= citation) : maybe [] (\value -> ["submitted" .= value]) submittedValue))
  pure (J.object ["timer" .= citationValue timer, "log" .= logValue, "lz" .= lzValue])

-- | The record-4 total length word, header included.
record4Length :: B.ByteString -> Either Refusal Word32
record4Length dat = case U.parseContainer dat of
  Left _ -> refuse "a DAT does not parse for its record-4 length"
  Right parsed -> pure (U.recTotalLen (U.recHeader (U.partRecord4 (U.parsedParts parsed))))

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
attachStateValue sources (AttachRules peers parts allowances registers board related) carriers = do
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
  registerValues <- mapM (registerValue sources) registers
  relationValue <- relationsValue sources related
  pure $ J.object
    [ "peers" .= [ J.object ["core" .= coreIndex owner, "retained_pc" .= map intervalValue retained, "composed_pc" .= map intervalValue composed, "reason" .= reason]
                 | Peer owner retained composed reason <- peers ]
    , "carriers" .= carrierValues
    , "allowances" .= allowanceValues
    , "registers" .= registerValues
    , "board" .= [ J.object (["pointer" .= pointer, "reason" .= reason] ++ case kind of
                     BoardExact retained composed -> ["kind" .= ("exact" :: Text), "retained" .= retained, "composed" .= composed]
                     BoardDeclared citation -> ["kind" .= ("declared" :: Text), "citation" .= citation])
                 | BoardEntry pointer kind reason <- board ]
    , "relations" .= relationValue
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
