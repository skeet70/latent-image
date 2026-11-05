{-# LANGUAGE OverloadedStrings #-}

-- | The scheduler crate's regions.json: per loader, each helper's stock extent, symbol and callees,
-- the stock raise-IRQ entry, and the shims placing a body in another helper's extent.
module Replacement.RtosScheduler.Regions
  ( Helper (..)
  , helpers
  , helperName
  , Callee (..)
  , Extent (..)
  , HelperRegion (..)
  , Shim (..)
  , SchedulerDomain (..)
  , SchedulerRegions (..)
  , schedulerDomain
  , parseSchedulerRegions
  , regionOf
  ) where

import Data.Aeson (FromJSON (..), withObject, withText, (.:))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.RtosOwnFirst.Regions (Domain (..))

-- | The lock pair is two rows.
data Helper = Acquire | Release | Find | Insert | Remove | Reset | Prepare | Notify | Requeue | Rotate
  deriving (Eq, Ord, Show, Enum, Bounded)

helpers :: [Helper]
helpers = [minBound .. maxBound]

-- | As regions.json names it.
helperName :: Helper -> String
helperName helper = case helper of
  Acquire -> "acquire"
  Release -> "release"
  Find -> "find"
  Insert -> "insert"
  Remove -> "remove"
  Reset -> "reset"
  Prepare -> "prepare"
  Notify -> "notify"
  Requeue -> "requeue"
  Rotate -> "rotate"

data Callee = CalleeHelper Helper | CalleeStockRaiseIrq
  deriving (Eq, Show)

-- | Half-open.
data Extent = Extent {extentStart :: Word32, extentEnd :: Word32}
  deriving (Eq, Ord, Show)

data HelperRegion = HelperRegion
  { regionHelper :: Helper
  , regionEntry :: Word32
  , -- | The stock extent, which the owned body fills.
    regionExtent :: Extent
  , regionSymbol :: String
  , regionCalls :: [Callee]
  }
  deriving (Eq, Show)

-- | @shimHelper@'s entry branches to a body at @shimBody@ inside @shimHost@'s extent.
data Shim = Shim
  { shimHelper :: Helper
  , shimBody :: Extent
  , shimBodySymbol :: String
  , shimHost :: Helper
  }
  deriving (Eq, Show)

data SchedulerDomain = SchedulerDomain
  { domainRaiseIrq :: Word32
  , domainRegions :: [HelperRegion]
  , domainShims :: [Shim]
  }
  deriving (Eq, Show)

data SchedulerRegions = SchedulerRegions
  { regionsFill :: Word32
  , regionsA :: SchedulerDomain
  , regionsB :: SchedulerDomain
  }
  deriving (Eq, Show)

schedulerDomain :: SchedulerRegions -> Domain -> SchedulerDomain
schedulerDomain regions domain = case domain of
  DomainA -> regionsA regions
  DomainB -> regionsB regions

regionOf :: SchedulerDomain -> Helper -> Either String HelperRegion
regionOf table helper = maybe (Left ("no region for " ++ helperName helper)) Right (find ((== helper) . regionHelper) (domainRegions table))

hexWord :: J.Value -> Parser Word32
hexWord = withText "hex word" $ \text -> case T.unpack text of
  '0' : 'x' : digits@(_ : _) | all (`elem` ("0123456789abcdef" :: String)) digits -> case readHex digits of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (fromInteger value)
    _ -> fail ("not a 32-bit word: " ++ T.unpack text)
  _ -> fail ("not a lowercase 0x hex word: " ++ T.unpack text)

address :: J.Object -> J.Key -> Parser Word32
address object key = object .: key >>= hexWord

helperNamed :: String -> Parser Helper
helperNamed name = maybe (fail ("unknown helper " ++ name)) pure (find ((== name) . helperName) helpers)

callee :: String -> Parser Callee
callee name
  | name == "stock_raise_irq" = pure CalleeStockRaiseIrq
  | otherwise = CalleeHelper <$> helperNamed name

newtype ParsedRegion = ParsedRegion HelperRegion

instance FromJSON ParsedRegion where
  parseJSON = withObject "region" $ \o ->
    fmap ParsedRegion $
      HelperRegion
        <$> (o .: "helper" >>= helperNamed)
        <*> address o "entry"
        <*> (Extent <$> address o "start" <*> address o "end")
        <*> o .: "symbol"
        <*> (o .: "calls" >>= mapM callee)

newtype ParsedShim = ParsedShim Shim

instance FromJSON ParsedShim where
  parseJSON = withObject "shim" $ \o -> do
    body <- o .: "body"
    fmap ParsedShim $
      Shim
        <$> (o .: "helper" >>= helperNamed)
        <*> (Extent <$> address body "address" <*> address body "end")
        <*> body .: "symbol"
        <*> (body .: "host_helper" >>= helperNamed)

newtype ParsedDomain = ParsedDomain SchedulerDomain

instance FromJSON ParsedDomain where
  parseJSON = withObject "domain" $ \o -> do
    regions <- o .: "regions"
    shims <- o .: "shims"
    raise <- address o "stock_raise_irq"
    pure (ParsedDomain (SchedulerDomain raise [r | ParsedRegion r <- regions] [s | ParsedShim s <- shims]))

instance FromJSON SchedulerRegions where
  parseJSON = withObject "regions" $ \o -> do
    fill <- o .: "fill_word" >>= hexWord
    tables <- o .: "domains"
    ParsedDomain a <- tables .: "a"
    ParsedDomain b <- tables .: "b"
    pure (SchedulerRegions fill a b)

parseSchedulerRegions :: B.ByteString -> Either String SchedulerRegions
parseSchedulerRegions = J.eitherDecodeStrict
