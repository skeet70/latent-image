{-# LANGUAGE OverloadedStrings #-}
-- | The menu crawl over a synthetic screen machine reached through a fake bridge.
module Main (main) where

import Control.Monad (unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Aeson (Value (..))
import Data.IORef
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isNothing)
import Emulation.MenuCrawl
import qualified Emulation.RamDiff as RamDiff
import EmulatorUsb (ModeDial (..), SnapshotName (..), SnapshotPress (..), TraceHit (..))
import System.Exit (exitFailure)

-- | A synthetic screen: a menu reading, a frame number and a controller-cell value.
data Synthetic = Synthetic { syntheticMenu :: Int, syntheticFrame :: Int, syntheticCell :: Int } deriving (Eq, Ord, Show)

-- | The fake bridge's guest: the current screen plus the dial position.
data Guest = Guest { guestScreen :: Synthetic, guestDial :: Maybe ModeDial } deriving (Eq, Show)

-- | What a test reads back from the fake bridge after a crawl.
data Fake = Fake
  { fakeSnapshots :: IORef (M.Map SnapshotName Guest)
  , fakePeak :: IORef Int
  , fakeDiscarded :: IORef [FilePath]
  , fakeRecords :: IORef Int
  }

config :: [CrawlKey] -> Int -> Int -> Int -> CrawlConfig
config keys trials nodes depth = CrawlConfig keys Nothing [] [] [CrawlRoot (rootOf "default") Nothing] trials nodes depth 1000

rootOf :: String -> RootName
rootOf name = either error rootName (decodeRoot name)
 where
  decodeRoot text = decodeCrawlConfig (C.pack ("{\"schema\":1,\"keys\":[\"up\"],\"roots\":[{\"name\":\"" ++ text
    ++ "\"}],\"max_trials\":1,\"max_nodes\":1,\"max_depth\":1,\"max_seconds\":1}")) >>= \decoded -> case configRoots decoded of
      [root] -> Right root
      _ -> Left "one root expected"

-- | Builds a fake bridge over a transition function; Nothing makes the press a strict stop.
fake :: Synthetic -> (Maybe ModeDial -> Synthetic -> CrawlKey -> Maybe Synthetic) -> IO (Fake, Driver)
fake start step = do
  current <- newIORef (Guest start Nothing)
  snapshots <- newIORef M.empty
  peak <- newIORef 0
  hits <- newIORef []
  clock <- newIORef 0
  discarded <- newIORef []
  records <- newIORef 0
  let state = Fake snapshots peak discarded records
      held name = M.lookup name <$> readIORef snapshots >>= maybe (ioError (userError ("not held: " ++ show name))) pure
      driver = Driver
        { driverSave = \name -> do
            existing <- M.member name <$> readIORef snapshots
            when existing (ioError (userError "snapshot already held"))
            guest <- readIORef current
            modifyIORef' snapshots (M.insert name guest)
            count <- M.size <$> readIORef snapshots
            when (count > 24) (ioError (userError "more than 24 snapshots held"))
            modifyIORef' peak (max count)
        , driverRestore = \name -> held name >>= writeIORef current
        , driverDrop = \name -> do
            _ <- held name
            modifyIORef' snapshots (M.delete name)
        , driverPress = \name key -> do
            guest <- held name
            modifyIORef' clock (+ 1)
            modifyIORef' hits (++ [TraceHit "crawl-dispatch-context" 0 0 (fromIntegral (syntheticMenu (guestScreen guest))) 0 (fromIntegral (fromEnum key)) 0
                                  , TraceHit "crawl-dispatch-action" 0 0 0 0 0 (0x00c00000 + fromIntegral (fromEnum key))])
            case step (guestDial guest) (guestScreen guest) key of
              Just next -> do
                writeIORef current guest { guestScreen = next }
                pure (SnapshotPressed (Just (fromIntegral (syntheticMenu next), 0, 0)) Null 10)
              Nothing -> pure (SnapshotStopped "synthetic stop" 3)
        , driverModeDial = \position -> modifyIORef' current (\(Guest (Synthetic menu _ _) _) ->
            Guest (Synthetic (menu + 50) (menu + 50) (menu + 50)) (Just position))
        , driverObserve = \label -> do
            Guest screen _ <- readIORef current
            pure (Screen (Just (fromIntegral (syntheticMenu screen), 0, 0)) (FrameDigest (show (syntheticFrame screen))) label
              (ControllerCells (B.pack [fromIntegral (syntheticCell screen), 0]) (B.pack [0, 0])) Nothing B.empty)
        , driverTraceMark = length <$> readIORef hits
        , driverTraceSince = \mark -> drop mark <$> readIORef hits
        , driverDiscardImage = \path -> modifyIORef' discarded (path :)
        , driverClock = readIORef clock
        , driverRecord = \_ -> modifyIORef' records (+ 1)
        }
  pure (state, driver)

