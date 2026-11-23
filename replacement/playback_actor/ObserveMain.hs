{-# LANGUAGE OverloadedStrings #-}

-- | playback-actor-observe observe --role stock|owned --machines N,... --update DAT --run JSON
--     --boot-rom ROM --adjustment-nand NAND [--card IMG] [--route KEY,...] --bridge BRIDGE --output NEW_DIR
--   playback-actor-observe compare STOCK_DIR OWNED_DIR OWNED_SYMBOLS OUTPUT_JSON
--   playback-actor-observe same-memory FIRST_DIR SECOND_DIR OUTPUT_JSON
--
-- observe: fresh-boots the RAM-push bridge with trace sites at the queue-4 machine dispatcher
-- and the retained services of the named owned machines, a write watch over their state and a
-- fetch watch over their retired action extents. After attach it presses the route keys (by default
-- PLAY and the playback keys), idles a fixed budget after each, captures the screen and
-- hashes the guest DRAM per MiB. Every hit samples the machine event and state halfwords.
--
-- compare: the dispatcher's entries and returns, the calls the machines make into the
-- retained services (calls returning into a retired extent) with the bytes they point at and r0
-- on return, the watched writes, the screens and the card's accepted writes, dirty sectors and
-- final command history must be equal, and the owned role must fetch nothing in the retired
-- extents outside the owned code (the functions OWNED_SYMBOLS places there).
--
-- same-memory: two observations of one role, DAT and route must have equal guest DRAM after
-- attach and after every step, whatever their boundaries.
module Main (main) where

import Control.DeepSeq (rnf)
import Control.Exception (SomeException, bracket, catch, displayException, evaluate, try)
import Control.Monad (foldM, forM, forM_, replicateM, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import qualified Data.ByteString.Char8 as BC
import Data.IORef
import Data.List (isPrefixOf, sort, sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import EmulatorUsb
import Numeric (readHex, showHex)
import Replacement.PlaybackActor.Actor
import qualified Replacement.PlaybackActor.Provenance as P
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), IOMode (..), hFlush, hSetBuffering, stdout, withBinaryFile)
import System.IO.Error (ioeGetErrorString, isUserError)
import GHC.Clock (getMonotonicTime)

data Role = Stock | Owned deriving (Eq)

roleName :: Role -> T.Text
roleName Stock = "stock"
roleName Owned = "owned"

data Inputs = Inputs
  { inputRole :: Role
  , inputBoundary :: Boundary
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

settleChunks, finalChunks, chunkInstructions, hitLimit, watchLimit, dramBytes, memoryChunk, memoryRead :: Int
settleChunks = 2
finalChunks = 2
chunkInstructions = 20000000
hitLimit = 4000000
watchLimit = 1048576
dramBytes = 0x20000000
memoryChunk = 0x100000
memoryRead = 4096

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

mergedSpans :: [Span] -> [Span]
mergedSpans = foldr merge [] . sortOn (\(Span s _) -> s)
  where
    merge range [] = [range]
    merge (Span s e) (Span s' e' : rest)
      | e >= s' = Span s (max e e') : rest
      | otherwise = Span s e : Span s' e' : rest

-- | The merged spans, the closest neighbours joined until at most the reset observation's 8 write
-- ranges remain. Writes the join adds are recorded but fall outside every compared span.
writeSpans :: [Span] -> [Span]
writeSpans = bound . mergedSpans
  where
    bound spans
      | length spans <= 8 = spans
      | otherwise = bound (joinAt (snd (minimum (zip (zipWith (\(Span _ e) (Span s _) -> s - e) spans (drop 1 spans)) [0 :: Int ..]))) spans)
    joinAt 0 (Span s _ : Span _ e : rest) = Span s e : rest
    joinAt k (range : rest) = range : joinAt (k - 1) rest
    joinAt _ [] = []

-- | Fetch records per retired extent and per PC; writes inside the boundary's write spans without
-- the role-specific PC, instruction count and tick, the writers' PCs apart, outside the
-- comparison. A CPU write's before and written bytes are byte lists in which a byte still marked
-- by provenance is null.
watchSummary :: Boundary -> M.Map Int P.MaskedWrite -> Value -> Value
watchSummary boundary masks watch =
  object
    [ "violation" .= field "violation" watch
    , "record_count" .= length records
    , "fetch_count" .= length fetches
    , "fetches_by_extent" .= object [K.fromString (hexWord s) .= n | (Span s _, n) <- byExtent, n > 0]
    , "fetches_by_pc" .= object [K.fromString (hexWord pc) .= n | (pc, n) <- M.toList byPc]
    , "writes" .= [object ["address" .= field "address" r, "size" .= field "size" r, "before" .= before i r, "value" .= value i r, "source" .= field "source" r, "core" .= field "core" r] | (i, r) <- writes]
    , "write_pcs" .= [field "pc" r | (_, r) <- writes]
    ]
  where
    records = arrayOf (field "records" watch)
    fetches = [r | r <- records, field "source" r == String "cpu-fetch"]
    writes =
      [ (i, r) | (i, r) <- zip [0 ..] records, field "source" r /= String "cpu-fetch", field "source" r /= String "cpu-read"
      , Just a <- [hexValue (field "address" r)], any (`inside` a) (boundaryWrites boundary) ]
    before i r = maybe (field "before" r) (A.toJSON . P.maskedBefore) (M.lookup i masks)
    value i r = maybe (field "value" r) (A.toJSON . P.maskedValue) (M.lookup i masks)
    fetchAddresses = mapMaybe (hexValue . field "address") fetches
    byExtent = [(extent, length (filter (inside extent) fetchAddresses)) | extent <- boundaryRetired boundary]
    byPc = M.fromListWith (+) [(a, 1 :: Int) | a <- fetchAddresses]

-- * Provenance of message 0x119

provenanceConfig :: P.Config
provenanceConfig = P.Config [(s, e) | Span s e <- provenanceMemory] appCopyLoads appCopyStores

-- | A guest DRAM address with the `0x40000000` alias removed.
dramPhysical :: Word32 -> Word32
dramPhysical a = if a >= 0x40000000 && a < 0x60000000 then a - 0x40000000 else a

registerValue :: T.Text -> Value -> Either String Word32
registerValue name hit = maybe (Left ("a provenance hit lacks " ++ T.unpack name)) (Right . dramPhysical) (hexValue (field name (field "registers" hit)))

numberOf :: T.Text -> Value -> Either String Int
numberOf name value = case field name value of
  Number n -> Right (truncate n)
  _ -> Left ("a provenance event lacks " ++ T.unpack name)

-- | Little-endian words from a hex byte string.
hexWords :: T.Text -> Either String [Word32]
hexWords text = do
  bytes <- byteList text
  pure [sum [fromIntegral b * 256 ^ k | (k, b) <- zip [0 :: Int ..] chunk] | chunk <- chunksOf4 bytes]
  where
    chunksOf4 [] = []
    chunksOf4 xs = take 4 xs : chunksOf4 (drop 4 xs)

byteList :: T.Text -> Either String [Word8]
byteList text
  | T.null text = Right []
  | [(b, "")] <- readHex (T.unpack (T.take 2 text)) = (b :) <$> byteList (T.drop 2 text)
  | otherwise = Left "provenance read bytes are not hexadecimal"

-- | The tracker event of a provenance site hit, its addresses physical.
hitEvent :: Value -> Either String (Maybe P.Event)
hitEvent hit = do
  core <- numberOf "core" hit
  case textOf (field "site" hit) of
    Just "provenance:send" -> do
      code <- case arrayOf (field "memory" hit) of
        [String text] -> hexWords text
        _ -> Left "the builder hit lacks its code bytes"
      count <- registerValue "r1" hit
      base <- registerValue "r2" hit
      stored <- P.builderStoredOffsets count code
      pure (Just (P.Built core base count stored))
    Just "provenance:sent" -> pure (Just (P.Sent core))
    Just label
      | Just _ <- stripPrefix "provenance:copy:" label -> do
          copy <- P.Copy <$> registerValue "r0" hit <*> registerValue "r1" hit <*> registerValue "r2" hit
          pure (Just (P.CopyBegins core copy))
      | Just name <- stripPrefix "provenance:copy-end:" label ->
          pure (Just (P.CopyEnds core (or [copySiteReleasesSource c | c <- messageCopies, copySiteName c == name])))
    _ -> pure Nothing

-- | The tracker event of a watch record, its addresses physical.
recordEvent :: Value -> Either String (Maybe P.Event)
recordEvent r = case field "source" r of
  String "cpu-read" -> do
    (core, pc, address, size) <- common
    pure (Just (P.Read core pc address size))
  String "cpu-write" -> do
    (core, pc, address, size) <- common
    before <- mapM byte (arrayOf (field "before" r))
    value <- case field "value" r of
      Number n | field "byte_order" r == String "little" -> Right [fromIntegral ((truncate n :: Integer) `div` (256 ^ k)) | k <- [0 .. size - 1]]
      _ -> Left "a CPU write record lacks a little-endian value"
    pure (Just (P.Write core pc address before value))
  _ -> pure Nothing
  where
    common = do
      core <- numberOf "core" r
      size <- numberOf "size" r
      pc <- maybe (Left "a CPU record lacks its pc") (Right . dramPhysical) (hexValue (field "pc" r))
      address <- maybe (Left "a CPU record lacks its physical address") Right (hexValue (field "physical" r))
      pure (core, pc, address, size)
    byte (Number n) = Right (truncate n)
    byte _ = Left "a CPU write record's before bytes are not numbers"

findingValue :: String -> P.Finding -> Value
findingValue window finding =
  object
    [ "window" .= window
    , "kind" .= (case finding of P.Interpreted {} -> "interpreted"; P.Untracked {} -> "untracked-copy"; P.Escaped {} -> "escaped" :: T.Text)
    , "pc" .= (case finding of P.Escaped {} -> Null; _ -> String (T.pack (hexWord (P.findingPc finding))))
    , "address" .= hexWord (P.findingAddress finding)
    , "message" .= P.originMessage (P.findingOrigin finding)
    , "offset" .= P.originOffset (P.findingOrigin finding)
    ]

-- | Steps the tracker through one watch window: its provenance hits and records in instruction
-- order, hits first at equal counts. Returns the masks of the window's write records by index.
trackWindow :: IORef (P.Tracker, [Value]) -> String -> [Value] -> [Value] -> IO (M.Map Int P.MaskedWrite)
trackWindow state window hits records = do
  hitEvents <- either (ioError . userError) pure (sequence [fmap ((,,) i (0 :: Int) . Left) <$> hitEvent h | (i, h) <- zip [0 :: Int ..] hits])
  recordEvents <- either (ioError . userError) pure (sequence [fmap ((,,) i (1 :: Int) . Right) <$> recordEvent r | (i, r) <- zip [0 :: Int ..] records])
  let timed = [(instructions, order, i, e) | (Just (i, order, e), instructions) <- zip hitEvents (map instructionsOf hits) ++ zip recordEvents (map instructionsOf records)]
      instructionsOf v = case field "instructions" v of
        Number n -> truncate n :: Integer
        _ -> -1
  (t0, found0) <- readIORef state
  started <- getMonotonicTime
  putStrLn (window ++ ": " ++ show (length records) ++ " records, " ++ show (length hits) ++ " provenance hits")
  let ordered = sortOn (\(n, o, i, _) -> (n, o, i)) timed
      (t1, findings, masked) = P.track t0 [either id id e | (_, _, _, e) <- ordered]
      masks = M.fromList [(i, m) | ((_, _, i, Right _), Just m) <- zip ordered masked]
      found1 = found0 ++ map (findingValue window) findings
  _ <- evaluate (M.size masks + length found1)
  finished <- getMonotonicTime
  putStrLn (window ++ ": tracked in " ++ show (finished - started) ++ " s, " ++ show (length found1) ++ " findings, " ++ show (M.size (P.marked t1)) ++ " marked")
  writeIORef state (t1, found1)
  pure masks

-- | A screen capture, or Left the bridge's refusal (no display plane is up) as the screen.
captureScreen :: Emulator -> String -> IO (Either Value Value)
captureScreen emulator label = (Right <$> emulatorCapture emulator label) `catch` refused
  where
    refused e
      | isUserError e = pure (Left (object ["refused" .= ioeGetErrorString e]))
      | otherwise = ioError e

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  case arguments of
    ("observe" : rest) -> parseInputs rest >>= observeMain
    ["compare", stock, owned, symbols, output] -> compareMain stock owned symbols output
    ["same-memory", first, second, output] -> sameMemoryMain first second output
    _ -> ioError (userError "usage: playback-actor-observe observe OPTIONS | compare STOCK_DIR OWNED_DIR OWNED_SYMBOLS OUTPUT_JSON | same-memory FIRST_DIR SECOND_DIR OUTPUT_JSON")

parseInputs :: [String] -> IO Inputs
parseInputs arguments = do
  pairs <- collect arguments
  let values = M.fromList pairs
      names = ["machines", "role", "update", "run", "boot-rom", "adjustment-nand", "bridge", "output"]
  need (length pairs == M.size values && M.keys (foldr M.delete values ["card", "route"]) == sort names) "observe requires each of --machines --role --update --run --boot-rom --adjustment-nand --bridge --output once, and at most one --card and --route"
  role <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  boundary <- maybe (ioError (userError "--machines must name distinct owned machines, comma-separated")) pure (boundaryParse (values M.! "machines"))
  keys <- case M.lookup "route" values of
    Nothing -> pure playbackRoute
    Just text -> mapM crawlKey (splitOn text)
  pure
    Inputs
      { inputRole = role, inputBoundary = boundary, inputUpdate = values M.! "update", inputRun = values M.! "run", inputRom = values M.! "boot-rom"
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
  record "boundary" (A.toJSON (boundaryWire (inputBoundary inputs)))
  record "inputs" (object (bindings ++ ["observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]]))
  let boundary = inputBoundary inputs
      machineReads = A.toJSON [[s, e - s] | Span s e <- [machineEvents, machineStates]]
      Span builderStart builderCall = message119Builder
      -- Provenance sites sit inside functions and track no return.
      sites =
        (dispatcher, "dispatch", Just (Capture [] []), Just machineReads)
          : [(serviceAddress s, "service:" ++ serviceName s, Just (serviceCapture s), Just machineReads) | s <- boundaryServices boundary]
          ++ (builderCall, "provenance:send", Nothing, Just (A.toJSON [[builderStart, builderCall - builderStart]]))
          : (message119Sent, "provenance:sent", Nothing, Nothing)
          : concat
            [ [(copySiteCall c, "provenance:copy:" ++ copySiteName c, Nothing, Nothing), (copySiteCall c + 4, "provenance:copy-end:" ++ copySiteName c, Nothing, Nothing)]
            | c <- messageCopies ]
      writeRanges = mergedSpans (boundaryWrites boundary ++ provenanceMemory)
      writes = map spanValue writeRanges
      readSpans = map spanValue provenanceMemory
      fetches = map spanValue (boundaryRetired boundary)
      -- Before attach the reset observation watches only the boundary's writes, and a message
      -- built there refuses the observation.
      spec =
        object
          [ "schema" .= (1 :: Int)
          , "sites" .= object [siteKey a .= label | (a, label, _, _) <- sites]
          , "reads" .= object [siteKey a .= r | (a, _, _, Just r) <- sites]
          , "captures" .= object [siteKey a .= captureValue capture | (a, _, Just capture, _) <- sites]
          , "task" .= ("logical" :: T.Text)
          , "hit_limit" .= hitLimit
          , "write_ranges" .= map spanValue (writeSpans (boundaryWrites boundary))
          , "fetch_ranges" .= fetches
          , "watch_limit" .= watchLimit
          ]
      specPath = directory </> "spec.json"
      watch =
        object
          [ "command" .= ("watch-begin" :: T.Text), "views" .= ("fixed" :: T.Text), "limit" .= watchLimit
          , "ranges" .= map spanValue (mergedSpans (writeRanges ++ boundaryRetired boundary))
          , "write_ranges" .= writes, "read_ranges" .= readSpans, "fetch_ranges" .= fetches ]
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
  pending <- newIORef ([] :: [(Int, Value)])
  provenance <- newIORef (P.tracker provenanceConfig, [])
  withBinaryFile (directory </> "hits.jsonl") WriteMode $ \hits -> do
    let drain emulator = do
          start <- readIORef cursor
          reply <- emulatorTraceHitsPage emulator start 65536
          let values = arrayOf (field "hits" reply)
          forM_ values (\value -> BL.hPut hits (A.encode value <> "\n"))
          modifyIORef' pending (++ [(i, v) | (i, v) <- zip [start ..] values, maybe False ("provenance:" `isPrefixOf`) (textOf (field "site" v))])
          let end = start + length values
          writeIORef cursor end
          hFlush hits
          case field "count" reply of
            Number total | end < truncate total -> drain emulator
            _ -> pure end
        -- The provenance hits drained before the given hit count, in order.
        takeHits end = do
          (taken, kept) <- span ((< end) . fst) <$> readIORef pending
          writeIORef pending kept
          pure (map snd taken)
        summarise window end observed = do
          windowHits <- takeHits end
          masks <- trackWindow provenance window windowHits (arrayOf (field "records" observed))
          pure (watchSummary boundary masks observed)
        idle emulator count = replicateM count $ do
          ran <- emulatorBridgeCommand emulator (object ["command" .= ("idle" :: T.Text), "instructions" .= chunkInstructions])
          media <- emulatorBridgeCommand emulator (object ["command" .= ("media-state" :: T.Text)])
          pure (object ["idle" .= ran, "media" .= media])
        saveSteps = BL.writeFile (directory </> "steps.json") . A.encode . reverse =<< readIORef steps
        memory emulator name = do
          chunks <- forM [0 .. dramBytes `div` memoryChunk - 1] $ \chunk -> do
            pages <- forM [0 .. memoryChunk `div` memoryRead - 1] $ \page -> do
              reply <- emulatorBridgeCommand emulator (object ["command" .= ("read-memory" :: T.Text), "address" .= (chunk * memoryChunk + page * memoryRead), "count" .= memoryRead])
              case field "bytes" reply of
                String hexBytes | T.length hexBytes == 2 * memoryRead -> pure (TE.encodeUtf8 hexBytes)
                _ -> ioError (userError "read-memory returned no bytes")
            let digest = sha256Hex (sha256 (B.concat pages))
            digest `seq` pure digest
          BL.writeFile (directory </> ("memory-" ++ name ++ ".json")) (A.encode (object ["chunk_bytes" .= memoryChunk, "chunks" .= chunks]) <> "\n")
          pure (sha256Hex (sha256 (BC.pack (concat chunks))))
    -- The bridge exits after refusing a capture.
    gone <- newIORef False
    let capture emulator label = do
          shot <- captureScreen emulator label
          either (const (writeIORef gone True)) (const (pure ())) shot
          pure shot
    bracket (startEmulator (paths M.! "bridge") directory bridgeArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, ()) -> readIORef gone >>= \g -> unless g (stopEmulator emulator)) $ \(emulator, start, ()) -> do
      need (bootStart start == FreshBootStart) "bridge did not fresh boot"
      record "emulator_start" (startRecord start)
      attached <- drain emulator
      reset <- B.readFile (directory </> "reset-observation.json") >>= either (ioError . userError) pure . A.eitherDecodeStrict'
      segments <- forM (arrayOf (field "segments" reset)) $ \s -> do
        end <- either (ioError . userError) pure (numberOf "hits_end" s)
        summary <- summarise ("reset-" ++ fromMaybe "?" (textOf (field "phase" s))) end (field "watch" s)
        pure (object ["phase" .= field "phase" s, "instructions" .= field "instructions" s, "hits_end" .= field "hits_end" s, "watch" .= summary])
      record "reset_segments" (A.toJSON segments)
      builtBeforeAttach <- P.builtMessages . fst <$> readIORef provenance
      need (builtBeforeAttach == 0) "a 0x119 message was built before attach, where its memory is not watched"
      record "hits_at_attach" (A.toJSON attached)
      shot <- capture emulator "attached"
      record "attached_screen" (either id id shot)
      let route [] = pure True
          route ((index, key) : rest) = do
            first <- readIORef cursor
            _ <- emulatorBridgeCommand emulator watch
            press <- emulatorBridgeCommand emulator (object ["command" .= ("key" :: T.Text), "key" .= crawlKeyWire key])
            settled <- idle emulator settleChunks
            observed <- emulatorWatchEnd emulator
            last' <- drain emulator
            summary <- summarise ("step-" ++ show index) (last' + 1) observed
            let common = ["step" .= index, "key" .= crawlKeyWire key, "hits" .= [first, last'], "press" .= press, "idle" .= settled, "watch" .= summary]
            screen <- capture emulator ("step-" ++ show index)
            case screen of
              Left refusal -> do
                modifyIORef' steps (object (common ++ ["screen" .= refusal]) :)
                saveSteps
                putStrLn ("step " ++ show index ++ " " ++ crawlKeyWire key ++ ": capture refused, route ends")
                pure False
              Right shown -> do
                state <- emulatorBridgeCommand emulator (object ["command" .= ("ui-state" :: T.Text)])
                putStrLn ("step-" ++ show index ++ ": captured")
                memoryDigest <- memory emulator (show index)
                modifyIORef' steps (object (common ++ ["screen" .= shown, "ui_state" .= state, "memory" .= memoryDigest]) :)
                saveSteps
                putStrLn ("step " ++ show index ++ " " ++ crawlKeyWire key ++ " hits " ++ show (last' - first))
                route rest
      live <- case shot of
        Left _ -> pure False
        Right _ -> do
          record "attached_memory" . A.toJSON =<< memory emulator "attached"
          route (zip [1 :: Int ..] (inputRoute inputs))
      when live $ do
        first <- readIORef cursor
        _ <- emulatorBridgeCommand emulator watch
        settled <- idle emulator finalChunks
        observed <- emulatorWatchEnd emulator
        last' <- drain emulator
        summary <- summarise "final" (last' + 1) observed
        screen <- capture emulator "final"
        memoryDigest <- either (const (pure Null)) (const (A.toJSON <$> memory emulator (show (length (inputRoute inputs) + 1)))) screen
        modifyIORef' steps (object ["step" .= (length (inputRoute inputs) + 1), "key" .= Null, "hits" .= [first, last'], "idle" .= settled, "watch" .= summary, "screen" .= either id id screen, "memory" .= memoryDigest] :)
        saveSteps
      record "hit_count" . A.toJSON =<< readIORef cursor
      (finalTracker, findings) <- readIORef provenance
      let provenanceRecord =
            object
              [ "messages_built" .= P.builtMessages finalTracker
              , "marked_at_end" .= M.size (P.marked finalTracker)
              , "watched" .= map spanValue provenanceMemory
              , "finding_count" .= length findings
              , "findings" .= take 64 findings
              ]
      BL.writeFile (directory </> "provenance.json") (A.encode (object ["findings" .= findings]) <> "\n")
      record "provenance" provenanceRecord

-- * Comparison

data Event = Event {eventKind :: String, eventName :: String, eventExternal :: Bool, eventValue :: Value}

-- | A return address inside a retired extent belongs to the actor.
external :: Boundary -> Word32 -> Bool
external boundary address = not (any (`inside` address) (boundaryRetired boundary))

registersOf :: [T.Text] -> Value -> Value
registersOf names hit = object [K.fromText r .= field r (field "registers" hit) | r <- names]

taskOf :: Value -> Value
taskOf hit = object ["kernel" .= field "kernel" (field "task" hit), "task_id" .= field "task_id" (field "task" hit), "kind" .= field "kind" (field "task" hit)]

readsOf :: Value -> Value
readsOf hit = A.toJSON [field "bytes" r | r <- arrayOf (field "reads" (field "observation" hit))]

event :: Boundary -> Value -> Maybe Event
event boundary hit = do
  label <- textOf (field "site" hit)
  address <- hexValue (field "address" hit)
  lr <- hexValue (field "lr" hit)
  let common = ["core" .= field "core" hit, "task" .= taskOf hit, "machines" .= field "memory" hit]
      serviceSites = M.fromList [(serviceAddress s, s) | s <- boundaryServices boundary]
  case label of
    "dispatch" -> Just (Event "dispatch" "dispatch" True (object (["kind" .= ("dispatch" :: T.Text), "machine" .= registersOf ["r0"] hit] ++ common)))
    _ | "service:" `isPrefixOf` label, Just s <- M.lookup address serviceSites, not (external boundary lr) ->
          Just (Event "service" (serviceName s) True (object (["kind" .= ("service" :: T.Text), "name" .= serviceName s, "arguments" .= registersOf (serviceArguments s) hit, "reads" .= readsOf hit] ++ common)))
      | "service:" `isPrefixOf` label -> Nothing
      | label == "call-return" -> do
          site <- hexValue (field "site" (field "observation" hit))
          case (site == dispatcher, M.lookup site serviceSites) of
            (True, _) -> Just (Event "dispatch-return" "dispatch" True (object (["kind" .= ("dispatch-return" :: T.Text), "result" .= registersOf ["r0"] hit] ++ common)))
            (_, Just s)
              | external boundary address -> Nothing
              | otherwise -> Just (Event "service-return" (serviceName s) True (object (["kind" .= ("service-return" :: T.Text), "name" .= serviceName s, "result" .= registersOf (serviceResult s) hit, "reads" .= readsOf hit] ++ common)))
            _ -> Nothing
      | otherwise -> Nothing

loadEvents :: Boundary -> FilePath -> IO [Event]
loadEvents boundary directory = do
  bytes <- BL.readFile (directory </> "hits.jsonl")
  reverse <$> foldM keep [] (filter (not . BL.null) (BLC.lines bytes))
  where
    keep kept line = case A.eitherDecode line of
      Left problem -> ioError (userError problem)
      Right hit -> case event boundary hit of
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

-- | The owned functions inside the retired extents, from a `.symbols` listing.
readBody :: Boundary -> FilePath -> IO [Span]
readBody boundary path = do
  rows <- map words . lines <$> readFile path
  let spans = [Span a (a + n) | [address, size, kind, _] <- rows, kind `elem` ["T", "t"], [(a, "")] <- [readHex address], [(n, "")] <- [readHex size], n > 0, any (`inside` a) (boundaryRetired boundary)]
  need (not (null spans)) (path ++ " places no function in a retired extent")
  pure spans

receiptBoundary :: Value -> IO Boundary
receiptBoundary receipt = maybe (ioError (userError "an observation names no machines")) pure (textOf (field "boundary" receipt) >>= boundaryParse)

compareMain :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
compareMain stockDir ownedDir symbols output = do
  exists <- doesPathExist output
  need (not exists) "comparison output must be new"
  stockReceipt <- loadJson (stockDir </> "receipt.json")
  ownedReceipt <- loadJson (ownedDir </> "receipt.json")
  boundary <- receiptBoundary stockReceipt
  ownedBoundary <- receiptBoundary ownedReceipt
  need (boundary == ownedBoundary) "the observations watch different boundaries"
  body <- readBody boundary symbols
  let load directory = do
        events <- loadEvents boundary directory
        steps <- arrayOf <$> loadJson (directory </> "steps.json")
        pure (events, steps)
  (stockEvents, stockSteps) <- load stockDir
  (ownedEvents, ownedSteps) <- load ownedDir
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
      ownedFetches = sum [n | w <- watches ownedReceipt ownedSteps, Object byPc <- [field "fetches_by_pc" w], (pc, Number c) <- KM.toList byPc, Just a <- [hexValue (String (K.toText pc))], not (any (`inside` a) body), let n = truncate c :: Integer]
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
      provenanceClean receipt = field "finding_count" (field "provenance" receipt) == Number 0
      maskedBytes writes = length [() | w <- writes, b <- arrayOf (field "before" w) ++ arrayOf (field "value" w), b == Null]
      equal = behaviorEqual && provenanceClean stockReceipt && provenanceClean ownedReceipt && (not ownedRole || ownedFetches == 0)
      summary events receipt steps fetches writes = object
        [ "inputs" .= field "inputs" receipt, "hits" .= field "hit_count" receipt, "events" .= length events
        , "interface_events" .= length (interface events), "internal_events" .= length (internal events)
        , "retired_fetches" .= fetches, "watched_writes" .= length writes, "steps" .= length steps ]
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "boundary" .= boundaryWire boundary
    , "retired" .= map spanValue (boundaryRetired boundary)
    , "owned_body" .= map spanValue body
    , "stock" .= summary stockEvents stockReceipt stockSteps (fetchTotal stockReceipt stockSteps) stockWrites
    , "owned" .= summary ownedEvents ownedReceipt ownedSteps ownedFetches ownedWrites
    , "entry_counts" .= countTable
    , "interface_sequence" .= firstDifferences 20 (interface stockEvents) (interface ownedEvents)
    , "internal_sequence" .= firstDifferences 20 (internal stockEvents) (internal ownedEvents)
    , "watched_writes" .= firstDifferences 20 stockWrites ownedWrites
    , "provenance" .= object
        [ "stock" .= field "provenance" stockReceipt, "owned" .= field "provenance" ownedReceipt
        , "masked_bytes" .= object ["stock" .= maskedBytes stockWrites, "owned" .= maskedBytes ownedWrites] ]
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

-- * Guest memory identity across boundaries

-- | The DRAM chunks, by guest physical address, whose digests differ.
differingChunks :: Value -> Value -> [Value]
differingChunks first second =
  [ object ["address" .= hexWord (fromIntegral (i * memoryChunk)), "first" .= a, "second" .= b]
  | (i, a, b) <- zip3 [0 :: Int ..] (arrayOf (field "chunks" first)) (arrayOf (field "chunks" second)), a /= b
  ]

sameMemoryMain :: FilePath -> FilePath -> FilePath -> IO ()
sameMemoryMain firstDir secondDir output = do
  exists <- doesPathExist output
  need (not exists) "same-memory output must be new"
  firstReceipt <- loadJson (firstDir </> "receipt.json")
  secondReceipt <- loadJson (secondDir </> "receipt.json")
  forM_ [firstReceipt, secondReceipt] $ \r -> need (field "status" (field "outcome" r) == String "completed") "an observation did not complete"
  forM_ ["role", "inputs", "route", "budget"] $ \name ->
    need (field name firstReceipt == field name secondReceipt) ("the observations differ in " ++ T.unpack name)
  firstSteps <- arrayOf <$> loadJson (firstDir </> "steps.json")
  secondSteps <- arrayOf <$> loadJson (secondDir </> "steps.json")
  need (length firstSteps == length secondSteps) "the observations ran different step counts"
  let points = ("attached", field "attached_memory" firstReceipt, field "attached_memory" secondReceipt)
        : [(show (n :: Int), field "memory" a, field "memory" b) | (n, a, b) <- zip3 [1 ..] firstSteps secondSteps]
  rows <- forM points $ \(name, a, b) -> do
    need (a /= Null && b /= Null) ("step " ++ name ++ " has no memory snapshot")
    firstChunks <- loadJson (firstDir </> ("memory-" ++ name ++ ".json"))
    secondChunks <- loadJson (secondDir </> ("memory-" ++ name ++ ".json"))
    let differing = differingChunks firstChunks secondChunks
        same = a == b && null differing
    pure (same, object ["step" .= name, "first" .= a, "second" .= b, "equal" .= same, "differing_chunks" .= differing])
  let equal = all fst rows
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "role" .= field "role" firstReceipt
    , "route" .= field "route" firstReceipt
    , "boundaries" .= [field "boundary" firstReceipt, field "boundary" secondReceipt]
    , "dram_bytes" .= dramBytes
    , "steps" .= map snd rows
    , "verdict" .= (if equal then "equal" else "different" :: T.Text)
    ]
  putStrLn ("memory " ++ (if equal then "equal" else "different"))
  unless equal exitFailure
