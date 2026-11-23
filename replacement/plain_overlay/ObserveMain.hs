{-# LANGUAGE OverloadedStrings #-}

-- | plain-overlay-observe observe --role stock|owned --update DAT --plain PLAIN --unit UNIT_JSON...
--     --run JSON --boot-rom ROM --adjustment-nand NAND [--card IMG] [--route KEY,...] --bridge BRIDGE
--     --output NEW_DIR
--   plain-overlay-observe compare STOCK_DIR OTHER_DIR OUTPUT_JSON [OWNED_ELF...]
--
-- observe: fresh-boots the RAM-push bridge with trace sites at every unit entry, a fetch watch over
-- the units' extents and a write watch over the RAM they own. PLAIN is the plain image the DAT
-- carries. After attach and after each route key it reads each units' overlay back and compares
-- it with PLAIN, and digests guest DRAM per MiB with the units' extents and rows zeroed.
--
-- compare: the entry hits and returns, the watched writes, the overlay residency, the DRAM digests,
-- the screens and the card's accepted writes, dirty sectors and final command history must be
-- equal, and an owned observation must fetch nothing in the extents outside the functions its
-- OWNED_ELF files place there.
module Main (main) where

import qualified ArmElf as E
import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM, forM_, replicateM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.IORef
import Data.List (isPrefixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import EmulatorUsb
import Numeric (readHex)
import Replacement.PlainOverlay.Unit
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
  , inputPaths :: M.Map String FilePath
  , inputUnits :: [FilePath]
  , inputRoute :: [CrawlKey]
  }

settleChunks, finalChunks, chunkInstructions, hitLimit, watchLimit, dramBytes, memoryChunk, memoryRead :: Int
settleChunks = 2
finalChunks = 2
chunkInstructions = 20000000
hitLimit = 4000000
watchLimit = 1048576
dramBytes = 0x20000000
memoryChunk = 0x100000
memoryRead = 0x10000

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

mergedSpans :: [Span] -> [Span]
mergedSpans = foldr merge [] . sortOn spanStart
  where
    merge range [] = [range]
    merge (Span s e) (Span s' e' : rest)
      | e >= s' = Span s (max e e') : rest
      | otherwise = Span s e : Span s' e' : rest

-- * Observation

parseInputs :: [String] -> IO Inputs
parseInputs arguments = do
  pairs <- collect arguments
  let units = [v | ("unit", v) <- pairs]
      others = [(k, v) | (k, v) <- pairs, k /= "unit"]
      values = M.fromList others
      required = ["adjustment-nand", "boot-rom", "bridge", "output", "plain", "role", "run", "update"]
  need (length others == M.size values && M.keys (foldr M.delete values ["card", "route"]) == required && not (null units))
    "observe requires each of --role --update --plain --run --boot-rom --adjustment-nand --bridge --output once, at least one --unit, and at most one --card and --route"
  role <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  keys <- maybe (pure [KeyPlay, KeyLeft, KeyLeft, KeyRight, KeyRight]) (mapM crawlKey . splitOn) (M.lookup "route" values)
  pure (Inputs role (M.delete "role" (M.delete "route" values)) units keys)
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

loadUnits :: [FilePath] -> IO [Unit]
loadUnits paths = forM paths $ \path -> B.readFile path >>= either (ioError . userError) pure . parseUnit

-- | Fetch and write records summarised without role-specific PCs, ticks or instruction counts.
watchSummary :: Value -> Value
watchSummary watch =
  object
    [ "violation" .= field "violation" watch
    , "record_count" .= length records
    , "fetch_count" .= length fetches
    , "fetches_by_pc" .= object [K.fromString (hexWord pc) .= n | (pc, n) <- M.toList byPc]
    , "writes" .= [object ["address" .= field "address" r, "size" .= field "size" r, "before" .= field "before" r, "value" .= field "value" r, "source" .= field "source" r, "core" .= field "core" r] | r <- writes]
    ]
  where
    records = arrayOf (field "records" watch)
    fetches = [r | r <- records, field "source" r == String "cpu-fetch"]
    writes = [r | r <- records, field "source" r /= String "cpu-fetch", field "source" r /= String "cpu-read"]
    byPc = M.fromListWith (+) [(a, 1 :: Int) | a <- mapMaybe (hexValue . field "address") fetches]

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  case arguments of
    ("observe" : rest) -> parseInputs rest >>= observeMain
    ("compare" : stock : other : output : elves) -> compareMain stock other output elves
    _ -> ioError (userError "usage: plain-overlay-observe observe OPTIONS | compare STOCK_DIR OTHER_DIR OUTPUT_JSON [OWNED_ELF...]")

