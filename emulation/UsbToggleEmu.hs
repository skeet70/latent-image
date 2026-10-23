{-# LANGUAGE OverloadedStrings #-}
-- | Full-system emulator runs E2-E7 of reports/usb-personality-toggle.md
-- section 8.2. The retained Python bridge (@continue_rec_scanout.py
-- --toggle-server@) supplies primitive commands; scenarios, the expected
-- behaviour (the table of section 3.2) and the receipts are here.
module Main (main) where

import Control.Exception (Exception, IOException, SomeException, catch, displayException, fromException, onException, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import Data.Aeson.Types (Pair)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import Data.Bits (shiftL, testBit, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isHexDigit)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (find, isPrefixOf)
import Data.Maybe (fromMaybe, isJust, isNothing, listToMaybe)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word8)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, stageViewerRuntime, validateFirmware, workspace)
import EmulatorUsb (BridgeExchange, bridgeUsbApi)
import qualified FujiAF.Broker as P
import FujiAF.Read (ReadWindow (..), decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame)
import FujiAF.Transport (UsbApi (..))
import FujiAF.UsbToggle (extentLength, extentStart, hookSite)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure, exitWith)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), Handle, IOMode (WriteMode), hClose, hFlush, hPutStrLn, hSetBinaryMode, hSetBuffering, openFile, stderr, stdout)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (..), callProcess, createProcess, proc, readProcess, terminateProcess, waitForProcess)

-- * Inputs

data DatRole = StockDat | CcBaseDat | CcToggleDat | Rc0ToggleDat deriving (Eq, Show)

data NandRole = Stored0 | Stored1 | Stored0Pid02b6 deriving (Eq, Show)

newtype Sha256 = Sha256 String deriving (Eq, Show)

datName :: DatRole -> String
datName StockDat = "stock"
datName CcBaseDat = "cc-base"
datName CcToggleDat = "cc-toggle"
datName Rc0ToggleDat = "rc0-toggle"

datPath :: DatRole -> FilePath
datPath StockDat = "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
datPath CcBaseDat = "private-data/analysis/classic-chrome-patch/candidate-pinned-dat/FWUP0002.DAT"
datPath CcToggleDat = "private-data/analysis/usb-toggle/cc-pinned-dat/FWUP0002.DAT"
datPath Rc0ToggleDat = "private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT"

datSha256 :: DatRole -> Sha256
datSha256 StockDat = Sha256 "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
datSha256 CcBaseDat = Sha256 "16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89"
datSha256 CcToggleDat = Sha256 "507ecf326119110713c34f15bb07d0f38bd6946127011835980f6e4d39ad0e1e"
datSha256 Rc0ToggleDat = Sha256 "10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445"

datHasToggle :: DatRole -> Bool
datHasToggle role = role `elem` [CcToggleDat, Rc0ToggleDat]

nandName :: NandRole -> String
nandName Stored0 = "stored-0"
nandName Stored1 = "stored-1"
nandName Stored0Pid02b6 = "stored-0-pid02b6"

-- | f7-0 is f7-1 with adjustment byte 0xf7 of page 0x381*64 restored to 00;
-- f7-0-pid02b6 is f7-0 with bytes 0x102..0x103 set to the body's b6 02.
nandPath :: NandRole -> FilePath
nandPath Stored0 = "private-data/analysis/usb-toggle/emu/inputs/f7-0.nand"
nandPath Stored1 = "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
nandPath Stored0Pid02b6 = "private-data/analysis/usb-toggle/emu/inputs/f7-0-pid02b6.nand"

nandSha256 :: NandRole -> Sha256
nandSha256 Stored0 = Sha256 "8e2b524355dee955745da8d85295bb86ef5b99cc4ef7bd2cf5be498eb479478b"
nandSha256 Stored1 = Sha256 "baf475358f1b8ec5b619fd4891cd32a1c8225c0d9341e60318031106a4354aa0"
nandSha256 Stored0Pid02b6 = Sha256 "633944533751ff6732f3714d71c13199bc3884e6b8d1c9ee1348042839382e04"

storedF7 :: NandRole -> Word8
storedF7 Stored1 = 1
storedF7 _ = 0

-- * Firmware sites and cells (X-M1 1.50)

data Site
  = HandlerA | F7Read | F7Used | AttachArm | DetachArm | ApplicationArm
  | Post1705 | Post1706 | ApplicationEvent | ServiceStart | TransportStart | Fill
  | AttachChoice | ModeRequest | StubEntry | KeyStateQ | DebugDispatch
  | HandlerB | StartupBarrier | ConnectObserver
  deriving (Eq, Show, Enum, Bounded)

siteAddress :: Site -> Word32
siteAddress site = case site of
  HandlerA -> 0x00ecf8b8
  F7Read -> hookSite
  F7Used -> hookSite + 4
  AttachArm -> 0x00ecf93c
  DetachArm -> 0x00ecf964
  ApplicationArm -> 0x00ecf99c
  Post1705 -> 0x00ed117c
  Post1706 -> 0x00ed11a4
  ApplicationEvent -> 0x00ecf894
  ServiceStart -> 0x00ecc104
  TransportStart -> 0x00d82478
  Fill -> 0x007221a0
  AttachChoice -> 0x00d0afe0
  ModeRequest -> 0x00d350c4
  StubEntry -> extentStart
  KeyStateQ -> 0x006a0d00
  DebugDispatch -> 0x00d803cc
  HandlerB -> 0x00ecf9c8
  StartupBarrier -> 0x00ec8014
  ConnectObserver -> 0x00ecfe64

-- | The r0 a site records; other entries are dropped by the bridge.
siteR0 :: Site -> Maybe Word32
siteR0 Fill = Just fillStart
siteR0 KeyStateQ = Just keyQ
siteR0 _ = Nothing

siteLabel :: Site -> T.Text
siteLabel site = case site of
  HandlerA -> "handler-a"
  F7Read -> "f7-read"
  F7Used -> "f7-used"
  AttachArm -> "attach-arm"
  DetachArm -> "detach-arm"
  ApplicationArm -> "application-arm"
  Post1705 -> "post-1705"
  Post1706 -> "post-1706"
  ApplicationEvent -> "application-event"
  ServiceStart -> "service-start"
  TransportStart -> "transport-start"
  Fill -> "fill"
  AttachChoice -> "attach-choice"
  ModeRequest -> "mode-request"
  StubEntry -> "stub"
  KeyStateQ -> "key-state-q"
  DebugDispatch -> "debug-dispatch"
  HandlerB -> "handler-b"
  StartupBarrier -> "startup-barrier"
  ConnectObserver -> "connect-observer"

armSites :: [Site]
armSites = [AttachArm, DetachArm, ApplicationArm]

keyQ, fillStart, fillLast :: Word32
keyQ = 0x32
fillStart = 0x43360000
fillLast = 0x45a9fffc

cachePointerCell, systemModeCell, transportModeCell, descriptorModeCell, vendorTableCell, latchCell :: Word32
cachePointerCell = 0x0096ca50
systemModeCell = 0x009570dc
transportModeCell = 0x0146a0c0
descriptorModeCell = 0x01469810
vendorTableCell = 0x0146a0cc
latchCell = 0x015c229c

-- | Transport driver state and status words (0x01469cd0 + 0x4b0, + 0x3c0).
driverStateCell, driverStatusCell :: Word32
driverStateCell = 0x0146a180
driverStatusCell = 0x0146a090

