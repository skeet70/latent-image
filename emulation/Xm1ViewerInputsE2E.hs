{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException, finally)
import Control.Monad (forM, forM_, void, when)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, toJSON, withObject, (.:), (.=))
import qualified Data.Aeson.Key as K
import Data.Aeson.Types (parseMaybe)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, toLower)
import Data.List (isPrefixOf)
import Data.Word (Word32)
import Numeric (showHex)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, resolve, stageViewerRuntime, stagedFirmwarePath)
import qualified Emulation.ViewerRuntime as Runtime
import EmulatorUsb
import FujiAF.Transport (ensurePrivateDirectory, writePrivateBytes)
import System.Directory (canonicalizePath, doesPathExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (hPutStrLn, stderr)
import System.Process (callProcess)

newtype InputPath = InputPath FilePath
newtype ReceiptPath = ReceiptPath FilePath
newtype ConsumerState = ConsumerState Word32 deriving (Eq, Show)

data ShootingMode = AutoState | SceneRecognitionState | PortraitState | LandscapeState
  | SportsState | ScenePositionState | AdvancedState | ProgramState | ShutterState
  | ApertureState | ManualState deriving (Eq, Show)

shootingMode :: Word32 -> Maybe ShootingMode
shootingMode value = case value of
  0 -> Just AutoState
  1 -> Just SceneRecognitionState
  5 -> Just PortraitState
  8 -> Just LandscapeState
  0xb -> Just SportsState
  0x18 -> Just ScenePositionState
  0x20 -> Just AdvancedState
  0x21 -> Just ProgramState
  0x22 -> Just ShutterState
  0x23 -> Just ApertureState
  0x24 -> Just ManualState
  _ -> Nothing

expectedShootingMode :: ModeDial -> ShootingMode
expectedShootingMode position = case position of
  ModeDialShutter -> ShutterState
  ModeDialProgram -> ProgramState
  ModeDialAperture -> ApertureState
  ModeDialManual -> ManualState
  ModeDialAuto -> AutoState
  ModeDialCustom -> ProgramState
  ModeDialSports -> SportsState
  ModeDialLandscape -> LandscapeState
  ModeDialPortrait -> PortraitState
  ModeDialSRAuto -> SceneRecognitionState
  ModeDialScene -> ScenePositionState
  ModeDialAdvanced -> AdvancedState

data TerminalState = StockPowerOff deriving (Eq)

terminalState :: Value -> Maybe TerminalState
terminalState = parseMaybe $ withObject "diagnostic state" $ \state ->
  state .: "failure" >>= withObject "terminal state" (\failure -> do
    cpu <- failure .: "cpu"
    pc <- failure .: "pc"
    if (cpu, reads pc) == (0 :: Int, [(0x00ec5818 :: Word32, "")])
      then pure StockPowerOff
      else fail "not the stock power-off halt")

data InputControl = MainUp | MainDown | HalfPress | FullPress | Movie deriving (Eq, Show)
data Case = DefaultCases | ControlsCase | ModesCase | FullPressCase | CheckpointRestoreCase | PowerDestructorCase deriving (Eq)
data Options = Options (Maybe InputPath) (Maybe InputPath) (Maybe ReceiptPath) (Maybe Firmware) Case Bool

data RunCase = ModeRun ModeDial | ControlsRun | FullPressRun | PowerDestructorRun
data CaseReceipt = CaseReceipt
  { baselineRecord :: Value
  , modeRecords :: [Value]
  , controlRecords :: [Value]
  , s2Records :: [Value]
  , destructorRecords :: [Value]
  }

caseName :: RunCase -> FilePath
caseName (ModeRun position) = "mode-" ++ show (fromEnum position)
caseName ControlsRun = "controls"
caseName FullPressRun = "s2"
caseName PowerDestructorRun = "power-destructor"

newtype ModeInputRow = ModeInputRow Word32

modeInputRow :: ModeDial -> ModeInputRow
modeInputRow position = ModeInputRow $ case position of
  ModeDialShutter -> 24
  ModeDialProgram -> 25
  ModeDialSRAuto -> 26
  ModeDialAuto -> 27
  ModeDialAdvanced -> 28
  ModeDialScene -> 29
  ModeDialSports -> 30
  ModeDialLandscape -> 31
  ModeDialPortrait -> 32
  ModeDialCustom -> 33
  ModeDialManual -> 34
  ModeDialAperture -> 35

controlName :: InputControl -> String
controlName control = case control of
  MainUp -> "main-dial-up"
  MainDown -> "main-dial-down"
  HalfPress -> "half-press"
  FullPress -> "full-press"
  Movie -> "movie"

consumerState :: InputControl -> ConsumerState
consumerState control = ConsumerState $ case control of
  MainUp -> 0x54
  MainDown -> 0x55
  HalfPress -> 1
  FullPress -> 3
  Movie -> 0x0d

usage :: String
usage = "xm1-viewer-inputs-e2e --installed-nand NAND --receipt-output DIR [--sd-card-image CARD --firmware DAT --case controls|modes|s2|checkpoint-restore|power-destructor --fresh-boot]\nHost-only stock input consumers and isolated mode state. S2 runs only with --case s2, in a fresh run with no card or a private card copy. checkpoint-restore requires an existing cardless attach checkpoint, restores it twice and checks that mode changes do not survive restoration. power-destructor records kernel object tables before and after a stock POWER OFF and the entries of the image4 queue destructor 0x724f08 and its kernel callees during it.\n"

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing DefaultCases False)
  where
    go options [] = pure options
    go (Options _ card receipt firmware selected fresh) ("--installed-nand" : value : rest) = go (Options (Just (InputPath value)) card receipt firmware selected fresh) rest
    go (Options nand _ receipt firmware selected fresh) ("--sd-card-image" : value : rest) = go (Options nand (Just (InputPath value)) receipt firmware selected fresh) rest
    go (Options nand card _ firmware selected fresh) ("--receipt-output" : value : rest) = go (Options nand card (Just (ReceiptPath value)) firmware selected fresh) rest
    go (Options nand card receipt _ selected fresh) ("--firmware" : value : rest) = go (Options nand card receipt (Just (Firmware value)) selected fresh) rest
    go (Options nand card receipt firmware selected _) ("--fresh-boot" : rest) = go (Options nand card receipt firmware selected True) rest
    go (Options nand card receipt firmware _ fresh) ("--case" : value : rest) = do
      selected <- case value of
        "controls" -> pure ControlsCase
        "modes" -> pure ModesCase
        "s2" -> pure FullPressCase
        "checkpoint-restore" -> pure CheckpointRestoreCase
        "power-destructor" -> pure PowerDestructorCase
        _ -> ioError (userError usage)
      go (Options nand card receipt firmware selected fresh) rest
    go _ _ = ioError (userError usage)

