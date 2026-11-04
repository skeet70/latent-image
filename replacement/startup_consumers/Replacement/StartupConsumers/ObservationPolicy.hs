{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.ObservationPolicy
  ( ObservationPolicy, PolicyError(..), standalonePolicy, sharedPolicy
  , policyGlobalSites, policyInitialOuterReturn, policyRetiresPoll, policyPollSites
  , policyExternalBefore, policyExtraFetchRanges, verifyPolicySegment
  ) where

import Control.Monad (foldM, forM_, unless)
import Data.Aeson (FromJSON(..), Object, eitherDecodeStrict', withObject, (.:))
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.))
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import qualified Data.Text as T
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.LoaderOrchestration.Interface as L
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupConsumers.JoinedHost as Host
import qualified Replacement.StartupConsumers.JoinedWire as J
import qualified Replacement.StartupConsumers.Source as S
import qualified Replacement.StartupConsumers.Wire as W

data PolicyError = PolicyDecodeRejected | PolicyBindingRejected | PolicyOwnersRejected
  | PolicyBoundaryRejected | PolicySegmentRejected deriving (Eq, Show)
data ObservationPolicy = Standalone | Shared [K.Address] [K.Address] (M.Map W.SegmentId SegmentObservation)
-- Start counts, end counts and ordered stops the policy was checked against, with the external sites held before the SDK step.
data SegmentObservation = SegmentObservation [K.Count] [K.Count] [W.TraceStop] [K.Address]
data Owner = RoutePermanent | DispatcherPermanent | RouteCall J.TraceIndex
  | RouteScheduledTarget J.TraceIndex | DispatcherReturn J.TraceIndex deriving (Eq, Ord, Show)
data OwnedSite = OwnedSite K.Address [Owner]
data BoundaryPolicy = BoundaryPolicy J.TraceIndex K.Core K.Address K.Count [OwnedSite] [OwnedSite]
-- Segment id and its [trace_begin,trace_end) slice of the boundaries; owner sets are replayed from the initial set.
data SegmentPolicy = SegmentPolicy W.SegmentId J.TraceIndex J.TraceIndex
newtype WatchInput = WatchInput J.FileBinding
data PolicyFile = PolicyFile Int K.Digest K.Digest K.Digest [WatchInput]
  J.TraceIndex J.TraceIndex [OwnedSite] [BoundaryPolicy] [SegmentPolicy]
type Owners = M.Map K.Address (Set.Set Owner)

closed :: [T.Text] -> Object -> Parser ()
closed names object = unless (sort (map Key.toText (KM.keys object)) == sort names) (fail "unexpected policy fields")
instance FromJSON Owner where
  parseJSON = withObject "observation owner" $ \o -> do
    kind <- o .: "kind" :: Parser T.Text
    case kind of
      "route-permanent" -> closed ["kind"] o >> pure RoutePermanent
      "dispatcher-permanent" -> closed ["kind"] o >> pure DispatcherPermanent
      "route-call" -> closed ["kind","origin"] o >> RouteCall <$> o .: "origin"
      "route-scheduled-target" -> closed ["kind","origin"] o >> RouteScheduledTarget <$> o .: "origin"
      "dispatcher-return" -> closed ["kind","origin"] o >> DispatcherReturn <$> o .: "origin"
      _ -> fail "unknown observation owner"
instance FromJSON OwnedSite where
  parseJSON = withObject "owned site" $ \o -> do
    closed ["address","owners"] o
    OwnedSite <$> o .: "address" <*> o .: "owners"
instance FromJSON BoundaryPolicy where
  parseJSON = withObject "observation boundary" $ \o -> do
    closed ["trace_index","core","pc","instructions","acquire","release"] o
    BoundaryPolicy <$> o .: "trace_index" <*> o .: "core" <*> o .: "pc" <*> o .: "instructions"
      <*> o .: "acquire" <*> o .: "release"
instance FromJSON SegmentPolicy where
  parseJSON = withObject "segment observation policy" $ \o -> do
    closed ["id","trace_begin","trace_end"] o
    SegmentPolicy <$> o .: "id" <*> o .: "trace_begin" <*> o .: "trace_end"