-- | KEY_Q condition row 19 of the table at 0x00828120 (28-byte stride).
qRow :: Word32
qRow = 0x00828334

-- | UI key bitmap word holding Q at bit 18.
uiKeyWord :: Word32
uiKeyWord = 0x0094f8bc

-- | usbcont (task 7) stack base pointer and size; the stack is filled with 0x07.
usbcontStackCell :: Word32
usbcontStackCell = 0x40a5b234

serviceVendorTable :: Word32
serviceVendorTable = 0x00d80cf0

-- | The linked stub occupies 22 words from the extent start.
stubBytes :: Word32
stubBytes = 22 * 4

-- | Function-2 read window of the service session.
ramReadAddress :: Word32
ramReadAddress = 0x015c22a0

ramReadCount :: Int
ramReadCount = 32

-- * Bridge process

data Stop = Stop String Value deriving (Show)

instance Exception Stop

data Server = Server
  { srvIn :: Handle
  , srvOut :: Handle
  , srvProcess :: ProcessHandle
  , srvLast :: IORef B.ByteString
  , srvHits :: IORef Int
  }

exchangeLine :: Server -> BridgeExchange
exchangeLine srv request = do
  BL.hPutStr (srvIn srv) request
  B.hPut (srvIn srv) "\n"
  hFlush (srvIn srv)
  line <- C.hGetLine (srvOut srv)
  writeIORef (srvLast srv) line
  pure line

decodeObject :: B.ByteString -> IO (M.KeyMap Value)
decodeObject line = case eitherDecodeStrict' line of
  Right (Object fields) -> pure fields
  Right _ -> ioError (userError "bridge line is not a JSON object")
  Left problem -> ioError (userError ("invalid bridge line: " ++ problem))

failure :: M.KeyMap Value -> Stop
failure fields = Stop (maybe "bridge refused" render (M.lookup "message" fields)) (fromMaybe Null (M.lookup "boundary" fields))
  where render (String text) = T.unpack text
        render other = show other

call :: Server -> T.Text -> [Pair] -> IO (M.KeyMap Value)
call srv command fields = do
  line <- exchangeLine srv (encode (object (("command" .= command) : fields)))
  reply <- decodeObject line
  case M.lookup "ok" reply of
    Just (Bool True) -> pure reply
    _ -> throwIO (failure reply)

field :: String -> M.KeyMap Value -> Value
field name = fromMaybe Null . M.lookup (K.fromString name)

intField :: String -> M.KeyMap Value -> IO Integer
intField name fields = case field name fields of
  Number n | Just value <- toBoundedInteger n -> pure (toInteger (value :: Int))
  other -> ioError (userError ("bridge field " ++ name ++ " is not an integer: " ++ show other))

textField :: String -> M.KeyMap Value -> IO String
textField name fields = case field name fields of
  String text -> pure (T.unpack text)
  other -> ioError (userError ("bridge field " ++ name ++ " is not text: " ++ show other))

hexDecode :: String -> IO B.ByteString
hexDecode text = do
  require (even (length text) && all isHexDigit text) "bridge returned invalid hex"
  pure (B.pack (pairs text))
  where pairs (a : b : rest) = fromIntegral (16 * digitToInt a + digitToInt b) : pairs rest
        pairs _ = []

-- | How a scenario's machine reaches its first scenario step.
data BootKind
  = RouteBoot
  | ProbeBoot
  | CableAtReset QAtReset
  deriving (Eq, Show)

data QAtReset = NoQ | QFromReset deriving (Eq, Show)

data Env = Env
  { envRoot :: FilePath
  , envPython :: FilePath
  , envRuntime :: FilePath
  , envCheckpoints :: FilePath
  , envFresh :: Bool
  , envReplaced :: IORef [String]
  }

data Started = Started
  { startKey :: String
  , startInputs :: Value
  , startKind :: String
  , startBoot :: Value
  }

-- | Starts the bridge for DAT and NAND. A route boot restores the keyed
-- checkpoint when one exists, else cold-boots, drives the first-start route
-- and saves it; the other boots always start cold and save nothing.
startServer :: Env -> DatRole -> NandRole -> BootKind -> Maybe Int -> FilePath -> IO (Server, Started)
startServer env dat nand boot qSample output = do
  datFile <- canonicalizePath (envRoot env </> datPath dat)
  nandFile <- canonicalizePath (envRoot env </> nandPath nand)
  let resetArguments = case boot of
        CableAtReset NoQ -> ["--cable-at-reset"]
        CableAtReset QFromReset -> ["--cable-at-reset", "--adc-at-reset", "9=" ++ show (fromMaybe 0 qSample)]
        ProbeBoot -> ["--access-probe-range", hex32 extentStart ++ ":" ++ hex32 (extentStart + fromIntegral extentLength)]
        RouteBoot -> []
      arguments =
        [ "tools/continue_rec_scanout.py"
        , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
        , "--installed-dat", datFile
        , "--adjustment-nand", nandFile
        , "--menus", "--toggle-server"
        , "--output", output
        , "--toggle-checkpoints", envCheckpoints env
        ] ++ resetArguments
  errors <- openFile (output ++ ".stderr") WriteMode
  (Just input, Just out, _, process) <- createProcess (proc (envPython env) arguments)
    { std_in = CreatePipe, std_out = CreatePipe, std_err = UseHandle errors, cwd = Just (envRuntime env) }
  hSetBinaryMode input True
  hSetBinaryMode out True
  lastLine <- newIORef B.empty
  hitIndex <- newIORef 0
  let srv = Server input out process lastLine hitIndex
  prepare srv `onException` (terminateProcess process >> waitForProcess process)
  where
    prepare srv = do
      options <- C.hGetLine (srvOut srv) >>= decodeObject
      key <- textField "key" options
      let inputs = field "inputs" options
          shaOf name = case inputs of
            Object o -> case field name o of String s -> Sha256 (T.unpack s); _ -> Sha256 ""
            _ -> Sha256 ""
      require (shaOf "installed_dat" == datSha256 dat) (datName dat ++ " DAT differs from its pinned SHA-256")
      require (shaOf "adjustment_nand" == nandSha256 nand) (nandName nand ++ " NAND differs from its pinned SHA-256")
      replaced <- readIORef (envReplaced env)
      let saved = field "checkpoint" options /= Null
          fresh = envFresh env && key `notElem` replaced
          restore = boot == RouteBoot && saved && not fresh
      ready <- exchangeLine srv (encode (object ["command" .= ("start" :: T.Text), "restore" .= restore])) >>= decodeObject
      require (field "ok" ready == Bool True) "bridge did not become ready"
      kind <- textField "start" ready
      armTraces srv
      bootRecord <- case boot of
        RouteBoot | kind == "cold" -> do
          route <- firstStartRoute srv
          saveReply <- call srv "save-checkpoint" ["replace" .= saved]
          modifyIORef' (envReplaced env) (key :)
          pure (object ["route" .= route, "checkpoint" .= Object saveReply])
        RouteBoot -> pure (object ["route" .= ("restored at the shooting screen" :: T.Text)])
        ProbeBoot -> object . (: []) . ("route" .=) <$> firstStartRoute srv
        CableAtReset _ -> do
          reply <- call srv "run" ["instructions" .= (140000000 :: Int)]
          pure (object ["route" .= ("cable at reset; 140000000 instructions without keys" :: T.Text), "run" .= Object reply])
      pure (srv, Started key inputs kind bootRecord)

