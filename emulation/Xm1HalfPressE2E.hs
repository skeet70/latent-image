{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException)
import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (..), encode, object, toJSON, withObject, (.:), (.=))
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

prepareNoLens :: KeyTiming -> Emulator -> IO [Value]
prepareNoLens timing emulator = do
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
    pulse = emulatorKey emulator timing
    seek remaining history current = do
      MenuPosition _ cursor <- menuPosition current
      if cursor == MenuCursor 20 then pure (reverse history) else do
        require (remaining > 0) ("stock navigation did not reach SHOOT WITHOUT LENS row: " ++ show current)
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

data SdCase = StandardMediaCases | PresentInitFails deriving (Eq)

newtype SdFlags = SdFlags Word32 deriving (Eq)
data DetectState = DetectState B.ByteString SdFlags

captureDetect :: Emulator -> IO DetectState
captureDetect emulator = DetectState <$> emulatorReadMemory emulator 0x00828216 2
  <*> (SdFlags . B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0
    <$> emulatorReadMemory emulator 0x009738c4 4)

detectValue :: DetectState -> Value
detectValue (DetectState bytes (SdFlags flags)) =
  object ["detect_current_previous" .= B.unpack bytes, "raw_sd_flags" .= flags]

requireBodyMedia :: DetectState -> MediaState -> IO ()
requireBodyMedia (DetectState bytes flags) (MediaState (MediaStatus status) (WarningId warning) (WarningMask active)) =
  require (bytes == B.pack [1, 1] && flags == SdFlags 0x10 && status == 4 && warning == 0x13 && active == 0x00080000)
    "present-init-fails did not reproduce E-CARD-DETECT-LIVE-150"

newtype ShootingUiState = ShootingUiState Word32

captureShootingUiState :: Emulator -> IO ShootingUiState
captureShootingUiState emulator = ShootingUiState . B.foldr (\byte rest -> fromIntegral byte .|. rest `shiftL` 8) 0
  <$> emulatorReadMemory emulator 0x0146761c 2

shootingUiValue :: ShootingUiState -> Value
shootingUiValue (ShootingUiState state) = toJSON state

refusalTraceSites :: [(Word32, String)]
refusalTraceSites =
  [(0x006a2f94, "s1-irq"), (0x00c09d24, "key-dispatch"), (0x00c58c38, "s1-action"),
   (0x00d24e34, "s1-admission"), (0x00d19324, "media-admission"),
   (0x00df48dc, "af-request"), (0x00ef9264, "af-send"), (0x00c58ccc, "s1-admission-result"),
   (0x00bfb0c8, "ui-handler"), (0x00d3bb18, "warning-select"),
   (0x00d3b64c, "warning-present"), (0x00d39410, "indicator-request"),
   (0x00db50b0, "indicator-forward"), (0x006a4d84, "indicator-driver"),
   (0x006a486c, "indicator-gpio-assert"), (0x006a48c4, "indicator-gpio-release")]

traceValue :: [TraceHit] -> Value
traceValue hits = toJSON [(traceSite hit, traceR0 hit, traceR1 hit, traceR2 hit, traceR3 hit) | hit <- hits]

data Options = Options
  { nandPath :: Maybe FilePath
  , cardPath :: Maybe FilePath
  , receiptPath :: Maybe FilePath
  , firmwarePath :: Maybe FilePath
  , freshBoot :: Bool
  , sdCase :: SdCase
  }

usage :: String
usage = "xm1-half-press-e2e --installed-nand NAND --receipt-output DIR [--sd-card-image CARD | --sd-profile present-init-fails] [--firmware DAT] [--fresh-boot]\nHost-only stock X-M1 1.50 S1 press/release and console-store AF request assertion. No payload or camera I/O.\n"

