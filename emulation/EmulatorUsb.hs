{-# LANGUAGE OverloadedStrings #-}
module EmulatorUsb
  ( Emulator
  , BootStart (..)
  , EmulatorStart (..)
  , InstallCheckpoint (..)
  , startEmulator
  , stopEmulator
  , emulatorSaveInstallCheckpoint
  , emulatorUsbApi
  , BridgeExchange
  , bridgeUsbApi
  , emulatorTraceSites
  , TraceHit (..)
  , emulatorTraceHits
  , emulatorTraceHitsRaw
  , emulatorTraceHitsPage
  , ReleaseSegment
  , releaseSegment
  , emulatorReleaseContext
  , emulatorKey
  , KeyPulse (..)
  , KeyTiming (..)
  , SettleBudget (..)
  , defaultKeyTiming
  , ModeDial (..)
  , modeDialName
  , emulatorModeDial
  , emulatorCapture
  , emulatorReadMemory
  , emulatorWatchBegin
  , emulatorWatchObservation
  , emulatorExecutionWatchBegin
  , emulatorWatchStateRaw
  , emulatorWatchStateLine
  , emulatorWatchEnd
  , decodeBridgeWatchLine
  , ModalResponse (..)
  , emulatorModalObservationBegin
  , emulatorModalObservationReport
  , emulatorModalObservationEnd
  , bridgeModalObservationBegin
  , bridgeModalObservationReport
  , bridgeModalObservationEnd
  , emulatorPreservationBegin
  , emulatorPreservationState
  , emulatorMediaState
  , CaptureCommand (..)
  , emulatorCaptureCommand
  , SnapshotName (..)
  , MenuReading
  , SnapshotPress (..)
  , emulatorSnapshotSave
  , emulatorSnapshotRestore
  , emulatorSnapshotDrop
  , emulatorSnapshotKey
  , CrawlKey (..)
  , crawlKeyWire
  , emulatorDisplayKey
  , emulatorIdle
  ) where

import Control.Concurrent.MVar
import Control.Exception (IOException, bracketOnError, try)
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Result (..), ToJSON (..), Value (..), eitherDecodeStrict', encode, fromJSON, object, withText, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Emulation.PreservationObservation
import Emulation.ModalObservation
import Emulation.WatchObservation
import qualified Emulation.CaptureStream as Capture
import qualified Emulation.SaveLifecycle as Lifecycle
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftL, (.|.))
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Exit (ExitCode (..))
import System.IO
import System.Process
import FujiAF.Transport
import Emulation.KeyReleaseObservation (ReleaseSegment, releaseSegment)
import qualified FujiAF.Broker as P

data Emulator = Emulator
  { emulatorInput :: Handle
  , emulatorOutput :: Handle
  , emulatorProcess :: ProcessHandle
  , emulatorLock :: MVar ()
  , emulatorCaptureReader :: Capture.BoundedReader
  }

-- | How the emulator reached its first host command.
data BootStart = ColdStart | FreshBootStart | RestoredStart | InstalledStart deriving (Eq, Show)

-- | The ready event's checkpoint record, kept whole for the receipt.
data EmulatorStart = EmulatorStart { bootStart :: BootStart, startRecord :: Value }

-- | A saved post-install machine over the current attach checkpoint: the allocation base, the
-- pushed manifest and runtime hashes, the host procedure that pushed them and the host record
-- the e2e stored with it.
data InstallCheckpoint = InstallCheckpoint
  { installDirectory :: FilePath
  , installBase :: Word32
  , installManifestDigest :: String
  , installRuntimeDigest :: String
  , installProcedureDigest :: Maybe String
  , installHost :: Value
  }

require :: Bool -> String -> IO ()
require yes message = unless yes (ioError (userError message))

objectField :: String -> M.KeyMap Value -> IO Value
objectField name fields = maybe (ioError (userError ("emulator response lacks " ++ name))) pure (M.lookup (K.fromString name) fields)

