{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairPlan
  ( module Emulation.SchedulerPairCatalog, Role (..), roleName, parseRole, Guard (..), ArmingEntries, armingEntries, armingEntry, retainedArmingEntries
  , Body (..), Aggregate (..), emptyAggregate, Stop (..), StopKind (..), stopKindName, DroppedStop (..)
  , Plan (..), TransitionExtent (..), TransitionBinding, stockTransitions, fixedDispatchEntries, deriveTransitions
  , deriveTransitionBinding, applyTransitionBinding, base, derivePlan, derivePlanWith, derivePlanWithAggregate, planValue ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (..), ToJSON (..), Value, eitherDecodeStrict', object, withObject, (.:), (.:?), (.=))
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCatalog
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256Hex)
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

data TransitionExtent = TransitionExtent
  { transitionDomain :: Domain, transitionStart :: Address, transitionEnd :: Address } deriving (Eq, Ord, Show)
data TransitionBinding = TransitionBinding !Sha256 ![TransitionExtent]

instance ToJSON TransitionExtent where
  toJSON (TransitionExtent domain (Address start) (Address end))=object
    ["domain" .= domainLabel domain,"start" .= hex start,"end" .= hex end]
instance FromJSON TransitionExtent where
  parseJSON=withObject "transition extent" $ \o->do
    domain<-o .: "domain" >>= parseDomain
    start<-o .: "start" >>= parseHex
    end<-o .: "end" >>= parseHex
    unless (start `mod` 4==0 && end `mod` 4==0 && start<end
      && base domain<=start && end<=base domain+if domain==DomainA then 0xdae0 else 0xdaa0)
      (fail "transition extent is outside its loader")
    pure (TransitionExtent domain (Address start) (Address end))
instance ToJSON TransitionBinding where
  toJSON (TransitionBinding digest extents)=object ["plan_sha256" .= sha256Hex digest,"transition_extents" .= extents]
instance FromJSON TransitionBinding where
  parseJSON=withObject "image transition binding" $ \o->TransitionBinding
    <$> (o .: "plan_sha256" >>= either fail pure . parseSha256Hex) <*> o .: "transition_extents"

stockTransitions :: [TransitionExtent]
stockTransitions=[TransitionExtent domain (Address (base domain+lo)) (Address (base domain+hi))
  | domain<-domains,(lo,hi)<-[(0x204,0x368),(0x3640,0x3810)]]

newtype TransitionSymbol = TransitionSymbol String

transitionSymbol :: DispatchPoint -> TransitionSymbol
transitionSymbol point=TransitionSymbol ("xm1_rtos_scheduler_"++case point of
  Save->"save"; Resume->"resume"; NoSave->"no_save"; Select->"select"; FirstEntry->"first_entry"
  IrqReturn->"irq_return"; IrqResume->"irq_resume"; IrqTail->"irq_tail")

retainedDispatchExtent :: Domain -> DispatchPoint -> TransitionExtent
retainedDispatchExtent domain point=TransitionExtent domain (Address (base domain+start)) (Address (base domain+end))
 where
  (start,end)=case point of
    Save->(0x3640,0x3668); Resume->(0x3668,0x3688); NoSave->(0x3688,0x36a4); Select->(0x36a4,0x3728)
    FirstEntry->(0x3728,0x375c); IrqReturn->(0x375c,0x37a4); IrqResume->(0x37a4,0x37a8); IrqTail->(0x37a8,0x3810)

fixedDispatchEntries :: Role -> [Entry]
fixedDispatchEntries role=[Entry domain (DispatchSemantic point) (if role==Stock && point==Select then Address (base domain+0x36ac) else transitionStart (retainedDispatchExtent domain point))
  | domain<-domains,point<-[Save,Resume,NoSave,Select,FirstEntry,IrqReturn,IrqResume,IrqTail]]

linkedDispatchBody :: Aggregate -> Domain -> DispatchPoint -> Maybe Body
linkedDispatchBody linked domain point=let TransitionSymbol name=transitionSymbol point
  in M.lookup name (M.findWithDefault M.empty domain (aggregateSymbols linked))

deriveDispatchTargets :: Maybe Aggregate -> Role -> [Entry]
deriveDispatchTargets aggregate role=
  [case (role,aggregate,entrySemantic entry) of
    (Stock,_,_)->entry
    (_,Just linked,DispatchSemantic point)->case linkedDispatchBody linked (entryDomain entry) point of
      Just body->entry {entryAddress=Address (bodyStart body)}
      Nothing->entry
    _->entry
  | entry<-fixedDispatchEntries role]

newtype ArmInstruction = ArmInstruction Word32 deriving Eq

linkedDispatchProbes :: Aggregate -> Domain -> (Address -> ArmInstruction) -> Either String [Probe]
linkedDispatchProbes linked domain instruction=do
  noSave<-body NoSave
  select<-body Select
  unless (bodyStart noSave+bodyBytes noSave==bodyStart select
    && instruction (Address (bodyStart noSave+bodyBytes noSave-4))==ArmInstruction 0xe129f000)
    (Left "linked NoSave does not commit CPSR before Select")
  sequence
    [ afterLoad IrqReturn EnabledRead (Register 0) 0xe5920000 0xe3500000
    , afterLoad Select GlobalLockRead (Register 1) 0xe5901000 (0xe3510000 .|. if domain==DomainA then 0x11 else 0x12)
    , afterLoad Select SelectedRead (Register 1) 0xe5901000 0xe5001008
    , afterLoad Save DiscardedRead (Register 3) 0xe5913030 0xe3530001
    , afterLoad IrqReturn DiscardedRead (Register 0) 0xe5910030 0xe3500001
    , afterLoad IrqReturn SelectedRead (Register 0) 0xe5920000 0xe1500001
    ]
 where
  body point=do
    found<-maybe (Left "linked dispatch probe body is missing") Right (linkedDispatchBody linked domain point)
    unless (bodyBytes found>=8 && bodyStart found `mod` 4==0 && bodyBytes found `mod` 4==0
      && base domain<=bodyStart found && toInteger (bodyStart found)+toInteger (bodyBytes found)<=toInteger (base domain)+(if domain==DomainA then 0xdae0 else 0xdaa0))
      (Left "linked dispatch probe body is outside its loader")
    pure found
  afterLoad point kind register load following=do
    found<-body point
    let matches=[address | address<-[bodyStart found,bodyStart found+4..bodyStart found+bodyBytes found-8]
          ,instruction (Address address)==ArmInstruction load,instruction (Address (address+4))==ArmInstruction following]
    case matches of
      [address]->Right (Probe domain (Address (address+4)) kind register)
      _->Left "linked dispatcher load is not unique"

deriveTransitions :: Maybe Aggregate -> Role -> Either String [TransitionExtent]
deriveTransitions aggregate role
  | role==Stock=Right stockTransitions
  | otherwise=concat <$> forM domains (\domain->do
      let vector=TransitionExtent domain (Address (base domain+0x204)) (Address (base domain+0x368))
          retained=[extent | extent<-stockTransitions,transitionDomain extent==domain]
          points=[Save,Resume,NoSave,Select,FirstEntry,IrqReturn,IrqResume,IrqTail]
      case aggregate of
        Nothing->pure retained
        Just linked->do
          let symbols=M.findWithDefault M.empty domain (aggregateSymbols linked)
          extents<-forM points $ \point->do
            let TransitionSymbol name=transitionSymbol point
                retainedExtent=retainedDispatchExtent domain point
                Address entry=transitionStart retainedExtent
            case M.lookup name symbols of
              Nothing->do
                unless (M.notMember (domain,entry) (aggregateEntrySymbols linked))
                  (Left "published dispatcher transition body is missing")
                pure retainedExtent
              Just body->do
                unless (bodyBytes body>0 && bodyStart body `mod` 4==0 && bodyBytes body `mod` 4==0
                  && base domain<=bodyStart body && toInteger (bodyStart body)+toInteger (bodyBytes body)<=toInteger (base domain)+(if domain==DomainA then 0xdae0 else 0xdaa0))
                  (Left "linked transition body is outside its loader")
                pure (TransitionExtent domain (Address (bodyStart body)) (Address (bodyStart body+bodyBytes body)))
          pure (vector:nub extents))

transitionBinding :: B.ByteString -> Plan -> [TransitionExtent] -> Either String TransitionBinding
transitionBinding bytes plan extents=do
  original<-eitherDecodePlan bytes
  unless (original {planTransitionExtents=planTransitionExtents plan}==plan) (Left "transition supplement changes its original plan")
  pure (TransitionBinding (sha256 bytes) extents)

deriveTransitionBinding :: Maybe Aggregate -> B.ByteString -> B.ByteString -> M.Map Domain B.ByteString -> Either String TransitionBinding
deriveTransitionBinding aggregate bytes dat loaders=do
  plan<-eitherDecodePlan bytes
  digest<-parseSha256Hex (planDat plan)
  unless (digest==sha256 dat) (Left "transition image differs from the plan DAT")
  forM_ (planGuards plan) $ \guard->do
    loader<-maybe (Left "missing transition image loader") Right (M.lookup (guardDomain guard) loaders)
    let Address start=guardStart guard
        Address end=guardEnd guard
        offset=base (guardDomain guard)
    unless (B.length loader==(if guardDomain guard==DomainA then 0xdae0 else 0xdaa0)
      && offset<=start && start<end && toInteger end<=toInteger offset+toInteger (B.length loader))
      (Left "transition image guard is outside its loader")
    expected<-parseSha256Hex (guardDigest guard)
    unless (expected==sha256 (B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-offset)) loader)))
      (Left "transition image guard differs")
  extents<-deriveTransitions aggregate (planRole plan)
  forM_ extents $ \extent->unless (any (\stock->transitionDomain stock==transitionDomain extent && transitionStart stock<=transitionStart extent && transitionEnd extent<=transitionEnd stock) stockTransitions || any (\guard->
    guardDomain guard==transitionDomain extent && guardStart guard<=transitionStart extent && transitionEnd extent<=guardEnd guard) (planGuards plan))
    (Left "linked transition body is outside the plan's immutable guards")
  transitionBinding bytes plan extents

