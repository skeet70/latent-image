{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairPlan
  ( module Emulation.SchedulerPairCatalog, Role (..), roleName, parseRole, Guard (..), Plan (..), base, derivePlan, planValue ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCatalog
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains, domainLabel)

data Role = Stock | Owned | Restored | SilentNotify | WrongPredicate deriving (Eq, Ord, Show, Enum, Bounded)
roleName :: Role -> String
roleName role = ["stock", "owned", "control-restored", "control-silent-notify", "control-wrong-predicate"] !! fromEnum role
parseRole :: String -> Either String Role
parseRole name = maybe (Left "unknown scheduler role") Right (lookup name [(roleName r,r) | r <- [minBound..maxBound]])
data Guard = Guard { guardDomain :: Domain, guardStart :: Address, guardEnd :: Address, guardDigest :: String } deriving (Eq, Show)
data Plan = Plan { planRole :: Role, planDat :: String, planEntries :: [Entry], planGuards :: [Guard], planFills :: [Address], planProbes :: [Probe] } deriving (Eq, Show)
base :: Domain -> Word32
base DomainA = 0x40000
base DomainB = 0xd0000

-- Extent identity and byte admission belong to each leaf stager.
derivePlan :: Role -> B.ByteString -> [Catalog] -> M.Map Domain B.ByteString -> Either String Plan
derivePlan role dat catalogs loaders = do
  unless (not (null catalogs) && all ((==0xe7f000f0) . catalogFill) catalogs) (Left "regions fill differs")
  let entries = concatMap catalogEntries catalogs
      keys = [(entryDomain entry,entryAddress entry) | entry <- entries]
      windows = nub (concatMap catalogWindows catalogs)
      probes = concatMap catalogProbes catalogs
  unless (not (null entries) && length keys==length (nub keys)) (Left "entry list is empty or repeats an address")
  perDomain <- forM domains $ \domain -> do
    loader <- maybe (Left "missing loader") Right (M.lookup domain loaders)
    unless (B.length loader == if domain==DomainA then 0xdae0 else 0xdaa0) (Left "loader size differs")
    let selected = [window | window <- windows,windowDomain window==domain]
        inLoader (Address address) = address>=base domain && address<base domain+fromIntegral (B.length loader) && address `mod` 4==0
    unless (not (null selected) && all (\(Window _ lo (Address hi)) -> inLoader lo && hi>unAddress lo && hi<=base domain+fromIntegral (B.length loader) && hi `mod` 4==0) selected)
      (Left "guarded extent outside loader")
    unless (all (inLoader . entryAddress) [entry | entry <- entries,entryDomain entry==domain] && all (inLoader . probeAddress) [probe | probe <- probes,probeDomain probe==domain])
      (Left "entry or probe outside loader")
    let bytes lo hi = B.take (fromIntegral (hi-lo)) (B.drop (fromIntegral (lo-base domain)) loader)
        guards = [Guard domain (Address lo) (Address hi) (sha256Hex (sha256 (bytes lo hi))) | Window _ (Address lo) (Address hi) <- selected]
        fills = [Address address | Window _ (Address lo) (Address hi) <- selected,address <- [lo,lo+4..hi-4],B.unpack (bytes address (address+4))==[0xf0,0x00,0xf0,0xe7]]
    pure (guards,fills)
  pure (Plan role (sha256Hex (sha256 dat)) entries (concatMap fst perDomain) (nub (concatMap snd perDomain)) probes)
 where unAddress (Address address) = address

planValue :: Plan -> Value
planValue plan = object
  [ "schema" .= (2::Int),"role" .= roleName (planRole plan),"dat_sha256" .= planDat plan
  , "domains" .= [object ["domain" .= domainLabel d,"loader_entry" .= hex (base d+0x20)] | d <- domains]
  , "entries" .= [object ["domain" .= domainLabel d,"entry" .= hex a,"semantic" .= semanticValue semantic] | Entry d semantic (Address a) <- planEntries plan]
  , "guarded" .= [object ["domain" .= domainLabel d,"start" .= hex lo,"end" .= hex hi,"sha256" .= digest] | Guard d (Address lo) (Address hi) digest <- planGuards plan]
  , "fills" .= [hex a | Address a <- planFills plan]
  , "probes" .= [object ["domain" .= domainLabel d,"address" .= hex a,"kind" .= probeKindName kind,"register" .= registerName register] | Probe d (Address a) kind register <- planProbes plan]
  ]
instance FromJSON Plan where
  parseJSON = withObject "scheduler plan" $ \o -> do
    schema <- o .: "schema"
    unless (schema==(2::Int)) (fail "plan schema differs")
    role <- o .: "role" >>= either fail pure . parseRole
    entries <- o .: "entries" >>= mapM (withObject "entry" $ \e -> Entry <$> (e .: "domain" >>= parseDomain) <*> e .: "semantic" <*> (Address <$> (e .: "entry" >>= parseHex)))
    guards <- o .: "guarded" >>= mapM (withObject "guard" $ \g -> Guard <$> (g .: "domain" >>= parseDomain) <*> (Address <$> (g .: "start" >>= parseHex)) <*> (Address <$> (g .: "end" >>= parseHex)) <*> g .: "sha256")
    probes <- o .: "probes" >>= mapM (withObject "probe" $ \p -> Probe <$> (p .: "domain" >>= parseDomain) <*> (Address <$> (p .: "address" >>= parseHex)) <*> (p .: "kind" >>= named probeKindName) <*> (p .: "register" >>= registerOf))
    Plan role <$> o .: "dat_sha256" <*> pure entries <*> pure guards <*> (o .: "fills" >>= mapM (fmap Address . parseHex)) <*> pure probes
