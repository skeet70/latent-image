{-# LANGUAGE OverloadedStrings #-}
module Xm1RamPushE2E (RelinkMode (..), AllowlistPolicy (..), OverrideReason (..), BootMode (..), PushMode (..), AttachHold (..), UiConversionProfile (..), CardMode (..), EmulationRuntime (..), FujiModBridge (..), ModalDiagnostic (..), E2ESettings (..), SymbolRead, parseSymbolRead, Payload, bankD3Payload, bankModal75Payload, bankLifecyclePayload, bankModalLifecyclePayload, classicChromePayload, classicChromeIconPayload, experimentPayload, runRamPushE2E, warmAttachCheckpoint) where

import Control.Applicative ((<|>))
import Control.Exception (IOException, SomeException, bracket, catch, displayException, finally, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, toJSON, withObject, (.:), (.=))
import Data.Aeson.Types (parseEither)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (nub, sort)
import Data.Maybe (catMaybes, isJust)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.ByteString.Char8 as C
import GHC.Clock (getMonotonicTime)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftL, shiftR, (.|.))
import Data.Int (Int32)
import Data.Word (Word32)
import Numeric (readHex, showFFloat, showHex)
import System.Directory (createDirectory, doesDirectoryExist, doesFileExist, getCurrentDirectory, listDirectory, removeDirectory, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getExecutablePath, lookupEnv)
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), hSetBuffering, stdout)
import System.Posix.Files (setFileMode)
import System.Posix.Temp (mkdtemp)
import Text.Read (readMaybe)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, hostProcedure, procedureHex, routeKey, routeKeyHex)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.SegmentReceipt (SegmentReceipt (..), SegmentResult (..), elapsedSeconds, segmentName, segmentNameText)
import Emulation.Symbols (SymbolTable, parseSymbolTable, resolveSymbol)
import Emulation.PreservationObservation
import Emulation.ModalObservation
import qualified Emulation.SaveLifecycle as SaveLifecycle
import Emulation.SaveLifecycleRun (runSaveLifecycle)
import Emulation.UiTrajectory
import Emulation.WatchObservation
import Emulation.ViewerRuntime (FixedSceneFrame (..), fixedSceneFrameArguments)
import EmulatorUsb
import FujiAF.RamPush
import FujiAF.Transport (ensurePrivateDirectory, realClock, writePrivateBytes)
import Xm1UnreviewedRelink (runUnreviewedRamPush, runUnreviewedRamUnload, withUnreviewedBuild)
import RamPushReviewedPins (reviewedContract)
import RamPushProcedure (procedureSourcesDigest)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)

-- | Unreviewed skips the compiled source-identity and publication-contract pins; its policy
-- decides whether a bank that fails the ELF allowlist is still pushed. Reviewed enforces it.
data RelinkMode = ReviewedRelink | UnreviewedRelink AllowlistPolicy deriving (Eq, Show)

-- | Fresh cold-boots and replaces the keyed attach checkpoint.
data BootMode = CheckpointedBoot | FreshBoot deriving (Eq, Show)

-- | FlashCandidate boots a DAT whose own loader publishes the bank and runs the segments
-- without ram-alloc, ram-push or ram-unload.
data PushMode = RamPush | FlashCandidate deriving (Eq, Show)

preservationReceipt :: PreservationRanges -> PreservationObservation -> WatchJudgement -> Value
preservationReceipt ranges observation judgement = object
  [ "enforcement" .= ("ram-byte-preservation" :: T.Text)
  , "ranges" .= preservationRangesJson ranges
  , "counters" .= preservationCountersJson (preservationCounters observation)
  , "first_violation" .= maybe Null preservationViolationJson (preservationFirstViolation observation)
  , "verdict" .= verdict ]
  where
    verdict = case judgement of
      CandidateQualified -> "preserved" :: T.Text
      ControlRefusedProtectedWrite -> "refused"

mediaJson :: MediaObservation -> Value
mediaJson media = object
  [ "present" .= mediaPresent media
  , "write_protected" .= mediaWriteProtected media
  , "dirty_sectors" .= sectorsValue (mediaDirtySectors media)
  , "started" .= commandsValue (mediaStarted media)
  , "completed" .= commandsValue (mediaCompleted media)
  , "irqs" .= irqsValue (mediaIrqs media)
  , "dma_boundaries" .= boundariesValue (mediaDmaBoundaries media)
  , "accepted_writes" .= acceptedWritesValue (mediaAcceptedWrites media)
  , "accepted_write_bytes" .= acceptedBytesValue (mediaAcceptedBytes media) ]

-- | A key held across the attach edge of every boot the e2e starts.
data AttachHold = NoAttachHold | AttachHoldQ deriving (Eq, Show)

-- | The UI conversion contract a session boots with; DeclaredUiCpuFallback is the
-- viewer's opt-in stock CPU conversion, which mode-dial positions with a mode guide need.
data UiConversionProfile = StrictUiConversion | DeclaredUiCpuFallback deriving (Eq, Show)

-- | CardBacked boots with the named virtual card image; Cardless boots with card-detect absent
-- through the cardless RAM-push bridge.
data CardMode = CardBacked FilePath | Cardless deriving (Eq, Show)

-- | The FujiMod RAM-push bridge (`emulation/fujimod-xm1`), its run document and the boot ROM.
data FujiModBridge = FujiModBridge
  { fujimodBridge :: FilePath
  , fujimodRunDocument :: FilePath
  , fujimodBootRom :: FilePath
  } deriving (Eq, Show)

-- | The engine behind the bridge protocol: the retained Python/native closure or jitl's FujiMod.
data EmulationRuntime = RetainedRuntime | FujiModRuntime FujiModBridge deriving (Eq, Show)

newtype ExportedSymbol = ExportedSymbol T.Text deriving (Eq, Show)
newtype ReadLength = ReadLength Int deriving (Eq, Show)
data SymbolRead = SymbolRead ExportedSymbol ReadLength deriving (Eq, Show)

parseSymbolRead :: String -> Either String SymbolRead
parseSymbolRead text = do
  let (name, suffix) = break (== ':') text
  unless (not (null name)) (Left "--read needs a symbol name")
  count <- case suffix of
    "" -> Right 64
    ':' : digits | Just value <- readMaybe digits, value > 0, value <= 65536 -> Right value
    _ -> Left "--read LENGTH must be an integer from 1 through 65536"
  pure (SymbolRead (ExportedSymbol (T.pack name)) (ReadLength count))

newtype ExperimentMetadata = ExperimentMetadata [(ExportedSymbol, Address)]

instance FromJSON ExperimentMetadata where
  parseJSON = withObject "experiment metadata" $ \fields -> do
    symbols <- fields .: "symbols"
    ExperimentMetadata <$> forM (KM.toList symbols) (\(name, value) -> do
      text <- parseJSON value
      address <- case text of
        '0' : 'x' : digits | not (null digits), length digits <= 8, [(word, "")] <- readHex digits -> pure (Address word)
        _ -> fail "experiment symbol address must be a 32-bit 0x-hex address"
      pure (ExportedSymbol (K.toText name), address))

data E2ESettings = E2ESettings
  { relinkMode :: RelinkMode
  , bootMode :: BootMode
  , checkpointRoot :: FilePath
  , segmentSelection :: Maybe [T.Text]
  , pushMode :: PushMode
  , e2ePayload :: Payload
  , attachHold :: AttachHold
  , flashUiConversion :: UiConversionProfile
  , emulationCardMode :: CardMode
  , fixedSceneFrame :: Maybe FixedSceneFrame
    -- | The card attach checkpoint key, which 'runRamPushE2E' sets.
  , cardAttachKey :: Maybe RouteKey
  , emulationRuntime :: EmulationRuntime
  -- | The DAT the user supplied, which a flash candidate's symbol table sits beside.
  , firmwareSource :: FilePath
  , experimentReads :: [SymbolRead]
  , modalDiagnostic :: ModalDiagnostic
  }

-- | The card attach checkpoint key: the staged guest state and the session's inputs.
cardAttachRouteKey :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO RouteKey
cardAttachRouteKey settings python runtime installedMedia adjustmentNand cardImage = do
  unicorn <- lookupEnv "X70_UNICORN_LIBRARY" >>= maybe (ioError (userError "X70_UNICORN_LIBRARY is missing")) pure
  guest <- guestState runtime (InterpreterExecutable python) (UnicornLibrary unicorn)
  installed <- sha256File (runtime </> installedMedia)
  adjustment <- sha256File adjustmentNand
  card <- sha256File cardImage
  pure (routeKey guest CardAttach (
    [ "profile" .= String "firmware/x-m1/1.50/emulator-rec.json"
    , "installed_dat" .= sha256Hex installed
    , "adjustment_nand" .= sha256Hex adjustment
    , "menu_sd_image" .= sha256Hex card ]
    ++ [ "attach_hold_key" .= String "q" | attachHold settings == AttachHoldQ ]
    ++ [ "ui_conversion_profile" .= String "declared-ui-cpu-fallback" | flashUiConversion settings == DeclaredUiCpuFallback ]
    ++ [ "fixed_scene_frame" .= frame | Just (FixedSceneFrame frame) <- [fixedSceneFrame settings] ]))

attachHoldArguments :: AttachHold -> [String]
attachHoldArguments NoAttachHold = []
attachHoldArguments AttachHoldQ = ["--attach-hold-key", "q"]

attachHoldText :: AttachHold -> T.Text
attachHoldText NoAttachHold = "none"
attachHoldText AttachHoldQ = "q"

cardRecord :: CardMode -> Value
cardRecord mode = object
  [ "present" .= (mode /= Cardless)
  , "image" .= (case mode of CardBacked image -> String (T.pack image); Cardless -> Null) ]

-- | The bridge executable and its arguments for one boot of the session's inputs writing to `output`.
sessionBridge :: E2ESettings -> FilePath -> FilePath -> FilePath -> BootMode -> FilePath -> IO (FilePath, [String])
sessionBridge settings python installedMedia adjustmentNand boot output = case (emulationRuntime settings, emulationCardMode settings) of
  (FujiModRuntime bridge, card) -> do
    require (card /= Cardless || attachHold settings == NoAttachHold) "the cardless RAM-push bridge holds no key across the attach edge"
    require (null (fixedSceneFrame settings)) "the FujiMod RAM-push bridge takes no --fixed-scene-frame"
    pure (fujimodBridge bridge,
      [ "--run", fujimodRunDocument bridge
      , "--boot-rom", fujimodBootRom bridge
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      ] ++ freshArguments ++ cardArguments card ++ attachHoldArguments (attachHold settings))
  (RetainedRuntime, CardBacked cardImage) -> do
    key <- maybe (ioError (userError "the card attach key is unset")) pure (cardAttachKey settings)
    pure (python,
      [ "tools/continue_rec_scanout.py"
      , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--menu-sd-image", cardImage
      , "--menus"
      , "--ram-push-server"
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      , "--checkpoint-key", routeKeyHex key
      , "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))
      ] ++ freshArguments ++ profileArguments ++ attachHoldArguments (attachHold settings) ++ frameArguments)
  (RetainedRuntime, Cardless) -> do
    require (attachHold settings == NoAttachHold) "the cardless RAM-push bridge holds no key across the attach edge"
    bridge <- runfilesCardlessBridge
    pure (bridge,
      [ "--profile", "firmware/x-m1/1.50/emulator-rec.json"
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      ] ++ freshArguments ++ profileArguments ++ frameArguments)
  where
    freshArguments = ["--fresh-boot" | boot == FreshBoot]
    profileArguments = case flashUiConversion settings of
      StrictUiConversion -> []
      DeclaredUiCpuFallback -> ["--ui-conversion-profile", "declared-ui-cpu-fallback"]
    frameArguments = fixedSceneFrameArguments (fixedSceneFrame settings)
    cardArguments (CardBacked image) = ["--sd-card-image", image]
    cardArguments Cardless = []

