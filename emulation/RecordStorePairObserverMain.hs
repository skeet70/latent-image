{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.IORef
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Maybe (isJust)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word64)
import Emulation.PropertyPairPlan (PhysicalRange (..), recordSettingsRanges, preKeyIdle)
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.WatchObservation (Tick (..))
import EmulatorUsb
import Numeric (readHex, showHex)
import System.Directory (createDirectory, doesPathExist, getFileSize, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), IOMode (..), hFlush, hSetBuffering, stdout, withBinaryFile)

data Role = Stock | Owned deriving (Eq)
data PowerOff = PowerOff { offBudget :: Word64, offTick :: Tick }
data Options = Options { role :: Role, inputs :: M.Map String FilePath, output :: FilePath, powerOff :: Maybe PowerOff }
data Trie = Trie [(CrawlKey, Trie)]

insertPath :: [CrawlKey] -> Trie -> Trie
insertPath [] trie = trie
insertPath (key : rest) (Trie children) = Trie $ case lookup key children of
  Just _ -> [(k, if k == key then insertPath rest child else child) | (k, child) <- children]
  Nothing -> children ++ [(key, insertPath rest (Trie []))]

need :: Bool -> String -> IO ()
need condition message = unless condition (ioError (userError message))

field :: T.Text -> Value -> Value
field name (Object fields) = maybe Null id (KM.lookup (K.fromText name) fields)
field _ _ = Null

hexWord :: Word32 -> String
hexWord word = "0x" ++ showHex word ""
hex :: B.ByteString -> String
hex = concatMap (\byte -> let text = showHex byte "" in replicate (2 - length text) '0' ++ text) . B.unpack

