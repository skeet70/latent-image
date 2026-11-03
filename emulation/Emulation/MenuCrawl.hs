{-# LANGUAGE OverloadedStrings #-}
-- | Depth-first crawl of the X-M1 screens reachable by physical keys. Every press starts from a
-- held guest snapshot of its source screen, so no press observes state left by a sibling branch.
-- After the depth-first pass, depth-limited screens that recorded edges reach from the root in
-- fewer presses are replayed along that path and expanded.
module Emulation.MenuCrawl
  ( CrawlKey (..)
  , crawlKeyWire
  , RootName
  , rootNameText
  , CrawlRoot (..)
  , ContextKeys (..)
  , CrawlConfig (..)
  , decodeCrawlConfig
  , FrameDigest (..)
  , ControllerCells (..)
  , controllerCount
  , Screen (..)
  , Driver (..)
  , CrawlSite (..)
  , crawlSites
  , crawlSiteAddress
  , crawlSiteLabel
  , Dispatch (..)
  , Effect (..)
  , Activity (..)
  , stepActivity
  , CellChange (..)
  , cellChanges
  , NodeId (..)
  , Frontier (..)
  , Node (..)
  , Match (..)
  , EdgeOutcome (..)
  , Edge (..)
  , ReplayOutcome (..)
  , Replay (..)
  , CrawlGraph (..)
  , shortestPaths
  , crawl
  , graphValue
  , edgeValue
  ) where

import Control.Monad (foldM, forM_, unless, when)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), eitherDecodeStrict', object, withObject, withText, (.:), (.:?), (.=))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.IORef
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word16, Word32)
import EmulatorUsb (MenuReading, ModeDial, SnapshotName (..), SnapshotPress (..), TraceHit (..), modeDialName)
import Numeric (showHex)

-- | A physical press the bridge accepts; MENU is OK and DISP is BACK on this body.
data CrawlKey = KeyUp | KeyDown | KeyLeft | KeyRight | KeyOk | KeyBack | KeyQ | KeyDialLeft | KeyDialRight | KeyPlay
  deriving (Eq, Ord, Show, Enum, Bounded)

crawlKeyWire :: CrawlKey -> String
crawlKeyWire key = case key of
  KeyUp -> "up"
  KeyDown -> "down"
  KeyLeft -> "left"
  KeyRight -> "right"
  KeyOk -> "ok"
  KeyBack -> "back"
  KeyQ -> "q"
  KeyDialLeft -> "dial-left"
  KeyDialRight -> "dial-right"
  KeyPlay -> "play"

instance FromJSON CrawlKey where
  parseJSON = withText "crawl key" $ \text ->
    case lookup (T.unpack text) [(crawlKeyWire key, key) | key <- [minBound .. maxBound]] of
      Just key -> pure key
      Nothing -> fail ("unsupported crawl key: " ++ T.unpack text)

instance ToJSON CrawlKey where
  toJSON = toJSON . crawlKeyWire

-- | 1..16 lowercase ASCII letters, digits or '-'.
newtype RootName = RootName String deriving (Eq, Ord, Show)

rootNameText :: RootName -> String
rootNameText (RootName name) = name

instance FromJSON RootName where
  parseJSON = withText "root name" $ \text -> do
    let name = T.unpack text
    unless (not (null name) && length name <= 16 && all (`elem` (['a' .. 'z'] ++ ['0' .. '9'] ++ "-")) name)
      (fail "root name must be 1..16 lowercase letters, digits or '-'")
    pure (RootName name)

instance ToJSON RootName where
  toJSON = toJSON . rootNameText

-- | A crawl start: the attach state, optionally with the mode dial turned first.
data CrawlRoot = CrawlRoot { rootName :: RootName, rootModeDial :: Maybe ModeDial } deriving (Eq, Show)

