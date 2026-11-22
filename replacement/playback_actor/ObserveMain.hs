{-# LANGUAGE OverloadedStrings #-}

-- | playback-actor-observe observe --role stock|owned --update DAT --run JSON --boot-rom ROM
--     --adjustment-nand NAND [--card IMG] [--route KEY,...] --bridge BRIDGE --output NEW_DIR
--   playback-actor-observe compare STOCK_DIR OWNED_DIR OUTPUT_JSON
--
-- observe: fresh-boots the RAM-push bridge with trace sites at the queue-4 machine dispatcher,
-- the actor's entries and the retained services the actor calls, a write watch over the actor's
-- state and a fetch watch over the retired extents minus the entry veneer words. After attach it
-- presses the route keys (by default PLAY and the playback keys), idles a fixed budget after
-- each, and captures the screen. Every hit samples the machine event and state halfwords.
--
-- compare: the dispatcher's entries and returns, the actor's entries called from outside the
-- retired extents and their returns, the calls the actor makes into the retained services with
-- the bytes they point at and r0 on return, the watched writes, the screens and the card's
-- accepted writes, dirty sectors and final command history must be equal, and the owned role
-- must fetch nothing in the retired extents.
module Main (main) where

import Control.DeepSeq (rnf)
import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (foldM, forM, forM_, replicateM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.IORef
import Data.List (isPrefixOf, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.Sha256 (sha256File, sha256Hex)
import EmulatorUsb
import Numeric (readHex, showHex)
import Replacement.PlaybackActor.Actor
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), IOMode (..), hFlush, hSetBuffering, stdout, withBinaryFile)

data Role = Stock | Owned deriving (Eq)

roleName :: Role -> T.Text
roleName Stock = "stock"
roleName Owned = "owned"

data Inputs = Inputs
  { inputRole :: Role
  , inputUpdate :: FilePath
  , inputRun :: FilePath
  , inputRom :: FilePath
  , inputNand :: FilePath
  , inputCard :: Maybe FilePath
  , inputBridge :: FilePath
  , inputOutput :: FilePath
  , inputRoute :: [CrawlKey]
  }

-- | The default route: playback opens DSCF3097 first; LEFT, LEFT, RIGHT, RIGHT then shows 3096,
-- 3095, 3096, 3097.
playbackRoute :: [CrawlKey]
playbackRoute = [KeyPlay, KeyLeft, KeyLeft, KeyRight, KeyRight]

settleChunks, finalChunks, chunkInstructions, hitLimit, watchLimit :: Int
settleChunks = 2
finalChunks = 2
chunkInstructions = 20000000
hitLimit = 4000000
watchLimit = 1048576

hexWord :: Word32 -> String
hexWord w = "0x" ++ showHex w ""

siteKey :: Word32 -> K.Key
siteKey = K.fromString . hexWord

spanValue :: Span -> Value
spanValue (Span s e) = A.toJSON [s, e]

need :: Bool -> String -> IO ()
need condition message = unless condition (ioError (userError message))

field :: T.Text -> Value -> Value
field name (Object fields) = fromMaybe Null (KM.lookup (K.fromText name) fields)
field _ _ = Null

textOf :: Value -> Maybe String
textOf (String t) = Just (T.unpack t)
textOf _ = Nothing

hexValue :: Value -> Maybe Word32
hexValue (String t) | Just digits <- T.stripPrefix "0x" t, [(v, "")] <- readHex (T.unpack digits) = Just v
hexValue (Number n) = Just (truncate n)
hexValue _ = Nothing

arrayOf :: Value -> [Value]
arrayOf (Array values) = V.toList values
arrayOf _ = []

readSpec :: Value -> Int -> Value
readSpec base count = object ["base" .= base, "count" .= count]

registerBase :: Int -> Value
registerBase r = object ["register" .= r]

captureValue :: Capture -> Value
captureValue capture =
  object
    [ "entry" .= [readSpec (registerBase r) n | (r, n) <- captureEntry capture]
    , "returns" .= [readSpec (registerBase r) n | (r, n) <- captureReturn capture]
    ]

-- | The retired extents minus each entry's veneer word.
fetchSpans :: [Span]
fetchSpans = concatMap split retired
  where
    veneers = map entryAddress entries
    split (Span s e) = go s (sort [v | v <- veneers, s <= v, v < e])
      where
        go from [] = [Span from e | from < e]
        go from (v : vs) = [Span from v | from < v] ++ go (v + 4) vs

mergedSpans :: [Span] -> [Span]
mergedSpans = foldr merge [] . sortOn (\(Span s _) -> s)
  where
    merge range [] = [range]
    merge (Span s e) (Span s' e' : rest)
      | e >= s' = Span s (max e e') : rest
      | otherwise = Span s e : Span s' e' : rest