privateInput :: Workspace -> InputPath -> IO FilePath
privateInput root (InputPath supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) "Inputs must be beneath private-data/"
  pure path

freshReceipt :: Workspace -> Maybe ReceiptPath -> IO FilePath
freshReceipt root supplied = do
  ReceiptPath value <- maybe (ioError (userError "--receipt-output is required")) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  let path = resolve root value
  parent <- canonicalizePath (takeDirectory path)
  exists <- doesPathExist path
  require (splitDirectories private `isPrefixOf` splitDirectories parent && not exists && takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must be a new directory beneath private-data/"
  ensurePrivateDirectory path
  pure path

writeJson :: FilePath -> Value -> IO ()
writeJson path = writePrivateBytes path . (<> "\n") . BL.toStrict . encode

readWord :: Emulator -> Word32 -> Int -> IO Word32
readWord emulator address width = B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0 <$> emulatorReadMemory emulator address width

menuState :: Value -> IO (Int, Int, Int)
menuState value = maybe (ioError (userError "missing firmware menu state")) pure $ parseMaybe
  (withObject "key reply" $ \reply -> reply .: "menu" >>= withObject "menu" (\menu -> (,,) <$> menu .: "menu_id" <*> menu .: "setting_id" <*> menu .: "cursor")) value

pulse :: Emulator -> String -> IO Value
pulse emulator = emulatorKey emulator (KeyTiming (Just (KeyPulse 8000000 12000000)) Nothing)

openShootingMenu :: Emulator -> IO Value
openShootingMenu emulator = do
  opened <- pulse emulator "ok"
  (menu, setting, _) <- menuState opened
  if menu == 16 && setting == 472
    then pulse emulator "ok"
    else pure opened

checkControl :: Emulator -> FilePath -> InputControl -> IO Value
checkControl emulator receipt control = do
  before <- ConsumerState <$> readWord emulator 0x01467778 2
  response <- pulse emulator (controlName control)
  after <- ConsumerState <$> readWord emulator 0x01467778 2
  capture <- emulatorCapture emulator (controlName control)
  let ConsumerState actual = after
      result = object ["control" .= controlName control, "reply" .= response, "consumer_state" .= actual, "capture" .= capture]
  writeJson (receipt </> controlName control ++ ".json") result
  require (after == consumerState control && before /= after) (controlName control ++ " did not change the stock UI input-consumer state")
  pure result

checkControls :: Emulator -> FilePath -> IO [Value]
checkControls emulator receipt = do
  void (emulatorModeDial emulator ModeDialShutter)
  opened <- openShootingMenu emulator
  (menu, setting, _) <- menuState opened
  require (menu == 1 && setting == 5) "MENU did not open the stock shooting menu"
  let checkDial before control = do
        response <- pulse emulator (controlName control)
        after <- menuState response
        capture <- emulatorCapture emulator (controlName control)
        let result = object ["control" .= controlName control, "before" .= before,
                             "after" .= after, "reply" .= response, "capture" .= capture]
        writeJson (receipt </> controlName control ++ ".json") result
        require (after /= before) (controlName control ++ " did not change the stock menu selection")
        pure (after, result)
  initial <- menuState opened
  (upState, upResult) <- checkDial initial MainUp
  (downState, downResult) <- checkDial upState MainDown
  require (downState == initial) "Opposite main-dial detents did not restore the stock menu selection"
  void (pulse emulator "back")
  otherControls <- forM [HalfPress, Movie] (checkControl emulator receipt)
  let controls = [upResult, downResult] ++ otherControls
  response <- pulse emulator "power"
  diagnostics <- either (ioError . userError) pure . eitherDecodeStrict'
    =<< B.readFile (receipt </> "bridge/state.json")
  let shutdown = terminalState diagnostics
      result = object ["control" .= String "power", "reply" .= response,
                       "terminal_state" .= (parseMaybe (withObject "state" (.: "failure")) diagnostics :: Maybe Value)]
  writeJson (receipt </> "power.json") result
  require (shutdown == Just StockPowerOff) "POWER OFF did not reach the stock terminal power-off loop"
  pure (controls ++ [result])

-- | A guest RAM range read whole before and after POWER OFF.
data MemoryWindow = MemoryWindow { windowName :: String, windowAddress :: Word32, windowBytes :: Int }

destructorWindows :: [MemoryWindow]
destructorWindows =
  [ MemoryWindow "a-mailbox-and-eventflag-creation" 0x4005d16c 0x5ac
  , MemoryWindow "b-mailbox-and-eventflag-creation" 0x400ed12c 0x5ac
  , MemoryWindow "a-eventflag-control-to-boot-stack-top" 0x400624b4 0xfac
  , MemoryWindow "b-eventflag-control-to-boot-stack-top" 0x400f2474 0xfac
  , MemoryWindow "a-data-queue-creation" 0x4005988c 0x780
  , MemoryWindow "b-data-queue-creation" 0x400e984c 0x780
  , MemoryWindow "task41-module-state" 0x008faef0 0x50
  , MemoryWindow "exit-barrier-mask" 0x015c1e5c 4
  ]

destructorSites :: [(Word32, String)]
destructorSites =
  [ (0x00ec6708, "exit-after-barrier"), (0x006e4ab4, "task41-stop-request"), (0x006e9dec, "task41-stop-sequence")
  , (0x006e9f68, "task41-stop-final"), (0x00724f08, "destructor-entry"), (0x00724fa0, "destructor-pool-delete-call")
  , (0x00724ffc, "destructor-d0-call"), (0x00725000, "destructor-d0-return"), (0x0072612c, "task-delete-entry")
  , (0x00724558, "flag-set-wrapper"), (0x00ec31f0, "exit-terminal-action"), (0x00041a34, "kernel-failure-log")
  , (0x0004d39c, "a-80"), (0x0004d434, "a-84"), (0x0004813c, "a-88"), (0x0004839c, "a-8c"), (0x000485c0, "a-90")
  , (0x000486f8, "a-94"), (0x000489ac, "a-98"), (0x00048b30, "a-9c")
  , (0x0004d540, "a-a4"), (0x0004d654, "a-d0"), (0x0004d790, "a-e8")
  , (0x000dd360, "b-80"), (0x000dd3f8, "b-84"), (0x000d80fc, "b-88"), (0x000d835c, "b-8c"), (0x000d8580, "b-90")
  , (0x000d86b8, "b-94"), (0x000d896c, "b-98"), (0x000d8af0, "b-9c")
  , (0x000dd504, "b-a4"), (0x000dd618, "b-d0"), (0x000dd754, "b-e8")
  ]

readWindows :: Emulator -> IO Value
readWindows emulator = object <$> forM destructorWindows (\window -> do
  bytes <- emulatorReadMemory emulator (windowAddress window) (windowBytes window)
  require (B.length bytes == windowBytes window) (windowName window ++ " read returned a short window")
  pure (K.fromString (windowName window) .= object
    ["address" .= ("0x" ++ showHex (windowAddress window) ""), "bytes" .= concatMap byteHex (B.unpack bytes)]))
  where byteHex byte = (if byte < 16 then ('0' :) else id) (showHex byte "")

hitValue :: TraceHit -> Value
hitValue hit = object
  [ "site" .= traceSite hit, "core" .= traceCore hit, "instructions" .= traceInstructions hit
  , "r0" .= word (traceR0 hit), "r1" .= word (traceR1 hit), "r2" .= word (traceR2 hit), "r3" .= word (traceR3 hit) ]
  where word value = "0x" ++ showHex value ""

checkPowerDestructor :: Emulator -> FilePath -> IO Value
checkPowerDestructor emulator receipt = do
  before <- readWindows emulator
  writeJson (receipt </> "destructor-before.json") before
  emulatorTraceSites emulator destructorSites
  (start, _) <- emulatorTraceHits emulator 0
  response <- pulse emulator "power"
  diagnostics <- either (ioError . userError) pure . eitherDecodeStrict'
    =<< B.readFile (receipt </> "bridge/state.json")
  (_, hits) <- emulatorTraceHits emulator start
  writeJson (receipt </> "destructor-hits.json") (toJSON (map hitValue hits))
  after <- readWindows emulator
  writeJson (receipt </> "destructor-after.json") after
  let result = object ["control" .= String "power", "reply" .= response,
                       "terminal_state" .= (parseMaybe (withObject "state" (.: "failure")) diagnostics :: Maybe Value),
                       "sites" .= [object ["address" .= ("0x" ++ showHex address ""), "label" .= label] | (address, label) <- destructorSites],
                       "hit_count" .= length hits]
  writeJson (receipt </> "power-destructor.json") result
  require (terminalState diagnostics == Just StockPowerOff) "POWER OFF did not reach the stock terminal power-off loop"
  pure result

checkMode :: Emulator -> FilePath -> ModeDial -> IO Value
checkMode emulator receipt position = do
  when (position == ModeDialShutter) (void (emulatorModeDial emulator ModeDialProgram))
  beforeWord <- readWord emulator 0x00956838 4
  before <- maybe (ioError (userError ("unknown baseline shooting state: " ++ show beforeWord))) pure (shootingMode beforeWord)
  response <- emulatorModeDial emulator position
  let ModeInputRow row = modeInputRow position
  selected <- readWord emulator (0x00828120 + 28 * row + 0x16) 1
  actualWord <- readWord emulator 0x00956838 4
  actual <- maybe (ioError (userError ("unknown consumed shooting state: " ++ show actualWord))) pure (shootingMode actualWord)
  capture <- emulatorCapture emulator ("mode-" ++ map (\c -> if isAlphaNum c then toLower c else '-') (modeDialName position))
  let result = object ["position" .= modeDialName position, "input_row" .= row,
                       "scanner_row_active" .= selected, "shooting_mode" .= actualWord,
                       "previous_shooting_mode" .= beforeWord, "mode_dial" .= response, "capture" .= capture]
  writeJson (receipt </> "mode-" ++ show (fromEnum position) ++ ".json") result
  require (selected == 1) (modeDialName position ++ " did not activate its stock scanner row")
  require (actual == expectedShootingMode position && actual /= before)
    (modeDialName position ++ " did not change to its stock shooting-mode state")
  pure result

run :: Options -> IO ()
run (Options nandInput cardInput receiptInput firmware selected fresh) = do
  arguments <- getArgs
  slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy arguments
  root@(Workspace workspace) <- Runtime.workspace Nothing
  nand <- maybe (ioError (userError "--installed-nand is required")) (privateInput root) nandInput
  card <- traverse (privateInput root) cardInput
  when (selected == CheckpointRestoreCase) $
    require (card == Nothing && not fresh) "checkpoint-restore requires cardless checkpoint restoration, without --fresh-boot"
  receipt <- freshReceipt root receiptInput
  staged <- stageViewerRuntime root Nothing (UniqueRuntime "xm1-viewer-inputs-e2e") firmware StageStockOriginal
  let runtime = stagedDirectory staged
      Python python = stagedPython staged
  callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
  guest <- guestState runtime (InterpreterExecutable python) (UnicornLibrary (stagedUnicorn staged))
  installed <- sha256File (runtime </> stagedFirmwarePath)
  adjustment <- sha256File nand
  originalCard <- traverse sha256File card
  let key = routeKey guest CardAttach
        ["profile" .= String "firmware/x-m1/1.50/emulator-rec.json", "installed_dat" .= sha256Hex installed,
         "adjustment_nand" .= sha256Hex adjustment, "menu_sd_image" .= fmap sha256Hex originalCard,
         "ui_conversion_profile" .= String "declared-ui-cpu-fallback"]
      common directory = ["--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-dat", stagedFirmwarePath,
                "--adjustment-nand", nand, "--output", directory </> "bridge", "--ui-conversion-profile", "declared-ui-cpu-fallback"]
      forceFresh = fresh || selected == FullPressCase
      requested = [ModeRun position | selected `elem` [DefaultCases, ModesCase], position <- [minBound .. maxBound]]
        ++ [ControlsRun | selected `elem` [DefaultCases, ControlsCase]]
        ++ [FullPressRun | selected == FullPressCase]
        ++ [PowerDestructorRun | selected == PowerDestructorCase]
  executable <- getExecutablePath
  let bridgeFor directory = case card of
        Nothing -> ((executable ++ ".runfiles") </> "_main/emulation/cardless-ram-push-bridge",
          common directory ++ ["--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push-cardless"] ++ ["--fresh-boot" | forceFresh])
        Just image -> (python, ["tools/continue_rec_scanout.py"] ++ common directory ++
          ["--menu-sd-image", image, "--menus", "--ram-push-server", "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push",
           "--checkpoint-key", routeKeyHex key, "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))] ++ ["--fresh-boot" | forceFresh])
      runOne request = do
        let directory = receipt </> caseName request
            (bridge, bridgeArgs) = bridgeFor directory
        ensurePrivateDirectory directory
        bracket (startEmulator bridge runtime bridgeArgs (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
          let baseline = object ["case" .= caseName request, "start" .= startRecord start]
          case request of
            ModeRun position -> do
              result <- checkMode emulator directory position
              pure (CaseReceipt baseline [result] [] [] [])
            ControlsRun -> do
              results <- checkControls emulator directory
              pure (CaseReceipt baseline [] results [] [])
            FullPressRun -> do
              result <- checkControl emulator directory FullPress
              pure (CaseReceipt baseline [] [] [result] [])
            PowerDestructorRun -> do
              result <- checkPowerDestructor emulator directory
              pure (CaseReceipt baseline [] [] [] [result])
      restoreRun name = do
        let directory = receipt </> name
            (bridge, bridgeArgs) = bridgeFor directory
        ensurePrivateDirectory directory
        bracket (startEmulator bridge runtime bridgeArgs (\_ -> pure (Nothing, ())))
          (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
            require (bootStart start == RestoredStart) "checkpoint-restore did not restore an existing attach checkpoint"
            beforeWord <- readWord emulator 0x00956838 4
            before <- maybe (ioError (userError "checkpoint has an unknown shooting mode")) pure (shootingMode beforeWord)
            changed <- checkMode emulator directory (if before == ManualState then ModeDialProgram else ModeDialManual)
            pure (before, CaseReceipt
              (object ["case" .= name, "start" .= startRecord start, "restored_mode" .= show before])
              [changed] [] [] [])
      restoreCase = do
        (firstMode, first) <- restoreRun "restore-first"
        (secondMode, second) <- restoreRun "restore-second"
        require (firstMode == secondMode) "checkpoint restoration retained the previous run's shooting-mode change"
        pure [first, second]
      unchanged = forM_ (zip (maybe [] pure card) (maybe [] pure originalCard)) $ \(path, before) -> do
        after <- sha256File path
        writeJson (receipt </> "card-integrity.json") (object ["original_sha256_before" .= sha256Hex before, "original_sha256_after" .= sha256Hex after])
        require (before == after) "The original virtual-card image changed"
  results <- (if selected == CheckpointRestoreCase then restoreCase else mapM runOne requested) `finally` unchanged
  writeJson (receipt </> "result.json") (object
    ["schema" .= (3 :: Int), "baselines" .= map baselineRecord results, "modes" .= concatMap modeRecords results,
     "controls" .= concatMap controlRecords results, "s2" .= concatMap s2Records results,
     "power_destructor" .= concatMap destructorRecords results])
  when (selected `notElem` [FullPressCase, PowerDestructorCase]) (putStrLn "Default viewer-input trajectories exclude S2")
  putStrLn ("Viewer input acceptance recorded: " ++ receipt </> "result.json")

main :: IO ()
main = (getArgs >>= \arguments -> if arguments == ["--help"] then putStr usage else parseOptions arguments >>= run)
  `catch` \problem -> hPutStrLn stderr (displayException (problem :: IOException)) >> exitFailure