parseOptions :: [String] -> IO Options
parseOptions args = do
  pairs <- collect args
  let values = M.fromList pairs
      required = ["routes", "update", "contract", "linked", "symbols", "run", "boot-rom", "adjustment-nand", "bridge"]
      controls = ["role", "output", "power-off-budget", "power-off-tick"]
  need (length pairs == M.size values && all (`M.member` values) ("role" : "output" : required)
    && all (`elem` (controls ++ "record-nand" : required)) (M.keys values)) "invalid record-store-pair-observe options"
  selected <- case values M.! "role" of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> ioError (userError "--role must be stock or owned")
  let bounded name low high = case M.lookup name values of
        Nothing -> pure Nothing
        Just text -> case reads text of
          [(value,"")] | value >= low && value <= (high::Integer) -> pure (Just (fromInteger value))
          _ -> ioError (userError ("--" ++ name ++ " must be " ++ show low ++ ".." ++ show high))
  budget <- bounded "power-off-budget" 1 4000000000
  tick <- bounded "power-off-tick" 1 (toInteger (maxBound :: Word64))
  cycle' <- case (budget,tick) of
    (Nothing,Nothing) -> pure Nothing
    (Just instructions,Just at) -> pure (Just (PowerOff instructions (Tick at)))
    _ -> ioError (userError "--power-off-budget and --power-off-tick go together")
  pure (Options selected (M.withoutKeys values (S.fromList controls)) (values M.! "output") cycle')
 where
  collect [] = pure []
  collect (('-':'-':key) : value : rest) = ((key, value) :) <$> collect rest
  collect _ = ioError (userError "options require --name value")

entries :: [Word32]
entries = [0x6ac2b4,0x6ac328,0x6ac478,0x6ac4c8,0x6ac7d4,0x6ac984,0x6acab8,0x6acda0,0x6ae0fc]

rangesValue :: [(Word32, Word32)] -> Value
rangesValue ranges = A.toJSON [[start,end] | (start,end) <- ranges]

retiredData :: [(Word32, Word32)]
retiredData = [(0x89f8f0,0x89f8f8),(0x917fa8,0x917fe4)]

cacheContract :: Value
cacheContract = object
  ["buffers" .= ([[0x8a0080,0x8c0080],[0x8c0080,0x8e0080]]::[[Word32]])
  ,"tags" .= object ["address" .= (0x8e0080::Word32),"bytes" .= (8::Int),"initial_values" .= ([0xffffffff,0xffffffff]::[Word32])]
  ,"timestamp" .= object ["address" .= (0x8e0088::Word32),"bytes" .= (4::Int),"initial_value" .= (0::Word32)]
  ,"ages" .= object ["address" .= (0x89fa60::Word32),"bytes" .= (8::Int),"initial_values" .= ([0,0]::[Word32])]
  ,"initialized_by" .= (0x6ac2b4::Word32)]

mergeRanges :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeRanges = foldr merge [] . sortOn fst
 where
  merge range [] = [range]
  merge (start,end) ((next,last') : rest)
    | end >= next = merge (start,max end last') rest
    | otherwise = (start,end) : (next,last') : rest

symbolExtents :: B.ByteString -> IO ([(Word32, Word32)], M.Map String Word32)
symbolExtents bytes = do
  rows <- forM (C.lines bytes) $ \line -> case words (C.unpack line) of
    [address,size,kind,name] | [(start,"")] <- readHex address, [(count,"")] <- readHex size -> pure (start,count,kind,name)
    _ -> ioError (userError "invalid carrier symbol row")
  let extents = mergeRanges [(start,start+size) | (start,size,kind,_) <- rows, kind `elem` ["t","T"], size > 0, start >= 0x6ac2b4, start+size <= 0x6ae114]
  need (all (\entry -> any (\(start,end) -> start <= entry && entry+4 <= end) extents) entries) "carrier symbols omit an entry"
  need (length rows == M.size (M.fromList [(name,start) | (start,_,_,name) <- rows])) "repeated carrier symbol"
  need (all (\(start,size,_,name) -> not (any (\(low,high) -> start < high && toInteger start + max 1 (toInteger size) > toInteger low) ([(0x89fa60,0x89fa68),(0x8a0080,0x8e008c)] :: [(Word32,Word32)]))
    || name `elem` ["record_store_block","record_store_second_block","record_store_cache_tags","record_store_cache_timestamp","record_store_cache_ages"]) rows) "undeclared cache ownership alias"
  pure (extents,M.fromList [(name,start) | (start,_,_,name) <- rows])

complementSpan :: [(Word32, Word32)] -> [(Word32, Word32)]
complementSpan ranges = go 0x6ac2b4 ranges
 where
  go start [] = [(start,0x6ae114) | start < 0x6ae114]
  go start ((next,end) : rest) = [(start,next) | start < next] ++ go end rest

readSpec :: Int -> Int -> Bool -> Value
readSpec reg count indirect = object ["base" .= object ["register" .= reg], "count" .= count, "indirect" .= indirect]

captureSpec :: Value
captureSpec = object [K.fromString (hexWord site) .= capture site | site <- entries ++ [0x6fb7c8,0x6fb57c]]
 where
  capture site
    | site == 0x6fb7c8 = object ["entry" .= [readSpec 0 16 False, object
        ["base" .= object ["register-word" .= object ["register" .= (0::Int), "offset" .= (12::Int)]], "count" .= (0::Int)
        ,"count_base" .= object ["register-word" .= object ["register" .= (0::Int), "offset" .= (8::Int)]]]]]
    | site == 0x6fb57c = object ["entry" .= [readSpec 0 8 False]]
    | otherwise = object ["entry" .= entryReads site, "returns" .= returnReads site]
  entryReads site
    | site == 0x6ac328 = [readSpec 13 20 False,object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | site == 0x6ac7d4 = [readSpec 2 4096 False]
    | site == 0x6ac4c8 = [readSpec 2 4 False,readSpec 1 2 False]
    | site == 0x6acab8 = [readSpec 2 4 False,readSpec 1 131072 False]
    | site == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []
  returnReads site
    | site == 0x6ac328 = [object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | site == 0x6ac4c8 = [readSpec 2 4 False,object ["base" .= object ["register" .= (2::Int)],"count" .= (4096::Int),"indirect" .= True,"on_success" .= True],readSpec 1 2 False]
    | site == 0x6acab8 = [readSpec 1 131072 False,readSpec 2 4 False]
    | site == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []

settings :: Emulator -> IO Value
settings emulator = A.toJSON <$> forM recordSettingsRanges (\range -> do
  bytes <- emulatorReadMemory emulator (rangeStart range) (fromIntegral (rangeEnd range - rangeStart range))
  pure (object ["start" .= rangeStart range,"bytes" .= hex bytes]))

medium :: Emulator -> IO Value
medium emulator = A.toJSON <$> forM ([0x52..0x59] ++ [0x68,0x69] :: [Word32]) (\logical -> do
  bytes <- emulatorReadMemory emulator (0x95daf0 + 2*logical) 2
  let mapped = fromIntegral (B.index bytes 0) .|. (fromIntegral (B.index bytes 1) `shiftL` 8) :: Word32
  if mapped == 0xffff then pure (object ["logical" .= logical,"physical" .= Null,"data" .= replicate (2*131072) 'f',"spare" .= Null])
  else do
    let physical = 0x380 + mapped
    readback <- emulatorBridgeCommand emulator (object ["command" .= ("nand-read"::T.Text),"first" .= (physical*64),"count" .= (64::Int)])
    pure (object ["logical" .= logical,"physical" .= physical,"data" .= field "data" readback,"spare" .= field "spare" readback]))

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  options <- getArgs >>= parseOptions
  exists <- doesPathExist (output options)
  need (not exists) "--output must be new"
  directory <- makeAbsolute (output options)
  createDirectory directory
  receipt <- newIORef KM.empty
  let record key value = modifyIORef' receipt (KM.insert key value)
      save status = do
        let cycleFiles = if isJust (powerOff options) then ["power-off.json","reboot-hits.jsonl","reboot/reset-observation.json","reboot/emulator-session.json"] else []
        hashes <- forM (["spec.json","owned-extents.json","reset-observation.json","hits.jsonl","steps.json","medium.json","emulator-session.json"] ++ cycleFiles) $ \name -> do
          present <- doesPathExist (directory </> name)
          digest <- if present then Just . sha256Hex <$> sha256File (directory </> name) else pure Nothing
          pure (K.fromString name .= digest)
        fields <- readIORef receipt
        BL.writeFile (directory </> "receipt.json") (A.encode (Object (KM.insert "outcome" status (KM.insert "files" (object hashes) fields))) <> "\n")
  outcome <- try (observe options directory record)
  case outcome of
    Right () -> save (object ["status" .= ("completed"::T.Text)])
    Left failure -> do
      save (object ["status" .= ("refused"::T.Text),"error" .= displayException (failure::SomeException)])
      putStrLn (displayException failure)
      exitFailure

observe :: Options -> FilePath -> (K.Key -> Value -> IO ()) -> IO ()
observe options directory record = do
  paths <- mapM makeAbsolute (inputs options)
  bindings <- forM (M.toList paths) $ \(name,path) -> do
    digest <- sha256File path
    pure (K.fromString name .= object ["path" .= path,"sha256" .= sha256Hex digest])
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  record "schema" (Number (if isJust (powerOff options) then 3 else 2))
  forM_ (powerOff options) $ \terms -> do
    record "power_off_budget" (A.toJSON (offBudget terms))
    record "power_off_tick" (A.toJSON (tickValue (offTick terms)))
  record "role" (String (if role options == Owned then "owned" else "stock"))
  record "inputs" (object (bindings ++ ["observer" .= object ["path" .= observer,"sha256" .= sha256Hex observerDigest]]))
  symbols <- B.readFile (paths M.! "symbols")
  (owned,syms) <- symbolExtents symbols
  let symbol name = maybe (ioError (userError ("missing symbol " ++ name))) pure (M.lookup name syms)
  forM_ [("record_store_initialized",0x89f8b0),("record_store_wrappers",0x89f8b4),("record_store_state",0x89f8f8)
    ,("record_store_block",0x8a0080),("record_store_second_block",0x8c0080),("record_store_cache_tags",0x8e0080)
    ,("record_store_cache_timestamp",0x8e0088),("record_store_cache_ages",0x89fa60)
    ,("record_store_staging_0",0x8e16a0),("record_store_staging_1",0x8e00a0),("record_store_staging_2",0x8e1700)
    ,("record_store_staging_3",0x89fa80),("record_store_staging_4",0x8e10a0)] $ \(name,address) ->
      symbol name >>= \actual -> need (actual == address) ("carrier placement differs: " ++ name)
  need (M.notMember "record_store_cache_tag" syms && all (\address -> address < 0x89f8f0 || address >= 0x89f8f8) (M.elems syms)) "obsolete cache symbol or gap alias remains"
  let ownedRam = [(0x89f8f8,0x89fa60),(0x89fa60,0x89fa68),(0x8a0080,0x8e0080),(0x8e0080,0x8e008c)]
      ownedValue = object ["code" .= rangesValue owned,"ram" .= rangesValue ownedRam
        ,"unwatched_owned_ram" .= rangesValue ownedRam,"activation_fetch" .= ([0x6ac2b4,0x6ac2b8]::[Word32])
        ,"rule" .= ("Owned-role stock-only RAM accesses fail after the first open-veneer fetch; initial image publication is admitted. Unoccupied code fetches fail. Owned state, cache buffers and metadata are not watched; the oracle, returned records and NAND effects cover them."::T.Text)]
  BL.writeFile (directory </> "owned-extents.json") (A.encode ownedValue <> "\n")
  contract <- B.readFile (paths M.! "contract") >>= either (ioError . userError) pure . A.eitherDecodeStrict'
  need (field "owned_cache" contract == cacheContract) "owned two-slot cache contract differs"
  datDigest <- sha256Hex <$> sha256File (paths M.! "update")
  linkedDigest <- sha256Hex <$> sha256File (paths M.! "linked")
  symbolsDigest <- sha256Hex <$> sha256File (paths M.! "symbols")
  need (field "linked_sha256" contract == A.toJSON linkedDigest && field "symbols_sha256" contract == A.toJSON symbolsDigest) "carrier inputs do not match contract"
  need (field (if role options == Owned then "candidate_sha256" else "source_sha256") contract == A.toJSON datDigest) "DAT does not match role contract"
  routesBytes <- B.readFile (paths M.! "routes")
  routes <- either (ioError . userError) pure (decodeRouteFile routesBytes)
  let rawCandidates = case field "raw_candidates" (field "reference_scan" contract) of Array values -> V.toList values; _ -> []
      residualSites = [(address,"residual") | candidate <- rawCandidates, Number n <- [field "site" candidate], let address = truncate n]
      siteMap = M.fromList (residualSites ++ [(address,"record-entry") | address <- entries] ++ [(0x6fb7c8,"nand-write"),(0x6fb57c,"nand-erase")])
      sites = object [K.fromString (hexWord address) .= (label::T.Text) | (address,label) <- M.toList siteMap]
      dataRanges = retiredData
      writes = [(rangeStart range,rangeEnd range) | range <- recordSettingsRanges] ++ dataRanges
      fetches = if role options == Owned then mergeRanges ((0x6ac2b4,0x6ac2b8) : complementSpan owned) else [(0x6ac2b4,0x6ae114)]
      spec = object ["schema" .= (1::Int),"sites" .= sites,"hit_limit" .= (1500000::Int),"captures" .= captureSpec
        ,"write_ranges" .= rangesValue writes,"read_ranges" .= rangesValue dataRanges,"fetch_ranges" .= rangesValue fetches,"watch_limit" .= (1048576::Int)]
      specPath = directory </> "spec.json"
      arguments = ["--run",paths M.! "run","--boot-rom",paths M.! "boot-rom","--installed-dat",paths M.! "update","--adjustment-nand",paths M.! "adjustment-nand"
        ,"--output",directory,"--ram-push-checkpoints",directory </> "unused-checkpoints","--fresh-boot","--reset-observation",specPath]
        ++ maybe [] (\path -> ["--record-nand",path]) (M.lookup "record-nand" paths)
      watch = object ["command" .= ("watch-begin"::T.Text),"views" .= ("fixed"::T.Text),"limit" .= (1048576::Int)
        ,"ranges" .= rangesValue (mergeRanges (writes ++ dataRanges ++ fetches)),"write_ranges" .= rangesValue writes,"read_ranges" .= rangesValue dataRanges,"fetch_ranges" .= rangesValue fetches]
  BL.writeFile specPath (A.encode spec <> "\n")
  record "raw_residual_count" (A.toJSON (length rawCandidates))
  record "ram_rule" ownedValue
  record "output_rule" (String "Compare outputs written by the stock contract across roles; require each contract-untouched output to equal its own entry preimage, without comparing residue across roles.")
  record "endpoint_contract" (String "finite-post-settle-prefix-v2")
  record "post_settle_prefix_ticks" (A.toJSON (tickValue (Tick 833334)))
  steps <- newIORef []
  cursor <- newIORef 0
  counter <- newIORef (0::Int)
  let keyRoutes = [routeKeys route | route <- routeList routes, not (null (routeKeys route))]
      exportPath = directory </> "nand-power-off.nand.gz"
      drainInto file position emulator = do
        start <- readIORef position
        reply <- emulatorTraceHitsPage emulator start 65536
        values <- case field "hits" reply of Array values -> pure (V.toList values); _ -> ioError (userError "trace-hits lacks hits")
        forM_ values (\value -> BL.hPut file (A.encode value <> "\n"))
        let end = start + length values
        writeIORef position end
        hFlush file
        case field "count" reply of Number total | end < truncate total -> drainInto file position emulator; _ -> pure end
      stop (emulator,_,()) = stopEmulator emulator
  when (isJust (powerOff options)) $ need (length keyRoutes == 1) "a power cycle follows exactly one key route"
  withBinaryFile (directory </> "hits.jsonl") WriteMode $ \hits -> do
    let drain = drainInto hits cursor
    bracket (startEmulator (paths M.! "bridge") directory arguments (\_ -> pure (Nothing,()))) stop $ \(emulator,start,()) -> do
      need (bootStart start == FreshBootStart) "bridge did not fresh boot"
      record "emulator_start" (startRecord start)
      _ <- drain emulator
      rootMedium <- medium emulator
      record "medium_at_attach" rootMedium
      rootSettings <- settings emulator
      record "settings_at_attach" rootSettings
      let walk path parent (Trie children) = do
            let branching = length children > 1
                name = SnapshotName ("n-" ++ show parent)
            when branching (emulatorSnapshotSave emulator name)
            forM_ (zip [0::Int ..] children) $ \(index,(key,child@(Trie descendants))) -> do
              when (index > 0) $ do
                _ <- emulatorSnapshotRestore emulator name
                when (index == length children - 1) (emulatorSnapshotDrop emulator name)
              step <- atomicModifyIORef' counter (\n -> (n+1,n+1))
              let keys = path ++ [key]
              before <- settings emulator
              first <- readIORef cursor
              _ <- emulatorBridgeCommand emulator watch
              idle <- emulatorIdle emulator preKeyIdle
              press <- emulatorDisplayKey emulator key
              need (field "failure" press == Null) ("key failed: " ++ show press)
              need (field "status" (field "settle" press) `elem` [String "packet-consumed",String "budget-exhausted"])
                ("key injection was not established at " ++ unwords (map crawlKeyWire keys) ++ ": " ++ show press)
              elapsed <- emulatorIdleTicks emulator (Tick 833334)
              need (field "ticks" elapsed == A.toJSON (tickValue (Tick 833334))) "post-settle prefix missed its exact tick boundary"
              state <- emulatorUiState emulator
              need (field "instructions" elapsed == field "instructions" state) "UI snapshot advanced beyond the prefix boundary"
              let endpoint = object ["elapsed" .= elapsed,"state" .= state]
              observed <- emulatorWatchEnd emulator
              last' <- drain emulator
              after <- settings emulator
              blocks <- if null descendants then medium emulator else pure Null
              modifyIORef' steps (object ["step" .= step,"parent" .= parent,"keys" .= map crawlKeyWire keys,"hits" .= [first,last']
                ,"idle" .= idle,"press" .= press,"bounded_post_settle_prefix" .= endpoint,"watch" .= observed,"regions_before" .= before,"regions_after" .= after,"medium_after" .= blocks] :)
              BL.writeFile (directory </> "steps.json") . A.encode . reverse =<< readIORef steps
              putStrLn ("step " ++ show step ++ " " ++ unwords (map crawlKeyWire keys))
              walk keys step child
      walk [] 0 (foldl (flip insertPath) (Trie []) keyRoutes)
      final <- medium emulator
      BL.writeFile (directory </> "medium.json") (A.encode final <> "\n")
      forM_ (powerOff options) $ \(PowerOff budget target) -> do
        first <- readIORef cursor
        before <- settings emulator
        _ <- emulatorBridgeCommand emulator watch
        let align = do
              state <- emulatorUiState emulator
              now <- case field "tick" state of Number n -> pure (truncate n :: Integer); _ -> ioError (userError "ui-state lacks its tick")
              let remaining = toInteger (tickValue target) - now
              need (remaining >= 0) "power-off tick precedes the final step"
              if remaining == 0 then pure state else do
                let chunk = min remaining 833334
                elapsed <- emulatorIdleTicks emulator (Tick (fromInteger chunk))
                need (field "ticks" elapsed == A.toJSON chunk) "power-off alignment missed its tick boundary"
                align
        aligned <- align
        off <- emulatorPowerOff emulator budget
        observed <- emulatorWatchEnd emulator
        last' <- drain emulator
        after <- settings emulator
        blocks <- medium emulator
        label <- either (ioError . userError) pure (nandExportLabel "power-off")
        exported <- emulatorNandExport emulator label
        let artifact = field "artifact" exported
        need (field "status" exported == String "exported" && field "boundary" exported == field "after" exported) "power-off NAND export refused or advanced the machine"
        need (field "path" artifact == String (T.pack exportPath)) "power-off NAND export path differs"
        digest <- sha256Hex <$> sha256File exportPath
        extent <- getFileSize exportPath
        need (field "sha256" artifact == String (T.pack digest) && field "bytes" artifact == A.toJSON extent) "power-off NAND export digest or extent differs"
        BL.writeFile (directory </> "power-off.json") (A.encode (object
          ["aligned" .= aligned,"power_off" .= off,"hits" .= [first,last'],"watch" .= observed,"regions_before" .= before,"regions_after" .= after
          ,"medium_after" .= blocks,"export" .= exported]) <> "\n")
        putStrLn ("power off " ++ show first ++ ".." ++ show last')
  forM_ (powerOff options) $ \_ -> do
    let rebootDirectory = directory </> "reboot"
        rebootArguments = ["--run",paths M.! "run","--boot-rom",paths M.! "boot-rom","--installed-dat",paths M.! "update","--committed-nand",exportPath
          ,"--output",rebootDirectory,"--ram-push-checkpoints",rebootDirectory </> "unused-checkpoints","--fresh-boot","--reset-observation",specPath]
    createDirectory rebootDirectory
    committed <- sha256Hex <$> sha256File exportPath
    rebootCursor <- newIORef 0
    withBinaryFile (directory </> "reboot-hits.jsonl") WriteMode $ \rebootHits ->
      bracket (startEmulator (paths M.! "bridge") rebootDirectory rebootArguments (\_ -> pure (Nothing,()))) stop $ \(emulator,start,()) -> do
        need (bootStart start == FreshBootStart) "reboot bridge did not fresh boot"
        let supplied = field "inputs" (startRecord start)
        need (field "committed_nand" supplied == String (T.pack committed) && field "source_kind" supplied == String "full-committed"
          && field "adjustment_nand" supplied == Null && field "record_nand" supplied == Null) "reboot did not boot only the exported medium"
        _ <- drainInto rebootHits rebootCursor emulator
        blocks <- medium emulator
        regions <- settings emulator
        record "reboot" (object ["emulator_start" .= startRecord start,"input_committed_sha256" .= committed,"medium_at_attach" .= blocks,"settings_at_attach" .= regions])
        putStrLn "reboot attached"
