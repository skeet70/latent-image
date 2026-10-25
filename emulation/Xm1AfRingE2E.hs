{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException, finally)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import Data.IORef (atomicModifyIORef', modifyIORef', newIORef)
import Data.List (isPrefixOf)
import qualified Data.Text as T
import Data.Word (Word8, Word32, Word64)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.UiTrajectory (Segment (..), Step (..), TraceSite (..), Trajectories (..), decodeTrajectories)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, resolve, stageViewerRuntime, stagedFirmwarePath)
import qualified Emulation.ViewerRuntime as Runtime
import EmulatorUsb
import FujiAF.AfRing
import qualified FujiAF.Broker as P
import FujiAF.RamPush
import FujiAF.StoredGate (openReplyTail)
import FujiAF.Transport (Clock (..), UsbApi, acquireDevice, descriptors, ensurePrivateDirectory, inputEndpoint, newHeld, newSession, outputEndpoint, releaseHeld, transferTimeoutMs, tx, writePrivateBytes)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), hPutStrLn, hSetBuffering, stderr, stdout)
import System.Process (callProcess)
import Xm1UnreviewedRelink

newtype SyntheticEntry = SyntheticEntry Word32

data RingMetadata = RingMetadata Address Int SyntheticEntry String

instance FromJSON RingMetadata where
  parseJSON = withObject "AF ring metadata" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported AF ring metadata schema")
    address <- fields .: "ring_address" >>= parseAddress
    bytes <- fields .: "ring_bytes"
    entry <- fields .: "synthetic_entry" >>= parseAddress
    digest <- fields .: "runtime_sha256"
    pure (RingMetadata (Address address) bytes (SyntheticEntry entry) digest)
    where
      parseAddress text = case T.unpack text of
        '0' : 'x' : digits | not (null digits), length digits <= 8, [(value, "")] <- readHex digits -> pure value
        _ -> fail "metadata address must be a 32-bit 0x-hex address"

data Options = Options
  { optNand :: Maybe FilePath
  , optCard :: Maybe FilePath
  , optReceipt :: Maybe FilePath
  , optFirmware :: Maybe FilePath
  , optFresh :: Bool
  }

usage :: String
usage = unlines
  [ "xm1-af-ring-e2e --installed-nand NAND --sd-card-image CARD --receipt-output DIR [--firmware DAT] [--fresh-boot]"
  , "Host-only, unreviewed AF ring push, half-press attempt, bulk capture/decode and unload."
  , "NAND must carry stored 0xf7 != 0. Inputs and the new output directory belong beneath private-data/."
  , "Uses an emulator slot and the keyed card-attach checkpoint. No camera or USB I/O."
  , "If the bridge cannot supply ordinary AF, calls the payload's synthetic recorder through ff80."
  ]

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing False)
  where
    go options [] = pure options
    go options ("--installed-nand" : value : rest) = go options { optNand = Just value } rest
    go options ("--sd-card-image" : value : rest) = go options { optCard = Just value } rest
    go options ("--receipt-output" : value : rest) = go options { optReceipt = Just value } rest
    go options ("--firmware" : value : rest) = go options { optFirmware = Just value } rest
    go options ("--fresh-boot" : rest) = go options { optFresh = True } rest
    go _ (argument : _) = ioError (userError ("unknown option or missing value: " ++ argument ++ "\n" ++ usage))

privateInput :: Workspace -> String -> Maybe FilePath -> IO FilePath
privateInput root label supplied = do
  input <- maybe (ioError (userError (label ++ " is required"))) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root input)
  exists <- doesFileExist path
  require (exists && splitDirectories private `isPrefixOf` splitDirectories path) (label ++ " must be a file beneath private-data/")
  pure path