payloadText :: Payload -> T.Text
payloadText p = case payloadKind p of
  RecipeBankD3 -> "recipe-bank-d3"
  RecipeBankModal75 -> "recipe-bank-modal75"
  RecipeBankLifecycle -> "recipe-bank-lifecycle"
  RecipeBankModalLifecycle -> "recipe-bank-modal-lifecycle"
  ClassicChrome -> "classic-chrome"
  ClassicChromeIcon -> "classic-chrome-icon"
  AfRing -> "af-ring"
  UsbToggle -> "usb-toggle"
  Experiment -> "experiment"

payloadContract :: Payload -> ReviewedContract
payloadContract p = case payloadKind p of
  RecipeBankD3 -> reviewedContract
  RecipeBankModal75 -> error "scoped modal bank has no reviewed live contract"
  RecipeBankLifecycle -> error "lifecycle bank has no reviewed live contract"
  RecipeBankModalLifecycle -> error "modal lifecycle bank has no reviewed live contract"
  ClassicChrome -> reviewedCcContract
  ClassicChromeIcon -> reviewedCcIconContract
  AfRing -> error "AF ring has no live reviewed contract; use //emulation:xm1-af-ring-e2e"
  UsbToggle -> reviewedToggleContract
  Experiment -> error "experiments have no reviewed contract"

-- | Whether the payload carries the Classic Chrome publications.
carriesClassicChrome :: Payload -> Bool
carriesClassicChrome p = payloadKind p `elem` [ClassicChrome, ClassicChromeIcon]

-- | The Classic Chrome entry words and hook sites (reports/classic-chrome-patch.md
-- section 8.2.7 step 2), armed from the attach to the end of install.
classicChromeFetchSites :: [(Word32, String)]
classicChromeFetchSites =
  [ (0x00738fe8, "cc-entry-capture-converter"), (0x00736a74, "cc-entry-settings-converter")
  , (0x00d7851c, "cc-entry-marker"), (0x00eab2e0, "cc-entry-filmmode"), (0x00eaac88, "cc-entry-saturation")
  , (0x00d461ec, "cc-entry-raw-commit"), (0x00d5ada4, "cc-entry-raw-highlight"), (0x012548a4, "cc-entry-raw-request")
  , (0x00d67284, "cc-hook-colour"), (0x00d60f60, "cc-hook-chroma-1"), (0x00d64ef4, "cc-hook-chroma-2")
  , (0x00d677ac, "cc-hook-tone-1"), (0x00d677bc, "cc-hook-tone-2") ]

relinkModeText :: RelinkMode -> T.Text
relinkModeText ReviewedRelink = "reviewed"
relinkModeText (UnreviewedRelink _) = "unreviewed"

require :: Bool -> String -> IO ()
require yes message = unless yes (ioError (userError message))

rawWatch :: IO Value -> IO (Either String Value)
rawWatch action = (Right <$> action) `catch` \(failure :: SomeException) -> pure (Left (displayException failure))

privateDirectory :: FilePath -> IO ()
privateDirectory path = createDirectory path >> setFileMode path 0o700

-- | Runs a transport session in a private directory on tmpfs (XDG_RUNTIME_DIR, else /dev/shm),
-- then copies its files into DIRECTORY, which must exist, whether or not the session succeeded.
-- The transport fsyncs every journal line, which costs tens of milliseconds a transfer on disk.
stagedSession :: FilePath -> (FilePath -> IO a) -> IO a
stagedSession directory session = do
  root <- maybe "/dev/shm" id <$> lookupEnv "XDG_RUNTIME_DIR"
  bracket (mkdtemp (root </> "xm1-e2e-session-")) removeDirectoryRecursive $ \staging -> do
    let work = staging </> "session"
    privateDirectory work
    session work `finally` (listDirectory work >>= mapM_ (\name -> B.readFile (work </> name) >>= writePrivateBytes (directory </> name)))

-- | Entry of the plain-image drawer that renders a menu message banner; r0 is the message id.
menuMessageDrawer :: Word32
menuMessageDrawer = 0x011392e0

menuMessageSite :: String
menuMessageSite = "menu-message"

-- | The D3 recipe hook's entry in the image-4 prefix (reports/recipe-patch-d3.md section 5.2).
d3HookEntry :: Word32
d3HookEntry = 0x0061c3ac

d3HookSite :: String
d3HookSite = "d3-hook"

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

menuState :: Value -> IO MenuState
menuState value = either (ioError . userError . ("invalid menu response: " ++)) pure $ parseEither parser value
  where
    parser = withObject "response" $ \response ->
      response .: "menu" >>= withObject "menu" (\current ->
        MenuState <$> current .: "menu_id" <*> current .: "setting_id" <*> current .: "cursor")
-- | A step key of four colon-separated fields (seek or cursor, a key, a target, a press limit) presses
-- the key until the setting id (seek) or the cursor (cursor) equals the target, at most limit times;
-- any other key is pressed once. Returns the key presses made and the final state.
pressStep :: Emulator -> KeyTiming -> String -> IO ([String], MenuState)
pressStep emulator timing key = case splitOn ':' key of
  [kind, seekKey, targetText, limit]
    | kind `elem` ["seek", "cursor"], [(target, "")] <- reads targetText, [(bound, "")] <- reads limit ->
    let field (MenuState _ setting cursor) = if kind == "seek" then setting else cursor
        go count pressed = do
          actual <- emulatorKey emulator timing seekKey >>= menuState
          if field actual == target || count + 1 >= (bound :: Int)
            then pure (reverse (seekKey : pressed), actual)
            else go (count + 1) (seekKey : pressed)
     in go 0 []
  _ -> (\actual -> ([key], actual)) <$> (emulatorKey emulator timing key >>= menuState)
  where
    splitOn c text = case break (== c) text of
      (front, []) -> [front]
      (front, _ : rest) -> front : splitOn c rest

-- | A step's own pulse, else the segment's; its own settle cap.
stepKeyTiming :: Maybe KeyPulse -> Step -> KeyTiming
stepKeyTiming segmentPulse step = KeyTiming (keyTimingPulse timing <|> segmentPulse) (keyTimingSettle timing)
  where timing = stepTiming step

press :: Emulator -> Maybe KeyPulse -> Step -> IO ()
press emulator pulse step = do
  (_, actual) <- pressStep emulator (stepKeyTiming pulse step) (stepKey step)
  require (actual == stepExpected step)
    ("menu trajectory after " ++ stepKey step ++ ": expected " ++ show (stepExpected step) ++ ", got " ++ show actual)

presses :: Emulator -> Maybe KeyPulse -> [Step] -> IO ()
presses emulator pulse = mapM_ (press emulator pulse)

-- | Presses every step and records the key presses, expected and actual state and wall seconds
-- without requiring the state.
observe :: Emulator -> Maybe KeyPulse -> [Step] -> IO [Value]
observe emulator pulse = mapM $ \step -> do
  let MenuState em es ec = stepExpected step
  started <- getMonotonicTime
  (pressed, MenuState am as ac) <- pressStep emulator (stepKeyTiming pulse step) (stepKey step)
  ended <- getMonotonicTime
  pure (object [ "key" .= stepKey step, "pressed" .= length pressed
               , "expected" .= [em, es, ec], "actual" .= [am, as, ac]
               , "matched" .= (MenuState am as ac == stepExpected step), "seconds" .= (ended - started) ])

data LensPreparation = OpenLensMenu | FindLensRow | OpenLensSetting | SelectLensOn | CommitLensSetting | CloseLensMenu
  deriving (Show)

prepareLifecycleCapture :: Emulator -> IO Value
prepareLifecycleCapture emulator = do
  keys <- forM [OpenLensMenu,FindLensRow,OpenLensSetting,SelectLensOn,CommitLensSetting,CloseLensMenu] $ \step -> do
    let key = case step of
          OpenLensMenu -> "ok"
          FindLensRow -> "cursor:down:20:64"
          OpenLensSetting -> "right"
          SelectLensOn -> "up"
          CommitLensSetting -> "ok"
          CloseLensMenu -> "back"
    (pressed,MenuState menu setting cursor) <- pressStep emulator defaultKeyTiming key
    case step of
      FindLensRow -> require (cursor == 20) "shooting-menu row 20 was not reached"
      OpenLensSetting -> require (setting == 0xa3) "SHOOT WITHOUT LENS setting was not opened"
      SelectLensOn -> require (cursor == 0) "SHOOT WITHOUT LENS ON was not selected"
      _ -> pure ()
    pure (object ["step" .= show step,"pressed" .= pressed,"menu" .= [menu,setting,cursor]])
  dialFirst <- emulatorModeDial emulator ModeDialProgram
  first <- emulatorCapture emulator "lifecycle-pre-press-a"
  dialSecond <- emulatorModeDial emulator ModeDialProgram
  second <- emulatorCapture emulator "lifecycle-pre-press"
  lens0 <- emulatorReadMemory emulator 0x009504d4 4
  lens1 <- emulatorReadMemory emulator 0x00950508 4
  pure (object ["keys" .= keys,"mode_dial" .= modeDialName ModeDialProgram
    ,"dial_replies" .= [dialFirst,dialSecond],"captures" .= [first,second],"lens_words" .= map hexBytes [lens0,lens1]])

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""


symbolAddress :: FilePath -> String -> IO Word32
symbolAddress path name = do
  table <- parseSymbolTable <$> B.readFile path
  either (ioError . userError) pure (resolveSymbol table name)

-- | The bank symbol table a flash candidate's package carries beside its linked kernel.
flashCandidateSymbols :: FilePath -> IO SymbolTable
flashCandidateSymbols firmware = do
  let path = takeDirectory firmware </> "kernel" </> "kernel.syms"
  present <- doesFileExist path
  unless present (ioError (userError ("the flash candidate has no symbol table: " ++ path)))
  parseSymbolTable <$> B.readFile path

mailboxFields :: [String]
mailboxFields =
  [ "magic", "abi", "nonce", "operation", "status", "validated", "setup_stored"
  , "activation_stored", "restored", "service_core", "peer_nonce", "peer_core", "peer_status"
  , "peer_lines", "spin_bound", "cpu0_ack_spins"
  , "rendezvous_parked", "rendezvous_go", "rendezvous_completed", "cpu0_park_spins", "cpu1_spins"
  , "cpu0_cycles", "cpu0_instructions", "cpu1_cycles", "cpu1_instructions", "reserved" ]