instance FromJSON CrawlRoot where
  parseJSON = withObject "crawl root" $ \root -> do
    unless (all (`elem` ["name", "mode_dial"]) (KM.keys root)) (fail "a crawl root takes only name and mode_dial")
    dial <- root .:? "mode_dial"
    CrawlRoot <$> root .: "name" <*> traverse modeDial dial
   where
    modeDial text = case lookup (T.unpack text) [(modeDialName position, position) | position <- [minBound .. maxBound]] of
      Just position -> pure position
      Nothing -> fail ("unsupported mode-dial position: " ++ T.unpack text)

-- | Keys pressed instead of the configured keys on screens where one controller holds one state.
data ContextKeys = ContextKeys { contextController :: Int, contextState :: Word16, contextKeys :: [CrawlKey] } deriving (Eq, Show)

instance FromJSON ContextKeys where
  parseJSON = withObject "context keys" $ \fields -> do
    unless (sort (KM.keys fields) == ["controller", "keys", "state"]) (fail "context keys take exactly controller, state and keys")
    context <- ContextKeys <$> fields .: "controller" <*> fields .: "state" <*> fields .: "keys"
    unless (contextController context >= 0 && contextController context < controllerCount) (fail "context controller must be 0..173")
    pure context

data CrawlConfig = CrawlConfig
  { configKeys :: [CrawlKey]
  , configContextKeys :: Maybe ContextKeys
  , configRoots :: [CrawlRoot]
  , configMaxTrials :: Int
  , configMaxNodes :: Int
  , configMaxDepth :: Int
  , configMaxSeconds :: Double
  } deriving (Eq, Show)

-- | The bridge holds at most 24 snapshots: the boot state, one root and one per open depth.
maxCrawlDepth :: Int
maxCrawlDepth = 20

instance FromJSON CrawlConfig where
  parseJSON = withObject "crawl config" $ \document -> do
    let required = ["keys", "max_depth", "max_nodes", "max_seconds", "max_trials", "roots", "schema"]
    unless (sort (filter (/= "context_keys") (KM.keys document)) == required)
      (fail "crawl config takes exactly schema, keys, roots, max_trials, max_nodes, max_depth and max_seconds, and optionally context_keys")
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported crawl config schema")
    config <- CrawlConfig <$> document .: "keys" <*> document .:? "context_keys" <*> document .: "roots" <*> document .: "max_trials"
      <*> document .: "max_nodes" <*> document .: "max_depth" <*> document .: "max_seconds"
    let distinct values = length values == M.size (M.fromList [(value, ()) | value <- values])
    unless (not (null (configKeys config)) && distinct (configKeys config)) (fail "crawl keys must be nonempty and distinct")
    case configContextKeys config of
      Just context -> unless (not (null (contextKeys context)) && distinct (contextKeys context) && all (`elem` configKeys config) (contextKeys context))
        (fail "context keys must be a nonempty distinct subset of the crawl keys")
      Nothing -> pure ()
    unless (not (null (configRoots config)) && distinct (map rootName (configRoots config)))
      (fail "crawl roots must be nonempty with distinct names")
    unless (configMaxTrials config > 0 && configMaxNodes config > 0 && configMaxSeconds config > 0)
      (fail "crawl budgets must be positive")
    unless (configMaxDepth config >= 1 && configMaxDepth config <= maxCrawlDepth)
      (fail ("max_depth must be 1.." ++ show maxCrawlDepth))
    pure config

-- | The keys a screen is crawled with.
keysFor :: CrawlConfig -> Screen -> [CrawlKey]
keysFor config screen = case configContextKeys config of
  Just context | stateOf (contextController context) == Just (contextState context) -> contextKeys context
  _ -> configKeys config
 where
  stateOf controller = case drop controller (halfwords (cellStates (screenCells screen))) of
    value : _ -> Just value
    [] -> Nothing

decodeCrawlConfig :: B.ByteString -> Either String CrawlConfig
decodeCrawlConfig = eitherDecodeStrict'

-- | SHA-256 of a captured frame's PNG, lowercase hex.
newtype FrameDigest = FrameDigest String deriving (Eq, Ord, Show)

-- | The 174 state halfwords at 0x01467588 and the 174 event halfwords at 0x014676e4.
data ControllerCells = ControllerCells { cellStates :: B.ByteString, cellEvents :: B.ByteString }
  deriving (Eq, Ord, Show)