-- | First-start route to the shooting screen: language and date prompts. The
-- prompts need an 8,000,000-instruction hold and release cap per key.
firstStartRoute :: Server -> IO Value
firstStartRoute srv = do
  _ <- call srv "run" ["instructions" .= (100000000 :: Int)]
  menus <- forM (replicate 12 "up" ++ ["ok", "ok"] :: [T.Text]) $ \key -> do
    reply <- call srv "key" ["key" .= key, "hold_instructions" .= firstStartPulse, "release_instructions" .= firstStartPulse]
    pure (field "menu" reply)
  _ <- call srv "run" ["instructions" .= (40000000 :: Int)]
  state <- call srv "state" []
  pure (object ["keys" .= (replicate 12 ("up" :: T.Text) ++ ["ok", "ok"]), "menus" .= menus, "state" .= Object state])

firstStartPulse :: Int
firstStartPulse = 8000000

armTraces :: Server -> IO ()
armTraces srv = do
  let site s = object (["address" .= siteAddress s, "label" .= siteLabel s] ++ maybe [] (\r0 -> ["r0" .= r0]) (siteR0 s))
  _ <- call srv "trace-sites" ["sites" .= map site [minBound .. maxBound :: Site]]
  pure ()

stopServer :: Server -> IO ()
stopServer srv = do
  _ <- (Just <$> call srv "finish" []) `catch` \(_ :: SomeException) -> pure Nothing
  hClose (srvIn srv)
  hClose (srvOut srv)
  _ <- waitForProcess (srvProcess srv)
  pure ()

-- * Observation

data Hit = Hit
  { hitSite :: Site
  , hitR0 :: Word32
  , hitR1 :: Word32
  , hitSp :: Word32
  , hitLr :: Word32
  , hitAt :: Integer
  }

hitValue :: Hit -> Value
hitValue hit = object
  [ "site" .= siteLabel (hitSite hit), "r0" .= hex32 (hitR0 hit), "r1" .= hex32 (hitR1 hit)
  , "sp" .= hex32 (hitSp hit), "lr" .= hex32 (hitLr hit), "instructions" .= hitAt hit ]

newHits :: Server -> IO [Hit]
newHits srv = do
  from <- readIORef (srvHits srv)
  reply <- call srv "trace-hits" ["from" .= from]
  count <- intField "count" reply
  writeIORef (srvHits srv) (fromInteger count)
  items <- case field "hits" reply of
    Array values -> pure (V.toList values)
    _ -> ioError (userError "trace hits must be an array")
  forM items $ \item -> case item of
    Object o -> do
      label <- textField "label" o
      site <- maybe (ioError (userError ("unknown trace label " ++ label))) pure
        (find ((== T.pack label) . siteLabel) [minBound .. maxBound])
      registers <- case field "registers" o of Object r -> pure r; _ -> ioError (userError "trace registers")
      let reg name = fromInteger <$> intField name registers
      Hit site <$> reg "r0" <*> reg "r1" <*> reg "sp" <*> reg "lr" <*> intField "instructions" o
    _ -> ioError (userError "trace hit must be an object")

readMemory :: Server -> Word32 -> Int -> IO B.ByteString
readMemory srv address count = do
  reply <- call srv "read-memory" ["address" .= address, "count" .= count]
  bytes <- textField "bytes" reply >>= hexDecode
  require (B.length bytes == count) "memory read length mismatch"
  pure bytes

le :: B.ByteString -> Integer
le = foldr (\byte acc -> acc `shiftL` 8 .|. fromIntegral byte) 0 . B.unpack

word32At :: Server -> Word32 -> IO Word32
word32At srv address = fromInteger . le <$> readMemory srv address 4

byteAt :: Server -> Word32 -> IO Word8
byteAt srv address = B.head <$> readMemory srv address 1

data Cells = Cells
  { cellF7 :: Word8
  , cellSystemMode :: Word32
  , cellTransportMode :: Word32
  , cellDescriptorMode :: Word8
  , cellVendorTable :: Word32
  , cellLatch :: Word8
  , cellQStable :: Word8
  , cellQDebounced :: Word8
  , cellQRaw :: Word8
  , cellUiQ :: Bool
  , cellCache100 :: B.ByteString
  , cellCache13e :: Word16
  , cellDriverState :: Word32
  , cellDriverStatus :: Word32
  , cellFillFirst :: Word32
  , cellFillLast :: Word32
  }

readCells :: Server -> IO Cells
readCells srv = do
  cache <- word32At srv cachePointerCell
  f7 <- byteAt srv (cache + 0xf7)
  c100 <- readMemory srv (cache + 0x100) 4
  c13e <- fromInteger . le <$> readMemory srv (cache + 0x13e) 2
  row <- readMemory srv (qRow + 0x14) 3
  ui <- word32At srv uiKeyWord
  Cells f7
    <$> word32At srv systemModeCell
    <*> word32At srv transportModeCell
    <*> byteAt srv descriptorModeCell
    <*> word32At srv vendorTableCell
    <*> byteAt srv latchCell
    <*> pure (B.index row 0) <*> pure (B.index row 1) <*> pure (B.index row 2)
    <*> pure (testBit ui 18)
    <*> pure c100 <*> pure c13e
    <*> word32At srv driverStateCell
    <*> word32At srv driverStatusCell
    <*> word32At srv fillStart
    <*> word32At srv fillLast

cellsValue :: Cells -> Value
cellsValue c = object
  [ "cache_f7" .= cellF7 c, "system_mode" .= cellSystemMode c
  , "transport_mode_0x0146a0c0" .= cellTransportMode c, "descriptor_mode_0x01469810" .= cellDescriptorMode c
  , "vendor_table_0x0146a0cc" .= hex32 (cellVendorTable c), "latch_0x015c229c" .= cellLatch c
  , "q_row_19" .= object ["stable_0x14" .= cellQStable c, "debounced_0x15" .= cellQDebounced c, "raw_0x16" .= cellQRaw c]
  , "ui_bitmap_q_bit" .= cellUiQ c
  , "cache_0x100" .= hexBytes (cellCache100 c), "cache_0x13e" .= hex16 (cellCache13e c)
  , "driver_state_0x0146a180" .= hex32 (cellDriverState c), "driver_status_0x0146a090" .= hex32 (cellDriverStatus c)
  , "fill_first_word" .= hex32 (cellFillFirst c), "fill_last_word" .= hex32 (cellFillLast c) ]

hex32 :: Word32 -> String
hex32 value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

hex16 :: Word16 -> String
hex16 value = "0x" ++ replicate (4 - length digits) '0' ++ digits where digits = showHex value ""

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\byte -> let d = showHex byte "" in replicate (2 - length d) '0' ++ d) . B.unpack

-- * Recording

data Check = Check T.Text Bool Value Value

data Outcome = Completed | Blocked String Value

data Recorder = Recorder
  { recSteps :: IORef [Value]
  , recChecks :: IORef [Check]
  , recCapture :: IORef Int
  }

newRecorder :: IO Recorder
newRecorder = Recorder <$> newIORef [] <*> newIORef [] <*> newIORef 0

check :: Recorder -> T.Text -> Bool -> Value -> Value -> IO ()
check recorder name passed expected observed = modifyIORef' (recChecks recorder) (++ [Check name passed expected observed])

equal :: (Eq a, ToValue a) => Recorder -> T.Text -> a -> a -> IO ()
equal recorder name expected observed = check recorder name (expected == observed) (toValue expected) (toValue observed)

