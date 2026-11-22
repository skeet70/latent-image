{-# LANGUAGE OverloadedStrings #-}

-- | dcf-numbering-observe observe --role stock|owned --update DAT --image4 ID4 [--linked BIN]
--     --run JSON --boot-rom ROM --adjustment-nand NAND [--card IMG] [--route KEY,...]
--     --bridge BRIDGE --output NEW_DIR
--   dcf-numbering-observe compare STOCK_DIR OTHER_DIR OWNED_SYMBOLS OUTPUT_JSON
--
-- observe: fresh-boots the RAM-push bridge with trace sites at the ten DCF numbering entries and
-- at every branch from the numbering code to a retained service (the stock extents of ID4 for
-- stock, the linked image BIN for owned), a write watch over the profile header and the media
-- context pointer, and a fetch watch over the five extents. After attach and after each route
-- key it reads the media context's numbering fields and the profile header.
--
-- compare: entries whose return address lies outside the extents are the interface. The ordered
-- interface entries (arguments), their returns (r0 of the entries that return a value), every
-- service call (arguments and the bytes they name), the watched writes, the numbering state and
-- profile header after attach and each key, the screens and the card's accepted writes, dirty
-- sectors and final command history must be equal. An owned observation must fetch nothing in
-- the extents outside the owned functions of OWNED_SYMBOLS.
module Main (main) where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM, forM_, replicateM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.IORef
import Data.List (isPrefixOf, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
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

data Inputs = Inputs
  { inputRole :: Role
  , inputPaths :: M.Map String FilePath
  , inputRoute :: [CrawlKey]
  }

-- | A half-open guest address range.
data Span = Span Word32 Word32

extents :: [Span]
extents =
  [ Span 0x70f790 0x70f8b8, Span 0x70faa0 0x7101e4, Span 0x71141c 0x7115a4
  , Span 0x7159f8 0x715a58, Span 0x716184 0x71657c ]

inside :: Span -> Word32 -> Bool
inside (Span s e) a = s <= a && a < e

inExtents :: Word32 -> Bool
inExtents a = any (`inside` a) extents

-- | An entry: address, name, register arguments and whether it returns a value.
data Entry = Entry {entryAddress :: Word32, entryName :: String, entryArity :: Int, entryReturns :: Bool}

entries :: [Entry]
entries =
  [ Entry 0x70f790 "set_prefix_mode" 1 False, Entry 0x70f840 "check_name" 2 True
  , Entry 0x70faa0 "apply_continuous_numbering" 1 False, Entry 0x70fb68 "read_numbers" 2 True
  , Entry 0x70fbfc "prepare_name" 3 True, Entry 0x71141c "note_written" 1 False
  , Entry 0x7159f8 "read_profile" 4 False, Entry 0x715a2c "write_profile" 4 False
  , Entry 0x716184 "create_folder" 2 True, Entry 0x716324 "last_used" 4 True ]

entryAt :: M.Map Word32 Entry
entryAt = M.fromList [(entryAddress e, e) | e <- entries]

-- | How a service call's captured bytes are judged.
data Extent = Fixed | CountIn Int | Terminated deriving (Eq)

-- | A retained service: name, register arguments and the bytes captured at the call, each from a
-- register with a byte count and how much of it is defined.
data Service = Service {serviceName :: String, serviceArity :: Int, serviceReads :: [(Int, Int, Extent)]}

services :: M.Map Word32 Service
services =
  M.fromList
    [ (0x71ddf0, Service "media_status" 1 []), (0x71f708, Service "current_drive" 0 [])
    , (0x715f0c, Service "group_index" 1 []), (0x726c74, Service "report_event" 2 [])
    , (0x712d08, Service "drive_prefix" 1 []), (0x70e6c4, Service "copy_bytes" 3 [(1, 64, CountIn 2)])
    , (0x70eaa0, Service "format_decimal" 4 []), (0x69f8fc, Service "configuration_byte" 1 [])
    , (0x6a8040, Service "profile_directory" 2 []), (0x6a801c, Service "profile_file" 2 [])
    , (0x6a802c, Service "store_profile_directory" 4 []), (0x6a8008, Service "store_profile_file" 3 [])
    , (0x6a8050, Service "reset_profile" 0 []), (0x712b4c, Service "refresh_snapshot" 0 [])
    , (0x43550, Service "fill_bytes" 3 []), (0x722218, Service "format_number" 4 [])
    , (0x722054, Service "copy_memory" 3 [(1, 64, CountIn 2)]), (0x701e18, Service "open_directory" 3 [(0, 64, Terminated)])
    , (0x713460, Service "entry_cluster" 1 [(0, 32, Fixed)]), (0x7160b8, Service "path_groups" 2 [(1, 32, Terminated)])
    , (0x71d450, Service "translate_status" 2 [])
    ]

-- | Stock image-4 data the stock entries pass by address, where the owned image passes its own
-- copy: the folder template and `\DCIM\`.
stockConstants :: [Span]
stockConstants = [Span 0x774d44 0x774d58, Span 0x781f60 0x781f68]

-- | Bytes above sp treated as the calling frame.
frameWindow :: Word32
frameWindow = 0x4000

image4Base :: Word32
image4Base = 0x61c400

contextSlot :: Word32
contextSlot = 0x910204

-- | The profile header and its byte count.
profileHeader :: Span
profileHeader = Span 0x9c281c 0x9c29b6

-- | The media context's numbering fields, relative to the context.
contextSpans :: [(T.Text, Word32, Int)]
contextSpans =
  [ ("files", 0x0, 0x200), ("catalog", 0x2160, 0x1460), ("sequence", 0x127d0, 0x8), ("numbering", 0x16bd0, 0xa0) ]

-- | The A32 B/BL sites in BYTES loaded at BASE, inside the extents, whose target is a service.
callSites :: Word32 -> B.ByteString -> [(Word32, Word32)]
callSites base bytes =
  [ (a, target)
  | Span s e <- extents
  , a <- [max s base, max s base + 4 .. e - 4]
  , let offset = fromIntegral (a - base)
  , offset + 4 <= B.length bytes
  , let w = foldr (\i acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes (offset + i))) 0 [0 .. 3] :: Word32
  , w `shiftR` 28 /= 0xf && (w `shiftR` 25) .&. 7 == 5
  , let imm = w .&. 0xffffff
        displacement = (if imm .&. 0x800000 /= 0 then imm .|. 0xff000000 else imm) `shiftL` 2
        target = a + 8 + displacement
  , M.member target services
  ]

settleChunks, finalChunks, chunkInstructions, hitLimit, watchLimit :: Int
settleChunks = 2
finalChunks = 2
chunkInstructions = 20000000
hitLimit = 4000000
watchLimit = 1048576

hexWord :: Word32 -> String
hexWord w = "0x" ++ showHex w ""

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

hexText :: B.ByteString -> T.Text
hexText = T.pack . concatMap (\b -> (if b < 16 then "0" else "") ++ showHex b "") . B.unpack

-- * Observation

parseInputs :: [String] -> IO Inputs
parseInputs arguments = do
  pairs <- collect arguments
  let values = M.fromList pairs
      required = ["role", "update", "image4", "run", "boot-rom", "adjustment-nand", "bridge", "output"]
  need (length pairs == M.size values && M.keys (foldr M.delete values ["card", "route", "linked"]) == sort required) "observe requires each of --role --update --image4 --run --boot-rom --adjustment-nand --bridge --output once, and at most one --card, --route and --linked"
  role <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  need ((role == Owned) == M.member "linked" values) "--linked is required for the owned role and refused for stock"
  keys <- maybe (pure [KeyPlay, KeyLeft, KeyLeft, KeyRight, KeyRight]) (mapM crawlKey . splitOn) (M.lookup "route" values)
  pure (Inputs role (M.delete "role" (M.delete "route" values)) keys)
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

readSpec :: Int -> Int -> Value
readSpec r count = object ["base" .= object ["register" .= r], "count" .= count]

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

-- | The context pointer, the numbering fields of the context it names and the profile header.
numberingState :: Emulator -> IO Value
numberingState emulator = do
  pointer <- emulatorReadMemory emulator contextSlot 4
  let context = B.foldr (\b acc -> acc `shiftL` 8 .|. fromIntegral b) 0 pointer :: Word32
      Span ps pe = profileHeader
  spans <-
    if context == 0
      then pure []
      else forM contextSpans $ \(name, offset, count) -> (K.fromText name .=) . hexText <$> emulatorReadMemory emulator (context + offset) count
  profile <- emulatorReadMemory emulator ps (fromIntegral (pe - ps))
  pure (object ["context" .= hexWord context, "fields" .= object spans, "profile" .= hexText profile])

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  case arguments of
    ("observe" : rest) -> parseInputs rest >>= observeMain
    ["compare", stock, other, symbols, output] -> compareMain stock other symbols output
    _ -> ioError (userError "usage: dcf-numbering-observe observe OPTIONS | compare STOCK_DIR OTHER_DIR OWNED_SYMBOLS OUTPUT_JSON")

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
  bindings <- forM (M.toList paths) $ \(name, path) -> do
    digest <- sha256File path
    pure (K.fromString name .= object ["path" .= path, "sha256" .= sha256Hex digest])
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  record "schema" (Number 1)
  record "role" (String (roleName (inputRole inputs)))
  record "inputs" (object (bindings ++ ["observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]]))
  calls <- case M.lookup "linked" paths of
    Nothing -> callSites image4Base <$> B.readFile (paths M.! "image4")
    Just linked -> callSites 0x70f790 <$> B.readFile linked
  need (not (null calls)) "no service call sites in the numbering code"
  record "call_sites" (A.toJSON [object ["site" .= hexWord a, "service" .= serviceName (services M.! t)] | (a, t) <- calls])
  let stateReads = A.toJSON [[contextSlot, 4], let Span s e = profileHeader in [s, e - s]]
      entryCapture e = object ["entry" .= [readSpec 0 32 | entryName e == "check_name"], "returns" .= ([] :: [Value])]
      callCapture service = object ["entry" .= [readSpec r count | (r, count, _) <- serviceReads service], "returns" .= ([] :: [Value])]
      sites =
        [(entryAddress e, "dcf:" ++ entryName e, entryCapture e) | e <- entries]
          ++ [(a, "call:" ++ serviceName (services M.! t), callCapture (services M.! t)) | (a, t) <- calls]
      writeSpans = [profileHeader, Span contextSlot (contextSlot + 4)]
      spec =
        object
          [ "schema" .= (1 :: Int)
          , "sites" .= object [K.fromString (hexWord a) .= label | (a, label, _) <- sites]
          , "reads" .= object [K.fromString (hexWord a) .= stateReads | (a, _, _) <- sites]
          , "captures" .= object [K.fromString (hexWord a) .= capture | (a, _, capture) <- sites]
          , "task" .= ("logical" :: T.Text)
          , "hit_limit" .= hitLimit
          , "write_ranges" .= map spanValue writeSpans
          , "fetch_ranges" .= map spanValue extents
          , "watch_limit" .= watchLimit
          ]
      specPath = directory </> "spec.json"
      watch =
        object
          [ "command" .= ("watch-begin" :: T.Text), "views" .= ("fixed" :: T.Text), "limit" .= watchLimit
          , "ranges" .= map spanValue (writeSpans ++ extents)
          , "write_ranges" .= map spanValue writeSpans, "fetch_ranges" .= map spanValue extents ]
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
      record "attached_state" =<< numberingState emulator
      record "attached_screen" =<< emulatorCapture emulator "attached"
      let step index key = do
            first <- readIORef cursor
            _ <- emulatorBridgeCommand emulator watch
            press <- maybe (pure Null) (\k -> emulatorBridgeCommand emulator (object ["command" .= ("key" :: T.Text), "key" .= crawlKeyWire k])) key
            settled <- idle emulator (maybe finalChunks (const settleChunks) key)
            observed <- emulatorWatchEnd emulator
            last' <- drain emulator
            state <- numberingState emulator
            screen <- emulatorCapture emulator (maybe "final" (const ("step-" ++ show index)) key)
            modifyIORef' steps (object
              [ "step" .= index, "key" .= fmap crawlKeyWire key, "hits" .= [first, last'], "press" .= press, "idle" .= settled
              , "watch" .= watchSummary observed, "state" .= state, "screen" .= screen ] :)
            saveSteps
            putStrLn ("step " ++ show index ++ " " ++ maybe "final" crawlKeyWire key ++ " hits " ++ show (last' - first))
            pure last'
      forM_ (zip [1 :: Int ..] (inputRoute inputs)) $ \(index, key) -> step index (Just key)
      total <- step (length (inputRoute inputs) + 1) Nothing
      record "hit_count" (A.toJSON total)

-- * Comparison

-- | An event, its comparable value, and whether it belongs to the interface (`True`) or is an
-- internal entry (`False`).
data Event = Event {eventName :: String, eventKind :: String, eventInterface :: Bool, eventValue :: Value}

hexBytes :: T.Text -> [Word8]
hexBytes text = go (T.unpack text)
  where
    go (a : b : rest) | [(v, "")] <- readHex [a, b] = fromIntegral (v :: Int) : go rest
    go _ = []

maskedHex :: [Bool] -> [Word8] -> T.Text
maskedHex flags bytes = T.pack (concat (zipWith byte flags bytes))
  where
    byte True b = (if b < 16 then "0" else "") ++ showHex b ""
    byte False _ = "--"

registerOf :: Value -> Int -> Maybe Word32
registerOf hit r = hexValue (field (T.pack ('r' : show r)) (field "registers" hit))

-- | A service argument: its value, or `stack` inside the caller's frame, or `constant` for an
-- address of the numbering code or the stock data it passes.
argumentValue :: Value -> Int -> Value
argumentValue hit r = case (registerOf hit r, hexValue (field "sp" hit)) of
  (Just v, Just sp)
    | v >= sp && v - sp < frameWindow -> String "stack"
    | inExtents v || any (`inside` v) stockConstants -> String "constant"
    | otherwise -> String (T.pack (hexWord v))
  (Just v, Nothing) -> String (T.pack (hexWord v))
  _ -> Null

capturedBytes :: Value -> [T.Text]
capturedBytes hit = [t | r <- arrayOf (field "reads" (field "observation" hit)), String t <- [field "bytes" r]]

serviceBytes :: Service -> Value -> Value
serviceBytes service hit = A.toJSON (zipWith judge (serviceReads service) (capturedBytes hit))
  where
    judge (_, _, extent) text =
      let bytes = hexBytes text
          flags = case extent of
            Fixed -> map (const True) bytes
            CountIn r -> let n = maybe 0 fromIntegral (registerOf hit r) :: Int in map (< n) [0 .. length bytes - 1]
            Terminated -> scanl (\open b -> open && b /= 0) True (take (length bytes - 1) bytes)
       in maskedHex flags bytes

common :: Value -> [(K.Key, Value)]
common hit = ["core" .= field "core" hit, "task" .= object ["kernel" .= field "kernel" task, "task_id" .= field "task_id" task, "kind" .= field "kind" task], "state" .= field "memory" hit]
  where task = field "task" hit

event :: Value -> Maybe Event
event hit = do
  label <- textOf (field "site" hit)
  address <- hexValue (field "address" hit)
  case label of
    _ | "dcf:" `isPrefixOf` label, Just entry <- M.lookup address entryAt, Just lr <- hexValue (field "lr" hit) ->
          let name = if entryName entry == "check_name" then [maskedHex (map (< maybe 0 fromIntegral (registerOf hit 1)) [0 :: Int ..]) (hexBytes t) | t <- capturedBytes hit] else []
           in Just (Event (entryName entry) "entry" (not (inExtents lr)) (object (["kind" .= ("entry" :: T.Text), "name" .= entryName entry, "lr" .= hexWord lr, "arguments" .= [A.toJSON (fmap hexWord (registerOf hit r)) | r <- [0 .. entryArity entry - 1]], "reads" .= name] ++ common hit)))
      | "call:" `isPrefixOf` label, Just service <- M.lookup (callTarget label) servicesByName ->
          Just (Event (serviceName service) "service" True (object (["kind" .= ("service" :: T.Text), "name" .= serviceName service, "arguments" .= [argumentValue hit r | r <- [0 .. serviceArity service - 1]], "reads" .= serviceBytes service hit] ++ common hit)))
      | label == "call-return" -> do
          site <- hexValue (field "site" (field "observation" hit))
          entry <- M.lookup site entryAt
          Just (Event (entryName entry) "return" (not (inExtents address)) (object (["kind" .= ("return" :: T.Text), "name" .= entryName entry, "return_address" .= hexWord address, "result" .= [A.toJSON (fmap hexWord (registerOf hit 0)) | entryReturns entry]] ++ common hit)))
      | otherwise -> Nothing
  where
    callTarget = drop 5
    servicesByName = M.fromList [(serviceName s, s) | s <- M.elems services]

loadEvents :: FilePath -> IO [Event]
loadEvents directory = do
  bytes <- BL.readFile (directory </> "hits.jsonl")
  forM (filter (not . BL.null) (BLC.lines bytes)) (either (ioError . userError) pure . A.eitherDecode) >>= pure . mapMaybe event

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

-- | The owned functions of a symbols file: every defined symbol with a size.
ownedFunctions :: FilePath -> IO [Span]
ownedFunctions path = do
  rows <- map words . lines <$> readFile path
  let functions = [Span a (a + s) | [address, size, kind, _] <- rows, kind `elem` ["T", "t"], [(a, "")] <- [readHex address], [(s, "")] <- [readHex size], s > 0]
  need (not (null functions)) (path ++ " names no owned function")
  pure functions

compareMain :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
compareMain stockDir otherDir symbols output = do
  exists <- doesPathExist output
  need (not exists) "comparison output must be new"
  functions <- ownedFunctions symbols
  let load directory = do
        events <- loadEvents directory
        receipt <- loadJson (directory </> "receipt.json")
        steps <- arrayOf <$> loadJson (directory </> "steps.json")
        pure (events, receipt, steps)
  (stockEvents, stockReceipt, stockSteps) <- load stockDir
  (otherEvents, otherReceipt, otherSteps) <- load otherDir
  forM_ [stockReceipt, otherReceipt] $ \r -> need (field "status" (field "outcome" r) == String "completed") "an observation did not complete"
  need (field "role" stockReceipt == String (roleName Stock)) "the first observation must have the stock role"
  let watches receipt steps = [field "watch" s | s <- arrayOf (field "reset_segments" receipt)] ++ [field "watch" s | s <- steps]
      writesOf receipt steps = concatMap (arrayOf . field "writes") (watches receipt steps)
      fetchesOf receipt steps = M.unionsWith (+) [M.fromList [(a, n) | (k, Number c) <- KM.toList fs, Just a <- [hexValue (String (K.toText k))], let n = truncate c :: Integer] | w <- watches receipt steps, Object fs <- [field "fetches_by_pc" w]]
      byExtent fetches = [object ["extent" .= spanValue e, "fetches" .= sum [n | (a, n) <- M.toList fetches, inside e a]] | e <- extents]
      interface events = [eventValue e | e <- events, eventInterface e]
      states receipt steps = field "attached_state" receipt : [field "state" s | s <- steps]
      screens receipt steps = field "sha256" (field "attached_screen" receipt) : [field "sha256" (field "screen" s) | s <- steps]
      violations receipt steps = [v | w <- watches receipt steps, let v = field "violation" w, v /= Null]
      card steps =
        [ object ["accepted_writes" .= field "accepted_writes" media, "accepted_write_bytes" .= field "accepted_write_bytes" media, "dirty_sectors" .= field "dirty_sectors" media]
        | s <- steps, idled <- take 1 (reverse (arrayOf (field "idle" s))), let media = field "media" idled ]
          ++ [field "history" (field "media" idled) | s <- take 1 (reverse steps), idled <- take 1 (reverse (arrayOf (field "idle" s)))]
      reach events = M.fromListWith (\(a, b) (c, d) -> (a + c, b + d)) [(eventName e, if eventInterface e then (1 :: Int, 0 :: Int) else (0, 1)) | e <- events, eventKind e == "entry"]
      reachValue events = object [K.fromString (entryName e) .= object ["interface" .= i, "internal" .= n] | e <- entries, let (i, n) = M.findWithDefault (0, 0) (entryName e) (reach events)]
      serviceCounts events = M.fromListWith (+) [(eventName e, 1 :: Int) | e <- events, eventKind e == "service"]
      stockFetches = fetchesOf stockReceipt stockSteps
      otherFetches = fetchesOf otherReceipt otherSteps
      otherOwned = field "role" otherReceipt == String (roleName Owned)
      stray = if otherOwned then M.filterWithKey (\a _ -> not (any (`inside` a) functions)) otherFetches else M.empty
      sharedInputs receipt = case field "inputs" receipt of
        Object fields -> Object (KM.delete "update" (KM.delete "linked" fields))
        other -> other
      behaviorEqual =
        interface stockEvents == interface otherEvents
          && writesOf stockReceipt stockSteps == writesOf otherReceipt otherSteps
          && states stockReceipt stockSteps == states otherReceipt otherSteps
          && screens stockReceipt stockSteps == screens otherReceipt otherSteps
          && card stockSteps == card otherSteps
          && null (violations stockReceipt stockSteps) && null (violations otherReceipt otherSteps)
          && sharedInputs stockReceipt == sharedInputs otherReceipt
      equal = behaviorEqual && M.null stray
      summary receipt events fetches =
        object
          [ "role" .= field "role" receipt, "inputs" .= field "inputs" receipt, "hits" .= field "hit_count" receipt
          , "interface_events" .= length (interface events), "service_calls" .= serviceCounts events
          , "extent_fetches" .= sum (M.elems fetches), "extent_fetches_by_extent" .= byExtent fetches
          , "entries" .= reachValue events ]
  BL.writeFile output $ A.encode $ object
    [ "schema" .= (1 :: Int)
    , "stock" .= summary stockReceipt stockEvents stockFetches
    , "other" .= summary otherReceipt otherEvents otherFetches
    , "owned_fetches_outside_owned_functions" .= object [K.fromString (hexWord a) .= n | (a, n) <- M.toList stray]
    , "interface_sequence" .= firstDifferences 20 (interface stockEvents) (interface otherEvents)
    , "watched_writes" .= firstDifferences 20 (writesOf stockReceipt stockSteps) (writesOf otherReceipt otherSteps)
    , "numbering_state" .= firstDifferences 5 (states stockReceipt stockSteps) (states otherReceipt otherSteps)
    , "screens" .= firstDifferences 20 (screens stockReceipt stockSteps) (screens otherReceipt otherSteps)
    , "card" .= object ["stock" .= card stockSteps, "other" .= card otherSteps, "equal" .= (card stockSteps == card otherSteps)]
    , "watch_violations" .= object ["stock" .= violations stockReceipt stockSteps, "other" .= violations otherReceipt otherSteps]
    , "shared_inputs_equal" .= (sharedInputs stockReceipt == sharedInputs otherReceipt)
    , "behavior_equal" .= behaviorEqual
    , "verdict" .= (if equal then "equal" else "different" :: T.Text)
    ]
  putStrLn ("verdict " ++ (if equal then "equal" else "different"))
  unless equal exitFailure
