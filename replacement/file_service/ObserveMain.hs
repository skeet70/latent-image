{-# LANGUAGE OverloadedStrings #-}

-- | file-service-observe observe --role stock|owned --update DAT --image4 ID4 --run JSON
--     --boot-rom ROM --adjustment-nand NAND [--card IMG] --bridge BRIDGE --output NEW_DIR
--   file-service-observe compare STOCK_DIR OWNED_DIR OUTPUT_JSON
--
-- observe: fresh-boots the RAM-push bridge with trace sites at the 29 file-service entries and
-- the media task's request workers, a write watch over the result slot, mediamsg session and
-- open set, and a fetch watch over the retired extents minus the entry veneer words. After
-- attach it presses PLAY and the playback route keys, idles a fixed budget after each, and
-- captures the screen.
--
-- compare: the calls whose return address lies outside the retired extents and the fixed
-- body are the component's interface; their ordered entries, returns, arguments up to each
-- call's arity, state snapshots, the worker sequence, the watched writes and the screens must be
-- equal, and the owned role must fetch nothing in the retired extents. Registers beyond the
-- arity and void return registers are reported, not compared.
module Main (main) where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM, forM_, replicateM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
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
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), IOMode (..), hFlush, hSetBuffering, stdout, withBinaryFile)

data Role = Stock | Owned deriving (Eq)

roleName :: Role -> T.Text
roleName Stock = "stock"
roleName Owned = "owned"

newtype Address = Address Word32 deriving (Eq, Ord)

data Entry = Entry {entryAddress :: Address, entryName :: String}

data Worker = Worker {workerCode :: Word32, workerAddress :: Address}

-- | A half-open guest address range.
data Span = Span Address Address

data Inputs = Inputs
  { inputRole :: Role
  , inputUpdate :: FilePath
  , inputImage4 :: FilePath
  , inputRun :: FilePath
  , inputRom :: FilePath
  , inputNand :: FilePath
  , inputCard :: Maybe FilePath
  , inputBridge :: FilePath
  , inputOutput :: FilePath
  }

entries :: [Entry]
entries =
  [ Entry (Address a) n
  | (a, n) <-
      [ (0x71bc2c, "result_acquire"), (0x71bc6c, "result_release"), (0x71bc90, "result_wait")
      , (0x71be74, "read"), (0x71bfe0, "write"), (0x71c134, "close"), (0x71c2b0, "open")
      , (0x71c634, "result_done"), (0x71c648, "open_async"), (0x71c670, "open_wait")
      , (0x71c6a4, "open_unflagged"), (0x71c6c8, "open_unflagged_wait"), (0x71c6fc, "read_async")
      , (0x71c720, "read_wait"), (0x71c75c, "write_async"), (0x71c780, "write_wait")
      , (0x71c7bc, "close_async"), (0x71c7e0, "close_wait"), (0x71d450, "translate")
      , (0x71ddf0, "media_ready"), (0x71ea34, "map_bind"), (0x71eaac, "map_unbind")
      , (0x71eb28, "map_lookup"), (0x71ef18, "tag_allocate"), (0x71eff4, "tag_release")
      , (0x71fde4, "post"), (0x71fe7c, "media_task"), (0x721578, "route"), (0x7217f0, "intercept")
      ]
  ]

retired :: [Span]
retired =
  [ Span (Address s) (Address e)
  | (s, e) <-
      [ (0x71bc2c, 0x71bcc8), (0x71be74, 0x71c504), (0x71c634, 0x71c81c), (0x71d450, 0x71d66c)
      , (0x71ddf0, 0x71ded4), (0x71ea34, 0x71eba0), (0x71ef18, 0x71f078), (0x71fde4, 0x71fe2c)
      , (0x71fe7c, 0x71ff3c), (0x721108, 0x721744), (0x7217f0, 0x721834)
      ]
  ]

-- | The reserved fixed-image RAM range holding the owned body.
fixedBody :: Span
fixedBody = Span (Address 0xba0000) (Address 0xbe0000)

resultSlot, sessionAndOpenSet :: Span
resultSlot = Span (Address 0x910134) (Address 0x910140)
sessionAndOpenSet = Span (Address 0x910218) (Address 0x9102e0)