-- | Little-endian word `index` of `bytes`.
leWord :: B.ByteString -> Int -> Word32
leWord bytes index = foldr (\byte acc -> acc `shiftL` 8 .|. fromIntegral byte) 0
  (B.unpack (B.take 4 (B.drop (4 * index) bytes)))

mailboxReport :: B.ByteString -> Value
mailboxReport bytes = object (zipWith field mailboxFields [0 ..])
  where
    field name index
      | name `elem` ["status", "peer_status"] = K.fromString name .= (fromIntegral (leWord bytes index) :: Int32)
      | otherwise = K.fromString name .= leWord bytes index

-- | The segments one emulator process runs, whether it unloads between them, and whether it may
-- start from a matching install checkpoint instead of allocating and pushing.
data Plan = Plan
  { planName :: T.Text
  , planBefore :: [Segment]
  , planAfter :: [Segment]
  , planUnload :: Bool
  , planFromInstall :: Bool
  , planTraceSites :: [TraceSite]
  , planProtection :: TrajectoryProtection
  , planExpect :: TrajectoryExpect
  , planForbiddenExecution :: [WatchRange]
  , planExecutionInterval :: ExecutionInterval
  , planExecutionControlPc :: Maybe WatchAddress
  }


-- | The named phase with every phase it transitively requires, in trajectory order; a phase after
-- unload brings the unload with it.
segmentPlan :: Trajectories -> T.Text -> IO Plan
segmentPlan trajectories name = do
  require (name `elem` phaseNames trajectories) ("unknown trajectory segment: " ++ T.unpack name)
  let entries = beforeUnload trajectories ++ afterUnload trajectories
      requiresOf phase = nub (concat
        [segmentRequires entry ++ map preservedSourcePhase (segmentReadPreservations entry)
        | entry <- entries, segmentPhase entry == phase])
      close [] seen = seen
      close (phase : rest) seen
        | phase `elem` seen = close rest seen
        | otherwise = close (requiresOf phase ++ rest) (phase : seen)
      chosen = close [name] []
      selected = filter ((`elem` chosen) . segmentPhase)
      after = selected (afterUnload trajectories)
  pure (Plan name (selected (beforeUnload trajectories)) after (not (null after)) True
    (trajectoryTraceSites trajectories) (trajectoryProtection trajectories) (trajectoryExpect trajectories)
    (trajectoryForbiddenExecution trajectories) (trajectoryExecutionInterval trajectories) (trajectoryExecutionControlPc trajectories))

-- | A relinker build held in memory.
data Relinked = Relinked
  { relinkedDocument :: B.ByteString
  , relinkedRuntime :: B.ByteString
  , relinkedSyms :: B.ByteString
  , relinkedDisasm :: B.ByteString
  , relinkedManifest :: RamPushManifest
  , relinkedAllowlist :: AllowlistOutcome
  , relinkedExperiment :: Maybe B.ByteString
  , relinkedModal :: Maybe RelinkedModal
  , relinkedAllowlistInput :: Maybe B.ByteString
  , relinkedLifecycle :: Maybe B.ByteString
  }

data RelinkedModal = RelinkedModal
  { relinkedModalScope :: B.ByteString
  , relinkedModalElf :: B.ByteString
  }

-- | What an install checkpoint carries for the host: the allocation and install state files, the
-- installer mailbox after install and the heap break around ram-alloc.
data InstallHost = InstallHost
  { hostAllocationState :: B.ByteString
  , hostPushState :: B.ByteString
  , hostInstallMailbox :: Value
  , hostBreakBefore :: String
  , hostBreakAfter :: String
  }

instance FromJSON InstallHost where
  parseJSON = withObject "install host" $ \host -> InstallHost
    <$> (TE.encodeUtf8 <$> host .: "allocation_state")
    <*> (TE.encodeUtf8 <$> host .: "push_state")
    <*> host .: "install_mailbox"
    <*> host .: "heap_break_before"
    <*> host .: "heap_break_after"

installHostValue :: InstallHost -> Value
installHostValue host = object
  [ "allocation_state" .= TE.decodeUtf8 (hostAllocationState host)
  , "push_state" .= TE.decodeUtf8 (hostPushState host)
  , "install_mailbox" .= hostInstallMailbox host
  , "heap_break_before" .= hostBreakBefore host
  , "heap_break_after" .= hostBreakAfter host ]

-- | GIVEN with its card attach key, timed and printed; the cardless and FujiMod bridges key their own.
keyedSettings :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> IO E2ESettings
keyedSettings given python runtime installedMedia adjustmentNand = case (emulationRuntime given, emulationCardMode given) of
  (FujiModRuntime _, _) -> pure given
  (RetainedRuntime, Cardless) -> pure given
  (RetainedRuntime, CardBacked cardImage) -> do
    started <- getMonotonicTime
    key <- cardAttachRouteKey given python runtime installedMedia adjustmentNand cardImage
    ended <- getMonotonicTime
    putStrLn ("checkpoint key: " ++ routeKeyHex key ++ " in " ++ seconds (ended - started))
    pure given { cardAttachKey = Just key }

-- | Boots or restores the session's attach checkpoint, saving it on a cold boot, then stops.
warmAttachCheckpoint :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
warmAttachCheckpoint given python installedMedia adjustmentNand output = do
  hSetBuffering stdout LineBuffering
  runtime <- getCurrentDirectory
  settings <- keyedSettings given python runtime installedMedia adjustmentNand
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) output
  started <- getMonotonicTime
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $
    \(_, start, ()) -> do
      ended <- getMonotonicTime
      putStrLn ("attach checkpoint: " ++ show (bootStart start) ++ " in " ++ seconds (ended - started))
      C.putStrLn (BL.toStrict (encode (startRecord start)))

-- | Runs the whole trajectory in one process, or with `segmentSelection` each named segment (and
-- what it requires) in its own process from the install checkpoint, under RECEIPT/segment-NAME.
runRamPushE2E :: E2ESettings -> FilePath -> FilePath -> Maybe FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runRamPushE2E given python workspace trajectoryFile installedMedia adjustmentNand receipt = do
  outcome <- try (runRamPushE2EUsing given python workspace trajectoryFile installedMedia adjustmentNand receipt)
  case outcome of
    Right () -> putStrLn ("receipt " ++ receipt ++ ": PASS (install, trajectory, unload)")
    Left (failure :: SomeException) -> do
      putStrLn ("receipt " ++ receipt ++ ": FAIL (install, trajectory, unload): " ++ displayException failure)
      throwIO failure

runRamPushE2EUsing :: E2ESettings -> FilePath -> FilePath -> Maybe FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runRamPushE2EUsing given python workspace trajectoryFile installedMedia adjustmentNand receipt = do
  hSetBuffering stdout LineBuffering
  bytes <- maybe (pure defaultExperimentTrajectory) B.readFile trajectoryFile
  trajectories <- either (ioError . userError . ("invalid RAM-push e2e trajectories: " ++)) pure (decodeTrajectories bytes)
  let lifecycleRequested = any (isJust . segmentSaveLifecycle) (beforeUnload trajectories ++ afterUnload trajectories)
  when lifecycleRequested $ require (pushMode given == RamPush)
    "save lifecycle observation requires an installed RAM package"
  when (payloadKind (e2ePayload given) `elem` [RecipeBankModal75, RecipeBankLifecycle, RecipeBankModalLifecycle]) $ do
    require (pushMode given == RamPush
      && (emulationCardMode given == Cardless
        || (payloadKind (e2ePayload given) == RecipeBankLifecycle && lifecycleRequested))
      && case emulationRuntime given of FujiModRuntime _ -> True; _ -> False)
      "unqualified bank variants require FujiMod RAM push; a virtual card requires the lifecycle capture interval"
    require (relinkMode given == UnreviewedRelink EnforceAllowlist)
      "unqualified bank variants require --unreviewed-relink without an allowlist override"
    require (modalDiagnostic given == NoModalDiagnostic) "--modal-observation is a stock no-install diagnostic"
  when (payloadKind (e2ePayload given) `elem` [RecipeBankModal75, RecipeBankModalLifecycle]) $
    either (ioError . userError) pure (checkModalObserverConfiguration trajectories)
  when (modalDiagnostic given == RetainedFilmModalDiagnostic) $ do
    require (pushMode given == FlashCandidate && emulationCardMode given == Cardless
      && case emulationRuntime given of FujiModRuntime _ -> True; _ -> False)
      "modal observation requires the no-install --cardless --fujimod --flash-candidate route"
    either (ioError . userError) pure (checkModalObserverConfiguration trajectories)
    require (not (null (beforeUnload trajectories)) && null (afterUnload trajectories)
      && all ((== "retained-film-route") . segmentPhase) (beforeUnload trajectories))
      "modal observation requires the retained-film-route physical-key trajectory"
  let settleCaps = [ () | segment <- beforeUnload trajectories ++ afterUnload trajectories
                        , step <- segmentSteps segment, isJust (keyTimingSettle (stepTiming step)) ]
  case emulationRuntime given of
    RetainedRuntime | not (null settleCaps) ->
      ioError (userError "settle_instructions is FujiMod press timing; the retained bridge takes no settle cap")
    _ -> pure ()
  exists <- doesDirectoryExist receipt
  when exists $ do
    entries <- listDirectory receipt
    require (null entries) "RAM-push receipt directory must be fresh or empty"
    removeDirectory receipt
  runtime <- getCurrentDirectory
  settings <- keyedSettings given python runtime installedMedia adjustmentNand
  -- Host RAM-push operations resolve workspace-relative builder paths.
  setCurrentDirectory workspace
  let session = case pushMode settings of
        RamPush -> runSession settings python runtime installedMedia adjustmentNand
        FlashCandidate -> runFlashSession settings python runtime installedMedia adjustmentNand
      selection = case (segmentSelection settings, pushMode settings) of
        (Just names, _) -> Just names
        (Nothing, FlashCandidate) -> Just (phaseNames trajectories)
        (Nothing, RamPush) -> Nothing
  case selection of
    Nothing -> session (Plan "all" (beforeUnload trajectories) (afterUnload trajectories) True False
      (trajectoryTraceSites trajectories) (trajectoryProtection trajectories) (trajectoryExpect trajectories)
      (trajectoryForbiddenExecution trajectories) (trajectoryExecutionInterval trajectories) (trajectoryExecutionControlPc trajectories)) receipt
    Just names -> do
      plans <- mapM (segmentPlan trajectories) names
      when (pushMode settings == FlashCandidate) $ forM_ plans $ \plan ->
        require (not (planUnload plan)) ("segment " ++ T.unpack (planName plan) ++ " follows ram-unload, which a flash candidate does not run")
      privateDirectory receipt
      outcomes <- forM plans $ \plan -> do
        name <- either (ioError . userError) pure (segmentName (planName plan))
        started <- getMonotonicTime
        outcome <- try (session plan (receipt </> ("segment-" ++ T.unpack (segmentNameText name))))
        ended <- getMonotonicTime
        elapsed <- either (ioError . userError) pure (elapsedSeconds (ended - started))
        let failure = either (\(exception :: SomeException) -> Just (T.pack (displayException exception))) (const Nothing) outcome
            problem = either Just (const Nothing) outcome :: Maybe SomeException
        putStrLn ("segment " ++ T.unpack (planName plan) ++ ": " ++ maybe "passed" (("failed: " ++) . T.unpack) failure
          ++ " in " ++ seconds (ended - started))
        pure (SegmentReceipt name elapsed (maybe SegmentPassed SegmentFailed failure), problem)
      writePrivateBytes (receipt </> "e2e-segments.json") (BL.toStrict (encode (map fst outcomes)) <> "\n")
      require (all (null . snd) outcomes) "one or more trajectory segments failed"

