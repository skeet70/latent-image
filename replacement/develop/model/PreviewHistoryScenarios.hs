{-# LANGUAGE GHC2021 #-}
module PreviewHistoryScenarios
  ( PreviewHistoryScenario(..), PreviewHistoryPhase(..)
  , PreviewInvocation(..), PreviewOutcome(..)
  , PreviewPrimaryConsumerLaw(..), PreviewCopyOutcome(..)
  , previewHistoryPublications, previewHistoryScenarios, previewPrimaryConsumerLaws
  ) where

import qualified Data.ByteString as B
import Data.List.NonEmpty (NonEmpty(..))
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import qualified BuffersScenarios as Buffers
import DiagnosticLoggingScenarios (textDisabledInitial, traceDisabledInitial)
import ImagePlanesScenarios (currentTaskCalls, priorityCalls, priorityInitial, signalCurrentCalls)
import PipelineModel
import Scenarios (Address(..), Call, Patch(..), base, call, context, params, stackTop, state, word, wordsAt)

-- Initial patches apply once; subsequent phases continue observed RAM.
data PreviewHistoryScenario = PreviewHistoryScenario
  { previewHistoryCaseName :: StageCaseName
  , previewHistoryInvocation :: PreviewInvocation
  , previewHistoryInitial :: [Patch]
  , previewHistoryCodePointers :: [(Address, StagePublication)]
  , previewHistoryPhases :: NonEmpty PreviewHistoryPhase
  }
data PreviewHistoryPhase = PreviewHistoryPhase
  { previewPhaseName :: StageCaseName
  , previewPhasePublication :: StagePublication
  , previewPhaseArguments :: [Word32]
  , previewPhaseExpected :: [Patch]
  , previewPhaseReturn :: Maybe Word32
  , previewPhaseCalls :: [Call]
  , previewPhaseOutcome :: PreviewOutcome
  }
data PreviewInvocation = PreviewRoot | PreviewOriginalAAPCS deriving (Eq, Show)
data PreviewOutcome = PreviewReturned | PreviewFrameReleaseTransfer [(N.Register, Word32)] deriving (Eq, Show)
data PreviewCopyOutcome = PreviewCopyFinished | PreviewCopyCancelled deriving (Eq, Show)

-- These laws describe an unexecuted retained consumer, not a scripted Root callee.
data PreviewPrimaryConsumerLaw = PreviewPrimaryConsumerLaw
  { previewConsumerCaseName :: StageCaseName
  , previewConsumerSource :: Address
  , previewConsumerArguments :: [Word32]
  , previewConsumerInitial :: [Patch]
  , previewConsumerRequiredPrimary :: [Patch]
  , previewConsumerExpected :: [Patch]
  , previewConsumerOutcome :: PreviewCopyOutcome
  , previewConsumerCalls :: [Call]
  }

data PreviewHistory = FirstOpaqueHistory | SecondOpaqueHistory deriving (Eq, Show)
data PreviewGeometry = FourByThree | ThreeByTwo | SixteenByNine | Square deriving (Eq, Show)
data PreviewPath = PublishPreview | GuardedPreview | RefusedPreview | AlreadyCompleted deriving (Eq, Show)
newtype PreviewMode = PreviewMode Word32 deriving (Eq, Show)

rootPublication, directPublication :: StagePublication
rootPublication = StagePublication (Address 0xea0934) (StageSymbol "xm1_develop_dispatch")
directPublication = StagePublication (Address 0xe9ac44) (StageSymbol "xm1_develop_pipeline_resize_preview")
previewHistoryPublications :: [StagePublication]
previewHistoryPublications = [rootPublication,directPublication]

task, objectDescriptor, pixels, consumerDescriptor, consumerPixels, cancelFlag :: Word32
task = 11
objectDescriptor = params+0x400
pixels = 0xa00000
consumerDescriptor = params+0x600
consumerPixels = 0x9c2000
cancelFlag = params+0x700

opaqueWords :: PreviewHistory -> [Word32]
opaqueWords FirstOpaqueHistory = [0x10203040,0x9c7000,0x13579bdf,0x2468ace0,0x0badcafe,0x81234567,0x55667788]
opaqueWords SecondOpaqueHistory = [0xfedcba98,0x9d7000,0x89abcdef,0x76543210,0xa55a3cc3,0x80000001,0xffffffff]

snapshotWords :: PreviewHistory -> [Word32]
snapshotWords FirstOpaqueHistory = [0x31415926,0x9c9000,0x27182818,0x44332211,0x88776655,0xcafebabe,0x5aa53cc3]
snapshotWords SecondOpaqueHistory = [0xabcdef01,0x9da000,0x90807060,0x10293847,0x56473829,0x13572468,0x8000ffff]

oldPrimary, objectPlane :: Plane
oldPrimary = Plane 0x17 0x9c9000 64 16 16 4 0x00400000
objectPlane = Plane 0x91 pixels 4096 128 120 32 0x81001234

aspect :: PreviewGeometry -> Word32
aspect FourByThree = 0
aspect ThreeByTwo = 1
aspect SixteenByNine = 2
aspect Square = 3

primary :: PreviewGeometry -> Plane
primary FourByThree = Plane 0x91 pixels 110976 544 544 204 0x81001234
primary ThreeByTwo = Plane 0x91 pixels 97920 544 544 180 0x81001234
primary SixteenByNine = Plane 0x91 pixels 83232 544 544 153 0x81001234
primary Square = Plane 0x91 pixels 83232 408 408 204 0x81001234

pixelPattern, previewPixels :: B.ByteString
pixelPattern = B.pack [0x17,0xa9,0x63,0xd1]
previewPixels = B.concat (replicate 27744 pixelPattern)

imageData :: B.ByteString -> Word32 -> Int -> Patch
imageData image address size = Patch (Address address) (B.take size (B.drop (fromIntegral (address-0xbf1a00)) image))

originalTables :: B.ByteString -> [Patch]
originalTables image =
  [ imageData image 0xea4870 (52*8)
  , imageData image 0xea3704 (8*4)
  , imageData image 0xea51a4 (257*4)
  , imageData image 0xea6738 (127*52)
  , imageData image 0xea5678 (51*16)
  , imageData image 0xea1edc 16
  , imageData image 0xea37d4 (129*4)
  ]

initial :: B.ByteString -> PreviewHistory -> PreviewGeometry -> PreviewMode -> PreviewPath -> [Patch]
initial image history geometry (PreviewMode mode) path = base ++ originalTables image ++
  Buffers.objectLookupInitial (state+0x7c) 7 0 [(0x1e,0,objectDescriptor,toBufferPlane objectPlane)] ++
  priorityInitial task ++ traceDisabledInitial ++ textDisabledInitial ++
  [ word (context+4) 0x28,word (context+8) 0x07200000,word (context+0x34) task
  , word (context+0x40) 0x55aa55aa,word (context+0x44) 3
  , word (context+0x48) (if path == RefusedPreview then 0 else 2)
  , word state (if path == AlreadyCompleted then 0x3d else 0x39)
  , word (state+0x40) 0xdeadbeef,word (state+0x78) 4,word (state+0x4b4) 0
  , word (state+0x4a8) mode,word (state+0x4f0) (aspect geometry)
  , word (state+0x27cc) 1,word 0x15480d8 0,word (0xe48370+12*16) 0
  , word 0x154a9bc 0,word 0xeb1750 0,word 0x15b4f7c 0
  , planePatch (state+0x3ec) oldPrimary,wordsAt (state+0x408) (opaqueWords history)
  , wordsAt (state+0x424) [0x19375573,0x28466482]
  , planePatch (state+0x3d0) (Plane 1 0x9c8000 8192 128 128 64 0x100000)
  , Patch (Address pixels) previewPixels
  , Patch (Address 0x9c9000) (B.pack [1..64])
  , journal []
  ]
 where
  toBufferPlane (Plane format address size stride width height flags) = Buffers.Plane format address size stride width height flags

journal :: [(Word32,Word32)] -> Patch
journal entries = wordsAt 0x9a3800
  ([fromIntegral (length entries),0] ++ concat [[context,operation,result] | (operation,result) <- entries] ++ replicate (48-3*length entries) 0)

rootIteration :: [Call]
rootIteration = Buffers.criticalCalls ++ signalCurrentCalls task 0 ++ currentTaskCalls task
beginPrefix, activeCalls, finishCalls :: [Call]
beginPrefix = currentTaskCalls task ++ [call 0x42188 [0xea843c] 0]
activeCalls = beginPrefix ++ Buffers.criticalCalls ++ Buffers.criticalCalls ++ Buffers.criticalCalls ++
  signalCurrentCalls task 0 ++ currentTaskCalls task ++ [call 0x42188 [0xea8444] 0]
finishCalls = rootIteration ++ [call 0x42188 [0xea93c0] 0]

rootCalls :: PreviewPath -> Bool -> [Call]
rootCalls path reentry = signalCurrentCalls task 0 ++ case path of
  RefusedPreview -> rootIteration ++ beginPrefix
  AlreadyCompleted -> finishCalls
  _ | reentry -> finishCalls
  GuardedPreview -> rootIteration ++ finishCalls
  PublishPreview -> rootIteration ++ activeCalls ++ finishCalls

expected :: PreviewHistory -> PreviewGeometry -> PreviewPath -> Bool -> [Patch]
expected history geometry path reentry =
  [ word state (if path == RefusedPreview then 0x39 else 0x3d)
  , word (context+0x40) (if path == RefusedPreview then 1 else 0)
  , word (state+0x40) 1
  , planePatch (state+0x3ec) (if path == PublishPreview then primary geometry else oldPrimary)
  , wordsAt (state+0x408) (opaqueWords history)
  , wordsAt (state+0x424) [0x19375573,0x28466482]
  , planePatch objectDescriptor objectPlane
  , Patch (Address pixels) previewPixels
  , Patch (Address 0x9c9000) (B.pack [1..64])
  , journal events
  ]
 where
  events = case path of
    PublishPreview -> [(0x3c,0),(0x3d,0),(0xfe,0)] ++ [(0xfe,0) | reentry]
    RefusedPreview -> replicate (if reentry then 2 else 1) (0xfe,6)
    _ -> replicate (if reentry then 2 else 1) (0xfe,0)

previewHistoryScenarios :: B.ByteString -> (StagePublication -> Address) -> [PreviewHistoryScenario]
previewHistoryScenarios image resolve = resolve rootPublication `seq` resolve directPublication `seq`
  rootCases ++ directCases
 where
  histories = [FirstOpaqueHistory,SecondOpaqueHistory]
  roots = [(PublishPreview,PreviewMode 4,geometry) | geometry <- [FourByThree,ThreeByTwo,SixteenByNine,Square]] ++
    [(PublishPreview,PreviewMode 9,FourByThree),(PublishPreview,PreviewMode 8,Square)] ++
    [(GuardedPreview,PreviewMode mode,FourByThree) | mode <- [0,7,10]] ++
    [(RefusedPreview,PreviewMode mode,FourByThree) | mode <- [4,9]] ++
    [(AlreadyCompleted,PreviewMode 4,FourByThree)]
  rootCases =
    [ PreviewHistoryScenario (StageCaseName ("preview-Root-"++show (path,mode,geometry,history))) PreviewRoot
        (initial image history geometry mode path) []
        (rootPhase "dispatch" False :| [rootPhase "dispatch-reentry" True])
    | (path,mode,geometry) <- roots,history <- histories
    , let rootPhase label reentry = PreviewHistoryPhase (StageCaseName label) rootPublication [context]
            (expected history geometry path reentry) Nothing (rootCalls path reentry) PreviewReturned
    ]
  directCases =
    [ PreviewHistoryScenario (StageCaseName ("preview-OriginalAAPCS-"++show (mode,geometry,history))) PreviewOriginalAAPCS
        (initial image history geometry mode PublishPreview ++ [wordsAt (stackTop-0x50) (snapshotWords history)]) []
        (PreviewHistoryPhase (StageCaseName "defined-entry-snapshot") directPublication [context]
          [ word state 0x3d,word (state+0x40) 1,word (context+0x40) 0x55aa55aa
          , planePatch (state+0x3ec) (primary geometry),wordsAt (state+0x408) (snapshotWords history)
          , wordsAt (state+0x424) [0x19375573,0x28466482],planePatch objectDescriptor objectPlane
          , Patch (Address pixels) previewPixels,journal [(0x3c,0),(0x3d,0)]
          ] (Just 0) activeCalls PreviewReturned :| [])
    | (mode,geometry) <- [(PreviewMode 4,FourByThree),(PreviewMode 9,Square)],history <- histories
    ]

-- The original e8f11c consumer loads source address, stride, line size and line count at +4,+0xc,+0x10,+0x14.
previewPrimaryConsumerLaws :: [PreviewPrimaryConsumerLaw]
previewPrimaryConsumerLaws =
  [ PreviewPrimaryConsumerLaw (StageCaseName ("preview-retained-primary-copy-"++show (history,outcome)))
      (Address 0xe8f11c) [cancelFlag,consumerDescriptor,state+0x3ec,task,6]
      [ planePatch consumerDescriptor (Plane 0x91 consumerPixels 117504 576 544 204 0x81001234)
      , Patch (Address consumerPixels) untouched,Patch (Address cancelFlag) (B.singleton cancel)
      ]
      [ planePatch (state+0x3ec) (primary FourByThree),wordsAt (state+0x408) (opaqueWords history)
      , Patch (Address pixels) previewPixels
      ]
      [ Patch (Address consumerPixels) (if outcome == PreviewCopyFinished then copied else untouched)
      , planePatch (state+0x3ec) (primary FourByThree),wordsAt (state+0x408) (opaqueWords history)
      , Patch (Address pixels) previewPixels
      ] outcome (priorityCalls task 12 ++ priorityCalls task 6)
  | history <- [FirstOpaqueHistory,SecondOpaqueHistory]
  , (outcome,cancel) <- [(PreviewCopyFinished,0),(PreviewCopyCancelled,1)]
  ]
 where
  untouched = B.replicate 117504 0xa5
  copied = B.concat (replicate 204 (B.concat (replicate 136 pixelPattern) <> B.replicate 32 0xa5))