applyTransitionBinding :: B.ByteString -> Plan -> TransitionBinding -> Either String Plan
applyTransitionBinding bytes plan (TransitionBinding digest extents)=do
  unless (digest==sha256 bytes) (Left "transition supplement names another plan")
  unless (all (\domain->TransitionExtent domain (Address (base domain+0x204)) (Address (base domain+0x368)) `elem` extents) domains)
    (Left "transition supplement omits a retained IRQ vector")
  _<-transitionBinding bytes plan extents
  pure plan {planTransitionExtents=extents}

eitherDecodePlan :: B.ByteString -> Either String Plan
eitherDecodePlan = eitherDecodeStrict'

data Plan = Plan
  { planRole :: Role, planDat :: String, planEntries :: [Entry], planGuards :: [Guard], planFills :: [Address], planProbes :: [Probe]
  , planArmingEntries :: ArmingEntries, planStops :: [Stop], planDroppedStops :: [DroppedStop]
  , planTransitionExtents :: [TransitionExtent], planDispatchTargets :: [Entry]
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
  let publishedEntries = map stockEntry (concatMap catalogEntries catalogs)
      targets=deriveDispatchTargets aggregate role
      observed entry=case (role,aggregate,entrySemantic entry) of
        (Stock,_,_)->entry
        (_,Just linked,DispatchSemantic point)->case linkedDispatchBody linked (entryDomain entry) point of
          Just body->entry {entryAddress=Address (bodyStart body)}
          Nothing->entry
        _->entry
      entries=map observed publishedEntries
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
        fixedS2Probes = if not dispatch then [] else
          [mk (offset+0x376c) EnabledRead 0,mk (offset+if role==Stock then 0x371c else 0x3708) GlobalLockRead (if role==Stock then 0 else 1)] ++
          (if role==Stock then
            [mk (offset+0x36b4) SelectedRead 1,mk (offset+0x377c) SelectedRead 0,mk (offset+0x3788) DiscardedRead 0]
          else
            [mk (offset+0x36ac) SelectedRead 1,mk (offset+0x3650) DiscardedRead 3,mk (offset+0x3778) DiscardedRead 0,mk (offset+0x3788) SelectedRead 0])
        deliverEntries = [start | Entry d (ServiceSemantic DeliverTaskException) (Address start) <- entries, d==domain]
        firstEntryEntries = [start | Entry d (DispatchSemantic FirstEntry) (Address start) <- publishedEntries, d==domain]
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
        dispatchBodies=case aggregate of
          Just linked | role/=Stock && dispatch->[body | point<-[Save,Resume,NoSave,Select,FirstEntry,IrqReturn,IrqResume,IrqTail],Just body<-[linkedDispatchBody linked domain point]]
          _->[]
        usedBodies = dispatchBodies++[body | start <- deliverEntries ++ firstEntryEntries, Just body <- [bodyOf start]]
        familyGuards = if role==Stock || not family then [] else
          [Guard domain (Address lo) (Address hi) (sha256Hex (sha256 (bytes lo hi))) | (lo,hi) <- familyRanges domain]
        guards = guards0 ++ familyGuards ++ [bodyGuard body | body <- nub usedBodies, not (bodyCovered body)]
        familyDomainProbes = if family then familyProbes domain else []
    s2Probes<-case aggregate of
      Just linked | role/=Stock && dispatch->linkedDispatchProbes linked domain (ArmInstruction . word . unAddress)
      _->pure fixedS2Probes
    let domainProbes=[probe | probe<-probes,probeDomain probe==domain,not dispatch]++s2Probes++callbackProbes++retries++familyDomainProbes
    unless (null deliverEntries || length callbackProbes==2)
      (Left "exception callback instruction differs")
    pure (guards,fills,domainProbes,stops,droppedStops)
  transitions<-deriveTransitions aggregate role
  pure (Plan role (sha256Hex (sha256 dat)) entries
    (concat [guards | (guards,_,_,_,_) <- perDomain])
    (nub (concat [fills | (_,fills,_,_,_) <- perDomain]))
    (concat [probes' | (_,_,probes',_,_) <- perDomain])
    retainedArmingEntries
    (concat [stops' | (_,_,_,stops',_) <- perDomain])
    (concat [dropped | (_,_,_,_,dropped) <- perDomain]) transitions targets)
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
  , "transition_extents" .= planTransitionExtents plan
  , "dispatch_targets" .= [object ["domain" .= domainLabel d,"entry" .= hex a,"semantic" .= semanticValue semantic] | Entry d semantic (Address a) <- planDispatchTargets plan]
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
    transitions<-o .:? "transition_extents" >>= maybe (pure stockTransitions) pure
    targets<-o .:? "dispatch_targets" >>= maybe (pure (fixedDispatchEntries role)) (mapM (withObject "dispatch target" $ \e->Entry <$> (e .: "domain" >>= parseDomain) <*> e .: "semantic" <*> (Address <$> (e .: "entry" >>= parseHex))))
    Plan role <$> o .: "dat_sha256" <*> pure entries <*> pure guards <*> (o .: "fills" >>= mapM (fmap Address . parseHex)) <*> pure probes <*> pure parsedArmings <*> pure stops <*> pure dropped <*> pure transitions <*> pure targets