defaultExperimentTrajectory :: B.ByteString
defaultExperimentTrajectory = "{\"schema\":1,\"before_unload\":[{\"phase\":\"experiment-menu\",\"observe\":true,\"capture\":\"experiment-menu\",\"steps\":[[\"menu\",0,0,0],[\"menu\",0,0,0],[\"down\",0,0,0],[\"up\",0,0,0],[\"right\",0,0,0],[\"left\",0,0,0]]},{\"phase\":\"experiment-back\",\"observe\":true,\"capture\":\"experiment-back\",\"steps\":[[\"back\",0,0,0]]}],\"after_unload\":[{\"phase\":\"experiment-unloaded\",\"observe\":true,\"capture\":\"experiment-unloaded\",\"steps\":[]}]}"

seconds :: Double -> String
seconds value = showFFloat (Just 1) value " s"

runSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runSession settings python runtime installedMedia adjustmentNand plan e2e = do
  lifecycle <- case [action | segment <- planBefore plan ++ planAfter plan, Just action <- [segmentSaveLifecycle segment]] of
    [] -> pure Nothing
    [action] -> do
      require (not (planUnload plan) && null (planAfter plan)
        && case reverse (planBefore plan) of segment:_ -> segmentSaveLifecycle segment == Just action; [] -> False)
        "save lifecycle requires one terminal before-unload segment selected without unload"
      require (planProtection plan == Unprotected && null (planForbiddenExecution plan) && planExpect plan == ExpectClean)
        "save lifecycle prefix does not accept separate preservation, write-watch or fetch-exclusion claims"
      require (case emulationRuntime settings of FujiModRuntime _ -> True; _ -> False)
        "save lifecycle requires the FujiMod protocol2 observer"
      require (case lifecycleRole action of
        RetainedLifecycle -> payloadKind (e2ePayload settings) `elem` [RecipeBankD3,RecipeBankModal75]
        OwnedLifecycle -> payloadKind (e2ePayload settings) `elem` [RecipeBankLifecycle,RecipeBankModalLifecycle])
        "save lifecycle program disagrees with the selected publication group"
      image5 <- B.readFile (lifecycleImage5 action)
      image6 <- B.readFile (lifecycleImage6 action)
      source <- either (ioError . userError . show) pure (SaveLifecycle.deriveSource image5 image6)
      pure (Just (action,source))
    _ -> ioError (userError "save lifecycle permits one terminal capture interval")
  phases <- newIORef []
  require (null (planForbiddenExecution plan) || case emulationRuntime settings of FujiModRuntime _ -> True; _ -> False)
    "forbidden execution requires the FujiMod fixed-view observer"
  case (planProtection plan, emulationRuntime settings) of
    (PreservationProtection _, FujiModRuntime _) -> pure ()
    (PreservationProtection _, RetainedRuntime) ->
      ioError (userError "the RAM-byte-preservation guard requires the FujiMod bridge")
    _ -> pure ()
  procedure <- case emulationRuntime settings of
    RetainedRuntime -> Just . procedureHex <$> hostProcedure runtime procedureSourcesDigest
    FujiModRuntime _ -> pure Nothing
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      selected = e2ePayload settings
      experimental = payloadKind selected == Experiment
      scoped = payloadKind selected `elem` [RecipeBankModal75, RecipeBankModalLifecycle]
      unreviewedBank = payloadKind selected `elem` [RecipeBankModal75, RecipeBankLifecycle, RecipeBankModalLifecycle]
      relinkAt base action
        | experimental = do
            relinker <- runfilesRelinker selected
            withRelinkedBuildUsing relinker EnforceAllowlist base action
        | otherwise = case relinkMode settings of
            ReviewedRelink -> withPinnedReviewedBuild (payloadContract selected) selected base action
            UnreviewedRelink policy -> withUnreviewedBuild selected policy base action
      pushWith retained
        | experimental = runRamPushWith xm1PeerCacheAvailability (retainedAllocation retained)
        | otherwise = case relinkMode settings of
            ReviewedRelink -> runRamPush (payloadContract selected) retained
            UnreviewedRelink _ -> runUnreviewedRamPush retained
      unloadWith
        | experimental = runRamUnloadWith xm1PeerCacheAvailability
        | otherwise = case relinkMode settings of
            ReviewedRelink -> runRamUnload (payloadContract selected)
            UnreviewedRelink _ -> runUnreviewedRamUnload
      fetchSites = if carriesClassicChrome selected then classicChromeFetchSites else []
      relinkFiles base = relinkAt base $ \pinned -> Relinked (pinnedDocument pinned) (pinnedRuntime pinned)
        <$> B.readFile (pinnedDirectory pinned </> "kernel.syms")
        <*> B.readFile (pinnedDirectory pinned </> "kernel.disasm")
        <*> pure (pinnedManifest pinned)
        <*> pure (pinnedAllowlist pinned)
        <*> (if experimental then Just <$> B.readFile (pinnedDirectory pinned </> "experiment.json") else pure Nothing)
        <*> (if scoped then Just <$> (RelinkedModal
          <$> B.readFile (pinnedDirectory pinned </> "modal-scope.json")
          <*> B.readFile (pinnedDirectory pinned </> "kernel.elf")) else pure Nothing)
        <*> (if isJust (payloadBankVariant selected)
          then Just <$> B.readFile (pinnedDirectory pinned </> "bank-allowlist-input.json")
          else pure Nothing)
        <*> (if payloadKind selected `elem` [RecipeBankLifecycle,RecipeBankModalLifecycle]
          then Just <$> B.readFile (pinnedDirectory pinned </> "save-lifecycle-program.json")
          else pure Nothing)
      choose candidates
        | not (planFromInstall plan) || bootMode settings == FreshBoot = pure (Nothing, Nothing)
        | otherwise = do
            built <- mapM (\base -> (,) base <$> relinkFiles (Address base)) (nub (map installBase candidates))
            let matches =
                  [ (candidate, relinked)
                  | candidate <- candidates
                  , Just relinked <- [lookup (installBase candidate) built]
                  , manifestDigest (relinkedManifest relinked) == installManifestDigest candidate
                  , runtimeDigest (relinkedManifest relinked) == installRuntimeDigest candidate
                  , installProcedureDigest candidate == procedure ]
            pure (case matches of
              (candidate, relinked) : _ -> (Just candidate, Just (candidate, relinked))
              [] -> (Nothing, Nothing))
  when scoped $
    require (any (not . null . segmentSteps) (planBefore plan))
      "scoped modal observation requires selected post-install physical-key segments"
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) e2e
  published <- bracket (timed "emulator-start" (startEmulator executable runtime arguments choose)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, installed) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; relink: " ++ T.unpack (relinkModeText (relinkMode settings))
      ++ "; payload: " ++ T.unpack (payloadText selected))
    let api = emulatorUsbApi emulator
        allocationDirectory = e2e </> "alloc"
        relinkDirectory = e2e </> "relinked"
        pushDirectory = e2e </> "push"
        unloadDirectory = e2e </> "unload"
        allocationBytes = payloadAllocationSize selected
        heapBreak = (\bytes -> hexAddress (leWord bytes 0)) <$> emulatorReadMemory emulator 0x00829a08 4
        writeRelinked relinked = do
          privateDirectory relinkDirectory
          writePrivateBytes (relinkDirectory </> "runtime.bin") (relinkedRuntime relinked)
          writePrivateBytes (relinkDirectory </> "ram-push-manifest.json") (relinkedDocument relinked)
          writePrivateBytes (relinkDirectory </> "kernel.syms") (relinkedSyms relinked)
          writePrivateBytes (relinkDirectory </> "kernel.disasm") (relinkedDisasm relinked)
          forM_ (relinkedExperiment relinked) (writePrivateBytes (relinkDirectory </> "experiment.json"))
          forM_ (relinkedModal relinked) $ \modal -> do
            writePrivateBytes (relinkDirectory </> "modal-scope.json") (relinkedModalScope modal)
            writePrivateBytes (relinkDirectory </> "kernel.elf") (relinkedModalElf modal)
          forM_ (relinkedAllowlistInput relinked) (writePrivateBytes (relinkDirectory </> "bank-allowlist-input.json"))
          forM_ (relinkedLifecycle relinked) (writePrivateBytes (relinkDirectory </> "save-lifecycle-program.json"))
          let allowlist = allowlistReceipt (relinkedAllowlist relinked)
          writePrivateBytes (relinkDirectory </> "bank-allowlist.json") (BL.toStrict (encode allowlist) <> "\n")
          pure allowlist
    setFileMode e2e 0o700
    ensurePrivateDirectory e2e
    privateDirectory allocationDirectory
    restoredHost <- forM installed $ \(candidate, _) ->
      either (ioError . userError . ("invalid install checkpoint host record: " ++)) pure
        (parseEither parseJSON (installHost candidate))
    (retained, breakBefore, breakAfter, allowlist, fetchStart) <- case (installed, restoredHost) of
      (Just (_, relinked), Just host) -> do
        let path = allocationDirectory </> "ram-alloc-state.json"
        writePrivateBytes path (hostAllocationState host)
        retained <- readRetainedAllocation path
        allowlist <- writeRelinked relinked
        pure (retained, hostBreakBefore host, hostBreakAfter host, allowlist, Nothing)
      _ -> do
        fetchStart <- if null fetchSites then pure Nothing else do
          emulatorTraceSites emulator fetchSites
          Just . fst <$> emulatorTraceHits emulator 0
        breakBefore <- heapBreak
        allocationPath <- timed "ram-alloc" (stagedSession allocationDirectory
          ((if unreviewedBank then runRamAllocWith xm1PeerCacheAvailability else runRamAlloc)
            allocationBytes realClock api)) >>= \outcome -> case outcome of
          RamAllocated path -> pure (allocationDirectory </> takeFileName path)
          other -> ioError (userError ("ram-alloc failed: " ++ show other))
        retained <- readRetainedAllocation allocationPath
        breakAfter <- heapBreak
        allowlist <- timed "relink" (relinkFiles (allocBase (retainedAllocation retained))) >>= writeRelinked
        pure (retained, breakBefore, breakAfter, allowlist, fetchStart)
    let allocation = retainedAllocation retained
        allocationPath = retainedStatePath retained
        Address base = allocBase allocation
    manifest <- readRamPushManifest (relinkDirectory </> "ram-push-manifest.json")
    modalObservation <- if scoped then do
      metadata <- B.readFile (relinkDirectory </> "modal-scope.json")
        >>= either (ioError . userError . ("invalid modal scope metadata: " ++)) pure . eitherDecodeStrict'
      runtimeBytes <- B.readFile (relinkDirectory </> "runtime.bin")
      elfBytes <- B.readFile (relinkDirectory </> "kernel.elf")
      source <- either (ioError . userError) pure (parseScopedModalSource (watchAddress base) runtimeBytes elfBytes metadata)
      (stock, stockSource) <- loadModalStock
      installedDigest <- sha256File (runtime </> installedMedia)
      request <- either (ioError . userError) pure (scopedModalRequest stock (either error id (modalLimit 65536)) source)
      pure (Just (request, object ["stock" .= stockSource, "linked" .= scopedModalSourceJson source
        , "installed_dat_sha256" .= sha256Hex installedDigest]))
      else pure Nothing
    rendezvous <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_rendezvous_locked"
    cpu0Exit <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_cpu0_window_exit"
    cpu1Exit <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_cpu1_window_exit"
    mailbox <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_mailbox"
    let symbolsPath = relinkDirectory </> "kernel.syms"
    symbolTable <- parseSymbolTable <$> B.readFile symbolsPath
    let resolveName name = either (ioError . userError) pure (resolveSymbol symbolTable name)
        resolveAddress (FixedAddress value) = pure value
        resolveAddress (RuntimeSymbol name offset) = do
          symbolBase <- resolveName name
          let address = toInteger symbolBase + toInteger offset
          require (address <= 0xffffffff) ("read address overflows 32 bits: " ++ name)
          pure (fromIntegral address)
    declaredTraceSites <- resolveTraceSites resolveName (planTraceSites plan)
    symbolsDigest <- sha256File symbolsPath
    exported <- if experimental
      then do
        metadata <- B.readFile (relinkDirectory </> "experiment.json")
        ExperimentMetadata symbols <- either (ioError . userError . ("invalid experiment.json: " ++)) pure (eitherDecodeStrict' metadata)
        forM (experimentReads settings) $ \(SymbolRead name@(ExportedSymbol text) (ReadLength count)) -> do
          address <- maybe (ioError (userError ("experiment exports no symbol " ++ T.unpack text))) pure (lookup name symbols)
          require (toInteger (unAddress address) + toInteger count <= 0x100000000) "--read window overflows the guest address space"
          pure (text, address, count)
      else pure []
    let readAddress = resolveAddress
    let readMailbox = mailboxReport <$> emulatorReadMemory emulator mailbox 104
        stop phase message = do
          report <- readMailbox
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message, "mailbox" .= report
            , "relink" .= relinkModeText (relinkMode settings)
            , "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    let entries = manifestEntries manifest
    emulatorTraceSites emulator $
      [ (unAddress (installEntry entries), "uploaded-installer")
      , (unAddress (rollbackEntry entries), "uploaded-rollback")
      , (unAddress (statusEntry entries), "uploaded-status")
      , (rendezvous, "rendezvous-locked")
      , (cpu0Exit, "cpu0-window-exit")
      , (cpu1Exit, "cpu1-window-exit")
      , (menuMessageDrawer, menuMessageSite)
      ] ++ [(payloadInstallerStart selected base - 0x244, d3HookSite) | payloadKind selected == RecipeBankD3] ++ fetchSites ++ declaredTraceSites
    privateDirectory pushDirectory
    (statePath, installMailbox) <- case restoredHost of
      Just host -> do
        let path = pushDirectory </> "ram-push-state.json"
        writePrivateBytes path (hostPushState host)
        pure (path, hostInstallMailbox host)
      Nothing -> do
        statePath <- timed "ram-push" (stagedSession pushDirectory (pushWith retained manifest realClock api)) >>= \outcome -> case outcome of
          RamPushInstalled path -> pure (pushDirectory </> takeFileName path)
          other -> stop "ram-push" (show other)
        installMailbox <- readMailbox
        forM_ fetchStart $ \from -> do
          (_, hits) <- emulatorTraceHits emulator from
          let fetched = [hit | hit <- hits, traceSite hit `elem` map snd fetchSites]
          writePrivateBytes (e2e </> "e2e-fetch-watch.json") (BL.toStrict (encode (object
            [ "scope" .= String "Classic Chrome entry words and hook sites, attach checkpoint to the end of ram-push"
            , "sites" .= object [K.fromString label .= hexAddress address | (address, label) <- fetchSites]
            , "hits" .= [object ["site" .= traceSite hit, "r0" .= hexAddress (traceR0 hit)] | hit <- fetched]
            , "fetched" .= not (null fetched) ])) <> "\n")
          putStrLn ("fetch watch: " ++ show (length fetched) ++ " entries into Classic Chrome sites from attach to install")
        host <- InstallHost <$> B.readFile allocationPath <*> B.readFile statePath
          <*> pure installMailbox <*> pure breakBefore <*> pure breakAfter
        saved <- timed "install-checkpoint" (emulatorSaveInstallCheckpoint emulator base
          (manifestDigest manifest) (runtimeDigest manifest) procedure (installHostValue host))
        putStrLn ("install checkpoint: " ++ C.unpack (BL.toStrict (encode saved)))
        pure (statePath, installMailbox)
    case lifecycle of
      Nothing -> do
        state <- readRamPushState statePath
        runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress (\phase message -> stop phase message) plan
        let segments = runSegments runner
            protection = planProtection plan
            expect = planExpect plan
            limit = either error id (watchLimit 4096)
            cardless = emulationCardMode settings == Cardless
            expectedPresent = not cardless
            scopeText = if cardless then "cardless" else "card"
            purpose = if expect == ExpectRefused then FaultControlRun else CandidateRun
            forbidden = planForbiddenExecution plan
            executionGuarded = not (null forbidden)
            protected = case protection of
              WatchProtection ranges -> ranges
              PreservationProtection ranges -> preservationRangeList ranges
              Unprotected -> []
            writePolicy = case protection of
              PreservationProtection _ -> PreserveProtectedBytes
              _ -> ForbidProtectedWrites
            watched = unionWatchRanges (protected ++ forbidden)
        executionAck <- if executionGuarded
          then Just <$> rawWatch (emulatorExecutionWatchBegin emulator protected forbidden limit)
          else pure Nothing
        executionEvidence <- newIORef Nothing
        preUnloadEvidence <- newIORef Nothing
        let snapshotExecution interval runPurpose controlPc final trajectoryProblem = when executionGuarded $ do
              rawState <- rawWatch (emulatorWatchStateRaw emulator)
              preUnload <- readIORef preUnloadEvidence
              let observerResult = do
                    ack <- maybe (Left "missing execution acknowledgement") id executionAck >>= parseFixedWatchAck
                    observation <- rawState >>= parseFixedWatchObservation limit
                    pure (ack, observation)
                  writeResult = do
                    (ack, observation) <- observerResult
                    checkExecutionWrites writePolicy limit protected forbidden ack observation
                  gateResult = do
                    (ack, observation) <- observerResult
                    checkExecution writePolicy limit protected forbidden ack observation
                  checked = do
                    gate <- gateResult
                    when final $ do
                      judgeExecution runPurpose controlPc (Right gate)
                      forM_ trajectoryProblem Left
                    pure gate
                  fetches = case gateResult of Right (ExecutionRejected values) -> values; _ -> []
                  evidence = object
                    [ "schema" .= (1 :: Int)
                    , "enforcement" .= String "original-body-fetch-exclusion"
                    , "ranges" .= [[lo, hi] | range <- forbidden, let (lo, hi) = watchRangeBounds range]
                    , "protected_write_ranges" .= [[lo, hi] | range <- protected, let (lo, hi) = watchRangeBounds range]
                    , "write_policy" .= writePolicyLabel writePolicy
                    , "write_observation" .= either (const Null) executionWriteGateJson writeResult
                    , "watched_ranges" .= [[lo, hi] | range <- watched, let (lo, hi) = watchRangeBounds range]
                    , "interval" .= executionIntervalLabel interval
                    , "start" .= String "successful-install"
                    , "end" .= (if final then executionIntervalLabel interval else "armed")
                    , "cores" .= ([0, 1] :: [Int])
                    , "views" .= String "fixed"
                    , "limit" .= watchLimitValue limit
                    , "acknowledgement" .= maybe Null (either (const Null) id) executionAck
                    , "acknowledgement_error" .= (executionAck >>= either Just (const Nothing))
                    , "final_state" .= either (const Null) id rawState
                    , "state_error" .= either Just (const Nothing) rawState
                    , "complete" .= (final && either (const False) (const True) checked)
                    , "forbidden_fetches" .= map forbiddenFetchJson fetches
                    , "forbidden_fetch_count" .= length fetches
                    , "control_pc" .= fmap watchAddressValue controlPc
                    , "expect" .= (case runPurpose of CandidateRun -> String "clean"; FaultControlRun -> String "refused")
                    , "pre_unload" .= (if interval == ThroughAfterUnload then preUnload else Nothing)
                    , "trajectory_failure" .= trajectoryProblem
                    , "problem" .= either Just (const Nothing) checked ]
                  evidencePath
                    | not final = "e2e-execution-watch-arm.json"
                    | interval == BeforeUnload && planExecutionInterval plan == ThroughAfterUnload = "e2e-execution-watch-before-unload.json"
                    | otherwise = "e2e-execution-watch.json"
              writePrivateBytes (e2e </> evidencePath) (BL.toStrict (encode evidence) <> "\n")
              either (stop "forbidden-execution") (const (pure ())) checked
              when final $ do
                either (stop "forbidden-execution-receipt") (const (pure ()))
                  (checkExecutionReceipt writePolicy limit protected forbidden interval runPurpose controlPc evidence)
                writeIORef executionEvidence (Just evidence)
                when (interval == BeforeUnload) (writeIORef preUnloadEvidence (Just evidence))
        snapshotExecution BeforeUnload CandidateRun Nothing False Nothing
        mediaBefore <- if executionGuarded || protection /= Unprotected
          then emulatorMediaState emulator
          else pure (Left "no media watch")
        watchAck <- case protection of
          WatchProtection ranges | not executionGuarded -> Just <$> emulatorWatchBegin emulator ranges limit
          _ -> pure Nothing
        preservationAck <- case protection of
          PreservationProtection ranges | not executionGuarded -> Just <$> emulatorPreservationBegin emulator ranges
          _ -> pure Nothing
        trajectoryOutcome <- try $ do
          captured <- case modalObservation of
            Nothing -> segments (planBefore plan)
            Just (request, source) -> observeModalInterval emulator e2e request source InstalledScopedRoute (segments (planBefore plan))
          windows <- forM exported $ \(name, Address address, count) -> do
            bytes <- emulatorReadMemory emulator address count
            putStrLn ("read " ++ T.unpack name ++ " " ++ hexAddress address ++ " (" ++ show count ++ " bytes): " ++ hexBytes bytes)
            pure (object ["symbol" .= name, "address" .= hexAddress address, "length" .= count, "bytes" .= hexBytes bytes])
          pure (captured, windows)
        (capturedBefore, symbolReads, trajectoryFailure) <- case trajectoryOutcome of
          Right (captured, windows) -> pure (captured, windows, Nothing)
          Left (failure :: SomeException)
            | experimental || executionGuarded -> pure ([], [], Just (displayException failure))
            | otherwise -> throwIO failure
        case planExecutionInterval plan of
          BeforeUnload -> snapshotExecution BeforeUnload purpose (planExecutionControlPc plan) True trajectoryFailure
          ThroughAfterUnload -> snapshotExecution BeforeUnload CandidateRun Nothing True trajectoryFailure
        (unloadMailbox, capturedAfter) <- (do
          unloadMailbox <- if planUnload plan
            then do
              privateDirectory unloadDirectory
              unload <- timed "ram-unload" (stagedSession unloadDirectory (unloadWith state realClock api))
              unless (unload == RamUnloaded) (stop "ram-unload" (show unload))
              Just <$> readMailbox
            else pure Nothing
          capturedAfter <- segments (planAfter plan)
          pure (unloadMailbox, capturedAfter))
          `catch` \(failure :: SomeException) -> do
            writePrivateBytes (e2e </> "e2e-lifecycle-failure.json")
              (BL.toStrict (encode (object ["failure" .= displayException failure])) <> "\n")
            when (planExecutionInterval plan == ThroughAfterUnload)
              (snapshotExecution ThroughAfterUnload purpose (planExecutionControlPc plan) True (Just (displayException failure))
                `catch` \(_ :: SomeException) -> pure ())
            throwIO failure
        when (planExecutionInterval plan == ThroughAfterUnload)
          (snapshotExecution ThroughAfterUnload purpose (planExecutionControlPc plan) True trajectoryFailure)
        (judgement, protectionFields, mediaAfter) <- case (executionGuarded, protection) of
          (True, _) -> do
            rawState <- rawWatch (emulatorWatchStateRaw emulator)
            mediaAfterValue <- emulatorMediaState emulator
            let writeResult = do
                  ack <- maybe (Left "missing execution acknowledgement") id executionAck >>= parseFixedWatchAck
                  observation <- rawState >>= parseFixedWatchObservation limit
                  checkExecutionWrites writePolicy limit protected forbidden ack observation
                checked = do
                  _ <- judgeExecutionWrites writeResult
                  before <- mediaBefore
                  after <- mediaAfterValue
                  checkMedia expectedPresent before after
                evidence = object
                  [ "enforcement" .= String "fixed-classified-protected-memory"
                  , "write_policy" .= writePolicyLabel writePolicy
                  , "ranges" .= [[lo, hi] | range <- protected, let (lo, hi) = watchRangeBounds range]
                  , "start" .= String "successful-install"
                  , "end" .= (if planUnload plan then String "after-unload" else String "before-unload")
                  , "acknowledgement" .= maybe Null (either (const Null) id) executionAck
                  , "acknowledgement_error" .= (executionAck >>= either Just (const Nothing))
                  , "final_state" .= either (const Null) id rawState
                  , "state_error" .= either Just (const Nothing) rawState
                  , "write_observation" .= either (const Null) executionWriteGateJson writeResult
                  , "media_before" .= either (const Null) mediaJson mediaBefore
                  , "media_after" .= either (const Null) mediaJson mediaAfterValue
                  , "problem" .= either Just (const Nothing) checked ]
            writePrivateBytes (e2e </> "e2e-classified-protected-memory.json") (BL.toStrict (encode evidence) <> "\n")
            either (stop "classified-protected-memory") pure checked
            pure (CandidateQualified, ["classified_protected_memory" .= evidence], mediaAfterValue)
          (False, Unprotected) -> pure (CandidateQualified, [], Left "no media watch")
          (False, WatchProtection ranges) -> do
            mediaAfterValue <- emulatorMediaState emulator
            observation <- emulatorWatchObservation emulator limit
            let gateResult = do
                  ack <- maybe (Left "no watch acknowledgement") Right watchAck
                  observationValue <- observation
                  checkWatch limit ranges ack observationValue
            let checked = do
                  gate <- gateResult
                  before <- mediaBefore
                  after <- mediaAfterValue
                  checkMedia expectedPresent before after
                  judgement <- judgeWatch purpose (Right gate)
                  pure (gate, judgement)
            (gate, judgement) <- either (stop "protected-writes") pure checked
            let summary = case gate of
                  WatchRejectedProtectedWrites value -> value
                  WatchAcceptedClean -> noProtectedWrites
            pure (judgement,
              [ "protected_writes" .= object
                  [ "ranges" .= [ [lo, hi] | range <- ranges, let (lo, hi) = watchRangeBounds range ]
                  , "expect" .= T.unpack (expectLabel expect)
                  , "cpu_writes" .= protectedCpuWritesValue (protectedCpuWrites summary)
                  , "host_writes" .= protectedHostWritesValue (protectedHostWrites summary)
                  , "bus_writes" .= protectedBusWritesValue (protectedBusWrites summary)
                  , "enforced" .= True ] ],
              mediaAfterValue)
          (False, PreservationProtection ranges) -> do
            mediaAfterValue <- emulatorMediaState emulator
            observationResult <- emulatorPreservationState emulator
            let ackValue = maybe (Left "no preservation acknowledgement") Right preservationAck
                checked = do
                  ack <- ackValue
                  observation <- observationResult
                  judgement <- judgePreservation purpose (checkPreservation ranges ack observation)
                  before <- mediaBefore
                  after <- mediaAfterValue
                  checkMedia expectedPresent before after
                  pure (observation, judgement)
                verdict :: String
                verdict = case checked of
                  Left _ -> "failed"
                  Right (_, CandidateQualified) -> "preserved"
                  Right (_, ControlRefusedProtectedWrite) -> "refused"
                evidence = object
                  [ "enforcement" .= ("ram-byte-preservation" :: T.Text)
                  , "ranges" .= preservationRangesJson ranges
                  , "counters" .= either (const Null) (preservationCountersJson . preservationCounters) observationResult
                  , "first_violation" .= either (const Null) (maybe Null preservationViolationJson . preservationFirstViolation) observationResult
                  , "verdict" .= verdict
                  , "problem" .= either Just (const Nothing) checked ]
            writePrivateBytes (e2e </> "e2e-preservation.json") (BL.toStrict (encode evidence) <> "\n")
            (observation, judgement) <- either (stop "ram-byte-preservation") pure checked
            pure (judgement,
              ["ram_preservation" .= preservationReceipt ranges observation judgement],
              mediaAfterValue)
        observed <- readIORef (runnerObservations runner)
        phaseTimes <- readIORef phases
        executionReceipt <- readIORef executionEvidence
        let result = object $
              [ "schema" .= (1 :: Int)
              , "scope" .= String "emulated X-M1 RC0 ordinary-boot RAM push"
              , "plan" .= planName plan
              , "payload" .= payloadText selected
              , "relink" .= object ["mode" .= relinkModeText (relinkMode settings),
                                    "source_identity" .= object [K.fromString name .= digest | (name, digest) <- sourceIdentity manifest],
                                    "allowlist" .= allowlist]
              , "emulator_start" .= startRecord start
              , "attach_hold" .= attachHoldText (attachHold settings)
              , "phases" .= (phaseTimes :: [Value])
              , "allocation" .= object ["base" .= hexAddress base, "bytes" .= allocationBytes,
                                         "cpu0" .= hexAddress (allocCpu0 allocation),
                                         "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter]
              , "runtime" .= object ["bytes" .= allocationSize manifest,
                                      "publications" .= length (manifestPublications manifest),
                                      "manifest" .= manifestPath manifest,
                                      "symbols_sha256" .= sha256Hex symbolsDigest]
              , "declared_trace_sites" .= traceSiteValues declaredTraceSites
              , "states" .= object ["allocation" .= allocationPath, "installed" .= statePath]
              , "card" .= cardRecord (emulationCardMode settings)
              , "menu_receipts" .= catMaybes (capturedBefore ++ capturedAfter)
              , "trajectories" .= map (\name -> object ["name" .= name, "result" .= String "matched"])
                  (nub (map segmentPhase (filter (not . segmentObserve) (planBefore plan ++ planAfter plan))))
              , "observations" .= (observed :: [Value])
              , "reads" .= symbolReads
              , "checks" .= object ["install" .= String "PASS", "trajectory" .= maybe (String "PASS") (const (String "FAIL")) trajectoryFailure, "unload" .= (if planUnload plan then String "PASS" else String "NOT_RUN")]
              , "trajectory_failure" .= trajectoryFailure
              , "mailbox" .= object ["install" .= installMailbox, "rollback" .= unloadMailbox,
                                     "address" .= hexAddress mailbox]
              , "outcome" .= (if scoped then String "unqualified-scoped-diagnostic"
                  else if executionGuarded && expect == ExpectRefused then String "control-refused"
                  else if unreviewedBank then String "unqualified-lifecycle-variant" else toJSON judgement)
              , "forbidden_execution" .= executionReceipt
              , "media" .= object
                  [ "scope" .= String scopeText
                  , "card" .= cardRecord (emulationCardMode settings)
                  , "before" .= either (const Null) mediaJson mediaBefore
                  , "after" .= either (const Null) mediaJson mediaAfter ]
              , "unload" .= (if planUnload plan
                  then Just (String (T.pack (show (length (manifestPublications manifest)) ++ " old words verified; allocation retained until power-off")))
                  else Nothing)
              ] ++ protectionFields ++
              [ "modal_observation" .= object
                  [ "receipt" .= (e2e </> "e2e-modal-observation.json")
                  , "interval" .= String "post-install selected physical-key segments only"
                  , "physical_fetch_coverage" .= False
                  , "task_identity_verified" .= False
                  , "whole_installed_lifetime_coverage" .= False
                  , "rollback_requested" .= planUnload plan
                  ]
              | scoped ]
        forM_ trajectoryFailure $ \problem -> do
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode result) <> "\n")
          ioError (userError problem)
        if judgement == ControlRefusedProtectedWrite || (executionGuarded && expect == ExpectRefused)
          then do
            stale <- doesFileExist (e2e </> "e2e-result.json")
            when stale (stop "protected-writes" "control output contains a stale e2e-result.json")
            writePrivateBytes (e2e </> "e2e-control.json") (BL.toStrict (encode result) <> "\n")
            ioError (userError "control refused as expected")
          else writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode result) <> "\n")
        pure (manifestPublications manifest)
      Just (action,source) -> do
        let output = e2e </> "save-lifecycle"
        privateDirectory output
        metadataBytes <- case lifecycleRole action of
          RetainedLifecycle -> pure "{\"kind\":\"retained\"}"
          OwnedLifecycle -> B.readFile (relinkDirectory </> "save-lifecycle-program.json")
        metadata <- either (ioError . userError) pure (eitherDecodeStrict' metadataBytes)
        runtimeBytes <- case lifecycleRole action of
          RetainedLifecycle -> pure B.empty
          OwnedLifecycle -> B.readFile (relinkDirectory </> "runtime.bin")
        program <- either (ioError . userError . show) pure (SaveLifecycle.parseProgram source runtimeBytes metadata)
        writePrivateBytes (output </> "program.json") (metadataBytes <> "\n")
        runner <- newSegmentRunner emulator e2e (\label action' -> timed label action') readAddress (\phase message -> stop phase message) plan
        captured <- case modalObservation of
          Nothing -> runSegments runner (planBefore plan)
          Just (request,modalSource) -> observeModalInterval emulator e2e request modalSource InstalledScopedRoute
            (runSegments runner (planBefore plan))
        windows <- forM exported $ \(name, Address address, count) -> do
          bytes <- emulatorReadMemory emulator address count
          pure (object ["symbol" .= name,"address" .= hexAddress address,"length" .= count,"bytes" .= hexBytes bytes])
        preparation <- prepareLifecycleCapture emulator
        writePrivateBytes (output </> "preparation.json") (BL.toStrict (encode preparation) <> "\n")
        witness <- runSaveLifecycle emulator output source program
        observed <- readIORef (runnerObservations runner)
        let result = object
              [ "schema" .= (1 :: Int)
              , "scope" .= String "emulated X-M1 normal-bank save lifecycle prefix"
              , "plan" .= planName plan, "payload" .= payloadText selected
              , "outcome" .= String "unqualified-lifecycle-prefix"
              , "full_save_qualified" .= False
              , "runtime" .= object ["bytes" .= allocationSize manifest,"publications" .= length (manifestPublications manifest)]
              , "card" .= cardRecord (emulationCardMode settings)
              , "allowlist" .= allowlist
              , "emulator_start" .= startRecord start
              , "mailbox" .= object ["install" .= installMailbox,"address" .= hexAddress mailbox]
              , "checks" .= object ["install" .= String "PASS","full_save" .= String "NOT_QUALIFIED","unload" .= String "NOT_RUN"]
              , "menu_receipts" .= catMaybes captured
              , "observations" .= (observed :: [Value]), "reads" .= windows
              , "save_lifecycle" .= witness ]
        writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode result) <> "\n")
        pure (manifestPublications manifest)
  when (planUnload plan && carriesClassicChrome selected) $
    timed "cold-restart-read" (coldRestartRead settings python runtime installedMedia adjustmentNand published e2e)

