{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairPlan
  ( module Emulation.SchedulerPairCatalog, Role (..), roleName, parseRole, Guard (..), ArmingEntries, armingEntries, armingEntry, retainedArmingEntries
  , Body (..), Aggregate (..), emptyAggregate, Stop (..), StopKind (..), stopKindName, DroppedStop (..)
  , Plan (..), base, derivePlan, derivePlanWith, derivePlanWithAggregate, planValue ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.:?), (.=))
import qualified Data.ByteString as B
import Data.Bits ((.&.), shiftL, shiftR)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCatalog
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains, domainLabel)

data Role = Stock | Owned | Restored | SilentNotify | WrongPredicate | WakeNoInsert deriving (Eq, Ord, Show, Enum, Bounded)
roleName :: Role -> String
roleName role = ["stock", "owned", "control-restored", "control-silent-notify", "control-wrong-predicate", "control-wake-no-insert"] !! fromEnum role
parseRole :: String -> Either String Role
parseRole name = maybe (Left "unknown scheduler role") Right (lookup name [(roleName r,r) | r <- [minBound..maxBound]])
data Guard = Guard { guardDomain :: Domain, guardStart :: Address, guardEnd :: Address, guardDigest :: String } deriving (Eq, Show)
data ArmingEntries = ArmingEntries !Address !Address deriving (Eq, Show)
armingEntries :: Address -> Address -> Either String ArmingEntries
armingEntries primary@(Address primaryPc) secondary@(Address secondaryPc)
  | primaryPc/=0 && secondaryPc/=0 && primaryPc/=secondaryPc && primaryPc `mod` 4==0 && secondaryPc `mod` 4==0 = Right (ArmingEntries primary secondary)
  | otherwise = Left "arming entries must be distinct nonzero aligned instruction addresses"
armingEntry :: ArmingEntries -> Domain -> Address
armingEntry (ArmingEntries primary _) DomainA = primary
armingEntry (ArmingEntries _ secondary) DomainB = secondary
retainedArmingEntries :: ArmingEntries
retainedArmingEntries = ArmingEntries (Address 0x40020) (Address 0xd0020)
-- | The linked extent of one owned body, as the aggregate's symbol table reports it.
data Body = Body { bodyStart :: Word32, bodyBytes :: Word32 } deriving (Eq, Show)

-- | The aggregate image's entry-to-symbol binding and symbol extents.
data Aggregate = Aggregate
  { aggregateSymbols :: M.Map Domain (M.Map String Body)
  , aggregateEntrySymbols :: M.Map (Domain, Word32) String
  }
emptyAggregate :: Aggregate
emptyAggregate = Aggregate M.empty M.empty

data StopKind = FirstEntryStop deriving (Eq, Show, Enum, Bounded)
stopKindName :: StopKind -> String
stopKindName FirstEntryStop = "first-entry"

-- | A scheduler instruction stop the observer arms instead of the stock address.
data Stop = Stop { stopDomain :: Domain, stopAddress :: Address, stopKind :: StopKind } deriving (Eq, Show)
data DroppedStop = DroppedStop { droppedDomain :: Domain, droppedName :: String, droppedReason :: String } deriving (Eq, Show)

data Plan = Plan
  { planRole :: Role, planDat :: String, planEntries :: [Entry], planGuards :: [Guard], planFills :: [Address], planProbes :: [Probe]
  , planArmingEntries :: ArmingEntries, planStops :: [Stop], planDroppedStops :: [DroppedStop]
  } deriving (Eq, Show)
base :: Domain -> Word32
base DomainA = 0x40000
base DomainB = 0xd0000

-- Extent identity and byte admission belong to each leaf stager.
derivePlan :: Role -> B.ByteString -> [Catalog] -> M.Map Domain B.ByteString -> Either String Plan
derivePlan = derivePlanWithAggregate Nothing

derivePlanWith :: Aggregate -> Role -> B.ByteString -> [Catalog] -> M.Map Domain B.ByteString -> Either String Plan
derivePlanWith = derivePlanWithAggregate . Just

