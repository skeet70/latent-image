{-# LANGUAGE OverloadedStrings #-}

-- | The declared retired-carrier union and its emitted demand.
module Replacement.RtosPlatform.Union
  ( Group (..)
  , groupLabel
  , Entry (..)
  , Carrier (..)
  , External (..)
  , ExternalKind (..)
  , Demand (..)
  , DomainMap (..)
  , Union (..)
  , UnionError (..)
  , FitError (..)
  , parseUnion
  , domainMap
  , runs
  , fit
  , totalRunBytes
  , totalDemandBytes
  ) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (FromJSON (..), withObject, withText, (.:))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.Text as T
import Data.List (sortOn)
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), domains, extentBytes, extentContains)

data Group = OwnFirst | S1 | S2 | S3a | S3b | S3c | S3d | Semaphore | FixedPool | TaskRecords | DataQueue | GlobalLock | Shared
  deriving (Eq, Ord, Show, Enum, Bounded)

groupLabel :: Group -> String
groupLabel value = case value of
  OwnFirst -> "own-first"
  S1 -> "s1"
  S2 -> "s2"
  S3a -> "s3a"
  S3b -> "s3b"
  S3c -> "s3c"
  S3d -> "s3d"
  Semaphore -> "semaphore"
  FixedPool -> "fixed-pool"
  TaskRecords -> "task-records"
  DataQueue -> "data-queue"
  GlobalLock -> "global-lock"
  Shared -> "shared"

data Entry = Entry
  { entryGroup :: Group
  , entryName :: String
  , entryAddress :: GuestAddress
  , entrySymbol :: String
  , entryVeneer :: String
  }
  deriving (Eq, Show)

data Carrier = Carrier {carrierGroup :: Group, carrierExtent :: Extent} deriving (Eq, Show)

data ExternalKind = OwnedEntry | RetainedService | ScratchEntry deriving (Eq, Show)

data External = External {externalName :: String, externalAddress :: GuestAddress, externalKind :: ExternalKind}
  deriving (Eq, Show)

data Demand = Demand {demandGroup :: Group, demandSymbol :: String, demandBytes :: Int} deriving (Eq, Show)

data DomainMap = DomainMap
  { mapDomain :: Domain
  , mapBase :: GuestAddress
  , mapSize :: Int
  , mapEntries :: [Entry]
  , mapCarriers :: [Carrier]
  , mapExternals :: [External]
  , mapDemand :: [Demand]
  }
  deriving (Eq, Show)

data Union = Union {unionFill :: Word32, unionA :: DomainMap, unionB :: DomainMap} deriving (Eq, Show)

data UnionError
  = UnionUnparsed String
  | FillWordDiffers Word32
  | DomainBaseDiffers Domain GuestAddress
  | CarrierNotAligned Domain Extent
  | CarriersOverlap Domain Extent Extent
  | EntryOutsideCarrier Domain Entry
  | EntryRepeated Domain GuestAddress
  | ExternalOutsideLoader Domain External
  deriving (Eq, Show)

data FitError
  = CarrierDeficit Domain Group String Int Int
  | TotalDeficit Domain Int Int
  deriving (Eq, Show)

hexWord :: J.Value -> Parser Word32
hexWord = withText "hex word" $ \text -> case T.unpack text of
  '0' : 'x' : digits@(_ : _) | all (`elem` ("0123456789abcdef" :: String)) digits -> case readHex digits of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (fromInteger value)
    _ -> fail ("not a 32-bit word: " ++ T.unpack text)
  _ -> fail ("not a lowercase 0x hex word: " ++ T.unpack text)

address :: J.Object -> J.Key -> Parser GuestAddress
address object key = object .: key >>= fmap GuestAddress . hexWord

group :: J.Value -> Parser Group
group = withText "group" $ \text -> case [g | g <- [minBound .. maxBound], groupLabel g == T.unpack text] of
  [g] -> pure g
  _ -> fail "unknown group"

newtype ParsedEntry = ParsedEntry Entry
instance FromJSON ParsedEntry where
  parseJSON = withObject "entry" $ \o ->
    fmap ParsedEntry $
      Entry <$> (o .: "group" >>= group)
        <*> o .: "name"
        <*> address o "address"
        <*> o .: "symbol"
        <*> o .: "veneer"

newtype ParsedCarrier = ParsedCarrier Carrier
instance FromJSON ParsedCarrier where
  parseJSON = withObject "carrier" $ \o ->
    fmap ParsedCarrier $ Carrier <$> (o .: "group" >>= group) <*> (Extent <$> address o "start" <*> address o "end")

newtype ParsedExternal = ParsedExternal External
instance FromJSON ParsedExternal where
  parseJSON = withObject "external" $ \o -> do
    kind <- o .: "kind"
    externalKind <- case kind :: String of
      "owned" -> pure OwnedEntry
      "retained" -> pure RetainedService
      "scratch" -> pure ScratchEntry
      _ -> fail "unknown external kind"
    fmap ParsedExternal $ External <$> o .: "name" <*> address o "address" <*> pure externalKind