-- | One request line to the bridge and its one response line.
type BridgeExchange = BL.ByteString -> IO B.ByteString

exchangeLine :: Emulator -> BridgeExchange
exchangeLine emulator request = withMVar (emulatorLock emulator) $ \_ -> exchangeLocked emulator request

-- Caller holds emulatorLock.
exchangeLocked :: Emulator -> BridgeExchange
exchangeLocked emulator = exchangeLockedReading emulator (C.hGetLine (emulatorOutput emulator))

exchangeLockedReading :: Emulator -> IO B.ByteString -> BridgeExchange
exchangeLockedReading emulator response request = do
  BL.hPutStr (emulatorInput emulator) request
  B.hPut (emulatorInput emulator) "\n"
  hFlush (emulatorInput emulator)
  response

rpc :: Emulator -> Value -> IO (M.KeyMap Value)
rpc = bridgeRpc . exchangeLine

watchRpc :: Emulator -> Value -> IO Value
watchRpc = bridgeWatchRpc . exchangeLine

bridgeWatchRpc :: BridgeExchange -> Value -> IO Value
bridgeWatchRpc exchange request = decodeBridgeWatchLine <$> exchange (encode request)

-- | A watch response line as a value; undecodable bytes and refusals are retained rather than thrown.
decodeBridgeWatchLine :: B.ByteString -> Value
decodeBridgeWatchLine line = case eitherDecodeStrict' line of
  Left problem -> object ["wire_bytes" .= B.unpack line, "decode_error" .= problem]
  Right (Object fields) | M.lookup "ok" fields == Just (Bool True) -> Object (M.delete "ok" fields)
  Right value -> value

data CaptureCommand
  = BeginSaveLifecycle Lifecycle.Program
  | CapturePage Capture.Cursor
  | AcknowledgeCapture Capture.Cursor
  | ResumeCapture
  | CaptureState
  | ShutterSequence
  | CaptureWindow

emulatorCaptureCommand :: Emulator -> CaptureCommand -> IO Value
emulatorCaptureCommand emulator = bridgeWatchRpc exchange . request
  where
    exchange encoded = withMVar (emulatorLock emulator) $ \_ ->
      exchangeLockedReading emulator
        (Capture.readBoundedLine (emulatorCaptureReader emulator)) encoded
    request command = case command of
      BeginSaveLifecycle program -> object
        ["command" .= ("capture-observation-begin" :: T.Text), "scope" .= Lifecycle.scopeValue program]
      CapturePage cursor -> Capture.hitsRequest cursor
      AcknowledgeCapture cursor -> Capture.ackRequest cursor
      ResumeCapture -> Capture.resumeRequest
      CaptureState -> object ["command" .= ("capture-observation-state" :: T.Text)]
      ShutterSequence -> object ["command" .= ("shutter-sequence" :: T.Text)]
      CaptureWindow -> object ["command" .= ("capture-observation-window" :: T.Text)]

-- Caller holds emulatorLock.
rpcLocked :: Emulator -> Value -> IO (M.KeyMap Value)
rpcLocked = bridgeRpc . exchangeLocked

emulatorReleaseContext :: Emulator -> Maybe ReleaseSegment -> IO ()
emulatorReleaseContext emulator segment = do
  _ <- rpc emulator (object ["command" .= ("release-context" :: T.Text), "segment" .= segment])
  pure ()

