{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException)
import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (..), encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (parseMaybe)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import qualified Data.Set as Set
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, resolve, stageViewerRuntime, stagedFirmwarePath)
import qualified Emulation.ViewerRuntime as Runtime
import EmulatorUsb
import FujiAF.ConsoleStore (Argument (..), Entry (..), Input (..), RecordType (..), Store (..), loadStore)
import FujiAF.Transport (ensurePrivateDirectory, writePrivateBytes)
import System.Directory (canonicalizePath, doesPathExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (hPutStrLn, stderr)
import System.Process (callProcess)
import Data.Word (Word32)

newtype MenuCursor = MenuCursor Int deriving (Eq, Show)
newtype SettingId = SettingId Word32 deriving (Eq, Show)
data MenuPosition = MenuPosition SettingId MenuCursor deriving (Show)

menuPosition :: Value -> IO MenuPosition
menuPosition value = maybe (ioError (userError "missing stock menu position")) pure $
  parseMaybe (withObject "key reply" $ \reply -> reply .: "menu" >>= withObject "menu" (\menu ->
    MenuPosition <$> (SettingId <$> menu .: "setting_id") <*> (MenuCursor <$> menu .: "cursor"))) value

prepareNoLens :: Emulator -> IO [Value]
prepareNoLens emulator = do
  opened <- pulse "ok"
  navigation <- seek (64 :: Int) [opened] opened
  lensMenu <- pulse "right"
  MenuPosition setting _ <- menuPosition lensMenu
  require (setting == SettingId 0xa3) "stock navigation did not open SHOOT WITHOUT LENS"
  selected <- pulse "up"
  MenuPosition _ cursor <- menuPosition selected
  require (cursor == MenuCursor 0) "stock navigation did not select SHOOT WITHOUT LENS ON"
  committed <- pulse "ok"
  closed <- pulse "back"
  pure (navigation ++ [lensMenu, selected, committed, closed])
  where
    pulse = emulatorKey emulator (KeyTiming Nothing Nothing)
    seek remaining history current = do
      MenuPosition _ cursor <- menuPosition current
      if cursor == MenuCursor 20 then pure (reverse history) else do
        require (remaining > 0) "stock navigation did not reach SHOOT WITHOUT LENS row"
        next <- pulse "down"
        seek (remaining - 1) (next : history) next

newtype LensStateWord = LensStateWord Word32
data LensAdmission = LensAdmission LensStateWord LensStateWord

lensValue :: LensAdmission -> Value
lensValue (LensAdmission (LensStateWord first) (LensStateWord second)) =
  object ["word_009504d4" .= first, "word_00950508" .= second]

captureLens :: Emulator -> IO LensAdmission
captureLens emulator = LensAdmission <$> (word 0x009504d4) <*> (word 0x00950508)
  where
    word address = LensStateWord . B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0 <$> emulatorReadMemory emulator address 4

newtype MediaStatus = MediaStatus Word32
newtype WarningId = WarningId Word32
newtype WarningMask = WarningMask Word32
data MediaState = MediaState MediaStatus WarningId WarningMask

mediaValue :: MediaState -> Value
mediaValue (MediaState (MediaStatus status) (WarningId warning) (WarningMask active)) =
  object ["media_status" .= status, "cached_warning" .= warning, "active_warning_mask" .= active]

cardErrorActive :: MediaState -> Bool
cardErrorActive (MediaState _ _ (WarningMask active)) = active .&. (1 `shiftL` 0x13) /= 0

captureMedia :: Emulator -> IO MediaState
captureMedia emulator = MediaState <$> (MediaStatus <$> word 0x00918af8) <*> (WarningId <$> word 0x00950114) <*> (WarningMask <$> word 0x00950118)
  where
    word address = B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0 <$> emulatorReadMemory emulator address 4

data Options = Options
  { nandPath :: Maybe FilePath
  , cardPath :: Maybe FilePath
  , receiptPath :: Maybe FilePath
  , firmwarePath :: Maybe FilePath
  , freshBoot :: Bool
  }

usage :: String
usage = "xm1-half-press-e2e --installed-nand NAND --sd-card-image CARD --receipt-output DIR [--firmware DAT] [--fresh-boot]\nHost-only stock X-M1 1.50 S1 press/release and console-store AF request assertion. No payload or camera I/O.\n"

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing False)
  where
    go options [] = pure options
    go options ("--installed-nand" : value : rest) = go options { nandPath = Just value } rest
    go options ("--sd-card-image" : value : rest) = go options { cardPath = Just value } rest
    go options ("--receipt-output" : value : rest) = go options { receiptPath = Just value } rest
    go options ("--firmware" : value : rest) = go options { firmwarePath = Just value } rest
    go options ("--fresh-boot" : rest) = go options { freshBoot = True } rest
    go _ _ = ioError (userError usage)

privateInput :: Workspace -> String -> Maybe FilePath -> IO FilePath
privateInput root label supplied = do
  value <- maybe (ioError (userError (label ++ " is required"))) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root value)
  require (splitDirectories private `isPrefixOf` splitDirectories path) (label ++ " must be beneath private-data/")
  pure path