-- | Without an aggregate the non-stock roles take the retained `0x43758`/`0xd3758` stop
-- and the s3c callback probe at `+0x4204`.
derivePlanWithAggregate :: Maybe Aggregate -> Role -> B.ByteString -> [Catalog] -> M.Map Domain B.ByteString -> Either String Plan
derivePlanWithAggregate aggregate role dat catalogs loaders = do
  unless (not (null catalogs) && all ((==0xe7f000f0) . catalogFill) catalogs) (Left "regions fill differs")
  let entries = map stockEntry (concatMap catalogEntries catalogs)
      keys = [(entryDomain entry,entryAddress entry) | entry <- entries]
      windows = nub (concatMap catalogWindows catalogs)
      probes = concatMap catalogProbes catalogs
      family = any isObservedEntry entries
      guardedFamily entry = not (isObservedEntry entry) || any (\window -> windowDomain window==entryDomain entry && windowStart window<=entryAddress entry && entryAddress entry<windowEnd window) windows
  unless (not (null entries) && length keys==length (nub keys)) (Left "entry list is empty or repeats an address")
  unless (not family || any (not . isObservedEntry) entries) (Left "family catalog requires scheduler entries")
  unless (all guardedFamily entries) (Left "family entry outside guarded extent")
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
        guards0 = [Guard domain (Address lo) (Address hi) (sha256Hex (sha256 (bytes lo hi))) | Window _ (Address lo) (Address hi) <- selected]
        fills = [Address address | Window _ (Address lo) (Address hi) <- selected,address <- [lo,lo+4..hi-4],B.unpack (bytes address (address+4))==[0xf0,0x00,0xf0,0xe7]]
        word address = sum [fromIntegral byte `shiftL` (8*index) | (index,byte) <- zip [0..] (B.unpack (bytes address (address+4)))] :: Word32
        mk address kind register = Probe domain (Address address) kind (Register register)
        offset = base domain
        dispatch = any (\entry -> entryDomain entry==domain && entrySemantic entry==DispatchSemantic Select) entries
        s2Probes = if not dispatch then [] else
          [mk (offset+0x376c) EnabledRead 0,mk (offset+if role==Stock then 0x371c else 0x3708) GlobalLockRead (if role==Stock then 0 else 1)] ++
          (if role==Stock then
            [mk (offset+0x36b4) SelectedRead 1,mk (offset+0x377c) SelectedRead 0,mk (offset+0x3788) DiscardedRead 0]
          else
            [mk (offset+0x36ac) SelectedRead 1,mk (offset+0x3650) DiscardedRead 3,mk (offset+0x3778) DiscardedRead 0,mk (offset+0x3788) SelectedRead 0])
        deliverEntries = [start | Entry d (ServiceSemantic DeliverTaskException) (Address start) <- entries, d==domain]
        firstEntryEntries = [start | Entry d (DispatchSemantic FirstEntry) (Address start) <- entries, d==domain]
        bodyOf address = do
          bound <- aggregate
          symbol <- M.lookup (domain, address) (aggregateEntrySymbols bound)
          M.lookup symbol (M.findWithDefault M.empty domain (aggregateSymbols bound))
        bodyWords body = [bodyStart body, bodyStart body+4 .. bodyStart body+bodyBytes body-4]
        bodyCovered body = any (\(Guard d (Address lo) (Address hi) _) -> d==domain && lo<=bodyStart body && bodyStart body+bodyBytes body<=hi) guards0
        bodyGuard body = Guard domain (Address (bodyStart body)) (Address (bodyStart body+bodyBytes body))
          (sha256Hex (sha256 (bytes (bodyStart body) (bodyStart body+bodyBytes body))))
        fixedCallbackProbes delta = concat
          [ [mk address Callback (fromIntegral (instruction .&. 15)),mk (address+4) CallbackReturn 0]
          | Entry d (ServiceSemantic DeliverTaskException) (Address start) <- entries,d==domain
          , let address=offset+delta-(if domain==DomainB then 64 else 0)
          , start<=address && address<start+212,let instruction=word address,instruction .&. 0x0ffffff0==0x012fff30]
        callbackProbes
          | role==Stock = fixedCallbackProbes 0x4218
          | Just _ <- aggregate = concat
              [ case bodyOf start of
                  Nothing -> []
                  Just body -> case callbackCall word body of
                    [(address,register)] | address+4 < bodyStart body+bodyBytes body ->
                      [mk address Callback (fromIntegral register),mk (address+4) CallbackReturn 0]
                    _ -> []
              | start <- deliverEntries]
          | otherwise = fixedCallbackProbes 0x4204
        retries = [mk (offset + (if role==Stock then stock else owned) - (if domain==DomainB then 64 else 0)) Retry 0
          | (service,stock,owned) <- [(DisableDispatch,0xca78,0xca4c),(SleepTask,0x5d9c,0x5d60),(SuspendTask,0x6b60,0x6b10),(EnableTaskException,0x74c8,0x749c)]
          , any (\entry -> entryDomain entry==domain && entrySemantic entry==ServiceSemantic service) entries]
        fixedStop = Stop domain (Address (offset+0x3758)) FirstEntryStop
        stops
          | role==Stock = []
          | Nothing <- aggregate = [fixedStop]
          | otherwise = concat
              [ case bodyOf start of
                  Nothing -> []
                  Just body -> case [address | address <- bodyWords body, word address==0xe592f008] of
                    [address] -> [Stop domain (Address address) FirstEntryStop]
                    _ -> []
              | start <- firstEntryEntries]
        droppedStops
          | role==Stock = []
          | Nothing <- aggregate = []
          | otherwise = concat
              [ case bodyOf start of
                  Nothing -> [DroppedStop domain "first-entry" "the owned symbol is not bound"]
                  Just body -> case [address | address <- bodyWords body, word address==0xe592f008] of
                    [_] -> []
                    [] -> [DroppedStop domain "first-entry" "the owned symbol commits no task pc"]
                    _ -> [DroppedStop domain "first-entry" "the owned symbol commits a task pc more than once"]
              | start <- firstEntryEntries]
        usedBodies = [body | start <- deliverEntries ++ firstEntryEntries, Just body <- [bodyOf start]]
        familyGuards = if role==Stock || not family then [] else
          [Guard domain (Address lo) (Address hi) (sha256Hex (sha256 (bytes lo hi))) | (lo,hi) <- familyRanges domain]
        guards = guards0 ++ familyGuards ++ [bodyGuard body | body <- nub usedBodies, not (bodyCovered body)]
        domainProbes = [probe | probe <- probes,probeDomain probe==domain,not dispatch] ++ s2Probes ++ callbackProbes ++ retries ++ familyDomainProbes
        familyDomainProbes = if family then familyProbes domain else []
    unless (null deliverEntries || length callbackProbes==2)
      (Left "exception callback instruction differs")
    pure (guards,fills,domainProbes,stops,droppedStops)
  pure (Plan role (sha256Hex (sha256 dat)) entries
    (concat [guards | (guards,_,_,_,_) <- perDomain])
    (nub (concat [fills | (_,fills,_,_,_) <- perDomain]))
    (concat [probes' | (_,_,probes',_,_) <- perDomain])
    retainedArmingEntries
    (concat [stops' | (_,_,_,stops',_) <- perDomain])
    (concat [dropped | (_,_,_,_,dropped) <- perDomain]))
 where
  unAddress (Address address) = address
  stockEntry entry@(Entry domain (DispatchSemantic Select) _) | role==Stock = entry { entryAddress=Address (base domain+0x36ac) }
  stockEntry entry = entry

