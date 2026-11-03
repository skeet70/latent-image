{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Counts the behaviours a menu crawl observed and maps the dispatched actions onto the
-- controller manifest and the callback shape clusters.
module Main where

import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, (.|.))
import Data.List (isPrefixOf, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing, mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype NodeId = NodeId Int deriving (Eq, Ord, Show)
newtype Target = Target Word32 deriving (Eq, Ord, Show)
newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)
newtype ShapeHash = ShapeHash String deriving (Eq, Ord, Show)
newtype FrameDigest = FrameDigest String deriving (Eq, Ord, Show)

-- | The last menu node the firmware stored, its setting id and cursor. The pointer at 0x9571d0 is
-- not cleared when a menu closes, so this reading alone does not say a menu is open.
data MenuReading = MenuReading { menuNode :: Word32, menuSetting :: Word32, menuCursor :: Word32 } deriving (Eq, Ord, Show)

data EffectSite = PropertyWrite | PropertySetter | PropertyCommit | MailboxWrite | ApplicationSend | DirectState
  deriving (Eq, Ord, Show, Enum, Bounded)

effectLabel :: EffectSite -> String
effectLabel site = case site of
  PropertyWrite -> "crawl-property-write"
  PropertySetter -> "crawl-property-setter"
  PropertyCommit -> "crawl-property-commit"
  MailboxWrite -> "crawl-mailbox-write"
  ApplicationSend -> "crawl-application-send"
  DirectState -> "crawl-dispatch-direct-state"

data Node = Node
  { nodeId :: NodeId
  , nodeMenu :: Maybe MenuReading
  , nodeDescriptor :: Maybe [Word32]
  , nodeFrames :: [FrameDigest]
  , nodeImage :: FilePath
  , nodePath :: [String]
  , nodeFrontier :: Maybe String
  , nodeStates :: B.ByteString
  }

data Dispatch = Dispatch { dispatchController :: ControllerId, dispatchAction :: Maybe Target }
data Effect = Effect { effectSite :: EffectSite, effectArguments :: [Word32] }
data Edge = Edge
  { edgeFrom :: NodeId
  , edgeKey :: String
  , edgeTo :: Maybe NodeId
  , edgeStopped :: Maybe String
  , edgeDispatches :: [Dispatch]
  , edgeEffects :: [Effect]
  }

hexWord :: String -> Either String Word32
hexWord ('0' : 'x' : digits) | not (null digits), [(value, "")] <- readHex digits, value <= 0xffffffff = Right (fromInteger value)
hexWord text = Left ("expected 0x-hex word: " ++ text)

hexBytes :: String -> Either String B.ByteString
hexBytes text
  | odd (length text) = Left "odd hex byte string"
  | otherwise = B.pack <$> mapM byte (pairs text)
 where
  pairs (a : b : rest) = [a, b] : pairs rest
  pairs _ = []
  byte digits = case readHex digits of [(value, "")] -> Right value; _ -> Left "invalid hex byte"

words32 :: B.ByteString -> [Word32]
words32 bytes = [foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3] | offset <- [0, 4 .. B.length bytes - 4]]

instance FromJSON MenuReading where
  parseJSON = withObject "menu" $ \fields -> MenuReading <$> fields .: "menu_id" <*> fields .: "setting_id" <*> fields .: "cursor"

instance FromJSON Node where
  parseJSON = withObject "node" $ \fields -> do
    descriptor <- fields .:? "menu_node_bytes" >>= traverse (either fail (pure . words32) . hexBytes)
    states <- fields .: "states" >>= either fail pure . hexBytes
    unless (B.length states == 2 * 174) (fail "a node records 174 state halfwords")
    Node <$> (NodeId <$> fields .: "id") <*> fields .:? "menu" <*> pure descriptor
      <*> (map FrameDigest <$> fields .: "frames") <*> fields .: "image" <*> fields .: "path" <*> fields .:? "frontier"
      <*> pure states