-- | A cold boot of the same inputs, then every publication word read against its old word.
coldRestartRead :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> [Publication] -> FilePath -> IO ()
coldRestartRead settings python runtime installedMedia adjustmentNand publications e2e = do
  let directory = e2e </> "cold-restart"
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand FreshBoot directory
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    words' <- forM publications $ \publication -> do
      bytes <- emulatorReadMemory emulator (unAddress (publicationAddress publication)) 4
      let current = leWord bytes 0
      pure (publication, current)
    let mismatched = [p | (p, current) <- words', current /= unWordValue (publicationOld p)]
    writePrivateBytes (directory </> "e2e-cold-restart.json") (BL.toStrict (encode (object
      [ "scope" .= String "fresh boot of the same DAT, NAND and card after ram-unload; every publication word read"
      , "card" .= cardRecord (emulationCardMode settings)
      , "emulator_start" .= startRecord start
      , "words" .= length words'
      , "mismatched" .= [hexAddress (unAddress (publicationAddress p)) | p <- mismatched]
      , "result" .= String (if null mismatched then "all old" else "mismatch") ])) <> "\n")
    putStrLn ("cold restart: " ++ show (length words') ++ " publication words read, " ++ show (length mismatched) ++ " differ from old")
    require (null mismatched) "cold restart read found publication words that are not old"

data CapturedRead = CapturedRead
  { capturedReadPhase :: T.Text
  , capturedReadLabel :: String
  , capturedReadBytes :: B.ByteString
  }

-- | What running segments needs from its session; hits and observations accumulate across calls.
data SegmentRunner = SegmentRunner
  { runnerEmulator :: Emulator
  , runnerReceipt :: FilePath
  , runnerTimed :: T.Text -> IO (Maybe Value) -> IO (Maybe Value)
  , runnerReadAddress :: ReadAddress -> IO Word32
  , runnerStop :: T.Text -> String -> IO ()
  , runnerTraceLabels :: [String]
  , runnerHits :: IORef Int
  , runnerObservations :: IORef [Value]
  , runnerReads :: IORef [CapturedRead]
  }

newSegmentRunner :: Emulator -> FilePath -> (T.Text -> IO (Maybe Value) -> IO (Maybe Value))
  -> (ReadAddress -> IO Word32) -> (T.Text -> String -> IO ()) -> Plan -> IO SegmentRunner
newSegmentRunner emulator receipt timedAction readAddress stopAction plan = do
  (recorded, _) <- emulatorTraceHits emulator 0
  SegmentRunner emulator receipt timedAction readAddress stopAction (map traceSiteLabel (planTraceSites plan))
    <$> newIORef recorded <*> newIORef [] <*> newIORef []

-- | The plan's trajectory-declared sites in the bridge's form.
resolveTraceSites :: (String -> IO Word32) -> [TraceSite] -> IO [(Word32, String)]
resolveTraceSites resolve sites = do
  resolved <- forM sites $ \site -> case traceSiteAddress site of
    FixedAddress address -> pure (address, traceSiteLabel site)
    RuntimeSymbol name offset -> do
      symbolBase <- resolve name
      let address = toInteger symbolBase + toInteger offset
      require (address <= 0xffffffff) ("trace site overflows 32 bits: " ++ traceSiteLabel site)
      pure (fromIntegral address, traceSiteLabel site)
  mapM_ (\(address, label) -> require (address `mod` 4 == 0) ("trace site is not word aligned: " ++ label)) resolved
  let addresses = map fst resolved
  require (length addresses == length (nub addresses)) "trace site addresses are not distinct"
  pure resolved

eightHex :: Word32 -> String
eightHex = drop 2 . hexAddress

traceSiteValues :: [(Word32, String)] -> [Value]
traceSiteValues sites = [object ["label" .= label, "address" .= eightHex address] | (address, label) <- sites]

traceHitValue :: TraceHit -> Value
traceHitValue hit = object
  [ "site" .= traceSite hit, "core" .= traceCore hit, "instructions" .= traceInstructions hit
  , "r0" .= hexAddress (traceR0 hit), "r1" .= hexAddress (traceR1 hit)
  , "r2" .= hexAddress (traceR2 hit), "r3" .= hexAddress (traceR3 hit) ]

-- | Runs each segment and returns its capture receipt.
runSegments :: SegmentRunner -> [Segment] -> IO [Maybe Value]
runSegments runner = mapM $ \segment -> runnerTimed runner ("segment " <> segmentPhase segment) $ do
  let emulator = runnerEmulator runner
      stop = runnerStop runner
  context <- either (ioError . userError) pure (releaseSegment (segmentPhase segment))
  emulatorReleaseContext emulator (Just context)
  (before, _) <- readIORef (runnerHits runner) >>= emulatorTraceHits emulator
  forM_ (segmentModeDial segment) $ \position -> emulatorModeDial emulator position
  observed <- if segmentObserve segment
    then observe emulator (segmentKeyPulse segment) (segmentSteps segment)
    else [] <$ (presses emulator (segmentKeyPulse segment) (segmentSteps segment) `catch` \(failure :: IOException) ->
      stop (segmentPhase segment) (displayException failure))
  captured <- traverse (emulatorCapture emulator) (segmentCapture segment)
  capturedWindows <- mapM (\(label, location, count) -> do
      address <- runnerReadAddress runner location
      bytes <- emulatorReadMemory emulator address count
      pure (CapturedRead (segmentPhase segment) label bytes,
        object ["label" .= label, "address" .= hexAddress address, "bytes" .= hexBytes bytes]))
    (segmentReads segment)
  previousReads <- readIORef (runnerReads runner)
  observationIndex <- length <$> readIORef (runnerObservations runner)
  let windows = map snd capturedWindows
      checks = [(expectedReadLabel expected, Right (expectedReadBytes expected), Nothing) | expected <- segmentReadExpectations segment]
        ++ [(preservedReadLabel preservation, case
              [capturedReadBytes source | source <- previousReads
              , capturedReadPhase source == preservedSourcePhase preservation
              , capturedReadLabel source == preservedSourceRead preservation] of
                [bytes] -> Right bytes
                _ -> Left "read preservation source is missing or ambiguous", Just preservation)
           | preservation <- segmentReadPreservations segment]
  forM_ (zip [0 :: Int ..] checks) $ \(checkIndex, (label, expected, source)) -> do
    let actual = case [capturedReadBytes value | (value, _) <- capturedWindows, capturedReadLabel value == label] of
          [bytes] -> Right bytes
          _ -> Left "read assertion target is missing or ambiguous"
        checked = do
          expectedBytes <- expected
          actualBytes <- actual
          checkReadExpectation (ReadExpectation label expectedBytes) actualBytes
        evidence = object
          [ "phase" .= segmentPhase segment, "read" .= label
          , "source_phase" .= fmap preservedSourcePhase source
          , "source_read" .= fmap preservedSourceRead source
          , "expected" .= either (const Nothing) (Just . hexBytes) expected
          , "actual" .= either (const Nothing) (Just . hexBytes) actual
          , "matched" .= either (const False) (const True) checked
          , "problem" .= either Just (const Nothing) checked ]
    writePrivateBytes (runnerReceipt runner </> ("e2e-read-check-" ++ show observationIndex ++ "-" ++ show checkIndex ++ ".json"))
      (BL.toStrict (encode evidence) <> "\n")
    either (stop (segmentPhase segment)) pure checked
  modifyIORef' (runnerReads runner) (++ map fst capturedWindows)
  film <- traverse (\expected -> do
      -- Ordinary image byte 0x8a stores the film code in bits 4-7.
      bytes <- emulatorReadMemory emulator 0x0096ccee 1
      require (B.length bytes == 1) "film identity read must contain one byte"
      pure (expected, fromIntegral (B.head bytes `shiftR` 4) :: Word32))
    (segmentFilmRow segment)
  (after, hits) <- emulatorTraceHits emulator before
  writeIORef (runnerHits runner) after
  let drawn = sort (nub [traceR0 hit | hit <- hits, traceSite hit == menuMessageSite])
      hookRows = [traceR0 hit | hit <- hits, traceSite hit == d3HookSite]
      ccHits = [traceSite hit | hit <- hits, traceSite hit `elem` map snd classicChromeFetchSites]
      declaredHits = [hit | hit <- hits, traceSite hit `elem` runnerTraceLabels runner]
  when (segmentObserve segment || not (null (segmentReads segment)) || not (null hookRows) || not (null ccHits)
        || not (null declaredHits) || not (null film)) $ do
    previous <- readIORef (runnerObservations runner)
    let current = object ([ "phase" .= segmentPhase segment, "capture" .= segmentCapture segment
                          , "steps" .= observed, "reads" .= windows
                          , "d3_hook_r0" .= map hexAddress hookRows
                          , "classic_chrome_site_entries" .= ccHits
                          , "menu_messages" .= map hexAddress drawn
                          , "trace_hits" .= map traceHitValue declaredHits ]
                          ++ [ "film_identity" .= object
                               ["expected_row" .= fromEnum expected, "expected_stored_code" .= filmStoredCode expected,
                                "stored_code" .= actual]
                             | (expected, actual) <- maybe [] pure film ])
    writeIORef (runnerObservations runner) (previous ++ [current])
    writePrivateBytes (runnerReceipt runner </> ("e2e-observation-" ++ show (length previous) ++ ".json"))
      (BL.toStrict (encode current) <> "\n")
  forM_ film $ \(expected, actual) ->
    unless (actual == filmStoredCode expected) $
      stop (segmentPhase segment) ("film row " ++ show (fromEnum expected)
        ++ " requires stored code " ++ show (filmStoredCode expected) ++ ", got " ++ show actual)
  unless (segmentObserve segment || drawn == sort (nub (segmentMessages segment))) $
    stop (segmentPhase segment) ("menu messages drawn " ++ show (map hexAddress drawn)
      ++ ", expected " ++ show (map hexAddress (segmentMessages segment)))
  emulatorReleaseContext emulator Nothing
  pure captured

-- | The cardless bridge from this binary's runfiles; //emulation:xm1-viewer lists it as data.
runfilesCardlessBridge :: IO FilePath
runfilesCardlessBridge = do
  executable <- getExecutablePath
  let path = (executable ++ ".runfiles") </> "_main" </> "emulation" </> "cardless-ram-push-bridge"
  present <- doesFileExist path
  require present ("cardless RAM-push bridge is missing from this binary's runfiles: " ++ path)
  pure path

-- | Boot or restore the supplied firmware's attach checkpoint and run segments without installation.
runFlashSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runFlashSession settings python runtime installedMedia adjustmentNand plan e2e = do
  when (emulationCardMode settings == Cardless) $
    case emulationRuntime settings of
      FujiModRuntime _ -> pure ()
      RetainedRuntime -> require False "--cardless --flash-candidate requires --fujimod"
  require (planProtection plan == Unprotected) "a protected-memory watch is unsupported on the flash-candidate path"
  require (planExpect plan == ExpectClean) "a refused-watch control is unsupported on the flash-candidate path"
  require (null (planForbiddenExecution plan)) "forbidden execution is unsupported on the flash-candidate path"
  modalSource <- case modalDiagnostic settings of
    NoModalDiagnostic -> pure Nothing
    RetainedFilmModalDiagnostic -> do
      datDigest <- sha256File (runtime </> installedMedia)
      require (sha256Hex datDigest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
        "retained modal diagnostic requires the pinned stock X-M1 1.50 DAT"
      (stock, source) <- loadModalStock
      pure (Just (stock, object ["dat_sha256" .= sha256Hex datDigest, "stock" .= source]))
  phases <- newIORef []
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      noInstall _ = pure (Nothing, ())
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) e2e
  bracket (timed "emulator-start" (startEmulator executable runtime arguments noInstall)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; flash candidate")
    setFileMode e2e 0o700
    ensurePrivateDirectory e2e
    let isRuntime (RuntimeSymbol _ _) = True
        isRuntime _ = False
        readStarts = [readStart | segment <- planBefore plan ++ planAfter plan, (_, readStart, _) <- segmentReads segment]
        needsSymbols = any isRuntime readStarts || any (isRuntime . traceSiteAddress) (planTraceSites plan)
    symbols <- if needsSymbols then Just <$> flashCandidateSymbols (firmwareSource settings) else pure Nothing
    let resolveName name = case symbols of
          Nothing -> ioError (userError ("runtime symbol requested without a symbol table: " ++ name))
          Just table -> either (ioError . userError) pure (resolveSymbol table name)
        resolveAddress (FixedAddress value) = pure value
        resolveAddress (RuntimeSymbol name offset) = do
          symbolBase <- resolveName name
          let address = toInteger symbolBase + toInteger offset
          require (address <= 0xffffffff) ("read address overflows 32 bits: " ++ name)
          pure (fromIntegral address)
    declaredTraceSites <- resolveTraceSites resolveName (planTraceSites plan)
    symbolsDigest <- case symbols of
      Nothing -> pure Nothing
      Just _ -> Just <$> sha256File (takeDirectory (firmwareSource settings) </> "kernel" </> "kernel.syms")
    emulatorTraceSites emulator ([(menuMessageDrawer, menuMessageSite), (d3HookEntry, d3HookSite)] ++ declaredTraceSites)
    let readAddress = resolveAddress
        stop phase message = do
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress stop plan
    captured <- case modalSource of
      Nothing -> runSegments runner (planBefore plan)
      Just (stock, source) -> observeRetainedModal emulator e2e stock source (runSegments runner (planBefore plan))
    observed <- readIORef (runnerObservations runner)
    phaseTimes <- readIORef phases
    writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode (object
      [ "schema" .= (1 :: Int)
      , "scope" .= String "emulated X-M1 flash candidate UI segments"
      , "plan" .= planName plan
      , "emulator_start" .= startRecord start
      , "runtime" .= object ["symbols_sha256" .= fmap sha256Hex symbolsDigest]
      , "declared_trace_sites" .= traceSiteValues declaredTraceSites
      , "outcome" .= (case modalDiagnostic settings of NoModalDiagnostic -> toJSON CandidateQualified; RetainedFilmModalDiagnostic -> String "diagnostic-observed")
      , "phases" .= (phaseTimes :: [Value])
      , "menu_receipts" .= catMaybes captured
      , "trajectories" .= map (\name -> object ["name" .= name, "result" .= String "matched"])
          (nub (map segmentPhase (filter (not . segmentObserve) (planBefore plan))))
      , "observations" .= (observed :: [Value])
      ])) <> "\n")

data ModalObservationInterval = RetainedStockRoute | InstalledScopedRoute

observeRetainedModal :: Emulator -> FilePath -> ModalStock -> Value -> IO a -> IO a
observeRetainedModal emulator receipt stock source =
  observeModalInterval emulator receipt request source RetainedStockRoute
 where
  request = either error id (modalRequest stock Nothing (either error id (modalLimit 65536)))

observeModalInterval :: Emulator -> FilePath -> ModalRequest -> Value -> ModalObservationInterval -> IO a -> IO a
observeModalInterval emulator receipt request source interval action = do
  let limit = modalRequestLimit request
      writerLimit = either error id (watchLimit 65536)
      captureModal call = (Right <$> call) `catch` \(failure :: SomeException) -> pure (Left (displayException failure))
      parsed response = response >>= modalParsedResponse
      rawResponse response = either (const Null) modalRawResponse response
      responseError response = either Just (either Just (const Nothing) . modalParsedResponse) response
  initialPermit <- captureModal (emulatorModalObservationReport emulator limit)
  initialWatch <- rawWatch (emulatorWatchStateRaw emulator)
  let idle = do
        report <- parsed initialPermit
        unless (not (modalReportEnabled report)) (Left "another modal observer is active")
  writerAck <- case idle of
    Left problem -> pure (Left problem)
    Right () -> rawWatch (emulatorExecutionWatchBegin emulator [modalArrayRange] [] writerLimit)
  armedWatch <- rawWatch (emulatorWatchStateRaw emulator)
  let writerReport state = do
        acknowledgement <- writerAck >>= parseFixedWatchAck
        observation <- state >>= parseFixedWatchObservation writerLimit
        summarizeArrayWrites writerLimit [modalArrayRange] [] acknowledgement observation
  begin <- case writerReport armedWatch of
    Left problem -> pure (Left problem)
    Right _ -> captureModal (emulatorModalObservationBegin emulator request)
  route <- try $ do
    either (ioError . userError) (const (pure ())) (parsed begin)
    action
  finalWatch <- rawWatch (emulatorWatchStateRaw emulator)
  beforeEnd <- captureModal (emulatorModalObservationReport emulator limit)
  ended <- case parsed begin of
    Left problem -> pure (Left ("modal observation was not armed: " ++ problem))
    Right _ -> captureModal (emulatorModalObservationEnd emulator limit)
  afterEnd <- captureModal (emulatorModalObservationReport emulator limit)
  let audit = writerReport finalWatch
      checked = do
        _ <- parsed begin
        _ <- audit
        report <- parsed beforeEnd
        forM_ (modalReportFailure report) (Left . T.unpack)
        _ <- parsed ended
        finalReport <- parsed afterEnd
        checkModalEnd finalReport
      routeFailure = either (Just . displayException) (const Nothing) route
      evidence = object
        [ "schema" .= (1 :: Int)
        , "scope" .= String "selected physical-key route, canonical modal points and physical array writer audit"
        , "source" .= source
        , "policy" .= (case interval of RetainedStockRoute -> String "stock-baseline"; InstalledScopedRoute -> String "scoped-owned75")
        , "start" .= (case interval of RetainedStockRoute -> String "before-selected-physical-key-segments"; InstalledScopedRoute -> String "post-install-before-selected-physical-key-segments")
        , "end" .= (case interval of RetainedStockRoute -> String "after-selected-physical-key-segments"; InstalledScopedRoute -> String "after-selected-physical-key-segments-before-rollback")
        , "whole_installed_lifetime_coverage" .= False
        , "physical_fetch_coverage" .= False
        , "task_identity_verified" .= False
        , "same_route_migration_to_core_with_existing_permit_detected" .= False
        , "writer_semantics" .= String "write attempts are audit evidence, not no-write admission"
        , "writer_ranges" .= [let (low, high) = watchRangeBounds modalArrayRange in [low, high]]
        , "writer_limit" .= watchLimitValue writerLimit
        , "permit_limit" .= modalLimitValue limit
        , "watch_before" .= either (const Null) id initialWatch
        , "watch_before_error" .= either Just (const Nothing) initialWatch
        , "watch_acknowledgement" .= either (const Null) id writerAck
        , "watch_acknowledgement_error" .= either Just (const Nothing) writerAck
        , "watch_armed" .= either (const Null) id armedWatch
        , "watch_armed_error" .= either Just (const Nothing) armedWatch
        , "watch_final" .= either (const Null) id finalWatch
        , "watch_final_error" .= either Just (const Nothing) finalWatch
        , "array_write_audit" .= either (const Null) arrayWriteReportJson audit
        , "array_write_audit_error" .= either Just (const Nothing) audit
        , "permit_before" .= rawResponse initialPermit
        , "permit_before_error" .= responseError initialPermit
        , "permit_begin" .= rawResponse begin
        , "permit_begin_error" .= responseError begin
        , "permit_before_end" .= rawResponse beforeEnd
        , "permit_before_end_error" .= responseError beforeEnd
        , "permit_end" .= rawResponse ended
        , "permit_end_error" .= responseError ended
        , "permit_after_end" .= rawResponse afterEnd
        , "permit_after_end_error" .= responseError afterEnd
        , "trajectory_failure" .= routeFailure
        , "observation_error" .= either Just (const Nothing) checked
        ]
  writePrivateBytes (receipt </> "e2e-modal-observation.json") (BL.toStrict (encode evidence) <> "\n")
  case route of
    Left (failure :: SomeException) -> throwIO failure
    Right result -> either (ioError . userError) (const (pure result)) checked

loadModalStock :: IO (ModalStock, Value)
loadModalStock = do
  let imagePath = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"
  imageDigest <- sha256File imagePath
  require (sha256Hex imageDigest == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
    "modal stock source differs from the pinned complete image-5 decode"
  image <- B.readFile imagePath
  stock <- either (ioError . userError) pure (modalStock (B.take 0xfc (B.drop (0x00bfb028 - 0x00bf1a00) image)))
  pure (stock, object ["image5_sha256" .= sha256Hex imageDigest
    , "stock_start" .= (0x00bfb028 :: Word32), "stock_bytes" .= (0xfc :: Int)])