-- | The @blx@ in an owned exception-loop body whose register was loaded from @[x+0x1c]@, the
-- exception handler the stock probe names at its second callback call.
callbackCall :: (Word32 -> Word32) -> Body -> [(Word32, Word32)]
callbackCall word body =
  [ (address, register)
  | address <- [bodyStart body, bodyStart body+4 .. bodyStart body+bodyBytes body-4]
  , let instruction = word address
  , instruction .&. 0x0ffffff0 == 0x012fff30
  , let register = instruction .&. 15
  , any (\load -> load < address && address - load <= 32
        && word load .&. 0x0ff00fff == 0x0590001c
        && (word load `shiftR` 12) .&. 15 == register)
      [bodyStart body, bodyStart body+4 .. address-4]
  ]

isObservedEntry :: Entry -> Bool
isObservedEntry (Entry _ (FamilySemantic _) _) = True
isObservedEntry (Entry _ (SemaphoreSemantic _) _) = True
isObservedEntry _ = False

-- | The timer carrier extents the family probes enter, from carriers.json.
familyRanges :: Domain -> [(Word32,Word32)]
familyRanges domain = case domain of
  DomainA -> [(0x44c60,0x44ca8),(0x44ca8,0x44d8c),(0x44318,0x444e4)]
  DomainB -> [(0xd4c20,0xd4c68),(0xd4c68,0xd4d4c),(0xd42d8,0xd44a4)]

