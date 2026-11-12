{-# LANGUAGE OverloadedStrings #-}
-- The media-stream model's reply. The model's capacity refusal is a typed failure, so a reply
-- that refuses for capacity stays a refusal instead of invalidating the whole reply.
module FirstStageMediaReply
  ( ByteCount (..)
  , FailureKind (..)
  , ModelReply (..)
  , ModelStatus (..)
  , parseModelReply
  ) where

import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, withText, (.:), (.:?))
import qualified Data.ByteString as B

newtype ByteCount = ByteCount Int deriving (Eq, Show)

data ModelStatus = Decoded | Refused deriving (Eq, Show)

data FailureKind
  = IoFailure
  | InputBounds
  | HeaderFailure
  | ZeroToken
  | InvalidMatch
  | OutputLimit
  | OutputCapacity
  | Truncated
  | OutputLength
  deriving (Eq, Show)

data ModelReply = ModelReply
  { replyStatus :: ModelStatus
  , replyRecordBytes :: ByteCount
  , replyDecodedBytes :: Maybe ByteCount
  , replyFailure :: Maybe FailureKind
  }
  deriving (Eq, Show)

instance FromJSON ByteCount where
  parseJSON value = do
    bytes <- parseJSON value
    if bytes >= 0 && bytes <= 16 * 1024 * 1024 then pure (ByteCount bytes) else fail "invalid byte count"

instance FromJSON ModelStatus where
  parseJSON = withText "model status" $ \value -> case value of
    "decoded" -> pure Decoded
    "refused" -> pure Refused
    _ -> fail "unknown model status"

instance FromJSON FailureKind where
  parseJSON = withText "model failure" $ \value -> case value of
    "io" -> pure IoFailure
    "input-bounds" -> pure InputBounds
    "header" -> pure HeaderFailure
    "zero-token" -> pure ZeroToken
    "invalid-match" -> pure InvalidMatch
    "output-limit" -> pure OutputLimit
    "output-capacity" -> pure OutputCapacity
    "truncated" -> pure Truncated
    "output-length" -> pure OutputLength
    _ -> fail "unknown model failure"

instance FromJSON ModelReply where
  parseJSON = withObject "model reply" $ \value ->
    ModelReply
      <$> value .: "status"
      <*> value .: "record_bytes"
      <*> value .:? "decoded_length"
      <*> value .:? "failure_code"

parseModelReply :: B.ByteString -> Either String ModelReply
parseModelReply = eitherDecodeStrict'