-- | Fetch records per retired extent and per PC; write records without the role-specific PC,
-- instruction count and tick.
watchSummary :: Value -> Value
watchSummary watch =
  object
    [ "violation" .= field "violation" watch
    , "record_count" .= length records
    , "fetch_count" .= length fetches
    , "fetches_by_extent" .= object [K.fromString (hexWord s) .= n | (Span s _, n) <- byExtent, n > 0]
    , "fetches_by_pc" .= object [K.fromString (hexWord pc) .= n | (pc, n) <- M.toList byPc]
    , "writes" .= [object ["address" .= field "address" r, "size" .= field "size" r, "before" .= field "before" r, "value" .= field "value" r, "source" .= field "source" r, "core" .= field "core" r] | r <- writes]
    ]
  where
    records = arrayOf (field "records" watch)
    fetches = [r | r <- records, field "source" r == String "cpu-fetch"]
    writes = [r | r <- records, field "source" r /= String "cpu-fetch", field "source" r /= String "cpu-read"]
    fetchAddresses = mapMaybe (hexValue . field "address") fetches
    byExtent = [(extent, length (filter (inside extent) fetchAddresses)) | extent <- retired]
    byPc = M.fromListWith (+) [(a, 1 :: Int) | a <- fetchAddresses]

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  case arguments of
    ("observe" : rest) -> parseInputs rest >>= observeMain
    ["compare", stock, owned, output] -> compareMain stock owned output
    _ -> ioError (userError "usage: playback-actor-observe observe OPTIONS | compare STOCK_DIR OWNED_DIR OUTPUT_JSON")

parseInputs :: [String] -> IO Inputs
parseInputs arguments = do
  pairs <- collect arguments
  let values = M.fromList pairs
      names = ["role", "update", "run", "boot-rom", "adjustment-nand", "bridge", "output"]
  need (length pairs == M.size values && M.keys (foldr M.delete values ["card", "route"]) == sort names) "observe requires each of --role --update --run --boot-rom --adjustment-nand --bridge --output once, and at most one --card and --route"
  role <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  keys <- case M.lookup "route" values of
    Nothing -> pure playbackRoute
    Just text -> mapM crawlKey (splitOn text)
  pure
    Inputs
      { inputRole = role, inputUpdate = values M.! "update", inputRun = values M.! "run", inputRom = values M.! "boot-rom"
      , inputNand = values M.! "adjustment-nand", inputCard = M.lookup "card" values, inputBridge = values M.! "bridge"
      , inputOutput = values M.! "output", inputRoute = keys
      }
  where
    collect [] = pure []
    collect (('-' : '-' : key) : value : rest) = ((key, value) :) <$> collect rest
    collect _ = ioError (userError "options require --name value")
    splitOn text = case break (== ',') text of
      (key, ',' : rest) -> key : splitOn rest
      (key, _) -> [key]
    crawlKey wire = case [key | key <- [minBound .. maxBound], crawlKeyWire key == wire] of
      [key] -> pure key
      _ -> ioError (userError ("--route names an unknown key: " ++ wire))

observeMain :: Inputs -> IO ()
observeMain inputs = do
  exists <- doesPathExist (inputOutput inputs)
  need (not exists) "--output must be new"
  directory <- makeAbsolute (inputOutput inputs)
  createDirectory directory
  receipt <- newIORef KM.empty
  let record key value = modifyIORef' receipt (KM.insert key value)
      save status = do
        fields <- readIORef receipt
        BL.writeFile (directory </> "receipt.json") (A.encode (Object (KM.insert "outcome" status fields)) <> "\n")
  outcome <- try (observe inputs directory record)
  case outcome of
    Right () -> save (object ["status" .= ("completed" :: T.Text)])
    Left failure -> do
      save (object ["status" .= ("refused" :: T.Text), "error" .= displayException (failure :: SomeException)])
      putStrLn (displayException failure)
      exitFailure

