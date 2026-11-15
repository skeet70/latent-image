{-# LANGUAGE BangPatterns, OverloadedStrings #-}
module Emulation.MediaInspection
  ( SectorExtent, sectorExtent, extentByteOffset, extentByteCount
  , Inspection, inspect, inspectionMatches, inspectionValue
  ) where

import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word8, Word32)
import Numeric (showHex)

-- | One bounded read of the declared 4 GiB fixture.
data SectorExtent = SectorExtent Word32 Word32 deriving (Eq, Show)

sectorExtent :: Word32 -> Word32 -> Either String SectorExtent
sectorExtent first count
  | count == 0 || count > 512 = Left "sector inspection requires 1..512 sectors"
  | toInteger first + toInteger count > 8388608 = Left "sector inspection exceeds the 4 GiB fixture"
  | otherwise = Right (SectorExtent first count)

extentByteOffset :: SectorExtent -> Integer
extentByteOffset (SectorExtent first _) = toInteger first * 512

extentByteCount :: SectorExtent -> Int
extentByteCount (SectorExtent _ count) = fromIntegral count * 512

data Inspection = Inspection SectorExtent B.ByteString B.ByteString

inspect :: SectorExtent -> B.ByteString -> B.ByteString -> Either String Inspection
inspect extent original observed
  | B.length original /= extentByteCount extent = Left "original sector extent is truncated"
  | B.length observed /= extentByteCount extent = Left "observed sector extent is truncated"
  | otherwise = Right (Inspection extent original observed)

inspectionMatches :: Inspection -> Bool
inspectionMatches (Inspection _ original observed) = original == observed

inspectionValue :: Inspection -> Value
inspectionValue (Inspection extent@(SectorExtent first count) original observed) = object
  [ "first" .= first, "sectors" .= count, "bytes" .= extentByteCount extent
  , "original_sha256" .= sha original, "observed_sha256" .= sha observed
  , "exact_match" .= (differentBytes == 0)
  , "differing_bytes" .= differentBytes
  , "first_difference" .= case firstDifference of
      Nothing -> Nothing
      Just (offset, expected, actual) -> Just (object
        [ "extent_byte_offset" .= offset
        , "card_byte_offset" .= (extentByteOffset extent + toInteger offset)
        , "original" .= expected, "observed" .= actual ])
  ]
  where
    (differentBytes, firstDifference) = differences 0 0 Nothing
    differences :: Int -> Int -> Maybe (Int, Word8, Word8) -> (Int, Maybe (Int, Word8, Word8))
    differences !offset !total !firstMismatch
      | offset == B.length original = (total, firstMismatch)
      | expected == actual = differences (offset + 1) total firstMismatch
      | otherwise = differences (offset + 1) (total + 1) (case firstMismatch of
          Nothing -> Just (offset, expected, actual)
          Just _ -> firstMismatch)
      where
        expected = B.index original offset
        actual = B.index observed offset
    sha = concatMap byteHex . B.unpack . SHA256.hash
    byteHex byte = let text = showHex byte "" in if length text == 1 then '0' : text else text
