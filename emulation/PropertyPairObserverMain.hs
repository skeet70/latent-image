{-# LANGUAGE OverloadedStrings #-}
-- | The property-service natural pair's observer: one RAM-push bridge session per role from a
-- zero-instruction reset through the fixed route set, recording getter, setter and area-3 entries,
-- their returns, settings writes and the owned roles' fetch guards.
module Main (main) where

import Control.Exception (Exception, SomeException, displayException, fromException, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import qualified CustomAdmission as StorageBoundaries
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.PropertyPairPlan
import Emulation.PropertyPairRoutes
import qualified Emulation.SettingsStorageMatrix as Storage
import Emulation.SettingsStorageObserver
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import qualified Emulation.WatchObservation as W
import EmulatorUsb
import FujiModPaths (fujimodRamPushBridge)
import qualified Replacement.PropertyService.Placement as Placement
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (..), hFlush, hSetBuffering, BufferMode (..), stdout, withBinaryFile)
import Text.Printf (printf)
data RouteInput = KeyRouteFile FilePath | StorageMatrixFile FilePath
data RouteDocument = KeyRouteDocument RouteFile | StorageMatrixDocument Storage.StorageMatrix


data Options = Options
  { optRole :: Role
  , optRouteInput :: RouteInput
  , optUpdate :: FilePath
  , optAnalysis :: FilePath
  , optLinked :: FilePath
  , optSymbols :: FilePath
  , optElf :: Maybe FilePath
  , optContract :: FilePath
  , optControl :: Maybe FilePath
  , optRun :: FilePath
  , optBootRom :: FilePath
  , optNand :: ObserverNand
  , optOutput :: FilePath
  , optProbe :: Maybe Probe
  , optProbeDrain :: Maybe Tick
  }

-- | A discriminating run: only this key path from the root, with the guest idle for the given
-- instructions before its last key.
data Probe = Probe [CrawlKey] (Maybe Instructions)

usage :: String
usage = "usage: property-pair-observe --role retained|owned|control (--routes ROUTES | --storage-matrix MATRIX) --update DAT --analysis ROOT \
  \--linked BIN --symbols SYMBOLS [--elf ELF (required for schema 3)] --contract CONTRACT [--control MANIFEST] --run RUN --boot-rom ROM \
  \(--adjustment-nand NAND | --committed-nand NAND) --output NEW_DIRECTORY [--probe-path KEY,KEY,... [--probe-idle INSTRUCTIONS] [--probe-drain-ticks TICKS]]"

parseOptions :: [String] -> Either String Options
parseOptions arguments = do
  pairs <- collect arguments
  let value name = lookup name pairs
      need name = maybe (Left (usage ++ "\nmissing " ++ name)) Right (value name)
  unless (all ((`elem` names) . fst) pairs && length pairs == length (M.fromList pairs)) (Left usage)
  routeInput <- case (value "--routes",value "--storage-matrix") of
    (Just path,Nothing) -> Right (KeyRouteFile path)
    (Nothing,Just path) -> Right (StorageMatrixFile path)
    _ -> Left "name exactly one of --routes and --storage-matrix"
  nand <- case (value "--adjustment-nand",value "--committed-nand") of
    (Just path,Nothing) -> Right (AdjustmentSeed path)
    (Nothing,Just path) -> Right (CommittedSeed path)
    _ -> Left "name exactly one of --adjustment-nand and --committed-nand"
  case (routeInput,nand) of
    (KeyRouteFile _,CommittedSeed _) -> Left "--committed-nand requires --storage-matrix"
    _ -> Right ()
  role <- need "--role" >>= parseRole
  unless ((role == ControlRole) == (value "--control" /= Nothing)) (Left "--control names the control role's manifest")
  probe <- case (value "--probe-path", value "--probe-idle") of
    (Nothing, Nothing) -> Right Nothing
    (Nothing, Just _) -> Left "--probe-idle needs --probe-path"
    (Just path, idle) -> do
      keys <- mapM parseKey (splitOn path)
      when (null keys) (Left "--probe-path names at least one key")
      instructions <- forM idle $ \text -> case reads text of
        [(count, "")] | count >= 1 && count <= instructionsValue maxIdle -> Right (Instructions count)
        _ -> Left ("--probe-idle must be 1.." ++ show (instructionsValue maxIdle))
      Right (Just (Probe keys instructions))
  diagnostic <- forM (value "--probe-drain-ticks") $ \text -> case reads text of
    [(ticks, "")] | ticks > 0 && ticks <= tickValue postSettle && value "--probe-path" /= Nothing && value "--probe-idle" == Nothing -> Right (Tick ticks)
    _ -> Left "--probe-drain-ticks needs --probe-path, no --probe-idle, and a positive cap no larger than postSettle"
  case routeInput of
    StorageMatrixFile _ -> unless (case probe of Nothing -> True; Just _ -> False) (Left "storage matrices do not accept diagnostic probes")
    KeyRouteFile _ -> Right ()
  Options role routeInput <$> need "--update" <*> need "--analysis" <*> need "--linked" <*> need "--symbols" <*> pure (value "--elf")
    <*> need "--contract" <*> pure (value "--control") <*> need "--run" <*> need "--boot-rom" <*> pure nand <*> need "--output"
    <*> pure probe <*> pure diagnostic
 where
  names = ["--role", "--routes", "--storage-matrix", "--update", "--analysis", "--linked", "--symbols", "--elf", "--contract", "--control", "--run", "--boot-rom", "--adjustment-nand", "--committed-nand", "--output", "--probe-path", "--probe-idle", "--probe-drain-ticks"]
  parseKey wire = maybe (Left ("unknown key " ++ wire)) Right (lookup wire [(crawlKeyWire key, key) | key <- [minBound .. maxBound]])
  splitOn text = case break (== ',') text of
    (key, []) -> [key]
    (key, _ : rest) -> key : splitOn rest
  collect (name : value : rest) = ((name, value) :) <$> collect rest
  collect [] = Right []
  collect _ = Left usage

-- | Why the observation stopped before completing every route.
data Refusal
  = GuardFetch Value
  | PressFailed [String] Value
  | InputRefused String
  | BridgeFailed String
  deriving (Show)

instance Exception Refusal

refusalValue :: Refusal -> Value
refusalValue refusal = case refusal of
  GuardFetch hit -> object ["reason" .= ("guarded-fetch" :: T.Text), "hit" .= hit]
  PressFailed path response -> object ["reason" .= ("press-failed" :: T.Text), "keys" .= path, "response" .= response]
  InputRefused message -> object ["reason" .= ("input-refused" :: T.Text), "message" .= message]
  BridgeFailed message -> object ["reason" .= ("bridge-failed" :: T.Text), "message" .= message]

refuse :: Refusal -> IO a
refuse = throwIO

-- | A trie of key routes in first-seen order; every node except the root is one step.
data Trie = Trie [(CrawlKey, Trie)]

insertPath :: [CrawlKey] -> Trie -> Trie
insertPath [] trie = trie
insertPath (key : rest) (Trie children) = Trie $ case lookup key children of
  Just child -> [(k, if k == key then insertPath rest child else c) | (k, c) <- children]
  Nothing -> children ++ [(key, insertPath rest (Trie []))]

-- | Hits requested per `trace-hits` reply.
hitPage :: Int
hitPage = 65536


field :: T.Text -> Value -> Maybe Value
field name (Object fields) = KM.lookup (K.fromText name) fields
field _ _ = Nothing

drainDiagnostics :: Emulator -> IO Value
drainDiagnostics emulator = do
  mode <- emulatorReadMemory emulator 0x9500ec 0x28
  cells <- forM
    [(0x949dd0, 8), (0x94f5d8, 8), (0x950098, 4), (0x950148, 4), (0x951424, 4), (0x951528, 4),
     (22955789, 1), (22956064, 4), (9807516, 4), (9807520, 4), (9807528, 4), (9807532, 4), (9765112, 4), (9765136, 1),
     (22955728, 4), (21902010, 4), (21902136, 4), (22955752, 4), (0x9571d0, 12), (0x1467588, 0x400),
     (0xd3d53c, 4), (0x944580, 0x50), (0x1708e3c, 0x18), (0x170952c, 0x70), (0x1715a5c, 0x18)] $ \(address, size) ->
      raw address <$> emulatorReadMemory emulator address size
  keys <- forM [14 .. 20] $ \row -> do
    let address = 8552736 + row * 28 + 21
    raw address <$> emulatorReadMemory emulator address 1
  tables <- forM [1074087440, 1074087572] $ \record -> do
    header <- emulatorReadMemory emulator (record - 0x40000000) 8
    let table = word32At header 0
        count = word32At header 4
        physical address = if address >= 0x40000000 then address - 0x40000000 else address
    rows <- if count < 1 || count > 128 then pure [] else forM [0 .. count - 1] $ \index -> do
      let address = physical table + index * 0x38
      bytes <- emulatorReadMemory emulator address 0x38
      descriptor <- emulatorReadMemory emulator (physical (word32At bytes 8)) 0x28
      stack <- if word32At descriptor 8 == 0xc22f60 && word32At descriptor 16 <= 65536
        then Just . raw (word32At descriptor 20) <$> emulatorReadMemory emulator (physical (word32At descriptor 20)) (fromIntegral (word32At descriptor 16))
        else pure Nothing
      pure (object ["row" .= raw address bytes, "descriptor" .= raw (word32At bytes 8) descriptor, "task1_stack" .= stack])
    pure (object ["record" .= raw record header, "rows" .= rows])
  pure (object
    [ "outcome" .= (if word32At mode 12 /= 0 || B.index mode 36 /= 0 then "source-mode-pending" else "diagnostic-only" :: String)
    , "source_mode" .= object ["current" .= word32At mode 0, "previous" .= word32At mode 4,
        "pending" .= word32At mode 12, "request" .= word32At mode 20, "transition_latch" .= B.index mode 36]
    , "source_mode_raw" .= raw 0x9500ec mode, "cells" .= cells, "keys" .= keys, "task_tables" .= tables ])
 where
  raw :: Word32 -> B.ByteString -> Value
  raw address bytes = object ["address" .= address, "bytes" .= B.unpack bytes]

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  options <- getArgs >>= either (\message -> putStrLn message >> exitFailure) pure . parseOptions
  exists <- doesPathExist (optOutput options)
  when exists (putStrLn "property-pair-observe: --output must be new" >> exitFailure)
  output <- makeAbsolute (optOutput options)
  createDirectory output
  outcome <- try (observe options output)
  case outcome of
    Right () -> putStrLn ("property-pair-observe: completed " ++ output)
    Left failure -> do
      putStrLn ("property-pair-observe: refused: " ++ displayException (failure :: SomeException))
      exitFailure

readBound :: FilePath -> IO (FilePath, B.ByteString)
readBound path = do
  absolute <- makeAbsolute path
  bytes <- B.readFile absolute
  pure (absolute, bytes)

digestOf :: B.ByteString -> String
digestOf = sha256Hex . sha256

observe :: Options -> FilePath -> IO ()
observe options output = do
  receipt <- newIORef (KM.fromList [] :: KM.KeyMap Value)
  let record key value = modifyIORef' receipt (KM.insert key value)
      writeReceipt status = do
        files <- forM ["spec.json", "reset-observation.json", "hits.jsonl", "steps.json", "emulator-session.json"] $ \name -> do
          present <- doesPathExist (output </> name)
          digest <- if present then Just . sha256Hex <$> sha256File (output </> name) else pure Nothing
          pure (K.fromString name .= digest)
        fields <- readIORef receipt
        BL.writeFile (output </> "receipt.json") (A.encode (Object (KM.insert "files" (object files) (KM.insert "outcome" status fields))) <> "\n")
  result <- try (run options output record)
  case result of
    Right () -> writeReceipt (object ["status" .= ("completed" :: T.Text)])
    Left failure -> do
      let refusal = maybe (BridgeFailed (displayException failure)) id (fromException failure)
      writeReceipt (object ["status" .= ("refused" :: T.Text), "refusal" .= refusalValue refusal])
      throwIO failure

run :: Options -> FilePath -> (K.Key -> Value -> IO ()) -> IO ()
run options output record = do
  (routesPath, routesBytes) <- readBound (case optRouteInput options of KeyRouteFile path -> path; StorageMatrixFile path -> path)
  (datPath, datBytes) <- readBound (optUpdate options)
  (contractPath, contractBytes) <- readBound (optContract options)
  (linkedPath, linked) <- readBound (optLinked options)
  (symbolsPath, symbols) <- readBound (optSymbols options)
  elf <- forM (optElf options) readBound
  (runPath, runBytes) <- readBound (optRun options)
  (romPath, rom) <- readBound (optBootRom options)
  (nandPath, nand) <- readBound (case optNand options of AdjustmentSeed path -> path; CommittedSeed path -> path)
  analysis <- makeAbsolute (optAnalysis options)
  images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  bridgeDigest <- sha256File fujimodRamPushBridge
  routes <- case optRouteInput options of
    KeyRouteFile _ -> KeyRouteDocument <$> either (refuse . InputRefused . ("route file: " ++)) pure (decodeRouteFile routesBytes)
    StorageMatrixFile _ -> StorageMatrixDocument <$> either (refuse . InputRefused . ("storage matrix: " ++)) pure (Storage.decodeStorageMatrix routesBytes)
  contract <- either (refuse . InputRefused . ("contract: " ++)) pure (decodeContract contractBytes)
  bankBoundary <- case (contractPublishedElf contract,elf) of
    (Just identity,Just (_,bytes)) -> do
      unless (digestOf bytes == publishedElfSha256 identity) (refuse (InputRefused "ELF differs from the composed publication"))
      publication <- maybe (refuse (InputRefused "ELF has no composed publication")) pure (contractPublication contract)
      boundary <- either (refuse . InputRefused . ("bank apply ELF: " ++)) pure
        (StorageBoundaries.admitBankApplyElf StorageBoundaries.BankApplyElfInput
          { StorageBoundaries.bankApplyImageBase = Placement.GuestAddress image4Base
          , StorageBoundaries.bankApplyCarriers = [Placement.Carrier (Placement.GuestAddress low) (Placement.GuestAddress high) | PhysicalRange low high <- publicationCarriers publication]
          , StorageBoundaries.bankApplyLinkedBytes = linked
          , StorageBoundaries.bankApplySymbolBytes = symbols
          , StorageBoundaries.bankApplyElfBytes = bytes
          })
      pure (Just boundary)
    (Just _,Nothing) -> refuse (InputRefused "composed publication requires --elf")
    (Nothing,Just _) -> refuse (InputRefused "--elf requires a composed publication")
    (Nothing,Nothing) -> pure Nothing
  control <- forM (optControl options) $ \path -> do
    (manifestPath, manifest) <- readBound path
    parsed <- either (refuse . InputRefused . ("control manifest: " ++)) pure (decodeControlManifest manifest)
    pure (manifestPath, manifest, parsed)
  let role = optRole options
      inputs = PlanInputs role contract images linked (fmap (\(_, _, (word, _, _)) -> word) control) bankBoundary
      bound path bytes = object ["path" .= path, "sha256" .= digestOf bytes]
  plan <- either (refuse . InputRefused) pure (case routes of KeyRouteDocument _ -> derivePlan inputs; StorageMatrixDocument matrix -> deriveStoragePlan inputs matrix)
  image4 <- either (refuse . InputRefused) pure (roleImage4 inputs)
  unless (digestOf symbols == contractSymbolsSha256 contract) (refuse (InputRefused "symbols differ from the contract"))
  expectedDat <- case (role, control) of
    (RetainedRole, Nothing) -> pure (contractSourceDat contract)
    (OwnedRole, Nothing) -> pure (contractCandidateDat contract)
    (ControlRole, Just (_, _, (_, datDigest, image4Digest))) -> do
      unless (digestOf image4 == image4Digest) (refuse (InputRefused "control image4 differs from its manifest"))
      pure datDigest
    _ -> refuse (InputRefused "role and control manifest disagree")
  unless (digestOf datBytes == expectedDat) (refuse (InputRefused ("DAT is not the " ++ roleWire role ++ " role's DAT")))
  record "schema" (Number 1)
  record "role" (String (T.pack (roleWire role)))
  record "inputs" $ object (
    [ "dat" .= bound datPath datBytes, "routes" .= bound routesPath routesBytes, "contract" .= bound contractPath contractBytes
    , "linked" .= bound linkedPath linked, "symbols" .= bound symbolsPath symbols, "run" .= bound runPath runBytes
    , "boot_rom" .= bound romPath rom, "adjustment_nand" .= (case optNand options of AdjustmentSeed _ -> bound nandPath nand; CommittedSeed _ -> Null)
    , "analysis" .= object [K.fromText (imageNameWire name) .= digestOf bytes | (name, bytes) <- M.toList images]
    , "role_image4_sha256" .= digestOf image4
    , "control" .= fmap (\(path, bytes, _) -> bound path bytes) control
    , "bridge" .= object ["path" .= fujimodRamPushBridge, "sha256" .= sha256Hex bridgeDigest]
    , "observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]
    ] ++ ["committed_nand" .= (case optNand options of AdjustmentSeed _ -> Null; CommittedSeed _ -> bound nandPath nand) | case routes of StorageMatrixDocument _ -> True; KeyRouteDocument _ -> False]
      ++ ["elf" .= bound path bytes | Just (path,bytes) <- [elf]])
  record "bounds" (object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit])
  record "route_exclusions" (case routes of KeyRouteDocument document -> A.toJSON (routeExclusions document); StorageMatrixDocument _ -> A.toJSON ([]::[Value]))
  let spec = specValue plan
      specPath = output </> "spec.json"
  BL.writeFile specPath (A.encode spec <> "\n")
  record "sites" (object [K.fromString (siteLabelWire label) .= length [() | l <- M.elems (planSites plan), l == label] | label <- siteLabels])
  let seed = case optNand options of AdjustmentSeed _ -> AdjustmentSeed nandPath; CommittedSeed _ -> CommittedSeed nandPath
      boot = StorageObserverInputs fujimodRamPushBridge runPath romPath datPath seed specPath
  case routes of
    KeyRouteDocument document -> runKeyRoutes options output document boot record
    StorageMatrixDocument matrix -> do
      record "execution" (String "settings-storage-lifecycle")
      record "probe" Null
      record "pre_key_idle_instructions" (A.toJSON (instructionsValue preKeyIdle))
      record "post_settle_prefix_ticks" (A.toJSON (tickValue postSettle))
      runStorageMatrix boot output matrix record

