-- | The X-M1 1.50 live-settings block, read around a RAM-push session that must leave it unchanged.
module Emulation.LiveSettings
  ( LiveSettings
  , liveSettings
  , LiveSettingChange (..)
  , liveSettingChange
  , describeLiveSettingChange
  , readLiveSettings
  ) where

import qualified Data.ByteString as B
import Data.Word (Word32, Word8)
import EmulatorUsb (Emulator, emulatorReadMemory)
import Numeric (showHex)

-- | `[0x0096cc64, 0x0096ce26)`, the extent the stock BACK settings restore copies.
newtype LiveSettings = LiveSettings B.ByteString

liveSettingsStart :: Word32
liveSettingsStart = 0x0096cc64

liveSettingsLength :: Int
liveSettingsLength = 450

liveSettings :: B.ByteString -> Either String LiveSettings
liveSettings bytes
  | B.length bytes == liveSettingsLength = Right (LiveSettings bytes)
  | otherwise = Left ("live-settings read returned " ++ show (B.length bytes) ++ " bytes, not " ++ show liveSettingsLength)

data LiveSettingChange = LiveSettingChange
  { changedSettingAddress :: Word32
  , changedSettingFrom :: Word8
  , changedSettingTo :: Word8
  } deriving (Eq, Show)

-- | The lowest byte that differs.
liveSettingChange :: LiveSettings -> LiveSettings -> Maybe LiveSettingChange
liveSettingChange (LiveSettings before) (LiveSettings after) =
  case [(offset, old, new) | (offset, old, new) <- zip3 [0 ..] (B.unpack before) (B.unpack after), old /= new] of
    (offset, old, new) : _ -> Just (LiveSettingChange (liveSettingsStart + offset) old new)
    [] -> Nothing

describeLiveSettingChange :: LiveSettingChange -> String
describeLiveSettingChange change = "live setting byte 0x" ++ pad 8 (showHex (changedSettingAddress change) "")
  ++ " changed from " ++ show (changedSettingFrom change) ++ " to " ++ show (changedSettingTo change)
  ++ "; ram-push and ram-unload run only at the idle shooting screen (transport/README.md)"
  where pad width digits = replicate (width - length digits) '0' ++ digits

readLiveSettings :: Emulator -> IO LiveSettings
readLiveSettings emulator = emulatorReadMemory emulator liveSettingsStart liveSettingsLength
  >>= either (ioError . userError) pure . liveSettings