controllerCount :: Int
controllerCount = 174

data Screen = Screen
  { screenMenu :: MenuReading
  , screenFrame :: FrameDigest
  , screenImage :: FilePath
  , screenCells :: ControllerCells
  -- | The 48 bytes of the open menu node descriptor.
  , screenNode :: Maybe B.ByteString
  } deriving (Eq, Show)

-- | The emulator operations a crawl uses. A capture label is lowercase ASCII.
data Driver = Driver
  { driverSave :: SnapshotName -> IO ()
  , driverRestore :: SnapshotName -> IO ()
  , driverDrop :: SnapshotName -> IO ()
  , driverPress :: SnapshotName -> CrawlKey -> IO SnapshotPress
  , driverModeDial :: ModeDial -> IO ()
  , driverObserve :: String -> IO Screen
  , driverTraceMark :: IO Int
  , driverTraceSince :: Int -> IO [TraceHit]
  , driverDiscardImage :: FilePath -> IO ()
  , driverClock :: IO Double
  , driverRecord :: Value -> IO ()
  }

-- | Code sites the crawl arms. Dispatch sites are inside 0xbfb028; the others are entries.
data CrawlSite
  = DispatchContext
  | DispatchAction
  | DispatchDirectState
  | PropertyWrite
  | PropertySetter
  | PropertyCommit
  | MailboxWrite
  | ApplicationSend
  deriving (Eq, Ord, Show, Enum, Bounded)

crawlSites :: [CrawlSite]
crawlSites = [minBound .. maxBound]

crawlSiteAddress :: CrawlSite -> Word32
crawlSiteAddress site = case site of
  DispatchContext -> 0x00bfb0a0
  DispatchAction -> 0x00bfb0c8
  DispatchDirectState -> 0x00bfb114
  PropertyWrite -> 0x0061c91c
  PropertySetter -> 0x00624694
  PropertyCommit -> 0x00d4fc74
  MailboxWrite -> 0x0063270c
  ApplicationSend -> 0x006334dc

crawlSiteLabel :: CrawlSite -> String
crawlSiteLabel site = case site of
  DispatchContext -> "crawl-dispatch-context"
  DispatchAction -> "crawl-dispatch-action"
  DispatchDirectState -> "crawl-dispatch-direct-state"
  PropertyWrite -> "crawl-property-write"
  PropertySetter -> "crawl-property-setter"
  PropertyCommit -> "crawl-property-commit"
  MailboxWrite -> "crawl-mailbox-write"
  ApplicationSend -> "crawl-application-send"

siteFromLabel :: String -> Maybe CrawlSite
siteFromLabel label = lookup label [(crawlSiteLabel site, site) | site <- crawlSites]

-- | One dispatcher entry with a nonnull cell table: at the prehook call r0 is the controller, r1
-- the state and r2 the event; the action is r3 at the following `blx r3` on the same core.
data Dispatch = Dispatch
  { dispatchCore :: Int
  , dispatchInstructions :: Integer
  , dispatchController :: Word32
  , dispatchState :: Word32
  , dispatchEvent :: Word32
  , dispatchAction :: Maybe Word32
  } deriving (Eq, Show)

-- | A non-dispatch site entry with r0-r3. At 0xbfb114 r3 is the stored state and the controller
-- is in r5, which the bridge does not record.
data Effect = Effect { effectSite :: CrawlSite, effectCore :: Int, effectInstructions :: Integer, effectArguments :: [Word32] }
  deriving (Eq, Show)

data Activity = Activity
  { activityDispatches :: [Dispatch]
  , activityEffects :: [Effect]
  -- | Action entries with no preceding unpaired context on their core.
  , activityOrphanActions :: [Word32]
  } deriving (Eq, Show)

