{-# LANGUAGE OverloadedStrings #-}

-- | The owned-slot region table, parsed from the crate's regions.json.
module Replacement.RtosOwnFirst.Regions
  ( Domain (..)
  , GuestAddress (..)
  , SlotOffset (..)
  , SymbolName (..)
  , FrameBytes (..)
  , Extent (..)
  , Region (..)
  , TailBody (..)
  , DomainTable (..)
  , RegionTable (..)
  , domains
  , domainLabel
  , domainLoaderId
  , domainLoaderBase
  , domainLoaderSize
  , domainTable
  , domainExtents
  , extentBytes
  , extentContains
  , parseRegionTable
  , hexText
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), withObject, withText, (.:))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex, showHex)

-- | The loader a body is linked into: A at 0x40000 (segment id 0), B at 0xd0000 (segment id 1).
data Domain = DomainA | DomainB deriving (Eq, Ord, Show, Enum, Bounded)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)

-- | A root slot's offset in the RTOS root table, as +0x60.
newtype SlotOffset = SlotOffset Word32 deriving (Eq, Ord, Show)

newtype SymbolName = SymbolName String deriving (Eq, Ord, Show)

-- | Deepest stack use below entry sp across the call tree.
newtype FrameBytes = FrameBytes Int deriving (Eq, Ord, Show)

-- | Half-open.
data Extent = Extent {extentStart :: GuestAddress, extentEnd :: GuestAddress} deriving (Eq, Ord, Show)

data Region = Region
  { regionSlot :: SlotOffset
  , regionEntry :: GuestAddress
  , regionExtent :: Extent
  , regionSymbol :: SymbolName
  , regionFrame :: FrameBytes
  }
  deriving (Eq, Show)

-- | The +0x1618 body, placed in the slack of the host slot's extent.
data TailBody = TailBody
  { bodyExtent :: Extent
  , bodySymbol :: SymbolName
  , bodyHost :: SlotOffset
  , bodyFrame :: FrameBytes
  }
  deriving (Eq, Show)

data DomainTable = DomainTable
  { tableBase :: GuestAddress
  , tableRegions :: [Region]
  , -- | The +0x1618 entry extent.
    tableTail :: Region
  , tableBody :: TailBody
  }
  deriving (Eq, Show)

data RegionTable = RegionTable
  { tableFill :: Word32
  , tableA :: DomainTable
  , tableB :: DomainTable
  }
  deriving (Eq, Show)

domains :: [Domain]
domains = [minBound .. maxBound]

domainLabel :: Domain -> String
domainLabel domain = case domain of
  DomainA -> "a"
  DomainB -> "b"

domainLoaderId :: Domain -> Word32
domainLoaderId domain = case domain of
  DomainA -> 0
  DomainB -> 1

domainLoaderBase :: Domain -> GuestAddress
domainLoaderBase domain = case domain of
  DomainA -> GuestAddress 0x40000
  DomainB -> GuestAddress 0xd0000

domainLoaderSize :: Domain -> Int
domainLoaderSize domain = case domain of
  DomainA -> 0xdae0
  DomainB -> 0xdaa0

domainTable :: RegionTable -> Domain -> DomainTable
domainTable table domain = case domain of
  DomainA -> tableA table
  DomainB -> tableB table

-- | The installed extents: the region rows, then the +0x1618 entry extent.
domainExtents :: DomainTable -> [Region]
domainExtents table = tableRegions table ++ [tableTail table]

extentBytes :: Extent -> Int
extentBytes (Extent (GuestAddress start) (GuestAddress end)) = fromIntegral end - fromIntegral start

extentContains :: Extent -> GuestAddress -> Bool
extentContains (Extent start end) candidate = start <= candidate && candidate < end

hexText :: Word32 -> String
hexText value = "0x" ++ showHex value ""

hexWord :: J.Value -> Parser Word32
hexWord = withText "hex word" $ \text -> case T.unpack text of
  '0' : 'x' : digits@(_ : _) | all (`elem` ("0123456789abcdef" :: String)) digits -> case readHex digits of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (fromInteger value)
    _ -> fail ("not a 32-bit word: " ++ T.unpack text)
  _ -> fail ("not a lowercase 0x hex word: " ++ T.unpack text)

address :: J.Object -> J.Key -> Parser GuestAddress
address object key = object .: key >>= fmap GuestAddress . hexWord

slot :: J.Object -> Parser SlotOffset
slot object = object .: "slot" >>= fmap SlotOffset . hexWord

frame :: J.Object -> Parser FrameBytes
frame object = do
  value <- object .: "frame_bytes"
  unless (value >= 0) (fail "negative frame_bytes")
  pure (FrameBytes value)

newtype ParsedRegion = ParsedRegion Region

instance FromJSON ParsedRegion where
  parseJSON = withObject "region" $ \object ->
    fmap ParsedRegion $
      Region
        <$> slot object
        <*> address object "entry"
        <*> (Extent <$> address object "start" <*> address object "end")
        <*> (SymbolName <$> object .: "symbol")
        <*> frame object

newtype ParsedDomain = ParsedDomain DomainTable

instance FromJSON ParsedDomain where
  parseJSON = withObject "domain" $ \object -> do
    base <- address object "loader_base"
    regions <- object .: "regions"
    tailObject <- object .: "tail"
    bodyObject <- tailObject .: "body"
    body <-
      TailBody
        <$> (Extent <$> address bodyObject "address" <*> address bodyObject "end")
        <*> (SymbolName <$> bodyObject .: "symbol")
        <*> (bodyObject .: "host_slot" >>= fmap SlotOffset . hexWord)
        <*> frame bodyObject
    entry <- address tailObject "address"
    tailRegion <-
      Region
        <$> slot tailObject
        <*> pure entry
        <*> (Extent entry <$> address tailObject "end")
        <*> (SymbolName <$> tailObject .: "symbol")
        <*> frame tailObject
    pure (ParsedDomain (DomainTable base [region | ParsedRegion region <- regions] tailRegion body))

instance FromJSON RegionTable where
  parseJSON = withObject "regions" $ \object -> do
    fill <- object .: "fill_word" >>= hexWord
    tables <- object .: "domains"
    ParsedDomain a <- tables .: "a"
    ParsedDomain b <- tables .: "b"
    pure (RegionTable fill a b)

parseRegionTable :: B.ByteString -> Either String RegionTable
parseRegionTable = J.eitherDecodeStrict