familyProbes :: Domain -> [Probe]
familyProbes domain =
  [Probe domain (Address address) kind (Register register)
  | (address,kind) <- [(timerInsert, TimerInsert),(timerRemove, TimerRemove)]
  , register <- [0,1]]
  ++ [Probe domain (Address consume) TimerConsume (Register 0)]
 where
  (timerInsert,timerRemove,consume) = case domain of
    DomainA -> (0x44c60,0x44ca8,0x44318)
    DomainB -> (0xd4c20,0xd4c68,0xd42d8)

planValue :: Plan -> Value
planValue plan = object
  ([ "schema" .= (3::Int),"role" .= roleName (planRole plan),"dat_sha256" .= planDat plan
  , "domains" .= [object ["domain" .= domainLabel d,"loader_entry" .= hex address] | d <- domains, let Address address=armingEntry (planArmingEntries plan) d]
  , "entries" .= [object ["domain" .= domainLabel d,"entry" .= hex a,"semantic" .= semanticValue semantic] | Entry d semantic (Address a) <- planEntries plan]
  , "guarded" .= [object ["domain" .= domainLabel d,"start" .= hex lo,"end" .= hex hi,"sha256" .= digest] | Guard d (Address lo) (Address hi) digest <- planGuards plan]
  , "fills" .= [hex a | Address a <- planFills plan]
  , "probes" .= [object ["domain" .= domainLabel d,"address" .= hex a,"kind" .= probeKindName kind,"register" .= registerName register] | Probe d (Address a) kind register <- planProbes plan]
  ] ++ if planRole plan == Stock then [] else
  [ "stops" .= [object ["domain" .= domainLabel d,"address" .= hex a,"kind" .= stopKindName kind] | Stop d (Address a) kind <- planStops plan]
  , "dropped_stops" .= [object ["domain" .= domainLabel d,"name" .= name,"reason" .= reason] | DroppedStop d name reason <- planDroppedStops plan]
  ])
instance FromJSON Plan where
  parseJSON = withObject "scheduler plan" $ \o -> do
    schema <- o .: "schema"
    unless (schema==(3::Int)) (fail "plan schema differs")
    role <- o .: "role" >>= either fail pure . parseRole
    domainEntries <- o .: "domains" >>= mapM (withObject "domain arming" $ \entry -> (,) <$> (entry .: "domain" >>= parseDomain) <*> (Address <$> (entry .: "loader_entry" >>= parseHex)))
    parsedArmings <- case sortOn fst domainEntries of
      [(DomainA,primary),(DomainB,secondary)] -> either fail pure (armingEntries primary secondary)
      _ -> fail "plan requires one arming entry for each domain"
    entries <- o .: "entries" >>= mapM (withObject "entry" $ \e -> Entry <$> (e .: "domain" >>= parseDomain) <*> e .: "semantic" <*> (Address <$> (e .: "entry" >>= parseHex)))
    guards <- o .: "guarded" >>= mapM (withObject "guard" $ \g -> Guard <$> (g .: "domain" >>= parseDomain) <*> (Address <$> (g .: "start" >>= parseHex)) <*> (Address <$> (g .: "end" >>= parseHex)) <*> g .: "sha256")
    probes <- o .: "probes" >>= mapM (withObject "probe" $ \p -> Probe <$> (p .: "domain" >>= parseDomain) <*> (Address <$> (p .: "address" >>= parseHex)) <*> (p .: "kind" >>= named probeKindName) <*> (p .: "register" >>= registerOf))
    stops <- o .:? "stops" >>= maybe (pure []) (mapM (withObject "stop" $ \s -> Stop <$> (s .: "domain" >>= parseDomain) <*> (Address <$> (s .: "address" >>= parseHex)) <*> (s .: "kind" >>= named stopKindName)))
    dropped <- o .:? "dropped_stops" >>= maybe (pure []) (mapM (withObject "dropped stop" $ \s -> DroppedStop <$> (s .: "domain" >>= parseDomain) <*> s .: "name" <*> s .: "reason"))
    Plan role <$> o .: "dat_sha256" <*> pure entries <*> pure guards <*> (o .: "fills" >>= mapM (fmap Address . parseHex)) <*> pure probes <*> pure parsedArmings <*> pure stops <*> pure dropped