instance FromJSON Dispatch where
  parseJSON = withObject "dispatch" $ \fields -> do
    action <- fields .:? "action" >>= traverse (either fail (pure . Target) . hexWord)
    Dispatch <$> (ControllerId <$> fields .: "controller") <*> pure action

instance FromJSON Effect where
  parseJSON = withObject "effect" $ \fields -> do
    label <- fields .: "site"
    site <- maybe (fail ("unknown effect site " ++ label)) pure (lookup label [(effectLabel s, s) | s <- [minBound .. maxBound]])
    arguments <- fields .: "arguments" >>= mapM (either fail pure . hexWord)
    pure (Effect site arguments)

instance FromJSON Edge where
  parseJSON = withObject "edge" $ \fields -> do
    activity <- fields .: "activity"
    Edge <$> (NodeId <$> fields .: "from") <*> fields .: "key" <*> (fmap NodeId <$> fields .:? "to") <*> fields .:? "stopped"
      <*> activity .: "dispatches" <*> activity .: "effects"

data Graph = Graph { graphNodes :: [Node], graphEdges :: [Edge], graphComplete :: Bool, graphTrials :: Int, graphLimit :: Maybe String }

instance FromJSON Graph where
  parseJSON = withObject "crawl receipt" $ \receipt -> do
    graph <- receipt .: "graph"
    withObject "graph" (\fields -> Graph <$> fields .: "nodes" <*> fields .: "edges" <*> fields .: "complete"
      <*> fields .: "trials" <*> fields .:? "limit") graph

-- | Action-table targets per controller from the controller manifest.
newtype Ownership = Ownership (M.Map Target ControllerId)

instance FromJSON Ownership where
  parseJSON = withObject "manifest" $ \manifest -> do
    controllers <- manifest .: "controllers" :: Parser [J.Value]
    pairs <- concat <$> mapM controller controllers
    pure (Ownership (M.fromList pairs))
   where
    controller = withObject "controller" $ \fields -> do
      identity <- fields .: "id"
      tables <- fields .: "tables" :: Parser [J.Value]
      targets <- concat <$> mapM (withObject "table" (\table -> do
        kind <- table .: "kind"
        if kind == ("Actions" :: String)
          then table .: "words" >>= mapM (either fail pure . hexWord)
          else pure [])) tables
      pure [(Target target, ControllerId identity) | target <- targets, target /= 0]

data Shape = Shape { shapeHash :: ShapeHash, shapeConditional :: Bool }
data Scale = Scale { scaleShapes :: M.Map Target Shape, scaleSizes :: M.Map ShapeHash Int }

instance FromJSON Scale where
  parseJSON = withObject "scale" $ \scale -> do
    index <- scale .: "callback_index" >>= mapM (withObject "callback" (\fields -> do
      address <- fields .: "address" >>= either fail pure . hexWord
      shape <- Shape <$> (ShapeHash <$> fields .: "helper_bound_hash") <*> fields .: "return_proof_conditional"
      pure (Target address, shape)))
    clusters <- scale .: "helper_bound_clusters" >>= mapM (withObject "cluster" (\fields ->
      (,) <$> (ShapeHash <$> fields .: "hash") <*> fields .: "count"))
    pure (Scale (M.fromList index) (M.fromList clusters))

-- | The state of controller 74 (modal 0x4a), the dispatcher of MENU and S1 from live view.
topController :: Int
topController = 74

topState :: Node -> Word32
topState node = fromIntegral (B.index states (2 * topController)) .|. (fromIntegral (B.index states (2 * topController + 1)) `shiftL` 8)
 where
  states = nodeStates node

-- | Controller 74's states on menu screens: 0x18 on the menus MENU and Q open from live view,
-- 0x00 under the SET-UP pages (crawl-4 frames).
menuStates :: [Word32]
menuStates = [0x18, 0x00]

-- | A behaviour unit: the stored menu node and setting id of a screen in one of `menuStates`.
-- Screens in any other controller 74 state are outside menus and form no key.
data MenuKey = MenuKey Word32 Word32 deriving (Eq, Ord, Show)

