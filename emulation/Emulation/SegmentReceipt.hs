{-# LANGUAGE OverloadedStrings #-}
-- | The declared segment list a segmented receipt carries: its typed names, elapsed seconds and results.
module Emulation.SegmentReceipt
  ( SegmentName
  , segmentName
  , segmentNameText
  , ElapsedSeconds
  , elapsedSeconds
  , elapsedSecondsValue
  , SegmentResult (..)
  , SegmentReceipt (..)
  , decodeDeclaredReceipts
  , decodeDeclaredSegments
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), eitherDecodeStrict', object, withText, (.:), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (withObject)
import qualified Data.ByteString as B
import Data.Char (isAlphaNum)
import Data.List (nub, sort)
import qualified Data.Text as T

newtype SegmentName = SegmentName T.Text deriving (Eq, Show)

segmentName :: T.Text -> Either String SegmentName
segmentName text
  | T.null text = Left "segment name is empty"
  | not (T.all (\c -> isAlphaNum c || c == '-' || c == '_') text) = Left "segment name is not a judgeable token"
  | otherwise = Right (SegmentName text)

segmentNameText :: SegmentName -> T.Text
segmentNameText (SegmentName text) = text

instance ToJSON SegmentName where
  toJSON = String . segmentNameText

instance FromJSON SegmentName where
  parseJSON = withText "segment name" (either fail pure . segmentName)

newtype ElapsedSeconds = ElapsedSeconds Double deriving (Eq, Show)

elapsedSeconds :: Double -> Either String ElapsedSeconds
elapsedSeconds value
  | isNaN value || isInfinite value = Left "elapsed seconds must be finite"
  | value < 0 = Left "elapsed seconds must not be negative"
  | otherwise = Right (ElapsedSeconds value)

elapsedSecondsValue :: ElapsedSeconds -> Double
elapsedSecondsValue (ElapsedSeconds value) = value

instance ToJSON ElapsedSeconds where
  toJSON = toJSON . elapsedSecondsValue

instance FromJSON ElapsedSeconds where
  parseJSON value = do
    seconds <- parseJSON value
    either fail pure (elapsedSeconds seconds)

data SegmentResult = SegmentPassed | SegmentFailed T.Text deriving (Eq, Show)

resultWord :: SegmentResult -> T.Text
resultWord SegmentPassed = "passed"
resultWord (SegmentFailed _) = "failed"

resultMessage :: SegmentResult -> Maybe T.Text
resultMessage SegmentPassed = Nothing
resultMessage (SegmentFailed text) = Just text

data SegmentReceipt = SegmentReceipt
  { segmentReceiptName :: SegmentName
  , segmentReceiptSeconds :: ElapsedSeconds
  , segmentReceiptResult :: SegmentResult
  } deriving (Eq, Show)

instance ToJSON SegmentReceipt where
  toJSON receipt = object
    [ "segment" .= segmentReceiptName receipt
    , "seconds" .= segmentReceiptSeconds receipt
    , "result" .= resultWord (segmentReceiptResult receipt)
    , "message" .= resultMessage (segmentReceiptResult receipt) ]

instance FromJSON SegmentReceipt where
  parseJSON = withObject "segment receipt" $ \value -> do
    unless (sort (KM.keys value) == sort (map K.fromString ["segment", "seconds", "result", "message"]))
      (fail "a segment receipt takes exactly segment, seconds, result and message")
    name <- value .: "segment"
    seconds <- value .: "seconds"
    word <- value .: "result"
    message <- value .: "message"
    result <- case (word, message) of
      ("passed", Nothing) -> pure SegmentPassed
      ("passed", Just _) -> fail "a passed segment carries no message"
      ("failed", Just text) -> pure (SegmentFailed text)
      ("failed", Nothing) -> fail "a failed segment carries a message"
      (other, _) -> fail ("unknown segment result: " ++ T.unpack other)
    pure (SegmentReceipt name seconds result)

decodeDeclaredReceipts :: B.ByteString -> Either String [SegmentReceipt]
decodeDeclaredReceipts bytes = do
  receipts <- eitherDecodeStrict' bytes
  let names = map segmentReceiptName receipts
  case names of
    [] -> Left "no segments are declared"
    _ | length names /= length (nub names) -> Left "a segment name is repeated"
      | otherwise -> Right receipts

decodeDeclaredSegments :: B.ByteString -> Either String [SegmentName]
decodeDeclaredSegments = fmap (map segmentReceiptName) . decodeDeclaredReceipts
