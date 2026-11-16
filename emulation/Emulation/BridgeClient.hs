{-# LANGUAGE OverloadedStrings #-}
module Emulation.BridgeClient
  ( Bridge (..), BridgeStopped (..), Fields
  , harness, require, num, textAt, objectAt, asObject, decodeHex
  , send, receive, call, command, event, readMemory, exchangeLine
  ) where

import Control.Exception (Exception, IOException, displayException, throwIO, try)
import Control.Monad (unless)
import Data.Aeson (FromJSON, Value (..), eitherDecodeStrict', encode, fromJSON, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32)
import qualified Emulation.CaptureStream as CS
import System.IO (Handle, hFlush)

-- | A failed bridge command or closed transport.
newtype BridgeStopped = BridgeStopped String deriving (Show)
instance Exception BridgeStopped

harness :: String -> IO a
harness = ioError . userError

require :: Bool -> String -> IO ()
require ok message = unless ok (harness message)

decodeHex :: T.Text -> Either String B.ByteString
decodeHex text
  | odd (B.length raw) = Left "odd hex length"
  | B.length decoded /= half = Left "invalid lowercase hex"
  | otherwise = Right decoded
  where
    raw = TE.encodeUtf8 text
    half = B.length raw `div` 2
    decoded = fst (B.unfoldrN half step 0)
    nibble w | w >= 48 && w <= 57 = Just (w - 48)
             | w >= 97 && w <= 102 = Just (w - 87)
             | otherwise = Nothing
    step i = do
      high <- nibble (B.index raw i)
      low <- nibble (B.index raw (i + 1))
      Just (high `shiftL` 4 .|. low, i + 2)

type Fields = KM.KeyMap Value

num :: FromJSON a => K.Key -> Fields -> Maybe a
num field fields = KM.lookup field fields >>= \value -> case fromJSON value of
  A.Success x -> Just x
  A.Error _ -> Nothing

textAt :: K.Key -> Fields -> Maybe T.Text
textAt field fields = case KM.lookup field fields of
  Just (String t) -> Just t
  _ -> Nothing

objectAt :: K.Key -> Fields -> Maybe Fields
objectAt field fields = case KM.lookup field fields of
  Just (Object o) -> Just o
  _ -> Nothing

asObject :: Value -> Either String Fields
asObject (Object o) = Right o
asObject _ = Left "bridge line is not an object"

-- Bridge client -------------------------------------------------------------------------------

data Bridge = Bridge
  { bridgeIn :: Handle
  , bridgeReader :: CS.BoundedReader
  , bridgeRequests :: Handle
  , bridgeStdout :: Handle
  }

send :: Bridge -> Value -> IO ()
send bridge value = writeLine bridge (encode value)

writeLine :: Bridge -> BL.ByteString -> IO ()
writeLine bridge encoded = do
  let line = BL.toStrict encoded <> "\n"
  B.hPut (bridgeRequests bridge) line >> hFlush (bridgeRequests bridge)
  B.hPut (bridgeIn bridge) line >> hFlush (bridgeIn bridge)

receive :: Bridge -> IO B.ByteString
receive bridge = do
  line <- CS.readBoundedLine (bridgeReader bridge)
  B.hPut (bridgeStdout bridge) (line <> "\n") >> hFlush (bridgeStdout bridge)
  pure line

exchangeLine :: Bridge -> BL.ByteString -> IO B.ByteString
exchangeLine bridge encoded = writeLine bridge encoded >> receive bridge

-- | One request and its one reply; ok:false or a closed pipe throws 'BridgeStopped'.
call :: Bridge -> Value -> IO Fields
call bridge value = do
  exchanged <- try (send bridge value >> receive bridge)
  line <- either (\e -> throwIO (BridgeStopped ("bridge pipe: " ++ displayException (e :: IOException)))) pure exchanged
  fields <- either (\e -> harness ("invalid bridge reply: " ++ e)) pure (eitherDecodeStrict' line >>= asObject)
  case KM.lookup "ok" fields of
    Just (Bool True) -> pure (KM.delete "ok" fields)
    Just (Bool False) -> throwIO (BridgeStopped (maybe "bridge error without message" T.unpack (textAt "message" fields)))
    _ -> harness "bridge reply lacks boolean ok"

command :: T.Text -> [(K.Key, Value)] -> Value
command name rest = Object (KM.fromList (("command", String name) : rest))

event :: Bridge -> T.Text -> IO Fields
event bridge name = do
  line <- receive bridge
  fields <- either (\e -> harness ("invalid bridge event: " ++ e)) pure (eitherDecodeStrict' line >>= asObject)
  require (KM.lookup "ok" fields == Just (Bool True) && textAt "event" fields == Just name)
    ("bridge did not send " ++ T.unpack name ++ ": " ++ C.unpack line)
  pure fields

readMemory :: Bridge -> Word32 -> Int -> IO B.ByteString
readMemory bridge address count = do
  require (count > 0 && count <= 4096 && toInteger address + toInteger count <= 0x100000000) "read-memory range exceeds page or address bound"
  reply <- call bridge (command "read-memory" ["address" .= address, "count" .= count])
  bytes <- either (harness . ("read-memory: " ++)) pure (maybe (Left "missing bytes") decodeHex (textAt "bytes" reply))
  require (B.length bytes == count) "read-memory returned a different length"
  pure bytes
