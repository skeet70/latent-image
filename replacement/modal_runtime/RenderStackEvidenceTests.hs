{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as ELF
import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Text as T
import Data.Word (Word32)
import Replacement.ModalRuntime.RenderStackEvidence

check :: String -> Bool -> IO ()
check label good = unless good (fail label)

set :: J.Key -> J.Value -> J.Value -> J.Value
set key value (J.Object fields) = J.Object (KM.insert key value fields)
set _ _ value = value

number :: Word32 -> J.Value
number = J.toJSON

boundary :: Word32 -> J.Value
boundary at = J.object ["instructions" J..= at,"tick" J..= at]

identity :: J.Value
identity = J.object
  [ "kernel" J..= ("primary" :: T.Text),"task" J..= (33 :: Word32)
  , "node" J..= (0x4006060c :: Word32),"creation" J..= (0x4005c3ac :: Word32)
  , "descriptor" J..= ([2,0,0x1109130,8,4096,0xa4ce00,0,0,33,1] :: [Word32]) ]

source :: J.Value
source = J.object ["kind" J..= ("retained" :: T.Text),"start" J..= (0x1100000 :: Word32)
  ,"path" J..= ("image6.bin" :: T.Text),"sha256" J..= T.replicate 64 "0"]

request :: J.Value
request = J.object
  [ "role" J..= ("owned" :: T.Text),"sources" J..= [source]
  , "entries" J..= [J.object ["kind" J..= kind,"frame_depth" J..= frame]
      | (kind,frame) <- [("tree",556),("picker",636),("pane",452)] :: [(T.Text,Word32)]]
  , "returns" J..= ([0x1158184] :: [Word32]) ]

minimumContext :: J.Value
minimumContext = J.object
  [ "coordinate" J..= J.object ["kind" J..= ("observation" :: T.Text),"ordinal" J..= (7 :: Word32)]
  , "core" J..= (0 :: Word32),"cpsr" J..= (0x60000053 :: Word32),"context_depth" J..= (0 :: Word32)
  , "current" J..= identity,"owner" J..= identity,"scheduler" J..= False,"mode_source" J..= J.Null ]

transitionContext :: J.Value
transitionContext = J.object
  [ "pc" J..= (0x119f758 :: Word32),"cpsr" J..= (0x60000053 :: Word32)
  , "context_depth" J..= (0 :: Word32),"current_node" J..= (0x4006060c :: Word32)
  , "owner_node" J..= (0x4006060c :: Word32),"scheduler" J..= False,"mode_source" J..= J.Null ]

sample :: Word32 -> J.Value -> J.Value
sample ordinal context = J.object ["kind" J..= ("sample" :: T.Text),"ordinal" J..= ordinal,"context" J..= context]

pair :: J.Value
pair = J.object
  [ "kind" J..= ("pair" :: T.Text),"first" J..= sample 2 transitionContext
  , "second" J..= sample 3 (set "pc" (number 0x119f75c) transitionContext)
  , "stride" J..= (2 :: Word32),"pairs" J..= (3 :: Word32),"trailing_first" J..= True ]

storage :: Integer -> Integer -> J.Value
storage retained logical = J.object
  [ "per_invocation_limit" J..= (4096 :: Integer),"aggregate_limit" J..= (65536 :: Integer)
  , "retained_entries" J..= retained,"logical_transitions" J..= logical ]

invocation :: J.Value
invocation = J.object
  [ "id" J..= (1 :: Word32),"parent" J..= J.Null,"kind" J..= ("pane" :: T.Text),"core" J..= (0 :: Word32)
  , "task_identity" J..= identity,"entry_pc" J..= (0x11662a0 :: Word32),"entry_sp" J..= (0xa4dd80 :: Word32)
  , "entry_lr" J..= (0x1158184 :: Word32),"entry_mode" J..= (19 :: Word32)
  , "entry_boundary" J..= boundary 2,"terminal_boundary" J..= boundary 9
  , "exit_sp" J..= (0xa4dd80 :: Word32),"min_sp" J..= (0xa4d66c :: Word32),"deepest_pc" J..= (0x119f758 :: Word32)
  , "deepest_lr" J..= (0x11a03a0 :: Word32)
  , "minimum_context" J..= minimumContext,"terminal" J..= ("owned-return" :: T.Text)
  , "frame_depth" J..= (452 :: Word32),"allowance" J..= (1024 :: Word32),"within_bound" J..= False
  , "transition_span" J..= J.object ["first" J..= (2 :: Word32),"last" J..= (8 :: Word32)]
  , "transition_count" J..= (7 :: Word32),"transitions" J..= [pair] ]

report :: [J.Value] -> J.Value
report invocations = J.object
  [ "version" J..= (2 :: Word32),"diagnostic_only" J..= True,"scope" J..= ("installed-before-unload" :: T.Text)
  , "whole_route_stack_minimum_proven" J..= False,"coverage" J..= ("continuous" :: T.Text),"failure" J..= J.Null
  , "active_invocations" J..= (0 :: Word32),"incomplete_intervals" J..= False,"finished" J..= True
  , "role" J..= ("owned" :: T.Text),"start" J..= boundary 1,"end" J..= boundary 10,"sources" J..= [source]
  , "retained_service_coverage" J..= ("partial-observed-source-transitions" :: T.Text),"retained_service_spans_informational" J..= True
  , "transition_encoding" J..= ("pair-stride-v1" :: T.Text)
  , "transition_storage" J..= storage (toInteger (length invocations)) (7 * toInteger (length invocations))
  , "invocations" J..= invocations,"performance" J..= J.object
      ["active_instructions" J..= (10 :: Word32),"invocation_count" J..= length invocations,"hook_transitions" J..= (2 * length invocations)] ]

main :: IO ()
main = do
  let parse = parseRenderStackEvidence OwnedStack request
      accepts = not . isLeft . parse
      singleton = report . pure
  evidence <- either fail pure (parse (singleton invocation))
  let verdict = allocationVerdict evidence
  check "allocation acceptance preserves the failed historical estimate"
    (J.parseEither (J.withObject "verdict" (\o -> (,) <$> o J..: "allocation_verdict" <*> o J..: "elf_plus_1024_estimate_violations"))
      (allocationVerdictJson verdict) == Right ("pass" :: T.Text,1 :: Int))
  check "finite route with no entries has no invented demand" (accepts (report []))
  forM_ [ set "min_sp" (number 0xa4ce3c) invocation
        , set "min_sp" (number 0xa4de04) invocation
        , set "min_sp" (number 0xa4d66d) invocation
        , set "exit_sp" (number 0xa4dd7c) invocation
        , set "task_identity" (set "node" (number 0x40060644) identity) invocation
        , set "minimum_context" J.Null invocation
        , set "minimum_context" (set "owner" J.Null minimumContext) invocation
        , set "minimum_context" (set "core" (number 1) minimumContext) invocation
        , set "minimum_context" (set "cpsr" (number 0x12) minimumContext) invocation
        , set "minimum_context" (set "coordinate" (J.object ["kind" J..= ("observation" :: T.Text),"ordinal" J..= (21 :: Word32)]) minimumContext) invocation
        , set "terminal_boundary" (boundary 11) invocation
        , set "terminal" (J.String "stock-return") invocation
        , set "parent" (number 2) invocation
        , set "within_bound" (J.Bool True) invocation
        , set "allowance" (number 1436) invocation
        , set "frame_depth" (number 500) invocation ] $ \invalid ->
    check "invalid allocation, owner, context, return or estimate evidence refuses" (not (accepts (singleton invalid)))
  check "minimum exactly at existing guard end is accepted" (accepts (singleton (set "min_sp" (number 0xa4ce40) invocation)))
  let fallback = set "terminal" (J.String "fallback-handoff") (set "exit_sp" (number 0xa4dd5c) invocation)
  check "fallback prefix has a separate restored 36-byte handoff" (accepts (singleton fallback))
  check "fallback cannot use owned-return restoration" (not (accepts (singleton (set "exit_sp" (number 0xa4dd80) fallback))))
  let nested = set "id" (number 2) (set "parent" (number 1)
        (set "entry_boundary" (boundary 3) (set "terminal_boundary" (boundary 9) invocation)))
      nestedReport values = set "performance" (J.object
        ["active_instructions" J..= (10 :: Word32),"invocation_count" J..= (2 :: Word32),"hook_transitions" J..= (2 :: Word32)]) (report values)
  check "nested invocation is covered by enclosing minimum and interval" (accepts (nestedReport [invocation,nested]))
  check "nested minimum cannot escape enclosing accounting"
    (not (accepts (nestedReport [set "min_sp" (number 0xa4d700) invocation,nested])))
  check "overlapping invocation cannot omit its enclosing parent"
    (not (accepts (report [invocation,set "parent" J.Null nested])))
  forM_ [ set "coverage" (J.String "refused") (singleton invocation)
        , set "finished" (J.Bool False) (singleton invocation)
        , set "active_invocations" (number 1) (singleton invocation)
        , set "incomplete_intervals" (J.Bool True) (singleton invocation)
        , set "sources" (J.toJSON ([] :: [J.Value])) (singleton invocation)
        , set "performance" (J.object ["active_instructions" J..= (20 :: Word32),"invocation_count" J..= (0 :: Word32),"hook_transitions" J..= (2 :: Word32)]) (singleton invocation)
        , report [invocation,invocation] ] $ \invalid ->
    check "missing, duplicate or incomplete continuous coverage refuses" (not (accepts invalid))
  let withRuns :: [J.Value] -> Integer -> J.Value
      withRuns runs count = set "transition_storage" (storage (toInteger (length runs)) count)
        (singleton (set "transitions" (J.toJSON runs) (set "transition_count" (J.toJSON count) invocation)))
      pairReport = withRuns [pair] 7
      alteredContext key value = withRuns [set "first" (sample 2 (set key value transitionContext)) pair] 7
  check "pair-stride run with trailing first preserves seven logical transitions" (accepts pairReport)
  forM_ [ ("pc",number 0x119f760),("cpsr",number 0x600000d3),("context_depth",number 1)
        , ("current_node",number 0x40060644),("owner_node",J.Null),("scheduler",J.Bool True)
        , ("mode_source",J.object ["pc" J..= (0x119f754 :: Word32),"word" J..= (0xe121f000 :: Word32)]) ] $ \(key,value) ->
    check "encoded contexts retain every field rather than discarding foreign or mode transitions"
      (accepts (alteredContext key value) && parse (alteredContext key value) /= parse pairReport)
  let late = sample 8 (set "current_node" (number 0x40060644) transitionContext)
      six = set "trailing_first" (J.Bool False) pair
  check "late differing transition remains distinct after repeated pairs"
    (accepts (withRuns [six,late] 7) && parse (withRuns [six,late] 7) /= parse pairReport)
  forM_ [ withRuns [set "pairs" (number 0) pair] 1
        , withRuns [set "pairs" (number 1) (set "trailing_first" (J.Bool False) pair)] 2
        , withRuns [set "stride" (number 1) pair] 7
        , withRuns [set "stride" (J.toJSON (18446744073709551615 :: Integer)) pair] 7
        , withRuns [set "pairs" (J.toJSON (18446744073709551615 :: Integer)) pair] 7
        , withRuns [set "first" (sample 3 transitionContext) pair] 7
        , withRuns [set "second" J.Null pair] 7
        , withRuns [set "first" (sample 2 (J.Object (KM.delete "mode_source" (case transitionContext of J.Object o -> o; _ -> KM.empty)))) pair] 7
        , withRuns [pair,sample 8 transitionContext] 8
        , withRuns [pair] 6
        , withRuns [] 7
        , set "version" (number 1) pairReport
        , J.Object (case pairReport of J.Object o -> KM.delete "transition_encoding" o; _ -> KM.empty)
        , set "transition_encoding" (J.String "flat") pairReport
        , set "transition_storage" (storage 2 7) pairReport
        , set "transition_storage" (storage 1 6) pairReport
        , set "transition_storage" (set "per_invocation_limit" (number 4097) (storage 1 7)) pairReport
        , set "transition_storage" (set "aggregate_limit" (number 65537) (storage 1 7)) pairReport
        , singleton (set "transition_span" (J.object ["first" J..= (0 :: Word32),"last" J..= (8 :: Word32)]) invocation)
        , singleton (set "transition_span" (J.object ["first" J..= (2 :: Word32),"last" J..= (11 :: Word32)]) invocation)
        , withRuns (replicate 4097 (sample 2 transitionContext)) 4097
        ] $ \invalid ->
    check "malformed truncated unordered overflowed or miscounted transition evidence refuses" (not (accepts invalid))
  let storageInvocation index rows =
        let first = 1 + index * 4096
            last_ = first + fromIntegral rows - 1
        in foldr (uncurry set) invocation
          [ ("id",number (index+1)),("entry_boundary",boundary first),("terminal_boundary",boundary last_)
          , ("transition_span",J.object ["first" J..= first,"last" J..= last_])
          , ("transition_count",J.toJSON rows),("transitions",J.toJSON [sample ordinal transitionContext | ordinal <- [first..last_]]) ]
      storageReport :: [Int] -> J.Value
      storageReport counts =
        let values = zipWith storageInvocation [0..] counts
            total = sum counts
        in foldr (uncurry set) (report values)
          [ ("start",boundary 0),("end",boundary 70000),("transition_storage",storage (toInteger total) (toInteger total))
          , ("performance",J.object ["active_instructions" J..= (70000 :: Word32)
              ,"invocation_count" J..= length values,"hook_transitions" J..= (2 * length values)]) ]
  check "exact per-invocation retained storage limit is accepted" (accepts (storageReport [4096 :: Int]))
  check "exact aggregate retained storage limit is accepted" (accepts (storageReport (replicate 16 4096)))
  check "aggregate retained storage cannot exceed its hard limit" (not (accepts (storageReport (replicate 16 4096 ++ [1]))))
  let longInvocation = foldr (uncurry set) invocation
        [ ("terminal_boundary",boundary 8193)
        , ("transition_span",J.object ["first" J..= (2 :: Word32),"last" J..= (8193 :: Word32)])
        , ("transition_count",number 8192)
        , ("transitions",J.toJSON [set "pairs" (number 4096) (set "trailing_first" (J.Bool False) pair)]) ]
      longReport = foldr (uncurry set) (singleton longInvocation)
        [ ("end",boundary 10000),("transition_storage",storage 1 8192)
        , ("performance",J.object ["active_instructions" J..= (10000 :: Word32)
            ,"invocation_count" J..= (1 :: Word32),"hook_transitions" J..= (2 :: Word32)]) ]
  check "logical rows can exceed retained entry limit without expanded storage" (accepts longReport)
  let frames = [(TreeStack,FrameBytes 556),(PickerStack,FrameBytes 636),(PaneStack,FrameBytes 452)]
      witness = EntryWitness PaneStack (InstructionCounter 2) (StackAddress 0xa4dd80) (CodeAddress 0x1158184)
  check "entry witness binds exact recorded instruction boundary" (checkEntryWitnesses frames [witness] evidence == Right ())
  forM_ [[],[witness,witness],[EntryWitness PaneStack (InstructionCounter 3) (StackAddress 0xa4dd80) (CodeAddress 0x1158184)]] $ \witnesses ->
    check "missing, extra or stale trace entries refuse" (isLeft (checkEntryWitnesses frames witnesses evidence))
  check "linked ELF depth cannot be replaced by fitted runtime demand"
    (isLeft (checkEntryWitnesses [(PaneStack,FrameBytes 1812)] [witness] evidence))
  let ownedSource = set "kind" (J.String "owned")
        (set "sha256" (J.String "379bec29dccd0a93c94826144d7ef6e42fab64ef195a3b8313a16926f66f388f") source)
      sourceRequest = set "sources" (J.toJSON [ownedSource]) request
      sourceReport = set "sources" (J.toJSON [ownedSource]) (singleton invocation)
      elf = ELF.Elf 0 [ELF.Section ".text" 1 6 0x1100000 0 4] [] [] (B.pack [0x1e,0xff,0x2f,0xe1])
  sourceEvidence <- either fail pure (parseRenderStackEvidence OwnedStack sourceRequest sourceReport)
  check "owned executable identity binds to linked ELF section bytes" (checkOwnedSources elf sourceEvidence == Right ())
  check "changed executable bytes cannot reuse old source receipt"
    (isLeft (checkOwnedSources (elf {ELF.elfBytes=B.replicate 4 0}) sourceEvidence))
  check "extra executable section cannot escape source binding"
    (isLeft (checkOwnedSources (elf {ELF.elfSections=ELF.elfSections elf ++ [ELF.Section ".extra" 1 6 0x1100010 0 4]}) sourceEvidence))
  let guard = J.object
        ["node" J..= (0x4006060c :: Word32),"creation" J..= (0x4005c3ac :: Word32),"entry" J..= (0x1109130 :: Word32)
        ,"base" J..= (0xa4ce00 :: Word32),"size" J..= (4096 :: Word32),"guard_bytes" J..= (64 :: Word32)
        ,"baseline" J..= replicate 64 (33 :: Word32)]
      classes = [J.object ["range" J..= ([start,end] :: [Word32])
        ,"kinds" J..= (["cpu-write","host-write","model-bus-write"] :: [T.Text])]
        | (start,end) <- [(0x60614,0x60618),(0x5c3ac,0x5c3d4),(0xa4ce00,0xa4ce40)]]
      watch = J.object ["violation" J..= J.Null,"enabled" J..= True,"views" J..= ("fixed" :: T.Text),"classes" J..= classes]
      completedGuard :: J.Value -> J.Value
      completedGuard guarded = foldr (uncurry set) guarded
        [("scope",J.String "diagnostic_only_bottom_guard_write_attempts")
        ,("priority",number 8),("task_id",number 33),("created",number 1)]
      permit :: J.Value -> J.Value
      permit guarded = J.object
        ["enabled" J..= False,"physical_fetch_coverage" J..= True,"failure" J..= J.Null
        ,"lifetime" J..= J.object ["phase" J..= ("ended" :: T.Text),"failure" J..= J.Null,"task33_stack_guard" J..= guarded]]
      lifetime :: J.Value -> J.Value
      lifetime guarded = J.object
        ["task_identity_verified" J..= False,"whole_installed_lifetime_coverage" J..= True,"failures" J..= ([] :: [J.Value])
        ,"observation_error" J..= J.Null,"permit_end_error" J..= J.Null,"watch_final_error" J..= J.Null
        ,"watch_final_summary" J..= watch,"permit_end" J..= permit (completedGuard guarded)
        ,"request" J..= J.object ["lifetime" J..= J.object ["task33_stack_guard" J..= guarded]]]
  check "completed Task33 guard verifies stock without unrelated modal task entries" (checkLifetimeGuard (lifetime guard) == Right ())
  forM_ [ set "whole_installed_lifetime_coverage" (J.Bool False) (lifetime guard)
        , set "watch_final_summary" (set "violation" (J.String "write-restored") watch) (lifetime guard)
        , set "watch_final_summary" (set "enabled" (J.Bool False) watch) (lifetime guard)
        , set "watch_final_summary" (set "views" (J.String "observe") watch) (lifetime guard)
        , set "watch_final_summary" (set "classes" (J.toJSON (take 2 classes)) watch) (lifetime guard)
        , lifetime (set "guard_bytes" (number 32) guard),lifetime (set "size" (number 8192) guard)
        , set "permit_end" J.Null (lifetime guard)
        , set "permit_end" (permit J.Null) (lifetime guard)
        , set "permit_end" (permit (set "task_id" (number 34) (completedGuard guard))) (lifetime guard)
        , set "permit_end" (permit (set "priority" (number 9) (completedGuard guard))) (lifetime guard)
        , set "permit_end" (permit (set "created" (number 0) (completedGuard guard))) (lifetime guard)
        , set "permit_end" (permit (set "baseline" (J.toJSON (replicate 64 (34 :: Word32))) (completedGuard guard))) (lifetime guard)
        , set "permit_end" (set "physical_fetch_coverage" (J.Bool False) (permit (completedGuard guard))) (lifetime guard)
        , set "permit_end" (set "enabled" (J.Bool True) (permit (completedGuard guard))) (lifetime guard)
        , set "permit_end" (set "lifetime" (J.object ["phase" J..= ("installed" :: T.Text),"failure" J..= J.Null
            ,"task33_stack_guard" J..= completedGuard guard]) (permit (completedGuard guard))) (lifetime guard)
        , set "observation_error" (J.String "failed") (lifetime guard)
        , lifetime (set "baseline" (J.toJSON ([] :: [Word32])) guard) ] $ \invalid ->
    check "missing lifetime coverage, attempted writes or repinned backing refuse" (isLeft (checkLifetimeGuard invalid))
