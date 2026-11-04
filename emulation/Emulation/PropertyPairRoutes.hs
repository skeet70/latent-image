{-# LANGUAGE OverloadedStrings #-}
-- | The fixed route set of the property-service natural pair and its selection from a menu crawl.
module Emulation.PropertyPairRoutes
  ( RouteName (..)
  , AttachPhase (..)
  , attachPhaseWire
  , DeclaredAction (..)
  , RouteOrigin (..)
  , RouteBody (..)
  , Route (..)
  , RouteFile (..)
  , EffectsSource (..)
  , Exclusion (..)
  , decodeRouteFile
  , encodeRouteFile
  , RetainedObservation (..)
  , EffectsRefusal (..)
  , retainedEffects
  , CrawlEffect (..)
  , CrawlEdge (..)
  , decodeCrawlEdges
  , selectCrawlRoutes
  , playbackExclusion
  , timeDrivenSequence
  , timeDrivenBlock
  , timeDrivenExclusion
  , declaredRoutes
  , phaseRoutes
  , routeKeys
  ) where

import Control.Monad (forM, unless, when)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), object, withObject, withText, (.:), (.:?), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.PropertyPairPlan (Role (..), SiteLabel (..), parseRole, parseSiteLabel)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex)
import EmulatorUsb (CrawlKey (..))
import Numeric (readHex, showHex)
import Text.Printf (printf)

-- | 1..48 lowercase ASCII letters, digits or '-'.
newtype RouteName = RouteName String deriving (Eq, Ord, Show)

instance FromJSON RouteName where
  parseJSON = withText "route name" $ \text -> do
    let name = T.unpack text
    unless (not (null name) && length name <= 48 && all (`elem` (['a' .. 'z'] ++ ['0' .. '9'] ++ "-")) name)
      (fail "route name must be 1..48 lowercase letters, digits or '-'")
    pure (RouteName name)

instance ToJSON RouteName where
  toJSON (RouteName name) = toJSON name

-- | The RAM-push bridge's reset-observation phases, each ending one route.
data AttachPhase = FirstStartPhase | LiveViewPhase | AttachedPhase deriving (Eq, Ord, Show, Enum, Bounded)

attachPhaseWire :: AttachPhase -> T.Text
attachPhaseWire phase = case phase of
  FirstStartPhase -> "first-start"
  LiveViewPhase -> "live-view"
  AttachedPhase -> "attached"

instance FromJSON AttachPhase where
  parseJSON = withText "attach phase" $ \text ->
    maybe (fail ("unknown attach phase: " ++ T.unpack text)) pure
      (lookup text [(attachPhaseWire phase, phase) | phase <- [minBound .. maxBound]])

instance ToJSON AttachPhase where
  toJSON = String . attachPhaseWire

-- | Routes that confirm a dialog the crawl left on CANCEL.
data DeclaredAction = CustomSetCopy deriving (Eq, Ord, Show, Enum, Bounded)

declaredWire :: DeclaredAction -> T.Text
declaredWire action = case action of
  CustomSetCopy -> "custom-set-copy"

-- | A crawl route carries the crawl edge's ordered effects.
data RouteOrigin = CrawlTrial Int [CrawlEffect] | Declared DeclaredAction deriving (Eq, Show)

-- | A phase route runs from the previous phase's end; a key route runs from the attached state.
data RouteBody = PhaseRoute AttachPhase | KeyRoute [CrawlKey] RouteOrigin deriving (Eq, Show)

data Route = Route { routeName :: RouteName, routeBody :: RouteBody } deriving (Eq, Show)

routeKeys :: Route -> [CrawlKey]
routeKeys route = case routeBody route of
  KeyRoute keys _ -> keys
  PhaseRoute _ -> []

data RouteFile = RouteFile
  { routeCrawlGraph :: String
  , routeSelection :: T.Text
  , routeEffectsSource :: EffectsSource
  , routeExclusions :: [Exclusion]
  , routeList :: [Route]
  } deriving (Eq, Show)

