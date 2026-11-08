{-# LANGUAGE OverloadedStrings #-}
-- | One key press on one screen observed through instruction trace sites and a PC-recording
-- write watch, after a key path from the attach root.
module Emulation.MenuObserve
  ( TraceSite (..)
  , SiteRead (..)
  , MenuObserveConfig (..)
  , decodeMenuObserveConfig
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.!=), (.:), (.:?))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.List (sort)
import Data.Int (Int32)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.MenuCrawl (CrawlKey, GuestRange (..))
import Numeric (readHex)

-- | An instruction address whose entry the bridge records with r0-r3 and lr, its label (1..48
-- lowercase ASCII letters, digits or '-'), and the physical ranges each entry reads.
data TraceSite = TraceSite { traceSiteAddress :: Word32, traceSiteLabel :: String, traceSiteReads :: [SiteRead] } deriving (Eq, Show)

data SiteRead = SiteRead Word32 Int | StackRead Int32 Int deriving (Eq, Show)

hexWord :: T.Text -> Maybe Word32
hexWord text = case T.unpack text of
  '0' : 'x' : digits | not (null digits), length digits <= 8, [(value, "")] <- readHex digits -> Just (fromInteger value)
  _ -> Nothing

instance FromJSON SiteRead where
  parseJSON = withObject "trace site read" $ \fields -> do
    if sort (KM.keys fields) == ["bytes", "sp"] then do
      offset <- fields .: "sp"
      count <- fields .: "bytes"
      unless (count >= 1 && count <= 4096) (fail "a stack-relative read requires 1..4096 bytes")
      pure (StackRead offset count)
    else absolute fields
    where
      absolute fields = do
        unless (sort (KM.keys fields) == ["address", "length"]) (fail "a trace site read takes exactly address and length or sp and bytes")
        start <- fields .: "address" >>= maybe (fail "expected a 0x-prefixed hex address") pure . hexWord
        count <- fields .: "length"
        unless (count >= 1 && count <= 4096 && toInteger start + toInteger count <= 0x100000000)
          (fail "a trace site read is 1..4096 bytes inside the 32-bit space")
        pure (SiteRead start count)

instance FromJSON TraceSite where
  parseJSON = withObject "trace site" $ \fields -> do
    unless (sort (KM.keys fields) `elem` [["address", "label"], ["address", "label", "reads"]])
      (fail "a trace site takes address, label and optionally reads")
    address <- fields .: "address"
    label <- fields .: "label"
    unless (not (null label) && length label <= 48 && all (\c -> isAsciiLower c || isDigit c || c == '-') label)
      (fail "a trace site label is 1..48 lowercase ASCII letters, digits or '-'")
    entryReads <- fields .:? "reads" .!= []
    unless (length entryReads <= 4) (fail "a trace site reads at most four ranges")
    case hexWord address of
      Just value -> pure (TraceSite value label entryReads)
      Nothing -> fail "expected a 0x-prefixed hex address"

-- | The key path from the root to the screen, the observed key, its trace sites, the guest ranges
-- whose writes are recorded with their PC, and the watch record limit.
data MenuObserveConfig = MenuObserveConfig
  { observePath :: [CrawlKey]
  , observeKey :: CrawlKey
  , observeSites :: [TraceSite]
  , observeWrites :: [GuestRange]
  , observeLimit :: Int
  } deriving (Eq, Show)

instance FromJSON MenuObserveConfig where
  parseJSON = withObject "menu observe config" $ \document -> do
    unless (sort (KM.keys document) == ["key", "limit", "path", "schema", "trace_sites", "write_ranges"])
      (fail "menu observe config takes exactly schema, path, key, trace_sites, write_ranges and limit")
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported menu observe config schema")
    config <- MenuObserveConfig <$> document .: "path" <*> document .: "key" <*> document .: "trace_sites"
      <*> document .: "write_ranges" <*> document .: "limit"
    let sites = observeSites config
        addresses = sort (map traceSiteAddress sites)
        labels = sort (map traceSiteLabel sites)
        ranges = sort [(rangeStart range, rangeEnd range) | range <- observeWrites config]
        distinct values = and (zipWith (/=) values (drop 1 values))
    unless (length sites <= 32 && distinct addresses && distinct labels) (fail "trace sites must be at most 32 with distinct addresses and labels")
    unless (not (null ranges) && length ranges <= 8 && and (zipWith (\(_, end) (start, _) -> end < start) ranges (drop 1 ranges)))
      (fail "write ranges must be 1..8, disjoint and not adjacent")
    unless (observeLimit config >= 1 && observeLimit config <= 1048576) (fail "limit must be 1..1048576")
    pure config

decodeMenuObserveConfig :: B.ByteString -> Either String MenuObserveConfig
decodeMenuObserveConfig = eitherDecodeStrict'