-- | Pairs each action with the latest unpaired context hit on its core; only the prehook runs
-- between the two.
stepActivity :: [TraceHit] -> Activity
stepActivity hits = finish (foldl' step (M.empty, [], [], []) hits)
 where
  step (open, dispatches, effects, orphans) hit = case siteFromLabel (traceSite hit) of
    Just DispatchContext ->
      let dispatch = Dispatch (traceCore hit) (traceInstructions hit) (traceR0 hit) (traceR1 hit) (traceR2 hit) Nothing
      in (M.insert (traceCore hit) (length dispatches) open, dispatches ++ [dispatch], effects, orphans)
    Just DispatchAction -> case M.lookup (traceCore hit) open of
      Just index ->
        let annotate position dispatch = if position == index then dispatch { dispatchAction = Just (traceR3 hit) } else dispatch
        in (M.delete (traceCore hit) open, zipWith annotate [0 ..] dispatches, effects, orphans)
      Nothing -> (open, dispatches, effects, orphans ++ [traceR3 hit])
    Just site -> (open, dispatches, effects ++ [Effect site (traceCore hit) (traceInstructions hit) [traceR0 hit, traceR1 hit, traceR2 hit, traceR3 hit]], orphans)
    Nothing -> (open, dispatches, effects, orphans)
  finish (_, dispatches, effects, orphans) = Activity dispatches effects orphans

-- | A controller whose state or event halfword differs between two readings.
data CellChange = CellChange { changeController :: Int, changeBefore :: Word16, changeAfter :: Word16 } deriving (Eq, Show)

halfwords :: B.ByteString -> [Word16]
halfwords bytes = [fromIntegral (B.index bytes (2 * index)) .|. (fromIntegral (B.index bytes (2 * index + 1)) `shiftL` 8)
  | index <- [0 .. B.length bytes `div` 2 - 1]]

cellChanges :: B.ByteString -> B.ByteString -> [CellChange]
cellChanges before after = [CellChange controller old new | (controller, old, new) <- zip3 [0 ..] (halfwords before) (halfwords after), old /= new]

newtype NodeId = NodeId Int deriving (Eq, Ord, Show)

-- | Why a reached screen was not, or not completely, expanded.
data Frontier = DepthLimit | NodeLimit | TrialLimit | TimeLimit deriving (Eq, Show)

data Node = Node
  { nodeId :: NodeId
  , nodeRoot :: RootName
  , nodeDepth :: Int
  , nodePath :: [CrawlKey]
  , nodeScreen :: Screen
  , nodeFrames :: [FrameDigest]
  , nodeUntried :: [CrawlKey]
  , nodeFrontier :: Maybe Frontier
  } deriving (Eq, Show)

-- | How a reached screen was identified with an existing node.
data Match = NewScreen | SameFrame | SameMenuAndCells deriving (Eq, Show)

data EdgeOutcome
  = Reached
      { reachedNode :: NodeId
      , reachedMatch :: Match
      , reachedFrame :: FrameDigest
      , reachedSettle :: Value
      , reachedInstructions :: Integer
      , reachedStates :: [CellChange]
      , reachedEvents :: [CellChange]
      }
  | Stopped { stoppedFailure :: String, stoppedInstructions :: Integer }
  deriving (Eq, Show)

data Edge = Edge
  { edgeTrial :: Int
  , edgeFrom :: NodeId
  , edgeKey :: CrawlKey
  , edgeActivity :: Activity
  , edgeOutcome :: EdgeOutcome
  } deriving (Eq, Show)

-- | The result of replaying a shorter recorded path to a depth-limited screen.
data ReplayOutcome
  = Replayed
  | Diverged { divergedFrame :: FrameDigest, divergedImage :: FilePath }
  | ReplayStopped { replayStepsTaken :: Int, replayFailure :: String }
  deriving (Eq, Show)

data Replay = Replay { replayNode :: NodeId, replayPath :: [CrawlKey], replayOutcome :: ReplayOutcome }
  deriving (Eq, Show)

data CrawlGraph = CrawlGraph
  { graphNodes :: [Node]
  , graphEdges :: [Edge]
  , graphReplays :: [Replay]
  , graphTrials :: Int
  , graphReplayPresses :: Int
  , graphSeconds :: Double
  , graphLimit :: Maybe Frontier
  } deriving (Eq, Show)

data CrawlState = CrawlState
  { stateNodes :: M.Map NodeId Node
  , stateByFrame :: M.Map FrameDigest NodeId
  , stateByCells :: M.Map (MenuReading, B.ByteString) NodeId
  , stateEdges :: [Edge]
  , stateReplays :: [Replay]
  , stateTrials :: Int
  , stateReplayPresses :: Int
  , stateLimit :: Maybe Frontier
  }

-- | The fewest-press key path from the origin to each node over reached edges; ties go to the
-- earlier edge.
shortestPaths :: NodeId -> [Edge] -> M.Map NodeId [CrawlKey]
shortestPaths origin edges = go (M.singleton origin []) [origin]
 where
  outgoing = M.fromListWith (flip (++)) [(edgeFrom edge, [(edgeKey edge, reachedNode outcome)]) | edge <- edges, outcome@Reached {} <- [edgeOutcome edge]]
  go paths [] = paths
  go paths frontier = let (paths', next) = foldl' visit (paths, []) frontier in go paths' (reverse next)
  visit (paths, next) from = foldl' (step from) (paths, next) (M.findWithDefault [] from outgoing)
  step from (paths, next) (key, to)
    | M.member to paths = (paths, next)
    | otherwise = (M.insert to (paths M.! from ++ [key]) paths, to : next)

-- | Explores every configured root depth-first within the configured budgets.
crawl :: CrawlConfig -> Driver -> IO CrawlGraph
crawl config driver = do
  started <- driverClock driver
  state <- newIORef (CrawlState M.empty M.empty M.empty [] [] 0 0 Nothing)
  let boot = SnapshotName "boot"
  driverSave driver boot
  forM_ (zip [0 :: Int ..] (configRoots config)) $ \(index, root) -> do
    driverRestore driver boot
    mapM_ (driverModeDial driver) (rootModeDial root)
    _ <- driverTraceSince driver =<< driverTraceMark driver
    let snapshot = SnapshotName ("root-" ++ show index)
    driverSave driver snapshot
    screen <- driverObserve driver ("root-" ++ show index)
    (node, match) <- intern state root 0 [] screen
    when (match == NewScreen) $ do
      expand started state root node snapshot
      deepen started state root (nodeId node) snapshot S.empty
    driverDrop driver snapshot
  driverDrop driver boot
  ended <- driverClock driver
  final <- readIORef state
  pure CrawlGraph
    { graphNodes = M.elems (stateNodes final)
    , graphEdges = reverse (stateEdges final)
    , graphReplays = reverse (stateReplays final)
    , graphTrials = stateTrials final
    , graphReplayPresses = stateReplayPresses final
    , graphSeconds = ended - started
    , graphLimit = stateLimit final
    }
 where
  expand started state root node snapshot = do
    let keys = keysFor config (nodeScreen node)
    untried <- foldM (tryKey started state root node snapshot) [] keys
    unless (null untried) $ do
      limit <- stateLimit <$> readIORef state
      updateNode state (nodeId node) (\current -> current { nodeUntried = reverse untried, nodeFrontier = limit })

  budgetLimit started current now
    | isJust (stateLimit current) = stateLimit current
    | stateTrials current >= configMaxTrials config = Just TrialLimit
    | now - started >= configMaxSeconds config = Just TimeLimit
    | otherwise = Nothing

  tryKey started state root node snapshot untried key = do
    current <- readIORef state
    now <- driverClock driver
    case budgetLimit started current now of
      Just reason -> do
        modifyIORef' state (\s -> s { stateLimit = Just reason })
        pure (key : untried)
      Nothing -> do
        mark <- driverTraceMark driver
        pressed <- driverPress driver snapshot key
        activity <- stepActivity <$> driverTraceSince driver mark
        trial <- stateTrials <$> readIORef state
        modifyIORef' state (\s -> s { stateTrials = trial + 1 })
        case pressed of
          SnapshotStopped failure instructions ->
            record state (Edge trial (nodeId node) key activity (Stopped failure instructions))
          SnapshotPressed _ settle instructions -> do
            screen <- driverObserve driver ("trial-" ++ show trial)
            (child, match) <- intern state root (nodeDepth node + 1) (nodePath node ++ [key]) screen
            let before = screenCells (nodeScreen node)
                after = screenCells screen
            record state (Edge trial (nodeId node) key activity (Reached (nodeId child) match (screenFrame screen) settle instructions
              (cellChanges (cellStates before) (cellStates after)) (cellChanges (cellEvents before) (cellEvents after))))
            let reopens = match /= NewScreen && nodeFrontier child == Just DepthLimit && nodeDepth node + 1 < nodeDepth child
            when (match == NewScreen || reopens) $
              open started state root (nodeId child) (nodePath node ++ [key]) screen reopens
        pure untried

  -- Places a node at a path and, unless a limit applies, expands it from the current guest state.
  open started state root identity path screen reopened = do
    let depth = length path
    stored <- (M.! identity) . stateNodes <$> readIORef state
    let baseline = screen { screenImage = screenImage (nodeScreen stored), screenFrame = screenFrame (nodeScreen stored) }
    updateNode state identity (\n -> n { nodeDepth = depth, nodePath = path, nodeScreen = baseline, nodeUntried = [], nodeFrontier = Nothing })
    when reopened $
      driverRecord driver . (\n -> object ["node" .= nodeValue n]) . (M.! identity) . stateNodes =<< readIORef state
    nodes <- M.size . stateNodes <$> readIORef state
    if depth >= configMaxDepth config
      then updateNode state identity (\n -> n { nodeUntried = keysFor config (nodeScreen n), nodeFrontier = Just DepthLimit })
      else if nodes > configMaxNodes config
        then updateNode state identity (\n -> n { nodeUntried = keysFor config (nodeScreen n), nodeFrontier = Just NodeLimit })
        else do
          let NodeId number = identity
              snapshot = SnapshotName ("node-" ++ show number)
          expanded <- (M.! identity) . stateNodes <$> readIORef state
          driverSave driver snapshot
          expand started state root expanded snapshot
          driverDrop driver snapshot

  -- Replays the shortest recorded path to each depth-limited screen of this root, nearest first,
  -- once per screen, until none is closer than its recorded depth or a budget runs out.
  deepen started state root origin rootSnapshot attempted = do
    current <- readIORef state
    now <- driverClock driver
    let paths = shortestPaths origin (reverse (stateEdges current))
        candidates = sort
          [ (length path, nodeId node, path)
          | node <- M.elems (stateNodes current), nodeRoot node == rootName root, nodeFrontier node == Just DepthLimit
          , S.notMember (nodeId node) attempted
          , Just path <- [M.lookup (nodeId node) paths], length path < nodeDepth node, length path < configMaxDepth config ]
    case (candidates, budgetLimit started current now) of
      ([], _) -> pure ()
      (_, Just reason) -> modifyIORef' state (\s -> s { stateLimit = Just reason })
      ((_, identity, path) : _, Nothing) -> do
        replay started state root identity path rootSnapshot
        deepen started state root origin rootSnapshot (S.insert identity attempted)

  replay started state root identity path rootSnapshot = do
    let NodeId number = identity
    walked <- walk number rootSnapshot Nothing (zip [0 ..] path)
    case walked of
      Left (steps, failure) -> note (Replay identity path (ReplayStopped steps failure))
      Right () -> do
        screen <- driverObserve driver ("replay-" ++ show number)
        current <- readIORef state
        let target = stateNodes current M.! identity
            frame = screenFrame screen
            known = M.lookup frame (stateByFrame current)
            sameCells = (screenMenu screen, cellStates (screenCells screen)) == (screenMenu (nodeScreen target), cellStates (screenCells (nodeScreen target)))
        -- The frame decides first, as in intern.
        case known of
          Just owner | owner == identity -> do
            driverDiscardImage driver (screenImage screen)
            note (Replay identity path Replayed)
            open started state root identity path screen True
          Nothing | sameCells -> do
            modifyIORef' state (\s -> s
              { stateByFrame = M.insert frame identity (stateByFrame s)
              , stateNodes = M.adjust (\n -> n { nodeFrames = nodeFrames n ++ [frame] }) identity (stateNodes s) })
            note (Replay identity path Replayed)
            open started state root identity path screen True
          _ -> note (Replay identity path (Diverged frame (screenImage screen)))
   where
    note entry = do
      modifyIORef' state (\s -> s { stateReplays = entry : stateReplays s })
      driverRecord driver (object ["replay" .= replayValue entry])
    -- Presses each key from the previous step's snapshot; the last step's guest state is kept live.
    walk _ _ held [] = mapM_ (driverDrop driver) held >> pure (Right ())
    walk number source held ((index, key) : rest) = do
      mark <- driverTraceMark driver
      pressed <- driverPress driver source key
      _ <- driverTraceSince driver mark
      mapM_ (driverDrop driver) held
      modifyIORef' state (\s -> s { stateReplayPresses = stateReplayPresses s + 1 })
      case pressed of
        SnapshotStopped failure _ -> pure (Left (index, failure))
        SnapshotPressed {}
          | null rest -> pure (Right ())
          | otherwise -> do
              let name = SnapshotName ("replay-" ++ show number ++ "-" ++ show index)
              driverSave driver name
              walk number name (Just name) rest

  record state edge = do
    modifyIORef' state (\s -> s { stateEdges = edge : stateEdges s })
    driverRecord driver (object ["edge" .= edgeValue edge])

  updateNode state identity change =
    modifyIORef' state (\s -> s { stateNodes = M.adjust change identity (stateNodes s) })

  -- The frame decides first; a frame-only difference under the same menu reading and state cells
  -- is a variant of the same screen.
  intern state root depth path screen = do
    current <- readIORef state
    let cellsKey = (screenMenu screen, cellStates (screenCells screen))
        frame = screenFrame screen
    case (M.lookup frame (stateByFrame current), M.lookup cellsKey (stateByCells current)) of
      (Just identity, _) -> do
        driverDiscardImage driver (screenImage screen)
        pure (stateNodes current M.! identity, SameFrame)
      (Nothing, Just identity) -> do
        let node = stateNodes current M.! identity
        writeIORef state current
          { stateByFrame = M.insert frame identity (stateByFrame current)
          , stateNodes = M.insert identity node { nodeFrames = nodeFrames node ++ [frame] } (stateNodes current)
          }
        pure (node, SameMenuAndCells)
      (Nothing, Nothing) -> do
        let identity = NodeId (M.size (stateNodes current))
            node = Node identity (rootName root) depth path screen [frame] [] Nothing
        writeIORef state current
          { stateNodes = M.insert identity node (stateNodes current)
          , stateByFrame = M.insert frame identity (stateByFrame current)
          , stateByCells = M.insert cellsKey identity (stateByCells current)
          }
        driverRecord driver (object ["node" .= nodeValue node])
        pure (node, NewScreen)

hex :: Integral a => a -> String
hex value = "0x" ++ showHex value ""

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\byte -> let digits = showHex byte "" in if length digits == 1 then '0' : digits else digits) . B.unpack