keyOf :: Node -> Maybe MenuKey
keyOf node = case nodeMenu node of
  Just menu | topState node `elem` menuStates -> Just (MenuKey (menuNode menu) (menuSetting menu))
  _ -> Nothing

-- | Observed behaviour of one menu key, from the edges leaving its screens.
data MenuBehaviour = MenuBehaviour
  { behaviourCursors :: S.Set Word32
  , behaviourCursorMoves :: Int
  , behaviourSettingMoves :: Int
  , behaviourCommits :: Int
  , behaviourEnters :: S.Set MenuKey
  , behaviourOutside :: Int
  }

-- | The behaviour classes counted, decided in this order: a key whose cursor moves commit a
-- property is a value list, a toggle when it has two positions; otherwise one that opens keys
-- that are not its ancestors is a navigation list; otherwise one with a commit of its own setting
-- id is a confirmation; the rest are other.
data BehaviourClass = Toggle | ValueList | NavigationList | Confirmation | OtherMenu deriving (Eq, Ord, Show, Enum, Bounded)

classText :: BehaviourClass -> String
classText kind = case kind of
  Toggle -> "toggle"
  ValueList -> "value-list"
  NavigationList -> "navigation-list"
  Confirmation -> "confirmation"
  OtherMenu -> "other-menu"

classify :: MenuBehaviour -> BehaviourClass
classify behaviour
  | behaviourSettingMoves behaviour > 0 = if S.size (behaviourCursors behaviour) == 2 then Toggle else ValueList
  | not (S.null (behaviourEnters behaviour)) = NavigationList
  | behaviourCommits behaviour > 0 = Confirmation
  | otherwise = OtherMenu