freshReceipt :: Workspace -> Maybe FilePath -> IO FilePath
freshReceipt root supplied = do
  input <- maybe (ioError (userError "--receipt-output is required")) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  let path = resolve root input
  parent <- canonicalizePath (takeDirectory path)
  require (splitDirectories private `isPrefixOf` splitDirectories parent && takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must name a new directory beneath private-data/"
  let destination = parent </> takeFileName path
  exists <- (||) <$> doesDirectoryExist destination <*> doesFileExist destination
  require (not exists) "--receipt-output already exists"
  ensurePrivateDirectory destination
  pure destination

-- Emulated transfers can exceed the live transport's host-time budget.
steppingClock :: IO Clock
steppingClock = do
  tick <- newIORef (1000000000 :: Word64)
  pure (TimedClock (atomicModifyIORef' tick (\n -> (n + 1000, n))) (\ms -> modifyIORef' tick (+ fromIntegral ms * 1000000)))

writeJson :: FilePath -> Value -> IO ()
writeJson path value = writePrivateBytes path (BL.toStrict (encode value) <> "\n")

symbolAddress :: B.ByteString -> String -> IO Word32
symbolAddress symbols name = case
  [ address | line <- C.lines symbols, [raw, _, symbol] <- [C.words line]
            , symbol == C.pack name, [(address, "")] <- [readHex (C.unpack raw)] ] of
  [address] -> pure address
  _ -> ioError (userError ("missing or ambiguous runtime symbol: " ++ name))

wordBytes :: Word32 -> [Word8]
wordBytes value = [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

wordAt :: [Word8] -> Word32
wordAt bytes = sum [fromIntegral byte * 256 ^ index | (index, byte) <- zip [0 :: Int .. 3] bytes]

-- This callback transaction is linked only into the host emulator runner.
triggerSynthetic :: SyntheticEntry -> Address -> Clock -> UsbApi -> FilePath -> IO ()
triggerSynthetic (SyntheticEntry entry) (Address ringAddress) clock api directory = do
  ensurePrivateDirectory directory
  session <- newSession clock api directory (1024 * 1024) 600000
  held <- newHeld
  let release = releaseHeld held api (\_ action -> action transferTimeoutMs)
  flip finally release $ do
    device <- acquireDevice session held
    let input = inputEndpoint (descriptors device)
        output = outputEndpoint (descriptors device)
        control opcode count sid = tx session (P.controlTransfer (P.control opcode count sid)) 12
    _ <- control 0xc0 8 0
    opened <- tx session (P.BulkIn input 8) 8
    require (drop 2 opened == openReplyTail) "synthetic C0 reply tail mismatch"
    let sid = fromIntegral (opened !! 0) + 256 * fromIntegral (opened !! 1)
        exchange frame replyBytes = do
          _ <- control 0xd0 (length frame) sid
          _ <- tx session (P.BulkOut output frame) (length frame)
          _ <- control 0xd1 replyBytes sid
          tx session (P.BulkIn input replyBytes) replyBytes
        memory function address count payload = do
          let frame = concatMap wordBytes [0x200001, 0, 0, 0, function, 0, 0, address, count] ++ payload
              replyBytes = if function == 2 then 36 + fromIntegral count else 36
          reply <- exchange frame replyBytes
          require (take 4 reply == wordBytes 0x200001 && wordAt (drop 4 reply) == 0 && wordAt (drop 8 reply) == 0 && wordAt (drop 16 reply) == function && wordAt (drop 28 reply) == address && wordAt (drop 32 reply) == count) "synthetic memory reply mismatch"
          pure (drop 36 reply)
        readWord address = wordAt <$> memory 2 address 4 []
        writeWord address value = do
          _ <- memory 3 address 4 (wordBytes value)
          actual <- readWord address
          require (actual == value) "synthetic callback slot readback mismatch"
        slot = unAddress callbackSlotAddress
        stock = unWordValue callbackStockWord
        close = do
          _ <- control 0xc4 2 sid
          _ <- tx session (P.BulkIn input 2) 2
          pure ()
    flip finally close $ do
      original <- readWord slot
      require (original == stock) "synthetic callback slot is not stock"
      flip finally (writeWord slot stock) $ do
        writeWord slot entry
        let frame = wordBytes 0x450006 ++ replicate 12 0 ++ wordBytes 300 ++ replicate 12 0
        reply <- exchange frame 32
        require (wordAt reply == 0x450006 && wordAt (drop 20 reply) == fromIntegral afRingBytes) "synthetic callback reply extent mismatch"
      descriptor <- memory 2 (unAddress queryOutputAddress) 8 []
      require (wordAt descriptor == ringAddress && wordAt (drop 4 descriptor) == fromIntegral afRingBytes) "synthetic callback output descriptor mismatch"

runTrajectory :: Emulator -> [Segment] -> IO ([Value], Bool)
runTrajectory emulator segments = do
  results <- forM segments $ \segment -> do
    mode <- traverse (emulatorModeDial emulator) (segmentModeDial segment)
    steps <- forM (segmentSteps segment) $ \step -> case stepKey step of
      "half-press" -> pure (object ["key" .= stepKey step, "accepted" .= False
        , "unsupported" .= String "The retained MenuKey API has no shutter half-press binding; sending this key terminates its bridge. See issue #102 and e2e-fifth."], False)
      key -> do
        response <- emulatorKey emulator (stepTiming step) key
        pure (object ["key" .= key, "response" .= response, "accepted" .= True], False)
    pure (object ["phase" .= segmentPhase segment, "mode_dial" .= mode, "steps" .= map fst steps], any snd steps)
  pure (map fst results, any snd results)

run :: Options -> IO ()
run options = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  root@(Workspace workspace) <- Runtime.workspace Nothing
  nand <- privateInput root "--installed-nand" (optNand options)
  card <- privateInput root "--sd-card-image" (optCard options)
  receipt <- freshReceipt root (optReceipt options)
  ensurePrivateDirectory (workspace </> "private-data/analysis/x-m1-1.50/af-ring")
  ensurePrivateDirectory (workspace </> payloadBuildRoot afRingPayload)
  trajectoryBytes <- B.readFile (workspace </> "emulation/af-ring-e2e-trajectories.json")
  trajectory <- either (ioError . userError) pure (decodeTrajectories trajectoryBytes)
  writePrivateBytes (receipt </> "trajectory.json") trajectoryBytes
  staged <- stageViewerRuntime root Nothing (UniqueRuntime "xm1-af-ring-e2e") (Firmware <$> optFirmware options) StageStockOriginal
  let runtime = stagedDirectory staged
      Python python = stagedPython staged
  callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
  guest <- guestState runtime (InterpreterExecutable python) (UnicornLibrary (stagedUnicorn staged))
  installed <- sha256File (runtime </> stagedFirmwarePath)
  adjustment <- sha256File nand
  cardDigest <- sha256File card
  let key = routeKey guest CardAttach
        [ "profile" .= String "firmware/x-m1/1.50/emulator-rec.json", "installed_dat" .= sha256Hex installed
        , "adjustment_nand" .= sha256Hex adjustment, "menu_sd_image" .= sha256Hex cardDigest
        , "ui_conversion_profile" .= String "declared-ui-cpu-fallback" ]
      bridgeArguments =
        [ "tools/continue_rec_scanout.py", "--profile", "firmware/x-m1/1.50/emulator-rec.json"
        , "--installed-dat", stagedFirmwarePath, "--adjustment-nand", nand, "--menu-sd-image", card
        , "--menus", "--ram-push-server", "--output", receipt </> "bridge"
        , "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push"
        , "--checkpoint-key", routeKeyHex key, "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))
        , "--ui-conversion-profile", "declared-ui-cpu-fallback" ] ++ ["--fresh-boot" | optFresh options]
  bracket (startEmulator python runtime bridgeArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    let api = emulatorUsbApi emulator
        allocDirectory = receipt </> "alloc"
    ensurePrivateDirectory allocDirectory
    allocationOutcome <- steppingClock >>= \clock -> runRamAlloc (payloadAllocationSize afRingPayload) clock api allocDirectory
    statePath <- case allocationOutcome of
      RamAllocated path -> pure path
      other -> ioError (userError ("AF ring ram-alloc failed: " ++ show other))
    retained <- readRetainedAllocation statePath
    withUnreviewedBuild afRingPayload EnforceAllowlist (allocBase (retainedAllocation retained)) $ \pinned -> do
      let manifest = pinnedManifest pinned
          Address base = allocationBase manifest
          relinked = receipt </> "relinked"
          pushDirectory = receipt </> "push"
          unloadDirectory = receipt </> "unload"
      ensurePrivateDirectory relinked
      forM_ ["runtime.bin", "ram-push-manifest.json", "kernel.syms", "kernel.elf", "af-ring.json"] $ \name ->
        B.readFile (pinnedDirectory pinned </> name) >>= writePrivateBytes (relinked </> name)
      metadataBytes <- B.readFile (pinnedDirectory pinned </> "af-ring.json")
      RingMetadata address bytes entry digest <- either (ioError . userError) pure (eitherDecodeStrict' metadataBytes)
      require (address == Address (base + 0x40004000) && bytes == afRingBytes && digest == runtimeDigest manifest) "AF ring metadata differs from relinked manifest"
      let SyntheticEntry synthetic = entry
      require (synthetic >= base && toInteger synthetic + 4 <= toInteger base + toInteger (allocationSize manifest) && synthetic `mod` 4 == 0) "synthetic entry lies outside the payload"
      symbols <- B.readFile (pinnedDirectory pinned </> "kernel.syms")
      installerSites <- forM
        [("ram_push_rendezvous_locked", "rendezvous-locked"), ("ram_push_cpu0_window_exit", "cpu0-window-exit"), ("ram_push_cpu1_window_exit", "cpu1-window-exit")] $ \(name, label) ->
          (\value -> (value, label)) <$> symbolAddress symbols name
      emulatorTraceSites emulator (installerSites ++ [(synthetic, "af-ring-synthetic")] ++
        [(traceSiteAddress site, traceSiteLabel site) | site <- trajectoryTraceSites trajectory])
      ensurePrivateDirectory pushDirectory
      pushed <- steppingClock >>= \clock -> runUnreviewedRamPush retained manifest clock api pushDirectory
      pushStatePath <- case pushed of
        RamPushInstalled path -> pure path
        other -> ioError (userError ("AF ring ram-push failed: " ++ show other))
      pushState <- readRamPushState pushStatePath
      let capture name = do
            let directory = receipt </> name
            ensurePrivateDirectory directory
            steppingClock >>= \clock -> readAfRing (AfRingManifest manifest) clock api directory
          unload = do
            ensurePrivateDirectory unloadDirectory
            outcome <- steppingClock >>= \clock -> runUnreviewedRamUnload pushState clock api unloadDirectory
            require (outcome == RamUnloaded) ("AF ring ram-unload failed: " ++ show outcome)
      flip finally unload $ do
        baseline <- capture "baseline"
        (observations, halfPressAccepted) <- runTrajectory emulator (beforeUnload trajectory)
        ordinary <- capture "ordinary-half-press"
        let genuine = halfPressAccepted && any (\event -> eventKind event /= Synthetic && unEventSequence (eventSequence event) > unEventCount (ringCommittedTotal baseline)) (ringEvents ordinary)
        require (not (any ((== Synthetic) . eventKind) (ringEvents ordinary))) "synthetic event preceded the synthetic trigger"
        result <- if genuine then pure ordinary else do
          clock <- steppingClock
          triggerSynthetic entry address clock api (receipt </> "synthetic-trigger")
          captured <- capture "synthetic-fallback"
          require (unEventCount (ringCommittedTotal captured) == unEventCount (ringCommittedTotal ordinary) + 300 && unEventCount (ringOverwritten captured) >= 44 && length (filter ((== Synthetic) . eventKind) (ringEvents captured)) == 256) "synthetic recorder did not commit the requested overwrite exercise"
          pure captured
        writeJson (receipt </> "e2e-result.json") (object
          [ "schema" .= (1 :: Int), "scope" .= String "host-only emulated X-M1 1.50 AF ring, unreviewed transient payload"
          , "payload" .= String "af-ring", "live_review_approved" .= False
          , "emulator_start" .= startRecord start, "allocation_base" .= ("0x" ++ showHex base "")
          , "trajectory" .= observations, "baseline" .= afRingValue baseline, "ordinary_attempt" .= afRingValue ordinary
          , "event_origin" .= (if genuine then String "ordinary-af" else String "synthetic-fallback")
          , "ordinary_af_observed" .= genuine, "half_press_accepted" .= halfPressAccepted, "synthetic_trigger_count" .= (if genuine then 0 else 300 :: Int)
          , "exclusive_model" .= object
              [ "uncontended_cpu0_local_monitor_exercised" .= (not genuine && not (ringContentionLoss result))
              , "contention_or_preemption_established" .= False
              , "global_monitor_established" .= False
              , "global_monitor_relied_on" .= False
              ]
          , "every_event_cpu0_validated" .= all ((== AfCpu0) . eventCore) (ringEvents result)
          , "missing_input" .= (if genuine then Null else if halfPressAccepted
              then String "The half-press request was accepted but no new AF hook events reached the ring within its bounded pulse. The bridge session receipt and trajectory record the supplied inputs; a working ordinary AF statistic/lens-response producer is not established by this run."
              else String "The retained ram-push bridge routes key requests through emulator/menu_inputs.py MenuKey, which has no shutter half-press binding. The unsupported input is flagged without sending the bridge-terminating request. No ordinary AF statistic producer or lens response was added by this runner.")
          , "capture_contract" .= String "best-effort bulk capture, not a coherent snapshot; shared decoder validates header and slot sequences"
          , "result" .= afRingValue result, "unload" .= String "mandatory after capture; see unload journals and e2e-complete.json" ])
    writeJson (receipt </> "e2e-complete.json") (object ["unloaded" .= True, "allocation_retained_until_power_off" .= True])
  putStrLn ("AF ring e2e complete: " ++ receipt </> "e2e-result.json")

main :: IO ()
main = do
  arguments <- getArgs
  if arguments == ["--help"] then putStr usage else
    (parseOptions arguments >>= run) `catch` \failure -> do
      hPutStrLn stderr (displayException (failure :: IOException))
      exitFailure
