{-# LANGUAGE OverloadedStrings #-}
-- | The property-service natural pair's observer: one RAM-push bridge session per role from a
-- zero-instruction reset through the fixed route set, recording getter, setter and area-3 entries,
-- their returns, settings writes and the owned roles' fetch guards.
module Main (main) where

import Control.Exception (Exception, SomeException, displayException, fromException, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.IORef
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.PropertyPairPlan
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import qualified Emulation.WatchObservation as W
import EmulatorUsb
import FujiModPaths (fujimodRamPushBridge)
import Numeric (readHex, showHex)
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (..), hFlush, hSetBuffering, BufferMode (..), stdout, withBinaryFile)
import Text.Printf (printf)

data Options = Options
  { optRole :: Role
  , optRoutes :: FilePath
  , optUpdate :: FilePath
  , optAnalysis :: FilePath
  , optLinked :: FilePath
  , optSymbols :: FilePath
  , optContract :: FilePath
  , optControl :: Maybe FilePath
  , optRun :: FilePath
  , optBootRom :: FilePath
  , optNand :: FilePath
  , optOutput :: FilePath
  }

usage :: String
usage = "usage: property-pair-observe --role retained|owned|control --routes ROUTES --update DAT --analysis ROOT \
  \--linked BIN --symbols SYMBOLS --contract CONTRACT [--control MANIFEST] --run RUN --boot-rom ROM \
  \--adjustment-nand NAND --output NEW_DIRECTORY"

parseOptions :: [String] -> Either String Options
parseOptions arguments = do
  pairs <- collect arguments
  let value name = lookup name pairs
      need name = maybe (Left (usage ++ "\nmissing " ++ name)) Right (value name)
  unless (all ((`elem` names) . fst) pairs && length pairs == length (M.fromList pairs)) (Left usage)
  role <- need "--role" >>= parseRole
  unless ((role == ControlRole) == (value "--control" /= Nothing)) (Left "--control names the control role's manifest")
  Options role <$> need "--routes" <*> need "--update" <*> need "--analysis" <*> need "--linked" <*> need "--symbols"
    <*> need "--contract" <*> pure (value "--control") <*> need "--run" <*> need "--boot-rom" <*> need "--adjustment-nand" <*> need "--output"
 where
  names = ["--role", "--routes", "--update", "--analysis", "--linked", "--symbols", "--contract", "--control", "--run", "--boot-rom", "--adjustment-nand", "--output"]
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

regionValues :: Emulator -> IO Value
regionValues emulator = fmap toJSONList . forM settingsRanges $ \range -> do
  bytes <- emulatorReadMemory emulator (rangeStart range) (fromIntegral (rangeEnd range - rangeStart range))
  pure (object ["start" .= rangeStart range, "bytes" .= C.unpack (hex bytes)])
 where
  toJSONList = Array . V.fromList
  hex = B.concatMap (\byte -> C.pack (pad (showHex byte "")))
  pad text = replicate (2 - length text) '0' ++ text

field :: T.Text -> Value -> Maybe Value
field name (Object fields) = KM.lookup (K.fromText name) fields
field _ _ = Nothing

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
  (routesPath, routesBytes) <- readBound (optRoutes options)
  (datPath, datBytes) <- readBound (optUpdate options)
  (contractPath, contractBytes) <- readBound (optContract options)
  (linkedPath, linked) <- readBound (optLinked options)
  (symbolsPath, symbols) <- readBound (optSymbols options)
  (runPath, runBytes) <- readBound (optRun options)
  (romPath, rom) <- readBound (optBootRom options)
  (nandPath, nand) <- readBound (optNand options)
  analysis <- makeAbsolute (optAnalysis options)
  images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
  observer <- getExecutablePath
  observerDigest <- sha256File observer
  bridgeDigest <- sha256File fujimodRamPushBridge
  routes <- either (refuse . InputRefused . ("route file: " ++)) pure (decodeRouteFile routesBytes)
  contract <- either (refuse . InputRefused . ("contract: " ++)) pure (decodeContract contractBytes)
  control <- forM (optControl options) $ \path -> do
    (manifestPath, manifest) <- readBound path
    parsed <- either (refuse . InputRefused . ("control manifest: " ++)) pure (decodeControlManifest manifest)
    pure (manifestPath, manifest, parsed)
  let role = optRole options
      inputs = PlanInputs role contract images linked (fmap (\(_, _, (word, _, _)) -> word) control)
      bound path bytes = object ["path" .= path, "sha256" .= digestOf bytes]
  plan <- either (refuse . InputRefused) pure (derivePlan inputs)
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
  record "inputs" $ object
    [ "dat" .= bound datPath datBytes, "routes" .= bound routesPath routesBytes, "contract" .= bound contractPath contractBytes
    , "linked" .= bound linkedPath linked, "symbols" .= bound symbolsPath symbols, "run" .= bound runPath runBytes
    , "boot_rom" .= bound romPath rom, "adjustment_nand" .= bound nandPath nand
    , "analysis" .= object [K.fromText (imageNameWire name) .= digestOf bytes | (name, bytes) <- M.toList images]
    , "role_image4_sha256" .= digestOf image4
    , "control" .= fmap (\(path, bytes, _) -> bound path bytes) control
    , "bridge" .= object ["path" .= fujimodRamPushBridge, "sha256" .= sha256Hex bridgeDigest]
    , "observer" .= object ["path" .= observer, "sha256" .= sha256Hex observerDigest]
    ]
  record "bounds" (object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit])
  record "route_exclusions" (A.toJSON (routeExclusions routes))
  let spec = specValue plan
      specPath = output </> "spec.json"
  BL.writeFile specPath (A.encode spec <> "\n")
  record "sites" (object [K.fromString (siteLabelWire label) .= length [() | l <- M.elems (planSites plan), l == label] | label <- siteLabels])
  let arguments =
        [ "--run", runPath, "--boot-rom", romPath, "--installed-dat", datPath, "--adjustment-nand", nandPath
        , "--output", output, "--ram-push-checkpoints", output </> "unused-checkpoints"
        , "--fresh-boot", "--reset-observation", specPath ]
  limit <- either (refuse . InputRefused) pure (W.watchLimit watchLimit)
  ranges <- either (refuse . InputRefused) pure
    (mapM (\range -> W.watchRange (W.watchAddress (rangeStart range)) (W.watchAddress (rangeEnd range))) settingsRanges)
  routeKeyPaths <- pure [routeKeys route | route <- routeList routes, not (null (routeKeys route))]
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
                before <- regionValues emulator
                hitsStart <- readIORef cursor
                armed <- emulatorExecutionWatchBegin emulator ranges [] limit
                unless (field "armed" armed == Just (Bool True)) (refuse (BridgeFailed ("watch-begin refused: " ++ show armed)))
                press <- emulatorDisplayKey emulator key
                case field "failure" press of
                  Just Null -> pure ()
                  _ -> do
                    _ <- try (drainHits emulator) :: IO (Either SomeException Int)
                    refuse (PressFailed (map crawlKeyWire keys) press)
                watch <- emulatorWatchEnd emulator
                after <- regionValues emulator
                hitsEnd <- drainHits emulator
                modifyIORef' steps (object
                  [ "step" .= stepId, "parent" .= parentStep, "keys" .= map crawlKeyWire keys
                  , "hits" .= [hitsStart, hitsEnd], "press" .= press, "watch" .= watch
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

-- | The control builder's manifest: the restored setter table word and the control's digests.
decodeControlManifest :: B.ByteString -> Either String (ControlWord, String, String)
decodeControlManifest bytes = do
  value <- A.eitherDecodeStrict' bytes
  let text name = case field name value of
        Just (String content) -> Right (T.unpack content)
        _ -> Left ("control manifest lacks " ++ T.unpack name)
      word name = text name >>= \content -> case content of
        '0' : 'x' : digits | [(parsed, "")] <- readHex digits, parsed <= (0xffffffff :: Integer) -> Right (fromIntegral parsed)
        _ -> Left ("control manifest " ++ T.unpack name ++ " is not a hexadecimal word")
  unless (field "schema" value == Just (Number 1)) (Left "control manifest schema differs")
  control <- ControlWord <$> word "property" <*> word "address" <*> word "target"
  (,,) control <$> text "dat_sha256" <*> text "image4_sha256"