image4Base, workerTable :: Word32
image4Base = 0x61c400
workerTable = 0x814740

-- | Playback opens DSCF3097 first; LEFT, LEFT, RIGHT, RIGHT then reads 3096, 3095, 3096, 3097.
route :: [CrawlKey]
route = [KeyPlay, KeyLeft, KeyLeft, KeyRight, KeyRight]

-- | The stock bridge session releases the power hold between 784M and 874M instructions after
-- reset; attach is at about 399M and one playback file's open, reads and close finish within 13M.
settleChunks, finalChunks :: Int
settleChunks = 2
finalChunks = 2

chunkInstructions :: Int
chunkInstructions = 20000000

hitLimit, watchLimit :: Int
hitLimit = 4000000
watchLimit = 1048576

inside :: Span -> Word32 -> Bool
inside (Span (Address s) (Address e)) a = s <= a && a < e

fetchSpans :: [Span]
fetchSpans = concatMap split retired
  where
    veneers = [a | Entry (Address a) _ <- entries]
    split (Span (Address s) (Address e)) = go s (sort [v | v <- veneers, s <= v, v < e])
      where
        go from [] = [Span (Address from) (Address e) | from < e]
        go from (v : vs) = [Span (Address from) (Address v) | from < v] ++ go (v + 4) vs

hexWord :: Word32 -> String
hexWord w = "0x" ++ showHex w ""

siteKey :: Address -> K.Key
siteKey (Address a) = K.fromString (hexWord a)

spanValue :: Span -> Value
spanValue (Span (Address s) (Address e)) = A.toJSON [s, e]

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

readWorkers :: B.ByteString -> IO [Worker]
readWorkers image = do
  let word offset = foldr (\i acc -> acc `shiftL` 8 .|. fromIntegral (B.index image (offset + i))) 0 [0 .. 3] :: Word32
      start = fromIntegral (workerTable - image4Base)
      rows = takeWhile ((/= 0) . fst) [(word (start + 8 * i), word (start + 8 * i + 4)) | i <- [0 ..]]
  need (B.length image > start + 8 * 64) "image 4 does not hold the worker table"
  let workers = [Worker code (Address address) | (code, address) <- rows]
  need (length workers == 34) "the media task's worker table does not hold 34 rows"
  pure workers

-- * Observation

parseInputs :: [String] -> IO Inputs
parseInputs arguments = do
  pairs <- collect arguments
  let values = M.fromList pairs
      names = ["role", "update", "image4", "run", "boot-rom", "adjustment-nand", "bridge", "output"]
  need (length pairs == M.size values && M.keys (M.delete "card" values) == sort names) "observe requires each of --role --update --image4 --run --boot-rom --adjustment-nand --bridge --output once, and at most one --card"
  role <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  pure
    Inputs
      { inputRole = role, inputUpdate = values M.! "update", inputImage4 = values M.! "image4"
      , inputRun = values M.! "run", inputRom = values M.! "boot-rom", inputNand = values M.! "adjustment-nand"
      , inputCard = M.lookup "card" values, inputBridge = values M.! "bridge", inputOutput = values M.! "output"
      }
  where
    collect [] = pure []
    collect (('-' : '-' : key) : value : rest) = ((key, value) :) <$> collect rest
    collect _ = ioError (userError "options require --name value")

readSpec :: Value -> Int -> Value
readSpec base count = object ["base" .= base, "count" .= count]

register :: Int -> Value
register r = object ["register" .= r]

registerWord :: Int -> Int -> Value
registerWord r offset = object ["register-word" .= object ["register" .= r, "offset" .= offset]]