bridgeRpc :: BridgeExchange -> Value -> IO (M.KeyMap Value)
bridgeRpc exchange request = do
  line <- exchange (encode request)
  value <- either (ioError . userError . ("invalid emulator response: " ++)) pure (eitherDecodeStrict' line)
  fields <- case value of Object result -> pure result; _ -> ioError (userError "emulator response must be an object")
  ok <- objectField "ok" fields
  case ok of
    Bool True -> pure (M.delete "ok" fields)
    Bool False -> do
      message <- objectField "message" fields
      ioError (userError ("emulator bridge: " ++ case message of String text -> T.unpack text; _ -> show message))
    _ -> ioError (userError "emulator response ok must be boolean")

-- | Starts the bridge in `directory`, offers its install checkpoints to `choose` and starts from
-- the chosen one, or else from the attach checkpoint or a cold boot.
startEmulator :: FilePath -> FilePath -> [String] -> ([InstallCheckpoint] -> IO (Maybe InstallCheckpoint, a)) -> IO (Emulator, EmulatorStart, a)
startEmulator python directory arguments choose = bracketOnError create stopEmulator $ \emulator -> do
  options <- event emulator "start-options"
  candidates <- objectField "installed" options >>= \value -> case value of
    Array items -> mapM installCheckpoint (foldr (:) [] items)
    _ -> ioError (userError "emulator start options must list install checkpoints")
  (chosen, extra) <- choose candidates
  send emulator (object ["command" .= String "start", "installed" .= fmap installDirectory chosen])
  fields <- event emulator "ready"
  record <- objectField "checkpoint" fields
  start <- case record of
    Object checkpoint -> objectField "start" checkpoint >>= \kind -> case kind of
      String "cold" -> pure ColdStart
      String "fresh-boot" -> pure FreshBootStart
      String "restored" -> pure RestoredStart
      String "installed" -> pure InstalledStart
      _ -> ioError (userError ("unknown emulator start: " ++ show kind))
    _ -> ioError (userError "emulator ready response lacks a checkpoint record")
  pure (emulator, EmulatorStart start record, extra)
  where
    create = do
      (Just input, Just output, _, process) <- createProcess (proc python arguments)
        { std_in = CreatePipe, std_out = CreatePipe, std_err = Inherit, cwd = Just directory }
      hSetBinaryMode input True
      hSetBinaryMode output True
      hSetBuffering input NoBuffering
      lock <- newMVar ()
      captureReader <- Capture.newBoundedReader output
      pure (Emulator input output process lock captureReader)
    event emulator name = do
      line <- C.hGetLine (emulatorOutput emulator)
      value <- either (ioError . userError . (("invalid emulator " ++ name ++ " event: ") ++)) pure (eitherDecodeStrict' line)
      case value of
        Object fields -> do
          ok <- objectField "ok" fields
          kind <- objectField "event" fields
          require (ok == Bool True && kind == String (T.pack name)) ("emulator did not send " ++ name)
          pure fields
        _ -> ioError (userError ("emulator " ++ name ++ " event must be an object"))
    send emulator request = do
      BL.hPutStr (emulatorInput emulator) (encode request)
      B.hPut (emulatorInput emulator) "\n"
      hFlush (emulatorInput emulator)
    installCheckpoint (Object fields) = do
      directoryValue <- textField "directory" fields
      baseText <- textField "base" fields
      base <- case baseText of
        '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure value
        _ -> ioError (userError ("invalid install checkpoint base: " ++ baseText))
      InstallCheckpoint directoryValue base
        <$> textField "manifest_sha256" fields
        <*> textField "runtime_sha256" fields
        <*> optionalTextField "procedure_sha256" fields
        <*> objectField "host" fields
    installCheckpoint _ = ioError (userError "install checkpoint must be an object")

-- | Saves the attached machine after INSTALL unless the same build's checkpoint exists; the
-- response says which.
emulatorSaveInstallCheckpoint :: Emulator -> Word32 -> String -> String -> Maybe String -> Value -> IO Value
emulatorSaveInstallCheckpoint emulator base manifest runtime procedure host = Object <$> rpc emulator (object
  ([ "command" .= String "save-install-checkpoint", "base" .= ("0x" ++ showHex base "")
   , "manifest_sha256" .= manifest, "runtime_sha256" .= runtime, "host" .= host ]
   ++ [ "procedure_sha256" .= value | Just value <- [procedure] ]))

-- | A bridge that answered a command with an error has left its command loop; the failed
-- `finish` is dropped so that error is the one reported.
stopEmulator :: Emulator -> IO ()
stopEmulator emulator = do
  _ <- try (rpc emulator (object ["command" .= String "finish"])) :: IO (Either IOException (M.KeyMap Value))
  hClose (emulatorInput emulator)
  hClose (emulatorOutput emulator)
  status <- waitForProcess (emulatorProcess emulator)
  require (status == ExitSuccess) ("emulator process failed: " ++ show status)

-- | The bytes of a lowercase hex field.
hexField :: String -> M.KeyMap Value -> IO B.ByteString
hexField name fields = objectField name fields >>= \value -> case value of
  String text -> do
    let encoded = TE.encodeUtf8 text
        nibble c | c >= 0x30 && c <= 0x39 = Just (c - 0x30)
                 | c >= 0x61 && c <= 0x66 = Just (c - 0x57)
                 | otherwise = Nothing
        byteAt i = (\high low -> high `shiftL` 4 .|. low) <$> nibble (B.index encoded (2 * i)) <*> nibble (B.index encoded (2 * i + 1))
        decoded = traverse byteAt [0 .. B.length encoded `div` 2 - 1]
    require (even (B.length encoded)) "emulator returned invalid lowercase hex"
    maybe (ioError (userError "emulator returned invalid lowercase hex")) (pure . B.pack) decoded
  _ -> ioError (userError ("invalid emulator hex " ++ name))

integerField :: String -> M.KeyMap Value -> IO Int
integerField name fields = objectField name fields >>= \value -> case fromJSON value of
  Success number -> pure number
  Error message -> ioError (userError ("invalid emulator integer " ++ name ++ ": " ++ message))

textField :: String -> M.KeyMap Value -> IO String
textField name fields = objectField name fields >>= \value -> case value of
  String text -> pure (T.unpack text)
  _ -> ioError (userError ("invalid emulator text " ++ name))

optionalTextField :: String -> M.KeyMap Value -> IO (Maybe String)
optionalTextField name fields = case M.lookup (K.fromString name) fields of
  Nothing -> pure Nothing
  Just (String text) -> pure (Just (T.unpack text))
  Just _ -> ioError (userError ("invalid emulator text " ++ name))

emulatorUsbApi :: Emulator -> UsbApi
emulatorUsbApi = bridgeUsbApi . exchangeLine

bridgeUsbApi :: BridgeExchange -> UsbApi
bridgeUsbApi exchange = UsbApi
  (\_ -> pure ())
  (\_ -> do
      _ <- bridgeRpc exchange (object ["command" .= String "list"])
      pure [Candidate "emulator:xm1" 0x04cb 0xff80 descriptors])
  (\candidate _ -> require (physicalPath candidate == "emulator:xm1") "unexpected emulator candidate")
  (\interface _ -> require (interface == 0) "unexpected emulator interface")
  transfer
  (\interface _ -> require (interface == 0) "unexpected emulator release interface")
  (\_ -> pure ())
  (\_ -> pure ())
  where
    descriptors = Descriptors 1 0 0 255 33 255 0x81 2 64 64 0 0 [(0x83,3,8,1)]
    transfer packet _ = do
      fields <- case packet of
        P.ControlWrite _ _ _ _ bytes -> bridgeRpc exchange (object
          ["command" .= String "transfer", "transfer" .= String "control", "bytes" .= P.hexBytes bytes])
        P.BulkOut endpoint bytes -> bridgeRpc exchange (object
          ["command" .= String "transfer", "transfer" .= String "bulk-out", "endpoint" .= endpoint, "bytes" .= P.hexBytes bytes])
        P.BulkIn endpoint count -> bridgeRpc exchange (object
          ["command" .= String "transfer", "transfer" .= String "bulk-in", "endpoint" .= endpoint, "count" .= count])
      count <- integerField "count" fields
      bytes <- B.unpack <$> hexField "bytes" fields
      pure (count, bytes)

emulatorTraceSites :: Emulator -> [(Word32, String)] -> IO ()
emulatorTraceSites emulator sites = do
  let address value = "0x" ++ showHex value ""
  _ <- rpc emulator (object ["command" .= String "trace-sites",
                             "sites" .= object [K.fromString (address site) .= label | (site,label) <- sites]])
  pure ()

-- | One recorded entry into a traced site: its label, the core, the guest instruction count and
-- r0-r3 at entry.
data TraceHit = TraceHit
  { traceSite :: String
  , traceCore :: Int
  , traceInstructions :: Integer
  , traceR0 :: Word32
  , traceR1 :: Word32
  , traceR2 :: Word32
  , traceR3 :: Word32
  } deriving (Eq, Show)

-- | The hit count so far and the hits recorded from index `start` on.
emulatorTraceHits :: Emulator -> Int -> IO (Int, [TraceHit])
emulatorTraceHits emulator start = do
  fields <- rpc emulator (object ["command" .= String "trace-hits", "from" .= start])
  count <- integerField "count" fields
  hits <- objectField "hits" fields >>= \value -> case value of
    Array items -> mapM hit (foldr (:) [] items)
    _ -> ioError (userError "emulator trace hits must be an array")
  pure (count, hits)
  where
    hit (Object item) = do
      site <- textField "site" item
      core <- integerField "core" item
      instructions <- objectField "instructions" item >>= \value -> case fromJSON value of
        Success number -> pure number
        Error message -> ioError (userError ("invalid emulator trace instruction count: " ++ message))
      registers <- objectField "registers" item >>= \value -> case value of
        Object registers -> pure registers
        _ -> ioError (userError "emulator trace hit registers must be an object")
      let register name = textField name registers >>= \text -> case text of
            '0' : 'x' : digits | [(word, "")] <- readHex digits -> pure word
            _ -> ioError (userError ("invalid emulator trace register " ++ name ++ ": " ++ text))
      TraceHit site core instructions <$> register "r0" <*> register "r1" <*> register "r2" <*> register "r3"
    hit _ = ioError (userError "emulator trace hit must be an object")

-- | The bridge's hit records from index `start` on, unchanged, with their `lr`, `sp` and `cpsr`.
emulatorTraceHitsRaw :: Emulator -> Int -> IO Value
emulatorTraceHitsRaw emulator start = do
  fields <- rpc emulator (object ["command" .= String "trace-hits", "from" .= start])
  objectField "hits" fields

-- | The bridge's trace-hits reply for at most `limit` recorded hits from index `start` on, undecoded.
emulatorTraceHitsPage :: Emulator -> Int -> Int -> IO Value
emulatorTraceHitsPage emulator start limit = Object <$> rpc emulator (object ["command" .= String "trace-hits", "from" .= start, "limit" .= limit])

-- | A key's hold and debounced-release cap in guest instructions, both positive.
data KeyPulse = KeyPulse { keyPulseHold :: Word32, keyPulseRelease :: Word32 } deriving (Eq, Show)

-- | A positive cap, in guest instructions, on the render drain before a key plus the settle after
-- it. Only the FujiMod bridge takes one.
newtype SettleBudget = SettleBudget Word32 deriving (Eq, Show)

-- | How a key is pressed; Nothing leaves the bridge's per-class pulse or its settle cap.
data KeyTiming = KeyTiming { keyTimingPulse :: Maybe KeyPulse, keyTimingSettle :: Maybe SettleBudget } deriving (Eq, Show)

defaultKeyTiming :: KeyTiming
defaultKeyTiming = KeyTiming Nothing Nothing

-- | Presses a key with the given timing.
emulatorKey :: Emulator -> KeyTiming -> String -> IO Value
emulatorKey emulator (KeyTiming pulse settle) key =
  Object <$> rpc emulator (object (["command" .= String "key", "key" .= key] ++ budget ++ cap))
  where
    budget = maybe [] (\(KeyPulse hold release) -> ["hold_instructions" .= hold, "release_instructions" .= release]) pulse
    cap = maybe [] (\(SettleBudget instructions) -> ["settle_instructions" .= instructions]) settle

-- | A mode-dial detent selected through its calibrated ADC window.
data ModeDial
  = ModeDialShutter | ModeDialProgram | ModeDialAperture | ModeDialManual | ModeDialAuto
  | ModeDialCustom | ModeDialSports | ModeDialLandscape | ModeDialPortrait
  | ModeDialSRAuto | ModeDialScene | ModeDialAdvanced
  deriving (Eq, Show, Enum, Bounded)

-- | The bridge's position name, the viewer's `ModeDialPosition` value.
modeDialName :: ModeDial -> String
modeDialName position = case position of
  ModeDialShutter -> "S"
  ModeDialProgram -> "P"
  ModeDialAperture -> "A"
  ModeDialManual -> "M"
  ModeDialAuto -> "AUTO"
  ModeDialCustom -> "C"
  ModeDialSports -> "Sports"
  ModeDialLandscape -> "Landscape"
  ModeDialPortrait -> "Portrait"
  ModeDialSRAuto -> "SR+"
  ModeDialScene -> "SP"
  ModeDialAdvanced -> "Adv."

emulatorModeDial :: Emulator -> ModeDial -> IO Value
emulatorModeDial emulator position = Object <$> rpc emulator (object
  ["command" .= String "mode-dial", "position" .= modeDialName position])

emulatorCapture :: Emulator -> String -> IO Value
emulatorCapture emulator label = Object <$> rpc emulator (object ["command" .= String "capture", "label" .= label])

emulatorWatchBegin :: Emulator -> [WatchRange] -> WatchLimit -> IO WatchAck
emulatorWatchBegin emulator ranges limit = do
  fields <- rpc emulator (object
    [ "command" .= String "watch-begin"
    , "ranges" .= [ [lo, hi] | range <- ranges, let (lo, hi) = watchRangeBounds range ]
    , "limit" .= watchLimitValue limit
    ])
  either (ioError . userError) pure (parseWatchAck (Object fields))

emulatorWatchObservation :: Emulator -> WatchLimit -> IO (Either String WatchObservation)
emulatorWatchObservation emulator limit = do
  fields <- rpc emulator (object ["command" .= String "watch-state"])
  pure (parseWatchObservation limit (Object fields))

emulatorExecutionWatchBegin :: Emulator -> [WatchRange] -> [WatchRange] -> WatchLimit -> IO Value
emulatorExecutionWatchBegin emulator protected forbidden limit = watchRpc emulator (object
  [ "command" .= String "watch-begin"
  , "ranges" .= bounds (unionWatchRanges (protected ++ forbidden))
  , "write_ranges" .= bounds protected
  , "fetch_ranges" .= bounds forbidden
  , "limit" .= watchLimitValue limit
  , "views" .= String "fixed" ])
  where
    bounds ranges = [[lo, hi] | range <- ranges, let (lo, hi) = watchRangeBounds range]

emulatorWatchStateRaw :: Emulator -> IO Value
emulatorWatchStateRaw emulator = watchRpc emulator (object ["command" .= String "watch-state"])

-- | The undecoded watch-state response line, for callers that store it before decoding.
emulatorWatchStateLine :: Emulator -> IO B.ByteString
emulatorWatchStateLine emulator = exchangeLine emulator (encode (object ["command" .= String "watch-state"]))

-- | Ends the armed memory watch and returns its final state.
emulatorWatchEnd :: Emulator -> IO Value
emulatorWatchEnd emulator = Object <$> rpc emulator (object ["command" .= String "watch-end"])


data ModalResponse = ModalResponse
  { modalRawResponse :: Value
  , modalParsedResponse :: Either String ModalReport
  }

emulatorModalObservationBegin :: Emulator -> ModalRequest -> IO ModalResponse
emulatorModalObservationBegin = bridgeModalObservationBegin . exchangeLine

bridgeModalObservationBegin :: BridgeExchange -> ModalRequest -> IO ModalResponse
bridgeModalObservationBegin exchange request = do
  response <- modalRpc exchange (modalRequestLimit request) (modalBeginRequestJson request)
  pure response { modalParsedResponse = do
    report <- modalParsedResponse response
    checkModalBegin request report
    pure report }

emulatorModalObservationReport :: Emulator -> ModalLimit -> IO ModalResponse
emulatorModalObservationReport = bridgeModalObservationReport . exchangeLine

bridgeModalObservationReport :: BridgeExchange -> ModalLimit -> IO ModalResponse
bridgeModalObservationReport exchange limit =
  modalRpc exchange limit (object ["command" .= String "modal-observation-report"])

emulatorModalObservationEnd :: Emulator -> ModalLimit -> IO ModalResponse
emulatorModalObservationEnd = bridgeModalObservationEnd . exchangeLine

bridgeModalObservationEnd :: BridgeExchange -> ModalLimit -> IO ModalResponse
bridgeModalObservationEnd exchange limit = do
  response <- modalRpc exchange limit (object ["command" .= String "modal-observation-end"])
  pure response { modalParsedResponse = do
    report <- modalParsedResponse response
    checkModalEnd report
    pure report }

modalRpc :: BridgeExchange -> ModalLimit -> Value -> IO ModalResponse
modalRpc exchange limit request = do
  raw <- bridgeWatchRpc exchange request
  pure (ModalResponse raw (parseModalReport limit raw))

emulatorPreservationBegin :: Emulator -> PreservationRanges -> IO PreservationAck
emulatorPreservationBegin emulator ranges = do
  fields <- rpc emulator (object
    [ "command" .= String "preservation-begin"
    , "ranges" .= preservationRangesJson ranges
    ])
  either (ioError . userError) pure (parsePreservationAck (Object fields))

emulatorPreservationState :: Emulator -> IO (Either String PreservationObservation)
emulatorPreservationState emulator = do
  fields <- rpc emulator (object ["command" .= String "preservation-state"])
  pure (parsePreservationObservation (Object fields))

emulatorMediaState :: Emulator -> IO (Either String MediaObservation)
emulatorMediaState emulator = do
  fields <- rpc emulator (object ["command" .= String "media-state"])
  pure (parseMediaObservation (Object fields))

emulatorReadMemory :: Emulator -> Word32 -> Int -> IO B.ByteString
emulatorReadMemory emulator address count = do
  require (count > 0 && count <= 65536 && toInteger address + toInteger count <= 0x100000000)
    "emulator memory read requires an address and 1..65536 bytes"
  withMVar (emulatorLock emulator) $ \_ -> B.concat <$> mapM readChunk [0,4096..count-1]
  where
    readChunk offset = do
      let chunkBytes = min 4096 (count - offset)
      fields <- rpcLocked emulator (object ["command" .= String "read-memory", "address" .= (address + fromIntegral offset), "count" .= chunkBytes])
      bytes <- hexField "bytes" fields
      require (B.length bytes == chunkBytes) "emulator memory read length mismatch"
      pure bytes

-- | A guest snapshot the bridge holds in memory under this name: 1..64 lowercase ASCII letters,
-- digits or '-'.
newtype SnapshotName = SnapshotName String deriving (Eq, Ord, Show)

-- | The firmware's current menu node id, its setting id and cursor; Nothing when no node is open.
type MenuReading = Maybe (Word32, Word32, Word32)

-- | One press from a held snapshot: the menu reading, the bridge's settle record and the guest
-- instructions it ran, or the strict stop the press reached.
data SnapshotPress
  = SnapshotPressed MenuReading Value Integer
  | SnapshotStopped String Integer
  deriving (Eq, Show)

emulatorSnapshotSave :: Emulator -> SnapshotName -> IO ()
emulatorSnapshotSave emulator (SnapshotName name) =
  () <$ rpc emulator (object ["command" .= String "snapshot-save", "name" .= name])

emulatorSnapshotRestore :: Emulator -> SnapshotName -> IO MenuReading
emulatorSnapshotRestore emulator (SnapshotName name) =
  rpc emulator (object ["command" .= String "snapshot-restore", "name" .= name]) >>= menuReading

emulatorSnapshotDrop :: Emulator -> SnapshotName -> IO ()
emulatorSnapshotDrop emulator (SnapshotName name) =
  () <$ rpc emulator (object ["command" .= String "snapshot-drop", "name" .= name])

-- | Restores the snapshot, then presses one bridge key name with the bridge's default pulse.
emulatorSnapshotKey :: Emulator -> SnapshotName -> String -> IO SnapshotPress
emulatorSnapshotKey emulator (SnapshotName name) key = do
  fields <- rpc emulator (object ["command" .= String "snapshot-key", "name" .= name, "key" .= key])
  instructions <- objectField "instructions" fields >>= \value -> case fromJSON value of
    Success count -> pure count
    Error message -> ioError (userError ("invalid snapshot-key instruction count: " ++ message))
  objectField "failure" fields >>= \failure -> case failure of
    Null -> SnapshotPressed <$> menuReading fields <*> objectField "settle" fields <*> pure instructions
    String message -> pure (SnapshotStopped (T.unpack message) instructions)
    _ -> ioError (userError "snapshot-key failure must be null or text")

-- | A physical press the bridge accepts; MENU is OK and DISP is BACK on this body.
data CrawlKey = KeyUp | KeyDown | KeyLeft | KeyRight | KeyOk | KeyBack | KeyQ | KeyDialLeft | KeyDialRight | KeyPlay
  deriving (Eq, Ord, Show, Enum, Bounded)

crawlKeyWire :: CrawlKey -> String
crawlKeyWire key = case key of
  KeyUp -> "up"
  KeyDown -> "down"
  KeyLeft -> "left"
  KeyRight -> "right"
  KeyOk -> "ok"
  KeyBack -> "back"
  KeyQ -> "q"
  KeyDialLeft -> "dial-left"
  KeyDialRight -> "dial-right"
  KeyPlay -> "play"

instance FromJSON CrawlKey where
  parseJSON = withText "crawl key" $ \text ->
    case lookup (T.unpack text) [(crawlKeyWire key, key) | key <- [minBound .. maxBound]] of
      Just key -> pure key
      Nothing -> fail ("unsupported crawl key: " ++ T.unpack text)

instance ToJSON CrawlKey where
  toJSON = toJSON . crawlKeyWire

-- | Presses one key from the current guest as `emulatorSnapshotKey` does after its restore; the
-- reply is undecoded.
emulatorDisplayKey :: Emulator -> CrawlKey -> IO Value
emulatorDisplayKey emulator key = Object <$> rpc emulator (object ["command" .= String "display-key", "key" .= crawlKeyWire key])

-- | Runs the guest for the given instructions with no input; the reply is undecoded.
emulatorIdle :: Emulator -> Instructions -> IO Value
emulatorIdle emulator instructions = Object <$> rpc emulator (object ["command" .= String "idle", "instructions" .= instructionsValue instructions])

menuReading :: M.KeyMap Value -> IO MenuReading
menuReading fields = objectField "menu" fields >>= \value -> case value of
  Object menu -> do
    let word name = objectField name menu >>= \field -> case field of
          Null -> pure Nothing
          _ -> case fromJSON field of
            Success number -> pure (Just number)
            Error message -> ioError (userError ("invalid menu " ++ name ++ ": " ++ message))
    reading <- (,,) <$> word "menu_id" <*> word "setting_id" <*> word "cursor"
    case reading of
      (Just node, Just setting, Just cursor) -> pure (Just (node, setting, cursor))
      (Nothing, Nothing, Nothing) -> pure Nothing
      _ -> ioError (userError "menu reading is partially null")
  Null -> pure Nothing
  _ -> ioError (userError "menu reading must be an object")