observe :: Inputs -> FilePath -> (K.Key -> Value -> IO ()) -> IO ()
observe inputs directory record = do
  let named = [("update", inputUpdate inputs), ("run", inputRun inputs), ("boot-rom", inputRom inputs)
              ,("adjustment-nand", inputNand inputs), ("bridge", inputBridge inputs)] ++ [("card", card) | Just card <- [inputCard inputs]]
  paths <- M.fromList <$> mapM (\(name, path) -> (,) name <$> makeAbsolute path) named
  bindings <- forM (M.toList paths) $ \(name, path) -> do
    digest <- sha256File path
    pure (K.fromString name .= object ["path" .= path, "sha256" .= sha256Hex digest])
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  record "schema" (Number 1)
  record "role" (String (roleName (inputRole inputs)))
  record "inputs" (object (bindings ++ ["observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]]))
  let sites =
        (dispatcher, "dispatch", Capture [] [])
          : [(entryAddress e, "entry:" ++ entryName e, entryCapture e) | e <- entries]
          ++ [(serviceAddress s, "service:" ++ serviceName s, serviceCapture s) | s <- services]
      machineReads = A.toJSON [[s, e - s] | Span s e <- [machineEvents, machineStates]]
      writes = map spanValue watchedWrites
      fetches = map spanValue fetchSpans
      spec =
        object
          [ "schema" .= (1 :: Int)
          , "sites" .= object [siteKey a .= label | (a, label, _) <- sites]
          , "reads" .= object [siteKey a .= machineReads | (a, _, _) <- sites]
          , "captures" .= object [siteKey a .= captureValue capture | (a, _, capture) <- sites]
          , "task" .= ("logical" :: T.Text)
          , "hit_limit" .= hitLimit
          , "write_ranges" .= writes
          , "fetch_ranges" .= fetches
          , "watch_limit" .= watchLimit
          ]
      specPath = directory </> "spec.json"
      watch =
        object
          [ "command" .= ("watch-begin" :: T.Text), "views" .= ("fixed" :: T.Text), "limit" .= watchLimit
          , "ranges" .= map spanValue (mergedSpans (watchedWrites ++ fetchSpans))
          , "write_ranges" .= writes, "fetch_ranges" .= fetches ]
      bridgeArguments =
        [ "--run", paths M.! "run", "--boot-rom", paths M.! "boot-rom", "--installed-dat", paths M.! "update"
        , "--adjustment-nand", paths M.! "adjustment-nand"
        , "--output", directory, "--ram-push-checkpoints", directory </> "unused-checkpoints"
        , "--fresh-boot", "--reset-observation", specPath ]
        ++ maybe [] (\card -> ["--sd-card-image", card]) (M.lookup "card" paths)
  BL.writeFile specPath (A.encode spec <> "\n")
  record "route" (A.toJSON (map crawlKeyWire (inputRoute inputs)))
  record "budget" (object ["settle_chunks" .= settleChunks, "final_chunks" .= finalChunks, "chunk_instructions" .= chunkInstructions])
  steps <- newIORef []
  cursor <- newIORef (0 :: Int)
  withBinaryFile (directory </> "hits.jsonl") WriteMode $ \hits -> do
    let drain emulator = do
          start <- readIORef cursor
          reply <- emulatorTraceHitsPage emulator start 65536
          let values = arrayOf (field "hits" reply)
          forM_ values (\value -> BL.hPut hits (A.encode value <> "\n"))
          let end = start + length values
          writeIORef cursor end
          hFlush hits
          case field "count" reply of
            Number total | end < truncate total -> drain emulator
            _ -> pure end
        idle emulator count = replicateM count $ do
          ran <- emulatorBridgeCommand emulator (object ["command" .= ("idle" :: T.Text), "instructions" .= chunkInstructions])
          media <- emulatorBridgeCommand emulator (object ["command" .= ("media-state" :: T.Text)])
          pure (object ["idle" .= ran, "media" .= media])
        saveSteps = BL.writeFile (directory </> "steps.json") . A.encode . reverse =<< readIORef steps
    bracket (startEmulator (paths M.! "bridge") directory bridgeArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, ()) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      need (bootStart start == FreshBootStart) "bridge did not fresh boot"
      record "emulator_start" (startRecord start)
      attached <- drain emulator
      reset <- B.readFile (directory </> "reset-observation.json") >>= either (ioError . userError) pure . A.eitherDecodeStrict'
      record "reset_segments" (A.toJSON [object ["phase" .= field "phase" s, "instructions" .= field "instructions" s, "hits_end" .= field "hits_end" s, "watch" .= watchSummary (field "watch" s)] | s <- arrayOf (field "segments" reset)])
      record "hits_at_attach" (A.toJSON attached)
      shot <- emulatorCapture emulator "attached"
      record "attached_screen" shot
      forM_ (zip [1 :: Int ..] (inputRoute inputs)) $ \(index, key) -> do
        first <- readIORef cursor
        _ <- emulatorBridgeCommand emulator watch
        press <- emulatorBridgeCommand emulator (object ["command" .= ("key" :: T.Text), "key" .= crawlKeyWire key])
        settled <- idle emulator settleChunks
        observed <- emulatorWatchEnd emulator
        last' <- drain emulator
        screen <- emulatorCapture emulator ("step-" ++ show index)
        state <- emulatorBridgeCommand emulator (object ["command" .= ("ui-state" :: T.Text)])
        modifyIORef' steps (object
          [ "step" .= index, "key" .= crawlKeyWire key, "hits" .= [first, last'], "press" .= press, "idle" .= settled
          , "watch" .= watchSummary observed, "screen" .= screen, "ui_state" .= state ] :)
        saveSteps
        putStrLn ("step " ++ show index ++ " " ++ crawlKeyWire key ++ " hits " ++ show (last' - first))
      first <- readIORef cursor
      _ <- emulatorBridgeCommand emulator watch
      settled <- idle emulator finalChunks
      observed <- emulatorWatchEnd emulator
      last' <- drain emulator
      screen <- emulatorCapture emulator "final"
      modifyIORef' steps (object ["step" .= (length (inputRoute inputs) + 1), "key" .= Null, "hits" .= [first, last'], "idle" .= settled, "watch" .= watchSummary observed, "screen" .= screen] :)
      saveSteps
      record "hit_count" (A.toJSON last')

-- * Comparison

data Event = Event {eventKind :: String, eventName :: String, eventExternal :: Bool, eventValue :: Value}

-- | A return address inside a retired extent belongs to the actor.
external :: Word32 -> Bool
external address = not (any (`inside` address) retired)

registersOf :: [T.Text] -> Value -> Value
registersOf names hit = object [K.fromText r .= field r (field "registers" hit) | r <- names]

taskOf :: Value -> Value
taskOf hit = object ["kernel" .= field "kernel" (field "task" hit), "task_id" .= field "task_id" (field "task" hit), "kind" .= field "kind" (field "task" hit)]

readsOf :: Value -> Value
readsOf hit = A.toJSON [field "bytes" r | r <- arrayOf (field "reads" (field "observation" hit))]

argumentNames :: Int -> [T.Text]
argumentNames n = take n ["r0", "r1", "r2", "r3"]

entryNames :: M.Map Word32 Entry
entryNames = M.fromList [(entryAddress e, e) | e <- entries]

serviceSites :: M.Map Word32 Service
serviceSites = M.fromList [(serviceAddress s, s) | s <- services]

event :: Value -> Maybe Event
event hit = do
  label <- textOf (field "site" hit)
  address <- hexValue (field "address" hit)
  lr <- hexValue (field "lr" hit)
  let common = ["core" .= field "core" hit, "task" .= taskOf hit, "machines" .= field "memory" hit]
  case label of
    "dispatch" -> Just (Event "dispatch" "dispatch" True (object (["kind" .= ("dispatch" :: T.Text), "machine" .= registersOf ["r0"] hit] ++ common)))
    _ | "entry:" `isPrefixOf` label, Just e <- M.lookup address entryNames ->
          Just (Event "entry" (entryName e) (external lr) (object (["kind" .= ("entry" :: T.Text), "name" .= entryName e, "lr" .= hexWord lr, "arguments" .= registersOf (argumentNames (entryArity e)) hit, "reads" .= readsOf hit] ++ common)))
      | "service:" `isPrefixOf` label, Just s <- M.lookup address serviceSites, not (external lr) ->
          Just (Event "service" (serviceName s) True (object (["kind" .= ("service" :: T.Text), "name" .= serviceName s, "arguments" .= registersOf (serviceArguments s) hit, "reads" .= readsOf hit] ++ common)))
      | "service:" `isPrefixOf` label -> Nothing
      | label == "call-return" -> do
          site <- hexValue (field "site" (field "observation" hit))
          case (site == dispatcher, M.lookup site serviceSites, M.lookup site entryNames) of
            (True, _, _) -> Just (Event "dispatch-return" "dispatch" True (object (["kind" .= ("dispatch-return" :: T.Text), "result" .= registersOf ["r0"] hit] ++ common)))
            (_, Just s, _)
              | external address -> Nothing
              | otherwise -> Just (Event "service-return" (serviceName s) True (object (["kind" .= ("service-return" :: T.Text), "name" .= serviceName s, "result" .= registersOf (serviceResult s) hit, "reads" .= readsOf hit] ++ common)))
            (_, _, Just e) -> Just (Event "return" (entryName e) (external address) (object (["kind" .= ("return" :: T.Text), "name" .= entryName e, "result" .= registersOf (entryResult e) hit, "reads" .= readsOf hit] ++ common)))
            _ -> Nothing
      | otherwise -> Nothing

loadEvents :: FilePath -> IO [Event]
loadEvents directory = do
  bytes <- BL.readFile (directory </> "hits.jsonl")
  reverse <$> foldM keep [] (filter (not . BL.null) (BLC.lines bytes))
  where
    keep kept line = case A.eitherDecode line of
      Left problem -> ioError (userError problem)
      Right hit -> case event hit of
        Nothing -> pure kept
        Just e -> rnf (eventValue e) `seq` rnf (eventKind e, eventName e, eventExternal e) `seq` pure (e : kept)

loadJson :: FilePath -> IO Value
loadJson path = B.readFile path >>= either (ioError . userError) pure . A.eitherDecodeStrict'

firstDifferences :: Int -> [Value] -> [Value] -> Value
firstDifferences limit stock owned =
  object
    [ "stock_length" .= length stock, "owned_length" .= length owned
    , "equal" .= (stock == owned)
    , "differences" .= take limit [object ["index" .= i, "stock" .= s, "owned" .= o] | (i, s, o) <- zip3 [0 :: Int ..] (pad stock) (pad owned), s /= o]
    ]
  where
    total = max (length stock) (length owned)
    pad xs = take total (xs ++ repeat Null)

counts :: [Event] -> M.Map String (Int, Int)
counts events = M.fromListWith add [(eventName e, if eventExternal e then (1, 0) else (0, 1)) | e <- events, eventKind e `elem` ["entry", "service", "dispatch"]]
  where add (a, b) (c, d) = (a + c, b + d)

compareMain :: FilePath -> FilePath -> FilePath -> IO ()
compareMain stockDir ownedDir output = do
  exists <- doesPathExist output
  need (not exists) "comparison output must be new"
  let load directory = do
        events <- loadEvents directory
        receipt <- loadJson (directory </> "receipt.json")
        steps <- arrayOf <$> loadJson (directory </> "steps.json")
        pure (events, receipt, steps)
  (stockEvents, stockReceipt, stockSteps) <- load stockDir
  (ownedEvents, ownedReceipt, ownedSteps) <- load ownedDir
  forM_ [stockReceipt, ownedReceipt] $ \r -> need (field "status" (field "outcome" r) == String "completed") "an observation did not complete"
  need (field "role" stockReceipt == String (roleName Stock)) "the first observation must have the stock role"
  let watches receipt steps = [field "watch" s | s <- arrayOf (field "reset_segments" receipt)] ++ [field "watch" s | s <- steps]
      writesOf receipt steps = concatMap (arrayOf . field "writes") (watches receipt steps)
      stockWrites = writesOf stockReceipt stockSteps
      ownedWrites = writesOf ownedReceipt ownedSteps
      interface events = [eventValue e | e <- events, eventExternal e]
      internal events = [eventValue e | e <- events, not (eventExternal e)]
      fetchTotal receipt steps = sum [n | w <- watches receipt steps, Number c <- [field "fetch_count" w], let n = truncate c :: Integer]
      violations receipt steps = [v | w <- watches receipt steps, let v = field "violation" w, v /= Null]
      screens receipt steps = field "sha256" (field "attached_screen" receipt) : [field "sha256" (field "screen" s) | s <- steps]
      countTable = object [K.fromString name .= object ["stock" .= pairValue (M.lookup name stockCounts), "owned" .= pairValue (M.lookup name ownedCounts)] | name <- M.keys (M.union stockCounts ownedCounts)]
      pairValue = maybe (object ["interface" .= (0 :: Int), "internal" .= (0 :: Int)]) (\(a, b) -> object ["interface" .= a, "internal" .= b])
      stockCounts = counts stockEvents
      ownedCounts = counts ownedEvents
      interfaceCounts = M.filter (> 0) . M.map fst
      card steps =
        [ object ["accepted_writes" .= field "accepted_writes" media, "accepted_write_bytes" .= field "accepted_write_bytes" media, "dirty_sectors" .= field "dirty_sectors" media]
        | s <- steps, idled <- take 1 (reverse (arrayOf (field "idle" s))), let media = field "media" idled
        ]
      finalHistory steps = [field "history" (field "media" idled) | s <- take 1 (reverse steps), idled <- take 1 (reverse (arrayOf (field "idle" s)))]
      cardOf steps = card steps ++ finalHistory steps
      ownedFetches = fetchTotal ownedReceipt ownedSteps
      sharedInputs receipt = case field "inputs" receipt of
        Object fields -> Object (KM.delete "update" fields)
        other -> other
      behaviorEqual =
        interface stockEvents == interface ownedEvents
          && interfaceCounts stockCounts == interfaceCounts ownedCounts
          && stockWrites == ownedWrites
          && screens stockReceipt stockSteps == screens ownedReceipt ownedSteps
          && cardOf stockSteps == cardOf ownedSteps
          && null (violations stockReceipt stockSteps) && null (violations ownedReceipt ownedSteps)
          && sharedInputs stockReceipt == sharedInputs ownedReceipt
      ownedRole = field "role" ownedReceipt == String (roleName Owned)
      equal = behaviorEqual && (not ownedRole || ownedFetches == 0)
      summary events receipt steps fetches writes = object
        [ "inputs" .= field "inputs" receipt, "hits" .= field "hit_count" receipt, "events" .= length events
        , "interface_events" .= length (interface events), "internal_events" .= length (internal events)
        , "retired_fetches" .= fetches, "watched_writes" .= length writes, "steps" .= length steps ]
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "retired" .= map spanValue retired
    , "stock" .= summary stockEvents stockReceipt stockSteps (fetchTotal stockReceipt stockSteps) stockWrites
    , "owned" .= summary ownedEvents ownedReceipt ownedSteps ownedFetches ownedWrites
    , "entry_counts" .= countTable
    , "interface_sequence" .= firstDifferences 20 (interface stockEvents) (interface ownedEvents)
    , "internal_sequence" .= firstDifferences 20 (internal stockEvents) (internal ownedEvents)
    , "watched_writes" .= firstDifferences 20 stockWrites ownedWrites
    , "screens" .= object ["stock" .= screens stockReceipt stockSteps, "owned" .= screens ownedReceipt ownedSteps, "comparison" .= firstDifferences 20 (screens stockReceipt stockSteps) (screens ownedReceipt ownedSteps)]
    , "card" .= object ["stock" .= card stockSteps, "owned" .= card ownedSteps, "comparison" .= firstDifferences 20 (cardOf stockSteps) (cardOf ownedSteps)]
    , "owned_retired_fetches_by_extent" .= [field "fetches_by_extent" w | w <- watches ownedReceipt ownedSteps]
    , "stock_retired_fetches_by_extent" .= [field "fetches_by_extent" w | w <- watches stockReceipt stockSteps]
    , "watch_violations" .= object ["stock" .= violations stockReceipt stockSteps, "owned" .= violations ownedReceipt ownedSteps]
    , "shared_inputs_equal" .= (sharedInputs stockReceipt == sharedInputs ownedReceipt)
    , "behavior_equal" .= behaviorEqual
    , "verdict" .= (if equal then "equal" else "different" :: T.Text)
    ]
  putStrLn ("verdict " ++ (if equal then "equal" else "different"))
  unless equal exitFailure