instance FromJSON WatchInput where
  parseJSON = withObject "watch input" $ \o -> do
    closed ["path","sha256"] o
    WatchInput <$> (J.FileBinding <$> o .: "path" <*> o .: "sha256")
instance FromJSON PolicyFile where
  parseJSON = withObject "shared external observation policy" $ \o -> do
    closed ["schema","scope","capture_sha256","trace_sha256","contract_sha256","watch_inputs"
      ,"begin_trace_index","end_trace_index","initial","boundaries","segments"] o
    scope <- o .: "scope" :: Parser T.Text
    unless (scope == "shared-external-observation-policy") (fail "wrong observation policy scope")
    PolicyFile <$> o .: "schema" <*> o .: "capture_sha256" <*> o .: "trace_sha256" <*> o .: "contract_sha256"
      <*> o .: "watch_inputs" <*> o .: "begin_trace_index" <*> o .: "end_trace_index"
      <*> o .: "initial" <*> o .: "boundaries" <*> o .: "segments"

require :: Bool -> PolicyError -> Either PolicyError ()
require condition failure = unless condition (Left failure)
ascending :: Ord a => [a] -> Bool
ascending values = and (zipWith (<) values (drop 1 values))

standalonePolicy :: ObservationPolicy
standalonePolicy = Standalone
policyGlobalSites :: ObservationPolicy -> [K.Address]
policyGlobalSites Standalone = S.globalSourceSites
policyGlobalSites (Shared sites _ _) = sites
policyInitialOuterReturn :: ObservationPolicy -> Bool
policyInitialOuterReturn Standalone = True
policyInitialOuterReturn (Shared _ _ _) = False
policyRetiresPoll :: ObservationPolicy -> Bool
policyRetiresPoll Standalone = True
policyRetiresPoll (Shared _ _ _) = False
policyPollSites :: ObservationPolicy -> [K.Address]
policyPollSites Standalone = canonicalPollSites
policyPollSites (Shared _ sites _) = sites
policyExternalBefore :: ObservationPolicy -> W.SegmentId -> Either PolicyError [K.Address]
policyExternalBefore Standalone _ = Right []
policyExternalBefore (Shared _ _ segments) ident = case M.lookup ident segments of
  Just (SegmentObservation _ _ _ sites) -> Right sites
  Nothing -> Left PolicySegmentRejected
verifyPolicySegment :: ObservationPolicy -> W.Segment -> Either PolicyError ()
verifyPolicySegment Standalone _ = Right ()
verifyPolicySegment (Shared _ _ segments) segment = case M.lookup (W.segmentId segment) segments of
  Nothing -> Left PolicySegmentRejected
  Just (SegmentObservation start end stops _) -> require
    (W.segmentStart segment == start && W.segmentEnd segment == end && W.segmentStops segment == stops) PolicySegmentRejected

splitClock :: K.Count
splitClock = K.Count 8000000
policyExtraFetchRanges :: ObservationPolicy -> K.Count -> [W.FetchRange]
policyExtraFetchRanges Standalone _ = []
policyExtraFetchRanges (Shared _ _ _) at = map range
  ([(0x40cac,0x40d50),(0xd0cac,0xd0d50),(0x40d70,0x40d98),(0xd0d70,0xd0d98)
   ,(0x69cc6c,0x69cd58),(0xed468c,0xed485c),(0xed4a8c,0xed4a9c)
   ,(0x409ac,0x40b00),(0xd09ac,0xd0b00),(0x69cdf8,0x69cf84),(0x725a04,0x725b54)]
   ++ if at < splitClock then [(0x41ad4,0x41aec),(0x41b78,0x41b84)] else [])
  where range (low,high) = W.FetchRange (W.PhysicalBoundary low) (W.PhysicalBoundary high)

canonicalPollSites :: [K.Address]
canonicalPollSites = map K.Address [0xd0a44,0xd0a48,0xd0a4c]

ownerOrigin :: Owner -> Maybe J.TraceIndex
ownerOrigin owner = case owner of
  RoutePermanent -> Nothing
  DispatcherPermanent -> Nothing
  RouteCall origin -> Just origin
  RouteScheduledTarget origin -> Just origin
  DispatcherReturn origin -> Just origin
