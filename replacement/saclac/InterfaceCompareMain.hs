{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson
import qualified Data.Aeson.Types as J
import qualified Data.ByteString.Lazy as L
import Data.List (findIndex)
import qualified Data.Text as Text
import Data.Word (Word8, Word16, Word32)
import qualified FujiAF.Host as H
import System.Environment (getArgs)
import System.Exit (exitFailure)
import Text.Read (readMaybe)

newtype Address = Address Word32 deriving (Eq, Show)
instance FromJSON Address where
  parseJSON = withText "guest address" $ \text ->
    maybe (fail "invalid guest address") (pure . Address) (readMaybe (Text.unpack text))
instance ToJSON Address where toJSON (Address address) = toJSON address

data Event
  = QueueIn Word16 Word8 [Word8]
  | PeerRequest Word8 Word16 [Word8]
  | Callback Address
  | ClientReply Address Address Word32
  | Store Address [Word8]
  deriving (Eq, Show)
instance FromJSON Event where
  parseJSON = withObject "interface event" $ \value -> do
    kind <- value .: "kind" :: J.Parser String
    case kind of
      "queue-in" -> QueueIn <$> value .: "operation" <*> value .: "sender" <*> value .: "payload"
      "peer-request" -> PeerRequest <$> value .: "queue" <*> value .: "operation" <*> value .: "payload"
      "callback" -> Callback <$> value .: "entry"
      "client-reply" -> ClientReply <$> value .: "entry" <*> value .: "target" <*> value .: "status"
      "store" -> Store <$> value .: "address" <*> value .: "bytes"
      _ -> fail "unknown interface event"
instance ToJSON Event where
  toJSON event = case event of
    QueueIn operation sender payload -> object ["kind" .= ("queue-in" :: String), "operation" .= operation, "sender" .= sender, "payload" .= payload]
    PeerRequest queue operation payload -> object ["kind" .= ("peer-request" :: String), "queue" .= queue, "operation" .= operation, "payload" .= payload]
    Callback entry -> object ["kind" .= ("callback" :: String), "entry" .= entry]
    ClientReply entry target status -> object ["kind" .= ("client-reply" :: String), "entry" .= entry, "target" .= target, "status" .= status]
    Store address bytes -> object ["kind" .= ("store" :: String), "address" .= address, "bytes" .= bytes]

newtype Observed = Observed Event
instance FromJSON Observed where
  parseJSON = withObject "observed event" $ \value -> Observed <$> value .: "event"

data Receipt = Receipt
  { scenario :: String
  , events :: [Event]
  , stockFetches :: Integer
  , observationError :: Maybe String
  , machineFailure :: Maybe String
  }
instance FromJSON Receipt where
  parseJSON = withObject "census receipt" $ \value -> do
    name <- value .: "scenario"
    rows <- (withObject "interface" (.: "events") =<< value .: "interface") :: J.Parser [Observed]
    ranges <- value .: "action_ranges" :: J.Parser [Value]
    fetches <- traverse (withObject "action range" (.: "fetches")) ranges
    Receipt name (concatMap (normalize . (\(Observed event) -> event)) rows) (sum fetches)
      <$> value .:? "error" <*> value .:? "machine_failure"

normalize :: Event -> [Event]
normalize (Store (Address address) bytes) = zipWith (\offset byte -> Store (Address (address + offset)) [byte]) [0..] bytes
normalize event = [event]

load :: FilePath -> IO Receipt
load path = do
  bytes <- H.readRegular (16 * 1024 * 1024) path
  either H.failH pure (eitherDecodeStrict bytes)

healthy :: Receipt -> Bool
healthy receipt = observationError receipt == Nothing && machineFailure receipt == Nothing
  && not (null (events receipt)) && stockFetches receipt > 0

firstDifference :: [Event] -> [Event] -> Maybe Int
firstDifference left right = case findIndex not (zipWith (==) left right) of
  Just index -> Just index
  Nothing | length left /= length right -> Just (min (length left) (length right))
  Nothing -> Nothing

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [stockPath, candidatePath, controlPath, output] -> do
      stock <- load stockPath
      candidate <- load candidatePath
      control <- load controlPath
      let difference = firstDifference (events stock) (events candidate)
          controlDifference = firstDifference (events stock) (events control)
          sameScenario = scenario stock == scenario candidate && scenario stock == scenario control
          passed = sameScenario && all healthy [stock, candidate, control]
            && difference == Nothing && controlDifference /= Nothing
          at receipt index = take 1 (drop index (events receipt))
      L.writeFile output (encode (object
        [ "schema" .= (1 :: Int)
        , "verdict" .= (if passed then "equal" else "different" :: String)
        , "scenario" .= scenario stock
        , "same_scenario" .= sameScenario
        , "stock_executed_instructions_in_action_extents" .= stockFetches stock
        , "stock_events" .= length (events stock)
        , "candidate_events" .= length (events candidate)
        , "control_events" .= length (events control)
        , "complete" .= all healthy [stock, candidate, control]
        , "first_difference" .= difference
        , "stock_at_difference" .= maybe [] (at stock) difference
        , "candidate_at_difference" .= maybe [] (at candidate) difference
        , "wrong_variant_detected" .= (controlDifference /= Nothing)
        , "control_first_difference" .= controlDifference
        , "normalization" .= ("instruction clocks and cores omitted; each store expanded into ascending-address bytes without reordering events" :: String)
        ]))
      unless passed exitFailure
    _ -> H.failH "usage: saclac-interface-compare STOCK_JSON CANDIDATE_JSON WRONG_JSON OUTPUT_JSON"