commits :: Edge -> Bool
commits edge = any ((== PropertyCommit) . effectSite) (edgeEffects edge)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  graphPath <- required "--graph"
  scalePath <- required "--scale"
  manifestPath <- required "--manifest"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  graphBytes <- B.readFile graphPath
  scaleBytes <- B.readFile scalePath
  manifestBytes <- B.readFile manifestPath
  unless (sha256Hex (sha256 scaleBytes) == "f69a020dede2d506ce847a411a22af13bd9eaf970b7c924d6dc074e8fdf5967a") (fail "scale identity differs")
  unless (sha256Hex (sha256 manifestBytes) == "f27b35628371e150cdc0cfea84b11424b52c8022f7604f832a04bb890273cd67") (fail "manifest identity differs")
  graph <- either fail pure (J.eitherDecodeStrict graphBytes)
  Ownership owners <- either fail pure (J.eitherDecodeStrict manifestBytes)
  scale <- either fail pure (J.eitherDecodeStrict scaleBytes)
  let nodes = M.fromList [(nodeId node, node) | node <- graphNodes graph]
      edges = graphEdges graph
  rootTop <- case M.lookup (NodeId 0) nodes of
    Just root -> pure (topState root)
    Nothing -> fail "the graph has no root node 0"
  let nodeAt identity = M.lookup identity nodes
      keyAt identity = nodeAt identity >>= keyOf
      menuKeys = S.fromList (mapMaybe keyOf (graphNodes graph))
      screensOf menuKey = [node | node <- graphNodes graph, keyOf node == Just menuKey]
      -- A target whose recorded path is a prefix of the source's path is an ancestor in the crawl tree.
      returns edge = case (nodeAt (edgeFrom edge), edgeTo edge >>= nodeAt) of
        (Just from, Just to) -> nodePath to `isPrefixOf` nodePath from
        _ -> False
      behaviourOf menuKey@(MenuKey _ setting) =
        let own = [edge | edge <- edges, keyAt (edgeFrom edge) == Just menuKey]
            targetKey edge = edgeTo edge >>= keyAt
            cursorAt identity = menuCursor <$> (nodeAt identity >>= nodeMenu)
            moved edge = targetKey edge == Just menuKey && fmap cursorAt (edgeTo edge) /= Just (cursorAt (edgeFrom edge))
        in MenuBehaviour
          { behaviourCursors = S.fromList [menuCursor menu | node <- screensOf menuKey, Just menu <- [nodeMenu node]]
          , behaviourCursorMoves = length (filter moved own)
          , behaviourSettingMoves = length [() | edge <- own, moved edge, commits edge]
          , behaviourCommits = length [() | edge <- own, any (\effect -> effectSite effect == PropertyCommit && take 1 (effectArguments effect) == [setting]) (edgeEffects edge)]
          , behaviourEnters = S.fromList [target | edge <- own, not (returns edge), Just target <- [targetKey edge], target /= menuKey]
          , behaviourOutside = length [() | edge <- own, isNothing (edgeStopped edge), isJust (edgeTo edge), isNothing (targetKey edge)]
          }
      behaviours = M.fromSet behaviourOf menuKeys
      classes = M.map classify behaviours
      descriptorOf menuKey = case [words' | node <- screensOf menuKey, Just words' <- [nodeDescriptor node]] of
        words' : _ -> words'
        [] -> []
      firstImage menuKey = case map nodeImage (screensOf menuKey) of
        image : _ -> image
        [] -> ""
      typeWord menuKey = case descriptorOf menuKey of _ : kind : _ -> Just kind; _ -> Nothing
      outside = [node | node <- graphNodes graph, isNothing (keyOf node)]
      dispatches = concatMap edgeDispatches edges
      targets = S.fromList (mapMaybe dispatchAction dispatches)
      shapeOf target = M.lookup target (scaleShapes scale)
      shapes = S.fromList (map shapeHash (mapMaybe shapeOf (S.toList targets)))
      sizeOf hash = M.findWithDefault 0 hash (scaleSizes scale)
      familyTargets kind = S.fromList [action | edge <- edges, Just menuKey <- [keyAt (edgeFrom edge)]
        , M.lookup menuKey classes == Just kind, Dispatch _ (Just action) <- edgeDispatches edge]
      outsideTargets = S.fromList [action | edge <- edges, isNothing (keyAt (edgeFrom edge)), Dispatch _ (Just action) <- edgeDispatches edge]
      shapesOf actions = S.fromList (map shapeHash (mapMaybe shapeOf (S.toList actions)))
      familyValue actions = object
        [ "actions" .= S.size actions, "shapes" .= S.size (shapesOf actions), "members_of_shapes" .= sum (map sizeOf (S.toList (shapesOf actions))) ]
      properties sites = S.fromList [property | edge <- edges, Effect site (property : _) <- edgeEffects edge, site `elem` sites]
      keyValue (MenuKey node setting) = object ["menu_id" .= node, "setting_id" .= setting]
      result = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("behaviour census of one emulated crawl; classes are rule-based from observed edges, not firmware semantics" :: String)
        , "inputs" .= object
            [ "graph" .= graphPath, "graph_sha256" .= sha256Hex (sha256 graphBytes)
            , "scale_sha256" .= sha256Hex (sha256 scaleBytes), "manifest_sha256" .= sha256Hex (sha256 manifestBytes) ]
        , "crawl" .= object
            [ "complete" .= graphComplete graph, "trials" .= graphTrials graph, "limit" .= graphLimit graph
            , "screens" .= M.size nodes, "frames" .= S.size (S.fromList (concatMap nodeFrames (graphNodes graph)))
            , "edges" .= length edges, "stopped_edges" .= length [() | edge <- edges, Just _ <- [edgeStopped edge]]
            , "frontier_screens" .= length [() | node <- graphNodes graph, Just _ <- [nodeFrontier node]] ]
        , "menus" .= object
            [ "root_controller_74_state" .= rootTop, "menu_controller_74_states" .= menuStates
            , "menu_keys" .= S.size menuKeys
            , "by_class" .= object [ K.fromString (classText kind) .= length [() | c <- M.elems classes, c == kind] | kind <- [minBound .. maxBound] ]
            , "by_descriptor_type" .= M.toList (M.fromListWith (+) [(maybe "none" hex (typeWord menuKey), 1 :: Int) | menuKey <- S.toList menuKeys])
            , "keys" .= [ object
                [ "key" .= keyValue menuKey, "class" .= classText (classes M.! menuKey), "descriptor" .= map hex (descriptorOf menuKey)
                , "screens" .= length (screensOf menuKey)
                , "cursors" .= S.toList (behaviourCursors behaviour), "cursor_moves" .= behaviourCursorMoves behaviour
                , "setting_moves" .= behaviourSettingMoves behaviour, "commits" .= behaviourCommits behaviour
                , "enters" .= map keyValue (S.toList (behaviourEnters behaviour)), "edges_outside_menus" .= behaviourOutside behaviour
                , "image" .= firstImage menuKey ]
              | (menuKey, behaviour) <- M.toList behaviours ] ]
        , "outside_menu_screens" .= [ object
            [ "id" .= identity, "path" .= nodePath node, "image" .= nodeImage node, "frontier" .= nodeFrontier node
            , "controller_74_state" .= topState node
            , "menu" .= fmap (\menu -> object ["menu_id" .= menuNode menu, "setting_id" .= menuSetting menu, "cursor" .= menuCursor menu]) (nodeMenu node) ]
            | node <- outside, let NodeId identity = nodeId node ]
        , "callbacks" .= object
            [ "dispatches" .= length dispatches
            , "controllers_dispatched" .= S.toList (S.fromList [identity | Dispatch (ControllerId identity) _ <- dispatches])
            , "distinct_actions" .= S.size targets
            , "actions_outside_manifest" .= [hex target | Target target <- S.toList targets, isNothing (M.lookup (Target target) owners)]
            , "controllers_owning_actions" .= S.toList (S.fromList [identity | target <- S.toList targets, Just (ControllerId identity) <- [M.lookup target owners]])
            , "helper_bound_shapes" .= S.size shapes
            , "actions_in_repeated_shapes" .= length [() | target <- S.toList targets, Just shape <- [shapeOf target], sizeOf (shapeHash shape) > 1]
            , "members_of_reached_shapes" .= sum (map sizeOf (S.toList shapes))
            , "actions_with_unresolved_return" .= length [() | target <- S.toList targets, Just shape <- [shapeOf target], not (shapeConditional shape)]
            , "by_class" .= object ([ K.fromString (classText kind) .= familyValue (familyTargets kind) | kind <- [minBound .. maxBound] ]
                ++ [ "outside-menus" .= familyValue outsideTargets ])
            , "actions" .= [ object
                [ "target" .= hex target, "controller" .= fmap (\(ControllerId identity) -> identity) (M.lookup (Target target) owners)
                , "shape_members" .= maybe 0 (sizeOf . shapeHash) (shapeOf (Target target))
                , "invocations" .= count ]
              | (Target target, count) <- sortOn (negate . snd) (M.toList (M.fromListWith (+) [(action, 1 :: Int) | Dispatch _ (Just action) <- dispatches])) ] ]
        , "effects" .= object
            [ "properties_written" .= map hex (S.toList (properties [PropertyWrite, PropertySetter]))
            , "properties_committed" .= map hex (S.toList (properties [PropertyCommit]))
            , "mailbox_codes" .= map hex (S.toList (properties [MailboxWrite]))
            , "application_sends" .= map hex (S.toList (properties [ApplicationSend])) ]
        ]
  L.writeFile outputPath (J.encode result)
  putStrLn ("census: " ++ show (M.size nodes) ++ " screens, " ++ show (S.size menuKeys) ++ " menu keys, "
    ++ show (S.size targets) ++ " actions in " ++ show (S.size shapes) ++ " helper-bound shapes; "
    ++ unwords [classText kind ++ "=" ++ show (length [() | c <- M.elems classes, c == kind]) | kind <- [minBound .. maxBound]]
    ++ maybe "" (" limit=" ++) (graphLimit graph))