-- | Where the crawl routes' effects come from: the crawl edges, or the last steps of a retained
-- observation, bound by its receipt, steps and hits digests.
data EffectsSource
  = CrawlEffectsSource
  | RetainedObservationSource { sourceReceipt :: Sha256, sourceSteps :: Sha256, sourceHits :: Sha256 }
  deriving Eq

instance Show EffectsSource where
  show source = case source of
    CrawlEffectsSource -> "CrawlEffectsSource"
    RetainedObservationSource receipt steps hits -> unwords ["RetainedObservationSource", sha256Hex receipt, sha256Hex steps, sha256Hex hits]

instance FromJSON EffectsSource where
  parseJSON = withObject "effects source" $ \fields -> do
    kind <- fields .: "kind"
    case kind :: T.Text of
      "crawl" -> exactKeys ["kind"] fields >> pure CrawlEffectsSource
      "retained-observation" -> do
        exactKeys ["kind", "receipt_sha256", "steps_sha256", "hits_sha256"] fields
        let digest name = fields .: name >>= either fail pure . parseSha256Hex
        RetainedObservationSource <$> digest "receipt_sha256" <*> digest "steps_sha256" <*> digest "hits_sha256"
      _ -> fail "an effects source is crawl or retained-observation"

instance ToJSON EffectsSource where
  toJSON source = case source of
    CrawlEffectsSource -> object ["kind" .= ("crawl" :: T.Text)]
    RetainedObservationSource receipt steps hits -> object
      [ "kind" .= ("retained-observation" :: T.Text), "receipt_sha256" .= sha256Hex receipt
      , "steps_sha256" .= sha256Hex steps, "hits_sha256" .= sha256Hex hits ]

-- | Crawl edges with a property effect the selection leaves out, and why.
data Exclusion = Exclusion { exclusionReason :: T.Text, exclusionTrials :: [Int] } deriving (Eq, Show)

instance FromJSON Exclusion where
  parseJSON = withObject "route exclusion" $ \fields -> do
    exactKeys ["reason", "crawl_trials"] fields
    Exclusion <$> fields .: "reason" <*> fields .: "crawl_trials"

instance ToJSON Exclusion where
  toJSON (Exclusion reason trials) = object ["reason" .= reason, "crawl_trials" .= trials]

exactKeys :: [K.Key] -> KM.KeyMap Value -> Parser ()
exactKeys keys fields = unless (sort (KM.keys fields) == sort keys) (fail ("expected exactly " ++ show keys))

instance FromJSON Route where
  parseJSON = withObject "route" $ \fields -> do
    name <- fields .: "name"
    phase <- fields .:? "phase"
    case phase of
      Just attach -> exactKeys ["name", "phase"] fields >> pure (Route name (PhaseRoute attach))
      Nothing -> do
        keys <- fields .: "keys"
        when (null keys) (fail "a key route presses at least one key")
        trial <- fields .:? "crawl_trial"
        action <- fields .:? "declared"
        origin <- case (trial, action) of
          (Just number, Nothing) -> do
            exactKeys ["name", "keys", "crawl_trial", "effects"] fields
            CrawlTrial number <$> fields .: "effects"
          (Nothing, Just text) -> do
            exactKeys ["name", "keys", "declared"] fields
            maybe (fail "unknown declared action") (pure . Declared)
              (lookup (text :: T.Text) [(declaredWire declared, declared) | declared <- [minBound .. maxBound]])
          _ -> fail "a key route names exactly one of crawl_trial and declared"
        pure (Route name (KeyRoute keys origin))

instance ToJSON Route where
  toJSON (Route name body) = object $ ("name" .= name) : case body of
    PhaseRoute phase -> ["phase" .= phase]
    KeyRoute keys (CrawlTrial trial effects) -> ["keys" .= keys, "crawl_trial" .= trial, "effects" .= effects]
    KeyRoute keys (Declared action) -> ["keys" .= keys, "declared" .= declaredWire action]

