{-# LANGUAGE GHC2021 #-}
module OperationPredicatesScenarios
  ( PredicateScenario(..), PredicateReentry(..), operationPredicatesScenarios, operationPredicateReentries ) where

import qualified Data.ByteString as B
import Data.Word (Word32)
import Scenarios (Patch(..), Address(..), Call, context, state, word)

-- All expected results below are literal branch witnesses from original image5 A32.
data PredicateScenario = PredicateScenario
  { predicateScenarioName :: String
  , predicateSource :: Word32
  , predicateSymbol :: String
  , predicateArguments :: [Word32]
  , predicateInitial :: [Patch]
  , predicateExpectedReturn :: Word32
  , predicateExpectedWrites :: [Patch]
  , predicateExpectedCalls :: [Call]
  } deriving (Eq, Show)

data PredicateReentry = PredicateReentry
  { predicateReentryName :: String
  , predicateReentrySteps :: [PredicateScenario]
  } deriving (Eq, Show)

operationPredicatesScenarios :: [PredicateScenario]
operationPredicatesScenarios = formatCases ++ planeCases ++ halfCases ++ constantCases
  ++ featureCases ++ correctionCases ++ modeCases ++ captureCases ++ objectCases ++ processingCases
  ++ getterCases ++ unreadCases
 where
  formatCases = concat
    [ cases 0xea256c "format_split" [(0x1a,0,1),(0x1a,5,0),(0x1a,0xffffffff,1),(0x2e,5,1),(0x2e,4,0),(0,5,0)] 0x530
    , cases 0xea25bc "format_family" [(0x1a,3,1),(0x1a,4,1),(0x1a,5,1),(0x1a,13,1),(0x1a,0,0),(0x1a,0xffffffff,0),(0x2e,5,0),(0x2e,3,1),(0,3,0)] 0x530
    ]
  planeCases = [fixture source suffix (show (group,value)) group
    [word (state+0x3228) object,word (object+0x110) value] expected
    | (source,suffix,accepted) <- [(0xea2628,"primary_plane_enabled",0x2f),(0xea2664,"secondary_plane_enabled",0x30)]
    , (group,value,expected) <- [(accepted,0,0),(accepted,1,1),(accepted,2,0),(accepted,0xffffffff,0),(0,1,0)]]
  halfCases = [fixture 0xea2890 "halfword_mode_split" (show (group,value)) group
    [Patch (Address (state+0x1682)) (B.pack [fromIntegral value,fromIntegral (value `div` 256),0xa5,0x5a])] expected
    | (group,value,expected) <- [(0x1a,0,0),(0x1a,1,1),(0x1a,257,0),(0x1b,0,1),(0x1b,1,0),(0x1b,65535,1),(0,1,0)]]
  constantCases = [fixture 0xea28e8 "never" (show group) group [] 0 | group <- [0,0x19,0xffffffff]]
  featureCases = concat
    [ cases 0xea2964 "feature_enabled" [(0,0,0),(0,1,1),(0,2,0),(0xffffffff,1,1)] 0x2fb8
    , cases 0xea2990 "feature_disabled" [(0,0,1),(0,1,0),(0,2,1),(0,0xffffffff,1)] 0x2fb8
    , cases 0xea2a88 "alternate_feature_disabled" [(0,0,1),(0,1,0),(0,2,1),(0,0xffffffff,1)] 0x2fbc
    ]
  correctionCases = [fixture source suffix (show (enabled,mode,primary,secondary)) 0
    [word (state+0x3098) object,word (object+0x58) enabled,word (state+0x4b4) mode,
     word (state+0x5b8) primary,word (state+0x5b4) secondary] expected
    | (source,suffix,values) <-
      [(0xea29b8,"correction_disabled",[(0,0,1,1,1),(2,0x19,1,1,1),(1,0,0,0,0),(1,0x19,0,1,1),(1,0x19,1,0,0),(1,0x19,2,1,1),(1,0x1a,1,0,1),(1,0x1a,0,1,0),(1,0x1a,1,0xffffffff,1)]),
       (0xea2a1c,"correction_enabled",[(0,0x19,1,1,0),(2,0x19,1,1,0),(1,0,1,1,0),(1,0x19,0,1,0),(1,0x19,1,0,1),(1,0x19,2,1,0),(1,0x1a,1,0,0),(1,0x1a,0,1,1),(1,0x1a,1,0xffffffff,0)])]
    , (enabled,mode,primary,secondary,expected) <- values]
  modeCases = concat
    [cases 0xea2ab0 "group_mode_split" [(0x19,0,1),(0x19,1,0),(0x19,0xffffffff,0),(0x1b,0,0),(0x1b,1,1),(0x1b,2,1),(0,0,0)] 0x63c,
     cases 0xea2afc "output_mode_split" [(0x29,0,1),(0x29,1,0),(0x29,0xffffffff,0),(0x2a,0,0),(0x2a,1,0),(0x2a,2,1),(0,2,0)] 0x63c]
  captureCases = [operationFixture 0xea26a0 "capture_operation" label group operation fields expected
    | (label,group,operation,fields,expected) <-
      [("ordinary",0x19,0x45,[],1),("ordinary-wrong-op",0x19,0x2d,[],0),
       ("mode1",0x19,0x49,[(0x578,1)],1),("mode1-range-low",0x19,0x2d,[(0x578,1),(0x550,0x97)],1),
       ("mode1-range-high",0x19,0x2d,[(0x578,1),(0x550,0x98)],1),("mode1-range-before",0x19,0x49,[(0x578,1),(0x550,0x96)],1),
       ("mode1-range-after",0x19,0x49,[(0x578,1),(0x550,0x99)],1),("mode1-nonzero-input",0x19,0x49,[(0x578,1),(0x4a8,1),(0x550,0x97)],1),
       ("flag5a8",0x19,0x2d,[(0x5a8,1)],1),("flag5a8-not-one",0x19,0x45,[(0x5a8,2)],1),
       ("mode2-active",0x19,0x49,[(0x578,2),(0x5a8,1)],1),("mode2-inactive",0x19,0x45,[(0x578,2)],1),
       ("input-range",0x19,0x2d,[(0x550,0x97)],1),("flag4ac",0x19,0x2d,[(0x4ac,1)],1),
       ("flag4ac-not-one",0x19,0x45,[(0x4ac,2)],1),("flag614",0x19,0x2d,[(0x614,0xffffffff)],1),
       ("flag61c",0x19,0x2d,[(0x61c,2)],1),("wrong-group",0,0x45,[],0)]]
  objectCases = [operationFixture source suffix (show (group,operation,mode,flag,byte)) group operation
    [(0x578,mode),(0x63c,flag)] expected `withByte` byte
    | (source,suffix,plain) <- [(0xea2794,"object_operation",0x21),(0xea2818,"alternate_object_operation",0x23)]
    , (group,operation,mode,flag,byte,expected) <-
      [(0x14,plain,0,0,0,1),(0x2b,plain,0,0,0,1),(0x14,0x25,0,0,1,1),
       (0x14,0x25,0,0,255,1),(0x14,0x65,1,0,0,1),(0x2b,0x65,0xffffffff,0,0,1),
       (0,plain,0,0,0,0),(0x14,0,0,0,0,0)]] ++
    [operationFixture 0xea2794 "object_operation" "word-flag" 0x14 0x25 [(0x63c,2)] 1,
     operationFixture 0xea2818 "alternate_object_operation" "ignores-word-flag" 0x14 0x23 [(0x63c,2)] 1]
  processingCases = [operationFixture 0xea2900 "processing_operation" (show (group,mode,operation)) group operation [(0x4a8,mode)] expected
    | (group,mode,operation,expected) <- [(0x20,0,0x39,1),(0x20,3,0x39,1),(0x20,4,0x3d,1),(0x20,5,0x3d,1),
       (0x20,6,0x3d,1),(0x20,7,0x39,1),(0x20,8,0x3d,1),(0x20,9,0x3d,1),(0x20,10,0x39,1),
       (0x20,0xffffffff,0x39,1),(0x20,4,0x39,0),(0,4,0x3d,0)]]
  getterCases = [operationFixture source suffix ("getter-"++show (kind,mode,index)) group operation fields 1
      `extra` [word (context+4) kind,word (context+0x34) index,word (state+slot) operation]
    | (source,suffix,group,operation,fields0) <- [(0xea26a0,"capture_operation",0x19,0x45,[]),
       (0xea2794,"object_operation",0x14,0x21,[]),(0xea2818,"alternate_object_operation",0x14,0x23,[]),
       (0xea2900,"processing_operation",0x20,0x39,[])]
    , (kind,mode,index,slot) <- [(7,0,11,0x38),(1,0x1d,11,0),(1,0,12,4),(1,0,0x45,0xe8)]
    , let fields = (0x4b4,mode):fields0]
  unreadCases = [PredicateScenario ("predicate-unread-"++suffix) source ("xm1_develop_predicate_"++suffix)
    [0,0] [] 0 [] []
    | (source,suffix) <- [(0xea256c,"format_split"),(0xea25bc,"format_family"),(0xea2628,"primary_plane_enabled"),
      (0xea2664,"secondary_plane_enabled"),(0xea2890,"halfword_mode_split"),(0xea28e8,"never"),
      (0xea2ab0,"group_mode_split"),(0xea2afc,"output_mode_split")]]

object :: Word32
object = 0x009c0000

fixture :: Word32 -> String -> String -> Word32 -> [Patch] -> Word32 -> PredicateScenario
fixture source suffix label group patches expected = PredicateScenario
  ("predicate-"++suffix++"-"++label) source ("xm1_develop_predicate_"++suffix) [context,group]
  ([word context state,word (context+4) 7,word (context+0x34) 11]++patches) expected [] []

cases :: Word32 -> String -> [(Word32,Word32,Word32)] -> Word32 -> [PredicateScenario]
cases source suffix values offset =
  [fixture source suffix (show (group,value)) group [word (state+offset) value] expected | (group,value,expected) <- values]

operationFixture :: Word32 -> String -> String -> Word32 -> Word32 -> [(Word32,Word32)] -> Word32 -> PredicateScenario
operationFixture source suffix label group operation fields expected = fixture source suffix label group
  ([word (state+offset) 0 | offset <- [0x4b4,0x578,0x5a8,0x4a8,0x550,0x4ac,0x614,0x61c,0x63c,0x19b4]]
   ++[word (state+0x38) operation]++[word (state+offset) value | (offset,value) <- fields]) expected

extra :: PredicateScenario -> [Patch] -> PredicateScenario
extra scenario patches = scenario { predicateInitial = predicateInitial scenario ++ patches }

withByte :: PredicateScenario -> Word32 -> PredicateScenario
withByte scenario value = scenario `extra` [Patch (Address (state+0x19b4)) (B.singleton (fromIntegral value))]

operationPredicateReentries :: [PredicateReentry]
operationPredicateReentries =
  [PredicateReentry "frame-replaced-between-calls"
    [fixture 0xea2964 "feature_enabled" "first-frame" 0 [word (state+0x2fb8) 1] 1,
     fixture 0xea2964 "feature_enabled" "replacement-frame" 0 [word context object,word (object+0x2fb8) 0] 0,
     fixture 0xea2964 "feature_enabled" "original-frame" 0 [word (state+0x2fb8) 1] 1],
   PredicateReentry "pointed-object-changes-between-calls"
    [fixture 0xea2628 "primary_plane_enabled" (show value) 0x2f [word (state+0x3228) object,word (object+0x110) value] expected
      | (value,expected) <- [(1,1),(2,0),(1,1)]],
   PredicateReentry "operation-and-mode-change-between-calls"
    [operationFixture 0xea2900 "processing_operation" (show mode) 0x20 operation [(0x4a8,mode)] expected
      | (mode,operation,expected) <- [(4,0x3d,1),(7,0x3d,0),(7,0x39,1)]]]
