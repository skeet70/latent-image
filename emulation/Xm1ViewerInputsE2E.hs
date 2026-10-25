{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException, finally)
import Control.Monad (forM, forM_, void, when)
import Data.Aeson (Value (..), encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (parseMaybe)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, toLower)
import Data.List (isPrefixOf)
import Data.Word (Word32)
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
data InputControl = MainUp | MainDown | HalfPress | FullPress | Movie deriving (Eq, Show)
data Case = DefaultCases | ControlsCase | ModesCase | FullPressCase deriving (Eq)
data Options = Options (Maybe InputPath) (Maybe InputPath) (Maybe ReceiptPath) (Maybe Firmware) Case Bool

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
usage = "xm1-viewer-inputs-e2e --installed-nand NAND --receipt-output DIR [--sd-card-image CARD --firmware DAT --case controls|modes|s2 --fresh-boot]\nHost-only stock input consumers and mode menus. S2 runs only with --case s2, in a fresh run with no card or a private card copy.\n"

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
  opened <- pulse emulator "ok"
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
  before <- readWord emulator 0x009500f0 4
  response <- pulse emulator "power"
  after <- readWord emulator 0x009500f0 4
  let result = object ["control" .= String "power", "reply" .= response, "mode_before" .= before, "mode_after" .= after]
  writeJson (receipt </> "power.json") result
  require (after /= before && after `elem` [0xc, 0xd]) "POWER OFF did not change the firmware application mode to a shutdown mode"
  pure (controls ++ [result])

checkModes :: Emulator -> FilePath -> IO [Value]
checkModes emulator receipt = forM [minBound .. maxBound] $ \position -> do
  void (emulatorModeDial emulator ModeDialShutter)
  void (emulatorModeDial emulator position)
  void (pulse emulator "back")
  response <- pulse emulator "ok"
  (menu, setting, _) <- menuState response
  actual <- readWord emulator 0x00956838 4
  capture <- emulatorCapture emulator ("mode-" ++ map (\c -> if isAlphaNum c then toLower c else '-') (modeDialName position))
  let result = object ["position" .= modeDialName position, "shooting_mode" .= actual, "menu" .= response, "capture" .= capture]
  writeJson (receipt </> "mode-" ++ show (fromEnum position) ++ ".json") result
  require (menu == 1 && setting == 5) (modeDialName position ++ " did not reach SHOOTING MENU")
  case position of
    ModeDialShutter -> require (actual == 0x22) "S did not select the stock shutter-priority state"
    ModeDialProgram -> require (actual == 0x21) "P did not select the stock program state"
    ModeDialAperture -> require (actual == 0x23) "A did not select the stock aperture-priority state"
    ModeDialManual -> require (actual == 0x24) "M did not select the stock manual state"
    ModeDialCustom -> require (actual == 0x2c) "C did not select the stock custom state"
    ModeDialSports -> require (actual == 0xb) "Sports did not select the stock action state"
    ModeDialLandscape -> require (actual == 8) "Landscape did not select the stock landscape state"
    ModeDialPortrait -> require (actual == 5) "Portrait did not select the stock portrait state"
    ModeDialSRAuto -> require (actual == 1) "SR+ did not select the stock scene-recognition state"
    _ -> require (actual /= 0x22) (modeDialName position ++ " left the stock shutter-priority state unchanged")
  void (pulse emulator "back")
  pure result

run :: Options -> IO ()
run (Options nandInput cardInput receiptInput firmware selected fresh) = do
  arguments <- getArgs
  slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy arguments
  root@(Workspace workspace) <- Runtime.workspace Nothing
  nand <- maybe (ioError (userError "--installed-nand is required")) (privateInput root) nandInput
  card <- traverse (privateInput root) cardInput
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
      common = ["--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-dat", stagedFirmwarePath,
                "--adjustment-nand", nand, "--output", receipt </> "bridge", "--ui-conversion-profile", "declared-ui-cpu-fallback"]
      forceFresh = fresh || selected == FullPressCase
  executable <- getExecutablePath
  let (bridge, bridgeArgs) = case card of
        Nothing -> ((executable ++ ".runfiles") </> "_main/emulation/cardless-ram-push-bridge",
          common ++ ["--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push-cardless"] ++ ["--fresh-boot" | forceFresh])
        Just image -> (python, ["tools/continue_rec_scanout.py"] ++ common ++
          ["--menu-sd-image", image, "--menus", "--ram-push-server", "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push",
           "--checkpoint-key", routeKeyHex key, "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))] ++ ["--fresh-boot" | forceFresh])
      unchanged = forM_ (zip (maybe [] pure card) (maybe [] pure originalCard)) $ \(path, before) -> do
        after <- sha256File path
        writeJson (receipt </> "card-integrity.json") (object ["original_sha256_before" .= sha256Hex before, "original_sha256_after" .= sha256Hex after])
        require (before == after) "The original virtual-card image changed"
  (bracket (startEmulator bridge runtime bridgeArgs (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    modeResults <- if selected `elem` [DefaultCases, ModesCase] then checkModes emulator receipt else pure []
    controlResults <- if selected `elem` [DefaultCases, ControlsCase] then checkControls emulator receipt else pure []
    shutterResults <- if selected == FullPressCase then (:[]) <$> checkControl emulator receipt FullPress else pure []
    writeJson (receipt </> "result.json") (object ["schema" .= (1 :: Int), "start" .= startRecord start, "modes" .= modeResults, "controls" .= controlResults, "s2" .= shutterResults])
    when (selected /= FullPressCase) (putStrLn "Default viewer-input trajectories exclude S2")
    putStrLn ("Viewer input acceptance recorded: " ++ receipt </> "result.json")) `finally` unchanged

main :: IO ()
main = (getArgs >>= \arguments -> if arguments == ["--help"] then putStr usage else parseOptions arguments >>= run)
  `catch` \problem -> hPutStrLn stderr (displayException (problem :: IOException)) >> exitFailure