instance FromJSON RouteFile where
  parseJSON = withObject "route file" $ \fields -> do
    exactKeys ["schema", "crawl_graph_sha256", "selection", "effects_source", "exclusions", "routes"] fields
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported route file schema")
    file <- RouteFile <$> fields .: "crawl_graph_sha256" <*> fields .: "selection" <*> fields .: "effects_source"
      <*> fields .: "exclusions" <*> fields .: "routes"
    let names = map routeName (routeList file)
        phases = [phase | Route _ (PhaseRoute phase) <- routeList file]
    unless (S.size (S.fromList names) == length names) (fail "route names repeat")
    unless (phases == [minBound .. maxBound] && take 3 (routeList file) == [route | route@(Route _ (PhaseRoute _)) <- routeList file])
      (fail "the route file starts with exactly the three attach phases in order")
    when (routeEffectsSource file == CrawlEffectsSource && or [null effects | Route _ (KeyRoute _ (CrawlTrial _ effects)) <- routeList file])
      (fail "a crawl route with crawl effects has at least one effect")
    pure file

decodeRouteFile :: B.ByteString -> Either String RouteFile
decodeRouteFile = A.eitherDecodeStrict'

encodeRouteFile :: RouteFile -> BL.ByteString
encodeRouteFile file = A.encode (object
  [ "schema" .= (1 :: Int)
  , "crawl_graph_sha256" .= routeCrawlGraph file
  , "selection" .= routeSelection file
  , "effects_source" .= routeEffectsSource file
  , "exclusions" .= routeExclusions file
  , "routes" .= routeList file
  ]) <> "\n"

-- | The receipt, steps and hits sidecars of an observation directory.
data RetainedObservation = RetainedObservation { retainedReceipt :: B.ByteString, retainedSteps :: B.ByteString, retainedHits :: B.ByteString }

data EffectsRefusal
  = ObservationNotRetained Role
  | ObservationNotCompleted T.Text
  | LastStepMissing RouteName
  | ObservationShape String
  deriving (Eq, Show)

