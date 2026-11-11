{-# LANGUAGE GHC2021 #-}
module OperationObjectsScenarios
  ( operationObjectsPublications
  , operationObjectsScenarios
  ) where

import qualified BuffersScenarios as Buffers
import ImagePlanesScenarios (currentTaskCalls, priorityInitial)
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), context, state, params, word, wordsAt)

operationObjectsPublications :: [StagePublication]
operationObjectsPublications =
  [ StagePublication (Address address) (StageSymbol symbol)
  | (address, symbol) <-
      [ (0xe92d10, "xm1_develop_object_rule_class")
      , (0xe92e30, "xm1_develop_operation_object_rule")
      , (0xe92fec, "xm1_develop_apply_operation_table")
      , (0xe93140, "xm1_develop_set_operation_objects")
      , (0xe4eca8, "xm1_develop_mark_task_active")
      , (0xeb0e48, "xm1_develop_bitmap_union")
      , (0xeb0ea0, "xm1_develop_bitmap_set")
      ]
  ]

publication :: Word32 -> StagePublication
publication address = case filter ((== Address address) . stageStockEntry) operationObjectsPublications of
  [entry] -> entry
  _ -> error "unknown operation-object publication"

operationObjectsScenarios :: [StageScenario]
operationObjectsScenarios = classCases ++ consumedCases ++ absentCases ++ legacyRangeCases ++ fallbackRangeCases ++ closeCases ++ markerCases ++ bitmapCases
 where
  make name entry args fields result = StageScenario
    (StageCaseName ("operation-objects-" ++ name))
    (publication entry)
    args
    (word context state : [word (state + offset) value | (offset, value) <- fields])
    [] result [] []
  classCases =
    [ make ("class-" ++ show index) 0xe92d10 [context]
        [(0x78, category), (0x4b4, mode), (0x27a8, variant)] (Just expected)
    | (index, (category, mode, variant, expected)) <- zip [0 :: Int ..]
        [ (0, 0, 0, 0)
        , (0, 0, 1, 1)
        , (0, 0, 2, 0)
        , (0, 25, 0, 1)
        , (0, 25, 2, 1)
        , (0, 26, 0, 2)
        , (0, 26, 1, 1)
        , (0, 26, 2, 2)
        , (0, 0xffffffff, 0, 0)
        , (1, 26, 0, 3)
        , (11, 25, 1, 3)
        , (0xffffffff, 0, 0, 3)
        ]
    ]
  consumedCases =
    [ make ("matched-no-rule-" ++ show (category, mode, variant, phase)) 0xe92fec
        [context, 0xfffffffe, phase, 0x1937]
        [(0x78, category), (0x4b4, mode), (0x27a8, variant)] (Just 1)
    | (category, mode, variant) <-
        [ (0, 0, 0), (0, 25, 0), (0, 26, 0), (0, 26, 1)
        , (1, 0, 0), (2, 0, 0), (3, 0, 0), (7, 0, 0)
        , (8, 0, 0), (9, 0, 0), (10, 0, 0), (11, 0, 0)
        ]
    , phase <- [0, 1, 2, 0xffffffff]
    ]
  absentCases =
    [ make ("missing-profile-" ++ show (category, mode, phase)) 0xe92fec
        [context, 0xfffffffe, phase, 0x1937]
        [(0x78, category), (0x4b4, mode)] (Just 0)
    | category <- [4, 5, 6, 12, 0xffffffff]
    , mode <- [0, 25, 26]
    , phase <- [0, 1, 2]
    ]
  legacyRangeCases =
    [ make ("legacy-outside-" ++ show (operation, phase)) 0xe92e30
        [context, operation, phase, 0x1937] [] Nothing
    | operation <- [113, 128, 0xffffffff]
    , phase <- [0, 1, 2]
    ]
  fallbackRangeCases =
    [ make ("fallback-unlisted-" ++ show (operation, phase)) 0xe93140
        [context, operation, phase, 0x1937] [(0x78, 4)] Nothing
    | operation <- [4, 0x21, 0x7f, 0xffffffff]
    , phase <- [0, 1, 2]
    ]
  closeCases =
    [ (make ("close-empty-" ++ show (entry, operation, phase)) entry
        [context, operation, phase, 0x1937] [(0x78, category)] result)
        { stageInitial =
            word context state : word (state + 0x78) category :
            Buffers.criticalInitial ++
            [word 0x154a9bc 0, word 0x154ad2c 0]
        , stageCalls = concat (replicate count Buffers.criticalCalls)
        }
    | (entry, operation, phase, category, count, result) <-
        [ (0xe92fec, 3, 2, 0, 2, Just 1)
        , (0xe92fec, 3, 1, 0, 0, Just 1)
        , (0xe92e30, 2, 0, 0, 1, Nothing)
        , (0xe92e30, 34, 2, 0, 2, Nothing)
        , (0xe92e30, 105, 2, 0, 0, Nothing)
        , (0xe93140, 3, 2, 4, 3, Nothing)
        , (0xe93140, 0x10, 2, 4, 1, Nothing)
        , (0xe93140, 0x32, 2, 4, 3, Nothing)
        ]
    ]
  markerCases =
    [ (make ("task-marker-" ++ show task) 0xe4eca8 [state] [] Nothing)
        { stageInitial = priorityInitial task ++
            [word (state + (task + 5) * 4) 0xdeadbeef, word (context + 0x34) 0xffffffff]
        , stageFinal = [word (state + (task + 5) * 4) 1]
        , stageCalls = currentTaskCalls task
        }
    | task <- [0, 0x45, 0x1ff]
    ]
  bitmapCases =
    [ (make ("bitmap-union-" ++ show bits) 0xeb0e48 [params, params + 0x100, bits] [] Nothing)
        { stageInitial = [wordsAt params [0xff0000ff, 0x10, 0xa5], wordsAt (params + 0x100) [0x00ff0000, 0x11, 0xf0]]
        , stageFinal = [wordsAt params expected]
        }
    | (bits, expected) <-
        [ (0, [0xff0000ff, 0x10, 0xa5])
        , (1, [0xffff00ff, 0x10, 0xa5])
        , (31, [0xffff00ff, 0x10, 0xa5])
        , (32, [0xffff00ff, 0x10, 0xa5])
        , (33, [0xffff00ff, 0x11, 0xa5])
        , (64, [0xffff00ff, 0x11, 0xa5])
        , (65, [0xffff00ff, 0x11, 0xf5])
        ]
    ] ++
    [ (make ("bitmap-set-" ++ show bit) 0xeb0ea0 [params, bit] [] Nothing)
        { stageInitial = [wordsAt params [0, 0x10, 0, 0xfeedbeef]]
        , stageFinal = [wordsAt params expected]
        }
    | (bit, expected) <-
        [ (0, [1, 0x10, 0, 0xfeedbeef])
        , (31, [0x80000000, 0x10, 0, 0xfeedbeef])
        , (32, [0, 0x11, 0, 0xfeedbeef])
        , (63, [0, 0x80000010, 0, 0xfeedbeef])
        , (64, [0, 0x10, 1, 0xfeedbeef])
        ]
    ]