menuValue :: MenuReading -> Value
menuValue reading = case reading of
  Nothing -> Null
  Just (node, setting, cursor) -> object ["menu_id" .= node, "setting_id" .= setting, "cursor" .= cursor]

frontierText :: Frontier -> String
frontierText reason = case reason of
  DepthLimit -> "depth-limit"
  NodeLimit -> "node-limit"
  TrialLimit -> "trial-limit"
  TimeLimit -> "time-limit"

matchText :: Match -> String
matchText match = case match of
  NewScreen -> "new-screen"
  SameFrame -> "same-frame"
  SameMenuAndCells -> "same-menu-and-cells"

changeValue :: CellChange -> Value
changeValue (CellChange controller before after) = object ["controller" .= controller, "before" .= before, "after" .= after]

activityValue :: Activity -> Value
activityValue (Activity dispatches effects orphans) = object
  [ "dispatches" .= [object
      [ "core" .= dispatchCore dispatch, "instructions" .= dispatchInstructions dispatch, "controller" .= dispatchController dispatch
      , "state" .= dispatchState dispatch, "event" .= dispatchEvent dispatch, "action" .= fmap hex (dispatchAction dispatch) ]
      | dispatch <- dispatches]
  , "effects" .= [object
      [ "site" .= crawlSiteLabel (effectSite effect), "core" .= effectCore effect, "instructions" .= effectInstructions effect
      , "arguments" .= map hex (effectArguments effect) ]
      | effect <- effects]
  , "orphan_actions" .= map hex orphans
  ]

