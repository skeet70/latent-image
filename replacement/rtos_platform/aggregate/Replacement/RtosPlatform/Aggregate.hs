{-# LANGUAGE OverloadedStrings #-}

-- | Reading a linked aggregate image and installing its relocated symbols over a stock base.
module Replacement.RtosPlatform.Aggregate
  ( AggregateImage (..)
  , AggregateSymbol (..)
  , SectionPlacement (..)
  , parseAggregateSymbols
  , parsePlacements
  , installPlacements
  , aggregateImage
  , symbolsWithPrefixes
  , sliceAggregate
  , installAggregate
  , installSymbol
  ) where

import Control.Monad (foldM)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Text as T
import Data.List (isPrefixOf, sortOn)
import Data.Maybe (catMaybes)
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

data SectionPlacement = SectionPlacement
  { placementAddress :: Word32
  , placementBytes :: Word32
  , placementSection :: String
  }
  deriving (Eq, Show)

instance FromJSON SectionPlacement where
  parseJSON = withObject "placement" $ \o ->
    SectionPlacement
      <$> (o .: "address" >>= either fail pure . hexPlacement)
      <*> o .: "bytes"
      <*> o .: "section"

newtype ParsedPlacements = ParsedPlacements [SectionPlacement]
instance FromJSON ParsedPlacements where
  parseJSON = withObject "placements" $ \o -> ParsedPlacements <$> o .: "placements"

parsePlacements :: B.ByteString -> Either String [SectionPlacement]
parsePlacements bytes = do
  ParsedPlacements placements <- eitherDecodeStrict' bytes
  pure placements

hexPlacement :: T.Text -> Either String Word32
hexPlacement text =
  let raw = T.unpack text
      digits = case raw of
        '0' : 'x' : rest -> rest
        '0' : 'X' : rest -> rest
        _ -> raw
   in case readHex digits of
        [(value, "")] | value <= toInteger (maxBound :: Word32) -> Right (fromInteger value)
        _ -> Left ("placement field is not a 32-bit hex word: " ++ raw)

installPlacements :: Word32 -> B.ByteString -> AggregateImage -> [SectionPlacement] -> Either String B.ByteString
installPlacements base current image = foldM install current
  where
    install bytes placement = do
      section <- sliceAggregate image (placementAddress placement) (placementBytes placement)
      installAt base bytes (placementAddress placement) section

parseAggregateSymbols :: B.ByteString -> Either String [AggregateSymbol]
parseAggregateSymbols bytes = catMaybes <$> mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line = case C.words line of
      [address, size, kind, name] | kind `elem` ["T", "t"] ->
        Just <$> (AggregateSymbol (C.unpack name) <$> hexWord address <*> hexWord size)
      [_, _, _, _] -> Right Nothing
      _ -> Left ("aggregate symbol row is not ADDR SIZE KIND NAME: " ++ C.unpack line)
    hexWord text = case readHex (C.unpack text) of
      [(value, "")] | value <= toInteger (maxBound :: Word32) -> Right (fromInteger value)
      _ -> Left ("aggregate symbol field is not a 32-bit hex word: " ++ C.unpack text)

-- | The image starts at the lowest symbol address and ends at the highest symbol end.
aggregateImage :: [AggregateSymbol] -> B.ByteString -> Either String AggregateImage
aggregateImage symbols bytes = case sortOn symbolAddress symbols of
  [] -> Left "aggregate image has no code symbols"
  first : rest ->
    let rows = first : rest
        base = symbolAddress first
        top = maximum [symbolAddress row + symbolSize row | row <- rows]
     in if top - base /= fromIntegral (B.length bytes)
          then Left ("aggregate image spans " ++ show (B.length bytes) ++ " bytes, symbols span " ++ show (top - base))
          else Right (AggregateImage base bytes)

symbolsWithPrefixes :: [String] -> [AggregateSymbol] -> [AggregateSymbol]
symbolsWithPrefixes prefixes = filter (\row -> any (`isPrefixOf` symbolName row) prefixes)

sliceAggregate :: AggregateImage -> Word32 -> Word32 -> Either String B.ByteString
sliceAggregate image address size
  | address < imageBase image = Left "aggregate slice starts below its image"
  | toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image)) = Left ("aggregate slice ends above its image: " ++ show (address, size, imageBase image, B.length (imageBytes image)))
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
  | toInteger address + toInteger (B.length bytes) > toInteger base + toInteger (B.length current) = Left "aggregate install ends above the image"
  | otherwise =
      let offset = fromIntegral (address - base)
       in Right (B.take offset current <> bytes <> B.drop (offset + B.length bytes) current)
