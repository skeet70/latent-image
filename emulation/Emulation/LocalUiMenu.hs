{-# LANGUAGE OverloadedStrings #-}
module Emulation.LocalUiMenu
  ( LocalUiMenu (..), MenuNodeAddress (..), MenuIdentifier (..), SettingIdentifier (..), MenuCursor (..), MenuDepth (..)
  , parseLocalUiMenu, LocalUiMode, parseLocalUiMode
  ) where

import Control.Monad (unless)
import Data.Aeson (Value (..), ToJSON (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import Numeric (readHex)

newtype MenuNodeAddress = MenuNodeAddress Word32 deriving (Eq, Show)
newtype MenuIdentifier = MenuIdentifier Word32 deriving (Eq, Show)
newtype SettingIdentifier = SettingIdentifier Word32 deriving (Eq, Show)
newtype MenuCursor = MenuCursor Word32 deriving (Eq, Show)
newtype MenuDepth = MenuDepth Word32 deriving (Eq, Show)
data LocalUiMenu = LocalUiMenu MenuNodeAddress (Maybe MenuIdentifier) (Maybe SettingIdentifier) MenuCursor MenuDepth deriving (Eq, Show)

instance ToJSON LocalUiMenu where
  toJSON (LocalUiMenu (MenuNodeAddress node) ident setting (MenuCursor cursor) (MenuDepth level)) = object
    ["node" .= node, "id" .= fmap (\(MenuIdentifier value) -> value) ident, "setting" .= fmap (\(SettingIdentifier value) -> value) setting, "cursor" .= cursor, "level" .= level]

newtype Mode = Mode Word32 deriving (Eq, Show)
newtype ModeRequest = ModeRequest Word32 deriving (Eq, Show)
newtype TransitionLatch = TransitionLatch Word8 deriving (Eq, Show)
data LocalUiMode = LocalUiMode Mode Mode Mode ModeRequest TransitionLatch deriving (Eq, Show)

instance ToJSON LocalUiMode where
  toJSON (LocalUiMode (Mode current) (Mode previous) (Mode pending) (ModeRequest request) (TransitionLatch latch)) = object
    ["current" .= current, "previous" .= previous, "pending" .= pending, "request" .= request, "transition_latch" .= latch]

parseLocalUiMode :: Value -> Either String LocalUiMode
parseLocalUiMode view = do
  bytes <- rawView 0x9500ec 40 view
  pure (LocalUiMode (Mode (word bytes 0)) (Mode (word bytes 4)) (Mode (word bytes 12))
    (ModeRequest (word bytes 20)) (TransitionLatch (B.index bytes 36)))

parseLocalUiMenu :: Value -> Either String LocalUiMenu
parseLocalUiMenu ui = do
  menu <- field "menu" ui >>= rawView 0x9571d0 12
  depth <- field "depth" ui >>= rawView 0x9575f4 4
  let node = word menu 0
      physical address = address .&. 0xbfffffff
  nodeView <- field "node" ui
  nodeBytes <- case nodeView of
    Null -> Right Nothing
    _ -> do
      actual <- field "address" nodeView >>= number
      unless (node /= 0 && physical actual == physical (node .&. 0xfffffffc)) (Left "local UI node address differs")
      Just <$> rawView actual 12 nodeView
  let at offset = do
        bytes <- nodeBytes
        let index = fromIntegral (node .&. 3) + offset
        if index `mod` 4 == 0 && index + 4 <= B.length bytes then Just (word bytes index) else Nothing
  pure (LocalUiMenu (MenuNodeAddress node) (MenuIdentifier <$> at 0) (SettingIdentifier <$> at 8) (MenuCursor (word menu 8)) (MenuDepth (word depth 0)))

word :: B.ByteString -> Int -> Word32
word bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0..3]]

number :: Value -> Either String Word32
number (Number value) = maybe (Left "invalid local UI address") Right (toBoundedInteger value)
number _ = Left "invalid local UI address"

field :: K.Key -> Value -> Either String Value
field key (Object fields) = maybe (Left "missing local UI view field") Right (KM.lookup key fields)
field _ _ = Left "invalid local UI view object"

array :: Value -> Either String [Value]
array (Array values) = Right (V.toList values)
array _ = Left "invalid local UI view array"

decodeBytes :: Value -> Either String B.ByteString
decodeBytes (String text) = B.pack <$> decode (T.unpack text)
 where
  decode [] = Right []
  decode (a:b:rest) = case readHex [a,b] of
    [(value, "")] -> (value :) <$> decode rest
    _ -> Left "invalid local UI view hex"
  decode _ = Left "invalid local UI view hex"
decodeBytes _ = Left "invalid local UI view bytes"

rawView :: Word32 -> Int -> Value -> Either String B.ByteString
rawView address size value = do
  actual <- field "address" value >>= number
  raw <- field "physical" value >>= decodeBytes
  spellings <- field "spellings" value >>= array >>= mapM array
  unless (actual == address && B.length raw == size && map length spellings == [2,2]) (Left "local UI view range differs")
  aliases <- mapM decodeBytes (concat spellings)
  unless (all (== raw) aliases) (Left "local UI view aliases differ")
  pure raw