parseOptions :: [String] -> IO Options
parseOptions = go (Options Nothing Nothing Nothing Nothing False StandardMediaCases)
  where
    go options [] = pure options
    go options ("--installed-nand" : value : rest) = go options { nandPath = Just value } rest
    go options ("--sd-card-image" : value : rest) = go options { cardPath = Just value } rest
    go options ("--receipt-output" : value : rest) = go options { receiptPath = Just value } rest
    go options ("--firmware" : value : rest) = go options { firmwarePath = Just value } rest
    go options ("--fresh-boot" : rest) = go options { freshBoot = True } rest
    go options ("--sd-profile" : "present-init-fails" : rest) = go options { sdCase = PresentInitFails } rest
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
  card <- case sdCase options of
    StandardMediaCases -> privateInput root "--sd-card-image" (cardPath options)
    PresentInitFails -> require (cardPath options == Nothing) "present-init-fails does not use a card image" >> pure ""
  receipt <- freshReceipt root (receiptPath options)
  staged <- stageViewerRuntime root Nothing (UniqueRuntime "xm1-half-press-e2e") (Firmware <$> firmwarePath options) StageStockOriginal
  let runtime = stagedDirectory staged
      Python python = stagedPython staged
  callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
  guest <- guestState runtime (InterpreterExecutable python) (UnicornLibrary (stagedUnicorn staged))
  installed <- sha256File (runtime </> stagedFirmwarePath)
  adjustment <- sha256File nand
  cardKeyFields <- case sdCase options of
    StandardMediaCases -> do
      cardDigest <- sha256File card
      pure ["menu_sd_image" .= sha256Hex cardDigest]
    PresentInitFails -> pure []
  let key = routeKey guest CardAttach
        ([ "profile" .= String "firmware/x-m1/1.50/emulator-rec.json", "installed_dat" .= sha256Hex installed
        , "adjustment_nand" .= sha256Hex adjustment
        , "ui_conversion_profile" .= String "declared-ui-cpu-fallback" ] ++ cardKeyFields)
      bridgeArguments =
        [ "tools/continue_rec_scanout.py", "--profile", "firmware/x-m1/1.50/emulator-rec.json"
        , "--installed-dat", stagedFirmwarePath, "--adjustment-nand", nand, "--menu-sd-image", card
        , "--menus", "--ram-push-server", "--output", receipt </> "bridge"
        , "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push"
        , "--checkpoint-key", routeKeyHex key, "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))
        , "--ui-conversion-profile", "declared-ui-cpu-fallback" ] ++ ["--fresh-boot" | freshBoot options]
  executable <- getExecutablePath
  let cardlessBridge = (executable ++ ".runfiles") </> "_main/emulation/cardless-ram-push-bridge"
      cardlessArguments label =
        [ "--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-dat", stagedFirmwarePath
        , "--adjustment-nand", nand, "--output", receipt </> label </> "bridge"
        , "--ram-push-checkpoints", workspace </> "private-data/emulator-checkpoints/xm1-ram-push"
        , "--ui-conversion-profile", "declared-ui-cpu-fallback" ] ++ ["--fresh-boot" | freshBoot options]
      cases = case sdCase options of
        StandardMediaCases -> [(True, "card-present", python, bridgeArguments), (False, "card-absent", cardlessBridge, cardlessArguments "card-absent")]
        PresentInitFails -> [(True, "present-init-fails", cardlessBridge, cardlessArguments "present-init-fails" ++ ["--sd-profile", "present-init-fails"])]
  results <- forM cases $ \(present, label, bridge, bridgeArgs) -> do
    let directory = receipt </> label
    ensurePrivateDirectory directory
    when (sdCase options == PresentInitFails) $ do
      let menuLabel = label ++ "-initial-menu"
          menuArguments = cardlessArguments menuLabel ++ ["--sd-profile", "present-init-fails"]
      ensurePrivateDirectory (receipt </> menuLabel)
      bracket (startEmulator bridge runtime menuArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, _, ()) -> do
        initialDetect <- captureDetect emulator
        initialMedia <- captureMedia emulator
        requireBodyMedia initialDetect initialMedia
        uiBefore <- captureShootingUiState emulator
        emulatorTraceSites emulator refusalTraceSites
        menu <- emulatorKey emulator (KeyTiming (Just (KeyPulse 12000000 12000000)) Nothing) "ok"
        (_, hits) <- emulatorTraceHits emulator 0
        media <- captureMedia emulator
        detect <- captureDetect emulator
        uiAfter <- captureShootingUiState emulator
        requireBodyMedia detect media
        writeJson (directory </> "initial-menu.json") (object
          ["menu" .= menu, "trace" .= traceValue hits, "media" .= mediaValue media, "detect" .= detectValue detect,
           "ui_state_before" .= shootingUiValue uiBefore, "ui_state_after" .= shootingUiValue uiAfter])
    bracket (startEmulator bridge runtime bridgeArgs (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      shootingMedia <- captureMedia emulator
      shootingDetect <- captureDetect emulator
      when (sdCase options == PresentInitFails) (requireBodyMedia shootingDetect shootingMedia)
      let timing = case sdCase options of
            StandardMediaCases -> KeyTiming Nothing Nothing
            PresentInitFails -> KeyTiming (Just (KeyPulse 12000000 12000000)) Nothing
      when (sdCase options == PresentInitFails) $ do
        emulatorTraceSites emulator refusalTraceSites
        uiBefore <- captureShootingUiState emulator
        initialS1 <- emulatorKey emulator (KeyTiming (Just (KeyPulse 8000000 8000000)) Nothing) "half-press"
        (afterInitialS1, initialS1Hits) <- emulatorTraceHits emulator 0
        uiAfter <- captureShootingUiState emulator
        initialMedia <- captureMedia emulator
        initialDetect <- captureDetect emulator
        requireBodyMedia initialDetect initialMedia
        initialMenu <- emulatorKey emulator timing "ok"
        (_, initialMenuHits) <- emulatorTraceHits emulator afterInitialS1
        writeJson (directory </> "initial-warning-inputs.json") (object
          ["half_press" .= initialS1, "half_press_trace" .= traceValue initialS1Hits,
           "half_press_media" .= mediaValue initialMedia, "half_press_detect" .= detectValue initialDetect,
           "ui_state_before" .= shootingUiValue uiBefore, "ui_state_after_half_press" .= shootingUiValue uiAfter,
           "menu" .= initialMenu, "menu_trace" .= traceValue initialMenuHits])
      dismissed <- case sdCase options of
        StandardMediaCases -> pure []
        PresentInitFails -> (: []) <$> emulatorKey emulator timing "back"
      preparation <- prepareNoLens timing emulator
      writeJson (directory </> "no-lens-preparation.json") (object ["stock_keys" .= (dismissed ++ preparation)])
      mediaBefore <- captureMedia emulator
      detectBefore <- captureDetect emulator
      when (sdCase options == PresentInitFails) (requireBodyMedia detectBefore mediaBefore)
      lensBefore <- captureLens emulator
      baseline <- captureStore emulator (directory </> "before-store.bin")
      emulatorTraceSites emulator refusalTraceSites
      (traceStart, _) <- emulatorTraceHits emulator 0
      response <- emulatorKey emulator (KeyTiming (Just (KeyPulse 8000000 8000000)) Nothing) "half-press"
      lensAfter <- captureLens emulator
      after <- captureStore emulator (directory </> "after-store.bin")
      mediaAfter <- captureMedia emulator
      detectAfter <- captureDetect emulator
      when (sdCase options == PresentInitFails) (requireBodyMedia detectAfter mediaAfter)
      released <- emulatorReadMemory emulator 0x008281c1 1
      (_, hits) <- emulatorTraceHits emulator traceStart
      let beforeCount = length (afRequests baseline)
          afterCount = length (afRequests after)
          newRequest = any (\entry -> Set.notMember (enRaw entry) priorRequests) (afRequests after)
          priorRequests = Set.fromList (map enRaw (afRequests baseline))
          result = object
            [ "card_present" .= present, "emulator_start" .= startRecord start, "half_press" .= response
            , "lens_before" .= lensValue lensBefore, "lens_after" .= lensValue lensAfter
            , "configuration_difference" .= String "SHOOT WITHOUT LENS ON, no initialized emulated lens; body has Viltrox AF 28mm F4.5 mounted"
            , "media_before" .= mediaValue mediaBefore, "media_after" .= mediaValue mediaAfter
            , "shooting_screen_media" .= mediaValue shootingMedia, "shooting_screen_detect" .= detectValue shootingDetect
            , "detect_before" .= detectValue detectBefore, "detect_after" .= detectValue detectAfter
            , "af_request_before" .= beforeCount, "af_request_after" .= afterCount
            , "new_s1_af_request" .= newRequest
            , "missing_input" .= (if newRequest || cardErrorActive mediaBefore then Null else String "X-M1 lens attach, initialization and lenscom reply model; see reports/autofocus-static-map.md")
            , "debounced_s1_released" .= (released == B.singleton 0)
            , "trace" .= traceValue hits
            , "scope" .= String "Stock message 0x3b0d send to saclac, not accepted AF statistics or physical focus" ]
      writeJson (directory </> "result.json") result
      when (sdCase options == PresentInitFails) $ do
        require (any ((== "media-admission") . traceSite) hits) "S1 did not reach the stock media predicate"
        require (any ((== "s1-admission-result") . traceSite) hits) "S1 did not reach the stock admission result"
        require (all (\hit -> traceSite hit /= "s1-admission-result" || traceR0 hit == 3) hits) "CARD ERROR did not return the stock media refusal"
        require (any (\hit -> traceSite hit == "indicator-driver" && (traceR0 hit, traceR1 hit, traceR2 hit, traceR3 hit) == (1, 3, 250, 250)) hits)
          "CARD ERROR S1 did not request the stock warning indicator"
      pure (result, present, newRequest, cardErrorActive mediaBefore, released == B.singleton 0)
  writeJson (receipt </> "result.json") (object ["schema" .= (1 :: Int), "cases" .= [result | (result, _, _, _, _) <- results]])
  forM_ results $ \(_, present, newRequest, cardError, released) -> do
    when (present && sdCase options == StandardMediaCases) (require newRequest "Missing X-M1 lens attach/reply model: S1 with the virtual card did not produce a new 0x3b0d AF request in the console store; see reports/autofocus-static-map.md")
    when cardError (require (not newRequest) "CARD ERROR did not inhibit the stock S1 AF request")
    require released "S1 did not debounce to released"
  putStrLn ("Stock S1 media trajectories recorded: " ++ receipt </> "result.json")

main :: IO ()
main = (getArgs >>= \arguments -> if arguments == ["--help"] then putStr usage else parseOptions arguments >>= run)
  `catch` \problem -> hPutStrLn stderr (displayException (problem :: IOException)) >> exitFailure
