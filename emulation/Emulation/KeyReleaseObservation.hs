{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE GeneralizedNewtypeDeriving #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.KeyReleaseObservation
  ( KeyReleaseObservation
  , ReleaseSegment
  , releaseSegment
  , readReleaseObservationLog
  , readReleaseObservationReceipt
  , writeReleaseObservationReceipt
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (String), eitherDecodeStrict', encode, object, withObject, withText, (.:), (.=))
import Data.Aeson.Types (parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Data.Word (Word64)
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.FilePath (takeDirectory)

newtype ReleaseRoute = ReleaseRoute T.Text
  deriving newtype (ToJSON)

newtype ReleaseSegment = ReleaseSegment T.Text
  deriving newtype (ToJSON)

releaseSegment :: T.Text -> Either String ReleaseSegment
releaseSegment name
  | T.null name = Left "A release segment must not be empty"
  | otherwise = Right (ReleaseSegment name)

instance FromJSON ReleaseSegment where
  parseJSON = withText "release segment" (either fail pure . releaseSegment)

instance FromJSON ReleaseRoute where
  parseJSON = withText "release route" $ \name ->
    if T.null name then fail "A release route must not be empty" else pure (ReleaseRoute name)

newtype InstructionCount = InstructionCount Word64
  deriving newtype (FromJSON, ToJSON)

newtype ReleaseCap = ReleaseCap Word64
  deriving newtype (FromJSON, ToJSON)

newtype KeyRow = KeyRow Word64
  deriving newtype (ToJSON)

instance FromJSON KeyRow where
  parseJSON value = do
    row <- parseJSON value
    if row < 36 then pure (KeyRow row) else fail "A key row must be within the firmware table"

data ReleaseKey = Up | Down | LeftKey | RightKey | Ok | Q | Back | Play | Fn | DialPush | HalfPress | FullPress | Movie | Power | DialLeft | DialRight | MainDialUp | MainDialDown
  deriving (Eq)

keyNames :: [(T.Text, ReleaseKey)]
keyNames =
  [ ("up", Up), ("down", Down), ("left", LeftKey), ("right", RightKey), ("ok", Ok)
  , ("q", Q), ("back", Back), ("play", Play), ("fn", Fn), ("dial-push", DialPush)
  , ("half-press", HalfPress), ("dial-left", DialLeft), ("dial-right", DialRight)
  , ("full-press", FullPress), ("movie", Movie), ("power", Power)
  , ("main-dial-up", MainDialUp), ("main-dial-down", MainDialDown)
  ]

instance FromJSON ReleaseKey where
  parseJSON = withText "release key" $ \name ->
    maybe (fail "Unknown release key") pure (lookup name keyNames)

instance ToJSON ReleaseKey where
  toJSON key = String $ case key of
    Up -> "up"
    Down -> "down"
    LeftKey -> "left"
    RightKey -> "right"
    Ok -> "ok"
    Q -> "q"
    Back -> "back"
    Play -> "play"
    Fn -> "fn"
    DialPush -> "dial-push"
    HalfPress -> "half-press"
    DialLeft -> "dial-left"
    DialRight -> "dial-right"
    FullPress -> "full-press"
    Movie -> "movie"
    Power -> "power"
    MainDialUp -> "main-dial-up"
    MainDialDown -> "main-dial-down"

data KeyReleaseObservation = KeyReleaseObservation
  { observedRoute :: Maybe ReleaseRoute
  , observedSegment :: Maybe ReleaseSegment
  , observedKey :: ReleaseKey
  , observedRow :: Maybe KeyRow
  , observedCap :: ReleaseCap
  , observedUsed :: InstructionCount
  , observedInstruction :: InstructionCount
  }

instance FromJSON KeyReleaseObservation where
  parseJSON = withObject "incomplete key release" $ \value -> do
    key <- value .: "key"
    row <- value .: "row"
    unless ((key `elem` [DialLeft, DialRight, MainDialUp, MainDialDown]) == maybe True (const False) row)
      (fail "Only rotary inputs have no key-table row")
    KeyReleaseObservation <$> value .: "route" <*> value .: "segment" <*> pure key <*> pure row
      <*> value .: "cap" <*> value .: "instructions_used" <*> value .: "instructions"

instance ToJSON KeyReleaseObservation where
  toJSON observation = object
    [ "route" .= observedRoute observation, "segment" .= observedSegment observation
    , "key" .= observedKey observation, "row" .= observedRow observation
    , "cap" .= observedCap observation, "instructions_used" .= observedUsed observation
    , "instructions" .= observedInstruction observation
    ]

readReleaseObservationLog :: FilePath -> IO [KeyReleaseObservation]
readReleaseObservationLog path = do
  present <- doesFileExist path
  if not present then pure [] else do
    rows <- filter (not . B.null) . C.lines <$> B.readFile path
    mapM decode rows
  where
    decode bytes = either (ioError . userError . ((path ++ ": ") ++)) pure (eitherDecodeStrict' bytes)

readReleaseObservationReceipt :: FilePath -> IO [KeyReleaseObservation]
readReleaseObservationReceipt path = do
  present <- doesFileExist path
  if not present then pure [] else do
    bytes <- B.readFile path
    either (ioError . userError . ((path ++ ": ") ++)) pure
      (eitherDecodeStrict' bytes >>= parseEither (withObject "key release receipt" (.: "incomplete_releases")))

writeReleaseObservationReceipt :: FilePath -> [KeyReleaseObservation] -> IO ()
writeReleaseObservationReceipt path observations = do
  createDirectoryIfMissing True (takeDirectory path)
  BL.writeFile path (encode (object
    [ "schema" .= (1 :: Int)
    , "scope" .= ("incomplete debounced key releases; observational, not an acceptance gate" :: T.Text)
    , "incomplete_releases" .= observations
    ]) <> "\n")
