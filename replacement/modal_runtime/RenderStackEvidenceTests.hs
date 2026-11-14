{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
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
  , "retained_entries" J..= retained,"allocated_entries" J..= (65536 :: Integer),"logical_transitions" J..= logical ]

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
  check "complete owned invocation with a failed historical estimate is accepted" (accepts (singleton invocation))
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
  let spanInvocation :: Word32 -> Maybe Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> J.Value
      spanInvocation ident parent entry terminal spanFirst spanLast = foldr (uncurry set) invocation
        [ ("id",number ident),("parent",maybe J.Null number parent)
        , ("entry_boundary",boundary entry),("terminal_boundary",boundary terminal)
        , ("transition_span",J.object ["first" J..= spanFirst,"last" J..= spanLast])
        , ("transition_count",number 1),("transitions",J.toJSON [sample spanFirst transitionContext]) ]
      spanReport :: [J.Value] -> Int -> J.Value
      spanReport values roots = foldr (uncurry set) (report values)
        [ ("transition_storage",storage (toInteger (length values)) (toInteger (length values)))
        , ("performance",J.object ["active_instructions" J..= (10 :: Word32)
            ,"invocation_count" J..= length values,"hook_transitions" J..= (2 * roots)]) ]
  check "ordered disjoint root transition spans are accepted"
    (accepts (spanReport [spanInvocation 1 Nothing 2 8 2 8,spanInvocation 2 Nothing 9 10 9 10] 2))
  check "root transition spans cannot overlap across sequential boundaries"
    (not (accepts (spanReport [spanInvocation 1 Nothing 2 8 2 8,spanInvocation 2 Nothing 9 10 7 8] 2)))
  check "nested transition span is contained in its parent"
    (accepts (spanReport [spanInvocation 1 Nothing 2 10 2 8,spanInvocation 2 (Just 1) 3 9 3 8] 1))
  check "nested transition span cannot leave its parent"
    (not (accepts (spanReport [spanInvocation 1 Nothing 2 10 2 5,spanInvocation 2 (Just 1) 3 9 3 8] 1)))
  check "reversed disjoint root ordinal spans refuse"
    (not (accepts (spanReport [spanInvocation 1 Nothing 2 8 4 10,spanInvocation 2 Nothing 9 10 2 3] 2)))
  check "nested invocation cannot change core" (not (accepts (nestedReport [invocation,set "core" (number 1) nested])))
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
  check "allocated capacity may equal retained storage"
    (accepts (set "transition_storage" (set "allocated_entries" (number 1) (storage 1 7)) pairReport))
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
        , withRuns [] 0
        , withRuns [sample 3 transitionContext] 1
        , set "version" (number 1) pairReport
        , J.Object (case pairReport of J.Object o -> KM.delete "transition_encoding" o; _ -> KM.empty)
        , set "transition_encoding" (J.String "flat") pairReport
        , set "transition_storage" (storage 2 7) pairReport
        , set "transition_storage" (storage 1 6) pairReport
        , set "transition_storage" (set "allocated_entries" (number 0) (storage 1 7)) pairReport
        , set "transition_storage" (set "allocated_entries" (number 65537) (storage 1 7)) pairReport
        , set "transition_storage" (set "allocated_entries" (J.toJSON (-1 :: Integer)) (storage 1 7)) pairReport
        , set "transition_storage" (set "allocated_entries" J.Null (storage 1 7)) pairReport
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
  let writes = [J.object ["range" J..= ([start,end] :: [Word32])
        ,"kinds" J..= (["cpu-write","host-write","model-bus-write"] :: [T.Text])]
        | (start,end) <- [(0x60614,0x60618),(0x5c3ac,0x5c3d4)]]
      fetches = J.object ["range" J..= ([0x1100000,0x1200000] :: [Word32]),"kinds" J..= (["cpu-fetch"] :: [T.Text])]
      watch = J.object ["violation" J..= J.Null,"enabled" J..= True,"views" J..= ("fixed" :: T.Text),"classes" J..= (writes ++ [fetches])]
      permitLifetime = J.object ["phase" J..= ("ended" :: T.Text),"failure" J..= J.Null,"publication_count" J..= (228 :: Word32)]
      permit = J.object
        ["enabled" J..= False,"physical_fetch_coverage" J..= True,"failure" J..= J.Null,"lifetime" J..= permitLifetime]
      lifetime = J.object
        ["task_identity_verified" J..= True,"whole_installed_lifetime_coverage" J..= True,"failures" J..= ([] :: [J.Value])
        ,"observation_error" J..= J.Null,"permit_end_error" J..= J.Null,"watch_final_error" J..= J.Null
        ,"watch_final_summary" J..= watch,"permit_end" J..= permit
        ,"request" J..= J.object ["lifetime" J..= J.object ["bank_variant" J..= ("modal75,menu-engine,render" :: T.Text)]]]
  check "completed lifetime with descriptor and backlink watches and no task 33 bottom guard is accepted"
    (checkLifetimeCoverage lifetime == Right ())
  forM_ [ set "whole_installed_lifetime_coverage" (J.Bool False) lifetime
        , set "failures" (J.toJSON [J.String "watch"]) lifetime
        , set "watch_final_summary" (set "violation" (J.String "write-restored") watch) lifetime
        , set "watch_final_summary" (set "enabled" (J.Bool False) watch) lifetime
        , set "watch_final_summary" (set "views" (J.String "observe") watch) lifetime
        , set "watch_final_summary" (set "classes" (J.toJSON (take 1 writes ++ [fetches])) watch) lifetime
        , set "watch_final_summary" (set "classes" (J.toJSON [fetches]) watch) lifetime
        , set "permit_end" J.Null lifetime
        , set "permit_end" (set "physical_fetch_coverage" (J.Bool False) permit) lifetime
        , set "permit_end" (set "enabled" (J.Bool True) permit) lifetime
        , set "permit_end" (set "failure" (J.String "refused") permit) lifetime
        , set "permit_end" (set "lifetime" (set "phase" (J.String "installed") permitLifetime) permit) lifetime
        , set "permit_end" (set "lifetime" (set "failure" (J.String "refused") permitLifetime) permit) lifetime
        , set "observation_error" (J.String "failed") lifetime ] $ \invalid ->
    check "missing lifetime coverage, attempted writes, descriptor or backlink watches refuse" (isLeft (checkLifetimeCoverage invalid))
