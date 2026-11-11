{-# LANGUAGE OverloadedStrings #-}
module Xm1RamPushE2E (RelinkMode (..), AllowlistPolicy (..), OverrideReason (..), BootMode (..), PushMode (..), AttachHold (..), UiConversionProfile (..), CardMode (..), EmulationRuntime (..), FujiModBridge (..), ModalDiagnostic (..), ModalLifetimeGuard (..), E2ESettings (..), ModalOwnedPackage (..), loadModalOwnedPackage, SymbolRead, parseSymbolRead, Payload, bankD3Payload, bankModal75Payload, bankModalEnginePayload, bankModalRenderPayload, bankLifecyclePayload, bankModalLifecyclePayload, classicChromePayload, classicChromeIconPayload, experimentPayload, runMenuCrawlE2E, runMenuObserveE2E, runRamDiffE2E, runRamPushE2E, sessionBridge, warmAttachCheckpoint, KeeperSessionId, keeperSessionId, KeeperState (..), KeeperAction (..), keeperTransition, checkPropertyAllocationSource, runPropertyAllocationKeeper, validateAfResetProof, validateAfResetPair, AfArchiveBinding (..), validateAfArchiveBinding) where

import Control.Applicative ((<|>))
import Control.Exception (IOException, SomeException, bracket, catch, displayException, finally, mask, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, toJSON, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseEither)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (find, nub, sort)
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
import System.Directory (canonicalizePath, createDirectory, doesDirectoryExist, doesFileExist, getCurrentDirectory, getFileSize, getTemporaryDirectory, listDirectory, removeDirectory, removeDirectoryRecursive, removeFile, setCurrentDirectory)
import System.Environment (getExecutablePath, lookupEnv)
import System.FilePath (isAbsolute, replaceExtension, splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), IOMode (WriteMode), hClose, hFlush, hSetBuffering, hIsEOF, hGetChar, stdin, stdout, withFile)
import System.Posix.Files (setFileMode)
import System.Posix.Temp (mkdtemp)
import System.Exit (ExitCode (..))
import System.Process (CreateProcess (..), StdStream (..), proc, waitForProcess, withCreateProcess)
import Text.Read (readMaybe)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, hostProcedure, procedureHex, routeKey, routeKeyHex)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import Emulation.SegmentReceipt (SegmentReceipt (..), SegmentResult (..), elapsedSeconds, segmentName, segmentNameText)
import Emulation.Symbols (SymbolTable, parseSymbolTable, resolveSymbol)
import Emulation.PreservationObservation
import Emulation.ModalObservation
import Emulation.ModalWatchStream (compactFinalWatch, readFinalModalWatch)
import Emulation.LiveSettings (describeLiveSettingChange, liveSettingChange, readLiveSettings)
import Emulation.MachineFingerprint (parseMachineFingerprintEnvelope)
import qualified Emulation.CaptureStream as CaptureJournal
import qualified Emulation.SaveLifecycle as SaveLifecycle
import Emulation.SaveLifecycleRun (runSaveLifecycle)
import qualified Emulation.MenuCrawl as Crawl
import qualified Emulation.MenuObserve as MenuObserve
import qualified Emulation.RamDiff as RamDiff
import Emulation.RenderStackDiagnostic
import Emulation.ObservationStream
import Replacement.ModalRuntime.RenderObjectProvenance (ProvenanceBatch)
import Emulation.UiTrajectory
import Emulation.WatchObservation
import Emulation.ViewerRuntime (FixedSceneFrame (..), fixedSceneFrameArguments)
import EmulatorUsb
import FujiAF.RamPush
import FujiAF.Transport (ensurePrivateDirectory, writePrivateBytes)
import Xm1UnreviewedRelink (runUnreviewedRamPush, runUnreviewedRamUnload, withPrelinkedBuild, withUnreviewedBuild, withUnreviewedBuildArchive)
import RamPushReviewedPins (reviewedContract)
import RamPushProcedure (procedureSourcesDigest)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)
import qualified RamPushLayout as Layout
import qualified Emulation.CpuWorkerSource as CpuWorker
import qualified FujiAF.Host as Host
import qualified Replacement.GeneralHeap.Admission as Heap

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
  , modalLifetimeGuard :: ModalLifetimeGuard
  , modalOwnedPackage :: Maybe ModalOwnedPackage
  , prelinkedModalBuild :: Maybe FilePath
  , afOwnershipPlanFile :: Maybe FilePath
  , afOwnershipControl :: Bool
  }

-- | A bank Rust archive built with the owned controller-75 table, and the
-- stock extents its generic actions retire.
data ModalOwnedPackage = ModalOwnedPackage
  { ownedArchivePath :: FilePath
  , ownedArchiveDigest :: String
  , ownedRetiredPath :: FilePath
  , ownedRetiredDigest :: String
  -- | The generated table's identity, which the archive's static table carries.
  , ownedTableIdentity :: String
  , ownedRetired :: [WatchRange]
  }

-- | Reads the archive and the generator's retired-extent document, which must
-- name the pinned image5 and the table identity the archive carries.
loadModalOwnedPackage :: FilePath -> FilePath -> IO ModalOwnedPackage
loadModalOwnedPackage archive retired = do
  archiveBytes <- B.readFile archive
  require (not (B.null archiveBytes) && B.length archiveBytes <= 16 * 1024 * 1024) "owned modal75 archive is empty or exceeds 16 MiB"
  document <- B.readFile retired
  require (B.length document <= 1024 * 1024) "owned modal75 retired document exceeds 1 MiB"
  (table, extents) <- either (ioError . userError . ("invalid owned modal75 retired document: " ++)) pure $ do
    value <- eitherDecodeStrict' document
    flip parseEither value $ withObject "retired document" $ \fields -> do
      schema <- fields .: "schema"
      unless (schema == (2 :: Int)) (fail "schema is not 2")
      image <- fields .: "image5_sha256"
      unless (image == ("3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" :: String)) (fail "image5_sha256 is not the pinned X-M1 1.50 image5")
      table <- fields .: "table_identity"
      unless (length table == 64 && all (`elem` ("0123456789abcdef" :: String)) table) (fail "table_identity is not a SHA-256")
      rows <- fields .: "retired"
      extents <- forM rows $ withObject "retired extent" $ \row -> do
        callback <- row .: "callback"
        low <- row .: "start"
        high <- row .: "end"
        unless (callback == hexAddress low) (fail "retired callback differs from its start")
        either fail pure (watchRange (watchAddress low) (watchAddress high))
      pure (table, extents)
  require (not (null extents)) "owned modal75 retired document names no extent"
  require (hexBytesOf table `B.isInfixOf` archiveBytes) "owned modal75 archive does not carry the retired document's table identity"
  pure (ModalOwnedPackage archive (sha256Hex (sha256 archiveBytes)) retired (sha256Hex (sha256 document)) table extents)
 where
  hexBytesOf digits = B.pack [byte | pair <- pairsOf digits, (byte, "") <- readHex pair]
  pairsOf (high : low : rest) = [high, low] : pairsOf rest
  pairsOf _ = []

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
    when (isJust (afOwnershipPlanFile settings)) $
      require (boot == FreshBoot) "AF prestate proof requires fresh boot"
    require (card /= Cardless || attachHold settings == NoAttachHold) "the cardless RAM-push bridge holds no key across the attach edge"
    require (null (fixedSceneFrame settings)) "the FujiMod RAM-push bridge takes no --fixed-scene-frame"
    pure (fujimodBridge bridge,
      [ "--run", fujimodRunDocument bridge
      , "--boot-rom", fujimodBootRom bridge
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      ] ++ freshArguments ++ ["--af-prestate-proof" | isJust (afOwnershipPlanFile settings)] ++ cardArguments card ++ attachHoldArguments (attachHold settings))
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
  RecipeBankModal75MenuEngine -> "recipe-bank-modal75-menu-engine"
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
  RecipeBankModal75MenuEngine -> error "menu engine bank has no reviewed live contract"
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

-- | Runs a transport session in a private directory under XDG_RUNTIME_DIR, else /dev/shm, else
-- the temporary directory, then copies its files into DIRECTORY, which must exist, whether or
-- not the session succeeded. The transport fsyncs every journal line, which costs tens of
-- milliseconds a transfer on a Linux disk.
stagedSession :: FilePath -> (FilePath -> IO a) -> IO a
stagedSession directory session = do
  runtime <- lookupEnv "XDG_RUNTIME_DIR"
  shm <- doesDirectoryExist "/dev/shm"
  root <- case runtime of
    Just path -> pure path
    Nothing | shm -> pure "/dev/shm"
            | otherwise -> getTemporaryDirectory
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

completeStep :: Emulator -> Maybe KeyPulse -> Step -> IO ([String], MenuState)
completeStep emulator pulse step = case stepCompletion step of
  PacketCompletion -> pressStep emulator (stepKeyTiming pulse step) (stepKey step)
  PresentedFrameCompletion key -> do
    require (pulse == Nothing) "presented-frame cannot override the display-key pulse"
    reply <- emulatorDisplayKey emulator key
    actual <- either (ioError . userError) pure (presentedFrameMenu reply)
    pure ([Crawl.crawlKeyWire key], actual)

press :: Emulator -> Maybe KeyPulse -> Step -> IO ()
press emulator pulse step = do
  forM_ (stepIdle step) (emulatorIdle emulator)
  (_, actual) <- completeStep emulator pulse step
  require (actual == stepExpected step)
    ("menu trajectory after " ++ stepKey step ++ ": expected " ++ show (stepExpected step) ++ ", got " ++ show actual)

presses :: Emulator -> Maybe KeyPulse -> [Step] -> IO ()
presses emulator pulse = mapM_ (press emulator pulse)

-- | Presses every step and records the key presses, expected and actual state and wall seconds
-- without requiring the state.
observe :: Emulator -> Maybe KeyPulse -> [Step] -> IO [Value]
observe emulator pulse = mapM $ \step -> do
  let MenuState em es ec = stepExpected step
  forM_ (stepIdle step) (emulatorIdle emulator)
  started <- getMonotonicTime
  (pressed, MenuState am as ac) <- completeStep emulator pulse step
  ended <- getMonotonicTime
  pure (object [ "key" .= stepKey step, "pressed" .= length pressed
               , "expected" .= [em, es, ec], "actual" .= [am, as, ac]
               , "matched" .= (MenuState am as ac == stepExpected step), "seconds" .= (ended - started) ])

data LensPreparation = FindLensRow | OpenLensSetting | SelectLensOn | CommitLensSetting | CloseLensMenu
  deriving (Show)

prepareLifecycleCapture :: FilePath -> Emulator -> IO Value
prepareLifecycleCapture output emulator =
  bracket (CaptureJournal.openJournal (output </> "preparation.jsonl")) CaptureJournal.closeJournal $ \journal -> do
    keys <- forM [FindLensRow,OpenLensSetting,SelectLensOn,CommitLensSetting,CloseLensMenu] $ \step -> do
      let key = case step of
            FindLensRow -> "cursor:down:20:64"
            OpenLensSetting -> "right"
            SelectLensOn -> "up"
            CommitLensSetting -> "ok"
            CloseLensMenu -> "back"
      (pressed,actual@(MenuState menu setting cursor)) <- pressStep emulator defaultKeyTiming key
      let record = object ["step" .= show step,"pressed" .= pressed,"menu" .= [menu,setting,cursor]]
      CaptureJournal.note journal "menu-step" record
      CaptureJournal.synchronizeJournal journal
      case step of
        FindLensRow -> require (cursor == 20) ("shooting-menu row 20 was not reached: " ++ show actual)
        OpenLensSetting -> require (setting == 0xa3) "SHOOT WITHOUT LENS setting was not opened"
        SelectLensOn -> require (cursor == 0) "SHOOT WITHOUT LENS ON was not selected"
        _ -> pure ()
      pure record
    dialFirst <- emulatorModeDial emulator ModeDialProgram
    first <- emulatorCapture emulator "lifecycle-pre-press-a"
    CaptureJournal.note journal "pre-press-a" (object ["mode_dial" .= dialFirst,"capture" .= first])
    CaptureJournal.synchronizeJournal journal
    dialSecond <- emulatorModeDial emulator ModeDialProgram
    second <- emulatorCapture emulator "lifecycle-pre-press"
    CaptureJournal.note journal "pre-press" (object ["mode_dial" .= dialSecond,"capture" .= second])
    CaptureJournal.synchronizeJournal journal
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
  , relinkedAllowlistProof :: Maybe B.ByteString
  , relinkedAllowlistReport :: Maybe B.ByteString
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

data RuntimeSourceRole = ViewerExecutable | SdkBridgeExecutable | RunDocumentSource | BootRomSource | AdjustmentNandSource
data RuntimeFileBinding = RuntimeFileBinding RuntimeSourceRole FilePath Sha256

runtimeFileBindings :: EmulationRuntime -> FilePath -> IO [RuntimeFileBinding]
runtimeFileBindings runtime adjustment = case runtime of
  RetainedRuntime -> ioError (userError "lifetime source binding requires the FujiMod SDK runtime")
  FujiModRuntime bridge -> forM
    [ViewerExecutable, SdkBridgeExecutable, RunDocumentSource, BootRomSource, AdjustmentNandSource] $ \role -> do
      path <- case role of
        ViewerExecutable -> getExecutablePath
        SdkBridgeExecutable -> pure (fujimodBridge bridge)
        RunDocumentSource -> pure (fujimodRunDocument bridge)
        BootRomSource -> pure (fujimodBootRom bridge)
        AdjustmentNandSource -> pure adjustment
      digest <- sha256File path
      pure (RuntimeFileBinding role path digest)