ownedSites :: [OwnedSite] -> Either PolicyError Owners
ownedSites sites = do
  require (ascending [site | OwnedSite site _ <- sites]) PolicyOwnersRejected
  forM_ sites $ \(OwnedSite (K.Address site) held) ->
    require (site .&. 3 == 0 && not (null held) && ascending held) PolicyOwnersRejected
  pure (M.fromList [(site,Set.fromList held) | OwnedSite site held <- sites])
ownerPairs :: Owners -> Set.Set (K.Address,Owner)
ownerPairs state = Set.fromList [(site,owner) | (site,held) <- M.toList state,owner <- Set.toList held]
advance :: Owners -> BoundaryPolicy -> Either PolicyError Owners
advance current (BoundaryPolicy index _ _ _ acquire release) = do
  added <- ownedSites acquire
  removed <- ownedSites release
  let held = ownerPairs current; additions = ownerPairs added; removals = ownerPairs removed
  require (Set.null (Set.intersection held additions) && removals `Set.isSubsetOf` held && Set.null (Set.intersection additions removals)
    && all ((==Just index) . ownerOrigin . snd) (Set.toList additions)) PolicyOwnersRejected
  pure (M.filter (not . Set.null) (M.unionWith Set.union added (M.mapWithKey
    (\site owners -> Set.difference owners (M.findWithDefault Set.empty site removed)) current)))

aggregate :: [K.Count] -> K.Count
aggregate counts = K.Count (sum [value | K.Count value <- counts])