class ToValue a where
  toValue :: a -> Value

instance ToValue Bool where toValue = Bool
instance ToValue Word8 where toValue = Number . fromIntegral
instance ToValue Word32 where toValue = String . T.pack . hex32
instance ToValue Int where toValue = Number . fromIntegral
instance ToValue T.Text where toValue = String
instance ToValue a => ToValue [a] where toValue = Array . V.fromList . map toValue
instance (ToValue a, ToValue b) => ToValue (a, b) where toValue (a, b) = Array (V.fromList [toValue a, toValue b])

step :: Recorder -> Value -> IO ()
step recorder value = modifyIORef' (recSteps recorder) (++ [value])

-- | A frame capture, or the bridge's refusal (no display plane in USB modes).
capture :: Server -> Recorder -> String -> IO Value
capture srv recorder label = do
  index <- readIORef (recCapture recorder)
  writeIORef (recCapture recorder) (index + 1)
  (Object <$> call srv "capture" ["label" .= (show index ++ "-" ++ label)])
    `catch` \(Stop message _) -> pure (object ["refused" .= message])

-- * The behaviour table (reports/usb-personality-toggle.md section 3.2)

data Hold = Plain | HoldQ deriving (Eq, Show)

data Personality = Service | Application deriving (Eq, Show)

data Model = Model
  { modelToggle :: Bool
  , modelF7 :: Word8
  , modelSystemMode :: Word32
  , modelPersonality :: Maybe Personality
  }

excludedMode :: Word32 -> Bool
excludedMode mode = mode `elem` [5, 7]

-- | The RAM byte after an attach edge and the arm handler A takes.
attachOutcome :: Model -> Hold -> (Word8, Personality)
attachOutcome model hold =
  let flips = modelToggle model && hold == HoldQ && not (excludedMode (modelSystemMode model))
      f7 = if flips then (if modelF7 model == 0 then 1 else 0) else modelF7 model
   in (f7, if f7 /= 0 && not (excludedMode (modelSystemMode model)) then Service else Application)

holdName :: Hold -> String
holdName Plain = "plain"
holdName HoldQ = "q"

-- * Steps

data Step = Attach Hold | Detach | Enumerate deriving (Eq, Show)

data Ctx = Ctx
  { ctxServer :: Server
  , ctxRecorder :: Recorder
  , ctxModel :: IORef Model
  , ctxDat :: DatRole
  , ctxHits :: IORef [Hit]
  }

runInstructions :: Server -> Int -> [Site] -> IO (Maybe T.Text, Integer)
runInstructions srv count stops = do
  reply <- call srv "run" ["instructions" .= count, "stop_on" .= map siteLabel stops]
  at <- intField "instructions" reply
  pure (case field "stopped_on" reply of String label -> Just label; _ -> Nothing, at)

-- | Runs in 10M-instruction rounds until PREDICATE holds on the cells.
settle :: Server -> Int -> (Cells -> Bool) -> IO (Bool, Cells)
settle srv rounds predicate = do
  cells <- readCells srv
  if predicate cells || rounds <= 0
    then pure (predicate cells, cells)
    else runInstructions srv 10000000 [] >> settle srv (rounds - 1) predicate

collect :: Ctx -> IO [Hit]
collect ctx = do
  hits <- newHits (ctxServer ctx)
  modifyIORef' (ctxHits ctx) (++ hits)
  pure hits

sitesOf :: Site -> [Hit] -> [Hit]
sitesOf site = filter ((== site) . hitSite)

qState :: Server -> IO Value
qState srv = do
  cells <- readCells srv
  state <- call srv "state" []
  pure (object ["q_debounced_0x15" .= cellQDebounced cells, "ui_bitmap_q_bit" .= cellUiQ cells
               , "asserted_keys" .= field "asserted_keys" state, "menu" .= field "menu" state])