runtimeFileBindingValue :: RuntimeFileBinding -> Value
runtimeFileBindingValue (RuntimeFileBinding role path digest) = object
  ["role" .= label, "path" .= path, "sha256" .= sha256Hex digest]
  where
    label :: T.Text
    label = case role of
      ViewerExecutable -> "viewer"
      SdkBridgeExecutable -> "bridge"
      RunDocumentSource -> "run_document"
      BootRomSource -> "boot_rom"
      AdjustmentNandSource -> "adjustment_nand"

newtype KeeperSessionId = KeeperSessionId Sha256 deriving (Eq)

keeperSessionId :: String -> Either String KeeperSessionId
keeperSessionId = fmap KeeperSessionId . parseSha256Hex

data HeldAllocationReadback = HeldAllocationReadback B.ByteString Heap.OwnedAllocation

data KeeperState = KeeperHolding | KeeperStopped deriving (Eq, Show)
data KeeperAction = KeeperStatus | KeeperReadback | KeeperStop deriving (Eq, Show)

keeperTransition :: KeeperSessionId -> KeeperState -> String -> Either String (KeeperAction, KeeperState)
keeperTransition (KeeperSessionId session) state line = do
  require_ (state == KeeperHolding) "allocation keeper is stopped"
  require_ (length line <= 160) "allocation keeper command exceeds 160 bytes"
  require_ (all (\character -> character >= ' ' && character <= '~') line) "allocation keeper command is not printable ASCII"
  case words line of
    [command, identity] | identity == sha256Hex session -> case command of
      "status" -> Right (KeeperStatus, KeeperHolding)
      "readback" -> Right (KeeperReadback, KeeperHolding)
      "stop" -> Right (KeeperStop, KeeperStopped)
      _ -> Left "unknown allocation keeper command"
    _ -> Left "allocation keeper command or session identity mismatch"
  where require_ accepted reason = unless accepted (Left reason)

checkPropertyAllocationSource :: Sha256 -> Sha256 -> Sha256 -> Either String ()
checkPropertyAllocationSource digest image4 contract = do
  unless (sha256Hex digest == "cd2f3d7d187ab72151abdb325891bac5b5a3257db8640c6b0c0e836d29b37626")
    (Left "property allocation keeper requires the immutable admitted full02 stage3 DAT")
  unless (sha256Hex image4 == "40d7fe9afdcd067a4441df8a079cef1fb4ee9e18a8240d9abd492a6db545dbdf")
    (Left "property allocation keeper image4 sidecar differs from the admitted full02 pair")
  unless (sha256Hex contract == "1b839ebdb8f7448c211014a619bb004832b5cecc3eac673faf045f8226d841eb")
    (Left "property allocation keeper composition contract differs from the admitted full02 pair")

