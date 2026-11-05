module Main (main) where

import Control.Monad (unless)
import Data.List (sort)
import Data.Word (Word64)
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.Evidence as E
import qualified Replacement.StartupConsumers.ObservationPolicy as P
import qualified Replacement.StartupConsumers.Wire as W
import System.Exit (exitFailure)

main :: IO ()
main = do
  assert "standalone exact policy and poll retirement" (check baseline == Right ())
  assert "standalone wrapper preserves admission" (E.checkFetchPolicies baseline == Right ())
  assert "standalone initial caller return is required" (check (firstSites (filter (/= K.Address 0x69ce00))) == Left E.WatchRejected)
  assert "standalone original global remains required" (check (firstSites (filter (/= K.Address 0x40a08))) == Left E.WatchRejected)
  assert "producer supersets are refused" (check (firstSites (sort . (K.Address 0x1000:))) == Left E.WatchRejected)
  assert "poll cannot retire before initializer call" (check (firstSites (filter (`notElem` polls))) == Left E.WatchRejected)
  assert "standalone poll retires at initializer call" (check (baseline {W.receiptSegments = [first,second {W.segmentArmedSites = initialSites}]}) == Left E.WatchRejected)
  assert "complete fetch policy is admitted" (E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy baseline == Right ())
  assert "complete fetch range cannot be omitted" (E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy
    (baseline {W.receiptSegments = [first {W.segmentFetchRanges = drop 1 completeRanges},second]}) == Left E.WatchRejected)
  assert "complete fetch classes cannot be narrowed" (E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy
    (baseline {W.receiptSegments = [first {W.segmentWatch = completeWatch {K.watchClasses = drop 1 (K.watchClasses completeWatch)}},second]}) == Left E.WatchRejected)
  assert "declared armed sites must ascend" (E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy
    (baseline {W.receiptSegments = [first {W.segmentArmedSites = reverse initialSites},second]}) == Left E.WatchRejected)
  assert "segments before the outer return keep complete fetch scope" (E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy
    (baseline {W.receiptSegments = [first {W.segmentFetchScope = W.ScopedFetch},second]}) == Left E.WatchRejected)
  assert "receipt recorded on the 16 KiB catalog ROM is admitted" (check (onRom 0x80004000) == Right ()
    && E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy (onRom 0x80004000) == Right ())
  assert "unknown ROM extent is refused" (check (onRom 0x80008000) == Left E.WatchRejected)
  assert "segments share one ROM backing" (check mixed == Left E.WatchRejected
    && E.checkDeclaredFetchPolicyWithPolicy P.standalonePolicy mixed == Left E.WatchRejected)
  where
    onRom end = baseline {W.receiptSegments = map (romSegment end) [first,second]}
    mixed = baseline {W.receiptSegments = [romSegment 0x80004000 first,second]}
    romSegment end s = s {W.segmentWatch = completeWatchTo end,W.segmentFetchRanges = completeRangesTo end
      ,W.segmentFetchAttribution = W.FetchAttribution [K.CpuRead,K.CpuWrite] (completeRangesTo end)}
    check = E.checkFetchPoliciesWithPolicy P.standalonePolicy
    firstSites change = baseline {W.receiptSegments = [first {W.segmentArmedSites = change initialSites},second]}
    assert name condition = unless condition (putStrLn ("FAIL: " ++ name) >> exitFailure)

-- Synthetic contexts exercise policy admission, not source or raw-fetch qualification.
context :: K.Address -> [K.Count] -> W.Context
context pc counts = W.Context K.Primary (K.Context pc (replicate 13 (K.WordValue 0))
  (K.Address 0x40063450) (K.Address 0x69ce00) (K.WordValue 0x13) counts
  (replicate 2 (replicate 2 (K.WordValue 0))) [K.WordValue 0x4003,K.WordValue 0x5003])