sharedPolicy :: Host.HostPolicyBytes -> LS.LoaderJoinedArtifact -> J.SharedCapture -> [J.TraceHit] -> Either PolicyError ObservationPolicy
sharedPolicy admitted artifact capture trace = do
  PolicyFile schema captureDigest traceDigest contractDigest watches beginIndex endIndex initial boundaries declared <-
    either (const (Left PolicyDecodeRejected)) Right (eitherDecodeStrict' (Host.policyBytes admitted))
  let consumer = J.captureConsumer capture
      receipt = J.consumerEvidence consumer
      segments = W.receiptSegments receipt
      identity = J.captureIdentity capture
      watchBindings = [binding | WatchInput binding <- watches]
      expectedWatches = sortOn J.bindingPath [binding | epoch <- J.captureEpochs capture,J.WatchFile binding _ <- [J.epochEvidence epoch]]
  begin <- maybe (Left PolicyBindingRejected) Right (J.consumerBegin consumer)
  end <- maybe (Left PolicyBindingRejected) Right (J.consumerEnd consumer)
  require (schema == 2 && captureDigest == Host.policyCaptureDigest admitted
    && Host.policyObserverDigest admitted == J.identityExecutable identity
    && W.identityObserver (W.receiptIdentity receipt) == J.identityExecutable identity
    && traceDigest == J.bindingDigest (J.captureTrace capture)
    && [binding | binding <- J.captureArtifacts capture,J.bindingPath binding == J.artifactFilename J.JoinedContract]
      == [J.FileBinding (J.artifactFilename J.JoinedContract) contractDigest]
    && W.identityJoinedContract (W.receiptIdentity receipt) == contractDigest
    && watchBindings == expectedWatches && ascending (map J.bindingPath watchBindings)
    && beginIndex == J.endpointTraceIndex begin && endIndex == J.endpointTraceIndex end) PolicyBindingRejected
  let J.TraceIndex low = beginIndex; J.TraceIndex high = endIndex
      selected = take (fromIntegral (high-low)) (drop (fromIntegral low) trace)
  require (low > 0 && low <= high && toInteger high <= toInteger (length trace)
    && length boundaries == length selected) PolicyBoundaryRejected
  forM_ (zip3 [low..] selected boundaries) $ \(index,hit,BoundaryPolicy actual which pc at _ _) ->
    require (actual == J.TraceIndex index && which == J.hitCore hit && pc == J.hitPc hit && at == J.hitInstructions hit) PolicyBoundaryRejected
  require (length declared == length segments
    && map W.segmentId segments == map W.SegmentId (take (length segments) [0..])) PolicySegmentRejected
  initialOwners <- ownedSites initial
  require (all (maybe True (<beginIndex) . ownerOrigin) (concatMap Set.toList (M.elems initialOwners))) PolicyOwnersRejected
  (remaining,remainingHits,afterIndex,_,observations) <- foldM checkSegment
    (boundaries,selected,beginIndex,initialOwners,[]) (zip segments declared)
  require (null remaining && null remainingHits && afterIndex == endIndex) PolicySegmentRejected
  let (globals,polls) = providerSites artifact (J.captureRole capture)
  pure (Shared globals polls (M.fromList observations))
  where
    checkSegment (pending,hits,index,current,observations) (segment,SegmentPolicy ident first next) = do
      let stops = W.segmentStops segment
          size = length stops
          (actions,laterActions) = splitAt size pending
          (stoppedHits,laterHits) = splitAt size hits
          J.TraceIndex ordinal = index
          startClock = aggregate (W.segmentStart segment)
          endClock = aggregate (W.segmentEnd segment)
      require (ident == W.segmentId segment && first == index && next == J.TraceIndex (ordinal+fromIntegral size)
        && length actions == size && length stoppedHits == size
        && (startClock >= splitClock || endClock <= splitClock)) PolicySegmentRejected
      forM_ (zip stops stoppedHits) $ \(stop,hit) -> do
        context <- case [value | value <- W.segmentEndContexts segment,W.contextCore value == W.stopCore stop] of
          [value] -> Right value
          _ -> Left PolicySegmentRejected
        require (W.stopCore stop == J.hitCore hit && W.stopPc stop == J.hitPc hit
          && W.stopInstructions stop == J.hitInstructions hit && W.stopInstructions stop == endClock
          && matchesContext hit context && K.contextCounts (W.contextMachine context) == W.segmentEnd segment) PolicyBoundaryRejected
      successor <- foldM advance current actions
      let observation = SegmentObservation (W.segmentStart segment) (W.segmentEnd segment) stops (M.keys current)
      pure $! forceObservation observation `seq` (laterActions,laterHits,next,successor,(ident,observation):observations)

-- Forces every field so the retained policy does not keep its source segment or decoded snapshot alive.
forceObservation :: SegmentObservation -> ()
forceObservation (SegmentObservation start end stops external) =
  foldr seq () start `seq` foldr seq () end `seq` foldr seq () stops `seq` foldr seq () external

matchesContext :: J.TraceHit -> W.Context -> Bool
matchesContext hit context =
  let machine = W.contextMachine context
  in J.hitCore hit == W.contextCore context && J.hitPc hit == K.contextPc machine
    && J.registerWords (J.hitRegisters hit) == K.contextRegisters machine
    && J.hitSp hit == K.contextSp machine && J.hitLr hit == K.contextLr machine && J.hitCpsr hit == K.contextCpsr machine
    && toInteger (aggregateValue (J.hitInstructions hit)) == sum [toInteger value | K.Count value <- K.contextCounts machine]
  where aggregateValue (K.Count value) = value

-- The joined role replaces the canonical loader A completion and loader B poll/initializer globals with the admitted native sites.
providerSites :: LS.LoaderJoinedArtifact -> J.CaptureRole -> ([K.Address],[K.Address])
providerSites _ J.RetainedCapture = (S.globalSourceSites,canonicalPollSites)
providerSites artifact J.JoinedCapture =
  let primary = LS.loaderSites artifact LS.LoaderA
      peer = LS.loaderSites artifact LS.LoaderB
      native (L.CodeAddress value) = K.Address value
      completion = map (\site -> native (site primary)) [LS.completionStore,LS.completionBeforeDsb,LS.completionAfterDsb]
      poll = map (\site -> native (site peer)) [LS.peerPoll,LS.peerPollPostload,LS.peerPollBranch]
      initializer = map (\site -> native (site peer)) [LS.initializerCall,LS.initializerEntry,LS.initializerReturn]
      replaced = map K.Address [0x40a08,0x40a0c,0x40a10,0xd0a44,0xd0a48,0xd0a4c,0xd0aac,0xd0430,0xd0ab0]
  in (Set.toAscList (Set.fromList (filter (`notElem` replaced) S.globalSourceSites ++ completion ++ poll ++ initializer)),poll)
