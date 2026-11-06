{-# LANGUAGE OverloadedStrings #-}
module Emulation.LocalUiMenu (LocalUiMenu, parseLocalUiMenu) where

import Control.Monad (unless)
import Data.Aeson (Value (..), ToJSON (..), object, (.=))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Numeric (readHex)

data LocalUiMenu = LocalUiMenu Word32 (Maybe Word32) (Maybe Word32) Word32 Word32 deriving (Eq, Show)

instance ToJSON LocalUiMenu where
  toJSON (LocalUiMenu node ident setting cursor level) = object
    ["node" .= node, "id" .= ident, "setting" .= setting, "cursor" .= cursor, "level" .= level]

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
  pure (LocalUiMenu node (at 0) (at 8) (word menu 8) (word depth 0))
 where
  word bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0..3]]
  number :: Value -> Either String Word32
  number (Number value) = maybe (Left "invalid local UI address") Right (toBoundedInteger value)
  number _ = Left "invalid local UI address"
  field key (Object fields) = maybe (Left "missing local UI menu field") Right (KM.lookup key fields)
  field _ _ = Left "invalid local UI menu object"
  array (Array values) = Right (V.toList values)
  array _ = Left "invalid local UI menu array"
  decodeBytes (String text) = B.pack <$> decode (T.unpack text)
  decodeBytes _ = Left "invalid local UI menu bytes"
  decode [] = Right []
  decode (a:b:rest) = case readHex [a,b] of
    [(value, "")] -> (value :) <$> decode rest
    _ -> Left "invalid local UI menu hex"
  decode _ = Left "invalid local UI menu hex"
  rawView :: Word32 -> Int -> Value -> Either String B.ByteString
  rawView address size value = do
    actual <- field "address" value >>= number
    raw <- field "physical" value >>= decodeBytes
    spellings <- field "spellings" value >>= array >>= mapM array
    unless (actual == address && B.length raw == size && map length spellings == [2,2]) (Left "local UI menu view range differs")
    aliases <- mapM decodeBytes (concat spellings)
    unless (all (== raw) aliases) (Left "local UI menu aliases differ")
    pure raw