runPropertyAllocationKeeper :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runPropertyAllocationKeeper settings python installedMedia adjustmentNand image4Path contractPath receipt = do
  require (emulationCardMode settings == Cardless && bootMode settings == FreshBoot
    && pushMode settings == RamPush && segmentSelection settings == Nothing
    && relinkMode settings == ReviewedRelink && payloadKind (e2ePayload settings) == RecipeBankD3
    && modalDiagnostic settings == NoModalDiagnostic && modalLifetimeGuard settings == NoModalLifetimeGuard
    && not (isJust (modalOwnedPackage settings)) && not (isJust (prelinkedModalBuild settings))
    && null (experimentReads settings) && attachHold settings == NoAttachHold)
    "property allocation keeper requires a fresh allocation-only cardless session"
  bridge <- case emulationRuntime settings of
    FujiModRuntime value -> pure value
    _ -> ioError (userError "property allocation keeper requires the pinned FujiMod bridge")
  dat <- sha256File installedMedia
  image4 <- Host.readRegular (16*1024*1024) image4Path
  contract <- Host.readRegular (4*1024*1024) contractPath
  either (ioError . userError) pure (checkPropertyAllocationSource dat (sha256 image4) (sha256 contract))
  workerSource <- either (ioError . userError) pure (CpuWorker.bindCpuWorkerSource image4)
  layout <- either (ioError . userError) pure (Layout.propertyPublicationLayout Layout.modalRenderBankVariant)
  expectedSize <- either (ioError . userError) (pure . fromIntegral) (Layout.propertyAllocationSize layout 0)
  exists <- doesDirectoryExist receipt
  if exists
    then listDirectory receipt >>= \entries -> require (null entries) "allocation keeper receipt must be fresh or empty"
    else privateDirectory receipt
  setFileMode receipt 0o700
  ensurePrivateDirectory receipt
  let allocationDirectory = receipt </> "alloc"
      keeperPath = receipt </> "allocation-keeper.json"
      json path value = writePrivateBytes path (BL.toStrict (encode value) <> "\n")
      inactive = object ["schema" .= (1 :: Int), "state" .= ("stopped" :: T.Text)
        , "live" .= False, "installable" .= False, "total" .= False, "installed_lifetime_admitted" .= False
        , "owned_union_writer_catalog_admitted" .= False]
  privateDirectory allocationDirectory
  runtime <- getCurrentDirectory
  provenance <- map runtimeFileBindingValue <$> runtimeFileBindings (FujiModRuntime bridge) adjustmentNand
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand FreshBoot receipt
  hSetBuffering stdout LineBuffering
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ())))
    (\(emulator, _, _) -> stopEmulator emulator `finally` json keeperPath inactive) $ \(emulator, start, ()) -> do
      require (bootStart start `elem` [ColdStart, FreshBootStart]) "property allocation keeper refuses a restored session"
      emulatorTraceSitesReading emulator [(unAddress bootstrapAddress, "property-allocation-bootstrap",
        [SiteRead 0x40054760 4, SiteRead 0x400545a4 4])]
      (traceStart, _) <- emulatorTraceHits emulator 0
      before <- emulatorReadMemory emulator 0x00829a08 4
      clock <- emulatedClock
      outcome <- stagedSession allocationDirectory
        (runPropertyAllocationWith xm1PeerCacheAvailability layout (CpuWorker.cpuWorkerArena workerSource) clock (emulatorUsbApi emulator))
      allocationPath <- case outcome of
        RamAllocated path -> pure (allocationDirectory </> takeFileName path)
        other -> ioError (userError ("property allocation failed: " ++ show other))
      allocation <- readRamAllocState allocationPath
      require (allocSize allocation == expectedSize) "property allocation state extent differs"
      actualSize <- either (ioError . userError) (pure . fromIntegral)
        (Layout.propertyAllocationSize layout (unAddress (allocBase allocation)))
      require (allocSize allocation == actualSize) "property allocation base leaves the layout domain"
      allocationLayout <- either (ioError . userError . show) pure (Heap.boundOwnedAllocation
        (CpuWorker.cpuWorkerArena workerSource) (Heap.HeapAddress (unAddress (allocBase allocation)))
        (Heap.AllocationBytes (fromIntegral (allocSize allocation))))
      let Heap.HeapAddress headerAddress = Heap.allocationHeaderAddress allocationLayout
      after <- emulatorReadMemory emulator 0x00829a08 4
      (_, hits) <- emulatorTraceHits emulator traceStart
      taskNodes <- forM hits $ \hit -> do
        require (traceSite hit == "property-allocation-bootstrap" && traceCore hit == 0
          && maybe False (\sp -> sp `mod` 8 == 0) (traceSp hit))
          "allocation bootstrap lacks CPU0 task-stack provenance"
        case traceMemory hit of
          [nodeBytes, contextBytes] | B.length nodeBytes == 4 && B.length contextBytes == 4 -> do
            let node = leWord nodeBytes 0
            require (node >= 0x4005ff0c && node < 0x4005ff0c + 56*120
              && (node - 0x4005ff0c) `mod` 56 == 0 && leWord contextBytes 0 == 0)
              "allocation bootstrap lacks a live primary-kernel task context"
            pure node
          _ -> ioError (userError "allocation bootstrap task-context reads unavailable")
      require (length taskNodes == 2 && length (nub taskNodes) == 1)
        "allocation and peer-preparation bootstraps did not run in the same live task"
      let readback = B.concat <$> forM [0,65536..allocSize allocation-1] (\offset ->
            emulatorReadMemory emulator (unAddress (allocBase allocation) + fromIntegral offset)
              (min 65536 (allocSize allocation-offset)))
          checkReadback = do
            header <- emulatorReadMemory emulator headerAddress 16
            owned <- either (ioError . userError . show) pure (Heap.admitOwnedAllocation allocationLayout header)
            bytes <- readback
            require (sha256Hex (sha256 bytes) == allocGuard allocation) "retained property allocation readback changed"
            slot <- emulatorReadMemory emulator (unAddress callbackSlotAddress) 4
            table <- emulatorReadMemory emulator (unAddress callbackTablePointerAddress) 4
            require (leWord slot 0 == unWordValue callbackStockWord && leWord table 0 == unAddress callbackTableAddress)
              "retained allocation callback bindings changed"
            pure (HeldAllocationReadback bytes owned)
      HeldAllocationReadback bytes owned <- checkReadback
      writePrivateBytes (receipt </> "allocation-readback.bin") bytes
      allocationDocument <- B.readFile allocationPath
      nonce <- getMonotonicTime
      let identity = sha256Hex (sha256 (allocationDocument <> C.pack (show nonce) <> BL.toStrict (encode provenance)))
      session <- either (ioError . userError) pure (keeperSessionId identity)
      let held event = object
            [ "schema" .= (1 :: Int), "event" .= (event :: T.Text), "session_id" .= identity
            , "state" .= ("holding" :: T.Text), "live" .= True, "installable" .= False
            , "total" .= False, "installed_lifetime_admitted" .= False, "runtime_delivery_observed" .= False
            , "owned_union_writer_catalog_admitted" .= False
            , "base" .= hexAddress (unAddress (allocBase allocation)), "size" .= allocSize allocation
            , "allocation_state" .= allocationPath, "allocation_state_sha256" .= sha256Hex (sha256 allocationDocument)
            , "cpu_worker_source" .= CpuWorker.cpuWorkerSourceValue workerSource
            , "owned_heap_allocation" .= Heap.ownedAllocationValue owned
            , "image4_sha256" .= sha256Hex (sha256 image4), "composition_contract_sha256" .= sha256Hex (sha256 contract)
            , "readback_sha256" .= allocGuard allocation, "installed_dat_sha256" .= sha256Hex dat
            , "heap_break_before" .= hexBytes before, "heap_break_after" .= hexBytes after
            , "bootstrap_sha256" .= sha256Hex (sha256 bootstrapBytes), "sources" .= provenance
            , "checkpoint" .= startRecord start, "bootstrap_hits" .= map (traceHitValue [] []) hits
            , "lifetime" .= ("valid only while this keeper process and exact emulator session remain holding" :: T.Text) ]
          emit event = C.putStrLn (BL.toStrict (encode (held event))) >> hFlush stdout
          loop state = do
            line <- boundedKeeperLine
            (action, next) <- either (ioError . userError) pure (keeperTransition session state line)
            case action of
              KeeperStop -> pure ()
              _ -> do
                HeldAllocationReadback current _ <- checkReadback
                when (action == KeeperReadback) (writePrivateBytes (receipt </> "allocation-readback.bin") current)
                emit (if action == KeeperStatus then "keeper-status" else "keeper-readback")
                loop next
      json (receipt </> "allocation-provenance.json") (object
        ["schema" .= (1 :: Int), "live_witness" .= False, "recorded_ready_event" .= held "keeper-ready"])
      json keeperPath (held "keeper-ready")
      emit "keeper-ready"
      loop KeeperHolding
  C.putStrLn (BL.toStrict (encode inactive))
  where
    boundedKeeperLine = go []
      where
        go reversed = do
          eof <- hIsEOF stdin
          require (not eof) "allocation keeper stdin closed before authenticated stop"
          character <- hGetChar stdin
          if character == '\n' then pure (reverse reversed) else do
            require (length reversed < 160 && character >= ' ' && character <= '~')
              "allocation keeper input is non-ASCII or exceeds 160 bytes"
            go (character : reversed)

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
  when (afOwnershipControl given) $
    require (isJust (afOwnershipPlanFile given)) "AF ownership control requires an ownership plan"
  when (isJust (afOwnershipPlanFile given)) $
    require (pushMode given == RamPush && modalLifetimeGuard given == ContinuousModalLifetimeGuard
      && bootMode given == FreshBoot && segmentSelection given == Nothing && trajectoryFile /= Nothing)
      "AF ownership requires fresh boot and an explicit continuous installed modal trajectory"
  bytes <- maybe (pure defaultExperimentTrajectory) B.readFile trajectoryFile
  trajectories <- either (ioError . userError . ("invalid RAM-push e2e trajectories: " ++)) pure (decodeTrajectories bytes)
  let lifecycleRequested = any (isJust . segmentSaveLifecycle) (beforeUnload trajectories ++ afterUnload trajectories)
      lifetimeGuarded = modalLifetimeGuard given == ContinuousModalLifetimeGuard
        || payloadKind (e2ePayload given) `elem` [RecipeBankModal75, RecipeBankModalLifecycle, RecipeBankModal75MenuEngine]
  when lifetimeGuarded $ do
    require (pushMode given == RamPush && emulationCardMode given == Cardless
      && case emulationRuntime given of FujiModRuntime _ -> True; _ -> False)
      "modal lifetime guard requires full cardless FujiMod RAM push"
    require (relinkMode given == UnreviewedRelink EnforceAllowlist)
      "modal lifetime guard requires unreviewed relink with the enforced allowlist"
    require (payloadKind (e2ePayload given) `elem` [RecipeBankD3, RecipeBankModal75, RecipeBankModal75MenuEngine])
      "modal lifetime guard supports only the stock bank, the scoped modal bank or the scoped menu-engine bank"
    require (segmentSelection given == Nothing) "modal lifetime guard refuses segment splitting"
    require (modalDiagnostic given == NoModalDiagnostic && not lifecycleRequested)
      "modal lifetime guard refuses other modal or lifecycle observers"
    either (ioError . userError) pure (checkModalObserverConfiguration trajectories)
  when lifecycleRequested $ require (pushMode given == RamPush)
    "save lifecycle observation requires an installed RAM package"
  when (payloadKind (e2ePayload given) `elem` [RecipeBankModal75, RecipeBankLifecycle, RecipeBankModalLifecycle, RecipeBankModal75MenuEngine]) $ do
    require (pushMode given == RamPush
      && (emulationCardMode given == Cardless
        || (payloadKind (e2ePayload given) == RecipeBankLifecycle && lifecycleRequested))
      && case emulationRuntime given of FujiModRuntime _ -> True; _ -> False)
      "unqualified bank variants require FujiMod RAM push; a virtual card requires the lifecycle capture interval"
    require (relinkMode given == UnreviewedRelink EnforceAllowlist)
      "unqualified bank variants require --unreviewed-relink without an allowlist override"
    require (modalDiagnostic given == NoModalDiagnostic) "--modal-observation is a stock no-install diagnostic"
  when (payloadKind (e2ePayload given) `elem` [RecipeBankModal75, RecipeBankModalLifecycle, RecipeBankModal75MenuEngine]) $
    either (ioError . userError) pure (checkModalObserverConfiguration trajectories)
  when (modalDiagnostic given == RetainedFilmModalDiagnostic) $ do
    require (pushMode given == FlashCandidate && emulationCardMode given == Cardless
      && case emulationRuntime given of FujiModRuntime _ -> True; _ -> False)
      "modal observation requires the no-install --cardless --fujimod --flash-candidate route"
    either (ioError . userError) pure (checkModalObserverConfiguration trajectories)
    require (not (null (beforeUnload trajectories)) && null (afterUnload trajectories)
      && all ((== "retained-film-route") . segmentPhase) (beforeUnload trajectories))
      "modal observation requires the retained-film-route physical-key trajectory"
  either (ioError . userError) pure (checkAfOwnershipRoute (isJust (afOwnershipPlanFile given)) (beforeUnload trajectories) (afterUnload trajectories))
  let settleCaps = [ () | segment <- beforeUnload trajectories ++ afterUnload trajectories
                        , step <- segmentSteps segment, isJust (keyTimingSettle (stepTiming step)) || isJust (stepIdle step) || stepCompletion step /= PacketCompletion ]
  case emulationRuntime given of
    RetainedRuntime | not (null settleCaps) ->
      ioError (userError "step timing and presented-frame completion require the FujiMod bridge")
    FujiModRuntime _ | phase : _ <- instructionPinnedPhases trajectories ->
      ioError (userError ("segment " ++ T.unpack phase ++ " pins an instruction hold, which only the retained engine takes;"
        ++ " FujiMod presses use the bridge's tick hold (run the NAME.fujimod.json variant without pins)"))
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
  stackDiagnostic <- lookupEnv "XM1_RENDER_STACK_DIAGNOSTIC" >>= traverse readRenderStackDiagnosticConfig
  afPlan <- traverse (\path -> do
    bytes <- B.readFile path
    require (B.length bytes <= 1024 * 1024) "AF ownership plan exceeds 1 MiB"
    either (ioError . userError) pure (decodeAfOwnershipPlan bytes)) (afOwnershipPlanFile settings)
  let enabledAfPlan = if afOwnershipControl settings then Nothing else afPlan
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
      scoped = payloadKind selected `elem` [RecipeBankModal75, RecipeBankModalLifecycle, RecipeBankModal75MenuEngine]
      lifetimeGuarded = scoped || modalLifetimeGuard settings == ContinuousModalLifetimeGuard
      unreviewedBank = payloadKind selected `elem` [RecipeBankModal75, RecipeBankLifecycle, RecipeBankModalLifecycle, RecipeBankModal75MenuEngine]
      relinkAt base action
        | experimental = do
            relinker <- runfilesRelinker selected
            withRelinkedBuildUsing relinker EnforceAllowlist base action
        | otherwise = case relinkMode settings of
            ReviewedRelink -> withPinnedReviewedBuild (payloadContract selected) selected base action
            UnreviewedRelink policy -> case modalOwnedPackage settings of
              Just owned -> case prelinkedModalBuild settings of
                Just directory -> do
                  archive <- either (ioError . userError) pure (parseSha256Hex (ownedArchiveDigest owned))
                  withPrelinkedBuild directory archive selected policy base action
                Nothing -> withUnreviewedBuildArchive (ownedArchivePath owned) selected policy base action
              Nothing -> withUnreviewedBuild selected policy base action
      pushWith observer retained
        | experimental = runRamPushWith xm1PeerCacheAvailability observer (retainedAllocation retained)
        | otherwise = case relinkMode settings of
            ReviewedRelink -> case observer of
              NoRamPushObserver -> runRamPush (payloadContract selected) retained
              BeforeRamPublication _ -> \_ _ _ _ -> ioError (userError "installed-lifetime observation requires an unreviewed emulation relink")
            UnreviewedRelink _ -> runUnreviewedRamPush observer retained
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
        <*> (if lifetimeGuarded then Just <$> (RelinkedModal
          <$> (if scoped then B.readFile (pinnedDirectory pinned </> "modal-scope.json") else pure B.empty)
          <*> B.readFile (pinnedDirectory pinned </> "kernel.elf")) else pure Nothing)
        <*> (if isJust (payloadBankVariant selected)
          then Just <$> B.readFile (pinnedDirectory pinned </> "bank-allowlist-input.json")
          else pure Nothing)
        <*> (do present <- doesFileExist (pinnedDirectory pinned </> "bank-allowlist-proof.json")
                if present then Just <$> B.readFile (pinnedDirectory pinned </> "bank-allowlist-proof.json") else pure Nothing)
        <*> (do present <- doesFileExist (pinnedDirectory pinned </> "bank-allowlist-report.json")
                if present then Just <$> B.readFile (pinnedDirectory pinned </> "bank-allowlist-report.json") else pure Nothing)
        <*> (if payloadKind selected `elem` [RecipeBankLifecycle,RecipeBankModalLifecycle]
          then Just <$> B.readFile (pinnedDirectory pinned </> "save-lifecycle-program.json")
          else pure Nothing)
      choose candidates
        | lifetimeGuarded && planFromInstall plan = ioError (userError "modal lifetime guard refuses installed checkpoint restore")
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
  require (maybe True (const (payloadKind selected `elem` [RecipeBankModal75, RecipeBankModal75MenuEngine] && relinkMode settings /= ReviewedRelink)) (modalOwnedPackage settings))
    "the owned modal75 package requires a scoped modal75 payload and an unreviewed relink"
  require (payloadKind selected /= RecipeBankModal75MenuEngine || isJust (modalOwnedPackage settings))
    "the menu engine payload links only an owned archive built with --cfg rb_menu_engine_owned"
  when (isJust (prelinkedModalBuild settings)) $ do
    require (isJust (modalOwnedPackage settings)) "a prelinked build requires an owned modal package"
    require (relinkMode settings /= ReviewedRelink) "a prelinked build requires an unreviewed relink"
  when (isJust stackDiagnostic) $ do
    require lifetimeGuarded "render stack diagnostic requires the continuous modal lifetime guard"
    require (case emulationRuntime settings of FujiModRuntime _ -> True; _ -> False)
      "render stack diagnostic requires the FujiMod observer"
  when scoped $
    require (any (not . null . segmentSteps) (planBefore plan))
      "scoped modal observation requires selected post-install physical-key segments"
  when lifetimeGuarded $
    either (ioError . userError) pure (checkModalLifetimeRoute
      (if planFromInstall plan then RestoredModalInstall else FreshModalInstall)
      (if planUnload plan then ModalUnloadRequested else ModalUnloadOmitted))
  when (isJust afPlan) $ do
    require lifetimeGuarded "AF ownership capture requires the continuous installed modal lifetime guard"
    require (pushMode settings == RamPush && segmentSelection settings == Nothing)
      "AF ownership capture refuses no-install and segmented routes"
    either (ioError . userError) pure (checkAfOwnershipRoute True (planBefore plan) (planAfter plan))
  lifetimeSession <- newIORef ModalLifetimeUnconfigured
  afBinding <- traverse (\_ -> case emulationRuntime settings of
    FujiModRuntime bridge -> loadAfArchiveValidator bridge
    _ -> ioError (userError "AF archive binding requires the native FujiMod bridge")) afPlan
  afCatalogue <- traverse (\validator -> case emulationRuntime settings of
    FujiModRuntime bridge -> loadAfMappingCatalogue validator (fujimodRunDocument bridge) (e2e </> "af-source-catalogue")
    _ -> ioError (userError "AF mapping catalogue requires the native FujiMod bridge")) afBinding
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) e2e
  published <- bracket (timed "emulator-start" (startEmulator executable runtime arguments choose))
    (\(emulator, _, _) -> ((finishAfOwnership emulator e2e enabledAfPlan `catch` \(_ :: SomeException) -> pure ()) `finally` finishModalLifetime emulator e2e lifetimeSession) `finally` stopEmulator emulator) $ \(emulator, start, installed) -> recordModalLifetimeFailure lifetimeSession $ do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; relink: " ++ T.unpack (relinkModeText (relinkMode settings))
      ++ "; payload: " ++ T.unpack (payloadText selected))
    forM_ afPlan $ \_ -> do
      proofBytes <- B.readFile (e2e </> "af-reset-prestate.json")
      proof <- either (ioError . userError) pure (eitherDecodeStrict' proofBytes)
      bridge <- case emulationRuntime settings of
        FujiModRuntime bridge -> pure bridge
        _ -> ioError (userError "AF reset proof requires FujiMod")
      modelDigest <- sha256File (fujimodRunDocument bridge)
      installedDigest <- sha256File (runtime </> installedMedia)
      either (ioError . userError) pure (validateAfResetProof modelDigest installedDigest proof)
      forM_ afBinding $ \validator@(AfArchiveValidator _ (AfArchiveBinding expectedModel _)) -> do
        fingerprints <- either (ioError . userError) pure $ flip parseEither proof $ withObject "bound AF reset proof" $ \fields -> do
          identity <- fields .: "identity"
          model <- withObject "bound AF reset model" (\values -> values .: "model" >>= parseFingerprintDigest) identity
          unless (model == expectedModel) (fail "AF reset compiled model differs from the selected SDK")
          mapM (\name -> (,) name <$> fields .: name) ["fingerprint", "live_before_reset"]
        forM_ fingerprints $ \(name, fingerprint) ->
          authenticateAfFingerprint validator (e2e </> ("af-reset-" ++ T.unpack (K.toText name))) fingerprint
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
            unless (B.null (relinkedModalScope modal)) $
              writePrivateBytes (relinkDirectory </> "modal-scope.json") (relinkedModalScope modal)
            writePrivateBytes (relinkDirectory </> "kernel.elf") (relinkedModalElf modal)
          forM_ (relinkedAllowlistInput relinked) (writePrivateBytes (relinkDirectory </> "bank-allowlist-input.json"))
          forM_ (relinkedAllowlistProof relinked) (writePrivateBytes (relinkDirectory </> "bank-allowlist-proof.json"))
          forM_ (relinkedLifecycle relinked) (writePrivateBytes (relinkDirectory </> "save-lifecycle-program.json"))
          let allowlist = allowlistReceipt (relinkedAllowlist relinked)
          case relinkedAllowlistReport relinked of
            Just bytes -> writePrivateBytes (relinkDirectory </> "bank-allowlist.json") bytes
            Nothing -> writePrivateBytes (relinkDirectory </> "bank-allowlist.json") (BL.toStrict (encode allowlist) <> "\n")
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
        allocationClock <- emulatedClock
        allocationPath <- timed "ram-alloc" (stagedSession allocationDirectory
          ((if unreviewedBank then runRamAllocWith xm1PeerCacheAvailability else runRamAlloc)
            allocationBytes allocationClock api)) >>= \outcome -> case outcome of
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
    modalObservation <- if lifetimeGuarded then do
      runtimeSources <- map runtimeFileBindingValue <$> runtimeFileBindings (emulationRuntime settings) adjustmentNand
      runtimeBytes <- B.readFile (relinkDirectory </> "runtime.bin")
      elfBytes <- B.readFile (relinkDirectory </> "kernel.elf")
      loaderA <- B.readFile "private-data/analysis/x-m1-1.50/decode/loader-40000.bin"
      loaderB <- B.readFile "private-data/analysis/x-m1-1.50/decode/loader-d0000.bin"
      lifetimeBound <- either (ioError . userError) pure (bindModalLifetimeSource manifest elfBytes loaderA loaderB)
      retiredSource <- either (ioError . userError) pure (maybe (Right lifetimeBound)
        (\owned -> withRetiredExecution (ownedRetired owned) lifetimeBound) (modalOwnedPackage settings))
      lifetimeSource <- case stackDiagnostic of
        Nothing -> pure retiredSource
        Just _ -> do
          baseline <- emulatorReadMemory emulator 0x00a4ce00 64
          either (ioError . userError) pure (withTask33StackGuard baseline retiredSource)
      forM_ (modalOwnedPackage settings) $ \owned ->
        writePrivateBytes (e2e </> "e2e-modal-owned-package.json") (BL.toStrict (encode (object
          [ "archive" .= ownedArchivePath owned, "archive_sha256" .= ownedArchiveDigest owned
          , "retired_document" .= ownedRetiredPath owned, "retired_document_sha256" .= ownedRetiredDigest owned
          , "table_identity" .= ownedTableIdentity owned
          , "retired" .= [[low, high] | range <- ownedRetired owned, let (low, high) = watchRangeBounds range] ])) <> "\n")
      writePrivateBytes (relinkDirectory </> "loader-a.bin") loaderA
      writePrivateBytes (relinkDirectory </> "loader-b.bin") loaderB
      (stock, stockSource) <- loadModalStock
      (baseRequest, linkedSource) <- if scoped then do
        metadata <- B.readFile (relinkDirectory </> "modal-scope.json")
          >>= either (ioError . userError . ("invalid modal scope metadata: " ++)) pure . eitherDecodeStrict'
        source <- either (ioError . userError) pure (parseScopedModalSource (watchAddress base) runtimeBytes elfBytes metadata)
        request <- either (ioError . userError) pure (scopedModalRequest stock (either error id (modalLimit 65536)) source)
        pure (request, scopedModalSourceJson source)
        else do
          request <- either (ioError . userError) pure (modalRequest stock Nothing (either error id (modalLimit 65536)))
          pure (request, Null)
      request <- either (ioError . userError) pure (lifetimeModalRequest baseRequest lifetimeSource)
      forM_ afPlan $ \af -> do
        bridge <- case emulationRuntime settings of
          FujiModRuntime bridge -> pure bridge
          _ -> ioError (userError "AF ownership capture requires FujiMod")
        bridgePath <- canonicalizePath (fujimodBridge bridge)
        let sdk = takeDirectory (takeDirectory bridgePath)
        require (take 3 (splitDirectories sdk) == ["/", "nix", "store"] && takeDirectory sdk == "/nix/store")
          "AF SDK binding requires an immutable Nix store executable"
        viewer <- getExecutablePath >>= sha256File
        bridgeDigest <- sha256File bridgePath
        model <- sha256File (fujimodRunDocument bridge)
        let actual = object ["sdk" .= sdk, "viewer" .= sha256Hex viewer, "bridge" .= sha256Hex bridgeDigest, "model" .= sha256Hex model, "package" .= manifestDigest manifest]
        require (afOwnershipIdentity af == actual) "AF ownership identity does not match the exact SDK/viewer/bridge/model/package"
        writePrivateBytes (e2e </> "af-ownership-plan.json") (BL.toStrict (encode (afOwnershipPlanJson af)) <> "\n")
      let source = object ["stock" .= stockSource, "linked" .= linkedSource, "lifetime" .= modalLifetimeSourceJson lifetimeSource
            , "manifest_sha256" .= manifestDigest manifest, "source_identity" .= sourceIdentity manifest
            , "runtime_sources" .= runtimeSources, "emulator_start" .= startRecord start]
      writeIORef lifetimeSession (ModalLifetimePrepared request source [])
      pure (Just request)
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
        requireLiveSettings phase before = readLiveSettings emulator >>= \after ->
          forM_ (liveSettingChange before after) (stop phase . describeLiveSettingChange)
    let entries = manifestEntries manifest
    emulatorTraceSitesReading emulator $
      [ (address, label, []) | (address, label) <-
        [ (unAddress (installEntry entries), "uploaded-installer")
        , (unAddress (rollbackEntry entries), "uploaded-rollback")
        , (unAddress (statusEntry entries), "uploaded-status")
        , (rendezvous, "rendezvous-locked")
        , (cpu0Exit, "cpu0-window-exit")
        , (cpu1Exit, "cpu1-window-exit")
        , (menuMessageDrawer, menuMessageSite)
        ] ++ [(payloadInstallerStart selected base - 0x244, d3HookSite) | payloadKind selected == RecipeBankD3] ++ fetchSites ]
      ++ declaredTraceSites
    privateDirectory pushDirectory
    (statePath, installMailbox) <- case restoredHost of
      Just host -> do
        let path = pushDirectory </> "ram-push-state.json"
        writePrivateBytes path (hostPushState host)
        pure (path, hostInstallMailbox host)
      Nothing -> do
        let observer = maybe NoRamPushObserver (\_ -> BeforeRamPublication (armModalLifetime emulator e2e lifetimeSession (afOwnershipPlanJson <$> enabledAfPlan))) modalObservation
        pushClock <- emulatedClock
        settingsBeforePush <- readLiveSettings emulator
        statePath <- timed "ram-push" (stagedSession pushDirectory (pushWith observer retained manifest pushClock api)) >>= \outcome -> case outcome of
          RamPushInstalled path -> pure (pushDirectory </> takeFileName path)
          other -> stop "ram-push" (show other)
        requireLiveSettings "ram-push" settingsBeforePush
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
        unless lifetimeGuarded $ do
          host <- InstallHost <$> B.readFile allocationPath <*> B.readFile statePath
            <*> pure installMailbox <*> pure breakBefore <*> pure breakAfter
          saved <- timed "install-checkpoint" (emulatorSaveInstallCheckpoint emulator base
            (manifestDigest manifest) (runtimeDigest manifest) procedure (installHostValue host))
          putStrLn ("install checkpoint: " ++ C.unpack (BL.toStrict (encode saved)))
        recordModalLifetimeBoundary lifetimeSession InstallationReadback
        pure (statePath, installMailbox)
    case lifecycle of
      Nothing -> do
        state <- readRamPushState statePath
        -- A caught trajectory failure is recorded once, in the result written below.
        let caught = experimental || not (null (planForbiddenExecution plan)) || lifetimeGuarded
            segmentStop phase message
              | caught = ioError (userError (show phase ++ " failed: " ++ message))
              | otherwise = stop phase message
        newRunner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress segmentStop plan
        let runner = newRunner { runnerOverlayExits = emulationRuntime settings == RetainedRuntime && pushMode settings == RamPush, runnerAfOwnership = afPlan, runnerAfOwnershipControl = afOwnershipControl settings, runnerAfArchiveBinding = afBinding, runnerAfMappingCatalogue = afCatalogue }
            segments = runSegments (if lifetimeGuarded then runner { runnerSnapshotPolicy = DifferentialSnapshotPreservation } else runner)
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
        let withStackDiagnostic action = case stackDiagnostic of
              Nothing -> action
              Just config -> mask $ \restore -> do
                writePrivateBytes (e2e </> "e2e-render-stack-diagnostic-request.json")
                  (BL.toStrict (encode (renderStackDiagnosticBeginJson config)) <> "\n")
                actionOutcome <- try $ restore $ do
                  acknowledgement <- emulatorRenderStackDiagnosticBegin emulator config
                  writePrivateBytes (e2e </> "e2e-render-stack-diagnostic-begin.json")
                    (BL.toStrict (encode acknowledgement) <> "\n")
                  either (ioError . userError) pure (checkRenderStackDiagnosticBegin acknowledgement)
                  action
                let outcomeFailure :: Either SomeException a -> Maybe String
                    outcomeFailure = either (Just . displayException) (const Nothing)
                    actionFailure = outcomeFailure actionOutcome
                actionPersistence <- try $ writePrivateBytes (e2e </> "e2e-render-stack-diagnostic-action.json")
                  (BL.toStrict (encode (object ["failure" .= actionFailure])) <> "\n")
                cleanupOutcome <- try $ do
                  report <- emulatorRenderStackDiagnosticEnd emulator
                  writePrivateBytes (e2e </> "e2e-render-stack-diagnostic.json")
                    (BL.toStrict (encode report) <> "\n")
                  either (ioError . userError) pure (checkRenderStackDiagnosticReport config report)
                let cleanupFailure = outcomeFailure cleanupOutcome
                    actionPersistenceFailure = outcomeFailure actionPersistence
                cleanupPersistence <- try $ writePrivateBytes (e2e </> "e2e-render-stack-diagnostic-cleanup.json")
                  (BL.toStrict (encode (object
                    ["failure" .= cleanupFailure, "action_persistence_failure" .= actionPersistenceFailure])) <> "\n")
                let failures = catMaybes
                      [ ("diagnostic action: " ++) <$> actionFailure
                      , ("diagnostic action persistence: " ++) <$> actionPersistenceFailure
                      , ("diagnostic cleanup: " ++) <$> cleanupFailure
                      , ("diagnostic cleanup persistence: " ++) <$> outcomeFailure cleanupPersistence ]
                case actionOutcome of
                  Right value | null failures -> pure value
                  _ -> ioError (userError (unlines failures))
        trajectoryOutcome <- try $ withStackDiagnostic $ do
          captured <- segments (planBefore plan)
          windows <- forM exported $ \(name, Address address, count) -> do
            bytes <- emulatorReadMemory emulator address count
            putStrLn ("read " ++ T.unpack name ++ " " ++ hexAddress address ++ " (" ++ show count ++ " bytes): " ++ hexBytes bytes)
            pure (object ["symbol" .= name, "address" .= hexAddress address, "length" .= count, "bytes" .= hexBytes bytes])
          pure (captured, windows)
        (capturedBefore, symbolReads, trajectoryFailure) <- case trajectoryOutcome of
          Right (captured, windows) -> pure (captured, windows, Nothing)
          Left (failure :: SomeException)
            | experimental || executionGuarded || lifetimeGuarded -> pure ([], [], Just (displayException failure))
            | otherwise -> throwIO failure
        forM_ trajectoryFailure (recordModalLifetimeProblem lifetimeSession)
        case planExecutionInterval plan of
          BeforeUnload -> snapshotExecution BeforeUnload purpose (planExecutionControlPc plan) True trajectoryFailure
          ThroughAfterUnload -> snapshotExecution BeforeUnload CandidateRun Nothing True trajectoryFailure
        (unloadMailbox, capturedAfter) <- (do
          unloadMailbox <- if planUnload plan
            then do
              privateDirectory unloadDirectory
              unloadClock <- emulatedClock
              settingsBeforeUnload <- readLiveSettings emulator
              unload <- timed "ram-unload" (stagedSession unloadDirectory (unloadWith state unloadClock api))
              unless (unload == RamUnloaded) (stop "ram-unload" (show unload))
              requireLiveSettings "ram-unload" settingsBeforeUnload
              recordModalLifetimeBoundary lifetimeSession RollbackReadback
              Just <$> readMailbox
            else pure Nothing
          capturedAfter <- segments (planAfter plan)
          recordModalLifetimeBoundary lifetimeSession AfterUnloadKeys
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
        lifetimeReport <- finishModalLifetime emulator e2e lifetimeSession
        observed <- readIORef (runnerObservations runner)
        rendezvousExits <- readIORef (runnerRendezvousExits runner)
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
              , "observations" .= (observed :: [ObservationRef])
              , "rendezvous_exits" .= (rendezvousExits :: [Value])
              , "reads" .= symbolReads
              , "checks" .= object ["install" .= String "PASS", "trajectory" .= maybe (String "PASS") (const (String "FAIL")) trajectoryFailure, "unload" .= (if planUnload plan then String "PASS" else String "NOT_RUN")]
              , "trajectory_failure" .= trajectoryFailure
              , "mailbox" .= object ["install" .= installMailbox, "rollback" .= unloadMailbox,
                                     "address" .= hexAddress mailbox]
              , "outcome" .= (if scoped && maybe False modalLifetimeCoverage lifetimeReport then String "qualified-scoped-lifetime-diagnostic"
                  else if scoped then String "unqualified-scoped-diagnostic"
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
                  , "interval" .= String "before-installer-through-reverse-rollback-and-after-unload-keys"
                  , "physical_fetch_coverage" .= maybe False modalPhysicalCoverage lifetimeReport
                  , "task_identity_verified" .= maybe False modalTaskIdentityVerified lifetimeReport
                  , "whole_installed_lifetime_coverage" .= maybe False modalLifetimeCoverage lifetimeReport
                  , "rollback_requested" .= planUnload plan
                  ]
              | lifetimeGuarded ]
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
        captured <- runSegments runner (planBefore plan)
        windows <- forM exported $ \(name, Address address, count) -> do
          bytes <- emulatorReadMemory emulator address count
          pure (object ["symbol" .= name,"address" .= hexAddress address,"length" .= count,"bytes" .= hexBytes bytes])
        preparation <- prepareLifecycleCapture output emulator
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
              , "observations" .= (observed :: [ObservationRef]), "reads" .= windows
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

data SnapshotPolicy = EnforceSnapshotPreservation | DifferentialSnapshotPreservation deriving (Eq, Show)
-- | What running segments needs from its session; hits and observations accumulate across calls.
data SegmentRunner = SegmentRunner
  { runnerEmulator :: Emulator
  , runnerReceipt :: FilePath
  , runnerTimed :: T.Text -> IO (Maybe Value) -> IO (Maybe Value)
  , runnerReadAddress :: ReadAddress -> IO Word32
  , runnerStop :: T.Text -> String -> IO ()
  , runnerTraceLabels :: [String]
  , runnerLrLabels :: [String]
  , runnerSpLabels :: [String]
  , runnerHits :: IORef Int
  , runnerObservations :: IORef [ObservationRef]
  -- | Whether the bridge's rendezvous leaves the focus-area overlay open (the retained engine), so
  -- each segment list's 'overlayExit' runs on its own and is recorded in 'runnerRendezvousExits'.
  , runnerOverlayExits :: Bool
  , runnerRendezvousExits :: IORef [Value]
  , runnerReads :: IORef [CapturedRead]
  , runnerSnapshotPolicy :: SnapshotPolicy
  , runnerAfOwnership :: Maybe AfOwnershipPlan
  , runnerAfOwnershipControl :: Bool
  , runnerAfArchiveBinding :: Maybe AfArchiveValidator
  , runnerAfMappingCatalogue :: Maybe AfMappingCatalogue
  }

newSegmentRunner :: Emulator -> FilePath -> (T.Text -> IO (Maybe Value) -> IO (Maybe Value))
  -> (ReadAddress -> IO Word32) -> (T.Text -> String -> IO ()) -> Plan -> IO SegmentRunner
newSegmentRunner emulator receipt timedAction readAddress stopAction plan = do
  when (any ((== FixedAddress 0x1164e1c) . traceSiteAddress) (planTraceSites plan)) $
    emulatorEnableRenderObjectProvenance emulator
  SegmentRunner emulator receipt timedAction readAddress stopAction (map traceSiteLabel (planTraceSites plan))
    [traceSiteLabel site | site <- planTraceSites plan, traceSiteLr site == RecordsLr]
    [traceSiteLabel site | site <- planTraceSites plan, traceSiteSp site == RecordsSp]
    <$> newIORef 0 <*> newIORef [] <*> pure False <*> newIORef [] <*> newIORef [] <*> pure EnforceSnapshotPreservation <*> pure Nothing <*> pure False <*> pure Nothing <*> pure Nothing

-- | The plan's trajectory-declared sites in the bridge's form, with each site's entry reads.
resolveTraceSites :: (String -> IO Word32) -> [TraceSite] -> IO [(Word32, String, [SiteRead])]
resolveTraceSites resolve sites = do
  resolved <- forM sites $ \site -> do
    address <- case traceSiteAddress site of
      FixedAddress address -> pure address
      RuntimeSymbol name offset -> do
        symbolBase <- resolve name
        let address = toInteger symbolBase + toInteger offset
        require (address <= 0xffffffff) ("trace site overflows 32 bits: " ++ traceSiteLabel site)
        pure (fromIntegral address)
    pure (address, traceSiteLabel site, traceSiteReads site)
  mapM_ (\(address, label, _) -> require (address `mod` 4 == 0) ("trace site is not word aligned: " ++ label)) resolved
  let addresses = [address | (address, _, _) <- resolved]
  require (length addresses == length (nub addresses)) "trace site addresses are not distinct"
  pure resolved

eightHex :: Word32 -> String
eightHex = drop 2 . hexAddress

data AfArchiveBinding = AfArchiveBinding Sha256 Sha256
data AfArchiveValidator = AfArchiveValidator FilePath AfArchiveBinding
data AfOfflineRequest = AuthenticateAfArchive Value | ReadAfMappingCatalogue FilePath

loadAfArchiveValidator :: FujiModBridge -> IO AfArchiveValidator
loadAfArchiveValidator bridge = do
  path <- canonicalizePath (fujimodBridge bridge)
  let sdk = takeDirectory (takeDirectory path)
  require (take 3 (splitDirectories sdk) == ["/", "nix", "store"] && takeDirectory sdk == "/nix/store")
    "AF archive binding requires an immutable SDK store executable"
  modelBytes <- B.readFile (sdk </> "share/fujimod-xm1/checkpoint-model-id")
  model <- either (ioError . userError) pure (parseSha256Hex (T.unpack (T.strip (TE.decodeUtf8 modelBytes))))
  binding <- AfArchiveBinding model <$> sha256File path
  pure (AfArchiveValidator path binding)

validateAfArchiveBinding :: AfArchiveBinding -> Value -> Either String ()
validateAfArchiveBinding (AfArchiveBinding expectedModel expectedExecutable) = parseEither $ withObject "bound AF machine fingerprint" $ \fields -> do
  archive <- fields .: "exact_state"
  flip (withObject "AF exact immutable identity") archive $ \identity -> do
    model <- identity .: "checkpoint_model_id" >>= parseFingerprintDigest
    executable <- identity .: "executable_sha256" >>= parseFingerprintDigest
    definition <- identity .: "model_definition_sha256" >>= parseFingerprintDigest
    unless (model == expectedModel && executable == expectedExecutable && definition == sha256 "null")
      (fail "AF exact archive immutable SDK identity differs")

runAfOfflineRequest :: AfArchiveValidator -> FilePath -> AfOfflineRequest -> IO Value
runAfOfflineRequest (AfArchiveValidator executable (AfArchiveBinding expectedModel expectedExecutable)) path request = do
  let output = path ++ ".validation.raw"
      arguments = case request of
        AuthenticateAfArchive _ -> ["--validate-machine-fingerprint",
          "--expected-checkpoint-model", sha256Hex expectedModel,
          "--expected-definition", sha256Hex (sha256 "null"),
          "--expected-executable", sha256Hex expectedExecutable]
        ReadAfMappingCatalogue runDocument -> ["--af-mapping-catalogue", "--run", runDocument]
  exit <- withFile output WriteMode $ \receipt -> do
    setFileMode output 0o600
    withCreateProcess (proc executable arguments) { std_in = CreatePipe, std_out = UseHandle receipt, std_err = UseHandle receipt } $ \input _ _ process -> do
      stream <- maybe (ioError (userError "AF validator stdin was not created")) pure input
      case request of
        AuthenticateAfArchive fingerprint -> BL.hPutStr stream (encode fingerprint)
        ReadAfMappingCatalogue _ -> pure ()
      hClose stream
      waitForProcess process
  require (exit == ExitSuccess) "AF offline SDK validation refused; raw response retained"
  outputBytes <- getFileSize output
  require (outputBytes <= 16777216) "AF offline SDK response exceeds 16 MiB"
  bytes <- B.readFile output
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

authenticateAfFingerprint :: AfArchiveValidator -> FilePath -> Value -> IO ()
authenticateAfFingerprint validator@(AfArchiveValidator _ binding@(AfArchiveBinding expectedModel expectedExecutable)) path fingerprint = do
  either (ioError . userError) pure (validateAfArchiveBinding binding fingerprint)
  response <- runAfOfflineRequest validator path (AuthenticateAfArchive fingerprint)
  expected <- either (ioError . userError) pure $ flip parseEither fingerprint $ withObject "AF authenticated fingerprint binding" $ \fields -> do
    machine <- fields .: "machine_sha256" :: Parser Value
    counts <- fields .: "counts" :: Parser Value
    now <- fields .: "now" :: Parser Value
    pure (object ["schema" .= (1 :: Int), "status" .= String "complete",
      "checkpoint_model_id" .= sha256Hex expectedModel, "model_definition_sha256" .= sha256Hex (sha256 "null"),
      "executable_sha256" .= sha256Hex expectedExecutable, "machine_sha256" .= machine, "counts" .= counts, "now" .= now])
  require (response == expected) "AF exact-state SDK authentication response differs from its input"

loadAfMappingCatalogue :: AfArchiveValidator -> FilePath -> FilePath -> IO AfMappingCatalogue
loadAfMappingCatalogue validator@(AfArchiveValidator _ (AfArchiveBinding expectedModel expectedExecutable)) runDocument path = do
  expectedRun <- sha256File runDocument
  response <- runAfOfflineRequest validator path (ReadAfMappingCatalogue runDocument)
  either (ioError . userError) pure $ flip parseEither response $ withObject "AF source mapping catalogue" $ \fields -> do
    unless (sort (KM.keys fields) == sort ["schema", "status", "checkpoint_model_id", "executable_sha256", "run_document_sha256", "mappings"])
      (fail "AF source mapping catalogue fields differ")
    schema <- fields .: "schema" :: Parser Int
    status <- fields .: "status" :: Parser T.Text
    model <- fields .: "checkpoint_model_id" >>= parseFingerprintDigest
    executable <- fields .: "executable_sha256" >>= parseFingerprintDigest
    run <- fields .: "run_document_sha256" >>= parseFingerprintDigest
    unless (schema == 1 && status == "complete" && model == expectedModel && executable == expectedExecutable && run == expectedRun)
      (fail "AF source mapping catalogue immutable bindings differ")
    mappings <- fields .: "mappings"
    either fail pure (afMappingCatalogue mappings)

captureAfResponse :: Emulator -> FilePath -> Value -> IO Value
captureAfResponse emulator path request = do
  raw <- emulatorAfCommandRaw emulator request
  writePrivateBytes (path ++ ".raw") raw
  require (B.length raw <= 134217728 && not (B.null raw) && B.last raw == 10 && B.count 10 raw == 1)
    "AF response is oversized, incomplete, or contains trailing protocol lines"
  response <- either (ioError . userError) pure (eitherDecodeStrict' raw)
  pure $ case response of
    Object fields | KM.lookup "ok" fields == Just (Bool True) -> Object (KM.delete "ok" fields)
    value -> value

finishAfOwnership :: Emulator -> FilePath -> Maybe AfOwnershipPlan -> IO ()
finishAfOwnership emulator receipt plan = when (isJust plan) $ do
  armed <- doesFileExist (receipt </> "af-ownership-arm.json")
  ended <- doesFileExist (receipt </> "af-ownership-end.json")
  when (armed && not ended) $ do
    result <- try (captureAfResponse emulator (receipt </> "af-ownership-end.json") (object ["command" .= String "af-ownership-end"]))
      :: IO (Either SomeException Value)
    let retained = either (\failure -> object ["failure" .= displayException failure]) id result
    writePrivateBytes (receipt </> "af-ownership-end.json") (BL.toStrict (encode retained) <> "\n")
      `catch` \(_ :: SomeException) -> pure ()

runAfOwnershipBoundary :: SegmentRunner -> AfOwnershipAction -> IO ()
runAfOwnershipBoundary runner action = do
  captureAfMachineState runner action
  unless (runnerAfOwnershipControl runner) (runEnabledAfOwnershipBoundary runner action)

captureAfMachineState :: SegmentRunner -> AfOwnershipAction -> IO ()
captureAfMachineState runner action = do
  let filename = case action of
        ArmAfOwnership -> "af-state-initial.json"
        EndAfOwnership -> "af-state-final.json"
  result <- try (captureAfResponse (runnerEmulator runner) (runnerReceipt runner </> filename) (object ["command" .= String "machine-fingerprint"]))
    :: IO (Either SomeException Value)
  let retained = either (\failure -> object ["failure" .= displayException failure]) id result
  writePrivateBytes (runnerReceipt runner </> filename) (BL.toStrict (encode retained) <> "\n")
  response <- either throwIO pure result
  either (ioError . userError) (const (pure ())) (parseEither parseMachineFingerprintEnvelope response)
  validator <- maybe (ioError (userError "AF fingerprint has no trusted SDK validator")) pure (runnerAfArchiveBinding runner)
  authenticateAfFingerprint validator (runnerReceipt runner </> filename) response


parseFingerprintDigest :: T.Text -> Parser Sha256
parseFingerprintDigest = either fail pure . parseSha256Hex . T.unpack

validateAfResetProof :: Sha256 -> Sha256 -> Value -> Either String ()
validateAfResetProof expectedRun expectedInstalled value = parseEither (withObject "AF reset prestate proof" $ \fields -> do
  unless (BL.length (encode value) <= 16777216) (fail "AF reset proof metadata exceeds 16 MiB")
  unless (sort (KM.keys fields) == sort ["schema", "status", "identity", "key", "snapshot_sha256", "fingerprint", "reset_cache", "live_before_reset"])
    (fail "AF reset proof fields differ")
  schema <- fields .: "schema"
  unless (schema == (1 :: Int)) (fail "AF reset proof schema differs")
  status <- fields .: "status"
  unless (status == String "complete") (fail "AF reset proof is incomplete")
  _ <- fields .: "key" >>= parseFingerprintDigest
  _ <- fields .: "snapshot_sha256" >>= parseFingerprintDigest
  identity <- fields .: "identity"
  flip (withObject "AF initialized reset identity") identity $ \identityFields -> do
    unless (sort (KM.keys identityFields) == sort ["schema", "model", "run_document", "installed_dat", "boot_rom", "adjustment_nand", "record_nand", "committed_nand", "source_kind", "card_sha256", "procedure"])
      (fail "AF reset identity fields differ")
    identitySchema <- identityFields .: "schema"
    unless (identitySchema == (3 :: Int)) (fail "AF reset identity schema differs")
    run <- identityFields .: "run_document" >>= parseFingerprintDigest
    installed <- identityFields .: "installed_dat" >>= parseFingerprintDigest
    unless (run == expectedRun && installed == expectedInstalled) (fail "AF reset identity differs from the selected run or installed DAT")
    forM_ ["model", "boot_rom"] $ \name -> do
      _ <- identityFields .: name >>= parseFingerprintDigest
      pure ()
    procedure <- identityFields .: "procedure" >>= parseFingerprintDigest
    unless (procedure == sha256 "initialized-adjusted-card-state-v2:af-hit-restored") (fail "AF reset procedure differs")
    sourceKind <- identityFields .: "source_kind" :: Parser T.Text
    unless (sourceKind `elem` ["initial-seeded", "full-committed"]) (fail "AF reset source kind differs")
    forM_ ["adjustment_nand", "record_nand", "committed_nand", "card_sha256"] $ \name -> do
      optionalDigest <- identityFields .: name :: Parser (Maybe T.Text)
      forM_ optionalDigest $ \digest -> do
        _ <- parseFingerprintDigest digest
        pure ()
    adjustment <- identityFields .: "adjustment_nand" :: Parser (Maybe T.Text)
    records <- identityFields .: "record_nand" :: Parser (Maybe T.Text)
    committed <- identityFields .: "committed_nand" :: Parser (Maybe T.Text)
    unless (if sourceKind == "initial-seeded" then isJust adjustment && committed == Nothing else adjustment == Nothing && records == Nothing && isJust committed)
      (fail "AF reset source identity disagrees with its NAND inputs")
  branch <- fields .: "reset_cache"
  withObject "AF reset cache branch" (\branchFields -> do
    unless (sort (KM.keys branchFields) == sort ["lookup", "restore"])
      (fail "AF reset cache branch fields differ")
    lookupResult <- branchFields .: "lookup"
    restoreResult <- branchFields .: "restore"
    unless ((lookupResult, restoreResult) == (String "hit", String "restored"))
      (fail "AF reset cache branch is inconsistent")) branch
  before <- fields .: "live_before_reset" >>= parseMachineFingerprintEnvelope
  unless (before == (0, 0)) (fail "AF incoming reset fingerprint is not zero-count")
  counts <- fields .: "fingerprint" >>= parseMachineFingerprintEnvelope
  unless (counts == (0, 0)) (fail "AF reset fingerprint is not zero-count")
  forM_ ["live_before_reset", "fingerprint"] $ \name -> do
    fingerprint <- fields .: name
    now <- withObject "AF zero-time fingerprint" (.: "now") fingerprint :: Parser Integer
    unless (now == 0) (fail "AF reset fingerprint has advanced virtual time")
  pure ()) value

validateAfResetPair :: Sha256 -> Sha256 -> Value -> Value -> Either String ()
validateAfResetPair expectedRun expectedInstalled enabled control = do
  unless (BL.length (encode enabled) + BL.length (encode control) <= 16777216)
    (Left "AF paired reset proof metadata exceeds 16 MiB")
  validateAfResetProof expectedRun expectedInstalled enabled
  validateAfResetProof expectedRun expectedInstalled control
  let field name (Object fields) = KM.lookup name fields
      field _ _ = Nothing
  unless (all (\name -> field name enabled == field name control)
      ["identity", "key", "snapshot_sha256", "reset_cache", "fingerprint"])
    (Left "AF arms did not restore the same immutable zero-state cache proof")

runEnabledAfOwnershipBoundary :: SegmentRunner -> AfOwnershipAction -> IO ()
runEnabledAfOwnershipBoundary runner action = do
  plan <- maybe (ioError (userError "AF boundary has no ownership plan")) pure (runnerAfOwnership runner)
  let (operation, filename) = case action of
        ArmAfOwnership -> ("af-ownership-arm", "af-ownership-arm.json")
        EndAfOwnership -> ("af-ownership-end", "af-ownership-end.json")
  result <- try (captureAfResponse (runnerEmulator runner) (runnerReceipt runner </> filename) (object ["command" .= (operation :: T.Text)]))
    :: IO (Either SomeException Value)
  let retained = either (\failure -> object ["failure" .= displayException failure]) id result
      bytes = BL.toStrict (encode retained) <> "\n"
  writePrivateBytes (runnerReceipt runner </> filename) bytes
  require (B.length bytes <= 134217728) "AF diagnostic exceeds output byte bound"
  response <- either throwIO pure result
  catalogue <- maybe (ioError (userError "AF capture has no trusted source mapping catalogue")) pure (runnerAfMappingCatalogue runner)
  either (ioError . userError) pure (validateAfOwnershipReceipt catalogue plan action response)
  when (action == EndAfOwnership) $ do
    armBytes <- B.readFile (runnerReceipt runner </> "af-ownership-arm.json")
    armed <- either (ioError . userError) pure (eitherDecodeStrict' armBytes)
    either (ioError . userError) pure (validateAfOwnershipReceipt catalogue plan ArmAfOwnership armed)
    let field name (Object fields) = KM.lookup name fields
        field _ _ = Nothing
    require (all (\name -> field name armed == field name response) ["identity", "initial", "mappings", "limits"])
      "AF final response differs from its armed immutable capture"

traceSiteValues :: [(Word32, String, [SiteRead])] -> [Value]
traceSiteValues sites =
  [ object (["label" .= label, "address" .= eightHex address]
      ++ ["reads" .= map readValue ranges | not (null ranges)])
  | (address, label, ranges) <- sites ]
  where
    readValue (SiteRead start count) = object ["address" .= eightHex start, "length" .= count]
    readValue (StackRead offset count) = object ["sp" .= offset, "bytes" .= count]

data HitEffects = HitEffects
  { effectDrawn :: ![Word32]
  , effectHookRows :: ![Word32]
  , effectCcHits :: ![String]
  , effectDeclared :: !Int
  }

emptyEffects :: HitEffects
emptyEffects = HitEffects [] [] [] 0

addHitEffect :: SegmentRunner -> HitEffects -> TraceHit -> HitEffects
addHitEffect runner effects hit = HitEffects
  { effectDrawn = if traceSite hit == menuMessageSite then let value = traceR0 hit in value `seq` (value : effectDrawn effects) else effectDrawn effects
  , effectHookRows = if traceSite hit == d3HookSite then let value = traceR0 hit in value `seq` (value : effectHookRows effects) else effectHookRows effects
  , effectCcHits = case find ((== traceSite hit) . snd) classicChromeFetchSites of
      Just (_, label) -> label : effectCcHits effects
      Nothing -> effectCcHits effects
  , effectDeclared = effectDeclared effects + if traceSite hit `elem` runnerTraceLabels runner then 1 else 0
  }

effectsSuffix :: HitEffects -> Maybe ProvenanceBatch -> [(FilmRow, Word32)] -> Value
effectsSuffix effects provenance film = object
  ( [ "d3_hook_r0" .= map hexAddress (reverse (effectHookRows effects))
    , "classic_chrome_site_entries" .= reverse (effectCcHits effects)
    , "menu_messages" .= map hexAddress (sort (nub (effectDrawn effects))) ]
    ++ ["render_object_provenance" .= batch | Just batch <- [provenance]]
    ++ [ "film_identity" .= object
         ["expected_row" .= fromEnum expected, "expected_stored_code" .= filmStoredCode expected,
          "stored_code" .= actual]
       | (expected, actual) <- film ] )

-- | Runs each segment and returns its capture receipt. With 'runnerOverlayExits', the list's
-- 'overlayExit' is pressed at the start of its segment, after that segment's mode dial; its step,
-- menu messages, D3 hook rows, Classic Chrome site entries and declared trace hits go to
-- 'runnerRendezvousExits' instead of the segment's observation.
runSegments :: SegmentRunner -> [Segment] -> IO [Maybe Value]
runSegments runner segments = forM (zip [0 :: Int ..] segments) $ \(index, listed) -> runnerTimed runner ("segment " <> segmentPhase listed) $ do
  let exit = case overlayExit segments of
        Just (exitIndex, step) | runnerOverlayExits runner, exitIndex == index -> Just step
        _ -> Nothing
      segment = maybe listed (const listed { segmentSteps = drop 1 (segmentSteps listed) }) exit
      emulator = runnerEmulator runner
      stop = runnerStop runner
  context <- either (ioError . userError) pure (releaseSegment (segmentPhase segment))
  emulatorReleaseContext emulator (Just context)
  before <- readIORef (runnerHits runner)
  let beforeCursor = TraceHitCursor before
  forM_ (segmentModeDial segment) $ \position -> emulatorModeDial emulator position
  (resume, leadingSources) <- case exit of
    Nothing -> pure (beforeCursor, [])
    Just step -> do
      atExit <- emulatorTraceHitCount emulator beforeCursor
      let leadingSources = if atExit > before then [emulatorTraceHitSource emulator beforeCursor (TraceHitCursor atExit) (TraceHitSnapshot atExit)] else []
      pressed <- if segmentObserve segment
        then observe emulator (segmentKeyPulse segment) [step]
        else [] <$ (presses emulator (segmentKeyPulse segment) [step] `catch` \(failure :: IOException) ->
          stop (segmentPhase segment) ("rendezvous overlay exit: " ++ displayException failure))
      afterExit <- emulatorTraceHitCount emulator (TraceHitCursor atExit)
      entry <- writeRendezvousExit runner segment step pressed atExit afterExit
      modifyIORef' (runnerRendezvousExits runner) (++ [entry])
      pure (TraceHitCursor afterExit, leadingSources)
  when (segmentAfOwnership segment == Just ArmAfOwnership) $
    runAfOwnershipBoundary runner ArmAfOwnership
  observed <- (if segmentObserve segment
    then observe emulator (segmentKeyPulse segment) (segmentSteps segment)
    else [] <$ (presses emulator (segmentKeyPulse segment) (segmentSteps segment) `catch` \(failure :: IOException) ->
      stop (segmentPhase segment) (displayException failure)))
    `catch` \(failure :: SomeException) -> do
      when (segmentAfOwnership segment == Just EndAfOwnership) $ do
        captureAfMachineState runner EndAfOwnership `catch` \(_ :: SomeException) -> pure ()
        unless (runnerAfOwnershipControl runner) $
          finishAfOwnership emulator (runnerReceipt runner) (runnerAfOwnership runner) `catch` \(_ :: SomeException) -> pure ()
      throwIO failure
  when (segmentAfOwnership segment == Just EndAfOwnership) $
    runAfOwnershipBoundary runner EndAfOwnership
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
          , "admission" .= (source == Nothing || runnerSnapshotPolicy runner == EnforceSnapshotPreservation)
          , "problem" .= either Just (const Nothing) checked ]
    writePrivateBytes (runnerReceipt runner </> ("e2e-read-check-" ++ show observationIndex ++ "-" ++ show checkIndex ++ ".json"))
      (BL.toStrict (encode evidence) <> "\n")
    when (source == Nothing || runnerSnapshotPolicy runner == EnforceSnapshotPreservation) $
      either (stop (segmentPhase segment)) pure checked
  modifyIORef' (runnerReads runner) (++ map fst capturedWindows)
  film <- traverse (\expected -> do
      -- Ordinary image byte 0x8a stores the film code in bits 4-7.
      bytes <- emulatorReadMemory emulator 0x0096ccee 1
      require (B.length bytes == 1) "film identity read must contain one byte"
      pure (expected, fromIntegral (B.head bytes `shiftR` 4) :: Word32))
    (segmentFilmRow segment)
  after <- emulatorTraceHitCount emulator resume
  let trailingSources = if after > cursorInt resume then [emulatorTraceHitSource emulator resume (TraceHitCursor after) (TraceHitSnapshot after)] else []
      sources = leadingSources ++ trailingSources
      format = TraceHitFormat (runnerLrLabels runner) (runnerSpLabels runner) (runnerTraceLabels runner)
  effects <- foldTraceHitSources sources (\effects hit -> pure (addHitEffect runner effects hit)) emptyEffects
  when (segmentObserve segment || not (null (segmentReads segment)) || not (null (effectHookRows effects))
        || not (null (effectCcHits effects)) || effectDeclared effects > 0 || not (null film)) $ do
    provenance <- emulatorRenderObjectProvenance emulator
    let prefix = object
          [ "phase" .= segmentPhase segment, "capture" .= segmentCapture segment
          , "steps" .= observed, "reads" .= windows ]
        suffix = effectsSuffix effects provenance (maybe [] pure film)
        spec = ObservationSpec (runnerReceipt runner) ("e2e-observation-" ++ show observationIndex)
          (segmentPhase segment) prefix sources format suffix
    ref <- writeObservationStream spec
    when (effectDeclared effects /= traceHitRefCount (observationRefTraceHits ref)) $
      stop (segmentPhase segment) "declared trace-hit count differs from the written observation reference"
    modifyIORef' (runnerObservations runner) (++ [ref])
  writeIORef (runnerHits runner) after
  forM_ film $ \(expected, actual) ->
    unless (actual == filmStoredCode expected) $
      stop (segmentPhase segment) ("film row " ++ show (fromEnum expected)
        ++ " requires stored code " ++ show (filmStoredCode expected) ++ ", got " ++ show actual)
  unless (segmentObserve segment || sort (nub (effectDrawn effects)) == sort (nub (segmentMessages segment))) $
    stop (segmentPhase segment) ("menu messages drawn " ++ show (map hexAddress (sort (nub (effectDrawn effects))))
      ++ ", expected " ++ show (map hexAddress (segmentMessages segment)))
  emulatorReleaseContext emulator Nothing
  pure captured

cursorInt :: TraceHitCursor -> Int
cursorInt (TraceHitCursor value) = value

writeRendezvousExit :: SegmentRunner -> Segment -> Step -> [Value] -> Int -> Int -> IO Value
writeRendezvousExit runner segment step observed atExit afterExit = do
  let emulator = runnerEmulator runner
      sources = if afterExit > atExit then [emulatorTraceHitSource emulator (TraceHitCursor atExit) (TraceHitCursor afterExit) (TraceHitSnapshot afterExit)] else []
      format = TraceHitFormat (runnerLrLabels runner) (runnerSpLabels runner) (runnerTraceLabels runner)
  effects <- foldTraceHitSources sources (\effects hit -> pure (addHitEffect runner effects hit)) emptyEffects
  index <- length <$> readIORef (runnerRendezvousExits runner)
  let prefix = object ["phase" .= segmentPhase segment, "key" .= stepKey step, "steps" .= observed]
      suffix = effectsSuffix effects Nothing []
      spec = ObservationSpec (runnerReceipt runner) ("e2e-rendezvous-exit-" ++ show index)
        (segmentPhase segment) prefix sources format suffix
  ref <- writeObservationStream spec
  pure (object
    ( [ "phase" .= segmentPhase segment, "key" .= stepKey step, "steps" .= observed
      , "observation" .= ref, "trace_hits" .= observationRefTraceHits ref ]
      ++ [ "menu_messages" .= map hexAddress (sort (nub (effectDrawn effects)))
         , "d3_hook_r0" .= map hexAddress (reverse (effectHookRows effects))
         , "classic_chrome_site_entries" .= reverse (effectCcHits effects) ] ))

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
    emulatorTraceSitesReading emulator ([(menuMessageDrawer, menuMessageSite, []), (d3HookEntry, d3HookSite, [])] ++ declaredTraceSites)
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
      , "observations" .= (observed :: [ObservationRef])
      ])) <> "\n")