runKeyRoutes :: Options -> FilePath -> RouteFile -> StorageObserverInputs -> (K.Key -> Value -> IO ()) -> IO ()
runKeyRoutes options output routes boot record = do
  let runPath = storageRun boot
      romPath = storageRom boot
      datPath = storageDat boot
      specPath = storageSpec boot
      medium = case storageInitialNand boot of AdjustmentSeed path -> ["--adjustment-nand",path]; CommittedSeed path -> ["--committed-nand",path]
  let arguments =
        [ "--run", runPath, "--boot-rom", romPath, "--installed-dat", datPath ]
        ++ medium ++ [ "--output", output, "--ram-push-checkpoints", output </> "unused-checkpoints"
                     , "--fresh-boot", "--reset-observation", specPath ]
  limit <- either (refuse . InputRefused) pure (W.watchLimit watchLimit)
  ranges <- either (refuse . InputRefused) pure
    (mapM (\range -> W.watchRange (W.watchAddress (rangeStart range)) (W.watchAddress (rangeEnd range))) settingsRanges)
  record "probe" (maybe Null (\(Probe path idle) -> object ["path" .= map crawlKeyWire path, "idle_instructions" .= fmap instructionsValue idle]) (optProbe options))
  record "pre_key_idle_instructions" (maybe (A.toJSON (instructionsValue preKeyIdle)) (const Null) (optProbe options))
  record "post_settle_prefix_ticks" (A.toJSON (tickValue postSettle))
  let routeKeyPaths = maybe [routeKeys route | route <- routeList routes, not (null (routeKeys route))] (\(Probe path _) -> [path]) (optProbe options)
  let trie = foldl (flip insertPath) (Trie []) routeKeyPaths
  withBinaryFile (output </> "hits.jsonl") WriteMode $ \hitsFile -> do
    cursor <- newIORef 0
    steps <- newIORef []
    stepCounter <- newIORef (0 :: Int)
    let drainHits = \emulator -> do
          start <- readIORef cursor
          reply <- emulatorTraceHitsPage emulator start hitPage
          hits <- case field "hits" reply of
            Just (Array values) -> pure (V.toList values)
            _ -> refuse (BridgeFailed "trace-hits reply lacks hits")
          recorded <- case field "count" reply of
            Just (Number count) -> pure (truncate count)
            _ -> refuse (BridgeFailed "trace-hits reply lacks count")
          forM_ hits $ \hit -> do
            BL.hPut hitsFile (A.encode hit <> "\n")
            case field "site" hit of
              Just (String label) | Just (Guard _) <- parseSiteLabel (T.unpack label) -> hFlush hitsFile >> refuse (GuardFetch hit)
              _ -> pure ()
          writeIORef cursor (start + length hits)
          hFlush hitsFile
          if start + length hits < recorded && not (null hits) then drainHits emulator else pure (start + length hits)
        finishSteps = BL.writeFile (output </> "steps.json") . (<> "\n") . A.encode . reverse =<< readIORef steps
    sessionResult <- try $ do
      (emulator, start, ()) <- startEmulator fujimodRamPushBridge output arguments (\_ -> pure (Nothing, ()))
      flip finallyStop emulator $ do
        unless (bootStart start == FreshBootStart) (refuse (BridgeFailed "the bridge did not cold-boot"))
        record "emulator_start" (startRecord start)
        _ <- drainHits emulator
        let walk path parentStep (Trie children) = do
              let branching = length children > 1
                  name = SnapshotName ("n-" ++ show parentStep)
              when branching (emulatorSnapshotSave emulator name)
              forM_ (zip [0 :: Int ..] children) $ \(index, (key, child)) -> do
                when (index > 0) $ do
                  _ <- emulatorSnapshotRestore emulator name
                  when (index == length children - 1) (emulatorSnapshotDrop emulator name)
                stepId <- atomicModifyIORef' stepCounter (\n -> (n + 1, n + 1))
                let keys = path ++ [key]
                before <- readSettingsRegions emulator
                hitsStart <- readIORef cursor
                armed <- emulatorExecutionWatchBegin emulator ranges [] limit
                unless (field "armed" armed == Just (Bool True)) (refuse (BridgeFailed ("watch-begin refused: " ++ show armed)))
                idled <- case optProbe options of
                  Nothing -> Just <$> emulatorIdle emulator preKeyIdle
                  Just _ | optProbeDrain options /= Nothing -> Just <$> emulatorIdle emulator preKeyIdle
                  Just (Probe probed (Just instructions)) | keys == probed -> Just <$> emulatorIdle emulator instructions
                  Just _ -> pure Nothing
                press <- emulatorDisplayKey emulator key
                case field "failure" press of
                  Just Null -> pure ()
                  _ -> do
                    _ <- try (drainHits emulator) :: IO (Either SomeException Int)
                    refuse (PressFailed (map crawlKeyWire keys) press)
                postIdle <- case (optProbe options, optProbeDrain options) of
                  (Just (Probe probed _), Just ticks) | keys == probed -> do
                    elapsed <- emulatorIdleTicks emulator ticks
                    diagnostic <- drainDiagnostics emulator
                    hitsEnd <- drainHits emulator
                    BL.writeFile (output </> "local-ui-drain-diagnostic.json") (A.encode (object
                      ["keys" .= map crawlKeyWire keys, "press" .= press, "elapsed" .= elapsed, "state" .= diagnostic,
                       "hits" .= [hitsStart, hitsEnd]]) <> "\n")
                    refuse (BridgeFailed "diagnostic-only bounded UI sample")
                  _ -> observeBoundedPrefix emulator
                watch <- emulatorWatchEnd emulator
                after <- readSettingsRegions emulator
                hitsEnd <- drainHits emulator
                modifyIORef' steps (object
                  [ "step" .= stepId, "parent" .= parentStep, "keys" .= map crawlKeyWire keys
                  , "hits" .= [hitsStart, hitsEnd], "idle" .= idled, "press" .= press, "bounded_post_settle_prefix" .= postIdle, "watch" .= watch
                  , "regions_before" .= before, "regions_after" .= after ] :)
                printf "step %d %s: hits %d..%d\n" stepId (unwords (map crawlKeyWire keys)) hitsStart hitsEnd
                walk keys stepId child
        walk [] 0 trie
    finishSteps
    case sessionResult of
      Right () -> pure ()
      Left failure -> throwIO (failure :: SomeException)
 where
  finallyStop action emulator = do
    outcome <- try action
    stopped <- try (stopEmulator emulator)
    case (outcome, stopped) of
      (Left failure, _) -> throwIO (failure :: SomeException)
      (Right (), Left failure) -> refuse (BridgeFailed ("bridge stop: " ++ displayException (failure :: SomeException)))
      (Right (), Right ()) -> pure ()