attach :: Ctx -> Int -> Hold -> IO ()
attach ctx index hold = do
  let srv = ctxServer ctx
      recorder = ctxRecorder ctx
      name = T.pack ("step-" ++ show index ++ "-attach-" ++ holdName hold)
      named suffix = name <> ": " <> suffix
  model <- readIORef (ctxModel ctx)
  let (expectedF7, personality) = attachOutcome model hold
  _ <- collect ctx
  pressed <- if hold == HoldQ
    then do
      press <- call srv "press" ["key" .= ("q" :: T.Text)]
      _ <- runInstructions srv 4000000 []
      cells <- readCells srv
      equal recorder (named "Q row 19 +0x15 before the edge") (1 :: Word8) (cellQDebounced cells)
      shot <- capture srv recorder "q-held"
      state <- qState srv
      pure (Just (object ["adc_sample" .= field "sample" press, "before_edge" .= state, "capture" .= shot]))
    else pure Nothing
  before <- readCells srv
  _ <- call srv "cable" ["connected" .= True]
  (armStop, _) <- runInstructions srv 60000000 armSites
  _ <- runInstructions srv 5000000 []
  edgeHits <- collect ctx
  (started, _) <- runInstructions srv 150000000 [TransportStart]
  (settled, afterStart) <- settle srv 10 $ \c -> case personality of
    Service -> cellTransportMode c == 3 && cellDescriptorMode c == 3 && cellVendorTable c == serviceVendorTable && cellFillLast c == 0xffffffff
    Application -> cellTransportMode c == 2 && cellDescriptorMode c == 2
  startHits <- collect ctx
  released <- if hold == HoldQ
    then do
      _ <- call srv "release" []
      _ <- runInstructions srv 8000000 []
      cells <- readCells srv
      equal recorder (named "Q row 19 +0x15 after release") (0 :: Word8) (cellQDebounced cells)
      shot <- capture srv recorder "q-released"
      state <- qState srv
      _ <- runInstructions srv 32000000 []
      laterShot <- capture srv recorder "q-released-40m"
      laterState <- qState srv
      pure (Just (object ["after_release" .= state, "capture" .= shot
                         , "after_release_40m" .= laterState, "capture_40m" .= laterShot]))
    else pure Nothing
  after <- readCells srv
  let hits = edgeHits ++ startHits
      arm = listToMaybe [hitSite h | h <- hits, hitSite h `elem` armSites]
      reads' = sitesOf F7Read hits
      used = sitesOf F7Used hits
      expectedArm = if personality == Service then AttachArm else ApplicationArm
  equal recorder (named "handler A arm") (siteLabel expectedArm) (maybe "none" siteLabel arm)
  equal recorder (named "attach bit r1 at 0x00ecf900") [1 :: Word32] (map hitR1 reads')
  equal recorder (named "value handler A tests at 0x00ecf904") [fromIntegral expectedF7 :: Word32] (map hitR0 used)
  equal recorder (named "cache[0xf7]") expectedF7 (cellF7 after)
  equal recorder (named "stub entries per 0x00ecf900 read")
    (if datHasToggle (ctxDat ctx) then length reads' else 0) (length (sitesOf StubEntry hits))
  case personality of
    Service -> do
      equal recorder (named "queue-7 0x1705 post (r0, r1)") [(7, 0) :: (Word32, Word32)] [(hitR0 h, hitR1 h) | h <- sitesOf Post1705 hits]
      equal recorder (named "service start 0x00ecc104") True (not (null (sitesOf ServiceStart hits)))
      equal recorder (named "fill 0x007221a0(0x43360000, ...)") True (not (null (sitesOf Fill hits)))
      equal recorder (named "transport start mode r1") [3 :: Word32] (map hitR1 (sitesOf TransportStart hits))
      equal recorder (named "0x0146a0c0") (3 :: Word32) (cellTransportMode after)
      equal recorder (named "0x01469810") (3 :: Word8) (cellDescriptorMode after)
      equal recorder (named "0x0146a0cc") serviceVendorTable (cellVendorTable after)
      equal recorder (named "application events") ([] :: [Word32]) (map hitR0 (sitesOf ApplicationEvent hits))
    Application -> do
      equal recorder (named "application events") [0x10 :: Word32] (map hitR0 (sitesOf ApplicationEvent hits))
      equal recorder (named "queue-7 0x1705 posts") (0 :: Int) (length (sitesOf Post1705 hits))
      equal recorder (named "service starts") (0 :: Int) (length (sitesOf ServiceStart hits))
      equal recorder (named "fills") (0 :: Int) (length (sitesOf Fill hits))
      equal recorder (named "0x0146a0c0") (2 :: Word32) (cellTransportMode after)
      equal recorder (named "0x01469810") (2 :: Word8) (cellDescriptorMode after)
  step recorder (object
    [ "step" .= name, "kind" .= ("attach" :: T.Text), "hold" .= holdName hold
    , "expected" .= object ["cache_f7" .= expectedF7, "personality" .= show personality]
    , "q_pressed" .= pressed, "q_released" .= released
    , "arm_stop" .= armStop, "transport_start_stop" .= started, "settled" .= settled
    , "cells_before" .= cellsValue before, "cells_after_start" .= cellsValue afterStart, "cells_after" .= cellsValue after
    , "hits" .= map hitValue hits ])
  writeIORef (ctxModel ctx) model {modelF7 = expectedF7, modelPersonality = Just personality}

-- | A detach edge; Nothing when handler A saw no edge in the budget.
detach :: Ctx -> Int -> IO (Maybe ())
detach ctx index = do
  let srv = ctxServer ctx
      recorder = ctxRecorder ctx
      name = T.pack ("step-" ++ show index ++ "-detach")
      named suffix = name <> ": " <> suffix
  model <- readIORef (ctxModel ctx)
  _ <- collect ctx
  before <- readCells srv
  _ <- call srv "cable" ["connected" .= False]
  (armStop, _) <- runInstructions srv 80000000 armSites
  _ <- when (isJust armStop) (() <$ runInstructions srv 10000000 [])
  hits <- collect ctx
  after <- readCells srv
  let service = modelF7 model /= 0 && not (excludedMode (modelSystemMode model))
      expectedArm = if service then DetachArm else ApplicationArm
      record extra = step recorder (object (
        [ "step" .= name, "kind" .= ("detach" :: T.Text), "arm_stop" .= armStop
        , "cells_before" .= cellsValue before, "cells_after" .= cellsValue after
        , "hits" .= map hitValue hits ] ++ extra))
  if isNothing armStop
    then do
      check recorder (named "handler A sees the detach edge") False (String "an arm within 80000000 instructions") (String "no arm")
      record ["boundary" .= detachBoundary after hits]
      pure Nothing
    else do
      equal recorder (named "handler A arm") (siteLabel expectedArm) (fromMaybe "none" armStop)
      equal recorder (named "attach bit r1 at 0x00ecf900") [0 :: Word32] (map hitR1 (sitesOf F7Read hits))
      equal recorder (named "cache[0xf7] unchanged") (modelF7 model) (cellF7 after)
      if service
        then do
          equal recorder (named "queue-7 0x1706 post (r0, r1)") [(7, 0) :: (Word32, Word32)] [(hitR0 h, hitR1 h) | h <- sitesOf Post1706 hits]
          equal recorder (named "application events") ([] :: [Word32]) (map hitR0 (sitesOf ApplicationEvent hits))
        else equal recorder (named "application events") [0x90 :: Word32] (map hitR0 (sitesOf ApplicationEvent hits))
      record []
      pure (Just ())

detachBoundary :: Cells -> [Hit] -> Value
detachBoundary cells hits = object
  [ "kind" .= ("no detach edge reached connect handler A" :: T.Text)
  , "driver_state" .= hex32 (cellDriverState cells), "driver_status" .= hex32 (cellDriverStatus cells)
  , "connect_observer_r0" .= map (hex32 . hitR0) (sitesOf ConnectObserver hits)
  , "reading" .= ("The transport driver samples the presence bit 0xfff55104[31] (0x00d7b98c) only in driver states 0..4 (0x00d81d04). In the configured state it waits for a controller event; the endpoint model raises none when the cable level drops." :: T.Text) ]

enumerate :: Ctx -> Int -> IO ()
enumerate ctx index = do
  let srv = ctxServer ctx
      recorder = ctxRecorder ctx
      name = T.pack ("step-" ++ show index ++ "-enumerate")
      named suffix = name <> ": " <> suffix
  model <- readIORef (ctxModel ctx)
  _ <- collect ctx
  reply <- call srv "enumerate" ["budget" .= (200000000 :: Int)]
  device <- textField "device" reply >>= hexDecode
  configuration <- textField "configuration" reply
  cells <- readCells srv
  let vid = fromInteger (le (B.take 2 (B.drop 8 device))) :: Word16
      pid = fromInteger (le (B.take 2 (B.drop 10 device))) :: Word16
      ids = T.pack (drop 2 (hex16 vid) ++ ":" ++ drop 2 (hex16 pid))
      personality = fromMaybe Application (modelPersonality model)
  shot <- capture srv recorder "enumerated"
  service <- case personality of
    Service -> do
      equal recorder (named "device descriptor VID:PID") ("04cb:ff80" :: T.Text) ids
      equal recorder (named "0x0146a0c0") (3 :: Word32) (cellTransportMode cells)
      equal recorder (named "0x01469810") (3 :: Word8) (cellDescriptorMode cells)
      equal recorder (named "0x0146a0cc") serviceVendorTable (cellVendorTable cells)
      Just <$> serviceSession ctx named
    Application -> do
      let template = T.pack ("04cb:" ++ drop 2 (hex16 (cellCache13e cells)))
      equal recorder (named "device descriptor VID:PID (template PID cache[0x13e..0x13f])") template ids
      equal recorder (named "0x0146a0c0") (2 :: Word32) (cellTransportMode cells)
      equal recorder (named "0x01469810") (2 :: Word8) (cellDescriptorMode cells)
      pure Nothing
  hits <- collect ctx
  step recorder (object
    [ "step" .= name, "kind" .= ("enumerate" :: T.Text), "device_descriptor" .= hexBytes device
    , "vid_pid" .= ids, "configuration_descriptor" .= configuration, "cells" .= cellsValue cells
    , "capture" .= shot, "service_session" .= service, "hits" .= map hitValue hits ])

-- | C0, identity, config window 0 and one function-2 read over the endpoint transport.
serviceSession :: Ctx -> (T.Text -> T.Text) -> IO Value
serviceSession ctx named = do
  let srv = ctxServer ctx
      recorder = ctxRecorder ctx
      api = bridgeUsbApi (exchangeLine srv)
      tx packet = snd <$> usbTransfer api packet 5000
  outcome <- try $ do
    _ <- collect ctx
    _ <- tx (P.controlTransfer (P.control 0xc0 8 0))
    opened <- tx (P.BulkIn 0x81 8)
    require (drop 2 opened == [0, 0, 0, 0, 1, 0]) ("C0 reply tail mismatch: " ++ show opened)
    sid <- case opened of
      low : high : _ -> pure (fromIntegral low + 256 * fromIntegral high)
      _ -> ioError (userError "C0 reply is shorter than two bytes")
    _ <- tx (P.controlTransfer (P.control 0xd0 32 sid))
    _ <- tx (P.BulkOut 2 P.identityPacket)
    _ <- tx (P.controlTransfer (P.control 0xd1 128 sid))
    identity <- tx (P.BulkIn 0x81 128)
    model <- either (ioError . userError) pure (P.decodeProduct identity)
    let window = ReadWindow 0 0 "config-000000"
    _ <- tx (P.controlTransfer (P.control 0xd0 readRequestBytes sid))
    _ <- tx (P.BulkOut 2 (readRequestFrame window))
    _ <- tx (P.controlTransfer (P.control 0xd1 readReplyBytes sid))
    configReply <- tx (P.BulkIn 0x81 readReplyBytes)
    config <- either (ioError . userError) pure (decodeReadReply window configReply)
    let frame = concatMap le32 [0x200001, 0, 0, 0, 2, 0, 0, ramReadAddress, fromIntegral ramReadCount]
    _ <- tx (P.controlTransfer (P.control 0xd0 (length frame) sid))
    _ <- tx (P.BulkOut 2 frame)
    _ <- tx (P.controlTransfer (P.control 0xd1 (36 + ramReadCount) sid))
    ramReply <- tx (P.BulkIn 0x81 (36 + ramReadCount))
    guest <- readMemory srv ramReadAddress ramReadCount
    hits <- collect ctx
    pure (sid, model, config, ramReply, guest, hits)
  case outcome of
    Left (problem :: IOException) -> do
      boundary <- readIORef (srvLast srv)
      check recorder (named "service session") False (String "C0, identity, config window 0, function-2 read") (String (T.pack (displayException problem)))
      pure (object ["error" .= displayException problem, "last_bridge_line" .= C.unpack boundary])
    Right (sid, model, config, ramReply, guest, hits) -> do
      equal recorder (named "identity") ("X-M1" :: T.Text) (T.pack model)
      equal recorder (named "config window 0 byte 0xf7") (1 :: Word8) (config !! 0xf7)
      equal recorder (named "function-2 reply equals guest memory") (T.pack (hexBytes guest)) (T.pack (hexBytes (B.pack (drop 36 ramReply))))
      equal recorder (named "function-2 dispatches") (1 :: Int) (length (sitesOf DebugDispatch hits))
      pure (object
        [ "sid" .= sid, "identity" .= model, "config_f7" .= (config !! 0xf7)
        , "config_window_0" .= hexBytes (B.pack config)
        , "function_2" .= object ["address" .= hex32 ramReadAddress, "bytes" .= ramReadCount
                                 , "reply_header" .= hexBytes (B.pack (take 36 ramReply)), "data" .= hexBytes (B.pack (drop 36 ramReply)) ] ])

le32 :: Word32 -> [Word8]
le32 value = [fromIntegral (value `div` (256 ^ k)) | k <- [0 .. 3 :: Int]]

-- * Stack

stackReport :: Server -> [Hit] -> IO Value
stackReport srv hits = do
  base <- word32At srv usbcontStackCell
  size <- word32At srv (usbcontStackCell + 4)
  bytes <- readMemory srv base (fromIntegral (min size 0x1000))
  let untouched = B.length (B.takeWhile (== 0x07) bytes)
      top = base + size
      inStack sp = sp >= base && sp < top
      hookSps = map hitSp (sitesOf F7Read hits)
      stubKeySps = [hitSp h | h <- sitesOf KeyStateQ hits, hitLr h >= extentStart, hitLr h < extentStart + stubBytes]
  pure (object
    [ "usbcont_stack" .= object ["base" .= hex32 base, "size" .= hex32 size, "top" .= hex32 top]
    , "sp_at_0x00ecf900" .= map hex32 hookSps
    , "sp_at_0x00ecf900_in_usbcont_stack" .= all inStack hookSps
    , "bytes_below_top_at_0x00ecf900" .= map (top -) hookSps
    , "sp_at_0x006a0d00_from_stub" .= map hex32 stubKeySps
    , "bytes_below_top_at_0x006a0d00_from_stub" .= map (top -) stubKeySps
    , "high_water_bytes_used" .= (fromIntegral size - untouched)
    , "untouched_0x07_bytes" .= untouched ])

-- * Scenarios

data Scenario = Scenario
  { scName :: String
  , scFamily :: String
  , scDat :: DatRole
  , scNand :: NandRole
  , scBoot :: BootKind
  , scSteps :: [Step]
  }

scenarios :: [Scenario]
scenarios =
  [ Scenario ("e2-" ++ datName d) "e2" d Stored0 ProbeBoot [Attach Plain] | d <- [StockDat, CcBaseDat, CcToggleDat, Rc0ToggleDat] ] ++
  [ Scenario ("e3-" ++ datName d) "e3" d Stored0 RouteBoot [Attach Plain, Enumerate] | d <- toggles ] ++
  [ Scenario ("e4e5-" ++ datName d) "e4e5" d Stored0 RouteBoot
      [Attach HoldQ, Enumerate, Detach, Attach Plain, Enumerate, Detach, Attach HoldQ, Enumerate, Detach] | d <- toggles ] ++
  [ Scenario ("e5u-cycle-" ++ datName d) "e5u" d Stored0 RouteBoot
      [Attach HoldQ, Detach, Attach Plain, Detach, Attach HoldQ, Detach, Attach Plain, Enumerate] | d <- toggles ] ++
  [ Scenario ("e5u-ff80-" ++ datName d) "e5u" d Stored0 RouteBoot [Attach HoldQ, Detach, Attach Plain, Enumerate] | d <- toggles ] ++
  [ Scenario ("e5u-q-ptp-" ++ datName d) "e5u" d Stored0 RouteBoot
      [Attach HoldQ, Detach, Attach Plain, Detach, Attach HoldQ, Enumerate] | d <- toggles ] ++
  [ Scenario ("e6-" ++ datName d ++ "-" ++ nandName Stored0Pid02b6 ++ q) "e6" d Stored0Pid02b6 (CableAtReset held) [Enumerate]
  | d <- toggles, (q, held) <- [("-plain", NoQ), ("-q", QFromReset)] ] ++
  [ Scenario ("e7-" ++ datName d ++ "-plain") "e7" d Stored1 RouteBoot [Attach Plain, Enumerate] | d <- toggles ] ++
  [ Scenario ("e7-" ++ datName d ++ "-q") "e7" d Stored1 RouteBoot [Attach HoldQ, Enumerate] | d <- toggles ] ++
  [ Scenario "control-e3-stock" "control" StockDat Stored0 RouteBoot [Attach Plain, Enumerate]
  , Scenario "control-e4-stock" "control" StockDat Stored0 RouteBoot [Attach HoldQ, Enumerate] ]
  where toggles = [CcToggleDat, Rc0ToggleDat]

-- | Q's loaded-calibration midpoint for ADC channel 9, read at the shooting
-- screen of the stored-0 route checkpoint (cache tags 0x379a low, 0x3798 high).
qMidpoint :: Env -> DatRole -> FilePath -> IO Int
qMidpoint env dat output = do
  (srv, _) <- startServer env dat Stored0 RouteBoot Nothing output
  cache <- word32At srv cachePointerCell
  high <- fromInteger . le <$> readMemory srv (cache + 0x3798) 2
  low <- fromInteger . le <$> readMemory srv (cache + 0x379a) 2
  stopServer srv
  pure ((low + high) `div` 2)

runScenario :: Env -> FilePath -> Scenario -> IO Value
runScenario env receipts scenario = do
  let directory = receipts </> scName scenario
  createDirectory directory
  setFileMode directory 0o700
  putStrLn ("scenario " ++ scName scenario)
  hFlush stdout
  qSample <- case scBoot scenario of
    CableAtReset QFromReset -> Just <$> qMidpoint env (scDat scenario) (directory </> "calibration")
    _ -> pure Nothing
  recorder <- newRecorder
  hitLog <- newIORef []
  result <- try $ do
    (srv, started) <- startServer env (scDat scenario) (scNand scenario) (scBoot scenario) qSample (directory </> "emulator")
    outcome <- try $ do
      bootHits <- newHits srv
      cells <- readCells srv
      state <- call srv "state" []
      let route = scBoot scenario == RouteBoot || scBoot scenario == ProbeBoot
      equal recorder "boot: cache[0xf7] equals the stored byte" (storedF7 (scNand scenario)) (cellF7 cells)
      if route
        then check recorder "boot: system mode neither 5 nor 7" (not (excludedMode (cellSystemMode cells))) (String "not 5 or 7") (toValue (cellSystemMode cells))
        else equal recorder "boot: USB-cable boot system mode" (5 :: Word32) (cellSystemMode cells)
      unless route $ do
        equal recorder "boot: stub entries" (0 :: Int) (length (sitesOf StubEntry bootHits))
        equal recorder "boot: values handler A tested at 0x00ecf904" ([] :: [Word32]) [hitR0 h | h <- sitesOf F7Used bootHits, hitR0 h /= 0]
      step recorder (object ["step" .= ("boot" :: T.Text), "kind" .= show (scBoot scenario), "q_adc_sample" .= qSample
                            , "record" .= startBoot started, "cells" .= cellsValue cells, "state" .= Object state
                            , "hits" .= map hitValue bootHits ])
      modelRef <- newIORef (Model (datHasToggle (scDat scenario)) (cellF7 cells) (cellSystemMode cells)
        (if route then Nothing else Just Application))
      let ctx = Ctx srv recorder modelRef (scDat scenario) hitLog
      modifyIORef' hitLog (++ bootHits)
      blocked <- runSteps ctx (zip [1 ..] (scSteps scenario))
      when (scBoot scenario == CableAtReset QFromReset) $ do
        _ <- call srv "adc" ["channel" .= (9 :: Int), "sample" .= (1023 :: Int)]
        _ <- runInstructions srv 8000000 []
        released <- readCells srv
        equal recorder "Q row 19 +0x15 after the reset hold is released" (0 :: Word8) (cellQDebounced released)
        shot <- capture srv recorder "q-released"
        endCells <- readCells srv
        equal recorder "cache[0xf7] at the end" (0 :: Word8) (cellF7 endCells)
        step recorder (object ["step" .= ("release-reset-q" :: T.Text), "cells" .= cellsValue released, "capture" .= shot])
      probe <- if scBoot scenario == ProbeBoot then Just <$> probeReport srv recorder scenario hitLog else pure Nothing
      hits <- readIORef hitLog
      stack <- stackReport srv hits
      pure (blocked, probe, stack)
    stopServer srv
    pure (case outcome of
      Left problem -> case fromException problem of
        Just (Stop message value) -> (started, Blocked message value, Nothing, Null)
        Nothing -> (started, Blocked (displayException problem) Null, Nothing, Null)
      Right (blocked, probe, stack) -> (started, fromMaybe Completed blocked, probe, stack))
  steps <- readIORef (recSteps recorder)
  checks <- readIORef (recChecks recorder)
  let passed = all (\(Check _ ok _ _) -> ok) checks
      (status, identity, boundary, probe, stack) = case result of
        Left (problem :: SomeException) -> ("blocked" :: T.Text, Null, case fromException problem of
          Just (Stop message value) -> object ["message" .= message, "boundary" .= value]
          Nothing -> String (T.pack (displayException problem)), Nothing, Null)
        Right (started, Blocked message value, p, s) ->
          ("blocked", startedValue started, object ["message" .= message, "boundary" .= value], p, s)
        Right (started, Completed, p, s) -> (if passed then "pass" else "fail", startedValue started, Null, p, s)
      receipt = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 USB personality toggle (reports/usb-personality-toggle.md 8.2)" :: T.Text)
        , "scenario" .= scName scenario, "family" .= scFamily scenario
        , "dat" .= object ["role" .= datName (scDat scenario), "path" .= datPath (scDat scenario), "sha256" .= sha (datSha256 (scDat scenario))]
        , "nand" .= object ["role" .= nandName (scNand scenario), "path" .= nandPath (scNand scenario)
                           , "sha256" .= sha (nandSha256 (scNand scenario)), "stored_f7" .= storedF7 (scNand scenario)]
        , "boot" .= show (scBoot scenario), "steps_planned" .= map show (scSteps scenario)
        , "emulator" .= identity
        , "result" .= status, "boundary" .= boundary
        , "checks" .= [object ["name" .= n, "pass" .= ok, "expected" .= e, "observed" .= o] | Check n ok e o <- checks]
        , "steps" .= steps, "access_probe" .= probe, "stack" .= stack ]
  BL.writeFile (directory </> "receipt.json") (encode receipt <> "\n")
  putStrLn ("  " ++ T.unpack status ++ " (" ++ show (length (filter (\(Check _ ok _ _) -> not ok) checks)) ++ " failed checks)")
  pure (object ["scenario" .= scName scenario, "result" .= status, "receipt" .= (directory </> "receipt.json")
               , "failed_checks" .= [n | Check n ok _ _ <- checks, not ok]])
  where
    sha (Sha256 s) = s
    startedValue started = object ["checkpoint_key" .= startKey started, "inputs" .= startInputs started, "start" .= startKind started]

-- | Runs the steps; a detach that never reaches handler A stops the run.
runSteps :: Ctx -> [(Int, Step)] -> IO (Maybe Outcome)
runSteps _ [] = pure Nothing
runSteps ctx ((index, current) : rest) = case current of
  Attach hold -> attach ctx index hold >> runSteps ctx rest
  Enumerate -> enumerate ctx index >> runSteps ctx rest
  Detach -> do
    seen <- detach ctx index
    case seen of
      Just () -> runSteps ctx rest
      Nothing -> do
        hits <- readIORef (ctxHits ctx)
        cells <- readCells (ctxServer ctx)
        pure (Just (Blocked ("step " ++ show index ++ ": the detach edge did not reach connect handler A; steps "
          ++ show (map fst rest) ++ " not run") (detachBoundary cells (lastObserver hits))))
  where lastObserver hits = sitesOf ConnectObserver hits

data AccessRecord = AccessRecord
  { accSource :: String
  , accPc :: Word32
  , accAddress :: Word32
  , accPhysical :: Word32
  , accSize :: Integer
  , accAt :: Integer
  }

accessValue :: AccessRecord -> Value
accessValue r = object
  [ "source" .= accSource r, "pc" .= hex32 (accPc r), "address" .= hex32 (accAddress r)
  , "physical" .= hex32 (accPhysical r), "size" .= accSize r, "instructions" .= accAt r ]

-- | E2: CPU accesses confined to the extent, against image-wide transfers
-- that cover it.
probeReport :: Server -> Recorder -> Scenario -> IORef [Hit] -> IO Value
probeReport srv recorder scenario hitLog = do
  reply <- call srv "access-probe" []
  items <- case field "records" reply of Array values -> pure (V.toList values); _ -> ioError (userError "access records")
  records <- forM items $ \item -> case item of
    Object o -> AccessRecord <$> textField "source" o
      <*> (fromInteger <$> intField "pc" o) <*> (fromInteger <$> intField "address" o)
      <*> (fromInteger <$> intField "physical" o) <*> intField "size" o <*> intField "instructions" o
    _ -> ioError (userError "access record")
  hits <- readIORef hitLog
  let low = extentStart
      high = extentStart + fromIntegral extentLength
      confined r = accPhysical r >= low && toInteger (accPhysical r) + accSize r <= toInteger high
      inside = filter confined records
      wide = filter (not . confined) records
      fetches = filter ((== "cpu-fetch") . accSource) inside
      reads' = sitesOf F7Read hits
      entries = filter ((== low) . accPc) fetches
      precededByRead r = any (\h -> hitAt h + 1 == accAt r) reads'
  if datHasToggle (scDat scenario)
    then do
      equal recorder "probe: only CPU fetches inside the extent" (length inside) (length fetches)
      check recorder "probe: fetches only within the 22-word stub" (all (\r -> accPc r >= low && accPc r < low + stubBytes) fetches)
        (String "PCs in [0x00bf4454, 0x00bf44ac)") (toValue (map accPc fetches))
      check recorder "probe: first fetch is the stub entry" (fmap accPc (listToMaybe fetches) == Just low)
        (toValue low) (toValue (map accPc (take 1 fetches)))
      check recorder "probe: each stub entry is the instruction after a 0x00ecf900 bl" (not (null entries) && all precededByRead entries)
        (String "entry at the 0x00ecf900 hit's instruction count + 1")
        (object ["f7_reads" .= map hitAt reads', "entries" .= map accAt entries])
      equal recorder "probe: stub entries equal 0x00ecf900 reads" (length reads') (length entries)
    else equal recorder "probe: CPU fetches, loads and stores inside the extent" (0 :: Int) (length inside)
  pure (object
    [ "range" .= [hex32 low, hex32 high], "enabled" .= field "enabled" reply, "violation" .= field "violation" reply
    , "records_inside_extent" .= map accessValue inside
    , "image_wide_transfers_covering_extent" .= map accessValue wide
    , "stub_entries" .= length entries, "f7_reads" .= length reads' ])

-- * Command line

data Options = Options
  { optReceipts :: Maybe FilePath
  , optFamilies :: [String]
  , optFresh :: Bool
  , optWorkspace :: Maybe Workspace
  }

usage :: String
usage = unlines
  [ "Usage: bazel run //emulation:usb-toggle-emu -- --receipt-output DIR [--runs FAMILY,...] [--fresh-boot]"
  , ""
  , "Runs the emulated USB-toggle scenarios of reports/usb-personality-toggle.md 8.2 in one emulator slot."
  , "Host-only: no camera or USB device. Receipts go to DIR (fresh, beneath private-data/)."
  , ""
  , "  --runs FAMILY,...   e2, e3, e4e5, e5u, e6, e7, control or all (default: all)"
  , "  --fresh-boot        cold-boot each route checkpoint once and replace it"
  , "  --workspace PATH    repository root (default: BUILD_WORKSPACE_DIRECTORY)"
  ]

families :: [String]
families = ["e2", "e3", "e4e5", "e5u", "e6", "e7", "control"]

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing families False Nothing)
  where
    go options [] = pure options
    go options ("--receipt-output" : value : rest) = go options {optReceipts = Just value} rest
    go options ("--runs" : value : rest) = do
      let names = splitComma value
          chosen = if names == ["all"] then families else names
      require (not (null chosen) && all (`elem` families) chosen) ("--runs takes " ++ show families ++ " or all")
      go options {optFamilies = chosen} rest
    go options ("--fresh-boot" : rest) = go options {optFresh = True} rest
    go options ("--workspace" : value : rest) = go options {optWorkspace = Just (Workspace value)} rest
    go _ (argument : _) = ioError (userError ("unknown option or missing value: " ++ argument ++ "\n" ++ usage))
    splitComma text = case break (== ',') text of
      (front, []) -> [front]
      (front, _ : rest) -> front : splitComma rest

freshReceipts :: FilePath -> FilePath -> IO FilePath
freshReceipts root supplied = do
  private <- canonicalizePath (root </> "private-data")
  let path = if "/" `isPrefixOf` supplied then supplied else root </> supplied
  parent <- canonicalizePath (takeDirectory path)
  require (takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must name a fresh directory"
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--receipt-output must be beneath workspace/private-data/"
  let destination = parent </> takeFileName path
  exists <- doesPathExist destination
  require (not exists) "--receipt-output must not exist"
  createDirectory destination
  setFileMode destination 0o700
  pure destination

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--validate-firmware", path] -> validateFirmware (Firmware path)
    _ | "--help" `elem` arguments -> putStr usage
      | otherwise -> (parseOptions arguments >>= launch arguments) `catch` \(problem :: IOException) ->
          hPutStrLn stderr (displayException problem) >> exitFailure

launch :: [String] -> Options -> IO ()
launch arguments options = do
  hSetBuffering stdout LineBuffering
  receiptsArgument <- maybe (ioError (userError ("--receipt-output is required\n" ++ usage))) pure (optReceipts options)
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  Workspace root <- workspace (optWorkspace options)
  forM_ [StockDat, CcBaseDat, CcToggleDat, Rc0ToggleDat] $ \dat -> validateFirmware (Firmware (root </> datPath dat))
  receipts <- freshReceipts root receiptsArgument
  staged <- stageViewerRuntime (Workspace root) Nothing (ReplacedRuntime ("emulator-runtimes" </> "xm1-usb-toggle")) Nothing StageStockOriginal
  let Python python = stagedPython staged
      runtime = stagedDirectory staged
  callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
  replaced <- newIORef []
  head' <- (filter (/= '\n') <$> readProcess "git" ["-C", root, "rev-parse", "HEAD"] "") `catch` \(_ :: IOException) -> pure "unknown"
  dirty <- (not . null <$> readProcess "git" ["-C", root, "status", "--porcelain", "--", "emulation", "host", "transport", "broker"] "") `catch` \(_ :: IOException) -> pure True
  let env = Env root python runtime (root </> "private-data" </> "emulator-checkpoints" </> "xm1-usb-toggle") (optFresh options) replaced
      chosen = [s | s <- scenarios, scFamily s `elem` optFamilies options]
  results <- mapM (runScenario env receipts) chosen
  BL.writeFile (receipts </> "summary.json") (encode (object
    [ "schema" .= (1 :: Int), "git_head" .= head', "git_dirty_harness" .= dirty
    , "runtime" .= runtime, "families" .= optFamilies options, "scenarios" .= results ]) <> "\n")
  let statuses = [s | Object o <- results, String s <- [field "result" o]]
  putStrLn ("summary: " ++ receipts </> "summary.json")
  unless (all (== "pass") statuses) (exitWith (ExitFailure 1))
