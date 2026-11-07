-- | The recorded candidate routes and their stable names.
module Emulation.CandidateRoute
  ( Route (..)
  , routeName
  , parseRoute
  , allRoutes
  ) where

data Route
  = FlashBootCheck
  | RamPushVolatile
  | RamPushClassicChrome
  | RamPushClassicChromeIcon
  | FlashCandidateIcon
  | FlashCandidateQTile
  | FlashCandidateRefresh
  | UsbToggleE8
  | UpdaterRc0Toggle
  | UpdaterCcToggle
  | NativeVolatile
  | NativeSd
  | FujiModFlashBootCheck
  | RamPushCcCardless
  | RamPushCcIconCardless
  | UsbToggleEmu
  | FujiModRamPushVolatile
  | FujiModRamPushCcCardless
  | FujiModRamPushCcIconCardless
  | FujiModRamPushCc
  | FujiModUsbToggleE8
  | FujiModUsbToggleEmu
  deriving (Eq, Ord, Enum, Bounded, Show)

routeName :: Route -> String
routeName route = case route of
  FlashBootCheck -> "flash-boot-check"
  RamPushVolatile -> "ram-push-volatile"
  RamPushClassicChrome -> "ram-push-cc"
  RamPushClassicChromeIcon -> "ram-push-cc-icon"
  FlashCandidateIcon -> "flash-candidate-icon-b"
  FlashCandidateQTile -> "flash-candidate-q-tile-b"
  FlashCandidateRefresh -> "flash-candidate-refresh"
  UsbToggleE8 -> "usb-toggle-e8"
  UpdaterRc0Toggle -> "updater-f7-rc0-toggle"
  UpdaterCcToggle -> "updater-f7-cc-toggle"
  NativeVolatile -> "native-volatile"
  NativeSd -> "native-sd"
  FujiModFlashBootCheck -> "fujimod-flash-boot-check"
  RamPushCcCardless -> "ram-push-cc-cardless"
  RamPushCcIconCardless -> "ram-push-cc-icon-cardless"
  UsbToggleEmu -> "usb-toggle-emu"
  FujiModRamPushVolatile -> "fujimod-ram-push-volatile"
  FujiModRamPushCcCardless -> "fujimod-ram-push-cc-cardless"
  FujiModRamPushCcIconCardless -> "fujimod-ram-push-cc-icon-cardless"
  FujiModRamPushCc -> "fujimod-ram-push-cc"
  FujiModUsbToggleE8 -> "fujimod-usb-toggle-e8"
  FujiModUsbToggleEmu -> "fujimod-usb-toggle-emu"

parseRoute :: String -> Maybe Route
parseRoute name = lookup name [(routeName route, route) | route <- allRoutes]

allRoutes :: [Route]
allRoutes = [minBound .. maxBound]