newtype ParsedDemand = ParsedDemand Demand
instance FromJSON ParsedDemand where
  parseJSON = withObject "demand" $ \o -> do
    bytes <- o .: "bytes"
    unless (bytes > 0) (fail "non-positive demand")
    fmap ParsedDemand $ Demand <$> (o .: "group" >>= group) <*> o .: "symbol" <*> pure bytes

newtype ParsedDomain = ParsedDomain DomainMap
instance FromJSON ParsedDomain where
  parseJSON = withObject "domain" $ \o -> do
    base <- address o "loader_base"
    size <- o .: "loader_size"
    entries <- o .: "entries"
    carriers <- o .: "carriers"
    externals <- o .: "externals"
    demand <- o .: "demand"
    pure (ParsedDomain (DomainMap (if base == GuestAddress 0x40000 then DomainA else DomainB) base size [e | ParsedEntry e <- entries] [c | ParsedCarrier c <- carriers] [e | ParsedExternal e <- externals] [d | ParsedDemand d <- demand]))

instance FromJSON Union where
  parseJSON = withObject "union" $ \o -> do
    fill <- o .: "fill_word" >>= hexWord
    ds <- o .: "domains"
    ParsedDomain a <- ds .: "a"
    ParsedDomain b <- ds .: "b"
    pure (Union fill a b)

parseUnion :: B.ByteString -> Either UnionError Union
parseUnion bytes = do
  union <- either (Left . UnionUnparsed) Right (J.eitherDecodeStrict bytes)
  unless (unionFill union == 0xe7f000f0) (Left (FillWordDiffers (unionFill union)))
  forM_ domains $ \domain -> do
    let dm = domainMap union domain
        expected = GuestAddress (if domain == DomainA then 0x40000 else 0xd0000)
    unless (mapBase dm == expected) (Left (DomainBaseDiffers domain (mapBase dm)))
    forM_ (mapCarriers dm) $ \c -> do
      let e@(Extent (GuestAddress lo) (GuestAddress hi)) = carrierExtent c
      unless (lo `mod` 4 == 0 && hi `mod` 4 == 0 && lo < hi) (Left (CarrierNotAligned domain e))
      let GuestAddress base = mapBase dm
      unless (lo >= base && hi <= base + fromIntegral (mapSize dm)) (Left (CarrierNotAligned domain e))
    forM_ (zip (sortOn (extentStart . carrierExtent) (mapCarriers dm)) (drop 1 (sortOn (extentStart . carrierExtent) (mapCarriers dm)))) $ \(left, right) ->
      when (extentEnd (carrierExtent left) > extentStart (carrierExtent right)) (Left (CarriersOverlap domain (carrierExtent left) (carrierExtent right)))
    forM_ (mapEntries dm) $ \e ->
      unless (any (\c -> extentContains (carrierExtent c) (entryAddress e)) (mapCarriers dm)) (Left (EntryOutsideCarrier domain e))
    forM_ (zip (sortOn entryAddress (mapEntries dm)) (drop 1 (sortOn entryAddress (mapEntries dm)))) $ \(left, right) ->
      when (entryAddress left == entryAddress right) (Left (EntryRepeated domain (entryAddress left)))
    forM_ (mapExternals dm) $ \e -> case externalKind e of
      ScratchEntry -> pure ()
      _ -> do
        let GuestAddress at = externalAddress e
            GuestAddress base = mapBase dm
        unless (at >= base && at < base + fromIntegral (mapSize dm)) (Left (ExternalOutsideLoader domain e))
  pure union

domainMap :: Union -> Domain -> DomainMap
domainMap union domain = case domain of
  DomainA -> unionA union
  DomainB -> unionB union

-- | The carrier bytes between entry words, in address order.
runs :: DomainMap -> [Extent]
runs dm = sortOn extentStart (concatMap split (map carrierExtent (mapCarriers dm)))
  where
    addresses = sortOn id (map entryAddress (mapEntries dm))
    split (Extent start end) =
      [Extent a b | (a, b) <- zip starts ends, b > a]
      where
        inside = [p | p <- addresses, start <= p, p < end]
        starts = start : map (offset 4) inside
        ends = inside ++ [end]
        offset n (GuestAddress p) = GuestAddress (p + n)

totalRunBytes :: DomainMap -> Int
totalRunBytes = sum . map extentBytes . runs

totalDemandBytes :: DomainMap -> Int
totalDemandBytes = sum . map demandBytes . mapDemand

-- | First-fit decreasing over whole sections. A section that exceeds the
-- largest entry-free run cannot be placed without slicing it.
fit :: DomainMap -> Either FitError ()
fit dm = do
  let largest = maximum (0 : map extentBytes (runs dm))
  forM_ (sortOn (negate . demandBytes) (mapDemand dm)) $ \d ->
    when (demandBytes d > largest) (Left (CarrierDeficit (domainOf dm) (demandGroup d) (demandSymbol d) (demandBytes d) largest))
  let demand = totalDemandBytes dm
      supply = totalRunBytes dm
  when (demand > supply) (Left (TotalDeficit (domainOf dm) demand supply))
  where
    domainOf = mapDomain