-- | Replaces each crawl route's effects with the setter-entry r0 values and commit entries, in hit
-- order, of the step that pressed the route's keys.
retainedEffects :: RetainedObservation -> [Route] -> Either EffectsRefusal (EffectsSource, [Route])
retainedEffects observation routes = do
  (roleText, status) <- shape (A.eitherDecodeStrict' (retainedReceipt observation) >>= parseEither receiptFields)
  role <- shape (parseRole roleText)
  unless (role == RetainedRole) (Left (ObservationNotRetained role))
  unless (status == "completed") (Left (ObservationNotCompleted status))
  steps <- shape (A.eitherDecodeStrict' (retainedSteps observation) >>= parseEither (mapM stepFields))
  let ranges = M.fromList steps
      hits = M.fromList (zip [0 :: Int ..] (filter (not . B.null) (C.lines (retainedHits observation))))
  replaced <- forM routes $ \route -> case routeBody route of
    KeyRoute keys (CrawlTrial trial _) -> do
      (from, to) <- maybe (Left (LastStepMissing (routeName route))) Right (M.lookup keys ranges)
      effects <- fmap concat . forM [from .. to - 1] $ \index -> do
        line <- maybe (Left (ObservationShape ("step hit range passes the hits: " ++ show index))) Right (M.lookup index hits)
        shape (A.eitherDecodeStrict' line >>= parseEither hitEffects)
      pure route { routeBody = KeyRoute keys (CrawlTrial trial effects) }
    _ -> pure route
  pure (RetainedObservationSource (sha256 (retainedReceipt observation)) (sha256 (retainedSteps observation)) (sha256 (retainedHits observation)), replaced)
 where
  shape :: Either String a -> Either EffectsRefusal a
  shape = either (Left . ObservationShape) Right
  receiptFields = withObject "receipt" $ \fields -> do
    outcome <- fields .: "outcome" :: Parser A.Object
    (,) <$> fields .: "role" <*> outcome .: "status"
  stepFields = withObject "step" $ \fields -> do
    keys <- fields .: "keys"
    range <- fields .: "hits" :: Parser [Int]
    case range of
      [from, to] | from <= to -> pure (keys :: [CrawlKey], (from, to))
      _ -> fail "step hit range is malformed"
  hitEffects = withObject "hit" $ \fields -> do
    site <- fields .: "site"
    case parseSiteLabel site of
      Just SetterEntry -> do
        registers <- fields .: "registers" :: Parser A.Object
        (: []) . SetterEffect <$> (registers .: "r0" >>= hexWord)
      Just PropertyCommit -> pure [CommitEffect]
      _ -> pure []

-- | A crawl edge's property effect in order: a setter entry with its property, or a commit entry.
data CrawlEffect = SetterEffect Word32 | CommitEffect deriving (Eq, Ord, Show)

instance FromJSON CrawlEffect where
  parseJSON = withObject "crawl effect" $ \fields -> do
    kind <- fields .: "kind"
    case kind :: T.Text of
      "setter" -> exactKeys ["kind", "property"] fields >> (fields .: "property" >>= \text -> SetterEffect <$> hexWord text)
      "commit" -> exactKeys ["kind"] fields >> pure CommitEffect
      _ -> fail "a crawl effect is a setter or a commit"

instance ToJSON CrawlEffect where
  toJSON effect = case effect of
    SetterEffect property -> object ["kind" .= ("setter" :: T.Text), "property" .= ("0x" ++ showHex property "")]
    CommitEffect -> object ["kind" .= ("commit" :: T.Text)]

hexWord :: T.Text -> Parser Word32
hexWord text = case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= (0xffffffff :: Integer) -> pure (fromIntegral value)
  _ -> fail "crawl effect argument is not a hexadecimal word"

data CrawlEdge = CrawlEdge
  { crawlTrial :: Int
  , crawlKeys :: [CrawlKey]
  , crawlEffects :: [CrawlEffect]
  } deriving (Eq, Show)

-- | The reached edges of a `crawl-graph.json`, each with the full key path from its root.
decodeCrawlEdges :: B.ByteString -> Either String [CrawlEdge]
decodeCrawlEdges bytes = do
  document <- A.eitherDecodeStrict' bytes
  flip parseEither document $ withObject "crawl graph document" $ \top -> do
    graph <- top .: "graph" :: Parser A.Object
    nodes <- graph .: "nodes" :: Parser [Value]
    paths <- fmap M.fromList . forM nodes $ withObject "crawl node" $ \node ->
      (,) <$> (node .: "id" :: Parser Int) <*> (node .: "path" :: Parser [CrawlKey])
    edges <- graph .: "edges" :: Parser [Value]
    fmap concat . forM edges $ withObject "crawl edge" $ \edge -> do
      trial <- edge .: "trial"
      from <- edge .: "from"
      key <- edge .: "key"
      reached <- edge .:? "to" :: Parser (Maybe Int)
      activity <- edge .: "activity" :: Parser A.Object
      effects <- activity .: "effects" :: Parser [Value]
      decoded <- fmap concat . forM effects $ withObject "crawl effect" $ \effect -> do
        site <- effect .: "site"
        arguments <- effect .: "arguments"
        case (site :: T.Text, arguments :: [T.Text]) of
          ("crawl-property-setter", property : _) -> (: []) . SetterEffect <$> hexWord property
          ("crawl-property-commit", _) -> pure [CommitEffect]
          _ -> pure []
      path <- maybe (fail "crawl edge leaves an unknown node") pure (M.lookup from paths)
      pure [CrawlEdge trial (path ++ [key]) decoded | reached /= Nothing]

-- | One route per distinct ordered effect sequence among edges with a property write or commit,
-- other than edges leaving the no-card playback screen and edges whose effects are only the
-- time-driven block: the fewest presses, then the lowest trial. Also the trials of the left-out
-- playback edges and of the left-out time-driven edges.
selectCrawlRoutes :: [CrawlEdge] -> ([Route], [Int], [Int])
selectCrawlRoutes edges =
  ( [ Route (RouteName (printf "crawl-trial-%05d" (crawlTrial edge))) (KeyRoute (crawlKeys edge) (CrawlTrial (crawlTrial edge) (crawlEffects edge)))
    | edge <- sortOn crawlTrial (M.elems chosen) ]
  , sort [crawlTrial edge | edge <- effective, fromPlayback edge]
  , sort [crawlTrial edge | edge <- effective, not (fromPlayback edge), timeDriven edge] )
 where
  effective = filter (not . null . crawlEffects) edges
  chosen = M.fromListWith shorter [(crawlEffects edge, edge) | edge <- effective, not (fromPlayback edge), not (timeDriven edge)]
  shorter a b = if rank a <= rank b then a else b
  rank edge = (length (crawlKeys edge), crawlTrial edge)
  fromPlayback edge = case reverse (crawlKeys edge) of
    _ : KeyPlay : _ -> True
    _ -> False
  timeDriven edge = all inBlock (crawlEffects edge)
  inBlock effect = case effect of
    SetterEffect property -> property `S.member` timeDrivenBlock
    CommitEffect -> False

-- | Setter properties written through 0x6309c4 (lr 0x630a04), value 0, bank 0, slot 1, in repeating
-- blocks of four in this order; the block recurs on 1,120 of crawl-4's 4,528 reached edges.
timeDrivenSequence :: [Word32]
timeDrivenSequence = [0x1c9, 0x1c8, 0x27f, 0x280]

timeDrivenBlock :: S.Set Word32
timeDrivenBlock = S.fromList timeDrivenSequence

-- | Why `selectCrawlRoutes` leaves out edges whose only effects are the time-driven block.
timeDrivenExclusion :: T.Text
timeDrivenExclusion = T.unwords
  [ "These edges write only the 0x1c9/0x1c8/0x27f/0x280 block, which recurs in blocks of four on 1,120 of"
  , "crawl-4's 4,528 reached edges; all of them exhausted their 90M budget in crawl-4, so whether their key"
  , "is accepted depends on render-drain timing. In natural-1 run 3, crawl-trial-01462's OK was not consumed"
  , "within 90M retained (32 block calls, as in crawl-4), while owned consumed it after 32.5M and opened"
  , "SHOOTING MENU (1,5,2), setting 0x278 and 0x279 on both banks, as stock crawl-4 trial 1364 does from the"
  , "same live view without the overlay; the getter (lr, r0-r3, result) sets agree with no conflicting result." ]

-- | Why `selectCrawlRoutes` leaves out edges from the screen PLAY opens without a card.
playbackExclusion :: T.Text
playbackExclusion = T.unwords
  [ "The screen PLAY opens without a card (NO MEMORY CARD) keeps rendering, so whether a key pressed on it is"
  , "pressed at all depends on render-drain timing. In natural-1 the PLAY step (back,left,ok,play) settled after"
  , "6.0M instructions retained and exhausted its 90M budget owned, leaving owned's next keys unpressed"
  , "(prior-render-pending). In that PLAY step retained's 4 setter calls are a prefix of owned's 28, whose extras"
  , "repeat the same 0x1c9/0x1c8/0x27f/0x280 block with result 1; the getter (lr, r0-r3, result) sets are equal"
  , "(99 and 464 calls, no conflicting result, first ordered difference at call 26 is task interleaving); neither"
  , "role makes a byte-changing settings write (1,357 and 9,499 same-value rows over the same 514 addresses)." ]

phaseRoutes :: [Route]
phaseRoutes =
  [ Route (RouteName "boot") (PhaseRoute FirstStartPhase)
  , Route (RouteName "start-menu") (PhaseRoute LiveViewPhase)
  , Route (RouteName "attach") (PhaseRoute AttachedPhase)
  ]

-- | SHOOTING MENU row 15 is CUSTOM SET (crawl-4 nodes 136/137); UP moves its dialog's cursor from
-- CANCEL to OK. SET-UP CUSTOM RESET is not a route: its confirming step calls the setter but
-- appends no area-3 record within the step.
declaredRoutes :: [Route]
declaredRoutes =
  [ Route (RouteName "custom-set-copy") (KeyRoute ([KeyOk] ++ replicate 13 KeyDown ++ [KeyOk, KeyUp, KeyOk]) (Declared CustomSetCopy)) ]
