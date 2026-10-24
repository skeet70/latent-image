{-# LANGUAGE OverloadedStrings #-}
-- | Drives the FujiMod RAM-push bridge (@fujimod-xm1-ram-push@) through the
-- unchanged 'EmulatorUsb' client on a cold boot of one DAT: start, list, a
-- transport identify over transfer, read-memory, key and mode-dial, each
-- against an expected stock behaviour.
module Main where

import Control.Exception (IOException, SomeException, bracket, catch, displayException, try)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (modifyIORef', newIORef, readIORef)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), freshRuntime, resolve, validateFirmware, workspace)
import EmulatorUsb
import FujiAF.Identify (ProductModel (..), runIdentify)
import FujiAF.Transport (UsbApi (..), realClock)
import FujiModPaths (fujimodRamPushBridge, fujimodRun)
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

usage :: String
usage = unlines
  [ "Usage: bazel run //emulation:fujimod-ram-push-check -- --installed-nand NAND [--firmware DAT] [--output DIR]"
  , ""
  , "Cold-boot DAT (default: the stock 1.50 DAT) with the adjustment NAND on the FujiMod RAM-push"
  , "bridge, attach USB and check start, list, transfer (a transport identify), read-memory, key"
  , "and mode-dial through emulation/EmulatorUsb.hs. Host-only: no camera or card-reader I/O."
  ]

data Options = Options (Maybe Runtime) Firmware FilePath

parseOptions :: [String] -> IO (Maybe Options)
parseOptions arguments
  | "--help" `elem` arguments = pure Nothing
  | otherwise = Just <$> go Nothing (Firmware "private-data/firmware/x-m1-1.50/FWUP0002.DAT") Nothing arguments
  where
    go output firmware (Just nand) [] = pure (Options output firmware nand)
    go _ _ Nothing [] = ioError (userError ("--installed-nand is required\n" ++ usage))
    go output _ nand ("--firmware" : value : rest) | not (null value) = go output (Firmware value) nand rest
    go _ firmware nand ("--output" : value : rest) | not (null value) = go (Just (Runtime value)) firmware nand rest
    go output firmware _ ("--installed-nand" : value : rest) | not (null value) = go output firmware (Just value) rest
    go _ _ _ (argument : _) = ioError (userError ("Unknown option or missing value: " ++ argument ++ "\n" ++ usage))

-- | The UP row of the control table at 0x828120: code 2 in row 14 of 28-byte rows.
upKeyRow :: Word32
upKeyRow = 0x828120 + 14 * 28

-- | The word image 4 reads as its heap break.
heapBreak :: Word32
heapBreak = 0x00829a08

-- | From M-mode live view a MENU opens SHOOTING MENU (1,5,2) and LEFT moves to its tab row
-- (0,5,0), as in the recorded volatile-bank trajectory.
shootingMenu, tabRow :: Value
shootingMenu = object ["menu_id" .= (1 :: Int), "setting_id" .= (5 :: Int), "cursor" .= (2 :: Int)]
tabRow = object ["menu_id" .= (0 :: Int), "setting_id" .= (5 :: Int), "cursor" .= (0 :: Int)]

leWord :: B.ByteString -> Word32
leWord bytes = foldr (\byte acc -> acc `shiftL` 8 .|. fromIntegral byte) 0 (B.unpack (B.take 4 bytes))

field :: String -> Value -> Maybe Value
field name (Object fields) = M.lookup (K.fromString name) fields
field _ _ = Nothing

text :: String -> Value
text = String . T.pack

check :: Options -> IO ()
check (Options output (Firmware supplied) nandSupplied) = do
  root <- workspace Nothing
  let dat = resolve root supplied
      nand = resolve root nandSupplied
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  validateFirmware (Firmware dat)
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  Runtime directory <- freshRuntime root output (UniqueRuntime "fujimod-ram-push-check")
  results <- newIORef []
  let record name passed detail = do
        modifyIORef' results (++ [object ["command" .= (name :: String), "passed" .= passed, "detail" .= detail]])
        putStrLn (name ++ ": " ++ (if passed then "passed " else "failed ") ++ show detail)
      bridgeArguments =
        [ "--run", fujimodRun, "--boot-rom", rom, "--installed-dat", dat, "--adjustment-nand", nand
        , "--output", directory </> "bridge", "--ram-push-checkpoints", directory </> "checkpoints", "--fresh-boot" ]
  outcome <- try $ bracket
    (startEmulator fujimodRamPushBridge directory bridgeArguments (\installed -> pure (Nothing, length installed)))
    (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, listed) -> do
      record "start" (bootStart start == FreshBootStart && listed == 0) (text (show (bootStart start)))
      let api = emulatorUsbApi emulator
      candidates <- usbList api 1000
      record "list" (length candidates == 1) (Number (fromIntegral (length candidates)))
      createDirectory (directory </> "identify")
      identified <- try (runIdentify realClock api (directory </> "identify"))
      case identified of
        Right (ProductModel model) -> record "transfer" (model == "X-M1") (text model)
        Left problem -> record "transfer" False (text (displayException (problem :: SomeException)))
      row <- emulatorReadMemory emulator upKeyRow 28
      breakWord <- leWord <$> emulatorReadMemory emulator heapBreak 4
      record "read-memory" (leWord row == 2 && breakWord > 0x0089ec00 && breakWord < 0x20000000)
        (object ["up_row_code" .= leWord row, "heap_break" .= breakWord])
      dial <- emulatorModeDial emulator ModeDialManual
      let number name = case field name dial of Just (Number value) -> Just value; _ -> Nothing
          inWindow = case (number "low", number "sample", number "high") of
            (Just low, Just sample, Just high) -> low <= sample && sample <= high
            _ -> False
      record "mode-dial" (field "position" dial == Just (String "M") && inWindow) dial
      menu <- emulatorKey emulator Nothing "menu"
      left <- emulatorKey emulator Nothing "left"
      record "key" (field "menu" menu == Just shootingMenu && field "menu" left == Just tabRow)
        (object ["menu" .= menu, "left" .= left])
  passed <- readIORef results
  let failure = either (Just . displayException) (const Nothing) (outcome :: Either SomeException ())
      allPassed = failure == Nothing && length passed == 6 && all (\value -> field "passed" value == Just (Bool True)) passed
      receipt = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 RAM-push bridge commands on FujiMod's EXR II runtime" :: String)
        , "result" .= (if allPassed then "passed" else "failed" :: String)
        , "firmware" .= dat, "installed_nand" .= nand, "bridge" .= fujimodRamPushBridge, "run_document" .= fujimodRun
        , "commands" .= passed, "failure" .= failure ]
  BL.writeFile (directory </> "fujimod-ram-push-check.json") (encode receipt <> "\n")
  putStrLn ("fujimod-ram-push-check receipt: " ++ directory </> "fujimod-ram-push-check.json")
  if allPassed
    then putStrLn "fujimod-ram-push-check: passed"
    else hPutStrLn stderr ("fujimod-ram-push-check: failed" ++ maybe "" (": " ++) failure) >> exitFailure

main :: IO ()
main = (getArgs >>= parseOptions >>= maybe (putStr usage) check) `catch` report
  where
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
