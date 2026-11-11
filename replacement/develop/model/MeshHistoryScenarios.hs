{-# LANGUAGE GHC2021 #-}
module MeshHistoryScenarios
  ( MeshHistoryScenario(..)
  , MeshHistoryPhase(..)
  , meshHistoryPublications
  , meshHistoryScenarios
  ) where

import qualified Data.ByteString as B
import Data.List.NonEmpty (NonEmpty(..))
import Data.Word (Word8, Word32)
import qualified BuffersScenarios as Buffers
import DiagnosticLoggingScenarios (textDisabledInitial, traceDisabledInitial)
import ImagePlanesScenarios (currentTaskCalls, priorityCalls, reservedInitial, reservedSignalCalls, reservedWaitCalls, signalCurrentCalls, waitCalls)
import ImagePixelsScenarios (blendCalls)
import qualified ImageOperationsScenarios as Operations
import PipelineModel
import Scenarios (Address(..), Call(..), Patch(..), base, call, context, params, state, word, wordBytes, wordsAt)
import qualified StripPlannerScenarios as Strip
import qualified WarpEngineScenarios as Warp

-- Initial memory and pointer bindings apply once. Phase expectations are assertions, not continuation inputs.
data MeshHistoryScenario = MeshHistoryScenario
  { meshHistoryCaseName :: StageCaseName
  , meshHistoryInitial :: [Patch]
  , meshHistoryCodePointers :: [(Address, StagePublication)]
  , meshHistoryPhases :: NonEmpty MeshHistoryPhase
  }

data MeshHistoryPhase = MeshHistoryPhase
  { meshPhaseName :: StageCaseName
  , meshPhasePublication :: StagePublication
  , meshPhaseArguments :: [Word32]
  , meshPhaseExpected :: [Patch]
  , meshPhaseReturn :: Maybe Word32
  , meshPhaseCalls :: [Call]
  }

data MeshGuard = Cancelled | MatchFailed | ReferenceFailed deriving (Eq, Show)
data MeshSchedule
  = EmptyInsufficient
  | PartialInsufficient
  | RisingEmptyThreshold
  | RisingPartialThreshold
  | CompletedAccepted
  | CompletedCachedRejection
  | NonUnitFailureFlag
  | AlreadyGuarded MeshGuard
  deriving (Eq, Show)
data MeshCandidates = NoCandidates | OneCandidate deriving (Eq, Show)
data MeshAcceptance = AcceptMesh | RejectCachedCount deriving (Eq, Show)
data MeshPixelObservation = BeforeBlend | AfterBlend deriving (Eq, Show)
data MeshExpectedOutcome = NotInvoked | Insufficient MeshCandidates | Completed MeshAcceptance deriving (Eq, Show)
newtype MeshThreshold = MeshThreshold Word8
newtype MeshPriorOrigin = MeshPriorOrigin (Word32, Word32) deriving (Eq, Show)

matchPublication, blendPublication, operationComplete, geometryInterrupt :: StagePublication
matchPublication = StagePublication (Address 0xea07ec) (StageSymbol "xm1_develop_pipeline_match_triangles")
blendPublication = StagePublication (Address 0xe94a44) (StageSymbol "xm1_develop_pipeline_blend_reference")
operationComplete = StagePublication (Address 0xe63e68) (StageSymbol "xm1_pipeline_image_operation_complete")
geometryInterrupt = StagePublication (Address 0xda9230) (StageSymbol "xm1_pipeline_image_geometry_irq")

meshHistoryPublications :: [StagePublication]
meshHistoryPublications = [matchPublication, blendPublication]

meshHistoryScenarios :: B.ByteString -> (StagePublication -> Address) -> [MeshHistoryScenario]
meshHistoryScenarios image resolve =
  [make schedule origin | schedule <- schedules, origin <- origins]
 where
  schedules = [EmptyInsufficient, PartialInsufficient, RisingEmptyThreshold, RisingPartialThreshold,
    CompletedAccepted, CompletedCachedRejection, NonUnitFailureFlag] ++ map AlreadyGuarded [Cancelled, MatchFailed, ReferenceFailed]
  origins = [MeshPriorOrigin (0x13579bdf,0xfdb97531), MeshPriorOrigin (8,0)]
  make schedule origin =
    let result = outcome schedule
        activeBlend = result == Completed AcceptMesh
        firstCalls = case result of
          NotInvoked -> []
          _ -> beginCalls 0xea8a10 ++ concat (replicate 3 Buffers.criticalCalls) ++ meshCalls schedule ++ completeCalls 0xea8a18
        secondCalls = if activeBlend then beginCalls 0xea8a3c ++ Buffers.criticalCalls ++
          copyErrorCalls firstCopy ++ blendCalls task ++ copyErrorCalls secondCopy ++ completeCalls 0xea8a48 else []
        initial = initialFrame image schedule origin ++ if activeBlend then hardwareInitial else []
        first = phase matchPublication "match-triangles" (visible schedule origin BeforeBlend ++
          [word (state+0x38) 0xbb, journal (if result == NotInvoked then [] else [0xba,0xbb])]) firstCalls
        second = phase blendPublication "blend-reference" (visible schedule origin (if activeBlend then AfterBlend else BeforeBlend) ++
          [word (state+0x38) 0xbd, journal (if result == NotInvoked then [] else if activeBlend then [0xba,0xbb,0xbc,0xbd] else [0xba,0xbb])] ++
          if activeBlend then copyErrorEffects secondCopy else []) secondCalls
     in resolve matchPublication `seq` resolve blendPublication `seq`
        MeshHistoryScenario (StageCaseName ("mesh-history-" ++ show schedule ++ "-" ++ show origin)) initial
          (if activeBlend then copyPointers else []) (first :| [second])
  phase publication name expected calls = MeshHistoryPhase (StageCaseName name) publication [context] expected (Just 0) calls
  firstCopy = copyErrorProtocol image resolve
    (Operations.GeometryPlane sourcePixelsAddress 64 64 16) (Operations.GeometryPlane scratchPixelsAddress 64 64 16)
  secondCopy = copyErrorProtocol image resolve
    (Operations.GeometryPlane destinationPixelsAddress 40 32 16) (Operations.GeometryPlane (scratchPixelsAddress+16) 64 32 16)

outcome :: MeshSchedule -> MeshExpectedOutcome
outcome schedule = case schedule of
  EmptyInsufficient -> Insufficient NoCandidates
  PartialInsufficient -> Insufficient OneCandidate
  RisingEmptyThreshold -> Insufficient NoCandidates
  RisingPartialThreshold -> Insufficient OneCandidate
  CompletedAccepted -> Completed AcceptMesh
  CompletedCachedRejection -> Completed RejectCachedCount
  NonUnitFailureFlag -> Insufficient NoCandidates
  AlreadyGuarded _ -> NotInvoked

thresholds :: MeshSchedule -> (MeshThreshold, MeshThreshold)
thresholds schedule = case schedule of
  EmptyInsufficient -> pair 1 1
  PartialInsufficient -> pair 2 2
  RisingEmptyThreshold -> pair 0 1
  RisingPartialThreshold -> pair 1 2
  CompletedAccepted -> pair 0 0
  CompletedCachedRejection -> pair 5 0
  NonUnitFailureFlag -> pair 1 1
  AlreadyGuarded _ -> pair 1 1
 where pair before after = (MeshThreshold before, MeshThreshold after)

initialFlags :: MeshSchedule -> [Word32]
initialFlags schedule = case schedule of
  AlreadyGuarded Cancelled -> [1,0,0]
  AlreadyGuarded MatchFailed -> [0,1,0]
  AlreadyGuarded ReferenceFailed -> [0,0,1]
  NonUnitFailureFlag -> [0,2,0]
  _ -> [0,0,0]

finalFlags :: MeshSchedule -> [Word32]
finalFlags schedule = case outcome schedule of
  NotInvoked -> initialFlags schedule
  Completed AcceptMesh -> [0,0,0]
  _ -> [0,1,0]

motion, task, sourcePixelsAddress, destinationPixelsAddress, scratchPixelsAddress, workspace, labels, points, mapped, triangles :: Word32
motion = state+0x10000
task = 0x45
sourcePixelsAddress = params+0x1000
destinationPixelsAddress = params+0x1800
scratchPixelsAddress = params+0x9000
workspace = params+0x4000
labels = params+0x5000
points = motion+0x11134
mapped = motion+0x1192c
triangles = motion+0x12124

sourcePlane, destinationPlane, priorReferencePlane, warpedPlane, scratchPlane :: Buffers.Plane
sourcePlane = Buffers.Plane 0 sourcePixelsAddress 1024 64 64 16 0x1234
destinationPlane = Buffers.Plane 0 destinationPixelsAddress 640 40 40 16 0x5678
priorReferencePlane = Buffers.Plane 0x91 0xdead0000 48 48 48 1 0x8282
warpedPlane = Buffers.Plane 0 destinationPixelsAddress 640 40 32 16 0x5678
scratchPlane = Buffers.Plane 0 scratchPixelsAddress 1024 64 64 16 0x9abc

sourcePixels, untouchedDestination, scratchPixels :: B.ByteString
sourcePixels = B.pack [fromIntegral ((n*13+37) `mod` 256) | n <- [0..1023 :: Int]]
untouchedDestination = B.replicate 640 0xa5
scratchPixels = B.replicate 1024 0xa5

initialFrame :: B.ByteString -> MeshSchedule -> MeshPriorOrigin -> [Patch]
initialFrame image schedule (MeshPriorOrigin (x,y)) = base ++ [word (context+8) 0x07200000] ++ traceDisabledInitial ++ textDisabledInitial ++
  Buffers.objectLookupInitial (state+0x7c) 7 0
    [(0x30,0,params+0x400,destinationPlane),
     (0x31,0,params+0x420,Buffers.Plane 0 workspace 4096 0 0 0 0),
     (0x32,0,params+0x440,Buffers.Plane 0 labels (992*16) 992 992 16 0),
     (0x33,0,params+0x460,scratchPlane)] ++
  [imageData image 0xea6738 (127*52), imageData image 0xea3f70 2280, imageData image 0xea55e8 144,
   wordsAt 0xe48380 [0,0,0,0], wordsAt 0xe48460 [0,0,0,0], word 0x15480d8 0,
   word (state+0x38) 0xb9, word (state+0x78) 4, word (state+0x128) 0, word (state+0x4b4) 0,
   word (state+0x5cc) 4, word (state+0x3228) motion, wordsAt (motion+0x10c) (initialFlags schedule),
   Buffers.planePatch (motion+0x1c) sourcePlane, Buffers.planePatch (motion+0x8c) priorReferencePlane,
   wordsAt (motion+0xfc) [0,0,32], wordsAt (motion+0x118) [candidateCount,16],
   wordsAt (motion+0x120) [x,y,0x51515151,0x61616161],
   wordsAt (motion+0x134) [16,8], wordsAt (motion+0x8134) [17,9],
   Patch (Address (motion+0x10134)) (B.singleton 1),
   wordsAt points (replicate 8 0x71717171), wordsAt mapped (replicate 8 0x81818181),
   wordsAt triangles (replicate 16 0x91919191), Patch (Address workspace) (B.replicate 4096 0x6b),
   Patch (Address sourcePixelsAddress) sourcePixels, Patch (Address destinationPixelsAddress) untouchedDestination,
   Patch (Address scratchPixelsAddress) scratchPixels, Patch (Address labels) (B.replicate (992*16) 0xcc),
   journal []] ++ configuration before
 where
  (before,_) = thresholds schedule
  candidateCount = case outcome schedule of Insufficient OneCandidate -> 1; _ -> 0

imageData :: B.ByteString -> Word32 -> Int -> Patch
imageData image address size = Patch (Address address) (B.take size (B.drop (fromIntegral (address-0xbf1a00)) image))

configuration :: MeshThreshold -> [Patch]
configuration threshold =
  [word 0x96ca50 0x9e0000, half 0x9f12 16, half 0x9fa7 16,
   thresholdPatch threshold, configByte 0x9fa9 8, configByte 0x9faa 8, configByte 0x9fab 32,
   configByte 0x9f6f 0, configByte 0xa8 0]
 where half key value = Patch (Address (0x9e0000+key)) (B.take 2 (wordBytes value))

configByte :: Word32 -> Word8 -> Patch
configByte key value = Patch (Address (0x9e0000+key)) (B.singleton value)
thresholdPatch :: MeshThreshold -> Patch
thresholdPatch (MeshThreshold value) = configByte 0x9fa5 value

meshCalls :: MeshSchedule -> [Call]
meshCalls schedule = currentTaskCalls task ++ priorityCalls task 13 ++
  [Call (Address 0xeb0d04) [] [] changes, call 0xeb0d04 [] 0] ++ priorityCalls task 6
 where
  (MeshThreshold before,after@(MeshThreshold inner)) = thresholds schedule
  changes = [thresholdPatch after | before /= inner]

beginCalls :: Word32 -> [Call]
beginCalls label = currentTaskCalls task ++ [call 0x42188 [label] 0] ++ Buffers.criticalCalls
completeCalls :: Word32 -> [Call]
completeCalls label = Buffers.criticalCalls ++ signalCurrentCalls task 0 ++ currentTaskCalls task ++ [call 0x42188 [label] 0]

journal :: [Word32] -> Patch
journal operations = Patch (Address 0x9a3800) (B.concat (map wordBytes values))
 where values = [fromIntegral (length operations),0] ++ concat [[context,operation,0] | operation <- operations] ++ replicate (48-3*length operations) 0

visible :: MeshSchedule -> MeshPriorOrigin -> MeshPixelObservation -> [Patch]
visible schedule (MeshPriorOrigin (x,y)) observation =
  [wordsAt (motion+0x10c) (finalFlags schedule), word (state+0x128) activity,
   wordsAt (motion+0x120) resultWords, Buffers.planePatch (motion+0x1c) (if observation == AfterBlend then scratchPlane else sourcePlane),
   Buffers.planePatch (motion+0x8c) reference,
   Patch (Address sourcePixelsAddress) sourcePixels, Patch (Address scratchPixelsAddress) scratchPixels,
   Patch (Address destinationPixelsAddress) destination, thresholdPatch finalThreshold,
   wordsAt (motion+0x134) [16,8], wordsAt (motion+0x8134) [17,9], Patch (Address (motion+0x10134)) (B.singleton 1)] ++
  case result of
    NotInvoked -> [Patch (Address labels) (B.replicate (992*16) 0xcc)] ++ unchangedWork
    Insufficient candidates ->
      [Patch (Address labels) (B.replicate (992*16) 0), wordsAt points (candidateWords candidates [16,8] 0x71717171),
       wordsAt mapped (candidateWords candidates [17,9] 0x81818181), wordsAt triangles (replicate 16 0x91919191),
       Patch (Address workspace) (B.replicate 4096 0x6b)]
    Completed _ ->
      [wordsAt points [8,0,8,15,24,0,24,15], wordsAt mapped [8,0,8,15,24,0,24,15],
       wordsAt triangles (concat [[0x00100008,a,b,c] | (a,b,c) <- [(0,1,2),(0,1,3),(0,2,3),(1,2,3)]]),
       wordsAt workspace (concat (replicate 4 [0x00100008,65536,0,0,0,65536,0])),
       Patch (Address labels) (B.concat [wordBytes (if offset `mod` 992 < 64 then 0x00100008 else 0) | offset <- [0,4..992*16-4 :: Int]])]
 where
  result = outcome schedule
  activity = if result == NotInvoked then 0 else 1
  (before,after) = thresholds schedule
  finalThreshold = if result == NotInvoked then before else after
  resultWords = case result of
    NotInvoked -> [x,y,0x51515151,0x61616161]
    Insufficient NoCandidates -> [x,y,0,0]
    Insufficient OneCandidate -> [x,y,1,0]
    Completed _ -> [8,0,4,4]
  reference = case result of NotInvoked -> priorReferencePlane; Completed _ -> warpedPlane; _ -> destinationPlane
  destination = case result of Completed _ -> if observation == AfterBlend then blendedPixels else warpedPixels; _ -> untouchedDestination
  candidateWords candidates accepted sentinel = case candidates of NoCandidates -> replicate 8 sentinel; OneCandidate -> accepted ++ replicate 6 sentinel
  unchangedWork = [wordsAt points (replicate 8 0x71717171), wordsAt mapped (replicate 8 0x81818181),
    wordsAt triangles (replicate 16 0x91919191), Patch (Address workspace) (B.replicate 4096 0x6b)]

warpedPixels, blendedPixels :: B.ByteString
warpedPixels = fourRows
  [[245,2,15,28,41,54,67,80,93,106,119,132,145,158,171,184,197,210,223,236,249,6,19,32,45,58,71,84,97,110,123,136],
   [53,66,79,92,105,118,131,144,157,170,183,196,209,222,235,248,5,18,31,44,57,70,83,96,109,122,135,148,161,174,187,200],
   [117,130,143,156,169,182,195,208,221,234,247,4,17,30,43,56,69,82,95,108,121,134,147,160,173,186,199,212,225,238,251,8],
   [181,194,207,220,233,246,3,16,29,42,55,68,81,94,107,120,133,146,159,172,185,198,211,224,237,250,7,20,33,46,59,72]]
blendedPixels = fourRows
  [[165,165,165,165,134,137,141,144,129,136,142,149,150,160,170,179,197,210,223,236,249,6,19,32,45,58,71,84,97,110,123,136],
   [165,165,165,165,150,153,157,160,161,168,174,181,198,208,218,227,5,18,31,44,57,70,83,96,109,122,135,148,161,174,187,200],
   [165,165,165,165,166,169,173,176,193,200,206,85,54,64,74,83,69,82,95,108,121,134,147,160,173,186,199,212,225,238,251,8],
   [165,165,165,165,182,185,125,128,97,104,110,117,102,112,122,131,133,146,159,172,185,198,211,224,237,250,7,20,33,46,59,72]]

fourRows :: [[Word8]] -> B.ByteString
fourRows rows = B.concat (replicate 4 (B.concat [B.pack row <> B.replicate 8 0xa5 | row <- rows]))

data CopyErrorProtocol = CopyErrorProtocol { copyErrorEffects :: [Patch], copyErrorCalls :: [Call] }

hardwareInitial :: [Patch]
hardwareInitial = Warp.routeInitial ++ Warp.interruptInitial ++ reservedInitial 44
copyPointers :: [(Address, StagePublication)]
copyPointers = [(Address (0x1575844+27*4),operationComplete),(Address (0x14ce578+27*4),operationComplete)]

-- Device errors precede destination transfer. The original copy frontend still publishes its descriptor and returns one.
copyErrorProtocol :: B.ByteString -> (StagePublication -> Address) -> Operations.GeometryPlane -> Operations.GeometryPlane -> CopyErrorProtocol
copyErrorProtocol image resolve input output = CopyErrorProtocol effects calls
 where
  addressWord (Address address) = address
  request = Operations.operationRequestWords image (Operations.OperationRequest 7 0 0 0 0 0 0)
    [Just input,Just input] 2 output (addressWord (resolve operationComplete))
  law = case Strip.geometryCopyLaw request (256,0,0,0) of Right value -> value; Left problem -> error (show problem)
  Strip.GeometryDimensionsWords dimensions = Strip.copyDimensions law
  Strip.GeometryPlanWords plan = Strip.copyMainPlan law
  effects = [word (address+fromIntegral index*4) value | address <- [0x1575844,0x14ce578],(index,value) <- zip [0::Int ..] request,index /= 30] ++
    [wordsAt 0x14ce600 (replicate 6 0), wordsAt 0x14ce680 dimensions, wordsAt 0x14ce618 plan,
     word 0x157bd08 0, wordsAt 0x96eb9c [1,0,0], wordsAt 0x14ce570 [2,0], word 0x942d8c 4,
     word 0xfff48004 0x24, word 0xfff480c8 1, word 0xfff480c4 0x331] ++ Warp.interruptReleaseEffects 68
  calls = [call 0x42188 [0xe66708] 0] ++ reservedWaitCalls 44 0xffffffff 0 ++ Warp.routeRequestCalls True ++ Warp.routeAcquireCalls ++
    [Strip.resetCall] ++ Strip.geometryCopyDeviceCalls law ++ Warp.interruptAcquireCalls 68 13 1 (addressWord (resolve geometryInterrupt)) ++
    [call 0x42188 [0xe66710] 0] ++ currentTaskCalls task ++ [call 0x42634 [0,0] 0] ++ waitCalls task 0x10 0x30 0 ++
    [Strip.resetCall] ++ Warp.interruptReleaseCalls 68 ++ [call 0x42188 [0xe66718] 0,Strip.resetCall] ++
    Warp.routeReleaseCalls False ++ Warp.routeRequestCalls True ++ reservedSignalCalls 44 0 0 ++ [call 0x42188 [0xe66720] 0]
