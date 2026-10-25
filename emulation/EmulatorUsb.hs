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
  ) where

import Control.Concurrent.MVar
import Control.Exception (IOException, bracketOnError, try)
import Control.Monad (unless)
import Data.Aeson (Result (..), Value (..), eitherDecodeStrict', encode, fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
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
exchangeLocked emulator request = do
  BL.hPutStr (emulatorInput emulator) request
  B.hPut (emulatorInput emulator) "\n"
  hFlush (emulatorInput emulator)
  C.hGetLine (emulatorOutput emulator)

rpc :: Emulator -> Value -> IO (M.KeyMap Value)
rpc = bridgeRpc . exchangeLine

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
    Bool True -> pure fields
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
      pure (Emulator input output process lock)
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