freshReceipt :: Workspace -> Maybe FilePath -> IO FilePath
freshReceipt root supplied = do
  value <- maybe (ioError (userError "--receipt-output is required")) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  let path = resolve root value
  parent <- canonicalizePath (takeDirectory path)
  let destination = parent </> takeFileName path
  exists <- doesPathExist destination
  require (splitDirectories private `isPrefixOf` splitDirectories parent && not exists && takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must be a new directory beneath private-data/"
  ensurePrivateDirectory destination
  pure destination

writeJson :: FilePath -> Value -> IO ()
writeJson path = writePrivateBytes path . (<> "\n") . BL.toStrict . encode

afRequests :: Store -> [Entry]
afRequests store = [entry | entry <- entries store, enType entry == Message
                         , let Argument word = enArg entry
                         , word `shiftR` 16 == 0x3b0d, word .&. 0xff == 0x2b]

captureStore :: Emulator -> FilePath -> IO Store
captureStore emulator path = do
  chunks <- forM [0, 4096 .. 0x1fff3] $ \offset ->
    emulatorReadMemory emulator (0x400a0800 + fromIntegral offset) (min 4096 (0x1fff4 - offset))
  writePrivateBytes path (B.concat chunks)
  loadStore (StoreFile path)

run :: Options -> IO ()
run options = do
  arguments <- getArgs
  slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy arguments
  root@(Workspace workspace) <- Runtime.workspace Nothing
  nand <- privateInput root "--installed-nand" (nandPath options)
  card <- privateInput root "--sd-card-image" (cardPath options)
  receipt <- freshReceipt root (receiptPath options)
  staged <- stageViewerRuntime root Nothing (UniqueRuntime "xm1-half-press-e2e") (Firmware <$> firmwarePath options) StageStockOriginal
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
        , "--ui-conversion-profile", "declared-ui-cpu-fallback" ] ++ ["--fresh-boot" | freshBoot options]
  executable <- getExecutablePath
  let cardlessBridge = (executable ++ ".runfiles") </> "_main/emulation/cardless-ram-push-bridge"
      cardlessArguments =
        [ "--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-dat", stagedFirmwarePath
        , "--adjustment-nand", nand, "--output", receipt </> "card-absent/bridge"
        , "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push"
        , "--ui-conversion-profile", "declared-ui-cpu-fallback" ] ++ ["--fresh-boot" | freshBoot options]
  results <- forM [(True, "card-present", python, bridgeArguments), (False, "card-absent", cardlessBridge, cardlessArguments)] $ \(present, label, bridge, bridgeArgs) -> do
    let directory = receipt </> label
    ensurePrivateDirectory directory
    bracket (startEmulator bridge runtime bridgeArgs (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      preparation <- prepareNoLens emulator
      writeJson (directory </> "no-lens-preparation.json") (object ["stock_keys" .= preparation])
      mediaBefore <- captureMedia emulator
      lensBefore <- captureLens emulator
      baseline <- captureStore emulator (directory </> "before-store.bin")
      emulatorTraceSites emulator
        [(0x006a2f94, "s1-irq"), (0x00c09d24, "key-dispatch"), (0x00c58c38, "s1-action"),
         (0x00d24e34, "s1-admission"), (0x00d19324, "media-admission"),
         (0x00df48dc, "af-request"), (0x00ef9264, "af-send"),
         (0x00c58ccc, "s1-admission-result")]
      response <- emulatorKey emulator (KeyTiming (Just (KeyPulse 8000000 8000000)) Nothing) "half-press"
      lensAfter <- captureLens emulator
      after <- captureStore emulator (directory </> "after-store.bin")
      mediaAfter <- captureMedia emulator
      released <- emulatorReadMemory emulator 0x008281c1 1
      (_, hits) <- emulatorTraceHits emulator 0
      let beforeCount = length (afRequests baseline)
          afterCount = length (afRequests after)
          newRequest = any (\entry -> Set.notMember (enRaw entry) priorRequests) (afRequests after)
          priorRequests = Set.fromList (map enRaw (afRequests baseline))
          result = object
            [ "card_present" .= present, "emulator_start" .= startRecord start, "half_press" .= response
            , "lens_before" .= lensValue lensBefore, "lens_after" .= lensValue lensAfter
            , "configuration_difference" .= String "SHOOT WITHOUT LENS ON, no initialized emulated lens; body has Viltrox AF 28mm F4.5 mounted"
            , "media_before" .= mediaValue mediaBefore, "media_after" .= mediaValue mediaAfter
            , "af_request_before" .= beforeCount, "af_request_after" .= afterCount
            , "new_s1_af_request" .= newRequest
            , "missing_input" .= (if newRequest then Null else String "X-M1 lens attach, initialization and lenscom reply model; see reports/autofocus-static-map.md")
            , "debounced_s1_released" .= (released == B.singleton 0)
            , "trace" .= [(traceSite hit, traceR0 hit, traceR1 hit, traceR2 hit, traceR3 hit) | hit <- hits]
            , "scope" .= String "Stock message 0x3b0d send to saclac, not accepted AF statistics or physical focus" ]
      writeJson (directory </> "result.json") result
      pure (result, present, newRequest, cardErrorActive mediaBefore, released == B.singleton 0)
  writeJson (receipt </> "result.json") (object ["schema" .= (1 :: Int), "cases" .= [result | (result, _, _, _, _) <- results]])
  forM_ results $ \(_, present, newRequest, cardError, released) -> do
    when present (require newRequest "Missing X-M1 lens attach/reply model: S1 with the virtual card did not produce a new 0x3b0d AF request in the console store; see reports/autofocus-static-map.md")
    when cardError (require (not newRequest) "CARD ERROR did not inhibit the stock S1 AF request")
    require released "S1 did not debounce to released"
  putStrLn ("Stock S1 media trajectories recorded: " ++ receipt </> "result.json")

main :: IO ()
main = (getArgs >>= \arguments -> if arguments == ["--help"] then putStr usage else parseOptions arguments >>= run)
  `catch` \problem -> hPutStrLn stderr (displayException (problem :: IOException)) >> exitFailure