edgeValue :: Edge -> Value
edgeValue (Edge trial (NodeId from) key activity outcome) = object
  ([ "trial" .= trial, "from" .= from, "key" .= key, "activity" .= activityValue activity ] ++ case outcome of
    Reached (NodeId to) match (FrameDigest frame) settle instructions states events ->
      [ "to" .= to, "match" .= matchText match, "frame_sha256" .= frame, "settle" .= settle, "instructions" .= instructions
      , "state_changes" .= map changeValue states, "event_changes" .= map changeValue events ]
    Stopped failure instructions -> ["stopped" .= failure, "instructions" .= instructions])

replayValue :: Replay -> Value
replayValue (Replay (NodeId node) path outcome) = object
  ([ "node" .= node, "path" .= path ] ++ case outcome of
    Replayed -> [ "outcome" .= String "replayed" ]
    Diverged (FrameDigest frame) image -> [ "outcome" .= String "diverged", "frame_sha256" .= frame, "image" .= image ]
    ReplayStopped steps failure -> [ "outcome" .= String "stopped", "steps" .= steps, "stopped" .= failure ])

nodeValue :: Node -> Value
nodeValue node = object
  [ "id" .= let NodeId number = nodeId node in number
  , "root" .= nodeRoot node
  , "depth" .= nodeDepth node
  , "path" .= nodePath node
  , "menu" .= menuValue (screenMenu screen)
  , "frames" .= [frame | FrameDigest frame <- nodeFrames node]
  , "image" .= screenImage screen
  , "states" .= hexBytes (cellStates (screenCells screen))
  , "events" .= hexBytes (cellEvents (screenCells screen))
  , "menu_node_bytes" .= fmap hexBytes (screenNode screen)
  , "untried_keys" .= nodeUntried node
  , "frontier" .= fmap frontierText (nodeFrontier node)
  ]
 where
  screen = nodeScreen node