outer, initializer, returned :: W.Context
outer = context (K.Address 0x69cdf8) [K.Count 0,K.Count 0]
initializer = context (K.Address 0xd0aac) [K.Count 1,K.Count 0]
returned = context (K.Address 0x69ce00) [K.Count 2,K.Count 0]
probe :: W.ProbeId -> W.ProbeKind -> W.Context -> W.Probe
probe ident kind state = W.Probe ident kind Nothing Nothing state (K.WordValue 0xe1a00000) []
polls :: [K.Address]
polls = map K.Address [0xd0a44,0xd0a48,0xd0a4c]
initialSites :: [K.Address]
initialSites = sort (map K.Address
  [0x69cdf8,0x725a04,0x42634,0x40a08,0x40a0c,0x40a10,0xd0a44,0xd0a48,0xd0a4c
  ,0xd0aac,0xd0430,0xd0ab0,0x7235d8,0x7235f4,0x7235f8,0x7235fc,0x723600,0x723604
  ,0x723608,0x725dd8,0x725c6c,0x723470,0x724cdc,0x72503c,0x725318,0x69ce00])
completeRanges :: [W.FetchRange]
completeRanges = completeRangesTo 0x80104000
completeRangesTo :: Word64 -> [W.FetchRange]
completeRangesTo romEnd = [W.FetchRange (W.PhysicalBoundary low) (W.PhysicalBoundary high)
  | (low,high) <- [(0,0x20000000),(0x3fff0000,0x3fff1000),(0x80000000,romEnd),(0x90000000,0x90020000),(0xfff00000,0xfff01000)]]
completeWatch :: K.Watch
completeWatch = completeWatchTo 0x80104000
completeWatchTo :: Word64 -> K.Watch
completeWatchTo romEnd = K.Watch True ranges [K.WatchClass range [K.CpuFetch] | range <- ranges] "fixed" [] Nothing
  where ranges = [[0,0x20000000],[0x3fff0000,0x3fff1000],[0x80000000,romEnd],[0x90000000,0x90020000],[0xfff00000,0xfff01000]]
segment :: W.SegmentId -> W.Context -> W.Context -> W.ProbeRange -> [K.Address] -> W.Segment
segment ident begin end probes sites = W.Segment ident (K.contextCounts (W.contextMachine begin))
  (K.contextCounts (W.contextMachine end)) [begin] [end]
  [W.TraceStop K.Primary (K.contextPc (W.contextMachine end)) (K.Count (sum [value | K.Count value <- K.contextCounts (W.contextMachine end)]))]
  completeWatch Nothing W.CompleteFetch completeRanges sites
  (W.FetchAttribution [K.CpuRead,K.CpuWrite] completeRanges) [] probes
first, second :: W.Segment
first = segment (W.SegmentId 0) outer initializer (W.ProbeRange (W.ProbeId 0) (W.ProbeId 2)) initialSites
second = segment (W.SegmentId 1) initializer returned (W.ProbeRange (W.ProbeId 2) (W.ProbeId 3)) (filter (`notElem` polls) initialSites)
baseline :: W.Receipt
baseline = W.Receipt
  {W.receiptSchema = 1,W.receiptRole = W.Retained,W.receiptOutcome = W.Observed,W.receiptError = Nothing,W.receiptFailure = Nothing
  ,W.receiptIdentity = W.Identity digest digest digest digest digest digest digest [] digest digest
  ,W.receiptCheckpoint = digest,W.receiptCheckpointReused = False,W.receiptInstructions = K.Count 2
  ,W.receiptLimits = W.Limits (K.Count 2) (K.Count 256) (K.Count 4096) (K.Count 0) (K.Count 3)
    (K.Count 0) (K.Count 0) (K.Count 0) (K.Count 0) (K.Count 0) (K.Count 2) (K.Count 0)
  ,W.receiptProbes = [probe (W.ProbeId 0) W.OuterEntry outer,probe (W.ProbeId 1) W.InitializerCall initializer,probe (W.ProbeId 2) W.OuterReturn returned]
  ,W.receiptInvocations = [],W.receiptResources = [],W.receiptSegments = [first,second],W.receiptCensus = []
  ,W.receiptFetches = [],W.receiptPayloads = []}
  where digest = K.Digest (replicate 64 '0')