-- | Entry reads (path strings, stack arguments, request headers) and reads at return (a
-- blocking read's buffer).
entryCapture :: String -> Value
entryCapture name = object ["entry" .= entryReads, "returns" .= returnReads]
  where
    stackArguments = readSpec (register 13) 8
    header = [readSpec (register 0) 12, readSpec (registerWord 0 8) 32]
    entryReads :: [Value]
    entryReads = case name of
      "open" -> [readSpec (register 1) 64, stackArguments]
      "read" -> [stackArguments]
      "write" -> [stackArguments]
      "close" -> [stackArguments]
      "open_async" -> [readSpec (register 1) 64]
      "open_unflagged" -> [readSpec (register 1) 64]
      "open_wait" -> [readSpec (register 0) 64]
      "open_unflagged_wait" -> [readSpec (register 0) 64]
      "post" -> header
      _ -> []
    returnReads :: [Value]
    returnReads = case name of
      "read" -> [readSpec (register 3) 64]
      "read_wait" -> [readSpec (register 2) 64]
      _ -> []

workerCapture :: Value
workerCapture = object ["entry" .= [readSpec (register 0) 12, readSpec (registerWord 0 8) 32], "returns" .= ([] :: [Value])]

stateReads :: Value
stateReads = A.toJSON [[s, e - s] | Span (Address s) (Address e) <- [resultSlot, sessionAndOpenSet]]

writeSpans :: [Span]
writeSpans = [resultSlot, sessionAndOpenSet]

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
    , "fetches_by_extent" .= object [K.fromString (hexWord s) .= n | (Span (Address s) _, n) <- byExtent, n > 0]
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
    _ -> ioError (userError "usage: file-service-observe observe OPTIONS | compare STOCK_DIR OWNED_DIR OUTPUT_JSON")

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
  let named = [("update", inputUpdate inputs), ("image4", inputImage4 inputs), ("run", inputRun inputs), ("boot-rom", inputRom inputs)
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
  workers <- B.readFile (paths M.! "image4") >>= readWorkers
  let sites =
        [(entryAddress e, "fs:" ++ entryName e, entryCapture (entryName e)) | e <- entries]
          ++ [(workerAddress w, "worker:" ++ hexWord (workerCode w), workerCapture) | w <- workers]
      writes = map spanValue writeSpans
      fetches = map spanValue fetchSpans
      spec =
        object
          [ "schema" .= (1 :: Int)
          , "sites" .= object [siteKey a .= label | (a, label, _) <- sites]
          , "reads" .= object [siteKey a .= stateReads | (a, _, _) <- sites]
          , "captures" .= object [siteKey a .= capture | (a, _, capture) <- sites]
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
          , "ranges" .= map spanValue (mergedSpans (writeSpans ++ fetchSpans))
          , "write_ranges" .= writes, "fetch_ranges" .= fetches ]
      bridgeArguments =
        [ "--run", paths M.! "run", "--boot-rom", paths M.! "boot-rom", "--installed-dat", paths M.! "update"
        , "--adjustment-nand", paths M.! "adjustment-nand"
        , "--output", directory, "--ram-push-checkpoints", directory </> "unused-checkpoints"
        , "--fresh-boot", "--reset-observation", specPath ]
        ++ maybe [] (\card -> ["--sd-card-image", card]) (M.lookup "card" paths)
  BL.writeFile specPath (A.encode spec <> "\n")
  record "workers" (A.toJSON [object ["code" .= workerCode w, "address" .= a] | w <- workers, let Address a = workerAddress w])
  record "route" (A.toJSON (map crawlKeyWire route))
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
      forM_ (zip [1 :: Int ..] route) $ \(index, key) -> do
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
      modifyIORef' steps (object ["step" .= (length route + 1), "key" .= Null, "hits" .= [first, last'], "idle" .= settled, "watch" .= watchSummary observed, "screen" .= screen] :)
      saveSteps
      record "hit_count" (A.toJSON last')

-- * Comparison

-- | `eventDead` holds the argument registers beyond the call's arity.
data Event = Event {eventKind :: String, eventName :: String, eventExternal :: Bool, eventValue :: Value, eventDead :: Value}

entryNames :: M.Map Word32 String
entryNames = M.fromList [(a, entryName e) | e <- entries, let Address a = entryAddress e]

-- | Return addresses inside the retired extents or the fixed body belong to the component.
external :: Word32 -> Bool
external address = not (any (`inside` address) (fixedBody : retired))

-- | Register arguments of each entry, from the stock signatures; the media task's workers take
-- the request header.
arity :: String -> Int
arity name = case name of
  "read" -> 4
  "write" -> 4
  "close" -> 4
  "open" -> 4
  "open_async" -> 3
  "open_wait" -> 2
  "open_unflagged" -> 3
  "open_unflagged_wait" -> 2
  "read_async" -> 4
  "read_wait" -> 3
  "write_async" -> 4
  "write_wait" -> 3
  "close_async" -> 4
  "close_wait" -> 3
  "translate" -> 2
  "media_ready" -> 1
  "map_bind" -> 1
  "map_unbind" -> 1
  "map_lookup" -> 1
  "tag_release" -> 1
  "post" -> 1
  "route" -> 1
  "intercept" -> 2
  _ | "worker:" `isPrefixOf` name -> 1
  _ -> 0

registersOf :: [T.Text] -> Value -> Value
registersOf names hit = object [K.fromText r .= field r (field "registers" hit) | r <- names]

argumentRegisters :: [T.Text]
argumentRegisters = ["r0", "r1", "r2", "r3"]

taskOf :: Value -> Value
taskOf hit = object ["kernel" .= field "kernel" (field "task" hit), "task_id" .= field "task_id" (field "task" hit), "kind" .= field "kind" (field "task" hit)]

readsOf :: Value -> Value
readsOf hit = A.toJSON [field "bytes" r | r <- arrayOf (field "reads" (field "observation" hit))]

event :: Value -> Maybe Event
event hit = do
  label <- textOf (field "site" hit)
  address <- hexValue (field "address" hit)
  lr <- hexValue (field "lr" hit)
  let common = ["core" .= field "core" hit, "task" .= taskOf hit, "state" .= field "memory" hit]
  case label of
    _ | "fs:" `isPrefixOf` label ->
          let name = drop 3 label
              (live, dead) = splitAt (arity name) argumentRegisters
           in Just (Event "entry" name (external lr) (object (["kind" .= ("entry" :: T.Text), "name" .= name, "lr" .= hexWord lr, "arguments" .= registersOf live hit, "reads" .= readsOf hit] ++ common)) (registersOf dead hit))
      | "worker:" `isPrefixOf` label ->
          let (live, dead) = splitAt (arity label) argumentRegisters
           in Just (Event "worker" label True (object (["kind" .= ("worker" :: T.Text), "name" .= label, "arguments" .= registersOf live hit, "reads" .= readsOf hit] ++ common)) (registersOf dead hit))
      | label == "call-return" -> do
          site <- hexValue (field "site" (field "observation" hit))
          let name = fromMaybe (hexWord site) (M.lookup site entryNames)
              kind = if M.member site entryNames then "return" else "worker-return"
              result = kind == "return" && name `notElem` ["map_unbind", "tag_release", "intercept"]
              (live, dead) = if result then (["r0"], ["r1", "r2", "r3"]) else ([], argumentRegisters)
          Just (Event kind name (kind == "worker-return" || external address) (object (["kind" .= kind, "name" .= name, "result" .= registersOf live hit, "reads" .= readsOf hit] ++ common)) (registersOf dead hit))
      | otherwise -> Nothing

loadHits :: FilePath -> IO [Value]
loadHits directory = do
  bytes <- B.readFile (directory </> "hits.jsonl")
  forM (filter (not . B.null) (C.lines bytes)) (either (ioError . userError) pure . A.eitherDecodeStrict')

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
counts events = M.fromListWith add [(eventName e, if eventExternal e then (1, 0) else (0, 1)) | e <- events, eventKind e `elem` ["entry", "worker"]]
  where add (a, b) (c, d) = (a + c, b + d)

compareMain :: FilePath -> FilePath -> FilePath -> IO ()
compareMain stockDir ownedDir output = do
  exists <- doesPathExist output
  need (not exists) "comparison output must be new"
  let load directory = do
        events <- mapMaybe event <$> loadHits directory
        receipt <- loadJson (directory </> "receipt.json")
        steps <- arrayOf <$> loadJson (directory </> "steps.json")
        pure (events, receipt, steps)
  (stockEvents, stockReceipt, stockSteps) <- load stockDir
  (ownedEvents, ownedReceipt, ownedSteps) <- load ownedDir
  forM_ [stockReceipt, ownedReceipt] $ \r -> need (field "status" (field "outcome" r) == String "completed") "an observation did not complete"
  let interface events = [eventValue e | e <- events, eventExternal e]
      dead events = [eventDead e | e <- events, eventExternal e]
      internal events = [eventValue e | e <- events, not (eventExternal e)]
      watches receipt steps = [field "watch" s | s <- arrayOf (field "reset_segments" receipt)] ++ [field "watch" s | s <- steps]
      writesOf receipt steps = concatMap (arrayOf . field "writes") (watches receipt steps)
      fetchTotal receipt steps = sum [n | w <- watches receipt steps, Number c <- [field "fetch_count" w], let n = truncate c :: Integer]
      violations receipt steps = [v | w <- watches receipt steps, let v = field "violation" w, v /= Null]
      screens receipt steps = field "sha256" (field "attached_screen" receipt) : [field "sha256" (field "screen" s) | s <- steps]
      countTable = object [K.fromString name .= object ["stock" .= pairValue (M.lookup name stockCounts), "owned" .= pairValue (M.lookup name ownedCounts)] | name <- M.keys (M.union stockCounts ownedCounts)]
      pairValue = maybe (object ["interface" .= (0 :: Int), "internal" .= (0 :: Int)]) (\(a, b) -> object ["interface" .= a, "internal" .= b])
      stockCounts = counts stockEvents
      ownedCounts = counts ownedEvents
      interfaceCounts = M.map fst
      sequenceDiff = firstDifferences 20 (interface stockEvents) (interface ownedEvents)
      writeDiff = firstDifferences 20 (writesOf stockReceipt stockSteps) (writesOf ownedReceipt ownedSteps)
      screenDiff = firstDifferences 20 (screens stockReceipt stockSteps) (screens ownedReceipt ownedSteps)
      ownedFetches = fetchTotal ownedReceipt ownedSteps
      behaviorEqual =
        interface stockEvents == interface ownedEvents
          && interfaceCounts stockCounts == interfaceCounts ownedCounts
          && writesOf stockReceipt stockSteps == writesOf ownedReceipt ownedSteps
          && screens stockReceipt stockSteps == screens ownedReceipt ownedSteps
          && null (violations stockReceipt stockSteps) && null (violations ownedReceipt ownedSteps)
          && sharedInputs stockReceipt == sharedInputs ownedReceipt
      equal = behaviorEqual && ownedFetches == 0
      sharedInputs receipt = case field "inputs" receipt of
        Object fields -> Object (KM.delete "update" fields)
        other -> other
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "stock" .= object ["directory" .= stockDir, "inputs" .= field "inputs" stockReceipt, "hits" .= field "hit_count" stockReceipt, "events" .= length stockEvents, "interface_events" .= length (interface stockEvents), "internal_events" .= length (internal stockEvents), "retired_fetches" .= fetchTotal stockReceipt stockSteps, "watched_writes" .= length (writesOf stockReceipt stockSteps)]
    , "owned" .= object ["directory" .= ownedDir, "inputs" .= field "inputs" ownedReceipt, "hits" .= field "hit_count" ownedReceipt, "events" .= length ownedEvents, "interface_events" .= length (interface ownedEvents), "internal_events" .= length (internal ownedEvents), "retired_fetches" .= ownedFetches, "watched_writes" .= length (writesOf ownedReceipt ownedSteps)]
    , "entry_counts" .= countTable
    , "interface_sequence" .= sequenceDiff
    , "dead_registers" .= firstDifferences 20 (dead stockEvents) (dead ownedEvents)
    , "watched_writes" .= writeDiff
    , "screens" .= object ["stock" .= screens stockReceipt stockSteps, "owned" .= screens ownedReceipt ownedSteps, "comparison" .= screenDiff]
    , "owned_retired_fetches_by_extent" .= [field "fetches_by_extent" w | w <- watches ownedReceipt ownedSteps]
    , "stock_retired_fetches_by_extent" .= [field "fetches_by_extent" w | w <- watches stockReceipt stockSteps]
    , "watch_violations" .= object ["stock" .= violations stockReceipt stockSteps, "owned" .= violations ownedReceipt ownedSteps]
    , "shared_inputs_equal" .= (sharedInputs stockReceipt == sharedInputs ownedReceipt)
    , "behavior_equal" .= behaviorEqual
    , "verdict" .= (if equal then "equal" else "different" :: T.Text)
    ]
  putStrLn ("verdict " ++ (if equal then "equal" else "different"))