graphValue :: CrawlConfig -> CrawlGraph -> Value
graphValue config graph = object
  [ "schema" .= (1 :: Int)
  , "scope" .= String "emulated X-M1 1.50 screens reachable by physical keys from held snapshots; static listing semantics are not implied"
  , "keys" .= configKeys config
  , "context_keys" .= fmap (\context -> object ["controller" .= contextController context, "state" .= contextState context
      , "keys" .= contextKeys context]) (configContextKeys config)
  , "roots" .= [object ["name" .= rootName root, "mode_dial" .= fmap modeDialName (rootModeDial root)] | root <- configRoots config]
  , "budgets" .= object
      [ "max_trials" .= configMaxTrials config, "max_nodes" .= configMaxNodes config
      , "max_depth" .= configMaxDepth config, "max_seconds" .= configMaxSeconds config ]
  , "trace_sites" .= [object ["label" .= crawlSiteLabel site, "address" .= hex (crawlSiteAddress site)] | site <- crawlSites]
  , "trials" .= graphTrials graph
  , "replay_presses" .= graphReplayPresses graph
  , "seconds" .= graphSeconds graph
  , "limit" .= fmap frontierText (graphLimit graph)
  , "complete" .= (isNothing (graphLimit graph) && all (\node -> isNothing (nodeFrontier node)) (graphNodes graph))
  , "nodes" .= map nodeValue (graphNodes graph)
  , "edges" .= map edgeValue (graphEdges graph)
  , "replays" .= map replayValue (graphReplays graph)
  ]