-- | X-M1 1.50 `firmware.current_menu` in `emulation/fujimod-xm1/data/xm1-150.json`: the open
-- node pointer at +0 and the cursor at +8.
currentMenuRecord :: Word32
currentMenuRecord = 0x009571d0

-- | Crawls the stock screens of the no-install cardless FujiMod route from held snapshots, journals
-- each trial to RECEIPT/crawl-trials.jsonl and writes RECEIPT/crawl-graph.json.
runMenuCrawlE2E :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runMenuCrawlE2E given python workspace configFile installedMedia adjustmentNand receipt = do
  configBytes <- B.readFile configFile
  config <- either (ioError . userError . ("invalid menu crawl config: " ++)) pure (Crawl.decodeCrawlConfig configBytes)
  withScreenProbe "menu crawl" given python workspace "crawl-config.json" installedMedia adjustmentNand receipt configBytes $
    \emulator inputs -> do
      emulatorTraceSites emulator [(Crawl.crawlSiteAddress site, Crawl.crawlSiteLabel site) | site <- Crawl.crawlSites]
      hitCursor <- newIORef . fst =<< emulatorTraceHits emulator 0
      let screenshot label = receipt </> "screenshots" </> (label ++ ".png")
          word bytes offset = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3] :: Word32
          observeScreen label = do
            digest <- probeFrame emulator label
            written <- doesFileExist (screenshot label)
            require written ("capture did not write " ++ screenshot label)
            removeFile (replaceExtension (screenshot label) "planes.bin")
            cells <- emulatorReadMemory emulator 0x01467588 (4 * Crawl.controllerCount)
            record <- emulatorReadMemory emulator currentMenuRecord 12
            let pointer = word record 0
            node <- if pointer == 0 then pure Nothing else Just <$> emulatorReadMemory emulator pointer 48
            identity <- B.concat <$> mapM (\range -> emulatorReadMemory emulator (Crawl.rangeStart range) (fromIntegral (Crawl.rangeEnd range - Crawl.rangeStart range)))
              (Crawl.configIdentityReads config)
            pure Crawl.Screen
              { Crawl.screenMenu = fmap (\bytes -> (word bytes 0, word bytes 8, word record 8)) node
              , Crawl.screenFrame = Crawl.FrameDigest digest
              , Crawl.screenImage = screenshot label
              , Crawl.screenCells = Crawl.ControllerCells (B.take (2 * Crawl.controllerCount) cells) (B.drop (2 * Crawl.controllerCount) cells)
              , Crawl.screenNode = node
              , Crawl.screenIdentity = identity
              }
      withFile (receipt </> "crawl-trials.jsonl") WriteMode $ \journal -> do
        let driver = Crawl.Driver
              { Crawl.driverSave = emulatorSnapshotSave emulator
              , Crawl.driverRestore = \name -> () <$ emulatorSnapshotRestore emulator name
              , Crawl.driverDrop = emulatorSnapshotDrop emulator
              , Crawl.driverPress = \name key -> emulatorSnapshotKey emulator name (crawlKeyWire key)
              , Crawl.driverModeDial = \position -> () <$ emulatorModeDial emulator position
              , Crawl.driverObserve = observeScreen
              , Crawl.driverTraceMark = readIORef hitCursor
              , Crawl.driverTraceSince = \mark -> do
                  (count, hits) <- emulatorTraceHits emulator mark
                  writeIORef hitCursor count
                  pure hits
              , Crawl.driverDiscardImage = \image -> removeFile image >> removeFile (replaceExtension image "scanout.json")
              , Crawl.driverClock = getMonotonicTime
              , Crawl.driverRecord = \value -> BL.hPut journal (encode value <> "\n") >> hFlush journal
              }
        graph <- Crawl.crawl config driver
        writePrivateBytes (receipt </> "crawl-graph.json") (BL.toStrict (encode (object
          [ "inputs" .= inputs
          , "graph" .= Crawl.graphValue config graph
          ])) <> "\n")
        putStrLn ("menu crawl: " ++ show (length (Crawl.graphNodes graph)) ++ " screens, " ++ show (Crawl.graphTrials graph)
          ++ " trials, " ++ seconds (Crawl.graphSeconds graph) ++ maybe "" (const ", budget-limited") (Crawl.graphLimit graph))