observeMain :: Inputs -> IO ()
observeMain inputs = do
  let output = inputPaths inputs M.! "output"
  exists <- doesPathExist output
  need (not exists) "--output must be new"
  directory <- makeAbsolute output
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
  paths <- mapM makeAbsolute (M.delete "output" (inputPaths inputs))
  units <- loadUnits (inputUnits inputs)
  bindings <- forM (M.toList paths) $ \(name, path) -> do
    digest <- sha256File path
    pure (K.fromString name .= object ["path" .= path, "sha256" .= sha256Hex digest])
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  plain <- B.readFile (paths M.! "plain")
  record "schema" (Number 1)
  record "role" (String (roleName (inputRole inputs)))
  record "units" (A.toJSON (map unitValue units))
  record "inputs" (object (bindings ++ ["observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]]))
  let extents = mergedSpans (concatMap unitExtents units)
      stateSpans = mergedSpans (concatMap unitState units)
      masked = mergedSpans (concatMap unitSpans units)
      overlays = nub (map unitOverlay units)
      sites = [(entryAddress e, "entry:" ++ unitName u ++ ":" ++ entryName e) | u <- units, e <- unitEntries u]
      spec =
        object
          [ "schema" .= (1 :: Int)
          , "sites" .= object [K.fromString (hexWord a) .= label | (a, label) <- sites]
          , "task" .= ("logical" :: T.Text)
          , "hit_limit" .= hitLimit
          , "write_ranges" .= map spanValue stateSpans
          , "fetch_ranges" .= map spanValue extents
          , "watch_limit" .= watchLimit
          ]
      specPath = directory </> "spec.json"
      watch =
        object
          [ "command" .= ("watch-begin" :: T.Text), "views" .= ("fixed" :: T.Text), "limit" .= watchLimit
          , "ranges" .= map spanValue (mergedSpans (extents ++ stateSpans))
          , "write_ranges" .= map spanValue stateSpans, "fetch_ranges" .= map spanValue extents ]
      bridgeArguments =
        [ "--run", paths M.! "run", "--boot-rom", paths M.! "boot-rom", "--installed-dat", paths M.! "update"
        , "--adjustment-nand", paths M.! "adjustment-nand"
        , "--output", directory, "--ram-push-checkpoints", directory </> "unused-checkpoints"
        , "--fresh-boot", "--reset-observation", specPath ]
          ++ maybe [] (\card -> ["--sd-card-image", card]) (M.lookup "card" paths)
      expected overlay = let Span s e = overlaySpan overlay in B.take (fromIntegral (e - s)) (B.drop (fromIntegral (s - plainBase)) plain)
  need (all (\o -> let Span _ e = overlaySpan o in fromIntegral (B.length plain) >= e - plainBase) overlays) "--plain does not hold the units' overlays"
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
        residency emulator = forM overlays $ \overlay -> do
          let Span s e = overlaySpan overlay
          held <- B.concat <$> forM [s, s + fromIntegral memoryRead .. e - 1] (\a -> emulatorReadMemory emulator a (min memoryRead (fromIntegral (e - a))))
          let want = expected overlay
          pure (object ["overlay" .= overlayNumber overlay, "sha256" .= sha256Hex (sha256 held), "expected_sha256" .= sha256Hex (sha256 want), "resident" .= (held == want)])
        memory emulator name = do
          chunks <- forM [0 .. dramBytes `div` memoryChunk - 1] $ \chunk -> do
            let base = fromIntegral (chunk * memoryChunk) :: Word32
            pages <- forM [0 .. memoryChunk `div` memoryRead - 1] $ \page -> emulatorReadMemory emulator (base + fromIntegral (page * memoryRead)) memoryRead
            let digest = sha256Hex (sha256 (mask base (B.concat pages)))
            digest `seq` pure digest
          BL.writeFile (directory </> ("memory-" ++ name ++ ".json")) (A.encode (object ["chunk_bytes" .= memoryChunk, "masked" .= map spanValue masked, "chunks" .= chunks]) <> "\n")
          pure (sha256Hex (sha256 (BC.pack (concat chunks))))
        mask base bytes = foldl (zeroSpan base) bytes masked
        zeroSpan base bytes (Span s e)
          | e <= base || s >= base + fromIntegral (B.length bytes) = bytes
          | otherwise =
              let from = fromIntegral (max s base - base)
                  to = fromIntegral (min e (base + fromIntegral (B.length bytes)) - base)
               in B.take from bytes <> B.replicate (to - from) 0 <> B.drop to bytes
        point emulator name = do
          resident <- residency emulator
          digest <- memory emulator name
          pure (resident, digest)
    bracket (startEmulator (paths M.! "bridge") directory bridgeArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, ()) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      need (bootStart start == FreshBootStart) "bridge did not fresh boot"
      record "emulator_start" (startRecord start)
      attached <- drain emulator
      reset <- B.readFile (directory </> "reset-observation.json") >>= either (ioError . userError) pure . A.eitherDecodeStrict'
      record "reset_segments" (A.toJSON [object ["phase" .= field "phase" s, "instructions" .= field "instructions" s, "hits_end" .= field "hits_end" s, "watch" .= watchSummary (field "watch" s)] | s <- arrayOf (field "segments" reset)])
      record "hits_at_attach" (A.toJSON attached)
      record "attached_screen" =<< emulatorCapture emulator "attached"
      (attachedResidency, attachedMemory) <- point emulator "attached"
      record "attached_residency" (A.toJSON attachedResidency)
      record "attached_memory" (A.toJSON attachedMemory)
      let step index key = do
            first <- readIORef cursor
            _ <- emulatorBridgeCommand emulator watch
            press <- maybe (pure Null) (\k -> emulatorBridgeCommand emulator (object ["command" .= ("key" :: T.Text), "key" .= crawlKeyWire k])) key
            settled <- idle emulator (maybe finalChunks (const settleChunks) key)
            observed <- emulatorWatchEnd emulator
            last' <- drain emulator
            screen <- emulatorCapture emulator (maybe "final" (const ("step-" ++ show index)) key)
            (resident, digest) <- point emulator (show index)
            modifyIORef' steps (object
              [ "step" .= index, "key" .= fmap crawlKeyWire key, "hits" .= [first, last'], "press" .= press, "idle" .= settled
              , "watch" .= watchSummary observed, "screen" .= screen, "residency" .= resident, "memory" .= digest ] :)
            saveSteps
            putStrLn ("step " ++ show index ++ " " ++ maybe "final" crawlKeyWire key ++ " hits " ++ show (last' - first))
            pure last'
      forM_ (zip [1 :: Int ..] (inputRoute inputs)) $ \(index, key) -> step index (Just key)
      total <- step (length (inputRoute inputs) + 1) Nothing
      record "hit_count" (A.toJSON total)

-- * Comparison

-- | An entry hit or return, and whether its return address lies outside every extent.
data Event = Event {eventName :: String, eventKind :: String, eventInterface :: Bool, eventValue :: Value}

registerOf :: Value -> Int -> Maybe Word32
registerOf hit r = hexValue (field (T.pack ('r' : show r)) (field "registers" hit))

event :: [Unit] -> Value -> Maybe Event
event units hit = do
  label <- textOf (field "site" hit)
  address <- hexValue (field "address" hit)
  let common = ["core" .= field "core" hit, "task" .= object ["kernel" .= field "kernel" task, "task_id" .= field "task_id" task, "kind" .= field "kind" task]]
      task = field "task" hit
      outside a = not (any (\u -> any (`inside` a) (unitExtents u)) units)
      named = M.fromList [(entryAddress e, unitName u ++ ":" ++ entryName e) | u <- units, e <- unitEntries u]
  case label of
    _ | "entry:" `isPrefixOf` label, Just name <- M.lookup address named, Just lr <- hexValue (field "lr" hit) ->
          Just (Event name "entry" (outside lr) (object (["kind" .= ("entry" :: T.Text), "name" .= name, "lr" .= hexWord lr, "arguments" .= [fmap hexWord (registerOf hit r) | r <- [0 .. 3]]] ++ common)))
      | label == "call-return" -> do
          site <- hexValue (field "site" (field "observation" hit))
          name <- M.lookup site named
          Just (Event name "return" (outside address) (object (["kind" .= ("return" :: T.Text), "name" .= name, "return_address" .= hexWord address, "result" .= fmap hexWord (registerOf hit 0)] ++ common)))
      | otherwise -> Nothing

loadEvents :: [Unit] -> FilePath -> IO [Event]
loadEvents units directory = do
  bytes <- BL.readFile (directory </> "hits.jsonl")
  forM (filter (not . BL.null) (BLC.lines bytes)) (either (ioError . userError) pure . A.eitherDecode) >>= pure . mapMaybe (event units)

loadJson :: FilePath -> IO Value
loadJson path = B.readFile path >>= either (ioError . userError) pure . A.eitherDecodeStrict'

firstDifferences :: Int -> [Value] -> [Value] -> Value
firstDifferences limit one two =
  object
    [ "stock_length" .= length one, "other_length" .= length two, "equal" .= (one == two)
    , "differences" .= take limit [object ["index" .= i, "stock" .= s, "other" .= o] | (i, s, o) <- zip3 [0 :: Int ..] (pad one) (pad two), s /= o] ]
  where
    total = max (length one) (length two)
    pad xs = take total (xs ++ repeat Null)

-- | The function symbols an owned ELF places inside the units' extents.
ownedFunctions :: [Unit] -> FilePath -> IO [Span]
ownedFunctions units path = do
  elf <- B.readFile path >>= either (ioError . userError) pure . E.parseElf
  pure
    [ Span a (a + E.symbolSize s)
    | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0
    , let a = E.symbolValue s
    , any (\u -> any (`inside` a) (unitExtents u)) units
    ]

-- | The DRAM chunks whose masked digests differ, by guest address.
differingChunks :: Value -> Value -> [Value]
differingChunks one two =
  [ object ["address" .= hexWord (fromIntegral (i * memoryChunk)), "stock" .= a, "other" .= b]
  | (i, a, b) <- zip3 [0 :: Int ..] (arrayOf (field "chunks" one)) (arrayOf (field "chunks" two)), a /= b
  ]

compareMain :: FilePath -> FilePath -> FilePath -> [FilePath] -> IO ()
compareMain stockDir otherDir output elves = do
  exists <- doesPathExist output
  need (not exists) "comparison output must be new"
  stockReceipt <- loadJson (stockDir </> "receipt.json")
  otherReceipt <- loadJson (otherDir </> "receipt.json")
  forM_ [stockReceipt, otherReceipt] $ \r -> need (field "status" (field "outcome" r) == String "completed") "an observation did not complete"
  need (field "role" stockReceipt == String (roleName Stock)) "the first observation must have the stock role"
  need (field "units" stockReceipt == field "units" otherReceipt) "the observations watch different units"
  units <- either (ioError . userError) pure (mapM (parseUnit . BL.toStrict . A.encode) (arrayOf (field "units" stockReceipt)))
  let otherOwned = field "role" otherReceipt == String (roleName Owned)
  need (otherOwned == not (null elves)) "an owned observation needs its ELFs and a stock one takes none"
  functions <- concat <$> mapM (ownedFunctions units) elves
  need (not otherOwned || not (null functions)) "the owned ELFs place no function in the extents"
  stockEvents <- loadEvents units stockDir
  otherEvents <- loadEvents units otherDir
  stockSteps <- arrayOf <$> loadJson (stockDir </> "steps.json")
  otherSteps <- arrayOf <$> loadJson (otherDir </> "steps.json")
  let points name receipt steps = ("attached", field ("attached_" <> name) receipt) : [(show (i :: Int), field name s) | (i, s) <- zip [1 ..] steps]
  memoryRows <- forM (zip (points "memory" stockReceipt stockSteps) (points "memory" otherReceipt otherSteps)) $ \((name, a), (_, b)) -> do
    need (a /= Null && b /= Null) ("point " ++ name ++ " has no memory digest")
    one <- loadJson (stockDir </> ("memory-" ++ name ++ ".json"))
    two <- loadJson (otherDir </> ("memory-" ++ name ++ ".json"))
    let differing = differingChunks one two
    pure (a == b && null differing && field "masked" one == field "masked" two, object ["point" .= name, "stock" .= a, "other" .= b, "differing_chunks" .= differing])
  let extents = concatMap unitExtents units
      watches receipt steps = [field "watch" s | s <- arrayOf (field "reset_segments" receipt)] ++ [field "watch" s | s <- steps]
      writesOf receipt steps = concatMap (arrayOf . field "writes") (watches receipt steps)
      fetchesOf receipt steps = M.unionsWith (+) [M.fromList [(a, n) | (k, Number c) <- KM.toList fs, Just a <- [hexValue (String (K.toText k))], let n = truncate c :: Integer] | w <- watches receipt steps, Object fs <- [field "fetches_by_pc" w]]
      byUnit fetches = object [K.fromString (unitName u) .= sum [n | (a, n) <- M.toList fetches, any (`inside` a) (unitExtents u)] | u <- units]
      events kind' list = [eventValue e | e <- list, eventInterface e == kind']
      residency receipt steps = [map (\r -> object ["overlay" .= field "overlay" r, "resident" .= field "resident" r]) (arrayOf v) | (_, v) <- points "residency" receipt steps]
      residentEverywhere receipt steps = and [field "resident" r == Bool True | (_, v) <- points "residency" receipt steps, r <- arrayOf v] && not (null steps)
      screens receipt steps = field "sha256" (field "attached_screen" receipt) : [field "sha256" (field "screen" s) | s <- steps]
      violations receipt steps = [v | w <- watches receipt steps, let v = field "violation" w, v /= Null]
      card steps =
        [ object ["accepted_writes" .= field "accepted_writes" media, "accepted_write_bytes" .= field "accepted_write_bytes" media, "dirty_sectors" .= field "dirty_sectors" media]
        | s <- steps, idled <- take 1 (reverse (arrayOf (field "idle" s))), let media = field "media" idled ]
          ++ [field "history" (field "media" idled) | s <- take 1 (reverse steps), idled <- take 1 (reverse (arrayOf (field "idle" s)))]
      reach list = object [K.fromString name .= length [() | e <- list, eventName e == name, eventKind e == "entry"] | name <- nub [eventName e | e <- list]]
      stockFetches = fetchesOf stockReceipt stockSteps
      otherFetches = fetchesOf otherReceipt otherSteps
      stray = if otherOwned then M.filterWithKey (\a _ -> not (any (`inside` a) functions)) otherFetches else M.empty
      sharedInputs receipt = case field "inputs" receipt of
        Object fields -> Object (KM.delete "update" (KM.delete "plain" fields))
        other -> other
      memoryEqual = all fst memoryRows && length (points "memory" stockReceipt stockSteps) == length (points "memory" otherReceipt otherSteps)
      behaviorEqual =
        events True stockEvents == events True otherEvents
          && events False stockEvents == events False otherEvents
          && writesOf stockReceipt stockSteps == writesOf otherReceipt otherSteps
          && residency stockReceipt stockSteps == residency otherReceipt otherSteps
          && memoryEqual
          && screens stockReceipt stockSteps == screens otherReceipt otherSteps
          && card stockSteps == card otherSteps
          && null (violations stockReceipt stockSteps) && null (violations otherReceipt otherSteps)
          && sharedInputs stockReceipt == sharedInputs otherReceipt
      equal = behaviorEqual && M.null stray
      summary receipt steps list fetches =
        object
          [ "role" .= field "role" receipt, "inputs" .= field "inputs" receipt, "hits" .= field "hit_count" receipt
          , "interface_events" .= length (events True list), "internal_events" .= length (events False list)
          , "entries_reached" .= reach list
          , "extent_fetches" .= sum (M.elems fetches), "extent_fetches_by_unit" .= byUnit fetches
          , "watched_writes" .= length (writesOf receipt steps)
          , "resident_at_every_point" .= residentEverywhere receipt steps
          , "residency" .= (field "attached_residency" receipt : [field "residency" s | s <- steps])
          ]
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "units" .= field "units" stockReceipt
    , "extents" .= map spanValue extents
    , "owned_functions" .= length functions
    , "stock" .= summary stockReceipt stockSteps stockEvents stockFetches
    , "other" .= summary otherReceipt otherSteps otherEvents otherFetches
    , "owned_fetches_outside_owned_functions" .= object [K.fromString (hexWord a) .= n | (a, n) <- M.toList stray]
    , "owned_stray_fetches" .= sum (M.elems stray)
    , "interface_sequence" .= firstDifferences 20 (events True stockEvents) (events True otherEvents)
    , "internal_sequence" .= firstDifferences 20 (events False stockEvents) (events False otherEvents)
    , "watched_writes" .= firstDifferences 20 (writesOf stockReceipt stockSteps) (writesOf otherReceipt otherSteps)
    , "residency" .= firstDifferences 5 (map A.toJSON (residency stockReceipt stockSteps)) (map A.toJSON (residency otherReceipt otherSteps))
    , "memory" .= object ["equal" .= memoryEqual, "points" .= map snd memoryRows]
    , "screens" .= firstDifferences 20 (screens stockReceipt stockSteps) (screens otherReceipt otherSteps)
    , "card" .= object ["stock" .= card stockSteps, "other" .= card otherSteps, "equal" .= (card stockSteps == card otherSteps)]
    , "watch_violations" .= object ["stock" .= violations stockReceipt stockSteps, "other" .= violations otherReceipt otherSteps]
    , "shared_inputs_equal" .= (sharedInputs stockReceipt == sharedInputs otherReceipt)
    , "behavior_equal" .= behaviorEqual
    , "verdict" .= (if equal then "equal" else "different" :: T.Text)
    ]
  putStrLn ("verdict " ++ (if equal then "equal" else "different"))
  unless equal exitFailure
