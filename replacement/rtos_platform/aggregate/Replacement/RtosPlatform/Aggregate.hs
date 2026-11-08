{-# LANGUAGE OverloadedStrings #-}

-- | Reading a linked aggregate image and installing its relocated symbols over a stock base.
module Replacement.RtosPlatform.Aggregate
  ( AggregateImage (..)
  , AggregateSymbol (..)
  , parseAggregateSymbols
  , aggregateImage
  , symbolsWithPrefixes
  , sliceAggregate
  , installAggregate
  , installSymbol
  ) where

import Control.Monad (foldM)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isPrefixOf, sortOn)
import Data.Word (Word32)
import Numeric (readHex)

data AggregateSymbol = AggregateSymbol
  { symbolName :: String
  , symbolAddress :: Word32
  , symbolSize :: Word32
  }
  deriving (Eq, Show)

-- | The flat image from its lowest linked symbol.
data AggregateImage = AggregateImage
  { imageBase :: Word32
  , imageBytes :: B.ByteString
  }
  deriving (Eq, Show)

parseAggregateSymbols :: B.ByteString -> Either String [AggregateSymbol]
parseAggregateSymbols bytes = mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line = case C.words line of
      [address, size, kind, name] | kind `elem` ["T", "t"] ->
        AggregateSymbol (C.unpack name) <$> hexWord address <*> hexWord size
      _ -> Left ("aggregate symbol row is not ADDR SIZE T NAME: " ++ C.unpack line)
    hexWord text = case readHex (C.unpack text) of
      [(value, "")] | value <= toInteger (maxBound :: Word32) -> Right (fromInteger value)
      _ -> Left ("aggregate symbol field is not a 32-bit hex word: " ++ C.unpack text)

-- | The image starts at the lowest symbol address and ends at the highest symbol end.
aggregateImage :: [AggregateSymbol] -> B.ByteString -> Either String AggregateImage
aggregateImage symbols bytes = case sortOn symbolAddress symbols of
  [] -> Left "aggregate image has no code symbols"
  rows ->
    let base = symbolAddress (head rows)
        top = maximum [symbolAddress row + symbolSize row | row <- rows]
     in if top - base /= fromIntegral (B.length bytes)
          then Left ("aggregate image spans " ++ show (B.length bytes) ++ " bytes, symbols span " ++ show (top - base))
          else Right (AggregateImage base bytes)

symbolsWithPrefixes :: [String] -> [AggregateSymbol] -> [AggregateSymbol]
symbolsWithPrefixes prefixes = filter (\row -> any (`isPrefixOf` symbolName row) prefixes)

sliceAggregate :: AggregateImage -> Word32 -> Word32 -> Either String B.ByteString
sliceAggregate image address size
  | address < imageBase image = Left "aggregate slice starts below its image"
  | address + size > imageBase image + fromIntegral (B.length (imageBytes image)) = Left "aggregate slice ends above its image"
  | otherwise =
      Right (B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image)))

-- | Writes one symbol's bytes at its linked address inside an image based at @base@.
installSymbol :: Word32 -> B.ByteString -> AggregateImage -> AggregateSymbol -> Either String B.ByteString
installSymbol base current image symbol = do
  bytes <- sliceAggregate image (symbolAddress symbol) (symbolSize symbol)
  installAt base current (symbolAddress symbol) bytes

installAggregate :: Word32 -> B.ByteString -> AggregateImage -> [AggregateSymbol] -> Either String B.ByteString
installAggregate base current image = foldM install current
  where
    install bytes symbol = installSymbol base bytes image symbol

installAt :: Word32 -> B.ByteString -> Word32 -> B.ByteString -> Either String B.ByteString
installAt base current address bytes
  | address < base = Left "aggregate install starts below the image"
  | address + fromIntegral (B.length bytes) > base + fromIntegral (B.length current) = Left "aggregate install ends above the image"
  | otherwise =
      let offset = fromIntegral (address - base)
       in Right (B.take offset current <> bytes <> B.drop (offset + B.length bytes) current)