-- | Starts the no-install cardless FujiMod route for a screen probe named `probe`, writes its
-- configuration to RECEIPT/CONFIG_NAME and runs `body` with the emulator and the input record.
withScreenProbe :: String -> E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> B.ByteString
  -> (Emulator -> Value -> IO ()) -> IO ()
withScreenProbe probe given python workspace configName installedMedia adjustmentNand receipt configBytes body = do
  hSetBuffering stdout LineBuffering
  require (pushMode given == FlashCandidate && emulationCardMode given == Cardless
    && case emulationRuntime given of FujiModRuntime _ -> True; RetainedRuntime -> False)
    ("the " ++ probe ++ " requires the no-install --cardless --fujimod --flash-candidate route")
  require (modalDiagnostic given == NoModalDiagnostic && segmentSelection given == Nothing)
    ("the " ++ probe ++ " takes no modal diagnostic or segment selection")
  require (isAbsolute receipt) ("the " ++ probe ++ " receipt path must be absolute")
  exists <- doesDirectoryExist receipt
  when exists $ do
    entries <- listDirectory receipt
    require (null entries) (probe ++ " receipt directory must be fresh or empty")
    removeDirectory receipt
  runtime <- getCurrentDirectory
  datDigest <- sha256File (runtime </> installedMedia)
  require (sha256Hex datDigest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    ("the " ++ probe ++ " requires the pinned stock X-M1 1.50 DAT")
  setCurrentDirectory workspace
  (executable, arguments) <- sessionBridge given python installedMedia adjustmentNand (bootMode given) receipt
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $
    \(emulator, start, ()) -> do
      setFileMode receipt 0o700
      ensurePrivateDirectory receipt
      writePrivateBytes (receipt </> configName) configBytes
      configDigest <- sha256File (receipt </> configName)
      body emulator (object
        [ "dat_sha256" .= sha256Hex datDigest, "config_sha256" .= sha256Hex configDigest, "adjustment_nand" .= adjustmentNand
        , "emulator_start" .= startRecord start ])

-- | The capture's PNG sha256.
probeFrame :: Emulator -> String -> IO String
probeFrame emulator label = do
  shot <- emulatorCapture emulator label
  case shot of
    Object fields | Just (String text) <- KM.lookup "sha256" fields -> pure (T.unpack text)
    _ -> ioError (userError "capture reply lacks its sha256")

-- | Restores `source` and presses `key`; a strict stop is an error.
probePress :: Emulator -> SnapshotName -> Crawl.CrawlKey -> IO ()
probePress emulator source key = do
  pressed <- emulatorSnapshotKey emulator source (Crawl.crawlKeyWire key)
  case pressed of
    SnapshotPressed {} -> pure ()
    SnapshotStopped failure _ -> ioError (userError ("probe press " ++ Crawl.crawlKeyWire key ++ " stopped: " ++ failure))

-- | Saves the current guest as `probe-root`, presses the path from it and returns the snapshot
-- of the last screen, holding only `probe-root` and that snapshot.
probeWalk :: Emulator -> [Crawl.CrawlKey] -> IO SnapshotName
probeWalk emulator path = do
  let root = SnapshotName "probe-root"
      walk source [] = pure source
      walk source ((index, key) : rest) = do
        probePress emulator source key
        let name = SnapshotName ("probe-path-" ++ show (index :: Int))
        emulatorSnapshotSave emulator name
        when (source /= root) (emulatorSnapshotDrop emulator source)
        walk name rest
  emulatorSnapshotSave emulator root
  walk root (zip [0 ..] path)

-- | From the no-install cardless FujiMod root, presses a key path to one screen, then samples the
-- configured windows before a key, after it, after each return key and after a control press from
-- the same screen, and writes RECEIPT/ram-diff.json with the words the key toggles.
runRamDiffE2E :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runRamDiffE2E given python workspace configFile installedMedia adjustmentNand receipt = do
  configBytes <- B.readFile configFile
  config <- either (ioError . userError . ("invalid RAM diff config: " ++)) pure (RamDiff.decodeRamDiffConfig configBytes)
  withScreenProbe "RAM diff" given python workspace "ram-diff-config.json" installedMedia adjustmentNand receipt configBytes $
    \emulator inputs -> do
      let windows = RamDiff.diffWindows config
          sample = mapM (\window -> B.concat <$> mapM (uncurry (emulatorReadMemory emulator)) (RamDiff.rangeChunks window)) windows
          frame = probeFrame emulator
          pressFrom = probePress emulator
      target <- probeWalk emulator (RamDiff.diffPath config)
      _ <- emulatorSnapshotRestore emulator target
      targetFrame <- frame "target"
      before <- sample
      pressFrom target (RamDiff.diffKey config)
      onceFrame <- frame "once"
      once <- sample
      emulatorSnapshotSave emulator (SnapshotName "diff-once")
      returns <- mapM (\key -> do
        pressFrom (SnapshotName "diff-once") key
        returnFrame <- frame ("return-" ++ Crawl.crawlKeyWire key)
        returned <- sample
        pure (key, returnFrame, returned)) (RamDiff.diffReturnKeys config)
      pressFrom target (RamDiff.diffControl config)
      controlFrame <- frame "control"
      control <- sample
      let restoredBy = RamDiff.byReturnKey
            [ (key, concat (zipWith5 RamDiff.toggledWords windows before once returned control)) | (key, _, returned) <- returns ]
          hex32 value = "0x" ++ showHex (value :: Word32) ""
      writePrivateBytes (receipt </> "ram-diff.json") (BL.toStrict (encode (object
        [ "inputs" .= inputs
        , "frames" .= object (["target" .= targetFrame, "once" .= onceFrame, "control" .= controlFrame]
            ++ [K.fromString ("return-" ++ Crawl.crawlKeyWire key) .= returnFrame | (key, returnFrame, _) <- returns])
        , "changed_words" .= object
            ([ "once" .= sum (zipWith RamDiff.changedWords before once)
             , "control" .= sum (zipWith RamDiff.changedWords before control) ]
            ++ [K.fromString ("return-" ++ Crawl.crawlKeyWire key) .= sum (zipWith RamDiff.changedWords before returned) | (key, _, returned) <- returns])
        , "candidates" .= [ object ["address" .= hex32 (RamDiff.candidateAddress candidate), "before" .= hex32 (RamDiff.candidateBefore candidate)
            , "after_key" .= hex32 (RamDiff.candidateAfterKey candidate), "restored_by" .= keys] | (candidate, keys) <- restoredBy ]
        ])) <> "\n")
      putStrLn ("RAM diff: " ++ show (length restoredBy) ++ " toggled words")
 where
  zipWith5 f (a : as) (b : bs) (c : cs) (d : ds) (e : es) = f a b c d e : zipWith5 f as bs cs ds es
  zipWith5 _ _ _ _ _ _ = []

-- | The menu globals `[0x009571d0,0x00957600)`, read before and after the observed press.
menuGlobals :: (Word32, Int)
menuGlobals = (0x009571d0, 0x430)

-- | From the no-install cardless FujiMod root, presses a key path to one screen, arms the
-- configured trace sites and a write watch over the configured ranges, presses the observed key
-- once without a snapshot restore, and writes RECEIPT/menu-observe.json with the bridge's trace
-- hits, the watch state and the menu globals before and after, then RECEIPT/menu-observe-after.json
-- with the frame after the press.
runMenuObserveE2E :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runMenuObserveE2E given python workspace configFile installedMedia adjustmentNand receipt = do
  configBytes <- B.readFile configFile
  config <- either (ioError . userError . ("invalid menu observe config: " ++)) pure (MenuObserve.decodeMenuObserveConfig configBytes)
  withScreenProbe "menu observation" given python workspace "menu-observe-config.json" installedMedia adjustmentNand receipt configBytes $
    \emulator inputs -> do
      target <- probeWalk emulator (MenuObserve.observePath config)
      _ <- emulatorSnapshotRestore emulator target
      beforeFrame <- probeFrame emulator "before"
      globalsBefore <- uncurry (emulatorReadMemory emulator) menuGlobals
      emulatorTraceSitesReading emulator
        [ (MenuObserve.traceSiteAddress site, MenuObserve.traceSiteLabel site
          , [case read_ of
               MenuObserve.SiteRead start count -> SiteRead start count
               MenuObserve.StackRead offset count -> StackRead offset count
            | read_ <- MenuObserve.traceSiteReads site])
        | site <- MenuObserve.observeSites config ]
      (mark, _) <- emulatorTraceHits emulator 0
      ranges <- either (ioError . userError) pure (sequence
        [ watchRange (watchAddress (Crawl.rangeStart range)) (watchAddress (Crawl.rangeEnd range)) | range <- MenuObserve.observeWrites config ])
      limit <- either (ioError . userError) pure (watchLimit (MenuObserve.observeLimit config))
      armed <- emulatorExecutionWatchBegin emulator ranges [] limit
      reply <- emulatorKey emulator defaultKeyTiming (Crawl.crawlKeyWire (MenuObserve.observeKey config))
      watch <- emulatorWatchStateRaw emulator
      hits <- emulatorTraceHitsRaw emulator mark
      globalsAfter <- uncurry (emulatorReadMemory emulator) menuGlobals
      let hex32 value = "0x" ++ showHex (value :: Word32) ""
          receiptFields =
            [ "inputs" .= inputs
            , "key" .= Crawl.crawlKeyWire (MenuObserve.observeKey config)
            , "key_reply" .= reply
            , "watch_armed" .= armed
            , "menu_globals" .= object
                [ "address" .= hex32 (fst menuGlobals), "before" .= hexBytes globalsBefore, "after" .= hexBytes globalsAfter ]
            , "trace_hits" .= hits
            , "watch" .= watch
            ]
          writeJson name fields = writePrivateBytes (receipt </> name) (BL.toStrict (encode (object fields)) <> "\n")
          count field = case watch of
            Object fields | Just (Array items) <- KM.lookup field fields -> length items
            _ -> 0
          violation = case watch of
            Object fields | Just (String text) <- KM.lookup "violation" fields -> T.unpack text
            _ -> "none"
          hitCount = case hits of
            Array items -> length items
            _ -> 0
      writeJson "menu-observe.json" (("frames" .= object ["before" .= beforeFrame]) : receiptFields)
      afterFrame <- probeFrame emulator "after"
      writeJson "menu-observe-after.json" ["frame" .= afterFrame]
      putStrLn ("menu observation: " ++ show hitCount ++ " trace hits, " ++ show (count "records") ++ " watch records, violation "
        ++ violation)


observeRetainedModal :: Emulator -> FilePath -> ModalStock -> Value -> IO a -> IO a
observeRetainedModal emulator receipt stock source action = do
  let request = either error id (modalRequest stock Nothing (either error id (modalLimit 65536)))
      limit = modalRequestLimit request
      writerLimit = either error id (watchLimit 65536)
      captureModal call = (Right <$> call) `catch` \(failure :: SomeException) -> pure (Left (displayException failure))
      parsed response = response >>= modalParsedResponse
      rawResponse response = either (const Null) modalRawResponse response
      responseError response = either Just (either Just (const Nothing) . modalParsedResponse) response
  initialPermit <- captureModal (emulatorModalObservationReport emulator limit)
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
        , "policy" .= String "stock-baseline"
        , "start" .= String "before-selected-physical-key-segments"
        , "end" .= String "after-selected-physical-key-segments"
        , "whole_installed_lifetime_coverage" .= False
        , "physical_fetch_coverage" .= False
        , "task_identity_verified" .= False
        , "same_route_migration_to_core_with_existing_permit_detected" .= False
        , "writer_semantics" .= String "write attempts are audit evidence, not no-write admission"
        , "writer_ranges" .= [let (low, high) = watchRangeBounds modalArrayRange in [low, high]]
        , "writer_limit" .= watchLimitValue writerLimit
        , "permit_limit" .= modalLimitValue limit
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

data ModalLifetimeBoundary = UploadSynchronized | InstallationReadback | RollbackReadback | AfterUnloadKeys deriving (Eq, Show)
data ModalLifetimeAttempt = ModalLifetimeAttempt
  { lifetimeAttemptRequest :: ModalRequest
  , lifetimeAttemptSource :: Value
  , lifetimeAttemptBefore :: Either String ModalResponse
  , lifetimeAttemptBegin :: Either String ModalResponse
  , lifetimeAttemptArmed :: Either String Value
  , lifetimeAttemptOwnsObserver :: Bool
  , lifetimeAttemptBoundaries :: [ModalLifetimeBoundary]
  , lifetimeAttemptProblems :: [String]
  }
data ModalLifetimeSession
  = ModalLifetimeUnconfigured
  | ModalLifetimePrepared ModalRequest Value [String]
  | ModalLifetimeObserving ModalLifetimeAttempt
  | ModalLifetimeOffline
  | ModalLifetimeFinished (Maybe ModalReport)

captureModalResponse :: IO ModalResponse -> IO (Either String ModalResponse)
captureModalResponse call = (Right <$> call) `catch` \(failure :: SomeException) -> pure (Left (displayException failure))

recordModalLifetimeProblem :: IORef ModalLifetimeSession -> String -> IO ()
recordModalLifetimeProblem session problem = modifyIORef' session $ \state -> case state of
  ModalLifetimePrepared request source previous -> ModalLifetimePrepared request source (previous ++ [problem])
  ModalLifetimeObserving attempt -> ModalLifetimeObserving attempt { lifetimeAttemptProblems = lifetimeAttemptProblems attempt ++ [problem] }
  _ -> state

recordModalLifetimeFailure :: IORef ModalLifetimeSession -> IO a -> IO a
recordModalLifetimeFailure session action = action `catch` \(failure :: SomeException) -> do
  recordModalLifetimeProblem session (displayException failure)
  throwIO failure

recordModalLifetimeBoundary :: IORef ModalLifetimeSession -> ModalLifetimeBoundary -> IO ()
recordModalLifetimeBoundary session boundary = modifyIORef' session $ \state -> case state of
  ModalLifetimeObserving attempt -> ModalLifetimeObserving attempt
    { lifetimeAttemptBoundaries = lifetimeAttemptBoundaries attempt ++ [boundary] }
  _ -> state

armModalLifetime :: Emulator -> FilePath -> IORef ModalLifetimeSession -> Maybe Value -> IO ()
armModalLifetime emulator receipt session afPlan = do
  state <- readIORef session
  case state of
    ModalLifetimePrepared request source problem -> do
      before <- captureModalResponse (emulatorModalObservationReport emulator (modalRequestLimit request))
      let idle = do
            report <- before >>= modalParsedResponse
            unless (not (modalReportEnabled report)) (Left "another modal observer is active before RAM publication")
      begun <- case idle of
        Left failure -> pure (Left failure)
        Right () -> captureModalResponse (maybe (emulatorModalObservationBegin emulator request)
          (\plan -> emulatorModalObservationBeginWithAfOwnership emulator plan request) afPlan)
      let ownsObserver = case (idle, begun) of
            (Right (), Right response) -> case modalRawResponse response of
              Object fields -> KM.lookup "enabled" fields == Just (Bool True)
              _ -> False
            _ -> False
      armed <- if ownsObserver then rawWatch (emulatorWatchStateRaw emulator) else pure (Left "modal observation was not armed")
      let attempt = ModalLifetimeAttempt request source before begun armed ownsObserver [UploadSynchronized] problem
      writeIORef session (ModalLifetimeObserving attempt)
      writePrivateBytes (receipt </> "e2e-modal-observation-arm.json")
        (BL.toStrict (encode (object
          [ "source" .= source, "before" .= either (const Null) modalRawResponse before
          , "before_error" .= modalResponseProblem before, "begin" .= either (const Null) modalRawResponse begun
          , "begin_error" .= modalResponseProblem begun, "watch" .= either (const Null) id armed
          , "watch_error" .= either Just (const Nothing) armed, "boundary" .= boundaryText UploadSynchronized ])) <> "\n")
      either (ioError . userError) (const (pure ())) $ do
        _ <- begun >>= modalParsedResponse
        _ <- checkLifetimeArrayAudit request armed (armed >>= parseFinalModalWatch)
        pure ()
    _ -> ioError (userError "modal lifetime publication callback is not prepared or was invoked twice")

modalResponseProblem :: Either String ModalResponse -> Maybe String
modalResponseProblem = either Just (either Just (const Nothing) . modalParsedResponse)

boundaryText :: ModalLifetimeBoundary -> String
boundaryText boundary = case boundary of
  UploadSynchronized -> "after-upload-and-sync-before-installer-invocation"
  InstallationReadback -> "all-new-publication-words-read-back"
  RollbackReadback -> "successful-reverse-rollback-all-old-publication-words-read-back"
  AfterUnloadKeys -> "after-unload-physical-key-segments-complete"

finishModalLifetime :: Emulator -> FilePath -> IORef ModalLifetimeSession -> IO (Maybe ModalReport)
finishModalLifetime emulator receipt session = do
  state <- readIORef session
  case state of
    ModalLifetimeUnconfigured -> pure Nothing
    ModalLifetimeFinished report -> pure report
    ModalLifetimeOffline -> pure Nothing
    ModalLifetimePrepared request source problem ->
      finish (ModalLifetimeAttempt request source (Left "publication callback was not reached")
        (Left "modal observation was not armed") (Left "modal observation was not armed") False [] problem)
    ModalLifetimeObserving attempt -> finish attempt
 where
  finish attempt = do
    let request = lifetimeAttemptRequest attempt
        limit = modalRequestLimit request
    let finalPath = receipt </> "e2e-modal-watch-final.json"
        rawPath = finalPath ++ ".raw"
    finalStored <- if lifetimeAttemptOwnsObserver attempt
      then (do
        emulatorWatchStateToFile emulator rawPath
        compactFinalWatch rawPath finalPath) `catch` \(failure :: SomeException) -> pure (Left (displayException failure))
      else pure (Left "modal observation was not armed")
    finalFile <- traverse (\() -> do
      bytes <- getFileSize finalPath
      digest <- sha256Hex <$> sha256File finalPath
      pure (bytes, digest)) finalStored
    beforeEnd <- captureModalResponse (emulatorModalObservationReport emulator limit)
    ended <- if lifetimeAttemptOwnsObserver attempt
      then captureModalResponse (emulatorModalObservationEnd emulator limit)
      else pure (Left "modal observation was not armed by this route")
    afterEnd <- captureModalResponse (emulatorModalObservationReport emulator limit)
    writeIORef session ModalLifetimeOffline
    shutdown <- try (stopEmulator emulator) :: IO (Either SomeException ())
    final <- either Left id <$> traverse (\() -> readFinalModalWatch finalPath) finalStored
    let audit = checkLifetimeArrayAudit request (lifetimeAttemptArmed attempt) final
        settingsWrites = final >>= completedSettingsWrites request
        checked = do
          either (Left . displayException) Right shutdown
          _ <- lifetimeAttemptBegin attempt >>= modalParsedResponse
          writes <- audit
          _ <- ended >>= modalParsedResponse
          report <- afterEnd >>= modalParsedResponse
          finalRows <- final
          forM_ [beforeEnd, ended, afterEnd] $ \response -> do
            parsed <- response >>= modalParsedResponse
            unless (modalReportDescribesFinalWatch finalRows parsed)
              (Left "modal watch summary, row count or row digest changed during no-execution teardown")
          checkCompletedModalLifetime request writes finalRows report
          unless (lifetimeAttemptBoundaries attempt == [UploadSynchronized, InstallationReadback, RollbackReadback, AfterUnloadKeys])
            (Left "modal lifetime host boundaries are incomplete or out of order")
          forM_ (lifetimeAttemptProblems attempt) Left
          pure report
        raw = either (const Null) modalRawResponse
        evidence = object
          [ "schema" .= (2 :: Int), "scope" .= String "source-bound continuous installed modal lifetime"
          , "writer_limit" .= watchLimitValue modalLifetimeWriterLimit
          , "source" .= lifetimeAttemptSource attempt, "request" .= modalBeginRequestJson request
          , "start" .= boundaryText UploadSynchronized
          , "end" .= String "after-unload-keys-and-SDK-old-word-verification"
          , "boundaries" .= map boundaryText (lifetimeAttemptBoundaries attempt)
          , "install_checkpoint_restored" .= False, "install_checkpoint_saved" .= False
          , "whole_installed_lifetime_coverage" .= either (const False) modalLifetimeCoverage checked
          , "physical_fetch_coverage" .= either (const False) modalPhysicalCoverage checked
          , "task_identity_verified" .= either (const False) modalTaskIdentityVerified checked
          , "permit_before" .= raw (lifetimeAttemptBefore attempt)
          , "permit_before_error" .= modalResponseProblem (lifetimeAttemptBefore attempt)
          , "permit_begin" .= raw (lifetimeAttemptBegin attempt)
          , "permit_begin_error" .= modalResponseProblem (lifetimeAttemptBegin attempt)
          , "watch_acknowledgement" .= either (const Null) id (lifetimeAttemptArmed attempt >>= lifetimeWatchAcknowledgement)
          , "watch_acknowledgement_error" .= either Just (const Nothing) (lifetimeAttemptArmed attempt >>= lifetimeWatchAcknowledgement)
          , "watch_armed" .= either (const Null) id (lifetimeAttemptArmed attempt)
          , "watch_armed_error" .= either Just (const Nothing) (lifetimeAttemptArmed attempt)
          , "watch_final_file" .= either (const Null) (\(bytes, digest) -> object
              [ "path" .= takeFileName finalPath, "bytes" .= bytes, "sha256" .= digest ]) finalFile
          , "watch_final_summary" .= either (const Null) finalModalWatchSummary final
          , "watch_final_error" .= either Just (const Nothing) (final >> Right ())
          , "array_write_audit" .= either (const Null) arrayWriteReportJson audit
          , "array_write_audit_error" .= either Just (const Nothing) audit
          , "permit_before_end" .= raw beforeEnd, "permit_before_end_error" .= modalResponseProblem beforeEnd
          , "permit_end" .= raw ended, "permit_end_error" .= modalResponseProblem ended
          , "permit_after_end" .= raw afterEnd, "permit_after_end_error" .= modalResponseProblem afterEnd
          , "shutdown_error" .= either (Just . displayException) (const Nothing) shutdown
          , "failures" .= lifetimeAttemptProblems attempt
          , "settings_writes" .= either (const Null) settingsWritesJson settingsWrites
          , "settings_writes_error" .= either Just (const Nothing) settingsWrites
          , "observation_error" .= either Just (const Nothing) checked ]
        completed = either (const Nothing) Just checked
    writePrivateBytes (receipt </> "e2e-modal-observation.json") (BL.toStrict (encode evidence) <> "\n")
    forM_ (checked >> settingsWrites) $ \writes ->
      writePrivateBytes (receipt </> "e2e-modal-settings-writes.json") (BL.toStrict (encode (settingsWritesJson writes)) <> "\n")
    writeIORef session (ModalLifetimeFinished completed)
    either throwIO pure shutdown
    case (lifetimeAttemptProblems attempt, checked) of
      ([], Left problem) -> ioError (userError problem)
      _ -> pure completed