-- | Screens 0..4 in a ring: UP moves forward, DOWN back, OK opens menu 10 from screen 0 only and
-- BACK returns from it to screen 0.
ring :: Maybe ModeDial -> Synthetic -> CrawlKey -> Maybe Synthetic
ring _ (Synthetic menu _ _) key = Just $ case (menu, key) of
  (10, KeyBack) -> screen 0
  (10, _) -> screen 10
  (0, KeyOk) -> screen 10
  (_, KeyUp) -> screen ((menu + 1) `mod` 5)
  (_, KeyDown) -> screen ((menu + 4) `mod` 5)
  _ -> screen menu
 where
  screen number = Synthetic number number number

menuOf :: Node -> Maybe Int
menuOf node = fmap (\(menu, _, _) -> fromIntegral menu) (screenMenu (nodeScreen node))

main :: IO ()
main = do
  failures <- newIORef (0 :: Int)
  let check label ok = unless ok (putStrLn ("FAIL " ++ label) >> modifyIORef' failures (+ 1))

  (ringFake, ringDriver) <- fake (Synthetic 0 0 0) ring
  let keys = [KeyUp, KeyDown, KeyOk, KeyBack]
  graph <- crawl (config keys 1000 100 10) ringDriver
  let menus = sort [menu | Just menu <- map menuOf (graphNodes graph)]
      byId = M.fromList [(nodeId node, node) | node <- graphNodes graph]
      reachedMenu edge = case edgeOutcome edge of
        Reached target _ _ _ _ _ _ -> menuOf (byId M.! target)
        Stopped _ _ -> Nothing
      expected edge = do
        source <- menuOf (byId M.! edgeFrom edge)
        ring Nothing (Synthetic source source source) (edgeKey edge) >>= Just . syntheticMenu
  check "every reachable screen is one node" (menus == [0, 1, 2, 3, 4, 10])
  check "every node tries every key once" (length (graphEdges graph) == 6 * length keys)
  check "each edge reaches the screen its key leads to" (all (\edge -> reachedMenu edge == expected edge) (graphEdges graph))
  check "an unbudgeted crawl is complete" (isNothing (graphLimit graph) && all (isNothing . nodeFrontier) (graphNodes graph))
  check "edges carry the paired dispatch action" (all (\edge -> case activityDispatches (edgeActivity edge) of
    [dispatch] -> dispatchAction dispatch == Just (0x00c00000 + fromIntegral (fromEnum (edgeKey edge)))
    _ -> False) (graphEdges graph))
  check "edges and new screens are journaled as produced" . (== length (graphEdges graph) + length (graphNodes graph)) =<< readIORef (fakeRecords ringFake)
  check "all snapshots are released" . M.null =<< readIORef (fakeSnapshots ringFake)
  check "held snapshots stay within one root and the open depth" . (<= 2 + 10) =<< readIORef (fakePeak ringFake)
  check "duplicate frames are discarded" . (== length [() | edge <- graphEdges graph, isSameFrame edge]) . length =<< readIORef (fakeDiscarded ringFake)

  (_, budgetDriver) <- fake (Synthetic 0 0 0) ring
  budgeted <- crawl (config keys 5 100 10) budgetDriver
  check "a trial budget stops the crawl" (graphTrials budgeted == 5 && graphLimit budgeted == Just TrialLimit)
  check "untried keys stay explicit" (any (not . null . nodeUntried) (graphNodes budgeted))

  let chain _ (Synthetic menu _ _) key = Just (if key == KeyOk then Synthetic (menu + 1) (menu + 1) (menu + 1) else Synthetic menu menu menu)
  (_, chainDriver) <- fake (Synthetic 0 0 0) chain
  deep <- crawl (config [KeyOk] 1000 100 3) chainDriver
  check "a depth limit leaves a frontier" (any (\node -> nodeFrontier node == Just DepthLimit && nodeDepth node == 3) (graphNodes deep))
  check "no node is deeper than the limit" (all ((<= 3) . nodeDepth) (graphNodes deep))

  (_, contextDriver) <- fake (Synthetic 0 0 0) ring
  contextual <- crawl ((config keys 1000 100 10) { configContextKeys = Just (ContextKeys 0 10 [KeyBack]) }) contextDriver
  let fromMenu number = [edgeKey edge | edge <- graphEdges contextual, Just source <- [lookup (edgeFrom edge) [(nodeId node, node) | node <- graphNodes contextual]], menuOf source == Just number]
  check "a screen in the context state is pressed with the context keys only" (fromMenu 10 == [KeyBack] && length (fromMenu 0) == length keys)

  let shortcut _ (Synthetic menu _ _) key = Just (case (menu, key) of
        (0, KeyUp) -> Synthetic 3 3 3
        (_, KeyOk) -> Synthetic (menu + 1) (menu + 1) (menu + 1)
        _ -> Synthetic menu menu menu)
  (_, shortcutDriver) <- fake (Synthetic 0 0 0) shortcut
  reopened <- crawl (config [KeyOk, KeyUp] 1000 100 3) shortcutDriver
  let screenAt number = [node | node <- graphNodes reopened, menuOf node == Just number]
  check "a depth-limited screen reached by a shorter path is expanded" (map (\node -> (nodeDepth node, nodeFrontier node)) (screenAt 3) == [(1, Nothing)]
    && map nodeDepth (screenAt 4) == [2])

  -- OK from 0 lands on screen 2 by frame; with cell 7 the following UP shows a different screen 3.
  let detour hidden _ (Synthetic menu frame cell) key = Just (case (menu, key) of
        (0, KeyOk) -> Synthetic 2 2 hidden
        (_, KeyUp) | menu < 4 -> Synthetic (menu + 1) (if cell == 7 then (menu + 1) * 10 else menu + 1) cell
        _ -> Synthetic menu frame cell)
  (detourFake, detourDriver) <- fake (Synthetic 0 0 0) (detour 2)
  deepened <- crawl (config [KeyUp, KeyOk] 1000 100 3) detourDriver
  let deepAt number = [node | node <- graphNodes deepened, menuOf node == Just number]
  check "a depth-limited screen with a shorter recorded path is replayed and expanded"
    (map (\node -> (nodeDepth node, nodeFrontier node)) (deepAt 3) == [(2, Nothing)]
    && map (\node -> (nodeDepth node, nodeFrontier node)) (deepAt 4) == [(3, Just DepthLimit)]
    && map (\entry -> (replayPath entry, replayOutcome entry)) (graphReplays deepened) == [([KeyOk, KeyUp], Replayed)]
    && graphReplayPresses deepened == 2)
  check "replays release their snapshots" . M.null =<< readIORef (fakeSnapshots detourFake)

  (_, divergingDriver) <- fake (Synthetic 0 0 0) (detour 7)
  diverging <- crawl (config [KeyUp, KeyOk] 1000 100 3) divergingDriver
  check "a replay that lands elsewhere leaves the screen depth-limited"
    (map (\node -> (nodeDepth node, nodeFrontier node)) [node | node <- graphNodes diverging, menuOf node == Just 3] == [(3, Just DepthLimit)]
    && case map replayOutcome (graphReplays diverging) of
         [Diverged (FrameDigest "30") _] -> True
         _ -> False)

  let variant _ (Synthetic menu frame cell) key = Just (case key of
        KeyUp -> Synthetic menu (frame + 1) cell
        _ -> Synthetic (menu + 1) (frame + 100) cell)
  (_, variantDriver) <- fake (Synthetic 0 0 0) variant
  variants <- crawl (config [KeyUp] 1000 100 10) variantDriver
  check "a frame-only difference is a variant of the same screen" (length (graphNodes variants) == 1
    && any (\edge -> case edgeOutcome edge of Reached _ SameMenuAndCells _ _ _ _ _ -> True; _ -> False) (graphEdges variants))

  let flicker _ (Synthetic menu frame cell) key = Just (if key == KeyUp then Synthetic menu (if even frame then frame + 1 else frame - 1) cell else Synthetic menu frame cell)
      withIdentity driver = driver { driverObserve = \label -> do
        screen <- driverObserve driver label
        let FrameDigest digest = screenFrame screen
        pure screen { screenIdentity = C.pack digest } }
  (_, plainDriver) <- fake (Synthetic 0 0 0) flicker
  plain <- crawl (config [KeyUp] 1000 100 10) plainDriver
  (_, identityDriver) <- fake (Synthetic 0 0 0) flicker
  identified <- crawl (config [KeyUp] 1000 100 10) (withIdentity identityDriver)
  check "identity bytes separate screens that share a menu reading and cells" (length (graphNodes plain) == 1 && length (graphNodes identified) == 2)

  (_, terminalDriver) <- fake (Synthetic 0 0 0) ring
  terminal <- crawl ((config keys 1000 100 10) { configTerminalPresses = [TerminalPress 0 0 KeyOk] }) terminalDriver
  let terminalNodes = [node | node <- graphNodes terminal, nodeFrontier node == Just Terminal]
  check "a terminal press records its landing screen without expanding it"
    (map menuOf terminalNodes == [Just 10] && null [() | edge <- graphEdges terminal, edgeFrom edge `elem` map nodeId terminalNodes]
    && sort [menu | Just menu <- map menuOf (graphNodes terminal)] == [0, 1, 2, 3, 4, 10])
  let ringToTen dial (Synthetic menu frame cell) key = if (menu, key) == (3, KeyOk) then Just (Synthetic 10 10 10) else ring dial (Synthetic menu frame cell) key
  (_, reopenDriver) <- fake (Synthetic 0 0 0) ringToTen
  reopenedTerminal <- crawl ((config [KeyOk, KeyUp, KeyDown, KeyBack] 1000 100 10) { configTerminalPresses = [TerminalPress 0 0 KeyOk] }) reopenDriver
  let tens = [node | node <- graphNodes reopenedTerminal, menuOf node == Just 10]
  check "a terminal screen reached later by an ordinary press is expanded"
    (map nodeFrontier tens == [Nothing] && not (null [() | edge <- graphEdges reopenedTerminal, edgeFrom edge `elem` map nodeId tens]))
  check "return keys group a candidate address"
    (RamDiff.byReturnKey [(KeyDown, [RamDiff.Candidate 4 1 2]), (KeyUp, [RamDiff.Candidate 4 1 2, RamDiff.Candidate 8 0 1])]
      == [(RamDiff.Candidate 4 1 2, [KeyDown, KeyUp]), (RamDiff.Candidate 8 0 1, [KeyUp])])
  check "configs refuse identity reads beyond 4096 bytes" (either (const True) (const False) (decodeCrawlConfig "{\"schema\":1,\"keys\":[\"up\"],\"roots\":[{\"name\":\"a\"}],\"max_trials\":1,\"max_nodes\":1,\"max_depth\":1,\"max_seconds\":1,\"identity_reads\":[{\"start\":\"0x0\",\"end\":\"0x1004\"}]}"))
  check "configs refuse a terminal key that is not crawled" (either (const True) (const False) (decodeCrawlConfig "{\"schema\":1,\"keys\":[\"up\"],\"roots\":[{\"name\":\"a\"}],\"max_trials\":1,\"max_nodes\":1,\"max_depth\":1,\"max_seconds\":1,\"terminal_presses\":[{\"controller\":76,\"state\":33,\"key\":\"ok\"}]}"))

  let stopping _ (Synthetic menu _ _) key = if key == KeyBack then Nothing else ring Nothing (Synthetic menu menu menu) key
  (_, stopDriver) <- fake (Synthetic 0 0 0) stopping
  stopped <- crawl (config keys 1000 100 10) stopDriver
  check "a strict stop is an edge and the crawl continues" (length [() | edge <- graphEdges stopped, isStopped edge] == 6
    && length (graphNodes stopped) == 6)

  let climbing _ (Synthetic menu _ _) _ = Just (let next = min 3 (menu + 1) in Synthetic next next next)
  (_, dialDriver) <- fake (Synthetic 0 0 0) climbing
  dialGraph <- crawl (CrawlConfig [KeyUp] Nothing [] [] [CrawlRoot (rootOf "default") Nothing, CrawlRoot (rootOf "auto") (Just ModeDialAuto)] 1000 100 10 1000) dialDriver
  check "each root starts from the boot snapshot" (sort [menu | node <- graphNodes dialGraph, nodeDepth node == 0, Just menu <- [menuOf node]] == [0, 50])

  let activity = stepActivity
        [ TraceHit "crawl-dispatch-context" 0 1 75 0 2 0
        , TraceHit "crawl-dispatch-context" 1 2 30 0 4 0
        , TraceHit "crawl-dispatch-action" 0 3 0 0 0 0xc72e10
        , TraceHit "crawl-dispatch-context" 0 4 100 0 0 0
        , TraceHit "crawl-dispatch-action" 0 5 0 0 0 0xc12aac
        , TraceHit "crawl-dispatch-action" 0 6 0 0 0 0xdead00
        , TraceHit "crawl-property-write" 0 7 0xfe 3 0 0
        ]
  check "actions pair with their own core's latest context" (map (\d -> (dispatchController d, dispatchAction d)) (activityDispatches activity)
    == [(75, Just 0xc72e10), (30, Nothing), (100, Just 0xc12aac)])
  check "an action with no open context is an orphan" (activityOrphanActions activity == [0xdead00])
  check "effects keep their arguments" (map effectArguments (activityEffects activity) == [[0xfe, 3, 0, 0]])
  check "cell changes name the controller" (cellChanges (B.pack [1, 0, 2, 0]) (B.pack [1, 0, 5, 0]) == [CellChange 1 2 5])

  check "configs refuse unknown keys" (either (const True) (const False) (decodeCrawlConfig "{\"schema\":1,\"keys\":[\"menu\"],\"roots\":[{\"name\":\"a\"}],\"max_trials\":1,\"max_nodes\":1,\"max_depth\":1,\"max_seconds\":1}"))
  check "configs refuse a depth beyond the snapshot budget" (either (const True) (const False) (decodeCrawlConfig "{\"schema\":1,\"keys\":[\"up\"],\"roots\":[{\"name\":\"a\"}],\"max_trials\":1,\"max_nodes\":1,\"max_depth\":21,\"max_seconds\":1}"))

  let window = GuestRange 0x1000 0x1010
      le values = B.pack (concatMap (\value -> [value, 0, 0, 0]) values)
      toggled = RamDiff.toggledWords window (le [1, 2, 3, 4]) (le [1, 9, 7, 5]) (le [1, 2, 7, 4]) (le [1, 2, 3, 6])
  check "a word the key changes, a second press restores and the control leaves is a candidate"
    (toggled == [RamDiff.Candidate 0x1004 2 9])
  check "read chunks cover a window exactly"
    (RamDiff.rangeChunks (GuestRange 0 0x18000) == [(0, 65536), (0x10000, 0x8000)])
  let diffConfig windows = RamDiff.decodeRamDiffConfig (C.pack ("{\"schema\":1,\"path\":[\"ok\"],\"key\":\"down\",\"return_keys\":[\"down\",\"up\"],\"control\":\"q\",\"windows\":" ++ windows ++ "}"))
  check "a RAM diff config decodes its windows" (fmap RamDiff.diffWindows (diffConfig "[{\"start\":\"0x100\",\"end\":\"0x200\"}]") == Right [GuestRange 0x100 0x200])
  check "RAM diff configs refuse overlapping windows"
    (either (const True) (const False) (diffConfig "[{\"start\":\"0x100\",\"end\":\"0x200\"},{\"start\":\"0x1fc\",\"end\":\"0x300\"}]"))
  check "RAM diff configs refuse more than 64 MiB" (either (const True) (const False) (diffConfig "[{\"start\":\"0x0\",\"end\":\"0x4000004\"}]"))

  count <- readIORef failures
  if count == 0 then putStrLn "menu crawl tests passed" else exitFailure
 where
  isSameFrame edge = case edgeOutcome edge of Reached _ SameFrame _ _ _ _ _ -> True; _ -> False
  isStopped edge = case edgeOutcome edge of Stopped _ _ -> True; _ -> False
